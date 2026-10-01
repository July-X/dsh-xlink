//! 点系统通知横幅 → 回到「工作台」。
//!
//! # 为什么需要一层进程间交接
//!
//! Windows 上未打包应用的通知被点中时，系统**启动这个 AUMID 对应的 exe**
//! （开始菜单快捷方式带 `System.AppUserModel.ID`、且没有
//! `ToastActivatorCLSID` 时的默认激活行为），而不是"把已经跑着的窗口叫到
//! 前面"。也就是说，用户点一下通知横幅，Windows 会再拉起一个 dsh-xlink。
//!
//! 第二个进程如果照常走完 `setup()`，会执行 `kernel::reap_orphans`——**把
//! 第一个进程正在服务的内核连同对话现场一起杀掉**。那是比"点了没反应"严重
//! 得多的回归：通知的意义就是让用户回来看结果，回来的路上先把结果弄没了。
//!
//! 所以这里做的是：抢到"本模式唯一实例"的那个进程负责监听管道；后来的进程
//! 把「回到工作台」写进管道就退出。`setup()` 里的任何一行都不会在第二实例上
//! 执行。互斥体在管道**之后**创建——顺序反了就会出现"看到了互斥体、但管道
//! 还没发布"的窗口，交接请求会被静默丢掉。
//!
//! # 平台差异
//!
//! - **Windows**：点横幅 = 系统启动 exe = 走本文件的交接（本模块的全部价值）。
//! - **macOS**：点横幅由系统**激活已运行的进程**（不拉起新进程），因此不走
//!   交接；壳改为在应用被重新打开时把工作台带到台前（`lib.rs` 的
//!   `RunEvent::Reopen` → [`focus_workbench`]）。
//!
//! 两端最终都汇到同一个动作 [`focus_workbench`]，因此"回到工作台"的语义
//! （工作台没开就开、开着就聚焦、失败就把管理面板叫回来说明原因）只有一份实现。

use tauri::{AppHandle, Emitter, Manager};

/// 交接载荷：让已在运行的实例把**工作台**带到台前。
const ACT_WORKBENCH: &str = "workbench";

/// 回到工作台失败时广播给管理面板的事件。横幅点击是**无声**的——窗口没动时
/// 用户不会得到任何解释，只会以为"点了没反应"，所以失败必须自己出声。
pub const FAILED_EVENT: &str = "workbench-activate-failed";

/// 启动守卫的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Startup {
    /// 本进程是该模式的唯一实例：正常启动，并负责监听交接请求。
    Primary,
    /// 已有实例在跑，本进程的意图已转交（或转交失败），**必须立即退出**。
    HandedOff,
}

/// 进程启动的第一件事：判定"是不是第一个"。
///
/// 必须在 [`tauri::Builder`] 之前调用——第二实例绝不能建窗，更不能进
/// `setup()`（那里会回收内核）。
pub fn claim_or_handoff() -> Startup {
    if !guard_enabled() {
        return Startup::Primary;
    }
    #[cfg(target_os = "windows")]
    {
        ipc::claim_or_handoff()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 非 Windows 平台点通知横幅只会激活已运行的进程，不会拉起第二个，
        // 因此这里没有"第二实例"要防。
        Startup::Primary
    }
}

/// 主实例：开一条后台线程等服务端管道，把收到的动作派发出去。
pub fn serve(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    ipc::serve(app);
    #[cfg(not(target_os = "windows"))]
    let _ = app;
}

