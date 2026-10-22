//! 只读取证探针 · **目录段与正文段各自的响应体形状**（DeepSeek 官网对话）。
//!
//! # 它要回答什么（**一条读数，不是判断**）
//!
//! 两个数一次坐实：
//!
//! 1. **目录 100 条为什么只裁出几条** —— 站点首屏那条
//!    `GET /api/v0/chat_session/fetch_page` 的响应体里，**递归**能数出 100 个
//!    会话 id；但壳侧只读 `data.biz_data.chat_sessions`（协议 P4 §4.3 ①）⇒ ⇒
//!    本段把**每一个** `chat_sessions` 数组的**路径与长度**、**带 `id` 字段的对象总数**、
//!    `has_more` 一并落下来 ⇒ ⇒ 「只裁出几条」到底是**裁剪口径**还是**站点只给了几条**。
//! 2. **单会话正文为什么裁不出消息** —— 深链导航后，站点**自己**发
//!    `GET /api/v0/chat/history_messages?chat_session_id=<uuid>` ⇒ ⇒ 本段把那条
//!    响应的**形状**落下来：消息数组在哪个路径、有多长、每条消息的**键名集合**、
//!    `role`／`type` 的**取值集合** ⇒ ⇒ **一次分开三种可能**：
//!    ① 消息数组长度 0（站点返回的是真空）② 元素的键名里**没有** `role`／`type`
//!    （壳侧认错了字段名）③ 键名里有 `role`／`type` 但**取值**不在
//!    `USER`／`ASSISTANT`／`REQUEST`／`RESPONSE`／`THINK` 里（取值口径不对）。
//!
//! # 与正式取数链的关系（**独立，不并入**）
//!
//! · 本文件是 **`src-tauri/examples/` 下的独立探针入口** ⇒ 不在
//!   `src/official_chat/session_structure.rs`（目录那条）与
//!   `src/official_chat/session_body.rs`（正文那条）任何一条正式取数流程里、
//!   不改那两条链的**任何一行**、也不给它们加任何入口。
//! · 复用**生产那份** `cdp_eval.rs`（`#[path]` 编进来 ⇒ 测的是将来进生产的
//!   同一份代码）与生产那份 `OFFICIAL_CHAT_BROWSER_ARGS`、生产那份 user-data
//!   目录的**形态**（`data_directory` 给一份**副本**）。
//! · **壳侧一个请求都不发**：探针只订阅与观察，唯一的 CDP 调用是
//!   `Network.enable`（订阅）与 `Network.getResponseBody`（取体）——
//!   **不导航到任何 API 端点、不带任何自造请求头、不做页内求值**。
//!
//! # 纪律
//!
//! - **只读**：不新建会话、不发消息、不点按钮。
//! - **不落盘正文**：目录响应体只在内存里解析出**键名与计数**就丢；正文响应体
//!   只解析出**键名集合**与 `role`／`type` 的**取值集合**就丢 ⇒ ⇒ 报告里**没有**
//!   正文原文、**没有**会话标题、**没有**凭据。
//! - **profile 由命令行显式给**（仓库硬不变量 `scripts/lib/probe-profile-isolation.mjs`：
//!   源码里不许出现产品 profile 目录名、不许把真实家目录与 profile 标记写在一行）⇒
//!   给的是**一次性临时目录下的副本**，浏览器不碰用户真实数据目录。
//! - **全程不开 DevTools 窗口**（只走协议调用）；**对照组**：不存在的 CDP 方法必须报错
//!   ⇒ 否则「`Network.enable` 成功」不能当证据。**每段各跑一次**这个对照。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_shape_probe -- \
//!   --profile "<一次性临时目录下的 user-data 目录副本>" \
//!   [--profile-src "<只读源 profile 目录>" --profile "<副本目录>"] \
//!   --out-dir "<读数目录>" \
//!   --session-id "<已知会话 id>" [--session-id "<另一个 id>"] \
//!   [--catalog-ms 120000] [--body-ms 60000]
//! ```
//!
//! ⚠️ **必须给副本**：WebView2 会**独占** user-data 目录 ⇒ ⇒ 直接指向源目录
//! 会被上一轮的浏览器锁住 ⇒ ⇒ 给 `--profile-src` 时探针**自己**用仓库现成的
//! **只读**拷贝器（`examples/support/profile_copy.rs`）先拷一份再用。
//!
//! # 平台边界（**Windows 专用**）
//!
//! 整条链建立在 WebView2 的 DevTools 协议上 ⇒ 别的平台上 `webview2_com` 这个
//! crate 根本不存在 ⇒ 非 Windows 的构建里这一整块不存在。

