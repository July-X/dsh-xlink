//! Defender 排除项：启动时**自动**把内核的「第三方包代码」目录交给 Defender 免检。
//!
//! ## 为什么是「排除项」而不是「防火墙例外」
//!
//! 防火墙管的是网络，文件被扫描是杀软的**实时保护**管的事——加防火墙例外对
//! 「打开文件慢」一点用都没有。真正管用的是实时保护的**排除项**。
//!
//! ## 为什么值得做
//!
//! 本机实测：打开一个从没读过的文件（open + close）约 7.7ms，再打开一次
//! 0.07ms；内核冷启动 55 秒 ≈ 7,700 次这样的 open × 7.7ms。杀软的扫描代价
//! **付在 open 上**，stat 上几乎没有。⇒ 冷启动慢的是杀软，不是内核。
//!
//! ## 范围（安全边界，不许放宽）
//!
//! 只排除**第三方包代码**两个目录：
//!
//! 1. 所有已安装内核的父目录 `<data_dir>/kernels`（底下只有安装树——只读、
//!    安装时按摘要校验过，没有用户数据）。**排父目录而不是 `<版本>`**：内核
//!    一升级版本目录就换名字，排除项会失效、又得弹一次 UAC。
//! 2. 当前实例 profile 的 `node_modules`（纯第三方包代码）。
//!
//! **不排除**实例数据目录的其余部分（`sessions/`、`.credentials.yaml`、
//! `storages/`）——那是用户数据与凭据，让它继续被实时扫描。也不排除用户
//! 目录、系统临时目录、进程可执行文件。
//!
//! ## 自动，但不偷偷来
//!
//! 启动 3 秒后（面板已经画出来）在后台跑一次，改排除项**必须**经 UAC 管理员
//! 授权——那一步就是用户的明确同意，拒绝即不做。做过什么记在
//! `<shell_dir>/defender-exclusions.json`：
//!
//! - 已经加好：以后启动**一个字节都不动**（不会每次弹 UAC）；
//! - 用户拒绝授权 / 在设置页点过「移除」：也不再自动重试——用户已经表过态；
//! - 其它失败（Defender 没运行、篡改防护拦下）：24 小时后再试一次。
//!
//! ## 代价（界面上必须写清）
//!
//! 排除之后这两个目录里的东西**不再被实时扫描**：装进来的第三方包若被篡改，
//! Defender 不会当场发现。这就是换来冷启动速度的代价。设置页的卡上原样写着
//! 这句话，并且提供「移除排除项」。
//!
//! 只对 Microsoft Defender 生效。装了 360 这类第三方杀软时，它们自己的扫描
//! 不受影响；那些产品没有公开接口，不能替用户改。
//!
//! ## 提权
//!
//! 用 `Start-Process -Verb RunAs` 起一个提权的 `powershell.exe`（UAC 弹窗由
//! 用户点「是」），等它跑完并取回退出码。载荷经 `-EncodedCommand`（UTF-16LE
//! 编成 base64）传：只含 ASCII，不受 PowerShell 引号规则与 cmd 转发规则影响。
//! 逐条结果写进报告文件——提权进程的 stdout 落在它自己的控制台里，我们拿不到。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 事件日志的逻辑名（落在「查看日志」面板里）。
const EVENT_LOG: &str = "av-trust";

/// 启动后多久开始自动加入：面板先画出第一帧，UAC 弹窗才有上下文。
const AUTO_DELAY: Duration = Duration::from_secs(3);

/// 提权等待上限。UAC 弹窗要用户点「是」，默认的 30 秒不够。
const ELEVATION_TIMEOUT: Duration = Duration::from_secs(120);

/// 非用户拒绝的失败之后，多久再自动试一次。
const RETRY_AFTER: Duration = Duration::from_secs(24 * 3600);

/// 自动加入的记录文件（按壳模式隔离，与设置文件同目录）。
const STATE_FILE: &str = "defender-exclusions.json";

const OUTCOME_APPLIED: &str = "applied";
const OUTCOME_REMOVED: &str = "removed";
const OUTCOME_DECLINED: &str = "declined";
const OUTCOME_FAILED: &str = "failed";

/// 面板读到的「Defender 排除项」现状。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvTrustStatus {
    /// 当前平台是否支持（只有 Windows 的 Defender 有「排除项」这回事）。
    pub supported: bool,
    /// 非 Windows 平台的一句话说明（Windows 上为空串）。
    pub platform_note: String,
    /// 系统里有没有 Defender。
    pub defender_present: bool,
    /// Defender 实时保护是否开着（关着时排除项不起作用）。
    pub defender_realtime: bool,
    /// 要排除的两个目录。
    pub targets: Vec<ExclusionTarget>,
    /// 上次加入 / 移除的结果。`None` = 还没试过。
    pub last: Option<LastAttempt>,
}

/// 一个要排除的目录。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionTarget {
    /// 人类可读的标签。
    pub label: String,
    /// 绝对路径原文。
    pub path: String,
    /// 这个目录现在存在吗（不存在就没什么可扫的）。
    pub exists: bool,
}

