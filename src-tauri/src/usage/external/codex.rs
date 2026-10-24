//! Codex（CLI / Codex Desktop）的会话 token 用量。
//!
//! ## 数据形态
//!
//! 会话文件在 `$CODEX_HOME/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl`
//! （归档的在 `$CODEX_HOME/archived_sessions/*.jsonl`，**平铺**、与
//! `sessions/` 零重叠）。每行一个 JSON，顶层 `type` 有五种与用量有关的形状：
//!
//! ### 两种形态：`.jsonl` 与 `.jsonl.zst`
//!
//! Codex 会把**7 天没动过**的会话压成同名的 `.jsonl.zst`（单帧 zstd、保留
//! mtime）并删掉原文件；所以**最近的会话是普通 `.jsonl`**。
//!
//! **只收 `.zst` 会把最近一周的账整段丢掉**（2026-10-24 实测本机：208 个
//! 会话里 201 个是 `.zst`、7 个是普通 `.jsonl`；那 7 个装着 42,401,192
//! tokens，且**全部落在最近 11 天**——「今日」「近 7 天」这两个最常看的范围
//! 会恒为 0）。**两种形态都收。**
//!
//! 同一逻辑会话的两种形态**可能同时存在**（Codex 继续一段压缩过的会话时，
//! 先解压回 `.jsonl` 再追加）：那一刻只读**普通形态**——它是 Codex 正在写的
//! 那份，内容不少于压缩那份；两份都读等于把同一段用量数两遍。与 cc-switch
//! 的 `dedupe_siblings` 是同一条规则。
//!
//! - `event_msg` + `payload.type == "token_count"`：老 CLI 的写法。
//!   `payload.info.total_token_usage` 是**累计**，`payload.info.last_token_usage`
//!   是**本次增量**。`payload.info` 可能是 `null`（2026-04 的老版本），要容错。
//! - `token_usage_record`（顶层类型，2026-09 起的新 CLI）：
//!   `payload.usage` 是本次增量，`payload.turn_token_usage` /
//!   `payload.thread_token_usage` 是累计。与上面那条**逐对相等**（同一次请求
//!   的两份记录），所以**两个形状只取一个**，取两个就是重复计数。
//!   （2026-10-24 实测本机 4,069 对逐对相等、0 对不等：cc-switch 那条
//!   「`token_count` 的 `last` 为 0 时用 `token_usage_record` 补记」的规则在
//!   这批数据上**不会改变总量**——那 4 条 `last` 为 0 的行没有配对的 record。
//!   所以本仓沿用「两形状只取一个」，不引入补记逻辑。）
//! - `turn_context`：`payload.model` 是这一轮用的模型。
//! - `session_meta`：`payload.model_provider` 是厂商名（`custom` / `crs` /
//!   `OpenAI`）。
//!
//! ## 三条口径（踩过的坑）
//!
//! 1. **「重发」必须去重**：38.4% 的 `token_count` 行是把**同一个状态**再 emit
//!    一遍（compaction / 恢复 / 事件重放），累计值与上一行完全相同。直接累加
//!    全部行会虚高 53.8%。判据是「本行的 `total_token_usage` 与上一行相同」。
//! 2. **总量取来源自己报的 `total_tokens`，不拿分量重算**：`crs` provider 的
//!    `total_tokens` 既不等于 `input + output`，也不等于任何分量组合。
//! 3. **`cached_input_tokens` 是 `input_tokens` 的子集**（69,151 条记录
//!    100% 成立），`reasoning_output_tokens` 是 `output_tokens` 的子集。
//!    照 OpenAI 某些 API 的 `cached_tokens` 惯例再加一遍就双计了。
//!
//! ## 增量策略
//!
//! Codex 的会话文件是**每次保存整体重写**的（单帧 zstd，压缩字节每次全变），
//! 压缩层做不了增量——能用的只有「文件变了就整个重扫」。于是缓存里存
//! **大小 + mtime**：两者都与记录的一致就复用，不一致就重扫该文件并**整条
//! 替换**它的账（不做加法，所以重扫不会把同一段用量数两遍）。
//!
//! **读失败的文件不写空账**：读不出来（文件被删 / 权限不对 / 不是合法的
//! zstd）时把它标成 `stale`——**保留上一次读到的账**（别让数字凭空少一块），
//! 但**下一轮一定会重扫**它。「大小与 mtime 没变就跳过」正是「读失败一次就
//! 永久归零」的成因（2026-10-24 评审发现）。
//!
//! **mtime 早于窗口起点的文件整个跳过**：Codex 的行内 `timestamp` 是**写入
//! 时刻**，而 mtime 是最后写入时刻，实测 201/201 个文件满足
//! `max(行内 timestamp) <= mtime`，所以窗口前的文件不可能含窗口内的记录。
//!
//! ## 已知缺口（如实记）
//!
//! - **fork 文件会重复计入一次请求**：fork 出来的 rollout 文件会把父线程的
//!   累计快照带过来，那一次的增量在父文件里已经数过。实测 6 个 fork 文件，
//!   合计约 37 万 token，占总量 0.007%——低于噪声，没有为它引入「按文件名
//!   猜 fork」的脆弱判据。
//! - **累计值会倒退**（会话被新进程接管时计数器重置），所以**不能**取文件
//!   末行的累计当会话总量；本模块只认增量。

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use memchr::memmem::Finder;
use ruzstd::decoding::StreamingDecoder;
use serde_json::Value;

