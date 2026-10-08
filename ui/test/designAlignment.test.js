// 概览页与侧栏的**设计稿对齐**契约（2026-10-07 用户反馈「脱离设计稿太多了」后
// 逐块比对写下的一组判据）。
//
// 判据一律走**层叠后的生效值**（`effectiveDeclaration`），不比原始 CSS 子串：
// 出事那次的形态正是「每处单看都合理、叠起来才不对」——三格指标被 theme.css
// 的全局 `.metric`（描边 + 底色 + 10/12 内边距）盖成三个独立卡片，而 scoped
// 只写了 `min-width: 0`，任何「我看看 scoped 里写了什么」的检查都看不出冲突。
//
// 另一类判据是**模板结构**（用 / 网格用哪个元素承载落位）。这类不看 CSS：
// `grid-column: 1/-1` 写在 `.usage-card` 上还是没写，是模板与栅格的组合事实。
import assert from 'node:assert/strict';
import test from 'node:test';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  SRC,
  allRulesIncludingVue,
  cssFiles,
  effectiveDeclaration,
  stripComments,
  subjectTokens,
} from './lib/css-cascade.mjs';

/** `ui/src` 下所有 `.vue` 与 `.js`，递归。**必须带 `.js`**：独立窗口的挂载
 *  点在 `main.js`（它按窗口类型挑根组件），只扫 `.vue` 会把「已经挂在窗口上了」
 *  误判成「没有任何地方挂载」。 */
function srcScriptFiles(dir = SRC, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = resolve(dir, entry.name);
    if (entry.isDirectory()) srcScriptFiles(p, out);
    else if (p.endsWith('.vue') || p.endsWith('.js')) out.push(p);
  }
  return out;
}

// 必须带 `.vue` 的 scoped 块：面板级样式大半写在组件里，只扫 `.css` 时
// `.kernel-summary` / `.sidebar__theme-btn` 这些规则一条都查不到，判据会退化成
// 「只查全局基线」——那不是层叠，只是 theme.css。
const RULES = allRulesIncludingVue();
const overview = readFileSync('ui/src/shell/OverviewPanel.vue', 'utf8');
const sidebar = readFileSync('ui/src/shell/SideBar.vue', 'utf8');
const controlTower = readFileSync('ui/src/diagnostics/ControlTower.vue', 'utf8');
const diagnosticsCss = readFileSync('ui/src/diagnostics/diagnostics.css', 'utf8');
const themeCss = readFileSync('ui/src/theme.css', 'utf8');
const versionsPanel = readFileSync('ui/src/kernel/VersionsPanel.vue', 'utf8');

/** scoped 样式块的内容（`.vue` 里最后一个 `<style scoped>`）。 */
function scopedStyle(src) {
  const start = src.indexOf('<style scoped>');
  assert.ok(start > 0, '找不到 <style scoped> 块');
  return src.slice(start);
}

