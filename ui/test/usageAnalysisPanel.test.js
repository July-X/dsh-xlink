// 「用量分析」面板（2026-10-28 用户要求）与它牵动的窗口 / 概览改造。
//
// 这件事有四块彼此独立的契约，单看任何一块都发现不了回归：
//   ① 侧栏多了一格菜单，App.vue 要把它注册成可切换的面板；
//   ② 新面板的版面顺序（套餐在上、模型用量在下）与 inline 内嵌形态；
//   ③ 概览页只留「今日用量」一个读数，旧入口与旧样式必须删干净；
//   ④ 两扇独立窗口的**界面入口**移除了，但组件、Rust 命令、capability 与
//      `main.js` 的窗口路由一个都不许跟着删（仓库先例：入口隐藏 ≠ 能力删除）。
//
// 判据纪律（ui/AGENTS.md §判据怎么写）：
//   · 扫模板不扫全文——模板里的中文注释经常原样引用被判据的词，`templateOf`
//     先剥 HTML 注释再判；脚本侧同样先剥块注释与行注释。
//   · 样式读层叠后的生效值，不读原始子串；工具读不到的祖先条件
//     （`.usagewin.viewer-shell--inline …`）改读剥注释后的规则原文。
//   · 钉结构事实不钉字面量：标题栏按「开标签里三个属性同在」判，菜单按
//     「分组内 id 顺序」判，不数 `</div>`、不复述整段源码。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  allRulesIncludingVue,
  effectiveDeclaration,
  ruleBlocks,
  stripComments,
} from './lib/css-cascade.mjs';

const read = (rel) => readFileSync(new URL(`../src/${rel}`, import.meta.url), 'utf8');
const readRoot = (rel) => readFileSync(new URL(`../../${rel}`, import.meta.url), 'utf8');

/** `.vue` 的模板块，HTML 注释已剥；收尾边界取 `<style`（文件可能没有 scoped 块）。 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

/** 脚本 / 样式侧的注释（块注释 + 整行行注释），用于「不再出现某标识符」类判据。 */
function commentless(src) {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^[ \t]*\/\/.*$/gm, '');
}

/** `<tag …>` 开标签：判「哪几个属性同属一个元素」时按它取，不按整段文本。 */
function openTagAt(text, tagName, from = 0) {
  const at = text.indexOf(`<${tagName}`, from);
  assert.ok(at >= 0, `找不到 <${tagName}> 开标签`);
  const end = text.indexOf('>', at);
  assert.ok(end > at, `<${tagName}> 开标签没有收尾`);
  return { at, tag: text.slice(at, end + 1) };
}

// --- ① 侧栏与面板注册 -------------------------------------------------------

test('侧栏「工作台」组新增「用量分析」，紧跟概览且图标是 TrendCharts', () => {
  const sidebar = read('shell/SideBar.vue');
  // MENU_GROUPS 的文本块（到下一段注释为止）。剥块注释后按组切——只查全文里
  // 出现过「用量分析」是假绿：菜单项被挪到别的分组、或挂成条件项都拦不住。
  const groups = stripComments(
    sidebar.slice(sidebar.indexOf('const MENU_GROUPS'), sidebar.indexOf('// 桌面端版本号')),
  );
  const workbench = groups.slice(groups.indexOf("label: '工作台'"), groups.indexOf("label: '资源'"));
  assert.ok(workbench.length > 0, '找不到「工作台」分组');
  const ids = [...workbench.matchAll(/\bid:\s*'([\w-]+)'/g)].map((m) => m[1]);
  assert.deepEqual(ids, ['overview', 'usage'], '「工作台」组应只有概览与用量分析，且概览在前');
  assert.match(workbench, /id: 'usage', label: '用量分析', icon: TrendCharts/);
  // 图标必须真的从 EP 图标库按名导入：漏 import 时模板把它求值成 undefined，
  // 菜单项没有图标且不报错（生产构建同样静默）。
  assert.match(
    sidebar,
    /import \{[\s\S]*?\bTrendCharts\b[\s\S]*?\} from '@element-plus\/icons-vue'/,
    'TrendCharts 必须从 @element-plus/icons-vue 导入',
  );
});

