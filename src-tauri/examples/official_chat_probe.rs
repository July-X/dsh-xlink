//! 官方对话（B 侧）**页内求值回程**与**首站最小只读探测**的隔离探针（P0 / G2）。
//!
//! # 两种模式
//!
//! ```sh
//! cd src-tauri
//!
//! # 模式一：回程机制（可控页面，不碰任何真实站点）
//! cargo run --locked --example official_chat_probe -- --mode controlled
//!
//! # 模式二：首站最小只读探测（真实站点，只读、低频、少量）
//! #
//! # profile **必须**是一次性临时目录。绝不能指向用户真实数据目录下的官网对话
//! # profile：浏览器一启动就往那个目录写 Cookies / Preferences / Local Storage /
//! # History / IndexedDB，而 Cookies 是活文件，写坏了回滚不了。
//! # 要复用已登录态，先把那份 profile **只读拷贝**一份到临时目录再用
//! # （Windows 上 Cookies 由 DPAPI 绑机器+用户、不绑路径，拷贝可移植）。
//! cargo run --locked --example official_chat_probe -- --mode site --site deepseek \
//!   --profile "$TEMP/dsh-probe-profile-copy"
//! ```
//!
//! 输出是 `PROBE_REPORT_BEGIN` / `PROBE_REPORT_END` 之间的一段 JSON。
//!
//! 模式一验证五种情形：延迟成功 / Promise 拒绝 / 超时 / 导航 / 页面销毁。
//! 模式二只取**目录元数据**（条数、字段名、分页形态、有效 origin、认证上下文
//! 是否自动满足），**不读任何会话正文**；页内脚本只把**结构**回传宿主，
//! 凭据与标题都不离开页面。
//!
//! # 为什么用 `#[path]` 把 lib 里的那份源码编进来
//!
//! `src/official_chat/script_eval.rs` 在 lib 里是 `pub(crate)`，而 example
//! 是**另一个 crate**，拿不到 `pub(crate)`。这里用 `#[path]` 把**同一份
//! 源文件**编进来，因此探针跑的就是将来要进生产的那份代码，不是副本。

// 探针只用到 harness 的一部分，`#[path]` 把整份源文件编进来会带出一批
// 未使用的常量与函数——这不影响探针行为，不必逐个处理。
#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

// CDP 求值可行性实验（只给探针用；放在子目录里，不会被 cargo 当成独立
// example target）。
#[allow(dead_code)]
#[path = "support/cdp_probe.rs"]
mod cdp_probe;

// 只读拷贝 profile 的工具（G3 验证用）。
#[allow(dead_code)]
#[path = "support/profile_copy.rs"]
mod profile_copy;

// G3 的读取上下文与零痕迹求值：**编译生产源码本身**（`#[path]`），测的就是
// 将来进生产的那份代码。
#[allow(dead_code)]
#[path = "../src/official_chat/context.rs"]
mod context;

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

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Manager;

use script_eval::{read_json, round_trip, RoundTripError};

/// 可控页面：五种情形的脚本都在这里，**不碰任何真实站点**。
const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>dsh official-chat round-trip probe</title>
<script>
window.__probeReady = true;
window.probeDelayed = function (ms, value) {
  return new Promise(function (resolve) { setTimeout(function () { resolve(value); }, ms); });
};
window.probeRejected = function (ms, message) {
  return new Promise(function (_resolve, reject) { setTimeout(function () { reject(new Error(message)); }, ms); });
};
window.probeNever = function () { return new Promise(function () {}); };
window.probeNavigate = function (delayMs, href) {
  return new Promise(function (resolve) {
    setTimeout(function () { window.location.href = href; resolve("resolved-after-navigation"); }, delayMs);
  });
};
</script>
<h1>dsh official-chat round-trip probe</h1>
<p>controlled page - no real site is contacted</p>
"#;

/// 导航之后的目标页。只证明「文档被换掉了」。
const NAV_PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>probe - navigated</title>
<h1>navigated</h1>
"#;

/// 首站探测目标。
///
/// `directory_path` 来自**第三方逆向项目**（二级来源，未在本机复核）。这里
/// 的用途只是**探测**：接口成不存在、认证上下文满不满足、响应结构长什么样，
/// 都以实测为准——探针不会因为「文档里这么写」就认为它成立。
#[derive(Clone, Copy)]
struct SiteSpec {
    key: &'static str,
    url: &'static str,
    directory_path: Option<&'static str>,
    /// 可以接受的有效 host。站点可能把入口域名 301 到另一个域名
    /// （实测 `agent.minimaxi.com` → `agent.minimax.cn`），所以按集合判定。
    hosts: &'static [&'static str],
}

