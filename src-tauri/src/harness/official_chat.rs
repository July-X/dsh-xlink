//! 官方对话窗口：常量、页签表、布局计算与重排判据。
//!
//! 这一组是纯粹的窗口事实——地址、页签表、窗口与页签栏尺寸、持久化目录
//! 标识，以及「某个逻辑尺寸该怎么切成 strip 加内容」这段算术。它不进数据
//! 库、不读设置，只在建窗、挂载子视图与 relayout 时用。从 `commands.rs`
//! 摘出来有两个原因：那份文件已到反棘轮上限，而「官方对话有哪些页签、窗口
//! 多大」这类问题在这里终于有一个明确落点。
//!
//! 搬的时候把 `logical_window_size` 也带过来了——它在 `commands.rs` 里仅剩
//! 的一个调用方是 `relayout_official_chat`，与本页的初始尺寸兜底同属一条
//! 链。留在原处会让 `harness` 反过来依赖 `commands`。

use tauri::WindowEvent;

/// `open_official_chat` 加载到专用 `official-chat` webview 中的
/// DeepSeek 官方对话入口。
///
/// 该窗口不覆盖用户代理：WebView2 引擎本身就是真正的桌面 Edge/Chromium
/// 构建。覆盖 UA 字符串会在请求头里声称是 Chrome，但 `Sec-CH-UA` 客户
/// 端提示和原生 `navigator.userAgentData` 仍然报出真正的 Edge 品牌——
/// 这种跨层不一致正是环境检测会盯上的东西，所以诚实的身份也是一致
/// 的身份。
pub const OFFICIAL_CHAT_URL: &str = "https://chat.deepseek.com";

// WKWebView 在 macOS 上把 cookies 和 localStorage 存到这个标识符下。
// 在跨发布版之间保持 ID 稳定，并区分别名为 debug 与 release 的数据。
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) const OFFICIAL_CHAT_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-dev-001";
#[cfg(all(target_os = "macos", not(debug_assertions)))]
pub(crate) const OFFICIAL_CHAT_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-rel-001";

/// 传给 `official-chat` webview 的 Chromium feature 开关。
///
/// `additional_browser_args` 会**替换** wry 自带的默认集合
/// （`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`），
/// 因此这里把相关条目重新声明一遍，避免悄悄被重新启用：少了这些项
/// 之后，WebView2 会显示 SmartScreen 拦截页以及只有 Edge 才有的浮层
/// UI，而普通桌面 Chrome 是不会有这些东西的。在此基础上，
/// `AutomationControlled`（既作为浏览器 feature，又作为 blink runtime
/// 标志位）阻止 Chromium 在引擎层就上报 `navigator.webdriver = true`，
/// 让任何 initialization_script 都没机会遮盖它；`TranslateUI` /
/// `InterestFeedContentSuggestions` 则压制更多 Edge-only 的界面。只有
/// WebView2 后端会消费这些浏览器参数；macOS / Linux 会忽略它们，因此
/// builder 的接线不必分平台分支。同一个 user-data 目录必须配一致的参
/// 数（per-folder options），这也是 `commands::open_official_chat` 把这个
/// 常量与专用 user-data 目录配对使用的原因。
pub const OFFICIAL_CHAT_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,AutomationControlled,TranslateUI,InterestFeedContentSuggestions --disable-blink-features=AutomationControlled";

/// 第二个官方对话页签：MiniMax agent。
pub const OFFICIAL_CHAT_MINIMAX_URL: &str = "https://agent.minimaxi.com";

/// 官网网页版窗口页签栏中按展示顺序排列的固定页签。第一个条目是打开时
/// 默认激活的页签。增加一行即可增加一个页签——strip webview 在运行
/// 时通过 `commands::official_chat_tabs` 发现这份列表，而内容 webview
/// 是在被选中时才惰性创建的，所以初次打开时不会加载任何其它站点。
pub const OFFICIAL_CHAT_TABS: &[(&str, &str)] = &[
    ("DeepSeek", OFFICIAL_CHAT_URL),
    ("MiniMax", OFFICIAL_CHAT_MINIMAX_URL),
];

