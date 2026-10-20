// P1 快照存储的判据（真落盘）。
//
// ## 为什么需要这条
//
// S-3 要求「落盘后单一事实源 = 快照文件」，S-4 要求「请求发生时快照已落盘，
// 且落盘内容与模型输入逐字节相同」。这两条都压在**存储**上：写不进去、写得
// 不对、读回来不是同一份、被人改过还照读——每一条都会让「就绪」变成假的。
//
// 客户端半边那侧（`client.test.mjs`）验的是**语义**（ref 是快照 id、状态由
// 文件派生、正文取自文件）；这里验的是**存储本身**：路径怎么解析、文件怎么
// 写、摘要怎么算、损坏了怎么办。
//
// ## 隔离
//
// 全程用**临时目录**当 `DSH_HOME`，不碰用户真实数据目录；用完删掉。

import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  MAX_SNAPSHOT_ID_LEN,
  SNAPSHOT_SCHEMA_VERSION,
  bodyOf,
  contentDigest,
  createSnapshotStore,
  isValidSnapshotId,
  providerOfSnapshotId,
  resolveSnapshotRoot,
} from '../host/index.js';
// **规范化口径在 `host/protocol.js`**（单一真相）：host 半边不再自带一份。
import { normalizeText } from '../host/protocol.js';

/** 造一个临时 home，返回 `{env, cleanup}`。 */
function tempHome() {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-official-chat-snapshot-'));
  return {
    env: { DSH_HOME: dir },
    cleanup: () => rmSync(dir, { recursive: true, force: true }),
  };
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
    capturedAt: '2026-10-20T00:00:00Z',
    sourceUpdatedAt: '',
    sourceFingerprint: '',
    ...overrides,
  };
}

test('快照 id 的形状与长度（协议 S-1）', () => {
  assert.ok(isValidSnapshotId('ds~abcdefghijkl~0001'));
  assert.ok(!isValidSnapshotId('ds~abcdefghijkl~1'), 'seq 必须是 4 位');
  assert.ok(!isValidSnapshotId('DS~abcdefghijkl~0001'), '只认小写');
  assert.ok(!isValidSnapshotId('ds~abc/def~0001'), '不许带路径分隔符');
  assert.ok(!isValidSnapshotId('ds~..~0001'), '不许带相对路径片段');
  assert.ok(!isValidSnapshotId('ds~abcdefghijklm~0001'), 'sessionShort ≤ 12');
  assert.ok(!isValidSnapshotId(`ds~${'a'.repeat(20)}~0001`), `整串 ≤ ${MAX_SNAPSHOT_ID_LEN}`);
  assert.equal(providerOfSnapshotId('ds~abcdefghijkl~0001'), 'ds');
});

test('根目录跟着后端解析：DSH_HOME → <home>/official-chat；覆盖项优先；都没有 = 不可用', () => {
  assert.equal(resolveSnapshotRoot({ DSH_HOME: '/tmp/x' }), join('/tmp/x', 'official-chat'));
  assert.equal(
    resolveSnapshotRoot({ DSH_HOME: '/tmp/x', DSH_XLINK_OFFICIAL_CHAT_ROOT: '/tmp/y' }),
    '/tmp/y',
  );
  assert.equal(resolveSnapshotRoot({}), null, '没有 DSH_HOME 就**不猜**（不落到默认位置）');
});

test('规范化：行尾统一 \\n、去 BOM、去整段末尾空白，**保留**正文内空行与缩进', () => {
  assert.equal(normalizeText('a\r\nb'), 'a\nb');
  assert.equal(normalizeText('\uFEFFabc'), 'abc');
  assert.equal(normalizeText('abc  \n'), 'abc');
  assert.equal(normalizeText('a\n\n  b\n'), 'a\n\n  b', '正文内的空行与缩进必须保留');
});

test('摘要 = 规范化后正文的 SHA-256（十六进制小写）', () => {
  const digest = contentDigest('abc');
  assert.match(digest, /^[0-9a-f]{64}$/);
  assert.equal(
    contentDigest('a\r\nbc'),
    contentDigest('a\nbc'),
    '摘要算在规范化之后（\\r\\n 与 \\n 同摘要）',
  );
  assert.notEqual(digest, contentDigest('abd'));
});

