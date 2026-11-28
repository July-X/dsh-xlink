//! 外观作用域：工作台页面按 WebView 更新，原生装饰只更新其独立宿主。
//!
//! 这些规则各对应一次真机故障或用户定案：
//!
//! 1. **只设当前 NSWindow**（2026-10-09）：tao 的 `set_theme` / 声明式主题会落到
//!    整个 `NSApplication`，一扇窗的固定配色会把别的窗口的系统配色查询一起带走。
//!    所以原生设置在 [`apply_native`] 里逐窗口做。
//! 2. **建窗时不替工作台钉主题**（2026-10-18）：工作台（[`WORKBENCH_LABEL`]）加载
//!    的是内核 Web UI，`system` 一档由内核在页内用
//!    `matchMedia('(prefers-color-scheme: dark)')` 解析。建窗钉死会让那个查询恒为
//!    同一档（实测：系统浅色 + 偏好 system，工作台仍渲染深色）。
//! 3. **内核 system 档由壳给出答案**（2026-11-02 用户定案）：内核 `ui-theme = system`
//!    的含义是「跟随外壳」，所以主壳把自己的偏好上报到 [`SHELL_THEME`]；工作台的
//!    注入脚本在内核偏好为 system 时采用它，内核选了固定 light/dark 时不采用——
//!    用户在内核里明确选的档优先。
//! 4. **工作台开着时，Windows 上壳自有窗口不下发窗口主题**（2026-11-02 用户报
//!    「内核界面和 titlebar 切换速度不一致」）：下发原生主题也会写进**共享的
//!    WebView2 profile**，把工作台页面的 `prefers-color-scheme` 提前改掉，而工作台
//!    标题栏还在等镜像脚本——两半一前一后。即使主壳和副窗改用原生标题栏，也保留
//!    这个保护。工作台没开时照常下发，让 profile 提前对齐。见 [`applies_native_theme`]。
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Theme, Window};

use crate::shell::window::{CHROME_BACKDROP, CHROME_BACKDROP_LIGHT};

/// 工作台文档的稳定 WebView label；独立宿主也用它选择主题策略。
pub const WORKBENCH_LABEL: &str = "harness";
pub const MCODE_WORKBENCH_LABEL: &str = "mcode-workbench";

/// 内核 `window.open` 弹窗的 label 前缀（账号登录的授权页、终端链接等）。
///
/// 它们加载的是 `about:blank` 随后导航去的**别人的页面**（或内核自己的回调页），
/// 与工作台同理：外观由页面 / 系统决定，壳不替它钉。
pub const POPUP_LABEL_PREFIX: &str = "workbench-popup";

/// 这扇窗的外观是不是由**壳**决定。
///
/// 壳自有窗口（面板与三扇副窗）是：主题真值在壳的 localStorage 里，Rust 建窗时
/// 读不到，于是建窗先钉深色避免首帧闪白，页面挂载后 `applyTheme()` 再按持久化
/// 偏好纠正本窗（见 `ui/src/shell/bridge.js` 的 `setWindowTheme`）。
///
/// 下面两类**不是**：
/// - 工作台（[`WORKBENCH_LABEL`]）：主题真值在内核的 `ui-theme` 设置里；
/// - 内核 `window.open` 弹窗（[`POPUP_LABEL_PREFIX`]）：页面是授权站点 / 内核回调页。
///
/// 给它们钉任何配色都会让页内的 `prefers-color-scheme` 恒为那一档，
/// 「跟随系统」从此失效。
pub fn owns_appearance(label: &str) -> bool {
    label != WORKBENCH_LABEL
        && label != MCODE_WORKBENCH_LABEL
        && !label.starts_with(POPUP_LABEL_PREFIX)
}

