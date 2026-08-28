import assert from 'node:assert/strict';
import test from 'node:test';

const deferred = () => {
  let resolve;
  const promise = new Promise((res) => {
    resolve = res;
  });
  return { promise, resolve };
};

const status = (quarantined) => ({
  kernel: { running: false, active: '0.1.1', active_installed: true, installed: ['0.1.1'] },
  node: { ok: true, path: '/node', version: '22.19.0' },
  settings: { port: 3090, profile: 'web' },
  shell_version: '0.1.1-rc.10',
  dev_build: false,
  quarantined,
  last_incident: {
    recovered: true,
    message: '安全模式',
    suspects: [],
  },
  official_chat_open: false,
});

const initialStatus = status([]);
const staleStatus = status([{ id: 'dsh-flowglass', name: 'dsh-flowglass' }]);
const freshStatus = status([]);
const stalePoll = deferred();
const freshRefresh = deferred();
let statusCalls = 0;

const core = {
  invoke(command) {
    if (command === 'get_status') {
      statusCalls += 1;
      if (statusCalls === 1) return Promise.resolve(initialStatus);
      if (statusCalls === 2) return stalePoll.promise;
      if (statusCalls === 3) return freshRefresh.promise;
      throw new Error('unexpected get_status call');
    }
    if (command === 'plugin_status') return Promise.resolve({ rows: [] });
    if (command === 'skill_status') return Promise.resolve({ rows: [] });
    throw new Error(`unexpected command: ${command}`);
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  querySelector: () => null,
  addEventListener() {},
  removeEventListener() {},
  body: { classList: { toggle() {} } },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

test('does not let an older poll overwrite a completed quarantine recovery', async () => {
  const { store, refreshAll, pollStatus } = await import('../src/store.js');

  await refreshAll();
  const poll = pollStatus();
  const refresh = refreshAll();

  freshRefresh.resolve(freshStatus);
  await refresh;
  assert.equal(store.view.quarantined.length, 0);

  stalePoll.resolve(staleStatus);
  await poll;

  assert.equal(store.view.quarantined.length, 0);
});
