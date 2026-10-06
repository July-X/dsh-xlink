//! 快照与恢复的 Tauri 命令层。
//!
//! ## 为什么从 `commands.rs` 拆出来
//!
//! 三条命令构成一个完整的用户动作（看见回退点 → 看差异 → 执行），而
//! `commands.rs` 是全仓受**反棘轮**约束的大文件：基线预算 2047 行，只许下调。
//! 2026-10-06 给 `snapshot_restore` 接上运行记录（诊断 §4.1 的 `restore` kind）
//! 之后它到了 2051 行，而那份文件不允许上调——门禁给出的唯一出路是把逻辑
//! 拆出去。与其为了 4 行去挤格式（挤出来的东西下一个人一样读不懂），不如
//! 照 `bisect_cmd.rs` 的先例让这一族命令自成模块：拆完 `commands.rs`
//! 降到 1981 行，预算可以跟着下调，反棘轮才算真的收紧了。
//!
//! 与 [`crate::diagnostics::snapshot`] / [`crate::diagnostics::restore`] 的
//! 分工不变：那边管「快照是什么、恢复会改什么」，这里管「怎么把这条动作
//! 起起来」——取 data_dir、备 node / pnpm、走长任务通道。
use std::path::Path;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::commands::{blocking, cached_node, promise_pnpm, AppState};
use crate::plugins;
use crate::shell::settings;
use crate::{diagnostics::operation_run, diagnostics::restore, diagnostics::snapshot};

/// 读当前实例的快照列表（安全网 P0 的只读面）。
///
/// 刻意**不含**恢复动作：P0 只负责让用户看得见「昨天那套配置是什么」，
/// 恢复要等 P1 的差异预览 + 二次确认。提前放一个「一键回退」按钮会让用户
/// 在没看清将要发生什么的情况下丢配置。
#[tauri::command]
pub async fn snapshot_list(
    state: State<'_, AppState>,
) -> Result<snapshot::SnapshotListView, String> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let (family, instance_id) = plugins::center::default_instance_key();
        let settings = settings::load_for_shell(settings::current_mode());
        let mut view = snapshot::list(
            &data_dir,
            &family,
            &instance_id,
            &settings.profile,
            settings.port,
        );
        // 展示路径容错读（面板还得能打开），但必须把「读到的是空文档」
        // 说出来——否则用户会以为"从来没有过回退点"。
        view.warning = snapshot::warning(&family, &instance_id);
        Ok::<_, std::convert::Infallible>(view)
    })
    .await
}

/// 预览「回到某个回退点」将要做什么。
///
/// 与 `snapshot_restore` **分成两条命令**是刻意的：用户要先看见将要失去
/// 什么，再点确认。合一意味着要点一次「恢复」才知道后果，而那时已经点了。
#[tauri::command]
pub async fn snapshot_preview_restore(
    state: State<'_, AppState>,
    id: String,
) -> Result<restore::RestoreDiff, String> {
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let (family, instance_id) = plugins::center::default_instance_key();
        let settings = settings::load_for_shell(settings::current_mode());
        restore::diff(
            &data_dir,
            &family,
            &instance_id,
            &settings.profile,
            settings.port,
            &id,
        )
        .map_err(|e| e.to_string())
    })
    .await
}

/// 真正执行恢复。**必须先调过 [`snapshot_preview_restore`]** 并让用户确认
/// 过差异——这里不再问一次，重复确认会把人问烦，而首次确认才是有信息量的
/// 那一次。
#[tauri::command]
pub async fn snapshot_restore(
    app: AppHandle,
    id: String,
    on_event: Channel<String>,
) -> Result<restore::RestoreOutcome, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    let promise_send = on_event.clone();
    blocking(move || -> Result<restore::RestoreOutcome, String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let settings = settings::load_for_shell(settings::current_mode());
        let (family, instance_id) = plugins::center::default_instance_key();
        let mut recorder =
            operation_run::begin_restore(&family, &instance_id, &settings.profile, &data_dir);
        let node_info = cached_node(&state, &settings);
        let (_, pnpm_exe) = promise_pnpm(&data_dir, &node_info, move |msg| {
            let _ = promise_send.send(msg.to_string());
        })
        .map_err(|error| operation_run::fail_restore_environment(&mut recorder, &error))?;
        let mut progress = |msg: &str| {
            let _ = on_event.send(msg.to_string());
        };
        let outcome = restore::restore(
            &data_dir,
            &family,
            &instance_id,
            &settings.profile,
            settings.port,
            &pnpm_exe,
            Path::new(&node_info.path),
            &id,
            &mut progress,
        );
        // 记账与结果转换都收在诊断层：命令层只负责把这条动作起起来。
        // 写记录失败绝不改变这里的结果（`run.rs` 的纪律 1）。
        operation_run::close_restore(&mut recorder, outcome, &data_dir)
    })
    .await
}
