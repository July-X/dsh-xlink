// P1 协议判据的**实跑**入口：对真实实现跑协议稿 §8 的 12 条验收项。
//
// ## 用法
//
// ```powershell
// node scripts/p1-protocol.run.mjs            # 跑一轮，输出到 %TEMP%\dsh-p1-protocol\
// node scripts/p1-protocol.run.mjs --round r2 # 指定轮次名（证据按轮次独立命名）
// ```
//
// ## 产出
//
// `%TEMP%\dsh-p1-protocol\results-<round>.json`：
// `{round, at, results: [{name,status,detail}], summary: {total,pass,fail,skipped}}`
// —— 与既有产物（内核 `results.<轮次>.json`）同构。
//
// ## 隔离
//
// 落盘根目录在 `%TEMP%\dsh-p1-protocol-*` 一次性目录里，**绝不指向用户真实数据目录**；
// 跑完删掉（只删自己建的）。原实现文件一个字节都不改。
//
// @module scripts/p1-protocol-run

import { execFileSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

import {
  PASS,
  FAIL,
  SKIPPED,
  checkCacheTtl,
  checkCapturedAtPriority,
  checkCaptureResultLanded,
  checkCaptureResultShape,
  checkCatalogCodesSeparated,
  checkChipInvalidNode,
  checkConstants,
  checkConstantsParity,
  checkCrossProcess,
  checkErrorCodeClosedSet,
  checkErrorCodeTable,
  checkErrorLogPath,
  checkErrorNoInternalLeak,
  checkFingerprintGate,
  checkFixtureCoverage,
  checkFixtureExpectationMatch,
  checkFixtureExpectations,
  checkFixtureParity,
  checkGapRegistry,
  checkHostInjection,
  checkHostRejectsUnclaimedId,
  checkInflightReferenceNotCovered,
  checkInvalidRejection,
  checkIsolationSurface,
  checkLogNotAgeDeleted,
  checkLogTruncation,
  checkNoBodyDedup,
  checkNamespaceIsolation,
  checkPickTwiceDistinct,
  checkRefLengthBoundary,
  checkRequestMatchesDisk,
  checkRetentionSweep,
  checkRootUnavailable,
  checkSeqCrossProcess,
  checkSingleSourceOfTruth,
  checkSweepFailureLogged,
  checkSweepThrottle,
  checkSweepTriggerBehaviour,
  checkSweepTriggerStatic,
  checkTwoStepDelete,
  stripJsComments,
  summarize,
  verdict,
} from './lib/p1-protocol.mjs';
import { createAdapter } from './lib/p1-adapter.mjs';
import {
  findDataDirectoryViolationsInFiles,
  findProfileViolationsInFiles,
} from './lib/probe-profile-isolation.mjs';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const REPO = fileURLToPath(new URL('../', import.meta.url));

/** 协议夹具目录（协议 §6 的「同一组 JSON 夹具」）。 */
const FIXTURE_DIR = join(REPO, 'plugins', 'official-chat', 'fixtures', 'protocol');

/** 本次新增的文件（协议 §8-11：新增代码不得让判据扫描面外泄）。 */
const NEW_FILES = [
  'scripts/lib/p1-protocol.mjs',
  'scripts/lib/p1-adapter.mjs',
  'scripts/p1-protocol.test.mjs',
  'scripts/p1-protocol.run.mjs',
  'src-tauri/examples/p1_protocol_fixture.rs',
];

/**
 * 读协议夹具集。
 *
 * @returns {Array<object>}
 */
function readFixtures() {
  if (!existsSync(FIXTURE_DIR)) return [];
  const indexPath = join(FIXTURE_DIR, 'index.json');
  const names = existsSync(indexPath)
    ? (JSON.parse(readFileSync(indexPath, 'utf8')).cases ?? [])
    : [];
  const list = names.length > 0 ? names : [];
  const out = [];
  for (const name of list) {
    const path = join(FIXTURE_DIR, name);
    if (existsSync(path)) out.push(JSON.parse(readFileSync(path, 'utf8')));
  }
  return out;
}

/**
 * A0｜**通道可达性**（client 半边 ↔ host 半边，真机级）——当前**不可机检 → 未执行**。
 *
 * 协议 v0.4 §8 第 13 条把这条拆成两条：
 *  · **A0s（仓库级、静态）**：host 半边**注册了 exact fetch route**（路径在 `/api`
 *    命名空间下）+ **不把 `rpc.handle` 当作已通** + 不用 cordis `ctx.set` 挂服务
 *    → **仓库级可机检**（见 `checkHostInjection`）。
 *  · **A0（真机级）**：client → host 经内核 `connection` 服务的**真实路由注册**
 *    跑通一次 → **仓库级 runner 跑不了**（需要真实内核进程与真实 cordis 树）。
 *
 * **为什么仓库级不能自己给自己打绿**：适配器里的 `connection` 是**模拟**内核
 * `connection` 服务的路由注册语义（**注册 exact route + 客户端自己 fetch**，含
 * 全等 pathname 匹配与 method 校验），但内核那边还有 owner 准入、`webServer`
 * 路由与 fiber scope 那一套。真机 p1-r2 / p1-r6 死在
 * `cannot get property "webServer" without inject`——**判据直连 handler 就会把
 * 这个洞盖住**（判据全绿、真机全红）。
 *
 * ⇒ 本条在仓库级**如实报「未执行」**并写明原因，**由真机复验轮执行**。
 *
 * @returns {{name: string, status: string, detail: string}}
 */
function checkChannelReachability() {
  return verdict(
    'A0-channel-reachability',
    SKIPPED,
    '未执行：**形态已定案**（协议 v0.4 §5：内核 `connection` 服务的 exact fetch route，' +
      '路径在 `/api` 命名空间下），但本条是**真机级**判据——需要真实内核进程与真实 ' +
      'cordis 树，仓库级 runner 不具备。仓库级已把能钉的部分拆成 A0s（静态：注册了 ' +
      'exact fetch route + 不用 `rpc.handle` + 不用 `ctx.set` 挂服务）；' +
      '「经真实路由注册跑通一次」由真机复验轮执行。',
  );
}

// ---------------------------------------------------------------------------
// A18 实跑：协议 §4.1 保留期主动清理的**剩余**口径
//
// ## 为什么要一个「实验室」
//
// A17 那条是**一次性造一批文件、扫一次、逐个看还在不在**。A18 要钉的是
// **节流、TTL、截断、两步法、失败处理、时间戳优先级**——这些**互相影响**
// （一次 `sweep()` 把四家族都动了），所以每条判据都用**自己的一次性根**，
// 互不串。
//
// ## 隔离
//
// 每个实验室都是 `%TEMP%` 下的一次性目录（`mkdtemp`），**绝不指向用户真实数据
// 目录**；跑完删掉（只删自己建的）。**原实现文件一个字节都不改**——
// 活体注入打在**副本**上（`hostDir` 参数）。
// ---------------------------------------------------------------------------

/**
 * 造一个「保留期清理实验室」。
 *
 * @param {object} options
 * @param {string} [options.hostDir] - host 半边目录（活体注入用**副本**）。
 * @param {string} [options.tag] - 临时目录标签。
 * @returns {Promise<object>} 实验室句柄。
 */
async function createSweepLab({ hostDir, tag = 'a18' } = {}) {
  const path = await import('node:path');
  const hostBase = hostDir
    ? pathToFileURL(`${hostDir}/`)
    : new URL('../plugins/official-chat/host/', import.meta.url);
  const sweepModule = await import(new URL('sweep.js', hostBase).href);
  const protocolModule = await import(new URL('protocol.js', hostBase).href);
  const retentionMs = protocolModule.CONSTANTS.snapshotRetentionMs;
  const cacheTtlMs = protocolModule.CONSTANTS.catalogCacheTtlMs;
  const provider = 'ds';
  const root = mkdtempSync(join(tmpdir(), `dsh-p1-sweep-${tag}-`));
  mkdirSync(join(root, 'snapshots', provider), { recursive: true });
  mkdirSync(join(root, 'seq-claims', provider), { recursive: true });
  mkdirSync(join(root, 'catalog-cache'), { recursive: true });
  mkdirSync(join(root, 'logs'), { recursive: true });

  const DAY = 24 * 60 * 60 * 1000;
  const now = Date.now();
  const expiredAt = now - retentionMs - DAY;
  const freshAt = now - 1000;

  /** 写一份快照（带清理要看的 `capturedAt` + 一点真实形状）。 */
  const writeSnapshot = ({ id, capturedAt, mtimeAt }) => {
    const file = join(root, 'snapshots', provider, `${id}.json`);
    const body = {
      schemaVersion: 1,
      snapshotId: id,
      providerId: provider,
      accountScope: 'p1/a18',
      sessionId: 'p1-a18',
      branch: 'main',
      messages: [{ role: 'user', text: 'A18 夹具正文' }],
      contentDigest: '0'.repeat(64),
      captureResult: { status: 'ok', omitted: [], unsupported: [] },
      capturedAt: new Date(capturedAt).toISOString(),
      sourceUpdatedAt: new Date(capturedAt).toISOString(),
      sourceFingerprint: 'fp-a18',
    };
    writeFileSync(file, `${JSON.stringify(body, null, 2)}\n`, 'utf8');
    if (mtimeAt !== undefined) utimesSync(file, mtimeAt / 1000, mtimeAt / 1000);
    return file;
  };

  /** 写一份序号认领。 */
  const writeClaim = ({ sessionShort, seq, claimedAt, mtimeAt }) => {
    const dir = join(root, 'seq-claims', provider, sessionShort);
    mkdirSync(dir, { recursive: true });
    const file = join(dir, `${seq}.json`);
    writeFileSync(
      file,
      `${JSON.stringify(
        {
          providerShort: provider,
          sessionShort,
          seq: Number(seq),
          claimedAt: new Date(claimedAt).toISOString(),
        },
        null,
        2,
      )}\n`,
      'utf8',
    );
    if (mtimeAt !== undefined) utimesSync(file, mtimeAt / 1000, mtimeAt / 1000);
    return file;
  };

  /** 写一份目录缓存。 */
  const writeCache = ({ name, mtimeAt }) => {
    const file = join(root, 'catalog-cache', name);
    writeFileSync(
      file,
      `${JSON.stringify({ fetchedAt: new Date(now).toISOString(), page: {} })}\n`,
      'utf8',
    );
    if (mtimeAt !== undefined) utimesSync(file, mtimeAt / 1000, mtimeAt / 1000);
    return file;
  };

  /** 写错误日志。 */
  const writeLog = ({ content, mtimeAt }) => {
    const file = join(root, 'logs', 'official-chat.log');
    writeFileSync(file, content, 'utf8');
    if (mtimeAt !== undefined) utimesSync(file, mtimeAt / 1000, mtimeAt / 1000);
    return file;
  };

  /** 造一个**真实**清理器（公开 API；`root` 语义 = 存储根）。 */
  const sweeper = (options = {}) =>
    sweepModule.createSweeper({
      root,
      retentionMs,
      cacheTtlMs,
      providerId: provider,
      now: () => now,
      ...options,
    });

  /** 目录下还剩什么（用于「不留半截文件」）。 */
  const leftovers = (rel) => {
    try {
      return readdirSync(join(root, rel));
    } catch {
      return [];
    }
  };

  return {
    root,
    provider,
    now,
    DAY,
    expiredAt,
    freshAt,
    retentionMs,
    cacheTtlMs,
    sweepModule,
    protocolModule,
    writeSnapshot,
    writeClaim,
    writeCache,
    writeLog,
    sweeper,
    leftovers,
    exists: (file) => existsSync(file),
    cleanup() {
      rmSync(root, { recursive: true, force: true });
    },
  };
}

/**
 * A18a｜§4.1 第 1 条（**静态**）：清理只在 `catalog` 入口触发、`capture` 不触发。
 *
 * **扫源码前必须剥注释**（实现里大量注释在解释「为什么这么设计」），
 * 剥的时候**保持行号**（等长空白替换）——判据要能点名 file:line。
 *
 * @param {string} [hostDir] - host 半边目录（活体注入用副本）。
 * @returns {{name: string, status: string, detail: string}}
 */
function runSweepTriggerStatic(hostDir) {
  const name = 'A18a-sweep-trigger-static';
  const file = hostDir === undefined
    ? join(REPO, 'plugins', 'official-chat', 'host', 'commands.js')
    : join(hostDir, 'commands.js');
  if (!existsSync(file)) {
    return verdict(name, SKIPPED, `没读到 ${file}——本条未执行`);
  }
  const code = stripJsComments(readFileSync(file, 'utf8'));
  const lines = code.split('\n');

  /** 找某个命令入口的行范围（按**同缩进**的下一个函数定义切）。 */
  const rangeOf = (fnName) => {
    const start = lines.findIndex((line) =>
      new RegExp(`^\\s*(async\\s+)?function\\s+${fnName}\\s*\\(`).test(line),
    );
    if (start < 0) return null;
    const indent = lines[start].match(/^\s*/)[0].length;
    for (let i = start + 1; i < lines.length; i += 1) {
      const m = /^(\s*)(async\s+)?function\s+\w+\s*\(/.exec(lines[i]);
      if (m !== null && m[1].length === indent) return { start, end: i - 1 };
    }
    return { start, end: lines.length - 1 };
  };

  const catalog = rangeOf('catalog');
  const capture = rangeOf('capture');
  if (catalog === null || capture === null) {
    return verdict(
      name,
      SKIPPED,
      `没定位到命令入口的实现形状（catalog=${catalog !== null} / capture=${capture !== null}）——` +
        '本条未执行，不判通过',
    );
  }

  // **sweeper 调用点**：`maybeSweep(` / `sweep(`（剥注释后仍命中的才是真调用）。
  //
  // **可选链也要算调用**：真实写法是 `sweeper?.maybeSweep?.()`——`maybeSweep`
  // 与 `(` 之间隔着 `?.`。只按 `maybeSweep(` 匹配会**一个都数不到** ⇒ 把合规实现
  // 判成「catalog 入口没有清理调用」的**假红**。
  const isSweepCall = (line) =>
    /\bmaybeSweep\s*(?:\?\.)?\s*\(/.test(line) || /\bsweep\s*(?:\?\.)?\s*\(/.test(line);
  let catalogCalls = 0;
  let captureCalls = 0;
  let outsideCalls = 0;
  const outsideLines = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (!isSweepCall(lines[i])) continue;
    if (i >= catalog.start && i <= catalog.end) catalogCalls += 1;
    else if (i >= capture.start && i <= capture.end) captureCalls += 1;
    else {
      outsideCalls += 1;
      outsideLines.push(i + 1);
    }
  }
  return checkSweepTriggerStatic({
    parsed: true,
    catalogCalls,
    captureCalls,
    outsideCalls,
    detail: outsideCalls > 0 ? `入口外的调用在第 ${outsideLines.join('、')} 行` : '',
  });
}

/**
 * A18b｜§4.1 第 1 条（**行为**）：清理由 `catalog` 触发、`capture` **不**触发。
 *
 * 观测口是适配器里的**替身 sweeper**（只记调用、不真扫）——它把「谁调了清理器」
 * 变成可观测，而判据要判的正是**触发点**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runSweepTriggerBehaviour(adapter) {
  const name = 'A18b-sweep-trigger-behaviour';
  if (typeof adapter.sweeperCalls !== 'function') {
    return verdict(name, SKIPPED, 'adapter 没有 sweeper 观测口——本条未执行');
  }
  const sessionId = 'p1-a18-trigger';
  const before = adapter.sweeperCalls().count;

  // ① `catalog` 入口：应当触发清理。
  const catalogResult = await adapter.callHost('catalog', {
    query: sessionId,
    requestId: 'p1-a18-catalog',
  });
  const afterCatalog = adapter.sweeperCalls().count;

  // ② `capture` 入口：**必须真的走完**（用 host 刚分配的 id）。
  //
  // **为什么不能随便传一个 id**：那会让 capture 在**校验阶段**就拒掉，
  // 于是「capture 没触发清理」只证明了「capture 进门就拒了」——
  // 若实现把清理调用放在 capture 的**末尾**（校验之后），这种观测**看不出来**。
  // ⇒ 先从 catalog 的回包里取 host 分配的 id，再让 capture 走完整条路。
  const allocated = String(catalogResult?.value?.allocations?.[sessionId] ?? '');
  if (allocated === '') {
    return verdict(
      name,
      SKIPPED,
      'catalog 没分配出 id（拿不到可用的 capture 入参）——本条未执行，不判通过',
    );
  }
  const captureResult = await adapter.callHost('capture', {
    sessionId,
    snapshotId: allocated,
    requestId: 'p1-a18-capture',
  });
  const afterCapture = adapter.sweeperCalls().count;

  // **capture 没成功 ⇒ 观测不成立**（路径没走完，说不清「没触发」）。
  const captureOk = captureResult?.ok === true;
  if (!captureOk) {
    return verdict(
      name,
      SKIPPED,
      `capture 没能走完（${JSON.stringify(captureResult?.error ?? captureResult)}）——` +
        '「capture 不触发清理」这条没法在没走完的路径上判，本条未执行',
    );
  }
  return checkSweepTriggerBehaviour({
    catalogTriggers: afterCatalog - before,
    captureTriggers: afterCapture - afterCatalog,
  });
}

/**
 * A18c｜§4.1 第 1 条：**同实例至多一天一次** + 节流键落位。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runSweepThrottle(hostDir) {
  const lab = await createSweepLab({ hostDir, tag: 'throttle' });
  try {
    const sweeper = lab.sweeper();
    const first = sweeper.maybeSweep();
    const second = sweeper.maybeSweep();
    const day = new Date(lab.now);
    const pad = (n) => String(n).padStart(2, '0');
    const dayKey = `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`;
    const stampName = `last-sweep-${dayKey}.json`;
    const stamp = join(lab.root, 'sweeps', stampName);
    // **节流键要落在自己的 `sweeps/` 目录里**（协议 §4.1 第 1 条：「独立目录，
    // 不混进快照目录」）⇒ 同时看三件事：
    //  ① `sweeps/` 确实是个目录；
    //  ② 目录里那个文件的名字**恰好**是 `last-sweep-<今天>.json`（不多不少）；
    //  ③ 快照目录里**没有**混进节流键文件。
    const sweepsDir = join(lab.root, 'sweeps');
    let sweepsIsDir = false;
    try {
      sweepsIsDir = statSync(sweepsDir).isDirectory();
    } catch {
      sweepsIsDir = false;
    }
    const dated = lab
      .leftovers('sweeps')
      .filter((name) => /^last-sweep-\d{4}-\d{2}-\d{2}\.json$/.test(name));
    const strayInSnapshots = lab
      .leftovers(join('snapshots', lab.provider))
      .some((name) => name.startsWith('last-sweep-'));
    return checkSweepThrottle({
      firstRan: first?.ran === true,
      secondRanFalse: second?.ran === false,
      stampExists: existsSync(stamp),
      stampNameMatches: dated.length === 1 && dated[0] === stampName,
      stampInOwnDir: sweepsIsDir && !strayInSnapshots,
    });
  } finally {
    lab.cleanup();
  }
}

/**
 * A18d｜§4.1 第 2 条：**目录缓存按 5 分钟 TTL 过期**（TTL 内留着）。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runCacheTtl(hostDir) {
  const lab = await createSweepLab({ hostDir, tag: 'cache-ttl' });
  try {
    const expired = lab.writeCache({
      name: 'expired.json',
      mtimeAt: lab.now - lab.cacheTtlMs - 60_000,
    });
    const fresh = lab.writeCache({ name: 'fresh.json', mtimeAt: lab.now - 1000 });
    lab.sweeper().sweep();
    return checkCacheTtl({
      expiredRemoved: !existsSync(expired),
      freshKept: existsSync(fresh),
    });
  } finally {
    lab.cleanup();
  }
}

/**
 * A18e｜§4.1 第 2 条：**日志按行数/大小截断**（未超不截；按行退；留尾部）。
 *
 * **上限用实现导出的常量来定输入大小**——那是**造输入**（怎么造一个超限的日志），
 * 不是**判标准答案**（判的是「超了要截、没超不截、截完仍是合法 UTF-8、留尾部」）。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runLogTruncation(hostDir) {
  // —— 情况①：行数超限（每行很短 ⇒ 字节没超）——
  const linesLab = await createSweepLab({ hostDir, tag: 'log-truncate-lines' });
  let truncatedWhenOver = false;
  let keptTail = false;
  let utf8Intact = true;
  try {
    const maxLines = linesLab.sweepModule.LOG_MAX_LINES ?? 2000;
    // **多字节字符**：按字节砍时若切半个字符，落盘就会是乱码（诊断证据被毁）。
    const line = '{"at":"2026-10-20T00:00:00.000Z","code":"timeout","detail":"中文字段"}';
    const lines = [];
    for (let i = 0; i < maxLines + 50; i += 1) lines.push(`${i} ${line}`);
    const big = `${lines.join('\n')}\n`;
    const bigFile = linesLab.writeLog({ content: big });
    const beforeBytes = statSync(bigFile).size;
    linesLab.sweeper().sweep();
    const after = readFileSync(bigFile, 'utf8');
    const afterBytes = Buffer.byteLength(after, 'utf8');
    truncatedWhenOver = afterBytes < beforeBytes;
    // **合法 UTF-8**：读回来的字符串里不该出现替换字符（U+FFFD）。
    utf8Intact = !after.includes('\uFFFD') && Buffer.from(after, 'utf8').toString('utf8') === after;
    // **留尾部**：最后一行（行号最大）还在。
    keptTail = after.includes(`${maxLines + 49} ${line}`);
  } finally {
    linesLab.cleanup();
  }

  // —— 情况②：字节超限（行数没超）——
  //
  // 协议 §4.1 第 2 条（PM 20:2x 澄清）：「按行数/大小截断」= **两个上限都要满足**。
  // 只钉「行数」那一路 ⇒ 实现只按行数截也会蒙过去 ⇒ 这里单独钉「字节」那一路，
  // 并且**行是长的、多字节的**（按字节砍时切半个多字节字符会毁掉诊断证据）。
  const bytesLab = await createSweepLab({ hostDir, tag: 'log-truncate-bytes' });
  let truncatedWhenOverBytes = false;
  let keptTailBytes = false;
  let utf8IntactBytes = true;
  try {
    const maxBytes = bytesLab.sweepModule.LOG_MAX_BYTES ?? 512 * 1024;
    const maxLines = bytesLab.sweepModule.LOG_MAX_LINES ?? 2000;
    // 每行 ~904 字节（300 个中文 = 900 字节）⇒ 行数远小于上限、字节数远超上限。
    const unit = '中'.repeat(300);
    const total = Math.ceil((maxBytes * 1.2) / 904) + 10;
    const lines = [];
    for (let i = 0; i < total; i += 1) lines.push(`${String(i).padStart(4, '0')} ${unit}`);
    const content = `${lines.join('\n')}\n`;
    const file = bytesLab.writeLog({ content });
    const beforeBytes = statSync(file).size;
    expectBytesOver(beforeBytes, maxBytes);
    bytesLab.sweeper().sweep();
    const after = readFileSync(file, 'utf8');
    const afterBytes = Buffer.byteLength(after, 'utf8');
    truncatedWhenOverBytes = afterBytes < beforeBytes && afterBytes <= maxBytes;
    utf8IntactBytes =
      !after.includes('\uFFFD') && Buffer.from(after, 'utf8').toString('utf8') === after;
    keptTailBytes = after.includes(`${String(total - 1).padStart(4, '0')} ${unit}`);
    // **行数上限也仍然要满足**（两个上限都要满足）。
    if (after.split('\n').length > maxLines) {
      truncatedWhenOverBytes = false;
    }
  } finally {
    bytesLab.cleanup();
  }

  // —— 情况③：没超限（反向回归）——
  const smallLab = await createSweepLab({ hostDir, tag: 'log-small' });
  let notTruncatedWhenSmall = false;
  try {
    const small = '一行日志\n两行日志\n';
    const smallFile = smallLab.writeLog({ content: small });
    smallLab.sweeper().sweep();
    notTruncatedWhenSmall = readFileSync(smallFile, 'utf8') === small;
  } finally {
    smallLab.cleanup();
  }

  return checkLogTruncation({
    truncatedWhenOver: truncatedWhenOver && truncatedWhenOverBytes,
    notTruncatedWhenSmall,
    utf8Intact: utf8Intact && utf8IntactBytes,
    keptTail: keptTail && keptTailBytes,
  });
}

/**
 * 「造出来的输入确实超了上限」的自检。
 *
 * **为什么要自检**：判据自己得先确认「输入真的超限」——否则「截断了」也可能只是
 * 实现乱删。自检不过就**抛**（让这一轮崩在明处），而不是把一个不成立的观测
 * 当成判据输入。
 *
 * @param {number} actual - 实际字节数。
 * @param {number} limit - 上限字节数。
 */
function expectBytesOver(actual, limit) {
  if (!(actual > limit)) {
    throw new Error(`判据自检失败：造出来的日志只有 ${actual} 字节，没超上限 ${limit}`);
  }
}

/**
 * A18f｜§4.1 第 2 条：**日志不按 7 天删**。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runLogNotAgeDeleted(hostDir) {
  const lab = await createSweepLab({ hostDir, tag: 'log-age' });
  try {
    const old = lab.now - 30 * lab.DAY;
    const file = lab.writeLog({ content: '很久以前的日志\n', mtimeAt: old });
    lab.sweeper().sweep();
    return checkLogNotAgeDeleted({ oldLogStillThere: existsSync(file) });
  } finally {
    lab.cleanup();
  }
}

/**
 * A18g｜§4.1 第 3 条：**两步法删除**（先 rename 再 unlink；不留半截文件）。
 *
 * 静态部分扫 `host/sweep.js`（剥注释）；行为部分看扫完之后**有没有残骸**。
 *
 * @param {string} [hostDir] - host 半边目录（活体注入用副本）。
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runTwoStepDelete(hostDir) {
  const name = 'A18g-two-step-delete';
  const file = hostDir === undefined
    ? join(REPO, 'plugins', 'official-chat', 'host', 'sweep.js')
    : join(hostDir, 'sweep.js');
  if (!existsSync(file)) {
    return verdict(name, SKIPPED, `没读到 ${file}——本条未执行`);
  }
  const code = stripJsComments(readFileSync(file, 'utf8'));
  const lines = code.split('\n');

  /**
   * 找出**包含某一行的那层函数**。
   *
   * **为什么要「包含」而不是「从那行往后」**：删除 helper 是
   * `rename` 在前、`unlink` 在后——如果拿 `unlink` 那行当区间起点，
   * `rename` 就不在区间里 ⇒ 「先 rename 后 unlink」永远判不成立。
   * ⇒ 先把**所有函数定义**列出来切成区间，再挑**包含**目标行的那一个
   * （嵌套时取**最内层**）。
   */
  const heads = [];
  for (let i = 0; i < lines.length; i += 1) {
    const m = /^(\s*)(async\s+)?function\s+\w+\s*\(/.exec(lines[i]);
    if (m !== null) heads.push({ start: i, indent: m[1].length });
  }
  const ranges = heads.map((head, idx) => {
    const next = heads.slice(idx + 1).find((other) => other.indent === head.indent);
    return { start: head.start, end: next === undefined ? lines.length - 1 : next.start - 1 };
  });
  const enclosingOf = (index) =>
    ranges
      .filter((range) => index >= range.start && index <= range.end)
      .sort((a, b) => a.end - a.start - (b.end - b.start))[0] ?? null;

  const unlinkAt = lines.findIndex((line) => /\bunlinkSync\s*\(/.test(line));
  if (unlinkAt < 0) {
    return verdict(name, SKIPPED, '没在实现里找到 unlink 调用——本条未执行，不判通过');
  }
  const fn = enclosingOf(unlinkAt);
  if (fn === null) {
    return verdict(name, SKIPPED, '没找到调用 unlink 的那个函数——本条未执行，不判通过');
  }
  const body = lines.slice(fn.start, fn.end + 1);
  const renameIndex = body.findIndex((line) => /\brenameSync\s*\(/.test(line));
  const unlinkIndex = body.findIndex((line) => /\bunlinkSync\s*\(/.test(line));
  const renameBeforeUnlink = renameIndex >= 0 && unlinkIndex >= 0 && renameIndex < unlinkIndex;
  // **unlink 的目标不是原路径**（原路径是函数的入参）：取 rename 的第二个实参、
  // unlink 的第一个实参，比对两者。
  const renameArgs = renameIndex >= 0 ? /\brenameSync\s*\(([^)]*)\)/.exec(body[renameIndex]) : null;
  const unlinkArgs = unlinkIndex >= 0 ? /\bunlinkSync\s*\(([^)]*)\)/.exec(body[unlinkIndex]) : null;
  const renameTarget = renameArgs === null ? '' : (renameArgs[1].split(',')[1] ?? '').trim();
  const unlinkTarget = unlinkArgs === null ? '' : (unlinkArgs[1].split(',')[0] ?? '').trim();
  const unlinkTargetsTombstone =
    renameTarget !== '' && unlinkTarget !== '' && renameTarget === unlinkTarget;

  // 行为部分：扫完**不留半截文件**。
  // **实验室也要用同一份副本**（活体注入打的是副本 ⇒ 静态与行为两侧都必须是它，
  // 否则「静态红了但行为还是真实现」⇒ 注入用例证不了任何东西）。
  const lab = await createSweepLab({ hostDir, tag: 'two-step' });
  try {
    const snap = lab.writeSnapshot({
      id: `${lab.provider}~a18twostep~9999`,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.expiredAt,
    });
    lab.sweeper().sweep();
    const rest = lab.leftovers(join('snapshots', lab.provider));
    const noLeftoverTombstone = !existsSync(snap) && rest.length === 0;
    return checkTwoStepDelete({
      renameBeforeUnlink,
      unlinkTargetsTombstone,
      noLeftoverTombstone,
      detail:
        `rename 目标=${renameTarget || '(未识别)'} / unlink 目标=${unlinkTarget || '(未识别)'}` +
        ` / 残留=${rest.join('、') || '（无）'}`,
    });
  } finally {
    lab.cleanup();
  }
}

/**
 * A18h｜§4.1 第 3 条：**根不可得就不扫**（不删任何东西、不抛）。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runRootUnavailable(hostDir) {
  const lab = await createSweepLab({ hostDir, tag: 'root-null' });
  try {
    const keeper = lab.writeSnapshot({
      id: `${lab.provider}~a18rootnull~9999`,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.expiredAt,
    });
    // **root 不可得的清理器**：根为 null（内核没给实例 home）。
    const noRoot = lab.sweepModule.createSweeper({
      root: null,
      providerId: lab.provider,
      now: () => lab.now,
    });
    let threw = false;
    let sweepResult = null;
    let maybeResult = null;
    try {
      sweepResult = noRoot.sweep();
      maybeResult = noRoot.maybeSweep();
    } catch {
      threw = true;
    }
    return checkRootUnavailable({
      skipped: sweepResult?.skipped === true,
      maybeSweepNotRan: maybeResult?.ran === false,
      noThrow: !threw,
      nothingElseTouched: existsSync(keeper),
    });
  } finally {
    lab.cleanup();
  }
}

/**
 * 制造一个「删不掉」的文件。
 *
 * ## Windows 上为什么**不能用只读**
 *
 * `chmod 444` 在 Windows 上**拦不住删除**——libuv 的 `fs__unlink` 会先把
 * `FILE_ATTRIBUTE_READONLY` 改回 `FILE_ATTRIBUTE_NORMAL` 再删（实测：只读文件
 * rename / unlink **都成功**）。⇒ 只读制造不出失败，本条会变成**恒「未执行」**。
 *
 * ## 办法①：ACL 拒绝删除（Windows）
 *
 * `icacls <file> /deny <user>:(D)` ⇒ rename / unlink 都 EPERM ⇒ 文件**原地不动**
 * （最贴近真实的「权限不足」）。
 *
 * ## 办法②（退路）：非空目录冒充文件
 *
 * 把那个路径换成**非空目录**：rename 成功、unlink 失败（EPERM）⇒ 内容被**改名**
 * 成墓碑（内容仍在磁盘上，只是改了名）——两步法删除下的另一种「删不掉」形态。
 *
 * @param {string} file - 目标文件。
 * @param {number} mtimeMs - 目标 mtime（过期时间）。
 * @returns {{mode: string, restore: () => void}|null} null = 造不出来。
 */
function makeUndeletable(file, mtimeMs) {
  if (process.platform === 'win32') {
    const user = process.env.USERNAME ?? process.env.USER ?? '';
    if (user !== '') {
      try {
        execFileSync('icacls', [file, '/deny', `${user}:(D)`], { stdio: 'ignore' });
      } catch {
        /* icacls 不可用 → 走退路 */
      }
      const probe = `${file}.probe`;
      try {
        renameSync(file, probe);
        renameSync(probe, file);
      } catch {
        // 改不动了 ⇒ 真的删不掉（ACL 生效）。
        return {
          mode: 'acl-deny-delete',
          restore: () => {
            try {
              execFileSync('icacls', [file, '/remove:d', user], { stdio: 'ignore' });
            } catch {
              /* 尽力而为 */
            }
          },
        };
      }
      // ACL 没生效 ⇒ 把它撤掉，别给退路留一颗雷。
      try {
        execFileSync('icacls', [file, '/remove:d', user], { stdio: 'ignore' });
      } catch {
        /* 尽力而为 */
      }
    }
  }
  // 退路：非空目录冒充。
  try {
    rmSync(file, { force: true });
    mkdirSync(file);
    writeFileSync(join(file, 'child.txt'), 'x');
    utimesSync(file, mtimeMs / 1000, mtimeMs / 1000);
    return { mode: 'non-empty-dir-masquerade', restore: () => {} };
  } catch {
    return null;
  }
}

/**
 * A18i｜§4.1 第 3 条：**清理失败只记日志，不阻塞业务**。
 *
 * 怎么制造「删不掉」：见 {@link makeUndeletable}（Windows 用 ACL 拒绝删除，
 * 退路用非空目录冒充）。删不掉的那一个**内容仍在磁盘上**，另一个过期文件**照删**——
 * 这就是「不阻塞业务」。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runSweepFailureLogged(hostDir) {
  const lab = await createSweepLab({ hostDir, tag: 'failure' });
  let restore = () => {};
  try {
    const stuckId = `${lab.provider}~a18stuck~9998`;
    const stuck = lab.writeSnapshot({
      id: stuckId,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.expiredAt,
    });
    const other = lab.writeSnapshot({
      id: `${lab.provider}~a18other~9999`,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.expiredAt,
    });

    // —— 制造「删不掉」——
    const made = makeUndeletable(stuck, lab.expiredAt);
    if (made === null) {
      return verdict(
        'A18i-sweep-failure-logged',
        SKIPPED,
        '本平台造不出「删不掉」——本条未执行，不判通过',
      );
    }
    restore = made.restore;

    const warnings = [];
    let threw = false;
    try {
      lab.sweeper({ logger: { warn: (text) => warnings.push(String(text)) } }).sweep();
    } catch {
      threw = true;
    }
    // **失败项的内容还在不在**：原路径**或**墓碑（两步法下 rename 成功、unlink
    // 失败 ⇒ 内容被改了名）——两种形态都算「没被假装删掉」。
    const tombstone = lab
      .leftovers(join('snapshots', lab.provider))
      .find((name) => name.startsWith(stuckId));
    const failedItemStillPresent = existsSync(stuck) || tombstone !== undefined;
    return checkSweepFailureLogged({
      noThrow: !threw,
      othersStillRemoved: !existsSync(other),
      failedItemStillPresent,
      failureLogged: warnings.length > 0,
    });
  } finally {
    restore();
    lab.cleanup();
  }
}

/**
 * A18j｜§4.1 第 2 条（PM 20:2x 澄清）：**`capturedAt` 优先，mtime 只兜底**。
 *
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runCapturedAtPriority(hostDir) {
  const lab = await createSweepLab({ hostDir, tag: 'captured-at' });
  try {
    // ① capturedAt 旧 + mtime 新 ⇒ 仍删
    const a = lab.writeSnapshot({
      id: `${lab.provider}~a18capold~9991`,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.now,
    });
    // ② capturedAt 新 + mtime 旧 ⇒ 不删
    const b = lab.writeSnapshot({
      id: `${lab.provider}~a18capnew~9992`,
      capturedAt: lab.now,
      mtimeAt: lab.expiredAt,
    });
    // ③ capturedAt 缺失 + mtime 旧 ⇒ 删（mtime 兜底）
    const cPath = join(
      lab.root,
      'snapshots',
      lab.provider,
      `${lab.provider}~a18capmiss~9993.json`,
    );
    lab.writeSnapshot({
      id: `${lab.provider}~a18capmiss~9993`,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.expiredAt,
    });
    const cRaw = JSON.parse(readFileSync(cPath, 'utf8'));
    delete cRaw.capturedAt;
    writeFileSync(cPath, `${JSON.stringify(cRaw, null, 2)}\n`, 'utf8');
    utimesSync(cPath, lab.expiredAt / 1000, lab.expiredAt / 1000);
    // ④ capturedAt 缺失 + mtime 新 ⇒ 不删
    const dPath = join(
      lab.root,
      'snapshots',
      lab.provider,
      `${lab.provider}~a18capmiss2~9994.json`,
    );
    lab.writeSnapshot({
      id: `${lab.provider}~a18capmiss2~9994`,
      capturedAt: lab.expiredAt,
      mtimeAt: lab.now,
    });
    const dRaw = JSON.parse(readFileSync(dPath, 'utf8'));
    delete dRaw.capturedAt;
    writeFileSync(dPath, `${JSON.stringify(dRaw, null, 2)}\n`, 'utf8');
    utimesSync(dPath, lab.now / 1000, lab.now / 1000);

    lab.sweeper().sweep();
    return checkCapturedAtPriority({
      oldCapturedNewMtimeRemoved: !existsSync(a),
      newCapturedOldMtimeKept: existsSync(b),
      missingCapturedOldMtimeRemoved: !existsSync(cPath),
      missingCapturedNewMtimeKept: existsSync(dPath),
    });
  } finally {
    lab.cleanup();
  }
}

/**
 * A17 的实跑：协议 §4.1 保留期主动清理（7 天）。
 *
 * **不碰实现文件**：超期快照/认领都用「真实实现产出的文件」**复制一份**改时间戳
 * 造出来；清理器直接调 `host/sweep.js` 的公开 API（`sweep()`）。
 *
 * 造五种文件，观测六个方向（**成对**：该清的清 / 不该清的留）：
 *   · `expired` 快照副本（capturedAt 超期）       → 期望**被清**
 *   · 原快照（未超期）                             → 期望**留着**（反向回归）
 *   · `keep`    认领（超期，但对应快照**还在**）    → 期望**留着**（删了会让序号二次分配）
 *   · `orphan`  认领（超期，对应快照**不存在**）    → 期望**被清**
 *   · `same`    认领（超期，对应 `expired` 快照）  → 同轮**一起清干净**（先快照后认领）
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runRetentionSweep(outerAdapter, options = {}) {
  // ## 这条**自建 adapter**，不复用外层那个
  //
  // **为什么**（实测，2026-10-20 21:0x）：外层 adapter 的 `withScope` 会**泄漏**
  // scoped 版 `commands`——它的 `finally` 只恢复 `store`/`client`/`scope`，
  // **既不恢复 `commands`，也不重新注册 exact route**（见
  // `scripts/lib/p1-adapter.mjs` 的 `withScope`）。于是 `withScope` 之后：
  //   · **写**（pick/settle/capture）落到 **scope 根**（scoped 版 commands）；
  //   · **观测**（`snapshotFile`）按**恢复后的默认 store 根**找。
  // ⇒ **两条路径分叉**：文件在 `<root>/scope/…/official-chat/snapshots/ds/`，
  // 观测口却在 `<root>/official-chat/snapshots/ds/` 找 ⇒ 这条判据永远「没落盘」。
  //
  // **那是 adapter 侧的状态污染，不是实现的缺口** ⇒ 判据不该把它算成「没落盘」，
  // 也不该由判据去改别人的文件（`p1-adapter.mjs` 正在被并行改动）⇒ 这条自己开
  // 一个**从没用过 `withScope`** 的实例，观测口与落盘根天然一致。
  //
  // **adapter 的缺陷要单独报给实现侧**（见群聊汇报），不在这儿悄悄绕过。
  //
  // `options.hostDir`：**活体注入用**——把 host 半边换成副本（注入打在副本上，
  // 原实现一个字节都不碰），用来证明这条判据**对实现的破坏敏感**。
  const { hostDir } = options;
  let adapter = null;
  try {
    adapter = await createAdapter({ tag: 'a17', hostDir });
  } catch (error) {
    return verdict(
      'A17-retention-sweep',
      SKIPPED,
      `adapter 创建失败，本条未执行：${String(error?.message ?? error)}`,
    );
  }
  try {
    return await runRetentionSweepInner(adapter, hostDir);
  } finally {
    adapter.cleanup();
  }
}

/**
 * A17 保留期清理的实跑（用**自建**的 adapter 实例）。
 *
 * @param {object} adapter
 * @param {string} [hostDir] - host 半边目录（活体注入用；不给就用仓库里那份）。
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function runRetentionSweepInner(adapter, hostDir) {
  const fs = await import('node:fs');
  const path = await import('node:path');
  // **实现模块跟着 hostDir 走**（活体注入时要用**副本**那份 `sweep.js`/`protocol.js`，
  // 否则注入的是副本、跑的却是原实现 ⇒ 判据会对破坏**不敏感**）。
  const hostBase = hostDir
    ? pathToFileURL(`${hostDir}/`)
    : new URL('../plugins/official-chat/host/', import.meta.url);
  const sweepModule = await import(new URL('sweep.js', hostBase).href);
  const protocolModule = await import(new URL('protocol.js', hostBase).href);
  const retentionMs = protocolModule.CONSTANTS.snapshotRetentionMs;
  const homeRoot = adapter.root;
  const DAY = 24 * 60 * 60 * 1000;
  const now = Date.now();
  const expiredAt = now - retentionMs - DAY;

  const picked = await adapter.pick({ sessionId: 'p1-a17' });
  const ref = String(picked?.ref ?? '');
  if (ref === '') {
    return verdict('A17-retention-sweep', SKIPPED, 'pick 没拿到 ref——本条未执行');
  }
  await adapter.settle(ref);
  // **等落盘**：`settle` 等的是「读取落定」，落盘是**另一条异步链**——两者不是
  // 同时发生的，别假设 pick 回来文件就一定在盘上。
  const waitForSnapshot = async (id, ms) => {
    const until = Date.now() + ms;
    let file = adapter.snapshotFile(id);
    while (!file.exists && Date.now() < until) {
      await new Promise((resolve) => setTimeout(resolve, 50));
      file = adapter.snapshotFile(id);
    }
    return file;
  };
  let fresh = await waitForSnapshot(ref, 2000);
  let how = '客户端链路自动落盘';
  if (!fresh.exists) {
    // 兜底：走**真实 host 命令**显式落盘（id 是 pick 时 host 认领过的，合法）。
    await adapter.callHost('capture', {
      sessionId: String(picked.sessionId ?? 'p1-a17'),
      snapshotId: ref,
      requestId: 'p1-a17-capture',
    });
    fresh = await waitForSnapshot(ref, 3000);
    how = '显式 host capture 落盘';
  }
  if (!fresh.exists) {
    // **诊断**：只列**关键事实**（期望路径 + 盘上同名文件在哪），别把整棵目录树
    // dump 进 detail（上一版 dump 了 100+ 行，验收人根本读不下去）。
    const found = [];
    const findByName = (dir, depth = 0) => {
      if (depth > 8 || found.length >= 5) return;
      let entries = [];
      try {
        entries = fs.readdirSync(dir, { withFileTypes: true });
      } catch {
        return;
      }
      for (const entry of entries) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) findByName(full, depth + 1);
        else if (entry.name.includes(ref)) found.push(path.relative(homeRoot, full));
      }
    };
    findByName(homeRoot);
    let captureResult = '';
    try {
      captureResult = JSON.stringify(
        await adapter.callHost('capture', {
          sessionId: String(picked.sessionId ?? 'p1-a17'),
          snapshotId: ref,
          requestId: 'p1-a17-capture-diag',
        }),
      ).slice(0, 200);
    } catch (error) {
      captureResult = `<capture 抛错：${String(error?.message ?? error)}>`;
    }
    return verdict(
      'A17-retention-sweep',
      SKIPPED,
      `快照没落盘——本条未执行｜ref=${ref}｜期望路径=${fresh.path}｜` +
        `盘上同名文件=${found.length > 0 ? found.join('、') : '（无）'}｜capture=${captureResult}`,
    );
  }
  const parts = ref.split('~');
  const provider = parts[0] ?? 'ds';
  // **存储根**（= `<DSH_HOME>/official-chat`）：**从落盘路径反推**，不假设 env 解析
  // （`resolveSnapshotRoot` 优先用 `DSH_XLINK_OFFICIAL_CHAT_ROOT` 覆盖项）。
  //
  // 落盘布局是 `<存储根>/snapshots/<provider>/<id>.json` ⇒ **上溯三级**才是存储根。
  //
  // **这里踩过坑**：`host/sweep.js` 的 `root` 语义是**存储根**
  // （`join(root,'snapshots',providerId)` / `join(root,'seq-claims',providerId)`），
  // 而 `adapter.root` 是 **DSH_HOME**——直接拿它当 sweeper 的 root 会**少一层**
  // ⇒ 认领文件扫不到、快照也扫不到 ⇒ 判据永远「未执行」。
  const storeRoot = path.dirname(path.dirname(path.dirname(fresh.path)));

  // ① 超期快照副本。
  const expiredId = `${provider}~${parts[1]}~9999`;
  const expiredSnapPath = path.join(path.dirname(fresh.path), `${expiredId}.json`);
  const expiredSnap = { ...(fresh.snapshot ?? {}), snapshotId: expiredId, capturedAt: expiredAt };
  fs.writeFileSync(expiredSnapPath, JSON.stringify(expiredSnap), 'utf8');
  fs.utimesSync(expiredSnapPath, expiredAt / 1000, expiredAt / 1000);

  // ② 认领文件：扫出真实的那份（真实实现产的），复制三份改内容造三种情形。
  const claimsRoot = path.join(storeRoot, 'seq-claims', provider);

  const claimFiles = [];
  const walkClaims = (dir) => {
    let entries = [];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walkClaims(full);
      else if (entry.name.endsWith('.json')) claimFiles.push(full);
    }
  };
  walkClaims(claimsRoot);
  if (claimFiles.length === 0) {
    return verdict(
      'A17-retention-sweep',
      SKIPPED,
      `没扫到认领文件（${claimsRoot}）——本条未执行`,
    );
  }
  let baseClaim = {};
  try {
    baseClaim = JSON.parse(fs.readFileSync(claimFiles[0], 'utf8'));
  } catch {
    baseClaim = {};
  }
  const writeClaim = (name, seq, claimedAt) => {
    const target = path.join(path.dirname(claimFiles[0]), name);
    const obj = {
      ...baseClaim,
      providerShort: provider,
      sessionShort: parts[1],
      seq,
      snapshotId: `${provider}~${parts[1]}~${seq}`,
      claimedAt,
    };
    fs.writeFileSync(target, JSON.stringify(obj), 'utf8');
    fs.utimesSync(target, claimedAt / 1000, claimedAt / 1000);
    return target;
  };
  // keep：对应**原快照**（还在、未超期）→ 协议要求认领**留着**。
  const claimKeep = writeClaim('keep-seq.json', parts[2] ?? '1', expiredAt);
  // orphan：对应快照**不存在** → 期望被清。
  const claimOrphan = writeClaim('orphan-seq.json', '9998', expiredAt);
  // same：对应 ① 的超期快照 → 同轮应与它一起清干净。
  const claimSame = writeClaim('same-seq.json', '9999', expiredAt);

  // ③ 跑清理器（公开 API，**不碰实现文件**）。
  // **root 必须是存储根**（`host/sweep.js` 扫 `join(root,'snapshots',providerId)`
  // 与 `join(root,'seq-claims',providerId)`），不是 DSH_HOME。
  const sweeper = sweepModule.createSweeper({ root: storeRoot, retentionMs, providerId: provider, now: () => now });
  const swept = sweeper.sweep();

  const exists = (file) => fs.existsSync(file);
  const observation = {
    expiredSnapshotRemoved: !exists(expiredSnapPath),
    freshSnapshotKept: exists(fresh.path),
    expiredClaimRemoved: !exists(claimOrphan),
    claimOfLiveSnapshotKept: exists(claimKeep),
    sameRoundBothRemoved: !exists(expiredSnapPath) && !exists(claimSame),
    afterSweepRejected: false,
  };
  // ④ 清掉的快照再读 → 必须拒（preview 走 host 命令层，直接读盘）。
  const preview = await adapter.callHost('preview', {
    sessionId: String(picked.sessionId ?? 'p1-a17'),
    snapshotId: expiredId,
    requestId: 'p1-a17-preview',
  });
  const previewText = JSON.stringify(preview ?? {});
  observation.afterSweepRejected =
    /snapshot_invalid/.test(previewText) && !/"ok"\s*:\s*true/.test(previewText);

  const result = checkRetentionSweep(observation);
  if (result.status === PASS) {
    return verdict(
      'A17-retention-sweep',
      PASS,
      `${result.detail}（${how}；sweep：${JSON.stringify(swept).slice(0, 200)}）`,
    );
  }
  return verdict(
    'A17-retention-sweep',
    result.status,
    `${result.detail}｜观测=${JSON.stringify(observation)}｜sweep=${JSON.stringify(swept).slice(0, 200)}` +
      `｜preview=${previewText.slice(0, 200)}｜落盘方式=${how}`,
  );
}

/**
 * 跑仓库级隔离门禁（协议 §8-11：`probe-profile-isolation` / `probe-data-directory` 仍绿）。
 *
 * **判据自身的两个坑（已修）**：
 *   1. 门禁输出里**没有** `probe-profile-isolation` / `probe-data-directory`
 *      这些分区名——它们是**失败分区名**，成功时打的是中文文案（`✓ 测试/探针代码
 *      没有指向用户真实数据目录下的浏览器 profile（已扫 N 个文件）`）。原来按
 *      分区名去 grep，**永远找不到** → 永远走「未找到但退出码为 0」的 PASS 分支
 *      ⇒ 这条判据**从来没真正验证过那两行**，是个假绿。
 *   2. 现在按**稳定的中文文案片段**匹配，且**找不到就报「未执行」**而不是通过
 *      ——「没测到」与「测了没过」必须能分开。
 *
 * @returns {{name: string, status: string, detail: string}}
 */
function checkRepoIsolationGate() {
  try {
    const out = execFileSync(process.execPath, [join(REPO, 'scripts', 'check-invariants.mjs')], {
      cwd: REPO,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    });
    // 判据 30 与判据 31 的成功文案片段（两个都要找到才算这条判据真的跑到了）。
    const markers = [
      '没有指向用户真实数据目录下的浏览器 profile',
      '都无条件给了 data_directory',
    ];
    const found = markers.filter((marker) => out.includes(marker));
    if (found.length < markers.length) {
      return verdict(
        'A11b-repo-isolation-gate',
        SKIPPED,
        `门禁输出里只找到 ${found.length}/${markers.length} 条判据 30/31 的成功文案` +
          '（输出格式可能变了）——本条**未执行**，不判通过',
      );
    }
    const lines = out
      .split('\n')
      .filter((line) => markers.some((marker) => line.includes(marker)))
      .map((l) => l.trim().replace(/^✓\s*/, ''));
    return verdict('A11b-repo-isolation-gate', PASS, `仓库级门禁绿：${lines.join('；')}`);
  } catch (error) {
    const output = `${error?.stdout ?? ''}${error?.stderr ?? ''}`.trim();
    return verdict('A11b-repo-isolation-gate', FAIL, `仓库级门禁红了：${output.slice(0, 800)}`);
  }
}

/**
 * JS 端跑同一组夹具（协议 §6 的「各跑一遍」之一）。
 *
 * @param {Array<object>} fixtures
 * @returns {Promise<Array<{id: string, result: unknown}>>}
 */
async function runJsFixtures(fixtures) {
  const protocol = await import(
    new URL('../plugins/official-chat/host/protocol.js', import.meta.url).href
  );
  if (typeof protocol.resolveCase !== 'function') {
    return undefined;
  }
  return fixtures.map((fixture) => ({
    id: String(fixture?.id ?? ''),
    result: protocol.resolveCase(fixture?.input ?? {}),
  }));
}

/**
 * Rust 端跑同一组夹具（`src-tauri/examples/p1_protocol_fixture.rs` 的产物）。
 *
 * @returns {Array<{id: string, result: unknown}>|undefined}
 */
function runRustFixtures() {
  return runRustPayload()?.cases;
}

/**
 * Rust 端 runner 的**完整产物**（夹具结果 + 两端常量 + 两端码表）。
 *
 * 协议 §6：「`schemaVersion` **两端常量必须一致**」——只跑夹具结果的话这条
 * 没人验，所以 example 一并输出常量与码表（`src-tauri/examples/p1_protocol_fixture.rs`）。
 *
 * @returns {{schemaVersion?: number, constants?: object, errorCodes?: object, cases?: Array}|undefined}
 */
function runRustPayload() {
  const exe = process.platform === 'win32' ? 'p1_protocol_fixture.exe' : 'p1_protocol_fixture';
  const bin = join(REPO, 'src-tauri', 'target', 'debug', 'examples', exe);
  if (!existsSync(bin)) return undefined;
  try {
    const stdout = execFileSync(bin, [], {
      cwd: REPO,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    const parsed = JSON.parse(stdout.trim());
    return parsed !== null && typeof parsed === 'object' ? parsed : undefined;
  } catch {
    return undefined;
  }
}

async function main() {
  const args = process.argv.slice(2);
  const roundIndex = args.indexOf('--round');
  const round = roundIndex >= 0 ? args[roundIndex + 1] : `r${Date.now().toString(36)}`;
  const outDir = join(tmpdir(), 'dsh-p1-protocol');
  mkdirSync(outDir, { recursive: true });

  const results = [];

  // —— 验收项 13：通道可达性（A0s 静态 + A0 真机）——
  const fixtures = readFixtures();
  // **先报「没测到的」**：真机级通道可达性当前不可机检，放最前面，别让它藏在绿里。
  results.push(checkChannelReachability());
  // A0s（仓库级静态）：host 半边**注册了 exact fetch route**（`/api` 命名空间下）
  // + 不把 `rpc.handle` 当已通 + 不用 cordis `ctx.set` 挂服务。
  // **要扫两个文件**：`index.js` 是注册处，`rpc.js` 是**路径常量与信封层**
  // （`/api/xlink-official-chat` 住在 `rpc.js`）。
  const hostDir = join(REPO, 'plugins', 'official-chat', 'host');
  const hostFiles = ['index.js', 'rpc.js'].map((name) => {
    const path = join(hostDir, name);
    return {
      file: `plugins/official-chat/host/${name}`,
      text: existsSync(path) ? readFileSync(path, 'utf8') : '',
    };
  });
  results.push(checkHostInjection({ files: hostFiles }));
  results.push(checkFixtureCoverage(fixtures));
  results.push(checkFixtureExpectations(fixtures));
  // §1.1（v0.2）：`captureResult` 的形态（夹具层）。
  results.push(checkCaptureResultShape(fixtures));
  const jsFixtures = await runJsFixtures(fixtures);
  const rustPayload = runRustPayload();
  const rustFixtures = rustPayload?.cases;
  results.push(checkFixtureParity({ js: jsFixtures, rust: rustFixtures }));
  // 夹具的 `expected` 就是契约：实现算出来的结果必须与它一致。
  // **两端各判一条、名字必须带端标识**——同名两条会让验收人分不清是哪端不一致。
  results.push(
    checkFixtureExpectationMatch(
      {
        expected: fixtures.map((fixture) => ({ id: String(fixture?.id ?? ''), result: fixture?.expected })),
        actual: jsFixtures,
      },
      'A1d-fixture-expectation-match-js',
    ),
  );
  results.push(
    checkFixtureExpectationMatch(
      {
        expected: fixtures.map((fixture) => ({ id: String(fixture?.id ?? ''), result: fixture?.expected })),
        actual: rustFixtures,
      },
      'A1d-fixture-expectation-match-rust',
    ),
  );

  // —— 判据 11：隔离面（本次新增文件）+ 仓库级门禁 ——
  const newFiles = NEW_FILES.filter((file) => existsSync(join(REPO, file))).map((file) => ({
    file,
    text: readFileSync(join(REPO, file), 'utf8'),
  }));
  results.push(
    checkIsolationSurface({
      files: newFiles,
      findProfileViolationsInFiles,
      findDataDirectoryViolationsInFiles,
    }),
  );
  results.push(checkRepoIsolationGate());

  /**
   * 跑一条判据，**崩了不许带走整轮**。
   *
   * 一崩就把**别的**判据结果一起盖住了，验收人就看不到别的了——
   * 所以单条抛错只把它自己报成「未执行」，并写明原因。
   *
   * @param {string} name - 判据名。
   * @param {() => Promise<object>|object} run - 实跑。
   * @returns {Promise<object>} 判据结果。
   */
  const runSafely = async (name, run) => {
    try {
      return await run();
    } catch (error) {
      return verdict(name, SKIPPED, `实跑抛错，本条未执行：${String(error?.message ?? error)}`);
    }
  };

  // —— A18：协议 §4.1「保留期主动清理」的**剩余**口径（v0.4）——
  //
  // **为什么放在这里（不在行为判据那一段）**：A18a 是**静态**的，A18c–A18j
  // **自建一次性夹具**（不依赖客户端模块）⇒ 客户端模块加载失败时它们**照跑**，
  // 别被一起盖住。只有 A18b（触发点的**行为**观测）要用适配器。
  //
  // A17（保留期清理六方向）**保持不动**——它判的是「清没清干净」，
  // A18 判的是「怎么触发、什么时候不触发、失败怎么办、时间戳谁优先」。
  results.push(await runSafely('A18a-sweep-trigger-static', () => runSweepTriggerStatic()));
  results.push(await runSafely('A18c-sweep-throttle', () => runSweepThrottle()));
  results.push(await runSafely('A18d-cache-ttl', () => runCacheTtl()));
  results.push(await runSafely('A18e-log-truncate', () => runLogTruncation()));
  results.push(await runSafely('A18f-log-not-age-deleted', () => runLogNotAgeDeleted()));
  results.push(await runSafely('A18g-two-step-delete', () => runTwoStepDelete()));
  results.push(await runSafely('A18h-root-unavailable', () => runRootUnavailable()));
  results.push(await runSafely('A18i-sweep-failure-logged', () => runSweepFailureLogged()));
  results.push(await runSafely('A18j-captured-at-priority', () => runCapturedAtPriority()));
  // A18k：§4.1 第 4 条（「正在发送中的引用」不保护）**不可机检** ⇒
  // 如实报「已知不覆盖」，**不假装能测**；后果由 S-5 / S-6 覆盖。
  results.push(checkInflightReferenceNotCovered());

  // —— 行为判据：对真实实现跑 ——
  //
  // **客户端模块加载失败时**（实现正在并行改动、处于中间态），
  // 行为判据一律报「未执行」并写明原因——**不能整轮崩掉**：
  // 一崩就把静态判据的结果也一起盖住了，验收人就看不到别的了。
  const BEHAVIOUR_CHECKS = [
    ['A9c-error-no-internal-leak', checkErrorNoInternalLeak],
    ['A9d-error-log-path', checkErrorLogPath],
    ['A13a-seq-cross-process', checkSeqCrossProcess],
    ['A13b-host-rejects-unclaimed-id', checkHostRejectsUnclaimedId],
    ['A14-error-code-closed-set', checkErrorCodeClosedSet],
    ['A15-fingerprint-gate-host-layer', checkFingerprintGate],
    ['A16b-capture-result-shape-landed', checkCaptureResultLanded],
    ['A2-pick-twice-distinct', checkPickTwiceDistinct],
    ['A3-ref-length-boundary', checkRefLengthBoundary],
    ['A4-single-source-of-truth', checkSingleSourceOfTruth],
    ['A5-request-matches-disk', checkRequestMatchesDisk],
    ['A6a-invalid-reject', checkInvalidRejection],
    ['A6b-chip-invalid-node', checkChipInvalidNode],
    ['A7-cross-process', checkCrossProcess],
    ['A8-no-body-dedup', checkNoBodyDedup],
    ['A10-namespace-isolation', checkNamespaceIsolation],
    // §4.1：保留期主动清理（7 天）。协议 v0.4 新增的实现，判据本轮才补上。
    ['A17-retention-sweep', runRetentionSweep],
    // §4.1 第 1 条（**行为**）：清理由 `catalog` 触发、`capture` 不触发。
    // 观测口是适配器里的**替身 sweeper**（只记调用、不真扫）。
    ['A18b-sweep-trigger-behaviour', runSweepTriggerBehaviour],
  ];

  let adapter = null;
  let adapterError = '';
  try {
    adapter = await createAdapter({ tag: `run-${round}` });
  } catch (error) {
    adapterError = String(error?.message ?? error);
  }

  if (adapter === null) {
    for (const [name] of BEHAVIOUR_CHECKS) {
      results.push(
        verdict(name, SKIPPED, `客户端模块加载失败，本条未执行：${adapterError}`),
      );
    }
  } else {
    try {
      const capabilities = adapter.capabilities();
      results.push(checkErrorCodeTable(capabilities));
      results.push(checkCatalogCodesSeparated(capabilities));
      results.push(checkConstants(capabilities));
      results.push(checkGapRegistry(capabilities));
      // §6：「`schemaVersion` **两端常量必须一致**」——夹具结果里没有常量，
      // 这条单独比（JS 的 capabilities vs Rust runner 吐出来的常量/码表）。
      results.push(
        checkConstantsParity({
          js: {
            schemaVersion: capabilities?.schemaVersion,
            constants: capabilities?.constants,
            errorCodes: capabilities?.errorCodes,
          },
          rust: {
            schemaVersion: rustPayload?.schemaVersion,
            constants: rustPayload?.constants,
            errorCodes: rustPayload?.errorCodes,
          },
        }),
      );

      for (const [name, check] of BEHAVIOUR_CHECKS) {
        // **单条抛错不许带走整轮**（同上的理由）。
        results.push(await runSafely(name, () => check(adapter)));
      }
    } finally {
      adapter.cleanup();
    }
  }

  const summary = summarize(results);
  const payload = {
    round,
    at: new Date().toISOString(),
    fixtureDir: FIXTURE_DIR,
    results,
    summary,
  };
  const outPath = join(outDir, `results-${round}.json`);
  writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, 'utf8');

  for (const item of results) {
    const mark = item.status === PASS ? '✔' : item.status === FAIL ? '✖' : '–';
    process.stdout.write(`${mark} [${item.status}] ${item.name}\n    ${item.detail}\n`);
  }
  process.stdout.write(
    `\nsummary: total=${summary.total} pass=${summary.pass} fail=${summary.fail} skipped=${summary.skipped}\n` +
      `证据：${outPath}\n`,
  );
  process.exitCode = summary.fail > 0 ? 1 : 0;
}

// **导出**：这些实跑函数要能被自测**import** 去做活体注入
// （往真实实现的副本里注入破坏，证明这条判据对实现的破坏**敏感**）。
// ⇒ 用 `import.meta.main` 守卫，`import` 本模块时**不跑**整轮。
export {
  runRetentionSweep,
  runSweepTriggerStatic,
  runSweepTriggerBehaviour,
  runSweepThrottle,
  runCacheTtl,
  runLogTruncation,
  runLogNotAgeDeleted,
  runTwoStepDelete,
  runRootUnavailable,
  runSweepFailureLogged,
  runCapturedAtPriority,
};

if (import.meta.main) {
  await main();
}
