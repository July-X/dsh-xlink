//! 给**已有成熟状态文件**的诊断操作补运行记录：恢复与二分。
//!
//! ## 为什么单独一个模块
//!
//! 启动与预检各自有专门的编排层（`startup_run` / `precheck`），所以它们的
//! 阶段事件是就地打的。而恢复与二分的判定逻辑**早就写好并各自有状态文件**
//!（`restore.rs` 的 `RestoreOutcome`、`bisect.rs` 的 session），在它们内部
//! 插阶段事件会侵入那两条成熟路径——它们有各自的测试、各自的边界，重新
//! 接线风险远大于收益。
//!
//! 所以这两条链走**旁路**：命令层调它们前后各打一次事件，结束时按它们的
//! **终态**（而不是「调用成功」）定这次运行的状态。旁路的代价是时间线粒度
//! 粗——只有「开始 / 各自步骤 / 结束」，没有每个内部阶段的耗时。
//!
//! ## 终态从哪来
//!
//! 恢复看 `RestoreOutcome.verification`（三态：verified / failed /
//! not-needed）与 `skipped`；二分看 `Conclusion.kind`。**不按「函数返回了
//! Ok」记账**——二分跑完返回 Ok 恰恰意味着「收敛到一个可疑集合」，那是
//! 排查的正常终点，不是成功也不是失败。

use std::path::Path;

use crate::diagnostics::run;

/// 恢复的阶段标识。
pub mod restore_stage {
    pub const PREPARE: &str = "prepare";
    pub const APPLY: &str = "apply";
    pub const VERIFY: &str = "verify";
    pub const BACKUP: &str = "backup";
}

/// 二分的阶段标识。
pub mod bisect_stage {
    pub const SELECT: &str = "select";
    pub const PROBE: &str = "probe";
    pub const CONCLUDE: &str = "conclude";
    pub const ABORT: &str = "abort";
}

/// 开一条恢复运行记录。
///
/// **内核版本在这里读而不是让调用方传**：`read_active` 是 `data_dir` 的纯
/// 函数，而命令层每次都要多算一个局部变量再多写一个实参。收进来之后调用面
/// 从五个参数降到四个，那一行才缩得进 `commands.rs` 的行宽里——而那份文件
/// 受反棘轮约束，只能变小。
pub fn begin_restore(
    family: &str,
    instance: &str,
    profile: &str,
    data_dir: &Path,
) -> run::Recorder {
    run::Recorder::begin(
        family,
        instance,
        run::kind::RESTORE,
        &crate::kernel::lifecycle::read_active(data_dir).unwrap_or_default(),
        profile,
        run::pinned_from_incident(data_dir),
    )
}

/// 开一条二分运行记录。
///
/// 返回的 recorder 还**不落盘**（`run::Recorder` 的约定），调用方要先把
/// [`crate::diagnostics::bisect::begin`] 拿到它的 id 存进会话，再推第一条
/// 事件——顺序反了会留下一条没有归属的空记录。
pub fn begin_bisect(family: &str, instance: &str, profile: &str, data_dir: &Path) -> run::Recorder {
    run::Recorder::begin(
        family,
        instance,
        run::kind::BISECT,
        &crate::kernel::lifecycle::read_active(data_dir).unwrap_or_default(),
        profile,
        Default::default(),
    )
}

/// 接上一条已落盘但还没收尾的记录。
///
/// 二分的每轮试探是一次独立命令调用，recorder 活不过命令边界；会话里存着
/// id，这里按 id 把它读回来续写。**读不到就返回 `None` 而不是新开一条**：
/// 新开一条会让一次排查在时间线上裂成两段，而"这段试探没进记录"是可以
/// 接受的降级（记录是旁路，主流程不受影响）。
pub fn attach_run(family: &str, instance: &str, run_id: Option<&str>) -> Option<run::Recorder> {
    let run_id = run_id?;
    let loaded = run::get(family, instance, run_id)?;
    Some(run::Recorder::attach(family, instance, loaded))
}

