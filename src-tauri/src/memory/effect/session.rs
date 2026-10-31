//! 会话下钻：把一个内核会话的**结构事件**摊成只读时间线（无正文、无标题、无路径）。
//!
//! 用途是让「账本里某次注入到底发生了什么」可核对：注入节点、请求节点（token/工具
//! 数）、压缩节点，以及请求当时快照是否有效（exposure）。解析口径与 scan.rs 完全一致：
//! 复用它的 ActiveSnapshot / memory_ids_in_section / shadows_snapshot，不另写一套
//! zstd 解码或记忆段识别。
//!
//! 节点上限 MAX_NODES：单个会话可能上千个事件，面板弹窗只需要「最近发生了什么」，
//! 超限截断并在 note 里说清——不能假装完整。

use std::fs;

use serde::Serialize;
use serde_json::Value;

use super::report::iso_utc;
use super::scan::{
    find_session_file, memory_ids_in_section, shadows_snapshot, usage_tokens, ActiveSnapshot,
};
use crate::memory::store::Namespace;
use crate::shell::error::AppError;
use crate::shell::paths;
use crate::usage::local::decode_tail;

/// 时间线节点上限（合同 §2.6）。
const MAX_NODES: usize = 300;

/// 节点种类：injection / request / compaction。
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionNode {
    pub at: String,
    pub turn: i64,
    pub step: i64,
    pub kind: String,
    /// compaction 节点记事件类型（compaction/start 等）；其余为空。
    pub reason: String,
    /// 请求到来时是否有有效记忆快照；注入节点恒 true；压缩节点恒 false。
    pub exposure: bool,
    /// 注入节点：段内 id 数；其余为 0。
    pub ids: u64,
    /// 注入节点：段字节数；其余为 0。
    pub bytes: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    /// 归属于本请求的工具调用 / 错误数（绑定到下一条请求之前的事件）。
    pub tool_calls: u64,
    pub tool_errors: u64,
}

/// 面板拿到的会话时间线。
#[derive(Debug, Clone, Serialize)]
pub struct SessionTimeline {
    pub session: String,
    pub available: bool,
    pub nodes: Vec<SessionNode>,
    pub note: String,
}

/// 扫描一个会话目录，产出只读时间线。
pub fn timeline(namespace: &Namespace, session_id: &str) -> Result<SessionTimeline, AppError> {
    let sessions_root =
        paths::instance_dsh_home(&namespace.family, &namespace.instance_id).join("sessions");
    timeline_in(&sessions_root, session_id)
}

/// 可测入口：sessions 根由调用方给出（单测不碰真实用户目录）。
pub(crate) fn timeline_in(
    sessions_root: &std::path::Path,
    session_id: &str,
) -> Result<SessionTimeline, AppError> {
    let Some(path) = find_session_file(sessions_root, session_id) else {
        return Err(AppError::Memory(format!(
            "会话不存在或已删除（{session_id}）。它可能属于另一个实例，或已经被内核清理。"
        )));
    };
    let mut handle = fs::File::open(&path).map_err(|error| {
        AppError::Memory(format!(
            "无法读取会话日志 {}：{error}。请确认文件未被占用或删除。",
            path.display()
        ))
    })?;
    let mut timeline = TimelineBuilder::new(session_id);
    decode_tail(&mut handle, &mut |line| timeline.on_line(line));
    Ok(timeline.finish())
}

struct TimelineBuilder {
    session: String,
    nodes: Vec<SessionNode>,
    active: ActiveSnapshot,
    current_turn: i64,
    current_step: i64,
    last_request: Option<usize>,
    total_nodes: u64,
    malformed: u64,
}

impl TimelineBuilder {
    fn new(session: &str) -> Self {
        Self {
            session: session.to_string(),
            nodes: Vec::new(),
            active: ActiveSnapshot::default(),
            current_turn: -1,
            current_step: -1,
            last_request: None,
            total_nodes: 0,
            malformed: 0,
        }
    }

