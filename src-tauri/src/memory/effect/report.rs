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

use super::{ledger::LedgerNote, DayAgg, EffectStateDoc, GroupAgg};
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
    /// P1：同会话配对的 L2 关联读数（不是因果）。
    pub comparison: Comparison,
    /// P1：提取来源记忆的质量概览（来自现有权威数据，无新埋点）。
    pub extraction: Extraction,
    /// P1：插件上报的注入账本（权威事件流；与扫描口径分列）。
    pub ledger: LedgerSection,
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

/// L2 分组对比：同会话内「暴露 vs 未暴露请求」的配对差（只写伴随，不写因果）。
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub available: bool,
    /// 固定值 same_session_pair：前端按它写文案。
    pub method: String,
    /// 同时含 exposed 与 unexposed 请求的会话数（配对样本量）。
    pub sessions_with_both: u64,
    pub exposed_requests: u64,
    pub unexposed_requests: u64,
    /// 出读数所需的最少配对会话数（n<10 只给计数）。
    pub required_sessions: u64,
    pub metrics: Vec<ComparisonMetric>,
    pub caveat: String,
}

/// 一项成对指标：暴露组均值、未暴露组均值、配对差与 95% CI。
#[derive(Debug, Serialize)]
pub struct ComparisonMetric {
    pub key: String,
    pub label: String,
    pub unit: String,
    pub exposed: f64,
    pub unexposed: f64,
    pub diff: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub n: u64,
}

/// 提取质量（P2 提前做）：只看 memory.json / jobs.json 已有事实。
#[derive(Debug, Serialize)]
pub struct Extraction {
    /// sources 含 kind==extract 的记忆条数。
    pub extract_memories: u64,
    pub disabled: u64,
    /// 窗口内一次都没被注入过的提取记忆条数。
    pub never_exposed: u64,
    /// 窗口内被模型主动 memory_read 过的条数。
    pub read: u64,
    /// 窗口内弱引用代理命中的合计次数。
    pub mentions: u64,
    pub extractor_versions: Vec<ExtractorVersion>,
}

