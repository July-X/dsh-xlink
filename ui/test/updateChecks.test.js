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

test('提示与确认框显式抬到进度浮层之上', async () => {
  // P2-11：Element Plus 的默认 z-index 基线是 2000 + 自增计数，恒低于进度浮层的
  // 3000。长任务进行中弹出的确认框（托盘「退出」的二次确认、补丁的「清除记录」
  // 确认）会被浮层盖住且点不到，而任务未失败时浮层没有关闭按钮——用户看到的是
  // "点了没反应"。这里钉住两侧的关系：notify 显式给 zIndex，且高于浮层。
  const fs = await import('node:fs');
  const notify = fs.readFileSync('ui/src/notify.js', 'utf8');
  assert.match(notify, /zIndex: NOTIFY_Z_INDEX/, 'ElMessage 必须显式指定 zIndex');
  assert.match(notify, /NOTIFY_Z_INDEX = PROGRESS_OVERLAY_Z_INDEX \+ 1000/);

  const css = fs.readFileSync('ui/src/theme.css', 'utf8');
  const overlayMatch = css.match(/\.progress-overlay\s*\{[^}]*z-index:\s*(\d+)/s);
  assert.ok(overlayMatch, '必须能从 theme.css 读到 .progress-overlay 的 z-index');
  const notifyMatch = notify.match(/PROGRESS_OVERLAY_Z_INDEX = (\d+)/);
  assert.ok(notifyMatch, 'notify.js 必须声明浮层 z-index 常量');
  assert.ok(
    Number(notifyMatch[1]) + 1000 > Number(overlayMatch[1]),
    '提示层级必须高于浮层'
  );
});
