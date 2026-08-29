import assert from 'node:assert/strict';
import test from 'node:test';

let releaseResolve;
let installCalls = 0;
const releaseRequest = new Promise((resolve) => {
  releaseResolve = resolve;
});
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
        if (command === 'fetch_releases') {
          return releaseRequest.then(() => ({ releases: [{ version: '0.1.2', prerelease: false }] }));
        }
        if (command === 'install_kernel') {
          installCalls += 1;
          return Promise.resolve();
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

test('latest release install keeps one exclusive lease across fetch and install', async () => {
  const [{ installLatestRelease }, { withExclusive }] = await Promise.all([
    import('../src/store.js'),
    import('../src/loading.js'),
  ]);

  const run = installLatestRelease();
  await Promise.resolve();
  assert.equal(withExclusive(() => Promise.resolve()), undefined);

  releaseResolve();
  assert.equal(await run, true);
  assert.equal(installCalls, 1);
});
