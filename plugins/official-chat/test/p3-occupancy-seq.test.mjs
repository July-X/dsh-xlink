// P3 第二批单②（协议 §S-11 ＋ §S-12 ＋ §S-13 裁定 4 / G3-1）的自测：
// **占用记录 `refs/` ＋ 序号水位**。
//
// ## 判什么
//
// · **§S-11 占用记录**：capture 成功 ⇒ `<root>/refs/<snapshotId>.json` 落盘，
//   字段**恰为**四个（`snapshotId`/`capturedAt`/`createdAt`/`sessionId`），
//   **不含正文、不含摘要**；**先写占用记录、再对外宣告「已保存」**。
// · **R1 显式释放**：host 侧显式调用 ⇒ 删占用记录。
// · **R2 兜底释放**：占用记录过了保留期（7 天）⇒ 视为失效并删（**清理流程负责**）。
// · **R3 拒绝释放**：占用记录**写坏** ⇒ **不删**，按被占着处理。
// · **回收只处理未引用记录**：被占用的快照**不删**；无占用的过期快照照删。
// · **§S-12 恢复顺序**：**先重建占用关系（扫 `refs/`）再回收**（次序可观测）。
// · **§S-13 裁定 4**：水位**先于**认领落盘；认领被删后水位**不倒退**、
//   再分配拿到**更大**的 `seq`；到上限报**既有的**「序号已用尽」。
//
// ## 不判什么
//
// · **内核草稿的真实持有关系**（上游缺口 G1-8：内核只「问」source、从不「告诉」
//   source）⇒ 本批只保证「回收时不误删」（§S-12 的降级口径）。
// · **client 侧的移除 UI**（本批**不接线**，那是 P4/UX 的活）。
// · **占用计数**（不引入引用计数：同一快照引用两次**仍是一条**）。

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { createCatalogCache, createCommands, createFixtureSource } from '../host/commands.js';
import { CONSTANTS, WATERMARK_FILE_NAME, makeSessionShort } from '../host/protocol.js';
import { createRefsStore } from '../host/refs-store.js';
import { createSeqAllocator } from '../host/seq-store.js';
import { createSweeper } from '../host/sweep.js';

const NOW = Date.parse('2026-10-21T09:00:00.000Z');
const HOUR = 60 * 60 * 1000;
const DAY = 24 * HOUR;
const BODY = 'P3 单②自测正文。';

/** 造一个临时 home（**绝不指向用户真实数据目录**）。 */
function makeHome() {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-p3b-'));
  return { dir, env: { DSH_HOME: dir } };
}

/** 落盘根（存储根 ＝ `<DSH_HOME>/official-chat`）。 */
function rootOf(home) {
  return join(home.dir, 'official-chat');
}

/** 官网会话 id（固定 ⇒ 会话短码固定）。 */
const SESSION_ID = 'p3b-session-alpha';

/**
 * 造一套**真命令层**（真落盘 store、真分配器、真夹具来源）。
 *
 * **为什么要走真链路**：`capture` 要求 id 的会话段对得上、且那个 `seq`
 * **已被 host 认领** ⇒ 不能自造 id ⇒ 必须先跑一次 `catalog` 让 host 分配。
 */