use super::{derive_view, ExternalFileEntry, ExternalModelDay, ExternalToolUsage};
use crate::usage::local::LineSplitter;

/// 来源 id（缓存文档里的键）。
pub const SOURCE_ID: &str = "codex";

/// 界面名。
const LABEL: &str = "Codex";

const TOKEN_COUNT_NEEDLE: &str = "\"token_count\"";
const TOKEN_RECORD_NEEDLE: &str = "\"token_usage_record\"";
const TURN_CONTEXT_NEEDLE: &str = "\"turn_context\"";
const SESSION_META_NEEDLE: &str = "\"session_meta\"";

/// 四个子串查找器**只建一次**。
///
/// `memmem::Finder::new` 要按 needle 建预过滤表——每行新建一个的话，光建表
/// 的开销就超过扫描本身了。
static TOKEN_COUNT_FINDER: LazyLock<Finder<'static>> =
    LazyLock::new(|| Finder::new(TOKEN_COUNT_NEEDLE));
static TOKEN_RECORD_FINDER: LazyLock<Finder<'static>> =
    LazyLock::new(|| Finder::new(TOKEN_RECORD_NEEDLE));
static TURN_CONTEXT_FINDER: LazyLock<Finder<'static>> =
    LazyLock::new(|| Finder::new(TURN_CONTEXT_NEEDLE));
static SESSION_META_FINDER: LazyLock<Finder<'static>> =
    LazyLock::new(|| Finder::new(SESSION_META_NEEDLE));

/// 单个文件的累计快照（判「重发」用）。六个分量全等才算同一个状态。
type UsageSnapshot = (u64, u64, u64, u64, u64, u64);

/// 一个待扫描的文件。
#[derive(Clone)]
struct ScanItem {
    /// 相对 Codex 根目录的路径（缓存的键）。
    relative: String,
    path: PathBuf,
    size: u64,
    mtime_ms: u64,
}

/// 扫描一个文件的结果。
struct ScanOutcome {
    relative: String,
    size: u64,
    mtime_ms: u64,
    /// 读到的账；`None` = **没读全**（读失败 / 解压中途坏帧）。
    ///
    /// 读失败的文件**不写空账**：写进去就等于把这个文件的用量永久抹掉
    /// （下一轮「大小与 mtime 没变就跳过」会直接复用那份空账）。
    days: Option<BTreeMap<String, BTreeMap<String, ExternalModelDay>>>,
}

/// 本轮扫描里见过的 `response_id`（**跨文件**共用）。
///
/// `response_id` 全局唯一：同一个请求被写进多个文件时（fork 出来的会话会把
/// 父线程的记录带过来），只算一次。此前的实现是**每个文件一份**，于是代码里
/// 那句「同一请求可能被写进多个文件」的注释并没有被兑现（2026-10-24 评审发现）。
type SharedResponses = Arc<Mutex<BTreeSet<String>>>;

/// 扫 Codex 的会话文件，产出用量视图。
pub(crate) fn scan(cutoff: &str, today: &str, now_ms: u64) -> ExternalToolUsage {
    let Some(root) = super::codex_root() else {
        return note("未找到 Codex 的本机数据目录（CODEX_HOME 或 ~/.codex）");
    };
    if !root.is_dir() {
        return note(&format!("未找到 Codex 的本机数据目录：{}", root.display()));
    }
    let previous = super::load_codex_state();
    let cached = if previous.root == root.to_string_lossy() {
        previous.files
    } else {
        // 根目录变了（换机器 / 换 CODEX_HOME）→ 旧账全部作废。
        BTreeMap::new()
    };
    let files_state = scan_root(&root, cutoff, today, now_ms, &cached);
    super::save_codex_state(&super::ExternalSourceState {
        root: root.to_string_lossy().into_owned(),
        files: files_state.clone(),
    });
    build_view(&root, &files_state, cutoff, today)
}