#![cfg_attr(not(windows), allow(dead_code))]

#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

#[allow(dead_code)]
#[path = "../src/official_chat/cdp_eval.rs"]
mod cdp_eval;

/// 仓库现成的**只读** profile 拷贝器（`examples/support/profile_copy.rs`）：
/// 源只读、写到一次性临时目录、源被独占时跳过并记账。
#[allow(dead_code)]
#[path = "support/profile_copy.rs"]
mod profile_copy;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cdp_eval::{cdp_call, cdp_field};

/// 站点首页（目录取数那一段的落点）。
const SITE_URL: &str = "https://chat.deepseek.com";

/// 单会话的深链形态（G4-19 已坐实）。
const DEEP_LINK_PREFIX: &str = "https://chat.deepseek.com/a/chat/s/";

/// 站点自己的会话目录端点（正式取数链认的那一个）。
const CATALOG_PATH: &str = "/api/v0/chat_session/fetch_page";

/// 站点自己的单会话正文端点。
const HISTORY_PATH: &str = "/api/v0/chat/history_messages";

/// 订阅的 DevTools 事件（**按具体事件名** —— 官方 API 形态）。
const SUBSCRIBE_EVENTS: &[&str] = &[
    "Network.requestWillBeSent",
    "Network.responseReceived",
    "Network.loadingFinished",
];

