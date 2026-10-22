//! 只读取证探针 · **站点什么时候给全量正文**（DeepSeek 官网对话 · 单会话正文）。
//!
//! # 它要回答什么（**一条读数，不是判断**）
//!
//! 上一支形状探针（`official_chat_shape_probe.rs`）已坐实：正文那条响应里
//! `data.biz_data.chat_messages` **长度 0**，而站点自己说
//! `chat_session.is_empty` ＝ **false**、`current_message_id` ＝ 32、
//! `chat_session.version` ＝ 32（＝请求 query 里的 `cache_version`）、
//! `cache_control` ＝ **"MERGE"** ⇒ ⇒ **空数组是「相对 version 32 的空增量」**。
//!
//! 这一支要坐实最后一环：**站点什么时候给全量正文**。
//!
//! # 三条臂（**每一臂都是同一个会话、同一段代码、同一份纪律**）
//!
//! | 臂 | profile 怎么来 | 问什么 |
//! |---|---|---|
//! | **A `cold-copy`** | 每跑一次就**重新复制**一份源（**原样**，不删任何东西） | 冷客户端**原样**能不能稳定拿到全量？ |
//! | **B `cold-purged`** | 每跑一次重新复制一份源，**再把 HTTP 缓存与本地存储删掉** | 把「客户端本地缓存」这个变量**摘掉**之后还剩什么？ |
//! | **C `warm-same`** | **同一份** profile 连跑 N 次（不复制） | 同一份副本上，第 2 次起是不是**从磁盘缓存**拿的？ |
//!
//! # ⭐ 每次都报 `response.fromDiskCache`
//!
//! `Network.responseReceived` 的 `response` 里带 `fromDiskCache`（布尔）——
//! 这是「空 vs 全量」的**钥匙**：若「空增量」那几次是 `fromDiskCache=true`、
//! 而全量那次是 `false`，那根因就坐实了（**那份响应被 WebView2 的 HTTP 缓存命中**
//! ⇒ ⇒ 站点按 MERGE 给的「空增量」是**上次那份响应的副本**，不是站点这次的判断）。
//!
//! # 纪律
//!
//! - **只读**：不新建会话、不发消息、不点按钮。
//! - **壳侧一个请求都不发**：只订阅与观察，唯一的 CDP 调用是 `Network.enable`
//!   与 `Network.getResponseBody`（＋每段一次的 bogus 对照）—— **不导航到任何
//!   API 端点、不带任何自造请求头、不做页内求值**。
//! - **profile 只读**：源目录**只从源读、绝不写、绝不删**；要删的只有**副本里**
//!   那些白名单目录（HTTP 缓存与本地存储）⇒ ⇒ 源一个字节都不动。
//! - **报告里只有**：键名、计数、字节数、状态码、`fromDiskCache` 等布尔、
//!   `cache_*` 的值 ⇒ ⇒ **不落正文原文、不落会话标题、不落凭据**。
//! - **全程不开 DevTools 窗口**；**对照组**：不存在的 CDP 方法必须报错
//!   ⇒ 否则「`Network.enable` 成功」不能当证据。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_merge_probe -- \
//!   --profile-src "<只读源 profile 目录>" \
//!   --cold-base "<冷副本的落点父目录>" \
//!   --warm-profile "<臂 C 复用的那份 profile 目录>" \
//!   --out-dir "<读数目录>" \
//!   --session-id "<已知会话 id>" \
//!   [--cold-runs 4] [--warm-runs 4]
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

/// 仓库现成的**只读** profile 拷贝器（`examples/support/profile_copy.rs`）：
/// 源只读、写到一次性临时目录、源被独占时**逐个文件**记跳过原因。
#[allow(dead_code)]
#[path = "support/profile_copy.rs"]
mod profile_copy;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cdp_eval::{cdp_call, cdp_field};

