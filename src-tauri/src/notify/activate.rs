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
//!   交接。壳在这里挂一个**激活**观察者（[`observe_activation`]），把工作台
//!   带到台前。
//!
//! # macOS 上「点横幅」到底派发了什么（2026-10-13 本机实测）
//!
//! macOS 26.7.1，一个签过名的 `.app` 内投递 `NSUserNotification`，三次真实点
//! 横幅 + 一次从通知中心点，全部记在同一个只写日志的探针里：
//!
//! ```text
//! applicationDidBecomeActive              每次都来
//! applicationShouldHandleReopen          一次都没有
//! userNotificationCenter:didActivate:     一次都没有
//! ```
//!
//! 结论有三条，第三条最贵：
//!
//! 1. `RunEvent::Reopen` 的**唯一**来源是 AppKit 的
//!    `applicationShouldHandleReopen:hasVisibleWindows:`，而横幅点击**从不**
//!    派发它——它只在应用被「重新打开」（点 Dock 图标、`open` Apple Event）
//!    时才来。此前 `lib.rs` 把「点横幅回到工作台」挂在 `RunEvent::Reopen`
//!    上，那条接线在真机上**一次都没有触发过**，点横幅表现为完全没反应。
//! 2. 旧通知 API 的点击回调 `userNotificationCenter:didActivateNotification:`
//!    在这个 macOS 版本上也不再派发（探针把 delegate 强引用持住了，不是引用
//!    被回收导致的漏收）。因此「拿点击回调」这条路也走不通。
//! 3. **唯一可靠的信号是激活**。tao 没实现 `applicationDidBecomeActive`，
//!    Tauri 也没有对应的 `RunEvent`，所以 [`observe_activation`] 直接向
//!    `NSNotificationCenter` 注册观察者。
//!
//! 两端最终都汇到同一个动作 [`focus_workbench`]，因此"回到工作台"的语义
//! （工作台没开就开、开着就聚焦、失败就把管理面板叫回来说明原因）只有一份实现。

use tauri::{AppHandle, Emitter, Manager};

/// 交接载荷：让已在运行的实例把**工作台**带到台前。
///
/// 零调用的成因是**平台假阳性**：唯一使用点是本文件 `mod ipc`
/// （`#[cfg(target_os = "windows")]`）里的 named-pipe 读端，macOS 不编译那段。
#[allow(dead_code, reason = "Windows 专属：调用点在 cfg(windows) 分支")]
const ACT_WORKBENCH: &str = "workbench";

/// 回到工作台失败时广播给管理面板的事件。横幅点击是**无声**的——窗口没动时
/// 用户不会得到任何解释，只会以为"点了没反应"，所以真故障必须自己出声。
/// 「真故障」怎么判见 [`announce_failure`]：内核压根没跑时此刻只是没有工作台，
/// 不发这个事件。
#[allow(dead_code, reason = "传递性：调用者在 Windows 分支内")]
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

/// 主实例：把「用户点了通知横幅」这条请求接进来。两端的机制不同（Windows 是
/// 管道交接、macOS 是激活观察），落点都是同一个 [`focus_workbench`]。
pub fn serve(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    ipc::serve(app);
    #[cfg(target_os = "macos")]
    observe_activation(app);
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = app;
}

/// 「工作台打不开」要不要向管理面板出声。
///
/// 这条通道被两种入口共用，而 Windows **分不出它们**：点通知横幅、点任务栏 /
/// 开始菜单里的启动项，系统都是按同一个快捷方式再拉起一个本 exe（见模块
/// 文档），两边的命令行一模一样，壳无从判断用户刚点的是哪一个。
///
/// 第二种在登录自启之后是用户的日常动作——壳已经在后台跑着、内核通常还没起。
/// 此时「打不开工作台」是一句事实陈述而不是故障，弹一条 8 秒红字纯属噪音
/// （2026-10-09 用户反馈：开机自启后点任务栏启动项，窗口出来了还挂着
/// 「回到工作台失败」）。面板此刻已经被叫回前台，概览页上「已停止」与
/// 「启动工作台」按钮就在那里，不解释也不会把他困住。
///
/// 反过来，内核确实在跑却打不开工作台（端口被无关程序占着、改过端口、建窗
/// 失败）是真故障：他点的横幅 / 图标什么都没发生，必须出声。判据取
/// [`crate::kernel_running`]，不去匹配 `open_harness` 的报错文案——后者是
/// 给人看的中文句子，改一次措辞这条判据就静默失效。
/// 「工作台打不开」要不要出声的判据（两端共用，见上）。
fn announce_failure(kernel_running: bool) -> bool {
    kernel_running
}

