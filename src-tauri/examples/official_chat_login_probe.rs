//! 官方对话「ChatGPT 网页登录 · Google 快速认证」流程探针（macOS / WKWebView 实测）。
//!
//! # 这个探针要回答什么
//!
//! 现象：官方网页对话窗口里的 ChatGPT 页签，登录时**选不了 Google 邮箱做快速认证**。
//! 两个候选原因，各自有各的修法，**不能靠猜**：
//!
//! - (A) **嵌入式 webview 能力缺失**：Google 的 OAuth 在嵌入式 webview 里被拒
//!   （"This browser or app may not be secure" 一类），页面根本不给出账号选择器。
//! - (B) **没有本地浏览器 session**：WKWebView 的数据存储是壳专属的那一份，
//!   与 Safari / Chrome 完全隔离 ⇒ Google 侧没有已登录 session ⇒ 「快速认证」
//!   这条路（选账号即登录）在原理上就不存在。
//! - (C) **弹窗被路由到系统浏览器**：`window.open` 走 `harness_window::new_window_response`
//!   ⇒ 外部 `http(s)` 一律交系统浏览器（`harness_window.rs` 的 `popup_route`）。
//!   若 Google 认证是弹窗形态，则认证在**别的浏览器**里完成，**会话回不到壳的
//!   webview** ⇒ 即使认证成功，ChatGPT 页签也仍是未登录。
//!
//! 三条都要**实测**才知道哪条成立，所以这个探针按**生产接线**起一个 webview
//! （同一个 `data_store_identifier` 家族、同一个 `additional_browser_args`、同一份
//! `chat-fingerprint.js`、同一个 `on_new_window` 路由语义），然后：
//!
//! 1. 打开 ChatGPT 登录页，读回页面现场（标题 / 正文文本 / UA / cookie 名 /
//!    是否 Cloudflare 拦截页）；
//! 2. 列出页面上与 Google 有关的可点元素（标签、文案、href）；
//! 3. 点它，等页面稳定，读回**新**现场 + 探针捕获到的 `window.open` 地址；
//! 4. 若捕获到 `accounts.google.com` 的地址，就在**同一个 webview 里**直接打开它，
//!    看 Google 在 WKWebView 里到底渲染成什么（账号选择器 / 邮箱输入 / 拦截页）。
//!
//! # 怎么跑
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_login_probe
//! ```
//!
//! 输出是 `PROBE_REPORT_BEGIN` / `PROBE_REPORT_END` 之间的一段 JSON。
//!
//! # 隔离
//!
//! - **不碰用户的真实档案**：本探针用**自己的** `data_store_identifier`
//!   （`dsh-chat-probe-01`），不是生产的 `dsh-chat-rel-001` / `dsh-chat-dev-001`。
//!   WKWebView 的非默认数据存储是**按标识符全局落盘**的（不在 `DSH_XLINK_HOME`
//!   下面），所以这里必须换标识符，否则会往用户真实 profile 里写 cookie。
//! - **不发起真实登录**：探针只点「Continue with Google」这一下，然后只**读**
//!   页面现场。不会输入账号密码、不会选账号、不会授权。
//! - `window.open` 一律**拒绝**（生产那里是把外部 `http(s)` 交系统浏览器；探针
//!   不该在诊断时弹出用户的浏览器）。生产会怎么处理，写在报告的 `production_note` 里。

// 探针只用到 harness 的一部分，`#[path]` 把整份源文件编进来会带出一批
// 未使用的常量与函数——这不影响探针行为，不必逐个处理。
#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

// 生产代码里 `crate::shell::window::window_title` / `crate::shell::appearance::
// initial_theme` 是**壳自己的**模块；example 是另一个 crate，没有它们。
// 这里给一份**同名同形状**的最小实现，让 `#[path]` 进来的那份生产源码
// 在探针里也能编译——测的仍然是生产那份代码，不是副本。
//
// 本探针没有用到这两个函数（`official_chat.rs` 里用到它们的那几处在探针
// 路径上不会被调用），所以显式放行 dead_code——`cargo clippy --all-targets
// -D warnings` 会把 examples 也算进去，不放行就是一条门禁红。
#[allow(dead_code)]
mod shell {
    pub mod window {
        pub fn window_title(name: &str) -> String {
            format!("{name} — dsh-xlink")
        }
    }
    pub mod appearance {
        use tauri::Theme;
        pub fn initial_theme(_label: &str) -> Option<Theme> {
            None
        }
    }
}

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use url::Url;

