// 记忆面板的两条判据：注入插件的「未启用」提示不许误报，以及它确实接在模板上。
//
// 这两条是一件事的两半：`injectionDisabled` 是纯函数、测起来容易，而它**没被模板
// 用上**的话一切照旧——本仓为此栽过（helper 有测试、接线没有，纯函数测试全绿而
// 真机上功能没接）。所以下面第二条读模板原文，钉住那个 v-if。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

// `memory.js` 顶层 import 了 `bridge.js`，后者在模块求值期就读 `window.__TAURI__`
// （与 builtinPlugins.test.js 同一套桩）。
globalThis.window = {
  __TAURI__: { core: { invoke: () => Promise.resolve([]), Channel: class { onmessage = null; } } },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'node' }, configurable: true });

/**
 * `.vue` 的模板块，HTML 注释已剥。
 *
 * 必须剥注释：本仓的模板里正是一段解释「为什么要判 `injectPlugin`」的中文注释，
 * 里面原样引用了被判的那个表达式——按整份文件找会把自己写的解释当成命中。
 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

test('状态还没读回来时不算「没启用」——那句提示会误报', async () => {
  const { injectionDisabled } = await import('../src/memory/memory.js');
  // null = `builtin_status` 还没回来（或这次读失败）：不知道就不说。
  assert.equal(injectionDisabled(null), false, '不知道就不能说它没启用');
  assert.equal(injectionDisabled(undefined), false);
  // 读回来了：按启用意图与状态文件是否损坏判。
  assert.equal(injectionDisabled({ requestedEnabled: true }), false);
  assert.equal(injectionDisabled({ requestedEnabled: false }), true);
  assert.equal(
    injectionDisabled({ requestedEnabled: true, stateError: '状态文件异常' }),
    true,
    '状态文件坏了时开关拨不动，这条提示是对的'
  );
});

test('「最近注入」那行话把三种状态分开（开关关着 / 没命中 / 注入了哪几条）', async () => {
  const { injectionText } = await import('../src/memory/memory.js');
  assert.equal(injectionText({ enabled: false, hits: [] }), '开关关着，这一回合没有注入');
  assert.equal(injectionText({ enabled: true, hits: [] }), '没有命中');
  assert.equal(
    injectionText({ enabled: true, hits: ['提交前先跑检查', '构建用 pnpm'] }),
    '注入了 2 条：提交前先跑检查、构建用 pnpm'
  );
  // 缺字段（老壳、或记录被截断）不许抛：这是一条辅助显示，不该把面板带崩。
  assert.equal(injectionText(null), '开关关着，这一回合没有注入');
  assert.equal(injectionText({ enabled: true }), '没有命中');
});

test('「已过期」有入口：编辑器能改验证范围，且真的发给了后端', async () => {
  const { memoryStore, editMemory, resetDraft } = await import('../src/memory/memory.js');
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  for (const label of ['已验证', '未验证', '已过期（事实可能过时）']) {
    assert.match(tpl, new RegExp(`<el-radio value="[a-z]+">${label}`), label);
  }
  const src = read('../src/memory/memory.js');
  assert.match(src, /verification: draft\.verification \|\| 'verified'/, '保存时要把它发出去');

  // 编辑既有条目时要带出它当前的验证范围，不能一律重置成「已验证」。
  memoryStore.draft = { id: '', verification: 'verified' };
  editMemory({
    id: 'x',
    revision: 3,
    scope: 'project',
    title: 't',
    body: 'b',
    keywords: [],
    pinned: false,
    verification: 'stale',
    workspace_path: '/tmp/ws-a',
  });
  assert.equal(memoryStore.draft.verification, 'stale');
  resetDraft();
  assert.equal(memoryStore.draft.verification, 'verified', '新条目默认已验证');
});

test('范围筛选与两个标签映射（§2 的两个视图 + 来源/验证状态显示）', async () => {
  const { visibleMemories, sourceText, verificationText } = await import('../src/memory/memory.js');
  const view = {
    memories: [
      { id: 'a', scope: 'project', title: 'A' },
      { id: 'b', scope: 'general', title: 'B' },
    ],
  };
  assert.equal(visibleMemories(view, 'all').length, 2, '默认看全部');
  assert.deepEqual(visibleMemories(view, 'project').map((row) => row.id), ['a']);
  assert.deepEqual(visibleMemories(view, 'general').map((row) => row.id), ['b']);
  // 还没加载 / 读失败时不许抛：这是一层显示筛选，不是数据源。
  assert.deepEqual(visibleMemories(null, 'project'), []);

  assert.equal(sourceText('user-manual'), '面板手动');
  assert.equal(sourceText('session'), '会话提出');
  assert.equal(sourceText('extract'), '自动提取');
  assert.equal(sourceText(''), '未知来源', '认不出的来源要如实说，不要静默空着');
  assert.equal(verificationText('verified'), '已验证');
  assert.equal(verificationText('unverified'), '未验证');
  assert.equal(verificationText('stale'), '已过期');
  assert.equal(verificationText(''), '未标注');
});

test('面板有范围筛选，行里显示来源与验证状态', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  for (const label of ['全部', '本项目', '通用偏好']) {
    assert.match(tpl, new RegExp(`<el-radio value="[a-z]+">${label}</el-radio>`), label);
  }
  // 行渲染 2026-10-14 拆进了 `MemoryRow.vue`（面板连着五轮长大，每次都靠上调预算过关）。
  const row = templateOf(read('../src/memory/MemoryRow.vue'));
  assert.match(row, /sourceText\(row\.source\.kind\)/, '行里要显示来源');
  assert.match(row, /verificationText\(row\.verification\)/, '行里要显示验证状态');
  assert.match(row, /emit\('edit', row\)/);
  assert.match(row, /emit\('toggle', accepted \? 'disabled' : 'accepted'\)/, '下一个状态由行自己算');
  assert.match(row, /emit\('remove', row\)/);
  // **面板要真的挂上它、并把三个事件接住**：少接一个就是「按钮点了没反应」，而上面那些
  // 模板判据照样绿（本仓反复栽过的形状）。
  assert.match(tpl, /<MemoryRow/, '面板要真的用它');
  for (const handler of ['@edit="editMemory"', '@toggle=', '@remove="removeMemory"']) {
    assert.ok(tpl.includes(handler), `面板没接住 ${handler}`);
  }
});

test('「清空」按钮接在确认框与那条命令上，确认文案说清清不掉的东西', async () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(tpl, />\s*清空\s*<\/el-button>/, '卡片头上要有清空入口');
  assert.match(tpl, /@click="clearMemory"/, '按钮要真的调那个动作');

  const src = read('../src/memory/memory.js');
  assert.match(src, /invoke\('memory_clear'\)/, '走的是 memory_clear');
  assert.match(src, /confirmDialog\('清空全部记忆？'/, '必须过一次确认框');
  // 三件必须说清的事：清掉的范围、清不掉的东西、防再生（§7.3）。
  assert.match(src, /将清空 \$\{memories\} 条记忆与 \$\{candidates\} 条待确认候选/);
  assert.match(src, /已导出的文件与已经发给模型的旧请求不会被撤回/);
  assert.match(src, /记下正文指纹防止模型把同一条重新提回来/);
});

test('面板真的画了「最近注入」并读后端那条命令', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(tpl, /<h2>最近注入<\/h2>/, '卡片标题');
  assert.match(tpl, /injectionText\(note\)/, '每行要说清注入了什么');
  assert.match(
    read('../src/memory/memory.js'),
    /invoke\('memory_recent_injections'\)/,
    '读的是实况那条命令（不是 preview）'
  );
});

test('面板的告警 v-if 用这个判据，而不是只判 !injectReady', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(
    tpl,
    /v-if="enabled && injectionDisabled\(injectPlugin\)"/,
    '告警必须在「确定没启用」时才出现'
  );
  assert.doesNotMatch(
    tpl,
    /!injectReady/,
    '只判 `!injectReady` 会在状态读回来之前挂一句假警报（读失败时永久挂着）'
  );
});
