import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";

import { readShellSource } from "./lib/shell-source.mjs";

const source = readShellSource("mcode-login.js");

/// 在假 DOM 上跑一遍注入脚本：登录层在不在、处于哪个阶段由用例控制，
/// 返回重载次数与观测回调。
function run() {
  const state = { overlay: false, phase: null, reloads: 0, storage: new Map(), observer: null };
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
  };
}

test("登录层从待登录变成已认证时重载一次（页面自己不收起登录层）", () => {
  const { state, tick } = run();
  state.overlay = true;
  state.phase = "awaiting";
  tick();
  assert.equal(state.reloads, 0, "还没认证不能刷新");

  state.phase = "authenticated";
  tick();
  assert.equal(state.reloads, 1, "认证完成后要刷新一次，登录层才会收起");
});

test("登录层一打开就是已登录时不刷新（那是页面自己的正常界面）", () => {
  const { state, tick } = run();
  state.overlay = true;
  state.phase = "authenticated";
  tick();
  assert.equal(state.reloads, 0);
});

test("登录层没有出现时什么也不做", () => {
  const { state, tick } = run();
  tick();
  assert.equal(state.reloads, 0);
});

test("同一分钟内不重复刷新（页面再打开登录层也不会变成刷新循环）", () => {
  const { state, tick } = run();
  state.overlay = true;
  state.phase = "awaiting";
  tick();
  state.phase = "authenticated";
  tick();
  assert.equal(state.reloads, 1);

  state.phase = "signed-out";
  tick();
  state.phase = "authenticated";
  tick();
  assert.equal(state.reloads, 1, "一分钟内只允许刷新一次");
});

test("注入脚本挂在 MiniMax 工作台建窗链上", () => {
  const windowSource = readShellSource("mcode_window.rs");
  assert.ok(
    windowSource.includes('initialization_script(include_str!("mcode-login.js"))'),
    "MiniMax 工作台建窗链必须注入 mcode-login.js",
  );
});
