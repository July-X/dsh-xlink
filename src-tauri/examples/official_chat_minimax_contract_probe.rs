//! MiniMax 路线 B 的**认证契约**探针（macOS 真机）。
//!
//! ## 它回答三个问题
//!
//! ① **token 在哪** —— localStorage 的哪个键 / 哪个 cookie
//!    （**只报键名与长度，值一步都不出页**）；
//! ② 目录端点的 **query 参数**哪些是**必需**的 —— 台账里那条 URL 是**站点自己
//!    发的**（带了一串 device 参数：`unix` / `uuid` / `device_id` …）⇒ ⇒ 哪些
//!    能省**必须实测**，不能照抄；
//! ③ 哪种 URL 形态能拿到**非空会话目录**。
//!
//! ## 为什么需要这一格
//!
//! 现有 `PageRoute` 的认证形态只有**「取凭据 → 拼成请求头」**（DeepSeek 那条），
//! 而 MiniMax 的认证是 **token 走 URL query**（台账实测：`…&token=<448 chars>&…`）
//! ⇒ ⇒ 生产形态**装不下它** ⇒ ⇒ 得先**测清**契约，再扩形态。
//!
//! ## 隔离（判据 `probe-profile-isolation`）
//!
//! 源是用户真实那份 WK 存储 ⇒ **只读拷贝**到探针自己的目录 ⇒ 探针写的全在
//! 拷贝里，用户正在跑的那份存储一个字节都没改。
//!
//! ## 读数纪律
//!
//! **不报**：token 的值、会话标题原文、会话 id。
//! **报**：键名、长度、状态码、业务码、条数、顶层键名。
//!
//! 这条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块编译掉。
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

/// 探针自己的数据存储标识符（**不是**生产的任何一个）。
///
/// ⚠️ **纪律**：探针**绝不能**起在用户真实那份档案上（浏览器一启动就写
/// Cookies／LocalStorage）⇒ ⇒ 生产那两个标识符在探针里**一个都不许出现**。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-mm2-001";

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::{json, Value};

    /// 探针自己的数据存储标识符（根级那个）。
    use crate::PROBE_DATA_STORE_IDENTIFIER;

    /// **源存储目录**从哪个环境变量读（源码里不写路径字面量）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// MiniMax 的入口地址（生产规格表里那个 `root_url`）。
    const SITE_URL: &str = "https://agent.minimaxi.com";

    /// 目录端点（生产规格表里那个 `catalog_path`）。
    const CATALOG_PATH: &str = "/minimax-cloud/api/v1/sidebar/session/tree";

    /// 台账记的那一串参数（**不含** device 身份那三个：`unix` / `uuid` /
    /// `device_id`）⇒ ⇒ 「哪些参数必需」由实测回答，不照抄。
    const BASE_PARAMS: &str = "limit=20&project_id=0&device_platform=web&biz_id=3&app_id=3001&version_code=22201&client=web&region=cn";

    /// **站点自有请求拦截器**（在页面脚本之前注入）。
    ///
    /// ## 它回答什么
    ///
    /// 「站点自己**到底打的是哪个端点、带哪些参数**」——**不靠猜**。
    /// ⇒ ⇒ 记录 `fetch` ／ `XMLHttpRequest` 的 URL（**token 打码**）与方法。
    ///
    /// ## 只记录 URL 与方法
    ///
    /// **不记录**请求体、**不记录**请求头 ⇒ ⇒ 凭据与内容都不进记录。
    const INTERCEPT_SCRIPT: &str = r#"