/// 扫一个 Codex 根目录，返回该根下每个文件的账。
///
/// 与 [`scan`] 分开是为了**可测**：根目录由调用方给出，测试就能在临时目录里
/// 造出会话文件，不碰用户真实数据（`CODEX_HOME` 是环境变量，测试里改它要
/// 抢全局 env 锁，并行用例会互相踩）。
///
/// `cached` 是上一轮的账：大小与 mtime 都对得上的文件直接复用，不重扫。
fn scan_root(
    root: &Path,
    cutoff: &str,
    _today: &str,
    now_ms: u64,
    cached: &BTreeMap<String, ExternalFileEntry>,
) -> BTreeMap<String, ExternalFileEntry> {
    let cutoff_ms = now_ms.saturating_sub((super::RETENTION_DAYS - 1) * 86_400_000);
    let files = collect_files(root, cutoff_ms);
    let mut items: Vec<ScanItem> = Vec::new();
    let mut reused: BTreeMap<String, ExternalFileEntry> = BTreeMap::new();
    for (relative, path, size, mtime_ms) in files {
        match cached.get(&relative) {
            // **stale 的条目不复用**：它是上一次没读全的，必须重扫。
            Some(entry) if !entry.stale && entry.size == size && entry.mtime_ms == mtime_ms => {
                reused.insert(relative, entry.clone());
            }
            _ => items.push(ScanItem {
                relative,
                path,
                size,
                mtime_ms,
            }),
        }
    }

    let outcomes = scan_parallel(items);
    let mut files_state = reused;
    for outcome in outcomes {
        let Some(days) = outcome.days else {
            // 没读全：**保留上一轮的账**（别让数字凭空少一块），并标成
            // `stale` —— 下一轮一定会重扫它（2026-10-24 评审发现：读失败一次
            // 就永久归零，是因为空账被当成「已读过」缓存了下来）。
            let previous = cached
                .get(&outcome.relative)
                .map(|entry| entry.days.clone())
                .unwrap_or_default();
            files_state.insert(
                outcome.relative,
                ExternalFileEntry {
                    size: outcome.size,
                    mtime_ms: outcome.mtime_ms,
                    days: previous,
                    stale: true,
                },
            );
            continue;
        };
        files_state.insert(
            outcome.relative,
            ExternalFileEntry {
                size: outcome.size,
                mtime_ms: outcome.mtime_ms,
                days,
                stale: false,
            },
        );
    }
    // 窗口外的文件整条移除：它们不会再有贡献，留着只是让缓存无界增长。
    files_state.retain(|_, entry| entry.mtime_ms >= cutoff_ms);
    // 窗口外的**日期**也剪掉：一个上周才结束的会话文件里可能留着更早的
    // 记录，它们永远不会再进窗口，留着只是让缓存一天天胖。
    for entry in files_state.values_mut() {
        entry.days.retain(|day, _| day.as_str() >= cutoff);
    }
    files_state
}

/// 把「每个文件的账」求和成视图。
fn build_view(
    root: &Path,
    files_state: &BTreeMap<String, ExternalFileEntry>,
    cutoff: &str,
    today: &str,
) -> ExternalToolUsage {
    let mut days: BTreeMap<String, BTreeMap<String, ExternalModelDay>> = BTreeMap::new();
    for entry in files_state.values() {
        for (day, per_model) in &entry.days {
            let bucket = days.entry(day.clone()).or_default();
            for (key, model_day) in per_model {
                let slot = bucket.entry(key.clone()).or_default();
                slot.requests += model_day.requests;
                slot.tokens += model_day.tokens;
                slot.input += model_day.input;
                slot.output += model_day.output;
                slot.cache_read += model_day.cache_read;
                slot.cache_write += model_day.cache_write;
            }
        }
    }
    let mut view = derive_view(
        LABEL,
        SOURCE_ID,
        &root.to_string_lossy(),
        files_state.len(),
        &days,
        cutoff,
        today,
    );
    if view.tokens == 0 && view.requests == 0 {
        view.note = Some(
            "近 90 天内没有读到 Codex 的会话用量记录（会话文件可能还没落盘，或统计窗口内没有对话）"
                .to_string(),
        );
    }
    view
}

/// 该来源不可用时的占位视图。
fn note(reason: &str) -> ExternalToolUsage {
    ExternalToolUsage {
        source: SOURCE_ID.to_string(),
        label: LABEL.to_string(),
        note: Some(reason.to_string()),
        ..Default::default()
    }
}