use script_eval::read_json;

/// ChatGPT 登录页。**带 `?` 查询串是刻意的**：无查询串时 Cloudflare 会先给
/// 一个 managed challenge；带查询串走的是登录页本身的入口路径。
const LOGIN_URL: &str = "https://chatgpt.com/auth/login?redirect_on_login=true";

/// 探针自己的 WKWebView 数据存储标识符（**不是**生产的任何一个）。
///
/// 16 字节，必须是合法标识符。生产用的是 `dsh-chat-rel-001` / `dsh-chat-dev-001`。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-prb-001";

/// **页内侦测**：`window.open` 的每一次调用都被记下来。
///
/// 为什么必须自己记：**宿主桥（`on_new_window`）拿到请求之前**，中间还隔着
/// WebKit 的弹窗拦截（没有用户手势的 `window.open` 直接被拦）与 wry 的
/// 派发。页内记的是「页面**确实调用了** `window.open`」，与宿主收不收得到
/// 是两件事——这两者的差值就是「被静默吞掉」的那部分。
///
/// 同时记一次「点下去之后有没有同帧导航」：宿主看不到导航，但页内可以。
const PROBE_INIT_SCRIPT: &str = r#"
(function () {
  if (window.top !== window.self) return;
  try {
    window.__probeOpenCalls = [];
    window.__probeNet = [];
    window.__probeNav = [];
    window.__probeErrors = [];
    window.addEventListener('error', function (e) {
      try { window.__probeErrors.push(String(e.message || e.error || 'error')); } catch (_e) {}
    });
    window.addEventListener('unhandledrejection', function (e) {
      try { window.__probeErrors.push('unhandled: ' + String(e.reason && e.reason.message ? e.reason.message : e.reason)); } catch (_e) {}
    });

    var originalOpen = window.open;
    window.open = function (url, target, features) {
      try {
        window.__probeOpenCalls.push({
          url: String(url), target: String(target), features: String(features), at: Date.now()
        });
      } catch (e) {}
      return originalOpen.apply(this, arguments);
    };

    // 导航的三条路都钩住：同帧跳转不经过 `window.open`。
    var originalAssign = location.assign;
    var originalReplace = location.replace;
    try {
      location.assign = function (url) {
        try { window.__probeNav.push('assign:' + String(url)); } catch (e) {}
        return originalAssign.call(this, url);
      };
      location.replace = function (url) {
        try { window.__probeNav.push('replace:' + String(url)); } catch (e) {}
        return originalReplace.call(this, url);
      };
    } catch (e) {}

    // 网络请求：handler 常常先取一次 csrf / 会话票据再决定往哪跳。
    var originalFetch = window.fetch;
    if (originalFetch) {
      window.fetch = function (input, init) {
        var url = '';
        try { url = String(input && input.url ? input.url : input); } catch (e) {}
        var result = originalFetch.apply(this, arguments);
        try {
          result.then(function (r) {
            window.__probeNet.push({ url: url.slice(0, 200), status: r.status, ok: r.ok });
          }, function (err) {
            window.__probeNet.push({ url: url.slice(0, 200), error: String(err) });
          });
        } catch (e) {}
        return result;
      };
    }
    var originalXhrOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (method, url) {
      try { this.__probeUrl = String(url).slice(0, 200); } catch (e) {}
      return originalXhrOpen.apply(this, arguments);
    };
    var originalXhrSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function () {
      var xhr = this;
      try {
        xhr.addEventListener('loadend', function () {
          try { window.__probeNet.push({ url: xhr.__probeUrl || '', status: xhr.status }); } catch (e) {}
        });
      } catch (e) {}
      return originalXhrSend.apply(this, arguments);
    };
  } catch (e) {}
})();
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--wait-click <秒>`：不起合成点击，改成**等一次真实鼠标点击**。
    //
    // 为什么要这一档：浏览器的弹窗拦截只认**用户手势**（user activation），
    // 合成点击触发的 `window.open` 会被页面自己拦掉 ⇒ 合成点击量到的是
    // 「GSI 弹不了窗时的退化路径」（同帧重定向到 identifier 表单），**不是**
    // 真人点下去时发生的事。要量真实的弹窗形态，只能让人真点一次。
    let wait_click_secs: u64 = arg_value(&args, "--wait-click")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // `--data-store <16 个字符>`：用别的 WKWebsiteDataStore 标识符起这个 webview。
    //
    // **用来做 A/B**：同一个地址、同一个 webview，唯一的差别是「这份存储里有没有
    // Google session」⇒ Google 渲染的是**账号选择器**还是**邮箱表单**。
    // 配套做法是**只读拷贝**一份要对照的存储到**本探针进程名**的目录下（探针写
    // 的是拷贝，不是用户的真实档案）——`~/Library/WebKit/<进程名>/WebsiteDataStore/
    // <标识符的十六进制>/`，所以拷贝必须放到探针自己的目录里。
    let data_store: Option<[u8; 16]> = arg_value(&args, "--data-store").and_then(|v| {
        let bytes = v.as_bytes();
        if bytes.len() != 16 {
            return None;
        }
        let mut out = [0u8; 16];
        out.copy_from_slice(bytes);
        Some(out)
    });
    // `--url <地址>`：把登录页换成别的地址（对照 Google 那个地址时用）。
    let url_override = arg_value(&args, "--url");
    // `--open-popup <地址>`：**照生产接线**建一扇壳托管弹窗，量「弹窗有没有
    // 继承到发起方那份 cookie jar」。
    //
    // 为什么需要这一档：GSI 的账号选择器是**弹窗**形态，而弹窗的放行需要
    // 真人点击（合成点击被 WebKit 拦掉）⇒ 「点按钮」这一环在无人值守下测不了。
    // 但**「弹窗与页签是不是同一份 cookie jar」与「谁点的」无关** ⇒ 可以绕开
    // 点击、直接让页面 `window.open` 一个地址，再看宿主建出来的那扇弹窗里
    // 是不是有发起方那份 session 的痕迹（有没有出现账号选择器）。
    let open_popup = arg_value(&args, "--open-popup");

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            // `window.open` 捕获到的地址。生产那里会被交系统浏览器；
            // 探针只记下来，不弹浏览器。
            let popups: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let captured = popups.clone();
            std::thread::Builder::new()
                .name("official-chat-login-probe".into())
                .spawn(move || {
                    let report = run_probe(
                        &handle,
                        captured,
                        wait_click_secs,
                        data_store,
                        url_override,
                        open_popup,
                    );
                    println!("PROBE_REPORT_BEGIN");
                    println!("{report}");
                    println!("PROBE_REPORT_END");
                    handle.exit(0);
                })
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("probe: 无法构建探针应用");

    app.run(|_app, _event| {});
}

