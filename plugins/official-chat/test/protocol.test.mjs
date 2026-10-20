// P1 协议的**夹具一致性**判据（协议冻结稿 §6）。
//
// ## 它判什么
//
// 「**同一组 JSON 夹具，Rust 与 JavaScript 各跑一遍，断言同一夹具 → 同一解析
// 结果**；`schemaVersion` 两端常量必须一致」。
//
// 跨端口径漂移是这类协议最常见的病：句柄形态、规范化范围、摘要算法、消息
// 分隔符——任何一处两端不一致，就会出现「客户端算出的 id 与宿主算出的 id 不是
// 同一个」这类**只在真机上才显形**的错。
//
// ## 判的是**数据**，不是代码长相
//
// 这个测试**不**去比对两个实现的源码形状（那种判据骗过自己好几回）。它跑的是
// **同一份夹具**（`plugins/official-chat/fixtures/protocol/`），两端各出一份
// 结果，逐字段比。Rust 那半在 `src-tauri/src/official_chat/protocol.rs`，
// 读的是**同一个目录**下的同一批文件——不是各写一份期望值。
//
// ## 为什么夹具要固化「期望值」
//
// 夹具里的 `expected` 是**字面量**：它由某一侧算出来后固化下来，于是另一侧
// 必须**独立**算出同样的值才算通过。这比「两边各自算各自的、再互相断言」
// 更严——它把「一致」这件事钉在**数据**上。
//
// 期望值本身的可信度另有一道独立复核：摘要用 PowerShell 的
// `System.Security.Cryptography.SHA256` 独立算过（不经过本仓的任何实现）。

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import {
  CONSTANTS,
  ERROR_CODE_NAMES,
  ERROR_CODES,
  SCHEMA_VERSION,
  STATES,
  TRANSITIONS,
  canTransition,
  captureFailureCode,
  contentDigest,
  errorPayload,
  makeSessionShort,
  makeSnapshotId,
  parseSnapshotId,
  resolveCase,
  validateCatalogPage,
  validateSnapshot,
} from '../host/protocol.js';

const FIXTURE_DIR = new URL('../fixtures/protocol/', import.meta.url);

/** 读一份夹具。 */
function readFixture(name) {
  return JSON.parse(readFileSync(new URL(name, FIXTURE_DIR), 'utf8'));
}

/** 夹具清单（两端都读它，保证覆盖集一致）。 */
const INDEX = readFixture('index.json');
const CASES = INDEX.cases.map((name) => readFixture(name));

test('夹具清单存在，且覆盖协议 §6 要求的十一类', () => {
  assert.ok(CASES.length >= 11, `夹具至少要覆盖 §6 的十一类，现在只有 ${CASES.length} 类`);
  const ids = CASES.map((item) => item.id);
  for (const required of [
    'normal',
    'partial',
    'oversized',
    'catalog-empty',
    'catalog-failed',
    'timeout',
    'cancelled',
    'account-changed',
    'snapshot-missing',
    'schema-unsupported',
    'digest-mismatch',
  ]) {
    assert.ok(ids.includes(required), `夹具缺少「${required}」这一类`);
  }
});

test('两端常量一致：夹具的 schemaVersion == JS 侧常量', () => {
  assert.equal(INDEX.schemaVersion, SCHEMA_VERSION);
  assert.equal(SCHEMA_VERSION, 1, '首版快照格式版本是 1（协议 §4）');
});

