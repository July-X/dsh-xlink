//! DeepSeek 官网对话：**登录态下的端点与字段形态**只读探针（P4 调查单 A 项）。
//!
//! # 它要回答什么
//!
//! P4 协议要写死「执行体在壳、API 调用在页内」。写死之前必须有两份**实测**
//! 事实，不能靠推断：
//!
//! 1. **DeepSeek 官网自己用哪些端点**（不是我们猜的那几个）。
//! 2. **登录态下这些端点返回什么结构**（字段名、层级、分页参数）。
//!
//! 这两件事**只能在页面里做**：端点清单来自浏览器的 resource timing
//! （`performance.getEntriesByType('resource')`），而响应结构来自**同源
//! fetch**——登录 cookie 是 HttpOnly，页外拿不到（实测：页外裸 fetch 与带
//! Bearer 都是 `40003 INVALID_TOKEN`）。
//!
//! # 核心手段
//!
//! - **端点清单**：`performance.getEntriesByType('resource')` ⇒ 只回 URL 的
//!   **path + query 键名**（query 的**值**不回，避免带出会话 id）。
//! - **响应结构**：对清单里的每个端点做一次**同源 GET**，只回
//!   **状态码 + 业务码 + 键名 + 条数**，**不回正文、不回标题、不回凭据**。
//!
//! # 纪律（与仓库既有探针一致）
//!
//! - **零痕迹**：不点任何按钮、不发消息、不预拉会话正文；页内脚本只在
//!   `localStorage`/`sessionStorage`/`document.cookie` 上**读键名与长度**，
//!   值不出页面。
//! - **只读 profile**：**必须**用命令行传入的 `--profile <目录>`（一份
//!   **只读拷贝**出来的副本）。源码里**不出现**任何用户真实路径字面量。
//! - **求值零残留**：用 DevTools 协议的 `Runtime.evaluate`（`awaitPromise`），
//!   **不创建任何全局量**——仓库 `src-tauri/src/official_chat/cdp_eval.rs`
//!   的实测：求值前后 `Object.getOwnPropertyNames(window).length` 不变。
//!
//! # 用法
//!
//! ```sh
//! cd src-tauri
//! cargo run --locked --example official_chat_deepseek_probe -- \
//!   --profile "<只读拷贝出来的 profile 目录>"
//! ```
//!
//! 输出是 `PROBE_REPORT_BEGIN` / `PROBE_REPORT_END` 之间的一段 JSON。
//!
//! # 为什么用 `#[path]` 把 lib 里的那份源码编进来
//!
//! `src/official_chat/cdp_eval.rs` 在 lib 里是 `pub(crate)`，而 example 是
//! **另一个 crate**，拿不到 `pub(crate)`。这里用 `#[path]` 把**同一份源文件**
//! 编进来，因此探针跑的就是将来要进生产的那份求值代码，不是副本。

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
//
// ⚠️ 这两个 `fn` 在**非 Windows** 上没人调（生产那边是 Windows 专用链在调）
// ⇒ 那边「未被使用」是**正常**的 ⇒ 显式豁免，别让 macOS 门禁挂在死代码上。
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

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cdp_eval::cdp_eval;

/// 探测的站点首页。
const SITE_URL: &str = "https://chat.deepseek.com";

/// 页面里要观察的 API 路径前缀。
///
/// **为什么按前缀筛**：resource timing 里全是静态资源（js/css/wasm/font），
/// 只有 `/api/` 开头的才是「数据端点」。
const API_PREFIX: &str = "/api/";

