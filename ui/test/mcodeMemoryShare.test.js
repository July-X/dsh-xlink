import assert from 'node:assert/strict';
import { test } from 'node:test';

let release;
let invokeMock = async () => new Promise(resolve => { release = resolve; });
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invokeMock(...args) } } };
const { mcode } = await import('../src/kernel/mcode.js');
const { memoryShares, refreshMemoryShares, resetMemoryShares, setMemoryShare } = await import('../src/memory/share.js');
const { ElMessageBox } = await import('element-plus/es/components/message-box/index.mjs');

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

test('共享页的逐条撤销不依赖旧页面内核选择', async () => {
  resetMemoryShares();
  mcode.view = { instance_id: 'mcode-a', memory_profile: 'web' };
  memoryShares.target = { instanceId: 'mcode-a', profile: 'web' };
  const calls = [];
  invokeMock = async (command, args) => { calls.push({ command, args }); return []; };
  const { pageFamilies } = await import('../src/kernel/instance.js');
  pageFamilies.memory = 'mcode';
  await setMemoryShare({ id: 'same-id', revision: 4 }, 'dsh/dsh-a/default', false);
  assert.deepEqual(calls[0], { command: 'set_mcode_memory_share', args: {
    instanceId: 'mcode-a', profile: 'web', sourceInstance: 'dsh-a', sourceProfile: 'default', recordId: 'same-id', revision: 4, shared: false,
  } });
  assert.equal(calls[1].command, 'list_mcode_memory_shares');
});

test('逐条授权确认期间目标切换，不得把旧确认发给新目标', async () => {
  resetMemoryShares();
  mcode.view = { instance_id: 'mcode-a', memory_profile: 'web' };
  memoryShares.target = { instanceId: 'mcode-a', profile: 'web' };
  const calls = [];
  invokeMock = async (command, args) => { calls.push({ command, args }); return []; };
  const oldConfirm = ElMessageBox.confirm;
  let confirm;
  ElMessageBox.confirm = () => new Promise(resolve => { confirm = resolve; });
  try {
    const pending = setMemoryShare({ id: 'same-id', title: '测试', revision: 4 }, 'dsh/dsh-a/default', true);
    mcode.view = { instance_id: 'mcode-b', memory_profile: 'web' };
    confirm();
    await pending;
    assert.deepEqual(calls, []);
    assert.equal(memoryShares.changing, false);
  } finally {
    ElMessageBox.confirm = oldConfirm;
  }
});
