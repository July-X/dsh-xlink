//! 工作台启动容错：看门狗、归因、渐进式插件停用以及故障上报。
//!
//! [`guarded_start`] 把内核启动封装在看门狗中。派生出的子进程必须在
//! [`READY_TIMEOUT_SECS`] 内应答端口，或者自行退出；任一未就绪的结果
//! 都会触发对内核日志末尾的归因分析，然后最多进行三次启动尝试，逐步
//! 切到更保守的接线状态：
//!
//! 1. 按原接线启动（防护之前的行为）；
//! 2. 通过隔离注册表停用日志中指出的可疑插件；
//! 3. 安全模式——停用所有第三方插件。
//!
//! 即便安全模式仍无法启动，流程也会把恢复前的接线和隔离状态复原，并
//! 上报一个不可恢复的 [`Incident`]，附带可操作的下一步提示。每一次已
//! 恢复的结果都会持久化其隔离记录，这样管理面板能为每个可疑项提供
//! 「保持禁用 / 重新启用 / 移除」的决策，而不是留下一个无法启动的工
//! 作台。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::process::read_tail;
use crate::quarantine::{self, QuarantineItem};
use crate::{kernel, plugins, settings};

/// 看门狗在判定启动挂起并杀掉进程前，会等待派生内核应答端口的最长时
/// 间。健康的 `dsh web` 在几秒之内就能监听端口；30 秒足以覆盖慢速磁
/// 盘上的冷缓存，同时不会让失败路径拉得过长。
const READY_TIMEOUT_SECS: u64 = 30;
/// 监听正在启动的子进程时的轮询间隔。
const WATCH_POLL_MILLIS: u64 = 500;
/// 归因时从 `kernel.log` 读取的尾部长度。堆栈信息加 Loader 输出已经
/// 足够完整，完整日志无论如何都保留在磁盘上。
const LOG_TAIL_BYTES: u64 = 32 * 1024;
/// 一次故障上报中报告的可疑项数量上限。启动失败通常牵涉不到几个以
/// 上的插件；这一上限保证故障面板的可读性。
const MAX_SUSPECTS: usize = 8;
/// 单个可疑项的证据摘录上限，按字符计。
const EVIDENCE_MAX_CHARS: usize = 480;

/// 一条日志证据指向的插件或内核组件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suspect {
    /// `plugin` 或 `kernel`。
    pub kind: String,
    /// 插件为商店 id，内核则为内核版本号。
    pub id: String,
    /// 在 UI 中按原样显示的展示名。
    pub name: String,
    /// 归因依据的日志摘录，按需在 UI 中展示。
    pub evidence: String,
}

/// 一次防护式启动的结果，持久化到数据目录中，使故障信息在 Shell 重
/// 启后仍能保留，并被后续消息引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    /// 停用某些插件后的重试是否成功启动了工作台。
    pub recovered: bool,
    /// 工作台当前是否在没有部分第三方插件的状态下运行（仅对已恢复的
    /// 故障有效）。
    pub safe_mode: bool,
    /// 作为面板标题的一小段摘要（简体中文）。
    pub message: String,
    pub suspects: Vec<Suspect>,
    /// 防护模块依次尝试过的操作的可读记录。
    pub attempts: Vec<String>,
    /// 归因时刻的 `kernel.log` 末尾片段。
    pub log_tail: String,
    /// `kernel.log` 的完整路径，供「打开日志」动作使用。
    pub log_path: String,
    /// 未恢复时给出的可操作下一步（简体中文）。
    pub hint: Option<String>,
    /// 自纪元以来的秒数，用于显示。
    pub at: u64,
    /// 用于在面板中选择处理动作的高层归因。
    #[serde(default)]
    pub cause: String,
    /// 当故障来自一个已加载但不健康的页面时，由 Shell 注入的探针捕
    /// 获的前端健康证据。
    #[serde(default)]
    pub health: Option<HealthReport>,
}

/// 由注入的 Shell 探针发送的前端健康信号。命令层会在该结构被写入故
/// 障文件前对每一段字符串做校验并加上长度上限，避免坏页面无限增长故
/// 障文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HealthReport {
    pub kind: String,
    pub message: String,
    pub stack: String,
    pub page_url: String,
}

/// 防护过的 `start_kernel` 命令的结果负载。
#[derive(Debug, Clone, Serialize)]
pub struct StartReport {
    pub port: u16,
    /// 命令返回时工作台是否正在提供服务。
    pub running: bool,
    /// 反映「隔离注册表非空」的便利标志位。
    pub safe_mode: bool,
    pub incident: Option<Incident>,
}

/// 防护模块启动子进程、重新接线以及回滚所需的一切信息。
pub struct GuardDeps<'a> {
    pub data_dir: &'a Path,
    pub settings: &'a settings::Settings,
    /// 已校验的 node 可执行文件，用于派生内核子进程。
    pub node_path: &'a Path,
    /// 解析到的 pnpm 可执行文件，用于在两次尝试之间重新同步 profile。
    pub pnpm_exe: &'a Path,
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn kernel_log_path(data_dir: &Path) -> PathBuf {
    // 取当天的轮转内核日志的末尾；更早日期的文件仍然可以通过
    // `read_log_file` 拿来进行更深入的分析，但启动失败归因总是希望拿
    // 到最新的证据。
    kernel::current_kernel_log_path(data_dir)
}

// --- 看门狗 ---------------------------------------------------------------

enum WatchVerdict {
    Ready,
    Exited(std::process::ExitStatus),
    Hung,
}

/// 监听一个刚刚派生的内核，直至端口应答、进程退出或达到截止时间。被
/// 判为挂起的子进程会在此处被终止（整体进程组，与「关闭工作台」拆解
/// 内核时的方式一致），这样半启动的实例不会残留并稍后冒充一个正在
/// 运行的工作台。
fn watch_child(child: &mut Child, port: u16) -> WatchVerdict {
    let deadline = Instant::now() + Duration::from_secs(READY_TIMEOUT_SECS);
    loop {
        if kernel::port_open(port) {
            return WatchVerdict::Ready;
        }
        if let Ok(Some(status)) = child.try_wait() {
            return WatchVerdict::Exited(status);
        }
        // OS 级 wait 出错（`Err` 分支）意味着无法获知子进程状态；继续轮询
        // 端口，以便健康的启动仍然能在竞速中胜出。
        if Instant::now() >= deadline {
            let _ = kernel::stop(child);
            return WatchVerdict::Hung;
        }
        std::thread::sleep(Duration::from_millis(WATCH_POLL_MILLIS));
    }
}

