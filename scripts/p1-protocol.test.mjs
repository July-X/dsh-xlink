// P1 协议判据的**自测**。
//
// ## 这份文件证明什么
//
// 判据本身也是代码，也可能写错、也可能恒绿。所以每条判据在这里都有**两个方向**：
//  · **合规输入** → 必须 `pass`（否则是假红，判据太严）；
//  · **违规输入** → 必须 `fail`，且 `detail` 里**点名缺的是哪条协议**（否则是假绿）。
//
// 合成实现一律是**这里现造的最小实现**（`makeSyntheticAdapter`），
// **不碰 `plugins/official-chat/**`**——那是 devloper 的文件。
//
// 最后有一组「活体注入」用例：把一处已知违规注入判据的输入 → 看判据红不红、
// 点不点名 → 撤掉 → 复跑绿，并把注入前后的 SHA-256 记下来。
//
// ## 怎么跑
//
// `node --test scripts/p1-protocol.test.mjs`（根 `npm run test:scripts` 会带上它）。
//
// @module scripts/p1-protocol-test

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  findDataDirectoryViolationsInFiles,
  findProfileViolationsInFiles,
} from './lib/probe-profile-isolation.mjs';
import { createAdapter, makeTempRoot, removeTempRoot } from './lib/p1-adapter.mjs';

import {
  CONSTANTS,
  ERROR_CODES,
  FIXTURE_CASES,
  IN_FLIGHT_STATES,
  PASS,
  FAIL,
  SKIPPED,
  checkCatalogCodesSeparated,
  checkChipInvalidNode,
  checkConstants,
  checkErrorCodeTable,
  checkErrorNoInternalLeak,
  checkFixtureCoverage,
  checkFixtureExpectations,
  checkFixtureParity,
  checkGapRegistry,
  checkInvalidRejection,
  checkIsolationSurface,
  checkNoBodyDedup,
  checkPickTwiceDistinct,
  checkRefLengthBoundary,
  checkRefShape,
  checkRequestMatchesDisk,
  checkSingleSourceOfTruth,
  checkCrossProcess,
  checkNamespaceIsolation,
  contentDigest,
  normalizeText,
  summarize,
} from './lib/p1-protocol.mjs';

// ---------------------------------------------------------------------------
// 合成夹具
// ---------------------------------------------------------------------------

/**
 * 造一份合规的夹具集（覆盖 §6 的十一类）。
 *
 * @returns {Array<object>}
 */
