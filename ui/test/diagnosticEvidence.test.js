// 诊断证据与运行类型的前端契约。
//
// 后端可能只留下 sandboxLog：例如沙盒基线进程静默退出时，内核常驻日志并不
// 一定存在。证据已经落盘却在页面上被隐藏，会把「能定位」退化成「只能猜」。
// 这里同时测模板契约和实际加载 / 路由行为，避免只扫字符串而漏掉状态层的漂移。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const runs = new Map([
  ['startup-run', {
    id: 'startup-run',
    kind: 'startup',
    status: 'failure',
    evidence: { sandboxLog: '/tmp/logs/startup-sandbox.log' },
    events: [],
  }],
  ['precheck-run', {
    id: 'precheck-run',
    kind: 'plugin-precheck',
    status: 'inconclusive',
    evidence: { sandboxLog: '/tmp/logs/precheck-sandbox.log' },
    events: [],
  }],
  ['restore-run', {
    id: 'restore-run',
    kind: 'restore',
    status: 'failure',
    evidence: { kernelLog: '/tmp/logs/restore.log' },
    events: [],
  }],
  ['bisect-run', {
    id: 'bisect-run',
    kind: 'bisect',
    status: 'inconclusive',
    evidence: { sandboxLog: '/tmp/logs/bisect-sandbox.log' },
    events: [],
  }],
]);

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command, args = {}) {
        if (command === 'diagnostic_run_get') return Promise.resolve(runs.get(args.runId) || null);
        if (command === 'diagnostic_run_latest') return Promise.resolve(null);
        if (command === 'get_status') return Promise.resolve({ kernel: { running: false }, node: { ok: true } });
        return Promise.resolve([]);
      },
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  body: { classList: { contains: () => false, toggle() {} } },
  addEventListener() {},
  removeEventListener() {},
};

const { diagnosticStore, openRunDiagnosis, closeDiagnosis, loadStartupDiagnosis } =
  await import('../src/diagnostics/diagnostics.js');

function readSource(name) {
  return readFileSync(new URL(`../src/diagnostics/${name}`, import.meta.url), 'utf8');
}

test('启动与操作诊断的证据卡对任一日志路径可见', () => {
  for (const name of ['StartupDiagnosis.vue', 'OperationDiagnosis.vue']) {
    const source = readSource(name);
    assert.match(
      source,
      /<div v-if="evidence\.kernelLog \|\| evidence\.sandboxLog" class="diag-card">/,
      `${name} 不能只用 kernelLog 判断是否显示证据卡`,
    );
    assert.match(source, /v-if="evidence\.kernelLog"/, `${name} 要显示内核日志路径`);
    assert.match(source, /v-if="evidence\.sandboxLog"/, `${name} 要显示沙盒日志路径`);
  }
});

test('启动记录只有 sandboxLog 时仍保留可打开的首选证据路径', async () => {
  diagnosticStore.currentRun = { id: 'old-run', evidence: { kernelLog: '/tmp/logs/old.log' } };
  diagnosticStore.evidencePath = '/tmp/logs/old.log';

  await loadStartupDiagnosis('startup-run');

  assert.equal(diagnosticStore.currentRun.id, 'startup-run');
  assert.equal(
    diagnosticStore.evidencePath,
    '/tmp/logs/startup-sandbox.log',
    '沙盒日志是这次诊断的直接证据，应成为查看日志的首选路径',
  );
});

test('最近操作的四种 kind 实际路由到对应诊断视图', async () => {
  const cases = [
    ['startup-run', 'startup'],
    ['precheck-run', 'plugin'],
    ['restore-run', 'restore'],
    ['bisect-run', 'bisect'],
  ];

  for (const [id, expectedKind] of cases) {
    const active = await openRunDiagnosis(runs.get(id), 'overview');
    assert.equal(active.kind, expectedKind, `${id} 不应被当成别的诊断类型`);
    assert.equal(diagnosticStore.currentRun.id, id, `${id} 应加载自己的运行记录`);
    assert.equal(diagnosticStore.sourcePanel, 'overview');
    closeDiagnosis();
  }
});
