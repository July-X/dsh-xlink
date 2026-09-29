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
    // 实例自己的内核，而用户自建实例在注册表分家后有意留在**两份**注册表里
    // （`registry_split` 不做归属推断），另一个壳完全可能正跑着它。pid 文件
    // 记了启动方的壳模式，顺带把「该去哪个壳里停」说清楚。
    let family = crate::instance::KERNEL_FAMILY_DSH;
    let id = crate::instance::current_instance_id();
    if let Some(record) = target_instance_running(family, &id) {
        return Err(blocker_message(&record, &id));
    }
    on_instance(id, |id| home_recovery::recover_misplaced_home(family, id)).await
}

/// 目标实例的内核是否还在跑（**不管哪个壳拉起的**）。
///
/// 判据与 [`crate::kernel::instance_workbench_running`] 同源：实例 pid 文件 +
/// `pid_is_kernel` 的活体校验（pid 会被系统复用，裸的「进程存在」会误伤）。
/// 交叉验证用的端口**优先取 pid 文件里记的启动时刻端口**——那是内核实际
/// 绑定的那一个；注册表记录的端口可能在启动之后又被改过（用户改端口设置），
/// 拿新端口去验旧进程会漏检。旧格式 pid 文件没有端口段时才退回实例记录
/// （注册表，再退磁盘上的 `instance.json`——`start_instance` 同步过的权威
/// 副本）。两处都给不出端口时放行：fail-open 与
/// `instance::instance_owned_by_other_shell` 同一取舍——误报会挡住用户的
/// 正常操作，漏报只是一次可重试的合并。
fn target_instance_running(family: &str, id: &str) -> Option<crate::instance::PidRecord> {
    let record_port = crate::instance::load_registry()
        .ok()
        .and_then(|registry| registry.get(id).map(|record| record.port))
        .or_else(|| crate::instance::load_record_from_disk(family, id).map(|record| record.port));
    let pid = crate::instance::read_pid(family, id)?;
    let port = pid.port.or(record_port)?;
    crate::kernel::pid_is_kernel(pid.pid, Some(port)).then_some(pid)
}

/// 守卫文案：谁在跑这个实例、为什么现在不能合并、下一步去哪停。抽成纯函数
/// 是为了直接测文案要素——与 `instance::blocked_message` 同一做法：阻断类
/// 错误要说清「谁占着、怎么解」。
fn blocker_message(record: &crate::instance::PidRecord, id: &str) -> String {
    // 旧格式的 pid 文件没有壳段（`shell: None`）——认不出主人时按本壳处理：
    // 指回概览页的「关闭工作台」对用户永远是一个可尝试的下一步。
    let other_shell = record
        .shell
        .filter(|owner| *owner != crate::settings::current_mode().as_str());
    match other_shell {
        Some(owner) => {
            let port_hint = record
                .port
                .map(|port| format!("、端口 {port}"))
                .unwrap_or_default();
            format!(
                "实例 {id} 正被另一个 dsh-xlink（{owner} 壳，进程 {}{port_hint}）使用，\
                 无法找回历史会话：内核把会话清单缓存在内存里，合并的结果会被它\
                 下一次落盘覆盖。请先在那个壳里停止该实例，再回这里重试",
                record.pid,
            )
        }
        None => format!(
            "实例 {id} 的工作台正在运行，无法找回历史会话。请先在概览页点\
             「关闭工作台」停止它，再重试"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::PidRecord;

    fn record(shell: Option<&'static str>, pid: u32, port: Option<u16>) -> PidRecord {
        PidRecord { pid, port, shell }
    }

    /// 另一个壳拉起时要指明「去那个壳里停」，并带上 pid 与端口便于核对——
    /// 这是用户在两个壳并列运行时唯一能对上号的线索。
    #[test]
    fn blocker_message_points_across_shells_when_the_owner_is_the_other_one() {
        // 测试进程恒为 dev 壳：release 启动的就是「另一个壳」。
        let message = blocker_message(&record(Some("release"), 4321, Some(3090)), "default");
        assert!(message.contains("release 壳"), "{message}");
        assert!(message.contains("4321"), "{message}");
        assert!(message.contains("3090"), "{message}");
        assert!(message.contains("那个壳"), "{message}");
    }

    /// 本壳拉起（或旧格式 pid 文件认不出壳）时指回概览页的「关闭工作台」。
    #[test]
    fn blocker_message_points_to_the_overview_page_for_the_own_shell() {
        let mine = blocker_message(
            &record(
                Some(crate::settings::current_mode().as_str()),
                1,
                Some(3091),
            ),
            "default-dev",
        );
        assert!(mine.contains("关闭工作台"), "{mine}");
        assert!(!mine.contains("另一个"), "{mine}");
        let unknown = blocker_message(&record(None, 1, None), "default-dev");
        assert!(unknown.contains("关闭工作台"), "{unknown}");
        assert!(!unknown.contains("另一个"), "{unknown}");
    }
}