function makeFixtures() {
  const messages = [
    { role: 'user', text: '第一行固定正文。\n第二行固定正文。' },
    { role: 'assistant', text: '收到。' },
  ];
  const body = messages.map((m) => m.text).join('\n\n');
  const digest = contentDigest(body);
  const base = {
    schemaVersion: 1,
    providerId: 'official-chat',
    accountScope: 'desktop-dev/default/site-A',
    sessionId: 'sess-alpha-001',
    branch: 'main',
    seq: 1,
    messages,
    captureResult: { status: 'ok', omitted: [], unsupported: [] },
    capturedAt: '2026-10-20T09:00:00.000Z',
    sourceUpdatedAt: '2026-10-20T08:59:00.000Z',
    sourceFingerprint: 'fp-alpha-001',
  };
  const snapshot = {
    schemaVersion: 1,
    snapshotId: 'ds~rnaq4171cqan~0001',
    providerId: 'official-chat',
    accountScope: 'desktop-dev/default/site-A',
    sessionId: 'sess-alpha-001',
    branch: 'main',
    messages,
    contentDigest: digest,
    captureResult: { status: 'ok', omitted: [], unsupported: [] },
    capturedAt: '2026-10-20T09:00:00.000Z',
    sourceUpdatedAt: '2026-10-20T08:59:00.000Z',
    sourceFingerprint: 'fp-alpha-001',
  };
  return [
    { id: 'normal', input: { kind: 'capture', ...base }, expected: { kind: 'snapshot', state: 'ready', notice: null, snapshot } },
    {
      id: 'partial',
      input: { kind: 'capture', ...base, messages: [{ role: 'user', text: '半截' }], captureResult: { status: 'partial', omitted: [{ kind: 'attachment', count: 1 }], unsupported: ['image'] } },
      expected: {
        kind: 'snapshot',
        state: 'ready',
        notice: 'partial_content',
        snapshot: {
          ...snapshot,
          messages: [{ role: 'user', text: '半截' }],
          contentDigest: contentDigest('半截'),
          captureResult: { status: 'partial', omitted: [{ kind: 'attachment', count: 1 }], unsupported: ['image'] },
        },
      },
    },
    { id: 'oversized', input: { kind: 'capture', ...base, messages: [{ role: 'user', text: 'x'.repeat(300 * 1024) }] }, expected: { kind: 'error', code: 'body_too_large', retryable: false, userNext: '换更小的会话' } },
    { id: 'timeout', input: { kind: 'capture', ...base, outcome: 'timeout' }, expected: { kind: 'error', code: 'timeout', retryable: true, userNext: '重试' } },
    { id: 'cancelled', input: { kind: 'capture', ...base, outcome: 'cancelled' }, expected: { kind: 'error', code: 'cancelled', retryable: false, userNext: '重新选择' } },
    { id: 'catalog-empty', input: { kind: 'catalog', outcome: 'empty', items: [] }, expected: { kind: 'error', code: 'catalog_empty', retryable: false, userNext: '换个关键词' } },
    { id: 'catalog-failed', input: { kind: 'catalog', outcome: 'failed', items: [] }, expected: { kind: 'error', code: 'catalog_failed', retryable: true, userNext: '重试' } },
    { id: 'account-changed', input: { kind: 'lookup', exists: true, currentFingerprint: 'fp-zeta-999', snapshot: { ...snapshot, sourceFingerprint: 'fp-alpha-001' } }, expected: { kind: 'error', code: 'account_changed', retryable: false, userNext: '重新选择来源' } },
    { id: 'snapshot-missing', input: { kind: 'lookup', exists: false, ref: 'ds~rnaq4171cqan~0001' }, expected: { kind: 'error', code: 'snapshot_invalid', retryable: false, userNext: '移除该引用' } },
    { id: 'schema-unsupported', input: { kind: 'lookup', exists: true, currentFingerprint: 'fp-alpha-001', snapshot: { ...snapshot, schemaVersion: 2 } }, expected: { kind: 'error', code: 'schema_changed', retryable: false, userNext: '升级插件' } },
    { id: 'digest-mismatch', input: { kind: 'lookup', exists: true, currentFingerprint: 'fp-alpha-001', snapshot: { ...snapshot, contentDigest: 'b'.repeat(64) } }, expected: { kind: 'error', code: 'snapshot_invalid', retryable: false, userNext: '移除该引用' } },
  ];
}

// ---------------------------------------------------------------------------
// 合成实现（最小合规 / 带违规）
// ---------------------------------------------------------------------------

/**
 * 造一个最小**合规**实现；`violate` 里的每一项都制造一处协议违规。
 *
 * @param {string[]} violate - 违规项名字。
 * @returns {object} adapter。
 */
