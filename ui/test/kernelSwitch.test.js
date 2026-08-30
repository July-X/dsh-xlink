import assert from 'node:assert/strict';
import test from 'node:test';

let activateCalls = 0;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command) {
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
