import assert from 'node:assert/strict';
import { test } from 'node:test';

let release;
globalThis.window = { __TAURI__: { core: { invoke: async () => new Promise(resolve => { release = resolve; }) } } };
const { mcode } = await import('../src/kernel/mcode.js');
const { memoryShares, refreshMemoryShares, resetMemoryShares } = await import('../src/memory/share.js');

test('目标消失时清除授权并结束加载，迟到的旧结果不可恢复', async () => {
  resetMemoryShares();
  mcode.view = { instance_id: 'mcode-a', memory_profile: 'web' };
  const oldRequest = refreshMemoryShares();
  assert.equal(memoryShares.loading, true);
  mcode.view = { instance_id: null, memory_profile: 'web' };
  await refreshMemoryShares();
  assert.equal(memoryShares.loading, false);
  assert.deepEqual(memoryShares.items, []);
  assert.equal(memoryShares.target, null);
  release([{ record_id: 'old' }]);
  await oldRequest;
  assert.deepEqual(memoryShares.items, []);
  assert.equal(memoryShares.loaded, false);
  assert.equal(memoryShares.loading, false);
});
