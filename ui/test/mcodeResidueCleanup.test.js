import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { ElMessageBox } from 'element-plus/es/components/message-box/index.mjs';

let invoke;
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invoke(...args) } } };
const { mcode, discardMcodeRunRecord, refreshMcodeStatus } = await import('../src/kernel/mcode.js');
const { globalBusy, isLoading } = await import('../src/shell/loading.js');
const stale = () => ({ instance_id: 'minimax-test', stale_pid: null, recovery_snapshot: 'snapshot-a', port: 4567, miniapp_port: 4568 });
const deferred = () => {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
};

test('取消恢复不发送命令，确认框说明重启与不结束进程', async () => {
  mcode.view = stale();
  let calls = 0;
  invoke = () => { calls++; };
  let shown;
  ElMessageBox.confirm = async (message, _title, options) => {
    shown = { message, options };
    throw new Error('cancel');
  };
  assert.equal(await discardMcodeRunRecord(), false);
  for (const text of ['minimax-test', '重启电脑', '其他窗口', '不结束任何进程', '无法验证']) assert.ok(shown.message.includes(text), text);
  assert.match(shown.options.confirmButtonText, /已重启/);
  assert.equal(calls, 0);
  assert.equal(globalBusy.value, false);
});

test('仅剩占位也可确认，恢复绑定确认时的实例和快照', async () => {
  mcode.view = stale();
  const confirmation = deferred();
  const status = deferred();
  ElMessageBox.confirm = () => confirmation.promise;
  const calls = [];
  invoke = async (cmd, args) => {
    calls.push(cmd);
    if (cmd === 'discard_mcode_run_record') {
      assert.deepEqual(args, { instanceId: 'minimax-test', expectedSnapshot: 'snapshot-a' });
      return null;
    }
    assert.equal(cmd, 'get_mcode_status');
    return status.promise;
  };
  const pending = discardMcodeRunRecord();
  assert.equal(globalBusy.value, true);
  assert.equal(isLoading('discardMcodeRunRecord'), true);
  await discardMcodeRunRecord();
  mcode.view = { ...stale(), instance_id: 'another-instance', recovery_snapshot: 'snapshot-b' };
  confirmation.resolve();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['discard_mcode_run_record', 'get_mcode_status']);
  assert.equal(globalBusy.value, true);
  status.resolve({ ...stale(), recovery_snapshot: null });
  assert.equal(await pending, true);
  assert.equal(globalBusy.value, false);
});

test('恢复失败的提示在同一快照轮询后保留，快照改变时清除', async () => {
  mcode.view = stale();
  ElMessageBox.confirm = async () => {};
  invoke = async cmd => {
    if (cmd === 'discard_mcode_run_record') throw new Error('旧窗口仍在运行');
    return stale();
  };
  assert.equal(await discardMcodeRunRecord(), false);
  assert.match(mcode.cleanupError, /旧窗口仍在运行/);
  await refreshMcodeStatus();
  assert.match(mcode.cleanupError, /旧窗口仍在运行/);
  invoke = async () => ({ ...stale(), recovery_snapshot: 'snapshot-b' });
  await refreshMcodeStatus();
  assert.equal(mcode.cleanupError, '');
});

test('恢复动作接入命令处理器和主窗口权限', () => {
  const commands = readFileSync(new URL('../../src-tauri/src/commands.rs', import.meta.url), 'utf8');
  const handler = commands.slice(commands.indexOf('pub async fn discard_mcode_run_record('), commands.indexOf('#[tauri::command]', commands.indexOf('pub async fn discard_mcode_run_record(')));
  assert.match(handler, /blocking\(move \|\|/);
  assert.match(handler, /mcode_service::discard_run_record\(&instance_id, &expected_snapshot\)/);
  assert.doesNotMatch(commands, /pub async fn cleanup_mcode_residue/);
  const registrations = readFileSync(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8');
  assert.match(registrations, /commands::discard_mcode_run_record,/);
  const acl = JSON.parse(readFileSync(new URL('../../src-tauri/permissions/app-commands.json', import.meta.url)));
  const local = acl.permission.find(permission => permission.identifier === 'allow-local-commands');
  assert.ok(local.commands.allow.includes('discard_mcode_run_record'));
  const capability = JSON.parse(readFileSync(new URL('../../src-tauri/capabilities/default.json', import.meta.url)));
  assert.ok(capability.windows.includes('main') && capability.permissions.includes('allow-local-commands'));
});

test('无旧记录不弹确认，占位单独存在时概览仍提供恢复入口', async () => {
  mcode.view = { ...stale(), recovery_snapshot: null };
  ElMessageBox.confirm = () => assert.fail('unexpected confirmation');
  invoke = () => assert.fail('unexpected IPC');
  assert.equal(await discardMcodeRunRecord(), false);
  const panel = readFileSync(new URL('../src/shell/OverviewPanel.vue', import.meta.url), 'utf8');
  assert.match(panel, /v-if="mcodeView\?\.recovery_snapshot"/);
  assert.match(panel, /@click="discardMcodeRunRecord"/);
  assert.doesNotMatch(panel, /cleanupMcodeResidue/);
});
