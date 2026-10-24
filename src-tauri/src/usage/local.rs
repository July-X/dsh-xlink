//! 模型用量统计：扫描内核 session 文件，按「本地日历日 × 模型」预聚合最近
//! [`RETENTION_DAYS`] 天的 token 用量，落一份最小体积的增量账目，并向面板
//! 提供汇总视图（概览卡片 + 「模型用量」详情弹窗）。
//!
//! ## 数据源
//!
//! 内核把每个会话落成 `<DSH_HOME>/sessions/<工作区>/session-<uuid>/` 下的
//! `session[.vN].jsonl.zstd`：zstd **多帧**流，内核每次追加写一个完整新帧。
//! 其中 `{"type":"assistant/message","time":<毫秒>,"data":{…}}` 携带一次模型
//! 回复的计量：`data.message.source`（`kind == "model"` 时含 `provider` /
//! `model`）与 `data.usage`（`inputTokens` / `outputTokens` /
//! `cacheReadTokens`，个别网关还有 `cacheWriteTokens`；内核的 `totalTokens`
//! 即各分量之和）。
//!
//! ## 统计机制（按文件增量，账即真相）
//!
//! - **只追加**：账目按「已消费的压缩字节 `offset`」增量推进，重扫只解新帧。
//!   解码在半帧 / 坏帧处停住（offset 不越过该帧起点），内核补写后下一轮自动
//!   续上——不丢账也绝不重账。
//! - **多代格式去重**：同一会话目录可能并存 v1/v2/v3 三代格式（内核升级迁移
//!   的遗留），只认版本号最高的一代；全量计入会把同一段用量数成三倍。
//! - **账即真相**：每个文件一份「天 × 模型」增量账，汇总视图在读取时求和
//!   派生。会话被删除、或旧代格式被新代取代 → 对应账目整条移除，汇总自动
//!   回落，不需要任何反扣逻辑。
//! - **旧文件零成本**：mtime 早于统计窗口的文件一条都不用解，首次见到就把
//!   offset 直接记到文件末尾。
//!
//! ## 存储与保留
//!
//! 账目落 `paths::instance_usage_state_file`（`state.rs` 容错读 + 原子写），
//! 跟实例走、与壳的 release/dev 模式无关。**只保留最近 90 天**：每次保存都
//! 把窗口外的日账剪掉，文件体积因此有硬上界——这正是「90 天记录就是为了只
//! 保存最小数据量」的兑现；超过 90 天的记录自动丢弃，UI 的 tooltip 负责把
//! 这条保留策略讲给用户。

use crate::shell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use ruzstd::decoding::{BlockDecodingStrategy, FrameDecoder};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use time::format_description::FormatItem;
use time::macros::format_description;

use super::variant_cache;
use crate::shell::error::AppError;
use crate::shell::instance;
use crate::shell::paths;
use crate::shell::process;
use crate::shell::state::StateCtx;

/// 统计窗口（天，含今天）。窗口外的日账在每次保存时剪掉。
pub const RETENTION_DAYS: u64 = 90;

/// 非强制读取时，距上次成功扫描不足该间隔就直接用现有账目回答——概览卡片
/// 随状态轮询反复拉取，不能每 2.5s 都真的扫一遍 sessions 目录。
///
/// **这个阈值必须大于最频繁调用方的间隔，否则它一次都挡不住。** 2026-10-15
/// 实测：前端概览卡片按 `SUMMARY_TTL_MS = 60s` 轮询（`ui/src/usage/usage.js`），
/// 而这里曾是 45s —— 45 < 60，于是**每一次轮询都必然穿透**这道闸去全量扫描，
/// perf 日志里 `usage_scan` 每日累计从 7.14s 涨到 48.13s 就是这么来的。
/// 取 90s 让隔次轮询被挡住（实际扫描间隔 120s），代价是账目最多滞后两分钟：
/// 独立用量窗口与手动「刷新」走 `force = true`，不受这道闸影响。
const FRESH_SCAN_INTERVAL_MS: u64 = 90_000;

const STATE_SCHEMA: u32 = 1;

/// 单文件 × 单日 × 单模型的增量账。tokens = 各分量之和（与内核 usage
/// 的 `totalTokens` 口径一致）。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ModelDay {
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
}

/// 一个 session 文件的增量账：`offset` 之前的压缩字节都已计入 `days`。
/// `offset` 永远停在完整帧的边界（或 0），半帧内容不会被记账。
/// 派生 `PartialEq` 供扫描前后对账——「账目没变就不落盘」的判据靠它。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct FileEntry {
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, ModelDay>>,
}

/// 用量账目文档（`usage/state.json`）。
#[derive(Serialize, Deserialize, Default)]
pub struct UsageStateDoc {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub retention_days: u64,
    #[serde(default)]
    pub last_scanned_at_ms: u64,
    #[serde(default)]
    pub files: BTreeMap<String, FileEntry>,
}

/// 按模型聚合的用量（弹窗的列表 / 饼图 / 柱状堆叠共用）。
#[derive(Serialize, Clone)]
pub struct ModelUsageView {
    /// `provider/model`，与内核 `source` 字段同构。
    pub key: String,
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub tokens: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
}

/// 单日用量（热力图与趋势图共用；`models` 供趋势图按模型堆叠）。
#[derive(Serialize)]
pub struct DayUsageView {
    /// 本地日历日 `YYYY-MM-DD`。
    pub date: String,
    pub tokens: u64,
    pub requests: u64,
    pub models: BTreeMap<String, u64>,
}

/// 面板拿到的汇总视图：窗口恒为最近 90 天、`days` 恒覆盖整个窗口
/// （无用量的日期补零），前端不需要再做日期推导。
#[derive(Serialize)]
pub struct UsageView {
    pub retention_days: u64,
    /// 最近一次成功扫描的时间（毫秒）；0 表示从未扫描过。
    pub last_scanned_at_ms: u64,
    pub tracked_files: usize,
    pub total_tokens: u64,
    pub total_requests: u64,
    /// 今日（本地日历日）的总用量——概览卡片上的数字。
    pub today_tokens: u64,
    /// 今日的请求次数。与 [`Self::today_tokens`] 由**同一次**精确匹配算出：
    /// 曾经只回 tokens，窗口的「今日 N tokens · N 次」只好自己去 `days` 里找
    /// 那一天，于是「今日」在这条链路上有了两套定义。
    pub today_requests: u64,
    /// 窗口内有用量记录的天数。
    pub active_days: u64,
    pub top_model: Option<String>,
    /// 按 tokens 降序。
    pub models: Vec<ModelUsageView>,
    /// 升序，恒为 [`RETENTION_DAYS`] 个元素。
    pub days: Vec<DayUsageView>,
    /// 用户勾选的**外部来源**（Codex / MiniMax Desktop）。
    ///
    /// 这一项**恒在**视图里，即使本次没扫外部来源——前端要靠它回填
    /// checkbox 的勾选状态。取值来自持久化的偏好，不是本次请求的参数。
    pub external_sources: crate::usage::external::ExternalSources,
    /// 本次扫到的外部来源用量。
    ///
    /// **与上面那份内核账目是两块独立展示**：provider 命名空间不同、总量
    /// 口径也不同，混进同一份日序列会把两套口径加到一起。
    ///
    /// 调用方没请求外部来源（`external` 参数为 `None`）时恒为空数组——概览
    /// 卡片走的就是那条路，它不显示外部来源，也不该为它付扫描成本。
    pub external: Vec<crate::usage::external::ExternalToolUsage>,
}

fn usage_ctx() -> StateCtx {
    StateCtx {
        corrupt: |reason| {
            format!(
                "模型用量记录损坏（{reason}）。已停止读写以免丢账或覆盖；\
                 如确认放弃历史统计，可删除该文件后重试。"
            )
        },
        kind: AppError::Usage,
    }
}

/// 同一时刻只允许一次扫描：并发命令在锁上排队，后到的发现账目已新鲜就只
/// 派生视图。锁只在 `spawn_blocking` worker 内获取（与 lifecycle 锁同约定）。
static SCAN_LOCK: Mutex<()> = Mutex::new(());

/// 进程内各实例最近一次成功扫描的时刻。空转对账不再落盘后，磁盘上的
/// `last_scanned_at_ms` 只在账目真正变化时推进，新鲜度窗口因此需要一份
/// 进程内真值——否则 45s 窗口内的重复调用会每次都真的扫一遍 sessions 目录。
///
/// **必须按 `(family, id)` 分键**（2026-10-11 评审）：账目按实例分文件，而
/// [`instance::resolve_default`] 会跟随用户在顶部页签切过的实例。进程级单值
/// 会把上一个实例的扫描时刻当成当前实例的——切过去之后 45s 内的新实例
/// `usage_view(false)` 直接返回它自己的**空账目**文件，用户看到用量面板空白。
/// 旧测试没抓到它是因为它靠别的用例污染后的值才绿，从来没有真正验证过自己。
///
/// 用 `Vec` 而不是 `HashMap`：条目数就是用户切过的实例数（一两个），
/// 线性扫比哈希表更省，而 `HashMap::new` 不是 const、静态里也放不下。
static LAST_SCAN_MS: Mutex<Vec<(String, String, u64)>> = Mutex::new(Vec::new());

/// 取某实例的进程内扫描时刻（没扫过为 0）。
fn last_scan_ms_for(family: &str, id: &str) -> u64 {
    LAST_SCAN_MS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(f, i, _)| f == family && i == id)
        .map(|(_, _, at)| *at)
        .unwrap_or(0)
}

/// 记下某实例本轮的扫描时刻。
fn remember_scan_ms(family: &str, id: &str, at: u64) {
    let mut scans = LAST_SCAN_MS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match scans.iter_mut().find(|(f, i, _)| f == family && i == id) {
        Some(entry) => entry.2 = at,
        None => scans.push((family.to_string(), id.to_string(), at)),
    }
}