/// 把**工作台**带到台前。这是「回到工作台」的唯一实现，三条入口（Windows
/// 通知横幅的进程交接、macOS 的应用重新打开、将来可能的深链）都走它。
///
/// 复用 [`crate::commands::open_harness`] 而不是自己写一套窗口操作：它已经
/// 覆盖了"已开则 show + 取消最小化 + 聚焦"与"没开则按当前 launch token 建
/// 窗"，还带着端口占用 / 端口上不是内核这类可操作的中文报错。焦点相关的
/// 跨线程限制由它内部的建窗线程处理，这里只负责在后台线程上等它的结果。
pub fn focus_workbench(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dsh-focus-workbench".into())
        .spawn(move || {
            let outcome =
                tauri::async_runtime::block_on(crate::commands::open_harness(app.clone()));
            if let Err(reason) = outcome {
                // 工作台打不开时**不能什么都不做**：横幅点击是静默的，用户
                // 看到的只是"点了没反应"。把管理面板叫回前台并说明原因，
                // 他至少知道该去哪里。
                crate::show_main_shell(&app);
                if let Some(main) = app.get_webview_window("main") {
                    force_foreground(&main);
                }
                let _ = app.emit(FAILED_EVENT, reason);
                return;
            }
            // 跨进程交接夺不走前台（见 [`force_foreground`]）：抢到工作台
            // 窗口之后还要再补一次，否则用户看到的仍然是"点了没反应"。
            if let Some(harness) = app.get_webview_window("harness") {
                force_foreground(&harness);
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| eprintln!("dsh-xlink: 无法启动「回到工作台」线程：{e}"));
}

/// 把窗口真正抢到台前。
///
/// Tauri 的 `set_focus` 最终落到 `SetForegroundWindow`，而 Windows 的**前台锁**
/// 会让它在调用方**不是前台进程**时静默失败——返回 true 也可能什么都没发生。
/// 跨进程交接正是这个场景：点横幅启动的是第二个进程，要抢焦点的却是第一个。
/// 标准解法是把两个线程 attach 到一起再改前台归属（Windows 自己切换窗口时
/// 也这么做），结束必须 detach，否则两个线程的按键会互相吞。
///
/// 其它平台没有这一层限制，`set_focus` 已经够用。
#[cfg(target_os = "windows")]
pub fn force_foreground(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    unsafe {
        let current = GetCurrentThreadId();
        let foreground_thread =
            GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let attached = foreground_thread != 0 && foreground_thread != current;
        if attached {
            AttachThreadInput(current, foreground_thread, 1);
        }
        let _ = BringWindowToTop(hwnd.0);
        let _ = SetForegroundWindow(hwnd.0);
        let _ = SetActiveWindow(hwnd.0);
        let _ = SetFocus(hwnd.0);
        if attached {
            AttachThreadInput(current, foreground_thread, 0);
        }
    }
}

/// 其它平台：`set_focus` 已经能真正把窗口带到台前。
#[cfg(not(target_os = "windows"))]
pub fn force_foreground(_window: &tauri::WebviewWindow) {}

/// 只把**已经开着**的工作台窗口抬到台前；没开过就什么都不做。
///
/// 与 [`focus_workbench`] 分开是因为入口的**意图**不同：Windows 的通知横幅
/// 交接来自"用户明确要去看结果"，工作台没开就替他开一个；而 macOS 的
/// `RunEvent::Reopen` 同时覆盖点 Dock 图标这类"我只是把应用叫回来"的动作，
/// 对它来说凭空开一个工作台窗口（或者在没开过时弹一条「回到工作台失败」）
/// 都是打扰。没有已开的工作台时，系统本来就已经把管理面板带回前台了。
pub fn raise_workbench_if_open(app: &AppHandle) {
    let Some(window) = app.get_webview_window("harness") else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// 单实例守卫是否生效。
///
/// **release 无条件生效**：安装版被点通知横幅拉起时，必须是交接而不是第二个
/// 壳（否则它会在 `setup()` 里杀掉在跑的内核）。
///
/// **debug 默认不生效**：`npm run dev` 的正常用法就是"改完代码再起一次"。
/// 旧实例还活着时（Windows 上关闭只是收进托盘），开了守卫的话第二次启动会
/// 秒退，开发者会对着旧代码调试而完全看不出异常。dev 壳另有一份独立的
/// 通道名，与安装版互不干扰，需要验证交接链路时用
/// `DSH_XLINK_SINGLE_INSTANCE=1` 显式打开。
fn guard_enabled() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("DSH_XLINK_SINGLE_INSTANCE").is_some()
}

/// 通道名按模式分开：dev 壳与安装版本来就允许同时跑（不同 data dir、不同
/// 端口、不同实例），共用一个名字会让其中一个根本起不来。
fn channel_name() -> String {
    format!(
        "dsh-xlink-shell-{}",
        crate::shell::settings::current_mode().as_str()
    )
}

#[cfg(target_os = "windows")]
mod ipc {
    //! 互斥体判"是不是第一个"、命名管道传"要做什么"。
    //!
    //! 不用 `tauri-plugin-single-instance`：它带一套跨平台 IPC 依赖，而我们
    //! 只需要一条 60 字节的载荷，而 windows-sys 已经在依赖图里。

    use std::time::Duration;

    use tauri::AppHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ,
        FILE_GENERIC_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;

    use super::{Startup, ACT_WORKBENCH};

    /// 载荷上限。交接消息只有一条固定长度的 ASCII 串，给 64 字节足够，
    /// 上限本身也是防御：读到一个超长的东西就当没读到。
    const PAYLOAD_LIMIT: usize = 64;
    /// 客户端连不上时的重试节奏与总时长。第一实例是**先发布管道、再创建
    /// 互斥体**的，正常不会连不上；这段重试只兜住"管道句柄已建、监听线程
    /// 还没起来"这几十毫秒。
    const CONNECT_RETRY: Duration = Duration::from_millis(50);
    const CONNECT_ATTEMPTS: u32 = 40;

    /// UTF-16 + NUL 结尾，供 Win32 的 `…W` 系列接口使用。
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `Local\` 是会话作用域：同一台机器上的另一个用户会话（多用户 / RDP）
    /// 各有自己的交互桌面，不该被同一把锁拦住。
    fn mutex_name() -> Vec<u16> {
        wide(&format!("Local\\{}", super::channel_name()))
    }

    fn pipe_name() -> Vec<u16> {
        wide(&format!(r"\\.\pipe\{}", super::channel_name()))
    }

    pub(super) fn claim_or_handoff() -> Startup {
        // 顺序即契约：管道先发布、互斥体后创建。看到"互斥体已存在"的进程，
        // 因此可以确定对方的管道已经可连，交接请求不会落空。
        let listener = create_pipe();
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 1, mutex_name().as_ptr()) };
        if mutex.is_null() {
            eprintln!(
                "dsh-xlink: 无法创建单实例互斥体（系统错误 {}）。\
                 本次按独立实例启动；若桌面上已有 dsh-xlink，请先用托盘菜单退出它，\
                 否则两个壳会争抢同一个内核端口。",
                unsafe { GetLastError() }
            );
            close(listener);
            return Startup::Primary;
        }
        // 互斥体句柄故意不关：它要活到本进程结束，"是否已有实例"的判据就是
        // 这个具名对象还在不在。
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            // 主实例不需要在这里留着一个管道句柄——监听线程自己按"一次一个
            // 连接"的节奏建。
            close(listener);
            return Startup::Primary;
        }
        deliver();
        close(listener);
        Startup::HandedOff
    }

    /// 把「回到工作台」写进管道。连不上也照样退出：宁可让这一次点击什么
    /// 都没发生，也不能让第二个进程走 `setup()` 去杀掉在跑的内核。
    fn deliver() {
        match connect() {
            Some(pipe) => {
                let payload = ACT_WORKBENCH.as_bytes();
                let mut written = 0u32;
                let ok = unsafe {
                    WriteFile(
                        pipe,
                        payload.as_ptr(),
                        payload.len() as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    )
                } != 0
                    && written as usize == payload.len();
                if !ok {
                    eprintln!("dsh-xlink: 未能把「回到工作台」写给已运行的实例。");
                }
                close(Some(pipe));
            }
            None => eprintln!(
                "dsh-xlink: 已有实例在运行，但连不上它的激活通道（系统错误 {}）。\
                 本次「回到工作台」没有送达；请从任务栏或通知区域图标唤起已运行的窗口。",
                unsafe { GetLastError() }
            ),
        }
    }

    /// 主实例的监听线程：建管道 → 等连接 → 读一条载荷 → 派发 → 断开重建。
    pub(super) fn serve(app: &AppHandle) {
        let app = app.clone();
        std::thread::Builder::new()
            .name("dsh-activate-ipc".into())
            .spawn(move || loop {
                let Some(pipe) = create_pipe() else {
                    std::thread::sleep(CONNECT_RETRY);
                    continue;
                };
                // 客户端先连上时 ConnectNamedPipe 也会立刻返回
                // ERROR_PIPE_CONNECTED，两种情况都要往下走。
                let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) } != 0
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                if connected {
                    match read_message(pipe) {
                        Some(action) if action == ACT_WORKBENCH => super::focus_workbench(&app),
                        Some(_) => {}
                        None => eprintln!("dsh-xlink: 激活通道收到无法识别的载荷，已忽略。"),
                    }
                    unsafe {
                        DisconnectNamedPipe(pipe);
                    }
                }
                close(Some(pipe));
            })
            .map(|_| ())
            .unwrap_or_else(|e| eprintln!("dsh-xlink: 无法启动激活通道监听线程：{e}"));
    }

    fn close(handle: Option<HANDLE>) {
        if let Some(handle) = handle {
            unsafe {
                CloseHandle(handle);
            }
        }
    }

    fn create_pipe() -> Option<HANDLE> {
        let name = pipe_name();
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                PAYLOAD_LIMIT as u32,
                PAYLOAD_LIMIT as u32,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            eprintln!(
                "dsh-xlink: 无法建立激活通道（系统错误 {}）。\
                 「点通知横幅回到工作台」本次不可用，其余功能不受影响。",
                unsafe { GetLastError() }
            );
            return None;
        }
        Some(handle)
    }

    fn connect() -> Option<HANDLE> {
        let name = pipe_name();
        for _ in 0..CONNECT_ATTEMPTS {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Some(handle);
            }
            std::thread::sleep(CONNECT_RETRY);
        }
        None
    }

    fn read_message(pipe: HANDLE) -> Option<String> {
        let mut buffer = [0u8; PAYLOAD_LIMIT];
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                pipe,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } != 0;
        if !ok || read == 0 {
            return None;
        }
        String::from_utf8(buffer[..read as usize].to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_name_follows_the_shell_mode() {
        // dev 壳与安装版必须落在两个不同的通道上：共用一个名字会让其中一个
        // 根本起不来，而它们本来就要能同时运行。
        assert_eq!(
            channel_name(),
            format!(
                "dsh-xlink-shell-{}",
                crate::shell::settings::current_mode().as_str()
            )
        );
    }

    #[test]
    fn guard_is_always_on_for_installed_builds() {
        // 安装版点通知横幅会被系统再拉起一个 exe，守卫是唯一的防线。
        if !cfg!(debug_assertions) {
            assert!(guard_enabled());
        }
    }
}
