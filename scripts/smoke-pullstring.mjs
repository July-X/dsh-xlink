// 针对 pullstring-launcher.js 的无头冒烟测试：提供一个最小 DOM 桩，
// 运行脚本，再模拟一次拉动，并断言灯泡点亮以及 focus_main_shell 命令
// 被调用。运行命令：node scripts/smoke-pullstring.mjs
import { readShellSource } from "./lib/shell-source.mjs";

const src = readShellSource("pullstring-launcher.js");

let invoked = [];
const listeners = {};
const elements = {};

function makeEl(tag, map = elements) {
  const el = {
    tag,
    children: [],
    attrs: {},
    classes: new Set(),
    style: {},
    _handlers: {},
    setAttribute(k, v) { el.attrs[k] = v; },
    appendChild(c) { el.children.push(c); return c; },
    addEventListener(type, fn) { (el._handlers[type] ??= []).push(fn); },
    dispatch(type, ev) { (el._handlers[type] || []).forEach((fn) => fn(ev || {})); },
    set innerHTML(v) { el._html = v; },
    get innerHTML() { return el._html; },
    set textContent(v) { el._text = v; },
    get textContent() { return el._text; },
    set id(v) { el._id = v; map[v] = el; },
    get id() { return el._id; },
    set className(v) { el._cls = v; },
    get className() { return el._cls; },
    set classList(v) {},
    get classList() {
      return {
        add: (c) => el.classes.add(c),
        remove: (c) => el.classes.delete(c),
        toggle: (c, force) => {
          const next = force === undefined ? !el.classes.has(c) : force;
          if (next) el.classes.add(c);
          else el.classes.delete(c);
          return next;
        },
        contains: (c) => el.classes.has(c),
      };
    },
  };
  return el;
}

const document = {
  readyState: "complete",
  head: makeEl("head"),
  body: makeEl("body"),
  createElement: makeEl,
  getElementById: (id) => elements[id] || null,
  addEventListener(type, fn) { (listeners[type] ??= []).push(fn); },
};

const window = {
  top: null,
  self: null,
  location: { search: "" },
  __TAURI__: { core: { invoke: (cmd, args) => { invoked.push([cmd, args]); return Promise.resolve(); } } },
  addEventListener() {},
};
window.top = window.self = window;

const sandbox = {
  window, document, console,
  setTimeout: () => 0,
  clearTimeout() {},
};

// 最简 eval：脚本是一个 IIFE，会引用 window/document/全局对象。
const fn = new Function("window", "document", "console", "setTimeout", "clearTimeout", src);
fn(window, document, console, sandbox.setTimeout, sandbox.clearTimeout);

const root = elements["dsh-shell-launcher"];
if (!root) throw new Error("widget root was not injected");
const style = elements["dsh-shell-launcher-style"];
if (!style || !style._text.includes("#dsh-shell-launcher")) throw new Error("style not injected");
// 浅色模式不能保留半透明白色玻璃：样式表必须按工作台主题标记
// （data-ds-dark-theme）显式给出覆盖。
if (!style._text.includes("body:not([data-ds-dark-theme])")) throw new Error("light-mode override missing");
if (!style._text.includes("--dsh-launcher-bulb-stroke: #a16207")) throw new Error("light-mode bulb stroke missing");

const btn = root.children[0];
if (btn.tag !== "button") throw new Error("launcher is not a button");
if (!btn._html.includes("svg")) throw new Error("bulb SVG missing");

// 模拟一次拉动：pointerdown -> pointerup -> click。
btn.dispatch("pointerdown");
if (!btn.classes.has("dsh-launcher-pulled")) throw new Error("pulled state not applied");
btn.dispatch("pointerup");
if (btn.classes.has("dsh-launcher-pulled")) throw new Error("pulled state not released");
btn.dispatch("click", { screenX: 500.4, screenY: 300.2 });
if (!btn.classes.has("dsh-launcher-on")) throw new Error("bulb did not light");
// 成功点击会让灯保持点亮，直到下一次点击。
btn.dispatch("click", { screenX: 500.4, screenY: 300.2 });
if (btn.classes.has("dsh-launcher-on")) throw new Error("bulb did not toggle off");
const [cmd, args] = invoked[0] || [];
if (cmd !== "focus_main_shell") throw new Error(`unexpected invokes: ${JSON.stringify(invoked)}`);
if (!args || args.x !== 500 || args.y !== 300)
  throw new Error(`click coordinates not forwarded: ${JSON.stringify(args)}`);