/// 端点清单最多回多少条。
const MAX_ENDPOINTS: usize = 60;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `--profile` **必须**显式给：这个目录是**只读拷贝**出来的副本。
    // 不给就**不做**任何探测——绝不让浏览器落回默认数据目录（用户真实
    // 安装目录），那是 2026-10-20 13:48 那次写坏用户 Cookies 的事故。
    let Some(profile) = arg_value(&args, "--profile").map(PathBuf::from) else {
        eprintln!("probe: 必须显式给 --profile <只读拷贝出来的 profile 目录>；不给就不探测（浏览器会落回默认数据目录）");
        std::process::exit(2);
    };

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let profile = profile.clone();
            std::thread::Builder::new()
                .name("official-chat-deepseek-probe".into())
                .spawn(move || {
                    let report = tauri::async_runtime::block_on(run_probe(&handle, &profile));
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

/// 跑一次探针，出 JSON 报告。
async fn run_probe(handle: &tauri::AppHandle, profile: &Path) -> String {
    let window = match build_window(handle, profile) {
        Ok(window) => window,
        Err(e) => return json!({ "fatal": format!("无法创建探测窗口：{e}") }).to_string(),
    };

    // 等页面就绪：origin 落到该站点 + 文档 complete。
    let ready = wait_for_site(&window).await;
    let mut report = json!({
        "probe": "official-chat deepseek logged-in endpoint & shape probe (P4/A)",
        "site_url": SITE_URL,
        "page_ready": match &ready {
            Ok(ms) => json!({ "ok": true, "elapsed_ms": ms }),
            Err(e) => json!({ "ok": false, "error": e }),
        },
    });

    if ready.is_err() {
        report["observation"] = json!({ "skipped": "页面未就绪，未发起任何接口请求" });
        return report.to_string();
    }

    // --- 第一步：页面事实（不发请求） ---
    report["page_facts"] = match eval_json(
        &window,
        &build_page_facts_expression(),
        Duration::from_secs(8),
    )
    .await
    {
        Ok(value) => value,
        Err(detail) => json!({ "error": detail }),
    };

    // --- 第二步：resource timing 取端点清单（不发请求） ---
    let endpoints = match eval_json(
        &window,
        &build_endpoint_listing_expression(),
        Duration::from_secs(15),
    )
    .await
    {
        Ok(value) => value,
        Err(detail) => {
            report["endpoints"] = json!({ "error": detail });
            json!({ "endpoints": [] })
        }
    };
    report["endpoints"] = endpoints.clone();

    // --- 第三步：对清单里的端点逐个做同源只读 fetch，看返回码与结构 ---
    let paths = collect_api_paths(&endpoints);
    report["probed_endpoints"] = json!(paths.len());
    let mut shapes = Vec::new();
    for path in &paths {
        let expression = build_shape_probe_expression(path);
        let started = Instant::now();
        let outcome = eval_json(&window, &expression, Duration::from_secs(20)).await;
        let elapsed = started.elapsed().as_millis() as u64;
        let mut entry = json!({ "path": path, "elapsed_ms": elapsed });
        match outcome {
            Ok(value) => {
                entry["shape"] = value;
            }
            Err(detail) => {
                entry["error"] = json!(detail);
            }
        }
        shapes.push(entry);
    }
    report["shapes"] = json!(shapes);

    report.to_string()
}

/// 页面事实：**不发任何网络请求**。
///
/// 只回**键名与长度**——凭据与标题都不离开页面。
fn build_page_facts_expression() -> String {
    r#"(function () {
  var out = {
    origin: location.origin,
    href: String(location.href),
    readyState: document.readyState,
    localStorageKeys: [],
    sessionStorageKeys: [],
    visibleCookieNames: [],
    credentialKeyLengths: {},
    pageLooksLikeAuth: false
  };
  try { out.localStorageKeys = Object.keys(localStorage).sort(); } catch (e) {}
  try { out.sessionStorageKeys = Object.keys(sessionStorage).sort(); } catch (e) {}
  try {
    out.visibleCookieNames = document.cookie.split(";").map(function (s) {
      return s.split("=")[0].trim();
    }).filter(Boolean).sort();
  } catch (e) {}
  var credKeys = ["userToken", "token", "accessToken", "sessionToken", "settingsJwt", "__appKit_userInfo"];
  for (var c = 0; c < credKeys.length; c++) {
    try {
      var v = localStorage.getItem(credKeys[c]);
      if (v !== null) out.credentialKeyLengths[credKeys[c]] = String(v).length;
    } catch (e) {}
  }
  out.pageLooksLikeAuth = /sign[_-]?in|sign[_-]?up|log[_-]?in|register|auth/i.test(String(location.href));
  return JSON.stringify(out);
})()"#
    .to_string()
}

