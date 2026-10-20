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
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  findDataDirectoryViolationsInFiles,
  findProfileViolationsInFiles,
} from './lib/probe-profile-isolation.mjs';
import { createAdapter, makeTempRoot, removeTempRoot } from './lib/p1-adapter.mjs';
// **A17 / A18 的实跑**（在 `run.mjs` 里，因为要跑真实 `host/sweep.js` 的 `sweep()`）：
// import 进来做**活体注入**——往真实实现的副本里注入破坏，证明这条判据对实现的
// 破坏**敏感**（`run.mjs` 用 `import.meta.main` 守卫，import 时不跑整轮）。
import {
  runCacheTtl,
  runCapturedAtPriority,
  runLogNotAgeDeleted,
  runLogTruncation,
  runRetentionSweep,
  runRootUnavailable,
  runSweepFailureLogged,
  runSweepThrottle,
  runSweepTriggerBehaviour,
  runSweepTriggerStatic,
  runTwoStepDelete,
} from './p1-protocol.run.mjs';

import {
  CONSTANTS,
  ERROR_CODES,
  FIXTURE_CASES,
  IN_FLIGHT_STATES,
  PASS,
  FAIL,
  SKIPPED,
  checkRetentionSweep,
  checkCaptureResultLanded,
  checkCaptureResultShape,
  checkCacheTtl,
  checkCapturedAtPriority,
  checkCatalogCodesSeparated,
  checkChipInvalidNode,
  checkConstants,
  checkConstantsParity,
  checkErrorCodeClosedSet,
  checkErrorCodeTable,
  checkErrorLogPath,
  checkErrorNoInternalLeak,
  checkFingerprintGate,
  checkFixtureCoverage,
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
  checkPickTwiceDistinct,
  checkRefLengthBoundary,
  checkRefShape,
  checkRequestMatchesDisk,
  checkRootUnavailable,
  checkSeqCrossProcess,
  checkSingleSourceOfTruth,
  checkSweepFailureLogged,
  checkSweepThrottle,
  checkSweepTriggerBehaviour,
  checkSweepTriggerStatic,
  checkTwoStepDelete,
  checkCrossProcess,
  checkNamespaceIsolation,
  contentDigest,
  normalizeText,
  stripJsComments,
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
  // 「当前来源指纹」是否可得（协议 §S-5 v0.2：不可得 ⇒ 必须拒）。
  let fingerprintAvailable = true;
  // 当前指纹的值（快照里记的那个；变了就是「换账号」）。
  const CURRENT_FINGERPRINT = 'fp-current-0000';

  const body = '合成正文第一行。\n合成正文第二行。';

  /**
   * 会话短码（合成实现只要**形状**合规即可）。
   *
   * @param {string} sessionId
   * @returns {string}
   */
  function shortOf(sessionId) {
    return String(sessionId)
      .toLowerCase()
      .replace(/[^a-z0-9]/g, '')
      .slice(0, 12)
      .padEnd(12, '0');
  }

  /**
   * 造一条**失败**负载（协议 §3 的形态：码 + 可重试性 + 用户下一步 + requestId
   * + 脱敏日志路径 + detail）。
   *
   * @param {string} code
   * @param {object} [extra]
   * @returns {object}
   */
  function failurePayload(code, extra = {}) {
    return {
      ok: false,
      error: {
        code,
        message: `${code}：${extra.userNext ?? '看看'}`,
        details: {
          retryable: extra.retryable ?? false,
          userNext: extra.userNext ?? '看看',
          requestId: extra.requestId ?? 'req-synthetic',
          // 「脱敏日志路径」：不可得的违规模式把它置空。
          logPath: bad.has('no-log-path') ? null : '<instance>/logs/official-chat.log',
          detail: extra.detail ?? null,
        },
      },
    };
  }

  function bucket() {
    if (!scopes.has(scopeTag)) scopes.set(scopeTag, new Map());
    return scopes.get(scopeTag);
  }

  function mintRef(sessionId) {
    seq += 1;
    if (bad.has('long-ref')) {
      return `ds~${'s'.repeat(40)}~${String(seq).padStart(4, '0')}`;
    }
    // **会话段必须符合 S-1 的形状**（小写字母数字、≤ 12 字符）：真实实现用的是
    // base36 会话短码（`host/protocol.js` 的 `makeSessionShort`）。合成实现原来
    // 直接切原始会话 id（带连字符）→ 形状不合规 → 「句柄长度/形状」判据把
    // **合规实现**判红——那是判据自己的夹具不真，不是实现的缺口。
    const short = String(sessionId)
      .toLowerCase()
      .replace(/[^a-z0-9]/g, '')
      .slice(0, 12)
      .padEnd(12, '0');
    return `ds~${short}~${String(seq).padStart(4, '0')}`;
  }

  const adapter = {
    root: '/synthetic/instance-root',
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
      bucket().set(ref, {
        body,
        path: `/snapshots/${scopeTag}/${ref}.json`,
        fingerprint: CURRENT_FINGERPRINT,
        changed: false,
      });
      memoryState = bad.has('memory-truth') ? 'ready' : 'pending';
      return { ref, insert: { ref }, sessionId: String(sessionId) };
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
      const entry = bucket().get(ref);
      if (entry === undefined) {
        const error = new Error(`快照不存在：${ref}`);
        error.code = 'snapshot_invalid';
        throw error;
      }
      // 指纹变了 ⇒ 命令层拒（协议 §S-5 v0.2）。
      if (entry.changed && !bad.has('no-reject')) {
        const error = new Error('account_changed');
        error.code = 'account_changed';
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
      return {
        exists: true,
        path: found.path,
        body: found.body,
        raw: found.body,
        // 落盘快照的 `captureResult`（协议 §1.1 v0.2：必须是对象、三字段齐）。
        snapshot: {
          captureResult: bad.has('bad-capture-result')
            ? 'partial'
            : { status: 'ok', omitted: [], unsupported: [] },
        },
      };
    },
    removeSnapshot(ref) {
      bucket().delete(ref);
    },
    async invalidate(ref, scenario) {
      if (scenario === 'missing') {
        bucket().delete(ref);
        return;
      }
      if (scenario === 'fingerprint') {
        // 指纹变了：**不删**快照（存储层不比对指纹，只校验 schemaVersion 与
        // contentDigest）——命令层才拒。
        const entry = bucket().get(ref);
        if (entry !== undefined) entry.changed = true;
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

    // —— v0.2/v0.3 新判据需要的观测口 ——

    /**
     * 按**真实 RPC 通道**调一个 host 命令，返回原始信封。
     *
     * @param {string} endpoint
     * @param {object} payload
     * @returns {Promise<object>}
     */
    async callHost(endpoint, payload = {}) {
      const requestId = String(payload.requestId ?? 'req-synthetic');
      if (endpoint === 'catalog') {
        return failurePayload('catalog_failed', { requestId, retryable: true, userNext: '重试' });
      }
      if (endpoint === 'preview') {
        // 当前指纹不可得 ⇒ 拒；指纹变了 ⇒ 拒（都在命令层）。
        const ref = String(payload.snapshotId ?? '');
        const entry = bucket().get(ref);
        const changed = entry !== undefined && entry.changed;
        if (!fingerprintAvailable || changed) {
          if (bad.has('fingerprint-gate-missing')) {
            return { ok: true, value: { ok: true, snapshot: {}, body: body } };
          }
          return failurePayload('account_changed', { requestId, userNext: '重新选择来源' });
        }
        return { ok: true, value: { ok: true, snapshot: {}, body } };
      }
      if (endpoint === 'capture') {
        const id = String(payload.snapshotId ?? '');
        if (!fingerprintAvailable) {
          if (bad.has('fingerprint-gate-missing')) {
            return { ok: true, value: { ok: true, snapshot: {} } };
          }
          return failurePayload('account_changed', { requestId, userNext: '重新选择来源' });
        }
        if (bad.has('allow-unclaimed') && id.endsWith('~9999')) {
          return { ok: true, value: { ok: true, snapshot: {} } };
        }
        // **内部冲突码漏到协议外**（§3 闭集之外）。
        if (bad.has('extra-code')) {
          return failurePayload('snapshot_exists', { requestId });
        }
        if (id === '这不是快照id' || id.startsWith('zz~') || id.endsWith('~9999')) {
          return failurePayload('snapshot_invalid', { requestId, detail: 'id 不合规' });
        }
        // 同 id 重复落盘：归 snapshot_invalid，原始码留在 detail。
        if (bad.has('lose-original-code')) {
          return failurePayload('snapshot_invalid', { requestId, detail: null });
        }
        return failurePayload('snapshot_invalid', { requestId, detail: 'snapshot_exists' });
      }
      return failurePayload('source_unsupported', { requestId });
    },

    /**
     * 用一个 host 从未认领过的 id 调 `capture`。
     *
     * @param {string} sessionId
     * @returns {Promise<{id: string, claimed: boolean, result: object}>}
     */
    async captureUnclaimedId(sessionId) {
      const id = `ds~${shortOf(sessionId)}~9999`;
      const result = await adapter.callHost('capture', {
        sessionId: String(sessionId),
        snapshotId: id,
        requestId: 'req-unclaimed',
      });
      return { id, claimed: false, result };
    },

    /**
     * 「另一个进程」认领到的序号。
     *
     * @param {string} sessionId
     * @returns {Promise<{seq: number, snapshotId: string}>}
     */
    async reserveInChildProcess(sessionId) {
      const seq = bad.has('seq-collision') ? 1 : 2;
      return { seq, snapshotId: `ds~${shortOf(sessionId)}~${String(seq).padStart(4, '0')}` };
    },

    /**
     * 在「当前来源指纹不可得」的状态下跑一段逻辑。
     *
     * @param {() => Promise<unknown>} run
     * @returns {Promise<unknown>}
     */
    async withUnavailableFingerprint(run) {
      const saved = fingerprintAvailable;
      fingerprintAvailable = false;
      try {
        return await run();
      } finally {
        fingerprintAvailable = saved;
      }
    },

    /**
     * 在「目录读取失败」的状态下跑一段逻辑。
     *
     * @param {() => Promise<unknown>} run
     * @returns {Promise<unknown>}
     */
    async withFailingCatalog(run) {
      return run();
    },

    /**
     * 存储层读一份快照（**不比对指纹**）。
     *
     * @param {string} ref
     * @returns {{loaded: boolean, error: string, fingerprint: string}}
     */
    storeLoad(ref) {
      const entry = bucket().get(String(ref));
      if (entry === undefined) {
        return { loaded: false, error: 'snapshot_invalid', fingerprint: '' };
      }
      if (entry.changed && bad.has('storage-compares-fingerprint')) {
        return { loaded: false, error: 'account_changed', fingerprint: '' };
      }
      return { loaded: true, error: '', fingerprint: entry.fingerprint };
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

test('A12：G1 缺口登记 —— 没有 degraded / 没有 G1 条目 / 缺编号都要报红', () => {
  // **协议 v0.4 §8-12：G1-1…G1-7 七个**（G1-6 = 内核无「chip 进入 `invalid` 态」
  // 的写入口；G1-7 = 通道被上游一行卡住）。自测的合成夹具必须跟着口径走，否则
  // 「合规实现」会被判红——那就成了判据自己制造的假红。
  const seven = {
    g1_1: 'onPick 仍是同步的',
    g1_2: '读取期间没有中间态',
    g1_3: '编辑区没有详情面板',
    g1_4: '没有多来源聚合',
    g1_5: '没有历史引用列表',
    g1_6: '内核没有 chip invalid 写入口',
    g1_7: '通道被上游一行卡住',
  };
  assert.equal(checkGapRegistry({ degraded: seven }).status, PASS);

  const noDegraded = checkGapRegistry({});
  assert.equal(noDegraded.status, FAIL);
  assert.match(noDegraded.detail, /degraded/);

  const noG1 = checkGapRegistry({ degraded: { something_else: '与 G1 无关' } });
  assert.equal(noG1.status, FAIL);
  assert.match(noG1.detail, /G1/);

  const onlyOne = checkGapRegistry({ degraded: { g1_1: 'onPick 仍是同步的' } });
  assert.equal(onlyOne.status, FAIL, '协议 §8-12 要求 G1-1…G1-7 七个都在，只登记一个不够');
  assert.match(onlyOne.detail, /七个/);
  assert.match(onlyOne.detail, /G1-2/, '必须点名缺哪些编号，不能只说「不够」');

  // **反向回归（v0.4 新增）**：v0.2 的六个（G1-1…G1-6）**不够**了——
  // 协议 v0.4 已把「通道被上游一行卡住」收进缺口清单（G1-7），实现侧须登记
  // `capabilities.degraded.g1_7` 让通道不可用时**显式降级可见**、不静默。
  // ⇒ 只登记六个必须报红，且**点名 G1-7**（点名才能定位到是「少登了通道那条」）。
  const sixOnly = { ...seven };
  delete sixOnly.g1_7;
  const red = checkGapRegistry({ degraded: sixOnly });
  assert.equal(red.status, FAIL, '只登记 G1-1…G1-6 六个不够——协议 v0.4 要求七个');
  assert.match(red.detail, /G1-7/);

  // **反向回归**：空说明也要报红（登记了编号但没写清楚 = 没登记）。
  const emptyNote = checkGapRegistry({ degraded: { ...seven, g1_7: '   ' } });
  assert.equal(emptyNote.status, FAIL);
  assert.match(emptyNote.detail, /g1_7/);
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

test('A0s：host 半边必须注册 exact fetch route、不得用 rpc.handle、不得用 ctx.set 挂服务 —— 三条违规都要报红', () => {
  // **合规**（协议 v0.4 §5 的通道形态）：注册 `/api` 命名空间下的 exact route。
  const good = [
    "const inject = ['connection'];",
    "const FETCH_ROUTE_PATH = '/api/xlink-official-chat';",
    'ctx.connection.fetch.register({',
    "  kind: 'exact',",
    '  path: FETCH_ROUTE_PATH,',
    "  methods: ['POST'],",
    '});',
  ].join('\n');
  const ok = checkHostInjection({ file: 'host/index.js', text: good });
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  // **反向用例 1：改用 `rpc.handle`**（v0.4 已证伪的那条路——插件侧必抛）。
  const withRpcHandle = [
    "const inject = ['connection'];",
    "const FETCH_ROUTE_PATH = '/api/xlink-official-chat';",
    "ctx.connection.rpc.handle('/xlink-official-chat', handler);",
  ].join('\n');
  const redRpcHandle = checkHostInjection({ file: 'host/index.js', text: withRpcHandle });
  assert.equal(redRpcHandle.status, FAIL, '用 rpc.handle 必须报红');
  assert.match(redRpcHandle.detail, /rpc\.handle/, '必须点名用的是 rpc.handle');
  assert.match(redRpcHandle.detail, /exact fetch route/, '必须点名该注册的是 exact fetch route');

  // **反向用例 2：根本没注册 exact route**。
  const noRoute = "const inject = ['connection'];\n";
  const redNoRoute = checkHostInjection({ file: 'host/index.js', text: noRoute });
  assert.equal(redNoRoute.status, FAIL, '没注册 exact fetch route 必须报红');
  assert.match(redNoRoute.detail, /exact fetch route/, '必须点名缺的是 exact fetch route');

  // **反向用例 3：路径不在 `/api` 命名空间下**（内核 `assertFetchRoute` 会抛）。
  const badPath = [
    "const inject = ['connection'];",
    "const FETCH_ROUTE_PATH = '/xlink-official-chat';",
    'ctx.connection.fetch.register({ kind: \'exact\', path: FETCH_ROUTE_PATH });',
  ].join('\n');
  const redPath = checkHostInjection({ file: 'host/index.js', text: badPath });
  assert.equal(redPath.status, FAIL, '路径不在 /api 下必须报红');
  assert.match(redPath.detail, /\/api/, '必须点名是 /api 命名空间这一条');

  // **反向用例 4：改用 cordis `ctx.set` 挂服务**（协议 §5 已显式否决那条路）。
  const withCtxSet = [
    "const inject = ['connection'];",
    "const FETCH_ROUTE_PATH = '/api/xlink-official-chat';",
    'ctx.connection.fetch.register({ kind: \'exact\', path: FETCH_ROUTE_PATH });',
    "ctx.set('xlink-official-chat', service);",
  ].join('\n');
  const redCtxSet = checkHostInjection({ file: 'host/index.js', text: withCtxSet });
  assert.equal(redCtxSet.status, FAIL, '用 ctx.set 挂服务必须报红');
  assert.match(redCtxSet.detail, /ctx\.set/, '必须点名是 ctx.set 那条路');
  assert.match(redCtxSet.detail, /host\/index\.js:\d+/, '必须点名 file:line');

  // **注释里提到 `ctx.set` 不算违规**（实现文件里大量注释正是在解释为什么不能用它）。
  const onlyComment = [
    "const inject = ['connection'];",
    "const FETCH_ROUTE_PATH = '/api/xlink-official-chat';",
    'ctx.connection.fetch.register({ kind: \'exact\', path: FETCH_ROUTE_PATH });',
    '// 不用 ctx.set 挂服务：cordis 要求先 provide',
  ].join('\n');
  assert.equal(
    checkHostInjection({ file: 'host/index.js', text: onlyComment }).status,
    PASS,
    '注释里的 ctx.set 不能被当成调用（否则是假红）',
  );
});

test('A1e：两端常量与码表必须一致 —— 不一致要报红，缺一端要报「未执行」', () => {
  const js = {
    schemaVersion: 1,
    constants: { ...CONSTANTS },
    errorCodes: JSON.parse(JSON.stringify(ERROR_CODES)),
  };
  const rust = {
    schemaVersion: 1,
    constants: { ...CONSTANTS },
    errorCodes: JSON.parse(JSON.stringify(ERROR_CODES)),
  };
  const ok = checkConstantsParity({ js, rust });
  assert.equal(ok.status, PASS, `两端一致不该红：${ok.detail}`);

  // **常量不一致**。
  const badConst = { ...rust, constants: { ...rust.constants, maxBodyBytes: 1024 } };
  const redConst = checkConstantsParity({ js, rust: badConst });
  assert.equal(redConst.status, FAIL);
  assert.match(redConst.detail, /maxBodyBytes/);

  // **码表缺一个**。
  const badCodes = { ...rust, errorCodes: { ...rust.errorCodes } };
  delete badCodes.errorCodes.timeout;
  const redCodes = checkConstantsParity({ js, rust: badCodes });
  assert.equal(redCodes.status, FAIL);
  assert.match(redCodes.detail, /timeout/);

  // **缺 Rust 端 ⇒ 「未执行」**（不能记成失败，也不能记成通过）。
  const skipped = checkConstantsParity({ js, rust: undefined });
  assert.equal(skipped.status, SKIPPED, '缺一端必须报「未执行」');
  assert.match(skipped.detail, /Rust/);
});

test('A9d：错误负载必须带脱敏日志路径与 requestId —— 不带日志路径要报红', async () => {
  const good = makeSyntheticAdapter();
  const ok = await checkErrorLogPath(good);
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const bad = makeSyntheticAdapter(['no-log-path']);
  const red = await checkErrorLogPath(bad);
  assert.equal(red.status, FAIL, '错误负载没带脱敏日志路径必须报红');
  assert.match(red.detail, /日志路径/);
});

test('A13a：id 由 host 分配且跨进程单调 —— 跨进程撞号必须报红', async () => {
  const good = makeSyntheticAdapter();
  const ok = await checkSeqCrossProcess(good);
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const bad = makeSyntheticAdapter(['seq-collision']);
  const red = await checkSeqCrossProcess(bad);
  assert.equal(red.status, FAIL, '跨进程撞到同一个 id 必须报红');
  assert.match(red.detail, /同一个 id|单调/);
});

test('A13b：host 必须拒「client 自造」的 id —— 放行要报红', async () => {
  const good = makeSyntheticAdapter();
  const ok = await checkHostRejectsUnclaimedId(good);
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const bad = makeSyntheticAdapter(['allow-unclaimed']);
  const red = await checkHostRejectsUnclaimedId(bad);
  assert.equal(red.status, FAIL, 'host 放行未认领的 id 必须报红');
  assert.match(red.detail, /自造|没分配/);
});

test('A14：13 个码是闭集 —— 漏出协议外的码 / 丢掉原始码都要报红', async () => {
  const good = makeSyntheticAdapter();
  const ok = await checkErrorCodeClosedSet(good);
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const extra = makeSyntheticAdapter(['extra-code']);
  const redExtra = await checkErrorCodeClosedSet(extra);
  assert.equal(redExtra.status, FAIL, '外发闭集之外的码必须报红');
  assert.match(redExtra.detail, /snapshot_exists/, '必须点名漏出来的那个码');

  const lost = makeSyntheticAdapter(['lose-original-code']);
  const redLost = await checkErrorCodeClosedSet(lost);
  assert.equal(redLost.status, FAIL, '内部码没留在 detail 必须报红');
  assert.match(redLost.detail, /detail/);
});

test('A15：指纹闸在 host 命令层 —— 不可得不放行 / 存储层比对指纹都要报红', async () => {
  const good = makeSyntheticAdapter();
  const ok = await checkFingerprintGate(good);
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const noGate = makeSyntheticAdapter(['fingerprint-gate-missing']);
  const redGate = await checkFingerprintGate(noGate);
  assert.equal(redGate.status, FAIL, '指纹不可得却放行必须报红');
  assert.match(redGate.detail, /指纹/);

  const storeGate = makeSyntheticAdapter(['storage-compares-fingerprint']);
  const redStore = await checkFingerprintGate(storeGate);
  assert.equal(redStore.status, FAIL, '存储层比对指纹必须报红（校验点在命令层）');
  assert.match(redStore.detail, /存储层/);
});

test('A16a：captureResult 的形态 —— partial/failed 用字符串必须报红', () => {
  const ok = checkCaptureResultShape(makeFixtures());
  assert.equal(ok.status, PASS, `合规夹具不该红：${ok.detail}`);

  const asString = makeFixtures();
  asString[0].input.captureResult = 'partial';
  const redString = checkCaptureResultShape(asString);
  assert.equal(redString.status, FAIL, 'partial 用字符串必须报红');
  assert.match(redString.detail, /partial/);

  const missingField = makeFixtures();
  delete missingField[0].input.captureResult.omitted;
  const redMissing = checkCaptureResultShape(missingField);
  assert.equal(redMissing.status, FAIL, '缺 omitted 字段必须报红');
  assert.match(redMissing.detail, /omitted/);
});

test('A16b：落盘快照的 captureResult 必须是对象 —— 字符串形态必须报红', async () => {
  const good = makeSyntheticAdapter();
  const ok = await checkCaptureResultLanded(good);
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const bad = makeSyntheticAdapter(['bad-capture-result']);
  const red = await checkCaptureResultLanded(bad);
  assert.equal(red.status, FAIL, '落盘快照的 captureResult 是字符串必须报红');
  assert.match(red.detail, /对象/);
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

// ---------------------------------------------------------------------------
// v0.2 / v0.3 新判据的**活体注入**（对真实实现的副本，原实现一个字节都不碰）
// ---------------------------------------------------------------------------

/**
 * 把 host 半边整个目录拷到一次性临时目录（活体注入用）。
 *
 * **为什么拷整个目录**：host 内部那几个模块是**相对 import** 的
 * （`commands.js` import `./protocol.js`）——只拷一个文件会断 import。
 *
 * @param {string} tag
 * @returns {string} 副本目录绝对路径。
 */
function copyHostDir(tag) {
  const src = new URL('../plugins/official-chat/host/', import.meta.url);
  const dir = makeTempRoot(tag);
  for (const name of readdirSync(src)) {
    writeFileSync(join(dir, name), readFileSync(new URL(name, src)));
  }
  return dir;
}

test('活体注入（静态 · A0s）：从真实 host 半边副本里把 exact fetch route 改回 rpc.handle → 门禁红并点名 → 还原 → 逐字节一致 → 复跑绿', () => {
  const originalUrl = new URL('../plugins/official-chat/host/index.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  const baselineText = readFileSync(originalUrl, 'utf8');

  // **拷整个 host 目录**：`rpc.js`（路径常量与信封层）也要参与扫描——
  // `/api/xlink-official-chat` 住在 `rpc.js` 里。
  const copyDir = copyHostDir('inject-a0s');
  const copyPath = join(copyDir, 'index.js');
  const baselineSha = createHash('sha256').update(readFileSync(copyPath)).digest('hex');
  const scope = () => ({
    files: ['index.js', 'rpc.js'].map((name) => ({
      file: `host/${name}`,
      text: readFileSync(join(copyDir, name), 'utf8'),
    })),
  });

  // —— 基线绿：副本与原文件逐字节相同，门禁必须绿 ——
  const green = checkHostInjection(scope());
  assert.equal(green.status, PASS, `基线必须绿：${green.detail}`);

  // —— 注入：把 exact fetch route 改回 `rpc.handle`（v0.4 已证伪的那条路）——
  //
  // **注入点选在「注册调用」上**：那正是真机 p1-r2/p1-r6 崩掉的地方（host 半边
  // 注册不上 ⇒ 客户端 404/405 ⇒ 整条通道哑火）。
  const anchor = 'connection.fetch.register({';
  assert.ok(baselineText.includes(anchor), '注入锚点不见了——实现改了，注入用例要跟着改');
  writeFileSync(
    copyPath,
    baselineText.replace(anchor, "connection.rpc.handle('/xlink-official-chat', {"),
    'utf8',
  );

  const red = checkHostInjection(scope());
  assert.equal(red.status, FAIL, '改回 rpc.handle 后门禁必须红');
  assert.match(red.detail, /rpc\.handle/, '红必须点名用的是 rpc.handle');
  assert.match(red.detail, /exact fetch route/, '红必须点名该注册的是 exact fetch route');

  // —— 撤掉注入：逐字节还原 ——
  writeFileSync(copyPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(copyPath)).digest('hex');
  assert.equal(restoredSha, baselineSha, '撤掉后必须与基线逐字节一致');

  // —— 复跑绿 ——
  const again = checkHostInjection(scope());
  assert.equal(again.status, PASS, `撤掉注入后必须复跑绿：${again.detail}`);

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(copyDir);
});

test('活体注入（行为型 · A15 指纹闸）：从真实 host 半边副本里去掉 preview 的指纹比对 → 判据红 → 还原 → 逐字节一致 → 复跑绿', async () => {
  const originalUrl = new URL('../plugins/official-chat/host/commands.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  const baselineText = readFileSync(originalUrl, 'utf8');

  // —— 基线绿：副本跑一遍，判据必须绿 ——
  const baselineDir = copyHostDir('inject-a15-baseline');
  const baselineAdapter = await createAdapter({ tag: 'inject-a15', hostDir: baselineDir });
  let green;
  try {
    green = await checkFingerprintGate(baselineAdapter);
  } finally {
    baselineAdapter.cleanup();
  }
  assert.equal(green.status, PASS, `基线必须绿：${green.detail}`);

  // —— 注入：把 preview 里「当前指纹 vs 快照指纹」的比对去掉 ——
  const anchor = "if (String(loaded.snapshot?.sourceFingerprint ?? '') !== fingerprint) {";
  assert.ok(baselineText.includes(anchor), '注入锚点不见了——实现改了，注入用例要跟着改');
  const injectedDir = copyHostDir('inject-a15-red');
  const injectedPath = join(injectedDir, 'commands.js');
  writeFileSync(injectedPath, baselineText.replace(anchor, 'if (false) {'), 'utf8');

  const injectedAdapter = await createAdapter({ tag: 'inject-a15', hostDir: injectedDir });
  let red;
  try {
    red = await checkFingerprintGate(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(red.status, FAIL, '去掉指纹比对后判据必须红');
  assert.match(red.detail, /指纹/, '红必须点名违规的性质');

  // —— 撤掉注入：逐字节还原（副本）——
  writeFileSync(injectedPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(injectedPath)).digest('hex');
  assert.equal(
    restoredSha,
    createHash('sha256').update(baselineText).digest('hex'),
    '撤掉后必须与基线逐字节一致',
  );

  // —— 复跑绿（**另起一份副本**：同一路径的模块已被 Node 缓存，还原文件不等于还原模块）——
  const restoredDir = copyHostDir('inject-a15-restored');
  const restoredAdapter = await createAdapter({ tag: 'inject-a15', hostDir: restoredDir });
  let again;
  try {
    again = await checkFingerprintGate(restoredAdapter);
  } finally {
    restoredAdapter.cleanup();
  }
  assert.equal(again.status, PASS, `撤掉注入后必须复跑绿：${again.detail}`);

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(baselineDir);
  removeTempRoot(injectedDir);
  removeTempRoot(restoredDir);
});

test('活体注入（行为型 · A9d 脱敏日志路径）：从真实 host 半边副本里去掉 logPath → 判据红 → 还原 → 逐字节一致 → 复跑绿', async () => {
  const originalUrl = new URL('../plugins/official-chat/host/commands.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  const baselineText = readFileSync(originalUrl, 'utf8');

  // —— 基线绿：真实实现必须带脱敏日志路径（协议 §3）——
  const baselineAdapter = await createAdapter({ tag: 'inject-a9d' });
  let green;
  try {
    green = await checkErrorLogPath(baselineAdapter);
  } finally {
    baselineAdapter.cleanup();
  }
  assert.equal(green.status, PASS, `基线必须绿：${green.detail}`);

  // —— 注入：把错误负载里的脱敏日志路径去掉 ——
  const anchor = 'logPath: logFailure(code, requestId, detail),';
  assert.ok(baselineText.includes(anchor), '注入锚点不见了——实现改了，注入用例要跟着改');
  const injectedDir = copyHostDir('inject-a9d-red');
  const injectedPath = join(injectedDir, 'commands.js');
  writeFileSync(injectedPath, baselineText.replace(anchor, 'logPath: null,'), 'utf8');

  const injectedAdapter = await createAdapter({ tag: 'inject-a9d', hostDir: injectedDir });
  let red;
  try {
    red = await checkErrorLogPath(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(red.status, FAIL, '错误负载没带脱敏日志路径时判据必须红');
  assert.match(red.detail, /日志路径/);

  // —— 撤掉注入：逐字节还原 ——
  writeFileSync(injectedPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(injectedPath)).digest('hex');
  assert.equal(
    restoredSha,
    createHash('sha256').update(baselineText).digest('hex'),
    '撤掉后必须与基线逐字节一致',
  );

  // —— 复跑绿（另起一份副本：同一路径的模块已被 Node 缓存）——
  const restoredDir = copyHostDir('inject-a9d-restored');
  const restoredAdapter = await createAdapter({ tag: 'inject-a9d', hostDir: restoredDir });
  let again;
  try {
    again = await checkErrorLogPath(restoredAdapter);
  } finally {
    restoredAdapter.cleanup();
  }
  assert.equal(again.status, PASS, `撤掉注入后必须复跑绿：${again.detail}`);

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(injectedDir);
  removeTempRoot(restoredDir);
});

// --- A17 保留期清理判据：反向回归必须钉住 ------------------------------------
//
// 「该清的清了」与「**不该清的留住了**」是**成对**的：只钉前者的话，
// 「无条件全删」也能蒙过去（那是多删，比漏判更难发现）。
test('A17 保留期清理：六条口径全对 → 通过', () => {
  const ok = {
    expiredSnapshotRemoved: true,
    freshSnapshotKept: true,
    expiredClaimRemoved: true,
    claimOfLiveSnapshotKept: true,
    sameRoundBothRemoved: true,
    afterSweepRejected: true,
  };
  assert.equal(checkRetentionSweep(ok).status, PASS);
});

test('A17 保留期清理：不该清的没留住 → 报红并点名（反向回归）', () => {
  const wrong = {
    expiredSnapshotRemoved: true,
    freshSnapshotKept: false, // ← 未超期快照被误删
    expiredClaimRemoved: true,
    claimOfLiveSnapshotKept: false, // ← 快照还在的认领被误删（序号会被二次分配）
    sameRoundBothRemoved: true,
    afterSweepRejected: true,
  };
  const result = checkRetentionSweep(wrong);
  assert.equal(result.status, FAIL);
  assert.ok(result.detail.includes('未超期快照留着'), 'detail 要点名缺的是哪一条');
  assert.ok(result.detail.includes('认领留着'), 'detail 要点名缺的是哪一条');
});

// ---------------------------------------------------------------------------
// A17 的**活体注入**：证明这条判据对**真实实现**的破坏敏感
// ---------------------------------------------------------------------------
//
// **为什么判据库级的反向回归不够**：那两条只证明「判据能分对错」，
// 证不了「判据跑在真实实现上时也会红」——实跑路径涉及**真实的 `host/sweep.js`**
// 与**真实落盘布局**（`<存储根>/snapshots/<provider>/`、`<存储根>/seq-claims/<provider>/`），
// 判据若把目录层级搞错，就会**永远扫不到东西** ⇒ 恒「未执行」（既不绿也不红）。
// ⇒ 必须真的往**实现副本**里注入一次破坏，看它红。
test('活体注入（行为型 · A17 保留期清理）：把真实 sweep.js 副本里的「删超期快照」破坏掉 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  const originalUrl = new URL('../plugins/official-chat/host/sweep.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  const baselineText = readFileSync(originalUrl, 'utf8');

  // —— 基线绿：真实实现的副本必须过 ——
  const baselineDir = copyHostDir('inject-a17-baseline');
  const green = await runRetentionSweep(null, { hostDir: baselineDir });
  assert.equal(green.status, PASS, `基线必须绿：${green.detail}`);
  assert.match(green.detail, /超期清/, '绿要说清清了什么');

  // —— 注入：让清理器**不再删超期快照**（协议 §4.1 第一条就破）——
  //
  // 锚点取 `sweepSnapshots` 里那处删除（`removeInTwoSteps` 在快照与认领两处都出现，
  // 正则只换**第一处** ⇒ 恰好只破坏「快照清理」，认领逻辑不动）。
  //
  // **锚点用正则而不是字面量**：helper 的调用形状会随实现演进（实现给
  // `removeInTwoSteps` 加了 `{ logger, root }`）——字面量锚点会在实现演进时
  // **假报**「锚点不见了」，把「实现变了」误报成「用例坏了」。
  const anchor = /if \(removeInTwoSteps\([^)]*\)\) result\.removed \+= 1;/;
  assert.match(baselineText, anchor, '注入锚点不见了——实现改了，注入用例要跟着改');
  const injectedDir = copyHostDir('inject-a17-red');
  const injectedPath = join(injectedDir, 'sweep.js');
  writeFileSync(injectedPath, baselineText.replace(anchor, 'if (false) { /* 注入：不删 */ }'), 'utf8');

  const red = await runRetentionSweep(null, { hostDir: injectedDir });
  assert.equal(red.status, FAIL, '不删超期快照时判据必须红（不能是「未执行」）');
  assert.match(red.detail, /超期快照被清掉/, '红必须点名缺的是哪条口径');

  // —— 撤掉注入：逐字节还原 ——
  writeFileSync(injectedPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(injectedPath)).digest('hex');
  assert.equal(
    restoredSha,
    createHash('sha256').update(baselineText).digest('hex'),
    '撤掉后必须与基线逐字节一致',
  );

  // —— 复跑绿（另起一份副本：同一路径的模块已被 Node 缓存）——
  const restoredDir = copyHostDir('inject-a17-restored');
  const again = await runRetentionSweep(null, { hostDir: restoredDir });
  assert.equal(again.status, PASS, `撤掉注入后必须复跑绿：${again.detail}`);

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(baselineDir);
  removeTempRoot(injectedDir);
  removeTempRoot(restoredDir);
});

// ---------------------------------------------------------------------------
// A18：协议 §4.1「保留期主动清理」的**剩余**口径
//
// ## 为什么要 A18（A17 之外）
//
// A17 判的是「**清没清干净**」（六个方向）；A18 判的是 §4.1 里**没被 A17 覆盖**的
// 那些条款：**怎么触发**、**什么时候不触发**、**节流**、**TTL**、**截断**、
// **两步法**、**根不可得**、**失败怎么办**、**时间戳谁优先**。
//
// ## 两层
//
//  1. **判据库级**：合规观测 ⇒ 通过；违规观测 ⇒ 报红**并点名缺哪一条**
//     （证明判据能分对错）。
//  2. **活体注入级**：往**真实实现的副本**里注入破坏 ⇒ 判据红 ⇒ 还原 ⇒
//     逐字节一致 ⇒ 复跑绿（证明判据跑在真实实现上时也会红）。
// ---------------------------------------------------------------------------

test('A18a 触发点（静态）：只挂在 catalog ⇒ 通过；capture 也挂 / 入口外也挂 / 哪都没挂 ⇒ 报红并点名；定位不到形状 ⇒ 未执行', () => {
  const ok = { parsed: true, catalogCalls: 1, captureCalls: 0, outsideCalls: 0 };
  assert.equal(checkSweepTriggerStatic(ok).status, PASS);

  const inCapture = checkSweepTriggerStatic({ ...ok, captureCalls: 1 });
  assert.equal(inCapture.status, FAIL);
  assert.match(inCapture.detail, /capture.*入口里有 1 处 sweeper 调用/, '红必须点名是 capture 入口');

  const outside = checkSweepTriggerStatic({ ...ok, outsideCalls: 2 });
  assert.equal(outside.status, FAIL);
  assert.match(outside.detail, /入口之外还有 2 处/, '红必须点名是入口外');

  const nowhere = checkSweepTriggerStatic({ ...ok, catalogCalls: 0 });
  assert.equal(nowhere.status, FAIL);
  assert.match(nowhere.detail, /catalog.*入口里没有 sweeper 调用/, '红必须点名 catalog 入口没挂');

  // **定位不到实现形状 ⇒ 未执行**（不判通过、也不判红）。
  assert.equal(checkSweepTriggerStatic({ parsed: false, detail: '没定位到' }).status, SKIPPED);
});

test('A18b 触发点（行为）：catalog 触发 ⇒ 通过；catalog 不触发 / capture 也触发 ⇒ 报红并点名', () => {
  assert.equal(checkSweepTriggerBehaviour({ catalogTriggers: 1, captureTriggers: 0 }).status, PASS);

  const notTriggered = checkSweepTriggerBehaviour({ catalogTriggers: 0, captureTriggers: 0 });
  assert.equal(notTriggered.status, FAIL);
  assert.match(notTriggered.detail, /调 `catalog` 没有触发清理/, '红必须点名 catalog 没触发');

  const captureTriggered = checkSweepTriggerBehaviour({ catalogTriggers: 1, captureTriggers: 1 });
  assert.equal(captureTriggered.status, FAIL);
  assert.match(captureTriggered.detail, /capture.*触发了 1 次清理/, '红必须点名 capture 也触发了');
});

test('A18c 节流：当天第一次跑、第二次不跑、节流键落位 ⇒ 通过；第二次又跑 / 节流键混进快照目录 ⇒ 报红并点名', () => {
  const ok = {
    firstRan: true,
    secondRanFalse: true,
    stampExists: true,
    stampNameMatches: true,
    stampInOwnDir: true,
  };
  assert.equal(checkSweepThrottle(ok).status, PASS);

  const twice = checkSweepThrottle({ ...ok, secondRanFalse: false });
  assert.equal(twice.status, FAIL);
  assert.match(twice.detail, /第二次 maybeSweep\(\) 不再跑/, '红必须点名当天第二次又跑了');

  const mixed = checkSweepThrottle({ ...ok, stampInOwnDir: false });
  assert.equal(mixed.status, FAIL);
  assert.match(mixed.detail, /独立的 sweeps\/ 目录/, '红必须点名节流键混进了快照目录');
});

test('A18d 目录缓存 TTL：超期的清、TTL 内的留 ⇒ 通过；把没超期的也清了 ⇒ 报红并点名（反向回归）', () => {
  assert.equal(checkCacheTtl({ expiredRemoved: true, freshKept: true }).status, PASS);

  const overDelete = checkCacheTtl({ expiredRemoved: true, freshKept: false });
  assert.equal(overDelete.status, FAIL);
  assert.match(overDelete.detail, /TTL 内的目录缓存留着/, '红必须点名是反向回归那条');

  const underDelete = checkCacheTtl({ expiredRemoved: false, freshKept: true });
  assert.equal(underDelete.status, FAIL);
  assert.match(underDelete.detail, /超过 TTL 的目录缓存被清掉/, '红必须点名超期的没清');
});

test('A18e 日志截断：超限要截 / 未超不截 / 截后合法 UTF-8 / 留尾部 ⇒ 通过；四条反向回归 ⇒ 报红并点名', () => {
  const ok = {
    truncatedWhenOver: true,
    notTruncatedWhenSmall: true,
    utf8Intact: true,
    keptTail: true,
  };
  assert.equal(checkLogTruncation(ok).status, PASS);

  for (const [key, pattern] of [
    ['truncatedWhenOver', /超行数\/字节上限的日志被截断/],
    ['notTruncatedWhenSmall', /未超限的日志没被截断/],
    ['utf8Intact', /仍是合法 UTF-8/],
    ['keptTail', /保留的是尾部/],
  ]) {
    const result = checkLogTruncation({ ...ok, [key]: false });
    assert.equal(result.status, FAIL, `${key} 违规时必须红`);
    assert.match(result.detail, pattern, `红必须点名缺的是哪一条（${key}）`);
  }
});

test('A18f 日志不按 7 天删：超期日志仍在 ⇒ 通过；被删了 ⇒ 报红并点名', () => {
  assert.equal(checkLogNotAgeDeleted({ oldLogStillThere: true }).status, PASS);
  const deleted = checkLogNotAgeDeleted({ oldLogStillThere: false });
  assert.equal(deleted.status, FAIL);
  assert.match(deleted.detail, /不按 7 天删/, '红必须点名日志被按天删了');
});

test('A18g 两步法删除：先 rename 后 unlink / unlink 的是改名后的 / 不留半截 ⇒ 通过；三条反向回归 ⇒ 报红并点名', () => {
  const ok = {
    renameBeforeUnlink: true,
    unlinkTargetsTombstone: true,
    noLeftoverTombstone: true,
  };
  assert.equal(checkTwoStepDelete(ok).status, PASS);

  for (const [key, pattern] of [
    ['renameBeforeUnlink', /先 rename 再 unlink/],
    ['unlinkTargetsTombstone', /unlink 的是改名后的那个/],
    ['noLeftoverTombstone', /扫完不留半截文件/],
  ]) {
    const result = checkTwoStepDelete({ ...ok, [key]: false });
    assert.equal(result.status, FAIL, `${key} 违规时必须红`);
    assert.match(result.detail, pattern, `红必须点名缺的是哪一条（${key}）`);
  }
});

test('A18h 根不可得就不扫：报 skipped / 不跑 / 不抛 / 不动别的根 ⇒ 通过；四条反向回归 ⇒ 报红并点名', () => {
  const ok = { skipped: true, maybeSweepNotRan: true, noThrow: true, nothingElseTouched: true };
  assert.equal(checkRootUnavailable(ok).status, PASS);

  for (const [key, pattern] of [
    ['skipped', /报 skipped/],
    ['maybeSweepNotRan', /ran:false/],
    ['noThrow', /不抛异常/],
    ['nothingElseTouched', /一个都没动/],
  ]) {
    const result = checkRootUnavailable({ ...ok, [key]: false });
    assert.equal(result.status, FAIL, `${key} 违规时必须红`);
    assert.match(result.detail, pattern, `红必须点名缺的是哪一条（${key}）`);
  }
});

test('A18i 清理失败只记日志：不抛 / 不阻塞 / 失败项内容仍在 / 失败进了日志 ⇒ 通过；四条反向回归 ⇒ 报红并点名', () => {
  const ok = {
    noThrow: true,
    othersStillRemoved: true,
    failedItemStillPresent: true,
    failureLogged: true,
  };
  assert.equal(checkSweepFailureLogged(ok).status, PASS);

  for (const [key, pattern] of [
    ['noThrow', /没抛异常/],
    ['othersStillRemoved', /照删/],
    ['failedItemStillPresent', /仍在磁盘上/],
    ['failureLogged', /进了日志/],
  ]) {
    const result = checkSweepFailureLogged({ ...ok, [key]: false });
    assert.equal(result.status, FAIL, `${key} 违规时必须红`);
    assert.match(result.detail, pattern, `红必须点名缺的是哪一条（${key}）`);
  }
});

test('A18j capturedAt 优先、mtime 兜底：四个方向全对 ⇒ 通过；两条反向回归 ⇒ 报红并点名', () => {
  const ok = {
    oldCapturedNewMtimeRemoved: true,
    newCapturedOldMtimeKept: true,
    missingCapturedOldMtimeRemoved: true,
    missingCapturedNewMtimeKept: true,
  };
  assert.equal(checkCapturedAtPriority(ok).status, PASS);

  const mtimeWins = checkCapturedAtPriority({ ...ok, oldCapturedNewMtimeRemoved: false });
  assert.equal(mtimeWins.status, FAIL);
  assert.match(mtimeWins.detail, /capturedAt 旧 \+ mtime 新 ⇒ 仍删/, '红必须点名是 capturedAt 优先那条');

  const mtimeOverDeletes = checkCapturedAtPriority({ ...ok, newCapturedOldMtimeKept: false });
  assert.equal(mtimeOverDeletes.status, FAIL);
  assert.match(mtimeOverDeletes.detail, /capturedAt 新 \+ mtime 旧 ⇒ 不删/, '红必须点名反向回归那条');
});

test('A18k 「正在发送中的引用」不保护 —— 不可机检 ⇒ 如实报「已知不覆盖」并说明后果由 S-5/S-6 覆盖', () => {
  const result = checkInflightReferenceNotCovered();
  assert.equal(result.status, SKIPPED);
  assert.match(result.detail, /已知不覆盖/, '必须写明「已知不覆盖」，不许假装能测');
  assert.match(result.detail, /S-5 \/ S-6/, '必须写明后果由 S-5/S-6 覆盖');
});

// ---------------------------------------------------------------------------
// A18 的**活体注入**：证明这些判据对**真实实现**的破坏敏感
// ---------------------------------------------------------------------------

/**
 * 活体注入的统一骨架。
 *
 * 顺序固定：**注入 ⇒ 红（点名）⇒ 还原 ⇒ 逐字节一致 ⇒ 另起一份副本复跑绿 ⇒
 * 原实现没被碰过**。
 *
 * **为什么复跑要另起一份副本**：Node 的模块缓存按**解析后的路径**算——
 * 同一路径的副本第二次 import 拿到的还是**第一次那份**（注入版）⇒ 复跑必然红。
 *
 * @param {object} options
 * @param {string} options.tag - 临时目录标签。
 * @param {string} options.file - 要改的文件名（相对 host 目录）。
 * @param {string} options.originalFile - 原实现文件名（用来验「没被碰过」）。
 * @param {(text: string) => string} options.mutate - 注入（改文本）。
 * @param {(copyDir: string) => Promise<object>} options.run - 跑判据。
 * @param {(verdict: object) => void} options.expectRed - 红的断言。
 * @param {(verdict: object) => void} [options.expectGreen] - 绿的断言。
 */
async function liveInject({ tag, file, originalFile, mutate, run, expectRed, expectGreen }) {
  const originalUrl = new URL(`../plugins/official-chat/host/${originalFile}`, import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');

  const copyDir = copyHostDir(tag);
  const copyPath = join(copyDir, file);
  const baselineText = readFileSync(copyPath, 'utf8');

  // —— 注入 ——
  const injectedText = mutate(baselineText);
  assert.notEqual(injectedText, baselineText, '注入没生效——锚点不见了，实现改了，用例要跟着改');
  writeFileSync(copyPath, injectedText, 'utf8');

  const red = await run(copyDir);
  assert.equal(red.status, FAIL, `注入后必须红（不能是「未执行」）：${red.detail}`);
  expectRed(red);

  // —— 还原：逐字节 ——
  writeFileSync(copyPath, baselineText, 'utf8');
  const restoredSha = createHash('sha256').update(readFileSync(copyPath)).digest('hex');
  assert.equal(
    restoredSha,
    createHash('sha256').update(baselineText).digest('hex'),
    '还原后必须与基线逐字节一致',
  );

  // —— 复跑绿（另起一份副本：同一路径的模块已被 Node 缓存）——
  const restoredDir = copyHostDir(`${tag}-restored`);
  try {
    const again = await run(restoredDir);
    assert.equal(again.status, PASS, `还原后必须复跑绿：${again.detail}`);
    if (expectGreen !== undefined) expectGreen(again);
  } finally {
    removeTempRoot(restoredDir);
  }

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(copyDir);
}

/** 往 `capture` 入口里塞一次清理调用（破坏「只在 catalog 触发」）。 */
const injectSweepIntoCapture = (text) =>
  text.replace(
    '  async function capture(request = {}) {',
    '  async function capture(request = {}) {\n    sweeper?.maybeSweep?.();',
  );

test('活体注入（静态 · A18a 触发点）：从真实 host 半边副本里往 capture 入口塞一次清理调用 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18a',
    file: 'commands.js',
    originalFile: 'commands.js',
    mutate: injectSweepIntoCapture,
    run: async (dir) => runSweepTriggerStatic(dir),
    expectRed: (r) => {
      assert.match(r.detail, /capture.*入口里有 1 处 sweeper 调用/, '红必须点名是 capture 入口');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /只挂在 `catalog` 入口/, '绿要说清挂在哪');
    },
  });
});

test('活体注入（行为型 · A18b 触发点）：从真实 host 半边副本里往 capture 入口塞一次清理调用 → 行为判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18b',
    file: 'commands.js',
    originalFile: 'commands.js',
    mutate: injectSweepIntoCapture,
    run: async (dir) => {
      const adapter = await createAdapter({ tag: 'inject-a18b-adapter', hostDir: dir });
      try {
        return await runSweepTriggerBehaviour(adapter);
      } finally {
        adapter.cleanup();
      }
    },
    expectRed: (r) => {
      assert.match(r.detail, /capture.*触发了 1 次清理/, '红必须点名 capture 也触发了清理');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /catalog 触发 1 次、capture 触发 0 次/, '绿要说清触发次数');
    },
  });
});

test('活体注入（行为型 · A18c 节流）：从真实 sweep.js 副本里去掉「记节流键」→ 当天第二次又扫 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18c',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) => text.replace('stampSweep();', '/* 注入：不记节流键 */;'),
    run: async (dir) => runSweepThrottle(dir),
    expectRed: (r) => {
      assert.match(r.detail, /第二次 maybeSweep\(\) 不再跑/, '红必须点名当天第二次又跑了');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /第二次不重跑/, '绿要说清第二次没重跑');
    },
  });
});

test('活体注入（行为型 · A18d 目录缓存 TTL）：从真实 sweep.js 副本里把缓存 TTL 放大 1000 倍 → 超期缓存没清 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18d',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) =>
      text.replace(
        'const cutoff = now() - cacheTtlMs;',
        'const cutoff = now() - cacheTtlMs * 1000; /* 注入：TTL 放大 1000 倍 */',
      ),
    run: async (dir) => runCacheTtl(dir),
    expectRed: (r) => {
      assert.match(r.detail, /超过 TTL 的目录缓存被清掉/, '红必须点名超期缓存没清');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /TTL 内留着/, '绿要说清 TTL 内的留着');
    },
  });
});