// 重复运行脚本不得重复创建控件。
fn(window, document, console, sandbox.setTimeout, sandbox.clearTimeout);
if (document.body.children.filter((c) => c._id === "dsh-shell-launcher").length !== 1)
  throw new Error("widget duplicated on re-run");

// 失败路径：invoke 拒绝时用红色闪烁替换暖光。
let failingInvoked = 0;
window.__TAURI__ = { core: { invoke: () => { failingInvoked++; return Promise.reject(new Error("denied")); } } };
btn.dispatch("click");
await Promise.resolve(); // 等拒绝处理函数执行完毕
if (!btn.classes.has("dsh-launcher-err")) throw new Error("error flash not applied");
if (btn.classes.has("dsh-launcher-on")) throw new Error("warm glow not cleared on error");
if (failingInvoked !== 1) throw new Error("failing invoke not called");

console.log("smoke-pullstring: all assertions passed");

// MiniMax 工作台：建窗链在注入本脚本前写 `window.__DSH_SHELL_SURFACE__ =
// "mcode"`。灯要挂进页面的 `[data-webui-shell]` 容器（才能继承
// `--webui-rail-width`），绳色走 MiniMax 品牌粉（深色 #ed94bd / 浅色
// #ad3b73），dsg 工作台那套 `body:not([data-ds-dark-theme])` 判据换成
// `html:not(.dark)`（MiniMax 用它自己的 html class 表达主题）。
const elements2 = {};
const shellHost = makeEl("div", elements2);
const doc2 = {
  readyState: "complete",
  head: makeEl("head", elements2),
  body: makeEl("body", elements2),
  createElement: (tag) => makeEl(tag, elements2),
  getElementById: (id) => elements2[id] || null,
  querySelector: (sel) => (sel === "[data-webui-shell]" ? shellHost : null),
  addEventListener() {},
};
const invoked2 = [];
const window2 = {
  top: null,
  self: null,
  location: { search: "" },
  __DSH_SHELL_SURFACE__: "mcode",
  __TAURI__: { core: { invoke: (cmd, args) => { invoked2.push([cmd, args]); return Promise.resolve(); } } },
  addEventListener() {},
};
window2.top = window2.self = window2;
fn(window2, doc2, console, sandbox.setTimeout, sandbox.clearTimeout);

const root2 = elements2["dsh-shell-launcher"];
if (!root2) throw new Error("mcode: widget root was not injected");
if (!shellHost.children.includes(root2)) throw new Error("mcode: lamp must live inside [data-webui-shell]");
if (doc2.body.children.includes(root2)) throw new Error("mcode: lamp must not be a body child");
const style2 = elements2["dsh-shell-launcher-style"];
if (!style2) throw new Error("mcode: style not injected");
for (const needle of [
  "left: calc(var(--webui-rail-width, 240px) - 26px);",
  "--dsh-launcher-cord: #ed94bd;",
  "stroke: var(--dsh-launcher-cord);",
  "html:not(.dark) #dsh-shell-launcher",
  "--dsh-launcher-cord: #ad3b73;",
]) {
  if (!style2._text.includes(needle)) throw new Error("mcode: missing CSS: " + needle);
}
if (style2._text.includes("body:not([data-ds-dark-theme])")) {
  throw new Error("mcode: DSH light-mode selector leaked into the MiniMax surface");
}
const btn2 = root2.children[0];
if (!btn2._html.includes('viewBox="0 0 24 66"')) throw new Error("mcode: expected the full pull-string lamp");
btn2.dispatch("click", { screenX: 10, screenY: 20 });
if (invoked2[0]?.[0] !== "focus_main_shell") throw new Error("mcode: focus_main_shell not invoked");