/// 枚举 Codex 的会话文件：相对路径 + 完整路径 + 大小 + mtime。
///
/// **两种形态都收**（`.jsonl` 与 `.jsonl.zst`）：最近的会话还没被压缩，只收
/// `.zst` 会把最近一周的账整段丢掉（见模块文档的实测）。
///
/// mtime 早于窗口起点的整个跳过（见模块文档的判据）。
fn collect_files(root: &Path, cutoff_ms: u64) -> Vec<(String, PathBuf, u64, u64)> {
    let mut out = Vec::new();
    // `sessions/` 的会话文件在 `sessions/<YYYY>/<MM>/<DD>/` 下（**三层**子
    // 目录），`archived_sessions/` 是**平铺**的——两棵树的深度不同，枚举时
    // 各给各的层数。
    for (relative_dir, depth) in [("sessions", 4), ("archived_sessions", 1)] {
        let dir = root.join(relative_dir);
        walk(&dir, depth, &mut |path| {
            if !is_rollout_file(path) {
                return;
            }
            let Ok(meta) = fs::metadata(path) else {
                return;
            };
            let Ok(mtime) = meta.modified() else {
                return;
            };
            let mtime_ms = match mtime.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_millis() as u64,
                Err(_) => return,
            };
            if mtime_ms < cutoff_ms {
                return;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                return;
            };
            out.push((
                relative.to_string_lossy().replace('\\', "/"),
                path.to_path_buf(),
                meta.len(),
                mtime_ms,
            ));
        });
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    // 同一逻辑会话的两种形态同时存在时**只留普通形态**（Codex 正在写的那份
    // 内容更多）：两份都读等于把同一段用量数两遍。
    dedupe_siblings(&mut out);
    out
}

/// 是不是 Codex 的会话文件（两种形态都算）。
fn is_rollout_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")
}

/// 是不是压缩形态（`.jsonl.zst`）。
fn is_compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

/// 逻辑名：去掉 `.zst` 后缀（两种形态指同一个会话）。
fn logical_name(relative: &str) -> &str {
    relative.strip_suffix(".zst").unwrap_or(relative)
}

/// 同一逻辑会话的两种形态同时存在时**只留普通形态**。
///
/// Codex 继续一段压缩过的会话时会先解压回 `.jsonl` 再追加——那一刻两份都在
/// 盘上，普通那份是 Codex 正在写的、内容不少于压缩那份。留压缩那份会漏掉
/// 追加的那几行，留两份则数两遍。留普通那份（与 cc-switch 的
/// `dedupe_siblings` 同一条规则）。
fn dedupe_siblings(files: &mut Vec<(String, PathBuf, u64, u64)>) {
    let plain: BTreeSet<String> = files
        .iter()
        .filter(|(_, path, _, _)| !is_compressed(path))
        .map(|(relative, _, _, _)| logical_name(relative).to_string())
        .collect();
    files.retain(|(relative, path, _, _)| {
        !is_compressed(path) || !plain.contains(logical_name(relative))
    });
}

/// 递归收集目录下的文件（深度上限防跑飞）。
fn walk(dir: &Path, depth: usize, on_file: &mut dyn FnMut(&Path)) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, depth - 1, on_file);
        } else if file_type.is_file() {
            on_file(&path);
        }
    }
}

/// 并行扫描：首次全量要解几百 MB，按文件打散到多个线程。
fn scan_parallel(items: Vec<ScanItem>) -> Vec<ScanOutcome> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8);
    // `response_id` 去重**跨文件**共用一份（见 [`SharedResponses`]）。
    let shared: SharedResponses = Arc::new(Mutex::new(BTreeSet::new()));
    if workers <= 1 || items.len() <= 1 {
        return items
            .into_iter()
            .map(|item| scan_item(item, &shared))
            .collect();
    }
    let chunk_size = items.len().div_ceil(workers);
    let mut outcomes = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk_size)
            .map(|chunk| {
                let chunk = chunk.to_vec();
                let shared = Arc::clone(&shared);
                scope.spawn(move || {
                    chunk
                        .into_iter()
                        .map(|item| scan_item(item, &shared))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            outcomes.extend(handle.join().unwrap_or_default());
        }
    });
    outcomes
}

/// 扫一个文件，解压后逐行入账。
fn scan_item(item: ScanItem, shared: &SharedResponses) -> ScanOutcome {
    let days = scan_file(&item.path, shared);
    ScanOutcome {
        relative: item.relative,
        size: item.size,
        mtime_ms: item.mtime_ms,
        days,
    }
}

