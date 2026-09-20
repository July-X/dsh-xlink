import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const panel = readFileSync('ui/src/components/MigrationPanel.vue', 'utf8');

test('迁移历史表格防御空 slot scope，避免二次打开时渲染崩溃', () => {
  assert.doesNotMatch(
    panel,
    /<template #default="\{ row \}">/,
    '迁移表格操作列不能在 slot 参数阶段直接解构 row',
  );
  assert.match(panel, /<template #default="scope">/);
  assert.match(panel, /v-if="scope && scope.row"/);
  assert.match(panel, /@click="onRollback\(scope\.row\.migration_id\)"/);
});