/// 二分开始：把候选规模写进时间线的第一条事件。
pub fn push_bisect_select(
    recorder: &mut run::Recorder,
    view: &crate::diagnostics::bisect::BisectView,
) {
    recorder.push(
        bisect_stage::SELECT,
        run::status::RUNNING,
        &format!(
            "候选 {} 个，按嫌疑度分 {} 轮左右",
            view.candidate_count,
            crate::diagnostics::bisect::rounds_estimate(view.candidate_count)
        ),
        None,
        None,
    );
}

/// 恢复结束：按 `RestoreOutcome` 定终态。
///
/// **「有跳过项」单独降一档到 `warning` 而不是 `success`**：恢复返回
/// Verified 但有一半条目跳不过去，用户以为环境干净了，实际上还有东西停在
/// 恢复前的状态——那正是恢复失败最难排查的一种形态。
pub fn finish_restore(
    recorder: &mut run::Recorder,
    outcome: &crate::diagnostics::restore::RestoreOutcome,
    data_dir: &Path,
) {
    use crate::diagnostics::restore::Verification;
    let skipped = outcome.skipped.len();
    let applied = outcome.applied.len();
    recorder.push(
        restore_stage::APPLY,
        if applied > 0 {
            run::status::SUCCESS
        } else {
            run::status::WARNING
        },
        &format!("已恢复 {applied} 项，跳过 {skipped} 项"),
        None,
        None,
    );
    let verified = matches!(
        outcome.verification,
        Verification::Verified | Verification::NotNeeded
    );
    recorder.push(
        restore_stage::VERIFY,
        if verified {
            run::status::SUCCESS
        } else {
            run::status::FAILURE
        },
        &if outcome.verification_detail.is_empty() {
            format!("恢复后自检：{}", verification_label(outcome.verification))
        } else {
            format!("恢复后自检：{}", outcome.verification_detail)
        },
        None,
        None,
    );
    // 作用域从 recorder 自己读，而不是再让调用方传一遍 family / instance：
    // 那两个值调用方手上也是从 `default_instance_key()` 现取的，多传一遍就
    // 多一次「两边不一致」的机会，而不一致的代价是证据路径指向别人的日志。
    let evidence = run::RunEvidence {
        kernel_log: crate::kernel::lifecycle::current_kernel_log_path(
            data_dir,
            &recorder.run().family,
            &recorder.run().instance_id,
        )
        .display()
        .to_string()
        .into(),
        incident_id: None,
        sandbox_log: None,
    };
    // 归因：自检不通过归 environment（配置层的问题），跳过项归 plugin
    // （那些条目本身就是插件 / 技能）。落到 unknown 会让用户在插件与配置
    // 之间无从下手，而这两种成因各占一半。
    let (status, cause) = match (verified, skipped > 0) {
        (false, _) => (run::status::FAILURE, run::cause::ENVIRONMENT),
        (true, true) => (run::status::WARNING, run::cause::PLUGIN),
        (true, false) => (run::status::SUCCESS, run::cause::UNKNOWN),
    };
    let _ = recorder.finish(status, cause, &outcome.verification_detail, Some(&evidence));
}

/// `Verification` 的中文名。三态各有各的说法——`NotNeeded` 不是「通过」。
fn verification_label(value: crate::diagnostics::restore::Verification) -> &'static str {
    use crate::diagnostics::restore::Verification;
    match value {
        Verification::Verified => "起过一次内核，正常应答",
        Verification::Failed => "装上恢复后的配置仍起不来",
        Verification::NotNeeded => "本次没有改动，因此没起内核验证",
    }
}

/// 恢复前置检查失败（环境不可用）时的收尾。**返回原错误**：调用方要用它做
/// `.map_err`，这样命令层不必为「记一笔」多写一段提前返回。
pub fn fail_restore_environment(recorder: &mut run::Recorder, reason: &str) -> String {
    recorder.push(
        restore_stage::PREPARE,
        run::status::FAILURE,
        reason,
        None,
        None,
    );
    let _ = recorder.finish(run::status::FAILURE, run::cause::ENVIRONMENT, reason, None);
    reason.to_string()
}

