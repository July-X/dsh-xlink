#!/usr/bin/env node
// 校验 dsh-file-perf 补丁是否已正确应用到当前激活的内核，并复跑关键行为断言。
//
// 用法：
//   node scripts/verify-dsh-file-perf.mjs                 # 自动定位激活内核
//   node scripts/verify-dsh-file-perf.mjs <内核根目录>
//
// 只读脚本：不写入内核、不写入 ~/.dsh，任何失败都以非零退出码结束。

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const DSH_HOME = process.env.DSH_HOME ?? join(homedir(), '.dsh');

const TARGETS = {
  fileReference: 'node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js',
  sessionReference: 'node_modules/@deepseek-ai/dsh-session-reference/lib/index.js',
};

// 与 src-tauri/resources/patches/dsh-file-perf/manifest.json 保持一致。
const SHA = {
  fileReferenceOriginal: 'dcf5299bf9a1c8dd33bf7d099f8d7bdfd52d69e516ba90e97cda0cdf402e0a7d',
  fileReferencePatched: '593e150adb4f5db2c16fa76d9980f66ef4190f31c6b73e0b890b76e92dedbd60',
  sessionReferenceOriginal: 'e67bda5c8ee2e39437b474a596bc7fa600dfc9eea6821ce0a5c3eb3003c4e106',
  sessionReferencePatched: 'a4bbe7c0010ccd004026a33b41787513bd28182a488339a4590a870a2a294a87',
};

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? `  ${detail}` : ''}`);
  if (!ok) failures += 1;
};

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

export function resolveKernelRoot(argv = process.argv.slice(2)) {
  if (argv[0]) return resolve(argv[0]);
  for (const variant of ['desktop', 'desktop-dev']) {
    const activeFile = join(DSH_HOME, variant, 'active.txt');
    if (!existsSync(activeFile)) continue;
    const version = readFileSync(activeFile, 'utf8').trim();
    const root = join(DSH_HOME, variant, 'kernels', version);
    if (existsSync(root)) return root;
  }
  throw new Error(`找不到激活内核：请显式传入内核根目录（检查 ${DSH_HOME}/desktop/active.txt）`);
}

function checkPayloadHashes(kernelRoot) {
  console.log('— 载荷哈希（补丁是否落到内核）');
  for (const [key, rel] of Object.entries(TARGETS)) {
    const path = join(kernelRoot, rel);
    if (!existsSync(path)) {
      check(`${rel} 存在`, false, '文件缺失');
      continue;
    }
    const actual = sha256(path);
    const patched = SHA[`${key}Patched`];
    const original = SHA[`${key}Original`];
    check(
      `${rel.split('/').at(-3)} 已是补丁后版本`,
      actual === patched,
      actual === original ? '仍是原始文件（补丁未应用）' : actual === patched ? '' : `未知哈希 ${actual.slice(0, 12)}…`,
    );
  }
}

async function checkFileReference(kernelRoot) {
  console.log('\n— file-reference-local 行为断言');
  const mod = await import(pathToFileURL(join(kernelRoot, TARGETS.fileReference)).href);
  const { WorkspaceFileSearch } = mod;
  const cfg = { maxResults: 20, maxEntries: 10000, excludedDirectories: ['.git', 'node_modules'] };
  const root = process.cwd();
  const signal = new AbortController().signal;

  // 1. 在途查询被 invalidate 打断时不得抛错（原实现会 abort 并 reject → 菜单空白）
  const a = new WorkspaceFileSearch(root, cfg);
  const inFlight = a.list('index', signal);
  setTimeout(() => a.invalidate(), 5);
  let interrupted = 'rejected';
  try {
    await inFlight;
    interrupted = 'resolved';
  } catch { /* 保持 rejected */ }
  check('tool/result 打断在途查询时不抛错', interrupted === 'resolved', `(${interrupted})`);

  // 2. 失效后首查走 stale-while-revalidate，应为毫秒级
  const b = new WorkspaceFileSearch(root, cfg);
  await b.list('index', signal);
  b.invalidate();
  const t0 = performance.now();
  await b.list('index', signal).catch(() => []);
  const elapsed = performance.now() - t0;
  check('失效后首查为毫秒级（stale-while-revalidate）', elapsed < 50, `${elapsed.toFixed(1)} ms`);

  // 3. 后台扫描完成后必须发布新 generation；刷新期间再次失效不能丢失 stale。
  const refreshProbe = new WorkspaceFileSearch(root, cfg);
  let scanCount = 0;
  let refreshStarted;
  let releaseRefresh;
  const refreshReady = new Promise((resolve) => { refreshStarted = resolve; });
  refreshProbe.scanWorkspace = async (scanSignal) => {
    scanCount += 1;
    if (scanCount === 1) return [{ path: 'index-old.txt', kind: 'file', lower: 'index-old.txt', base: 'index-old.txt', hidden: false }];
    refreshStarted();
    await new Promise((resolve) => { releaseRefresh = resolve; });
    scanSignal.throwIfAborted();
    return [{ path: 'index-new.txt', kind: 'file', lower: 'index-new.txt', base: 'index-new.txt', hidden: false }];
  };
  await refreshProbe.list('old', signal);
  const servedGeneration = refreshProbe.generation;
  refreshProbe.invalidate();
  await refreshProbe.list('old', signal);
  await refreshReady;
  refreshProbe.invalidate();
  releaseRefresh();
  for (let i = 0; i < 100 && refreshProbe.refreshGeneration !== undefined; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  const refreshedIndex = await refreshProbe.generation.promise;
  check('后台扫描完成后发布新 generation', refreshProbe.generation !== servedGeneration && refreshedIndex.some((entry) => entry.path === 'index-new.txt'));
  check('刷新期间再次失效保留 stale', refreshProbe.stale === true);

  // 4. 目录列举回归守卫：@./ 与隐藏目录必须仍有结果
  const c = new WorkspaceFileSearch(root, cfg);
  const dot = await c.list('./', signal);
  check('@./ 目录列举非空（隐藏过滤回归守卫）', dot.length > 0, `${dot.length} 条`);
  a.dispose();
  b.dispose();
  c.dispose();
  refreshProbe.dispose();
}

async function checkSessionReference(kernelRoot) {
  console.log('\n— session-reference 行为断言');
  const wsPath = join(DSH_HOME, 'storages', 'workspace.json');
  if (!existsSync(wsPath)) {
    check('workspace.json 可读', false, `缺少 ${wsPath}`);
    return;
  }
  const archivedIds = JSON.parse(readFileSync(wsPath, 'utf8')).global?.archivedSessionIds ?? [];
  const archived = new Set(archivedIds);

  const sessionsRoot = join(DSH_HOME, 'sessions');
  const records = [];
  for (const cwdDir of existsSync(sessionsRoot) ? readdirSync(sessionsRoot) : []) {
    const base = join(sessionsRoot, cwdDir);
    if (!statSync(base).isDirectory()) continue;
    for (const sid of readdirSync(base)) {
      if (!existsSync(join(base, sid, 'session.jsonl.zstd'))) continue;
      records.push({ header: { id: sid, cwd: `/${cwdDir.replace(/^-|-$/g, '').replaceAll('-', '/')}`, createdAt: 1 }, live: false, persisted: true });
    }
  }
  if (records.length === 0 || archivedIds.length === 0) {
    console.log(`  (跳过：磁盘会话 ${records.length}，归档 ${archivedIds.length}，样本不足)`);
    return;
  }

  const { SessionReferenceResolver } = await import(pathToFileURL(join(kernelRoot, TARGETS.sessionReference)).href);
  const titleReads = [];
  const cacheReads = [];
  let listCalls = 0;
  const makeCtx = (get, useCache = true) => ({
    sessionQuery: {
      listSessions: async () => {
        listCalls += 1;
        return records;
      },
      readTitleSnapshots: async (ids) => {
        titleReads.push(...ids);
        return ids.map(() => ({ status: 'fulfilled', value: { title: { title: 't' } } }));
      },
    },
    get: (key) => {
      if (key === 'sessionProjectionCache' && useCache) {
        return {
          cachedSnapshot: (header) => {
            cacheReads.push(header.id);
            return { asOfSeq: 0, values: { title: `cached-${header.id}` } };
          },
        };
      }
      return get(key);
    },
    logger: { warn() {} },
  });
  const make = (get, useCache = true) => {
    const svc = Object.create(SessionReferenceResolver.prototype);
    Object.defineProperty(svc, 'ctx', { value: makeCtx(get, useCache), writable: true });
    svc.config = { maxReferences: 3, candidateLimit: 8, maxReferenceBytes: 65536 };
    svc.records = undefined;
    svc.recordsRefresh = undefined;
    svc.recordsStale = false;
    svc.recordsRevision = 0;
    return svc;
  };
  const headers = new Map(records.map((record) => [record.header.id, record.header]));
  const registryGet = (key) => (key === 'workspaceRegistry' ? { archivedSessionIds: archivedIds, headers } : undefined);

  const self = records.find((r) => !archived.has(r.header.id)).header.id;
  const agent = { id: self, session: { header: { cwd: process.cwd() } } };

  const fastSvc = make(registryGet);
  const beforeListCalls = listCalls;
  const fast = await fastSvc.listCandidates(agent, '', 8, undefined);
  check('workspace header 索引可直接返回候选', fast.length > 0 && listCalls === beforeListCalls, `${fast.length} 条，listSessions ${listCalls - beforeListCalls} 次`);

  const svc = make(registryGet);
  titleReads.length = 0;
  cacheReads.length = 0;
  const empty = await svc.listCandidates(agent, '', 8, undefined);
  check('空查询候选不含已归档', empty.every((c) => !archived.has(c.sessionId)), `${empty.length} 条`);
  check('候选优先使用投影缓存标题', empty.every((c) => c.label.startsWith('cached-')), `读缓存 ${cacheReads.length} 条`);
  check('候选路径不再读取会话日志标题', titleReads.length === 0, `readTitleSnapshots ${titleReads.length} 次`);

  const archivedId = archivedIds[0];
  const needle = String(archivedId).slice(8, 16);
  const hit = await svc.listCandidates(agent, needle, 8, undefined);
  check('按 id 精确查已归档会话返回空', hit.length === 0, `needle=${needle}，返回 ${hit.length} 条`);

  const liveOne = records.find((r) => !archived.has(r.header.id) && r.header.id !== self);
  if (liveOne) {
    const found = await svc.listCandidates(agent, liveOne.header.id.slice(8, 16), 8, undefined);
    check('未归档会话仍可被 id 查到', found.some((c) => c.sessionId === liveOne.header.id));
  }

  const noRegistry = make(() => undefined);
  check('未挂载 workspace 时退回不过滤', (await noRegistry.listCandidates(agent, needle, 8, undefined)).length === 1);

  const unstarted = make(() => ({ get archivedSessionIds() { throw new Error('workspace registry is not started yet'); } }));
  check('注册表未启动时退回不过滤且不抛错', (await unstarted.listCandidates(agent, needle, 8, undefined)).length === 1);

  const noCache = make(registryGet, false);
  const fallback = await noCache.listCandidates(agent, '', 1, undefined);
  check('投影缓存不可用时回退到 session id', fallback[0]?.label === fallback[0]?.sessionId);
}

async function main() {
  const kernelRoot = resolveKernelRoot();
  console.log(`内核根目录：${kernelRoot}\n`);
  checkPayloadHashes(kernelRoot);
  await checkFileReference(kernelRoot);
  await checkSessionReference(kernelRoot);
  console.log(failures === 0 ? '\n全部通过' : `\n${failures} 项失败`);
  process.exitCode = failures === 0 ? 0 : 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  await main();
}
