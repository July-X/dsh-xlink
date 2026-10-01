//! 任务型子进程的调度优先级：别让装内核的 pnpm 抢走正在服务用户的进程。
//!
//! ## 背景（2026-09-29 实测）
//!
//! dev 壳安装内核 `0.2.0-rc.2` 的 8 秒（19:09:08 → 19:09:16）里，**release
//! 壳的工作台 webview 被重载**，并撞上内核的启动顺序竞态
//! （`renderSlot('root') before any 'root' registration`，记在
//! `dsh/desktop/last-incident.json` 的 `at=19:09:11`）。
//!
//! 同一时刻 release 内核**一行输出都没有**（最后一条停在 18:46:42，当天它
//! 已经启动过 12 次，每次都打一行 `dsh web: http://…`）——所以内核没被惊动，
//! 被打中的是承载工作台的 WebView2 渲染进程：pnpm 硬链接数万文件 + node-gyp
//! 编译原生模块把 CPU 与磁盘打满，渲染进程不堪重负被重启。
//!
//! ## 为什么选降优先级，而不是禁止装内核
//!
//! 降优先级不解决「机器忙」本身，但让 pnpm 在与内核 / webview 争 CPU 时让路，
//! 而且**不破坏双壳并行调试**——两个壳的工作台照常开着。如果改成「另一个壳的
//! 工作台在跑就禁止装内核」，用户就不能一边用 release 一边在 dev 调试新内核了。
//!
//! **只降装东西的工具**：判定看可执行文件名（pnpm / npm / npx），因此
//! `smoke_load_native_modules` 的 Node 探针、内核自身的子进程都不受影响——
//! 探针降优先级会让它误报「原生模块加载失败」，那是比原问题更糟的假阴性。

use std::path::Path;
use std::process::Child;

use crate::shell::shell_events;

/// 可执行文件名属于「装东西的工具」时才降优先级。纯函数，因为它是本模块
/// 唯一有判断的地方——判错的后果是给不该降的进程降（拖慢用户自己的任务）。
///
/// Windows 上 pnpm / npm 通常是 `.cmd` / `.ps1` shim（`%LOCALAPPDATA%\pnpm\bin`），
/// 所以比对的是 `file_stem()` 而不是完整文件名。
fn is_package_manager(exe: &Path) -> bool {
    exe.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_ascii_lowercase())
        .is_some_and(|name| matches!(name.as_str(), "pnpm" | "npm" | "npx"))
}

/// 刚 spawn 出来的子进程若是装包工具，降到 `BELOW_NORMAL`。
///
/// **失败只记事件日志**：优先级调不成不该让一次正常安装失败。
pub fn deprioritize(exe: &Path, child: &Child) {
    if !is_package_manager(exe) {
        return;
    }
    if apply(child) {
        let name = exe
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("子进程");
        shell_events::record(
            "child-scheduling",
            &format!("{name} 已降到 BELOW_NORMAL 优先级：装包任务不该抢工作台与内核的 CPU"),
        );
    }
}

#[cfg(windows)]
fn apply(child: &Child) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::{SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS};
    // SAFETY：传的是自己刚 spawn 出来的子进程句柄；`as_raw_handle` 只是借用，
    // 不转移所有权，句柄的存活期由 `child` 保证。优先级类常量取自同一个 crate。
    unsafe { SetPriorityClass(child.as_raw_handle() as _, BELOW_NORMAL_PRIORITY_CLASS) != 0 }
}

/// 其它平台没有等价的「按子进程降优先级」调用，POSIX 的 `nice` 会作用到整个
/// 进程组、且改回来需要权限，代价大于收益。返回 `false` 表示「本次没有降」。
#[cfg(not(windows))]
fn apply(_child: &Child) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_package_managers_are_recognised() {
        for (exe, expected) in [
            ("pnpm", true),
            ("pnpm.cmd", true),
            ("pnpm.ps1", true),
            ("PNPM", true),
            ("npm", true),
            ("npx", true),
            // 探针与内核自身必须留在正常优先级。
            ("node", false),
            ("node.exe", false),
            ("dsh", false),
            // 名字里带 pnpm 但不是它自己。
            ("my-pnpm-wrapper", false),
            ("pnpmx", false),
        ] {
            assert_eq!(
                is_package_manager(Path::new(exe)),
                expected,
                "{exe} 的判定与预期不符"
            );
        }
    }

    /// 降优先级**不能影响子进程本身**：调完之后它照常跑完、照常返回退出码。
    /// 这条钉住的是「优先级调整失败时也要安静地放行」——不抛错、不吞掉子进程。
    #[test]
    fn deprioritizing_does_not_disturb_the_child() {
        let Ok(mut child) = std::process::Command::new("cmd")
            .args(["/c", "exit", "0"])
            .spawn()
        else {
            return; // 非 Windows 环境没有 cmd，跳过。
        };
        deprioritize(Path::new("pnpm"), &child);
        let status = child.wait().expect("子进程应正常结束");
        assert!(status.success(), "降优先级后子进程退出码不应改变");
    }

    /// 钉住「确实降下来了」，而不只是「调了没炸」——本模块存在的全部理由就是
    /// 这个效果，它一旦静默失效，表现形式恰恰是**什么都不发生**（用户继续
    /// 看到工作台被打崩），所以必须读回内核状态来证明。
    ///
    /// 不断言调整前的值：子进程继承父进程优先级，而测试进程的优先级由运行环境
    /// 决定。断言调整后的值就足够——`SetPriorityClass` 失败时它保持原样，不会
    /// 变成 `BELOW_NORMAL`。
    #[cfg(windows)]
    #[test]
    fn the_priority_class_is_actually_lowered() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Threading::{
            GetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS,
        };
        // 选一个活得够久的子进程：ping 自己不会被防火墙拦，且 2 秒内不会退出。
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping -n 3 127.0.0.1 > nul"])
            .spawn()
            .expect("spawn");
        deprioritize(Path::new("pnpm"), &child);
        // SAFETY：读自己 spawn 的子进程句柄；`child` 持有它直到用例结束。
        let observed = unsafe { GetPriorityClass(child.as_raw_handle() as HANDLE) };
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(
            observed, BELOW_NORMAL_PRIORITY_CLASS,
            "pnpm 子进程应当落在 BELOW_NORMAL，实际是 {observed}"
        );
    }
}
