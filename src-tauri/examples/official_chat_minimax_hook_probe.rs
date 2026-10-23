//! MiniMax「官网引用」路线 B 的**页内取数**探针（macOS 真机，只读测量）。
//!
//! # 它回答什么
//!
//! 2026-10-23 已实测确定（**本探针不再重复测那几格**）：
//!
//! · MiniMax 的 API 请求**必须带签名**（站点自己那一发的头名是
//!   `Content-Type, token, yy, x-timestamp, x-signature`），
//! · **签名复刻不出来**：用新鲜登录态试了 102 种形态（URL 形态 × 认证头 9 种）
//!   ⇒ **全部 400 `invalid signature`** ⇒ **token 过期这条被排除**（token 是新的），
//! · **token 也在 URL query 里**（`…&token=<448 chars>&…`），
//! · ⇒ ⇒ **「自己拼一个请求」这条路在 MiniMax 上走不通**。
//!
//! 本探针只问剩下那一问：**能不能不签名，让站点自己发那一发，然后把它的
//! 响应体截下来？**
//!
//! · **Q1 目录**：页内挂钩（`fetch` ／ `XMLHttpRequest`）截获站点自己那一发
//!   `/minimax-cloud/api/v1/sidebar/session/tree` 的**响应体** ⇒ 报状态码、
//!   顶层键名、`$.sessions` 条数、条目键名、`has_more`／`next_cursor` 在不在。
//! · **Q2 正文**：把同一扇窗**导航到那条会话的深链**，让站点自己去拉消息 ⇒
//!   报状态码、顶层键名、`$.messages` 条数、消息键名、角色取值集合、
//!   `msg_content` 在不在、`has_more` 在不在。
//! · **Q3 重放**：把站点那一发的 **URL ＋ 完整头**（含 `x-signature` ／
//!   `x-timestamp` 的值，**在页内捕获**）原样重放，**立刻**／**+30 秒**／
//!   **+60 秒**各一次 ⇒ 判「签名是否**时间窗绑定**」。
//!
//! 另有一条**反向对照**：同源访问一个**不存在**的路径
//! （`/minimax-cloud/api/v1/__probe_control__`）**必须失败** ⇒ 否则前面的
//! 「ok」不能当证据（“同源随便发都能过” 与 “这一发带对了签名” 是两回事）。
//!
//! # 隔离（判据 `probe-profile-isolation`）
//!
//! 源是用户真实那份 WK 存储 ⇒ **只读拷贝**到探针自己的目录 ⇒ 探针写的全在
//! 拷贝里，用户正在跑的那份存储**一个字节都没改**。
//!
//! # 读数纪律
//!
//! **不报**：token 的值、签名的值、cookie 的值、**会话 id**、会话标题原文。
//! **报**：头名、值的**长度**、结构键名、条数、状态码。
//!
//! **会话 id 的处理**：深链要用真实 id ⇒ 但 id **全程留在页内**
//! （页内自己拼深链、自己导航）⇒ 宿主**一个字节的 id 都拿不到** ⇒
//! 读数里只有 `sessionIdLength`。
//!
//! 这条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块编译掉。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe: 本探针只支持 macOS（WKWebsiteDataStore 路线）。");
    std::process::exit(2);
}

/// **看门狗**：探针卡住时**自己**退出，不占着整轮等待。
#[cfg(target_os = "macos")]
const WATCHDOG_SECONDS: u64 = 420;

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

/// 探针自己的数据存储标识符（**不是**生产的任何一个）。
///
/// ⚠️ **纪律**：探针**绝不能**起在用户真实那份档案上（浏览器一启动就写
/// Cookies／LocalStorage）⇒ ⇒ 生产那两个标识符在探针里**一个都不许出现**。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-mhk-001";

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};

    /// 探针自己的数据存储标识符（根级那个）。
    use crate::PROBE_DATA_STORE_IDENTIFIER;

    /// **源存储目录**从哪个环境变量读（源码里不写路径字面量）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// MiniMax 的入口地址（生产规格表里那个 `root_url`；会 301 到
    /// `agent.minimax.cn`）。
    const SITE_URL: &str = "https://agent.minimaxi.com";

    /// 目录端点（生产规格表里那个 `catalog_path`）。
    const CATALOG_PATH: &str = "/minimax-cloud/api/v1/sidebar/session/tree";

    /// 反向对照用的**不存在**的路径。
    const CONTROL_PATH: &str = "/minimax-cloud/api/v1/__probe_control__";

    /// 深链前缀（生产规格表里那个 `deep_link_prefix`）。
    const DEEP_LINK_PREFIX: &str = "https://agent.minimax.cn/?id=";

    /// **页内挂钩脚本**（在页面脚本之前注入，**每个文档都跑一遍**）。
    ///
    /// ## 它做什么
    ///
    /// 1. 猴补 `window.fetch` 与 `XMLHttpRequest` ⇒ 记下站点自己那一发的
    ///    **URL（打码）**、方法、**头名 ＋ 头值长度**；
    /// 2. **响应体捕获**：`fetch` 用 `res.clone().text()`（克隆，不动站点自己
    ///    那份流）；`XMLHttpRequest` 用 `load` 事件读 `responseText`。
    ///    读不到（流式／`responseType` 不是文本）就记「body 不可得」，**不挂死**
    ///    （8 秒兜底）。
    /// 3. **重放**：第一次抓到目录请求时，把 **URL ＋ 完整头**（**值留在页内**）
    ///    留在页内，并按 **0 / 30 / 60 秒**三个时刻各重放一次 ⇒ 判签名是不是
    ///    **时间窗绑定**。
    /// 4. **反向对照**：同源打一个**不存在**的路径 ⇒ 必须失败。
    ///
    /// ## 为什么重放要留在页内
    ///
    /// `x-signature` / `x-timestamp` 的**值**一步都不出页面 ⇒ 宿主拿到的只是
    /// 「状态码 ＋ 结构」⇒ 凭据零落盘这条纪律在探针里也成立。
    ///
    /// ## 为什么用 `setInterval` 补挂
    ///
    /// 站点自己可能**覆盖** `fetch` ⇒ 只在最开始挂一次会漏掉它后来的请求。
    const HOOK_SCRIPT: &str = r##"
