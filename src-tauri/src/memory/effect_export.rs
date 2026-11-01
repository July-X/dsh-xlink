//! 记忆效果的 agent 可读快照（2026-11-01）。
//!
//! 面板上的效果报告每次由 `effect::report()` 现算，只在 Tauri IPC 里活一次；本机
//! agent（开发期的分析 / 回归进程）拿不到那个对象，只能自己去读 `state.json` 与注入
//! 账本，而最关键的 L2 配对、L3 实验、健康度全在 Rust 里派生。这里把同一次报告落成
//! 两份文件：
//!
//! · `<effect_dir>/report.json`：完整 `EffectReport` 加一层信封（schema / kind /
//!   generated_at / 刷新口径），给程序读；
//! · `<effect_dir>/report.md`：几 KB 的摘要（含口径纪律与可复查线索），给模型直接读，
//!   不必先啃 22k 行的 `state.json`。
//!
//! ## 边界
//!
//! · **派生数据，不是权威记录**：写失败只记一行壳日志，绝不影响面板取数或记忆库；
//! · **只在报告生成时重写**：面板打开「记忆 → 效果」页或切 7/30/90 时间窗时会调
//!   `memory_effect_report`，这里顺手写一份。不做后台定时扫描——冷扫要解几百 MB
//!   会话日志，不该塞进启动路径；权威事件流是同目录 `ledger/notes-YYYY-MM.jsonl`，
//!   它随每次注入实时追加，不受本快照的新鲜度影响；
//! · **隐私**：JSON / MD 与 `memory.json` 同目录、0600。榜单标题按 id 从
//!   `memory.json` 现查，只落在这一份快照里，不进账本、不进壳日志；
//! · **不复制口径**：所有数字来自 `EffectReport`，这里只做排版，绝不重新聚合。
//!
//! 读法与已知缺口见 `docs/features/memory/agent-effect-snapshot.md`。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::memory::effect::report::EffectReport;
use crate::memory::store::Namespace;
use crate::shell::atomic_write::atomic_write_private;
use crate::shell::error::AppError;
use crate::shell::{paths, shell_events};
use crate::usage::local::date_string_of_ms;

/// 快照信封的 schema。与 `state.json`（schema 3）无关：那是扫描缓存，这里只承诺
/// 外层 `schema/kind/generated_at/range_days/refresh/report` 六个键的形状。
const SNAPSHOT_SCHEMA: u32 = 1;

/// 信封里的 kind：让后来的 agent 一眼认出这是哪份文件，别跟 state.json 混。
const SNAPSHOT_KIND: &str = "xlink.memory.effect-report";

/// 刷新口径写进文件本身，读的人不用先翻文档。
const REFRESH_NOTE: &str = "在每次 effect::report 生成时重写（管理面板：记忆 → 效果页；切 7/30/90 时间窗也会重写）。权威事件流是同目录 ledger/notes-YYYY-MM.jsonl；state.json 是内部扫描缓存，不能替代本快照。";

/// 快照要写的两个文件。路径显式给出，单测用临时目录、不碰真实家目录。
#[derive(Debug, Clone)]
pub(crate) struct SnapshotPaths {
    pub json: PathBuf,
    pub markdown: PathBuf,
}

/// 本壳默认实例的快照路径（实例 × profile 隔离，与 `state.json` 同目录）。
pub(crate) fn snapshot_paths(namespace: &Namespace) -> SnapshotPaths {
    SnapshotPaths {
        json: paths::instance_memory_effect_report_file(
            &namespace.family,
            &namespace.instance_id,
            &namespace.profile,
        ),
        markdown: paths::instance_memory_effect_report_markdown_file(
            &namespace.family,
            &namespace.instance_id,
            &namespace.profile,
        ),
    }
}

/// 信封：外层是文件契约，`report` 原样保留 `EffectReport`（面板与 agent 同一份口径）。
#[derive(Debug, Serialize)]
struct SnapshotDoc<'a> {
    schema: u32,
    kind: &'static str,
    generated_at: &'a str,
    range_days: u32,
    refresh: &'static str,
    report: &'a EffectReport,
}

