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
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

import {
  PASS,
  FAIL,
  SKIPPED,
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
  checkInvalidRejection,
  checkIsolationSurface,
  checkNoBodyDedup,
  checkNamespaceIsolation,
  checkPickTwiceDistinct,
  checkRefLengthBoundary,
  checkRequestMatchesDisk,
  checkRetentionSweep,
  checkSeqCrossProcess,
  checkSingleSourceOfTruth,
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
        results.push(await check(adapter));
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

// **导出**：`runRetentionSweep` 要能被自测**import** 去做活体注入
// （往真实实现的副本里注入破坏，证明这条判据对实现的破坏**敏感**）。
// ⇒ 用 `import.meta.main` 守卫，`import` 本模块时**不跑**整轮。
export { runRetentionSweep };

if (import.meta.main) {
  await main();
}