/// 恢复跑完：记账并把结果原样交回调用方。
///
/// **整段编排住在这份模块里而不是 `commands.rs`**：那份文件是全仓受反棘轮
/// 约束的大文件，只许变小。「开记录 → 跑 → 按终态收尾」这套三步每多接一种
/// kind 就多几十行，而它本来就属于诊断层。
pub fn close_restore(
    recorder: &mut run::Recorder,
    outcome: Result<crate::diagnostics::restore::RestoreOutcome, crate::shell::error::AppError>,
    data_dir: &Path,
) -> Result<crate::diagnostics::restore::RestoreOutcome, String> {
    match outcome {
        Ok(outcome) => {
            finish_restore(recorder, &outcome, data_dir);
            Ok(outcome)
        }
        Err(error) => {
            // 恢复失败**别一律说成「环境坏了」**：`restore` 返回 Err 有可能是
            // 目标快照本身缺条目，也有可能是中途装不上。归因写 unknown，让
            // 用户自己看时间线里的原文，而不是被一个看着笃定的标签带偏。
            recorder.push(
                restore_stage::APPLY,
                run::status::FAILURE,
                &error.to_string(),
                None,
                None,
            );
            let _ = recorder.finish(
                run::status::FAILURE,
                run::cause::UNKNOWN,
                &error.to_string(),
                None,
            );
            Err(error.to_string())
        }
    }
}

/// 二分每一轮试探。
///
/// `verdict` 与 `probe_result` 由调用方从沙盒判据直接映射，不在这里猜——
/// 「没试成」（Inconclusive）是第三种结果，折叠进「起不来」会让用户以为
/// 那一半已被排除，而它其实根本没被验证过。
pub fn push_bisect_round(
    recorder: &mut run::Recorder,
    round: u32,
    total: u32,
    probe: &str,
    status: &str,
    probe_result: &str,
) {
    recorder.push(
        bisect_stage::PROBE,
        status,
        &format!("第 {round}/{total} 轮：{probe} → {probe_result}"),
        Some(round),
        None,
    );
}

/// 沙盒判据 → (记录状态, 中文结论) 的唯一映射。
///
/// 二分命令层与这份记录用的是**同一份映射**：两边各写一套的话，界面上一轮
/// 「起不来」而时间线里可能写着「已排除」，用户对不上该信哪个。
pub fn probe_verdict(verdict: crate::plugins::sandbox::Verdict) -> (&'static str, &'static str) {
    use crate::plugins::sandbox::Verdict;
    match verdict {
        Verdict::Pass => (run::status::SUCCESS, "能起来"),
        Verdict::Fail => (run::status::FAILURE, "起不来"),
        Verdict::Inconclusive => (run::status::WARNING, "这一轮没试成，不能算排除"),
    }
}

/// 二分结束。
///
/// **文案绝不称「根因」**：组合效应会让二分停在不可修的答案上，正确的说法
/// 是「最小可疑集合」或「当前证据指向」（设计 §11.1）。
pub fn finish_bisect(recorder: &mut run::Recorder, view: &crate::diagnostics::bisect::BisectView) {
    let Some(conclusion) = &view.conclusion else {
        // 没有结论 = 用户还开着会话。记成 running 之外的状态会让概览的
        // 「最近一次操作」显示成已结束，而排查还在进行。
        recorder.push(
            bisect_stage::CONCLUDE,
            run::status::INCONCLUSIVE,
            "排查未收出结论（会话仍开着）",
            None,
            None,
        );
        let _ = recorder.finish(
            run::status::INCONCLUSIVE,
            run::cause::UNKNOWN,
            "排查未收出结论",
            None,
        );
        return;
    };
    let aborted = conclusion.kind == "aborted";
    recorder.push(
        if aborted {
            bisect_stage::ABORT
        } else {
            bisect_stage::CONCLUDE
        },
        if aborted {
            run::status::CANCELED
        } else {
            run::status::SUCCESS
        },
        &conclusion.text,
        None,
        None,
    );
    let (status, summary) = if aborted {
        (run::status::CANCELED, conclusion.text.clone())
    } else {
        (run::status::SUCCESS, conclusion.text.clone())
    };
    // 收出的是「最小可疑集合」，不是根因——cause 记 plugin 但文案里必须
    // 保留「当前证据指向」这个限定，否则用户会去逐个删除一个可能无辜的包。
    let _ = recorder.finish(
        status,
        if aborted {
            run::cause::UNKNOWN
        } else {
            run::cause::PLUGIN
        },
        &summary,
        None,
    );
}