/// 面板命令入口：先算报告，再尽力写快照。**写失败不回滚报告**——面板要的是读数，
/// 快照只是给本机 agent 的旁路产物。
pub(crate) fn report_and_write(range_days: Option<u32>) -> Result<EffectReport, AppError> {
    let report = crate::memory::effect::report(range_days)?;
    let snapshot = snapshot_paths(&Namespace::current());
    if let Err(error) = write_to(&snapshot, &report) {
        shell_events::record("memory-effect", &format!("记忆效果快照没写出来：{error}"));
    }
    Ok(report)
}

/// 把一份报告写成 JSON + Markdown；同一份数据只排版两次。
pub(crate) fn write_to(snapshot: &SnapshotPaths, report: &EffectReport) -> Result<(), AppError> {
    let doc = SnapshotDoc {
        schema: SNAPSHOT_SCHEMA,
        kind: SNAPSHOT_KIND,
        generated_at: &report.generated_at,
        range_days: report.range_days,
        refresh: REFRESH_NOTE,
        report,
    };
    let json = serde_json::to_string_pretty(&doc).map_err(|error| {
        AppError::Memory(format!(
            "序列化记忆效果快照失败：{error}。这是内部错误，不影响面板读数"
        ))
    })?;
    let markdown = render_markdown(report);
    write_private(&snapshot.json, json.as_bytes())?;
    write_private(&snapshot.markdown, markdown.as_bytes())?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    atomic_write_private(path, bytes).map_err(|error| {
        AppError::Memory(format!(
            "写入记忆效果快照失败（{}）：{error}。快照只是派生读数，不影响记忆库；请检查磁盘空间与目录写入权限",
            path.display()
        ))
    })
}

/// 生成给模型直接读的摘要。**只排版 EffectReport**，不引入第二个口径。
fn render_markdown(report: &EffectReport) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str("# 记忆效果快照（agent 可读）\n\n");
    out.push_str(&format!(
        "- 生成时间：{}\n- 窗口：{} ~ {}（{} 天，本地日历日）\n- 文件契约：同目录 report.json（schema {}，完整字段）；权威事件流：ledger/notes-YYYY-MM.jsonl；内部缓存：state.json\n- 刷新：{}\n",
        report.generated_at,
        report.window.from,
        report.window.to,
        report.range_days,
        SNAPSHOT_SCHEMA,
        REFRESH_NOTE
    ));
    render_trust(&mut out, report);
    render_kpi(&mut out, report);
    render_ledger(&mut out, report);
    render_l2(&mut out, report);
    render_l3(&mut out, report);
    render_extraction(&mut out, report);
    render_leaderboard(&mut out, report);
    render_reading_guide(&mut out);
    render_hints(&mut out, report);
    out
}

fn render_trust(out: &mut String, report: &EffectReport) {
    let coverage = &report.coverage;
    out.push_str("\n## 1. 数据可信度（先看这节，再信其它数字）\n\n");
    out.push_str(&format!(
        "- 会话覆盖：窗口内出现过 {} 个会话（历史保留 {}）；其中有请求 {} 个、有注入 {} 个\n",
        coverage.sessions_in_window,
        coverage.sessions_total,
        coverage.sessions_active,
        coverage.sessions_with_injection
    ));
    out.push_str(&format!(
        "- 解析健康：解码失败文件 {} 个、日志坏行 {} 行、未知事件 {:.1}%\n",
        coverage.decode_failures, coverage.malformed_lines, coverage.unknown_event_ratio
    ));
    out.push_str(&format!(
        "- 样本门槛：请求样本{}、注入样本{}（>= 30 才出比率）；L2 配对会话 {} / 需 {}；L3 对照组 {}、实验组 {}（每组 >= {}）\n",
        enough(report.health.has_enough_requests),
        enough(report.health.has_enough_injections),
        report.comparison.sessions_with_both,
        report.comparison.required_sessions,
        report.experiment.control_sessions,
        report.experiment.treatment_sessions,
        report.experiment.required_sessions
    ));
    let ledger = &report.ledger;
    let link = &ledger.link;
    let linked_total = link.linked + link.unlinked;
    if ledger.available {
        out.push_str(&format!(
            "- 注入账本：已启用；注入事件与扫描快照关联 {} / {}（{:.1}%）\n",
            link.linked, linked_total, link.rate
        ));
    } else {
        out.push_str("- 注入账本：未启用——只能看扫描口径，未注入原因（预算 / 零命中 / 开关 / 桥失败）无法区分\n");
    }
}

