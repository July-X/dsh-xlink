// 「任务通知」是 DSH 与 MiniMax 两个工作台共用的：两边的任务完成都要通知，列表
// 按 item.kernel 标出来源。判据读模板里真实的 v-if 表达式并求值，不是只匹配文案——
// 标题改回按内核分叉、chip 分支写死 DSH、提示少一个前提，都要当场红。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const panel = readFileSync(new URL('../src/shell/SettingsPanel.vue', import.meta.url), 'utf8');

/** 模板块，HTML 注释已剥；收尾边界取 style 标签（与 designAlignment 同一做法）。 */
function templateOf(src) {
  const start = src.indexOf('<template>');
  assert.ok(start >= 0, '找不到 <template>');
  const styleAt = src.indexOf('<style', start);
  return src.slice(start, styleAt > start ? styleAt : src.length).replace(/<!--[\s\S]*?-->/g, '');
}

/** 任务通知卡：从卡的开标签到下一张卡（右栏它是最后一张，切到尾即可）。
 *  锚点用完成记录列表而不是表单一类名：设置页「后台常驻」卡也复用 .notify-form。 */
function notificationCard(src) {
  const tpl = templateOf(src);
  const listAt = tpl.indexOf('class="notify-items"');
  assert.ok(listAt > 0, '找不到任务通知卡的完成记录列表');
  const start = tpl.lastIndexOf('<div class="card">', listAt);
  assert.ok(start >= 0, '任务通知必须还在自己的卡片里');
  const next = tpl.indexOf('<div class="card">', listAt);
  return tpl.slice(start, next > start ? next : tpl.length);
}

/** 对模板里的条件表达式求值：判据要的是「什么状态下显示」，不是文案命中。 */
function evalWith(expr, scope) {
  return new Function(...Object.keys(scope), `return (${expr});`)(...Object.values(scope));
}

function assertFixedTitle(src) {
  const card = notificationCard(src);
  const at = card.indexOf('任务通知');
  assert.ok(at > 0, '卡片标题必须还是「任务通知」');
  const h2 = card.slice(card.lastIndexOf('<h2', at), card.indexOf('</h2>', at) + '</h2>'.length);
  assert.match(h2, /class="card-title-with-tip"/);
  // 两个内核共用一份卡片：不存在「DSH 任务通知」这种按内核换的标题。
  assert.doesNotMatch(h2, /\{\{/, '标题不许再按内核改文案');
  assert.doesNotMatch(h2, /DSH 任务通知/);
}

function assertKernelChips(src) {
  const card = notificationCard(src);
  const at = card.indexOf('class="notify-item-head"');
  assert.ok(at > 0, '找不到完成记录的行头');
  const head = card.slice(at, card.indexOf('notify-item-time', at));

  const mcode = /<span v-if="([^"]+)" class="origin-chip origin-chip-kernel"><span class="minimax-wordmark">MiniMax<\/span><\/span>/.exec(head);
  assert.ok(mcode, 'MiniMax 来源 chip 必须按 item.kernel 分支渲染');
  assert.ok(
    /<span v-else class="origin-chip origin-chip-kernel">DSH<\/span>/.test(head),
    'DSH 必须留 v-else 兜底分支：旧载荷没有 kernel 字段',
  );
  const pick = (kernel) => (evalWith(mcode[1], { item: { kernel } }) ? 'MiniMax' : 'DSH');
  assert.equal(pick('mcode'), 'MiniMax', 'item.kernel === mcode 必须渲染 MiniMax chip');
  assert.equal(pick('dsh'), 'DSH');
  assert.equal(pick(undefined), 'DSH', '缺失 kernel 按 DSH 兜底');
  assert.equal(pick('MC'), 'DSH', '未知内核不许标成 MiniMax');

  // chip 在标题前；时间行与问答行的结构不动。
  assert.ok(head.indexOf('origin-chip') < head.indexOf('notify-item-title'), '来源 chip 要放在标题前');
  const item = card.slice(at);
  assert.match(item, /<span class="notify-item-time">/);
  assert.equal((item.match(/class="notify-item-line"/g) || []).length, 2, '问答两行仍是原来的结构');
  assert.equal((item.match(/class="notify-item-role"/g) || []).length, 2);
}

/** 卡片里的一句提示：返回它的出现条件与整段模板。 */
function hint(card, marker) {
  const at = card.indexOf(marker);
  assert.ok(at > 0, `找不到提示：${marker}`);
  const block = card.slice(card.lastIndexOf('<p', at), card.indexOf('</p>', at) + '</p>'.length);
  const cond = /v-if="([^"]+)"/.exec(block.slice(0, block.indexOf('>')));
  assert.ok(cond, `提示必须带出现条件：${marker}`);
  return { block, expression: cond[1] };
}

