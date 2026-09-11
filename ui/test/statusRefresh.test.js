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

const frontendIncident = {
  recovered: false,
  safe_mode: false,
  cause: 'frontend',
  message: '工作台页面抛出了一个未处理的前端异常…工作台本身仍在运行。',
  suspects: [],
  attempts: [],
  log_tail: '',
  log_path: '/tmp/kernel.log',
  hint: '请打开日志并把自检证据里的消息一并反馈。',
  at: 1_789_133_158,
  health: {
    kind: 'unhandled-rejection',
    message: 'TypeError: Assistant stream raw chunk must be a lossless JSON object',
    stack: 'validateRecord@http://127.0.0.1:4090/plugins/:148814:26',
    page_url: 'http://127.0.0.1:4090/',
  },
};

test('frontend bundle incidents are recorded without opening the modal', async () => {
  const { store, showIncident, isNonFatalFrontendIncident } = await import('../src/store.js');

  store.incidentVisible = false;
  store.incident = null;
  store.lastIncident = null;

  assert.equal(isNonFatalFrontendIncident(frontendIncident), true);
  showIncident(frontendIncident);
  assert.equal(store.incidentVisible, false, '页面仍在运行的前端异常不得弹模态框');
  assert.equal(store.incident, null);
  // store 是 reactive：读回来的是代理，按字段比较。
  assert.equal(store.lastIncident.cause, 'frontend');
  assert.equal(store.lastIncident.message, frontendIncident.message, '但必须记进概览横幅');

  // 概览横幅的「查看详情」显式要求时仍然打开面板，证据不会丢失。
  showIncident(store.lastIncident, { force: true });
  assert.equal(store.incidentVisible, true);
  assert.equal(store.incident.health.kind, 'unhandled-rejection');
});

test('incidents with an actionable suspect or a blank page still interrupt', async () => {
  const { store, showIncident } = await import('../src/store.js');

  store.incidentVisible = false;
  showIncident({
    ...frontendIncident,
    cause: 'plugin',
    message: '工作台页面异常，错误证据指向插件「x」',
    suspects: [{ kind: 'plugin', id: 'x', name: 'x', evidence: 'boom' }],
  });
  assert.equal(store.incidentVisible, true, '有可处置对象的事故照旧弹面板');

  store.incidentVisible = false;
  showIncident({
    ...frontendIncident,
    health: { ...frontendIncident.health, kind: 'blank' },
  });
  assert.equal(store.incidentVisible, true, '白屏报告不受降级影响');
});
