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
pub fn sanitize(text: &str) -> String {
    const MAX_CHARS: usize = 480;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut out = text.to_string();
    if !home.as_os_str().is_empty() {
        let home = home.display().to_string();
        if home.len() > 1 {
            out = out.replace(&home, "~");
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
/// 日期与时间取**本地时区**（`current_date_string`），因为它出现在用户看得到
/// 的地方；而排序永远用 [`DiagnosticRun::started_at_ms`]，不解析这个字符串。
pub fn new_run_id() -> String {
    use std::time::UNIX_EPOCH;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let date = crate::shell::process::current_date_string();
    let stamp = local_hms(secs);
    let nanos = now.subsec_nanos() % 0xffff;
    format!(
        "run-{}-{}-{:04x}",
        date.replace('-', ""),
        stamp.replace(':', ""),
        nanos
    )
}

/// 本地 `HHMMSS`。
fn local_hms(secs: u64) -> String {
    let local = secs + local_offset_secs(secs);
    let day = local % 86_400;
    format!("{:02}{:02}{:02}", day / 3600, (day % 3600) / 60, day % 60)
}

/// 本地时区相对 UTC 的偏移秒数。Rust std 没有时区 API，这里按「本地时间 =
/// UTC + 偏移」反解一次：把 epoch 秒当本地时间读一遍 UTC 时钟，得到的就是
/// 该时刻的本地分量。
fn local_offset_secs(secs: u64) -> u64 {
    use std::time::UNIX_EPOCH;
    let local = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    local.saturating_sub(secs)
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
