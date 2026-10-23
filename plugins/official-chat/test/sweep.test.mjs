// **保留期主动清理**（协议 §4.1，v0.4 裁定）的判据。
//
// ## 判什么
//
// · 过期快照被删、没过期的**留着**（按文件内的 `capturedAt`，mtime 只兜底）；
// · **序号认领**：过期**且对应快照已经不在**才删（快照还在时删认领 ⇒ 序号被二次
//   分配 ⇒ 撞上仍存在的快照 id）；
// · 目录缓存按 TTL 删；错误日志按行数截断（**不按 7 天删**——日志是诊断证据）；
// · **只动本实例根下那四家族**；根不可得 ⇒ **不扫**；
// · **同实例至多一天一次**（节流键 `<root>/sweeps/last-sweep-<日期>.json`）；
// · 触发点**只在 `catalog` 入口**（`capture` 不触发）。
//
// ## 判据不替真机跑
//
// 这里证的是「清理的形状与取舍对不对」。真机上清理跑在**惰性触发**里
// （插件不常驻，定时不可靠），那部分由真机复验轮与使用观察覆盖。

import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';

import { createCommands } from '../host/commands.js';
import { CONSTANTS } from '../host/protocol.js';
import { LOG_MAX_LINES, createSweeper } from '../host/sweep.js';

const DAY = 24 * 60 * 60 * 1000;
const NOW = Date.parse('2026-10-20T12:00:00.000Z');

test('清理过期正文临时文件及删除残留，保留新文件和不认识的文件', (t) => {
  const root = makeRoot();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const dir = join(root, 'snapshots', 'ds');
  mkdirSync(dir, { recursive: true });
  const old = [
    '.ds~abcdefghijkl~0001.body.txt.tmp-999-1',
    'ds~abcdefghijkl~0001.body.txt.expired-1',
    'ds~abcdefghijkl~0001.json.expired-1',
  ];
  const keep = ['ds~abcdefghijkl~0002.body.txt.expired-2', 'private-not-a-snapshot.json.expired-1'];
  for (const name of [...old, ...keep]) {
    const path = join(dir, name);
    writeFileSync(path, 'PUBLIC TEST BODY');
    const time = name === keep[0] ? NOW : NOW - 2 * DAY;
    utimesSync(path, time / 1000, time / 1000);
  }
  createSweeper({ root, now: () => NOW }).sweep();
  for (const name of old) assert.equal(existsSync(join(dir, name)), false, name);
  for (const name of keep) assert.equal(existsSync(join(dir, name)), true, name);
});

test('快照改名失败时保留原正文，解除故障后两者均可回收', (t) => {
  const root = makeRoot();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const id = 'ds~abcdefghijkl~0001';
  writeSnapshot(root, id, new Date(NOW - 10 * DAY).toISOString());
  const json = join(root, 'snapshots', 'ds', `${id}.json`);
  const body = join(root, 'snapshots', 'ds', `${id}.body.txt`);
  writeFileSync(body, 'PUBLIC TEST BODY');
  const rename = fs.renameSync;
  const sweeper = createSweeper({ root, now: () => NOW });
  try {
    fs.renameSync = (from, to) => {
      if (from === json) throw Object.assign(new Error('injected sharing failure'), { code: 'EPERM' });
      return rename(from, to);
    };
    syncBuiltinESMExports();
    sweeper.sweep();
    assert.equal(existsSync(json), true);
    assert.equal(existsSync(body), true);
  } finally {
    fs.renameSync = rename;
    syncBuiltinESMExports();
  }
  sweeper.sweep();
  assert.equal(existsSync(json), false);
  assert.equal(existsSync(body), false);
});

/** 造一个一次性根目录（**绝不指向用户真实数据目录**）。 */
function makeRoot() {
  return mkdtempSync(join(tmpdir(), 'official-chat-sweep-'));
}

/**
 * 写一份快照文件。
 *
 * @param {string} root - 根目录。
 * @param {string} id - 快照 id。
 * @param {string} capturedAt - 捕获时间（ISO）。
 */
function writeSnapshot(root, id, capturedAt) {
  const dir = join(root, 'snapshots', 'ds');
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, `${id}.json`),
    `${JSON.stringify({ snapshotId: id, capturedAt, messages: [] }, null, 2)}\n`,
    'utf8',
  );
}

/**
 * 写一个序号认领文件。
 *
 * @param {string} root - 根目录。
 * @param {string} sessionShort - 会话短码。
 * @param {number} seq - 序号。
 * @param {string} claimedAt - 认领时间（ISO）。
 */
function writeClaim(root, sessionShort, seq, claimedAt) {
  const dir = join(root, 'seq-claims', 'ds', sessionShort);
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, `${String(seq).padStart(4, '0')}.json`),
    `${JSON.stringify({ providerShort: 'ds', sessionShort, seq, claimedAt }, null, 2)}\n`,
    'utf8',
  );
}