/// 上一次加入 / 移除的结果。**普通用户读不到 Defender 的排除项清单**
/// （那棵子树的 ACL 收给了管理员），所以「排没排」只能靠这份自己记的账。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastAttempt {
    /// `applied` / `removed` / `declined` / `failed`。
    pub outcome: String,
    /// 本地时刻，给人看。
    pub at: String,
    /// 失败或拒绝时的原话（成功时为空串）。
    pub detail: String,
    /// 这些目录现在在排除项里（`removed` 时为空）。
    pub paths: Vec<String>,
}

/// 内部用的目标目录（标签 + 路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    label: &'static str,
    path: PathBuf,
}

/// 落盘的记录。**它是「排过没排过」的唯一来源**：Defender 的排除项清单要
/// 管理员才读得到，而每次启动都弹一次 UAC 去核对是不可接受的。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    /// 上次加入的目录（`removed` 时为空）。
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    outcome: String,
    /// 本地时刻，给人看。
    #[serde(default)]
    at: String,
    /// 同一时刻的 Unix 秒，只用于「隔多久再试」。
    #[serde(default)]
    at_unix: u64,
    #[serde(default)]
    detail: String,
}

/// 要排除的目录（标签 + 路径）。**纯函数**：两个路径显式传进来，
/// 单测因此不必碰用户真实的数据目录。
///
/// 只收第三方包代码：内核安装树与实例 profile 的 `node_modules`。实例数据
/// 目录的其余部分（会话、凭据、storages）是用户数据，**不在**这里。
fn targets(kernel_trees: Option<&Path>, profile_modules: Option<&Path>) -> Vec<Target> {
    let mut out = Vec::new();
    if let Some(path) = kernel_trees {
        out.push(Target {
            label: "内核目录",
            path: path.to_path_buf(),
        });
    }
    if let Some(path) = profile_modules {
        out.push(Target {
            label: "实例依赖包",
            path: path.to_path_buf(),
        });
    }
    out
}

/// 所有已安装内核的父目录：`<data_dir>/kernels`。
///
/// 排除的是这一层而不是 `<版本>` 那一层——版本一升级目录名就变，排除项立刻
/// 失效。这一层底下只有内核安装树。
fn kernel_trees_dir(data_dir: &Path) -> PathBuf {
    // 与 `kernel_dir` 同源（那个是 `<data_dir>/kernels/<版本>`），布局变了这里
    // 跟着变。
    crate::kernel::lifecycle::kernels_dir(data_dir)
}

/// 实例 profile 的第三方依赖包目录：`<instance_dir>/profiles/<profile>/node_modules`。
///
/// profile 名必须先过 `validate_id_component`——`paths::instance_profile_dir`
/// 内部是 `expect`，一个非法 profile 名会让整个壳崩掉。
fn profile_modules_dir(family: &str, instance_id: &str, profile: &str) -> Option<PathBuf> {
    crate::shell::paths::validate_id_component(profile).ok()?;
    Some(
        crate::shell::paths::instance_profile_dir(family, instance_id, profile)
            .join("node_modules"),
    )
}

/// 当前机器上要排除的两个目录。
fn resolve_targets() -> Vec<Target> {
    let (family, instance_id) = crate::shell::instance::resolve_default();
    let data_dir = crate::kernel::lifecycle::data_dir(family);
    let profile =
        crate::shell::settings::load_for_shell(crate::shell::settings::current_mode()).profile;
    let profile_modules = profile_modules_dir(family, instance_id, &profile);
    targets(
        Some(&kernel_trees_dir(&data_dir)),
        profile_modules.as_deref(),
    )
}

/// 现在该不该自动加入。抽成纯函数是为了能逐条测：真实调用点里的每个输入都
/// 来自系统状态（注册表、磁盘记录、时钟），造不出来。
///
/// 三条「不再动」的理由各不相同，都不要合并：
///
/// - **已经加好了**（目标也没变）：再跑一次只会再弹一次 UAC。
/// - **用户拒绝过 / 在设置页移除过**：用户已经明确表过态，反复弹窗是在骚扰。
/// - **失败还没到重试时间**：Defender 没运行、篡改防护拦下这类失败不是瞬时的，
///   每次启动都撞一遍只会在日志里刷同样的错。
fn should_attempt(record: Option<&Record>, targets: &[String], now: u64) -> bool {
    let Some(record) = record else {
        return true;
    };
    match record.outcome.as_str() {
        OUTCOME_APPLIED => record.paths != targets,
        OUTCOME_REMOVED | OUTCOME_DECLINED => false,
        OUTCOME_FAILED => now.saturating_sub(record.at_unix) >= RETRY_AFTER.as_secs(),
        // 认不出的结果（旧版本写的、被手工改过）：按没试过处理。
        _ => true,
    }
}

/// 记录文件的位置。
fn record_path() -> PathBuf {
    crate::shell::paths::shell_dir(crate::shell::settings::current_mode()).join(STATE_FILE)
}

