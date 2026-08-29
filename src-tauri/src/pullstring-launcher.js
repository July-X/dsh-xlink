/**
 * 初始化脚本，注入到外壳拥有的两个本地 webview 中：
 *
 * - `open_harness`（dsh web 工作台 webview，由内核通过
 *   `http://127.0.0.1:<port>` 提供）。
 * - `open_official_chat` 的 strip webview（标签 `official-chat-strip`，
 *   渲染标签栏的本地 SPA 路由 `index.html?chatstrip=1`）。灯与 strip
 *   同一 HWND 共存，是一个紧凑的 24x38 台灯，正好嵌入 38px 的天然标签栏高度。
 *
 * 两个 webview 都保留 `window.__TAURI__` 不动 —— 它们的 IPC 接口都不会被
 * 阉割 —— 因此灯可以直接与全局对象通信，注入脚本只需处理 chrome 几何，
 * 无需处理指纹防御。
 *
 * 拉动（点击）绳子会点亮灯泡，并通过 `focus_main_shell` 命令把外壳的主
 * 管理窗口带到前台。控件通过 `window.__TAURI__.core.invoke`（tauri.conf.json
 * 设置了 `withGlobalTauri: true`）与外壳通信，因此不需要页面侧端点。
 *
 * 为什么由外壳注入而非随内核一起发布：内核是已发布的 npm 制品
 * （`@deepseek-ai/dsh@<ver>`），而回到外壳管理面板的快捷方式是外壳的 chrome，
 * 而不是页面内容。official-chat 的内容 webview（DeepSeek / 千问 / MiniMax）
 * 属于第三方源，我们无法向其中注入脚本；灯本身属于窗口级 chrome，
 * 位于外壳拥有的 strip webview 上。
 *
 * 两个表面，两套锚点与配色：
 *
 * - dsh web 工作台：Gitea 绿色绳 (#609926)，位于 left:212px，恰好落在
 *   侧栏折叠按钮旁（具体几何在偏移附近的注释中详述）。
 *
 * - official-chat strip：DeepSeek 蓝色绳 (#4D6BFE)，位于 right:12px，
 *   镜像到右边 —— strip 右侧没有其他内容会与之冲突，镜像到右可以让灯
 *   在 strip 标签按钮数量变化时始终保持可见。
 */
