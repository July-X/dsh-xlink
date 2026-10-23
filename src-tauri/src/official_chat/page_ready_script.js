// 页内**就绪**判定脚本（路线 B 的一步）——同源、页面加载完、**凭据已经在**。
//
// 为什么要把「凭据已经在」也算进就绪条件：页内 fetch 用的就是那份凭据 ⇒
// 凭据不在时发出去**必然**是未登录 ⇒ ⇒ 与其发一次注定失败的请求，不如
// **立刻**报「没有登录态」（可重试），别让用户等一个注定失败的响应。
//
// ⚠️ **只报「有没有」**，绝不报值。

JSON.stringify((function () {
  var SPEC = __SPEC__;
  var ready = String(document.readyState);
  var origin = String(location.origin);
  var credential = null;
  try {
    credential = SPEC.storage === "sessionStorage"
      ? window.sessionStorage.getItem(SPEC.key)
      : window.localStorage.getItem(SPEC.key);
  } catch (e) { credential = null; }
  return {
    origin: origin,
    href: String(location.href),
    readyState: ready,
    hasCredential: !!(credential && String(credential).length > 0),
    originMatches: !SPEC.origin || origin.indexOf(SPEC.origin) >= 0
  };
})())