fn render_kpi(out: &mut String, report: &EffectReport) {
    let kpi = &report.kpi;
    let funnel = &report.funnel;
    let cost = &report.cost;
    let store = &report.store;
    out.push_str("\n## 2. 核心读数（L1 事实，口径：扫描本机会话日志）\n\n");
    out.push_str(&format!(
        "- 注入快照 {} 次、注入字节 {}、估算 token {}\n",
        kpi.injection_snapshots, kpi.injected_bytes, kpi.injected_tokens_est
    ));
    let ratio = if report.health.has_enough_requests {
        format!("{:.1}%", kpi.memory_request_ratio)
    } else {
        "样本不足，只给计数".to_string()
    };
    out.push_str(&format!(
        "- 请求 {} 次；携带记忆的请求 {} 次（{}）\n",
        kpi.requests, kpi.memory_requests, ratio
    ));
    out.push_str(&format!(
        "- 工具调用 {} 次（错误 {}）；memory_read {} / memory_search {} / memory_propose {}\n",
        kpi.tool_calls,
        kpi.tool_errors,
        kpi.memory_read_calls,
        kpi.memory_search_calls,
        kpi.memory_propose_calls
    ));
    out.push_str(&format!(
        "- 漏斗（窗口内）：候选新增 {} → 记忆新增 {} → 被注入的独立记忆 {} → 被 memory_read 的 {}\n",
        funnel.candidates_created,
        funnel.memories_created,
        funnel.memories_injected,
        funnel.memories_read
    ));
    out.push_str(&format!(
        "- 成本：注入 token 估算 {}；提取任务完成 {} / 失败 {}，提取 token 上界估算 {}\n",
        cost.injected_tokens_est,
        cost.extraction_jobs_done,
        cost.extraction_jobs_failed,
        cost.extraction_tokens_upper_est
    ));
    out.push_str(&format!(
        "- 库存：记忆 {} / 经验 {} / 交接 {}；候选待确认 {}；提取任务 pending {} / running {} / done {} / failed {}\n",
        store.memories,
        store.experiences,
        store.handoffs,
        store.candidates_pending,
        store.jobs_pending,
        store.jobs_running,
        store.jobs_done,
        store.jobs_failed
    ));
}

fn render_ledger(out: &mut String, report: &EffectReport) {
    let ledger = &report.ledger;
    out.push_str("\n## 3. 未注入原因（插件账本，权威事件流）\n\n");
    if !ledger.available {
        out.push_str("- 账本未启用：原因分解与版本表不可用。此时只靠扫描口径，\"没注入\"分不清预算 / 零命中 / 开关 / 桥失败\n");
        return;
    }
    out.push_str(&format!(
        "- 账本事件：inject {} / skip {}\n",
        ledger.injected_events, ledger.skip_events
    ));
    // 面板的「未注入原因」只画 skip；inject 行回答的是「为什么注入」，不是「为什么
    // 没注入」。两类混在一节会让读者把 inject/turn 当成一种未注入原因，这里分列。
    let skip_rows: Vec<_> = ledger
        .reasons
        .iter()
        .filter(|row| row.kind == "skip")
        .collect();
    if !skip_rows.is_empty() {
        out.push_str("- 未注入原因（skip × reason，与面板口径一致）：\n");
        for row in skip_rows.iter().take(16) {
            out.push_str(&format!(
                "  - {} / {}：{}\n",
                row.kind, row.reason, row.count
            ));
        }
    }
    let inject_rows: Vec<_> = ledger
        .reasons
        .iter()
        .filter(|row| row.kind != "skip")
        .collect();
    if !inject_rows.is_empty() {
        out.push_str("- 注入触发路径（inject × reason，仅供理解注入时机，不是「未注入」）：\n");
        for row in inject_rows.iter().take(8) {
            out.push_str(&format!(
                "  - {} / {}：{}\n",
                row.kind, row.reason, row.count
            ));
        }
    }
    if !ledger.versions.is_empty() {
        out.push_str("- 版本表（插件 × 内核）：\n");
        for row in ledger.versions.iter().take(8) {
            out.push_str(&format!(
                "  - {} / {}：inject {}、skip {}、bytes {}\n",
                row.plugin_version, row.kernel_version, row.injected, row.skip, row.bytes
            ));
        }
    }
    if !ledger.recent.is_empty() {
        out.push_str("- 最近事件（新→旧，最多 5 条；session 只给短 id）：\n");
        for row in ledger.recent.iter().take(5) {
            out.push_str(&format!(
                "  - {} {} turn {} {} / {} ids {} bytes {}\n",
                row.at,
                short_session(&row.session),
                row.turn,
                row.kind,
                row.reason,
                row.ids,
                row.bytes
            ));
        }
    }
}

