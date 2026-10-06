//! 启动诊断的编排：从开记录到收尾归因，中间只管「把阶段事件打进时间线」。
//!
//! ## 为什么单独一个模块
//!
//! `commands::start_kernel_blocking` 是本仓最大的文件之一，且在代码预算的
//! **反棘轮**上（基线超 600 行，预算只许下调）。启动诊断要往启动流程里插
//! 十几个阶段事件 + 一套收尾归因，这些逻辑「只在启动诊断这一条路径上成立」，
//! 挤进 `commands.rs` 只会让那个文件继续变大。本模块把「记录怎么长」这件事
//! 收在一处，`commands.rs` 只剩编排骨架。
//!
//! ## 与看护的分工
//!
//! 看护内部（`GuardDeps::run_push`）只落盘、不推通道；它攒下的信封由
//! [`flush`] 在 `guarded_start` 返回后一次补发。见 `GuardDeps::run_push`
//! 的注释解释为什么不能就地推。

use std::path::{Path, PathBuf};

use crate::diagnostics::guard::{StartReport, WatchPhase};
use crate::diagnostics::run;

/// 把一条启动阶段事件送进通道与运行记录。
///
/// **刻意用宏而不是闭包**：进度回调是 `&mut dyn FnMut(&str)`，任何捕获它的
/// 闭包都会独占这份可变借用，而 `guarded_start` 与 `promise_pnpm` 还要继续
/// 用它。宏在展开点直接用 `$send(...)`，借用只在那一瞬间存在。
///
/// 通道里只发**一个**信封（`run::channel_envelope`）：进度浮层取它的
/// `message` 显示人话，诊断时间线取整个事件。发两条（一条纯文本一条
/// JSON）会让日志区把同一句话记两遍。
macro_rules! stage {
    ($send:expr, $recorder:expr, $stage:expr, $status:expr, $message:expr, $attempt:expr $(,)?) => {{
        $recorder.push($stage, $status, $message, $attempt, None);
        for envelope in $recorder.take_pending() {
            $send(&envelope);
        }
    }};
}

/// 开一条启动运行记录。
///
/// 记录从**解析实例之后**才开：Node 探测与 pnpm 解析也要进时间线，但它们的
/// family / instance 需要先解析一次。`pinned_from_incident` 在这里传入，
/// 是因为它要读 `data_dir`——事故文件按壳的 data_dir 存放，而本模块其余部分
/// 一律按 (family, instance) 定位，两条路径的交点只有调用方知道。
pub(crate) fn begin(
    send: &mut dyn FnMut(&str),
    family: &str,
    instance: &str,
    kernel_version: &str,
    profile: &str,
    data_dir: &Path,
) -> run::Recorder {
    let mut recorder = run::Recorder::begin(
        family,
        instance,
        run::kind::STARTUP,
        kernel_version,
        profile,
        run::pinned_from_incident(data_dir),
    );
    passed(
        send,
        &mut recorder,
        run::stage::RESOLVE_INSTANCE,
        &format!("已解析实例 {instance}"),
    );
    recorder
}

/// 记录一个已通过的前置阶段。
pub(crate) fn passed(
    send: &mut dyn FnMut(&str),
    recorder: &mut run::Recorder,
    stage: &str,
    message: &str,
) {
    stage!(send, recorder, stage, run::status::SUCCESS, message, None);
}

/// 记录一个进行中的阶段。
pub(crate) fn running(
    send: &mut dyn FnMut(&str),
    recorder: &mut run::Recorder,
    stage: &str,
    message: &str,
) {
    stage!(send, recorder, stage, run::status::RUNNING, message, None);
}

/// 记录一个失败阶段并结束运行记录，归因为环境类。
///
/// **启动失败和记录失败是两件事**：这里先如实结束记录，调用方再把原始错误
/// 原样抛出——不能因为多了记录层而改写用户的启动结果。
pub(crate) fn fail_environment(recorder: &mut run::Recorder, stage: &str, reason: &str) {
    recorder.push(stage, run::status::FAILURE, reason, None, None);
    let _ = recorder.finish(run::status::FAILURE, run::cause::ENVIRONMENT, reason, None);
}