/// 账目落盘的最小间隔。两次落盘之间即使账目变了也只在内存里推进 offset。
///
/// 2026-10-15 实测落盘成本：141,858 字节的 `state.json` 里，`write_all +
/// sync_all` 20.35ms、目录 `sync_all` 19.43ms、`rename` 只有 0.50ms——**两次
/// fsync 占了全部**。那两次 fsync 是 `atomic_write` 给凭据与 `settings.json`
/// 共用的持久性保证，不该为了用量账目削弱它；能动的只有频率。
///
/// **跳过落盘不会丢账**：`usage_view` 每次都从磁盘重新读 `doc`，而扫描按
/// offset 增量、**幂等**——下一轮从旧 offset 重扫出同一份账目，代价只是重新
/// 解那一小段。磁盘上的账因此最多滞后 [`SAVE_DEBOUNCE_MS`]，对「按天聚合的
/// token 用量」毫无可见影响。
///
/// `force = true`（用户打开用量窗口 / 手动刷新）**无条件落盘**：那一刻用户
/// 明确在看数据，不该让磁盘状态由一个时间窗口决定。
const SAVE_DEBOUNCE_MS: u64 = 300_000;

/// 每实例最近一次**真正落盘**的时刻，与 [`LAST_SCAN_MS`] 同形同理由
/// （按实例取，否则切换实例后会把上一个实例的时刻当成自己的）。
static LAST_SAVE_MS: Mutex<Vec<(String, String, u64)>> = Mutex::new(Vec::new());

fn last_save_ms_for(family: &str, id: &str) -> u64 {
    LAST_SAVE_MS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(f, i, _)| f == family && i == id)
        .map(|(_, _, at)| *at)
        .unwrap_or(0)
}

fn remember_save_ms(family: &str, id: &str, at: u64) {
    let mut saves = LAST_SAVE_MS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match saves.iter_mut().find(|(f, i, _)| f == family && i == id) {
        Some(entry) => entry.2 = at,
        None => saves.push((family.to_string(), id.to_string(), at)),
    }
}

/// 面板入口：需要时增量扫描，再派生最近 90 天的汇总视图。
///
/// `force = true`（详情弹窗打开 / 刷新）无视新鲜度窗口立即重扫；概览卡片用
/// `false`，命中窗口就只是把账求和，几乎零成本。
///
/// 这条被前端按 60s TTL 静默刷新，首扫要解全部 session 文件（数百 MB 级），
/// 此后增量只有零星文件有新帧——分段耗时走 PerfSample 记进 perf-status 日志
/// （`perf=usage_scan`，source 区分 poll / refresh）：周期性路径不往 shell
/// 事件里刷人话行，采样关闭时零开销。`items=` 是本轮规划出的待解文件数，
/// `saved=` 记本轮是否落了盘。
///
/// `external` 是本次**要扫**的外部来源（Codex / MiniMax Desktop）。
/// `None` = **按已保存的偏好扫**（窗口首次打开走这条：一次调用既拿到勾选
/// 状态又拿到数据）；显式给出 = 只扫给定的那些（概览卡片传全 `false`，
/// 它不显示外部来源，也不该为它付扫描成本）。
pub fn usage_view(
    force: bool,
    external: Option<crate::usage::external::ExternalSources>,
) -> Result<UsageView, AppError> {
    let mut perf = crate::diagnostics::perf::PerfSample::new(
        "usage_scan",
        Some(if force { "refresh" } else { "poll" }),
    );
    let _guard = SCAN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // 一次解析两条路径：state 与 sessions 根同源（instance::resolve_default
    // 读注册表，此前一次调用解析两遍）。
    let (family, id) = instance::resolve_default();
    let path = paths::instance_usage_state_file(family, id);
    let root = paths::instance_dsh_home(family, id).join("sessions");
    // **这里刻意不用 `load_checked`**（2026-10-17）：这份账目整份都能从 sessions
    // 目录里的帧重算，损坏时唯一正确的处理是「从零重扫」——而不是像插件清单那样
    // 拒绝读写。账目被重置、历史统计从头重扫是用户看得见的后果，所以把原因记进
    // 壳日志（`shell_events`），别让它是静默的。配套的写入走 `save_timed_light`
    // （不为派生数据付两次 fsync），两半必须同时成立：见 `state::load_rebuildable`。
    let (mut doc, corrupted): (UsageStateDoc, _) =
        perf.measure("load", || shell::state::load_rebuildable(&path));
    if let Some(reason) = corrupted {
        crate::shell::shell_events::record(
            "usage-state",
            &format!("用量账目损坏，已从零重扫（{reason}）：{}", path.display()),
        );
    }
    let now_ms = process::epoch_millis();
    refresh_doc(
        &mut doc,
        &path,
        &root,
        (family, id),
        force,
        now_ms,
        &mut perf,
    )?;
    let mut view = perf.measure("derive", || derive_view(&doc, now_ms));
    // 外部来源的扫描在**内核账目之外**：它是别的工具留在本机的数据，口径也
    // 不同，所以**不进内核那份日序列**，只作为并列的一块展示。
    let saved = crate::usage::external::saved_sources();
    view.external_sources = saved;
    // `external` 省略 / `null` = **按已保存的偏好扫**（窗口首次打开走这条，
    // 一次调用就能拿到勾选状态与数据）；显式给出 = 只扫给定的那些（概览卡片
    // 传全 `false`，它不显示外部来源，也不该为它付扫描成本）。
    let sources = external.unwrap_or(saved);
    if sources.any() {
        view.external = perf.measure("external", || crate::usage::external::scan(sources, now_ms));
        crate::usage::external::remember_scan(now_ms);
    }
    perf.finish();
    Ok(view)
}

/// 新鲜度窗口之外的账目刷新：扫描 + 按需落盘。路径由调用方给出（可测），
/// 锁由 [`usage_view`] 持有。
///
/// **空转对账不落盘**（2026-10-11 perf 数据驱动）：`items=0` 的扫描占 93%
///（近 4 天 353 次扫描实测），而账目无变化时重写 132KB + fsync 的 save 段
/// p50≈41ms——纯属浪费。判据是扫描前后 `doc.files` 逐值相等（clone 对账，
/// 整段 <1ms）；schema / 保留期落后于当前常量时也强制落一次盘顺带归一。
/// `last_scanned_at_ms` 无论落不落盘都在内存里推进——视图上报的是内存值，
/// 新鲜度窗口另有 [`LAST_SCAN_MS`] 进程内真值，跳过落盘不会让扫描变得更频繁。
fn refresh_doc(
    doc: &mut UsageStateDoc,
    state_path: &Path,
    sessions_root: &Path,
    instance: (&str, &str),
    force: bool,
    now_ms: u64,
    perf: &mut crate::diagnostics::perf::PerfSample,
) -> Result<(), AppError> {
    let schema_stale = doc.schema != STATE_SCHEMA || doc.retention_days != RETENTION_DAYS;
    doc.schema = STATE_SCHEMA;
    doc.retention_days = RETENTION_DAYS;
    // 进程内真值按实例取：拿别的实例的时刻当自己的，会让新实例在新鲜度
    // 窗口内直接返回自己的空账目（见 [`LAST_SCAN_MS`]）。
    let last_in_process = last_scan_ms_for(instance.0, instance.1);
    let last_known = doc.last_scanned_at_ms.max(last_in_process);
    if !force && now_ms.saturating_sub(last_known) < FRESH_SCAN_INTERVAL_MS {
        return Ok(());
    }
    let before = doc.files.clone();
    let started = perf.start();
    let planned = scan_in(doc, sessions_root, now_ms, perf);
    perf.end("scan", started);
    // planned = 规划段判定「有新帧要解」的文件数：0 说明这轮只是空转对账。
    perf.count("items", planned as u64);
    // 用**调用方传进来的** now_ms 而不是再读一次墙钟：这一次扫描的「时刻」与
    // 规划 / 新鲜度判据必须是同一个值。两次读墙钟在快机器上会落在同一毫秒里，
    // `no_op_refresh_skips_save_but_advances_in_memory_timestamp` 那条判据
    // 「第二次扫描的时刻严格变大」就会偶发转红（2026-10-17 去掉落盘 fsync 之后
    // 才稳定复现：那次 fsync 恰好让两次读跨过了毫秒边界）。
    doc.last_scanned_at_ms = now_ms;
    let changed = schema_stale || before != doc.files;
    // `saved` 记的是**本轮有没有真落盘**，不是「账目有没有变」：变了但在
    // 去抖窗口内也会跳过落盘（省掉 40ms 的两次 fsync），跳过时磁盘上的账目
    // 滞后但绝不丢——扫描幂等，下一轮会从旧 offset 重扫出同一份账。
    let persist = changed
        && (force
            || now_ms.saturating_sub(last_save_ms_for(instance.0, instance.1)) >= SAVE_DEBOUNCE_MS);
    perf.count("saved", u64::from(persist));
    if persist {
        // save 段拆成四段记：这份账目被反复重写，而它的成本几乎全在两次
        // fsync 上（见 `atomic_write::WriteTimings` 的说明）。只有一个
        // `save_us` 的话，下次再看到 40ms 仍然不知道该不该动 fsync。
        let started = perf.start();
        // 免 fsync 的落盘：这份账目是派生数据，掉电最多丢一笔 offset——下次全量
        // 重扫会算回来，而 `load_rebuildable` 已经把「读到半截」接住了。省掉的是
        // 每次保存的两次 fsync（2026-10-17 perf 日志：save_us p50 44ms 里
        // save_fsync 21ms + save_dirsync 19ms，占这一段成本的九成）。
        let timings = shell::state::save_timed_light(state_path, doc, usage_ctx())?;
        perf.end("save", started);
        perf.record_all(timings.as_fields());
        remember_save_ms(instance.0, instance.1, doc.last_scanned_at_ms);
    }
    remember_scan_ms(instance.0, instance.1, doc.last_scanned_at_ms);
    Ok(())
}