/// 建窗兜底主题。`label` 是这扇窗自己的 label——[`owns_appearance`] 按它判断，
/// 所以新增建窗链时必须把窗口 label 传进来，别让壳替内核的页面做决定。
///
/// Windows 的窗口主题 API 有窗口作用域；macOS 建窗阶段不能强制应用主题（那会落到
/// `NSApplication` 上），所以 macOS 返回 `None`，由页加载时的 [`initialize`] 按窗口
/// 补一次深色。
///
/// **工作台与内核弹窗在两个平台都返回 `None`**：它们不是「先深后纠正」，而是
/// 从头到尾由页面 / 内核决定。Windows 上这一条尤其要紧——`window.theme()` 会经
/// `tauri-runtime-wry` 变成 WebView2 的 `SetPreferredColorScheme`
/// （wry 0.57 `src/webview2/mod.rs` 的 `set_theme`），钉住它等于钉住
/// `prefers-color-scheme`。
pub fn initial_theme(label: &str) -> Option<Theme> {
    if !owns_appearance(label) {
        return None;
    }
    if cfg!(target_os = "macos") {
        None
    } else {
        Some(Theme::Dark)
    }
}

/// 页加载时给**壳自有窗口**一个深色起步；页面挂载后按持久化偏好纠正本窗。
///
/// 工作台与内核弹窗直接跳过，理由见 [`owns_appearance`]：这条钩子是 app 级的
/// （`lib.rs` 的 `.on_page_load` 对每个 webview 都跑），不按窗口分流的话它会把
/// 它们也钉成深色。
pub fn initialize(window: Window) {
    if !owns_appearance(window.label()) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let target = window.clone();
        if let Err(error) = window.run_on_main_thread(move || {
            if let Err(error) = apply_native(&target, Some(Theme::Dark)) {
                eprintln!("dsh-xlink: 初始化窗口外观失败：{error}");
            }
        }) {
            eprintln!("dsh-xlink: 无法调度窗口外观初始化：{error}");
        }
    }
    // 非 macOS 在页加载后没有外观动作：标题栏主题在建窗期就定了（见
    // [`initial_theme`]，走 Tauri 的窗口主题实现）。**这个显式分支不能删**——
    // 砍掉它之后函数体在非 macOS 编译时只剩上面那条 `return`，clippy 的
    // `needless_return` 会建议把它删掉，而照删会让 macOS 失去对工作台 / 内核
    // 弹窗的豁免（那两扇窗的外观归页面自己决定）。
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
    }
}

/// 主壳当前**偏好**的镜像：外层 `None` = 还没上报过，`Some(None)` = 跟随系统。
///
/// 工作台脚本在拿到第一次上报前不动本窗外观——冷启动时把「还没答案」当成
/// 「system」会让内核的固定档在页面加载后被壳推翻。
static SHELL_THEME: Mutex<Option<Option<Theme>>> = Mutex::new(None);

/// 壳偏好变化时推给工作台的 DOM 事件名。
///
/// 不用 Tauri event 的 `emit`：工作台是 remote origin，页面侧 `listen` 属于 core
/// 插件命令，需要额外的 `core:event:allow-listen` 授权——漏了不会编译失败，症状
/// 是标题栏永远等下一拍轮询。`eval` 是本仓既有的「壳 → 工作台」通道（窗口标题、
/// 全屏状态都走它），不经过 ACL。
pub const SHELL_THEME_EVENT: &str = "dsh-shell-theme";

/// 工作台脚本读到的壳偏好。`reported = false` 表示主壳还没上报，保持现状。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ShellThemeView {
    pub reported: bool,
    pub theme: Option<Theme>,
    /// 仅用于检测系统变化，不是偏好，也不写回偏好。
    #[serde(rename = "systemTheme")]
    pub system_theme: Option<Theme>,
}

fn store_shell_theme(theme: Option<Theme>) -> bool {
    let mut slot = SHELL_THEME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if *slot == Some(theme) {
        return false;
    }
    *slot = Some(theme);
    true
}

fn shell_theme_view() -> ShellThemeView {
    let slot = SHELL_THEME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ShellThemeView {
        reported: slot.is_some(),
        theme: slot.flatten(),
        system_theme: None,
    }
}

/// 主壳在启动与每次切换后上报自己的偏好（`system` 传 `null`）。
#[tauri::command]
pub fn report_shell_theme(app: AppHandle, theme: Option<Theme>) {
    if !store_shell_theme(theme) {
        return;
    }
    push_shell_theme(&app, theme);
}

