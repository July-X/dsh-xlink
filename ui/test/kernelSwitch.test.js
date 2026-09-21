import assert from 'node:assert/strict';
import test from 'node:test';

let activateCalls = 0;
let instanceInvoke;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command, args) {
        if (instanceInvoke) return instanceInvoke(command, args);
        if (command === 'activate_version') {
          activateCalls += 1;
          return Promise.resolve();
        }
        throw new Error(`unexpected command: ${command}`);
      },
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
  getComputedStyle() {
    return { transitionDuration: '0s', animationDuration: '0s', transitionDelay: '0s', animationDelay: '0s' };
  },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const instances = (selected) => ['a', 'b'].map((id) => ({
  record: { id, kernel_family: 'dsh' }, is_default: id === selected,
}));

test('instance reads deduplicate and a stale list cannot undo a switch', async () => {
  const { instanceStore, loadInstances, setDefaultInstance } = await import('../src/instance.js');
  const { globalBusy } = await import('../src/loading.js');
  instanceStore.list = instances('a');
  instanceStore.defaultInstanceId = 'a';
  const read = deferred(), write = deferred(), refresh = deferred();
  let reads = 0, writes = 0;
  instanceInvoke = (command, args) => {
    if (command === 'list_instances') { reads++; return read.promise; }
    assert.equal(command, 'set_default_instance');
    assert.equal(args.id, 'b');
    writes++;
    return write.promise;
  };
  const first = loadInstances();
  assert.equal(loadInstances(), first);
  await Promise.resolve();
  assert.equal(instanceStore.loaded, true);
  const switching = setDefaultInstance('b', () => refresh.promise);
  assert.equal(globalBusy.value, true);
  assert.equal(await setDefaultInstance('a'), false);
  assert.equal(await setDefaultInstance('b'), false);
  write.resolve();
  await Promise.resolve();
  read.resolve(instances('a'));
  await first;
  assert.equal(instanceStore.defaultInstanceId, 'b');
  assert.equal(instanceStore.loaded, false);
  assert.equal(instanceStore.switching, true);
  refresh.resolve();
  assert.equal(await switching, true);
  assert.equal(instanceStore.switching, false);
  assert.equal(globalBusy.value, false);
  assert.equal(reads, 1);
  assert.equal(writes, 1);
  instanceInvoke = null;
});

test('failed instance reads retain selection and retries recover; failed writes release busy state', async () => {
  const { instanceStore, loadInstances, setDefaultInstance } = await import('../src/instance.js');
  const { globalBusy, withExclusive } = await import('../src/loading.js');
  instanceStore.list = instances('a');
  instanceStore.defaultInstanceId = 'a';
  instanceInvoke = () => Promise.reject(new Error('registry unavailable'));
  await assert.rejects(loadInstances(), /registry unavailable/);
  assert.equal(instanceStore.loaded, false);
  assert.match(instanceStore.error, /registry unavailable/);
  assert.equal(instanceStore.defaultInstanceId, 'a');
  assert.equal(instanceStore.list.length, 2);
  await assert.rejects(setDefaultInstance('b'), /registry unavailable/);
  assert.equal(instanceStore.defaultInstanceId, 'a');
  assert.equal(instanceStore.switching, false);
  assert.equal(globalBusy.value, false);
  await withExclusive(async () => assert.equal(await setDefaultInstance('b'), false));
  instanceInvoke = () => Promise.resolve([]);
  await loadInstances();
  assert.equal(instanceStore.loaded, false);
  assert.equal(instanceStore.error, '');
  assert.equal(instanceStore.defaultInstanceId, '');
  assert.deepEqual(instanceStore.list, []);
  instanceInvoke = null;
});

const makeElement = () => ({
  ownerDocument: globalThis.document,
  style: {},
  classList: { add() {}, remove() {}, contains() { return false; }, toggle() {} },
  addEventListener() {},
  removeEventListener() {},
  setAttribute() {},
  removeAttribute() {},
  appendChild() {},
  removeChild() {},
  insertBefore() {},
});
const body = makeElement();
globalThis.document = {
  createElement: makeElement,
  createElementNS: makeElement,
  createTextNode: makeElement,
  createComment: makeElement,
  body,
  documentElement: makeElement(),
  addEventListener() {},
  removeEventListener() {},
};
globalThis.requestAnimationFrame = (callback) => {
  callback();
  return 1;
};
globalThis.cancelAnimationFrame = () => {};

test('blocks kernel switching while the workbench is running or starting', async () => {
  const { store, activateVersion, workbenchActiveNow } = await import('../src/store.js');
  store.view = { kernel: { running: true } };
  store.starting = false;

  assert.equal(workbenchActiveNow(), true);
  assert.equal(await activateVersion('0.2.0'), false);

  store.view.kernel.running = false;
  store.starting = true;
  assert.equal(workbenchActiveNow(), true);
  assert.equal(await activateVersion('0.2.0'), false);

  assert.equal(activateCalls, 0);
  store.starting = false;
  store.view = null;
});
