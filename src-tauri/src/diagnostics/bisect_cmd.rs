//! 二分定位的 Tauri 命令层。
//!
//! 与 [`crate::diagnostics::bisect`] 的分工：那边管"试了什么、排除了谁"的记录，这边管
//! 怎么把一次试探真正跑起来（起沙盒内核、等就绪、回报结果）。
//!
//! **刻意不放进 `commands.rs`**：它已经是全项目第二大的文件（2200 行），
//! 每往里塞一批命令它就离"下一个 plugins.rs"更近一步，而二分这一组命令
//! 内部耦合极强、只服务这一件事，自成模块更清楚。同一原则下 `snapshot.rs`
//! 的命令（`snapshot_list` / `snapshot_preview_restore` / `snapshot_restore`）
//! 也留在了 `commands.rs` 之外——它们短、只做转发，拆出去反而多一层壳。
use crate::commands::AppState;
use crate::diagnostics::operation_run;
use crate::plugins;
use crate::shell::error::AppError;
use crate::{diagnostics::bisect, shell::settings};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
/// 二分定位的只读视图（事故面板 / 概览页用）。
#[tauri::command]
pub async fn bisect_view(state: State<'_, AppState>) -> Result<bisect::BisectView, String> {
    let _ = state;
    let (family, instance_id) = plugins::center::default_instance_key();
    tauri::async_runtime::spawn_blocking(move || Ok(bisect::view(&family, &instance_id)))
        .await
        .map_err(|e: tauri::Error| e.to_string())?
}
/// 发起一次二分排查。
///
/// 走一条**长任务**通道：二分要反复起临时内核，全程几十秒。每轮试探的进度
/// 通过通道播报，用户能看见"已排除 N 个"而不是对着一个不动的进度条。
#[tauri::command]
pub async fn bisect_start(
    app: AppHandle,
    on_event: Channel<String>,
) -> Result<bisect::BisectView, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    let promise_send = on_event.clone();
    crate::commands::blocking(move || -> Result<bisect::BisectView, String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        // 工作台运行时环境正被真实内核占用，此时排查出来的现象与用户眼前
        // 对不上。与 `restore` 同一条理由，后端也要拦住，不能只靠 UI 置灰。
        let settings = settings::load_for_shell(settings::current_mode());
        if crate::kernel::lifecycle::workbench_running(&data_dir, &settings) {
            return Err(
                "工作台正在运行，无法排查。请先在概览页点「关闭工作台」，再重新发起排查".into(),
            );
        }
        let (family, instance_id) = plugins::center::default_instance_key();
        // 上面只看**本壳**的 data dir；用户自建实例有意留在两份注册表里，
        // 另一个壳完全可能正跑着它（设计 §11.2 明确点名了这一条）。判据按
        // 实例 pid 文件，与 restore / precheck 同一套。
        if let Some(record) = crate::shell::instance::instance_kernel_running(&family, &instance_id)
        {
            return Err(crate::shell::instance::instance_kernel_running_message(
                &record,
                &instance_id,
                "排查插件组合",
            ));
        }
        let candidates = bisect::candidates(&data_dir);
        // 候选太少时二分得不偿失（逐个停用更快），在后端就说清楚，而不是
        // 让用户点一次再读一句报错。
        if candidates.len() < bisect::MIN_CANDIDATES {
            return Err(format!(
                "可排查的插件只有 {} 个，逐个停用比二分更快，请直接在插件面板里处理",
                candidates.len()
            ));
        }
        // 守卫全过了才开记录：被守卫拦下的发起根本没发生，留一条空记录只会
        // 在「最近一次操作」里假装用户排查过一次。
        let mut recorder =
            operation_run::begin_bisect(&family, &instance_id, &settings.profile, &data_dir);
        let view = match bisect::begin(
            &family,
            &instance_id,
            candidates,
            Some(recorder.id().to_string()),
        ) {
            Ok(view) => view,
            Err(error) => {
                operation_run::fail_bisect(&mut recorder, &error.to_string());
                return Err(error.to_string());
            }
        };
        operation_run::push_bisect_select(&mut recorder, &view);
        let _ = promise_send.send(format!(
            "开始排查：共 {} 个候选，按嫌疑度分 {} 轮左右",
            view.candidate_count,
            bisect::rounds_estimate(view.candidate_count)
        ));
        Ok(view)
    })
    .await
}
/// 推进一轮二分：起一次沙盒内核，按「只启用这一半，内核起不起来」回报。
///
/// 判据走 [`crate::diagnostics::verify::probe_once`]，与 P1 恢复后的自检**同一条**——
/// 两条路径的判据一旦分叉，二分会静默收敛到错误的答案（把"没试成"当成
/// "起来了"，坏的那一半就被记成已排除）。
#[tauri::command]
pub async fn bisect_probe(
    app: AppHandle,
    on_event: Channel<String>,
) -> Result<bisect::BisectView, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    let promise_send = on_event.clone();
    crate::commands::blocking(move || -> Result<bisect::BisectView, String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let settings = settings::load_for_shell(settings::current_mode());
        let (family, instance_id) = plugins::center::default_instance_key();
        // 一次排查横跨 N 次调用，recorder 只能按会话里存的 id 接回来。接不上
        // 就安静地不记这条——时间线少一轮试探，排查本身照常跑完。
        let mut recorder = operation_run::attach_run(
            &family,
            &instance_id,
            bisect::run_id(&family, &instance_id).as_deref(),
        );
        // 没有下一轮就说明已经收尾，直接把当前视图交回去。
        let Some(trial) = bisect::next_trial(&family, &instance_id) else {
            let view = bisect::view(&family, &instance_id);
            if let Some(recorder) = recorder.as_mut() {
                operation_run::finish_bisect(recorder, &view);
            }
            return Ok(view);
        };
        let round = bisect::view(&family, &instance_id).rounds + 1;
        let probe_label = trial.join("、");
        let total =
            bisect::rounds_estimate(bisect::view(&family, &instance_id).candidate_count) as u32;
        let _ = promise_send.send(format!(
            "第 {round} 轮：只启用 {probe_label}（共 {} 个），观察能否启动 …",
            trial.len()
        ));
        let node_info = crate::commands::cached_node(&state, &settings);
        let (_, pnpm_exe) = match crate::commands::promise_pnpm(&data_dir, &node_info, |msg| {
            let _ = promise_send.send(msg.to_string());
        }) {
            Ok(pair) => pair,
            Err(error) => {
                // 沙盒起不来是环境问题，不是"这半边是好的"——不收尾的话这条
                // 记录会永远停在 running，而排查其实已经做不下去了。
                if let Some(recorder) = recorder.as_mut() {
                    operation_run::fail_bisect(recorder, &error);
                }
                return Err(error);
            }
        };
        let mut progress = |msg: &str| {
            let _ = promise_send.send(msg.to_string());
        };
        // 本轮**只把 trial 里那几个装进沙盒**，其余的既不物化也不进 profile
        // 清单——内核因此真的在"缺另外一半"的状态下启动。分治的全部前提就在
        // 这几行：装错一半，结论就是错的，而且不会报任何错。
        let trial_set: std::collections::BTreeSet<String> = trial.iter().cloned().collect();
        let allow = move |item: &crate::plugins::center::StoreItem| trial_set.contains(&item.id);
        let result = crate::diagnostics::verify::probe_once(
            &data_dir,
            &family,
            &settings,
            &pnpm_exe,
            std::path::Path::new(&node_info.path),
            &allow,
            &mut progress,
        );
        let outcome = match result.verdict {
            crate::plugins::sandbox::Verdict::Pass => bisect::Outcome::Pass,
            crate::plugins::sandbox::Verdict::Fail => bisect::Outcome::Fail,
            // 没试成就是没试成——记成 Pass 会让二分把真正有嫌疑的那半边标成
            // 已排除，那比不做二分更糟。
            crate::plugins::sandbox::Verdict::Inconclusive => bisect::Outcome::Inconclusive,
        };
        if let Some(recorder) = recorder.as_mut() {
            // 这一轮**该不该记进时间线**按判据，不按"函数返回了"：Inconclusive
            // 单独画出来，因为"没试成"和"这半边是好的"在排查里是两回事。
            let (status, label) = operation_run::probe_verdict(result.verdict);
            operation_run::push_bisect_round(
                recorder,
                round as u32,
                total,
                &probe_label,
                status,
                label,
            );
        }
        let view = bisect::advance(&family, &instance_id, trial, outcome, result.evidence)
            .map_err(err_string)?;
        if let Some(recorder) = recorder.as_mut() {
            // 收出结论的那一轮才收尾记录；还在跑就留着，下一轮接着推。
            if view.conclusion.is_some() {
                operation_run::finish_bisect(recorder, &view);
            }
        }
        Ok(view)
    })
    .await
}
/// 中断排查。已排除的结果保留——用户下次重开能接着缩小。
#[tauri::command]
pub async fn bisect_abort(state: State<'_, AppState>) -> Result<bisect::BisectView, String> {
    let _ = state;
    let (family, instance_id) = plugins::center::default_instance_key();
    tauri::async_runtime::spawn_blocking(move || {
        // 中止也要收尾记录：留着 running 的记录会让概览显示「排查进行中」，
        // 而用户刚亲手把它停了。
        let mut recorder = operation_run::attach_run(
            &family,
            &instance_id,
            bisect::run_id(&family, &instance_id).as_deref(),
        );
        let view = bisect::abort(
            &family,
            &instance_id,
            "已手动停止排查。已排除的结果保留，重新发起会接着缩小范围。",
        )
        .map_err(err_string)?;
        if let Some(recorder) = recorder.as_mut() {
            operation_run::finish_bisect(recorder, &view);
        }
        Ok(view)
    })
    .await
    .map_err(|e: tauri::Error| e.to_string())?
}
fn err_string(error: AppError) -> String {
    error.to_string()
}