/// 二分中途失败（环境不可用、候选太少等）。
pub fn fail_bisect(recorder: &mut run::Recorder, reason: &str) {
    recorder.push(
        bisect_stage::SELECT,
        run::status::FAILURE,
        reason,
        None,
        None,
    );
    let _ = recorder.finish(
        run::status::INCONCLUSIVE,
        run::cause::ENVIRONMENT,
        reason,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::restore::{RestoreOutcome, Verification};
    use crate::tests::scoped_xlink_home;

    fn temp_home(tag: &str) -> std::path::PathBuf {
        let home = std::env::temp_dir().join(format!("dsh-oprun-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("temp home");
        home
    }

    fn outcome(
        verification: Verification,
        detail: &str,
        applied: usize,
        skipped: usize,
    ) -> RestoreOutcome {
        RestoreOutcome {
            skipped: (0..skipped).map(|i| format!("条目 {i}")).collect(),
            applied: (0..applied).map(|i| format!("条目 {i}")).collect(),
            verification,
            verification_detail: String::from(detail),
            resulting_fingerprint: String::new(),
            backup_snapshot_id: String::new(),
        }
    }

    #[test]
    fn a_restore_with_skipped_items_is_warning_not_success() {
        // 恢复返回 Verified 但有一半条目跳不过去：用户以为环境干净了，实际
        // 还有东西停在恢复前——那是最难排查的一种形态，不能记成成功。
        let home = temp_home("restore-skip");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = begin_restore("dsh", "default", "web", &home);
        finish_restore(
            &mut recorder,
            &outcome(Verification::Verified, "工作台起来了", 5, 2),
            &home,
        );
        let loaded = run::get("dsh", "default", recorder.id()).expect("已落盘");
        assert_eq!(loaded.status, run::status::WARNING);
        assert_eq!(loaded.cause, run::cause::PLUGIN);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_failed_verification_is_failure_and_environment() {
        let home = temp_home("restore-fail");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = begin_restore("dsh", "default", "web", &home);
        finish_restore(
            &mut recorder,
            &outcome(Verification::Failed, "恢复后内核仍起不来", 5, 0),
            &home,
        );
        let loaded = run::get("dsh", "default", recorder.id()).expect("已落盘");
        assert_eq!(loaded.status, run::status::FAILURE);
        assert_eq!(loaded.cause, run::cause::ENVIRONMENT);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn not_needed_is_not_shown_as_passed() {
        // 「没测」画成「测过没问题」是这类工具最容易犯也最伤害信任的错。
        let home = temp_home("restore-notneeded");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = begin_restore("dsh", "default", "web", &home);
        finish_restore(
            &mut recorder,
            &outcome(Verification::NotNeeded, "", 0, 0),
            &home,
        );
        let events = recorder.run().events.clone();
        let verify = events
            .iter()
            .find(|e| e.stage == restore_stage::VERIFY)
            .expect("有自检阶段");
        assert!(
            verify.message.contains("没起内核验证"),
            "NotNeeded 必须说清「没测」，不能显示成通过：{}",
            verify.message
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn bisect_never_calls_its_conclusion_a_root_cause() {
        let home = temp_home("bisect");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = begin_bisect("dsh", "default", "web", &home);
        // 第三种判据必须有自己的说法：Inconclusive 折叠进「起不来」会让用户
        // 以为那半边已被排除，而它根本没被验证过。
        for (round, verdict) in [
            (1, crate::plugins::sandbox::Verdict::Fail),
            (2, crate::plugins::sandbox::Verdict::Pass),
            (3, crate::plugins::sandbox::Verdict::Inconclusive),
        ] {
            let (status, label) = probe_verdict(verdict);
            push_bisect_round(&mut recorder, round, 3, "启用 a、b", status, label);
        }
        let view = crate::diagnostics::bisect::BisectView {
            running: false,
            probe: String::new(),
            k: 0,
            candidate_count: 3,
            remaining: 1,
            cleared: Vec::new(),
            steps: Vec::new(),
            rounds: 2,
            conclusion: Some(crate::diagnostics::bisect::Conclusion {
                kind: String::from("narrowed"),
                members: vec![String::from("b")],
                text: String::from("最小可疑集合：b（当前证据指向，未必是根因）"),
            }),
            started_at_ms: 0,
        };
        finish_bisect(&mut recorder, &view);
        let loaded = run::get("dsh", "default", recorder.id()).expect("已落盘");
        assert_eq!(loaded.status, run::status::SUCCESS);
        assert_eq!(loaded.events.len(), 4, "三轮试探 + 一个结论");
        // 轮次序号要保留：三轮判据不同，但光看 status 分不出第几轮。
        assert_eq!(loaded.events[0].attempt, Some(1));
        assert_eq!(loaded.events[1].attempt, Some(2));
        // 「没试成」不能画成「起不来」——两者的下一步动作完全相反。
        assert_eq!(loaded.events[2].status, run::status::WARNING);
        assert!(
            loaded.events[2].message.contains("没试成"),
            "第三轮要说清没试成：{}",
            loaded.events[2].message
        );

        // 收不出结论（会话还开着）要记 inconclusive 而不是 success——
        // 否则概览的「最近一次操作」会显示成已结束，而排查还在跑。
        let mut recorder2 = begin_bisect("dsh", "default", "web", &home);
        let mut open_view = view;
        open_view.conclusion = None;
        finish_bisect(&mut recorder2, &open_view);
        let loaded2 = run::get("dsh", "default", recorder2.id()).expect("已落盘");
        assert_eq!(loaded2.status, run::status::INCONCLUSIVE);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_aborted_bisect_is_canceled_not_success() {
        let home = temp_home("bisect-abort");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = begin_bisect("dsh", "default", "web", &home);
        let view = crate::diagnostics::bisect::BisectView {
            running: false,
            probe: String::new(),
            k: 0,
            candidate_count: 4,
            remaining: 2,
            cleared: Vec::new(),
            steps: Vec::new(),
            rounds: 1,
            conclusion: Some(crate::diagnostics::bisect::Conclusion {
                kind: String::from("aborted"),
                members: Vec::new(),
                text: String::from("用户中止了排查"),
            }),
            started_at_ms: 0,
        };
        finish_bisect(&mut recorder, &view);
        let loaded = run::get("dsh", "default", recorder.id()).expect("已落盘");
        assert_eq!(loaded.status, run::status::CANCELED);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_bisect_run_survives_the_command_boundary_it_spans() {
        // 二分是 start / probe ×N / abort 一串独立命令，recorder 活不过命令
        // 边界。这条测试走的是真实形状：begin → 存 id → attach → 续写 →
        // 收尾，断言最后落盘的**同一条**记录里既有前一条命令的事件也有后一条。
        let home = temp_home("bisect-attach");
        let _guard = scoped_xlink_home(&home);
        let mut first = begin_bisect("dsh", "default", "web", &home);
        push_bisect_select(
            &mut first,
            &crate::diagnostics::bisect::BisectView {
                running: true,
                probe: String::from("startup-failed"),
                k: 1,
                candidate_count: 6,
                remaining: 6,
                cleared: Vec::new(),
                steps: Vec::new(),
                rounds: 0,
                conclusion: None,
                started_at_ms: 0,
            },
        );
        let run_id = first.id().to_string();
        drop(first); // 命令结束，recorder 出作用域

        // 第二条命令按 id 接回来。
        let mut second = attach_run("dsh", "default", Some(&run_id)).expect("能接上");
        assert_eq!(second.id(), run_id, "接的是同一条记录，不是新开一条");
        let (status, label) = probe_verdict(crate::plugins::sandbox::Verdict::Fail);
        push_bisect_round(&mut second, 1, 3, "启用 a、b、c", status, label);
        let (status, label) = probe_verdict(crate::plugins::sandbox::Verdict::Pass);
        push_bisect_round(&mut second, 2, 3, "启用 d、e、f", status, label);
        finish_bisect(
            &mut second,
            &crate::diagnostics::bisect::BisectView {
                running: false,
                probe: String::from("startup-failed"),
                k: 1,
                candidate_count: 6,
                remaining: 3,
                cleared: vec![String::from("a"), String::from("b")],
                steps: Vec::new(),
                rounds: 2,
                conclusion: Some(crate::diagnostics::bisect::Conclusion {
                    kind: String::from("narrowed"),
                    members: vec![String::from("d")],
                    text: String::from("最小可疑集合：d（当前证据指向，未必是根因）"),
                }),
                started_at_ms: 0,
            },
        );

        let loaded = run::get("dsh", "default", &run_id).expect("已落盘");
        assert_eq!(loaded.status, run::status::SUCCESS);
        assert_eq!(
            loaded.events.len(),
            4,
            "第一条命令的 select + 两条命令各自的轮次 + 结论，都得在同一条记录里"
        );
        assert_eq!(loaded.events[0].stage, bisect_stage::SELECT);
        assert_eq!(loaded.events[3].stage, bisect_stage::CONCLUDE);
        // 序列必须连续：跨命令续写时 seq 从已落盘的长度接着数。
        for (index, event) in loaded.events.iter().enumerate() {
            assert_eq!(event.seq, index as u32 + 1, "第 {index} 条的 seq 断了");
        }

        // 接不上时安静降级，绝不新开一条：时间线少一轮试探，排查照常跑完。
        assert!(attach_run("dsh", "default", Some("run-does-not-exist")).is_none());
        assert!(attach_run("dsh", "default", None).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_unknown_stage_from_a_newer_build_is_kept_verbatim() {
        // 降级安装的机器上，新版写的记录会被旧版读。时间线遇到不认识的 stage
        // 必须原样显示成 stage 字符串，而不是悄悄换成"未知阶段"——那会抹掉
        // 「这条事件到底叫什么」这个唯一能帮用户自查版本的线索。
        let home = temp_home("unknown-stage");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = begin_bisect("dsh", "default", "web", &home);
        recorder.push(
            "quarantine-v3",
            run::status::WARNING,
            "未来的阶段",
            None,
            None,
        );
        recorder
            .finish(run::status::SUCCESS, run::cause::UNKNOWN, "ok", None)
            .expect("finish");
        let loaded = run::get("dsh", "default", recorder.id()).expect("已落盘");
        assert_eq!(loaded.events[0].stage, "quarantine-v3");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn restore_and_bisect_round_trip_through_the_same_model() {
        // 两种 kind 共用一套状态与归因词表，正是「一次操作 = 一条记录」的
        // 意义：用户在概览的「最近一次操作」里能按同一套口径读它们。
        let home = temp_home("kinds");
        let _guard = scoped_xlink_home(&home);
        for kind in [run::kind::RESTORE, run::kind::BISECT] {
            let mut rec =
                run::Recorder::begin("dsh", "default", kind, "0.2.1", "web", Default::default());
            rec.push("a-stage", run::status::SUCCESS, "步骤", None, None);
            rec.finish(run::status::SUCCESS, run::cause::UNKNOWN, "ok", None)
                .expect("finish");
            let loaded = run::get("dsh", "default", rec.id()).expect("已落盘");
            assert_eq!(loaded.kind, kind);
            assert_eq!(loaded.events[0].stage, "a-stage");
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}
