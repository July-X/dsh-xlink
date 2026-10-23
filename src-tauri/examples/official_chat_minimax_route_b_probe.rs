//! MiniMax「**页内挂钩（页内旁听）**」形态的**端到端探针**（macOS 真机，只读测量）。
//!
//! # 它验的是**生产那份**页内脚本，不是探针自己的副本
//!
//! 页内脚本是**生产文件**（`src/official_chat/page_hook_script.js`）⇒ ⇒ 本探针
//! 用 `include_str!` 把它**原样**引进来（只替换 `__SPEC__` 规格占位）⇒ ⇒ 验的
//! 就是生产那份（与 `official_chat_macos_route_b_probe.rs` 验 `capture_via_page`
//! 的做法同构）。
//!
//! # 这一轮验的五条断言
//!
//! | 编号 | 断言 | 判据 |
//! | --- | --- | --- |
//! | ① | **目录截到** | `capture.summary.sessionsCount ≥ 1`；顶层键名含 `sessions`／`base_resp`／`has_more`／`next_cursor`；条目键名含 `session_id`／`title`／`updated_at` |
//! | ② | **还原后残留为零** | `restore.fetchHasOurSource == false`（`fetch.toString()` 里不含我们的源码）＋ `restore.newXhrHookMarks` **空**（新建 XHR 实例无 `__mmx*` 标记）＋ `restore.windowOwnCountDelta == 0`（还原前后 window 自有属性数差 0） |
//! | ③ | **正文重放** | `body.summary.status == 200`；`messagesCount ≥ 1`；顶层键名含 `messages`；消息键名含 `msg_id`／`role`／`msg_content`；角色取值集合 ⊆ {`user`,`assistant`} |
//! | ④ | **反向对照** | `controls.unsigned.summary.status == 400`（摘掉 `x-signature`／`x-timestamp`／`yy` ⇒ 签名被拒 ⇒ 证明读到的不是空壳） |
//! | ⑤ | **阳性对照** | `controls.positive.summary.status == 200`（**还原之前**发的重放 ⇒ 截到的签名头确实可用） |
//!
//! # 隔离（判据 `probe-profile-isolation` / `probe-data-directory`）
//!
//! · 源是用户真实那份 WK 存储 ⇒ ⇒ **只读拷贝**到探针自己的目录（探针写的全在
//!   拷贝里）⇒ ⇒ 跑前跑后核**源 cookie 文件的 sha256 相同**。
//! · 建 webview **无条件**给 `data_directory`（判据按**代码形状**判，「有才给」
//!   同样违规）；`data_store_identifier` **正好 16 字节**。
//! · **不许关掉最后一扇窗**（tauri v2 里关掉最后一扇窗 ⇒ 事件循环结束 ⇒ 进程直接
//!   退出 ⇒ 报告来不及打印）。
//!
//! # 读数纪律
//!
//! **不报**：会话标题原文、会话 id、任何凭据的值（签名／token／cookie）。
//! **报**：状态码、业务码、结构键名、条数、字节数、凭据**长度**、耗时。
//!
//! # 证据
//!
//! `/tmp/dsh-minimax-route-b-probe.<round>.log` ＋ `/tmp/dsh-minimax-route-b-probe.<round>.json`
//! （`DSH_PROBE_ROUND` 决定轮次 ⇒ ⇒ **按轮次独立命名**，不覆盖上一轮的原件）。
//!
//! 这条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块编译掉。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe: 本探针只支持 macOS（WKWebsiteDataStore 路线）。");
    std::process::exit(2);
}

/// **看门狗**：探针卡住时**自己**退出，不占着整轮等待。
///
/// ⚠️ 一轮最长 ≈「等 commit」＋「等站点那层」（45 s）＋「等目录那一发」（45 s）
/// ⇒ ⇒ 看门狗给到 15 分钟。
#[cfg(target_os = "macos")]
const WATCHDOG_SECONDS: u64 = 1200;