test('每一条夹具：同一输入 → 同一解析结果（§6 的核心断言）', () => {
  for (const fixture of CASES) {
    const actual = resolveCase(fixture.input);
    const expected = fixture.expected;
    assert.equal(actual.kind, expected.kind, `${fixture.id}：结果类别必须一致`);

    if (expected.kind === 'error') {
      assert.equal(actual.code, expected.code, `${fixture.id}：错误码必须一致`);
      assert.equal(actual.retryable, expected.retryable, `${fixture.id}：可重试性必须一致`);
      assert.equal(actual.userNext, expected.userNext, `${fixture.id}：用户下一步必须一致`);
      // **错误里不得出现内部异常串**（协议 §3）：detail 只能是分类好的事实。
      assert.equal(actual.detail ?? null, expected.detail ?? null, `${fixture.id}：detail 必须一致`);
      continue;
    }

    if (expected.kind === 'catalog') {
      assert.deepEqual(actual.page, expected.page, `${fixture.id}：目录页必须一致`);
      continue;
    }

    if (expected.kind === 'duplicates') {
      // **成组判定必须一致**（P2 §1.1：键五字段全等）。
      assert.equal(actual.sameGroup, expected.sameGroup, `${fixture.id}：成组判定必须一致`);
      // **groupKey 必须与夹具一致**（协议 v0.2 §5：键的形状已定稿、可写；§1.2：
      // groupKey 用于判据比对）⇒ 夹具把它的值钉住，两端都断这一条 ⇒ 跨语言
      // 算得不一样会当场报红（不必等「两端逐字段相等」那条判据）。
      assert.equal(actual.groupKey, expected.groupKey, `${fixture.id}：groupKey 必须一致`);
      assert.match(actual.groupKey, /^[0-9a-f]{16}$/, `${fixture.id}：groupKey 必须是 16 位小写十六进制`);
      continue;
    }

    for (const field of Object.keys(expected.snapshot)) {
      assert.deepEqual(
        actual.snapshot[field],
        expected.snapshot[field],
        `${fixture.id}：快照字段 ${field} 必须一致`,
      );
    }
    assert.equal(actual.state, expected.state, `${fixture.id}：状态必须一致`);
    assert.equal(actual.notice ?? null, expected.notice ?? null, `${fixture.id}：提示码必须一致`);
  }
});

test('§3：13 个错误码全有稳定码名、可重试性与用户下一步', () => {
  assert.equal(ERROR_CODE_NAMES.length, 13);
  assert.deepEqual([...ERROR_CODE_NAMES].sort(), [...Object.keys(ERROR_CODES)].sort());
  for (const code of ERROR_CODE_NAMES) {
    const spec = ERROR_CODES[code];
    assert.equal(typeof spec.retryable, 'boolean', `${code} 缺可重试性`);
    assert.equal(typeof spec.userNext, 'string', `${code} 缺用户下一步`);
    assert.ok(spec.userNext.length > 0, `${code} 的用户下一步不能是空串`);
  }
  // 只有超时与目录读取失败可重试。
  const retryable = ERROR_CODE_NAMES.filter((code) => ERROR_CODES[code].retryable);
  assert.deepEqual(retryable.sort(), ['catalog_failed', 'timeout']);
});

test('§3：catalog_empty 与 catalog_failed 必须分离（空结果 ≠ 读取失败）', () => {
  const empty = errorPayload('catalog_empty');
  const failed = errorPayload('catalog_failed');
  assert.equal(empty.code, 'catalog_empty');
  assert.equal(failed.code, 'catalog_failed');
  assert.notEqual(empty.userNext, failed.userNext);
  assert.equal(empty.retryable, false);
  assert.equal(failed.retryable, true);
});

test('§3：错误负载不带内部异常串，也不带未登记的码', () => {
  const payload = errorPayload('snapshot_invalid', { requestId: 'r-1' });
  assert.equal(payload.code, 'snapshot_invalid');
  assert.equal(payload.requestId, 'r-1');
  assert.equal(payload.detail, null, '没给 detail 就不许有 detail');
  // 未知码 → 明确失败，不许「当成成功」。
  // **归 `snapshot_invalid`**（协议 §3「内部冲突码一律归入 `snapshot_invalid`」）——
  // **不归 `source_unsupported`**：那个码只表示「这个来源根本不支持该能力」。
  const unknown = errorPayload('no_such_code');
  assert.equal(unknown.ok, false);
  assert.equal(unknown.code, 'snapshot_invalid');
});

test('§4：首版常量与协议一致', () => {
  assert.equal(CONSTANTS.catalogPageSize, 50);
  assert.equal(CONSTANTS.catalogCacheTtlMs, 5 * 60 * 1000);
  assert.equal(CONSTANTS.searchDebounceMs, 300);
  assert.equal(CONSTANTS.readTimeoutMs, 30_000);
  assert.equal(CONSTANTS.maxBodyBytes, 256 * 1024);
  assert.equal(CONSTANTS.maxResponseBytes, 1024 * 1024);
  assert.equal(CONSTANTS.snapshotRetentionMs, 7 * 24 * 60 * 60 * 1000);
  assert.equal(CONSTANTS.cacheTotalBudgetBytes, 512 * 1024 * 1024);
  assert.equal(CONSTANTS.maxReferencesPerMessage, 8);
  assert.equal(CONSTANTS.perBodyBudgetBytes, 64 * 1024);
  assert.equal(CONSTANTS.perMessageBudgetBytes, 256 * 1024);
  assert.equal(CONSTANTS.schemaVersion, SCHEMA_VERSION);
});

