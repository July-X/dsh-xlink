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

use std::path::{Path, PathBuf};

use tauri::WindowEvent;
use url::Url;

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

/// 条件页签的地址：本机 Codex 登录可用时追加的 ChatGPT 网页版。
///
/// 放在末尾而不是插在中间，与 `subscription.rs` 的 `PROVIDER_ORDER` 同一
/// 条纪律（新增项追加、不重排既有项）：页签下标就是内容 webview 的 label
/// 数字后缀，插在中间会让已经挂载的 webview 与新名单错位。
pub const OFFICIAL_CHAT_CHATGPT_URL: &str = "https://chatgpt.com";

/// 官网网页版窗口页签栏中**恒定存在**的页签，按展示顺序排列。第一个条目
/// 是打开时默认激活的页签。strip webview 在运行时通过
/// `commands::official_chat_tabs` 发现这份列表，而内容 webview 是在被选中
/// 时惰性创建的，所以初次打开时不会加载任何其它站点。
pub const OFFICIAL_CHAT_TABS: &[(&str, &str)] = &[
    ("DeepSeek", OFFICIAL_CHAT_URL),
    ("MiniMax", OFFICIAL_CHAT_MINIMAX_URL),
];

/// **按登录状态追加**在固定页签之后的条件页签。
///
/// 与 [`OFFICIAL_CHAT_TABS`] 分开而不是合进一张表，是因为判据在运行期才成立
/// （见 [`crate::usage::subscription_codex::chatgpt_web_available`]）：Codex 的
/// 访问令牌会自己轮换，`active_usage()` 认的是 `exp`，于是同一次运行里
/// 「页签在不在」可能翻转。页签表因此在**窗口打开那一刻求值一次**并快照，
/// 由 `AppState::official_chat_tabs` 持有——快照之外的每一次读取都拿同一份
/// 名单，条带渲染、内容 webview 的 label、relayout 的循环才不会各读各的。
///
/// 判据为真的那些条目按本表顺序追加在末尾。
pub const OFFICIAL_CHAT_OPTIONAL_TABS: &[(&str, &str)] = &[("ChatGPT", OFFICIAL_CHAT_CHATGPT_URL)];

/// 求值出这一轮真正要挂的页签表：固定页签 + 命中的条件页签。
///
/// 纯函数（判据以参数传入），所以「条件页签只追加在末尾、既有下标不动」
/// 这条性质能被单测钉住，而不必真去读一遍凭据。
pub(crate) fn resolved_tabs(chatgpt_available: bool) -> Vec<(&'static str, &'static str)> {
    let mut tabs: Vec<(&'static str, &'static str)> = OFFICIAL_CHAT_TABS.to_vec();
    if chatgpt_available {
        tabs.extend_from_slice(OFFICIAL_CHAT_OPTIONAL_TABS);
    }
    tabs
}

