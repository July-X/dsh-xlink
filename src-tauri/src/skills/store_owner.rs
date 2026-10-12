//! 技能中央库的两个跨进程判据共用的「这个 pid 还活着吗」。
//!
//! 独立成文件的理由只有一个：**它此前被抄了两份**，而这两份已经开始漂。
//! [`super::store_busy`] 判「另一个壳是不是正在写暂存目录」、
//! [`super::store_lock`] 判「锁文件里的持有者是不是已经死了、可以抢」——两者都
//! 需要同一个答案，而它们各自的 Windows 实现不一样：`store_busy` 用
//! `OpenProcess` 探句柄，`store_lock` 又抄了一遍同样的十行。判活一旦只在其中
//! 一份上修好，另一份就会继续把死进程当活人（2026-10-12 修 Windows 测试时
//! 实测：两个文件的实现当时是逐字相同的，所以是同一个缺陷、不是两个）。
//!
//! 语义上刻意只回答一件事：**这个 pid 对应的进程此刻还在跑吗**。「进程对象还在」
//! 不等于「进程还在跑」——Windows 上前者只要还有句柄就成立，而持句柄的往往是
//! 观测方自己（`std::process::Child` 在 `wait()` 之后仍持有句柄），于是
//! 「刚退出的子进程」会被判成活着。

/// `pid` 是否仍是一个**正在运行**的进程。
///
/// Unix 用 `kill(pid, 0)`：它不投递信号，只问「这个 pid 在不在」，且**不会**回收
/// 僵尸——`waitpid` 会把观测动作本身变成破坏，所以这里绝不能用它。返回 `EPERM`
/// 表示进程在、只是没权限，同样算活着；只有 `ESRCH`（pid 从进程表消失）才算没了。
#[cfg(unix)]
pub(crate) fn process_alive(pid: u32) -> bool {
    if unsafe { libc::kill(pid as i32, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Windows 上没有信号 0 的等价物，走两步：先 `OpenProcess` 探一个查询句柄
/// （能打开 = 这个 pid 还能被指认），再用 `GetExitCodeProcess` 问它的退出码。
///
/// **第二步不能省**：`OpenProcess` 成功只说明进程对象还在，而只要还有句柄
/// 指向它，对象就还在——刚 `wait()` 完的子进程正属于这一类。只探句柄的话，
/// 「创建者已退出的暂存目录」会被判成「中央库正忙」，崩溃残留永远回收不掉，
/// 技能安装的僵尸锁也永远抢不到（2026-10-12 的两条 Windows 失败就是它）。
/// 退出码等于 `STILL_ACTIVE`（259）才是「还在跑」。
///
/// 已知的边角：一个真的以退出码 259 结束的进程会被判成活着。代价方向是保守的
/// （当成有人占着，最坏是多等一轮），与 Unix 上「`EPERM` 也算活着」同一取向。
#[cfg(windows)]
pub(crate) fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // Safety: 两个调用都只查询、不修改目标进程；句柄用完立刻关，不留泄漏。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code = 0u32;
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe { CloseHandle(handle) };
    queried && exit_code == STILL_ACTIVE as u32
}

/// 起一个立刻退出的子进程，拿它的 pid 当「已死掉的持有者」。
///
/// Windows 上没有 `sh`，用 `cmd /C exit 0`。**不要**在 `wait()` 之前就把
/// `Child` drop 掉再断言——那会让「持有句柄」这个前提消失，测试就测不到
/// `GetExitCodeProcess` 那一步了（句柄开着正是本文件要处理的场景）。
#[cfg(test)]
pub(crate) fn spawn_exiting() -> std::process::Child {
    #[cfg(unix)]
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("spawn exiting owner");
    #[cfg(windows)]
    let mut child = std::process::Command::new("cmd")
        .args(["/C", "exit 0"])
        .spawn()
        .expect("spawn exiting owner");
    let _ = child.wait();
    child
}

/// 起一个短命但确实在跑的子进程，当「别人的活进程」。
///
/// Windows 上不用 `timeout /t`：它要求控制台，stdin 被重定向时直接报错退出，
/// 那样测到的会是「进程已经死了」而不是「进程活着」。`ping` 的计时靠的是
/// 发包间隔，重定向下照样睡满。
#[cfg(test)]
pub(crate) fn spawn_sleeper() -> std::process::Child {
    #[cfg(unix)]
    let child = std::process::Command::new("sh")
        .args(["-c", "sleep 30"])
        .spawn()
        .expect("spawn sleeping owner");
    #[cfg(windows)]
    let child = std::process::Command::new("cmd")
        .args(["/C", "ping -n 31 127.0.0.1 > NUL"])
        .spawn()
        .expect("spawn sleeping owner");
    child
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminated_child_is_not_alive_even_while_its_handle_is_open() {
        let mut child = spawn_exiting();
        assert!(
            !process_alive(child.id()),
            "已退出但句柄仍被持有的进程必须判为「没了」，否则崩溃残留永远回收不掉"
        );
        let _ = child.wait();
    }

    #[test]
    fn a_running_child_is_alive() {
        let mut child = spawn_sleeper();
        assert!(
            process_alive(child.id()),
            "正在跑的子进程必须判为活着——否则并发安装会被当成残留"
        );
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn our_own_process_is_alive() {
        assert!(process_alive(std::process::id()));
    }
}
