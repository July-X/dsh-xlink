import assert from 'node:assert/strict';
import test from 'node:test';

let invoke;
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invoke(...args) } } };
const { mcode, refreshMcodeStatus } = await import('../src/kernel/mcode.js');

test('较早的 MiniMax 状态响应不能覆盖后一次刷新', async () => {
  let release;
  invoke = () => new Promise(resolve => { release = resolve; });
  const old = refreshMcodeStatus();
  invoke = () => Promise.resolve({ running: true, port: 4321 });
  await refreshMcodeStatus();
  release({ running: false, port: 1234 });
  await old;
  assert.equal(mcode.view.running, true);
  assert.equal(mcode.view.port, 4321);
  assert.equal(mcode.loading, false);
});