test('§2 S-1：句柄形态、长度上限与可解析性', () => {
  const short = makeSessionShort('sess-alpha-001');
  assert.equal(short, 'rnaq4171cqan', '会话短码必须与客户端半边逐位一致');
  const id = makeSnapshotId({ providerShort: 'ds', sessionShort: short, seq: 1 });
  assert.equal(id, 'ds~rnaq4171cqan~0001');
  assert.ok(id.length <= 24, '句柄整串 ≤ 24 可见字符（HC-2：chip 宽度）');
  assert.deepEqual(parseSnapshotId(id), { providerShort: 'ds', sessionShort: short, seq: 1 });
  assert.equal(parseSnapshotId('not-a-snapshot-id'), null);
  assert.equal(parseSnapshotId('ds~abc~0001/../x'), null);
});

test('§2 S-2：状态机的合法转移表与「只有 ready 才算就绪」', () => {
  assert.ok(canTransition(STATES.selected, STATES.reading));
  assert.ok(canTransition(STATES.reading, STATES.validating));
  assert.ok(canTransition(STATES.validating, STATES.snapshotSaved));
  assert.ok(canTransition(STATES.snapshotSaved, STATES.ready));
  assert.ok(canTransition(STATES.ready, STATES.serialized));
  assert.ok(!canTransition(STATES.selected, STATES.ready), '不能从 selected 直接跳到 ready');
  assert.ok(!canTransition(STATES.failed, STATES.ready), '失败态不能变就绪');
  assert.ok(!canTransition(STATES.cancelled, STATES.ready));
  assert.ok(!canTransition(STATES.stale, STATES.ready));
});

test('§1.1：快照字段表逐字段校验（缺字段 / 版本不对 / 摘要形态不对都要拒）', () => {
  const snapshot = {
    schemaVersion: 1,
    snapshotId: 'ds~rnaq4171cqan~0001',
    providerId: 'official-chat',
    accountScope: '',
    sessionId: 'sess-alpha-001',
    branch: 'main',
    messages: [{ role: 'user', text: 'x' }],
    contentDigest: 'a'.repeat(64),
    captureResult: { status: 'ok', omitted: [], unsupported: [] },
    capturedAt: '2026-10-20T09:00:00.000Z',
    sourceUpdatedAt: '2026-10-20T08:59:00.000Z',
    sourceFingerprint: 'fp',
  };
  assert.deepEqual(validateSnapshot(snapshot), { ok: true, errors: [] });
  assert.equal(validateSnapshot({ ...snapshot, schemaVersion: 2 }).ok, false);
  assert.equal(validateSnapshot({ ...snapshot, contentDigest: 'NOTHEX' }).ok, false);
  assert.equal(validateSnapshot({ ...snapshot, messages: [{ role: 'User', text: 'x' }] }).ok, false);
  const { sourceFingerprint, ...withoutFingerprint } = snapshot;
  assert.equal(validateSnapshot(withoutFingerprint).ok, false, '缺字段要报');
});

test('§1.1（协议 v0.2 裁定）：captureResult 的形态 = 对象；字符串只兼容 \'ok\'', () => {
  // 协议 v0.2 裁定：**形态 = 对象**（状态 + 省略事实 + 未支持内容清单）；
  // `ok` **兼容字符串** `'ok'`（等价 `{status:'ok',omitted:[],unsupported:[]}`）；
  // `partial` / `failed` **必须用对象**。
  //
  // **为什么 `'partial'` 不能用字符串**：字符串会**丢掉「省略了什么」**——
  // 违反「不静默裁掉」纪律（协议 §1.1：未支持内容要显式记）。
  const base = {
    schemaVersion: 1,
    snapshotId: 'ds~rnaq4171cqan~0001',
    providerId: 'official-chat',
    accountScope: '',
    sessionId: 'sess-alpha-001',
    branch: 'main',
    messages: [{ role: 'user', text: 'x' }],
    contentDigest: 'a'.repeat(64),
    capturedAt: '2026-10-20T09:00:00.000Z',
    sourceUpdatedAt: '2026-10-20T08:59:00.000Z',
    sourceFingerprint: 'fp',
  };
  assert.equal(
    validateSnapshot({ ...base, captureResult: { status: 'ok', omitted: [], unsupported: [] } }).ok,
    true,
    '对象形态（协议）要收',
  );
  assert.equal(validateSnapshot({ ...base, captureResult: 'ok' }).ok, true, '字符串 \'ok\' 要收（无损兼容）');
  assert.equal(validateSnapshot({ ...base, captureResult: 'weird' }).ok, false, '乱值要拒');
  assert.equal(
    validateSnapshot({ ...base, captureResult: { status: 'ok' } }).ok,
    false,
    '对象形态缺省略事实/未支持内容清单要报',
  );
  // **v0.2 的新增判定**：partial / failed 用字符串一律拒。
  assert.equal(
    validateSnapshot({ ...base, captureResult: 'partial' }).ok,
    false,
    '\'partial\' 用字符串会丢省略事实，必须用对象',
  );
  assert.equal(
    validateSnapshot({ ...base, captureResult: 'failed' }).ok,
    false,
    '\'failed\' 用字符串会丢省略事实，必须用对象',
  );
  assert.equal(
    validateSnapshot({
      ...base,
      captureResult: { status: 'partial', omitted: ['图片'], unsupported: [] },
    }).ok,
    true,
    'partial 的对象形态要收（省略事实显式记着）',
  );
});