/// 看护返回后补发它攒下的信封，让实时时间线不丢任何阶段。
pub(crate) fn flush(send: &mut dyn FnMut(&str), recorder: &mut run::Recorder) {
    for envelope in recorder.take_pending() {
        send(&envelope);
    }
}

/// 启动成功后的收尾：注册子进程、确保事件订阅线程在跑。
///
/// 这两件事**不属于诊断**，但它们是「启动流程的尾部」，而看护不碰它们
/// （看护只管进程活没活起来）。放这里是为了让 `commands.rs` 的启动主体
/// 只剩「装配 + 调看护 + 收尾」。
pub(crate) fn after_started(
    app: &tauri::AppHandle,
    state: &crate::commands::AppState,
    data_dir: &Path,
    port: u16,
    child: Option<std::process::Child>,
    report: &mut StartReport,
) {
    if let Some(child) = child {
        // 非致命异常照样要进日志文件（stderr）与面板（report.warning）。
        if let Some(warning) = crate::commands::register_child(state, data_dir, port, child) {
            eprintln!("dsh-xlink: {warning}");
            report.warning = Some(warning);
        }
    }
    // 内核已经在服务（本次是新拉起的，或者端口上本来就有一个健康实例
    // ——`guarded_start` 的 no-op 分支）：确保事件订阅线程在跑。
    // `start_watcher` 幂等，重复调用不会叠加线程；反过来，"内核在跑却没
    // 有人订阅"就等于任务完成通知静默失效。
    if crate::kernel_running(app) {
        crate::notify::task::start_watcher(app);
    }
}

/// 事故文件的位置。
pub fn incident_path(data_dir: &Path) -> PathBuf {
    data_dir.join("last-incident.json")
}

/// 持久化故障信息，使其在 Shell 重启后仍然存在；尽力而为地写，因为写入
/// 失败不能掩盖用户正在等待的启动结果。
pub fn save_incident(data_dir: &Path, incident: &crate::diagnostics::guard::Incident) {
    crate::shell::state::save_best_effort(&incident_path(data_dir), incident);
}

/// 在一次干净、正常的启动后清除已记录的故障——否则这份陈旧的报告会与刚刚
/// 健康启动的工作台相矛盾。
pub fn clear_incident(data_dir: &Path) {
    let _ = std::fs::remove_file(incident_path(data_dir));
}

/// 读取最近一次记录的故障（供展示历史的命令使用）。
///
/// 读路径上会对旧记录重新判定一次「前端 bundle」这一类：判断与措辞的修复
/// 必须能作用到已经落盘的事故上，否则升级外壳后概览横幅仍在念旧的「暂未能
/// 归因」文案。重新判定是纯读操作：不写隔离、不改接线、也不回写文件。
pub(crate) fn load_incident(data_dir: &Path) -> Option<crate::diagnostics::guard::Incident> {
    let text = std::fs::read_to_string(incident_path(data_dir)).ok()?;
    let mut incident: crate::diagnostics::guard::Incident = serde_json::from_str(&text).ok()?;
    crate::diagnostics::guard::reclassify(&mut incident);
    Some(incident)
}

