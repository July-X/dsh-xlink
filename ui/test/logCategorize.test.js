// 日志分类：「全屏」弹窗与主面板的左栏都要按用途分桶（内核 / 安装 / 插件 /
// 环境 / 维护 / 其它）。判据来自日志文件名 `name` 段，与 build 类型 / 实例 id /
// 日期都无关，避免一次构建多实例就把同一类日志拆到不同桶。
import assert from 'node:assert/strict';
import test from 'node:test';

const core = {
  invoke() {
    return Promise.reject(new Error('not used in this test'));
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

test('parseLogFilename extracts the logical name from every supported format', async () => {
  const { parseLogFilename } = await import('../src/logs.js');

  // 壳级三段格式（kind / name / date）
  assert.equal(parseLogFilename('release-kernel-2026-09-24.log').name, 'kernel');
  assert.equal(parseLogFilename('dev-plugin-wiring-2026-09-24.log').name, 'plugin-wiring');
  assert.equal(parseLogFilename('release-pnpm-install-1735123456-2026-09-24.log').name, 'pnpm-install-1735123456');
  assert.equal(parseLogFilename('release-node-install-2026-09-24.log').name, 'node-install');
  assert.equal(parseLogFilename('release-shell-update-cleanup-2026-09-24.log').name, 'shell-update-cleanup');

  // 实例感知五段格式（kind / family / instance_id / name / date）
  assert.equal(parseLogFilename('release-dsh-default-kernel-2026-09-24.log').name, 'kernel');
  assert.equal(
    parseLogFilename('release-dsh-default-install-0.1.2-rc.6-2026-09-24.log').name,
    'install-0.1.2-rc.6',
    '带 `-` 的逻辑名不能被日期前缀截断',
  );

  // 单插件构建日志：`plugin-<id>` 必须把整个 id 当 name
  assert.equal(parseLogFilename('release-plugin-mcp-browser-2026-09-24.log').name, 'plugin-mcp-browser');

  // 旧命名（迁移前的轮转备份）也能跑通；分类只看 name 段，与扩展名无关
  assert.equal(parseLogFilename('release-kernel-2026-09-24.log.1').name, 'kernel');

  // 没有日期后缀的兜底：整个 stem 当 name，分类退回「其它」
  assert.equal(parseLogFilename('weird-file.log').name, 'weird-file');
});

test('categorizeLogFile routes every file to the correct bucket', async () => {
  const { categorizeLogFile, LOG_CATEGORIES } = await import('../src/logs.js');

  // 所有声明过的桶 ID 都必须能命中，否则 LOG_CATEGORIES 与实现脱节
  const ids = new Set(LOG_CATEGORIES.map((c) => c.id));

  const cases = [
    // 内核
    ['release-kernel-2026-09-24.log', 'kernel'],
    ['release-dsh-default-kernel-2026-09-24.log', 'kernel'],
    // 内核安装
    ['release-install-0.1.2-rc.6-2026-09-24.log', 'install'],
    ['release-dsh-default-install-0.1.2-rc.6-2026-09-24.log', 'install'],
    // 插件接线
    ['release-plugin-wiring-2026-09-24.log', 'plugin-wiring'],
    // 插件构建（必须排在 plugin-wiring 之后判断）
    ['release-plugin-mcp-browser-2026-09-24.log', 'plugin'],
    ['dev-plugin-foo-bar-2026-09-24.log', 'plugin'],
    // pnpm 安装
    ['release-pnpm-install-1735123456-2026-09-24.log', 'pnpm-install'],
    // Node 安装
    ['release-node-install-2026-09-24.log', 'node-install'],
    // 更新清理
    ['release-shell-update-cleanup-2026-09-24.log', 'update-cleanup'],
    // 兜底
    ['release-unknown-thing-2026-09-24.log', 'other'],
  ];

  for (const [filename, expected] of cases) {
    const got = categorizeLogFile(filename);
    assert.equal(
      got,
      expected,
      `${filename} 应分到 ${expected}，实际分到 ${got}`,
    );
    assert.ok(ids.has(got), `${got} 不在 LOG_CATEGORIES 中（新增桶时忘了同步分类函数）`);
  }

  // `plugin-wiring` 必须**不能**被 `plugin-*` 吃掉（之前差点出过这种 bug）
  assert.equal(categorizeLogFile('release-plugin-wiring-2026-09-24.log'), 'plugin-wiring');
});

test('groupLogFiles groups, preserves within-group order, and drops empty buckets', async () => {
  const { groupLogFiles } = await import('../src/logs.js');

  const files = [
    // 已按 list_log_files 的「基名逆序 + 代次升序」排好，组内必须保留该顺序
    { name: 'release-dsh-default-kernel-2026-09-24.log', size: 1024 },
    { name: 'release-dsh-default-kernel-2026-09-23.log', size: 2048 },
    { name: 'release-install-0.1.2-rc.6-2026-09-24.log', size: 512 },
    { name: 'release-plugin-wiring-2026-09-24.log', size: 256 },
    { name: 'release-plugin-mcp-browser-2026-09-24.log', size: 128 },
    { name: 'release-pnpm-install-1735123456-2026-09-24.log', size: 64 },
    { name: 'release-shell-update-cleanup-2026-09-24.log', size: 32 },
  ];

  const groups = groupLogFiles(files);

  // 空桶（这里：node-install）必须被丢掉，否则侧栏渲染出没有文件的「分类」标题
  const ids = groups.map((g) => g.id);
  assert.deepEqual(ids, ['kernel', 'install', 'plugin-wiring', 'plugin', 'pnpm-install', 'update-cleanup']);

  // 每组内部按 list_log_files 原顺序
  assert.deepEqual(
    groups[0].files.map((f) => f.name),
    ['release-dsh-default-kernel-2026-09-24.log', 'release-dsh-default-kernel-2026-09-23.log'],
    '内核组必须保留原排序',
  );

  // 空输入不抛异常，给空数组
  assert.deepEqual(groupLogFiles([]), []);
  assert.deepEqual(groupLogFiles(undefined), []);
});