/// 取 `--key value` 或 `--key=value` 形式的参数值。
fn arg_value(args: &[String], key: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            return iter.next().cloned();
        }
        if let Some(rest) = arg.strip_prefix(&format!("{key}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

/// 起一个**按生产接线**配置的 webview，跑完四步观测。
fn run_probe(
    app: &tauri::AppHandle,
    popups: Arc<Mutex<Vec<String>>>,
    wait_click_secs: u64,
    data_store: Option<[u8; 16]>,
    url_override: Option<String>,
    open_popup: Option<String>,
) -> String {
    // `--url` 给了就是「被测页面」。**只有当被测页面本身就是 Google 页面时**
    // 才跳过点击——那时候点「Google」字样会自己跳走，把现场毁掉。
    // 其余情况（用 `--url` 指定另一个登录页，例如 `auth.openai.com/log-in`）
    // 仍然要点：那个模式量的正是「有 Google session 时点 GSI 按钮会怎样」。
    let ab_mode = url_override
        .as_deref()
        .map(|u| u.contains("accounts.google.com"))
        .unwrap_or(false);
    let login_url = url_override
        .clone()
        .unwrap_or_else(|| LOGIN_URL.to_string());
    let url = match Url::parse(&login_url) {
        Ok(url) => url,
        Err(e) => return json!({ "fatal": format!("登录地址非法：{e}") }).to_string(),
    };
    let captured = popups.clone();
    // **无条件**给 `data_directory`：Windows 上 WebView2 的 user-data 目录
    // 不给就落回**默认**目录（= 用户真实安装目录，浏览器一启动就写 Cookies）。
    // macOS 上这个参数被忽略（WKWebView 认的是 `data_store_identifier`），
    // 但照样给——判据 `probe-profile-isolation` 的第 ③ 条按**代码形状**判，
    // 「有才给」同样违规。
    let profile_dir = probe_profile_dir();
    // 托管弹窗建好之后放进这里，好让观测线程读它的页面现场。
    let popup_slot: Arc<Mutex<Option<tauri::WebviewWindow>>> = Arc::new(Mutex::new(None));
    let popup_slot_for_handler = popup_slot.clone();
    let popup_app = app.clone();
    let popup_profile_dir = profile_dir.clone();
    let popup_data_store = data_store;
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        "official-chat-login-probe",
        tauri::WebviewUrl::External(url),
    )
    .title("chatgpt login probe")
    .inner_size(1280.0, 900.0)
    .visible(true)
    .data_directory(profile_dir)
    // 与生产同一份浏览器参数（macOS 上被忽略，但保持接线一致）。
    .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
    // 与生产同一份指纹脚本：它会删掉 `window.__TAURI__` 并冻结
    // `navigator.webdriver`。如果登录流程对这两样敏感，探针必须复现。
    .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
    // 探针自己的页内侦测（生产没有这一段）。
    .initialization_script(PROBE_INIT_SCRIPT)
    // **照生产接线**处理 `window.open`：建一扇**壳托管**弹窗，并让它继承
    // 发起方那份浏览器档案（macOS = 同一个 WKWebsiteDataStore 标识符，
    // Windows = 同一个 user-data 目录 + 同一套浏览器参数）+ 同一份指纹脚本。
    //
    // 这段是本探针的**关键**：修复的兑现与否，全看这扇弹窗里有没有发起方
    // 那份 cookie jar 的痕迹（有没有出现账号选择器）。
    .on_new_window(move |link, _features| {
        if let Ok(mut list) = captured.lock() {
            list.push(link.to_string());
        }
        let popup = tauri::WebviewWindowBuilder::new(
            &popup_app,
            "official-chat-managed-popup",
            tauri::WebviewUrl::External(link.clone()),
        )
        .title("chatgpt login probe popup")
        .inner_size(900.0, 700.0)
        .visible(true)
        .data_directory(popup_profile_dir.clone())
        .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .initialization_script(include_str!("../src/harness/chat-fingerprint.js"));
        // ⚠️ **macOS 上这一句是「重新绑定」而不是「改写」** —— 原来写的是
        // `popup = popup.data_store_identifier(...)` ⇒ ⇒ 那是给一个 `let`
        // 绑定的不可变变量**二次赋值** ⇒ ⇒ macOS 上根本编不过
        // （E0384）。改成**遮蔽**：macOS 上第二个 `let popup` 盖住第一个，
        // 类型不变（还是 builder），后面 `popup.build()` 用的就是带
        // data_store 标识符那一份；别的平台上这段整块不存在。
        #[cfg(target_os = "macos")]
        let popup =
            popup.data_store_identifier(popup_data_store.unwrap_or(PROBE_DATA_STORE_IDENTIFIER));
        // 别的平台没有「WKWebsiteDataStore 标识符」这回事（Windows 靠
        // user-data 目录）⇒ 读一次消掉「未使用」告警。
        //
        // **这条不是洁癖**：CI 的 `cargo clippy --all-targets -- -D warnings`
        // 把 examples 也算进去，Windows / Linux 上一个未使用变量就是一条门禁
        // 红 ⇒ 而这个 example 是首次进仓的，从没在那些平台上编过。
        #[cfg(not(target_os = "macos"))]
        let _ = &popup_data_store;
        match popup.build() {
            Ok(window) => {
                if let Ok(mut slot) = popup_slot_for_handler.lock() {
                    *slot = Some(window.clone());
                }
                tauri::webview::NewWindowResponse::Create { window }
            }
            Err(error) => {
                eprintln!("dsh-xlink: 探针无法创建托管弹窗：{error}");
                tauri::webview::NewWindowResponse::Deny
            }
        }
    });
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(data_store.unwrap_or(PROBE_DATA_STORE_IDENTIFIER));

    let window = match builder.build() {
        Ok(window) => window,
        Err(e) => return json!({ "fatal": format!("无法创建探针窗口：{e}") }).to_string(),
    };

    tauri::async_runtime::block_on(async move {
        let mut steps: Vec<Value> = Vec::new();

        // --- 步骤 1：登录页现场 -------------------------------------------------
        // Cloudflare 的 managed challenge 需要几秒；给页面 20s 落地。
        wait_until_ready(&window, Duration::from_secs(20)).await;
        let login_page = read_scene(&window).await;
        steps.push(json!({
            "step": "1-login-page",
            "note": "打开 ChatGPT 登录页后的现场",
            "scene": login_page,
        }));

        // --- 步骤 2：Google 相关元素 -------------------------------------------
        let candidates = read_google_candidates(&window).await;
        steps.push(json!({
            "step": "2-google-candidates",
            "note": "页面上与 Google 有关的可点元素",
            "candidates": candidates,
        }));

        // --- 步骤 3：触发 Google 入口 ------------------------------------------
        // **合成点击 + 页内侦测**。为什么不直接用真实鼠标：合成点击缺的是
        // 「用户手势」（user activation），而**浏览器的弹窗拦截只认用户手势**
        // ⇒ 合成点击触发的 `window.open` 可能被页面自己拦掉，宿主也就收不到。
        // 但**处理器照样跑**（React 的根监听器会收到派发出去的 click），
        // 所以页内钩子照样记得到「页面确实调用了 `window.open`、参数是什么」。
        // 于是能分清两件事：① 流程形态（弹窗 / 同帧导航）；② 宿主收没收到。
        // 合成点击与真实点击的差值只在「弹窗是否被浏览器放行」这一层，
        // 而那一层是**宿主**的行为（`on_new_window` 收到没有），不靠这个差值下结论。
        let clicked = if ab_mode {
            // A/B 模式：页面就是被测对象，点任何按钮都会破坏现场。
            json!({
                "clicked": false,
                "why": "A/B 模式（--url 指定的页面即被测对象），不触发任何点击",
            })
        } else if wait_click_secs > 0 {
            // 等一次**真实鼠标点击**。窗口标题就是「chatgpt login probe」，
            // 页面上那个按钮是「使用 Google 账户继续」。
            println!("PROBE_READY_FOR_CLICK");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            json!({
                "mode": "wait-real-click",
                "seconds": wait_click_secs,
                "hint": "请在标题为「chatgpt login probe」的窗口里点一次「使用 Google 账户继续」",
            })
        } else {
            click_google(&window).await
        };
        let (href_history, open_calls, nav_calls, net_calls, errors) =
            watch_after_click(&window, &popups, wait_click_secs).await;
        steps.push(json!({
            "step": "3-click",
            "note": "触发 Google 入口后的观测",
            "clicked": clicked,
            "href_history": href_history,
            "page_open_calls": open_calls,
            "page_nav_calls": nav_calls,
            "page_net_calls": net_calls,
            "page_errors": errors,
            "host_popup_urls": popups.lock().map(|l| l.clone()).unwrap_or_default(),
        }));

        // --- 步骤 4：点击之后的现场 -------------------------------------------
        let after = read_scene(&window).await;
        steps.push(json!({
            "step": "4-after-click",
            "note": "点击之后的现场",
            "scene": after,
        }));

        // --- 步骤 5：把 Google 的地址在同一 webview 里直接打开 -------------------
        // 这一步回答「Google 在 WKWebView 里到底渲染成什么」：账号选择器（快速
        // 认证的前提）/ 邮箱输入表单 / 被拒的拦截页。
        let popup_urls = popups.lock().map(|l| l.clone()).unwrap_or_default();
        let mut google_candidates: Vec<String> = popup_urls
            .iter()
            .filter(|u| u.contains("accounts.google.com"))
            .cloned()
            .collect();
        // 页内记到的调用也算——宿主可能压根没收到。
        if let Some(calls) = open_calls.as_array() {
            for call in calls {
                if let Some(url) = call.get("url").and_then(Value::as_str) {
                    if url.contains("accounts.google.com")
                        && !google_candidates.contains(&url.to_string())
                    {
                        google_candidates.push(url.to_string());
                    }
                }
            }
        }
        let mut google_scene = Value::Null;
        if let Some(target) = google_candidates.first() {
            if let Ok(parsed) = Url::parse(target) {
                let _ = window.navigate(parsed);
                tokio::time::sleep(Duration::from_secs(10)).await;
                wait_until_ready(&window, Duration::from_secs(15)).await;
                google_scene = read_scene(&window).await;
            }
        }
        steps.push(json!({
            "step": "5-google-in-webview",
            "note": if ab_mode {
                "A/B 模式跳过：被测页面已经是 Google 页面本身"
            } else {
                "把 accounts.google.com 的地址放进同一个 webview 后看到的页面"
            },
            "target": google_candidates.first(),
            "scene": google_scene,
        }));

        // --- 步骤 6：托管弹窗有没有继承发起方那份 cookie jar -------------------
        // 这一步是「修复有没有兑现」的**决定性**读数：让页面自己
        // `window.open` 一个 Google 的账号选择器地址，宿主照生产接线建出
        // **壳托管**弹窗（同一个 data store 标识符），然后读那扇弹窗的现场。
        // 弹窗里出现「选择账号 + 账号名」⇒ 弹窗与页签同一份 cookie jar；
        // 弹窗里只有「邮箱或电话号码」⇒ 弹窗落到了默认数据存储（没继承）。
        let popup_step = match &open_popup {
            None => Value::Null,
            Some(target) => {
                let expression = format!(
                    "String((function () {{ try {{ window.open({:?}, '_blank'); return 'called'; }} catch (e) {{ return 'threw: ' + e; }} }})())",
                    target
                );
                let called = match read_json(&window, &expression, Duration::from_secs(6)).await {
                    Ok(Value::String(text)) => Value::String(text),
                    Ok(other) => other,
                    Err(error) => json!({ "error": format!("读回失败：{error}") }),
                };
                // 等弹窗建出来并落地
                let mut popup = None;
                for _ in 0..40 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    if let Ok(slot) = popup_slot.lock() {
                        if slot.is_some() {
                            popup = slot.clone();
                            break;
                        }
                    }
                }
                let mut scene = Value::Null;
                if let Some(p) = &popup {
                    tokio::time::sleep(Duration::from_secs(8)).await;
                    wait_until_ready(p, Duration::from_secs(15)).await;
                    scene = read_scene(p).await;
                }
                json!({
                    "step": "6-managed-popup",
                    "note": "宿主照生产接线建出的托管弹窗（同一个 data store 标识符）里的现场",
                    "open_called": called,
                    "popup_created": popup.is_some(),
                    "scene": scene,
                })
            }
        };
        if !popup_step.is_null() {
            steps.push(popup_step);
        }

        json!({
            "probe": "official-chat chatgpt google login",
            "mode": if ab_mode { "ab" } else { "login-page" },
            "login_url": LOGIN_URL,
            "url": login_url,
            "data_store": data_store
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .unwrap_or_else(|| "dsh-chat-prb-001".to_string()),
            "steps": steps,
            "production_note": "生产的 on_new_window 路由（harness_window::popup_route）：外部 http(s) → 系统浏览器 + Deny；about:blank / 本机页面 → 壳托管弹窗。探针只记地址不弹浏览器。",
        })
        .to_string()
    })
}