/// 安全网 P0：启动成功且**没有事故** → 打一个 `startup-ok` 快照。
///
/// 这是唯一能确立「上一个能跑起来的组合」的时点：安装完成、用户点确定都
/// 不代表能跑起来，只有真正起来并应答过才算。
///
/// **刻意要求 `incident.is_none()`**：带事故启动的实例不算"良好"——比如
/// 看护停用了两个插件才起来的环境，把它记成 last-known-good，等于让恢复时
/// 把"停用过的状态"当成用户原本的样子。
pub(crate) fn record_good_snapshot(
    send: &mut dyn FnMut(&str),
    recorder: &mut run::Recorder,
    report: &StartReport,
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
    port: u16,
) {
    if !report.running || report.incident.is_some() {
        return;
    }
    let outcome = crate::diagnostics::snapshot::record(
        data_dir,
        family,
        instance,
        profile,
        port,
        crate::diagnostics::snapshot::reason::STARTUP_OK,
    )
    .map(|_| ())
    .map_err(|error| error.to_string());
    if let Err(error) = &outcome {
        eprintln!("dsh-xlink: 记录启动快照失败：{error}");
    }
    match outcome {
        Ok(()) => stage!(
            send,
            recorder,
            run::stage::RECORD_STARTUP_OK,
            run::status::SUCCESS,
            "已保存本次成功启动的快照",
            None
        ),
        Err(error) => stage!(
            send,
            recorder,
            run::stage::RECORD_STARTUP_OK,
            run::status::WARNING,
            &format!("启动成功，但保存快照失败：{error}"),
            None
        ),
    }
}

/// 收尾：定终态、归因，并把运行 id 写回事故记录。
///
/// **三档必须分开，不能压成「成功 / 失败」两档**：安全模式启动与带事故启动
/// **都真的起来了**，把它们记成普通成功会让启动诊断说「一切正常」，而用户
/// 眼前的工作台正跑在停用了插件的状态里。
///
/// 事故 `cause` 可能为空（旧文件 / 未知归因）。空值会让运行记录在列表里没有
/// 任何归因标签，所以回退到 `unknown`——而不是让 cause 整个空着，前端对空
/// cause 显示不出任何东西。
pub(crate) fn finish(
    recorder: &mut run::Recorder,
    report: &mut StartReport,
    data_dir: &Path,
    family: &str,
    instance: &str,
    port: u16,
) {
    let evidence = run::RunEvidence {
        kernel_log: crate::kernel::lifecycle::current_kernel_log_path(data_dir, family, instance)
            .display()
            .to_string()
            .into(),
        incident_id: report.incident.as_ref().map(|_| recorder.id().to_string()),
        sandbox_log: None,
    };
    let (final_status, final_cause, final_summary): (&str, String, String) =
        match (&report.incident, report.running) {
            (Some(incident), true) => (
                run::status::WARNING,
                String::from(incident.cause.as_str()),
                incident.message.clone(),
            ),
            (Some(incident), false) => (
                run::status::FAILURE,
                String::from(incident.cause.as_str()),
                incident.message.clone(),
            ),
            (None, true) => (
                run::status::SUCCESS,
                String::from(run::cause::UNKNOWN),
                format!("工作台已启动，端口 {port}"),
            ),
            (None, false) => (
                run::status::INCONCLUSIVE,
                String::from(run::cause::UNKNOWN),
                String::from("工作台未在本次启动后运行，且没有留下可归因的事故记录"),
            ),
        };
    if let Err(error) = recorder.finish(final_status, &final_cause, &final_summary, Some(&evidence))
    {
        eprintln!("dsh-xlink: 写入启动诊断记录失败：{error}");
    }
    // 事故面板的「查看启动诊断」靠这条引用跳转；顺带让运行记录的裁剪知道
    // 这份记录不能删。
    if let Some(incident) = report.incident.as_mut() {
        incident.run_id = Some(recorder.id().to_string());
        let path = data_dir.join("last-incident.json");
        crate::shell::state::save_best_effort(&path, incident);
    }
}

/// 解析 pnpm 可执行文件，失败时结束记录并返回原始原因。
pub(crate) fn resolve_pnpm(
    send: &mut dyn FnMut(&str),
    recorder: &mut run::Recorder,
    data_dir: &Path,
    node_info: &crate::node::detect::NodeInfo,
) -> Result<PathBuf, String> {
    match crate::commands::promise_pnpm(data_dir, node_info, &mut *send) {
        Ok((_, pnpm_exe)) => {
            passed(send, recorder, run::stage::RESOLVE_PNPM, "已准备 pnpm");
            Ok(pnpm_exe)
        }
        Err(reason) => {
            fail_environment(recorder, run::stage::RESOLVE_PNPM, &reason);
            Err(reason)
        }
    }
}

