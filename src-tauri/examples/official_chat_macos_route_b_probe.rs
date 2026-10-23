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
///
/// ⚠️ **为什么是 420s**（2026-10-23 调过）：这一轮给探针加了 ChatGPT 那一格
/// （**目录 ＋ 正文 ＋ 反向对照**）⇒ ⇒ 一轮里多了**三次站点导航**（ChatGPT
/// 首页两次 ＋ 每次生产取数各建一扇窗）⇒ ⇒ 240s 不够用 ⇒ ⇒ 放宽到 420s。
#[cfg(target_os = "macos")]
const WATCHDOG_SECONDS: u64 = 420;

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

#[cfg(target_os = "macos")]
mod macos_probe {
    /// **探针只跑形态 B**（`PageRoute::Fetch`）—— ChatGPT／DeepSeek 的既有链。
    ///
    /// ⚠️ **为什么要显式 expect 而不是 `unwrap_or_default`**：`PageRoute` 现在是
    /// **枚举**（形态 B ＝ `Fetch`／形态 C ＝ `Hook`）⇒ ⇒ 访问器返回 `Option` ⇒ ⇒
    /// 探针若拿到 `Hook` 变体，**必须立刻报出来**（不静默套用别的契约）。
    const ONLY_FETCH: &str = "探针只跑形态 B（`PageRoute::Fetch`）——拿到别的变体就是契约变了";
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::{json, Value};
    use tauri::Manager;

    use crate::session_structure::PageRoute;

    /// 探针自己的数据存储标识符（**根级那个**——生产建窗链也要用它）。
    ///
    /// ⚠️ **为什么提到根级**：生产的 `build_capture_window` 从
    /// `crate::harness::official_chat::OFFICIAL_CHAT_DATA_STORE_IDENTIFIER` 取
    /// 标识符 ⇒ ⇒ 探针要把**生产的建窗链**也跑起来，就必须让那个位置**指向
    /// 探针自己的存储** ⇒ ⇒ 这一个常量是**唯一真相**（不许出现第二份字面量）。
    use crate::PROBE_DATA_STORE_IDENTIFIER;

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

    /// **正文那一条链**的**形状**探针（页内跑）：取 token → 取目录 → 取**第一个**
    /// 会话 id → 取正文 ⇒ ⇒ **只回形状**（角色、`content` 的键名集合、`parts`
    /// 的项类型与首项键名）—— **一个字的内容都不回**。
    ///
    /// ⚠️ **为什么要有这一格**：生产那条链把正文裁成「角色 ＋ 文本」⇒ ⇒ 裁剪
    /// 之后**看不出**「那些取不到文本的消息**长什么样**」⇒ ⇒ 要判断「是站点
    /// 那种消息本来就空」还是「**是我们少认了一种形状**」，**必须**去看原始
    /// 形状（**上游行为不能靠接口名推断**）。
    const BODY_SHAPE_SCRIPT: &str = r#"
(async function () {
  var out = { ok: true };
  try {
    var authRes = await fetch('/api/auth/session', { credentials: 'include' });
    var authData = await authRes.json();
    var token = (authData && typeof authData.accessToken === 'string') ? authData.accessToken : null;
    if (!token) { out.ok = false; out.stage = 'token'; out.error = '端点响应里没有 accessToken'; return out; }
    var headers = { authorization: 'Bearer ' + token };
    var catRes = await fetch('/backend-api/conversations', { credentials: 'include', headers: headers });
    var catJson = await catRes.json();
    var items = (catJson && Array.isArray(catJson.items)) ? catJson.items : [];
    out.catalogStatus = catRes.status;
    out.catalogCount = items.length;
    if (!items.length) { out.stage = 'catalog-empty'; return out; }
    var sid = items[0].id;
    var bodyRes = await fetch('/backend-api/conversations/' + encodeURIComponent(sid), { credentials: 'include', headers: headers });
    var bodyJson = await bodyRes.json();
    out.bodyStatus = bodyRes.status;
    var messages = (bodyJson && Array.isArray(bodyJson.messages)) ? bodyJson.messages : [];
    out.messagesCount = messages.length;
    var shapes = [];
    for (var i = 0; i < messages.length; i++) {
      var m = messages[i] || {};
      var author = m.author || {};
      var content = m.content;
      var shape = { role: author.role || null, contentType: typeof content };
      if (content && typeof content === 'object') {
        shape.contentKeys = Object.keys(content);
        var parts = content.parts;
        if (Array.isArray(parts)) {
          shape.partsLen = parts.length;
          var types = {};
          var lengths = [];
          for (var j = 0; j < parts.length; j++) {
            var t = typeof parts[j];
            types[t] = (types[t] || 0) + 1;
            // ⚠️ **只报长度**（不报内容）—— 与正文那条链报 `text_lengths` 同一口径。
            lengths.push(typeof parts[j] === 'string' ? parts[j].length : -1);
          }
          shape.partTypes = types;
          shape.partLengths = lengths;
          if (parts.length && parts[0] && typeof parts[0] === 'object') {
            shape.firstPartKeys = Object.keys(parts[0]);
          }
        }
      }
      shapes.push(shape);
    }
    out.shapes = shapes;
  } catch (e) {
    out.ok = false; out.stage = 'fetch'; out.error = String(e);
  }
  return out;
})()
"#;

