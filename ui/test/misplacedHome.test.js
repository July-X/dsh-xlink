import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// 与其他 UI 测试同款：只测共享 JS 模块 + 面板源码，不渲染 Vue 组件。
// migration.js 用了 `reactive`，document mock 必须能撑起 vue runtime-dom
// 在模块加载时的那一次 createElement('template')。
const core = {
  invoke() {
    return Promise.reject(new Error('not used in these tests'));
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  body: { classList: { toggle() {} } },
  addEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
globalThis.requestAnimationFrame = (cb) => {
  cb();
  return 1;
};

const panel = readFileSync('ui/src/components/MigrationPanel.vue', 'utf8');

const { misplacedFileCount, misplacedTotalBytes, misplacedStore } = await import(
  '../src/migration.js'
);

// 后端 MisplacedScan 沿用 migration.rs 同一条链路的 snake_case（`file_count` /
// `total_bytes`）。这条契约错一个字母就是 NaN，而面板上会安静地显示「0 个文件
// · 0 B」——用户因此以为没有东西可找回，正好是这个卡片要解决的那类静默失败。
test('回收统计读 snake_case 字段，读错时归零而不是 NaN', () => {
  const scan = {
    instance: 'default',
    home: '/x/kernels/dsh/instances/default/home',
    dirs: [
      { name: 'sessions', holder: 'default-dev', file_count: 7, total_bytes: 2048, entries: ['a'] },
      { name: 'attachments', holder: 'default-dev', file_count: 3, total_bytes: 512, entries: [] },
    ],
  };
  assert.equal(misplacedFileCount(scan), 10);
  assert.equal(misplacedTotalBytes(scan), 2560);

  // 字段名写成 camelCase（有人给结构体加了 rename_all 又忘了前端）时必须安静
  // 归零，不能把 NaN 渲染到界面上。
  const camel = { dirs: [{ fileCount: 7, totalBytes: 2048 }] };
  assert.equal(misplacedFileCount(camel), 0);
  assert.equal(misplacedTotalBytes(camel), 0);

  // 扫描失败 / 还没查过时不能炸：卡片靠 dirs/workspaces 判断显不显示。
  assert.equal(misplacedFileCount(null), 0);
  assert.equal(misplacedFileCount({}), 0);
  assert.equal(misplacedStore.scan, null, '初始状态应是未扫描');
});

test('「找回历史会话」卡片挂在扫描结果上，按钮按行挂 loading', () => {
  assert.match(panel, /v-if="misplacedVisible"/);
  assert.match(panel, /v-for="dir in misplacedDirs"/);
  assert.match(panel, /v-for="ws in misplacedWorkspaces"/);
  assert.match(panel, /:loading="isLoading\('homeRecovery'\)"/);
  assert.match(panel, /@click="onRecoverMisplaced"/);
  // 源不删除这句话必须留在界面上：用户凭什么敢点，取决于这条。
  assert.match(panel, /源目录不会被删除/);
  // 只搬目录不登记清单 = 会话仍然不显示（2026-09-29 本机实测）。确认框因此
  // 必须写明「要登记会话清单」与「要先关闭工作台」（内核内存缓存会覆盖清单）。
  assert.match(panel, /并把 \$\{wsSessions\} 个会话登记进本实例的会话清单/);
  assert.match(panel, /需要先关闭工作台/);
  // 确认弹窗必须写清「会失去什么」——这里承诺的是「什么都不失去」。
  assert.match(panel, /confirmDialog\(/);
});
