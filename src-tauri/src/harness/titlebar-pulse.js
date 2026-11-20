/**
 * 初始化脚本，给**两个工作台**画一条静态的顶部品牌线（两端渐变淡出）：
 * dsh web 工作台走 DeepSeek 主题蓝，MiniMax 工作台走自己的品牌粉
 * （2026-11-20 用户要求；此前只有 dsh 那条，且是 Gitea 绿）。
 *
 * 拉绳灯要跟这条光带在交界处看起来连续：脚本把同一份渐变暴露成
 * `window.__DSH_TITLEBAR_PULSE_COLOR__(cssX)`，拉绳按接触点横坐标取色。
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
  // 表面选择：MiniMax 工作台由建窗链先写 `window.__DSH_SHELL_SURFACE__ =
  // "mcode"`（它和 dsh 工作台同为 loopback origin，URL 上分不出来）。dsh 走
  // 与拉绳灯点亮后的灯球同源的 DeepSeek 主题蓝 #4D6BFE（2026-11-20 用户要求，
  // 此前是 Gitea 绿）；MiniMax 走外壳 --experimental 品牌粉 #ed94bd（与那盏灯
  // 的绳色同源）。**两端渐变淡出**——工作台那条一直是渐变，与官方那条（已移除）
  // 的实色直角是两种形状。
  var isMcode = window.__DSH_SHELL_SURFACE__ === "mcode";
  var PALETTE = isMcode
    ? {
        rgb: "237, 148, 189",
        hex: "#ed94bd",
      }
    : {
        rgb: "77, 107, 254",
        hex: "#4D6BFE",
      };
  // 光带渐变的色标（百分比 → 透明度，颜色都是当前表面的品牌色）。拉绳灯要从
  // 同一份渐变里取「接触点」上的颜色，所以渐变只在 STOPS 这一处维护。
  var STOPS = [
    { at: 0, alpha: 0 },
    { at: 15, alpha: 0.45 },
    { at: 50, alpha: 1 },
    { at: 85, alpha: 0.45 },
    { at: 100, alpha: 0 },
  ];
  // 光带整条的透明度：绳取同一份合成后的颜色，交界处才无缝。
  var BAND_OPACITY = 0.92;

  function stopColor(alpha) {
    if (alpha === 0) return "transparent";
    if (alpha === 1) return PALETTE.hex;
    return "rgba(" + PALETTE.rgb + ", " + alpha + ")";
  }

  function buildCss() {
    var gradient =
      "linear-gradient(90deg, " +
      STOPS.map(function (stop) {
        return stopColor(stop.alpha) + " " + stop.at + "%";
      }).join(", ") +
      ")";
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
      "  opacity: " + BAND_OPACITY + " !important;",
      "  filter: none !important;",
      "  box-shadow: none !important;",
      "  animation: none !important;",
      "}",
    ].join("\n");
  }

  /**
   * 光带上横坐标 `cssX` 处的实际颜色（含整条 0.92 的透明度）。拉绳灯把绳染成
   * 「接触点」上的渐变色，两块 chrome 在交界处看起来就是连续的；窗口宽度读不到
   * 时回 null，调用方退回自己的品牌色。
   */
  window.__DSH_TITLEBAR_PULSE_COLOR__ = function (cssX) {
    var width = window.innerWidth || 0;
    var x = Number(cssX);
    if (!(width > 0) || !isFinite(x)) {
      return null;
    }
    var percent = Math.min(100, Math.max(0, (x / width) * 100));
    var alpha = 0;
    for (var i = 1; i < STOPS.length; i += 1) {
      var hi = STOPS[i];
      if (percent <= hi.at) {
        var lo = STOPS[i - 1];
        var t = hi.at === lo.at ? 0 : (percent - lo.at) / (hi.at - lo.at);
        alpha = lo.alpha + (hi.alpha - lo.alpha) * t;
        break;
      }
    }
    var composed = Math.round(alpha * BAND_OPACITY * 1000) / 1000;
    return "rgba(" + PALETTE.rgb + ", " + composed + ")";
  };

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
