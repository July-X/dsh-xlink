//! P4 前置实测：**WebView2 DevTools 协议 Network 域支持度**（只读探针）。
//!
//! # 它要回答什么
//!
//! P4 协议要写死「执行体在壳、API 调用在页内」。页内的数据怎么回到壳，
//! 首选路线是「壳内 DevTools Network 域收响应」——但**支持度未验**。
//! 本探针只回答三件事：
//!
//! 1. `ICoreWebView2::CallDevToolsProtocolMethod` 发 **`Network.enable`**
//!    能否成功？返回什么？
//! 2. `ICoreWebView2::GetDevToolsProtocolEventReceiver` 订阅后，能否收到
//!    **`Network.responseReceived`**？
//! 3. 命中站点自己的请求后，**`Network.getResponseBody`** 能否取到响应体？
//!
//! # 接口形态（本机 SDK + 官方文档双向核对）
//!
//! - `GetDevToolsProtocolEventReceiver(LPCWSTR eventName, receiver)` ——
//!   **带事件名参数**（官方文档 `icorewebview2.md` 原文：
//!   “Get a DevTools Protocol event receiver that allows you to subscribe to a
//!   DevTools Protocol event.”；本机 `webview2-com-sys 0.39.1`
//!   `src/bindings.rs:1518` 同形）。所以订阅是**按事件名**做的，不是「收全部」。
//! - 事件载荷是**参数对象本身**（官方文档：args 里是 “the parameter object of
//!   the DevTools Protocol event as a JSON string”）⇒ 载荷里**没有** `method`
//!   字段，`requestId` / `response` 就在顶层。
//! - 事件名不从载荷里读（按名订阅，事件名是已知量）；本探针仍记录每条事件的
//!   顶层键名，作为「载荷形态」的读数。
//!
//! # 纪律
//!
//! - **不新建会话、不发消息、不点任何按钮** ⇒ 站点侧零写入。
//! - **不落盘任何响应体正文**：只报**结构**（顶层键名 / `data` 键名 /
//!   `data.biz_data` 键名 / 数组长度 / 数组首元素键名）＋ 字节长度 ＋ SHA-256。
//! - **profile 目录从命令行显式给**（`--profile`）：本探针**不**在源码里写
//!   官网对话那份 user-data 目录的名字 —— 那是仓库硬不变量
//!   `scripts/lib/probe-profile-isolation.mjs` 盯着的东西（2026-10-20 13:48
//!   那次写坏用户 Cookies 的事故后立的：探针里**不许**出现产品 profile 目录
//!   名，因为浏览器一启动就往那里写 Cookies/Preferences/Local Storage/History）。
//!   生产同一条推导由**调用方**按 `xlink_home()/<family>/<mode>/<官网对话
//!   profile 目录>` 做，且根必须指到**一次性临时目录** ⇒ 绝不碰用户真实数据目录。
//! - **URL 里的 uuid / 长 token 串打码**。
//! - **不开 DevTools 窗口**：全程只走协议调用（与生产 `cdp_eval.rs` 同一形态）。
//!
//! # 为什么先订阅再导航
//!
//! 页面初始化批次（`phase=init`）里的请求发生在**导航后几百毫秒内**。先建窗
//! （`about:blank`）→ 订阅 → `Network.enable` → 再导航，才能保证「不依赖
//! init 时机」这条性质被**真的**测到（而不是碰巧赶上）。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_devtools_network_probe -- \
//!   --profile "<一次性临时目录下的 user-data 目录>" --out "<读数文件路径>"
//! ```
//!
//! 输出是 `PROBE_REPORT_BEGIN` / `PROBE_REPORT_END` 之间的一段 JSON，
//! 同时写进 `--out` 给出的文件。

// 探针只用到 official_chat 的一小部分（浏览器参数常量），`#[path]` 把整份
// 源文件编进来会带出一批未使用的常量与函数——这不影响探针行为。
#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

// 生产代码里 `src/official_chat/script_eval.rs` 是 `pub(crate)`，example 是
// 另一个 crate，拿不到——用 `#[path]` 把**同一份源文件**编进来。
#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

// 生产代码里 `src/official_chat/cdp_eval.rs` 同理。
#[allow(dead_code)]
#[path = "../src/official_chat/cdp_eval.rs"]
mod cdp_eval;