/// 读回记录。文件不存在 / 损坏都当成「没试过」——那正是再试一次的语义。
fn load_record() -> Option<Record> {
    crate::shell::state::load_lossy::<Option<Record>>(&record_path())
}

/// 落盘记录。写不进去只影响「下次会不会再试一次」，不该阻断调用方。
fn save_record(record: &Record) -> Option<Record> {
    crate::shell::state::save_best_effort(&record_path(), record);
    Some(record.clone())
}

/// 现在这一刻的 `(本地时刻, Unix 秒)`。
fn now_stamp() -> (String, u64) {
    let now = SystemTime::now();
    let at = format!(
        "{} {}",
        crate::shell::localtime::local_date_string(now),
        crate::shell::localtime::local_clock_string(now)
    );
    let unix = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    (at, unix)
}

/// 面板读到的现状（只读）。
pub fn status() -> Result<AvTrustStatus, String> {
    Ok(build_status())
}

/// 组装面板要显示的状态。
fn build_status() -> AvTrustStatus {
    AvTrustStatus {
        supported: cfg!(target_os = "windows"),
        platform_note: if cfg!(target_os = "windows") {
            String::new()
        } else {
            "本平台不需要：只有 Windows 的 Microsoft Defender 有「排除项」这回事。".to_string()
        },
        defender_present: platform::defender_present(),
        defender_realtime: platform::defender_realtime(),
        targets: resolve_targets()
            .into_iter()
            .map(|target| ExclusionTarget {
                label: target.label.to_string(),
                path: target.path.display().to_string(),
                exists: target.path.is_dir(),
            })
            .collect(),
        last: load_record().map(|record| LastAttempt {
            outcome: record.outcome,
            at: record.at,
            detail: record.detail,
            paths: record.paths,
        }),
    }
}

