//! 会话日志增量扫描：把效果账目从内核 session 文件推进到最新。
//!
//! 架构照搬 usage::local 的经验（同一条数据源、同一种多帧 zstd 追加写），但账目结构
//! 完全不同：那边是天 × 模型的 token 用量，这边是注入快照、携带记忆的请求、工具调用与
//! 每条记忆的曝光。解码与切行复用 usage::local::decode_tail（它按完整帧交付、残帧停住
//! 且 offset 不越过坏帧），变体选择复用 usage::variant_cache。
//!
//! 扫描先串行：本机实测 63 个会话 / 321 MB 解压目标在秒级，P0 不做并行，避免把
//! usage 的 scan_parallel 复制一份（重复代码门禁）。
//!
//! 解析分两层：FileAccumulator::on_line 是纯解析（可单测、只写内存结构），
//! decode_file 负责文件 IO 与合账。两者分开后，口径判据不必真的落盘一个 zstd 文件。

use std::collections::BTreeMap;
use std::fs;
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

use super::{DayAgg, EffectStateDoc, MemDayAgg, RETENTION_DAYS};
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
struct FileAccumulator {
    cutoff: String,
    active: ActiveSnapshot,
    days: BTreeMap<String, DayAgg>,
    memories: BTreeMap<String, BTreeMap<String, MemDayAgg>>,
    malformed: u64,
}

impl FileAccumulator {
    fn new(cutoff: &str, active: ActiveSnapshot) -> Self {
        Self {
            cutoff: cutoff.to_string(),
            active,
            days: BTreeMap::new(),
            memories: BTreeMap::new(),
            malformed: 0,
        }
    }