/// 探测可用的 Node.js，失败时如实结束运行记录并返回原始原因。
///
/// **缓存失效判据**：缓存只在安装内核与托管 Node 时作废，而启动是低频动作，
/// 所以命中失败时强制重探一次——避免「检测 Node.js」刚报成功、「启动工作台」
/// 仍拿旧结论拒绝（用户在壳运行期间用安装器 / nvm 装好了 Node 的情形）。
pub(crate) fn resolve_node(
    send: &mut dyn FnMut(&str),
    recorder: &mut run::Recorder,
    state: &crate::commands::AppState,
    settings: &crate::shell::settings::Settings,
) -> Result<crate::node::detect::NodeInfo, String> {
    let mut info = crate::commands::cached_node(state, settings);
    if !info.ok {
        *crate::lock(&state.node_cache) = None;
        info = crate::commands::cached_node(state, settings);
    }
    if !info.ok {
        fail_environment(recorder, run::stage::DETECT_NODE, &info.reason);
        return Err(info.reason.clone());
    }
    passed(
        send,
        recorder,
        run::stage::DETECT_NODE,
        "检测到满足要求的 Node.js",
    );
    Ok(info)
}

/// 看护阶段 → 落盘事件的映射表。
///
/// **文案与阶段标识在这里，而不在看护里**：看护只说「发生了什么」，
/// 怎么说归诊断层管。新增一个启动阶段时改这张表即可，看护不动。
struct WatchText {
    stage: &'static str,
    running_status: &'static str,
    running_message: &'static str,
    done_status: &'static str,
    done_message: &'static str,
}

const fn watch_text(phase: WatchPhase) -> WatchText {
    match phase {
        WatchPhase::AlreadyRunning => WatchText {
            stage: run::stage::HEALTH_CHECK,
            running_status: run::status::RUNNING,
            running_message: "正在检查工作台状态",
            done_status: run::status::SUCCESS,
            done_message: "工作台已在运行，本次未重复启动",
        },
        WatchPhase::Spawning => WatchText {
            stage: run::stage::SPAWN_KERNEL,
            running_status: run::status::RUNNING,
            running_message: "正在派生内核进程",
            done_status: run::status::SUCCESS,
            done_message: "内核已在服务（端口已有应答）",
        },
        WatchPhase::AlreadyServing => WatchText {
            stage: run::stage::SPAWN_KERNEL,
            running_status: run::status::SUCCESS,
            running_message: "",
            done_status: run::status::SUCCESS,
            done_message: "内核已在服务（端口已有应答）",
        },
        WatchPhase::WaitingReady => WatchText {
            stage: run::stage::WAIT_READY,
            running_status: run::status::RUNNING,
            running_message: "内核进程已派生，等待端口就绪",
            done_status: run::status::SUCCESS,
            done_message: "端口已开始应答",
        },
        WatchPhase::PortReady => WatchText {
            stage: run::stage::WAIT_READY,
            running_status: run::status::SUCCESS,
            running_message: "",
            done_status: run::status::SUCCESS,
            done_message: "端口已开始应答",
        },
        WatchPhase::SpawnFailed => WatchText {
            stage: run::stage::SPAWN_KERNEL,
            running_status: run::status::FAILURE,
            running_message: "",
            done_status: run::status::FAILURE,
            done_message: "无法拉起内核进程",
        },
        WatchPhase::Exited => WatchText {
            stage: run::stage::WAIT_READY,
            running_status: run::status::FAILURE,
            running_message: "",
            done_status: run::status::FAILURE,
            done_message: "内核进程在就绪前退出",
        },
        WatchPhase::TimedOut => WatchText {
            stage: run::stage::WAIT_READY,
            running_status: run::status::FAILURE,
            running_message: "",
            done_status: run::status::FAILURE,
            done_message: "等待内核就绪超时",
        },
        WatchPhase::RetryPlugins => WatchText {
            stage: run::stage::RETRY,
            running_status: run::status::RUNNING,
            running_message: "检测到疑似引发故障的插件，正在停用后重试",
            done_status: run::status::SUCCESS,
            done_message: "已在停用嫌疑插件后重试",
        },
        WatchPhase::RetrySafeMode => WatchText {
            stage: run::stage::RETRY,
            running_status: run::status::RUNNING,
            running_message: "仍未启动成功，正在进入安全模式（停用全部第三方插件）后重试",
            done_status: run::status::SUCCESS,
            done_message: "已在安全模式下重试",
        },
        WatchPhase::EnvironmentBlocked => WatchText {
            stage: run::stage::HEALTH_CHECK,
            running_status: run::status::FAILURE,
            running_message: "",
            done_status: run::status::FAILURE,
            done_message: "检测到环境类问题，已跳过插件归因",
        },
        WatchPhase::Wiring => WatchText {
            stage: run::stage::PREPARE_WIRING,
            running_status: run::status::RUNNING,
            running_message: "正在准备插件接线",
            done_status: run::status::SUCCESS,
            done_message: "插件接线已就绪",
        },
        WatchPhase::WiringReady => WatchText {
            stage: run::stage::PREPARE_WIRING,
            running_status: run::status::SUCCESS,
            running_message: "",
            done_status: run::status::SUCCESS,
            done_message: "插件接线已就绪",
        },
    }
}

