import assert from 'node:assert/strict';
import test from 'node:test';
import { confirmFirstRelease } from './confirm-first-release.mjs';

test('缺少更新资产时只有明确没有正式 Release 才放行', async () => {
  await confirmFirstRelease(async () => ({ status: 404 }));
  for (const status of [200, 503]) {
    await assert.rejects(confirmFirstRelease(async () => ({ status })), /无法确认首次发布/);
  }
});

test('403 单独提示疑似限流，不再说成资产问题', async () => {
  await assert.rejects(confirmFirstRelease(async () => ({ status: 403 })), /限流/);
  await assert.rejects(
    confirmFirstRelease(async () => ({ status: 403 })),
    (error) => {
      assert.doesNotMatch(error.message, /latest\.json/);
      return true;
    },
  );
});

test('带 GITHUB_TOKEN 调 API：未认证配额 60 次/小时/IP 会被耗尽成 403', async () => {
  let seen = null;
  await confirmFirstRelease(
    async (_url, options) => {
      seen = options;
      return { status: 404 };
    },
    { GITHUB_TOKEN: 'test-token' },
  );
  assert.equal(seen.headers.Authorization, 'Bearer test-token');
  await confirmFirstRelease(
    async (_url, options) => {
      seen = options;
      return { status: 404 };
    },
    {},
  );
  assert.equal(seen.headers.Authorization, undefined);
});
