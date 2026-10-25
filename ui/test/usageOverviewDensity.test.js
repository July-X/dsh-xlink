// 「模型用量」副窗里**用户点名的那块区域**（顶部「外部工具」勾选行 + 概览瓦片
// + 活跃热力图）的高度预算。
//
// 2026-10-25 用户反馈「UI 太松散了，纵向空间占用有点多」。这一轮把这一块从
// **191.5px** 压到 **133.0px**（省 58.5px，-30.5%），本文件把压缩它的每一条杠杆
// 钉成判据：下一次有人「顺手把 padding 加回去」时，这里立刻红。
//
// 口径（三条，别改）：
//   1. 每条杠杆读**层叠后的生效值**（effectiveDeclaration），不读源码子串——
//      别的规则再写一条就能把它盖回去，而那正是当初出事的地方。
//   2. **`:deep()` 写法的规则层叠工具读不到**（主语以 `:` 开头会被直接跳过，
//      `effectiveDeclaration([...], 'font-size')` 返回 null）。这类改读规则原文，
//      与 designAlignment.test.js 同一套做法。
//   3. 几何读数来自**真实渲染**（真实组件 + 真实 CSS + 桩数据，容器复刻真机
//      760×800 = `USAGE_VIEWER_SIZE`），不是肉眼估的。桩数据只用于几何，不参与
//      任何结论。
//
// 各条杠杆值多少（2026-10-25 真机几何实测，改前 → 改后）：
//   外部工具行      47.0px → 23.4px   （EP 的 32px 勾选框高度 + 7px 内边距）
//   main 上边距      12px  → 8px
//   概览区          132.5px → 102.0px
//     标题行        22.5px → 18.0px
//     热力图格子块  100.0px → 79.0px （格子 12px → 9px，7 行每行省 3px）
//     单个瓦片      59.0px → 48.3px
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { allRulesIncludingVue, effectiveDeclaration, specificity } from './lib/css-cascade.mjs';

const RULES = allRulesIncludingVue();

/** 层叠后的生效值（读不到时是 null——判据里要显式区分 null 与「值不对」）。 */
const eff = (tokens, prop) => effectiveDeclaration(tokens, RULES, prop);

/** 规则原文：给层叠工具读不准的那类（`:deep()`、祖先条件）用。 */
const ruleBySelector = (selector) => RULES.find((r) => r.selector === selector);

test('外部工具那一行：EP 的 32px 勾选框高度必须被压掉，内边距也收紧', () => {
  // EP 的 `.el-checkbox` 自带 `height: var(--el-checkbox-height,32px)`。那一行只有
  // 两个勾选框，却被这个 32px 撑到 47px（实测）。
  assert.equal(
    eff(['usage-sources', 'el-checkbox'], 'height'),
    'auto',
    'EP 的 32px 勾选框高度没被压掉——外部工具那一行会被撑回 47px'
  );
  assert.equal(
    eff(['usage-sources', 'el-checkbox'], 'min-height'),
    '0',
    'flex 子项的 min-height 不给 0，勾选框的最小高度又会把那一行撑回去'
  );
  assert.equal(
    eff(['usage-sources'], 'padding'),
    '4px 14px',
    '外部工具行的上下内边距回到 7px，这行就又高 6px'
  );
  // 行高由这一行**文字的 strut** 撑着（容器字号 13px 乘全局行高 ≈ 18px，比 14px
  // 的勾选框还高）——不显式压它，这一行矮不下去。
  assert.equal(
    eff(['usage-sources'], 'line-height'),
    '1.2',
    '外部工具行的行高没压：那一行由文字 strut 撑着，会比 14px 的勾选框还高'
  );
});

