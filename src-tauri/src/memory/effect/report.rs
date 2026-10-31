//! 从效果账目 + 记忆库 + 提取任务表派生面板视图（EffectReport）。
//!
//! 所有计数、比率、趋势补零与榜单排序都在这里算；前端只负责格式化与展示。
//! 窗口过滤只对日账生效：状态文件保留 90 天，7/30/90 的差别在这里收口。
//! 注意所有含 ratio 的字段都是 0-100 的百分数（已乘 100），前端直接显示。

use std::collections::BTreeMap;

use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime};

use super::{DayAgg, EffectStateDoc};
use crate::memory::jobs::{self, JobsDoc};
use crate::memory::store::MemoryDoc;
use crate::usage::local::date_string_of_ms;

const DAY_MS: u64 = 86_400_000;

/// 注入 token 估算系数：字节 / 3.07 = token（2026-10-31 的离线标定，见设计稿 8.2）。
/// 只能给估算值，面板必须标注为估算。
const BYTES_PER_TOKEN: f64 = 3.07;

/// 一次提取任务尝试的输入 token 上界：24 KiB 切片 / 3.07 约 8.6k。
/// jobs.json 目前不记 usage，P0 只给上界，P1 再补账。
const EXTRACTION_ATTEMPT_TOKENS: u64 = 8_600;

/// 样本量下限：分母不足时面板只显示计数、不显示百分比（设计稿 3.3）。
const MIN_SAMPLE: u64 = 30;

/// 榜单最多返回多少条（UI 只展示 Top 8，多留一些给下钻）。
const LEADERBOARD_LIMIT: usize = 20;

/// 面板效果页签的主数据。
#[derive(Debug, Serialize)]
pub struct EffectReport {
    pub generated_at: String,
    pub range_days: u32,
    pub window: WindowSpan,
    pub coverage: Coverage,
    pub kpi: Kpi,
    pub funnel: Funnel,
    pub trend: Vec<TrendDay>,
    pub leaderboard: Vec<LeaderboardRow>,
    pub cost: Cost,
    pub store: StoreCounts,
    pub health: Health,
}