test('活体注入（行为型 · A18e 日志截断）：从真实 sweep.js 副本里去掉截断 → 超限日志没截 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18e',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) =>
      text.replace(
        'if (!tooManyLines && !tooManyBytes) return { truncated: false };',
        'if (true) return { truncated: false }; /* 注入：不截断 */',
      ),
    run: async (dir) => runLogTruncation(dir),
    expectRed: (r) => {
      assert.match(r.detail, /超行数\/字节上限的日志被截断/, '红必须点名超限日志没被截断');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /未超不截/, '绿要说清没超不截');
    },
  });
});

test('活体注入（行为型 · A18f 日志不按 7 天删）：往真实 sweep.js 副本里加一条「超 7 天就删日志」→ 日志被按天删 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18f',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) =>
      text.replace(
        "  const path = join(root, 'logs', 'official-chat.log');",
        "  const path = join(root, 'logs', 'official-chat.log');\n" +
          '  try { if (existsSync(path) && now() - statSync(path).mtimeMs > retentionMs) unlinkSync(path); } catch { /* 注入 */ }',
      ),
    run: async (dir) => runLogNotAgeDeleted(dir),
    expectRed: (r) => {
      assert.match(r.detail, /不按 7 天删/, '红必须点名日志被按天删了');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /超期日志仍在/, '绿要说清超期日志还在');
    },
  });
});