/// 认证域：登录链**必须留在壳内**。
///
/// ## 为什么单列一条判据
///
/// 外部 `http(s)` 一律交系统浏览器，是**普通外链**的正确处置（壳不做浏览器，
/// 用户的链接就该去用户的浏览器）。但**认证链**不行：认证在别的浏览器里完成
/// ⇒ 会话 cookie 落在**那个浏览器**的存储里 ⇒ 壳内的 webview 永远拿不到
/// 登录态 ⇒ 用户看到的是「登录做完了，但页签还是未登录」——**静默失败**，
/// 壳侧一行日志都没有。
///
/// 这不是假想：ChatGPT 的「使用 Google 账户继续」实测是**同帧导航**
/// （不是弹窗），所以今天走的是同帧那条路；但同一条流程一旦改成弹窗形态
/// （上游随时可以改），外部 http(s) 交系统浏览器这条处置就会把认证链
/// 劈成两半。判据要有，才能拦住。
///
/// ## 名单是「身份提供方的账号主机」
///
/// 不是「所有看起来像登录的地址」——那种名单会越滚越大，最后把普通外链
/// 也劫持进壳内弹窗。列进来的每一个都是**站点登录页会跳过去的那个账号
/// 主机**（ChatGPT 登录页实测提供 Google / Apple / 两条；OpenAI 自己的
/// `auth.openai.com` 是 OAuth 回调落点）。
pub(crate) fn is_login_domain(url: &Url) -> bool {
    const LOGIN_HOSTS: &[&str] = &[
        // Google 账号主机（ChatGPT 登录页的「使用 Google 账户继续」跳这里）
        "accounts.google.com",
        // OpenAI 自己的 OAuth 回调落点
        "auth.openai.com",
        // Apple ID
        "appleid.apple.com",
        "idmsa.apple.com",
        // Microsoft 账号
        "login.microsoftonline.com",
        "login.live.com",
        // GitHub（很多站点的第三方登录）
        "github.com",
    ];
    match url.host_str() {
        Some(host) => {
            let host = host.to_ascii_lowercase();
            LOGIN_HOSTS
                .iter()
                .any(|known| host == *known || host.ends_with(&format!(".{known}")))
        }
        None => false,
    }
}

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

/// 下载落点名字的兜底：两边都推不出名字时用它（例如 `blob:` 地址且页面没给
/// `download` 名字）。
pub(crate) const OFFICIAL_CHAT_DOWNLOAD_FALLBACK: &str = "official-chat-download";

/// 官网网页版内容页签的下载文件名。
///
/// `suggested` 是 WebView2 在 `DownloadStarting` 时预先给的建议路径——`<a download>`
/// 那个名字就落在这里，通常也是唯一可信的名字。它取不到时（`blob:` 下载常见）才
/// 从 URL 末段推。名字来自**别人的站点**，所以一律过
/// [`sanitize_download_file_name`]：不允许目录分隔符、控制字符与 Windows 保留字符
/// 把落点带出下载目录。
pub(crate) fn download_file_name(url: &Url, suggested: Option<&Path>) -> String {
    let suggested = suggested
        .and_then(Path::file_name)
        .map(|name| sanitize_download_file_name(&name.to_string_lossy()))
        .filter(|name| !name.is_empty());
    if let Some(name) = suggested {
        return name;
    }
    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(sanitize_download_file_name)
        .filter(|name| !name.is_empty());
    from_url.unwrap_or_else(|| OFFICIAL_CHAT_DOWNLOAD_FALLBACK.to_string())
}

/// 把页面给的文件名收成「一个安全的文件名」。
///
/// 只做文件名这一段：`<>:"/\|?*` 与控制字符换成 `_`，首尾的空白 / 句点去掉
/// （Windows 上以点结尾的名字建不出来），长度收到 180 个字符。`.` / `..` 这类名字
/// 清理后为空，由调用方回落。
pub(crate) fn sanitize_download_file_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(360));
    for ch in raw.chars() {
        if out.chars().count() >= 180 {
            break;
        }
        out.push(match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        });
    }
    out.trim_start().trim_end_matches([' ', '.']).to_owned()
}

/// 同一目录下不覆盖已有文件的落点：重名时按 `名字 (1).ext` 递增。
///
/// 只是**尽力**去重（检查与下载开始之间仍有窗口），但足以避免「第二次下载同一份
/// 文档把第一次的覆盖掉」——WebView2 自己的重名策略与我们不同，显式落点之后这一层
/// 得由壳来兜。
pub(crate) fn unique_download_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, extension) = split_download_name(name);
    for round in 1..=999u32 {
        let candidate = dir.join(format!("{stem} ({round}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem} (1000){extension}"))
}