    /// 在**刚建好的窗**里跑一段脚本（**重试到真的跑通为止**）。
    ///
    /// ⚠️ **为什么必须重试**（2026-10-22 实测 · wry-0.57.0
    /// `wkwebview/mod.rs:723-725`）：**首次导航 commit 之前**，`eval` 的脚本会被
    /// **静默排队**、**callback 一起丢掉** ⇒ ⇒ 回程通道的 sender 没了 ⇒ 报
    /// 「结果通道已关闭（webview 被销毁）」——**这条报错会骗人**（webview 好得
    /// 很，是脚本被排队了）⇒ ⇒ 生产那条链靠「等就绪」那一格绕开它（那正是
    /// 「先有一次求值真的跑通」的含义）⇒ ⇒ 探针这一格也得**先跑通一次**。
    async fn round_trip_until_it_runs(
        window: &tauri::WebviewWindow,
        script: &str,
        attempts: u32,
    ) -> Result<crate::script_eval::RoundTrip, crate::script_eval::RoundTripError> {
        let mut last: Option<crate::script_eval::RoundTripError> = None;
        for attempt in 0..attempts {
            match crate::script_eval::round_trip(window, script, Duration::from_secs(60)).await {
                Ok(trip) => return Ok(trip),
                Err(e) => {
                    log_line("retry", &format!("第 {} 次没跑通：{e}", attempt + 1));
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(700)).await;
                }
            }
        }
        Err(
            last.unwrap_or(crate::script_eval::RoundTripError::Transport {
                detail: "一次都没跑过".to_string(),
            }),
        )
    }

    /// 把**认证来源**写进读数（**只写契约，不写凭据的值**）。
    ///
    /// ⚠️ **为什么要有这个函数**：生产那张表把「凭据从哪儿来」收敛成了一个
    /// **枚举**（页内存储 ／ 同源端点）⇒ ⇒ 读数要照着那个枚举写 ⇒ ⇒ 两个站点
    /// 各写各的字段名会**漏掉另一半**。
    ///
    /// ⚠️ **只写契约**（存储名／键名／取法 ／ 端点路径／JSON 键名）⇒ ⇒
    /// **凭据的值一个字都不进读数**（纪律：只报长度与来源，不报值）。
    fn auth_reading(auth: &crate::session_structure::PageAuth) -> Value {
        use crate::session_structure::PageAuthSource;
        match auth.source {
            PageAuthSource::Storage {
                storage,
                key,
                extract,
            } => json!({
                "kind": "storage",
                "storage": storage.as_str(),
                "key": key,
                "extract": extract.as_str(),
            }),
            PageAuthSource::Endpoint { url, json_key } => json!({
                "kind": "endpoint",
                "endpointUrl": url,
                "jsonKey": json_key,
            }),
        }
    }

    /// 把一条**目录响应**写进读数（**只报条数与结构键名**）。
    ///
    /// ⚠️ **不报**会话标题、**不报**会话 id ⇒ ⇒ 只报状态码、长度、顶层键名、
    /// `items` 的条数（**结构**与**条数**，不是内容）。
    fn catalog_reading(captured: &crate::session_structure::CapturedResponse) -> Value {
        let parsed: Option<Value> = serde_json::from_str(&captured.body).ok();
        let top_keys = parsed
            .as_ref()
            .and_then(|value| value.as_object())
            .map(|map| map.keys().cloned().collect::<Vec<String>>())
            .unwrap_or_default();
        let items_count = parsed
            .as_ref()
            .and_then(|value| value.get("items"))
            .and_then(|value| value.as_array())
            .map(|items| items.len());
        json!({
            "status": captured.status,
            "length": captured.body.chars().count(),
            "top_keys": top_keys,
            "items_count": items_count,
            "note": "只报条数与结构键名（不报标题、不报 id）",
        })
    }

    /// 把一次**页内取数**的结果写进读数（**成功 ⇒ 条数与结构键名；失败 ⇒ 既有码
    /// 与原因**）。
    fn capture_reading(
        outcome: Result<
            crate::session_structure::CapturedResponse,
            crate::session_structure::CaptureFailure,
        >,
    ) -> Value {
        match outcome {
            Ok(captured) => catalog_reading(&captured),
            Err(error) => json!({
                "ok": false,
                "code": error.code(),
                "detail": error.detail(),
            }),
        }
    }

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
        let _ = store_reading;
        let store_dir_for_run = store_dir.clone();
        log_line("probe", "存储与规格表就绪");

        // ⚠️ **`app.run()` 必须在主线程、且不能放进 `block_on`**：Tauri 的事件
        // 循环要在主线程跑。取数在 `RunEvent::Ready` 之后起的**工作线程**上做。
        let app = tauri::Builder::default()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("probe: 无法构建探针应用");

        // ⚠️ **生产的 `capture_for` 要两样登记**（探针里照生产那样登记）：
        // ① `crate::commands::AppState`（`profile_dir` 从它取数据目录）——
        //    **必须给临时目录**：生产那个 `data_directory` 参数在 macOS 上被
        //    WKWebView **忽略**，但代码**照样 `create_dir_all`** ⇒ ⇒ 给了用户
        //    真实家目录就会在用户数据目录里**留一个探针目录**。
        // ② `bind_app`（拿不到句柄时 `capture_for` 直接报「壳还没把 app 句柄
        //    交给取数模块」⇒ ⇒ 那样就**验不到生产入口**了）。
        let probe_state_dir = std::env::temp_dir().join(format!(
            "dsh-official-chat-route-b-probe/state-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&probe_state_dir).expect("probe: 无法创建临时状态目录");
        app.manage(crate::commands::AppState {
            data_dir: probe_state_dir,
        });
        crate::session_structure::bind_app(app.handle());
        log_line(
            "probe",
            "已登记 AppState 与 app 句柄（生产的 capture_for 需要）",
        );

        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let url_for_run = url.clone();
        app.run(move |app, event| {
            if let tauri::RunEvent::Ready = event {
                if started.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let handle = app.clone();
                let url = url_for_run.clone();
                let store_dir = store_dir_for_run.clone();
                std::thread::Builder::new()
                    .name("official-chat-route-b-probe".into())
                    .spawn(move || {
                        log_line("probe", "工作线程启动");
                        let exit_handle = handle.clone();
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                tauri::async_runtime::block_on(async move {
                                    run_probe_body(&handle, &url, Some(route), &store_dir).await
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

    /// **窗口标签监视器**：在一段时间里**持续采样**壳里有哪些窗（按 label）。
    ///
    /// ## 它回答什么
    ///
    /// 「**生产的建窗链**真的跑过吗」——生产取数窗的 label 族是
    /// `official-chat-capture-*`（`session_structure::build_capture_window` 里
    /// 拼的）⇒ ⇒ 采到 ⇒ 跑过；**采不到** ⇒ 这一轮**没有建窗**。
    ///
    /// ## 为什么必须**采样**而不是「数一下建了几扇窗」
    ///
    /// 生产链**建完就关**（`ensure_started` 末尾 `window.close()`）⇒ ⇒ 调用
    /// 返回之后那扇窗**已经不在了** ⇒ ⇒ 只能在**调用期间**采样才问得到。
    ///
    /// ## ⚠️ 为什么读数是「**新增**的 label」而不是「采到的全部」
    ///
    /// r27 实测踩到过：生产链建的窗**只活十几毫秒**（没适配的站：建窗 → 立刻
    /// 早退 → 关窗）⇒ ⇒ 采样间隔 15ms 时，**上一格**那扇窗有可能落到**下一格**
    /// 的采样窗口里 ⇒ ⇒ 「这一格采到的标签」**不等于**「这一格建的窗」。
    ///
    /// ⇒ ⇒ 判据改成：**调用开始时已经在的** label ＝ 基线；只有**基线之后新增**
    /// 的才算这一格建的窗 ⇒ ⇒ 跨格串味不再发生。
    ///
    /// ## ⚠️ 即便如此，「标签为空」**也证明不了「没建窗」**
    ///
    /// r28 实测：没适配的站（S13／S14）那扇窗**只活十几毫秒**⇒ ⇒ 即便采样
    /// 加密到 2ms 也**可能抓不到** ⇒ ⇒ 「没抓到」**不等于**「没建」。
    ///
    /// ⇒ ⇒ **真正的判据是序号**：生产建窗链每建一扇窗就把 `CAPTURE_SEQ` 加一
    /// ⇒ ⇒ label 里的**序号**是连续的 ⇒ ⇒ **序号出现空洞** ⇒ ⇒ 中间那几扇
    /// **建过**（只是没被采样抓到）⇒ ⇒ 「这一格建没建窗」由**序号**说了算，
    /// 不由采样说了算。
    struct LabelMonitor {
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        baseline: Vec<String>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl LabelMonitor {
        /// 起监视（采样间隔 **2ms** —— 够密，抓得住那扇只活几秒的窗；也尽量
        /// 抓得住只活十几毫秒的那扇，但**不指望它** ⇒ 判据看序号）。
        fn start(app: tauri::AppHandle) -> Self {
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            // **基线**：起监视那一刻**已经在**的窗（探针自己那扇窗在里面）。
            let baseline: Vec<String> = app.webview_windows().keys().cloned().collect();
            let handle = {
                let stop = stop.clone();
                let seen = seen.clone();
                std::thread::Builder::new()
                    .name("route-b-label-monitor".into())
                    .spawn(move || {
                        while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                            let labels: Vec<String> =
                                app.webview_windows().keys().cloned().collect();
                            if let Ok(mut seen) = seen.lock() {
                                for label in labels {
                                    if !seen.iter().any(|item| item == &label) {
                                        seen.push(label);
                                    }
                                }
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                    })
                    .expect("probe: 无法起标签监视线程")
            };
            Self {
                stop,
                seen,
                baseline,
                handle: Some(handle),
            }
        }

        /// 停监视 ⇒ 交回**这一格新增的** label（基线里已有的不算）。
        fn stop(mut self) -> Vec<String> {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            let seen = self
                .seen
                .lock()
                .map(|seen| seen.clone())
                .unwrap_or_default();
            seen.into_iter()
                .filter(|label| !self.baseline.contains(label))
                .collect()
        }
    }

    /// 从采样结果里**只挑生产取数窗那族**（探针自己那扇窗不算）。
    fn production_capture_labels(labels: &[String]) -> Vec<String> {
        labels
            .iter()
            .filter(|label| label.starts_with("official-chat-capture-"))
            .cloned()
            .collect()
    }

    /// 探针那份 WK 存储里 **HTTP 缓存记录**的条数（**站点侧有没有发过请求**的
    /// 直接证据）。
    ///
    /// ## 为什么数它
    ///
    /// 「一个**没有路线 B 适配**的站，会不会产生站点侧副作用」——**读代码**能答
    /// （`capture_in` 在查适配那一步就早退 ⇒ **不导航** ⇒ 不发请求）⇒ ⇒ 但那是
    /// **推断** ⇒ ⇒ 要**实测**。
    ///
    /// ⇒ ⇒ **导航 ⇒ 站点响应 ⇒ WK 把响应写进 HTTP 缓存** ⇒ ⇒ 缓存记录条数
    /// **会变** ⇒ ⇒ 没变 ⇒ 那段时间**没有发过请求** ⇒ ⇒ 「没导航」从此有实测
    /// 支撑（而不是只有一句「代码上是这样的」）。
    fn network_record_count(store_dir: &Path) -> usize {
        fn count(dir: &Path) -> usize {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            entries
                .flatten()
                .map(|entry| {
                    let path = entry.path();
                    if path.is_dir() {
                        count(&path)
                    } else {
                        1
                    }
                })
                .sum()
        }
        count(&store_dir.join("NetworkCache"))
    }

    /// label 末尾那个**序号**（生产建窗链每建一扇窗加一 ⇒ ⇒ 序号连续 ⇒ ⇒
    /// **出现空洞就是「建过但没被采样抓到」**）。
    fn label_seq(label: &str) -> Option<u64> {
        label.rsplit('-').next().and_then(|part| part.parse().ok())
    }

    /// **S12–S17**：跑**生产入口**（插件真正调的那两个函数）。
    ///
    /// ## 为什么这一段与 S1–S11 不是一回事
    ///
    /// S1–S11 验的是「取数窗里那几行」（`capture_via_page` ＋ 裁剪）⇒ ⇒ 它
    /// **没有**验到：站点分派（`spec_for`）、缓存（保鲜期／快路径）、生产建窗
    /// 链（label／数据存储绑定）、以及**没有路线 B 适配的站点**那条明确报错。
    ///
    /// ⇒ ⇒ 这一段跑的是**桥上那条路由真正调的两个函数**：
    /// · 目录那条：`session_structure::capture_for(site, refresh)`；
    /// · 正文那条：`session_body::capture_for(site, session_id)`。
    ///
    /// ## 读数纪律
    ///
    /// **不报**：会话标题原文、会话 id、正文原文、任何凭据的值。
    /// **报**：状态码、业务码、条数、键名、角色、文本长度、错误文案。
    async fn run_production_entry_steps(
        app: &tauri::AppHandle,
        store_dir: &Path,
        report: &mut Value,
    ) {
        // **这一段跑下来生产建窗链一共建了几扇窗**（按 label 里的序号排）⇒ ⇒
        // 序号出现**空洞** ⇒ ⇒ 中间那几扇**建过**（只是太短、没被采样抓到）。
        let mut all_production_labels: Vec<String> = Vec::new();
        // ── S12：目录那条链的**生产入口**（`refresh=true` ⇒ 真取一轮）─────
        let monitor = LabelMonitor::start(app.clone());
        let started = std::time::Instant::now();
        let outcome = crate::session_structure::capture_for(SITE, true).await;
        let elapsed = started.elapsed().as_millis();
        let labels = monitor.stop();
        let production_labels = production_capture_labels(&labels);
        all_production_labels.extend(production_labels.clone());
        // 会话 id **只在内层用**（喂 S16 那条正文链）⇒ ⇒ **不进读数**。
        let mut first_session_id = String::new();
        match outcome {
            Ok(structure) => {
                first_session_id = structure
                    .to_json()
                    .get("items")
                    .and_then(|value| value.as_array())
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();
                report["s12_capture_for"] = json!({
                    "ok": true,
                    "code": structure.code,
                    "sessions_length": structure.sessions_length,
                    "items_count": structure.items.len(),
                    "has_more": structure.has_more,
                    "data_keys": structure.data_keys,
                    "first_item_keys": structure.first_item_keys,
                    "elapsed_ms": elapsed,
                    "production_window_labels": production_labels,
                    "note": "标题原文与会话 id 都不进读数；生产窗口标签非空 ⇒ 生产建窗链真的跑过",
                });
            }
            Err(error) => {
                report["s12_capture_for"] = json!({
                    "ok": false,
                    "code": error.code(),
                    "detail": error.detail(),
                    "elapsed_ms": elapsed,
                    "production_window_labels": production_labels,
                });
            }
        }

        // ── S13：MiniMax（**没有**路线 B 适配）⇒ 必须**明确报不支持** ─────
        //
        // ⚠️ MiniMax **不该**建窗、更**不该**导航（`capture_in` 在查适配那一步
        //    就早退）⇒ ⇒ 「有没有生产窗口标签」本身就是判据。
        // ⚠️ 「**站点侧零请求**」由**探针存储的 HTTP 缓存记录条数**前后对比来量
        //    （不是只读代码）。
        let net_before = network_record_count(store_dir);
        {
            let monitor = LabelMonitor::start(app.clone());
            let outcome =
                crate::session_structure::capture_for(crate::session_structure::MINIMAX_SITE, true)
                    .await;
            let labels = monitor.stop();
            let production_labels = production_capture_labels(&labels);
            all_production_labels.extend(production_labels.clone());
            report["s13_minimax"] = match outcome {
                Ok(structure) => json!({
                    "ok": true,
                    "unexpected": true,
                    "sessions_length": structure.sessions_length,
                    "production_window_labels": production_labels,
                }),
                Err(error) => json!({
                    "ok": false,
                    "code": error.code(),
                    "detail": error.detail(),
                    "production_window_labels": production_labels,
                    "note": "标签非空 ⇒ 这一格**建了窗**（并被采样抓到）；标签为空 ⇒ **可能**建了（那扇窗只活十几毫秒）⇒ 判据看末尾的序号分析（built_count_hint）",
                }),
            };
        }
        let net_after = network_record_count(store_dir);
        report["s13_no_network"] = json!({
            "http_cache_records_before": net_before,
            "http_cache_records_after": net_after,
            "delta": net_after as i64 - net_before as i64,
            "note": "delta = 0 ⇒ MiniMax 那一格**没有发过任何网络请求** ⇒ 「不导航」有实测支撑（不是只读代码）。⚠️ **ChatGPT 那一格（S14）会导航**（它**有**路线 B 适配）⇒ ⇒ 它**不**在这条判据里。",
        });

        // ── S14：ChatGPT（**有**路线 B 适配）⇒ 必须取到**真数据** ─────────
        //
        // ⚠️ **这一格是这一轮的新东西**（2026-10-23）：ChatGPT 那个站点**现在**
        // 有「页内取数」适配了（凭据来自**同源端点的响应** —— `GET
        // /api/auth/session` ⇒ 顶层 `accessToken`）⇒ ⇒ 这一格验的是**生产那条
        // 链**能不能在真机上取到**真的**会话目录与正文。
        //
        // ⚠️ **读数纪律**：**不报**会话标题原文、**不报**会话 id、**不报**任何
        // 凭据的值 ⇒ ⇒ 只报条数、结构键名、状态码、角色集合。
        {
            let monitor = LabelMonitor::start(app.clone());
            let started = std::time::Instant::now();
            let outcome =
                crate::session_structure::capture_for(crate::session_structure::CHATGPT_SITE, true)
                    .await;
            let elapsed = started.elapsed().as_millis();
            let labels = monitor.stop();
            let production_labels = production_capture_labels(&labels);
            all_production_labels.extend(production_labels.clone());
            // 会话 id **只在内层用**（喂正文那条链）⇒ ⇒ **不进读数**。
            let mut first_session_id = String::new();
            match outcome {
                Ok(structure) => {
                    first_session_id = structure
                        .to_json()
                        .get("items")
                        .and_then(|value| value.as_array())
                        .and_then(|items| items.first())
                        .and_then(|item| item.get("id"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();
                    report["s14_chatgpt_catalog"] = json!({
                        "ok": true,
                        "code": structure.code,
                        "sessions_length": structure.sessions_length,
                        "items_count": structure.items.len(),
                        "has_more": structure.has_more,
                        "data_keys": structure.data_keys,
                        "first_item_keys": structure.first_item_keys,
                        "elapsed_ms": elapsed,
                        "production_window_labels": production_labels,
                        "note": "标题原文与会话 id 都不进读数；sessions_length ＝ 站点给的原始条数，items_count ＝ 裁完的 6 条",
                    });
                }
                Err(error) => {
                    report["s14_chatgpt_catalog"] = json!({
                        "ok": false,
                        "code": error.code(),
                        "detail": error.detail(),
                        "elapsed_ms": elapsed,
                        "production_window_labels": production_labels,
                    });
                }
            }

            // ── S14b：**正文那条链**（生产入口，用上面那个会话 id）─────────
            if first_session_id.is_empty() {
                report["s14_chatgpt_body"] = json!({
                    "ok": false,
                    "detail": "目录那条没拿到会话 id ⇒ 正文那条没跑（不是正文链的读数）",
                });
            } else {
                let monitor = LabelMonitor::start(app.clone());
                let started = std::time::Instant::now();
                let outcome = crate::session_body::capture_for(
                    crate::session_structure::CHATGPT_SITE,
                    &first_session_id,
                )
                .await;
                let elapsed = started.elapsed().as_millis();
                let labels = monitor.stop();
                let production_labels = production_capture_labels(&labels);
                all_production_labels.extend(production_labels.clone());
                match outcome {
                    Ok(body) => {
                        // ⚠️ **角色集合**（去重后报）—— 不按条数展开。
                        let mut role_set: Vec<String> = body
                            .messages
                            .iter()
                            .map(|message| message.role.clone())
                            .collect();
                        role_set.sort();
                        role_set.dedup();
                        // ⚠️ **只报长度**，正文原文一个字都不进读数。
                        let text_lengths: Vec<usize> = body
                            .messages
                            .iter()
                            .map(|message| message.text.chars().count())
                            .collect();
                        report["s14_chatgpt_body"] = json!({
                            "ok": true,
                            "messages": body.messages.len(),
                            "roles": role_set,
                            "text_lengths": text_lengths,
                            "capture_result": body.capture_result,
                            "elapsed_ms": elapsed,
                            "production_window_labels": production_labels,
                            "note": "正文原文一个字都不进读数（只报条数、角色、长度）；capture_result 里只有省略事实的种类与条数",
                        });
                    }
                    Err(error) => {
                        report["s14_chatgpt_body"] = json!({
                            "ok": false,
                            "code": error.code(),
                            "detail": error.detail(),
                            "elapsed_ms": elapsed,
                            "production_window_labels": production_labels,
                        });
                    }
                }
            }
        }

        // ── S14c：**反向对照**（不带 Bearer ⇒ 必须取不到目录）─────────────
        //
        // ⚠️ **为什么必须有这一格**：S14 那一格「取到了目录」**不足以**证明
        // 「**是 Bearer 起的作用**」—— 同一份 cookie 本身也许就能读 ⇒ ⇒ 必须
        // 有一臂**只把认证头换掉**（同一个端点、同一个 cookie、**不带**
        // Bearer）⇒ ⇒ 那一臂必须**取不到目录**。
        //
        // ⚠️ **怎么做到「只换认证头」**：用**生产那份**取数链
        // （`capture_via_page` ＝ 生产真正用的那条），但把认证头的**名字**换成
        // 一个站点**不认**的头 ⇒ ⇒ 站点收不到 `authorization` ⇒ ⇒ 只剩
        // cookie ⇒ ⇒ 实测（2026-10-23 r8 的 `A-cookie-only` 那一臂）：200／
        // **0 条**。
        //
        // ⚠️ **两臂用同一扇窗**（同一份 cookie、同一个页面、同一份脚本）⇒ ⇒
        // 差别**只有**那个认证头 ⇒ ⇒ 这才是**对照**。
        {
            let chatgpt_route =
                crate::session_structure::spec_for(crate::session_structure::CHATGPT_SITE)
                    .and_then(|spec| spec.page_route);
            let Some(route) = chatgpt_route else {
                report["s14_chatgpt_control"] = json!({
                    "ok": false,
                    "detail": "规格表里 ChatGPT 没有路线 B 适配 ⇒ 这一格没跑",
                });
                return;
            };
            // ⚠️ **对照臂**：认证头的**名字**换掉（**值与前缀照旧**）⇒ ⇒ 站点
            // 收不到 `authorization` ⇒ ⇒ 这一臂只剩 cookie。
            let control_route = crate::session_structure::PageRoute::Fetch {
                url: route.fetch_url().expect(ONLY_FETCH),
                origin: route.fetch_origin().expect(ONLY_FETCH),
                auth: crate::session_structure::PageAuth {
                    source: route.fetch_auth().expect(ONLY_FETCH).source,
                    header: "x-dsh-probe-without-authorization",
                    prefix: route.fetch_auth().expect(ONLY_FETCH).prefix,
                },
                root_url: route.root_url(),
                body_url: route.fetch_body_url(),
            };
            let chatgpt_url = match url::Url::parse(route.root_url()) {
                Ok(url) => url,
                Err(e) => {
                    report["s14_chatgpt_control"] = json!({
                        "ok": false,
                        "detail": format!("ChatGPT 入口地址非法：{e}"),
                    });
                    return;
                }
            };
            let window = match build_window(
                app,
                "official-chat-route-b-probe-chatgpt",
                &chatgpt_url,
                &PROBE_DATA_STORE_IDENTIFIER,
                false,
            ) {
                Ok(window) => window,
                Err(e) => {
                    report["s14_chatgpt_control"] = json!({
                        "ok": false,
                        "detail": format!("建 ChatGPT 那一扇窗失败：{e}"),
                    });
                    return;
                }
            };
            // ① **正臂**：生产那份链，原样的认证头 ⇒ ⇒ 必须取到目录。
            let positive = capture_reading(
                crate::session_structure::capture_via_page(
                    &window,
                    &route,
                    route.fetch_url().expect(ONLY_FETCH),
                )
                .await,
            );
            // ② **对照臂**：只把认证头的**名字**换掉 ⇒ ⇒ 必须取不到目录。
            let control = capture_reading(
                crate::session_structure::capture_via_page(
                    &window,
                    &control_route,
                    route.fetch_url().expect(ONLY_FETCH),
                )
                .await,
            );
            let _ = window.close();
            report["s14_chatgpt_control"] = json!({
                "positive": positive,
                "control": control,
                "note": "两臂用同一扇窗、同一份 cookie、同一段脚本 ⇒ 差别**只有**认证头的名字。对照臂的 headers 换成了站点不认的头 ⇒ 站点只收到 cookie。⚠️ 只报条数与结构键名（不报标题、不报 id）。",
            });
        }

        // ── S14d：**正文那一条链的形状**（页内读原始形状）──────────────────
        //
        // ⚠️ **这一格问什么**：S14b 那格把正文裁成「角色 ＋ 文本」⇒ ⇒ 裁剪之后
        // **看不出**「那些**取不到文本**的消息长什么样」⇒ ⇒ 要判断「是站点那种
        // 消息本来就空」还是「**是我们少认了一种形状**」，**必须**去看原始形状。
        //
        // ⚠️ **只回形状**：角色、`content` 的键名集合、`parts` 的项类型、首项键名
        // ⇒ ⇒ **一个字的内容都不回**（正文原文不进读数）。
        {
            let chatgpt_url = url::Url::parse("https://chatgpt.com").expect("ChatGPT 入口地址");
            let window = match build_window(
                app,
                "official-chat-route-b-probe-chatgpt-shape",
                &chatgpt_url,
                &PROBE_DATA_STORE_IDENTIFIER,
                false,
            ) {
                Ok(window) => window,
                Err(e) => {
                    report["s14_chatgpt_shape"] = json!({
                        "ok": false,
                        "detail": format!("建 ChatGPT 那一扇窗失败：{e}"),
                    });
                    return;
                }
            };
            let script = BODY_SHAPE_SCRIPT;
            let outcome = round_trip_until_it_runs(&window, script, 12).await;
            let _ = window.close();
            report["s14_chatgpt_shape"] = match outcome {
                Ok(trip) => {
                    let value = &trip.value;
                    json!({
                        "ok": value.get("ok").and_then(Value::as_bool).unwrap_or(false),
                        "stage": value.get("stage").and_then(Value::as_str).unwrap_or(""),
                        "error": value.get("error").and_then(Value::as_str).unwrap_or(""),
                        "catalog_status": value.get("catalogStatus").and_then(Value::as_i64),
                        "catalog_count": value.get("catalogCount").and_then(Value::as_i64),
                        "body_status": value.get("bodyStatus").and_then(Value::as_i64),
                        "messages_count": value.get("messagesCount").and_then(Value::as_i64),
                        "shapes": value.get("shapes").cloned().unwrap_or(Value::Null),
                        "note": "只回形状（角色／content 键名／parts 项类型／首项键名）⇒ 正文原文一个字都不回",
                    })
                }
                Err(e) => json!({ "ok": false, "error": format!("{e:?}") }),
            };
        }

        // ── S15：**快路径**（`refresh=false`）⇒ 不该再建窗 ───────────────
        //
        // 生产 `capture_for` 的快路径：保鲜期（60s）内直接回缓存 ⇒ ⇒ 这一格
        // 验的是「**缓存命中时不会再建一扇窗**」（插件每个按键都调一次 `catalog`
        // ⇒ ⇒ 不守这一条就会每按一个键建一扇窗）。
        let monitor = LabelMonitor::start(app.clone());
        let started = std::time::Instant::now();
        let outcome = crate::session_structure::capture_for(SITE, false).await;
        let elapsed = started.elapsed().as_millis();
        let labels = monitor.stop();
        let production_labels = production_capture_labels(&labels);
        all_production_labels.extend(production_labels.clone());
        report["s15_fast_path"] = match outcome {
            Ok(structure) => json!({
                "ok": true,
                "sessions_length": structure.sessions_length,
                "items_count": structure.items.len(),
                "elapsed_ms": elapsed,
                "production_window_labels": production_labels,
                "note": "这一格**新增**的生产窗口标签为空 ⇒ 快路径**没有建窗**（直接回缓存）",
            }),
            Err(error) => json!({
                "ok": false,
                "code": error.code(),
                "detail": error.detail(),
                "elapsed_ms": elapsed,
                "production_window_labels": production_labels,
            }),
        };

        // ── S16：正文那条链的**生产入口**（用 S12 拿到的那个会话 id）─────
        if first_session_id.is_empty() {
            report["s16_body_capture_for"] = json!({
                "ok": false,
                "detail": "目录那条没拿到会话 id ⇒ 正文那条没跑（不是正文链的读数）",
            });
        } else {
            let monitor = LabelMonitor::start(app.clone());
            let started = std::time::Instant::now();
            let outcome = crate::session_body::capture_for(SITE, &first_session_id).await;
            let elapsed = started.elapsed().as_millis();
            let labels = monitor.stop();
            let production_labels = production_capture_labels(&labels);
            all_production_labels.extend(production_labels.clone());
            match outcome {
                Ok(body) => {
                    let roles: Vec<String> = body
                        .messages
                        .iter()
                        .map(|message| message.role.clone())
                        .collect();
                    // **只报长度**，正文原文一个字都不进读数。
                    let text_lengths: Vec<usize> = body
                        .messages
                        .iter()
                        .map(|message| message.text.chars().count())
                        .collect();
                    report["s16_body_capture_for"] = json!({
                        "ok": true,
                        "messages": body.messages.len(),
                        "roles": roles,
                        "text_lengths": text_lengths,
                        "capture_result": body.capture_result,
                        "pick_keys": body
                            .pick
                            .as_object()
                            .map(|map| map.keys().cloned().collect::<Vec<String>>())
                            .unwrap_or_default(),
                        "elapsed_ms": elapsed,
                        "production_window_labels": production_labels,
                        "note": "正文原文不进读数（只报条数、角色、长度）",
                    });
                }
                Err(error) => {
                    report["s16_body_capture_for"] = json!({
                        "ok": false,
                        "code": error.code(),
                        "detail": error.detail(),
                        "elapsed_ms": elapsed,
                        "production_window_labels": production_labels,
                    });
                }
            }
        }

        // ── S17：正文那条链在**没有正文适配**的站点上 ⇒ 明确报不支持 ─────
        let outcome = crate::session_body::capture_for(
            crate::session_structure::MINIMAX_SITE,
            "probe-not-a-real-session",
        )
        .await;
        report["s17_body_unsupported"] = match outcome {
            Ok(body) => json!({
                "ok": true,
                "unexpected": true,
                "messages": body.messages.len(),
            }),
            Err(error) => json!({
                "ok": false,
                "code": error.code(),
                "detail": error.detail(),
            }),
        };

        // ── 收尾：生产建窗链的**序号分析**（这一段一共建了几扇窗）────────
        //
        // 序号**空洞** ＝ 中间那几扇**建过**、但太短、没被采样抓到 ⇒ ⇒ 「这一格
        // 建没建窗」由**序号**说了算（不靠采样）。
        let mut seqs: Vec<u64> = all_production_labels
            .iter()
            .filter_map(|label| label_seq(label))
            .collect();
        seqs.sort_unstable();
        seqs.dedup();
        let max_seq = seqs.iter().copied().max();
        report["production_window_sequence"] = json!({
            "labels": all_production_labels,
            "seqs_seen": seqs,
            "max_seq": max_seq,
            "built_count_hint": max_seq.map(|max| max + 1),
            "note": "生产建窗链每建一扇窗序号加一 ⇒ ⇒ built_count_hint ＝ 这一段**建过**的窗数（含没被采样抓到的）",
        });
    }

    /// 实测主体（在 Tauri 事件循环起来之后）。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &url::Url,
        route: Option<PageRoute>,
        store_dir: &Path,
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
            "url": route.fetch_url().expect(ONLY_FETCH),
            "origin": route.fetch_origin().expect(ONLY_FETCH),
            "auth": auth_reading(route.fetch_auth().expect(ONLY_FETCH)),
            "header": route.fetch_auth().expect(ONLY_FETCH).header,
            "prefix": route.fetch_auth().expect(ONLY_FETCH).prefix,
            "body_url": route.fetch_body_url(),
            "note": "只写契约（来源／键名／端点路径）⇒ 凭据的值一个字都不进读数",
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
            // ⚠️ **用生产那份脚本构造器**（不是探针自己拼规格）⇒ ⇒ 这一格
            // 验的是**生产真正注入的那段脚本**。
            let script = crate::session_structure::page_fetch_script(
                &route,
                route.fetch_url().expect(ONLY_FETCH),
            );
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
            let captured = crate::session_structure::capture_via_page(
                &window,
                &route,
                route.fetch_url().expect(ONLY_FETCH),
            )
            .await;
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
            // ⚠️ **这段脚本是 DeepSeek 形态专用**（页内存储取凭据）⇒ ⇒ 键名从
            // 「存储那一支」取；**取不到就给空串**（脚本自己会报「页内没有凭据」
            // ⇒ ⇒ **不静默换契约**）。
            let storage_key = match route.fetch_auth().expect(ONLY_FETCH).source {
                crate::session_structure::PageAuthSource::Storage { key, .. } => key.to_string(),
                crate::session_structure::PageAuthSource::Endpoint { .. } => String::new(),
            };
            let spec = json!({
                "key": storage_key,
                "header": route.fetch_auth().expect(ONLY_FETCH).header,
                "prefix": route.fetch_auth().expect(ONLY_FETCH).prefix,
                "catalogUrl": route.fetch_url().expect(ONLY_FETCH),
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
            let body_template = route.fetch_body_url();
            // 取一个真实的会话 id（**只在内层用**，不进读数）。
            let session_id = match crate::session_structure::capture_via_page(
                &window,
                &route,
                route.fetch_url().expect(ONLY_FETCH),
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
                    let captured = crate::session_structure::capture_via_page(
                        &window_b,
                        &route,
                        route.fetch_url().expect(ONLY_FETCH),
                    )
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

        // ── S12–S17：**生产入口**那两条链（插件真正调的那两个函数）────────
        //
        // ⚠️ **放在最后**：这几格要**建自己的窗**（生产建窗链）⇒ ⇒ 放最后
        // 才不会让前面那些格的窗标签混进来。
        run_production_entry_steps(app, store_dir, &mut report).await;

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

/// 探针自己的数据存储标识符（**不是**生产的任何一个）。
///
/// ## 为什么它必须在**根级**
///
/// 生产的建窗链（`session_structure::build_capture_window`）从
/// `crate::harness::official_chat::OFFICIAL_CHAT_DATA_STORE_IDENTIFIER` 取标识符
/// ⇒ ⇒ 探针要把**生产那份建窗链**也跑起来（S12 那格），就必须让那个位置
/// **指向探针自己的存储** ⇒ ⇒ 这一个常量是**唯一真相**（不许有第二份字面量）。
///
/// ⚠️ **纪律**：探针**绝不能**起在用户真实那份档案上（浏览器一启动就写
/// Cookies／LocalStorage）⇒ ⇒ 生产那两个标识符（`dsh-chat-rel-001` ／
/// `dsh-chat-dev-001`）在探针里**一个都不许出现**。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-rbp-001";

#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod harness {
    pub mod official_chat {
        pub use crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS;
        pub use crate::official_chat::OFFICIAL_CHAT_CHATGPT_URL;
        pub use crate::official_chat::OFFICIAL_CHAT_MINIMAX_URL;
        pub use crate::official_chat::OFFICIAL_CHAT_URL;
        // ⚠️ **这里刻意**不转出生产那个标识符（`OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`）
        // ⇒ ⇒ **换成探针自己那份** ⇒ ⇒ 生产的建窗链在探针里也**不会**起在用户
        // 真实档案上（纪律：探针不许碰用户真实浏览器 profile）。
        pub(crate) use crate::PROBE_DATA_STORE_IDENTIFIER as OFFICIAL_CHAT_DATA_STORE_IDENTIFIER;
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
/// **生产那份正文链**（协议 §4.4 的入口 `session_body::capture_for`）。
///
/// ## 为什么也要编进来
///
/// 目录那条链的生产入口是 `session_structure::capture_for`，正文那条链的生产
/// 入口是 `session_body::capture_for` ⇒ ⇒ **两条链各有一个入口** ⇒ ⇒ 探针要
/// 验的是「**插件真正调的那两个函数**」，不是「取数窗里那几行」。
///
/// ## 平台差异
///
/// `session_body.rs` 里的**冷档案**那段整块是 `#[cfg(windows)]` ⇒ ⇒ macOS 上
/// 这份文件**不含**任何 Windows 专用依赖 ⇒ ⇒ 可以直接编进探针。
///
/// ⚠️ **同样不许挂 `#[allow(dead_code)]`**（它也自带内层
/// `#![cfg_attr(not(windows), allow(dead_code))]` ⇒ 两个一起挂会被 clippy 的
/// `duplicated_attributes` 判红）。
#[cfg(target_os = "macos")]
#[path = "../src/official_chat/session_body.rs"]
mod session_body;
/// **生产那份取数模块**（页内脚本、站点契约、裁剪逻辑全在里面）。
// ⚠️ **不许再挂 `#[allow(dead_code)]`**：`session_structure.rs` **自带**内层
// `#![cfg_attr(not(windows), allow(dead_code))]` ⇒ ⇒ 两个一起挂会被 clippy 的
// `duplicated_attributes` 判红（2026-10-22 22:3x 实测）。
#[cfg(target_os = "macos")]
#[path = "../src/official_chat/session_structure.rs"]
mod session_structure;