/// 单会话的深链形态（G4-19 已坐实）。
const DEEP_LINK_PREFIX: &str = "https://chat.deepseek.com/a/chat/s/";

/// 站点自己的单会话正文端点。
const HISTORY_PATH: &str = "/api/v0/chat/history_messages";

/// 站点自己的会话目录端点（**顺带抓它** ⇒ ⇒ 用来核「客户端是从哪儿学到的
/// `cache_version`」—— 若目录项里就带着版本号，那客户端**天生**报上服务端的当前
/// 版本 ⇒ ⇒ 增量恒为空）。
const CATALOG_PATH: &str = "/api/v0/chat_session/fetch_page";

/// 订阅的 DevTools 事件（**按具体事件名** —— 官方 API 形态）。
const SUBSCRIBE_EVENTS: &[&str] = &[
    "Network.requestWillBeSent",
    "Network.responseReceived",
    "Network.loadingFinished",
];

/// 臂 B 要从**副本**里删掉的目录（**白名单**，且只在副本里删）。
///
/// ⚠️ **绝不含 `Network`** —— 那个目录里是 **Cookies**（登录态）⇒ ⇒ 删了就登出。
/// 删的是：HTTP 磁盘缓存、代码缓存、GPU 缓存、以及站点的**本地存储**
/// （`Local Storage` / `IndexedDB` / `Session Storage`）—— 消息缓存若在客户端，
/// 就在这几处。
const PURGE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "Local Storage",
    "IndexedDB",
    "Session Storage",
];

/// 各条臂要删哪些目录（**名字 ⇒ 目录白名单**）。
///
/// ⚠️ **实测教训**（2026-10-22 10:0x）：`all` 那套（连 `Local Storage` 一起删）
/// 会把**登录态**一起清掉 ⇒ ⇒ 站点直接跳**微信扫码登录页** ⇒ ⇒ 那一臂
/// **根本没有正文请求**（不是「站点不给」，是「客户端没登录」）⇒ ⇒ **那一臂作废**。
/// ⇒ ⇒ 下面把「只删 HTTP 缓存」「只删 IndexedDB」「只删 Local Storage」分开，
/// 好把「缓存」与「登录态」这两个变量**分开**。
fn purge_set_dirs(name: &str) -> Vec<&'static str> {
    match name {
        "none" => Vec::new(),
        "cache-only" => vec!["Cache", "Code Cache", "GPUCache"],
        "indexeddb-only" => vec!["IndexedDB"],
        "localstorage-only" => vec!["Local Storage"],
        "sessionstorage-only" => vec!["Session Storage"],
        "all" => PURGE_DIRS.to_vec(),
        _ => Vec::new(),
    }
}