/// 端点清单：从 resource timing 里挑出 `/api/` 路径。
///
/// **只回 path + query 的键名**：query 的**值**（含会话 id）不回。
fn build_endpoint_listing_expression() -> String {
    r#"(function () {
  var out = { total: 0, apiCount: 0, endpoints: [] };
  try {
    var entries = performance.getEntriesByType("resource") || [];
    out.total = entries.length;
    var seen = {};
    for (var i = 0; i < entries.length; i++) {
      var name = String(entries[i].name || "");
      if (name.indexOf("/api/") === -1) continue;
      var u;
      try { u = new URL(name); } catch (e) { continue; }
      if (u.origin !== location.origin) continue;
      var queryKeys = [];
      try {
        u.searchParams.forEach(function (_v, k) {
          if (queryKeys.indexOf(k) === -1) queryKeys.push(k);
        });
      } catch (e) {}
      var key = u.pathname + "?" + queryKeys.join(",");
      if (seen[key]) { seen[key].hits++; continue; }
      seen[key] = { hits: 1 };
      out.endpoints.push({ path: u.pathname, queryKeys: queryKeys.slice(0, 24), hits: 1 });
    }
    out.apiCount = out.endpoints.length;
  } catch (e) { out.error = String(e && e.message ? e.message : e); }
  return JSON.stringify(out);
})()"#
        .to_string()
}

/// 对**一个**端点做只读 fetch，**只回结构**。
///
/// - 方法：**GET**（只读；不 POST、不发消息、不触发写操作）。
/// - 凭据：`same-origin`（浏览器自动带上站点的 HttpOnly 登录 cookie）——
///   这正是「页内同源 fetch」这条路成立的原因。
/// - 回传：**状态码 + 业务码 + 键名 + 条数**。**不回**正文、标题、会话 id。
///
/// **为什么用占位符而不是 `format!`**：`format!` 把 `{`/`}` 当占位符语法，
/// 于是 JS 里每一个花括号都得写成 `{{`/`}}`——多写一个、少写一个，整段脚本
/// 就静默变形（编译能过、跑起来才发现不对）。这里改成**运行时替换**：JS 里
/// 只有一处 `__ENDPOINT__` 占位，填进去的是 `serde_json` 转义过的路径
/// （JSON 字符串字面量），**花括号一个都不用转义**。
fn build_shape_probe_expression(path: &str) -> String {
    let template = r#"(async function () {
  var PATH = __ENDPOINT__;
  var out = { path: PATH };
  try {
    var res = await fetch(PATH, { method: "GET", credentials: "same-origin", headers: { "accept": "application/json" } });
    out.status = res.status;
    var text = await res.text();
    out.bodyLength = text.length;
    var parsed = null;
    try { parsed = JSON.parse(text); } catch (e) {}
    if (parsed === null || typeof parsed !== "object") {
      out.json = false;
      return JSON.stringify(out);
    }
    out.json = true;
    out.topKeys = Object.keys(parsed);
    if (typeof parsed.code !== "undefined") out.code = parsed.code;
    if (typeof parsed.msg === "string") out.msg = parsed.msg.slice(0, 120);
    var data = parsed.data;
    if (data === null || typeof data !== "object") {
      out.dataType = data === null ? "null" : typeof data;
      return JSON.stringify(out);
    }
    out.dataKeys = Object.keys(data);
    if (typeof data.biz_code !== "undefined") out.bizCode = data.biz_code;
    var biz = data.biz_data;
    if (biz === null || typeof biz !== "object") {
      out.bizDataType = biz === null ? "null" : typeof biz;
      return JSON.stringify(out);
    }
    out.bizDataKeys = Object.keys(biz);
    for (var k in biz) {
      if (!Object.prototype.hasOwnProperty.call(biz, k)) continue;
      var v = biz[k];
      if (Array.isArray(v)) {
        out.arrayCounts = out.arrayCounts || {};
        out.arrayCounts[k] = v.length;
        if (v.length > 0 && v[0] && typeof v[0] === "object") {
          out.itemKeys = out.itemKeys || {};
          out.itemKeys[k] = Object.keys(v[0]);
        }
      }
    }
    return JSON.stringify(out);
  } catch (e) {
    out.error = String(e && e.message ? e.message : e);
    return JSON.stringify(out);
  }
})()"#;
    let literal = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".to_string());
    template.replace("__ENDPOINT__", &literal)
}

