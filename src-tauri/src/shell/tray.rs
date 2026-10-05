//! Windows 通知区域（托盘）图标。
//!
//! 管理面板在 Windows 上「常驻后台」：标题栏的关闭按钮与最小化按钮都只是
//! 把主窗口收起来，内核、官方对话与更新检查继续运行；真正的退出走托盘右键
//! 菜单的「退出」。托盘图标因此必须存在——没有它就等于「窗口消失且无法找回」。
//!
//! **关闭语义与 macOS 已统一**（2026-10-02），实现在
//! [`crate::shell::resident`]：本模块只负责 Windows 特有的那几件事——
//! 按 DPI 与任务栏主题选帧、`ITaskbarList` 删任务栏按钮、监听注册表主题变化。
//! 「收起 / 恢复 / 退出」这三个动作刻意不在这里另写一份，两端各有一份就会
//! 漂移（2026-10-02 之前 macOS 走的就是另一套语义）。macOS 对端见
//! [`crate::shell::menu_bar`]。
//!
//! 整个模块按 `cfg(windows)` 编译。

use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// 主管理窗口的 label（与 `tauri.conf.json`、其它命令保持一致）。
pub const MAIN_WINDOW: &str = "main";
/// 托盘图标 id：`refresh_icon` 靠它取回同一个托盘项写新帧。
const TRAY_ID: &str = "main-tray";
// 菜单项（显示 / 退出）的 id 与接线都在 `resident::build_background_menu`，
// 与 macOS 菜单栏共用一份——见该函数的文档注释。

/// 托盘图标的全部档位：由 `scripts/build-icons.sh` 从 `assets/whale-head.svg`
/// 生成的 `tray-*.png`——整条鲸鱼 + 放大的红眼。两套帧对应两种任务栏主题，
/// 运行时按 [`taskbar_is_light`] 选一套、按 [`small_icon_size`] 选一档：
///
/// - `tray-light-*`（浅色任务栏）：套板，与桌面图标（`32x32.png` / `icon.ico`）
///   同一套板规则。浅色任务栏上白瓦片的边缘几乎不可见，效果是给鲸鱼留了内边距，
///   与桌面/任务栏按钮看到的是同一个图标。
/// - `tray-dark-*`（深色任务栏）：透明底 + 白描边（白描边是**在目标尺寸上**做的
///   1px/档栅格膨胀）。白瓦片在深色任务栏上是一块刺眼的白方块，读作"徽标"
///   而不是系统图标；近黑的鲸体又必须靠这圈白边才认得出轮廓。
///
/// 两套共十二档都用 `include_image!` 在编译期解码成 RGBA 常量，运行时不需要
/// 图片解码依赖。几何与取舍见 `docs/icon-design.md`。
const TRAY_FRAMES_DARK: [(i32, tauri::image::Image<'static>); 6] = [
    (16, tauri::include_image!("icons/tray-dark-16.png")),
    (20, tauri::include_image!("icons/tray-dark-20.png")),
    (24, tauri::include_image!("icons/tray-dark-24.png")),
    (32, tauri::include_image!("icons/tray-dark-32.png")),
    (40, tauri::include_image!("icons/tray-dark-40.png")),
    (48, tauri::include_image!("icons/tray-dark-48.png")),
];
const TRAY_FRAMES_LIGHT: [(i32, tauri::image::Image<'static>); 6] = [
    (16, tauri::include_image!("icons/tray-light-16.png")),
    (20, tauri::include_image!("icons/tray-light-20.png")),
    (24, tauri::include_image!("icons/tray-light-24.png")),
    (32, tauri::include_image!("icons/tray-light-32.png")),
    (40, tauri::include_image!("icons/tray-light-40.png")),
    (48, tauri::include_image!("icons/tray-light-48.png")),
];

/// `SystemUsesLightTheme` 所在子键与值名：通知区域的底色跟着它走。
const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const LIGHT_THEME_VALUE: &str = "SystemUsesLightTheme";

/// 通知区域槽位的边长（物理像素）。
///
/// Windows 用 `SM_CXSMICON` 告诉应用托盘图标该画多大：100/125/150/200% 缩放下
/// 分别是 16/20/24/32。而壳交给 shell 的 HICON 是按图片自身尺寸建的（`tray-icon`
/// 的 Windows 实现走 `CreateIcon(w, h)`），尺寸不对就只能由 shell 缩放——那样
/// 自带的 16px 帧永远显示不出来，最小档还要多挨一次缩放。所以这里按当前 DPI 选帧。
///
/// DPI 取主窗口所在显示器：通知区域在任务栏上，而任务栏基本就在用户正在用的那块
/// 屏幕；窗口换屏或系统缩放变化时 `WindowEvent::ScaleFactorChanged` 会回调
/// [`refresh_icon`] 重取。
fn small_icon_size(app: &AppHandle) -> i32 {
    use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON;

    let dpi = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|window| window.hwnd().ok())
        .map(|hwnd| unsafe { GetDpiForWindow(hwnd.0) })
        .filter(|dpi| *dpi > 0)
        .unwrap_or_else(|| unsafe { GetDpiForSystem() });
    unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }
}