/// `name` → (`stem`, `.ext`)；没有扩展名时扩展名为空串。
fn split_download_name(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            (stem.to_owned(), format!(".{extension}"))
        }
        _ => (name.to_owned(), String::new()),
    }
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
        // 条件页签一并纳入：它同样是内容 webview，同样要占满 strip 下方。
        let regions: Vec<_> = resolved_tabs(true)
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

    /// 条件页签只允许**追加在末尾**。页签下标就是内容 webview 的 label
    /// （`official-chat-tab-<index>`），插在中间会让本次运行里已经挂载的
    /// webview 与名单错位——切到新下标会加载出另一个站点。
    #[test]
    fn conditional_tabs_are_appended_without_moving_existing_indices() {
        let base = resolved_tabs(false);
        let with_chatgpt = resolved_tabs(true);

        // 判据为假时必须恰好是固定页签，一条不多。
        assert_eq!(base, OFFICIAL_CHAT_TABS.to_vec());
        // 判据为真却没有追加任何条件页签 = 条件页签表与接线脱钩。
        assert!(with_chatgpt.len() > base.len());
        // 条件页签插到了中间或重排了既有页签：既有下标会指向别的站点。
        assert_eq!(&with_chatgpt[..base.len()], &base[..]);
        // 追加段必须原样是条件页签表。
        assert_eq!(
            with_chatgpt[base.len()..].to_vec(),
            OFFICIAL_CHAT_OPTIONAL_TABS.to_vec()
        );
    }

    /// 认证域判定：身份提供方的账号主机留在壳内，其余外链照旧交系统浏览器。
    ///
    /// 这条判据的**后果**是「登录链不被劈成两半」，所以它必须同时钉住
    /// 两头：认得的账号主机**要**命中，不该命中的普通外链**不能**命中。
    #[test]
    fn login_domains_are_recognized_by_their_account_hosts() {
        // ChatGPT 登录页实测跳过去的那个 Google 账号主机（2026-10-20 实测
        // 抓到的真实地址形态：accounts.google.com/v3/signin/identifier?...）
        let google = Url::parse(
            "https://accounts.google.com/v3/signin/identifier?client_id=799222349882-x&redirect_uri=https%3A%2F%2Fauth.openai.com%2Fapi%2Faccounts%2Fcallback%2Fgoogle",
        )
        .expect("url");
        assert!(is_login_domain(&google));

        // OpenAI 自己的 OAuth 回调落点
        let openai =
            Url::parse("https://auth.openai.com/api/accounts/callback/google").expect("url");
        assert!(is_login_domain(&openai));

        // 子域也算（区域账号主机等）
        let sub = Url::parse("https://foo.accounts.google.com/signin").expect("url");
        assert!(is_login_domain(&sub));
    }

    #[test]
    fn ordinary_external_links_are_not_login_domains() {
        // 普通外链：交系统浏览器（现状不变）
        for url in [
            "https://chatgpt.com/",
            "https://chat.deepseek.com/sign_in",
            "https://help.openai.com/zh-hans-cn/",
            "https://accounts.google.com.evil.example/",
            "https://notaccounts.google.com/",
            "https://example.com/",
        ] {
            let parsed = Url::parse(url).expect("url");
            assert!(
                !is_login_domain(&parsed),
                "{url} 不该被当成认证域（后缀匹配必须按点分界，不能按字符串包含）"
            );
        }
    }

    #[test]
    fn a_url_without_a_host_is_not_a_login_domain() {
        // `mailto:` 之类没有 host 的地址：不是认证域，也不该 panic
        let mailto = Url::parse("mailto:someone@example.com").expect("url");
        assert!(!is_login_domain(&mailto));
    }

    /// 条件页签存在时，ChatGPT 必须在里面；且它排在固定页签之后。
    #[test]
    fn chatgpt_appears_only_when_available_and_after_the_fixed_tabs() {
        let without = resolved_tabs(false);
        assert!(!without.iter().any(|(title, _)| *title == "ChatGPT"));

        let with_chatgpt = resolved_tabs(true);
        let position = with_chatgpt
            .iter()
            .position(|(title, _)| *title == "ChatGPT")
            .expect("判据为真时必须有 ChatGPT 页签");
        assert_eq!(position, OFFICIAL_CHAT_TABS.len());
        assert_eq!(with_chatgpt[position].1, OFFICIAL_CHAT_CHATGPT_URL);
    }

    /// 每次求值都从固定页签重新构造，返回值不共享上一轮的缓冲区。
    /// 快照会被 `AppState` 长期持有，别处再拿它去 extend 就会互相踩。
    #[test]
    fn resolution_does_not_alias_the_constant_table() {
        let mut first = resolved_tabs(false);
        first.push(("mutated", "https://example.invalid"));
        assert_eq!(resolved_tabs(false), OFFICIAL_CHAT_TABS.to_vec());
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

    /// 页面给的名字优先：`<a download>` 的名字就在 WebView2 的建议路径里。
    #[test]
    fn download_file_name_prefers_the_name_the_page_suggests() {
        let url = Url::parse("https://files.oaiusercontent.com/file-abc?se=2026").expect("url");
        let suggested = Path::new("/Downloads/设计规范 v1.0.docx");
        assert_eq!(
            download_file_name(&url, Some(suggested)),
            "设计规范 v1.0.docx"
        );
    }

    /// 名字来自别人的站点：目录分隔符与 Windows 保留字符不许把落点带出下载目录。
    #[test]
    fn download_file_name_cannot_escape_the_download_directory() {
        let url = Url::parse("https://chatgpt.com/backend-api/files/x/download").expect("url");
        // 建议路径的目录段由 `Path::file_name` 摘掉（两个平台都成立），
        // 剩下的这一段再过一遍清洗。
        assert_eq!(
            download_file_name(&url, Some(Path::new("boot.ini"))),
            "boot.ini"
        );
        assert_eq!(
            sanitize_download_file_name(r"..\..\boot.ini"),
            ".._.._boot.ini"
        );
        assert_eq!(sanitize_download_file_name("a<bad>.txt"), "a_bad_.txt");
        // 以点结尾的名字在 Windows 上建不出来，清理后为空再回落。
        assert_eq!(sanitize_download_file_name("..."), "");
        let no_tail = Url::parse("https://chatgpt.com/").expect("url");
        assert_eq!(
            download_file_name(&no_tail, Some(Path::new("..."))),
            OFFICIAL_CHAT_DOWNLOAD_FALLBACK
        );
    }

    /// 没有建议名时（`blob:` 下载常见）从 URL 末段推，推不出来才用兜底名。
    #[test]
    fn download_file_name_falls_back_to_the_url_tail() {
        let url =
            Url::parse("https://chatgpt.com/backend-api/files/abc/report.pdf?x=1").expect("url");
        assert_eq!(download_file_name(&url, None), "report.pdf");
        let no_tail = Url::parse("https://chatgpt.com/").expect("url");
        assert_eq!(
            download_file_name(&no_tail, None),
            OFFICIAL_CHAT_DOWNLOAD_FALLBACK
        );
    }

    /// 重名不覆盖：第二次下载同一份文档落到 `名字 (1).ext`，扩展名保留。
    #[test]
    fn unique_download_path_keeps_earlier_downloads() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-official-chat-download-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("临时目录");
        assert_eq!(
            unique_download_path(&dir, "报告.docx"),
            dir.join("报告.docx")
        );
        std::fs::write(dir.join("报告.docx"), b"first").expect("占位");
        assert_eq!(
            unique_download_path(&dir, "报告.docx"),
            dir.join("报告 (1).docx")
        );
        std::fs::write(dir.join("报告 (1).docx"), b"second").expect("占位");
        assert_eq!(
            unique_download_path(&dir, "报告.docx"),
            dir.join("报告 (2).docx")
        );
        // 没有扩展名时不凭空造一个点。
        std::fs::write(dir.join("download"), b"third").expect("占位");
        assert_eq!(
            unique_download_path(&dir, "download"),
            dir.join("download (1)")
        );
        std::fs::remove_dir_all(&dir).expect("只清自己那块临时目录");
    }
}
