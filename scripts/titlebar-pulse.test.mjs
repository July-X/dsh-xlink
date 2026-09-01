import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import vm from "node:vm";

const scriptPath = path.resolve("src-tauri/src/titlebar-pulse.js");
const source = fs.readFileSync(scriptPath, "utf8");

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
  const styles = [];
  const document = {
    readyState: "complete",
    documentElement: { getBoundingClientRect: () => ({ width: 1280 }) },
    getElementById: () => null,
    createElement: () => ({ id: "", textContent: "" }),
    head: { appendChild: (style) => styles.push(style) },
  };
  const window = { location: { hostname: "127.0.0.1" } };
  window.top = window;
  window.self = window;

  vm.runInNewContext(source, { document, window });

  assert.equal(styles.length, 1);
  assert.match(styles[0].textContent, /width: 100% !important/);
  assert.match(styles[0].textContent, /animation: none !important/);
  assert.doesNotMatch(styles[0].textContent, /@keyframes|translateX|blur\(/);
});