enum BootVerdict {
    Ready,
    Failed(String),
    Hung,
}

impl BootVerdict {
    fn reason(&self) -> String {
        match self {
            BootVerdict::Ready => String::from("启动成功"),
            BootVerdict::Failed(detail) => detail.clone(),
            BootVerdict::Hung => format!("等待内核就绪超时（{READY_TIMEOUT_SECS} 秒）"),
        }
    }
}

/// 一次受防护的启动尝试：通过常规路径派生进程，再进行监听。返回判定
/// 结果，以及在 `Ready` 路径上的存活子进程（调用方将其注册到应用状
/// 态）；失败路径会消费掉子进程。
fn boot_once(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> (BootVerdict, Option<Child>) {
    match kernel::start_maybe(deps.data_dir, deps.node_path) {
        Ok(None) => (
            // 端口在流程中途已经开始应答（另一个 Shell 实例或残留的孤
            // 儿进程抢到了这个端口）。这里视为已就绪；孤儿进程的回收
            // 由 Shell 启动时的 reap_orphans 负责。
            BootVerdict::Ready,
            None,
        ),
        Ok(Some(mut child)) => match watch_child(&mut child, deps.settings.port) {
            WatchVerdict::Ready => (BootVerdict::Ready, Some(child)),
            WatchVerdict::Exited(status) => {
                let _ = child.wait();
                on_progress(&format!("内核进程在就绪前退出（{status}）"));
                (
                    BootVerdict::Failed(format!("内核进程在就绪前退出（{status}）")),
                    None,
                )
            }
            WatchVerdict::Hung => {
                let _ = child.wait();
                (BootVerdict::Hung, None)
            }
        },
        Err(e) => {
            on_progress(&format!("无法拉起内核进程：{e}"));
            (BootVerdict::Failed(e.to_string()), None)
        }
    }
}

// --- 归因 --------------------------------------------------------------------

/// 可能携带失败原因的行。启动日志把 Loader 的输出（形如 «loading
/// plugin …»）与真正错误混合在一起；只匹配错误形态的行，才能把那些
/// 看起来热闹但无关的插件排除在可疑列表之外，这正是自动停用可以无人
/// 值守运行的安全前提。
fn is_error_line(line: &str) -> bool {
    const MARKERS: [&str; 12] = [
        "Error",
        "error",
        "ERR_",
        "Cannot",
        "cannot",
        "throw",
        "Throw",
        "Failed",
        "failed",
        "Uncaught",
        "uncaught",
        "TypeError",
    ];
    if MARKERS.iter().any(|marker| line.contains(marker)) {
        return true;
    }
    // HTTP 4xx / 5xx 访问行同样属于错误形态：前端无法加载自己打包的文
    // 件，效果上等同于模块缺失。
    let lower = line.to_ascii_lowercase();
    (lower.contains(" 4") || lower.contains(" 5"))
        && (lower.contains("http")
            || lower.contains("get ")
            || lower.contains("post ")
            || lower.contains("put ")
            || lower.contains("delete "))
}

/// 把命中的行与其前后各一行的上下文拼接起来，并对长度设上限。
fn excerpt(lines: &[&str], idx: usize) -> String {
    let start = idx.saturating_sub(1);
    let end = (idx + 2).min(lines.len());
    let joined = lines[start..end].join("\n");
    if joined.chars().count() <= EVIDENCE_MAX_CHARS {
        return joined;
    }
    let truncated: String = joined.chars().take(EVIDENCE_MAX_CHARS).collect();
    format!("{truncated}…")
}

/// 根据日志末尾将一次启动失败归因到已安装的插件（或内核自身）。插件
/// 候选只匹配锚定的形态——它们的 Shell 物化路径段 `plugins/<id>`，
/// 加上 `/` 前缀的包名路径段，或者带引号的包名——因为纯子串匹配会让
/// 任何短包名只要碰巧出现在堆栈里的任何位置就被当成可疑项。
///
/// 设计上保持保守：没有任何候选命中时返回空列表，空列表会路由到安
/// 全模式，而不是胡乱指认某个插件。
pub fn attribute(
    log_tail: &str,
    store_items: &[plugins::StoreItem],
    kernel_label: &str,
) -> Vec<Suspect> {
    let lines: Vec<&str> = log_tail.lines().collect();
    let mut suspects: Vec<Suspect> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for item in store_items {
        if suspects.len() >= MAX_SUSPECTS {
            return suspects;
        }
        // `plugins/<id>` 覆盖 link 模式下的内核插件目录；`/name` 同样能匹
        // 配 copy 模式下 profile 路径中的 `/name/...` 形式；带引号的形
        // 式则能命中 `Cannot find package 'x'` / `Cannot find module 'x'`。
        let needles = [
            format!("plugins/{}", item.id),
            format!("/{}", item.name),
            format!("'{}'", item.name),
            format!("\"{}\"", item.name),
        ];
        let Some(idx) = lines.iter().position(|line| {
            is_error_line(line) && needles.iter().any(|needle| line.contains(needle))
        }) else {
            continue;
        };
        if seen.insert(item.id.clone()) {
            suspects.push(Suspect {
                kind: String::from("plugin"),
                id: item.id.clone(),
                name: item.name.clone(),
                evidence: excerpt(&lines, idx),
            });
        }
    }

    if suspects.is_empty() {
        // 内核安装损坏信号：任何指向 `@deepseek-ai/dsh-*` 包，或者内核
        // 自有 client-module loader 的错误形态行。该命名空间被内核发
        // 行版保留（内建的 `client-ui-*`、`base`、`web-app`、`headless`
        // 等），所以一旦命中，定义上就不可能是社区插件。我们还接受一
        // 小组内核 Loader 在其预打包 chunk 表过时（启动时暴露的 build-
        // time externals drift）时输出的特定短语：这些短语在不同内核版
        // 本间稳定且很少变动，因此保守的规则是「命中其中任何一条」，
        // 而非「只匹配我们今天认识的那几条」。
        let Some(idx) = lines.iter().position(|line| is_kernel_evidence_line(line)) else {
            return suspects;
        };
        suspects.push(Suspect {
            kind: String::from("kernel"),
            id: kernel_label.to_string(),
            name: format!("dsh 内核 {kernel_label}"),
            evidence: excerpt(&lines, idx),
        });
    }
    suspects
}

/// 判断 `line` 是否既呈现错误形态，又指向内核内部组件（内核的包命
/// 名空间，或已知的 client-module loader 短语）。保守地：调用方必须
/// 已经先按 `is_error_line` 的形态匹配确认这一行确实是错误，所以本
/// 辅助函数只是在其之上叠加一个「是否归我们管？」的问题。
fn is_kernel_evidence_line(line: &str) -> bool {
    if !is_error_line(line) {
        return false;
    }
    // 任何提到内核命名空间包名的错误。边界判断很关键：
    // `@deepseek-ai/dsh/` 覆盖根入口文件（例如
    // `@deepseek-ai/dsh/lib/bin.js`），而 `@deepseek-ai/dsh-` 覆盖
    // 内核随附的所有内建 client module（例如
    // `@deepseek-ai/dsh-client-ui-theme`）。以非字母数字的分隔符锚定
    // 可以避免一个名叫 `@scope/dsh-foo` 的社区插件仅凭 `dsh` 子串被
    // 误当成内核包。
    if has_kernel_package_ref(line) {
        return true;
    }
    // 稳定的内核 Loader 短语。这些是内核 client-module loader 在预打
    // 包 chunk 表缺少某条目时输出的特定字符串；识别它们使得故障面板
    // 能把空白 / 加载失败的报告路由到对应的内核版本，而不是留下一个
    // 空洞的「暂未能归因」。
    const KERNEL_LOADER_PHRASES: [&str; 6] = [
        "client-modules",
        "build-time externals drift",
        "missed the module table",
        "platform seed word",
        "not a materialized module",
        "no registered package factory",
    ];
    KERNEL_LOADER_PHRASES
        .iter()
        .any(|phrase| line.contains(phrase))
}

/// 当且仅当 `line` 引用了内核自己的包命名空间（`@deepseek-ai/dsh` 后
/// 接非字母数字边界）时返回 true。不应匹配 `@deepseek-ai/dshfoo`——
/// 那是（假设存在的）名字里碰巧含有 `dsh` 的社区包，并非内核。
fn has_kernel_package_ref(line: &str) -> bool {
    const NEEDLE: &str = "@deepseek-ai/dsh";
    let mut start = 0;
    while let Some(idx) = line[start..].find(NEEDLE) {
        let after = start + idx + NEEDLE.len();
        // 手写一个「下一个字符（如果有）是否为非字母数字边界？」的判
        // 断——`Option::is_none_or` 是 1.77 之后才有的，`Cargo.toml`
        // 中的 MSRV 门禁会拒绝在此调用该方法。
        let boundary_ok = match line[after..].chars().next() {
            None => true,
            Some(c) => !c.is_ascii_alphanumeric(),
        };
        if boundary_ok {
            return true;
        }
        start = after;
    }
    false
}

fn suspect_records(suspects: &[Suspect], reason: String) -> Vec<QuarantineItem> {
    suspects
        .iter()
        .filter(|s| s.kind == "plugin")
        .map(|s| QuarantineItem {
            id: s.id.clone(),
            name: s.name.clone(),
            reason: reason.clone(),
            evidence: s.evidence.clone(),
            at: epoch_secs(),
        })
        .collect()
}

// --- 接线辅助 -----------------------------------------------------------------

/// 首次尝试的接线修复，思路与防护之前的静默通过一致：失败落到插件
/// 商店告警中，而不是阻塞启动。
fn sync_wiring_record_warning(deps: &GuardDeps<'_>, on_progress: &mut dyn FnMut(&str)) {
    match plugins::ensure_wiring(deps.data_dir, deps.settings, deps.pnpm_exe, on_progress) {
        Ok(_) => plugins::set_store_warning(deps.data_dir, None),
        Err(e) => plugins::set_store_warning(deps.data_dir, Some(e.to_string())),
    }
}

/// 在两次尝试之间重新同步 profile，无论结果如何都会把过程追加到故
/// 障轨迹中。
fn refresh_wiring(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
    trail: &mut Vec<String>,
) {
    match plugins::ensure_wiring(deps.data_dir, deps.settings, deps.pnpm_exe, on_progress) {
        Ok((count, changed)) => trail.push(format!(
            "已按屏蔽清单重新接线（{count} 个插件接入，清单变更：{changed}）"
        )),
        Err(e) => trail.push(format!("重新接线失败：{e}")),
    }
}

fn log_tail(deps: &GuardDeps<'_>) -> String {
    read_tail(&kernel_log_path(deps.data_dir), LOG_TAIL_BYTES)
}

// --- 故障持久化 ---------------------------------------------------------------

fn incident_path(data_dir: &Path) -> PathBuf {
    data_dir.join("last-incident.json")
}

/// 持久化故障信息，使其在 Shell 重启后仍然存在；尽力而为地写，因为
/// 写入失败不能掩盖用户正在等待的启动结果。
fn save_incident(data_dir: &Path, incident: &Incident) {
    if let Ok(text) = serde_json::to_string_pretty(incident) {
        let _ = std::fs::write(incident_path(data_dir), text + "\n");
    }
}

/// 在一次干净、正常的启动后清除已记录的故障——否则这份陈旧的报告
/// 会与刚刚健康启动的工作台相矛盾。
fn clear_incident(data_dir: &Path) {
    let _ = std::fs::remove_file(incident_path(data_dir));
}

/// 读取最近一次记录的故障（供展示历史的命令使用）。
pub fn load_incident(data_dir: &Path) -> Option<Incident> {
    let text = std::fs::read_to_string(incident_path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

// --- 编排 --------------------------------------------------------------------

/// 在启动防护下启动当前活动的内核。向 UI 返回报告，并在成功路径上
/// 一起返回存活中的子进程（调用方把它注册到应用状态并记录其 pid）。
pub fn guarded_start(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> (StartReport, Option<Child>) {
    let port = deps.settings.port;

    if kernel::port_open(port) {
        // 幂等启动：有东西已经在监听这个端口。此处不必再就历史的隔离
        // 记录唠叨，概览页的横幅负责展示。
        return (
            StartReport {
                port,
                running: true,
                safe_mode: false,
                incident: None,
            },
            None,
        );
    }

    sync_wiring_record_warning(deps, on_progress);

    let store_items = plugins::load_store(deps.data_dir).items;
    let kernel_label = kernel::read_active(deps.data_dir).unwrap_or_default();
    let prior_quarantine = quarantine::load(deps.data_dir);
    let manifest_snapshot =
        plugins::snapshot_profile_manifest_text(deps.data_dir, &deps.settings.profile);
    let mut trail: Vec<String> = Vec::new();

    // 第 1 次尝试：完全按既有接线启动。
    on_progress("正在启动工作台…");
    let (verdict, child) = boot_once(deps, on_progress);
    // 没有子进程但得到 `Ready` 判定，意味着流程中途已有别的东西开始
    // 应答端口；这也属于一个正在运行的工作台。
    if matches!(verdict, BootVerdict::Ready) {
        clear_incident(deps.data_dir);
        return (
            StartReport {
                port,
                running: true,
                safe_mode: !prior_quarantine.items.is_empty(),
                incident: None,
            },
            child,
        );
    }
    trail.push(format!("常规启动失败：{}", verdict.reason()));
    let tail = log_tail(deps);
    let mut suspects = attribute(&tail, &store_items, &kernel_label);

    // 第 2 次尝试：停用归因得到的可疑插件后再试。当归因没有结果时跳过
    // ——凭空猜测只会误伤无辜插件。
    if !suspects.is_empty() {
        on_progress("检测到疑似引发故障的插件，正在停用后重试…");
        let records = suspect_records(
            &suspects,
            String::from("内核启动失败，错误日志指向该插件，已自动停用"),
        );
        if quarantine::add_all(deps.data_dir, &records).is_ok() {
            refresh_wiring(deps, on_progress, &mut trail);
            let (verdict2, child2) = boot_once(deps, on_progress);
            if matches!(verdict2, BootVerdict::Ready) {
                trail.push("停用疑似插件后启动成功".to_string());
                let incident = Incident {
                    recovered: true,
                    safe_mode: true,
                    message: String::from(
                        "工作台已在停用以下插件后成功启动。请查看错误原因，选择移除或保持禁用；确认插件已修复后可重新启用。",
                    ),
                    suspects,
                    attempts: trail,
                    log_tail: tail,
                    log_path: kernel_log_path(deps.data_dir).display().to_string(),
                    hint: None,
                    at: epoch_secs(),
                    cause: String::from("plugin"),
                    health: None,
                };
                save_incident(deps.data_dir, &incident);
                return (
                    StartReport {
                        port,
                        running: true,
                        safe_mode: true,
                        incident: Some(incident),
                    },
                    child2,
                );
            }
            trail.push(format!("停用疑似插件后仍失败：{}", verdict2.reason()));
            suspects.extend(attribute(&log_tail(deps), &store_items, &kernel_label));
        } else {
            trail.push(String::from("写入隔离记录失败，跳过定向停用"));
        }
    }

    // 第 3 次尝试：安全模式。如果没有第三方插件可停用——bare-profile 失
    // 败通常是内核或环境的问题。
    if !store_items.is_empty() {
        on_progress("仍未启动成功，正在进入安全模式（停用全部第三方插件）后重试…");
        let already: HashSet<String> = quarantined_ids_now(deps.data_dir);
        let rest: Vec<Suspect> = store_items
            .iter()
            .filter(|item| !already.contains(&item.id))
            .map(|item| Suspect {
                kind: String::from("plugin"),
                id: item.id.clone(),
                name: item.name.clone(),
                evidence: String::new(),
            })
            .collect();
        let records = suspect_records(
            &rest,
            String::from("无法定位具体引发故障的插件，安全模式已停用全部第三方插件"),
        );
        if quarantine::add_all(deps.data_dir, &records).is_ok() {
            refresh_wiring(deps, on_progress, &mut trail);
            let (verdict3, child3) = boot_once(deps, on_progress);
            if matches!(verdict3, BootVerdict::Ready) {
                trail.push("安全模式（全部第三方插件停用）下启动成功".to_string());
                // 把每个插件都报告为可疑项：用户必须按插件决定是移除还
                // 是保持禁用，那些带有真正日志证据的会一起带上摘录。
                let all_suspects: Vec<Suspect> = store_items
                    .iter()
                    .map(|item| Suspect {
                        kind: String::from("plugin"),
                        id: item.id.clone(),
                        name: item.name.clone(),
                        evidence: suspects
                            .iter()
                            .find(|s| s.id == item.id)
                            .map(|s| s.evidence.clone())
                            .unwrap_or_default(),
                    })
                    .take(MAX_SUSPECTS)
                    .collect();
                let incident = Incident {
                    recovered: true,
                    safe_mode: true,
                    message: String::from(
                        "工作台仅在不加载任何第三方插件时才能启动，已将全部插件临时停用。请逐个查看并决定移除或恢复。",
                    ),
                    suspects: all_suspects,
                    attempts: trail,
                    log_tail: tail,
                    log_path: kernel_log_path(deps.data_dir).display().to_string(),
                    hint: None,
                    at: epoch_secs(),
                    cause: String::from("plugin"),
                    health: None,
                };
                save_incident(deps.data_dir, &incident);
                return (
                    StartReport {
                        port,
                        running: true,
                        safe_mode: true,
                        incident: Some(incident),
                    },
                    child3,
                );
            }
            trail.push(format!("安全模式下仍失败：{}", verdict3.reason()));
        } else {
            trail.push(String::from("写入隔离记录失败，跳过安全模式"));
        }
    }

    // 全部尝试都失败：这不是插件引起的。撤销防护期间做过的所有改动，
    // 这样在防护之外采取的修复（重装内核版本、替换损坏的磁盘状态）不
    // 会被半应用的隔离或接线状态所遮蔽。
    on_progress("多次尝试后仍无法启动，正在恢复原有配置…");
    trail.push(String::from("已放弃自动修复，恢复原有接线与隔离状态"));
    let _ = quarantine::save(deps.data_dir, &prior_quarantine);
    if let Err(e) = plugins::restore_profile_manifest(
        deps.data_dir,
        deps.settings,
        deps.pnpm_exe,
        manifest_snapshot.as_deref(),
        on_progress,
    ) {
        trail.push(format!("恢复原接线失败：{e}"));
    }
    let kernel_suspected = suspects.iter().any(|s| s.kind == "kernel");
    let multiple_versions = kernel::list_installed(deps.data_dir).len() > 1;
    let mut hint = String::from("此次失败与第三方插件无关。请通过「打开日志」查看完整内核日志；也可在「内核版本」页删除当前版本后重新安装。");
    if kernel_suspected || multiple_versions {
        hint = format!("也可先尝试在「内核版本」页切换到其他已安装版本。{hint}");
    }
    let incident = Incident {
        recovered: false,
        safe_mode: false,
        message: String::from("多次尝试后工作台仍无法启动，已恢复原有插件配置。"),
        suspects: dedup_suspects(suspects),
        attempts: trail,
        log_tail: tail,
        log_path: kernel_log_path(deps.data_dir).display().to_string(),
        hint: Some(hint),
        at: epoch_secs(),
        cause: if kernel_suspected {
            String::from("kernel")
        } else {
            String::from("unknown")
        },
        health: None,
    };
    save_incident(deps.data_dir, &incident);
    (
        StartReport {
            port,
            running: false,
            safe_mode: false,
            incident: Some(incident),
        },
        None,
    )
}

fn quarantined_ids_now(data_dir: &Path) -> HashSet<String> {
    quarantine::ids(data_dir)
}

/// 按 id 合并跨次尝试的归因结果，保留先出现的证据。
fn dedup_suspects(suspects: Vec<Suspect>) -> Vec<Suspect> {
    let mut out: Vec<Suspect> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for suspect in suspects {
        if seen.insert(suspect.id.clone()) {
            out.push(suspect);
        }
    }
    out
}

fn runtime_cause(suspects: &[Suspect]) -> &'static str {
    if suspects.iter().any(|s| s.kind == "plugin") {
        "plugin"
    } else if suspects.iter().any(|s| s.kind == "kernel") {
        "kernel"
    } else {
        "unknown"
    }
}

/// 把前端健康证据放进与内核日志相同的错误形态字符串流。这样既有的保
/// 守路径/名称匹配器就能对一份从未落到 `kernel.log` 的客户端堆栈进
/// 行归因。
fn runtime_evidence(report: &HealthReport, kernel_tail: &str) -> String {
    let mut lines = Vec::new();
    if !report.kind.trim().is_empty() {
        lines.push(format!("Error: 工作台自检类型：{}", report.kind.trim()));
    }
    if !report.message.trim().is_empty() {
        lines.push(format!("Error: 工作台前端错误：{}", report.message.trim()));
    }
    for line in report
        .stack
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        lines.push(format!("Error: 前端堆栈：{line}"));
    }
    if !report.page_url.trim().is_empty() {
        lines.push(format!("Error: 工作台页面地址：{}", report.page_url.trim()));
    }
    if !kernel_tail.is_empty() {
        lines.push(kernel_tail.to_string());
    }
    lines.join("\n")
}

fn runtime_attempt(report: &HealthReport) -> String {
    let kind = report.kind.trim();
    let message = report.message.trim();
    match (kind.is_empty(), message.is_empty()) {
        (true, true) => String::from("工作台健康探针报告：页面异常"),
        (true, false) => format!("工作台健康探针报告：{message}"),
        (_, true) => format!("工作台健康探针报告：{kind}"),
        (false, false) => format!("工作台健康探针报告：{kind}：{message}"),
    }
}

/// 为空白页面场景构建一份「软信号」可疑项列表：当页面渲染为空白但内
/// 核仍在响应时，插件代码是最常见的沉默元凶（同步的初始化错误、把
/// 一切藏起来的 CSS bug、卡住的异步 Loader）。我们把每个已安装的第
/// 三方插件都列出为软可疑项，但不附具体的日志证据，以便用户采取行
/// 动；我们绝不在软信号下自动隔离——只有保守的 `attribute` 证据才
/// 能驱动这件事。
fn soft_attribute_blank(store_items: &[plugins::StoreItem], report: &HealthReport) -> Vec<Suspect> {
    if report.kind.trim() != "blank" || store_items.is_empty() {
        return Vec::new();
    }
    store_items
        .iter()
        .take(MAX_SUSPECTS)
        .map(|item| Suspect {
            kind: String::from("plugin"),
            id: item.id.clone(),
            name: item.name.clone(),
            evidence: String::from(
                "工作台页面加载完成后仍为空白，未发现可见内容。内核仍在响应 HTTP 请求，但页面未渲染。",
            ),
        })
        .collect()
}

/// 启发式判断：当日志中出现 HTTP 4xx/5xx 访问行时，前端很可能没能
/// 加载到某个资源——这看起来就像插件（link 模式接线指向了错误的路径）
/// 的行为，值得作为软信号展示出来。
fn log_has_http_failure(tail: &str) -> bool {
    tail.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        (lower.contains(" 4") || lower.contains(" 5"))
            && (lower.contains("http")
                || lower.contains("get ")
                || lower.contains("post ")
                || lower.contains("put ")
                || lower.contains("delete "))
    })
}