test('活体注入（静态 + 行为型 · A18g 两步法删除）：把真实 sweep.js 副本里的「先改名再删」改成「直接删」→ 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18g',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) =>
      text.replace(
        '    renameSync(path, tombstone);',
        '    unlinkSync(path); /* 注入：直接删，不改名 */',
      ),
    run: async (dir) => runTwoStepDelete(dir),
    expectRed: (r) => {
      assert.match(r.detail, /先 rename 再 unlink/, '红必须点名两步法没了');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /两步法删除/, '绿要说清是两步法');
    },
  });
});

test('活体注入（行为型 · A18h 根不可得就不扫）：从真实 sweep.js 副本里让「根不可得」也往下扫 → 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18h',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) =>
      text.replace(
        "return { skipped: true, reason: '落盘根不可得（内核没给实例 home）——不扫' };",
        "return { skipped: false, reason: '注入：根不可得也扫' };",
      ),
    run: async (dir) => runRootUnavailable(dir),
    expectRed: (r) => {
      assert.match(r.detail, /root 不可得时 sweep\(\) 报 skipped/, '红必须点名根不可得没跳过');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /不扫、不删、不抛/, '绿要说清没扫没删没抛');
    },
  });
});

test('活体注入（行为型 · A18j capturedAt 优先）：从真实 sweep.js 副本里改成「先看 mtime」→ 判据红并点名 → 还原 → 逐字节一致 → 复跑绿', async () => {
  await liveInject({
    tag: 'inject-a18j',
    file: 'sweep.js',
    originalFile: 'sweep.js',
    mutate: (text) =>
      text.replace(
        "if (stampOf(path, 'capturedAt', mtime) >= cutoff) continue;",
        'if (mtime >= cutoff) continue; /* 注入：先看 mtime */',
      ),
    run: async (dir) => runCapturedAtPriority(dir),
    expectRed: (r) => {
      assert.match(r.detail, /capturedAt 旧 \+ mtime 新 ⇒ 仍删/, '红必须点名 capturedAt 没优先');
    },
    expectGreen: (r) => {
      assert.match(r.detail, /capturedAt 优先、mtime 仅兜底/, '绿要说清谁优先');
    },
  });
});