/// 起一条后台线程，在面板画出来之后自动把内核目录加进 Defender 排除项。
///
/// 线程名固定：任务管理器 / 调试器里一眼看出这条线程在干什么。起不来就退化
/// ——设置页那张卡上还能手动点。
pub(crate) fn spawn_auto() {
    // 非 Windows 上整段逻辑都不成立（没有 Defender、没有排除项）。
    if !cfg!(target_os = "windows") {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("dsh-defender-exclude".into())
        .spawn(|| {
            std::thread::sleep(AUTO_DELAY);
            auto_once();
        });
}

/// 自动加入的一次尝试。
///
/// 预期内的跳过（没有可排除的目录、Defender 没装 / 实时保护没开、已经加过、
/// 用户拒绝过）**不写日志**：这是启动路径上的后台动作，每次启动刷一行只会让
/// 「查看日志」变吵。只有真的触发了提权（成功或失败）才落
/// [`crate::shell::shell_events::record`]。
fn auto_once() {
    let wanted: Vec<String> = resolve_targets()
        .into_iter()
        .filter(|target| target.path.is_dir())
        .map(|target| target.path.display().to_string())
        .collect();
    if wanted.is_empty() {
        return;
    }
    if !platform::defender_present() || !platform::defender_realtime() {
        return;
    }
    if !should_attempt(load_record().as_ref(), &wanted, now_stamp().1) {
        return;
    }
    crate::shell::shell_events::record(
        EVENT_LOG,
        &format!("自动加入 Defender 排除项：{}", wanted.join("；")),
    );
    match apply_exclusion(true) {
        Ok(_) => {}
        Err(error) => crate::shell::shell_events::record(
            EVENT_LOG,
            &format!("自动加入 Defender 排除项未完成：{error}"),
        ),
    }
}

/// 加 / 删排除项（需要管理员，走 UAC 提权）。
///
/// 返回**生效后的状态**，让面板一次往返就能回显。失败时给的是带下一步的完整
/// 句子——GUI 程序里 `eprintln!` 没有去处，用户只拿得到这个字符串。
///
/// 无论成败都会更新记录文件：自动路径靠它决定「下次还试不试」，界面靠它
/// 说明「现在是什么状态」。
fn apply_exclusion(add: bool) -> Result<AvTrustStatus, String> {
    if !cfg!(target_os = "windows") {
        return Err(
            "本平台不需要：只有 Windows 的 Microsoft Defender 有「排除项」这回事。".to_string(),
        );
    }
    // Defender 没在运行时 `Add-/Remove-MpPreference` 只会回一个 0x800106ba：
    // 用户读不懂，UAC 也是白弹。先拦下，给能做的下一步。
    if !platform::defender_realtime() {
        return Err("Windows Defender 没有在运行（可能已被第三方杀软接管），\
             排除项对它不会起作用，这次没有请求管理员授权。\
             若要用 Defender，请先在「Windows 安全中心」里打开实时保护；\
             否则请到第三方杀软的信任区里手动添加设置页列出的两个目录。"
            .to_string());
    }
    let all = resolve_targets();
    // 加入时只排存在着的目录（Defender 拒收不存在的路径）；移除时不排——
    // 目录没了但排除项还在，正是移除该做的事。
    let paths: Vec<String> = if add {
        all.iter()
            .filter(|target| target.path.is_dir())
            .map(|target| target.path.display().to_string())
            .collect()
    } else {
        all.iter()
            .map(|target| target.path.display().to_string())
            .collect()
    };
    if paths.is_empty() {
        return Err(if add {
            "还没有可排除的目录：内核尚未安装，或实例依赖目录还不存在。\
             请先在「内核版本」页装好内核，再回到这里加入排除项。"
                .to_string()
        } else {
            "没有可移除的排除项：内核尚未安装。".to_string()
        });
    }

    let report_file = std::env::temp_dir().join(format!(
        "dsh-xlink-defender-{}-{}.txt",
        std::process::id(),
        now_stamp().1
    ));
    let script = elevated_script(add, &paths, &report_file.display().to_string());
    let encoded = encode_powershell(&outer_script(&encode_powershell(&script)));

    let mut cmd = crate::shell::process::command_with_path("powershell.exe");
    cmd.arg("-NoProfile").arg("-EncodedCommand").arg(&encoded);
    let (_ok, stdout, stderr) = crate::shell::process::run_command_capture_with_timeout(
        cmd,
        "powershell",
        ELEVATION_TIMEOUT,
    )
    .map_err(|error| {
        format!(
            "调用 PowerShell 失败：{error}。\
             请确认系统里有 powershell.exe（Windows 自带），再点一次；\
             若反复失败，打开「查看日志」把 av-trust 那几行发给维护者。"
        )
    })?;

    let report_text = std::fs::read_to_string(&report_file).unwrap_or_default();
    let _ = std::fs::remove_file(&report_file);
    let report = parse_report(report_text.trim_start_matches('\u{feff}'));

    let mut exit_code: Option<i32> = None;
    let mut launch_error: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("EXITCODE=") {
            exit_code = rest.trim().parse::<i32>().ok();
        } else if let Some(rest) = line.strip_prefix("LAUNCHERROR=") {
            launch_error = Some(rest.trim().to_string());
        }
    }

    if let Some(message) = launch_error {
        // UAC 被拒：记成「用户拒绝」，之后不再自动弹窗——用户的表态要算数。
        let (at, at_unix) = now_stamp();
        save_record(&Record {
            paths: Vec::new(),
            outcome: OUTCOME_DECLINED.to_string(),
            at,
            at_unix,
            detail: message.clone(),
        });
        return Err(format!(
            "系统没有批准管理员授权（UAC）：{message}。\
             请重新点一次，并在弹出的授权窗口里选「是」（拒绝之后不会再自动尝试）；\
             若不想授权，可手动在「Windows 安全中心 → 病毒和威胁防护 → 管理设置 → 排除项」\
             里添加上面列出的目录。"
        ));
    }

    if !report.failures.is_empty() {
        let detail = report.failures.join("；");
        record_failure(&detail);
        return Err(format!(
            "Defender 拒绝了这次改动：{detail}。\
             下一步：确认「Windows 安全中心」里 Defender 正在运行、且「篡改防护」没有拦下这次修改；\
             也可以手动在「病毒和威胁防护 → 管理设置 → 排除项」里添加上面列出的目录。"
        ));
    }
    if exit_code.is_some_and(|code| code != 0) {
        // stderr 里常混着 PowerShell 的 CLIXML 进度噪声（模块首次加载），那不是
        // 给人看的错误——滤掉，只留正常文本的前一小段。
        let detail: String = stderr
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with('<') && !trimmed.starts_with("#<")
            })
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(300)
            .collect();
        record_failure(&format!("退出码 {exit_code:?}，stderr：{detail}"));
        return Err(format!(
            "排除项改动没有成功（提权进程退出码 {exit_code:?}）。\
             {detail}。下一步：确认「Windows 安全中心 → 病毒和威胁防护」里的 Defender 正在运行，\
             然后重试；仍失败请打开「查看日志」把 av-trust 那几行发给维护者。"
        ));
    }
    if report.accepted.is_empty() {
        record_failure("提权进程没有回报逐条结果（报告为空）");
        return Err("这次没有改动任何排除项（提权进程没有回报结果）。\
             下一步：确认「Windows 安全中心 → 病毒和威胁防护」里的 Defender 正在运行，然后重试；\
             也可以手动在「管理设置 → 排除项」里添加上面列出的目录。"
            .to_string());
    }

    let target_paths = if add { report.accepted } else { Vec::new() };
    let (at, at_unix) = now_stamp();
    save_record(&Record {
        paths: target_paths,
        outcome: if add {
            OUTCOME_APPLIED.to_string()
        } else {
            OUTCOME_REMOVED.to_string()
        },
        at,
        at_unix,
        detail: String::new(),
    });
    crate::shell::shell_events::record(
        EVENT_LOG,
        &format!(
            "已{} Defender 排除项：{}",
            if add { "加入" } else { "移除" },
            paths.join("；")
        ),
    );
    Ok(build_status())
}

