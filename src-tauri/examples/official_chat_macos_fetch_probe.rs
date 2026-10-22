//! macOS「官网引用」路线 B 的**真机实测**探针。
//!
//! # 它要回答什么
//!
//! 台账上那条裁决写的是「macOS 侧无通路」/「不与 Windows 并列成两平台各一种」。
//! 本探针用**本机实测**去检验这条裁决能不能站住——测的是**生产代码本身**，
//! 不是副本。
//!
//! 路线 B 的三块拼图，每块**各自独立取证**（任一块红 ⇒ 路线 B 在 macOS 上
//! 不成立；三块全绿 ⇒ 路线 B 在 macOS 上成立 ⇒ 那条裁决被推翻）：
//!
//! 1. **壳内页内求值成立**：生产的 `script_eval::read_json` 在 macOS 上拿得到值
//!    （wry 的 `WKWebView.evaluateJavaScript` 回调给的是结果的 JSON 序列化）。
//! 2. **异步结果回传成立**：生产的 `script_eval::round_trip`（「脚本自己 await
//!    ＋ 宿主轮询槽位」）在 macOS 上拿得到 **Promise settle 之后**的值。
//!    对照臂同时证明**必须**走这条路：原生求值拿到的是 Promise **对象本身**
//!    （JSON 序列化 = `{}`），不是它 resolve 出来的值。
//! 3. **同源 fetch 成立**：真实 DeepSeek 官网 ＋ 真实登录态下，页内
//!    `fetch('/api/v0/chat_session/fetch_page')` 拿得到站点的响应。
//!
//! # 登录态怎么来的（**只读拷贝**）
//!
//! macOS 上 WKWebView 的持久化存储落在
//! `~/Library/WebKit/<应用目录>/WebsiteDataStore/<数据存储标识符的 hex>/`，
//! `data_directory`（Windows 那套 user-data 目录）**被 WKWebView 忽略**。
//!
//! 所以本探针：
//!
//! · **源路径只从环境变量读**（`DSH_MACOS_WK_SOURCE`）——源码里**一个路径
//!   字面量都没有**（判据 `probe-profile-isolation` 第 30 条）。
//! · 把源**只读拷贝**到**探针自己的进程名**下面（探针写的是拷贝，不是用户的
//!   档案）⇒ 用户正在跑的壳那份存储**一个字节都不改**。
//! · 拷贝完**逐字节校验**（`Cookies.binarycookies` 两边比字节）：源正在被
//!   写时拷贝可能撕裂 ⇒ 校验不过就重拷，重试次数与最终一致性都进读数。
//!
//! # 读数纪律
//!
//! **不报**：会话标题原文、会话 id、任何凭据的值。
//! **报**：状态码、业务错误码、结构键名、条数、凭据的**长度**。
//!
//! 输出是 `PROBE_REPORT_BEGIN` / `PROBE_REPORT_END` 之间的一段 JSON。

// 探针只用到 harness 的一部分，`#[path]` 把整份源文件编进来会带出一批
// 未使用的常量与函数——这不影响探针行为，不必逐个处理。
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/harness/official_chat.rs"]
mod official_chat;

// **生产那份回程机制本身**——本探针量的就是它，不是副本。
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/official_chat/script_eval.rs"]
mod script_eval;

// 生产代码里 `crate::shell::window::window_title` /
// `crate::shell::appearance::initial_theme` 是**壳自己的**模块；example 是
// 另一个 crate，没有它们。这里给一份**同名同形状**的最小实现，让 `#[path]`
// 进来的那份生产源码在探针里也能编译——测的仍然是生产那份代码，不是副本。
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

// 本探针的整条路线依赖 WKWebsiteDataStore（macOS 专有）⇒ 别的平台上整块
// 编译掉，给一个明确占位（否则 CI 的 `cargo clippy --all-targets -D warnings`
// 把 examples 也算进去，Windows / Linux 上编不过就是一条门禁红）。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!(
        "probe: 本探针只支持 macOS（WKWebsiteDataStore 路线）。\n\
         probe: 别的平台请用 `official_chat_probe.rs --mode site --site deepseek`。"
    );
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    macos_probe::run();
}

#[cfg(target_os = "macos")]
mod macos_probe {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};
    use url::Url;

    use crate::official_chat;
    use crate::script_eval::{read_json, round_trip};

    /// 探针自己的 WKWebView 数据存储标识符（**不是**生产的任何一个）。
    ///
    /// 16 字节，必须是合法标识符。生产用的是 `dsh-chat-rel-001` /
    /// `dsh-chat-dev-001`。
    const PROBE_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-mfs-001";

    /// **源存储目录**从哪个环境变量读。
    ///
    /// 源码里不写路径字面量：路径是**运行时的**事实（哪一份存储、拷到哪），
    /// 写进源码就等于把「用户真实数据目录」焊进探针（判据第 30 条）。
    const ENV_SOURCE: &str = "DSH_MACOS_WK_SOURCE";

    /// 被测站点。DeepSeek 官网的会话目录端点见
    /// `src-tauri/src/official_chat/session_body.rs` 的 `CATALOG_PATH`。
    const SITE_URL: &str = "https://chat.deepseek.com/";
    const CATALOG_PATH: &str = "/api/v0/chat_session/fetch_page";

    /// **页内 token 定位脚本**：那个 `authorization: Bearer <64 字符>` 的 token
    /// **从哪来**。
    ///
    /// 为什么必须问：页内**观测**到站点自己发了这个头，不等于**壳能自己构造**
    /// 它。生产实现要复刻这一发，就得知道 token 存在哪儿、能不能从页内读到。
    ///
    /// ⚠️ **值全程不出页面**：脚本只在页内做**比对**，命中时只报**位置**
    /// （存储种类 ＋ 键名/键路径），**不报 token 本身**。
    const PROBE_TOKEN_LOCATOR_SCRIPT: &str = r#"
