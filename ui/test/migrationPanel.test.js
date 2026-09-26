import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const panel = readFileSync('ui/src/components/MigrationPanel.vue', 'utf8');

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
