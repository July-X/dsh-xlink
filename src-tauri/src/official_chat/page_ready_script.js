// 页内**就绪**判定脚本（路线 B 的一步）——同源、页面加载完、**凭据已经在**。
//
// 为什么要把「凭据已经在」也算进就绪条件：页内 fetch 用的就是那份凭据 ⇒
// 凭据不在时发出去**必然**是未登录 ⇒ ⇒ 与其发一次注定失败的请求，不如
// **立刻**报「没有登录态」（可重试），别让用户等一个注定失败的响应。
//
// ⚠️ **只报「有没有」**，绝不报值。
//
// ⚠️ **两种凭据来源 ⇒ 两种就绪判据**（2026-10-23 实测出来的）：
//
// · **页内存储取**（DeepSeek）：凭据就在 `localStorage`／`sessionStorage` 里
//   ⇒ ⇒ **同步**读得到 ⇒ ⇒ 这一支**同步给回值**（宿主走 `read_json` —— 原生
//   求值**不等** Promise）。
// · **同源端点取**（ChatGPT）：凭据**只在** `GET /api/auth/session` 的响应里
//   （响应**顶层**的 `accessToken`）⇒ ⇒ 必须 **await** 那一发 ⇒ ⇒ 这一支
//   **给回一个 Promise**（宿主走 `round_trip` —— **能 await**）。
//
// ⚠️ **分派键是 `SPEC.source`**：`"endpoint"` 走异步那一支；**没有这个键**
// （旧规格）或 `"storage"` 走同步那一支 ⇒ ⇒ 旧规格**一字不改**也能用。
(function () {
  var SPEC = __SPEC__;

  // ① **异步形态**：凭据在同源端点的响应里 ⇒ ⇒ 必须 await 那一发。
  if (SPEC.source === "endpoint") {
    return (async function () {
      var ready = String(document.readyState);
      var origin = String(location.origin);
      var base = {
        origin: origin,
        href: String(location.href),
        readyState: ready,
        credentialMissing: false,
        originMatches: !SPEC.origin || origin.indexOf(SPEC.origin) >= 0
      };
      // 页面还没就绪 ⇒ ⇒ **不发那一发**（免得白发一次）。
      if (ready !== "complete" || !base.originMatches) {
        base.hasCredential = false;
        base.credentialDetail = "页面还没就绪";
        return base;
      }
      // 取 token：**同源**端点 ⇒ 浏览器自动带 cookie ⇒ ⇒ 登录态就在那一发里。
      //
      // ⚠️ **只判「有没有」**：token 出了页面就是泄漏 ⇒ ⇒ 这里**只留一个布尔**
      // （连长度都不回）。
      var token = null;
      var detail = "";
      try {
        var res = await fetch(SPEC.endpointUrl, { credentials: "include" });
        if (!res.ok) {
          detail = "取 token 的端点返回 " + res.status;
          base.credentialMissing = res.status === 401;
        } else {
          var data = await res.json();
          token = (data && typeof data[SPEC.jsonKey] === "string") ? data[SPEC.jsonKey] : null;
          if (!token) detail = "端点响应里没有 " + SPEC.jsonKey;
          base.credentialMissing = !token;
        }
      } catch (e) {
        detail = String(e);
      }
      base.hasCredential = !!token;
      base.credentialDetail = detail;
      return base;
    })();
  }

  // ② **同步形态**：凭据在页内存储里（DeepSeek）—— **逐字保持现状**。
  return JSON.stringify((function () {
    var SPEC = __SPEC__;
    var ready = String(document.readyState);
    var origin = String(location.origin);
    var credential = null;
    var storageError = false;
    try {
      credential = SPEC.storage === "sessionStorage"
        ? window.sessionStorage.getItem(SPEC.key)
        : window.localStorage.getItem(SPEC.key);
    } catch (e) { credential = null; storageError = true; }
    return {
      origin: origin,
      href: String(location.href),
      readyState: ready,
      hasCredential: !!(credential && String(credential).length > 0),
      credentialMissing: !storageError && !(credential && String(credential).length > 0),
      originMatches: !SPEC.origin || origin.indexOf(SPEC.origin) >= 0
    };
  })());
})()