#[cfg(windows)]
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
    // `--profile-src`（可选）：**只读**源 profile 目录 —— 探针**自己**先用仓库
    // 现成的只读拷贝器拷一份到 `--profile` 那个副本目录（WebView2 会独占目录 ⇒
    // 直接指向源会被上一轮的浏览器锁住）。
    let profile_src = arg_value(&args, "--profile-src").map(PathBuf::from);
    let Some(out_dir) = arg_value(&args, "--out-dir").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --out-dir <读数目录>");
        std::process::exit(2);
    };
    // 待验的会话 id（可给多个；不给就没有正文段）。
    let session_ids = arg_values(&args, "--session-id");
    let catalog_ms: u64 = arg_value(&args, "--catalog-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(120_000);
    let body_ms: u64 = arg_value(&args, "--body-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(60_000);

    // --- 拷贝 profile（源只读；拷不动就跳过并记账） ---
    //
    // **必须在建 app 之前做完**：WebView2 会**独占** user-data 目录 ⇒ ⇒ 浏览器
    // 启动前副本就得在位；而且拷贝要花时间 ⇒ ⇒ 放在探针线程里会拖慢「建窗」
    // ⇒ ⇒ 与下面的保活窗竞态撞车。
    let mut copy_report = json!({ "copied": false });
    if let Some(src) = profile_src.as_ref() {
        match profile_copy::copy_dir_readonly(src, &profile) {
            Ok(report) => {
                copy_report = json!({
                    "copied": true,
                    "files": report.files,
                    "bytes": report.bytes,
                    "skipped_count": report.skipped.len(),
                    "skip_reason": report.skip_reason,
                    "note": "源只读：只从源读、绝不写、绝不删；源被独占时跳过并记账"
                });
            }
            Err(e) => {
                copy_report = json!({ "copied": false, "error": format!("{e}") });
            }
        }
    }

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile = profile.clone();
            let out_dir = out_dir.clone();
            let session_ids = session_ids.clone();
            let copy_report = copy_report.clone();

            // **保活窗**：Tauri 的事件循环在**一扇窗口都没有**时会立刻退出 ⇒ ⇒
            // 探针自己的窗是探针线程建的（慢一点就输给这个竞态）⇒ ⇒ 先建一扇
            // `about:blank` 的不可见窗把事件循环**按住**（它不导航、不发任何请求）。
            if let Err(e) = build_window(&handle, &profile, "shape-keepalive") {
                eprintln!("probe: 保活窗建不起来：{e}");
            }

            std::thread::Builder::new()
                .name("official-chat-shape-probe".into())
                .spawn(move || {
                    let (mut catalog, mut body) = tauri::async_runtime::block_on(run_probe(
                        &handle,
                        &profile,
                        &session_ids,
                        catalog_ms,
                        body_ms,
                    ));

                    catalog["profile_copy"] = copy_report.clone();
                    body["profile_copy"] = copy_report;

                    let catalog_path = out_dir.join("reading-catalog.json");
                    let body_path = out_dir.join("reading-body.json");
                    let _ = std::fs::create_dir_all(&out_dir);
                    let _ = std::fs::write(
                        &catalog_path,
                        format!(
                            "{}\n",
                            serde_json::to_string_pretty(&catalog).unwrap_or_default()
                        ),
                    );
                    let _ = std::fs::write(
                        &body_path,
                        format!(
                            "{}\n",
                            serde_json::to_string_pretty(&body).unwrap_or_default()
                        ),
                    );
                    println!("PROBE_REPORT_BEGIN");
                    println!("catalog_reading={}", catalog_path.display());
                    println!("body_reading={}", body_path.display());
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

/// 取一个命令行参数的值（`--key value` 与 `--key=value` 都认）。
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

/// 取**全部**同名命令行参数的值（`--key a --key b`）。
fn arg_values(args: &[String], key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            if let Some(value) = iter.next() {
                out.push(value.clone());
            }
            continue;
        }
        if let Some(rest) = arg.strip_prefix(&format!("{key}=")) {
            out.push(rest.to_string());
        }
    }
    out
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
    /// 站点**发出**这条请求的时刻（epoch 毫秒）。
    at_ms: u64,
}

/// 一条**命中**的响应（路径匹配上了目标端点）。
struct Hit {
    request_id: String,
    method: String,
    path: String,
    query: Vec<(String, String)>,
    status: i64,
    resource_type: String,
    at_ms: u64,
    /// 响应体字节数（**取体成功**才有）。
    body_bytes: Option<usize>,
    /// 取体失败的原因（**不落 body 原文**）。
    body_error: Option<String>,
    /// 响应体的**形状**分析（**只有键名与计数**）。
    body_shape: Option<Value>,
}

/// 事件累积状态。
#[derive(Default)]
struct EventState {
    total: u64,
    by_event: BTreeMap<String, u64>,
    setup_errors: Vec<String>,
    /// 已完成加载的请求（`loadingFinished` 到的 `requestId`）。
    finished: BTreeSet<String>,
    /// 请求元信息（`requestId` ⇒ 方法/类型/时刻）。
    req_meta: BTreeMap<String, ReqMeta>,
    /// 站点自己发出的**全部** API 请求（只记路径，不记 URL 全文）。
    api_paths: Vec<String>,
    /// 观察到的文档 URL。
    documents: Vec<String>,
    /// 命中目标端点的那些响应。
    hits: Vec<Hit>,
}

/// 跑整个探针，出两份读数（目录段 ＋ 正文段）。
async fn run_probe(
    handle: &tauri::AppHandle,
    profile: &Path,
    session_ids: &[String],
    catalog_ms: u64,
    body_ms: u64,
) -> (Value, Value) {
    let started = now_ms();
    let discipline = vec![
        "只读：不新建会话、不发消息、不点按钮",
        "壳侧零请求：只订阅与观察，CDP 调用只有 Network.enable、Network.getResponseBody（＋每段一次的 bogus 对照）",
        "报告里只有键名、计数、字节数、状态码与会话 id：没有正文原文、没有会话标题、没有凭据",
        "profile 由命令行显式给（一次性临时目录下的副本）⇒ 不碰用户真实数据目录",
        "全程不开 DevTools 窗口",
    ];
    let path_kind =
        "壳内路径：壳的 WebView2 ＋ 生产那份 cdp_eval.rs，但跑在**探针 example 进程**里，不是 dev 壳 GUI 进程";

    // --- 阶段 A：目录段 ---
    let catalog =
        run_catalog_phase(handle, profile, catalog_ms, started, &discipline, path_kind).await;

    // --- 阶段 B：正文段（每个会话 id 各跑一次）---
    let mut sessions = Vec::new();
    for session_id in session_ids {
        let one = run_body_phase(handle, profile, session_id, body_ms, started).await;
        sessions.push(one);
    }
    let body = json!({
        "probe": "只读取证 · 单会话正文的响应体形状（DeepSeek）",
        "phase": "body",
        "path_kind": path_kind,
        "discipline": discipline,
        "started_at_utc": iso_utc(started),
        "session_count": session_ids.len(),
        "sessions": sessions,
        "finished_at_utc": iso_utc(now_ms()),
    });

    (catalog, body)
}

/// 阶段 A：导航到站点首页 → 站点自己发目录端点 → 壳侧取体 → 只落**键名与计数**。
async fn run_catalog_phase(
    handle: &tauri::AppHandle,
    profile: &Path,
    catalog_ms: u64,
    started: u64,
    discipline: &[&str],
    path_kind: &str,
) -> Value {
    let mut out = json!({
        "probe": "只读取证 · 会话目录的响应体形状（DeepSeek）",
        "phase": "catalog",
        "site_url": SITE_URL,
        "path_kind": path_kind,
        "discipline": discipline,
        "started_at_utc": iso_utc(started),
        "profile_dir": profile.to_string_lossy(),
    });

    let window = match build_window(handle, profile, "shape-catalog") {
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
    // 对照组：不存在的 CDP 方法必须报错（**每段各跑一次**）。
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
    observe(&window, &events, &mut state, deadline, CATALOG_PATH, 3_000).await;

    out["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "documents": state.documents.clone(),
        "api_request_count": state.api_paths.len(),
        "api_paths": state.api_paths.clone(),
        "note": "目录端点由**站点自己**发出；壳侧只是订阅与取体"
    });

    let hit = state.hits.first();
    out["hit_count"] = json!(state.hits.len());
    out["http_status"] = match hit {
        Some(h) => json!(h.status),
        None => Value::Null,
    };
    out["body_bytes"] = match hit.and_then(|h| h.body_bytes) {
        Some(bytes) => json!(bytes),
        None => Value::Null,
    };
    out["body_error"] = match hit {
        Some(h) => json!(h.body_error.clone()),
        None => Value::Null,
    };
    out["body_shape"] = match hit.and_then(|h| h.body_shape.clone()) {
        Some(shape) => shape,
        None => json!({ "note": "这一段没取到目录响应体 ⇒ 没有形状读数" }),
    };
    out["finished_at_utc"] = json!(iso_utc(now_ms()));

    // ⚠️ **不关窗**：Tauri 的事件循环在**最后一扇窗关掉**时会退出 ⇒ ⇒ 关窗等于
    // 把整个探针进程带走（读数还没落盘就没了）。窗留着，进程由 `handle.exit(0)` 收。
    out
}

/// 阶段 B：导航到那条会话的深链 → 站点自己发正文端点 → 壳侧取体 → 只落**键名与计数**。
async fn run_body_phase(
    handle: &tauri::AppHandle,
    profile: &Path,
    session_id: &str,
    body_ms: u64,
    started: u64,
) -> Value {
    let deep_link = format!("{DEEP_LINK_PREFIX}{session_id}");
    let mut out = json!({
        "phase": "body",
        "session_id": session_id,
        "deep_link": deep_link,
        "started_at_utc": iso_utc(started),
    });

    let label = format!(
        "shape-body-{}",
        session_id.chars().take(8).collect::<String>()
    );
    let window = match build_window(handle, profile, &label) {
        Ok(window) => window,
        Err(e) => {
            out["fatal"] = json!(format!("无法创建探测窗口：{e}"));
            return out;
        }
    };

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
    // 对照组：不存在的 CDP 方法必须报错（**每段各跑一次**）。
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

    let url: tauri::Url = match deep_link.parse() {
        Ok(url) => url,
        Err(e) => {
            out["fatal"] = json!(format!("深链非法：{e}"));
            return out;
        }
    };
    out["navigate"] = match window.navigate(url) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };

    let mut state = EventState::default();
    let deadline = Instant::now() + Duration::from_millis(body_ms);
    observe(&window, &events, &mut state, deadline, HISTORY_PATH, 5_000).await;

    out["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "documents": state.documents.clone(),
        "api_request_count": state.api_paths.len(),
        "api_paths": state.api_paths.clone(),
        "note": "正文端点由**站点自己**发出；壳侧只是订阅与取体"
    });

    out["history_messages_hit_count"] = json!(state.hits.len());
    out["history_messages_hits"] = json!(state
        .hits
        .iter()
        .map(|hit| {
            json!({
                "request_id": hit.request_id,
                "method": hit.method,
                "path": hit.path,
                "query": hit.query.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
                "status": hit.status,
                "resource_type": hit.resource_type,
                "timestamp_utc": iso_utc(hit.at_ms),
                "body_bytes": hit.body_bytes,
                "body_error": hit.body_error,
                "chat_session_id_matches_target": hit
                    .query
                    .iter()
                    .any(|(k, v)| k == "chat_session_id" && v == session_id),
                "body_shape": hit.body_shape.clone(),
            })
        })
        .collect::<Vec<_>>());
    out["finished_at_utc"] = json!(iso_utc(now_ms()));

    // ⚠️ **不关窗**（同目录段那一条）：关掉最后一扇窗 = 事件循环退出 = 进程走人。
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
        .title("形状探针")
        .inner_size(1000.0, 760.0)
        .visible(false)
        .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .data_directory(profile.to_path_buf())
        .build()
        .map_err(|e| format!("无法创建探针窗口：{e}"))
}

