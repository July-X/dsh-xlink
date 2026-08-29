import assert from 'node:assert/strict';
import test from 'node:test';

let pluginChecks = 0;
const status = {
  kernel: { running: false, active: '0.1.1', active_installed: true, installed: ['0.1.1'] },
  node: { ok: true, path: '/node', version: '22.19.0' },
  settings: { port: 3090, profile: 'web' },
  shell_version: '0.1.1-rc.10',
  dev_build: false,
  quarantined: [],
  last_incident: null,
  official_chat_open: false,
};

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command) {
        if (command === 'plugin_check_updates') {
          pluginChecks += 1;
          if (pluginChecks === 1) return Promise.reject(new Error('temporary failure'));
          return Promise.resolve([]);
        }
        if (command === 'get_status') return Promise.resolve(status);
        if (command === 'plugin_status') return Promise.resolve({ rows: [] });
        if (command === 'skill_status') return Promise.resolve({ rows: [] });
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

test('failed plugin update checks do not consume the success TTL', async () => {
  const { checkPluginUpdates } = await import('../src/plugins.js');

  assert.equal(await checkPluginUpdates({ busy: true }), null);
  assert.equal(await checkPluginUpdates({ busy: true }), undefined);
  assert.equal(pluginChecks, 2);
  assert.equal(await checkPluginUpdates({ busy: true }), null);
  assert.equal(pluginChecks, 2);
});
