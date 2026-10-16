// 记忆面板的判据：纯函数的行为，以及**它们真的接在模板/命令上**。
//
// 贯穿全篇的是一条本仓栽过的形状——**helper 有测试、接线没有**：纯函数测试全绿，
// 真机上功能没接。所以每一条能测纯函数的，后面都跟着一条读模板原文、或真的走一遍
// 命令→store 的判据。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { allRulesIncludingVue, effectiveDeclaration } from './lib/css-cascade.mjs';

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
  // 表单自 2026-10-16 起在弹窗组件里（`MemoryEditor.vue`）——三档单选跟着它搬家，
  // 再对着面板找就会红在一句「没有这个入口」上，而入口其实还在。
  const tpl = templateOf(read(`${MEM_DIR}MemoryEditor.vue`));
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
  const { visibleMemories, sourceText, sourcesText, verificationText } = await import('../src/memory/memory.js');
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
  // 一条记录可以带**好几个**来源（C4）：全部都要出现在这一行里。
  assert.equal(
    sourcesText({ sources: [{ kind: 'user-manual' }, { kind: 'extract' }] }),
    '面板手动 + 自动提取',
    '多个来源要全列'
  );
  assert.equal(sourcesText({ source: { kind: 'session' } }), '会话提出', '旧的单来源形状也要认');
  assert.equal(sourcesText({ sources: [] }), '未知来源');
  assert.equal(sourcesText({}), '未知来源', '没有来源字段时如实说不知道');
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
  assert.match(row, /sourcesText\(row\)/, '行里要显示**全部**来源');
  assert.match(row, /verificationText\(row\.verification\)/, '行里要显示验证状态');
  assert.match(row, /emit\('edit', row\)/);
  assert.match(row, /emit\('toggle', accepted \? 'disabled' : 'accepted'\)/, '下一个状态由行自己算');
  assert.match(row, /emit\('remove', row\)/);
  // **面板要真的挂上它、并把三个事件接住**：少接一个就是「按钮点了没反应」，而上面那些
  // 模板判据照样绿（本仓反复栽过的形状）。
  assert.match(tpl, /<MemoryRow/, '面板要真的用它');
  for (const handler of ['@edit="openMemoryEditor"', '@toggle=', '@remove="removeMemory"']) {
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

test('开关的生效语义：有确切判据就用确切的，没有就不许说「已生效」（E6）', async () => {
  const { switchEffectText } = await import('../src/memory/memory.js');
  // 壳侧能算出「改过、而插件还没再来问」——那时说**处理中**，并把「为什么可能要等」讲清。
  const pending = switchEffectText({ effect_pending: true });
  assert.match(pending, /^处理中/, '还没被确认就说处理中');
  assert.match(pending, /用尽/, '要写明预算用尽时为什么要等');
  assert.match(pending, /新会话/, '要指出出路是新会话');
  assert.ok(!/已生效/.test(pending), '处理中时不许说已生效');
  // 插件已经来过一次：这时才敢说已生效。
  assert.match(switchEffectText({ effect_pending: false }), /已生效/);
  // 状态还没读到：不许给一句听起来干脆的话。
  const unknown = switchEffectText(null);
  assert.ok(!/已生效|立即生效|即时生效/.test(unknown), '不知道就不说已生效');
  assert.match(unknown, /还没读到状态/);
  const template = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(template, /switchEffectText\(status\)/, '这句话要接在开关旁边，并带上状态');
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
  // 计数器自 2026-10-16 起跟着表单搬进弹窗组件，判据要跟着指过去。
  const template = templateOf(read(`${MEM_DIR}MemoryEditor.vue`));
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

test('开关返回的状态真的被存下来——「处理中」靠的就是它（E6 的面板那半）', async () => {
  // 这条判据针对的形状是本仓反复栽的那个：**纯函数有测试、接线没有**。
  // `switchEffectText` 的三条分支上面已经测了，可它读的是 `memoryStore.status`；
  // 如果 `setMemoryEnabled` 把命令返回的那个对象丢掉（或另造一个字段子集），
  // 面板会一直停在「还没读到状态」——而那看起来像正常的初始态，不会有人报错。
  //
  // 顺带把 `effect_pending` 钉在这里：它是**新加的字段**，走的正是这条路
  // （命令返回 → store → 模板），少一环就等于没做。
  const { setMemoryEnabled, memoryStore, switchEffectText } = await import(
    '../src/memory/memory.js'
  );
  const returned = { effect_pending: true, file: '/tmp/probe/memory.json', generation: 7 };
  globalThis.window.__TAURI__.core.invoke = () => Promise.resolve(returned);
  await setMemoryEnabled(true);
  // 判据按**字段**而不是对象引用：`memoryStore` 是 Vue 的 `reactive`，读出来是代理，
  // 引用天然不同（先按引用写过一版，红得对、但红的是判据不是实现）。真正要钉的是
  // 「命令返回什么，面板就能看到什么」——逐字段比，抄子集的写法照样会被抓住。
  for (const [key, value] of Object.entries(returned)) {
    assert.deepEqual(memoryStore.status[key], value, `字段 ${key} 没有传到面板`);
  }
  assert.match(switchEffectText(memoryStore.status), /处理中/, '存下来之后面板就该说「处理中」');
});

// —— 界面布局（2026-10-15 这一轮）————————————————————————————————————————
//
// 下面四条钉的是「记忆面板长什么样」的结构事实。四条都按本仓规矩反向验过：把源码
// 改回旧形状，对应判据必须转红（绿在没生效的变异上说明不了任何事）。
//
// 样式一律读**层叠后的生效值**，而且**查规则的出处**：`memory-row*` 这一族类名同时被
// 面板（「待确认」里的候选行）和 `MemoryRow.vue`（已保存的行）用，而规则原先只写在
// `MemoryRow.vue` 的 `<style scoped>` 里。scoped 会给每条规则补一个自家组件的
// `[data-v-*]`，**另一个组件写出来的元素一条都匹配不上**——不报错、build 全绿，
// 只是候选行退回 14px 正文、`<p>` 的默认外边距回来、长正文不换行。

const MEM_DIR = '../src/memory/';
const VOID_TAGS = new Set(['img', 'br', 'hr', 'input', 'meta', 'link', 'source']);

/** 字面量 `class="…"` 的类名。`:class="…"` 不算（那个是不定类名，判据读不到）。 */
function classAttrOf(attrs) {
  const m = /(?<![:\w-])class="([^"]*)"/.exec(attrs);
  return m ? m[1].split(/\s+/).filter(Boolean) : [];
}

function literalClasses(block) {
  return [...block.matchAll(/(?<![:\w-])class="([^"]*)"/g)].flatMap((m) => classAttrOf(m[0]));
}

/** 只留全局 `.css` 里的规则：scoped 块的规则在这里必须一条都不算数。 */
function globalRules() {
  return allRulesIncludingVue().filter((r) => r.file.endsWith('.css'));
}

/** 按标签配对切出一个元素块（注释先剥：本仓注释里会写出字面量的标签名）。 */
function elementBlock(src, tag, cls) {
  const text = src.replace(/<!--[\s\S]*?-->/g, '');
  const opens = new RegExp(`<${tag}\\b[^>]*>`, 'g');
  let at = -1;
  let m;
  while ((m = opens.exec(text))) {
    if (cls === null || classAttrOf(m[0]).includes(cls)) {
      at = m.index;
      break;
    }
  }
  assert.notEqual(at, -1, `模板里找不到 .${cls}（<${tag}>）`);
  const tags = new RegExp(`<\\/?${tag}\\b[^>]*>`, 'g');
  tags.lastIndex = at;
  let depth = 0;
  while ((m = tags.exec(text))) {
    if (m[0].startsWith('</')) {
      depth -= 1;
      if (depth === 0) return text.slice(at, tags.lastIndex);
    } else if (!m[0].endsWith('/>')) {
      depth += 1;
    }
  }
  return assert.fail(`.${cls} 的 <${tag}> 没有配对的结束标签`);
}

/**
 * 某块的**直接**子元素。按标签栈的深度取，不比 `</div>` 个数——「说明被提到页头直接
 * 子级」与「说明还在包裹层里」这两种错结构的 `</div>` 计数是一样的。
 */
function directChildren(block) {
  const re = /<(\/?)([a-zA-Z][\w-]*)((?:"[^"]*"|'[^']*'|[^>"'])*?)(\/?)>/g;
  const stack = [];
  const out = [];
  let m;
  while ((m = re.exec(block))) {
    const tag = m[2].toLowerCase();
    if (m[1]) {
      stack.pop();
      continue;
    }
    if (stack.length === 1) out.push({ tag, classes: classAttrOf(m[3] || '') });
    if (!m[4] && !VOID_TAGS.has(tag)) stack.push(tag);
  }
  return out;
}

test('记忆行的样式落在全局 .css 里——scoped 块跨不过组件，两张卡片共用这一族类名', () => {
  const panel = templateOf(read(`${MEM_DIR}MemoryPanel.vue`));
  const row = templateOf(read(`${MEM_DIR}MemoryRow.vue`));

  // 类名清单**从模板里读**，不写死在判据里：日后谁把 `memory-row__body` 改名而忘了搬
  // 样式，下面的循环会当成「这一族已经不这么叫了」而静默空转。
  const used = [...new Set([...literalClasses(panel), ...literalClasses(row)])]
    .filter((c) => c.startsWith('memory-row'))
    .sort();
  assert.deepEqual(
    used,
    [
      'memory-row',
      'memory-row__actions',
      'memory-row__body',
      'memory-row__main',
      'memory-row__meta',
      'memory-row__name',
      'memory-row__title',
    ],
    '这一族类名的清单变了：下面按类名查生效值的循环会静默空转',
  );

  // 根行还得同时挂 `page-row`：行盒、行间分隔线与「10px 12px」的内边距全来自它，
  // 而它是全局原语——漏挂不会有任何报错，只是两张卡里的行长得不一样。
  for (const [name, tpl] of [['MemoryPanel.vue', panel], ['MemoryRow.vue', row]]) {
    assert.ok(literalClasses(tpl).includes('page-row'), `${name} 的根行漏了 page-row`);
  }

  const expected = {
    'memory-row': { display: 'flex', 'align-items': 'flex-start', gap: '12px' },
    'memory-row__main': { flex: '1', 'min-width': '0' },
    'memory-row__title': { display: 'flex', 'flex-wrap': 'wrap', 'align-items': 'center' },
    'memory-row__name': { 'font-size': '15px', 'font-weight': '600' },
    'memory-row__body': {
      'white-space': 'pre-wrap',
      'overflow-wrap': 'anywhere',
      margin: '4px 0 0',
      color: 'var(--text-secondary)',
    },
    'memory-row__meta': { 'font-size': '12px', color: 'var(--text-muted)' },
    'memory-row__actions': { display: 'flex', flex: '0 0 auto' },
  };

  // 只传**类名自己**当主语 token：多传一个 `page-row` 会让 `.page-row` 那条 (0,1,0)
  // 的同特异度规则一起参与比较，读出来的就不是这一族自己的声明了。
  const rules = globalRules();
  for (const [cls, decls] of Object.entries(expected)) {
    for (const [prop, value] of Object.entries(decls)) {
      const got = effectiveDeclaration([cls], rules, prop);
      assert.equal(
        got,
        value,
        `.${cls} 的 ${prop} 生效值是 ${got === null ? '（全局 .css 里没有这条规则）' : got}，` +
          `期望 ${value}：这一族类名两个组件都在用，规则必须写在 import 进 main.js 的 .css 里`,
      );
    }
  }
});

test('memory.css 的 import 排在 theme.css 之后——.memory-row 与 .page-row 同特异度', () => {
  // 这条不是抄一份 import 清单，而是那份清单里唯一承重的一格：`.page-row` 给的是
  // `align-items: center`、`.memory-row` 要把它改成 `flex-start`（多行正文时行首对齐）。
  // 两条选择器都是 (0,1,0)，**只能靠源码顺序决胜**——顺序一旦反过来，样式不报错，
  // 只是头像那类多行行又回到垂直居中。
  const src = read('../src/main.js');
  const theme = src.indexOf("import './theme.css'");
  const memory = src.indexOf("import './memory/memory.css'");
  assert.ok(theme >= 0, 'main.js 里没有 theme.css 的 import');
  assert.ok(memory >= 0, 'main.js 里没有 memory/memory.css 的 import');
  assert.ok(theme < memory, 'memory.css 必须先于 theme.css 之后被 import，否则 .page-row 会赢');
});

test('记忆页头只有两个直接子级：标题块 + 整行独占的开关块', () => {
  const tpl = templateOf(read(`${MEM_DIR}MemoryPanel.vue`));
  const head = elementBlock(tpl, 'div', 'page-head');
  assert.deepEqual(
    directChildren(head).map((c) => c.classes[0]),
    ['page-head__text', 'memory-switch'],
    '页头形状变了：多一个直接子级就会被 space-between 当成「右侧动作」顶到页面右缘',
  );

  // 生效说明必须跟在开关**同一层**里（同一个 `.memory-switch` 块）。此前它是页头的
  // 第三个直接子级，被挤成页面最右边一个字宽、竖着排三行。
  const sw = elementBlock(head, 'div', 'memory-switch');
  const status = elementBlock(sw, 'p', 'memory-switch__status');
  assert.match(status, /switchEffectText\(status\)/, '开关块里那句生效说明不见了');

  // 样式那半：`flex: 1 0 100%` 才是「落到第二行」的机制。少了它，开关块会缩成内容宽
  // 去和标题抢第一行，说明又回到被挤扁的样子。
  //
  // **另一半前提钉在 `designAlignment.test.js`**（`.page-head` 的 `flex-wrap`，与它的
  // `justify-content: space-between` 挨着）：basis 100% 只在**能换行**的容器里等于
  // 「独占一行」。上一版把「页头是 wrap 的 flex 行」当事实写在注释里、一条断言都没有，
  // 而 theme.css 的 `.page-head` 当时只有 `space-between`——于是开关块不退到第二行，
  // 反而把 shrink 1 的 `.page-head__text` 压到 min-content：标题与开关叠在一起、说明
  // 一个字一行竖排（2026-10-16 用户截图）。判据绿着而页面坏着。
  const rules = allRulesIncludingVue();
  assert.equal(effectiveDeclaration(['memory-switch'], rules, 'flex'), '1 0 100%');
  assert.equal(effectiveDeclaration(['memory-switch__status'], rules, 'flex'), '1 1 auto');
  assert.equal(effectiveDeclaration(['memory-switch__status'], rules, 'min-width'), '0');
});

test('清单筛选自成一行，不再挤在卡头那一排里', () => {
  const tpl = templateOf(read(`${MEM_DIR}MemoryPanel.vue`));
  const card = elementBlock(tpl, 'div', 'card');
  const head = elementBlock(card, 'div', 'card-head');

  // 卡头只剩「标题 + 三枚动作」两栏。三档单选约 234px，先前与那三枚按钮（约 303px）
  // 挤在同一条 340px 里，「通用偏好」折行、按钮再折一次，三样东西各自换行。
  assert.ok(!head.includes('el-radio-group'), '筛选又回到卡头里了');
  const actions = elementBlock(head, 'div', 'memory-head__actions');
  assert.equal((actions.match(/<el-button\b/g) || []).length, 3, '卡头动作排不是三枚按钮了');
  for (const label of ['导出 Markdown', '清空', '刷新']) {
    assert.equal(
      actions.split(label).length - 1,
      1,
      `动作排里的「${label}」不是恰好一枚（判「有一个」不等于判「每一枚都在」）`,
    );
  }

  // 挪出来的那一层：筛选是**卡片自己的直接子级**（卡头与它之间不再夹着包裹层），
  // 而卡内只有这一组单选——按标签判而不按内联文案判，文案是可以改的。
  const kids = directChildren(card);
  assert.equal(
    kids.filter((c) => c.tag === 'el-radio-group').length,
    1,
    '筛选不是卡片的直接子级：夹在卡头或某个包裹层里就会跟着那一排的宽度走',
  );
  const group = elementBlock(card, 'el-radio-group', null);
  assert.match(group, /:model-value="memoryStore\.listScope"/, '那一组单选不是清单筛选');
  // `<el-radio\b` 会把包住它的 `<el-radio-group` 也算成一枚（`-` 是词边界），
  // 所以要连后面的空白或 `>` 一起匹配。
  assert.equal((group.match(/<el-radio[\s>]/g) || []).length, 3, '筛选不是三档了');
  // 空态那三句各挂在 `v-if` / `v-else-if` / `v-else` 上，与筛选互不干扰。
  assert.match(card, /<div v-else class="page-list">/, '空态与列表的分支没了');
});

/**
 * 新建与编辑**共用同一个弹窗**（用户 2026-10-16 要求：表单不再常驻右栏）。
 *
 * 三件各自会静默的事：① 表单留在面板里（改回常驻卡，用户看到的就是没改）；
 * ② 触发按钮的括号漏了——`@click` 直接写方法名会把 MouseEvent 当成 record 传进
 * `openMemoryEditor(record = null)`，于是「新建」打开的是一个事件对象；
 * ③ 行内「编辑」另走一条路（两个入口两套行为，迟早一边改了另一边没改）。
 */
test('新建与编辑共用一个弹窗：按钮与行内「编辑」都走 openMemoryEditor', () => {
  const panel = templateOf(read(`${MEM_DIR}MemoryPanel.vue`));
  const editor = read(`${MEM_DIR}MemoryEditor.vue`);

  // ① 表单字段只在编辑器组件里。
  for (const cls of ['memory-form', 'memory-body']) {
    assert.ok(!panel.includes(cls), `.${cls} 又回到面板里了——它该在弹窗组件里`);
    assert.ok(editor.includes(cls), `编辑器里没有 .${cls}`);
  }
  assert.match(panel, /<MemoryEditor\s*\/>/, '编辑器组件要真的挂上（建了不挂是本仓反复栽的形状）');

  // ② 触发按钮：primary，且**显式空参调用**。
  assert.match(
    panel,
    /<el-button type="primary" :icon="Plus"[^>]*@click="openMemoryEditor\(\)"/,
    '「新建记忆」要接在 openMemoryEditor() 上（少一对括号就会把 MouseEvent 当 record）',
  );
  assert.match(panel, /@edit="openMemoryEditor"/, '行内「编辑」要走同一个入口');

  // ③ 它必须在 `.memory-head__actions` **外面**：那一排的三枚文字按钮已约 287px，
  //    而卡内可用宽只有 348px；四枚并排约 390px，那一排自己不换行 → 最右一枚被裁。
  const head = elementBlock(panel, 'div', 'card-head');
  const actions = elementBlock(head, 'div', 'memory-head__actions');
  assert.ok(!actions.includes('新建记忆'), '「新建记忆」塞进动作排了：那一排放不下第四枚（348px）');
  assert.match(head, /新建记忆/, '「新建记忆」要在卡头里');

  // ④ 编辑器是**弹窗**，不是普通块；宽度显式给（EP 默认 50% = 520px）。
  assert.match(editor, /<el-dialog\b/, '编辑器必须真的用 el-dialog');
  assert.match(editor, /:model-value="memoryStore\.editorVisible"/, '可见性读 store，不在组件里另立一份');
  assert.match(editor, /width="min\(560px, 92vw\)"/, '宽度必须显式给，且与其余弹窗一样夹 vw');
  assert.match(editor, /append-to-body/, '与其余五个弹窗一致（面板祖先带 overflow）');
  assert.match(editor, /:close-on-click-modal="false"/, '点遮罩不该丢草稿');
  assert.match(editor, /:close-on-press-escape="false"/, 'ESC 与遮罩是同一条理由：都是手滑的路径');
});

/**
 * 弹窗开关与草稿（store 那半）：新建清空、编辑带出、关掉丢弃、**保存成功才关**。
 *
 * 「保存失败不许关」不是洁癖：关掉一个装着用户刚写完正文的弹窗，等于把那段文字从他
 * 手里拿走，而且没有第二次机会。`toast` 要真实 DOM，node 里会抛，所以两条路都把那个
 * 异常吞掉——状态改动都在它之前，判据量的正是那些状态。
 */
test('弹窗开关与草稿：新建清空、编辑带出、关闭丢弃、保存成功才关', async () => {
  const { memoryStore, openMemoryEditor, closeMemoryEditor, saveMemory } = await import(
    '../src/memory/memory.js'
  );
  const core = globalThis.window.__TAURI__.core;
  const original = core.invoke;
  const swallow = (p) => p.catch(() => undefined);
  try {
    // 新建：先把草稿弄脏再打开——半截正文不许跟进来（检索预览正是拿草稿当查询的）。
    memoryStore.draft = { ...memoryStore.draft, title: '上一次没写完的' };
    openMemoryEditor();
    assert.equal(memoryStore.editorVisible, true, '新建要打开弹窗');
    assert.equal(memoryStore.draft.title, '', '新建要先清草稿');

    // 编辑：带出那条记录的字段与 revision（少了 revision，后端会把编辑当成新条目）。
    openMemoryEditor({
      id: 'rec-1',
      revision: 7,
      scope: 'general',
      title: 'T',
      body: 'B',
      keywords: ['k'],
      pinned: true,
      verification: 'stale',
    });
    assert.equal(memoryStore.editorVisible, true);
    assert.equal(memoryStore.draft.id, 'rec-1');
    assert.equal(memoryStore.draft.expectedRevision, 7);
    assert.equal(memoryStore.draft.verification, 'stale', '编辑要带出它当前的验证范围');

    // 关掉（✕ / ESC / 取消 都走这一个动作）：草稿丢弃。
    closeMemoryEditor();
    assert.equal(memoryStore.editorVisible, false);
    assert.equal(memoryStore.draft.id, '');
    assert.equal(memoryStore.draft.title, '');

    // 保存失败：弹窗必须还开着、草稿一个字不动。
    openMemoryEditor();
    memoryStore.draft = { ...memoryStore.draft, scope: 'general', title: 't1', body: 'b1' };
    core.invoke = (cmd) => (cmd === 'memory_save' ? Promise.reject(new Error('boom')) : Promise.resolve([]));
    await swallow(saveMemory());
    assert.equal(memoryStore.editorVisible, true, '保存失败还关弹窗 = 把用户刚写的正文拿走');
    assert.equal(memoryStore.draft.title, 't1', '失败时草稿不许动');

    // 保存成功：关弹窗 + 清草稿。
    core.invoke = (cmd) => (cmd === 'memory_save' ? Promise.resolve({ id: 'new-1', scope: 'general' }) : Promise.resolve([]));
    await swallow(saveMemory());
    assert.equal(memoryStore.editorVisible, false, '保存成功后弹窗要关');
    assert.equal(memoryStore.draft.title, '', '保存成功后草稿要清');
  } finally {
    core.invoke = original;
  }
});

test('正文超限的提示读 --danger-strong，且模板仍绑着这个条件类', () => {
  // 12px 的 `--danger` 在白卡上只有 3.68:1（正文门槛 4.5:1），`--danger-strong`
  // 是 5.14:1——写 token 而不是写死色值，暗色那档才跟着一起对。
  const rules = allRulesIncludingVue();
  assert.equal(effectiveDeclaration(['memory-note'], rules, 'color'), 'var(--text-muted)');
  assert.equal(effectiveDeclaration(['memory-note'], rules, 'font-size'), '12px');
  assert.equal(
    effectiveDeclaration(['memory-note', 'memory-over-limit'], rules, 'color'),
    'var(--danger-strong)',
    '超限那档没盖住基线的 muted',
  );

  // 样式看得见、绑定看不见：`classesOf` / `subjectTokens` 只认字面量 `class="…"`，
  // Vue 的 `:class` 一个都读不到，所以这里读原文并数次数。表单（连同这个计数器）
  // 2026-10-16 搬进 `MemoryEditor.vue`，模板锚点跟着搬；样式那半仍走全局扫描
  // （`.memory-note` 已按跨组件共用的约定落进 `memory.css`）。
  const tpl = templateOf(read(`${MEM_DIR}MemoryEditor.vue`));
  const counter = elementBlock(elementBlock(tpl, 'div', 'memory-body'), 'p', 'memory-note');
  assert.match(
    counter,
    /:class="\{\s*'memory-over-limit':\s*bodyBytes\s*>\s*MAX_BODY_BYTES\s*\}"/,
    '计数器不再绑超限那个条件类了：读到的色是常驻的 muted',
  );
});

/**
 * C5：意义冲突的**并排展示**（设计 §7-2「面板把两条并排摆出来，让用户删一条」）。
 *
 * 这条判据盯三件容易各自静默的事：组件被建出来了但没插进候选卡片（helper 有测试、
 * 接线没有——本仓反复栽的形状）；插进去了但没有 id→记录的解析（于是面板上永远是
 * 「不在库里」）；文案里混进了「建议保留新的」这类**代做判断**的话（裁决权在用户）。
 */
test('冲突标记在候选卡片里并排摆出来，动作落在 store 已有命令上', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(tpl, /<MemoryConflict[\s/>]/, '候选卡片要真的用上这个组件');
  assert.match(tpl, /:conflicts="item\.conflicts_with \|\| \[\]"/, '冲突 id 要从候选本身读（老数据没有这个键，所以给空数组兜底）');
  assert.match(tpl, /:records="allMemories"/, '要拿全库记录去解析 id → 标题与正文（用筛选后的列表会解析不到别的项目的）');
  // 两个动作复用面板已有的那两个命令，不新造一条路径。
  assert.match(tpl, /@disable="\(record\) => setMemoryState\(record, 'disabled'\)"/, '停用走已有的 memory_set_state');
  assert.match(tpl, /@remove="removeMemory"/, '删除走已有的 memory_delete（它自带影响范围确认框）');
});

/**
 * C6「用户纠正优先于旧候选」的**并排展示**：模型说这句推翻了先前提出的哪几条，
 * 面板把它们列出来给一个动作。
 *
 * 盯三件容易各自静默的事：组件建出来了但没插进候选卡片（helper 有测试、接线没有）；
 * 插进去了但把 id 解析成「已保存的记忆」——而候选根本不在那份列表里，于是永远显示
 * 「已不在」；以及文案里出现「已自动作废」，那会把「模型标了一下」说成「已经替你决定了」。
 */
test('取代标记在候选卡片里列出被推翻的那几条，动作是丢弃它', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  // **标签名后面必须是空白、斜杠或尖括号**：只写 `/<MemorySuperseded/` 的话，
  // 把它改成 `<MemorySupersededX` 就照样匹配——反向验（2026-10-16）真的量到了这一条：
  // 判据绿着，而「组件没被用上」这件事已经发生了。C5 那条同款断言一并收紧。
  assert.match(tpl, /<MemorySuperseded[\s/>]/, '候选卡片要真的用上这个组件');
  assert.match(tpl, /:supersedes="item\.supersedes \|\| \[\]"/, '取代 id 要从候选本身读（老数据没有这个键，给空数组兜底）');
  assert.match(tpl, /:candidates="candidates"/, '要拿**待确认候选**那份列表去解析 id（记录列表里没有候选）');
  // 动作复用面板已有的那一个命令，不新造一条路径；`false` 就是「拒绝」。
  assert.match(tpl, /@discard="\(candidate\) => decideCandidate\(candidate, false\)"/, '丢弃走已有的 memory_decide_candidate');
});