/// 记一次失败：24 小时后再自动重试。
fn record_failure(detail: &str) {
    let (at, at_unix) = now_stamp();
    save_record(&Record {
        paths: Vec::new(),
        outcome: OUTCOME_FAILED.to_string(),
        at,
        at_unix,
        detail: detail.to_string(),
    });
}

/// 提权进程回报的逐条结果。
#[derive(Debug, Default, PartialEq, Eq)]
struct Report {
    /// 这条路径现在处于要的状态（本次改成功，或本来就已经是）。
    accepted: Vec<String>,
    /// 改失败的（路径 + 原因）。
    failures: Vec<String>,
}

/// 解析提权进程写回的报告文件。每行 `标记 \t 路径 [\t 原因]`。
fn parse_report(text: &str) -> Report {
    let mut report = Report::default();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        match (parts.next(), parts.next()) {
            (Some("OK" | "SKIP"), Some(path)) => report.accepted.push(path.to_string()),
            (Some("ERR"), Some(path)) => {
                let reason = parts.next().unwrap_or("未知原因").trim();
                report.failures.push(format!("{path}：{reason}"));
            }
            _ => {}
        }
    }
    report
}

/// PowerShell 单引号串：`'` 要写成 `''`。
fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 提权后那段 PowerShell（加 / 删排除项）。
///
/// 逐条 try/catch 并把结果写进报告文件：提权进程的 stdout 落在它自己的
/// 控制台里，我们拿不到，只有文件能穿回来。
///
/// 两条「先看一眼再动手」的判据都不能省：`Add-MpPreference` 对已经排过的路径
/// 不保证不报错，`Remove-MpPreference` 对没排过的路径**一定会**报错——而两者
/// 报出来的错都会让用户以为出了故障。已经在目标状态上的直接记 `SKIP`。
fn elevated_script(add: bool, paths: &[String], report_path: &str) -> String {
    let list = paths
        .iter()
        .map(|path| ps_quote(path))
        .collect::<Vec<_>>()
        .join(", ");
    let action = if add {
        "Add-MpPreference"
    } else {
        "Remove-MpPreference"
    };
    let skip = if add {
        "($current -contains $p)"
    } else {
        "($current -notcontains $p)"
    };
    format!(
        "$ErrorActionPreference = 'Continue'\n\
$lines = @()\n\
$current = @()\n\
try {{ $current = @(Get-MpPreference).ExclusionPath }} catch {{ $current = @() }}\n\
foreach ($p in @({list})) {{\n\
  try {{\n\
    if ({skip}) {{ $lines += ('SKIP' + \"`t\" + $p) }}\n\
    else {{ {action} -ExclusionPath $p -ErrorAction Stop; $lines += ('OK' + \"`t\" + $p) }}\n\
  }}\n\
  catch {{ $lines += ('ERR' + \"`t\" + $p + \"`t\" + $_.Exception.Message) }}\n\
}}\n\
Set-Content -LiteralPath {report} -Value $lines -Encoding UTF8\n",
        list = list,
        skip = skip,
        action = action,
        report = ps_quote(report_path),
    )
}