/// 触发 Google 入口后持续观测一段时间，记录三样东西：
///
/// - `href` 变化史（有没有同帧导航到 Google）；
/// - 页内记到的 `window.open` 调用（页面**确实调用了**没有）；
/// - 宿主收到的弹窗地址（`on_new_window` 拿到了没有）。
///
/// 这两者的差值就是「被静默吞掉」的部分。
async fn watch_after_click(
    window: &tauri::WebviewWindow,
    popups: &Arc<Mutex<Vec<String>>>,
    wait_secs: u64,
) -> (Vec<String>, Value, Value, Value, Value) {
    let started = Instant::now();
    let budget = if wait_secs > 0 {
        Duration::from_secs(wait_secs)
    } else {
        Duration::from_secs(12)
    };
    let mut href_history: Vec<String> = Vec::new();
    let mut last_href = String::new();
    let mut open_calls = Value::Array(Vec::new());
    let mut nav_calls = Value::Array(Vec::new());
    let mut net_calls = Value::Array(Vec::new());
    let mut errors = Value::Array(Vec::new());

    while started.elapsed() < budget {
        tokio::time::sleep(Duration::from_millis(600)).await;

        // 页面可能正在导航，读回会失败——失败本身就是「正在换页」的信号。
        if let Ok(Value::String(text)) =
            read_json(window, "String(location.href)", Duration::from_secs(2)).await
        {
            if text != last_href {
                last_href = text.clone();
                href_history.push(text);
            }
        }
        // 页内四路读数：窗口打开 / 同帧导航 / 网络 / 错误。
        // 「点击后什么都没发生」时，它们能分清是**处理器没跑**（四路皆空）
        // 还是**处理器跑了但卡在别处**（有网络/有错误/有导航尝试）。
        for (expression, slot) in [
            (
                "JSON.stringify(window.__probeOpenCalls || [])",
                &mut open_calls,
            ),
            ("JSON.stringify(window.__probeNav || [])", &mut nav_calls),
            ("JSON.stringify(window.__probeNet || [])", &mut net_calls),
            ("JSON.stringify(window.__probeErrors || [])", &mut errors),
        ] {
            if let Ok(Value::String(text)) =
                read_json(window, expression, Duration::from_secs(2)).await
            {
                if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                    if parsed.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                        *slot = parsed;
                    }
                }
            }
        }
        let _ = popups;
    }
    (href_history, open_calls, nav_calls, net_calls, errors)
}