/// 扫描 sessions 目录，把每个新追加的帧计入对应文件的账。sessions 根目录
/// 由调用方给出（[`usage_view`] 解析实例路径，测试注入临时目录）。
///
/// 三段式：先串行**规划**（枚举 + 对账决定每个文件从哪里续扫），再把真正
/// 花时间的解码**并行**打散到多个线程（首次全量要解几百 MB，并行把它从
/// 十几秒压到一两秒；常规增量只有零星几个文件有新帧），最后串行把结果
/// **合账**。账目文档只被串行段碰，工作线程只拿自己的任务副本。
/// 返回规划出的待解文件数（供 perf 采样记 `items`）。
///
/// 「哪个文件该读」交给 [`variant_cache`]：2026-10-15 实测，那一步要为每个
/// 会话目录各开一次 `read_dir`，308 个目录热缓存下就是 30ms（冷 255ms），
/// 而同样数量的 `metadata` 只要 3.7ms——目录 mtime 闸省掉的就是前者。
fn scan_in(
    doc: &mut UsageStateDoc,
    root: &Path,
    now_ms: u64,
    perf: &mut crate::diagnostics::perf::PerfSample,
) -> usize {
    let cutoff = cutoff_date_string(now_ms);
    let mut seen = BTreeSet::new();
    let mut items: Vec<ScanItem> = Vec::new();
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    let mut cache = variant_cache::variant_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // 三段各记各的耗时：2026-10-15 那次定位花的时间几乎都在这里——
    // 一个 `scan_us` 总数只告诉你「慢」，不告诉你慢在枚举、对账还是解码。
    // 三个阶段的规模也一并记下，否则「12ms」没法解释。
    let started = std::time::Instant::now();
    let (root_cache, gen) = variant_cache::begin_round(&mut cache, root);
    if let Ok(workspaces) = fs::read_dir(root) {
        for ws in workspaces.flatten() {
            let Ok(sessions) = fs::read_dir(ws.path()) else {
                continue;
            };
            for session in sessions.flatten() {
                let Some(file) = variant_cache::resolve_variant(&session.path(), root_cache, gen)
                else {
                    continue;
                };
                let key = format!(
                    "{}/{}/{}",
                    ws.file_name().to_string_lossy(),
                    session.file_name().to_string_lossy(),
                    file.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                seen.insert(key.clone());
                found.push((key, file));
            }
        }
    }
    // 会话目录被删掉后本轮不会再 visit 到它，它的条目必须跟着收（见
    // variant_cache::end_round 的理由：键是绝对路径，不收就无界增长）。
    let stats = variant_cache::end_round(root_cache, gen);
    perf.record_micros("scan_walk", started.elapsed().as_micros());
    perf.count("files", found.len() as u64);
    perf.count("cache_hits", stats.hits);
    perf.count("cache_misses", stats.misses);
    perf.count("cache_pruned", stats.pruned);

    let started = std::time::Instant::now();
    for (key, file) in &found {
        plan_item(doc, key, file, now_ms, &cutoff, &mut items);
    }
    perf.record_micros("scan_reconcile", started.elapsed().as_micros());
    let started = std::time::Instant::now();
    let planned = items.len();
    for outcome in scan_parallel(items) {
        merge_outcome(doc, outcome);
    }
    perf.record_micros("scan_decode", started.elapsed().as_micros());
    // 会话目录被删 / 整个实例清空 → 账目随之消失，汇总自动回落。
    doc.files.retain(|key, _| seen.contains(key));
    prune_window(doc, &cutoff);
    planned
}

/// 一个待解码的文件：从 `start_offset`（`reset` 时为 0）续扫。
struct ScanItem {
    key: String,
    path: PathBuf,
    cutoff: String,
    #[allow(dead_code, reason = "扫描结果里记录本文件的起始偏移，用于增量续扫")]
    #[allow(dead_code, reason = "增量续扫的起点，当前走全量重算")]
    start_offset: u64,
    reset: bool,
}

/// 一个文件的解码结果：`days` 只含本次解码的记录，合账时叠加进旧账。
struct ScanOutcome {
    key: String,
    /// 本文件的起始偏移，原为增量续扫保留；当前账目走全量重算，不再回读它。
    #[allow(dead_code, reason = "增量续扫的起点，当前走全量重算")]
    start_offset: u64,
    reset: bool,
    offset: u64,
    days: BTreeMap<String, BTreeMap<String, ModelDay>>,
}

/// 串行段：对账 + 决定这个文件要不要解、从哪里解。旧文件（mtime 早于
/// 窗口）在这里被整文件跳过，一条都不用解。
fn plan_item(
    doc: &mut UsageStateDoc,
    key: &str,
    file: &Path,
    now_ms: u64,
    cutoff: &str,
    items: &mut Vec<ScanItem>,
) {
    let Ok(meta) = fs::metadata(file) else {
        return;
    };
    let size = meta.len();
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let known_offset = doc.files.get(key).map_or(0, |entry| entry.offset);
    // 文件被截断 / 重写：这份账作废重来。增量账按文件独立，重扫不会与
    // 旧账叠加。
    let reset = size < known_offset;
    let mut start = if reset { 0 } else { known_offset };
    if start == 0 && mtime_ms > 0 && mtime_ms + (RETENTION_DAYS + 1) * 86_400_000 < now_ms {
        // 最后修改时间都在窗口之前 ⇒ 文件里每条记录都早于窗口。
        start = size;
    }
    if start < size {
        items.push(ScanItem {
            key: key.to_string(),
            path: file.to_path_buf(),
            cutoff: cutoff.to_string(),
            start_offset: start,
            reset,
        });
        return;
    }
    // 没有新内容：首次见到的文件也要建条目记下 offset（下次增量才有落点），
    // 截断过的文件则趁机清掉作废的旧账。
    let entry = doc.files.entry(key.to_string()).or_default();
    if reset {
        entry.days.clear();
    }
    entry.offset = start;
}

/// 并行段：把解码任务分给至多 8 个线程（`thread::scope` 无新依赖；账目在
/// 各线程里各自独立构建，串行段合账时才交汇）。
fn scan_parallel(items: Vec<ScanItem>) -> Vec<ScanOutcome> {
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .min(8);
    if items.len() < 2 || workers < 2 {
        return items.into_iter().map(scan_item).collect();
    }
    let queue: Mutex<std::collections::VecDeque<ScanItem>> = Mutex::new(items.into());
    let outcomes: Mutex<Vec<ScanOutcome>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let item = queue
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .pop_front();
                let Some(item) = item else { break };
                let outcome = scan_item(item);
                outcomes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(outcome);
            });
        }
    });
    outcomes
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 工作线程：解码一个文件的新内容。任何失败都按“这次没扫到”处理——账目
/// 滞后、offset 不推进，下一轮扫描从头再试。
fn scan_item(item: ScanItem) -> ScanOutcome {
    let mut outcome = ScanOutcome {
        key: item.key,
        start_offset: item.start_offset,
        reset: item.reset,
        offset: item.start_offset,
        days: BTreeMap::new(),
    };
    let Ok(mut handle) = fs::File::open(&item.path) else {
        return outcome;
    };
    if handle.seek(SeekFrom::Start(item.start_offset)).is_err() {
        return outcome;
    }
    let mut days = BTreeMap::new();
    let cutoff = item.cutoff.as_str();
    let consumed = decode_tail(&mut handle, &mut |line| {
        accumulate_line(line, cutoff, &mut days)
    });
    // 坏帧处 consumed 停在该帧起点，之后的内容下一轮从这里续扫。
    outcome.offset = item.start_offset + consumed;
    outcome.days = days;
    outcome
}

/// 串行段：把一个文件的解码结果叠进总账。
fn merge_outcome(doc: &mut UsageStateDoc, outcome: ScanOutcome) {
    let entry = doc.files.entry(outcome.key).or_default();
    if outcome.reset {
        entry.days.clear();
    }
    for (day, models) in outcome.days {
        let day_bucket = entry.days.entry(day).or_default();
        for (model, model_day) in models {
            let bucket = day_bucket.entry(model).or_default();
            bucket.requests += model_day.requests;
            bucket.input += model_day.input;
            bucket.output += model_day.output;
            bucket.cache_read += model_day.cache_read;
            bucket.cache_write += model_day.cache_write;
        }
    }
    entry.offset = outcome.offset;
}

/// 窗口外的日账在每次保存前剪掉——这是存储体积的硬上界。
fn prune_window(doc: &mut UsageStateDoc, cutoff: &str) {
    for entry in doc.files.values_mut() {
        entry.days.retain(|day, _| day.as_str() >= cutoff);
    }
}

/// 从当前位置逐帧解码到 EOF，返回消费的压缩字节数。
///
/// 输出按**完整帧**交付：帧没解完（尾部残帧 / 损坏）就整体丢弃本帧输出并
/// 停住——返回值不含该帧，下次扫描从它的起点重试。内核按帧原子追加，这
/// 意味着正常运行的文件永远不会走这条分支；只有崩溃留下的半帧会。
fn decode_tail<R: Read>(reader: &mut R, on_line: &mut dyn FnMut(&[u8])) -> u64 {
    // 单帧解出的内容上限。真实帧是内核每轮写入的一小段（通常 <1MB）；
    // 超限按坏帧处理，防止异常构造的帧把内存吃穿。
    const MAX_FRAME_OUTPUT: usize = 64 * 1024 * 1024;
    let mut decoder = FrameDecoder::new();
    let mut frame_out: Vec<u8> = Vec::new();
    let mut splitter = LineSplitter { buf: Vec::new() };
    let chunk = [0u8; 32 * 1024];
    let mut consumed: u64 = 0;
    loop {
        // reset 读帧头：干净的 EOF 和无法识别的数据都在这里停住。
        if decoder.reset(&mut *reader).is_err() {
            return consumed;
        }
        frame_out.clear();
        let mut broken = false;
        while !decoder.is_finished() {
            if decoder
                .decode_blocks(&mut *reader, BlockDecodingStrategy::UptoBytes(chunk.len()))
                .is_err()
            {
                broken = true;
                break;
            }
            let mut tmp = [0u8; 32 * 1024];
            while decoder.can_collect() > 0 {
                match decoder.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        if frame_out.len().saturating_add(n) > MAX_FRAME_OUTPUT {
                            broken = true;
                            break;
                        }
                        frame_out.extend_from_slice(&tmp[..n]);
                    }
                    Err(_) => {
                        broken = true;
                        break;
                    }
                }
            }
            if broken {
                break;
            }
        }
        if broken {
            return consumed;
        }
        splitter.feed(&frame_out, on_line);
        consumed += decoder.bytes_read_from_source();
    }
}

