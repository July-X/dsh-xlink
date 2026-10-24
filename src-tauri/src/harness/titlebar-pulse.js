/**
 * 初始化脚本，给 **dsh web 工作台** 画一条静态的顶部品牌线（Gitea 绿，两端
 * 渐变淡出）。
 *
 * **官方对话的内容页不再注入本脚本**（2026-10-24 用户拍板「移除下方的彩色
 * 条」）：此前它也给官方内容页画了一条 3px 的品牌线，而品牌色此后改由**官方
 * 对话页签栏的文字**承担（Rust 侧 `official_chat::OFFICIAL_CHAT_BRAND_COLORS`
 * → `official_chat_tabs` 命令 → 页签栏）。官方页签栏自己（strip webview）也
 * 不注入它——它是壳自己的 chrome，条带画在别人的页面顶部本来就盖住人家的页头。
 *
 * 这里不使用 CSS 无限动画、transform、filter 或 box-shadow。工作台会持续
 * 接收流式 DOM 更新，任何常驻动画都会让 WKWebView 以帧频重复布局和合成，
 * 即使没有用户操作也会保持较高 CPU/GPU 占用。
 *
 * 顶层 frame 守卫：Tauri 会在每个 frame 中运行初始化脚本，因此只允许顶层
 * 文档挂载条带，避免远程页面的 iframe 重复创建合成层。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var STYLE_ID = "dsh-titlebar-pulse";
  // 工作台的品牌色：Gitea 绿。**两端渐变淡出**——工作台那条一直是渐变，
  // 与官方那条（已移除）的实色直角是两种形状。
  var PALETTE = {
    rgb: "96, 152, 38",
    hex: "#609926",
  };
  var HALO = "rgba(" + PALETTE.rgb + ", 0.45)";

  function buildCss() {
    var gradient =
      "linear-gradient(90deg, transparent 0%, " + HALO + " 15%, " + PALETTE.hex + " 50%, " + HALO + " 85%, transparent 100%)";
    return [
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
