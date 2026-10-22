//! P4 补测单 · **WebView2 DevTools 协议 Network 域**：三格未验的封口探针（只读）。
//!
//! # 这一版（v3）相对上一版改了什么
//!
//! 上一版（`52bed5d`）留了三个未验格，这一版只针对它们：
//!
//! 1. **格①（大缓冲能否避免淘汰）**：上一版那一格**失败**，但失败更可能是**探针自身
//!    的缺陷** —— reload 之前的旧事件被当成新事件 ⇒ 取了旧 `requestId` ⇒ 这一版的
//!    做法是：**① 按 `loaderId` 过滤事件**（新导航 ⇒ 新 `loaderId` ⇒ 旧文档的事件
//!    一律不算）＋ **② 本版不再 reload**（每个 arm 用自己的冷 profile，避免跨文档
//!    事件流）＋ **③ 对同一条 `requestId` 做「立即取 vs 拖长后取」两个分支**，各记
//!    sha256 ⇒ 既答「同一个响应体」，又答「晚取还在不在」。
//! 2. **格②（Document 类型 body 的边界）**：对同一站点页面，**Document / XHR / Fetch / Script**
//!    四类 `resourceType` **逐候选取体**（每类最多 6 个候选）⇒ 逐类记
//!    「取到 / 取不到 ＋ 错误原文 ＋ 字节长度」。
//! 3. **格③（Network 域以外的域）**：**不做**（PM 17:2x 裁定）⇒ 保持「未验」登记。
//!
//! **v3 相对 v2 的两处订正（都是 v2 跑完后实测出来的探针缺陷）**：
//!
//! - **格② 改成「每类逐候选取体」**（每类最多 6 个候选）—— v2「每类只取第一个请求」
//!   会选中被取消 / 被重定向 / preflight 的那个候选 ⇒ 取体必然报 `参数错误` ⇒
//!   **凭空造出「该类不可取」的假结论** ⇒ ⇒ 逐候选试才是该类**真正**的边界。
//! - **观察窗口可配**（`--observe-ms`，默认 180 s）—— v2 写死 60 s，会在站点还没走完
//!   WeChat OAuth（跳到微信扫码页再自动回跳）时就收工 ⇒ **观察不到 `fetch_page`**。
//!
//! # 上一版已成立的结论（本版继续复现，**一字不改**）
//!
//! `Network.enable` 返回 `{}`｜bogus 方法报 `0x80070057`（对照组）｜三事件订阅零错误｜
//! **按事件名订阅**（本机 SDK 无 `Subscribe`）｜**载荷无 `method` 字段**｜
//! 取体须等 `loadingFinished`｜登录态字段面与协议 §4.3 逐项对上。
//!
//! ⚠️ **其中「取体须等 `loadingFinished`」有一条本轮实测补充**（v3 读数）：
//! 某 XHR **没有** `loadingFinished` 记录、`getResponseBody` **照样取得到**（95 B）⇒
//! ⇒ 那条**不是**所有资源类型的**必要条件**（只能当「取体更稳的充分经验」）。
//! **协议 §8.5 那条表述是否要同步订正，归 PM**，本探针只如实记读数。
//!
//! # 头注沿革
//!
//! `52bed5d` 那版头注里的「**支持度未验**」与「**API 调用在页内**」两句，
//! 已随 v2/v3 的整段头注重写**一并移除**（形态定案见协议 v0.10/v0.11：
//! **壳侧 DevTools `Network` 域订阅**、页内零动作）。
//!
//! ⚠️ **这份探针的主体是 PM 线写的真壳版**（`tauri::test::mock_context` ＋ `#[path]`
//! 编**生产那份** `cdp_eval.rs` ＋ 生产 `OFFICIAL_CHAT_BROWSER_ARGS` ＋ 生产 profile
//! 推导）⇒ ⇒ **`--mode trace` 臂由 tester 为判据第 2 条真机轨所加**
//! （2026-10-21 19:3x）—— 那一臂只做「跑前快照 → 真跑一轮壳侧订阅 → 跑后快照」
//! ＋ 一个**不订阅**的对照组窗口，产出判据 `judgeTrace` 的读数契约
//! （`{before, after, baseline:{before, after}}`）⇒ **判据本体一个字未改**。
//! 防漂移：探针里那份快照脚本抄本与判据导出**逐字相等**，由
//! `scripts/check-official-chat-p4-trace.test.mjs` 里的跨文件自测机器拦。
//!
//! # 纪律
//!
//! - **只读**：不新建会话、不发消息、不点任何按钮（探针本身**零写请求**）。
//! - **不落盘任何响应体正文**：只留**字节长度 ＋ SHA-256 ＋ 结构（键名）**。
//! - **profile 由命令行显式给**（源码里不写产品 profile 目录名 —— 仓库硬不变量
//!   `scripts/lib/probe-profile-isolation.mjs`），根在**一次性临时目录** ⇒ 不碰用户真实数据目录。
//! - **URL 里的 uuid / 长 token 串打码**；`loaderId` / `requestId` 也打码。
//! - **全程不开 DevTools 窗口**（只走协议调用）。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_devtools_network_probe -- \
//!   --profile "<一次性临时目录下的 user-data 目录>" \
//!   --out "<读数文件路径>" \
//!   --arm default|bigbuffer \
//!   --late-wait-ms 60000 \
//!   --observe-ms 180000
//! ```

