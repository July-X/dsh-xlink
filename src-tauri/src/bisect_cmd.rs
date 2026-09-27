//! 二分定位的 Tauri 命令层。
//!
//! 与 [`crate::bisect`] 的分工：那边管"试了什么、排除了谁"的记录，这边管
//! 怎么把一次试探真正跑起来（起沙盒内核、等就绪、回报结果）。
//!
//! **刻意不放进 `commands.rs`**：它已经是全项目第二大的文件（2200 行），
//! 每往里塞一批命令它就离"下一个 plugins.rs"更近一步，而二分这一组命令
//! 内部耦合极强、只服务这一件事，自成模块更清楚。同一原则下 `snapshot.rs`
//! 的命令（`snapshot_list` / `snapshot_preview_restore` / `snapshot_restore`）
//! 也留在了 `commands.rs` 之外——它们短、只做转发，拆出去反而多一层壳。

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::commands::AppState;
use crate::error::AppError;
use crate::{bisect, plugins, settings};

/// 二分的轮数上界估算（⌈log₂n⌉）。面板用它给"预计还要多久"一个量级，
/// 而不是让用户对着一句"正在进行"干等。
pub fn rounds_estimate(candidates: usize) -> usize {
    if candidates <= 1 {
        return 0;
    }
    let mut n = candidates;
    let mut rounds = 0;
    while n > 1 {
        n = n.div_ceil(2);
        rounds += 1;
    }
    rounds
}

/// 二分定位的只读视图（事故面板 / 概览页用）。
#[tauri::command]
pub async fn bisect_view(state: State<'_, AppState>) -> Result<bisect::BisectView, String> {
    let _ = state;
    let (family, instance_id) = plugins::default_instance_key();
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
        let (family, instance_id) = plugins::default_instance_key();
        let candidates = bisect::candidates(&data_dir);
        // 候选太少时二分得不偿失（逐个停用更快），在后端就说清楚，而不是
        // 让用户点一次再读一句报错。
        if candidates.len() < bisect::MIN_CANDIDATES {
            return Err(format!(
                "可排查的扩展只有 {} 个，逐个停用比二分更快，请直接在插件 / 技能面板里处理",
                candidates.len()
            ));
        }
        let view = bisect::begin(&family, &instance_id, candidates).map_err(err_string)?;
        let _ = promise_send.send(format!(
            "开始排查：共 {} 个候选，按嫌疑度分 {} 轮左右",
            view.candidate_count,
            rounds_estimate(view.candidate_count)
        ));
        Ok(view)
    })
    .await
}

/// 推进一轮二分：起一次沙盒内核，按「只启用这一半，内核起不起来」回报。
///
/// 判据走 [`crate::verify::probe_once`]，与 P1 恢复后的自检**同一条**——
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
        let (family, instance_id) = plugins::default_instance_key();

        // 没有下一轮就说明已经收尾，直接把当前视图交回去。
        let Some(trial) = bisect::next_trial(&family, &instance_id) else {
            return Ok(bisect::view(&family, &instance_id));
        };
        let round = bisect::view(&family, &instance_id).rounds + 1;
        let _ = promise_send.send(format!(
            "第 {round} 轮：只启用 {}（共 {} 个），观察能否启动 …",
            trial.join("、"),
            trial.len()
        ));
        let node_info = crate::commands::cached_node(&state, &settings);
        let mut progress = |msg: &str| {
            let _ = promise_send.send(msg.to_string());
        };
        let result = crate::verify::probe_once(
            &data_dir,
            &family,
            &instance_id,
            &settings,
            std::path::Path::new(&node_info.path),
            &mut progress,
        );
        let outcome = match result.verdict {
            crate::sandbox::Verdict::Pass => bisect::Outcome::Pass,
            crate::sandbox::Verdict::Fail => bisect::Outcome::Fail,
            // 没试成就是没试成——记成 Pass 会让二分把真正有嫌疑的那半边标成
            // 已排除，那比不做二分更糟。
            crate::sandbox::Verdict::Inconclusive => bisect::Outcome::Inconclusive,
        };
        bisect::advance(&family, &instance_id, trial, outcome, result.evidence).map_err(err_string)
    })
    .await
}

/// 中断排查。已排除的结果保留——用户下次重开能接着缩小。
#[tauri::command]
pub async fn bisect_abort(state: State<'_, AppState>) -> Result<bisect::BisectView, String> {
    let _ = state;
    let (family, instance_id) = plugins::default_instance_key();
    tauri::async_runtime::spawn_blocking(move || {
        bisect::abort(
            &family,
            &instance_id,
            "已手动停止排查。已排除的结果保留，重新发起会接着缩小范围。",
        )
        .map_err(err_string)
    })
    .await
    .map_err(|e: tauri::Error| e.to_string())?
}

fn err_string(error: AppError) -> String {
    error.to_string()
}