/// 解压一个文件并把它的用量入账。
///
/// **读不出来就返回 `None`**（文件被删 / 权限不对 / 不是合法的 zstd / 解压
/// 中途坏帧）——**不**把空账写进缓存，那样会把这个文件的用量永久抹掉
/// （下一轮「大小与 mtime 没变就跳过」会直接复用那份空账）。坏帧时已解出来
/// 的那部分**也不算数**：它是半份账，当成完整的记下去等于少记却看不出来。
///
/// 用 [`StreamingDecoder`] 而不是 `FrameDecoder`：前者实现 `io::Read`，
/// 多帧文件与「读到最后一块」这两种边界都自己处理。后者要自己管帧边界，
/// 而 Codex 的 `.zst` 是**单帧**——`decode_blocks` 在读完最后一块后会去读
/// 下一帧的头并撞上 EOF，于是「已解出的内容」与「错误」同时发生，自己管
/// 边界就一定会漏掉其中一半。
///
/// 普通 `.jsonl` 走同样的行切分，只是没有解压层。
fn scan_file(
    path: &Path,
    shared: &SharedResponses,
) -> Option<BTreeMap<String, BTreeMap<String, ExternalModelDay>>> {
    let file = File::open(path).ok()?;
    let mut splitter = LineSplitter::default();
    let mut state = FileState::new(Arc::clone(shared));
    let mut buf = [0u8; 64 * 1024];
    if is_compressed(path) {
        let reader = BufReader::with_capacity(256 * 1024, file);
        let mut decoder = StreamingDecoder::new(reader).ok()?;
        loop {
            match decoder.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => splitter.feed(&buf[..n], &mut |line| state.consume(line)),
                // 坏帧：这份账没读全 → 整个文件按「没读全」处理。
                Err(_) => return None,
            }
        }
    } else {
        let mut reader = BufReader::with_capacity(256 * 1024, file);
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => splitter.feed(&buf[..n], &mut |line| state.consume(line)),
                Err(_) => return None,
            }
        }
    }
    // 文件末尾那一行没有换行，不 flush 就丢了（每个会话少算最后一次请求）。
    splitter.flush(&mut |line| state.consume(line));
    Some(state.finish())
}

/// 把一次调用记进「天 × 模型」。
///
/// **总量取来源自己报的 `total_tokens`**，不拿分量重算（`crs` provider 的两者
/// 对不上）。`cached` 与 `reasoning` 是 `input` / `output` 的**子集**，单独
/// 记着供展示，**不参与总量**。
fn record(
    days: &mut BTreeMap<String, BTreeMap<String, ExternalModelDay>>,
    provider: &str,
    model: &str,
    value: &Value,
    usage: UsageSnapshot,
) {
    let (input, output, cached, cache_write, _reasoning, total) = usage;
    if total == 0 && input == 0 && output == 0 {
        return;
    }
    let Some(day) = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(super::local_day_from_iso)
    else {
        return;
    };
    let key = format!("{provider}/{model}");
    let slot = days.entry(day).or_default().entry(key).or_default();
    slot.requests += 1;
    slot.tokens += total;
    slot.input += input;
    slot.output += output;
    slot.cache_read += cached;
    slot.cache_write += cache_write;
}

/// 一个文件扫描过程中的可变状态。
#[derive(Default)]
struct FileState {
    /// 厂商名（`session_meta.payload.model_provider`）。
    provider: Option<String>,
    /// 当前生效的模型（`turn_context.payload.model`）。
    model: Option<String>,
    /// 上一条 `token_count` 的累计快照（判「重发」用）。
    previous_cum: Option<UsageSnapshot>,
    /// 见过可用的 `token_count` 行吗？见过就只用那一种形状（两个形状对同一
    /// 次请求记的是同一份用量，取两个就是重复计数）。
    saw_token_count: bool,
    /// `token_count` 形状入的账。
    count_days: BTreeMap<String, BTreeMap<String, ExternalModelDay>>,
    /// `token_usage_record` 形状入的账（只在文件里没有 `token_count` 时才用）。
    record_days: BTreeMap<String, BTreeMap<String, ExternalModelDay>>,
    /// 已见过的 `response_id`（**跨文件**共用，见 [`SharedResponses`]）。
    shared_responses: SharedResponses,
}

impl FileState {
    fn new(shared: SharedResponses) -> Self {
        Self {
            shared_responses: shared,
            ..Default::default()
        }
    }
}

/// 一次模型调用在某个来源里报出来的六个分量。
fn usage_snapshot(value: Option<&Value>) -> Option<UsageSnapshot> {
    let value = value?;
    Some((
        num(value, "input_tokens"),
        num(value, "output_tokens"),
        num(value, "cached_input_tokens"),
        num(value, "cache_write_input_tokens"),
        num(value, "reasoning_output_tokens"),
        num(value, "total_tokens"),
    ))
}

/// 取一个 JSON 对象里的整数（缺失或不是数就当 0）。
fn num(value: &Value, field: &str) -> u64 {
    match value.get(field) {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0).round() as u64))
            .unwrap_or(0),
        _ => 0,
    }
}

impl FileState {
    /// 处理一行。
    fn consume(&mut self, line: &[u8]) {
        // 先用便宜的子串预筛：只有 4.27% 的解压字节落在带计量的行上，
        // 剩下的（工具输出、消息正文）整行跳过，连 JSON 解析都不做。
        let is_count = TOKEN_COUNT_FINDER.find(line).is_some();
        let is_record = !is_count && TOKEN_RECORD_FINDER.find(line).is_some();
        if !is_count && !is_record {
            if TURN_CONTEXT_FINDER.find(line).is_some() {
                self.consume_turn_context(line);
            } else if SESSION_META_FINDER.find(line).is_some() {
                self.consume_session_meta(line);
            }
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if is_record {
            self.consume_token_record(&value);
        } else {
            self.consume_token_count(&value);
        }
    }

    /// `session_meta`：取厂商名。
    fn consume_session_meta(&mut self, line: &[u8]) {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            return;
        }
        if let Some(provider) = value
            .get("payload")
            .and_then(|p| p.get("model_provider"))
            .and_then(Value::as_str)
        {
            if self.provider.is_none() && !provider.is_empty() {
                self.provider = Some(provider.to_string());
            }
        }
    }