/// 从端点清单里挑出要探测的路径。
///
/// **去重**：同一个 path 只探一次（不同 query 往往是同一端点的不同参数）。
/// **排除**：带路径参数的那几个（会话 id 会从 query 里露出去）——只探
/// 「形态已知」的目录/配置类端点。
fn collect_api_paths(endpoints: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let list = endpoints
        .get("endpoints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for entry in list.iter().take(MAX_ENDPOINTS) {
        let Some(path) = entry.get("path").and_then(Value::as_str) else {
            continue;
        };
        if !path.starts_with(API_PREFIX) {
            continue;
        }
        // 带路径参数的端点（会话 id 就在路径里）——**不探**：会把会话 id 探出来。
        if path.contains("chat_session_id") || path.ends_with("/current") {
            continue;
        }
        if seen.iter().any(|p| p == path) {
            continue;
        }
        seen.push(path.to_string());
        out.push(path.to_string());
    }
    out
}

/// 在页面里求值一个表达式，把返回值解析成 JSON。
///
/// 返回的是**页面回传的那个 JSON 字符串**解析后的值；页面异常/超时返回
/// `Err`（原因串）。
async fn eval_json(
    window: &tauri::WebviewWindow,
    expression: &str,
    timeout: Duration,
) -> Result<Value, String> {
    match cdp_eval(window, expression, timeout).await {
        Ok(envelope) => {
            let (value, exception) = crate::cdp_eval::unwrap_cdp_result(&envelope);
            if let Some(detail) = exception {
                return Err(format!("页内求值抛错：{detail}"));
            }
            let Some(value) = value else {
                return Err("页内求值没有返回值".to_string());
            };
            // 页面回传的是 JSON **字符串** ⇒ 解析它。
            match value {
                Value::String(raw) => serde_json::from_str::<Value>(&raw)
                    .map_err(|e| format!("页内回传的不是合法 JSON：{e}")),
                other => Ok(other),
            }
        }
        Err(e) => Err(format!("{e}")),
    }
}

/// 等站点页面就绪：origin 落到该站点 + 文档 complete + 有内容。
async fn wait_for_site(window: &tauri::WebviewWindow) -> Result<u64, String> {
    let started = Instant::now();
    let mut last = String::new();
    loop {
        if started.elapsed() > Duration::from_secs(45) {
            return Err(format!("站点页面 45s 内没有就绪；最后状态：{last}"));
        }
        let probe = "JSON.stringify({ origin: location.origin, href: String(location.href), ready: document.readyState, hasBody: !!(document.body && document.body.children.length) })";
        if let Ok(Value::String(raw)) =
            crate::script_eval::read_json(window, probe, Duration::from_secs(1)).await
        {
            if let Ok(state) = serde_json::from_str::<Value>(&raw) {
                let origin = state["origin"].as_str().unwrap_or("");
                let ready = state["ready"].as_str().unwrap_or("");
                let has_body = state["hasBody"].as_bool().unwrap_or(false);
                last = raw.clone();
                if origin.contains("chat.deepseek.com") && ready == "complete" && has_body {
                    return Ok(started.elapsed().as_millis() as u64);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// 建探针窗口。
///
/// **`data_directory` 必须无条件给**：不设它，WebView2 会用**默认**数据
/// 目录（本项目安装目录下的 `EBWebView`），那是**用户真实安装目录**——
/// 浏览器一启动就往那里写 Cookies / Preferences / History。判据
/// `scripts/lib/probe-profile-isolation.mjs` 的第 ③ 类盯的就是这一条。
fn build_window(handle: &tauri::AppHandle, profile: &Path) -> Result<tauri::WebviewWindow, String> {
    let url = SITE_URL.parse().map_err(|e| format!("URL 非法：{e}"))?;
    std::fs::create_dir_all(profile).map_err(|e| format!("无法创建 profile 目录：{e}"))?;
    tauri::WebviewWindowBuilder::new(
        handle,
        "official-chat-deepseek-probe",
        tauri::WebviewUrl::External(url),
    )
    .title("dsh DeepSeek 端点探针")
    .inner_size(1000.0, 760.0)
    .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
    .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
    .data_directory(profile.to_path_buf())
    .build()
    .map_err(|e| format!("无法创建探针窗口：{e}"))
}
