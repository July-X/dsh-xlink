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

use crate::error::AppError;
use crate::instance;
use crate::paths;
use crate::process;
use crate::state::{self, StateCtx};

/// 统计窗口（天，含今天）。窗口外的日账在每次保存时剪掉。
pub const RETENTION_DAYS: u64 = 90;

/// 非强制读取时，距上次成功扫描不足该间隔就直接用现有账目回答——概览卡片
/// 随状态轮询反复拉取，不能每 2.5s 都真的扫一遍 sessions 目录。
const FRESH_SCAN_INTERVAL_MS: u64 = 45_000;

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
#[derive(Serialize, Deserialize, Clone, Default)]
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
    /// 窗口内有用量记录的天数。
    pub active_days: u64,
    pub top_model: Option<String>,
    /// 按 tokens 降序。
    pub models: Vec<ModelUsageView>,
    /// 升序，恒为 [`RETENTION_DAYS`] 个元素。
    pub days: Vec<DayUsageView>,
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

fn state_path() -> PathBuf {
    let (family, id) = instance::resolve_default();
    paths::instance_usage_state_file(family, id)
}

fn sessions_root() -> PathBuf {
    let (family, id) = instance::resolve_default();
    paths::instance_dsh_home(family, id).join("sessions")
}

/// 同一时刻只允许一次扫描：并发命令在锁上排队，后到的发现账目已新鲜就只
/// 派生视图。锁只在 `spawn_blocking` worker 内获取（与 lifecycle 锁同约定）。
static SCAN_LOCK: Mutex<()> = Mutex::new(());

/// 面板入口：需要时增量扫描，再派生最近 90 天的汇总视图。
///
/// `force = true`（详情弹窗打开 / 刷新）无视新鲜度窗口立即重扫；概览卡片用
/// `false`，命中窗口就只是把账求和，几乎零成本。
pub fn usage_view(force: bool) -> Result<UsageView, AppError> {
    let _guard = SCAN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = state_path();
    let mut doc: UsageStateDoc = state::load_checked(&path, usage_ctx())?;
    doc.schema = STATE_SCHEMA;
    doc.retention_days = RETENTION_DAYS;
    let now_ms = process::epoch_millis();
    if force || now_ms.saturating_sub(doc.last_scanned_at_ms) >= FRESH_SCAN_INTERVAL_MS {
        scan(&mut doc, now_ms);
        doc.last_scanned_at_ms = process::epoch_millis();
        state::save(&path, &doc, usage_ctx())?;
    }
    Ok(derive_view(&doc, now_ms))
}

/// 扫描 sessions 目录，把每个新追加的帧计入对应文件的账。
///
/// 三段式：先串行**规划**（枚举 + 对账决定每个文件从哪里续扫），再把真正
/// 花时间的解码**并行**打散到多个线程（首次全量要解几百 MB，并行把它从
/// 十几秒压到一两秒；常规增量只有零星几个文件有新帧），最后串行把结果
/// **合账**。账目文档只被串行段碰，工作线程只拿自己的任务副本。
fn scan(doc: &mut UsageStateDoc, now_ms: u64) {
    let root = sessions_root();
    scan_in(doc, &root, now_ms);
}