// 生产代码里 `crate::shell::window::window_title` / `crate::shell::appearance::
// initial_theme` 是**壳自己的**模块；example 是另一个 crate，没有它们。
// 这里给一份**同名同形状**的最小实现，让 `#[path]` 进来的那份生产源码
// 在探针里也能编译——测的仍然是生产那份代码，不是副本。
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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::cdp_eval::cdp_eval;

/// 探测的站点首页。
const SITE_URL: &str = "https://chat.deepseek.com";

/// 协议 §4.3 关心的那个端点（**只读 GET**，不新建会话、不发消息）。
const FETCH_PAGE_PATH: &str = "/api/v0/chat_session/fetch_page?count=5";

/// 订阅的事件（**按事件名**订阅，官方 API 形态）。
const SUBSCRIBE_EVENTS: &[&str] = &[
    "Network.responseReceived",
    "Network.loadingFinished",
    "Network.requestWillBeSent",
];

/// 第一阶段的观察窗口上限。
const OBSERVE_WINDOW: Duration = Duration::from_secs(40);

/// 看到第一个 `/api/` 响应之后再多观察这么久（抓站点初始化批次的后续请求）。
const AFTER_FIRST_API_GRACE: Duration = Duration::from_secs(12);

/// 补测阶段的观察窗口上限。
const FALLBACK_WINDOW: Duration = Duration::from_secs(20);

/// 探针从站里收到的两类消息。
enum Msg {
    /// 一条 DevTools 协议事件（事件名 + 载荷原文 JSON）。
    Event { name: String, raw: String },
    /// 订阅/接线阶段的错误。
    Setup(String),
}