/// 响应头白名单（**只取缓存相关的那几个** ⇒ ⇒ 不会有 `set-cookie`、不会有凭据）。
const CACHE_HEADERS: &[&str] = &[
    "cache-control",
    "age",
    "etag",
    "expires",
    "date",
    "last-modified",
    "vary",
    "content-type",
    "content-length",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--profile-src` 必须显式给：那是**只读源**；不给就不探测。
    let Some(profile_src) = arg_value(&args, "--profile-src").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --profile-src <只读源 profile 目录>");
        std::process::exit(2);
    };
    // 冷副本的落点父目录（探针自己按臂与轮次复制）。
    let Some(cold_base) = arg_value(&args, "--cold-base").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --cold-base <冷副本的落点父目录>");
        std::process::exit(2);
    };
    // 臂 C 复用的那份 profile（**不复制**，连跑 N 次）。
    let warm_profile = arg_value(&args, "--warm-profile").map(PathBuf::from);
    let Some(out_dir) = arg_value(&args, "--out-dir").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --out-dir <读数目录>");
        std::process::exit(2);
    };
    let Some(session_id) = arg_value(&args, "--session-id") else {
        eprintln!("probe: 必须显式给 --session-id <已知会话 id>");
        std::process::exit(2);
    };
    let cold_runs: usize = arg_value(&args, "--cold-runs")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(4);
    let warm_runs: usize = arg_value(&args, "--warm-runs")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(4);
    let observe_ms: u64 = arg_value(&args, "--observe-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(45_000);

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile_src = profile_src.clone();
            let cold_base = cold_base.clone();
            let warm_profile = warm_profile.clone();
            let out_dir = out_dir.clone();
            let session_id = session_id.clone();
            // **保活窗**：Tauri 的事件循环在**一扇窗口都没有**时会立刻退出 ⇒ ⇒
            // 探针自己的窗是探针线程建的（而第一件事是**复制 profile**，要好几秒）
            // ⇒ ⇒ 先建一扇 `about:blank` 的不可见窗把事件循环**按住**（它不导航、
            // 不发任何请求，用的是**一份空目录**、跟任何测量臂都不沾边）。
            let keepalive = out_dir.join("keepalive").join("profile");
            let _ = std::fs::create_dir_all(&keepalive);
            if let Err(e) = build_window(&handle, &keepalive, "merge-keepalive") {
                eprintln!("probe: 保活窗建不起来：{e}");
            }
            std::thread::Builder::new()
                .name("official-chat-merge-probe".into())
                .spawn(move || {
                    let runs = tauri::async_runtime::block_on(run_all(
                        &handle,
                        &profile_src,
                        &cold_base,
                        warm_profile.as_deref(),
                        &session_id,
                        RunCounts {
                            cold: cold_runs,
                            warm: warm_runs,
                        },
                        observe_ms,
                    ));

                    let summary = json!({
                        "probe": "只读取证 · 站点什么时候给全量正文（DeepSeek 单会话正文）",
                        "phase": "merge",
                        "path_kind": "壳内路径：壳的 WebView2 ＋ 生产那份 cdp_eval.rs，但跑在**探针 example 进程**里，不是 dev 壳 GUI 进程",
                        "discipline": [
                            "只读：不新建会话、不发消息、不点按钮",
                            "壳侧零请求：只订阅与观察，CDP 调用只有 Network.enable、Network.getResponseBody（＋每段一次的 bogus 对照）",
                            "源 profile 只读：只从源读、绝不写、绝不删；删的只有**副本**里白名单那几个目录",
                            "报告里只有键名、计数、字节数、状态码、布尔与 cache_* 的值：没有正文原文、没有会话标题、没有凭据",
                            "全程不开 DevTools 窗口"
                        ],
                        "session_id": session_id,
                        "profile_src": profile_src.to_string_lossy(),
                        "cold_base": cold_base.to_string_lossy(),
                        "warm_profile": warm_profile.as_ref().map(|p| p.to_string_lossy().to_string()),
                        "cold_runs": cold_runs,
                        "warm_runs": warm_runs,
                        "run_count": runs.len(),
                        "runs": runs,
                        "finished_at_utc": iso_utc(now_ms()),
                    });

                    let summary_path = out_dir.join("reading-merge.json");
                    let _ = std::fs::create_dir_all(&out_dir);
                    let _ = std::fs::write(
                        &summary_path,
                        format!("{}\n", serde_json::to_string_pretty(&summary).unwrap_or_default()),
                    );
                    println!("PROBE_REPORT_BEGIN");
                    println!("summary={}", summary_path.display());
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
    /// 站点**发出**这条请求的时刻（epoch 毫秒）。
    at_ms: u64,
}

/// 一条**命中**正文端点（或目录端点）的响应。
struct Hit {
    /// `body` ＝ 正文那条；`catalog` ＝ 目录那条（**顺带抓，用来核版本号的来源**）。
    kind: String,
    request_id: String,
    method: String,
    path: String,
    query: Vec<(String, String)>,
    status: i64,
    /// ⭐ `response.fromDiskCache`（**这把钥匙**）。
    from_disk_cache: Option<bool>,
    from_service_worker: Option<bool>,
    from_prefetch_cache: Option<bool>,
    mime_type: String,
    protocol: String,
    encoded_data_length: Option<i64>,
    /// 缓存相关的响应头（白名单 ⇒ 没有 `set-cookie`）。
    cache_headers: Value,
    resource_type: String,
    at_ms: u64,
    body_bytes: Option<usize>,
    body_error: Option<String>,
    body_shape: Option<Value>,
}

/// 事件累积状态。
#[derive(Default)]
struct EventState {
    total: u64,
    by_event: BTreeMap<String, u64>,
    setup_errors: Vec<String>,
    finished: BTreeSet<String>,
    req_meta: BTreeMap<String, ReqMeta>,
    api_paths: Vec<String>,
    documents: Vec<String>,
    hits: Vec<Hit>,
}

/// 每一类臂**各跑几轮**（冷臂／温臂）。
#[derive(Clone, Copy)]
struct RunCounts {
    cold: usize,
    warm: usize,
}

/// 跑完三臂，产出每一轮一条读数。
async fn run_all(
    handle: &tauri::AppHandle,
    profile_src: &Path,
    cold_base: &Path,
    warm_profile: Option<&Path>,
    session_id: &str,
    counts: RunCounts,
    observe_ms: u64,
) -> Vec<Value> {
    let RunCounts {
        cold: cold_runs,
        warm: warm_runs,
    } = counts;
    let mut runs: Vec<Value> = Vec::new();

    // --- 臂 A：每轮一份**全新复制**的副本（原样，不删东西） ---
    for index in 0..cold_runs {
        let label = format!("a-cold-copy-{}", index + 1);
        let dir = cold_base
            .join(format!("a-cold-copy-{}", index + 1))
            .join("profile");
        let copy = copy_profile(profile_src, &dir);
        let reading = run_one(handle, &dir, &label, "cold-copy", session_id, observe_ms).await;
        runs.push(assemble(
            label,
            "cold-copy",
            &dir,
            copy,
            Vec::new(),
            reading,
        ));
    }

    // --- 臂 B/E/F：每轮一份全新副本，**再删掉指定的那几样** ---
    //
    // ⚠️ `all`（连 `Local Storage` 一起删）实测会把**登录态**也清掉 ⇒ ⇒ 站点跳
    // **微信扫码登录页** ⇒ ⇒ 那一臂**作废**（没有正文请求，不是因为站点不给）。
    // ⇒ ⇒ 这里改成**一组一组的**删，好把「缓存」与「登录态」两个变量分开。
    for (set_name, arm_label, count) in [
        ("cache-only", "b-cache-only", cold_runs),
        ("indexeddb-only", "c-indexeddb-only", cold_runs),
        ("localstorage-only", "d-localstorage-only", cold_runs),
    ] {
        for index in 0..count {
            let label = format!("{}-{}", arm_label, index + 1);
            let dir = cold_base.join(&label).join("profile");
            let copy = copy_profile(profile_src, &dir);
            let purged = purge_caches(&dir, set_name);
            let reading = run_one(handle, &dir, &label, arm_label, session_id, observe_ms).await;
            runs.push(assemble(label, arm_label, &dir, copy, purged, reading));
        }
    }

    // --- 臂 E：**生产那套白名单**（`ColdProfile::prepare`）⇒ ⇒ 验的就是生产那套
    //
    // ⚠️ 生产侧取正文走的就是这条路（白名单子集 ＋ **不带 `IndexedDB`**）⇒ ⇒
    // 这一臂不是「再来一遍对照」，而是**验生产实现本身**：白名单少了哪一样，
    // 会表现为「登录态掉」或「站点又当成温客户端」。
    for index in 0..cold_runs {
        let label = format!("e-prod-whitelist-{}", index + 1);
        match dsh_xlink::cold_profile::ColdProfile::prepare(profile_src) {
            Ok(cold) => {
                let reading = run_one(
                    handle,
                    cold.dir.as_path(),
                    &label,
                    "prod-whitelist",
                    session_id,
                    observe_ms,
                )
                .await;
                runs.push(assemble(
                    label,
                    "prod-whitelist",
                    cold.dir.as_path(),
                    json!({
                        "copied": true,
                        "note": "生产白名单子集（`ColdProfile::prepare`：Local State / Preferences / Secure Preferences / Local Storage / Network，**不含 IndexedDB**）",
                        "production_path": true
                    }),
                    Vec::new(),
                    reading,
                ));
            }
            Err(e) => {
                runs.push(json!({
                    "label": label,
                    "arm": "prod-whitelist",
                    "fatal": format!("冷档案备不出来：{e}"),
                }));
            }
        }
    }

    // --- 臂 G：同一份 profile 连跑 N 次（**不复制**） ---
    if let Some(dir) = warm_profile {
        for index in 0..warm_runs {
            let label = format!("g-warm-same-{}", index + 1);
            let reading = run_one(handle, dir, &label, "warm-same", session_id, observe_ms).await;
            runs.push(assemble(
                label,
                "warm-same",
                dir,
                json!({ "copied": false, "note": "臂 G 复用同一份 profile，不复制" }),
                Vec::new(),
                reading,
            ));
        }
    }

    runs
}

/// 复制一份 profile（**源只读**），并把**逐个文件的跳过原因**带回来。
fn copy_profile(src: &Path, dst: &Path) -> Value {
    match profile_copy::copy_dir_readonly(src, dst) {
        Ok(report) => json!({
            "copied": true,
            "files": report.files,
            "bytes": report.bytes,
            "skipped_count": report.skipped.len(),
            // **逐个文件记跳过原因**（不静默）。
            "skipped": report.skipped,
            "skip_reason": report.skip_reason,
            "note": "源只读：只从源读、绝不写、绝不删；源被独占时逐个文件记跳过原因"
        }),
        Err(e) => json!({ "copied": false, "error": format!("{e}") }),
    }
}

/// 从**副本**里删掉白名单那几个目录（**只删副本，绝不碰源**）。
///
/// 返回**实际删掉了哪些**（路径 ＋ 原因）⇒ ⇒ 读数里说得清这一轮「冷」到什么程度。
fn purge_caches(profile_dir: &Path, set_name: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let default_dir = profile_dir.join("EBWebView").join("Default");
    for name in purge_set_dirs(set_name) {
        let target = default_dir.join(name);
        if !target.exists() {
            out.push(
                json!({ "path": target.to_string_lossy(), "removed": false, "reason": "不存在" }),
            );
            continue;
        }
        match std::fs::remove_dir_all(&target) {
            Ok(()) => out.push(json!({ "path": target.to_string_lossy(), "removed": true })),
            Err(e) => out.push(json!({ "path": target.to_string_lossy(), "removed": false, "reason": format!("{e}") })),
        }
    }
    out
}

/// 把一轮的读数与它的 profile 事实装到一起。
fn assemble(
    label: String,
    arm: &str,
    profile_dir: &Path,
    copy: Value,
    purged: Vec<Value>,
    reading: Value,
) -> Value {
    json!({
        "label": label,
        "arm": arm,
        "profile_dir": profile_dir.to_string_lossy(),
        "profile_copy": copy,
        "purged_dirs": purged,
        "reading": reading,
    })
}

/// 跑一轮：建窗 → 订阅 → `Network.enable` → 导航到深链 → 等正文那条响应 → 取体。
async fn run_one(
    handle: &tauri::AppHandle,
    profile: &Path,
    label: &str,
    arm: &str,
    session_id: &str,
    observe_ms: u64,
) -> Value {
    let deep_link = format!("{DEEP_LINK_PREFIX}{session_id}");
    let mut out = json!({
        "arm": arm,
        "label": label,
        "session_id": session_id,
        "deep_link": deep_link,
        "started_at_utc": iso_utc(now_ms()),
    });

    let window = match build_window(handle, profile, &format!("merge-{label}")) {
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
    // 对照组：不存在的 CDP 方法必须报错（**每轮各跑一次**）。
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
        out["fatal"] = json!("Network.enable 失败 ⇒ 本轮没跑成");
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
    let deadline = Instant::now() + Duration::from_millis(observe_ms);
    observe(&window, &events, &mut state, deadline, observe_ms).await;

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
    out["history_messages_hits"] = json!(state.hits.iter().map(hit_to_json).collect::<Vec<_>>());
    out["finished_at_utc"] = json!(iso_utc(now_ms()));

    // ⚠️ **不关窗**：Tauri 的事件循环在**最后一扇窗关掉**时会退出 ⇒ ⇒ 关窗等于
    // 把整个探针进程带走（读数还没落盘就没了）。窗留着，进程由 `handle.exit(0)` 收。
    out
}

/// 一条命中 → 读数里的一个对象。
fn hit_to_json(hit: &Hit) -> Value {
    json!({
        "kind": hit.kind,
        "request_id": hit.request_id,
        "method": hit.method,
        "path": hit.path,
        "query": hit.query.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
        "status": hit.status,
        "fromDiskCache": hit.from_disk_cache,
        "fromServiceWorker": hit.from_service_worker,
        "fromPrefetchCache": hit.from_prefetch_cache,
        "mimeType": hit.mime_type,
        "protocol": hit.protocol,
        "encodedDataLength": hit.encoded_data_length,
        "cache_headers": hit.cache_headers,
        "resource_type": hit.resource_type,
        "timestamp_utc": iso_utc(hit.at_ms),
        "body_bytes": hit.body_bytes,
        "body_error": hit.body_error,
        "body_shape": hit.body_shape,
    })
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
        .title("合并语义探针")
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

/// 抽干事件直到截止时间；命中正文端点且**加载完**的响应**立刻取体**。
///
/// 取体只在 `Network.loadingFinished` 之后（协议 §8.5.1 硬约束 1：取早了会**静默**
/// 拿到长度 0 的 body 且不报错 ⇒ ⇒ 空体一律算「回程失败」，不当成「站点没数据」）。
async fn observe(
    window: &tauri::WebviewWindow,
    events: &Receiver<Msg>,
    state: &mut EventState,
    deadline: Instant,
    observe_ms: u64,
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
                        // 只抓**正文**那条与**目录**那条（别的 /api/ 路径不取体）。
                        let kind = if path.contains(HISTORY_PATH) {
                            "body"
                        } else if path.contains(CATALOG_PATH) {
                            "catalog"
                        } else {
                            continue;
                        };
                        let meta = state.req_meta.get(&request_id);
                        state.hits.push(Hit {
                            kind: kind.to_string(),
                            request_id: request_id.clone(),
                            method: meta
                                .map(|m| m.method.clone())
                                .filter(|m| !m.is_empty())
                                .unwrap_or_else(|| "?".to_string()),
                            path,
                            query: url_query(&url),
                            status,
                            // ⭐ **这把钥匙**：站点这条响应是从磁盘缓存拿的，还是真的回了网络。
                            from_disk_cache: response.get("fromDiskCache").and_then(Value::as_bool),
                            from_service_worker: response
                                .get("fromServiceWorker")
                                .and_then(Value::as_bool),
                            from_prefetch_cache: response
                                .get("fromPrefetchCache")
                                .and_then(Value::as_bool),
                            mime_type: response
                                .get("mimeType")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            protocol: response
                                .get("protocol")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            encoded_data_length: response
                                .get("encodedDataLength")
                                .and_then(Value::as_i64),
                            cache_headers: pick_cache_headers(response.get("headers")),
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
                        let kind = state.hits[index].kind.clone();
                        shape = Some(if kind == "catalog" {
                            analyze_catalog(&text)
                        } else {
                            analyze_body(&text)
                        });
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

        // 静默期：已经取到体、且一段时间没有新命中 ⇒ 收工（最多等 observe_ms）。
        if let Some(at) = last_hit_at {
            let has_body = state
                .hits
                .iter()
                .any(|hit| hit.body_bytes.is_some() || hit.body_error.is_some());
            if has_body && at.elapsed() >= Duration::from_millis(observe_ms.min(5_000)) {
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

/// 把**目录**那条响应体解析成**形状读数**（**只有键名、计数、字段类型**）。
///
/// 用途：核「客户端是从哪儿学到的 `cache_version`」—— 若 `chat_sessions` 的条目里
/// **就带着版本号**，那客户端**天生**报上服务端的当前版本 ⇒ ⇒ 增量恒为空。
/// ⚠️ **只报键名与类型**：条目的**值**（标题等）一个都不落。
fn analyze_catalog(body: &str) -> Value {
    let root: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(e) => return json!({ "parse_error": format!("{e}") }),
    };
    let data = root.get("data");
    let biz_data = data.and_then(|d| d.get("biz_data"));
    let list = biz_data
        .and_then(|b| b.get("chat_sessions"))
        .and_then(Value::as_array);

    let mut out = json!({
        "top_level_keys": json!(sorted_keys(Some(&root))),
        "data_keys": json!(sorted_keys(data)),
        "biz_data_keys": json!(sorted_keys(biz_data)),
        "site_code": root.get("code").cloned().unwrap_or(Value::Null),
        "site_biz_code": data.and_then(|d| d.get("biz_code")).cloned().unwrap_or(Value::Null),
        "chat_sessions_len": array_len(biz_data.and_then(|b| b.get("chat_sessions"))),
        "has_more": biz_data.and_then(|b| b.get("has_more")).cloned().unwrap_or(Value::Null),
    });

    match list {
        Some(items) => {
            out["chat_sessions_item_key_sets"] = json!(message_key_sets(items));
            // **第一项的字段类型**（键名 ⇒ 类型）—— 用来核「有没有版本号这个键」。
            out["chat_sessions_first_item_field_types"] = json!(field_types(items.first()));
            out["chat_sessions_element_count"] = json!(items.len());
        }
        None => {
            out["chat_sessions_item_key_sets"] = json!([]);
            out["chat_sessions_first_item_field_types"] = json!({});
            out["chat_sessions_element_count"] = json!(0);
        }
    }
    out
}

/// 一个对象的**键名 ⇒ 值的类型**（**只报类型，不报值**）。
fn field_types(item: Option<&Value>) -> Value {
    let Some(Value::Object(map)) = item else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        out.insert(key.clone(), json!(json_type_of(value)));
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

/// 把响应体解析成**形状读数**（**只有键名、计数、状态码、布尔与 cache_* 的值**）。
fn analyze_body(body: &str) -> Value {
    let root: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(e) => return json!({ "parse_error": format!("{e}") }),
    };
    let data = root.get("data");
    let biz_data = data.and_then(|d| d.get("biz_data"));

    let mut out = json!({
        "top_level_keys": json!(sorted_keys(Some(&root))),
        "data_keys": json!(sorted_keys(data)),
        "biz_data_keys": json!(sorted_keys(biz_data)),
        // 站内码（协议 §6.2）。
        "site_code": root.get("code").cloned().unwrap_or(Value::Null),
        "site_biz_code": data.and_then(|d| d.get("biz_code")).cloned().unwrap_or(Value::Null),
        "site_msg": root.get("msg").and_then(Value::as_str).map(|t| json!(clip(t))).unwrap_or(Value::Null),
        "site_biz_msg": data
            .and_then(|d| d.get("biz_msg"))
            .and_then(Value::as_str)
            .map(|t| json!(clip(t)))
            .unwrap_or(Value::Null),
        // ⭐ 缓存语义那三个字段。
        "cache_control": biz_data.and_then(|b| b.get("cache_control")).cloned().unwrap_or(Value::Null),
        "chat_session_scalars": chat_session_scalars(biz_data),
        // 消息数组：两个候选键都报（**站点的键名是 `chat_messages`**）。
        "chat_messages_len": array_len(biz_data.and_then(|b| b.get("chat_messages"))),
        "messages_len_at_biz_data": array_len(biz_data.and_then(|b| b.get("messages"))),
    });

    // 消息数组的元素形状（**只有键名与 role/type 取值** —— 一个正文值都不落）。
    let list = biz_data
        .and_then(|b| b.get("chat_messages"))
        .and_then(Value::as_array)
        .or_else(|| {
            biz_data
                .and_then(|b| b.get("messages"))
                .and_then(Value::as_array)
        });
    match list {
        Some(items) => {
            out["message_key_sets"] = json!(message_key_sets(items));
            out["role_values"] = json!(field_value_set(items, "role"));
            out["type_values"] = json!(field_value_set(items, "type"));
            out["elements_with_role"] =
                json!(items.iter().filter(|i| i.get("role").is_some()).count());
            out["elements_with_type"] =
                json!(items.iter().filter(|i| i.get("type").is_some()).count());
        }
        None => {
            out["message_key_sets"] = json!([]);
            out["role_values"] = json!([]);
            out["type_values"] = json!([]);
            out["elements_with_role"] = json!(0);
            out["elements_with_type"] = json!(0);
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

/// 某个值是不是数组（是就给长度，不是就给 `null`）。
fn array_len(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Array(items)) => json!(items.len()),
        _ => Value::Null,
    }
}

/// `chat_session` 里的**标量**（**`title` 不落** —— 会话标题，纪律不许落）。
fn chat_session_scalars(biz_data: Option<&Value>) -> Value {
    let Some(session) = biz_data.and_then(|b| b.get("chat_session")) else {
        return Value::Null;
    };
    let Some(map) = session.as_object() else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        if key == "title" {
            continue;
        }
        match value {
            Value::Array(_) | Value::Object(_) => {}
            Value::String(text) => {
                out.insert(key.clone(), json!(clip(text)));
            }
            other => {
                out.insert(key.clone(), other.clone());
            }
        }
    }
    Value::Object(out)
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

/// 截断一段站点消息（**状态信息**，不是正文；限长防意外带出长串）。
fn clip(text: &str) -> String {
    const LIMIT: usize = 120;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let head: String = text.chars().take(LIMIT).collect();
    format!("{head}…（已截断，原长 {} 字符）", text.chars().count())
}

/// 取响应头里的**缓存相关那几个**（白名单 ⇒ ⇒ 不会有 `set-cookie`、不会有凭据）。
fn pick_cache_headers(headers: Option<&Value>) -> Value {
    let Some(Value::Object(map)) = headers else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    for (name, value) in map {
        if !CACHE_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        let text = match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        out.insert(name.clone(), json!(clip(&text)));
    }
    Value::Object(out)
}

/// 取 URL 的路径部分。
fn url_path(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => parsed.path().to_string(),
        Err(_) => url.to_string(),
    }
}

/// 取 URL 的 query（**键名全留**；值：三个缓存/会话键**原样留**，其余长串打码）。
fn url_query(url: &str) -> Vec<(String, String)> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Vec::new();
    };
    parsed
        .query_pairs()
        .map(|(key, value)| {
            let key = key.to_string();
            let keep_verbatim = matches!(
                key.as_str(),
                "chat_session_id" | "cache_version" | "cache_reset_at"
            );
            let value = if keep_verbatim || value.len() <= 32 {
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