/**
 * `.vue` 的模板块，HTML 注释已剥。
 *
 * 为什么判据要专门挑模板块：本仓每个模板都带着大段解释「为什么这么写」的中文
 * 注释，而注释里经常**原样引用被判据的词**（「活动视图已同步」那条为什么不画、
 * 「插件中心那个 ⓘ」现在在哪）。按整份文件 indexOf / doesNotMatch，判据会把自己
 * 写的解释当成命中。同一个坑这轮已经踩了三次，三次都是扫全文。
 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  // 收尾边界取 `<style` 而不是 `<style scoped>`：PluginsPanel 没有自己的
  // scoped 块（样式都在 theme.css），写死 scoped 会在这类文件上直接 assert 挂掉。
  const styleAt = src.indexOf('<style', start);
  const end = styleAt > start ? styleAt : src.length;
  return src.slice(start, end).replace(/<!--[\s\S]*?-->/g, '');
}

// --- 概览主栅格落位 -------------------------------------------------------
//
// 设计稿 `.grid` 是 1.25fr / 0.75fr 两列，「需要关注」整宽、当前内核与系统
// 同行、套餐用量跨两列、最近操作整行。这四条落位用 `order` + `grid-column`
// 表达（`grid-row` 会在「需要关注」缺席时整体前移，`order` 对此免疫）。

test('概览主栅格取设计稿的 1.25fr / 0.75fr，列间 12px', () => {
  assert.equal(effectiveDeclaration(['overview-grid'], RULES, 'grid-template-columns'), 'minmax(0, 1.25fr) minmax(280px, 0.75fr)');
  assert.equal(effectiveDeclaration(['overview-grid'], RULES, 'gap'), '12px');
});

test('套餐用量跨栅整宽；首次运行引导占首行整宽', () => {
  assert.equal(effectiveDeclaration(['callout-firstrun'], RULES, 'grid-column'), '1 / -1');
  assert.equal(effectiveDeclaration(['usage-card'], RULES, 'grid-column'), '1 / -1');
});

test('当前内核卡在第 1 列，用 order 而不是 grid-row 落位', () => {
  assert.equal(effectiveDeclaration(['kernel-card'], RULES, 'grid-column'), '1');
  // 「需要关注」缺席时行号会前移，grid-row 随之错位；order 只管视觉顺序。
  assert.equal(effectiveDeclaration(['kernel-card'], RULES, 'grid-row'), null);
});

// --- 当前内核卡 -----------------------------------------------------------
//
// 设计稿 `.kernel-summary` 把版本号与状态行放在**同一个竖向块**里
// （版本号一行、`kernel-status-row` 下一行）。横排时状态胶囊会随内核状态
// 换到不同位置，这一行的读法就不稳定了——所以判据直接查模板结构。
//
// 2026-10-08 次级入口那一排三枚按钮也接进这个块（用户：「操作按钮迁移到
// 版本号右侧，靠右显示」）。落位因此变成**两行网格**：版本号 + 按钮同占
// 第一行（按钮靠右），状态行 `grid-column: 1 / -1` 独占第二行。理由是实测
// 宽度——版本号 98px + 按钮 354px + 状态行 287px = 739px，而卡内可用宽只有
// 482px，三者平铺会把状态行挤到折成三四行。
//
// **下面那条判据因此改写过一次**：它原来断言 `kernel-version` 与
// `kernel-status-row` 是「紧邻的两个兄弟」，按钮插进中间后那条正则必然失配。
// 但那条正则真正要拦的是「两者并排」，不是「两者相邻」——把相邻当成意图，
// 就会在往块里加第三个元素时逼人改判据而不是改布局。所以这里改成查**栅格
// 落位**：状态行必须跨两列独占第二行，这才是「不并排」的结构事实。

test('版本号与状态行不并排：状态行跨两列独占第二行', () => {
  assert.match(overview, /<div class="kernel-summary">\s*<!--[\s\S]*?-->\s*<div class="kernel-summary-main">/);
  assert.match(overview, /class="kernel-summary-main">[\s\S]*?<div\s+class="kernel-version"/);
  assert.match(overview, /class="kernel-summary-main">[\s\S]*?<div class="kernel-status-row">/);
  // 「不并排」的结构事实：版本号在第一行第 1 列，状态行跨满两列落在第二行。
  // 只查模板里的类名顺序是查不出并排的——CSS 才是落位的事实来源。
  assert.equal(effectiveDeclaration(['kernel-summary-main'], RULES, 'display'), 'grid');
  assert.equal(
    effectiveDeclaration(['kernel-summary-main'], RULES, 'grid-template-columns'),
    'minmax(0, 1fr) auto',
    '第一列随版本号伸缩、第二列按按钮实宽，右列才是「靠右」的前提',
  );
  assert.equal(effectiveDeclaration(['kernel-status-row'], RULES, 'grid-column'), '1 / -1');
  assert.equal(effectiveDeclaration(['kernel-status-row'], RULES, 'grid-row'), '2');
  assert.equal(effectiveDeclaration(['kernel-version'], RULES, 'grid-row'), '1');
});

test('次级入口在版本号右侧靠右：网格第二列 + justify-self: end', () => {
  const tpl = templateOf(overview);
  // **必须在 `.kernel-summary-main` 里面**，不能停在三格指标下方。判位置按
  // 模板里两个锚点的先后顺序判，不按「文件里出现过这句话」——后者在按钮被
  // 搬回去时照样绿。这条断言的反向验第一版就漏在这里：只查了区间内有按钮，
  // 没查**区间之后**没有按钮，于是「搬回去」这种改坏完全抓不到。
  const mainAt = tpl.indexOf('class="kernel-summary-main"');
  const metricsAt = tpl.indexOf('class="metrics"');
  assert.ok(mainAt > 0 && metricsAt > mainAt, '应能找到 kernel-summary-main 与 metrics');
  const block = tpl.slice(mainAt, metricsAt);
  const after = tpl.slice(metricsAt);
  assert.match(block, /class="btn-row btn-row-sub"/, '次级入口这一排应落在版本号块内');
  assert.doesNotMatch(after, /class="btn-row btn-row-sub"/, '次级入口不许退回三格指标下方');
  // 三枚一枚都不能少。这三条各自用**整枚按钮**（从开标签到收标签）计数，而不是
  // `assert.match(块, 文案)`——后者中招过一次：`官网网页版窗口` 包含
  // `工作台窗口` 这个子串，删掉任何一枚，剩下的仍能让三条断言都命中，判据照样绿。
  // 计数取 `<el-button … >文案</el-button>` 的完整形状，删一枚就少一次。
  for (const label of ['工作台窗口', '刷新工作台', '官网网页版窗口']) {
    const re = new RegExp(`<el-button[\\s\\S]*?>\\s*${label}\\s*</el-button>`);
    const n = (block.match(new RegExp(re.source, 'g')) || []).length;
    assert.equal(n, 1, `「${label}」这一枚必须恰好存在一次（实测 ${n} 次）`);
  }
  // 靠右：网格项默认 `stretch`，不钉 `justify-self: end` 就会被拉满整列、
  // 贴着版本号而不是右边。`justify-self` 不带伪类也不带后代条件，可以走
  // effectiveDeclaration。
  assert.equal(effectiveDeclaration(['kernel-summary-main', 'btn-row-sub'], RULES, 'grid-column'), '2');
  assert.equal(effectiveDeclaration(['kernel-summary-main', 'btn-row-sub'], RULES, 'grid-row'), '1');
  assert.equal(effectiveDeclaration(['kernel-summary-main', 'btn-row-sub'], RULES, 'justify-self'), 'end');
  // 左侧那条「挂在上方读数下方」的竖线与缩进随这次搬家删除：它表达的是
  // 「从属于下方那块读数」，而这一排现在与版本号同行。留着会成为一条没有
  // 归属的孤线，还白占 16px 卡内宽度。
  assert.equal(effectiveDeclaration(['btn-row-sub'], RULES, 'border-left'), null);
  assert.equal(effectiveDeclaration(['btn-row-sub'], RULES, 'padding-left'), null);
});

test('版本号是纯文字：没有 tag 图标、没有徽标外壳，贴着左缘', () => {
  // 2026-10-08 用户截图要求「移除 tag icon，版本号文字放大，靠最左边」。
  // 三件事各自会被别的原因悄悄改回去，所以分开钉：
  //   · 去掉徽标 → OverviewPanel 不该再引 VersionBadge（那段注释里原样写着
  //     它的名字，扫全文会把自己当成命中，所以扫剥掉注释后的脚本块）；
  //   · 靠最左 → `margin-left: 0` 必须显式钉住：徽标没了之后，胶囊曾经
  //     用来把首字推离左缘的那 5px 内边距也一起没了，但外层缩进不会自动归零；
  //   · 放大 / 主文字色 → 下面那条按值钉。
  assert.doesNotMatch(
    overview
      // 三种注释都要剥：HTML 注释（模板里解释「为什么去掉徽标」那段原样写着
      // VersionBadge）、CSS 块注释（`.kernel-version` 规则上方那段同样提到了
      // 它）、JS 行注释。少剥一种，判据就会把自己写的说明当成命中——本仓
      // 同一个坑踩过多次，每次都是扫全文。
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .replace(/<!--[\s\S]*?-->/g, '')
      .replace(/^\s*\/\/.*$/gm, ''),
    /VersionBadge/,
    '概览的版本号不该再走徽标组件；设计稿的 .kernel-version 就是一段纯文字',
  );
  assert.doesNotMatch(templateOf(overview), /<VersionBadge|version-badge/);
  assert.equal(
    effectiveDeclaration(['kernel-version'], RULES, 'margin-left'),
    '0',
    '版本号必须贴左缘——去徽标后没有任何东西再负责这段缩进',
  );
  assert.equal(effectiveDeclaration(['kernel-version'], RULES, 'color'), 'var(--text)');
  assert.equal(effectiveDeclaration(['kernel-version'], RULES, 'font-weight'), '700');
});

test('概览「当前内核」卡不再显示实例 caption，也不从 KernelStatus 读不存在的字段', () => {
  // 2026-10-07 用户要求删掉卡头右侧那行「DSH / default-dev」：卡里已经有一个
  // 大号活动内核版本号和它旁边的状态胶囊，族名与实例 id 在这里是把实现细节
  // 摆到第一屏。判据钉的是「概览页不再引实例注册表」。
  assert.doesNotMatch(overview, /instanceStore/, '概览页不该再引实例注册表');
  assert.doesNotMatch(overview, /familyLabel/);
  assert.doesNotMatch(overview, /loadInstances/);
  // 光断「不引注册表」不够：把一句写死的 `card-caption` 塞回卡头并不会重新
  // import 任何东西，那条判据照样绿。名字本身也要断。
  // **必须扫模板块**：`OverviewPanel.vue` 里那段解释「为什么删」的注释里原样
  // 写着 `kernelCaption`，扫全文会把自己写的说明当成命中——本仓同一个坑踩过
  // 三次，都是扫全文。
  assert.doesNotMatch(templateOf(overview), /kernelCaption/);
  // 「不读 KernelStatus 的不存在字段」这条教训保留：`KernelStatus` 没有族名字段
  // 也没有实例 id 字段，从 kernel 上读它们取到的是 undefined，而
  // `undefined || …` 会静默落到兜底分支——门禁 [ipc-fields] 当场抓的就是这条。
  // 匹配的是「从 kernel 上取」这个形状，不是具体字段名：后者连注释里提一嘴都
  // 会误判成真实读取。
  assert.doesNotMatch(overview, /kernel(\.value)?\.[a-z_]*family/i);
});

test('「当前内核」的主操作与标题同一行，卡头只剩标题 + 动作两栏', () => {
  // 切片起点要带 `<div `：只切到 `class="card-head"` 会把开标签本身切掉，
  // 后面那句「紧跟 `<h2`」的断言就永远匹配不上（第一版就是这么写的，红的是
  // 判据不是代码）。
  const headStart = overview.indexOf('<div class="card-head">');
  assert.ok(headStart > 0, '概览页第一张卡应是「当前内核」');
  const head = overview.slice(headStart);
  const headBlock = head.slice(0, head.indexOf('<div class="kernel-summary">'));
  // 两个按钮都在 `.kernel-header-actions` 里，而它必须与 `<h2 class="kernel-title">`
  // 同属一个 `.card-head`——原先它们窝在 `.card-head-left` 里，落在标题下面
  // 那一行的左侧，视线要往下再折一次才找得到。
  assert.match(headBlock, /<div class="card-head">\s*<h2 class="kernel-title">/);
  assert.match(headBlock, /<div class="kernel-header-actions">/);
  assert.doesNotMatch(headBlock, /card-head-left/, '卡头不该再有「标题 + 按钮」那个左栏包裹层');
  // 两个按钮都要在，别在挪位置时掉一个。
  assert.match(headBlock, /工作台/);
  assert.match(headBlock, /官网网页版/);
});

test('版本号 20px / 700（设计稿 18px，用户要求再放大一档）；摘要块内边距 13px 0 12px', () => {
  const style = scopedStyle(overview);
  assert.equal(effectiveDeclaration(['kernel-version'], RULES, 'font-size'), '20px');
  // `align-items: start` 原先挂在 `.kernel-summary`（那时它是 flex）。2026-10-08
  // 次级入口搬进来后它变成两行网格，纵向对齐的责任落到 `.kernel-summary-main`
  // 的 `align-items: center`——按钮与版本号同高时居中才好看，靠上会错半格。
  // 这里跟着搬：钉在旧选择器上会得到 null，钉在新选择器上才是活的断言。
  assert.equal(effectiveDeclaration(['kernel-summary-main'], RULES, 'align-items'), 'center');
  assert.equal(effectiveDeclaration(['kernel-summary'], RULES, 'padding'), '13px 0 12px');
  assert.ok(style.includes('kernel-summary'), 'scoped 块应包含 kernel-summary');
});

test('三格指标：1fr 三列、上边线 + 后两格左边线；格内按钮只收左右内边距', () => {
  assert.equal(effectiveDeclaration(['metrics'], RULES, 'grid-template-columns'), 'repeat(3, minmax(0, 1fr))');
  assert.equal(effectiveDeclaration(['metrics'], RULES, 'border-top'), '1px solid var(--border-soft)');
  // theme.css 的全局 `.metric` 曾给它描边 + 底色，把三格变成三个独立卡片。
  // 它已经删除，这条断言守的是「别把它当成可用原语再加回来」。
  assert.equal(effectiveDeclaration(['metric'], RULES, 'border'), null);
  assert.equal(effectiveDeclaration(['metric'], RULES, 'background'), null);
  assert.equal(effectiveDeclaration(['metric'], RULES, 'padding'), null);
  // Element Plus 的 small 按钮左右各 11px，「今日用量 + 模型用量」放不下会折行。
  // `:deep()` 是编译期伪类，工具的主语 token 匹配接不住它（`:deep` 会被当成
  // 一个不存在的类）。这条判据直接读 scoped 块里的那条规则原文。
  assert.match(
    scopedStyle(overview),
    /\.metrics :deep\(\.el-button\)\s*\{[^}]*padding-left:\s*4px;[^}]*padding-right:\s*4px;/s
  );
});

test('三格的动作按钮：统一 accent 色 + 600 字重 + 16px 图标槽（设计稿 .text-action）', () => {
  // 2026-10-08 用户截图指出三格按钮「颜色字体大小」没对齐稿子。真因有二：
  //   · **配色两套**：只有「模型用量」带 `type="primary"` 走 accent 蓝，
  //     「重新检测」与「打开」没带，于是同一排三个同级入口里一枚蓝两枚黑；
  //   · **图标太小**：EP 的 `.el-icon` 是 `font-size: inherit`，按钮 11px
  //     把图标一起缩到 11px，认不出是检测 / 图表 / 文件夹。
  // 稿子的 `.button.text-action` 给的是 `color: var(--accent)` / 11px / 600，
  // `.button-icon` 是 16×16 的槽。
  const style = scopedStyle(overview);
  const btnRule = style.match(/\.metrics :deep\(\.el-button\)\s*\{([^}]*)\}/s);
  assert.ok(btnRule, '找不到 .metrics :deep(.el-button) 规则');
  assert.match(btnRule[1], /color:\s*var\(--accent\)/, '三个按钮必须同一个 accent 色');
  assert.match(btnRule[1], /font-weight:\s*600/, '设计稿 .text-action 是 600 字重');
  assert.match(btnRule[1], /font-size:\s*11px/);
  assert.match(
    style,
    /\.metrics :deep\(\.el-button \.el-icon\)\s*\{\s*font-size:\s*16px;/,
    '图标槽必须是 16px（设计稿 .button-icon.ui-icon 的 flex: 0 0 16px）',
  );
  // 配色统一之后，模板上就不该再有人手动挂 `type="primary"` —— 漏一个就又
  // 变回一枚蓝两枚黑，而它不会报任何错。切片从 `<div class="metrics">` 到紧跟其后
  // 的 `<el-alert`（那一段就是三格本身），不按 `</div>` 数层数。
  const tpl = templateOf(overview);
  const from = tpl.indexOf('<div class="metrics">');
  assert.ok(from > 0, '概览模板里要有三格指标');
  const metricsBlock = tpl.slice(from, tpl.indexOf('<el-alert', from));
  assert.doesNotMatch(metricsBlock, /type="primary"/, '配色归 CSS 管，模板上不该再挂 type="primary"');
});

test('指标标题与值取设计稿的 11px muted / 12px 600 text', () => {
  assert.equal(effectiveDeclaration(['metric-label'], RULES, 'font-size'), '11px');
  assert.equal(effectiveDeclaration(['metric-label'], RULES, 'color'), 'var(--text-muted)');
  assert.equal(effectiveDeclaration(['metric-value'], RULES, 'font-size'), '12px');
  assert.equal(effectiveDeclaration(['metric-value'], RULES, 'font-weight'), '600');
  assert.equal(effectiveDeclaration(['metric-value'], RULES, 'color'), 'var(--text)');
});

test('「数据目录」那一格指向应用根 ~/.dsh-xlink，实例目录退到 title', () => {
  // 2026-10-08 用户定的：那一格显示与「打开」都指向 `~/.dsh-xlink` 根目录。
  // 实例目录 `~/.dsh-xlink/<family>/desktop/` 在 141px 的格子里被 `display_short`
  // 截成 `~/.dsh-xlink/dsh/…`，尾巴上的省略号看着像一条坏掉的路径。
  assert.match(
    overview,
    /kernel\.value\.xlink_home/,
    '这一格读 KernelStatus.xlink_home，不再读 data_dir',
  );
  assert.match(
    templateOf(overview),
    /class="metric-value metric-value--path"[\s\S]*?当前实例目录/,
    '实例目录仍要出现在 title 里——一个信息都不许丢',
  );
});

// --- 套餐用量 -------------------------------------------------------------
//
// 设计稿 `.usage-grid` 是**固定三列**（不是自适应）。1040 宽版内容列约
// 790px，`auto-fill minmax(170px,1fr)` 会排成四列，一家的余额与额度条被摊薄。

test('套餐用量卡固定三列，服务商分块按设计稿的 6px 圆角 + 10px 内边距', () => {
  assert.equal(effectiveDeclaration(['plan-grid'], RULES, 'grid-template-columns'), 'repeat(3, minmax(0, 1fr))');
  assert.equal(effectiveDeclaration(['plan-provider'], RULES, 'border-radius'), '6px');
  assert.equal(effectiveDeclaration(['plan-provider'], RULES, 'padding'), '10px');
});

test('额度行是设计稿的一行栅格「名称 22px | 进度条 1fr | 百分比 auto」，不是两行', () => {
  assert.equal(
    effectiveDeclaration(['plan-tier-row'], RULES, 'grid-template-columns'),
    '22px minmax(0, 1fr) auto'
  );
  assert.equal(effectiveDeclaration(['plan-bar'], RULES, 'height'), '6px');
  // 百分比在条外右对齐；曾经它被绝对定位压在条中央（`.plan-bar-percent`），
  // 那条规则连同它的 10px 条高一起是为了「条内能塞下文字」才存在的。
  assert.equal(effectiveDeclaration(['plan-bar-percent'], RULES, 'position'), null);
  assert.ok(overview.includes('class="plan-tier-value"'), '模板应渲染条外百分比');
});

test('余额区总额是 14px 粗体主读数，赠金 / 充值降到第二行 10px', () => {
  assert.equal(effectiveDeclaration(['plan-balance-total'], RULES, 'font-size'), '14px');
  assert.equal(effectiveDeclaration(['plan-balance-total'], RULES, 'font-weight'), '700');
  assert.equal(effectiveDeclaration(['plan-balance-label'], RULES, 'font-size'), '10px');
  assert.equal(effectiveDeclaration(['plan-balance-detail'], RULES, 'font-size'), '10px');
});

// --- 侧栏 -----------------------------------------------------------------
//
// 设计稿底部是一行：标签 + 版本徽标 + 26px 方钮 + 34px 拨杆。主题开关是
// **拨杆**（本体无图标，只有一颗 14px 圆点），不是「月亮 / 太阳」图标按钮。

test('主题开关是 34px 拨杆、无图标；深色态由 aria-pressed 表达而不是图标', () => {
  // 拨杆与图标按钮的区别有两处（宽 34 而非 26、体内无图标），其中「体内无
  // 元素」是结构性的、不能用层叠工具查（元素有没有子节点不是 CSS 声明），
  // 所以这两条判据直接看 scoped 块。
  const style = scopedStyle(sidebar);
  const rule = (cls) => {
    const m = style.match(new RegExp(`\\.${cls}\\s*\\{([^}]*)\\}`));
    assert.ok(m, `侧栏 scoped 块里应有一条 .${cls} 规则`);
    return m[1];
  };
  const toggle = rule('sidebar__theme-btn');
  assert.match(toggle, /width:\s*34px/);
  assert.match(toggle, /border-radius:\s*11px/);
  // 图标按钮与拨杆的形状与语义都不同：前者像「进设置」，后者读得出「这是个开关」。
  assert.doesNotMatch(style, /\.sidebar__theme-btn\s+\.el-icon/);
  assert.doesNotMatch(sidebar, /sidebar__theme-btn[\s\S]{0,200}<el-icon>/);
  assert.match(sidebar, /:aria-pressed="theme === 'dark'"/);
  // 深色态把整条染成强调色、圆点滑到右端（状态属性选择器，工具同样分不清）。
  assert.match(style, /\.sidebar__theme-btn\[aria-pressed='true'\]\s*\{[^}]*background:\s*var\(--accent-strong\)/);
  assert.match(style, /\.sidebar__theme-btn\[aria-pressed='true'\]::before\s*\{[^}]*transform:\s*translateX\(12px\)/);
});

test('底部一行固定三个方形控件：收起 / 刷新 26×22，标签与版本徽标可压缩', () => {
  assert.equal(effectiveDeclaration(['sidebar__icon-btn'], RULES, 'width'), '26px');
  assert.equal(effectiveDeclaration(['sidebar__icon-btn'], RULES, 'height'), '22px');
  assert.equal(effectiveDeclaration(['sidebar__footer-label'], RULES, 'white-space'), 'nowrap');
  // 三个控件：收起开关（2026-10-07 从品牌行挪来）+ 刷新 + 主题拨杆。
  assert.equal((sidebar.match(/class="sidebar__icon-btn"/g) || []).length, 2, '收起与刷新共用 26×22 方钮');
  assert.equal((sidebar.match(/class="sidebar__theme-btn"/g) || []).length, 1);
});

test('收起开关在底部一行，排在刷新之前；品牌行不再有按钮', () => {
  // 2026-10-07 用户要求：收起开关从品牌行右端移到底部（覆盖设计稿——稿子的
  // `.sidebar-footer` 只有标签/版本/刷新/主题四个元素，`.sidebar-toggle` 画在
  // `.brand` 右端）。判据钉的是**位置与顺序**这两件事实。
  const collapseAt = sidebar.indexOf(':aria-expanded="!collapsed"');
  const refreshAt = sidebar.indexOf('检查桌面端是否有新版本');
  const footerAt = sidebar.indexOf('class="sidebar__footer"');
  assert.ok(collapseAt > 0, '应有收起开关');
  assert.ok(collapseAt > footerAt, '收起开关必须落在 .sidebar__footer 之后');
  assert.ok(collapseAt < refreshAt, '收起开关排在刷新之前（控件组的第一个）');
  // 品牌行只剩 logo 与文字：`.brand__toggle` 连同它的绝对定位规则一起没了。
  // 判据不许被注释里的解释文字命中（那里正是在说明「为什么删」），所以模板段
  // 看有没有按钮、样式段先剥注释再看还有没有那条规则。
  const brandBlock = sidebar.slice(
    sidebar.indexOf('class="brand"'),
    sidebar.indexOf('class="sidebar__nav"'),
  );
  assert.doesNotMatch(brandBlock, /<button/, '品牌行不该再有按钮');
  assert.doesNotMatch(stripComments(scopedStyle(sidebar)), /brand__toggle/, '`.brand__toggle` 的规则已删');
  assert.doesNotMatch(themeCss, /\.brand\s*\{[^}]*position:\s*relative/, '只为绝对定位开关存在的定位要跟着删');
});

test('收起态底部竖排三个控件（64px 侧栏横排必然溢出到主区）', () => {
  // 后代 + 类组合选择器，层叠工具按主语匹配时只看到 `.sidebar__footer`，
  // 问不出 `is-collapsed` 那一半，所以这条直接看选择器文本。
  assert.match(
    themeCss,
    /\.sidebar\.is-collapsed \.sidebar__footer\s*\{[^}]*flex-direction:\s*column/,
  );
  assert.match(themeCss, /\.sidebar\.is-collapsed \.version-badge\s*\{\s*display:\s*none/);
});

test('品牌主名 + 副名独占品牌行（收起开关搬走后不再需要给谁让位）', () => {
  assert.equal(effectiveDeclaration(['brand__name'], RULES, 'font-size'), '15px');
  assert.equal(effectiveDeclaration(['brand__name'], RULES, 'text-overflow'), 'ellipsis');
});

test('鲸鱼图标放大到 42px，且 `<img>` 的尺寸属性与 CSS 相等', () => {
  // 2026-10-08 用户截图指过来：「黑鲸icon放大」。34px 与 15px 主名 + 11px 副名
  // 那一摞文字等高，读起来是「配图」而不是品牌主体。
  assert.equal(effectiveDeclaration(['brand', 'img'], RULES, 'width'), '42px');
  assert.equal(effectiveDeclaration(['brand', 'img'], RULES, 'height'), '42px');
  // `flex: 0 0 <同值>` 少一条，图标就在 flex 行里被文字挤扁——图会变形而不报错。
  assert.match(stripComments(themeCss), /\.brand img\s*\{[^}]*flex:\s*0 0 42px/);

  // **属性与 CSS 必须相等**：CSS 只覆盖绘制，`width` / `height` 属性是给读屏与
  // 布局的第一手尺寸，两处各写一遍的话迟早漂——漂了之后无障碍文本念出的尺寸
  // 与眼睛看到的不一样，而没有任何检查会响。
  const attr = /<img src="\/whale-icon\.png" alt="" width="(\d+)" height="(\d+)"/.exec(sidebar);
  assert.ok(attr, 'SideBar 的鲸鱼图标应带 width / height 属性');
  assert.equal(attr[1], '42', '`<img width>` 与 theme.css 的 `.brand img` 不一致');
  assert.equal(attr[2], '42', '`<img height>` 与 theme.css 的 `.brand img` 不一致');
  // 收起态 64px 侧栏减去 8px 横向内边距后内容盒 48px，图标放得下吗？
  assert.ok(42 <= 64 - 16, '图标在收起态的侧栏内容盒里放不下（图标会比侧栏还宽）');
});

/** `:root` 里某个档位 token 的字面值（排版 / 控件档位只定义一次，两套主题共用）。 */
function tokenValue(name) {
  const m = new RegExp(`(?:^|[;{\\s])${name}:\\s*([^;]+);`).exec(stripComments(themeCss));
  assert.ok(m, `theme.css 的 :root 里缺 ${name}`);
  return m[1].trim();
}