/// 把**工作台**带到台前。这是「回到工作台」的唯一实现，两条入口（Windows
/// 通知横幅的进程交接、macOS 的激活观察）都走它。
///
/// Windows 的那一条同时覆盖「点任务栏 / 开始菜单里的启动项」——系统拉起第
/// 二个进程时无法区分这两者（见 [`announce_failure`]），但两者要的结果本来
/// 也只差在工作台能否打开：打不开时落回管理面板，这正是点启动项的人想要的。
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
                // 面板一定要叫回前台：交接是静默的，窗口没动只会让用户以为
                // "点了没反应"；跨进程交接还抢不到前台，见 force_foreground。
                crate::show_main_shell(&app);
                if let Some(main) = app.get_webview_window("main") {
                    force_foreground(&main);
                }
                // 出不出声另算：此刻压根没有工作台（内核没跑）不是故障。
                if announce_failure(crate::kernel_running(&app)) {
                    let _ = app.emit(FAILED_EVENT, reason);
                }
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

// ---------------------------------------------------------------------------
// macOS：激活观察者（「点横幅」在这条路上唯一的信号）
//
// **这一段每个符号都挂 `#[cfg(target_os = "macos")]`**：唯一调用它们的
// [`observe_activation`] 是 macOS 专属的，不挂的话 Windows 上三个符号全是死
// 代码，而 `check:rust` 的 clippy 带 `-D warnings`（`package.json`）——本机
// 在 Windows 上跑 `npm run check` 会直接红。判据函数被单测引用并不能救它：测试
// 目标只存在于 `--all-targets`，正式编译里那个引用不存在。
// ---------------------------------------------------------------------------

/// 「刚弹出的横幅」算新鲜的时间窗（毫秒）。
///
/// 激活不是横幅独有的：Cmd-Tab、点 Dock 图标、点菜单栏图标都会激活应用，而
/// [`raise_workbench_if_open`] 的文档明确记着「凭空开一个工作台窗口（或者在
/// 没开过时弹一条『回到工作台失败』）都是打扰」。只看「有未读」会把这条约定
/// 也一起推翻——用户点自己的面板时工作台也会窜到前面。
///
/// 而横幅是**由某次任务完成触发的**，所以第二道判据是「最近一次完成距今多久」。
/// 5 分钟：macOS 横幅本身只停几秒，但通知中心里的条目一直留着，按它算是用户
/// 明确表达过「我要回工作台」；再久就没有意义了，那时的未读已经被别的动作
/// （面板里的「全部已读」）收过一次。
#[cfg(target_os = "macos")]
const BANNER_FRESH_MS: u64 = 5 * 60 * 1000;

/// 一次激活算不算「用户点了刚弹出的横幅」。
///
/// 拆成纯函数是为了能脱离 `AppHandle` 单测：它是这条修复里唯一有分支的判据，
/// 而判据写错一次的症状是「点面板时莫名多出一个工作台窗口」——纯函数测试能
/// 把这种形状错误挡住，编译测试不能。
#[cfg(target_os = "macos")]
fn banner_activation(unread: u32, newest_finished_at_ms: Option<u64>, now_ms: u64) -> bool {
    unread > 0
        && newest_finished_at_ms.is_some_and(|at| now_ms >= at && now_ms - at <= BANNER_FRESH_MS)
}

/// macOS：一次激活。按「点横幅」处理的就走 [`focus_workbench`]，其余不动。
///
/// 这个回调在**主线程**上跑（观察者注册时队列传的是 `None`），而
/// [`crate::notify::task::status`] 要读设置文件——**IO 就在这里发生**，只是它
/// 只有一次几毫秒的文件读，而「读完之后」的所有动作（起窗口、建工作台、等结果）
/// 都交给了 [`focus_workbench`] 的后台线程，主线程上不碰任何窗口。
#[cfg(target_os = "macos")]
fn on_activated(app: &AppHandle) {
    let status = crate::notify::task::status(app);
    let banner = banner_activation(
        status.unread,
        status.items.first().map(|item| item.finished_at_ms),
        crate::shell::process::epoch_millis(),
    );
    if banner {
        focus_workbench(app);
    }
}

