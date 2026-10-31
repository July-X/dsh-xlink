//! 记忆效果（P0 只读事实层）：从本实例的会话日志里聚合「记忆注入 / 携带记忆的请求 /
//! 主动读取 / 提取成本 / 数据健康」这些可复核的事实读数。
//!
//! ## 为什么在这里而不是 UI 侧
//!
//! 注入是否发生只能从内核自己写的会话日志里读：user/message 事件带 runtime-context
//! 快照，快照的 sections 里有 name 为 xlink.memory 的段；工具调用与请求用量也在同一
//! 份 JSONL 里。解析口径、聚合与时间窗都放在壳侧（前端只渲染），避免同一个数字在两处
//! 各算一遍。
//!
//! ## 口径（与设计稿 §3.2 / 合同 §2.2 对齐）
//!
//! - 注入快照：user/message 且 data.source.kind == runtime-context、form == snapshot，
//!   sections 中 name == xlink.memory 的段；段文本的 UTF-8 字节数即注入字节。
//!   不靠文本 marker 判断（对话里会出现同样的字样）。
//! - 有效记忆快照：最近一次注入快照，且没有被 compaction/start（压缩开了新一页）或
//!   compaction/prune、compaction/summary 的 shadowedSeqs / shadowedRange 遮蔽。
//!   请求（带 usage 的 assistant/message）到来时若有有效快照则计入携带记忆请求，并对
//!   快照内每个 id 记一次 exposure。
//! - 快照 id 从段文本行首的 [32 位 hex] 表头提取；不是 32 位 hex 的不计。
//! - 注入快照按出现次数计数（压缩后原样重发也算一次），与携带记忆请求分开。
//! - **粘性历史**：快照是追加在对话历史里的 user 消息，只有压缩/裁剪才会移除；后续一份
//!   不带 xlink.memory 段的 runtime-context 快照不会把更早那条记忆消息从历史里删掉，所以
//!   曝光一直记到被 shadowedSeqs / shadowedRange 遮蔽为止——这是设计稿 §3.2 的 exposure
//!   定义，不是遗漏。
//! - 已知近似：一个会话日志里可能交错多个 agent（子智能体）的事件，而事件没有稳定的
//!   agent 标识可用来分桶。按 seq 的粘性口径会把子智能体请求也计入父级曝光；在 10 个真实
//!   会话拷贝上实测偏差 8/1845 = 0.43%，P0 接受，P1 用注入账本的 request_seq 精确 join。
//! - 时间归属用事件顶层 time（毫秒）换算成本地日历日；窗口起点含当天、终点为今天。
//! - 隐私：本模块只把 id、字节数与计数写进效果账本，绝不写标题或正文；面板上的标题
//!   在出报告时按 id 从 memory.json 现查（已删除的 id 只给状态文案）。
//!
//! ## 状态布局
//!
//! 效果账本落 paths::instance_memory_effect_file（instance_memory_dir/effect/state.json），
//! 与 memory.json 同源隔离（按实例与 profile 分家），键是「工作区/会话目录/文件名」。
//! 它整份都是派生数据：损坏时按 state::load_rebuildable 从零重扫（并记一行壳日志），
//! 绝不能反过来影响权威记忆库。落盘走 save_timed_light（派生数据不付两次 fsync）。
//!
//! 增量：每个文件记 offset，只解新追加的 zstd 帧；FileEntry 同时持久化 active 快照
//! （seq/ids/bytes），否则续扫之后的请求无法判断曝光。文件被截断或同一会话目录换了
//! 更高的 variant 时，该文件账目作废并从 0 重扫（variant 切换体现为账目键变化，旧的
//! 键在本轮没有再次出现，随 retain 一起移除，因此不会双计）。
//!
//! 保留 90 天；窗口外的日账在每轮扫描结束时剪掉。