/// 探针自己的数据存储标识符（**正好 16 字节** —— 判据硬要求）。
///
/// ⚠️ **纪律**：探针**绝不能**起在用户真实那份档案上（浏览器一启动就写 Cookies／
/// LocalStorage）⇒ ⇒ 生产那两个标识符在探针里**一个都不许出现**。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-mrb-001";

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::{json, Value};

    /// 探针自己的数据存储标识符（根级那个）。
    use crate::PROBE_DATA_STORE_IDENTIFIER;

    /// **源存储目录**从哪个环境变量读（源码里不写路径字面量）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// MiniMax 的入口地址（会 301 到 `agent.minimax.cn`）。
    const SITE_URL: &str = "https://agent.minimaxi.com";

    /// 目录端点（规格表里那个 `catalog_path`）。
    const CATALOG_PATH: &str = "/minimax-cloud/api/v1/sidebar/session/tree";

    /// 正文端点的**路径形状**（`session_id` 留在页内 ⇒ 宿主拿不到真值）。
    const MESSAGE_PATH_TEMPLATE: &str = "/minimax-cloud/api/v1/session/{session}/message";

    /// 正文那一发的 query（**实测形态**；`limit` 是目录那一发自己的分页参数，
    /// 已在脚本里剔掉）。
    const MESSAGE_QUERY: &str = "limit=80&_source=session-switch";

    /// 签名三件套（**头名**；值一步都不出页面）。
    const SIGNATURE_HEADERS: [&str; 3] = ["x-signature", "x-timestamp", "yy"];

    /// 页内脚本的**等待预算**（等站点那层／等目录那一发各一份）。
    const POLL_BUDGET_MS: u64 = 45_000;

    /// 页内那一发的**回程超时**（`round_trip`）。
    const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(180);

    /// 每一步都落一行（**文件名按轮次独立命名** ⇒ 不覆盖上一轮的原始证据）。
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
    ///
    /// ⚠️ **显式落 `/tmp`**：`std::env::temp_dir()` 在 macOS 上是 `$TMPDIR`
    /// （`/var/folders/…`）⇒ 证据会散在别处 ⇒ ⇒ 显式写 `/tmp`。
    fn log_path() -> PathBuf {
        let round = std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string());
        PathBuf::from("/tmp").join(format!("dsh-minimax-route-b-probe.{round}.log"))
    }

    /// 报告（JSON）的路径（**轮次独立命名**）。
    fn report_path() -> PathBuf {
        let round = std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string());
        PathBuf::from("/tmp").join(format!("dsh-minimax-route-b-probe.{round}.json"))
    }

    /// 打完报告就退出。
    fn finish(report: &str) {
        println!("PROBE_REPORT_BEGIN");
        println!("{report}");
        println!("PROBE_REPORT_END");
        log_line("report", report);
        let path = report_path();
        let _ = std::fs::write(&path, report);
        log_line("report", &format!("报告落盘：{}", path.display()));
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

    /// 探针自己的 WK 存储目录（**用自身 exe 文件名** ⇒ 天然隔离）。
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
                "copied": report.files,
                "bytes": report.bytes,
                "skipped": report.skipped.len(),
                "probe_store_dir": target.display().to_string(),
                "note": "源是**只读拷贝**：探针写的全在拷贝里，用户正在跑的那份存储一个字节都没改",
            }),
        ))
    }

    /// 源存储里 cookie 文件的 sha256（**只读证明**：跑前跑后必须一样）。
    fn source_cookie_sha256() -> Option<String> {
        use sha2::{Digest, Sha256};
        let source = std::env::var(ENV_SOURCE).ok()?;
        let cookie = Path::new(&source)
            .join("Cookies")
            .join("Cookies.binarycookies");
        let bytes = std::fs::read(&cookie).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Some(format!("{:x}", hasher.finalize()))
    }

    /// **这一轮要不要起隐藏窗**（`DSH_PROBE_HIDDEN=1` ⇒ 隐藏）。
    fn hidden_window() -> bool {
        std::env::var("DSH_PROBE_HIDDEN")
            .map(|value| value == "1")
            .unwrap_or(false)
    }

    /// **生产那份**页内脚本（只替换 `__SPEC__` 规格占位）。
    ///
    /// ⚠️ **为什么不自己抄一份**：抄一份就变成「验的是探针自己的副本」⇒ ⇒ 这里
    ///    直接 `include_str!` 生产那份文件。
    fn hook_script() -> String {
        let spec = json!({
            "catalog_path": CATALOG_PATH,
            "signature_headers": SIGNATURE_HEADERS,
            "sessions_path": ["sessions"],
            "item_path": ["session"],
            "id_path": ["session_id"],
            "body_path_template": MESSAGE_PATH_TEMPLATE,
            "body_query": MESSAGE_QUERY,
            "item_fields": {
                "title": ["title"],
                "title_type": [],
                "model_type": ["model", "model_id"],
                "pinned": [],
                "updated_at": ["updated_at"]
            },
            "poll_budget_ms": POLL_BUDGET_MS,
            "max_poll_attempts": 400000,
            "replay_body": true,
            "positive_control": true
        });
        let spec = serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string());
        include_str!("../src/official_chat/page_hook_script.js").replace("__SPEC__", &spec)
    }

    /// 建窗（**探针自己的**存储标识符 ⇒ **不碰**用户真实档案）。
    fn build_window(
        app: &tauri::AppHandle,
        label: &str,
        url: &url::Url,
        identifier: &[u8; 16],
    ) -> Result<tauri::WebviewWindow, String> {
        // **无条件**给 `data_directory`（判据 `probe-data-directory` 按**代码形状**
        // 判，「有才给」同样违规）。macOS 上 WKWebView **忽略**它（它认的是
        // `data_store_identifier`），但照样给——口径一致。
        let profile = std::env::temp_dir().join("dsh-official-chat-minimax-route-b-probe");
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("无法创建临时 profile 目录（{}）：{e}", profile.display()))?;

        let builder =
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(url.clone()))
                .title("dsh MiniMax 页内挂钩（形态 C）探针")
                .inner_size(1000.0, 760.0)
                .visible(!hidden_window())
                .data_directory(profile)
                .additional_browser_args(crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
                .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
                .data_store_identifier(*identifier);

        log_line("probe", &format!("准备 build()（label={label}）"));
        let built = builder.build();
        log_line("probe", "build() 返回");
        built.map_err(|e| format!("无法创建探针窗口：{e}"))
    }

    /// 等**首次导航 commit**。
    ///
    /// ⚠️ **为什么必须先等这一格**：wry 在 macOS 上把 commit **之前**的求值
    ///    **排队**并**丢掉回调** ⇒ ⇒ 那时候求值会报「结果通道已关闭（webview 被
    ///    销毁）」——**那条报错会骗人**（webview 好好的，是脚本被排队了）。
    async fn wait_commit(window: &tauri::WebviewWindow) -> Result<Value, String> {
        let mut last = String::new();
        for _ in 0..160 {
            // ⚠️ **读不到就继续等**（commit 之前求值会被排队并丢掉回调 ⇒ ⇒ 那
            //    时候读回必然失败）⇒ ⇒ 这一格是**条件等待**，不是固定睡眠。
            if let Ok(value) = crate::script_eval::read_json(
                window,
                "String(location.href)",
                Duration::from_secs(2),
            )
            .await
            {
                let text = value.as_str().unwrap_or("").to_string();
                if !text.is_empty() {
                    return Ok(json!({ "href_present": true, "href": text }));
                }
                last = text;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(format!("首次导航没有 commit（最后读到的 href：{last}）"))
    }

    // ---- 读数工具 -----------------------------------------------------------

    fn num(value: &Value, path: &[&str]) -> Option<f64> {
        let mut cur = value;
        for key in path {
            cur = cur.get(*key)?;
        }
        cur.as_f64()
    }

    fn text(value: &Value, path: &[&str]) -> Option<String> {
        let mut cur = value;
        for key in path {
            cur = cur.get(*key)?;
        }
        cur.as_str().map(|s| s.to_string())
    }

    fn strings(value: &Value, path: &[&str]) -> Vec<String> {
        let mut cur = value;
        for key in path {
            match cur.get(*key) {
                Some(next) => cur = next,
                None => return Vec::new(),
            }
        }
        cur.as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn bool_of(value: &Value, path: &[&str]) -> Option<bool> {
        let mut cur = value;
        for key in path {
            cur = cur.get(*key)?;
        }
        cur.as_bool()
    }

    /// 目录**首条目**里某个数值字段（**时间戳**这一格：规格表说它是**毫秒**
    /// ⇒ ⇒ 读出来才能核 `updated_at_divisor`）。
    ///
    /// ⚠️ **只取数值字段**（时间戳）—— 会话 id／标题原文**一个字都不碰**。
    fn first_item_number(value: &Value, key: &str) -> Option<f64> {
        value
            .get("items")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get(key))
            .and_then(Value::as_f64)
    }

    /// 断言：**顶层键名**必须含这些（缺一个就红）。
    fn has_all(keys: &[String], required: &[&str]) -> (bool, Vec<String>) {
        let missing: Vec<String> = required
            .iter()
            .filter(|key| !keys.iter().any(|k| k == *key))
            .map(|key| key.to_string())
            .collect();
        (missing.is_empty(), missing)
    }

    /// **本机可用的「让出一次宏任务」原语**（**诊断读数**）。
    ///
    /// ⚠️ **为什么要有这一格**：页内脚本等「站点自己那层出现」要**让出**一次
    ///    宏任务 ⇒ ⇒ 用哪个原语**直接影响宿主通道会不会被饿死**（r7 实测：
    ///    `MessageChannel` 连续投递 ⇒ ⇒ 宿主的求值在 2 s 内**没有回应** ⇒ ⇒
    ///    那一轮整轮失败）⇒ ⇒ 所以这里把**本机到底有什么**量出来。
    const CAPABILITIES_EXPR: &str = r#"(function () {
      var out = {};
      try { out.scheduler = typeof scheduler; } catch (e) { out.scheduler = 'undefined'; }
      try { out.schedulerYield = typeof scheduler.yield; } catch (e) { out.schedulerYield = 'undefined'; }
      try { out.schedulerPostTask = typeof scheduler.postTask; } catch (e) { out.schedulerPostTask = 'undefined'; }
      try { out.requestIdleCallback = typeof requestIdleCallback; } catch (e) { out.requestIdleCallback = 'undefined'; }
      try { out.messageChannel = typeof MessageChannel; } catch (e) { out.messageChannel = 'undefined'; }
      try { out.requestAnimationFrame = typeof requestAnimationFrame; } catch (e) { out.requestAnimationFrame = 'undefined'; }
      try { out.visibilityState = String(document.visibilityState); } catch (e) { out.visibilityState = 'unknown'; }
      try { out.userAgent = String(navigator.userAgent); } catch (e) { out.userAgent = 'unknown'; }
      return out;
    })()"#;

    /// **独立**的残留读数（**不信任脚本的自报**）。
    ///
    /// ⚠️ **为什么要有这一格**：断言②如果只读脚本自己报回来的 `restore` 那几格
    ///    ⇒ ⇒ 一份**改坏了的**脚本完全可以自己报「残留为零」⇒ ⇒ 断言就成摆设
    ///    ⇒ ⇒ 这里由**探针自己**在页内再量一遍：
    ///    ① `window.fetch` 的源码里**含不含**我们的标记串；
    ///    ② **新建**一个 XHR 实例 ⇒ 它身上**带不带** `__mmx*` 标记（原生
    ///    `open()` 只在**原型钩子还在**时才会打这些标记 ⇒ ⇒ 这就是「我们那层
    ///    还在不在链上」的探针）；
    ///    ③ `window` 自有属性里带 `__dsh`／`__mmx` 前缀的名字。
    ///
    /// ⚠️ **只 `open()` 不 `send()`** ⇒ ⇒ **零网络请求**。
    const RESIDUE_EXPR: &str = r#"(function () {
      var out = {};
      try {
        var src = Function.prototype.toString.call(window.fetch);
        out.fetchSrcLength = src.length;
        out.fetchHasOurSource = src.indexOf('dsh-page-hook-marker') >= 0;
        out.fetchIsNative = src.indexOf('[native code]') >= 0;
      } catch (e) { out.fetchError = String(e).slice(0, 120); }
      try {
        var names = Object.getOwnPropertyNames(window);
        out.windowOwnCount = names.length;
        var marks = [];
        for (var i = 0; i < names.length; i++) {
          var n = String(names[i]);
          if (n.indexOf('__dsh') === 0 || n.indexOf('__mmx') === 0) marks.push(n);
        }
        out.probeOwnNames = marks;
      } catch (e2) {}
      try {
        var xhr = new XMLHttpRequest();
        xhr.open('GET', String(location.href));
        var xn = Object.getOwnPropertyNames(xhr);
        var xm = [];
        for (var j = 0; j < xn.length; j++) {
          if (String(xn[j]).indexOf('__mmx') === 0) xm.push(String(xn[j]));
        }
        out.newXhrHookMarks = xm;
      } catch (e3) { out.xhrError = String(e3).slice(0, 120); }
      return out;
    })()"#;

    /// **容错**回程：**注入一次** ＋ 轮询取回（**读回失败就再读**，**不重新注入**）。
    ///
    /// ⚠️ **为什么不直接用 `script_eval::round_trip`**：它**首次读回失败即放弃**
    ///    （`Err(Gone)`）⇒ ⇒ 页面正忙时（站点 bundle 正在解析／本机负载高）那一发
    ///    求值可能 2 s 内没有回应 ⇒ ⇒ **整轮失败**（r7/r8 实测：两次都是
    ///    「求值在 2000ms 内没有回应」）⇒ ⇒ 这里改成「读回失败就再读一次，直到
    ///    总超时」。
    ///
    /// ⚠️ **为什么「读回失败重试」而不是「整轮重试」**：重试整轮＝**重新注入**
    ///    ⇒ ⇒ 页内会跑**两个实例** ⇒ ⇒ 两个实例各装一层 ⇒ ⇒ **还原链就错了**
    ///    （后一个还原成前一个的包装 ⇒ ⇒ 残留）⇒ ⇒ **注入必须只发生一次**。
    ///
    /// ⚠️ **槽位协议与 `script_eval::round_trip` 同形**（随机名、不可枚举、
    ///    取回即删）⇒ ⇒ **那是宿主自己的槽位**（形态 B 的已知缺口 G4-29），
    ///    **不是页内脚本的**（脚本自己一个槽位都不建）。
    async fn round_trip_tolerant(
        window: &tauri::WebviewWindow,
        expression: &str,
        timeout: Duration,
    ) -> Result<(Value, u32, u32), String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let slot = format!("__dsh_mrb_{:x}", nanos);
        let slot_json = serde_json::to_string(&slot).unwrap_or_else(|_| "\"\"".to_string());
        let script = format!(
            r#"(function () {{
  var SLOT = {slot};
  function put(state) {{
    try {{
      Object.defineProperty(window, SLOT, {{
        value: state, writable: true, configurable: true, enumerable: false
      }});
    }} catch (e) {{
      try {{ window[SLOT] = state; }} catch (e2) {{ /* 页面不允许写就算了 */ }}
    }}
  }}
  put({{ state: "pending", startedAt: Date.now() }});
  try {{
    Promise.resolve().then(function () {{ return ({expression}); }}).then(
      function (value) {{
        put({{ state: "done", value: (value === undefined ? null : value), finishedAt: Date.now() }});
      }},
      function (error) {{
        var message = "";
        try {{ message = (error && error.message) ? String(error.message) : String(error); }}
        catch (e) {{ message = "无法读取拒绝原因"; }}
        put({{ state: "rejected", error: message, finishedAt: Date.now() }});
      }}
    );
  }} catch (e) {{
    put({{ state: "rejected", error: "表达式抛错: " + String(e), finishedAt: Date.now() }});
  }}
  return true;
}})()"#,
            slot = slot_json,
            expression = expression
        );
        window
            .eval(&script)
            .map_err(|e| format!("注入脚本失败：{e}"))?;
        let started = std::time::Instant::now();
        let mut polls: u32 = 0;
        let mut failed_reads: u32 = 0;
        loop {
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                let _ = window.eval(format!(
                    "try {{ delete window[{slot}]; }} catch (e) {{}}",
                    slot = slot_json
                ));
                return Err(format!(
                    "页内请求在 {}ms 内没有结束（已等 {}ms）",
                    timeout.as_millis(),
                    elapsed.as_millis()
                ));
            }
            match crate::script_eval::read_json(
                window,
                &format!("window[{slot}]", slot = slot_json),
                Duration::from_millis(2000),
            )
            .await
            {
                Ok(Value::Object(map)) => {
                    polls += 1;
                    match map.get("state").and_then(Value::as_str) {
                        Some("done") => {
                            let value = map.get("value").cloned().unwrap_or(Value::Null);
                            let _ = window.eval(format!(
                                "try {{ delete window[{slot}]; }} catch (e) {{}}",
                                slot = slot_json
                            ));
                            return Ok((value, polls, failed_reads));
                        }
                        Some("rejected") => {
                            let message = map
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("页面脚本拒绝（未给出原因）")
                                .to_string();
                            let _ = window.eval(format!(
                                "try {{ delete window[{slot}]; }} catch (e) {{}}",
                                slot = slot_json
                            ));
                            return Err(message);
                        }
                        _ => {}
                    }
                }
                Ok(_) => {
                    polls += 1;
                }
                Err(_) => {
                    // ⚠️ **读回失败**（页面正忙／文档被换掉）⇒ ⇒ **重试**，**不放弃**。
                    failed_reads += 1;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// 探针主体。
    pub fn run() {
        // 看门狗：卡住时自己退出。
        std::thread::Builder::new()
            .name("minimax-route-b-watchdog".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(super::WATCHDOG_SECONDS));
                log_line("WATCHDOG", "超时，探针自己退出");
                std::process::exit(3);
            })
            .expect("probe: 无法起看门狗线程");

        log_line("probe", "run() 入口");
        let cookie_before = source_cookie_sha256();
        let (_store_dir, store_reading) = match prepare_store(&PROBE_DATA_STORE_IDENTIFIER) {
            Ok(pair) => pair,
            Err(e) => {
                finish(&json!({ "fatal": format!("存储准备失败：{e}") }).to_string());
                std::process::exit(2);
            }
        };
        let url = match url::Url::parse(SITE_URL) {
            Ok(url) => url,
            Err(e) => {
                finish(&json!({ "fatal": format!("站点地址非法：{e}") }).to_string());
                std::process::exit(2);
            }
        };
        log_line("probe", "存储与站点地址就绪");

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
                let store_reading = store_reading.clone();
                let cookie_before = cookie_before.clone();
                std::thread::Builder::new()
                    .name("minimax-route-b-probe".into())
                    .spawn(move || {
                        log_line("probe", "工作线程启动");
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                tauri::async_runtime::block_on(async move {
                                    run_probe_body(&handle, &url, store_reading, cookie_before)
                                        .await
                                })
                            }));
                        let report = match outcome {
                            Ok(report) => report,
                            Err(_) => json!({ "fatal": "工作线程 panic" }),
                        };
                        finish(&report.to_string());
                        // ⚠️ **不要关窗**：tauri v2 里关掉最后一扇窗 ⇒ 事件循环结束
                        // ⇒ **主线程结束 ⇒ 进程直接退出** ⇒ 报告来不及打印。
                        std::process::exit(0);
                    })
                    .expect("probe: 无法起工作线程");
            }
        });
    }

    /// 主体：建窗 → 等 commit → 跑**生产那份**页内脚本 → 五条断言。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &url::Url,
        store_reading: Value,
        cookie_before: Option<String>,
    ) -> Value {
        let mut report = json!({
            "site": "minimax",
            "probe": "official_chat_minimax_route_b_probe",
            "round": std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string()),
            "purpose": "验生产那份页内挂钩脚本（形态 C）：目录截到 / 还原后残留为零 / 正文重放 / 反向对照 400 / 阳性对照 200",
            "site_url": SITE_URL,
            "catalog_path": CATALOG_PATH,
            "message_path_shape": MESSAGE_PATH_TEMPLATE,
            "store": store_reading,
            "hidden_window": hidden_window(),
            "source_cookie_sha256_before": cookie_before.clone().map(Value::from).unwrap_or(Value::Null),
            "discipline": "凭据（签名/token/cookie）与会话 id、标题原文全程不进出读数；只报状态码、业务码、结构键名、条数、字节数、凭据长度、耗时",
        });

        // ── S1：建窗（绑探针自己的存储）＋ 导航 ────────────────────────────
        let window = match build_window(
            app,
            "minimax-route-b-probe",
            url,
            &PROBE_DATA_STORE_IDENTIFIER,
        ) {
            Ok(window) => window,
            Err(e) => {
                report["fatal"] = json!(e);
                return report;
            }
        };

        // ── S2：等首次导航 commit（**求值能被丢掉的那一格**）────────────────
        let commit = match wait_commit(&window).await {
            Ok(value) => value,
            Err(e) => {
                report["fatal"] = json!(e);
                return report;
            }
        };
        report["s2_commit"] = commit;
        log_line("S2", "首次导航已 commit");

        // ── S2.5：本机有哪些「让出宏任务」的原语（**诊断**）──────────────
        let capabilities =
            crate::script_eval::read_json(&window, CAPABILITIES_EXPR, Duration::from_secs(5))
                .await
                .unwrap_or(Value::Null);
        report["s2_capabilities"] = capabilities.clone();
        log_line("S2", &format!("本机原语：{capabilities}"));

        // ── S3：跑**生产那份**页内脚本（回程走 `round_trip`）────────────────
        let script = hook_script();
        log_line("S3", &format!("注入生产脚本（{} 字节）", script.len()));
        let trip = round_trip_tolerant(&window, &script, ROUND_TRIP_TIMEOUT).await;
        let reading = match trip {
            Ok((value, polls, failed_reads)) => {
                log_line(
                    "S3",
                    &format!("回程拿到结果（轮数={polls} 读回失败重试={failed_reads}）"),
                );
                report["s3_round_trip"] = json!({
                    "polls": polls,
                    "failed_reads_retried": failed_reads,
                    "note": "**注入只发生一次**；读回失败是**重试**不是重新注入（重新注入会让页内跑两个实例 ⇒ 还原链就错了）",
                });
                value
            }
            Err(e) => {
                report["fatal"] = json!(format!("页内脚本回程失败：{e}"));
                report["source_cookie_sha256_after"] = source_cookie_sha256()
                    .map(Value::from)
                    .unwrap_or(Value::Null);
                return report;
            }
        };

        // ⚠️ **读数纪律**：脚本回的是**结果对象**（含裁剪后的目录条目 ⇒ 里面有
        //    会话 id 与标题原文）⇒ ⇒ **报告里一个字都不许带** ⇒ ⇒ 只挑结构字段。
        let ok = reading.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let stage = text(&reading, &["stage"]).unwrap_or_default();
        report["s3_reading"] = json!({
            "ok": ok,
            "stage": stage,
            "error": text(&reading, &["error"]),
            "install": reading.get("install").cloned().unwrap_or(Value::Null),
            "capture": reading.get("capture").cloned().unwrap_or(Value::Null),
            "restore": reading.get("restore").cloned().unwrap_or(Value::Null),
            "items_count": reading.get("itemsCount").cloned().unwrap_or(Value::Null),
            "body": reading.get("body").cloned().unwrap_or(Value::Null),
            "controls": reading.get("controls").cloned().unwrap_or(Value::Null),
            "window_own_count_at_start": reading.get("windowOwnCountAtStart").cloned().unwrap_or(Value::Null),
            "window_own_count_now": reading.get("windowOwnCountNow").cloned().unwrap_or(Value::Null),
            "note": "**目录条目本身（会话 id／标题原文）不进报告** —— 只报条数与键名",
        });
        log_line("S3", &format!("页内脚本结果：ok={ok} stage={stage}"));

        // ── 断言 ①：目录截到 ───────────────────────────────────────────────
        let capture_top_keys = strings(&reading, &["capture", "summary", "topKeys"]);
        let capture_item_keys = strings(&reading, &["capture", "summary", "firstItemKeys"]);
        let sessions_count = num(&reading, &["capture", "summary", "sessionsCount"]).unwrap_or(0.0);
        let (top_ok, top_missing) = has_all(
            &capture_top_keys,
            &["sessions", "base_resp", "has_more", "next_cursor"],
        );
        let (item_ok, item_missing) =
            has_all(&capture_item_keys, &["session_id", "title", "updated_at"]);
        let assert_1 = ok && sessions_count >= 1.0 && top_ok && item_ok;
        report["assert_1_catalog_captured"] = json!({
            "pass": assert_1,
            "sessions_count": sessions_count,
            "top_keys": capture_top_keys,
            "top_keys_missing": top_missing,
            "item_keys": capture_item_keys,
            "item_keys_missing": item_missing,
            "capture_header_names": strings(&reading, &["capture", "headerNames"]),
            "capture_header_lengths": reading
                .get("capture")
                .and_then(|c| c.get("headerLengths"))
                .cloned()
                .unwrap_or(Value::Null),
            "body_length": num(&reading, &["capture", "summary", "bodyLength"]),
            "biz_code": num(&reading, &["capture", "summary", "bizCode"]),
            // ⚠️ **首条目时间戳**（**只取数值**）：规格表说 MiniMax 的 `updated_at`
            //    是**毫秒** ⇒ ⇒ 这一格就是核 `updated_at_divisor: 1000.0` 的读数。
            "first_item_updated_at": first_item_number(&reading, "updated_at"),
            "note": "**值不进读数**（头只有名字与长度；目录条目只报键名与时间戳）",
        });
        log_line(
            "A1",
            &format!("目录截到：{assert_1}（条数={sessions_count}）"),
        );

        // ── 断言 ②：还原后残留为零 ─────────────────────────────────────────
        // ⚠️ **以「探针独立测量」为准**（脚本自报只作参考）⇒ ⇒ 一份改坏了的
        //    脚本自己报「残留为零」骗不过这一格。
        let independent =
            crate::script_eval::read_json(&window, RESIDUE_EXPR, Duration::from_secs(5))
                .await
                .unwrap_or(Value::Null);
        let ind_has_our_source = independent
            .get("fetchHasOurSource")
            .and_then(Value::as_bool);
        let ind_xhr_marks = independent
            .get("newXhrHookMarks")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let ind_own_count = independent.get("windowOwnCount").and_then(Value::as_f64);
        let ind_probe_names = independent
            .get("probeOwnNames")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let fetch_has_our_source = bool_of(&reading, &["restore", "fetchHasOurSource"]);
        let xhr_marks = strings(&reading, &["restore", "newXhrHookMarks"]);
        let own_delta = num(&reading, &["restore", "windowOwnCountDelta"]);
        let restored_is_prev = bool_of(&reading, &["restore", "restoredFetchIsPrev"]);
        let assert_2 = ind_has_our_source == Some(false)
            && ind_xhr_marks.is_empty()
            && ind_own_count.is_some()
            && restored_is_prev == Some(true);
        report["assert_2_residue_zero"] = json!({
            "pass": assert_2,
            "independent": {
                "note": "**探针自己量的**（不信任脚本自报）",
                "fetch_to_string_has_our_source": ind_has_our_source,
                "fetch_src_length": independent.get("fetchSrcLength").cloned().unwrap_or(Value::Null),
                "fetch_is_native": independent.get("fetchIsNative").cloned().unwrap_or(Value::Null),
                "new_xhr_instance_marks": ind_xhr_marks,
                "window_own_count": ind_own_count,
                "probe_own_names": ind_probe_names,
            },
            "script_self_report": {
                "fetch_to_string_has_our_source": fetch_has_our_source,
                "new_xhr_instance_marks": xhr_marks,
                "window_own_count_delta": own_delta,
                "restored_fetch_is_previous_reference": restored_is_prev,
                "window_own_count_before": num(&reading, &["restore", "windowOwnCountBefore"]),
                "window_own_count_after": num(&reading, &["restore", "windowOwnCountAfter"]),
                "probe_own_names": strings(&reading, &["restore", "probeOwnNames"]),
                "fetch_src_length_after_restore": num(&reading, &["restore", "fetchSrcLength"]),
            },
            "note": "三条分别对应：`fetch.toString()` 不含我们的源码 / 新建 XHR 实例无 `__mmx*` 标记 / 还原前后 window 自有属性数差 0",
        });
        log_line("A2", &format!("残留为零：{assert_2}（独立读数 oursource={ind_has_our_source:?} xhr_marks={ind_xhr_marks:?}）"));

        // ── 断言 ③：正文重放（还原之后，用截到的签名头）───────────────────
        let body_status = num(&reading, &["body", "summary", "status"]);
        let body_messages = num(&reading, &["body", "summary", "messagesCount"]).unwrap_or(0.0);
        let body_top_keys = strings(&reading, &["body", "summary", "topKeys"]);
        let body_message_keys = strings(&reading, &["body", "summary", "firstMessageKeys"]);
        let (body_top_ok, body_top_missing) =
            has_all(&body_top_keys, &["messages", "has_more", "base_resp"]);
        let (body_msg_ok, body_msg_missing) =
            has_all(&body_message_keys, &["msg_id", "role", "msg_content"]);
        let roles: Vec<String> = reading
            .get("body")
            .and_then(|b| b.get("summary"))
            .and_then(|s| s.get("roleCounts"))
            .and_then(Value::as_object)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        let roles_ok = !roles.is_empty()
            && roles
                .iter()
                .all(|role| role == "user" || role == "assistant");
        let assert_3 = body_status == Some(200.0)
            && body_messages >= 1.0
            && body_top_ok
            && body_msg_ok
            && roles_ok;
        report["assert_3_body_replay"] = json!({
            "pass": assert_3,
            "status": body_status,
            "biz_code": num(&reading, &["body", "summary", "bizCode"]),
            "messages_count": body_messages,
            "top_keys": body_top_keys,
            "top_keys_missing": body_top_missing,
            "message_keys": body_message_keys,
            "message_keys_missing": body_msg_missing,
            "roles": roles,
            "has_msg_content": bool_of(&reading, &["body", "summary", "hasMsgContent"]),
            "has_thinking_content": bool_of(&reading, &["body", "summary", "hasThinkingContent"]),
            "body_length": num(&reading, &["body", "summary", "bodyLength"]),
            "session_id_length": num(&reading, &["body", "prep", "sessionIdLength"]),
            "note": "**会话 id 的值不出页**（只报长度）",
        });
        log_line(
            "A3",
            &format!("正文重放：{assert_3}（条数={body_messages}）"),
        );

        // ── 断言 ④：反向对照（摘掉签名 ⇒ 400）──────────────────────────────
        let unsigned_status = num(&reading, &["controls", "unsigned", "summary", "status"]);
        let assert_4 = unsigned_status == Some(400.0);
        report["assert_4_unsigned_control"] = json!({
            "pass": assert_4,
            "status": unsigned_status,
            "body_length": num(&reading, &["controls", "unsigned", "summary", "bodyLength"]),
            "kept_header_names": strings(&reading, &["controls", "unsigned", "strippedHeaderNames"]),
            "note": "摘掉签名三件套后重放 ⇒ 必须 400（证明读到的是真签名，不是空壳）",
        });
        log_line(
            "A4",
            &format!("反向对照：{assert_4}（status={unsigned_status:?}）"),
        );

        // ── 断言 ⑤：阳性对照（还原之前发的那一发 ⇒ 200）────────────────────
        let positive_status = num(&reading, &["controls", "positive", "summary", "status"]);
        let assert_5 = positive_status == Some(200.0);
        report["assert_5_positive_control"] = json!({
            "pass": assert_5,
            "status": positive_status,
            "biz_code": num(&reading, &["controls", "positive", "summary", "bizCode"]),
            "body_length": num(&reading, &["controls", "positive", "summary", "bodyLength"]),
            "issued_before_restore": bool_of(&reading, &["controls", "positive", "issuedBeforeRestore"]),
            "note": "**在还原之前**发出的重放（钩子还在）⇒ 证明截到的签名头确实可用",
        });
        log_line(
            "A5",
            &format!("阳性对照：{assert_5}（status={positive_status:?}）"),
        );

        // ── 汇总 ───────────────────────────────────────────────────────────
        let all_pass = assert_1 && assert_2 && assert_3 && assert_4 && assert_5;
        report["all_assertions_pass"] = json!(all_pass);
        report["timing"] = json!({
            "install_waited_ms": num(&reading, &["install", "waitedMs"]),
            "install_attempts": num(&reading, &["install", "attempts"]),
            "install_path": text(&reading, &["install", "path"]),
            "catalog_already_requested_before_install": bool_of(
                &reading,
                &["install", "catalogAlreadyRequestedBeforeInstall"]
            ),
            "on_top_of_src_length": num(&reading, &["install", "onTopOfSrcLength"]),
            "on_top_of_is_native": bool_of(&reading, &["install", "onTopOfIsNative"]),
        });
        report["notes"] = json!([
            "页内脚本是**生产那份**（`include_str!` 引进，只替换 `__SPEC__` 规格占位）",
            "**取到即还原**：还原发生在截到**请求头**那一刻（响应体走克隆体读，与钩子无关）",
            "还原成**替换前那个引用**（站点自己那层），不是原生",
            "报告里**不含**会话 id／标题原文／任何凭据的值",
        ]);

        // ── 源 cookie 的 sha256（跑前跑后必须一样）────────────────────────
        let cookie_after = source_cookie_sha256();
        report["source_cookie_sha256_after"] =
            cookie_after.clone().map(Value::from).unwrap_or(Value::Null);
        report["source_cookie_unchanged"] = match (&cookie_before, &cookie_after) {
            (Some(a), Some(b)) => json!(a == b),
            _ => json!(null),
        };

        report
    }
}

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