/// 裸窗口的 label。一个 `Window`（不是 `WebviewWindow`）承载 strip 加
/// 每个页签对应的一个子 `Webview`；关窗时它们会一并被拆解。
pub(crate) const OFFICIAL_CHAT_WINDOW_LABEL: &str = "official-chat";
/// 用于渲染页签栏的本地 SPA webview（`index.html?chatstrip=1`）。它保
/// 留 `window.__TAURI__`——`chat-fingerprint.js` 不在这里注入——
/// 因此可以调用 `commands::official_chat_tabs` /
/// `commands::switch_official_chat_tab`。拉绳小台灯也放在这里，因为
/// 在 Tauri 2.11 / wry 0.55.1 这版上，子 WebView 的透明效果并不可靠。
/// 紧凑的小台灯和页签控件可以共用同一个 38px 高的 strip。
pub(crate) const OFFICIAL_CHAT_STRIP_LABEL: &str = "official-chat-strip";
/// 被钉在顶部的页签栏的逻辑高度。紧凑的 24×38 台灯 SVG 正好放进
/// 38px 高的页签栏中。
pub(crate) const OFFICIAL_CHAT_INITIAL_WIDTH: f64 = 1366.0;
pub(crate) const OFFICIAL_CHAT_INITIAL_HEIGHT: f64 = 768.0;
pub(crate) const OFFICIAL_CHAT_STRIP_HEIGHT: f64 = 38.0;

/// 把 Tao 的物理 client-area 尺寸转换为逻辑点。
pub(crate) fn logical_window_size(width: u32, height: u32, scale: f64) -> Option<(f64, f64)> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let width = width as f64 / scale;
    let height = height as f64 / scale;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some((width, height))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OfficialChatLayout {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) strip_height: f64,
    pub(crate) content_y: f64,
    pub(crate) content_height: f64,
}

/// 能使原生子视图 frame 失效的事件。
#[derive(Clone, Copy)]
pub(crate) enum OfficialChatRelayoutTrigger {
    Geometry,
    Focused(bool),
    Other,
}

pub(crate) fn should_relayout_for_trigger(trigger: OfficialChatRelayoutTrigger) -> bool {
    matches!(
        trigger,
        OfficialChatRelayoutTrigger::Geometry | OfficialChatRelayoutTrigger::Focused(true)
    )
}

pub(crate) fn official_chat_layout(width: f64, height: f64) -> OfficialChatLayout {
    let width = width.max(0.0);
    let height = height.max(0.0);
    OfficialChatLayout {
        width,
        height,
        strip_height: OFFICIAL_CHAT_STRIP_HEIGHT.min(height),
        content_y: OFFICIAL_CHAT_STRIP_HEIGHT.min(height),
        content_height: (height - OFFICIAL_CHAT_STRIP_HEIGHT).max(0.0),
    }
}

pub(crate) fn official_chat_initial_size(width: u32, height: u32, scale: f64) -> (f64, f64) {
    logical_window_size(width, height, scale)
        .filter(|(width, height)| {
            *width >= OFFICIAL_CHAT_STRIP_HEIGHT && *height >= OFFICIAL_CHAT_STRIP_HEIGHT
        })
        .unwrap_or((OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT))
}

pub(crate) fn current_official_chat_layout(window: &tauri::Window) -> OfficialChatLayout {
    let scale = window.scale_factor().unwrap_or(1.0);
    let phys = window.inner_size().unwrap_or_default();
    let (width, height) = official_chat_initial_size(phys.width, phys.height, scale);
    official_chat_layout(width, height)
}

/// 判断某个原生 window 事件是否会改变子视图的几何信息。
pub(crate) fn should_relayout_official_chat(event: &WindowEvent) -> bool {
    let trigger = match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            OfficialChatRelayoutTrigger::Geometry
        }
        WindowEvent::Focused(focused) => OfficialChatRelayoutTrigger::Focused(*focused),
        _ => OfficialChatRelayoutTrigger::Other,
    };
    should_relayout_for_trigger(trigger)
}