pub(crate) mod report;
pub(crate) mod scan;

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::memory::jobs::{self, JobsDoc};
use crate::memory::store::Namespace;
use crate::memory::store::{self, MemoryDoc};
use crate::shell::error::AppError;
use crate::shell::state::StateCtx;
use crate::shell::{paths, shell_events, state};

/// 效果账本的保留窗口（天，含今天），与模型用量账目取同一档。
pub const RETENTION_DAYS: u64 = 90;

/// 面板可选的时间窗；报告只接受这三档。
pub const RANGE_CHOICES: [u32; 3] = [7, 30, 90];

const STATE_SCHEMA: u32 = 1;

/// 单文件 × 单日的增量账。所有计数都是「本文件本日」的累计值，视图在读取时按窗口求和。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct DayAgg {
    /// 注入快照次数（段出现次数，压缩后重发也算）。
    pub injection_snapshots: u64,
    /// 注入段字节数（UTF-8）。
    pub injected_bytes: u64,
    /// 带 usage 的模型请求数。
    pub requests: u64,
    /// 请求到来时存在有效记忆快照的次数。
    pub memory_requests: u64,
    /// 本日解析到的事件行数（含未知类型）。
    pub events: u64,
    /// 本日类型不在已知兼容清单里的事件数。
    pub unknown_events: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub memory_read_calls: u64,
    pub memory_search_calls: u64,
    pub memory_propose_calls: u64,
}

impl DayAgg {
    /// 增量合账：把另一个同口径的日账加进来。
    pub(crate) fn merge(&mut self, other: &DayAgg) {
        self.injection_snapshots += other.injection_snapshots;
        self.injected_bytes += other.injected_bytes;
        self.requests += other.requests;
        self.memory_requests += other.memory_requests;
        self.events += other.events;
        self.unknown_events += other.unknown_events;
        self.tool_calls += other.tool_calls;
        self.tool_errors += other.tool_errors;
        self.memory_read_calls += other.memory_read_calls;
        self.memory_search_calls += other.memory_search_calls;
        self.memory_propose_calls += other.memory_propose_calls;
    }
}

/// 单条记忆 × 单日的增量账。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct MemDayAgg {
    /// 该记忆出现在注入快照里的次数。
    pub injections: u64,
    /// 携带该记忆的请求数（exposure，单位条·请求）。
    pub exposures: u64,
    /// 模型用 memory_read 按 id 主动读全文的次数。
    pub reads: u64,
}

impl MemDayAgg {
    pub(crate) fn merge(&mut self, other: &MemDayAgg) {
        self.injections += other.injections;
        self.exposures += other.exposures;
        self.reads += other.reads;
    }
}

/// 一个会话文件的增量账。
///
/// active 快照的 seq/ids/bytes 必须持久化：账目按 offset 续扫，续扫后的请求只能靠它
/// 判断「当时上下文里还有没有记忆」。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct FileEntry {
    /// 已消费的压缩字节数，永远停在完整帧边界。
    pub offset: u64,
    /// 最近一次有效注入快照的事件 seq；0 表示当前没有有效快照。
    pub active_seq: u64,
    /// 该快照里的记忆 id（行首 [32 位 hex] 表头提取）。
    pub active_ids: Vec<String>,
    /// 该快照的记忆段字节数。
    pub active_bytes: u64,
    /// 无法解析成 JSON 的行数（累计；它没有日期，因此不参与日账）。
    pub malformed_lines: u64,
    pub days: BTreeMap<String, DayAgg>,
    /// 记忆 id -> 本地日历日 -> 日账。
    pub memories: BTreeMap<String, BTreeMap<String, MemDayAgg>>,
}

/// 效果账本文档（memory/<profile>/effect/state.json）。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct EffectStateDoc {
    pub schema: u32,
    pub retention_days: u64,
    pub last_scanned_at_ms: u64,
    /// 最近一轮扫描里「还有未消费字节、却在帧头就停住」的文件数（损坏迹象）。
    pub decode_failures: u64,
    pub files: BTreeMap<String, FileEntry>,
}