/// 取不小于目标边长的最小一档；超出最大档时用最大档（放大总比糊成一片好）。
fn frame_for(
    frames: &[(i32, tauri::image::Image<'static>)],
    size: i32,
) -> tauri::image::Image<'static> {
    let index = frames
        .iter()
        .position(|(frame, _)| *frame >= size)
        .unwrap_or(frames.len() - 1);
    frames[index].1.clone()
}

/// UTF-16 + NUL 结尾，供 Win32 的 `…W` 系列接口使用。
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 打开 `Personalize` 键。`KEY_READ` 已包含 `KEY_NOTIFY`，读值与等变化共用。
fn open_personalize_key() -> Option<windows_sys::Win32::System::Registry::HKEY> {
    use windows_sys::Win32::System::Registry::{RegOpenKeyExW, HKEY_CURRENT_USER, KEY_READ};

    let subkey = wide(PERSONALIZE_KEY);
    let mut key = std::ptr::null_mut();
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut key) };
    (status == 0 && !key.is_null()).then_some(key)
}

/// 任务栏（连同通知区域）当前是不是浅色。
///
/// **读不到时按深色处理**：透明底 + 白描边在两种底色上都能看，白瓦片在深色
/// 任务栏上不能看——失败要退到两种底色都成立的那一边。
fn taskbar_is_light() -> bool {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegQueryValueExW, REG_DWORD};

    let Some(key) = open_personalize_key() else {
        return false;
    };
    let name = wide(LIGHT_THEME_VALUE);
    let mut kind = 0u32;
    let mut data = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            &mut data as *mut u32 as *mut u8,
            &mut len,
        )
    };
    unsafe { RegCloseKey(key) };
    status == 0 && kind == REG_DWORD && data == 1
}

/// 阻塞等待 `Personalize` 键的下一次变化，成功返回 `true`。
///
/// 主题切换既不广播给应用（通知区域不是我们的窗口），注册表也没有回调，
/// 所以直接把这次等待压在注册表上：`hEvent` 为空、`bAsync = 0` 时
/// `RegNotifyChangeKeyValue` 会阻塞到变化发生，不占 CPU。
fn wait_personalize_change() -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET,
    };

    let Some(key) = open_personalize_key() else {
        return false;
    };
    let status = unsafe {
        RegNotifyChangeKeyValue(key, 0, REG_NOTIFY_CHANGE_LAST_SET, std::ptr::null_mut(), 0)
    };
    unsafe { RegCloseKey(key) };
    status == 0
}

/// 当前该显示哪一帧：先按任务栏主题选一套，再按显示缩放选一档。
fn tray_icon(app: &AppHandle) -> tauri::image::Image<'static> {
    let frames = if taskbar_is_light() {
        &TRAY_FRAMES_LIGHT
    } else {
        &TRAY_FRAMES_DARK
    };
    frame_for(frames, small_icon_size(app))
}