/// 按事件名订阅 DevTools 协议事件（官方 API 形态）。
#[cfg(windows)]
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

/// 抽干事件直到截止时间；命中目标端点且**加载完**的响应**立刻取体**。
///
/// 取体只在 `Network.loadingFinished` 之后（协议 §8.5.1 硬约束 1：取早了会**静默**
/// 拿到长度 0 的 body 且不报错 ⇒ ⇒ 空体一律算「回程失败」，不当成「站点没数据」）。
async fn observe(
    window: &tauri::WebviewWindow,
    events: &Receiver<Msg>,
    state: &mut EventState,
    deadline: Instant,
    match_path: &str,
    quiet_ms: u64,
) {
    let mut last_hit_at: Option<Instant> = None;
    loop {
        if Instant::now() >= deadline {
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
                        if !url.contains("/api/") {
                            continue;
                        }
                        let path = url_path(&url);
                        state.api_paths.push(path.clone());
                        if !path.contains(match_path) {
                            continue;
                        }
                        let meta = state.req_meta.get(&request_id);
                        state.hits.push(Hit {
                            request_id: request_id.clone(),
                            method: meta
                                .map(|m| m.method.clone())
                                .filter(|m| !m.is_empty())
                                .unwrap_or_else(|| "?".to_string()),
                            path,
                            query: url_query(&url),
                            status,
                            resource_type: if resource_type.is_empty() {
                                meta.map(|m| m.resource_type.clone()).unwrap_or_default()
                            } else {
                                resource_type.clone()
                            },
                            at_ms: meta.map(|m| m.at_ms).unwrap_or_else(now_ms),
                            body_bytes: None,
                            body_error: None,
                            body_shape: None,
                        });
                        last_hit_at = Some(Instant::now());
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

        // 取体：命中且已加载完 ⇒ 立刻取（不等到窗口末尾，避免 body 被浏览器回收）。
        for index in 0..state.hits.len() {
            let ready = match state.hits.get(index) {
                Some(hit) => {
                    hit.body_bytes.is_none()
                        && hit.body_error.is_none()
                        && state.finished.contains(&hit.request_id)
                }
                None => false,
            };
            if !ready {
                continue;
            }
            let request_id = state.hits[index].request_id.clone();
            let envelope = cdp_call(
                window,
                "Network.getResponseBody",
                json!({ "requestId": request_id }),
                Duration::from_secs(20),
            )
            .await;
            let mut bytes = None;
            let mut error = None;
            let mut shape = None;
            match envelope {
                Ok(raw) => match decode_body(&raw) {
                    Ok(text) => {
                        bytes = Some(text.len());
                        shape = Some(analyze_shape(&text));
                    }
                    Err(detail) => error = Some(detail),
                },
                Err(e) => error = Some(format!("{e:?}")),
            }
            if let Some(hit) = state.hits.get_mut(index) {
                hit.body_bytes = bytes;
                hit.body_error = error;
                hit.body_shape = shape;
            }
        }

        // 静默期：已经取到体、且一段时间没有新命中 ⇒ 收工。
        if let Some(at) = last_hit_at {
            let has_body = state
                .hits
                .iter()
                .any(|hit| hit.body_bytes.is_some() || hit.body_error.is_some());
            if has_body && at.elapsed() >= Duration::from_millis(quiet_ms) {
                return;
            }
        }
    }
}

/// 解出响应体（`base64Encoded` 两种都认 —— 与生产那份 `cdp_eval` 的取体口径一致）。
fn decode_body(envelope: &Value) -> Result<String, String> {
    let encoded = cdp_field(envelope, "base64Encoded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let body = cdp_field(envelope, "body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if body.is_empty() {
        return Err(
            "取到的响应体是空的（协议 §8.5.1 硬约束 1：取早了会**静默**拿到空体 ⇒ 这是回程失败，不是站点没数据）"
                .to_string(),
        );
    }
    if !encoded {
        return Ok(body);
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|e| format!("响应体是 base64 但解不开：{e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("响应体不是 UTF-8：{e}"))
}

/// 把响应体解析成**形状读数**（**只有键名与计数** —— 一个值都不落）。
///
/// 一段代码同时服务目录段与正文段 ⇒ ⇒ 两段的读数口径**完全一致**。
fn analyze_shape(body: &str) -> Value {
    let root: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(e) => return json!({ "parse_error": format!("{e}") }),
    };
    let data = root.get("data");
    let biz_data = data.and_then(|d| d.get("biz_data"));

    // 递归找出**所有**叫 `chat_sessions` 的数组（路径 ＋ 长度）。
    let mut session_arrays: Vec<Value> = Vec::new();
    find_arrays_named(&root, &["chat_sessions"], "$", &mut session_arrays);
    // 递归找出**所有**叫 `messages` / `chat_messages` 的数组（路径 ＋ 长度）。
    let mut message_arrays: Vec<Value> = Vec::new();
    find_arrays_named(
        &root,
        &["messages", "chat_messages"],
        "$",
        &mut message_arrays,
    );

    let mut out = json!({
        "top_level_keys": sorted_keys_value(Some(&root)),
        "data_keys": sorted_keys_value(data),
        "biz_data_keys": sorted_keys_value(biz_data),
        // 站内码（协议 §6.2：顶层 `code` ＋ 业务层 `biz_code`）——**状态码**，不是正文。
        "site_code": root.get("code").cloned().unwrap_or(Value::Null),
        "site_biz_code": data.and_then(|d| d.get("biz_code")).cloned().unwrap_or(Value::Null),
        "site_msg": root
            .get("msg")
            .and_then(Value::as_str)
            .map(|text| json!(clip(text)))
            .unwrap_or(Value::Null),
        "site_biz_msg": data
            .and_then(|d| d.get("biz_msg"))
            .and_then(Value::as_str)
            .map(|text| json!(clip(text)))
            .unwrap_or(Value::Null),
    });

    // --- 目录段要的那几个字段 ---
    out["chat_sessions_len_at_biz_data"] = array_len(biz_data.and_then(|b| b.get("chat_sessions")));
    out["all_chat_sessions_arrays"] = session_arrays.into();
    out["objects_with_id_count"] = count_objects_with_id(&root).into();
    out["has_more"] = biz_data
        .and_then(|b| b.get("has_more"))
        .cloned()
        .unwrap_or(Value::Null);

    // --- 正文段要的那几个字段 ---
    out["messages_len_at_biz_data"] = array_len(biz_data.and_then(|b| b.get("messages")));
    // 实际用的消息数组：`data.biz_data.messages` 优先；不在那儿就退到递归找到的第一个
    // `messages`／`chat_messages` 数组（并把路径记下来 ⇒ 读数能说清「实际在哪儿」）。
    let used: Option<(String, &Vec<Value>)> = biz_data
        .and_then(|b| b.get("messages"))
        .and_then(Value::as_array)
        .map(|items| ("data.biz_data.messages".to_string(), items))
        .or_else(|| find_message_array(&root, "$", &["messages", "chat_messages"]));
    match &used {
        Some((path, items)) => {
            out["message_key_sets"] = message_key_sets(items).into();
            out["role_values"] = field_value_set(items, "role").into();
            out["type_values"] = field_value_set(items, "type").into();
            out["extra_shape_facts"] = json!({
                "messages_array_used": path,
                "messages_len_used": items.len(),
                "elements_with_role": items.iter().filter(|item| item.get("role").is_some()).count(),
                "elements_with_type": items.iter().filter(|item| item.get("type").is_some()).count(),
                "messages_arrays_found": message_arrays,
                "has_more_paths": find_scalar_paths(&root, "has_more"),
                "biz_data_object_shapes": biz_data_object_shapes(biz_data),
                "note": "extra_shape_facts 只是让「三种可能」一次分得开：只有键名与计数，没有正文"
            });
        }
        None => {
            out["message_key_sets"] = json!([]);
            out["role_values"] = json!([]);
            out["type_values"] = json!([]);
            out["extra_shape_facts"] = json!({
                "messages_array_used": Value::Null,
                "messages_len_used": Value::Null,
                "elements_with_role": 0,
                "elements_with_type": 0,
                "messages_arrays_found": message_arrays,
                "has_more_paths": find_scalar_paths(&root, "has_more"),
                "biz_data_object_shapes": biz_data_object_shapes(biz_data),
                "note": "响应体里**没有** messages / chat_messages 数组 ⇒ 消息数组根本不在响应体里"
            });
        }
    }

    out
}

/// 某个对象的**键名**（排序；不是对象就是空数组）。
fn sorted_keys(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    let mut keys: Vec<String> = map.keys().cloned().collect();
    keys.sort();
    keys
}

/// 同上，但直接给 `Value`（免得在 `json!` 里写 `.into()` 触发类型推断）。
fn sorted_keys_value(value: Option<&Value>) -> Value {
    json!(sorted_keys(value))
}

/// 某个值是不是数组（是就给长度，不是就给 `null`）。
fn array_len(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Array(items)) => json!(items.len()),
        _ => Value::Null,
    }
}

/// 递归找出**所有**叫这些名字的数组（路径 ＋ 长度）。
fn find_arrays_named(node: &Value, names: &[&str], path: &str, out: &mut Vec<Value>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                let child = format!("{path}.{key}");
                if names.contains(&key.as_str()) {
                    if let Value::Array(items) = value {
                        out.push(json!({ "path": child, "len": items.len() }));
                    }
                }
                find_arrays_named(value, names, &child, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                find_arrays_named(item, names, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

/// 递归找出**第一个**叫这些名字的数组（路径 ＋ 引用）。
fn find_message_array<'a>(
    node: &'a Value,
    path: &str,
    names: &[&str],
) -> Option<(String, &'a Vec<Value>)> {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                let child = format!("{path}.{key}");
                if names.contains(&key.as_str()) {
                    if let Value::Array(items) = value {
                        return Some((child, items));
                    }
                }
                if let Some(found) = find_message_array(value, &child, names) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                if let Some(found) = find_message_array(item, &format!("{path}[{index}]"), names) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// 递归数出**带 `id` 字段的对象**总数。
fn count_objects_with_id(node: &Value) -> usize {
    match node {
        Value::Object(map) => {
            let here = if map.contains_key("id") { 1 } else { 0 };
            here + map.values().map(count_objects_with_id).sum::<usize>()
        }
        Value::Array(items) => items.iter().map(count_objects_with_id).sum(),
        _ => 0,
    }
}

/// 每条消息的**键名集合**（不同消息的集合**去重**后列出 —— **一个值都不落**）。
fn message_key_sets(items: &[Value]) -> Vec<Value> {
    let mut sets: Vec<Vec<String>> = Vec::new();
    for item in items {
        if !item.is_object() {
            continue;
        }
        let keys = sorted_keys(Some(item));
        if !sets.contains(&keys) {
            sets.push(keys);
        }
    }
    sets.into_iter().map(|keys| json!(keys)).collect()
}

/// 某个字段在所有消息里的**取值集合**（排序、去重）。
fn field_value_set(items: &[Value], field: &str) -> Vec<String> {
    let mut values: BTreeSet<String> = BTreeSet::new();
    for item in items {
        let Some(map) = item.as_object() else {
            continue;
        };
        let Some(value) = map.get(field) else {
            continue;
        };
        let text = match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        values.insert(text);
    }
    values.into_iter().collect()
}

/// 递归找出某个键的**全部**出现位置（路径 ＋ 它的值 —— 值只可能是布尔/数字/字符串）。
fn find_scalar_paths(node: &Value, key: &str) -> Vec<Value> {
    let mut out = Vec::new();
    walk_scalar_paths(node, key, "$", &mut out);
    out
}

fn walk_scalar_paths(node: &Value, key: &str, path: &str, out: &mut Vec<Value>) {
    match node {
        Value::Object(map) => {
            for (name, value) in map {
                let child = format!("{path}.{name}");
                if name == key && !value.is_array() && !value.is_object() {
                    out.push(json!({ "path": child, "value": value.clone() }));
                }
                walk_scalar_paths(value, key, &child, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk_scalar_paths(item, key, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

/// 截断一段站点消息（**状态信息**，不是正文；限长防意外带出长串）。
fn clip(text: &str) -> String {
    const LIMIT: usize = 120;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let head: String = text.chars().take(LIMIT).collect();
    format!("{head}…（已截断，原长 {} 字符）", text.chars().count())
}

/// `data.biz_data` 里那几个**对象**的形状（**键名 + 值的类型 + 非内容标量的值**）。
///
/// 用途：正文那条响应里 `chat_session`（会话自身）与 `cache_control`（缓存控制）
/// 是什么形状 —— 它们能说明「站点为什么给一个空的消息数组」。
///
/// ⚠️ **`title` 不落盘**（会话标题，纪律不许落）⇒ ⇒ 只落**其余**标量
/// （`is_empty` / `current_message_id` / `seq_id` / `version` / 时间戳…）。
fn biz_data_object_shapes(biz_data: Option<&Value>) -> Value {
    let Some(biz_data) = biz_data else {
        return json!({});
    };
    let Some(map) = biz_data.as_object() else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    for name in ["chat_session", "cache_control"] {
        let Some(value) = map.get(name) else {
            continue;
        };
        let mut fields = serde_json::Map::new();
        let mut scalars = serde_json::Map::new();
        if let Some(inner) = value.as_object() {
            for (key, field) in inner {
                fields.insert(key.clone(), json!(json_type_of(field)));
                // **标题不许落**；其余标量（标志位、计数、时间戳）可以落。
                if key == "title" {
                    continue;
                }
                match field {
                    Value::Array(_) | Value::Object(_) => {}
                    Value::String(text) => {
                        scalars.insert(key.clone(), json!(clip(text)));
                    }
                    other => {
                        scalars.insert(key.clone(), other.clone());
                    }
                }
            }
        } else if let Value::String(text) = value {
            scalars.insert("*".to_string(), json!(clip(text)));
        }
        out.insert(
            name.to_string(),
            json!({
                "keys": sorted_keys_value(Some(value)),
                "field_types": Value::Object(fields),
                "scalar_values_except_title": Value::Object(scalars),
                "value_type": json_type_of(value),
            }),
        );
    }
    Value::Object(out)
}

/// 一个值的 JSON 类型名（**只报类型，不报值**）。
fn json_type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
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
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

/// 这个探针整条链建立在 WebView2 的 DevTools 协议上 ⇒ **Windows 专用**
/// （见文件头那段「平台边界」）⇒ ⇒ 别的平台上它**整块不存在**。
#[cfg(not(windows))]
fn main() {
    eprintln!("这个探针只在 Windows 上跑（整条链建立在 WebView2 的 DevTools 协议上）。");
}
