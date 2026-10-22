//! ChatGPT（chatgpt.com）取数面 · 运行时取证探针（**只读取证**；独立 example 入口）。
//!
//! # 它要回答什么（两段，各一格）
//!
//! 1. **挑战重测**（最高优先级）：`https://chatgpt.com/` 与
//!    `https://chat.openai.com/` **各导航一次** ⇒ ⇒ 报：落地 URL ｜ 标题 ｜
//!    页面里有没有 Cloudflare 挑战特征 ｜ **HTTP 状态码** ｜ body 字节数 ｜
//!    站点**自己**发了哪些 `/backend-api/*` 请求 ｜ 那些请求的状态码。
//! 2. **运行时取数面**（**只在第 1 段显示没被挡时做**）：深链
//!    `https://chatgpt.com/c/<sessionId>` ⇒ ⇒ 站点自己发的**每一条** API 请求
//!    （路径 ＋ query 键名 ＋ 状态码 ＋ body 字节数）＋ 响应体的**形状**
//!    （只有键名／路径／计数／字节数，**一个值都不落**）。
//!
//! # 取数路线（**不可改**）
//!
//! **壳侧一个请求都不发**：建一扇**自己家的不可见窗**（`about:blank` 起、
//! `visible(false)`）⇒ **按具体事件名**订阅 DevTools `Network` 域 ⇒
//! **`Network.enable`** ⇒ ⇒ **然后**才导航 ⇒ ⇒ 等**站点自己**的请求，壳侧
//! 订阅抓响应。CDP 调用只有 `Network.enable`、`Network.getResponseBody`，外加
//! **每段一次**的 bogus 对照 ⇒ ⇒ **没有页内 fetch、没有注入脚本、没有 reload**。
//!
//! # 与正式取数链的关系（**独立，不并入**）
//!
//! * 本文件是 **`src-tauri/examples/` 下的独立探针入口** ⇒ 不在
//!   `src/official_chat/` 任何一条正式取数流程里、不改那条链的任何一行、
//!   也不给正式链加任何入口。
//! * 复用**生产那份** `cdp_eval.rs`（`#[path]` 编进来 ⇒ 测的是将来进生产的
//!   同一份代码）与生产那份 `OFFICIAL_CHAT_BROWSER_ARGS`、生产那份 user-data
//!   目录的**形态**（`data_directory` 给一份**副本**）。
//!
//! # 纪律
//!
//! - **只读**：不新建会话、不发消息、不点按钮。
//! - **不绕过挑战**：不换 UA、不改指纹、不走代理、不重试轰炸 ⇒ ⇒ 只**如实观测**
//!   它挡没挡。
//! - **不落盘敏感物**：读数里只有**键名、计数、字节数、状态码、URL 路径**与
//!   **打码后**的 query 键名 ⇒ ⇒ **没有正文原文、没有会话标题、没有
//!   token／cookie／凭据**（会话 id 只留打码形态）。
//! - **profile 由命令行显式给**（仓库硬不变量 `scripts/lib/probe-profile-isolation.mjs`：
//!   源码里不许出现产品 profile 目录名、不许把真实家目录与 profile 标记写在
//!   一行）⇒ 给的是**一次性临时目录下的副本**，浏览器不碰用户真实数据目录。
//! - **防假绿**：**每段各跑一次** bogus CDP 方法对照（不存在的 CDP 方法必须
//!   报错）⇒ 否则「`Network.enable` 成功」不能当证据。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --example official_chat_chatgpt_probe -- \
//!   --profile "<一次性临时目录下的 user-data 目录副本>" \
//!   [--profile-src "<只读源 profile 目录>"] \
//!   --out-dir "<读数目录>" \
//!   [--session-id "<已知会话 id>"] \
//!   [--phase1-ms 40000] [--phase2-ms 60000]
//! ```
//!
//! ⚠️ **必须给副本**：WebView2 会**独占** user-data 目录 ⇒ ⇒ 直接指向源目录
//! 会被上一轮的浏览器锁住、且浏览器会往用户真实档案里写 Cookies ⇒ ⇒ 给
//! `--profile-src` 时探针**自己**用仓库现成的**只读**拷贝器
//! （`examples/support/profile_copy.rs`）先拷一份再用（源只被读、绝不写、绝不删）。
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

// 生产那份「目录结构」与「正文」模块：探针要跑**生产那份**裁剪 ⇒ ⇒ 同一份
// 源文件编进来（`pub(crate)` 跨 crate 拿不到 ⇒ ⇒ 用 `#[path]`）。
#[allow(dead_code)]
#[path = "../src/official_chat/session_structure.rs"]
mod session_structure;

#[allow(dead_code)]
#[path = "../src/official_chat/session_body.rs"]
mod session_body;

/// 仓库现成的**只读** profile 拷贝器（`examples/support/profile_copy.rs`）：
/// 源只读、写到一次性临时目录、源被独占时跳过并记账。
#[allow(dead_code)]
#[path = "support/profile_copy.rs"]
mod profile_copy;

// 生产代码里 `crate::harness::official_chat::*` / `crate::commands::AppState` /
// `crate::shell::*` / `crate::cold_profile::*` 是**壳自己的**模块；example 是
// 另一个 crate，没有它们 ⇒ ⇒ 这里给一份**同名同形状**的最小实现，让
// `#[path]` 进来的那份生产源码在探针里也能编译 —— 测的仍然是生产那份代码，
// 不是副本。
mod harness {
    pub mod official_chat {
        pub use crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS;
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

#[allow(dead_code)]
mod cold_profile {
    use std::path::{Path, PathBuf};

    /// 生产那份冷档案在探针里**用不到**（探针自己管副本）⇒ ⇒ 只给**同名同形状**
    /// 的最小实现，让生产源码能编进来。
    pub struct ColdProfile {
        pub dir: PathBuf,
    }