/// 事件累积状态。
#[derive(Default)]
struct EventState {
    total: u64,
    unparsable: u64,
    by_event: BTreeMap<String, u64>,
    api_hits: Vec<Value>,
    sent: Vec<Value>,
    finished: BTreeSet<String>,
    setup_errors: Vec<String>,
    first_event_shape: Option<Value>,
    first_api_at: Option<Instant>,
    /// 站点自己发的**第一条** `/api/` 响应（用于「早取 vs 晚取」对照）。
    first_hit: Option<Value>,
    first_hit_seen_at: Option<Instant>,
    /// 该响应一加载完就**立刻**取到的体。
    immediate_body: Option<Value>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--profile` / `--out` **必须**显式给：不给就没法保证「绝不碰用户真实
    // 数据目录」，也没法把读数交给验收方。
    let Some(profile) = arg_value(&args, "--profile").map(PathBuf::from) else {
        eprintln!(
            "probe: 必须显式给 --profile <一次性临时目录下的 user-data 目录>；\
             不给就不探测（浏览器会落回默认数据目录 = 用户真实安装目录）"
        );
        std::process::exit(2);
    };
    let Some(out) = arg_value(&args, "--out").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --out <读数文件路径>");
        std::process::exit(2);
    };
    let url_text = arg_value(&args, "--url").unwrap_or_else(|| SITE_URL.to_string());
    // 「晚取」要等多久（毫秒）——用来分辨「取体有时间/缓冲窗口」与「能力不支持」。
    let late_wait_ms: u64 = arg_value(&args, "--late-wait-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(12_000);

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile = profile.clone();
            let out = out.clone();
            let url_text = url_text.clone();
            std::thread::Builder::new()
                .name("official-chat-devtools-network-probe".into())
                .spawn(move || {
                    let report = tauri::async_runtime::block_on(run_probe(
                        &handle,
                        &profile,
                        &url_text,
                        late_wait_ms,
                    ));
                    println!("PROBE_REPORT_BEGIN");
                    println!("{report}");
                    println!("PROBE_REPORT_END");
                    if let Some(parent) = out.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&out, format!("{report}\n"));
                    handle.exit(0);
                })
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("probe: 无法构建探针应用");

    app.run(|_app, _event| {});
}

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

/// 跑一次探针，出 JSON 读数。
async fn run_probe(
    handle: &tauri::AppHandle,
    profile: &Path,
    url_text: &str,
    late_wait_ms: u64,
) -> String {
    let mut report = json!({
        "probe": "official-chat · WebView2 DevTools Network 域支持度只读探针（P4 前置）",
        "site_url": url_text,
        "questions": {
            "q1": "Network.enable 能否成功？返回什么？",
            "q2": "GetDevToolsProtocolEventReceiver 订阅后能否收到 Network.responseReceived？",
            "q3": "命中站点请求后 Network.getResponseBody 能否取到响应体？"
        },
        "discipline": [
            "不新建会话、不发消息、不点任何按钮 ⇒ 站点侧零写入",
            "不落盘任何响应体正文：只报结构 + 字节长度 + SHA-256",
            "profile 由命令行显式给，且根是一次性临时目录 ⇒ 绝不碰用户真实数据目录",
            "URL 里 uuid / 长 token 串打码",
            "全程不开 DevTools 窗口（只走协议调用）"
        ]
    });

    // --- profile 目录：调用方显式给（源码里不写产品 profile 目录名） ---
    let _ = std::fs::create_dir_all(&profile);
    report["profile"] = json!({
        "given_by": "--profile（命令行显式给；源码不写产品 profile 目录名 —— 仓库硬不变量 scripts/lib/probe-profile-isolation.mjs）",
        "profile_dir": profile.to_string_lossy(),
        "production_derivation": [
            "src/shell/paths.rs:132 xlink_home()（DSH_XLINK_HOME 优先）",
            "src/shell/paths.rs:156 family_runtime_dir(family, mode)",
            "src/shell/paths.rs:100 runtime_subdir() → desktop-dev（debug 壳）",
            "src/commands.rs:1874 state.data_dir.join(<官网对话那份 user-data 目录>)",
            "src/official_chat/context.rs:13-19 必须复用同一份 user-data 目录"
        ],
        "note": "本次实测的根被指到一次性临时目录 ⇒ 绝不碰用户真实数据目录"
    });

    let window = match build_window(handle, &profile) {
        Ok(window) => window,
        Err(e) => {
            report["fatal"] = json!(format!("无法创建探测窗口：{e}"));
            return report.to_string();
        }
    };

    // --- 订阅（必须在导航之前） ---
    let events = subscribe_devtools_events(&window);

    // --- ① Network.enable ---
    let enable = call_cdp(&window, "Network.enable", "{}").await;
    report["q1_network_enable"] = match &enable {
        Ok(raw) => json!({ "ok": true, "raw_result": raw }),
        Err(e) => json!({ "ok": false, "error": e }),
    };

    // --- 对照组：不存在的 CDP 方法必须报错（否则上面的 ok 不能当证据） ---
    let control = call_cdp(&window, "Network.thisMethodDoesNotExist", "{}").await;
    report["control_bogus_cdp_method"] = match &control {
        Ok(raw) => json!({
            "ok": true,
            "raw_result": raw,
            "note": "对照组异常：不存在的 CDP 方法居然成功 ⇒ q1 的 ok 不能当证据"
        }),
        Err(e) => json!({
            "ok": false,
            "error": e,
            "note": "对照组正常：不存在的 CDP 方法确实报错 ⇒ q1 的 ok 是真成功"
        }),
    };

    // --- 导航到站点 ---
    let url: tauri::Url = match url_text.parse() {
        Ok(url) => url,
        Err(e) => {
            report["fatal"] = json!(format!("URL 非法：{e}"));
            return report.to_string();
        }
    };
    report["navigate"] = match window.navigate(url) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };

    // --- 第一阶段：观察站点自己发的请求（并对第一条 /api/ 响应「立刻」取体） ---
    let mut state = EventState::default();
    observe(
        &window,
        &events,
        &mut state,
        Instant::now() + OBSERVE_WINDOW,
        |st| match st.first_api_at {
            Some(at) => at.elapsed() >= AFTER_FIRST_API_GRACE,
            None => false,
        },
    )
    .await;

    report["q2_events"] = json!({
        "subscribe_setup_errors": state.setup_errors.clone(),
        "subscribed_events": SUBSCRIBE_EVENTS,
        "event_total": state.total,
        "unparsable": state.unparsable,
        "by_event": state.by_event.clone(),
        "first_event_shape": state.first_event_shape.clone(),
        "api_response_received_count": state.api_hits.len(),
        "api_requests": state.api_hits.clone(),
        "request_will_be_sent_sample": state.sent.clone(),
        "loading_finished_count": state.finished.len(),
        "note": "按事件名订阅 ⇒ 事件名是已知量；first_event_shape 记录载荷顶层键名（载荷即参数对象，无 method 字段）"
    });

    // --- ③ 取响应体 ---
    //
    // 这里刻意做**两次**取体，分辨「取不到体」到底是「这条请求的体取不到」
    // 还是「取晚了」：
    //
    // - **早取**：观察窗口内，站点第一条 `/api/` 响应一收到
    //   `Network.loadingFinished` 就立刻取（见 `observe`）。
    // - **晚取**：观察窗口结束后，对**同一条**请求再取一次。
    //
    // 两次的结果对照起来，才能把「时序/缓冲」与「能力不支持」分开。
    let fetch_page_hit = state
        .api_hits
        .iter()
        .find(|hit| url_of(hit).contains("chat_session/fetch_page"))
        .cloned();

    let mut bodies: Vec<Value> = Vec::new();
    if let Some(entry) = state.immediate_body.clone() {
        bodies.push(entry);
    }
    let late_same = match &state.first_hit {
        Some(hit) => {
            // 故意再等一段（默认缓冲下的「晚取」）。
            tokio::time::sleep(Duration::from_millis(late_wait_ms)).await;
            let waited = state
                .first_hit_seen_at
                .map(|at| at.elapsed().as_millis() as u64)
                .unwrap_or(0);
            match fetch_body(&window, "late_same_first_site_api", hit).await {
                Some(mut entry) => {
                    if let Some(map) = entry.as_object_mut() {
                        map.insert(
                            "waited_ms_since_response".to_string(),
                            Value::from(waited),
                        );
                    }
                    Some(entry)
                }
                None => None,
            }
        }
        None => None,
    };
    if let Some(entry) = late_same {
        bodies.push(entry);
    }
    if let Some(hit) = fetch_page_hit.clone() {
        if let Some(entry) = fetch_body(&window, "site_fetch_page", &hit).await {
            bodies.push(entry);
        }
    }
    report["q3_get_response_body"] = json!({
        "attempts": bodies.clone(),
        "late_wait_ms": late_wait_ms,
        "note": "响应体正文不落盘：只报结构 + 字节长度 + SHA-256；early/late 是同一条请求的两次取体，用来分辨「时序/缓冲」与「能力不支持」"
    });

    // --- 补测：站点自然批次里没有 fetch_page 时，用一次**只读 GET** 验证通路 ---
    let mut fallback: Option<Value> = None;
    if fetch_page_hit.is_none() {
        let expression = build_readonly_get_expression(FETCH_PAGE_PATH);
        let evaluated = cdp_eval(&window, &expression, Duration::from_secs(20)).await;
        let page_side = match &evaluated {
            Ok(envelope) => {
                let (value, exception) = crate::cdp_eval::unwrap_cdp_result(envelope);
                json!({ "page_result": value, "exception": exception })
            }
            Err(e) => json!({ "error": e.to_string() }),
        };

        // 等这条 GET 的 responseReceived 事件。
        let mut state2 = EventState::default();
        observe(
            &window,
            &events,
            &mut state2,
            Instant::now() + FALLBACK_WINDOW,
            |st| {
                st.api_hits
                    .iter()
                    .any(|hit| url_of(hit).contains("chat_session/fetch_page"))
            },
        )
        .await;
        let hit = state2
            .api_hits
            .iter()
            .find(|hit| url_of(hit).contains("chat_session/fetch_page"))
            .cloned();
        let body = match hit {
            Some(hit) => {
                fetch_body_after_finished(&window, &events, &mut state2, "fallback_readonly_get", &hit)
                    .await
            }
            None => None,
        };
        fallback = Some(json!({
            "why": "未登录态下站点自己不会发 fetch_page ⇒ 用一次同源只读 GET（不新建会话、不发消息）验证「Network.getResponseBody 能取到 body」这条通路",
            "method": "GET（同源、credentials=same-origin、只读）",
            "path": FETCH_PAGE_PATH,
            "page_side": page_side,
            "events_seen": state2.total,
            "body": body
        }));
    }
    report["fallback_readonly_get"] = fallback.unwrap_or(Value::Null);

    // --- Phase C：把「晚取失败」是不是缓冲容量问题，分辨出来 ---
    //
    // 做法：把 `Network.enable` 的缓冲调大（`maxTotalBufferSize` /
    // `maxResourceBufferSize`），重载页面，再等 20s 才取体。
    // 大缓冲下晚取成功 ⇒ 晚取失败是**缓冲/时序**问题，不是能力不支持。
    let big_buffer = json!({
        "maxTotalBufferSize": 104857600,
        "maxResourceBufferSize": 52428800
    })
    .to_string();
    let enable_big = call_cdp(&window, "Network.enable", &big_buffer).await;
    let reload = window.reload();
    let mut state3 = EventState::default();
    observe(
        &window,
        &events,
        &mut state3,
        Instant::now() + Duration::from_secs(40),
        |st| match (&st.first_hit, &st.first_hit_seen_at) {
            (Some(hit), Some(_)) => {
                let rid = hit.get("request_id").and_then(Value::as_str).unwrap_or("");
                st.finished.contains(rid)
            }
            _ => false,
        },
    )
    .await;
    // 故意等 20s 再取体（模拟「不是立刻取」）。
    tokio::time::sleep(Duration::from_secs(20)).await;
    let big_late = match &state3.first_hit {
        Some(hit) => fetch_body(&window, "big_buffer_late_20s", hit).await,
        None => None,
    };
    report["phase_c_big_buffer"] = json!({
        "why": "分辨「晚取失败」是缓冲容量问题还是能力不支持：大缓冲 + 重载 + 故意晚取 20s",
        "network_enable_with_buffer": match &enable_big {
            Ok(raw) => json!({ "ok": true, "raw_result": raw }),
            Err(e) => json!({ "ok": false, "error": e }),
        },
        "reload": match &reload {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "error": e.to_string() }),
        },
        "events_seen": state3.total,
        "first_api_url": state3
            .first_hit
            .as_ref()
            .map(|hit| url_of(hit))
            .unwrap_or_default(),
        "body_late_20s": big_late
    });

    report.to_string()
}

/// 建探针窗口：先开在 `about:blank`，等订阅与 `Network.enable` 都完成后
/// 才导航（见模块注释「为什么先订阅再导航」）。
fn build_window(
    handle: &tauri::AppHandle,
    profile: &Path,
) -> Result<tauri::WebviewWindow, String> {
    let url: tauri::Url = "about:blank"
        .parse()
        .map_err(|e| format!("URL 非法：{e}"))?;
    tauri::WebviewWindowBuilder::new(
        handle,
        "official-chat-devtools-network-probe",
        tauri::WebviewUrl::External(url),
    )
    .title("dsh DevTools Network 域探针")
    .inner_size(1000.0, 760.0)
    .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
    .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
    .data_directory(profile.to_path_buf())
    .build()
    .map_err(|e| format!("无法创建探针窗口：{e}"))
}

/// 按事件名订阅 DevTools 协议事件。
///
/// 官方 API 形态：`GetDevToolsProtocolEventReceiver(LPCWSTR eventName, receiver)`
/// —— 接收器是**按事件名**取的，拿到后用
/// `add_DevToolsProtocolEventReceived` 注册处理函数。
fn subscribe_devtools_events(window: &tauri::WebviewWindow) -> Receiver<Msg> {
    use webview2_com::DevToolsProtocolEventReceivedEventHandler;
    use windows_core::HSTRING;

    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    // 外层闭包是 `move` 的，会把 sender 带走 ⇒ 闭包里用拷贝，外层留一份。
    let tx_outer = tx.clone();
    let dispatched = window.with_webview(move |platform| {
        let controller = platform.controller();
        // SAFETY: `CoreWebView2` 只是取回控制器持有的核心对象引用，不转移
        // 所有权；返回值由 COM 引用计数管理。
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                let _ = tx_outer.send(Msg::Setup(format!("取不到 WebView2 核心：{e}")));
                return;
            }
        };
        for event_name in SUBSCRIBE_EVENTS {
            // 每个订阅一份 sender：事件处理函数是 `'static` 的闭包，会把
            // sender 带走，所以循环里必须各拿一份拷贝。
            let tx_event = tx_outer.clone();
            let name_hstring = HSTRING::from(*event_name);
            // SAFETY: 事件名是静态字符串，调用期间由 `name_hstring` 持有。
            let receiver = match unsafe { core.GetDevToolsProtocolEventReceiver(&name_hstring) } {
                Ok(receiver) => receiver,
                Err(e) => {
                    let _ = tx_outer.send(Msg::Setup(format!(
                        "取不到事件 {event_name} 的接收器：{e}"
                    )));
                    continue;
                }
            };
            let label = event_name.to_string();
            let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(
                move |_sender, args| {
                    if let Some(args) = args {
                        let mut pwstr = windows_core::PWSTR::null();
                        // SAFETY: `ParameterObjectAsJson` 把一段以
                        // CoTaskMemAlloc 分配的 PWSTR 写进 `pwstr`；下面的
                        // `CoTaskMemPWSTR` 负责释放它。
                        let ok = unsafe {
                            args.ParameterObjectAsJson(&mut pwstr as *mut windows_core::PWSTR)
                        }
                        .is_ok();
                        if ok {
                            let text = webview2_com::CoTaskMemPWSTR::from(pwstr).to_string();
                            let _ = tx_event.send(Msg::Event {
                                name: label.clone(),
                                raw: text,
                            });
                        }
                    }
                    Ok(())
                },
            ));
            let mut token: i64 = 0;
            // SAFETY: 只注册处理函数，不改页面状态。
            if let Err(e) = unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) } {
                let _ = tx_outer.send(Msg::Setup(format!("订阅 {event_name} 失败：{e}")));
            }
        }
    });
    if let Err(e) = dispatched {
        let _ = tx.send(Msg::Setup(format!("with_webview 失败：{e}")));
    }
    rx
}