fn render_l2(out: &mut String, report: &EffectReport) {
    let comparison = &report.comparison;
    out.push_str("\n## 4. L2 同会话配对（伴随差异，不是因果）\n\n");
    out.push_str(&format!(
        "- 方法 {}；配对会话 {} 个，需要 {} 个\n",
        comparison.method, comparison.sessions_with_both, comparison.required_sessions
    ));
    if comparison.available && !comparison.metrics.is_empty() {
        out.push_str(
            "\n| 指标 | 暴露 | 未暴露 | 差 | 95% CI | n |\n| --- | --- | --- | --- | --- | --- |\n",
        );
        for row in &comparison.metrics {
            out.push_str(&format!(
                "| {} | {:.2}{} | {:.2}{} | {:+.2}{} | {:.2} ~ {:.2} | {} |\n",
                row.label,
                row.exposed,
                row.unit,
                row.unexposed,
                row.unit,
                row.diff,
                row.unit,
                row.ci_low,
                row.ci_high,
                row.n
            ));
        }
    } else {
        out.push_str(&format!(
            "- 样本不足：同时含暴露与未暴露请求的会话只有 {} 个（需 {} 个），这里只该引用计数、不下结论\n",
            comparison.sessions_with_both, comparison.required_sessions
        ));
    }
    out.push_str(&format!("- 口径：{}\n", comparison.caveat));
}

fn render_l3(out: &mut String, report: &EffectReport) {
    let experiment = &report.experiment;
    out.push_str("\n## 5. L3 会话级随机对照（只有实验开启时才谈因果）\n\n");
    let state = if !experiment.configured {
        "未配置（默认关闭）".to_string()
    } else if experiment.active {
        "进行中".to_string()
    } else if experiment.stopped_at_ms > 0 {
        format!("已停止（{}）", date_string_of_ms(experiment.stopped_at_ms))
    } else {
        format!("已到期（{}）", date_string_of_ms(experiment.ends_at_ms))
    };
    let window = if experiment.configured {
        format!(
            "{} ~ {}",
            date_string_of_ms(experiment.started_at_ms),
            date_string_of_ms(experiment.ends_at_ms)
        )
    } else {
        "—".to_string()
    };
    out.push_str(&format!(
        "- 状态：{}；对照组比例 {:.1}%；实验窗口 {}（按本地日期）\n",
        state, experiment.ratio_percent, window
    ));
    out.push_str(&format!(
        "- 样本：对照组 {} / 实验组 {}（每组 >= {} 才出读数）\n",
        experiment.control_sessions, experiment.treatment_sessions, experiment.required_sessions
    ));
    if experiment.metrics_available && !experiment.metrics.is_empty() {
        out.push_str("\n| 指标 | 对照组 | 实验组 | 差 | 95% CI | n |\n| --- | --- | --- | --- | --- | --- |\n");
        for row in &experiment.metrics {
            out.push_str(&format!(
                "| {} | {:.2}{} | {:.2}{} | {:+.2}{} | {:.2} ~ {:.2} | {}/{} |\n",
                row.label,
                row.control,
                row.unit,
                row.treatment,
                row.unit,
                row.diff,
                row.unit,
                row.ci_low,
                row.ci_high,
                row.n_control,
                row.n_treatment
            ));
        }
    } else {
        out.push_str("- 读数未出：样本不足或实验未运行，别引用任何 L3 结论\n");
    }
    out.push_str(&format!("- 口径：{}\n", experiment.caveat));
}

fn render_extraction(out: &mut String, report: &EffectReport) {
    let extraction = &report.extraction;
    out.push_str("\n## 6. 提取质量（自动提取来源的记忆）\n\n");
    out.push_str(&format!(
        "- 窗口内新建的提取记忆 {} 条：已停用 {}；从未曝光 {}；被 memory_read 读过 {}；弱引用命中 {} 次\n",
        extraction.extract_memories,
        extraction.disabled,
        extraction.never_exposed,
        extraction.read,
        extraction.mentions
    ));
    if extraction.extractor_versions.is_empty() {
        out.push_str("- 提取器版本：窗口内没有提取任务\n");
    } else {
        out.push_str("- 提取器版本：\n");
        for row in extraction.extractor_versions.iter().take(8) {
            out.push_str(&format!(
                "  - {}：任务 {} / 失败 {}\n",
                row.version, row.jobs, row.failed
            ));
        }
    }
}

