// E2E launcher 的纯函数判据（OPT-08）：不跑真实构建，只钉隔离规则与产物解析。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import {
  buildRuntimeEnv,
  cleanup,
  debugBinaryName,
  inside,
  isolationProblems,
  makeRunId,
  markerFor,
  mergeTauriConfig,
  pickFreePort,
} from '../e2e/launcher.mjs';

test('Windows 用 .exe 产物名，macOS 不带后缀', () => {
  assert.equal(debugBinaryName('win32'), 'dsh-xlink.exe');
  assert.equal(debugBinaryName('darwin'), 'dsh-xlink');
  assert.equal(debugBinaryName('linux'), 'dsh-xlink');
});

test('runId 只含路径安全字符且两轮不同', () => {
  const first = makeRunId(1700000000000, 'aaa111');
  const second = makeRunId(1700000000000, 'bbb222');
  assert.match(first, /^e2e-[0-9]{14}-[0-9]+-[0-9a-f]{6}$/);
  assert.notEqual(first, second);
});

test('TAURI_CONFIG 合并只覆盖 appDirectoriesOverride，不丢别的键', () => {
  const merged = JSON.parse(
    mergeTauriConfig('{"build":{"devUrl":"http://localhost:5190"},"app":{"withGlobalTauri":true}}', '/tmp/run/appdata')
  );
  assert.equal(merged.app.appDirectoriesOverride, '/tmp/run/appdata');
  assert.equal(merged.app.withGlobalTauri, true);
  assert.equal(merged.build.devUrl, 'http://localhost:5190');
  assert.throws(() => mergeTauriConfig('not json', '/tmp/run'), /不是合法 JSON/);
});

test('隔离自检拒绝测试根外的路径与缺失的归属标记', () => {
  const root = resolve(tmpdir(), 'dsh-e2e-fake-root');
  const marker = markerFor('e2e-run', 42, '/repo');
  const good = {
    homeDir: join(root, 'home'),
    appDataDir: join(root, 'appdata'),
    evidenceDir: join(root, 'evidence'),
    decoyHome: join(root, 'decoy-home'),
    targetDir: join(root, 'target'),
    binary: join(root, 'target', 'debug', 'dsh-xlink'),
  };
  assert.deepEqual(isolationProblems(root, marker, 'e2e-run', good), []);
  assert.deepEqual(isolationProblems(root, marker, 'other-run', good), [
    '缺少本轮归属标记或 runId 不匹配',
  ]);
  const escaped = isolationProblems(root, marker, 'e2e-run', {
    ...good,
    homeDir: join(root, '..', 'outside-home'),
  });
  assert.ok(escaped.some((problem) => problem.includes('不在本轮测试根内')));
  assert.equal(inside(root, join(root, 'home', 'x')), true);
  assert.equal(inside(root, root), false);
});

test('隔离端口从高位空端口分配，避开常用内核端口', async () => {
  const port = await pickFreePort();
  assert.ok(Number.isInteger(port));
  assert.ok(port >= 35000 && port < 35020);
  assert.notEqual(port, 3090);
  assert.notEqual(port, 3091);
});

test('运行时环境把四项隔离与 decoy 家目录一起交给子进程', () => {
  const paths = {
    runRoot: '/tmp/dsh-run',
    runId: 'e2e-run',
    homeDir: '/tmp/dsh-run/home',
    appDataDir: '/tmp/dsh-run/appdata',
    evidenceDir: '/tmp/dsh-run/evidence',
    decoyHome: '/tmp/dsh-run/decoy-home',
    dshHome: '/tmp/dsh-run/decoy-home/.dsh',
    binary: '/tmp/dsh-run/target/debug/dsh-xlink',
    port: 35123,
  };
  const env = buildRuntimeEnv({ KEEP_ME: '1' }, paths, 'sha256-abc');
  assert.equal(env.KEEP_ME, '1');
  assert.equal(env.DSH_XLINK_HOME, paths.homeDir);
  assert.equal(env.DSH_HOME, paths.dshHome);
  assert.equal(env.DSH_XLINK_CHANNEL_SUFFIX, paths.runId);
  assert.equal(env.DSH_E2E_APP_BINARY, paths.binary);
  assert.equal(env.DSH_E2E_BINARY_SHA256, 'sha256-abc');
  assert.equal(env.DSH_E2E_EXPECTED_PORT, '35123');
  assert.equal(env.HOME, paths.decoyHome);
  assert.equal(env.USERPROFILE, paths.decoyHome);
});

test('清理只认本轮归属标记', () => {
  const root = mkdtempSync(join(tmpdir(), 'dsh-e2e-cleanup-'));
  writeFileSync(join(root, '.dsh-e2e-run.json'), JSON.stringify(markerFor('run-a', 1, '/repo')));
  assert.throws(() => cleanup(root, markerFor('run-b', 1, '/repo'), () => {}), /归属不符/);
  cleanup(root, markerFor('run-a', 1, '/repo'), () => {});
  assert.throws(() => readFileSync(join(root, '.dsh-e2e-run.json')), /ENOENT/);
  rmSync(root, { recursive: true, force: true });
});
