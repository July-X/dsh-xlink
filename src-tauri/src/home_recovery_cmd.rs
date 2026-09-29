//! 「搬错实例」的会话回收命令壳（`home_recovery.rs` 的 Tauri 面）。
//!
//! 为什么要单独成文件：`commands.rs` 是代码预算里的反棘轮文件（只许下调），
//! 而这两条命令与它其余 70 多条命令**没有任何共享逻辑**——它们只做一件事，
//! 把「当前壳正在服务的那个实例」喂给 [`home_recovery`]，再把结果原样透传。
//! 留在 `commands.rs` 里只能靠上调数字过门禁，而调数字是 AGENTS.md 明确禁止的
//! 反应。`bisect_cmd.rs` 是同一处理由。
//!
//! 字段一律 snake_case（结构体没套 `camelCase`），与 `migration.rs` 那条
//! IPC 链路保持一致——`check:invariants` 的 ipc-fields 一项会核对这一点。

use tauri::State;

use crate::{home_recovery, AppState};

/// 两条命令都作用在**本壳当前服务的实例**上（用户在顶部页签切过就用他选的那个），
/// 因此共用这一个入口。
fn on_current_instance<T: Send + 'static>(
    f: impl FnOnce(&str) -> T + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    let id = crate::instance::current_instance_id();
    crate::commands::blocking(move || Ok::<_, String>(f(&id)))
}

/// 扫描「别的实例 home 里有没有本实例缺的历史会话」。**只读**——不创建任何
/// 目录、不触碰源 / 目标。「数据迁移」面板用它决定是否渲染「找回历史会话」
/// 卡片（2026-09-28 的 `~/.dsh` 搬错实例事故就是靠这条路径可见的）。
#[tauri::command]
pub async fn scan_misplaced_home() -> Result<home_recovery::MisplacedScan, String> {
    on_current_instance(|id| {
        home_recovery::scan_misplaced_home(crate::instance::KERNEL_FAMILY_DSH, id)
    })
    .await
}

/// 把扫描出的缺失条目**复制**进本实例 home，并合并会话清单。源永不删除、
/// 目标已有条目永不覆盖——回收不是搬家。逐条结果由 `MisplacedRecovery` 汇报，
/// 部分失败不阻断其余条目。
#[tauri::command]
pub async fn recover_misplaced_home(
    state: State<'_, AppState>,
) -> Result<home_recovery::MisplacedRecovery, String> {
    // 与 `restore` 同一条纪律：内核把 storages 缓存在内存里，工作台运行时
    // 合并进去的会话清单会被它下一次落盘整个覆盖——用户看到的是「点了没反应」。
    let data_dir = state.data_dir.clone();
    let settings = crate::settings::load_for_shell(crate::settings::current_mode());
    if crate::kernel::workbench_running(&data_dir, &settings) {
        return Err(
            "工作台正在运行，无法找回历史会话。请先在概览页点「关闭工作台」，再重试".into(),
        );
    }
    on_current_instance(|id| {
        home_recovery::recover_misplaced_home(crate::instance::KERNEL_FAMILY_DSH, id)
    })
    .await
}
