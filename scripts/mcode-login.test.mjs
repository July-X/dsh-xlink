import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

import { readShellSource } from "./lib/shell-source.mjs";

const source = readShellSource("mcode-login.js");

/// 在假 DOM 上跑一遍注入脚本：登录层在不在、处于哪个阶段由用例控制，
/// 返回重载次数、真实发过的命令与观测回调。
function run(options = {}) {
  const state = {
    overlay: false,
    phase: null,
    reloads: 0,
    storage: new Map(),
    observer: null,
    invokes: [],
  };
  const document = {
    documentElement: {},
    addEventListener() {},
    querySelector(selector) {
      if (selector.includes("account-login-page-overlay")) {
        return state.overlay ? { getAttribute: () => null } : null;
      }
      if (selector.includes("account-login-page")) {
        return state.overlay
          ? {
              getAttribute: (name) =>
                name === "data-webui-account-login-phase" ? state.phase : null,
            }
          : null;
      }
      return null;
    },
  };
  const window = { addEventListener() {} };
  window.top = window;
  window.self = window;
  if (options.bridge !== false) {
    window.__TAURI__ = {
      core: {
        invoke: (command) => {
          state.invokes.push(command);
          return options.invoke ? options.invoke() : Promise.resolve();
        },
      },
    };
  }
  const location = { reload: () => { state.reloads += 1; } };
  const sessionStorage = {
    getItem: (key) => (state.storage.has(key) ? state.storage.get(key) : null),
    setItem: (key, value) => state.storage.set(key, value),
  };

  vm.runInNewContext(source, {
    window,
    document,
    location,
    sessionStorage,
    console,
    setTimeout,
    MutationObserver: class {
      constructor(callback) {
        state.observer = callback;
      }
      observe() {}
    },
  });

  return {
    state,
    /** 页面变化后让脚本看一眼（真实页面靠 MutationObserver 触发）。 */
    tick: () => state.observer?.(),
    /** 等一波微任务 / 计时器：收尾是先关弹窗、后重载。 */
    settle: (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms)),
  };
}

function authenticated(options) {
  const ctx = run(options);
  ctx.state.overlay = true;
  ctx.state.phase = "awaiting";
  ctx.tick();
  ctx.state.phase = "authenticated";
  ctx.tick();
  return ctx;
}

test("登录层从待登录变成已认证时重载一次（页面自己不收起登录层）", async () => {
  const { state, tick, settle } = run();
  state.overlay = true;
  state.phase = "awaiting";
  tick();
  assert.equal(state.reloads, 0, "还没认证不能刷新");

  state.phase = "authenticated";
  tick();
  await settle();
  assert.equal(state.reloads, 1, "认证完成后要刷新一次，登录层才会收起");
});

test("重载前先关掉登录时打开的授权弹窗（上游成功路径不关它）", async () => {
  const { state, settle } = authenticated();
  await settle();
  assert.deepEqual(state.invokes, ["close_mcode_popups"], "登录收尾要关本次工作台开的弹窗");
  assert.equal(state.reloads, 1, "关弹窗之后仍要重载，登录层才会收起");
});

test("关弹窗命令失败也照常重载（命令只影响弹窗）", async () => {
  const { state, settle } = authenticated({
    invoke: () => Promise.reject(new Error("ACL denied")),
  });
  await settle();
  assert.equal(state.reloads, 1);
});

test("拿不到壳桥时照常重载（旧壳或脚本被单独加载）", async () => {
  const { state, settle } = authenticated({ bridge: false });
  await settle();
  assert.deepEqual(state.invokes, []);
  assert.equal(state.reloads, 1);
});

test("关弹窗命令没有回程时到点照常重载，不把登录收尾卡住", async () => {
  const { state, settle } = authenticated({ invoke: () => new Promise(() => {}) });
  await settle(350);
  assert.equal(state.reloads, 1);
});

test("登录层一打开就是已登录时不刷新，也不关弹窗（那是页面自己的正常界面）", async () => {
  const { state, tick, settle } = run();
  state.overlay = true;
  state.phase = "authenticated";
  tick();
  await settle();
  assert.equal(state.reloads, 0);
  assert.deepEqual(state.invokes, []);
});

test("登录层没有出现时什么也不做", async () => {
  const { state, tick, settle } = run();
  tick();
  await settle();
  assert.equal(state.reloads, 0);
  assert.deepEqual(state.invokes, []);
});

test("同一分钟内不重复刷新（页面再打开登录层也不会变成刷新循环）", async () => {
  const { state, tick, settle } = run();
  state.overlay = true;
  state.phase = "awaiting";
  tick();
  state.phase = "authenticated";
  tick();
  await settle();
  assert.equal(state.reloads, 1);

  state.phase = "signed-out";
  tick();
  state.phase = "authenticated";
  tick();
  await settle();
  assert.equal(state.reloads, 1, "一分钟内只允许刷新一次");
});

test("注入脚本挂在 MiniMax 工作台建窗链上", () => {
  const windowSource = readShellSource("mcode_window.rs");
  assert.ok(
    windowSource.includes('initialization_script(include_str!("mcode-login.js"))'),
    "MiniMax 工作台建窗链必须注入 mcode-login.js",
  );
});

test("关弹窗命令已授权给 MiniMax 工作台（漏授是 ACL 静默拒，只有真机才能发现）", () => {
  const capability = JSON.parse(
    readFileSync(new URL("../src-tauri/capabilities/mcode-remote.json", import.meta.url), "utf8"),
  );
  assert.ok(
    capability.permissions.includes("allow-close-mcode-popups"),
    "capabilities/mcode-remote.json 必须给 MiniMax 工作台授权 allow-close-mcode-popups",
  );
  assert.ok(capability.windows.includes("mcode-workbench"));
});
