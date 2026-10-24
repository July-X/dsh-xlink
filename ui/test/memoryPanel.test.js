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
    false,
    '接线行还指着旧指纹：内核照旧加载着它，重接线由启动工作台自动做，不许挂假警报'
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

test('「最近注入」那行话把三种状态分开（开关关着 / 没匹配到 / 注入了哪几条）', async () => {
  const { injectionText } = await import('../src/memory/memory.js');
  assert.equal(injectionText({ enabled: false, hits: [] }), '开关关着，这一回合没有注入');
  // 「没有命中」是检索术语，用户读不出「命中」的主语是谁；写成「没有匹配到记忆」。
  assert.equal(injectionText({ enabled: true, hits: [] }), '没有匹配到记忆');
  assert.equal(
    injectionText({ enabled: true, hits: ['提交前先跑检查', '构建用 pnpm'] }),
    '注入了 2 条：提交前先跑检查、构建用 pnpm'
  );
  // 缺字段（老壳、或记录被截断）不许抛：这是一条辅助显示，不该把面板带崩。
  assert.equal(injectionText(null), '开关关着，这一回合没有注入');
  assert.equal(injectionText({ enabled: true }), '没有匹配到记忆');
});

test('项目名只取最后一段，完整路径交给 title（一行放得下，信息一个字不丢）', async () => {
  const { baseName } = await import('../src/shell/labels.js');
  assert.equal(baseName('/Users/a/2026/github/dsh-xlink'), 'dsh-xlink');
  assert.equal(baseName('/Users/a/proj/'), 'proj', '尾斜杠不算一段');
  assert.equal(baseName('C:\\a\\b'), 'b', 'Windows 的反斜杠同样认');
  assert.equal(baseName('proj'), 'proj', '本来就只有一段');
  // 认不出就原样返回：只有根、空值、缺字段都不许抛（调用方用 `|| '未知项目'` 兜底）。
  assert.equal(baseName('/'), '/');
  assert.equal(baseName(''), '');
  assert.equal(baseName(null), '');
  assert.equal(baseName(undefined), '');
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
  // 「按项目」而不是「本项目」（用户 2026-10-17 第二问）：这一档列的是**所有**项目范围的
  // 记忆，不是「当前这个项目」——旧名字会让人以为它按当前目录过滤。
  for (const label of ['全部', '按项目', '通用偏好']) {
    assert.match(tpl, new RegExp(`<el-radio value="[a-z]+">${label}</el-radio>`), label);
  }
  // 行渲染 2026-10-14 拆进了 `MemoryRow.vue`（面板连着五轮长大，每次都靠上调预算过关）。
  const row = templateOf(read('../src/memory/MemoryRow.vue'));
  assert.match(row, /sourcesText\(row\)/, '行里要显示**全部**来源');
  assert.match(row, /verificationText\(row\.verification\)/, '行里要显示验证状态');
  // 行在 2026-10-17 之后只发一个 `open`：动作按钮全部搬进详情弹窗（用户要求「默认内容
  // 精简收缩，点击后弹出操作界面」）。三个事件名留在这一行的断言里，是因为「面板挂上了
  // 组件却没接住事件」是本仓反复栽过的形状——现在要接住的是 `open`。
  assert.match(row, /emit\('open', row\)/);
  assert.doesNotMatch(row, /emit\('(edit|toggle|remove)'/, '行自己不许再有动作事件');
  assert.match(tpl, /<MemoryRow/, '面板要真的用它');
  assert.ok(tpl.includes('@open="openMemoryDetail"'), '面板没接住 @open');
  // 动作那半改挂在详情弹窗上：接不住就是「弹窗里的按钮点了没反应」。
  for (const handler of ['@edit="editFromDetail"', '@toggle=', '@remove="removeMemory"']) {
    assert.ok(tpl.includes(handler), `面板没接住 ${handler}`);
  }
});

test('批准候选时能当场选范围（§6.1：由用户明确选择通用范围）', async () => {
  // 2026-10-19：三枚动作从候选行抽进 `MemoryCandidateActions.vue`——**行与详情弹窗
  // 共用这一份**（同一条候选在两处都要能裁决，各写一份迟早一边漏了 scope 或 loading
  // key，而两边都不报错）。接线跟着搬到那个文件，**接线在哪个文件里就查哪个文件**。
  const actions = templateOf(read('../src/memory/MemoryCandidateActions.vue'));
  assert.match(
    actions,
    /@click="decideCandidate\(item, true, 'general'\)"/,
    '「存为通用偏好」要真的把 scope 传下去'
  );
  assert.match(
    actions,
    /v-if="item\.kind !== 'handoff' && item\.kind !== 'experience' && item\.scope !== 'general'"/,
    '已经是通用偏好的候选不必再给这个按钮；经验有自己的双范围按钮、交接恒为项目范围，都不该出现这枚'
  );
  assert.match(actions, /@click="decideCandidate\(item, true\)"/, '「保存」还在');
  assert.match(actions, /@click="decideCandidate\(item, false\)"/, '「忽略」还在');
  // 共用那半：行与详情都必须用这一份，各写一排按钮就会有两个事实源。
  for (const file of ['MemoryCandidateRow.vue', 'MemoryCandidateDetail.vue']) {
    const tpl = templateOf(read(`../src/memory/${file}`));
    assert.match(tpl, /<MemoryCandidateActions[\s/>]/, `${file} 没用共用的那排动作`);
    assert.equal(
      (tpl.match(/<el-button\b/g) || []).length,
      0,
      `${file} 里又自己写了一排按钮：裁决只有一份实现`,
    );
  }
  // 私有信息那句话必须在：通用偏好会跟着用户去别的项目（§6.1 要求提升前去掉路径与客户名）。
  // 它挂在卡头的 ⓘ 上（切到「待确认」时才出现），所以仍然在面板模板里。
  assert.match(templateOf(read('../src/memory/MemoryPanel.vue')), /先确认正文里没有路径、客户名/);

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
  // 标题里自 2026-10-16 起还挂着那枚 ⓘ（常驻说明收进 tooltip），所以只钉**标题文字**
  // 而不钉 `<h2>最近注入</h2>` 这个字面形状——判据该钉「这张卡片在不在」，
  // 钉死标签形状会让每次往标题加东西都变成一次假红。
  assert.match(tpl, /最近注入\s*<el-tooltip/, '卡片标题');
  assert.match(tpl, /injectionText\(note\)/, '每行要说清注入了什么');
  // 2026-10-16 用户要求这一屏不出现纵向滚动条：项目名只留最后一段（实测 5 条 × 18px
  // = 90px，多过整页溢出量），完整路径进 `title`——截的是显示，不是这条信息。
  assert.match(tpl, /baseName\(note\.workspace\)/, '项目名要压成最后一段');
  assert.match(tpl, /:title="note\.workspace"/, '完整路径进 title，一个字都不许丢');
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
  const { jobsSummaryText, jobLine, jobStateText, stageText } = await import('../src/memory/memory.js');
  // 还没读到：如实说「还没读到」，不要假装「没有任务」。
  assert.match(jobsSummaryText(null), /还没读到/);
  // 全零：**「一条任务都没有」本身就是答案**（说明这一轮压根没排），不能只显示一个空串。
  assert.match(jobsSummaryText({ summary: { pending: 0, running: 0, done: 0, failed: 0 } }), /还没有提取任务/);
  assert.equal(
    jobsSummaryText({ summary: { pending: 2, running: 1, done: 7, failed: 1 } }),
    '待跑 2 · 在跑 1 · 完成 7 · 失败 1'
  );
  // 失败那行要带 stage：用户问「为什么一条都没提取出来」时，答案就是这一列。
  // **阶段名要中文化**：插件把 `${stage}：${reason}` 原样拼进错误串，中文句子里夹一个
  // `model` 读起来像乱码；映射表与 `plugins/memory/host/run.js` 的 `STAGES` 同一套值。
  assert.equal(jobLine({ state: 'failed', key: { cursor: 42 }, error: 'model：模型没有返回任何内容' }),
    '第 42 条事件 · 失败：模型调用：模型没有返回任何内容');
  assert.equal(jobLine({ state: 'failed', key: { cursor: 7 }, error: 'parse：解析不了' }),
    '第 7 条事件 · 失败：解析输出：解析不了');
  // 认不出的阶段名原样留着（插件将来加阶段时至少看得见），正文里的英文不许被顺手改掉。
  assert.equal(stageText('weird'), 'weird');
  assert.equal(stageText(''), '');
  assert.equal(
    jobLine({ state: 'failed', key: { cursor: 1 }, error: 'not a stage: keep me' }),
    '第 1 条事件 · 失败：not a stage: keep me'
  );
  // **中文界面里不许冒英文状态**：`pending` / `running` 是 `jobs.rs` 里的常量，
  // 而摘要那句写的是「待跑 / 在跑 / 完成 / 失败」——同一屏里同一件事只能有一个名字。
  assert.equal(jobLine({ state: 'pending', key: { cursor: 9 } }), '第 9 条事件 · 待跑');
  for (const [raw, label] of [['pending', '待跑'], ['running', '在跑'], ['done', '完成'], ['failed', '失败']]) {
    assert.equal(jobStateText(raw), label, `${raw} 要有中文标签`);
    assert.ok(!jobLine({ state: raw, key: { cursor: 1 } }).includes(raw), '行里不许出现英文状态');
  }
  // 认不出的状态原样显示而不是吞掉（老壳 / 新状态先出现时至少看得见）。
  assert.equal(jobStateText('weird'), 'weird');
  assert.equal(jobStateText(''), '未知');
});

test('提取任务卡片真的接在模板上（helper 有测试、接线没有是本仓反复栽的形状）', () => {
  const template = templateOf(read('../src/memory/MemoryPanel.vue'));
  // 同上：标题文字 + 紧跟的那枚 ⓘ，不钉 `<h2>提取任务</h2>` 的字面形状。
  assert.match(template, /提取任务\s*<el-tooltip/, '卡片要在面板上');
  assert.match(template, /jobsSummaryText|jobsText/, '摘要那句要接上');
  assert.match(template, /jobLine\(job\)/, '每条任务那行要接上');
  // 「一条都没提取出来时先看这里」——这句提示是这张卡片存在的理由，钉住它。
  assert.match(template, /一条记忆都没提取出来时先看这里/);
  // **「（P2）」是内部阶段名**：用户读不懂，而这一屏的所有字都是产品的一部分。
  assert.ok(!template.includes('P2'), '面板里不许留内部阶段名');
  // 自动提取归上面那个开关管（`jobs_routes.rs` 的 enqueue 在 `!use_approved` 时回 disabled）——
  // 不说的话，想停掉自动提取的人找不到开关，想留着它的人也不知道关掉会一起停。
  assert.match(template, /它跟着上面那个开关一起开关/);
});

test('面板的文案不许把内部术语、英文状态或会读错的计数摆给用户（2026-10-18 消歧义）', async () => {
  const panel = templateOf(read('../src/memory/MemoryPanel.vue'));
  const raw = read('../src/memory/MemoryPanel.vue');
  // 页头必须说清「只带相关的几条」：写「相关会话会带上这些内容」时，用户以为库里
  // 所有记忆都进了上下文——而实际是按这句话匹配、有**条数上限**。
  // 但**上限那个数必须来自后端**（`MemoryStatus.max_hits` ← `query.rs::MAX_HITS`）：
  // 前端抄一个数就会在它变的时候静默说谎，而没有任何判据会因此变红——同一个形状刚在
  // 插件的注入预算上发生过一次（3 → 8）。
  assert.match(panel, /injectLimitText/, '条数要读后端下发的值');
  assert.ok(!/最多 6 条/.test(panel), '不许把上限写死在页头');
  // 「固定项**优先**带上」而不是「每次都带」：pinned 只影响排序，它与命中项一起占那几个
  // 名额（pin 超过上限时排最后的那条进不去），项目范围的固定项还只在它自己的工作区生效。
  assert.match(panel, /固定项优先带上/, '固定项是有条件地优先，不是无条件地每次都带');
  assert.match(raw, /injectLimitText = computed/, '那句要真的接上 computed');
  // 开关自己的文字只写了「记忆」，而它同时管自动提取：那句作用域必须留在 tooltip 里。
  assert.match(raw, /这个开关同时管两件事：记忆注入与自动提取/, '开关的作用域要说清');
  // 卡头计数：筛选之后列表短了，而计数还是全库的数——两个数要分开写。
  assert.match(
    panel,
    /\$\{list\.length\} \/ 共 \$\{allMemories\.length\}/,
    '筛选档下计数要同时给「显示几条」与「共几条」'
  );
  // 「已用 N 次」是 `use_count`，只由 `memory_read` 增加（`store.rs::read_and_note_use`），
  // 说「已用」会被读成「被注入过几次」。
  for (const file of ['MemoryRow.vue', 'MemoryDetail.vue']) {
    const tpl = templateOf(read(`../src/memory/${file}`));
    assert.match(tpl, /模型读过/, `${file} 要说清这个次数是谁用的`);
    assert.ok(!tpl.includes('已用'), `${file} 里不该再有「已用 N 次」`);
  }
  // 同一个 `stale` 在两处是**两件事**：候选行那个是「来源那段话被改过」（H7，会话压缩或
  // 编辑之后重提同一段），记忆行那个是「这条事实可能过时」（verification，而候选被批准时
  // 后端会把它重置成 verified）。用同一套词，用户会按错的那个去理解。
  const candidate = templateOf(read('../src/memory/MemoryCandidateRow.vue'));
  assert.match(candidate, /来源已变/, '候选行的标记要说「来源变了」');
  assert.ok(!candidate.includes('可能已过期'), '候选行不许再用「已过期」那套词');
  // 固定项是有条件地优先（见上面那条），编辑器里也一样。
  assert.match(
    templateOf(read('../src/memory/MemoryEditor.vue')),
    /固定项（优先带上）/,
    '编辑器里的固定项也要说「优先」'
  );
  // **方位引用会随布局过期**：「权威记录是上面那份文件」在文件行挪到下面那张卡之后就错了。
  // 这一格现在点名说清是哪份文件（磁盘上的 memory.json），不再指方位。
  assert.ok(!panel.includes('上面那份文件'), '权威记录的指引不许指方位');
  assert.match(panel, /磁盘上的 memory\.json/, '要说清权威记录到底是哪份文件');
  // 卡头计数**含停用条目**（`memory_list` 带 `include_disabled`），不写出来会被读成
  // 「启用了 N 条」——而右栏那行才是分开数的。
  assert.match(panel, /含停用/, '总数里有停用条目时要说出来');
  // 「未知项目」把「没记下来」说成「项目不存在」；那是两个不同的意思。
  assert.match(panel, /未记录项目/, '没记下工作区时说「未记录」，不说「未知」');
  assert.ok(!panel.includes('未知项目'), '「未知项目」这个词不许再出现');
  // 路径缺失时那一格显示的是**项目名**，只写范围名会让同一位置出现两种语义层级。
  for (const file of ['MemoryRow.vue', 'MemoryDetail.vue']) {
    assert.match(
      templateOf(read(`../src/memory/${file}`)) + read(`../src/memory/${file}`),
      /项目记忆（未记路径）/,
      `${file} 的兜底要说清缺的是路径`
    );
  }
});

test('范围文案统一成「项目记忆」——不再与筛选那一档的「按项目」撞名', async () => {
  const { scopeText } = await import('../src/memory/memory.js');
  assert.equal(scopeText({ scope: 'general' }), '通用偏好（所有项目）');
  assert.equal(scopeText({ scope: 'project', workspace_path: '/a/b' }), '项目记忆 · /a/b');
  assert.equal(scopeText({ scope: 'project' }), '项目记忆');
  // 编辑器那个单选也必须是同一个名字：它旁边的下拉让用户任选工作区，
  // 选中的未必是「当前项目」，而列表筛选那档叫「按项目」。
  const editor = templateOf(read('../src/memory/MemoryEditor.vue'));
  assert.match(editor, /<el-radio value="project">项目记忆<\/el-radio>/);
  assert.ok(!editor.includes('>本项目<'), '编辑器里不许再叫「本项目」');
});

test('候选行的来源会话给短 id，完整 id 进 title（设计 §2 要追得回，行里也要读得下去）', async () => {
  const { sessionText } = await import('../src/memory/memory.js');
  assert.equal(sessionText('session-fbb5df89-a57a-4f93-a645-8bc184dfa4b4'), '会话 fbb5df89');
  assert.equal(sessionText(''), '本机', '没有会话 id 的是本机提出来的');
  assert.equal(sessionText(null), '本机');
  const tpl = templateOf(read('../src/memory/MemoryCandidateRow.vue'));
  assert.match(tpl, /sessionText\(item\.source\.session\)/, '行里要用短 id');
  assert.match(tpl, /:title="item\.source\.session/, '完整 id 要留在 title 里');
  // 2026-10-19：详情弹窗反过来要给**完整** id——行里压短是为了读得下去，那一层是
  // 追查用的（「这条到底是哪一次会话提的」），再压一次就永远查不着了。
  const detail = templateOf(read('../src/memory/MemoryCandidateDetail.vue'));
  assert.match(
    detail,
    /\{\{ item\.source\.session \|\| '（本机，这一条没有会话 id）' \}\}/,
    '详情要把完整会话 id 摆出来（没有会话的如实说本机）'
  );
  assert.ok(!detail.includes('sessionText('), '详情不该再把 id 压成短的那个');
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
  // **这个数从后端的源码里读出来比**，不再抄一个 8192：抄下来的数在后端改上限时会静默
  // 说谎——面板照旧按旧值拦（用户以为能存下）或照旧放行（保存时才被拒），而判据全绿。
  // 用户 2026-10-18 定的标准：「不能展示过时、老旧的信息」。
  const rust = read('../../src-tauri/src/memory/store.rs');
  const found = /pub const MAX_BODY_BYTES: usize = ([0-9_*\s]+);/.exec(rust);
  assert.ok(found, '没在 store.rs 里找到 MAX_BODY_BYTES——常量改名或搬家时这条判据要跟着改');
  const backend = found[1]
    .split('*')
    .reduce((acc, part) => acc * Number(part.trim().replaceAll('_', '')), 1);
  assert.equal(MAX_BODY_BYTES, backend, '面板与后端的正文上限必须相等');
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

/**
 * 「检索预览」的查询**不能挂在草稿上**（2026-10-16 评审第 1 条）。
 *
 * 两个事实合起来让挂在草稿上的查询恒为空：编辑器是 modal 弹窗（遮罩挡着面板上那枚
 * 「试一次」），而弹窗一关 `closeMemoryEditor` 就把草稿清了。于是用户能点到它的时候，
 * 查询必然是空串，而后端对空查询是「所有在范围内的条目 0 分」（`query.rs` 的 `score`
 * 不返回 None）——预览会报出一批与用户输入毫无关系的命中，还长得很像真的。
 */
test('检索预览的查询存在 store 里：关掉编辑器之后仍然发得出上一次的标题与关键词', async () => {
  const { memoryStore, editMemory, closeMemoryEditor, openMemoryEditor, previewContext } =
    await import('../src/memory/memory.js');
  const core = globalThis.window.__TAURI__.core;
  const original = core.invoke;
  try {
    memoryStore.workspaces = [];
    // 编辑一条既有记忆：查询跟着它走。
    editMemory({
      id: 'rec-1',
      revision: 1,
      scope: 'general',
      title: '提交前先跑检查',
      body: 'b',
      keywords: ['npm run check'],
      pinned: false,
      verification: 'verified',
    });
    // **关掉编辑器**（这就是用户点「试一次」之前必然发生的那一步）：草稿被丢，但查询要留下。
    closeMemoryEditor();
    assert.equal(memoryStore.draft.title, '', '前提：关窗确实丢草稿');
    assert.equal(memoryStore.previewQuery, '提交前先跑检查 npm run check', '查询不许跟着草稿一起没');

    const calls = [];
    core.invoke = (cmd, args) => {
      calls.push([cmd, args]);
      return Promise.resolve({ enabled: true, text: '', total_bytes: 0, hits: [] });
    };
    await previewContext().catch(() => undefined);
    assert.equal(calls.length, 1, '要真的发一次预览请求');
    assert.equal(calls[0][0], 'memory_preview_context');
    assert.equal(calls[0][1].queryText, '提交前先跑检查 npm run check', '发出去的查询不能是空串');

    // 新建：查询跟着清空，免得把上一条的标题当成新条目的查询。
    openMemoryEditor();
    assert.equal(memoryStore.previewQuery, '', '新建要先清掉上一条的查询');

    // 面板那枚按钮在没有查询时是禁用的——点了等于没点。
    const panel = templateOf(read(`${MEM_DIR}MemoryPanel.vue`));
    assert.match(
      panel,
      /:disabled="globalBusy \|\| !memoryStore\.previewQuery"/,
      '没有查询时「试一次」必须禁用（否则按钮点下去会报出一批与输入无关的命中）',
    );
    assert.match(panel, /先在编辑器里填标题或关键词/, '空查询那一路要给一句可执行的下一步');
  } finally {
    core.invoke = original;
  }
});

/**
 * 编辑器关窗前的闸门，以及保存期间那条闸门用哪个信号（2026-10-16 评审第 2、3 条）。
 *
 * 遮罩与 ESC 已经由 prop 拦住，剩下 ✕ 是**明确**的点击——但「明确」说的是意图，不是
 * 「他愿意丢掉刚写的三段话」。另一条：`globalBusy` 只由 `withExclusive` 抬，而保存走的是
 * `withLoading('memorySave')`，保存期间它恒为 false，拿它当闸门等于没闸门。
 *
 * 这两条都在 `<script setup>` 里，node 跑不到——所以照本仓的规矩判**接线形状**，
 * 行为那半留在真机 e2e（`e2e/specs/memory.spec.mjs`）。
 */
test('编辑器关窗前先问一句；保存期间用真的保存信号当闸门', () => {
  const editor = read(`${MEM_DIR}MemoryEditor.vue`);
  assert.match(editor, /:before-close="beforeClose"/, '✕ 必须走 before-close 这道闸门');
  assert.match(
    editor,
    /function beforeClose\(\)[\s\S]*confirmDialog\(`放弃这条\$\{kindName\.value\}？`/,
    '有内容就先确认再关（直接关等于把刚写的正文拿走）',
  );
  assert.match(editor, /if \(ok\) closeMemoryEditor\(\)/, '确认框说「放弃」才真的关');
  assert.match(editor, /还没有保存|还没保存/, '确认文案要说清关掉会丢什么');
  assert.ok(!/@update:model-value="dismiss"/.test(editor), '别退回 update:model-value：EP 是先关再通知，拦不住');
  assert.doesNotMatch(editor, /el-button :disabled="globalBusy"/, '关窗那两条不许再拿 globalBusy 当闸门（保存期间它是 false）');
  assert.match(
    editor,
    /const saving = computed\(\(\) => isLoading\('memorySave'\)\)/,
    '闸门信号要取自 memorySave 这个 loading key',
  );
  assert.match(editor, /:disabled="saving" @click="closeMemoryEditor"/, '保存进行中「取消」不可点');
  assert.match(editor, /:show-close="!saving"/, '保存进行中不画 ✕');
});

/**
 * 弹窗的可见性只有一个真值：`memoryStore.editorVisible`。
 *
 * 用户 2026-10-16 报的「切走功能菜单，它又自己弹出来」是**两个写者**：EP 的 `done()` 只把
 * 它内部那份 `visible` 置 false，store 里的 `editorVisible` 留在 true，而 `App.vue` 用
 * `<component :key="activePanel">` 换面板——切回记忆页重新挂载时 EP 见 `modelValue` 还是
 * true，就在 `onMounted` 里自己 `open()` 一次。于是关掉的弹窗跟着菜单切换回来了。
 *
 * 两条判据各自堵一半，都是**形状**判据：`<script setup>` 跑不到，行为那半留在真机。
 */
test('关窗与卸载都要把 editorVisible 收回去（切走菜单再回来不许自己弹）', () => {
  const editor = read(`${MEM_DIR}MemoryEditor.vue`);
  const start = editor.indexOf('function beforeClose');
  assert.ok(start > 0, '✕ 的闸门还在（`before-close` 那条接线在上一组判据里）');
  const gate = editor.slice(start, editor.indexOf('\n}\n', start));
  assert.equal(
    (gate.match(/closeMemoryEditor\(\)/g) ?? []).length,
    2,
    '空表单与「放弃」两条出口都要经过 closeMemoryEditor，否则 store 标记留在 true',
  );
  assert.doesNotMatch(gate, /\bdone\b/, '不许再接 EP 递进来的 done：它只关 EP 自己那份 visible，两个写者就此分家');
  assert.match(
    editor,
    /import \{[^}]*\bonUnmounted\b[^}]*\} from 'vue'/,
    '卸载钩子要真的从 vue 引进来（漏了是 setup 期 ReferenceError，整页白）',
  );
  assert.match(
    editor,
    /onUnmounted\([\s\S]{0,40}?if \(memoryStore\.editorVisible\) closeMemoryEditor\(\)/,
    '卸载那一刻只在开着的时候收回 editorVisible（面板被换掉时 store 不会自己回 false）',
  );
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
  // 2026-10-17 起候选行也在这一族里（`MemoryCandidateRow.vue`）：三个模板都要数，
  // 少扫一个就等于漏掉它那几条样式是被谁用的。2026-10-19 再加候选详情——它同样写这
  // 一族类名（正文不截断），新长出一个类名时下面的清单会立刻对不上。
  const candidate = templateOf(read(`${MEM_DIR}MemoryCandidateRow.vue`));
  const candidateDetail = templateOf(read(`${MEM_DIR}MemoryCandidateDetail.vue`));

  // 类名清单**从模板里读**，不写死在判据里：日后谁把 `memory-row__body` 改名而忘了搬
  // 样式，下面的循环会当成「这一族已经不这么叫了」而静默空转。
  const used = [
    ...new Set([
      ...literalClasses(panel),
      ...literalClasses(row),
      ...literalClasses(candidate),
      ...literalClasses(candidateDetail),
    ]),
  ]
    .filter((c) => c.startsWith('memory-row'))
    .sort();
  assert.deepEqual(
    used,
    [
      'memory-row',
      // 2026-10-17 新增三条：`--clickable`（行的根，整块可点）、`--candidate`（候选行的
      // 动作排沉到行底、占满整行）与 `__body--clamp`（列表里只留两行正文）。三条都写在
      // `memory.css` 里——它们被多个组件的模板共用（面板 / 行 / 候选行 / 详情），scoped
      // 块跨不过去。数组按排序顺序写（`-` 在 `_` 前面）。
      'memory-row--candidate',
      'memory-row--clickable',
      'memory-row__actions',
      'memory-row__body',
      'memory-row__body--clamp',
      'memory-row__main',
      'memory-row__meta',
      'memory-row__name',
      'memory-row__title',
    ],
    '这一族类名的清单变了：下面按类名查生效值的循环会静默空转',
  );

  // 根行还得同时挂 `page-row`：行盒、行间分隔线与内边距全来自它，而它是全局原语——
  // 漏挂不会有任何报错，只是两张列表里的行长得不一样。**面板自己不再写行模板**
  // （2026-10-17 起两个行族各在自己的组件里：`MemoryRow` / `MemoryCandidateRow`）。
  for (const [name, tpl] of [
    ['MemoryRow.vue', row],
    ['MemoryCandidateRow.vue', candidate],
  ]) {
    assert.ok(literalClasses(tpl).includes('page-row'), `${name} 的根行漏了 page-row`);
  }

  const expected = {
    // 2026-10-16：`.memory-row` 从 `flex` 改成 `grid` + `subgrid`，让动作区只占标题那一行、
    // 正文横跨整行（用户要求「新增的记忆显示UI横向展开完全」）。这组值跟着实现走——本条
    // 判据真正钉的是「这一族的规则写在 import 进 main.js 的 .css 里」，不是这几个字面量。
    'memory-row': {
      display: 'grid',
      'align-items': 'flex-start',
      'grid-template-columns': 'minmax(0, 1fr) auto',
      // 2026-10-17 收紧：`10px 12px` → `5px 12px`。条目本身三行文字（标题 / 两行正文 /
      // 次行），行盒的上下留白是纯支出——一条记忆少 10px，三条就少一整行的位置。
      padding: '5px 12px',
    },
    // 候选行：动作排沉到行底，所以父网格只剩一列（否则标题列被动作挤到 ~190px，
    // 「已安装 cargo-cache 0.8.3」这种标题折成三行）。动作那半是后代选择器，
    // 层叠工具是祖先盲的，下面按规则原文单独查。
    'memory-row--candidate': { 'grid-template-columns': 'minmax(0, 1fr)' },
    'memory-row__main': {
      display: 'grid',
      'grid-template-rows': 'subgrid',
      // 2026-10-17：`__main` 跨两列**且**自己带 `subgrid` 列，两条缺一不可。只写
      // `1 / -1` 而漏掉列定义时，它内部只剩一条隐式列——`__title` 的 `grid-column: 1`
      // 落在那一列上、宽度等于整行，与 `__actions` 叠进同一格，按钮压住标题（用户第一次
      // 截图）；改回 `grid-column: 1` 能治压字，却把 `__body` / `__meta` 一起关进第 1 列，
      // 正文只占左半、右边全空（用户第二次截图）。实测两处症状都在，见下方 `memory-flag`。
      'grid-column': '1 / -1',
      'grid-template-columns': 'subgrid',
      'min-width': '0',
    },
    // 2026-10-17：行整块可点（点开详情弹窗）。`cursor: pointer` 必须与 hover / 焦点反馈
    // 成对出现——`diagnostics.css` 的 `.diag-row--static` 那条注释写过同一件事：只给指针
    // 不给响应，用户会以为点了没反应。两个状态值在下面单独查（伪类不进这张表）。
    'memory-row--clickable': { cursor: 'pointer' },
    'memory-row__title': { display: 'flex', 'flex-wrap': 'wrap', 'align-items': 'center' },
    'memory-row__name': { 'font-size': '15px', 'font-weight': '600' },
    'memory-row__body': {
      'white-space': 'pre-wrap',
      'overflow-wrap': 'anywhere',
      'grid-column': '1 / -1',
      // 2026-10-17：4px → 3px（同一轮把行盒整体压紧；正文与次行都是紧随上一行的
      // 说明性文字，3px 已经够分开）。
      margin: '3px 0 0',
      color: 'var(--text-secondary)',
    },
    // 2026-10-17：列表里那一行正文只留两行（用户要求「默认内容精简收缩」），全文进详情
    // 弹窗。行数截断靠 `display: -webkit-box` + `overflow: hidden` 这一对，缺哪个都只会
    // 把文字裁在半个字上。`-webkit-line-clamp` 与标准的 `line-clamp` 都写：后者是新的
    // 标准名，`css-cascade` 的属性正则有前置断言，两条各自查得到（见 `ui/AGENTS.md`
    // 的层叠工具说明）。
    'memory-row__body--clamp': {
      display: '-webkit-box',
      '-webkit-box-orient': 'vertical',
      '-webkit-line-clamp': '2',
      'line-clamp': '2',
      overflow: 'hidden',
    },
    'memory-row__meta': {
      'font-size': '12px',
      color: 'var(--text-muted)',
      'grid-column': '1 / -1',
    },
    'memory-row__actions': {
      display: 'flex',
      'align-self': 'start',
    },
    // 这一族不在上面的类名清单里（它不属于 `memory-row*`），但同样只能写在全局 `.css`：
    // 三个关系块的模板根都是 `.memory-flag`，其中两个还是各自组件的根元素。它是 `__main`
    // 里的整行块，与 `__body` / `__meta` 同一条理由——不写这一条就只占第 1 列，被挤成
    // 左边一条、右边空着（同一张截图里的黄框）。
    'memory-flag': { 'grid-column': '1 / -1' },
  };

  // 只传**类名自己**当主语 token：多传一个 `page-row` 会让 `.page-row` 那条 (0,1,0)
  // 的同特异度规则一起参与比较，读出来的就不是这一族自己的声明了。
  const rules = globalRules().filter((rule) => !rule.selector.includes('.memory-dialog'));
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

  // 候选行动作排那条（后代选择器，层叠工具读不准）：grid-column / grid-row 与
  // `justify-content` 三件事缺一不可——少 `grid-column: 1` 它会被自动放进第 2 列
  // （父网格只有一列时又回到「挤在标题右侧」），少 `flex-end` 则三枚按钮贴左。
  // **`grid-column` 这一条不能走 `effectiveDeclaration`**：候选行那条后代规则
  // （`.memory-row--candidate .memory-row__actions`）与基线同主语，而层叠工具是祖先盲的，
  // 于是它把候选那条的 `grid-column: 1` 当成基线生效值（实测红在「生效值是 1，期望 2」）。
  // 这正是 `ui/AGENTS.md` 那句「主语类被多条后代规则复用时改读规则文本」——按规则原文查。
  const baseActions = rules.find((r) => r.selector === '.memory-row__actions');
  assert.ok(baseActions, '找不到 .memory-row__actions 的基线规则');
  for (const decl of ['grid-column: 2', 'grid-row: 1']) {
    assert.ok(baseActions.body.includes(decl), `记忆行的动作区少了基线「${decl}」`);
  }
  const candActions = rules.find((r) => r.selector === '.memory-row--candidate .memory-row__actions');
  assert.ok(candActions, '候选行的动作排没有自己的规则');
  for (const decl of ['grid-column: 1', 'grid-row: 4', 'justify-content: flex-end']) {
    assert.ok(candActions.body.includes(decl), `候选行的动作排少了「${decl}」`);
  }

  // 两条状态值单独查：伪类不进上面那张表（表里走的是「不给 pseudo 就不匹配带伪类的
  // 规则」这条口径），而它们恰恰是「整块可点」的另一半——只有 `cursor: pointer` 的话，
  // 鼠标压上去什么都不变，`.diag-row--static` 那条注释已经把这个坑写明白了。
  const hoverBg = effectiveDeclaration(['memory-row--clickable'], rules, 'background', {
    pseudo: 'hover',
  });
  assert.equal(hoverBg, 'var(--overlay-soft)', '可点的记忆行悬停时要有底色反馈');
  const focusRing = effectiveDeclaration(['memory-row--clickable'], rules, 'outline', {
    pseudo: 'focus-visible',
  });
  assert.equal(
    focusRing,
    '1px solid var(--accent)',
    '可点的记忆行要能用键盘聚焦——行是 div，不给 outline 就完全没有焦点圈',
  );
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

test('记忆行是精简卡片、点开才出操作：动作按钮是弹窗底部的徽章', () => {
  const panelSrc = read(`${MEM_DIR}MemoryPanel.vue`);
  const panel = templateOf(panelSrc);
  const row = templateOf(read(`${MEM_DIR}MemoryRow.vue`));
  const detailSrc = read(`${MEM_DIR}MemoryDetail.vue`);
  const detail = templateOf(detailSrc);
  const rules = globalRules();

  // ① 「默认精简」：行里只剩标题、两行正文与元信息，一枚动作按钮都没有——此前正文整段
  //    摊开、右侧常驻三个文字动作，一条记忆能占六行高。
  assert.equal((row.match(/<el-button[\s>]/g) || []).length, 0, '记忆行里又长出了按钮');
  for (const handler of ['@edit', '@toggle', '@remove']) {
    assert.ok(!row.includes(handler), `记忆行还在自己处理 ${handler}：动作已经搬进详情弹窗`);
  }
  assert.match(
    row,
    /class="memory-row__body memory-row__body--clamp"/,
    '行的正文没有压缩：两行截断只有 __body--clamp 给',
  );

  // ② 整块可点。行是 div 不是原生 <button>：这一族的版式压在 `.memory-row` 的栅格与
  //    `__main` 子栅格上（正文被挤成窄条、候选行按钮压住标题，两次都出在这里），换标签
  //    就得连版式一起重做，于是用 `role="button"` + `tabindex` 补上读屏与键盘那半边。
  for (const attr of ['role="button"', 'tabindex="0"', 'aria-haspopup="dialog"']) {
    assert.equal(row.split(attr).length - 1, 1, `记忆行少了 ${attr}`);
  }
  assert.equal((row.match(/@click="emit\('open', row\)"/g) || []).length, 1, '点整行不开详情');
  assert.equal(
    (row.match(/@keydown\.enter="emit\('open', row\)"/g) || []).length,
    1,
    '回车打不开详情',
  );
  // 空格必须 `.prevent`：不拦的话按键会先把列表滚一屏（这一屏的滚动容器就是列表自己）。
  assert.equal(
    (row.match(/@keydown\.space\.prevent="emit\('open', row\)"/g) || []).length,
    1,
    '空格没接，或接了但没拦住默认滚动',
  );

  // ③ 「列表内部滚动」：主体连 2026-10-17 起钉在窗口高度内（页面级滚动条不再出现），
  //    长出来的那一截只能落在 `memory-list` 上。两个列表容器**共用**这个类名（已保存
  //    记忆与待确认候选），所以这里按「装 MemoryRow 的那一个」判，而不是 `elementBlock`
  //    抓到的第一个——第一个现在是候选列表。
  assert.match(
    panel,
    /<div v-else class="page-list memory-list">\s*<MemoryRow/,
    '装 MemoryRow 的容器丢了 memory-list（长出来的那段只能在这里自己滚）',
  );
  assert.equal((panel.match(/@open="openMemoryDetail"/g) || []).length, 1, '行没把 @open 接出来');
  assert.match(
    panel,
    /v-else-if="memoryStore\.listScope === 'candidates'"\s+class="page-list memory-list"\s*>\s*<MemoryCandidateRow/,
    '候选那一档没有复用同一个列表容器：它也得能自己滚',
  );
  // 高度不再写死 320px：列表吃卡片的剩余高度（`flex: 1 1 auto` + `min-height: 0`），
  // 上限由窗口给。写死上限会在卡片里留一块空白，条目一多还与卡面各算各的。
  assert.equal(effectiveDeclaration(['memory-list'], rules, 'overflow-y'), 'auto', '列表不自己滚');
  assert.equal(effectiveDeclaration(['memory-list'], rules, 'flex'), '1 1 auto', '列表没吃剩余高度');
  assert.equal(
    effectiveDeclaration(['memory-list'], rules, 'min-height'),
    '0',
    'flex 子项的下限不给 0，列表会撑回内容高度、整页又长出去',
  );
  assert.equal(effectiveDeclaration(['memory-list'], rules, 'max-height'), null, '还在写死高度上限');
  // 页面级那一半（scoped 规则的祖先条件 layer 工具读不准，按本仓规矩读剥注释后的文本）：
  // 高度链每一级都要在——`.panel` 拿满 main 的内容高度、栅格两列 stretch、卡片允许被压到
  // 内容高度以下。少一级，页面级滚动条就回来了。
  const scoped = panelSrc.slice(panelSrc.indexOf('<style scoped>')).replace(/\/\*[\s\S]*?\*\//g, '');
  for (const rule of [
    [/\.panel\s*\{[^}]*height:\s*100%/, '.panel 没有拿满 main 的高度'],
    [/\.page-layout\s*\{[^}]*flex:\s*1 1 auto/, '栅格不在高度链上'],
    [/\.page-layout\s*\{[^}]*min-height:\s*0/, '栅格的下限没给 0'],
    [/\.memory-library\s*\{[^}]*min-height:\s*0/, '卡片的下限没给 0，压不下去'],
  ]) {
    assert.match(scoped, rule[0], rule[1]);
  }

  // ④ 弹窗里那一行的来源：按 id 从**未筛选**的 allMemories 里现取。改成挂 `list` 里那一份
  //    的话，读记忆时切一下范围（那组单选就在同屏）弹窗就跟着没了。
  assert.equal(
    panelSrc.split(
      'const detailRow = computed(() => allRecords.value.find((row) => row.id === detailId.value) || null);',
    ).length - 1,
    1,
    '详情弹窗的行没有按 id 从 allMemories（未筛选）里现取',
  );

  // ⑤ 挂载接线挂在 `v-if="detailRow"` 上，而不是自己记「开没开」——记忆被删掉时那个
  //    computed 变 null，弹窗自己关掉，不必再同步一次开关状态。
  const at = panelSrc.indexOf('<MemoryDetail');
  assert.ok(at > 0, '面板里没有挂载记忆详情弹窗');
  const mount = panelSrc.slice(at, panelSrc.indexOf('/>', at) + 2);
  for (const binding of [
    'v-if="detailRow && detailRow.kind !== \'handoff\'"',
    ':row="detailRow"',
    '@close="detailId = null"',
    '@edit="editFromDetail"',
  ]) {
    assert.equal(mount.split(binding).length - 1, 1, `详情弹窗没接 ${binding}`);
  }
  assert.match(
    mount,
    /@toggle="\(state\) => setMemoryState\(detailRow, state\)"/,
    '详情里的启停没接回 setMemoryState',
  );
  assert.match(mount, /@remove="removeMemory"/, '详情里的删除没接回 removeMemory');

  // ⑥ 弹窗页脚。`memory-actions` 不能省：`.dialog-actions` 只给 `justify-content`，
  //    缝与换行都在 `.memory-actions` 上（同 `MigrationPrompt` 的 `.prompt-footer-ask`）。
  assert.match(detail, /<p class="memory-row__body">/, '详情正文丢了 memory-row__body');
  assert.ok(
    !detail.includes('memory-row__body--clamp'),
    '详情里还在截正文：这一层就是拿来看完整正文的',
  );

  const footAt = detail.indexOf('<template #footer>');
  assert.ok(footAt > 0, '详情弹窗没有页脚');
  const foot = detail.slice(footAt);
  assert.equal(
    (foot.match(/class="memory-actions dialog-actions"/g) || []).length,
    1,
    '动作排的包裹层不是 memory-actions + dialog-actions',
  );
  assert.equal((foot.match(/<el-button\b/g) || []).length, 3, '页脚不是三枚按钮');
  for (const label of ['编辑', '删除']) {
    assert.equal(foot.split(label).length - 1, 1, `页脚里的「${label}」不是恰好一枚`);
  }
  assert.match(foot, /\{\{ accepted \? '停用' : '启用' \}\}/, '启停那枚的文案丢了');
  assert.equal(
    (foot.match(/@click="emit\('toggle', accepted \? 'disabled' : 'accepted'\)"/g) || []).length,
    1,
    '启停点下去的落点不对：停用要写 disabled、启用要写 accepted',
  );
  assert.equal(
    (foot.match(/:disabled="globalBusy"/g) || []).length,
    3,
    '长任务在跑时三枚按钮都要禁用',
  );
  for (const key of ["isLoading('memoryState:' + row.id)", "isLoading('memoryDelete:' + row.id)"]) {
    assert.equal(foot.split(key).length - 1, 1, `页脚丢了在途状态 ${key}`);
  }
  assert.equal(
    (detailSrc.match(/const accepted = computed\(\(\) => props\.row\.state === 'accepted'\);/g) || [])
      .length,
    1,
    '启停按钮判不出该显示「停用」还是「启用」',
  );

  // ⑦ 徽章样式。`margin-left` 与 hover 两条**只读规则原文**，不走 effectiveDeclaration：
  //    那个工具的选择器匹配是「祖先盲」的，`.installed-tabs-wrap .tabs-head-actions
  //    .el-button + .el-button`（margin-left: 6px，(0,4,0)）与 `.panel .el-button` 都会被
  //    算成这条徽章的竞争者，可它们都到不了挂在 body 上的弹窗——照它读，答案是 6px。
  //    这条规则自己那个 `margin-left: 0` 抵消的正是 Element Plus 加在每颗按钮上的 12px。
  const badge = rules.find((r) => r.selector === '.memory-actions .el-button.memory-badge');
  assert.ok(badge, '找不到徽章按钮那条规则');
  for (const decl of [
    'margin-left: 0',
    'height: auto',
    'padding: 2px 10px',
    'border-radius: 999px',
    'border: 1px solid var(--accent-line)',
    'background: var(--accent-fill)',
    'color: var(--accent)',
  ]) {
    assert.ok(badge.body.includes(decl), `徽章少了「${decl}」`);
  }
  // 其余属性上扫不到能压过它的竞争者（`.kv .el-button` 那条是 (0,2,0)），所以这几条照
  // 层叠读：真被别的规则改写时判据会红，而不是只证明「某处写过」。
  assert.equal(
    effectiveDeclaration(['el-button', 'memory-badge'], rules, 'border-radius'),
    '999px',
    '徽章不是胶囊形',
  );
  assert.equal(
    effectiveDeclaration(['el-button', 'memory-badge'], rules, 'height'),
    'auto',
    '徽章被 Element Plus 的 32px 撑回了整颗按钮：高度不归零就不像徽章',
  );
  assert.equal(
    effectiveDeclaration(['el-button', 'memory-badge'], rules, 'color'),
    'var(--accent)',
    '徽章的主色不是强调色',
  );
  // 状态色靠变体类，不是换 `el-button` 的 type/plain/round：那几档各带一整套背景与边框，
  // 颜色改不动。（`.diag-row__cta` 正是因此用裸 span，这里要留下按钮的 loading 与焦点圈。）
  assert.equal(
    effectiveDeclaration(['el-button', 'memory-badge', 'memory-badge--warn'], rules, 'color'),
    'var(--warning)',
    '启停那枚不是警告色',
  );
  assert.equal(
    effectiveDeclaration(['el-button', 'memory-badge', 'memory-badge--danger'], rules, 'color'),
    'var(--danger)',
    '删除那枚不是危险色',
  );
  // 禁用态：徽章自己的 (0,3,0) 主色规则压过 Element Plus 的 `.el-button.is-disabled`
  // （(0,2,0)，样式表在 node_modules 里），颜色不会自己变灰——不另给信号就成了「看着能点、
  // 点了没反应」，`.diag-row--static` 那条注释记的就是这个坑。
  assert.equal(
    effectiveDeclaration(['el-button', 'memory-badge', 'is-disabled'], rules, 'opacity'),
    '0.5',
    '禁用态没有任何视觉差别',
  );
  const badgeHover = rules.find(
    (r) =>
      r.selector.includes('memory-badge:not(.is-disabled):hover') &&
      r.body.includes('border-color'),
  );
  assert.ok(badgeHover, '徽章没有 hover / focus 反馈：`.diag-row--static` 的注释写明了这个坑');
  assert.ok(
    badgeHover.selector.includes('memory-badge:not(.is-disabled):focus-visible'),
    '键盘聚焦没和悬停共用一条规则：漏掉 focus-visible，键盘用户看不到焦点',
  );
  assert.ok(
    badgeHover.body.includes('border-color: currentcolor'),
    ':not(.is-disabled) 去掉的话，禁用中的徽章也会在悬停时加亮边框',
  );

  // 缝由容器给：三枚徽章自己不带 margin（上面那条 `margin-left: 0` 是这条的一半），
  // 换行交给 flex-wrap——弹窗宽 `min(560px, 92vw)`，窄屏上三枚挤不进一行。
  assert.equal(effectiveDeclaration(['memory-actions'], rules, 'gap'), '8px', '动作排没有缝');
  assert.equal(
    effectiveDeclaration(['memory-actions'], rules, 'display'),
    'flex',
    '`.dialog-actions` 只给 justify-content，display 与 wrap 得自己给',
  );
  assert.equal(
    effectiveDeclaration(['memory-actions', 'dialog-actions'], rules, 'justify-content'),
    'flex-end',
    '动作排没靠右：`.dialog-actions` 是那一排位置唯一的来源',
  );
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
  // 2026-10-17 起是**四档**：三档范围 + 「待确认（N）」那一档视图。候选从另一张卡并进
  // 同一条列表视图，为的是消掉页面级纵向滚动条；数量写进标签，队列不是隐形的。
  assert.equal((group.match(/<el-radio[\s>]/g) || []).length, 6, '筛选需包含记忆三档、交接、经验与待确认');
  assert.match(
    group,
    /<el-radio\s+v-if="candidates\.length \|\| memoryStore\.listScope === 'candidates'"\s+value="candidates"/,
    '「待确认」那一档丢了，或没在「没有候选」时不画它（已在那一档时例外，否则切不回来）',
  );
  assert.match(group, /待确认（\{\{ candidates\.length \}\}）/, '候选数量要写在标签里');
  // 空态那三句各挂在 `v-if` / `v-else-if` / `v-else` 上，与筛选互不干扰。
  // `memory-list` 加在同一格上：`page-list` 给的是栅格与行列缝，内部滚动那半只有
  // `memory-list` 有（2026-10-17 起，这一屏唯一的滚动容器）。
  assert.match(
    card,
    /<div v-else class="page-list memory-list">/,
    '空态与列表的分支没了，或列表格丢了 memory-list（这一屏没有页面级滚动条，长出来的那段只能在这里自己滚）',
  );
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
  const panelSrc = read(`${MEM_DIR}MemoryPanel.vue`);
  const panel = templateOf(panelSrc);
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
    /<el-button[^>]*type="primary" :icon="Plus"[^>]*@click="openMemoryEditor\(\)"/,
    '「新建记忆」要接在 openMemoryEditor() 上（少一对括号就会把 MouseEvent 当 record）',
  );
  // 行内「编辑」在 2026-10-17 之后从详情弹窗转一手（列表行不再带按钮），但入口仍是
  // 同一枚 `openMemoryEditor`：转手那一步必须先把详情关掉，两个弹窗都 `append-to-body`，
  // 叠着开就是两层遮罩（`@edit="openMemoryEditor"` 直接挂下去正是这么坏的）。
  assert.match(panel, /@edit="editFromDetail"/, '详情里的「编辑」要接上转手那一步');
  assert.match(
    panelSrc,
    /function editFromDetail\(row\) \{\s*detailId\.value = null;\s*openMemoryEditor\(row\);/,
    '转手函数要关详情、再开编辑器，且用的是同一个入口',
  );

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
test('窄列里的路径只显示最后一段，完整路径进 title（压的是显示，不是信息）', () => {
  // 2026-10-17：三条路径都在窄列里，整条路径会把行顶宽、把次行折成两行（每条记忆多 18px）。
  // 这一组是**决策文档写了的**（panel-decisions「次行的项目名只留最后一段」），所以判据要跟上：
  // 只改文档不改判据，下一次重构把 `baseName` 摘掉时没人会红。
  // `projectName` / `scopeTitle` 是 `<script setup>` 里的 computed，`templateOf()` 只给模板
  // —— 这一条读整份文件。
  for (const file of ['MemoryRow.vue', 'MemoryDetail.vue']) {
    const src = read(`../src/memory/${file}`);
    assert.match(src, /baseName\(props\.row\.workspace_path\)/, `${file} 的项目名没折成最后一段`);
    assert.match(src, /:title="scopeTitle"/, `${file} 的完整路径没进 title，一个字不许丢`);
    assert.match(
      templateOf(src),
      /\{\{ row\.scope === 'general' \? '通用偏好' : projectName \}\}/,
      `${file} 的标签还在写「本项目」这类通用词——要写具体项目名`,
    );
  }
  // 2026-10-19：候选详情是同一件事的第三处（候选也有范围标签，而它同样在弹窗里）。
  // 它读的是 `props.item`，所以单独列——同一屏里同一个东西只能有一个名字。
  const candidateDetailSrc = read('../src/memory/MemoryCandidateDetail.vue');
  assert.match(
    candidateDetailSrc,
    /baseName\(props\.item\.workspace_path\)/,
    '候选详情的项目名没折成最后一段',
  );
  assert.match(candidateDetailSrc, /:title="scopeTitle"/, '候选详情的完整路径没进 title');
  assert.match(
    templateOf(candidateDetailSrc),
    /\{\{ item\.scope === 'general' \? '通用偏好' : projectName \}\}/,
    '候选详情的范围标签没写具体项目名',
  );
  // 次行不再重复标签已经说过的范围，只留来源 / 验证 / 时间 / 次数。
  assert.doesNotMatch(
    templateOf(read('../src/memory/MemoryRow.vue')),
    /\{\{ place \}\}/,
    '次行又拿范围前缀占宽度了：项目名已经在标签上',
  );

  const panel = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.match(panel, /baseName\(status\.file\)/, '「记录文件」没折成最后一段');
  assert.match(panel, /:title="status \? status\.file : ''"/, '记录文件的完整路径要进 title');

  // 候选行的来源标签必须过 `sourceText()` 的中文映射：直接打印后端的 kind，
  // 用户在候选卡上看到的就是一个英文单词 `extract`（用户截图里就是它）。
  const candidate = templateOf(read('../src/memory/MemoryCandidateRow.vue'));
  assert.match(
    candidate,
    /\{\{ sourceText\(item\.source\.kind\) \}\}/,
    '候选行的来源标签没走 sourceText()，屏幕上会露出 extract',
  );
});

test('冲突标记在候选卡片里并排摆出来，动作落在 store 已有命令上', () => {
  // 2026-10-17：候选行整体搬进 `MemoryCandidateRow.vue`，这块标记跟着走。
  const tpl = templateOf(read('../src/memory/MemoryCandidateRow.vue'));
  assert.match(tpl, /<MemoryConflict[\s/>]/, '候选行要真的用上这个组件');
  assert.match(tpl, /:conflicts="item\.conflicts_with \|\| \[\]"/, '冲突 id 要从候选本身读（老数据没有这个键，所以给空数组兜底）');
  assert.match(tpl, /:records="records"/, '要从 prop 读那份「全库记录」');
  // **全库那一份由面板传下去**：传筛选后的列表会解析不到别的项目的记录。候选行与
  // 候选详情各要一份（2026-10-19 起），两处都必须是**未筛选**的那份。
  const panel = templateOf(read('../src/memory/MemoryPanel.vue'));
  assert.equal(
    (panel.match(/:records="allMemories"/g) || []).length,
    2,
    '面板没把手里的全库记录同时传给候选行与候选详情',
  );
  // 两个动作复用面板已有的那两个命令，不新造一条路径。
  assert.match(tpl, /@disable="\(record\) => setMemoryState\(record, 'disabled'\)"/, '停用走已有的 memory_set_state');
  assert.match(tpl, /@remove="removeMemory"/, '删除走已有的 memory_delete（它自带影响范围确认框）');
  // 详情里也要摆出冲突块：行上被压缩的是**显示**，不是信息。
  const detail = templateOf(read('../src/memory/MemoryCandidateDetail.vue'));
  assert.match(detail, /<MemoryConflict[\s/>]/, '候选详情要摆出冲突的那几条');
  assert.match(detail, /:records="records"/, '详情也要拿全库记录解析冲突 id');
  assert.match(
    detail,
    /@disable="\(record\) => setMemoryState\(record, 'disabled'\)"/,
    '详情里的「停用这条」要走同一个命令',
  );
  assert.match(detail, /@remove="removeMemory"/, '详情里的「删除这条」要走同一个命令');
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
  const tpl = templateOf(read('../src/memory/MemoryCandidateRow.vue'));
  // **标签名后面必须是空白、斜杠或尖括号**：只写 `/<MemorySuperseded/` 的话，
  // 把它改成 `<MemorySupersededX` 就照样匹配——反向验（2026-10-16）真的量到了这一条：
  // 判据绿着，而「组件没被用上」这件事已经发生了。C5 那条同款断言一并收紧。
  assert.match(tpl, /<MemorySuperseded[\s/>]/, '候选卡片要真的用上这个组件');
  assert.match(tpl, /:supersedes="item\.supersedes \|\| \[\]"/, '取代 id 要从候选本身读（老数据没有这个键，给空数组兜底）');
  assert.match(tpl, /:candidates="candidates"/, '要拿**待确认候选**那份列表去解析 id（记录列表里没有候选）');
  // 候选行与候选详情各要一份**候选**列表（2026-10-19 起），传错成记忆列表就永远「已不在」。
  assert.equal(
    (templateOf(read('../src/memory/MemoryPanel.vue')).match(/:candidates="candidates"/g) || []).length,
    2,
    '面板没把候选那份列表同时传给候选行与候选详情',
  );
  // 动作复用面板已有的那一个命令，不新造一条路径；`false` 就是「拒绝」。
  assert.match(tpl, /@discard="\(candidate\) => decideCandidate\(candidate, false\)"/, '丢弃走已有的 memory_decide_candidate');
  // 详情里也要摆出取代块，动作落在同一个命令上。
  const detail = templateOf(read('../src/memory/MemoryCandidateDetail.vue'));
  assert.match(detail, /<MemorySuperseded[\s/>]/, '候选详情要摆出被推翻的那几条');
  assert.match(detail, /:candidates="candidates"/, '详情也要拿候选列表解析取代 id');
  assert.match(
    detail,
    /@discard="\(candidate\) => decideCandidate\(candidate, false\)"/,
    '详情里的「丢弃这条」要走同一个命令',
  );
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
  // 2026-10-17：候选行搬进 `MemoryCandidateRow.vue` 之后，这一行本身就是「候选那张
  // 卡片」——不再需要从面板里切区间（上一版那两个锚点已经因为搬卡片而失效一次）。
  const src = read('../src/memory/MemoryCandidateRow.vue');
  const tpl = templateOf(src);
  // **判据只能盯候选这一行**，不能盯整份模板：手工新建记录那组「已过期」单选
  // （`el-radio value="stale"`）也是 `stale`，而它跟「只标不执行」毫无关系。
  // 一开始写成盯整份模板，红在那个正当的用法上——判据越界也是一种判据骗自己。
  const start = tpl.indexOf('<div class="page-row memory-row memory-row--candidate">');
  const end = tpl.lastIndexOf('</div>');
  assert.ok(start >= 0 && end > start, '候选行没切出来——锚点对不上');
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
  // 两个动作本来就在（上面那条判据是「不许动它们」，得先确认它们在）。2026-10-19
  // 起它们由共用的 `MemoryCandidateActions` 渲染：行里只留「把那一排放哪」的接线，
  // 裁决本身在组件里（接线与动作不许各写一份）。
  assert.match(card, /<MemoryCandidateActions[\s/>]/, '保存 / 忽略那一排还在行里');
  assert.match(
    templateOf(read('../src/memory/MemoryCandidateActions.vue')),
    /@click="decideCandidate\(item, true\)"/,
    '保存按钮还在（共用组件里）',
  );
  assert.match(
    templateOf(read('../src/memory/MemoryCandidateActions.vue')),
    /@click="decideCandidate\(item, false\)"/,
    '忽略按钮还在（共用组件里）',
  );
  // 详情弹窗同样不许因为 stale 藏动作：那一层是点开候选后唯一能裁决的地方。
  assert.match(
    templateOf(read('../src/memory/MemoryCandidateDetail.vue')),
    /<MemoryCandidateActions[\s/>]/,
    '详情弹窗也要留着裁决那一排',
  );
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
  const tpl = templateOf(read('../src/memory/MemoryCandidateRow.vue'));
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

// —— 候选详情弹窗（2026-10-19 用户反馈）————————————————————————————————————
//
// 用户原话（截图圈着「待确认」的候选行）：「正文被截成两行，点行没有任何反应，读不到
// 完整信息」。修法照记忆行那一套：正文那半整块可点，点开是 `MemoryCandidateDetail`
// （完整正文 + 来源凭据 + 三种关系块），页脚能直接裁决。
//
// 下面三条盯的都是本仓反复栽的形状：属性写了事件没绑 / 可点语义挂错层 / 面板没接住
// 事件 / 详情照抄了行里的截断。样式与结构层沿用上面的 `elementBlock` / 全局规则。

test('候选行的正文那半可点：Enter / 空格都发 open，面板接得住', () => {
  const row = templateOf(read(`${MEM_DIR}MemoryCandidateRow.vue`));
  // **只判 `__main`**：根元素下面还挂着动作排，`role="button"` 挂错层的话，点「忽略」
  // 会顺手把详情弹窗也打开（两个动作叠在一起）。
  const main = elementBlock(row, 'div', 'memory-row__main');
  for (const attr of ['role="button"', 'tabindex="0"', 'aria-haspopup="dialog"']) {
    assert.equal(
      (main.match(new RegExp(attr.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g')) || []).length,
      1,
      `候选行正文那半少了 ${attr}`,
    );
  }
  assert.match(
    main,
    /:aria-label="`查看候选详情：\$\{item\.title \|\| '（无标题）'\}`"/,
    '可访问名要说明点开的是哪一条（无标题时有兜底）',
  );
  // 三个入口逐个数——少一个键盘用户就只能靠鼠标。**空格不用 `.prevent` 修饰符**：
  // 那会在关系块里按钮的冒泡事件上也调一次 preventDefault，把键盘激活吞掉；拦默认
  // 行为写在 `openFromSpace` 里，过完闸门才拦。事件显式写成 `$event`：裸引用带形参
  // 会被 `ui/test/clickHandlerArity.test.js` 判成 bug（那一族检查的正确形状）。
  for (const handler of [
    '@click="openFromEvent($event)"',
    '@keydown.enter="openFromEvent($event)"',
    '@keydown.space="openFromSpace($event)"',
  ]) {
    assert.equal(main.split(handler).length - 1, 1, `候选行正文少了 ${handler}`);
  }
  assert.ok(
    !main.includes('@keydown.space.prevent'),
    '不该用 .prevent 修饰符：关系块里按钮的空格激活会连坐被拦掉',
  );
  const src = read(`${MEM_DIR}MemoryCandidateRow.vue`);
  // 闸门本身：可点区域里嵌着「停用这条 / 删除这条 / 丢弃这条」三枚按钮，它们的点击会
  // 冒泡到这里——不拦的话点一枚按钮会顺手打开详情弹窗（一次点击做两件事）。
  for (const [fn, extra] of [
    ['openFromEvent', ''],
    ['openFromSpace', 'event\\.preventDefault\\(\\);'],
  ]) {
    assert.match(
      src,
      new RegExp(
        `function ${fn}\\(event\\) \\{\\s*if \\(event\\.target\\.closest\\('button, a'\\)\\) return;[\\s\\S]{0,40}?${extra}[\\s\\S]{0,40}?emit\\('open', props\\.item\\);`,
      ),
      `${fn} 少了「冒泡按钮不触发详情」的闸门，或漏了 emit`,
    );
  }
  // 空格的默认行为只能在自己身上拦：过了闸门再 preventDefault。
  assert.match(
    src,
    /function openFromSpace\(event\) \{[\s\S]{0,120}?event\.preventDefault\(\);[\s\S]{0,60}?emit\('open', props\.item\);/,
    '空格要先过闸门再 preventDefault（顺序反了会拦掉按钮自己的空格激活）',
  );
  assert.match(src, /defineEmits\(\['open'\]\)/, '行要真的声明 open 事件');
  assert.ok(!src.includes("emit('open', row)"), '候选行发错了对象：它是 item 不是 row');
  // 动作排必须在 `__main` **外面**：套进可点区域里的话，点「忽略」会连详情弹窗一起打开，
  // 一次点击做两件事。行里只留「把动作排放哪」的接线。
  assert.ok(
    !main.includes('<MemoryCandidateActions'),
    '动作排被塞进可点区域里了：点「忽略」会顺手打开详情弹窗',
  );
  assert.match(
    row,
    /<MemoryCandidateActions class="memory-row__actions" :item="item" \/>/,
    '动作排还在行里，但它该挂在可点区域外面、位置类仍是 memory-row__actions',
  );
  // 可点的视觉反馈复用记忆行那一族规则（cursor / hover / focus-visible 只有一份实现）。
  assert.ok(
    literalClasses(main).includes('memory-row--clickable'),
    '可点的那块没挂上可点样式：cursor 与焦点圈都不会有',
  );
  const rules = globalRules();
  assert.equal(effectiveDeclaration(['memory-row--clickable'], rules, 'cursor'), 'pointer');
  assert.equal(
    effectiveDeclaration(['memory-row--clickable'], rules, 'outline', { pseudo: 'focus-visible' }),
    '1px solid var(--accent)',
    '键盘用户必须有焦点圈（行是 div，浏览器不会自己给）',
  );
  // 行上仍然是两行截断——截的是显示不是信息，全文在详情里。
  assert.match(
    row,
    /<p class="memory-row__body memory-row__body--clamp">\{\{ item\.body \}\}<\/p>/,
    '行上不该把正文摊开：列表要精简，完整正文在详情弹窗',
  );
  assert.match(
    templateOf(read(`${MEM_DIR}MemoryPanel.vue`)),
    /@open="openCandidateDetail"/,
    '面板没接住候选行的 @open',
  );
});

test('候选详情挂在 candidates 上：候选被处理完就从待确认里消失、弹窗跟着关', () => {
  const panelSrc = read(`${MEM_DIR}MemoryPanel.vue`);
  // **按 id 从 `candidates` 现取**，不攥行对象：保存 / 忽略之后 `memory_list` 回读会把
  // 这条从待确认里摘掉，按住 id 重算时 computed 变 `null`、弹窗自己关。攥对象的话，
  // 刚处理完的候选会一直画在弹窗里，而列表里已经没有它了。
  assert.match(panelSrc, /const candidateDetailId = ref\(null\)/, '详情弹窗要记 id 而不是行对象');
  assert.match(
    panelSrc,
    /const candidateDetailItem = computed\([\s\S]{0,120}candidates\.value\.find\(\(item\) => item\.id === candidateDetailId\.value\)/,
    '详情要按 id 从 candidates 现取（不能攥旧对象，也不能从记忆列表取）',
  );
  const at = panelSrc.indexOf('<MemoryCandidateDetail');
  assert.ok(at > 0, '面板里没有挂载候选详情弹窗');
  const mount = panelSrc.slice(at, panelSrc.indexOf('/>', at) + 2);
  for (const binding of [
    'v-if="candidateDetailItem"',
    ':item="candidateDetailItem"',
    // 两个标记块各自要的那份列表：冲突查全库、取代查待确认。
    ':records="allMemories"',
    ':candidates="candidates"',
    '@close="candidateDetailId = null"',
  ]) {
    assert.equal(mount.split(binding).length - 1, 1, `候选详情没接 ${binding}`);
  }
});

/**
 * 候选详情弹窗（2026-10-19）：点开之后要能读到行上被截掉的那部分——完整正文、完整
 * 会话 id、创建时间、验证状态，以及冲突 / 取代 / 来源已变三种关系。
 *
 * 盯三件各自会静默的事：① 弹窗照抄了行里的 `--clamp`（则详情里还是两行，等于没做）；
 * ② 组件建了没挂（本仓反复栽的形状，这里靠上一条判挂载、这一条判内容）；
 * ③ 弹窗参数与 `MemoryDetail.vue` 不一致（同屏两个详情弹窗一个点遮罩关、一个不关，
 * 用户会以为哪个坏了）。
 */
test('候选详情摆出完整正文与来源凭据，弹窗写法与记忆详情同一条', () => {
  const src = read(`${MEM_DIR}MemoryCandidateDetail.vue`);
  const tpl = templateOf(src);
  // 弹窗参数与 `MemoryDetail.vue` 逐项对齐。
  for (const attr of [
    ':model-value="true"',
    'width="min(560px, 92vw)"',
    'append-to-body',
    ':close-on-click-modal="false"',
  ]) {
    assert.equal(tpl.split(attr).length - 1, 1, `候选详情缺 ${attr}`);
  }
  assert.match(
    tpl,
    /@update:model-value="\(open\) => !open && emit\('close'\)"/,
    '关闭（✕ / ESC）要真的通知面板收回 id',
  );
  // 完整正文：不许带行里那个两行截断类。
  assert.match(tpl, /<p class="memory-row__body">/, '详情正文丢了 memory-row__body');
  assert.ok(
    !tpl.includes('memory-row__body--clamp'),
    '详情里还在截正文：这一层就是拿来看完整正文的',
  );
  // 标题用候选自己的（弹窗标题不是「候选详情」四个字），空标题有兜底。
  assert.match(tpl, /:title="item\.title \|\| '（无标题）'"/, '弹窗标题要用候选自己的标题');
  // 来源凭据：完整会话 id / 创建时间 / 验证状态。
  assert.match(tpl, /item\.source\.session \|\| '（本机，这一条没有会话 id）'/, '完整会话 id 丢了');
  assert.match(tpl, /shortTime\(item\.created_at\)/, '创建时间丢了');
  assert.match(tpl, /\{\{ verificationLabel \}\}/, '验证状态丢了');
  assert.match(
    src,
    /props\.item\.verification === 'stale' \? '来源已变'/,
    '候选的 stale 是「来源变了」，直接打印 verificationText 会说成「已过期」',
  );
  // 三种关系块与行上同一批组件：行里被压短的是显示，不是信息。
  assert.match(tpl, /<MemoryConflict[\s/>]/, '冲突块');
  assert.match(tpl, /<MemorySuperseded[\s/>]/, '取代块');
  assert.match(
    tpl,
    /<div v-if="item\.verification === 'stale'" class="memory-flag">/,
    '来源已变提示块',
  );
  // 页脚动作排与行共用一份，位置类由详情这侧补（靠右）。
  assert.match(
    tpl,
    /<MemoryCandidateActions class="dialog-actions" :item="item" \/>/,
    '页脚没有用共用的动作排，或没把那一排靠右',
  );
});

/**
 * 自动提取的落点变了（2026-10-19 用户要求：提取出来的长期约定直接成为项目记忆，
 * 只有模型标了冲突 / 取代的才留在待确认等用户裁决）。面板上凡是描述「提取结果去哪」
 * 的文案都必须跟着改，而**旧说法一条都不许留**——新旧两句同时挂在界面上互相矛盾，
 * 用户只能按其中一句信。
 *
 * `templateOf` 已剥 HTML 注释，所以这里命中的只可能是真文案；新句子逐字钉住
 * （四句都是用户看得见的），旧句子按子串判不存在。
 */
test('提取结果直接存记忆之后的文案：新事实逐字在，旧说法一条不留', () => {
  const panel = templateOf(read(`${MEM_DIR}MemoryPanel.vue`));
  // 新事实：页头 / 待确认 ⓘ / 待确认空态 / 提取任务 ⓘ 四处逐字钉。
  for (const line of [
    '模型只能读取与提出候选；自动提取的长期约定会直接存成记忆（未验证），只有与既有记忆冲突或推翻旧候选的才进「待确认」由你裁决。',
    '待确认里是模型在会话里主动提出的候选，以及自动提取里与既有记忆冲突、或推翻旧候选、需要你裁决的条目：正文与来源都摆在这里，只有你点保存才会成为记忆。',
    '没有待确认的候选。模型在会话里主动提出候选、或自动提取的结果需要你裁决时会出现在这里，只有点「保存」才成为记忆。',
    '提取出来的长期约定会直接存成项目记忆；与既有记忆冲突或推翻旧候选的才进「待确认」由你裁决',
  ]) {
    assert.ok(panel.includes(line), `面板上少了这句事实：${line}`);
  }
  // 旧说法：模型不能自己批准 / 自动提取先进入待确认 / 提取结果先「提成候选」。
  for (const stale of [
    '不能自己批准',
    '提出来的先进「待确认」，你点了保存才成为记忆',
    '提成候选',
    '这些是模型或会话提出的候选：正文与来源都摆在这里',
    '模型或会话提出候选后会出现在这里',
  ]) {
    assert.ok(!panel.includes(stale), `旧文案还留着：${stale}`);
  }
  // 同一段里那两句必须保留（它们与提取结果去哪无关，是「出问题时看哪里」）。
  assert.match(panel, /一条记忆都没提取出来时先看这里/);
  assert.match(panel, /memory-jobs/);
  // 「模型提出候选 → 待确认 → 点保存」对**会话里主动提出的候选仍然成立**，所以空态
  // 里那句留着——判据把它与「自动提取」那句分开，防止有人把整段删掉。
  assert.match(panel, /在工作台里让模型提出候选，再切到「待确认」里点保存/);
});
