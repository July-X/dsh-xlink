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

use crate::home_recovery;

/// 两条命令都作用在**本壳当前服务的实例**上（用户在顶部页签切过就用他选的那个，
/// 且选择必须是本壳注册表里真实存在的实例，见 `instance::current_instance_id`
/// 的陈旧指针校正），因此共用这一个入口。
///
/// `id` 由调用方解析后**传入**而不是在闭包里再取一次：守卫与执行必须落在
/// 同一个实例上——两次解析之间用户切换页签的话，守卫查的是 A、动的却是 B。
fn on_instance<T: Send + 'static>(
    id: String,
    f: impl FnOnce(&str) -> T + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    crate::commands::blocking(move || Ok::<_, String>(f(&id)))
}

/// 扫描「别的实例 home 里有没有本实例缺的历史会话」。**只读**——不创建任何
/// 目录、不触碰源 / 目标。「数据迁移」面板用它决定是否渲染「找回历史会话」
/// 卡片（2026-09-28 的 `~/.dsh` 搬错实例事故就是靠这条路径可见的）。
#[tauri::command]
pub async fn scan_misplaced_home() -> Result<home_recovery::MisplacedScan, String> {
    on_instance(crate::instance::current_instance_id(), |id| {
        home_recovery::scan_misplaced_home(crate::instance::KERNEL_FAMILY_DSH, id)
    })
    .await
}

/// 把扫描出的缺失条目**复制**进本实例 home，并合并会话清单。源永不删除、
/// 目标已有条目永不覆盖——回收不是搬家。逐条结果由 `MisplacedRecovery` 汇报，
/// 部分失败不阻断其余条目。
#[tauri::command]
pub async fn recover_misplaced_home() -> Result<home_recovery::MisplacedRecovery, String> {
    // 与 `restore` 同一条纪律：内核把 storages 缓存在内存里，实例运行期间合并
    // 进去的会话清单会被它下一次落盘整个覆盖——用户看到的是「点了没反应」。
    //
    // 判据必须是**实例级**的，不能只看本壳工作台：会把清单缓存住的正是目标
    // 实例自己的内核，而用户自建实例在注册表分家后有意留在**两份**注册表里，
    // 另一个壳完全可能正跑着它。判据与文案都在 `instance` 那层共享层
    // （`snapshot_restore` 走同一条），pid 文件里记了启动方的壳模式，顺带把
    // 「该去哪个壳里停」说清楚。
    let family = crate::instance::KERNEL_FAMILY_DSH;
    let id = crate::instance::current_instance_id();
    if let Some(record) = crate::instance::instance_kernel_running(family, &id) {
        return Err(crate::instance::instance_kernel_running_message(
            &record,
            &id,
            "找回历史会话",
        ));
    }
    on_instance(id, |id| home_recovery::recover_misplaced_home(family, id)).await
}
