import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";

import { readShellSource } from "./lib/shell-source.mjs";

// 按文件名定位（注入脚本 2026-10-01 搬进了 `harness/`）。解析器见
// scripts/lib/shell-source.mjs 的文件头。
const source = readShellSource("workbench-appearance.js");

/// 在假 DOM / 假 Tauri 桥上跑一遍注入脚本，返回它下发给 `set_window_appearance`
/// 的参数序列，以及一个「Rust 用 eval 推 DOM 事件」的模拟入口。
///
/// 2026-11-02 的语义（内核设置里选「跟随系统」＝跟随外壳）：
/// - 内核偏好固定 `light` / `dark`：钉同一档，壳偏好不参与；
/// - 内核偏好 `system`：用壳上报的偏好；壳也是 `system` 时下发 `null`（解除覆盖），
///   否则页内的 `prefers-color-scheme` 会被自己冻住，「跟随系统」再次失效。
async function run({ themeSource, darkAttribute = false, shell = { reported: false } } = {}) {
  const calls = [];
  const listeners = new Map();
  const attributes = new Map();
  if (themeSource) {
    attributes.set("data-ds-theme-source", themeSource);
  }
  const document = {
    documentElement: {
      getAttribute: (name) => (attributes.has(name) ? attributes.get(name) : null),
    },
    body: {
      hasAttribute: (name) => name === "data-ds-dark-theme" && darkAttribute,
    },
    addEventListener() {},
  };
  const window = {
    addEventListener: (name, handler) => listeners.set(name, handler),
    __TAURI__: {
      core: {
        invoke: (command, args) => {
          if (command === "get_shell_theme") {
            return Promise.resolve(shell);
          }
          calls.push({ command, args });
          return Promise.resolve();
        },
      },
    },
  };
  window.top = window;
  window.self = window;

  vm.runInNewContext(source, {
    window,
    document,
    console,
    MutationObserver: class {
      observe() {}
    },
    setInterval: () => 0,
  });

  // 让 `get_shell_theme` 的 Promise 链跑完，再看下发结果。
  await Promise.resolve();
  await Promise.resolve();

  return {
    calls,
    dispatch: (detail) => listeners.get("dsh-shell-theme")?.({ detail }),
  };
}

test("内核跟随系统时用壳偏好：壳固定深色就钉深色", async () => {
  const { calls } = await run({
    themeSource: "system",
    shell: { reported: true, theme: "dark" },
  });
  assert.deepEqual(calls.map((call) => call.args.theme), ["dark"]);
});

test("内核跟随系统、壳也跟随系统时下发 null（解除覆盖）", async () => {
  const { calls } = await run({
    themeSource: "system",
    shell: { reported: true, theme: null },
  });
  assert.deepEqual(calls.map((call) => call.args.theme), [null]);
});

test("内核固定档优先：壳主题不覆盖用户在内核里的明确选择", async () => {
  const dark = await run({ themeSource: "dark", shell: { reported: true, theme: "light" } });
  assert.deepEqual(dark.calls.map((call) => call.args.theme), ["dark"]);
  const light = await run({ themeSource: "light", shell: { reported: true, theme: "dark" } });
  assert.deepEqual(light.calls.map((call) => call.args.theme), ["light"]);
});

test("读不到内核信号时什么都不做（降级为「没有这个脚本」）", async () => {
  assert.equal((await run({})).calls.length, 0);
  // 只有 body 上的解析结果、没有偏好属性时按固定档当深色处理（旧版内核兜底）。
  assert.deepEqual(
    (await run({ darkAttribute: true })).calls.map((call) => call.args.theme),
    ["dark"],
  );
});

test("壳还没上报时不动窗口；Rust 推来 DOM 事件后立即生效且同档不重复下发", async () => {
  const { calls, dispatch } = await run({
    themeSource: "system",
    shell: { reported: false },
  });
  assert.equal(calls.length, 0, "壳还没上报时不能把「没答案」当成 system");

  dispatch("light");
  assert.deepEqual(calls.map((call) => call.args.theme), ["light"], "推送到达就立刻切");
  dispatch("light");
  assert.equal(calls.length, 1, "同一档不重复跨 IPC");
});
