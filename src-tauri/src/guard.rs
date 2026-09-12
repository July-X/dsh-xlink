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
use std::time::{Duration, Instant};

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
    /// 进程被拉起来了，但没能在就绪前提供服务（崩溃 / 端口竞态 / 挂起）——
    /// 这时内核日志里可能有归因证据。
    Failed(String),
    /// 内核**根本没有被派生出来**：端口被无关进程占用、激活版本未安装、
    /// 日志目录不可写……这类失败与第三方插件无关，日志里也不会有任何插件
    /// 证据，因此绝不能用来触发"停用插件"。
    SpawnFailed(String),
    Hung,
}

impl BootVerdict {
    fn reason(&self) -> String {
        match self {
            BootVerdict::Ready => String::from("启动成功"),
            BootVerdict::Failed(detail) => detail.clone(),
            BootVerdict::SpawnFailed(detail) => detail.clone(),
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
            // 这是"根本没起来"，不是"起来又崩了"：日志里没有任何可归因的插件
            // 证据，把它当成普通失败会让看护去停用一批无辜插件。
            (BootVerdict::SpawnFailed(e.to_string()), None)
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

/// 一行日志是否把某条错误**锚定地**指向这个商店条目。
///
/// 四种锚定形态：
/// 1. `plugins/<id>`：link 模式下内核插件目录里的物化路径段；
/// 2. `node_modules/<name>`：copy 模式下 profile 里的包路径；
/// 3. 带引号的包名：`Cannot find package 'x'` / `Cannot find module "x"`；
/// 4. 前端 bundle 成员：内核的 client-modules 用
///    `/plugins/??<包名>/client.js,…&rev=…` 组合路由服务全部客户端模块，前端堆栈
///    里的包名只出现在这个查询串里。
///
/// 1、2、4 都要求标识之后紧跟**路径段边界** —— 否则 `main` 会命中
/// `node_modules/main-utils`、`plugins/main__extra`，等于又退回前缀匹配。
///
/// 形态 4 只在**单成员**组合路由（以及 source map 里的 `/plugins/<包名>/client.js`
/// 形态）下成立：多成员组合是若干插件拼成的同一个脚本，帧落在哪一段无法从 URL
/// 判定，按成员逐个匹配会把同一批里的旁观者一起写进隔离清单。
///
/// 这里曾经还有一条裸的 `/<name>` 子串规则，命中的是路径段*前缀*：插件名短
/// （`main`/`ui`/`x`）时，一行 `GET /assets/main.js 500` 就足以把它写进
/// quarantine 并停用。URL 路径不是包路径，那条规则已删除。
fn is_anchored_plugin_hit(line: &str, item: &plugins::StoreItem) -> bool {
    if line.contains(&format!("'{0}'", item.name)) || line.contains(&format!("\"{0}\"", item.name))
    {
        return true;
    }
    // Windows 的日志里路径是反斜杠形态（`...\node_modules\main\index.js`、
    // `...\kernels\0.1.5\plugins\main\...`），段落锚定必须在归一化后的文本上
    // 做，否则同一份证据在 Windows 上会退化成"未归因"或全量安全模式（P2-5）。
    let normalized = line.replace('\\', "/");
    if has_segment_path(&normalized, "plugins/", &item.id)
        || has_segment_path(&normalized, "node_modules/", &item.name)
    {
        return true;
    }
    !is_ambiguous_combo_line(&normalized) && bundle_member(&normalized, &item.name)
}

/// `line` 里是否出现 bundle 路径或组合路由中的 `<name>/client.js`，且名字前是
/// 路由/查询分隔符、名字后是 URL 边界。
///
/// 前导字符这一条是必须的：只有 `??<name>/client.js`、`,<name>/client.js`、
/// `/plugins/<name>/client.js` 这种**整段成员**才算数，否则名为 `main` 的插件
/// 会被 `GET /assets/main/client.js` 这类无关请求误判。
fn bundle_member(line: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let needle = format!("{name}/client.js");
    let mut from = 0;
    while let Some(offset) = line[from..].find(&needle) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = match line[..start].chars().next_back() {
            None => true,
            // 组合路由的分隔符：`/plugins/??<包名>/client.js,…`。
            Some('?' | ',') => true,
            // 只认插件路由段里的 `/plugins/<包名>/client.js`（source map 里的来源
            // 名就是这个形态）；`/assets/main/client.js` 这类无关静态资源路径不算。
            Some('/') => line[..start - 1].ends_with("/plugins"),
            Some(_) => false,
        };
        let after_ok = line[end..]
            .chars()
            .next()
            .map(|c| !is_path_segment_char(c))
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// 行内内核 client-modules 组合路由的成员数。
///
/// 形态：`/plugins/??<包名>/client.js,<包名>/client.js&rev=<hash>`（见内核的
/// `dsh-client-modules`：`comboUrl`）。`None` 表示这一行里没有组合路由。
fn combo_route_members(line: &str) -> Option<usize> {
    const ROUTE: &str = "/plugins/??";
    let start = line.find(ROUTE)? + ROUTE.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ')' | ']' | '<' | '>'))
        .unwrap_or(rest.len());
    let members = rest[..end]
        .split(',')
        .filter(|member| !member.is_empty())
        .count();
    (members > 0).then_some(members)
}

/// 这一行是否只是「多成员组合 bundle」的地址：一个脚本里同时打着多个包，里面出现
/// 任何包名都不能作为指向该包的证据。
fn is_ambiguous_combo_line(line: &str) -> bool {
    matches!(combo_route_members(line), Some(members) if members > 1)
}

/// `line` 中是否出现 `<anchor><name>`，且 `<name>` 之后是路径段边界（或行尾）。
///
/// 与 `has_kernel_package_ref` 的边界规则**故意不同**，不要合并：那里锚定的是包
/// 命名空间 `@deepseek-ai/dsh`，其后跟 `-` 仍属同一命名空间
/// （`dsh-client-ui-theme`）；这里锚定的是**路径段**，`-` 是段内字符，因此
/// `main` 不会命中 `main-utils`。
fn has_segment_path(line: &str, anchor: &str, name: &str) -> bool {
    let needle = format!("{anchor}{name}");
    let mut from = 0;
    while let Some(offset) = line[from..].find(&needle) {
        let end = from + offset + needle.len();
        let boundary_ok = line[end..]
            .chars()
            .next()
            .map(|c| !is_path_segment_char(c))
            .unwrap_or(true);
        if boundary_ok {
            return true;
        }
        from = end;
    }
    false
}

/// 路径段内允许出现的字符。`/`、空白、引号、括号、冒号、行尾等都是段边界。
fn is_path_segment_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@')
}

/// 根据日志末尾将一次启动失败归因到已安装的插件（或内核自身）。插件
/// 候选只匹配锚定的形态（见 [`is_anchored_plugin_hit`]），因为纯子串匹配
/// 会让任何短包名只要碰巧出现在堆栈里的任何位置就被当成可疑项。
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
        let Some(idx) = lines
            .iter()
            .position(|line| is_error_line(line) && is_anchored_plugin_hit(line, item))
        else {
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
    // 同 `is_anchored_plugin_hit`：Windows 上包路径是反斜杠形态，
    // `@deepseek-ai\dsh\lib\bin.js` 也必须能命中内核命名空间（P2-5）。
    let normalized = line.replace('\\', "/");
    // 任何提到内核命名空间包名的错误。边界判断很关键：
    // `@deepseek-ai/dsh/` 覆盖根入口文件（例如
    // `@deepseek-ai/dsh/lib/bin.js`），而 `@deepseek-ai/dsh-` 覆盖
    // 内核随附的所有内建 client module（例如
    // `@deepseek-ai/dsh-client-ui-theme`）。以非字母数字的分隔符锚定
    // 可以避免一个名叫 `@scope/dsh-foo` 的社区插件仅凭 `dsh` 子串被
    // 误当成内核包。
    //
    // 例外是多成员组合 bundle 的地址：那一个脚本里同时打着第三方插件的
    // bundle，命中其中的内核包名并不能证明帧落在内核那一段上，归到内核
    // 就等于把插件的错记到内核头上（P2 级归因误报）。这种行留给
    // `diagnose_runtime` 的「前端 bundle」分支如实说明未定位到包名。
    if has_kernel_package_ref(&normalized) && !is_ambiguous_combo_line(&normalized) {
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

/// 明确的「环境类」启动失败特征。
///
/// 这类失败里内核进程**起来过**（因此不会被判成 `SpawnFailed`），但失败原因与
/// 插件无关：端口被占、目录不可写、磁盘满、原生模块与 Node ABI 不匹配……
/// 把"停用全部插件"施加在它们身上，只会在下一次重试恰好成功时（占用端口的进程
/// 退出了）把功劳记到"插件有问题"头上，并在 `quarantine.json` 里留下无辜插件的
/// 记录（P2-4）。
const ENV_FAILURE_MARKERS: [&str; 10] = [
    "eaddrinuse",
    "address already in use",
    "eacces",
    "eperm",
    "permission denied",
    "enospc",
    "no space left on device",
    "node_module_version",
    "was compiled against a different node.js version",
    "cannot find module './build/release",
];

/// 日志里是否出现了环境类失败特征；命中时返回命中的那一条（用于文案）。
fn environment_failure(log_tail: &str) -> Option<String> {
    let lower = log_tail.to_ascii_lowercase();
    ENV_FAILURE_MARKERS
        .iter()
        .find(|marker| lower.contains(*marker))
        .map(|marker| (*marker).to_string())
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
            at: crate::process::epoch_secs(),
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
    crate::state::save_best_effort(&incident_path(data_dir), incident);
}

/// 在一次干净、正常的启动后清除已记录的故障——否则这份陈旧的报告
/// 会与刚刚健康启动的工作台相矛盾。
fn clear_incident(data_dir: &Path) {
    let _ = std::fs::remove_file(incident_path(data_dir));
}

/// 读取最近一次记录的故障（供展示历史的命令使用）。
///
/// 读路径上会对旧记录重新判定一次「前端 bundle」这一类（见
/// [`reclassify_frontend_bundle_incident`]）：判断与措辞的修复必须能作用到已经
/// 落盘的事故上，否则升级外壳后概览横幅仍在念旧的「暂未能归因」文案。重新判定
/// 是纯读操作：不写隔离、不改接线、也不回写文件。
pub fn load_incident(data_dir: &Path) -> Option<Incident> {
    let text = std::fs::read_to_string(incident_path(data_dir)).ok()?;
    let mut incident: Incident = serde_json::from_str(&text).ok()?;
    reclassify_frontend_bundle_incident(&mut incident);
    Some(incident)
}

// --- 编排 --------------------------------------------------------------------

/// 在启动防护下启动当前活动的内核。向 UI 返回报告，并在成功路径上
/// 一起返回存活中的子进程（调用方把它注册到应用状态并记录其 pid）。
pub fn guarded_start(
    deps: &GuardDeps<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> (StartReport, Option<Child>) {
    let port = deps.settings.port;

    // 幂等启动：本 data dir 的内核已经在跑就什么都不做。判据走
    // `kernel::workbench_pid`（pid 文件 + 内核身份校验），而不是"配置端口上
    // 有东西在监听"——后者一是会把用户改端口之前启动的内核读成"没在跑"、
    // 从而在同一 data dir 上拉起第二个内核（会话日志损坏），二是会把恰好
    // 占用该端口的无关进程误报成"工作台已在运行"。
    if kernel::workbench_running(deps.data_dir, deps.settings) {
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
    // 本次防护是否真的改过隔离 / 接线。没改过就不必用一次完整的 pnpm install
    // 去"恢复"（P2-3）。
    let mut quarantined_this_run = false;

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
    // 内核是否**真的跑起来过**。没有的话，下面所有"插件归因 → 停用 → 重试"
    // 的阶梯都失去事实基础：那只会改写 quarantine.json 与 profile 接线，把无辜
    // 插件标成故障源（用户按提示逐个处置，可能真的把它们删掉），而真实原因
    // （端口被占、版本未安装、目录不可写）自始至终没被触及。
    let kernel_started = !matches!(verdict, BootVerdict::SpawnFailed(_));
    trail.push(format!("常规启动失败：{}", verdict.reason()));
    // 日志写入本身失败时，下面的 `log_tail` 读到的内容可能是不完整的，
    // 而事故面板仍会引用这个日志路径。把"日志坏了"这件事放进 trail，
    // 用户才知道该看哪里（P2-3）。
    if let Some(log_error) = crate::process::take_log_write_error() {
        trail.push(format!("{log_error}（本次日志可能不完整）"));
    }
    let tail = log_tail(deps);
    // 环境类失败（端口被占、目录不可写、ABI 不匹配……）与"压根没起来"同等对待：
    // 不做插件归因、不重试停用、不进安全模式。否则重试恰好成功时（占用者退出）
    // 会把环境故障记成插件故障，并让用户去处置无辜插件（P2-4）。
    let env_marker = environment_failure(&tail);
    let plugin_ladder = kernel_started && env_marker.is_none();
    if let Some(marker) = &env_marker {
        trail.push(format!(
            "检测到环境类失败特征（{marker}），跳过插件归因与安全模式"
        ));
    }
    let mut suspects = if plugin_ladder {
        attribute(&tail, &store_items, &kernel_label)
    } else {
        Vec::new()
    };

    // 第 2 次尝试：停用归因得到的可疑插件后再试。当归因没有结果时跳过
    // ——凭空猜测只会误伤无辜插件。
    if !suspects.is_empty() {
        on_progress("检测到疑似引发故障的插件，正在停用后重试…");
        let records = suspect_records(
            &suspects,
            String::from("内核启动失败，错误日志指向该插件，已自动停用"),
        );
        if quarantine::add_all(deps.data_dir, &records).is_ok() {
            quarantined_this_run = true;
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
                    at: crate::process::epoch_secs(),
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
    // 败通常是内核或环境的问题。内核压根没起来时同样跳过：把"停用全部插件"
    // 施加在环境类失败上，只会在下一次重试恰好成功时把功劳错误地记到
    // "插件有问题"头上。
    if plugin_ladder && !store_items.is_empty() {
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
            quarantined_this_run = true;
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
                    at: crate::process::epoch_secs(),
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
    if quarantined_this_run {
        if let Err(e) = plugins::restore_profile_manifest(
            deps.data_dir,
            deps.settings,
            deps.pnpm_exe,
            manifest_snapshot.as_deref(),
            on_progress,
        ) {
            trail.push(format!("恢复原接线失败：{e}"));
        }
    } else {
        // 没有归因、也没进过安全模式 ⇒ 看护本次没改过插件接线，没必要用一次
        // 完整的 `pnpm install`（可能数分钟、要联网）去"恢复"它；而且那条路径
        // 会把失败原因埋进"已恢复原有插件配置"的插件口径话术里（P2-3）。
        trail.push(String::from(
            "本次未改动插件接线（无归因、未进入安全模式），跳过接线恢复与 pnpm 重装",
        ));
    }
    let kernel_suspected = suspects.iter().any(|s| s.kind == "kernel");
    let multiple_versions = kernel::list_installed(deps.data_dir).len() > 1;
    let mut hint = String::from("此次失败与第三方插件无关。请通过「打开日志」查看完整内核日志；也可在「内核版本」页删除当前版本后重新安装。");
    if kernel_suspected || multiple_versions {
        hint = format!("也可先尝试在「内核版本」页切换到其他已安装版本。{hint}");
    }
    // 环境类失败给出真实原因与真实下一步：旧文案一律说"已恢复原有插件配置"，
    // 把用户引向插件/内核重装，而真正的出路（换端口、释放端口、检查数据目录权限）
    // 只埋在折叠的 attempts 里（P2-3/P2-4）。
    let mut message = String::from("多次尝试后工作台仍无法启动，已恢复原有插件配置。");
    let mut cause = if kernel_suspected {
        String::from("kernel")
    } else {
        String::from("unknown")
    };
    let env_reason = if kernel_started {
        env_marker
            .as_deref()
            .map(|marker| format!("内核启动过程中报告了环境类错误（{marker}）"))
    } else {
        Some(format!("内核进程没有被拉起来：{}", verdict.reason()))
    };
    if let Some(reason) = env_reason {
        message = format!("工作台无法启动：{reason}。本次未改动任何插件配置。");
        hint = String::from(
            "请按上面的原因处理后重试：端口被占用就换一个端口（设置页）或结束占用该端口的进程；\n\
             权限 / 磁盘问题请检查数据目录是否可写、磁盘是否已满；仍不确定时用「查看日志」看完整内核日志。",
        );
        cause = String::from("env");
    }
    let incident = Incident {
        recovered: false,
        safe_mode: false,
        message,
        suspects: dedup_suspects(suspects),
        attempts: trail,
        log_tail: tail,
        log_path: kernel_log_path(deps.data_dir).display().to_string(),
        hint: Some(hint),
        at: crate::process::epoch_secs(),
        cause,
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

/// 前端堆栈行在合并证据里的前缀。归因匹配与「帧是否落在客户端 bundle 路由上」
/// 的判定都靠它把堆栈行与内核日志行区分开，改这里必须两处一起改。
const FRONTEND_STACK_PREFIX: &str = "前端堆栈：";

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
        lines.push(format!("Error: {FRONTEND_STACK_PREFIX}{line}"));
    }
    if !report.page_url.trim().is_empty() {
        lines.push(format!("Error: 工作台页面地址：{}", report.page_url.trim()));
    }
    if !kernel_tail.is_empty() {
        lines.push(kernel_tail.to_string());
    }
    lines.join("\n")
}

/// 报告的堆栈里是否有帧落在内核 client-modules 的 bundle 路由（`/plugins/`）上。
///
/// 只看**前端堆栈行**：内核日志里的 `GET /plugins/… 404` 访问行同样含这个路径，
/// 但它说明的是静态资源加载失败，与「某个 bundle 抛了异常」是两回事。
fn has_client_bundle_frames(evidence: &str) -> bool {
    evidence
        .lines()
        .any(|line| line.contains(FRONTEND_STACK_PREFIX) && line.contains("/plugins/"))
}

/// 「前端 bundle」这一类事故的文案。
///
/// 判断只说证据支持的事（异常来自客户端模块 bundle、但没有包名），并且明确
/// 页面仍在运行——把一个非致命的前端异常说成「页面异常」，再让用户去切换或
/// 重装内核版本，是这次误报的直接来源。下一步先要可诊断的材料（自检证据里的
/// 消息），再谈动插件或换版本。
fn frontend_bundle_wording() -> (String, String) {
    (
        String::from(
            "工作台页面抛出了一个未处理的前端异常，异常来自内核服务的客户端模块 bundle（/plugins/），但证据里没有包名，无法区分内核内置组件与第三方插件；工作台本身仍在运行。",
        ),
        String::from(
            "请用「打开日志」查看内核侧记录，并把上方自检证据里的「消息」一并反馈（它指出具体是哪段代码或哪条数据不合法）；若该提示反复出现，可先在插件页停用第三方插件后重启验证，再到内核版本页切换其他版本。",
        ),
    )
}

/// 把一份**已经落盘**的旧事故重新判定为「前端 bundle」。
///
/// 只改判断与文案，不碰 suspects / attempts / log_tail —— 那些是诊断当时的原始
/// 记录，重写它们等于伪造证据。判据与 `diagnose_runtime` 完全一致：健康证据里
/// 必须真有落在 bundle 路由上的前端堆栈帧，且没有任何指向插件/内核的嫌疑对象。
fn reclassify_frontend_bundle_incident(incident: &mut Incident) {
    if incident.cause != "unknown" || !incident.suspects.is_empty() {
        return;
    }
    let Some(health) = incident.health.as_ref() else {
        return;
    };
    if !has_client_bundle_frames(&runtime_evidence(health, "")) {
        return;
    }
    let (message, hint) = frontend_bundle_wording();
    incident.cause = String::from("frontend");
    incident.message = message;
    incident.hint = Some(hint);
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
    let now = crate::process::epoch_secs();
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
    let evidence = runtime_evidence(&report, &tail);
    // 强证据：锚定到插件或内核的错误行。
    let mut suspects = attribute(&evidence, &store_items, &kernel_label);
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
    let mut cause = runtime_cause(&suspects);
    // 前端 bundle 证据：帧落在内核 client-modules 的 `/plugins/` 组合路由上，却没有
    // 能锚定到包的证据。包名只存在于组合 URL 的查询串里，而 WebKit 的堆栈文本会把
    // 查询串整个丢掉（只剩 `http://127.0.0.1:3090/plugins/`），因此多成员组合里的
    // 帧既不能归给插件（同批里可能只是旁观者），也不能归给内核（同批里有第三方
    // bundle），必须如实说「未定位到包名」，而不是含糊的「暂未能归因」。
    if cause == "unknown" && has_client_bundle_frames(&evidence) {
        cause = "frontend";
    }
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
        "frontend" => {
            // 前端 bundle 路径：帧指向内核服务的客户端模块 bundle，但证据里没有
            // 可归因的包名。不隔离、不改插件，也不把页面说成坏了——页面仍在运行，
            // 抛出异常的只是其中一段 bundle 代码。
            attempts.push(String::from(
                "错误堆栈指向内核服务的客户端模块 bundle（/plugins/ 组合路由），但证据里没有可归因的包名",
            ));
            frontend_bundle_wording()
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
    fn plugin_named_like_a_url_segment_is_not_blamed() {
        // P2-7：插件名 `main` 撞上一条静态资源请求日志。旧实现的裸 `/<name>`
        // 子串规则命中 `GET /assets/main.js`，把这个插件写进 quarantine 并
        // 停用 —— 一条与它毫无关系的 5xx 就足以废掉一个正常插件。
        let tail = "Error: GET /assets/main.js 500 Internal Server Error\n";
        let items = vec![store_item("main", "main")];
        assert!(
            attribute(tail, &items, "0.1.1").is_empty(),
            "URL 路径不是包路径，不能作为归因证据"
        );

        // 连 `/main` 这种"整段就是包名"的 URL 也不能算（它不是模块路径）。
        let tail = "Error: GET /main HTTP/1.1 500\n";
        assert!(attribute(tail, &items, "0.1.1").is_empty());
    }

    #[test]
    fn module_paths_still_attribute_after_anchoring() {
        // 收紧之后真正该命中的形态不能丢：copy 模式的 profile 路径与
        // link 模式的 plugins/ 路径。
        let items = vec![store_item("main", "main")];
        let copy_tail =
            "Error: Cannot find module '/Users/u/.dsh/profiles/web/node_modules/main/index.js'\n";
        let suspects = attribute(copy_tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].id, "main");

        let link_tail = "Error: ENOENT: no such file '/Users/u/.dsh/desktop/kernels/0.1.1/plugins/main/index.js'\n";
        assert_eq!(attribute(link_tail, &items, "0.1.1").len(), 1);
    }

    #[test]
    fn plugin_name_that_prefixes_another_package_is_not_blamed() {
        // 段边界检查：`main` 不该命中 `node_modules/main-utils`。
        let tail = "Error: Cannot find module '/p/node_modules/main-utils/index.js'\n";
        let items = vec![store_item("main", "main")];
        assert!(attribute(tail, &items, "0.1.1").is_empty());
    }

    #[test]
    fn plugin_id_that_prefixes_another_id_is_not_blamed() {
        // 同一类前缀问题也存在于 `plugins/<id>`：id `main` 是 `main__extra`
        // 的前缀，必须只在真正属于它的路径段上命中。
        let tail = "Error: ENOENT: no such file '/p/plugins/main__extra/index.js'\n";
        let items = vec![
            store_item("main", "main"),
            store_item("main__extra", "@scope/main-extra"),
        ];
        let suspects = attribute(tail, &items, "0.1.1");
        assert_eq!(suspects.len(), 1);
        assert_eq!(
            suspects[0].id, "main__extra",
            "只有 id 与路径段完全一致的那个插件才算被命中"
        );
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

    /// 2026-09-11 在 macOS（WKWebView）上真实捕获的前端自检证据：内核
    /// client-modules 的组合 bundle URL 到了 `Error.stack` 里只剩 `/plugins/`，
    /// 只存在于查询串里的包名被 WebKit 丢掉。行号 148814 是组合脚本里的绝对行号，
    /// 因此它确实来自内核服务的那一个 bundle，但**没有**任何可归因的包名。
    const MACOS_BUNDLE_STACK: &str = "validateRecord@http://127.0.0.1:4090/plugins/:148814:26\n\
expandAssistantStream@http://127.0.0.1:4090/plugins/:148722:34\n\
replace@http://127.0.0.1:4090/plugins/:148874:80\n\
installWindow@http://127.0.0.1:4090/plugins/:149517:49\n\
acceptEventChange@http://127.0.0.1:4090/plugins/:149504:25\n\
publish@http://127.0.0.1:4090/plugins/:149479:29\n\
publish@http://127.0.0.1:4090/plugins/:147869:22\n\
replaceFromOpening@http://127.0.0.1:4090/plugins/:1115:25\n\
replaceGeneration@http://127.0.0.1:4090/plugins/:1093:28\n\
open@http://127.0.0.1:4090/plugins/:1011:28";

    /// 每个用例一个唯一的「dsh home」，data dir 是它下面的 `desktop/`。
    ///
    /// **data dir 必须带这一层父目录**：插件中央库在 `data_dir` 的**父目录**下
    /// （`plugins::store_dir` = `<home>/plugins`），把临时目录本身当 data dir 会让
    /// 所有用例共用同一个 `/tmp/plugins`——并行跑测试时互相覆盖，甚至被某个用例的
    /// 清理逻辑整个删掉，表现为与被测行为无关的偶发失败（本次就是踩到了这个）。
    fn temp_data_dir(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "dsh-guard-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let data_dir = home.join("desktop");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        data_dir
    }

    fn write_store(data_dir: &Path, json: &str) {
        let store_dir = plugins::store_dir(data_dir);
        std::fs::create_dir_all(&store_dir).expect("create store dir");
        std::fs::write(store_dir.join("store.json"), json).expect("write store");
    }

    #[test]
    fn combo_route_members_counts_members() {
        assert_eq!(
            combo_route_members(
                "Error: 前端堆栈：f@http://127.0.0.1:4090/plugins/??a/client.js&rev=1:2:3"
            ),
            Some(1)
        );
        assert_eq!(
            combo_route_members(
                "Error: 前端堆栈：f@http://127.0.0.1:4090/plugins/??a/client.js,b/client.js&rev=1:2:3"
            ),
            Some(2)
        );
        // `/plugins/<包名>/client.js`（source map 里的来源名）不是组合路由。
        assert_eq!(
            combo_route_members("Error: 前端堆栈：f@http://127.0.0.1:4090/plugins/a/client.js:2:3"),
            None
        );
        // 同一个包在组合 URL 里出现两次也只算两次成员——它依然是多成员脚本。
        assert_eq!(
            combo_route_members("/plugins/??a/client.js,a/client.js&rev=1"),
            Some(2)
        );
    }

    #[test]
    fn bundle_member_requires_a_whole_member_segment() {
        // 单成员组合路由与 map 里的 `/plugins/<包名>/client.js` 都算。
        assert!(bundle_member(
            "Error: f@http://127.0.0.1:4090/plugins/??main/client.js&rev=1:2:3",
            "main"
        ));
        assert!(bundle_member(
            "Error: f@http://127.0.0.1:4090/plugins/main/client.js:2:3",
            "main"
        ));
        // 路径段前缀不是成员：`main-utils`、以及无关静态资源目录下的同名文件。
        assert!(!bundle_member(
            "Error: f@http://127.0.0.1:4090/plugins/??main-utils/client.js&rev=1:2:3",
            "main"
        ));
        assert!(!bundle_member(
            "Error: GET /assets/main/client.js 500",
            "main"
        ));
    }

    #[test]
    fn single_member_combo_attributes_plugin_from_frontend_stack() {
        // WebView2（V8）会保留查询串：单成员组合路由里的包名就是唯一成员，
        // 这条帧可以锚定到该插件。
        let report = HealthReport {
            kind: "unhandled-rejection".into(),
            message: "TypeError: 组件初始化失败".into(),
            stack:
                "at mount (http://127.0.0.1:4090/plugins/??ghost-plugin/client.js&rev=abc:12:26)"
                    .into(),
            page_url: "http://127.0.0.1:4090".into(),
        };
        let suspects = attribute(
            &runtime_evidence(&report, ""),
            &[store_item("ghost-plugin", "ghost-plugin")],
            "1.0.0",
        );
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].kind, "plugin");
        assert_eq!(suspects[0].id, "ghost-plugin");
    }

    #[test]
    fn multi_member_combo_blames_neither_plugin_nor_kernel() {
        // 多成员组合是若干包拼成的同一个脚本：里面的每个包名都只是「同batch的
        // 邻居」。按成员逐条匹配会把无辜插件写进隔离清单，按内核命名空间匹配
        // 会把插件的错记到内核头上——两者都必须拒绝。
        let evidence = runtime_evidence(
            &HealthReport {
                kind: "unhandled-rejection".into(),
                message: "TypeError: boom".into(),
                stack: "at validateRecord (http://127.0.0.1:4090/plugins/??ghost-plugin/client.js,@deepseek-ai/dsh-api-session-controller/client.js&rev=abc:148814:26)".into(),
                page_url: "http://127.0.0.1:4090".into(),
            },
            "",
        );
        assert!(
            attribute(
                &evidence,
                &[store_item("ghost-plugin", "ghost-plugin")],
                "1.0.0"
            )
            .is_empty(),
            "多成员组合里的插件名不能作为指向该插件的证据"
        );
        assert!(!is_kernel_evidence_line(
            "Error: 前端堆栈：at validateRecord (http://127.0.0.1:4090/plugins/??ghost-plugin/client.js,@deepseek-ai/dsh-api-session-controller/client.js&rev=abc:148814:26)"
        ));
    }

    #[test]
    fn client_bundle_frames_only_count_frontend_stack_lines() {
        let evidence = runtime_evidence(
            &HealthReport {
                kind: "unhandled-rejection".into(),
                message: String::new(),
                stack: "validateRecord@http://127.0.0.1:4090/plugins/:148814:26".into(),
                page_url: String::new(),
            },
            "",
        );
        assert!(has_client_bundle_frames(&evidence));
        // 内核日志里的 `/plugins/…` 访问行说的是静态资源加载失败，
        // 不是「某个 bundle 抛了异常」，不能算 bundle 帧。
        assert!(!has_client_bundle_frames(
            "Error: GET /plugins/??x/client.js 500 Internal Server Error"
        ));
    }

    /// 归因落在「内核服务的客户端模块 bundle、但证据里没有包名」时，事故面板必须
    /// 如实说这一点：含糊的「暂未能归因」+「切换/重装内核」会把一次前端异常说成
    /// 页面故障，并让用户去动一个无辜的内核版本。同时不得隔离任何插件。
    #[test]
    fn runtime_client_bundle_fault_is_classified_without_blaming_anyone() {
        let data_dir = temp_data_dir("frontend-bundle");
        write_store(
            &data_dir,
            r#"{"schemaVersion":1,"items":[{"id":"ghost-plugin","name":"ghost-plugin"}]}"#,
        );
        let report = HealthReport {
            kind: "unhandled-rejection".into(),
            message: "TypeError: Assistant stream raw chunk must be a lossless JSON object".into(),
            stack: MACOS_BUNDLE_STACK.into(),
            page_url: "http://127.0.0.1:4090/".into(),
        };

        let incident = diagnose_runtime(&data_dir, report);

        assert_eq!(incident.cause, "frontend");
        assert!(
            incident.suspects.is_empty(),
            "没有包名就不许指认任何嫌疑对象"
        );
        assert!(
            incident.message.contains("前端") && incident.message.contains("bundle"),
            "文案必须点明异常来自前端 bundle：{}",
            incident.message
        );
        assert!(
            incident.message.contains("仍在运行"),
            "页面仍在运行时不得把它说成页面故障：{}",
            incident.message
        );
        let hint = incident.hint.clone().unwrap_or_default();
        assert!(
            hint.contains("消息"),
            "下一步必须引导用户反馈自检证据里的错误消息：{hint}"
        );
        assert!(
            crate::quarantine::load(&data_dir).items.is_empty(),
            "前端 bundle 证据不得隔离任何插件"
        );
        assert_eq!(
            incident.health.map(|health| health.kind),
            Some(String::from("unhandled-rejection")),
            "自检证据必须原样留在事故里"
        );

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// 单成员组合路由里的插件名是强证据，仍必须走自动隔离路径——修多成员误报
    /// 时不能把这条真实可用的归因一起收紧掉。
    #[test]
    fn runtime_single_bundle_plugin_evidence_still_quarantines() {
        let data_dir = temp_data_dir("frontend-single");
        write_store(
            &data_dir,
            r#"{"schemaVersion":1,"items":[{"id":"ghost-plugin","name":"ghost-plugin"}]}"#,
        );
        let incident = diagnose_runtime(
            &data_dir,
            HealthReport {
                kind: "unhandled-rejection".into(),
                message: "TypeError: boom".into(),
                stack:
                    "at mount (http://127.0.0.1:4090/plugins/??ghost-plugin/client.js&rev=abc:12:26)"
                        .into(),
                page_url: "http://127.0.0.1:4090/".into(),
            },
        );

        assert_eq!(incident.cause, "plugin");
        assert_eq!(incident.suspects.len(), 1);
        assert_eq!(incident.suspects[0].name, "ghost-plugin");
        let quarantined = crate::quarantine::load(&data_dir).items;
        assert_eq!(quarantined.len(), 1);
        assert_eq!(quarantined[0].id, "ghost-plugin");

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// 已经落盘的旧事故也必须跟着改判：用户升级外壳后，概览横幅读的就是这份
    /// 文件。改判只许动判断与文案，原始证据（attempts / log_tail）与文件本身
    /// 都不能被改写——读路径不产生副作用。
    #[test]
    fn stored_bundle_incident_is_reclassified_without_rewriting_evidence() {
        let data_dir = temp_data_dir("frontend-stored");
        let stored = Incident {
            recovered: false,
            safe_mode: false,
            message: String::from("工作台页面异常，但暂未找到足够证据区分插件和内核。"),
            suspects: Vec::new(),
            attempts: vec![
                String::from("工作台健康探针报告：unhandled-rejection：validateRecord@…"),
                String::from("未发现足够的插件或内核证据，暂不作强归因"),
            ],
            log_tail: String::from("dsh web: http://127.0.0.1:4090/?token=…\n"),
            log_path: String::from("/tmp/kernel.log"),
            hint: Some(String::from(
                "请打开日志并重试；若持续发生，再到内核版本页切换其他版本或重新安装当前版本。",
            )),
            at: 1_789_133_158,
            cause: String::from("unknown"),
            health: Some(HealthReport {
                kind: String::from("unhandled-rejection"),
                message: MACOS_BUNDLE_STACK.to_string(),
                stack: MACOS_BUNDLE_STACK.to_string(),
                page_url: String::from("http://127.0.0.1:4090/"),
            }),
        };
        save_incident(&data_dir, &stored);
        let on_disk = std::fs::read_to_string(incident_path(&data_dir)).expect("read incident");

        let loaded = load_incident(&data_dir).expect("incident loads");

        assert_eq!(loaded.cause, "frontend");
        assert_eq!(loaded.message, frontend_bundle_wording().0);
        assert_eq!(loaded.hint, Some(frontend_bundle_wording().1));
        assert_eq!(loaded.attempts, stored.attempts, "原始轨迹不得被改写");
        assert_eq!(loaded.log_tail, stored.log_tail, "原始日志片段不得被改写");
        assert_eq!(loaded.at, stored.at);
        assert_eq!(
            std::fs::read_to_string(incident_path(&data_dir)).expect("read incident"),
            on_disk,
            "读路径不得回写事故文件"
        );

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// 改判必须有边界：有嫌疑对象的事故（启动时停用了插件）和没有健康证据的
    /// 事故都不能被贴上「前端 bundle」的标签。
    #[test]
    fn reclassification_leaves_other_incidents_alone() {
        let bundle_health = Some(HealthReport {
            kind: String::from("unhandled-rejection"),
            message: MACOS_BUNDLE_STACK.to_string(),
            stack: MACOS_BUNDLE_STACK.to_string(),
            page_url: String::from("http://127.0.0.1:4090/"),
        });
        let mut with_suspect = Incident {
            recovered: true,
            safe_mode: true,
            message: String::from("工作台已在停用以下插件后成功启动。"),
            suspects: vec![Suspect {
                kind: String::from("plugin"),
                id: String::from("ghost-plugin"),
                name: String::from("ghost-plugin"),
                evidence: String::new(),
            }],
            attempts: Vec::new(),
            log_tail: String::new(),
            log_path: String::new(),
            hint: None,
            at: 1,
            cause: String::from("plugin"),
            health: bundle_health,
        };
        reclassify_frontend_bundle_incident(&mut with_suspect);
        assert_eq!(with_suspect.cause, "plugin");
        assert_eq!(with_suspect.message, "工作台已在停用以下插件后成功启动。");

        let mut startup = Incident {
            cause: String::from("unknown"),
            health: None,
            ..with_suspect.clone()
        };
        startup.suspects.clear();
        reclassify_frontend_bundle_incident(&mut startup);
        assert_eq!(startup.cause, "unknown", "没有健康证据就不能改判");
    }

    /// P2-5：Windows 的反斜杠路径必须能参与段落锚定。
    ///
    /// 不归一化时 `...\node_modules\ghost-plugin\index.js` 这类**不带引号**的
    /// 路径行既匹配不到 `node_modules/<name>`，也匹配不到内核命名空间，同一份
    /// 证据在 Windows 上会退化成"未归因"或全量安全模式。
    #[test]
    fn attribution_matches_windows_backslash_paths() {
        let items = vec![store_item("ghost-plugin", "ghost-plugin")];
        let bare =
            r"Error: at C:\Users\me\kernels\0.1.5\node_modules\ghost-plugin\lib\index.js:1:1";
        assert!(
            is_anchored_plugin_hit(bare, &items[0]),
            "反斜杠路径必须命中 node_modules/<name> 段落"
        );
        let suspects = attribute(bare, &items, "0.1.5");
        assert_eq!(suspects.len(), 1, "反斜杠路径必须能归因到插件");
        assert_eq!(suspects[0].id, "ghost-plugin");

        assert!(
            is_kernel_evidence_line(
                r"Error: Cannot find module 'C:\Users\me\kernels\0.1.5\node_modules\@deepseek-ai\dsh\lib\bin.js'"
            ),
            "反斜杠形态的内核包路径同样要命中内核命名空间"
        );
        // 段落边界仍然生效：`ghost-plugin-utils` 不是 `ghost-plugin`。
        assert!(!is_anchored_plugin_hit(
            r"Error: at C:\tmp\node_modules\ghost-plugin-utils\index.js:1:1",
            &items[0]
        ));
    }

    /// P2-4：环境类失败特征识别（端口被占、权限、ABI 不匹配……）。
    #[test]
    fn environment_failure_recognizes_port_and_permission_errors() {
        assert!(environment_failure(
            "Error: listen EADDRINUSE: address already in use 127.0.0.1:3090"
        )
        .is_some());
        assert!(environment_failure("Error: EACCES: permission denied, open '/x'").is_some());
        assert!(environment_failure(
            "Error: The module was compiled against a different Node.js version using NODE_MODULE_VERSION 127"
        )
        .is_some());
        assert!(environment_failure("Error: ENOSPC: no space left on device").is_some());
        assert!(
            environment_failure("TypeError: cannot read property 'x' of undefined").is_none(),
            "普通前端/插件错误不得被当成环境类失败"
        );
        assert!(environment_failure("").is_none());
    }

    /// P2-3 / P2-4：内核"起来又因环境原因退出"时，看护不得进入插件阶梯，也不该
    /// 用一次完整的 pnpm install 去"恢复"从未改过的接线，事故文案要给出真实原因。
    ///
    /// 构造：假 node（脚本）打印一行 EADDRINUSE 后以非 0 退出——这正是"端口被占"
    /// 在内核日志里的形态。修复前：`kernel_started` 为真 ⇒ 进入安全模式停用全部
    /// 插件、改写 profile 接线，最后跑 `pnpm install` 恢复，并告诉用户"已恢复原有
    /// 插件配置 / 请重装内核"。
    #[cfg(unix)]
    #[test]
    fn environment_exit_skips_the_plugin_ladder_and_the_pnpm_restore() {
        use std::os::unix::fs::PermissionsExt;

        let data_dir = temp_data_dir("env-exit");
        let version = "0.1.2";
        let kernel_dir = crate::kernel::kernel_dir(&data_dir, version);
        let bin = kernel_dir.join("node_modules/@deepseek-ai/dsh/lib/bin.js");
        std::fs::create_dir_all(bin.parent().expect("bin parent")).expect("kernel tree");
        std::fs::write(&bin, "// stub\n").expect("write bin");

        // 假 node：模拟"端口被占"的内核启动失败。
        let fake_node = data_dir.join("fake-node");
        std::fs::write(
            &fake_node,
            "#!/bin/sh\necho 'Error: listen EADDRINUSE: address already in use 127.0.0.1:3090' 1>&2\nexit 1\n",
        )
        .expect("write fake node");
        std::fs::set_permissions(&fake_node, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake node");

        // 端口用 0：`port_open(0)` 恒为假，因此这个用例不会因为并行跑的其它用例
        // 抢走临时端口而走进"端口被占用"的 SpawnFailed 分支（那会让下面的文案断言
        // 随机失败）。假 node 与端口无关，它总是打印 EADDRINUSE 后退出。
        let settings = crate::settings::Settings {
            port: 0,
            ..crate::settings::Settings::default()
        };
        crate::settings::save(&data_dir, &settings).expect("save settings");
        crate::kernel::write_active(&data_dir, Some(version)).expect("write active");

        // 中央库里有一个已安装插件：修复前它会被全量停用。
        let store_dir = plugins::store_dir(&data_dir);
        std::fs::create_dir_all(&store_dir).expect("store dir");
        std::fs::write(
            store_dir.join("store.json"),
            r#"{"schemaVersion":1,"items":[{"id":"ghost","name":"ghost-plugin"}]}"#,
        )
        .expect("write store");

        let deps = GuardDeps {
            data_dir: &data_dir,
            settings: &settings,
            node_path: &fake_node,
            pnpm_exe: Path::new("/nonexistent/pnpm"),
        };
        let (report, child) = guarded_start(&deps, &mut |_| {});
        assert!(child.is_none(), "环境类失败不该留下内核进程");

        let incident = report.incident.expect("必须有事故面板");
        assert_eq!(incident.cause, "env", "环境类失败必须标成 env");
        assert!(
            incident.message.contains("环境类错误"),
            "文案要点出真实原因：{}",
            incident.message
        );
        assert!(
            !incident
                .attempts
                .iter()
                .any(|entry| entry.contains("正在进入安全模式") || entry.contains("下启动成功")),
            "环境类失败不得进入安全模式；实际轨迹：{:?}",
            incident.attempts
        );
        assert!(
            incident
                .attempts
                .iter()
                .any(|entry| entry.contains("跳过接线恢复")),
            "没有改过接线就不该跑 pnpm 恢复；实际轨迹：{:?}",
            incident.attempts
        );
        assert!(
            crate::quarantine::load(&data_dir).items.is_empty(),
            "环境类失败不得隔离任何插件"
        );
        // 记录文件本身在整轮里必须仍然存在；内容层面的"记录未被清空"由
        // `unrelated_port_occupant_does_not_quarantine_plugins` 用同一份夹具断言
        // （这里不重复读盘：并行跑测试时另一次读盘可能与写盘交错，让断言变成
        // 与被测行为无关的噪声）。
        assert!(
            plugins::store_dir(&data_dir).join("store.json").is_file(),
            "插件清单文件不得被删除"
        );

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// 内核**根本没被拉起来**时（这里是端口被无关进程占用），看护不得进入
    /// "归因 → 停用插件 → 安全模式"的阶梯。
    ///
    /// 修复前：`start_maybe` 的 Err 被折叠成普通失败，日志里自然没有插件证据
    /// （`attribute` 返回空），但第 3 次尝试仍会无条件停用**全部**第三方插件并
    /// 改写 profile 接线。若那次重试恰好成功（占用端口的进程退出了），用户会
    /// 收到"已停用以下插件后成功启动"的报告——把一次环境故障记成插件故障。
    #[test]
    fn unrelated_port_occupant_does_not_quarantine_plugins() {
        // 走同一个 helper：data dir 带自己的父目录，中央库才不会落到共享的
        // `/tmp/plugins` 上（见 `temp_data_dir` 的说明）。
        let data_dir = temp_data_dir("spawn-failed");

        // 端口被本测试进程占用——它不是 dsh 内核。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
        let port = listener.local_addr().expect("listener addr").port();
        let settings = crate::settings::Settings {
            port,
            ..crate::settings::Settings::default()
        };
        crate::settings::save(&data_dir, &settings).expect("save settings");

        // 中央库里有一个已安装插件：修复前它会被全量停用并写进 quarantine.json。
        let store_dir = plugins::store_dir(&data_dir);
        std::fs::create_dir_all(&store_dir).expect("create store dir");
        std::fs::write(
            store_dir.join("store.json"),
            r#"{"schemaVersion":1,"items":[{"id":"ghost","name":"ghost-plugin"}]}"#,
        )
        .expect("write store");
        assert_eq!(
            plugins::load_store(&data_dir).items.len(),
            1,
            "测试前置：store.json 必须能被解析，否则这个用例失去区分度"
        );

        let deps = GuardDeps {
            data_dir: &data_dir,
            settings: &settings,
            node_path: Path::new("/nonexistent/node"),
            pnpm_exe: Path::new("/nonexistent/pnpm"),
        };
        let (report, child) = guarded_start(&deps, &mut |_| {});

        assert!(child.is_none(), "内核不该被拉起来");
        assert!(
            !report.running,
            "端口被无关进程占用时不得报告工作台正在运行"
        );
        // 关键断言是"有没有进入安全模式"：旧行为确实会隔离全部插件、改写
        // profile 接线，只是因为重试同样失败才在最后回滚隔离状态——所以单看
        // quarantine 是否为空无法区分。真正致命的分支是"重试恰好成功"：那时
        // 看护会把环境故障记成插件故障，并让用户按提示去处置无辜插件。
        let attempts = report
            .incident
            .as_ref()
            .map(|incident| incident.attempts.clone())
            .unwrap_or_default();
        // 判据是「有没有**进入**安全模式」，不能拿裸词「安全模式」当判据：正确路径
        // 的收尾文案里也有一句「本次未改动插件接线（无归因、未进入安全模式）」，那是
        // 否定用法，按裸词匹配会把正确行为判成失败。
        assert!(
            !attempts.iter().any(|entry| entry.starts_with("安全模式")),
            "内核根本没被拉起来时不得进入安全模式；实际轨迹：{attempts:?}"
        );
        assert!(
            report
                .incident
                .as_ref()
                .map(|incident| incident.suspects.is_empty())
                .unwrap_or(true),
            "不得把任何插件列为疑似故障源"
        );
        assert!(
            crate::quarantine::load(&data_dir).items.is_empty(),
            "环境类失败不得隔离任何插件"
        );
        assert_eq!(
            plugins::load_store(&data_dir).items.len(),
            1,
            "插件记录必须原样保留"
        );

        drop(listener);
        let _ = std::fs::remove_dir_all(&data_dir);
        let _ = std::fs::remove_dir_all(&store_dir);
    }
}