    impl ColdProfile {
        pub fn prepare(_source: &Path) -> Result<Self, String> {
            Err("探针不用冷档案（它自己管副本）".to_string())
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cdp_eval::{cdp_call, cdp_field};

/// 挑战重测的两个落点（**各导航一次**，不重试、不轰炸）。
const CHALLENGE_TARGETS: &[&str] = &["https://chatgpt.com/", "https://chat.openai.com/"];

/// 深链前缀（**sessionId 在 path 里**）。
const DEEP_LINK_PREFIX: &str = "https://chatgpt.com/c/";

/// 静态取证的**候选**端点——**只做对照**，判定一律以实测路径为准。
const STATIC_CATALOG_CANDIDATE: &str = "/backend-api/conversations";

/// 站点自己的 API 前缀（ChatGPT 的 backend-api）。
const BACKEND_API_PREFIX: &str = "/backend-api/";

/// **query 的值**允许被记下来的键名（**白名单**：其余键**连值都不许落**）。
///
/// ⚠️ **为什么这几个键的值可以落**：它们全是**站点自己的分页/取数参数**
/// （条数、偏移、排序、取几轮）⇒ ⇒ **不是凭据、不是内容** ⇒ ⇒ 而生产那条链
/// 「命中多条时取哪一条」正是按 `num_turns` 判的 ⇒ ⇒ **不记它就判不了**。
const QUERY_VALUE_KEYS: &[&str] = &[
    "num_turns",
    "limit",
    "offset",
    "order",
    "include_has_versions",
    "is_archived",
    "is_starred",
    "expand",
];

/// 抄副本时**从副本里**摘掉的缓存目录名（让请求真的走网络，而不是吃缓存）。
///
/// 摘的是**副本**，源一个字节都不动 ⇒ ⇒ 这不是「绕过挑战」，是**防止假绿**：
/// 缓存命中时根本不会回源，看到的就不是站点当下的真实响应。
const CACHE_DIR_NAMES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "ShaderCache",
    "GrShaderCache",
    "blob_storage",
    "Service Worker",
];

/// 响应头里**允许记值**的白名单（其余头**只记名字**；`set-cookie` 连名字都不记）。
const HEADER_SIGNAL_NAMES: &[&str] = &[
    "content-type",
    "server",
    "cf-mitigated",
    "cf-cache-status",
    "cf-ray",
    "location",
    "content-length",
    "x-frame-options",
];

/// Cloudflare 挑战的**强特征**（命中任意一个即「页面里就是挑战」）。
const CF_STRONG_MARKERS: &[(&str, &str)] = &[
    ("just_a_moment", "Just a moment"),
    ("checking_your_browser", "Checking your browser"),
    ("enable_js_and_cookies", "Enable JavaScript and cookies to continue"),
    ("challenge_platform", "challenge-platform"),
    ("cdn_cgi_challenge", "/cdn-cgi/challenge-platform"),
    ("cf_chl", "cf_chl"),
    ("cf_chl_opt", "__cf_chl"),
    ("cf_error", "cf-error"),
    ("attention_required", "Attention Required"),
    ("ray_id", "Ray ID"),
    ("verify_you_are_human", "Verify you are human"),
    ("disable_cookies", "cookies are disabled"),
];

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
    let Some(out_dir) = arg_value(&args, "--out-dir").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --out-dir <读数目录>");
        std::process::exit(2);
    };
    // 只读源 profile 目录（可选）：探针**自己**先拷一份再用。
    let profile_src = arg_value(&args, "--profile-src").map(PathBuf::from);
    // 已知会话 id（不给就自己从目录响应体里解析一个）。
    let session_id = arg_value(&args, "--session-id");
    let phase1_ms: u64 = arg_value(&args, "--phase1-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(40_000);
    let phase2_ms: u64 = arg_value(&args, "--phase2-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(60_000);

    // --- 拷贝 profile（源只读）---
    //
    // **必须在建 app 之前做完**：WebView2 会**独占** user-data 目录 ⇒ ⇒ 浏览器
    // 启动前副本就得在位。
    let mut copy_report = json!({ "copied": false });
    let mut removed_cache_dirs: Vec<String> = Vec::new();
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
                // 摘掉**副本**里的缓存 ⇒ 让这一轮真的回源（防假绿）。
                removed_cache_dirs = remove_cache_dirs(&profile);
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
            let session_id = session_id.clone();
            let copy_report = copy_report.clone();
            let removed_cache_dirs = removed_cache_dirs.clone();

            // **保活窗**：Tauri 的事件循环在**一扇窗口都没有**时会立刻退出 ⇒ ⇒
            // 探针自己的窗是探针线程建的（慢一点就输给这个竞态）⇒ ⇒ 先建一扇
            // `about:blank` 的不可见窗把事件循环**按住**（它不导航、不发任何请求）。
            if let Err(e) = build_window(&handle, &profile, "chatgpt-keepalive") {
                eprintln!("probe: 保活窗建不起来：{e}");
            }

            std::thread::Builder::new()
                .name("official-chat-chatgpt-probe".into())
                .spawn(move || {
                    let report = tauri::async_runtime::block_on(run_probe(
                        &handle,
                        &profile,
                        session_id.as_deref(),
                        phase1_ms,
                        phase2_ms,
                        copy_report.clone(),
                        removed_cache_dirs.clone(),
                    ));

                    let _ = std::fs::create_dir_all(&out_dir);
                    let _ = std::fs::write(
                        out_dir.join("chatgpt-reading.json"),
                        format!(
                            "{}\n",
                            serde_json::to_string_pretty(&report).unwrap_or_default()
                        ),
                    );
                    if let Some(phase1) = report.get("phase1_challenge") {
                        let _ = std::fs::write(
                            out_dir.join("chatgpt-challenge.json"),
                            format!(
                                "{}\n",
                                serde_json::to_string_pretty(phase1).unwrap_or_default()
                            ),
                        );
                    }
                    if let Some(phase2) = report.get("phase2_deeplink") {
                        let _ = std::fs::write(
                            out_dir.join("chatgpt-deeplink.json"),
                            format!(
                                "{}\n",
                                serde_json::to_string_pretty(phase2).unwrap_or_default()
                            ),
                        );
                    }
                    println!("PROBE_REPORT_BEGIN");
                    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
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
    at_ms: u64,
}

/// 一条**响应**（站点自己发出的请求的回程）。
#[derive(Clone)]
struct Hit {
    request_id: String,
    method: String,
    origin: String,
    path: String,
    /// query 的**键名**（值一律不落）。
    query_keys: Vec<String>,
    /// query 里**白名单键**的**值**（分页/取数参数；**不是内容、不是凭据**）。
    query_values: BTreeMap<String, String>,
    status: i64,
    resource_type: String,
    from_disk_cache: bool,
    from_service_worker: bool,
    at_ms: u64,
    /// 响应体字节数（**取体成功**才有）。
    body_bytes: Option<usize>,
    /// 取体失败的原因（**不落 body 原文**）。
    body_error: Option<String>,
    /// 响应体的**形状**（**只有键名与计数**）。
    body_shape: Option<Value>,
    /// **生产那份**解析器跑这条真响应体的结果（**只留条数／键名／计数**）。
    production_parse: Option<Value>,
    /// 从这条响应体里解析出的会话 id（**只在内存里**用，不进读数）。
    derived_session_id: Option<String>,
}

impl Hit {
    fn to_json(&self, with_shape: bool) -> Value {
        let mut out = json!({
            "method": self.method,
            "origin": self.origin,
            "path": self.path,
            "query_keys": self.query_keys,
            "query_values": self.query_values,
            "status": self.status,
            "resource_type": self.resource_type,
            "from_disk_cache": self.from_disk_cache,
            "from_service_worker": self.from_service_worker,
            "timestamp_utc": iso_utc(self.at_ms),
            "body_bytes": self.body_bytes,
            "body_error": self.body_error,
        });
        if with_shape {
            out["body_shape"] = match &self.body_shape {
                Some(shape) => shape.clone(),
                None => json!({ "note": "这一条没取到响应体 ⇒ 没有形状读数" }),
            };
            out["production_parse"] = match &self.production_parse {
                Some(parsed) => parsed.clone(),
                None => json!({ "note": "这一条不是取数端点（或没取到体）⇒ 没有生产裁剪读数" }),
            };
        }
        out
    }
}

/// 一条**文档**（主框架）的回程——挑战重测的主证据。
#[derive(Clone)]
struct DocEntry {
    request_id: String,
    /// 这一跳**最初**请求的 URL（打码）。
    requested_url: String,
    /// 最终落地的 URL（打码）——重定向后就是目标 URL。
    url: String,
    origin: String,
    path: String,
    query_keys: Vec<String>,
    status: Option<i64>,
    mime_type: Option<String>,
    from_disk_cache: bool,
    from_service_worker: bool,
    /// 响应头的**名字**（`set-cookie` 连名字都不记）。
    header_names: Vec<String>,
    /// 白名单头的**值**。
    header_signals: Value,
    /// 上一次跳过来的那一步（状态码 ＋ 目标）。
    redirect: Option<Value>,
    at_ms: u64,
    body_bytes: Option<usize>,
    body_error: Option<String>,
    /// 文档 `<title>`（**只取首页那一段**；深链段只记长度，避免落会话标题）。
    title: Option<String>,
    /// 文档里有没有 Cloudflare 挑战特征。
    markers: Option<Value>,
}

impl DocEntry {
    fn to_json(&self, with_title: bool, main_frame: bool) -> Value {
        let mut out = json!({
            "requested_url_masked": self.requested_url,
            "url_masked": self.url,
            "origin": self.origin,
            "path": self.path,
            "query_keys": self.query_keys,
            "main_frame": main_frame,
            "status": self.status,
            "mime_type": self.mime_type,
            "from_disk_cache": self.from_disk_cache,
            "from_service_worker": self.from_service_worker,
            "header_names": self.header_names,
            "header_signals": self.header_signals,
            "redirect": self.redirect,
            "timestamp_utc": iso_utc(self.at_ms),
            "body_bytes": self.body_bytes,
            "body_error": self.body_error,
        });
        if with_title {
            out["title"] = match &self.title {
                Some(title) => json!(title),
                None => Value::Null,
            };
        } else {
            out["title_present"] = json!(self.title.is_some());
            out["title_len"] = json!(self.title.as_ref().map(|t| t.chars().count()).unwrap_or(0));
        }
        out["markers"] = match &self.markers {
            Some(markers) => markers.clone(),
            None => Value::Null,
        };
        out
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
    /// 请求元信息（`requestId` ⇒ 方法/类型/时刻）。
    req_meta: BTreeMap<String, ReqMeta>,
    /// 站点自己发出的 API 请求回程（XHR/Fetch 与 `/backend-api/*`）。
    hits: Vec<Hit>,
    /// 文档链（一跳一个条目）。
    documents: Vec<DocEntry>,
    /// 其它请求的路径（静态资源等）——只记路径，用来证明「浏览器确实在干活」。
    other_paths: Vec<String>,
}

/// 跑整个探针，出 JSON 读数。
async fn run_probe(
    handle: &tauri::AppHandle,
    profile: &Path,
    session_id: Option<&str>,
    phase1_ms: u64,
    phase2_ms: u64,
    copy_report: Value,
    removed_cache_dirs: Vec<String>,
) -> Value {
    let started = now_ms();
    let discipline = vec![
        "只读：不新建会话、不发消息、不点按钮",
        "壳侧零请求：CDP 调用只有 Network.enable、Network.getResponseBody（＋每段一次的 bogus 对照）",
        "没有页内 fetch、没有注入脚本、没有 reload",
        "不绕过挑战：不换 UA、不改指纹、不走代理、不重试轰炸",
        "报告里只有键名、计数、字节数、状态码、URL 路径与打码后的 query 键名：没有正文原文、没有会话标题、没有 token／cookie／凭据",
        "profile 由命令行显式给（一次性临时目录下的副本）⇒ 不碰用户真实数据目录",
        "全程不开 DevTools 窗口",
    ];
    let path_kind =
        "壳内路径：壳的 WebView2 ＋ 生产那份 cdp_eval.rs，但跑在**探针 example 进程**里，不是 dev 壳 GUI 进程";

    let mut report = json!({
        "probe": "ChatGPT 取数面 · 运行时取证（挑战重测 ＋ 取数面）",
        "path_kind": path_kind,
        "discipline": discipline,
        "started_at_utc": iso_utc(started),
        "profile_dir": profile.to_string_lossy(),
        "profile_copy": copy_report,
        "cache_dirs_removed_in_copy": removed_cache_dirs,
    });

    // --- 第 1 段：挑战重测（两个落点各一次） ---
    let mut targets: Vec<Value> = Vec::new();
    let mut derived_session: Option<String> = None;
    for target in CHALLENGE_TARGETS {
        let outcome = run_challenge_phase(handle, profile, target, phase1_ms).await;
        if derived_session.is_none() {
            derived_session = outcome.session_id.clone();
        }
        targets.push(outcome.reading);
    }
    let chatgpt_blocked = targets
        .iter()
        .find(|entry| {
            entry
                .get("target")
                .and_then(Value::as_str)
                .map(|t| t.starts_with("https://chatgpt.com"))
                .unwrap_or(false)
        })
        .and_then(|entry| entry.get("challenge_detected").and_then(Value::as_bool))
        .unwrap_or(true);
    let blocked_any = targets
        .iter()
        .any(|entry| entry.get("challenge_detected").and_then(Value::as_bool) == Some(true));

    report["phase1_challenge"] = json!({
        "phase": "challenge",
        "targets": targets,
        "chatgpt_root_blocked": chatgpt_blocked,
        "blocked_any_target": blocked_any,
        "rule": "challenge_detected = 文档里命中任一强特征 OR 响应头 cf-mitigated=challenge OR (5xx/403 且文档体像挑战页)",
    });

    // --- 第 2 段：运行时取数面（**只在没被挡时做**） ---
    let chosen: Option<String> = session_id
        .map(|id| id.to_string())
        .or(derived_session);
    if chatgpt_blocked {
        report["phase2_deeplink"] = json!({
            "phase": "deeplink",
            "skipped": true,
            "reason": "chatgpt.com 落地页就是 Cloudflare 挑战 ⇒ 取数面未验（不绕过挑战）"
        });
    } else if let Some(session_id) = chosen {
        let phase2 = run_deeplink_phase(handle, profile, &session_id, phase2_ms).await;
        report["phase2_deeplink"] = phase2;
    } else {
        report["phase2_deeplink"] = json!({
            "phase": "deeplink",
            "skipped": true,
            "reason": "没拿到任何真实会话 id（目录响应体里没解析出 id）⇒ 深链那一段没法跑"
        });
    }

    report["finished_at_utc"] = json!(iso_utc(now_ms()));
    report
}

/// 第 1 段的结果：读数 ＋ 从目录响应体里解析出的会话 id（**只在内存里**）。
struct Phase1Outcome {
    reading: Value,
    session_id: Option<String>,
}

/// 第 1 段：导航到一个落点 → 观测挑战特征 ＋ 站点自己发的 API 请求。
async fn run_challenge_phase(
    handle: &tauri::AppHandle,
    profile: &Path,
    target: &str,
    phase1_ms: u64,
) -> Phase1Outcome {
    let label = format!("chatgpt-p1-{}", host_slug(target));
    let mut out = json!({
        "phase": "challenge",
        "target": target,
        "window_label": label,
    });

    let window = match build_window(handle, profile, &label) {
        Ok(window) => window,
        Err(e) => {
            out["fatal"] = json!(format!("无法创建探测窗口：{e}"));
            return Phase1Outcome {
                reading: out,
                session_id: None,
            };
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
        return Phase1Outcome {
            reading: out,
            session_id: None,
        };
    }

    let url: tauri::Url = match target.parse() {
        Ok(url) => url,
        Err(e) => {
            out["fatal"] = json!(format!("导航目标非法：{e}"));
            return Phase1Outcome {
                reading: out,
                session_id: None,
            };
        }
    };
    out["navigate"] = match window.navigate(url) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };

    let mut state = EventState::default();
    let deadline = Instant::now() + Duration::from_millis(phase1_ms);
    observe(&window, &events, &mut state, deadline).await;

    out["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "other_request_path_count": state.other_paths.len(),
        "other_request_paths_sample": state.other_paths.iter().take(40).collect::<Vec<_>>(),
        "note": "站点自己的请求由**站点**发出；壳侧只是订阅与取体"
    });

    // 文档链 + 落点。
    let documents: Vec<Value> = state
        .documents
        .iter()
        .enumerate()
        .map(|(index, doc)| doc.to_json(true, index == 0))
        .collect::<Vec<_>>();
    out["document_chain"] = json!(documents);
    // 落地 = **主框架**（第一跳文档）；后面几条是跨域子框架（iframe）。
    let landing = state.documents.first();
    out["landing"] = match landing {
        Some(doc) => json!({
            "url_masked": doc.url,
            "requested_url_masked": doc.requested_url,
            "status": doc.status,
            "title": doc.title,
            "body_bytes": doc.body_bytes,
            "from_disk_cache": doc.from_disk_cache,
            "markers": doc.markers,
            "note": "落地取**主框架**（第一跳文档）；document_chain 里后面的条目是子框架"
        }),
        None => json!({ "note": "这一段没看到任何文档请求" }),
    };
    out["sub_document_count"] = json!(state.documents.len().saturating_sub(1));

    // 站点自己的 API 请求。
    let backend: Vec<&Hit> = state
        .hits
        .iter()
        .filter(|hit| hit.path.starts_with(BACKEND_API_PREFIX))
        .collect();
    out["site_api_request_count"] = json!(state.hits.len());
    out["backend_api_request_count"] = json!(backend.len());
    out["backend_api_requests"] = json!(backend
        .iter()
        .map(|hit| hit.to_json(true))
        .collect::<Vec<_>>());
    out["other_api_requests"] = json!(state
        .hits
        .iter()
        .filter(|hit| !hit.path.starts_with(BACKEND_API_PREFIX))
        .map(|hit| hit.to_json(false))
        .collect::<Vec<_>>());

    // 挑战判据（**只列事实与规则**，不替读者下结论）。
    let challenge_detected = detect_challenge(&state);
    out["challenge_detected"] = json!(challenge_detected);
    out["site_sent_own_api_requests"] = json!(!backend.is_empty());

    // 目录端点的会话 id（**只在内存里**用；报告里只留打码形态）。
    let mut derived: Option<String> = None;
    let mut catalog_notes: Vec<Value> = Vec::new();
    for hit in &backend {
        if !hit.path.starts_with(STATIC_CATALOG_CANDIDATE) {
            continue;
        }
        catalog_notes.push(json!({
            "path": hit.path,
            "status": hit.status,
            "body_bytes": hit.body_bytes,
            "conversation_id_masked": hit
                .body_shape
                .as_ref()
                .and_then(|shape| shape.get("first_conversation_id_masked").cloned())
                .unwrap_or(Value::Null),
        }));
        if derived.is_none() {
            derived = hit.derived_session_id.clone();
        }
    }
    out["catalog_candidate_observations"] = json!(catalog_notes);

    Phase1Outcome {
        reading: out,
        session_id: derived,
    }
}

/// 第 2 段：导航到深链 → 观测站点自己发的每一条 API 请求 ＋ 响应体形状。
async fn run_deeplink_phase(
    handle: &tauri::AppHandle,
    profile: &Path,
    session_id: &str,
    phase2_ms: u64,
) -> Value {
    let deep_link = format!("{DEEP_LINK_PREFIX}{session_id}");
    let mut out = json!({
        "phase": "deeplink",
        "deep_link_masked": mask_id(session_id),
        "session_id_masked": mask_id(session_id),
        "session_id_len": session_id.chars().count(),
    });

    let window = match build_window(handle, profile, "chatgpt-p2-deeplink") {
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
    let deadline = Instant::now() + Duration::from_millis(phase2_ms);
    observe(&window, &events, &mut state, deadline).await;

    out["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "by_event": state.by_event.clone(),
        "other_request_path_count": state.other_paths.len(),
        "note": "深链导航里站点自己发的请求：逐条都记（不只第一条）"
    });

    out["document_chain"] = json!(state
        .documents
        .iter()
        .enumerate()
        .map(|(index, doc)| doc.to_json(false, index == 0))
        .collect::<Vec<_>>());

    // **逐条**记下站点自己发的每一条 API 请求。
    out["api_request_count"] = json!(state.hits.len());
    out["api_requests"] = json!(state
        .hits
        .iter()
        .map(|hit| hit.to_json(true))
        .collect::<Vec<_>>());

    let backend: Vec<&Hit> = state
        .hits
        .iter()
        .filter(|hit| hit.path.starts_with(BACKEND_API_PREFIX))
        .collect();
    out["backend_api_request_count"] = json!(backend.len());
    out["backend_api_requests"] = json!(backend
        .iter()
        .map(|hit| hit.to_json(true))
        .collect::<Vec<_>>());

    // **实测路径**去重（判定一律看这份，不看静态取证）。
    let mut measured_paths: Vec<String> = Vec::new();
    for hit in &state.hits {
        if !measured_paths.contains(&hit.path) {
            measured_paths.push(hit.path.clone());
        }
    }
    out["measured_api_paths"] = json!(measured_paths);

    // 静态取证的**候选**与实测的对照（候选只做对照，结论看实测）。
    out["static_candidate_check"] = json!({
        "catalog_candidate": STATIC_CATALOG_CANDIDATE,
        "catalog_candidate_observed": backend.iter().any(|hit| hit.path.starts_with(STATIC_CATALOG_CANDIDATE)),
        "catalog_candidate_measured_paths": backend
            .iter()
            .filter(|hit| hit.path.starts_with(STATIC_CATALOG_CANDIDATE))
            .map(|hit| hit.path.clone())
            .collect::<Vec<_>>(),
        "body_candidate_prefix": "/backend-api/conversations/",
        "body_candidate_observed": backend.iter().any(|hit| hit.path.starts_with("/backend-api/conversations/")),
        "body_candidate_measured_paths": backend
            .iter()
            .filter(|hit| hit.path.starts_with("/backend-api/conversations/"))
            .map(|hit| hit.path.clone())
            .collect::<Vec<_>>(),
        "note": "候选来自静态取证；**实测路径**在 measured_api_paths 里，判定以实测为准"
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
        .title("ChatGPT 取证探针")
        .inner_size(1000.0, 760.0)
        .visible(false)
        .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .data_directory(profile.to_path_buf())
        .build()
        .map_err(|e| format!("无法创建探测窗口：{e}"))
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

/// 抽干事件直到截止时间；**取体**在 `loadingFinished` 之后立刻做。
///
/// 取体要等加载完（协议 §8.5.1 硬约束 1：取早了会**静默**拿到长度 0 的 body
/// 且不报错 ⇒ ⇒ 空体一律算「回程失败」，不当成「站点没数据」）。
async fn observe(
    window: &tauri::WebviewWindow,
    events: &Receiver<Msg>,
    state: &mut EventState,
    deadline: Instant,
) {
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
                        if resource_type == "Document" && !request_id.is_empty() {
                            // 上一跳（重定向）带在 `redirectResponse` 里。
                            let redirect = value.get("redirectResponse").and_then(|r| {
                                let status = r.get("status").and_then(Value::as_i64)?;
                                let location = r
                                    .get("headers")
                                    .and_then(|h| h.get("location"))
                                    .and_then(Value::as_str)
                                    .map(|text| mask_url(text))
                                    .unwrap_or_else(|| "<none>".to_string());
                                Some(json!({ "status": status, "location_masked": location }))
                            });
                            // 同一条 requestId 再次出现 = **重定向到了下一跳** ⇒ ⇒
                            // 改写那一条的落地 URL（不新开一条），免得「落地 URL」
                            // 取到的是重定向链中间那一跳。
                            if let Some(existing) = state
                                .documents
                                .iter_mut()
                                .find(|doc| doc.request_id == request_id)
                            {
                                existing.url = mask_url(&url);
                                existing.origin = origin_of(&url);
                                existing.path = url_path(&url);
                                existing.query_keys = url_query_keys(&url);
                                if redirect.is_some() {
                                    existing.redirect = redirect;
                                }
                                continue;
                            }
                            state.documents.push(DocEntry {
                                request_id,
                                requested_url: mask_url(&url),
                                url: mask_url(&url),
                                origin: origin_of(&url),
                                path: url_path(&url),
                                query_keys: url_query_keys(&url),
                                status: None,
                                mime_type: None,
                                from_disk_cache: false,
                                from_service_worker: false,
                                header_names: Vec::new(),
                                header_signals: json!({}),
                                redirect,
                                at_ms: now_ms(),
                                body_bytes: None,
                                body_error: None,
                                title: None,
                                markers: None,
                            });
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
                        let from_disk_cache = response
                            .get("fromDiskCache")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let from_service_worker = response
                            .get("fromServiceWorker")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);

                        if resource_type == "Document" {
                            if let Some(doc) = state
                                .documents
                                .iter_mut()
                                .rev()
                                .find(|doc| doc.request_id == request_id)
                            {
                                doc.status = Some(status);
                                doc.mime_type = response
                                    .get("mimeType")
                                    .and_then(Value::as_str)
                                    .map(|text| text.to_string());
                                doc.from_disk_cache = from_disk_cache;
                                doc.from_service_worker = from_service_worker;
                                let (names, signals) = header_facts(response.get("headers"));
                                doc.header_names = names;
                                doc.header_signals = signals;
                                continue;
                            }
                        }

                        let path = url_path(&url);
                        let is_api = resource_type == "XHR" || resource_type == "Fetch";
                        let is_backend = path.starts_with(BACKEND_API_PREFIX);
                        if !is_api && !is_backend {
                            if state.other_paths.len() < 400 {
                                state.other_paths.push(path);
                            }
                            continue;
                        }
                        let meta = state.req_meta.get(&request_id);
                        state.hits.push(Hit {
                            request_id: request_id.clone(),
                            method: meta
                                .map(|m| m.method.clone())
                                .filter(|m| !m.is_empty())
                                .unwrap_or_else(|| "?".to_string()),
                            origin: origin_of(&url),
                            path,
                            query_keys: url_query_keys(&url),
                            query_values: url_query_values(&url),
                            status,
                            resource_type: if resource_type.is_empty() {
                                meta.map(|m| m.resource_type.clone()).unwrap_or_default()
                            } else {
                                resource_type.clone()
                            },
                            from_disk_cache,
                            from_service_worker,
                            at_ms: meta.map(|m| m.at_ms).unwrap_or_else(now_ms),
                            body_bytes: None,
                            body_error: None,
                            body_shape: None,
                            production_parse: None,
                            derived_session_id: None,
                        });
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

        // 取体：加载完的响应**立刻取**（不等到窗口末尾，避免 body 被浏览器回收）。
        let ready_hits: Vec<String> = state
            .hits
            .iter()
            .filter(|hit| {
                hit.body_bytes.is_none()
                    && hit.body_error.is_none()
                    && state.finished.contains(&hit.request_id)
            })
            .map(|hit| hit.request_id.clone())
            .collect();
        for request_id in ready_hits {
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
            let mut derived_id = None;
            let mut production = None;
            match envelope {
                Ok(raw) => match decode_body(&raw) {
                    Ok(text) => {
                        bytes = Some(text.len());
                        let (shape_value, session_id) = analyze_shape(&text);
                        shape = Some(shape_value);
                        derived_id = session_id;
                        // ⚠️ **生产那份**解析器跑一遍**真响应体** ⇒ ⇒ 读数里的
                        // 「生产裁剪结果」就是**将来上线那份**的结果。
                        let hit_path = state
                            .hits
                            .iter()
                            .find(|hit| hit.request_id == request_id)
                            .map(|hit| hit.path.clone());
                        let hit_status = state
                            .hits
                            .iter()
                            .find(|hit| hit.request_id == request_id)
                            .map(|hit| hit.status)
                            .unwrap_or_default();
                        if let Some(path) = hit_path.as_deref() {
                            production = production_parse(path, &text, hit_status);
                        }
                    }
                    Err(detail) => error = Some(detail),
                },
                Err(e) => error = Some(format!("{e:?}")),
            }
            if let Some(hit) = state.hits.iter_mut().find(|hit| hit.request_id == request_id) {
                hit.body_bytes = bytes;
                hit.body_error = error;
                hit.body_shape = shape;
                hit.production_parse = production;
                hit.derived_session_id = derived_id;
            }
        }

        let ready_docs: Vec<String> = state
            .documents
            .iter()
            .filter(|doc| {
                doc.body_bytes.is_none()
                    && doc.body_error.is_none()
                    && state.finished.contains(&doc.request_id)
            })
            .map(|doc| doc.request_id.clone())
            .collect();
        for request_id in ready_docs {
            let envelope = cdp_call(
                window,
                "Network.getResponseBody",
                json!({ "requestId": request_id }),
                Duration::from_secs(20),
            )
            .await;
            let mut bytes = None;
            let mut error = None;
            let mut title = None;
            let mut markers = None;
            match envelope {
                Ok(raw) => match decode_body(&raw) {
                    Ok(text) => {
                        bytes = Some(text.len());
                        title = extract_title(&text);
                        markers = Some(scan_markers(&text));
                    }
                    Err(detail) => error = Some(detail),
                },
                Err(e) => error = Some(format!("{e:?}")),
            }
            if let Some(doc) = state
                .documents
                .iter_mut()
                .find(|doc| doc.request_id == request_id)
            {
                doc.body_bytes = bytes;
                doc.body_error = error;
                doc.title = title;
                doc.markers = markers;
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
/// 返回两样：形状读数 ＋ 从目录响应体里解析出的**会话 id**（后者只在内存里
/// 传给下一段，**不进读数**——读数里只留打码形态）。
fn analyze_shape(body: &str) -> (Value, Option<String>) {
    let root: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(e) => {
            return (
                json!({
                    "parse_error": format!("{e}"),
                    "body_bytes": body.len(),
                    "note": "响应体不是 JSON ⇒ 只有字节数，没有形状"
                }),
                None,
            )
        }
    };
    let data = root.get("data");
    let mut facts = ShapeFacts {
        budget: 200_000,
        ..Default::default()
    };
    facts.walk(&root, "$", 0);

    let mut out = json!({
        "top_level_keys": sorted_keys(Some(&root)),
        "data_keys": sorted_keys(data),
        "arrays": facts.arrays,
        "array_shapes": facts.array_shapes,
        "message_like_arrays": facts.message_like_arrays,
        "object_key_sets": facts.object_key_sets,
        "message_objects": facts.message_objects,
        "mapping_objects": facts.mapping_objects,
        "role_values": facts.role_values.into_iter().collect::<Vec<_>>(),
        "author_role_values": facts.author_role_values.into_iter().collect::<Vec<_>>(),
        "author_string_values": facts.author_string_values.into_iter().collect::<Vec<_>>(),
        "objects_with_id_count": facts.id_count,
        "truncated": facts.truncated,
        "note": "只有键名、路径、计数：一个正文值都没落"
    });
    // **事实读数**（时间戳的**数值** ＋ `content` 的**形状** ＋ 增量协议那两个键
    // 在不在）⇒ ⇒ 生产那条链要按它落规格表 ⇒ ⇒ 不记就等于猜。
    out["facts"] = analyze_facts(&root);
    // 目录端点的会话 id：**报告里只留打码形态**。
    let session_id = first_conversation_id(&root);
    if let Some(id) = session_id.as_ref() {
        out["first_conversation_id_masked"] = json!(mask_id(id));
    }
    (out, session_id)
}

/// **事实读数**：时间戳的**数值** ＋ 角色**取值集合** ＋ `content` 的**形状**
/// ＋ **增量协议那两个键在不在**。
///
/// ## 为什么需要这一段
///
/// 形状读数只给**键名与计数** ⇒ ⇒ 答不了三个**落规格表必须答**的问题：
///
/// · **`update_time` 是秒还是毫秒**（协议 §4.3 ② 那个字段是**秒级小数**）
///   ⇒ ⇒ **时间戳的值**必须读出来（**数字，不是内容**）；
/// · **`content` 到底长什么样**（字符串？对象？里面有哪些键？`parts[]` 每项
///   有哪些键？）⇒ ⇒ 生产那条链的正文裁剪**按实测实现**，不许照抄公开文档；
/// · **正文端点是不是增量协议**（`cache_control`／`cache_version` 在不在）
///   ⇒ ⇒ 冷档案那一套是**为 DeepSeek 的 MERGE 协议**定的 ⇒ ⇒ ChatGPT 要不要
///   用冷档案**按这个判**。
///
/// ⚠️ **只落数字、键名、计数与长度** ⇒ ⇒ **没有正文原文**（文本一律只留长度）。
fn analyze_facts(root: &Value) -> Value {
    let mut facts = serde_json::Map::new();

    // ① 时间戳的**值**（数字）—— 目录那条的 `update_time` / `create_time`，
    //    正文那条顶层的 `create_time` / `update_time` 都在内。
    let mut update_times: Vec<f64> = Vec::new();
    collect_numbers(root, "update_time", 0, &mut update_times);
    if !update_times.is_empty() {
        facts.insert("update_time_values".into(), json!(update_times));
    }
    // ⚠️ **时间戳也可能给成**字符串** ⇒ ⇒ **字符串那一支也记**（否则「值是
    // 字符串」这件事会被读成「没有这个字段」⇒ ⇒ 生产那条链会静默取到 0）。
    let mut update_texts: Vec<String> = Vec::new();
    collect_texts(root, "update_time", 0, &mut update_texts);
    if !update_texts.is_empty() {
        facts.insert("update_time_texts".into(), json!(update_texts));
    }
    let mut create_times: Vec<f64> = Vec::new();
    collect_numbers(root, "create_time", 0, &mut create_times);
    if !create_times.is_empty() {
        facts.insert("create_time_values".into(), json!(create_times));
    }
    let mut create_texts: Vec<String> = Vec::new();
    collect_texts(root, "create_time", 0, &mut create_texts);
    if !create_texts.is_empty() {
        facts.insert("create_time_texts".into(), json!(create_texts));
    }
    // ①' **最新助手轮次**那个时间戳（目录条目里另有一个带时间的字段）。
    let mut turn_times: Vec<f64> = Vec::new();
    collect_numbers(root, "latest_assistant_turn_created_at", 0, &mut turn_times);
    if !turn_times.is_empty() {
        facts.insert("latest_assistant_turn_created_at_values".into(), json!(turn_times));
    }

    // ①'' **条目各键的类型**（目录那条的 `$.items[0]`）—— 协议 §2.4 那一栏
    //     「条目的类型」就是它 ⇒ ⇒ **类型**必须量出来（`update_time` 是数还是
    //     串、`is_starred` 是不是布尔……）。
    if let Some(items) = root.get("items").and_then(Value::as_array) {
        if let Some(first) = items.first() {
            facts.insert("item_field_types".into(), field_types(first));
        }
    }

    // ② 增量协议那两个键（`cache_control` / `cache_version`）**在不在**。
    facts.insert(
        "cache_control_present".into(),
        json!(has_key_anywhere(root, "cache_control", 0)),
    );
    facts.insert(
        "cache_version_present".into(),
        json!(has_key_anywhere(root, "cache_version", 0)),
    );

    // ③ 消息数组：条数 ＋ 角色直方图 ＋ `content` 的形状 ＋ 空内容条数。
    if let Some(list) = messages_of(root) {
        let mut histogram: BTreeMap<String, usize> = BTreeMap::new();
        let mut empty_text = 0usize;
        let mut samples: BTreeMap<String, Value> = BTreeMap::new();
        for item in list {
            let role = role_of_message(item);
            *histogram.entry(role.clone()).or_insert(0) += 1;
            if message_text_length(item) == 0 {
                empty_text += 1;
            }
            samples.entry(role).or_insert_with(|| content_sample(item));
        }
        facts.insert("messages_count".into(), json!(list.len()));
        facts.insert("role_histogram".into(), json!(histogram));
        facts.insert("empty_content_count".into(), json!(empty_text));
        facts.insert("content_samples".into(), json!(samples));
    }

    Value::Object(facts)
}

/// 收集某个**数字**字段的全部取值（**只取数字** ⇒ 不可能是正文）。
fn collect_numbers(value: &Value, field: &str, depth: usize, out: &mut Vec<f64>) {
    if depth > 12 {
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
                collect_numbers(child, field, depth + 1, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_numbers(item, field, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// 收集某个**字符串**字段的全部取值（**只取字符串**）。
///
/// ⚠️ **为什么与 [`collect_numbers`] 并列**：站点的同一个时间戳字段**可能给成
/// 字符串** ⇒ ⇒ 只记数字那一支会把「值是字符串」读成「没有这个字段」。
fn collect_texts(value: &Value, field: &str, depth: usize, out: &mut Vec<String>) {
    if depth > 12 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == field {
                    if let Some(text) = child.as_str() {
                        out.push(text.to_string());
                    }
                }
                collect_texts(child, field, depth + 1, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_texts(item, field, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// 一个对象**各键的类型**（协议 §2.4 的「条目的类型」那一栏）。
fn field_types(value: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(map) = value.as_object() {
        for (key, child) in map {
            out.insert(key.clone(), json!(type_name_of(child)));
        }
    }
    Value::Object(out)
}

/// 一个值的**类型名**（协议 §2.4 那一栏的六个类型名）。
fn type_name_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 某个键名**在不在**（**任意深度**；只判在不在，不取值）。
fn has_key_anywhere(value: &Value, key: &str, depth: usize) -> bool {
    if depth > 12 {
        return false;
    }
    match value {
        Value::Object(map) => {
            if map.contains_key(key) {
                return true;
            }
            map.values().any(|child| has_key_anywhere(child, key, depth + 1))
        }
        Value::Array(items) => items
            .iter()
            .any(|item| has_key_anywhere(item, key, depth + 1)),
        _ => false,
    }
}

/// 找**消息数组**（顶层 `messages` 优先；否则找第一个「元素带 `content` ＋
/// `author`/`role`」的数组）。
fn messages_of(root: &Value) -> Option<&Vec<Value>> {
    if let Some(list) = root.get("messages").and_then(Value::as_array) {
        return Some(list);
    }
    fn search(value: &Value, depth: usize) -> Option<&Vec<Value>> {
        if depth > 8 {
            return None;
        }
        match value {
            Value::Array(items) => {
                let looks_like_messages = items.iter().take(3).any(|item| {
                    let Some(map) = item.as_object() else {
                        return false;
                    };
                    map.contains_key("content")
                        && (map.contains_key("author") || map.contains_key("role"))
                });
                if looks_like_messages {
                    return Some(items);
                }
                for item in items {
                    if let Some(found) = search(item, depth + 1) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Object(map) => {
                for child in map.values() {
                    if let Some(found) = search(child, depth + 1) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }
    search(root, 0)
}

/// 一条消息的**角色**（`author.role` ／ `author` 本身是串 ／ `role`）。
fn role_of_message(item: &Value) -> String {
    let Some(map) = item.as_object() else {
        return "<not-an-object>".to_string();
    };
    if let Some(author) = map.get("author") {
        if let Some(role) = author.get("role").and_then(Value::as_str) {
            return role.to_string();
        }
        if let Some(role) = author.as_str() {
            return role.to_string();
        }
    }
    if let Some(role) = map.get("role").and_then(Value::as_str) {
        return role.to_string();
    }
    "<none>".to_string()
}

/// 一条消息的**正文长度**（按**实测**的两条形状算；**只留长度**，不落原文）。
///
/// ## 形状（2026-10-22 13:5x · ChatGPT 运行时实测）
///
/// · **user ／ tool** 消息：`content` ＝ `{content_type, parts}`，而 `parts[]`
///   的每一项是**字符串**（实测 `parts` 长度 1）；
/// · **assistant** 消息：`content` ＝ `{content_type, content}` —— **没有
///   `parts`** ⇒ ⇒ 正文在**那个 `content` 字段**里。
fn message_text_length(item: &Value) -> usize {
    let Some(content) = item.get("content") else {
        return 0;
    };
    if let Some(text) = content.as_str() {
        return text.chars().count();
    }
    // **assistant 形态**：`content.content` 是字符串。
    if let Some(text) = content.get("content").and_then(Value::as_str) {
        return text.chars().count();
    }
    // **user ／ tool 形态**：`content.parts[]` 的每一项是字符串。
    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
        return 0;
    };
    parts
        .iter()
        .filter_map(|part| part.as_str())
        .map(|text| text.chars().count())
        .sum()
}

/// 一条消息的 `content` **形状**（类型 ＋ 键名 ＋ `parts[]` 的键名集合）。
fn content_sample(item: &Value) -> Value {
    let Some(content) = item.get("content") else {
        return json!({ "present": false });
    };
    let kind = match content {
        Value::String(_) => "string",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::Null => "null",
        _ => "other",
    };
    let mut sample = json!({
        "present": true,
        "content_type": kind,
        "content_keys": sorted_keys(Some(content)),
        // ⚠️ **各键的类型**（`content.content` 到底是字符串还是对象 ⇒ ⇒
        // 正文裁剪**按它实现**，不许照抄公开文档）。
        "content_field_types": field_types(content),
    });
    if let Some(parts) = content.get("parts").and_then(Value::as_array) {
        let mut sets: Vec<Vec<String>> = Vec::new();
        for part in parts.iter().take(5) {
            let keys = sorted_keys(Some(part));
            if !sets.contains(&keys) {
                sets.push(keys);
            }
        }
        sample["parts_len"] = json!(parts.len());
        sample["parts_item_key_sets"] = json!(sets);
        // ⚠️ **`parts[]` 每一项的类型**（实测：每一项**是字符串** ⇒ ⇒ 不是对象）。
        sample["parts_item_types"] = json!(
            parts
                .iter()
                .take(5)
                .map(|part| type_name_of(part))
                .collect::<Vec<&str>>()
        );
        // `parts[]` 里每一项的**类型**取值（`text` / `image_asset_pointer` …）
        // —— **类型名不是内容** ⇒ ⇒ 可以落。
        let mut types: Vec<String> = Vec::new();
        for part in parts.iter().take(50) {
            if let Some(kind) = part.get("type").and_then(Value::as_str) {
                let kind = kind.to_string();
                if !types.contains(&kind) {
                    types.push(kind);
                }
            }
        }
        sample["parts_types"] = json!(types);
    }
    sample
}

/// 用**生产那份**解析器跑一遍**真响应体**（读数里只留**条数／键名／计数**）。
///
/// ⚠️ **跑的是生产那份源文件**（`#[path]` 编进来的同一份）⇒ ⇒ 读数里的
/// 「生产裁剪结果」就是**将来上线那份**的结果。
///
/// ⚠️ **不落正文原文**、**不落会话标题**（只留长度）、**不落会话 id**（只留
/// 打码形态）⇒ ⇒ 与探针的纪律一致。
fn production_parse(path: &str, text: &str, status: i64) -> Option<Value> {
    let spec = session_structure::spec_for(session_structure::CHATGPT_SITE)?;
    let catalog_path = spec.catalog_path;
    // ⚠️ **目录那条**：路径**就是**目录端点（正文那条多一段会话 id）。
    if path == catalog_path {
        return Some(match session_structure::SessionStructure::from_body_for(text, status, spec) {
            Ok(structure) => {
                let payload = structure.to_json();
                let items: Vec<Value> = payload["items"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .map(|item| {
                                json!({
                                    "id_masked": mask_id(item["id"].as_str().unwrap_or_default()),
                                    "title_len": item["title"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .chars()
                                        .count(),
                                    "title_type": item["title_type"],
                                    "model_type": item["model_type"],
                                    "pinned": item["pinned"],
                                    "updated_at": item["updated_at"],
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                json!({
                    "ok": true,
                    "sessions_length": structure.sessions_length,
                    "items_count": items.len(),
                    "items": items,
                    "has_more": structure.has_more,
                })
            }
            Err(failure) => json!({
                "ok": false,
                "code": failure.code(),
                "detail": failure.detail(),
            }),
        });
    }
    // ⚠️ **正文那条**：路径是「目录端点 ＋ 一段会话 id」。
    let body_prefix = format!("{catalog_path}/");
    if !path.starts_with(&body_prefix) {
        return None;
    }
    Some(match session_body::SessionBody::from_body_for(text, status, spec) {
        Ok(body) => {
            let payload = body.to_json();
            // ⚠️ **只留条数与角色分布**（**不落正文原文**）。
            let mut roles: BTreeMap<String, usize> = BTreeMap::new();
            if let Some(messages) = payload["messages"].as_array() {
                for message in messages {
                    let role = message["role"].as_str().unwrap_or("<none>").to_string();
                    *roles.entry(role).or_insert(0) += 1;
                }
            }
            json!({
                "ok": true,
                "messages_count": payload["messages"].as_array().map(|m| m.len()).unwrap_or(0),
                "roles": roles,
                "capture_result": payload["captureResult"],
                "pick": payload["pick"],
            })
        }
        Err(failure) => json!({
            "ok": false,
            "code": failure.capture_code(),
            "detail": failure.capture_detail(),
        }),
    })
}

/// 形状事实的累加器（**只有键名与计数**）。
#[derive(Default)]
struct ShapeFacts {    /// 所有数组（路径 ＋ 长度）。
    arrays: Vec<Value>,
    /// 数组的元素键名集合（路径 ＋ 长度 ＋ 键名集合）。
    array_shapes: Vec<Value>,
    /// 看起来像**消息数组**的那些（元素带 role/author/message/content）。
    message_like_arrays: Vec<Value>,
    /// 「有意思」的对象（键名命中观察名单）。
    object_key_sets: Vec<Value>,
    /// 带 `message` 键的对象。
    message_objects: Vec<Value>,
    /// `mapping` 对象（ChatGPT 的正文在 mapping 树里，**不是数组**）。
    mapping_objects: Vec<Value>,
    role_values: BTreeSet<String>,
    author_role_values: BTreeSet<String>,
    author_string_values: BTreeSet<String>,
    id_count: usize,
    truncated: bool,
    /// 遍历预算（防超大响应体把探针拖死）。
    budget: usize,
}

/// 观察名单：这些键名出现的对象值得记一笔键名。
const WATCH_KEYS: &[&str] = &[
    "id",
    "role",
    "author",
    "message",
    "mapping",
    "messages",
    "items",
    "conversations",
    "content",
    "title",
    "create_time",
    "update_time",
    "current_node",
    "parent_node",
    "children",
    "conversation_id",
    "parts",
    "model",
    "status",
    "error",
    "data",
    "result",
];

impl ShapeFacts {
    fn walk(&mut self, node: &Value, path: &str, depth: usize) {
        if depth > 14 {
            self.truncated = true;
            return;
        }
        if self.budget == 0 {
            self.truncated = true;
            return;
        }
        self.budget = self.budget.saturating_sub(1);

        match node {
            Value::Object(map) => {
                if map.contains_key("id") {
                    self.id_count += 1;
                }
                if let Some(Value::String(role)) = map.get("role") {
                    self.role_values.insert(clip(role, 40));
                }
                if let Some(Value::String(author)) = map.get("author") {
                    self.author_string_values.insert(clip(author, 40));
                }
                if let Some(Value::Object(author)) = map.get("author") {
                    if let Some(Value::String(role)) = author.get("role") {
                        self.author_role_values.insert(clip(role, 40));
                    }
                }
                if map.keys().any(|k| WATCH_KEYS.contains(&k.as_str()))
                    && self.object_key_sets.len() < 80
                {
                    self.object_key_sets
                        .push(json!({ "path": path, "keys": sorted_keys(Some(node)) }));
                }
                // `mapping` 是**对象**（不是数组）⇒ 正文消息不在数组里。
                if let Some(Value::Object(mapping)) = map.get("mapping") {
                    let mut entry_key_sets: Vec<Vec<String>> = Vec::new();
                    let mut message_key_sets: Vec<Vec<String>> = Vec::new();
                    let mut author_roles: BTreeSet<String> = BTreeSet::new();
                    for entry in mapping.values().take(300) {
                        let keys = sorted_keys(Some(entry));
                        if !entry_key_sets.contains(&keys) && entry_key_sets.len() < 6 {
                            entry_key_sets.push(keys);
                        }
                        if let Some(message) = entry.get("message") {
                            let message_keys = sorted_keys(Some(message));
                            if !message_key_sets.contains(&message_keys) && message_key_sets.len() < 6 {
                                message_key_sets.push(message_keys);
                            }
                            if let Some(Value::Object(author)) = message.get("author") {
                                if let Some(Value::String(role)) = author.get("role") {
                                    author_roles.insert(clip(role, 40));
                                }
                            }
                        }
                    }
                    self.mapping_objects.push(json!({
                        "path": format!("{path}.mapping"),
                        "entries": mapping.len(),
                        "entry_key_sets": entry_key_sets,
                        "message_key_sets": message_key_sets,
                        "author_role_values": author_roles.into_iter().collect::<Vec<_>>(),
                    }));
                }
                if let Some(message) = map.get("message") {
                    if message.is_object() {
                        let author_role = message
                            .get("author")
                            .and_then(|author| author.get("role"))
                            .and_then(Value::as_str)
                            .map(|role| clip(role, 40));
                        self.message_objects.push(json!({
                            "path": format!("{path}.message"),
                            "owner_keys": sorted_keys(Some(node)),
                            "message_keys": sorted_keys(Some(message)),
                            "author_role": author_role,
                        }));
                    }
                }
                for (key, value) in map {
                    self.walk(value, &format!("{path}.{key}"), depth + 1);
                }
            }
            Value::Array(items) => {
                if self.arrays.len() < 200 {
                    self.arrays
                        .push(json!({ "path": path, "len": items.len() }));
                } else {
                    self.truncated = true;
                }
                let mut sets: Vec<Vec<String>> = Vec::new();
                let mut message_like = false;
                for item in items.iter().take(60) {
                    if !item.is_object() {
                        continue;
                    }
                    let keys = sorted_keys(Some(item));
                    if !sets.contains(&keys) && sets.len() < 5 {
                        sets.push(keys);
                    }
                    if item.get("role").is_some()
                        || item.get("author").is_some()
                        || item.get("message").is_some()
                        || item.get("content").is_some()
                    {
                        message_like = true;
                    }
                }
                if !sets.is_empty() {
                    if self.array_shapes.len() < 120 {
                        self.array_shapes.push(json!({
                            "path": path,
                            "len": items.len(),
                            "element_key_sets": sets,
                        }));
                    } else {
                        self.truncated = true;
                    }
                    if message_like && self.message_like_arrays.len() < 40 {
                        self.message_like_arrays.push(json!({
                            "path": path,
                            "len": items.len(),
                            "element_key_sets": sets,
                        }));
                    }
                }
                for (index, item) in items.iter().enumerate() {
                    self.walk(item, &format!("{path}[{index}]"), depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// 从 JSON 里找**第一个**会话 id（目录响应体 ⇒ `items[].id`）。
fn first_conversation_id(root: &Value) -> Option<String> {
    let mut found: Option<String> = None;
    collect_conversation_ids(root, &mut found);
    found
}

fn collect_conversation_ids(node: &Value, out: &mut Option<String>) {
    if out.is_some() {
        return;
    }
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "items" || key == "conversations" {
                    if let Some(items) = value.as_array() {
                        for item in items {
                            if let Some(id) = item
                                .get("id")
                                .or_else(|| item.get("conversation_id"))
                                .and_then(Value::as_str)
                            {
                                if !id.is_empty() {
                                    *out = Some(id.to_string());
                                    return;
                                }
                            }
                        }
                    }
                }
                collect_conversation_ids(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_conversation_ids(item, out);
            }
        }
        _ => {}
    }
}

/// 判「这一轮是不是撞上了 Cloudflare 挑战」（**只按规则判，事实另列**）。
fn detect_challenge(state: &EventState) -> bool {
    for doc in &state.documents {
        // 规则一：文档体里命中强特征。
        if let Some(markers) = &doc.markers {
            if markers
                .get("strong_hits")
                .and_then(Value::as_array)
                .map(|hits| !hits.is_empty())
                .unwrap_or(false)
            {
                return true;
            }
        }
        // 规则二：响应头 `cf-mitigated: challenge`。
        if let Some(mitigated) = doc
            .header_signals
            .get("cf-mitigated")
            .and_then(Value::as_str)
        {
            if mitigated.eq_ignore_ascii_case("challenge") {
                return true;
            }
        }
        // 规则三：5xx/403 且文档体像挑战页（弱特征命中）。
        let status = doc.status.unwrap_or_default();
        if matches!(status, 403 | 429 | 503) || (500..600).contains(&status) {
            if let Some(markers) = &doc.markers {
                let weak = markers
                    .get("weak_hits")
                    .and_then(Value::as_array)
                    .map(|hits| !hits.is_empty())
                    .unwrap_or(false);
                if weak {
                    return true;
                }
            }
        }
    }
    false
}

/// 文档体里的 Cloudflare 挑战特征（**强特征**与**弱特征**分开列）。
fn scan_markers(html: &str) -> Value {
    let mut strong_hits: Vec<String> = Vec::new();
    for (name, needle) in CF_STRONG_MARKERS {
        if html.contains(needle) {
            strong_hits.push((*name).to_string());
        }
    }
    let mut weak_hits: Vec<String> = Vec::new();
    let cf_dash = html.matches("cf-").count();
    if cf_dash > 0 {
        weak_hits.push(format!("cf_dash_x{cf_dash}"));
    }
    if html.contains("cloudflare") || html.contains("Cloudflare") {
        weak_hits.push("cloudflare_word".to_string());
    }
    json!({
        "strong_hits": strong_hits,
        "weak_hits": weak_hits,
        "body_len": html.len(),
    })
}

/// 文档的 `<title>`（**只取第一段**，裁到 200 字）。
fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let rest = &html[open_end..];
    let end = rest.to_ascii_lowercase().find("</title>")?;
    let title = rest[..end].trim();
    if title.is_empty() {
        return None;
    }
    Some(clip(title, 200))
}

/// 响应头的两件事：**名字**（`set-cookie` 不记）与**白名单的值**。
fn header_facts(headers: Option<&Value>) -> (Vec<String>, Value) {
    let mut names: Vec<String> = Vec::new();
    let mut signals = serde_json::Map::new();
    let Some(Value::Object(map)) = headers else {
        return (names, Value::Object(signals));
    };
    for (name, value) in map {
        let lower = name.to_ascii_lowercase();
        if lower == "set-cookie" {
            // 连名字都不记：cookie 是凭据。
            continue;
        }
        names.push(lower.clone());
        if HEADER_SIGNAL_NAMES.contains(&lower.as_str()) {
            if let Some(text) = value.as_str() {
                signals.insert(lower.clone(), json!(mask_header_value(&lower, text)));
            }
        }
    }
    names.sort();
    names.dedup();
    (names, Value::Object(signals))
}

/// 头部值的打码（`cf-ray` 留个前缀、`location` 只留 host ＋ path）。
fn mask_header_value(name: &str, value: &str) -> String {
    match name {
        "cf-ray" => clip(value, 8),
        "location" => mask_url(value),
        _ => clip(value, 120),
    }
}

/// URL 打码：**query 的值**一律换成 `<masked>`，键名保留。
fn mask_url(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return clip(url, 200);
    };
    let mut out = format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or(""),
        parsed.path()
    );
    let pairs: Vec<String> = parsed
        .query_pairs()
        .map(|(key, _)| format!("{key}=<masked>"))
        .collect();
    if !pairs.is_empty() {
        out.push('?');
        out.push_str(&pairs.join("&"));
    }
    out
}

/// 取 URL 的路径部分。
fn url_path(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => parsed.path().to_string(),
        Err(_) => url.to_string(),
    }
}

/// 取 URL 的 origin（`scheme://host`）。
fn origin_of(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => format!(
            "{}://{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or("")
        ),
        Err(_) => String::new(),
    }
}

/// 取 URL 的 query **键名**（值一律不落）。
fn url_query_keys(url: &str) -> Vec<String> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = parsed
        .query_pairs()
        .map(|(key, _)| key.to_string())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// 取 URL 的 query 里**白名单键**的**值**（其余键**连值都不落**）。
fn url_query_values(url: &str) -> BTreeMap<String, String> {
    let Ok(parsed) = url::Url::parse(url) else {
        return BTreeMap::new();
    };
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in parsed.query_pairs() {
        if QUERY_VALUE_KEYS.contains(&key.as_ref()) {
            out.insert(key.to_string(), value.to_string());
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

/// 裁剪一段文本（**按字符**，不切坏 UTF-8）。
fn clip(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// 会话 id 的打码形态（**不落全文**）。
fn mask_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 10 {
        return format!("<masked:{} chars>", chars.len());
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// 目标 URL 的主机片段（拿来当窗口 label）。
fn host_slug(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.to_string()))
        .unwrap_or_else(|| "target".to_string())
        .replace('.', "-")
}

/// 摘掉**副本**里的缓存目录（**源一个字节都不动**）。
///
/// 为什么要摘：缓存命中时浏览器**根本不会回源** ⇒ ⇒ 看到的是历史响应，
/// 不是站点当下的真实响应 ⇒ ⇒ 那是**假绿**。摘的是副本，删不掉的（被锁住）
/// 记下来，不当失败。
fn remove_cache_dirs(root: &Path) -> Vec<String> {
    let mut removed: Vec<String> = Vec::new();
    collect_cache_dirs(root, &mut removed);
    for dir in &removed {
        let _ = std::fs::remove_dir_all(dir);
    }
    removed
}

fn collect_cache_dirs(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if CACHE_DIR_NAMES.contains(&name.as_str()) {
            out.push(path.to_string_lossy().to_string());
            continue;
        }
        collect_cache_dirs(&path, out);
    }
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
