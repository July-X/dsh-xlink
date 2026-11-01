// 桥接未注入时的 IPC 失败必须是 transport，而不是一条后端业务错误。
//
// 单独一个文件：bridge.js 在模块求值期捕获 window.__TAURI__，同一进程里
// 无法在已注入 core 之后再测「没有 core」的路径。
import assert from 'node:assert/strict';
import test from 'node:test';

globalThis.window = { navigator: { userAgent: 'node' } };
const { ipcCall } = await import('../src/shell/ipc-contract.js');
const { IpcCallError } = await import('../src/shell/ipc-schema.js');

test('纯浏览器 / 桥未注入：判为 transport 并保留可读提示', async () => {
  await assert.rejects(
    () => ipcCall('get_status', {}),
    (error) =>
      error instanceof IpcCallError &&
      error.kind === 'transport' &&
      error.code === 'ipc.transport' &&
      /Tauri bridge 未注入/.test(error.message)
  );
});
