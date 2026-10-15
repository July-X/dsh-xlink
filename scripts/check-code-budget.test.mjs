// 门禁自己也要有测试：check-code-budget.mjs 是唯一一份「决定别人能不能
// 提交」的脚本，而它的两条判定（反棘轮管谁、提示文案说什么）此前完全没有
// 自测——2026-10-15 那次「拆分模块 or 上调数字」的误导就是这么漏出去的。
//
// 用法：`node --test scripts/check-code-budget.test.mjs`
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SCRIPT = 'scripts/check-code-budget.mjs';
const text = readFileSync(join(root, SCRIPT), 'utf8');

/** 取源码里的 `const NAME = <数字>;`，测常量本身没被改小。 */
function constant(name) {
  const m = text.match(new RegExp(`const ${name} = (\\d+);`));
  assert.ok(m, `${name} 应在脚本里定义`);
  return Number(m[1]);
}

function budgetsOf(src) {
  const block = src.match(/FILE_BUDGETS\s*=\s*\{([\s\S]*?)\n\};/);
  assert.ok(block, 'FILE_BUDGETS 应存在');
  const out = new Map();
  for (const line of block[1].split('\n')) {
    const m = line.match(/^\s*'([^']+)'\s*:\s*(\d+)/);
    if (m) out.set(m[1], Number(m[2]));
  }
  return out;
}

const budgets = budgetsOf(text);
const head = budgetsOf(
  execFileSync('git', ['show', `HEAD:${SCRIPT}`], { cwd: root, encoding: 'utf8', maxBuffer: 8e6 }),
);

test('阈值切在「关注点数量」的分界上，不是随手挑的数', () => {
  const T = constant('RATCHET_THRESHOLD');
  const managed = [...budgets].filter(([, v]) => v >= T);
  const exempt = [...budgets].filter(([, v]) => v < T);

  // 两个区间都不能空：阈值挪到最上/最下会让其中一条规则彻底失效。
  assert.ok(managed.length > 0, '应至少有一个文件受反棘轮管');
  assert.ok(exempt.length > managed.length * 3, '豁免区应是多数，否则门槛形同虚设');

  // 关键不变量：**受管的必须是最大的那批**。任何被管的文件都不该比
  // 任何豁免的文件小——否则这条线切反了。
  const smallestManaged = Math.min(...managed.map(([, v]) => v));
  const largestExempt = Math.max(...exempt.map(([, v]) => v));
  assert.ok(
    smallestManaged >= largestExempt,
    `受管区最小值 ${smallestManaged} 必须 ≥ 豁免区最大值 ${largestExempt}`,
  );

  // 阈值与硬顶之间要留出「刚拆出来的模块能长大」的空间，否则等于禁止拆文件。
  const ceiling = constant('HARD_FILE_CEILING');
  assert.ok(ceiling > T, `硬顶 ${ceiling} 必须高于阈值 ${T}`);
});

test('超预算提示按「受不受反棘轮管」分岔', () => {
  // 受管的文件：提示必须说清「上调是死路」，否则会像 2026-10-15 那样
  // 让人以为「拆 / 调」二选一，实际只能拆。
  const T = constant('RATCHET_THRESHOLD');
  const ratcheted = [...budgets].find(([, v]) => v >= T);
  assert.ok(ratcheted, '需要一个受管样例');
  assert.ok(
    /受反棘轮管，预算只许下调/.test(text),
    '受管分支的提示必须写明预算只许下调',
  );

  // 不受管的文件：仍保留「可上调」这条路，别把话说绝。
  const EXEMPT_MSG = '请拆分模块，或在同一个提交里上调 FILE_BUDGETS 里的数字';
  assert.ok(text.includes(EXEMPT_MSG), '豁免分支必须保留「可上调」的提示');

  // 「可上调」这句**只能出现一次**，且必须落在豁免分支里。若它出现在
  // 受管分支（或与受管分支并列成同一条消息），受管文件就会再次被告知
  // 一条走不通的路——那正是原来的写法。
  const occurrences = text.split(EXEMPT_MSG).length - 1;
  assert.equal(
    occurrences,
    1,
    `「可上调」提示应只出现一次（当前 ${occurrences} 次）：它只能属于豁免分支`,
  );
  const ratchetBranch = text.slice(
    text.indexOf('受反棘轮管，预算只许下调'),
    text.indexOf(EXEMPT_MSG),
  );
  assert.ok(ratchetBranch.length > 0, '受管分支应存在且排在豁免分支之前');
  assert.ok(
    !ratchetBranch.includes('或在同一个提交里上调'),
    '受管分支里不得出现「可上调」',
  );
});

test('量法修正豁免按模块标识查，文件被搬进子目录后仍然认得', () => {
  // 豁免表按完整路径登记，而反棘轮的基线已经退化成按 `moduleId(path)`
  // 查——两者耐受度必须一致，否则文件一搬，豁免静默失守。
  assert.ok(
    /function correctionFor\(path\)/.test(text),
    '应有 correctionFor 做「按模块标识回退」的查法',
  );
  const body = text.match(/function correctionFor\(path\) \{[\s\S]*?\n\}/);
  assert.ok(body, 'correctionFor 应有函数体');
  assert.ok(
    /moduleId\(key\)\s*===\s*wanted/.test(body[0]),
    'correctionFor 必须用 moduleId 比较，而不只是按路径直查',
  );

  // 当前唯一的豁免项确实登记着一个真实存在的文件。键是**独占一行的
  // 路径字面量**——不要用 `'([^']+)'` 宽泛地取，那会把 `reason` 里带引号的
  // 说明文字（注释里的 `/*`）也当成路径。
  const block = text.match(/MEASUREMENT_CORRECTIONS\s*=\s*new Map\(\[([\s\S]*?)\n\]\);/);
  assert.ok(block, 'MEASUREMENT_CORRECTIONS 应存在');
  const keys = [...block[1].matchAll(/^\s*'([^']+)'\s*,?\s*$/gm)].map((m) => m[1]);
  assert.ok(keys.length > 0, '应至少解析出一项豁免');
  for (const key of keys) {
    assert.ok(
      readFileSync(join(root, key), 'utf8').length > 0,
      `豁免项 ${key} 应指向一个真实文件`,
    );
    assert.ok(
      head.has(key) || budgets.has(key),
      `豁免项 ${key} 应当仍登记在 FILE_BUDGETS 里`,
    );
  }
});
