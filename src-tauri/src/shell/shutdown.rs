//! 壳退出与内核派生之间的取消标记和串行闸门。
//!
//! `RunEvent::Exit` 与后台预启动、用户手动启动可能并发：退出清理跑完
//! 之后再由启动线程派生内核，会留下占端口的孤儿。只靠 [`AppState::lifecycle`]
//! 不够——启动可能在做 pnpm 重试或等待端口就绪，退出路径不能为了拿那把
//! 锁把用户卡住几分钟；只靠标记也不够——退出清理可能在「子进程已经派生、
//! 但 pid 还没登记」的窗口里错过它。
//!
//! 因此拆成两层：
//!
//! - [`begin`] 是单向取消标记，所有派生内核的入口（看护路径与
//!   `commands::start_instance`）在花钱之前检查；
//! - [`spawn_gate`] 只包住 `start_instance` 这段「派生 + 写实例 pid」的
//!   临界区。退出路径先置位标记，再拿闸门清理；启动路径拿到闸门后必须
//!   重新检查标记。这样退出最多等一次进程创建，不会等整个启动流程。
//!
//! 闸门只保证「清理开始后不再出现新的实例 pid」；**退出清理还必须在闸门
//! 内按实例 pid 兜底回收一次**（`lib.rs` 的 `RunEvent::Exit`）：刚派生、
//! 还没就绪的内核不在 `state.running` 或 `kernel.pid` 里（那两份都要等
//! 就绪后才写），不补这一刀，闸门放行后的那一瞬照样会漏。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// 壳是否已进入退出流程。`RunEvent::Exit` 一开始就置位，之后不再复位。
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// 串行化「派生内核并写 pid」与退出清理。只保护这一小段，不覆盖等待
/// 就绪、插件重试或 pnpm 安装。
static SPAWN_GATE: Mutex<()> = Mutex::new(());

/// 标记壳正在退出。可重复调用。
pub(crate) fn begin() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

/// 壳是否已进入退出流程。
pub(crate) fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// 拿派生闸门。启动线程在闸门内检查 [`is_shutting_down`] 后再派生；
/// 退出路径拿到闸门后不会再出现新的实例 pid。
///
/// 锁中毒时取回内部值：闸门保护的只是「不要和退出并发派生」，持有者
/// panic 不应让退出流程永久失败。
pub(crate) fn spawn_gate() -> MutexGuard<'static, ()> {
    SPAWN_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
