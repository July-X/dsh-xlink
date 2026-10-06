//! 统一「运行记录」：把启动内核、插件预检、恢复快照、二分排查收成同一份
//! 可追溯的时间线。
//!
//! ## 为什么要有这一层
//!
//! 三项诊断功能（启动诊断 / 插件安全诊断 / 内核状态诊断）原本各答一个问题，
//! 数据却来自三处互不相认的现场：`guard.rs` 的 attempts、`precheck.rs` 的
//! `PrecheckReport`、`snapshot.rs` 的快照索引。用户看到的是「启动失败」，拿
//! 到的却是散落在三处的字符串，既无法排序也无法定位。**一次操作 = 一条记录**
//! 让这三件事共享契约：事件按 [`seq`] 定序，状态由 [`RunStatus`] 统一表达，
//! 归因由 [`cause`] 统一表达。
//!
//! ## 三条不可让步的纪律
//!
//! 1. **记录写失败绝不改变主流程结果。** [`Recorder`] 的每个落盘点都是
//!    `let _ = …`，失败只落 [`crate::shell::shell_events::record`]。启动成功
//!    就必须是启动成功——不能因为「顺手记一笔」失败而把用户的启动判成失败。
//! 2. **记录不包含凭据与会话正文。** [`sanitize`] 折叠 home 前缀、剥掉常见
//!    令牌形状、截断超长字段。这份 JSON 会被导出、可能被用户贴进求助帖。
//! 3. **未知取值原样保留。** 新版壳写了 `stage: "health-probe"` 给旧壳读，
//!    旧壳必须显示「未知阶段」而不是丢掉这一行——丢掉会让时间线出现一个洞，
//!    而用户看到的正是那个洞最费解。
//!
//! 设计见 `docs/runtime-diagnostics-design.md` §4。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::shell::error::AppError;
use crate::shell::state::StateCtx;

/// 保留的运行摘要份数。20 条足够覆盖「今天出了什么事」，而每条摘要与详情
/// 都有明确上界（事件数见 [`MAX_EVENTS`]）。
pub const MAX_RUNS: usize = 20;
/// 单条运行记录的事件数上限。一次启动或预检只有十几个阶段，留这个上限是为了
/// 兜住「把每一行日志都变成事件」这种误接法——那会让记录膨胀到几十 MB。
pub const MAX_EVENTS: usize = 200;
/// 文档结构版本。
const SCHEMA: u32 = 1;

/// 运行记录的类型。
pub mod kind {
    /// 启动工作台。
    pub const STARTUP: &str = "startup";
    /// 插件安装预检。
    pub const PLUGIN_PRECHECK: &str = "plugin-precheck";
    /// 从快照恢复。
    pub const RESTORE: &str = "restore";
    /// 二分排查。
    pub const BISECT: &str = "bisect";
}

/// 运行记录的状态。
pub mod status {
    /// 正在进行。
    pub const RUNNING: &str = "running";
    /// 成功。
    pub const SUCCESS: &str = "success";
    /// 成功但带告警。
    pub const WARNING: &str = "warning";
    /// 失败。
    pub const FAILURE: &str = "failure";
    /// 证据不足，无法判断。
    pub const INCONCLUSIVE: &str = "inconclusive";
    /// 用户中止。
    pub const CANCELED: &str = "canceled";
}

/// 归因类别。沿用 `Incident::cause` 的语义，**未知取值原样保留**。
pub mod cause {
    /// 环境类（端口被占、目录不可写、ABI 不匹配）。
    pub const ENVIRONMENT: &str = "environment";
    /// 内核自身。
    pub const KERNEL: &str = "kernel";
    /// 插件。
    pub const PLUGIN: &str = "plugin";
    /// 技能。
    pub const SKILL: &str = "skill";
    /// 工作台前端。
    pub const FRONTEND: &str = "frontend";
    /// 无法归因。
    pub const UNKNOWN: &str = "unknown";
}

/// 启动阶段标识。**稳定英文**，中文显示名集中放在前端
/// `diagnostic-labels.js`——文案要改不该牵动落盘格式。
pub mod stage {
    pub const RESOLVE_INSTANCE: &str = "resolve-instance";
    pub const DETECT_NODE: &str = "detect-node";
    pub const RESOLVE_PNPM: &str = "resolve-pnpm";
    pub const PREPARE_WIRING: &str = "prepare-wiring";
    pub const SPAWN_KERNEL: &str = "spawn-kernel";
    pub const WAIT_READY: &str = "wait-ready";
    pub const HEALTH_CHECK: &str = "health-check";
    pub const RECORD_STARTUP_OK: &str = "record-startup-ok";
    /// 看护重试（停用嫌疑插件 / 安全模式）。
    pub const RETRY: &str = "retry";
    /// 预检：建沙盒。
    pub const SANDBOX_CREATE: &str = "sandbox-create";
    /// 预检：基线启动。
    pub const BASELINE: &str = "baseline";
    /// 预检：装入候选插件。
    pub const INSTALL_CANDIDATE: &str = "install-candidate";
    /// 预检：探测候选。
    pub const PROBE_CANDIDATE: &str = "probe-candidate";
    /// 预检：出报告。
    pub const REPORT: &str = "report";
}

/// 看护报告的阶段。
///
/// **放在模型层而不是看护里**：看护只负责说「发生了什么」，而「怎么说、
/// 在哪落盘」是诊断层的事。枚举若留在 `guard.rs`，`startup_run` 就要反过来
/// 引用看护，形成环；放在这里两边都只依赖模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchPhase {
    /// 工作台本来就在跑，本次未重复启动。
    AlreadyRunning,
    /// 开始派生内核进程。
    Spawning,
    /// 端口上已经有东西在应答。
    AlreadyServing,
    /// 进程已派生，等端口就绪。
    WaitingReady,
    /// 端口已应答。
    PortReady,
    /// 派生失败。
    SpawnFailed,
    /// 就绪前退出。
    Exited,
    /// 等待就绪超时。
    TimedOut,
    /// 正在停用嫌疑插件后重试。
    RetryPlugins,
    /// 正在进入安全模式后重试。
    RetrySafeMode,
    /// 环境类问题，已跳过插件归因。
    EnvironmentBlocked,
    /// 正在准备插件接线。
    Wiring,
    /// 插件接线已就绪。
    WiringReady,
}