/// 调一次 DevTools 协议方法，返回**原始结果文本**。
///
/// 与生产 `cdp_eval.rs` 同一形态（同一个 `ICoreWebView2CallDevToolsProtocolMethodCompletedHandler`），
/// 只是这里把方法名与参数放开，因为探针要调的不止 `Runtime.evaluate`。
async fn call_cdp(
    window: &tauri::WebviewWindow,
    method: &str,
    params: &str,
) -> Result<String, String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::HSTRING;

    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let tx_fallback = tx.clone();
    let method_h = HSTRING::from(method);
    let params_h = HSTRING::from(params);

    let dispatched = window.with_webview(move |platform| {
        let controller = platform.controller();
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                let _ = tx.try_send(Err(format!("取不到 WebView2 核心：{e}")));
                return;
            }
        };
        let handler =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, result| {
                let payload: Result<String, String> = match hr {
                    Ok(()) => Ok(result),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.try_send(payload);
                Ok(())
            }));
        if let Err(e) = unsafe { core.CallDevToolsProtocolMethod(&method_h, &params_h, &handler) } {
            let _ = tx_fallback.try_send(Err(e.to_string()));
        }
    });
    if let Err(e) = dispatched {
        return Err(format!("webview 已不可用：{e}"));
    }

    match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
        Ok(Some(Ok(text))) => Ok(text),
        Ok(Some(Err(detail))) => Err(detail),
        Ok(None) => Err("结果通道已关闭（webview 被销毁）".to_string()),
        Err(_) => Err(format!("CDP 调用 {method} 在 20s 内没有回应")),
    }
}

