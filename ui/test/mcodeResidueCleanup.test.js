import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { ElMessageBox } from 'element-plus/es/components/message-box/index.mjs';

let invoke;
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invoke(...args) } } };
const { mcode, cleanupMcodeResidue, refreshMcodeStatus } = await import('../src/kernel/mcode.js');
const { globalBusy, isLoading } = await import('../src/shell/loading.js');
const stale = () => ({ instance_id: 'minimax-test', stale_pid: 12345, port: 4567, miniapp_port: 4568 });
const deferred = () => {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
};

test('取消清理不发送命令，确认文案包含实例、PID、双端口和安全边界', async () => {
  mcode.view = stale();
  let calls = 0;
  invoke = () => { calls++; };
  ElMessageBox.confirm = async (message, title) => {
    assert.match(title, /清理 MiniMax/);
    for (const text of ['minimax-test', '12345', '4567', '4568', '只停止能核验属于该实例', '归属不明', '中断正在执行的任务']) {
      assert.ok(message.includes(text), text);
    }
    throw new Error('cancel');
  };
  assert.equal(await cleanupMcodeResidue(), false);
  assert.equal(calls, 0);
  assert.equal(globalBusy.value, false);
});

test('确认使用原始快照，清理去重并持有全局忙碌态直到刷新完成', async () => {
  mcode.view = stale();
  const confirmation = deferred();
  const status = deferred();
  let confirms = 0;
  ElMessageBox.confirm = () => { confirms++; return confirmation.promise; };
  const calls = [];
  invoke = async (cmd, args) => {
    calls.push(cmd);
    if (cmd === 'cleanup_mcode_residue') {
      assert.deepEqual(args, { instanceId: 'minimax-test', expectedPid: 12345, port: 4567, miniappPort: 4568 });
    } else if (cmd === 'get_mcode_status') {
      return status.promise;
    } else assert.fail(cmd);
  };
  const pending = cleanupMcodeResidue();
  assert.equal(globalBusy.value, true);
  assert.equal(isLoading('cleanupMcodeResidue'), true);
  await cleanupMcodeResidue();
  assert.equal(confirms, 1);
  mcode.view = { ...stale(), stale_pid: 54321, port: 9999 };
  confirmation.resolve();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['cleanup_mcode_residue', 'get_mcode_status']);
  assert.equal(globalBusy.value, true);
  status.resolve({ ...stale(), stale_pid: null, running: false });
  assert.equal(await pending, true);
  assert.equal(mcode.view.stale_pid, null);
  assert.equal(globalBusy.value, false);
  assert.equal(isLoading('cleanupMcodeResidue'), false);
});

test('失败也刷新状态，已批准清理会作废此前的轮询响应', async () => {
  const oldStatus = deferred();
  const cleanup = deferred();
  invoke = () => oldStatus.promise;
  const old = refreshMcodeStatus();
  mcode.view = stale();
  ElMessageBox.confirm = async () => {};
  let refreshed = 0;
  invoke = async cmd => {
    if (cmd === 'cleanup_mcode_residue') {
      await cleanup.promise;
      throw new Error('归属不明，请手动检查');
    }
    assert.equal(cmd, 'get_mcode_status');
    refreshed++;
    return { ...stale(), error: '归属不明' };
  };
  const pending = cleanupMcodeResidue();
  await new Promise(resolve => setImmediate(resolve));
  oldStatus.resolve({ running: true, port: 9999 });
  await old;
  assert.equal(mcode.view.port, 4567);
  cleanup.resolve();
  assert.equal(await pending, false);
  assert.equal(refreshed, 1);
  assert.equal(mcode.view.error, '归属不明');
  assert.match(mcode.cleanupError, /归属不明，请手动检查/);
  await refreshMcodeStatus();
  assert.match(mcode.cleanupError, /归属不明，请手动检查/, '同一旧记录的轮询不能抹掉检查步骤');
  invoke = async () => ({ ...stale(), stale_pid: null });
  await refreshMcodeStatus();
  assert.equal(mcode.cleanupError, '', '清理完成后移除过时提示');
  assert.equal(globalBusy.value, false);
});

test('没有旧记录不会弹确认或调用清理；概览区分端口清理和仅记录清理', async () => {
  mcode.view = { ...stale(), stale_pid: null };
  ElMessageBox.confirm = () => assert.fail('unexpected confirmation');
  invoke = () => assert.fail('unexpected IPC');
  assert.equal(await cleanupMcodeResidue(), false);
  const panel = readFileSync(new URL('../src/shell/OverviewPanel.vue', import.meta.url), 'utf8');
  assert.match(panel, /<el-button\s+v-if="mcodeView\?\.stale_pid"[^>]+:loading="isLoading\('cleanupMcodeResidue'\)"[^>]+:disabled="mcodeBusy"[^>]+@click="cleanupMcodeResidue"/);
  assert.match(panel, /@click="discardMcodeRunRecord"\s*>\s*仅清理旧记录/);
  assert.match(panel, /if \(!globalBusy.value\) refreshMcodeStatus\(\)/);
});

test('实例和 PID 不变但任一端口变化时清除过期检查步骤', async () => {
  for (const changed of [{ port: 7777 }, { miniapp_port: 8888 }]) {
    mcode.view = stale();
    mcode.cleanupError = '请检查旧端口';
    mcode.cleanupErrorPid = 12345;
    mcode.cleanupErrorInstance = 'minimax-test';
    mcode.cleanupErrorPort = 4567;
    mcode.cleanupErrorMiniappPort = 4568;
    invoke = async () => ({ ...stale(), ...changed });
    await refreshMcodeStatus();
    assert.equal(mcode.cleanupError, '');
    assert.equal(mcode.cleanupErrorPort, null);
    assert.equal(mcode.cleanupErrorMiniappPort, null);
  }
});