fn effect_ctx() -> StateCtx {
    StateCtx {
        corrupt: |reason| {
            format!("记忆效果账本损坏（{reason}）。已从零重扫；它只是派生统计，不影响记忆库本身。")
        },
        kind: AppError::Memory,
    }
}

/// 把任意输入归一到三档中距离最近的一档；缺省为 7 天。
///
/// 面板只给 7/30/90 三个按钮，但命令参数来自 IPC，不能假设它合法。
pub fn normalize_range_days(requested: Option<u32>) -> u32 {
    let days = requested.unwrap_or(RANGE_CHOICES[0]);
    RANGE_CHOICES
        .iter()
        .copied()
        .min_by_key(|choice| choice.abs_diff(days))
        .unwrap_or(RANGE_CHOICES[0])
}

/// 面板入口：解析本壳默认实例、加载输入、增量扫描、派生报告。
///
/// 扫描可能解几百 MB 的会话帧，调用方必须把它放进 spawn_blocking（cmd.rs 的 blocking）。
pub fn report(range_days: Option<u32>) -> Result<report::EffectReport, AppError> {
    let namespace = Namespace::current();
    let state_path = paths::instance_memory_effect_file(
        &namespace.family,
        &namespace.instance_id,
        &namespace.profile,
    );
    let sessions_root =
        paths::instance_dsh_home(&namespace.family, &namespace.instance_id).join("sessions");
    // 记忆库读不出来时不挡报告：标题只是榜单的展示字段，账目本身仍可给。
    let memory_doc = match store::load(&namespace) {
        Ok(doc) => doc,
        Err(error) => {
            shell_events::record(
                "memory-effect",
                &format!("读取记忆库失败，效果榜单退回状态文案（{error}）"),
            );
            MemoryDoc::default()
        }
    };
    let jobs_doc = jobs::load(&namespace).unwrap_or_default();
    let now_ms = crate::shell::process::epoch_millis();
    report_at(
        &state_path,
        &sessions_root,
        &memory_doc,
        &jobs_doc,
        range_days,
        now_ms,
    )
}