/// 外层那段 PowerShell：起一个提权的 `powershell.exe` 跑内层脚本，等它结束，
/// 把退出码写回 stdout。UAC 被拒时 `Start-Process` 抛错，这里接住并回报。
fn outer_script(inner_encoded: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
try {{\n\
  $p = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{encoded}' -Verb RunAs -Wait -PassThru -WindowStyle Hidden\n\
  Write-Output ('EXITCODE=' + $p.ExitCode)\n\
}}\n\
catch {{\n\
  Write-Output ('LAUNCHERROR=' + $_.Exception.Message)\n\
}}\n",
        encoded = inner_encoded,
    )
}

/// 把一段 PowerShell 脚本编成 `-EncodedCommand` 的载荷（UTF-16LE + base64）。
///
/// 只含 ASCII，因此不受引号规则与 cmd 转发规则影响——路径里有空格、单引号、
/// 中文都不会被拆开。
fn encode_powershell(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 面板读到的现状（只读）。
#[tauri::command]
pub async fn av_trust_status() -> Result<AvTrustStatus, String> {
    crate::commands::blocking(status).await
}

/// 手动加入 / 移除 Defender 排除项，返回生效后的状态。
///
/// 加入本来由启动时的后台线程自动做一次；这个命令是「自动那次被拒 / 失败」
/// 之后的重试入口，以及**唯一**的撤销入口。
#[tauri::command]
pub async fn av_trust_set_exclusion(enabled: bool) -> Result<AvTrustStatus, String> {
    crate::commands::blocking(move || apply_exclusion(enabled)).await
}

/// 平台相关的只读探测。
#[cfg(target_os = "windows")]
mod platform {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    };

    /// 实时保护开关（1 = 关）。
    const REALTIME_KEY: &str = r"SOFTWARE\Microsoft\Windows Defender\Real-Time Protection";
    const REALTIME_VALUE: &str = "DisableRealtimeMonitoring";
    /// 「禁用 Defender」策略（1 = Defender 整个不跑）。
    const DISABLE_ANTISPYWARE_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows Defender";
    /// 主键里同类开关（1 = 关）。政策键不存在时它们才是唯一线索：第三方杀软
    /// 接管与优化工具改的就是这里（2026-11-02 本机实测：服务停着、
    /// `Get-MpPreference` 报 0x800106ba，而这两个值都是 1）。
    const DISABLE_LOCAL_KEY: &str = r"SOFTWARE\Microsoft\Windows Defender";
    const DISABLE_ANTISPYWARE_VALUE: &str = "DisableAntiSpyware";
    const DISABLE_ANTIVIRUS_VALUE: &str = "DisableAntiVirus";
    /// Defender 的驱动服务：它不在就不是 Defender。
    const DEFENDER_SERVICE: &str = r"SYSTEM\CurrentControlSet\Services\WinDefend";

    /// 打开 HKLM 下的一个键（只读）。
    fn open_key(subkey: &str) -> Option<HKEY> {
        let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: 只写 `key`；参数是本函数持有的 NUL 结尾缓冲。
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide.as_ptr(), 0, KEY_READ, &mut key) };
        (status == 0).then_some(key)
    }

    /// 键存在吗？
    fn key_exists(subkey: &str) -> bool {
        match open_key(subkey) {
            Some(key) => {
                // SAFETY: `key` 由 `open_key` 返回且未关闭过。
                unsafe { RegCloseKey(key) };
                true
            }
            None => false,
        }
    }

    /// 读一个 REG_DWORD 值。
    fn read_dword(subkey: &str, value: &str) -> Option<u32> {
        let key = open_key(subkey)?;
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let mut kind = 0u32;
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `data` 恰好 4 字节，`size` 是 in/out。
        let status = unsafe {
            RegQueryValueExW(
                key,
                wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                (&mut data as *mut u32).cast::<u8>(),
                &mut size,
            )
        };
        // SAFETY: `key` 有效且尚未关闭。
        unsafe { RegCloseKey(key) };
        if status != 0 || kind != REG_DWORD {
            return None;
        }
        Some(data)
    }

    /// 系统里有没有 Defender。
    pub(super) fn defender_present() -> bool {
        key_exists(DEFENDER_SERVICE)
    }

    /// 「Defender 整个被关掉」的三个注册表值任一为 1：政策键，加上主键里的
    /// 两个（第三方杀软接管 / 优化工具写的那一对）。
    fn defender_disabled() -> bool {
        [
            (DISABLE_ANTISPYWARE_KEY, DISABLE_ANTISPYWARE_VALUE),
            (DISABLE_LOCAL_KEY, DISABLE_ANTISPYWARE_VALUE),
            (DISABLE_LOCAL_KEY, DISABLE_ANTIVIRUS_VALUE),
        ]
        .iter()
        .any(|(key, value)| read_dword(key, value) == Some(1))
    }

    /// 实时保护是否开着的**判定规则**（纯函数，读数由调用方注入）。
    ///
    /// 任一读数命中就是「没开着」：系统里没有 Defender；`DisableAntiSpyware` /
    /// `DisableAntiVirus` 把它整个关掉（被第三方杀软接管时 `MpPreference`
    /// 一律返回 0x800106ba）；或实时保护开关是 1。实时保护开关**缺省按开着**
    /// 处理——那是 Defender 的默认。
    pub(super) fn realtime_from(
        present: bool,
        defender_disabled: bool,
        disable_realtime: Option<u32>,
    ) -> bool {
        if !present || defender_disabled {
            return false;
        }
        match disable_realtime {
            Some(value) => value == 0,
            None => true,
        }
    }

    /// 从本机注册表读三组事实再套 [`realtime_from`]。
    pub(super) fn defender_realtime() -> bool {
        realtime_from(
            defender_present(),
            defender_disabled(),
            read_dword(REALTIME_KEY, REALTIME_VALUE),
        )
    }
}

/// 平台相关的只读探测。非 Windows 上什么都没有，界面显示「本平台不需要」。
#[cfg(not(target_os = "windows"))]
mod platform {
    pub(super) fn defender_present() -> bool {
        false
    }

