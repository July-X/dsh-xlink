// P3 第二批单①（协议 §S-8 ＋ §S-9 ＋ §S-10）的自测：**原子发布链**。
//
// ## 判什么
//
// · **§S-8 原子发布**：写**临时名** → **回读校验** → `rename`；发布出去的快照，
//   盘上 `contentDigest` ＝ **读回后重算**的值（不是内存里那份）。
// · **§S-9 发布点 ＝ `rename` 成功那一刻**：临时名**不进清单、不占序号**；
//   目录里只允许有最终名。
// · **§S-10 第五家族**：超过阈值的临时残留被清；阈值内的、不是临时名形态的
//   **不删**；删除路径**不越出缓存根**；删不掉**只记日志、不抛**。
// · **不覆盖**（P1 §6「同 id 不重写」）：同 id 再写一次 ⇒ `snapshot_exists`，
//   且**原文件逐字未变**。
// · **`schemaVersion` 不变**（§4）：落盘**方式**改了，落**什么**没改 ⇒ 旧快照
//   照旧可读。
//
// ## 不判什么
//
// · 「写到一半进程死掉」「写坏内容」的**端到端**形状：那需要能在写临时文件的
//   **过程中**注入失败 ⇒ 那是**注入点**，按纪律**交 PM 裁定**（见回执）。
//   这里用**纯函数口径**（`snapshotReadBackMatches`）把「不一致 ⇒ 不发布」
//   这条**判定本身**断掉。
// · 真机上的目录布局：这里只判 Node 落盘那一层。

import assert from 'node:assert/strict';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  SNAPSHOT_SCHEMA_VERSION,
  bodyOf,
  createSnapshotStore,
  snapshotReadBackMatches,
} from '../host/index.js';
import { CONSTANTS, contentDigest, isTempSnapshotName, tempSnapshotName } from '../host/protocol.js';
import { createSweeper } from '../host/sweep.js';

const NOW = Date.parse('2026-10-21T09:00:00.000Z');
const HOUR = 60 * 60 * 1000;

/** 造一个临时 home（**绝不指向用户真实数据目录**）。 */
function makeHome() {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-official-chat-p3-'));
  return { dir, env: { DSH_HOME: dir } };
}

/** 快照目录（存储根 ＝ `<DSH_HOME>/official-chat`，再按 provider 分目录）。 */
function snapshotsDir(home) {
  return join(home.dir, 'official-chat', 'snapshots', 'ds');
}

/** 一份合法的快照。 */
function snapshot(overrides = {}) {
  return {
    schemaVersion: SNAPSHOT_SCHEMA_VERSION,
    snapshotId: 'ds~abcdefghijkl~0001',
    providerId: 'ds',
    accountScope: '',
    sessionId: 'session-1',
    branch: '',
    messages: [{ role: 'user', text: '第一行。\n第二行。' }],
    captureResult: 'ok',
    capturedAt: '2026-10-21T00:00:00.000Z',
    sourceUpdatedAt: '',
    sourceFingerprint: '',
    ...overrides,
  };
}

test('§S-8／§S-9：发布出去的快照在最终名下，临时名不留在盘上，盘上摘要 ＝ 读回重算的值', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  const saved = store.save(snapshot());
  const names = readdirSync(snapshotsDir(home));
  assert.deepEqual(
    names.sort(),
    // ⚠️ **正文文件也是最终名**（模型读的那一份，PM 2026-10-22 21:5x 裁）⇒ ⇒
    // 目录里有两个**已发布**的名字 ⇒ ⇒ 断言要一起认 ⇒ ⇒ 但**临时名一个都不许留**
    // （`.tmp-` 那种），那才是这条用例真正要守的东西。
    ['ds~abcdefghijkl~0001.body.txt', 'ds~abcdefghijkl~0001.json'],
    '目录里只允许有最终名（临时名已被 rename 掉，不留残迹）',
  );
  const raw = readFileSync(saved.path, 'utf8');
  const parsed = JSON.parse(raw);
  // **盘上的摘要 ＝ 读回之后重算的值**（P3-1③：不是内存里那份）。
  assert.equal(parsed.contentDigest, contentDigest(bodyOf(parsed)));
  assert.equal(parsed.contentDigest, saved.contentDigest);
  // **回读校验的判定**：这份内容是「一致」的。
  assert.equal(snapshotReadBackMatches(raw, saved.contentDigest), true);
});