test('概览卡头的主操作放大到 30px / 13.5px，图标槽跟着抬', () => {
  // Element Plus 的 `size="small"` 是 24px / 12px —— 与 17px 的块标题、20px 的
  // 版本号并排时读起来像脚注，而这两枚恰恰是那一屏唯一的**主操作**。
  // 2026-10-08 起这份规格按**角色**收进 theme.css 的 `.btn-action`，写在任何
  // 一个面板的 scoped 块里都只有那一页是这一档。
  const css = stripComments(themeCss);
  const size = /\.btn-action\s*\{([^}]*)\}/.exec(css);
  assert.ok(size, '应有共用的动作按钮规格 .btn-action');
  assert.match(size[1], /height:\s*var\(--action-h\)/);
  assert.match(size[1], /font-size:\s*var\(--fs-action\)/);
  // 只抬盒高不抬横向内边距，两枚挤在一起像被人按扁了——横向要一起走。
  assert.match(size[1], /padding:\s*0 15px/);
  assert.equal(tokenValue('--action-h'), '30px');
  assert.equal(tokenValue('--fs-action'), '13.5px');
  // 图标槽同理。`.entity-action` 那种列表行里的圆形图标按钮**刻意不挂这个类**：
  // 那是「紧凑动作」，另一个角色，由自己的 26px 规则管。
  assert.match(css, /\.btn-action \.el-icon\s*\{\s*font-size:\s*15px/);

  // **七枚动作按钮都要挂上这个类**，一处漏挂就退回 EP 自己的尺寸。
  // 判据分两层：① 每个面板挂了几处（数「类」而不是数 `class="…"` 那个整串）；
  // ② 三枚**文案不歧义**的逐个点名。
  // 不按文案去找按钮是因为「工作台」既在按钮正文里、也在 `:title` 与另外两枚
  // 按钮的正文里（「工作台窗口」），`includes` 一数就是四枚——判据会变成掷骰子。
  const expected = {
    'shell/OverviewPanel.vue': 4, // 工作台 / 官网网页版 / 刷新 / 查看详情
    'skills/SkillsPanel.vue': 2, // 浏览 topic / 安装
    'plugins/PluginsPanel.vue': 1, // 刷新数据
  };
  for (const [rel, n] of Object.entries(expected)) {
    const tpl = templateOf(readFileSync(resolve(SRC, rel), 'utf8'));
    assert.equal(
      (tpl.match(/\bbtn-action\b/g) || []).length,
      n,
      `${rel} 应当恰好有 ${n} 处 btn-action（多出来的是漏收的新按钮，少了的是被摘掉的）`,
    );
  }
  // 点名三枚：按**它们独有的 `@click` 绑定**找开标签，而不是按可见文案。
  // 文案会重名（「工作台」还在 `:title` 与另外两枚按钮的正文里），而 `@click`
  // 的值在这个面板里只出现一次，落到哪一枚没有歧义。
  for (const [rel, click] of [
    ['plugins/PluginsPanel.vue', '@click="searchCatalog({ force: true, loud: true })"'],
    ['skills/SkillsPanel.vue', "aria-label=\"打开 GitHub dsh-skill topic\""],
    ['skills/SkillsPanel.vue', '@click="installSkill"'],
  ]) {
    const tpl = templateOf(readFileSync(resolve(SRC, rel), 'utf8'));
    const at = tpl.indexOf(click);
    assert.ok(at > 0, `${rel} 里找不到 ${click}`);
    const open = tpl.lastIndexOf('<el-button', at);
    const tag = tpl.slice(open, tpl.indexOf('>', open) + 1);
    assert.match(tag, /\bbtn-action\b/, `${rel} 里 ${click} 那枚按钮没挂 btn-action`);
  }
  // 「工作台」那两枚带 `:class` 绑定，静态 class 与它并存是 Vue 的正常用法——
  // 顺带钉住，免得有人以为两者互斥而删掉静态那个。
  assert.match(overview, /class="btn-action"\s*\n\s*:class="\{/);

  // **不许有面板自己再写一份盒高**——那正是「各挑一个」的复发口。
  const scopedBlocks = [...readdirSync(SRC, { recursive: true, withFileTypes: true })]
    .filter((e) => e.isFile() && e.name.endsWith('.vue'))
    .map((e) => [e.name, readFileSync(resolve(SRC, e.parentPath ?? e.path, e.name), 'utf8')]);
  const withLocalHeight = scopedBlocks
    .filter(([, t]) => t.includes('.btn-action') && /btn-action[^}]*height/.test(t))
    .map(([n]) => n);
  assert.deepEqual(withLocalHeight, [], '不该有面板在 scoped 块里另写 btn-action 的盒高');
});

test('技能页的两种标题都在档位上，且输入行与同行按钮等高', () => {
  // 「社区资源」原先是 `<span class="card-title">`，而 `.card-title` 在本仓**没有
  // 对应规则**（设计稿里有、实现里从来没接），字号直接继承 body 的 14px——同一张
  // 卡里比「技能社区」还小。改成 `<h2>` 后 `.card h2` 那一份自动生效。
  const tpl = templateOf(skills);
  assert.match(tpl, /<h2>社区资源<\/h2>/, '「社区资源」应是 <h2>，让 `.card h2` 生效');
  assert.doesNotMatch(tpl, /class="card-title"/, '`.card-title` 没有对应规则，别再用它当卡头标题');
  // 卡内小节标题（技能社区 / 手动安装）与 `.card h3` 同档。
  assert.equal(
    effectiveDeclaration(['community-title'], RULES, 'font-size'),
    'var(--fs-subtitle)',
  );
  // 输入框必须跟着按钮一起压：只压按钮的话按钮比输入框矮 2px，一高一矮看着像没对齐。
  assert.match(
    stripComments(themeCss),
    /\.install-row \{[^}]*--el-component-size:\s*30px/,
    '.install-row 应把输入框压到与动作按钮同一档',
  );
});

// --- 系统健康 -------------------------------------------------------------
//
// 设计稿 `card-caption health-ok">全部正常`。汇总句只数异常格（bad / warn）：
// 「尚未读取」「暂无」是中性态，算进异常会让一张健康的卡写出「1 项异常」。

test('系统健康卡头有一句汇总，且异常态翻红', () => {
  assert.match(controlTower, /healthCaption/);
  // 异常判定落在 `cell()` 里而不是 caption 自己：caption 与下方四行必须同源
  // （分开遍历两份数据一旦不同步，汇总句就会和实际行对不上）。
  assert.match(controlTower, /bad:\s*tone === 'bad' \|\| tone === 'warn'/);
  assert.equal(effectiveDeclaration(['diag-card__aside', 'diag-card__aside--bad'], RULES, 'color'), 'var(--el-color-danger)');
});

// --- 内核版本页 -----------------------------------------------------------
//
// 设计说明 §2：卡片标题是「**官方版本**」，标题 + 最近检查时间 + 两个图标操作
// 必须同一行且标题不换行（换行会遮住 tooltip）。

test('发布列表卡头叫「官方版本」而不是「npm 发布」，标题不换行', () => {
  assert.match(versionsPanel, /<span class="official-version-title">官方版本<\/span>/);
  assert.doesNotMatch(versionsPanel, />npm 发布</);
  assert.equal(effectiveDeclaration(['list-head-with-logo'], RULES, 'flex-wrap'), 'nowrap');
  assert.equal(effectiveDeclaration(['official-version-title'], RULES, 'white-space'), 'nowrap');
});

