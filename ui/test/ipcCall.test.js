// 试点 IPC 包装器的行为判据（2026-11 OPT-12）。
//
// 契约校验不是只在单测里跑：store 真正调用的是 ipcCall，这里用假桥验证
// 正常、请求不合法、业务失败、响应漂移四条路径，避免「契约测试绿、调用点另写一份」。
import assert from 'node:assert/strict';
import test from 'node:test';
import { statusView } from './lib/ipc-fixtures.mjs';

let calls = [];
let nextResult = statusView();
const core = {
  invoke(command, args) {
    calls.push({ command, args });
    return typeof nextResult === 'function' ? nextResult() : Promise.resolve(nextResult);
  },
  Channel: class {
    onmessage = null;
  },
};
globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };

const { ipcCall } = await import('../src/shell/ipc-contract.js');
const { IpcCallError, IpcContractError } = await import('../src/shell/ipc-schema.js');

test('正常调用：请求过契约、响应过契约、原样返回数据', async () => {
  calls = [];
  nextResult = statusView({ shell_version: '0.6.5' });
  const view = await ipcCall('get_status', { source: 'test' });
  assert.equal(view.shell_version, '0.6.5');
  assert.deepEqual(calls, [{ command: 'get_status', args: { source: 'test' } }]);
});

test('请求不合法：在到达桥之前就按契约失败', async () => {
  calls = [];
  await assert.rejects(
    () => ipcCall('get_status', { source: 42 }),
    (error) => error instanceof IpcContractError && /source/.test(error.message)
  );
  assert.equal(calls.length, 0, '非法请求不许到达 invoke');
  await assert.rejects(
    () => ipcCall('get_model_usage', { unknown_param: true }),
    (error) => error instanceof IpcContractError && /unknown_param/.test(error.message)
  );
});

test('业务失败：保留后端文案并归为 business', async () => {
  calls = [];
  nextResult = () => Promise.reject(new Error('数据目录不可读'));
  await assert.rejects(
    () => ipcCall('stop_kernel'),
    (error) =>
      error instanceof IpcCallError &&
      error.kind === 'business' &&
      error.code === 'ipc.business' &&
      error.message === '数据目录不可读'
  );
});

test('响应漂移：契约拒收，并归为 contract 而不是业务失败', async () => {
  calls = [];
  nextResult = { kernel: { running: false } };
  await assert.rejects(
    () => ipcCall('get_status', {}),
    (error) => error instanceof IpcContractError && /shell_version/.test(error.message)
  );
});
