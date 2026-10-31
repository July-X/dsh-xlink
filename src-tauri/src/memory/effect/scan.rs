//! 会话日志增量扫描：把效果账目从内核 session 文件推进到最新。
//!
//! 架构照搬 usage::local 的经验（同一条数据源、同一种多帧 zstd 追加写），但账目结构
//! 完全不同：那边是天 × 模型的 token 用量，这边是注入快照、携带记忆的请求、工具调用、
//! 每条记忆的曝光，以及 P1 的同会话分组（暴露 vs 未暴露）与快照消费者的 turn。
//! 解码与切行复用 usage::local::decode_tail（它按完整帧交付、残帧停住且 offset 不越过
//! 坏帧），变体选择复用 usage::variant_cache。
//!
//! 扫描先串行：P0 实测本机规模在秒级，P1 不做并行，避免把 usage 的 scan_parallel 复制
//! 一份（重复代码门禁）。解析分两层：FileAccumulator::on_line 是纯解析（可单测、只写
//! 内存结构），decode_file 负责文件 IO 与合账。

use std::collections::BTreeMap;
use std::fs;
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

use super::{DayAgg, EffectStateDoc, GroupAgg, MemDayAgg, RETENTION_DAYS};
use crate::usage::local::{date_string_of_ms, decode_tail};
use crate::usage::variant_cache;

const DAY_MS: u64 = 86_400_000;

/// 认识但不需要解析的事件类型。不在清单里的类型计入未知事件占比（格式漂移的警报）。
/// 这张表只回答「内核是不是换了一种事件」，不回答「我们有没有用到它」。
const KNOWN_EVENT_TYPES: &[&str] = &[
    "session",
    "subagent/descriptor",
    "subagent/model-selection-policy",
    "subagent/catalog",
    "permission/preset",
    "sandbox/mode",
    "approval/policy",
    "agent/inbox/spliced",
    "turn/start",
    "turn/end",
    "step/start",
    "step/end",
    "system/message",
    "developer/message",
    "user/message",
    "assistant/message",
    "assistant/attempt",
    "tool/call",
    "tool/result",
    "request/header",
    "request/context",
    "session/title",
    "session/title-llm-request",
    "session/end-seed",
    "compaction/start",
    "compaction/summary",
    "compaction/prune",
    "compaction/end",
    "hook/invoked",
    "hook/result",
    "job/registered",
    "job/settled",
    "team/task",
    "team/member",
    "todo/write",
    "llm/retry",
    "llm/retry-started",
    "web/deepseek-search-llm-request",
    "deliverables/presented",
    "workspace/changes",
    "goal/change",
    "command/run",
    "command/done",
];

/// 当前有效的记忆快照（只存在于扫描期内存里；落盘时摊到 FileEntry 的 active_* 字段）。
#[derive(Clone, Default)]
pub(crate) struct ActiveSnapshot {
    pub seq: u64,
    pub ids: Vec<String>,
    pub bytes: u64,
}

/// 一个会话文件本轮新解出来的账，合账前只在这里攒着。
///
/// `titles` 是 P1 的弱引用代理素材（id -> 记忆标题）。它**只在内存里**参与子串匹配，
/// 绝不写进 state——隐私红线仍是「账本不含标题/正文」。
struct FileAccumulator<'a> {
    cutoff: String,
    active: ActiveSnapshot,
    /// 还没被下一条带 usage 的 assistant/message 回填的快照数（跨帧持久化）。
    pending_snapshots: u64,
    /// 最近一条请求是否有有效快照（tool 事件按它归入 exposed/unexposed 组）。
    last_request_exposed: Option<bool>,
    titles: &'a [(String, String)],
    days: BTreeMap<String, DayAgg>,
    memories: BTreeMap<String, BTreeMap<String, MemDayAgg>>,
    snapshot_turns: BTreeMap<u32, u64>,
    malformed: u64,
}