test('§3：错误负载带齐协议要求的字段（A1c：两端同构）', () => {
  // 协议 §3：每个错误码带「可重试性、用户下一步、关联 requestId、**脱敏**日志路径」。
  // **A1c**：Rust 侧此前少了 `ok` / `requestId` / `logPath` / `detail` ⇒ 两端不同构。
  const payload = errorPayload('timeout', { requestId: 'r-1', logPath: '<instance>/logs/x.log' });
  assert.equal(payload.ok, false);
  assert.equal(payload.code, 'timeout');
  assert.equal(payload.retryable, true);
  assert.equal(payload.userNext, '重试');
  assert.equal(payload.requestId, 'r-1');
  assert.equal(payload.logPath, '<instance>/logs/x.log');
  assert.equal(payload.detail, null);
  // 未知码不许漏出第 14 个码（协议 §3：码表是闭集，13 个）。
  // **归 `snapshot_invalid`**（§3「内部冲突码一律归入 `snapshot_invalid`」）——
  // **不归 `source_unsupported`**：那个码只表示「这个来源根本不支持该能力」。
  const unknown = errorPayload('snapshot_exists', { detail: 'snapshot_exists' });
  assert.equal(unknown.code, 'snapshot_invalid', '未知码要归到表内的码');
  assert.match(unknown.detail, /未知错误码/, '原始码留在 detail');
});

test('§3（v0.3）：捕获失败**按原因分流**——不新增码，也不拿 source_unsupported 兜', () => {
  // 超时 → timeout（**可重试**）
  assert.equal(captureFailureCode('timeout'), 'timeout');
  assert.equal(captureFailureCode('timed_out'), 'timeout');
  assert.equal(captureFailureCode('deadline_exceeded'), 'timeout');
  // 官网页面结构变了 → schema_changed
  assert.equal(captureFailureCode('schema_changed'), 'schema_changed');
  assert.equal(captureFailureCode('structure_changed'), 'schema_changed');
  // 用户取消 → cancelled
  assert.equal(captureFailureCode('cancelled'), 'cancelled');
  assert.equal(captureFailureCode('user_cancelled'), 'cancelled');
  // 来源明说不支持该能力 → source_unsupported（**唯一**允许它的场景）
  assert.equal(captureFailureCode('source_unsupported'), 'source_unsupported');
  // **原因不明 → timeout 兜**（可重试），不猜
  assert.equal(captureFailureCode(''), 'timeout');
  assert.equal(captureFailureCode('   '), 'timeout');
  assert.equal(captureFailureCode('一些没登记的原因'), 'timeout');
  // **绝不兜 source_unsupported**：那会让用户看到「换一个来源」而其实该重试
  assert.notEqual(captureFailureCode(''), 'source_unsupported');
  assert.notEqual(captureFailureCode('一些没登记的原因'), 'source_unsupported');

  // 判定层：captureResult.status='failed' 时走分流，detail 带脱敏线索
  const failed = resolveCase({
    kind: 'capture',
    schemaVersion: SCHEMA_VERSION,
    sessionId: 'sess-x',
    seq: 1,
    messages: [],
    captureResult: { status: 'failed', omitted: [], unsupported: [] },
  });
  assert.equal(failed.kind, 'error');
  assert.equal(failed.code, 'timeout', '原因不明 → timeout 兜（可重试）');
  assert.equal(failed.retryable, true);
  assert.equal(failed.detail, '捕获失败，原因未明');

  // 有明确原因时按原因分流
  const schemaFail = resolveCase({
    kind: 'capture',
    schemaVersion: SCHEMA_VERSION,
    sessionId: 'sess-x',
    seq: 1,
    messages: [],
    captureResult: { status: 'failed', omitted: [], unsupported: [] },
    captureFailure: { reason: 'schema_changed' },
  });
  assert.equal(schemaFail.code, 'schema_changed');
  assert.equal(schemaFail.detail, '捕获失败原因：schema_changed');

  // **不认识的 reason 不外发**（免得把内部异常串当原因线索漏出去）
  const leaky = resolveCase({
    kind: 'capture',
    schemaVersion: SCHEMA_VERSION,
    sessionId: 'sess-x',
    seq: 1,
    messages: [],
    captureResult: { status: 'failed', omitted: [], unsupported: [] },
    captureFailure: { reason: 'Error: boom at /Users/someone/secret' },
  });
  assert.equal(leaky.code, 'timeout');
  assert.equal(leaky.detail, '捕获失败，原因未明');
  assert.ok(!String(leaky.detail).includes('secret'), '内部异常串不许进 detail');
});

