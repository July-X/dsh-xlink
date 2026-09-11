//! Windows 通知区域（托盘）图标。
//!
//! 管理面板在 Windows 上「常驻后台」：标题栏的关闭按钮与最小化按钮都只是
//! 把主窗口收起来，内核、官方对话与更新检查继续运行；真正的退出走托盘右键
//! 菜单的「退出」。托盘图标因此必须存在——没有它就等于「窗口消失且无法找回」。
//!
//! 关闭语义的唯一实现在 [`intercept_close`]：它在窗口关闭请求上调用
//! `prevent_close()` 并隐藏窗口。macOS 不参与（那里最小化/关闭沿用系统语义，
//! Dock 承担常驻入口），所以整个模块按 `cfg(windows)` 编译。

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

/// 主管理窗口的 label（与 `tauri.conf.json`、其它命令保持一致）。
pub const MAIN_WINDOW: &str = "main";
/// 托盘菜单项：把主窗口调回前台。
const MENU_SHOW: &str = "tray-show";
/// 托盘菜单项：真正退出（复用前端已有的退出确认流程）。
const MENU_QUIT: &str = "tray-quit";
/// 托盘图标：32×32 带白色圆角底板的鲸鱼，与 `scripts/build-icons.sh` 生成的
/// 桌面图标同一套几何（见 `docs/icon-design.md`）。用 `include_image!` 在编译期
/// 解码成 RGBA 常量，因此运行时不需要图片解码依赖。
const TRAY_ICON: tauri::image::Image<'static> = tauri::include_image!("icons/tray-32.png");

/// 连续关闭（用户反复点 X）时不要刷屏：只有距上次提示超过该间隔才再发一次。
const CLOSE_HINT_INTERVAL_MS: u64 = 4000;

static LAST_CLOSE_HINT_AT: AtomicU64 = AtomicU64::new(0);

/// 建立托盘图标。必须在事件循环启动前的 `setup` 里调用。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "显示主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出 dsh-xlink", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(TRAY_ICON)
        .tooltip("dsh-xlink 桌面管理台")
        // 右键出菜单；左键在下面单独处理成「显示主界面」——Windows 上单击
        // 托盘图标把窗口叫回来，比让用户先右键再选更顺手。
        .show_menu_on_left_click(false)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_shell(app),
            MENU_QUIT => request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_shell(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 把主窗口收进通知区域：隐藏窗口，并从任务栏移除它的按钮。
///
/// `set_skip_taskbar(true)` 走的是 `ITaskbarList::DeleteTab`，会把任务栏按钮
/// 直接删掉——只 `hide()` 不够：窗口虽然不可见，任务栏按钮与 Alt+Tab 条目
/// 仍在，点它会得到一个空窗口。恢复时（[`show_main_shell`]）必须加回来，
/// 否则窗口回来也不在任务栏上。
pub fn hide_to_tray(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.set_skip_taskbar(true);
    let _ = window.hide();
}

/// 把主窗口从隐藏 / 最小化状态恢复到前台。
///
/// 与工作台拉绳（`commands::focus_main_shell`）共用同一套动作，但**动作只能
/// 写在这一处**：`crate::show_main_shell` 在 Windows 上会调回本函数，如果本
/// 函数再回调它就是一个无限递归（首版就是这么写的，点托盘图标直接
/// `thread 'main' has overflowed its stack`）。所以这里直接做窗口操作，
/// `crate::show_main_shell` 只是「按平台选实现」的分发点。
pub fn show_main_shell(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    // 先恢复任务栏条目再显示：`DeleteTab` 之后窗口即便可见也不会回到任务栏，
    // 顺序反了会让用户看到一个「没有任务栏按钮」的窗口。
    let _ = window.set_skip_taskbar(false);
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_always_on_top(false);
    let _ = window.set_focus();
}

/// 关闭请求的常驻化处理：把主窗口收进托盘而不是退出进程。
///
/// 返回 `true` 表示这次关闭已被接管（调用方应停止后续处理）。`prevent_close()`
/// 必须在这里就调用，否则窗口会真的开始关闭。
pub fn intercept_close(app: &AppHandle, label: &str, api: &tauri::CloseRequestApi) -> bool {
    if label != MAIN_WINDOW {
        return false;
    }
    api.prevent_close();
    hide_to_tray(app);
    notify_hidden_once(app);
    true
}

/// 首次（以及隔一会儿之后）收起窗口时告诉用户「程序还在跑、去哪找它」，
/// 否则窗口连同任务栏按钮一起消失时会像是崩了。连续收起按
/// [`CLOSE_HINT_INTERVAL_MS`] 节流，避免刷屏。
pub fn notify_hidden_once(app: &AppHandle) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_CLOSE_HINT_AT.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= CLOSE_HINT_INTERVAL_MS
        && LAST_CLOSE_HINT_AT
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        let _ = app.emit("shell-hidden-to-tray", ());
    }
}

/// 托盘菜单的「退出」：复用前端已有的「确认退出」流程。
///
/// 退出路径不能在这里直接 `app.exit()`——内核仍在运行时需要先问用户，
/// 并让前端依次执行 `stop_kernel` 与 `confirm_close_shell`。所以这里只把
/// 窗口叫回前台并广播同一条 `request-quit-confirm` 事件，与操作系统关闭
/// 按钮走完全相同的分支。
fn request_quit(app: &AppHandle) {
    let official_chat_open = app.get_window("official-chat").is_some();
    show_main_shell(app);
    let _ = app.emit(
        "request-quit-confirm",
        serde_json::json!({
            "kernel_running": crate::kernel_running(app),
            "official_chat_open": official_chat_open,
            "from_tray": true,
        }),
    );
}
