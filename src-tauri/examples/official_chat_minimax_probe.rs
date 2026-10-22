//! MiniMax 官网对话 · 站点取数面 **只读取证探针**（独立 example 入口；**不并进正式取数路径**）。
//!
//! # 它要回答什么（两个模式，一个探针）
//!
//! | `--site` | 问什么 | 报什么 |
//! |---|---|---|
//! | `minimax`  | 目录那条与正文那条**响应体的形状** | **键名／数组路径／计数／字节数／状态码／角色取值集合**（**全部**正文响应都要记，不许只记第一条） |
//! | `deepseek` | 目录那条响应里，**生产那份**裁剪保留下来的**确实是最近 N 条**吗（N ＝ 生产那个条数上限常量） | 站点总条数 ＋ 保留的 N 个 `updated_at` ＋ **被丢掉那批里最新的一条** ＋ 断言（保留的最小值 ≥ 被丢的最大值） |
//!
//! # 纪律
//!
//! · **只读**：不新建会话、不发消息、不点按钮、**不做页内求值**。
//! · **壳侧零请求**：只订阅与观察；唯一的 CDP 调用是 `Network.enable` 与
//!   `Network.getResponseBody`（＋每段一次的 bogus 对照）。
//! · **凭据打码**：MiniMax 的鉴权**在 query 的 `token=` 里** ⇒ ⇒ 报告里那个值
//!   **一律打码** ⇒ ⇒ 不落 token、不落标题、不落正文原文。
//! · **响应体只在进程内存里解析**：读数只带**结构**（键名／计数／类型／时间戳）
//!   ⇒ ⇒ **响应体本身不落盘**、不进仓库。
//! · **profile 只读**：浏览器只写**副本**（`--profile` 是副本落点）。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_minimax_probe -- \
//!   --site minimax \
//!   --profile-copy-from "<源 profile 目录>" \
//!   --profile "$TEMP/dsh-minimax-probe/profile" \
//!   --out "$TEMP/dsh-minimax-probe/readings.json"
//! ```
//!
//! # 为什么用 `#[path]` 把 lib 里的那份源码编进来
//!
//! `src/official_chat/session_structure.rs` 在 lib 里是 `pub(crate)`，而 example
//! 是**另一个 crate**，拿不到 ⇒ ⇒ 这里用 `#[path]` 把**同一份源文件**编进来
//! ⇒ ⇒ 探针跑的就是将来要进生产的那份裁剪（`SessionStructure::from_body` ／
//! `to_json`）与**生产那个**条数上限常量，不是副本。

#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

#[allow(dead_code)]
#[path = "../src/official_chat/cdp_eval.rs"]
mod cdp_eval;

// 生产那份「目录结构」模块：探针要跑**生产那份**裁剪 ⇒ ⇒ 同一份源文件编进来。
#[allow(dead_code)]
#[path = "../src/official_chat/session_structure.rs"]
mod session_structure;

// 共享 helper：**只读**拷贝一个浏览器 profile（源只被读，写全落在副本上）。
#[allow(dead_code)]
#[path = "support/profile_copy.rs"]
mod profile_copy;

// 生产代码里 `crate::harness::official_chat::*` / `crate::commands::AppState` /
// `crate::shell::*` 是**壳自己的**模块；example 是另一个 crate，没有它们 ⇒ ⇒
// 这里给一份**同名同形状**的最小实现，让 `#[path]` 进来的那份生产源码在探针里
// 也能编译 —— 测的仍然是生产那份代码，不是副本。
mod harness {
    pub mod official_chat {
        pub use crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS;
        // ⚠️ **ChatGPT 那个常量也要导出**（规格表里那第三站用它）—— 少了它，
        // 这份探针**编不过**（`#[path]` 进来的是**生产那份**源文件）。
        pub use crate::official_chat::OFFICIAL_CHAT_CHATGPT_URL;
        pub use crate::official_chat::OFFICIAL_CHAT_MINIMAX_URL;
        pub use crate::official_chat::OFFICIAL_CHAT_URL;
    }
}

mod commands {
    pub struct AppState {
        pub data_dir: std::path::PathBuf,
    }
}

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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cdp_eval::{cdp_call, cdp_field};

/// 订阅的 DevTools 事件（**按具体事件名** —— 官方 API 形态）。
const SUBSCRIBE_EVENTS: &[&str] = &[
    "Network.requestWillBeSent",
    "Network.responseReceived",
    "Network.loadingFinished",
];

/// 可能装着「角色」的键名（**候选集**，不是断言）—— 读到读数的人才知道该站
/// 到底用哪个键 ⇒ ⇒ 探针只报「哪些键真的出现过、各自的取值集合」。
const ROLE_KEYS: &[&str] = &[
    "role",
    "role_type",
    "roleType",
    "role_name",
    "sender",
    "sender_type",
    "author",
    "message_type",
    "msg_type",
];

/// query 里**一律打码**的键（凭据）。
const SECRET_QUERY_KEYS: &[&str] = &[
    "token",
    "access_token",
    "accesstoken",
    "refresh_token",
    "jwt",
    "auth",
    "authorization",
    "sid",
    "_token",
    "_sid",
];