/// 把解码输出按 `\n` 切成完整行；末尾不带换行的残行留到下一个 chunk。
///
/// **外部来源那份账目也用它**（`usage/external/codex.rs`）：切行的语义与
/// 压缩层无关，两个来源的解码器不同但行边界是同一件事。
#[derive(Default)]
pub(crate) struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    pub(crate) fn feed(&mut self, chunk: &[u8], on_line: &mut dyn FnMut(&[u8])) {
        let mut start = 0;
        while let Some(pos) = chunk[start..].iter().position(|&b| b == b'\n') {
            let end = start + pos;
            self.buf.extend_from_slice(&chunk[start..end]);
            on_line(&self.buf);
            self.buf.clear();
            start = end + 1;
        }
        self.buf.extend_from_slice(&chunk[start..]);
    }

    /// 把缓冲区里剩下的内容当作**最后一行**交出去。
    ///
    /// 文件末尾没有换行时那最后一行会一直留在缓冲区里——不 flush 就等于
    /// 把它丢了。内核的 session 文件每行都有换行，所以这条路径平时走不到；
    /// **Codex 的 rollout 文件最后一行没有换行**，不 flush 就等于**每个会话
    /// 少算最后一次请求**（2026-10-24 单测抓到的就是这个）。
    pub(crate) fn flush(&mut self, on_line: &mut dyn FnMut(&[u8])) {
        if !self.buf.is_empty() {
            on_line(&self.buf);
            self.buf.clear();
        }
    }
}

const ASSISTANT_TYPE_NEEDLE: &[u8] = b"\"assistant/message\"";

/// 单行 JSONL → 账目。只认 `type == "assistant/message"`、来源是真实模型调用
/// （`source.kind == "model"`）且带 usage 的记录；用户消息、工具事件、乃至
/// 用户正文里恰好嵌着同款字符串的行都一律忽略——判据是解析后的顶层 `type`。
fn accumulate_line(
    line: &[u8],
    cutoff: &str,
    days: &mut BTreeMap<String, BTreeMap<String, ModelDay>>,
) {
    let line = btrim(line);
    if line.is_empty() || !contains_needle(line, ASSISTANT_TYPE_NEEDLE) {
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
        return;
    };
    if value.get("type").and_then(|v| v.as_str()) != Some("assistant/message") {
        return;
    }
    let Some(time_ms) = value.get("time").and_then(|v| v.as_u64()) else {
        return;
    };
    let day = date_string_of_ms(time_ms);
    if day.as_str() < cutoff {
        return; // 90 天窗口之外：丢弃，不入账。
    }
    let Some(usage) = value.get("data").and_then(|d| d.get("usage")) else {
        return;
    };
    let source = value
        .get("data")
        .and_then(|d| d.get("message"))
        .and_then(|m| m.get("source"));
    if source.and_then(|s| s.get("kind")).and_then(|v| v.as_str()) != Some("model") {
        return;
    }
    let (Some(provider), Some(model)) = (
        source
            .and_then(|s| s.get("provider"))
            .and_then(|v| v.as_str()),
        source.and_then(|s| s.get("model")).and_then(|v| v.as_str()),
    ) else {
        return;
    };
    let input = usage_num(usage, "inputTokens");
    let output = usage_num(usage, "outputTokens");
    let cache_read = usage_num(usage, "cacheReadTokens");
    let cache_write = usage_num(usage, "cacheWriteTokens");
    if input + output + cache_read + cache_write == 0 {
        return; // 没有可计量的 token（异常记录），不占请求次数。
    }
    let bucket = days
        .entry(day)
        .or_default()
        .entry(format!("{provider}/{model}"))
        .or_default();
    bucket.requests += 1;
    bucket.input += input;
    bucket.output += output;
    bucket.cache_read += cache_read;
    bucket.cache_write += cache_write;
}

fn usage_num(value: &serde_json::Value, field: &str) -> u64 {
    match value.get(field) {
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0).round() as u64))
            .unwrap_or(0),
        _ => 0,
    }
}

fn contains_needle(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn btrim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |p| p + 1);
    &bytes[start..end]
}

/// 统计窗口起点（含）：今天 − (RETENTION_DAYS−1) 天的本地日历日。
fn cutoff_date_string(now_ms: u64) -> String {
    date_string_of_ms(now_ms.saturating_sub((RETENTION_DAYS - 1) * 86_400_000))
}

fn date_string_of_ms(ms: u64) -> String {
    crate::shell::localtime::local_date_string(UNIX_EPOCH + Duration::from_millis(ms))
}

/// 把账目求和成面板视图：`days` 恒覆盖整个 90 天窗口（缺的日期补零），
/// `today_tokens` 是今日（本地日历日）的用量（卡片口径）。
fn derive_view(doc: &UsageStateDoc, now_ms: u64) -> UsageView {
    let cutoff = cutoff_date_string(now_ms);
    let mut models: BTreeMap<String, ModelUsageView> = BTreeMap::new();
    let mut by_day: BTreeMap<String, DayUsageView> = BTreeMap::new();
    for entry in doc.files.values() {
        for (day, per_model) in &entry.days {
            if day.as_str() < cutoff.as_str() {
                continue;
            }
            let day_view = by_day.entry(day.clone()).or_insert_with(|| DayUsageView {
                date: day.clone(),
                tokens: 0,
                requests: 0,
                models: BTreeMap::new(),
            });
            for (key, model_day) in per_model {
                let model = models.entry(key.clone()).or_insert_with(|| {
                    let (provider, model) = key.split_once('/').unwrap_or((key.as_str(), ""));
                    ModelUsageView {
                        key: key.clone(),
                        provider: provider.to_string(),
                        model: model.to_string(),
                        requests: 0,
                        tokens: 0,
                        input: 0,
                        output: 0,
                        cache_read: 0,
                    }
                });
                let tokens = model_day.input
                    + model_day.output
                    + model_day.cache_read
                    + model_day.cache_write;
                model.requests += model_day.requests;
                model.tokens += tokens;
                model.input += model_day.input;
                model.output += model_day.output;
                model.cache_read += model_day.cache_read;
                day_view.tokens += tokens;
                day_view.requests += model_day.requests;
                *day_view.models.entry(key.clone()).or_insert(0) += tokens;
            }
        }
    }

    // 逐日推进补齐窗口内的零日；日期解析失败（不会发生）时退化为仅已有日。
    const DAY_FORMAT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");
    let mut days: Vec<DayUsageView> = Vec::new();
    let mut cursor = time::Date::parse(&cutoff, &DAY_FORMAT).ok();
    let end = time::Date::parse(&date_string_of_ms(now_ms), &DAY_FORMAT).ok();
    while let Some(date) = cursor {
        let key = date.format(&DAY_FORMAT).unwrap_or_default();
        days.push(by_day.remove(&key).unwrap_or_else(|| DayUsageView {
            tokens: 0,
            requests: 0,
            models: BTreeMap::new(),
            date: key.clone(),
        }));
        if Some(date) == end {
            break;
        }
        match date.next_day() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    days.extend(by_day.into_values());

    let mut models: Vec<ModelUsageView> = models.into_values().collect();
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.key.cmp(&b.key)));
    let total_tokens = days.iter().map(|d| d.tokens).sum();
    // 今日口径按日期字符串精确匹配，不取 days.last()：晚于今天的异常日期
    // （时钟漂移 / 手工改过的 session 文件）会被下面的 extend 追加到序列末尾。
    let today_key = date_string_of_ms(now_ms);
    let (today_tokens, today_requests) = days
        .iter()
        .find(|d| d.date == today_key)
        .map(|d| (d.tokens, d.requests))
        .unwrap_or((0, 0));
    UsageView {
        retention_days: RETENTION_DAYS,
        last_scanned_at_ms: doc.last_scanned_at_ms,
        tracked_files: doc.files.len(),
        total_tokens,
        total_requests: days.iter().map(|d| d.requests).sum(),
        today_tokens,
        today_requests,
        active_days: days.iter().filter(|d| d.requests > 0).count() as u64,
        top_model: models.first().map(|m| m.key.clone()),
        models,
        days,
        // 这两项由 `usage_view` 在内核账目之外填：外部来源不进内核那份日
        // 序列（口径与命名空间都不同），只是并列的一块展示。
        external_sources: crate::usage::external::ExternalSources::default(),
        external: Vec::new(),
    }
}

/// `get_model_usage`：面板读取模型用量。
///
/// 重活在 `spawn_blocking` 里做：首次全量扫描要解全部 session 文件
/// （数百 MB 级），绝不能占 Tauri 主线程。
///
/// `external` 是本次**要扫**的外部来源（Codex / MiniMax Desktop）。省略或
/// 传 `null` = **按已保存的偏好扫**（窗口首次打开走这条）；显式给出 = 只扫
/// 给定的那些（概览卡片显式传全 `false`，它不显示外部来源）。视图里的
/// `external_sources` 恒给出**已保存的勾选状态**，前端靠它回填 checkbox。
#[tauri::command]
pub async fn get_model_usage(
    force: Option<bool>,
    external: Option<crate::usage::external::ExternalSources>,
) -> Result<UsageView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        usage_view(force.unwrap_or(false), external)
    })
    .await
    .map_err(|join| {
        format!(
            "后台任务异常结束（{join}）。请重试；若持续出现，请在终端用 `npm run dev` 启动以便看到完整输出"
        )
    })?
    .map_err(|e| e.to_string())
}