(function () {
  var rec = [];
  function mask(url) {
    return String(url).replace(/([?&]token=)[^&]*/g, '$1<REDACTED>');
  }
  function record(url, method, headerNames) {
    try {
      rec.push({ url: mask(url), method: String(method || 'GET'), headers: headerNames || [] });
      if (rec.length > 80) rec.shift();
    } catch (e) {}
  }
  function hook() {
    try {
      if (!window.fetch.__mmxHooked) {
        var origFetch = window.fetch;
        var wrapped = function (input, init) {
          try {
            var u = (typeof input === 'string') ? input : ((input && input.url) || '');
            var names = [];
            try {
              var h = (init && init.headers) || (input && input.headers);
              if (h && typeof h.forEach === 'function') { h.forEach(function (v, k) { names.push(String(k)); }); }
              else if (h && typeof h === 'object') { names = Object.keys(h); }
            } catch (e) {}
            record(u, (init && init.method) || (input && input.method) || 'GET', names);
          } catch (e) {}
          return origFetch.apply(this, arguments);
        };
        try { wrapped.__mmxHooked = true; } catch (e) {}
        window.fetch = wrapped;
      }
    } catch (e) {}
    try {
      if (!XMLHttpRequest.prototype.open.__mmxHooked) {
        var XO = XMLHttpRequest.prototype.open;
        var XS = XMLHttpRequest.prototype.setRequestHeader;
        var wrappedOpen = function (method, url) {
          try {
            this.__mmxHeaders = [];
            var names = this.__mmxHeaders;
            if (!this.__mmxSetHooked) {
              var wrappedSet = function (name, value) {
                try { this.__mmxHeaders.push(String(name)); } catch (e) {}
                return XS.apply(this, arguments);
              };
              try { this.__mmxSetHooked = true; } catch (e) {}
              this.setRequestHeader = wrappedSet;
            }
            record(url, method, []);
            var self = this;
            setTimeout(function () { try { record(url, method, self.__mmxHeaders || []); } catch (e) {} }, 0);
          } catch (e) {}
          return XO.apply(this, arguments);
        };
        try { wrappedOpen.__mmxHooked = true; } catch (e) {}
        XMLHttpRequest.prototype.open = wrappedOpen;
      }
    } catch (e) {}
  }
  hook();
  // 站点自己可能会**覆盖** fetch ⇒ ⇒ 定时补挂（抓到它后来的请求）。
  try { setInterval(hook, 50); } catch (e) {}
  window.__mmxReqs = rec;
})()
"#;

    /// 页内脚本：把**页内事实**与**几种 URL 形态的取数结果**一起带回来。
    ///
    /// ⚠️ **token 的值只留在页内**：脚本只在结果里报**长度** ⇒ ⇒ Rust 侧
    /// **一个字节的 token 都拿不到** ⇒ ⇒ 凭据零落盘这条纪律在探针里也成立。
    const CONTRACT_SCRIPT: &str = r#"