const SITES: &[SiteSpec] = &[
    SiteSpec {
        key: "deepseek",
        url: "https://chat.deepseek.com",
        directory_path: Some("/api/v0/chat_session/fetch_page?count=5"),
        hosts: &["chat.deepseek.com"],
    },
    SiteSpec {
        key: "chatgpt",
        url: "https://chatgpt.com",
        directory_path: Some(
            "/backend-api/conversations?offset=0&limit=5&order=updated&is_archived=false",
        ),
        hosts: &["chatgpt.com", "chat.openai.com"],
    },
    SiteSpec {
        key: "minimax",
        url: "https://agent.minimaxi.com",
        directory_path: None,
        hosts: &["agent.minimaxi.com", "agent.minimax.cn"],
    },
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = arg_value(&args, "--mode").unwrap_or_else(|| "controlled".to_string());
    let site_key = arg_value(&args, "--site").unwrap_or_else(|| "deepseek".to_string());
    let profile = arg_value(&args, "--profile").map(PathBuf::from);

    // 只有 site / g3 模式才认站点；controlled / cdp 模式必须让 spec 为 None，
    // 否则 setup 里会走站点分支，误把本地可控页换成真实站点。
    let spec = if mode == "site" || mode == "g3" {
        SITES.iter().find(|s| s.key == site_key).copied()
    } else {
        None
    };
    if (mode == "site" || mode == "g3") && spec.is_none() {
        eprintln!("probe: 未知站点 {site_key}；可选：deepseek / chatgpt / minimax");
        std::process::exit(2);
    }

    let controlled = if mode == "controlled" || mode == "cdp" {
        match ControlledServer::start() {
            Ok(server) => Some(server),
            Err(e) => {
                eprintln!("probe: 无法启动可控页面服务：{e}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    let url = match &controlled {
        Some(server) => format!("http://127.0.0.1:{}/", server.port),
        None => spec.expect("site 模式必须有站点").url.to_string(),
    };

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            let url = url.clone();
            let site = spec;
            let profile = profile.clone();
            let mode = mode.clone();
            std::thread::Builder::new()
                .name("official-chat-probe".into())
                .spawn(move || {
                    let report = match (mode.as_str(), site) {
                        ("site", Some(site)) => {
                            run_site_probe(&handle, &url, site, profile.as_deref())
                        }
                        ("cdp", _) => tauri::async_runtime::block_on(run_cdp_probe(&handle, &url)),
                        ("g3", Some(site)) => tauri::async_runtime::block_on(run_g3_probe(
                            &handle,
                            site,
                            profile.as_deref(),
                        )),
                        _ => run_controlled_probe(&handle, &url),
                    };
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

// --- 模式一：回程机制 -------------------------------------------------------

/// 跑完五种情形，出一份 JSON 报告。
fn run_controlled_probe(handle: &tauri::AppHandle, url: &str) -> String {
    // 留一个不关的窗口：Tauri 在最后一个窗口关闭时会结束进程，而「页面
    // 销毁」那一情形要关掉探针窗口——没有这个陪衬，报告就打印不出来。
    //
    // 陪衬窗口必须和探针窗口用**同一套**浏览器参数与 user-data 目录，
    // 否则 WebView2 拒绝在同一个 user-data 目录上建第二个环境
    // （ERROR_INVALID_STATE / 0x8007139F）。
    let keepalive = build_window_with(handle, "official-chat-probe-keepalive", url, None, false);
    if let Err(e) = keepalive {
        return json!({ "fatal": format!("无法创建陪衬窗口：{e}") }).to_string();
    }

    let window = match build_window(handle, "official-chat-probe", url, None) {
        Ok(window) => window,
        Err(e) => return json!({ "fatal": format!("无法创建探针窗口：{e}") }).to_string(),
    };

    tauri::async_runtime::block_on(async move {
        let mut results: Vec<Value> = Vec::new();

        // 先问一句「求值通道通不通、页面到底是什么」——失败时要能分清
        // 「页面没加载」与「页面加载了但脚本没跑」。
        let state = page_state(&window).await;
        results.push(json!({
            "scenario": "page-state",
            "expect": "确认求值通道与当前文档",
            "ok": state["eval_ok"].as_bool().unwrap_or(false),
            "detail": state.to_string(),
        }));

        // 页面就绪之前不开始计时。
        let ready = wait_for_ready(&window).await;
        results.push(json!({
            "scenario": "page-ready",
            "expect": "可控页面加载完成",
            "ok": ready.is_ok(),
            "detail": match &ready {
                Ok(ms) => format!("就绪耗时 {ms}ms"),
                Err(e) => e.clone(),
            },
        }));

        // 1. 延迟成功：必须拿到**完成后的值**，不是 Promise 对象。
        results.push(
            run_case(
                &window,
                "delayed-success",
                "拿到表达式完成后的值",
                "window.probeDelayed(250, { value: 42, role: \"assistant\" })",
                Duration::from_secs(5),
            )
            .await,
        );

        // 2. Promise 拒绝：必须拿到可区分的失败结构。
        results.push(
            run_case(
                &window,
                "promise-rejected",
                "拿到 Rejected 分类与拒绝原因",
                "window.probeRejected(50, \"probe-expected-rejection\")",
                Duration::from_secs(5),
            )
            .await,
        );

        // 3. 超时：必须超时退出并给出超时原因，不能无限挂起。
        results.push(
            run_case(
                &window,
                "timeout",
                "超时退出并给出超时原因",
                "window.probeNever()",
                Duration::from_secs(2),
            )
            .await,
        );

        // 4. 导航：旧页面的在途结果要能被识别为「页面没了」。
        results.push(
            run_case(
                &window,
                "navigation",
                "识别为页面被替换（Gone），不是空结果",
                "window.probeNavigate(250, \"/nav-target.html\")",
                Duration::from_secs(5),
            )
            .await,
        );

        // 5. 页面销毁：不得 panic、不得无限挂起。必须放在最后——窗口没了，
        //    后面的情形就无从跑起。
        //
        //    表达式必须**自包含**：上一个情形已经导航到了 /nav-target.html，
        //    那个页面没有 probeNever，用它只会测出「函数不存在」而不是
        //    「页面没了」。
        let closed = window.clone();
        std::thread::Builder::new()
            .name("official-chat-probe-closer".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(250));
                let _ = closed.close();
            })
            .expect("probe: 无法创建关闭线程");
        results.push(
            run_case(
                &window,
                "webview-destroyed",
                "识别为页面不在了（Gone），不 panic、不无限挂起",
                "new Promise(function () {})",
                Duration::from_secs(8),
            )
            .await,
        );

        json!({
            "probe": "official-chat round-trip (G2)",
            "page": "local controlled page (no real site contacted)",
            "scenarios": results,
        })
        .to_string()
    })
}

/// 跑一个情形，把结果归成「通过 / 失败 / 未执行」。
async fn run_case(
    window: &tauri::WebviewWindow,
    name: &str,
    expect: &str,
    expression: &str,
    timeout: Duration,
) -> Value {
    let started = Instant::now();
    let outcome = round_trip(window, expression, timeout).await;
    let elapsed = started.elapsed().as_millis() as u64;

    match outcome {
        Ok(trip) => {
            // 「拿到 Promise 对象」不算通过：值必须是完成后的内容。
            let looks_like_promise = trip
                .value
                .as_object()
                .map(|m| m.is_empty())
                .unwrap_or(false);
            let ok = !looks_like_promise;
            json!({
                "scenario": name,
                "expect": expect,
                "ok": ok,
                "detail": if ok {
                    format!("拿到完成后的值（{} 轮读回，{}ms）", trip.polls, trip.elapsed_ms)
                } else {
                    "拿到的是空对象——那很可能是 Promise 对象的序列化，不是完成后的值".to_string()
                },
                "elapsed_ms": elapsed,
                "polls": trip.polls,
                "value": trip.value,
            })
        }
        Err(e) => {
            let (ok, detail) = match &e {
                RoundTripError::Rejected { message } => (
                    message.contains("probe-expected-rejection"),
                    format!("拿到可区分的拒绝结构：{message}"),
                ),
                RoundTripError::Timeout { .. } => (true, e.to_string()),
                RoundTripError::Gone { .. } => (true, e.to_string()),
                RoundTripError::Transport { .. } => (false, e.to_string()),
            };
            json!({
                "scenario": name,
                "expect": expect,
                "ok": ok,
                "detail": detail,
                "elapsed_ms": elapsed,
                "error": format!("{e}"),
            })
        }
    }
}

/// 问一句「求值通道通不通、页面现在是什么」。
///
/// 失败时必须能区分三件事：通道本身坏了（Transport）、页面还没导航
/// （文档还是初始的）、页面加载了但脚本没执行。
async fn page_state(window: &tauri::WebviewWindow) -> Value {
    let mut out = serde_json::Map::new();
    for (label, expression) in [
        ("sum", "1 + 1"),
        ("href", "String(location.href)"),
        ("origin", "String(location.origin)"),
        ("readyState", "String(document.readyState)"),
        ("title", "String(document.title)"),
        (
            "bodyLen",
            "document.body ? document.body.innerHTML.length : -1",
        ),
        ("probeReadyType", "typeof window.__probeReady"),
    ] {
        let outcome = read_json(window, expression, Duration::from_secs(3)).await;
        let value = match outcome {
            Ok(v) => json!({ "ok": true, "value": v }),
            Err(e) => json!({ "ok": false, "error": format!("{e}") }),
        };
        out.insert(label.to_string(), value);
    }
    let all_ok = out.values().all(|v| v["ok"].as_bool().unwrap_or(false));
    out.insert("eval_ok".to_string(), json!(all_ok));
    Value::Object(out)
}

/// 等可控页面就绪。
async fn wait_for_ready(window: &tauri::WebviewWindow) -> Result<u64, String> {
    let started = Instant::now();
    let mut last_state = String::new();
    loop {
        if started.elapsed() > Duration::from_secs(15) {
            return Err(format!(
                "可控页面 15s 内没有就绪；最后页面状态：{last_state}"
            ));
        }
        match read_json(
            window,
            "window.__probeReady === true",
            Duration::from_secs(1),
        )
        .await
        {
            Ok(Value::Bool(true)) => return Ok(started.elapsed().as_millis() as u64),
            Ok(_) => {}
            Err(e) => return Err(format!("读回失败：{e}")),
        }
        // 记录一次页面状态：失败时要能分清「页面没加载」与「页面加载了但脚本没跑」。
        if last_state.is_empty() {
            let probe = r#"JSON.stringify({
                href: String(location.href),
                origin: String(location.origin),
                ready: String(document.readyState),
                title: String(document.title),
                bodyLen: document.body ? document.body.innerHTML.length : -1,
                hasReadyFlag: typeof window.__probeReady
            })"#;
            if let Ok(Value::String(raw)) = read_json(window, probe, Duration::from_secs(1)).await {
                last_state = raw;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// --- 模式四：G3 官网窗口未打开时仍可读取 --------------------------------------

/// G3 验证：官网窗口未打开时仍可读取。
///
/// **隔离纪律**：本模式**不指向用户真实数据目录**。启动时把指定 profile
/// **只读拷贝**到一次性临时目录，跑完删掉拷贝（绝不删源）。
///
/// 六个场景：
/// 1. 页签已挂载 → 复用已挂载页签；
/// 2. 官网窗口关闭 → 按需建隐藏读取上下文，且不抢焦点；
/// 3. 尚未登录 → 明确返回「未登录」，不挂到空白新账号；
/// 4. 账号切换 → 旧上下文失效；
/// 5. 重定向 → 记录实际导航落点；
/// 6. 所有权转移 → 用户开/关官网窗口时归属变化，且不销毁用户正在用的页签。
async fn run_g3_probe(
    handle: &tauri::AppHandle,
    site: SiteSpec,
    source_profile: Option<&std::path::Path>,
) -> String {
    let mut report = json!({
        "probe": "official-chat reading-context (G3)",
        "site": site.key,
    });

    // 陪衬窗口：Tauri 在**最后一个窗口**关闭时会结束进程。场景 2 要关掉
    // 模拟的官网页签，没有这个陪衬就会中途退出——报告打印不出来，临时拷贝
    // 也来不及删。
    let _ = PROBE_HANDLE.set(handle.clone());
    let keepalive = build_window_with(
        handle,
        "official-chat-probe-keepalive",
        site.url,
        None,
        false,
    );
    if let Err(e) = keepalive {
        report["fatal"] = json!(format!("无法创建陪衬窗口：{e}"));
        return report.to_string();
    }

    // --- 隔离：只读拷贝 ---
    let copy_root = profile_copy::temp_copy_root();
    let copied_profile = copy_root.join("profile");
    let mut copy_report = json!({});
    if let Some(src) = source_profile {
        match profile_copy::copy_dir_readonly(src, &copied_profile) {
            Ok(r) => {
                copy_report = json!({
                    "source_profile": src.display().to_string(),
                    "source_read_only": true,
                    "temp_copy_root": copy_root.display().to_string(),
                    "temp_copy_profile": copied_profile.display().to_string(),
                    "files_copied": r.files,
                    "bytes_copied": r.bytes,
                    "files_skipped": r.skipped.len(),
                    "skip_reason": r.skip_reason,
                    "skipped_sample": r.skipped.iter().take(5).cloned().collect::<Vec<_>>(),
                });
            }
            Err(e) => {
                copy_report = json!({ "copy_failed": format!("{e}") });
            }
        }
    } else {
        copy_report = json!({ "skipped": "未提供 --profile，使用空 profile（无登录态）" });
        let _ = std::fs::create_dir_all(&copied_profile);
    }
    report["profile_isolation"] = copy_report.clone();

    let mounted_label = "official-chat-tab-0";
    let browser_args = official_chat::OFFICIAL_CHAT_BROWSER_ARGS;
    let mut scenarios: Vec<Value> = Vec::new();

    // --- 场景 1：页签已挂载 → 复用 ---
    let mounted = build_window_with(
        handle,
        mounted_label,
        site.url,
        Some(&copied_profile),
        false,
    );
    if let Err(e) = mounted {
        report["fatal"] = json!(format!("无法模拟官网对话页签：{e}"));
        cleanup_copy(&copy_root, source_profile, &mut report);
        return report.to_string();
    }
    let ctx1 = context::ReadingContext::resolve(
        handle,
        site.key,
        site.url,
        &copied_profile,
        mounted_label,
        browser_args,
    );
    match ctx1 {
        Ok(ctx) => {
            let same_dir = ctx.profile_dir == copied_profile;
            scenarios.push(json!({
                "scenario": "1-tab-already-mounted",
                "expect": "官网窗口开着、内容页签已挂载 → 复用那个页签，不新建 webview",
                "ok": ctx.source == context::ContextSource::MountedTab && same_dir,
                "source": ctx.source.as_str(),
                "profile_dir": ctx.profile_dir.display().to_string(),
                "profile_dir_equals_official_chat_profile": same_dir,
                "owned_by_shell": ctx.owned,
            }));
        }
        Err(e) => scenarios.push(json!({
            "scenario": "1-tab-already-mounted",
            "ok": false,
            "error": e,
        })),
    }

    // --- 场景 2：官网窗口关闭 → 按需建隐藏读取上下文，不抢焦点 ---
    // 关掉模拟的官网页签。
    if let Some(w) = handle.get_webview_window(mounted_label) {
        let _ = w.close();
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    let ctx2 = context::ReadingContext::resolve(
        handle,
        site.key,
        site.url,
        &copied_profile,
        mounted_label,
        browser_args,
    );
    let mut ctx2 = match ctx2 {
        Ok(ctx) => ctx,
        Err(e) => {
            scenarios
                .push(json!({ "scenario": "2-official-window-closed", "ok": false, "error": e }));
            report["scenarios"] = Value::Array(scenarios);
            cleanup_copy(&copy_root, source_profile, &mut report);
            return report.to_string();
        }
    };
    let hidden = !ctx2.window.is_visible().unwrap_or(true);
    scenarios.push(json!({
        "scenario": "2-official-window-closed",
        "expect": "官网窗口关闭 → 按需建立读取上下文，且窗口不可见（不抢焦点）",
        "ok": ctx2.source == context::ContextSource::LazyHidden && hidden,
        "source": ctx2.source.as_str(),
        "window_visible": ctx2.window.is_visible().unwrap_or(true),
        "window_hidden": hidden,
        "owned_by_shell": ctx2.owned,
        "profile_dir_equals_official_chat_profile": ctx2.profile_dir == copied_profile,
    }));

    // --- 场景 5（先做）：重定向 → 记录实际导航落点 ---
    // 站点的登录重定向是**异步**的（先落在 "/"，再被跳到 "/sign_in"），
    // 所以必须**等落点稳定**再记录，否则会把中间态当成最终落点。
    let mut landed_origin = String::new();
    let mut landed_path = String::new();
    let mut stable = false;
    for _ in 0..20 {
        if let Ok((o, p)) = ctx2.probe_landing(Duration::from_secs(2)).await {
            if !p.is_empty() && p == landed_path {
                landed_origin = o;
                stable = true;
                break;
            }
            landed_origin = o;
            landed_path = p;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    scenarios.push(json!({
        "scenario": "5-redirect-landing",
        "expect": "记录实际导航落点（不假设入口域名就是有效 origin）",
        "ok": stable && !landed_origin.is_empty(),
        "requested_url": site.url,
        "landed_origin": landed_origin,
        "landed_path": landed_path,
        "landing_stable": stable,
    }));

    // --- 场景 3：尚未登录 → 明确返回「未登录」 ---
    // 判据：**最终落点**在登录/注册路径上 → 未登录；否则再看页内有没有账号信息。
    // 两种情况都归为未登录，且**不许**把候选挂到空白新账号上。
    let auth_state = ctx2
        .read(
            r#"JSON.stringify({
  path: String(location.pathname),
  looksLikeAuth: /sign[_-]?in|sign[_-]?up|log[_-]?in|register/i.test(String(location.pathname)),
  hasUserInfo: (function () {
    try {
      var keys = Object.keys(localStorage);
      for (var i = 0; i < keys.length; i++) {
        if (/userinfo|user_info|^user$/i.test(keys[i])) {
          var v = localStorage.getItem(keys[i]);
          if (v && String(v).length > 2) return true;
        }
      }
    } catch (e) {}
    return false;
  })()
})"#,
            Duration::from_secs(3),
        )
        .await;
    let auth_parsed = auth_state
        .as_ref()
        .ok()
        .and_then(|v| v.as_str())
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let page_looks_like_auth = auth_parsed
        .as_ref()
        .and_then(|v| v.get("looksLikeAuth"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_user_info = auth_parsed
        .as_ref()
        .and_then(|v| v.get("hasUserInfo"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // 最终落点判定优先（站点的重定向是权威信号），页内信息作辅助。
    // Rust 没有内置正则，这里用字符串包含判断。
    let path_lower = landed_path.to_ascii_lowercase();
    let landed_on_auth_path = page_looks_like_auth
        || path_lower.contains("sign_in")
        || path_lower.contains("sign-in")
        || path_lower.contains("signup")
        || path_lower.contains("sign_up")
        || path_lower.contains("login")
        || path_lower.contains("register");
    let logged_in = !landed_on_auth_path && has_user_info;
    scenarios.push(json!({
        "scenario": "3-not-logged-in",
        "expect": "未登录时明确返回「未登录」，不得挂到空白新账号",
        "ok": !logged_in,
        "logged_in": logged_in,
        "landed_on_auth_path": landed_on_auth_path,
        "page_looks_like_auth": page_looks_like_auth,
        "page_has_user_info": has_user_info,
        "landed_path": landed_path,
        "decision": if logged_in { "attached-to-session" } else { "not-logged-in" },
    }));

    // --- 场景 4：账号切换 → 旧上下文失效 ---
    let fp_before = ctx2.account_fingerprint(Duration::from_secs(3)).await.ok();
    // 模拟账号切换：改一个账号相关键的**长度**（只写在拷贝 profile 里）。
    let write_result = ctx2
        .read(
            r#"(function () {
  try {
    localStorage.setItem("__dsh_probe_account_marker", "account-b");
    return "ok";
  } catch (e) { return "failed: " + String(e); }
})()"#,
            Duration::from_secs(3),
        )
        .await;
    // 重新算一次指纹：现在应该与之前不同（模拟换了账号）。
    let fp_after = ctx2.account_fingerprint(Duration::from_secs(3)).await.ok();
    if let (Some(before), Some(after)) = (fp_before.as_ref(), fp_after.as_ref()) {
        // 先把「变更前」的指纹登记成基线，再登记变更后的 —— 第一次登记
        // 只是建立基线，不算「切换」。
        ctx2.note_account(before);
        let changed_detected = ctx2.note_account(after);
        scenarios.push(json!({
            "scenario": "4-account-switch",
            "expect": "账号指纹变化 → 旧上下文与缓存失效",
            "ok": changed_detected && before != after,
            "fingerprint_before": before,
            "fingerprint_after": after,
            "write_result": write_result.as_ref().ok().and_then(Value::as_str),
            "note_account_reported_switch": changed_detected,
        }));
    } else {
        scenarios.push(json!({
            "scenario": "4-account-switch",
            "ok": false,
            "error": "拿不到账号指纹（前后任一为空）",
        }));
    }

    // --- 场景 6 的前半：用户**打开**官网窗口 → 已挂载页签重新出现 ---
    let reopened = build_window_with(
        handle,
        mounted_label,
        site.url,
        Some(&copied_profile),
        false,
    );
    let ctx3 = context::ReadingContext::resolve(
        handle,
        site.key,
        site.url,
        &copied_profile,
        mounted_label,
        browser_args,
    );
    let transferred = matches!(
        ctx3.as_ref().ok().map(|c| c.source),
        Some(context::ContextSource::MountedTab)
    );

    // --- 闲置回收（要在回收之前测） ---
    // 自建上下文闲置到阈值 → 可回收；用户挂载的页签 → **永远不由壳回收**。
    let idle_own = ctx2.is_idle(Duration::from_millis(1));
    let idle_user_tab = ctx3
        .as_ref()
        .ok()
        .map(|c| c.is_idle(Duration::from_millis(1)))
        .unwrap_or(false);
    scenarios.push(json!({
        "scenario": "idle-reclaim",
        "expect": "自建读取上下文闲置可回收；用户挂载的页签永远不由壳回收",
        "ok": idle_own && !idle_user_tab,
        "own_context_idle_reclaimable": idle_own,
        "user_mounted_tab_idle_reclaimable": idle_user_tab,
    }));

    // --- 场景 6：所有权转移 → 回收自建上下文不销毁用户的页签 ---
    // 回收自建的隐藏上下文：**不得**销毁用户正在使用的页签。
    let released_own = ctx2.release();
    let user_tab_still_alive = handle
        .get_webview_window(mounted_label)
        .map(|w| w.url().is_ok())
        .unwrap_or(false);
    scenarios.push(json!({
        "scenario": "6-ownership-transfer",
        "expect": "用户开官网窗口 → 读取上下文移交给已挂载页签；回收自建上下文不销毁用户的页签",
        "ok": transferred && released_own && user_tab_still_alive,
        "reopened_window": reopened.is_ok(),
        "source_after_reopen": ctx3.as_ref().ok().map(|c| c.source.as_str()),
        "released_own_hidden_context": released_own,
        "user_tab_still_alive": user_tab_still_alive,
    }));

    report["scenarios"] = Value::Array(scenarios);
    cleanup_copy(&copy_root, source_profile, &mut report);
    report.to_string()
}

/// 跑完删掉临时拷贝，并**只删自己那块**。
///
/// 顺序很重要：**先关掉所有探针窗口**，等浏览器进程退出、文件句柄释放，
/// 再删目录——否则 WebView2 还占着文件，删除会失败并留下残留。
///
/// 顺带断言**源目录还在**——这是本次事故的核心红线：拷贝可以删，源不能删。
fn cleanup_copy(copy_root: &std::path::Path, source: Option<&std::path::Path>, report: &mut Value) {
    // 关掉探针建的所有**业务**窗口（模拟官网页签 / 隐藏读取窗口）。
    // **不关陪衬窗口**——它是最后一个窗口，关掉它会让 Tauri 结束进程，
    // 报告就打印不出来了。陪衬用的是默认 user-data 目录，不占用拷贝目录。
    for label in [
        "official-chat-tab-0",
        "official-chat-read-deepseek",
        "official-chat-read-chatgpt",
        "official-chat-read-minimax",
    ] {
        if let Some(w) = report_handle().and_then(|h| h.get_webview_window(label)) {
            let _ = w.close();
        }
    }
    // 给浏览器进程一点时间释放文件句柄。
    std::thread::sleep(Duration::from_millis(1500));

    let removed = match profile_copy::remove_temp_copy(copy_root) {
        Ok(()) => true,
        Err(e) => {
            report["cleanup"]["error"] = json!(format!("{e}"));
            false
        }
    };
    let source_still_exists = source.map(|s| s.exists()).unwrap_or(true);
    report["cleanup"] = json!({
        "temp_copy_removed": removed,
        "temp_copy_root": copy_root.display().to_string(),
        "source_deleted": false,
        "source_still_exists": source_still_exists,
        "source_profile": source.map(|s| s.display().to_string()),
        "note": "只删了本模块创建的临时拷贝，源 profile 未动",
    });
}

/// 探针的 `AppHandle`（清理阶段要关窗口）。
static PROBE_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

fn report_handle() -> Option<&'static tauri::AppHandle> {
    PROBE_HANDLE.get()
}

// --- 模式三：CDP 求值可行性实验 ---------------------------------------------

/// CDP 求值实验：核实「用 DevTools 协议求值替代页面槽位」是否成立。
///
/// 只在**本地可控页**上跑，不碰任何真实站点，也不改生产实现。
///
/// 三个实验点：
/// 1. **未开 DevTools 窗口**能不能调用（本进程从头到尾没开过 DevTools，
///    这正是生产环境的状态）。
/// 2. `awaitPromise: true` 能不能拿到 **Promise 结算后的值**。
/// 3. 求值前后**页面 JS 空间**的全局量有没有变化（槽位方案的核心缺陷）。
async fn run_cdp_probe(handle: &tauri::AppHandle, url: &str) -> String {
    // 陪衬窗口：Tauri 在最后一个窗口关闭时会结束进程。
    let keepalive = build_window_with(handle, "official-chat-probe-keepalive", url, None, false);
    if let Err(e) = keepalive {
        return json!({ "fatal": format!("无法创建陪衬窗口：{e}") }).to_string();
    }
    let window = match build_window(handle, "official-chat-probe", url, None) {
        Ok(window) => window,
        Err(e) => return json!({ "fatal": format!("无法创建探针窗口：{e}") }).to_string(),
    };

    // 等页面就绪。
    let ready = wait_for_ready(&window).await;

    let mut results: Vec<Value> = Vec::new();

    // --- 实验 0：求值前，页面 JS 空间长什么样 ---
    let before = read_json(
        &window,
        "JSON.stringify({ ownNames: Object.getOwnPropertyNames(window).length, ownNamesSample: Object.getOwnPropertyNames(window).filter(function (n) { return n.indexOf('__dsh') === 0; }) })",
        Duration::from_secs(3),
    )
    .await;

    // --- 实验 1：CDP 求值，普通同步表达式 ---
    let sync_expr = "JSON.stringify({ n: 1 + 1, href: String(location.href) })";
    let sync_result = cdp_probe::cdp_eval(&window, sync_expr, true, Duration::from_secs(10)).await;
    let (sync_value, sync_exception) = match &sync_result {
        Ok(envelope) => cdp_probe::unwrap_cdp_result(envelope),
        Err(_) => (None, None),
    };
    results.push(json!({
        "experiment": "cdp-sync-expression",
        "expect": "未开 DevTools 窗口时也能调用 Runtime.evaluate 并取回值",
        "ok": sync_result.is_ok() && sync_value.is_some(),
        "envelope": sync_result.as_ref().ok(),
        "value": sync_value,
        "exception": sync_exception,
    }));

    // --- 实验 2：awaitPromise —— 能不能拿到 Promise 结算后的值 ---
    let promise_expr = "window.probeDelayed(250, { value: 42, role: \"assistant\" })";
    let promise_result =
        cdp_probe::cdp_eval(&window, promise_expr, true, Duration::from_secs(10)).await;
    let (promise_value, promise_exception) = match &promise_result {
        Ok(envelope) => cdp_probe::unwrap_cdp_result(envelope),
        Err(_) => (None, None),
    };
    // 「拿到 Promise 对象」不算通过：必须是结算后的内容。
    let looks_like_promise = promise_value
        .as_ref()
        .and_then(Value::as_object)
        .map(|m| m.is_empty())
        .unwrap_or(false);
    results.push(json!({
        "experiment": "cdp-await-promise",
        "expect": "awaitPromise 直接拿到 Promise 结算后的值（不是 Promise 对象）",
        "ok": promise_result.is_ok() && promise_value.is_some() && !looks_like_promise,
        "envelope": promise_result.as_ref().ok(),
        "value": promise_value,
        "exception": promise_exception,
    }));

    // --- 实验 3：Promise 拒绝 ---
    let reject_expr = "window.probeRejected(50, \"probe-expected-rejection\")";
    let reject_result =
        cdp_probe::cdp_eval(&window, reject_expr, true, Duration::from_secs(10)).await;
    let (reject_value, reject_exception) = match &reject_result {
        Ok(envelope) => cdp_probe::unwrap_cdp_result(envelope),
        Err(_) => (None, None),
    };
    results.push(json!({
        "experiment": "cdp-rejected-promise",
        "expect": "拒绝的 Promise 能被区分出来（异常描述可读）",
        "ok": reject_exception.is_some(),
        "envelope": reject_result.as_ref().ok(),
        "value": reject_value,
        "exception": reject_exception,
    }));

    // --- 实验 3b：不开 awaitPromise 的对照 ---
    // 用来证明「必须显式 awaitPromise」：不开时拿到的应该是 Promise 对象的
    // 序列化（空对象），而不是结算后的值。
    let no_await_result =
        cdp_probe::cdp_eval(&window, promise_expr, false, Duration::from_secs(10)).await;
    let (no_await_value, _) = match &no_await_result {
        Ok(envelope) => cdp_probe::unwrap_cdp_result(envelope),
        Err(_) => (None, None),
    };
    results.push(json!({
        "experiment": "cdp-without-await-promise",
        "expect": "对照组：不开 awaitPromise 时拿不到结算值（说明必须显式开）",
        "ok": no_await_result.is_ok(),
        "envelope": no_await_result.as_ref().ok(),
        "value": no_await_value,
    }));

    // --- 实验 4：求值后，页面 JS 空间有没有多出东西 ---
    let after = read_json(
        &window,
        "JSON.stringify({ ownNames: Object.getOwnPropertyNames(window).length, ownNamesSample: Object.getOwnPropertyNames(window).filter(function (n) { return n.indexOf('__dsh') === 0; }) })",
        Duration::from_secs(3),
    )
    .await;
    let own_before = before
        .as_ref()
        .ok()
        .and_then(|v| v.as_str())
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|v| v.get("ownNames").and_then(Value::as_u64));
    let own_after = after
        .as_ref()
        .ok()
        .and_then(|v| v.as_str())
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|v| v.get("ownNames").and_then(Value::as_u64));
    let zero_trace = match (own_before, own_after) {
        (Some(b), Some(a)) => a == b,
        _ => false,
    };
    results.push(json!({
        "experiment": "page-zero-trace",
        "expect": "求值前后页面 JS 空间的全局量数量不变（零痕迹）",
        "ok": zero_trace,
        "own_names_before": own_before,
        "own_names_after": own_after,
        "before": before.as_ref().ok(),
        "after": after.as_ref().ok(),
    }));

    json!({
        "probe": "official-chat CDP round-trip feasibility (G2 assessment)",
        "page": "local controlled page (no real site is contacted)",
        "devtools_window_opened": false,
        "page_ready": match &ready {
            Ok(ms) => json!({ "ok": true, "elapsed_ms": ms }),
            Err(e) => json!({ "ok": false, "error": e }),
        },
        "experiments": results,
    })
    .to_string()
}

// --- 模式二：首站最小只读探测 -----------------------------------------------

/// 首站探测。
///
/// 纪律：**只读、低频、少量**。本轮只打目录接口、只取结构；**不读任何会话
/// 正文**。页内脚本只把结构（键名 / 条数 / 布尔量）回传宿主——标题、正文、
/// 凭据都不离开页面。
fn run_site_probe(
    handle: &tauri::AppHandle,
    url: &str,
    site: SiteSpec,
    profile: Option<&std::path::Path>,
) -> String {
    let window = match build_window(handle, "official-chat-site-probe", url, profile) {
        Ok(window) => window,
        Err(e) => return json!({ "fatal": format!("无法创建探测窗口：{e}") }).to_string(),
    };

    tauri::async_runtime::block_on(async move {
        // 等站点页面真正就绪：origin 落到该站点的有效 host + 文档 complete。
        let ready = wait_for_site(&window, site.hosts).await;
        let mut report = json!({
            "probe": "official-chat first-site read-only probe (G2)",
            "site": site.key,
            "requested_url": site.url,
            "profile": profile.map(|p| p.display().to_string()),
            "page_ready": match &ready {
                Ok(ms) => json!({ "ok": true, "elapsed_ms": ms }),
                Err(e) => json!({ "ok": false, "error": e }),
            },
        });

        if ready.is_err() {
            report["measurements"] = json!({ "skipped": "页面未就绪，未发起任何接口请求" });
            return report.to_string();
        }

        // 页面事实：**不发任何网络请求**，对所有站点都采一次。
        // 「最终 origin 落到哪」「凭据在不在」「是不是登录页」这三件事，
        // 与是否知道目录端点无关。
        let facts = collect_page_facts(&window).await;
        report["page_facts"] = facts.clone();

        match site.directory_path {
            None => {
                report["measurements"] = json!({
                    "directory_endpoint": "未验证（该站点的目录接口尚无公开可查的端点）",
                    "page_requests": 0,
                });
            }
            Some(path) => {
                let expression = build_measurement_expression(path);
                let started = Instant::now();
                let outcome = round_trip(&window, &expression, Duration::from_secs(30)).await;
                let elapsed = started.elapsed().as_millis() as u64;
                let measurements = match outcome {
                    Ok(trip) => json!({
                        "ok": true,
                        "elapsed_ms": elapsed,
                        "polls": trip.polls,
                        "result": trip.value,
                    }),
                    Err(e) => json!({
                        "ok": false,
                        "elapsed_ms": elapsed,
                        "error": format!("{e}"),
                    }),
                };
                report["measurements"] = measurements;
            }
        }

        report.to_string()
    })
}

/// 页面内的测量脚本。**只回结构，不回内容**。
///
/// 关键问题：「同源 fetch 会不会自动带上站点的应用级认证」——站点自己的
/// JS 是在发请求前手工加 `Authorization` 头的，裸 `fetch` 不会自动继承。
/// 所以这里分两步各打一次：一次裸的、一次带页面里能找到的 token。
fn build_measurement_expression(directory_path: &str) -> String {
    format!(
        r#"(async function () {{
  var ENDPOINT = {endpoint};
  var out = {{
    origin: location.origin,
    href: location.href,
    readyState: document.readyState,
    localStorageKeys: [],
    sessionStorageKeys: [],
    visibleCookieNames: [],
    // **「键在」与「服务端接受」是两件事**（2026-10-20 实测：两次都是
    // 「键在、值非空」而服务端回 40002 / 40003）。所以拆成两个独立字段，
    // 谁也不许从另一个推出来。
    tokenKeyTried: null,
    tokenKeysTried: [],
    tokenPresent: false,
    tokenAccepted: false,
    tokenAcceptEvidence: null,
    pageRequests: 0,
    attempts: []
  }};
  try {{ out.localStorageKeys = Object.keys(localStorage); }} catch (e) {{}}
  try {{ out.sessionStorageKeys = Object.keys(sessionStorage); }} catch (e) {{}}
  try {{
    out.visibleCookieNames = document.cookie.split(";").map(function (s) {{
      return s.split("=")[0].trim();
    }}).filter(Boolean);
  }} catch (e) {{}}

  function summarize(text) {{
    var summary = {{ bodyLength: text.length }};
    try {{
      var parsed = JSON.parse(text);
      summary.json = true;
      summary.topKeys = Object.keys(parsed);
      if (typeof parsed.code !== "undefined") summary.code = parsed.code;
      // msg 是站点给的业务提示（不含凭据），截断保留。
      if (typeof parsed.msg === "string") summary.msg = parsed.msg.slice(0, 200);
      if (typeof parsed.total !== "undefined") {{ summary.totalPresent = true; summary.total = parsed.total; }}
      if (Array.isArray(parsed.items)) {{
        summary.itemCount = parsed.items.length;
        if (parsed.items.length > 0) summary.itemKeys = Object.keys(parsed.items[0]);
      }}
      var data = parsed.data;
      if (data === null || typeof data === "undefined") {{
        summary.dataType = data === null ? "null" : "absent";
      }} else if (typeof data === "object") {{
        summary.dataType = "object";
        summary.dataKeys = Object.keys(data);
        var biz = data.biz_data;
        if (biz && typeof biz === "object") {{
          summary.bizKeys = Object.keys(biz);
          if (typeof biz.has_more !== "undefined") summary.hasMore = Boolean(biz.has_more);
          var sessions = biz.chat_sessions;
          if (Array.isArray(sessions)) {{
            summary.itemCount = sessions.length;
            if (sessions.length > 0) summary.itemKeys = Object.keys(sessions[0]);
          }}
        }}
      }} else {{
        summary.dataType = typeof data;
      }}
    }} catch (e) {{
      summary.json = false;
      summary.head = text.slice(0, 120);
    }}
    return summary;
  }}

  async function attempt(label, headers) {{
    out.pageRequests += 1;
    try {{
      var res = await fetch(ENDPOINT, {{ credentials: "include", headers: headers || {{}} }});
      var text = await res.text();
      out.attempts.push({{
        label: label,
        status: res.status,
        ok: res.ok,
        contentType: res.headers.get("content-type"),
        summary: summarize(text)
      }});
    }} catch (e) {{
      out.attempts.push({{ label: label, error: String(e) }});
    }}
  }}

  // 1) 裸 fetch：同源 cookie 够不够？
  await attempt("plain-same-origin-fetch", {{ "accept": "application/json" }});

  // 1b) 凭据的**长度**（不是值）：用来判断「键在但值已空 / 值还在」。
  //     长度不泄露凭据内容，是可以安全回传的最小信息。
  var credKeys = ["userToken", "settingsJwt", "__appKit_userInfo", "_token", "accessToken", "sessionToken"];
  out.credentialLengths = {{}};
  for (var c = 0; c < credKeys.length; c++) {{
    var ck = credKeys[c];
    try {{
      if (localStorage.getItem(ck) !== null) {{
        out.credentialLengths[ck] = String(localStorage.getItem(ck)).length;
      }}
    }} catch (e) {{}}
  }}

  // 1c) 页面是不是停在登录/注册页（这会直接解释「接口为什么返回未登录」）。
  out.pageLooksLikeAuth = /sign[_-]?in|sign[_-]?up|log[_-]?in|register|auth/i.test(String(location.href));

  // 1d) R2（本地回环中继）的前提：站点的 connect-src 是否放行回环地址。
  //     判据用标准信号 `securitypolicyviolation`：只有 CSP 违规才会触发它
  //     （混合内容、CORS 失败都不触发），所以「有没有 violation 且
  //     directive 是 connect-src」就回答了「R2 会不会被 CSP 挡住」。
  //     探测只发一个到本机回环的请求，不碰任何第三方。
  out.cspViolations = [];
  try {{
    document.addEventListener("securitypolicyviolation", function (e) {{
      out.cspViolations.push({{ directive: String(e.violatedDirective), blockedURI: String(e.blockedURI) }});
    }});
  }} catch (e) {{}}
  try {{
    await fetch("http://127.0.0.1:9/loopback-probe", {{ mode: "no-cors", cache: "no-store" }});
  }} catch (e) {{
    // 连不上是预期的（回环探测），我们要看的是有没有 CSP violation。
  }}
  // 给 violation 事件留出派发时间。
  await new Promise(function (resolve) {{ setTimeout(resolve, 300); }});
  out.connectSrcBlocksLoopback = out.cspViolations.some(function (v) {{
    return v.directive.indexOf("connect-src") === 0 && v.blockedURI.indexOf("127.0.0.1") >= 0;
  }});

  // 2) 带上页面里能拿到的 token（若站点把凭据放在 localStorage）。
  //    凭据只在页面里用，**不离开页面**；宿主只拿到「有没有、能不能用」。
  //
  //    **按显式优先级取，不按枚举顺序**（2026-10-20 实测踩过）：原来那句
  //    「第一个 /token/i 匹配的键」在真实 profile 命中 `userToken`，在全新
  //    profile 命中 `__tea_cache_tokens_20006317`（腾讯分析缓存键）——两次测的
  //    根本不是同一个东西，结论不可复现。优先级表是显式的，且**输出实际
  //    试过的键名**，读结果的人才知道这次测的是哪一个。
  var TOKEN_KEY_PRIORITY = ["userToken", "accessToken", "sessionToken", "_token"];
  var token = null;
  var tokenKey = null;
  var tokenKeysTried = [];
  for (var t = 0; t < TOKEN_KEY_PRIORITY.length; t++) {{
    var key = TOKEN_KEY_PRIORITY[t];
    var raw = null;
    try {{ raw = localStorage.getItem(key); }} catch (e) {{ continue; }}
    tokenKeysTried.push(key);
    if (!raw) continue;
    var value = null;
    try {{
      var wrapped = JSON.parse(raw);
      value = (wrapped && typeof wrapped.value === "string") ? wrapped.value : raw;
    }} catch (e) {{ value = raw; }}
    if (!value) continue;
    tokenKey = key;
    token = value;
    break;
  }}
  out.tokenKeysTried = tokenKeysTried;
  out.tokenKeyTried = tokenKey;
  // **「值非空」只是存在性，不是可用性**。
  out.tokenPresent = Boolean(token);
  if (token) {{
    var before = out.attempts.length;
    await attempt("with-site-bearer-token", {{
      "accept": "application/json",
      "authorization": "Bearer " + token
    }});
    // **「服务端接受」≠「状态码 200」**：ChatGPT 那次实测 200 但 items 为空
    // （未登录），DeepSeek 那次 200 但业务 code 是 40002 / 40003。
    // 判定要求三件事同时成立：HTTP 2xx、业务 code 表示成功、响应里确有内容。
    //
    // 「有内容」要**看值不看字段在不在**：ChatGPT 未登录时 `total: 0` 字段是
    // 在的——按「字段存在」判会把未登录判成已登录。
    var got = out.attempts[before];
    var sum = got && got.summary ? got.summary : {{}};
    var codeOk = (typeof sum.code === "undefined") || (sum.code === 0) || (sum.code === 200);
    var hasContent = (sum.itemCount > 0) || (sum.dataType === "object") || (sum.total > 0);
    out.tokenAccepted = Boolean(got && got.ok && codeOk && hasContent);
    out.tokenAcceptEvidence = {{
      status: got ? got.status : null,
      code: typeof sum.code === "undefined" ? null : sum.code,
      itemCount: typeof sum.itemCount === "undefined" ? null : sum.itemCount,
      dataType: typeof sum.dataType === "undefined" ? null : sum.dataType
    }};
  }}
  return out;
}})()"#,
        endpoint = serde_json::to_string(directory_path).unwrap_or_else(|_| "\"/\"".to_string()),
    )
}

/// 页面事实采集脚本：**不发起任何网络请求**。
///
/// 只读页面自身状态：最终 origin / 当前路径 / 存储里的键名 / 可见 cookie 名 /
/// 凭据的长度（不是值）/ 是否停在登录页。键名与长度都不泄露凭据内容。
fn build_page_facts_expression() -> String {
    r#"(function () {
  var out = {
    origin: location.origin,
    href: String(location.href),
    readyState: document.readyState,
    localStorageKeys: [],
    sessionStorageKeys: [],
    visibleCookieNames: [],
    credentialLengths: {},
    pageLooksLikeAuth: false
  };
  try { out.localStorageKeys = Object.keys(localStorage); } catch (e) {}
  try { out.sessionStorageKeys = Object.keys(sessionStorage); } catch (e) {}
  try {
    out.visibleCookieNames = document.cookie.split(";").map(function (s) {
      return s.split("=")[0].trim();
    }).filter(Boolean);
  } catch (e) {}
  var credKeys = ["userToken", "settingsJwt", "__appKit_userInfo", "_token", "accessToken", "sessionToken"];
  for (var c = 0; c < credKeys.length; c++) {
    try {
      if (localStorage.getItem(credKeys[c]) !== null) {
        out.credentialLengths[credKeys[c]] = String(localStorage.getItem(credKeys[c])).length;
      }
    } catch (e) {}
  }
  out.pageLooksLikeAuth = /sign[_-]?in|sign[_-]?up|log[_-]?in|register|auth/i.test(String(location.href));
  return out;
})()"#
    .to_string()
}

/// 采集页面事实（不发起网络请求）。
async fn collect_page_facts(window: &tauri::WebviewWindow) -> Value {
    match read_json(
        window,
        &build_page_facts_expression(),
        Duration::from_secs(5),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => json!({ "error": format!("{e}") }),
    }
}

/// 等站点页面就绪：origin 落到该站点的有效 host + 文档 complete。
async fn wait_for_site(window: &tauri::WebviewWindow, hosts: &[&str]) -> Result<u64, String> {
    let started = Instant::now();
    let mut last = String::new();
    loop {
        if started.elapsed() > Duration::from_secs(45) {
            return Err(format!("站点页面 45s 内没有就绪；最后状态：{last}"));
        }
        let probe = "JSON.stringify({ origin: location.origin, href: String(location.href), ready: document.readyState, hasBody: !!(document.body && document.body.children.length) })";
        if let Ok(Value::String(raw)) = read_json(window, probe, Duration::from_secs(1)).await {
            if let Ok(state) = serde_json::from_str::<Value>(&raw) {
                let origin = state["origin"].as_str().unwrap_or("");
                let ready = state["ready"].as_str().unwrap_or("");
                let has_body = state["hasBody"].as_bool().unwrap_or(false);
                last = raw.clone();
                if hosts.iter().any(|h| origin.contains(h)) && ready == "complete" && has_body {
                    return Ok(started.elapsed().as_millis() as u64);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

// --- 窗口与受控页面 ---------------------------------------------------------

/// 建一个探针窗口。
///
/// `profile` 给定时，复用官网对话的**专用 user-data 目录**与**同一套浏览器
/// 参数**（WebView2 要求同一个 user-data 目录下的所有环境配置一致），并注入
/// 与生产一致的指纹脚本——探针要量的就是**壳真实环境**下的站点行为。
fn build_window(
    handle: &tauri::AppHandle,
    label: &str,
    url: &str,
    profile: Option<&std::path::Path>,
) -> Result<tauri::WebviewWindow, String> {
    build_window_with(handle, label, url, profile, true)
}

/// 与 [`build_window`] 相同，但可以要求窗口不可见。
///
/// **注意**：同一个 user-data 目录下的所有 WebView2 环境，浏览器参数必须
/// 完全一致，否则 WebView2 会以 `ERROR_INVALID_STATE (0x8007139F)` 拒绝
/// 创建第二个 webview。所以陪衬窗口不能自己另起一套参数——它必须和探针
/// 窗口用同一套（`additional_browser_args` + `data_directory`）。
fn build_window_with(
    handle: &tauri::AppHandle,
    label: &str,
    url: &str,
    profile: Option<&std::path::Path>,
    visible: bool,
) -> Result<tauri::WebviewWindow, String> {
    let parsed = url.parse().map_err(|e| format!("URL 非法：{e}"))?;
    let mut builder =
        tauri::WebviewWindowBuilder::new(handle, label, tauri::WebviewUrl::External(parsed))
            .title("dsh 官方对话探针")
            .inner_size(1000.0, 760.0)
            .visible(visible)
            .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
            .initialization_script(include_str!("../src/harness/chat-fingerprint.js"));
    if let Some(profile) = profile {
        std::fs::create_dir_all(profile).map_err(|e| format!("无法创建 profile 目录：{e}"))?;
        builder = builder.data_directory(profile.to_path_buf());
    }
    builder
        .build()
        .map_err(|e| format!("无法创建探针窗口：{e}"))
}

/// 最小 HTTP 服务：只回两个静态页面。
struct ControlledServer {
    port: u16,
}

impl ControlledServer {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        std::thread::Builder::new()
            .name("official-chat-probe-http".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let _ = serve(&mut stream);
                }
            })?;
        Ok(Self { port })
    }
}

fn serve(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let path = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("/");
    let body = if path.starts_with("/nav-target") {
        NAV_PAGE
    } else {
        PAGE
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}