test('EP 那条 32px 确实还在，而且我们盖得过它', () => {
  // 覆盖的前提是对手还在。EP 升级后若不再给勾选框固定高度，这条判据会提示
  // 「可以去掉了」，而不是让一行死代码永远留着。
  const ep = readFileSync('node_modules/element-plus/theme-chalk/el-checkbox.css', 'utf8');
  assert.match(
    ep,
    /\.el-checkbox\{[^}]*height:\s*var\(--el-checkbox-height,\s*32px\)/,
    'EP 的 .el-checkbox 不再自带 32px 高度——那条覆盖可以撤了（别把死覆盖留着）'
  );
  // 覆盖要赢：我们的 `.usage-sources .el-checkbox`（两个类）必须比 EP 的
  // `.el-checkbox`（一个类）更具体，否则是同特异度按源码顺序决胜——谁在后面
  // 全看打包顺序，不看语义。
  const ours = specificity('.usage-sources .el-checkbox');
  const theirs = specificity('.el-checkbox');
  assert.ok(
    ours > theirs,
    `覆盖必须比 EP 的规则更具体：我们的 ${ours} vs EP 的 ${theirs}`
  );
});

test('勾选框标签那条必须带 :deep()，否则它永远匹配不上', () => {
  // scoped 的 scope id 只落在子组件的**根元素**上；`.el-checkbox__label` 是
  // el-checkbox 内部的 span，拿不到父组件的 scope id——写成
  // `.usage-sources .el-checkbox__label` 时编译结果是
  // `… .el-checkbox__label[data-v-xxx]`，那条规则永远匹配不上。
  // 2026-10-25 在编译产物里核实过：label 上没有 data-v 属性，计算字号是 EP 的
  // 14px，不是这里写的 13px。
  const labelRules = RULES.filter((r) => r.selector.includes('el-checkbox__label'));
  assert.equal(labelRules.length, 1, `应当只有一条 label 规则，实际 ${labelRules.length} 条`);
  assert.equal(
    labelRules[0].selector,
    '.usage-sources :deep(.el-checkbox__label)',
    'label 规则必须写 :deep()——少了它，scoped 的 scope id 匹配不上，这条规则是死的'
  );
  assert.match(labelRules[0].body, /font-size:\s*13px;/, '勾选框标签是 13px');
  // 反向也钉一句：别再写一条没带 :deep() 的同名规则，它匹配不上，只会把人骗了。
  const dead = RULES.filter(
    (r) => r.selector.includes('el-checkbox__label') && !r.selector.includes(':deep(')
  );
  assert.equal(dead.length, 0, `这些 label 规则匹配不上（scoped 的 scope id 落不到那里）：${dead.map((r) => r.selector).join('、')}`);
});

test('概览两栏与瓦片的间距收紧，但列数与跨列不变', () => {
  assert.equal(eff(['usagewin-main'], 'padding'), '8px 14px 8px', 'main 的上内边距回到 12px，标注区就高 4px');
  assert.equal(eff(['usage-overview'], 'gap'), '12px', '概览两栏的间距回到 16px');
  assert.equal(eff(['usage-overview-stats'], 'gap'), '5px', '瓦片之间的间距回到 6px');
  assert.equal(eff(['usage-overview-heat'], 'padding-left'), '10px', '热力图左栏的留白回到 14px');
  // 列数与跨列**不许动**：1 行放不下「最常用模型」的名字（会被截断），而且概览的
  // 高度由热力图那一栏支配，改成 1 行并不省高度。
  assert.equal(
    eff(['usage-overview-stats'], 'grid-template-columns'),
    'repeat(3, minmax(0, 1fr))',
    '瓦片必须仍是 3 列'
  );
  assert.equal(
    eff(['usage-summary-card', 'wide'], 'grid-column'),
    'span 2',
    '「最常用模型」那张必须仍跨 2 列'
  );
});

