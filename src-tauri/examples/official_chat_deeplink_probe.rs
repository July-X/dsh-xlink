//! G4-20 只读取证探针 · **深链 ⇒ 站点自己发不发 `history_messages`**。
//!
//! # 它要回答什么（**一条读数，不是判断**）
//!
//! 协议 §4.4 那条候选机制的前置 **G4-20**：「导航到一条会话的深链
//! ⇒ 站点**确实**会发 `GET /api/v0/chat/history_messages?chat_session_id=<uuid>`」。
//! 本探针跑两段，只产出读数：
//!
//! 1. **目录取数**（拿真实会话 id）—— 站点自己那条
//!    `GET /api/v0/chat_session/fetch_page`，**壳侧**用
//!    `Network.getResponseBody` 取体（**不是**页内 fetch、不是页内注入）
//!    ⇒ 只解析出 `biz_data.chat_sessions[].id`。
//! 2. **深链导航** —— 建一扇**自己家的不可见窗**（`about:blank` 起、
//!    `visible(false)`）⇒ **先**按事件名订阅 `Network` 域 ＋ `Network.enable`
//!    ⇒ **然后**才导航到 `https://chat.deepseek.com/a/chat/s/<sessionId>`
//!    ⇒ 记录站点**自己**发出的请求行（端点 ＋ query ＋ 状态码 ＋ 时间戳）。
//!
//! # 与正式取数链的关系（**独立，不并入**）
//!
//! · 本文件是 **`src-tauri/examples/` 下的独立探针入口** ⇒ 不在
//!   `src/official_chat/session_structure.rs` 那条正式取数流程里、不改那条链的
//!   任何一行、也不给正式链加任何入口。
//! · 复用**生产那份** `cdp_eval.rs`（`#[path]` 编进来 ⇒ 测的是将来进生产的
//!   同一份代码）与生产那份 `OFFICIAL_CHAT_BROWSER_ARGS`、生产那份 user-data
//!   目录的**形态**（`data_directory` 给一份**副本**）。
//! · **壳侧一个请求都不发**：探针只订阅与观察，唯一的 CDP 调用是
//!   `Network.enable`（订阅）与 `Network.getResponseBody`（取体）——
//!   **不导航到任何 API 端点、不带任何自造请求头**。
//!
//! # 纪律
//!
//! - **只读**：不新建会话、不发消息、不点按钮。
//! - **不落盘正文**：目录响应体只在内存里解析出 `id` 就丢；报告里**只有会话 id**，
//!   **没有标题、没有正文、没有凭据**。
//! - **profile 由命令行显式给**（仓库硬不变量 `scripts/lib/probe-profile-isolation.mjs`：
//!   源码里不许出现产品 profile 目录名、不许把真实家目录与 profile 标记写在一行）⇒
//!   给的是**一次性临时目录下的副本**，浏览器不碰用户真实数据目录。
//! - **全程不开 DevTools 窗口**（只走协议调用）；对照组：**不存在的 CDP 方法必须报错**
//!   ⇒ 否则「`Network.enable` 成功」不能当证据。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_deeplink_probe -- \
//!   --profile "<一次性临时目录下的 user-data 目录副本>" \
//!   --out "<读数文件路径>" \
//!   [--session-id "<已知会话 id>"] \
//!   [--catalog-ms 120000] [--observe-ms 60000]
//! ```
//!
//! # 平台边界（**Windows 专用**）
//!
//! 整条链建立在 WebView2 的 DevTools 协议上 ⇒ 别的平台上 `webview2_com` 这个
//! crate 根本不存在 ⇒ 非 Windows 的构建里这一整块不存在。

#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

#[allow(dead_code)]
#[path = "../src/official_chat/cdp_eval.rs"]
mod cdp_eval;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cdp_eval::{cdp_call, cdp_field};

/// 站点首页（目录取数那一段的落点）。
const SITE_URL: &str = "https://chat.deepseek.com";

/// 协议 §4.4 的深链形态（G4-19 已坐实：两段参数 `agentId` ＋ `sessionId`）。
const DEEP_LINK_PREFIX: &str = "https://chat.deepseek.com/a/chat/s/";

/// 站点自己的会话目录端点（正式取数链认的那一个）。
const CATALOG_PATH: &str = "/api/v0/chat_session/fetch_page";

/// 待验的那条正文端点。
const HISTORY_PATH: &str = "/api/v0/chat/history_messages";

