// 诊断页的日志入口只留头部「更多 → 查看完整日志」一个（2026-10-07）。
//
// 用户原话：「移除重复的查看日志按钮」「查看完整日志使用统一的查看日志弹出
// window 的方式来查看」「同时检查其他查看日志的入口」。
//
// 删之前那一层每个页面有**三个**日志入口，而且三个落到同一个文件：
//   · 头部「更多 → 查看完整日志」→ `openEvidence(diagnosticStore.evidencePath)`
//   · 页面底部栏的「查看日志」/「查看完整日志」→ `openEvidence(report
//     .evidencePath)` 或无参 `openEvidence()`
//   · `RunTimeline` 失败行里行内的「查看日志」→ 无参 `openEvidence()`
// 行内那个挂在红色阶段旁边，看着像"这一步的日志"，其实开的是同一份——比多一个
// 入口更糟的是它给了用户一个**错的预期**。
//
// 内核状态页 2026-10-07 已经先删过一次底部栏（见 diagnostic-actions.js 的记录），
// 本次把剩下三个页面一并收掉，口径与它一致。
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import test from 'node:test';

const ROOT = resolve('.');
const DIR = resolve('ui/src/diagnostics');

const DIAGNOSIS_VUE = readdirSync(DIR)
  .filter((f) => f.endsWith('.vue'))
  .map((f) => join(DIR, f));

/**
 * 去掉注释再断言。
 *
 * 必须去掉：这几页的注释里全在解释「为什么删了那个按钮」，删的正是这两个词。
 * 断言的是「界面上没有这个按钮」，不是「文件里没出现过这个词」。
 */
const templateOf = (file) => {
  const src = readFileSync(file, 'utf8');
  const at = src.indexOf('<template>');
  return (at === -1 ? src : src.slice(at)).replace(/<!--[\s\S]*?-->/g, '');
};

test('诊断页正文里不再有日志入口按钮', () => {
  const hits = [];
  for (const file of DIAGNOSIS_VUE) {
    const tpl = templateOf(file);
    for (const m of tpl.matchAll(/查看完整日志|查看日志/g)) {
      const line = tpl.slice(0, m.index).split('\n').length;
      hits.push(`${relative(ROOT, file)}:${line} 「${m[0]}」`);
    }
  }
  assert.equal(
    hits.length,
    0,
    `以下页面正文里还留着日志入口，与头部「更多 → 查看完整日志」重复：\n  ${hits.join('\n  ')}\n` +
      '要开日志就调 `openEvidence()`，入口由 `diagnosis-more-menu.js` 统一收口。'
  );
});

test('唯一入口仍在：更多菜单第一项走 openEvidence', () => {
  const menu = readFileSync(join(DIR, 'diagnosis-more-menu.js'), 'utf8');
  assert.match(
    menu,
    /\{ key: 'logs', label: '查看完整日志'/,
    '「更多」菜单必须保留「查看完整日志」——它是删掉页面按钮之后唯一的入口'
  );
  assert.match(menu, /openEvidence/, '菜单项必须经 openEvidence 打开，不要自己读文件');
});

test('openEvidence 的回落保住插件页的沙盒日志', () => {
  // 这是本次删按钮**唯一有精度风险**的地方：PluginDiagnosis 原先显式传
  // `report.evidencePath`，删掉之后要靠回落链顶上来。而 evidencePath 是跨页
  // 共用的槽位（openPluginDiagnosis 切页不清它），预检没落下运行记录时会留着
  // 上一个页面的路径——照它开日志，用户拿到的是一份与本次预检无关的文件，
  // 而报告正文正指着真正的沙盒日志说「看这里」。所以这一条必须排在
  // `diagnosticStore.evidencePath` **前面**。
  const actions = readFileSync(join(DIR, 'diagnostic-actions.js'), 'utf8');
  const body = actions.slice(actions.indexOf('export function openEvidence'));
  // 从 `const target =` 开始切，别把函数签名里的形参 `path` 也算进来。
  const target = body.slice(body.indexOf('const target ='), body.indexOf('if (target)'));
  const order = [...target.matchAll(/(path|spec\?\.report\?\.evidencePath|diagnosticStore\.evidencePath)/g)].map(
    (m) => m[1]
  );
  assert.deepEqual(
    order.slice(0, 3),
    ['path', 'spec?.report?.evidencePath', 'diagnosticStore.evidencePath'],
    `回落链顺序错了，实际是 ${JSON.stringify(order)}：` +
      '传入路径 → 本页 spec 的报告证据 → 诊断状态里已记的那份'
  );
});

test('RunTimeline 不再留 row-actions 插槽', () => {
  // 删掉两个使用方之后，这个插槽没有调用方了，而它外面还包着一个 div——
  // 每个展开的时间线行都在渲染一个空盒子。留着它，下一个人很容易顺手再挂
  // 一个同义入口，而那个入口多半会和「更多」菜单打开同一份文件。
  const timeline = readFileSync(join(DIR, 'RunTimeline.vue'), 'utf8');
  assert.doesNotMatch(
    timeline.replace(/<!--[\s\S]*?-->/g, ''),
    /name="row-actions"/,
    'RunTimeline 仍有 row-actions 插槽：删掉最后两个使用方后它是无调用方的死代码'
  );
  const css = readFileSync(join(DIR, 'diagnostics.css'), 'utf8');
  assert.doesNotMatch(
    css,
    /\.diag-timeline__actions\s*\{/,
    '.diag-timeline__actions 只服务于那个插槽，不该留下'
  );
});