/// 把壳偏好推进工作台页面；窗口没开 / 还在加载态页时 `eval` 可能落空，脚本的
/// 轮询（`get_shell_theme`）会补上。
fn push_shell_theme(app: &AppHandle, theme: Option<Theme>) {
    let payload = serde_json::to_string(&theme).unwrap_or_else(|_| "null".to_string());
    let script = format!(
        "window.dispatchEvent(new CustomEvent('{SHELL_THEME_EVENT}', {{ detail: {payload} }}));"
    );
    for label in [WORKBENCH_LABEL, MCODE_WORKBENCH_LABEL] {
        if let Some(window) = app.get_webview(label) {
            let _ = window.eval(&script);
        }
    }
}

/// 工作台脚本读壳偏好：冷启动读一次，之后靠推送 + 轮询。
#[tauri::command]
pub async fn get_shell_theme(webview: tauri::Webview) -> Result<ShellThemeView, String> {
    let mut view = shell_theme_view();
    if view.reported && view.theme.is_none() {
        view.system_theme = Some(read_system_theme(&webview.window()).await?);
    }
    Ok(view)
}

/// 这扇窗的窗口级主题是否值得下发。
///
/// Windows 的壳自有窗口（主壳 + 日志 / 用量 / 套餐）使用原生标题栏，但修改
/// 窗口主题也会经 tao 的 `ThemeChanged` → wry 的 `WebView::set_theme` 写进
/// **共享的 WebView2 profile**（所有窗口共用默认 profile）。
///
/// **工作台开着时必须跳过**：那次下发会把内核页面的 `prefers-color-scheme` 提前
/// 改掉，而工作台标题栏要等外观镜像脚本那一步才变——两半一前一后（2026-11-02
/// 用户报「切换速度不一致」）。工作台没开时照常下发：让共享 profile 提前对齐
/// 壳主题，下次开工作台不必先渲染一帧 OS 配色再翻。
///
/// 工作台走到这个守卫之前已经单独处理；其他非壳窗口保持原来的路径。
fn applies_native_theme(label: &str, workbench_open: bool) -> bool {
    !(cfg!(target_os = "windows") && workbench_open && owns_appearance(label))
}

/// 工作台页面的窗口 / WebView 底色：浅色页取白，其余取既有深色首帧底。
///
/// 页面给滚动条留了 `scrollbar-gutter: stable`，但槽里不画底色——露出来的就是
/// 这层。macOS 开「始终显示滚动条」时那条槽一直占位，浅色页配深底就是右侧一条
/// 黑边（2026-11-09 用户截图）。外观已经按页面主题同步过了，底色跟着换即可。
fn workbench_backdrop(theme: Theme) -> tauri::webview::Color {
    if theme == Theme::Light {
        CHROME_BACKDROP_LIGHT
    } else {
        CHROME_BACKDROP
    }
}

#[tauri::command]
pub async fn set_window_appearance(
    webview: tauri::Webview,
    theme: Option<Theme>,
) -> Result<(), String> {
    let window = webview.window();
    if matches!(webview.label(), WORKBENCH_LABEL | MCODE_WORKBENCH_LABEL) {
        let resolved = match theme {
            Some(theme) => theme,
            None => read_system_theme(&window).await?,
        };
        apply_workbench_view(&webview, resolved).await?;
        webview
            .set_background_color(Some(workbench_backdrop(resolved)))
            .map_err(|error| format!("设置工作台底色失败：{error}"))?;
        // 主题信号与底色只作用于调用者；绝不改内嵌宿主的装饰。
        let host = webview.window();
        if host.label() != "main" {
            #[cfg(target_os = "macos")]
            run_on_main(&host, move |window| apply_native(&window, theme)).await?;
            #[cfg(target_os = "windows")]
            run_on_main(&host, move |window| {
                apply_workbench_caption(&window, resolved)
            })
            .await?;
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            host.set_theme(theme).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let workbench_open = [WORKBENCH_LABEL, MCODE_WORKBENCH_LABEL]
        .into_iter()
        .any(|label| window.app_handle().get_webview(label).is_some());
    if !applies_native_theme(webview.label(), workbench_open) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    run_on_main(&window, move |window| apply_native(&window, theme)).await?;
    #[cfg(not(target_os = "macos"))]
    window.set_theme(theme).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn run_on_main<T: Send + 'static>(
    window: &Window,
    f: impl FnOnce(Window) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(f(target));
        })
        .map_err(|e| e.to_string())?;
    rx.await.map_err(|e| format!("窗口外观响应中断：{e}"))?
}

