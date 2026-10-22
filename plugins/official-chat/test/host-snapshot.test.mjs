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

test('**正文文件**（模型读的那一份）：落盘、**逐字节等于**摘要覆盖的那份正文', () => {
  // **为什么逐字节**：模型读到的那份必须与 `contentDigest` 覆盖的是**同一份**
  // ⇒ ⇒ 加一个尾部换行就断了 ⇒ ⇒ 这条用例盯的就是「一个字节都不多」。
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    const saved = store.save(snapshot());
    const bodyFile = join(
      env.DSH_HOME,
      'official-chat',
      'snapshots',
      'ds',
      'ds~abcdefghijkl~0001.body.txt',
    );
    assert.ok(existsSync(bodyFile), `正文文件应当落在这里：${bodyFile}`);
    assert.equal(
      readFileSync(bodyFile, 'utf8'),
      bodyOf(snapshot()),
      '正文文件必须与摘要覆盖的正文逐字节相同（不加尾部换行）',
    );
    assert.equal(saved.bodyPath, bodyFile, '落盘结果要带上正文文件路径');
    // **读回来时也要报这个文件**（客户端靠它拼文件指针）。
    const loaded = store.load('ds~abcdefghijkl~0001');
    assert.equal(loaded.bodyFile?.path, bodyFile);
    assert.equal(loaded.bodyFile?.bytes, Buffer.byteLength(bodyOf(snapshot()), 'utf8'));
  } finally {
    cleanup();
  }
});

test('**老快照没有正文文件** ⇒ 读的时候**补一份**（行为统一，不靠「有没有新代码」区分）', () => {
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    store.save(snapshot());
    // **模拟老快照**：把正文文件删掉（这次改动之前捕获的就是这个样子）。
    const bodyFile = join(
      env.DSH_HOME,
      'official-chat',
      'snapshots',
      'ds',
      'ds~abcdefghijkl~0001.body.txt',
    );
    rmSync(bodyFile, { force: true });
    assert.equal(existsSync(bodyFile), false);
    const loaded = store.load('ds~abcdefghijkl~0001');
    assert.equal(loaded.bodyFile?.path, bodyFile, '读的时候要补出来');
    assert.ok(existsSync(bodyFile), '补写之后文件应当在');
    assert.equal(readFileSync(bodyFile, 'utf8'), bodyOf(snapshot()));
  } finally {
    cleanup();
  }
});

test('同一 id 不重写（协议 §6：快照文件不可变）', () => {  const { env, cleanup } = tempHome();
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

test('G3-2b：单条正文超「单条正文预算」⇒ 拒收**这一条**（不落盘），别的引用不受影响', () => {
  // 协议 P1 §3 第 14 码：单条正文（进模型输入的**那一条**）超 64 KB ⇒ 报
  // `body_budget_exceeded` ＋ **拒收该条** ⇒ ⇒ **别的引用不受影响**（各引用各自落盘）。
  const { env, cleanup } = tempHome();
  try {
    const store = createSnapshotStore(env);
    // ① 先落一条**正常**引用（它必须照常在盘上）。
    store.save(snapshot({ snapshotId: 'ds~aaaaaaaaaaaa~0001' }));
    // ② 再落一条超单条正文预算的引用 ⇒ 必须被拒。
    const budget = 64 * 1024;
    assert.throws(
      () =>
        store.save(
          snapshot({
            snapshotId: 'ds~bbbbbbbbbbbb~0001',
            messages: [{ role: 'user', text: 'a'.repeat(budget + 1) }],
          }),
        ),
      (error) => {
        assert.equal(error.code, 'body_budget_exceeded', '单条正文超预算必须落第 14 码');
        // **`detail` 必须带三个数**（正文字节 / 预算字节 / 超出量）——客户端拿不到
        // host 的内部变量 ⇒ 界面文案要用的三个数只能由这里带出去（PM 2026-10-21 裁）
        // ⇒ 形态与协议层 `resolveCaptureCase`、Rust 侧**逐字同构**。
        assert.equal(
          error.message,
          `单条正文超单条正文预算：正文 ${budget + 1} 字节，超出单条正文预算 ${budget} 字节（超 1 字节）`,
          'detail 必须带「正文字节 / 预算字节 / 超出量」三个数',
        );
        // ⚠️ **三个数还必须挂在 `error.detail` 上** —— 存储层的异常要过
        // `host/commands.js` 的 catch 才到得了 RPC 边界，而**那一层只认
        // `error.detail`**（`error.message` 在那层被丢掉）⇒ ⇒ 不挂 `detail`
        // ⇒ 三个数到不了对端（客户端只剩一个码名 ⇒ 界面退化成无数字文案）。
        assert.equal(
          error.detail,
          `单条正文超单条正文预算：正文 ${budget + 1} 字节，超出单条正文预算 ${budget} 字节（超 1 字节）`,
          'error.detail 必须带三个数（边界转换只认它）',
        );
        return true;
      },
      '单条正文超预算必须落第 14 码（不得拿 body_too_large 兼表）',
    );
    // ②' **判序**（P1 §3）：超过 256 KB ⇒ 落**老码** `body_too_large`（第 14 码不得
    //     兼表那一档）；64 KB～256 KB 那一档 ⇒ 落第 14 码。
    assert.throws(
      () =>
        store.save(
          snapshot({
            snapshotId: 'ds~cccccccccccc~0001',
            messages: [{ role: 'user', text: 'a'.repeat(256 * 1024 + 1) }],
          }),
        ),
      (error) => error.code === 'body_too_large',
      '超过 256 KB 必须落老码',
    );
    assert.throws(
      () =>
        store.save(
          snapshot({
            snapshotId: 'ds~dddddddddddd~0001',
            messages: [{ role: 'user', text: 'a'.repeat(100_000) }],
          }),
        ),
      (error) => error.code === 'body_budget_exceeded',
      '64 KB～256 KB 那一档必须落第 14 码',
    );
    // ③ 被拒的那条**没有落盘**（拒收 ＝ 根本没进来）。
    const rejected = join(
      env.DSH_HOME,
      'official-chat',
      'snapshots',
      'ds',
      'ds~bbbbbbbbbbbb~0001.json',
    );
    assert.equal(existsSync(rejected), false, '被拒的引用不得落盘');
    // ④ **别的引用不受影响**（这条是本单的核心语义，必须钉住）。
    const kept = join(env.DSH_HOME, 'official-chat', 'snapshots', 'ds', 'ds~aaaaaaaaaaaa~0001.json');
    assert.ok(existsSync(kept), '别的引用必须照常落盘（拒收只影响那一条）');
    assert.equal(store.load('ds~aaaaaaaaaaaa~0001').snapshot.snapshotId, 'ds~aaaaaaaaaaaa~0001');
  } finally {
    cleanup();
  }
});