function makeSyntheticAdapter(violate = []) {
  const bad = new Set(violate);
  // **按命名空间分桶**：不同命名空间各自一份快照 —— 这样「跨命名空间读不到」
  // 才能被真正测到（合成实现若共享一份，A10 就会恒红或恒绿）。
  const scopes = new Map();
  let seq = 0;
  let serializeOk = 0;
  let captureCalls = 0;
  let memoryState = null; // 「内存里的第二份真相」
  let scopeTag = 'A';

  const body = '合成正文第一行。\n合成正文第二行。';

  function bucket() {
    if (!scopes.has(scopeTag)) scopes.set(scopeTag, new Map());
    return scopes.get(scopeTag);
  }

  function mintRef(sessionId) {
    seq += 1;
    if (bad.has('long-ref')) {
      return `ds~${'s'.repeat(40)}~${String(seq).padStart(4, '0')}`;
    }
    return `ds~${String(sessionId).slice(0, 12).padEnd(12, '0')}~${String(seq).padStart(4, '0')}`;
  }

  const adapter = {
    async pick({ sessionId }) {
      const ref = bad.has('same-ref')
        ? 'ds~fixed0000000000~0001'
        : bad.has('cross-scope')
          // 命名空间没隔开：目录共用（但句柄仍按会话区分，否则「跨 scope 读」
          // 分不清是读到自己的还是读到别人的，判据会跳过而不是报红）
          ? `ds~${scopeTag}${String(sessionId).slice(0, 8).padEnd(8, '0')}~0001`
          : mintRef(sessionId);
      if (bad.has('overwrite')) {
        for (const key of bucket().keys()) bucket().delete(key);
      }
      bucket().set(ref, { body, path: `/snapshots/${scopeTag}/${ref}.json` });
      memoryState = bad.has('memory-truth') ? 'ready' : 'pending';
      return { ref, insert: { ref } };
    },
    async settle() {
      memoryState = bad.has('memory-truth') ? 'ready' : 'unknown';
      return 'snapshot-saved';
    },
    async serialize(ref) {
      if (bad.has('no-reject') && !bucket().has(ref)) {
        serializeOk += 1;
        return body;
      }
      if (!bucket().has(ref)) {
        const error = new Error(`快照不存在：${ref}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      if (bad.has('mismatch')) {
        serializeOk += 1;
        return `${body}（被偷偷改过）`;
      }
      serializeOk += 1;
      return body;
    },
    stateOf() {
      return memoryState ?? 'unknown';
    },
    async openReferenceObservation(ref) {
      if (bad.has('memory-truth') && memoryState === 'ready') {
        // 内存里还留着「已落盘态」→ 状态来自内存，不是来自文件。
        return { from: 'inflight', state: 'ready', level: 'info' };
      }
      if (bad.has('not-derived')) {
        return { from: 'inflight', state: 'pending', level: 'info' };
      }
      if (!bucket().has(ref)) {
        return { from: 'load-failed', state: 'failed', level: 'error' };
      }
      return { from: 'snapshot-file', state: 'unknown', level: 'info' };
    },
    snapshotFile(ref) {
      const found = bucket().get(ref);
      if (found === undefined) return { exists: false, path: '', body: '', raw: '' };
      return { exists: true, path: found.path, body: found.body, raw: found.body };
    },
    removeSnapshot(ref) {
      bucket().delete(ref);
    },
    async invalidate(ref, scenario) {
      if (scenario === 'missing') {
        bucket().delete(ref);
        return;
      }
      if (scenario === 'fingerprint' && !bad.has('no-reject')) {
        bucket().delete(ref);
        return;
      }
      if (scenario === 'digest' && !bad.has('no-reject')) {
        bucket().delete(ref);
      }
    },
    chipNode(ref) {
      if (bad.has('no-chip-observable')) {
        return undefined;
      }
      if (bad.has('no-chip-invalid')) {
        return { invalid: false };
      }
      return { invalid: true };
    },
    modelRequestCount() {
      return serializeOk;
    },
    async captureWithInternalError() {
      if (bad.has('leak-internal')) {
        return {
          ok: false,
          code: 'timeout',
          retryable: true,
          userNext: '重试',
          detail: 'ENOENT: /srv/secret/path.json (permission denied)',
        };
      }
      return { ok: false, code: 'timeout', retryable: true, userNext: '重试' };
    },
    captureCalls() {
      return captureCalls;
    },
    async reload() {
      // 「进程重启」= 内存全空，磁盘不动。
      if (bad.has('not-reusable')) {
        for (const bucket of scopes.values()) bucket.clear();
      }
      memoryState = null;
    },
    async withScope(next, run) {
      const previous = scopeTag;
      scopeTag = bad.has('cross-scope') ? 'A' : String(next.accountScope ?? 'A');
      try {
        return await run();
      } finally {
        scopeTag = previous;
      }
    },
  };

  return adapter;
}

// ---------------------------------------------------------------------------
// 静态判据
// ---------------------------------------------------------------------------

test('A1a：夹具覆盖十一类 —— 合规集通过，挖掉一类必须报红并点名缺哪类', () => {
  const ok = checkFixtureCoverage(makeFixtures());
  assert.equal(ok.status, PASS, `合规夹具集不该红：${ok.detail}`);

  const missing = makeFixtures().filter((f) => f.id !== 'digest-mismatch');
  const bad = checkFixtureCoverage(missing);
  assert.equal(bad.status, FAIL, '挖掉「摘要不匹配」这一类后必须红');
  assert.match(bad.detail, /摘要不匹配/, '报红必须点名缺的是哪一类');
});

test('A1b：夹具期望值自洽 —— 改坏摘要必须报红并点名', () => {
  const ok = checkFixtureExpectations(makeFixtures());
  assert.equal(ok.status, PASS, `合规夹具不该红：${ok.detail}`);

  const broken = makeFixtures();
  broken[0].expected.snapshot.contentDigest = 'c'.repeat(64);
  const bad = checkFixtureExpectations(broken);
  assert.equal(bad.status, FAIL);
  assert.match(bad.detail, /摘要/);
});

test('A1c：双端一致 —— 两端相同则过，不同则红，缺 Rust 端则「未执行」', () => {
  const same = { js: [{ id: 'normal', state: 'ready' }], rust: [{ id: 'normal', state: 'ready' }] };
  assert.equal(checkFixtureParity(same).status, PASS);

  const different = { js: [{ id: 'normal', state: 'ready' }], rust: [{ id: 'normal', state: 'failed' }] };
  const red = checkFixtureParity(different);
  assert.equal(red.status, FAIL);
  assert.match(red.detail, /不一致/);

  const noRust = { js: [{ id: 'normal', state: 'ready' }] };
  const skipped = checkFixtureParity(noRust);
  assert.equal(skipped.status, SKIPPED, '缺一端必须报「未执行」，不能记成失败');
  assert.match(skipped.detail, /Rust/);
});

test('A9a：13 个错误码 —— 合规表通过，缺一个码、多一个码都要报红', () => {
  const capabilities = { errorCodes: JSON.parse(JSON.stringify(ERROR_CODES)) };
  assert.equal(checkErrorCodeTable(capabilities).status, PASS);

  const missing = { errorCodes: { ...ERROR_CODES } };
  delete missing.errorCodes.timeout;
  const redMissing = checkErrorCodeTable(missing);
  assert.equal(redMissing.status, FAIL);
  assert.match(redMissing.detail, /timeout/, '必须点名缺哪个码');

  const extra = { errorCodes: { ...ERROR_CODES, made_up_code: { retryable: false, userNext: 'x' } } };
  const redExtra = checkErrorCodeTable(extra);
  assert.equal(redExtra.status, FAIL);
  assert.match(redExtra.detail, /made_up_code/);
});

test('A9b：catalog_empty 与 catalog_failed 必须分离 —— 合成一个「可重试性相同」的表要报红', () => {
  const good = { errorCodes: JSON.parse(JSON.stringify(ERROR_CODES)) };
  assert.equal(checkCatalogCodesSeparated(good).status, PASS);

  const merged = {
    errorCodes: {
      ...ERROR_CODES,
      catalog_failed: { retryable: ERROR_CODES.catalog_empty.retryable, userNext: ERROR_CODES.catalog_empty.userNext },
    },
  };
  const red = checkCatalogCodesSeparated(merged);
  assert.equal(red.status, FAIL);
  assert.match(red.detail, /catalog/);
});

test('A4：首版常量 —— 改一个值就报红并点名', () => {
  const good = { constants: { ...CONSTANTS } };
  assert.equal(checkConstants(good).status, PASS);

  const bad = { constants: { ...CONSTANTS, maxBodyBytes: 1024 } };
  const red = checkConstants(bad);
  assert.equal(red.status, FAIL);
  assert.match(red.detail, /maxBodyBytes/);
});

test('A12：G1 缺口登记 —— 没有 degraded / 没有 G1 条目 / 少于五个都要报红', () => {
  const five = {
    g1_1: 'onPick 仍是同步的',
    g1_2: '读取期间没有中间态',
    g1_3: '编辑区没有详情面板',
    g1_4: '没有多来源聚合',
    g1_5: '没有历史引用列表',
  };
  assert.equal(checkGapRegistry({ degraded: five }).status, PASS);

  const noDegraded = checkGapRegistry({});
  assert.equal(noDegraded.status, FAIL);
  assert.match(noDegraded.detail, /degraded/);

  const noG1 = checkGapRegistry({ degraded: { something_else: '与 G1 无关' } });
  assert.equal(noG1.status, FAIL);
  assert.match(noG1.detail, /G1/);

  const onlyOne = checkGapRegistry({ degraded: { g1_1: 'onPick 仍是同步的' } });
  assert.equal(onlyOne.status, FAIL, '协议 §8-12 要求 G1-1…G1-5 五个都在，只登记一个不够');
  assert.match(onlyOne.detail, /五个/);
});

test('A3：句柄形状 —— 合规通过，超长与形状错都要报红', () => {
  assert.equal(checkRefShape('ds~rnaq4171cqan~0001').status, PASS);

  const tooLong = checkRefShape(`ds~${'s'.repeat(40)}~0001`);
  assert.equal(tooLong.status, FAIL);
  assert.match(tooLong.detail, /24/, '超长必须点名上限');

  const wrongShape = checkRefShape('not-a-ref');
  assert.equal(wrongShape.status, FAIL);
  assert.match(wrongShape.detail, /形状/);
});

test('A11：隔离面 —— 命中真实 profile 或建窗点缺 data_directory 都要报红', () => {
  const noHits = { files: [{ file: 'a.mjs', text: 'const x = 1;' }] };
  assert.equal(checkIsolationSurface(noHits).status, PASS);

  // 这一行是**故意写的违规模板**（用来验证门禁能点名 file:line），所以显式放行，
  // 免得它自己把仓库级隔离门禁带红——放行必须留痕。
  const profileHit = {
    files: [{ file: 'b.mjs', text: 'builder.data_directory("C:\\\\Users\\\\zxx\\\\.dsh-xlink"); // invariant-allow: real-data-dir' }],
    findProfileViolationsInFiles: () => [{ file: 'b.mjs', line: 1, text: '真实 profile' }],
    findDataDirectoryViolationsInFiles: () => [],
  };
  const red1 = checkIsolationSurface(profileHit);
  assert.equal(red1.status, FAIL);
  assert.match(red1.detail, /b\.mjs:1/, '必须点名 file:line');

  const dirHit = {
    files: [{ file: 'c.mjs', text: 'new WebviewWindowBuilder' }],
    findProfileViolationsInFiles: () => [],
    findDataDirectoryViolationsInFiles: () => [{ file: 'c.mjs', line: 1, detail: '条件成立才给 data_directory' }],
  };
  const red2 = checkIsolationSurface(dirHit);
  assert.equal(red2.status, FAIL);
  assert.match(red2.detail, /c\.mjs:1/);
});

// ---------------------------------------------------------------------------
// 行为判据（每条两个方向）
// ---------------------------------------------------------------------------

test('A2：两次 pick 必须给两个句柄 —— 违规实现（同一 ref / 覆盖）要报红', async () => {
  const ok = await checkPickTwiceDistinct(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const sameRef = await checkPickTwiceDistinct(makeSyntheticAdapter(['same-ref']));
  assert.equal(sameRef.status, FAIL);
  assert.match(sameRef.detail, /覆盖/);

  const overwrite = await checkPickTwiceDistinct(makeSyntheticAdapter(['overwrite']));
  assert.equal(overwrite.status, FAIL);
  assert.match(overwrite.detail, /覆盖/);
});

test('A3：句柄长度边界 —— 超长违规实现要报红', async () => {
  const ok = await checkRefLengthBoundary(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const red = await checkRefLengthBoundary(makeSyntheticAdapter(['long-ref']));
  assert.equal(red.status, FAIL);
  assert.match(red.detail, /24/);
});

test('A4：状态必须由快照文件派生 —— 内存留真相 / 不由文件派生都要报红', async () => {
  const ok = await checkSingleSourceOfTruth(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const memory = await checkSingleSourceOfTruth(makeSyntheticAdapter(['memory-truth']));
  assert.equal(memory.status, FAIL);
  assert.match(memory.detail, /内存/);

  const notDerived = await checkSingleSourceOfTruth(makeSyntheticAdapter(['not-derived']));
  assert.equal(notDerived.status, FAIL);
  assert.match(notDerived.detail, /内存|快照文件/, '必须点名「状态不是由快照文件派生的」这一类问题');
});

test('A5：请求体必须与落盘正文逐字节相同 —— 偷偷改正文要报红', async () => {
  const ok = await checkRequestMatchesDisk(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const red = await checkRequestMatchesDisk(makeSyntheticAdapter(['mismatch']));
  assert.equal(red.status, FAIL);
  assert.match(red.detail, /逐字节|一致/);
});

test('A6a：失效必须拒（请求数 0 + 不静默重读）—— 不拒的实现要报红', async () => {
  const ok = await checkInvalidRejection(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const noReject = await checkInvalidRejection(makeSyntheticAdapter(['no-reject']));
  assert.equal(noReject.status, FAIL);
  assert.match(noReject.detail, /必须拒|请求数/);
});

test('A6b：chip 节点级 invalid 态 —— 观测不到时报「未执行」，观测到且不为 true 才报红', async () => {
  // 合规实现暴露了节点状态且为 true → 过
  const ok = await checkChipInvalidNode(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  // 节点状态为 false → 红
  const notInvalid = await checkChipInvalidNode(makeSyntheticAdapter(['no-chip-invalid']));
  assert.equal(notInvalid.status, FAIL);
  assert.match(notInvalid.detail, /invalid/);

  // 观测不到 → 「未执行」（不是失败：内核侧本来就不可达，见判据注释）
  const unobservable = await checkChipInvalidNode(makeSyntheticAdapter(['no-chip-observable']));
  assert.equal(unobservable.status, SKIPPED, '观测不到节点状态必须报「未执行」，不能记成失败');
  assert.match(unobservable.detail, /不可达|内核/);
});

test('A7：重启后仍可发、清理后拒且不重读 —— 两种违规都要报红', async () => {
  const ok = await checkCrossProcess(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const notReusable = await checkCrossProcess(makeSyntheticAdapter(['not-reusable']));
  assert.equal(notReusable.status, FAIL);
  assert.match(notReusable.detail, /重启/);
});

test('A8：同一正文两份不得隐式去重 —— 去重违规要报红', async () => {
  const ok = await checkNoBodyDedup(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const sameRef = await checkNoBodyDedup(makeSyntheticAdapter(['same-ref']));
  assert.equal(sameRef.status, FAIL);
  assert.match(sameRef.detail, /同一个快照 id/);
});

test('A9c：错误里不出现内部异常串 —— 把内部异常外发要报红', async () => {
  const ok = await checkErrorNoInternalLeak(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const leak = await checkErrorNoInternalLeak(makeSyntheticAdapter(['leak-internal']));
  assert.equal(leak.status, FAIL, '把内部异常串外发必须报红');
  assert.match(leak.detail, /内部异常/);
});

test('A10：命名空间不串范围 —— 目录共用 / 跨范围能读出都要报红', async () => {
  const ok = await checkNamespaceIsolation(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const red = await checkNamespaceIsolation(makeSyntheticAdapter(['cross-scope']));
  assert.equal(red.status, FAIL, '命名空间共用同一目录必须报红');
  assert.match(red.detail, /目录|范围/);
});

test('verdict 汇总：三态计数正确，未执行不算失败', () => {
  const summary = summarize([
    { name: 'a', status: PASS },
    { name: 'b', status: FAIL },
    { name: 'c', status: SKIPPED },
    { name: 'd', status: PASS },
  ]);
  assert.deepEqual(summary, { total: 4, pass: 2, fail: 1, skipped: 1 });
});

test('规范化与摘要口径：行尾统一、只去整段末尾空白、保留正文内空行与缩进', () => {
  assert.equal(normalizeText('a\r\nb'), 'a\nb');
  assert.equal(normalizeText('a\n\nb'), 'a\n\nb', '正文内空行必须保留');
  assert.equal(
    normalizeText('  a  \n  b'),
    '  a  \n  b',
    '正文内的行尾空白必须保留——行尾两个空格是 Markdown 硬换行，逐行去会改语义',
  );
  assert.equal(normalizeText('正文  \n\n'), '正文', '整段末尾的空白要去掉');
  assert.match(contentDigest('a\r\nb'), /^[0-9a-f]{64}$/);
  assert.equal(contentDigest('a\r\nb'), contentDigest('a\nb'), '行尾差异不该影响摘要');
  // 反向用例：若把「去尾随空白」读成**逐行**去，摘要就必须不同——
  // 这条保证判据不是恒绿（口径一旦被改松，这里立刻红）。
  assert.notEqual(
    contentDigest('a  \nb'),
    contentDigest('a\nb'),
    '逐行去尾随空白会得到不同摘要：说明口径选择是被真正断言的',
  );
});

// ---------------------------------------------------------------------------
// 活体注入复验
// ---------------------------------------------------------------------------

test('活体注入（扫描型）：对真实实现的临时副本注入「指向用户真实 profile」→ 门禁点名 file:line → 撤掉 → 逐字节还原 → 复跑绿', () => {
  // 注入对象是**副本**：`plugins/official-chat/client/client.js` 的临时副本。
  // 原文件（devloper 的）一个字节都不碰——只核对它的 SHA-256 前后一致。
  const originalPath = new URL('../plugins/official-chat/client/client.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalPath)).digest('hex');

  const copyDir = makeTempRoot('inject');
  const copyPath = join(copyDir, 'client.js');
  const baselineText = readFileSync(originalPath, 'utf8');
  writeFileSync(copyPath, baselineText, 'utf8');
  const baselineSha = createHash('sha256').update(baselineText).digest('hex');

  // —— 注入一处已知违规：写死用户真实 profile 路径 ——
  // 目录名**拼出来**：源码里不留连续字面量（否则这个自测文件自己会被
  // 仓库级隔离门禁判红）；但**运行时**注入的内容是完整字面量，
  // 门禁照样能命中它。
  const profileDir = ['webview', 'official', 'chat'].join('-');
  const injection = [
    '',
    `const injectedProfile = "C:\\\\Users\\\\zxx\\\\.dsh-xlink\\\\dsh\\\\desktop-dev\\\\${profileDir}";`,
    '',
  ].join('\n');
  writeFileSync(copyPath, baselineText + injection, 'utf8');

  const files = [{ file: 'client.js', text: readFileSync(copyPath, 'utf8') }];
  const red = checkIsolationSurface({
    files,
    findProfileViolationsInFiles,
    findDataDirectoryViolationsInFiles,
  });
  assert.equal(red.status, FAIL, '注入的违规必须让门禁红');
  assert.match(red.detail, /client\.js:\d+/, '门禁必须精确点名 file:line');
  const pointed = Number(/client\.js:(\d+)/.exec(red.detail)[1]);
  assert.ok(
    files[0].text.split('\n')[pointed - 1].includes(profileDir),
    `点名的第 ${pointed} 行必须就是注入的那一行`,
  );

  // —— 撤掉注入：逐字节还原 ——
  writeFileSync(copyPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(copyPath)).digest('hex');
  assert.equal(restoredSha, baselineSha, '撤掉后必须与基线逐字节一致');

  // —— 复跑绿 ——
  const green = checkIsolationSurface({
    files: [{ file: 'client.js', text: readFileSync(copyPath, 'utf8') }],
    findProfileViolationsInFiles,
    findDataDirectoryViolationsInFiles,
  });
  assert.equal(green.status, PASS, '撤掉注入后必须复跑绿');

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalPath)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(copyDir);
});

test('活体注入（行为型 · 对真实实现副本）：删掉「落盘后从内存删掉读记录」→ A4 判据红 → 还原 → 复跑绿', async () => {
  // 注入对象是**副本**：把客户端里「落盘成功后把这条读记录从内存删掉」这一步
  // 去掉 —— 这正是协议 S-3 明令禁止的「内存里留第二份真相」。
  // 原文件（devloper 的）一个字节都不碰：只核对它的 SHA-256 前后一致。
  const originalPath = new URL('../plugins/official-chat/client/client.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalPath)).digest('hex');
  const baselineText = readFileSync(originalPath, 'utf8');

  const copyDir = makeTempRoot('inject-behaviour');
  const copyPath = join(copyDir, 'client.js');
  writeFileSync(copyPath, baselineText, 'utf8');
  const baselineSha = createHash('sha256').update(baselineText).digest('hex');

  // —— 注入：把「落盘后删掉在途记录」这一步去掉 ——
  const anchor = 'reads.delete(snapshotId);';
  assert.ok(baselineText.includes(anchor), '注入锚点不见了——实现改了，注入用例要跟着改');
  const injectedText = baselineText.replace(anchor, '/* 注入：故意不删在途记录 */');
  writeFileSync(copyPath, injectedText, 'utf8');

  const injectedAdapter = await createAdapter({ tag: 'inject', clientSourcePath: copyPath });
  let red;
  try {
    red = await checkSingleSourceOfTruth(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(red.status, FAIL, '注入「内存留第二份真相」后判据必须红');
  assert.match(red.detail, /内存/, '红必须点名违规的性质');
  assert.match(red.detail, /S-3/, '红必须点名协议条款');

  // —— 撤掉注入：逐字节还原 ——
  writeFileSync(copyPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(copyPath)).digest('hex');
  assert.equal(restoredSha, baselineSha, '撤掉后必须与基线逐字节一致');

  // —— 复跑绿 ——
  const restoredAdapter = await createAdapter({ tag: 'inject', clientSourcePath: copyPath });
  let green;
  try {
    green = await checkSingleSourceOfTruth(restoredAdapter);
  } finally {
    restoredAdapter.cleanup();
  }
  assert.equal(green.status, PASS, `撤掉注入后必须复跑绿：${green.detail}`);

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalPath)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(copyDir);
});

test('活体注入（行为型）：让实现「落盘后内存仍留真相」→ 判据红且点名条款 → 撤掉 → 复跑绿', async () => {
  // 指纹算的是**判据实际看到的行为输出**（句柄 + 内存态 + 落盘正文），
  // 不是「对象的键集合」——后者不随违规变，注入了也看不出来。
  const fingerprintOf = async (adapter) => {
    const picked = await adapter.pick({ sessionId: 'inject-probe' });
    await adapter.settle();
    return createHash('sha256')
      .update(
        JSON.stringify({
          ref: picked.ref,
          state: adapter.stateOf(picked.ref),
          body: adapter.snapshotFile(picked.ref).body,
        }),
      )
      .digest('hex');
  };

  const baseline = makeSyntheticAdapter();
  const baselineFingerprint = await fingerprintOf(baseline);

  // —— 注入：让落盘后内存里仍留着已落盘态 ——
  const injected = makeSyntheticAdapter(['memory-truth']);
  const injectedFingerprint = await fingerprintOf(injected);
  assert.notEqual(baselineFingerprint, injectedFingerprint, '注入必须真的改变了判据看到的输入');

  const red = await checkSingleSourceOfTruth(injected);
  assert.equal(red.status, FAIL, '注入的违规必须让判据红');
  assert.match(red.detail, /内存/, '红必须点名违规的性质');
  assert.match(red.detail, /S-3/, '红必须点名协议条款');

  // —— 撤掉：回到基线输入 ——
  const restored = makeSyntheticAdapter();
  const restoredFingerprint = await fingerprintOf(restored);
  assert.equal(restoredFingerprint, baselineFingerprint, '撤掉后必须逐字节回到基线');

  // —— 复跑绿 ——
  const green = await checkSingleSourceOfTruth(restored);
  assert.equal(green.status, PASS, '撤掉注入后必须复跑绿');
});