fn render_leaderboard(out: &mut String, report: &EffectReport) {
    out.push_str(
        "\n## 7. 记忆榜单（按注入快照排序，最多 10 条；标题来自 memory.json，只在本机）\n\n",
    );
    if report.leaderboard.is_empty() {
        out.push_str("- （窗口内没有可统计的记忆）\n");
        return;
    }
    out.push_str("| 标题 | 范围/验证 | 注入 | 携带请求 | 会话 | 弱引用 |\n| --- | --- | --- | --- | --- | --- |\n");
    for row in report.leaderboard.iter().take(10) {
        out.push_str(&format!(
            "| {} | {} / {} | {} | {} | {} | {} |\n",
            escape_cell(&row.title),
            escape_cell(&row.scope),
            escape_cell(&row.verification),
            row.injection_snapshots,
            row.memory_requests,
            row.sessions,
            row.mentions
        ));
    }
}

fn render_reading_guide(out: &mut String) {
    out.push_str("\n## 8. 给后续开发者 / agent 的读法\n\n");
    out.push_str("- 三个口径不能相加：扫描「注入快照」、账本「inject 事件」、扫描「携带记忆请求」是三种单位，面板也从不相加。\n");
    out.push_str("- 「模型读过 0 次」是结构性常态：正文已随注入给全，memory_read 只统计按 id 读全文，不要据此判定记忆无效。\n");
    out.push_str("- L2 只能写「伴随」；L3 才能谈因果，且样本门槛不到时只给计数。不要造单一「记忆有效性总分」。\n");
    out.push_str("- 逐请求 join 目前不存在：账本缺 request_seq / event_seq（设计稿 §4.2 的字段未落地），别假装能从现有产物精确算出「这次注入对应哪次请求、结果如何」。\n");
    out.push_str("- 需要更多维度读 report.json，或直接读 ledger 与 state.json；不要另行实现一份 scan 口径——两份实现必然漂。\n");
    out.push_str(
        "- 本文件在面板打开效果页时刷新；要更实时的注入原因，直接看 ledger/notes-YYYY-MM.jsonl。\n",
    );
}