test('App.vue 注册 usage 面板；概览刷新不等同于用量页刷新', () => {
  const app = read('App.vue');
  assert.match(app, /import UsageAnalysisPanel from '\.\/usage\/UsageAnalysisPanel\.vue';/);
  assert.match(app, /const PANELS = \{[\s\S]*?\busage: UsageAnalysisPanel,/);
  const refresh = app.slice(
    app.indexOf('function refreshActivePanelData'),
    app.indexOf('watch(() => store.activePanel'),
  );
  // 概览的「今日用量」仍立即对账。
  assert.match(refresh, /loadUsageSummary\(\)/, '切回概览时没有触发用量摘要刷新');
  // 但用量分析页**不能**走这条：`loadUsageSummary()` 是概览口径（显式
  // `external:false`），会把内嵌 UsageWindow 已加载的外部来源整块覆盖掉
  // （2026-10-28 评审实锤：切回页面且超过 TTL 后外部用量静默消失）。
  assert.doesNotMatch(refresh, /activePanel === 'usage'/, '用量页不得复用概览的摘要刷新');
  // 用量页进入时的重扫由窗口自己负责：挂载即全量 refreshUsage()。
  assert.match(
    read('usage/UsageWindow.vue'),
    /onMounted\(\(\) => \{\s*refreshUsage\(\);/,
    '内嵌的模型用量挂载时必须自己全量重扫',
  );
});

// --- ② 新面板的版面与内嵌形态 ------------------------------------------------

test('用量分析页：页头「用量分析」，套餐卡在上、模型用量以 inline 内嵌在下', () => {
  const tpl = templateOf(read('usage/UsageAnalysisPanel.vue'));
  assert.match(tpl, /<section class="panel usage-analysis-panel">/);
  // 页头沿用页面原语：标题与说明同属 `.page-head__text`（与其余面板同一结构
  // 契约，designAlignment 的深度判据也会覆盖这个文件）。
  assert.match(tpl, /class="page-head__text"/);
  assert.match(tpl, /<h1 class="page-title">用量分析<\/h1>/);
  assert.match(tpl, /<p class="page-desc">[^<]*\S[^<]*<\/p>/);
  // 版面顺序就是用户原话「上方是套餐用量，下方的详细页是原模型用量窗口」。
  const card = openTagAt(tpl, 'SubscriptionUsageCard');
  const win = openTagAt(tpl, 'UsageWindow');
  assert.ok(card.at < win.at, '套餐用量卡必须在模型用量上方');
  // 模型用量走 inline 内嵌：不带窗口标题（那会再画一条窗口 chrome），且确实
  // 被包在一张卡里（卡提供表面与描边）。
  assert.match(win.tag, /\binline\b/, 'UsageWindow 必须以 inline 模式内嵌');
  assert.doesNotMatch(win.tag, /\btitle=/, '内嵌形态不该再传独立窗口标题');
  const modelCard = tpl.lastIndexOf('<div', win.at);
  assert.ok(modelCard > 0, '模型用量应包在卡片容器里');
  const modelCardTag = tpl.slice(modelCard, tpl.indexOf('>', modelCard) + 1);
  assert.match(modelCardTag, /class="card usage-model-card"/);
});

// --- ③ 概览页只剩「今日用量」一个读数 ---------------------------------------

test('概览页不再有套餐用量卡与「模型用量」入口，三格指标只留今日用量', () => {
  const overview = read('shell/OverviewPanel.vue');
  const tpl = templateOf(overview);
  assert.doesNotMatch(
    tpl,
    /usage-card|套餐用量|plan-grid|SubscriptionUsageCard|UsageWindow/,
    '概览模板里还残留套餐用量卡的标记',
  );
  assert.doesNotMatch(tpl, /openUsageWindow|openSubscriptionWindow/, '概览模板里还有打开用量窗口的动作');
  // 三格指标仍在：Node.js / 今日用量 / 数据目录。今日用量那一格不再挂动作按钮。
  const metricsStart = tpl.indexOf('<div class="metrics">');
  assert.ok(metricsStart > 0, '概览模板里应有三格指标');
  const metrics = tpl.slice(metricsStart, tpl.indexOf('<el-alert', metricsStart));
  assert.equal((metrics.match(/class="metric"/g) || []).length, 3, '三格指标应仍是三格');
  assert.match(metrics, /<span>今日用量<\/span>/);
  assert.match(metrics, /<span>Node\.js<\/span>/);
  assert.match(metrics, /<span>数据目录<\/span>/);
  assert.doesNotMatch(metrics, /模型用量/, '今日用量那一格不许再挂「模型用量」按钮');
  // 脚本侧也不能还引着两扇窗口 / 迁走的卡片（注释已剥，命中即真实引用）。
  const script = commentless(overview.slice(0, overview.indexOf('<template>')));
  assert.doesNotMatch(
    script,
    /openUsageWindow|openSubscriptionWindow|UsageWindow|SubscriptionUsageCard/,
    '概览脚本里还留着用量窗口 / 套餐卡的引用',
  );
  // 旧位置的样式必须删干净：迁移之后它们不再有任何作用方，留着只会让人以为
  // 概览还在画套餐分块（ui/AGENTS.md「旧位置的规则必须删干净」）。
  const style = commentless(overview.slice(overview.indexOf('<style scoped>')));
  for (const selector of ['.plan-', '.usage-card', '.plan-head-actions', '.usage-consent-tip']) {
    assert.ok(!style.includes(selector), `概览 scoped 仍残留 ${selector} 规则`);
  }
});

test('套餐卡整体迁入 SubscriptionUsageCard.vue：「查看详情」已删，刷新与凭据入口保留', () => {
  const card = read('subscription/SubscriptionUsageCard.vue');
  const code = commentless(card);
  assert.match(code, /class="card usage-card"/, '套餐卡应是一张有卡头的 card');
  assert.match(code, /class="plan-grid"/, '三列服务商栅格仍在');
  assert.match(code, /class="plan-tier-value"/, '额度行仍在条外渲染百分比');
  // 入口隐藏的是「查看详情」：模板与脚本都不许再出现它或其动作。
  assert.doesNotMatch(code, /查看详情/, '「查看详情」按钮应已删除');
  assert.doesNotMatch(code, /openSubscriptionWindow/, '卡片不该再打开套餐用量独立窗口');
  // 但卡片自身的两个既有能力保留：刷新（数据仍要能更新）与前往模型设置。
  assert.match(code, /@click="onRefreshPlan"/, '刷新按钮必须保留');
  assert.match(code, /@click="openHarnessWindow"/, '「前往模型设置」入口必须保留');
  // 抬头文案仍是「套餐用量」，与调用页的「用量分析」页头不是同一个名字。
  assert.match(code, /套餐用量/);
});

// --- ④ 入口隐藏 ≠ 能力删除 ---------------------------------------------------

test('两扇独立窗口仍挂在 main.js，Rust 命令与 capability 一个不少', () => {
  const main = read('main.js');
  assert.match(main, /import UsageWindow from '\.\/usage\/UsageWindow\.vue';/);
  assert.match(main, /import SubscriptionWindow from '\.\/subscription\/SubscriptionWindow\.vue';/);
  assert.match(main, /params\.has\('usage'\)/);
  assert.match(main, /params\.has\('subscription'\)/);
  assert.match(main, /isUsageViewer\s*\?\s*UsageWindow/);
  assert.match(main, /isSubscriptionViewer\s*\?\s*SubscriptionWindow/);

  // Rust 侧命令仍注册：本轮删的是界面入口，不是窗口能力。
  const lib = readRoot('src-tauri/src/lib.rs');
  assert.match(lib, /usage::local::open_usage_window/);
  assert.match(lib, /usage::subscription::open_subscription_window/);

  // capability 文件与其中的窗口权限原样保留（缺 start-dragging 时窗口拖不动，
  // 且不会报错——那是 windowChrome.test.js 另一条在守的同一批权限）。
  for (const [rel, perm] of [
    ['usage-viewer.json', 'allow-usage-viewer'],
    ['subscription-viewer.json', 'allow-subscription-viewer'],
  ]) {
    const cap = JSON.parse(readRoot(`src-tauri/capabilities/${rel}`));
    assert.equal(cap.windows.length, 1, `${rel} 应仍只绑一扇窗口`);
    for (const wanted of [perm, 'core:window:allow-start-dragging']) {
      assert.ok(cap.permissions.includes(wanted), `${rel} 缺 ${wanted}`);
    }
  }

  // app-commands.json 的命令白名单是窗口能力的另一半：视图与刷新命令仍在。
  const perms = JSON.parse(readRoot('src-tauri/permissions/app-commands.json'));
  const byId = new Map(perms.permission.map((entry) => [entry.identifier, entry]));
  for (const [id, commands] of [
    ['allow-usage-viewer', ['get_model_usage', 'set_usage_external_sources']],
    [
      'allow-subscription-viewer',
      ['get_subscription_usage', 'set_codex_usage_enabled', 'open_usage_window', 'open_harness', 'builtin_status'],
    ],
  ]) {
    const entry = byId.get(id);
    assert.ok(entry, `app-commands.json 缺 ${id}`);
    for (const command of commands) {
      assert.ok(entry.commands.allow.includes(command), `${id} 不再允许 ${command}`);
    }
  }
});

// --- ⑤ ViewerShell / UsageWindow 的 inline 形态 ------------------------------

test('ViewerShell inline：窗口 chrome 退场，窗口模式四件事一件不少', () => {
  const shell = read('shell/ViewerShell.vue');
  assert.match(shell, /inline:\s*\{\s*type:\s*Boolean,\s*default:\s*false\s*\}/, 'inline 必须默认 false');
  // 标题栏是**窗口模式**的 chrome：三个属性同属一个开标签，且带 `!inline` 门控。
  const bar = openTagAt(shell, 'WindowTitleBar');
  assert.match(bar.tag, /v-if="!inline"/, '标题栏必须只在内嵌模式之外渲染');
  assert.match(bar.tag, /:title="title"/, '窗口模式仍要把功能标题传进标题栏');
  assert.match(bar.tag, /close-only/, '副窗模式仍只画关闭按钮');
  // 模板把 inline 落到根类上（`:class` 数组）——只加 prop 不挂类，样式覆盖永不生效。
  assert.match(shell, /:class="\[shellClass, \{ 'viewer-shell--inline': inline \}\]"/);

  const rules = allRulesIncludingVue();
  // 窗口模式（默认）：定高 / 圆角 / 窗口底色 / 裁切一个不少。
  assert.equal(effectiveDeclaration(['viewer-shell'], rules, 'height'), '100vh');
  assert.equal(effectiveDeclaration(['viewer-shell'], rules, 'border-radius'), 'var(--window-radius)');
  assert.equal(effectiveDeclaration(['viewer-shell'], rules, 'background-color'), 'var(--window)');
  assert.equal(effectiveDeclaration(['viewer-shell'], rules, 'overflow'), 'hidden');
  // 内嵌模式：这四件事全部退场——否则面板里会出现一扇固定 100vh 的「窗口」。
  assert.equal(effectiveDeclaration(['viewer-shell--inline'], rules, 'height'), 'auto');
  assert.equal(effectiveDeclaration(['viewer-shell--inline'], rules, 'border-radius'), '0');
  assert.equal(effectiveDeclaration(['viewer-shell--inline'], rules, 'background-color'), 'transparent');
  assert.equal(effectiveDeclaration(['viewer-shell--inline'], rules, 'overflow'), 'visible');
});

test('UsageWindow inline：脉冲归主壳，窗口模式的高度与滚动契约不变', () => {
  const usageWindow = read('usage/UsageWindow.vue');
  assert.match(usageWindow, /inline:\s*\{\s*type:\s*Boolean,\s*default:\s*false\s*\}/);
  // 脉冲类 `pulse-active` 只能有一个订阅方：内嵌时归主壳 App.vue，本组件不 toggle。
  assert.match(
    usageWindow,
    /if \(!props\.inline\) \{\s*watchEffect\(\(\) => \{\s*document\.body\.classList\.toggle\('pulse-active', ioActive\.value\);/,
    '内嵌时不许再订阅鲸眼脉冲（两个订阅方谁先卸载谁留下脏类）',
  );
  const shellTag = openTagAt(usageWindow, 'ViewerShell');
  assert.match(shellTag.tag, /shell-class="usagewin"/);
  assert.match(shellTag.tag, /:inline="inline"/, 'inline 必须真的转交给共享外壳');
  assert.match(shellTag.tag, /\btitle="模型用量"/, '窗口模式的功能标题仍在（内嵌时由外壳门控掉 chrome）');

  // 祖先条件选择器（`.usagewin.viewer-shell--inline …`）层叠工具看不见，按
  // ui/AGENTS.md §判据2 改读剥注释后的规则原文；窗口模式的基线规则同样读原文，
  // 避免 inline 覆盖被工具当成窗口模式的答案。
  const scopedStyle = /<style scoped>([\s\S]*?)<\/style>/.exec(usageWindow);
  assert.ok(scopedStyle, 'UsageWindow 应有 scoped 样式块');
  const style = stripComments(scopedStyle[1]);
  const rules = ruleBlocks(style);
  const ruleOf = (selector) => rules.find((rule) => rule.selector === selector);
  const baseWin = ruleOf('.usagewin');
  assert.ok(baseWin, '缺少 .usagewin 基线规则');
  assert.match(baseWin.body, /height:\s*100vh/, '窗口模式仍是定高窗口');
  assert.match(baseWin.body, /background:\s*var\(--window\)/, '窗口模式仍有窗口底色');
  const baseMain = ruleOf('.usagewin-main');
  assert.ok(baseMain, '缺少 .usagewin-main 基线规则');
  assert.match(baseMain.body, /overflow-y:\s*auto/, '窗口模式下 main 必须能滚（外部来源那段靠它）');
  const inlineWin = ruleOf('.usagewin.viewer-shell--inline');
  assert.ok(inlineWin, '缺少内嵌模式的 .usagewin 覆盖规则');
  assert.match(inlineWin.body, /height:\s*auto/, '内嵌模式不许占 100vh');
  assert.match(inlineWin.body, /background:\s*transparent/, '内嵌模式不许再画窗口底色');
  const inlineMain = ruleOf('.usagewin.viewer-shell--inline .usagewin-main');
  assert.ok(inlineMain, '缺少内嵌模式的 main 覆盖规则');
  assert.match(inlineMain.body, /flex:\s*none/, '内嵌时 main 不许被压扁');
  assert.match(inlineMain.body, /overflow:\s*visible/, '内嵌时滚动交给面板 main，main 不许自己滚');
  assert.match(inlineMain.body, /scrollbar-gutter:\s*auto/, '内嵌时不再预留窗口滚动条槽');
});

test('模型用量卡头用语义图标（柱状图），不再拿品牌鲸鱼当区块图标', () => {
  const usageWindow = read('usage/UsageWindow.vue');
  const tpl = templateOf(usageWindow);
  const headAt = tpl.indexOf('<header class="usagewin-head">');
  assert.ok(headAt >= 0, '找不到模型用量卡头');
  const headEnd = tpl.indexOf('</header>', headAt);
  assert.ok(headEnd > headAt, '卡头没有收尾');
  const head = tpl.slice(headAt, headEnd);
  // 2026-10-28 用户截图指认「不应该是小黑鱼」：`/whale-icon.png` 是应用品牌标，
  // 放进区块标题既不表意（这块讲的是 token 用量），深色卡面上还几乎看不见。
  assert.doesNotMatch(head, /whale-icon/, '模型用量卡头不许再用品牌鲸鱼图标');
  // 形状与「套餐用量」卡头对齐：accent 方块 + 白色图形（全局 `.card-title-icon`）。
  assert.match(
    head,
    /<span class="card-title-icon" aria-hidden="true"><el-icon><Histogram \/><\/el-icon><\/span>/,
    '区块图标应为 .card-title-icon 方块 + Histogram（用量 = 柱状图，14px 下仍读得出）',
  );
  // 图标必须真的按名导入：漏 import 时模板把它求值成 undefined，图标静默消失。
  assert.match(
    usageWindow,
    /import \{[^}]*\bHistogram\b[^}]*\} from '@element-plus\/icons-vue'/,
    'Histogram 必须从 @element-plus/icons-vue 导入',
  );
});

test('套餐卡并入独立窗口的两处信息：查询实例 tooltip 与「尚未查询」年龄胶囊', () => {
  // 2026-10-28 用户问「这两个入口是什么」后核对：原套餐用量窗口比卡多出的
  // 有效信息只有「这份余额属于哪个实例/profile」与「从未查询成功」这两种；
  // 其余是窗口 chrome / 纵向布局。把这两处并进卡里，入口就不必恢复。
  const card = read('subscription/SubscriptionUsageCard.vue');
  assert.match(
    card,
    /const instanceLabel = computed\(\(\) => \{[\s\S]{0,220}?subscription\.data\.instance[\s\S]{0,200}?profile \$\{info\.profile\}/,
    '卡必须从 subscription.data.instance 拼出实例 / profile 标识',
  );
  assert.match(card, /<template v-if="instanceLabel">[\s\S]{0,160}?当前查询实例/, 'tooltip 里要展示查询实例');
  assert.match(card, /\{\{ row\.queriedCompact \|\| '尚未查询' \}\}/, '从未查询成功时要写「尚未查询」，不能空白');
  assert.match(card, /function agePillTitle\(row\)/, '年龄胶囊的悬停说明要走同一个判据');
  assert.match(card, /尚未查询成功，点击查询/, '悬停说明要覆盖从未查询成功的情形');
});

// --- 反向验：断言真的能命中它要拦的回归 --------------------------------------

test('判据自检：结构断言对「把卡搬回概览 / 删掉 inline 门控 / 栅格回三行」真的转红', () => {
  // 反向验的变异必须在**被测产物**上剪贴，而不是另写一段相似的假代码：
  // 变异后先断言「新串读得到被搬回的东西」，再确认同一条判据会抛错。
  const tpl = templateOf(read('shell/OverviewPanel.vue'));
  const movedBack = tpl.replace(
    '<div class="metrics">',
    '<div class="card usage-card"><h2>套餐用量</h2></div>\n          <div class="metrics">',
  );
  assert.notEqual(movedBack, tpl, '反向验变异没有命中概览模板（原串没被改到）');
  assert.match(movedBack, /usage-card[\s\S]*套餐用量/, '变异后的模板必须真的包含被搬回的套餐卡');
  assert.throws(
    () => assert.doesNotMatch(movedBack, /usage-card|套餐用量/),
    '「概览不再有套餐卡」断言没有命中搬回的卡——这条判据是瞎的',
  );

  const shell = read('shell/ViewerShell.vue');
  const bar = openTagAt(shell, 'WindowTitleBar').tag;
  const ungated = bar.replace(/\s*v-if="!inline"/, '');
  assert.notEqual(ungated, bar, '反向验变异没有命中标题栏开标签');
  assert.throws(
    () => assert.match(ungated, /v-if="!inline"/),
    '窗口 chrome 的门控断言没有命中删掉 v-if 的标题栏',
  );

  // 概览行模板：在**产物**上把两行改回三行，确认判据会红（旧写法只是在本
  // 文件里拼两个字符串自证，不碰被测文件——2026-10-28 评审指出的假反向验）。
  const overview = read('shell/OverviewPanel.vue');
  const twoRows = 'grid-template-rows: auto minmax(min-content, 1fr)';
  assert.ok(overview.includes(twoRows), '概览行模板不是当前的两行值，先核对设计');
  const threeRows = overview.replace(
    twoRows,
    'grid-template-rows: auto auto minmax(min-content, 1fr)',
  );
  assert.notEqual(threeRows, overview, '反向验变异没有命中概览行模板');
  assert.throws(
    () => assert.match(threeRows, /grid-template-rows:\s*auto minmax\(min-content, 1fr\)/),
    '概览行模板断言没有命中三行值',
  );
});
