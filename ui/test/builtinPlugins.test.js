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

// 「本内核版本不带某个可选 peer」= 能力降级，不是错误（2026-10-12）：
// `@deepseek-ai/dsh-hooks-claude-code` 在 0.2.1-alpha.2 里被上游降级成
// devDependency，ponytail 仍要能启用，但「会话启动即注入规则」缺席。
// 三条：缺了要说、说的时候用后端那句、没说的时候一个字的提示都不许冒出来。
test('降级说明逐字取后端那句，且只在后端报了 missingPeers 时出现', async () => {
  const { builtinDegradedNote } = await import('../src/plugins/builtin.js');
  assert.equal(builtinDegradedNote(null), '', '状态没取回时不许凭空报降级');
  assert.equal(
    builtinDegradedNote({ id: 'openai-oauth', missingPeers: [] }),
    '',
    '字段为空 = 本内核没有降级，面板不该冒出一枚警告图标',
  );
  assert.equal(
    builtinDegradedNote({ id: 'openai-oauth' }),
    '',
    '老载荷没有这个字段时按「没有降级」处理，不许抛',
  );
  const note =
    '当前内核版本不带 @deepseek-ai/dsh-hooks-claude-code（上游把它从运行时依赖降级成了开发依赖）……';
  assert.equal(
    builtinDegradedNote({ missingPeers: [{ package: 'dsh-hooks-claude-code', note }] }),
    note,
    '文案必须原样用后端那句：前端拼一份就与登记表漂移，而漂移的那份不参与 Rust 侧测试',
  );
  assert.match(
    builtinDegradedNote({ missingPeers: [{ package: 'dsh-hooks-claude-code' }] }),
    /dsh-hooks-claude-code/,
    '后端漏给说明时兜底也要提到包名，不能显示成空白 tooltip',
  );
});

test('降级不许静默：面板画 ⚠，启用成功的提示里也要带出说明', () => {
  const panel = read('../src/plugins/PluginsPanel.vue');
  assert.match(
    panel,
    /v-if="builtinDegradedNote\(builtin\)"/,
    '内置插件行要按后端下发的降级说明画提示',
  );
  assert.match(
    panel,
    /head-tip-icon--warning/,
    '降级是「有件事你该知道」，用 ⚠ 那一档（ⓘ 是补充说明）',
  );
  assert.match(
    panel,
    /:aria-label="builtinDegradedNote\(builtin\)"[\s\S]{0,40}tabindex="0"/,
    '图标 tooltip 的触发元素必须可聚焦 + 有可访问名，否则键盘与读屏用户看不到这条降级',
  );
  const store = read('../src/plugins/builtin.js');
  assert.match(
    store,
    /const degraded = enabled \? builtinDegradedNote\(next\) : '';/,
    '启用成功时要看服务端返回的那一项有没有降级',
  );
  assert.match(
    store,
    /toast\([\s\S]*?已启用，但本内核版本少一项能力[\s\S]*?'warning'\)/,
    '有降级时不能只报「已启用」——用户会以为规则会在会话启动时自动注入',
  );
});