/// 一次运行中的单个阶段事件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    /// 同一毫秒内的定序键。**不能只按时间戳排序**：两个阶段可能落在同一毫秒，
    /// 排序不稳定会让时间线在刷新时跳动。
    pub seq: u32,
    /// [`stage`] 里的标识；未知值原样保留。
    pub stage: String,
    /// 该阶段的结果，取值同 [`status`]；未知值原样保留。
    pub status: String,
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// 面向用户的一句话。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    /// 第几次尝试。看护的三次尝试必须分开标，否则时间线会把
    /// 「失败→停用插件→成功」画成一条看似连续的流程。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}

/// 运行记录的证据入口。只存路径与摘要，不存正文。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunEvidence {
    /// 内核日志路径。
    ///
    /// **刻意叫 `kernel_log` 而不是 `log_path`**：`Incident` 里有一个同名
    /// 但**不参与 camelCase 改名**的 `log_path`，而 `check-invariants` 的
    /// ipc-fields 项按**字段名**（不按结构）扫全前端：同名会让它把
    /// `store.js` 里合法的 `incident.log_path` 报成「读错字段名」。两个
    /// 结构用不同的名字，比让门禁学会区分结构便宜得多。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_log: Option<String>,
    /// 关联的事故面板记录（`last-incident.json` 的那次）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    /// 沙盒 / 预检保留的证据路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_log: Option<String>,
}

/// 一条运行记录的完整详情（单次运行一个文件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRun {
    #[serde(default)]
    pub schema: u32,
    pub id: String,
    /// [`kind`] 里的取值之一。
    pub kind: String,
    pub family: String,
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kernel_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile: String,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    /// 取值同 [`status`]。
    pub status: String,
    /// 取值同 [`cause`]，未知值原样保留。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cause: String,
    /// 一句话结论。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default)]
    pub evidence: RunEvidence,
    #[serde(default)]
    pub events: Vec<DiagnosticEvent>,
}

/// 列表里用的摘要。**不带 events**：概览与列表不需要几十个事件，
/// 详情页再按 id 读单文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cause: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    /// 事件条数，供列表显示「完成 N 个阶段」。
    #[serde(default)]
    pub event_count: usize,
}

/// 索引文档（`index.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunIndex {
    #[serde(default)]
    pub schema: u32,
    /// 倒序（最新在前）。
    #[serde(default)]
    pub entries: Vec<RunSummary>,
}

impl RunIndex {
    fn is_compatible(&self) -> bool {
        self.schema <= SCHEMA
    }
}

fn ctx() -> StateCtx {
    StateCtx {
        corrupt: |reason| {
            format!(
                "运行记录索引无法解析：{reason}。诊断历史暂时读不到；下一次运行会重建索引，\
                 但覆盖前请先确认当前实例的诊断记录不需要保留"
            )
        },
        kind: |reason| AppError::Io(format!("运行记录索引无法解析，已拒绝覆盖：{reason}")),
    }
}

// --- 存储 ------------------------------------------------------------------
/// `<instance_dir>/diagnostics/runs/`。
pub fn runs_dir(family: &str, instance: &str) -> PathBuf {
    crate::shell::paths::instance_diagnostics_runs_dir(family, instance)
}

fn index_path(family: &str, instance: &str) -> PathBuf {
    runs_dir(family, instance).join("index.json")
}

fn run_path(family: &str, instance: &str, run_id: &str) -> PathBuf {
    runs_dir(family, instance).join(format!("{run_id}.json"))
}

/// 展示路径用的容错读。
pub fn load_index(family: &str, instance: &str) -> RunIndex {
    let index: RunIndex = crate::shell::state::load_lossy(&index_path(family, instance));
    if index.is_compatible() {
        return index;
    }
    RunIndex {
        schema: SCHEMA,
        ..RunIndex::default()
    }
}

/// 读-改-写路径：索引损坏时报错，不覆盖用户记录。
fn load_index_checked(family: &str, instance: &str) -> Result<RunIndex, AppError> {
    let index: RunIndex = crate::shell::state::load_checked(&index_path(family, instance), ctx())?;
    if !index.is_compatible() {
        return Err(AppError::Io(format!(
            "运行记录索引结构版本 {} 高于本版支持的 {SCHEMA}，请升级桌面端后再查看诊断记录",
            index.schema
        )));
    }
    Ok(index)
}

/// 读一条运行详情。索引里没有这条、或详情文件丢失时返回 `None`。
pub fn get(family: &str, instance: &str, run_id: &str) -> Option<DiagnosticRun> {
    crate::shell::state::load_lossy(&run_path(family, instance, run_id))
}

/// 最近一条指定类型的记录。用于「查看上一次启动诊断」。
pub fn latest(family: &str, instance: &str, kind: &str) -> Option<RunSummary> {
    load_index(family, instance)
        .entries
        .into_iter()
        .find(|entry| entry.kind == kind)
}

/// 脱敏：折叠 home 前缀 + 剥掉常见令牌形状 + 长度封顶。
///
/// 运行记录会被导出、可能被用户整份贴进求助帖。折叠 `~/` 之外的完整 home
/// 路径是隐私要求（截图求助时暴露用户名），令牌形状则是**内容**要求——
/// 日志里出现 `sk-…` 的概率不为零，而这份 JSON 的读者是用户和论坛。
///
/// **home 取 [`crate::shell::paths::dirs_home`] 而不是裸读 `$HOME`**：Windows
/// 上 `HOME` 常常根本不设（只有 `USERPROFILE`），裸读会让
/// `C:\Users\用户名\…` 原样留在记录里，而内核日志路径恰恰来自那里。2026-10-06
/// 审查（docs/runtime-diagnostics-review-2026-10-06.md P1-07）抓到的就是这一条。
/// 路径层那份解析已经处理了 Unix / Windows 两边，这里不重写。
pub fn sanitize(text: &str) -> String {
    sanitize_with_roots(text, &home_roots())
}

/// 需要折叠的路径根：用户 home，加上可能覆盖它的 `DSH_XLINK_HOME`。
fn home_roots() -> Vec<PathBuf> {
    let mut roots = vec![crate::shell::paths::dirs_home()];
    if let Some(custom) = std::env::var_os("DSH_XLINK_HOME") {
        roots.push(PathBuf::from(custom));
    }
    roots
}

