#!/usr/bin/env node
// 验证 dsh-session-perf 补丁的清单、语法和缓存行为。
//
// 用法：
//   node scripts/verify-dsh-session-perf.mjs
//   node scripts/verify-dsh-session-perf.mjs <内核根目录>
//   node scripts/verify-dsh-session-perf.mjs --require-applied
//
// 默认只读：不会修改内核目录或 ~/.dsh。行为测试把仓库中的补丁载荷写入临时目录，
// 并通过临时 node_modules 链接加载官方依赖。

import { existsSync, readFileSync } from 'node:fs';
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const DSH_HOME = process.env.DSH_HOME ?? join(homedir(), '.dsh');
const PATCH_ID = 'dsh-session-perf';
const PATCH_VERSION = '1.0.1';
const KERNEL_VERSION = '0.1.1-rc.2';
const TARGET = 'node_modules/@deepseek-ai/dsh-session-persistence-jsonl/lib/index.js';
const MANIFEST = resolve('src-tauri/resources/patches/dsh-session-perf/manifest.json');
const ORIGINAL_SHA256 = '8b6ebc4509a3e969ab3ad6e0dfb553ae4861e5b101831afed23e593d148d97f3';
const LEGACY_PATCHED_SHA256 = 'f20c1453291953cc875a1ec1519327c7a5abbd9d37a4244180ab41e807342355';
const PATCHED_SHA256 = '9ed3fe3cfa3890e8559efd9369efac9866c19c3737c3328b6355c338f0a7f96e';
const CACHE_MARKER = 'const SESSION_ARTIFACT_LIST_CACHE_TTL_MS = 1000;';
const requireApplied = process.argv.includes('--require-applied');
const positional = process.argv.slice(2).find((arg) => !arg.startsWith('--'));

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  ${detail}` : ''}`);
  if (!ok) failures += 1;
};

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function resolveKernelRoot() {
  if (positional) return resolve(positional);
  for (const variant of ['desktop', 'desktop-dev']) {
    const activeFile = join(DSH_HOME, variant, 'active.txt');
    try {
      const version = readFileSync(activeFile, 'utf8').trim();
      const root = join(DSH_HOME, variant, 'kernels', version);
      if (existsSync(root)) return root;
    } catch {
      // Try the next DSH data variant.
    }
  }
  throw new Error(`找不到激活内核：请显式传入内核根目录，或检查 ${DSH_HOME}/desktop/active.txt`);
}

async function loadPatch(kernelRoot) {
  const manifest = JSON.parse(await readFile(MANIFEST, 'utf8'));
  check('manifest schemaVersion=1', manifest.schemaVersion === 1);
  const patch = manifest.patches?.find((candidate) => candidate.id === PATCH_ID);
  check('manifest 包含 dsh-session-perf', patch !== undefined);
  if (patch === undefined) throw new Error('manifest 中缺少 dsh-session-perf');
  check(`补丁版本为 ${PATCH_VERSION}`, patch.version === PATCH_VERSION);
  check(`补丁版本范围精确覆盖 ${KERNEL_VERSION}`, patch.minKernelVersion === KERNEL_VERSION && patch.maxKernelVersion === KERNEL_VERSION);
  check('补丁只修改一个 persistence 目标', patch.files?.length === 1 && patch.files[0]?.mode === 'copy' && patch.files[0]?.to === TARGET);
  const file = patch.files?.[0];
  if (file === undefined || file.mode !== 'copy' || typeof file.from !== 'string') throw new Error('manifest 中缺少 persistence copy 文件');

  const targetPath = join(kernelRoot, TARGET);
  const payloadPath = join(dirname(MANIFEST), file.from);
  const originalSource = await readFile(targetPath, 'utf8');
  const payloadSource = await readFile(payloadPath, 'utf8');
  const targetSha = sha256(originalSource);
  const payloadSha = sha256(payloadSource);
  const isPatched = targetSha === PATCHED_SHA256;
  const isLegacyPatched = targetSha === LEGACY_PATCHED_SHA256;
  check('manifest expectSha256 与原始 dist 一致', file.expectSha256 === ORIGINAL_SHA256);
  check('补丁载荷哈希与记录一致', payloadSha === PATCHED_SHA256, `sha256=${payloadSha}`);
  check(
    '目标文件为原始版本、本补丁版本或可识别旧版本',
    targetSha === ORIGINAL_SHA256 || isPatched || isLegacyPatched,
    `sha256=${targetSha}`
  );
  if (isLegacyPatched) {
    check('目标文件不是旧版载荷', false, '检测到 v1.0.0 载荷，请先撤销旧补丁记录，再应用当前版本');
  }
  if (requireApplied) check('目标文件已应用补丁', isPatched);

  const patchedSource = isPatched ? originalSource : payloadSource;
  check('补丁载荷包含 header cache', patchedSource.includes(CACHE_MARKER));
  check('补丁载荷保留原始 export', patchedSource.includes('export { JsonlCompressionSchema, JsonlSessionPersistence, JsonlSessionPersistence as default };'));
  return { patch, targetPath, originalSource, patchedSource, targetSha };
}