test('§S-8①：写临时文件失败 ⇒ **最终名不得出现**（半条记录不得发布）', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  // **制造一次写失败**：把实现要用的临时名**占成目录** ⇒ 写不进去。
  // （用目录而不是文件，是因为「占成同名文件」会被 `wx` 判成撞名而换一个
  // attempt 重试；占成目录则是**写不进去**，任何 attempt 都躲不掉。）
  // 三个 attempt 都占 ⇒ 不管实现先试哪个都撞上。
  const dir = snapshotsDir(home);
  mkdirSync(dir, { recursive: true });
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    mkdirSync(
      join(dir, tempSnapshotName('ds~abcdefghijkl~0001', { pid: process.pid, seq: 1, attempt })),
      { recursive: true },
    );
  }
  assert.throws(
    () => store.save(snapshot()),
    (error) => error?.code === 'snapshot_invalid',
    '写失败必须报协议内的码（不外泄内部异常串）',
  );
  // **最终名清单里没有那个 id** ⇒ 半条记录没有被发布。
  assert.equal(
    existsSync(join(dir, 'ds~abcdefghijkl~0001.json')),
    false,
    '写失败 ⇒ 最终名不得出现（发布点只能是 rename 成功那一刻）',
  );
});

test('§S-9：临时名不进清单、不占序号（发布前它不是快照）', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  store.save(snapshot());
  const dir = snapshotsDir(home);
  // **手工造一个临时名文件**（§2.3 的形态）。
  const tempName = tempSnapshotName('ds~abcdefghijkl~0002', { pid: 1234, seq: 2, attempt: 1 });
  assert.ok(isTempSnapshotName(tempName), '临时名形态要能被认出来');
  writeFileSync(join(dir, tempName), '{"schemaVersion":', 'utf8');
  const seqs = store.listSessionSeqs('ds');
  assert.equal(seqs['ds~abcdefghijkl'], 1, '临时名不得进清单、不得占序号');
  // **按 id 读也读不到临时文件**（id 的闸不认 `.`）。
  assert.throws(
    () => store.load('.ds~abcdefghijkl~0002.json.tmp-1234-0002-1'),
    (error) => error?.code === 'snapshot_invalid',
  );
});

test('§S-8④：不覆盖（同 id 再写一次 ⇒ snapshot_exists，原文件逐字未变）', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  store.save(snapshot());
  const path = join(snapshotsDir(home), 'ds~abcdefghijkl~0001.json');
  const before = readFileSync(path, 'utf8');
  assert.throws(
    () => store.save(snapshot()),
    (error) => error?.code === 'snapshot_exists',
    '同 id 不重写（P1 §6）',
  );
  assert.equal(readFileSync(path, 'utf8'), before, '原文件必须逐字未变');
});

test('§S-8②：回读校验 —— 盘上内容与摘要对不上 ⇒ 判定为「不许发布」', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  const saved = store.save(snapshot());
  const raw = readFileSync(saved.path, 'utf8');
  assert.equal(snapshotReadBackMatches(raw, saved.contentDigest), true, '正常内容是一致的');
  // **正文被改坏**（摘要没变）⇒ 不一致 ⇒ 不许发布。
  const tamperedBody = raw.replace('第一行。', '被改坏了。');
  assert.notEqual(tamperedBody, raw, '篡改必须真的改到内容');
  assert.equal(snapshotReadBackMatches(tamperedBody, saved.contentDigest), false, '正文被改 ⇒ 不许发布');
  // **摘要字段被改坏**（正文没变）⇒ 不一致。
  const tamperedDigest = raw.replace(saved.contentDigest, 'f'.repeat(64));
  assert.equal(snapshotReadBackMatches(tamperedDigest, saved.contentDigest), false, '摘要被改 ⇒ 不许发布');
  // **不是 JSON**（半截内容）⇒ 不一致。
  assert.equal(snapshotReadBackMatches('{"schemaVersion":', saved.contentDigest), false, '半截内容 ⇒ 不许发布');
});