/// 一个站点的「壳侧要知道的那几样」。
struct SiteSpec {
    /// 站点短码。
    key: &'static str,
    /// 站点首页（目录那条链导航到哪）。
    root_url: &'static str,
    /// 目录那条响应要盯的**路径片段**（按路径认；query 由站点自己管）。
    catalog_markers: &'static [&'static str],
    /// 正文那条响应的**路径前缀**（MiniMax 的正文路径里带会话 id）。
    body_path_prefix: Option<&'static str>,
    /// 正文那条响应的**路径后缀**。
    body_path_suffix: &'static str,
    /// 深链**模板**（`{prefix}{sessionId}`）—— 可能不止一种形态 ⇒ ⇒ 逐个试，
    /// **哪一条真的把正文端点催出来，以读数为准**（不猜）。
    deep_link_templates: &'static [&'static str],
}

impl SiteSpec {
    /// 按站点短码取那一份规格（**只有这一处**按站点分叉）。
    fn by_key(key: &str) -> Option<Self> {
        match key {
            "deepseek" => Some(Self {
                key: "deepseek",
                root_url: "https://chat.deepseek.com",
                catalog_markers: &["/api/v0/chat_session/fetch_page"],
                body_path_prefix: Some("/api/v0/chat/history_messages"),
                body_path_suffix: "",
                deep_link_templates: &["https://chat.deepseek.com/a/chat/s/"],
            }),
            "minimax" => Some(Self {
                key: "minimax",
                root_url: "https://agent.minimaxi.com",
                catalog_markers: &[
                    "/minimax-cloud/api/v1/sidebar/session/tree",
                    "/minimax-cloud/api/v1/session/search",
                ],
                body_path_prefix: Some("/minimax-cloud/api/v1/session/"),
                body_path_suffix: "/message",
                // ⚠️ **三种形态都试**（读数说话，不猜）：入口域名会 302 到
                // `agent.minimax.cn`，而路径是不是 `/` 也要看读数。
                deep_link_templates: &[
                    "https://agent.minimax.cn/?id=",
                    "https://agent.minimax.cn/mavis?id=",
                    "https://agent.minimax.io/?id=",
                ],
            }),
            _ => None,
        }
    }

    /// 这条路径命中哪一档（`catalog` / `body` / 不命中）。
    fn classify(&self, path: &str) -> Option<&'static str> {
        if self
            .catalog_markers
            .iter()
            .any(|marker| path.contains(marker))
        {
            return Some("catalog");
        }
        if let Some(prefix) = self.body_path_prefix {
            if path.starts_with(prefix) && path.ends_with(self.body_path_suffix) {
                return Some("body");
            }
        }
        None
    }
}

/// 探针从站里收到的两类消息。
enum Msg {
    /// 一条 DevTools 协议事件（事件名 ＋ 载荷原文 JSON）。
    Event { name: String, raw: String },
    /// 订阅／接线阶段的错误。
    Setup(String),
}

/// 抓到的那一条响应（**体只在内存里**）。
struct BodyRecord {
    /// 命中哪一档（`catalog` / `body`）。
    kind: &'static str,
    /// 路径（**不带 query**）。
    path: String,
    /// query（**值已打码**）。
    query: Vec<(String, String)>,
    /// HTTP 状态码。
    status: Option<i64>,
    /// 响应体字节数。
    body_bytes: usize,
    /// 解析后的响应体（**只在进程内存里**；报告里只落它的**形状**）。
    body: Option<Value>,
    /// 取体失败时的原因。
    fetch_error: Option<String>,
}

/// 事件累积状态。
#[derive(Default)]
struct State {
    total: u64,
    by_event: BTreeMap<String, u64>,
    setup_errors: Vec<String>,
    documents: Vec<String>,
    /// `requestId` → 请求 URL。
    request_urls: BTreeMap<String, String>,
    /// `requestId` → （URL、状态码）。
    responses: BTreeMap<String, (String, Option<i64>)>,
    /// 站点发出的**全部**请求（方法 ＋ 路径 ＋ query **键名**）——
    /// ⚠️ **query 的值一个都不留**（可能有凭据）。
    api_paths: Vec<Value>,
    /// 抓到体的那些响应。
    bodies: Vec<BodyRecord>,
}