/// 抽干事件直到截止时间或 `stop` 成立。
///
/// 顺带做一件关键事：站点自己发的**第一条** `/api/` 响应一收到
/// `Network.loadingFinished`，就**立刻**取它的体（不等到观察窗口结束）——
/// 这是「早取」那一支的读数来源。
async fn observe<F>(
    window: &tauri::WebviewWindow,
    events: &Receiver<Msg>,
    state: &mut EventState,
    deadline: Instant,
    mut stop: F,
) where
    F: FnMut(&EventState) -> bool,
{
    loop {
        if Instant::now() >= deadline || stop(state) {
            return;
        }
        match events.recv_timeout(Duration::from_millis(300)) {
            Ok(Msg::Setup(detail)) => state.setup_errors.push(detail),
            Ok(Msg::Event { name, raw }) => {
                state.total += 1;
                *state.by_event.entry(name.clone()).or_insert(0) += 1;
                let parsed: Option<Value> = serde_json::from_str(&raw).ok();
                let Some(value) = parsed else {
                    state.unparsable += 1;
                    continue;
                };
                if state.first_event_shape.is_none() {
                    state.first_event_shape = Some(json!({
                        "event": name.clone(),
                        "top_keys": keys_of(&value),
                        "sample_prefix": mask_url(&raw.chars().take(240).collect::<String>())
                    }));
                }
                match name.as_str() {
                    "Network.responseReceived" => {
                        if let Some(hit) = describe_response_received(&value) {
                            if url_of(&hit).contains("/api/") {
                                if state.first_hit.is_none() {
                                    state.first_hit = Some(hit.clone());
                                    state.first_hit_seen_at = Some(Instant::now());
                                }
                                if state.api_hits.len() < 60 {
                                    state.api_hits.push(hit);
                                }
                                if state.first_api_at.is_none() {
                                    state.first_api_at = Some(Instant::now());
                                }
                            }
                        }
                    }
                    "Network.loadingFinished" => {
                        if let Some(request_id) = value.get("requestId").and_then(Value::as_str) {
                            state.finished.insert(request_id.to_string());
                            // 站点第一条 /api/ 响应一加载完 → **立刻**取体。
                            if state.immediate_body.is_none() {
                                if let Some(hit) = state.first_hit.clone() {
                                    let target = hit
                                        .get("request_id")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string();
                                    if target == request_id {
                                        let waited = state
                                            .first_hit_seen_at
                                            .map(|at| at.elapsed().as_millis() as u64)
                                            .unwrap_or(0);
                                        if let Some(mut entry) =
                                            fetch_body(window, "early_first_site_api", &hit).await
                                        {
                                            if let Some(map) = entry.as_object_mut() {
                                                map.insert(
                                                    "waited_ms_since_response".to_string(),
                                                    Value::from(waited),
                                                );
                                            }
                                            state.immediate_body = Some(entry);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "Network.requestWillBeSent" => {
                        if state.sent.len() < 20 {
                            if let Some(request) = value.get("request") {
                                let url = request
                                    .get("url")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();
                                if url.contains("/api/") {
                                    state.sent.push(json!({
                                        "url": mask_url(url),
                                        "method": request.get("method").cloned().unwrap_or(Value::Null),
                                        "resource_type": value.get("type").cloned().unwrap_or(Value::Null),
                                        "request_id": value.get("requestId").cloned().unwrap_or(Value::Null)
                                    }));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// 从 `Network.responseReceived` 事件里抽出**结构化**信息（URL 已打码）。
///
/// 载荷是**参数对象本身**（官方 API：args 里给的是事件的 parameter object），
/// 所以 `requestId` / `response` 就在顶层；若将来载荷带 `params` 包装也兼容。
fn describe_response_received(value: &Value) -> Option<Value> {
    let params = match value.get("params") {
        Some(inner) if inner.is_object() => inner,
        _ => value,
    };
    let response = params.get("response")?;
    let url = response.get("url").and_then(Value::as_str)?;
    Some(json!({
        "url": mask_url(url),
        "status": response.get("status").cloned().unwrap_or(Value::Null),
        "mime_type": response.get("mimeType").cloned().unwrap_or(Value::Null),
        "resource_type": params.get("type").cloned().unwrap_or(Value::Null),
        "request_id": params.get("requestId").cloned().unwrap_or(Value::Null)
    }))
}

/// 对一条命中请求调 `Network.getResponseBody`，只回**结构**。
///
/// **不**等 `Network.loadingFinished`——取体的时机由调用方决定，而这正是
/// 「早取 vs 晚取」要测的那个变量。
async fn fetch_body(
    window: &tauri::WebviewWindow,
    label: &str,
    hit: &Value,
) -> Option<Value> {
    let request_id = hit.get("request_id").and_then(Value::as_str)?.to_string();
    let params = json!({ "requestId": request_id }).to_string();
    let entry = match call_cdp(window, "Network.getResponseBody", &params).await {
        Ok(raw) => {
            let parsed: Option<Value> = serde_json::from_str(&raw).ok();
            match parsed {
                Some(value) => {
                    let body = value.get("body").and_then(Value::as_str).unwrap_or("");
                    json!({
                        "label": label,
                        "request": hit.clone(),
                        "ok": true,
                        "base64_encoded": value
                            .get("base64Encoded")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        "body_bytes": body.len(),
                        "body_sha256": sha256_hex(body.as_bytes()),
                        "structure": structure_of(body),
                        "note": "响应体正文不落盘：只报结构 + 字节长度 + SHA-256"
                    })
                }
                None => json!({
                    "label": label,
                    "request": hit.clone(),
                    "ok": true,
                    "cdp_envelope_unparsable": true,
                    "envelope_bytes": raw.len()
                }),
            }
        }
        Err(e) => json!({
            "label": label,
            "request": hit.clone(),
            "ok": false,
            "error": e
        }),
    };
    Some(entry)
}

/// 先等这条请求的 `Network.loadingFinished` 到达，再取体。
///
/// 这正是 P4 实现该做的次序（**实测**：不等就取 ⇒ `E_INVALIDARG`）。
async fn fetch_body_after_finished(
    window: &tauri::WebviewWindow,
    events: &Receiver<Msg>,
    state: &mut EventState,
    label: &str,
    hit: &Value,
) -> Option<Value> {
    let request_id = hit.get("request_id").and_then(Value::as_str)?.to_string();
    if !state.finished.contains(&request_id) {
        observe(
            window,
            events,
            state,
            Instant::now() + Duration::from_secs(15),
            |st| st.finished.contains(&request_id),
        )
        .await;
    }
    let mut entry = fetch_body(window, label, hit).await?;
    if let Some(map) = entry.as_object_mut() {
        map.insert(
            "loading_finished_seen".to_string(),
            Value::from(state.finished.contains(&request_id)),
        );
    }
    Some(entry)
}

/// 把响应体收成**结构**（协议 §4.3 同一口径）：顶层键名 / `data` 键名 /
/// `data.biz_data` 键名 / 数组长度 / 数组首元素键名。
fn structure_of(body: &str) -> Value {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return json!({ "json": false, "body_bytes": body.len() });
    };
    let mut out = json!({ "json": true, "top_keys": keys_of(&value) });
    if let Some(code) = value.get("code") {
        if code.is_number() {
            out["code"] = code.clone();
        }
    }
    let Some(data) = value.get("data") else {
        return out;
    };
    if !data.is_object() {
        out["data_type"] = kind_of(data).into();
        return out;
    }
    out["data_keys"] = keys_of(data).into();
    if let Some(code) = data.get("biz_code") {
        if code.is_number() {
            out["biz_code"] = code.clone();
        }
    }
    let Some(biz) = data.get("biz_data") else {
        return out;
    };
    if !biz.is_object() {
        out["biz_data_type"] = kind_of(biz).into();
        return out;
    }
    out["biz_data_keys"] = keys_of(biz).into();
    let mut arrays = serde_json::Map::new();
    for (key, value) in biz.as_object().expect("biz_data 是对象") {
        let Some(items) = value.as_array() else {
            continue;
        };
        let mut entry = json!({ "len": items.len() });
        if let Some(first) = items.first() {
            if first.is_object() {
                entry["first_item_keys"] = keys_of(first).into();
            }
        }
        arrays.insert(key.clone(), entry);
    }
    out["biz_data_arrays"] = Value::Object(arrays);
    out
}

/// 页内**只读 GET**表达式。
///
/// **为什么用占位符而不是 `format!`**：`format!` 把 `{`/`}` 当占位符语法，
/// 于是 JS 里每一个花括号都得写成 `{{`/`}}`——多写一个、少写一个，整段脚本
/// 就静默变形（编译能过、跑起来才发现不对）。这里改成**运行时替换**。
///
/// 这个表达式**只做 GET**：不新建会话、不发消息、不点任何按钮。
fn build_readonly_get_expression(path: &str) -> String {
    let template = r#"(async function () {
  var PATH = __ENDPOINT__;
  try {
    var res = await fetch(PATH, { method: "GET", credentials: "same-origin", headers: { accept: "application/json" } });
    return JSON.stringify({ ok: true, status: res.status });
  } catch (e) {
    return JSON.stringify({ ok: false, error: String(e && e.message ? e.message : e) });
  }
})()"#;
    let literal = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".to_string());
    template.replace("__ENDPOINT__", &literal)
}

/// 打码：uuid / 长十六进制 / 长 base64url 串 → `<masked>`。
fn mask_url(raw: &str) -> String {
    let is_token_char =
        |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
    let mut out = String::with_capacity(raw.len());
    let mut run = String::new();
    for ch in raw.chars() {
        if is_token_char(ch) {
            run.push(ch);
            continue;
        }
        out.push_str(&mask_run(&run));
        run.clear();
        out.push(ch);
    }
    out.push_str(&mask_run(&run));
    out
}

fn mask_run(run: &str) -> String {
    if run.len() >= 32 {
        return "<masked>".to_string();
    }
    let all_hex = run.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if all_hex && run.len() >= 16 {
        return "<masked>".to_string();
    }
    run.to_string()
}

fn keys_of(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn url_of(hit: &Value) -> String {
    hit.get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