test('§S-10：第五家族 —— 超过阈值的临时残留被清，阈值内的与非临时名形态的不删', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  store.save(snapshot());
  const dir = snapshotsDir(home);
  // ① 旧残留（超阈值）⇒ 该删。
  const oldTemp = join(dir, tempSnapshotName('ds~abcdefghijkl~0009', { pid: 1, seq: 9, attempt: 1 }));
  writeFileSync(oldTemp, '{"half":', 'utf8');
  const old = new Date(NOW - 3 * 24 * HOUR);
  utimesSync(oldTemp, old, old);
  // ② 新残留（阈值内）⇒ 不该删（**正在发布的窗口**也落在这里）。
  const freshTemp = join(dir, tempSnapshotName('ds~abcdefghijkl~0010', { pid: 2, seq: 10, attempt: 1 }));
  writeFileSync(freshTemp, '{"half":', 'utf8');
  // ③ **不是临时名形态**的文件 ⇒ 不该删（普通快照不归临时家族管）。
  const notTemp = join(dir, 'ds~abcdefghijkl~0011.json');
  writeFileSync(notTemp, '{"x":1}', 'utf8');
  // ④ 根外的哨兵 ⇒ 清理后必须还在（**不得越出缓存根**）。
  const sentinel = join(home.dir, 'sentinel.txt');
  writeFileSync(sentinel, 'x', 'utf8');

  const sweeper = createSweeper({ root: store.root, now: () => NOW });
  const result = sweeper.sweep();
  assert.equal(result.tmp.removed, 1, '只该删那一个超阈值的临时残留');
  assert.equal(existsSync(oldTemp), false, '超阈值的临时残留要删');
  assert.equal(existsSync(freshTemp), true, '阈值内的临时残留不删');
  assert.equal(existsSync(notTemp), true, '不是临时名形态的文件不删');
  assert.equal(existsSync(sentinel), true, '不得越出缓存根');
  // **临时残留有自己的阈值**（24 小时），**不与 7 天保留期混用**。
  assert.equal(CONSTANTS.tmpResidualMs, 24 * HOUR);
  assert.notEqual(CONSTANTS.tmpResidualMs, CONSTANTS.snapshotRetentionMs);
});

test('§S-10：删不掉 ⇒ 只记日志、不抛异常、不拖垮整轮', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  const dir = snapshotsDir(home);
  mkdirSync(dir, { recursive: true });
  const temp = join(dir, tempSnapshotName('ds~abcdefghijkl~0012', { pid: 3, seq: 12, attempt: 1 }));
  writeFileSync(temp, '{"half":', 'utf8');
  const old = new Date(NOW - 3 * 24 * HOUR);
  utimesSync(temp, old, old);
  // **制造一个「删不掉」**：两步法先 `rename` 成墓碑再 `unlink` ⇒ 把**墓碑名**
  // 占成一个**同名目录** ⇒ rename 必然失败（POSIX：EISDIR；Windows：
  // MoveFileEx  onto 一个目录同样失败）⇒ **两个平台通吃**。
  //
  // **墓碑名要可预测** ⇒ 把全局 `Date.now` 冻住（`removeInTwoSteps` 用的就是它）。
  const realNow = Date.now;
  Date.now = () => 1_700_000_000_000;
  const tombstone = `${temp}.expired-${Date.now()}`;
  mkdirSync(tombstone, { recursive: true });
  const warnings = [];
  try {
    const sweeper = createSweeper({
      root: store.root,
      now: () => NOW,
      logger: { warn: (message) => warnings.push(String(message)) },
    });
    // **不许抛**（清理失败只记日志，不阻塞业务：协议 §4.1 第 3 条）。
    const result = sweeper.sweep();
    assert.equal(result.skipped, false, '一轮清理不许被一次删失败带崩');
    assert.equal(warnings.length >= 1, true, '删不掉必须留痕（不能静默吞掉）');
    assert.match(warnings.join('\n'), /临时残留/);
    // **没假装删掉**：那一份还在盘上。
    assert.equal(existsSync(temp), true, '删不掉的文件必须还在（不许假装删了）');
  } finally {
    Date.now = realNow;
  }
});

test('§4：schemaVersion 不变 ⇒ 旧快照照旧可读（发布方式改了，落什么没改）', () => {
  const home = makeHome();
  const store = createSnapshotStore(home.env);
  const dir = snapshotsDir(home);
  mkdirSync(dir, { recursive: true });
  // **手写一份「旧形态」的快照**（字段与现在一致，版本仍是 1）。
  const legacy = {
    schemaVersion: SNAPSHOT_SCHEMA_VERSION,
    snapshotId: 'ds~abcdefghijkl~0007',
    providerId: 'ds',
    accountScope: '',
    sessionId: 'session-1',
    branch: '',
    messages: [{ role: 'user', text: '旧快照正文。' }],
    contentDigest: contentDigest('旧快照正文。'),
    captureResult: { status: 'ok', omitted: [], unsupported: [] },
    capturedAt: '2026-10-20T00:00:00.000Z',
    sourceUpdatedAt: '2026-10-20T00:00:00.000Z',
    sourceFingerprint: '',
  };
  writeFileSync(join(dir, 'ds~abcdefghijkl~0007.json'), `${JSON.stringify(legacy, null, 2)}\n`, 'utf8');
  const loaded = store.load('ds~abcdefghijkl~0007');
  assert.equal(loaded.snapshot.contentDigest, legacy.contentDigest);
  assert.equal(loaded.body, '旧快照正文。');
});
