/**
 * 初始化脚本，注入到 `harness` 工作台 webview 以及由 `open_official_chat`
 * 创建的远程内容 webview 中。它提供静态 chrome 品牌条带；本地的
 * `official-chat-strip` webview 故意不接收本脚本。
 *
 * 这里不使用 CSS 无限动画、transform、filter 或 box-shadow。工作台会持续
 * 接收流式 DOM 更新，任何常驻动画都会让 WKWebView 以帧频重复布局和合成，
 * 即使没有用户操作也会保持较高 CPU/GPU 占用。品牌色仍按 hostname 选择。
 *
 * **官网网页版那条线两端走直角**（2026-10-23 用户拍板）：实色铺满，不做两端
 * 渐变淡出——淡出读作「圆角过渡」。工作台那条仍是原来的渐变（不同表面）。
 *
 * **三站各用各的品牌色**（2026-10-24 用户拍板）：DeepSeek 蓝、MiniMax 官方粉、
 * ChatGPT 黑。此前是「官方一律蓝」，而 MiniMax 那个页签压根没进官方分支
 * （入口域会 302 到落地域，见下面那张站表）⇒ 它的顶条走成了工作台的绿——用户
 * 实机看到的就是那条绿线。hostname 与颜色的对应见下面那张站表。
 *
 * 顶层 frame 守卫：Tauri 会在每个 frame 中运行初始化脚本，因此只允许顶层
 * 文档挂载条带，避免远程页面的 iframe 重复创建合成层。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var STYLE_ID = "dsh-titlebar-pulse";
  // 官网三站：各自的内容页 hostname 与品牌色。
  //
  // **每站一个色，不是一个「官方色」**（2026-10-24 用户拍板）：三个页签是三个
  // 不同的站，顶条要跟各自的品牌一致。
  //   · DeepSeek 蓝 —— DeepSeek 网页版品牌蓝（此前那个「官方蓝」）。
  //   · MiniMax 粉 —— **MiniMax 官方 logo 渐变的品牌色 1**（官方图标 SVG 的
  //     linearGradient stop-color，另一端是橙红）。官方 logo 素材里 MiniMax
  //     标志的两段 stop-color 就是这两个值，取靠粉的那一段。
  //   · ChatGPT 黑 —— OpenAI 品牌黑。
  //
  // hosts 里放**该站可能出现的内容页 hostname**：MiniMax 的入口
  // `agent.minimaxi.com` 会 302 到 `agent.minimax.cn`（2026-10-22 实测），内容
  // webview 最终的 `location.hostname` 是**落地域**。两个域名都得写在这一站里
  // ——只写入口域，落地域就落回工作台绿（2026-10-24 用户实机看到的那条绿线
  // 就是它）。
  //
  // 与 Rust 的 `official_chat::OFFICIAL_CHAT_TABS` /
  // `OFFICIAL_CHAT_OPTIONAL_TABS` 是同一件事的两份名单，判据第 20 项机械比对：
  // 页签表里的 host 必须是某一站的成员（漏了 ⇒ 顶条走工作台绿），每站也必须有
  // 页签表里的那个 host（否则是死站）。
  var SITES = [
    {
      hosts: ["chat.deepseek.com"],
      rgb: "77, 107, 254",
      hex: "#4D6BFE",
    },
    {
      hosts: ["agent.minimaxi.com", "agent.minimax.cn"],
      rgb: "226, 22, 126",
      hex: "#E2167E",
    },
    {
      hosts: ["chatgpt.com"],
      rgb: "0, 0, 0",
      hex: "#000000",
    },
  ];
  // 工作台（内核 Web UI）：Gitea 绿，两端渐变淡出。
  var WORKBENCH = {
    rgb: "96, 152, 38",
    hex: "#609926",
  };

  var hostname = window.location.hostname;
  var site = null;
  for (var i = 0; i < SITES.length; i++) {
    if (SITES[i].hosts.indexOf(hostname) !== -1) {
      site = SITES[i];
      break;
    }
  }
  var isOfficial = site !== null;
  var PALETTE = isOfficial ? site : WORKBENCH;
  var HALO = "rgba(" + PALETTE.rgb + ", 0.45)";

  // 官方那条线用**实色**（两端硬边 = 直角）；工作台那条保持原来的渐变。
  // 形状由 `scripts/titlebar-pulse.test.mjs` 钉住（它跑的就是官方那条分支）。

  function buildCss() {
    var gradient =
      "linear-gradient(90deg, transparent 0%, " + HALO + " 15%, " + PALETTE.hex + " 50%, " + HALO + " 85%, transparent 100%)";
    // 官方：实色 ⇒ 两端是硬边（直角）。工作台：渐变 ⇒ 两端淡出。
    var fill = isOfficial ? PALETTE.hex : gradient;
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
      "  background: " + fill + " !important;",
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