/// 注册激活观察者。由 [`serve`] 在 `setup()` 里调用——**必须在 AppKit 主线程
/// 上**。
#[cfg(target_os = "macos")]
fn observe_activation(app: &AppHandle) {
    use std::ptr::NonNull;

    use block2::{DynBlock, StackBlock};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSString};

    let handle = app.clone();
    // SAFETY: 由 `setup()` 在主线程上调用。`addObserverForName:…usingBlock:` 在
    // 注册时**拷贝**这个 block，所以它不必活到函数返回之后；回调本身只起一个
    // 后台线程（[`focus_workbench`] 自带那个线程），不在主线程上碰任何窗口。
    unsafe {
        let center = NSNotificationCenter::defaultCenter();
        let name = NSString::from_str("NSApplicationDidBecomeActiveNotification");
        let block: &DynBlock<dyn Fn(NonNull<NSNotification>)> =
            &StackBlock::new(move |_note| on_activated(&handle));
        let token = center.addObserverForName_object_queue_usingBlock(
            Some(name.as_ref()),
            None,
            None,
            block,
        );
        // 令牌持有整条注册，一被释放系统就顺手注销观察者。这条注册是**常驻**
        // 能力（壳活着，「点横幅回到工作台」就一直在），所以令牌必须活到进程
        // 结束。`Retained<ProtocolObject<dyn NSObjectProtocol>>` 既不 `Send`
        // 也不 `Sync`（objc2 的类型系统里 ObjC 协议不是线程安全的），因此它放
        // 不进 `static`——而 `mem::forget` 在这里的语义恰好是「显式地不要注销」
        //，不是漏掉的清理。
        std::mem::forget(token);
    }
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
#[allow(dead_code, reason = "传递性：调用者在 Windows 分支内")]
pub fn force_foreground(_window: &tauri::WebviewWindow) {}