/// [`scan`] 的可测形态：sessions 根目录由调用方给出。
fn scan_in(doc: &mut UsageStateDoc, root: &Path, now_ms: u64) {
    let cutoff = cutoff_date_string(now_ms);
    let mut seen = BTreeSet::new();
    let mut items: Vec<ScanItem> = Vec::new();
    if let Ok(workspaces) = fs::read_dir(root) {
        for ws in workspaces.flatten() {
            let Ok(sessions) = fs::read_dir(ws.path()) else {
                continue;
            };
            for session in sessions.flatten() {
                let Some(file) = newest_variant(&session.path()) else {
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
                plan_item(doc, &key, &file, now_ms, &cutoff, &mut items);
            }
        }
    }
    for outcome in scan_parallel(items) {
        merge_outcome(doc, outcome);
    }
    // 会话目录被删 / 整个实例清空 → 账目随之消失，汇总自动回落。
    doc.files.retain(|key, _| seen.contains(key));
    prune_window(doc, &cutoff);
}

/// 一个待解码的文件：从 `start_offset`（`reset` 时为 0）续扫。
struct ScanItem {
    key: String,
    path: PathBuf,
    cutoff: String,
    start_offset: u64,
    reset: bool,
}

/// 一个文件的解码结果：`days` 只含本次解码的记录，合账时叠加进旧账。
struct ScanOutcome {
    key: String,
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

/// 会话目录里的当前格式：`session[.v<N>].jsonl.zstd` 中 N 最大者
/// （无 `.vN` 视为 v0）。旧代文件是迁移遗留，绝不能与新版一起计入。
fn newest_variant(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix("session") else {
            continue;
        };
        let version = if rest == ".jsonl.zstd" {
            0
        } else {
            // 不认识的文件名（session.lock、未来的其它产物）只跳过自身，
            // 不影响目录里其余候选。
            let Some(digits) = rest.strip_prefix(".v") else {
                continue;
            };
            let Some((digits, tail)) = digits.split_once('.') else {
                continue;
            };
            if tail != "jsonl.zstd"
                || digits.is_empty()
                || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                continue;
            }
            digits.parse::<u32>().unwrap_or(0)
        };
        if best.as_ref().map_or(true, |(v, _)| version > *v) {
            best = Some((version, entry.path()));
        }
    }
    best.map(|(_, path)| path)
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
struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    fn feed(&mut self, chunk: &[u8], on_line: &mut dyn FnMut(&[u8])) {
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
    process::local_date_string(UNIX_EPOCH + Duration::from_millis(ms))
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
    let today_tokens = days.last().map(|d| d.tokens).unwrap_or(0);
    UsageView {
        retention_days: RETENTION_DAYS,
        last_scanned_at_ms: doc.last_scanned_at_ms,
        tracked_files: doc.files.len(),
        total_tokens,
        total_requests: days.iter().map(|d| d.requests).sum(),
        today_tokens,
        active_days: days.iter().filter(|d| d.requests > 0).count() as u64,
        top_model: models.first().map(|m| m.key.clone()),
        models,
        days,
    }
}

/// `get_model_usage`：面板读取模型用量。
///
/// 重活在 `spawn_blocking` 里做：首次全量扫描要解全部 session 文件
/// （数百 MB 级），绝不能占 Tauri 主线程。
#[tauri::command]
pub async fn get_model_usage(force: Option<bool>) -> Result<UsageView, String> {
    tauri::async_runtime::spawn_blocking(move || usage_view(force.unwrap_or(false)))
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
/// 程就上移夹在屏内，详见 `crate::window::dock_x` / `dock_y`）。构造过程
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
            let backdrop = crate::commands::chrome_backdrop(&handle);
            // 吸附定位取主窗的物理坐标 + 缩放比换算成逻辑坐标交给 builder；
            // 主窗不在（理论上不会）或取不到显示器信息时保持默认居中。
            let dock = handle.get_webview_window("main").and_then(|main| {
                crate::window::dock_position_logical(&main, crate::window::USAGE_VIEWER_SIZE)
            });
            let mut builder = tauri::WebviewWindowBuilder::new(
                &handle,
                "usage-viewer",
                tauri::WebviewUrl::App("index.html?usage=1".into()),
            )
            .title("模型用量")
            .inner_size(
                crate::window::USAGE_VIEWER_SIZE.width,
                crate::window::USAGE_VIEWER_SIZE.height,
            )
            .min_inner_size(720.0, 520.0)
            .resizable(true)
            .background_color(backdrop);
            if let Some((x, y)) = dock {
                builder = builder.position(x, y);
            }
            let result = builder
                .build()
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
    fn newest_variant_prefers_highest_version_and_ignores_locks() {
        let dir = tempfile_fresh("usage-variant-");
        fs::write(dir.join("session.jsonl.zstd"), b"v0").unwrap();
        fs::write(dir.join("session.v2.jsonl.zstd"), b"v2").unwrap();
        fs::write(dir.join("session.v3.jsonl.zstd"), b"v3").unwrap();
        fs::write(dir.join("session.lock"), b"lock").unwrap();
        let chosen = newest_variant(&dir).expect("应找到 session 文件");
        assert_eq!(
            chosen.file_name().unwrap().to_string_lossy(),
            "session.v3.jsonl.zstd"
        );

        let plain = tempfile_fresh("usage-variant-plain-");
        fs::write(plain.join("session.jsonl.zstd"), b"v0").unwrap();
        assert_eq!(
            newest_variant(&plain)
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            "session.jsonl.zstd"
        );
        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&plain).ok();
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
        scan_in(&mut doc, &dir, now);
        let key = "workdir/session-x/session.jsonl.zstd";
        assert_eq!(doc.files.get(key).unwrap().offset, first.len() as u64);

        // 追加一帧：只消费新帧（offset 推进到新的文件长度）。
        let mut grown = first.clone();
        grown.extend_from_slice(&raw_frame(b"{\"type\":\"assistant/message\"}\n", true));
        fs::write(&file, &grown).unwrap();
        scan_in(&mut doc, &dir, now);
        assert_eq!(doc.files.get(key).unwrap().offset, grown.len() as u64);

        // 截断重写：账目作废，offset 归零后重扫到新长度。
        let shrunk = raw_frame(b"{\"type\":\"assistant/message\"}\n", true);
        fs::write(&file, &shrunk).unwrap();
        scan_in(&mut doc, &dir, now);
        let entry = doc.files.get(key).unwrap();
        assert_eq!(
            entry.offset,
            shrunk.len() as u64,
            "截断后应重扫并推进到新末尾"
        );

        // 会话目录整体消失：账目随之移除。
        fs::remove_file(&file).unwrap();
        scan_in(&mut doc, &dir, now);
        assert!(!doc.files.contains_key(key), "文件消失后账目必须移除");
        fs::remove_dir_all(&dir).ok();
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
        scan_in(&mut doc, &dir, now);
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

    /// 手动真机验证（`cargo test --lib usage:: -- --ignored --nocapture`）：
    /// 用 `DSH_XLINK_HOME` 指向一个 home 目录，把
    /// `kernels/dsh/instances/default/home/sessions` 软链到真实 sessions，
    /// 即可对真实数据全量扫描一次（对真实数据只读，账目落在临时 home）。
    /// 输出耗时与汇总，用来核对增量机制在真实规模上的表现。
    #[test]
    #[ignore = "真机验证：需要 DSH_XLINK_HOME 指向带 sessions 软链的临时 home"]
    fn real_scale_scan_manual_benchmark() {
        let started = std::time::Instant::now();
        let view = usage_view(true).expect("全量扫描应成功");
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
        let view = usage_view(true).expect("增量扫描应成功");
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
