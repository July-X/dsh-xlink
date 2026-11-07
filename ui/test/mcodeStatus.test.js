import assert from 'node:assert/strict';
import test from 'node:test';

let invoke;
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invoke(...args) } } };
const { mcode, refreshMcodeStatus, loadMcodeAvailableVersions, loadMcodeInstalledVersions } = await import('../src/kernel/mcode.js');

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

test('撤销授权或更换实例后，不保留之前的 Ponytail 更新结果', async () => {
  mcode.view = { instance_id: 'first', ponytail_enabled: true };
  mcode.ponytailUpdate = { update_available: true };
  invoke = () => Promise.resolve({ instance_id: 'first', ponytail_enabled: false });
  await refreshMcodeStatus();
  assert.equal(mcode.ponytailUpdate, null);
  mcode.ponytailUpdate = { update_available: true };
  invoke = () => Promise.resolve({ instance_id: 'second', ponytail_enabled: true });
  await refreshMcodeStatus();
  assert.equal(mcode.ponytailUpdate, null);
});

test('版本查询失败清除旧结果，本地与远程错误各自独立', async () => {
  invoke = command => command === 'list_mcode_installed_versions'
    ? Promise.reject(new Error('local'))
    : Promise.resolve([{ version: '0.1.7' }]);
  await loadMcodeInstalledVersions();
  await loadMcodeAvailableVersions();
  assert.match(mcode.installedVersionsError, /local/);
  assert.equal(mcode.versionsLoaded, true);
  invoke = () => Promise.reject(new Error('offline'));
  await loadMcodeAvailableVersions();
  assert.equal(mcode.versionsLoaded, false);
  assert.deepEqual(mcode.availableVersions, []);
  assert.match(mcode.availableVersionsError, /offline/);
  assert.match(mcode.installedVersionsError, /local/);
});


test('已安装版本读取去重，失败释放加载态并允许重试', async () => {
  const { isLoading } = await import('../src/shell/loading.js');
  let rejectRead;
  let calls = 0;
  invoke = command => {
    assert.equal(command, 'list_mcode_installed_versions');
    calls++;
    return new Promise((resolve, reject) => { rejectRead = reject; });
  };
  const first = loadMcodeInstalledVersions();
  assert.equal(isLoading('mcodeInstalledVersions'), true);
  await loadMcodeInstalledVersions();
  assert.equal(calls, 1);
  rejectRead(new Error('无法读取'));
  await first;
  assert.equal(isLoading('mcodeInstalledVersions'), false);
  assert.match(mcode.installedVersionsError, /无法读取/);
  invoke = () => Promise.resolve([{ version: '0.1.7' }]);
  await loadMcodeInstalledVersions();
  assert.equal(mcode.installedVersionsError, '');
  assert.equal(mcode.installedVersions[0].version, '0.1.7');
});
