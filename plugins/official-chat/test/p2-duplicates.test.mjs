// P2 第一批（协议 §1.1 / §1.2 / §2.2 / §2.3 / §4）的判据：
// **重复识别 ＋ 可见提示 ＋ 预算预检**。
//
// ## 判什么
//
// · **成组**：键五字段全等 ⇒ 成组（§1.1）；**任一**字段不同 ⇒ 不成组。
// · **组标识**：`groupKey` = 五字段摘要前 16 hex（§1.2），稳定可复现。
// · **出参**：`{ok, groups[], budget{}}` 逐字段照 §2.2。
// · **失败口径**：§2.3 的四种情形 → 码；闭集外的码按 P1 §3 归入
//   `snapshot_invalid`、原码进 `detail`（**不新增码**）。
// · **预算**：`projectedBytes` = 各引用**各送一份**的总字节；超限只**标记**，
//   **不截断正文**（§4 / §3.3）。
//
// ## 不判什么
//
// · **可见提示的界面形态与文案**：UX 方案未定稿 ⇒ 这里只判**数据**
//   （组 / 条数 / 字节 / 是否超限），不判文案措辞。
// · **真机上的承载位**：内核输入区 notice 的呈现策略属 UI 层，不在命令层。
//
// ## 为什么用内存桩存储
//
// 与 `host-commands.test.mjs` 同款：命令层是**纯业务**，这里证的是
// 「成组 / 不成组 / 预算 / 失败口径」这四条口径，不证传输与真机。

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { createCatalogCache, createCommands, createFixtureSource } from '../host/commands.js';
import { buildSnapshot, groupKeyOf } from '../host/protocol.js';
import { createSeqAllocator } from '../host/seq-store.js';

const ACCOUNT_SCOPE = 'desktop-dev/default/site-A';

/**
 * 快照里的 `sourceFingerprint`（host 侧按账户范围推导；**协议未规定算法**，
 * 这里复刻 host 的那一行作为桩约定 ⇒ 与命令层的比对口径一致）。
 */
const FINGERPRINT = createHash('sha256').update(ACCOUNT_SCOPE, 'utf8').digest('hex').slice(0, 32);

