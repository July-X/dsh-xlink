// 页内取数脚本（**路线 B**：壳侧在页内发那一发）——**凭据全程不出页面**。
//
// 这段脚本在官网页面里跑，做三件事：
//
//   ① 从页内存储取凭据（键名与取法由站点规格表给）；
//   ② 按站点契约把凭据拼成认证头；
//   ③ 同源 `fetch` 那一发，**只把响应体与状态码带回宿主**。
//
// ⚠️ **凭据的值不出页面**：它在这里取、在这里用、在这里丢掉 ⇒ 宿主永远拿不到
// 用户的 token。
//
// ⚠️ **为什么是独立文件**：这段 JS 有大量 `{`／`}`，塞进 Rust 的 `format!`
// 模板要双写转义，漏一处就是**运行期**语法错误（编译期看不出来）⇒ ⇒ 放独立
// 文件，`include_str!` 引进（相对路径对 example 那些 `#[path]` 引进同一份源
// 文件的探针也成立）。
//
// ⚠️ **`__SPEC__` 占位**：宿主把站点契约（URL、存储、键名、取法、头名、前缀）
// 注入这里。**不拼进字符串字面量**，避免引号/转义问题。

// ⚠️ **回执的形状：直接回「对象」，不要 `JSON.stringify`**。
//
// 契约：`round_trip` 把表达式 resolve 出来的值原样放进槽位 ⇒ 宿主拿到的
// `value` 就是这个对象本身 ⇒ 宿主直接 `value.get("ok")` 读字段。
//
// ⚠️ **踩过的坑**（2026-10-22 22:0x 实测）：脚本曾经回**一段 JSON 文本**
// （字符串）⇒ 宿主那边 `value.get("ok")` 在字符串上永远是 `None` ⇒ 报成
// 「未知阶段／没有给出原因」——看着像站点没数据，其实是自己的契约读错了。
// ⇒ ⇒ **回对象**，别回文本。

// ⚠️ **必须是 async IIFE**：`await` 放在**顶层**是语法错误 ⇒ `eval` 直接报
// SyntaxError ⇒ 槽位永远不会写进去 ⇒ 宿主只能等到超时（2026-10-22 21:5x 实测）。
(async function () {
  var SPEC = __SPEC__;

  function fail(stage, message) {
    return { ok: false, stage: stage, error: String(message) };
  }

  // ① 取凭据。
  var raw = null;
  try {
    if (SPEC.storage === "localStorage") {
      raw = window.localStorage.getItem(SPEC.key);
    } else if (SPEC.storage === "sessionStorage") {
      raw = window.sessionStorage.getItem(SPEC.key);
    } else {
      return fail("read-credential", "未知的凭据存储：" + SPEC.storage);
    }
  } catch (e) {
    return fail("read-credential", e);
  }
  if (raw === null || raw === undefined || raw === "") {
    return fail("read-credential", "页内没有这个键（没有登录态？）");
  }

  // ② 取 token（**形状按站点规格表**：json-value ＝ `JSON.parse(x).value`）。
  var token = raw;
  if (SPEC.extract === "json-value") {
    try {
      var parsed = JSON.parse(raw);
      token = (parsed && typeof parsed.value === "string") ? parsed.value : null;
    } catch (e) {
      token = null;
    }
    if (!token) return fail("extract-credential", "按规格表取不到 token");
  } else if (SPEC.extract !== "raw") {
    return fail("extract-credential", "未知的取法：" + SPEC.extract);
  }
  if (!token) return fail("extract-credential", "取不到 token");

  // ③ 发那一发（**同源相对路径** ⇒ 浏览器自动带 cookie）。
  var headers = {};
  headers[SPEC.header] = SPEC.prefix + token;
  var res = null;
  var text = null;
  try {
    res = await fetch(SPEC.url, { credentials: "include", headers: headers });
    text = await res.text();
  } catch (e) {
    return fail("fetch", e);
  }

  // ④ **只回响应体与状态码**——凭据与 token 都不在这里。
  return { ok: true, status: res.status, body: text };
})()