async function syntaxCheck(source) {
  const tempRoot = await mkdtemp(join(tmpdir(), 'dsh-session-perf-syntax-'));
  const tempFile = join(tempRoot, 'index.js');
  try {
    await writeFile(tempFile, source, 'utf8');
    const result = spawnSync(process.execPath, ['--check', tempFile], { encoding: 'utf8' });
    check('补丁载荷通过 Node 语法检查', result.status === 0, result.stderr.trim().split('\n')[0] ?? '');
  } finally {
    await rm(tempRoot, { recursive: true, force: true });
  }
}

async function behaviorCheck(source, kernelRoot) {
  const tempRoot = await mkdtemp(join(tmpdir(), 'dsh-session-perf-module-'));
  const packageRoot = join(tempRoot, 'node_modules', '@deepseek-ai', 'dsh-session-persistence-jsonl');
  const moduleDir = join(packageRoot, 'lib');
  const moduleFile = join(moduleDir, 'index.js');
  const tempSessionRoot = await mkdtemp(join(tmpdir(), 'dsh-session-perf-data-'));
  const delay = (milliseconds) => new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
  const realDateNow = Date.now;
  try {
    await mkdir(moduleDir, { recursive: true });
    const dependencyNodeModules = join(packageRoot, 'node_modules');
    await symlink(join(kernelRoot, 'node_modules'), dependencyNodeModules, process.platform === 'win32' ? 'junction' : 'dir');
    await writeFile(moduleFile, source, 'utf8');

    const cordisUrl = pathToFileURL(join(kernelRoot, 'node_modules/@deepseek-ai/cordis/lib/index.js')).href;
    const persistenceUrl = `${pathToFileURL(moduleFile).href}?dshSessionPerf=${Date.now()}`;
    const [{ Context }, { JsonlSessionPersistence }] = await Promise.all([
      import(cordisUrl),
      import(persistenceUrl),
    ]);

    const ctx = new Context();
    ctx.provide('sessions', { list: () => [], get: () => undefined });
    const persistence = new JsonlSessionPersistence(ctx, { root: tempSessionRoot, compression: 'zstd' });
    // Bypass only the real filesystem walk; the patched prototype remains under test.
    persistence.rootEncodingCheck = Promise.resolve();
    persistence.coordinator.initFor = () => ({});
    persistence.coordinator.retire = () => {};
    const projectPath = join(tempSessionRoot, 'project');
    const sessionPath = join(projectPath, 'session');
    await mkdir(sessionPath, { recursive: true });
    await writeFile(join(sessionPath, 'session.jsonl.zstd'), 'test');
    let walks = 0;
    let clock = realDateNow();
    Date.now = () => clock;
    persistence.listProjectDirs = async () => {
      walks += 1;
      await delay(10);
      return [projectPath];
    };
    persistence.listSessionDirs = async () => [sessionPath];
    persistence.exists = async (path) => path.endsWith('session.jsonl.zstd');
    persistence.readFirstZstdLine = async () => JSON.stringify({
      type: 'session',
      version: 1,
      id: 'dsh-session-perf-test',
      createdAt: 1,
      cwd: tempSessionRoot,
      delegationDepth: 0,
    });
    persistence.assertStoredIdentity = async () => {};

    const [first, second] = await Promise.all([persistence.list(), persistence.list()]);
    check('并发 persistence.list 共享一次扫描', walks === 1, `扫描次数=${walks}`);
    check('首次扫描返回 artifact header', first.length === 1 && first[0].id === 'dsh-session-perf-test');
    check('每个调用方获得独立的数组和 header', first !== second && first[0] !== second[0], '避免调用方修改缓存');
    first[0].id = 'caller-mutated';
    first.push({ id: 'caller-mutated' });
    const afterMutation = await persistence.list();
    check('TTL 内重复 persistence.list 命中缓存', walks === 1, `扫描次数=${walks}`);
    check('调用方修改不会污染缓存', afterMutation.length === 1 && afterMutation[0].id === 'dsh-session-perf-test');

    const snapshots = await persistence.listSnapshots();
    check('listSnapshots 复用 artifact 缓存', walks === 1 && snapshots.length === 1 && typeof snapshots[0].revision === 'string');

    clock += 1001;
    const afterTtl = await persistence.list();
    check('TTL 到期后重新扫描', walks === 2 && afterTtl.length === 1, `扫描次数=${walks}`);

    ctx.emit('session/created', { id: 'dsh-session-perf-test' });
    await persistence.list();
    check('session/created 事件使缓存失效', walks === 3, `扫描次数=${walks}`);

    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    await persistence.list();
    check('session/disposed 事件使缓存失效', walks === 4, `扫描次数=${walks}`);

    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    persistence.exists = async () => false;
    const missing = await persistence.list();
    check('缺失 artifact 仍 fail-soft 且不返回幽灵会话', walks === 5 && missing.length === 0);

    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    persistence.exists = async (path) => path.endsWith('session.jsonl.zstd');
    persistence.readFirstZstdLine = async () => 'not-json';
    const malformed = await persistence.list();
    check('损坏 header 仍 fail-soft 且不阻断列表', walks === 6 && malformed.length === 0);

    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    let failedAttempt = true;
    persistence.readFirstZstdLine = async () => {
      if (failedAttempt) {
        failedAttempt = false;
        throw new Error('simulated header read failure');
      }
      return JSON.stringify({
        type: 'session',
        version: 1,
        id: 'dsh-session-perf-test',
        createdAt: 1,
        cwd: tempSessionRoot,
        delegationDepth: 0,
      });
    };
    let scanFailed = false;
    try {
      await persistence.list();
    } catch {
      scanFailed = true;
    }
    const recovered = await persistence.list();
    check('扫描失败不会写入缓存，下一次调用会重试', scanFailed && walks === 8 && recovered.length === 1);

    const concurrentCtx = new Context();
    concurrentCtx.provide('sessions', { list: () => [], get: () => undefined });
    const concurrentPersistence = new JsonlSessionPersistence(concurrentCtx, { root: tempSessionRoot, compression: 'zstd' });
    concurrentPersistence.rootEncodingCheck = Promise.resolve();
    const concurrentProjectPath = join(tempSessionRoot, 'concurrent');
    const concurrentDirs = Array.from({ length: 20 }, (_, index) => join(concurrentProjectPath, `session-${index}`));
    let concurrentWalks = 0;
    let headerReads = 0;
    let activeHeaders = 0;
    let maxActiveHeaders = 0;
    concurrentPersistence.listProjectDirs = async () => {
      concurrentWalks += 1;
      return [concurrentProjectPath];
    };
    concurrentPersistence.listSessionDirs = async () => concurrentDirs;
    concurrentPersistence.exists = async (path) => path.endsWith('session.jsonl.zstd');
    concurrentPersistence.assertStoredIdentity = async () => {};
    concurrentPersistence.readFirstZstdLine = async (path, signal) => {
      signal?.throwIfAborted();
      activeHeaders += 1;
      maxActiveHeaders = Math.max(maxActiveHeaders, activeHeaders);
      try {
        await delay(5);
        signal?.throwIfAborted();
        headerReads += 1;
        return JSON.stringify({
          type: 'session',
          version: 1,
          id: basename(dirname(path)),
          createdAt: 1,
          cwd: tempSessionRoot,
          delegationDepth: 0,
        });
      } finally {
        activeHeaders -= 1;
      }
    };
    const concurrentRows = await concurrentPersistence.list();
    check(
      'header 探测使用有界并发且保留目录顺序',
      concurrentWalks === 1 && headerReads === concurrentDirs.length && concurrentRows.length === concurrentDirs.length
        && maxActiveHeaders > 1 && maxActiveHeaders <= 16
    );
    check('并发扫描结果顺序稳定', concurrentRows[0]?.id === 'session-0' && concurrentRows.at(-1)?.id === 'session-19');

    const abortCtx = new Context();
    abortCtx.provide('sessions', { list: () => [], get: () => undefined });
    const abortPersistence = new JsonlSessionPersistence(abortCtx, { root: tempSessionRoot, compression: 'zstd' });
    abortPersistence.rootEncodingCheck = Promise.resolve();
    let sharedSignal;
    abortPersistence.listProjectDirs = async (signal) => {
      sharedSignal = signal;
      await delay(50);
      signal?.throwIfAborted();
      return [];
    };
    const controller = new AbortController();
    const pending = abortPersistence.list(controller.signal);
    const survivor = abortPersistence.list();
    controller.abort();
    let aborted = false;
    try {
      await pending;
    } catch {
      aborted = true;
    }
    const sharedScanSurvived = sharedSignal !== undefined && !sharedSignal.aborted;
    const survivorRows = await survivor;
    check('调用方 abort 只取消自身等待', aborted && sharedScanSurvived && survivorRows.length === 0);

    const allAbortCtx = new Context();
    allAbortCtx.provide('sessions', { list: () => [], get: () => undefined });
    const allAbortPersistence = new JsonlSessionPersistence(allAbortCtx, { root: tempSessionRoot, compression: 'zstd' });
    allAbortPersistence.rootEncodingCheck = Promise.resolve();
    let canceledSignal;
    allAbortPersistence.listProjectDirs = async (signal) => {
      canceledSignal = signal;
      await delay(50);
      signal?.throwIfAborted();
      return [];
    };
    const onlyController = new AbortController();
    const onlyPending = allAbortPersistence.list(onlyController.signal);
    await new Promise((resolvePromise) => setImmediate(resolvePromise));
    onlyController.abort();
    let onlyAborted = false;
    try {
      await onlyPending;
    } catch {
      onlyAborted = true;
    }
    check('所有等待者退出后才取消共享扫描', onlyAborted && canceledSignal?.aborted === true);
  } finally {
    Date.now = realDateNow;
    await rm(tempRoot, { recursive: true, force: true });
    await rm(tempSessionRoot, { recursive: true, force: true });
  }
}

async function main() {
  const kernelRoot = resolveKernelRoot();
  console.log(`内核根目录：${kernelRoot}`);
  const { patchedSource, targetSha } = await loadPatch(kernelRoot);
  await syntaxCheck(patchedSource);
  await behaviorCheck(patchedSource, kernelRoot);
  console.log(`\n当前目标状态：${targetSha === ORIGINAL_SHA256 ? '未应用（载荷验证模式）' : '已应用或已漂移'}`);
  if (requireApplied && failures === 0) console.log('补丁已应用且验证通过');
  else if (!requireApplied && failures === 0) console.log('载荷验证通过；应用后追加 --require-applied 检查目标文件');
  process.exitCode = failures === 0 ? 0 : 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  await main();
}