#[derive(Debug, Serialize)]
pub struct WindowSpan {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub sessions_total: u64,
    pub sessions_active: u64,
    pub sessions_with_injection: u64,
    /// 帧头就解不动、还有未消费字节的文件数（数据健康用；正常为 0）。
    pub decode_failures: u64,
    /// 未知事件占比（0-100）：类型不在已知兼容清单里的事件 / 事件总数。
    pub unknown_event_ratio: f64,
    /// 会话文件格式版本分布，如 v4: 57。按账目键里的文件名统计。
    pub format_versions: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
pub struct Kpi {
    pub injection_snapshots: u64,
    pub injected_bytes: u64,
    pub injected_tokens_est: u64,
    pub requests: u64,
    pub memory_requests: u64,
    /// 携带记忆请求率（0-100）。
    pub memory_request_ratio: f64,
    pub memory_read_calls: u64,
    pub memory_search_calls: u64,
    pub memory_propose_calls: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
}

#[derive(Debug, Serialize)]
pub struct Funnel {
    /// 窗口内新建的候选。
    pub candidates_created: u64,
    /// 窗口内新建的记忆。
    pub memories_created: u64,
    /// 窗口内被注入过的独立记忆条数。
    pub memories_injected: u64,
    /// 窗口内被模型主动 memory_read 读过的独立记忆条数。
    pub memories_read: u64,
}

#[derive(Debug, Serialize)]
pub struct TrendDay {
    pub date: String,
    pub injection_snapshots: u64,
    pub injected_bytes: u64,
    pub memory_requests: u64,
    pub requests: u64,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardRow {
    pub id: String,
    /// 标题来自 memory.json；已删除 / 读不到的 id 给状态文案，不给正文。
    pub title: String,
    pub scope: String,
    pub verification: String,
    pub injection_snapshots: u64,
    pub memory_requests: u64,
    pub sessions: u64,
}

#[derive(Debug, Serialize)]
pub struct Cost {
    pub injected_bytes: u64,
    pub injected_tokens_est: u64,
    pub extraction_jobs_done: u64,
    pub extraction_jobs_failed: u64,
    /// 提取 token 上界（估算）：任务尝试次数 x 每次输入上界。
    pub extraction_tokens_upper_est: u64,
}

#[derive(Debug, Serialize)]
pub struct StoreCounts {
    pub memories: u64,
    pub experiences: u64,
    pub handoffs: u64,
    pub candidates_pending: u64,
    pub jobs_pending: u64,
    pub jobs_running: u64,
    pub jobs_done: u64,
    pub jobs_failed: u64,
}

#[derive(Debug, Serialize)]
pub struct Health {
    /// 请求样本是否够（>= 30）。
    pub has_enough_requests: bool,
    /// 注入样本是否够（>= 30）。
    pub has_enough_injections: bool,
}

/// 单条记忆在窗口内的汇总（榜单行与漏斗共用）。
#[derive(Default)]
struct MemoryTotal {
    injections: u64,
    exposures: u64,
    reads: u64,
    sessions: u64,
}

/// 派生报告。state 是已经扫过一轮的账目；memory / jobs 是权威库与任务表。
pub(crate) fn build(
    state: &EffectStateDoc,
    memory: &MemoryDoc,
    jobs: &JobsDoc,
    now_ms: u64,
    range_days: u32,
) -> EffectReport {
    let to = date_string_of_ms(now_ms);
    let from = date_string_of_ms(now_ms.saturating_sub((range_days as u64 - 1) * DAY_MS));
    let mut by_day: BTreeMap<String, DayAgg> = BTreeMap::new();
    let mut versions: BTreeMap<String, u64> = BTreeMap::new();
    let mut memory_totals: BTreeMap<String, MemoryTotal> = BTreeMap::new();
    let mut sessions_active = 0u64;
    let mut sessions_with_injection = 0u64;

    for (key, entry) in &state.files {
        *versions.entry(file_version(key)).or_default() += 1;
        let mut file_requests = 0u64;
        let mut file_injections = 0u64;
        for (day, bucket) in &entry.days {
            if !in_window(day, &from, &to) {
                continue;
            }
            by_day.entry(day.clone()).or_default().merge(bucket);
            file_requests += bucket.requests;
            file_injections += bucket.injection_snapshots;
        }
        if file_requests > 0 {
            sessions_active += 1;
        }
        if file_injections > 0 {
            sessions_with_injection += 1;
        }
        for (id, per_day) in &entry.memories {
            let mut total = MemoryTotal::default();
            for (day, bucket) in per_day {
                if !in_window(day, &from, &to) {
                    continue;
                }
                total.injections += bucket.injections;
                total.exposures += bucket.exposures;
                total.reads += bucket.reads;
            }
            if total.injections == 0 && total.exposures == 0 && total.reads == 0 {
                continue;
            }
            if total.injections > 0 {
                // 一个文件就是一个会话；本文件注入过就记一个触达会话。
                total.sessions = 1;
            }
            let slot = memory_totals.entry(id.clone()).or_default();
            slot.injections += total.injections;
            slot.exposures += total.exposures;
            slot.reads += total.reads;
            slot.sessions += total.sessions;
        }
    }

    let mut totals = DayAgg::default();
    for bucket in by_day.values() {
        totals.merge(bucket);
    }
    let injected_tokens_est = tokens_est(totals.injected_bytes);
    let jobs_summary = jobs::summary(jobs);
    let extraction_attempts: u64 = jobs.jobs.iter().map(|job| job.attempts as u64).sum();

    let mut leaderboard: Vec<LeaderboardRow> = memory_totals
        .iter()
        .map(|(id, total)| {
            let record = memory.memories.iter().find(|record| &record.id == id);
            LeaderboardRow {
                id: id.clone(),
                title: record
                    .map(|record| record.title.clone())
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| "已删除或不活跃".to_string()),
                scope: record
                    .map(|record| record.scope.clone())
                    .unwrap_or_default(),
                verification: record
                    .map(|record| record.verification.clone())
                    .unwrap_or_default(),
                injection_snapshots: total.injections,
                memory_requests: total.exposures,
                sessions: total.sessions,
            }
        })
        .collect();
    leaderboard.sort_by(|left, right| {
        right
            .injection_snapshots
            .cmp(&left.injection_snapshots)
            .then_with(|| right.memory_requests.cmp(&left.memory_requests))
            .then_with(|| left.id.cmp(&right.id))
    });
    leaderboard.truncate(LEADERBOARD_LIMIT);

