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
  // 字段没下发（旧壳）：也不说——「不知道」不能当成「没接线」。
  assert.equal(injectionDisabled({ requestedEnabled: false }), false, '没有 wired 就别下结论');
  // 读回来了：按**接线事实**判（记忆内核那次评审第 3 条）——内核读的是接线行，
  // `requestedEnabled` 只是壳里那份 state.json，只看它两个方向都会错。
  assert.equal(injectionDisabled({ wired: true, loadState: 'prepared' }), false, '接线了且就绪');
  assert.equal(injectionDisabled({ wired: false, loadState: 'none' }), true, '没接线才说没启用');
  assert.equal(
    injectionDisabled({ wired: true, loadState: 'incompatible' }),
    true,
    '接线行还在但内核已切换：内核照样加载着它，而面板要说清「重新拨一次开关修复」'
  );
  assert.equal(
    injectionDisabled({ wired: false, requestedEnabled: true, loadState: 'none' }),
    true,
    '启用的两步之间失败：接线行没写成，内核确实没加载它'
  );
  assert.equal(
    injectionDisabled({ wired: false, stateError: '状态文件异常' }),
    false,
    '状态文件坏了：不知道就不说'
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

test('批准候选时能当场选范围（§6.1：由用户明确选择通用范围）', async () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(
    tpl,
    /@click="decideCandidate\(item, true, 'general'\)"/,
    '候选卡片要有「存为通用偏好」'
  );
  assert.match(tpl, /v-if="item\.scope !== 'general'"/, '已经是通用偏好的候选不必再给这个按钮');
  // 私有信息那句话必须在：通用偏好会跟着用户去别的项目（§6.1 要求提升前去掉路径与客户名）。
  assert.match(tpl, /先确认正文里没有路径、客户名/);

  const src = read('../src/memory/memory.js');
  assert.match(
    src,
    /invoke\('memory_decide_candidate', \{ id: candidate\.id, accept, scope \}\)/,
    'scope 要真的发给后端'
  );
  assert.match(src, /已存为通用偏好/, '结果提示要分得清存成了哪一种');
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

test('提取任务卡片：全零也要说话，失败那行要带原因（评审第 12 条）', async () => {
  const { jobsSummaryText, jobLine } = await import('../src/memory/memory.js');
  // 还没读到：如实说「还没读到」，不要假装「没有任务」。
  assert.match(jobsSummaryText(null), /还没读到/);
  // 全零：**「一条任务都没有」本身就是答案**（说明这一轮压根没排），不能只显示一个空串。
  assert.match(jobsSummaryText({ summary: { pending: 0, running: 0, done: 0, failed: 0 } }), /还没有提取任务/);
  assert.equal(
    jobsSummaryText({ summary: { pending: 2, running: 1, done: 7, failed: 1 } }),
    '待跑 2 · 在跑 1 · 完成 7 · 失败 1'
  );
  // 失败那行要带 stage：用户问「为什么一条都没提取出来」时，答案就是这一列。
  assert.equal(jobLine({ state: 'failed', key: { cursor: 42 }, error: 'model：模型没有返回任何内容' }),
    '#42 failed：model：模型没有返回任何内容');
  assert.equal(jobLine({ state: 'pending', key: { cursor: 9 } }), '#9 pending');
});

test('提取任务卡片真的接在模板上（helper 有测试、接线没有是本仓反复栽的形状）', () => {
  const template = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(template, /<h2>提取任务<\/h2>/, '卡片要在面板上');
  assert.match(template, /jobsSummaryText|jobsText/, '摘要那句要接上');
  assert.match(template, /jobLine\(job\)/, '每条任务那行要接上');
  // 「一条都没提取出来时先看这里」——这句提示是这张卡片存在的理由，钉住它。
  assert.match(template, /一条记忆都没提取出来时先看这里/);
});

test('开关的生效语义是**条件式**的，不许说「已生效」（E6）', async () => {
  const { switchEffectText } = await import('../src/memory/memory.js');
  const text = switchEffectText();
  // 一半是「下一步就生效」、另一半是「预算用尽要等新会话」——两半都要有。
  assert.match(text, /下一步/, '要写明通常下一步就生效');
  assert.match(text, /用尽/, '也要写明预算用尽时的情况');
  assert.match(text, /新会话/, '要指出出路是新会话');
  assert.ok(!/已生效|立即生效|即时生效/.test(text), '不许给一句听起来干脆但可能不真的话');
  const template = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(template, /switchEffectText\(\)/, '这句话要接在开关旁边');
});

test('正文按**字节**判，与后端同一个数（评审第 4 条）', async () => {
  const { MAX_BODY_BYTES, byteLength, canSaveDraft, memoryStore } = await import('../src/memory/memory.js');
  assert.equal(MAX_BODY_BYTES, 8192, '与后端 store::MAX_BODY_BYTES 逐字对齐');
  assert.equal(byteLength('abc'), 3);
  assert.equal(byteLength('中'), 3, '中文一个字 3 字节');
  // 4000 个中文字 ≈ 12000 字节 > 8192：**面板自己就该拦住**，不能等后端拒。
  memoryStore.draft.title = '标题';
  memoryStore.draft.body = '中'.repeat(4000);
  memoryStore.draft.scope = 'general';
  assert.equal(canSaveDraft(), false, '超字节上限时不许保存');
  memoryStore.draft.body = '中'.repeat(2000); // 6000 字节，放行
  assert.equal(canSaveDraft(), true);
  // 模板里不该再有按字符数的 maxlength（它会骗人）。
  const template = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.ok(!/maxlength="4000"/.test(template), '正文那栏不许再按字符数限长');
  assert.match(template, /bodyBytes/, '要有按字节的计数器');
});

test('导出、清空、空态都按**未筛选**的库判（评审第 2 条）', () => {
  const template = templateOf(read('../src/memory/MemoryPanel.vue'));
  // 三处动作/空态都不许读筛选后的 `list`——它会让「库里只有通用偏好 + 视图选本项目」
  // 显示成「还没有记忆」，还把导出与清空变灰。
  assert.match(template, /allMemories\.length/, '要有未筛选的那份');
  assert.ok(!/disabled="globalBusy \|\| !list\.length"/.test(template), '导出不该读 list');
  assert.ok(!/!list\.length && !candidates\.length/.test(template), '清空不该读 list');
  // 两种空态要分开说。
  assert.match(template, /还没有记忆/);
  assert.match(template, /当前筛选下没有记忆/, '「筛掉了」与「库是空的」是两回事');
});