fn render_hints(out: &mut String, report: &EffectReport) {
    let mut hints: Vec<String> = Vec::new();
    let coverage = &report.coverage;
    if coverage.decode_failures > 0 {
        hints.push(format!(
            "有 {} 个会话文件解码失败：先确认内核会话格式是否变了（看 coverage.format_versions），再怀疑扫码头",
            coverage.decode_failures
        ));
    }
    if coverage.malformed_lines > 0 {
        hints.push(format!(
            "有 {} 行日志解析失败：可能是写入中断或格式漂移，比率类读数要打折看",
            coverage.malformed_lines
        ));
    }
    if coverage.unknown_event_ratio > 5.0 {
        hints.push(format!(
            "未知事件占 {:.1}%（> 5%）：更像内核版本漂移，报告口径可能漏事件",
            coverage.unknown_event_ratio
        ));
    }
    if coverage.sessions_in_window == 0 {
        hints.push(
            "窗口内没有可读会话：确认工作台确实在用本实例，或把时间窗切到 30/90 天".to_string(),
        );
    }
    if !report.ledger.available {
        hints.push(
            "账本未启用：未注入原因分解不可用；先确认本地记忆插件已启用并升级到带账本的版本"
                .to_string(),
        );
    }
    let linked_total = report.ledger.link.linked + report.ledger.link.unlinked;
    if report.ledger.available && linked_total >= 20 && report.ledger.link.rate < 90.0 {
        hints.push(format!(
            "账本与扫描快照关联率 {:.1}%（< 90%，样本 {}）：注入事件与请求的对应关系偏弱，L2 结论要更保守",
            report.ledger.link.rate, linked_total
        ));
    }
    let no_hit = reason_count(report, "no_hit");
    let budget = reason_count(report, "budget");
    let bridge_failed = reason_count(report, "bridge_failed");
    let disabled = reason_count(report, "disabled");
    let skips = report.ledger.skip_events.max(1);
    if no_hit * 2 >= skips && no_hit > 0 {
        hints.push(format!(
            "skip 里 no_hit 占多数（{}/{}）：检索命中面可能偏窄，先看 query.rs 的关键词 / 标题匹配与固定项",
            no_hit, report.ledger.skip_events
        ));
    }
    if budget > 0 {
        hints.push(format!(
            "skip 里 budget {} 次：预算触顶，核对 policy.js 的 8+2 次 / 36 KiB 与压缩后重置语义",
            budget
        ));
    }
    if bridge_failed > 0 {
        hints.push(format!(
            "skip 里 bridge_failed {} 次：插件到壳的桥调用失败，去「查看日志」的 memory-bridge 标签看原因",
            bridge_failed
        ));
    }
    if disabled > 0 {
        hints.push(format!(
            "skip 里 disabled {} 次：开关关闭期间的回合，不能算作记忆失效",
            disabled
        ));
    }
    if report.kpi.injection_snapshots > 0 && report.kpi.memory_read_calls == 0 {
        hints.push(
            "注入过但 memory_read 为 0：这符合预期（正文已随注入给全），不是缺陷".to_string(),
        );
    }
    if report.health.has_enough_requests
        && report.kpi.memory_search_calls == 0
        && report.kpi.memory_propose_calls == 0
    {
        hints.push("主动检索与提候选都是 0：检查三个工具的描述与注入块里「怎么用」的指路是否真的让模型愿意调用".to_string());
    }
    if report.comparison.sessions_with_both < report.comparison.required_sessions {
        hints.push(format!(
            "L2 配对会话不足（{} < {}）：先攒样本，别把现有差值当结论",
            report.comparison.sessions_with_both, report.comparison.required_sessions
        ));
    }
    if report.experiment.active && !report.experiment.metrics_available {
        hints.push(
            "L3 实验在跑但读数未出：等每组样本，或检查插件是否在报 experiment 事件".to_string(),
        );
    }
    out.push_str("\n## 9. 可复查的线索（自动生成，不是结论）\n\n");
    if hints.is_empty() {
        out.push_str("- （没有触发自动提示；先看第 1 节的数据健康）\n");
    } else {
        for hint in hints {
            out.push_str(&format!("- {hint}\n"));
        }
    }
}

fn enough(ok: bool) -> &'static str {
    if ok {
        "充足"
    } else {
        "不足"
    }
}

fn reason_count(report: &EffectReport, reason: &str) -> u64 {
    report
        .ledger
        .reasons
        .iter()
        .filter(|row| row.reason == reason && row.kind == "skip")
        .map(|row| row.count)
        .sum()
}

