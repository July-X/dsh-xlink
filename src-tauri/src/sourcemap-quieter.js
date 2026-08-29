/**
 * 初始化脚本，注入到 `harness` 工作台 webview 中（由 dsh 内核通过
 * `http://127.0.0.1:<port>` 提供的内核页面）。
 *
 * dsh 内核（`@deepseek-ai/dsh`）发布的 JS 包在每个脚本末尾都包含一行
 * `//# sourceMappingURL=<file>.js.map` —— 工作台前端
 * （`@deepseek-ai/dsh-web-frontend/dist/assets/...`）以及内核打包的
 * 插件（`plugins/@deepseek-ai/dsh-.../...`）都是如此。内核的 npm 包
 * 故意不携带 `.js.map` 文件（构建期裁剪），但末尾的引用仍保留在
 * 发布版 JS 中。浏览器随后会请求每个 map，内核的 HTTP 服务器返回 404，
 * DevTools 记录为
 *     Failed to load resource: the server responded with a status of 404 (Not Found)
 *     http://127.0.0.1:<port>/<file>.js.map
 * 默认安装下累计可达约 44 行 —— 看起来嘈杂，但本质上无伤大雅。
 *
 * 根据 AGENTS.md，外壳不修改也不重新发布内核。因此修复完全位于
 * 工作台 webview 内：拦截外向的 `.js.map` 请求，并以合成的、
 * 合法但为空的 source map（一个不含 sources/mappings 的最小 v3 JSON
 * 文档）回应，使 DevTools 看到 200，丢掉控制台错误，并在堆栈跟踪时
 * 回退到已经加载的压缩 JS。对用户而言，空 map 与 404 map 同样无效
 * （都无法给出源码），因此这次替换纯粹是降噪，不会损失调试能力。
 *
 * 之所以要修补三层拦截，是因为 Chromium 会根据调用位置把 source map
 * 拉取请求路由到任意一条：
 *
 *   1. `window.fetch` —— Vite / SPA 运行时使用的现代 API。
 *   2. `XMLHttpRequest.open/send` —— 旧路径以及部分 DevTools 探针。
 *   3. `console.error` / `console.warn` —— 对 `Failed to load resource`
 *      这一行的兜底；该行由浏览器网络层独立于页面请求发出，只有当
 *      请求本身成功时它才会消失。即便未来某条浏览器路径绕过了上述
 *      两层网络重写，过滤掉最后这行也能让 DevTools 保持干净。
 *
 * 控制台过滤器的匹配刻意收窄：只命中字面的 `.js.map` 后缀以及
 * DevTools 所用的 `404 (Not Found)` 字样，因此工作台真正的应用错误
 * 永远不会被吞掉。
 *
 * 顶层 frame 守卫与其他注入脚本保持一致
 * （`titlebar-pulse.js`、`pullstring-launcher.js`、`chat-fingerprint.js`），
 * 因为 Tauri 会在每个 frame 中运行初始化脚本；缺少这道守卫，工作台
 * 内的 iframe 就会再挂载一个拦截器，并在内部页面观察到包装后的 fetch
 * 时产生混淆。
 *
 * 本脚本为纯 JavaScript（无 TypeScript），且字符串安全 —— 它在编译期
 * 通过 `include_str!` 嵌入到 Rust 源码中，任何反引号 / 非 ASCII 字面量
 * 都会导致构建失败。
 */
(function () {
  if (window.top !== window.self) return;
  if (window.__DSH_SOURCEMAP_QUIETER__) return;
  window.__DSH_SOURCEMAP_QUIETER__ = true;

  // 合法的最小 v3 source map：无 sources、无 mappings。结构刚刚好，
  // 让 DevTools 接受该响应且不报警。
  var EMPTY_MAP = '{"version":3,"sources":[],"names":[],"mappings":""}';
  var SOURCE_MAP_RE = /\.js\.map($|\?)/;

  function looksLikeSourceMap(url) {
    return typeof url === "string" && SOURCE_MAP_RE.test(url);
  }

  function syntheticMapResponse() {
    return new Response(EMPTY_MAP, {
      status: 200,
      statusText: "OK",
      headers: { "Content-Type": "application/json" }
    });
  }

  // --- fetch ---------------------------------------------------------------
  if (typeof window.fetch === "function") {
    var origFetch = window.fetch.bind(window);
    window.fetch = function (input, init) {
      var url =
        typeof input === "string"
          ? input
          : input && typeof input.url === "string"
          ? input.url
          : "";
      if (looksLikeSourceMap(url)) {
        return Promise.resolve(syntheticMapResponse());
      }
      return origFetch(input, init);
    };
  }

  // --- XMLHttpRequest ------------------------------------------------------
  if (typeof XMLHttpRequest !== "undefined") {
    var origOpen = XMLHttpRequest.prototype.open;
    var origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function (method, url) {
      this.__dshIsMap = looksLikeSourceMap(url);
      return origOpen.apply(this, arguments);
    };
    XMLHttpRequest.prototype.send = function () {
      if (this.__dshIsMap) {
        var xhr = this;
        // 推迟到下一 tick，以便在 `send()` 之后使用同步 XHR 的调用方
        // 仍能观察到它们期望的生命周期。
        setTimeout(function () {
          Object.defineProperty(xhr, "readyState", { configurable: true, value: 4 });
          Object.defineProperty(xhr, "status", { configurable: true, value: 200 });
          Object.defineProperty(xhr, "statusText", { configurable: true, value: "OK" });
          Object.defineProperty(xhr, "response", { configurable: true, value: EMPTY_MAP });
          Object.defineProperty(xhr, "responseText", { configurable: true, value: EMPTY_MAP });
          xhr.dispatchEvent(new Event("load"));
          xhr.dispatchEvent(new Event("loadend"));
        }, 0);
        return;
      }
      return origSend.apply(this, arguments);
    };
  }

  // --- console.error / console.warn ---------------------------------------
  // 浏览器会输出 "Failed to load resource: ... 404 (Not Found)"，并在
  // 第二个参数中附带 URL。下面的正则同时匹配这条头部与任何独立的
  // ".js.map ... 404" 后续 —— 足够窄，永远不会触碰真正的应用错误。
  // 我们针对拼接后的参数进行匹配，因为 Chromium 把消息和 URL 拆到
  // 两个参数中。
  var NOISE_RE = /\.js\.map[\s\S]*404(?:\s*\(\s*Not Found\s*\))?|404(?:\s*\(\s*Not Found\s*\))[\s\S]*\.js\.map/;
  function isMapNoise(args) {
    var joined = "";
    for (var i = 0; i < args.length; i += 1) {
      var piece = args[i];
      if (typeof piece === "string") joined += piece + "\n";
      else if (piece && typeof piece.message === "string") joined += piece.message + "\n";
    }
    return NOISE_RE.test(joined);
  }
  if (typeof console !== "undefined") {
    if (typeof console.error === "function") {
      var origError = console.error.bind(console);
      console.error = function () {
        if (isMapNoise(arguments)) return;
        return origError.apply(console, arguments);
      };
    }
    if (typeof console.warn === "function") {
      var origWarn = console.warn.bind(console);
      console.warn = function () {
        if (isMapNoise(arguments)) return;
        return origWarn.apply(console, arguments);
      };
    }
  }
})();