impl<'a> FileAccumulator<'a> {
    fn new(
        cutoff: &str,
        active: ActiveSnapshot,
        pending_snapshots: u64,
        last_request_exposed: Option<bool>,
        titles: &'a [(String, String)],
    ) -> Self {
        Self {
            cutoff: cutoff.to_string(),
            active,
            pending_snapshots,
            last_request_exposed,
            titles,
            days: BTreeMap::new(),
            memories: BTreeMap::new(),
            snapshot_turns: BTreeMap::new(),
            malformed: 0,
        }
    }

    /// 一行 JSONL -> 账。只有能归属于窗口内某一天的事件才进日账；状态机（有效快照的
    /// 建立与遮蔽、快照 turn 的回填）对所有事件都跑，否则一个 90 天前建立、至今没被
    /// 遮蔽的快照会让窗口内的请求全部少算。
    fn on_line(&mut self, line: &[u8]) {
        let Ok(text) = std::str::from_utf8(line) else {
            self.malformed += 1;
            return;
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(text) else {
            self.malformed += 1;
            return;
        };
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        let day = event
            .get("time")
            .and_then(Value::as_u64)
            .map(date_string_of_ms)
            .filter(|day| day.as_str() >= self.cutoff.as_str());
        if let Some(day) = day.as_deref() {
            let bucket = self.days.entry(day.to_string()).or_default();
            bucket.events += 1;
            if !KNOWN_EVENT_TYPES.contains(&event_type) {
                bucket.unknown_events += 1;
            }
        }
        match event_type {
            "user/message" => self.on_user_message(&event, day.as_deref()),
            "assistant/message" => self.on_assistant_message(&event, day.as_deref()),
            "tool/call" => self.on_tool_call(&event, day.as_deref()),
            "tool/result" => self.on_tool_result(&event, day.as_deref()),
            "compaction/start" => {
                self.active = ActiveSnapshot::default();
                self.pending_snapshots = 0;
            }
            "compaction/summary" | "compaction/prune" => {
                if let Some(data) = event.get("data") {
                    if shadows_snapshot(data, self.active.seq) {
                        self.active = ActiveSnapshot::default();
                        self.pending_snapshots = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn on_user_message(&mut self, event: &Value, day: Option<&str>) {
        let Some(source) = event.get("data").and_then(|data| data.get("source")) else {
            return;
        };
        if source.get("kind").and_then(Value::as_str) != Some("runtime-context")
            || source.get("form").and_then(Value::as_str) != Some("snapshot")
        {
            return;
        }
        let Some(sections) = source.get("sections").and_then(Value::as_array) else {
            return;
        };
        let Some(section) = sections
            .iter()
            .find(|section| section.get("name").and_then(Value::as_str) == Some("xlink.memory"))
        else {
            return;
        };
        let text = section.get("text").and_then(Value::as_str).unwrap_or("");
        if text.is_empty() {
            // 空段 = 插件这次没放任何记忆，不构成一次注入快照。
            return;
        }
        let ids = memory_ids_in_section(text);
        let bytes = text.len() as u64;
        let seq = event.get("seq").and_then(Value::as_u64).unwrap_or(0);
        if let Some(day) = day {
            let bucket = self.days.entry(day.to_string()).or_default();
            bucket.injection_snapshots += 1;
            bucket.injected_bytes += bytes;
            for id in &ids {
                self.memory_day(id, day).injections += 1;
            }
        }
        self.active = ActiveSnapshot { seq, ids, bytes };
        // 快照本身没有 turn：按合同由下一条带 usage 的 assistant/message 回填。
        self.pending_snapshots += 1;
    }

    fn on_assistant_message(&mut self, event: &Value, day: Option<&str>) {
        let Some(data) = event.get("data") else {
            return;
        };
        // 只有带 usage 的 assistant/message 才是一次模型请求（种子回放不算）。
        let Some(usage) = data.get("usage").and_then(Value::as_object) else {
            return;
        };
        let has_snapshot = self.active.seq != 0;
        let active_ids = self.active.ids.clone();
        // 状态机部分与日窗口无关：快照 turn 的回填是账本关联的依据。
        if self.pending_snapshots > 0 {
            if let Some(turn) = data.get("turn").and_then(Value::as_u64) {
                *self.snapshot_turns.entry(turn as u32).or_default() += self.pending_snapshots;
                self.pending_snapshots = 0;
            }
        }
        self.last_request_exposed = Some(has_snapshot);
        let reply_text = message_text(data);
        if let Some(day) = day {
            let bucket = self.days.entry(day.to_string()).or_default();
            bucket.requests += 1;
            // P2 对照实验的会话级指标用日账总量（与分组无关的 token 总数）。
            bucket.input_tokens += usage_tokens(usage, "inputTokens");
            bucket.output_tokens += usage_tokens(usage, "outputTokens");
            bucket.cache_read_tokens += usage_tokens(usage, "cacheReadTokens");
            let group = if has_snapshot {
                &mut bucket.exposed
            } else {
                &mut bucket.unexposed
            };
            group.requests += 1;
            group.input_tokens += usage_tokens(usage, "inputTokens");
            group.output_tokens += usage_tokens(usage, "outputTokens");
            group.cache_read_tokens += usage_tokens(usage, "cacheReadTokens");
            if has_snapshot {
                bucket.memory_requests += 1;
            }
            // 弱引用代理：回复正文里出现某条记忆的标题即记一次（同一回复同一 id 只 +1）。
            let mut mentioned: Vec<String> = Vec::new();
            if !reply_text.is_empty() {
                for (id, title) in self.titles {
                    if reply_text.contains(title.as_str()) {
                        mentioned.push(id.clone());
                    }
                }
            }
            for id in &mentioned {
                self.memory_day(id, day).mentions += 1;
            }
            if has_snapshot {
                for id in &active_ids {
                    self.memory_day(id, day).exposures += 1;
                }
            }
        }
    }

    fn on_tool_call(&mut self, event: &Value, day: Option<&str>) {
        let Some(data) = event.get("data") else {
            return;
        };
        let name = data.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(day) = day else {
            return;
        };
        let read_id = if name == "memory_read" {
            memory_read_id(data.get("arguments"))
        } else {
            None
        };
        let exposed = self.last_request_exposed;
        let bucket = self.days.entry(day.to_string()).or_default();
        bucket.tool_calls += 1;
        // tool 调用归到「发起它的那条请求」所在组：这是同会话配对里「一次请求用了
        // 多少次工具」的口径。请求之前的孤立 tool 事件没有可归属的组，只进总量。
        if let Some(exposed) = exposed {
            let group: &mut GroupAgg = if exposed {
                &mut bucket.exposed
            } else {
                &mut bucket.unexposed
            };
            group.tool_calls += 1;
        }
        match name {
            "memory_read" => bucket.memory_read_calls += 1,
            "memory_search" => bucket.memory_search_calls += 1,
            "memory_propose" => bucket.memory_propose_calls += 1,
            _ => {}
        }
        if let Some(id) = read_id {
            self.memory_day(&id, day).reads += 1;
        }
    }

    fn on_tool_result(&mut self, event: &Value, day: Option<&str>) {
        let Some(day) = day else {
            return;
        };
        let is_error = event
            .get("data")
            .and_then(|data| data.get("message"))
            .and_then(|message| message.get("isError"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_error {
            return;
        }
        let exposed = self.last_request_exposed;
        let bucket = self.days.entry(day.to_string()).or_default();
        bucket.tool_errors += 1;
        if let Some(exposed) = exposed {
            let group: &mut GroupAgg = if exposed {
                &mut bucket.exposed
            } else {
                &mut bucket.unexposed
            };
            group.tool_errors += 1;
        }
    }

    fn memory_day(&mut self, id: &str, day: &str) -> &mut MemDayAgg {
        self.memories
            .entry(id.to_string())
            .or_default()
            .entry(day.to_string())
            .or_default()
    }
}

/// usage 字段 -> u64；取不到按 0（与 usage::local 的用法同一口径）。下钻时间线复用。
pub(crate) fn usage_tokens(usage: &serde_json::Map<String, Value>, field: &str) -> u64 {
    match usage.get(field) {
        Some(Value::Number(number)) => number
            .as_u64()
            .or_else(|| number.as_f64().map(|value| value.max(0.0).round() as u64))
            .unwrap_or(0),
        _ => 0,
    }
}

/// assistant/message 的回复正文：content 数组里 type=="text" 的 text 拼接。
/// 只用于内存里的 mentions 匹配与下钻的「无正文」统计，不会被写进任何落盘文件。
pub(crate) fn message_text(data: &Value) -> String {
    let Some(content) = data
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return String::new();
    };
    let mut text = String::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(part) = block.get("text").and_then(Value::as_str) {
            text.push_str(part);
        }
    }
    text
}

/// 快照段文本里按行首 [32 位 hex] 表头提取 id。格式来自 query.rs::render_context：
/// 每个条目以 "画\n[<id>] <标题>（范围 · 验证，更新于 ...）" 开头。
pub(crate) fn memory_ids_in_section(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let bytes = rest.as_bytes();
        if bytes.len() < 33 || bytes[32] != b']' {
            continue;
        }
        let Ok(id) = std::str::from_utf8(&bytes[..32]) else {
            continue;
        };
        if id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            ids.push(id.to_string());
        }
    }
    ids
}

/// memory_read 的参数里取 id（{"id":"..."}）；取不到就不记 reads。
fn memory_read_id(arguments: Option<&Value>) -> Option<String> {
    let raw = arguments?.as_str()?;
    let parsed: Value = serde_json::from_str(raw).ok()?;
    parsed
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// 这条 compaction 事件是否遮蔽了 seq 对应的快照。
pub(crate) fn shadows_snapshot(data: &Value, seq: u64) -> bool {
    if seq == 0 {
        return false;
    }
    if let Some(seqs) = data.get("shadowedSeqs").and_then(Value::as_array) {
        if seqs.iter().any(|entry| entry.as_u64() == Some(seq)) {
            return true;
        }
    }
    if let Some(range) = data.get("shadowedRange") {
        let start = range.get("start").and_then(Value::as_u64);
        let end = range.get("end").and_then(Value::as_u64);
        if let (Some(start), Some(end)) = (start, end) {
            if seq >= start && seq <= end {
                return true;
            }
        }
    }
    false
}

/// 扫描 sessions 根：枚举会话文件、续扫各自的新帧、合账、剪窗口。
///
/// `titles` 是 P1 弱引用代理用的 id->标题 列表（只进内存）。
pub(crate) fn scan_in(
    doc: &mut EffectStateDoc,
    sessions_root: &Path,
    now_ms: u64,
    titles: &[(String, String)],
) {
    let cutoff = date_string_of_ms(now_ms.saturating_sub((RETENTION_DAYS - 1) * DAY_MS));
    let found = enumerate_sessions(sessions_root);
    let mut decode_failures = 0u64;
    for (key, path) in &found {
        decode_file(
            doc,
            key,
            path,
            now_ms,
            &cutoff,
            titles,
            &mut decode_failures,
        );
    }
    // 会话目录被删 / 变体切换：账目键不再出现就整条移除（变体切换即从 0 重扫，不会双计）。
    doc.files
        .retain(|key, _| found.iter().any(|(seen, _)| seen == key));
    prune_window(doc, &cutoff);
    doc.decode_failures = decode_failures;
}

/// 枚举当前的会话文件（key 与文件路径）。变体解析走 usage::variant_cache。
fn enumerate_sessions(sessions_root: &Path) -> Vec<(String, PathBuf)> {
    collect_session_files(sessions_root)
        .into_iter()
        .map(|(key, _, path)| (key, path))
        .collect()
}

/// 枚举会话文件：返回（帐目键, 会话目录名, 当前 variant 文件）。
///
/// 会话目录名就是内核会话 id（session-<uuid>），账本关联与下钻命令都按它找。
fn collect_session_files(sessions_root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut found = Vec::new();
    let Ok(workspaces) = fs::read_dir(sessions_root) else {
        return found;
    };
    let mut cache = variant_cache::variant_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (root_cache, gen) = variant_cache::begin_round(&mut cache, sessions_root);
    for workspace in workspaces.flatten() {
        let Ok(sessions) = fs::read_dir(workspace.path()) else {
            continue;
        };
        for session in sessions.flatten() {
            let Some(file) = variant_cache::resolve_variant(&session.path(), root_cache, gen)
            else {
                continue;
            };
            let session_id = session.file_name().to_string_lossy().into_owned();
            let key = format!(
                "{}/{}/{}",
                workspace.file_name().to_string_lossy(),
                session_id,
                file.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
            found.push((key, session_id, file));
        }
    }
    let _ = variant_cache::end_round(root_cache, gen);
    found
}

/// 按内核会话 id 找到当前 variant 文件（下钻命令用；找不到返回 None）。
pub(crate) fn find_session_file(sessions_root: &Path, session_id: &str) -> Option<PathBuf> {
    collect_session_files(sessions_root)
        .into_iter()
        .find(|(_, id, _)| id == session_id)
        .map(|(_, _, path)| path)
}

/// 续扫一个文件：从 offset 解到 EOF，把新事件叠进该文件的账。
fn decode_file(
    doc: &mut EffectStateDoc,
    key: &str,
    path: &Path,
    now_ms: u64,
    cutoff: &str,
    titles: &[(String, String)],
    decode_failures: &mut u64,
) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    let size = meta.len();
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let known_offset = doc.files.get(key).map_or(0, |entry| entry.offset);
    // 文件被截断 / 重写：这份账作废重来。
    let reset = size < known_offset;
    let start = if reset { 0 } else { known_offset };
    if start == 0 && mtime_ms > 0 && mtime_ms + (RETENTION_DAYS + 1) * DAY_MS < now_ms {
        // 最后修改时间都在窗口之前 => 文件里每条记录都早于窗口，一条都不用解。
        let entry = doc.files.entry(key.to_string()).or_default();
        clear_entry(entry, size);
        return;
    }
    let entry = doc.files.entry(key.to_string()).or_default();
    if start >= size {
        if reset {
            clear_entry(entry, start);
        }
        return;
    }
    let Ok(mut handle) = fs::File::open(path) else {
        return;
    };
    if handle.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let active = if reset {
        ActiveSnapshot::default()
    } else {
        ActiveSnapshot {
            seq: entry.active_seq,
            ids: entry.active_ids.clone(),
            bytes: entry.active_bytes,
        }
    };
    let pending_snapshots = if reset {
        0
    } else {
        entry.pending_snapshot_turns
    };
    let last_request_exposed = if reset {
        None
    } else {
        entry.last_request_exposed
    };
    let mut accumulator = FileAccumulator::new(
        cutoff,
        active,
        pending_snapshots,
        last_request_exposed,
        titles,
    );
    let consumed = decode_tail(&mut handle, &mut |line| accumulator.on_line(line));
    let pending = size.saturating_sub(start);
    if consumed == 0 && pending > 0 {
        // 有未消费字节但整帧就解不动：文件损坏的迹象，记进数据健康。
        *decode_failures += 1;
    }
    if reset {
        clear_entry(entry, 0);
        entry.offset = start;
    }
    for (day, bucket) in &accumulator.days {
        entry.days.entry(day.clone()).or_default().merge(bucket);
    }
    for (id, per_day) in &accumulator.memories {
        let target = entry.memories.entry(id.clone()).or_default();
        for (day, bucket) in per_day {
            target.entry(day.clone()).or_default().merge(bucket);
        }
    }
    for (turn, count) in &accumulator.snapshot_turns {
        *entry.snapshot_turns.entry(*turn).or_default() += count;
    }
    entry.malformed_lines += accumulator.malformed;
    entry.offset = start + consumed;
    entry.active_seq = accumulator.active.seq;
    entry.active_ids = accumulator.active.ids;
    entry.active_bytes = accumulator.active.bytes;
    entry.pending_snapshot_turns = accumulator.pending_snapshots;
    entry.last_request_exposed = accumulator.last_request_exposed;
}

/// 作废一份文件账（截断 / 变体切换 / 窗口外旧文件）。
fn clear_entry(entry: &mut super::FileEntry, offset: u64) {
    entry.offset = offset;
    entry.active_seq = 0;
    entry.active_ids.clear();
    entry.active_bytes = 0;
    entry.pending_snapshot_turns = 0;
    entry.last_request_exposed = None;
    entry.malformed_lines = 0;
    entry.days.clear();
    entry.memories.clear();
    entry.snapshot_turns.clear();
}

/// 窗口外的日账在每轮保存前剪掉：账目体积因此有硬上界。
fn prune_window(doc: &mut EffectStateDoc, cutoff: &str) {
    for entry in doc.files.values_mut() {
        entry.days.retain(|day, _| day.as_str() >= cutoff);
        entry.memories.retain(|_, per_day| {
            per_day.retain(|day, _| day.as_str() >= cutoff);
            !per_day.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot_text(ids: &[&str]) -> String {
        let mut text = String::from("以下是本机保存的、与当前工作相关的记忆。\n");
        for id in ids {
            text.push_str(&format!("\n[{id}] 标题\n正文\n"));
        }
        text
    }

    fn snapshot_event(seq: u64, now: u64, ids: &[&str]) -> Value {
        json!({
            "type": "user/message",
            "seq": seq,
            "time": now,
            "data": {"source": {"kind": "runtime-context", "form": "snapshot", "sections": [
                {"name": "xlink.memory", "text": snapshot_text(ids)},
                {"name": "working-directory:current", "text": "x"}
            ]}}
        })
    }

    fn request_event(seq: u64, now: u64, turn: u64, usage: Value, texts: &[&str]) -> Value {
        json!({
            "type": "assistant/message",
            "seq": seq,
            "time": now,
            "data": {
                "turn": turn,
                "step": 1,
                "usage": usage,
                "message": {"role": "assistant", "content": texts.iter().map(|text| json!({"type": "text", "text": text})).collect::<Vec<_>>()}
            }
        })
    }

    fn accumulator<'a>(titles: &'a [(String, String)]) -> FileAccumulator<'a> {
        FileAccumulator::new("0000-00-00", ActiveSnapshot::default(), 0, None, titles)
    }

    fn feed(acc: &mut FileAccumulator<'_>, event: &Value) {
        acc.on_line(serde_json::to_string(event).unwrap().as_bytes());
    }

    #[test]
    fn memory_ids_come_only_from_32_hex_headers() {
        let text = format!(
            "开头说明\n\n[{}] 标题（项目记忆 · 已验证，更新于 2026-10-01T00:00:00Z）\n正文\n\n[{}] 不是 id\n[短] 也不算\n",
            "a".repeat(32),
            "b".repeat(31),
        );
        assert_eq!(memory_ids_in_section(&text), vec!["a".repeat(32)]);
        let body = "正文 [00112233445566778899aabbccddeeff] 不算";
        assert!(memory_ids_in_section(body).is_empty());
    }

    #[test]
    fn shadow_check_honors_seqs_and_range() {
        let data = json!({"shadowedSeqs": [3, 7], "shadowedRange": {"start": 10, "end": 20}});
        assert!(shadows_snapshot(&data, 7));
        assert!(shadows_snapshot(&data, 15));
        assert!(!shadows_snapshot(&data, 8));
        assert!(!shadows_snapshot(&data, 0));
    }

    #[test]
    fn accumulator_counts_snapshot_requests_and_tools() {
        let now = 1_790_000_000_000u64;
        let day = date_string_of_ms(now);
        let snapshot = snapshot_event(1, now, &["a".repeat(32).as_str()]);
        let request = request_event(
            2,
            now,
            1,
            json!({"inputTokens": 10, "outputTokens": 2, "cacheReadTokens": 3}),
            &[],
        );
        let call = json!({
            "type": "tool/call",
            "seq": 3,
            "time": now,
            "data": {"name": "memory_search", "arguments": "{}"}
        });
        let failed = json!({
            "type": "tool/result",
            "seq": 4,
            "time": now,
            "data": {"message": {"isError": true}}
        });
        let mut acc = accumulator(&[]);
        for event in [snapshot, request, call, failed] {
            feed(&mut acc, &event);
        }
        let bucket = acc.days.get(&day).expect("应有日账");
        assert_eq!(bucket.injection_snapshots, 1);
        assert!(bucket.injected_bytes > 0);
        assert_eq!(bucket.requests, 1);
        assert_eq!(bucket.memory_requests, 1);
        assert_eq!(bucket.tool_calls, 1);
        assert_eq!(bucket.tool_errors, 1);
        assert_eq!(bucket.memory_search_calls, 1);
        assert_eq!(bucket.exposed.requests, 1, "有有效快照的请求进 exposed 组");
        assert_eq!(bucket.exposed.input_tokens, 10);
        assert_eq!(bucket.exposed.tool_calls, 1, "工具调用按发起它的请求归组");
        assert_eq!(bucket.exposed.tool_errors, 1);
        assert_eq!(bucket.unexposed.requests, 0);
        assert_eq!(acc.memories["a".repeat(32).as_str()][&day].injections, 1);
        assert_eq!(acc.memories["a".repeat(32).as_str()][&day].exposures, 1);
        assert_eq!(acc.active.seq, 1);
        assert_eq!(
            acc.snapshot_turns.get(&1),
            Some(&1),
            "快照 turn 由下一条请求回填"
        );
    }

    #[test]
    fn compaction_clears_active_snapshot_until_next_injection() {
        let now = 1_790_000_000_000u64;
        let start = json!({"type": "compaction/start", "seq": 3, "time": now, "data": {"turn": 1}});
        let mut acc = accumulator(&[]);
        for event in [
            snapshot_event(1, now, &["c".repeat(32).as_str()]),
            request_event(2, now, 1, json!({"inputTokens": 1}), &[]),
            start,
            request_event(5, now, 2, json!({"inputTokens": 1}), &[]),
        ] {
            feed(&mut acc, &event);
        }
        let today = date_string_of_ms(now);
        assert_eq!(
            acc.days[&today].memory_requests, 1,
            "压缩后请求不再算携带记忆"
        );
        assert_eq!(acc.days[&today].exposed.requests, 1);
        assert_eq!(acc.days[&today].unexposed.requests, 1);
        feed(
            &mut acc,
            &snapshot_event(7, now, &["c".repeat(32).as_str()]),
        );
        feed(
            &mut acc,
            &request_event(8, now, 3, json!({"inputTokens": 1}), &[]),
        );
        assert_eq!(acc.days[&today].memory_requests, 2);
        assert_eq!(acc.active.seq, 7);
    }

    #[test]
    fn snapshot_turns_count_occurrences_until_the_consuming_request() {
        let now = 1_790_000_000_000u64;
        let mut acc = accumulator(&[]);
        // 两条快照 + 一条带 usage 的请求：turn 4 记 2 次。
        feed(
            &mut acc,
            &snapshot_event(1, now, &["a".repeat(32).as_str()]),
        );
        feed(
            &mut acc,
            &snapshot_event(2, now, &["a".repeat(32).as_str()]),
        );
        feed(
            &mut acc,
            &request_event(3, now, 4, json!({"inputTokens": 1}), &[]),
        );
        assert_eq!(acc.snapshot_turns.get(&4), Some(&2));
        assert_eq!(acc.pending_snapshots, 0);
        // 没有后续请求的快照不回填任何 turn。
        let run_after_last = snapshot_event(4, now, &["a".repeat(32).as_str()]);
        feed(&mut acc, &run_after_last);
        assert!(!acc.snapshot_turns.contains_key(&5));
        assert_eq!(acc.pending_snapshots, 1);
    }

    #[test]
    fn mentions_match_titles_once_per_reply_and_only_in_memory() {
        let now = 1_790_000_000_000u64;
        let day = date_string_of_ms(now);
        let hit_id = "1".repeat(32);
        let miss_id = "2".repeat(32);
        let titles = vec![
            (hit_id.clone(), "开发约束".to_string()),
            (miss_id.clone(), "完全不相关的标题".to_string()),
        ];
        let mut acc = accumulator(&titles);
        feed(&mut acc, &snapshot_event(1, now, &[hit_id.as_str()]));
        // 同一条回复里标题出现两次，同一 id 只 +1。
        feed(
            &mut acc,
            &request_event(
                2,
                now,
                1,
                json!({"inputTokens": 1}),
                &["按「开发约束」办", "开发约束 第一条"],
            ),
        );
        assert_eq!(acc.memories[&hit_id][&day].mentions, 1);
        assert_eq!(acc.memories.get(&miss_id).map(|_| ()), None);
        // 标题区分大小写、按原样子串匹配：换一种写法不算。
        feed(
            &mut acc,
            &request_event(3, now, 2, json!({"inputTokens": 1}), &["开发 约束"]),
        );
        assert_eq!(acc.memories[&hit_id][&day].mentions, 1);
    }

    #[test]
    fn old_state_without_p1_fields_still_deserializes() {
        // P0 的 state.json 形状：没有 exposed/unexposed/snapshot_turns/mentions。
        let legacy = json!({
            "schema": 1,
            "retention_days": 90,
            "last_scanned_at_ms": 1,
            "decode_failures": 0,
            "files": {
                "ws/session-x/session.v4.jsonl.zstd": {
                    "offset": 10,
                    "active_seq": 3,
                    "active_ids": ["3".repeat(32)],
                    "active_bytes": 100,
                    "malformed_lines": 0,
                    "days": {"2026-10-20": {"injection_snapshots": 1, "requests": 2, "memory_requests": 1}},
                    "memories": {}
                }
            }
        });
        let doc: EffectStateDoc = serde_json::from_value(legacy).expect("旧 state 必须可读");
        let entry = &doc.files["ws/session-x/session.v4.jsonl.zstd"];
        assert_eq!(entry.offset, 10);
        assert!(entry.snapshot_turns.is_empty());
        assert_eq!(entry.days["2026-10-20"].exposed, GroupAgg::default());
        assert_eq!(entry.days["2026-10-20"].requests, 2);
    }

    #[test]
    fn find_session_file_locates_by_kernel_session_id() {
        let root = std::env::temp_dir().join(format!(
            "effect-find-session-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let session = root.join("ws").join("session-abc");
        fs::create_dir_all(&session).expect("建目录");
        fs::write(session.join("session.v4.jsonl.zstd"), b"x").expect("写文件");
        assert_eq!(
            find_session_file(&root, "session-abc"),
            Some(session.join("session.v4.jsonl.zstd"))
        );
        assert!(find_session_file(&root, "session-missing").is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