/** 造一套内存桩：存储 + 目录缓存 + 固定夹具取数源。 */
function makeHarness() {
  const root = mkdtempSync(join(tmpdir(), 'official-chat-p2-'));
  const written = new Map();
  const store = {
    save: (snapshot) => {
      written.set(String(snapshot?.snapshotId ?? ''), snapshot);
      return { snapshotId: snapshot.snapshotId, contentDigest: snapshot.contentDigest };
    },
    load: (id) => {
      const found = written.get(String(id));
      if (found === undefined) {
        const error = new Error(`快照不存在：${String(id)}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      return { snapshot: found, body: found.messages.map((m) => m.text).join('\n\n') };
    },
    listSessionSeqs: () => ({}),
  };
  const commands = createCommands({
    store,
    catalogCache: createCatalogCache({ rootDir: join(root, 'catalog-cache') }),
    captureSource: createFixtureSource([
      { sessionId: 'sess-alpha-001', title: '夹具对话', body: '夹具正文。' },
    ]),
    seqAllocator: createSeqAllocator({ root, providerId: 'official-chat' }),
    providerId: 'official-chat',
    accountScope: ACCOUNT_SCOPE,
  });
  return { root, store, commands };
}

/**
 * 造一个快照（**五字段可控** ⇒ 成组 / 不成组的夹具靠它）。
 *
 * @param {object} options
 * @returns {object} 快照对象。
 */
function makeSnapshot({
  seq,
  sessionId = 'sess-alpha-001',
  branch = 'main',
  accountScope = ACCOUNT_SCOPE,
  capturedAt = '2026-10-20T00:00:00.000Z',
  text = '同一段正文。',
}) {
  return buildSnapshot({
    providerId: 'official-chat',
    accountScope,
    sessionId,
    branch,
    seq,
    messages: [{ role: 'user', text }],
    capturedAt,
    sourceUpdatedAt: capturedAt,
    sourceFingerprint: FINGERPRINT,
  });
}

test('P2 §1.1/§1.2：键五字段全等才成组；groupKey 是 16 位小写十六进制且稳定', () => {
  const a = makeSnapshot({ seq: 1 });
  const b = makeSnapshot({ seq: 2 });
  // **键全等 ⇒ 同组**（`seq` 不在键里 ⇒ 两个不同 id 仍同组）。
  assert.equal(groupKeyOf(a), groupKeyOf(b));
  assert.match(groupKeyOf(a), /^[0-9a-f]{16}$/);
  // **任一字段不同 ⇒ 不成组**（§1.1 的四条负例）。
  assert.notEqual(groupKeyOf(a), groupKeyOf(makeSnapshot({ seq: 3, capturedAt: '2026-10-21T00:00:00.000Z' })));
  assert.notEqual(groupKeyOf(a), groupKeyOf(makeSnapshot({ seq: 4, accountScope: 'desktop-dev/default/site-B' })));
  assert.notEqual(groupKeyOf(a), groupKeyOf(makeSnapshot({ seq: 5, branch: 'feature-x' })));
  assert.notEqual(groupKeyOf(a), groupKeyOf(makeSnapshot({ seq: 6, sessionId: 'sess-beta-002' })));
});

test('P2 §2.2：同组 ⇒ 两条都在 groups 里，redundantBytes = (n-1) × 单条正文', async () => {
  const { commands, store } = makeHarness();
  const a = makeSnapshot({ seq: 1, text: '重复正文。' });
  const b = makeSnapshot({ seq: 2, text: '重复正文。' });
  store.save(a);
  store.save(b);
  const result = await commands.duplicates({
    sessionId: 'sess-alpha-001',
    snapshotIds: [a.snapshotId, b.snapshotId],
  });
  assert.equal(result.ok, true);
  assert.equal(result.groups.length, 1);
  const [group] = result.groups;
  assert.equal(group.members.length, 2);
  assert.deepEqual(
    group.members.map((m) => m.snapshotId).sort(),
    [a.snapshotId, b.snapshotId].sort(),
  );
  assert.equal(group.members[0].contentDigest, a.contentDigest);
  assert.equal(group.members[0].capturedAt, '2026-10-20T00:00:00.000Z');
  // **冗余字节 = (成员数 - 1) × 单条正文**（只是数字，不真的省）。
  assert.equal(group.redundantBytes, Buffer.byteLength('重复正文。', 'utf8'));
  // **成员不合并**（§1.3）：两个 id 都还在（P2-1：不得退化成复用旧 id）。
  assert.notEqual(a.snapshotId, b.snapshotId);
});

test('P2 §1.1 负例：capturedAt / accountScope / branch / sessionId 不同 ⇒ 不成组', async () => {
  const { commands, store } = makeHarness();
  const a = makeSnapshot({ seq: 1 });
  const cases = [
    makeSnapshot({ seq: 2, capturedAt: '2026-10-21T00:00:00.000Z' }),
    makeSnapshot({ seq: 3, accountScope: 'desktop-dev/default/site-B' }),
    makeSnapshot({ seq: 4, branch: 'feature-x' }),
    makeSnapshot({ seq: 5, sessionId: 'sess-beta-002' }),
  ];
  store.save(a);
  for (const one of cases) store.save(one);
  for (const one of cases) {
    const result = await commands.duplicates({
      sessionId: 'sess-alpha-001',
      snapshotIds: [a.snapshotId, one.snapshotId],
    });
    assert.equal(result.ok, true);
    assert.deepEqual(result.groups, [], `不该成组：${one.snapshotId}`);
  }
});

test('P2 §4：预算按各送一份累计；超限只标记，不截断正文', async () => {
  const { commands, store } = makeHarness();
  const big = 'A'.repeat(200000);
  const a = makeSnapshot({ seq: 1, text: big });
  const b = makeSnapshot({ seq: 2, text: big });
  store.save(a);
  store.save(b);
  const result = await commands.duplicates({
    sessionId: 'sess-alpha-001',
    snapshotIds: [a.snapshotId, b.snapshotId],
  });
  assert.equal(result.ok, true);
  assert.equal(result.budget.limitBytes, 262144);
  assert.equal(result.budget.projectedBytes, 400000);
  assert.equal(result.budget.wouldExceed, true);
  assert.equal(result.budget.exceedByBytes, 400000 - 262144);
  // **正文没被截断**：盘上那份还是 200000 字节。
  assert.equal(Buffer.byteLength(store.load(a.snapshotId).body, 'utf8'), 200000);
});

test('P2 §2.3：入参形态不对 ⇒ 失败；码留在闭集内，原码进 detail', async () => {
  const { commands } = makeHarness();
  const noSession = await commands.duplicates({ sessionId: '', snapshotIds: [] });
  assert.equal(noSession.ok, false);
  // **闭集内**（P1 §3 的 13 码）——P2 §6.5 明写「不新增错误码」。
  assert.equal(noSession.code, 'snapshot_invalid');
  // **原始码要留着**（P1 §3：内部冲突码归入 snapshot_invalid、原码保留在 detail）。
  assert.match(String(noSession.detail), /invalid_request/);
  const notArray = await commands.duplicates({ sessionId: 'sess-alpha-001', snapshotIds: 'ds~x~0001' });
  assert.equal(notArray.ok, false);
  assert.equal(notArray.code, 'snapshot_invalid');
});

test('P2 §2.3：快照不存在 ⇒ 拒（snapshot_invalid），不静默通过', async () => {
  const { commands } = makeHarness();
  const result = await commands.duplicates({ sessionId: 'sess-alpha-001', snapshotIds: ['ds~nope~0001'] });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'snapshot_invalid');
  // **错误负载带齐 P1 §3 要求的字段**（码 / 可重试性 / 用户下一步 / requestId /
  // 脱敏日志路径 / detail）。
  //
  // **不断言「字段全集」**：P1 §3 v0.4 写的是 **8 字段闭集**（含 `kind`），
  // 而实现侧的 `errorPayload` 目前只产 **7 个**（无 `kind`）——这是**既有的**
  // 协议/实现差异（不是本批引入）⇒ 这里只断「必备字段都在」，**不把「7 个」
  // 固化成正确口径**（待 PM 裁定）。
  for (const key of ['ok', 'code', 'retryable', 'userNext', 'requestId', 'logPath', 'detail']) {
    assert.ok(key in result, `错误负载缺字段：${key}`);
  }
});
