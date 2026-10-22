//! **路线 B 端到端探针**（macOS）：验**生产那份**实现能不能取到真实会话目录。
//!
//! # 它与 `official_chat_macos_fetch_probe.rs` 的分工
//!
//! · `official_chat_macos_fetch_probe.rs`（2026-10-22 20:0x）：**问清契约**
//!   —— 认证头长什么样、token 在哪、路线 B 到底可不可行。**它自己实现**页内
//!   fetch，**不碰生产代码**。
//! · **本探针**：**验生产实现** —— 页内脚本、站点契约、裁剪逻辑**全部走生产
//!   那份**（`session_structure::capture_via_page` ＋ `SessionStructure::from_body_for`）。
//!
//! # 登录态怎么办
//!
//! 生产取数窗绑的是**官网对话窗口那一份** WKWebsiteDataStore（`dsh-chat-rel-001`
//! ／`dsh-chat-dev-001`）⇒ ⇒ 那是**用户真实档案** ⇒ ⇒ **探针不许用它**
//! （浏览器一启动就写 Cookies／LocalStorage）。
//!
//! ⇒ 本探针用**只读拷贝**：把用户那份存储**只读拷贝**一份到**探针自己的进程名**
//! 下面，探针写的是拷贝 ⇒ ⇒ 用户那份**一个字节都不改**（跑前/跑后比
//! `Cookies.binarycookies` 的 sha256 验证这一点）。
//!
//! ⚠️ **因此本探针没有验的那一步**：生产建窗时绑 `OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`
//! （那 1 行）—— 探针**必须**用自己的存储（纪律），所以那一步只能在真壳里验。
//!
//! # 本轮要回答的问题（2026-10-22 22:0x）
//!
//! 上一轮实测到：`capture_via_page` 的**就绪判定全过**（`readyState=complete`、
//! `origin_match=true`、`cred=true`、`stable=3` ⇒ 登录态在），紧接着「开始发那一发」
//! 之后**进程没了**（无 panic、无报告）。同一轮里三条诊断臂（纯 JS／fetch／
//! 带认证头的 fetch）**全部**回 `Err(Gone { "结果通道已关闭（webview 被销毁）" })`。
//!
//! ## 读源码得到的一条**可验的因果候选**
//!
//! wry-0.57.0 `src/wkwebview/mod.rs:723-725`：
//!
//! ```text
//! pub fn eval(&self, js: &str, callback: Option<impl Fn(String) + Send + 'static>) -> Result<()> {
//!   if let Some(scripts) = &mut *self.pending_scripts.lock().unwrap() {
//!     scripts.push(js.into());          // ← 只排队
//!   } else { …真的求值… }
//! ```
//!
//! ⇒ **排队分支里 `callback` 被直接丢掉**（它是 `Option<impl Fn…>`，函数结束时
//! 就析构）⇒ ⇒ 回程通道的 sender 没了 ⇒ `rx.recv()` 立刻返回 `None`
//! ⇒ 报「**结果通道已关闭**（webview 被销毁）」。
//!
//! 而 `pending_scripts` **只在首次导航 commit 时置空**
//! （`src/wkwebview/navigation.rs:28-35`：`did_commit_navigation` 里
//! `*pending_scripts = None`）⇒ ⇒ **建窗之后、首次导航落地之前**的每一次
//! 求值都会踩这个坑（脚本没跑 ＋ callback 被丢）。
//!
//! ## 台阶（一次跑完；每一步都带标记落盘）
//!
//! | 台阶 | 问什么 |
//! | --- | --- |
//! | S1 | 建窗后第一次求值成功吗？**连续重试到成功为止** ⇒ 验「首次导航 commit 前求值会中排队坑」 |
//! | S2 | 稳定后连发 3 次平凡求值 ⇒ 验「回程机制本身稳不稳」 |
//! | S3 | `webview.eval`（**无 callback**）注入的标记读得回来吗？ |
//! | S4 | `eval_with_callback`（**有 callback**）注入的标记读得回来吗？ |
//! | S5 | **生产那份** `round_trip`（真 Promise 往返） |
//! | S6 | **候选修法**：`eval_with_callback` 注入 ＋ `read_json` 轮询（含异步 Promise 一档） |
//! | S7 | **生产那份** `capture_via_page`（真活） |
//! | S8 | **生产那份** `SessionStructure::from_body_for`（裁剪） |
//!
//! # 读数纪律
//!
//! **不报**：会话标题原文、会话 id、任何凭据的值。
//! **报**：状态码、业务码、条数、键名、形状。

// 这条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块编译掉。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe: 本探针只支持 macOS（WKWebsiteDataStore 路线）。");
    std::process::exit(2);
}