    EffectReport {
        generated_at: iso_utc(now_ms),
        range_days,
        window: WindowSpan {
            from: from.clone(),
            to: to.clone(),
        },
        coverage: Coverage {
            sessions_total: state.files.len() as u64,
            sessions_active,
            sessions_with_injection,
            decode_failures: state.decode_failures + malformed_lines(state),
            unknown_event_ratio: percent(totals.unknown_events, totals.events),
            format_versions: versions,
        },
        kpi: Kpi {
            injection_snapshots: totals.injection_snapshots,
            injected_bytes: totals.injected_bytes,
            injected_tokens_est,
            requests: totals.requests,
            memory_requests: totals.memory_requests,
            memory_request_ratio: percent(totals.memory_requests, totals.requests),
            memory_read_calls: totals.memory_read_calls,
            memory_search_calls: totals.memory_search_calls,
            memory_propose_calls: totals.memory_propose_calls,
            tool_calls: totals.tool_calls,
            tool_errors: totals.tool_errors,
        },
        funnel: Funnel {
            candidates_created: memory
                .candidates
                .iter()
                .filter(|candidate| date_in_window(&candidate.created_at, &from, &to))
                .count() as u64,
            memories_created: memory
                .memories
                .iter()
                .filter(|record| date_in_window(&record.created_at, &from, &to))
                .count() as u64,
            memories_injected: memory_totals
                .values()
                .filter(|total| total.injections > 0)
                .count() as u64,
            memories_read: memory_totals
                .values()
                .filter(|total| total.reads > 0)
                .count() as u64,
        },
        trend: trend_days(by_day, &from, &to),
        leaderboard,
        cost: Cost {
            injected_bytes: totals.injected_bytes,
            injected_tokens_est,
            extraction_jobs_done: jobs_summary.done as u64,
            extraction_jobs_failed: jobs_summary.failed as u64,
            extraction_tokens_upper_est: extraction_attempts
                .saturating_mul(EXTRACTION_ATTEMPT_TOKENS),
        },
        store: StoreCounts {
            memories: memory.memories.len() as u64,
            experiences: memory.experiences.len() as u64,
            handoffs: memory.handoffs.len() as u64,
            candidates_pending: memory
                .candidates
                .iter()
                .filter(|candidate| candidate.state == crate::memory::store::CANDIDATE_PENDING)
                .count() as u64,
            jobs_pending: jobs_summary.pending as u64,
            jobs_running: jobs_summary.running as u64,
            jobs_done: jobs_summary.done as u64,
            jobs_failed: jobs_summary.failed as u64,
        },
        health: Health {
            has_enough_requests: totals.requests >= MIN_SAMPLE,
            has_enough_injections: totals.injection_snapshots >= MIN_SAMPLE,
        },
    }
}

/// 按日推进补齐窗口内的零日（趋势图不能缺日期）；晚于今天的异常日期追加到末尾。
fn trend_days(by_day: BTreeMap<String, DayAgg>, from: &str, to: &str) -> Vec<TrendDay> {
    const DAY_FORMAT: &[time::format_description::FormatItem<'static>] =
        format_description!("[year]-[month]-[day]");
    let mut out = Vec::new();
    let mut cursor = Date::parse(from, DAY_FORMAT).ok();
    let end = Date::parse(to, DAY_FORMAT).ok();
    let mut remaining = by_day;
    while let Some(date) = cursor {
        let key = date.format(DAY_FORMAT).unwrap_or_default();
        let bucket = remaining.remove(&key).unwrap_or_default();
        out.push(TrendDay {
            date: key,
            injection_snapshots: bucket.injection_snapshots,
            injected_bytes: bucket.injected_bytes,
            memory_requests: bucket.memory_requests,
            requests: bucket.requests,
        });
        if Some(date) == end {
            break;
        }
        match date.next_day() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    for (date, bucket) in remaining {
        out.push(TrendDay {
            date,
            injection_snapshots: bucket.injection_snapshots,
            injected_bytes: bucket.injected_bytes,
            memory_requests: bucket.memory_requests,
            requests: bucket.requests,
        });
    }
    out
}

/// 字节到 token 估算（四舍五入）。
fn tokens_est(bytes: u64) -> u64 {
    if bytes == 0 {
        return 0;
    }
    (bytes as f64 / BYTES_PER_TOKEN).round() as u64
}

/// 比率（0-100，保留两位小数）。分母为 0 时给 0，而不是 NaN。
fn percent(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    ((numerator as f64) * 10_000.0 / (denominator as f64)).round() / 100.0
}

fn in_window(day: &str, from: &str, to: &str) -> bool {
    day >= from && day <= to
}

/// 账目键里的文件名 -> 格式版本（session.v4.jsonl.zstd => v4）。
fn file_version(key: &str) -> String {
    let name = key.rsplit('/').next().unwrap_or(key);
    let Some(rest) = name.strip_prefix("session") else {
        return "unknown".to_string();
    };
    if rest == ".jsonl.zstd" {
        return "v0".to_string();
    }
    if let Some(rest) = rest.strip_prefix(".v") {
        if let Some((digits, tail)) = rest.split_once('.') {
            if tail == "jsonl.zstd"
                && !digits.is_empty()
                && digits.bytes().all(|byte| byte.is_ascii_digit())
            {
                return format!("v{digits}");
            }
        }
    }
    "unknown".to_string()
}

/// RFC3339 里取前 10 位日期做窗口比较；空值自然落在窗口外。
fn date_in_window(rfc3339: &str, from: &str, to: &str) -> bool {
    let day = rfc3339.get(..10).unwrap_or_default();
    !day.is_empty() && in_window(day, from, to)
}

/// 单文件解析失败行数的上界；正常会话里为 0。
fn malformed_lines(state: &EffectStateDoc) -> u64 {
    state
        .files
        .values()
        .map(|entry| entry.malformed_lines)
        .sum()
}

/// epoch 毫秒 -> RFC3339（UTC）。
fn iso_utc(ms: u64) -> String {
    OffsetDateTime::from_unix_timestamp((ms / 1000) as i64)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::jobs::{Job, JobKey, JobsDoc};
    use crate::memory::store::{MemoryCandidate, MemoryDoc, MemoryRecord};

    fn record(id: &str, title: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            title: title.to_string(),
            scope: "project".to_string(),
            verification: "verified".to_string(),
            created_at: "2026-10-20T00:00:00Z".to_string(),
            ..MemoryRecord::default()
        }
    }

    fn state_with_day(day: &str) -> EffectStateDoc {
        let mut doc = EffectStateDoc::default();
        let mut entry = super::super::FileEntry::default();
        let bucket = entry.days.entry(day.to_string()).or_default();
        bucket.injection_snapshots = 2;
        bucket.injected_bytes = 614;
        bucket.requests = 4;
        bucket.memory_requests = 3;
        bucket.tool_calls = 5;
        bucket.memory_read_calls = 1;
        entry
            .memories
            .entry("d".repeat(32))
            .or_default()
            .entry(day.to_string())
            .or_default()
            .injections = 2;
        doc.files
            .insert("ws/session-x/session.v4.jsonl.zstd".to_string(), entry);
        doc
    }

    #[test]
    fn window_filter_and_trend_zero_fill() {
        let now = 1_790_000_000_000u64;
        let today = date_string_of_ms(now);
        let three_days_ago = date_string_of_ms(now - 3 * DAY_MS);
        let old = date_string_of_ms(now - 40 * DAY_MS);
        let mut doc = state_with_day(&today);
        for (key, entry) in state_with_day(&three_days_ago).files {
            doc.files.insert(format!("other/{key}"), entry);
        }
        for (key, entry) in state_with_day(&old).files {
            doc.files.insert(format!("old/{key}"), entry);
        }

        let report = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), now, 7);
        assert_eq!(report.window.to, today);
        assert_eq!(report.window.from, date_string_of_ms(now - 6 * DAY_MS));
        assert_eq!(report.trend.len(), 7, "7 天窗口恒补满 7 个日期");
        assert_eq!(report.kpi.injection_snapshots, 4, "窗口外的 40 天前不计");
        assert_eq!(report.kpi.requests, 8);
        assert_eq!(report.kpi.memory_requests, 6);
        assert_eq!(report.kpi.memory_request_ratio, 75.0);
        assert_eq!(report.cost.injected_tokens_est, tokens_est(1228));
        assert_eq!(report.coverage.format_versions["v4"], 3);
        assert_eq!(report.funnel.memories_injected, 1);

        let wide = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), now, 90);
        assert_eq!(wide.trend.len(), 90);
        assert_eq!(wide.kpi.injection_snapshots, 6, "90 天窗口含 40 天前");

        let narrow = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), now, 30);
        assert_eq!(narrow.trend.len(), 30);
        assert_eq!(narrow.kpi.injection_snapshots, 4);
    }

    #[test]
    fn leaderboard_uses_memory_titles_and_marks_deleted_ids() {
        let now = 1_790_000_000_000u64;
        let today = date_string_of_ms(now);
        let mut doc = state_with_day(&today);
        let mut entry = doc
            .files
            .remove("ws/session-x/session.v4.jsonl.zstd")
            .unwrap();
        entry
            .memories
            .entry("e".repeat(32))
            .or_default()
            .entry(today.clone())
            .or_default()
            .injections = 9;
        doc.files
            .insert("ws/session-y/session.v4.jsonl.zstd".to_string(), entry);

        let memory = MemoryDoc {
            memories: vec![record(&"d".repeat(32), "开发约束")],
            candidates: vec![MemoryCandidate {
                state: "pending".to_string(),
                created_at: "2026-10-20T00:00:00Z".to_string(),
                ..MemoryCandidate::default()
            }],
            ..MemoryDoc::default()
        };
        let report = build(&doc, &memory, &JobsDoc::new(), now, 7);
        assert_eq!(report.leaderboard.len(), 2);
        let known = report
            .leaderboard
            .iter()
            .find(|row| row.id == "d".repeat(32))
            .expect("记忆库里的 id 应保留标题");
        assert_eq!(known.title, "开发约束");
        assert_eq!(known.injection_snapshots, 2);
        assert_eq!(known.sessions, 1);
        let deleted = report
            .leaderboard
            .iter()
            .find(|row| row.id == "e".repeat(32))
            .expect("账本里不在库中的 id 也要上榜");
        assert_eq!(deleted.title, "已删除或不活跃");
        assert_eq!(deleted.injection_snapshots, 9);
        assert_eq!(report.store.memories, 1);
        assert_eq!(report.store.candidates_pending, 1);
    }

    #[test]
    fn ratio_fields_are_percentages_and_guard_zero_denominators() {
        let now = 1_790_000_000_000u64;
        let empty = build(
            &EffectStateDoc::default(),
            &MemoryDoc::default(),
            &JobsDoc::new(),
            now,
            7,
        );
        assert_eq!(empty.kpi.memory_request_ratio, 0.0);
        assert_eq!(empty.coverage.unknown_event_ratio, 0.0);
        assert!(!empty.health.has_enough_requests);
        assert_eq!(empty.trend.len(), 7);
        assert!(!empty.window.from.is_empty());
        assert!(!empty.window.to.is_empty());

        let doc = state_with_day(&date_string_of_ms(now));
        let report = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), now, 7);
        assert_eq!(
            report.kpi.memory_request_ratio, 75.0,
            "0-100 百分数而非 0-1"
        );
    }

    #[test]
    fn extraction_upper_estimate_uses_attempts_not_done_count() {
        let now = 1_790_000_000_000u64;
        let mut jobs = JobsDoc::new();
        let key = JobKey {
            session: "s".to_string(),
            cursor: 0,
            extractor: "e".to_string(),
            from_cursor: 0,
        };
        let mut job = Job {
            key,
            state: "failed".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            attempts: 3,
            lease: None,
            error: String::new(),
        };
        jobs.jobs.push(job.clone());
        job.state = "done".to_string();
        jobs.jobs.push(job);
        let report = build(
            &EffectStateDoc::default(),
            &MemoryDoc::default(),
            &jobs,
            now,
            7,
        );
        assert_eq!(report.cost.extraction_jobs_done, 1);
        assert_eq!(report.cost.extraction_jobs_failed, 1);
        assert_eq!(
            report.cost.extraction_tokens_upper_est,
            6 * 8_600,
            "上界按尝试次数算，不是按任务数"
        );
    }

    #[test]
    fn format_versions_read_the_variant_in_the_key() {
        assert_eq!(file_version("ws/s/session.v4.jsonl.zstd"), "v4");
        assert_eq!(file_version("ws/s/session.jsonl.zstd"), "v0");
        assert_eq!(file_version("ws/s/session.lock"), "unknown");
    }
}