function assertMcodeHint(src) {
  const { block, expression } = hint(notificationCard(src), '未连接 MiniMax 事件流');
  // v-if 只看真假：短路表达式在 view 为空时返回 undefined，按不显示处理。
  const visible = ({ experience = true, view = { running: true }, watchingMcode = false }) =>
    Boolean(
      evalWith(expression, {
        mcodeExperience: experience,
        mcode: { view },
        notificationStore: { watchingMcode },
      }),
    );
  assert.equal(visible({}), true, '体验开 + 工作台运行 + 未连接 → 提示');
  assert.equal(visible({ experience: false }), false, '体验关着时不该出现');
  assert.equal(visible({ view: { running: false } }), false, 'MiniMax 没在运行时不提示（没有事件可丢）');
  assert.equal(visible({ view: null }), false, '状态还没读到时不能先报未连接');
  assert.equal(visible({ watchingMcode: true }), false, '已连接时不提示');

  // 有错误时把原因一起显示，别只留一句「未连接」。
  const errCond = /v-if="([^"]*mcodeError[^"]*)"/.exec(block);
  assert.ok(errCond, 'mcodeError 有值时要显示具体原因');
  const hasError = (mcodeError) =>
    Boolean(evalWith(errCond[1], { notificationStore: { mcodeError } }));
  assert.equal(hasError('MiniMax 事件流 403'), true);
  assert.equal(hasError(null), false);
  assert.equal(hasError(''), false, '空串不算错误');
  assert.match(block, /\{\{\s*notificationStore\.mcodeError\s*\}\}/, '提示里要带出 mcodeError 的内容');
}

function assertDshHint(src) {
  const { block, expression } = hint(notificationCard(src), '未连接 DSH 事件流');
  assert.equal(expression.trim(), '!notificationStore.watching', 'DSH 提示仍只在 DSH 事件流断开时出现');
  assert.equal(evalWith(expression, { notificationStore: { watching: false } }), true);
  assert.equal(evalWith(expression, { notificationStore: { watching: true } }), false);
  // 两条提示可能同时出现，文案要分得清是哪一侧断了。
  const text = block.replace(/<[^>]+>/g, '');
  assert.match(text, /未连接/);
  assert.match(text, /DSH/);
  assert.match(text, /不会提醒/);
}

test('设置页任务通知卡标题恒为「任务通知」', () => {
  assertFixedTitle(panel);
});

test('任务通知卡的 ⓘ 说明两个工作台都会通知、列表标出来源', () => {
  const card = notificationCard(panel);
  const tip = /<div class="card-info-tooltip">([\s\S]*?)<\/div>/.exec(card);
  assert.ok(tip, '缺少 ⓘ 说明');
  const text = tip[1].replace(/\s+/g, '');
  assert.match(text, /DSH/);
  assert.match(text, /MiniMax/);
  assert.match(text, /都会通知/);
  assert.match(text, /标出来源/);
  assert.equal((text.match(/。/g) || []).length, 1, 'ⓘ 保持一句话，别铺成整段');
});

test('完成记录按 item.kernel 渲染来源 chip：MiniMax / DSH 两个分支', () => {
  assertKernelChips(panel);
});

test('MiniMax 未连接提示只在「体验开 + 运行中 + 未连接」时出现，并带 mcodeError', () => {
  assertMcodeHint(panel);
});

test('DSH 未连接提示的条件与图文意图不回退', () => {
  assertDshHint(panel);
});

test('MiniMax 字样只挂 .minimax-wordmark，组件里不自己写颜色 / 字族', () => {
  const card = notificationCard(panel);
  const headAt = card.indexOf('class="notify-item-head"');
  const head = card.slice(headAt, card.indexOf('notify-item-time', headAt));
  assert.match(head, /<span class="minimax-wordmark">MiniMax<\/span>/);
  // 手写体 / 粉色归 theme.css 的全局类；scoped 的 [data-v] 特异性更高，重写会把品牌色盖回灰。
  const style = panel.slice(panel.indexOf('<style')).replace(/\/\*[\s\S]*?\*\//g, '');
  assert.doesNotMatch(style, /minimax-wordmark\s*\{/, '字号 / 手写体 / 粉色只归 theme.css');
  assert.doesNotMatch(style, /font-hand/, '手写体字族只定义一次');
  assert.doesNotMatch(head, /style="[^"]*(color|font)/, 'chip 不许内联颜色或字族');
});

test('反向验证：标题改回按内核分叉时判据变红', () => {
  const broken = panel.replace(
    /(\n\s*)任务通知(\s*\n)/,
    "$1{{ mcodeExperience ? 'DSH 任务通知' : '任务通知' }}$2",
  );
  assert.notEqual(broken, panel, '反向验证必须改到真实标题行');
  assert.throws(() => assertFixedTitle(broken), /标题不许再按内核改文案/);
});

test('反向验证：来源 chip 写死 DSH 时判据变红', () => {
  const broken = panel.replace(`v-if="item.kernel === 'mcode'"`, 'v-if="false"');
  assert.notEqual(broken, panel, '反向验证必须改到真实 chip 分支');
  assert.throws(() => assertKernelChips(broken), /item\.kernel === mcode 必须渲染 MiniMax chip/);
});