/// 启动诊断的阶段观察者：把看护的阶段翻译成运行记录里的事件。
///
/// **只落盘，不推通道。** 通道回调被 `guarded_start` 以 `&mut dyn FnMut(&str)`
/// 持有，而看护期间 `&mut Observer` 归这里独占；就地推通道就得让观察者
/// 长期借用那个回调，命令层随后连 `recorder.finish()` 都做不了。
///
/// 代价是看护期间的阶段事件**实时性差一拍**：它们在 `guarded_start` 返回后
/// 由 [`flush`] 一次补发。用户看到的仍是完整时间线，只是最后一个阶段晚
/// 几毫秒出现——远好过一次失败无法落盘。
pub struct WatchObserver<'a> {
    recorder: &'a mut run::Recorder,
}

impl<'a> WatchObserver<'a> {
    pub fn new(recorder: &'a mut run::Recorder) -> Self {
        Self { recorder }
    }
}

impl crate::diagnostics::guard::StageObserver for WatchObserver<'_> {
    fn on_stage(&mut self, phase: WatchPhase, attempt: Option<u32>) {
        let text = watch_text(phase);
        // `running_message` 为空表示这是**终态**阶段（一次性发生，没有
        // 「进行中」），只写一条；否则先写进行中再写完成。
        if text.running_message.is_empty() {
            self.recorder.push(
                text.stage,
                text.done_status,
                text.done_message,
                attempt,
                None,
            );
        } else {
            self.recorder.push(
                text.stage,
                text.running_status,
                text.running_message,
                attempt,
                None,
            );
            self.recorder.push(
                text.stage,
                text.done_status,
                text.done_message,
                attempt,
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    /// 造一份最小的启动报告：只有 `running` / `incident` 两项被测，其余给
    /// 中性值。`Incident` 有 `Default`（旧事故文件缺字段时本就需要默认值），
    /// 但 `StartReport` 没有——为一个测试 convenience 给它加 `Default`
    /// 会改动生产类型的契约。
    fn report(running: bool, incident: Option<crate::diagnostics::guard::Incident>) -> StartReport {
        StartReport {
            port: 3090,
            running,
            safe_mode: false,
            incident,
            warning: None,
        }
    }

    fn incident(message: &str, cause: &str) -> crate::diagnostics::guard::Incident {
        crate::diagnostics::guard::Incident {
            message: String::from(message),
            cause: String::from(cause),
            ..Default::default()
        }
    }

    fn temp_home(tag: &str) -> std::path::PathBuf {
        let home =
            std::env::temp_dir().join(format!("dsh-startup-run-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("temp home");
        home
    }

    #[test]
    fn a_stage_pushes_exactly_one_envelope_and_keeps_the_human_text() {
        let home = temp_home("envelope");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = run::Recorder::begin(
            "dsh",
            "default",
            run::kind::STARTUP,
            "0.2.1",
            "web",
            Default::default(),
        );
        let mut seen: Vec<String> = Vec::new();
        {
            let mut send = |msg: &str| seen.push(msg.to_string());
            running(
                &mut send,
                &mut recorder,
                run::stage::DETECT_NODE,
                "正在检查 Node.js",
            );
        }
        assert_eq!(
            seen.len(),
            1,
            "一个阶段只发一个信封，否则日志区会把同一句记两遍"
        );
        let parsed = run::channel_envelope(recorder.id(), &recorder.run().events[0]);
        assert_eq!(parsed, seen[0]);
        assert!(seen[0].contains("正在检查 Node.js"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn environment_failure_ends_the_run_without_touching_the_caller() {
        let home = temp_home("env-fail");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = run::Recorder::begin(
            "dsh",
            "default",
            run::kind::STARTUP,
            "0.2.1",
            "web",
            Default::default(),
        );
        fail_environment(&mut recorder, run::stage::DETECT_NODE, "未找到 Node.js");
        let loaded = run::get("dsh", "default", recorder.id()).expect("记录已落盘");
        assert_eq!(loaded.status, run::status::FAILURE);
        assert_eq!(loaded.cause, run::cause::ENVIRONMENT);
        assert_eq!(loaded.summary, "未找到 Node.js");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_workbench_that_started_under_an_incident_is_warning_not_success() {
        // 安全模式启动真的起来了，但它跑在停用了插件的状态里。记成 success
        // 会让诊断页说「一切正常」，而用户的插件正被停着。
        let home = temp_home("warning");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = run::Recorder::begin(
            "dsh",
            "default",
            run::kind::STARTUP,
            "0.2.1",
            "web",
            Default::default(),
        );
        let mut report = report(true, Some(incident("停用两个插件后才起来", "plugin")));
        finish(&mut recorder, &mut report, &home, "dsh", "default", 3090);
        let loaded = run::get("dsh", "default", recorder.id()).expect("记录已落盘");
        assert_eq!(loaded.status, run::status::WARNING);
        assert_eq!(loaded.cause, "plugin");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_workbench_that_did_not_run_is_inconclusive() {
        // 「没起来但没有事故」是证据不足，不是失败——画成失败会让用户去
        // 处置无辜插件。
        let home = temp_home("inconclusive");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = run::Recorder::begin(
            "dsh",
            "default",
            run::kind::STARTUP,
            "0.2.1",
            "web",
            Default::default(),
        );
        let mut report = report(false, None);
        finish(&mut recorder, &mut report, &home, "dsh", "default", 3090);
        let loaded = run::get("dsh", "default", recorder.id()).expect("记录已落盘");
        assert_eq!(loaded.status, run::status::INCONCLUSIVE);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn finish_writes_the_run_id_back_into_the_incident() {
        let home = temp_home("run-id");
        let _guard = scoped_xlink_home(&home);
        let mut recorder = run::Recorder::begin(
            "dsh",
            "default",
            run::kind::STARTUP,
            "0.2.1",
            "web",
            Default::default(),
        );
        let mut report = report(false, Some(incident("内核启动失败", "kernel")));
        finish(&mut recorder, &mut report, &home, "dsh", "default", 3090);
        let incident = report.incident.expect("事故仍在");
        assert_eq!(
            incident.run_id.as_deref(),
            Some(recorder.id()),
            "事故面板的「查看启动诊断」靠这条引用跳转"
        );
        assert!(
            home.join("last-incident.json").exists(),
            "事故文件要落盘，否则下次打开壳看不到这条记录"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