async fn read_system_theme(window: &Window) -> Result<Theme, String> {
    #[cfg(target_os = "macos")]
    return run_on_main(window, |_| {
        use objc2::{class, msg_send, runtime::AnyObject};
        // SAFETY: 在 AppKit 主线程读取未被强制覆盖的 NSApplication 外观。
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let appearance: *mut AnyObject = msg_send![app, effectiveAppearance];
            let name: *mut AnyObject = msg_send![appearance, name];
            let dark: objc2::runtime::Bool = msg_send![name, hasSuffix: cocoa_string(c"DarkAqua")];
            Ok(if dark.as_bool() {
                Theme::Dark
            } else {
                Theme::Light
            })
        }
    })
    .await;
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Registry::{
            RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD,
        };
        let _ = window;
        let key: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
            .encode_utf16()
            .chain([0])
            .collect();
        let name: Vec<u16> = "AppsUseLightTheme".encode_utf16().chain([0]).collect();
        let mut light = 1u32;
        let mut size = std::mem::size_of_val(&light) as u32;
        // SAFETY: 两个 UTF-16 字符串均以 NUL 结尾；输出缓冲区为一个 DWORD。
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut light as *mut u32).cast(),
                &mut size,
            )
        };
        Ok(if status == 0 && light == 0 {
            Theme::Dark
        } else {
            Theme::Light
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    window.theme().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
unsafe fn cocoa_string(value: &std::ffi::CStr) -> *mut objc2::runtime::AnyObject {
    use objc2::{class, msg_send};
    unsafe { msg_send![class!(NSString), stringWithUTF8String: value.as_ptr()] }
}

#[cfg(target_os = "macos")]
unsafe fn named_appearance(theme: Theme) -> *mut objc2::runtime::AnyObject {
    use objc2::{class, msg_send};
    let name = if theme == Theme::Dark {
        c"NSAppearanceNameDarkAqua"
    } else {
        c"NSAppearanceNameAqua"
    };
    unsafe { msg_send![class!(NSAppearance), appearanceNamed: cocoa_string(name)] }
}

#[cfg(target_os = "macos")]
async fn apply_workbench_view(webview: &tauri::Webview, resolved: Theme) -> Result<(), String> {
    use objc2::{msg_send, runtime::AnyObject};
    let (tx, rx) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |platform| {
            // SAFETY: with_webview 在 UI 线程提供活的 WKWebView；只覆盖此 NSView。
            unsafe {
                let target = &*platform.inner().cast::<AnyObject>();
                let _: () = msg_send![target, setAppearance: named_appearance(resolved)];
            }
            let _ = tx.send(());
        })
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|e| format!("工作台 WebKit 外观响应中断：{e}"))
}

#[cfg(target_os = "windows")]
async fn apply_workbench_view(webview: &tauri::Webview, resolved: Theme) -> Result<(), String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::HSTRING;

    let color = if resolved == Theme::Dark {
        "dark"
    } else {
        "light"
    };
    let params =
        serde_json::json!({ "features": [{ "name": "prefers-color-scheme", "value": color }] });
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    webview
        .with_webview(move |platform| {
            let reply = tx.clone();
            let handler =
                CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
                    let _ = reply.try_send(result.map_err(|error| error.to_string()));
                    Ok(())
                }));
            // SAFETY: UI 线程持有该 WebView2 的 COM 控制器；CDP 仅作用于这一页面，
            // 不修改同进程其他 WebView 共用的 PreferredColorScheme。
            let sent = unsafe {
                platform.controller().CoreWebView2().and_then(|core| {
                    core.CallDevToolsProtocolMethod(
                        &HSTRING::from("Emulation.setEmulatedMedia"),
                        &HSTRING::from(params.to_string()),
                        &handler,
                    )
                })
            };
            if let Err(error) = sent {
                let _ = tx.try_send(Err(error.to_string()));
            }
        })
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .map_err(|_| "同步工作台 WebView2 外观超时".to_string())?
        .ok_or_else(|| "工作台 WebView2 外观响应中断".to_string())?
}

