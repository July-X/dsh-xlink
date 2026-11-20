import assert from 'node:assert/strict';
import test from 'node:test';
import { confirmFirstRelease } from './confirm-first-release.mjs';

test('缺少更新资产时只有明确没有正式 Release 才放行', async () => {
  await confirmFirstRelease(async () => ({ status: 404 }));
  for (const status of [200, 403, 503]) {
    await assert.rejects(confirmFirstRelease(async () => ({ status })), /无法确认首次发布/);
  }
});