/// 订阅的 DevTools 事件（**按具体事件名** —— 官方 API 形态）。
const SUBSCRIBE_EVENTS: &[&str] = &[
    "Network.requestWillBeSent",
    "Network.responseReceived",
    "Network.loadingFinished",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--profile` 必须显式给：那是**一次性临时目录下的副本**；不给就不探测
    // （浏览器会落回默认数据目录 = 用户真实安装目录）。
    let Some(profile) = arg_value(&args, "--profile").map(PathBuf::from) else {
        eprintln!(
            "probe: 必须显式给 --profile <一次性临时目录下的 user-data 目录副本>；\
             不给就不探测（浏览器会落回默认数据目录）"
        );
        std::process::exit(2);
    };
    let Some(out) = arg_value(&args, "--out").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --out <读数文件路径>");
        std::process::exit(2);
    };
    // 已知会话 id（不给就自己走目录取数拿一个）。
    let session_id = arg_value(&args, "--session-id");
    // 目录取数那段的观察窗口（站点首屏可能慢）。
    let catalog_ms: u64 = arg_value(&args, "--catalog-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(120_000);
    // 深链那一段的观察窗口。
    let observe_ms: u64 = arg_value(&args, "--observe-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(60_000);
    // 跑哪一臂：`deeplink`（导航到会话深链）/ `root`（**对照组**：只导航到站点首页）。
    let arm = arg_value(&args, "--arm").unwrap_or_else(|| "deeplink".to_string());

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile = profile.clone();
            let out = out.clone();
            let session_id = session_id.clone();
            std::thread::Builder::new()
                .name("official-chat-deeplink-probe".into())
                .spawn(move || {
                    let report = tauri::async_runtime::block_on(run_probe(
                        &handle,
                        &profile,
                        session_id.as_deref(),
                        catalog_ms,
                        observe_ms,
                        &arm,
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

/// 探针从站里收到的两类消息。
enum Msg {
    /// 一条 DevTools 协议事件（事件名 ＋ 载荷原文 JSON）。
    Event { name: String, raw: String },
    /// 订阅/接线阶段的错误。
    Setup(String),
}

/// 一条请求的元信息（由 `requestWillBeSent` 记）。
#[derive(Clone)]
struct ReqMeta {
    method: String,
    resource_type: String,
    /// 站点**发出**这条请求的时刻（epoch 毫秒）——请求行的时间戳取这里。
    at_ms: u64,
}

/// 一条**请求行**（站点自己发出的）。
#[derive(Clone)]
struct RequestLine {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    status: i64,
    resource_type: String,
    loader_id: String,
    at_ms: u64,
}

impl RequestLine {
    /// 逐字请求行：`GET /api/v0/chat/history_messages?chat_session_id=<id> -> 200 @ <ts>`。
    fn render(&self) -> String {
        let query = if self.query.is_empty() {
            String::new()
        } else {
            let pairs = self
                .query
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            format!("?{pairs}")
        };
        format!(
            "{} {}{} -> {} @ {}",
            self.method,
            self.path,
            query,
            self.status,
            iso_utc(self.at_ms)
        )
    }

    fn to_json(&self) -> Value {
        json!({
            "request_line": self.render(),
            "method": self.method,
            "path": self.path,
            "query": self.query.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
            "status": self.status,
            "resource_type": self.resource_type,
            "loader_id": self.loader_id,
            "timestamp_utc": iso_utc(self.at_ms),
            "timestamp_epoch_ms": self.at_ms,
        })
    }
}

/// 事件累积状态。
#[derive(Default)]
struct EventState {
    total: u64,
    by_event: BTreeMap<String, u64>,
    setup_errors: Vec<String>,
    /// 已完成加载的请求（`loadingFinished` 到的 `requestId`）。
    finished: BTreeSet<String>,
    /// 请求元信息（`requestId` ⇒ 方法/URL/类型/时刻）。
    req_meta: BTreeMap<String, ReqMeta>,
    /// 站点自己发出的**全部** API 请求行。
    api_lines: Vec<RequestLine>,
    /// 观察到的文档 URL（这一轮停在哪个页面）。
    documents: Vec<String>,
    /// 目录端点那条响应的 `requestId`。
    catalog_request_id: Option<String>,
    /// 目录端点那条响应的状态码。
    catalog_status: Option<i64>,
}

/// 跑整个探针，出 JSON 读数。
async fn run_probe(
    handle: &tauri::AppHandle,
    profile: &Path,
    session_id: Option<&str>,
    catalog_ms: u64,
    observe_ms: u64,
    arm: &str,
) -> String {
    let mut report = json!({
        "probe": "G4-20 · 深链 ⇒ 站点是否自己发 history_messages（只读取证）",
        "arm": arm,
        "path_kind": "壳内路径：壳的 WebView2 ＋ 生产那份 cdp_eval.rs（探针 example 进程，非 dev 壳 GUI 进程）",
        "discipline": [
            "只读：不新建会话、不发消息、不点按钮",
            "壳侧零请求：只订阅与观察，唯一 CDP 调用是 Network.enable 与 Network.getResponseBody",
            "报告里只有会话 id：没有标题、没有正文、没有凭据",
            "profile 由命令行显式给（一次性临时目录下的副本）⇒ 不碰用户真实数据目录",
            "全程不开 DevTools 窗口"
        ],
        "started_at_utc": iso_utc(now_ms()),
        "profile_dir": profile.to_string_lossy(),
    });

    // --- 阶段 A：目录取数（拿真实会话 id） ---
    let catalog = match session_id {
        Some(_) => json!({ "skipped": true, "reason": "命令行已给 --session-id" }),
        None => run_catalog_phase(handle, profile, catalog_ms).await,
    };
    report["catalog"] = catalog.clone();

    let chosen: Option<String> = session_id.map(|s| s.to_string()).or_else(|| {
        catalog
            .get("session_ids")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(|s| s.to_string())
    });

    let Some(session_id) = chosen else {
        report["fatal"] = json!("没拿到真实会话 id（目录取数没成）⇒ 深链那一段没法跑");
        report["finished_at_utc"] = json!(iso_utc(now_ms()));
        return report.to_string();
    };
    report["session_id"] = json!(session_id);
    report["deep_link"] = json!(format!("{DEEP_LINK_PREFIX}{session_id}"));

    // --- 阶段 B：导航（深链，或对照组只到站点首页） ---
    report["deeplink"] = run_deeplink_phase(handle, profile, &session_id, observe_ms, arm).await;

    report["finished_at_utc"] = json!(iso_utc(now_ms()));
    report.to_string()
}

/// 阶段 A：导航到站点首页 → 站点自己发目录端点 → 壳侧取体 → 只解析出会话 id。
async fn run_catalog_phase(handle: &tauri::AppHandle, profile: &Path, catalog_ms: u64) -> Value {
    let mut out = json!({ "phase": "catalog", "site_url": SITE_URL });

    let window = match build_window(handle, profile, "g420-catalog") {
        Ok(window) => window,
        Err(e) => {
            out["fatal"] = json!(format!("无法创建探测窗口：{e}"));
            return out;
        }
    };

    // 订阅与 `Network.enable` **都在导航之前**（协议 §8.5.1 硬约束 3）。
    let events = subscribe_devtools_events(&window);
    let enable = cdp_call(
        &window,
        "Network.enable",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["network_enable"] = json!({
        "result": match &enable { Ok(raw) => json!({ "ok": true, "raw_result": raw }), Err(e) => json!({ "ok": false, "error": format!("{e:?}") }) }
    });
    // 对照组：不存在的 CDP 方法必须报错。
    let control = cdp_call(
        &window,
        "Network.thisMethodDoesNotExist",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["control_bogus_cdp_method"] = match &control {
        Ok(raw) => {
            json!({ "ok": true, "raw_result": raw, "note": "对照组异常：不存在的 CDP 方法居然成功" })
        }
        Err(e) => {
            json!({ "ok": false, "error": format!("{e:?}"), "note": "对照组正常 ⇒ 上面的 ok 是真成功" })
        }
    };
    if enable.is_err() {
        out["fatal"] = json!("Network.enable 失败 ⇒ 本段没跑成");
        return out;
    }

    let url: tauri::Url = match SITE_URL.parse() {
        Ok(url) => url,
        Err(e) => {
            out["fatal"] = json!(format!("URL 非法：{e}"));
            return out;
        }
    };
    out["navigate"] = match window.navigate(url) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };

    // 等目录端点那条响应**加载完**（取体要等 `loadingFinished`）。
    let mut state = EventState::default();
    let deadline = Instant::now() + Duration::from_millis(catalog_ms);
    observe(&events, &mut state, deadline, |st| {
        st.catalog_request_id
            .as_ref()
            .map(|rid| st.finished.contains(rid))
            .unwrap_or(false)
    });

    out["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "documents": state.documents.clone(),
        "catalog_observed": state.catalog_request_id.is_some(),
        "catalog_status": state.catalog_status,
        "note": "目录端点由**站点自己**发出；壳侧只是订阅与取体"
    });

    let Some(request_id) = state.catalog_request_id.clone() else {
        out["fatal"] = json!("本轮没看到站点发目录端点那条请求");
        return out;
    };

    // 壳侧取体（生产那份 `cdp_call` ⇒ 与正式取数链同一个调用）。
    let body = cdp_call(
        &window,
        "Network.getResponseBody",
        json!({ "requestId": request_id }),
        Duration::from_secs(20),
    )
    .await;
    match body {
        Ok(envelope) => {
            let text = cdp_field(&envelope, "body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let mut ids = Vec::new();
            collect_session_ids(&parsed, &mut ids);
            ids.dedup();
            out["session_ids"] = json!(ids);
            out["session_count"] = json!(ids.len());
            out["body_bytes"] = json!(text.len());
            out["body_note"] = json!("响应体只在内存里解析出 id 就丢；报告里不落盘正文");
        }
        Err(e) => {
            out["fatal"] = json!(format!("取目录响应体失败：{e:?}"));
        }
    }

    out
}

/// 阶段 B：导航到深链 → 记录站点**自己**发出的请求行。
async fn run_deeplink_phase(
    handle: &tauri::AppHandle,
    profile: &Path,
    session_id: &str,
    observe_ms: u64,
    arm: &str,
) -> Value {
    let mut out = json!({ "phase": "deeplink", "arm": arm });

    let window = match build_window(handle, profile, "g420-deeplink") {
        Ok(window) => window,
        Err(e) => {
            out["fatal"] = json!(format!("无法创建探测窗口：{e}"));
            return out;
        }
    };

    // 订阅与 `Network.enable` **都在导航之前**。
    let events = subscribe_devtools_events(&window);
    let enable = cdp_call(
        &window,
        "Network.enable",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["network_enable"] = json!({
        "result": match &enable { Ok(raw) => json!({ "ok": true, "raw_result": raw }), Err(e) => json!({ "ok": false, "error": format!("{e:?}") }) }
    });
    let control = cdp_call(
        &window,
        "Network.thisMethodDoesNotExist",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["control_bogus_cdp_method"] = match &control {
        Ok(raw) => {
            json!({ "ok": true, "raw_result": raw, "note": "对照组异常：不存在的 CDP 方法居然成功" })
        }
        Err(e) => {
            json!({ "ok": false, "error": format!("{e:?}"), "note": "对照组正常 ⇒ 上面的 ok 是真成功" })
        }
    };
    if enable.is_err() {
        out["fatal"] = json!("Network.enable 失败 ⇒ 本段没跑成");
        return out;
    }

    // `deeplink` 臂：导航到那条会话的深链；`root` 臂（**对照组**）：只导航到站点首页
    // —— 用来证明「那条正文请求是深链导航带出来的」，不是站点首屏自带的。
    let deep_link = format!("{DEEP_LINK_PREFIX}{session_id}");
    let target = match arm {
        "deeplink" => deep_link.clone(),
        "root" => SITE_URL.to_string(),
        other => {
            out["fatal"] = json!(format!("未知 arm：{other}（只认 deeplink / root）"));
            return out;
        }
    };
    out["deep_link"] = json!(deep_link);
    out["navigate_target"] = json!(target);
    let url: tauri::Url = match target.parse() {
        Ok(url) => url,
        Err(e) => {
            out["fatal"] = json!(format!("导航目标非法：{e}"));
            return out;
        }
    };
    out["navigate"] = match window.navigate(url) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };

    // 观察整段窗口：站点自己会发什么就记什么。
    let mut state = EventState::default();
    let deadline = Instant::now() + Duration::from_millis(observe_ms);
    observe(&events, &mut state, deadline, |_| false);

    // 落点 URL（这一轮最终停在哪个页面）—— 只取 URL，不取标题（标题可能带会话标题）。
    let landing = cdp_call(
        &window,
        "Runtime.evaluate",
        json!({ "expression": "location.href", "returnByValue": true }),
        Duration::from_secs(15),
    )
    .await;
    out["landing_url"] = match &landing {
        Ok(envelope) => cdp_field(envelope, "value")
            .and_then(Value::as_str)
            .map(|s| json!(s))
            .unwrap_or(Value::Null),
        Err(e) => json!(format!("取不到落点 URL：{e:?}")),
    };

    let history_hits: Vec<&RequestLine> = state
        .api_lines
        .iter()
        .filter(|line| line.path.contains(HISTORY_PATH))
        .collect();

    out["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "documents": state.documents.clone(),
        "api_request_count": state.api_lines.len(),
        "note": "api_request_count 只数站点自己发的 /api/ 请求（壳侧一个都没发）"
    });

    out["api_request_lines"] = json!(state
        .api_lines
        .iter()
        .map(RequestLine::to_json)
        .collect::<Vec<_>>());

    out["history_messages_hits"] =
        json!(history_hits.iter().map(|l| l.to_json()).collect::<Vec<_>>());
    out["history_messages_observed"] = json!(!history_hits.is_empty());

    // 判定所需的三个事实，逐条列出来（**只列事实，不下结论**）。
    out["facts"] = json!({
        "site_initiated_api_requests": state.api_lines.len(),
        "history_messages_request_seen": !history_hits.is_empty(),
        "history_messages_query_keys": history_hits.first().map(|l| l.query.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()).unwrap_or_default(),
        "history_messages_query_value_matches_session_id": history_hits.first().map(|l| l.query.iter().any(|(k, v)| k == "chat_session_id" && v == session_id)).unwrap_or(false),
        "history_messages_status": history_hits.first().map(|l| l.status),
        "other_api_paths": state.api_lines.iter().map(|l| l.path.clone()).filter(|p| !p.contains(HISTORY_PATH)).collect::<Vec<_>>()
    });

    out
}

/// 建探针窗口：停在 `about:blank`，等订阅与 `Network.enable` 完成后才导航。
///
/// **与生产那份取数窗同形**：不可见（`visible(false)`）＋ 显式 `data_directory`
/// ＋ 生产那份浏览器参数 ⇒ 站点侧看到的是**普通浏览器**，页内零注入。
fn build_window(
    handle: &tauri::AppHandle,
    profile: &Path,
    label: &str,
) -> Result<tauri::WebviewWindow, String> {
    let url: tauri::Url = "about:blank"
        .parse()
        .map_err(|e| format!("URL 非法：{e}"))?;
    tauri::WebviewWindowBuilder::new(handle, label, tauri::WebviewUrl::External(url))
        .title("G4-20 深链探针")
        .inner_size(1000.0, 760.0)
        .visible(false)
        .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .data_directory(profile.to_path_buf())
        .build()
        .map_err(|e| format!("无法创建探针窗口：{e}"))
}

/// 按事件名订阅 DevTools 协议事件（官方 API 形态）。
fn subscribe_devtools_events(window: &tauri::WebviewWindow) -> Receiver<Msg> {
    use webview2_com::DevToolsProtocolEventReceivedEventHandler;
    use windows_core::HSTRING;

    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let tx_outer = tx.clone();
    let tx_in_closure = tx_outer.clone();
    let dispatched = window.with_webview(move |platform| {
        let controller = platform.controller();
        // SAFETY: `CoreWebView2` 只是取回控制器持有的核心对象引用，不转移所有权。
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                let _ = tx_in_closure.send(Msg::Setup(format!("取不到 WebView2 核心：{e}")));
                return;
            }
        };
        for event_name in SUBSCRIBE_EVENTS {
            // 每个订阅一份 sender：事件处理函数是 `'static` 的闭包，会把 sender 带走。
            let tx_event = tx_in_closure.clone();
            let name_hstring = HSTRING::from(*event_name);
            // SAFETY: 事件名是静态字符串，调用期间由 `name_hstring` 持有。
            let receiver = match unsafe { core.GetDevToolsProtocolEventReceiver(&name_hstring) } {
                Ok(receiver) => receiver,
                Err(e) => {
                    let _ = tx_in_closure
                        .send(Msg::Setup(format!("取不到事件 {event_name} 的接收器：{e}")));
                    continue;
                }
            };
            let label = event_name.to_string();
            let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(
                move |_sender, args| {
                    if let Some(args) = args {
                        let mut pwstr = windows_core::PWSTR::null();
                        // SAFETY: `ParameterObjectAsJson` 把一段以 CoTaskMemAlloc
                        // 分配的 PWSTR 写进 `pwstr`；下面的 `CoTaskMemPWSTR` 负责释放它。
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
            if let Err(e) =
                unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }
            {
                let _ = tx_in_closure.send(Msg::Setup(format!("订阅 {event_name} 失败：{e}")));
            }
        }
    });
    if let Err(e) = dispatched {
        let _ = tx_outer.send(Msg::Setup(format!("with_webview 失败：{e}")));
    }
    rx
}