function buildLab(home, overrides = {}) {
  const root = rootOf(home);
  const written = [];
  const store = {
    root,
    save: (snapshot) => {
      written.push(snapshot);
      mkdirSync(join(root, 'snapshots', 'ds'), { recursive: true });
      writeFileSync(
        join(root, 'snapshots', 'ds', `${snapshot.snapshotId}.json`),
        `${JSON.stringify(snapshot, null, 2)}\n`,
        'utf8',
      );
    },
    load: (id) => {
      const found = written.find((one) => one.snapshotId === String(id));
      if (found === undefined) {
        const error = new Error(`快照不存在：${String(id)}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      return { snapshot: found, body: found.messages.map((m) => m.text).join('\n\n') };
    },
    listSessionSeqs: () => ({}),
  };
  const seqAllocator = createSeqAllocator({ root, providerId: 'ds' });
  const commands = createCommands({
    store,
    catalogCache: createCatalogCache({ rootDir: join(root, 'catalog-cache') }),
    captureSource: createFixtureSource([
      { sessionId: SESSION_ID, title: 'P3 单②自测会话', body: BODY },
    ]),
    seqAllocator,
    providerId: 'ds',
    accountScope: 'p3b/self-test-account',
    now: () => NOW,
    ...overrides,
  });
  return { root, store, seqAllocator, commands, written };
}

/** 走真实链路拿一个可 capture 的快照 id。 */
async function allocateId(commands) {
  const page = await commands.catalog({ requestId: 'p3b-allocate', query: '' });
  const id = page?.allocations?.[SESSION_ID];
  assert.equal(typeof id, 'string', 'catalog 应当为会话分配出快照 id');
  assert.notEqual(id, '', '分配到的 id 不能是空串');
  return id;
}

/** 在**子进程**里跑一段脚本（脚本里可以先 patch `node:fs` 再动态 import 实现）。 */
function runInChild(script) {
  try {
    const stdout = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      encoding: 'utf8',
      timeout: 60_000,
    });
    return JSON.parse(stdout.trim().split('\n').pop() ?? 'null');
  } catch (error) {
    return { error: String(error?.stderr ?? error?.message ?? error) };
  }
}

/** 子进程脚本的公共前导：patch CJS `node:fs` 的写盘/读盘原语并记录调用序列。 */
function tracePreamble() {
  return [
    "import { createRequire } from 'node:module';",
    "const require = createRequire('file:///' + process.cwd().replace(/\\\\/g, '/') + '/');",
    "const fs = require('node:fs');",
    'const TRACE = [];',
    'const realWrite = fs.writeFileSync;',
    'fs.writeFileSync = function (p, d, o) {',
    "  TRACE.push({ kind: 'write', path: String(p) });",
    '  return realWrite.call(fs, p, d, o);',
    '};',
    'const realRead = fs.readFileSync;',
    'fs.readFileSync = function (p, ...rest) {',
    "  TRACE.push({ kind: 'readfile', path: String(p) });",
    '  return realRead.call(fs, p, ...rest);',
    '};',
    'const realReaddir = fs.readdirSync;',
    'fs.readdirSync = function (d, ...rest) {',
    "  TRACE.push({ kind: 'readdir', path: String(d) });",
    '  return realReaddir.call(fs, d, ...rest);',
    '};',
    'const realRename = fs.renameSync;',
    'fs.renameSync = function (f, t, ...rest) {',
    "  TRACE.push({ kind: 'rename', path: String(f), to: String(t) });",
    '  return realRename.call(fs, f, t, ...rest);',
    '};',
    'const realUnlink = fs.unlinkSync;',
    'fs.unlinkSync = function (p, ...rest) {',
    "  TRACE.push({ kind: 'unlink', path: String(p) });",
    '  return realUnlink.call(fs, p, ...rest);',
    '};',
  ].join('\n');
}

// ---------------------------------------------------------------------------
// §S-11｜占用记录
// ---------------------------------------------------------------------------

test('§S-11：capture 成功 ⇒ 占用记录落盘，字段恰为四个（不含正文、不含摘要）', async () => {
  const home = makeHome();
  const lab = buildLab(home);
  const id = await allocateId(lab.commands);
  const result = await lab.commands.capture({ sessionId: SESSION_ID, snapshotId: id, requestId: 'p3b-cap' });
  assert.equal(result.ok, true, 'capture 应当成功');
  const path = join(lab.root, 'refs', `${id}.json`);
  assert.ok(existsSync(path), `占用记录应当落在 ${path}`);
  const raw = readFileSync(path, 'utf8');
  const parsed = JSON.parse(raw);
  assert.deepEqual(
    Object.keys(parsed).sort(),
    ['capturedAt', 'createdAt', 'sessionId', 'snapshotId'],
    '占用记录**只含**这四个字段（多一个就是形态不合）',
  );
  assert.equal(parsed.snapshotId, id);
  assert.equal(parsed.sessionId, SESSION_ID);
  assert.equal(parsed.capturedAt, result.snapshot.capturedAt, '占用记录记的是那份快照的捕获时刻');
  assert.equal(raw.includes(BODY), false, '占用记录**不含正文**');
  assert.equal(/contentDigest/.test(raw), false, '占用记录**不含摘要**');
});

test('§S-11：先写占用记录、再对外宣告「已保存」（快照先落，占用记录后写）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const commandsUrl = new URL('../host/commands.js', import.meta.url).href;
  const seqUrl = new URL('../host/seq-store.js', import.meta.url).href;
  // **在子进程里 patch `node:fs`**（同进程 patch 不生效：`node:fs` 的 ESM facade
  // 在首次 import 时就快照了 CJS 导出值）⇒ 观测得到**真实的调用次序**。
  //
  // **为什么必须绕这一圈**：实现侧**不留可注入的口子**（占用记录的落点是
  // 实现内部的事）⇒ 次序只能靠**观测真实写盘**来断，不能靠「塞一个替身进去」。
  const script = [
    "import { join } from 'node:path';",
    tracePreamble(),
    `const { createCommands, createCatalogCache, createFixtureSource } = await import(${JSON.stringify(commandsUrl)});`,
    `const { createSeqAllocator } = await import(${JSON.stringify(seqUrl)});`,
    `const root = ${JSON.stringify(root)};`,
    `const SESSION = ${JSON.stringify(SESSION_ID)};`,
    'const written = [];',
    'const store = {',
    '  root,',
    '  save: (snapshot) => {',
    '    written.push(snapshot);',
    "    fs.mkdirSync(join(root, 'snapshots', 'ds'), { recursive: true });",
    "    fs.writeFileSync(join(root, 'snapshots', 'ds', snapshot.snapshotId + '.json'), JSON.stringify(snapshot, null, 2) + '\\n', 'utf8');",
    '  },',
    '  load: (id) => {',
    '    const found = written.find((one) => one.snapshotId === String(id));',
    '    if (found === undefined) { const error = new Error("快照不存在"); error.code = "snapshot_invalid"; throw error; }',
    "    return { snapshot: found, body: found.messages.map((m) => m.text).join('\\n\\n') };",
    '  },',
    '  listSessionSeqs: () => ({}),',
    '};',
    'const commands = createCommands({',
    '  store,',
    "  catalogCache: createCatalogCache({ rootDir: join(root, 'catalog-cache') }),",
    `  captureSource: createFixtureSource([{ sessionId: SESSION, title: 'P3 单②自测会话', body: ${JSON.stringify(BODY)} }]),`,
    "  seqAllocator: createSeqAllocator({ root, providerId: 'ds' }),",
    "  providerId: 'ds',",
    "  accountScope: 'p3b/self-test-account',",
    `  now: () => ${NOW},`,
    '});',
    "const page = await commands.catalog({ requestId: 'p3b-allocate', query: '' });",
    'const id = page?.allocations?.[SESSION];',
    'const refsPath = join(root, "refs", id + ".json");',
    'let refsOnDiskAtDelivery = null;',
    // **在结果交付的那一刻**检查占用记录在不在盘上（`then` 回调跑在结果
    // resolve 之后、我们 await 之前）⇒ 断的正是「先写占用记录、再宣告已保存」。
    "const result = await commands.capture({ sessionId: SESSION, snapshotId: id, requestId: 'p3b-order' }).then((value) => {",
    '  refsOnDiskAtDelivery = fs.existsSync(refsPath);',
    '  return value;',
    '});',
    'process.stdout.write(JSON.stringify({ trace: TRACE, ok: result?.ok === true, id, refsOnDiskAtDelivery }));',
  ].join('\n');
  const result = runInChild(script);
  assert.ok(Array.isArray(result?.trace), `子进程应当回传调用序列：${JSON.stringify(result)}`);
  assert.equal(result?.ok, true, 'capture 应当成功');
  assert.equal(
    result?.refsOnDiskAtDelivery,
    true,
    '结果交付的那一刻，占用记录必须**已经在盘上**（先写占用记录、再对外宣告「已保存」）',
  );
  const snapshotAt = result.trace.findIndex(
    (item) => item.kind === 'write' && item.path === join(root, 'snapshots', 'ds', `${result.id}.json`),
  );
  const refsAt = result.trace.findIndex(
    (item) => item.kind === 'write' && item.path === join(root, 'refs', `${result.id}.json`),
  );
  assert.ok(snapshotAt >= 0, '快照必须真的落到盘上');
  assert.ok(refsAt >= 0, '占用记录必须真的写到盘上');
  assert.ok(
    snapshotAt < refsAt,
    `占用记录必须在快照落盘**之后**写（快照在第 ${snapshotAt} 次写、占用记录在第 ${refsAt} 次写）`,
  );
});

test('§S-11：占用记录写不进去 ⇒ 不宣告「已保存」（refs 路径被占成目录）', async () => {
  const home = makeHome();
  const lab = buildLab(home);
  // **把 refs 目录占成「同名文件」** ⇒ 建目录会失败 ⇒ 占用记录写不进去。
  mkdirSync(lab.root, { recursive: true });
  writeFileSync(join(lab.root, 'refs'), 'not-a-directory', 'utf8');
  const id = await allocateId(lab.commands);
  const result = await lab.commands.capture({ sessionId: SESSION_ID, snapshotId: id, requestId: 'p3b-block' });
  assert.equal(result.ok, false, '占用记录没落 ⇒ 不许宣告「已保存」');
  assert.equal(result.code, 'snapshot_invalid', '失败要带协议内的稳定码（错误负载是扁平的）');
  assert.equal(
    existsSync(join(lab.root, 'snapshots', 'ds', `${id}.json`)),
    true,
    '快照本身已经发布（发布点在 save 内部）——这条只断「不宣告已保存」',
  );
});

test('§S-11 R1：显式释放入口删掉占用记录（`releaseOccupancy` 与 `release` 同一个入口）', async () => {
  const home = makeHome();
  const lab = buildLab(home);
  const id = await allocateId(lab.commands);
  await lab.commands.capture({ sessionId: SESSION_ID, snapshotId: id, requestId: 'p3b-rel' });
  const path = join(lab.root, 'refs', `${id}.json`);
  assert.ok(existsSync(path), '释放前占用记录在盘上');
  const released = lab.commands.releaseOccupancy({ snapshotId: id });
  assert.equal(released.ok, true);
  assert.equal(released.released, true, '确实删掉了');
  assert.equal(existsSync(path), false, 'R1：显式释放 ⇒ 占用记录没了');
  // **别名**：同一个入口的另一个名字。
  const again = lab.commands.release(id);
  assert.equal(again.ok, true);
  assert.equal(again.released, false, '已经没有记录了 ⇒ 如实报「没释放」，不假装');
});

test('§S-11 R2：占用记录过了保留期 ⇒ 兜底释放（由清理流程负责，不是释放入口）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const sweeper = createSweeper({ root, providerId: 'ds', now: () => NOW });
  const refsDir = join(root, 'refs');
  mkdirSync(refsDir, { recursive: true });
  // ① 过期占用记录（7 天前）⇒ 该释放。
  const old = '2026-10-01T00:00:00.000Z';
  writeFileSync(
    join(refsDir, 'ds~aaaa~0001.json'),
    `${JSON.stringify({ snapshotId: 'ds~aaaa~0001', capturedAt: old, createdAt: old, sessionId: 's' })}\n`,
    'utf8',
  );
  // ② 未过期占用记录 ⇒ 不该动。
  const fresh = new Date(NOW - HOUR).toISOString();
  writeFileSync(
    join(refsDir, 'ds~aaaa~0002.json'),
    `${JSON.stringify({ snapshotId: 'ds~aaaa~0002', capturedAt: fresh, createdAt: fresh, sessionId: 's' })}\n`,
    'utf8',
  );
  const result = sweeper.sweep();
  assert.equal(result.refs.scanned, 2, '两条占用记录都要被扫到');
  assert.equal(result.refs.removed, 1, '只释放过期那一条');
  assert.equal(existsSync(join(refsDir, 'ds~aaaa~0001.json')), false, '过期的占用记录被释放');
  assert.equal(existsSync(join(refsDir, 'ds~aaaa~0002.json')), true, '未过期的占用记录留着');
  assert.equal(CONSTANTS.occupancyRetentionMs, 7 * DAY, '占用保留期 7 天（协议 §4）');
});

test('§S-11 R3：占用记录写坏 ⇒ 不删，且它占着的快照也不删', () => {
  const home = makeHome();
  const root = rootOf(home);
  const snapshotsDir = join(root, 'snapshots', 'ds');
  mkdirSync(snapshotsDir, { recursive: true });
  // 一份**过期**的快照（有占用记录 ⇒ 按 R3 仍算被占着）。
  const id = 'ds~aaaa~0001';
  const expiredAt = new Date(NOW - 8 * DAY).toISOString();
  writeFileSync(
    join(snapshotsDir, `${id}.json`),
    `${JSON.stringify({ schemaVersion: 1, snapshotId: id, capturedAt: expiredAt })}\n`,
    'utf8',
  );
  // 一份**写坏**的占用记录（读不动 ⇒ R3 ⇒ 按被占着处理）。
  const refsDir = join(root, 'refs');
  mkdirSync(refsDir, { recursive: true });
  writeFileSync(join(refsDir, `${id}.json`), '{"snapshotId": "ds~aaaa~0001", ', 'utf8');
  const sweeper = createSweeper({ root, providerId: 'ds', now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(existsSync(join(refsDir, `${id}.json`)), true, 'R3：读不出的占用记录不删');
  assert.equal(existsSync(join(snapshotsDir, `${id}.json`)), true, 'R3：按被占着处理 ⇒ 快照不删');
  assert.equal(result.snapshots.removed, 0, '这一轮不该删掉任何快照');
});

test('§S-11：回收只处理未引用记录 —— 有占用记录 ⇒ 快照不删；无占用记录 ⇒ 照删', () => {
  const home = makeHome();
  const root = rootOf(home);
  const snapshotsDir = join(root, 'snapshots', 'ds');
  mkdirSync(snapshotsDir, { recursive: true });
  const expiredAt = new Date(NOW - 8 * DAY).toISOString();
  const write = (id) =>
    writeFileSync(
      join(snapshotsDir, `${id}.json`),
      `${JSON.stringify({ schemaVersion: 1, snapshotId: id, capturedAt: expiredAt })}\n`,
      'utf8',
    );
  const held = 'ds~aaaa~0001';
  const free = 'ds~aaaa~0002';
  write(held);
  write(free);
  // 被占着的那一份有占用记录（未过期）。
  const refsDir = join(root, 'refs');
  mkdirSync(refsDir, { recursive: true });
  const fresh = new Date(NOW - HOUR).toISOString();
  writeFileSync(
    join(refsDir, `${held}.json`),
    `${JSON.stringify({ snapshotId: held, capturedAt: fresh, createdAt: fresh, sessionId: 's' })}\n`,
    'utf8',
  );
  const sweeper = createSweeper({ root, providerId: 'ds', now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(existsSync(join(snapshotsDir, `${held}.json`)), true, '被占用的快照不删');
  assert.equal(existsSync(join(snapshotsDir, `${free}.json`)), false, '无占用的过期快照照删');
  assert.equal(result.snapshots.removed, 1, '只删了没被占着的那一份');
});

test('§S-11③：占用记录**自己也过期**时，这一轮的回收仍按「本轮开始时的占用关系」判（下一轮起才可回收）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const snapshotsDir = join(root, 'snapshots', 'ds');
  mkdirSync(snapshotsDir, { recursive: true });
  const id = 'ds~aaaa~0001';
  const expiredAt = new Date(NOW - 8 * DAY).toISOString();
  writeFileSync(
    join(snapshotsDir, `${id}.json`),
    `${JSON.stringify({ schemaVersion: 1, snapshotId: id, capturedAt: expiredAt })}\n`,
    'utf8',
  );
  // 占用记录**自己也过期**（7 天前）。
  const refsDir = join(root, 'refs');
  mkdirSync(refsDir, { recursive: true });
  const old = new Date(NOW - 8 * DAY).toISOString();
  writeFileSync(
    join(refsDir, `${id}.json`),
    `${JSON.stringify({ snapshotId: id, capturedAt: old, createdAt: old, sessionId: 's' })}\n`,
    'utf8',
  );
  const sweeper = createSweeper({ root, providerId: 'ds', now: () => NOW });
  // **第一轮**：占用关系在这一轮开始时是**存在**的 ⇒ 快照**不删**；
  // 而那条占用记录**过了保留期** ⇒ 被 R2 释放掉。
  const first = sweeper.sweep();
  assert.equal(existsSync(join(snapshotsDir, `${id}.json`)), true, '这一轮：占用记录还在 ⇒ 快照不删');
  assert.equal(existsSync(join(refsDir, `${id}.json`)), false, '这一轮：过期的占用记录被 R2 释放');
  assert.equal(first.refs.removed, 1, '释放了那条过期的占用记录');
  // **第二轮**：占用关系已经没了 ⇒ 快照这回收得掉。
  sweeper.sweep();
  assert.equal(existsSync(join(snapshotsDir, `${id}.json`)), false, '下一轮起：没有占用记录 ⇒ 过期快照被回收');
});

test('§S-11：同一快照被引用两次 ⇒ 仍是一条占用记录（不引入引用计数）', async () => {
  const home = makeHome();
  const root = rootOf(home);
  const refs = createRefsStore({ root, now: () => NOW });
  const id = 'ds~aaaa~0001';
  refs.write({ snapshotId: id, capturedAt: '2026-10-21T00:00:00.000Z', createdAt: '2026-10-21T00:00:00.000Z', sessionId: 's' });
  refs.write({ snapshotId: id, capturedAt: '2026-10-21T00:00:00.000Z', createdAt: '2026-10-21T00:00:00.000Z', sessionId: 's' });
  const names = readdirSync(join(root, 'refs'));
  assert.deepEqual(names, [`${id}.json`], '两次引用 ⇒ 仍是一条占用记录');
});

test('§S-12：先重建占用关系、再回收（扫 `refs/` 的次序在删快照之前）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const snapshotsDir = join(root, 'snapshots', 'ds');
  mkdirSync(snapshotsDir, { recursive: true });
  const expiredAt = new Date(NOW - 8 * DAY).toISOString();
  writeFileSync(
    join(snapshotsDir, 'ds~aaaa~0001.json'),
    `${JSON.stringify({ schemaVersion: 1, snapshotId: 'ds~aaaa~0001', capturedAt: expiredAt })}\n`,
    'utf8',
  );
  const sweepUrl = new URL('../host/sweep.js', import.meta.url).href;
  // **在子进程里 patch `node:fs`**（同进程 patch 不生效：`node:fs` 的 ESM facade
  // 在首次 import 时就快照了 CJS 导出值）⇒ 观测得到**真实的调用次序**。
  const script = [
    tracePreamble(),
    `const { createSweeper } = await import(${JSON.stringify(sweepUrl)});`,
    `const sweeper = createSweeper({ root: ${JSON.stringify(root)}, providerId: 'ds', now: () => ${NOW} });`,
    'sweeper.sweep();',
    'process.stdout.write(JSON.stringify(TRACE));',
  ].join('\n');
  const trace = runInChild(script);
  assert.ok(Array.isArray(trace), `子进程应当回传调用序列：${JSON.stringify(trace)}`);
  const scanAt = trace.findIndex(
    (item) => item.kind === 'readdir' && item.path.includes('refs'),
  );
  const deleteAt = trace.findIndex(
    (item) =>
      (item.kind === 'rename' || item.kind === 'unlink') && item.path.includes('snapshots'),
  );
  assert.ok(scanAt >= 0, '清理过程里必须真的读过一次 `refs/` 目录（占用关系的全量扫描）');
  assert.ok(deleteAt >= 0, '这一轮应当真的删了那份过期快照（否则次序没被观测到）');
  assert.ok(
    scanAt < deleteAt,
    `占用关系的全量扫描必须在任何快照删除之前（扫描在第 ${scanAt} 次调用、删除在第 ${deleteAt} 次调用）`,
  );
});

// ---------------------------------------------------------------------------
// §S-13 裁定 4｜序号水位
// ---------------------------------------------------------------------------

test('§S-13 裁定 4：水位写入**早于**认领写入（不留「认领已建、水位未写」的窗口）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const seqUrl = new URL('../host/seq-store.js', import.meta.url).href;
  const script = [
    tracePreamble(),
    `const { createSeqAllocator } = await import(${JSON.stringify(seqUrl)});`,
    `const allocator = createSeqAllocator({ root: ${JSON.stringify(root)}, providerId: 'ds' });`,
    "const r = allocator.reserve('p3b-seq-session');",
    'process.stdout.write(JSON.stringify({ trace: TRACE, seq: r.seq }));',
  ].join('\n');
  const result = runInChild(script);
  assert.ok(Array.isArray(result?.trace), `子进程应当回传写盘次序：${JSON.stringify(result)}`);
  // **「水位在盘上」的时刻**有两种落法（两种都算）：
  // · **直接写**目标文件（旧写法：原地截断写）⇒ 写的那一刻就在盘上；
  // · **原子落盘**（先写同目录临时文件、再 `rename` 覆盖）⇒ `rename` 的那一刻
  //   才在盘上 ⇒ ⇒ 认的是 **`rename` 的落点**，不是临时文件的名字。
  //
  // ⚠️ 这里认的是**语义事件**（「水位已经生效」），不是某一种实现写法 ⇒
  // 实现从「原地写」换成「临时文件 + rename」时，这条判据**不许**跟着红。
  // （**原子性本身**由 `seq-store.test.mjs` 那条「并发写时读者不许读到半个
  // 文件」单独钉住 —— 那条才是不许回退的实现判据。）
  const watermarkAt = result.trace.findIndex(
    (item) =>
      (item.kind === 'write' && item.path.endsWith(WATERMARK_FILE_NAME)) ||
      (item.kind === 'rename' && String(item.to ?? '').endsWith(WATERMARK_FILE_NAME)),
  );
  const claimAt = result.trace.findIndex(
    (item) => item.kind === 'write' && /[\\/]\d{4}\.json$/.test(item.path),
  );
  assert.ok(watermarkAt >= 0, `水位记录必须真的写了（路径：${WATERMARK_FILE_NAME}）`);
  assert.ok(claimAt >= 0, '认领文件必须真的写了');
  assert.ok(
    watermarkAt < claimAt,
    `水位写入（第 ${watermarkAt} 次）必须早于认领写入（第 ${claimAt} 次）`,
  );
});

test('§S-13 裁定 4：认领被删后水位不倒退，再分配拿到更大的 `seq`', () => {
  const home = makeHome();
  const root = rootOf(home);
  // **冻结同一个时钟**：认领的 `claimedAt` 与水位的 `updatedAt` 都落在 NOW。
  const allocator = createSeqAllocator({ root, providerId: 'ds', now: () => NOW });
  const first = allocator.reserve('p3b-seq-session');
  assert.equal(first.seq, 1);
  const sessionShort = makeSessionShort('p3b-seq-session');
  const claimPath = join(root, 'seq-claims', 'ds', sessionShort, '0001.json');
  const watermarkPath = join(root, 'seq-claims', 'ds', sessionShort, WATERMARK_FILE_NAME);
  assert.ok(existsSync(claimPath), '认领文件在盘上');
  assert.ok(existsSync(watermarkPath), '水位记录在盘上');
  // **认领被删**（保留期清理会这么做：已过期 ＋ 快照不在）⇒ 水位**必须还在、
  // 且不倒退**。⇒ 清理器的时钟往前推 9 天（超过 7 天保留期）。
  const sweeper = createSweeper({ root, providerId: 'ds', now: () => NOW + 9 * DAY });
  sweeper.sweep();
  assert.equal(existsSync(claimPath), false, '认领确实被清掉了（已过期 ＋ 快照不在）');
  assert.equal(existsSync(watermarkPath), true, '水位记录**不得**被清理流程删掉');
  // **水位不倒退** ⇒ 再分配拿到**更大**的号。
  const after = createSeqAllocator({ root, providerId: 'ds', now: () => NOW }).reserve('p3b-seq-session');
  assert.ok(after.seq > first.seq, `再分配必须拿到更大的 seq（旧 ${first.seq} → 新 ${after.seq}）`);
  assert.notEqual(after.snapshotId, first.snapshotId, '同一个 snapshotId 不得再指向另一份内容');
});

test('§S-13 裁定 4：到上限报**既有的**「序号已用尽」（码与文案都不换）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const allocator = createSeqAllocator({ root, providerId: 'ds' });
  allocator.reserve('p3b-seq-session');
  const sessionShort = makeSessionShort('p3b-seq-session');
  // **把水位顶到上限**（水位是实现方自己的格式 ⇒ 这一步由实现自己写）。
  const watermarkPath = join(root, 'seq-claims', 'ds', sessionShort, WATERMARK_FILE_NAME);
  const current = JSON.parse(readFileSync(watermarkPath, 'utf8'));
  writeFileSync(
    watermarkPath,
    `${JSON.stringify({ ...current, seq: 9999 })}\n`,
    'utf8',
  );
  assert.throws(
    () => createSeqAllocator({ root, providerId: 'ds' }).reserve('p3b-seq-session'),
    (error) => {
      assert.equal(error?.code, 'source_unsupported', '沿用既有的错误码，不许新造');
      assert.match(String(error?.message ?? ''), /序号已用尽/, '沿用既有的文案');
      return true;
    },
  );
});

test('§S-13 裁定 4：水位到上限时**认领目录全满**也报同一句（跨进程抢号全撞上）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const allocator = createSeqAllocator({ root, providerId: 'ds' });
  allocator.reserve('p3b-seq-session');
  const sessionShort = makeSessionShort('p3b-seq-session');
  const claimsDir = join(root, 'seq-claims', 'ds', sessionShort);
  const seqUrl = new URL('../host/seq-store.js', import.meta.url).href;
  // **在子进程里把认领目录补成满 range**：水位之外的号**全被占着**
  // ⇒ 分配器一路往后试 ⇒ 最后撞上限（这是「跨进程抢号」的真实形状）。
  const script = [
    tracePreamble(),
    // **注意**：前导已经把 `fs.readdirSync` 换过了 ⇒ 这里接着换（变量名别撞）。
    'const patchedReaddir = fs.readdirSync;',
    `const CLAIMS_DIR = ${JSON.stringify(claimsDir)};`,
    'fs.readdirSync = function (dir, o) {',
    '  if (String(dir) === CLAIMS_DIR) {',
    '    return Array.from({ length: 9999 }, (_, i) => String(i + 1).padStart(4, "0") + ".json");',
    '  }',
    '  return patchedReaddir.call(fs, dir, o);',
    '};',
    `const { createSeqAllocator } = await import(${JSON.stringify(seqUrl)});`,
    `const allocator = createSeqAllocator({ root: ${JSON.stringify(root)}, providerId: 'ds' });`,
    'let code = ""; let message = "";',
    'try { allocator.reserve("p3b-seq-session"); } catch (error) { code = String(error?.code ?? ""); message = String(error?.message ?? ""); }',
    'process.stdout.write(JSON.stringify({ code, message }));',
  ].join('\n');
  const result = runInChild(script);
  assert.equal(result?.code, 'source_unsupported', '沿用既有的错误码');
  assert.match(String(result?.message ?? ''), /序号已用尽/, '沿用既有的文案');
});

test('§S-13 裁定 4：水位读不出来 ⇒ 拒绝分配（宁可停发也不复用序号）', () => {
  const home = makeHome();
  const root = rootOf(home);
  const allocator = createSeqAllocator({ root, providerId: 'ds' });
  allocator.reserve('p3b-seq-session');
  const sessionShort = makeSessionShort('p3b-seq-session');
  const watermarkPath = join(root, 'seq-claims', 'ds', sessionShort, WATERMARK_FILE_NAME);
  writeFileSync(watermarkPath, '{ 这不是 JSON', 'utf8');
  assert.throws(
    () => createSeqAllocator({ root, providerId: 'ds' }).reserve('p3b-seq-session'),
    (error) => error?.code === 'snapshot_invalid',
    '水位不明 ⇒ 拒绝分配（不许猜着发）',
  );
});