/// 可测入口：路径与输入由调用方给出（单测与手工夹具不碰真实用户目录）。
pub(crate) fn report_at(
    state_path: &Path,
    sessions_root: &Path,
    memory_doc: &MemoryDoc,
    jobs_doc: &JobsDoc,
    range_days: Option<u32>,
    now_ms: u64,
) -> Result<report::EffectReport, AppError> {
    let range = normalize_range_days(range_days);
    let (mut doc, corrupted): (EffectStateDoc, _) = state::load_rebuildable(state_path);
    if let Some(reason) = corrupted {
        shell_events::record(
            "memory-effect-state",
            &format!(
                "记忆效果账本损坏，已从零重扫（{reason}）：{}",
                state_path.display()
            ),
        );
    }
    let schema_stale = doc.schema != STATE_SCHEMA || doc.retention_days != RETENTION_DAYS;
    doc.schema = STATE_SCHEMA;
    doc.retention_days = RETENTION_DAYS;
    let before = doc.files.clone();
    scan::scan_in(&mut doc, sessions_root, now_ms);
    doc.last_scanned_at_ms = now_ms;
    // 空转不落盘：扫描幂等，下一次改账时会把这一段重算回来。schema/保留期落后时
    // 即使没新事件也落一次，顺手把文档形状归一。
    if schema_stale || before != doc.files {
        state::save_timed_light(state_path, &doc, effect_ctx())?;
    }
    Ok(report::build(&doc, memory_doc, jobs_doc, now_ms, range))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::jobs::JobsDoc;
    use crate::memory::store::{MemoryDoc, MemoryRecord};
    use crate::usage::local::date_string_of_ms;
    use serde_json::json;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 固定时刻（2026-09-13 前后），让日期断言不依赖真实时钟。
    const BASE_MS: u64 = 1_790_000_000_000;
    const DAY: u64 = 86_400_000;

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "memory-effect-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 手工构造单帧 zstd 裸块流（Raw block，规范固定可静态编码）：magic + FHD + WD +
    /// 块头 + 原文。不依赖编码器依赖即可测多帧增量与截断重扫。
    fn raw_frame(content: &[u8]) -> Vec<u8> {
        assert!(content.len() < (1 << 17), "测试用裸块放不下");
        let mut frame = vec![0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x00];
        let header = ((content.len() as u32) << 3) | 1;
        frame.extend_from_slice(&header.to_le_bytes()[..3]);
        frame.extend_from_slice(content);
        frame
    }

    fn json_lines(events: &[serde_json::Value]) -> Vec<u8> {
        let mut content = Vec::new();
        for event in events {
            content.extend_from_slice(serde_json::to_string(event).expect("序列化事件").as_bytes());
            content.push(b'\n');
        }
        content
    }

    fn append_frame(path: &Path, events: &[serde_json::Value]) {
        let mut handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("打开会话文件");
        handle
            .write_all(&raw_frame(&json_lines(events)))
            .expect("追加帧");
    }

    fn session_path(root: &Path, workspace: &str, session: &str, file: &str) -> PathBuf {
        let dir = root.join(workspace).join(session);
        fs::create_dir_all(&dir).expect("建会话目录");
        dir.join(file)
    }

    fn snapshot_text(ids: &[String], marker: &str) -> String {
        let mut text = String::from("以下是本机保存的、与当前工作相关的记忆。\n");
        for id in ids {
            text.push_str(&format!("\n[{id}] {marker} 标题\n{marker} 正文\n"));
        }
        text
    }

    fn snapshot(seq: u64, time_ms: u64, ids: &[String], marker: &str) -> serde_json::Value {
        json!({
            "type": "user/message",
            "seq": seq,
            "time": time_ms,
            "data": {
                "role": "user",
                "content": [{"type": "text", "text": "runtime-context"}],
                "source": {
                    "kind": "runtime-context",
                    "form": "snapshot",
                    "sections": [
                        {"name": "working-directory:current", "text": "/tmp"},
                        {"name": "xlink.memory", "text": snapshot_text(ids, marker)}
                    ]
                }
            }
        })
    }

    fn request(seq: u64, time_ms: u64) -> serde_json::Value {
        json!({
            "type": "assistant/message",
            "seq": seq,
            "time": time_ms,
            "data": {
                "turn": 1,
                "step": 1,
                "message": {"role": "assistant", "source": {"kind": "model", "provider": "p", "model": "m"}},
                "usage": {"inputTokens": 10, "outputTokens": 2, "totalTokens": 12, "cacheReadTokens": 3}
            }
        })
    }

    fn tool_call(seq: u64, time_ms: u64, name: &str, arguments: &str) -> serde_json::Value {
        json!({
            "type": "tool/call",
            "seq": seq,
            "time": time_ms,
            "data": {"turn": 1, "step": 1, "callId": "c", "name": name, "arguments": arguments}
        })
    }

    fn tool_error(seq: u64, time_ms: u64) -> serde_json::Value {
        json!({
            "type": "tool/result",
            "seq": seq,
            "time": time_ms,
            "data": {"turn": 1, "step": 1, "message": {"role": "tool", "isError": true, "content": [{"type": "text", "text": "boom"}]}}
        })
    }

    fn compaction_prune(seq: u64, time_ms: u64, shadowed: u64) -> serde_json::Value {
        json!({
            "type": "compaction/prune",
            "seq": seq,
            "time": time_ms,
            "data": {"shadowedSeqs": [shadowed], "shadowedRange": {"start": shadowed, "end": shadowed}, "shadowedTokenCount": 1}
        })
    }

    fn no_jobs() -> JobsDoc {
        JobsDoc::new()
    }

    fn report_at_fixture(
        sessions_root: &Path,
        state_path: &Path,
        memory: &MemoryDoc,
        now_ms: u64,
        range: Option<u32>,
    ) -> report::EffectReport {
        report_at(state_path, sessions_root, memory, &no_jobs(), range, now_ms)
            .expect("效果报告应成功")
    }

    #[test]
    fn multiframe_fixture_parses_injection_requests_and_tools() {
        let root = scratch("parse-sessions");
        let state_path = scratch("parse-state").join("state.json");
        let file = session_path(&root, "ws", "session-a", "session.v4.jsonl.zstd");
        let id_a = "a".repeat(32);
        let id_b = "b".repeat(32);
        append_frame(
            &file,
            &[snapshot(1, BASE_MS, &[id_a.clone(), id_b.clone()], "PARSE")],
        );
        append_frame(
            &file,
            &[
                request(2, BASE_MS),
                tool_call(3, BASE_MS, "memory_propose", "{}"),
                tool_call(4, BASE_MS, "bash", "{}"),
                tool_error(5, BASE_MS),
            ],
        );
        let report = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));

        assert_eq!(report.kpi.injection_snapshots, 1);
        assert_eq!(
            report.kpi.injected_bytes,
            snapshot_text(&[id_a.clone(), id_b.clone()], "PARSE").len() as u64
        );
        assert_eq!(report.kpi.requests, 1);
        assert_eq!(report.kpi.memory_requests, 1);
        assert_eq!(report.kpi.tool_calls, 2);
        assert_eq!(report.kpi.tool_errors, 1);
        assert_eq!(report.kpi.memory_propose_calls, 1);
        assert_eq!(report.funnel.memories_injected, 2);
        assert_eq!(report.coverage.sessions_total, 1);
        assert_eq!(report.coverage.sessions_active, 1);
        assert_eq!(report.coverage.sessions_with_injection, 1);
        assert_eq!(report.coverage.format_versions["v4"], 1);
        assert_eq!(report.coverage.unknown_event_ratio, 0.0);
        let today = date_string_of_ms(BASE_MS);
        let day = report
            .trend
            .iter()
            .find(|row| row.date == today)
            .expect("今天应有趋势行");
        assert_eq!(day.injection_snapshots, 1);
        assert_eq!(day.requests, 1);
        assert_eq!(day.memory_requests, 1);
        assert_eq!(report.leaderboard.len(), 2);
        assert_eq!(report.leaderboard[0].title, "已删除或不活跃");
    }

    #[test]
    fn shadowed_snapshot_pauses_memory_requests_until_next_injection() {
        let root = scratch("shadow-sessions");
        let state_path = scratch("shadow-state").join("state.json");
        let file = session_path(&root, "ws", "session-b", "session.v4.jsonl.zstd");
        let id = "c".repeat(32);
        append_frame(
            &file,
            &[
                snapshot(1, BASE_MS, std::slice::from_ref(&id), "SHADOW"),
                request(2, BASE_MS),
            ],
        );
        append_frame(
            &file,
            &[compaction_prune(3, BASE_MS, 1), request(4, BASE_MS)],
        );
        append_frame(
            &file,
            &[
                snapshot(5, BASE_MS, std::slice::from_ref(&id), "SHADOW-2"),
                request(6, BASE_MS),
            ],
        );
        let report = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));
        assert_eq!(report.kpi.injection_snapshots, 2);
        assert_eq!(report.kpi.requests, 3);
        assert_eq!(
            report.kpi.memory_requests, 2,
            "被 compaction/prune 遮蔽后的请求不算携带记忆；重新注入后恢复"
        );
        assert_eq!(report.funnel.memories_injected, 1);
    }

    #[test]
    fn incremental_append_and_restart_keep_the_active_snapshot() {
        let root = scratch("incremental-sessions");
        let state_path = scratch("incremental-state").join("state.json");
        let file = session_path(&root, "ws", "session-c", "session.v4.jsonl.zstd");
        let id = "d".repeat(32);
        append_frame(
            &file,
            &[
                snapshot(1, BASE_MS, std::slice::from_ref(&id), "INC"),
                request(2, BASE_MS),
            ],
        );
        let first = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));
        assert_eq!(first.kpi.requests, 1);
        assert_eq!(first.kpi.memory_requests, 1);

        // 追加一帧只有请求、没有新快照：续扫必须靠持久化的 active 快照继续计数。
        append_frame(&file, &[request(3, BASE_MS + 1_000)]);
        // report_at 每次从磁盘 load state（等价于重启后的续扫），计数只增不重。
        let second = report_at_fixture(
            &root,
            &state_path,
            &MemoryDoc::default(),
            BASE_MS + 2_000,
            Some(7),
        );
        assert_eq!(second.kpi.requests, 2, "只增不重");
        assert_eq!(second.kpi.memory_requests, 2, "active 快照必须跨重启仍生效");

        append_frame(&file, &[request(4, BASE_MS + 3_000)]);
        let third = report_at_fixture(
            &root,
            &state_path,
            &MemoryDoc::default(),
            BASE_MS + 4_000,
            Some(7),
        );
        assert_eq!(third.kpi.requests, 3);
        assert_eq!(third.kpi.memory_requests, 3);
    }

    #[test]
    fn newest_variant_wins_and_truncation_recounts_without_double_count() {
        let root = scratch("variant-sessions");
        let state_path = scratch("variant-state").join("state.json");
        let dir = root.join("ws").join("session-d");
        fs::create_dir_all(&dir).expect("建会话目录");
        let old_id = "e".repeat(32);
        let new_id = "f".repeat(32);
        fs::write(
            dir.join("session.jsonl.zstd"),
            raw_frame(&json_lines(&[
                snapshot(1, BASE_MS, std::slice::from_ref(&old_id), "OLD"),
                request(2, BASE_MS),
            ])),
        )
        .expect("写 v0");
        fs::write(
            dir.join("session.v4.jsonl.zstd"),
            raw_frame(&json_lines(&[
                snapshot(1, BASE_MS, std::slice::from_ref(&new_id), "NEW"),
                request(2, BASE_MS),
                request(3, BASE_MS),
            ])),
        )
        .expect("写 v4");
        let first = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));
        assert_eq!(first.kpi.injection_snapshots, 1, "只认版本号最高的一代");
        assert_eq!(first.kpi.requests, 2);
        assert_eq!(first.coverage.sessions_total, 1);
        assert_eq!(first.leaderboard.len(), 1);
        assert_eq!(first.leaderboard[0].id, new_id);

        // 截断重写：文件变小 => 从 0 重扫，不双计。
        fs::write(
            dir.join("session.v4.jsonl.zstd"),
            raw_frame(&json_lines(&[request(1, BASE_MS)])),
        )
        .expect("截断重写");
        let shrunk = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));
        assert_eq!(shrunk.kpi.requests, 1, "截断后只按新内容重算");
        assert_eq!(shrunk.kpi.injection_snapshots, 0);
        assert!(shrunk.leaderboard.is_empty());
        assert_eq!(shrunk.coverage.sessions_total, 1);
    }

    #[test]
    fn corrupt_state_is_rebuilt_without_panicking() {
        let home = scratch("corrupt-home");
        // shell_events 落盘会经 paths 解析 home，必须把它指到临时目录。
        let _scoped_home = crate::tests::scoped_xlink_home(&home);
        let root = scratch("corrupt-sessions");
        let state_path = scratch("corrupt-state").join("state.json");
        let file = session_path(&root, "ws", "session-e", "session.v4.jsonl.zstd");
        let id = "1".repeat(32);
        append_frame(
            &file,
            &[
                snapshot(1, BASE_MS, std::slice::from_ref(&id), "CORRUPT"),
                request(2, BASE_MS),
            ],
        );
        fs::write(&state_path, b"{\"schema\": 1, \"fil").expect("写坏账本");
        let report = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));
        assert_eq!(report.kpi.injection_snapshots, 1, "坏账本必须从零重扫");
        let (rebuilt, corrupted) = state::load_rebuildable::<EffectStateDoc>(&state_path);
        assert!(corrupted.is_none(), "重扫后账本必须可读");
        assert_eq!(rebuilt.files.len(), 1);
        assert!(rebuilt.last_scanned_at_ms > 0);
    }

    #[test]
    fn scan_skips_events_outside_retention_and_report_keeps_window() {
        let root = scratch("window-sessions");
        let state_path = scratch("window-state").join("state.json");
        let file = session_path(&root, "ws", "session-f", "session.v4.jsonl.zstd");
        let id = "2".repeat(32);
        append_frame(
            &file,
            &[
                snapshot(1, BASE_MS - 100 * DAY, std::slice::from_ref(&id), "TOO-OLD"),
                request(2, BASE_MS - 100 * DAY),
            ],
        );
        append_frame(
            &file,
            &[
                snapshot(3, BASE_MS - 10 * DAY, std::slice::from_ref(&id), "MID"),
                request(4, BASE_MS - 10 * DAY),
            ],
        );
        let wide = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(90));
        assert_eq!(wide.kpi.requests, 1, "90 天窗口含 10 天前，不含 100 天前");
        assert_eq!(wide.kpi.memory_requests, 1);
        let narrow = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, Some(7));
        assert_eq!(narrow.kpi.requests, 0, "7 天窗口不含 10 天前");

        assert_eq!(normalize_range_days(None), 7);
        assert_eq!(normalize_range_days(Some(1)), 7);
        assert_eq!(normalize_range_days(Some(45)), 30);
        assert_eq!(normalize_range_days(Some(365)), 90);
        assert_eq!(normalize_range_days(Some(30)), 30);
    }

    #[test]
    fn serialized_state_contains_no_titles_or_bodies() {
        let root = scratch("privacy-sessions");
        let state_path = scratch("privacy-state").join("state.json");
        let file = session_path(&root, "ws", "session-g", "session.v4.jsonl.zstd");
        let id = "3".repeat(32);
        append_frame(
            &file,
            &[
                snapshot(1, BASE_MS, std::slice::from_ref(&id), "PRIVATE-MARKER"),
                request(2, BASE_MS),
            ],
        );
        let memory = MemoryDoc {
            memories: vec![MemoryRecord {
                id: id.clone(),
                title: "TITLE-SECRET-MARKER".to_string(),
                body: "BODY-SECRET-MARKER".to_string(),
                ..MemoryRecord::default()
            }],
            ..MemoryDoc::default()
        };
        let report = report_at_fixture(&root, &state_path, &memory, BASE_MS, Some(7));
        let state_text = fs::read_to_string(&state_path).expect("读回账本");
        assert!(state_text.contains(&id), "账本必须保留 id");
        assert!(
            !state_text.contains("PRIVATE-MARKER"),
            "快照段文本不得进账本"
        );
        assert!(
            !state_text.contains("TITLE-SECRET-MARKER"),
            "标题不得进账本"
        );
        assert!(!state_text.contains("BODY-SECRET-MARKER"), "正文不得进账本");
        assert_eq!(
            report.leaderboard[0].title, "TITLE-SECRET-MARKER",
            "榜单标题来自 memory.json（工作台可见），不来自扫描状态"
        );
    }

    #[test]
    fn empty_fixture_still_returns_window_and_all_contract_fields() {
        let root = scratch("empty-sessions");
        let state_path = scratch("empty-state").join("state.json");
        let report = report_at_fixture(&root, &state_path, &MemoryDoc::default(), BASE_MS, None);
        assert_eq!(report.range_days, 7);
        assert_eq!(report.trend.len(), 7);
        assert_eq!(report.window.from.len(), 10);
        assert_eq!(report.window.to.len(), 10);
        assert!(!report.generated_at.is_empty());

        let value = serde_json::to_value(&report).expect("报告必须可序列化");
        for key in [
            "generated_at",
            "range_days",
            "window",
            "coverage",
            "kpi",
            "funnel",
            "trend",
            "leaderboard",
            "cost",
            "store",
            "health",
        ] {
            assert!(value.get(key).is_some(), "报告缺字段 {key}");
        }
        for key in [
            "sessions_total",
            "sessions_active",
            "sessions_with_injection",
            "decode_failures",
            "unknown_event_ratio",
            "format_versions",
        ] {
            assert!(
                value["coverage"].get(key).is_some(),
                "coverage 缺字段 {key}"
            );
        }
        for key in [
            "injection_snapshots",
            "injected_bytes",
            "injected_tokens_est",
            "requests",
            "memory_requests",
            "memory_request_ratio",
            "memory_read_calls",
            "memory_search_calls",
            "memory_propose_calls",
            "tool_calls",
            "tool_errors",
        ] {
            assert!(value["kpi"].get(key).is_some(), "kpi 缺字段 {key}");
        }
        for key in [
            "injected_bytes",
            "injected_tokens_est",
            "extraction_jobs_done",
            "extraction_jobs_failed",
            "extraction_tokens_upper_est",
        ] {
            assert!(value["cost"].get(key).is_some(), "cost 缺字段 {key}");
        }
        for key in [
            "memories",
            "experiences",
            "handoffs",
            "candidates_pending",
            "jobs_pending",
            "jobs_running",
            "jobs_done",
            "jobs_failed",
        ] {
            assert!(value["store"].get(key).is_some(), "store 缺字段 {key}");
        }
        assert!(value["health"].get("has_enough_requests").is_some());
        assert!(value["health"].get("has_enough_injections").is_some());
        assert_eq!(value["trend"].as_array().map(Vec::len), Some(7));
    }

    /// 手工夹具（#[ignore]，不碰真实用户目录）：对 DSH_XLINK_EFFECT_PROBE_ROOT 指向的
    /// 会话日志【拷贝】跑一遍完整报告并打印关键字段，供 Lead 与独立参考值对拍。
    #[test]
    #[ignore = "手工夹具：DSH_XLINK_EFFECT_PROBE_ROOT 指向会话日志拷贝（只读）"]
    fn manual_effect_report_over_copied_sessions() {
        let root = std::env::var("DSH_XLINK_EFFECT_PROBE_ROOT")
            .expect("需要 DSH_XLINK_EFFECT_PROBE_ROOT（sessions 根目录的拷贝）");
        let sessions_root = PathBuf::from(&root);
        assert!(
            sessions_root.is_dir(),
            "DSH_XLINK_EFFECT_PROBE_ROOT 不是目录：{root}"
        );
        let state_dir = scratch("probe-state");
        let state_path = state_dir.join("state.json");
        let now_ms = crate::shell::process::epoch_millis();
        let report = report_at(
            &state_path,
            &sessions_root,
            &MemoryDoc::default(),
            &no_jobs(),
            Some(90),
            now_ms,
        )
        .expect("扫描拷贝应成功");

        let (state_doc, _) = state::load_rebuildable::<EffectStateDoc>(&state_path);
        let mut injected_ids = std::collections::BTreeSet::new();
        let mut read_ids = std::collections::BTreeSet::new();
        for entry in state_doc.files.values() {
            for (id, per_day) in &entry.memories {
                let injections: u64 = per_day.values().map(|bucket| bucket.injections).sum();
                let reads: u64 = per_day.values().map(|bucket| bucket.reads).sum();
                if injections > 0 {
                    injected_ids.insert(id.clone());
                }
                if reads > 0 {
                    read_ids.insert(id.clone());
                }
            }
        }
        let report_value = serde_json::to_value(&report).expect("报告必须可序列化");
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "report": report_value,
                "distinct_injected_ids": injected_ids.len(),
                "distinct_read_ids": read_ids.len(),
            }))
            .expect("打印 JSON")
        );
        fs::remove_dir_all(&state_dir).ok();
    }
}
