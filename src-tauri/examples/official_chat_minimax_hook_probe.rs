//! MiniMax「官网引用」路线 B 的**页内取数**探针（macOS 真机，只读测量）。
//!
//! # 这一轮问什么：**能不能把「页内猴补」的痕迹从「整窗生命周期」压到「首屏几秒」**
//!
//! 2026-10-23 上一轮已实测确定（**本轮不重复测那几格**）：
//!
//! · MiniMax 的 API 请求**必须带签名**（站点自己那一发的头名是
//!   `Content-Type, token, yy, x-timestamp, x-signature`），
//! · **签名复刻不出来**（102 种形态全 400 `invalid signature`），
//! · ⇒ **「自己拼一个请求」这条路走不通**；
//! · 形态①（页内挂钩截响应体）与形态②（重放截到的签名请求）都成立。
//!
//! ## 生产落地形态的问题
//!
//! 生产若照抄探针 ⇒「document-start 猴补 `window.fetch` 与
//! `XMLHttpRequest.prototype.*`，并 `setInterval` 反复补挂」⇒ **痕迹贯穿整窗**。
//! P0 裁定 3（`docs/features/official-chat/official-chat-feasibility.md:198`/`:202`）
//! 的纪律是「页面 JS 空间不得留壳留下的东西」⇒ 猴补虽然不新增 window 属性名，
//! 但会让 `window.fetch` **不再是原生函数**（`fetch.toString()` 不再是
//! `[native code]`）⇒ 这正是「像不像普通浏览器」维度的偏离。
//!
//! ## 本轮要量的五格
//!
//! · **M1（核心）**：钩子截到目录那一发 ⇒ **立即还原**
//!   `window.fetch` 与 `XMLHttpRequest.prototype.open/setRequestHeader/send`
//!   ⇒ 然后用截到的**签名请求头**在页内 `fetch` **正文路径**
//!   ⇒ 报状态码 / 业务码 / 顶层键名 / `$.messages` 条数 / 消息键名 /
//!   角色取值 / `msg_content` 与 `thinking_content` 在不在 / 正文响应字节数。
//!   · 阳性对照：**不还原、直接重放**（= 上一轮形态②）。
//!   · 反向对照：摘掉 `x-signature`/`x-timestamp`/`yy` 再重放 ⇒ 必须 400。
//! · **M2**：还原**前后**各读一次痕迹（`fetch.toString()` 头与长度、
//!   `Object.getOwnPropertyNames(window).length`、名字里带 `__dsh`/`__mmx`
//!   的 window 自有属性计数、**还原后我们那几层还在不在链上**）。
//! · **M3**：目录那一发走的是 `fetch` 还是 `XMLHttpRequest`。
//! · **M4**：同一份签名请求头在 **+0/+30/+60/+120/+300 秒**各重放一次
//!   （上一轮只到 60 秒）⇒ 每一档都配一次「摘掉签名」的��向对照。
//! · **M5**：还原之后站点**再发一次**该接口的请求 ⇒ 确认站点自己那一发
//!   仍 200 ⇒ 证明「还原」对站点透明。
//!
//! ## 「还原」怎么算还原（**这一轮量出来的形状，不是假设**）
//!
//! 上一轮实测：目录那一发在 `REC.requests` 里**被记了两次**（相隔 9ms）
//! ⇒ 说明**钩子被套了两层**（站点自己也包了一层 `fetch`，我们的
//! `setInterval` 又把站点那层包了一层）⇒ ⇒ **还原必须分层做**：
//! 顶层包装还原成「**它替换掉的那个函数**」，内层包装只能「失效」（变回
//! 纯转发）。本轮用**残留探针**（还原后发一发、看我们的记录里还有没有它）
//! **实测**内层残留到底还在不在。
//!
//! # 隔离（判据 `probe-profile-isolation`）
//!
//! 源是用户真实那份 WK 存储 ⇒ **只读拷贝**到探针自己的目录 ⇒ 探针写的全在
//! 拷贝里。跑前跑后核**源 cookie 文件的 sha256** 相同（只读证明）。
//!
//! # 读数纪律
//!
//! **不报**：token 的值、签名的值、cookie 的值、**会话 id**、会话标题原文、
//! 消息正文原文。**报**：头名、值的**长度**、结构键名、条数、状态码、字节数。
//!
//! **会话 id 全程留在页内**（页内自己拼正文路径、自己发）⇒ 宿主**一个字节的
//! id 都拿不到** ⇒ 读数里只有 `sessionIdLength`。
//!
//! 这条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块编译掉。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe: 本探针只支持 macOS（WKWebsiteDataStore 路线）。");
    std::process::exit(2);
}

/// **看门狗**：探针卡住时**自己**退出，不占着整轮等待。
///
/// ⚠️ 本轮要等 **+300 秒**那一档（M4）⇒ 加上建窗/等就绪/正文重放/M5，
/// 一轮最长约 8 分钟 ⇒ 看门狗给到 15 分钟。
#[cfg(target_os = "macos")]
const WATCHDOG_SECONDS: u64 = 1200;

/// 探针自己的数据存储标识符（**不是**生产的任何一个）。
///
/// ⚠️ **纪律**：探针**绝不能**起在用户真实那份档案上（浏览器一启动就写
/// Cookies／LocalStorage）⇒ ⇒ 生产那两个标识符在探针里**一个都不许出现**。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-mhk-001";

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};

    /// 探针自己的数据存储标识符（根级那个）。
    use crate::PROBE_DATA_STORE_IDENTIFIER;

    /// **源存储目录**从哪个环境变量读（源码里不写路径字面量）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// MiniMax 的入口地址（会 301 到 `agent.minimax.cn`）。
    const SITE_URL: &str = "https://agent.minimaxi.com";

    /// 目录端点（生产规格表里那个 `catalog_path`）。
    const CATALOG_PATH: &str = "/minimax-cloud/api/v1/sidebar/session/tree";

    /// 反向对照用的**不存在**的路径。
    const CONTROL_PATH: &str = "/minimax-cloud/api/v1/__probe_control__";

    /// **残留探针**用的**不存在**的路径（与上面的对照路径分开 ⇒ 记录不混）。
    const RESIDUAL_PATH: &str = "/minimax-cloud/api/v1/__probe_residual__";

    /// 正文端点的**路径形状**（`session_id` 留在页内 ⇒ 宿主拿不到真值）。
    const MESSAGE_PATH_SHAPE: &str = "/minimax-cloud/api/v1/session/<REDACTED>/message";

    /// 深链前缀（生产规格表里那个 `deep_link_prefix`）。**本轮不用**（M1 走
    /// 重放，不走站点自己导航），留着是为了让报告里能对照上一轮。
    #[allow(dead_code)]
    const DEEP_LINK_PREFIX: &str = "https://agent.minimax.cn/?id=";

    /// **页内挂钩脚本**（在页面脚本之前注入，**每个文档都跑一遍**）。
    ///
    /// ## 它做什么
    ///
    /// 1. **先量「猴补前」的基线**（window 自有属性名数、`fetch` 是不是原生
    ///    函数、XHR 三个原型方法是不是原生函数）——**在装任何钩子之前**。
    /// 2. 猴补 `window.fetch` 与 `XMLHttpRequest` 的三个原型方法 ⇒ 记下
    ///    站点自己那一发的 **URL（打码）**、方法、**头名 ＋ 头值长度**、
    ///    以及**走的是哪条通道**（`via`）。
    /// 3. **响应体捕获**：`fetch` 用 `res.clone().text()`（克隆，不动站点自己
    ///    那份流）；`XMLHttpRequest` 用 `load` 事件读 `responseText`。
    ///    读不到就记「body 不可得」，**不挂死**（8 秒兜底）。
    /// 4. **截到目录那一发 ⇒ 立刻还原**（分层还原，见上）⇒ 并留下**还原前后**
    ///    两份痕迹快照。
    /// 5. **重放**（签名有效期阶梯 +0/+30/+60/+120/+300 秒，每档配一次
    ///    「摘掉签名」的反向对照）。
    /// 6. **正文重放**：用截到的签名请求头，在页内 `fetch` 正文路径。
    ///
    /// ## 为什么重放要留在页内
    ///
    /// `x-signature` / `x-timestamp` 的**值**一步都不出页面 ⇒ 宿主拿到的只是
    /// 「状态码 ＋ 结构」⇒ 凭据零落盘这条纪律在探针里也成立。
    const HOOK_SCRIPT: &str = r##"