/// 只把**已经开着**的工作台窗口抬到台前；没开过就返回 `false`（返回 `true`
/// 表示工作台存在且已被抬到台前）。
///
/// 与 [`focus_workbench`] 分开是因为入口的**意图**不同：Windows 的通知横幅
/// 交接来自"用户明确要去看结果"，工作台没开就替他开一个；而 macOS 的
/// `RunEvent::Reopen` 同时覆盖点 Dock 图标这类"我只是把应用叫回来"的动作，
/// 对它来说凭空开一个工作台窗口（或者在没开过时弹一条「回到工作台失败」）
/// 都是打扰。
///
/// 「没开过就什么都不做」**不能理解成调用方也可以什么都不做**：macOS 的
/// 激活只把进程带到前台，**不会替我们把 `hide()` 掉的窗口重新显示出来**
/// （orderOut 的窗口不在系统的「可见窗口」清单里，这里的注释此前宣称
/// "系统本来就已经把管理面板带回前台了"，是错的——2026-10-05 重启实测
/// 打脸）。所以返回 `false` 时调用方必须自己调
/// [`crate::shell::resident::show_main_shell`] 兜底，否则用户点的是一枚
/// 点了没反应的图标。
#[allow(dead_code, reason = "唯一调用方在 lib.rs 的 macOS Reopen 分支")]
pub fn raise_workbench_if_open(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("harness") else {
        return false;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    true
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
#[allow(dead_code, reason = "Windows 专属：调用点在 cfg(windows) 分支")]
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
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PeekNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;

    use super::{Startup, ACT_WORKBENCH};

    /// 载荷上限。交接消息只有一条固定长度的 ASCII 串，给 64 字节足够，
    /// 上限本身也是防御：读到一个超长的东西就当没读到。
    const PAYLOAD_LIMIT: usize = 64;
    /// 客户端先连不上时的重试节奏与总时长。第一实例是**先发布管道、再创建
    /// 互斥体**的，正常不会连不上；这段重试只兜住"管道句柄已建、监听线程
    /// 还没起来"这几十毫秒。
    const CONNECT_RETRY: Duration = Duration::from_millis(50);
    const CONNECT_ATTEMPTS: u32 = 40;
    /// 客户端连上之后、等它把载荷写进来的上限（见 [`wait_for_payload`]）。
    const READ_TIMEOUT: Duration = Duration::from_millis(3000);
    /// 上面那段等待的轮询间隔。
    const READ_POLL: Duration = Duration::from_millis(50);

    /// UTF-16 + NUL 结尾，供 Win32 的 `…W` 系列接口使用。
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `Local\` 是会话作用域：同一台机器上的另一个用户会话（多用户 / RDP）
    /// 各有自己的交互桌面，不该被同一把锁拦住。
    fn mutex_name() -> Vec<u16> {
        wide(&format!("Local\\{}", super::channel_name()))
    }

    fn pipe_name() -> String {
        format!(r"\\.\pipe\{}", super::channel_name())
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
                    // 先等载荷真的到，再读：`ReadFile` 是阻塞的，而
                    // `\\.\pipe\<name>` 任何本机进程都能打开，一个连上之后
                    // 既不写也不断开的客户端会把整个通道挂死。
                    if wait_for_payload(pipe, READ_TIMEOUT) {
                        match read_message(pipe) {
                            Some(action) if action == ACT_WORKBENCH => super::focus_workbench(&app),
                            Some(_) => {}
                            None => eprintln!("dsh-xlink: 激活通道收到无法识别的载荷，已忽略。"),
                        }
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
        create_named_pipe(&pipe_name())
    }

    /// 建一条服务端管道实例。**名字由调用方给**：测试要用一条自己的通道，
    /// 否则 `CreateFileW` 可能连到真壳正在监听的同名实例上去——那等于把用户的
    /// 一次「回到工作台」吃掉，还让测试永远等不到自己的对端。
    fn create_named_pipe(name: &str) -> Option<HANDLE> {
        let name = wide(name);
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
        connect_named(&pipe_name())
    }

    fn connect_named(name: &str) -> Option<HANDLE> {
        let name = wide(name);
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

    /// 管道里是否真的已经有可读的字节。
    ///
    /// **`ReadFile` 之前必须有这一步**：它是阻塞读，而 `\\.\pipe\<名字>` 是任何
    /// 本机进程都能 `CreateFileW` 打开的公共资源——一个连上之后既不写数据也
    /// 不关闭的客户端（别的自启动项、体检脚本、手滑的 `dir \\.\pipe\`）会把监听
    /// 线程永久挂住，此后点横幅、点任务栏启动项全部静默失效，而且日志里一行都
    /// 没有，只能重启壳才恢复。`PeekNamedPipe` 立即返回，用轮询换掉那次阻塞：
    /// 有数据、或超时、或对端断开，三种情况都让监听循环回到 `DisconnectNamedPipe`
    /// 重建管道——超时按「这条连接没带请求」处理，与不认识的载荷同等对待。
    fn wait_for_payload(pipe: HANDLE, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut available = 0u32;
            // SAFETY: 句柄是 `create_named_pipe` 刚建出的服务端实例（有读权限）；
            // 三个指针要么为 null（PeekNamedPipe 允许 lpbuffer 为 null，此时只问
            // 可用字节数），要么指向本函数栈上的 u32。无可变共享状态。
            let ok = unsafe {
                PeekNamedPipe(
                    pipe,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            } != 0;
            // 问不出来 = 对端已断开（ERROR_BROKEN_PIPE）或管道已坏，两种都按
            // 「没有请求」处理，这里不区分（区分了也只有日志差别）。
            if !ok {
                return false;
            }
            if available > 0 {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(READ_POLL);
        }
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

    /// 管道往返的自动化覆盖。**这条链路此前只有「人工点一次横幅」验过**：
    /// 通道名、字节模式、载荷上限、UTF-8 解码，任何一处改错的症状都是
    /// 「点横幅完全没反应」，而编译器与文件里既有的两条单测（`channel_name`、
    /// `guard_enabled`）都不会响。
    #[cfg(test)]
    mod tests {
        use super::*;

        /// 测试专用的通道名。**不能复用真壳那一条**：具名管道允许有多个实例，
        /// `CreateFileW` 连到哪一个并不确定——连到真壳那一条就等于把用户的一次
        /// 「回到工作台」吃掉，还让本测试永远等不到自己的对端。
        fn test_channel(tag: &str) -> String {
            format!(r"\\.\pipe\dsh-xlink-test-{}-{tag}", std::process::id())
        }

        /// 客户端写 `workbench`，监听端必须原样读回来——这是"第二个进程把
        /// 意图交给在跑的那个"这一步的全部内容。
        #[test]
        fn payload_survives_the_round_trip() {
            // 载荷上限同时就是管道缓冲。上限一旦小于载荷，客户端的 `WriteFile`
            // 会**阻塞**而不是报错——本用例就会挂住而不是变红。先钉成一次快速
            // 断言，让「上限被改小」这件事以断言的形式红掉。
            assert!(
                PAYLOAD_LIMIT >= ACT_WORKBENCH.len(),
                "载荷上限（同时是管道缓冲）必须装得下载荷"
            );
            let channel = test_channel("roundtrip");
            let server = create_named_pipe(&channel).expect("建不出测试管道");
            let client = connect_named(&channel).expect("连不上测试管道");
            let payload = ACT_WORKBENCH.as_bytes();
            let mut written = 0u32;
            // SAFETY: 两个句柄都是刚建/刚连上的本地管道实例，写入源是本地数组，
            // 计数落回本地变量。载荷长度远小于管道缓冲，写入立即完成。
            let ok = unsafe {
                WriteFile(
                    client,
                    payload.as_ptr(),
                    payload.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            } != 0;
            assert!(ok, "客户端没能把载荷写进管道");
            assert_eq!(written as usize, payload.len(), "载荷被截断了");
            assert!(
                wait_for_payload(server, READ_TIMEOUT),
                "载荷已经写进去了却等不到它"
            );
            assert_eq!(read_message(server).as_deref(), Some(ACT_WORKBENCH));
            close(Some(server));
            close(Some(client));
        }

        /// 客户端连上却**不写数据**时，监听端必须自己超时走掉。少了这一步，
        /// 一个只连不写的第三方进程就能把激活通道挂死到壳退出为止——点横幅从此
        /// 静默失效，且没有任何日志。
        #[test]
        fn a_silent_client_cannot_block_the_channel() {
            let channel = test_channel("silent");
            let server = create_named_pipe(&channel).expect("建不出测试管道");
            let client = connect_named(&channel).expect("连不上测试管道");
            let started = std::time::Instant::now();
            assert!(
                !wait_for_payload(server, Duration::from_millis(200)),
                "静默客户端不该被判成带了一条请求"
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "wait_for_payload 必须自己超时退出，而不是把监听线程挂死"
            );
            close(Some(server));
            close(Some(client));
        }
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

    /// 交接失败出声的判据：内核没在跑 → 静默，内核在跑 → 出声。
    ///
    /// 反向验：把这个判据写成常量 `true`（旧行为）时点任务栏启动项会重新出现
    /// 「回到工作台失败」；写成 `false` 则端口冲突这类真故障重新变成静默——
    /// 两者都是用户看得见的症状，所以这条单点判据值得钉住。
    #[test]
    fn only_a_real_fault_announces() {
        assert!(
            !announce_failure(false),
            "内核没跑时只是没有工作台，不是故障"
        );
        assert!(
            announce_failure(true),
            "内核在跑却打不开工作台是故障，必须出声"
        );
    }

    /// 激活判据的完整语义表（`banner_activation`）。
    ///
    /// 这个判据的错误方向是不对称的，两头各有一种用户可见症状，所以四面都钉：
    /// 判据恒 `false` → 点横幅仍然完全没反应（本次要修的 bug）；恒 `true` →
    /// 用户点自己的面板 / Cmd-Tab 时凭空多开一个工作台窗口。
    ///
    /// **跟着判据一起挂 cfg**：判据本身是 macOS 专属的（见上面那段说明），
    /// Windows 上连同它的测试一起不存在。
    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_fresh_banner_claims_the_activation() {
        const MIN: u64 = 60_000;
        // 点刚弹出的横幅：未读 + 刚完成 → 接管。
        assert!(banner_activation(1, Some(MIN), MIN + BANNER_FRESH_MS));
        // 没有未读（用户已经看过、角标清了）→ 不接管，哪怕刚完成过。
        assert!(!banner_activation(0, Some(MIN), MIN + 1));
        // 没有完成记录（面板刚清空过）→ 不接管。
        assert!(!banner_activation(3, None, MIN));
        // 太久之前的完成：那是"普通的把应用叫回来"，归 `RunEvent::Reopen` 管。
        assert!(!banner_activation(1, Some(MIN), MIN + BANNER_FRESH_MS + 1));
        // 时钟回拨 / 完成时刻落在未来：不能当成"永远新鲜"——那会让每一次激活
        // 都被横幅接管，正是判据写反时最贵的症状。
        assert!(!banner_activation(1, Some(u64::MAX), 0));
        assert!(!banner_activation(1, Some(MIN), MIN - 1));
    }
}