    /// `turn_context`：取这一轮的模型。
    fn consume_turn_context(&mut self, line: &[u8]) {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if value.get("type").and_then(Value::as_str) != Some("turn_context") {
            return;
        }
        if let Some(model) = value
            .get("payload")
            .and_then(|p| p.get("model"))
            .and_then(Value::as_str)
        {
            if !model.is_empty() {
                self.model = Some(model.to_string());
            }
        }
    }

    /// `event_msg` + `token_count`：老 CLI 的形状。
    fn consume_token_count(&mut self, value: &Value) {
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            return;
        }
        let Some(payload) = value.get("payload") else {
            return;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            return;
        }
        // `info` 可能是 null（2026-04 的老版本）——那一行没有任何计量。
        let Some(info) = payload.get("info").filter(|v| v.is_object()) else {
            return;
        };
        let Some(cum) = usage_snapshot(info.get("total_token_usage")) else {
            return;
        };
        let Some(last) = usage_snapshot(info.get("last_token_usage")) else {
            return;
        };
        self.saw_token_count = true;
        // 「重发」：本行的累计与上一行完全相同 ⇒ 同一个状态被再 emit 了一遍，
        // 不入账（直接累加会虚高 53.8%）。
        if self.previous_cum == Some(cum) {
            return;
        }
        self.previous_cum = Some(cum);
        let (provider, model) = self.model_key();
        record(&mut self.count_days, &provider, &model, value, last);
    }

    /// `token_usage_record`：新 CLI 的形状（本次增量在 `payload.usage`）。
    fn consume_token_record(&mut self, value: &Value) {
        if value.get("type").and_then(Value::as_str) != Some("token_usage_record") {
            return;
        }
        let Some(payload) = value.get("payload") else {
            return;
        };
        // `response_id` 全局唯一，用它去重（同一请求可能被写进多个文件——
        // fork 出来的会话会把父线程的记录带过来）。去重集合**跨文件共用**。
        if let Some(id) = payload.get("response_id").and_then(Value::as_str) {
            let mut seen = self
                .shared_responses
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !seen.insert(id.to_string()) {
                return;
            }
        }
        let Some(usage) = usage_snapshot(payload.get("usage")) else {
            return;
        };
        let (provider, model) = self.model_key();
        record(&mut self.record_days, &provider, &model, value, usage);
    }

    /// 当前生效的 `provider/model`。
    fn model_key(&self) -> (String, String) {
        (
            self.provider
                .clone()
                .unwrap_or_else(|| SOURCE_ID.to_string()),
            self.model.clone().unwrap_or_else(|| "unknown".to_string()),
        )
    }

    /// 这个文件的最终账。
    fn finish(self) -> BTreeMap<String, BTreeMap<String, ExternalModelDay>> {
        if self.saw_token_count {
            self.count_days
        } else {
            self.record_days
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruzstd::encoding::{CompressionLevel, FrameCompressor};

    /// 造一个临时目录（用完删掉）。**不碰用户真实数据目录**。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dsh-xlink-usage-external-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("临时目录应可创建");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 写一个 `.jsonl.zst` 会话文件。
    fn write_rollout(path: &Path, lines: &[&str]) {
        let body = lines.join("\n");
        let mut compressor = FrameCompressor::new(CompressionLevel::Uncompressed);
        compressor.set_source(body.as_bytes());
        compressor.set_drain(File::create(path).expect("会话文件应可写"));
        compressor.compress();
    }

    /// 写一个**普通**（未压缩）`.jsonl` 会话文件。
    ///
    /// 最近的会话就是普通形态（Codex 只把 7 天没动过的压成 `.zst`），
    /// 只收 `.zst` 会把最近一周的账整段丢掉。
    fn write_plain(path: &Path, lines: &[&str]) {
        fs::write(path, lines.join("\n")).expect("会话文件应可写");
    }

    /// 一条 `token_count` 行。
    ///
    /// 用 `serde_json` 造而不是拼字符串——手拼的转义括号少一个就编不过，
    /// 而且改字段时看不出改的是哪一层。
    pub(super) fn token_count(ts: &str, cum_in: u64, last_in: u64, last_out: u64) -> String {
        let usage = |input: u64, output: u64| {
            serde_json::json!({
                "input_tokens": input,
                "output_tokens": output,
                "cached_input_tokens": 0,
                "cache_write_input_tokens": 0,
                "reasoning_output_tokens": 0,
                "total_tokens": input + output,
            })
        };
        serde_json::json!({
            "timestamp": ts,
            "ordinal": 1,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": usage(cum_in, 0),
                    "last_token_usage": usage(last_in, last_out),
                }
            }
        })
        .to_string()
    }

    const META: &str = r#"{"timestamp":"2026-10-24T02:00:00.000Z","ordinal":0,"type":"session_meta","payload":{"id":"t1","model_provider":"custom"}}"#;
    const TURN: &str = r#"{"timestamp":"2026-10-24T02:00:00.000Z","ordinal":1,"type":"turn_context","payload":{"model":"gpt-test"}}"#;

    /// 今天的 epoch 毫秒（测试里当「现在」）。
    fn now_ms() -> u64 {
        crate::shell::process::epoch_millis()
    }

    #[test]
    fn reemitted_lines_are_not_counted_twice() {
        let dir = TempDir::new("codex-reemit");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        write_rollout(
            &sessions.join("rollout-a.jsonl.zst"),
            &[
                META,
                TURN,
                // 第一次请求：累计 100，增量 100。
                &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
                // **重发**：同一个状态再 emit 了一遍 —— 必须跳过。
                &token_count("2026-10-24T02:00:02.000Z", 100, 100, 10),
                // 第二次请求：累计 150，增量 50。
                &token_count("2026-10-24T02:00:03.000Z", 150, 50, 5),
            ],
        );
        let files = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        assert_eq!(files.len(), 1);
        let entry = files.values().next().unwrap();
        let day = entry.days.keys().next().expect("应有一天的账");
        let model = entry.days[day].values().next().expect("应有一个模型的账");
        assert_eq!(model.requests, 2, "重发行不算一次请求");
        // 第一次请求 100 输入 + 10 输出，第二次 50 输入 + 5 输出。
        assert_eq!(model.tokens, 165, "总量取来源自己报的 total_tokens");
        assert_eq!(model.input, 150);
        assert_eq!(model.output, 15);
    }

    #[test]
    fn model_and_provider_come_from_the_file_itself() {
        let dir = TempDir::new("codex-model");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        write_rollout(
            &sessions.join("rollout-a.jsonl.zst"),
            &[
                META,
                TURN,
                &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
            ],
        );
        let files = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        let entry = files.values().next().unwrap();
        let day = entry.days.keys().next().unwrap();
        let keys: Vec<&String> = entry.days[day].keys().collect();
        assert_eq!(keys, vec!["custom/gpt-test"], "键形如 provider/model");
    }

    #[test]
    fn usage_outside_the_window_is_dropped() {
        let dir = TempDir::new("codex-window");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        write_rollout(
            &sessions.join("rollout-a.jsonl.zst"),
            &[
                META,
                TURN,
                &token_count("2026-01-01T00:00:01.000Z", 100, 100, 10),
            ],
        );
        let files = scan_root(
            dir.path(),
            "2026-10-01",
            "2026-10-31",
            now_ms(),
            &BTreeMap::new(),
        );
        let entry = files.values().next().unwrap();
        assert!(
            entry.days.is_empty(),
            "窗口外的日期不进账：{:?}",
            entry.days.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn unchanged_files_reuse_the_cached_ledger() {
        let dir = TempDir::new("codex-cache");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        write_rollout(
            &sessions.join("rollout-a.jsonl.zst"),
            &[
                META,
                TURN,
                &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
            ],
        );
        let first = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        // 第二轮用上一轮的账当缓存：结果必须一致（复用而不是重扫）。
        let second = scan_root(dir.path(), "2026-01-01", "2026-12-31", now_ms(), &first);
        assert_eq!(
            first.keys().collect::<Vec<_>>(),
            second.keys().collect::<Vec<_>>()
        );
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
    }

    #[test]
    fn archived_sessions_are_scanned_too() {
        let dir = TempDir::new("codex-archived");
        let archived = dir.path().join("archived_sessions");
        fs::create_dir_all(&archived).unwrap();
        write_rollout(
            &archived.join("rollout-old.jsonl.zst"),
            &[
                META,
                TURN,
                &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
            ],
        );
        let files = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        assert_eq!(files.len(), 1, "归档目录里的会话也要算");
        assert!(files
            .keys()
            .next()
            .unwrap()
            .starts_with("archived_sessions/"));
    }

    /// **最近的会话是普通 `.jsonl`**（Codex 只把 7 天没动过的压成 `.zst`）。
    ///
    /// 只收 `.zst` ⇒ 「今日」「近 7 天」恒为 0（2026-10-24 实测本机：7 个
    /// 普通形态的会话装着 42,401,192 tokens，全落在最近 11 天）。
    #[test]
    fn plain_jsonl_sessions_are_counted_too() {
        let dir = TempDir::new("codex-plain");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        write_plain(
            &sessions.join("rollout-a.jsonl"),
            &[
                META,
                TURN,
                &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
            ],
        );
        let files = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        assert_eq!(files.len(), 1, "普通形态的会话也要算");
        let entry = files.values().next().unwrap();
        let day = entry.days.keys().next().expect("应有一天的账");
        let model = entry.days[day].values().next().expect("应有一个模型的账");
        assert_eq!(model.tokens, 110, "总量与压缩形态同一口径");
    }

    /// 同一逻辑会话的两种形态同时存在时**只算一份**。
    ///
    /// Codex 继续一段压缩过的会话时先解压回 `.jsonl` 再追加——那一刻两份都在
    /// 盘上。两份都读等于把同一段用量数两遍；留压缩那份会漏掉追加的行。
    #[test]
    fn both_forms_of_one_session_are_counted_once() {
        let dir = TempDir::new("codex-both-forms");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        let lines = [
            META,
            TURN,
            &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
        ];
        write_rollout(&sessions.join("rollout-a.jsonl.zst"), &lines);
        write_plain(&sessions.join("rollout-a.jsonl"), &lines);
        let files = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        assert_eq!(
            files.len(),
            1,
            "两种形态只留一份（普通那份是 Codex 正在写的）"
        );
        assert!(
            files.keys().next().unwrap().ends_with(".jsonl"),
            "留的是普通形态"
        );
        let entry = files.values().next().unwrap();
        let day = entry.days.keys().next().expect("应有一天的账");
        let model = entry.days[day].values().next().expect("应有一个模型的账");
        assert_eq!(model.tokens, 110, "只算一次，不翻倍");
    }

    /// **读失败的文件不能被当成「已读过」缓存下来**。
    ///
    /// 此前扫不出来时写的是一份**空账**，而空账带着那个文件的 size + mtime →
    /// 下一轮「大小与 mtime 没变就跳过」直接复用它 ⇒ 这个文件的用量被**永久**
    /// 抹掉（2026-10-24 评审发现）。
    #[test]
    fn a_failed_scan_is_retried_instead_of_being_cached_as_empty() {
        let dir = TempDir::new("codex-failed");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("rollout-a.jsonl.zst");
        // 不是合法的 zstd ⇒ 读不出来。
        fs::write(&path, b"this is not a zstd stream").unwrap();

        let first = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        let entry = first.values().next().expect("文件应在账里");
        assert!(entry.stale, "读失败的条目必须标成 stale（下一轮重扫）");
        assert!(entry.days.is_empty(), "读失败就没有账");

        // 文件换成好的之后，下一轮必须**重扫**（而不是复用那份空账）。
        write_rollout(
            &path,
            &[
                META,
                TURN,
                &token_count("2026-10-24T02:00:01.000Z", 100, 100, 10),
            ],
        );
        let second = scan_root(dir.path(), "2026-01-01", "2026-12-31", now_ms(), &first);
        let entry = second.values().next().expect("文件应在账里");
        assert!(!entry.stale, "读成功了就不再是 stale");
        let day = entry.days.keys().next().expect("应有一天的账");
        let model = entry.days[day].values().next().expect("应有一个模型的账");
        assert_eq!(model.tokens, 110, "重扫后账回来了");
    }

    /// `response_id` 的去重**跨文件**生效。
    ///
    /// 同一个请求被写进多个文件（fork 出来的会话会把父线程的记录带过来）时
    /// 只算一次。此前去重集合是**每个文件一份**，于是代码里那句「同一请求
    /// 可能被写进多个文件」的注释并没有被兑现（2026-10-24 评审发现）。
    #[test]
    fn the_same_response_id_in_two_files_counts_once() {
        let dir = TempDir::new("codex-dup-response");
        let sessions = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&sessions).unwrap();
        let record = serde_json::json!({
            "timestamp": "2026-10-24T02:00:01.000Z",
            "type": "token_usage_record",
            "payload": {
                "response_id": "resp-shared",
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 10,
                    "cached_input_tokens": 0,
                    "cache_write_input_tokens": 0,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 110
                }
            }
        })
        .to_string();
        write_plain(&sessions.join("rollout-a.jsonl"), &[META, TURN, &record]);
        write_plain(&sessions.join("rollout-b.jsonl"), &[META, TURN, &record]);
        let files = scan_root(
            dir.path(),
            "2026-01-01",
            "2026-12-31",
            now_ms(),
            &BTreeMap::new(),
        );
        assert_eq!(files.len(), 2, "两个文件都在");
        let mut requests = 0;
        let mut tokens = 0;
        for entry in files.values() {
            for per_model in entry.days.values() {
                for model in per_model.values() {
                    requests += model.requests;
                    tokens += model.tokens;
                }
            }
        }
        assert_eq!(requests, 1, "同一个 response_id 只算一次");
        assert_eq!(tokens, 110);
    }
}
