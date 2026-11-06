import assert from 'node:assert/strict';
import test from 'node:test';

let invoke;
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invoke(...args) } } };
const { instanceStore, loadKernelFamily, selectKernelFamily } = await import('../src/kernel/instance.js');

test('迟到的读取不会覆盖用户选择，切换失败保持原选择', async () => {
  let release;
  invoke = (command, args) => {
    if (command === 'get_kernel_selection') return new Promise(resolve => { release = resolve; });
    assert.equal(command, 'set_kernel_selection');
    assert.equal(args.selection, 'mcode');
    return Promise.resolve();
  };
  const read = loadKernelFamily();
  assert.equal(await selectKernelFamily('mcode'), true);
  release('dsh');
  await read;
  assert.equal(instanceStore.family, 'mcode');
  assert.equal(instanceStore.familyLoaded, true);
  invoke = () => Promise.reject(new Error('无法保存'));
  assert.equal(await selectKernelFamily('dsh'), false);
  assert.equal(instanceStore.family, 'mcode');
  assert.match(instanceStore.familyError, /无法保存/);
  assert.equal(instanceStore.familySwitching, false);
  assert.equal(await selectKernelFamily('unknown'), false);
});