/// 探针的**调节旋钮**。
struct ProbeOptions {
    observe_ms: u64,
    catalog_ms: u64,
    session_id: Option<String>,
    max_candidates: usize,
    copy_note: Value,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--profile` 必须显式给：那是**一次性临时目录下的副本**；不给就不探测。
    let Some(profile) = arg_value(&args, "--profile").map(PathBuf::from) else {
        eprintln!(
            "probe: 必须显式给 --profile <一次性临时目录下的 user-data 目录副本>；不给就不探测"
        );
        std::process::exit(2);
    };
    let Some(out) = arg_value(&args, "--out").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --out <读数文件路径>");
        std::process::exit(2);
    };
    let Some(site_key) = arg_value(&args, "--site") else {
        eprintln!("probe: 必须显式给 --site deepseek|minimax");
        std::process::exit(2);
    };
    let Some(spec) = SiteSpec::by_key(&site_key) else {
        eprintln!("probe: 不认识的 --site {site_key}（只认 deepseek|minimax）");
        std::process::exit(2);
    };
    let observe_ms: u64 = arg_value(&args, "--observe-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(45_000);
    let catalog_ms: u64 = arg_value(&args, "--catalog-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(60_000);
    let session_id = arg_value(&args, "--session-id");
    let max_candidates: usize = arg_value(&args, "--max-candidates")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(3);

    // 可选：让探针自己把**源 profile 只读拷贝**一份到 `--profile` 那个一次性目录。
    let mut copy_note = Value::Null;
    if let Some(src) = arg_value(&args, "--profile-copy-from").map(PathBuf::from) {
        match profile_copy::copy_dir_readonly(&src, &profile) {
            Ok(report) => {
                copy_note = json!({
                    "copied_files": report.files,
                    "copied_bytes": report.bytes,
                    "skipped_count": report.skipped.len(),
                    "skip_reason": report.skip_reason,
                    "note": "只读拷贝：源只被读，浏览器只写副本",
                });
                eprintln!("probe: profile 只读拷贝完成");
            }
            Err(e) => {
                eprintln!("probe: profile 只读拷贝失败：{e}");
                std::process::exit(3);
            }
        }
    }

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile = profile.clone();
            let out = out.clone();
            let session_id = session_id.clone();
            let copy_note = copy_note.clone();
            let options = ProbeOptions {
                observe_ms,
                catalog_ms,
                session_id,
                max_candidates,
                copy_note,
            };
            std::thread::Builder::new()
                .name("official-chat-site-probe".into())
                .spawn(move || {
                    let report = tauri::async_runtime::block_on(run_probe(
                        &handle, &profile, &spec, &options,
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

/// 跑整个探针，出 JSON 读数。
async fn run_probe(
    handle: &tauri::AppHandle,
    profile: &Path,
    spec: &SiteSpec,
    options: &ProbeOptions,
) -> String {
    let mut report = json!({
        "probe": "官网对话 · 站点取数面只读取证（独立 example 入口）",
        "site": spec.key,
        "started_at_utc": iso_utc(now_ms()),
        "profile_dir": profile.to_string_lossy(),
        "profile_copy": options.copy_note.clone(),
        "discipline": [
            "只读：不新建会话、不发消息、不点按钮、不做页内求值",
            "壳侧零请求：只订阅与观察，唯一 CDP 调用是 Network.enable 与 Network.getResponseBody",
            "query 里的凭据（token 等）一律打码；报告里不落 token、不落标题、不落正文原文",
            "响应体只在进程内存里解析：报告只带结构（键名／计数／类型／时间戳）",
        ],
    });

    match spec.key {
        "deepseek" => {
            report["deepseek"] = run_deepseek(handle, profile, spec, options).await;
        }
        "minimax" => {
            report["minimax"] = run_minimax(handle, profile, spec, options).await;
        }
        _ => {}
    }

    report["finished_at_utc"] = json!(iso_utc(now_ms()));
    report.to_string()
}

/// DeepSeek 那一格：**生产那份**裁剪保留下来的**确实是最近 N 条**吗。
async fn run_deepseek(
    handle: &tauri::AppHandle,
    profile: &Path,
    spec: &SiteSpec,
    options: &ProbeOptions,
) -> Value {
    let mut out = json!({ "phase": "deepseek-catalog" });

    let (window, mut events) = match arm_window(handle, profile, "site-probe-deepseek").await {
        Ok(pair) => pair,
        Err(e) => {
            out["fatal"] = json!(format!("无法建探测窗口：{e}"));
            return out;
        }
    };
    let enable = cdp_call(
        &window,
        "Network.enable",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["network_enable"] = json!({
        "ok": enable.is_ok(),
        "error": enable.as_ref().err().map(|e| format!("{e:?}")),
    });
    let control = cdp_call(
        &window,
        "Network.thisMethodDoesNotExist",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["control_bogus_cdp_method"] = json!({
        "ok": control.is_ok(),
        "note": "对照组：不存在的 CDP 方法**必须**报错 ⇒ 否则上面的 ok 不能当证据",
    });
    if enable.is_err() {
        out["fatal"] = json!("Network.enable 失败 ⇒ 这一格没跑成");
        return out;
    }

    // 导航到站点首页 ⇒ ⇒ **站点自己**发目录那条请求。
    let url: tauri::Url = spec.root_url.parse().expect("站点 URL 非法");
    out["navigate"] = json!({ "url": spec.root_url, "ok": window.navigate(url).is_ok() });

    let mut state = State::default();
    pump(
        &mut events,
        &window,
        &mut state,
        Instant::now() + Duration::from_millis(options.catalog_ms),
        spec,
    )
    .await;

    out["events"] = json!({
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "subscribe_setup_errors": state.setup_errors.clone(),
        "documents": state.documents.clone(),
    });
    out["endpoints"] = json!(state
        .bodies
        .iter()
        .map(|record| endpoint_reading(record))
        .collect::<Vec<_>>());

    // **生产那份**裁剪（`from_body` ＋ `to_json` ＋ 生产那个条数上限常量）。
    let catalog = state.bodies.iter().find(|record| record.kind == "catalog");
    match catalog {
        Some(record) => {
            let raw = record.body.clone().unwrap_or(Value::Null);
            let status = record.status.unwrap_or(0);
            let reading =
                match session_structure::SessionStructure::from_body(&raw.to_string(), status) {
                    Ok(structure) => {
                        let payload = structure.to_json();
                        let kept: Vec<f64> = payload
                            .get("items")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(|item| {
                                        item.get("updated_at").and_then(Value::as_f64)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        // **站点给的全部会话的 `updated_at`**（按数值降序）。
                        let mut all: Vec<f64> = raw
                            .get("data")
                            .and_then(|d| d.get("biz_data"))
                            .and_then(|b| b.get("chat_sessions"))
                            .and_then(Value::as_array)
                            .map(|list| {
                                list.iter()
                                    .filter_map(|item| {
                                        item.get("updated_at").and_then(Value::as_f64).or_else(
                                            || {
                                                item.get("updated_at")
                                                    .and_then(Value::as_str)
                                                    .and_then(|s| s.parse::<f64>().ok())
                                            },
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        all.sort_by(|left, right| right.total_cmp(left));
                        let min_kept = kept.iter().copied().fold(f64::INFINITY, f64::min);
                        // **被丢掉那批里最新的一条** ＝ 降序排完后的第 kept.len() 个。
                        let max_dropped = all.get(kept.len()).copied().unwrap_or(f64::NEG_INFINITY);
                        json!({
                            "catalog_item_limit": session_structure::CATALOG_ITEM_LIMIT,
                            "site_total": all.len(),
                            "site_total_from_production_parser": structure.sessions_length,
                            "kept_count": kept.len(),
                            "kept_updated_at": kept,
                            "min_kept": min_kept,
                            "max_dropped": max_dropped,
                            "is_newest": min_kept >= max_dropped,
                            "assertion": if min_kept >= max_dropped {
                                "ASSERT_PASS：保留的最小值 ≥ 被丢的最大值 ⇒ 保留的确实是最近 N 条"
                            } else {
                                "ASSERT_FAIL：保留的最小值 < 被丢的最大值"
                            },
                        })
                    }
                    Err(failure) => json!({
                        // ⚠️ `capture_code`／`capture_detail` 那份映射在 `session_body.rs`
                        // 里（它要 `cold_profile` 那一层）⇒ ⇒ 探针只报**变体名**。
                        "error": format!("{failure:?}"),
                    }),
                };
            out["newest_n_reading"] = reading;
        }
        None => {
            out["fatal"] = json!("本轮没抓到目录那条响应（站点没发？或没登录？）");
        }
    }

    let _ = window.close();
    out
}

/// MiniMax 那一格：**目录格 ＋ 正文格**，只报**形状**。
async fn run_minimax(
    handle: &tauri::AppHandle,
    profile: &Path,
    spec: &SiteSpec,
    options: &ProbeOptions,
) -> Value {
    let mut out = json!({ "phase": "minimax" });

    // ---- 目录格：站点自己发不发目录端点 ----
    let (window, mut events) = match arm_window(handle, profile, "site-probe-minimax-catalog").await
    {
        Ok(pair) => pair,
        Err(e) => {
            out["fatal"] = json!(format!("无法建探测窗口：{e}"));
            return out;
        }
    };
    let enable = cdp_call(
        &window,
        "Network.enable",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["catalog_network_enable"] = json!({ "ok": enable.is_ok() });
    if enable.is_err() {
        out["fatal"] = json!("Network.enable 失败 ⇒ 目录格没跑成");
        return out;
    }
    let url: tauri::Url = spec.root_url.parse().expect("站点 URL 非法");
    out["catalog_navigate"] = json!({ "url": spec.root_url, "ok": window.navigate(url).is_ok() });

    let mut state = State::default();
    pump(
        &mut events,
        &window,
        &mut state,
        Instant::now() + Duration::from_millis(options.catalog_ms),
        spec,
    )
    .await;
    out["catalog_events"] = json!({
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "subscribe_setup_errors": state.setup_errors.clone(),
        "documents": state.documents.clone(),
    });
    out["catalog_api_requests"] = json!(state.api_paths.clone());
    out["catalog_endpoints"] = json!(state
        .bodies
        .iter()
        .map(|record| endpoint_reading(record))
        .collect::<Vec<_>>());

    // ⚠️ **生产那份裁剪**跑一遍**真响应体**（只在进程内存里）⇒ ⇒ 证明「MiniMax
    // 的条目**映射**到协议那 6 个键」与「**最近 6 条**」这两条**真的成立**。
    // ⚠️ 报告里**只有键名、类型、条数、时间戳** ⇒ ⇒ **不落标题、不落会话 id**。
    if let Some(record) = state.bodies.iter().find(|record| record.kind == "catalog") {
        if let (Some(body), Some(status)) = (&record.body, record.status) {
            let spec = session_structure::spec_for(session_structure::MINIMAX_SITE)
                .expect("规格表里必须有 MiniMax 那一条");
            match session_structure::SessionStructure::from_body_for(
                &body.to_string(),
                status,
                spec,
            ) {
                Ok(structure) => {
                    let payload = structure.to_json();
                    let items = payload
                        .get("items")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let first_keys: Vec<String> = items
                        .first()
                        .and_then(Value::as_object)
                        .map(|map| map.keys().cloned().collect())
                        .unwrap_or_default();
                    let stamps: Vec<f64> = items
                        .iter()
                        .filter_map(|item| item.get("updated_at").and_then(Value::as_f64))
                        .collect();
                    out["production_parse"] = json!({
                        "ok": true,
                        "item_count": items.len(),
                        "catalog_item_limit": session_structure::CATALOG_ITEM_LIMIT,
                        "sessions_length": structure.sessions_length,
                        "top_keys": structure.top_keys.clone(),
                        "data_keys": structure.data_keys.clone(),
                        "code": structure.code,
                        "has_more": structure.has_more,
                        "first_item_keys": first_keys,
                        "first_item_key_count": items
                            .first()
                            .and_then(Value::as_object)
                            .map(|map| map.len())
                            .unwrap_or_default(),
                        "kept_updated_at": stamps,
                        "note": "生产那份 SessionStructure::from_body_for 跑的**真响应体**（只在内存里）；报告里只有键名／类型／条数／时间戳",
                    });
                }
                Err(failure) => {
                    out["production_parse"] = json!({
                        "ok": false,
                        "error": format!("{failure:?}"),
                    });
                }
            }
        }
    }

    // 候选会话 id：命令行给的优先，否则从目录响应体里**认**出来。
    let candidates: Vec<String> = match &options.session_id {
        Some(id) => vec![id.clone()],
        None => {
            let mut found: Vec<(String, String)> = Vec::new();
            for record in state.bodies.iter().filter(|r| r.kind == "catalog") {
                if let Some(body) = &record.body {
                    collect_session_ids(body, "$", &mut found, 0);
                }
            }
            // 去重，并**按路径可信度排序**（带 session 字样的优先）。
            let mut seen = BTreeSet::new();
            let mut unique: Vec<(String, String)> = Vec::new();
            for (path, id) in found {
                if seen.insert(id.clone()) {
                    unique.push((path, id));
                }
            }
            unique.sort_by_key(|(path, _)| {
                let lower = path.to_ascii_lowercase();
                if lower.contains("sessionid") || lower.contains("session_id") {
                    0
                } else if lower.contains("session") {
                    1
                } else {
                    2
                }
            });
            out["catalog_session_id_paths"] = json!(unique
                .iter()
                .map(|(path, id)| json!({ "json_path": path, "id": id }))
                .collect::<Vec<_>>());
            unique.into_iter().map(|(_, id)| id).collect()
        }
    };
    out["session_candidate_count"] = json!(candidates.len());

    // ---- 正文格：逐个候选深链导航 → **全部**正文响应都要记 ----
    let mut attempts: Vec<Value> = Vec::new();
    for (index, session_id) in candidates
        .iter()
        .take(options.max_candidates.max(1))
        .enumerate()
    {
        let attempt = run_minimax_deeplink(handle, profile, spec, session_id, options).await;
        let hit = attempt
            .get("body_endpoint_observed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        attempts.push(attempt);
        if hit {
            out["stopped_after_attempt"] = json!(index + 1);
            break;
        }
    }
    out["deeplink_attempts"] = json!(attempts);

    let _ = window.close();
    out
}

/// 一次 MiniMax 深链导航（**每扇窗都是新的** ⇒ 订阅必然早于站点首个请求）。
///
/// ⚠️ **深链模板可能不止一种** ⇒ ⇒ **逐个试**，哪一条真的把正文端点催出来，
/// 以读数为准（不猜）；每个模板都把**它看到的全部请求路径**记下来。
async fn run_minimax_deeplink(
    handle: &tauri::AppHandle,
    profile: &Path,
    spec: &SiteSpec,
    session_id: &str,
    options: &ProbeOptions,
) -> Value {
    let mut out = json!({ "session_id": session_id });
    let mut attempts: Vec<Value> = Vec::new();

    for template in spec.deep_link_templates {
        let attempt = run_one_deeplink(handle, profile, spec, session_id, template, options).await;
        let hit = attempt
            .get("body_endpoint_observed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        attempts.push(attempt);
        if hit {
            out["hit_template"] = json!(template);
            break;
        }
    }

    out["template_attempts"] = json!(attempts);
    let hit = attempts.iter().any(|one| {
        one.get("body_endpoint_observed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    out["body_endpoint_observed"] = json!(hit);
    out
}

/// 一次深链导航（**一个模板**）。
async fn run_one_deeplink(
    handle: &tauri::AppHandle,
    profile: &Path,
    spec: &SiteSpec,
    session_id: &str,
    template: &str,
    options: &ProbeOptions,
) -> Value {
    let mut out = json!({ "template": template });

    let (window, mut events) =
        match arm_window(handle, profile, "site-probe-minimax-deeplink").await {
            Ok(pair) => pair,
            Err(e) => {
                out["fatal"] = json!(format!("无法建探测窗口：{e}"));
                return out;
            }
        };
    let enable = cdp_call(
        &window,
        "Network.enable",
        json!({}),
        Duration::from_secs(10),
    )
    .await;
    out["network_enable"] = json!({ "ok": enable.is_ok() });
    if enable.is_err() {
        out["fatal"] = json!("Network.enable 失败 ⇒ 这一格没跑成");
        return out;
    }

    let deep_link = format!("{template}{session_id}");
    out["deep_link"] = json!(deep_link);
    let url: tauri::Url = deep_link.parse().expect("深链 URL 非法");
    out["navigate"] = json!({ "ok": window.navigate(url).is_ok() });

    let mut state = State::default();
    pump(
        &mut events,
        &window,
        &mut state,
        Instant::now() + Duration::from_millis(options.observe_ms),
        spec,
    )
    .await;

    out["events"] = json!({
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "subscribe_setup_errors": state.setup_errors.clone(),
        "documents": state.documents.clone(),
    });
    // ⚠️ **站点自己发的全部请求**（query 只留键名）—— 用来认「正文那条到底
    // 是什么路径／什么参数名」（**读数说话，不猜**）。
    out["api_requests"] = json!(state.api_paths.clone());
    // ⚠️ **一次深链会发好几条正文请求** ⇒ ⇒ 这里**全部**都记下来（不许只记第一条）。
    let body_records: Vec<&BodyRecord> = state.bodies.iter().filter(|r| r.kind == "body").collect();
    out["body_endpoint_observed"] = json!(!body_records.is_empty());
    out["body_request_count"] = json!(body_records.len());
    out["body_endpoints"] = json!(body_records
        .iter()
        .map(|record| endpoint_reading(record))
        .collect::<Vec<_>>());

    let _ = window.close();
    out
}

/// 一条端点响应的读数（**只有结构，没有内容**）。
fn endpoint_reading(record: &BodyRecord) -> Value {
    let mut out = json!({
        "kind": record.kind,
        "path": record.path,
        "query": record.query.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
        "status": record.status,
        "body_bytes": record.body_bytes,
        "fetch_error": record.fetch_error,
    });
    if let Some(body) = &record.body {
        out["shape"] = shape_of(body, 6);
        let mut arrays: Vec<Value> = Vec::new();
        collect_arrays(body, "$", 0, &mut arrays);
        out["arrays"] = json!(arrays);
        let mut roles: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        collect_role_values(body, "$", 0, &mut roles);
        out["role_like_keys"] = json!(roles
            .iter()
            .map(|(path, values)| json!({
                "json_path": path,
                "values": values.iter().cloned().collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>());
        // ⚠️ **顶层那几个标量**（**只取布尔与数字** ⇒ 不可能是正文）—— `has_more`
        // 那个布尔是「**取全没取全**」的判据（协议 §9 G4-23）⇒ ⇒ 必须读到**值**；
        // `base_resp.status_code` 是站内码。
        let mut scalars: serde_json::Map<String, Value> = serde_json::Map::new();
        if let Some(map) = body.as_object() {
            for (key, value) in map {
                if matches!(value, Value::Bool(_) | Value::Number(_)) {
                    scalars.insert(key.clone(), value.clone());
                }
            }
            if let Some(base) = map.get("base_resp").and_then(Value::as_object) {
                for (key, value) in base {
                    if matches!(value, Value::Bool(_) | Value::Number(_)) {
                        scalars.insert(format!("base_resp.{key}"), value.clone());
                    }
                }
            }
        }
        if !scalars.is_empty() {
            out["top_scalars"] = Value::Object(scalars);
        }
        // ⚠️ **时间戳的值**要读出来（**数字，不是内容**）—— 协议 §4.3 ② 那 6 个键
        // 里的 `updated_at` 是**秒级小数时间戳** ⇒ ⇒ 得看清站点给的是秒还是毫秒。
        let mut stamps: Vec<f64> = Vec::new();
        collect_numeric_field(body, "updated_at", 0, &mut stamps);
        if !stamps.is_empty() {
            stamps.sort_by(|left, right| right.total_cmp(left));
            out["updated_at_values"] = json!(stamps);
        }
    }
    out
}

/// 收集某个**数字**字段的全部取值（**只取数字** ⇒ 不可能是正文）。
fn collect_numeric_field(value: &Value, field: &str, depth: usize, out: &mut Vec<f64>) {
    if depth > 8 || out.len() > 500 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == field {
                    if let Some(number) = child.as_f64() {
                        out.push(number);
                    }
                }
                collect_numeric_field(child, field, depth + 1, out);
            }
        }
        Value::Array(list) => {
            for child in list.iter().take(200) {
                collect_numeric_field(child, field, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// 建窗 ＋ 订阅 ＋ `Network.enable`（**三件事都在导航之前**）。
async fn arm_window(
    handle: &tauri::AppHandle,
    profile: &Path,
    label: &str,
) -> Result<
    (
        tauri::WebviewWindow,
        tokio::sync::mpsc::UnboundedReceiver<Msg>,
    ),
    String,
> {
    let window = build_window(handle, profile, label)?;
    let events = subscribe_devtools_events(&window);
    cdp_call(
        &window,
        "Network.enable",
        json!({}),
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| format!("Network.enable 失败：{e:?}"))?;
    Ok((window, events))
}

/// 建窗 label 的**序号**（Tauri 的 label **撞名就建窗失败** ⇒ ⇒ 每扇窗一个唯一 label）。
static WINDOW_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    // ⚠️ **label 必须每扇窗唯一**（Tauri 撞名就建窗失败 ⇒ ⇒ 上一扇还没销毁时
    // 下一扇就撞上了）。
    let unique = format!(
        "{label}-{}",
        WINDOW_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    tauri::WebviewWindowBuilder::new(handle, &unique, tauri::WebviewUrl::External(url))
        .title("官网对话取数面取证探针")
        .inner_size(1000.0, 760.0)
        .visible(false)
        .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .data_directory(profile.to_path_buf())
        .build()
        .map_err(|e| format!("无法创建探针窗口：{e}"))
}

/// 按事件名订阅 DevTools 协议事件（官方 API 形态）。
fn subscribe_devtools_events(
    window: &tauri::WebviewWindow,
) -> tokio::sync::mpsc::UnboundedReceiver<Msg> {
    use webview2_com::DevToolsProtocolEventReceivedEventHandler;
    use windows_core::HSTRING;

    // ⚠️ **必须用无界通道**：事件处理函数是**同步回调** ⇒ ⇒ 这里的 `send` 必须是
    // **同步**的（`tauri::async_runtime::Sender::send` 是 async ⇒ 在同步回调里
    // 那个 future 会被丢掉 ⇒ ⇒ 一个事件都收不到）。
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
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

/// 抽事件，**命中端点且加载完 ⇒ ⇒ 立刻取体**（硬约束 1：取晚了会拿不到）。
async fn pump(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<Msg>,
    window: &tauri::WebviewWindow,
    state: &mut State,
    deadline: Instant,
    spec: &SiteSpec,
) {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        let msg = match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Some(msg)) => msg,
            Ok(None) => return,
            Err(_) => return,
        };
        match msg {
            Msg::Setup(detail) => state.setup_errors.push(detail),
            Msg::Event { name, raw } => {
                state.total += 1;
                *state.by_event.entry(name.clone()).or_insert(0) += 1;
                let Ok(value) = serde_json::from_str::<Value>(&raw) else {
                    continue;
                };
                let request_id = value
                    .get("requestId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if request_id.is_empty() {
                    continue;
                }
                match name.as_str() {
                    "Network.requestWillBeSent" => {
                        let url = value
                            .get("request")
                            .and_then(|r| r.get("url"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let method = value
                            .get("request")
                            .and_then(|r| r.get("method"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let resource_type = value
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if resource_type == "Document" && state.documents.len() < 16 {
                            state.documents.push(url.clone());
                        }
                        // ⚠️ **query 只留键名**（值一律不留 ⇒ 凭据不可能外泄）。
                        if state.api_paths.len() < 400 {
                            state.api_paths.push(json!({
                                "method": method,
                                "path": url_path(&url),
                                "query_keys": url_query(&url).into_iter().map(|(k, _)| k).collect::<Vec<_>>(),
                            }));
                        }
                        state.request_urls.insert(request_id.clone(), url);
                    }
                    "Network.responseReceived" => {
                        let response = value.get("response");
                        let url = response
                            .and_then(|r| r.get("url"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let status = response
                            .and_then(|r| r.get("status"))
                            .and_then(Value::as_i64);
                        state.responses.insert(request_id.clone(), (url, status));
                    }
                    "Network.loadingFinished" => {
                        // 这条加载完了 ⇒ ⇒ 若它命中端点 ⇒ **立刻**取体。
                        let Some((url, status)) = state.responses.get(&request_id).cloned() else {
                            continue;
                        };
                        let path = url_path(&url);
                        let Some(kind) = spec.classify(&path) else {
                            continue;
                        };
                        let fetched = cdp_call(
                            window,
                            "Network.getResponseBody",
                            json!({ "requestId": request_id }),
                            Duration::from_secs(20),
                        )
                        .await;
                        let (body_text, fetch_error) = match fetched {
                            Ok(envelope) => {
                                let text = cdp_field(&envelope, "body")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                (Some(text), None)
                            }
                            Err(e) => (None, Some(format!("{e:?}"))),
                        };
                        let body = body_text
                            .as_ref()
                            .and_then(|text| serde_json::from_str::<Value>(text).ok());
                        state.bodies.push(BodyRecord {
                            kind,
                            path: path.clone(),
                            query: url_query(&url),
                            status,
                            body_bytes: body_text.as_ref().map(|text| text.len()).unwrap_or(0),
                            body,
                            fetch_error,
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 一份 JSON 的**形状**（键名／类型／数组长度；**值一个字都不带**）。
fn shape_of(value: &Value, depth: usize) -> Value {
    if depth == 0 {
        return json!("…");
    }
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                out.insert(key.clone(), shape_of(child, depth - 1));
            }
            Value::Object(out)
        }
        Value::Array(list) => json!({
            "__len": list.len(),
            "__first": list.first().map(|one| shape_of(one, depth - 1)),
        }),
        Value::String(_) => json!("string"),
        Value::Number(_) => json!("number"),
        Value::Bool(_) => json!("bool"),
        Value::Null => json!("null"),
    }
}

/// 列出所有**数组的路径与长度**（**只进数组的第一个元素** ⇒ 不炸）。
fn collect_arrays(value: &Value, path: &str, depth: usize, out: &mut Vec<Value>) {
    if depth > 6 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                if let Value::Array(list) = child {
                    out.push(json!({
                        "path": child_path,
                        "len": list.len(),
                        "first_keys": list
                            .first()
                            .and_then(|one| one.as_object())
                            .map(|obj| obj.keys().cloned().collect::<Vec<String>>()),
                    }));
                }
                collect_arrays(child, &child_path, depth + 1, out);
            }
        }
        Value::Array(list) => {
            if let Some(first) = list.first() {
                collect_arrays(first, &format!("{path}[0]"), depth + 1, out);
            }
        }
        _ => {}
    }
}

/// 收集「像角色的键」的取值集合（**候选键名，不猜**）。
fn collect_role_values(
    value: &Value,
    path: &str,
    depth: usize,
    out: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                if ROLE_KEYS.contains(&key.as_str()) {
                    if let Some(text) = child.as_str() {
                        if text.chars().count() <= 32 {
                            out.entry(child_path.clone())
                                .or_default()
                                .insert(text.to_string());
                        }
                    }
                }
                collect_role_values(child, &child_path, depth + 1, out);
            }
        }
        Value::Array(list) => {
            for (index, child) in list.iter().enumerate().take(200) {
                collect_role_values(child, &format!("{path}[{index}]"), depth + 1, out);
            }
        }
        _ => {}
    }
}

/// 从目录响应体里**认**出候选会话 id（**只报 id 与它的 JSON 路径**）。
fn collect_session_ids(value: &Value, path: &str, out: &mut Vec<(String, String)>, depth: usize) {
    if depth > 8 || out.len() > 400 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                let lower = key.to_ascii_lowercase();
                if lower == "id" || lower == "sessionid" || lower == "session_id" {
                    if let Some(text) = child.as_str() {
                        if looks_like_id(text) {
                            out.push((child_path.clone(), text.to_string()));
                        }
                    }
                }
                collect_session_ids(child, &child_path, out, depth + 1);
            }
        }
        Value::Array(list) => {
            for (index, child) in list.iter().enumerate().take(200) {
                collect_session_ids(child, &format!("{path}[{index}]"), out, depth + 1);
            }
        }
        _ => {}
    }
}

/// 像不像一个 id（**纯 ASCII、无空格、长度适中**）。
fn looks_like_id(text: &str) -> bool {
    let length = text.chars().count();
    (8..=128).contains(&length) && text.chars().all(|c| c.is_ascii() && !c.is_whitespace())
}

/// 取 URL 的路径部分。
fn url_path(url: &str) -> String {
    let without_query = url.split('?').next().unwrap_or(url);
    let without_scheme = match without_query.find("://") {
        Some(index) => match without_query[index + 3..].find('/') {
            Some(slash) => &without_query[index + 3 + slash..],
            None => "/",
        },
        None => without_query,
    };
    without_scheme.to_string()
}

/// 取 URL 的 query（**键名全留；凭据的值一律打码**）。
fn url_query(url: &str) -> Vec<(String, String)> {
    let Some(index) = url.find('?') else {
        return Vec::new();
    };
    let query = &url[index + 1..];
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.find('=') {
            Some(split) => (&pair[..split], &pair[split + 1..]),
            None => (pair, ""),
        };
        let lower = key.to_ascii_lowercase();
        let masked = if SECRET_QUERY_KEYS.contains(&lower.as_str()) || value.chars().count() > 64 {
            "***".to_string()
        } else {
            value.to_string()
        };
        out.push((key.to_string(), masked));
    }
    out
}

/// 现在的时刻（epoch 毫秒）。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// 现在的时刻（UTC ISO）。
fn iso_utc(ms: u64) -> String {
    let seconds = (ms / 1000) as i64;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    match time::OffsetDateTime::from_unix_timestamp(seconds) {
        Ok(datetime) => datetime
            .format(&time::format_description::well_known::Rfc3339)
            .map(|text| format!("{text}.{nanos:09}"))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}
