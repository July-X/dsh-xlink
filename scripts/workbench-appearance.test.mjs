import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";

import { readShellSource } from "./lib/shell-source.mjs";

// 按文件名定位（注入脚本 2026-10-01 搬进了 `harness/`）。解析器见
// scripts/lib/shell-source.mjs 的文件头。
const source = readShellSource("workbench-appearance.js");

/// 在假 DOM / 假 Tauri 桥上跑一遍注入脚本，返回它下发给 `set_window_appearance`
/// 的参数序列。
///
/// 关键契约只有一条：**按 `data-ds-theme-source`（偏好）镜像，绝不按解析结果钉**。
/// `system` 必须下发 `null` —— 若在这里钉住当时解析出来的那一档，页内的
/// `prefers-color-scheme` 会被自己冻住，系统切档时内核再也收不到变化，
/// 「跟随系统」第二次失效。2026-10-18 的修复就是从这个坑里爬出来的。
function run({ themeSource, darkAttribute = false }) {
  const calls = [];
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
    __TAURI__: {
      core: {
        invoke: (command, args) => {
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

  return calls;
}

test("跟随系统时下发 null（解除覆盖），绝不钉住解析出来的那一档", () => {
  const calls = run({ themeSource: "system" });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, "set_window_appearance");
  assert.equal(calls[0].args.theme, null);
});

test("固定档按偏好钉同一档，让原生标题栏与页面同色", () => {
  assert.deepEqual(run({ themeSource: "dark" }).map((call) => call.args.theme), ["dark"]);
  assert.deepEqual(run({ themeSource: "light" }).map((call) => call.args.theme), ["light"]);
});

test("读不到内核信号时什么都不做（降级为「没有这个脚本」）", () => {
  assert.equal(run({}).length, 0);
  // 只有 body 上的解析结果、没有偏好属性时按固定档当深色处理（旧版内核兜底）。
  assert.deepEqual(run({ darkAttribute: true }).map((call) => call.args.theme), ["dark"]);
});

test("同一档不会重复下发（避免每次轮询都跨 IPC）", () => {
  // 脚本内 `applied` 去重：一次性场景里 run() 只跑一轮初始化，因此只验首轮
  // 恰好一次；重复轮询的去重靠 `applied` 字段，这里用源码形状兜住，
  // 防止将来被简化成「每次 tick 都 invoke」。
  assert.match(source, /if \(wanted === applied\) \{/);
  assert.equal(run({ themeSource: "light" }).length, 1);
});