/// 判断某个逻辑布局是否合理到可以应用到子 webview。AppKit 在刚创建
/// 完一个 macOS 窗口之后可能立刻报告一个极小的临时 client size；把这
/// 种布局应用上去会在每次 relayout 时把 strip 和内容 webview 都缩回去。
/// 该判断与 [`official_chat_initial_size`] 中的初始尺寸兜底相互呼应；
/// 真正的布局 bug 修复落在 window builder 的 title-bar style 上（见
/// `commands::open_official_chat`）。
pub(crate) fn official_chat_layout_plausible(layout: OfficialChatLayout) -> bool {
    layout.width >= OFFICIAL_CHAT_STRIP_HEIGHT && layout.height >= OFFICIAL_CHAT_STRIP_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_retina_pixels_to_logical_points_once() {
        assert_eq!(logical_window_size(2732, 1536, 2.0), Some((1366.0, 768.0)),);
        assert_eq!(logical_window_size(1366, 768, 0.0), None);
    }

    #[test]
    fn ignores_tiny_provisional_window_metrics_for_initial_layout() {
        assert_eq!(
            official_chat_initial_size(1366, 6, 1.0),
            (OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT),
        );
        assert_eq!(
            official_chat_initial_size(6, 768, 1.0),
            (OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT),
        );
        assert_eq!(official_chat_initial_size(2732, 1536, 2.0), (1366.0, 768.0),);
    }

    #[test]
    fn reserves_the_strip_once_for_content() {
        let layout = official_chat_layout(1366.0, 768.0);

        assert_eq!(layout.width, 1366.0);
        assert_eq!(layout.height, 768.0);
        assert_eq!(layout.strip_height, OFFICIAL_CHAT_STRIP_HEIGHT);
        assert_eq!(layout.content_y, OFFICIAL_CHAT_STRIP_HEIGHT);
        assert_eq!(layout.content_height, 730.0);
    }

    #[test]
    fn clamps_layout_when_window_is_shorter_than_the_strip() {
        let layout = official_chat_layout(640.0, 24.0);

        assert_eq!(layout.width, 640.0);
        assert_eq!(layout.height, 24.0);
        assert_eq!(layout.strip_height, 24.0);
        assert_eq!(layout.content_y, 24.0);
        assert_eq!(layout.content_height, 0.0);
    }

    #[test]
    fn relayout_rejects_tiny_provisional_layouts_that_collapse_macos_windows() {
        // AppKit 在 macOS 上创建窗口后会立刻报告几像素大小的临时 client
        // size；如果照此应用，strip 和内容 webview 都会坍缩成那条窄
        // 缝。relayout 必须保留上一次良好的 frame。
        assert!(!official_chat_layout_plausible(official_chat_layout(
            1366.0, 3.0,
        )));
        assert!(!official_chat_layout_plausible(official_chat_layout(
            4.0, 768.0,
        )));
        // 一个真实的窗口总是至少和页签栏一样大。
        assert!(official_chat_layout_plausible(official_chat_layout(
            1366.0, 768.0,
        )));
    }

    #[test]
    fn every_official_chat_tab_uses_the_same_content_region() {
        let layout = official_chat_layout(1366.0, 768.0);
        let regions: Vec<_> = OFFICIAL_CHAT_TABS
            .iter()
            .map(|_| (layout.width, layout.content_y, layout.content_height))
            .collect();

        // 不钉具体页签数：增删页签是常规操作，钉死数字只会让每次调整都要来改
        // 测试，而它真正要防的是**空列表上空过**——`windows(2)` 在空表上恒真。
        assert!(
            !OFFICIAL_CHAT_TABS.is_empty(),
            "页签表不能为空，否则下面那条「所有页签内容区相同」会在空列表上白过"
        );
        assert!(regions.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn relayouts_for_geometry_events_but_not_focus_loss() {
        assert!(should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Geometry,
        ));
        assert!(should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Focused(true),
        ));
        assert!(!should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Focused(false),
        ));
        assert!(!should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Other,
        ));

        assert!(should_relayout_official_chat(&WindowEvent::Resized(
            tauri::PhysicalSize::new(1366, 768),
        )));
        assert!(!should_relayout_official_chat(&WindowEvent::Focused(false)));
    }
}
