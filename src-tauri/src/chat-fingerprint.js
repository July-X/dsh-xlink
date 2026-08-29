/**
 * 初始化脚本，仅注入到 `official-chat` webview 中（专用的 WebviewWindow，
 * 加载 `https://chat.deepseek.com`）。
 *
 * 策略 —— 做一个诚实的桌面浏览器，而不是伪装成别的。WebView2 背后的引擎
 * 本就是真正的桌面版 Edge/Chromium，它在网站可以比对的每一层都保持自洽：
 * User-Agent 请求头（不再覆盖）、由真实品牌派生的 `Sec-CH-UA` client 提示、
 * 原生的 `navigator.userAgentData`、插件、权限、canvas/WebGL 读数。本脚本
 * 早期版本曾用纯 JS shim 把这些面伪装成 "Google Chrome"；而每个 shim
 * 本身都可被探测到（规范本应是 `NavigatorUAData` 实例的地方出现 `{}`，
 * 规范本应是原生方法的地方出现脚本函数），而且 JS 层声称的 Chrome 与
 * HTTP 层 Edge 品牌相矛盾 —— 这正是环境检测会查找的那种不一致。所有这些
 * 都已移除。
 *
 * 现在剩下的只是抹除嵌入式 webview 的痕迹：
 *
 * 1. 顶层 frame 守卫 —— 初始化脚本会在每个 frame 中运行；只加固顶层文档。
 * 2. `navigator.webdriver` —— 固定为 `false`（普通非自动化桌面浏览器的取值）。
 *    `OFFICIAL_CHAT_BROWSER_ARGS` 中的启动器参数已在引擎层禁用自动化开关；
 *    原型上的固定是对那些拿不到该标志的构建做的一道兜底。注意是 `false`，
 *    而不是 `undefined`：属性缺失本身就是机器人信号。
 * 3. Tauri 全局变量 —— 直接删除（`__TAURI__`、`__TAURI_INTERNALS__`、
 *    `__TAURI_METADATA__`、`__TAURI_IPC__`）。普通浏览器里这些位置什么都没有，
 *    所以页面探针也必须看到什么都没有 —— 哪怕暴露一个 Proxy 让 typeof
 *    检查通过，依然是在宣告自己是嵌入式 webview。拉绳灯位于 official-chat
 *    strip webview（标签 `official-chat-strip`）上，这是一个本地 webview，
 *    并不运行本指纹脚本，因此它保留着活跃的桥接通道。（我们曾在独立的
 *    `official-chat-launcher` webview 上短暂试验过，但因 WebView2 子 HWND
 *    透明在当前 Tauri 2.11 / wry 0.55.1 上不生效而回滚 —— 无论控制器的
 *    DefaultBackgroundColor 是否透明，启动器都会绘制一个不透明的深色方块；
 *    因此灯被移回 strip，strip 也从 38px 增高到 66px 以容纳 66px 的 SVG。）
 *
 * 本脚本为纯 JavaScript（无 TypeScript），且字符串安全 —— 它在编译期通过
 * `include_str!` 嵌入到 Rust 源码中，任何嵌套反引号 / 未转义引号 /
 * 非 ASCII 字面量都会导致构建失败。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  // --- navigator.webdriver ------------------------------------------------
  // Frozen on the prototype so every fresh document inherits the spoofed
  // value before any script can re-define it on the instance.
  try {
    Object.defineProperty(Navigator.prototype, "webdriver", {
      get: function () { return false; },
      configurable: true,
      enumerable: true,
    });
  } catch (e) {
    // 较老引擎可能禁止覆盖原型 —— 此时回退到实例级 defineProperty，
    // 对当前文档的探针仍然能通过。
    try {
      Object.defineProperty(navigator, "webdriver", {
        get: function () { return false; },
        configurable: true,
        enumerable: true,
      });
    } catch (_e2) { /* 这里已无计可施 */ }
  }

  // --- Tauri 全局变量 ------------------------------------------------------
  // 直接删除 IPC 接口而不是遮盖它：普通浏览器根本没有 `__TAURI_*` 属性，
  // 所以页面探针必须观察到完全为空。属性可配置时（常见情况）`delete` 生效；
  // 若某构建使其成为永久属性，则回退到 undefined getter，使读取到的值
  // 至少呈现为缺失。
  var tauriGlobals = ["__TAURI__", "__TAURI_INTERNALS__", "__TAURI_METADATA__", "__TAURI_IPC__"];
  for (var i = 0; i < tauriGlobals.length; i++) {
    var name = tauriGlobals[i];
    try {
      delete window[name];
    } catch (e) { /* 不可配置 —— 下方遮盖 */ }
    if (window[name] !== undefined) {
      try {
        Object.defineProperty(window, name, {
          get: function () { return undefined; },
          configurable: true,
          enumerable: false,
        });
      } catch (e2) { /* 宁可保留原状也不要让文档崩溃 */ }
    }
  }
})();
