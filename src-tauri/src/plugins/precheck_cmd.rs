//! 插件安装预检的 Tauri 命令层。
//!
//! ## 为什么从 `commands.rs` 拆出来
//!
//! 两条预检命令（取证 + 应用）+ 它们共用的通道封装构成一个自成一块的单元。
//! `commands.rs` 在代码预算的**反棘轮**上（1969 行，只许下调）：2026-10-06
//! 给预检加「应用变更」的第二阶段（`plugin_precheck_apply`）之后它到了 1998，
//! 而门禁不允许上调。差的那 29 行去挤格式只会把难读的东西留给下一个人——
//! 照 `diagnostics/bisect_cmd.rs` 与 `snapshot_cmd.rs` 的先例拆出来更合适。
//!
//! 与 [`crate::plugins::precheck`] 的分工不变：那边管「预检怎么跑、应用怎么
//! 落地」，这里管「怎么把这两条动作起起来」——取 data_dir、备 node / pnpm、
//! 走长任务通道。

use std::path::Path;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::commands::{blocking, cached_node, promise_pnpm, AppState};
use crate::plugins;
use crate::shell::error::AppError;
use crate::shell::settings;

/// 插件安装预检：在一个一次性沙盒实例里**真的装一次、真的起一次**内核，
/// 然后**什么都不装**就把结论交回。
///
/// 与 [`plugin_install`] 的区别只有一处，但它是关键的那处：**目标实例是
/// 沙盒**。取源、完整性校验、manifest 校验、物化、profile 接线走的都是
/// 生产同一条路径，所以「预检通过」验证的是真实安装本身，而不是它的某种
/// 近似。预检期间真实实例的 `extensions/` 与 `wiring.json` 不变；判定失败
/// 时中央库按字节回滚，用户看到的最终状态与点安装之前完全一致。
///
/// **这条命令到���为止不会改动真实实例**（两阶段契约，2026-10-06 用户拍板）：
/// 通过了也只是「可以装」，装不装由用户点 [`plugin_precheck_apply`] 决定。
/// 此前这里是 fail-open——沙盒起不来就直接装上，而那等于把「没验过」说成
/// 「验过没问题」。现在 inconclusive 时如实返回 inconclusive。
///
/// 进度通道会额外播报「建立环境基线」「验证插件」等阶段，失败时报告里的
/// `evidence` 是沙盒内核的启动日志末尾。
#[tauri::command]
pub async fn plugin_precheck_install(
    app: AppHandle,
    spec: String,
    mode: Option<String>,
    on_event: Channel<String>,
) -> Result<crate::plugins::sandbox::PrecheckReport, String> {
    let mode = mode.unwrap_or_else(|| String::from("link"));
    run_precheck_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, node_path, progress| {
            let (family, instance_id) = plugins::center::default_instance_key();
            crate::plugins::precheck::plugin_install(
                &family,
                &instance_id,
                data_dir,
                settings,
                pnpm_exe,
                node_path,
                &spec,
                &mode,
                progress,
            )
        },
    )
    .await
}

/// 应用预检通过的插件（两阶段契约的第二阶段）。
///
/// 走的是**生产安装路径本身**，不另开一条——预检那套沙盒事务验证的正是这条
/// 路径的等价物，应用时换成它才是「验过的就是要跑的」。应用前打一份 pre-change
/// 快照，装完发现不对能退回去。
///
/// 守卫与预检同一条：目标实例的内核还在跑时改接线，等于在用户正工作的
/// 时候动它的依赖。UI 会把按钮置灰，**这里再查一遍**——置灰是给人看的，
/// 这条是给并发的。
///
/// `verified_at_ms` 是那次预检的时刻。沙盒验证不重跑（重跑要几十秒，而用户
/// 已经看过报告并确认过了），但「这是多久前的结论」要由报告带出去，界面
/// 必须显示，不能让人以为刚刚验过。
#[tauri::command]
pub async fn plugin_precheck_apply(
    app: AppHandle,
    spec: String,
    mode: Option<String>,
    verified_at_ms: Option<u64>,
    on_event: Channel<String>,
) -> Result<crate::plugins::sandbox::PrecheckReport, String> {
    let mode = mode.unwrap_or_else(|| String::from("link"));
    run_precheck_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, _node_path, progress| {
            let (family, instance_id) = plugins::center::default_instance_key();
            crate::plugins::precheck::plugin_apply(
                &family,
                &instance_id,
                data_dir,
                settings,
                pnpm_exe,
                &spec,
                &mode,
                verified_at_ms.unwrap_or(0),
                progress,
            )
        },
    )
    .await
}

/// [`run_plugin_command`] 的预检变体：除了 pnpm 还要交出 node 可执行文件
/// （沙盒要靠它派生临时内核），并且要**返回**报告而不是 `()`。
///
/// 生命周期锁在整个预检期间持有：预检要起两次内核、期间会写中央库并可能
/// 向真实实例接线。用户在预检跑完之前无法「关闭工作台」是刻意的——此时放
/// 行走会让看护与接线同时改同一份 profile。
async fn run_precheck_command(
    app: AppHandle,
    on_event: Channel<String>,
    op: impl FnOnce(
            &Path,
            &settings::Settings,
            &Path,
            &Path,
            &mut dyn FnMut(&str),
        ) -> Result<crate::plugins::sandbox::PrecheckReport, AppError>
        + Send
        + 'static,
) -> Result<crate::plugins::sandbox::PrecheckReport, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    blocking(
        move || -> Result<crate::plugins::sandbox::PrecheckReport, String> {
            let state = app.state::<AppState>();
            let _lifecycle_guard = crate::lock(&state.lifecycle);
            let settings = settings::load_for_shell(settings::current_mode());
            let node_info = cached_node(&state, &settings);
            let promise_send = on_event.clone();
            let (_, pnpm_exe) = promise_pnpm(&data_dir, &node_info, move |msg| {
                let _ = promise_send.send(msg.to_string());
            })?;
            let mut progress = |msg: &str| {
                let _ = on_event.send(msg.to_string());
            };
            op(
                &data_dir,
                &settings,
                &pnpm_exe,
                Path::new(&node_info.path),
                &mut progress,
            )
            .map_err(|e| e.to_string())
        },
    )
    .await
}