(function () {
  function locateTokenSource() {
    var REC = window.__dsh_probe_net;
    var out = { candidates: [], hits: [] };
    if (!REC) return { error: "没有观测记录（拦截器没装上？）" };
    var raw = REC.authValue;
    if (!raw) return { error: "没抓到 authorization 头的值" };
    var token = String(raw).replace(/^Bearer\s+/i, "").trim();
    out.tokenLength = token.length;
    out.tokenPrefixIsBearer = /^Bearer\s+/i.test(String(raw));
    if (!token) return { error: "authorization 头里没有 token" };

    // 命中判定：**精确相等** ＋ **包含**（token 被包在更大的 JSON 里）。
    function check(kind, name, value, depth) {
      if (value === null || value === undefined) return;
      var s = String(value);
      if (s.length < 16) return;
      if (s === token) {
        out.hits.push({ kind: kind, name: name, how: "exact", length: s.length, depth: depth || 0 });
        return;
      }
      if (s.indexOf(token) >= 0) {
        out.hits.push({ kind: kind, name: name, how: "contains", length: s.length, depth: depth || 0 });
        return;
      }
      // 值本身是 JSON ⇒ 往下再找一层（token 可能嵌在 userInfo 这种对象里）。
      if ((depth || 0) < 4 && s.length < 200000 && (s.charAt(0) === "{" || s.charAt(0) === "[")) {
        var parsed = null;
        try { parsed = JSON.parse(s); } catch (e) { return; }
        walk(parsed, kind + "." + name, depth || 0);
      }
    }

    function walk(node, path, depth) {
      if (!node || depth > 4) return;
      if (typeof node === "string") { check("json", path, node, depth); return; }
      if (Array.isArray(node)) {
        for (var i = 0; i < node.length && i < 200; i++) walk(node[i], path + "[" + i + "]", depth + 1);
        return;
      }
      if (typeof node === "object") {
        var keys = Object.keys(node);
        for (var j = 0; j < keys.length && j < 200; j++) {
          walk(node[keys[j]], path + "." + keys[j], depth + 1);
        }
      }
    }

    try {
      for (var i = 0; i < localStorage.length; i++) {
        var k = localStorage.key(i);
        check("localStorage", k, localStorage.getItem(k), 0);
      }
    } catch (e) {}
    try {
      for (var i = 0; i < sessionStorage.length; i++) {
        var k = sessionStorage.key(i);
        check("sessionStorage", k, sessionStorage.getItem(k), 0);
      }
    } catch (e) {}
    try {
      document.cookie.split(";").forEach(function (part) {
        var p = part.split("=");
        check("cookie", p[0].trim(), p.slice(1).join("="), 0);
      });
    } catch (e) {}
    // window 上的一层字符串 + 一层对象的字符串属性。
    try {
      Object.keys(window).forEach(function (k) {
        try {
          var v = window[k];
          if (typeof v === "string") { check("window", k, v, 0); }
          else if (v && typeof v === "object" && !Array.isArray(v)) {
            var ks = Object.keys(v);
            for (var j = 0; j < ks.length && j < 200; j++) {
              try {
                var v2 = v[ks[j]];
                if (typeof v2 === "string") check("window." + k, ks[j], v2, 1);
                else if (v2 && typeof v2 === "object") walk(v2, "window." + k + "." + ks[j], 2);
              } catch (e) {}
            }
          }
        } catch (e) {}
      });
    } catch (e) {}

    // 命中去重（同一个位置可能报多次）。
    var seen = {};
    out.hits = out.hits.filter(function (h) {
      var key = h.kind + "|" + h.name + "|" + h.how;
      if (seen[key]) return false;
      seen[key] = true;
      return true;
    });

    // ① `userToken` 的**形状**：`{"value": "<token>", ...}` 这种包装？
    //    生产实现要按形状取值 ⇒ 这里只报键名与长度，**不报值**。
    try {
      var raw = localStorage.getItem("userToken");
      if (raw === null) {
        out.userTokenShape = { present: false };
      } else {
        var shape = { present: true, rawLength: raw.length };
        try {
          var parsed = JSON.parse(raw);
          shape.json = true;
          shape.topKeys = Object.keys(parsed);
          if (typeof parsed.value === "string") {
            shape.valueLength = parsed.value.length;
            shape.valueEqualsToken = parsed.value === token;
          }
        } catch (e) {
          shape.json = false;
        }
        out.userTokenShape = shape;
      }
    } catch (e) {}

    // ② `x-device-id` 页内能不能读到？（它是站点发的那一发的头之一）
    //    生产实现若要复刻整套头，就得知道这个值从哪来。
    try {
      var deviceId = REC.deviceIdValue;
      if (!deviceId) {
        out.deviceId = { present: false };
      } else {
        var dInfo = { present: true, length: deviceId.length, hits: [] };
        var dSeen = {};
        function dCheck(kind, name, value) {
          if (value === null || value === undefined) return;
          var s = String(value);
          if (s.length < 16) return;
          var how = null;
          if (s === deviceId) how = "exact";
          else if (s.indexOf(deviceId) >= 0) how = "contains";
          if (!how) return;
          var key = kind + "|" + name + "|" + how;
          if (dSeen[key]) return;
          dSeen[key] = true;
          dInfo.hits.push({ kind: kind, name: name, how: how, length: s.length });
        }
        try {
          for (var i = 0; i < localStorage.length; i++) {
            var k = localStorage.key(i);
            dCheck("localStorage", k, localStorage.getItem(k));
          }
        } catch (e) {}
        try {
          for (var i = 0; i < sessionStorage.length; i++) {
            var k = sessionStorage.key(i);
            dCheck("sessionStorage", k, sessionStorage.getItem(k));
          }
        } catch (e) {}
        try {
          document.cookie.split(";").forEach(function (part) {
            var p = part.split("=");
            dCheck("cookie", p[0].trim(), p.slice(1).join("="));
          });
        } catch (e) {}
        try {
          Object.keys(window).forEach(function (k) {
            try {
              var v = window[k];
              if (typeof v === "string") { dCheck("window", k, v); }
              else if (v && typeof v === "object" && !Array.isArray(v)) {
                var ks = Object.keys(v);
                for (var j = 0; j < ks.length && j < 200; j++) {
                  try {
                    var v2 = v[ks[j]];
                    if (typeof v2 === "string") dCheck("window." + k, ks[j], v2);
                  } catch (e) {}
                }
              }
            } catch (e) {}
          });
        } catch (e) {}
        out.deviceId = dInfo;
      }
    } catch (e) {}

    return out;
  }

  try {
    Object.defineProperty(window, "__dsh_locate_token", {
      value: locateTokenSource, writable: true, configurable: true, enumerable: false
    });
  } catch (e) { try { window.__dsh_locate_token = locateTokenSource; } catch (e2) {} }
})();
"#;

    /// **页内观测脚本**：拦下站点自己发的那一发，把它的**请求头契约**记下来。
    ///
    /// 为什么需要它：裸 `fetch` 被服务端以 `40003 INVALID_TOKEN` 拒绝（实测，
    /// 连不存在的路径也是同一个码 ⇒ 鉴权中间件在路由之前）。而**页面里能看到
    /// 真实会话列表** ⇒ 站点自己那一发带对了认证。要回答「认证契约长什么样」，
    /// 就得**在页内**看它自己发的请求。
    ///
    /// **凭据全程不出页面**：拦截器只向宿主报「头名 ＋ 值的长度 ＋ 值的前 7 个
    /// 字符（够看出 `Bearer ` 这类前缀，不够还原凭据）」；**值本身**留在页面里，
    /// 重放也用**页面内**的那份值。
    ///
    /// ⚠️ **猴补 `fetch`/`XHR` 是页内痕迹**，生产实现不许照抄（站点能检测到
    /// `window.fetch` 被改写）。本探针用它的目的只有一个：**把契约问清楚**，
    /// 契约定下来之后生产实现按契约**直接构造**请求，不猴补。
    const PROBE_INTERCEPT_SCRIPT: &str = r#"
