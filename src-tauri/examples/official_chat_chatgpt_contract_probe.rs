//! ChatGPT（chatgpt.com）路线 B 的**页内取数契约**探针（macOS 真机）。
//!
//! # 它回答什么（每格都有对照）
//!
//! ① **登录态**：壳侧那份 WK 存储里**有没有** chatgpt.com 的登录态
//!    ⇒ cookie **名**（只报名与长度，**值一步都不出页**）＋ 页面**账号态**证据
//!    （`/api/auth/session` ／ `/backend-api/me` 的**响应形状**）。
//! ② **目录那条**（`GET /backend-api/conversations`）在**页内同源 fetch** 下通不通：
//!    · **形态 A**：只靠**同源 cookie**（`credentials:'include'`，**不带**任何显式认证头）；
//!    · **形态 B**：显式 `Authorization: Bearer <某个 cookie 的值>`；
//!    · **形态 C**：带 `oai-device-id`（**站点自己**发的那一个）。
//!    每种形态记：HTTP 状态码、响应体**顶层键名**、条目条数、条目字段键名、
//!    业务错误**键名**与**码**。
//! ③ **正文那条**（`GET /backend-api/conversations/<真实会话 id>`）的形状：
//!    状态码、顶层键名、`$.messages` 条数、消息对象键名、**角色取值集合**、
//!    `content` 的形状（对象还是字符串、里面有哪些键）。
//! ④ **反向验（必须有）**：同源但**不存在**的路径（`/backend-api/__probe_control__`）
//!    **必须**非 200 ⇒ 否则前面那些 ok **不能**当证据。
//! ⑤ **形态是否随页面状态变化**：「刚导航完」与「页面加载完」**两组读数都记**。
//!
//! # 为什么必须自己测（不能照抄路线 A 的读数）
//!
//! 现有规格表里 ChatGPT 的端点（`/backend-api/conversations` ＋
//! `/backend-api/conversations/<id>`、`$.items` ／ `$.messages`）是**路线 A
//! （CDP 抓网络）** 得来的 ⇒ ⇒ **路线 B 是否同形没有验证过** ⇒ ⇒ 这正是本探针。
//!
//! # 隔离（判据 `probe-profile-isolation` ／ `probe-data-directory`）
//!
//! 源是用户真实那份 WK 存储 ⇒ **只读拷贝**到探针自己的目录
//! （`~/Library/WebKit/<探针 exe 名>/WebsiteDataStore/<identifier 的 hex>/`）
//! ⇒ ⇒ 探针写的全在**拷贝**里 ⇒ 用户正在跑的那份存储**一个字节都不改**
//! ⇒ ⇒ 跑前／跑后比**源**那份 `Cookies.binarycookies` 的 sha256 作为证据。
//!
//! # 读数纪律
//!
//! **不报**：任何 cookie ／ token ／ 设备 id 的**值**、会话标题原文、会话 id。
//! **报**：cookie **名**与**长度**、键名、状态码、业务码、条数、形状。
//!
//! 这条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块编译掉。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe: 本探针只支持 macOS（WKWebsiteDataStore 路线）。");
    std::process::exit(2);
}

/// **看门狗**：探针卡住时**自己**退出，不占着整轮等待。
#[cfg(target_os = "macos")]
const WATCHDOG_SECONDS: u64 = 300;

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

/// 探针自己的数据存储标识符（**不是**生产的任何一个）。
///
/// ⚠️ **纪律**：探针**绝不能**起在用户真实那份档案上（浏览器一启动就写
/// Cookies／LocalStorage）⇒ ⇒ 生产那两个标识符在探针里**一个都不许出现**。
#[cfg(target_os = "macos")]
const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-gpt-001";

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::{json, Value};

    /// 探针自己的数据存储标识符（根级那个）。
    use crate::PROBE_DATA_STORE_IDENTIFIER;

    /// **源存储目录**从哪个环境变量读（源码里不写路径字面量）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// ChatGPT 的入口地址（生产规格表里那个 `root_url`）。
    const SITE_URL: &str = "https://chatgpt.com";

    /// 目录端点（生产规格表里那个 `catalog_path`）。
    const CATALOG_PATH: &str = "/backend-api/conversations";

    /// 正文端点模板（生产规格表里那个 `body_path_template`）。
    const BODY_PATH_TEMPLATE: &str = "/backend-api/conversations/{session}";

    /// **反向验**用的不存在的同源路径（必须非 200）。
    const CONTROL_PATH: &str = "/backend-api/__probe_control__";

    /// **账号态**探针端点（都只看**响应形状**，不看值）。
    const AUTH_PATHS: [&str; 2] = ["/api/auth/session", "/backend-api/me"];

    /// **站点自有请求拦截器**（在页面脚本之前注入）。
    ///
    /// ## 它回答什么
    ///
    /// 「站点自己**到底打的是哪个端点、带哪些头**」——**不靠猜**。
    ///
    /// ## 只记录 URL 的**路径 + query 键名 + 值长度**
    ///
    /// **不记录**请求体、**不记录**请求头的值 ⇒ ⇒ 凭据与内容都不进记录。
    /// 唯一的例外：`oai-device-id` 的**值**会**留在页内**（用于形态 C 重放），
    /// 但**绝不进读数**（只报「有没有」与长度）。
    const INTERCEPT_SCRIPT: &str = r#"