(function () {
  // Tauri 会在每个 frame 中运行初始化脚本；该控件只属于顶层页面 —— 没有这
  // 道守卫，工作台里的 iframe 会自己挂载一个灯，并对每个嵌套文档重复绘制
  // 图标。strip 页面是单一文档，因此这里的守卫对它是无害的空操作。
  if (window.top !== window.self) {
    return;
  }

  // 表面选择：official-chat strip webview（在 `?chatstrip=1` 加载）使用
  // DeepSeek 蓝色的右侧 chrome；dsh web 工作台使用 Gitea 绿色的左侧锚点，
  // 紧贴侧栏折叠按钮。保留对旧的 `chatlauncher=1` 的检查，对陈旧 URL
  // 仍是无害的，而当前 official-chat 的 chrome 由 strip webview 持有。
  var search = window.location.search;
  var isOfficial =
    search.indexOf("chatstrip=1") !== -1 ||
    search.indexOf("chatlauncher=1") !== -1;
  var PALETTE = isOfficial
    ? {
        cord: "#4D6BFE",
      }
    : {
        cord: "#609926",
      };
  // 官方对话窗口的拉绳灯挂在右侧边缘（right:12px，与原先的左侧 left:12px 镜像
  // 对称）；dsh web 工作台仍贴左侧 left:212px（在品牌 logo 右侧、侧栏折叠键旁）。
  // SVG（绳/底座/灯泡/灯丝）全部以 x=12 为中心左右对称，故仅切换锚定边即可，
  // 无需水平翻转图形。
  var SIDE = isOfficial ? "right" : "left";
  var EDGE_PX = isOfficial ? "12px" : "212px";
  // 两种 variant：官方对话 strip 用 24x38 的紧凑小台灯（不撑高 strip），
  // dsh 工作台用 24x66 的传统拉绳灯（cord 38px + 螺丝底座 + 大灯泡 + 灯丝），
  // 跟它们各自窗口的视觉语义匹配。
  var LAMP_VARIANT = isOfficial ? "desk" : "pull-string";
  // 两种 variant 的 pull 反馈幅度不一样：紧凑台灯只有 3px 才有
  // 比例感；老拉绳灯用 6px 看起来更"按下去"。
  var PULL_TRANSLATE_PX = LAMP_VARIANT === "desk" ? "3px" : "6px";
  // 灯所在的两个表面都有足够的高度容纳当前 variant 的 SVG：dsh web 工作台
  // 是占满窗口的 webview（顶部不会被裁切），official-chat strip 是天然的
  // 38px 标签栏高度（紧凑的 24x38 台灯正好嵌入）。两种情况下 `top: 0`
  // 都把绳锚定在视口顶部 —— 不论是较长的拉绳（pull-string variant）还是
  // 较短的拉链（desk variant） —— 都读作灯悬挂在 chrome 上。
  var TOP_PX = "0px";

  var ROOT_ID = "dsh-shell-launcher";
  var STYLE_ID = "dsh-shell-launcher-style";
  // 两套 SVG —— 由 variant 决定挂载哪一套。
  // .dsh-launcher-shade / .dsh-launcher-stem 只在 desk 出现，
  // .dsh-launcher-filament 只在 pull-string 出现；下面的 buildCss
  // 只生成与当前激活 SVG 实际匹配的规则，因此我们永远不会为不在
  // DOM 中的元素发布 CSS 选择器。
  var SVG_DESK = [
    '<svg viewBox="0 0 24 38" width="24" height="38" aria-hidden="true">',
    /* 拉链（短，从 strip 顶部向下到灯罩）。 */
    '<line class="dsh-launcher-cord" x1="12" y1="2" x2="12" y2="5"/>',
    /* 灯罩（梯形：上窄下宽）。 */
    '<polygon class="dsh-launcher-shade" points="9,5 15,5 17,13 7,13"/>',
    /* 从灯罩下方露出的灯泡。 */
    '<circle class="dsh-launcher-bulb" cx="12" cy="14" r="2"/>',
    /* 灯杆。 */
    '<line class="dsh-launcher-stem" x1="12" y1="16" x2="12" y2="30"/>',
    /* 圆角底座。 */
    '<rect class="dsh-launcher-base" x="3" y="30" width="18" height="6" rx="1.5"/>',
    "</svg>",
  ].join("");
  // 工作台 variant 使用 66px 拉绳灯：更长的绳、螺丝底座、灯泡与灯丝
  // 适配更高的工作台 chrome。
  var SVG_PULL = [
    '<svg viewBox="0 0 24 66" width="24" height="66" aria-hidden="true">',
    /* 从视口顶部边缘悬挂下来的绳子。 */
    '<line class="dsh-launcher-cord" x1="12" y1="0" x2="12" y2="38"/>',
    /* 绳子与灯泡相接处的螺丝底座。 */
    '<rect class="dsh-launcher-base" x="8.5" y="37" width="7" height="7" rx="1.5"/>',
    /* 灯泡玻璃罩。 */
    '<circle class="dsh-launcher-bulb" cx="12" cy="54" r="9.5"/>',
    /* 灯丝，点亮时可见。 */
    '<path class="dsh-launcher-filament" d="M9 52 q1.5 3 3 0 q1.5 -3 3 0" fill="none"/>',
    "</svg>",
  ].join("");
  var SVG = LAMP_VARIANT === "desk" ? SVG_DESK : SVG_PULL;

  function buildCss() {
    var rules = [
      "#" + ROOT_ID + " {",
      "  position: fixed;",
      "  top: " + TOP_PX + ";",
      /* 锚定在 chrome 的转角。dsh web 工作台把灯挂在品牌 logo 右侧
         (left:212px)；official-chat strip 把它镜像到右边 (right:12px)。
         普通的窗口尺寸变化不会影响这两个锚点 —— 工作台侧栏是定宽的，
         strip 是天然的 38px 标签栏高度；每个 variant 的 SVG 都按尺寸
         适配（strip 上的 24x38 台灯，工作台上的 24x66 拉绳灯）。
         两个表面都把 `top` 设为 `0`，让绳锚定在视口顶部。 */
      "  " + SIDE + ": " + EDGE_PX + ";",
      "  z-index: 2147483647;",
      "  pointer-events: none;",
      "}",
      "#" + ROOT_ID + " .dsh-launcher-btn {",
      "  pointer-events: auto;",
      "  display: block;",
      "  margin: 0;",
      "  padding: 0 6px;",
      "  border: 0;",
      "  background: none;",
      "  cursor: pointer;",
      "  transform-origin: 50% 0;",
      "  animation: dsh-shell-launcher-sway 6.4s ease-in-out infinite;",
      "  -webkit-tap-highlight-color: transparent;",
      "}",
      "#" + ROOT_ID + " svg {",
      "  display: block;",
      "  overflow: visible;",
      "  transition: transform 0.18s cubic-bezier(0.34, 1.56, 0.64, 1);",
      "  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.45));",
      "}",
      /* 绳色与所在表面的品牌色匹配（工作台绿、官方对话蓝），
         让灯读作与横贯视口顶部的 titlebar pulse 同一片 chrome。 */
      "#" + ROOT_ID + " .dsh-launcher-cord {",
      "  stroke: " + PALETTE.cord + ";",
      "  stroke-width: 2;",
      "  stroke-linecap: round;",
      "}",
      "#" + ROOT_ID + " .dsh-launcher-base {",
      "  fill: var(--dsh-launcher-base-fill);",
      "  transition: fill 0.18s ease;",
      "}",
      /* 调色板变量，默认使用在深色页面上仍可读出的半透明白色；
         样式表末尾的浅色模式覆盖会替换这些值。 */
      "#" + ROOT_ID + " {",
      "  --dsh-launcher-base-fill: rgba(255, 255, 255, 0.42);",
      "  --dsh-launcher-bulb-fill: rgba(255, 255, 255, 0.14);",
      "  --dsh-launcher-bulb-stroke: rgba(255, 255, 255, 0.55);",
      "  --dsh-launcher-filament-stroke: rgba(255, 255, 255, 0.35);",
      "  --dsh-launcher-lit-fill: #ffd45e;",
      "  --dsh-launcher-lit-stroke: #ffdf8a;",
      "}",
      "#" + ROOT_ID + " .dsh-launcher-bulb {",
      "  fill: var(--dsh-launcher-bulb-fill);",
      "  stroke: var(--dsh-launcher-bulb-stroke);",
      "  stroke-width: 0.8;",
      "  transition: fill 0.18s ease-out, stroke 0.18s ease-out, stroke-width 0.18s ease-out;",
      "}",
    ];

    // Variant 专属的选择器，在另一种 variant 的 SVG 上并不存在
    //（filament 只属于 pull-string，shade+stem 只属于 desk）。
    if (LAMP_VARIANT === "pull-string") {
      rules.push(
        "#" + ROOT_ID + " .dsh-launcher-filament {",
        "  stroke: var(--dsh-launcher-filament-stroke);",
        "  stroke-width: 1.2;",
        "  stroke-linecap: round;",
        "  transition: stroke 0.18s ease-out;",
        "}",
      );
    }
    if (LAMP_VARIANT === "desk") {
      rules.push(
        "/* 台灯的圆锥形灯罩：与灯泡相同的半透明玻璃，",
        "   配以较细的浅色描边，在深色 strip 上仍能读出清晰的形状。 */",
        "#" + ROOT_ID + " .dsh-launcher-shade {",
        "  fill: rgba(255, 255, 255, 0.18);",
        "  stroke: rgba(255, 255, 255, 0.55);",
        "  stroke-width: 0.8;",
        "  stroke-linejoin: round;",
        "  transition: fill 0.18s ease, stroke 0.18s ease, stroke-width 0.18s ease;",
        "}",
        "/* 灯杆是连接灯罩与底座的单条 1px 线。 */",
        "#" + ROOT_ID + " .dsh-launcher-stem {",
        "  stroke: rgba(255, 255, 255, 0.55);",
        "  stroke-width: 1;",
        "  stroke-linecap: round;",
        "  transition: stroke 0.18s ease, stroke-width 0.18s ease;",
        "}",
      );
    }

    // 拉动时：绳与灯身一起下移，松手回弹。紧凑的 38px 台灯下移 3px；
    // 较高的 66px 工作台灯下移 6px。
    rules.push(
      "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-pulled svg {",
      "  transform: translateY(" + PULL_TRANSLATE_PX + ");",
      "}",
    );

    // 点亮（点击切换）：灯发出光芒。两种 variant 共享相同的常亮语义
    //（无闪烁、无描边粗细跳变 —— 仅保持持续发光直到下一次点击），
    // 但点亮的具体表面不同：紧凑台灯会把灯罩、灯杆、底座、灯泡一起
    // 染成暖色，并叠加 12px 紧凑 + 24px 宽暖的 drop-shadow，让小图标
    // 也能读作真实光源；高挑的拉绳灯只点亮灯泡并把灯丝染成暖色
    //（它已经画得足够大，一圈暖色光晕就够了）。
    if (LAMP_VARIANT === "desk") {
      rules.push(
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on .dsh-launcher-shade {",
        "  fill: rgba(255, 212, 94, 0.32);",
        "  stroke: rgba(255, 212, 94, 0.85);",
        "  stroke-width: 1.2;",
        "}",
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on .dsh-launcher-bulb {",
        "  fill: #ffd45e;",
        "  stroke: #ffdf8a;",
        "  stroke-width: 1.2;",
        "}",
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on .dsh-launcher-stem {",
        "  stroke: rgba(255, 212, 94, 0.7);",
        "  stroke-width: 1.2;",
        "}",
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on .dsh-launcher-base {",
        "  fill: rgba(255, 255, 255, 0.55);",
        "}",
        /* 叠加：细微的景深阴影 + 12px 紧凑暖光 + 24px 宽幅柔光。 */
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on svg {",
        "  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.45))",
        "          drop-shadow(0 0 12px rgba(255, 212, 94, 0.95))",
        "          drop-shadow(0 0 24px rgba(255, 180, 50, 0.55));",
        "}",
      );
    } else {
      // pull-string variant 点亮状态：灯泡填充暖色，灯丝染橙
      //（即爱迪生灯泡中可见的发光元件），整张 SVG 外加一圈 drop-shadow 光晕。
      rules.push(
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on .dsh-launcher-bulb {",
        "  fill: var(--dsh-launcher-lit-fill);",
        "  stroke: var(--dsh-launcher-lit-stroke);",
        "}",
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on .dsh-launcher-filament {",
        "  stroke: #b45309;",
        "}",
        "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-on svg {",
        "  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.45))",
        "          drop-shadow(0 0 10px rgba(255, 212, 94, 0.85));",
        "}",
      );
    }

    // 调用失败（例如 IPC 不可用）：灯泡改亮红色而非维持暖色，
    // 让坏的拉动无需打开开发者工具即可察觉。
    rules.push(
      "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-err .dsh-launcher-bulb {",
      "  fill: #ef4444;",
      "  stroke: #f87171;",
      "}",
      "#" + ROOT_ID + " .dsh-launcher-btn.dsh-launcher-err svg {",
      "  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.45)) drop-shadow(0 0 10px rgba(239, 68, 68, 0.85));",
      "}",
    );

    // 浅色模式（工作台用 body[data-ds-dark-theme] 标记深色主题，
    // 由 boot-theme.ts 在插件加载前写入，并由 ThemePresenter 之后维持）：
    // 琥珀色玻璃与更深色的描边，使灯泡在白色工作台上仍然清晰可读。
    // chat 页面不会设置该属性，所以这条规则依然经由 :not 生效 —— 同时
    // 它在 chat 的白色表面上渲染也毫无问题。仅调色板变量发生变化，
    // 因此 hover/常亮 在不同主题下的优先级保持一致。
    rules.push(
      "body:not([data-ds-dark-theme]) #" + ROOT_ID + " {",
      "  --dsh-launcher-base-fill: #78716c;",
      "  --dsh-launcher-bulb-fill: rgba(245, 158, 11, 0.24);",
      "  --dsh-launcher-bulb-stroke: #a16207;",
      "  --dsh-launcher-filament-stroke: #92400e;",
      "  --dsh-launcher-lit-fill: #f59e0b;",
      "  --dsh-launcher-lit-stroke: #92400e;",
      "}",
      "@keyframes dsh-shell-launcher-sway {",
      "  0%, 100% { transform: rotate(1.6deg); }",
      "  50% { transform: rotate(-1.6deg); }",
      "}",
    );

    return rules.join("\n");
  }

  /**
   * 请外壳把其管理窗口提升到点击位置附近。
   * `point` 是点击的屏幕坐标（MouseEvent.screenX/Y，CSS 像素）；
   * 外壳会把面板移到该坐标附近，让用户无需在另一块显示器上寻找窗口。
   * 当 Tauri IPC 不可用或命令被拒绝时，`onError` 触发，调用方可以
   * 把暖色光替换为红色错误闪烁。
   *
   * 灯所在的两个表面（工作台 webview 与 official-chat strip webview）
   * 都保留 `window.__TAURI__` 不动，因此直接读取是正确的：在注入与
   * 点击之间没有任何指纹脚本会阉割桥接通道。
   */
  function invokeFocusMainShell(point, onError) {
    try {
      var tauri = window.__TAURI__;
      if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
        tauri.core
          .invoke("focus_main_shell", { x: Math.round(point.x), y: Math.round(point.y) })
          .catch(function (err) {
            console.warn("dsh-xlink: focus_main_shell failed:", err);
            onError();
          });
      } else {
        console.warn("dsh-xlink: __TAURI__ unavailable; focus_main_shell not sent");
        onError();
      }
    } catch (err) {
      console.warn("dsh-xlink: focus_main_shell failed:", err);
      onError();
    }
  }

  function inject() {
    if (document.getElementById(ROOT_ID)) {
      return;
    }

    var style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = buildCss();
    document.head.appendChild(style);

    var root = document.createElement("div");
    root.id = ROOT_ID;

    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "dsh-launcher-btn";
    btn.title = "显示主工作台";
    btn.setAttribute("aria-label", "显示主工作台");
    btn.innerHTML = SVG;
    root.appendChild(btn);

    // 点击切换灯的开/关（不会自动熄灭）。每次点击仍会调用 focus_main_shell
    // 把管理面板提到最前；若 IPC 失败，灯会回到关闭状态并显示红色错误
    // 提示，直到下一次点击。
    var isOn = false;
    btn.addEventListener("pointerdown", function () {
      btn.classList.add("dsh-launcher-pulled");
    });
    var release = function () {
      btn.classList.remove("dsh-launcher-pulled");
    };
    btn.addEventListener("pointerup", release);
    btn.addEventListener("pointerleave", release);
    btn.addEventListener("click", function (ev) {
      isOn = !isOn;
      btn.classList.remove("dsh-launcher-err");
      btn.classList.toggle("dsh-launcher-on", isOn);
      invokeFocusMainShell({ x: ev.screenX, y: ev.screenY }, function () {
        isOn = false;
        btn.classList.remove("dsh-launcher-on");
        btn.classList.add("dsh-launcher-err");
      });
    });

    document.body.appendChild(root);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", inject);
  } else {
    inject();
  }
})();