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

test('版本号与状态行同属一个竖向块（kernel-summary-main），不是两个平级 flex 子项', () => {
  assert.match(overview, /<div class="kernel-summary">\s*<!--[\s\S]*?-->\s*<div class="kernel-summary-main">/);
  assert.match(overview, /class="kernel-summary-main">\s*<VersionBadge[\s\S]*?<\/VersionBadge>\s*<div class="kernel-status-row">/);
});

test('卡头 caption 的族名与实例 id 取自同一条实例记录，不读 KernelStatus 的不存在字段', () => {
  // `KernelStatus` 没有族名字段，也没有实例 id 字段。从 kernel 上读它们取到的
  // 是 undefined，而 `undefined || …` 会静默落到兜底分支——门禁 [ipc-fields]
  // 当场抓的就是这条（2026-10-07）。判据匹配的是「从 kernel 上取」这个形状，
  // 不是某几个具体字段名：后者连注释里提一嘴都会误判成真实读取。
  assert.doesNotMatch(overview, /kernel(\.value)?\.[a-z_]*family/i);
  // 族与 id 必须来自同一次查找：分两次查可能落在不同实例上。
  assert.match(
    overview,
    /const current = instanceStore\.list\.find\(\(it\) => it\.record\.id === instanceStore\.defaultInstanceId\);/
  );
  assert.match(
    overview,
    /return `\$\{familyLabel\(current\.record\.kernel_family\)\} \/ \$\{current\.record\.id\}`;/
  );
});

test('版本号取设计稿的 18px；摘要行 align-items: start 且内边距 13px 0 12px', () => {
  const style = scopedStyle(overview);
  assert.equal(effectiveDeclaration(['kernel-version'], RULES, 'font-size'), '18px');
  assert.equal(effectiveDeclaration(['kernel-summary'], RULES, 'align-items'), 'start');
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
  // 独立窗口入口
  assert.match(modal, /@click="openLogWindow"/, '弹层要有独立日志窗口入口');
  assert.match(modal, /invoke\('open_log_window'/, '独立窗口应走后端命令');
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

test('控制塔三张塔卡用 order 落位；健康卡在右半栏，关注与最近操作末行并排', () => {
  for (const [cls, order, col] of [
    ['diag-card--health', '2', '2'],
    ['diag-card--attention', '4', '1'],
    ['diag-card--activity', '5', '2'],
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
  assert.match(skillsTpl, /<span class="card-title">社区资源<\/span>/);
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

// --- 设置页：右栏收成两张卡 -------------------------------------------------
//
// 用户 2026-10-07 指出「左栏两张、右栏四张，左栏空一大截」，拍板按设计稿
// draft 2844 合并成「环境回退与诊断」一张。原右栏四张（任务通知 / 数据迁移 /
// 环境回退点 / 深入排查）同属「环境出问题时才动」，拆成四张只是把一个上下文
// 摊成四段。下面几条钉的就是「两栏各两张 + 那一张里四行」这个形状。
const settingsPanel = readFileSync('ui/src/shell/SettingsPanel.vue', 'utf8');
const settingsTpl = templateOf(settingsPanel);
const snapshotCard = readFileSync('ui/src/diagnostics/SnapshotCard.vue', 'utf8');
const bisectPanel = readFileSync('ui/src/diagnostics/BisectPanel.vue', 'utf8');

test('设置页两栏各两张卡，右栏第二张是「环境回退与诊断」', () => {
  // 只数模板块里 `.card` 的直接出现次数：卡片真身是 `<div class="card">`，
  // 组件自带的外框也已在本轮删掉（下面那条判据钉着）。
  assert.equal((settingsTpl.match(/<div class="card">/g) || []).length, 4);
  assert.match(settingsTpl, /<h2>环境回退与诊断<\/h2>/);
  // caption 走既有的 head-meta + muted，不为这一处新增 .card-caption。
  assert.match(settingsTpl, /出问题时使用/);
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

test('删掉的是「界面上的切实例」，实例上下文与运行状态一个不少', () => {
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
  // —— 「用户在服务的是哪个实例」这件事一个都不能少：
  // 概览页「当前内核」卡头照旧渲染 `族名 / 实例 id`，插件页照旧列出所有实例。
  assert.match(overview, /instanceStore\.defaultInstanceId/);
  assert.match(overview, /familyLabel\(current\.record\.kernel_family\)/);
  assert.match(readFileSync('ui/src/plugins/PluginsPanel.vue', 'utf8'), /instanceStore\.list/);
  // 运行状态胶囊（.status-pill 是 theme.css 的共用词汇）仍在概览页读同一个字段。
  assert.match(overview, /class="status-pill"/);
  assert.match(stripComments(themeCss), /^\.status-pill \{/m);
});