test('瓦片本身：内边距、圆角、标签行高、数值字号', () => {
  assert.equal(eff(['usage-summary-card'], 'padding'), '4px 9px', '瓦片内边距回到 6/10');
  assert.equal(eff(['usage-summary-card'], 'border-radius'), '8px', '瓦片圆角回到 10px');
  // 全局行高 1.5 让标签这行占 18px（实测），一行标签白占 3px。
  assert.equal(eff(['usage-summary-label'], 'line-height'), '1.35', '标签行高回到 1.5，白占 3px');
  assert.equal(eff(['usage-summary-value'], 'font-size'), '17px', '数值字号回到 18px');
  assert.equal(eff(['usage-summary-value'], 'line-height'), '1.3', '数值的行高没压');
});

test('活跃热力图：格子缩到 9px，标题行只压这一栏', () => {
  // 热力图是 7 行 × 13 列，格子每边少 3px 就省下 21px 的整块高度（实测格子块
  // 100px → 79px）。图例里那几枚也用同一条规则，跟着一起变小。
  assert.equal(eff(['usage-heat-cell'], 'width'), '9px', '热力图格子回到 12px，整块就高 21px');
  assert.equal(eff(['usage-heat-cell'], 'height'), '9px', '热力图格子回到 12px');
  assert.equal(eff(['usage-heat-cell'], 'border-radius'), '2px', '格子的圆角回到 3px');
  // 标题行：这一栏的标题行从 22.5px 压到 18.0px。
  assert.equal(
    eff(['usage-section-head'], 'margin-bottom'),
    '5px',
    '热力图标题行的下外边距回到 10px'
  );
  // **字号不许压**：全仓块标题（h2/h3）的下限是 15px（designAlignment.test.js
  // 「全仓没有低于 15px 的块标题」）。那一条守的是屏幕层级，不是这一行的高度——
  // 压缩只能从行高和下外边距上来。
  assert.equal(
    eff(['usage-section-head', 'h3'], 'font-size'),
    'var(--fs-subtitle)',
    '热力图标题的字号不许压到 15px 以下——全仓块标题的下限是 15px'
  );
  assert.equal(eff(['usage-section-head', 'h3'], 'line-height'), '1.2', '热力图标题的行高没压');
  assert.equal(eff(['usage-source-chip'], 'font-size'), '10px', '热力图那枚来源牌字号回到 11px');
  assert.equal(eff(['usage-source-chip'], 'line-height'), '14px', '热力图那枚来源牌行高回到 16px');
  assert.equal(eff(['usage-source-chip'], 'padding'), '0 6px', '热力图那枚来源牌内边距回到 0 7px');
  assert.equal(eff(['usage-legend'], 'font-size'), '11px', '图例字号回到 12px');
});

test('压热力图不许压到另外两段的标题（读规则原文）', () => {
  // `.usage-section-head` 是「按天趋势」「模型用量」两段共用的：那两条的标题行
  // 尺寸不许跟着变小，所以**不能**改这条基线，只能在热力图那一栏上另开一条。
  // 层叠工具是祖先盲的（`.usage-overview-heat .usage-section-head` 的主语同样是
  // `.usage-section-head` 且特异度更高），所以这两条只能读规则原文。
  const base = ruleBySelector('.usage-section-head');
  assert.ok(base, '找不到 .usage-section-head 的基线规则');
  assert.match(base.body, /margin-bottom:\s*10px;/, '基线标题行的下外边距必须是 10px（另两段靠它）');
  assert.match(base.body, /gap:\s*10px;/, '基线标题行的间距必须是 10px');

  const baseH3 = ruleBySelector('.usage-section-head h3');
  assert.ok(baseH3, '找不到 .usage-section-head h3 的基线规则');
  assert.match(
    baseH3.body,
    /font-size:\s*var\(--fs-subtitle\)/,
    '基线标题字号必须仍是 var(--fs-subtitle)——压它会把「按天趋势」「模型用量」两段的标题一起变小'
  );
  assert.doesNotMatch(
    baseH3.body,
    /font-size:\s*13px/,
    '基线标题字号不许直接写成 13px：那样两段标题跟着变小，压缩就溢出了热力图那一栏'
  );
});