//! # 平台边界（**Windows 专用**）
//!
//! 这个探针整条链都建立在 **WebView2 的 DevTools 协议**上（`webview2_com` /
//! `windows_core`）⇒ ⇒ **Windows 专用**：别的平台上这两个 crate 根本不存在
//! ⇒ ⇒ 非 Windows 上这一整块不存在（见文件末尾那个非 Windows 的 `main`）。
//!
//! ⚠️ 因此非 Windows 的构建里，本文件**多数**函数没有生产调用方（只有本文件的
//! `main` 用得到，而它也被门掉了）⇒ 那边的「未被调用」是**正常**的
//! ⇒ ⇒ 模块级豁免死代码警告（**只豁免非 Windows 那边**；Windows 那边的构建
//! 才是这条链的严格检查面）。
#![cfg_attr(not(windows), allow(dead_code))]

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
        // ⚠️ **这两个最小实现是刻意留的** —— `#[path]` 进来的生产源码会调到它们
        // （example 是另一个 crate，没有壳自己的模块）⇒ ⇒ 「当前这个 example
        // 用不到」是常态 ⇒ ⇒ 不许因此删掉它们（删了生产那份源码就编不过）。
        #[allow(dead_code)]
        pub fn window_title(name: &str) -> String {
            format!("{name} — dsh-xlink")
        }
    }
    pub mod appearance {
        use tauri::Theme;
        #[allow(dead_code)]
        pub fn initial_theme(_label: &str) -> Option<Theme> {
            None
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
/// Windows 专用链才用得到（`main` 把命令行给的路径转成 `PathBuf`）。
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 探测的站点首页。
const SITE_URL: &str = "https://chat.deepseek.com";

/// 协议 §4.3 关心的那个端点。
const FETCH_PAGE_PATH: &str = "/api/v0/chat_session/fetch_page";

/// 订阅的事件（**按事件名**订阅，官方 API 形态）。
const SUBSCRIBE_EVENTS: &[&str] = &[
    "Network.responseReceived",
    "Network.loadingFinished",
    "Network.requestWillBeSent",
];

/// 格② 要逐类试的 `resourceType`。
const BODY_TYPES: &[&str] = &["Document", "XHR", "Fetch", "Script"];

/// 大缓冲 arm 的参数值。
///
/// 默认缓冲是 Chromium 的 ~10 MB 量级；页面首屏光 JS chunk 就能把它灌满 ⇒
/// 这几档取「远大于首屏资源总量」的值（首屏总量在读数里由 `encoded_total_bytes` 给出）。
const BIG_MAX_TOTAL_BUFFER: u64 = 268_435_456; // 256 MB
const BIG_MAX_RESOURCE_BUFFER: u64 = 67_108_864; // 64 MB

/// 观察窗口上限（等 fetch_page 与四类 resourceType 都到齐）的**默认值**。
///
/// 冷 profile 下站点会先走一遍 WeChat OAuth（要跳到 `open.weixin.qq.com` 再自动
/// 回跳），整个流程可能超过一分钟 ⇒ 窗口太短会「还没登录完就收工」⇒ 观察不到
/// `fetch_page`。故窗口可由 `--observe-ms` 覆盖。
const OBSERVE_WINDOW: Duration = Duration::from_secs(180);

/// 每种 `resourceType` 最多记多少个候选请求（取体时逐个试）。
const MAX_CANDIDATES_PER_TYPE: usize = 6;

/// **页内零副作用快照脚本**（判据第 2 条真机轨用的那段）。
///
/// ⚠️ **逐字抄自** `scripts/check-official-chat-p4-trace.mjs` 的 `TRACE_SNAPSHOT_JS`
/// （该文件 :88-98）—— 必须逐字一致，否则测的就不是判据那一段脚本。
/// 它是 IIFE、**不建任何全局量**，只回结构（属性名数组 ＋ 函数文本）。
const TRACE_SNAPSHOT_JS: &str = r#"(() => {
  const own = (o) => Object.getOwnPropertyNames(o);
  const text = (f) => (typeof f === 'function' ? Function.prototype.toString.call(f) : null);
  const proto = globalThis.XMLHttpRequest ? globalThis.XMLHttpRequest.prototype : null;
  return {
    windowProps: own(globalThis),
    protoProps: proto ? own(proto) : null,
    openText: proto ? text(proto.open) : null,
    sendText: proto ? text(proto.send) : null,
  };
})()"#;