test('取代块的文案只摆事实，不说「已经替你作废」', () => {
  const src = read('../src/memory/MemorySuperseded.vue');
  const tpl = templateOf(src);
  assert.match(tpl, /由你决定丢弃哪几条/, '要说清这一步在用户手里');
  assert.match(src, /已经不在「待确认」里了/, '引用的那条不在时要如实说，不能静默隐藏');
  // **先剥注释再判**：组件的文档注释里原样写着「已自动作废」作为**反例**，
  // 不剥的话这条判据会命中自己写的解释（判据命中解释 = 判据不判代码）。
  const code = src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
  assert.ok(
    !/已自动作废|自动丢弃|已替你保留|以新的为准/.test(code),
    '出现「已经替你作废」这类话就等于系统已经替用户决定了'
  );
  assert.match(tpl, /v-if="!row\.missing && !busy"/, '候选不在待确认里时不该给动作');
});

test('两个标记块共用同一族类名：样式在 memory.css，两个组件都不许自带', () => {
  const css = read('../src/memory/memory.css');
  assert.match(css, /\.memory-flag\b/, '共用样式要在 memory.css（scoped 的样式组件之间不共享）');
  // **不许两个组件各带一份**：复制一份会被门禁的重复区间扫描算成重复代码，而两种关系
  // 在界面上必须长得一样重——不一样会暗示哪一种更严重。
  for (const file of ['../src/memory/MemoryConflict.vue', '../src/memory/MemorySuperseded.vue']) {
    const src = read(file);
    assert.ok(!/<style/.test(src), `${file} 不该自带样式块（共用 memory.css 那一份）`);
    assert.match(src, /class="memory-flag"/, `${file} 要用共用那一族类名`);
  }
});