/// 触发那个 Google 入口。返回点了什么、以及点完立刻读到的 href。
///
/// 合成点击：它触发的是**处理器**（要证明的就是处理器调了什么），
/// 不是浏览器那层「弹窗要不要放行」。
///
/// **点之前必须等水合完成**。ChatGPT 登录页是客户端渲染的：页面**文本**已经
/// 在了，但 React 的事件处理器还没挂上时，`.click()` 派发出去**没有人接**
/// ⇒ 读数是「点了没反应」。而 `gsi/log` **不能**当水合的证据——实测它在
/// 完全没有点击时也会打（GSI 初始化就发），拿它当「点了」的证据会得出
/// 假结论。判据用**页面自己的网络活动**：`window.__probeNet` 非空 ⇒
/// 页面脚本已经在跑（实测：网络为空的那一轮，点击后处理器确实没跑）。
async fn click_google(window: &tauri::WebviewWindow) -> Value {
    // 等 GSI 客户端就绪：**`window.google.accounts.id` 存在**才是「按钮可点」。
    //
    // 为什么不能只看「页面有网络活动」：实测有一轮页面脚本明明在跑（有自己的
    // 埋点请求），但 GSI 客户端还没初始化 ⇒ 按钮是**哑的** ⇒ 点击后什么
    // 都没发生。那一轮会被误读成「Google 这条路在壳里不通」。
    // `window.google.accounts.id` 是 GSI 客户端 `initialize()` 之后才挂上的，
    // 它在 ⇒ 按钮的处理器已经接好。
    let started = Instant::now();
    let mut ready = false;
    let mut saw_gsi_client = false;
    while started.elapsed() < Duration::from_secs(25) {
        if let Ok(Value::String(text)) = read_json(
            window,
            "String(!!(window.google && window.google.accounts && window.google.accounts.id))",
            Duration::from_secs(2),
        )
        .await
        {
            if text.trim() == "true" {
                ready = true;
                break;
            }
        }
        if let Ok(Value::String(text)) = read_json(
            window,
            "String(!!(window.google && window.google.accounts))",
            Duration::from_secs(2),
        )
        .await
        {
            saw_gsi_client = text.trim() == "true";
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !ready {
        return json!({
            "clicked": false,
            "why": if saw_gsi_client {
                "GSI 客户端出现了但 25s 内没完成 initialize（`window.google.accounts.id` 不存在）——按钮是哑的，这一轮不作数"
            } else {
                "25s 内 `window.google.accounts` 都没出现——GSI 客户端脚本没加载（网络不可达 / 被拦），这一轮不作数"
            },
            "saw_gsi_client": saw_gsi_client,
        });
    }
    let expression = r#"JSON.stringify((function () {
        var nodes = document.querySelectorAll('button,a,[role="button"],[data-testid],div,span');
        var best = null;
        for (var i = 0; i < nodes.length; i++) {
            var el = nodes[i];
            var text = (el.innerText || el.textContent || '').trim();
            if (!/google/i.test(text)) continue;
            if (text.length > 120) continue;
            if (el.offsetParent === null && getComputedStyle(el).position !== 'fixed') continue;
            if (!best || text.length < best.text.length) best = { el: el, text: text };
        }
        if (!best) return { clicked: false, why: '页面上没有可见的 Google 入口' };
        var before = String(location.href);
        best.el.click();
        return { clicked: true, text: best.text.slice(0, 80), href_before: before };
    })())"#;
    match read_json(window, expression, Duration::from_secs(6)).await {
        Ok(Value::String(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(e) => json!({ "error": format!("点击结果不是合法 JSON：{e}") }),
        },
        Ok(other) => other,
        Err(e) => json!({ "error": format!("点击失败：{e}") }),
    }
}