    /// 一行 JSONL -> 账。只有能归属于窗口内某一天的事件才进日账；状态机（有效快照的
    /// 建立与遮蔽）对所有事件都跑，否则一个 90 天前建立、至今没被遮蔽的快照会让窗口内
    /// 的请求全部少算。
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
            "compaction/start" => self.active = ActiveSnapshot::default(),
            "compaction/summary" | "compaction/prune" => {
                let data = event.get("data");
                if let Some(data) = data {
                    if shadows_snapshot(data, self.active.seq) {
                        self.active = ActiveSnapshot::default();
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
    }

    fn on_assistant_message(&mut self, event: &Value, day: Option<&str>) {
        let Some(data) = event.get("data") else {
            return;
        };
        // 只有带 usage 的 assistant/message 才是一次模型请求（种子回放不算）。
        if data.get("usage").and_then(Value::as_object).is_none() {
            return;
        }
        let Some(day) = day else {
            return;
        };
        // 先把快照 ids 复制出来：下面既改日账又改每条记忆的日账，不能同时持两把 &self。
        let active_ids = self.active.ids.clone();
        let has_snapshot = self.active.seq != 0;
        let bucket = self.days.entry(day.to_string()).or_default();
        bucket.requests += 1;
        if !has_snapshot {
            return;
        }
        bucket.memory_requests += 1;
        for id in &active_ids {
            self.memory_day(id, day).exposures += 1;
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
        let bucket = self.days.entry(day.to_string()).or_default();
        bucket.tool_calls += 1;
        match name {
            "memory_read" => {
                bucket.memory_read_calls += 1;
                if let Some(id) = memory_read_id(data.get("arguments")) {
                    self.memory_day(&id, day).reads += 1;
                }
            }
            "memory_search" => bucket.memory_search_calls += 1,
            "memory_propose" => bucket.memory_propose_calls += 1,
            _ => {}
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
        if is_error {
            self.days.entry(day.to_string()).or_default().tool_errors += 1;
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

/// 快照段文本里按行首 [32 位 hex] 表头提取 id。格式来自 query.rs::render_context：
/// 每个条目以 "\n\n[<id>] <标题>（范围 · 验证，更新于 ...）" 开头。
fn memory_ids_in_section(text: &str) -> Vec<String> {
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
fn shadows_snapshot(data: &Value, seq: u64) -> bool {
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
pub(crate) fn scan_in(doc: &mut EffectStateDoc, sessions_root: &Path, now_ms: u64) {
    let cutoff = date_string_of_ms(now_ms.saturating_sub((RETENTION_DAYS - 1) * DAY_MS));
    let found = enumerate_sessions(sessions_root);
    let mut decode_failures = 0u64;
    for (key, path) in &found {
        decode_file(doc, key, path, now_ms, &cutoff, &mut decode_failures);
    }
    // 会话目录被删 / 变体切换：账目键不再出现就整条移除（变体切换即从 0 重扫，不会双计）。
    doc.files
        .retain(|key, _| found.iter().any(|(seen, _)| seen == key));
    prune_window(doc, &cutoff);
    doc.decode_failures = decode_failures;
}

/// 枚举当前的会话文件。变体解析走 usage::variant_cache（目录 mtime 闸，跨轮缓存）。
fn enumerate_sessions(sessions_root: &Path) -> Vec<(String, PathBuf)> {
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
            let key = format!(
                "{}/{}/{}",
                workspace.file_name().to_string_lossy(),
                session.file_name().to_string_lossy(),
                file.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
            found.push((key, file));
        }
    }
    let _ = variant_cache::end_round(root_cache, gen);
    found
}

/// 续扫一个文件：从 offset 解到 EOF，把新事件叠进该文件的账。
fn decode_file(
    doc: &mut EffectStateDoc,
    key: &str,
    path: &Path,
    now_ms: u64,
    cutoff: &str,
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
        entry.offset = size;
        entry.days.clear();
        entry.memories.clear();
        entry.active_seq = 0;
        entry.active_ids.clear();
        entry.active_bytes = 0;
        return;
    }
    let entry = doc.files.entry(key.to_string()).or_default();
    if start >= size {
        if reset {
            entry.offset = start;
            entry.days.clear();
            entry.memories.clear();
            entry.malformed_lines = 0;
            entry.active_seq = 0;
            entry.active_ids.clear();
            entry.active_bytes = 0;
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
    let mut accumulator = FileAccumulator::new(cutoff, active);
    let consumed = decode_tail(&mut handle, &mut |line| accumulator.on_line(line));
    let pending = size.saturating_sub(start);
    if consumed == 0 && pending > 0 {
        // 有未消费字节但整帧就解不动：文件损坏的迹象，记进数据健康。
        *decode_failures += 1;
    }
    if reset {
        entry.days.clear();
        entry.memories.clear();
        entry.malformed_lines = 0;
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
    entry.malformed_lines += accumulator.malformed;
    entry.offset = start + consumed;
    entry.active_seq = accumulator.active.seq;
    entry.active_ids = accumulator.active.ids;
    entry.active_bytes = accumulator.active.bytes;
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

    fn snapshot_text(ids: &[&str]) -> String {
        let mut text = String::from("以下是本机保存的、与当前工作相关的记忆。\n");
        for id in ids {
            text.push_str(&format!("\n[{id}] 标题\n正文\n"));
        }
        text
    }

    #[test]
    fn memory_ids_come_only_from_32_hex_headers() {
        let text = format!(
            "开头说明\n\n[{}] 标题（项目记忆 · 已验证，更新于 2026-10-01T00:00:00Z）\n正文\n\n[{}] 不是 id\n[短] 也不算\n",
            "a".repeat(32),
            "b".repeat(31),
        );
        assert_eq!(memory_ids_in_section(&text), vec!["a".repeat(32)]);
        // 正文里出现方括号不算（必须行首且 32 位 hex）。
        let body = "正文 [00112233445566778899aabbccddeeff] 不算";
        assert!(memory_ids_in_section(body).is_empty());
    }

    #[test]
    fn shadow_check_honors_seqs_and_range() {
        let data =
            serde_json::json!({"shadowedSeqs": [3, 7], "shadowedRange": {"start": 10, "end": 20}});
        assert!(shadows_snapshot(&data, 7));
        assert!(shadows_snapshot(&data, 15));
        assert!(!shadows_snapshot(&data, 8));
        assert!(!shadows_snapshot(&data, 0));
    }

    #[test]
    fn accumulator_counts_snapshot_requests_and_tools() {
        let now = 1_790_000_000_000u64;
        let day = date_string_of_ms(now);
        let snapshot = serde_json::json!({
            "type": "user/message",
            "seq": 1,
            "time": now,
            "data": {"source": {"kind": "runtime-context", "form": "snapshot", "sections": [
                {"name": "xlink.memory", "text": snapshot_text(&["a".repeat(32).as_str()])},
                {"name": "working-directory:current", "text": "x"}
            ]}}
        });
        let request = serde_json::json!({
            "type": "assistant/message",
            "seq": 2,
            "time": now,
            "data": {"turn": 1, "step": 1, "usage": {"inputTokens": 1}}
        });
        let call = serde_json::json!({
            "type": "tool/call",
            "seq": 3,
            "time": now,
            "data": {"name": "memory_search", "arguments": "{}"}
        });
        let failed = serde_json::json!({
            "type": "tool/result",
            "seq": 4,
            "time": now,
            "data": {"message": {"isError": true}}
        });
        let mut acc = FileAccumulator::new("0000-00-00", ActiveSnapshot::default());
        for event in [snapshot, request, call, failed] {
            acc.on_line(serde_json::to_string(&event).unwrap().as_bytes());
        }
        let bucket = acc.days.get(&day).expect("应有日账");
        assert_eq!(bucket.injection_snapshots, 1);
        assert!(bucket.injected_bytes > 0);
        assert_eq!(bucket.requests, 1);
        assert_eq!(bucket.memory_requests, 1);
        assert_eq!(bucket.tool_calls, 1);
        assert_eq!(bucket.tool_errors, 1);
        assert_eq!(bucket.memory_search_calls, 1);
        assert_eq!(acc.memories["a".repeat(32).as_str()][&day].injections, 1);
        assert_eq!(acc.memories["a".repeat(32).as_str()][&day].exposures, 1);
        assert_eq!(acc.active.seq, 1);
    }

    #[test]
    fn compaction_clears_active_snapshot_until_next_injection() {
        let now = 1_790_000_000_000u64;
        let snapshot = |seq: u64| {
            serde_json::json!({
                "type": "user/message", "seq": seq, "time": now,
                "data": {"source": {"kind": "runtime-context", "form": "snapshot", "sections": [
                    {"name": "xlink.memory", "text": snapshot_text(&["c".repeat(32).as_str()])}
                ]}}
            })
        };
        let request = |seq: u64| {
            serde_json::json!({
                "type": "assistant/message", "seq": seq, "time": now,
                "data": {"usage": {"inputTokens": 1}}
            })
        };
        let start = serde_json::json!({"type": "compaction/start", "seq": 3, "time": now, "data": {"turn": 1}});
        let prune = serde_json::json!({
            "type": "compaction/prune", "seq": 4, "time": now,
            "data": {"shadowedSeqs": [1], "shadowedRange": {"start": 1, "end": 1}}
        });
        let mut acc = FileAccumulator::new("0000-00-00", ActiveSnapshot::default());
        for event in [snapshot(1), request(2), start, request(5)] {
            acc.on_line(serde_json::to_string(&event).unwrap().as_bytes());
        }
        let today = date_string_of_ms(now);
        assert_eq!(
            acc.days[&today].memory_requests, 1,
            "压缩后请求不再算携带记忆"
        );
        // 再来一次 prune，遮蔽新快照；重新注入后恢复。
        let _ = prune;
        acc.on_line(serde_json::to_string(&snapshot(7)).unwrap().as_bytes());
        acc.on_line(serde_json::to_string(&request(8)).unwrap().as_bytes());
        assert_eq!(acc.days[&today].memory_requests, 2);
        assert_eq!(acc.active.seq, 7);
    }
}