/// **看门狗**：探针卡住时**自己**退出，不占着整轮等待。
#[cfg(target_os = "macos")]
const WATCHDOG_SECONDS: u64 = 240;

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::{json, Value};
    use tauri::Manager;

    use crate::session_structure::PageRoute;

    /// 探针自己的数据存储标识符（**不是**生产的任何一个）。
    const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-rbp-001";

    /// **源存储目录**从哪个环境变量读（源码里不写路径字面量）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// 探针的站点（DeepSeek —— 唯一有路线 B 适配的那一站）。
    const SITE: &str = "deepseek";

    /// **正文那条链的页内探针脚本**（S10）：页内取 token → 取目录 → 取**第一个**
    /// 会话 id → 页内 fetch 正文端点 ⇒ ⇒ **只回条数与形状**（不回 id、不回标题）。
    const BODY_PROBE_SCRIPT: &str = r#"
(async function () {
  var SPEC = __SPEC__;
  function readToken() {
    var raw = null;
    try { raw = window.localStorage.getItem(SPEC.key); } catch (e) { return null; }
    if (!raw) return null;
    try { var p = JSON.parse(raw); if (p && typeof p.value === 'string') return p.value; } catch (e) {}
    return raw;
  }
  var out = { ok: true };
  var token = readToken();
  if (!token) { out.ok = false; out.stage = 'token'; out.error = '页内没有凭据'; return out; }
  var headers = {};
  headers[SPEC.header] = SPEC.prefix + token;
  try {
    var catRes = await fetch(SPEC.catalogUrl, { credentials: 'include', headers: headers });
    var catText = await catRes.text();
    out.catalogStatus = catRes.status;
    out.catalogLength = catText.length;
    var sessions = [];
    try {
      var catJson = JSON.parse(catText);
      sessions = (catJson && catJson.data && catJson.data.biz_data && catJson.data.biz_data.chat_sessions) || [];
    } catch (e) { out.catalogParseError = String(e); }
    out.catalogCount = Array.isArray(sessions) ? sessions.length : -1;
    if (!Array.isArray(sessions) || sessions.length === 0) { out.stage = 'catalog-empty'; return out; }
    // 连测**前 5 条**会话的正文条数（**只报条数**，不报 id、不报标题）：
    // 判据 ① 每条都**非空**（空 ⇒ 拿到的是空增量）② 条数**各不相同**
    // （全一样 ⇒ 像是被某个固定上限截断）。
    var counts = [];
    var sample = Math.min(5, sessions.length);
    for (var i = 0; i < sample; i++) {
      var sid = sessions[i].id;
      var bodyUrl = SPEC.bodyPath + '?chat_session_id=' + encodeURIComponent(sid);
      try {
        var res = await fetch(bodyUrl, { credentials: 'include', headers: headers });
        var text = await res.text();
        var count = -1;
        var keys = [];
        try {
          var j = JSON.parse(text);
          var biz = (j && j.data && j.data.biz_data) || {};
          var msgs = biz.messages || biz.chat_messages || null;
          count = Array.isArray(msgs) ? msgs.length : -1;
          keys = (Array.isArray(msgs) && msgs.length) ? Object.keys(msgs[0] || {}) : [];
          if (i === 0) {
            out.bodyStatus = res.status;
            out.bodyLength = text.length;
            out.bodyCode = (j && typeof j.code !== 'undefined') ? j.code : null;
            out.bodyTopKeys = Object.keys(j || {});
            out.bizKeys = Object.keys(biz || {});
            out.messageKeys = keys;
          }
        } catch (e) { /* 这一条解析失败就记 -1 */ }
        counts.push(count);
      } catch (e) {
        counts.push(-2);
      }
    }
    out.messageCounts = counts;
    out.sampleSize = sample;
  } catch (e) {
    out.ok = false; out.stage = 'fetch'; out.error = String(e);
  }
  return out;
})()
"#;

    pub fn run() {
        // 看门狗线程：超时后**自己**退出，并留一行标记。
        std::thread::Builder::new()
            .name("route-b-watchdog".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(super::WATCHDOG_SECONDS));
                log_line("WATCHDOG", "超时，探针自己退出");
                std::process::exit(3);
            })
            .expect("probe: 无法起看门狗线程");

        log_line("probe", "run() 入口");
        let (store_dir, store_reading) = match prepare_store(&PROBE_DATA_STORE_IDENTIFIER) {
            Ok(pair) => pair,
            Err(e) => {
                let report = json!({ "fatal": format!("存储准备失败：{e}") });
                finish(&report.to_string());
                std::process::exit(2);
            }
        };
        let spec = match crate::session_structure::spec_for(SITE) {
            Some(spec) => spec,
            None => {
                let report = json!({ "fatal": "规格表里没有这个站点" });
                finish(&report.to_string());
                std::process::exit(2);
            }
        };
        let route = match spec.page_route {
            Some(route) => route,
            None => {
                let report = json!({ "fatal": "这个站点没有路线 B 适配" });
                finish(&report.to_string());
                std::process::exit(2);
            }
        };
        let url = match url::Url::parse(spec.root_url) {
            Ok(url) => url,
            Err(e) => {
                let report = json!({ "fatal": format!("站点地址非法：{e}") });
                finish(&report.to_string());
                std::process::exit(2);
            }
        };
        let _ = (store_dir, store_reading);
        log_line("probe", "存储与规格表就绪");

        // ⚠️ **`app.run()` 必须在主线程、且不能放进 `block_on`**：Tauri 的事件
        // 循环要在主线程跑。取数在 `RunEvent::Ready` 之后起的**工作线程**上做。
        let app = tauri::Builder::default()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("probe: 无法构建探针应用");
        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let url_for_run = url.clone();
        app.run(move |app, event| {
            if let tauri::RunEvent::Ready = event {
                if started.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let handle = app.clone();
                let url = url_for_run.clone();
                std::thread::Builder::new()
                    .name("official-chat-route-b-probe".into())
                    .spawn(move || {
                        log_line("probe", "工作线程启动");
                        let exit_handle = handle.clone();
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                tauri::async_runtime::block_on(async move {
                                    run_probe_body(&handle, &url, Some(route)).await
                                })
                            }));
                        let report = match outcome {
                            Ok(report) => report,
                            Err(panic) => {
                                let detail = panic
                                    .downcast_ref::<&str>()
                                    .map(|s| s.to_string())
                                    .or_else(|| panic.downcast_ref::<String>().cloned())
                                    .unwrap_or_else(|| "（不是字符串）".to_string());
                                json!({ "panic": detail }).to_string()
                            }
                        };
                        finish(&report);
                        exit_handle.exit(0);
                    })
                    .expect("probe: 无法起工作线程");
            }
        });
        log_line("probe", "app.run() 返回（事件循环结束）");
    }

    /// 报告落地：stdout ＋ 轮次独立命名的文件。
    fn finish(report: &str) {
        println!("PROBE_REPORT_BEGIN");
        println!("{report}");
        println!("PROBE_REPORT_END");
        log_line("report", report);
    }

    /// 每一步都落一行（**文件名按轮次独立命名** ⇒ 不会覆盖上一轮的原始证据）。
    fn log_line(tag: &str, message: &str) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("[{stamp}] {tag} {message}");
        eprintln!("{line}");
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path())
        {
            let _ = file.write_all(format!("{line}\n").as_bytes());
            let _ = file.flush();
        }
    }

    /// 证据日志的路径（**轮次独立命名**）。
    fn log_path() -> PathBuf {
        let round = std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string());
        std::env::temp_dir().join(format!("dsh-route-b-probe.{round}.log"))
    }

    /// 把一个值压成一行短读数（**不报凭据值**）。
    fn brief(value: &Value) -> String {
        let text = value.to_string();
        if text.chars().count() > 140 {
            format!("{}…", text.chars().take(140).collect::<String>())
        } else {
            text
        }
    }

    /// 数据存储标识符 → WKWebView 持久化目录用的那串 hex（8-4-4-4-12）。
    fn identifier_hex(identifier: &[u8; 16]) -> String {
        let hex: String = identifier.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    /// 探针自己的 WK 存储目录。
    fn probe_store_dir(identifier: &[u8; 16]) -> Result<PathBuf, String> {
        let home = std::env::var("HOME").map_err(|e| format!("读不到 HOME：{e}"))?;
        let exe = std::env::current_exe().map_err(|e| format!("读不到自身路径：{e}"))?;
        let app_dir = exe
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "自身文件名不是合法 UTF-8".to_string())?;
        Ok(Path::new(&home)
            .join("Library")
            .join("WebKit")
            .join(app_dir)
            .join("WebsiteDataStore")
            .join(identifier_hex(identifier)))
    }

    /// 把用户那份存储**只读拷贝**到探针自己的目录。
    fn prepare_store(identifier: &[u8; 16]) -> Result<(PathBuf, Value), String> {
        let source = std::env::var(ENV_SOURCE)
            .map_err(|_| format!("没给环境变量 {ENV_SOURCE}（源存储目录）"))?;
        let source = PathBuf::from(source);
        if !source.is_dir() {
            return Err(format!("源存储目录不存在：{}", source.display()));
        }
        let target = probe_store_dir(identifier)?;
        if target.exists() {
            let _ = std::fs::remove_dir_all(&target);
        }
        let report = crate::profile_copy::copy_dir_readonly(&source, &target)
            .map_err(|e| format!("只读拷贝失败：{e}"))?;
        Ok((
            target.clone(),
            json!({
                "ok": true,
                "target_dir": target.display().to_string(),
                "identifier_hex": identifier_hex(identifier),
                "copied": report.files,
                "bytes": report.bytes,
                "note": "源是**只读拷贝**：探针写的全在拷贝里，用户正在跑的那份存储一个字节都没改",
            }),
        ))
    }

    /// 建窗（**探针自己的**存储标识符 ⇒ **不碰**用户真实档案）。
    fn build_window(
        app: &tauri::AppHandle,
        label: &str,
        url: &url::Url,
        identifier: &[u8; 16],
        visible: bool,
    ) -> Result<tauri::WebviewWindow, String> {
        // **无条件**给 `data_directory`（判据 `probe-profile-isolation` 按**代码
        // 形状**判，「有才给」同样违规）。macOS 上 WKWebView **忽略**它
        // （它认的是 `data_store_identifier`），但照样给——口径一致。
        let profile = std::env::temp_dir().join("dsh-official-chat-route-b-probe");
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("无法创建临时 profile 目录（{}）：{e}", profile.display()))?;

        // ⚠️ **建窗就直奔给定的 URL、之后不再导航**：`capture_via_page` **本身
        // 不含导航**（导航在生产的 `capture_in` 里）⇒ ⇒ 探针不该替它导航。
        let start_url = url.clone();
        let builder =
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(start_url))
                .title("dsh 路线 B 探针")
                .inner_size(900.0, 700.0)
                .visible(visible)
                .data_directory(profile)
                .additional_browser_args(crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
                .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
                .data_store_identifier(*identifier);

        log_line(
            "probe",
            &format!("准备 build()（label={label} visible={visible}）"),
        );
        let built = builder.build();
        log_line("probe", "build() 返回");
        built.map_err(|e| format!("无法创建探针窗口：{e}"))
    }

    /// 实测主体（在 Tauri 事件循环起来之后）。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &url::Url,
        route: Option<PageRoute>,
    ) -> String {
        let mut report = json!({});
        log_line("S0", "开始建窗");
        let window = match build_window(
            app,
            "official-chat-route-b-probe",
            url,
            &PROBE_DATA_STORE_IDENTIFIER,
            true,
        ) {
            Ok(window) => window,
            Err(e) => {
                report["fatal"] = json!(e);
                return report.to_string();
            }
        };
        let Some(route) = route else {
            report["fatal"] = json!("没有路线 B 适配");
            return report.to_string();
        };
        report["route"] = json!({
            "url": route.url,
            "origin": route.origin,
            "storage": route.auth.storage.as_str(),
            "key": route.auth.key,
            "extract": route.auth.extract.as_str(),
            "header": route.auth.header,
        });

        // ── S1：建窗之后第一次求值成功吗？连续重试到成功为止 ──────────────
        // 这一格直接验「首次导航 commit 前，求值会被排队、callback 会被丢掉」。
        {
            let started = std::time::Instant::now();
            let mut attempts: u32 = 0;
            let mut first_ok_at_ms: Option<u64> = None;
            let mut errors: Vec<String> = Vec::new();
            loop {
                attempts += 1;
                let result =
                    crate::script_eval::read_json(&window, "1+1", Duration::from_secs(2)).await;
                match result {
                    Ok(value) => {
                        log_line(
                            "S1",
                            &format!(
                                "第 {attempts} 次求值成功：{}（{}ms）",
                                brief(&value),
                                started.elapsed().as_millis()
                            ),
                        );
                        first_ok_at_ms = Some(started.elapsed().as_millis() as u64);
                        break;
                    }
                    Err(e) => {
                        if errors.len() < 6 {
                            errors.push(format!("#{attempts}: {e:?}"));
                        }
                        if started.elapsed() >= Duration::from_secs(45) {
                            log_line("S1", "45s 内没等到第一次成功");
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
            report["s1_first_eval"] = json!({
                "attempts": attempts,
                "first_ok_at_ms": first_ok_at_ms,
                "errors": errors,
            });
        }

        // ── S2：稳定后连发 3 次平凡求值（顺带看页面有没有在跳） ────────────
        {
            let mut readings: Vec<Value> = Vec::new();
            for index in 1..=3u32 {
                let result = crate::script_eval::read_json(
                    &window,
                    "JSON.stringify({href: String(location.href), ready: String(document.readyState)})",
                    Duration::from_secs(3),
                )
                .await;
                log_line("S2", &format!("第 {index} 次：{result:?}"));
                readings.push(match result {
                    Ok(value) => value,
                    Err(e) => json!({ "error": format!("{e:?}") }),
                });
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
            report["s2_repeat_eval"] = json!(readings);
        }

        // ── S2b：**隐藏**窗口后还能不能求值（生产的取数窗是 `visible(false)`） ─
        {
            let _ = window.hide();
            tokio::time::sleep(Duration::from_millis(400)).await;
            let result = crate::script_eval::read_json(
                &window,
                "JSON.stringify({href: String(location.href), ready: String(document.readyState)})",
                Duration::from_secs(3),
            )
            .await;
            log_line("S2b", &format!("隐藏窗口后：{result:?}"));
            report["s2b_hidden_window_eval"] = match &result {
                Ok(value) => json!(brief(value)),
                Err(e) => json!(format!("{e:?}")),
            };
            let _ = window.show();
        }

        // ── S3：`webview.eval`（**无 callback**）注入的标记读得回来吗？ ─────
        {
            let marker = "window.__dsh_probe_m3";
            let injected = window.eval(format!("{marker} = 'm3';"));
            log_line("S3", &format!("注入（无 callback）返回：{injected:?}"));
            tokio::time::sleep(Duration::from_millis(200)).await;
            let read = crate::script_eval::read_json(&window, marker, Duration::from_secs(3)).await;
            log_line("S3", &format!("读回：{read:?}"));
            report["s3_eval_no_callback"] = json!({
                "inject_result": format!("{injected:?}"),
                "read_back": match &read {
                    Ok(value) => brief(value),
                    Err(e) => format!("{e:?}"),
                },
            });
        }

        // ── S4：`eval_with_callback`（**有 callback**）注入的标记读得回来吗？ ─
        {
            let marker = "window.__dsh_probe_m4";
            let injected = window.eval_with_callback(format!("{marker} = 'm4';"), |_| {});
            log_line("S4", &format!("注入（有 callback）返回：{injected:?}"));
            tokio::time::sleep(Duration::from_millis(200)).await;
            let read = crate::script_eval::read_json(&window, marker, Duration::from_secs(3)).await;
            log_line("S4", &format!("读回：{read:?}"));
            report["s4_eval_with_callback"] = json!({
                "inject_result": format!("{injected:?}"),
                "read_back": match &read {
                    Ok(value) => brief(value),
                    Err(e) => format!("{e:?}"),
                },
            });
        }

        // ── S5：生产那份 `round_trip`（真 Promise 往返） ─────────────────────
        {
            let result = crate::script_eval::round_trip(
                &window,
                "Promise.resolve('a-ok')",
                Duration::from_secs(10),
            )
            .await;
            log_line("S5", &format!("生产 round_trip：{result:?}"));
            report["s5_production_round_trip"] = match &result {
                Ok(trip) => json!({
                    "value": brief(&trip.value),
                    "elapsed_ms": trip.elapsed_ms,
                    "polls": trip.polls,
                }),
                Err(e) => json!({ "error": format!("{e:?}") }),
            };
        }

        // ── S6：候选修法 —— `eval_with_callback` 注入 ＋ `read_json` 轮询 ──
        {
            let slot = "__dsh_probe_slot_sync";
            let injected = window.eval_with_callback(
                format!(
                    "Object.defineProperty(window, '{slot}', {{ value: {{ state: 'done', value: 'sync-ok' }}, writable: true, configurable: true, enumerable: false }});"
                ),
                |_| {},
            );
            log_line("S6", &format!("注入（同步档）返回：{injected:?}"));
            let sync = poll_slot(&window, slot, Duration::from_secs(6)).await;
            log_line("S6", &format!("同步档轮询：{sync:?}"));

            let slot_async = "__dsh_probe_slot_async";
            let injected_async = window.eval_with_callback(
                format!(
                    "Promise.resolve('async-ok').then(function (v) {{ Object.defineProperty(window, '{slot_async}', {{ value: {{ state: 'done', value: v }}, writable: true, configurable: true, enumerable: false }}); }});"
                ),
                |_| {},
            );
            log_line("S6", &format!("注入（异步档）返回：{injected_async:?}"));
            let async_read = poll_slot(&window, slot_async, Duration::from_secs(6)).await;
            log_line("S6", &format!("异步档轮询：{async_read:?}"));

            report["s6_candidate_inject_poll"] = json!({
                "sync_inject": format!("{injected:?}"),
                "sync_read": match &sync {
                    Ok(value) => brief(value),
                    Err(e) => format!("{e:?}"),
                },
                "async_inject": format!("{injected_async:?}"),
                "async_read": match &async_read {
                    Ok(value) => brief(value),
                    Err(e) => format!("{e:?}"),
                },
            });
        }

        // ── S6b：**生产那份取数脚本**直接喂给 `round_trip` ──────────────────
        // 这一格把「脚本自己失败」与「capture_via_page 的管线失败」分开：
        // 脚本失败时它自己会回 `{ok:false, stage, error}` ⇒ 阶段与原因都在读数里。
        {
            let spec = json!({
                "url": route.url,
                "storage": route.auth.storage.as_str(),
                "key": route.auth.key,
                "extract": route.auth.extract.as_str(),
                "header": route.auth.header,
                "prefix": route.auth.prefix,
            });
            let spec = serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string());
            let script = include_str!("../src/official_chat/page_fetch_script.js")
                .replace("__SPEC__", &spec);
            let result =
                crate::script_eval::round_trip(&window, &script, Duration::from_secs(30)).await;
            log_line(
                "S6b",
                &format!("生产取数脚本的 round_trip：{result:?}")
                    .chars()
                    .take(400)
                    .collect::<String>(),
            );
            report["s6b_production_fetch_script"] = match &result {
                Ok(trip) => {
                    // 脚本回的是**对象**（2026-10-22 22:0x 起改了契约：不再回文本）。
                    let value = &trip.value;
                    let ok = value.get("ok").and_then(Value::as_bool);
                    let stage = value.get("stage").and_then(Value::as_str).unwrap_or("");
                    let error = value.get("error").and_then(Value::as_str).unwrap_or("");
                    let status = value.get("status").and_then(Value::as_i64).unwrap_or(0);
                    let body = value.get("body").and_then(Value::as_str).unwrap_or("");
                    let body_shape: Value = serde_json::from_str::<Value>(body)
                        .map(|parsed: Value| {
                            json!({
                                "top_keys": parsed
                                    .as_object()
                                    .map(|map| map.keys().cloned().collect::<Vec<_>>())
                                    .unwrap_or_default(),
                                "code": parsed.get("code").cloned().unwrap_or(Value::Null),
                            })
                        })
                        .unwrap_or_else(|e| json!({ "not_json": format!("{e}") }));
                    json!({
                        "ok": ok,
                        "stage": stage,
                        "error": error,
                        "status": status,
                        "body_length": body.chars().count(),
                        "body_shape": body_shape,
                        "elapsed_ms": trip.elapsed_ms,
                        "polls": trip.polls,
                    })
                }
                Err(e) => json!({ "error": format!("{e:?}") }),
            };
        }

        // ── S7：生产那份 `capture_via_page`（真活） ────────────────────────
        {
            let captured =
                crate::session_structure::capture_via_page(&window, &route, route.url).await;
            log_line(
                "S7",
                &format!("生产 capture_via_page：{:?}", captured.is_ok()),
            );
            match captured {
                Ok(captured) => {
                    report["captured"] = json!({
                        "status": captured.status,
                        "body_length": captured.body.len(),
                        "matched": captured.matched,
                        "rule_matched": captured.rule_matched,
                        "rule_detail": captured.rule_detail,
                    });
                    // ── S8：生产那份裁剪 ────────────────────────────────────
                    match crate::session_structure::SessionStructure::from_body_for(
                        &captured.body,
                        captured.status,
                        crate::session_structure::spec_for(SITE).expect("规格表里必须有这一条"),
                    ) {
                        Ok(structure) => {
                            report["structure"] = json!({
                                "top_keys": structure.top_keys,
                                "code": structure.code,
                                "msg": structure.msg,
                                "data_keys": structure.data_keys,
                                "sessions_length": structure.sessions_length,
                                "has_more": structure.has_more,
                                "first_item_keys": structure.first_item_keys,
                                "item_types": structure.item_types,
                                "items_count": structure.items.len(),
                                "note": "标题原文与会话 id **都不进读数**（条目面只报条数与结构）",
                            });
                        }
                        Err(e) => {
                            report["structure"] = json!({ "error": e.detail() });
                        }
                    }
                }
                Err(e) => {
                    report["capture_error"] = json!({
                        "code": e.code(),
                        "detail": e.detail(),
                    });
                }
            }
        }

        // ── S10：正文那条链在路线 B 下能不能拿到**非空**正文 ──────────────
        // ⚠️ **为什么必须实测**：正文端点是**增量（MERGE）协议**（台账 12:3x）
        // —— 站点按「客户端缓存到第几版」只回**增量** ⇒ ⇒ Windows 侧专门准备了
        // 「冷档案」副本。⇒ 路线 B 自己构造请求（**只带 `authorization`**，不带
        // 站点的缓存版本头）到底拿到**全量**还是**空增量**，**没有读数**。
        // 这一格就量这件事：页内取第一个会话 id → 页内 fetch 正文端点 → 只报
        // **条数与形状**（不报 id、不报标题）。
        {
            let spec = json!({
                "key": route.auth.key,
                "header": route.auth.header,
                "prefix": route.auth.prefix,
                "catalogUrl": route.url,
                "bodyPath": "/api/v0/chat/history_messages",
            });
            let spec = serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string());
            let script = BODY_PROBE_SCRIPT.replace("__SPEC__", &spec);
            let result =
                crate::script_eval::round_trip(&window, &script, Duration::from_secs(30)).await;
            log_line(
                "S10",
                &format!("正文端点（路线 B）：{result:?}")
                    .chars()
                    .take(400)
                    .collect::<String>(),
            );
            report["s10_body_via_page"] = match &result {
                Ok(trip) => json!({
                    "value": trip.value,
                    "elapsed_ms": trip.elapsed_ms,
                    "polls": trip.polls,
                }),
                Err(e) => json!({ "error": format!("{e:?}") }),
            };
        }

        // ── S11：**正文那条链**在 macOS 上（路线 B） ──────────────────────
        // 走**生产那份**取数：用目录响应里的**第一个会话 id** 填正文 URL 模板
        // ⇒ ⇒ 再调生产的 `capture_via_page`。
        // ⇒ **只报条数与形状**（不报 id、不报标题）。
        //
        // ⚠️ **为什么不在这里调生产的 `SessionBody::from_body_for`**：那一层是
        // **跨平台共享的纯逻辑**（Windows 那条链已经在用）⇒ ⇒ macOS 没有专属
        // 行为要验；把它引进来反而要把 `official_chat::protocol` 整套拖进探针
        // （与探针里 harness 的同名模块撞车）。
        {
            let body_template = route.body_url;
            // 取一个真实的会话 id（**只在内层用**，不进读数）。
            let session_id = match crate::session_structure::capture_via_page(
                &window, &route, route.url,
            )
            .await
            {
                Ok(captured) => {
                    let parsed: Value = serde_json::from_str(&captured.body).unwrap_or(Value::Null);
                    parsed
                        .pointer("/data/biz_data/chat_sessions/0/id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                }
                Err(e) => {
                    log_line("S11", &format!("取目录失败：{e:?}"));
                    String::new()
                }
            };
            if session_id.is_empty() {
                report["s11_body_chain"] = json!({ "skipped": "没取到会话 id" });
            } else {
                let url = body_template
                    .map(|template| template.replace("{session}", &session_id))
                    .unwrap_or_default();
                let captured =
                    crate::session_structure::capture_via_page(&window, &route, &url).await;
                log_line(
                    "S11",
                    &format!("正文那条（路线 B）成功？{}", captured.is_ok()),
                );
                match captured {
                    Ok(captured) => {
                        let parsed: Value =
                            serde_json::from_str(&captured.body).unwrap_or(Value::Null);
                        let biz = parsed
                            .pointer("/data/biz_data")
                            .cloned()
                            .unwrap_or(Value::Null);
                        let messages = biz
                            .get("chat_messages")
                            .or_else(|| biz.get("messages"))
                            .and_then(Value::as_array)
                            .map(|items| items.len())
                            .unwrap_or(0);
                        report["s11_body_chain"] = json!({
                            "status": captured.status,
                            "body_length": captured.body.len(),
                            "messages_count": messages,
                            "biz_keys": biz
                                .as_object()
                                .map(|map| map.keys().cloned().collect::<Vec<_>>())
                                .unwrap_or_default(),
                            "code": parsed.get("code").cloned().unwrap_or(Value::Null),
                            "note": "会话 id 与正文原文**都不进读数**",
                        });
                    }
                    Err(e) => {
                        report["s11_body_chain"] = json!({
                            "code": e.code(),
                            "detail": e.detail(),
                        });
                    }
                }
            }
        }

        report["window_count_after"] = json!(app.webview_windows().len());

        // ── S9：**生产那扇窗的形状**（about:blank 起 ＋ 隐藏 ＋ 导航一次） ──
        // 生产 `build_capture_window`（macOS）就是：建在 `about:blank`、
        // `visible(false)`，然后 `capture_in` 导航一次 ⇒ ⇒ 这一格照抄那个形状。
        {
            let blank = url::Url::parse("about:blank").expect("about:blank 一定合法");
            match build_window(
                app,
                "official-chat-route-b-probe-b",
                &blank,
                &PROBE_DATA_STORE_IDENTIFIER,
                false,
            ) {
                Ok(window_b) => {
                    let nav = window_b.navigate(url.clone());
                    log_line("S9", &format!("导航返回：{nav:?}"));
                    let captured =
                        crate::session_structure::capture_via_page(&window_b, &route, route.url)
                            .await;
                    log_line(
                        "S9",
                        &format!(
                            "生产 capture_via_page（生产形状）成功？{}",
                            captured.is_ok()
                        ),
                    );
                    match captured {
                        Ok(captured) => {
                            report["s9_production_shape"] = json!({
                                "status": captured.status,
                                "body_length": captured.body.len(),
                                "matched": captured.matched,
                                "rule_matched": captured.rule_matched,
                            });
                            match crate::session_structure::SessionStructure::from_body_for(
                                &captured.body,
                                captured.status,
                                crate::session_structure::spec_for(SITE)
                                    .expect("规格表里必须有这一条"),
                            ) {
                                Ok(structure) => {
                                    report["s9_structure"] = json!({
                                        "top_keys": structure.top_keys,
                                        "code": structure.code,
                                        "msg": structure.msg,
                                        "data_keys": structure.data_keys,
                                        "sessions_length": structure.sessions_length,
                                        "has_more": structure.has_more,
                                        "first_item_keys": structure.first_item_keys,
                                        "item_types": structure.item_types,
                                        "items_count": structure.items.len(),
                                        "note": "标题原文与会话 id **都不进读数**",
                                    });
                                }
                                Err(e) => {
                                    report["s9_structure"] = json!({ "error": e.detail() });
                                }
                            }
                        }
                        Err(e) => {
                            report["s9_production_shape"] = json!({
                                "code": e.code(),
                                "detail": e.detail(),
                            });
                        }
                    }
                }
                Err(e) => {
                    report["s9_production_shape"] = json!({ "error": e });
                }
            }
        }

        // ⚠️ **不许在这里关窗**（2026-10-22 22:0x 实测踩到过）：关掉最后一扇窗
        // ⇒ Tauri 结束事件循环 ⇒ `app.run()` 返回 ⇒ **主线程结束 ⇒ 进程立刻退出**
        // ⇒ 工作线程还没来得及打印报告就被带走 ⇒ 读数全丢（上一轮「进程正常
        // exit 0、无报告」就是这个原因，**不是** webview 崩了）。
        // ⇒ 报告由工作线程打印完再 `exit(0)` 收尾。
        report.to_string()
    }

    /// 轮询页内某个槽位（**只用 `read_json`** ⇒ 只用已证明可用的那条回程）。
    async fn poll_slot(
        window: &tauri::WebviewWindow,
        slot: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        let started = std::time::Instant::now();
        let expression = format!(
            "window[{}]",
            serde_json::to_string(slot).unwrap_or_else(|_| "\"\"".to_string())
        );
        loop {
            let read =
                match crate::script_eval::read_json(window, &expression, Duration::from_secs(2))
                    .await
                {
                    Ok(value) if value.is_object() => return Ok(value),
                    Ok(value) => format!("空值：{}", brief(&value)),
                    Err(e) => format!("{e:?}"),
                };
            if started.elapsed() >= timeout {
                return Err(read);
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    }
}

// 生产代码里 `crate::shell::…` 与 `crate::commands::AppState` 是**壳自己的**
// 模块；example 是另一个 crate ⇒ 给**同名同形状**的最小实现。
#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod commands {
    use std::path::PathBuf;
    pub struct AppState {
        pub data_dir: PathBuf,
    }
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod harness {
    pub mod official_chat {
        pub use crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS;
        pub use crate::official_chat::OFFICIAL_CHAT_CHATGPT_URL;
        pub use crate::official_chat::OFFICIAL_CHAT_MINIMAX_URL;
        pub use crate::official_chat::OFFICIAL_CHAT_URL;
        // ⚠️ **macOS 的数据存储标识符**（生产取数窗要绑**同一份**才有登录态）。
        #[cfg(target_os = "macos")]
        pub(crate) use crate::official_chat::OFFICIAL_CHAT_DATA_STORE_IDENTIFIER;
    }
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/official_chat/cdp_eval.rs"]
mod cdp_eval;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "support/profile_copy.rs"]
mod profile_copy;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;
/// **生产那份取数模块**（页内脚本、站点契约、裁剪逻辑全在里面）。
// ⚠️ **不许再挂 `#[allow(dead_code)]`**：`session_structure.rs` **自带**内层
// `#![cfg_attr(not(windows), allow(dead_code))]` ⇒ ⇒ 两个一起挂会被 clippy 的
// `duplicated_attributes` 判红（2026-10-22 22:3x 实测）。
#[cfg(target_os = "macos")]
#[path = "../src/official_chat/session_structure.rs"]
mod session_structure;
