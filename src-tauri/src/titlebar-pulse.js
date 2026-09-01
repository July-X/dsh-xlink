/**
 * 初始化脚本，注入到 `harness` 工作台 webview 以及由 `open_official_chat`
 * 创建的远程内容 webview 中。它提供静态 chrome 品牌条带；本地的
 * `official-chat-strip` webview 故意不接收本脚本。
 *
 * 这里不使用 CSS 无限动画、transform、filter 或 box-shadow。工作台会持续
 * 接收流式 DOM 更新，任何常驻动画都会让 WKWebView 以帧频重复布局和合成，
 * 即使没有用户操作也会保持较高 CPU/GPU 占用。品牌色仍按 hostname 选择。
 *
 * 顶层 frame 守卫：Tauri 会在每个 frame 中运行初始化脚本，因此只允许顶层
 * 文档挂载条带，避免远程页面的 iframe 重复创建合成层。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var STYLE_ID = "dsh-titlebar-pulse";
  var OFFICIAL_HOSTNAMES = ["chat.deepseek.com", "www.qianwen.com", "agent.minimaxi.com"];
  var isOfficial = OFFICIAL_HOSTNAMES.indexOf(window.location.hostname) !== -1;
  var PALETTE = isOfficial
    ? {
        rgb: "77, 107, 254",
        hex: "#4D6BFE",
      }
    : {
        rgb: "96, 152, 38",
        hex: "#609926",
      };
  var HALO = "rgba(" + PALETTE.rgb + ", 0.45)";

  function buildCss() {
    var gradient =
      "linear-gradient(90deg, transparent 0%, " + HALO + " 15%, " + PALETTE.hex + " 50%, " + HALO + " 85%, transparent 100%)";
    return [
      /* 覆盖页面自带的顶部伪元素，只保留 Shell 的品牌条带。 */
      "body::before { content: none !important; display: none !important; }",
      "body::after {",
      "  content: '' !important;",
      "  position: fixed !important;",
      "  top: 0 !important;",
      "  left: 0 !important;",
      "  width: 100% !important;",
      "  height: 3px !important;",
      "  z-index: 1000 !important;",
      "  pointer-events: none !important;",
      "  background: " + gradient + " !important;",
      "  opacity: 0.92 !important;",
      "  filter: none !important;",
      "  box-shadow: none !important;",
      "  animation: none !important;",
      "}",
      /* 工作台的第二条装饰线由页面自身提供，静态模式下隐藏以免重复绘制。 */
      "body > [data-titlebar-pulse='2'] { display: none !important; }",
    ].join("\n");
  }

  function inject() {
    var existing = document.getElementById(STYLE_ID);
    if (existing) {
      existing.textContent = buildCss();
      return;
    }
    var style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = buildCss();
    document.head.appendChild(style);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", inject, { once: true });
  } else {
    inject();
  }
})();
