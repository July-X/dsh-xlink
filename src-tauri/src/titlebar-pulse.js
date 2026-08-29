/**
 * 初始化脚本，注入到 `harness` 工作台 webview 以及由 `open_official_chat`
 * 创建的每个远程内容 webview 中。它负责在外壳打开的每个表面呈现 chrome
 * 行扫光，使 pulse 读作外壳 chrome，而不是工作台 / 供应商内容。本地的
 * `official-chat-strip` webview 故意不接收本脚本。
 *
 * 两个表面，两套配色：
 *
 * - dsh web 工作台（`http://127.0.0.1:<port>`）：Gitea 绿 (#609926,
 *   rgb 96,152,38)，让工作台读作独立的品牌表面。工作台自带
 *   `body > [data-titlebar-pulse='2']` DOM 节点，因此第二次扫光直接
 *   对应那个已存在的元素。
 *
 * - DeepSeek 官方对话（`https://chat.deepseek.com`）：DeepSeek 官方蓝
 *   (#4D6BFE, rgb 77,107,254)，让对话窗口读作官方品牌表面。chat
 *   页面并未自带第二个扫光节点，因此本脚本在应用样式前先追加一个
 *   —— 否则半周期偏移将无处匹配。
 *
 * 两个表面都使用相同的 6.01s 周期，使 chrome 行扫光在所有表面之间
 * 保持同一节奏；颜色是唯一区分点，并在注入时通过 `location.hostname`
 * 选定。
 *
 * 关键帧端点由 `getBoundingClientRect()` 计算，并在 `resize` 时重新
 * 计算：在 WKWebView 中，`documentElement.clientWidth` 返回的是物理
 * 像素（例如 2x retina 屏上为 2560），而 CSS 布局与 `transform` 都在
 * CSS 像素（例如 1280）中工作。因此若端点直接取自 `clientWidth`，
 * 会得到 2 倍于实际可见宽度的 transform 范围 —— 条带会在动画走完一半
 * 时抵达合成层的右边缘并被裁切，随后周期重置并瞬移回左边缘。视觉表现
 * 就是"条带在屏幕中段消失，再在左侧重新出现"。`getBoundingClientRect().width`
 * 返回的是 CSS 像素，与 `transform` 和 `width` 使用的坐标系一致。
 * `resize` 监听器会在窗口尺寸或显示缩放改变时重新计算端点，使条带
 * 始终恰好行进一个视口宽度加一个条带宽度。
 *
 * 本脚本与页面自身样式表的加载顺序无法保证，因此每条规则都带有
 * `!important` —— 同时脚本会在 `DOMContentLoaded`（如果文档已经解析
 * 完成则立即执行）追加样式节点，使注入的样式表位于 `<head>` 末尾，
 * 因此即便没有 `!important` 也能赢得特异性比较。双重保险。
 *
 * 顶层 frame 守卫：Tauri 会在每个 frame 中运行初始化脚本，因此若没有
 * 这道守卫，chat.deepseek.com 内的 iframe（登录组件、埋点等）会自己
 * 挂载一份条带并重复绘制扫光。工作台页面是单一文档，因此这里对它
 * 是无害的空操作。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var STYLE_ID = "dsh-titlebar-pulse";

  // 通过 hostname 查询配色。official-chat 的内容 webview（DeepSeek、
  // 千问、MiniMax）都在同一个专用窗口内运行，共用官方品牌蓝；
  // 除此之外（包括位于 127.0.0.1 的 dsh web 工作台）一律使用 Gitea 绿。
  var OFFICIAL_HOSTNAMES = ["chat.deepseek.com", "www.qianwen.com", "agent.minimaxi.com"];
  var isOfficial = OFFICIAL_HOSTNAMES.indexOf(window.location.hostname) !== -1;
  var PALETTE = isOfficial
    ? {
        rgb: "77, 107, 254",
        hex: "#4D6BFE",
        hover: "#7C92FF",
      }
    : {
        rgb: "96, 152, 38",
        hex: "#609926",
        hover: "#7dbd45",
      };
  var HALO = "rgba(" + PALETTE.rgb + ", 0.45)";

  function cssViewportWidth() {
    return document.documentElement.getBoundingClientRect().width
      || document.documentElement.clientWidth
      || window.innerWidth;
  }

  function ensureSecondBar() {
    // dsh web 工作台自带 `body > [data-titlebar-pulse='2']` 节点；
    // chat.deepseek.com（以及任何其他远程页面）则不会。这里补上一个，
    // 让半周期的第二次扫光有节点可挂载，按结构与工作台保持一致，
    // 而非依赖页面自行提供 DOM。
    var existing = document.querySelector("body > [data-titlebar-pulse='2']");
    if (existing) {
      return existing;
    }
    var node = document.createElement("div");
    node.setAttribute("data-titlebar-pulse", "2");
    node.setAttribute("aria-hidden", "true");
    document.body.appendChild(node);
    return node;
  }

  function buildCss() {
    var viewportPx = cssViewportWidth();
    // 宽度：布局视口的 18.24%，以 CSS 像素表示。
    var bandPx = viewportPx * 0.1824;
    // 关键帧端点采用 CSS 像素而非 vw：
    //   0%   → 条带前沿位于左边缘外一个条带宽度处
    //   100% → 条带后沿位于右边缘外一个条带宽度处
    var startPx = -bandPx;
    var endPx = viewportPx + bandPx;
    var gradient =
      "linear-gradient(90deg, transparent 0%, " + HALO + " 15%, " + PALETTE.hex + " 50%, " + HALO + " 85%, transparent 100%)";
    return [
      /* 若页面自带静态品牌条带，将其隐藏。 */
      "body::before { content: none !important; display: none !important; }",
      /* 第一次扫光 —— 在 chrome 行上从左向右行进。 */
      "body::after {",
      "  content: '' !important;",
      "  position: fixed !important;",
      "  top: 0 !important;",
      "  left: 0 !important;",
      "  height: 3px !important;",
      "  width: " + bandPx + "px !important;",
      "  z-index: 1000 !important;",
      "  pointer-events: none !important;",
      "  background: " + gradient + " !important;",
      "  filter: blur(0.4px) !important;",
      "  border-radius: 999px !important;",
      "  animation: dsh-titlebar-pulse-sweep 6.01s linear infinite !important;",
      "  box-shadow: 0 0 8px " + HALO + " !important;",
      "}",
      /* 第二次扫光 —— 同宽，半周期偏移，使视觉读作连续扫描，而不是
         单条短横划过之后留白。在 dsh web 工作台上对应页面自带的节点；
         在 chat.deepseek.com 上对应上文 ensureSecondBar 追加的节点。 */
      "body > [data-titlebar-pulse='2'] {",
      "  position: fixed !important;",
      "  top: 0 !important;",
      "  left: 0 !important;",
      "  height: 3px !important;",
      "  width: " + bandPx + "px !important;",
      "  z-index: 1000 !important;",
      "  pointer-events: none !important;",
      "  background: " + gradient + " !important;",
      "  filter: blur(0.4px) !important;",
      "  border-radius: 999px !important;",
      "  animation: dsh-titlebar-pulse-sweep 6.01s linear infinite !important;",
      "  animation-delay: 3.005s !important;",
      "  box-shadow: 0 0 8px " + HALO + " !important;",
      "}",
      "@keyframes dsh-titlebar-pulse-sweep {",
      "  0% { transform: translateX(" + startPx + "px); opacity: 1; }",
      "  100% { transform: translateX(" + endPx + "px); opacity: 1; }",
      "}",
    ].join("\n");
  }

  function inject() {
    ensureSecondBar();
    var existing = document.getElementById(STYLE_ID);
    if (existing) {
      // resize 时重新计算：替换样式表内容，使关键帧跟随当前的 CSS 视口宽度。
      existing.textContent = buildCss();
      return;
    }
    var style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = buildCss();
    document.head.appendChild(style);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", inject);
  } else {
    inject();
  }

  // 在视口尺寸或缩放变化时重新计算关键帧。WKWebView 会在缩放、窗口
  // 尺寸变化以及显示缩放变化时触发 `resize`，因此单一监听器即可覆盖
  // CSS 像素宽度变化的所有情形。
  var resizeTimer;
  window.addEventListener("resize", function () {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(inject, 100);
  });
})();