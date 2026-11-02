// E2E launcher 的纯函数判据（OPT-08 + 2026-11 FIX-06/07）：不跑真实构建，
// 只钉隔离规则、清理顺序与日志生命周期。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { _setGlobal } from '@wdio/globals';
import { mochaHooks } from '../e2e/isolation-probe.mjs';
import {
  backendIsolationProblems,
  buildRuntimeEnv,
  cleanup,
  completeRun,
  createFileLogger,
  dataDirKernelsPath,
  debugBinaryName,
  diskUsageReportPaths,
  inside,
  isolationProblems,
  makeRunId,
  markerFor,
  mergeTauriConfig,
  pickFreePort,
  writeEvidenceResult,
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
    tempDir: join(root, 'temp'),
    roamingDir: join(root, 'appdata-roaming'),
    localDir: join(root, 'appdata-local'),
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

function fakePaths() {
  return {
    runRoot: '/tmp/dsh-run',
    runId: 'e2e-run',
    homeDir: '/tmp/dsh-run/home',
    appDataDir: '/tmp/dsh-run/appdata',
    evidenceDir: '/tmp/dsh-run/evidence',
    decoyHome: '/tmp/dsh-run/decoy-home',
    dshHome: '/tmp/dsh-run/decoy-home/.dsh',
    tempDir: '/tmp/dsh-run/temp',
    roamingDir: '/tmp/dsh-run/appdata-roaming',
    localDir: '/tmp/dsh-run/appdata-local',
    binary: '/tmp/dsh-run/target/debug/dsh-xlink',
    port: 35123,
  };
}

test('运行时环境只继承白名单：目录覆盖变量一律丢弃或重定向到 runRoot', () => {
  const paths = fakePaths();
  const env = buildRuntimeEnv(
    {
      PATH: '/usr/local/bin',
      LANG: 'zh_CN.UTF-8',
      // 用户终端里可能有这些覆盖量；任何一个漏进去都可能让壳读写真数据。
      DSH_DESKTOP_DATA_DIR: '/outside/real-data',
      DSH_NPM_REGISTRY: 'https://private.example/',
      DSH_NPM_TARBALL_HOSTS: 'cdn.example',
      CODEX_HOME: '/outside/codex',
      NODE_OPTIONS: '--require /outside/hook.cjs',
      HTTP_PROXY: 'http://proxy:8080',
      TAURI_CONFIG: '{"app":{}}',
      TMPDIR: '/var/folders/user/T/',
      KEEP_ME: '1',
    },
    paths,
    'sha256-abc'
  );

  // OS 级变量允许继承（进程创建 / 编码）。
  assert.equal(env.PATH, '/usr/local/bin');
  assert.equal(env.LANG, 'zh_CN.UTF-8');
  // 目录 / 配置 / 代理覆盖量：一个都不许继承。
  for (const name of [
    'DSH_DESKTOP_DATA_DIR',
    'DSH_NPM_REGISTRY',
    'DSH_NPM_TARBALL_HOSTS',
    'CODEX_HOME',
    'NODE_OPTIONS',
    'HTTP_PROXY',
    'TAURI_CONFIG',
    'KEEP_ME',
  ]) {
    assert.equal(env[name], undefined, name + ' 不得从用户环境继承');
  }
  // 隔离项照旧写入。
  assert.equal(env.DSH_XLINK_HOME, paths.homeDir);
  assert.equal(env.DSH_HOME, paths.dshHome);
  assert.equal(env.DSH_XLINK_CHANNEL_SUFFIX, paths.runId);
  assert.equal(env.DSH_E2E_APP_BINARY, paths.binary);
  assert.equal(env.DSH_E2E_BINARY_SHA256, 'sha256-abc');
  assert.equal(env.DSH_E2E_EXPECTED_PORT, '35123');
  // 目录类变量重定向到 runRoot，而不是丢弃（子进程仍要有可写 home / temp）。
  assert.equal(env.HOME, paths.decoyHome);
  assert.equal(env.USERPROFILE, paths.decoyHome);
  assert.equal(env.TEMP, paths.tempDir);
  assert.equal(env.TMP, paths.tempDir);
  // macOS 的 TMPDIR 保留系统值（WKWebView 依赖它的格式），Windows 重定向到 runRoot。
  assert.equal(env.TMPDIR, process.platform === 'darwin' ? '/var/folders/user/T/' : paths.tempDir);
  assert.equal(env.APPDATA, paths.roamingDir);
  assert.equal(env.LOCALAPPDATA, paths.localDir);
});

test('Windows 环境变量名大小写不敏感：Path 也按白名单继承', () => {
  const env = buildRuntimeEnv({ Path: 'C:\\Windows\\System32' }, fakePaths(), 'sha');
  if (process.platform === 'win32') assert.equal(env.Path, 'C:\\Windows\\System32');
  else assert.equal(env.Path, undefined, '非 Windows 不允许未在白名单里的大小写变体');
});

test('从后端读到的数据目录 canonicalize 后必须落在 runRoot 内', () => {
  const root = mkdtempSync(join(tmpdir(), 'dsh-e2e-iso-'));
  const outside = mkdtempSync(join(tmpdir(), 'dsh-e2e-outside-'));
  const home = join(root, 'home');
  mkdirSync(home, { recursive: true });
  const kernelsPath = join(home, 'dsh', 'desktop-dev', 'kernels');
  try {
    // 正常：data_dir 可能还没落盘（隔离 home 从没装过内核），仍必须判为在根内。
    assert.deepEqual(
      backendIsolationProblems(root, { dataDir: join(home, 'dsh', 'desktop-dev') }),
      []
    );
    // 逃逸：预设的 DSH_DESKTOP_DATA_DIR 真生效了会是一个外部路径，这里抓住它。
    const escaped = backendIsolationProblems(root, { dataDir: join(outside, 'desktop-dev') });
    assert.ok(
      escaped.some((problem) => problem.includes('逃出本轮测试根')),
      JSON.stringify(escaped)
    );
    // 不存在的路径也按 realpath -m 语义解析：外部前缀仍然拒绝。
    assert.ok(backendIsolationProblems(root, { dataDir: join(outside, 'missing') }).length > 0);
    // 非绝对路径同样拒绝。
    assert.ok(backendIsolationProblems(root, { dataDir: 'relative/path' }).length > 0);

    // disk_usage 报表路径提取：unreadable（不存在的目录）与存在目录的 path 都收。
    const instancePath = join(home, 'kernels', 'dsh', 'instances', 'default');
    const report = {
      unreadable: [kernelsPath, join(home, 'kernels')],
      groups: [{ entries: [{ path: instancePath }, { path: 'relative-ignored' }] }],
    };
    const reportPaths = diskUsageReportPaths(report);
    assert.deepEqual(reportPaths, [kernelsPath, join(home, 'kernels'), instancePath]);
    // 数据目录证据 = <home>/<family>/desktop[-dev]/kernels；<home>/kernels 不是它。
    assert.equal(dataDirKernelsPath(home, reportPaths), kernelsPath);
    assert.equal(dataDirKernelsPath(home, [join(home, 'kernels')]), null);
    // 逃逸后的 kernels 不在 <home> 下：认不出数据目录，但整条路径会被判逃逸。
    const escapedKernels = join(outside, 'dsh', 'desktop-dev', 'kernels');
    assert.equal(dataDirKernelsPath(home, [escapedKernels]), null);
    assert.ok(
      backendIsolationProblems(root, { 'diskUsage.paths[0]': escapedKernels }).length > 0
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

function tempRunRoot() {
  const root = mkdtempSync(join(tmpdir(), 'dsh-e2e-finish-'));
  const marker = markerFor('run-a', 1, '/repo');
  writeFileSync(join(root, '.dsh-e2e-run.json'), JSON.stringify(marker));
  const evidence = join(root, 'evidence');
  mkdirSync(evidence, { recursive: true });
  return { root, marker, evidence };
}

test('无 --keep 成功路径：真实文件 logger 先关闭再删 runRoot，退出码 0', () => {
  const { root, marker, evidence } = tempRunRoot();
  const logger = createFileLogger(join(evidence, 'launcher.log'));
  logger.log('清理前的一行证据');
  try {
    const code = completeRun({ runRoot: root, marker, keep: false, logger });
    assert.equal(code, 0, '无 --keep 成功路径必须退出码 0');
    assert.equal(logger.closed, true, '删 runRoot 之前必须先关闭日志');
    assert.equal(existsSync(root), false, '无 --keep 必须清理本轮目录');
    // 删除之后再调用 logger 不得再碰已删除的路径（旧实现这里 ENOENT）。
    assert.doesNotThrow(() => logger.log('删除后再写'));
    assert.equal(existsSync(join(evidence, 'launcher.log')), false, '不得在已删目录上重建日志');
  } finally {
    if (existsSync(root)) rmSync(root, { recursive: true, force: true });
  }
});

test('--keep 成功路径：目录与日志证据按策略保留', () => {
  const { root, marker, evidence } = tempRunRoot();
  const logger = createFileLogger(join(evidence, 'launcher.log'));
  try {
    const code = completeRun({ runRoot: root, marker, keep: true, logger });
    assert.equal(code, 0);
    assert.equal(logger.closed, true, '--keep 也要关闭日志，证据才完整');
    assert.equal(existsSync(root), true, '--keep 不得删除本轮目录');
    const log = readFileSync(join(evidence, 'launcher.log'), 'utf8');
    assert.match(log, /E2E 通过/);
    assert.match(log, /按要求保留本轮目录/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('失败路径：证据目录与日志完整保留，关闭日志后仍可读', () => {
  const { root, evidence } = tempRunRoot();
  const logger = createFileLogger(join(evidence, 'launcher.log'));
  try {
    logger.log('E2E 启动失败：合成失败');
    writeEvidenceResult(evidence, { error: '合成失败' });
    logger.close();
    assert.equal(existsSync(root), true, '失败路径不得清理目录');
    assert.match(readFileSync(join(evidence, 'launcher.log'), 'utf8'), /E2E 启动失败/);
    assert.equal(JSON.parse(readFileSync(join(evidence, 'result.json'), 'utf8')).error, '合成失败');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('异常路径不依赖已删证据目录：清理后写结果只落 stderr', () => {
  const { root, marker, evidence } = tempRunRoot();
  cleanup(root, marker);
  assert.equal(existsSync(root), false);
  // 旧实现会在成功清理后的异常处理里再向已删目录写日志/结果，把真实错误盖成 ENOENT。
  assert.doesNotThrow(() => writeEvidenceResult(evidence, { error: 'cleanup 之后才失败的错' }));
});

test('清理只认本轮归属标记，外部目录不在删除目标里', () => {
  const root = mkdtempSync(join(tmpdir(), 'dsh-e2e-cleanup-'));
  const outside = mkdtempSync(join(tmpdir(), 'dsh-e2e-cleanup-outside-'));
  writeFileSync(join(root, '.dsh-e2e-run.json'), JSON.stringify(markerFor('run-a', 1, '/repo')));
  writeFileSync(join(outside, 'keep.txt'), 'user data\n');
  try {
    assert.throws(() => cleanup(root, markerFor('run-b', 1, '/repo')), /归属不符/);
    cleanup(root, markerFor('run-a', 1, '/repo'));
    assert.equal(existsSync(root), false);
    // 清理只删 runRoot；预设到外部的目录（如 DSH_DESKTOP_DATA_DIR）一个字节都不动。
    assert.equal(readFileSync(join(outside, 'keep.txt'), 'utf8'), 'user data\n');
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

// --- 隔离探针（e2e/isolation-probe.mjs）------------------------------------
//
// 探针本身是 wdio 的 Mocha 根 hook，真跑要构建整个壳；但它对后端路径的判断
// 全是纯逻辑。把 `@wdio/globals` 的 browser 换成一个只认识 get_status /
// disk_usage 的假实现，就能在没有桌面壳的机器上把「后端路径 → canonicalize
// → 必须落在 runRoot」整段判据跑完（FIX-06 的关键分支）。
function fakeBrowser({ xlinkHomeDisplay, report }) {
  return {
    async execute(script) {
      if (typeof script === 'function') return true; // 桥已注入
      return null;
    },
    async executeAsync(script) {
      if (script.includes('"get_status"')) {
        return {
          ok: true,
          value: { kernel: { data_dir: 'display-only', xlink_home: xlinkHomeDisplay } },
        };
      }
      if (script.includes('"disk_usage"')) return { ok: true, value: { report } };
      return { ok: false, error: '假 browser 不认识的命令' };
    },
    async pause() {},
  };
}

async function withProbeFixture(build, body) {
  const root = mkdtempSync(join(tmpdir(), 'dsh-e2e-probe-'));
  const outside = mkdtempSync(join(tmpdir(), 'dsh-e2e-probe-outside-'));
  const home = join(root, 'home');
  const evidenceDir = join(root, 'evidence');
  mkdirSync(home, { recursive: true });
  mkdirSync(evidenceDir, { recursive: true });
  const env = {
    DSH_E2E_RUN_ROOT: root,
    DSH_E2E_EVIDENCE_DIR: evidenceDir,
    DSH_E2E_EXPECTED_HOME: home,
  };
  const previous = new Map(Object.keys(env).map((name) => [name, process.env[name]]));
  for (const [name, value] of Object.entries(env)) process.env[name] = value;
  _setGlobal('browser', fakeBrowser(build({ root, home, outside })), false);
  try {
    return await body({
      root,
      home,
      outside,
      readEvidence: () => JSON.parse(readFileSync(join(evidenceDir, 'backend-isolation.json'), 'utf8')),
    });
  } finally {
    for (const [name, value] of previous) {
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
    rmSync(root, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
}

test('隔离探针：后端数据目录落在 runRoot 内时通过并留下证据', async () => {
  await withProbeFixture(
    ({ home }) => ({
      xlinkHomeDisplay: home,
      // 隔离 home 从没装过内核：data_dir/kernels 必然不存在，disk_usage 会列进 unreadable。
      report: { unreadable: [join(home, 'dsh', 'desktop-dev', 'kernels')], groups: [] },
    }),
    async ({ root, readEvidence }) => {
      await mochaHooks.beforeAll();
      const evidence = readEvidence();
      assert.deepEqual(evidence.problems, []);
      assert.equal(inside(root, evidence.actual.dataDir), true, JSON.stringify(evidence.actual));
      assert.equal(evidence.getStatus.dataDirDisplay, 'display-only');
    }
  );
});

test('隔离探针：后端 data_dir 逃出 runRoot 时在用例前失败并写证据', async () => {
  await withProbeFixture(
    ({ home, outside }) => ({
      xlinkHomeDisplay: home,
      report: { unreadable: [join(outside, 'dsh', 'desktop-dev', 'kernels')], groups: [] },
    }),
    async ({ readEvidence }) => {
      await assert.rejects(mochaHooks.beforeAll(), /核对失败/);
      const evidence = readEvidence();
      assert.ok(
        evidence.problems.some((problem) => problem.includes('逃出本轮测试根')),
        JSON.stringify(evidence.problems)
      );
    }
  );
});

test('隔离探针：后端 xlink_home 与注入值不一致时失败', async () => {
  await withProbeFixture(
    ({ home, outside }) => ({
      // 后端报告的是另一个 home：即使它也在 runRoot 外仍要失败。
      xlinkHomeDisplay: outside,
      report: { unreadable: [join(home, 'dsh', 'desktop-dev', 'kernels')], groups: [] },
    }),
    async ({ readEvidence }) => {
      await assert.rejects(mochaHooks.beforeAll(), /核对失败/);
      assert.ok(
        readEvidence().problems.some((problem) => problem.includes('不一致')),
        '必须点名 xlink_home 与 launcher 注入值不一致'
      );
    }
  );
});

test('隔离探针：没观察到数据目录证据时失败，不装作通过', async () => {
  await withProbeFixture(
    ({ home }) => ({ xlinkHomeDisplay: home, report: { unreadable: [], groups: [] } }),
    async () => {
      await assert.rejects(mochaHooks.beforeAll(), /没有报出数据目录/);
    }
  );
});