(async function () {
  var out = { ok: true, origin: String(location.origin), host: String(location.host), href: String(location.href) };

  // ---- 站点**自己**打出去的请求（URL 已打码）⇒ ⇒ 端点与参数名不靠猜 ----
  try {
    var reqs = (window.__mmxReqs || []).filter(function (r) { return r && r.url; });
    out.siteRequests = reqs.slice(0, 40);
    out.siteRequestCount = reqs.length;
  } catch (e) { out.siteRequestsError = String(e); }

  // ---- localStorage：**键名 + 值长度**（不报值）----
  var ls = [];
  try {
    for (var i = 0; i < localStorage.length; i++) {
      var k = localStorage.key(i);
      var v = '';
      try { v = localStorage.getItem(k) || ''; } catch (e) {}
      ls.push({ key: k, len: v.length });
    }
  } catch (e) { out.localStorageError = String(e); }
  out.localStorage = ls;

  // ---- cookie：**名 + 长度**（不报值）----
  var ck = [];
  try {
    var parts = String(document.cookie || '').split(';');
    for (var i = 0; i < parts.length; i++) {
      var t = parts[i].trim();
      if (!t) continue;
      var eq = t.indexOf('=');
      var name = eq >= 0 ? t.slice(0, eq) : t;
      var val = eq >= 0 ? t.slice(eq + 1) : '';
      ck.push({ name: name, len: val.length });
    }
  } catch (e) { out.cookieError = String(e); }
  out.cookies = ck;

  function lsGet(key) { try { return localStorage.getItem(key) || ''; } catch (e) { return ''; } }
  function ckGet(name) {
    try {
      var parts = String(document.cookie || '').split(';');
      for (var i = 0; i < parts.length; i++) {
        var t = parts[i].trim();
        if (t.indexOf(name + '=') === 0) return t.slice(name.length + 1);
      }
    } catch (e) {}
    return '';
  }

  // ---- 页面状态：SPA 有没有真起来（标题 ＋ 正文前 300 字）----
  try {
    out.pageTitle = String(document.title || '');
    out.pageText = String(document.body ? (document.body.innerText || '') : '').slice(0, 300);
  } catch (e) { out.pageStateError = String(e); }

  // ---- 浏览器自己记的请求清单（`performance`）⇒ ⇒ 不靠 JS 挂钩也能拿到 ----
  try {
    var perf = performance.getEntriesByType('resource') || [];
    var picked = [];
    for (var i = 0; i < perf.length; i++) {
      var name = String(perf[i].name || '');
      picked.push(name.replace(/([?&]token=)[^&]*/g, '$1<REDACTED>'));
    }
    out.apiUrls = picked.filter(function (name) {
      return /minimax-cloud|\/archon\/|sidebar|session\/tree|\/api\//i.test(name);
    }).slice(0, 20);
    out.allUrls = picked.slice(0, 30);
    {
      var inner = '';
      try { inner = String(document.body ? (document.body.innerText || '') : ''); } catch (e) {}
      out.innerTextLength = inner.length;
      out.innerTextHead = inner.slice(0, 200);
    }
    out.performanceUrls = picked.slice(0, 30);
    out.performanceCount = perf.length;
  } catch (e) { out.performanceError = String(e); }

  // ---- 设备身份类键（**长度**；值只在页内用）----
  var deviceish = [];
  for (var i = 0; i < ls.length; i++) {
    if (/uuid|device|hardware|unique|install|guid/i.test(ls[i].key)) {
      deviceish.push({ key: ls[i].key, len: ls[i].len });
    }
  }
  out.deviceishKeys = deviceish;

  // ---- token 候选：**键名里带 token 的**（localStorage 与 cookie 各一遍）----
  var cands = [];
  for (var i = 0; i < ls.length; i++) {
    if (ls[i].len > 0 && /token/i.test(ls[i].key)) {
      cands.push({ from: 'localStorage', key: ls[i].key, value: lsGet(ls[i].key) });
    }
  }
  for (var i = 0; i < ck.length; i++) {
    if (ck[i].len > 0 && /token/i.test(ck[i].name)) {
      cands.push({ from: 'cookie', key: ck[i].name, value: ckGet(ck[i].name) });
    }
  }
  out.tokenCandidates = cands.map(function (c) { return { from: c.from, key: c.key, len: c.value.length }; });

  // ---- 重放：目录端点的几种 URL 形态 ----
  function shape(text, status) {
    var s = { status: status, len: text.length, topKeys: [], sessionsLen: -1, bizCode: null, firstItemKeys: [] };
    try {
      var j = JSON.parse(text);
      s.topKeys = Object.keys(j || {}).slice(0, 14);
      if (j && Array.isArray(j.sessions)) {
        s.sessionsLen = j.sessions.length;
        if (j.sessions.length) {
          var el = j.sessions[0] || {};
          s.firstItemKeys = Object.keys(el).slice(0, 12);
          if (el.session && typeof el.session === 'object') s.firstItemKeys = Object.keys(el.session).slice(0, 12);
        }
      }
      if (j && j.base_resp && typeof j.base_resp.status_code !== 'undefined') s.bizCode = j.base_resp.status_code;
      // 失败形态的**服务端文案**（不是凭据，可报）—— 打码后截断。
      if (j && j.error !== undefined) {
        s.errorText = String(j.error).replace(/([?&]token=)[^&]*/g, '$1<REDACTED>').slice(0, 120);
      }
    } catch (e) { s.parseError = true; }
    return s;
  }
  async function attempt(label, url) {
    try {
      var res = await fetch(url, { credentials: 'include' });
      var text = await res.text();
      var s = shape(text, res.status);
      s.label = label;
      return s;
    } catch (e) {
      return { label: label, error: String(e) };
    }
  }

  var PATHS = ['__PATH__', '/archon/api/v1/sidebar/session/tree'];
  // 设备身份：从 localStorage 里**名字像设备标识**的键取值（页内取，不出页）。
  var deviceId = '', uuid = '';
  try {
    for (var i = 0; i < ls.length; i++) {
      var k = ls[i].key;
      // ⚠️ 这里**不能**用后面才定义的 `lsGet`（会 ReferenceError ⇒ 整个脚本抛错
      // ⇒ 后面的读数全丢）⇒ ⇒ 直接取。
      try {
        if (!deviceId && /device/i.test(k)) deviceId = localStorage.getItem(k) || '';
        if (!uuid && /uuid/i.test(k)) uuid = localStorage.getItem(k) || '';
      } catch (e) {}
    }
  } catch (e) {}
  var unix = String(Date.now());
  var results = [];

  async function attempt(label, url, headers) {
    try {
      var opts = { credentials: 'include' };
      if (headers) opts.headers = headers;
      var res = await fetch(url, opts);
      var text = await res.text();
      var s = shape(text, res.status);
      s.label = label;
      return s;
    } catch (e) {
      return { label: label, error: String(e) };
    }
  }

  for (var p = 0; p < PATHS.length; p++) {
    var path = PATHS[p];
    var tag = 'path' + p;
    var devQ = (deviceId ? '&device_id=' + encodeURIComponent(deviceId) : '')
             + (uuid ? '&uuid=' + encodeURIComponent(uuid) : '')
             + '&unix=' + encodeURIComponent(unix);
    var paramSets = [
      { name: 'bare', q: '' },
      { name: 'base', q: '__BASE__' },
      { name: 'base+dev', q: '__BASE__' + devQ }
    ];
    for (var sIdx = 0; sIdx < paramSets.length; sIdx++) {
      var ps = paramSets[sIdx];
      var q = ps.q ? (ps.q + '&') : '';
      results.push(await attempt(tag + '|' + ps.name + '|no-auth', path + (q ? '?' + q : ''), null));
      for (var i = 0; i < cands.length; i++) {
        var c = cands[i];
        var tok = c.value;
        var enc = encodeURIComponent(tok);
        // ① token 走 query
        results.push(await attempt(tag + '|' + ps.name + '|query-token', path + '?' + q + 'token=' + enc, null));
        // ② token 走各种头（**头名**是候选，值一律是那个 token 本身）
        var placements = [
          ['h-token', { 'token': tok }],
          ['h-authorization-bearer', { 'authorization': 'Bearer ' + tok }],
          ['h-authorization-raw', { 'authorization': tok }],
          ['h-x-token', { 'x-token': tok }],
          ['h-access-token', { 'access-token': tok }],
          ['h-sign', { 'sign': tok }],
          ['h-signature', { 'signature': tok }]
        ];
        for (var pi = 0; pi < placements.length; pi++) {
          var pl = placements[pi];
          var hdrs = {};
          hdrs[pl[0].slice(2)] = pl[1][Object.keys(pl[1])[0]];
          results.push(await attempt(tag + '|' + ps.name + '|' + pl[0], path + (q ? '?' + q : ''), hdrs));
        }
      }
    }
  }
  out.attempts = results;
  return out;
})()
"#;

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
    fn log_path() -> PathBuf {
        let round = std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string());
        std::env::temp_dir().join(format!("dsh-minimax-contract-probe.{round}.log"))
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
    ) -> Result<tauri::WebviewWindow, String> {
        // **无条件**给 `data_directory`（判据 `probe-profile-isolation` 按**代码
        // 形状**判，「有才给」同样违规）。macOS 上 WKWebView **忽略**它
        // （它认的是 `data_store_identifier`），但照样给——口径一致。
        let profile = std::env::temp_dir().join("dsh-official-chat-minimax-contract-probe");
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("无法创建临时 profile 目录（{}）：{e}", profile.display()))?;

        let builder =
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(url.clone()))
                .title("dsh MiniMax 契约探针")
                .inner_size(900.0, 700.0)
                // ⚠️ **必须可见**：隐藏窗里那个 SPA **不跑**（r5 实测：站点自己
                // 一次 API 都没打 ⇒ performance 里零命中）⇒ ⇒ 签名的形态**抓不到**
                // ⇒ ⇒ 可见窗让站点**自己**发那一发，我们只看它的 URL 与头名。
                .visible(true)
                .data_directory(profile)
                .additional_browser_args(crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
                .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
                .initialization_script(INTERCEPT_SCRIPT)
                .data_store_identifier(*identifier);

        log_line("probe", &format!("准备 build()（label={label}）"));
        let built = builder.build();
        log_line("probe", "build() 返回");
        built.map_err(|e| format!("无法创建探针窗口：{e}"))
    }

    /// 等页面就绪：`location.origin` **连续 3 次**不变才算稳（站点会 301 到
    /// 真正的 origin ⇒ ⇒ 第一次读到的 origin 未必是最终那个）。
    async fn wait_ready(window: &tauri::WebviewWindow) -> Result<Value, String> {
        let mut stable = 0;
        let mut last = String::new();
        for _ in 0..120 {
            match crate::script_eval::read_json(
                window,
                "String(location.origin)",
                Duration::from_secs(2),
            )
            .await
            {
                Ok(value) => {
                    let text = value.as_str().unwrap_or("").to_string();
                    if !text.is_empty() && text == last {
                        stable += 1;
                    } else {
                        stable = 0;
                    }
                    last = text.clone();
                    if stable >= 3 {
                        return Ok(json!({ "origin": text }));
                    }
                }
                Err(_) => {
                    stable = 0;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(format!("页面没就绪（最后读到的 origin：{last}）"))
    }

    /// 探针主体。
    pub fn run() {
        // 看门狗：卡住时自己退出。
        std::thread::Builder::new()
            .name("minimax-contract-watchdog".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(super::WATCHDOG_SECONDS));
                log_line("WATCHDOG", "超时，探针自己退出");
                std::process::exit(3);
            })
            .expect("probe: 无法起看门狗线程");

        log_line("probe", "run() 入口");
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
                std::thread::Builder::new()
                    .name("minimax-contract-probe".into())
                    .spawn(move || {
                        log_line("probe", "工作线程启动");
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            tauri::async_runtime::block_on(async move {
                                run_probe_body(&handle, &url, store_reading).await
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

    /// 打完报告就退出。
    fn finish(report: &str) {
        println!("PROBE_REPORT_BEGIN");
        println!("{report}");
        println!("PROBE_REPORT_END");
        log_line("report", report);
    }

    /// 三格：**事实** → **重放** → **结论**。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &url::Url,
        store_reading: Value,
    ) -> Value {
        let mut report = json!({
            "site": "minimax",
            "site_url": SITE_URL,
            "catalog_path": CATALOG_PATH,
            "store": store_reading,
        });

        // ── S1：建窗（绑探针自己的存储）＋ 导航 ＋ 等就绪 ──────────────────
        let window = match build_window(app, "minimax-contract-probe", url, &PROBE_DATA_STORE_IDENTIFIER)
        {
            Ok(window) => window,
            Err(e) => {
                report["fatal"] = json!(e);
                return report;
            }
        };
        let ready = match wait_ready(&window).await {
            Ok(ready) => ready,
            Err(e) => {
                report["fatal"] = json!(e);
                return report;
            }
        };
        report["s1_ready"] = ready;

        // ⚠️ **要等 SPA 真的起来**：r6 实测（origin 已稳、正文仍空）⇒ ⇒ 页面
        // 是 SSR 出来的壳，**应用还没渲染** ⇒ ⇒ 那时站点自己的 API 请求还没发
        // ⇒ ⇒ 签名形态抓不到 ⇒ ⇒ 这里**静候 10 秒**再取读数。
        tokio::time::sleep(Duration::from_secs(10)).await;
        log_line("S2", "等 SPA 起来（10 秒）后开始取读数");

        // ── S2／S3：页内事实 ＋ 几种 URL 形态的重放（一次跑完）───────────
        let script = CONTRACT_SCRIPT
            .replace("__PATH__", CATALOG_PATH)
            .replace("__BASE__", BASE_PARAMS);
        let result =
            crate::script_eval::round_trip(&window, &script, Duration::from_secs(60)).await;
        report["s2_contract"] = match &result {
            Ok(trip) => {
                let value = &trip.value;
                let attempts = value
                    .get("attempts")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let token_candidates = value
                    .get("tokenCandidates")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let local_storage = value.get("localStorage").cloned().unwrap_or_else(|| json!([]));
                let cookies = value.get("cookies").cloned().unwrap_or_else(|| json!([]));
                json!({
                    "ok": true,
                    "origin": value.get("origin").cloned().unwrap_or(Value::Null),
                    "host": value.get("host").cloned().unwrap_or(Value::Null),
                    "site_requests": value.get("siteRequests").cloned().unwrap_or_else(|| json!([])),
                    "page_title": value.get("pageTitle").cloned().unwrap_or(Value::Null),
                    "page_text": value.get("pageText").cloned().unwrap_or(Value::Null),
                    "performance_urls": value.get("apiUrls").cloned().unwrap_or_else(|| json!([])),
                    "all_urls": value.get("allUrls").cloned().unwrap_or_else(|| json!([])),
                    "inner_text_length": value.get("innerTextLength").cloned().unwrap_or(Value::Null),
                    "inner_text_head": value.get("innerTextHead").cloned().unwrap_or(Value::Null),
                    "deviceish_keys": value.get("deviceishKeys").cloned().unwrap_or_else(|| json!([])),
                    "site_request_count": value.get("siteRequestCount").cloned().unwrap_or(Value::Null),
                    "token_candidates": token_candidates,
                    "localStorage_keys": local_storage,
                    "cookie_names": cookies,
                    "attempts": attempts,
                    "note": "token 的值**一步都没出页**（只报长度）；不报标题／会话 id",
                })
            }
            Err(e) => json!({ "ok": false, "error": e.to_string() }),
        };

        // ── 结论：哪种形态 200 且会话目录非空 ──────────────────────────────
        if let Some(attempts) = report["s2_contract"]["attempts"].as_array().cloned() {
            let winners: Vec<Value> = attempts
                .iter()
                .filter(|attempt| {
                    attempt.get("status").and_then(Value::as_i64) == Some(200)
                        && attempt
                            .get("sessionsLen")
                            .and_then(Value::as_i64)
                            .map(|count| count > 0)
                            .unwrap_or(false)
                })
                .cloned()
                .collect();
            report["s3_verdict"] = json!({
                "working_shapes": winners,
                "count": winners.len(),
                "note": "working_shapes 里那些 label ＝ 拿到**非空目录**的形态",
            });
        }

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
