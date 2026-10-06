//! 运行记录的 Tauri 命令层。
//!
//! 三条读命令**只读，且没有副作用**：它们不启动内核、不出网、不改快照，
//! 也**不创建**运行记录。用户打开「查看启动诊断」不应该凭空多出一条
//! 「诊断被查看」的记录——那会让列表自己长出噪声，用户下次再看时
//! 会以为壳在他没操作的时候启动过工作台。
//!
//! 唯一的写命令是用户明确点出来的 [`diagnostic_run_clear`]：它删掉的东西
//! 全部是**这台机器上只服务于本壳的诊断历史**，不碰内核安装树、实例 home、
//! 插件中央库或技能库。
//!
//! 实例解析一律走 [`instance::resolve_default`]：诊断记录是**某一个实例**
//! 的事实，让调用方自己传 family/instance 只会让前端有机会读到另一个实例
//! 的启动现场。它也正是写入侧（`startup_run` / `precheck`）解析的那一份，
//! 读写不同源会让「刚启动完去看诊断」读了个空列表。
//!
//! 命令的 `spawn_blocking` 一律走 [`crate::commands::blocking`]：裸调用
//! 把 `JoinError` 的英文原文直接甩到 UI 上。
use crate::diagnostics::run;
use crate::shell::instance;
use tauri::State;

use crate::commands::AppState;

/// 运行记录列表（概览「最近一次操作」与启动诊断的历史入口用）。
#[tauri::command]
pub async fn diagnostic_run_list(
    _state: State<'_, AppState>,
    kind: Option<String>,
) -> Result<Vec<run::RunSummary>, String> {
    let (family, instance_id) = instance::resolve_default();
    crate::commands::blocking(move || {
        Ok::<_, std::convert::Infallible>(
            run::load_index(family, instance_id)
                .entries
                .into_iter()
                .filter(|entry| match &kind {
                    Some(wanted) if !wanted.is_empty() => &entry.kind == wanted,
                    _ => true,
                })
                .collect(),
        )
    })
    .await
}

/// 单条运行记录详情（含全部事件）。
///
/// 找不到时返回 `Ok(None)` 而不是 `Err`：记录被裁剪是**正常**的路径
/// （超过 [`run::MAX_RUNS`]），把它当错误会让用户看到一句没有出处的
/// "运行记录不存在"。前端据此显示「记录已被清理」并给出去向提示。
#[tauri::command]
pub async fn diagnostic_run_get(
    _state: State<'_, AppState>,
    run_id: String,
) -> Result<Option<run::DiagnosticRun>, String> {
    let (family, instance_id) = instance::resolve_default();
    crate::commands::blocking(move || {
        Ok::<_, std::convert::Infallible>(run::get(family, instance_id, &run_id))
    })
    .await
}

/// 最近一次指定类型的运行记录摘要。`kind` 为空时取最近一条。
///
/// 概览页「最近一次操作」与「后台启动记录」都读它——它们要的是「上一次
/// 发生了什么」，不需要拉全量列表再在前端排序。
#[tauri::command]
pub async fn diagnostic_run_latest(
    _state: State<'_, AppState>,
    kind: Option<String>,
) -> Result<Option<run::RunSummary>, String> {
    let (family, instance_id) = instance::resolve_default();
    crate::commands::blocking(move || {
        let wanted = kind.unwrap_or_default();
        Ok::<_, std::convert::Infallible>(
            run::load_index(family, instance_id)
                .entries
                .into_iter()
                .find(|entry| wanted.is_empty() || entry.kind == wanted),
        )
    })
    .await
}

/// 清除本实例的全部诊断记录，返回删除的详情文件个数。
///
/// 与 [`run::sweep_orphans`] 的分工：那条在启动期自动跑，只清「索引里
/// 已经查不到、用户也看不见」的孤儿文件；这条是用户主动点的，要的是
/// 「这段历史我不要了」——连索引一起清，被事故引用的那条也不豁免。
///
/// 返回 `Err` 时详情文件已删但索引没能重写（详见 [`run::clear`]），用户
/// 会看到一句明确的失败而不是一个假的「已清除」。
#[tauri::command]
pub async fn diagnostic_run_clear(_state: State<'_, AppState>) -> Result<usize, String> {
    let (family, instance_id) = instance::resolve_default();
    crate::commands::blocking(move || run::clear(family, instance_id))
        .await
        .map_err(|error| error.to_string())
}
