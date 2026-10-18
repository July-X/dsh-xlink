//! 窗口外观的作用域规则：**只设当前窗口，且只设壳自己的窗口**。
//!
//! 两条规则各对应一次真机故障：
//!
//! 1. **只设当前 NSWindow**（2026-10-09）：tao 的 `set_theme` / 声明式主题会落到
//!    整个 `NSApplication`，一扇窗的固定配色会把别的窗口的系统配色查询一起带走。
//!    所以原生设置在 [`apply_native`] 里逐窗口做。
//! 2. **只设壳自己的窗口**（2026-10-18）：工作台（[`WORKBENCH_LABEL`]）加载的是
//!    内核 Web UI，它的明暗由内核的 `ui-theme` 设置决定，而 `system` 一档是内核
//!    在页内用 `matchMedia('(prefers-color-scheme: dark)')` 解析的。壳一旦给这扇
//!    窗钉住外观，那个查询就恒为同一档：用户在设置里点「跟随系统」，页面不会跟随
//!    （实测：系统浅色 + 偏好 system，工作台仍渲染深色）。
use tauri::{Theme, Window};

/// 工作台窗口的 label。两条建窗链（`harness_window::build` 与
/// `commands::open_harness`）都用它；`check-invariants` 第 18 项按它核对
/// 「工作台一条建窗主题都不许钉」。
pub const WORKBENCH_LABEL: &str = "harness";

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
    label != WORKBENCH_LABEL && !label.starts_with(POPUP_LABEL_PREFIX)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 工作台永远不吃壳的建窗兜底：钉住它 = 把内核的「跟随系统」钉死。
    #[test]
    fn workbench_never_gets_a_forced_theme() {
        assert!(!owns_appearance(WORKBENCH_LABEL));
        assert_eq!(initial_theme(WORKBENCH_LABEL), None);
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