fn short_session(session: &str) -> String {
    let trimmed = session.strip_prefix("session-").unwrap_or(session);
    let mut out: String = trimmed.chars().take(8).collect();
    if trimmed.chars().count() > 8 {
        out.push('…');
    }
    if out.is_empty() {
        "—".to_string()
    } else {
        out
    }
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::effect::experiment;
    use crate::memory::effect::report;
    use crate::memory::effect::{DayAgg, EffectStateDoc, FileEntry, MemDayAgg};
    use crate::memory::jobs::JobsDoc;
    use crate::memory::store::{MemoryDoc, MemoryRecord};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 固定时刻，让日期断言不依赖真实时钟。
    const BASE_MS: u64 = 1_790_000_000_000;

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "memory-effect-snapshot-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 空账目也能出完整报告：快照的核心价值是「哪怕没数据也把口径与空态写清楚」。
    fn fixture_report() -> report::EffectReport {
        let experiment_state = experiment::state_of(&experiment::load(Path::new("")), BASE_MS);
        report::build_with_experiment(
            &EffectStateDoc::default(),
            &MemoryDoc::default(),
            &JobsDoc::new(),
            &[],
            &experiment_state,
            BASE_MS,
            7,
        )
    }

    /// 带一条「窗口内被注入过的记忆」：验标题能进快照、正文与 store 字段绝不进。
    fn fixture_with_memory() -> report::EffectReport {
        let id = "a".repeat(32);
        let day = date_string_of_ms(BASE_MS);
        let mut state = EffectStateDoc::default();
        state.files.insert(
            "ws/session-a/session.v4.jsonl.zstd".to_string(),
            FileEntry {
                days: BTreeMap::from([(
                    day.clone(),
                    DayAgg {
                        injection_snapshots: 1,
                        injected_bytes: 128,
                        requests: 5,
                        memory_requests: 5,
                        ..Default::default()
                    },
                )]),
                memories: BTreeMap::from([(
                    id.clone(),
                    BTreeMap::from([(
                        day,
                        MemDayAgg {
                            injections: 1,
                            exposures: 5,
                            ..Default::default()
                        },
                    )]),
                )]),
                ..Default::default()
            },
        );
        let memory = MemoryDoc {
            memories: vec![MemoryRecord {
                id,
                title: "快照标题可用".to_string(),
                body: "正文不该进快照".to_string(),
                scope: "project".to_string(),
                verification: "verified".to_string(),
                state: "accepted".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let experiment_state = experiment::state_of(&experiment::load(Path::new("")), BASE_MS);
        report::build_with_experiment(
            &state,
            &memory,
            &JobsDoc::new(),
            &[],
            &experiment_state,
            BASE_MS,
            7,
        )
    }

    /// 带 skip / inject 两行账本：摘要第 3 节要把两类分开（inject 不是「未注入原因」）。
    fn fixture_with_ledger_notes() -> report::EffectReport {
        use crate::memory::effect::ledger::{LedgerNote, KIND_INJECT, KIND_SKIP};
        let skip = LedgerNote {
            at_ms: BASE_MS,
            session: "session-ledger".to_string(),
            turn: 3,
            kind: KIND_SKIP.to_string(),
            reason: "budget".to_string(),
            ..Default::default()
        };
        let inject = LedgerNote {
            at_ms: BASE_MS,
            session: "session-ledger".to_string(),
            turn: 4,
            kind: KIND_INJECT.to_string(),
            reason: "turn".to_string(),
            ..Default::default()
        };
        let experiment_state = experiment::state_of(&experiment::load(Path::new("")), BASE_MS);
        report::build_with_experiment(
            &EffectStateDoc::default(),
            &MemoryDoc::default(),
            &JobsDoc::new(),
            &[skip, inject],
            &experiment_state,
            BASE_MS,
            7,
        )
    }

    fn fixture_paths(dir: &Path) -> SnapshotPaths {
        SnapshotPaths {
            json: dir.join("report.json"),
            markdown: dir.join("report.md"),
        }
    }

    #[test]
    fn writes_json_envelope_and_markdown_digest() {
        let dir = scratch("write");
        write_to(&fixture_paths(&dir), &fixture_with_memory()).expect("写快照应成功");

        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("report.json")).expect("读 json"))
                .expect("快照 JSON 应可解析");
        assert_eq!(json["schema"], SNAPSHOT_SCHEMA);
        assert_eq!(json["kind"], SNAPSHOT_KIND);
        assert_eq!(json["range_days"], 7);
        assert_eq!(json["report"]["range_days"], 7);
        assert!(json["refresh"]
            .as_str()
            .expect("refresh 文案")
            .contains("效果页"));
        assert!(json["report"]["window"]["from"].is_string());
        // 具体读数也要过一遍：只断言小节标题存在时，两个字段互换都照样绿。
        assert_eq!(
            json["report"]["kpi"]["injection_snapshots"].as_u64(),
            Some(1)
        );
        assert_eq!(json["report"]["kpi"]["injected_bytes"].as_u64(), Some(128));

        let markdown = fs::read_to_string(dir.join("report.md")).expect("读 md");
        assert!(
            markdown.contains("- 注入快照 1 次、注入字节 128"),
            "摘要读数应来自同一份报告"
        );
        assert!(markdown.contains("快照标题可用"), "榜单应使用记忆库标题");
        for heading in [
            "# 记忆效果快照（agent 可读）",
            "## 1. 数据可信度",
            "## 2. 核心读数",
            "## 3. 未注入原因",
            "## 4. L2 同会话配对",
            "## 5. L3 会话级随机对照",
            "## 6. 提取质量",
            "## 7. 记忆榜单",
            "## 8. 给后续开发者 / agent 的读法",
            "## 9. 可复查的线索",
        ] {
            assert!(markdown.contains(heading), "缺段落：{heading}");
        }
    }

    #[test]
    fn empty_window_reports_sample_shortage_and_no_fake_ratio() {
        let dir = scratch("empty");
        write_to(&fixture_paths(&dir), &fixture_report()).expect("写快照应成功");
        let markdown = fs::read_to_string(dir.join("report.md")).expect("读 md");
        assert!(
            markdown.contains("请求样本不足"),
            "空窗口应说明请求样本不足而不是给 0% 比率"
        );
        assert!(
            !markdown.contains("携带记忆的请求 0 次（0.0%）"),
            "样本不足时不给比率"
        );
        assert!(markdown.contains("窗口内没有可读会话"));
    }

    #[test]
    fn snapshot_keeps_titles_but_never_memory_bodies() {
        let dir = scratch("privacy");
        let report = fixture_with_memory();
        assert!(!report.leaderboard.is_empty(), "夹具应产生一条榜单");
        write_to(&fixture_paths(&dir), &report).expect("写快照应成功");

        let markdown = fs::read_to_string(dir.join("report.md")).expect("读 md");
        let json = fs::read_to_string(dir.join("report.json")).expect("读 json");
        assert!(markdown.contains("快照标题可用"), "摘要应能引用标题");
        assert!(json.contains("快照标题可用"), "JSON 榜单应带标题");
        for text in [&markdown, &json] {
            assert!(!text.contains("正文不该进快照"), "正文绝不能进快照");
        }
        for forbidden in ["\"body\"", "\"keywords\"", "\"sources\""] {
            assert!(!json.contains(forbidden), "JSON 不该出现 {forbidden}");
        }
    }

    #[test]
    fn digest_separates_skip_reasons_from_inject_triggers() {
        let dir = scratch("ledger-digest");
        write_to(&fixture_paths(&dir), &fixture_with_ledger_notes()).expect("写快照应成功");
        let markdown = fs::read_to_string(dir.join("report.md")).expect("读 md");
        assert!(
            markdown.contains("未注入原因（skip × reason"),
            "skip 原因要单列：{markdown}"
        );
        assert!(markdown.contains("skip / budget"), "skip 原因行应出现");
        assert!(
            markdown.contains("注入触发路径（inject × reason"),
            "inject 触发路径应另起一列"
        );
        assert!(markdown.contains("inject / turn"), "inject 触发行应出现");
    }

    /// 生产入口（命令实际调用的函数）要真的把两个文件写进实例目录：光测 write_to
    /// 会把「路径解析错 / 设错目录 / 写失败被吞」这三类问题全部漏掉。
    #[test]
    fn production_entry_writes_snapshots_under_the_instance_effect_dir() {
        let home = scratch("entry-home");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let report = report_and_write(Some(7)).expect("生产入口应返回报告");
        let snapshot = snapshot_paths(&Namespace::current());

        assert!(
            snapshot.json.exists(),
            "report.json 没写出来：{}",
            snapshot.json.display()
        );
        assert!(
            snapshot.markdown.exists(),
            "report.md 没写出来：{}",
            snapshot.markdown.display()
        );
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&snapshot.json).expect("读 report.json"))
                .expect("report.json 可解析");
        assert_eq!(json["schema"], SNAPSHOT_SCHEMA);
        assert_eq!(
            json["report"]["range_days"].as_u64(),
            Some(report.range_days as u64)
        );
        for (path, suffix) in [
            (&snapshot.json, "effect/report.json"),
            (&snapshot.markdown, "effect/report.md"),
        ] {
            let text = path.to_string_lossy();
            assert!(text.contains("memory"), "路径应在实例记忆目录下：{text}");
            assert!(text.ends_with(suffix), "路径后缀不对：{text}");
        }
    }

    #[test]
    fn write_failure_carries_an_actionable_next_step() {
        let dir = scratch("failure");
        let blocker = dir.join("not-a-dir");
        fs::write(&blocker, b"x").expect("占位文件");
        let paths = SnapshotPaths {
            json: blocker.join("report.json"),
            markdown: blocker.join("report.md"),
        };
        let error = write_to(&paths, &fixture_report()).expect_err("父路径是文件时应失败");
        let text = error.to_string();
        assert!(
            text.contains("写入记忆效果快照失败"),
            "错误应指出动作：{text}"
        );
        assert!(text.contains("磁盘空间"), "错误应给出可操作下一步：{text}");
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_files_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        write_to(&fixture_paths(&dir), &fixture_report()).expect("写快照应成功");
        for file in ["report.json", "report.md"] {
            let mode = fs::metadata(dir.join(file))
                .expect("取元数据")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{file} 应是 0600，实际 {mode:o}");
        }
    }
}