(function () {
  var rec = [];
  var raws = [];
  var hdrVals = {};
  var hdrAuth = [];
  var shapes = [];
  function maskQuery(u) {
    // **只留路径 + query 键名 + 值长度**（不报值）
    try {
      var s = String(u || '');
      var h = s, q = '';
      var i = s.indexOf('?');
      if (i >= 0) { h = s.slice(0, i); q = s.slice(i + 1); }
      var origin = '';
      try { origin = String(location.origin); } catch (e) {}
      var path = h;
      if (origin && h.indexOf(origin) === 0) path = h.slice(origin.length);
      var keys = [];
      if (q) {
        var parts = q.split('&');
        for (var pi = 0; pi < parts.length; pi++) {
          var t = parts[pi];
          if (!t) continue;
          var eq = t.indexOf('=');
          var k = eq >= 0 ? t.slice(0, eq) : t;
          var v = eq >= 0 ? t.slice(eq + 1) : '';
          var kk = k;
          try { kk = decodeURIComponent(k); } catch (e) {}
          keys.push({ key: kk, len: v.length });
        }
      }
      return { path: path, queryKeys: keys };
    } catch (e) { return { path: '', queryKeys: [] }; }
  }
  function headerNames(h) {
    var names = [];
    try {
      if (h && typeof h.forEach === 'function') { h.forEach(function (v, k) { names.push(String(k)); }); }
      else if (h && typeof h === 'object') { names = Object.keys(h); }
    } catch (e) {}
    return names;
  }
  function headerValue(h, want) {
    try {
      var found = '';
      if (h && typeof h.forEach === 'function') {
        h.forEach(function (v, k) { if (String(k).toLowerCase() === want) found = String(v); });
      } else if (h && typeof h === 'object') {
        var keys = Object.keys(h);
        for (var i = 0; i < keys.length; i++) {
          if (String(keys[i]).toLowerCase() === want) found = String(h[keys[i]]);
        }
      }
      return found;
    } catch (e) { return ''; }
  }
  function record(url, method, headers) {
    try {
      var m = maskQuery(url);
      m.method = String(method || 'GET');
      m.headerNames = headerNames(headers);
      rec.push(m);
      raws.push(String(url || ''));
      var dev = headerValue(headers, 'oai-device-id');
      if (dev && !hdrVals['oai-device-id']) hdrVals['oai-device-id'] = dev;
      var auth = headerValue(headers, 'authorization');
      if (auth && hdrAuth.indexOf(auth) < 0 && hdrAuth.length < 8) hdrAuth.push(auth);
      if (rec.length > 120) { rec.shift(); raws.shift(); }
    } catch (e) {}
  }
  // **站点自己那一发的响应形状**（状态码 + 顶层键名 + 条数）⇒ ⇒ 站点在
  // **同一个页面、同一个 origin、同一份 cookie** 下拿到什么，就是读数。
  // ⚠️ **只记形状**，不记正文、不记会话 id、不记任何凭据。
  function recordShape(url, status, payload, readBody) {
    try {
      var m = maskQuery(url);
      var s = {
        path: m.path,
        queryKeys: m.queryKeys,
        status: status,
        len: 0,
        topKeys: [],
        itemsLen: -1,
        messagesLen: -1
      };
      var j = null;
      if (payload && typeof payload === 'object') {
        j = payload;
        try { s.len = JSON.stringify(payload).length; } catch (e) {}
      } else if (readBody) {
        var text = String(payload == null ? '' : payload);
        s.len = text.length;
        try { j = JSON.parse(text); } catch (e) {}
      }
      if (j && typeof j === 'object') {
        try { s.topKeys = Object.keys(j).slice(0, 16); } catch (e) {}
        if (Array.isArray(j.items)) s.itemsLen = j.items.length;
        if (Array.isArray(j.messages)) s.messagesLen = j.messages.length;
      }
      shapes.push(s);
      if (shapes.length > 80) shapes.shift();
    } catch (e) {}
  }
  // **只对关心的路径读响应体**（r6 实测：站点自己会拉 **2.4MB 的 CDN JSON**
  // ⇒ 把它也 clone+text 读一遍会**拖慢事件循环** ⇒ ⇒ 下一阶段的页内回程
  // 直接超时 ⇒ ⇒ 只读 `/backend-api/` 与 `/api/auth/` 这两族）。
  function caresAboutBody(path) {
    try {
      var p = String(path || '');
      return p.indexOf('/backend-api/') === 0 || p.indexOf('/api/auth/') === 0;
    } catch (e) { return false; }
  }
  function hook() {
    try {
      if (window.fetch && !window.fetch.__gptHooked) {
        var origFetch = window.fetch;
        var wrapped = function (input, init) {
          try {
            var u = (typeof input === 'string') ? input : ((input && input.url) || '');
            var h = (init && init.headers) || (input && input.headers);
            record(u, (init && init.method) || (input && input.method) || 'GET', h);
          } catch (e) {}
          var p = origFetch.apply(this, arguments);
          // **站点自己那一发的响应形状**。
          //
          // ⚠️ **踩过的坑（r2–r5 恒空的原因）**：`origFetch.apply()` 返回的是
          // **Promise**，**不是** Response ⇒ ⇒ `p.clone` 是 `undefined` ⇒ ⇒
          // 那一分支**从不进入** ⇒ ⇒ 记录恒空（连**自己**发的 fetch 也没有）。
          // ⇒ ⇒ 必须 `p.then(res => res.clone())`。
          // ⇒ ⇒ 下面那几个计数器是**自证**：包装器被调用几次 / 真的记到几次。
          try {
            window.__gptFetchSeen = (window.__gptFetchSeen || 0) + 1;
            if (p && typeof p.then === 'function') {
              p.then(function (res) {
                try {
                  window.__gptFetchResolved = (window.__gptFetchResolved || 0) + 1;
                  if (res && typeof res.clone === 'function') {
                    var c = res.clone();
                    if (c && typeof c.text === 'function') {
                      c.text().then(
                        function (t) {
                          window.__gptShapeOk = (window.__gptShapeOk || 0) + 1;
                          recordShape(u, res.status, t, caresAboutBody(u));
                        },
                        function () { window.__gptShapeErr = (window.__gptShapeErr || 0) + 1; }
                      );
                    }
                  } else {
                    window.__gptNoClone = (window.__gptNoClone || 0) + 1;
                  }
                } catch (e) { window.__gptShapeErr = (window.__gptShapeErr || 0) + 1; }
              }, function () { window.__gptFetchRejected = (window.__gptFetchRejected || 0) + 1; });
            }
          } catch (e) {}
          return p;
        };
        try { wrapped.__gptHooked = true; } catch (e) {}
        window.fetch = wrapped;
      }
    } catch (e) {}
    try {
      if (XMLHttpRequest.prototype.open && !XMLHttpRequest.prototype.open.__gptHooked) {
        var XO = XMLHttpRequest.prototype.open;
        var XS = XMLHttpRequest.prototype.setRequestHeader;
        var wrappedOpen = function (method, url) {
          try {
            this.__gptHeaders = [];
            var names = this.__gptHeaders;
            try { this.__gptUrl = String(url || ''); } catch (e) {}
            if (!this.__gptSetHooked) {
              var wrappedSet = function (name, value) {
                try { this.__gptHeaders.push(String(name)); } catch (e) {}
                return XS.apply(this, arguments);
              };
              try { this.__gptSetHooked = true; } catch (e) {}
              this.setRequestHeader = wrappedSet;
            }
            record(url, method, []);
            var self = this;
            setTimeout(function () { try { record(url, method, self.__gptHeaders || []); } catch (e) {} }, 0);
          } catch (e) {}
          return XO.apply(this, arguments);
        };
        try { wrappedOpen.__gptHooked = true; } catch (e) {}
        XMLHttpRequest.prototype.open = wrappedOpen;
        // XHR 的**响应**形状（ChatGPT 站点也可能走 XHR）
        try {
          if (!XMLHttpRequest.prototype.send.__gptHooked) {
            var origSend = XMLHttpRequest.prototype.send;
            var wrappedSend = function () {
              try {
                var self = this;
                this.addEventListener('load', function () {
                  try {
                    var payload = null;
                    var readBody = caresAboutBody(self.__gptUrl || '');
                    if (readBody) {
                      try { payload = self.response; } catch (e) {}
                      if (payload == null) { try { payload = self.responseText; } catch (e) {} }
                    }
                    recordShape(self.__gptUrl || '', self.status, payload, readBody);
                  } catch (e) {}
                });
              } catch (e) {}
              return origSend.apply(this, arguments);
            };
            try { wrappedSend.__gptHooked = true; } catch (e) {}
            XMLHttpRequest.prototype.send = wrappedSend;
          }
        } catch (e) {}
      }
    } catch (e) {}
  }
  hook();
  // 站点自己可能会**覆盖** fetch ⇒ ⇒ 定时补挂（抓到它后来的请求）。
  try { setInterval(hook, 50); } catch (e) {}
  window.__gptReqs = rec;
  window.__gptRaw = raws;
  window.__gptHeaderValues = hdrVals;
  window.__gptAuthHeaders = hdrAuth;
  window.__gptShapes = shapes;
})()
"#;

    /// 页内契约脚本：把**页内事实**与**几种形态的取数结果**一起带回来。
    ///
    /// ⚠️ **凭据的值只留在页内**：脚本只在结果里报**长度** ⇒ ⇒ Rust 侧
    /// **一个字节的 cookie／token 都拿不到** ⇒ ⇒ 凭据零落盘这条纪律在探针里也成立。
    ///
    /// ⚠️ **回对象，不回 JSON 文本**（与生产 `page_fetch_script.js` 同契约）。
    const CONTRACT_SCRIPT: &str = r#"