/// 在不重启、不改动运行中的内核的前提下诊断一份工作台健康报告。
/// 插件证据会被临时隔离，这样下一次重启是安全的；至于具体是保持、
/// 恢复还是移除插件，仍然由用户在故障面板里决定。
pub fn diagnose_runtime(data_dir: &Path, report: HealthReport) -> Incident {
    let now = epoch_secs();
    let attempt = runtime_attempt(&report);
    if let Some(existing) = load_incident(data_dir) {
        if existing.health.as_ref() == Some(&report) && now.saturating_sub(existing.at) < 60 {
            return existing;
        }
    }

    let tail = log_tail(&GuardDeps {
        data_dir,
        settings: &settings::Settings::default(),
        node_path: Path::new(""),
        pnpm_exe: Path::new(""),
    });
    let store_items = plugins::load_store(data_dir).items;
    let kernel_label = kernel::read_active(data_dir).unwrap_or_default();
    // 强证据：锚定到插件或内核的错误行。
    let mut suspects = attribute(
        &runtime_evidence(&report, &tail),
        &store_items,
        &kernel_label,
    );
    let soft_only = suspects.is_empty();
    // 软证据：插件已安装时页面空白，或者内核日志中出现 HTTP 失败。我
    // 们在这些场景下把每个已安装的插件都列为软可疑项，方便用户拿到
    // 一个具体的清单去操作，但我们不会自动隔离——只有 `attribute` 的
    // 证据强到足以在无人值守的情况下写隔离注册表。
    if soft_only {
        let blank_soft = soft_attribute_blank(&store_items, &report);
        let http_soft =
            log_has_http_failure(&tail) && !store_items.is_empty() && report.kind.trim() == "blank";
        if !blank_soft.is_empty() || http_soft {
            let mut soft = blank_soft;
            if http_soft {
                let extra: Vec<Suspect> = store_items
                    .iter()
                    .take(MAX_SUSPECTS.saturating_sub(soft.len()))
                    .map(|item| Suspect {
                        kind: String::from("plugin"),
                        id: item.id.clone(),
                        name: item.name.clone(),
                        evidence: String::from(
                            "内核日志出现 HTTP 4xx/5xx 响应，可能是插件静态资源加载失败",
                        ),
                    })
                    .collect();
                soft.extend(extra);
            }
            suspects = soft;
        }
    }
    let plugin_suspects: Vec<Suspect> = suspects
        .iter()
        .filter(|s| s.kind == "plugin")
        .cloned()
        .collect();
    let cause = runtime_cause(&suspects);
    let mut attempts = vec![attempt];

    let (message, hint) = match cause {
        "plugin" if !soft_only => {
            // 强证据路径：自动隔离可疑项。
            let names = plugin_suspects
                .iter()
                .map(|suspect| suspect.name.as_str())
                .collect::<Vec<_>>()
                .join("、");
            let records = suspect_records(
                &plugin_suspects,
                String::from("工作台运行异常，前端或内核错误证据指向该插件，已临时隔离"),
            );
            let isolated = quarantine::add_all(data_dir, &records).is_ok();
            if isolated {
                attempts.push(format!("已临时隔离疑似插件：{names}，重启后验证"));
            } else {
                attempts.push(String::from("写入插件隔离记录失败，请在插件页手动处理"));
            }
            if isolated {
                (
                    format!(
                        "工作台页面异常，错误证据指向插件「{names}」，已临时隔离；请决定如何修复。"
                    ),
                    String::from("请到插件页选择保持禁用、重新启用或移除，然后重启工作台验证。"),
                )
            } else {
                (
                    format!("工作台页面异常，错误证据指向插件「{names}」，但自动隔离失败。"),
                    String::from("请先到插件页手动禁用或移除该插件，再重启工作台验证。"),
                )
            }
        }
        "plugin" => {
            // 软证据路径：已安装插件但页面空白或出现 HTTP 失败、且无具
            // 体日志证据。不自动隔离。
            let names = plugin_suspects
                .iter()
                .map(|suspect| suspect.name.as_str())
                .collect::<Vec<_>>()
                .join("、");
            attempts.push(format!(
                "未发现具体的内核错误堆栈，但已安装的第三方插件无法排除：{names}（已列出但未自动停用）"
            ));
            attempts.push(String::from("软信号不会自动隔离，请按下方提示人工确认。"));
            (
                String::from("工作台页面加载后仍为空白，且已安装第三方插件。无法定位到具体插件的日志证据，请人工排查。"),
                String::from(
                    "建议步骤：① 打开「内核日志」查看是否有 4xx/5xx 或资源加载错误；② 在「插件」页临时停用全部第三方插件后重启工作台；③ 若停用后正常，逐个重新启用定位问题插件。",
                ),
            )
        }
        "kernel" => {
            attempts.push(String::from("错误证据指向内核组件，未自动修改插件"));
            (
                String::from("工作台页面异常，错误证据指向当前内核组件，未自动修改插件。"),
                String::from("请先查看完整内核日志，再到内核版本页切换其他版本；仍失败时删除当前版本后重新安装。"),
            )
        }
        _ => {
            // 没有安装插件，同时也没有内核证据：纯粹属于环境 / 未知这
            // 一类。仍然要给出一个比「我们不知道」更具体的下一步——
            // 内核仍在响应，所以只是页面为空。引导用户去日志（CSS/JS
            // 网络失败会出现在那里）以及去切换版本。
            let http_hint = log_has_http_failure(&tail);
            if http_hint {
                attempts.push(String::from(
                    "内核日志出现 HTTP 4xx/5xx，未定位到具体插件或内核组件",
                ));
                (
                    String::from("工作台页面异常；内核仍在响应但日志含 HTTP 4xx/5xx，未定位到具体插件或内核组件。"),
                    String::from(
                        "请打开「内核日志」查看 4xx/5xx 详情；先尝试在「内核版本」页切换到其他已安装版本，仍失败时删除当前版本后重新安装。",
                    ),
                )
            } else {
                attempts.push(String::from("未发现足够的插件或内核证据，暂不作强归因"));
                (
                    String::from("工作台页面异常，但暂未找到足够证据区分插件和内核。"),
                    String::from(
                        "请打开日志并重试；若持续发生，再到内核版本页切换其他版本或重新安装当前版本。",
                    ),
                )
            }
        }
    };

    let incident = Incident {
        recovered: false,
        safe_mode: false,
        message,
        suspects,
        attempts,
        log_tail: tail,
        log_path: kernel_log_path(data_dir).display().to_string(),
        hint: Some(hint),
        at: now,
        cause: cause.to_string(),
        health: Some(report),
    };
    save_incident(data_dir, &incident);
    incident
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_item(id: &str, name: &str) -> plugins::StoreItem {
        plugins::StoreItem {
            id: id.to_string(),
            name: name.to_string(),
            origin: String::from("npm"),
            source: name.to_string(),
            installed_version: String::from("1.0.0"),
            latest_version: None,
            mode: String::from("link"),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        }
    }

    #[test]
    fn attributes_link_layout_by_materialized_path() {
        // Link 模式的崩溃解析路径会经过内核插件目录，该目录的路径段里携带的
        // 是商店 id（`/` → `__`），而不是 npm 包名。
        let tail = "node:internal/modules/esm/resolve\n\
                    Error: Cannot find module '/Users/u/.dsh/desktop/kernels/0.1.1/plugins/@scope__pkg/lib/index.js'\n";
        let items = vec![store_item("@scope__pkg", "@scope/pkg")];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "plugin");
        assert_eq!(suspects[0].id, "@scope__pkg");
        assert!(suspects[0].evidence.contains("Cannot find module"));
    }

    #[test]
    fn attributes_copy_layout_by_package_name() {
        // Copy 模式的崩溃解析路径经过 profile 的 node_modules，那里携带的是包名
        // 而不是商店 id。
        let tail = "Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@scope/pkg' imported from /Users/u/.dsh/profiles/web/node_modules/@scope/pkg/lib/index.js\n";
        let items = vec![store_item("@scope__pkg", "@scope/pkg")];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].name, "@scope/pkg");
    }

    #[test]
    fn ignores_chatty_but_innocent_plugins() {
        // Loader 的进度行会提到很多插件；它们并不呈现错误形态，所以这些吵闹但
        // 无辜的 Loader 必须留在可疑列表之外，即便失败的包就出现在它
        // 们旁边。
        let tail = "[loader] loading plugin alpha\n\
                    [loader] loading plugin beta\n\
                    Error: Cannot find package 'beta' imported from lib/index.js\n";
        let items = vec![store_item("alpha", "alpha"), store_item("beta", "beta")];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].id, "beta");
    }

    #[test]
    fn bare_substring_occurrences_do_not_blame_short_names() {
        // 短包名在一条无关错误行里不带引号出现，不能算作证据；只有锚定形态
        // 才算数。
        let tail = "Error: something failed while preparing plugins\n";
        let items = vec![store_item("p", "p"), store_item("plug_x", "plug")];
        assert!(attribute(tail, &items, "0.1.1").is_empty());
    }

    #[test]
    fn no_match_yields_empty_list() {
        let tail = "Error: EADDRINUSE: address already in use 127.0.0.1:3090\n";
        let items = vec![store_item("p", "p")];
        assert!(attribute(tail, &items, "0.1.1").is_empty());
    }

    #[test]
    fn kernel_fallback_when_no_plugin_matches() {
        let tail = "Error: Cannot find module '@deepseek-ai/dsh/lib/bin.js'\n";
        let items = vec![store_item("p", "p")];
        let suspects = attribute(tail, &items, "0.1.2");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "kernel");
        assert_eq!(suspects[0].id, "0.1.2");
    }

    #[test]
    fn kernel_fallback_for_client_module_externals_drift() {
        // 内核的 client-module loader 在其预打包 chunk 表过时时会输出这种
        // 形态。日志中的包名是一个*内建*的内核 client module，而非社
        // 区插件（这里插件商店为空），因此保守的匹配器必须把这个归到
        // 对应的内核版本，而不是返回「暂未能归因」。
        let tail = "Failed to load plugins\n\
                    failed to import loader entry 84ed0f28 \
                    (@deepseek-ai/dsh-client-ui-theme): client-modules: \
                    require(\"@deepseek-ai/dsh-client-runtime/client\") \
                    missed the module table — not a platform seed word, \
                    not a materialized module, and no registered package \
                    factory (a build-time externals drift, or a dynamic \
                    dependency that did not arrive)\n";
        let items = vec![store_item("p", "p")];
        let suspects = attribute(tail, &items, "0.1.1-rc.2");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "kernel");
        assert_eq!(suspects[0].id, "0.1.1-rc.2");
        assert!(suspects[0].evidence.contains("client-modules"));
        assert!(suspects[0].evidence.contains("build-time externals drift"));
    }

    #[test]
    fn kernel_fallback_for_any_dsh_dash_package_in_error_line() {
        // 任何提到 `@deepseek-ai/dsh-*` 包的错误行——即便不带 Loader 的特定
        // 短语——都必须归到内核，因为该命名空间专属于内核发行版。社区插
        // 件使用不同的 scope（例如 `@scope/plugin-name`），不会冲突。
        let tail = "Error: failed to load chunk for @deepseek-ai/dsh-web-app/entry\n";
        let items: Vec<plugins::StoreItem> = Vec::new();
        let suspects = attribute(tail, &items, "0.1.0");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "kernel");
    }

    #[test]
    fn kernel_fallback_ignores_community_plugin_with_dsh_in_name() {
        // 原则上用户可以把社区插件命名为含 "dsh" 的名字（例如
        // `@scope/dsh-foo`）。匹配器仍然必须根据*命名空间*
        // `@deepseek-ai/dsh-` 而非 `dsh` 这个子串来判断，所以这种插件
        // 仍然会落到插件分支。
        let tail = "Error: Cannot find package '@scope/dsh-foo' \
                    imported from plugins/@scope__dsh-foo/lib/index.js\n";
        let items = vec![store_item("@scope__dsh-foo", "@scope/dsh-foo")];
        let suspects = attribute(tail, &items, "0.1.0");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "plugin");
        assert_eq!(suspects[0].id, "@scope__dsh-foo");
    }

    #[test]
    fn is_kernel_evidence_line_recognises_loader_phrases() {
        // 内核 client-module loader 在其 chunk 表过时时会输出的确切短语。其
        // 中任何一条单独出现，都必须在错误行上触发内核分支。
        assert!(is_kernel_evidence_line(
            "Error: client-modules: require('x') missed the module table"
        ));
        assert!(is_kernel_evidence_line(
            "Error: a build-time externals drift was detected"
        ));
        assert!(is_kernel_evidence_line(
            "Error: missed the module table for chunk abc"
        ));
        assert!(is_kernel_evidence_line(
            "Error: not a platform seed word: foo"
        ));
        assert!(is_kernel_evidence_line(
            "Error: not a materialized module: bar"
        ));
        assert!(is_kernel_evidence_line(
            "Error: no registered package factory for baz"
        ));
        // 命名空间规则覆盖更广范围的错误。
        assert!(is_kernel_evidence_line(
            "Error: failed to load @deepseek-ai/dsh-base/lib/index.js"
        ));
        // 非错误的行永远不算内核证据，即便它们提到了某个内核 Loader
        // 短语（例如进度行 "client-modules: pre-bundling 12 chunks" 也
        // 不能被标记）。
        assert!(!is_kernel_evidence_line(
            "client-modules: pre-bundling 12 chunks"
        ));
        // 既不提命名空间也不提 Loader 短语的行，也不算内核证据。
        assert!(!is_kernel_evidence_line("Error: EADDRINUSE: port in use"));
    }

    #[test]
    fn has_kernel_package_ref_anchors_on_boundary() {
        // 内核命名空间的匹配：内核在日志中实际产生的各种形式（根包、子包、
        // 带引号、在括号里、名字后紧跟一个闭括号）。
        assert!(has_kernel_package_ref("@deepseek-ai/dsh/lib/bin.js"));
        assert!(has_kernel_package_ref(
            "(@deepseek-ai/dsh-client-ui-theme):"
        ));
        assert!(has_kernel_package_ref(
            "require(\"@deepseek-ai/dsh-client-runtime/client\")"
        ));
        assert!(has_kernel_package_ref("'@deepseek-ai/dsh-base'"));
        // 边界不匹配：名字碰巧含有 `dsh`（或 `dshfoo`）的社区插件不应
        // 该被标成内核包。
        assert!(!has_kernel_package_ref("@scope/dsh-foo"));
        assert!(!has_kernel_package_ref("@scope/dshfoo"));
        // 没有 scope 的裸子串也算不匹配（根本没有 `@deepseek-ai/dsh`）。
        assert!(!has_kernel_package_ref("dsh-foo"));
    }

    #[test]
    fn runtime_cause_prefers_plugin_evidence_and_defaults_to_unknown() {
        assert_eq!(runtime_cause(&[]), "unknown");
        assert_eq!(
            runtime_cause(&[
                Suspect {
                    kind: "kernel".into(),
                    id: "1".into(),
                    name: "dsh".into(),
                    evidence: String::new(),
                },
                Suspect {
                    kind: "plugin".into(),
                    id: "p".into(),
                    name: "plugin".into(),
                    evidence: String::new(),
                }
            ]),
            "plugin"
        );
    }

    #[test]
    fn soft_attribute_blank_lists_installed_plugins() {
        // 空白页面报告且装有两个插件：两者都作为软可疑项呈现，让用户得到一
        // 个可以操作的具体清单，即便日志里没有任何证据专门指向其中任
        // 何一个。
        let report = HealthReport {
            kind: "blank".into(),
            message: "工作台页面加载完成后仍为空白".into(),
            stack: String::new(),
            page_url: "http://127.0.0.1:3090/".into(),
        };
        let items = vec![store_item("alpha", "alpha"), store_item("beta", "beta")];
        let soft = soft_attribute_blank(&items, &report);
        assert_eq!(soft.len(), 2);
        assert!(soft.iter().all(|s| s.kind == "plugin"));
        let names: Vec<&str> = soft.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"alpha") && names.contains(&"beta"));
    }

    #[test]
    fn soft_attribute_blank_no_plugins_returns_empty() {
        // 没有安装插件：空白页面不是插件信号，函数必须返回空列表，使归因
        // 留在内核 / 未知分支，并配有正确的下一步。
        let report = HealthReport {
            kind: "blank".into(),
            message: String::new(),
            stack: String::new(),
            page_url: String::new(),
        };
        let items: Vec<plugins::StoreItem> = Vec::new();
        assert!(soft_attribute_blank(&items, &report).is_empty());
    }

    #[test]
    fn soft_attribute_blank_ignores_non_blank_kinds() {
        // runtime-error / unhandled-rejection 不应走软信号路径：它们带
        // 有真正的堆栈，应该由强证据分支负责。
        let report = HealthReport {
            kind: "runtime-error".into(),
            message: String::new(),
            stack: String::new(),
            page_url: String::new(),
        };
        let items = vec![store_item("alpha", "alpha")];
        assert!(soft_attribute_blank(&items, &report).is_empty());
    }

    #[test]
    fn log_has_http_failure_detects_5xx_and_4xx_access_lines() {
        assert!(log_has_http_failure(
            "2024-01-15T12:00:00 GET /plugins/x/main.js 500 Internal Server Error"
        ));
        assert!(log_has_http_failure(
            "127.0.0.1 - - [15/Jan/2024:12:00:00] \"GET /assets/index.css HTTP/1.1\" 404 -"
        ));
        assert!(!log_has_http_failure(
            "2024-01-15T12:00:00 info: ready on port 3090"
        ));
        // 单独的 "5" 没有动词不算 HTTP 失败；没有动词的话，这个启发式会把任
        // 何 5 字符的 id 都误判为失败。
        assert!(!log_has_http_failure("id=12345"));
    }

    #[test]
    fn is_error_line_catches_more_shapes() {
        // 新识别出的错误形态，防护模块现在把它们视为证据。
        assert!(is_error_line(
            "TypeError: cannot read property 'x' of undefined"
        ));
        assert!(is_error_line("Uncaught (in promise) Connection refused"));
        assert!(is_error_line("\"GET /assets/main.js HTTP/1.1\" 500 -"));
    }

    #[test]
    fn runtime_evidence_attributes_plugin_from_frontend_stack() {
        let report = HealthReport {
            kind: "runtime-error".into(),
            message: "组件初始化失败".into(),
            stack: "at mount (http://127.0.0.1:3090/plugins/ghost/main.js:1:1)".into(),
            page_url: "http://127.0.0.1:3090".into(),
        };
        let suspects = attribute(
            &runtime_evidence(&report, ""),
            &[store_item("ghost", "ghost-plugin")],
            "1.0.0",
        );
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "plugin");
        assert_eq!(suspects[0].id, "ghost");
    }

    #[test]
    fn excerpt_caps_long_evidence() {
        let long_line = format!("Error: {}", "x".repeat(2000));
        let lines = vec!["context", long_line.as_str()];
        let text = excerpt(&lines, 1);
        assert!(text.chars().count() <= EVIDENCE_MAX_CHARS + 1);
        assert!(text.ends_with('…'));
    }
}