(function () {
  // 只在顶层 frame 干活。
  try { if (window.top !== window.self) return; } catch (e) { return; }

  // ---- 小工具 ------------------------------------------------------------
  function isNative(fn) {
    try { return Function.prototype.toString.call(fn).indexOf('[native code]') >= 0; }
    catch (e) { return null; }
  }
  function srcInfo(fn, marker) {
    var out = { threw: false, length: -1, head: '', hasMarker: null, isNative: null, error: null };
    try {
      var s = Function.prototype.toString.call(fn);
      out.length = s.length;
      out.head = s.slice(0, 200);
      out.hasMarker = s.indexOf(marker) >= 0;
      out.isNative = s.indexOf('[native code]') >= 0;
    } catch (e) {
      // ⚠️ **`Function.prototype.toString` 对 Proxy 会抛** ⇒ 这本身是一条读数
      //    （站点自己那层 `fetch` 若�� Proxy ⇒ 猴补前它就读不出来）。
      out.threw = true;
      out.error = String(e).slice(0, 120);
    }
    return out;
  }
  function srcOf(fn) {
    try { return Function.prototype.toString.call(fn); } catch (e) { return ''; }
  }
  function srcHead(fn, n) {
    try { return srcOf(fn).slice(0, n); } catch (e) { return ''; }
  }
  function srcLen(fn) {
    try { return srcOf(fn).length; } catch (e) { return -1; }
  }
  function ownNames() {
    try { return Object.getOwnPropertyNames(window); } catch (e) { return []; }
  }
  function probeOwnNames() {
    var out = [];
    var names = ownNames();
    for (var i = 0; i < names.length; i++) {
      var n = String(names[i]);
      if (n.indexOf('__dsh') === 0 || n.indexOf('__mmx') === 0) out.push(n);
    }
    return out;
  }

  // ---- 猴补前的基线（**在装任何钩子、创建任何槽位之前**）-----------------
  var BASE = {
    at: Date.now(),
    ownCount: ownNames().length,
    probeOwnNames: probeOwnNames(),
    fetchInfo: srcInfo(window.fetch, 'REC.suppress'),
    fetchName: (function () { try { return String(window.fetch.name || ''); } catch (e) { return '<读不到>'; } })(),
    xhrOpenInfo: srcInfo(XMLHttpRequest.prototype.open, '__mmxPairs'),
    xhrSetHeaderInfo: srcInfo(XMLHttpRequest.prototype.setRequestHeader, '__mmxPairs'),
    xhrSendInfo: srcInfo(XMLHttpRequest.prototype.send, '__mmxPairs')
  };

  // ---- 两份记录：REC 可读（能进读数），SEC 有秘密（**永不进读数**）--------
  var REC = {
    probe: 'official_chat_minimax_hook_probe',
    docInstalledAt: BASE.at,
    base: BASE,
    requests: [],
    responses: [],
    m1: {},
    ladder: {},
    snapshots: [],
    suppress: false,
    catalogCapturedAt: null,
    catalogCapturedTransport: null,
    catalogUrlMasked: null,
    catalogHandled: false,
    firstSignedApiAt: null,
    firstSignedApiTransport: null,
    firstSignedApiUrlMasked: null,
    restoreResult: null,
    bodyPrep: null,
    residualProbe: null
  };
  var SEC = { signed: null, rawCatalogBody: null, sessionIdLength: 0, bodyUrl: null };

  // ---- 还原触发模式 -------------------------------------------------------
  // 'catalog'（默认）：document-start 就猴补 ⇒ 截到目录那一发才还原。
  // 'on_top'：**不**在 document-start 猴补 ⇒ 等站点自己把 fetch 包好之后，
  //            再**套在它上面** ⇒ 还原时能把我们这一层**整个摘掉**（零残留）。
  var MODE = 'catalog';
  try {
    var h = String(location.hash || '') + ' ' + String(location.search || '');
    if (h.indexOf('dsh_probe_mode=on_top') >= 0) MODE = 'on_top';
  } catch (e) {}
  REC.mode = MODE;

  // 猴补前那三个原函数（PRE）——「还原」的**判定基准**。
  var PRE = {
    fetch: window.fetch,
    xhrOpen: XMLHttpRequest.prototype.open,
    xhrSetHeader: XMLHttpRequest.prototype.setRequestHeader,
    xhrSend: XMLHttpRequest.prototype.send
  };
  // 我们自己套过的每一层（用来数「我们还在链上几层」）。
  var OURS = { fetch: [], xhrOpen: [], xhrSetHeader: [], xhrSend: [] };
  var TIMER = null;

  // ---- 打码：凭据 / 会话 id 的**值**一律不进读数 ---------------------------
  function maskUrl(u) {
    var s = String(u == null ? '' : u);
    try {
      s = s.replace(/([?&](?:token|id|uuid|device_id|user_id|sign|signature|x-signature|x-timestamp|yy|agent_name|project_id)=)[^&]*/g, '$1<REDACTED>');
      // ⚠️ **会话 id 在路径里** ⇒ 一律打码。
      s = s.replace(/(\/api\/v1\/session\/)[^/?#]+/g, '$1<REDACTED>');
    } catch (e) {}
    return s;
  }

  // ---- 端点判定 ------------------------------------------------------------
  function isApi(u) { return /\/minimax-cloud\/api\//.test(String(u || '')); }
  function isCatalog(u) { return /\/minimax-cloud\/api\/v1\/sidebar\/session\/tree/.test(String(u || '')); }

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

  function pairsHaveSignature(pairs) {
    for (var i = 0; i < pairs.length; i++) {
      var n = String(pairs[i][0]).toLowerCase();
      if (n === 'x-signature' || n === 'x-timestamp' || n === 'yy') return true;
    }
    return false;
  }

  /** 摘掉签名三件套（`x-signature` / `x-timestamp` / `yy`）⇒ 反向对照用。 */
  function stripSignature(pairs) {
    var headers = {};
    var kept = [];
    for (var i = 0; i < pairs.length; i++) {
      var name = String(pairs[i][0]);
      var lower = name.toLowerCase();
      if (lower === 'x-signature' || lower === 'x-timestamp' || lower === 'yy') continue;
      headers[name] = String(pairs[i][1]);
      kept.push(name);
    }
    return { headers: headers, kept: kept };
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

  // ---- 痕迹快照（**还原前 / 还原后**各来一份）-----------------------------
  function snapshot(label) {
    var s = { label: label, at: Date.now() };
    try {
      var names = ownNames();
      s.ownCount = names.length;
      var pn = probeOwnNames();
      s.probeOwnNames = pn;
      s.probeOwnCount = pn.length;
    } catch (e) { s.ownCount = -1; }
    var fi = srcInfo(window.fetch, 'REC.suppress');
    s.fetchInfo = fi;
    s.fetchIsNative = fi.isNative;
    s.fetchSrcLength = fi.length;
    s.fetchSrcHead = fi.head;
    s.fetchSrcThrew = fi.threw;
    s.fetchHasOurMarker = fi.hasMarker;
    try { s.fetchName = String(window.fetch.name || ''); } catch (e) { s.fetchName = '<读不到>'; }
    try { s.fetchHasHookMark = !!(window.fetch && window.fetch.__mmxHooked); } catch (e) {}
    try { s.fetchOwnProps = Object.getOwnPropertyNames(window.fetch); } catch (e) {}
    var oi = srcInfo(XMLHttpRequest.prototype.open, '__mmxPairs');
    s.xhrOpenInfo = oi;
    s.xhrOpenIsNative = oi.isNative;
    s.xhrOpenSrcLength = oi.length;
    s.xhrOpenHasOurMarker = oi.hasMarker;
    try { s.xhrOpenHasHookMark = !!(XMLHttpRequest.prototype.open && XMLHttpRequest.prototype.open.__mmxHooked); } catch (e) {}
    var si = srcInfo(XMLHttpRequest.prototype.setRequestHeader, '__mmxPairs');
    s.xhrSetHeaderIsNative = si.isNative;
    s.xhrSetHeaderSrcLength = si.length;
    var di = srcInfo(XMLHttpRequest.prototype.send, '__mmxPairs');
    s.xhrSendIsNative = di.isNative;
    s.xhrSendSrcLength = di.length;
    try { s.xhrSendHasHookMark = !!(XMLHttpRequest.prototype.send && XMLHttpRequest.prototype.send.__mmxHooked); } catch (e) {}
    // **新建一个 XHR 实例** ⇒ 看它身上还有没有 `__mmx*` 标记。
    //    ⚠️ 必须**调一次 open()**（不发请求）⇒ 原型上的钩子还在时，
    //    `open()` 会往实例上打 `__mmxMethod/__mmxUrl/__mmxPairs`。
    try {
      var x = new XMLHttpRequest();
      var xn = Object.getOwnPropertyNames(x);
      var marks = [];
      for (var j = 0; j < xn.length; j++) if (String(xn[j]).indexOf('__mmx') === 0) marks.push(xn[j]);
      s.newXhrOwnPropsBeforeOpen = xn.length;
      s.newXhrHookMarksBeforeOpen = marks;
      try {
        x.open('GET', location.href, false);
        var xn2 = Object.getOwnPropertyNames(x);
        var marks2 = [];
        for (var k = 0; k < xn2.length; k++) if (String(xn2[k]).indexOf('__mmx') === 0) marks2.push(xn2[k]);
        s.newXhrOwnPropsAfterOpen = xn2.length;
        s.newXhrHookMarksAfterOpen = marks2;
      } catch (e3) { s.newXhrOpenError = String(e3).slice(0, 120); }
    } catch (e) { s.newXhrHookMarks = ['<读不到>']; }
    return s;
  }

  // ---- 重放：URL ＋ 完整头（**值在页内**）原样再发一次 ----------------------
  //    `suppress` 为真 ⇒ 这一发**不进** `requests`/`responses`
  //    （否则会被误当成「站点自己那一发」）。
  function runFetch(obj, key, url, headers, note, suppress) {
    obj[key] = { state: 'running', note: note, at: Date.now() };
    var p;
    try {
      REC.suppress = !!suppress;
      p = fetch(url, { method: 'GET', headers: headers, credentials: 'include' });
      REC.suppress = false;
    } catch (e) {
      REC.suppress = false;
      obj[key] = { state: 'done', note: note, error: '发起失败：' + String(e).slice(0, 120) };
      return false;
    }
    p.then(function (res) {
      return readBody(res).then(function (r) {
        obj[key] = {
          state: 'done', note: note, status: res.status, at: Date.now(),
          deltaMs: REC.catalogCapturedAt ? Date.now() - REC.catalogCapturedAt : null,
          summary: r.error ? { status: res.status, bodyUnavailable: r.error } : summarize(res.status, r.text)
        };
        return null;
      });
    }).catch(function (e) {
      obj[key] = { state: 'done', note: note, error: String(e).slice(0, 160), at: Date.now() };
    });
    return true;
  }

  // ---- 「还原」：分层还原（**顶层换回它替换掉的那个函数**）------------------
  //
  // ⚠️ **签名必须是 `(target, key)`**（r10 实测踩过：三参版本被两参调用 ⇒
  //    `target[key]` 取到 `undefined` ⇒ 还原**静默失败**，读数里「顶层不是
  //    我们的包装」）。
  function restoreOne(target, key) {
    try {
      var cur = target[key];
      if (cur && cur.__mmxHooked && cur.__mmxPrev) {
        target[key] = cur.__mmxPrev;
        var info = srcInfo(target[key], 'REC.suppress');
        return {
          restored: true,
          restoredIsNative: info.isNative,
          restoredSrcThrew: info.threw,
          restoredHasOurMarker: info.hasMarker,
          restoredSrcLength: info.length,
          restoredSrcHead: info.head,
          restoredOwnProps: (function () { try { return Object.getOwnPropertyNames(target[key]); } catch (e) { return []; } })()
        };
      }
      var curInfo = srcInfo(cur, 'REC.suppress');
      return {
        restored: false,
        reason: '顶层不是我们的包装（站点自己又包了一层，或已经还原过）',
        currentIsNative: curInfo.isNative,
        currentSrcThrew: curInfo.threw,
        currentHasOurMarker: curInfo.hasMarker,
        currentSrcLength: curInfo.length,
        currentSrcHead: curInfo.head
      };
    } catch (e) {
      return { restored: false, error: String(e).slice(0, 120) };
    }
  }

  function stopTimer() {
    if (TIMER === null) return false;
    try { clearInterval(TIMER); TIMER = null; return true; } catch (e) { return false; }
  }

  function restoreAll(reason) {
    if (REC.restoreResult) return REC.restoreResult;
    var out = { at: Date.now(), reason: reason || '', pre: snapshot('pre-restore') };
    out.fetch = restoreOne(window, 'fetch');
    out.xhr = {
      open: restoreOne(XMLHttpRequest.prototype, 'open'),
      setRequestHeader: restoreOne(XMLHttpRequest.prototype, 'setRequestHeader'),
      send: restoreOne(XMLHttpRequest.prototype, 'send')
    };
    out.timerStopped = stopTimer();
    out.oursLayerCount = { fetch: OURS.fetch.length, xhrOpen: OURS.xhrOpen.length, xhrSetHeader: OURS.xhrSetHeader.length, xhrSend: OURS.xhrSend.length };
    try { out.equalsPrePatchFetch = (window.fetch === PRE.fetch); } catch (e) {}
    try { out.equalsPrePatchXhrOpen = (XMLHttpRequest.prototype.open === PRE.xhrOpen); } catch (e) {}
    try { out.equalsPrePatchXhrSetHeader = (XMLHttpRequest.prototype.setRequestHeader === PRE.xhrSetHeader); } catch (e) {}
    try { out.equalsPrePatchXhrSend = (XMLHttpRequest.prototype.send === PRE.xhrSend); } catch (e) {}
    out.post = snapshot('post-restore');
    out.windowOwnCountDelta = out.post.ownCount - out.pre.ownCount;
    out.docToRestoreMs = out.at - REC.docInstalledAt;
    out.captureToRestoreMs = REC.catalogCapturedAt ? out.at - REC.catalogCapturedAt : null;
    REC.restoreResult = out;
    REC.snapshots.push(out.pre);
    REC.snapshots.push(out.post);
    return out;
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
        var signed = pairsHaveSignature(pairs);
        if (capture) {
          REC.requests.push({
            transport: 'fetch', via: 'fetch', url: maskUrl(url), method: method,
            headerShape: headerShape(pairs), signed: signed, at: Date.now()
          });
          if (signed && !REC.firstSignedApiAt) {
            REC.firstSignedApiAt = Date.now();
            REC.firstSignedApiTransport = 'fetch';
            REC.firstSignedApiUrlMasked = maskUrl(url);
            REC.snapshots.push(snapshot('at-first-signed-api-request'));
          }
          if (isCatalog(url) && !SEC.signed) {
            SEC.signed = { url: String(url), method: method, pairs: pairs, headersObj: toObject(pairs), transport: 'fetch' };
            REC.catalogCapturedAt = Date.now();
            REC.catalogCapturedTransport = 'fetch';
            REC.catalogUrlMasked = maskUrl(url);
            onCatalogCaptured();
          }
        }
        var res;
        try { res = origFetch.apply(this, arguments); }
        catch (e) { return Promise.reject(e); }
        if (!capture) return res;
        // ⚠️ **`fetch` 返回的是 Promise，不是 Response**（上一轮实测踩过：
        //    对 Promise 调 `.clone()` ⇒ TypeError ⇒ 响应体一条都没截到）。
        try {
          res.then(function (response) {
            try {
              var cloned = response.clone();
              readBody(cloned).then(function (r) {
                var summary = r.error
                  ? { status: response.status, bodyUnavailable: r.error }
                  : summarize(response.status, r.text);
                REC.responses.push({ transport: 'fetch', via: 'fetch', url: maskUrl(url), at: Date.now(), summary: summary });
                try { if (isCatalog(url) && !SEC.rawCatalogBody) SEC.rawCatalogBody = r.text || ''; } catch (e) {}
                try { if (isCatalog(url) && SEC.rawCatalogBody) maybeStartBodyReplays(); } catch (e2) {}
              }).catch(function (e) {
                REC.responses.push({
                  transport: 'fetch', via: 'fetch', url: maskUrl(url),
                  summary: { status: response.status, bodyUnavailable: '读体失败：' + String(e).slice(0, 100) }
                });
              });
            } catch (e) {
              REC.responses.push({
                transport: 'fetch', via: 'fetch', url: maskUrl(url),
                summary: { status: (response && response.status) || 0, bodyUnavailable: 'clone 失败：' + String(e).slice(0, 100) }
              });
            }
          }, function (err) {
            REC.responses.push({
              transport: 'fetch', via: 'fetch', url: maskUrl(url),
              summary: { status: 0, bodyUnavailable: '请求失败：' + String(err).slice(0, 100) }
            });
          });
        } catch (e) {}
        return res;
      };
      try { wrapped.__mmxHooked = true; } catch (e) {}
      try { wrapped.__mmxPrev = origFetch; } catch (e) {}
      OURS.fetch.push(wrapped);
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
      try { wrappedOpen.__mmxPrev = XO; } catch (e) {}
      OURS.xhrOpen.push(wrappedOpen);
      XMLHttpRequest.prototype.open = wrappedOpen;

      var wrappedSet = function (name, value) {
        try { this.__mmxPairs.push([String(name), String(value)]); } catch (e) {}
        return XS.apply(this, arguments);
      };
      try { wrappedSet.__mmxHooked = true; } catch (e) {}
      try { wrappedSet.__mmxPrev = XS; } catch (e) {}
      OURS.xhrSetHeader.push(wrappedSet);
      XMLHttpRequest.prototype.setRequestHeader = wrappedSet;

      var wrappedSend = function () {
        try {
          var self = this;
          var url = self.__mmxUrl || '';
          if (isApi(url) && !REC.suppress) {
            var pairs = self.__mmxPairs || [];
            var method = self.__mmxMethod || 'GET';
            var signed = pairsHaveSignature(pairs);
            REC.requests.push({
              transport: 'xhr', via: 'xhr', url: maskUrl(url), method: method,
              headerShape: headerShape(pairs), signed: signed, at: Date.now()
            });
            if (signed && !REC.firstSignedApiAt) {
              REC.firstSignedApiAt = Date.now();
              REC.firstSignedApiTransport = 'xhr';
              REC.firstSignedApiUrlMasked = maskUrl(url);
              REC.snapshots.push(snapshot('at-first-signed-api-request'));
            }
            if (isCatalog(url) && !SEC.signed) {
              SEC.signed = { url: String(url), method: method, pairs: pairs, headersObj: toObject(pairs), transport: 'xhr' };
              REC.catalogCapturedAt = Date.now();
              REC.catalogCapturedTransport = 'xhr';
              REC.catalogUrlMasked = maskUrl(url);
              onCatalogCaptured();
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
                  REC.responses.push({ transport: 'xhr', via: 'xhr', url: maskUrl(url), at: Date.now(), summary: summary });
                  try { if (isCatalog(url) && !SEC.rawCatalogBody) SEC.rawCatalogBody = text || ''; } catch (e3) {}
                  try { if (isCatalog(url) && SEC.rawCatalogBody) maybeStartBodyReplays(); } catch (e4) {}
                } catch (e5) {
                  REC.responses.push({
                    transport: 'xhr', via: 'xhr', url: maskUrl(url),
                    summary: { status: 0, bodyUnavailable: String(e5).slice(0, 120) }
                  });
                }
              });
            } catch (e6) {}
          }
        } catch (e) {}
        return XSend.apply(this, arguments);
      };
      try { wrappedSend.__mmxHooked = true; } catch (e) {}
      try { wrappedSend.__mmxPrev = XSend; } catch (e) {}
      OURS.xhrSend.push(wrappedSend);
      XMLHttpRequest.prototype.send = wrappedSend;
    } catch (e) {}
  }

  function hookAll() { hookFetch(); hookXhr(); }

  // ---- 签名有效期阶梯（+0 / +30 / +60 / +120 / +300 秒），每档配反向对照 ----
  function scheduleLadder() {
    var stages = [0, 30000, 60000, 120000, 300000];
    for (var i = 0; i < stages.length; i++) {
      (function (d) {
        var slot = 'plus' + d + 'ms';
        REC.ladder[slot] = { signed: null, unsigned: null, scheduledAt: Date.now() };
        setTimeout(function () {
          var sec = SEC.signed;
          if (!sec) {
            REC.ladder[slot] = {
              signed: { state: 'done', error: '没抓到可重放的签名请求' },
              unsigned: { state: 'done', error: '没抓到可重放的签名请求' }
            };
            return;
          }
          // ⚠️ **不要**把 `runFetch` 的返回值（布尔）赋回去 ⇒ 会把刚写的
          // 「running/ done」对象覆盖掉。
          runFetch(
            REC.ladder[slot], 'signed', sec.url, sec.headersObj,
            '同一份签名请求头重放（+' + d + 'ms）', true);
          var no = stripSignature(sec.pairs);
          runFetch(
            REC.ladder[slot], 'unsigned', sec.url, no.headers,
            '反向对照：摘掉 x-signature/x-timestamp/yy 重放（+' + d + 'ms）', true);
        }, d);
      })(stages[i]);
    }
  }

  // ---- 截到目录那一发 ⇒ 阳性对照 + **立刻还原** + 排阶梯 ------------------
  //
  // ⚠️ **为什么这里就要还原**：痕迹窗口 = 「装钩 → 截到目录那一发」这一段。
  //    还原越早，窗口越短 ⇒ ⇒ 还原必须**在页内、同步**发生（不能等宿主来问）。
  function onCatalogCaptured() {
    if (REC.catalogHandled) return;
    REC.catalogHandled = true;
    var sec = SEC.signed;
    if (!sec) return;
    REC.m1 = REC.m1 || {};
    // **阳性对照**：钩子还在（**未还原**）、直接重放截到的签名请求
    //             （= 上一轮形态②那条已成立的路）。
    runFetch(REC.m1, 'positive_no_restore', sec.url, sec.headersObj,
      '阳性对照：钩子还在（未还原）、直接重放截到的签名请求', true);
    // **反向对照**（未还原）：摘掉签名三件套 ⇒ 必须 400。
    var no = stripSignature(sec.pairs);
    runFetch(REC.m1, 'negative_no_restore', sec.url, no.headers,
      '反向对照（未还原）：摘掉 x-signature/x-timestamp/yy 重放', true);
    // **立刻还原** ⇒ 痕迹窗口到此为止。
    restoreAll('截到目录那一发 ⇒ 立刻还原');
    // 签名有效期阶梯（+0/+30/+60/+120/+300 秒，每档配一次反向对照）。
    scheduleLadder();
  }

  // ---- 拼正文路径（**session_id 全程留在页内**）---------------------------
  function buildBodyUrl() {
    var out = { at: Date.now() };
    try {
      var text = SEC.rawCatalogBody;
      if (!text) { out.ok = false; out.error = '没有截到目录响应体'; return out; }
      var j = JSON.parse(text);
      var arr = (j && Array.isArray(j.sessions)) ? j.sessions : [];
      if (!arr.length) { out.ok = false; out.error = '目录里没有会话条目'; return out; }
      var el = arr[0] || {};
      var s = (el.session && typeof el.session === 'object') ? el.session : el;
      var id = s.session_id || s.id || el.session_id || el.id || '';
      if (!id) {
        out.ok = false; out.error = '第一条会话里找不到 id 字段';
        out.availableKeys = Object.keys(s).slice(0, 32);
        return out;
      }
      SEC.sessionIdLength = String(id).length;
      var catUrl = SEC.signed ? String(SEC.signed.url) : '';
      var qi = catUrl.indexOf('?');
      var q = qi >= 0 ? catUrl.slice(qi + 1) : '';
      var kept = [];
      var parts = q.split('&');
      for (var i = 0; i < parts.length; i++) {
        if (!parts[i]) continue;
        var pn = parts[i].split('=')[0];
        if (pn === 'limit' || pn === 'project_id') continue;
        kept.push(parts[i]);
      }
      // ⚠️ **session_id 必须拼进路径**（r10 实测踩过：漏拼 ⇒ URL 变成
      //    `/api/v1/session/?…` ⇒ 404，正向与反向对照都 404 ⇒ 读数作废）。
      SEC.bodyUrl = location.origin + '__MESSAGE_PATH__' + encodeURIComponent(String(id)) +
        '/message?limit=80&_source=session-switch&' + kept.join('&');
      out.ok = true;
      out.sessionIdLength = SEC.sessionIdLength;
      out.bodyPathShape = '__MESSAGE_PATH_SHAPE__';
      out.queryKeys = [];
      for (var k = 0; k < kept.length; k++) out.queryKeys.push(kept[k].split('=')[0]);
      out.queryKeyCount = kept.length;
      out.bodyUrlMasked = maskUrl(SEC.bodyUrl);
      out.urlMasked = maskUrl(SEC.bodyUrl);
      return out;
    } catch (e) {
      out.ok = false; out.error = String(e).slice(0, 160);
      return out;
    }
  }

  /** 目录响应体到手 ⇒ 立刻做 M1 的正文重放（**已经还原过了**）。 */
  function maybeStartBodyReplays() {
    if (REC.m1 && REC.m1.body) return;
    var prep = buildBodyUrl();
    REC.bodyPrep = prep;
    if (!prep.ok) return;
    var sec = SEC.signed;
    if (!sec) return;
    REC.m1 = REC.m1 || {};
    REC.m1.body = { startedAt: Date.now(), restoredBefore: !!REC.restoreResult };
    runFetch(REC.m1.body, 'signed', SEC.bodyUrl, sec.headersObj,
      '还原后：用截到的签名请求头在页内 fetch 正文路径', true);
    var no = stripSignature(sec.pairs);
    runFetch(REC.m1.body, 'unsigned', SEC.bodyUrl, no.headers,
      '反向对照：摘掉 x-signature/x-timestamp/yy 后重放正文路径', true);
  }

  // ---- 页内工具（宿主通过求值调用）---------------------------------------

  /** 还原（幂等）。 */
  REC.doRestore = function () { return restoreAll('截到目录那一发 ⇒ 立刻还原'); };

  /** 痕迹快照。 */
  REC.doSnapshot = function (label) { var s = snapshot(String(label || 'adhoc')); REC.snapshots.push(s); return s; };

  /** 残留探针：还原后发一发**不带签名**的控制路径请求 ⇒ 看我们的记录里还有没有它。 */
  REC.doResidualProbe = function () {
    REC.residualProbe = { at: Date.now(), path: '__RESIDUAL_PATH__', beforeRequestCount: REC.requests.length };
    runFetch(REC.residualProbe, 'result', location.origin + '__RESIDUAL_PATH__', {},
      '还原后的残留探针（不带签名，只看我们的记录里还有没有它）', false);
    return REC.residualProbe;
  };

  /** M5：让站点自己再发一次请求（派发焦点/可见性事件，不点链接 ⇒ 不导航）。 */
  REC.doM5Trigger = function () {
    var out = { at: Date.now(), actions: [] };
    try { window.dispatchEvent(new Event('focus')); out.actions.push('window:focus'); } catch (e) {}
    try { document.dispatchEvent(new Event('visibilitychange')); out.actions.push('document:visibilitychange'); } catch (e) {}
    try { window.scrollTo(0, (document.body && document.body.scrollHeight) || 0); out.actions.push('scroll:bottom'); } catch (e) {}
    out.requestCountBefore = REC.requests.length;
    out.responseCountBefore = REC.responses.length;
    return out;
  };

  /** 读资源计时（**不碰任何原型** ⇒ 观察通道本身不留痕迹）。 */
  REC.readPerf = function () {
    var out = { at: Date.now(), ok: false, entries: [], hasResponseStatus: false };
    try {
      var es = performance.getEntriesByType('resource') || [];
      out.total = es.length;
      for (var i = 0; i < es.length; i++) {
        var e = es[i];
        var n = String(e.name || '');
        if (!/\/minimax-cloud\/api\//.test(n)) continue;
        var item = { name: maskUrl(n), startTime: Math.round(e.startTime), duration: Math.round(e.duration || 0) };
        try { item.transferSize = e.transferSize; } catch (e2) {}
        try { if (typeof e.responseStatus !== 'undefined') { item.responseStatus = e.responseStatus; out.hasResponseStatus = true; } } catch (e3) {}
        out.entries.push(item);
      }
      out.apiEntryCount = out.entries.length;
      out.ok = true;
    } catch (e) { out.error = String(e).slice(0, 120); }
    return out;
  };

  /** 清理：把探针在 window 上的槽位全删掉 ⇒ 「猴补痕迹」归零。 */
  REC.doCleanup = function () {
    var out = { at: Date.now() };
    try { out.beforeOwnCount = ownNames().length; } catch (e) {}
    try { out.beforeProbeOwnNames = probeOwnNames(); } catch (e) {}
    try { delete window.__mmxHook; } catch (e) { out.deleteError = String(e).slice(0, 120); }
    try { out.afterOwnCount = ownNames().length; } catch (e) {}
    try { out.afterProbeOwnNames = probeOwnNames(); } catch (e) {}
    return out;
  };

  /** 重新导航（同一个 URL）⇒ 新文档会重跑初始化脚本 ⇒ M5 的第二臂。 */
  REC.doReload = function () {
    try { location.reload(); return { ok: true }; } catch (e) { return { ok: false, error: String(e) }; }
  };

  // ---- 装钩 ---------------------------------------------------------------
  //
  // ⚠️ **两种装法**（`MODE` 决定）：
  // · 'catalog'：document-start 直接猴补 ⇒ 一定能截到站点的那一发，但还原时
  //   **我们那层被压在站点那层下面** ⇒ 站点那层的闭包里还留着我们的一层
  //   （r11 实测：残留探针仍看得到我们 ⇒ XHR 实例上仍会多出 `__mmx*` 标记）。
  // · 'on_top'：**先不猴补**，等站点自己把 fetch 包好（`toString` 不再是
  //   `[native code]`）⇒ 再**套在它上面** ⇒ 还原时把��们这层**整个摘掉**
  //   ⇒ 残留应当为 0。
  function looksNative(fn) {
    try { return Function.prototype.toString.call(fn).indexOf('[native code]') >= 0; }
    catch (e) { return false; }   // Proxy 读不出来 ⇒ 视为「站点那层」
  }
  function installOnTop() {
    var ready = false;
    try { ready = !!window.fetch && !looksNative(window.fetch); } catch (e) {}
    if (!ready) return false;
    hookAll();
    if (!REC.installAt) {
      REC.installPath = 'site-layer-found';
      REC.installAt = Date.now();
      REC.installedOnTopOfSrcLength = srcLen(window.fetch && window.fetch.__mmxPrev);
      REC.installedOnTopOfIsNative = isNative(window.fetch && window.fetch.__mmxPrev);
    }
    return true;
  }
  if (MODE === 'on_top') {
    // 轮询等站点那层出现（5ms 一档）⇒ 套在它**上面**。
    // ⚠️ **不清这个定时器**：站点再替换 fetch 时，我们仍然**套在上面**
    //    （而不是像 'catalog' 那样被压在下面 ⇒ 还原后有残留）。
    try {
      TIMER = setInterval(function () {
        installOnTop();
      }, 5);
    } catch (e) { TIMER = null; }
    // 兜底：站点一直没包 ⇒ 照旧猴补（压在原生下面 ⇒ 会有残留）。
    // ⚠️ **兜底必须给足时间**（r12 实测踩过：2.5 秒的兜底**抢在**站点那层
    //    出现之前触发了 ⇒ 装到了原生下面 ⇒ 还原后仍有残留 ⇒ 白跑一轮）。
    try {
      setTimeout(function () {
        if (!REC.installAt) {
          REC.installPath = 'fallback-timeout';
          hookAll();
          REC.installAt = Date.now();
        }
      }, 30000);
    } catch (e) {}
  } else {
    hookAll();
    // 站点自己可能**覆盖** fetch ⇒ 定时补挂（抓到它后来的请求）。
    try { TIMER = setInterval(hookAll, 50); } catch (e) { TIMER = null; }
  }

  // ---- 暴露（**只有一个 window 槽位** ⇒ 猴补痕迹只有 1 个属性名）----------
  try {
    Object.defineProperty(window, '__mmxHook', {
      value: REC, writable: true, configurable: true, enumerable: false
    });
  } catch (e) { try { window.__mmxHook = REC; } catch (e2) {} }
})();
"##;

    /// 页内求值用的表达式。
    const HOOK_EXPR: &str = "window.__mmxHook";

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
    /// ⚠️ **显式落 `/tmp`**：`std::env::temp_dir()` 在 macOS 上是
    /// `$TMPDIR`（`/var/folders/…`）⇒ 证据会散在别处 ⇒ 显式写 `/tmp`。
    fn log_path() -> PathBuf {
        let round = std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string());
        PathBuf::from("/tmp").join(format!("dsh-minimax-hook-probe.{round}.log"))
    }

    /// 报告（JSON）的路径（**轮次独立命名**）。
    fn report_path() -> PathBuf {
        let round = std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string());
        PathBuf::from("/tmp").join(format!("dsh-minimax-hook-probe.{round}.json"))
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

    /// **装钩模式**（`DSH_PROBE_MODE=on_top` ⇒ 套在站点自己那层**上面**）。
    fn probe_mode() -> String {
        std::env::var("DSH_PROBE_MODE").unwrap_or_else(|_| "catalog".to_string())
    }

    /// 这一轮要不要**跳过签名有效期阶梯**（`DSH_PROBE_SKIP_LADDER=1`）。
    ///
    /// 阶梯要等 +300 秒 ⇒ 5 分钟。M4 已经单独量过（r11），换装钩形态时
    /// 不必再等一遍 ⇒ 这一档可以跳过，把整轮压到 1 分钟内。
    fn skip_ladder() -> bool {
        std::env::var("DSH_PROBE_SKIP_LADDER")
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
            .replace("__RESIDUAL_PATH__", RESIDUAL_PATH)
            .replace("__MESSAGE_PATH__", "/minimax-cloud/api/v1/session/")
            .replace("__MESSAGE_PATH_SHAPE__", MESSAGE_PATH_SHAPE);

        let builder =
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(url.clone()))
                .title("dsh MiniMax 页内挂钩探针")
                .inner_size(1000.0, 760.0)
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
        // 装钩模式用 **URL 的 hash** 传给页内脚本（页内读 `location.hash`）。
        let mode = probe_mode();
        let url = url.join(&format!("#dsh_probe_mode={mode}")).unwrap_or(url);
        log_line("probe", &format!("装钩模式：{mode}（url={url}）"));
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
                    .name("minimax-hook-probe".into())
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

    // ---- 读数工具 -----------------------------------------------------------

    /// 读回页内那份挂钩记录（**只含可报的东西**：URL 已打码、头只有名字与长度）。
    async fn read_hook(window: &tauri::WebviewWindow) -> Option<Value> {
        crate::script_eval::read_json(window, HOOK_EXPR, Duration::from_secs(3))
            .await
            .ok()
            .filter(|v| v.is_object())
    }

    /// 调用页内某个**同步**函数并拿回它的 JSON。
    async fn call_page(window: &tauri::WebviewWindow, expr: &str) -> Option<Value> {
        crate::script_eval::read_json(window, expr, Duration::from_secs(12))
            .await
            .ok()
            .filter(|v| v.is_object())
    }

    /// 等一个页内条件成立（**条件等待**，不靠固定睡眠）。
    async fn wait_until<F>(
        window: &tauri::WebviewWindow,
        budget: Duration,
        mut pred: F,
    ) -> Option<Value>
    where
        F: FnMut(&Value) -> bool,
    {
        let deadline = Instant::now() + budget;
        let mut last: Option<Value> = None;
        while Instant::now() < deadline {
            if let Some(h) = read_hook(window).await {
                if pred(&h) {
                    return Some(h);
                }
                last = Some(h);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        last
    }

    fn slot_done(v: Option<&Value>) -> bool {
        v.and_then(|x| x.get("state")).and_then(Value::as_str) == Some("done")
    }

    /// 签名有效期阶梯：五档 × 两臂（带签名 / 摘掉签名）。
    fn ladder_done(h: &Value) -> bool {
        let Some(ladder) = h.get("ladder").and_then(Value::as_object) else {
            return false;
        };
        if ladder.len() < 5 {
            return false;
        }
        ladder
            .values()
            .all(|stage| slot_done(stage.get("signed")) && slot_done(stage.get("unsigned")))
    }

    // ---- 主体 ---------------------------------------------------------------

    /// 五格：**M1（还原后重放正文）** → **M2（痕迹读数）** → **M3（通道）**
    /// → **M4（签名有效期）** → **M5（还原有没有搞坏站点）**。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &url::Url,
        store_reading: Value,
        cookie_before: Option<String>,
    ) -> Value {
        let mut report = json!({
            "site": "minimax",
            "probe": "official_chat_minimax_hook_probe",
            "round": std::env::var("DSH_PROBE_ROUND").unwrap_or_else(|_| "r0".to_string()),
            "purpose": "M1-M5：还原后能不能重放正文 / 还原后的痕迹读数 / 站点那一发走哪条通道 / 签名有效期的真实长度 / 还原有没有把站点搞坏",
            "site_url": SITE_URL,
            "catalog_path": CATALOG_PATH,
            "control_path": CONTROL_PATH,
            "residual_path": RESIDUAL_PATH,
            "message_path_shape": MESSAGE_PATH_SHAPE,
            "store": store_reading,
            "hidden_window": hidden_window(),
            "source_cookie_sha256_before": cookie_before.clone().map(Value::from).unwrap_or(Value::Null),
            "discipline": "凭据（token/签名/cookie）与会话 id 全程留在页内；读数只有头名、长度、结构键名、条数、状态码、字节数",
            "notes": [
                "重放那一发走 suppress ⇒ **不进** requests/responses ⇒ 「站点自己那一发」与「我们的重放」可分辨",
                "还原是**分层**的：顶层包装换回它替换掉的那个函数；内层包装只能变纯转发 ⇒ 用残留探针实测内层还在不在",
                "M4 每一档都配一次「摘掉签名」的反向对照 ⇒ 免得把「站点根本不校验这一档」读成「签名一直有效」",
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

        // ── S2：等**目录那一发**被截到（截到 ⇒ 页内立刻还原）───────────────
        let phase_a = wait_until(&window, Duration::from_secs(150), |h| {
            h.get("catalogCapturedAt")
                .map(|v| !v.is_null())
                .unwrap_or(false)
                && h.get("restoreResult")
                    .map(|v| !v.is_null())
                    .unwrap_or(false)
        })
        .await;
        let captured = phase_a
            .as_ref()
            .and_then(|h| h.get("catalogCapturedAt"))
            .map(|v| !v.is_null())
            .unwrap_or(false);
        log_line("S2", &format!("目录那一发截到并已还原：{captured}"));
        report["s2_catalog_captured"] = json!(captured);

        // ── M3：站点那一发走的是哪条通道 ───────────────────────────────────
        let transport = phase_a
            .as_ref()
            .and_then(|h| h.get("catalogCapturedTransport"))
            .cloned()
            .unwrap_or(Value::Null);
        report["m3_channel"] = json!({
            "catalog_via": transport,
            "first_signed_api_via": phase_a
                .as_ref()
                .and_then(|h| h.get("firstSignedApiTransport"))
                .cloned()
                .unwrap_or(Value::Null),
            "first_signed_api_at_ms_since_doc_start": phase_a
                .as_ref()
                .and_then(|h| h.get("firstSignedApiAt"))
                .and_then(Value::as_u64)
                .and_then(|t| h_base_at(phase_a.as_ref().unwrap()).map(|b| t.saturating_sub(b))),
            "note": "via=fetch ⇒ 生产上只需猴补一条通道；两条都补 ⇒ 多余的痕迹",
        });

        // ── M2：还原前后各一份痕迹读数 ─────────────────────────────────────
        report["m2_trace"] = phase_a
            .as_ref()
            .and_then(|h| h.get("restoreResult"))
            .cloned()
            .unwrap_or(Value::Null);

        // ── M1：还原后重放正文（等两条都跑完）──────────────────────────────
        let m1_hook = wait_until(&window, Duration::from_secs(60), |h| {
            h.get("m1")
                .and_then(|m| m.get("body"))
                .map(|b| slot_done(b.get("signed")) && slot_done(b.get("unsigned")))
                .unwrap_or(false)
        })
        .await;
        report["m1"] = json!({
            "body_replays": m1_hook
                .as_ref()
                .and_then(|h| h.get("m1"))
                .cloned()
                .unwrap_or(Value::Null),
            "body_prep": m1_hook
                .as_ref()
                .and_then(|h| h.get("bodyPrep"))
                .cloned()
                .unwrap_or(Value::Null),
        });
        log_line("M1", "正文重放读数取完");

        // ── M4：签名有效期阶梯（+0/+30/+60/+120/+300 秒，每档带反向对照）────
        if skip_ladder() {
            report["m4_ladder"] = json!({ "skipped": true, "reason": "DSH_PROBE_SKIP_LADDER=1（M4 已在 r11 单独量过）" });
            report["m4_ladder_complete"] = json!(false);
            log_line("M4", "阶梯已跳过（DSH_PROBE_SKIP_LADDER=1）");
        } else {
            let ladder_hook = wait_until(&window, Duration::from_secs(360), ladder_done).await;
            report["m4_ladder"] = ladder_hook
                .as_ref()
                .and_then(|h| h.get("ladder"))
                .cloned()
                .unwrap_or(Value::Null);
            report["m4_ladder_complete"] =
                json!(ladder_hook.as_ref().map(ladder_done).unwrap_or(false));
            log_line("M4", "签名有效期阶梯读数取完");
        }

        // ── M5：还原之后站点自己再发一次请求 ───────────────────────────────
        //    ⚠️ **必须排在残留探针之前**：残留探针**不带 suppress** ⇒ 它自己
        //    也会进 `requests`/`responses` ⇒ 排在前面会污染 M5 的读数。
        let perf_before = call_page(&window, "window.__mmxHook.readPerf()").await;
        let trigger = call_page(&window, "window.__mmxHook.doM5Trigger()").await;
        // 等站点自己再发（**条件等待**：等我们的记录里出现还原之后的新请求）。
        let after_restore_requests = wait_until(&window, Duration::from_secs(50), |h| {
            h.get("restoreResult")
                .and_then(|r| r.get("at"))
                .and_then(Value::as_u64)
                .map(|restore_at| {
                    h.get("requests")
                        .and_then(Value::as_array)
                        .map(|rs| {
                            rs.iter().any(|r| {
                                r.get("at")
                                    .and_then(Value::as_u64)
                                    .map(|t| t > restore_at)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .await;
        let perf_after = call_page(&window, "window.__mmxHook.readPerf()").await;
        // 还原时刻（页内给的毫秒时间戳）⇒ 用来切「还原之后」这一段。
        let restore_at = phase_a
            .as_ref()
            .and_then(|h| h.get("restoreResult"))
            .and_then(|r| r.get("at"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        // 还原之后站点自己发的请求（**只报路径形状与状态码**）。
        let post_restore_requests: Vec<Value> = after_restore_requests
            .as_ref()
            .and_then(|h| h.get("requests"))
            .and_then(Value::as_array)
            .map(|rs| {
                rs.iter()
                    .filter(|r| r.get("at").and_then(Value::as_u64).unwrap_or(0) > restore_at)
                    .map(|r| {
                        json!({
                            "at": r.get("at").cloned().unwrap_or(Value::Null),
                            "via": r.get("via").cloned().unwrap_or(Value::Null),
                            "url": r.get("url").cloned().unwrap_or(Value::Null),
                            "signed": r.get("signed").cloned().unwrap_or(Value::Null),
                            "header_names": r
                                .get("headerShape")
                                .and_then(|h| h.get("names"))
                                .cloned()
                                .unwrap_or(Value::Null),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let post_restore_responses: Vec<Value> = after_restore_requests
            .as_ref()
            .and_then(|h| h.get("responses"))
            .and_then(Value::as_array)
            .map(|rs| {
                rs.iter()
                    .filter(|r| r.get("at").and_then(Value::as_u64).unwrap_or(0) > restore_at)
                    .map(|r| {
                        json!({
                            "at": r.get("at").cloned().unwrap_or(Value::Null),
                            "via": r.get("via").cloned().unwrap_or(Value::Null),
                            "url": r.get("url").cloned().unwrap_or(Value::Null),
                            "status": r
                                .get("summary")
                                .and_then(|s| s.get("status"))
                                .cloned()
                                .unwrap_or(Value::Null),
                            "bizCode": r
                                .get("summary")
                                .and_then(|s| s.get("bizCode"))
                                .cloned()
                                .unwrap_or(Value::Null),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        // 新文档臂：再导航一次同一页 ⇒ 站点自己那一发仍要 200。
        // ⚠️ 必须等**新文档**（`docInstalledAt` 变大）⇒ 否则会读到旧文档的
        //    记录（r10 实测踩过：读到的是第一次捕获那条，at 早了 300 秒）。
        let doc_installed_at = after_restore_requests
            .as_ref()
            .and_then(|h| h.get("docInstalledAt"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let reload = call_page(&window, "window.__mmxHook.doReload()").await;
        let reload_hook = wait_until(&window, Duration::from_secs(150), |h| {
            let newer = h
                .get("docInstalledAt")
                .and_then(Value::as_u64)
                .map(|t| t > doc_installed_at)
                .unwrap_or(false);
            let catalog = h
                .get("responses")
                .and_then(Value::as_array)
                .map(|rs| {
                    rs.iter().any(|r| {
                        r.get("url")
                            .and_then(Value::as_str)
                            .map(|u| u.contains("/sidebar/session/tree"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            newer && catalog
        })
        .await;
        let reload_catalog = reload_hook
            .as_ref()
            .and_then(|h| h.get("responses"))
            .and_then(Value::as_array)
            .and_then(|rs| {
                rs.iter().rev().find(|r| {
                    r.get("url")
                        .and_then(Value::as_str)
                        .map(|u| u.contains("/sidebar/session/tree"))
                        .unwrap_or(false)
                })
            })
            .cloned()
            .unwrap_or(Value::Null);
        let post_restore_non_2xx = post_restore_responses
            .iter()
            .filter(|r| {
                r.get("status")
                    .and_then(Value::as_u64)
                    .map(|s| !(200..300).contains(&s))
                    .unwrap_or(false)
            })
            .count();
        report["m5"] = json!({
            "trigger": trigger.unwrap_or(Value::Null),
            "perf_before": perf_before.unwrap_or(Value::Null),
            "perf_after": perf_after.unwrap_or(Value::Null),
            "site_requests_after_restore": post_restore_requests.len(),
            "site_requests_after_restore_detail": post_restore_requests,
            "site_responses_after_restore_count": post_restore_responses.len(),
            "site_responses_after_restore_non_2xx": post_restore_non_2xx,
            "site_responses_after_restore_detail": post_restore_responses,
            "restore_at_ms": restore_at,
            "reload_call": reload.unwrap_or(Value::Null),
            "reload_is_new_document": reload_hook
                .as_ref()
                .and_then(|h| h.get("docInstalledAt"))
                .and_then(Value::as_u64)
                .map(|t| t > doc_installed_at)
                .unwrap_or(false),
            "reload_catalog_response": reload_catalog,
        });
        log_line("M5", "M5 读数取完");

        // ── M2 续：残留探针（还原后我们那层还在不在链上）────────────────────
        //    ⚠️ 排在 M5 **之后** ⇒ 不污染 M5 的读数。
        let residual = call_page(&window, "window.__mmxHook.doResidualProbe()").await;
        report["m2_residual_probe"] = residual.clone().unwrap_or(Value::Null);
        // 等它跑完（**条件等待**：等我们的记录里出现它）。
        let residual_hook = wait_until(&window, Duration::from_secs(12), |h| {
            h.get("residualProbe")
                .and_then(|p| p.get("result"))
                .map(|r| slot_done(Some(r)))
                .unwrap_or(false)
        })
        .await;
        let residual_seen = residual_hook
            .as_ref()
            .and_then(|h| h.get("requests"))
            .and_then(Value::as_array)
            .map(|rs| {
                rs.iter().any(|r| {
                    r.get("url")
                        .and_then(Value::as_str)
                        .map(|u| u.contains("__probe_residual__"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        report["m2_residual_seen_by_hook"] = json!(residual_seen);
        log_line("M2", &format!("残留探针被我们的记录看到：{residual_seen}"));

        // ── 收尾：整份页内记录（可复核）＋ 清理页内槽位 ────────────────────
        let final_page = read_hook(&window).await;
        report["page_record"] = final_page.clone().unwrap_or(Value::Null);

        let cleanup = call_page(&window, "window.__mmxHook.doCleanup()").await;
        report["cleanup"] = cleanup.clone().unwrap_or(Value::Null);
        // 清理**之后**直接在页里数（不依赖任何槽位）。
        let after_cleanup_own = crate::script_eval::read_json(
            &window,
            "Object.getOwnPropertyNames(window).length",
            Duration::from_secs(5),
        )
        .await
        .ok();
        let after_cleanup_probe_names = crate::script_eval::read_json(
            &window,
            "Object.getOwnPropertyNames(window).filter(function(n){return String(n).indexOf('__dsh')===0||String(n).indexOf('__mmx')===0;})",
            Duration::from_secs(5),
        )
        .await
        .ok();
        report["after_cleanup"] = json!({
            "window_own_count": after_cleanup_own.unwrap_or(Value::Null),
            "probe_own_names": after_cleanup_probe_names.unwrap_or(Value::Null),
        });

        // ── 源 cookie 的 sha256（跑前跑后必须一样）─────────────────────────
        let cookie_after = source_cookie_sha256();
        report["source_cookie_sha256_after"] =
            cookie_after.clone().map(Value::from).unwrap_or(Value::Null);
        report["source_cookie_unchanged"] = match (&cookie_before, &cookie_after) {
            (Some(a), Some(b)) => json!(a == b),
            _ => json!(null),
        };

        report
    }

    /// 猴补前的基线时间（document-start）。
    fn h_base_at(h: &Value) -> Option<u64> {
        h.get("base")
            .and_then(|b| b.get("at"))
            .and_then(Value::as_u64)
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