/// 探针自己的临时 user-data 目录（Windows 上 WebView2 用）。
///
/// 一次性目录，进程退出后由调用方清理；**绝不**指向用户真实数据目录。
fn probe_profile_dir() -> std::path::PathBuf {
    let base = std::env::temp_dir().join("dsh-official-chat-login-probe");
    let _ = std::fs::create_dir_all(&base);
    base
}

/// 页面现场。
///
/// **只读结构与文案，不取任何 cookie 值**——cookie 只记名字。
async fn read_scene(window: &tauri::WebviewWindow) -> Value {
    let expression = r#"JSON.stringify({
        href: String(location.href),
        title: String(document.title),
        ready: String(document.readyState),
        ua: String(navigator.userAgent),
        webdriver: String(navigator.webdriver),
        hasTauriGlobal: typeof window.__TAURI__,
        cookieNames: (function () {
            try {
                return document.cookie.split(';').map(function (s) { return s.split('=')[0].trim(); })
                    .filter(Boolean).slice(0, 40);
            } catch (e) { return []; }
        })(),
        bodyText: (document.body ? (document.body.innerText || "") : "").slice(0, 1200),
        bodyLen: document.body ? document.body.innerHTML.length : -1,
        challenge: /just a moment|enable javascript and cookies|checking your browser|challenge-platform/i
            .test((document.body ? (document.body.innerText || "") : ""))
    })"#;
    match read_json(window, expression, Duration::from_secs(6)).await {
        Ok(Value::String(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(e) => json!({ "error": format!("现场不是合法 JSON：{e}") }),
        },
        Ok(other) => other,
        Err(e) => json!({ "error": format!("读回失败：{e}") }),
    }
}

