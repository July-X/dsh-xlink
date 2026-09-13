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
const PATCH_VERSION = '1.3.0';
const MIN_KERNEL_VERSION = '0.1.5-alpha.2';
const MAX_KERNEL_VERSION = '0.1.5-rc.2';
const TARGET = 'node_modules/@deepseek-ai/dsh-session-persistence-jsonl/lib/index.js';
const MANIFEST = resolve('src-tauri/resources/patches/dsh-session-perf/manifest.json');
// npm @deepseek-ai/dsh-session-persistence-jsonl@0.1.5-alpha.2 / 0.1.5-rc.1 / 0.1.5-rc.2
// 原始 dist（三者逐字节相同，本机已安装内核实测一致）
const ORIGINAL_SHA256 = '7d0640c9fc4be6c703b77605fdee6af519c542fae28a6cd4489353309812f062';
// 更早的 patched 载荷：v1.0.1 / v1.1.0 针对 0.1.1-rc.2 与 0.1.2-alpha.2，v1.2.0 针对
// 0.1.2-alpha.2 / alpha.3。它们锚定的是另一条内核线（官方自 0.1.2-alpha.4 起重写了目标
// 文件），在当前 0.1.5 范围内不可能出现，因此只用于识别与提示，不再计入失败——把"另一个
// 内核线上的历史载荷"报成红灯会让维护者以为补丁坏了。已存在的应用记录仍可在设置页撤销。
const LEGACY_PATCHED_SHA256 = [
  '9ed3fe3cfa3890e8559efd9369efac9866c19c3737c3328b6355c338f0a7f96e',
  'f9985512945738f32a29a6c34a3cda2e64ec1d051482a371c634e3fadaffb6ff',
  '29d2501e9477633e0d1829edd554078329fdf3959bf0fff50672159bdeda6299',
];
// v1.3.0 的 patched 载荷（锚定 0.1.5-rc.2）
const PATCHED_SHA256 = '89f0ad6567e791c9a8bf3bd293fe2a7650835bfd146a4adc9b1fcc5c5cedb14a';
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
  check(`补丁最低支持内核 ${MIN_KERNEL_VERSION}`, patch.minKernelVersion === MIN_KERNEL_VERSION);
  check(`补丁最高支持内核 ${MAX_KERNEL_VERSION}`, patch.maxKernelVersion === MAX_KERNEL_VERSION);
  check('补丁不标记 superseded（官方仍未实现枚举缓存）', patch.supersededSinceKernelVersion === undefined);
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
  const isLegacyPatched = LEGACY_PATCHED_SHA256.includes(targetSha);
  check('manifest expectSha256 与原始 dist 一致', file.expectSha256 === ORIGINAL_SHA256);
  check('补丁载荷哈希与记录一致', payloadSha === PATCHED_SHA256, `sha256=${payloadSha}`);

  // 目标文件检查同样只在"这就是本补丁锚定的那一份"时有意义：自动发现到的
  // 活动内核可能是已被官方重写过目标文件的更高版本，把这种"不适用"报成
  // 失败会让维护者以为补丁坏了（P2-47）。
  const anchoredTarget = targetSha === ORIGINAL_SHA256 || isPatched || isLegacyPatched;
  if (anchoredTarget) {
    check('目标文件为原始版本、本补丁版本或可识别旧版本', true, `sha256=${targetSha}`);
  } else if (requireApplied || positional) {
    check('目标文件为原始版本、本补丁版本或可识别旧版本', false, `sha256=${targetSha}`);
  } else {
    console.log(
      `! 目标文件不是本补丁锚定的版本（sha256=${targetSha}），跳过目标文件状态校验；` +
        '显式传入锚定内核根目录或追加 --require-applied 进行严格检查',
    );
  }
  if (isLegacyPatched) {
    console.log(
      '! 目标文件是更早版本（v1.0.1 / v1.1.0 / v1.2.0）的补丁载荷：那些版本锚定的是 ' +
        '0.1.1-rc.2 / 0.1.2-alpha.x 内核线，当前版本不适用于该内核；' +
        '已存在的应用记录可在设置页正常撤销（撤销走 state.json 记录，不受版本范围影响）',
    );
  }
  if (requireApplied) check('目标文件已应用补丁', isPatched);

  const patchedSource = isPatched ? originalSource : payloadSource;
  check('补丁载荷包含 header cache', patchedSource.includes(CACHE_MARKER));
  // 0.1.5 线起官方去掉了具名导出 `JsonlSessionPersistence`，只保留 default，
  // 载荷必须与目标 dist 的导出面完全一致。
  check(
    '补丁载荷保留原始 export',
    patchedSource.includes('export { JsonlCompressionSchema, JsonlSessionPersistence as default };'),
  );
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
    // 0.1.5 线的 dist 只导出 default（具名导出 `JsonlSessionPersistence` 已被官方移除），
    // 载荷必须保持这一点，因此这里也只取 default。
    const { SessionFormatUnsupportedError } = await import(
      pathToFileURL(join(kernelRoot, 'node_modules/@deepseek-ai/dsh-session-persistence/lib/index.js')).href
    );
    const [{ Context }, { default: JsonlSessionPersistence }] = await Promise.all([
      import(cordisUrl),
      import(persistenceUrl),
    ]);

    const ctx = new Context();
    ctx.provide('sessions', { list: () => [], get: () => undefined });
    const persistence = new JsonlSessionPersistence(ctx, { root: tempSessionRoot, compression: 'zstd' });
    // 只绕过真实目录的编码探测：补丁后的 listArtifacts 仍然是被测对象。
    // `ensureRootEncoding()` 会自行调用 listProjectDirs，绕不过去就会把扫描计数搅乱。
    persistence.rootEncodingCheck = Promise.resolve();
    const projectPath = join(tempSessionRoot, 'project');
    const sessionPath = join(projectPath, 'session');
    await mkdir(sessionPath, { recursive: true });
    // list() 会对 artifact.path 做真实的 stat（revision/sizeBytes），所以路径必须真实存在。
    await writeFile(join(sessionPath, 'session.jsonl.zstd'), 'test');
    const header = { version: 3, id: 'dsh-session-perf-test', createdAt: 1 };
    // 0.1.5 线把"逐目录找日志 + 读 header"拆成了 resolveGenerationInDirectory /
    // readGenerationHeader 两步，补丁改的正是驱动这两步的目录循环，因此 stub 这两步。
    const selectedFor = (dir) => ({ sourcePath: join(dir, 'session.jsonl.zstd'), sourceVersion: 3 });
    let walks = 0;
    let clock = realDateNow();
    Date.now = () => clock;
    persistence.listProjectDirs = async () => {
      walks += 1;
      await delay(10);
      return [projectPath];
    };
    persistence.listSessionDirs = async () => [sessionPath];
    persistence.resolveGenerationInDirectory = async (dir) => selectedFor(dir);
    persistence.readGenerationHeader = async () => header;

    const [first, second] = await Promise.all([persistence.list(), persistence.list()]);
    check('并发 persistence.list 共享一次扫描', walks === 1, `扫描次数=${walks}`);
    check('首次扫描返回 artifact header', first.length === 1 && first[0].header?.id === 'dsh-session-perf-test');
    check(
      '每个调用方获得独立的数组和 header',
      first !== second && first[0] !== second[0] && first[0].header !== second[0].header,
      '避免调用方修改缓存',
    );
    first[0].header.id = 'caller-mutated';
    first.push({ header: { id: 'caller-mutated' } });
    const afterMutation = await persistence.list();
    check('TTL 内重复 persistence.list 命中缓存', walks === 1, `扫描次数=${walks}`);
    check('调用方修改不会污染缓存', afterMutation.length === 1 && afterMutation[0].header.id === 'dsh-session-perf-test');

    clock += 1001;
    const afterTtl = await persistence.list();
    check('TTL 到期后重新扫描', walks === 2 && afterTtl.length === 1, `扫描次数=${walks}`);

    ctx.emit('session/created', { id: 'dsh-session-perf-test' });
    await persistence.list();
    check('session/created 事件使缓存失效', walks === 3, `扫描次数=${walks}`);

    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    await persistence.list();
    check('session/disposed 事件使缓存失效', walks === 4, `扫描次数=${walks}`);

    // 该目录没有可用 generation：上游语义是跳过（continue），不是报错。
    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    persistence.resolveGenerationInDirectory = async () => undefined;
    const missing = await persistence.list();
    check('无可用 generation 仍 fail-soft 且不返回幽灵会话', walks === 5 && missing.length === 0);

    // header 解析不出来：上游语义同样是跳过。
    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    persistence.resolveGenerationInDirectory = async (dir) => selectedFor(dir);
    persistence.readGenerationHeader = async () => undefined;
    const malformed = await persistence.list();
    check('损坏 header 仍 fail-soft 且不阻断列表', walks === 6 && malformed.length === 0);

    // 不支持的既有格式：上游抛 SessionFormatUnsupportedError 并跳过。
    // 并发化不能把这个 fail-soft 契约变成"整个列表失败"。
    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    persistence.readGenerationHeader = async () => {
      throw new SessionFormatUnsupportedError('unsupported fixture format', { kind: 'jsonl', path: 'fixture' });
    };
    const unsupported = await persistence.list();
    check('不支持的 generation 被跳过而不是让整次扫描失败', walks === 7 && unsupported.length === 0);

    ctx.emit('session/disposed', { id: 'dsh-session-perf-test' });
    let failedAttempt = true;
    persistence.readGenerationHeader = async () => {
      if (failedAttempt) {
        failedAttempt = false;
        throw new Error('simulated header read failure');
      }
      return header;
    };
    let scanFailed = false;
    try {
      await persistence.list();
    } catch {
      scanFailed = true;
    }
    const recovered = await persistence.list();
    check('扫描失败不会写入缓存，下一次调用会重试', scanFailed && walks === 9 && recovered.length === 1, `扫描次数=${walks}`);

    // 重复会话 id：仍必须报错，且判据按**目录顺序**而不是完成顺序裁决
    // （第一个目录故意更慢，若聚合时丢掉了顺序，这里就会漏报）。
    const dupCtx = new Context();
    dupCtx.provide('sessions', { list: () => [], get: () => undefined });
    const dupPersistence = new JsonlSessionPersistence(dupCtx, { root: tempSessionRoot, compression: 'zstd' });
    dupPersistence.rootEncodingCheck = Promise.resolve();
    const dupDirs = [];
    for (const name of ['dup-a', 'dup-b']) {
      const dir = join(tempSessionRoot, name, 'session');
      await mkdir(dir, { recursive: true });
      await writeFile(join(dir, 'session.jsonl.zstd'), 'test');
      dupDirs.push(dir);
    }
    dupPersistence.listProjectDirs = async () => [join(tempSessionRoot, 'dup-a'), join(tempSessionRoot, 'dup-b')];
    dupPersistence.listSessionDirs = async () => dupDirs;
    dupPersistence.resolveGenerationInDirectory = async (dir) => selectedFor(dir);
    dupPersistence.readGenerationHeader = async (selected) => {
      await delay(selected.sourcePath.includes('dup-a') ? 15 : 1);
      return { version: 3, id: 'duplicate-fixture', createdAt: 1 };
    };
    let duplicateRejected = false;
    try {
      await dupPersistence.list();
    } catch (error) {
      duplicateRejected = /duplicate JSONL session id/.test(String(error && error.message));
    }
    check('并发扫描仍按目录顺序检出重复会话 id', duplicateRejected);

    const concurrentCtx = new Context();
    concurrentCtx.provide('sessions', { list: () => [], get: () => undefined });
    const concurrentPersistence = new JsonlSessionPersistence(concurrentCtx, { root: tempSessionRoot, compression: 'zstd' });
    concurrentPersistence.rootEncodingCheck = Promise.resolve();
    const concurrentProjectPath = join(tempSessionRoot, 'concurrent');
    const concurrentDirs = Array.from({ length: 20 }, (_, index) => join(concurrentProjectPath, `session-${index}`));
    for (const dir of concurrentDirs) {
      await mkdir(dir, { recursive: true });
      await writeFile(join(dir, 'session.jsonl.zstd'), 'test');
    }
    let concurrentWalks = 0;
    let headerReads = 0;
    let activeHeaders = 0;
    let maxActiveHeaders = 0;
    concurrentPersistence.listProjectDirs = async () => {
      concurrentWalks += 1;
      return [concurrentProjectPath];
    };
    concurrentPersistence.listSessionDirs = async () => concurrentDirs;
    concurrentPersistence.resolveGenerationInDirectory = async (dir) => selectedFor(dir);
    concurrentPersistence.readGenerationHeader = async (selected, expectedId, signal) => {
      signal?.throwIfAborted();
      activeHeaders += 1;
      maxActiveHeaders = Math.max(maxActiveHeaders, activeHeaders);
      try {
        await delay(5);
        signal?.throwIfAborted();
        headerReads += 1;
        return { version: 3, id: basename(dirname(selected.sourcePath)), createdAt: 1 };
      } finally {
        activeHeaders -= 1;
      }
    };
    const concurrentRows = await concurrentPersistence.list();
    check(
      'header 探测使用有界并发且保留目录顺序',
      concurrentWalks === 1 && headerReads === concurrentDirs.length && concurrentRows.length === concurrentDirs.length
        && maxActiveHeaders > 1 && maxActiveHeaders <= 16,
      `最大并发=${maxActiveHeaders}`,
    );
    check(
      '并发扫描结果顺序稳定',
      concurrentRows[0]?.header?.id === 'session-0' && concurrentRows.at(-1)?.header?.id === 'session-19',
    );

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
    const pending = abortPersistence.list({ signal: controller.signal });
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
    const onlyPending = allAbortPersistence.list({ signal: onlyController.signal });
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

  // 行为检查只在目标文件确实是本补丁锚定的那一份时才有意义。自动发现到的
  // 活动内核很可能是更高版本（官方已经重写过这个模块），此时按老版本 dist
  // 写的模块导入会直接崩栈，维护者看到的是"补丁坏了"而不是"当前内核不适
  // 用"（P2-47）。显式传入内核根目录或 --require-applied 时仍然严格运行。
  const anchored =
    targetSha === ORIGINAL_SHA256 ||
    targetSha === PATCHED_SHA256 ||
    LEGACY_PATCHED_SHA256.includes(targetSha);
  const explicit = requireApplied || Boolean(positional);
  if (anchored || explicit) {
    try {
      await behaviorCheck(patchedSource, kernelRoot);
    } catch (error) {
      // 崩栈也要计入失败并汇总退出码，而不是把栈直接甩给用户。
      // （注意 `failures` 是计数器而不是数组。）
      failures += 1;
      console.log(
        `✗ 行为检查抛出异常：${error && error.message ? error.message : error}`,
      );
    }
  } else {
    console.log(
      `! 当前内核不是本补丁锚定的版本（目标文件 sha256=${targetSha}），跳过行为检查；` +
        '显式传入锚定内核根目录或追加 --require-applied 可强制运行',
    );
  }
  console.log(`\n当前目标状态：${targetSha === ORIGINAL_SHA256 ? '未应用（载荷验证模式）' : '已应用或已漂移'}`);
  if (requireApplied && failures === 0) console.log('补丁已应用且验证通过');
  else if (!requireApplied && failures === 0) console.log('载荷验证通过；应用后追加 --require-applied 检查目标文件');
  process.exitCode = failures === 0 ? 0 : 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  await main();
}
