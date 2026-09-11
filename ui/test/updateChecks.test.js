import assert from 'node:assert/strict';
import test from 'node:test';

let pluginChecks = 0;
let skillChecks = 0;
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
        if (command === 'skill_check_updates') {
          skillChecks += 1;
          // 逐包失败：至少一个包带 error（P2-12 的失败退避与静默路径都由它触发）。
          return Promise.resolve([{ id: 'ghost', error: 'network unreachable' }]);
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

test('failed skill update checks back off, and manual checks bypass the backoff', async () => {
  // P2-12：逐包失败不推进成功 TTL（后端语义如此），但自动路径必须有一个退避窗口，
  // 否则切页 / 回焦 / 长任务结束都会重跑全量探测（git 来源会真的起子进程）。
  const { checkSkillUpdates } = await import('../src/skills.js');

  const before = skillChecks;
  const first = await checkSkillUpdates({ busy: false });
  assert.equal(skillChecks, before + 1, '第一次自动检查应当真的跑');
  assert.ok(Array.isArray(first) && first.length === 1, '结果原样返回');

  // 自动路径的第二次调用命中失败退避：不再探测。
  const second = await checkSkillUpdates({ busy: false });
  assert.equal(second, null);
  assert.equal(skillChecks, before + 1, '退避窗口内不得重跑');

  // 手动点击（busy=true）绕过退避。
  const manual = await checkSkillUpdates({ busy: true });
  assert.ok(Array.isArray(manual), '手动检查必须真的跑');
  assert.equal(skillChecks, before + 2, '手动检查不受退避限制');
});
