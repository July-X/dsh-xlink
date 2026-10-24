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

test("工作台那条线保持原来的渐变（不同表面，不在本次改动范围）", () => {
  const css = renderFor("127.0.0.1");

  assert.match(css, /linear-gradient/);
  assert.match(css, /transparent 0%/);
  assert.match(css, /background: linear-gradient/);
});

test("官方分支已随那条彩色条一起退役，脚本里不许留着站表", () => {
  // 2026-10-24 用户拍板「移除下方的彩色条」：官方内容页**不再**注入本脚本，
  // 品牌色改由页签栏的文字承担（Rust 侧 `OFFICIAL_CHAT_BRAND_COLORS` →
  // `official_chat_tabs` → 页签栏）。
  //
  // 脚本里若还留着那张按 hostname 分色的站表，它就是一份**没有消费方的死事实
  // 源**：它与 Rust 那份迟早漂，而漂了没人会发现——官方内容页压根不跑这个脚本
  // 了。判据第 20 项比的是 Rust 那张表。
  assert.doesNotMatch(source, /OFFICIAL_HOSTNAMES/);
  assert.doesNotMatch(source, /var SITES/);
  assert.doesNotMatch(source, /isOfficial/);
});