test('落盘 → 读回：内容逐字节相同，文件落在 <root>/snapshots/<provider>/<id>.json', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    const saved = store.save(snapshot());
    assert.equal(saved.snapshotId, 'ds~abcdefghijkl~0001');
    const file = join(env.DSH_HOME, 'official-chat', 'snapshots', 'ds', 'ds~abcdefghijkl~0001.json');
    assert.ok(existsSync(file), `快照文件应当落在这里：${file}`);
    const loaded = store.load('ds~abcdefghijkl~0001');
    assert.equal(loaded.body, bodyOf(snapshot()));
    assert.equal(loaded.snapshot.contentDigest, saved.contentDigest);
  } finally {
    cleanup();
  }
});

test('同一 id 不重写（协议 §6：快照文件不可变）', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    store.save(snapshot());
    assert.throws(
      () => store.save(snapshot({ messages: [{ role: 'user', text: '改了' }] })),
      (error) => error.code === 'snapshot_exists',
      '同 id 再写必须被拒',
    );
  } finally {
    cleanup();
  }
});

test('读不存在的快照 → snapshot_invalid（拒，不静默消失）', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    assert.throws(
      () => store.load('ds~abcdefghijkl~0002'),
      (error) => error.code === 'snapshot_invalid',
    );
  } finally {
    cleanup();
  }
});

test('文件被改过（摘要对不上）→ snapshot_invalid（是损坏，不是重读）', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    store.save(snapshot());
    const file = join(env.DSH_HOME, 'official-chat', 'snapshots', 'ds', 'ds~abcdefghijkl~0001.json');
    const raw = JSON.parse(readFileSync(file, 'utf8'));
    raw.messages[0].text = '被人改过的正文';
    writeFileSync(file, JSON.stringify(raw), 'utf8');
    assert.throws(
      () => store.load('ds~abcdefghijkl~0001'),
      (error) => error.code === 'snapshot_invalid',
      '摘要对不上必须拒',
    );
  } finally {
    cleanup();
  }
});

test('版本不支持 → schema_changed（不是 snapshot_invalid：两码要分得开）', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    store.save(snapshot());
    const file = join(env.DSH_HOME, 'official-chat', 'snapshots', 'ds', 'ds~abcdefghijkl~0001.json');
    const raw = JSON.parse(readFileSync(file, 'utf8'));
    raw.schemaVersion = 99;
    writeFileSync(file, JSON.stringify(raw), 'utf8');
    assert.throws(
      () => store.load('ds~abcdefghijkl~0001'),
      (error) => error.code === 'schema_changed',
    );
  } finally {
    cleanup();
  }
});

test('id 不合法 → 直接拒（不拼路径、不落盘）', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    assert.throws(
      () => store.save(snapshot({ snapshotId: '../escape' })),
      (error) => error.code === 'snapshot_invalid',
    );
  } finally {
    cleanup();
  }
});

test('没有 DSH_HOME → 存储不可用（**不猜**默认位置）', () => {
  const store = createSnapshotStore({});
  assert.equal(store.available, false);
  assert.throws(
    () => store.save(snapshot()),
    (error) => error.code === 'snapshot_store_unavailable',
  );
});

test('listSessionSeqs：报出每个会话短码已用掉的最大序号', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    store.save(snapshot({ snapshotId: 'ds~aaaaaaaaaaaa~0001' }));
    store.save(snapshot({ snapshotId: 'ds~aaaaaaaaaaaa~0002' }));
    store.save(snapshot({ snapshotId: 'ds~bbbbbbbbbbbb~0001' }));
    const seqs = store.listSessionSeqs('ds');
    assert.equal(seqs['ds~aaaaaaaaaaaa'], 2);
    assert.equal(seqs['ds~bbbbbbbbbbbb'], 1);
  } finally {
    cleanup();
  }
});