/// 真正的折叠逻辑，**不读环境**。
///
/// 拆出来不是为了好看：它让 Windows / 自定义数据根这两条路径能被**纯函数
/// 测试**覆盖。第一版是直接 `set_var("HOME")` / `remove_var("HOME")` 写测试的，
/// 结果那条 `remove_var` 漏到了并发的 `shell::autostart` 测试里（它断言
/// 「HOME 一定存在」），一次 `cargo test` 红了四条不相关的用例。仓库自己的
/// 纪律写着「裸 `std::env::set_var` 拦不住别的测试正持着同一把 env 锁」——
/// 这次是它自己撞上了。
fn sanitize_with_roots(text: &str, roots: &[PathBuf]) -> String {
    const MAX_CHARS: usize = 480;
    // 长的先折：两者有包含关系时（数据根就在 home 下），先折短的会让长的
    // 永远匹配不上——`~/…` 已经把前缀替掉了。
    let mut ordered: Vec<&PathBuf> = roots.iter().collect();
    ordered.sort_by_key(|path| std::cmp::Reverse(path.display().to_string().len()));
    let mut out = text.to_string();
    for root in ordered {
        let root = root.display().to_string();
        // 长度 1 的根（Windows 的 `C:\`、某些环境下的 `/`）折掉会把整台机器
        // 上所有路径都变成 `~/…`，比不折更糟。
        if root.len() > 1 {
            out = out.replace(&root, "~");
        }
    }
    out = strip_token_shapes(&out);
    if out.chars().count() > MAX_CHARS {
        let mut truncated: String = out.chars().take(MAX_CHARS).collect();
        truncated.push('…');
        return truncated;
    }
    out
}