test('§1.2：目录页是独立缓存物，**不含正文**', () => {
  const page = {
    accountScope: 'scope',
    providerId: 'official-chat',
    items: [
      {
        sessionId: 's1',
        title: '标题',
        updatedAt: '2026-10-20T08:59:00.000Z',
        deepLink: 'https://example.invalid/chat/s1',
      },
    ],
    cursor: 'c-1',
    hasMore: false,
    fetchedAt: '2026-10-20T09:00:00.000Z',
  };
  assert.deepEqual(validateCatalogPage(page), { ok: true, errors: [] });
  const withBody = {
    ...page,
    items: [{ ...page.items[0], body: '偷偷带正文' }],
  };
  const check = validateCatalogPage(withBody);
  assert.equal(check.ok, false, '目录项带正文要被拒');
  assert.ok(check.errors.some((error) => error.includes('不含正文')));
});

test('§1.3：规范化只去整段末尾空白，保留正文内空行与缩进', () => {
  const raw = '第一行\r\n\r\n  缩进行  \r\n';
  const digest = contentDigest(raw);
  assert.match(digest, /^[0-9a-f]{64}$/);
  // 整段**末尾**的空白被规范化掉 → 追加尾部空白不改变摘要。
  assert.equal(contentDigest(raw), contentDigest(`${raw}   `));
  // 内容真的变了 → 摘要必须变（否则摘要就白算了）。
  assert.notEqual(contentDigest(raw), contentDigest(`${raw}x`));
  // 正文**中间**的空行与缩进不参与规范化 → 逐行去尾随空白会改掉的那些内容，
  // 这里必须原样保留（协议写的是「不得改动语义」）。
  const withInnerBlank = '第一行\r\n\r\n  缩进行  \r\n';
  const withoutInnerBlank = '第一行\r\n缩进行  \r\n';
  assert.notEqual(contentDigest(withInnerBlank), contentDigest(withoutInnerBlank));
});

test('§3 v0.4：错误结果对象 = 8 字段闭集，全部必备（G-2：JS 不得再缺 kind）', () => {
  // **此前 JS 侧只产 7 个**（缺 `kind`）——那是 **JS 漏字段、不是协议写错**
  // （协议闭集不动，实现补齐）⇒ 这里把闭集钉死：谁再省一个字段，当场红。
  const payload = errorPayload('timeout', {
    requestId: 'r-1',
    detail: '线索',
    logPath: '/tmp/x.log',
  });
  assert.deepEqual(
    Object.keys(payload).sort(),
    ['code', 'detail', 'kind', 'logPath', 'ok', 'requestId', 'retryable', 'userNext'],
    '错误负载必须恰好是 8 个字段（§3 v0.4 闭集，不省略）',
  );
  assert.equal(payload.kind, 'error', '错误负载的 kind 恒为 error');
  assert.equal(payload.ok, false, '错误路径的 ok 恒为 false');
  // **未知码的归一分支走同一条闭集** ⇒ 同样不得省字段。
  const unknown = errorPayload('not_a_real_code');
  assert.equal(Object.keys(unknown).length, 8, '未知码分支也必须 8 个字段');
  assert.equal(unknown.code, 'snapshot_invalid', '未知码归一为 snapshot_invalid（不新增第 14 个码）');
  assert.match(String(unknown.detail), /not_a_real_code/, '原始码要留在 detail');
});
