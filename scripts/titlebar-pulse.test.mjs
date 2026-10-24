import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";

import { readShellSource } from "./lib/shell-source.mjs";

// 按文件名定位，不写死 `src-tauri/src/titlebar-pulse.js`：注入脚本 2026-10-01
// 搬进了 `harness/`，写死路径的话这条测试会以 ENOENT 变红，而它要查的契约
// （没有常驻动画）一个字都没变。解析器见 lib/shell-source.mjs 的文件头。
const source = readShellSource("titlebar-pulse.js");

/** 在假的 document 里跑一遍注入脚本，返回它生成的 CSS。 */
function renderFor(hostname) {
  const styles = [];
  const document = {
    readyState: "complete",
    documentElement: { getBoundingClientRect: () => ({ width: 1280 }) },
    getElementById: () => null,
    createElement: () => ({ id: "", textContent: "" }),
    head: { appendChild: (style) => styles.push(style) },
  };
  const window = { location: { hostname } };
  window.top = window;
  window.self = window;

  vm.runInNewContext(source, { document, window });

  assert.equal(styles.length, 1, "每个表面只挂一条品牌线");
  return styles[0].textContent;
}

test("标题栏注入脚本不包含常驻渲染动画", () => {
  assert.doesNotMatch(source, /@keyframes/);
  assert.doesNotMatch(source, /animation:[^\n]*infinite/);
  assert.doesNotMatch(source, /requestAnimationFrame|setInterval|setTimeout/);
  assert.doesNotMatch(source, /transform:/);
  assert.doesNotMatch(source, /filter: blur/);
  assert.doesNotMatch(source, /box-shadow: 0/);
  assert.match(source, /animation: none !important/);
});

test("标题栏脚本注入静态品牌线", () => {
  const css = renderFor("127.0.0.1");

  assert.match(css, /width: 100% !important/);
  assert.match(css, /animation: none !important/);
  assert.doesNotMatch(css, /@keyframes|translateX|blur\(/);
});

test("官网网页版那条线两端是直角（实色铺满，不做两端淡出）", () => {
  // 2026-10-23 用户拍板：两端渐变淡出读作「圆角过渡」，要的是硬边。
  // 这条判据跑的是**官网那条分支**——此前所有断言都只跑 127.0.0.1（工作台
  // 那条），官方分支的形状根本没被测到过。
  //
  // 2026-10-24 三站三色之后，「官方蓝」不再是所有官方站的颜色，形状这条改跑
  // DeepSeek（它仍是官网那族的蓝）；颜色由下面那两条分站断。
  const css = renderFor("chat.deepseek.com");

  // 实色：背景就是那个官方蓝本身，没有任何渐变、没有 transparent。
  assert.match(css, /background: #4D6BFE !important/);
  assert.doesNotMatch(css, /linear-gradient/);
  assert.doesNotMatch(css, /transparent/);
  // 线还是 3px（本次只改两端的形状，不改粗细）。
  assert.match(css, /height: 3px !important/);
});

test("三站各用各的品牌色（2026-10-24 用户拍板）", () => {
  // MiniMax 粉 = MiniMax **官方 logo 渐变的品牌色 1**（官方图标 SVG 的
  // linearGradient stop-color，另一端是橙红）。ChatGPT 黑 = OpenAI 品牌黑。
  // DeepSeek 蓝不变。
  assert.match(renderFor("chat.deepseek.com"), /background: #4D6BFE !important/);
  assert.match(renderFor("agent.minimaxi.com"), /background: #E2167E !important/);
  assert.match(renderFor("chatgpt.com"), /background: #000000 !important/);
});

test("MiniMax 的落地域也走粉色（漏了它就是工作台绿线）", () => {
  // MiniMax 的入口 `agent.minimaxi.com` 会 302 到 `agent.minimax.cn`，内容
  // webview 最终的 `location.hostname` 是**落地域**。站表里只写入口域的话，
  // 落地域匹配不到任何站 ⇒ 顶条走工作台分支 ⇒ 用户实机看到的是一条绿线
  // （2026-10-24 报「MiniMax 这个绿色条」）。这条判据钉住落地域也在站表里。
  const css = renderFor("agent.minimax.cn");

  assert.match(css, /background: #E2167E !important/);
  assert.doesNotMatch(css, /linear-gradient/);
  assert.doesNotMatch(css, /transparent/);
});

test("工作台那条线保持原来的渐变（不同表面，不在本次改动范围）", () => {
  const css = renderFor("127.0.0.1");

  assert.match(css, /linear-gradient/);
  assert.match(css, /transparent 0%/);
  assert.match(css, /background: linear-gradient/);
});