/**
 * 把「删除失败要留痕」**关掉**（做成不留痕的样子）。
 *
 * ## 注入方式
 *
 * 去掉调用点那个 `else logger?.warn?.(...)` 分支 ⇒ 删不掉被**悄悄吞掉**
 * （这正是协议 §4.1 第 3 条不许出现的那种「静默失败」）。
 *
 * **锚点用正则**（不是字面量）：留痕写在「同一行」还是「独立成行」、helper 的
 * 调用形状带不带参数，实现都会动——字面量锚点会在实现演进时**假报**
 * 「锚点不见了」。
 *
 * @param {string} text - sweep.js 源码。
 * @returns {string} 注入后的源码。
 */
const stripFailureLog = (text) =>
  text.replace(
    /(if \(removeInTwoSteps\([^)]*\)\) result\.removed \+= 1;)(\s*)else logger\?\.warn\?\.\([^;]*\);/,
    '$1',
  );

test('活体注入（行为型 · A18i 清理失败只记日志）：把副本里的「删除失败要留痕」关掉 ⇒ 判据红并点名 ⇒ 还原成留痕 ⇒ 判据绿（两个方向都由副本控制，不依赖现役实现当前的状态）', async () => {
  const originalUrl = new URL('../plugins/official-chat/host/sweep.js', import.meta.url);
  const originalBefore = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');

  const copyDir = copyHostDir('inject-a18i');
  const copyPath = join(copyDir, 'sweep.js');
  const baselineText = readFileSync(copyPath, 'utf8');

  // —— 方向①：不合规（删除失败**不留痕**）——
  const nonCompliant = stripFailureLog(baselineText);
  assert.notEqual(nonCompliant, baselineText, '注入没生效——调用点的形状变了，注入用例要跟着改');
  writeFileSync(copyPath, nonCompliant, 'utf8');
  const red = await runSweepFailureLogged(copyDir);
  assert.equal(red.status, FAIL, '删除失败不留痕时判据必须红');
  assert.match(red.detail, /进了日志/, '红必须点名「失败要进日志」这条');

  // —— 方向②：合规（删除失败**留痕**）——
  //
  // **合规副本就是现役实现本身**——它现在按协议留痕（helper 内部
  // `logger?.warn?.(...)`）。⇒ 「合规」这一侧钉的是**实现的现状**，
  // 「不合规」那一侧钉的是**判据的分辨力**。
  const compliantDir = copyHostDir('inject-a18i-compliant');
  writeFileSync(join(compliantDir, 'sweep.js'), baselineText, 'utf8');
  const green = await runSweepFailureLogged(compliantDir);
  assert.equal(green.status, PASS, `删除失败留痕时判据必须绿：${green.detail}`);
  assert.match(green.detail, /失败进了日志/, '绿要说清失败进了日志');

  // —— 原实现文件没被碰过 ——
  const originalAfter = createHash('sha256').update(readFileSync(originalUrl)).digest('hex');
  assert.equal(originalAfter, originalBefore, '原实现文件必须一个字节都没变');

  removeTempRoot(copyDir);
  removeTempRoot(compliantDir);
});