(async function () {
  var out = { ok: true };

  function head(text, n) {
    var s = String(text == null ? '' : text);
    if (s.length > n) s = s.slice(0, n);
    var last = s.length ? s.charCodeAt(s.length - 1) : 0;
    if (last >= 0xD800 && last <= 0xDBFF) s = s.slice(0, s.length - 1);
    return s;
  }

  function keysOf(obj) {
    try {
      if (obj && typeof obj === 'object') return Object.keys(obj).slice(0, 24);
    } catch (e) {}
    return [];
  }

  // 业务错误：**只报键名 + 码**（字符串值只报长度，**不报整段文本**）。
  function bizCodeOf(j) {
    try {
      if (!j || typeof j !== 'object') return null;
      var ks = Object.keys(j);
      for (var i = 0; i < ks.length; i++) {
        var k = ks[i];
        if (!/code|error|detail|message|status|reason/i.test(k)) continue;
        var v = j[k];
        if (typeof v === 'number') return { key: k, kind: 'number', value: v };
        if (typeof v === 'string') return { key: k, kind: 'string', len: v.length };
        if (v && typeof v === 'object') {
          if (typeof v.code === 'number') return { key: k + '.code', kind: 'number', value: v.code };
          return { key: k, kind: 'object', nestedKeys: keysOf(v) };
        }
      }
    } catch (e) {}
    return null;
  }

  function shape(text, status, label) {
    var s = {
      label: label,
      status: status,
      len: text.length,
      json: false,
      topKeys: [],
      itemsLen: -1,
      totalCount: -1,
      limitCount: -1,
      offsetCount: -1,
      itemKeys: [],
      itemHasId: false,
      itemIdLen: 0,
      bizCode: null,
      _ids: []
    };
    var j = null;
    try { j = JSON.parse(text); s.json = true; } catch (e) { s.parseError = true; }
    if (j && typeof j === 'object') {
      s.topKeys = keysOf(j);
      s.bizCode = bizCodeOf(j);
      if (Array.isArray(j.items)) {
        s.itemsLen = j.items.length;
        if (typeof j.total === 'number') s.totalCount = j.total;
        if (typeof j.limit === 'number') s.limitCount = j.limit;
        if (typeof j.offset === 'number') s.offsetCount = j.offset;
        for (var i = 0; i < j.items.length && i < 60; i++) {
          var it = j.items[i] || {};
          if (i === 0) s.itemKeys = keysOf(it);
          if (typeof it.id === 'string') {
            s.itemHasId = true;
            if (i === 0) s.itemIdLen = it.id.length;
            s._ids.push(it.id);
          }
        }
      }
    }
    return s;
  }

  function bodyShape(text, status, label) {
    var s = {
      label: label,
      status: status,
      len: text.length,
      json: false,
      topKeys: [],
      messagesLen: -1,
      messageKeys: [],
      authorKeys: [],
      roles: [],
      contentShapes: [],
      bizCode: null
    };
    var j = null;
    try { j = JSON.parse(text); s.json = true; } catch (e) { s.parseError = true; }
    if (j && typeof j === 'object') {
      s.topKeys = keysOf(j);
      s.bizCode = bizCodeOf(j);
      var msgs = Array.isArray(j.messages) ? j.messages : null;
      if (msgs) {
        s.messagesLen = msgs.length;
        var roleSet = {};
        var keyUnion = [];
        for (var i = 0; i < msgs.length && i < 300; i++) {
          var m = msgs[i] || {};
          var mk = keysOf(m);
          for (var k = 0; k < mk.length; k++) {
            if (keyUnion.indexOf(mk[k]) < 0 && keyUnion.length < 30) keyUnion.push(mk[k]);
          }
          if (i === 0) {
            try { if (m.author && typeof m.author === 'object') s.authorKeys = keysOf(m.author); } catch (e) {}
          }
          var role = null;
          try { if (m.author && typeof m.author === 'object' && m.author.role != null) role = String(m.author.role); } catch (e) {}
          if (role) roleSet[role] = (roleSet[role] || 0) + 1;
          if (s.contentShapes.length < 8) {
            var c = m.content;
            var cs = { role: role, contentType: typeof c };
            if (c && typeof c === 'object') {
              cs.keys = keysOf(c);
              cs.innerContentType = typeof c.content;
              if (typeof c.content === 'string') cs.innerContentLen = c.content.length;
              if (Array.isArray(c.parts)) {
                cs.partsLen = c.parts.length;
                var pt = {};
                for (var p = 0; p < c.parts.length; p++) {
                  var t = typeof c.parts[p];
                  pt[t] = (pt[t] || 0) + 1;
                }
                cs.partTypes = pt;
              }
            } else if (typeof c === 'string') {
              cs.innerContentLen = c.length;
            }
            s.contentShapes.push(cs);
          }
        }
        s.messageKeys = keyUnion;
        s.roles = Object.keys(roleSet).map(function (r) { return { role: r, count: roleSet[r] }; });
      }
    }
    return s;
  }

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

  async function attempt(label, url, headers, creds) {
    try {
      var opts = { credentials: creds || 'include' };
      if (headers) opts.headers = headers;
      var res = await fetch(url, opts);
      var text = await res.text();
      var s = shape(text, res.status, label);
      s.ok = res.ok;
      s.contentType = String((res.headers.get('content-type') || '')).split(';')[0];
      return s;
    } catch (e) {
      return { label: label, error: String(e), status: -1 };
    }
  }

  async function bodyAttempt(label, url, headers) {
    try {
      var opts = { credentials: 'include' };
      if (headers) opts.headers = headers;
      var res = await fetch(url, opts);
      var text = await res.text();
      var s = bodyShape(text, res.status, label);
      s.ok = res.ok;
      return s;
    } catch (e) {
      return { label: label, error: String(e), status: -1 };
    }
  }

  // ---------- 页面事实 ----------
  out.origin = String(location.origin);
  out.host = String(location.host);
  out.href = String(location.href);
  out.readyState = String(document.readyState);
  out.pageTitle = String(document.title || '');
  try {
    var inner = String(document.body ? (document.body.innerText || '') : '');
    out.innerTextLength = inner.length;
    out.innerTextHead = head(inner, 220);
  } catch (e) { out.pageStateError = String(e); }

  // ---------- cookie：**名 + 长度**（值不出页） ----------
  var cookies = [];
  try {
    var parts = String(document.cookie || '').split(';');
    for (var i = 0; i < parts.length; i++) {
      var t = parts[i].trim();
      if (!t) continue;
      var eq = t.indexOf('=');
      var name = eq >= 0 ? t.slice(0, eq) : t;
      var val = eq >= 0 ? t.slice(eq + 1) : '';
      cookies.push({ name: name, len: val.length });
    }
  } catch (e) { out.cookieError = String(e); }
  out.cookies = cookies;
  out.cookieCount = cookies.length;

  // ---------- localStorage：**键名 + 长度** ----------
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

  // ---------- 站点自己的请求（路径 + query 键名 + 头**名**） ----------
  var reqs = window.__gptReqs || [];
  out.siteRequests = reqs.slice(0, 60);
  out.siteRequestCount = reqs.length;

  // ---------- 站点自己那一发的**响应形状**（同一页面／同一 origin／同一份 cookie） ----------
  var shapes = window.__gptShapes || [];
  out.siteShapes = shapes.slice(0, 60);
  out.siteShapeCount = shapes.length;
  // **自证计数器**（区分「包装器没被调用」与「clone 失败」）
  out.fetchCounters = {
    seen: window.__gptFetchSeen || 0,
    resolved: window.__gptFetchResolved || 0,
    rejected: window.__gptFetchRejected || 0,
    shapeOk: window.__gptShapeOk || 0,
    shapeErr: window.__gptShapeErr || 0,
    noClone: window.__gptNoClone || 0
  };

  // 站点自己往目录端点打的那几发（**原始 URL 只留在页内**，用来逐字重放）
  // ⚠️ **只认路径**恰好等于目录端点的那些（`/backend-api/conversations/<id>`
  // 那是**正文**那条，不算目录）。
  var CATALOG_PATH_ONLY = '__CATALOG__';
  // ⚠️ **等站点自己把「带 query 的那一发」发出来**（它在首屏之后才发 ⇒
  // 不等就抓不到 ⇒ ⇒ 「生产 URL 该带什么 query」那一格就答不了）。
  var WAIT_SITE = __WAIT_SITE__;
  if (WAIT_SITE) {
    var waited = 0;
    while (waited < 15000) {
      var hasOriginQ = false;
      var rr = window.__gptRaw || [];
      for (var wi = 0; wi < rr.length; wi++) {
        if (String(rr[wi] || '').indexOf('conversation_origin') >= 0) { hasOriginQ = true; break; }
      }
      if (hasOriginQ) break;
      await new Promise(function (r) { setTimeout(r, 500); });
      waited += 500;
    }
    out.siteWaitMs = waited;
  }
  var raws = window.__gptRaw || [];
  var rawCatalog = [];
  var rawBodyUrls = [];
  var seenCatalog = {};
  for (var i = 0; i < raws.length; i++) {
    var u = String(raws[i] || '');
    var pathOnly = u.split('?')[0];
    if (pathOnly.indexOf(CATALOG_PATH_ONLY) !== 0) continue;
    if (pathOnly.length > CATALOG_PATH_ONLY.length) { rawBodyUrls.push(u); continue; }
    if (!seenCatalog[u]) { seenCatalog[u] = true; rawCatalog.push(u); }
  }
  out.siteCatalogRequestCount = rawCatalog.length;
  out.siteBodyRequestCount = rawBodyUrls.length;

  // 每条站点自己的请求的 **query 键名摘要**（用来区分重放的是哪一形态）
  function qSummary(u) {
    try {
      var s = String(u || '');
      var i = s.indexOf('?');
      if (i < 0) return '';
      var q = s.slice(i + 1);
      var parts = q.split('&');
      var names = [];
      for (var p = 0; p < parts.length; p++) {
        var t = parts[p];
        if (!t) continue;
        var eq = t.indexOf('=');
        var k = eq >= 0 ? t.slice(0, eq) : t;
        try { k = decodeURIComponent(k); } catch (e) {}
        names.push(k);
      }
      names.sort();
      return names.join(',');
    } catch (e) { return ''; }
  }

  // ---------- 账号态探针（只看**响应形状**） ----------
  var authProbes = [];
  for (var i = 0; i < __AUTH_PATHS__.length; i++) {
    var p = __AUTH_PATHS__[i];
    var probe = null;
    try {
      var res = await fetch(p, { credentials: 'include' });
      var text = await res.text();
      probe = shape(text, res.status, 'AUTH|' + p);
      probe.path = p;
      try {
        var j = JSON.parse(text);
        if (j && typeof j === 'object') {
          if (j.user && typeof j.user === 'object') { probe.hasUser = true; probe.userKeys = keysOf(j.user); }
          else { probe.hasUser = false; }
          if (j.account && typeof j.account === 'object') { probe.hasAccount = true; probe.accountKeys = keysOf(j.account); }
          else { probe.hasAccount = false; }
          if (j.data && typeof j.data === 'object') { probe.hasData = true; probe.dataKeys = keysOf(j.data); }
        }
      } catch (e) {}
    } catch (e) { probe = { label: 'AUTH|' + p, path: p, error: String(e), status: -1 }; }
    authProbes.push(probe);
  }
  out.authProbes = authProbes;

  // ---------- 形态 D 的凭据：**页内取** `/api/auth/session` 的 accessToken ----------
  // ⚠️ **值只留在页内**；读数里只报**长度**与**键名**。
  var tokenCands = [];
  try {
    var sres = await fetch('/api/auth/session', { credentials: 'include' });
    var stext = await sres.text();
    var sj = JSON.parse(stext);
    if (sj && typeof sj === 'object') {
      if (typeof sj.accessToken === 'string' && sj.accessToken.length) tokenCands.push({ key: 'accessToken', value: sj.accessToken });
      if (typeof sj.sessionToken === 'string' && sj.sessionToken.length) tokenCands.push({ key: 'sessionToken', value: sj.sessionToken });
      try { if (sj.account && typeof sj.account === 'object' && typeof sj.account.id === 'string') tokenCands.push({ key: 'accountId', value: sj.account.id, accountId: true }); } catch (e) {}
    }
  } catch (e) { out.sessionTokenFetchError = String(e); }
  out.sessionTokenCandidates = tokenCands.map(function (c) { return { key: c.key, len: c.value.length }; });

  // 交叉验证：**站点自己那一发**的 `authorization` 头，是不是就是这个 accessToken？
  // （只在页内比对 ⇒ 只报布尔）
  var accessTokenValue = '';
  for (var ti = 0; ti < tokenCands.length; ti++) if (tokenCands[ti].key === 'accessToken') accessTokenValue = tokenCands[ti].value;
  var siteAuthHeaders = window.__gptAuthHeaders || [];
  out.siteAuthHeaderCount = siteAuthHeaders.length;
  out.siteAuthHeaderMatchesAccessToken = null;
  if (accessTokenValue) {
    var matched = false;
    for (var ai = 0; ai < siteAuthHeaders.length; ai++) {
      var av = String(siteAuthHeaders[ai] || '');
      if (av === accessTokenValue) { matched = true; break; }
      if (av.length > 7 && av.slice(0, 7).toLowerCase() === 'bearer ' && av.slice(7) === accessTokenValue) { matched = true; break; }
    }
    out.siteAuthHeaderMatchesAccessToken = matched;
  }

  // ---------- 目录端点：形态 A / B / C + 对照 ----------
  var attempts = [];

  // 反向验 ①：同源但**不存在**的路径（必须非 200）
  attempts.push(await attempt('CTRL-nonexistent-path|cookie-only', '__CONTROL__', null, 'include'));
  // 反向验 ②：**不带 cookie**（证明「身份是 cookie 带上去的」）
  attempts.push(await attempt('CTRL-omit-credentials|bare', '__CATALOG__', null, 'omit'));

  // 形态 A：只靠同源 cookie
  attempts.push(await attempt('A-cookie-only|bare', '__CATALOG__', null, 'include'));
  for (var i = 0; i < rawCatalog.length && i < 8; i++) {
    attempts.push(await attempt('A-cookie-only|site-url#' + i + '|q=' + qSummary(rawCatalog[i]), rawCatalog[i], null, 'include'));
  }

  // 形态 B：显式 `Authorization: Bearer <cookie 值>`
  // 候选：cookie 名里像会话令牌的（**只报名**）＋ localStorage 里名字像 token 的
  var bCandidates = [];
  for (var i = 0; i < cookies.length; i++) {
    // ⚠️ **宽口径**：名字像会话令牌的，**或者**长度 ≥ 40 的（像 `oai-sc` 那种
    // 名字里没有 token/session 字样的）⇒ ⇒ 这样才能回答「**有没有哪个 cookie
    // 能直接当 Bearer 用**」⇒ ⇒ 不能只按名字筛。
    if (cookies[i].len > 0 && (/session|token|auth|bearer/i.test(cookies[i].name) || cookies[i].len >= 40)) {
      bCandidates.push({ from: 'cookie', key: cookies[i].name, value: ckGet(cookies[i].name) });
    }
  }
  for (var i = 0; i < ls.length; i++) {
    if (ls[i].len > 0 && /token|auth|session/i.test(ls[i].key)) {
      var rawLs = '';
      try { rawLs = localStorage.getItem(ls[i].key) || ''; } catch (e) {}
      bCandidates.push({ from: 'localStorage', key: ls[i].key, value: rawLs });
    }
  }
  out.bearerCandidates = bCandidates.map(function (c) { return { from: c.from, key: c.key, len: String(c.value).length }; });
  for (var ci = 0; ci < bCandidates.length && ci < 8; ci++) {
    var c = bCandidates[ci];
    var tag = 'B-authorization|' + c.from + ':' + c.key;
    attempts.push(await attempt(tag + '|bearer|bare', '__CATALOG__', { 'authorization': 'Bearer ' + c.value }, 'include'));
    attempts.push(await attempt(tag + '|raw|bare', '__CATALOG__', { 'authorization': c.value }, 'include'));
  }

  // 形态 C：`oai-device-id`（**站点自己**发的那一个；值只留在页内）
  // 候选来源：站点请求头里抓到的 ／ cookie `oai-device-id` ／ cookie `oai-did`
  var deviceCands = [];
  try { if (window.__gptHeaderValues && window.__gptHeaderValues['oai-device-id']) deviceCands.push({ src: 'site-header', name: 'oai-device-id', value: String(window.__gptHeaderValues['oai-device-id']) }); } catch (e) {}
  var ckDev = ckGet('oai-device-id');
  if (ckDev) deviceCands.push({ src: 'cookie', name: 'oai-device-id', value: ckDev });
  var ckDid = ckGet('oai-did');
  if (ckDid) deviceCands.push({ src: 'cookie', name: 'oai-did', value: ckDid });
  out.deviceCandidates = deviceCands.map(function (c) { return { src: c.src, name: c.name, len: c.value.length }; });
  out.deviceIdPresent = deviceCands.length > 0;
  out.deviceIdLen = deviceCands.length ? deviceCands[0].value.length : 0;
  for (var dc = 0; dc < deviceCands.length; dc++) {
    var d = deviceCands[dc];
    var dh = {};
    dh['oai-device-id'] = d.value;
    attempts.push(await attempt('C-device-header|' + d.src + ':' + d.name, '__CATALOG__', dh, 'include'));
    if (d.name !== 'oai-device-id') {
      var dh2 = {};
      dh2[d.name] = d.value;
      attempts.push(await attempt('C-device-header|' + d.src + ':' + d.name + '|as-' + d.name, '__CATALOG__', dh2, 'include'));
    }
  }

  // 形态 D：**Bearer <页内取到的 token>**
  var accessToken = '';
  for (var di = 0; di < tokenCands.length; di++) if (tokenCands[di].key === 'accessToken') accessToken = tokenCands[di].value;
  for (var tc = 0; tc < tokenCands.length; tc++) {
    var t2 = tokenCands[tc];
    if (t2.accountId) {
      // 形态 E：`ChatGPT-Account-Id` 头（站点自己那一发**带**这个头）
      attempts.push(await attempt('E-account-id-header|' + t2.key, '__CATALOG__', { 'authorization': 'Bearer ' + accessToken, 'ChatGPT-Account-Id': t2.value }, 'include'));
      continue;
    }
    attempts.push(await attempt('D-session-bearer|' + t2.key, '__CATALOG__', { 'authorization': 'Bearer ' + t2.value }, 'include'));
    attempts.push(await attempt('D-session-raw|' + t2.key, '__CATALOG__', { 'authorization': t2.value }, 'include'));
  }
  // 形态 D 的 **URL 变体**：把站点自己那一发（**带 query 的**）逐字重放，
  // 看「生产 `page_route.url` 该带什么 query」。
  if (accessToken) {
    for (var dv = 0; dv < rawCatalog.length && dv < 8; dv++) {
      attempts.push(await attempt('D-bearer|site-url#' + dv + '|q=' + qSummary(rawCatalog[dv]), rawCatalog[dv], { 'authorization': 'Bearer ' + accessToken }, 'include'));
    }
    // 形态 F：`Bearer` ＋ `oai-device-id`（站点自己那一发**带**这个头）
    if (deviceCands.length) {
      var fh = {};
      fh['authorization'] = 'Bearer ' + accessToken;
      fh['oai-device-id'] = deviceCands[0].value;
      attempts.push(await attempt('F-bearer+device-id', '__CATALOG__', fh, 'include'));
    }
    // 形态 G：**分页**（`limit`／`offset`）⇒ 回答「目录能不能取全」
    var pages = ['?limit=2&offset=0', '?limit=2&offset=2', '?limit=50&offset=50'];
    for (var pi = 0; pi < pages.length; pi++) {
      attempts.push(await attempt('G-page|' + pages[pi], '__CATALOG__' + pages[pi], { 'authorization': 'Bearer ' + accessToken }, 'include'));
    }
  }

  // ---------- 正文那条（会话 id **只留在页内**） ----------
  var convId = null;
  var winningLabel = null;
  for (var i = 0; i < attempts.length; i++) {
    var a = attempts[i];
    if (a && a.status === 200 && a.itemsLen > 0 && a._ids && a._ids.length) {
      convId = a._ids[0];
      winningLabel = a.label;
      break;
    }
  }
  // 站点**自己**打的目录／正文请求的响应形状（分出来看）
  out.siteCatalogShapes = [];
  out.siteBodyShapes = [];
  for (var i = 0; i < out.siteShapes.length; i++) {
    var sh = out.siteShapes[i];
    if (!sh) continue;
    var sp = String(sh.path || '');
    if (sp.indexOf('/backend-api/conversations/') === 0) out.siteBodyShapes.push(sh);
    else if (sp.indexOf('/backend-api/conversations') === 0) out.siteCatalogShapes.push(sh);
  }
  var bodyReadings = [];
  if (convId) {
    var bodyUrl = '__BODY__' + encodeURIComponent(convId);
    bodyReadings.push(await bodyAttempt('BODY-A-cookie-only', bodyUrl, null));
    if (bCandidates.length) {
      bodyReadings.push(await bodyAttempt('BODY-B-authorization|' + bCandidates[0].from + ':' + bCandidates[0].key, bodyUrl, { 'authorization': 'Bearer ' + bCandidates[0].value }));
    }
    if (deviceCands.length) {
      var bdh = {};
      bdh['oai-device-id'] = deviceCands[0].value;
      bodyReadings.push(await bodyAttempt('BODY-C-device-header|' + deviceCands[0].src, bodyUrl, bdh));
    }
    if (tokenCands.length && accessToken) {
      bodyReadings.push(await bodyAttempt('BODY-D-session-bearer|accessToken', bodyUrl, { 'authorization': 'Bearer ' + accessToken }));
    }
    // 反向验：不存在的会话 id
    bodyReadings.push(await bodyAttempt('CTRL-nonexistent-session', '__BODY__' + encodeURIComponent('__dsh_probe_no_such_session__'), null));
  }
  out.bodyReadings = bodyReadings;
  out.catalogWinnerLabel = winningLabel ? winningLabel : 'none';
  out.bodyAttempted = !!convId;

  // ---------- 回程：把**内部字段**（会话 id 列表）剔掉 ----------
  var cleanAttempts = [];
  for (var i = 0; i < attempts.length; i++) {
    var a = attempts[i] || {};
    var clean = {};
    for (var k in a) {
      if (Object.prototype.hasOwnProperty.call(a, k) && k !== '_ids') clean[k] = a[k];
    }
    cleanAttempts.push(clean);
  }
  out.attempts = cleanAttempts;

  // ⚠️ **静候一小会儿再取形状快照**：`recordShape` 是在微任务／后续 tick 里跑的
  // （`clone().text()` 是异步的）⇒ ⇒ 脚本末尾立刻取快照 ⇒ ⇒ **我自己那些
  // fetch 的形状**照不上（r6 实测：站点自己早先发的那几条照上了，我自己的没照上）。
  await new Promise(function (r) { setTimeout(r, 500); });
  var shapes2 = window.__gptShapes || [];
  out.siteShapes = shapes2.slice(0, 60);
  out.siteShapeCount = shapes2.length;
  out.fetchCounters = {
    seen: window.__gptFetchSeen || 0,
    resolved: window.__gptFetchResolved || 0,
    rejected: window.__gptFetchRejected || 0,
    shapeOk: window.__gptShapeOk || 0,
    shapeErr: window.__gptShapeErr || 0,
    noClone: window.__gptNoClone || 0
  };
  out.siteCatalogShapes = [];
  out.siteBodyShapes = [];
  for (var i = 0; i < out.siteShapes.length; i++) {
    var sh = out.siteShapes[i];
    if (!sh) continue;
    var sp = String(sh.path || '');
    if (sp.indexOf('/backend-api/conversations/') === 0) out.siteBodyShapes.push(sh);
    else if (sp.indexOf('/backend-api/conversations') === 0) out.siteCatalogShapes.push(sh);
  }

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
        std::env::temp_dir().join(format!("dsh-chatgpt-contract-probe.{round}.log"))
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
                "skipped": report.skipped.len(),
                "note": "源是**只读拷贝**：探针写的全在拷贝里，用户正在跑的那份存储一个字节都没改",
            }),
        ))
    }

    /// **源**那份 cookie 文件的 sha256（跑前／跑后各一次 ⇒ 证明「没被改」）。
    fn source_cookies_sha() -> Option<String> {
        let source = PathBuf::from(std::env::var(ENV_SOURCE).ok()?);
        let path = source.join("Cookies").join("Cookies.binarycookies");
        let bytes = std::fs::read(path).ok()?;
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Some(
            hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }

    /// 建窗（**探针自己的**存储标识符 ⇒ **不碰**用户真实档案）。
    fn build_window(
        app: &tauri::AppHandle,
        label: &str,
        url: &url::Url,
        identifier: &[u8; 16],
    ) -> Result<tauri::WebviewWindow, String> {
        // **无条件**给 `data_directory`（判据 `probe-data-directory` 按**代码
        // 形状**判，「有才给」同样违规）。macOS 上 WKWebView **忽略**它
        // （它认的是 `data_store_identifier`），但照样给——口径一致。
        let profile = std::env::temp_dir().join("dsh-official-chat-chatgpt-contract-probe");
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("无法创建临时 profile 目录（{}）：{e}", profile.display()))?;

        let builder =
            tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(url.clone()))
                .title("dsh ChatGPT 契约探针")
                .inner_size(1000.0, 760.0)
                // ⚠️ **必须可见**：隐藏窗里那个 SPA **不跑**（MiniMax 探针 r5 实测：
                // 站点自己一次 API 都没打）⇒ ⇒ 站点自己的那一发抓不到 ⇒ ⇒ 可见窗
                // 让站点**自己**发，我们只看它的**路径 / query 键名 / 头名**。
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

    /// 等页面就绪：`location.origin` **连续 3 次**不变才算稳（站点会 301）。
    async fn wait_ready(window: &tauri::WebviewWindow) -> Result<Value, String> {
        let mut stable = 0;
        let mut last = String::new();
        for _ in 0..160 {
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

    /// 跑一次页内契约脚本。
    ///
    /// ⚠️ **重试**：页面还在加载时 wry 的求值回调可能被丢掉（见
    /// `script_eval.rs` 里 macOS 那段说明）⇒ ⇒ 传输层失败**重试几次**，
    /// 别把「回程没搭上」当成「站点没数据」。
    async fn run_contract(window: &tauri::WebviewWindow, phase: &str) -> Value {
        let script = CONTRACT_SCRIPT
            .replace("__AUTH_PATHS__", &json_array(AUTH_PATHS))
            .replace("__CONTROL__", CONTROL_PATH)
            .replace("__CATALOG__", CATALOG_PATH)
            .replace("__BODY__", &BODY_PATH_TEMPLATE.replace("{session}", ""))
            // 只有「页面加载完」那一阶段才等站点自己发那一发。
            .replace(
                "__WAIT_SITE__",
                if phase == "late" { "true" } else { "false" },
            );
        let mut last_error = String::new();
        for attempt_no in 1..=3 {
            log_line(
                "contract",
                &format!("跑页内契约脚本（phase={phase}，第 {attempt_no} 次）"),
            );
            match crate::script_eval::round_trip(window, &script, Duration::from_secs(90)).await {
                Ok(trip) => {
                    log_line(
                        "contract",
                        &format!(
                            "phase={phase} 读回（elapsed_ms={}，polls={}）",
                            trip.elapsed_ms, trip.polls
                        ),
                    );
                    return trip.value;
                }
                Err(e) => {
                    last_error = e.to_string();
                    log_line("contract", &format!("phase={phase} 回程失败：{e}"));
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
        json!({ "ok": false, "phase": phase, "error": last_error })
    }

    /// 把一组路径渲染成 JS 数组字面量。
    fn json_array(items: [&str; 2]) -> String {
        let parts: Vec<String> = items.iter().map(|p| format!("\"{p}\"")).collect();
        format!("[{}]", parts.join(", "))
    }

    /// 探针主体。
    pub fn run() {
        // 看门狗：卡住时自己退出。
        std::thread::Builder::new()
            .name("chatgpt-contract-watchdog".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(super::WATCHDOG_SECONDS));
                log_line("WATCHDOG", "超时，探针自己退出");
                std::process::exit(3);
            })
            .expect("probe: 无法起看门狗线程");

        log_line("probe", "run() 入口");
        let sha_before = source_cookies_sha();
        log_line(
            "probe",
            &format!("源 cookie 文件 sha256（前）={:?}", sha_before),
        );

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
                let sha_before = sha_before.clone();
                std::thread::Builder::new()
                    .name("chatgpt-contract-probe".into())
                    .spawn(move || {
                        log_line("probe", "工作线程启动");
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                tauri::async_runtime::block_on(async move {
                                    run_probe_body(&handle, &url, store_reading, sha_before).await
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

    /// 从一份契约读数里**提炼**结论（哪些形态 200 且目录非空、控件是否非 200）。
    fn summarize(phase: &str, value: &Value) -> Value {
        let attempts = value
            .get("attempts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut working = Vec::new();
        let mut controls = Vec::new();
        let mut statuses = Vec::new();
        for attempt in &attempts {
            let label = attempt
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let status = attempt.get("status").and_then(Value::as_i64);
            let items = attempt.get("itemsLen").and_then(Value::as_i64);
            statuses.push(json!({
                "label": label,
                "status": status,
                "itemsLen": items,
                "total": attempt.get("totalCount").cloned().unwrap_or(Value::Null),
                "limit": attempt.get("limitCount").cloned().unwrap_or(Value::Null),
                "offset": attempt.get("offsetCount").cloned().unwrap_or(Value::Null),
                "topKeys": attempt.get("topKeys").cloned().unwrap_or(Value::Null),
                "itemKeys": attempt.get("itemKeys").cloned().unwrap_or(Value::Null),
                "bizCode": attempt.get("bizCode").cloned().unwrap_or(Value::Null),
            }));
            if label.starts_with("CTRL") {
                controls.push(json!({
                    "label": label,
                    "status": status,
                    "is200": status == Some(200),
                    "bizCode": attempt.get("bizCode").cloned().unwrap_or(Value::Null),
                }));
                continue;
            }
            if status == Some(200) && items.map(|n| n > 0).unwrap_or(false) {
                working.push(json!({
                    "label": label,
                    "status": status,
                    "itemsLen": items,
                    "topKeys": attempt.get("topKeys").cloned().unwrap_or(Value::Null),
                    "itemKeys": attempt.get("itemKeys").cloned().unwrap_or(Value::Null),
                }));
            }
        }
        let control_all_non_200 = controls
            .iter()
            .all(|c| c.get("is200").and_then(Value::as_bool) != Some(true));
        json!({
            "phase": phase,
            "working_catalog_forms": working,
            "controls": controls,
            "control_all_non_200": control_all_non_200,
            "catalog_winner_label": value.get("catalogWinnerLabel").cloned().unwrap_or(Value::Null),
            "body_attempted": value.get("bodyAttempted").cloned().unwrap_or(Value::Null),
            "body_readings": value.get("bodyReadings").cloned().unwrap_or(Value::Null),
            "auth_probes": value.get("authProbes").cloned().unwrap_or(Value::Null),
            "cookie_names": value.get("cookies").cloned().unwrap_or(Value::Null),
            "bearer_candidates": value.get("bearerCandidates").cloned().unwrap_or(Value::Null),
            "session_token_candidates": value.get("sessionTokenCandidates").cloned().unwrap_or(Value::Null),
            "site_auth_header_count": value.get("siteAuthHeaderCount").cloned().unwrap_or(Value::Null),
            "site_auth_header_matches_access_token": value
                .get("siteAuthHeaderMatchesAccessToken")
                .cloned()
                .unwrap_or(Value::Null),
            "device_candidates": value.get("deviceCandidates").cloned().unwrap_or(Value::Null),
            "device_id_present": value.get("deviceIdPresent").cloned().unwrap_or(Value::Null),
            "device_id_len": value.get("deviceIdLen").cloned().unwrap_or(Value::Null),
            "site_catalog_request_count": value.get("siteCatalogRequestCount").cloned().unwrap_or(Value::Null),
            "site_body_request_count": value.get("siteBodyRequestCount").cloned().unwrap_or(Value::Null),
            "site_wait_ms": value.get("siteWaitMs").cloned().unwrap_or(Value::Null),
            "site_catalog_shapes": value.get("siteCatalogShapes").cloned().unwrap_or(Value::Null),
            "site_body_shapes": value.get("siteBodyShapes").cloned().unwrap_or(Value::Null),
            "all_attempts": statuses,
        })
    }

    /// 探针主体（异步部分）。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &url::Url,
        store_reading: Value,
        sha_before: Option<String>,
    ) -> Value {
        let mut report = json!({
            "site": "chatgpt",
            "site_url": SITE_URL,
            "catalog_path": CATALOG_PATH,
            "body_path_template": BODY_PATH_TEMPLATE,
            "control_path": CONTROL_PATH,
            "auth_paths": AUTH_PATHS,
            "store": store_reading,
        });

        // ── S1：建窗（绑探针自己的存储）＋ 导航 ＋ 等就绪 ──────────────────
        let window = match build_window(
            app,
            "chatgpt-contract-probe",
            url,
            &PROBE_DATA_STORE_IDENTIFIER,
        ) {
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

        // ── 阶段一：**刚导航完**（origin 已稳，页面未必渲染完） ────────────
        let early = run_contract(&window, "early").await;
        report["phase_early"] = early.clone();
        report["verdict_early"] = summarize("early", &early);

        // ⚠️ **要等 SPA 真的起来**（MiniMax 探针 r6 实测：origin 已稳、正文仍空
        // ⇒ 页面是 SSR 出来的壳，应用还没渲染）⇒ ⇒ 这里**静候 10 秒**再取第二组读数。
        tokio::time::sleep(Duration::from_secs(10)).await;
        log_line("S2", "等 SPA 起来（10 秒）后取第二组读数");

        // ── 阶段二：**页面加载完** ──────────────────────────────────────────
        let late = run_contract(&window, "late").await;
        report["phase_late"] = late.clone();
        report["verdict_late"] = summarize("late", &late);

        // ── 源存储「没被改」的证据（跑前／跑后 sha256） ────────────────────
        let sha_after = source_cookies_sha();
        report["source_store_untouched"] = json!({
            "file": "Cookies/Cookies.binarycookies",
            "sha256_before": sha_before,
            "sha256_after": sha_after,
            "same": sha_before.is_some() && sha_before == sha_after,
            "note": "相等 ⇒ 用户真实那份 cookie 文件**一个字节都没改**（探针写的是拷贝）",
        });

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