(function () {
  var REC = { fetches: [], xhrs: [], auth: null, replay: null, replayState: "pending" };
  function put(slot, value) {
    try {
      Object.defineProperty(window, slot, {
        value: value, writable: true, configurable: true, enumerable: false
      });
    } catch (e) { try { window[slot] = value; } catch (e2) {} }
  }
  put("__dsh_probe_net", REC);

  // 头契约的**形状**：只报头名、值的长度、值的前 7 个字符。
  function describe(headers) {
    var out = [];
    try {
      if (!headers) return out;
      if (typeof headers.forEach === "function") {
        headers.forEach(function (v, k) {
          out.push({ name: String(k), length: String(v).length, prefix: String(v).slice(0, 7) });
        });
      } else {
        Object.keys(headers).forEach(function (k) {
          var v = headers[k];
          out.push({ name: String(k), length: String(v).length, prefix: String(v).slice(0, 7) });
        });
      }
    } catch (e) {}
    return out;
  }

  function looksLikeCatalog(url) {
    return String(url || "").indexOf("/api/v0/chat_session/fetch_page") >= 0;
  }

  // 响应体 → **只报结构**（状态码、业务 code、会话条数、标题长度）。
  // 标题原文与会话 id 不进读数。
  function summarizeResponse(status, text) {
    var summary = { status: status, bodyLength: String(text || "").length };
    try {
      var parsed = JSON.parse(text);
      summary.json = true;
      summary.topKeys = Object.keys(parsed);
      if (typeof parsed.code !== "undefined") summary.code = parsed.code;
      if (typeof parsed.msg === "string") summary.msg = parsed.msg.slice(0, 120);
      var data = parsed.data;
      if (data && typeof data === "object") {
        summary.dataType = "object";
        summary.dataKeys = Object.keys(data);
        var biz = data.biz_data;
        if (biz && typeof biz === "object") {
          summary.bizKeys = Object.keys(biz);
          var sessions = biz.chat_sessions;
          if (Array.isArray(sessions)) {
            summary.sessionCount = sessions.length;
            if (sessions.length > 0) {
              summary.sessionKeys = Object.keys(sessions[0]);
              summary.titleLengths = sessions.slice(0, 10).map(function (s) {
                return String((s && s.title) || "").length;
              });
              summary.hasSessionId = sessions.every(function (s) { return !!(s && s.id); });
            }
          }
        }
      } else {
        summary.dataType = data === null ? "null" : typeof data;
      }
    } catch (e) {
      summary.json = false;
      summary.head = String(text || "").slice(0, 120);
    }
    return summary;
  }

  // **重放**：用页面里**观测到的那个头对象**（带值）再发一次目录请求。
  // 值不出页面 ⇒ 宿主只拿到结果的结构。
  function replay(headersObj, method, body) {
    try {
      var init = { credentials: "include", headers: headersObj };
      if (method) init.method = method;
      if (body !== undefined && body !== null) init.body = body;
      REC.replayState = "running";
      fetch("/api/v0/chat_session/fetch_page", init).then(function (res) {
        return res.text().then(function (text) {
          var summary = { status: res.status, bodyLength: text.length };
          try {
            var parsed = JSON.parse(text);
            summary.json = true;
            summary.topKeys = Object.keys(parsed);
            if (typeof parsed.code !== "undefined") summary.code = parsed.code;
            if (typeof parsed.msg === "string") summary.msg = parsed.msg.slice(0, 120);
            var data = parsed.data;
            if (data && typeof data === "object") {
              summary.dataType = "object";
              summary.dataKeys = Object.keys(data);
              var biz = data.biz_data;
              if (biz && typeof biz === "object") {
                summary.bizKeys = Object.keys(biz);
                var sessions = biz.chat_sessions;
                if (Array.isArray(sessions)) {
                  summary.sessionCount = sessions.length;
                  if (sessions.length > 0) {
                    summary.sessionKeys = Object.keys(sessions[0]);
                    summary.titleLengths = sessions.slice(0, 10).map(function (s) {
                      return String((s && s.title) || "").length;
                    });
                    summary.hasSessionId = sessions.every(function (s) { return !!(s && s.id); });
                  }
                }
              }
            } else {
              summary.dataType = data === null ? "null" : typeof data;
            }
          } catch (e) {
            summary.json = false;
            summary.head = text.slice(0, 120);
          }
          REC.replay = summary;
          REC.replayState = "done";
        });
      }).catch(function (e) {
        REC.replay = { error: String(e) };
        REC.replayState = "done";
      });
    } catch (e) {
      REC.replay = { error: String(e) };
      REC.replayState = "done";
    }
  }

  var origFetch = window.fetch;
  if (origFetch) {
    window.fetch = function (input, init) {
      try {
        var url = (typeof input === "string") ? input : ((input && input.url) || "");
        if (looksLikeCatalog(url)) {
          var h = (init && init.headers) || (input && input.headers) || null;
          var entry = {
            url: String(url).slice(0, 200),
            method: String((init && init.method) || (input && input.method) || "GET"),
            credentialMode: String((init && init.credentials) || "default"),
            headerContract: describe(h),
            hasBody: !!(init && init.body),
            bodyLength: (init && init.body) ? String(init.body).length : 0
          };
          REC.fetches.push(entry);
          // **第一次**抓到目录请求时：把头对象（带值）留在页面里，并重放一次。
          if (!REC.auth) {
            REC.auth = { captured: true, headerContract: entry.headerContract, method: entry.method };
            try {
              // 用同一份 headers（值在页面内）：能重放就说明契约对了。
              var headersObj = (init && init.headers) || (input && input.headers) || null;
              if (headersObj) {
                var body = (init && init.body !== undefined) ? init.body : null;
                replay(headersObj, entry.method, body);
              } else {
                REC.replayState = "done";
                REC.replay = { error: "站点那一发没有可复用的 headers 对象" };
              }
            } catch (e) {
              REC.replayState = "done";
              REC.replay = { error: String(e) };
            }
          }
        }
      } catch (e) {}
      return origFetch.apply(this, arguments);
    };
  }

  try {
    var origOpen = XMLHttpRequest.prototype.open;
    var origSetHeader = XMLHttpRequest.prototype.setRequestHeader;
    var origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function (method, url) {
      try {
        this.__dsh_url = String(url);
        this.__dsh_method = String(method);
        this.__dsh_headers = [];
        this.__dsh_headerPairs = [];
      } catch (e) {}
      return origOpen.apply(this, arguments);
    };
    XMLHttpRequest.prototype.setRequestHeader = function (name, value) {
      try {
        this.__dsh_headers.push({ name: String(name), length: String(value).length, prefix: String(value).slice(0, 7) });
        // **值本身留在页面里**（不进读数），重放时要按原样设回去。
        this.__dsh_headerPairs.push([String(name), String(value)]);
        // 认证头的值也留在页面里，供 `__dsh_locate_token` 在页内比对来源。
        if (String(name).toLowerCase() === "authorization") {
          REC.authValue = String(value);
        }
        if (String(name).toLowerCase() === "x-device-id") {
          REC.deviceIdValue = String(value);
        }
      } catch (e) {}
      return origSetHeader.apply(this, arguments);
    };
    XMLHttpRequest.prototype.send = function () {
      try {
        var u = this.__dsh_url || "";
        if (looksLikeCatalog(u)) {
          var self = this;
          var pairs = this.__dsh_headerPairs || [];
          REC.xhrs.push({ url: u.slice(0, 200), method: this.__dsh_method || "", headerContract: this.__dsh_headers || [] });
          // ① 站点自己那一发的**响应**（它成功 = 页内确实拿得到目录数据）。
          try {
            this.addEventListener("load", function () {
              try {
                REC.siteResponse = summarizeResponse(self.status, self.responseText);
              } catch (e) {
                REC.siteResponse = { error: String(e) };
              }
            });
          } catch (e) {}
          // ② **重放**：用**同一份头**（值在页面内）再发一次 ⇒
          //    「壳能自己发起这个请求」这一步的独立证据。
          if (!REC.auth) {
            REC.auth = {
              captured: true,
              headerContract: this.__dsh_headers || [],
              method: this.__dsh_method || "GET",
              url: u.slice(0, 200)
            };
            setTimeout(function () {
              try {
                var xhr = new XMLHttpRequest();
                xhr.open(REC.auth.method, REC.auth.url, true);
                for (var i = 0; i < pairs.length; i++) {
                  try { xhr.setRequestHeader(pairs[i][0], pairs[i][1]); } catch (e) {}
                }
                xhr.onload = function () {
                  try {
                    REC.replay = summarizeResponse(xhr.status, xhr.responseText);
                  } catch (e) {
                    REC.replay = { error: String(e) };
                  }
                  REC.replayState = "done";
                };
                xhr.onerror = function () {
                  REC.replay = { error: "重放的 XHR 触发了 error 事件" };
                  REC.replayState = "done";
                };
                REC.replayState = "running";
                xhr.send();
              } catch (e) {
                REC.replay = { error: String(e) };
                REC.replayState = "done";
              }
            }, 300);
            // ③ **最小头集**再重放一次：**只带 `authorization`**（不带 `x-*`）。
            //    这一发直接回答「生产实现最少要构造哪些头」。
            setTimeout(function () {
              try {
                var authPair = null;
                for (var i = 0; i < pairs.length; i++) {
                  if (String(pairs[i][0]).toLowerCase() === "authorization") {
                    authPair = pairs[i];
                    break;
                  }
                }
                if (!authPair) {
                  REC.replayMinimal = { error: "没有 authorization 头" };
                  REC.replayMinimalState = "done";
                  return;
                }
                var xhr2 = new XMLHttpRequest();
                xhr2.open(REC.auth.method, REC.auth.url, true);
                try { xhr2.setRequestHeader(authPair[0], authPair[1]); } catch (e) {}
                xhr2.onload = function () {
                  try {
                    REC.replayMinimal = summarizeResponse(xhr2.status, xhr2.responseText);
                  } catch (e) {
                    REC.replayMinimal = { error: String(e) };
                  }
                  REC.replayMinimalState = "done";
                };
                xhr2.onerror = function () {
                  REC.replayMinimal = { error: "重放的 XHR 触发了 error 事件" };
                  REC.replayMinimalState = "done";
                };
                REC.replayMinimalState = "running";
                xhr2.send();
              } catch (e) {
                REC.replayMinimal = { error: String(e) };
                REC.replayMinimalState = "done";
              }
            }, 900);
          }
        }
      } catch (e) {}
      return origSend.apply(this, arguments);
    };
  } catch (e) {}
})();
"#;

    /// 拷贝重试次数（源正在被写时可能拷到撕裂的快照）。
    const COPY_ATTEMPTS: usize = 3;

    pub fn run() {
        let url = match Url::parse(SITE_URL) {
            Ok(url) => url,
            Err(e) => {
                eprintln!("probe: 站点地址非法：{e}");
                std::process::exit(2);
            }
        };

        // --- 存储准备（只读拷贝）------------------------------------------------
        // 必须在建窗**之前**：WebKit 启动时就把这份存储读进来。
        let (store_dir, store_reading) = match prepare_store(&PROBE_DATA_STORE_IDENTIFIER) {
            Ok(pair) => pair,
            Err(e) => {
                let report = json!({
                    "fatal": format!("存储准备失败：{e}"),
                    "hint": format!(
                        "先设 {ENV_SOURCE}=<一份 WKWebsiteDataStore 目录>（只读拷贝的源）"
                    ),
                })
                .to_string();
                println!("PROBE_REPORT_BEGIN");
                println!("{report}");
                println!("PROBE_REPORT_END");
                std::process::exit(2);
            }
        };

        let app = tauri::Builder::default()
            .setup(move |app| {
                let handle = app.handle().clone();
                let url = url.clone();
                let store_dir = store_dir.clone();
                let store_reading = store_reading.clone();
                std::thread::Builder::new()
                    .name("official-chat-macos-fetch-probe".into())
                    .spawn(move || {
                        let report = tauri::async_runtime::block_on(async {
                            run_probe_body(&handle, &url, store_dir, store_reading).await
                        });
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

    /// 数据存储标识符 → WKWebView 持久化目录用的那串 hex。
    ///
    /// 形状是标准 UUID 写法（8-4-4-4-12），**字节顺序与字符串一致**：
    /// `dsh-chat-prb-001` ⇒ `6473682d-6368-6174-2d70-72622d303031`
    /// （与探针 `official_chat_login_probe` 实测落盘的目录名逐字一致）。
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

    /// 探针自己的 WK 存储目录：`~/Library/WebKit/<进程名>/WebsiteDataStore/<hex>/`。
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

    /// 递归拷贝一个目录树。
    fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dst)
            .map_err(|e| format!("建目标目录失败（{}）：{e}", dst.display()))?;
        let entries = std::fs::read_dir(src)
            .map_err(|e| format!("读源目录失败（{}）：{e}", src.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("读源目录项失败：{e}"))?;
            let file_type = entry
                .file_type()
                .map_err(|e| format!("读源目录项类型失败（{}）：{e}", entry.path().display()))?;
            let target = dst.join(entry.file_name());
            if file_type.is_dir() {
                copy_tree(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), &target).map_err(|e| {
                    format!(
                        "拷文件失败（{} → {}）：{e}",
                        entry.path().display(),
                        target.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    /// 两边逐字节比一个文件。
    fn same_bytes(left: &Path, right: &Path) -> Result<Option<(u64, u64)>, String> {
        let a = std::fs::read(left).map_err(|e| format!("读 {} 失败：{e}", left.display()))?;
        let b = std::fs::read(right).map_err(|e| format!("读 {} 失败：{e}", right.display()))?;
        Ok(if a == b {
            Some((a.len() as u64, b.len() as u64))
        } else {
            None
        })
    }

    /// 把源存储**只读拷贝**到探针自己的目录，并校验一致性。
    ///
    /// 返回（目标目录、读数）。
    fn prepare_store(identifier: &[u8; 16]) -> Result<(PathBuf, Value), String> {
        let source = std::env::var(ENV_SOURCE)
            .map_err(|_| format!("没给环境变量 {ENV_SOURCE}（源存储目录）"))?;
        let source = PathBuf::from(source);
        if !source.is_dir() {
            return Err(format!("源存储目录不存在：{}", source.display()));
        }
        let target = probe_store_dir(identifier)?;

        let mut attempts: Vec<Value> = Vec::new();
        for round in 1..=COPY_ATTEMPTS {
            // 目标先清干净：上一轮残留的内容会与这一轮的混在一起。
            if target.exists() {
                let _ = std::fs::remove_dir_all(&target);
            }
            copy_tree(&source, &target)?;

            // 校验点：**cookie 文件**逐字节一致（它是活文件，最容易撕裂）。
            let rel = Path::new("Cookies").join("Cookies.binarycookies");
            let src_cookie = source.join(&rel);
            let dst_cookie = target.join(&rel);
            let result = if src_cookie.exists() && dst_cookie.exists() {
                match same_bytes(&src_cookie, &dst_cookie)? {
                    Some((a, b)) => json!({ "identical": true, "src_bytes": a, "dst_bytes": b }),
                    None => json!({
                        "identical": false,
                        "src_bytes": std::fs::metadata(&src_cookie).map(|m| m.len()).unwrap_or(0),
                        "dst_bytes": std::fs::metadata(&dst_cookie).map(|m| m.len()).unwrap_or(0),
                    }),
                }
            } else {
                json!({
                    "identical": false,
                    "note": "源或目标里没有 cookie 文件（没登录过，或存储不在这个位置）",
                    "src_exists": src_cookie.exists(),
                    "dst_exists": dst_cookie.exists(),
                })
            };
            let ok = result
                .get("identical")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            attempts.push(json!({ "round": round, "result": result }));
            if ok {
                return Ok((
                    target.clone(),
                    json!({
                        "ok": true,
                        "rounds": attempts,
                        "source_dir": source.display().to_string(),
                        "target_dir": target.display().to_string(),
                        "identifier_hex": identifier_hex(identifier),
                        "note": "源是**只读拷贝**：探针写的全在拷贝里，用户正在跑的那份存储一个字节都没改",
                    }),
                ));
            }
        }
        Err(format!(
            "拷贝 {COPY_ATTEMPTS} 轮都没拿到一致的 cookie 快照（源正在被写？）：{attempts:?}"
        ))
    }

    /// 建一个探针窗口（**无条件**给 `data_directory`）。
    fn build_window(
        app: &tauri::AppHandle,
        url: &Url,
        identifier: &[u8; 16],
    ) -> Result<tauri::WebviewWindow, String> {
        // **无条件**给 `data_directory`：判据 `probe-profile-isolation` 第 ③ 条
        // 按**代码形状**判，「有才给」同样违规。macOS 上 WKWebView **忽略**这个
        // 参数（它认的是 `data_store_identifier`），但照样给——口径一致。
        let profile = std::env::temp_dir().join("dsh-official-chat-macos-fetch-probe");
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("无法创建临时 profile 目录（{}）：{e}", profile.display()))?;

        let builder = tauri::WebviewWindowBuilder::new(
            app,
            "official-chat-macos-fetch-probe",
            tauri::WebviewUrl::External(url.clone()),
        )
        .title("dsh macOS 路线 B 探针")
        .inner_size(1100.0, 800.0)
        .visible(true)
        .data_directory(profile)
        .additional_browser_args(official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        // **观测脚本先注入**：它要在站点自己的脚本跑之前就把 `fetch`/`XHR` 包好，
        // 否则站点首屏那一发抓不到。
        .initialization_script(PROBE_INTERCEPT_SCRIPT)
        // token 定位脚本与观测脚本通过 `window.__dsh_probe_net` 通信。
        .initialization_script(PROBE_TOKEN_LOCATOR_SCRIPT)
        .initialization_script(include_str!("../src/harness/chat-fingerprint.js"))
        .data_store_identifier(*identifier);

        builder
            .build()
            .map_err(|e| format!("无法创建探针窗口：{e}"))
    }

    /// 等站点页面真正就绪：origin 落到该站点 ＋ 文档 complete ＋ body 非空。
    async fn wait_for_site(window: &tauri::WebviewWindow, budget: Duration) -> Result<u64, String> {
        let started = Instant::now();
        let mut last = String::new();
        while started.elapsed() < budget {
            let probe = "JSON.stringify({ origin: location.origin, href: String(location.href), ready: document.readyState, hasBody: !!(document.body && document.body.children.length) })";
            if let Ok(Value::String(raw)) = read_json(window, probe, Duration::from_secs(2)).await {
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
        Err(format!("页面在预算内没就绪（最后一次读数：{last}）"))
    }

    /// 页面事实：**不发任何网络请求**。只报键名与长度，不报凭据的值。
    async fn collect_page_facts(window: &tauri::WebviewWindow) -> Value {
        let expression = r#"JSON.stringify({
            origin: String(location.origin),
            href: String(location.href),
            title: String(document.title).slice(0, 120),
            readyState: String(document.readyState),
            localStorageKeys: [],
            sessionStorageKeys: [],
            visibleCookieNames: [],
            pageLooksLikeAuth: /sign[_-]?in|sign[_-]?up|log[_-]?in|register|\/auth/i.test(String(location.href)),
            bodyHead: (document.body ? (document.body.innerText || "") : "").slice(0, 300)
        })"#;
        // 上面这个表达式是**同步**的：原生求值拿得到值（不需要等 Promise）。
        // 这里用一次 `round_trip` 也行，但同步读回更能说明「原生求值本身可用」。
        match read_json(window, expression, Duration::from_secs(6)).await {
            Ok(Value::String(raw)) => match serde_json::from_str::<Value>(&raw) {
                Ok(mut value) => {
                    // 凭据的**长度**（不是值）：用来判断「键在但值已空 / 值还在」。
                    // 长度不泄露凭据内容，是可以安全回传的最小信息。
                    let lengths = r#"JSON.stringify((function () {
                        var keys = ["userToken", "settingsJwt", "__appKit_userInfo", "_token",
                                    "accessToken", "sessionToken", "token", "auth_token"];
                        var out = {};
                        for (var i = 0; i < keys.length; i++) {
                            try {
                                var v = localStorage.getItem(keys[i]);
                                if (v !== null) out[keys[i]] = String(v).length;
                            } catch (e) {}
                        }
                        return out;
                    })())"#;
                    if let Ok(Value::String(text)) =
                        read_json(window, lengths, Duration::from_secs(4)).await
                    {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                            value["credentialLengths"] = parsed;
                        }
                    }
                    value
                }
                Err(e) => json!({ "error": format!("现场不是合法 JSON：{e}") }),
            },
            Ok(other) => other,
            Err(e) => json!({ "error": format!("读回失败：{e}") }),
        }
    }

    /// 读数①：**原生求值**（不等 Promise）。
    ///
    /// 两发对照：
    /// · 同步表达式 ⇒ 应拿得到值（macOS 上 wry 给的是结果的 JSON 序列化）；
    /// · Promise 表达式 ⇒ 拿到的应是 **Promise 对象本身**（序列化 = `{}`），
    ///   这就是「必须走 round_trip」的根据。
    async fn measure_native_eval(window: &tauri::WebviewWindow) -> Value {
        let sync = read_json(window, "String(1 + 1)", Duration::from_secs(4)).await;
        let sync_reading = match sync {
            Ok(value) => json!({ "ok": true, "value": value }),
            Err(e) => json!({ "ok": false, "error": format!("{e}") }),
        };

        // 对照臂：直接把一个 Promise 交给原生求值。
        let raw = eval_raw(window, "Promise.resolve('native-does-not-await')").await;
        json!({
            "sync_expression": sync_reading,
            "promise_via_native_eval": match raw {
                Ok(text) => json!({
                    "ok": true,
                    "raw": text,
                    "is_empty_object": text.trim() == "{}",
                    "note": "原生求值拿到的是 Promise **对象本身**（JSON 序列化 = {}），不是 resolve 出来的值 ⇒ 必须走 round_trip",
                }),
                Err(e) => json!({ "ok": false, "error": e }),
            },
        })
    }

    /// 直接调原生求值，把回调拿到的**原始文本**取回来。
    async fn eval_raw(window: &tauri::WebviewWindow, script: &str) -> Result<String, String> {
        let (tx, mut rx) = tauri::async_runtime::channel(1);
        let script = script.to_string();
        window
            .eval_with_callback(script, move |raw| {
                let _ = tx.try_send(raw);
            })
            .map_err(|e| format!("页内求值失败：{e}"))?;
        match tokio::time::timeout(Duration::from_secs(6), rx.recv()).await {
            Ok(Some(text)) => Ok(text),
            Ok(None) => Err("结果通道已关闭（webview 被销毁）".to_string()),
            Err(_) => Err("原生求值在 6s 内没有回应".to_string()),
        }
    }

    /// 读数②：**异步结果回传**（生产的 `round_trip`）。
    async fn measure_round_trip(window: &tauri::WebviewWindow) -> Value {
        let started = Instant::now();
        let outcome = round_trip(
            window,
            "(function () { return new Promise(function (resolve) { setTimeout(function () { resolve('async-ok'); }, 300); }); })()",
            Duration::from_secs(10),
        )
        .await;
        let elapsed = started.elapsed().as_millis() as u64;
        match outcome {
            Ok(trip) => json!({
                "ok": true,
                "value": trip.value,
                "elapsed_ms": elapsed,
                "polls": trip.polls,
                "note": "拿到的是 Promise **settle 之后**的值（300ms 的定时器）⇒ 页内异步 ＋ 宿主轮询这条路在 macOS 上成立",
            }),
            Err(e) => json!({ "ok": false, "elapsed_ms": elapsed, "error": format!("{e}") }),
        }
    }

    /// 读数②'：**站点的认证契约**（页内观测 ＋ 页内重放）。    ///
    /// 这一发回答的是「裸 fetch 为什么被拒」：**站点自己那一发带了什么头**。
    /// 拦截器在页面里记下头契约（头名 ＋ 值的长度 ＋ 值的前 7 个字符），
    /// **值本身留在页面里**，并用**同一份头**在页内重放一次目录请求。
    ///
    /// ⇒ 若重放拿到 2xx ＋ `chat_sessions` 条数 > 0 ⇒ **路线 B 完整成立**
    /// （页内 fetch ＋ 按契约构造的认证头），macOS 与 Windows 可以并列成同一种形态。
    async fn measure_auth_contract(window: &tauri::WebviewWindow) -> Value {
        // 等页内拦截器抓到站点自己那一发，并等重放跑完。
        let started = Instant::now();
        let budget = Duration::from_secs(30);
        let mut last: Option<Value> = None;
        while started.elapsed() < budget {
            let probe = "JSON.stringify((function () { var n = window.__dsh_probe_net; return n ? { fetches: n.fetches, xhrs: n.xhrs, auth: n.auth, replay: n.replay, replayState: n.replayState, replayMinimal: n.replayMinimal, replayMinimalState: n.replayMinimalState, siteResponse: n.siteResponse } : null; })())";
            if let Ok(Value::String(raw)) = read_json(window, probe, Duration::from_secs(3)).await {
                if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                    let state = parsed
                        .get("replayState")
                        .and_then(Value::as_str)
                        .unwrap_or("pending");
                    let minimal_state = parsed
                        .get("replayMinimalState")
                        .and_then(Value::as_str)
                        .unwrap_or("pending");
                    let captured = parsed
                        .pointer("/auth/captured")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if captured && state == "done" && minimal_state == "done" {
                        return json!({
                            "ok": true,
                            "elapsed_ms": started.elapsed().as_millis() as u64,
                            "observed": parsed,
                        });
                    }
                    last = Some(parsed);
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        json!({
            "ok": false,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "note": "没等到「抓到站点那一发 ＋ 重放跑完」",
            "last": last,
        })
    }

    /// 读数②''：**token 从哪来**（页内比对，只报位置不报值）。
    ///
    /// 这一发决定「路线 B 能不能落生产」：**观测**到站点发了
    /// `authorization: Bearer <64 字符>` 不等于**壳能自己构造**它。
    /// 若命中在 `localStorage` / `cookie` / 某个全局量上 ⇒ 页内可读 ⇒ 壳能自己
    /// 构造这一发；若**一处都没命中** ⇒ token 在页内**读不到**的地方
    /// （例如 httpOnly cookie）⇒ 路线 B 在 macOS 上**技术上成立但落不了地**。
    async fn measure_token_source(window: &tauri::WebviewWindow) -> Value {
        let expression = "(function () { try { return JSON.stringify(window.__dsh_locate_token ? window.__dsh_locate_token() : { error: '页内没有定位器' }); } catch (e) { return JSON.stringify({ error: String(e) }); } })()";
        match read_json(window, expression, Duration::from_secs(15)).await {
            Ok(Value::String(raw)) => match serde_json::from_str::<Value>(&raw) {
                Ok(value) => value,
                Err(e) => json!({ "error": format!("定位结果不是合法 JSON：{e}") }),
            },
            Ok(other) => other,
            Err(e) => json!({ "error": format!("读回失败：{e}") }),
        }
    }

    /// 读数③：**同源 fetch**（真实站点 ＋ 真实登录态）。
    ///
    /// 只报结构与计数：状态码、业务错误码、结构键名、会话条数、**标题长度**
    /// （不报标题原文，不报会话 id）。
    async fn measure_catalog_fetch(window: &tauri::WebviewWindow) -> Value {
        let expression = format!(
            r#"(async function () {{
  var out = {{ origin: String(location.origin), attempts: [] }};

  function summarize(text) {{
    var summary = {{ bodyLength: text.length }};
    try {{
      var parsed = JSON.parse(text);
      summary.json = true;
      summary.topKeys = Object.keys(parsed);
      if (typeof parsed.code !== "undefined") summary.code = parsed.code;
      if (typeof parsed.msg === "string") summary.msg = parsed.msg.slice(0, 200);
      var data = parsed.data;
      if (data === null) {{
        summary.dataType = "null";
      }} else if (data && typeof data === "object") {{
        summary.dataType = "object";
        summary.dataKeys = Object.keys(data);
        var biz = data.biz_data;
        if (biz && typeof biz === "object") {{
          summary.bizKeys = Object.keys(biz);
          if (typeof biz.has_more !== "undefined") summary.hasMore = Boolean(biz.has_more);
          var sessions = biz.chat_sessions;
          if (Array.isArray(sessions)) {{
            summary.sessionCount = sessions.length;
            if (sessions.length > 0) {{
              summary.sessionKeys = Object.keys(sessions[0]);
              // 只报**长度**与**形状**：标题原文与会话 id 不进读数。
              summary.titleLengths = sessions.slice(0, 10).map(function (s) {{
                return String((s && s.title) || "").length;
              }});
              summary.hasSessionId = sessions.every(function (s) {{ return !!(s && s.id); }});
            }}
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

  async function attempt(label, url, init) {{
    try {{
      var res = await fetch(url, init);
      var text = await res.text();
      out.attempts.push({{
        label: label,
        url: String(url),
        status: res.status,
        ok: res.ok,
        contentType: res.headers.get("content-type"),
        summary: summarize(text)
      }});
    }} catch (e) {{
      out.attempts.push({{ label: label, url: String(url), error: String(e) }});
    }}
  }}

  var jsonHeaders = {{ "accept": "application/json", "content-type": "application/json" }};

  // ① 目录端点：GET（带 query，与 `official_chat_probe.rs` 的 site 模式同形）。
  await attempt("get-catalog", "{CATALOG_PATH}?count=5",
    {{ credentials: "include", headers: {{ "accept": "application/json" }} }});

  // ② 目录端点：POST（站点自己首屏发的那一发，带凭据头）。
  await attempt("post-catalog", "{CATALOG_PATH}",
    {{ method: "POST", credentials: "include", headers: jsonHeaders,
       body: JSON.stringify({{ p: 0, size: 50 }}) }});

  // ③ **对照臂**：故意打一个不存在的路径 ⇒ 期望非 200 / 非 JSON。
  //    这一发证明「fetch 真的到了服务器」，而不是被本地什么东西拦下了。
  await attempt("control-not-exist", "{CATALOG_PATH}/dsh_probe_control_not_exist",
    {{ credentials: "include", headers: {{ "accept": "application/json" }} }});

  return out;
}})()"#
        );

        let started = Instant::now();
        let outcome = round_trip(window, &expression, Duration::from_secs(45)).await;
        let elapsed = started.elapsed().as_millis() as u64;
        match outcome {
            Ok(trip) => json!({
                "ok": true,
                "elapsed_ms": elapsed,
                "polls": trip.polls,
                "result": trip.value,
            }),
            Err(e) => json!({ "ok": false, "elapsed_ms": elapsed, "error": format!("{e}") }),
        }
    }

    /// 实测主体（跑在 Tauri 事件循环起来之后）。
    async fn run_probe_body(
        app: &tauri::AppHandle,
        url: &Url,
        store_dir: PathBuf,
        store_reading: Value,
    ) -> String {
        let window = match build_window(app, url, &PROBE_DATA_STORE_IDENTIFIER) {
            Ok(window) => window,
            Err(e) => return json!({ "fatal": e }).to_string(),
        };

        let mut report = json!({
            "probe": "macOS official-chat route-B live probe",
            "platform": "macos",
            "site": SITE_URL,
            "catalog_path": CATALOG_PATH,
            "store": store_reading,
        });

        // --- ① 页面就绪 --------------------------------------------------------
        let ready = wait_for_site(&window, Duration::from_secs(45)).await;
        report["page_ready"] = match &ready {
            Ok(ms) => json!({ "ok": true, "elapsed_ms": ms }),
            Err(e) => json!({ "ok": false, "error": e }),
        };
        if ready.is_err() {
            report["verdict"] = verdict(&json!({}), &json!({}), &json!({}), &json!({}));
            report["store_target_dir"] = Value::String(store_dir.display().to_string());
            return report.to_string();
        }

        // --- ② 页面事实（凭据在不在）-------------------------------------------
        let facts = collect_page_facts(&window).await;
        report["page_facts"] = facts.clone();

        // --- ③ 原生求值 --------------------------------------------------------
        let native = measure_native_eval(&window).await;
        report["native_eval"] = native.clone();

        // --- ④ 异步结果回传 ----------------------------------------------------
        let trip = measure_round_trip(&window).await;
        report["round_trip"] = trip.clone();

        // --- ④' 站点的认证契约（页内观测 ＋ 页内重放）--------------------------
        let auth = measure_auth_contract(&window).await;
        report["auth_contract"] = auth.clone();

        // --- ④'' token 从哪来（页内比对）---------------------------------------
        let token_source = measure_token_source(&window).await;
        report["token_source"] = token_source.clone();

        // --- ⑤ 同源 fetch ------------------------------------------------------
        let catalog = measure_catalog_fetch(&window).await;
        report["catalog_fetch"] = catalog.clone();

        report["verdict"] = verdict(&native, &trip, &catalog, &auth);
        // token 能不能在页内读到：决定「路线 B 能不能落生产」。
        let hits = token_source
            .get("hits")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        report["verdict"]["token_source_readable_in_page"] = Value::Bool(!hits.is_empty());
        report["verdict"]["token_source_hits"] = Value::Array(hits);
        report["store_target_dir"] = Value::String(store_dir.display().to_string());
        report.to_string()
    }

    /// 把三块拼图的读数收成一句话结论。
    ///
    /// 判据**只看读数本身**，不预设结论：
    /// · 求值成立 ＝ 同步表达式读回了值；
    /// · 异步回传成立 ＝ `round_trip` 读回了 Promise settle 后的值；
    /// · 同源 fetch 成立 ＝ 目录端点至少一发拿到了 HTTP 响应（不论状态码）；
    /// · 登录态被服务端接受 ＝ 目录端点有一发 2xx 且 `chat_sessions` 条数 > 0。
    fn verdict(native: &Value, trip: &Value, catalog: &Value, auth: &Value) -> Value {
        let eval_ok = native
            .pointer("/sync_expression/ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // 原生求值对 Promise 拿到的**不是值**：wry 侧给的是空串或空对象。
        let native_promise_not_value = native
            .pointer("/promise_via_native_eval/raw")
            .and_then(Value::as_str)
            .map(|raw| {
                let trimmed = raw.trim();
                trimmed.is_empty() || trimmed == "{}" || trimmed == "null"
            })
            .unwrap_or(false);
        let trip_ok = trip.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let trip_value = trip.get("value").cloned().unwrap_or(Value::Null);

        let attempts = catalog
            .pointer("/result/attempts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let catalog_responded = attempts.iter().any(|a| a.get("status").is_some());
        let authenticated = attempts.iter().any(|a| {
            let status = a.get("status").and_then(Value::as_u64).unwrap_or(0);
            let count = a
                .pointer("/summary/sessionCount")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (200..300).contains(&status) && count > 0
        });
        let control = attempts
            .iter()
            .find(|a| a.get("label").and_then(Value::as_str) == Some("control-not-exist"))
            .cloned()
            .unwrap_or(Value::Null);

        // **页内重放**：用站点自己那一发的头（值留在页面里）重放目录请求。
        let replay = auth
            .pointer("/observed/replay")
            .cloned()
            .unwrap_or(Value::Null);
        let replay_status = replay.get("status").and_then(Value::as_u64).unwrap_or(0);
        let replay_count = replay
            .pointer("/sessionCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let replay_code = replay.get("code").cloned().unwrap_or(Value::Null);
        let replay_accepted = (200..300).contains(&replay_status) && replay_count > 0;

        // **最小头集**（只带 `authorization`）的重放结果。
        let minimal = auth
            .pointer("/observed/replayMinimal")
            .cloned()
            .unwrap_or(Value::Null);
        let minimal_status = minimal.get("status").and_then(Value::as_u64).unwrap_or(0);
        let minimal_count = minimal
            .pointer("/sessionCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let minimal_code = minimal.get("code").cloned().unwrap_or(Value::Null);
        let minimal_accepted = (200..300).contains(&minimal_status) && minimal_count > 0;

        json!({
            "eval_sync_ok": eval_ok,
            "native_promise_returns_object_not_value": native_promise_not_value,
            "round_trip_ok": trip_ok,
            "round_trip_value": trip_value,
            "catalog_endpoint_responded": catalog_responded,
            "catalog_authenticated_bare_fetch": authenticated,
            "control_arm": control,
            "replay_with_observed_headers": {
                "captured": auth.pointer("/observed/auth/captured").and_then(Value::as_bool).unwrap_or(false),
                "status": replay_status,
                "code": replay_code,
                "session_count": replay_count,
                "accepted": replay_accepted,
            },
            "replay_with_authorization_only": {
                "status": minimal_status,
                "code": minimal_code,
                "session_count": minimal_count,
                "accepted": minimal_accepted,
            },
            "route_b_on_macos": eval_ok && trip_ok && catalog_responded,
            "route_b_usable_on_macos": eval_ok && trip_ok && catalog_responded && replay_accepted,
            "note": "route_b_on_macos ＝「页内求值 ＋ 异步回传 ＋ 同源 fetch」三块拼图在 macOS 上成立；\
                     route_b_usable_on_macos ＝ 再加上「按站点契约构造的认证头能被服务端接受」。\
                     裸 fetch 被 40003 拒绝**不是平台问题**，是契约问题（站点自己手工加认证头）。",
        })
    }
}
