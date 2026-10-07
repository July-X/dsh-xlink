// 控制塔「需要关注」的首屏上限与优先级（审查 R2-P2-04）。
//
// 设计 §7.2 写的是「首屏最多三条关注事项，更多内容通过『查看全部』进入详情」。
// 过去没有上限，于是事故、设置告警、另一壳工作台、预检、Node 五条一起出现时
// 会把「系统健康」和「最近操作」两块推到首屏以下——而那两块才是用户进概览
// 最常看的内容。（uiv2 改版把窗口从 480×800 放到 1040×748，测试理由跟着
// 改写；上限仍是三条，因为右半栏要同时放下这两张卡与「套餐用量」。）
//
// 加了上限之后**顺序就成了这件事本身**：代码里的 push 顺序把「未找到
// Node.js」排在最后，于是它恰好是最容易被挤掉的那条。这两条钉的是「前三条
// 是谁」，不是「有三条」。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const SOURCE = readFileSync(new URL('../src/diagnostics/ControlTower.vue', import.meta.url), 'utf8');

/** 从 computed 源码里抽出 `rank: N` 附近的 key，还原「哪条排第几」。 */
function rankOf(key) {
  const re = new RegExp("key: '" + key + "',\\s*\\n\\s*rank: (\\d+)");
  const m = SOURCE.match(re);
  assert.ok(m, `源码里找不到 key: '${key}' 的 rank —— 关注项改了形状，这条测试会瞎`);
  return Number(m[1]);
}

test('首屏上限是三条，且它是一个具名常量', () => {
  assert.match(
    SOURCE,
    /const ATTENTION_FIRST_SCREEN = 3;/,
    '首屏条数要写成具名常量：写成字面量 3 的话，改的人不知道自己动的是设计 §7.2'
  );
});

test('优先级是显式的 rank，不是 push 的顺序', () => {
  // rank 越小越先显示。这条测试就是防「有人在列表中间插一条忘了给 rank」
  // ——插在事故与 Node 之间的新问题，默认就会排到最前面。
  assert.ok(rankOf('incident') < rankOf('node'), '工作台没起来 > 缺 Node');
  assert.ok(rankOf('node') < rankOf('precheck'), '缺 Node > 预检没验过');
  assert.ok(rankOf('precheck') < rankOf('settings'), '预检没验过 > 设置回退默认值');
  assert.ok(rankOf('settings') < rankOf('other-shell'), '设置告警 > 只是提醒的另一壳');
});

test('每一条关注项都带 rank，没有默认值漏网', () => {
  const keys = [...SOURCE.matchAll(/key: '([a-z-]+)',\s*\n\s*rank: \d+/g)].map((m) => m[1]);
  assert.ok(keys.length >= 5, `只认到 ${keys.length} 条带 rank 的关注项`);
  // 反向查：源码里出现过的关注项 key 都要出现在带 rank 的那一组里。
  const allKeys = [...SOURCE.matchAll(/key: '([a-z-]+)'/g)].map((m) => m[1]);
  const missing = [...new Set(allKeys)].filter((k) => !keys.includes(k));
  assert.deepEqual(missing, [], '这些关注项没有 rank，排序时会落到未定义的位置');
});

test('上限之外的项有「查看全部」入口，且不静默丢弃', () => {
  assert.match(
    SOURCE,
    /const attentionHidden = computed\(\(\) => Math\.max\(0, attentionAll\.value\.length - ATTENTION_FIRST_SCREEN\)\)/,
    '要算清被收起的有几条，否则用户会以为「只有 3 个问题」'
  );
  // 模板里是「收起 : `还有 ${attentionHidden} 项`」这样一个三元 + 模板字面量，
  // 所以只能断它带上了 attentionHidden，不能断整句文案。
  assert.match(SOURCE, /还有 \$\{attentionHidden\} 项/, '收起时要说清还剩几项');
  assert.match(SOURCE, /v-if="attentionHidden > 0 \|\| attentionExpanded"/, '不足三条时不该出现这个入口');
  // 收起时也不能把标题旁的计数改成 3——那会让「4 项」显示成「3 项」。
  assert.match(SOURCE, /\{\{ attentionAll\.length \}\} 项/, '标题上的计数要报总数');
});