/// 页面上与 Google 有关的可点元素（只取文案与 href，不点）。
async fn read_google_candidates(window: &tauri::WebviewWindow) -> Value {
    let expression = r#"JSON.stringify((function () {
        var out = [];
        var nodes = document.querySelectorAll('button,a,[role="button"],[data-testid],div,span');
        for (var i = 0; i < nodes.length && out.length < 25; i++) {
            var el = nodes[i];
            var text = (el.innerText || el.textContent || '').trim();
            if (!/google/i.test(text)) continue;
            if (text.length > 120) continue;
            if (el.offsetParent === null && getComputedStyle(el).position !== 'fixed') continue;
            out.push({
                tag: el.tagName,
                text: text.slice(0, 80),
                href: el.getAttribute ? (el.getAttribute('href') || '') : '',
                testid: el.getAttribute ? (el.getAttribute('data-testid') || '') : '',
                role: el.getAttribute ? (el.getAttribute('role') || '') : ''
            });
        }
        // 内层元素优先（文案更短的那一层通常是真正的按钮）
        out.sort(function (a, b) { return a.text.length - b.text.length; });
        return out;
    })())"#;
    match read_json(window, expression, Duration::from_secs(6)).await {
        Ok(Value::String(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(e) => json!({ "error": format!("候选不是合法 JSON：{e}") }),
        },
        Ok(other) => other,
        Err(e) => json!({ "error": format!("读回失败：{e}") }),
    }
}

/// 等页面 readyState 变成 complete（Cloudflare 挑战要几秒）。
async fn wait_until_ready(window: &tauri::WebviewWindow, budget: Duration) {
    let started = Instant::now();
    loop {
        if started.elapsed() >= budget {
            return;
        }
        if let Ok(Value::String(text)) = read_json(
            window,
            "String(document.readyState)",
            Duration::from_secs(2),
        )
        .await
        {
            if text.trim() == "complete" {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