/**
 * H7「来源变了」的展示：候选被标 stale 时面板要说一句为什么，**而且不许动那两个按钮**。
 *
 * 盯两件各自会静默的事：一是提示块建了没插进候选卡片（helper 有测试、接线没有）；
 * 二是有人顺手把 stale 当成「系统已经处理掉了」——加上 `v-if="… !== 'stale'"` 把动作
 * 藏起来，或者 `:disabled="… === 'stale'"` 把它点灰。后者在真机上完全看不出来：
 * 用户看到的是「这条过期了」，而他的两条按钮**变灰了**却没有任何地方解释为什么。
 */
test('stale 只多一句提示，保存与丢弃两个动作照常在', () => {
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  // **判据只能盯候选那张卡片**，不能盯整份模板：手工新建记录那组「已过期」单选
  // （`el-radio value="stale"`）也是 `stale`，而它跟「只标不执行」毫无关系。
  // 一开始写成盯整份模板，红在那个正当的用法上——判据越界也是一种判据骗自己。
  //
  // **从候选循环切到它所在那列的末尾**。第一版拿 `<div class="card-head">` 当右锚点，
  // 而那张卡片自己就有一个 card-head（切出一段空的，红在「实际 0 处」——那条红看着像
  // 「提示没接上」，真因是切错区间）。锚点选法本身也是判据的一部分。
  const start = tpl.indexOf('<div v-for="item in candidates"');
  const end = tpl.indexOf('</div>\n\n      <div class="page-layout__col">', start);
  assert.ok(start >= 0 && end > start, '候选那张卡片没找全——两个锚点至少有一个对不上');
  const card = tpl.slice(start, end);
  // 唯一的合法写法：**用它决定「显不显示提示」**。
  const guard = `v-if="item.verification === 'stale'"`;
  const guards = card.split(guard).length - 1;
  assert.equal(
    guards,
    2,
    `标题上的标签与那句提示都该由这一个条件控制，实际 ${guards} 处`
  );
  // **除此之外那张卡片里不许再出现 stale**：只要它出现在 `:disabled` / `v-if` / `v-show` 里，
  // 就是把「只标记不执行」读成了「已经替你处理掉了」。
  const rest = card.split(guard).join('');
  assert.ok(
    !rest.includes('stale'),
    'stale 只允许出现在「显示提示」的那一个条件上——它出现在动作的 disabled/隐藏里就是替你做决定了'
  );
  // 两个动作本来就在（上面那条判据是「不许动它们」，得先确认它们在）。
  assert.match(card, /@click="decideCandidate\(item, true\)"/, '保存按钮还在');
  assert.match(card, /@click="decideCandidate\(item, false\)"/, '丢弃按钮还在');
  // 复用共用那一族类名（三种关系在界面上必须一样重）。
  assert.match(
    card,
    /<div v-if="item\.verification === 'stale'" class="memory-flag">/,
    '提示块要用 memory.css 那份共用样式'
  );
});

