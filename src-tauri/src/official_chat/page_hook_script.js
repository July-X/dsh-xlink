// 页内挂钩脚本（**形态 C · 页内旁听**）—— 截住**站点自己**那一发目录请求，
// **取到即还原**。
//
// # 形态
//
// 壳侧把这段脚本当**一次页内求值**发进去（`script_eval::round_trip`）⇒ ⇒ 脚本
// **返回一个 Promise**，resolve 出来的是**结果对象**（壳侧拿到的是结构，不是凭据）。
//
// ⚠️ **顶层只有这一句 IIFE** ⇒ ⇒ 顶层不留 `var`／`function` ⇒ ⇒ 状态全在闭包里。
//
// # 三条硬约束（协议 §7.1 · v0.7 留档口径 —— 本形态用的就是这一条）
//
// ① **不新增 `window` 自有属性** —— 全部状态在 **async IIFE 的闭包**里；文件顶层
//    只有一句 IIFE ⇒ ⇒ 页内看不到任何新名字（`getOwnPropertyNames(window)`
//    前后**逐项一致**）。
// ② **取到即还原** —— 截到目录那一发（**拿到请求头**那一刻）就**同步还原**成
//    **替换前那个引用**（协议 §7.1 已实测：「正确写法是『还原成替换前那个引用』
//    （**不是**还原成原生）」）⇒ ⇒ **不留钩子**｜**不留定时器**（全篇**没有**
//    `setTimeout`／`setInterval`）｜**不留槽位**（没有 `window.__xxx`）。
// ③ **不写站点数据** —— 只读 GET；不点按钮、不发消息、不写 localStorage／
//    cookie／DOM。
//
// # 为什么**必须**等「站点自己那层」出现才装钩（**实测结论**，不是推断）
//
// · 装在站点那层**下面**（document-start 就猴补）⇒ ⇒ 站点那层的闭包里留着我们
//   那层 ⇒ ⇒ 还原后**残留看得见**（新建 XHR 实例仍带 `__mmx*` 标记；实测装钩→
//   还原窗口 2,566 ms）。
// · 装在站点那层**上面** ⇒ ⇒ 还原时把**我们这层整个摘掉** ⇒ ⇒ **残留归零**
//   （实测：残留探针看不到我们、XHR 实例 `__mmx*` 标记 `[]`、我们的层数 1 而不是 2；
//   代价是装钩窗口 6,238 ms —— 那一轮站点 +8.9 s 才发目录）。
// ⇒ ⇒ 判据：「`Function.prototype.toString.call(window.fetch)` 已不含
//    `[native code]`」＝ 站点自己那层出现了 ⇒ ⇒ **轮询到就装**（**不许固定睡眠**）。
//
// # 为什么**不用定时器**等那一层出现
//
// 协议 §7.1 约束②「不留定时器」⇒ ⇒ 全篇**没有** `setTimeout`／`setInterval`
// （上一轮探针用 `setInterval(hookAll, 50)` 反复补挂 ⇒ ⇒ 生产形态去掉）。
// ⇒ ⇒ 等待改成**有限次轮询 ＋ 一次宏任务让出**：`MessageChannel` 的一次往返就是
// 一次**宏任务**（消息任务 ＝ 任务队列里的一个任务，**不是定时器**）⇒ ⇒ 让出之后
// 站点自己的脚本与网络回调才能跑。（`scheduler.yield`／`scheduler.postTask` 在场
// 时优先用它们 —— 它们让浏览器先跑别的工作；`MessageChannel` 是兜底。）
//
// # 为什么**不是**「宿主驱动的分段求值」
//
// 分段求值要求脚本把状态**跨多次求值**保留下来 ⇒ ⇒ 必须落一个 `window` 槽位
// ⇒ ⇒ **违反约束①** ⇒ ⇒ 选「页内一次求值、闭包持有状态、Promise 回程」。
//
// # 凭据零外传（协议 §7.2）
//
// 签名头（`x-signature`／`x-timestamp`／`yy`）与 cookie **一步都不出页面**：在这里
// 取、在这里用、这里丢 ⇒ ⇒ 回给壳侧的**没有**任何凭据的值。
//
// ⚠️ **回给壳侧的是三样**：① **结构**（键名／条数／字节数／业务码／凭据
// **长度**）② **裁剪后的目录条目**（协议 §2.4 面二那 6 个键）③ **那一发的响应体
// 原文**（`capturedBody.text`）—— ③ 是给**壳侧那一套裁剪**用的（白名单逐字不变
// ⇒ ⇒ 裁剪只有**一处**、规格表仍是**唯一真相**）⇒ ⇒ ③ 里**不含**任何凭据。
//
// ⚠️ **单位换算不在这里做**：MiniMax 的 `updated_at` 是**毫秒**，协议要**秒级小数**
// ⇒ ⇒ 换算按规格表的 `ItemFields.updated_at_divisor` 在**壳侧**做（这里原样回）。
(async function () {
  var SPEC = __SPEC__;

  // ---- 规格（宿主注入；源码里不写站点路径字面量）--------------------------
  var CATALOG_PATH = String(SPEC.catalog_path || "");
  var WITHOUT_QUERY_KEY = String(SPEC.without_query_key || "");
  function matchesTarget(url) {
    if (!CATALOG_PATH || url.indexOf(CATALOG_PATH) < 0) return false;
    if (!WITHOUT_QUERY_KEY) return true;
    try {
      return !new URL(url, window.location.href).searchParams.has(WITHOUT_QUERY_KEY);
    } catch (e) { return false; }
  }
  var SIGNATURE_HEADERS = (
    SPEC.signature_headers || ["x-signature", "x-timestamp", "yy"]
  ).map(function (name) {
    return String(name).toLowerCase();
  });
  var POLL_BUDGET_MS = Number(SPEC.poll_budget_ms || 30000);
  var MAX_POLL_ATTEMPTS = Number(SPEC.max_poll_attempts || 200000);
  var REPLAY_BODY = SPEC.replay_body !== false;
  var POSITIVE_CONTROL = SPEC.positive_control !== false;
  var BODY_TEMPLATE = String(SPEC.body_path_template || "");
  var BODY_QUERY = String(SPEC.body_query || "");
  var SESSIONS_PATH = SPEC.sessions_path || ["sessions"];
  var ITEM_PATH = SPEC.item_path || [];
  var ID_PATH = SPEC.id_path || ["session_id"];
  var ITEM_FIELDS = SPEC.item_fields || {};

  // ---- 状态（**全在闭包里** ⇒ 顶层不留任何名字）--------------------------
  var startedAt = Date.now();
  var nativeFetch = null; // 脚本开始那一刻的 window.fetch
  var nativeAtStart = true; // 那一刻它是不是原生的
  var prevFetch = null; // **我们替换掉的那个引用**（＝站点自己那层）
  var prevFetchSrcLength = -1; // 站点那层的源码长度（**还原后应当还是它**）
  var prevFetchIsNative = null; // 站点那层是不是原生（本站：**不是**）
  var prevXhr = null; // 三个 XHR 原型方法**替换前**的引用
  var installed = false;
  var restored = false;
  var restoreReading = null;
  var captured = null; // {url, method, pairs, headersObj, at}
  var selfCall = false; // 我们自己发的请求（不进捕获）
  var catalogBodyText = null;
  var catalogStatus = null;
  var positiveControlReading = null; // 阳性对照那一发的读数（**在还原之前**发出去的）
  var bodyResolve = null;
  var bodyPromise = new Promise(function (resolve) {
    bodyResolve = resolve;
  });
  var lastChannel = null; // 保住 MessageChannel 引用（否则可能被 GC ⇒ 消息不到）

  // ---- 判据用的源码标记（还原后 `fetch.toString()` 里**不含**这一段）------
  var OUR_MARKER = "dsh-page-hook-marker";

  // ---- 小工具 -------------------------------------------------------------
  function nowMs() {
    return Date.now();
  }

  function asText(value) {
    try {
      return value === null || typeof value === "undefined" ? "" : String(value);
    } catch (e) {
      return "";
    }
  }

  function asNumber(value) {
    try {
      var n = Number(value);
      return isFinite(n) ? n : 0;
    } catch (e) {
      return 0;
    }
  }

  /** 按路径取值（**空路径 ＝ 没有这一栏** ⇒ 回默认值，不许拿别的字段顶替）。 */
  function pickByPath(root, path) {
    if (!path || !path.length) return undefined;
    var cur = root;
    for (var i = 0; i < path.length; i++) {
      if (cur === null || typeof cur === "undefined") return undefined;
      try {
        cur = cur[path[i]];
      } catch (e) {
        return undefined;
      }
    }
    return cur;
  }

  function pickText(root, path) {
    if (!path || !path.length) return "";
    var value = pickByPath(root, path);
    return typeof value === "undefined" || value === null ? "" : asText(value);
  }

  function srcOf(fn) {
    try {
      return Function.prototype.toString.call(fn);
    } catch (e) {
      return "";
    }
  }

  function isNativeFn(fn) {
    try {
      return srcOf(fn).indexOf("[native code]") >= 0;
    } catch (e) {
      return false;
    }
  }

  function ownNames() {
    try {
      return Object.getOwnPropertyNames(window);
    } catch (e) {
      return [];
    }
  }

  // ---- 让出一次宏任务（**不是定时器**）------------------------------------
  //
  // ⚠️ **为什么不用 `setTimeout`／`setInterval`**：协议 §7.1 约束②「不留定时器」
  //    ⇒ ⇒ 全篇没有这两个调用 ⇒ ⇒ 改用 `MessageChannel` 的一次往返（消息任务 ＝
  //    任务队列里的一个任务，**不是定时器**）让出一次宏任务 ⇒ ⇒ 站点自己的脚本
  //    与网络回调因此能跑。
  function yieldTask() {
    return new Promise(function (resolve) {
      try {
        if (
          typeof scheduler === "object" &&
          scheduler &&
          typeof scheduler.yield === "function"
        ) {
          scheduler.yield(function () {
            resolve();
          });
          return;
        }
      } catch (e) {
        /* 落到下一档 */
      }
      try {
        if (
          typeof scheduler === "object" &&
          scheduler &&
          typeof scheduler.postTask === "function"
        ) {
          scheduler.postTask(
            function () {
              resolve();
            },
            { priority: "background" }
          );
          return;
        }
      } catch (e2) {
        /* 落到下一档 */
      }
      try {
        if (typeof MessageChannel === "function") {
          var ch = new MessageChannel();
          lastChannel = ch;
          ch.port1.onmessage = function () {
            resolve();
          };
          ch.port2.postMessage(0);
          return;
        }
      } catch (e3) {
        /* 落到下一档 */
      }
      // ⚠️ **最后一档**：微任务让出（只让出一次微任务队列 ⇒ 站点自己的宏任务
      //    仍要等）⇒ ⇒ 只在连 `MessageChannel` 都没有的引擎上才会走到。
      resolve();
    });
  }

  // ---- 请求头（**名字 ＋ 长度**进读数；**值留在页内**给重放用）--------------
  function headerPairsOf(input, init) {
    var pairs = [];
    try {
      var headers = null;
      try {
        if (init && init.headers) headers = init.headers;
      } catch (e) {
        /* 忽略 */
      }
      try {
        if (!headers && input && input.headers) headers = input.headers;
      } catch (e2) {
        /* 忽略 */
      }
      if (!headers) return pairs;
      if (typeof headers.forEach === "function") {
        headers.forEach(function (value, key) {
          pairs.push([asText(key), asText(value)]);
        });
      } else if (Array.isArray(headers)) {
        for (var i = 0; i < headers.length; i++) {
          try {
            if (headers[i] && headers[i].length >= 2) {
              pairs.push([asText(headers[i][0]), asText(headers[i][1])]);
            }
          } catch (e3) {
            /* 忽略 */
          }
        }
      } else {
        var keys = Object.keys(headers);
        for (var j = 0; j < keys.length; j++) {
          try {
            pairs.push([asText(keys[j]), asText(headers[keys[j]])]);
          } catch (e4) {
            /* 忽略 */
          }
        }
      }
    } catch (e5) {
      /* 忽略 */
    }
    return pairs;
  }

  function pairsToObject(pairs) {
    var out = {};
    for (var i = 0; i < pairs.length; i++) {
      try {
        out[asText(pairs[i][0])] = asText(pairs[i][1]);
      } catch (e) {
        /* 忽略 */
      }
    }
    return out;
  }

  function headerShape(pairs) {
    var names = [];
    var lengths = {};
    for (var i = 0; i < pairs.length; i++) {
      var name = asText(pairs[i][0]);
      names.push(name);
      lengths[name.toLowerCase()] = asText(pairs[i][1]).length;
    }
    return { names: names, lengths: lengths };
  }

  /** 摘掉签名三件套 ⇒ **反向对照**用（证明读到的不是空壳）。 */
  function stripSignature(pairs) {
    var headers = {};
    var kept = [];
    for (var i = 0; i < pairs.length; i++) {
      var name = asText(pairs[i][0]);
      var lower = name.toLowerCase();
      var isSignature = false;
      for (var k = 0; k < SIGNATURE_HEADERS.length; k++) {
        if (lower === SIGNATURE_HEADERS[k]) {
          isSignature = true;
          break;
        }
      }
      if (isSignature) continue;
      headers[name] = asText(pairs[i][1]);
      kept.push(name);
    }
    return { headers: headers, kept: kept };
  }

  // ---- 响应体 → **只报结构**（状态码／业务码／键名／条数／字节数）----------
  //
  // ⚠️ **标题原文、会话 id、消息正文一律不进读数**（只报长度与键名）。
  function summarize(status, text) {
    var out = {
      status: asNumber(status),
      bodyLength: asText(text).length,
    };
    var json = null;
    try {
      json = JSON.parse(asText(text));
    } catch (e) {
      out.json = false;
      return out;
    }
    out.json = true;
    try {
      out.topKeys = Object.keys(json || {}).slice(0, 24);
    } catch (e2) {
      out.topKeys = [];
    }
    try {
      if (json && json.base_resp && typeof json.base_resp === "object") {
        out.baseRespKeys = Object.keys(json.base_resp).slice(0, 10);
        if (typeof json.base_resp.status_code !== "undefined") {
          out.bizCode = json.base_resp.status_code;
        }
        if (typeof json.base_resp.status_msg === "string") {
          out.bizMsg = asText(json.base_resp.status_msg).slice(0, 120);
        }
      }
      if (json && Object.prototype.hasOwnProperty.call(json, "has_more")) {
        out.hasMore = json.has_more;
      }
      var sessions = pickByPath(json, SESSIONS_PATH);
      if (Array.isArray(sessions)) {
        out.sessionsCount = sessions.length;
        if (sessions.length) {
          out.firstElementKeys = Object.keys(sessions[0] || {}).slice(0, 24);
          var item = ITEM_PATH.length ? pickByPath(sessions[0], ITEM_PATH) : sessions[0];
          if (item && typeof item === "object") {
            out.firstItemKeys = Object.keys(item).slice(0, 32);
          }
        }
      }
      if (json && Array.isArray(json.messages)) {
        out.messagesCount = json.messages.length;
        if (json.messages.length) {
          out.firstMessageKeys = Object.keys(json.messages[0] || {}).slice(0, 32);
          var roles = {};
          for (var i = 0; i < json.messages.length; i++) {
            var role = (json.messages[i] || {}).role;
            var key =
              typeof role === "undefined" || role === null ? "<absent>" : asText(role);
            roles[key] = (roles[key] || 0) + 1;
          }
          out.roleCounts = roles;
          var hasContent = false;
          var hasThinking = false;
          for (var j = 0; j < json.messages.length; j++) {
            var m = json.messages[j] || {};
            if (typeof m.msg_content !== "undefined") hasContent = true;
            if (typeof m.thinking_content !== "undefined") hasThinking = true;
          }
          out.hasMsgContent = hasContent;
          out.hasThinkingContent = hasThinking;
        }
      }
      if (json && typeof json.error !== "undefined") {
        out.errorText = asText(json.error).slice(0, 120);
      }
    } catch (e3) {
      /* 结构读不出来就只报状态码与字节数 */
    }
    return out;
  }

  // ---- 痕迹快照（**还原前 / 还原后**各一份）-------------------------------
  function residueSnapshot() {
    var s = {};
    try {
      s.windowOwnCount = ownNames().length;
      var names = ownNames();
      var marks = [];
      for (var i = 0; i < names.length; i++) {
        var n = asText(names[i]);
        if (n.indexOf("__dsh") === 0 || n.indexOf("__mmx") === 0) marks.push(n);
      }
      s.probeOwnNames = marks;
    } catch (e) {
      s.windowOwnCount = -1;
      s.probeOwnNames = [];
    }
    try {
      var src = srcOf(window.fetch);
      s.fetchSrcLength = src.length;
      s.fetchHasOurSource = src.indexOf(OUR_MARKER) >= 0;
      s.fetchIsNative = src.indexOf("[native code]") >= 0;
    } catch (e2) {
      s.fetchSrcError = asText(e2).slice(0, 120);
    }
    try {
      s.xhrOpenIsNative = isNativeFn(XMLHttpRequest.prototype.open);
      s.xhrOpenHasOurSource = srcOf(XMLHttpRequest.prototype.open).indexOf(OUR_MARKER) >= 0;
    } catch (e3) {
      /* 忽略 */
    }
    // ⚠️ **新建一个 XHR 实例** ⇒ 看它身上还有没有 `__mmx*` 标记。
    //    必须**调一次 `open()`**（**不发请求** —— 只 `open` 不 `send` ⇒ 零网络）
    //    ⇒ 原型上的钩子还在时，`open()` 会往实例上打 `__mmxMethod/__mmxUrl/
    //    __mmxPairs` ⇒ ⇒ 这就是「我们那层还在不在链上」的探针。
    try {
      var xhr = new XMLHttpRequest();
      var before = Object.getOwnPropertyNames(xhr);
      s.newXhrOwnPropsBeforeOpen = before.length;
      var marksBefore = [];
      for (var k = 0; k < before.length; k++) {
        if (asText(before[k]).indexOf("__mmx") === 0) marksBefore.push(asText(before[k]));
      }
      s.newXhrHookMarksBeforeOpen = marksBefore;
      try {
        xhr.open("GET", asText(location.href));
      } catch (e4) {
        s.newXhrOpenError = asText(e4).slice(0, 120);
      }
      var after = Object.getOwnPropertyNames(xhr);
      s.newXhrOwnPropsAfterOpen = after.length;
      var marksAfter = [];
      for (var m = 0; m < after.length; m++) {
        if (asText(after[m]).indexOf("__mmx") === 0) marksAfter.push(asText(after[m]));
      }
      s.newXhrHookMarksAfterOpen = marksAfter;
    } catch (e5) {
      s.newXhrError = asText(e5).slice(0, 120);
    }
    return s;
  }

  // ---- 还原（**还原成替换前那个引用**）-------------------------------------
  //
  // ⚠️ **协议 §7.1 已实测**：正确写法是「还原成**替换前那个引用**」（**不是**还原
  //    成原生）⇒ ⇒ 我们把 `prevFetch` / `prevXhr` 记在装钩那一刻 ⇒ ⇒ 还原就是
  //    把它们**放回去**。
  function restore(reason) {
    if (restored) return restoreReading;
    restored = true;
    var pre = residueSnapshot();
    try {
      if (prevFetch) window.fetch = prevFetch;
    } catch (e) {
      /* 忽略 */
    }
    try {
      if (prevXhr) {
        XMLHttpRequest.prototype.open = prevXhr.open;
        XMLHttpRequest.prototype.setRequestHeader = prevXhr.setRequestHeader;
        XMLHttpRequest.prototype.send = prevXhr.send;
      }
    } catch (e2) {
      /* 忽略 */
    }
    var post = residueSnapshot();
    restoreReading = {
      at: nowMs(),
      reason: asText(reason),
      windowOwnCountBefore: pre.windowOwnCount,
      windowOwnCountAfter: post.windowOwnCount,
      // ⚠️ **这一格是「还原前后」的自比** ⇒ ⇒ 期望 0（页面的自然增长不算在内）。
      windowOwnCountDelta: post.windowOwnCount - pre.windowOwnCount,
      probeOwnNames: post.probeOwnNames,
      fetchSrcLength: post.fetchSrcLength,
      // ⚠️ **期望 false**：还原后 `window.fetch` 的源码里**不含**我们的那一段。
      fetchHasOurSource: post.fetchHasOurSource,
      fetchIsNative: post.fetchIsNative,
      xhrOpenHasOurSource: post.xhrOpenHasOurSource,
      // ⚠️ **期望空数组**：还原后新建的 XHR 实例**不带** `__mmx*` 标记。
      newXhrHookMarks: post.newXhrHookMarksAfterOpen,
      newXhrHookMarksBeforeOpen: post.newXhrHookMarksBeforeOpen,
      restoredFetchIsPrev: (function () {
        try {
          return window.fetch === prevFetch;
        } catch (e3) {
          return false;
        }
      })(),
      restoredXhrOpenIsPrev: (function () {
        try {
          return XMLHttpRequest.prototype.open === prevXhr.open;
        } catch (e4) {
          return false;
        }
      })(),
      // ⚠️ **这两格量的是「我们那层」**（还原**之前**快照里的 `window.fetch`
      //    ＝ 我们的包装）⇒ ⇒ **不是**「站点那层」⇒ ⇒ 站点那层的读数在
      //    `install.onTopOfSrcLength`。
      ourWrapperSrcLengthBeforeRestore: pre.fetchSrcLength,
      ourWrapperIsNativeBeforeRestore: pre.fetchIsNative,
    };
    return restoreReading;
  }

  // ---- 装钩（**套在站点自己那层「上面」**）--------------------------------
  function install() {
    if (installed) return;
    installed = true;
    prevFetch = window.fetch; // ← **替换前那个引用**（＝站点自己那层）
    // ⚠️ **站点那层自己的源码长度／是不是原生**（**还原后应当还是它** ⇒ ⇒ 这是
    //    「还原成替换前那个引用」的读数，不是「我们那层」的）。
    try {
      prevFetchSrcLength = srcOf(prevFetch).length;
      prevFetchIsNative = isNativeFn(prevFetch);
    } catch (e) {
      /* 忽略 */
    }
    prevXhr = {
      open: XMLHttpRequest.prototype.open,
      setRequestHeader: XMLHttpRequest.prototype.setRequestHeader,
      send: XMLHttpRequest.prototype.send,
    };
    window.fetch = makeFetchWrapper();
    XMLHttpRequest.prototype.open = makeXhrOpenWrapper();
    XMLHttpRequest.prototype.setRequestHeader = makeXhrHeaderWrapper();
    XMLHttpRequest.prototype.send = makeXhrSendWrapper();
  }

  // ---- fetch 挂钩 ---------------------------------------------------------
  function makeFetchWrapper() {
    var wrapper = function (input, init) {
      /* dsh-page-hook-marker：判据用 —— 还原后 `fetch.toString()` 里**不含**它。
         ⚠️ **这条注释必须在函数体里面** ⇒ `Function.prototype.toString` 才会
         把它带出来 ⇒ ⇒ 「源码里含不含我们」这一格才是**真判据**（放在函数
         **外面**的话 ⇒ 那一格恒为 false ⇒ ⇒ 断言成了摆设）。 */
      var url = "";
      try {
        url = typeof input === "string" ? input : (input && input.url) || "";
      } catch (e) {
        /* 忽略 */
      }
      var isTarget =
        !selfCall && !captured && matchesTarget(url);
      var pairs = [];
      var method = "GET";
      if (isTarget) {
        try {
          pairs = headerPairsOf(input, init);
        } catch (e2) {
          /* 忽略 */
        }
        try {
          method = asText(
            (init && init.method) || (input && input.method) || "GET"
          );
        } catch (e3) {
          /* 忽略 */
        }
      }
      var res;
      try {
        // ⚠️ **调「替换前那个引用」**（不是原生）⇒ ⇒ 站点那层该做的事一件不少。
        res = prevFetch.apply(this, arguments);
      } catch (e4) {
        return Promise.reject(e4);
      }
      if (!isTarget) return res;
      // 截到目录那一发 ⇒ 记下**请求头** ⇒ **立刻还原**（痕迹窗口到此为止）。
      captured = {
        url: asText(url),
        method: method,
        pairs: pairs,
        headersObj: pairsToObject(pairs),
        at: nowMs(),
      };
      var control = null;
      if (POSITIVE_CONTROL) control = issuePositiveControl(captured);
      restore("截到目录那一发 ⇒ 立刻还原");
      // 响应体：**克隆**一份读（**不动站点自己那份流**）⇒ ⇒ 还原之后照样读得到。
      try {
        res.then(
          function (response) {
            try {
              var cloned = response.clone();
              cloned.text().then(
                function (text) {
                  catalogStatus = response.status;
                  catalogBodyText = asText(text);
                  bodyResolve({ ok: true });
                },
                function (err) {
                  bodyResolve({ ok: false, error: asText(err).slice(0, 120) });
                }
              );
            } catch (e5) {
              bodyResolve({ ok: false, error: asText(e5).slice(0, 120) });
            }
          },
          function (err) {
            bodyResolve({ ok: false, error: asText(err).slice(0, 120) });
          }
        );
      } catch (e6) {
        bodyResolve({ ok: false, error: asText(e6).slice(0, 120) });
      }
      return res;
    };
    return wrapper;
  }

  // ---- XHR 挂钩（**站点实测走的是 fetch** ⇒ 这一档是兜底）-----------------
  //
  // ⚠️ **为什么也补 XHR**：站点哪天换通道时我们不至于**完全看不到**那一发 ⇒ ⇒
  //    代价是**多三处原型替换** ⇒ ⇒ 但还原时同样**还原成替换前那个引用** ⇒ ⇒
  //    残留归零（实测：还原后新建 XHR 实例 `__mmx*` 标记 `[]`）。
  function makeXhrOpenWrapper() {
    var wrapper = function (method, url) {
      /* dsh-page-hook-marker：判据用（XHR 那一档）—— 见 `makeFetchWrapper` 里
         那条注释：**标记必须在函数体里面**，否则那一格恒为 false。 */
      try {
        this.__mmxMethod = asText(method);
        this.__mmxUrl = asText(url);
        this.__mmxPairs = [];
      } catch (e) {
        /* 忽略 */
      }
      return prevXhr.open.apply(this, arguments);
    };
    return wrapper;
  }

  function makeXhrHeaderWrapper() {
    var wrapper = function (name, value) {
      /* dsh-page-hook-marker：判据用（XHR 那一档） */
      try {
        if (!this.__mmxPairs) this.__mmxPairs = [];
        this.__mmxPairs.push([asText(name), asText(value)]);
      } catch (e) {
        /* 忽略 */
      }
      return prevXhr.setRequestHeader.apply(this, arguments);
    };
    return wrapper;
  }

  function makeXhrSendWrapper() {
    var wrapper = function () {
      /* dsh-page-hook-marker：判据用（XHR 那一档） */
      try {
        var url = asText(this.__mmxUrl);
        var isTarget =
          !selfCall && !captured && matchesTarget(url);
        if (isTarget) {
          var self = this;
          var pairs = this.__mmxPairs || [];
          captured = {
            url: url,
            method: asText(this.__mmxMethod || "GET"),
            pairs: pairs,
            headersObj: pairsToObject(pairs),
            at: nowMs(),
          };
          if (POSITIVE_CONTROL) issuePositiveControl(captured);
          restore("截到目录那一发（XHR） ⇒ 立刻还原");
          try {
            this.addEventListener("load", function () {
              var text = null;
              var err = null;
              try {
                text = self.responseText;
              } catch (e) {
                err = "responseText 不可读";
              }
              if (err) {
                bodyResolve({ ok: false, error: err });
                return;
              }
              catalogStatus = self.status;
              catalogBodyText = asText(text);
              bodyResolve({ ok: true });
            });
          } catch (e2) {
            bodyResolve({ ok: false, error: asText(e2).slice(0, 120) });
          }
        }
      } catch (e3) {
        /* 忽略 */
      }
      return prevXhr.send.apply(this, arguments);
    };
    return wrapper;
  }

  // ---- 阳性对照：**在还原之前**用截到的签名头重放那一发 --------------------
  //
  // ⚠️ **为什么必须在还原之前发**：这一臂要证明的是「**钩子还在**的时候，重放也
  //    照样 200」⇒ ⇒ 它是「截到的签名头确实可用」的**阳性对照** ⇒ ⇒ 与反向对照
  //    （摘掉签名 ⇒ 400）成对。
  function issuePositiveControl(hit) {
    var out = { issuedAt: nowMs(), issuedBeforeRestore: true, urlPath: CATALOG_PATH };
    positiveControlReading = out;
    try {
      selfCall = true;
      var p = fetch(hit.url, {
        method: hit.method || "GET",
        headers: hit.headersObj,
        credentials: "include",
      });
      selfCall = false;
      p.then(
        function (response) {
          out.status = response.status;
          return response.text().then(
            function (text) {
              out.summary = summarize(response.status, text);
              out.done = true;
            },
            function (err) {
              out.error = asText(err).slice(0, 120);
              out.done = true;
            }
          );
        },
        function (err) {
          out.error = asText(err).slice(0, 120);
          out.done = true;
        }
      );
    } catch (e) {
      selfCall = false;
      out.error = asText(e).slice(0, 120);
      out.done = true;
    }
    return out;
  }

  // ---- 拼正文路径（**session_id 全程留在页内**）---------------------------
  function buildBodyUrl() {
    var out = { ok: false };
    if (!catalogBodyText) {
      out.error = "没有拿到目录响应体";
      return out;
    }
    var json = null;
    try {
      json = JSON.parse(catalogBodyText);
    } catch (e) {
      out.error = "目录响应体不是 JSON";
      return out;
    }
    var sessions = pickByPath(json, SESSIONS_PATH);
    if (!Array.isArray(sessions) || !sessions.length) {
      out.error = "目录里没有会话条目";
      return out;
    }
    var item = ITEM_PATH.length ? pickByPath(sessions[0], ITEM_PATH) : sessions[0];
    var id = pickText(item, ID_PATH);
    if (!id) {
      out.error = "第一条会话里取不到 id";
      return out;
    }
    out.sessionIdLength = id.length;
    out.bodyPathShape = BODY_TEMPLATE.replace("{session}", "<REDACTED>");
    // 目录那一发的 query：**去掉** `limit`／`project_id`（那是目录自己的分页参数）
    // ⇒ ⇒ 其余原样带到正文那一发（实测：换 query 不影响签名）。
    var kept = [];
    try {
      var queryIndex = asText(captured.url).indexOf("?");
      if (queryIndex >= 0) {
        var parts = asText(captured.url).slice(queryIndex + 1).split("&");
        for (var i = 0; i < parts.length; i++) {
          if (!parts[i]) continue;
          var key = parts[i].split("=")[0];
          if (key === "limit" || key === "project_id") continue;
          kept.push(parts[i]);
        }
      }
    } catch (e2) {
      /* 忽略 */
    }
    var url =
      asText(location.origin) +
      BODY_TEMPLATE.replace("{session}", encodeURIComponent(id)) +
      "?" +
      BODY_QUERY +
      (kept.length ? "&" + kept.join("&") : "");
    out.url = url;
    out.queryKeys = [];
    for (var k = 0; k < kept.length; k++) out.queryKeys.push(kept[k].split("=")[0]);
    out.ok = true;
    return out;
  }

  // ---- 正文重放（**用截到的签名头**；页内发 ⇒ 凭据不出页）----------------
  async function replayBody(hit, headers) {
    var out = { ok: false };
    var prep = buildBodyUrl();
    out.prep = {
      ok: prep.ok,
      error: prep.error,
      sessionIdLength: prep.sessionIdLength,
      bodyPathShape: prep.bodyPathShape,
      queryKeys: prep.queryKeys,
    };
    if (!prep.ok) return out;
    var res = null;
    var text = "";
    try {
      selfCall = true;
      res = await fetch(prep.url, {
        method: "GET",
        headers: headers,
        credentials: "include",
      });
      text = await res.text();
    } catch (e) {
      selfCall = false;
      out.error = asText(e).slice(0, 160);
      return out;
    } finally {
      selfCall = false;
    }
    out.ok = true;
    out.summary = summarize(res.status, text);
    return out;
  }

  // ---- 「装钩前站点是不是已经发过目录那一发」（**只读**判据）---------------
  //
  // ⚠️ **为什么要有这一格**：站点自己那一发**可能**在我们装钩之前就发生了
  //    （那一次响应体我们拿不到）⇒ ⇒ 这一格用 **Resource Timing**（**只读**，
  //    **不留任何痕迹**）告诉我们「是不是已经晚了」⇒ ⇒ 错因才分得清。
  function catalogAlreadyRequested() {
    try {
      var entries = performance.getEntriesByType("resource") || [];
      for (var i = 0; i < entries.length; i++) {
        var name = asText(entries[i] && entries[i].name);
        if (CATALOG_PATH && name.indexOf(CATALOG_PATH) >= 0) return true;
      }
    } catch (e) {
      /* 读不到就当「没有」 */
    }
    return false;
  }

  // ---- 等「站点自己那层」出现（**条件等待**；不许固定睡眠）----------------
  //
  // ⚠️ **为什么先等「文档解析完」而不是直接轮询**：站点的 bundle 是在**解析期**
  //    执行的（普通脚本／`defer`／`module` **都**在 `DOMContentLoaded` 之前跑完）
  //    ⇒ ⇒ 站点自己那层**通常在那一刻就已经在了** ⇒ ⇒ **不必轮询** ⇒ ⇒ 既不占
  //    CPU，**也不会把宿主的求值通道饿死**。
  //
  // ⚠️ **实测踩过的坑（2026-10-23 r7/r8）**：一开始是**连续投递 `MessageChannel`
  //    的紧轮询** ⇒ ⇒ 宿主那一发的求值在 **2 s 内没有回应**（`round_trip` 报
  //    「页面在请求途中不在了」）⇒ ⇒ **整轮失败** ⇒ ⇒ 紧轮询这条路**不能走**
  //    ⇒ ⇒ 改成「**事件驱动优先**（`DOMContentLoaded`）＋ **有限次**轮询兜底」。
  function waitForDocumentParsed() {
    return new Promise(function (resolve) {
      var settled = false;
      function fin(how) {
        if (settled) return;
        settled = true;
        resolve(how);
      }
      try {
        if (String(document.readyState) !== "loading") {
          fin("already-parsed");
          return;
        }
      } catch (e) {
        fin("unknown");
        return;
      }
      try {
        document.addEventListener(
          "DOMContentLoaded",
          function () {
            fin("DOMContentLoaded");
          },
          { once: true }
        );
      } catch (e2) {
        fin("listen-failed");
        return;
      }
      // ⚠️ **兜底**：万一那个事件不来（页面已在解析末尾）⇒ ⇒ **有限次**让出后放弃
      //    （**不用定时器** ⇒ 只能靠有限次让出）。
      var ticks = 0;
      (function tick() {
        ticks++;
        if (settled) return;
        try {
          if (String(document.readyState) !== "loading") {
            fin("readyState-changed");
            return;
          }
        } catch (e3) {
          fin("unknown");
          return;
        }
        if (ticks > 400) {
          fin("ticks-exhausted");
          return;
        }
        yieldTask().then(tick);
      })();
    });
  }

  // ---- 「站点自己那层」判据（**恒等比较** ⇒ 每轮 O(1)）--------------------
  function siteLayerPresent() {
    try {
      // 脚本开始那一刻它就已经**不是**原生的 ⇒ ⇒ 站点那层**早于**我们存在。
      if (!nativeAtStart) return true;
      // 站点换过它 ⇒ ⇒ 站点那层出现了。
      return window.fetch !== nativeFetch;
    } catch (e) {
      return false;
    }
  }

  // ---- 失败结果（**可区分的错因**）----------------------------------------
  function failure(stage, message, extra) {
    var out = {
      ok: false,
      stage: asText(stage),
      error: asText(message),
      startedAt: startedAt,
      finishedAt: nowMs(),
    };
    if (extra) {
      for (var key in extra) {
        if (Object.prototype.hasOwnProperty.call(extra, key)) out[key] = extra[key];
      }
    }
    try {
      out.windowOwnCountAtStart = ownNames().length;
    } catch (e) {
      /* 忽略 */
    }
    return out;
  }

  // ---- 主体 ---------------------------------------------------------------
  try {
    // ① 记下「脚本开始那一刻」的 fetch 引用（**判据基准**）。
    nativeFetch = window.fetch;
    nativeAtStart = isNativeFn(nativeFetch);

    // ② 等「站点自己那层」出现（**条件等待**，不许固定睡眠）。
    //    ②a 先等文档解析完（**事件驱动** ⇒ 不轮询 ⇒ 不饿死宿主通道）。
    var parsedHow = await waitForDocumentParsed();
    var parsedAt = nowMs();
    // ⚠️ **这一格必须在装钩之前取**（还原之后再取就没意义了）：文档解析完那一刻，
    //    站点自己那层**在不在**。
    var layerAtParsed = siteLayerPresent();
    //    ②b 那层**通常**这时已经在 ⇒ ⇒ 先查一次；不在才进**有限次**轮询。
    var deadline = nowMs() + POLL_BUDGET_MS;
    var attempts = 0;
    var layerSeenAt = 0;
    while (attempts < MAX_POLL_ATTEMPTS && nowMs() < deadline) {
      attempts++;
      if (siteLayerPresent()) {
        layerSeenAt = nowMs();
        break;
      }
      await yieldTask();
    }
    if (!layerSeenAt) {
      // ⚠️ **如实报**：站点那层始终没出现 ⇒ ⇒ **不硬装**（硬装会落在站点那层
      //    **下面** ⇒ ⇒ 还原后有残留）⇒ ⇒ 这一档是**可区分的错因**。
      return failure("site-layer-missing", "站点自己那层 fetch 始终没出现", {
        waitedMs: nowMs() - startedAt,
        attempts: attempts,
      });
    }

    // ③ 装钩（**套在站点自己那层「上面」** ⇒ ⇒ 还原后残留归零）。
    var alreadyRequested = catalogAlreadyRequested();
    install();
    var installedAt = nowMs();

    // ④ 等目录那一发被截到（**条件等待**）。
    var captureDeadline = nowMs() + POLL_BUDGET_MS;
    while (!captured && nowMs() < captureDeadline && attempts < MAX_POLL_ATTEMPTS) {
      attempts++;
      await yieldTask();
    }
    if (!captured) {
      var restoreMiss = restore("没截到目录那一发 ⇒ 收工还原");
      // ⚠️ **两种错因分开报**：站点那一发**是不是已经发过了**（那一次我们拿不到）。
      var missStage = alreadyRequested
        ? "catalog-already-happened"
        : "catalog-not-captured";
      var missError = alreadyRequested
        ? "装钩之前站点已经发过目录那一发（响应体拿不到），且之后没有再发"
        : "没截到目录那一发";
      return failure(missStage, missError, {
        waitedMs: nowMs() - startedAt,
        attempts: attempts,
        catalogAlreadyRequestedBeforeInstall: alreadyRequested,
        restore: restoreMiss,
      });
    }

    // ⑤ 等目录响应体读完（**克隆体**读 ⇒ ⇒ 与还原无关）。
    var bodyRead = await bodyPromise;
    var capturedAt = captured.at;
    var catalogSummary = catalogBodyText
      ? summarize(catalogStatus, catalogBodyText)
      : { status: catalogStatus, bodyUnavailable: (bodyRead && bodyRead.error) || "响应体没读到" };

    // ⑥ 目录条目（**裁剪成协议 §2.4 面二那 6 个键**；会话 id／标题**允许**出壳）。
    var items = [];
    try {
      var json = JSON.parse(catalogBodyText);
      var sessions = pickByPath(json, SESSIONS_PATH);
      if (Array.isArray(sessions)) {
        for (var i = 0; i < sessions.length; i++) {
          var raw = ITEM_PATH.length ? pickByPath(sessions[i], ITEM_PATH) : sessions[i];
          if (!raw || typeof raw !== "object") continue;
          items.push({
            id: pickText(raw, ID_PATH),
            title: pickText(raw, ITEM_FIELDS.title || []),
            title_type: pickText(raw, ITEM_FIELDS.title_type || []),
            model_type: pickText(raw, ITEM_FIELDS.model_type || []),
            pinned: !!(ITEM_FIELDS.pinned || []).length
              ? !!pickByPath(raw, ITEM_FIELDS.pinned)
              : false,
            updated_at: (ITEM_FIELDS.updated_at || []).length
              ? asNumber(pickByPath(raw, ITEM_FIELDS.updated_at))
              : 0,
          });
        }
      }
    } catch (e) {
      /* 目录条目读不出来就回空数组（**如实**，不编） */
    }

    // ⑦ 正文重放（**还原之后**；用截到的签名头 ⇒ 凭据不出页）。
    var bodyReplay = null;
    var unsignedReplay = null;
    if (REPLAY_BODY) {
      bodyReplay = await replayBody(captured, captured.headersObj);
      var stripped = stripSignature(captured.pairs);
      unsignedReplay = await replayBody(captured, stripped.headers);
      unsignedReplay.strippedHeaderNames = stripped.kept;
    }

    // ⑧ 阳性对照（**在还原之前**发的那一发）的读数。
    var positive = null;
    try {
      positive = await waitForControlDone();
    } catch (e) {
      /* 忽略 */
    }

    var reading = {
      ok: true,
      stage: "done",
      startedAt: startedAt,
      finishedAt: nowMs(),
      install: {
        path: "site-layer-found",
        installedAt: installedAt,
        layerSeenAt: layerSeenAt,
        waitedMs: layerSeenAt - startedAt,
        attempts: attempts,
        // ⚠️ **这两格说明「等站点那层」是怎么等的**：`DOMContentLoaded` 之后
        //    **第一次查**就在不在 ⇒ ⇒ 不在 ⇒ 才进轮询。
        parsedHow: parsedHow,
        parsedAt: parsedAt,
        parsedWaitedMs: parsedAt - startedAt,
        layerPresentAtParsed: layerAtParsed,
        onTopOfSrcLength: prevFetchSrcLength,
        onTopOfIsNative: prevFetchIsNative,
        catalogAlreadyRequestedBeforeInstall: alreadyRequested,
      },
      capture: {
        at: capturedAt,
        method: captured.method,
        urlPath: CATALOG_PATH,
        queryKeys: queryKeyNames(captured.url),
        headerNames: headerShape(captured.pairs).names,
        headerLengths: headerShape(captured.pairs).lengths,
        summary: catalogSummary,
      },
      restore: restoreReading,
      itemsCount: items.length,
      items: items,
      // ⚠️ **那一发的响应体原文**（**壳侧裁** ⇒ ⇒ 协议 §2.4 的白名单**逐字不变**）。
      //
      // 为什么这一格要过壳：白名单那 6 个键 ＋ 结构面（顶层键名／条数／类型／
      // `has_more`／站内码）**全都**从**整包**里裁 ⇒ ⇒ 整包得**到壳侧**才有
      // 那些字段（页内那份 `items` 是**页内裁**的，排序／条数上限／时间戳单位
      // 与壳侧那一套**不是同一口径** ⇒ ⇒ 不能拿它当最终结果）。
      //
      // ⚠️ **凭据仍然一步都不出页面**：签名头（`x-signature`／`x-timestamp`／
      // `yy`）与 cookie **只在这里取、只在这里用**（这里丢）⇒ ⇒ 响应体里**不含**
      // 任何凭据（那是站点自己发的那一发的响应）。
      capturedBody: {
        status: catalogStatus,
        text: catalogBodyText,
      },
      body: bodyReplay,
      controls: {
        positive: positive,
        unsigned: unsignedReplay,
      },
    };
    try {
      reading.windowOwnCountAtStart = ownNames().length;
      reading.windowOwnCountNow = ownNames().length;
    } catch (e) {
      /* 忽略 */
    }
    return reading;
  } catch (error) {
    var restoreOnError = restore("异常收工");
    return failure("exception", asText(error), { restore: restoreOnError });
  }

  // ---- 阳性对照的读数（等它跑完；**不留在页内**）--------------------------
  function waitForControlDone() {
    return new Promise(function (resolve) {
      var deadline = nowMs() + 15000;
      (function tick() {
        if (!positiveControlReading || positiveControlReading.done) {
          resolve(positiveControlReading);
          return;
        }
        if (nowMs() > deadline) {
          resolve(positiveControlReading);
          return;
        }
        yieldTask().then(tick);
      })();
    });
  }

  function queryKeyNames(url) {
    var out = [];
    try {
      var index = asText(url).indexOf("?");
      if (index < 0) return out;
      var parts = asText(url).slice(index + 1).split("&");
      for (var i = 0; i < parts.length; i++) {
        if (!parts[i]) continue;
        out.push(parts[i].split("=")[0]);
      }
    } catch (e) {
      /* 忽略 */
    }
    return out;
  }
})()
