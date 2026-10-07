import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const panel = readFileSync('ui/src/migration/MigrationPanel.vue', 'utf8');

test('迁移历史行直接 v-for 渲染，回滚按钮按行挂 loading', () => {
  // 2b02867 把 el-table 换成纯 div 行列表：slot scope 已不存在，
  // 「空 slot scope 渲染崩溃」的防御随之从模板层移除，这里钉住新结构。
  assert.doesNotMatch(
    panel,
    /<template #default="\{ row \}">/,
    '迁移历史不能回到 slot 参数阶段直接解构 row 的写法',
  );
  assert.match(panel, /v-for="row in historyList"/);
  assert.match(panel, /@click="onRollback\(row\.migration_id\)"/);
  assert.match(
    panel,
    /:loading="isLoading\('migrationRollback'\) && migrationStore\.rollbackInFlight"/
  );
});

// 取「某张表的第 n 列」那条规则里声明的 width / white-space。
// 注意匹配的是**声明值**而不是原始子串：2026-10-07 第一版断言比的是两段起点
// 不同的子串，结果把两列并进同一条规则它照样绿——那种断言永远抓不到回归。
function colRule(table, n) {
  const m = panel.match(
    new RegExp(`\\.${table} th:nth-child\\(${n}\\)[^{]*\\{([^}]*)\\}`)
  );
  return m ? m[1] : null;
}

test('发现表与结果表都走固定列宽，数字列不折行', () => {
  // 宽版（1040）下自动布局会把「文件数」拆成「文件 / 数」、「896.7 KiB」拆成
  // 两行。改成 table-layout 固定后列宽不再随内容生长，于是**宽度变成承重的**：
  // 给窄了不会自动换行，而是文字直接溢出表格右边界（2026-10-07 就是把「大小」
  // 和「文件数」并进同一条 52px 规则，导致 896.7 KiB 跑到边框外）。
  assert.match(panel, /class="preview-table preview-table--sources"/);
  assert.match(panel, /class="preview-table preview-table--result"/);
  assert.match(
    panel,
    /\.preview-table--sources,\s*\n\.preview-table--result \{ table-layout: fixed; \}/
  );

  // 「文件数」是 3 个汉字，「大小」最长的值约 60px：内容量不同，宽度就不能相同。
  const count = colRule('preview-table--sources', 4);
  const size = colRule('preview-table--sources', 5);
  assert.ok(count, '「文件数」列没有自己的宽度规则');
  assert.ok(size, '「大小」列没有自己的宽度规则');
  assert.match(count, /width:\s*\d+px/);
  assert.match(size, /width:\s*\d+px/);
  assert.notEqual(
    count.match(/width:\s*\d+px/)[0],
    size.match(/width:\s*\d+px/)[0],
    '「文件数」和「大小」不能共用同一个宽度：固定布局下窄了是溢出，不是换行'
  );
  for (const [name, decl] of [
    ['文件数', count],
    ['大小', size],
  ]) {
    assert.match(
      decl,
      /white-space:\s*nowrap/,
      `「${name}」列必须 nowrap，否则固定布局下表头自己会竖排成「文件 / 数」`
    );
  }

  // 结果表：两个计数列同理；第 4 列是备份路径，必须留得住能折行。
  for (const n of [2, 3]) {
    const decl = colRule('preview-table--result', n);
    assert.ok(decl, `结果表第 ${n} 列没有宽度规则`);
    assert.match(decl, /white-space:\s*nowrap/);
  }
  assert.equal(
    colRule('preview-table--result', 4),
    null,
    '备份路径列不该被固定列宽规则覆盖：长路径要能折行，截断比换行更容易读错'
  );
});

// --- 「点击无反应」这一类 bug -----------------------------------------------
//
// 2026-10-08 用户实测报：数据迁移面板底部「重新扫描 / 再次询问迁移」点了没反应。
// 真因是两条**静默空操作**——一个把「重新扫描」的结果咽下去，一个在注释里
// 承诺了一句「轻提示」而那句提示从未被写出来。这类 bug 有一个共同形状：
// **它没有抛错、没有告警、状态也没变**，所以任何以「有没有报错」为准的检查
// 都看不见它。判据只能直接盯「这条分支上有没有给用户留下落点」。

/** 剥掉注释后的脚本块——注释里写着「轻提示」「不弹窗」等解释，扫全文会自命中。 */
function script(src) {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');
}

test('「再次询问迁移」在没有可迁移内容时必须说原因，不能静默 return', () => {
  const body = script(panel).slice(
    script(panel).indexOf('async function onReopenPrompt'),
    script(panel).indexOf('function toggleSource')
  );
  assert.ok(body.length > 0, '找不到 onReopenPrompt');
  // 找到那条 early-return 分支，检查它从 `return` 往上能找到一次 toast。
  const guard = body.indexOf('if (!migrationStore.hasMigratable)');
  assert.ok(guard > 0, '应保留「没有可迁移内容就不开弹窗」这道门');
  const upToReturn = body.slice(guard, body.indexOf('return', guard));
  assert.match(
    upToReturn,
    /toast\(/,
    '这道门之后必须有一次 toast：不开弹窗是对的，但必须说原因——' +
      '否则用户分不清「没反应」和「点了但没用」',
  );
});

test('「重新扫描」两条成功分支都要有落点，不只是换个 spinner', () => {
  const body = script(panel).slice(
    script(panel).indexOf('async function refreshPreview'),
    script(panel).indexOf('function onFinish')
  );
  assert.ok(body.length > 0, '找不到 refreshPreview');
  // **两条分支都要查，不是一条。** 只要求「函数里出现过 toast」是不够的：
  // 扫到东西的那条分支有 toast，而「什么都没扫到」那条没有——后者恰恰是用户
  // 实测「点击无反应」时所处的路径，而那种判据会照样绿（2026-10-08 反向验实测）。
  const emptyGuard = body.indexOf('if (!migratable.length)');
  assert.ok(emptyGuard > 0, '应保留「没扫到东西」这条早退分支');
  assert.match(
    body.slice(emptyGuard, body.indexOf('return', emptyGuard)),
    /toast\(/,
    '「未检测到可迁移数据」这条分支必须 toast：页面内容不变时只闪一下 spinner，' +
      '用户看不出这一下到底干了什么',
  );
  assert.match(
    body.slice(body.indexOf('return', emptyGuard)),
    /toast\(/,
    '「扫到了东西」这条分支也必须 toast：扫到了几项、共多大，用户要能从页面上核对',
  );
});

test('「再次询问迁移」触发了 IO，必须挂 loading', () => {
  // 它内部会调 migration_skip_clear、必要时再 migration_preview——两个都是
  // 真 IO。本仓纪律：触发 IO 的按钮必须挂 loading，否则一次慢请求看起来就像
  // 「点了没反应」。
  assert.match(
    panel,
    /:loading="isLoading\('migrationRePrompt'\)"[\s\S]{0,80}@click="onReopenPrompt"/,
  );
});