(function () {
  // 只在顶层 frame 干活。
  try { if (window.top !== window.self) return; } catch (e) { return; }
  if (window.__mmxHookInstalled) return;
  window.__mmxHookInstalled = true;

  // ---- 两份记录：REC 可读（能进读数），SEC 有秘密（**永不进读数**）--------
  var REC = {
    docInstalledAt: Date.now(),
    requests: [],
    responses: [],
    replays: {},
    control: null,
    controlNoSignature: null,
    controlSignatureOnOtherUrl: null,
    controlSignatureChangedQuery: null,
    controlSignatureExtraQuery: null,
    controlStarted: false,
    suppress: false
  };
  var SEC = { catalog: null, message: null, rawCatalogBody: null, deepLink: null, sessionIdLength: 0 };

  function put(slot, value) {
    try {
      Object.defineProperty(window, slot, {
        value: value, writable: true, configurable: true, enumerable: false
      });
    } catch (e) { try { window[slot] = value; } catch (e2) {} }
  }
  put('__mmxHook', REC);
  put('__mmxSecret', SEC);

  // ---- 打码：凭据 / 会话 id 的**值**一律不进读数 ---------------------------
  function maskUrl(u) {
    var s = String(u == null ? '' : u);
    try {
      s = s.replace(/([?&](?:token|id|uuid|device_id|user_id|sign|signature|x-signature|x-timestamp|yy|agent_name|project_id)=)[^&]*/g, '$1<REDACTED>');
      // ⚠️ **会话 id 在路径里**（`/api/v1/session/<id>`、`/api/v1/session/<id>/message`、
      //    `/queue`、`/input-summaries` …）⇒ ⇒ 一律打码。
      //    只打 `/api/v1/session/` 那一档 ⇒ `/api/v1/sidebar/session/tree` 不受影响。
      s = s.replace(/(\/api\/v1\/session\/)[^/?#]+/g, '$1<REDACTED>');
    } catch (e) {}
    return s;
  }

  // ---- 端点判定 ------------------------------------------------------------
  function isApi(u) { return /\/minimax-cloud\/api\//.test(String(u || '')); }
  function isCatalog(u) { return /\/minimax-cloud\/api\/v1\/sidebar\/session\/tree/.test(String(u || '')); }
  function isMessage(u) {
    var s = String(u || '');
    if (/\/minimax-cloud\/api\/v1\/session\/[^/?#]+\/message/.test(s)) return true;
    return /\/minimax-cloud\/api\//.test(s) && /message/i.test(s);
  }

  // ---- 头：名字 + 长度（**值不进读数**，但**留在页内**给重放用）-------------
  function headerPairsOf(input, init) {
    var pairs = [];
    try {
      var h = null;
      try { if (init && init.headers) h = init.headers; } catch (e) {}
      try { if (!h && input && input.headers) h = input.headers; } catch (e2) {}
      if (!h) return pairs;
      if (typeof h.forEach === 'function') {
        h.forEach(function (v, k) { pairs.push([String(k), String(v)]); });
      } else if (Array.isArray(h)) {
        for (var i = 0; i < h.length; i++) {
          try { if (h[i] && h[i].length >= 2) pairs.push([String(h[i][0]), String(h[i][1])]); } catch (e) {}
        }
      } else {
        var ks = Object.keys(h);
        for (var j = 0; j < ks.length; j++) {
          try { pairs.push([String(ks[j]), String(h[ks[j]])]); } catch (e3) {}
        }
      }
    } catch (e) {}
    return pairs;
  }

  function headerShape(pairs) {
    var names = [];
    var lengths = {};
    for (var i = 0; i < pairs.length; i++) {
      var n = String(pairs[i][0]);
      names.push(n);
      lengths[n.toLowerCase()] = String(pairs[i][1]).length;
    }
    return { names: names, lengths: lengths };
  }

  function toObject(pairs) {
    var o = {};
    for (var i = 0; i < pairs.length; i++) {
      try { o[String(pairs[i][0])] = String(pairs[i][1]); } catch (e) {}
    }
    return o;
  }

  // ---- 响应体 → **只报结构**（状态码 / 业务码 / 顶层键名 / 条数 / 键名）----
  //    标题原文、会话 id、消息正文**一律不进读数**（只报长度）。
  function summarize(status, text) {
    var s = { status: status, bodyLength: String(text == null ? '' : text).length };
    try {
      var j = JSON.parse(text);
      s.json = true;
      s.topKeys = Object.keys(j == null ? {} : j).slice(0, 24);
      if (j && j.base_resp && typeof j.base_resp === 'object') {
        s.baseRespKeys = Object.keys(j.base_resp).slice(0, 10);
        if (typeof j.base_resp.status_code !== 'undefined') s.bizCode = j.base_resp.status_code;
        if (typeof j.base_resp.status_msg === 'string') s.bizMsg = String(j.base_resp.status_msg).slice(0, 120);
      }
      s.hasMoreKey = !!(j && Object.prototype.hasOwnProperty.call(j, 'has_more'));
      s.hasMore = (j && typeof j.has_more !== 'undefined') ? j.has_more : null;
      s.hasNextCursorKey = !!(j && Object.prototype.hasOwnProperty.call(j, 'next_cursor'));
      s.nextCursorType = (j && typeof j.next_cursor !== 'undefined')
        ? (j.next_cursor === null ? 'null' : typeof j.next_cursor) : 'absent';
      if (j && Array.isArray(j.sessions)) {
        s.sessionsCount = j.sessions.length;
        if (j.sessions.length) {
          var el = j.sessions[0] || {};
          s.firstElementKeys = Object.keys(el).slice(0, 20);
          if (el.session && typeof el.session === 'object') {
            s.firstSessionKeys = Object.keys(el.session).slice(0, 32);
          }
        }
      }
      if (j && Array.isArray(j.messages)) {
        s.messagesCount = j.messages.length;
        if (j.messages.length) {
          s.firstMessageKeys = Object.keys(j.messages[0] || {}).slice(0, 32);
          var roles = {};
          for (var i = 0; i < j.messages.length; i++) {
            var m = j.messages[i] || {};
            var r = m.role;
            var key = (typeof r === 'undefined' || r === null) ? '<absent>' : String(r);
            roles[key] = (roles[key] || 0) + 1;
          }
          s.roleCounts = roles;
          s.hasMsgContent = j.messages.some(function (m) { return !!(m && typeof m.msg_content !== 'undefined'); });
          s.hasThinkingContent = j.messages.some(function (m) { return !!(m && typeof m.thinking_content !== 'undefined'); });
          s.msgContentLengths = j.messages.slice(0, 20).map(function (m) {
            return (m && typeof m.msg_content === 'string') ? m.msg_content.length : -1;
          });
        }
      }
      if (j && typeof j.error !== 'undefined') s.errorText = String(j.error).slice(0, 120);
    } catch (e) {
      // ⚠️ **不报原文**（可能是标题／正文）⇒ 只报「不是 JSON」＋长度。
      s.json = false;
    }
    return s;
  }

  // ---- 读响应体（带 8 秒兜底，读不到就记「不可得」，**不挂死**）-------------
  function readBody(res) {
    return new Promise(function (resolve) {
      var done = false;
      var timer = null;
      function fin(v) { if (done) return; done = true; try { clearTimeout(timer); } catch (e) {} resolve(v); }
      timer = setTimeout(function () { fin({ error: 'body 读超时（8 秒）' }); }, 8000);
      try {
        res.text().then(
          function (t) { fin({ text: String(t) }); },
          function (e) { fin({ error: 'body 读不到：' + String(e).slice(0, 100) }); }
        );
      } catch (e) {
        fin({ error: 'body 读不到：' + String(e).slice(0, 100) });
      }
    });
  }

  // ---- 重放：URL ＋ 完整头（**值在页内**）原样再发一次 ----------------------
  function replayAt(label, delayMs) {
    setTimeout(function () {
      var sec = SEC.catalog;
      if (!sec) {
        REC.replays[label] = { state: 'done', error: '没抓到可重放的目录请求', delayMs: delayMs };
        return;
      }
      var entry = { state: 'running', delayMs: delayMs, at: Date.now() };
      if (REC.catalogCapturedAt) entry.deltaMs = Date.now() - REC.catalogCapturedAt;
      REC.replays[label] = entry;
      var p;
      try {
        // 重放这一发**自己**不能再被当成「站点那一发」记一遍。
        REC.suppress = true;
        p = fetch(sec.url, {
          method: sec.method || 'GET',
          headers: sec.headersObj,
          credentials: 'include'
        });
        REC.suppress = false;
      } catch (e) {
        REC.suppress = false;
        REC.replays[label] = { state: 'done', error: '发起重放失败：' + String(e).slice(0, 120), delayMs: delayMs };
        return;
      }
      p.then(function (res) {
        return readBody(res).then(function (r) {
          var summary = r.error ? { status: res.status, bodyUnavailable: r.error } : summarize(res.status, r.text);
          REC.replays[label] = {
            state: 'done', delayMs: delayMs, at: Date.now(),
            deltaMs: REC.catalogCapturedAt ? Date.now() - REC.catalogCapturedAt : null,
            summary: summary, status: res.status
          };
          return null;
        });
      }).catch(function (e) {
        REC.replays[label] = {
          state: 'done', delayMs: delayMs, at: Date.now(),
          error: String(e).slice(0, 160)
        };
      });
    }, delayMs);
  }

  // ---- 反向对照：同源打一个**不存在**的路径 ⇒ 必须失败 --------------------
  function runControl() {
    if (REC.controlStarted) return;
    REC.controlStarted = true;
    REC.control = { state: 'running', path: '__CONTROL_PATH__' };
    var p;
    try {
      REC.suppress = true;
      p = fetch('__CONTROL_PATH__', { credentials: 'include' });
      REC.suppress = false;
    } catch (e) {
      REC.suppress = false;
      REC.control = { state: 'done', path: '__CONTROL_PATH__', error: String(e).slice(0, 160) };
      return;
    }
    p.then(function (res) {
      return readBody(res).then(function (r) {
        REC.control = {
          state: 'done', path: '__CONTROL_PATH__',
          summary: r.error ? { status: res.status, bodyUnavailable: r.error } : summarize(res.status, r.text)
        };
        return null;
      });
    }).catch(function (e) {
      REC.control = { state: 'done', path: '__CONTROL_PATH__', error: String(e).slice(0, 160) };
    });
  }

  // ---- 反向对照之二：**同一路径**，但**去掉签名头** ⇒ 必须失败 -------------
  //    （这才是「签名是必需的」的直接 A/B：同一条 URL、同一份 token，
  //      只把 x-signature / x-timestamp / yy 摘掉。）
  function runControlNoSignature() {
    var sec = SEC.catalog;
    if (!sec) {
      REC.controlNoSignature = { state: 'done', error: '没抓到目录请求' };
      return;
    }
    var headers = {};
    var kept = [];
    for (var i = 0; i < sec.pairs.length; i++) {
      var name = String(sec.pairs[i][0]);
      var lower = name.toLowerCase();
      if (lower === 'x-signature' || lower === 'x-timestamp' || lower === 'yy') continue;
      headers[name] = String(sec.pairs[i][1]);
      kept.push(name);
    }
    REC.controlNoSignature = {
      state: 'running',
      removedHeaders: ['x-signature', 'x-timestamp', 'yy'],
      keptHeaderNames: kept
    };
    var p;
    try {
      REC.suppress = true;
      p = fetch(sec.url, {
        method: sec.method || 'GET', headers: headers, credentials: 'include'
      });
      REC.suppress = false;
    } catch (e) {
      REC.suppress = false;
      REC.controlNoSignature = { state: 'done', error: String(e).slice(0, 160) };
      return;
    }
    p.then(function (res) {
      return readBody(res).then(function (r) {
        REC.controlNoSignature = {
          state: 'done',
          removedHeaders: ['x-signature', 'x-timestamp', 'yy'],
          keptHeaderNames: kept,
          summary: r.error ? { status: res.status, bodyUnavailable: r.error } : summarize(res.status, r.text)
        };
        return null;
      });
    }).catch(function (e) {
      REC.controlNoSignature = { state: 'done', error: String(e).slice(0, 160) };
    });
  }

  // ---- 反向对照之三：**同一份签名**，打到**另一个路径** --------------------
//    ⇒ 判「签名绑不绑 URL」：
//    · 签名**含 URL** ⇒ 换路径 ⇒ 400 invalid signature；
//    · 签名**只含 token+时间戳** ⇒ 换路径 ⇒ 200
//    （这决定「重放签名请求」能不能当独立备选路：绑 URL ⇒ 只能取站点自己
//      打过的那几条；不绑 ⇒ 截一次签名就能打任意端点。）
  function runControlSignatureOnOtherUrl() {
    var sec = SEC.catalog;
    if (!sec) {
      REC.controlSignatureOnOtherUrl = { state: 'done', error: '没抓到目录请求' };
      return;
    }
    // 站点自己也会打 `/minimax-cloud/api/v1/config` ⇒ 换一个**存在**的路径。
    var otherUrl = String(sec.url).replace('/sidebar/session/tree', '/config');
    if (otherUrl === String(sec.url)) {
      REC.controlSignatureOnOtherUrl = { state: 'done', error: '没能换掉路径' };
      return;
    }
    REC.controlSignatureOnOtherUrl = {
      state: 'running',
      note: '同一份头（含 x-signature），URL 的路径换成 /minimax-cloud/api/v1/config'
    };
    var p;
    try {
      REC.suppress = true;
      p = fetch(otherUrl, {
        method: sec.method || 'GET', headers: sec.headersObj, credentials: 'include'
      });
      REC.suppress = false;
    } catch (e) {
      REC.suppress = false;
      REC.controlSignatureOnOtherUrl = { state: 'done', error: String(e).slice(0, 160) };
      return;
    }
    p.then(function (res) {
      return readBody(res).then(function (r) {
        REC.controlSignatureOnOtherUrl = {
          state: 'done',
          note: '同一份头（含 x-signature），URL 的路径换成 /minimax-cloud/api/v1/config',
          summary: r.error ? { status: res.status, bodyUnavailable: r.error } : summarize(res.status, r.text)
        };
        return null;
      });
    }).catch(function (e) {
      REC.controlSignatureOnOtherUrl = { state: 'done', error: String(e).slice(0, 160) };
    });
  }

  // ---- 反向对照之四／五：**同一份签名**，但**改 query** --------------------
//    ⇒ 判「签名绑不绑 query」：
//    · 改 query 仍 200 ⇒ 壳可以**自己加游标参数**（正文那条是分页的，
//      `before` 游标、翻页、改 `limit` 都才可行）；
//    · 改 query 就 400 ⇒ 签名含 query ⇒ 只能原样重放站点自己那一发。
  function queryVariant(baseUrl, mode) {
    if (mode === 'change') {
      var changed = String(baseUrl).replace(/([?&])limit=\d+/, '$1limit=5');
      return changed === String(baseUrl) ? null : changed;
    }
    if (mode === 'extra') {
      return String(baseUrl) + (String(baseUrl).indexOf('?') >= 0 ? '&' : '?') + '__probe_extra__=1';
    }
    return null;
  }

  function runControlSignatureWithQuery(mode, slot) {
    var sec = SEC.catalog;
    if (!sec) {
      REC[slot] = { state: 'done', error: '没抓到目录请求' };
      return;
    }
    var variant = queryVariant(sec.url, mode);
    if (!variant) {
      REC[slot] = { state: 'done', error: '没能改出 query 变体' };
      return;
    }
    REC[slot] = { state: 'running', mode: mode, note: '同一份头（含 x-signature），query 换了一种写法' };
    var p;
    try {
      REC.suppress = true;
      p = fetch(variant, {
        method: sec.method || 'GET', headers: sec.headersObj, credentials: 'include'
      });
      REC.suppress = false;
    } catch (e) {
      REC.suppress = false;
      REC[slot] = { state: 'done', mode: mode, error: String(e).slice(0, 160) };
      return;
    }
    p.then(function (res) {
      return readBody(res).then(function (r) {
        REC[slot] = {
          state: 'done', mode: mode,
          note: '同一份头（含 x-signature），query 换了一种写法',
          summary: r.error ? { status: res.status, bodyUnavailable: r.error } : summarize(res.status, r.text)
        };
        return null;
      });
    }).catch(function (e) {
      REC[slot] = { state: 'done', mode: mode, error: String(e).slice(0, 160) };
    });
  }

  // ---- fetch 挂钩 ----------------------------------------------------------
  function hookFetch() {
    try {
      if (!window.fetch || window.fetch.__mmxHooked) return;
      var origFetch = window.fetch;
      var wrapped = function (input, init) {
        var url = '';
        try { url = (typeof input === 'string') ? input : ((input && input.url) || ''); } catch (e) {}
        var capture = isApi(url) && !REC.suppress;
        var pairs = capture ? headerPairsOf(input, init) : [];
        var method = 'GET';
        try { method = String((init && init.method) || (input && input.method) || 'GET'); } catch (e2) {}
        if (capture) {
          REC.requests.push({
            transport: 'fetch', url: maskUrl(url), method: method,
            headerShape: headerShape(pairs), at: Date.now()
          });
          if (isCatalog(url) && !SEC.catalog) {
            SEC.catalog = { url: String(url), method: method, pairs: pairs, headersObj: toObject(pairs) };
            REC.catalogCapturedAt = Date.now();
            REC.catalogCapturedTransport = 'fetch';
            // 三个重放时刻：立刻 / +30 秒 / +60 秒。
            replayAt('immediate', 0);
            replayAt('plus30s', 30000);
            replayAt('plus60s', 60000);
            runControl();
            runControlNoSignature();
            runControlSignatureOnOtherUrl();
            runControlSignatureWithQuery('change', 'controlSignatureChangedQuery');
            runControlSignatureWithQuery('extra', 'controlSignatureExtraQuery');
          }
          if (isMessage(url)) {
            SEC.message = { url: String(url), method: method, pairs: pairs, headersObj: toObject(pairs) };
          }
        }
        var res;
        try { res = origFetch.apply(this, arguments); }
        catch (e) { return Promise.reject(e); }
        if (!capture) return res;
        // ⚠️ **`fetch` 返回的是 Promise，不是 Response**（r1 实测踩过：
        //    对 Promise 调 `.clone()` ⇒ TypeError ⇒ 响应体一条都没截到）。
        //    ⇒ 响应体要挂在 **`.then`** 上取，且**必须克隆**
        //    （不消费站点自己那份流）。
        try {
          res.then(function (response) {
            try {
              var cloned = response.clone();
              readBody(cloned).then(function (r) {
                var summary = r.error
                  ? { status: response.status, bodyUnavailable: r.error }
                  : summarize(response.status, r.text);
                REC.responses.push({ transport: 'fetch', url: maskUrl(url), at: Date.now(), summary: summary });
                // 目录那条的**原始响应体**留在页内（深链要用会话 id）。
                try { if (isCatalog(url) && !SEC.rawCatalogBody) SEC.rawCatalogBody = r.text || ''; } catch (e) {}
              }).catch(function (e) {
                REC.responses.push({
                  transport: 'fetch', url: maskUrl(url),
                  summary: { status: response.status, bodyUnavailable: '读体失败：' + String(e).slice(0, 100) }
                });
              });
            } catch (e) {
              REC.responses.push({
                transport: 'fetch', url: maskUrl(url),
                summary: { status: (response && response.status) || 0, bodyUnavailable: 'clone 失败：' + String(e).slice(0, 100) }
              });
            }
          }, function (err) {
            REC.responses.push({
              transport: 'fetch', url: maskUrl(url),
              summary: { status: 0, bodyUnavailable: '请求失败：' + String(err).slice(0, 100) }
            });
          });
        } catch (e) {}
        return res;
      };
      try { wrapped.__mmxHooked = true; } catch (e) {}
      window.fetch = wrapped;
    } catch (e) {}
  }

  // ---- XHR 挂钩 -----------------------------------------------------------
  function hookXhr() {
    try {
      if (!XMLHttpRequest || XMLHttpRequest.prototype.open.__mmxHooked) return;
      var XO = XMLHttpRequest.prototype.open;
      var XS = XMLHttpRequest.prototype.setRequestHeader;
      var XSend = XMLHttpRequest.prototype.send;

      var wrappedOpen = function (method, url) {
        try {
          this.__mmxMethod = String(method);
          this.__mmxUrl = String(url);
          this.__mmxPairs = [];
        } catch (e) {}
        return XO.apply(this, arguments);
      };
      try { wrappedOpen.__mmxHooked = true; } catch (e) {}
      XMLHttpRequest.prototype.open = wrappedOpen;

      var wrappedSet = function (name, value) {
        try { this.__mmxPairs.push([String(name), String(value)]); } catch (e) {}
        return XS.apply(this, arguments);
      };
      XMLHttpRequest.prototype.setRequestHeader = wrappedSet;

      var wrappedSend = function () {
        try {
          var self = this;
          var url = self.__mmxUrl || '';
          if (isApi(url) && !REC.suppress) {
            var pairs = self.__mmxPairs || [];
            var method = self.__mmxMethod || 'GET';
            REC.requests.push({
              transport: 'xhr', url: maskUrl(url), method: method,
              headerShape: headerShape(pairs), at: Date.now()
            });
            if (isCatalog(url) && !SEC.catalog) {
              SEC.catalog = { url: String(url), method: method, pairs: pairs, headersObj: toObject(pairs) };
              REC.catalogCapturedAt = Date.now();
              REC.catalogCapturedTransport = 'xhr';
              replayAt('immediate', 0);
              replayAt('plus30s', 30000);
              replayAt('plus60s', 60000);
              runControl();
              runControlNoSignature();
              runControlSignatureOnOtherUrl();
              runControlSignatureWithQuery('change', 'controlSignatureChangedQuery');
              runControlSignatureWithQuery('extra', 'controlSignatureExtraQuery');
            }
            if (isMessage(url)) {
              SEC.message = { url: String(url), method: method, pairs: pairs, headersObj: toObject(pairs) };
            }
            try {
              this.addEventListener('load', function () {
                try {
                  var text = null;
                  var err = null;
                  try { text = self.responseText; }
                  catch (e) {
                    err = 'responseText 不可读（responseType=' + self.responseType + '）';
                    try { text = JSON.stringify(self.response); } catch (e2) {}
                  }
                  var summary = err ? { status: self.status, bodyUnavailable: err } : summarize(self.status, text);
                  REC.responses.push({ transport: 'xhr', url: maskUrl(url), at: Date.now(), summary: summary });
                  try { if (isCatalog(url) && !SEC.rawCatalogBody) SEC.rawCatalogBody = text || ''; } catch (e3) {}
                } catch (e4) {
                  REC.responses.push({
                    transport: 'xhr', url: maskUrl(url),
                    summary: { status: 0, bodyUnavailable: String(e4).slice(0, 120) }
                  });
                }
              });
            } catch (e5) {}
          }
        } catch (e) {}
        return XSend.apply(this, arguments);
      };
      XMLHttpRequest.prototype.send = wrappedSend;
    } catch (e) {}
  }

  function hookAll() { hookFetch(); hookXhr(); }
  hookAll();
  // 站点自己可能**覆盖** fetch ⇒ 定时补挂（抓到它后来的请求）。
  try { setInterval(hookAll, 50); } catch (e) {}
  // 反向对照**无条件**先跑一次（目录请求抓不到时也有一档读数）。
  runControl();

  // ===================== 页内工具（宿主通过求值调用）=======================

  /** 拼深链（**id 不出页**）⇒ 返回的只是「长度」这类可报的东西。 */
  window.__mmxPrepareDeepLink = function () {
    try {
      var text = SEC.rawCatalogBody;
      if (!text) return { ok: false, error: '没有截到目录响应体' };
      var j = JSON.parse(text);
      var arr = (j && Array.isArray(j.sessions)) ? j.sessions : [];
      if (!arr.length) return { ok: false, error: '目录里没有会话条目' };
      var el = arr[0] || {};
      var s = (el.session && typeof el.session === 'object') ? el.session : el;
      var id = s.session_id || s.id || el.session_id || el.id || '';
      if (!id) {
        return { ok: false, error: '第一条会话里找不到 id 字段', availableKeys: Object.keys(s).slice(0, 32) };
      }
      SEC.deepLink = '__DEEP_LINK__' + encodeURIComponent(String(id));
      SEC.sessionIdLength = String(id).length;
      return {
        ok: true,
        sessionIdLength: String(id).length,
        deepLinkHost: 'agent.minimax.cn',
        deepLinkPrefix: '__DEEP_LINK__',
        idSource: '$.sessions[0].session.' + (s.session_id ? 'session_id' : (s.id ? 'id' : '?')),
        candidateSessionKeys: Object.keys(s).slice(0, 32)
      };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  };

  /** 导航到那条会话的深链（**id 仍在页内**）。 */
  window.__mmxGoDeepLink = function () {
    try {
      if (!SEC.deepLink) return { ok: false, error: '还没准备深链' };
      location.href = SEC.deepLink;
      return { ok: true };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  };

  /** 触发站点自己的行为（滚到底 ＋ 点会话入口）——Q2 的第二种试法。 */
  window.__mmxTryTrigger = function () {
    var out = { scrolled: false, clicked: false, hrefCount: 0, linkCandidates: [] };
    try { window.scrollTo(0, (document.body && document.body.scrollHeight) || 0); out.scrolled = true; } catch (e) {}
    try {
      var links = document.querySelectorAll('a[href*="id="]');
      out.hrefCount = links.length;
      for (var i = 0; i < links.length && i < 8; i++) {
        try { out.linkCandidates.push(maskUrl(links[i].getAttribute('href'))); } catch (e) {}
      }
      for (var j = 0; j < links.length && j < 8; j++) {
        try { links[j].click(); out.clicked = true; break; } catch (e2) {}
      }
    } catch (e3) {}
    return out;
  };

  /** window 上**可发现的全局函数名**（只报名字，不报值）。 */
  window.__mmxDiscoverGlobals = function () {
    var out = [];
    try {
      var names = Object.getOwnPropertyNames(window);
      for (var i = 0; i < names.length && out.length < 80; i++) {
        var n = names[i];
        if (String(n).indexOf('__mmx') === 0) continue;
        if (String(n).charAt(0) === '_') continue;
        // 只报**站点自己**的全局函数：名字里带业务词，且**不是** DOM 内建物
        // （`NodeList` / `MessageEvent` 那一类后缀要排掉）。
        if (!/session|message|history|conversation|sidebar|chat|tree|agent/i.test(n)) continue;
        if (/(List|Event|EventTarget|Options|Collection|Node|Entry|Worker|Provider)$/.test(n)) continue;
        var t = '';
        try { t = typeof window[n]; } catch (e) { t = 'error'; }
        if (t === 'function') out.push(String(n));
      }
    } catch (e) {}
    return out;
  };
})();
"##;

    /// 页内求值用的表达式。
    const HOOK_EXPR: &str = "window.__mmxHook";
    const PREPARE_DEEPLINK_EXPR: &str = "window.__mmxPrepareDeepLink()";
    const GO_DEEPLINK_EXPR: &str = "window.__mmxGoDeepLink()";
    const TRIGGER_EXPR: &str = "window.__mmxTryTrigger()";
    const GLOBALS_EXPR: &str = "window.__mmxDiscoverGlobals()";

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
        std::env::temp_dir().join(format!("dsh-minimax-hook-probe.{round}.log"))
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
                "note": "源是**只读拷贝**：探针写的全在拷贝里，用户正在跑的那份存储一个字节都没改",
            }),
        ))
    }

    /// **这一轮要不要起隐藏窗**（`DSH_PROBE_HIDDEN=1` ⇒ 隐藏）。
    ///
    /// ⚠️ **为什么要有这一档**：生产的 macOS 取数窗是 `.visible(false)`
    /// ⇒ ⇒ 「隐藏窗里站点自己发不发那一发」**直接决定** MiniMax 路线 B 在生产里
    /// 成不成立 ⇒ ⇒ 得能在这个探针里**复现生产的窗形态**来测。
    fn hidden_window() -> bool {
        std::env::var("DSH_PROBE_HIDDEN")
            .map(|value| value == "1")
            .unwrap_or(false)
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
        let profile = std::env::temp_dir().join("dsh-official-chat-minimax-hook-probe");
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("无法创建临时 profile 目录（{}）：{e}", profile.display()))?;

        // 挂钩脚本里的路径是**占位符**（源码里不写站点路径字面量 ⇒ 换端点只改
        // 常量，不用改 JS）。
        let hook_script = HOOK_SCRIPT
            .replace("__CONTROL_PATH__", CONTROL_PATH)
            .replace("__DEEP_LINK__", DEEP_LINK_PREFIX);

        let builder =
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(url.clone()))
                .title("dsh MiniMax 页内挂钩探针")
                .inner_size(1000.0, 760.0)
                // ⚠️ **默认必须可见**：隐藏窗里那个 SPA **不跑**（r5 实测：站点自己
                // 一次 API 都没打）⇒ ⇒ 可见窗让站点**自己**发那一发。
                //
                // ⚠️ **`DSH_PROBE_HIDDEN=1`** ＝ **故意起隐藏窗** ⇒ ⇒ 用来验
                // 「**生产的隐藏取数窗**能不能驱动站点」那一格（生产侧
                // `build_capture_window` 是 `.visible(false)`）⇒ ⇒ 这一格直接决定
                // MiniMax 路线 B 在生产里成不成立。
                .visible(!hidden_window())
                .data_directory(profile)
                .additional_browser_args(crate::official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
                .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
                .initialization_script(&hook_script)
                .data_store_identifier(*identifier);

        log_line("probe", &format!("准备 build()（label={label}）"));
        let built = builder.build();
        log_line("probe", "build() 返回");
        built.map_err(|e| format!("无法创建探针窗口：{e}"))
    }

    /// 等页面就绪：`location.origin` **连续 3 次**不变才算稳（站点会 301）。
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
            .name("minimax-hook-watchdog".into())
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
                    .name("minimax-hook-probe".into())
                    .spawn(move || {
                        log_line("probe", "工作线程启动");
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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

    // ---- 读数工具 -----------------------------------------------------------

    /// 读回页内那份挂钩记录（**只含可报的东西**：URL 已打码、头只有名字与长度）。
    async fn read_hook(window: &tauri::WebviewWindow) -> Option<Value> {
        crate::script_eval::read_json(window, HOOK_EXPR, Duration::from_secs(3))
            .await
            .ok()
            .filter(|v| v.is_object())
    }

    fn url_of(entry: &Value) -> String {
        entry
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    /// 去重后的 URL 列表（挂钩被**套了两层**——站点自己也包了一层
    /// `fetch`／`setRequestHeader` ⇒ 同一发会被记两次）。
    fn unique_urls(entries: &[Value], limit: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for entry in entries {
            let url = url_of(entry);
            if url.is_empty() {
                continue;
            }
            if !out.contains(&url) {
                out.push(url);
            }
            if out.len() >= limit {
                break;
            }
        }
        out
    }

    /// 从挂钩记录里挑出**目录**那一发。
    fn extract_catalog(hook: &Option<Value>) -> Value {
        let Some(h) = hook else {
            return json!({ "captured": false, "error": "页内挂钩记录读不到" });
        };
        let requests = h
            .get("requests")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let responses = h
            .get("responses")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let req = requests
            .iter()
            .find(|r| url_of(r).contains("/sidebar/session/tree"));
        let resp = responses
            .iter()
            .rev()
            .find(|r| url_of(r).contains("/sidebar/session/tree"));
        json!({
            "captured": resp.is_some(),
            "doc_installed_at": h.get("docInstalledAt").cloned().unwrap_or(Value::Null),
            "catalog_captured_transport": h.get("catalogCapturedTransport").cloned().unwrap_or(Value::Null),
            "request": req.cloned().unwrap_or(Value::Null),
            // 整条响应条目（含 `at`）⇒ 能核对「站点自己那一发的响应」**早于**
            // 「重放那一发」（重放那一发走 `REC.suppress` ⇒ **不进** `responses`）。
            "response": resp.cloned().unwrap_or(Value::Null),
            "response_summary": resp
                .and_then(|r| r.get("summary").cloned())
                .unwrap_or(Value::Null),
            "api_request_urls": unique_urls(&requests, 25),
        })
    }

    /// 从挂钩记录里挑出**正文**那一发（`…/session/<id>/message`）。
    fn extract_body(hook: &Option<Value>) -> Value {
        let Some(h) = hook else {
            return json!({ "captured": false, "error": "页内挂钩记录读不到" });
        };
        let requests = h
            .get("requests")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let responses = h
            .get("responses")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let req = requests.iter().find(|r| url_of(r).contains("/message"));
        let resp = responses
            .iter()
            .rev()
            .find(|r| url_of(r).contains("/message"));
        json!({
            "captured": resp.is_some(),
            "doc_installed_at": h.get("docInstalledAt").cloned().unwrap_or(Value::Null),
            "request": req.cloned().unwrap_or(Value::Null),
            "response": resp.cloned().unwrap_or(Value::Null),
            "response_summary": resp
                .and_then(|r| r.get("summary").cloned())
                .unwrap_or(Value::Null),
            "api_request_urls": unique_urls(&requests, 30),
        })
    }

    /// 等**目录**那一发的**响应体**被抓到（不靠固定睡眠——r1 实测：目录请求
    /// 在 origin 稳定后 **十几秒**才发，固定睡 12 秒会读早）。
    async fn wait_for_catalog(window: &tauri::WebviewWindow, budget: Duration) -> Option<Value> {
        let deadline = Instant::now() + budget;
        let mut last: Option<Value> = None;
        while Instant::now() < deadline {
            if let Some(h) = read_hook(window).await {
                let hit = h
                    .get("responses")
                    .and_then(Value::as_array)
                    .map(|rs| {
                        rs.iter()
                            .any(|r| url_of(r).contains("/sidebar/session/tree"))
                    })
                    .unwrap_or(false);
                if hit {
                    return Some(h);
                }
                last = Some(h);
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        last
    }

    /// 等**反向对照**那一发跑完。
    async fn wait_for_control(
        window: &tauri::WebviewWindow,
        budget: Duration,
    ) -> (Value, Value, Value, Value, Value) {
        let deadline = Instant::now() + budget;
        let mut last = Value::Null;
        let mut last_nosig = Value::Null;
        let mut last_otherurl = Value::Null;
        let mut last_chgq = Value::Null;
        let mut last_extraq = Value::Null;
        while Instant::now() < deadline {
            if let Some(h) = read_hook(window).await {
                if let Some(c) = h.get("control") {
                    if c.get("state").and_then(Value::as_str) == Some("done") {
                        last = c.clone();
                    }
                }
                if let Some(c) = h.get("controlNoSignature") {
                    if c.get("state").and_then(Value::as_str) == Some("done") {
                        last_nosig = c.clone();
                    }
                }
                if let Some(c) = h.get("controlSignatureOnOtherUrl") {
                    if c.get("state").and_then(Value::as_str) == Some("done") {
                        last_otherurl = c.clone();
                    }
                }
                if let Some(c) = h.get("controlSignatureChangedQuery") {
                    if c.get("state").and_then(Value::as_str) == Some("done") {
                        last_chgq = c.clone();
                    }
                }
                if let Some(c) = h.get("controlSignatureExtraQuery") {
                    if c.get("state").and_then(Value::as_str) == Some("done") {
                        last_extraq = c.clone();
                    }
                }
                if !last.is_null()
                    && !last_nosig.is_null()
                    && !last_otherurl.is_null()
                    && !last_chgq.is_null()
                    && !last_extraq.is_null()
                {
                    return (last, last_nosig, last_otherurl, last_chgq, last_extraq);
                }
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        (last, last_nosig, last_otherurl, last_chgq, last_extraq)
    }

    /// 等三个重放都跑完（或者目录请求压根没抓到）。
    async fn wait_for_replays(
        window: &tauri::WebviewWindow,
        budget: Duration,
    ) -> (Value, Option<Value>) {
        let deadline = Instant::now() + budget;
        let mut last: Value = Value::Null;
        let mut last_hook: Option<Value> = None;
        while Instant::now() < deadline {
            if let Some(h) = read_hook(window).await {
                last_hook = Some(h.clone());
                let replays = h.get("replays").cloned().unwrap_or(Value::Null);
                let done = replays
                    .as_object()
                    .map(|m| {
                        m.len() >= 3
                            && m.values()
                                .filter(|v| v.get("state").and_then(Value::as_str) == Some("done"))
                                .count()
                                >= 3
                    })
                    .unwrap_or(false);
                if done {
                    return (json!({ "replays": replays }), last_hook);
                }
                last = replays.clone();
                // 目录请求压根没抓到 ⇒ 不会有重放 ⇒ 不必空等。
                let got_catalog = h
                    .get("requests")
                    .and_then(Value::as_array)
                    .map(|rs| {
                        rs.iter()
                            .any(|r| url_of(r).contains("/sidebar/session/tree"))
                    })
                    .unwrap_or(false);
                if !got_catalog && replays.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                    let elapsed = deadline
                        .saturating_duration_since(Instant::now())
                        .saturating_sub(budget);
                    if elapsed > Duration::from_secs(25) {
                        return (
                            json!({
                                "note": "站点自己那一发目录请求始终没被抓到 ⇒ 没有可重放的对象（提前结束等待）",
                                "replays": last
                            }),
                            last_hook,
                        );
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        (
            json!({
                "note": "等待重放超时（预算耗尽）",
                "replays": last
            }),
            last_hook,
        )
    }

    /// 等**正文**那一发被抓到（带预算）。
    async fn wait_for_message(
        window: &tauri::WebviewWindow,
        budget: Duration,
    ) -> (Value, Option<Value>) {
        let deadline = Instant::now() + budget;
        let mut last: Option<Value> = None;
        while Instant::now() < deadline {
            if let Some(h) = read_hook(window).await {
                let responses = h
                    .get("responses")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if responses.iter().any(|r| url_of(r).contains("/message")) {
                    return (h.clone(), Some(h));
                }
                last = Some(h);
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        (last.clone().unwrap_or(Value::Null), last)
    }

    // ---- 主体 ---------------------------------------------------------------

    /// 三格：**目录（Q1）** → **重放（Q3）＋对照** → **正文（Q2）**。
    async fn run_probe_body(app: &tauri::AppHandle, url: &url::Url, store_reading: Value) -> Value {
        let mut report = json!({
            "site": "minimax",
            "probe": "official_chat_minimax_hook_probe",
            "round": std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string()),
            "site_url": SITE_URL,
            "catalog_path": CATALOG_PATH,
            "control_path": CONTROL_PATH,
            "store": store_reading,
            "discipline": "凭据（token/签名/cookie）与会话 id 全程留在页内；读数只有头名、长度、结构键名、条数、状态码",
            "notes": [
                "重放那一发走 REC.suppress ⇒ **不进** requests/responses ⇒ Q1/Q2 的响应条目必然来自**站点自己那一发**",
                "站点自己也包了一层 fetch／setRequestHeader ⇒ 同一发会被记两次（URL 列表已去重）",
                "x-timestamp 值长 10 ⇒ 秒级时间戳；x-signature / yy 值长各 32",
            ],
        });

        // ── S1：建窗（绑探针自己的存储）＋ 导航 ＋ 等就绪 ──────────────────
        let window =
            match build_window(app, "minimax-hook-probe", url, &PROBE_DATA_STORE_IDENTIFIER) {
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

        // ⚠️ **不能靠固定睡眠**：r1 实测——origin 稳定后 **十几秒**站点才发
        // 目录那一发（固定睡 12 秒 ⇒ 读早 ⇒ 读数是空的）。
        // ⇒ 这里**等它真的出现**。
        let phase_a = wait_for_catalog(&window, Duration::from_secs(75)).await;
        report["q1_catalog"] = extract_catalog(&phase_a);
        let catalog_captured = report["q1_catalog"]["captured"].as_bool().unwrap_or(false);
        log_line("Q1", &format!("目录响应体截到了：{catalog_captured}"));

        // ── 反向对照：同源打一个**不存在**的路径 ⇒ 必须失败 ─────────────────
        let (control, control_nosig, control_otherurl, control_chgq, control_extraq) =
            wait_for_control(&window, Duration::from_secs(30)).await;
        report["control_probe"] = control;
        report["control_probe_no_signature"] = control_nosig;
        report["control_probe_signature_on_other_url"] = control_otherurl;
        report["control_probe_signature_changed_query"] = control_chgq;
        report["control_probe_signature_extra_query"] = control_extraq;

        // ── Q3：重放（立刻 / +30 秒 / +60 秒）───────────────────────────────
        let (replays, hook_after_replays) =
            wait_for_replays(&window, Duration::from_secs(110)).await;
        report["q3_replay"] = replays;
        // 重放后的记录里可能**又**多了几条响应（重放那一发自己）⇒ 一并带出。
        report["q3_all_requests"] = hook_after_replays
            .as_ref()
            .and_then(|h| h.get("requests").cloned())
            .unwrap_or(Value::Null);
        log_line("Q3", "重放三档读完");

        // ── Q2：正文 —— 把窗导航到那条会话的深链 ─────────────────────────────
        //    深链由**页内自己**拼（id 不出页）⇒ 宿主拿不到 id。
        let prep =
            crate::script_eval::round_trip(&window, PREPARE_DEEPLINK_EXPR, Duration::from_secs(15))
                .await;
        report["q2_prepare_deeplink"] = match &prep {
            Ok(trip) => trip.value.clone(),
            Err(e) => json!({ "error": e.to_string() }),
        };
        let prepared = report["q2_prepare_deeplink"]["ok"]
            .as_bool()
            .unwrap_or(false);
        log_line("Q2", &format!("深链准备：{prepared}"));

        if prepared {
            let go =
                crate::script_eval::round_trip(&window, GO_DEEPLINK_EXPR, Duration::from_secs(8))
                    .await;
            report["q2_navigate"] = match &go {
                Ok(trip) => trip.value.clone(),
                // 导航把文档换掉 ⇒ 回程读不到是**预期**的，不算失败。
                Err(e) => json!({
                    "note": "导航后回程读不到（预期：文档已被替换）",
                    "error": e.to_string()
                }),
            };
            // 等新文档加载完（初始化脚本会在新文档里重装挂钩）。
            tokio::time::sleep(Duration::from_secs(14)).await;
            let (body_hook, _) = wait_for_message(&window, Duration::from_secs(45)).await;
            report["q2_body"] = extract_body(&Some(body_hook.clone()));
            let body_captured = report["q2_body"]["captured"].as_bool().unwrap_or(false);
            log_line("Q2", &format!("正文响应体截到了：{body_captured}"));

            if !body_captured {
                // ② 第二种试法：触发站点自己的行为（滚到底 ＋ 点会话入口）。
                let trig =
                    crate::script_eval::round_trip(&window, TRIGGER_EXPR, Duration::from_secs(10))
                        .await;
                report["q2_fallback_trigger"] = match &trig {
                    Ok(trip) => trip.value.clone(),
                    Err(e) => json!({ "error": e.to_string() }),
                };
                let (body_hook2, _) = wait_for_message(&window, Duration::from_secs(30)).await;
                report["q2_body_after_trigger"] = extract_body(&Some(body_hook2.clone()));
            }
        } else {
            report["q2_body"] = json!({
                "captured": false,
                "error": "深链没准备成（页内没截到目录响应体，或目录里没有会话条目）"
            });
        }

        // ③ 第三种试法的前提材料：window 上**可发现的全局函数名**。
        let globals =
            crate::script_eval::round_trip(&window, GLOBALS_EXPR, Duration::from_secs(10)).await;
        report["q2_globals"] = match &globals {
            Ok(trip) => trip.value.clone(),
            Err(e) => json!({ "error": e.to_string() }),
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