    fn push(&mut self, node: SessionNode) {
        self.total_nodes += 1;
        if self.nodes.len() < MAX_NODES {
            self.nodes.push(node);
        }
    }

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
        let time_ms = event.get("time").and_then(Value::as_u64);
        let at = time_ms.map(iso_utc).unwrap_or_default();
        match event_type {
            "turn/start" => {
                if let Some(turn) = event
                    .get("data")
                    .and_then(|data| data.get("turn"))
                    .and_then(Value::as_u64)
                {
                    self.current_turn = turn as i64;
                    self.current_step = -1;
                }
            }
            "step/start" => {
                let data = event.get("data");
                if let Some(turn) = data
                    .and_then(|data| data.get("turn"))
                    .and_then(Value::as_u64)
                {
                    self.current_turn = turn as i64;
                }
                if let Some(step) = data
                    .and_then(|data| data.get("step"))
                    .and_then(Value::as_u64)
                {
                    self.current_step = step as i64;
                }
            }
            "user/message" => self.on_snapshot(&event, at),
            "assistant/message" => self.on_request(&event, at),
            "tool/call" => {
                if let Some(index) = self.last_request {
                    self.nodes[index].tool_calls += 1;
                }
            }
            "tool/result" => {
                let is_error = event
                    .get("data")
                    .and_then(|data| data.get("message"))
                    .and_then(|message| message.get("isError"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if is_error {
                    if let Some(index) = self.last_request {
                        self.nodes[index].tool_errors += 1;
                    }
                }
            }
            "compaction/start" | "compaction/summary" | "compaction/prune" => {
                self.on_compaction(&event, event_type, at);
            }
            _ => {}
        }
    }

    fn on_snapshot(&mut self, event: &Value, at: String) {
        let Some(source) = event.get("data").and_then(|data| data.get("source")) else {
            return;
        };
        if source.get("kind").and_then(Value::as_str) != Some("runtime-context")
            || source.get("form").and_then(Value::as_str) != Some("snapshot")
        {
            return;
        }
        let Some(section) = source
            .get("sections")
            .and_then(Value::as_array)
            .and_then(|sections| {
                sections.iter().find(|section| {
                    section.get("name").and_then(Value::as_str) == Some("xlink.memory")
                })
            })
        else {
            return;
        };
        let text = section.get("text").and_then(Value::as_str).unwrap_or("");
        if text.is_empty() {
            return;
        }
        let ids = memory_ids_in_section(text);
        let bytes = text.len() as u64;
        let seq = event.get("seq").and_then(Value::as_u64).unwrap_or(0);
        self.push(SessionNode {
            at,
            turn: self.current_turn,
            step: self.current_step,
            kind: "injection".to_string(),
            reason: String::new(),
            exposure: true,
            ids: ids.len() as u64,
            bytes,
            ..SessionNode::default()
        });
        self.active = ActiveSnapshot { seq, ids, bytes };
    }

    fn on_request(&mut self, event: &Value, at: String) {
        let Some(data) = event.get("data") else {
            return;
        };
        let Some(usage) = data.get("usage").and_then(Value::as_object) else {
            return;
        };
        let turn = data.get("turn").and_then(Value::as_u64).unwrap_or(0) as i64;
        let step = data.get("step").and_then(Value::as_u64).unwrap_or(0) as i64;
        let node = SessionNode {
            at,
            turn,
            step,
            kind: "request".to_string(),
            reason: String::new(),
            exposure: self.active.seq != 0,
            ids: 0,
            bytes: 0,
            input: usage_tokens(usage, "inputTokens"),
            output: usage_tokens(usage, "outputTokens"),
            cache_read: usage_tokens(usage, "cacheReadTokens"),
            tool_calls: 0,
            tool_errors: 0,
        };
        self.push(node);
        self.last_request = self.nodes.len().checked_sub(1);
    }

    fn on_compaction(&mut self, event: &Value, event_type: &str, at: String) {
        let data = event.get("data");
        let turn = data
            .and_then(|data| data.get("turn"))
            .and_then(Value::as_u64)
            .map(|turn| turn as i64)
            .unwrap_or(self.current_turn);
        self.push(SessionNode {
            at,
            turn,
            step: -1,
            kind: "compaction".to_string(),
            reason: event_type.to_string(),
            exposure: false,
            ..SessionNode::default()
        });
        if event_type == "compaction/start" {
            self.active = ActiveSnapshot::default();
        } else if let Some(data) = data {
            if shadows_snapshot(data, self.active.seq) {
                self.active = ActiveSnapshot::default();
            }
        }
    }

    fn finish(self) -> SessionTimeline {
        let mut note = String::from("只读；不显示任何正文、标题或路径。");
        if self.total_nodes > MAX_NODES as u64 {
            note.push_str(&format!(
                "事件较多，已截断为最近前 {MAX_NODES} 个节点（共 {} 个）。",
                self.total_nodes
            ));
        }
        if self.malformed > 0 {
            note.push_str(&format!("有 {} 行无法解析，已跳过。", self.malformed));
        }
        SessionTimeline {
            session: self.session,
            available: true,
            nodes: self.nodes,
            note,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "effect-session-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 手工构造单帧 zstd 裸块流（Raw block）。与 effect::tests 的夹具同一格式，
    /// 但这里只需要「能解码出这些行」，因此直接按帧头字段拼。
    fn raw_frame(content: &[u8]) -> Vec<u8> {
        assert!(content.len() < (1 << 17), "测试用裸块放不下");
        let mut frame = vec![0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x00];
        let header = ((content.len() as u32) << 3) | 1;
        frame.extend_from_slice(&header.to_le_bytes()[..3]);
        frame.extend_from_slice(content);
        frame
    }

    fn write_session(root: &Path, events: &[serde_json::Value]) -> PathBuf {
        let dir = root.join("ws").join("session-abc");
        fs::create_dir_all(&dir).expect("建会话目录");
        let path = dir.join("session.v4.jsonl.zstd");
        let mut content = Vec::new();
        for event in events {
            content.extend_from_slice(serde_json::to_string(event).expect("序列化事件").as_bytes());
            content.push(b'\n');
        }
        let mut file = fs::File::create(&path).expect("建会话文件");
        file.write_all(&raw_frame(&content)).expect("写帧");
        path
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(1_790_000_000_000)
    }

    fn snapshot_event(seq: u64, time: u64, marker: &str) -> serde_json::Value {
        let text = format!(
            "以下是本机保存的、与当前工作相关的记忆。\n\n[{}] {marker} 标题\n{marker} 正文\n",
            "a".repeat(32)
        );
        json!({
            "type": "user/message",
            "seq": seq,
            "time": time,
            "data": {"source": {"kind": "runtime-context", "form": "snapshot", "sections": [
                {"name": "xlink.memory", "text": text}
            ]}}
        })
    }

    fn request_event(seq: u64, time: u64, turn: u64, step: u64) -> serde_json::Value {
        json!({
            "type": "assistant/message",
            "seq": seq,
            "time": time,
            "data": {
                "turn": turn,
                "step": step,
                "usage": {"inputTokens": 100, "outputTokens": 20, "cacheReadTokens": 5, "totalTokens": 125},
                "message": {"role": "assistant", "content": []}
            }
        })
    }

    #[test]
    fn timeline_marks_injection_request_and_compaction_nodes() {
        let root = scratch("timeline");
        let now = now_ms();
        let path = write_session(
            &root,
            &[
                json!({"type": "turn/start", "seq": 1, "time": now, "data": {"turn": 1}}),
                json!({"type": "step/start", "seq": 2, "time": now, "data": {"turn": 1, "step": 1}}),
                snapshot_event(3, now, "SECRET-MARKER"),
                request_event(4, now, 1, 1),
                json!({"type": "tool/call", "seq": 5, "time": now, "data": {"name": "bash", "arguments": "{}"}}),
                json!({"type": "tool/result", "seq": 6, "time": now, "data": {"message": {"isError": true}}}),
                json!({"type": "compaction/prune", "seq": 7, "time": now, "data": {"shadowedSeqs": [3], "shadowedRange": {"start": 3, "end": 3}}}),
                json!({"type": "turn/start", "seq": 8, "time": now, "data": {"turn": 2}}),
                json!({"type": "step/start", "seq": 9, "time": now, "data": {"turn": 2, "step": 1}}),
                request_event(10, now, 2, 1),
            ],
        );
        let timeline = timeline_in(&root, "session-abc").expect("时间线应成功");
        assert!(timeline.available);
        let kinds: Vec<&str> = timeline
            .nodes
            .iter()
            .map(|node| node.kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec!["injection", "request", "compaction", "request"],
            "节点顺序必须是事件顺序：{kinds:?}"
        );
        let injection = &timeline.nodes[0];
        assert_eq!(injection.ids, 1);
        assert!(injection.bytes > 0);
        assert!(injection.exposure);
        assert_eq!(injection.turn, 1, "注入节点用当前 step 的 turn/step 定位");
        assert_eq!(injection.step, 1);

        let first_request = &timeline.nodes[1];
        assert!(first_request.exposure, "快照有效时请求标记为暴露");
        assert_eq!(first_request.input, 100);
        assert_eq!(first_request.output, 20);
        assert_eq!(first_request.cache_read, 5);
        assert_eq!(first_request.tool_calls, 1, "工具调用归到发起它的请求");
        assert_eq!(first_request.tool_errors, 1);

        let compaction = &timeline.nodes[2];
        assert!(!compaction.exposure);
        assert_eq!(compaction.kind, "compaction");
        assert_eq!(compaction.reason, "compaction/prune");

        let second_request = &timeline.nodes[3];
        assert!(!second_request.exposure, "快照被 prune 遮蔽后请求不再暴露");
        assert!(timeline.note.contains("只读"), "note 要说明只读");
        // 隐私：时间线序列化后不得出现快照里的标题/正文标记。
        let serialized = serde_json::to_string(&timeline).expect("序列化时间线");
        assert!(!serialized.contains("SECRET-MARKER"));
        let _ = path;
    }

    #[test]
    fn timeline_truncates_after_the_node_limit() {
        let root = scratch("truncate");
        let now = now_ms();
        let mut events = Vec::new();
        for index in 0..(MAX_NODES as u64 + 5) {
            events.push(request_event(index + 1, now, index + 1, 1));
        }
        write_session(&root, &events);
        let timeline = timeline_in(&root, "session-abc").expect("时间线应成功");
        assert_eq!(timeline.nodes.len(), MAX_NODES, "节点数必须封顶");
        assert!(
            timeline.note.contains("截断"),
            "截断必须在 note 里说清：{}",
            timeline.note
        );
    }

    #[test]
    fn timeline_reports_missing_session_with_actionable_error() {
        let root = scratch("missing");
        let error = timeline_in(&root, "session-missing").expect_err("不存在的会话必须报错");
        let message = error.to_string();
        assert!(
            message.contains("会话不存在或已删除"),
            "错误要告诉用户下一步：{message}"
        );
    }
}
