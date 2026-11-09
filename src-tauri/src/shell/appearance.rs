//! 窗口外观的作用域规则：**只设当前窗口，且只设壳自己的窗口**。
//!
//! 三条规则各对应一次真机故障或用户定案：
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
//!    「内核界面和 titlebar 切换速度不一致」）：主壳与三扇副窗都是自绘标题栏，
//!    窗口主题在它们身上没有原生 chrome 可画，唯一可见的效果是写进**共享的
//!    WebView2 profile**，把工作台页面的 `prefers-color-scheme` 提前改掉，而工作台
//!    标题栏还在等镜像脚本——两半一前一后。工作台没开时照常下发，让 profile 提前
//!    对齐，下次开工作台不会先闪一帧 OS 配色。见 [`applies_native_theme`]。
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Theme, Window};

use crate::shell::window::{CHROME_BACKDROP, CHROME_BACKDROP_LIGHT};

/// 工作台窗口的 label。两条建窗链（`harness_window::build` 与
/// `commands::open_harness`）都用它；`check-invariants` 第 18 项按它核对
/// 「工作台一条建窗主题都不许钉」。
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
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.eval(&script);
        }
    }
}

/// 工作台脚本读壳偏好：冷启动读一次，之后靠推送 + 轮询。
#[tauri::command]
pub fn get_shell_theme() -> ShellThemeView {
    shell_theme_view()
}

/// 这扇窗的窗口级主题是否值得下发。
///
/// Windows 的壳自有窗口（主壳 + 日志 / 用量 / 套餐）建窗期就是 `decorations(false)`
/// 的自绘标题栏，窗口主题在它们身上没有原生 chrome 可画。它唯一可见的效果是经
/// tao 的 `ThemeChanged` → wry 的 `WebView::set_theme` 写进**共享的 WebView2
/// profile**（所有窗口共用默认 profile）。
///
/// **工作台开着时必须跳过**：那次下发会把内核页面的 `prefers-color-scheme` 提前
/// 改掉，而工作台标题栏要等外观镜像脚本那一步才变——两半一前一后（2026-11-02
/// 用户报「切换速度不一致」）。工作台没开时照常下发：让共享 profile 提前对齐
/// 壳主题，下次开工作台不必先渲染一帧 OS 配色再翻。
///
/// 工作台与内核弹窗（[`owns_appearance`] 为 false）永远照旧：它们就是靠这次下发
/// 把标题栏与页面一起切过去。
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
pub async fn set_window_appearance(window: Window, theme: Option<Theme>) -> Result<(), String> {
    let workbench_open = window
        .app_handle()
        .get_webview_window(WORKBENCH_LABEL)
        .is_some();
    if !applies_native_theme(window.label(), workbench_open) {
        return Ok(());
    }
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
        rx.await.map_err(|e| format!("窗口外观响应中断：{e}"))??;
    }
    #[cfg(not(target_os = "macos"))]
    window.set_theme(theme).map_err(|e| e.to_string())?;
    // 工作台页面不画滚动条槽的底色，槽里露出的是窗口 / WebView 那层。两处都选
    // 「跟随系统」时外观已经解除覆盖，按当前系统档取底色。
    if window.label() == WORKBENCH_LABEL || window.label() == MCODE_WORKBENCH_LABEL {
        let resolved = theme.unwrap_or_else(|| window.theme().unwrap_or(Theme::Dark));
        if let Some(webview_window) = window.app_handle().get_webview_window(window.label()) {
            webview_window
                .set_background_color(Some(workbench_backdrop(resolved)))
                .map_err(|error| format!("设置工作台底色失败：{error}"))?;
        }
    }
    Ok(())
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

    /// Windows 上壳自有窗口是自绘标题栏：工作台开着时下发窗口主题只会写共享
    /// WebView2 profile，把内核页面提前改色而标题栏还没动；工作台没开时照常
    /// 下发（给下次开窗对齐 profile）。工作台 / 弹窗永远照旧下发。
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
            "工作台就是靠这一步把标题栏与内核页面一起切过去"
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
