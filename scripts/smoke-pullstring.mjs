// Headless smoke test for pullstring-launcher.js: stub a minimal DOM,
// run the script, then simulate a pull and assert the bulb lights and the
// focus_main_shell command is invoked. Run: node scripts/smoke-pullstring.mjs
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("../src-tauri/src/pullstring-launcher.js", import.meta.url), "utf8");

let invoked = [];
const listeners = {};
const elements = {};

function makeEl(tag) {
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
    set id(v) { el._id = v; elements[v] = el; },
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

// Minimal eval: the script is an IIFE referencing window/document/globals.
const fn = new Function("window", "document", "console", "setTimeout", "clearTimeout", src);
fn(window, document, console, sandbox.setTimeout, sandbox.clearTimeout);

const root = elements["dsh-shell-launcher"];
if (!root) throw new Error("widget root was not injected");
const style = elements["dsh-shell-launcher-style"];
if (!style || !style._text.includes("#dsh-shell-launcher")) throw new Error("style not injected");
// Light mode must not keep the translucent-white glass: the sheet needs an
// explicit override keyed on the workbench theme marker (data-ds-dark-theme).
if (!style._text.includes("body:not([data-ds-dark-theme])")) throw new Error("light-mode override missing");
if (!style._text.includes("--dsh-launcher-bulb-stroke: #a16207")) throw new Error("light-mode bulb stroke missing");

const btn = root.children[0];
if (btn.tag !== "button") throw new Error("launcher is not a button");
if (!btn._html.includes("svg")) throw new Error("bulb SVG missing");

// Simulate a pull: pointerdown -> pointerup -> click.
btn.dispatch("pointerdown");
if (!btn.classes.has("dsh-launcher-pulled")) throw new Error("pulled state not applied");
btn.dispatch("pointerup");
if (btn.classes.has("dsh-launcher-pulled")) throw new Error("pulled state not released");
btn.dispatch("click", { screenX: 500.4, screenY: 300.2 });
if (!btn.classes.has("dsh-launcher-on")) throw new Error("bulb did not light");
// A successful click keeps the lamp lit until the next click.
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