test('stale 那句话只说发生了什么，不说系统已经处理掉', () => {
  // `templateOf` 已经剥掉 HTML 注释，所以这里判的是模板里的**字面文案**，
  // 不会命中我写在模板注释里的反例说明（判据命中解释 = 判据不判代码）。
  const tpl = templateOf(read('../src/memory/MemoryPanel.vue'));
  const flag = tpl.slice(tpl.indexOf('class="memory-flag">', tpl.indexOf("item.verification === 'stale'")));
  const lead = flag.slice(0, flag.indexOf('</p>'));
  assert.match(lead, /同一段/, '要说清是「同一段来源被重提」，否则用户看不懂为什么这条忽然过期');
  assert.ok(
    !/已自动|已经替你|已为你处理|以新的为准|不再需要确认/.test(lead),
    '出现「已经替你处理」这类话，就与「只标不执行」相反'
  );
  assert.match(lead, /系统没有替你处理掉/, '反过来要把「没替你处理」明说出来');
});

test('冲突块的文案只摆事实，不代做判断', () => {
  const src = read('../src/memory/MemoryConflict.vue');
  const tpl = templateOf(src);
  assert.match(tpl, /由你决定留哪一条/, '要说清裁决权在用户');
  // 那句话在 `<script setup>` 里（它在准备数据时就定好了），所以判整份文件而不是模板块。
  assert.match(src, /已不在库里/, '引用的那条不在时要如实说，不能静默隐藏');
  // **先剥注释再判**：组件的文档注释里原样写着「建议保留新的」作为**反例**，
  // 不剥的话这条判据会命中自己写的解释（判据命中解释 = 判据不判代码，本仓栽过好几次）。
  const code = src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
  assert.ok(
    !/建议保留|推荐保留|以新的为准|自动保留/.test(code),
    '出现「建议保留哪一条」这类话就等于系统已经替用户选好了'
  );
  // 找不到记录时**不渲染动作**：对一条不存在的记忆点「停用」只会得到一句失败。
  assert.match(tpl, /v-if="!row\.missing && !busy"/, '记录不在时不该给动作');
});