/// 探针从站里收到的两类消息。
enum Msg {
    /// 一条 DevTools 协议事件（事件名 ＋ 载荷原文 JSON）。
    Event { name: String, raw: String },
    /// 订阅/接线阶段的错误。
    Setup(String),
}

/// 一条候选请求（取体时逐个试；`request_id` 原样，打码只在输出时做）。
#[derive(Clone)]
struct RequestEntry {
    request_id: String,
    url: String,
}

/// 事件累积状态。
#[derive(Default)]
struct EventState {
    total: u64,
    /// **被 `loaderId` 过滤掉**的事件数（证明过滤器真的在起作用）。
    filtered_out: u64,
    unparsable: u64,
    by_event: BTreeMap<String, u64>,
    by_loader: BTreeMap<String, u64>,
    setup_errors: Vec<String>,
    first_event_shape: Option<Value>,
    /// 当前文档的 `loaderId`（由 Document 类事件更新）。
    current_loader: Option<String>,
    /// 观察到的资源总字节（`encodedDataLength` 之和）。
    encoded_total: u64,
    /// 已完成的请求（`loadingFinished` 到的 `requestId`）。
    finished: BTreeSet<String>,
    /// 每种 `resourceType` 第一个命中的 `requestId`。
    request_by_type: BTreeMap<String, String>,
    /// 每种 `resourceType` 第一个命中的请求的 URL（已打码）。
    url_by_type: BTreeMap<String, String>,
    /// 每种 `resourceType` 的**全部候选**请求（取体时逐个试）。
    ///
    /// 「只取每类第一个」太脆：第一个可能是被取消/被重定向/preflight 的请求，
    /// 取体必然报 `参数错误`；逐个试才能给出该类**真正**的边界。
    candidates_by_type: BTreeMap<String, Vec<RequestEntry>>,
    /// 观察到的所有 Document 类请求 URL（打码，最多 12 条）——用来看清
    /// 「这一轮停在哪个页面」（例如卡在微信扫码页就没回到站点）。
    document_urls: Vec<String>,
    /// 站点自己发的 `fetch_page` 响应（打码后的结构）。
    fetch_page_hit: Option<Value>,
    /// 站点自己发的 `fetch_page` 的 `requestId`（原样，取体要用）。
    fetch_page_request_id: Option<String>,
}