/// 在独立可缩放窗口中打开模型用量统计。
///
/// 管理窗口被固定为 480×800（tauri.conf.json），热力图 / 趋势 / 环形图
/// 需要可缩放的横向空间——与日志「全屏」同一条路：展示交给专属 OS 窗口
/// （`?usage=1` 挂载 `UsageWindow.vue`，capability `usage-viewer.json`
/// 只授予 `get_model_usage`）。窗口尺寸 760×800：**高度与主壳一致**，
/// 打开时吸附在主窗右侧、顶边对齐（右侧贴不下屏幕就贴左侧，垂直超出行
/// 程就上移夹在屏内，详见 `crate::shell::window::dock_x` / `dock_y`）。构造过程
/// 与 `open_log_window` 一致：webview 在新线程上构建（Windows 主线程同
/// 步建 webview 会死锁）；已有窗口先销毁再重建——窗口是只读的，重建即顺
/// 手拿到一次 force 重扫，不损失任何状态。
#[tauri::command]
pub async fn open_usage_window(app: tauri::AppHandle) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("dsh-open-usage-viewer".into())
        .spawn(move || {
            if let Some(existing) = handle.get_webview_window("usage-viewer") {
                let _ = existing.destroy();
            }
            use crate::shell::window::window_title;
            // 吸附定位取主窗的物理坐标 + 缩放比换算成逻辑坐标交给 builder；
            // 主窗不在（理论上不会）或取不到显示器信息时保持默认居中。
            let dock = handle.get_webview_window("main").and_then(|main| {
                crate::shell::window::dock_position_logical(
                    &main,
                    crate::shell::window::USAGE_VIEWER_SIZE,
                )
            });
            let mut builder = tauri::WebviewWindowBuilder::new(
                &handle,
                "usage-viewer",
                tauri::WebviewUrl::App("index.html?usage=1".into()),
            )
            .title(window_title("模型用量"))
            .inner_size(
                crate::shell::window::USAGE_VIEWER_SIZE.width,
                crate::shell::window::USAGE_VIEWER_SIZE.height,
            )
            .min_inner_size(720.0, 520.0)
            .resizable(true)
            .theme(crate::shell::appearance::initial_theme("usage-viewer"));
            // 无边框 + 透明 + 无系统阴影（交通灯由前端自绘，与主壳同一份视觉）。
            // 三项必须在**建窗期**给：事后 `set_decorations(false)` 会抹掉
            // `Miniaturizable`，黄灯随即变成死按钮。依据见该函数的文档注释。
            builder = crate::shell::window::decorate_transparent(builder);
            if let Some((x, y)) = dock {
                builder = builder.position(x, y);
            }
            let result = builder.build();
            if result.is_ok() {
                // 建出后以实测外框校正：外框高度对齐主壳 + 补偿不可见边框贴紧。
                crate::shell::window::snap_to_main(
                    &handle,
                    crate::shell::window::USAGE_VIEWER_LABEL,
                );
            }
            let result = result
                .map(|_| ())
                .map_err(|e| format!("打开模型用量窗口失败：{e}。请重试"));
            let _ = tx.send(result);
        })
        .map_err(|e| format!("无法启动用量窗口线程：{e}"))?;

    // 等待与超时同 open_log_window：放 blocking 线程上，不占 tokio worker；
    // 超时按失败上报，让用户至少知道发生了什么。
    tauri::async_runtime::spawn_blocking(move || {
        match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(result) => result,
            Err(_) => Err("打开模型用量窗口超时（20 秒）。请重试".into()),
        }
    })
    .await
    .map_err(|join| format!("后台任务异常结束（{join}）。请重试"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::perf::PerfSample;

    /// 手工构造单帧 zstd 裸块流（Raw block 不压缩，规范固定可静态编码）：
    /// magic + FHD(0x00) + Window_Descriptor(0x00 → 1KiB 窗口，满足 ruzstd
    /// 的 MIN_WINDOW_SIZE) + 块头 + 原文。不依赖任何 zstd 编码器依赖即可
    /// 测多帧增量与残帧停住。
    fn raw_frame(content: &[u8], last: bool) -> Vec<u8> {
        assert!(content.len() < (1 << 17), "测试用裸块放不下");
        let mut out = vec![0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x00];
        let header = ((content.len() as u32) << 3) | u32::from(last);
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(content);
        out
    }

    fn collect_decoded(input: &[u8]) -> (u64, Vec<String>) {
        let mut reader = std::io::Cursor::new(input.to_vec());
        let mut lines = Vec::new();
        let consumed = decode_tail(&mut reader, &mut |line| {
            lines.push(String::from_utf8_lossy(line).into_owned());
        });
        (consumed, lines)
    }

    #[test]
    fn raw_frames_decode_in_order_across_frames() {
        // 每个帧都以 last block 结尾（合法 zstd 的前提）；多帧 = 完整帧相连，
        // 与内核的追加写完全一致。
        let first = raw_frame(b"line-1\n", true);
        let second = raw_frame(b"line-2\nline-3\n", true);
        let mut stream = first.clone();
        stream.extend_from_slice(&second);
        let (consumed, lines) = collect_decoded(&stream);
        assert_eq!(lines, vec!["line-1", "line-2", "line-3"]);
        assert_eq!(consumed, stream.len() as u64);
        // 帧边界：消费量减去第二帧长度必须正好落在第一帧末尾。
        assert_eq!(consumed as usize - second.len(), first.len());
    }

    #[test]
    fn torn_tail_frame_stops_at_frame_start_and_delivers_nothing_from_it() {
        let first = raw_frame(b"a\n", true);
        let second = raw_frame(b"b\n", true);
        let mut full = first.clone();
        full.extend_from_slice(&second);
        // 模拟崩溃留下的半帧：文件截断在第二帧的中部。
        let torn = full[..first.len() + 5].to_vec();
        let (consumed, lines) = collect_decoded(&torn);
        assert_eq!(lines, vec!["a"], "完整帧的内容必须交付");
        assert_eq!(consumed, first.len() as u64, "offset 必须停在残帧起点");
    }

    /// 真实内核流的对拍样张：zstd CLI 压出的两帧流，第一帧含一条
    /// assistant/message 与一条 user/message，第二帧含带模型来源的记录。
    const REAL_TWO_FRAME_ZSTD: &str = "28b52ffd249745030052c6141990a939740a891f8de873234944b3196142f56c73b3ebf9450765d70861f3139fa9a0405565770357c9c638cc9a984884ab249197292ff3fd0c36c639672038cc88a0db180fa78a08e07957a47a3663b2b857060050105086216a287a4d6b485daf019401cb42b5a528b52ffd24bd2d0400c2881b1c703571038420b5fd456bf24951f2b702ad8a6fff4fea90264085601c81f75e71740b8b38f4380ebaad632926cf6a9165e8c614bacb767cefe98967cf277b10c04ad8f1893d2b3d7f14428ea42dbecbbbbcb69ea3298661100a93df84faf83d28f326412b6d46ccfafcb280be0b07004a91848d8d145f99cecc2d2d2af5a604a007a6e82711";

    #[test]
    fn real_kernel_stream_decodes_and_accumulates_only_model_usage() {
        let bytes: Vec<u8> = (0..REAL_TWO_FRAME_ZSTD.len() / 2)
            .map(|i| u8::from_str_radix(&REAL_TWO_FRAME_ZSTD[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        let mut days = BTreeMap::new();
        // 样张记录的时间是 2026-09-25 前后：窗口起点取 0（全收）。
        let mut reader = std::io::Cursor::new(bytes.clone());
        let consumed = decode_tail(&mut reader, &mut |line| {
            accumulate_line(line, "0000-00-00", &mut days)
        });
        assert_eq!(consumed, bytes.len() as u64);
        let all: &BTreeMap<String, ModelDay> = days.values().next().expect("应有入账记录");
        let key = "p/m";
        let day = all.get(key).expect("带模型来源的记录应入账");
        assert_eq!(day.requests, 1);
        assert_eq!(day.input, 100);
        assert_eq!(day.output, 5);
        assert_eq!(day.cache_read, 7);
        // 第一帧那条没有 source 的 assistant/message 不该计进任何其他模型键。
        assert_eq!(all.len(), 1, "无模型来源的记录不得入账");
    }

    #[test]
    fn accumulate_line_filters_non_model_and_out_of_window_records() {
        let now = process::epoch_millis();
        let today = date_string_of_ms(now);
        let mut days = BTreeMap::new();
        let model_line = format!(
            r#"{{"type":"assistant/message","time":{now},"data":{{"message":{{"source":{{"kind":"model","provider":"minimax-cn","model":"MiniMax-M3"}}}},"usage":{{"inputTokens":10,"outputTokens":3,"cacheReadTokens":4}}}}}}"#
        );
        accumulate_line(model_line.as_bytes(), &today, &mut days);
        let day_bucket = days.get(&today).expect("今天的记录应入账");
        let bucket = day_bucket
            .get("minimax-cn/MiniMax-M3")
            .expect("模型键应为 provider/model");
        assert_eq!(bucket.requests, 1);
        assert_eq!(bucket.input, 10);
        assert_eq!(bucket.output, 3);
        assert_eq!(bucket.cache_read, 4);
        assert_eq!(bucket.cache_write, 0);

        // 用户正文里嵌着同款字符串：顶层 type 不是 assistant/message，忽略。
        let embedded = format!(
            r#"{{"type":"user/message","time":{now},"data":{{"content":"{{\"type\":\"assistant/message\"}}"}}}}"#
        );
        accumulate_line(embedded.as_bytes(), &today, &mut days);
        assert_eq!(days.len(), 1, "用户消息不得入账");

        // 90 天窗口外的记录：丢弃。
        let old_ms = now - (RETENTION_DAYS + 30) * 86_400_000;
        let old_line = model_line.replace(&now.to_string(), &old_ms.to_string());
        accumulate_line(old_line.as_bytes(), &today, &mut days);
        assert_eq!(days.len(), 1, "窗口外记录不得入账");

        // 非 model 来源（如 seed / 工具回放）不入账。
        let seeded = model_line.replace("\"kind\":\"model\"", "\"kind\":\"seed\"");
        accumulate_line(seeded.as_bytes(), &today, &mut days);
        assert_eq!(
            days.get(&today)
                .unwrap()
                .get("minimax-cn/MiniMax-M3")
                .unwrap()
                .requests,
            1
        );
    }

    #[test]
    fn truncate_resets_account_and_append_continues_from_offset() {
        let dir = tempfile_fresh("usage-update-");
        // scan_in 期望 `<root>/<工作区>/<会话目录>/` 两层结构。
        let ws = dir.join("workdir");
        let file_dir = ws.join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("session.jsonl.zstd");
        let first = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(&file, &first).unwrap();

        let mut doc = UsageStateDoc::default();
        let now = process::epoch_millis();
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        let key = "workdir/session-x/session.jsonl.zstd";
        assert_eq!(doc.files.get(key).unwrap().offset, first.len() as u64);

        // 追加一帧：只消费新帧（offset 推进到新的文件长度）。
        let mut grown = first.clone();
        grown.extend_from_slice(&raw_frame(b"{\"type\":\"assistant/message\"}\n", true));
        fs::write(&file, &grown).unwrap();
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        assert_eq!(doc.files.get(key).unwrap().offset, grown.len() as u64);

        // 截断重写：账目作废，offset 归零后重扫到新长度。
        let shrunk = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(&file, &shrunk).unwrap();
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        let entry = doc.files.get(key).unwrap();
        assert_eq!(
            entry.offset,
            shrunk.len() as u64,
            "截断后应重扫并推进到新末尾"
        );

        // 会话目录整体消失：账目随之移除。
        //
        // **前提要跨过一个时钟 tick**：目录条目增删才会刷新目录 mtime，而
        // 「这一轮该读哪个文件」正是靠 `variant_cache::resolve_variant` 的
        // 目录 mtime 闸判缓存失效。删文件与上一轮扫描若落在同一个 tick
        // （Windows 默认粒度 15.6ms），两轮读到一模一样的目录 mtime，缓存
        // 继续命中并返回**那个已经删掉的文件**的路径——这一轮照样把它算作
        // 「还在」，账目就不会被移除。2026-10-20 的 Windows CI（run
        // 39363486523）红的就是这一句：红的是时钟精度，不是账目逻辑。
        //
        // 与兄弟用例 `a_new_generation_in_the_same_session_dir_is_still_picked_up`
        // 同一形状、同一处校准（40ms）。
        let mtime_before = fs::metadata(&file_dir).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(40));
        fs::remove_file(&file).unwrap();
        assert!(
            fs::metadata(&file_dir).unwrap().modified().unwrap() > mtime_before,
            "前提：跨过时钟 tick 后删除目录条目必须刷新目录 mtime——\
             不成立时这条用例验的是时间戳精度，不是缓存失效路径"
        );
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        assert!(!doc.files.contains_key(key), "文件消失后账目必须移除");
        fs::remove_dir_all(&dir).ok();
    }

    /// mtime 闸唯一会漏账的形状：**同一个会话目录里出现新一代 variant 文件**。
    /// 上一轮刚把这个目录缓存成 v1，这一轮目录 mtime 变了，必须失效并重新
    /// 枚举到 v2——否则缓存把「版本号最高的那一代」永久钉死在旧值上。
    /// 反向验：把 `resolve_variant` 的 mtime 比较去掉，这条必红。
    #[test]
    fn a_new_generation_in_the_same_session_dir_is_still_picked_up() {
        let dir = tempfile_fresh("usage-variant-swap-");
        let ws = dir.join("workdir");
        let session = ws.join("session-x");
        fs::create_dir_all(&session).unwrap();
        let frame = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(session.join("session.jsonl.zstd"), &frame).unwrap();

        let mut doc = UsageStateDoc::default();
        let now = process::epoch_millis();
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        assert!(
            doc.files
                .contains_key("workdir/session-x/session.jsonl.zstd"),
            "首轮应记下 v1"
        );
        assert!(
            !doc.files
                .contains_key("workdir/session-x/session.v2.jsonl.zstd"),
            "首轮还不该看见 v2"
        );

        // 内核升级：同一目录里多出 v2。目录条目变了 ⇒ 目录 mtime 必变——但
        // 这条前提要**跨过一个时钟 tick** 才成立：文件时间戳取的是系统时间，
        // 而系统时间只在时钟中断时推进（Windows 默认粒度 15.6ms），上面那几步
        // （建目录 → 写 v1 → 扫描）与写 v2 若落在同一个 tick 里，两轮读到的
        // 目录 mtime 一模一样，闸门据此判「目录没变」，红的是时钟而不是闸门
        // ——2026-10-16 的 Windows CI（run 38923395574）就是这样红的，而这几步
        // 实测只花几毫秒。40ms 与 `variant_cache::advance_dir_mtime` 同一处校准。
        let mtime_before_v2 = fs::metadata(&session).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(40));
        fs::write(session.join("session.v2.jsonl.zstd"), &frame).unwrap();
        assert!(
            fs::metadata(&session).unwrap().modified().unwrap() > mtime_before_v2,
            "前提：跨过时钟 tick 后新增目录条目必须刷新目录 mtime——\
             不成立时这条用例验的是时间戳精度，不是 mtime 闸的失效路径"
        );
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        assert!(
            doc.files
                .contains_key("workdir/session-x/session.v2.jsonl.zstd"),
            "新一代 variant 出现后必须被扫到（目录 mtime 闸的失效路径）"
        );
        assert!(
            !doc.files
                .contains_key("workdir/session-x/session.jsonl.zstd"),
            "只认版本号最高的一代：v1 的账目应随之移除"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 只追加内容**不改目录 mtime**，所以这条走的是缓存的命中路径：变体
    /// 路径没变，offset 仍要靠 `plan_item` 对文件本身取 metadata 推进。
    /// 少看了这一条，缓存就会把「新增的用量」整段漏掉。
    #[test]
    fn appending_to_an_existing_variant_still_advances_the_offset() {
        let dir = tempfile_fresh("usage-append-cache-");
        let ws = dir.join("workdir");
        let session = ws.join("session-x");
        fs::create_dir_all(&session).unwrap();
        let file = session.join("session.jsonl.zstd");
        let first = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(&file, &first).unwrap();

        let mut doc = UsageStateDoc::default();
        let now = process::epoch_millis();
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        let key = "workdir/session-x/session.jsonl.zstd";
        assert_eq!(doc.files.get(key).unwrap().offset, first.len() as u64);

        // 第二轮之前先确认目录 mtime 确实没被这次追加改动——这是上面那条
        // 测试能代表「命中路径」的前提，不成立的话它就名不副实。
        let dir_mtime_before = fs::metadata(&session).unwrap().modified().unwrap();
        let mut grown = first.clone();
        grown.extend_from_slice(&raw_frame(b"{\"type\":\"assistant/message\"}\n", true));
        fs::write(&file, &grown).unwrap();

        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        assert_eq!(
            doc.files.get(key).unwrap().offset,
            grown.len() as u64,
            "追加后 offset 必须推进到新末尾"
        );
        assert_eq!(
            fs::metadata(&session).unwrap().modified().unwrap(),
            dir_mtime_before,
            "前提：向已存在的文件追加不改变所在目录的 mtime——\
             缓存正是靠这条性质把「增删条目」与「追加内容」分开"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 临时基准（测量用，非门禁）：同进程内连跑 N 轮 `scan_in`，只测
    /// 遍历 + 对账那一段，取中位数。首轮必然含全量解码的冷成本，所以从
    /// 第二轮开始统计。
    #[test]
    #[ignore]
    fn bench_scan_walk_only() {
        let root = std::path::PathBuf::from(
            std::env::var("XLINK_BENCH_SESSIONS").expect("XLINK_BENCH_SESSIONS"),
        );
        let now = process::epoch_millis();
        let mut doc = UsageStateDoc::default();
        let mut times: Vec<f64> = Vec::new();
        for i in 0..12 {
            let started = std::time::Instant::now();
            scan_in(&mut doc, &root, now, &mut PerfSample::disabled());
            let ms = started.elapsed().as_secs_f64() * 1e3;
            if i > 0 {
                times.push(ms);
            }
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "SCANWALK n={} p50={:.2}ms min={:.2}ms max={:.2}ms files={}",
            times.len(),
            times[times.len() / 2],
            times[0],
            times[times.len() - 1],
            doc.files.len(),
        );
    }

    #[test]
    fn old_mtime_file_is_skipped_without_decoding() {
        let dir = tempfile_fresh("usage-old-");
        let ws = dir.join("workdir");
        let file_dir = ws.join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("session.jsonl.zstd");
        fs::write(&file, raw_frame(b"x\n", true)).unwrap();
        let old_ms = process::epoch_millis() - (RETENTION_DAYS + 10) * 86_400_000;
        // mtime 改不动就跳过该用例（CI 文件系统差异），断言主体是跳过逻辑。
        let _ = filetime_set(&file, old_ms);

        let mut doc = UsageStateDoc::default();
        let now = process::epoch_millis();
        scan_in(&mut doc, &dir, now, &mut PerfSample::disabled());
        let key = "workdir/session-x/session.jsonl.zstd";
        let entry = doc.files.get(key).expect("文件应建立账目");
        let size = fs::metadata(&file).unwrap().len();
        if entry.offset == size {
            assert!(entry.days.is_empty(), "窗口外的文件不应有任何账");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_view_sums_models_fills_zero_days_and_computes_today() {
        let now = process::epoch_millis();
        let today = date_string_of_ms(now);
        let three_days_ago = date_string_of_ms(now - 3 * 86_400_000);
        let old = date_string_of_ms(now - (RETENTION_DAYS + 5) * 86_400_000);
        let mut doc = UsageStateDoc::default();
        let mut entry = FileEntry::default();
        let bucket = |r: u64, i: u64, o: u64| ModelDay {
            requests: r,
            input: i,
            output: o,
            cache_read: 0,
            cache_write: 0,
        };
        entry
            .days
            .entry(today.clone())
            .or_default()
            .insert("a/Alpha".into(), bucket(2, 100, 10));
        entry
            .days
            .entry(today.clone())
            .or_default()
            .insert("b/Beta".into(), bucket(1, 50, 5));
        entry
            .days
            .entry(three_days_ago.clone())
            .or_default()
            .insert("a/Alpha".into(), bucket(1, 20, 2));
        // 窗口外：不计入视图。
        entry
            .days
            .entry(old.clone())
            .or_default()
            .insert("a/Alpha".into(), bucket(9, 999, 9));
        doc.files.insert("f".into(), entry);

        let view = derive_view(&doc, now);
        assert_eq!(view.days.len() as u64, RETENTION_DAYS, "窗口恒覆盖 90 天");
        assert_eq!(view.days.last().unwrap().date, today);
        assert_eq!(view.total_tokens, 100 + 10 + 50 + 5 + 20 + 2);
        assert_eq!(view.total_requests, 4);
        assert_eq!(view.active_days, 2);
        assert_eq!(
            view.today_tokens,
            100 + 10 + 50 + 5,
            "今日只含今天的日账，不含 3 天前的"
        );
        assert_eq!(view.top_model.as_deref(), Some("a/Alpha"));
        assert_eq!(view.models[0].key, "a/Alpha");
        assert_eq!(view.models[0].provider, "a");
        assert_eq!(view.models[0].model, "Alpha");
        let today_view = view.days.last().unwrap();
        assert_eq!(today_view.models.get("b/Beta"), Some(&(50 + 5)));
        // 中间的零日必须补出来，否则热力图会错位。
        let yesterday = date_string_of_ms(now - 86_400_000);
        let zero = view
            .days
            .iter()
            .find(|d| d.date == yesterday)
            .expect("零日应补齐");
        assert_eq!(zero.tokens, 0);
        assert_eq!(zero.requests, 0);
    }

    #[test]
    fn derive_view_today_survives_future_dated_entries() {
        // 时钟漂移 / 手工改过的 session 文件可能产出晚于今天的日期键；
        // 它会被 extend 追加到 days 末尾，今日口径必须仍按日期精确命中今天。
        let now = process::epoch_millis();
        let today = date_string_of_ms(now);
        let tomorrow = date_string_of_ms(now + 86_400_000);
        let mut doc = UsageStateDoc::default();
        let mut entry = FileEntry::default();
        entry.days.entry(today.clone()).or_default().insert(
            "a/Alpha".into(),
            ModelDay {
                requests: 1,
                input: 30,
                output: 3,
                cache_read: 0,
                cache_write: 0,
            },
        );
        entry.days.entry(tomorrow.clone()).or_default().insert(
            "a/Alpha".into(),
            ModelDay {
                requests: 7,
                input: 700,
                output: 70,
                cache_read: 0,
                cache_write: 0,
            },
        );
        doc.files.insert("f".into(), entry);

        let view = derive_view(&doc, now);
        assert_eq!(
            view.days.last().unwrap().date,
            tomorrow,
            "前置：未来日确实被追加到末尾（days.last() 不可用作今日口径）"
        );
        assert_eq!(view.today_tokens, 33, "今日只数今天的日账，不被未来日顶替");
        // tokens 与 requests 由**同一次**匹配算出。曾经只回 tokens，窗口的
        // 「今日 N tokens · N 次」只好自己去 days 里找那一天——「今日」于是有了
        // 两套定义，两个面板能为同一份统计显示两个数。
        assert_eq!(view.today_requests, 1, "今日请求次数与 tokens 同源");
        assert_ne!(view.today_requests, 7, "未来日的 7 次不得顶替今日的 1 次");
    }

    #[test]
    fn prune_window_drops_days_outside_retention() {
        let now = process::epoch_millis();
        let cutoff = cutoff_date_string(now);
        let old = date_string_of_ms(now - (RETENTION_DAYS + 3) * 86_400_000);
        let recent = date_string_of_ms(now - 86_400_000);
        let mut doc = UsageStateDoc::default();
        let mut entry = FileEntry::default();
        entry.days.insert(old.clone(), BTreeMap::new());
        entry.days.insert(recent.clone(), BTreeMap::new());
        doc.files.insert("f".into(), entry);
        prune_window(&mut doc, &cutoff);
        let entry = &doc.files["f"];
        assert!(!entry.days.contains_key(&old), "窗口外日账必须剪掉");
        assert!(entry.days.contains_key(&recent), "窗口内日账必须保留");
        assert_eq!(
            doc.files.len(),
            1,
            "账目条目本身保留（它是增量 checkpoint），只剪日账"
        );
    }

    /// 账目损坏时**不许**把用户挡在门外：它整份都能从 sessions 重算，正确的
    /// 处理是从零重扫并覆盖回去（`load_rebuildable` + `save_timed_light` 那一对）。
    ///
    /// 这条判据盯的是「静默失败」的反面：以前 `usage_view` 走 `load_checked`，
    /// 一个半截文件会让用量卡一直报错、要用户手删文件——而它本来没有任何不可再生的
    /// 内容。半截文件的来源是免 fsync 落盘（掉电），所以两条改动必须一起成立。
    #[test]
    fn a_corrupt_ledger_is_rebuilt_instead_of_blocking_the_panel() {
        let dir = tempfile_fresh("usage-corrupt-");
        let root = dir.join("sessions");
        let file_dir = root.join("workdir").join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        fs::write(
            file_dir.join("session.jsonl.zstd"),
            raw_frame(b"{\"type\":\"assistant/message\"}\n", true),
        )
        .unwrap();
        let state = dir.join("state.json");
        fs::write(&state, b"{\"schema\": 1, \"fil").unwrap();

        let (mut doc, reason): (UsageStateDoc, _) = crate::shell::state::load_rebuildable(&state);
        assert!(reason.is_some(), "损坏必须如实交回原因（上层拿它写壳日志）");
        assert!(doc.files.is_empty(), "从零起步，不拿半截账目当底");

        let mut perf = crate::diagnostics::perf::PerfSample::disabled();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "corrupt"),
            false,
            process::epoch_millis(),
            &mut perf,
        )
        .unwrap();
        let rebuilt: UsageStateDoc =
            crate::shell::state::load_checked(&state, usage_ctx()).unwrap();
        assert_eq!(rebuilt.files.len(), 1, "重扫后账目必须重新落盘且可读");
        assert!(rebuilt.last_scanned_at_ms > 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_op_refresh_skips_save_but_advances_in_memory_timestamp() {
        let dir = tempfile_fresh("usage-noop-save-");
        let root = dir.join("sessions");
        let ws = root.join("workdir");
        let file_dir = ws.join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("session.jsonl.zstd");
        let first = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(&file, &first).unwrap();
        let state = dir.join("state.json");

        // 首扫：建立账目，必须落盘。
        let mut perf = crate::diagnostics::perf::PerfSample::disabled();
        let mut doc = UsageStateDoc::default();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "noop-save"),
            true,
            process::epoch_millis(),
            &mut perf,
        )
        .unwrap();
        assert!(state.exists(), "首扫建账必须落盘");
        let first_ts = doc.last_scanned_at_ms;
        assert!(first_ts > 0);

        // 空转对账：删掉盘上账目再扫，若仍走 save 文件会被重建——这就是
        // 「无变化不落盘」的判据（比 mtime 稳，不依赖文件系统时间精度）。
        fs::remove_file(&state).unwrap();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "noop-save"),
            true,
            process::epoch_millis() + 60_000,
            &mut perf,
        )
        .unwrap();
        assert!(!state.exists(), "账目未变时不得重写账目文件");
        assert!(
            doc.last_scanned_at_ms > first_ts,
            "内存里的扫描时刻仍要推进（新鲜度窗口与视图上报都用它）"
        );

        // 有新帧：必须落盘（否则 offset checkpoint 丢失，下次重解全部新帧）。
        let mut grown = first.clone();
        grown.extend_from_slice(&raw_frame(b"{\"type\":\"assistant/message\"}\n", true));
        fs::write(&file, &grown).unwrap();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "noop-save"),
            true,
            process::epoch_millis() + 120_000,
            &mut perf,
        )
        .unwrap();
        assert!(state.exists(), "账目变化（offset 推进）必须落盘");
        fs::remove_dir_all(&dir).ok();
    }

    /// 落盘去抖：`force = false` 时，第二轮**既过了新鲜度闸**（间隔
    /// 120s > 90s）**又没到落盘窗口**（120s < 300s），账目变了也不写盘——
    /// 省掉的是实测 40ms 的两次 fsync。
    ///
    /// 判据直接核对真实数据：**盘上 offset 停在第一帧，内存里已到第二帧**。
    /// 这一对同时钉住了「该跳过的跳过」和「跳过不等于丢账」两件事。
    /// 反向验：把 `persist` 里的去抖条件去掉，盘上 offset 会变成第二帧，本
    /// 条必红。
    #[test]
    fn changed_within_the_save_debounce_window_skips_the_write() {
        let dir = tempfile_fresh("usage-save-debounce-");
        let root = dir.join("sessions");
        let ws = root.join("workdir");
        let file_dir = ws.join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("session.jsonl.zstd");
        let state = dir.join("state.json");
        let key = "workdir/session-x/session.jsonl.zstd";
        let frame = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(&file, &frame).unwrap();

        let mut perf = crate::diagnostics::perf::PerfSample::disabled();
        let t0 = process::epoch_millis();
        let mut doc = UsageStateDoc::default();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "debounce"),
            false,
            t0,
            &mut perf,
        )
        .unwrap();
        assert!(
            state.exists(),
            "首扫建账必须落盘（此时还没有落盘记录可去抖）"
        );

        // 追加一帧造出「账目有变」，时刻取新鲜度闸之外、落盘窗口之内。
        let mut grown = frame.clone();
        grown.extend_from_slice(&raw_frame(b"{\"type\":\"assistant/message\"}\n", true));
        fs::write(&file, &grown).unwrap();
        let t1 = t0 + FRESH_SCAN_INTERVAL_MS + 30_000;
        let mut doc2 = UsageStateDoc::default();
        refresh_doc(
            &mut doc2,
            &state,
            &root,
            ("t", "debounce"),
            false,
            t1,
            &mut perf,
        )
        .unwrap();

        let on_disk: UsageStateDoc = shell::state::load_checked(&state, usage_ctx()).unwrap();
        assert_eq!(
            on_disk.files.get(key).map(|e| e.offset),
            Some(frame.len() as u64),
            "去抖窗口内不该落盘：盘上 offset 必须仍停在第一帧之后"
        );
        assert_eq!(
            doc2.files.get(key).map(|e| e.offset),
            Some(grown.len() as u64),
            "账目本身仍要推进到最新——跳过落盘不等于丢账"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 埋点覆盖面：这一轮扫描 + 落盘**必须**记下哪些分段与规模。
    ///
    /// 这不是「顺手加的测试」——它是这套埋点唯一的防线。字段名少一个，下一次
    /// 排查就又得回到「只有总数、只能再挂一次真机基准」的状态，而那种退化在
    /// 门禁里完全看不出来（perf 行是运行期数据，静态判据扫不到）。
    /// 反向验：删掉下面列表里的任意一个字段名，本条必红。
    #[test]
    fn a_scan_records_every_phase_and_scale_it_should() {
        let dir = tempfile_fresh("usage-perf-fields-");
        let root = dir.join("sessions");
        let ws = root.join("workdir");
        let file_dir = ws.join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("session.jsonl.zstd");
        let state = dir.join("state.json");
        fs::write(
            &file,
            raw_frame(b"{\"type\":\"assistant/message\"}\n", true),
        )
        .unwrap();
        let now = process::epoch_millis();

        // 首扫：变的是磁盘上的账目 ⇒ 会落盘 ⇒ 四段 save 字段也该出现。
        let mut perf = PerfSample::for_tests();
        refresh_doc(
            &mut UsageStateDoc::default(),
            &state,
            &root,
            ("t", "perf-fields"),
            true,
            now,
            &mut perf,
        )
        .unwrap();
        let captured = perf.captured_fields();
        for field in [
            // 扫描三段 + 规模：缺任何一个，「慢在哪」就退回不可回答
            "scan",
            "scan_walk",
            "scan_reconcile",
            "scan_decode",
            "files",
            "cache_hits",
            "cache_misses",
            "cache_pruned",
            // 落盘四段：这个原语被凭据与设置共用，此前零观测
            "save",
            "save_serialize",
            "save_fsync",
            "save_rename",
            "save_dirsync",
            "items",
            "saved",
        ] {
            assert!(
                captured.contains(&field),
                "首扫的 perf 行缺字段 `{field}`；实际记到 {captured:?}"
            );
        }
        // `load` / `derive` 是 `usage_view` 层记的（读账目文档、派生汇总视图），
        // 不经 `refresh_doc`，所以不在上面那个列表里。要钉它们得整条
        // `usage_view` 跑一遍，而那会解析真实实例路径——测试不该碰用户数据。
        assert!(
            !captured.contains(&"load") && !captured.contains(&"derive"),
            "`load` / `derive` 属于 `usage_view`，不该由 `refresh_doc` 记：{captured:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 空转轮次只记扫描三段与规模，**不记 save 四段**——落盘没发生，记下来
    /// 的四段就全是 0，混进聚合会把 p50 拉偏。
    #[test]
    fn a_no_op_scan_records_no_save_phases() {
        let dir = tempfile_fresh("usage-perf-noop-");
        let root = dir.join("sessions");
        let ws = root.join("workdir");
        let file_dir = ws.join("session-x");
        fs::create_dir_all(&file_dir).unwrap();
        fs::write(
            file_dir.join("session.jsonl.zstd"),
            raw_frame(b"{\"type\":\"assistant/message\"}\n", true),
        )
        .unwrap();
        let state = dir.join("state.json");
        let now = process::epoch_millis();
        let mut doc = UsageStateDoc::default();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "perf-noop"),
            true,
            now,
            &mut PerfSample::for_tests(),
        )
        .unwrap();

        // 第二轮：账目没变 ⇒ 不落盘。
        let mut perf = PerfSample::for_tests();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "perf-noop"),
            true,
            now,
            &mut perf,
        )
        .unwrap();
        let captured = perf.captured_fields();
        assert!(captured.contains(&"scan_walk"), "空转也要记扫描三段");
        assert!(
            !captured.contains(&"save_fsync"),
            "没落盘就不该出现 save 分段，否则聚合里混着一堆 0：{captured:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_normalization_forces_one_save() {
        let dir = tempfile_fresh("usage-schema-save-");
        let root = dir.join("sessions");
        fs::create_dir_all(&root).unwrap();
        let state = dir.join("state.json");
        // 盘上是一份 schema 缺失的老账（serde default 前的形态）。
        fs::write(&state, "{}\n").unwrap();
        let mut doc: UsageStateDoc = shell::state::load_checked(&state, usage_ctx()).unwrap();
        assert_eq!(doc.schema, 0, "前置：盘上 schema 确实缺失");
        let mut perf = crate::diagnostics::perf::PerfSample::disabled();
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "schema"),
            true,
            process::epoch_millis(),
            &mut perf,
        )
        .unwrap();
        let saved: UsageStateDoc = shell::state::load_checked(&state, usage_ctx()).unwrap();
        assert_eq!(saved.schema, STATE_SCHEMA, "归一化必须随本轮落盘");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn freshness_gate_honors_in_process_timestamp() {
        // 空转不落盘后，45s 窗口内的重复调用必须靠进程内时间戳挡住，
        // 否则每轮 60s TTL 轮询都会重复扫 sessions 目录。
        let dir = tempfile_fresh("usage-gate-");
        let root = dir.join("sessions");
        fs::create_dir_all(&root).unwrap();
        let state = dir.join("state.json");
        let mut doc = UsageStateDoc::default();
        let mut perf = crate::diagnostics::perf::PerfSample::disabled();
        let t0 = process::epoch_millis();
        refresh_doc(&mut doc, &state, &root, ("t", "gate"), true, t0, &mut perf).unwrap();
        // 盘上时间戳改回很久以前：若只看盘值，610s 早已过窗口、必然重扫；
        // 进程内真值（约 t0）必须把它挡住。挡住的证据是时间戳未被刷新
        // （重扫会把它设为当前时刻）。
        let stale = t0 - 10 * 60_000;
        doc.last_scanned_at_ms = stale;
        refresh_doc(
            &mut doc,
            &state,
            &root,
            ("t", "gate"),
            false,
            t0 + 10_000,
            &mut perf,
        )
        .unwrap();
        assert_eq!(
            doc.last_scanned_at_ms, stale,
            "窗口内不得重复扫描（进程内时间戳挡住，未走扫描分支）"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// **上一个实例刚扫过，不等于当前实例新鲜**（2026-10-11 评审）。
    ///
    /// 账目按 `(family, id)` 分文件，而 `resolve_default()` 会跟随用户在顶部
    /// 页签切过的实例。进程内若只存一个时刻，切过去之后的 45s 内新实例会拿
    /// 别人的时刻判定「新鲜」而直接返回——它自己的账目文件此时是空的，用户
    /// 看到的是用量面板空白，45s 后才自愈。
    ///
    /// 旧实现在这里必红：`last_known = doc.last_scanned_at_ms.max(LAST_SCAN_MS)`
    /// 拿到的是实例 A 的 t0，`t0 + 10s` 落在 45s 窗口内 → 直接返回 →
    /// `doc_b.last_scanned_at_ms` 恒为 0。
    #[test]
    fn a_fresh_scan_on_one_instance_does_not_satisfy_another() {
        let dir = tempfile_fresh("usage-inst-");
        let t0 = process::epoch_millis();
        let mut perf = crate::diagnostics::perf::PerfSample::disabled();

        // 实例 A：建账（force）。
        let root_a = dir.join("a/sessions");
        fs::create_dir_all(&root_a).unwrap();
        let mut doc_a = UsageStateDoc::default();
        refresh_doc(
            &mut doc_a,
            &dir.join("a/state.json"),
            &root_a,
            ("t", "inst-a"),
            true,
            t0,
            &mut perf,
        )
        .unwrap();
        assert!(doc_a.last_scanned_at_ms > 0);

        // 实例 B：盘上账目是空的（从没扫过），非强制调用。
        let root_b = dir.join("b/sessions");
        fs::create_dir_all(&root_b).unwrap();
        let mut doc_b = UsageStateDoc::default();
        assert_eq!(doc_b.last_scanned_at_ms, 0, "前置：B 的账目确实是空的");
        refresh_doc(
            &mut doc_b,
            &dir.join("b/state.json"),
            &root_b,
            ("t", "inst-b"),
            false,
            t0 + 10_000,
            &mut perf,
        )
        .unwrap();
        assert!(
            doc_b.last_scanned_at_ms > 0,
            "B 必须扫出自己的扫描时刻，不能被 A 的进程内时刻挡掉"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// 手动真机验证（`cargo test --lib usage:: -- --ignored --nocapture`）：
    /// 用 `DSH_XLINK_HOME` 指向一个 home 目录，把
    /// `kernels/dsh/instances/default/home/sessions` 软链到真实 sessions，
    /// 即可对真实数据全量扫描一次（对真实数据只读，账目落在临时 home）。
    /// 输出耗时与汇总，用来核对增量机制在真实规模上的表现。
    #[test]
    #[ignore = "真机验证：需要 DSH_XLINK_HOME 指向带 sessions 软链的临时 home"]
    fn real_scale_scan_manual_benchmark() {
        let started = std::time::Instant::now();
        let view = usage_view(true, None).expect("全量扫描应成功");
        println!(
            "usage_view(force) 耗时 {:?}: files={} days={} total={} requests={} today={} top={:?}",
            started.elapsed(),
            view.tracked_files,
            view.days.len(),
            view.total_tokens,
            view.total_requests,
            view.today_tokens,
            view.top_model,
        );
        // 第二次全量扫描是纯增量：只有新追加的帧会被解。
        let started = std::time::Instant::now();
        let view = usage_view(true, None).expect("增量扫描应成功");
        println!(
            "增量 usage_view(force) 耗时 {:?}: total={}",
            started.elapsed(),
            view.total_tokens
        );
        assert_eq!(view.retention_days, RETENTION_DAYS);
        assert_eq!(view.days.len() as u64, RETENTION_DAYS);
    }

    // --- 测试工具 ------------------------------------------------------------

    fn tempfile_fresh(prefix: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "{prefix}{}-{}-{}",
            std::process::id(),
            process::epoch_millis(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn filetime_set(path: &Path, ms: u64) -> std::io::Result<()> {
        let file = fs::File::options().write(true).open(path)?;
        file.set_modified(UNIX_EPOCH + Duration::from_millis(ms))
    }
}