test('过期快照被删、没过期的留着（按文件内的 capturedAt，不看 mtime）', () => {
  const root = makeRoot();
  writeSnapshot(root, 'ds~aaaa~0001', new Date(NOW - 8 * DAY).toISOString());
  writeSnapshot(root, 'ds~aaaa~0002', new Date(NOW - 1 * DAY).toISOString());
  const sweeper = createSweeper({ root, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.skipped, false);
  assert.equal(result.snapshots.removed, 1, '只该删那一份过期的');
  assert.equal(existsSync(join(root, 'snapshots', 'ds', 'ds~aaaa~0001.json')), false);
  assert.equal(existsSync(join(root, 'snapshots', 'ds', 'ds~aaaa~0002.json')), true);
});

test('**正文文件跟快照一起走**（它是快照的投影 ⇒ 快照没了它就是孤儿）', () => {
  // **为什么**：正文文件里是**用户私有内容**（模型读的那一份）⇒ ⇒ 快照被回收
  // 而它留着 ⇒ 就是**多漏一份用户内容** ⇒ ⇒ 必须一起删。
  const root = makeRoot();
  writeSnapshot(root, 'ds~aaaa~0001', new Date(NOW - 8 * DAY).toISOString());
  writeSnapshot(root, 'ds~aaaa~0002', new Date(NOW - 1 * DAY).toISOString());
  const dir = join(root, 'snapshots', 'ds');
  writeFileSync(join(dir, 'ds~aaaa~0001.body.txt'), '旧正文', 'utf8');
  writeFileSync(join(dir, 'ds~aaaa~0002.body.txt'), '没过期的正文', 'utf8');
  const sweeper = createSweeper({ root, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.snapshots.removed, 1);
  assert.equal(existsSync(join(dir, 'ds~aaaa~0001.body.txt')), false, '过期快照的正文文件要一起删');
  assert.equal(existsSync(join(dir, 'ds~aaaa~0002.body.txt')), true, '没过期的正文文件留着');
});

test('序号认领：过期 **且** 对应快照已不在才删；快照还在 ⇒ 认领留着', () => {  const root = makeRoot();
  // 认领 0001：**认领过期**但**快照还没过期**（还在）⇒ 不删。
  //   删了会让那个序号被二次分配 ⇒ 撞上仍然存在的快照 id。
  writeClaim(root, 'aaaa', 1, new Date(NOW - 8 * DAY).toISOString());
  writeSnapshot(root, 'ds~aaaa~0001', new Date(NOW - 1 * DAY).toISOString());
  // 认领 0002：过期**且**快照已经不在 ⇒ 删。
  writeClaim(root, 'aaaa', 2, new Date(NOW - 8 * DAY).toISOString());
  // 认领 0003：认领自己没过期的 ⇒ 留着（根本不看快照）。
  writeClaim(root, 'aaaa', 3, new Date(NOW - 1 * DAY).toISOString());

  const sweeper = createSweeper({ root, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.claims.removed, 1, '只该删「过期且快照已不在」的那一个');
  assert.equal(existsSync(join(root, 'seq-claims', 'ds', 'aaaa', '0001.json')), true);
  assert.equal(existsSync(join(root, 'seq-claims', 'ds', 'aaaa', '0002.json')), false);
  assert.equal(existsSync(join(root, 'seq-claims', 'ds', 'aaaa', '0003.json')), true);
});

test('顺序**先快照后认领**：同一轮里「过期快照 + 过期认领」一起清掉', () => {
  const root = makeRoot();
  // 认领 0004 与它的快照**都过期**。
  // 协议 §4.1 第 3 条要求「先快照后认领」（认领先删会指向**仍存在但已过期**的
  // 快照 ⇒ 那个序号会被二次分配 ⇒ 撞上仍存在的快照 id）⇒ 快照先被删，
  // 认领随后满足「过期且快照已不在」⇒ **同轮一起清干净**。
  writeClaim(root, 'aaaa', 4, new Date(NOW - 8 * DAY).toISOString());
  writeSnapshot(root, 'ds~aaaa~0004', new Date(NOW - 8 * DAY).toISOString());

  const sweeper = createSweeper({ root, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.snapshots.removed, 1, '过期快照要删');
  assert.equal(result.claims.removed, 1, '它的过期认领同轮也要删');
  assert.equal(existsSync(join(root, 'snapshots', 'ds', 'ds~aaaa~0004.json')), false);
  assert.equal(existsSync(join(root, 'seq-claims', 'ds', 'aaaa', '0004.json')), false);
});

test('目录缓存按 TTL 删；错误日志按行数截断（**不按 7 天删**）', () => {
  const root = makeRoot();
  const cacheDir = join(root, 'catalog-cache');
  mkdirSync(cacheDir, { recursive: true });
  writeFileSync(join(cacheDir, 'old.json'), '{}', 'utf8');
  const logDir = join(root, 'logs');
  mkdirSync(logDir, { recursive: true });
  const lines = [];
  for (let i = 0; i < LOG_MAX_LINES + 50; i += 1) lines.push(`第 ${i} 行`);
  writeFileSync(join(logDir, 'official-chat.log'), `${lines.join('\n')}\n`, 'utf8');

  // **缓存文件的 mtime 要真的老**（缓存不看文件内容，只看 mtime + TTL）。
  const old = new Date(NOW - 10 * 60 * 1000);
  utimesSync(join(cacheDir, 'old.json'), old, old);

  const sweeper = createSweeper({ root, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.cache.removed, 1, '过期缓存要删');
  assert.equal(existsSync(join(cacheDir, 'old.json')), false);
  assert.equal(result.log.truncated, true, '超行数的日志要截断');
  const kept = readFileSync(join(logDir, 'official-chat.log'), 'utf8');
  assert.ok(kept.includes(`第 ${LOG_MAX_LINES + 49} 行`), '截断要留尾部');
  assert.ok(!kept.includes('第 0 行\n'), '截断要丢掉头部');
});

test('根不可得 ⇒ **不扫**（宁可不清理，不乱删）', () => {
  const sweeper = createSweeper({ root: null, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.skipped, true);
  assert.match(result.reason, /不扫/);
  const lazy = sweeper.maybeSweep();
  assert.equal(lazy.ran, false);
});

test('同实例至多一天一次：第二次不跑，节流键落在 `<root>/sweeps/`', () => {
  const root = makeRoot();
  writeSnapshot(root, 'ds~aaaa~0001', new Date(NOW - 8 * DAY).toISOString());
  const sweeper = createSweeper({ root, now: () => NOW });
  const first = sweeper.maybeSweep();
  assert.equal(first.ran, true, '当天第一次要真跑');
  assert.equal(existsSync(join(root, 'snapshots', 'ds', 'ds~aaaa~0001.json')), false);
  const second = sweeper.maybeSweep();
  assert.equal(second.ran, false, '当天第二次不跑');
  assert.match(second.reason, /已经扫过/);
  const stamp = join(root, 'sweeps', 'last-sweep-2026-10-20.json');
  assert.equal(existsSync(stamp), true, '节流键落在 `<root>/sweeps/`');
});

test('触发点**只在 `catalog` 入口**（协议 §4.1 第 1 条）', async () => {
  const root = makeRoot();
  writeSnapshot(root, 'ds~aaaa~0001', new Date(NOW - 8 * DAY).toISOString());
  const sweeper = createSweeper({ root, now: () => NOW });
  const store = {
    save: () => {},
    load: () => {
      throw new Error('不该被读');
    },
    listSessionSeqs: () => ({}),
  };
  const commands = createCommands({
    store,
    catalogCache: { read: () => null, write: () => {} },
    captureSource: { list: async () => ({ ok: true, items: [] }), fetch: async () => ({ ok: true }) },
    sweeper,
    providerId: 'ds',
    accountScope: 'desktop-dev/default/site-A',
    now: () => NOW,
  });
  await commands.catalog({ query: '' });
  assert.equal(
    existsSync(join(root, 'snapshots', 'ds', 'ds~aaaa~0001.json')),
    false,
    'catalog 入口要触发清理',
  );
});

test('`capture` **不触发**清理（用户主动路径更短更可控）', async () => {
  const root = makeRoot();
  const expired = join(root, 'snapshots', 'ds', 'ds~aaaa~0001.json');
  writeSnapshot(root, 'ds~aaaa~0001', new Date(NOW - 8 * DAY).toISOString());
  const sweeper = createSweeper({ root, now: () => NOW });
  const commands = createCommands({
    store: { save: () => {}, load: () => ({}), listSessionSeqs: () => ({}) },
    catalogCache: { read: () => null, write: () => {} },
    captureSource: { list: async () => ({ ok: true, items: [] }), fetch: async () => ({ ok: true }) },
    sweeper,
    providerId: 'ds',
    accountScope: 'desktop-dev/default/site-A',
    now: () => NOW,
  });
  await commands.capture({ sessionId: 's', snapshotId: 'ds~aaaa~0001' });
  assert.equal(existsSync(expired), true, 'capture 不该触发清理');
});

test('保留期就是 §4 的 `snapshotRetentionMs`（7 天）', () => {
  assert.equal(CONSTANTS.snapshotRetentionMs, 7 * DAY);
});
