// 内置插件的 N 形态（2026-10-12）：状态是列表、开关按 id 发、显示名归后端。
//
// 这三条此前都不成立也不需要成立——只有一个内置插件时，`view` 是单个对象、
// `invoke('builtin_openai_set_enabled', { enabled })` 不带 id、显示名是前端常量。
// 加第二个内置插件（Ponytail）把三处一起改掉了，于是每一处都要有判据：它们
// 的失效症状全是「界面上没有」「点错了插件」「名字对不上」，而没有任何一行报错。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

// 桥接要在**导入 builtin.js 之前**就位：`bridge.js` 在模块顶层就读
// `window.__TAURI__`，没有 window 就直接 ReferenceError（与 kernelSwitch.test.js
// 同一套桩）。顺带这层桩让下面能真的验「发出去的参数形状」，而不是读源码猜。
let invocations = [];
globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command, args) {
        invocations.push({ command, args });
        if (command === 'builtin_set_enabled') {
          const view = { id: args.plugin, displayName: 'stub', requestedEnabled: args.enabled };
          return Promise.resolve(view);
        }
        return Promise.resolve([]);
      },
      Channel: class { onmessage = null; },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', {
  value: { userAgent: 'node' },
  configurable: true,
});

test('状态是列表；按 id 取的那一项必须真的按 id 取（不是「第一项」）', async () => {
  const { builtinStore, builtinById, openaiBuiltin } = await import('../src/plugins/builtin.js');
  // ponytail 排在前面：拿「第一项」当 openai 的实现（很容易写出来）在别的插件
  // 排前面时会静默返回 ponytail 的状态——概览页与套餐窗口的 Codex 授权入口
  // 就会跟着一个不相干的插件的开关走，而且界面上看不出任何异常。
  builtinStore.list = [
    { id: 'ponytail', displayName: 'Ponytail', requestedEnabled: true },
    { id: 'openai-oauth', displayName: 'OpenAI-OAuth-Plugin', requestedEnabled: false },
  ];
  assert.equal(openaiBuiltin.value.id, 'openai-oauth');
  assert.equal(openaiBuiltin.value.displayName, 'OpenAI-OAuth-Plugin');
  assert.equal(builtinById('ponytail').requestedEnabled, true);
  assert.equal(builtinById('not-a-builtin'), null);
  builtinStore.list = null;
  assert.equal(builtinById('openai-oauth'), null, '未取回时按 null 处理，不许抛');
  assert.equal(openaiBuiltin.value, null);
});

test('拨开关带上插件 id，回填只替换那一项', async () => {
  const { builtinStore, setBuiltinEnabled, loadBuiltinStatus } = await import(
    '../src/plugins/builtin.js'
  );
  invocations = [];
  builtinStore.list = [
    { id: 'openai-oauth', displayName: 'A', requestedEnabled: false },
    { id: 'ponytail', displayName: 'B', requestedEnabled: true },
  ];
  await setBuiltinEnabled('ponytail', false);

  const call = invocations.find((item) => item.command === 'builtin_set_enabled');
  assert.ok(call, "没发出 builtin_set_enabled——命令改名时这条会红");
  assert.equal(call.args.plugin, 'ponytail', '必须按 id 指名要拨哪个插件的开关');
  assert.equal(call.args.enabled, false);
  assert.deepEqual(
    builtinStore.list.map((view) => view.requestedEnabled),
    [false, false],
    '回填只替换被拨的那一项，另一项不能跟着变',
  );

  invocations = [];
  await loadBuiltinStatus();
  assert.equal(
    invocations[0].command,
    'builtin_status',
    '状态命令一次返回全部，不再按插件分命令',
  );
});

test('loading key 带插件 id：两个内置插件同时在拨时 loading 不会互相覆盖', () => {
  const source = read('../src/plugins/builtin.js');
  assert.match(
    source,
    /withLoading\(\s*`builtinToggle:\$\{id\}`/,
    'loading key 必须按插件区分，否则拨 ponytail 会让 openai 的开关也转起来',
  );
  const panel = read('../src/plugins/PluginsPanel.vue');
  assert.match(
    panel,
    /isLoading\('builtinToggle:' \+ builtin\.id\)/,
    '面板读取 loading 时也要带 id，与写入时的 key 对齐',
  );
});

test('概览与套餐窗口读 openaiBuiltin，不再读单对象 builtinStore.view', () => {
  for (const p of [
    '../src/shell/OverviewPanel.vue',
    '../src/subscription/CodexUsageConsent.vue',
  ]) {
    const src = read(p);
    assert.match(
      src,
      /openaiBuiltin\?\.requestedEnabled === true/,
      `${p} 必须按 id 读 openai-oauth 的启用意图`,
    );
    assert.doesNotMatch(
      src,
      /builtinStore\.view/,
      `${p} 还按「只有一个内置插件」的旧形状读状态`,
    );
  }
});