/// **Windows 专用**（整条链建立在 WebView2 的 DevTools 协议上）。
#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

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
    // 格① 的两个 arm：`default`（不带缓冲参数）/ `bigbuffer`（带大缓冲参数）。
    let arm = arg_value(&args, "--arm").unwrap_or_else(|| "default".to_string());
    // 「拖长后取」要等多久（毫秒）。
    let late_wait_ms: u64 = arg_value(&args, "--late-wait-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(60_000);
    // 观察窗口：等站点把 `fetch_page` 与四类 `resourceType` 都发出来。
    let observe_ms: u64 = arg_value(&args, "--observe-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(OBSERVE_WINDOW.as_millis() as u64);
    // 跑哪一套：`gaps`（补测三格，默认）/ `trace`（判据第 2 条真机轨）。
    let mode = arg_value(&args, "--mode").unwrap_or_else(|| "gaps".to_string());
    // trace 臂：等页面稳定多久 / 「跑一轮订阅」持续多久。
    let settle_ms: u64 = arg_value(&args, "--settle-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(25_000);
    let hold_ms: u64 = arg_value(&args, "--hold-ms")
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(5_000);

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile = profile.clone();
            let out = out.clone();
            let url_text = url_text.clone();
            let arm = arm.clone();
            std::thread::Builder::new()
                .name("official-chat-devtools-network-probe".into())
                .spawn(move || {
                    let report = if mode == "trace" {
                        tauri::async_runtime::block_on(run_trace(
                            &handle, &profile, &url_text, settle_ms, hold_ms,
                        ))
                    } else {
                        tauri::async_runtime::block_on(run_probe(
                            &handle,
                            &profile,
                            &url_text,
                            &arm,
                            late_wait_ms,
                            observe_ms,
                        ))
                    };
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

/// **Windows 专用**（它要 WebView2 的 DevTools 通路 —— 别的平台上这条链不存在）。
#[cfg(not(windows))]
fn main() {
    eprintln!(
        "official_chat_devtools_network_probe 只支持 Windows（它要 WebView2 的 DevTools 通路）"
    );
    std::process::exit(2);
}

/// 跑一次探针，出 JSON 读数。
#[cfg(windows)]
async fn run_probe(
    handle: &tauri::AppHandle,
    profile: &Path,
    url_text: &str,
    arm: &str,
    late_wait_ms: u64,
    observe_ms: u64,
) -> String {
    let mut report = json!({
        "probe": "official-chat · WebView2 DevTools Network 域 · 补测三格（只读）",
        "arm": arm,
        "site_url": url_text,
        "observe_ms": observe_ms,
        "late_wait_ms": late_wait_ms,
        "discipline": [
            "只读：探针本身零写请求（不新建会话、不发消息、不点按钮）",
            "不落盘响应体正文：只留字节长度 + SHA-256 + 结构（键名）",
            "profile 由命令行显式给、根在一次性临时目录 ⇒ 不碰用户真实数据目录",
            "URL / loaderId / requestId 里的随机串一律打码",
            "全程不开 DevTools 窗口（只走协议调用）"
        ]
    });

    // --- profile 目录：调用方显式给（源码里不写产品 profile 目录名） ---
    let _ = std::fs::create_dir_all(profile);
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

    let window = match build_window(handle, profile, "official-chat-devtools-network-probe") {
        Ok(window) => window,
        Err(e) => {
            report["fatal"] = json!(format!("无法创建探测窗口：{e}"));
            return report.to_string();
        }
    };

    // --- 订阅（必须在导航之前） ---
    let events = subscribe_devtools_events(&window);

    // --- Network.enable（按 arm 决定带不带缓冲参数） ---
    let enable_params = match arm {
        "bigbuffer" => json!({
            "maxTotalBufferSize": BIG_MAX_TOTAL_BUFFER,
            "maxResourceBufferSize": BIG_MAX_RESOURCE_BUFFER
        })
        .to_string(),
        "default" => "{}".to_string(),
        other => {
            report["fatal"] = json!(format!("未知 arm：{other}（只认 default / bigbuffer）"));
            return report.to_string();
        }
    };
    let enable = call_cdp(&window, "Network.enable", &enable_params).await;
    report["network_enable"] = json!({
        "arm": arm,
        "params": enable_params.clone(),
        "max_total_buffer_bytes": if arm == "bigbuffer" { BIG_MAX_TOTAL_BUFFER } else { 0 },
        "max_resource_buffer_bytes": if arm == "bigbuffer" { BIG_MAX_RESOURCE_BUFFER } else { 0 },
        "result": match &enable {
            Ok(raw) => json!({ "ok": true, "raw_result": raw }),
            Err(e) => json!({ "ok": false, "error": e }),
        }
    });

    // --- 对照组：不存在的 CDP 方法必须报错（否则上面的 ok 不能当证据） ---
    let control = call_cdp(&window, "Network.thisMethodDoesNotExist", "{}").await;
    report["control_bogus_cdp_method"] = match &control {
        Ok(raw) => json!({
            "ok": true,
            "raw_result": raw,
            "note": "对照组异常：不存在的 CDP 方法居然成功 ⇒ network_enable 的 ok 不能当证据"
        }),
        Err(e) => json!({
            "ok": false,
            "error": e,
            "note": "对照组正常：不存在的 CDP 方法确实报错 ⇒ network_enable 的 ok 是真成功"
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

    // --- 观察：等站点的 fetch_page 与四类 resourceType 到齐 ---
    let mut state = EventState::default();
    let observe_deadline = Instant::now() + Duration::from_millis(observe_ms);
    observe(&events, &mut state, observe_deadline, |st| {
        let fetch_ready = st
            .fetch_page_request_id
            .as_ref()
            .map(|rid| st.finished.contains(rid))
            .unwrap_or(false);
        let types_ready = BODY_TYPES
            .iter()
            .filter(|kind| st.request_by_type.contains_key(**kind))
            .count();
        fetch_ready && types_ready >= 3
    });

    report["events"] = json!({
        "subscribed_events": SUBSCRIBE_EVENTS,
        "subscribe_setup_errors": state.setup_errors.clone(),
        "event_total": state.total,
        "filtered_out_by_loader_id": state.filtered_out,
        "unparsable": state.unparsable,
        "by_event": state.by_event.clone(),
        "by_loader_id": state.by_loader.clone(),
        "current_loader_id": state.current_loader.clone().map(|id| mask_id(&id)),
        "encoded_total_bytes": state.encoded_total,
        "loading_finished_count": state.finished.len(),
        "first_event_shape": state.first_event_shape.clone(),
        "document_urls": state.document_urls.clone(),
        "fetch_page_observed": state.fetch_page_hit.is_some(),
        "note": "loaderId 过滤：只收「当前文档」的事件，其余计入 filtered_out_by_loader_id"
    });

    // --- 格① 的前半：同一条 fetch_page 的 requestId，**立刻**取 ---
    let fetch_page_request_id = state.fetch_page_request_id.clone();
    let early = match fetch_page_request_id.clone() {
        Some(rid) => fetch_body(&window, "early_fetch_page", &rid).await,
        None => None,
    };
    report["g1_early_fetch"] = early.unwrap_or(Value::Null);

    // --- 格②：四类 resourceType 逐类取 body ---
    //
    // 每类有多个候选请求（第一个可能是被取消/重定向/preflight 的）⇒ 逐个试，
    // 记下**每一个**候选的「取到/取不到 + 错误原文 + 字节长度」，
    // 类型结论取第一个成功者；全失败则该类结论为「取不到」。
    let mut type_report = Vec::new();
    for kind in BODY_TYPES {
        let candidates = state
            .candidates_by_type
            .get(*kind)
            .cloned()
            .unwrap_or_default();
        if candidates.is_empty() {
            type_report.push(json!({
                "resource_type": kind,
                "seen": false,
                "note": "本轮没观察到这一类 resourceType"
            }));
            continue;
        }
        let mut attempts = Vec::new();
        let mut chosen: Option<Value> = None;
        for (index, entry) in candidates.iter().enumerate() {
            let finished = state.finished.contains(&entry.request_id);
            let outcome =
                fetch_body(&window, &format!("type_{kind}#{index}"), &entry.request_id).await;
            let ok = outcome
                .as_ref()
                .and_then(|v| v.get("ok"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            attempts.push(json!({
                "index": index,
                "url": mask_url(&entry.url),
                "request_id": mask_id(&entry.request_id),
                "loading_finished_seen": finished,
                "body": outcome
            }));
            if ok && chosen.is_none() {
                chosen = Some(json!({
                    "index": index,
                    "url": mask_url(&entry.url),
                    "loading_finished_seen": finished
                }));
            }
        }
        type_report.push(json!({
            "resource_type": kind,
            "seen": true,
            "candidate_count": candidates.len(),
            "fetchable": chosen.is_some(),
            "chosen": chosen,
            "attempts": attempts,
            "note": "取到 = 至少一个候选请求的 getResponseBody 成功；逐候选的原始错误见 attempts"
        }));
    }
    report["g2_body_by_resource_type"] = json!({
        "attempts": type_report,
        "note": "同一站点页面上的四类 resourceType；只报「取到/取不到 + 错误原文 + 字节长度 + SHA-256」"
    });

    // --- 格① 的后半：**拖长**后取**同一条** requestId ---
    tokio::time::sleep(Duration::from_millis(late_wait_ms)).await;
    let late = match fetch_page_request_id.clone() {
        Some(rid) => fetch_body(&window, "late_fetch_page", &rid).await,
        None => None,
    };
    report["g1_late_fetch"] = json!({
        "waited_ms": late_wait_ms,
        "request_id": fetch_page_request_id.as_deref().map(mask_id),
        "body": late,
        "note": "与 g1_early_fetch 是**同一条 requestId**；两次的 sha256 相同 ⇒ 取到的是同一个响应体"
    });

    report.to_string()
}

/// 在页面里求值一次 [`TRACE_SNAPSHOT_JS`]，取回快照对象。
///
/// 走**生产那份** `cdp_eval`（`Runtime.evaluate` ＋ `returnByValue`）—— 协议 §1.2
/// 规定的唯一观测通路（**页内求值只用于量痕迹面，不进数据通路**）。
async fn snapshot(window: &tauri::WebviewWindow) -> Result<Value, String> {
    eval_value(window, TRACE_SNAPSHOT_JS).await
}

/// 在页面里求值一段表达式，取回 `result.value`。
async fn eval_value(window: &tauri::WebviewWindow, expression: &str) -> Result<Value, String> {
    let raw = cdp_eval::cdp_eval(window, expression, Duration::from_secs(30))
        .await
        .map_err(|e| format!("页内求值失败：{e:?}"))?;
    let (value, err) = cdp_eval::unwrap_cdp_result(&raw);
    if let Some(detail) = err {
        return Err(format!("页内求值回执里有异常：{detail}"));
    }
    value.ok_or_else(|| "页内求值回执里没有 value".to_string())
}

/// **判据第 2 条（零痕迹）的真机轨**：跑一轮真机，产出 `readings`。
///
/// 流程（每个窗口各自独立，同一个 profile、同一个站点页形态）：
///
/// 1. 建窗口 → **不订阅**（订阅是本轮**被测的那个动作**，不能提前发生）；
/// 2. 导航到站点 → 等页面稳定（`settle_ms`）；
/// 3. 取**跑前**快照；
/// 4. **实测组**：按事件名订阅 ＋ `Network.enable`（＝真跑一轮壳侧订阅）→ 等 `hold_ms`；
///    **对照组**：什么都不做，只等同样长的 `hold_ms`（用来扣站点自然增长）；
/// 5. 取**跑后**快照。
///
/// ⇒ 实测组 before/after 是**同一个 tab 的同一页**，对照组 likewise；
/// 差集（扣掉对照组自然增长）就是「壳侧订阅动作在页内留下的痕迹」。
/// **Windows 专用**（它要 WebView2 的 DevTools 通路 —— 别的平台上这条链不存在）。
#[cfg(windows)]
async fn run_trace(
    handle: &tauri::AppHandle,
    profile: &Path,
    url_text: &str,
    settle_ms: u64,
    hold_ms: u64,
) -> String {
    let mut report = json!({
        "probe": "official-chat · P4 判据第 2 条（零痕迹）· 真机轨",
        "site_url": url_text,
        "settle_ms": settle_ms,
        "hold_ms": hold_ms,
        "discipline": [
            "只读：不新建会话、不发消息、不点任何按钮",
            "快照只回结构（属性名数组 ＋ 函数文本），不回任何值/正文/凭据",
            "profile 由命令行显式给、根在一次性临时目录 ⇒ 不碰用户真实数据目录",
            "全程不开 DevTools 窗口（只走协议调用）"
        ]
    });

    let _ = std::fs::create_dir_all(profile);

    let url: tauri::Url = match url_text.parse() {
        Ok(url) => url,
        Err(e) => {
            report["fatal"] = json!(format!("URL 非法：{e}"));
            return report.to_string();
        }
    };

    let measure = match build_window(handle, profile, "p4-trace-measure") {
        Ok(window) => window,
        Err(e) => {
            report["fatal"] = json!(format!("无法创建实测组窗口：{e}"));
            return report.to_string();
        }
    };
    let control = match build_window(handle, profile, "p4-trace-control") {
        Ok(window) => window,
        Err(e) => {
            report["fatal"] = json!(format!("无法创建对照组窗口：{e}"));
            return report.to_string();
        }
    };

    // 两个窗口都导航到同一个站点页（对照组同样要有页面，否则扣不掉自然增长）
    for (label, window) in [("measure", &measure), ("control", &control)] {
        match window.navigate(url.clone()) {
            Ok(()) => report["navigate"] = json!({ label: "ok" }),
            Err(e) => {
                report["fatal"] = json!(format!("{label} 窗口导航失败：{e}"));
                return report.to_string();
            }
        }
    }

    // 等页面稳定（首屏加载完、站点自己的初始化跑完）
    tokio::time::sleep(Duration::from_millis(settle_ms)).await;

    // 落点 URL（**在跑前快照之前**取 ⇒ 不影响 before/after 差集）—— 用来如实说明
    // 「这一轮站在哪个页面上」（未登录态可能被 OAuth 带去微信授权页）。
    for (label, window) in [("measure", &measure), ("control", &control)] {
        let href = eval_value(window, "location.href").await;
        report[format!("{label}_landing_url")] = match &href {
            Ok(value) => json!(mask_url(value.as_str().unwrap_or_default())),
            Err(e) => json!(format!("取不到落点 URL：{e}")),
        };
    }

    // --- 实测组：跑前快照 ---
    let before = snapshot(&measure).await;
    report["measure_before_error"] = match &before {
        Ok(_) => Value::Null,
        Err(e) => json!(e),
    };

    // --- 实测组：真跑一轮壳侧订阅（**被测动作**） ---
    let _events = subscribe_devtools_events(&measure);
    let enable = call_cdp(&measure, "Network.enable", "{}").await;
    report["measure_network_enable"] = match &enable {
        Ok(raw) => json!({ "ok": true, "raw_result": raw }),
        Err(e) => json!({ "ok": false, "error": e }),
    };
    tokio::time::sleep(Duration::from_millis(hold_ms)).await;

    // --- 实测组：跑后快照 ---
    let after = snapshot(&measure).await;
    report["measure_after_error"] = match &after {
        Ok(_) => Value::Null,
        Err(e) => json!(e),
    };

    // --- 对照组：同样等，但**不订阅、不 enable** ---
    let ctrl_before = snapshot(&control).await;
    tokio::time::sleep(Duration::from_millis(hold_ms)).await;
    let ctrl_after = snapshot(&control).await;

    report["readings"] = json!({
        "before": before.unwrap_or(Value::Null),
        "after": after.unwrap_or(Value::Null),
        "baseline": {
            "before": ctrl_before.unwrap_or(Value::Null),
            "after": ctrl_after.unwrap_or(Value::Null)
        }
    });
    report["note"] = json!(
        "readings 直接喂 `DSH_P4_TRACE_READINGS` 指向的 JSON 文件 ⇒ 判据第 2 条的真机轨会真判"
    );

    report.to_string()
}

/// 建探针窗口：先开在 `about:blank`，等订阅与 `Network.enable` 都完成后才导航。
fn build_window(
    handle: &tauri::AppHandle,
    profile: &Path,
    label: &str,
) -> Result<tauri::WebviewWindow, String> {
    let url: tauri::Url = "about:blank"
        .parse()
        .map_err(|e| format!("URL 非法：{e}"))?;
    tauri::WebviewWindowBuilder::new(handle, label, tauri::WebviewUrl::External(url))
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
/// **Windows 专用**（`webview2_com` 是 WebView2 的 COM 绑定 —— 别的平台上
/// 这个 crate 根本不存在）。
#[cfg(windows)]
fn subscribe_devtools_events(window: &tauri::WebviewWindow) -> Receiver<Msg> {
    use webview2_com::DevToolsProtocolEventReceivedEventHandler;
    use windows_core::HSTRING;

    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    // 外层闭包是 `move` 的，会把 sender 带走 ⇒ 闭包里用拷贝，外层留一份。
    let tx_outer = tx.clone();
    // 闭包是 move 的 ⇒ 闭包内用一份拷贝，闭包外留一份。
    let tx_in_closure = tx_outer.clone();
    let dispatched = window.with_webview(move |platform| {
        let controller = platform.controller();
        // SAFETY: `CoreWebView2` 只是取回控制器持有的核心对象引用，不转移
        // 所有权；返回值由 COM 引用计数管理。
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                let _ = tx_in_closure.send(Msg::Setup(format!("取不到 WebView2 核心：{e}")));
                return;
            }
        };
        for event_name in SUBSCRIBE_EVENTS {
            // 每个订阅一份 sender：事件处理函数是 `'static` 的闭包，会把
            // sender 带走，所以循环里必须各拿一份拷贝。
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

/// 调一次 DevTools 协议方法，返回**原始结果文本**。
///
/// **Windows 专用**（`webview2_com` 是 WebView2 的 COM 绑定 —— 别的平台上
/// 这个 crate 根本不存在）。
#[cfg(windows)]
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
/// **按 `loaderId` 过滤**：只有「当前文档」的事件才算数；旧文档（reload 前）
/// 的事件计入 `filtered_out` ⇒ 这就是上一版那个缺陷的修法。
fn observe<F>(events: &Receiver<Msg>, state: &mut EventState, deadline: Instant, mut stop: F)
where
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
                let Some(value) = serde_json::from_str::<Value>(&raw).ok() else {
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
                let loader = value
                    .get("loaderId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let resource_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let request_id = value
                    .get("requestId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                // --- loaderId 过滤 ---
                let is_document = resource_type == "Document";
                if is_document {
                    // 文档事件一律收，并把它当作「当前文档」。
                    if !loader.is_empty() {
                        state.current_loader = Some(loader.clone());
                    }
                } else {
                    match &state.current_loader {
                        Some(current) if !loader.is_empty() && *current != loader => {
                            state.filtered_out += 1;
                            continue;
                        }
                        None if !loader.is_empty() => {
                            state.current_loader = Some(loader.clone());
                        }
                        _ => {}
                    }
                }

                *state.by_event.entry(name.clone()).or_insert(0) += 1;
                if !loader.is_empty() {
                    *state.by_loader.entry(mask_id(&loader)).or_insert(0) += 1;
                }

                match name.as_str() {
                    "Network.requestWillBeSent" => {
                        let url = value
                            .get("request")
                            .and_then(|request| request.get("url"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if resource_type == "Document" && state.document_urls.len() < 12 {
                            state.document_urls.push(mask_url(&url));
                        }
                        record_type(state, &resource_type, &request_id, &url);
                    }
                    "Network.responseReceived" => {
                        if let Some(response) = value.get("response") {
                            if let Some(len) =
                                response.get("encodedDataLength").and_then(Value::as_u64)
                            {
                                state.encoded_total += len;
                            }
                            let url = response
                                .get("url")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            record_type(state, &resource_type, &request_id, &url);
                            if url.contains(FETCH_PAGE_PATH) && state.fetch_page_hit.is_none() {
                                let status = response.get("status").cloned().unwrap_or(Value::Null);
                                state.fetch_page_hit = Some(json!({
                                    "url": mask_url(&url),
                                    "status": status,
                                    "mime_type": response.get("mimeType").cloned().unwrap_or(Value::Null),
                                    "resource_type": resource_type.clone(),
                                    "request_id": mask_id(&request_id),
                                    "loader_id": mask_id(&loader)
                                }));
                                state.fetch_page_request_id = Some(request_id.clone());
                            }
                        }
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

/// 记一条「某类 `resourceType` 的命中」。
///
/// **Document 类**：只认站点自己的文档（`chat.deepseek.com`），排除
/// ① OAuth 回调文档（`/api/v0/...`）② 微信授权页（`open.weixin.qq.com`）
/// —— 两者都不是「我们要测的页面文档」；`request_by_type` 取**最后一个**
/// （页面最后一次加载）。
///
/// **其它类**：`request_by_type` 取第一个（兼容旧口径），但
/// `candidates_by_type` 记**全部**候选 —— 第一个候选常是被取消/重定向/
/// preflight 的请求，取体必然报 `参数错误`，逐个试才能给出真实边界。
fn record_type(state: &mut EventState, resource_type: &str, request_id: &str, url: &str) {
    if request_id.is_empty() || resource_type.is_empty() {
        return;
    }
    if resource_type == "Document" {
        if url.contains("/api/v0/") || !url.contains("chat.deepseek.com") {
            return;
        }
        state
            .request_by_type
            .insert(resource_type.to_string(), request_id.to_string());
        state
            .url_by_type
            .insert(resource_type.to_string(), mask_url(url));
        let entry = RequestEntry {
            request_id: request_id.to_string(),
            url: url.to_string(),
        };
        let bucket = state
            .candidates_by_type
            .entry(resource_type.to_string())
            .or_default();
        bucket.push(entry);
        return;
    }
    state
        .request_by_type
        .entry(resource_type.to_string())
        .or_insert_with(|| request_id.to_string());
    state
        .url_by_type
        .entry(resource_type.to_string())
        .or_insert_with(|| mask_url(url));
    let bucket = state
        .candidates_by_type
        .entry(resource_type.to_string())
        .or_default();
    if bucket.len() < MAX_CANDIDATES_PER_TYPE {
        bucket.push(RequestEntry {
            request_id: request_id.to_string(),
            url: url.to_string(),
        });
    }
}

/// 对一条 `requestId` 调 `Network.getResponseBody`，只回**长度 + SHA-256 + 结构**。
///
/// **Windows 专用**（它要 WebView2 的 DevTools 通路）。
#[cfg(windows)]
async fn fetch_body(window: &tauri::WebviewWindow, label: &str, request_id: &str) -> Option<Value> {
    let params = json!({ "requestId": request_id }).to_string();
    let entry = match call_cdp(window, "Network.getResponseBody", &params).await {
        Ok(raw) => {
            let parsed: Option<Value> = serde_json::from_str(&raw).ok();
            match parsed {
                Some(value) => {
                    let body = value.get("body").and_then(Value::as_str).unwrap_or("");
                    json!({
                        "label": label,
                        "request_id": mask_id(request_id),
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
                    "request_id": mask_id(request_id),
                    "ok": true,
                    "cdp_envelope_unparsable": true,
                    "envelope_bytes": raw.len()
                }),
            }
        }
        Err(e) => json!({
            "label": label,
            "request_id": mask_id(request_id),
            "ok": false,
            "error": e
        }),
    };
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

/// 打码：uuid / 长十六进制 / 长 base64url 串 → `<masked>`。
fn mask_url(raw: &str) -> String {
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
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

/// 打码单个标识（`requestId` / `loaderId` 这类）。
fn mask_id(raw: &str) -> String {
    let masked = mask_url(raw);
    if masked == raw && raw.len() >= 8 {
        // 纯字母数字且不足 32 位 ⇒ 不是 uuid 形态，但仍是会话内标识 ⇒ 照样打码。
        return "<masked>".to_string();
    }
    masked
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

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