/// 抽干事件直到截止时间或 `stop` 成立。
fn observe<F>(events: &Receiver<Msg>, state: &mut EventState, deadline: Instant, mut stop: F)
where
    F: FnMut(&EventState) -> bool,
{
    loop {
        if Instant::now() >= deadline || stop(state) {
            return;
        }
        match events.recv_timeout(Duration::from_millis(200)) {
            Ok(Msg::Setup(detail)) => state.setup_errors.push(detail),
            Ok(Msg::Event { name, raw }) => {
                state.total += 1;
                let Ok(value) = serde_json::from_str::<Value>(&raw) else {
                    continue;
                };
                *state.by_event.entry(name.clone()).or_insert(0) += 1;
                let request_id = value
                    .get("requestId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let loader_id = value
                    .get("loaderId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let resource_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                match name.as_str() {
                    "Network.requestWillBeSent" => {
                        let request = value.get("request");
                        let url = request
                            .and_then(|r| r.get("url"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let method = request
                            .and_then(|r| r.get("method"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if resource_type == "Document" && state.documents.len() < 16 {
                            state.documents.push(url.clone());
                        }
                        if !request_id.is_empty() {
                            state.req_meta.insert(
                                request_id.clone(),
                                ReqMeta {
                                    method,
                                    resource_type: resource_type.clone(),
                                    at_ms: now_ms(),
                                },
                            );
                        }
                    }
                    "Network.responseReceived" => {
                        let Some(response) = value.get("response") else {
                            continue;
                        };
                        let url = response
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let status = response
                            .get("status")
                            .and_then(Value::as_i64)
                            .unwrap_or_default();
                        if url.contains(CATALOG_PATH) && state.catalog_request_id.is_none() {
                            state.catalog_request_id = Some(request_id.clone());
                            state.catalog_status = Some(status);
                        }
                        if !url.contains("/api/") {
                            continue;
                        }
                        let meta = state.req_meta.get(&request_id);
                        let line = RequestLine {
                            method: meta
                                .map(|m| m.method.clone())
                                .filter(|m| !m.is_empty())
                                .unwrap_or_else(|| "?".to_string()),
                            path: url_path(&url),
                            query: url_query(&url),
                            status,
                            resource_type: if resource_type.is_empty() {
                                meta.map(|m| m.resource_type.clone()).unwrap_or_default()
                            } else {
                                resource_type.clone()
                            },
                            loader_id: loader_id.clone(),
                            at_ms: meta.map(|m| m.at_ms).unwrap_or_else(now_ms),
                        };
                        state.api_lines.push(line);
                    }
                    "Network.loadingFinished" if !request_id.is_empty() => {
                        state.finished.insert(request_id.clone());
                    }
                    _ => {}
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// 从目录响应体里收集**会话 id**（只要 `id`，标题/正文一律不留）。
fn collect_session_ids(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "chat_sessions" {
                    if let Some(items) = value.as_array() {
                        for item in items {
                            if let Some(id) = item.get("id").and_then(Value::as_str) {
                                if !id.is_empty() {
                                    out.push(id.to_string());
                                }
                            }
                        }
                    }
                }
                collect_session_ids(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_session_ids(item, out);
            }
        }
        _ => {}
    }
}

/// 取 URL 的路径部分。
fn url_path(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => parsed.path().to_string(),
        Err(_) => url.to_string(),
    }
}

/// 取 URL 的 query（**键名全留**；值：会话 id 原样留，其余长串打码）。
fn url_query(url: &str) -> Vec<(String, String)> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Vec::new();
    };
    parsed
        .query_pairs()
        .map(|(key, value)| {
            let key = key.to_string();
            let value = if key == "chat_session_id" || value.len() <= 32 {
                value.to_string()
            } else {
                format!("<masked:{} chars>", value.len())
            };
            (key, value)
        })
        .collect()
}

/// 现在的时刻（epoch 毫秒）。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// epoch 毫秒 ⇒ UTC ISO 8601（带毫秒）。
fn iso_utc(ms: u64) -> String {
    let seconds = ms / 1000;
    let millis = ms % 1000;
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// 天数 ⇒ 公历日期（Howard Hinnant 的 `civil_from_days`）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}