/// 主题切换后换一套帧。用一个后台线程等注册表变化，而不是轮询：切换浅/深色后
/// 图标立刻跟上，空闲时不占 CPU。线程与进程同生命周期，退出时随进程结束。
pub fn watch_theme(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last = taskbar_is_light();
        loop {
            if !wait_personalize_change() {
                // 等不到（键不存在 / 权限异常）时退避，避免变成忙循环。
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
            let now = taskbar_is_light();
            if now == last {
                continue;
            }
            last = now;
            // 托盘图标只能在主线程上改（tray-icon 把更新发给自己的窗口）。
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || refresh_icon(&handle));
        }
    });
}

/// 显示缩放或任务栏主题变化后重选帧。
///
/// 托盘图标是 shell 侧的一张 HICON 快照：系统不会替我们按新 DPI 重取，而
/// `tray-icon` 也不处理 DPI 变化（它的 Windows 实现里只有 `TaskbarCreated`
/// 的重建，没有 `WM_DPICHANGED`），所以必须在窗口缩放事件里自己重设一次。
pub fn refresh_icon(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Err(error) = tray.set_icon(Some(tray_icon(app))) {
        eprintln!(
            "dsh-xlink: 无法按新的显示缩放/任务栏主题更新通知区域图标（{error}）；\
             托盘图标会继续用旧样式，显示略糊但功能不受影响。\
             若图标明显错位或消失，重启应用即可恢复。"
        );
    }
}

/// 建立托盘图标。必须在事件循环启动前的 `setup` 里调用。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon(app))
        .tooltip("dsh-xlink 桌面管理台")
        // 右键出菜单；左键在共用接线里处理成「显示主界面」——Windows 上单击
        // 托盘图标把窗口叫回来，比让用户先右键再选更顺手。
        .show_menu_on_left_click(false);
    super::resident::build_background_menu(builder, app, "显示主界面", "退出 dsh-xlink")?;
    // 托盘项建好之后再起主题监听：用户切浅/深色模式时换另一套帧（见 [`watch_theme`]）。
    watch_theme(app);
    Ok(())
}

/// 把主窗口收进通知区域：隐藏窗口，并从任务栏移除它的按钮。
///
/// `set_skip_taskbar(true)` 走的是 `ITaskbarList::DeleteTab`，会把任务栏按钮
/// 直接删掉——只 `hide()` 不够：窗口虽然不可见，任务栏按钮与 Alt+Tab 条目
/// 仍在，点它会得到一个空窗口。恢复时（[`super::resident::show_main_shell`]）
/// 必须加回来，否则窗口回来也不在任务栏上。
pub fn hide_to_tray(app: &AppHandle) {
    super::resident::hide_to_shell(app);
    // 最小化是**用户主动**的收起：置位后，本进程第一次恢复时会补发「收进
    // 后台」提示（登录自启的隐藏不走这里，开机后第一次唤回是静默的——
    // 见 `resident::consume_restore_hint`）。
    super::resident::mark_hidden_by_user();
}

/// 把主窗口从隐藏 / 最小化状态恢复到前台。
///
/// 与工作台拉绳（`commands::focus_main_shell`）共用同一套动作，而动作本身在
/// [`super::resident::show_main_shell`]。本函数只做 Windows 独有的一件事：
/// 补回任务栏按钮（`DeleteTab` 之后窗口即便可见也不会回到任务栏，顺序反了
/// 会让用户看到一个「没有任务栏按钮」的窗口）。
///
/// **这里不能反过来调 `crate::show_main_shell`**：那个函数在 Windows 上就是
/// 本模块的「按平台选实现」分发点，两边互调是无限递归（首版即如此，点托盘
/// 图标直接 `thread 'main' has overflowed its stack`）。
pub fn show_main_shell(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.set_skip_taskbar(false);
    super::resident::show_main_shell(app);
}
