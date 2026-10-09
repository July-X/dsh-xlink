//! macOS 外观必须按 NSWindow 设置；tao 的 set_theme 会覆盖整个 NSApplication。
use tauri::{Theme, Window};

/// Windows 的窗口主题 API 有窗口作用域；macOS 建窗阶段不能强制应用主题。
pub fn initial_theme() -> Option<Theme> {
    if cfg!(target_os = "macos") {
        None
    } else {
        Some(Theme::Dark)
    }
}

/// 页加载时给本窗口默认深色；壳页面随后按持久化偏好纠正，工作台保持深色。
pub fn initialize(window: Window) {
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
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

#[tauri::command]
pub async fn set_window_appearance(window: Window, theme: Option<Theme>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let target = window.clone();
        window
            .run_on_main_thread(move || {
                let result = apply_native(&target, theme);
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        rx.await.map_err(|e| format!("窗口外观响应中断：{e}"))?
    }
    #[cfg(not(target_os = "macos"))]
    window.set_theme(theme).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn apply_native(window: &Window, theme: Option<Theme>) -> Result<(), String> {
    use objc2::{class, msg_send, runtime::AnyObject};
    let native = window.ns_window().map_err(|e| e.to_string())?;
    // SAFETY: run_on_main_thread 保证 AppKit 线程；NSWindow 由持有的 Tauri 句柄保活。
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let nil: *mut AnyObject = std::ptr::null_mut();
        // 应用永远继承系统；任何固定配色都只落在自己的 NSWindow 上。
        let _: () = msg_send![app, setAppearance: nil];
        let appearance: *mut AnyObject = match theme {
            Some(theme) => {
                let name = if theme == Theme::Dark {
                    c"NSAppearanceNameDarkAqua"
                } else {
                    c"NSAppearanceNameAqua"
                };
                let name: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
                msg_send![class!(NSAppearance), appearanceNamed: name]
            }
            None => nil,
        };
        let native = &*(native as *const AnyObject);
        let _: () = msg_send![native, setAppearance: appearance];
    }
    Ok(())
}
