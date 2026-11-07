import assert from 'node:assert/strict';
import test from 'node:test';

let invoke;
globalThis.window = { __TAURI__: { core: { invoke: (...args) => invoke(...args) } } };
const { instanceStore, pageFamilies, loadKernelFamily, selectPageFamily } = await import('../src/kernel/instance.js');

test('每页各存一份内核选择：一页切换不带动另一页，迟到的读取不覆盖、失败保留原值', async () => {
  let release;
  invoke = (command, args) => {
    if (command === 'get_kernel_selection') return new Promise(resolve => { release = resolve; });
    assert.equal(command, 'set_kernel_selection');
    assert.ok(['dsh', 'mcode'].includes(args.selection));
    return Promise.resolve();
  };
  const read = loadKernelFamily();
  // 概览页切到 MiniMax：记忆页与内核版本页仍停在 DSH。
  assert.equal(await selectPageFamily('overview', 'mcode'), true);
  assert.equal(pageFamilies.overview, 'mcode');
  assert.equal(pageFamilies.versions, 'dsh');
  assert.equal(pageFamilies.memory, 'dsh');
  assert.equal(instanceStore.family, 'mcode', '落盘值跟最后一次切换走，只当重启种子');
  release('dsh');
  await read;
  assert.equal(pageFamilies.overview, 'mcode', '迟到的读取不许覆盖已切过的那一页');
  assert.equal(pageFamilies.versions, 'dsh', '迟到的读取也不许动没切过的页');
  // 全局落盘值已经是 mcode，但内核版本页自己还没切过：去重必须按页，不能按全局值。
  assert.equal(await selectPageFamily('versions', 'mcode'), true);
  assert.equal(pageFamilies.versions, 'mcode');
  assert.equal(pageFamilies.memory, 'dsh');
  // 记忆页切换失败：保持原选择并留下错误，busy 释放。
  invoke = () => Promise.reject(new Error('无法保存'));
  assert.equal(await selectPageFamily('memory', 'mcode'), false);
  assert.equal(pageFamilies.memory, 'dsh');
  assert.match(instanceStore.familyError, /无法保存/);
  assert.equal(instanceStore.familySwitching, false);
  // 点本页已选中的一项是空操作；非法页 / 非法内核族一律拒绝。
  assert.equal(await selectPageFamily('overview', 'mcode'), false);
  assert.equal(await selectPageFamily('nope', 'dsh'), false);
  assert.equal(await selectPageFamily('memory', 'unknown'), false);
  // 用户已切过一页，后续重读不应重新播种其他页面。
  invoke = (command) => {
    assert.equal(command, 'get_kernel_selection');
    return Promise.resolve('mcode');
  };
  await loadKernelFamily();
  assert.deepEqual({ ...pageFamilies }, { overview: 'mcode', versions: 'mcode', memory: 'dsh' });
  // 种子只播一次：重试 / 重读不会把各页已经挑好的选择冲回同一个值。
  invoke = () => Promise.resolve('dsh');
  await loadKernelFamily();
  assert.deepEqual({ ...pageFamilies }, { overview: 'mcode', versions: 'mcode', memory: 'dsh' });
});


test('连点重试去重，写入期间的重读不发请求，切换完成后仍能继续操作', async () => {
  let releaseRead;
  let reads = 0;
  invoke = () => { reads++; return new Promise(resolve => { releaseRead = resolve; }); };
  const read = loadKernelFamily();
  await loadKernelFamily();
  assert.equal(reads, 1);
  releaseRead('dsh');
  await read;

  let releaseWrite;
  let writes = 0;
  invoke = (command) => {
    assert.equal(command, 'set_kernel_selection');
    writes++;
    return new Promise(resolve => { releaseWrite = resolve; });
  };
  const write = selectPageFamily('overview', 'dsh');
  assert.equal(await loadKernelFamily(), false);
  assert.equal(await selectPageFamily('versions', 'dsh'), false);
  assert.equal(writes, 1);
  releaseWrite();
  assert.equal(await write, true);
  assert.equal(instanceStore.familySwitching, false);
  assert.equal(pageFamilies.versions, 'mcode');
  assert.equal(await selectPageFamily('__proto__', 'mcode'), false);
});

test('首次读取仍以保存值初始化三页，读取失败后可重试', async () => {
  // 独立模块实例模拟新启动，不依赖前一轮的播种标记。
  const fresh = await import('../src/kernel/instance.js?fresh-start');
  invoke = () => Promise.reject(new Error('读取失败'));
  await fresh.loadKernelFamily();
  assert.match(fresh.instanceStore.familyError, /读取失败/);
  invoke = () => Promise.resolve('mcode');
  await fresh.loadKernelFamily();
  assert.equal(fresh.instanceStore.familyError, '');
  assert.deepEqual({ ...fresh.pageFamilies }, { overview: 'mcode', versions: 'mcode', memory: 'mcode' });
});
