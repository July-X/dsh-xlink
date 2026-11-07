import assert from 'node:assert/strict';
import { test } from 'node:test';

const calls = [];
let status = { use_approved: false, warning: null };
let records = [];
let failList = false;
let blockNextStatus = null;
globalThis.window = { __TAURI__: { core: { invoke: async (command, args) => {
  calls.push({ command, args });
  if (command === 'mcode_memory_status') {
    if (blockNextStatus) {
      const pending = blockNextStatus;
      blockNextStatus = null;
      await pending;
    }
    return status;
  }
  if (command === 'mcode_memory_list') {
    if (failList) throw new Error('读取失败');
    return { status, memories: records };
  }
  if (command === 'mcode_memory_set_enabled') status = { ...status, use_approved: args.useApproved };
  if (command === 'mcode_memory_save') records = [{ id: 'memory-1', state: 'accepted', ...args.input }];
  if (command === 'mcode_memory_set_state') records = records.map((item) => ({ ...item, state: args.state }));
  if (command === 'mcode_memory_delete') records = records.filter((item) => item.id !== args.id);
  return null;
} } } };

const memory = await import('../src/kernel/mcodeMemory.js');
const { mcode } = await import('../src/kernel/mcode.js');
mcode.view = { instance_id: 'mcode-a', memory_profile: 'web' };

test('记忆操作携带绑定的实例，不接受其他工作台路径', async () => {
  memory.resetMcodeMemory();
  await memory.refreshMcodeMemory();
  assert.equal(memory.mcodeMemory.status.use_approved, false);
  assert.equal(await memory.setMcodeMemoryEnabled(true), true);
  assert.equal(memory.mcodeMemory.status.use_approved, true);
  assert.equal(await memory.saveMcodeMemory('标题', '内容'), true);
  assert.equal(memory.mcodeMemory.memories[0].body, '内容');
  assert.equal(await memory.setMcodeMemoryState('memory-1', 'disabled'), true);
  assert.equal(memory.mcodeMemory.memories[0].state, 'disabled');
  assert.equal(await memory.deleteMcodeMemory('memory-1'), true);
  assert.equal(memory.mcodeMemory.memories.length, 0);
  assert.deepEqual(calls.find((call) => call.command === 'mcode_memory_save').args, {
    instanceId: 'mcode-a',
    profile: 'web',
    input: { kind: 'memory', scope: 'general', title: '标题', body: '内容' },
  });
  for (const { args } of calls) {
    assert.equal(args.instanceId, 'mcode-a');
    assert.equal(args.profile, 'web');
  }
});

test('写入成功后刷新失败不误称保存失败，也不保留旧列表', async () => {
  failList = true;
  assert.equal(await memory.saveMcodeMemory('新标题', '新内容'), true);
  assert.equal(memory.mcodeMemory.status, null);
  assert.deepEqual(memory.mcodeMemory.memories, []);
  assert.match(memory.mcodeMemory.error, /操作已保存，但刷新失败/);
  failList = false;
  await memory.refreshMcodeMemory();
  assert.equal(memory.mcodeMemory.memories[0].title, '新标题');
});

test('读取失败时失效旧目标，刷新前不允许继续写入', async () => {
  await memory.refreshMcodeMemory();
  assert.ok(memory.mcodeMemory.target);
  failList = true;
  await memory.refreshMcodeMemory();
  assert.equal(memory.mcodeMemory.target, null);
  const count = calls.filter(call => call.command === 'mcode_memory_set_enabled').length;
  assert.equal(await memory.setMcodeMemoryEnabled(false), false);
  assert.equal(calls.filter(call => call.command === 'mcode_memory_set_enabled').length, count);
  failList = false;
  await memory.refreshMcodeMemory();
});

test('切换实例后旧请求不能阻止新实例刷新或覆盖结果', async () => {
  let release;
  blockNextStatus = new Promise((resolve) => { release = resolve; });
  const previous = memory.refreshMcodeMemory();
  memory.resetMcodeMemory();
  mcode.view = { instance_id: 'mcode-b', memory_profile: 'web' };
  records = [{ id: 'new-instance', title: '新实例', state: 'accepted' }];
  await memory.refreshMcodeMemory();
  assert.equal(memory.mcodeMemory.memories[0].title, '新实例');
  const listCalls = calls.filter((call) => call.command === 'mcode_memory_list').length;
  release();
  await previous;
  assert.equal(memory.mcodeMemory.memories[0].title, '新实例');
  assert.equal(calls.filter((call) => call.command === 'mcode_memory_list').length, listCalls);
});

test('编辑携带读取版本并保留范围与元数据，不传其他工作台的项目 ID', async () => {
  const record = {
    id: 'memory-edit', revision: 7, scope: 'workspace', workspace_id: 'derived-id',
    workspace_path: 'C:/isolated/project', keywords: ['偏好'], pinned: true,
    verification: 'verified', state: 'disabled',
  };
  assert.equal(await memory.saveMcodeMemory('改标题', '改正文', record), true);
  const input = calls.filter(call => call.command === 'mcode_memory_save').at(-1).args.input;
  assert.deepEqual(input, {
    kind: 'memory', id: 'memory-edit', expected_revision: 7, scope: 'workspace',
    workspace_path: 'C:/isolated/project', keywords: ['偏好'], pinned: true,
    verification: 'verified', title: '改标题', body: '改正文',
  });
  assert.equal(record.revision, 7);
});