    pub(super) fn defender_realtime() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    /// 目标目录只收**第三方包代码**：内核安装树父目录 + 实例 profile 的
    /// node_modules。少一个就少一份收益，多一个就越界（用户数据与凭据不该被免检）。
    #[test]
    fn targets_are_the_two_third_party_code_directories() {
        let list = targets(
            Some(Path::new("/data/kernels")),
            Some(Path::new("/data/profiles/web/node_modules")),
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].label, "内核目录");
        assert_eq!(list[0].path, Path::new("/data/kernels"));
        assert_eq!(list[1].label, "实例依赖包");
        assert!(list[1].path.ends_with("node_modules"));
    }

    /// 缺哪一项都只给剩下的那一个，不许拿别的东西顶替。
    #[test]
    fn targets_skip_the_missing_ones() {
        assert!(targets(None, None).is_empty());
        assert_eq!(targets(None, Some(Path::new("/m"))).len(), 1);
        assert_eq!(targets(Some(Path::new("/k")), None).len(), 1);
    }

    /// 排除的是**所有版本共用的那一层**：它必须正好是 `kernel_dir` 的父目录。
    /// 内核升级会换版本目录名，排 `<版本>` 那一层等于每次升级都失效一次。
    #[test]
    fn kernel_trees_dir_covers_every_installed_version() {
        let data_dir = Path::new(r"C:\x\desktop");
        assert_eq!(
            kernel_trees_dir(data_dir).join("0.2.2-alpha.2"),
            crate::kernel::lifecycle::kernel_dir(data_dir, "0.2.2-alpha.2")
        );
    }

    /// 实例依赖包目录必须落在实例的 profile 下，且**不能**把实例数据目录的
    /// 其余部分（会话、凭据）圈进来。
    #[test]
    fn profile_modules_dir_is_under_the_profile() {
        let home = std::env::temp_dir().join(format!(
            "dsh-defender-home-{}-{}",
            std::process::id(),
            now_stamp().1
        ));
        let _guard = scoped_xlink_home(&home);
        let dir = profile_modules_dir("dsh", "default", "web").expect("合法 profile");
        assert!(
            dir.ends_with("profiles/web/node_modules"),
            "{}",
            dir.display()
        );
        assert!(dir.starts_with(&home), "路径必须在 xlink_home 下");
        assert!(
            !dir.display().to_string().contains("sessions"),
            "不能把会话目录圈进排除项：{}",
            dir.display()
        );
    }

    /// 非法 profile 名不能让整个壳崩掉（`paths::instance_profile_dir` 内部是
    /// `expect`），这里必须安静地跳过这一项。
    #[test]
    fn profile_modules_dir_rejects_invalid_profile() {
        assert!(profile_modules_dir("dsh", "default", "../escape").is_none());
        assert!(profile_modules_dir("dsh", "default", "").is_none());
    }

    /// 自动加入的闸门：每条「不再自动动」的理由各自成立，也各自**不许**扩张——
    /// 已经加好却还去加，用户每次启动都要点一次 UAC；用户明确移除过却还加回来，
    /// 那个按钮就是骗人的。
    #[test]
    fn auto_attempt_stops_after_the_user_has_spoken() {
        let wanted = vec![r"C:\data\kernels".to_string()];
        let record = |outcome: &str, paths: Vec<String>, at_unix: u64| Record {
            paths,
            outcome: outcome.to_string(),
            at: "2026-11-02 09:00:00".to_string(),
            at_unix,
            detail: String::new(),
        };

        // 没试过 → 试。
        assert!(should_attempt(None, &wanted, 1_000));

        // 已经加好、目标没变 → 不再动。
        assert!(!should_attempt(
            Some(&record(OUTCOME_APPLIED, wanted.clone(), 1)),
            &wanted,
            1_000
        ));
        // 加过但目标变了（换实例 / 换 profile / 数据目录搬了）→ 重新加。
        assert!(should_attempt(
            Some(&record(OUTCOME_APPLIED, vec![r"C:\old".to_string()], 1)),
            &wanted,
            1_000
        ));

        // 用户拒绝授权、或在设置页点过「移除」→ 永远不再自动加回来。
        for outcome in [OUTCOME_DECLINED, OUTCOME_REMOVED] {
            assert!(
                !should_attempt(Some(&record(outcome, Vec::new(), 1)), &wanted, 1_000),
                "{outcome} 之后不许自动重试"
            );
        }

        // 其它失败 → 隔一天再试，不是每次启动都撞一遍。
        let failed = record(OUTCOME_FAILED, Vec::new(), 1_000);
        assert!(!should_attempt(Some(&failed), &wanted, 1_000 + 60));
        assert!(should_attempt(
            Some(&failed),
            &wanted,
            1_000 + RETRY_AFTER.as_secs()
        ));

        // 认不出的结果（旧版本写的）→ 当作没试过。
        assert!(should_attempt(
            Some(&record("whatever", Vec::new(), 1)),
            &wanted,
            1_000
        ));
    }

    /// 记录要能落盘再读回来：自动路径靠它决定下次还试不试。写不进去时下游
    /// 只是「再试一次」，不该抛。
    #[test]
    fn record_round_trips_through_the_shell_state_file() {
        let home = std::env::temp_dir().join(format!(
            "dsh-defender-state-{}-{}",
            std::process::id(),
            now_stamp().1
        ));
        let _guard = scoped_xlink_home(&home);
        assert!(load_record().is_none(), "没有文件时应当读成「没试过」");

        let record = Record {
            paths: vec![r"C:\data\kernels".to_string()],
            outcome: OUTCOME_APPLIED.to_string(),
            at: "2026-11-02 09:00:00".to_string(),
            at_unix: 1_000,
            detail: String::new(),
        };
        save_record(&record);
        let loaded = load_record().expect("刚写的记录应当读得回来");
        assert_eq!(loaded.paths, record.paths);
        assert_eq!(loaded.outcome, OUTCOME_APPLIED);

        // 文件损坏 → 当作没试过（再试一次的语义），不许 panic。
        std::fs::write(record_path(), b"{ not json").expect("写坏文件");
        assert!(load_record().is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    /// PowerShell 单引号串里的 `'` 要加倍，否则路径里的单引号会提前闭合字符串。
    #[test]
    fn ps_quote_doubles_single_quotes() {
        assert_eq!(ps_quote(r"C:\a'b"), r"'C:\a''b'");
        assert_eq!(ps_quote(r"C:\a"), r"'C:\a'");
    }

    /// 加与删必须是官方那两条命令，且路径逐字进去；已经处于目标状态的要走
    /// `SKIP`——`Remove-MpPreference` 对没排过的路径一定会报错。
    #[test]
    fn elevated_script_uses_the_official_cmdlets() {
        let add = elevated_script(true, &[r"C:\data\kernels".to_string()], r"C:\t\r.txt");
        assert!(add.contains("Add-MpPreference -ExclusionPath $p"));
        assert!(!add.contains("Remove-MpPreference"));
        assert!(add.contains("($current -contains $p)"), "已排过的要跳过");
        assert!(add.contains(r"'C:\data\kernels'"));
        assert!(add.contains(r"'C:\t\r.txt'"), "报告路径要进脚本");

        let remove = elevated_script(false, &[r"C:\data\kernels".to_string()], r"C:\t\r.txt");
        assert!(remove.contains("Remove-MpPreference -ExclusionPath $p"));
        assert!(!remove.contains("Add-MpPreference"));
        assert!(
            remove.contains("($current -notcontains $p)"),
            "移除前要先看它在不在"
        );
    }

    /// 报告文件的三种行：改成功 / 本来就对 / 失败（带原因）。
    #[test]
    fn report_lines_map_to_accepted_and_failed() {
        let report = parse_report("OK\tC:\\a\nSKIP\tC:\\b\nERR\tC:\\c\t拒绝访问\n乱码行\n");
        assert_eq!(
            report.accepted,
            vec![r"C:\a".to_string(), r"C:\b".to_string()]
        );
        assert_eq!(report.failures, vec![r"C:\c：拒绝访问".to_string()]);
        // 空报告（提权进程什么也没写）不能当成「都成功了」。
        assert!(parse_report("").accepted.is_empty());
    }

    /// 载荷必须是纯 ASCII 的 base64（UTF-16LE）：路径里有空格、单引号、中文
    /// 都不会被 cmd / PowerShell 的转发规则拆开。
    #[test]
    fn encoded_payload_is_ascii_base64_of_utf16le() {
        use base64::Engine;
        let script = elevated_script(true, &[r"C:\我的 目录\kernels".to_string()], r"C:\t\r.txt");
        let encoded = encode_powershell(&script);
        assert!(encoded.is_ascii(), "载荷必须是 ASCII");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .expect("应能解回 base64");
        let expected: Vec<u8> = script
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        assert_eq!(bytes, expected, "载荷必须是脚本的 UTF-16LE 字节");
    }

    /// 外层脚本必须用 `-Verb RunAs` 提权，并把退出码回报出来；UAC 被拒要有话可说。
    #[test]
    fn outer_script_elevates_and_reports_the_exit_code() {
        let outer = outer_script("QUJD");
        assert!(outer.contains("-Verb RunAs"));
        assert!(outer.contains("EXITCODE="));
        assert!(outer.contains("LAUNCHERROR="), "UAC 被拒也要有话可说");
        assert!(outer.contains("'QUJD'"));
    }

    /// FIX-10：实时保护的**判定规则**用注入读数做确定性覆盖。真实机器探测只当
    /// 诊断信息，不作 CI 通过条件——hosted runner 会关掉实时保护。
    #[cfg(target_os = "windows")]
    #[test]
    fn defender_realtime_fixtures_are_deterministic() {
        use platform::realtime_from;
        // 没有 Defender：无论其它读数如何都不是「开着」。
        assert!(!realtime_from(false, false, None));
        assert!(!realtime_from(false, false, Some(0)));
        // 被第三方杀软 / 优化工具整个关掉：主键与政策键都算。
        assert!(!realtime_from(true, true, None));
        assert!(!realtime_from(true, true, Some(0)));
        // 实时保护开关：缺省按开着；0 = 开着；1 = 关。
        assert!(realtime_from(true, false, None));
        assert!(realtime_from(true, false, Some(0)));
        assert!(!realtime_from(true, false, Some(1)));
        // 意外的 DWORD 值不猜成「开着」。
        assert!(!realtime_from(true, false, Some(2)));
    }

    /// 真实机器探测只做诊断（值取决于本机策略，不作判据）。
    #[cfg(target_os = "windows")]
    #[test]
    fn registry_reads_only_answer_presence_questions() {
        let present = platform::defender_present();
        let realtime = platform::defender_realtime();
        assert!(!realtime || present, "实时保护开着时 Defender 必须存在");
    }
}