test('两个动作收成 icon-only（带 aria-label），卡头给出最近检查时间', () => {
  assert.match(versionsPanel, /aria-label="检查官方版本"/);
  assert.match(versionsPanel, /aria-label="打开发布页"/);
  assert.match(versionsPanel, /\{\{ checkedLabel \}\}/, '卡头应渲染 checkedLabel');
  // 没成功检查过一次时整句不渲染，而不是写「尚未检查」——下面那句空态已经说了。
  assert.match(versionsPanel, /const checkedLabel = computed\(\(\) => \{[\s\S]*?return age \? `最近检查 \$\{age\}` : '';/);
});

test('空发布列表不参与「吃掉剩余高度」，已安装空态垂直居中', () => {
  assert.match(versionsPanel, /release-list-box:has\(> \.release-list > \.muted:only-child\)/);
  assert.match(versionsPanel, /installed-list:has\(> \.el-empty\)/);
  assert.equal(effectiveDeclaration(['installed-list', 'el-empty'], RULES, 'justify-content'), 'center');
});

// --- 列表空态换行 ---------------------------------------------------------
//
// Element Plus 的 `.el-empty__description p` 不换行，而 `.entity-list` 有
// `overflow: hidden`：两列布局下左栏约 360px，26 字的说明被裁掉后半句，
// 看着像「插件」两个字后面渲染坏了。

test('列表空态描述允许换行且限宽在容器内（插件页 / 技能页共用这一条）', () => {
  const tokens = ['entity-list', 'is-empty', 'el-empty__description', 'p'];
  assert.equal(effectiveDeclaration(tokens, RULES, 'white-space'), 'normal');
  assert.equal(effectiveDeclaration(tokens, RULES, 'max-width'), '100%');
  assert.equal(effectiveDeclaration(tokens, RULES, 'margin'), '0 auto');
  // 内边距：0 的话文字贴着虚线框边。
  assert.equal(effectiveDeclaration(['entity-list', 'is-empty', 'el-empty'], RULES, 'padding'), '22px 14px');
});

// --- 功能齐备（入口不减少）-------------------------------------------------
//
// 用户 2026-10-07 明确「原本的功能不能丢失，要保证功能齐备」。对齐设计稿时
// 唯一一次删入口是概览页的「查看日志」按钮——它与「系统健康 → 日志系统」
// 指向同一件事，行为还不一致（前者直接开独立窗口，后者走弹层）。
// 设计说明 §4 对独立窗口的要求是「日志查看 | LogModal.vue | 保留刷新、折叠
// 侧栏和独立日志窗口入口」，这三项都在弹层里，所以入口没丢。这条判据钉的
// 就是那三项——有人日后把它们删了，这里会红。

// 设计说明 §独立窗口把十一个窗口 / 弹窗逐个列了出来，逐一标了「开发要求」。
// 它们不像概览页的卡片那样显眼：删掉一个入口往往只表现为「某个菜单项不见了」，
// 而那一行通常还有别的入口接着，肉眼扫不出来。这条判据把「文件在 + 还有调用方」
// 一起钉住——只在文件树上存在、没有任何地方挂载的弹窗，和不存在是一回事。
const WINDOW_ENTRY_POINTS = [
  ['ui/src/usage/UsageWindow.vue', '模型用量'],
  ['ui/src/subscription/SubscriptionWindow.vue', '套餐用量'],
  ['ui/src/official-chat/OfficialChatTabs.vue', '官网对话'],
  ['ui/src/logs/LogModal.vue', '日志查看'],
  ['ui/src/shell/ProgressOverlay.vue', '长任务进度'],
  ['ui/src/plugins/PrecheckDialog.vue', '插件预检'],
  ['ui/src/incidents/IncidentModal.vue', '启动故障'],
  ['ui/src/diagnostics/DiagnosisShell.vue', '诊断页面'],
  ['ui/src/diagnostics/SnapshotRestoreDialog.vue', '环境回退确认'],
  ['ui/src/diagnostics/SnapshotCard.vue', '环境回退入口'],
  ['ui/src/migration/MigrationPrompt.vue', '迁移提示'],
];

test('设计说明 §独立窗口 列出的十一个窗口 / 弹窗都在，且都还有入口', () => {
  for (const [rel, label] of WINDOW_ENTRY_POINTS) {
    // rel 是**相对仓库根**的路径（判据表照抄说明文档里的写法），不要再拼 SRC。
    assert.ok(existsSync(resolve(rel)), `${label}：缺 ${rel}`);
    const name = rel.split('/').pop().replace('.vue', '');
    const ownDir = resolve(rel, '..');
    const refs = [];
    for (const file of [...srcScriptFiles(), ...cssFiles()]) {
      // **同目录的不算引用**。组件自家的 store 模块（`usage/usage.js` 引用
      // `UsageWindow`）只是它自己的状态容器，不是入口——第一版判据没排除它，
      // 结果把 main.js 里的挂载点整段摘掉它照样绿（反向验时才发现）。
      // 入口必须来自这个组件**所在目录之外**：main.js / App.vue / 某个面板。
      if (resolve(file, '..') === ownDir) continue;
      if (readFileSync(file, 'utf8').includes(name)) refs.push(file);
    }
    assert.ok(refs.length > 0, `${label}（${name}）在自身目录之外没有任何地方挂载它，等于没有入口`);
  }
});

test('日志弹层保留刷新、折叠侧栏与独立窗口入口（设计说明 §4 的硬要求）', () => {
  const modal = readFileSync('ui/src/logs/LogModal.vue', 'utf8');
  // 独立窗口入口。
  // 必须是**调用**（`openLogWindow(...)`）而不是裸引用 `openLogWindow`：函数
  // 搬去共享层之后带了 `name` 形参，裸引用会把 MouseEvent 当文件名发出去，
  // 而 `[object PointerEvent]` 不含路径分隔符、能过 validate_log_name，于是
  // 窗口标题会真的变成「日志 · [object PointerEvent]」。clickHandlerArity 独立
  // 守这条，这里只是顺带要求写成调用，不单独重复一份断言。
  assert.match(modal, /@click="openLogWindow\(/, '弹层要有独立日志窗口入口（必须是调用）');
  // 命令调用搬到了共享层（logs.js），弹层只调用它。钉住「只有一份实现」：
  // 两个入口各写一份 invoke，迟早一个改了另一个没改，于是又回到「同一屏两个
  // 日志入口、点开结果不一致」——那正是 2026-10-07 已经被用户点掉一次的状态。
  assert.match(
    modal,
    /import\s*\{[^}]*\bopenLogWindow\b[^}]*\}\s*from\s*'\.\/logs\.js'/,
    '弹层必须从 logs.js 引入 openLogWindow，而不是自己实现一份'
  );
  assert.doesNotMatch(
    modal,
    /open_log_window/,
    '开窗命令只在 logs.js 发一次；弹层里再写一份就是两个入口两种行为'
  );
  assert.match(
    readFileSync('ui/src/logs/logs.js', 'utf8'),
    /invoke\('open_log_window'/,
    '独立窗口应走后端命令'
  );
  // 刷新
  assert.match(modal, /@click="loadActiveLog"/, '弹层要有重新读取');
  // 折叠侧栏
  assert.match(modal, /LogSidebar/, '分类侧栏仍在');
  assert.match(modal, /collapsed|collapsedManual|sidebarCollapsed/, '侧栏仍可手动收起');
});

test('概览页不再有第二个日志入口（同一屏两个日志入口且行为不一致）', () => {
  // 这条钉的是「不要把它加回来」，理由写在 OverviewPanel 那段模板注释里。
  assert.doesNotMatch(overview, /@click="showLogs/);
  // 而系统健康那一行仍然在：它才是概览页的日志入口。
  assert.match(controlTower, /showLogs\(\)/);
});

// --- 数据迁移的入口形态 -----------------------------------------------------
//
// 维护者 2026-10-07 决定：按设计稿独立成侧栏常驻菜单，放在「设置」下面；
// 整个功能在 v0.6.0 之后整体移除。
//
// 设计稿自身有张力：2550 行的侧栏是无条件菜单项，而说明文档 §侧栏 写
// 「数据迁移入口受旧数据发现状态控制…不应把条件入口强行固定成常驻功能」。
// 这里以维护者的决定为准（**它是覆盖，不是两边都对**），理由与代价记在
// `SideBar.vue` 那段注释里——过渡期内它是随时要能打开的正式功能，而入口必须
// 先存在，才谈得上"到期删干净"。

test('侧栏「数据迁移」是系统组里的常驻菜单项，紧跟在「设置」之后', () => {
  const groups = sidebar.slice(sidebar.indexOf('const MENU_GROUPS'), sidebar.indexOf('// 桌面端版本号'));
  // 系统组是最后一组
  const sys = groups.slice(groups.lastIndexOf("label: '系统'"));
  const settingsAt = sys.indexOf("id: 'settings'");
  const migrationAt = sys.indexOf("id: 'migration'");
  assert.ok(settingsAt >= 0, '系统组应有「设置」');
  assert.ok(migrationAt >= 0, '系统组应有「数据迁移」');
  assert.ok(migrationAt > settingsAt, '「数据迁移」必须排在「设置」下面');
  // **常驻**：不再有 show / 条件渲染。此前它是条件项，只在
  // 「从未迁移过 + 扫到遗留数据」时出现——那个条件在多数用户机器上不成立，
  // 于是入口在侧栏凭空消失，而设置页那张卡成了唯一入口。
  assert.doesNotMatch(sidebar, /migrationStore/);
  assert.doesNotMatch(sys, /show:\s*\(\)/);
});

test('设置页不再有独立「数据迁移」大卡，只留一行入口（设计稿 2844 行）', () => {
  const panel = readFileSync('ui/src/shell/SettingsPanel.vue', 'utf8');
  // 入口仍在 —— 设计说明 §侧栏：「迁移完成后，设置页仍保留进入迁移功能的入口」。
  assert.match(panel, /@click="store\.activePanel = 'migration'"/);
  // 但它不再是那张默认折叠、带展开态的大卡。
  assert.doesNotMatch(panel, /migrationExpanded/);
  assert.doesNotMatch(panel, /card-head-toggle/);
});

test('v0.6.0 移除数据迁移的计划已登记在两份 AGENTS.md（删干净要动四处）', () => {
  for (const rel of ['ui/AGENTS.md', 'AGENTS.md']) {
    const text = readFileSync(rel, 'utf8');
    assert.match(text, /v0\.6\.0/, `${rel} 应登记 v0.6.0 移除计划`);
  }
  const ui = readFileSync('ui/AGENTS.md', 'utf8');
  // 四块都要在清单里：只删前端菜单、Rust 侧命令还挂着不算移除。
  assert.match(ui, /ui\/src\/migration\//, '清单缺前端模块');
  assert.match(ui, /src-tauri\/src\/migration\//, '清单缺 Rust 模块');
  assert.match(ui, /capabilities|commands\.rs/, '清单缺命令注册');
  assert.match(ui, /migrationPanel\.test\.js|misplacedHome\.test\.js/, '清单缺判据');
  // 两处同名不同职责的必须点名不要删。
  assert.match(ui, /legacy_migration_target/, '应点名 legacy_migration_target');
  assert.match(ui, /store_relocate/, '应点名 store_relocate');
});

// --- 控制塔落位 -----------------------------------------------------------
//
// 2026-10-07 用户改过一次：末行是「需要关注 | 最近操作」**左右并排**，
// 「需要关注」不再独占首行整宽。首行让给「当前内核 / 系统健康」——那两张
// 才是进概览最常看的。

// **末行左右对调过一次**（2026-10-08）：「最近操作」挪到左半栏、「需要关注」到
// 右半栏。起因是「没有待处理项」这个最常见的状态下「需要关注」整张卡不渲染，
// 于是它原本所在的左半栏整块空着——换过来之后异常状态下留白在右，正常状态下
// 是「最近操作（左） | 需要关注（右）」。下面钉的是**对调之后**的落位。
test('控制塔三张塔卡用 order 落位；健康卡在右半栏，最近操作在左、需要关注在右', () => {
  for (const [cls, order, col] of [
    ['diag-card--health', '2', '2'],
    ['diag-card--activity', '4', '1'],
    ['diag-card--attention', '5', '2'],
  ]) {
    assert.equal(effectiveDeclaration([cls], RULES, 'order'), order, `${cls} 的 order`);
    assert.equal(effectiveDeclaration([cls], RULES, 'grid-column'), col, `${cls} 的 grid-column`);
  }
  // 「需要关注」缺席时行号会整体前移，所以三条都得是 order 而不是 grid-row。
  for (const cls of ['diag-card--health', 'diag-card--attention', 'diag-card--activity']) {
    assert.equal(effectiveDeclaration([cls], RULES, 'grid-row'), null, `${cls} 不该用 grid-row 落位`);
  }
  assert.ok(diagnosticsCss.includes('.diag-rows--grid'), '健康行列表应在 grid 里');
});

test('概览主栅格：末行吃满剩余纵向空间', () => {
  // 2026-10-08 用户要求「两个功能块都增加高度，用满纵向高度」。此前是
  // `align-content: start` 且不写行高，末行两张卡有多高就多高，内容少的时候
  // 下半屏空一大片。
  assert.equal(
    effectiveDeclaration(['overview-grid'], RULES, 'grid-template-rows'),
    'auto auto minmax(min-content, 1fr)',
    '末行必须是 min-content 起底、1fr 吃剩余——写成 minmax(0, 1fr) 会在空间不够时'
      + '把卡压扁、内容溢出自己的格子，看着像布局坏了'
  );
  assert.equal(effectiveDeclaration(['overview-grid'], RULES, 'flex'), '1');
  // 百分比高度要解得开，祖先 `.panel` 必须是确定高度那一层。
  assert.equal(effectiveDeclaration(['panel'], RULES, 'min-height'), '100%');
});

test('行尾动作是带边框、带 icon 的 badge，不是纯文字箭头', () => {
  // 2026-10-08 用户要求「操作按钮高亮、加 icon、badge 边框」。此前那格是一段
  // 纯文字（› / 查看），在一列不能点的读数里完全看不出能不能点。
  const css = readFileSync('ui/src/diagnostics/diagnostics.css', 'utf8');
  const cta = css.match(/\.diag-row__cta \{([^}]*)\}/);
  assert.ok(cta, '必须能找到 .diag-row__cta 规则');
  assert.match(cta[1], /border:\s*1px solid var\(--accent-line\)/, 'badge 必须有边框');
  assert.match(cta[1], /background:\s*var\(--accent-fill\)/, 'badge 必须有填色（高亮）');
  assert.match(cta[1], /border-radius:\s*999px/, 'badge 形态是胶囊');
  // 展开箭头**不该**复用这个类：它标的是「还有没有」，不是动作。
  assert.match(css, /\.diag-row__caret \{/, '展开箭头要有独立的类名');
  assert.doesNotMatch(
    css,
    /\.diag-row__arrow/,
    '动作与展开指示不能共用一个类名：改其中一种会顺手改到另一种'
  );
  // 三处行尾动作都要渲染成 badge，且都带 icon。
  const tower = readFileSync('ui/src/diagnostics/ControlTower.vue', 'utf8');
  // 三张塔卡的行尾动作各一枚：需要关注「处理」、系统健康的提示、最近操作「查看」。
  const badges = (tower.match(/class="diag-row__cta"/g) || []).length;
  assert.equal(badges, 3);
  // **三枚都必须带 icon**：只断言「有一枚带」的话，删掉另外两枚照样绿——
  // 而删掉图标正是最容易发生的那种改动（换个 icon 名、或复制粘贴时漏掉）。
  const icons = (tower.match(/<el-icon aria-hidden="true"><ArrowRight \/><\/el-icon>/g) || []).length;
  assert.equal(icons, badges, '每一枚 badge 都要带 icon，不能只给一枚');
  assert.match(tower, /import \{ ArrowRight \} from '@element-plus\/icons-vue'/);
});

test('概览主栅格的内核卡与用量卡让出首行给它们自己', () => {
  assert.equal(effectiveDeclaration(['kernel-card'], RULES, 'order'), '1');
  assert.equal(effectiveDeclaration(['usage-card'], RULES, 'order'), '3');
});

// --- 叠色 token（主题适配）-------------------------------------------------
//
// 2026-10-07 用户要求核对弹窗与副窗的主题适配，根因是全仓 40 处写死的
// `rgba(255,255,255,…)` 与 5 处 `rgba(0,0,0,…)`：暗色下叠白是对的，浅色下
// 「白底上的白 hover」「白底上的白进度条轨道」直接消失。同一个视觉意图必须
// 走同一个变量，两套主题各给值。

const OVERLAY_TOKENS = ['--overlay-faint', '--overlay-soft', '--overlay-strong', '--surface-sunken'];

test('四个叠色 token 在浅色与暗色下都有定义（`:root` 与 `html.dark` 各一份）', () => {
  const css = themeCss;
  // 按**选择器块**切，不按 `indexOf('html.dark')`：文件开头的注释里就提到了
  // `html.dark` 这个字样，第一次出现远早于真正的选择器块，切出来长度是负的。
  const darkStart = css.indexOf('\nhtml.dark {');
  assert.ok(darkStart > 0, 'theme.css 里应有 html.dark 选择器块');
  const light = css.slice(0, darkStart);
  const dark = css.slice(darkStart);
  for (const token of OVERLAY_TOKENS) {
    assert.match(light, new RegExp(`${token}:\\s*[^;]+;`), `${token} 缺浅色定义`);
    assert.match(dark, new RegExp(`${token}:\\s*[^;]+;`), `${token} 缺暗色定义`);
    // 「有定义」还不够：批量化替换叠色时**定义行自己**也会被替掉，于是
    // `--overlay-strong: var(--overlay-strong)` 这种自引用照样通过上面那条
    // （它当然「有定义」），而那会让这个变量彻底失效。这里钉住值必须是字面量。
    for (const [label, block] of [['浅色', light], ['暗色', dark]]) {
      const m = new RegExp(`${token}:\\s*([^;]+);`).exec(block);
      assert.doesNotMatch(
        m[1],
        /var\(/,
        `${label}的 ${token} 指向了另一个变量（自引用会让它彻底失效）：${m[1]}`
      );
    }
  }
});

test('生产 CSS / Vue 里不再有硬编码的白叠色（注释不算）', () => {
  // 注释里允许出现字面量：那里正是在解释「为什么这一处当年写死了」。用
  // `stripComments` 剥块注释而不是逐行判断——那段解释写在 `/* */` 中间、
  // 不在行首，逐行方案会漏掉它，判据于是红在自己文件里解释「别写死」的
  // 那句话上。行注释另按行剥。
  const files = [
    'ui/src/theme.css',
    'ui/src/diagnostics/diagnostics.css',
    'ui/src/kernel/VersionsPanel.vue',
    'ui/src/migration/MigrationPanel.vue',
    'ui/src/subscription/SubscriptionWindow.vue',
    'ui/src/usage/UsageWindow.vue',
    'ui/src/plugins/PrecheckDialog.vue',
    'ui/src/shell/OverviewPanel.vue',
  ];
  const hardcoded = /rgba\(255,\s*255,\s*255/;
  for (const rel of files) {
    const text = stripComments(readFileSync(rel, 'utf8'))
      .split('\n')
      .map((line) => line.replace(/\/\/.*$/, ''))
      // 叠色 token 的**定义行**本身就该是字面量——它就是这套值的真相源。
      // 判据要拦的是「在规则里又写了一遍」，不是「这里有定义」。
      .filter((line) => !/^\s*--(?:overlay-(?:faint|soft|strong)|surface-sunken):/.test(line))
      .join('\n');
    const hit = hardcoded.exec(text);
    assert.equal(hit, null, `${rel} 仍有硬编码白叠色：${hit && hit[0]}`);
  }
});

test('生产 CSS / Vue 里不再有硬编码的黑叠色容器底（阴影与 sunken token 除外）', () => {
  const files = [
    'ui/src/theme.css',
    'ui/src/diagnostics/diagnostics.css',
    'ui/src/kernel/VersionsPanel.vue',
    'ui/src/usage/UsageWindow.vue',
    'ui/src/subscription/SubscriptionWindow.vue',
  ];
  for (const rel of files) {
    const text = stripComments(readFileSync(rel, 'utf8'))
      .split('\n')
      .map((line) => line.replace(/\/\/.*$/, ''))
      .join('\n');
    // 阴影（box-shadow / --shadow*）与 token 定义本身不算：它们的黑是阴影该有的黑。
    const hit = /background:\s*rgba\(0,\s*0,\s*0/.exec(text);
    assert.equal(hit, null, `${rel} 仍有硬编码黑叠色背景：${hit && hit[0]}`);
  }
});

// --- 插件页标题旁的第三方来源提示 -----------------------------------------
//
// 2026-10-07 用户要求：原先页头下面那条整宽 notice 收成标题旁的 ⚠ 图标。
// 三条判据分别钉「形态」「同一份文案」「键盘够得着」，缺一条这条提示就会
// 悄悄退化成只有鼠标用户看得见的东西。

test('插件页不再有整宽 notice 条，免责提示收成标题旁的警告图标', () => {
  const panel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
  assert.doesNotMatch(panel, /panel-notice/, '整宽提示条已删除');
  // 规则本身也不能留在 theme.css 里当死 CSS——上一轮删模板引用时漏了样式
  // 的反向案例，这一条一起钉住。
  assert.doesNotMatch(stripComments(themeCss), /\.panel-notice/);

  // 图标挂在「插件」标题旁：`.page-title-row` 是页头标题与图标的同行容器。
  assert.match(panel, /class="page-title-row"/);
  assert.match(panel, /class="head-tip-icon head-tip-icon--warning"/);
  const rowAt = panel.indexOf('class="page-title-row"');
  const titleAt = panel.indexOf('class="page-title"');
  const iconAt = panel.indexOf('head-tip-icon--warning');
  assert.ok(titleAt > rowAt && iconAt > titleAt, '⚠ 必须与标题同在 `.page-title-row` 内');
});

test('警告图标的 tooltip 与可访问名取同一份文案常量', () => {
  const panel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
  // 两处各写一遍字符串，改了一处就会出现「图标说有提示、读屏说没提示」。
  assert.match(panel, /const THIRD_PARTY_NOTICE = '第三方插件由社区提供/);
  assert.match(panel, /:content="THIRD_PARTY_NOTICE"/);
  assert.match(panel, /:aria-label="THIRD_PARTY_NOTICE"/);
  // 文案仍在源码里（不因收成图标而消失）：它现在住在 tooltip 里。
  assert.equal(
    (panel.match(/第三方插件由社区提供，本工具不对其安全性负责，请自行甄别。/g) || []).length,
    1,
    '免责文案在 PluginsPanel 里只应出现一次（常量声明处）',
  );
});

test('警告图标可聚焦，且 focus 态与 hover 同色（hover-only 的信息对键盘用户等于不存在）', () => {
  const panel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
  assert.match(panel, /head-tip-icon--warning[\s\S]{0,200}?tabindex="0"/);

  assert.equal(effectiveDeclaration(['head-tip-icon--warning'], RULES, 'color'), 'var(--warning)');
  // 与 ⓘ 分色：两条提示严重程度不同。hover 也不改成强调蓝，否则会被读成可点。
  assert.equal(
    effectiveDeclaration(['head-tip-icon--warning'], RULES, 'color', { pseudo: ':hover' }),
    'var(--el-color-warning-dark-2, var(--warning))',
  );
  assert.equal(
    effectiveDeclaration(['head-tip-icon--warning'], RULES, 'color', { pseudo: ':focus-visible' }),
    'var(--el-color-warning-dark-2, var(--warning))',
  );
});

test('页头标题行是 flex 且居中对齐（⚠ 与标题不各走各的）', () => {
  assert.equal(effectiveDeclaration(['page-title-row'], RULES, 'display'), 'flex');
  assert.equal(effectiveDeclaration(['page-title-row'], RULES, 'align-items'), 'center');
});

/** HTML 空元素：标签语法上不带结束标签，不能压进标签栈（压了栈永远弹不回来）。 */
const VOID_TAGS = new Set(['img', 'br', 'hr', 'input', 'meta', 'link', 'source']);

/**
 * 按标签配对取出 `.page-head` 那个元素**及其内部**，而不是往后一直切到文件尾。
 *
 * 必须按配对切：此前用 `src.slice(src.indexOf('page-head'))` 把整个余下文件
 * 都算进来了，标签栈里混进后面几十个兄弟元素，深度读数毫无意义。
 *
 * 也必须先剥注释：Vue 模板里到处是解释结构的中文注释，而这些注释**会写出字面量
 * 标签名**（「外面还要再包一层 `<div>`」）。剥晚了配对就会数出一个不存在的
 * `<div>`，块永远配不平。
 */
function pageHeadBlock(src) {
  const text = src.replace(/<!--[\s\S]*?-->/g, '');
  const open = /<(div|header)[^>]*class="[^"]*\bpage-head\b[^"]*"[^>]*>/.exec(text);
  assert.ok(open, '找不到 .page-head 元素');
  const tag = open[1];
  const re = new RegExp(`<\\/?${tag}\\b[^>]*>`, 'g');
  re.lastIndex = open.index;
  let depth = 0;
  let m;
  while ((m = re.exec(text))) {
    if (m[0].startsWith('</')) {
      depth -= 1;
      if (depth === 0) return text.slice(open.index, re.lastIndex);
    } else if (!m[0].endsWith('/>')) {
      depth += 1;
    }
  }
  assert.fail(`.page-head 的 <${tag}> 没有配对的结束标签`);
}

/**
 * `.page-head` 块里，`class` 里带 `name` 的元素所处的标签栈深度。
 *
 * 深度就是「它外面套着几层还没闭合的标签」。返回 null = 块里没有这个元素。
 * 这里的问法是**结构**问题而不是样式问题，所以不走 CSS 层叠工具。
 */
function elementDepth(block, name) {
  const re = /<(\/?)([a-zA-Z][\w-]*)((?:"[^"]*"|'[^']*'|[^>"'])*?)(\/?)>/g;
  const stack = [];
  let m;
  while ((m = re.exec(block))) {
    const [, close, tag, attrs = '', selfClose] = m;
    if (!close) {
      const cls = (attrs.match(/class="([^"]*)"/) || [])[1] || '';
      if (cls.split(/\s+/).includes(name)) return stack.length;
      if (!selfClose && !VOID_TAGS.has(tag.toLowerCase())) stack.push(tag);
    } else {
      stack.pop();
    }
  }
  return null;
}

test('页头标题与说明同属一个子项——否则说明会被 space-between 顶到右缘', () => {
  // 这条是本轮真实踩到的回归：给插件页加标题行时把 `.page-desc` 提到了
  // `.page-head` 的直接子级，`.page-head` 是 `space-between` 的 flex 行，说明立刻
  // 被当成了右侧的「动作」飘到页面右缘。六个页面共用「标题 + 说明包在一个 div 里」
  // 这个结构，别只在一个页面上靠肉眼记。
  //
  // 判据比的是**嵌套深度**而不是 `</div>` 的个数：两种错结构的 `</div>` 计数一样，
  // 数个数那一版判据在真实回归面前是绿的（踩过）。
  assert.equal(effectiveDeclaration(['page-head'], RULES, 'justify-content'), 'space-between');
  for (const rel of [
    'ui/src/shell/OverviewPanel.vue',
    'ui/src/shell/SettingsPanel.vue',
    'ui/src/kernel/VersionsPanel.vue',
    'ui/src/plugins/PluginsPanel.vue',
    'ui/src/skills/SkillsPanel.vue',
    'ui/src/migration/MigrationPanel.vue',
  ]) {
    const head = pageHeadBlock(readFileSync(rel, 'utf8'));
    const descDepth = elementDepth(head, 'page-desc');
    const titleDepth = elementDepth(head, 'page-title');
    const rowDepth = elementDepth(head, 'page-title-row');
    assert.ok(descDepth !== null && titleDepth !== null, `${rel} 的 page-head 缺少标题或说明`);
    if (rowDepth === null) {
      assert.equal(descDepth, titleDepth, `${rel} 的 page-desc 跑到了 page-head 直接子级`);
      continue;
    }
    // 有标题行时：标题裹在标题行里（深一层），说明与标题行**同深**——
    // 同深才说明二者在同一个包裹 div 内，而不是各自成了 `.page-head` 的子项。
    assert.equal(titleDepth, rowDepth + 1, `${rel} 的 page-title 应在 .page-title-row 内`);
    assert.equal(descDepth, rowDepth, `${rel} 的 page-desc 应与 .page-title-row 同属一个包裹元素`);
  }
});

// --- 技能页的卡片划分 -----------------------------------------------------
//
// 2026-10-07 对齐设计稿：原先「手动安装」是「已安装」卡底部的一条虚线分隔，
// 读起来像「装完了顺手在这里补一个」，而它其实是另一件事——包的来源在社区。
// 设计稿（2817-2820 行）是两张卡：已安装 / 社区资源。

const skills = readFileSync('ui/src/skills/SkillsPanel.vue', 'utf8');

const skillsTpl = templateOf(skills);

test('技能页是「已安装 / 社区资源」两张卡，不是一张卡里的分隔线', () => {
  const cards = skillsTpl.match(/class="card [^"]*"/g) || [];
  assert.deepEqual(cards, ['class="card entity-card"', 'class="card community-card"']);
  assert.match(skillsTpl, /<h2>社区资源<\/h2>/);
  // 「手动安装」不再挂在已安装卡里：它是社区资源卡的第二段。
  assert.ok(
    skillsTpl.indexOf('社区资源') < skillsTpl.indexOf('手动安装'),
    '「手动安装」应在「社区资源」卡里',
  );
  assert.ok(
    skillsTpl.indexOf('class="entity-list"') < skillsTpl.indexOf('class="card community-card"'),
    '已安装列表应在社区资源卡之前',
  );
  // 原先那条分隔线随拆卡一起没了。
  assert.doesNotMatch(skillsTpl, /section-divider/);
  assert.doesNotMatch(stripComments(themeCss), /\.install-hint/, '`.install-hint` 已随拆卡删除');
});

test('社区资源卡不画设计稿那张「技能状态」卡（后端没返回那些数字，硬写就是编数据）', () => {
  // 设计稿第一张卡写着「启用状态 4 / 4」「活动视图 已同步」。前者可由列表推算，
  // 后者没有任何后端字段支撑——设计说明「不能从设计稿推断出的内容」里点名了
  // 「未在后端返回的余额、百分比、更新时间、星标数量或插件数量」。宁可不画。
  // 判据只看模板：源文件注释里正是在解释**为什么**不画它。
  assert.doesNotMatch(skillsTpl, /活动视图/);
  assert.doesNotMatch(skillsTpl, /技能状态/);
  // 说明句照实写：面板里**没有**目录查询 / 筛选 / 排序。
  assert.match(skillsTpl, /面板内没有技能目录查询、筛选和排序列表/);
});

test('手动安装有显式按钮，按钮与回车同一个 installSkill，且不挂假 loading', () => {
  const installBlock = skillsTpl.slice(
    skillsTpl.indexOf('class="install-row"'),
    skillsTpl.indexOf('skill-remediation-note'),
  );
  assert.match(installBlock, /@click="installSkill"/);
  assert.match(installBlock, /@keyup\.enter="installSkill"/);
  // `installSkill` 走 `withProgress`（长任务），不经过 `withLoading`：
  // `isLoading('…')` 对它永远 false，挂上去就是一个永远不转的 loading。
  assert.doesNotMatch(installBlock, /:loading="isLoading/);
  assert.match(installBlock, /:disabled="globalBusy \|\| !skillStore\.spec\.trim\(\)"/);
});

test('社区资源卡的样式留在 scoped，且复用全局 `.install-row` 而不是另写一份', () => {
  const style = scopedStyle(skills);
  // theme.css 走反棘轮（只许下调），单页专用的样式不进它。
  assert.doesNotMatch(stripComments(themeCss), /\.community-/);
  for (const cls of ['community-browse-row', 'community-title', 'community-meta', 'skill-remediation-note']) {
    assert.match(style, new RegExp(`\\.${cls}\\s*\\{`), `.${cls} 应在 scoped 块里`);
  }
  assert.match(skills, /<div class="install-row">/);
  assert.doesNotMatch(style, /\.community-install-control/);
});

// --- 同一页里不再有两个名字指同一份东西 ----------------------------------
//
// 2026-10-07 逐页比对设计稿时发现的两处。两处都不是排版问题，是**命名**问题：
// 同一个词在一屏里出现两次，或者两个不同的词指同一件事，用户要靠猜。

const versionsTpl = templateOf(versionsPanel);

test('内核版本页卡头叫「已安装 / N 个版本」，不复述页面标题也不与组标题重复', () => {
  // 设计稿 draft 2669-2673：卡头 = 已安装 / caption 2 个版本。原先卡头写的是
  // 「内核版本」——页面标题已经叫内核版本了，卡头再说一遍等于把整页的名字
  // 复述一次；而这一段真正在讲的是「本机装了哪几个」。
  assert.match(versionsTpl, /<h2>已安装<\/h2>/);
  assert.match(versionsTpl, /个版本<\/span>/);
  // 组内那个 `<h3>已安装</h3>` 一并去掉，否则同一个词连着出现在两行。
  // 判据只数这两个标题标签，不数全文：「已安装」在发布列表里还作为**行内标记**
  // 出现一次（标出这个远端版本本机已经有了），那是另一个含义，不能一起禁掉。
  assert.equal((versionsTpl.match(/<h2>已安装<\/h2>/g) || []).length, 1);
  assert.equal((versionsTpl.match(/<h3>已安装<\/h3>/g) || []).length, 0);
});

const pluginsPanel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
const pluginsTpl = templateOf(pluginsPanel);

test('插件页：外层卡叫「插件管理」，右栏那份远端目录才叫「插件中心」', () => {
  // draft 2746 外层卡 = 插件管理；draft 2775 右栏 = 插件中心（来自 dshfind.com）。
  // 原先外层卡叫「插件中心」、右栏叫「插件仓库」——同一份 dshfind.com 目录，
  // 同一页里两个名字。
  assert.match(pluginsTpl, /<span class="plugin-center-title">插件管理<\/span>/);
  assert.match(pluginsTpl, /<h3 class="section-divider">\s*插件中心/);
  assert.doesNotMatch(pluginsTpl, /插件仓库(?![一-龥])/);
  // dshfind.com 的链接跟着右栏走（它是那份目录的来源，不是整页的来源）。
  assert.match(pluginsTpl, /dshfind\.com\/zh/);
});

test('卡头 ⓘ 是纯图标：可见文字与 tooltip 内容说的是同一件事', () => {
  // 原先 ⓘ 旁边写着「数据来源于 dshfind.com」，而它的 tooltip 里讲的是插件存放
  // 路径与生效规则——鼠标停在字上弹出的是另一段话。改成纯图标后不再错配，
  // 也与技能页 / 概览页的 ⓘ 一致（见「提示收成图标」）。
  assert.match(pluginsTpl, /<el-tooltip[^>]*:content="installTip"[^>]*>\s*<el-icon class="head-tip-icon">/);
  assert.doesNotMatch(pluginsTpl, /plugin-center-source/);
  assert.doesNotMatch(stripComments(themeCss), /\.plugin-center-source/, '`.plugin-center-source` 已随模板删除');
});

// --- 设置页：两栏的卡分别归哪一栏 -------------------------------------------
//
// 2026-10-07 用户指出「左栏两张、右栏四张，左栏空一大截」，拍板按设计稿
// draft 2844 把后三张（数据迁移 / 环境回退点 / 深入排查）合并成「环境回退与
// 诊断」一张——它们同属「环境出问题时才动」，拆成四张只是把一个上下文摊成四段。
//
// 2026-10-08 用户又把那张卡从右栏移到左栏、接在「后台常驻」下方。最终形状是
// **左栏三张（工作台 / 后台常驻 / 环境回退与诊断）、右栏一张（任务通知）**。
//
// 下面这几条盯的不只是「有几张卡」，还有**每一张归哪一栏**。上一版判据只数了
// `.card` 的总数、又只查了标题存在，于是名字里写着「右栏第二张」而实际从未判过
// 第几栏——卡被搬走之后它照样绿（2026-10-08 实测）。所以这里按 `.page-layout__col`
// 的位置切成两段分别断言：总数对、归属也对。
const settingsPanel = readFileSync('ui/src/shell/SettingsPanel.vue', 'utf8');
const settingsTpl = templateOf(settingsPanel);
const snapshotCard = readFileSync('ui/src/diagnostics/SnapshotCard.vue', 'utf8');
const bisectPanel = readFileSync('ui/src/diagnostics/BisectPanel.vue', 'utf8');

/** 设置页模板块按列切开：左栏 / 右栏各自的模板块文本。 */
function settingsColumns() {
  const at = [...settingsTpl.matchAll(/<div class="page-layout__col">/g)].map((m) => m.index);
  assert.equal(at.length, 2, '设置页应是两列');
  return { left: settingsTpl.slice(0, at[1]), right: settingsTpl.slice(at[1]) };
}

/**
 * 设置页每张卡的标题，以及它落在哪一栏。
 *
 * **不能按字面搜标题再判它在哪一栏**：「工作台」这三个字在右栏那张卡里还作为
 * 表单 label 出现（「工作台不在前台才通知」），按字面搜会得出「工作台也在右栏」
 * 的结论。这里改为提取每张卡 `<h2>` 的**标题文字**，再按它在模板块里的位置归栏。
 */
function settingsCardTitles() {
  const at = [...settingsTpl.matchAll(/<div class="page-layout__col">/g)].map((m) => m.index);
  assert.equal(at.length, 2, '设置页应是两列');
  return [...settingsTpl.matchAll(/<h2[^>]*>\s*([^<]+?)\s*</g)].map((m) => ({
    title: m[1].trim(),
    col: m.index < at[1] ? 'left' : 'right',
  }));
}

test('设置页共四张卡，归属与顺序钉死：左三右一', () => {
  // 只数模板块里 `.card` 的直接出现次数：卡片真身是 `<div class="card">`，
  // 组件自带的外框也已删掉（下面那条判据钉着）。
  assert.equal((settingsTpl.match(/<div class="card">/g) || []).length, 4);
  // 整体比对：栏位 + 标题 + 先后顺序一次说清。少比一项，下一次把卡搬回右栏
  // 就又是绿的。
  assert.deepEqual(settingsCardTitles(), [
    { title: '工作台', col: 'left' },
    { title: '后台常驻', col: 'left' },
    { title: '环境回退与诊断', col: 'left' },
    { title: '任务通知', col: 'right' },
  ]);
});

test('「环境回退与诊断」的 caption 走 head-meta + muted', () => {
  const { left, right } = settingsColumns();
  assert.match(left, /<h2>环境回退与诊断<\/h2>/);
  // caption 走既有的 head-meta + muted，不为这一处新增 .card-caption。
  assert.match(left, /出问题时使用/);
  // 右栏只剩「任务通知」一张——它在内核事件流断开时会展开一大段环境说明与告警，
  // 把排查入口压在它下面时，用户要先撞上告警才找得到自己要找的东西。
  assert.equal(
    (right.match(/<div class="card">/g) || []).length,
    1,
    '右栏应只剩「任务通知」一张',
  );
});

test('「环境回退与诊断」是一张卡里的四个行式条目', () => {
  // `.page-list` 是全仓共用的行式列表原语（theme.css），四个条目都落在它里面。
  const listStart = settingsTpl.indexOf('<div class="page-list">');
  assert.ok(listStart > 0, '设置页该有一个 .page-list 列表');
  // 两个组件必须在列表**内部**：它们是 Fragment，行与明细直接落进这个栅格。
  assert.match(settingsTpl.slice(listStart), /<SnapshotCard \/>\s*<BisectPanel \/>/);
  for (const [label, tpl, title] of [
    ['环境回退点', templateOf(snapshotCard), '环境回退点'],
    ['深入排查', templateOf(bisectPanel), '深入排查'],
    ['设置页', settingsTpl.slice(listStart), '启动诊断'],
    ['设置页', settingsTpl.slice(listStart), '数据迁移'],
  ]) {
    // 标题换行排版，所以标题文字前面允许多余空白。
    assert.match(tpl, new RegExp(`class="page-list-title">\\s*${title}`), `${label}：${title} 应是 page-list 的一行`);
  }
  // 数据迁移与启动诊断都是真跳转，不是有内容的展开层。`openStartupDiagnosis`
  // 在 `<script>` 里（模板只调本文件定义的 openStartupRun），所以这一处读全文。
  assert.match(settingsTpl, /@click="openStartupRun"/);
  assert.match(settingsPanel, /openStartupDiagnosis\(/);
  assert.match(settingsTpl, /store\.activePanel = 'migration'/);
});

test('环境回退点与深入排查渲染成行，不再自带 .card 外框', () => {
  // 组件根节点是 Fragment：行是常驻的，明细与告警是它的兄弟节点，直接落进
  // 同一个 `.page-list` 栅格。曾经它们各自是 `<div class="card …">`，
  // 套进共享卡里就会出现卡中卡。
  // 正则收在 `card` 后面不加任何字符：`card-info-tooltip` / `card-head` 是
  // 别的 class，不能一起禁掉（第一版写得太宽，把这两个也判成卡外框了）。
  for (const [label, src, family] of [
    ['环境回退点', snapshotCard, 'snapshot'],
    ['深入排查', bisectPanel, 'bisect'],
  ]) {
    const tpl = templateOf(src);
    assert.doesNotMatch(tpl, /<div class="card(?![\w-])/, `${label} 不应再渲染自己的 .card 外框`);
    assert.match(tpl, /^<template>\s*<div class="page-list-row">/, `${label} 的第一个根节点应是那一行`);
    assert.doesNotMatch(stripComments(themeCss), new RegExp(`\\.${family}-card`));
  }
});

test('收起的只有明细：状态说明与告警常驻在闸门之外', () => {
  // 「收起」省的是那一屏十几行列表，不是状态本身。headline 挪到
  // `.page-list-meta`（次行常驻），drifted 与 conclusion 两条告警不进闸门——
  // 排查跑几分钟时用户唯一能看懂的进度，恰恰不能被藏起来。
  const snapTpl = templateOf(snapshotCard);
  assert.match(snapTpl, /<p class="page-list-meta">\{\{ view \? headline\(view\) : '正在读取…' \}\}<\/p>/);
  assert.doesNotMatch(snapTpl, /v-if="open && drifted"/);
  const bisectTpl = templateOf(bisectPanel);
  assert.match(bisectTpl, /<p class="page-list-meta">\{\{ view \? bisectHeadline\(view\) : '正在读取…' \}\}<\/p>/);
  assert.doesNotMatch(bisectTpl, /v-if="open && view && view\.conclusion"[\s\S]{0,80}show-icon/);
});

test('恢复 / 刷新 / 开始排查 / 停止四个动作都在行上，不进展开层', () => {
  // 用户 2026-10-07 的硬要求是「对齐过程中原本的功能不能丢失」。这条钉的是
  // 最容易被折叠掉的那一档：安全网的动作全部留在常驻行上，「查看」只开明细。
  const snapRow = templateOf(snapshotCard).slice(0, templateOf(snapshotCard).indexOf('</div>\n\n  <el-alert'));
  for (const action of ['restoreLastGood', 'loadSnapshots(true)']) {
    assert.match(snapRow, new RegExp(action.replace(/[()']/g, '\\$&')), `环境回退点行上缺 ${action}`);
  }
  const bisectRow = templateOf(bisectPanel).slice(0, templateOf(bisectPanel).indexOf('</div>\n\n  <el-alert'));
  for (const action of ['abortBisect', 'onStart']) {
    assert.match(bisectRow, new RegExp(action), `深入排查行上缺 ${action}`);
  }
});

test('行式列表的次行说明不截断成一行（有意偏离设计稿的 nowrap）', () => {
  // draft 1459-1470 的 `.page-list-meta` 是 nowrap + 省略号。这里放开换行：
  // 这一处的次行是 `headline(view)` 那类整句状态说明，截成一行就分不出
  // 「还没成功启动过」和「文档损坏」。层叠后仍然是换行的。
  assert.equal(effectiveDeclaration(['page-list-meta'], RULES, 'white-space'), null);
  assert.equal(effectiveDeclaration(['page-list-row'], RULES, 'border-bottom'), '1px solid var(--border-soft)');
  // 收尾那条的判定收在列表的直接子级上：Fragment 展开后明细也是直接子级，
  // 按行判 `:last-child` 会把展开内容当收尾行，连带抹掉上面的分隔线。
  assert.match(themeCss, /\.page-list > \*:last-child \{/);
  assert.doesNotMatch(themeCss, /\.page-list-row:last-child \{/);
});

test('内核版本页维持一张卡竖排：有意保留的偏差，不是漏改', () => {
  // draft 709-728 的 `.versions-layout` 是两列栅格，用户 2026-10-07 拍板
  // **不改**。理由记在 ui/AGENTS.md：这一屏已经调稳，三处行为依赖当前竖排
  // ——发布列表的 z-index 层叠、淡出带移除后的裁切行为、以及为 120px 限高
  // 量过的磁盘区 6+9px 间距。改两列要连这三条一起重调，而收益只是左右各短一点。
  //
  // 钉住的是「整页只有一张卡、没有两列栅格」这个**形状**，不是某个类名：
  // 设计稿那份 `.versions-layout` 从未进过 theme.css，拿它当判据等于
  // 断言一个本仓库不存在的符号（第一版就是这么写的，红的是判据不是代码）。
  assert.equal((versionsTpl.match(/<div class="card\s/g) || []).length, 1);
  assert.doesNotMatch(versionsTpl, /page-layout|grid-template-columns/);
  assert.doesNotMatch(stripComments(themeCss), /\.versions-layout/);
});

// --- 内核版本页两栏之间那道竖线（2026-10-07 用户要求「不贯通」）--------------
//
// 用户指着一张截图说「左右功能块中间增加纵向分割线，**不贯通**」。右栏是带内部
// 滚动的长列表，它自己的 `.release-list-box` 本来就有边框和底色，边界不缺；线若
// 一路通到卡片底边，这一屏会被读成两个并排窗格，而不是一张卡里的两组清单。
//
// 两条判据分别钉住「为什么不是通栏线」的两个环节，缺一环它就退回去：
// 左栏不按内容收 → 线被拉高；线改用 border 画 → 线跟着元素高度走。
function ruleText(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const m = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`));
  assert.ok(m, `必须能找到 ${selector}`);
  return m[1];
}
/** 扫出「选择器命中 `pattern` 且规则体里含 `decl`」的全部规则，返回 [选择器, 规则体]。 */
function rulesWith(css, selectorPattern, decl) {
  return [...css.matchAll(/([^{}]+)\{([^}]*)\}/g)]
    .filter(([, sel, body]) => selectorPattern.test(sel) && body.includes(decl))
    .map(([, sel, body]) => [sel.trim(), body]);
}
const versionsScoped = stripComments(scopedStyle(versionsPanel));

test('两栏之间的竖线不贯通：左栏按内容高度收，线只跟着已安装那几行', () => {
  // 主语是 `.list-group:first-child:not(...)`，全仓唯一，直接读规则文本，
  // 不走 effectiveDeclaration（见文件头那批「主语被复用就别信它」的坑）。
  // 判据不拿「我传进去的选择器」当证据——那只是调用点的字面量，改判据它照样绿；
  // 要从**文件里**把真正设了 align-self 的那条规则捞出来，再看它的选择器。
  const hits = rulesWith(versionsScoped, /\.updates-lists\s*>\s*\.list-group/, 'align-self: start');
  assert.equal(hits.length, 1, `应当恰好有一条让左栏按内容高度收的规则，实际 ${hits.length} 条`);
  const [selector, body] = hits[0];
  assert.match(body, /align-self:\s*start/);
  assert.match(selector, /:first-child/, '收高度的必须是左栏（第一栏），右栏要继续与卡片齐平');
  // 空态必须排除在外：`.installed-list` 一旦不伸展，下面那条
  // `justify-content: center` 就没有可分配空间，el-empty 会被顶回标题下方。
  assert.match(selector, /:not\([^)]*\.el-empty/, '空态那一栏要保持伸展，否则空列表的居中会失效');
});

test('两栏之间的竖线用伪元素画，不许退回 border-left', () => {
  const after = ruleText(versionsScoped, '.kernel-card .list-group .installed-list::after');
  assert.match(after, /position:\s*absolute/, '线必须绝对定位，才能落进 8px 栅格缝里而不占布局宽度');
  assert.match(after, /top:\s*0/, '线要从左栏顶端起');
  assert.match(after, /bottom:\s*0/, '线要收到左栏底端——这才是不贯通的关键');
  // border 属于元素自身，元素被拉多高线就有多高，正是要避开的那条通栏线。
  assert.doesNotMatch(
    versionsScoped,
    /installed-list[^}]*border-(left|right)/,
    'border 跟着元素自身高度走，画出来就是通栏线',
  );
  // 线挂在右缘之外（left: 100%），margin 把它推进 8px 缝里。
  assert.match(after, /left:\s*100%/);
  assert.match(after, /width:\s*1px/);
  assert.match(after, /background:\s*var\(--border-soft\)/);
});

// --- 插件页页签行距（2026-10-07 用户截图：「页签像是压在内容框上」）-----------
//
// `margin-bottom: 0` 看着是「少一段空白」，实际把页签那颗**有底色的药丸**直接压在
// 内容框边上：浏览器实测改前只剩 4px、改后 10px。清零的是行距，药丸还在，所以那
// 4px 读起来不像间距，像「被切了一截」。

test('插件页页签与内容之间有真间距，不能归零', () => {
  // 主语是 `.el-tabs__header`（Element Plus 的类）。全仓只有这一条规则，但仍然
  // 读剥注释后的规则文本而不是走 effectiveDeclaration——见文件头那批「主语被复用
  // 就别信它」的坑，这条判据不能自己踩。
  const css = stripComments(themeCss);
  const m = css.match(/\.installed-tabs \.el-tabs__header\s*\{([^}]*)\}/);
  assert.ok(m, '必须能找到 .installed-tabs .el-tabs__header 规则');
  const gap = m[1].match(/margin-bottom:\s*(\d+)px/);
  assert.ok(gap, '必须用 margin-bottom 表达这段行距');
  assert.ok(
    Number(gap[1]) >= 8,
    `页签与内容之间只有 ${gap[1]}px：药丸自带底色，8px 以下读起来像「没有间距」`,
  );
  // 取 10 而不是 Element Plus 默认的 15，是为了与 `.card-head { padding-bottom: 9px }`
  // 同一档，不在这一屏里引入第三种节奏。
  assert.equal(Number(gap[1]), 10);
});

// --- 「刷新数据」跟着它作用的东西走（2026-10-07 用户要求）--------------------
//
// 它刷的是 **dshfind.com 那份远端目录**，原先挂在整张卡（「插件管理」）的卡头靠右，
// 等于让一个作用在右栏的按钮出现在左栏。搬到右栏「插件中心」那一行之后，下面那句
// 「目录为空或加载失败，点「刷新数据」重试」才有个近处的按钮可指。
//
// 判据扫的是模板块且注释已剥（`templateOf`）——插件面板的注释里正是在解释这次搬家
// （写着「卡头」「插件中心」「刷新数据」），不剥就会被自己写的说明当成命中。

test('「刷新数据」只有一枚，且必须落在右栏「插件中心」那一行', () => {
  const tpl = templateOf(readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8'));
  // 数的是**类名**而不是整条 class 属性：按钮后来又挂上了 `btn-action`，
  // `class="plugin-center-refresh"` 这个整串会跟着不匹配——那不是「多了一枚」，
  // 是判据绑死在了属性写法上（与本文件「数整枚按钮而不是 match 一个词」同一条纪律）。
  assert.equal(
    (tpl.match(/\bplugin-center-refresh\b/g) || []).length,
    1,
    '刷新数据应当只有一枚：两枚意味着同一个动作在两处都能点，用户会不知道点哪',
  );

  // 卡头那一行里不许再出现它。
  const titleRow = tpl.match(/<div class="plugin-center-title-row">([\s\S]*?)<\/div>/);
  assert.ok(titleRow, '必须能找到 .plugin-center-title-row');
  assert.doesNotMatch(
    titleRow[1],
    /plugin-center-refresh/,
    '卡头是「插件管理」这一张卡的标题，不是目录的标题；刷目录的按钮不该出现在这里',
  );

  // 它必须在「插件中心」那个分组标题里（同一行靠右）。
  const dividers = [...tpl.matchAll(/<h3 class="section-divider">[\s\S]*?<\/h3>/g)].map((m) => m[0]);
  const catalog = dividers.find((d) => d.includes('插件中心'));
  assert.ok(catalog, '必须能找到「插件中心」那个分组标题');
  assert.match(catalog, /\bplugin-center-refresh\b/, '刷新数据必须与「插件中心」同一行');
});

test('刷新数据在分组行里靠右（跟着 .section-divider 的 flex 排）', () => {
  const css = stripComments(themeCss);
  // `.section-divider` 本身已经是 flex 行，这里只负责把它推到行尾。
  const m = css.match(/\.section-divider \.plugin-center-refresh\s*\{([^}]*)\}/);
  assert.ok(m, '必须能找到 .section-divider .plugin-center-refresh 规则');
  assert.match(m[1], /margin-left:\s*auto/);
  // 旧位置的规则必须删干净：元素不在那儿了，留着就是一条没有作用方的声明。
  assert.doesNotMatch(
    css,
    /\.plugin-center-title-row \.plugin-center-refresh/,
    '按钮已经搬走，标题行里那条 margin-left:auto 没有作用方了',
  );
});

// --- 顶部工作条整条删除（2026-10-07 用户要求）------------------------------
//
// 用户指着一张截图说「移除这个区域」——标题栏正下方那条 48px 的 `.workspace-bar`：
// 左边内核族页签，右边「当前实例 + 运行状态」。设计稿 2516 行是有这条的，所以
// 这是**覆盖设计稿**的决定。下面两条钉的是「删干净」与「别删过头」。
const appVue = readFileSync('ui/src/App.vue', 'utf8');
const instanceJs = readFileSync('ui/src/kernel/instance.js', 'utf8');

test('顶部工作条已整条删除：组件、挂载点、行高 token 都不在', () => {
  assert.ok(!existsSync(resolve('ui/src/kernel/KernelTabs.vue')), 'KernelTabs.vue 应已删除');
  assert.doesNotMatch(appVue, /KernelTabs/);
  assert.doesNotMatch(stripComments(themeCss), /--workspace-bar-height/, '只为那条 bar 定义的行高 token 应一并删掉');
  // 样式也不该有残留：页签那套是 KernelTabs 的 scoped，文件没了就该一起没。
  assert.doesNotMatch(stripComments(themeCss), /\.kernel-tab/);
});

test('删掉的是「界面上的切实例」，运行状态与实例清单一个不少', () => {
  // 那条页签**从第一天起就是空操作**：它按 kernel_family 去重，而后端只定义了
  // `dsh` 一个族（`mcode` 在 paths.rs 里写着「将来的」）。一个族只画一个页签，
  // 它必然就是当前那个，点下去在 `defaultInstanceId === id` 那一步就 return
  // false 了。所以删掉它没有拿走任何**能用的**功能。
  //
  // 但 `setDefaultInstance` **不许跟着删**。先前的版本把它当死代码删了，
  // `ui/test/kernelSwitch.test.js` 当场变红——它不是「永远走不到的代码」，而是
  // 没有调用方的 API，那条测试钉着三条语义（读去重、陈旧列表不得冲掉切换结果、
  // 失败必释放 busy）。删函数就要连语义一起删，那是拿测试换行数。这条判据就是
  // 防止下一次「顺手清死代码」再犯一遍。
  assert.match(instanceJs, /export async function setDefaultInstance/);
  assert.match(instanceJs, /selectionRevision/, '陈旧列表冲不掉切换结果这条语义要留着');
  // 但界面上确实没有入口了：没有任何模板再 import 它。
  const callers = srcScriptFiles().filter(
    (file) => file.endsWith('.vue') && readFileSync(file, 'utf8').includes('setDefaultInstance')
  );
  assert.deepEqual(callers, [], '还有模板在调 setDefaultInstance，工作条没删干净');
  // —— 运行状态与实例清单仍在：概览页状态胶囊读同一个 `store.view.kernel`，
  // 插件页照旧按实例列出接线状态（`族名 · 实例 id`）。
  assert.match(overview, /class="status-pill"/);
  assert.match(stripComments(themeCss), /^\.status-pill \{/m);
  const pluginsPanel = readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8');
  assert.match(pluginsPanel, /instanceStore\.list/);
  assert.match(pluginsPanel, /familyLabel\(inst\.record\.kernel_family\)/);
});

// --- 侧栏字号与图标放大（2026-10-07 用户要求，覆盖设计稿）--------------------
//
// 用户指着一张侧栏截图说「放大字体、icon」。设计稿 draft 509-524 行给的是
// `font-size: 13px` / `gap: 11px`，实现此前照抄；现在整体抬一档。
//
// 这几条钉的是**层叠后的生效值**，不是原始子串：`.nav-item` 是全局类，
// `.sidebar.is-collapsed .nav-item` 会在收起态再叠一条 padding，两个状态下
// 的行高必须分别核对。
test('侧栏菜单：文字 15px、图标显式 17px，分组标题 12px', () => {
  assert.equal(effectiveDeclaration(['nav-item'], RULES, 'font-size'), '15px');
  assert.equal(effectiveDeclaration(['nav-item'], RULES, 'gap'), '11px');
  assert.equal(effectiveDeclaration(['sidebar__section-label'], RULES, 'font-size'), '12px');

  // 图标这条**不能走 effectiveDeclaration**。它只认主语 token，祖先条件一概
  // 忽略，于是 `.btn-row .el-button .el-icon { font-size: 17px }`（特异度 300）
  // 会被算成「`.el-icon` 的生效值」并赢过 `.nav-item > .el-icon`（200）——
  // 判据于是**因为错误的原因而绿**：图标那条规则改成什么，它都答 17px。
  // 这条是反向验逼出来的，所以直接读规则文本。
  const css = stripComments(themeCss);
  const icon = css.match(/\.nav-item > \.el-icon \{([^}]*)\}/);
  assert.ok(icon, '找不到 `.nav-item > .el-icon` 规则：图标会退回继承字号，等于没被单独放大');
  assert.match(icon[1], /font-size: 17px/);
  assert.doesNotMatch(icon[1], /inherit/, '必须是显式像素值，写 inherit 等于没放大');
});

test('侧栏放大后，收起态与底部工具区都跟着调', () => {
  // 收起态内容盒只有 64 − 16 = 48px，行高必须另算。
  assert.equal(
    effectiveDeclaration(['sidebar', 'is-collapsed', 'nav-item'], RULES, 'padding'),
    '8px 0'
  );
  // 底部工具区跟着抬一档：15px 的菜单压着 11px 的版本号会显得头重脚轻。
  assert.equal(effectiveDeclaration(['sidebar__footer'], RULES, 'font-size'), '12px');
  // 侧栏 224px 仍放得下最长的一项（「数据迁移」四字）。
  assert.equal(effectiveDeclaration(['sidebar'], RULES, 'width'), 'var(--sidebar-width)');
});

test('角标圆点从 18px 变 20px，跟上放大的菜单字', () => {
  // **不能按主语查角标**：`.sidebar.is-collapsed .nav-item__badge` 的主语也是
  // `.nav-item__badge`，`effectiveDeclaration(['nav-item__badge'], …)` 返回的
  // 是**收起态**那个 8px / font-size:0 的小红点（祖先条件不参与主语判定，
  // 这是 `effectiveDeclaration` 的已知边界，见本文件顶部注释）。所以这条读
  // 剥掉注释后的规则文本，并把两条规则分开认——它们靠祖先条件区分，混在一起
  // 比就会让收起态的红点盖掉展开态的尺寸。
  const css = stripComments(themeCss);
  const expanded = css.match(/\.nav-item__badge \{([^}]*)\}/);
  assert.ok(expanded, '找不到 .nav-item__badge 规则');
  assert.match(expanded[1], /min-width: 20px/);
  assert.match(expanded[1], /height: 20px/);
  assert.match(expanded[1], /font-size: 12px/);
  const collapsed = css.match(/\.sidebar\.is-collapsed \.nav-item__badge \{([^}]*)\}/);
  assert.ok(collapsed, '找不到收起态的角标规则');
  assert.match(collapsed[1], /min-width: 8px/);
});

// --- 功能块标题整体放大（2026-10-07 用户要求，覆盖设计稿）--------------------
//
// 用户说「所有功能块的 title 字体大小都需要放大」。设计稿 draft 1975 行给的
// `.card-title` 是 13px，实现此前是 14px——在 1040 宽的窗口里，一个块的边界
// 正是靠标题建立的，字太小等于没有块。
//
// 这几条钉的是**每一层标题各自的生效值**：页面标题、块标题（h2）、块内小标题
// （h3）、行式条目标题、诊断层的两种标题，以及标题旁的 ⓘ。ⓘ 必须一起长——
// 留在 14px 会读成「一个更小的另一个元素」，而不是「这个标题的补充说明」。
test('功能块标题整体放大：块标题 17px / 块内小标题 15px / 页标题 21px', () => {
  // `.card h2` / `.card h3` **不能走 effectiveDeclaration**：它们的主语是**裸
  // 标签** `h2` / `h3`，全仓有十来条同主语的规则（`.migration header h2` 特异度
  // 102 直接盖过 `.card h2` 的 101，`.callout-body h3` / `.step-body h3` 等同
  // 特异度但源码在后，靠 `>=` 决胜又轮番抢走）。问出来的会是别的规则的值——
  // 与 `el-icon` 那次同一个坑的另一个变体。这两条读规则文本。
  const css = stripComments(themeCss);
  // 2026-10-08 起这两条读的是档位 token 而不是字面量：六个面板共用一份，
  // 「块标题该多大」这件事只在一个地方说了算。
  assert.match(css.match(/\.card h2 \{([^}]*)\}/)[1], /font-size: var\(--fs-block-title\)/);
  assert.match(css.match(/\.card h3 \{([^}]*)\}/)[1], /font-size: var\(--fs-subtitle\)/);
  assert.equal(tokenValue('--fs-block-title'), '17px');
  assert.equal(tokenValue('--fs-subtitle'), '15px');
  assert.equal(effectiveDeclaration(['page-title'], RULES, 'font-size'), '21px');
  // 「环境回退与诊断」那张卡的四个行式条目走 `.page-list-title`，也得跟上。
  assert.equal(
    effectiveDeclaration(['page-list-title'], RULES, 'font-size'),
    'var(--fs-subtitle)',
  );
  // 插件页右栏那份远端目录本来就写着 17px，与块标题齐平——两条一起认，
  // 免得只放大其中一条把它们拉成两档。
  assert.equal(effectiveDeclaration(['plugin-center-title'], RULES, 'font-size'), 'var(--fs-block-title)');
  assert.equal(effectiveDeclaration(['card-info-icon'], RULES, 'font-size'), 'var(--fs-tip-icon)');
});

test('诊断层的两种块标题也跟着放大', () => {
  // 系统健康 / 最近操作 / 事故 / 预检这几张卡住在 `diagnostics.css` 的
  // `.diag-card` 家族里，不在 `.card h2 / h3` 的管辖范围内——只改 theme.css
  // 会让概览页放大了、诊断层没放大，同屏两档。
  assert.equal(effectiveDeclaration(['diag-card__title'], RULES, 'font-size'), '15px');
  assert.equal(effectiveDeclaration(['diagnosis__title'], RULES, 'font-size'), '17px');
});

test('全仓没有低于 15px 的块标题（h2 / h3）', () => {
  // 单点断言只能守住改过的那几条。这条扫全表：`.card h2` 这种「一个裸标签被
  // 全仓复用」的形状，逐个断言必然漏——2026-10-07 那批 `.callout-body h3` /
  // `.step-body h3` / `.disk-usage h2` 全是这么漏掉的，它们的字号直接决定
  // 那一屏的层级，所以用「全表没有更小的」这条一网打尽。
  const tooSmall = [];
  for (const rule of RULES) {
    const subject = rule.selector;
    const tags = subjectTokens(subject);
    if (!tags.has('h2') && !tags.has('h3')) continue;
    const value = (rule.body.match(/font-size:\s*([^;}]+)/) || [])[1];
    if (value === undefined) continue;
    const px = parseFloat(value);
    if (Number.isFinite(px) && px < 15) tooSmall.push(`${subject} → ${value.trim()}`);
  }
  assert.deepEqual(tooSmall, [], `还有块标题小于 15px：\n${tooSmall.join('\n')}`);
});

// --- 弹窗与浮层的主题适配（2026-10-08）---------------------------------------
//
// 用户拿一张浅色主题下的进度浮层截图说「这类弹窗还是没有适配主题，注意统一
// 调整」。根因不是样式没写，是**四处浮层底色写死了深色主题的值**：
// `.progress-body` / `.el-dialog` 都是 `#0d1428`（暗色主题的 `--surface`），
// 于是浅色用户看到一块黑板子配深灰字，而同一屏上 `.el-overlay` 的遮罩又
// 是 Element Plus 浅色默认的 `rgba(255,255,255,.9)`——一层九成白。
// 同一个遮罩在两套主题里明暗相反，这正是「没适配」最显眼的一半。
//
// 下面这组判据钉的是**不变量**而不是当前那几行：只要再出现一处写死的浮层底色
// 或新造一个遮罩值，它就转红。

/** 四块浮层：壳自己画的三块 + Element Plus 的 dialog。`background` 这一列是它们
 *  唯一的底色来源——有一处改回字面量（`#0d1428` / `#fff` /
 *  `var(--el-bg-color, …)`）就等于退回 2026-10-08 之前那个状态。
 *  `.el-message` / `.el-message-box` 的底色由库的 `--el-bg-color-overlay` 提供，
 *  那个变量两套主题都已经覆写过，不需要在这里再钉一遍。 */
const OVERLAY_SURFACES = [
  ['.progress-body', 'shell/ProgressOverlay.vue'],
  ['.debug-panel', 'shell/DebugPanel.vue'],
  ['.render-error-fallback', 'App.vue'],
  ['.el-overlay-dialog .el-dialog', null],
];

test('四块浮层的底色都走 --surface-raised，没有一处写死深色', () => {
  for (const [selector, where] of OVERLAY_SURFACES) {
    // 传类集合而不是选择器串：`have.has('progress-body')` 与 `have.has('.progress-body')`
    // 不是一回事，后者会让工具一条规则都匹配不上、安静地答 null。
    const value = effectiveDeclaration(subjectTokens(selector), RULES, 'background');
    // `.el-dialog` 的底色走的是自定义属性而不是 `background`，工具取不到，
    // 单独放行（下面一条判据专门盯它）。
    if (selector.startsWith('.el-')) {
      assert.ok(value === null || value === 'var(--surface-raised)', `${selector} 的底色是 ${value}`);
      continue;
    }
    assert.equal(value, 'var(--surface-raised)', `${selector}（${where}）的底色不是 --surface-raised`);
  }
  // `.el-dialog` 的底色是 `--el-dialog-bg-color`，**必须声明在元素自身**：
  // 库在 `.el-dialog` 规则里就写了同名自定义属性，写到 `:root` 上会被它赢掉，
  // 症状是「规则看着在，弹窗底色不变」。这条钉住选择器本身。
  const css = stripComments(themeCss);
  assert.match(css, /\.el-overlay-dialog \.el-dialog\s*\{[^}]*--el-dialog-bg-color: var\(--surface-raised\)/);
  // 写死的那一档也要一并钉住，免得换个色值再回来。
  assert.doesNotMatch(css, /#0d1428/, '生产样式里不该再出现写死的深色浮层底色');
});

test('遮罩只有一个来源，且浅色主题下不是 Element Plus 那层九成白', () => {
  // `.el-overlay`（`el-dialog` / `el-message-box` 那一层）与壳自己的
  // `.progress-overlay` 必须读同一个变量，否则两套主题下两个遮罩各调各的。
  assert.equal(
    effectiveDeclaration(subjectTokens('.progress-overlay'), RULES, 'background'),
    'var(--el-mask-color)'
  );

  // Element Plus 在**浅色**下的默认是 `rgba(255,255,255,.9)`，暗色下是 `#000c`：
  // 同一个遮罩在两套主题里明暗相反。`:root` 必须把它拉成深色半透明。
  const darkStart = themeCss.indexOf('\nhtml.dark {');
  const light = themeCss.slice(0, darkStart);
  const dark = themeCss.slice(darkStart);
  const mask = /--el-mask-color:\s*([^;]+);/.exec(light);
  assert.ok(mask, ':root 缺 --el-mask-color（浅色下会落回库的 rgba(255,255,255,.9)）');
  assert.match(mask[1], /^rgba\(\s*\d+\s*,\s*\d+\s*,\s*\d+\s*,/, `浅色遮罩要是不透明就没有层次：${mask[1]}`);
  assert.match(dark, /--el-mask-color:\s*[^;]+;/, 'html.dark 缺 --el-mask-color');
});

test('生产 CSS / Vue 里不再引用已改名的旧 token', () => {
  // `var(--muted, 兜底值)` 是这一类里最阴的一种：token 已改名，兜底值是近白，
  // 浅色主题下**整条声明看着还在**（CSS 不会报错），元素只是退回浏览器默认色
  // 或读那个近白——`--text-muted` 那种「次要文字」在浅底上直接消失。
  // 2026-10-08 修掉的 9 处：theme.css 三处、MigrationPrompt 五处、
  // PrecheckDialog 与 SnapshotRestoreDialog 各一处。
  const renamed = /var\(\s*--(?:muted|text-dim|text-dim2|warn|good|bad|surface-soft|card|bg)\b/;
  const files = [themeCss, ...srcScriptFiles().map((p) => readFileSync(p, 'utf8'))];
  const hits = [];
  for (const text of files) {
    const hit = renamed.exec(stripComments(text));
    if (hit) hits.push(`${hit[0]}`);
  }
  assert.deepEqual(hits, [], `还有已改名的 token：${[...new Set(hits)].join('、')}`);
});

// --- 彩色按钮的文字必须两套主题都读得清（2026-10-08）------------------------
//
// 用户拿浅色主题下的概览页截图说「亮色模式下，这个『官网网页版』，文字颜色不清晰」。
// 根因与上一组同类但低一层：**文字色**写死了「为深色底调的淡彩」——薄荷绿
// `#6ee7b7` 在白卡上只有 1.52:1、hover 的 `#a7f3d0` 只有 1.28:1（正文门槛 4.5:1），
// 关闭态的 `#f87171` 也只有 2.77:1。Tailwind 的 emerald-300 / red-400 那一档
// 在深色底上舒服，在浅色底上等于「没有颜色」。
//
// 这类值的特征是**只在一套主题下成立**：它们在暗色下看着是对的，所以任何只测
// 暗色的检查都抓不到。判据因此钉的是「读 token」而不是「等于某个色值」。

/** 会以语义色显示文字、且浮在卡片上的按钮 / 徽章。取值是「静止态的那条规则」，
 *  hover / 焦点档由下面一条单独钉——只钉静止态的话，把 hover 换回淡粉照样绿。 */
const COLORED_CONTROLS = [
  ['btn-chat', 'el-button', '--success'],
  ['btn-danger', 'el-button', '--danger'],
  ['entity-mode', 'el-button', null], // is-link 修饰，见下一条
];

test('彩色按钮的文字走语义 token，不再是只在一套主题下成立的淡彩', () => {
  const css = stripComments(themeCss);
  for (const [cls, tag, token] of COLORED_CONTROLS) {
    if (!token) continue;
    const value = effectiveDeclaration(subjectTokens(`.${cls}.${tag}`), RULES, 'color');
    assert.equal(value, `var(${token})`, `.${cls} 的文字色是 ${value}，应当是 var(${token})`);
  }
  // 插件页「链接」模式徽章是同一处缺陷的另一个落点（emerald-400，白底 1.7:1）。
  const link = effectiveDeclaration(
    ['el-button', 'entity-mode', 'is-link'],
    RULES,
    'color',
    { pseudo: null }
  );
  assert.equal(link, 'var(--success)', `「链接」徽章的文字色是 ${link}`);

  // 这一族色值（emerald / red 的 300-400 档）在浅色主题下全部不达标，
  // 钉住它们不再出现——注释里仍允许写，那是当年为什么选它们的解释。
  for (const pastel of ['#6ee7b7', '#a7f3d0', '#f87171', '#fca5a5', '#34d399']) {
    assert.doesNotMatch(css, new RegExp(pastel, 'i'), `${pastel} 是深色底专用色，浅色主题下不达标`);
  }
});

test('语义色的「更强调一档」两套主题都定义了', () => {
  // `--success-strong` / `--danger-strong` 是按钮 hover 与焦点态的落点：浅色下
  // 更深、暗色下更亮。**只在一套里定义的话**，另一套下那条声明整条失效，hover
  // 静默退回静止色——不报错、也看不出来，直到用户把鼠标放上去才发现「hover 没反应」。
  const darkStart = themeCss.indexOf('\nhtml.dark {');
  const light = themeCss.slice(0, darkStart);
  const dark = themeCss.slice(darkStart);
  for (const token of ['--success-strong', '--danger-strong', '--success-fill']) {
    assert.match(light, new RegExp(`${token}:\\s*[^;]+;`), `${token} 缺浅色定义`);
    assert.match(dark, new RegExp(`${token}:\\s*[^;]+;`), `${token} 缺暗色定义`);
  }
  // hover 档确实读的是 strong 档而不是又一遍静止色。
  const css = stripComments(themeCss);
  assert.match(
    css,
    /\.btn-chat\.el-button:not\(:active\):hover,\s*\.btn-chat\.el-button:not\(:active\):focus-visible\s*\{\s*color: var\(--success-strong\)/,
    '.btn-chat 的 hover / 焦点态必须落到 --success-strong，且按下时不生效'
  );
  assert.match(
    css,
    /\.btn-danger\.el-button:not\(:active\):hover,\s*\.btn-danger\.el-button:not\(:active\):focus-visible\s*\{\s*color: var\(--danger-strong\)/,
    '.btn-danger 的 hover / 焦点态必须落到 --danger-strong，且按下时不生效'
  );
  // 条件写进 hover 选择器（`:not(:active)`），而不是另起一条 `:active` 把颜色
  // 打回去——后者每多一个状态就要再抄一条，且全靠源码顺序决胜。
  assert.doesNotMatch(css, /\.btn-(?:chat|danger)\.el-button:active/);
});

test('彩色按钮不再单列 .el-icon 选择器（Element Plus 的图标本来就继承）', () => {
  // `.el-icon` 是 `--color: inherit` + `color: var(--color)`，图标跟着按钮走。
  // 历史上每条按钮规则都把 `.el-icon` 重抄一遍（`.btn-chat` / `.btn-danger` 各四处），
  // 纯冗余，而且**抄漏一处就只染到一半**：图标与文字不同色时看起来像渲染错位。
  const css = stripComments(themeCss);
  assert.doesNotMatch(css, /\.btn-(?:chat|danger)\.el-button[^\n]*\.el-icon/);
});

// --- 药丸页签的内边距两侧都要在（2026-10-08）---------------------------------
//
// 用户报「『已安装』按钮文字未居中」。按截图量：药丸 54px = 文字 42px + **单侧**
// 12px，另一侧 0。根因是 Element Plus 有两条 (0,4,0) 的规则把页签内边距单侧清零
// （`:nth-child(2) { padding-left: 0 }` / `:last-child { padding-right: 0 }`）——
// 它的前提是页签没有底色，而本页的页签是药丸。单侧清零在无底色时看不出来（缝由
// 前一个的 padding-right 承担），在药丸上就是「字整块贴着一边」。
//
// 判据钉的是**特异度**，不是当前那几行值：把选择器写回 `.installed-tabs
// .el-tabs__item`（0,2,0）会静默地输给库，单测照样全绿。

test('药丸页签的规则特异度不低于 Element Plus 那两条单侧清零', () => {
  const rule = /\.installed-tabs [^{]*\.el-tabs__item[^{]*\{([^}]*)\}/.exec(stripComments(themeCss));
  assert.ok(rule, '应能找到药丸页签的尺寸规则');
  const selector = rule[0].slice(0, rule[0].indexOf('{')).trim();
  // (0,4,0)：三个类 + 一个伪类。少任何一个就退回 (0,3,0) / (0,2,0)，输给库。
  assert.match(
    selector,
    /\.installed-tabs \.el-tabs__nav \.el-tabs__item:nth-child\(n\)/,
    `药丸页签的选择器必须带 .el-tabs__nav + :nth-child(n)（当前：${selector}）`
  );
  assert.match(rule[1], /padding:\s*0 12px/, '药丸页签必须两侧等宽内边距');

  // 库里那两条确实存在（钉住前提，防止升级 EP 后判据变成一句空话）。
  const ep = readFileSync(
    'node_modules/element-plus/theme-chalk/el-tabs.css',
    'utf8',
  );
  assert.match(ep, /\.el-tabs--top>\.el-tabs__header \.el-tabs__item:nth-child\(2\)[^{]*\{padding-left:0\}/);
  assert.match(ep, /\.el-tabs--top>\.el-tabs__header \.el-tabs__item:last-child[^{]*\{padding-right:0\}/);
});

test('「插件中心」右侧的辅助信息比标题小一档', () => {
  // 标题是 h3（15px），辅助信息此前只挂 `.muted`（只管颜色），字号直接继承 15px，
  // 于是补充与标题一样大、标题不再是这一行的主体。
  assert.equal(
    effectiveDeclaration(['section-divider__note'], RULES, 'font-size'),
    '12px',
    '「来自 dshfind.com」应当是 12px（标题 15px）'
  );
  // 字号不许加进 `.muted`：那个类全仓 23 处在用，一刀切会顺带改掉六个面板。
  const muted = stripComments(themeCss).match(/\.muted \{([^}]*)\}/);
  assert.ok(muted, '`.muted` 规则应在');
  assert.doesNotMatch(muted[1], /font-size/, '`.muted` 只管颜色，不该带字号');
  // 模板上这枚辅助信息确实挂了这个类。
  assert.match(
    stripComments(templateOf(pluginsPanel)),
    /class="muted section-divider__note"/,
  );
});