#[cfg(target_os = "windows")]
fn apply_workbench_caption(window: &Window, resolved: Theme) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as windows_sys::Win32::Foundation::HWND;
    let dark = resolved == Theme::Dark;
    set_workbench_caption(hwnd, dark)?;
    // SAFETY: 同一窗口线程；回调只保存一个布尔值，销毁窗口时注销。重复注册更新该值。
    if unsafe { SetWindowSubclass(hwnd, Some(workbench_caption_proc), 0, usize::from(dark)) } == 0 {
        return Err("无法监听独立工作台标题栏的系统主题变化".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn workbench_caption_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    dark: usize,
) -> isize {
    use windows_sys::Win32::UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass},
        WindowsAndMessaging::{WM_NCDESTROY, WM_SETTINGCHANGE},
    };
    // SAFETY: comctl32 在其窗口线程调用；销毁时先注销，再交给原窗口过程。
    unsafe {
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(workbench_caption_proc), id);
        }
        let result = DefSubclassProc(hwnd, msg, wparam, lparam);
        if msg == WM_SETTINGCHANGE {
            // tao 可能按系统主题重置 DWM，即使没有发出 ThemeChanged。
            if let Err(error) = set_workbench_caption(hwnd, dark != 0) {
                crate::shell::shell_events::record("harness-window", &error);
            }
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn set_workbench_caption(
    hwnd: windows_sys::Win32::Foundation::HWND,
    dark: bool,
) -> Result<(), String> {
    use windows_sys::Win32::{
        Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
        UI::{
            Input::KeyboardAndMouse::GetActiveWindow,
            WindowsAndMessaging::{DefWindowProcW, WM_NCACTIVATE},
        },
    };
    let dark = i32::from(dark);
    // SAFETY: 在窗口线程使用活 HWND，DWM 读取一个 BOOL。不要走 Tauri set_theme：
    // 它会让 tao 广播 ThemeChanged 并触发 Wry 改写 WebView2 profile。
    let set = |attribute| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&dark as *const i32).cast(),
            std::mem::size_of_val(&dark) as u32,
        )
    };
    if set(DWMWA_USE_IMMERSIVE_DARK_MODE as u32) < 0 {
        let status = set(19); // 较早的 Windows 10 版本使用旧编号。
        if status < 0 {
            return Err(format!("设置独立工作台标题栏失败（HRESULT {status:#x}）"));
        }
    }
    // SAFETY: 按 tao 同样的方式重绘标题栏，不改变实际焦点。
    unsafe {
        let active = GetActiveWindow() == hwnd;
        DefWindowProcW(hwnd, WM_NCACTIVATE, usize::from(!active), 0);
        DefWindowProcW(hwnd, WM_NCACTIVATE, usize::from(active), 0);
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn apply_workbench_view(_: &tauri::Webview, _: Theme) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_native(window: &Window, theme: Option<Theme>) -> Result<(), String> {
    use objc2::{msg_send, runtime::AnyObject};
    let native = window.ns_window().map_err(|e| e.to_string())?;
    // SAFETY: run_on_main_thread 保证 AppKit 线程；NSWindow 由持有的 Tauri 句柄保活。
    unsafe {
        let appearance = theme.map_or(std::ptr::null_mut(), |theme| named_appearance(theme));
        let native = &*(native as *const AnyObject);
        let _: () = msg_send![native, setAppearance: appearance];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 壳偏好镜像：先「没上报」，再 system（None），再固定档；重复上报同一档
    /// 不该再广播（工作台每次轮询都会读到同一份值）。
    #[test]
    fn shell_theme_reports_roundtrip_and_only_announces_changes() {
        assert!(
            !shell_theme_view().reported,
            "测试开始前不应有上报（本模块唯一写者）"
        );
        assert!(store_shell_theme(None), "第一次上报必须算变化");
        let view = shell_theme_view();
        assert!(view.reported);
        assert_eq!(view.theme, None, "system 档上报为 None");
        assert!(!store_shell_theme(None), "重复上报同一档不该再广播");
        assert!(store_shell_theme(Some(Theme::Dark)));
        assert_eq!(shell_theme_view().theme, Some(Theme::Dark));
        assert!(!store_shell_theme(Some(Theme::Dark)));
        assert!(store_shell_theme(Some(Theme::Light)));
        assert_eq!(shell_theme_view().theme, Some(Theme::Light));
    }

    /// Windows 上工作台开着时，避免壳自有窗口改写共享 WebView2 profile，
    /// 导致内核页面先于工作台标题栏变色。工作台没开时照常下发（给下次开窗
    /// 对齐 profile）；非壳窗口不受此守卫约束。
    #[test]
    fn windows_shell_windows_skip_the_shared_webview_profile_only_while_workbench_is_open() {
        let expected = !cfg!(target_os = "windows");
        assert_eq!(
            applies_native_theme("main", true),
            expected,
            "工作台开着时，Windows 的壳自有窗口不下发窗口主题"
        );
        assert_eq!(
            applies_native_theme("log-viewer", true),
            expected,
            "副窗同理"
        );
        assert!(
            applies_native_theme("main", false),
            "工作台没开时照常下发，给下次开窗把共享 profile 对齐"
        );
        assert!(
            applies_native_theme(WORKBENCH_LABEL, true),
            "工作台不归壳自有窗口主题策略管理"
        );
        assert!(
            applies_native_theme("workbench-popup-1", true),
            "内核弹窗不归壳管，但窗口主题动作本身不该被跳过"
        );
    }

    /// 工作台永远不吃壳的建窗兜底：钉住它 = 把内核的「跟随系统」钉死。
    #[test]
    fn workbench_never_gets_a_forced_theme() {
        assert!(!owns_appearance(WORKBENCH_LABEL));
        assert_eq!(initial_theme(WORKBENCH_LABEL), None);
        assert!(!owns_appearance(MCODE_WORKBENCH_LABEL));
        assert_eq!(initial_theme(MCODE_WORKBENCH_LABEL), None);
    }

    /// 工作台底色跟页面主题走：浅色页配深底会让 macOS「始终显示滚动条」下的
    /// stable gutter 变成右侧一条黑边（2026-11-09 用户截图）。
    #[test]
    fn workbench_backdrop_follows_the_page_theme() {
        assert_eq!(workbench_backdrop(Theme::Light), CHROME_BACKDROP_LIGHT);
        assert_eq!(workbench_backdrop(Theme::Dark), CHROME_BACKDROP);
        assert_eq!(
            CHROME_BACKDROP_LIGHT,
            tauri::webview::Color(0xff, 0xff, 0xff, 0xff),
            "浅色页取页面主背景的白"
        );
    }

    /// 内核 `window.open` 弹窗同理：页面是授权站点 / 内核回调页，外观由页面与系统决定。
    #[test]
    fn workbench_popups_never_get_a_forced_theme() {
        let label = format!("{POPUP_LABEL_PREFIX}-1");
        assert!(!owns_appearance(&label));
        assert_eq!(initial_theme(&label), None);
    }

    /// 壳自有窗口照旧：macOS 靠页加载兜底（建窗返回 `None`），其他平台建窗即深色。
    /// 两个平台都**不**返回浅色——浅色是页面按用户偏好纠正出来的结果，不是建窗兜底。
    #[test]
    fn shell_windows_keep_the_dark_bootstrap() {
        assert!(owns_appearance("main"));
        let expected = if cfg!(target_os = "macos") {
            None
        } else {
            Some(Theme::Dark)
        };
        assert_eq!(initial_theme("main"), expected);
        assert_eq!(initial_theme("usage-viewer"), expected);
    }
}