#[derive(Debug, Serialize)]
pub struct ExtractorVersion {
    pub version: String,
    pub jobs: u64,
    pub failed: u64,
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
    /// 弱引用代理：窗口内助手回复正文出现该记忆标题的次数（启发式）。
    pub mentions: u64,
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

/// P1 注入账本段：插件上报的权威事件流（注入 + 跳过原因），与扫描口径分列。
///
/// 没有账本文件时 available=false 且各计数为 0：这是「未启用」而不是「没有任何注入」，
/// 前端不能把 0 当读数展示。
#[derive(Debug, Serialize)]
pub struct LedgerSection {
    pub available: bool,
    pub injected_events: u64,
    pub skip_events: u64,
    /// 按 kind × reason 分组的原因分解（inject 的触发路径也在这里）。
    pub reasons: Vec<LedgerReasonCount>,
    /// 按 插件版本 × 内核版本 分组的事件与字节（版本对比用）。
    pub versions: Vec<LedgerVersion>,
    /// 最近 10 条事件（新的在前）。
    pub recent: Vec<LedgerRecent>,
    /// 注入事件与扫描状态里 snapshot_turns 的关联率。
    pub link: LedgerLink,
    /// 关联率所需最少样本（0 = 当前未设门槛，只报计数与比例）。
    pub required_sessions: u64,
}

#[derive(Debug, Serialize)]
pub struct LedgerReasonCount {
    pub kind: String,
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct LedgerVersion {
    pub plugin_version: String,
    pub kernel_version: String,
    pub injected: u64,
    pub skip: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct LedgerRecent {
    pub at: String,
    pub session: String,
    pub turn: i64,
    pub kind: String,
    pub reason: String,
    pub ids: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct LedgerLink {
    pub linked: u64,
    pub unlinked: u64,
    /// 关联率（0-100）。
    pub rate: f64,
}

/// 单条记忆在窗口内的汇总（榜单行与漏斗共用）。
#[derive(Default)]
struct MemoryTotal {
    injections: u64,
    exposures: u64,
    reads: u64,
    mentions: u64,
    sessions: u64,
}

/// 派生报告。state 是已经扫过一轮的账目；memory / jobs 是权威库与任务表。
pub(crate) fn build(
    state: &EffectStateDoc,
    memory: &MemoryDoc,
    jobs: &JobsDoc,
    ledger_notes: &[LedgerNote],
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
                total.mentions += bucket.mentions;
            }
            if total.injections == 0
                && total.exposures == 0
                && total.reads == 0
                && total.mentions == 0
            {
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
            slot.mentions += total.mentions;
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
                mentions: total.mentions,
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
    let comparison = build_comparison(state, &from, &to);
    let extraction = build_extraction(memory, jobs, &memory_totals);
    let ledger = build_ledger(state, ledger_notes, &from, &to);

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
        comparison,
        extraction,
        ledger,
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

/// 一个会话在窗口内的两组请求聚合（L2 配对的最小单位）。
#[derive(Default)]
struct SessionGroups {
    exposed: GroupAgg,
    unexposed: GroupAgg,
}

/// 把窗口内的账按会话收拢成 exposed / unexposed 两组。
fn window_sessions(state: &EffectStateDoc, from: &str, to: &str) -> Vec<SessionGroups> {
    let mut sessions = Vec::new();
    for entry in state.files.values() {
        let mut groups = SessionGroups::default();
        for (day, bucket) in &entry.days {
            if !in_window(day, from, to) {
                continue;
            }
            groups.exposed.merge(&bucket.exposed);
            groups.unexposed.merge(&bucket.unexposed);
        }
        sessions.push(groups);
    }
    sessions
}

fn per_request(total: u64, requests: u64) -> Option<f64> {
    (requests > 0).then(|| total as f64 / requests as f64)
}

fn ratio_percent(numerator: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 * 100.0 / denominator as f64)
}

fn tool_calls_per_request(group: &GroupAgg) -> Option<f64> {
    per_request(group.tool_calls, group.requests)
}

fn input_tokens_per_request(group: &GroupAgg) -> Option<f64> {
    per_request(group.input_tokens, group.requests)
}

fn output_tokens_per_request(group: &GroupAgg) -> Option<f64> {
    per_request(group.output_tokens, group.requests)
}

fn cache_read_ratio(group: &GroupAgg) -> Option<f64> {
    ratio_percent(
        group.cache_read_tokens,
        group.input_tokens.saturating_add(group.cache_read_tokens),
    )
}

fn tool_error_rate(group: &GroupAgg) -> Option<f64> {
    ratio_percent(group.tool_errors, group.tool_calls)
}

/// 一项 L2 指标的定义：从一组请求聚合里取出可比数值。
struct ComparisonMetricDef {
    key: &'static str,
    label: &'static str,
    unit: &'static str,
    value: fn(&GroupAgg) -> Option<f64>,
}

const COMPARISON_METRICS: &[ComparisonMetricDef] = &[
    ComparisonMetricDef {
        key: "tool_calls_per_request",
        label: "工具调用 / 请求",
        unit: "次",
        value: tool_calls_per_request,
    },
    ComparisonMetricDef {
        key: "input_tokens_per_request",
        label: "输入 token / 请求",
        unit: "token",
        value: input_tokens_per_request,
    },
    ComparisonMetricDef {
        key: "output_tokens_per_request",
        label: "输出 token / 请求",
        unit: "token",
        value: output_tokens_per_request,
    },
    ComparisonMetricDef {
        key: "cache_read_ratio",
        label: "缓存命中率",
        unit: "%",
        value: cache_read_ratio,
    },
    ComparisonMetricDef {
        key: "tool_error_rate",
        label: "工具错误率",
        unit: "%",
        value: tool_error_rate,
    },
];

/// 出读数所需的最少配对会话数（合同 §2.2：n<10 只给计数）。
const REQUIRED_COMPARISON_SESSIONS: usize = 10;

/// L2 同会话配对：只有两组 requests 都 > 0 的会话进入配对；n<10 只给计数。
fn build_comparison(state: &EffectStateDoc, from: &str, to: &str) -> Comparison {
    let sessions = window_sessions(state, from, to);
    let mut exposed_requests = 0u64;
    let mut unexposed_requests = 0u64;
    let mut paired = 0usize;
    let mut diffs: Vec<Vec<f64>> = COMPARISON_METRICS.iter().map(|_| Vec::new()).collect();
    let mut exposed_values: Vec<Vec<f64>> = COMPARISON_METRICS.iter().map(|_| Vec::new()).collect();
    let mut unexposed_values: Vec<Vec<f64>> =
        COMPARISON_METRICS.iter().map(|_| Vec::new()).collect();

    for session in &sessions {
        exposed_requests += session.exposed.requests;
        unexposed_requests += session.unexposed.requests;
        // 两组都要有请求才构成一个可配对样本；只有一组的是计数素材，不进 CI。
        if session.exposed.requests == 0 || session.unexposed.requests == 0 {
            continue;
        }
        paired += 1;
        for (index, definition) in COMPARISON_METRICS.iter().enumerate() {
            let (Some(exposed), Some(unexposed)) = (
                (definition.value)(&session.exposed),
                (definition.value)(&session.unexposed),
            ) else {
                continue;
            };
            diffs[index].push(exposed - unexposed);
            exposed_values[index].push(exposed);
            unexposed_values[index].push(unexposed);
        }
    }

    let available = paired >= REQUIRED_COMPARISON_SESSIONS;
    let mut metrics = Vec::new();
    if available {
        for (index, definition) in COMPARISON_METRICS.iter().enumerate() {
            let values = &diffs[index];
            if values.is_empty() {
                continue;
            }
            let center = mean(values);
            let (ci_low, ci_high) = confidence_interval(values, center);
            metrics.push(ComparisonMetric {
                key: definition.key.to_string(),
                label: definition.label.to_string(),
                unit: definition.unit.to_string(),
                exposed: round2(mean(&exposed_values[index])),
                unexposed: round2(mean(&unexposed_values[index])),
                diff: round2(center),
                ci_low: round2(ci_low),
                ci_high: round2(ci_high),
                n: values.len() as u64,
            });
        }
    }

    Comparison {
        available,
        method: "same_session_pair".to_string(),
        sessions_with_both: paired as u64,
        exposed_requests,
        unexposed_requests,
        required_sessions: REQUIRED_COMPARISON_SESSIONS as u64,
        metrics,
        caveat: "L2 关联：同会话内暴露 vs 未暴露请求的配对差；不是因果。样本为会话级配对，n<10 不给读数。".to_string(),
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// 配对差的 95% CI：mean ± 1.96 * SE（SE = 样本标准差 / sqrt(n)）。
fn confidence_interval(values: &[f64], center: f64) -> (f64, f64) {
    if values.len() < 2 {
        return (center, center);
    }
    let variance = values
        .iter()
        .map(|value| (value - center).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    let standard_error = (variance / values.len() as f64).sqrt();
    let half = 1.96 * standard_error;
    (center - half, center + half)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// 注入账本段：只做计数与分组，不做任何跨口径合并。
///
/// 关联（link）只对 inject 事件算：turn>=0 时要求扫描状态的 snapshot_turns 里该 turn
/// 有回填；turn<0（旧插件）退化为「事件当天该会话有注入快照」的近似，仍计入分子但
/// 口径更弱——这一点由 rate 旁边的计数与 UI 文案承担，不在后端假装精确。
fn build_ledger(
    state: &EffectStateDoc,
    notes: &[LedgerNote],
    from: &str,
    to: &str,
) -> LedgerSection {
    let mut scoped: Vec<&LedgerNote> = Vec::new();
    let mut reasons: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut versions: BTreeMap<(String, String), (u64, u64, u64)> = BTreeMap::new();
    let mut injected_events = 0u64;
    let mut skip_events = 0u64;
    for note in notes {
        let day = date_string_of_ms(note.at_ms);
        if !in_window(&day, from, to) {
            continue;
        }
        scoped.push(note);
        let is_inject = note.is_inject();
        if is_inject {
            injected_events += 1;
        } else {
            skip_events += 1;
        }
        *reasons
            .entry((note.kind.clone(), note.reason.clone()))
            .or_default() += 1;
        let version = versions
            .entry((note.plugin_version.clone(), note.kernel_version.clone()))
            .or_default();
        if is_inject {
            version.0 += 1;
            version.2 += note.bytes;
        } else {
            version.1 += 1;
        }
    }

    let mut linked = 0u64;
    let mut unlinked = 0u64;
    for note in &scoped {
        if !note.is_inject() {
            continue;
        }
        let matched = match find_session_entry(state, &note.session) {
            None => false,
            Some(entry) if note.turn >= 0 => {
                entry
                    .snapshot_turns
                    .get(&(note.turn as u32))
                    .copied()
                    .unwrap_or(0)
                    > 0
            }
            Some(entry) => {
                // 旧插件没有 turn：退化为「事件当天该会话有注入快照」。
                let day = date_string_of_ms(note.at_ms);
                entry
                    .days
                    .get(&day)
                    .is_some_and(|bucket| bucket.injection_snapshots > 0)
            }
        };
        if matched {
            linked += 1;
        } else {
            unlinked += 1;
        }
    }
    let link_total = linked + unlinked;
    let link_rate = if link_total == 0 {
        0.0
    } else {
        round2(linked as f64 * 100.0 / link_total as f64)
    };

    let mut reason_rows: Vec<LedgerReasonCount> = reasons
        .into_iter()
        .map(|((kind, reason), count)| LedgerReasonCount {
            kind,
            reason,
            count,
        })
        .collect();
    reason_rows.sort_by(|left, right| {
        kind_rank(&left.kind)
            .cmp(&kind_rank(&right.kind))
            .then_with(|| right.count.cmp(&left.count))
            .then_with(|| left.reason.cmp(&right.reason))
    });

    let mut version_rows: Vec<LedgerVersion> = versions
        .into_iter()
        .map(
            |((plugin_version, kernel_version), (injected, skip, bytes))| LedgerVersion {
                plugin_version,
                kernel_version,
                injected,
                skip,
                bytes,
            },
        )
        .collect();
    version_rows.sort_by(|left, right| {
        right
            .injected
            .cmp(&left.injected)
            .then_with(|| right.skip.cmp(&left.skip))
            .then_with(|| left.plugin_version.cmp(&right.plugin_version))
            .then_with(|| left.kernel_version.cmp(&right.kernel_version))
    });

    // recent 取最新 10 条：notes 已按 at_ms 升序。
    let recent: Vec<LedgerRecent> = scoped
        .iter()
        .rev()
        .take(10)
        .map(|note| LedgerRecent {
            at: iso_utc(note.at_ms),
            session: note.session.clone(),
            turn: note.turn,
            kind: note.kind.clone(),
            reason: note.reason.clone(),
            ids: note.ids.len() as u64,
            bytes: note.bytes,
        })
        .collect();

    LedgerSection {
        available: !scoped.is_empty(),
        injected_events,
        skip_events,
        reasons: reason_rows,
        versions: version_rows,
        recent,
        link: LedgerLink {
            linked,
            unlinked,
            rate: link_rate,
        },
        required_sessions: 0,
    }
}

/// inject 排前、skip 排后（原因分解的主次顺序）。
fn kind_rank(kind: &str) -> u8 {
    if kind == crate::memory::effect::ledger::KIND_INJECT {
        0
    } else {
        1
    }
}

/// 账目键是「工作区/会话目录/文件名」，会话 id 是倒数第二段。
fn find_session_entry<'a>(
    state: &'a EffectStateDoc,
    session: &str,
) -> Option<&'a super::FileEntry> {
    state.files.iter().find_map(|(key, entry)| {
        let mut parts = key.split('/');
        let _workspace = parts.next()?;
        let name = parts.next()?;
        (name == session).then_some(entry)
    })
}

/// 提取质量：只看 memory.json / jobs.json 已有事实（P2 提前做，无新埋点）。
fn build_extraction(
    memory: &MemoryDoc,
    jobs: &JobsDoc,
    memory_totals: &BTreeMap<String, MemoryTotal>,
) -> Extraction {
    let mut extract_records = Vec::new();
    for record in &memory.memories {
        if record
            .sources
            .iter()
            .any(|source| source.kind == crate::memory::store::SOURCE_EXTRACT)
        {
            extract_records.push(record);
        }
    }
    let mut disabled = 0u64;
    let mut never_exposed = 0u64;
    let mut read = 0u64;
    let mut mentions = 0u64;
    for record in &extract_records {
        if record.state == crate::memory::store::STATE_DISABLED {
            disabled += 1;
        }
        let total = memory_totals.get(&record.id);
        if total.map_or(0, |total| total.exposures) == 0 {
            never_exposed += 1;
        }
        if total.is_some_and(|total| total.reads > 0) {
            read += 1;
        }
        mentions += total.map_or(0, |total| total.mentions);
    }
    let mut by_extractor: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for job in &jobs.jobs {
        let slot = by_extractor.entry(job.key.extractor.clone()).or_default();
        slot.0 += 1;
        if job.state == crate::memory::jobs::JOB_FAILED {
            slot.1 += 1;
        }
    }
    let mut extractor_versions: Vec<ExtractorVersion> = by_extractor
        .into_iter()
        .map(|(version, (jobs, failed))| ExtractorVersion {
            version,
            jobs,
            failed,
        })
        .collect();
    extractor_versions.sort_by(|left, right| {
        right
            .jobs
            .cmp(&left.jobs)
            .then_with(|| left.version.cmp(&right.version))
    });
    Extraction {
        extract_memories: extract_records.len() as u64,
        disabled,
        never_exposed,
        read,
        mentions,
        extractor_versions,
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
pub(crate) fn iso_utc(ms: u64) -> String {
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

        let report = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &[], now, 7);
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

        let wide = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &[], now, 90);
        assert_eq!(wide.trend.len(), 90);
        assert_eq!(wide.kpi.injection_snapshots, 6, "90 天窗口含 40 天前");

        let narrow = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &[], now, 30);
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
        let report = build(&doc, &memory, &JobsDoc::new(), &[], now, 7);
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
            &[],
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
        let report = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &[], now, 7);
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
            &[],
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

    fn group(
        requests: u64,
        tool_calls: u64,
        tool_errors: u64,
        input: u64,
        output: u64,
        cache_read: u64,
    ) -> crate::memory::effect::GroupAgg {
        crate::memory::effect::GroupAgg {
            requests,
            tool_calls,
            tool_errors,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
        }
    }

    fn state_with_groups(now: u64, sessions: usize) -> EffectStateDoc {
        let day = date_string_of_ms(now);
        let mut doc = EffectStateDoc::default();
        for index in 0..sessions {
            let mut entry = super::super::FileEntry::default();
            let bucket = entry.days.entry(day.clone()).or_default();
            bucket.requests = 4;
            bucket.exposed = group(2, 4, 0, 2000, 200, 1000);
            bucket.unexposed = group(2, 6, 2, 1600, 400, 200);
            doc.files
                .insert(format!("ws/session-{index}/session.v4.jsonl.zstd"), entry);
        }
        doc
    }

    #[test]
    fn comparison_pairs_same_session_metrics_and_needs_ten_sessions() {
        let now = 1_790_000_000_000u64;
        let report = build(
            &state_with_groups(now, 12),
            &MemoryDoc::default(),
            &JobsDoc::new(),
            &[],
            now,
            7,
        );
        let comparison = &report.comparison;
        assert!(comparison.available);
        assert_eq!(comparison.method, "same_session_pair");
        assert_eq!(comparison.sessions_with_both, 12);
        assert_eq!(comparison.exposed_requests, 24);
        assert_eq!(comparison.unexposed_requests, 24);
        assert_eq!(comparison.required_sessions, 10);
        let tool_calls = comparison
            .metrics
            .iter()
            .find(|metric| metric.key == "tool_calls_per_request")
            .expect("工具调用指标");
        assert_eq!(tool_calls.exposed, 2.0);
        assert_eq!(tool_calls.unexposed, 3.0);
        assert_eq!(tool_calls.diff, -1.0);
        assert_eq!(tool_calls.n, 12);
        assert_eq!((tool_calls.ci_low, tool_calls.ci_high), (-1.0, -1.0));
        let cache = comparison
            .metrics
            .iter()
            .find(|metric| metric.key == "cache_read_ratio")
            .expect("缓存命中率指标");
        assert_eq!(cache.exposed, 33.33);
        assert_eq!(cache.unexposed, 11.11);
        assert_eq!(cache.diff, 22.22);
        let error_rate = comparison
            .metrics
            .iter()
            .find(|metric| metric.key == "tool_error_rate")
            .expect("工具错误率指标");
        assert_eq!(error_rate.exposed, 0.0);
        assert_eq!(error_rate.unexposed, 33.33);
        assert!(
            comparison.caveat.contains("不是因果"),
            "L2 文案不许写成因果"
        );

        // n<10：只给计数，不给读数。
        let small = build(
            &state_with_groups(now, 9),
            &MemoryDoc::default(),
            &JobsDoc::new(),
            &[],
            now,
            7,
        );
        assert!(!small.comparison.available);
        assert!(small.comparison.metrics.is_empty());
        assert_eq!(small.comparison.sessions_with_both, 9);
        assert_eq!(small.comparison.required_sessions, 10);
    }

    #[test]
    fn comparison_skips_sessions_that_only_have_one_group() {
        let now = 1_790_000_000_000u64;
        let mut doc = state_with_groups(now, 12);
        let day = date_string_of_ms(now);
        let mut partial = super::super::FileEntry::default();
        partial.days.entry(day).or_default().exposed = group(2, 2, 0, 100, 10, 0);
        doc.files.insert(
            "ws/session-partial/session.v4.jsonl.zstd".to_string(),
            partial,
        );
        let report = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &[], now, 7);
        assert_eq!(
            report.comparison.sessions_with_both, 12,
            "只有一组请求的会话不算配对样本"
        );
        assert_eq!(report.comparison.exposed_requests, 26, "请求数仍计入总量");
        let tool_calls = report
            .comparison
            .metrics
            .iter()
            .find(|metric| metric.key == "tool_calls_per_request")
            .expect("工具调用指标");
        assert_eq!(tool_calls.n, 12, "配对 n 不含只有一组的会话");
    }

    fn extract_meta(id: &str, state: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            title: "提取记忆".to_string(),
            scope: "project".to_string(),
            state: state.to_string(),
            sources: vec![crate::memory::store::MemorySource {
                kind: crate::memory::store::SOURCE_EXTRACT.to_string(),
                ..crate::memory::store::MemorySource::default()
            }],
            created_at: "2026-10-01T00:00:00Z".to_string(),
            ..MemoryRecord::default()
        }
    }

    #[test]
    fn extraction_quality_reads_store_sources_and_jobs_versions() {
        let now = 1_790_000_000_000u64;
        let day = date_string_of_ms(now);
        let extract_id = "1".repeat(32);
        let disabled_id = "2".repeat(32);
        let manual_id = "3".repeat(32);
        let mut doc = EffectStateDoc::default();
        let mut entry = super::super::FileEntry::default();
        let bucket = entry
            .memories
            .entry(extract_id.clone())
            .or_default()
            .entry(day.clone())
            .or_default();
        bucket.exposures = 3;
        bucket.reads = 1;
        bucket.mentions = 2;
        entry
            .memories
            .entry(manual_id.clone())
            .or_default()
            .entry(day)
            .or_default()
            .exposures = 5;
        doc.files
            .insert("ws/session-x/session.v4.jsonl.zstd".to_string(), entry);
        let memory = MemoryDoc {
            memories: vec![
                extract_meta(&extract_id, "accepted"),
                extract_meta(&disabled_id, "disabled"),
                MemoryRecord {
                    id: manual_id,
                    sources: vec![crate::memory::store::MemorySource::manual(
                        "2026-10-01T00:00:00Z",
                    )],
                    ..MemoryRecord::default()
                },
            ],
            ..MemoryDoc::default()
        };
        let mut jobs = JobsDoc::new();
        for (extractor, state) in [
            ("xlink-memory-extract/5", "done"),
            ("xlink-memory-extract/5", "failed"),
            ("xlink-memory-extract/4", "done"),
        ] {
            jobs.jobs.push(Job {
                key: JobKey {
                    session: "s".to_string(),
                    cursor: 0,
                    extractor: extractor.to_string(),
                    from_cursor: 0,
                },
                state: state.to_string(),
                created_at_ms: 0,
                updated_at_ms: 0,
                attempts: 1,
                lease: None,
                error: String::new(),
            });
        }
        let report = build(&doc, &memory, &jobs, &[], now, 7);
        let extraction = &report.extraction;
        assert_eq!(extraction.extract_memories, 2);
        assert_eq!(extraction.disabled, 1);
        assert_eq!(extraction.never_exposed, 1);
        assert_eq!(extraction.read, 1);
        assert_eq!(extraction.mentions, 2);
        assert_eq!(extraction.extractor_versions.len(), 2);
        assert_eq!(
            extraction.extractor_versions[0].version,
            "xlink-memory-extract/5"
        );
        assert_eq!(extraction.extractor_versions[0].jobs, 2);
        assert_eq!(extraction.extractor_versions[0].failed, 1);
        assert_eq!(
            extraction.extractor_versions[1].version,
            "xlink-memory-extract/4"
        );
    }

    fn ledger_note(
        at_ms: u64,
        session: &str,
        turn: i64,
        kind: &str,
        reason: &str,
        plugin_version: &str,
        bytes: u64,
    ) -> crate::memory::effect::ledger::LedgerNote {
        crate::memory::effect::ledger::LedgerNote {
            at_ms,
            session: session.to_string(),
            turn,
            step: -1,
            kind: kind.to_string(),
            reason: reason.to_string(),
            enabled: true,
            ids: vec!["a".repeat(32)],
            bytes,
            generation: 1,
            workspace_id: String::new(),
            plugin_version: plugin_version.to_string(),
            kernel_version: "k1".to_string(),
        }
    }

    #[test]
    fn ledger_section_groups_reasons_versions_and_links_to_snapshot_turns() {
        let now = 1_790_000_000_000u64;
        let day = date_string_of_ms(now);
        let mut doc = EffectStateDoc::default();
        let mut entry = super::super::FileEntry::default();
        entry.snapshot_turns.insert(5, 1);
        entry
            .days
            .entry(day.clone())
            .or_default()
            .injection_snapshots = 1;
        doc.files
            .insert("ws/session-x/session.v4.jsonl.zstd".to_string(), entry);
        let notes = vec![
            ledger_note(now, "session-x", 5, "inject", "turn", "0.2.0", 100),
            ledger_note(now, "session-x", 9, "inject", "library_change", "0.2.0", 50),
            // 旧插件（turn=-1）：退化为「事件当天该会话有注入快照」的近似关联。
            ledger_note(now, "session-x", -1, "inject", "turn", "0.1.0", 30),
            ledger_note(now, "other", 7, "skip", "budget", "0.2.0", 0),
            ledger_note(now, "other", 7, "skip", "no_hit", "0.2.0", 0),
        ];
        let report = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &notes, now, 7);
        let ledger = &report.ledger;
        assert!(ledger.available);
        assert_eq!(ledger.injected_events, 3);
        assert_eq!(ledger.skip_events, 2);
        assert_eq!(ledger.link.linked, 2, "turn 命中 + 旧插件近似各一条");
        assert_eq!(ledger.link.unlinked, 1);
        assert_eq!(ledger.link.rate, 66.67);
        assert_eq!(ledger.reasons[0].kind, "inject", "inject 原因排前");
        assert_eq!(ledger.reasons[0].reason, "turn");
        assert_eq!(ledger.reasons[0].count, 2);
        let version = ledger
            .versions
            .iter()
            .find(|version| version.plugin_version == "0.2.0")
            .expect("0.2.0 版本行");
        assert_eq!((version.injected, version.skip, version.bytes), (2, 2, 150));
        assert_eq!(ledger.recent.len(), 5);
        assert_eq!(ledger.recent[0].kind, "skip", "recent 新的在前");

        // 没有账本：available=false，不把 0 当读数。
        let empty = build(&doc, &MemoryDoc::default(), &JobsDoc::new(), &[], now, 7);
        assert!(!empty.ledger.available);
        assert_eq!(empty.ledger.injected_events, 0);
        assert_eq!(empty.ledger.recent.len(), 0);
    }
}