/// 剥掉看起来像凭据的片段。
///
/// 只处理**形状明确**的几类（`sk-` 开头的 key、`Bearer ` 头、赋值形态的
/// token/secret）。刻意不做通用正则去匹配任意长随机串——那会把正常的
/// 版本号、指纹、端口号也吃掉，反而把证据毁掉。
fn strip_token_shapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // `sk-` 开头的 key：DeepSeek / OpenAI 风格的最小前缀。
        if chars[i] == 's' && chars.get(i + 1) == Some(&'k') && chars.get(i + 2) == Some(&'-') {
            let mut j = i + 3;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '-') {
                j += 1;
            }
            if j - i > 8 {
                out.push_str("sk-***");
                i = j;
                continue;
            }
        }
        // `Bearer <token>`
        if chars[i] == 'B' && text_at(&chars, i, "Bearer ") {
            let mut j = i + 7;
            while j < chars.len() && !chars[j].is_whitespace() {
                j += 1;
            }
            if j > i + 7 {
                out.push_str("Bearer ***");
                i = j;
                continue;
            }
        }
        // `<key>=<value>` / `<key>: <value>`，键名命中 token/secret/key/password。
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let mut j = i;
            while j < chars.len()
                && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-')
            {
                j += 1;
            }
            let key: String = chars[i..j].iter().collect();
            let lowered = key.to_ascii_lowercase();
            let sensitive = [
                "token",
                "secret",
                "apikey",
                "api_key",
                "password",
                "passwd",
                "credential",
                "authorization",
            ]
            .iter()
            .any(|needle| lowered.contains(needle));
            let mut k = j;
            while k < chars.len() && (chars[k] == ' ' || chars[k] == '=' || chars[k] == ':') {
                k += 1;
            }
            let has_sep = k > j;
            if sensitive && has_sep {
                let mut v = k;
                while v < chars.len() && !chars[v].is_whitespace() && chars[v] != ',' {
                    v += 1;
                }
                if v > k {
                    out.push_str(&key);
                    out.push('=');
                    out.push_str("***");
                    i = v;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn text_at(chars: &[char], start: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(start + offset) == Some(&expected))
}

/// 生成运行记录 id：`run-<日期>-<时间>-<随机后缀>`。
///
/// 日期与时间取**本地时区**（`shell::localtime` 的三个格式化，共用同一份
/// 偏移换算），因为它出现在用户看得到的地方；
/// 而排序永远用 [`DiagnosticRun::started_at_ms`]，不解析这个字符串——**格式
/// 不变**，历史记录照旧可读。
pub fn new_run_id() -> String {
    use std::time::UNIX_EPOCH;
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let date = crate::shell::process::current_date_string();
    let stamp = crate::shell::localtime::local_hms_string(now);
    let nanos = since_epoch.subsec_nanos() % 0xffff;
    format!("run-{}-{}-{:04x}", date.replace('-', ""), stamp, nanos)
}

// --- 写入 ------------------------------------------------------------------
/// 通道信封：把一条事件与它所属的运行记录一起包成 JSON。
///
/// 前端按 `type` 分流——`diagnostic-event` 进诊断时间线，其余原样当纯文本
/// 显示。**发不出 / 解析不了都不能让进度面板失效**，所以前端那条解析
/// 永远带纯文本兜底。
///
/// 用 `serde_json` 而不是手写字符串拼接：事件文案可能含引号、反斜杠与
/// 换行，手写转义错一次就是一条解析不了的通道消息——而那条消息按上面的
/// 兜底规则会原样显示成一段 JSON 噪声。
pub fn channel_envelope(run_id: &str, event: &DiagnosticEvent) -> String {
    serde_json::json!({
        "type": "diagnostic-event",
        "runId": run_id,
        "event": event,
    })
    .to_string()
}

/// 一次运行记录的累加器。
///
/// 持有它的调用方在**每个**落盘点都用 `let _ = …` 吞掉错误（见模块文档的
/// 纪律 1）；这里仍然返回 `Result` 是为了让测试能断言写入成功，而生产调用
/// 方的忽略是有意的。
pub struct Recorder {
    family: String,
    instance: String,
    run: DiagnosticRun,
    /// 本次运行的起点，用来算事件耗时。
    started: Instant,
    /// 上一条事件，用于给下一条算相对耗时。
    last_at: Instant,
    /// 阶段计时器：同一 stage 连续推送时累加，而不是每次从零开始。
    stage_started: std::collections::HashMap<String, Instant>,
    /// 待发的通道信封。
    ///
    /// 看护内部不推通道（见 `GuardDeps::run_push` 的注释：deps 在整个
    /// 看护期间独占 `&mut Recorder`，没法再借通道回调），事件先攒在这里，
    /// 由调用方在 [`Recorder::take_pending`] 取走补发。这样启动的**实时**
    /// 时间线与落盘记录仍然是同一份事件，不会出现「界面上有、磁盘上没有」
    /// 的那种只在某一条路径上生效的阶段。
    pending: Vec<String>,
    /// 已被事故 / 快照引用的记录 id，裁剪时跳过。
    pinned: HashSet<String>,
}

impl Recorder {
    /// 开一条新记录。**不落盘**——落盘发生在 [`Recorder::push`]，这样一条
    /// 从未产生任何事件的记录不会在磁盘上留下空壳。
    pub fn begin(
        family: &str,
        instance: &str,
        kind: &str,
        kernel_version: &str,
        profile: &str,
        pinned: HashSet<String>,
    ) -> Self {
        let run = DiagnosticRun {
            schema: SCHEMA,
            id: new_run_id(),
            kind: kind.to_string(),
            family: family.to_string(),
            instance_id: instance.to_string(),
            kernel_version: sanitize(kernel_version),
            profile: sanitize(profile),
            started_at_ms: crate::shell::process::epoch_millis(),
            finished_at_ms: None,
            status: status::RUNNING.to_string(),
            cause: String::new(),
            summary: String::new(),
            evidence: RunEvidence::default(),
            events: Vec::new(),
        };
        Self {
            family: family.to_string(),
            instance: instance.to_string(),
            run,
            started: Instant::now(),
            last_at: Instant::now(),
            stage_started: std::collections::HashMap::new(),
            pending: Vec::new(),
            pinned,
        }
    }

    /// 接续一条**已落盘但还没收尾**的记录。
    ///
    /// 二分是多命令流程（`start` → `probe` ×N → `abort`），每条命令都是一次
    /// 独立的调用，recorder 活不过命令边界。重新 [`Recorder::begin`] 一条会
    /// 把一次排查拆成 N 条互不相干的记录——而「一次操作 = 一条记录」正是
    /// 这套模型存在的理由：用户在时间线上要看到的是同一场排查的各轮试探，
    /// 不是 N 场各自开头结尾都看不见的排查。
    ///
    /// `run.status` 若已不是 `running`（上一条命令已收尾过），这里照原样接
    /// 续——调用方拿到的仍是那条已结束的记录，再推事件也只是往一个终态上
    /// 追加，不比重新开一条更糟。
    pub fn attach(family: &str, instance: &str, run: DiagnosticRun) -> Self {
        let now = Instant::now();
        Self {
            family: family.to_string(),
            instance: instance.to_string(),
            run,
            started: now,
            last_at: now,
            stage_started: std::collections::HashMap::new(),
            pending: Vec::new(),
            pinned: HashSet::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.run.id
    }

    pub fn run(&self) -> &DiagnosticRun {
        &self.run
    }

    /// 追加一条阶段事件并落盘。
    ///
    /// `attempt` 只在需要区分看护重试时给：第 1 次尝试是 `None`，
    /// 之后每次重试递增。
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        stage: &str,
        status: &str,
        message: &str,
        attempt: Option<u32>,
        evidence: Option<&RunEvidence>,
    ) -> DiagnosticEvent {
        let now = Instant::now();
        let event = DiagnosticEvent {
            seq: self.run.events.len() as u32 + 1,
            stage: stage.to_string(),
            status: status.to_string(),
            at_ms: crate::shell::process::epoch_millis(),
            duration_ms: Some(now.duration_since(self.last_at).as_millis() as u64),
            message: sanitize(message),
            attempt,
        };
        self.last_at = now;
        self.stage_started.insert(stage.to_string(), now);
        if let Some(evidence) = evidence {
            merge_evidence(&mut self.run.evidence, evidence);
        }
        self.run.events.push(event.clone());
        // 超过上限时丢**最旧**的：时间线的价值在尾部（刚才发生了什么），
        // 而开头那几条通常是「检查 Node.js」这种固定前缀。
        if self.run.events.len() > MAX_EVENTS {
            let overflow = self.run.events.len() - MAX_EVENTS;
            self.run.events.drain(0..overflow);
        }
        self.pending.push(channel_envelope(&self.run.id, &event));
        self.persist();
        event
    }

    /// 取走待发的通道信封并清空。
    pub fn take_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }

    /// 标记某阶段开始（用于「正在启动内核」这种进行中态）。
    pub fn stage_started(&mut self, stage: &str, message: &str, attempt: Option<u32>) -> &mut Self {
        self.stage_started.insert(stage.to_string(), Instant::now());
        self.push(stage, status::RUNNING, message, attempt, None);
        self
    }

    /// 某阶段耗时（自该阶段最近一次 `running` 起）。
    pub fn stage_duration_ms(&mut self, stage: &str) -> Option<u64> {
        self.stage_started
            .get(stage)
            .map(|at| at.elapsed().as_millis() as u64)
    }

    /// 结束运行记录。
    ///
    /// `status` / `cause` 原样写入——**未知取值不进白名单**。前端负责把
    /// 不认识的显示成「未知状态」，把它悄悄换成 `unknown` 等于抹掉证据。
    pub fn finish(
        &mut self,
        status: &str,
        cause: &str,
        summary: &str,
        evidence: Option<&RunEvidence>,
    ) -> Result<(), AppError> {
        self.run.status = status.to_string();
        self.run.cause = sanitize(cause);
        self.run.summary = sanitize(summary);
        self.run.finished_at_ms = Some(crate::shell::process::epoch_millis());
        if let Some(evidence) = evidence {
            merge_evidence(&mut self.run.evidence, evidence);
        }
        self.persist_index()?;
        self.persist_run()
    }

    /// 用户中止。语义上不同于失败：没有任何证据表明哪里错了。
    pub fn cancel(&mut self, summary: &str) -> Result<(), AppError> {
        self.finish(status::CANCELED, cause::UNKNOWN, summary, None)
    }

    /// 落盘单条详情。写失败只落事件日志（纪律 1）。
    fn persist(&self) {
        if let Err(error) = self.persist_run() {
            crate::shell::shell_events::record(
                "diagnostics-run",
                &format!("运行记录 {} 写入失败：{error}", self.run.id),
            );
        }
    }

    fn persist_run(&self) -> Result<(), AppError> {
        crate::shell::state::save(
            &run_path(&self.family, &self.instance, &self.run.id),
            &self.run,
            ctx(),
        )
    }

    /// 写详情 + 更新索引 + 裁剪。索引是「列表」的唯一真相，详情缺失时列表
    /// 仍能显示状态与摘要——这正是详情与索引分两个文件的原因。
    fn persist_index(&self) -> Result<(), AppError> {
        let mut index = load_index_checked(&self.family, &self.instance)?;
        index.entries.retain(|entry| entry.id != self.run.id);
        index.entries.insert(
            0,
            RunSummary {
                id: self.run.id.clone(),
                kind: self.run.kind.clone(),
                status: self.run.status.clone(),
                cause: self.run.cause.clone(),
                summary: self.run.summary.clone(),
                started_at_ms: self.run.started_at_ms,
                finished_at_ms: self.run.finished_at_ms,
                event_count: self.run.events.len(),
            },
        );
        prune(&self.family, &self.instance, &mut index, &self.pinned);
        crate::shell::state::save(&index_path(&self.family, &self.instance), &index, ctx())
    }

    /// 本次运行已耗时（毫秒）。
    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

/// 证据合并：已有字段不覆盖。事故面板与启动诊断引用同一份日志路径时，
/// 后写入的那次不该把先写入的路径改掉。
fn merge_evidence(target: &mut RunEvidence, extra: &RunEvidence) {
    if target.kernel_log.is_none() {
        target.kernel_log = extra.kernel_log.clone();
    }
    if target.incident_id.is_none() {
        target.incident_id = extra.incident_id.clone();
    }
    if target.sandbox_log.is_none() {
        target.sandbox_log = extra.sandbox_log.clone();
    }
}

/// 裁剪到 [`MAX_RUNS`]，并跳过仍被引用的记录。
///
/// **引用保护是这个函数存在的理由**：一份 `failure` 记录如果正被事故面板或
/// 未读通知引用，删掉它会让用户点进去看到「记录不存在」——而且是在他正要
/// 处置故障的时候。被引用者可以超出上限，这是有意的。
///
/// 被引用的记录**不占额度**：一份被事故引用的三个月前的启动失败不该把最近
/// 20 条挤掉一格。用户想看的是最近的记录，而事故那条他另有入口。
pub fn prune(family: &str, instance: &str, index: &mut RunIndex, pinned: &HashSet<String>) {
    let mut kept = 0usize;
    let mut removed: Vec<String> = Vec::new();
    index.entries.retain(|entry| {
        if pinned.contains(&entry.id) {
            return true;
        }
        if kept < MAX_RUNS {
            kept += 1;
            return true;
        }
        removed.push(entry.id.clone());
        false
    });
    for id in removed {
        let _ = std::fs::remove_file(run_path(family, instance, &id));
    }
}

/// 删掉「索引里已经没有、但详情文件还在」的孤儿记录，返回删除个数。
///
/// [`prune`] 只清「被 20 条上限挤掉」的那几条。孤儿是另一类：详情文件先
/// 落地、索引后写入，壳在两者之间被强杀（用户拔电源、任务管理器结束
/// 进程）就会留下一个索引里查不到的 `run-*.json`。它们对用户完全不可见——
/// 列表读索引——却会一直占着磁盘，而详情文件带着整份事件流，单条可达
/// 几百 KB。启动时扫一遍，让磁盘状态回到索引描述的样子。
///
/// **只删 `run-` 前缀的 `.json`**：目录里可能还有别的文件（`index.json`
/// 本身、未来的附属产物），按前缀认领才不会误伤。
///
/// **索引读不出来时一个都不删**，这是本函数最要紧的一条。`load_index` 是
/// 容错读：`index.json` 损坏时它同样返回空索引。若拿这份空索引去比对，
/// 磁盘上**每一条**详情文件都会变成「孤儿」，一次启动就把用户的全部诊断
/// 历史抹干净——而起因只是一次半截写入。因此这里走 `load_checked`：文件
/// 不存在是正常路径（`Missing` → 空索引 → 目录本来就是空的），损坏则
/// 原地返回 0，把「删不删」的判断交给用户下次自己处理。
pub fn sweep_orphans(family: &str, instance: &str) -> usize {
    let dir = runs_dir(family, instance);
    if !dir.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let index: RunIndex =
        match crate::shell::state::load_checked(&index_path(family, instance), ctx()) {
            Ok(index) => index,
            Err(_) => return 0,
        };
    let known: HashSet<String> = index.entries.iter().map(|entry| entry.id.clone()).collect();
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // 详情文件名就是 `{run_id}.json`，而 run_id 本身以 `run-` 开头
        // （见 `run-<时刻>-<后缀>`），所以比对的键是**带前缀的** stem。
        // 剥掉前缀再比会把每一条记录都当成孤儿——这正是这条单测抓到的。
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if !stem.starts_with("run-") || known.contains(stem) {
            continue;
        }
        if std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// 清空本实例的全部运行记录，返回删除的详情文件个数。
///
/// 与 [`sweep_orphans`] 的区别是它**连索引一起清空**：这是用户在控制塔
/// 点「清除诊断记录」时的语义——要的是「这段历史我不要了」，而不是「把
/// 你看不见的东西顺手删掉」。因此被事故引用的那条**也不豁免**：那是用户
/// 明确表达的删除意图，`prune` 的引用保护是给自动裁剪用的自动决策让路。
///
/// 索引按 [`SCHEMA`] 重写成空文档而不是删文件：删掉它会让下一次
/// `load_index` 走 `load_lossy` 的空值分支，那条分支的文案是「索引无法
/// 解析」，会让一个刚被用户主动清空的目录看起来像损坏。
///
/// 返回 `Err` 只有一种情况：详情文件已删但索引没能重写。那时旧的
/// `index.json` 仍列着已被删掉的记录，用户会在列表里点进一片「记录不存
/// 在」——这必须让用户看见，不能当无事发生。
pub fn clear(family: &str, instance: &str) -> Result<usize, AppError> {
    let dir = runs_dir(family, instance);
    let mut removed = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let orphaned = name
                .to_str()
                .and_then(|n| n.strip_prefix("run-"))
                .is_some_and(|rest| rest.ends_with(".json"));
            if orphaned && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    let index: RunIndex = RunIndex {
        schema: SCHEMA,
        ..RunIndex::default()
    };
    crate::shell::state::save(&index_path(family, instance), &index, ctx())?;
    Ok(removed)
}

/// 当前应当被保护、不参与裁剪的记录 id。
///
/// 只认「事故面板正在展示的那次」：`last-incident.json` 里引用的运行 id。
/// 未读通知这条链首期不接——通知里存的是任务摘要而不是运行记录 id，
/// 硬凑一个匹配只会删错东西。
///
/// `data_dir` 由调用方给出而不是在这里解析：事故文件按壳的 `data_dir`
/// 存放，而本模块其余部分一律按 (family, instance) 定位，两条路径的
/// 交点只有调用方知道（见 `commands::start_kernel_blocking`）。
pub fn pinned_from_incident(data_dir: &Path) -> HashSet<String> {
    let mut pinned = HashSet::new();
    if let Some(incident) = super::startup_run::load_incident(data_dir) {
        if let Some(run_id) = incident.run_id {
            pinned.insert(run_id);
        }
    }
    pinned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    fn temp_home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("dsh-run-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("temp home");
        home
    }

    /// 写一份索引，entries 全部用给定的 id。
    fn seed_index(ids: &[&str]) {
        let index: RunIndex = RunIndex {
            schema: SCHEMA,
            entries: ids
                .iter()
                .map(|id| RunSummary {
                    id: (*id).to_string(),
                    kind: kind::STARTUP.to_string(),
                    status: status::SUCCESS.to_string(),
                    cause: String::new(),
                    summary: String::new(),
                    started_at_ms: 0,
                    finished_at_ms: None,
                    event_count: 0,
                })
                .collect(),
        };
        std::fs::create_dir_all(runs_dir("dsh", "default")).unwrap();
        crate::shell::state::save(&index_path("dsh", "default"), &index, ctx()).unwrap();
    }

    #[test]
    fn sweep_orphans_deletes_only_details_the_index_no_longer_lists() {
        let home = temp_home("sweep");
        let _guard = scoped_xlink_home(&home);
        seed_index(&["run-kept"]);
        std::fs::create_dir_all(runs_dir("dsh", "default")).unwrap();
        std::fs::write(run_path("dsh", "default", "run-kept"), "{}").unwrap();
        // 详情先落地、索引后写入，壳在两者之间被强杀：索引里没有，文件在。
        std::fs::write(run_path("dsh", "default", "run-half-written"), "{}").unwrap();
        let removed = sweep_orphans("dsh", "default");
        assert_eq!(removed, 1, "只删索引里查不到的那一个");
        assert!(
            run_path("dsh", "default", "run-kept").exists(),
            "索引里还列着的详情不能被当成孤儿删掉"
        );
        assert!(!run_path("dsh", "default", "run-half-written").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sweep_orphans_deletes_nothing_when_the_index_is_corrupt() {
        let home = temp_home("sweep-corrupt");
        let _guard = scoped_xlink_home(&home);
        let dir = runs_dir("dsh", "default");
        std::fs::create_dir_all(&dir).unwrap();
        // 半个 JSON：一次被强杀的写入留下的正是这种文件。
        std::fs::write(index_path("dsh", "default"), "{\"schema\":1,\"entr").unwrap();
        std::fs::write(run_path("dsh", "default", "run-a"), "{}").unwrap();
        std::fs::write(run_path("dsh", "default", "run-b"), "{}").unwrap();
        // `load_index` 是容错读，损坏时返回空索引。拿那份空索引去比对，磁盘上
        // 每一条详情文件都会变成「孤儿」——一次启动抹掉全部诊断历史。
        assert!(
            load_index("dsh", "default").entries.is_empty(),
            "前提：容错读给出空索引"
        );
        assert_eq!(
            sweep_orphans("dsh", "default"),
            0,
            "索引读不出来时一个都不能删"
        );
        assert!(run_path("dsh", "default", "run-a").exists());
        assert!(run_path("dsh", "default", "run-b").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sweep_orphans_leaves_unrelated_files_alone() {
        let home = temp_home("sweep-keep");
        let _guard = scoped_xlink_home(&home);
        seed_index(&["run-kept"]);
        let dir = runs_dir("dsh", "default");
        std::fs::create_dir_all(&dir).unwrap();
        // 未来可能出现的附属产物、以及索引本身，都不在 run- 前缀的认领范围内。
        std::fs::write(dir.join("thumbnails.json"), "{}").unwrap();
        let before = std::fs::read_to_string(index_path("dsh", "default")).unwrap();
        assert_eq!(sweep_orphans("dsh", "default"), 0);
        assert!(
            dir.join("thumbnails.json").exists(),
            "非 run- 前缀的文件不能被误删"
        );
        assert_eq!(
            std::fs::read_to_string(index_path("dsh", "default")).unwrap(),
            before,
            "清扫只删详情文件，不许改索引"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn clear_empties_both_index_and_details() {
        let home = temp_home("clear");
        let _guard = scoped_xlink_home(&home);
        seed_index(&["run-a", "run-b"]);
        std::fs::write(run_path("dsh", "default", "run-a"), "{}").unwrap();
        std::fs::write(run_path("dsh", "default", "run-b"), "{}").unwrap();
        std::fs::write(runs_dir("dsh", "default").join("thumbnails.json"), "{}").unwrap();
        assert_eq!(clear("dsh", "default").expect("清空成功"), 2);
        assert!(
            load_index("dsh", "default").entries.is_empty(),
            "索引要一起清空"
        );
        assert!(!run_path("dsh", "default", "run-a").exists());
        // 重写成空文档而不是删文件：删掉会让下一次读走「索引无法解析」那条
        // 损坏文案，而用户刚刚亲手清空过一次。
        assert!(
            index_path("dsh", "default").exists(),
            "索引文件要保留，只是变空"
        );
        assert!(
            runs_dir("dsh", "default").join("thumbnails.json").exists(),
            "清除只认 run- 前缀的详情文件"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn events_keep_order_within_the_same_millisecond() {
        let home = temp_home("order");
        let _guard = scoped_xlink_home(&home);
        let mut rec = Recorder::begin(
            "dsh",
            "default",
            kind::STARTUP,
            "0.2.1",
            "web",
            HashSet::new(),
        );
        // 故意用同一个 at_ms，验证排序靠 seq 而不是时间戳。
        let event = DiagnosticEvent {
            seq: 1,
            stage: stage::SPAWN_KERNEL.to_string(),
            status: status::RUNNING.to_string(),
            at_ms: rec.run.started_at_ms,
            duration_ms: None,
            message: String::from("正在启动内核"),
            attempt: None,
        };
        rec.run.events.push(event);
        rec.run.events.push(DiagnosticEvent {
            seq: 2,
            stage: stage::WAIT_READY.to_string(),
            status: status::SUCCESS.to_string(),
            at_ms: rec.run.started_at_ms,
            duration_ms: None,
            message: String::from("端口已就绪"),
            attempt: None,
        });
        let seqs: Vec<u32> = rec.run.events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 2], "同毫秒事件必须按 seq 保持顺序");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn run_round_trips_through_disk() {
        let home = temp_home("roundtrip");
        let _guard = scoped_xlink_home(&home);
        let id = {
            let mut rec = Recorder::begin(
                "dsh",
                "default",
                kind::STARTUP,
                "0.2.1",
                "web",
                HashSet::new(),
            );
            rec.push(
                stage::DETECT_NODE,
                status::SUCCESS,
                "检测到满足要求的 Node.js",
                None,
                None,
            );
            rec.finish(status::FAILURE, cause::ENVIRONMENT, "端口被占用", None)
                .expect("finish");
            rec.id().to_string()
        };
        let loaded = get("dsh", "default", &id).expect("详情可读");
        assert_eq!(loaded.status, status::FAILURE);
        assert_eq!(loaded.cause, cause::ENVIRONMENT);
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.events[0].seq, 1);
        assert_eq!(latest("dsh", "default", kind::STARTUP).unwrap().id, id);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn unknown_stage_status_and_cause_survive_a_round_trip() {
        let home = temp_home("unknown");
        let _guard = scoped_xlink_home(&home);
        let id = {
            let mut rec = Recorder::begin(
                "dsh",
                "default",
                kind::BISECT,
                "0.2.1",
                "web",
                HashSet::new(),
            );
            rec.push(
                "stage-from-the-future",
                "halfway",
                "未来版本的阶段",
                None,
                None,
            );
            rec.finish("weird-status", "weird-cause", "未知取值的记录", None)
                .expect("finish");
            rec.id().to_string()
        };
        let loaded = get("dsh", "default", &id).expect("详情可读");
        assert_eq!(loaded.events[0].stage, "stage-from-the-future");
        assert_eq!(loaded.events[0].status, "halfway");
        assert_eq!(loaded.status, "weird-status");
        assert_eq!(loaded.cause, "weird-cause");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn prune_keeps_pinned_runs_beyond_the_limit() {
        let home = temp_home("prune");
        let _guard = scoped_xlink_home(&home);
        let mut index = RunIndex {
            schema: SCHEMA,
            entries: Vec::new(),
        };
        // 被事故引用的那条排在**窗口之外**（index 25）——这才是引用保护
        // 真正要救的场景：常规裁剪一定会先把它删掉。
        for n in 0..(MAX_RUNS + 5) {
            index.entries.push(RunSummary {
                id: format!("run-{n}"),
                kind: kind::STARTUP.to_string(),
                status: status::FAILURE.to_string(),
                cause: String::new(),
                summary: String::new(),
                started_at_ms: n as u64,
                finished_at_ms: None,
                event_count: 0,
            });
        }
        let oldest_pinned = format!("run-{}", MAX_RUNS + 4);
        let pinned = HashSet::from([oldest_pinned.clone()]);
        prune("dsh", "default", &mut index, &pinned);
        assert!(
            index.entries.iter().any(|e| e.id == oldest_pinned),
            "仍被事故引用的记录不能被裁掉"
        );
        assert_eq!(
            index.entries.len(),
            MAX_RUNS + 1,
            "被引用者不占额度：上限仍是 {MAX_RUNS} 条正常记录 + 1 条被引用"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn prune_deletes_the_detail_file_of_dropped_runs() {
        let home = temp_home("prune-files");
        let _guard = scoped_xlink_home(&home);
        let mut index = RunIndex {
            schema: SCHEMA,
            entries: Vec::new(),
        };
        let doomed = "run-doomed";
        std::fs::create_dir_all(runs_dir("dsh", "default")).unwrap();
        std::fs::write(run_path("dsh", "default", doomed), "{}").unwrap();
        // 排在窗口之外，确保它必然被裁掉。
        for n in 0..(MAX_RUNS + 2) {
            index.entries.push(RunSummary {
                id: format!("run-{n}"),
                kind: kind::STARTUP.to_string(),
                status: status::SUCCESS.to_string(),
                cause: String::new(),
                summary: String::new(),
                started_at_ms: n as u64,
                finished_at_ms: None,
                event_count: 0,
            });
        }
        index.entries.push(RunSummary {
            id: doomed.to_string(),
            kind: kind::STARTUP.to_string(),
            status: status::SUCCESS.to_string(),
            cause: String::new(),
            summary: String::new(),
            started_at_ms: 9999,
            finished_at_ms: None,
            event_count: 0,
        });
        prune("dsh", "default", &mut index, &HashSet::new());
        assert_eq!(index.entries.len(), MAX_RUNS);
        assert!(
            !run_path("dsh", "default", doomed).exists(),
            "被裁掉的记录要连详情文件一起清掉，否则磁盘上会一直躺着孤儿文件"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sanitize_folds_home_and_strips_token_shapes() {
        let home = temp_home("sanitize");
        let _guard = scoped_xlink_home(&home);
        std::env::set_var("HOME", &home);
        let text = sanitize(&format!(
            "日志在 {}/logs/kernel.log，key 是 sk-abcdef1234567890，token=ghp_secretvalue",
            home.display()
        ));
        assert!(
            !text.contains(&home.display().to_string()),
            "完整 home 路径不能进运行记录"
        );
        assert!(text.contains("~/logs/kernel.log"));
        assert!(!text.contains("sk-abcdef1234567890"), "key 必须被抹掉");
        assert!(!text.contains("ghp_secretvalue"), "token 必须被抹掉");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sanitize_folds_a_windows_user_profile_path() {
        // 审查 P1-07：Windows 上 `HOME` 常常根本不设，只有 `USERPROFILE`。
        // 裸读 `$HOME` 的实现会把 `C:\Users\用户名\…` 原样留在运行记录里，
        // 而内核日志路径恰恰来自那里——用户整份贴进求助帖就等于贴了用户名。
        //
        // **纯函数测试，不改环境变量**：第一版用 `remove_var("HOME")` 模拟
        // Windows，那条 remove 漏到了并发的 `shell::autostart` 用例，一次
        // cargo test 红了四条与本功能无关的测试。
        let profile = PathBuf::from(r"C:\Users\tester\AppData");
        let text = sanitize_with_roots(
            r"内核日志在 C:\Users\tester\AppData\Local\dsh\logs\kernel.log",
            &[profile],
        );
        assert!(
            !text.contains("tester"),
            "Windows 用户名不能留在运行记录里：{text}"
        );
        assert!(text.contains(r"~\Local\dsh\logs\kernel.log"), "{text}");
    }

    #[test]
    fn sanitize_folds_a_relocated_data_root_that_carries_a_user_name() {
        // `DSH_XLINK_HOME` 能把数据根搬到任意目录（外置盘、测试目录），那条
        // 路径里同样可能带用户名——只折 home 折不掉它。
        let moved = PathBuf::from("/Volumes/外置盘/tester-home/data");
        let text = sanitize_with_roots(
            "/Volumes/外置盘/tester-home/data/dsh/desktop/active.txt",
            &[moved],
        );
        assert!(!text.contains("tester"), "{text}");
        assert!(text.contains("~/dsh/desktop/active.txt"), "{text}");
    }

    #[test]
    fn a_longer_root_is_folded_before_a_shorter_one_containing_it() {
        // 数据根就在 home 下时，两者有包含关系。先折短的会让长的永远匹配不上
        // ——`~/…` 已经把前缀替掉了，而那条路径里带的用户名也就留下了。
        let home = PathBuf::from("/Users/tester");
        let moved = PathBuf::from("/Users/tester/external/data");
        let text = sanitize_with_roots(
            "/Users/tester/external/data/dsh/active.txt",
            &[home.clone(), moved.clone()],
        );
        assert!(!text.contains("tester"), "{text}");
        // 短的根仍然能折它自己那部分（长根没覆盖到的地方）。
        assert_eq!(
            sanitize_with_roots("/Users/tester/notes.txt", &[home, moved]),
            "~/notes.txt"
        );
    }

    #[test]
    fn a_one_character_root_is_never_folded() {
        // Windows 的 `C:\`、某些环境下的 `/`：折掉它会把整台机器上所有路径
        // 都变成 `~/…`，那比不折更糟——证据直接失效。
        let text = sanitize_with_roots("C:\\Users\\tester\\notes.txt", &[PathBuf::from("C:\\")]);
        assert!(text.contains("tester"), "{text}");
    }

    #[test]
    fn a_run_id_uses_local_time_for_both_its_date_and_its_clock() {
        // 审查 P2-04：日期按本地偏移、时刻按「当前时间 - 现在」估的偏移，
        // 两者在东八区会差 8 小时——runId 里出现「昨天的日期配明天的时间」。
        // 断言的是**两者自洽**（同一份偏移），不硬编码某个时区的值，那会让
        // 测试在 CI 换时区后假红。
        let id = new_run_id();
        let rest = id.strip_prefix("run-").expect("runId 格式未变");
        let mut parts = rest.split('-');
        let date = parts.next().expect("日期段");
        let clock = parts.next().expect("时刻段");
        assert_eq!(date.len(), 8, "日期必须是 YYYYMMDD：{id}");
        assert_eq!(clock.len(), 6, "时刻必须是 HHMMSS：{id}");
        let hour: u32 = clock[..2].parse().expect("小时");
        assert!(hour < 24, "小时必须在本地时区的取值范围内：{id}");
        let minute: u32 = clock[2..4].parse().expect("分钟");
        assert!(minute < 60, "{id}");

        // 本地日期与本地时刻必须来自同一次换算：把此刻的本地日期还原出来，
        // 应当与 id 里的日期段一致（同一天内成立；跨午夜那一瞬不成立，
        // 所以只断言格式与取值范围，不做跨日断言）。
        let local_date = crate::shell::process::current_date_string().replace('-', "");
        if local_date == date {
            // 同一天：小时应当接近现在的小时（±1 容忍跨小时的取整）。
            let local_clock = crate::shell::localtime::local_hms_string(SystemTime::now());
            let now_hour: u32 = local_clock[..2].parse().unwrap_or(0);
            assert!(
                (hour as i32 - now_hour as i32).abs() <= 1,
                "同一天里 runId 的时刻 {clock} 与本地现在 {local_clock} 对不上"
            );
        }
    }

    #[test]
    fn sanitize_keeps_ordinary_version_and_path_text() {
        // 脱敏不能连正常证据一起吃掉：版本号、端口、指纹都要留着。
        let text = sanitize("0.2.1-alpha.1 起内核，端口 3090，指纹 abcdef1234567890");
        assert!(text.contains("0.2.1-alpha.1"));
        assert!(text.contains("3090"));
        assert!(text.contains("abcdef1234567890"));
    }

    #[test]
    fn sanitize_truncates_very_long_messages() {
        let long = "x".repeat(5000);
        let text = sanitize(&long);
        assert!(text.chars().count() <= 481, "超长字段必须截断");
        assert!(text.ends_with('…'));
    }

    #[test]
    fn event_budget_drops_the_oldest_entries() {
        let home = temp_home("budget");
        let _guard = scoped_xlink_home(&home);
        let mut rec = Recorder::begin(
            "dsh",
            "default",
            kind::STARTUP,
            "0.2.1",
            "web",
            HashSet::new(),
        );
        for n in 0..(MAX_EVENTS + 10) {
            rec.push(
                stage::RETRY,
                status::RUNNING,
                &format!("第 {n} 次"),
                None,
                None,
            );
        }
        assert_eq!(rec.run().events.len(), MAX_EVENTS);
        assert_eq!(
            rec.run().events.first().unwrap().message,
            "第 10 次",
            "超预算时丢最旧的，保留尾部"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_failure_does_not_change_the_run_outcome() {
        // 索引目录不可写时，push 仍然要正常返回——启动结果不能被记录失败改写。
        let home = temp_home("write-fail");
        let _guard = scoped_xlink_home(&home);
        let blocker = home.join("blocker");
        std::fs::write(&blocker, "not a dir").unwrap();
        std::env::set_var("DSH_XLINK_HOME", &blocker);
        let mut rec = Recorder::begin(
            "dsh",
            "default",
            kind::STARTUP,
            "0.2.1",
            "web",
            HashSet::new(),
        );
        rec.push(
            stage::SPAWN_KERNEL,
            status::RUNNING,
            "正在启动内核",
            None,
            None,
        );
        assert_eq!(rec.run().status, status::RUNNING);
        assert_eq!(
            rec.run().events.len(),
            1,
            "事件仍在内存里，UI 实时时间线不受影响"
        );
        let _ = std::fs::remove_dir_all(&home);
        std::env::remove_var("DSH_XLINK_HOME");
    }

    #[test]
    fn evidence_merge_keeps_the_first_path() {
        let mut target = RunEvidence {
            kernel_log: Some("/a/kernel.log".into()),
            incident_id: Some("inc-1".into()),
            sandbox_log: None,
        };
        merge_evidence(
            &mut target,
            &RunEvidence {
                kernel_log: Some("/b/kernel.log".into()),
                incident_id: None,
                sandbox_log: Some("/c/sandbox.log".into()),
            },
        );
        assert_eq!(target.kernel_log.as_deref(), Some("/a/kernel.log"));
        assert_eq!(target.incident_id.as_deref(), Some("inc-1"));
        assert_eq!(target.sandbox_log.as_deref(), Some("/c/sandbox.log"));
    }

    #[test]
    fn run_id_is_unique_and_prefixed() {
        let a = new_run_id();
        let b = new_run_id();
        assert!(a.starts_with("run-"), "id 必须带可识别前缀");
        assert_ne!(a, b, "两次记录不能撞 id，否则后者会覆盖前者");
    }
}
