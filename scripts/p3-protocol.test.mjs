// P3 协议判据的**自测**。
//
// ## 判什么
//
// · **正反两个方向**：每条判据对**合规实现**必须是绿的，对**违规实现**必须红
//   且**点名缺哪一条协议**（不然红得没有信息量）。
// · **活体注入**：对**真实实现的副本**注入一个缺陷 ⇒ 判据必须红并点名 ⇒
//   原实现**逐字节未动**（注入只能打在副本上）。
// · **真机冒烟**：十四条判据在真实实现上**都跑得起来**（不是「未执行」、也不是
//   「判据自身抛错」）——红绿**不硬断言**（实现会演进，红会变绿）。
//
// ## 为什么要有「合成适配器」这一层
//
// 活体注入证明的是「判据能抓到**这一种**缺陷」。合成适配器证明的是
// 「判据对**这一类**缺陷都能红」——两者都不可省：
// 只做活体注入 ⇒ 判据可能只认得自己注入的那一种形状；
// 只做合成 ⇒ 判据可能根本没接到真实实现的形状上。
//
// ## 合成适配器的身份
//
// 合成适配器是**一份合规实现**（按协议 §2/§4/§S-8…§S-15 的形状写的**替身**），
// 不是「判据的桩」⇒ 它必须**真的**满足协议（临时文件 → 回读 → rename、
// 占用记录四字段、先扫后删、日志四字段、账户维度服务端推导…）。
// ⇒ 「九条全绿」这件事本身就是判据**可满足**的证据。
//
// ## 纪律
//
// · 期望值**从协议稿转写**（摘要自己算，不 import 实现当标准答案）。
// · 注入打在**副本**上；每个用例结束删掉自己建的临时目录（只删自己建的）。
// · **测试永远不碰用户的真实数据目录**（落盘根一律在 `%TEMP%` 下的一次性目录）。
//
// @module scripts/p3-protocol-test

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  truncateSync,
  unlinkSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  FAIL,
  MAX_SEQ as CRITERION_MAX_SEQ,
  PASS,
  SKIPPED,
  checkP3_1_atomicPublish,
  checkP3_2_writeBackVerify,
  checkP3_3_tmpResidualSwept,
  checkP3_4_occupancyRecorded,
  checkP3_5_reclaimRespectsOccupancy,
  checkP3_6_orphanClaimReclaimed,
  checkP3_7_storageProtection,
  checkP3_8_policyDocumented,
  checkP3_9_accountScopeServerDerived,
  checkP3_10_seqNotReused,
  checkP3_11_releasePaths,
  checkP3_12_twoRoundRelease,
  checkP3_13_cacheBudget,
  checkP3_14_accountTiming,
  P3_13_CACHE_BUDGET_BYTES as CRITERION_CACHE_BUDGET_BYTES,
} from './lib/p3-protocol.mjs';
import {
  OCCUPANCY_RETENTION_MS,
  SNAPSHOT_RETENTION_MS,
  TMP_RESIDUAL_MS,
  createP3Adapter,
  makeTempRoot,
  removeTempRoot,
} from './lib/p3-adapter.mjs';

const REPO = fileURLToPath(new URL('../', import.meta.url));
const HOST_DIR = join(REPO, 'plugins', 'official-chat', 'host');

/** 十四条判据（P3 协议稿 §5）。 */
const CRITERIA = [
  { id: 'P3-1', run: checkP3_1_atomicPublish },
  { id: 'P3-2', run: checkP3_2_writeBackVerify },
  { id: 'P3-3', run: checkP3_3_tmpResidualSwept },
  { id: 'P3-4', run: checkP3_4_occupancyRecorded },
  { id: 'P3-5', run: checkP3_5_reclaimRespectsOccupancy },
  { id: 'P3-6', run: checkP3_6_orphanClaimReclaimed },
  { id: 'P3-7', run: checkP3_7_storageProtection },
  { id: 'P3-8', run: checkP3_8_policyDocumented },
  { id: 'P3-9', run: checkP3_9_accountScopeServerDerived },
  { id: 'P3-10', run: checkP3_10_seqNotReused },
  { id: 'P3-11', run: checkP3_11_releasePaths },
  { id: 'P3-12', run: checkP3_12_twoRoundRelease },
  { id: 'P3-13', run: checkP3_13_cacheBudget },
  { id: 'P3-14', run: checkP3_14_accountTiming },
];

// ---------------------------------------------------------------------------
// 合成适配器（**一份合规实现** ＋ 可控的违规点）
// ---------------------------------------------------------------------------

/** 服务端推导的账户维度（合成实现自己的服务端变量）。 */
const SERVER_SCOPE = 'p3-server-derived-account';

/** SHA-256（十六进制）。 */
function sha256(text) {
  return createHash('sha256').update(String(text), 'utf8').digest('hex');
}

/** 会话短码（合成实现自己的口径：会话 id 的摘要前 12 位）。 */
function sessionShort(sessionId) {
  return sha256(String(sessionId)).slice(0, 12);
}

/**
 * 造一个**合成适配器**（判据契约的替身；**默认是合规实现**）。
 *
 * @param {object} [options]
 * @param {string[]} [options.violations] - 要打开的违规点（见 `VIOLATIONS` 表）。
 * @param {string} [options.tag] - 临时目录后缀。
 * @returns {object} adapter（判据契约）。
 */
function makeSyntheticAdapter({ violations = [], tag = 'synth' } = {}) {
  const bad = new Set(violations);
  const providerId = 'ds';
  const root = makeTempRoot(tag);
  const logLines = [];
  let tmpCounter = 0;

  /**
   * 合成实现自己的**缓存总量上限**（协议 §4 `cacheTotalBudgetBytes` ＝ 512 MB）。
   *
   * ## 为什么合成实现也要有它
   *
   * §S-16 裁定 2/3/5 说清了「**什么时候核算**」「**超了怎么办**」「**留什么痕**」⇒
   * 合成实现按这套口径做 ⇒ 判据的①-⑤ 在它身上**应该全绿** ⇒ 「全绿」本身
   * 就是判据**可满足**的证据。
   */
  const SYNTH_BUDGET = 512 * 1024 * 1024;

  /** 合成实现自己的**目录缓存 TTL**（5 分钟）。 */
  const SYNTH_CACHE_TTL_MS = 5 * 60 * 1000;

  /** 正文摘要（`sha256(正文)`）。 */
  const digestOf = (messages) =>
    sha256((Array.isArray(messages) ? messages : []).map((one) => String(one?.text ?? '')).join('\n\n'));

  /** 写一行错误日志（协议 §S-14：只有 `at`/`code`/`requestId`/`detail` 四个字段）。 */
  function writeErrorLog(requestId, code, detail) {
    const dir = join(root, 'logs');
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const line = {
      at: new Date().toISOString(),
      code: String(code),
      requestId: String(requestId ?? ''),
      detail: String(detail ?? ''),
    };
    // **违规**：把正文混进日志（协议 §S-14：日志不收正文）。
    if (bad.has('log-body')) line.body = 'P3 判据正文';
    appendFileSync(join(dir, 'official-chat.log'), `${JSON.stringify(line)}\n`, { mode: 0o600 });
  }

  /** 落盘一份快照（**合规**：临时文件 → 回读校验 → rename）。 */
  function saveSnapshot({ snapshotId, accountScope, sessionId, branch = 'main', capturedAt, messages }) {
    const dir = join(root, 'snapshots', providerId);
    mkdirSync(dir, { recursive: true, ...(bad.has('no-mode-args') ? {} : { mode: 0o700 }) });
    const stored = {
      schemaVersion: 1,
      snapshotId,
      providerId,
      accountScope,
      sessionId,
      branch,
      messages,
      contentDigest: digestOf(messages),
      capturedAt,
    };
    const text = `${JSON.stringify(stored, null, 2)}\n`;
    if (bad.has('direct-publish')) {
      // **违规**：直写最终路径（没有临时名、没有回读、没有 rename）。
      writeFileSync(join(dir, `${snapshotId}.json`), text, { mode: 0o600 });
      return;
    }
    const tmp = join(dir, `.${snapshotId}.json.tmp-${Date.now()}-${tmpCounter++}`);
    writeFileSync(tmp, text, bad.has('no-mode-args') ? 'utf8' : { mode: 0o600 });
    // **回读校验**：读回来重算摘要，比不上 ⇒ 不 rename。
    const back = JSON.parse(readFileSync(tmp, 'utf8'));
    if (digestOf(back.messages) !== String(back.contentDigest ?? '')) {
      throw new Error('回读校验不通过');
    }
    renameSync(tmp, join(dir, `${snapshotId}.json`));
  }

  /** 文件里的业务时间（读不动就退到 mtime）。 */
  function stampOf(path, field) {
    try {
      const parsed = JSON.parse(readFileSync(path, 'utf8'));
      const value = Date.parse(String(parsed?.[field] ?? ''));
      if (Number.isFinite(value)) return value;
    } catch {
      /* 读不动就退到 mtime */
    }
    return statSync(path).mtimeMs;
  }

  /** 读 JSON（读不动就是 null）。 */
  function readJson(path) {
    try {
      const parsed = JSON.parse(readFileSync(path, 'utf8'));
      return parsed && typeof parsed === 'object' ? parsed : null;
    } catch {
      return null;
    }
  }

  /** 每个会话的序号分配。 */
  const seqBySession = new Map();

  function nextSeq(sessionId) {
    const next = (seqBySession.get(sessionId) ?? 0) + 1;
    seqBySession.set(sessionId, next);
    return next;
  }

  /** 写认领（合成实现自己的口径）。 */
  function writeClaimFor(sessionId, seq) {
    const sess = sessionShort(sessionId);
    const dir = join(root, 'seq-claims', providerId, sess);
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const path = join(dir, `${String(seq).padStart(4, '0')}.json`);
    recordWrite('write', path);
    writeFileSync(
      path,
      `${JSON.stringify(
        { providerShort: providerId, sessionShort: sess, seq, claimedAt: new Date().toISOString() },
        null,
        2,
      )}\n`,
      { mode: 0o600 },
    );
  }

  /** 那个序号是不是已被认领。 */
  function claimExists(snapshotId) {
    const parts = String(snapshotId).split('~');
    if (parts.length !== 3) return false;
    return existsSync(
      join(root, 'seq-claims', parts[0], parts[1], `${parts[2].padStart(4, '0')}.json`),
    );
  }

  // -------------------------------------------------------------------------
  // 序号分配（协议 §2.5 水位 ＋ §S-13 裁定 4）
  // -------------------------------------------------------------------------

  /** 序号上限（**合成实现自己的常量**；协议 §S-1 的 4 位上限 9999）。 */
  const SYNTH_MAX_SEQ = 9999;

  /** 写盘次序的**记录口**（`null` ＝ 不记 ⇒ 平时零开销）。 */
  let traceSink = null;

  /** 记一次写盘（给「写序探针」用）。 */
  function recordWrite(kind, path) {
    if (traceSink !== null) traceSink.push({ kind, path: String(path) });
  }

  /** 某个会话的**认领目录**（绝对路径）。 */
  function claimsDirOf(sessionId) {
    return join(root, 'seq-claims', providerId, sessionShort(sessionId));
  }

  /** 某个会话目录里**现存的认领序号**（升序）。 */
  function claimSeqsOf(sessionId) {
    const dir = claimsDirOf(sessionId);
    if (!existsSync(dir)) return [];
    const out = [];
    for (const name of readdirSync(dir)) {
      if (!/^\d+\.json$/.test(name)) continue;
      out.push(Number(name.slice(0, -5)));
    }
    return out.sort((a, b) => a - b);
  }

  /** 读**水位记录**（合成实现自己的口径：会话目录下的 `watermark.json`）。 */
  function readWatermark(sessionId) {
    const path = join(claimsDirOf(sessionId), 'watermark.json');
    if (!existsSync(path)) return 0;
    try {
      const parsed = JSON.parse(readFileSync(path, 'utf8'));
      const seq = Number(parsed?.seq ?? 0);
      return Number.isFinite(seq) && seq > 0 ? seq : 0;
    } catch {
      return 0;
    }
  }

  /** 写**水位记录**（**独立于认领文件**落盘 ⇒ 认领被删也不回头）。 */
  function writeWatermark(sessionId, seq) {
    const dir = claimsDirOf(sessionId);
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const path = join(dir, 'watermark.json');
    recordWrite('write', path);
    writeFileSync(
      path,
      `${JSON.stringify({ seq, updatedAt: new Date().toISOString() }, null, 2)}\n`,
      { mode: 0o600 },
    );
  }

  /** 合成实现自己的 `snapshotId` 形状（`<providerShort>~<sessionShort>~<seq>`）。 */
  function synthSnapshotId(sessionId, seq) {
    return `${providerId}~${sessionShort(sessionId)}~${String(seq).padStart(4, '0')}`;
  }

  /**
   * 造一套**新的**序号分配器。
   *
   * ## 为什么每次都是「重新读盘」
   *
   * 分配器**不持有任何进程内状态** ⇒ 「换一套分配器」＝「进程重启」
   * ⇒ 判据的②（重启后仍不复用）才有意义。
   *
   * @returns {{reserve: (sessionId: string) => {seq: number, snapshotId: string}}}
   */
  function buildSeqAllocator() {
    return {
      reserve(sessionId) {
        const dir = claimsDirOf(sessionId);
        mkdirSync(dir, { recursive: true, mode: 0o700 });
        // **合规**：序号从**水位**接（认领被删也不回头）；
        // **违规 `no-watermark`**：从**现存认领**接（认领被删 ⇒ 序号被复用）。
        const useWatermark = !bad.has('no-watermark');
        const seqs = claimSeqsOf(sessionId);
        const high = useWatermark ? readWatermark(sessionId) : (seqs[seqs.length - 1] ?? 0);
        const next = high + 1;
        if (next > SYNTH_MAX_SEQ) {
          // **违规 `wrap-at-limit`**：到上限**绕过去**（而不是报「序号已用尽」）。
          if (bad.has('wrap-at-limit')) {
            const wrapped = 1;
            writeClaimFor(sessionId, wrapped);
            if (useWatermark) writeWatermark(sessionId, wrapped);
            return { seq: wrapped, snapshotId: synthSnapshotId(sessionId, wrapped) };
          }
          // **合规**：报既有的「序号已用尽」（`source_unsupported`）。
          // **违规 `new-limit-error`**：**换形态**（换码、换话）。
          const code = bad.has('new-limit-error') ? 'budget_exceeded' : 'source_unsupported';
          const message = bad.has('new-limit-error')
            ? '这条会话的 pick 序号预算不足'
            : `这条会话的 pick 序号已用尽（协议 S-1 的 4 位上限 ${SYNTH_MAX_SEQ}）`;
          const error = new Error(message);
          error.code = code;
          throw error;
        }
        const seq = next;
        // **写序**：合规＝**先水位、后认领**（协议 §S-13 裁定 4②）；
        // **违规 `claim-first`** ＝反过来（留下「认领已建、水位未写」的窗口）。
        if (bad.has('claim-first')) {
          writeClaimFor(sessionId, seq);
          if (useWatermark) writeWatermark(sessionId, seq);
        } else {
          if (useWatermark) writeWatermark(sessionId, seq);
          writeClaimFor(sessionId, seq);
        }
        return { seq, snapshotId: synthSnapshotId(sessionId, seq) };
      },
    };
  }

  /** 某条占用记录的**绝对路径**（合成实现自己的口径：`refs/<id>.json`）。 */
  function occupancyPathOf(snapshotId) {
    return join(root, 'refs', `${snapshotId}.json`);
  }

  /**
   * 合成实现自己的 `release` 命令（协议 §S-11 R1）。
   *
   * **合规形态**：删掉被点名的那一条占用记录 ⇒ 成功返回
   * `{ ok: true, released: true }`；该 id 本来就没有记录 ⇒ **仍成功**、
   * `released: false`（**幂等**）。
   *
   * @param {object|string} input - `{ snapshotId }`（也收裸 id）。
   * @returns {object} 命令返回。
   */
  function releaseOccupancy(input = {}) {
    const snapshotId =
      input !== null && typeof input === 'object'
        ? String(input.snapshotId ?? '')
        : String(input ?? '');
    if (snapshotId === '') {
      return { ok: false, error: { code: 'snapshot_invalid', message: '快照 id 为空' } };
    }
    // **违规 `release-cross`**：把**所有**占用记录都删掉（越界）。
    if (bad.has('release-cross')) {
      const dir = join(root, 'refs');
      if (existsSync(dir)) {
        for (const name of readdirSync(dir)) {
          if (!name.endsWith('.json')) continue;
          unlinkSync(join(dir, name));
        }
      }
      return { ok: true, released: true };
    }
    const path = occupancyPathOf(snapshotId);
    if (!existsSync(path)) {
      // **违规 `release-not-idempotent`**：没有记录时**报错**（不幂等）。
      if (bad.has('release-not-idempotent')) {
        return {
          ok: false,
          error: { code: 'snapshot_invalid', message: '没有可释放的占用记录' },
        };
      }
      return { ok: true, released: false };
    }
    // **违规 `release-noop`**：报成功、但**不删**。
    if (!bad.has('release-noop')) {
      unlinkSync(path);
    }
    // **违规 `release-wrong-shape`**：返回里**没有** `released` 这个布尔。
    if (bad.has('release-wrong-shape')) {
      return { ok: true };
    }
    return { ok: true, released: true };
  }

  /** 造一套命令层（合成实现自己的）。 */
  function buildCommands() {
    return {
      /**
       * `release`：**R1 显式释放**（协议 §S-11）。
       *
       * **违规 `release-missing`**：**没有**这个入口 ⇒ 协议 §S-11 R1 要求有。
       */
      ...(bad.has('release-missing') ? {} : { release: releaseOccupancy }),

      async catalog(request = {}) {
        const requestId = String(request.requestId ?? '');
        const provided = request.accountScope;
        if (
          !bad.has('scope-from-request') &&
          provided !== undefined &&
          provided !== null &&
          String(provided) !== SERVER_SCOPE
        ) {
          writeErrorLog(requestId, 'account_changed', '账户范围与服务端推导值不一致');
          return {
            ok: false,
            code: 'account_changed',
            detail: '账户范围与服务端推导值不一致',
            requestId,
          };
        }
        // **账户维度由服务端推导**（违规变体：取自请求参数）。
        const scope = bad.has('scope-from-request')
          ? String(provided ?? SERVER_SCOPE)
          : SERVER_SCOPE;
        const query = String(request.query ?? '');
        const cacheDir = join(root, 'catalog-cache');
        mkdirSync(cacheDir, { recursive: true, ...(bad.has('no-mode-args') ? {} : { mode: 0o700 }) });
        writeFileSync(
          join(cacheDir, `${providerId}__${scope}__${query}.json`),
          `${JSON.stringify({ fetchedAt: new Date().toISOString(), page: {} }, null, 2)}\n`,
          bad.has('no-mode-args') ? 'utf8' : { mode: 0o600 },
        );
        const allocations = {};
        for (const sessionId of ['p3-session', 'p3-session-2']) {
          const seq = nextSeq(sessionId);
          allocations[sessionId] = `${providerId}~${sessionShort(sessionId)}~${String(seq).padStart(4, '0')}`;
          writeClaimFor(sessionId, seq);
        }
        return { ok: true, allocations };
      },

      async capture(request = {}) {
        const requestId = String(request.requestId ?? '');
        const provided = request.accountScope;
        if (
          !bad.has('scope-from-request') &&
          provided !== undefined &&
          provided !== null &&
          String(provided) !== SERVER_SCOPE
        ) {
          writeErrorLog(requestId, 'account_changed', '账户范围与服务端推导值不一致');
          return {
            ok: false,
            code: 'account_changed',
            detail: '账户范围与服务端推导值不一致',
            requestId,
          };
        }
        const scope = bad.has('scope-from-request')
          ? String(provided ?? SERVER_SCOPE)
          : SERVER_SCOPE;
        const sessionId = String(request.sessionId ?? '');
        const snapshotId = String(request.snapshotId ?? '');
        if (!snapshotId.startsWith(`${providerId}~${sessionShort(sessionId)}~`)) {
          writeErrorLog(requestId, 'snapshot_invalid', '快照 id 与会话不匹配');
          return { ok: false, code: 'snapshot_invalid', detail: '快照 id 与会话不匹配', requestId };
        }
        if (!claimExists(snapshotId)) {
          writeErrorLog(requestId, 'snapshot_invalid', '序号未被认领');
          return { ok: false, code: 'snapshot_invalid', detail: '序号未被认领', requestId };
        }
        const capturedAt = new Date().toISOString();
        saveSnapshot({
          snapshotId,
          accountScope: scope,
          sessionId,
          capturedAt,
          messages: [{ text: 'P3 判据正文' }],
        });
        // **占用记录**（协议 §2.4 的四字段闭集）。
        if (!bad.has('no-occupancy-record')) {
          const refsDir = join(root, 'refs');
          mkdirSync(refsDir, { recursive: true, mode: 0o700 });
          writeFileSync(
            join(refsDir, `${snapshotId}.json`),
            `${JSON.stringify(
              { snapshotId, capturedAt, createdAt: capturedAt, sessionId },
              null,
              2,
            )}\n`,
            { mode: 0o600 },
          );
        }
        return { ok: true, snapshotId };
      },
    };
  }

  /** 走真实链路拿一个可 capture 的快照 id。 */
  async function allocateSnapshotId(sessionId = 'p3-session') {
    const commands = buildCommands();
    const page = await commands.catalog({ requestId: 'p3-synth-allocate', query: '' });
    const allocation = page?.allocations?.[sessionId];
    if (typeof allocation !== 'string' || allocation === '') {
      throw new Error(`合成适配器没有为会话 ${sessionId} 分配出快照 id`);
    }
    return allocation;
  }

  /**
   * 目录的**字节数**（递归；**与判据的独立加总同口径**）。
   *
   * @param {string} dir - 绝对路径。
   * @returns {number}
   */
  function dirBytesOf(dir) {
    let total = 0;
    if (!existsSync(dir)) return 0;
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      let st;
      try {
        st = statSync(full);
      } catch {
        continue;
      }
      if (st.isDirectory()) total += dirBytesOf(full);
      else if (st.isFile()) total += st.size;
    }
    return total;
  }

  /**
   * 合成实现的**核算总量**（协议 §S-16 裁定 1）。
   *
   * - **计入**：`snapshots/**`（**逐 provider 目录**求和）＋ `catalog-cache/**`（**平铺**）；
   * - **不计入**：`seq-claims/`、`refs/`、`logs/`、`sweeps/`。
   *
   * **违规 `count-excluded`**：把**被排除的四个目录**也算进去 ⇒ 判据的①必须红。
   *
   * @returns {{totalBytes: number, snapshotsBytes: number, cacheBytes: number}}
   */
  function measureTotal() {
    const snapshotsBytes = dirBytesOf(join(root, 'snapshots'));
    const cacheBytes = dirBytesOf(join(root, 'catalog-cache'));
    let totalBytes = snapshotsBytes + cacheBytes;
    if (bad.has('count-excluded')) {
      for (const dir of ['seq-claims', 'refs', 'logs', 'sweeps']) {
        totalBytes += dirBytesOf(join(root, dir));
      }
    }
    return { totalBytes, snapshotsBytes, cacheBytes };
  }

  /** 跑一次清理（合成实现：**先扫占用，再回收**）。 */
  async function runSweep({ failRemove = [] } = {}) {
    const trace = [];
    const logs = [];
    const now = Date.now();
    const cutoff = now - SNAPSHOT_RETENTION_MS;
    const tmpCutoff = now - TMP_RESIDUAL_MS;

    // **常规清理之前**的总量（供违规变体 `account-before-cleanup` 用：
    // 协议 §S-16 裁定 2 要求核算排在常规清理**之后**）。
    const totalAtStart = measureTotal();

    /** 两步法删除（删不掉 ⇒ 只记日志，不抛）。 */
    const removeOne = (path, label) => {
      trace.push({ kind: 'rename', path });
      const complain = () => logs.push(`official-chat: 保留期清理失败：${label} 删不掉（已保留，未假装删掉）`);
      if (failRemove.includes(path)) {
        complain();
        return false;
      }
      const tombstone = `${path}.expired-${now}`;
      try {
        renameSync(path, tombstone);
      } catch {
        complain();
        return false;
      }
      trace.push({ kind: 'unlink', path: tombstone });
      try {
        unlinkSync(tombstone);
      } catch {
        complain();
        return false;
      }
      return true;
    };

    // ① **占用记录的全量扫描**（协议 §S-12③：先重建占用关系，再回收）。
    const referenced = new Set();
    const refsDir = join(root, 'refs');
    if (!bad.has('no-occupancy-scan')) {
      trace.push({ kind: 'readdir', path: refsDir });
      if (existsSync(refsDir)) {
        for (const name of readdirSync(refsDir)) {
          if (!name.endsWith('.json')) continue;
          const path = join(refsDir, name);
          trace.push({ kind: 'readfile', path });
          const parsed = readJson(path);
          if (parsed === null) {
            // **R3**：读不动 ⇒ 「被占着」无法证伪 ⇒ 按被占着处理。
            if (!bad.has('drop-broken-occupancy')) referenced.add(name.slice(0, -5));
            continue;
          }
          referenced.add(String(parsed.snapshotId ?? name.slice(0, -5)));
        }
      }

      // ①′ **R2 兜底释放**（协议 §S-11）：**过期**的占用记录 ⇒ 视为失效并删
      //    （由清理流程负责；**读坏的不删** —— R3 由 P3-5④ 覆盖）。
      //    ⇒ 放在**扫描之内**：真实实现里释放动作就住在那次全量扫描里
      //    （`releaseExpired()` 内部先 `scan()`）⇒ 不扫描 ⇒ 也就没有释放。
      if (existsSync(refsDir)) {
        for (const name of readdirSync(refsDir)) {
          if (!name.endsWith('.json')) continue;
          const path = join(refsDir, name);
          const record = readJson(path);
          if (record === null) continue; // **R3**：读不动 ⇒ 不删
          const stamp = Date.parse(String(record.createdAt ?? record.capturedAt ?? ''));
          const expired = Number.isFinite(stamp) && Date.now() - stamp > OCCUPANCY_RETENTION_MS;
          // **违规 `no-r2-release`**：一条也不释放。
          if (bad.has('no-r2-release')) continue;
          // **违规 `r2-deletes-fresh`**：连**没过期**的也删。
          if (!expired && !bad.has('r2-deletes-fresh')) continue;
          removeOne(path, `refs/${name}`);
          // **违规 `release-before-reclaim`**：释放的同时**把这份快照的占用关系也撤掉**
          // ⇒ 本轮就把那份快照删了（协议 §S-11 R2 补充口径要求「本轮不删」）。
          if (bad.has('release-before-reclaim')) {
            referenced.delete(String(record.snapshotId ?? name.slice(0, -5)));
          }
        }
      }
    }

    // ② **回收过期快照**（被占用的不动）。
    //    **违规 `no-snapshot-reclaim`**：**一个快照都不回收**。
    const snapDir = join(root, 'snapshots', providerId);
    if (!bad.has('no-snapshot-reclaim') && existsSync(snapDir)) {
      for (const name of readdirSync(snapDir)) {
        const path = join(snapDir, name);
        if (name.endsWith('.json')) {
          if (stampOf(path, 'capturedAt') >= cutoff) continue;
          const id = name.slice(0, -5);
          if (!bad.has('delete-occupied') && referenced.has(id)) continue;
          removeOne(path, `snapshots/${providerId}/${name}`);
          continue;
        }
        // ③ **临时残留**（第五家族）：形态匹配 ＋ 超过阈值。
        if (bad.has('no-tmp-cleanup')) continue;
        if (statSync(path).mtimeMs >= tmpCutoff) continue;
        removeOne(path, `snapshots/${providerId}/${name}`);
      }
    }

    // ④ **回收孤儿认领**（快照还在 ⇒ 不删）。
    const claimsRoot = join(root, 'seq-claims', providerId);
    if (existsSync(claimsRoot)) {
      for (const session of readdirSync(claimsRoot)) {
        const sessionDir = join(claimsRoot, session);
        if (!statSync(sessionDir).isDirectory()) continue;
        for (const name of readdirSync(sessionDir)) {
          if (!name.endsWith('.json')) continue;
          const path = join(sessionDir, name);
          if (stampOf(path, 'claimedAt') >= cutoff) continue;
          const claim = readJson(path);
          if (claim === null) continue;
          const snapshotId = `${claim.providerShort ?? providerId}~${claim.sessionShort ?? session}~${String(
            claim.seq ?? name.slice(0, -5),
          ).padStart(4, '0')}`;
          const stillThere = existsSync(
            join(root, 'snapshots', providerId, `${snapshotId}.json`),
          );
          if (stillThere && !bad.has('delete-claim-when-snapshot-exists')) continue;
          // **违规 `keep-claim`**：把孤儿认领**留着**（协议 §S-13 裁定 4① 禁止
          // 靠「保留认领文件本体」达成不复用 ⇒ 那样会让 P3-6 变红）。
          if (bad.has('keep-claim')) continue;
          removeOne(path, `seq-claims/${providerId}/${session}/${name}`);
        }
      }
    }

    // ⑤ **越界清理**（违规变体）：把缓存根之外的东西也删了。
    if (bad.has('cross-boundary')) {
      const parent = join(root, '..');
      for (const name of readdirSync(parent)) {
        if (!name.startsWith('dsh-p3-outside-')) continue;
        const path = join(parent, name);
        removeOne(path, name);
      }
    }

    // ⑤′ **常规清理：目录缓存的 TTL**（协议 §4 的 5 分钟 TTL）——**过期**的删、
    //    **TTL 内**的留着（**TTL 内的只有额外淘汰会动**，见 ⑥）。
    const cacheDir = join(root, 'catalog-cache');
    if (existsSync(cacheDir)) {
      for (const name of readdirSync(cacheDir)) {
        const full = join(cacheDir, name);
        let st;
        try {
          st = statSync(full);
        } catch {
          continue;
        }
        if (!st.isFile()) continue;
        if (st.mtimeMs >= Date.now() - SYNTH_CACHE_TTL_MS) continue;
        removeOne(full, `catalog-cache/${name}`);
      }
    }

    // ⑥ **核算 ＋ 额外淘汰**（协议 §S-16）：排在**常规清理之后** ⇒ 核算读到的是
    //    「本轮该回收的都回收了之后」的总量。
    //
    // **违规 `account-before-cleanup`**：核算读到的是**常规清理之前**的总量
    //    （协议 §S-16 裁定 2：核算排在『占用关系全量扫描 ＋ 常规清理』**之后**）。
    const measured = measureTotal();
    const before = bad.has('account-before-cleanup')
      ? {
          totalBytes: totalAtStart.totalBytes,
          snapshotsBytes: totalAtStart.snapshotsBytes,
          cacheBytes: totalAtStart.cacheBytes,
        }
      : measured;
    const budget = {
      totalBytes: before.totalBytes,
      snapshotsBytes: before.snapshotsBytes,
      cacheBytes: before.cacheBytes,
      budgetBytes: SYNTH_BUDGET,
      over: before.totalBytes > SYNTH_BUDGET,
      evictedCacheFiles: 0,
      totalBytesAfterEviction: before.totalBytes,
      stillOver: false,
    };
    if (budget.over) {
      // **违规 `no-eviction`**：超预算也**不**触发额外淘汰。
      if (!bad.has('no-eviction')) {
        const cacheDir = join(root, 'catalog-cache');
        if (existsSync(cacheDir)) {
          for (const name of readdirSync(cacheDir)) {
            const full = join(cacheDir, name);
            let st;
            try {
              st = statSync(full);
            } catch {
              continue;
            }
            if (!st.isFile()) continue;
            // TTL 已过的不是这一步该删的（那是常规清理的事）。
            if (st.mtimeMs < Date.now() - SYNTH_CACHE_TTL_MS) continue;
            if (removeOne(full, `catalog-cache/${name}`)) budget.evictedCacheFiles += 1;
          }
        }
      }
      // **违规 `evict-occupied`**：额外淘汰把**被占用的**快照也删了（协议 §S-16 裁定 4 禁止）。
      // **违规 `evict-unexpired`**：额外淘汰把**未到期**的快照也删了（P1 §4.1 第 3 条禁止）。
      if (bad.has('evict-occupied') || bad.has('evict-unexpired')) {
        const snapDir = join(root, 'snapshots', providerId);
        if (existsSync(snapDir)) {
          for (const name of readdirSync(snapDir)) {
            if (!name.endsWith('.json')) continue;
            const occupied = referenced.has(name.slice(0, -5));
            if (bad.has('evict-occupied') && !occupied) continue;
            if (bad.has('evict-unexpired') && occupied) continue;
            removeOne(join(snapDir, name), `snapshots/${providerId}/${name}`);
          }
        }
      }
      const after = measureTotal();
      budget.totalBytesAfterEviction = after.totalBytes;
      budget.stillOver = after.totalBytes > SYNTH_BUDGET;
      // **违规 `account-after-eviction`**：核算发生在额外淘汰**之后** ⇒ 自报的
      //    「核算总量」是**淘汰后**的数（协议 §S-16 裁定 2：核算排在额外淘汰**之前**）。
      if (bad.has('account-after-eviction')) {
        budget.totalBytes = after.totalBytes;
        budget.snapshotsBytes = after.snapshotsBytes;
        budget.cacheBytes = after.cacheBytes;
      }
      // **违规 `no-trace`**：超预算却**不留痕**（协议 §S-16 裁定 5 要求可读留痕）。
      if (!bad.has('no-trace')) {
        logs.push(
          `official-chat: 缓存总量超预算（核算总量 ${budget.totalBytes} 字节 > 预算 ${SYNTH_BUDGET} 字节）` +
            `——已淘汰目录缓存 ${budget.evictedCacheFiles} 个；淘汰后总量 ${budget.totalBytesAfterEviction} 字节` +
            `${budget.stillOver ? '（仍超预算：已按协议保留未到期快照与被占用的快照——尽力而为）' : '（已回到预算内）'}`,
        );
      }
    }

    logLines.push(...logs);
    return { trace, logs, threw: false, outcome: { budget }, broken: false };
  }

  return {
    root,
    providerId,
    retentionMs: SNAPSHOT_RETENTION_MS,
    tmpResidualMs: TMP_RESIDUAL_MS,
    occupancyRetentionMs: OCCUPANCY_RETENTION_MS,
    logLines,
    logger: { warn: (m) => logLines.push(String(m)) },
    isUnix: process.platform !== 'win32',

    store: {
      save(input) {
        saveSnapshot({
          snapshotId: String(input?.snapshotId ?? ''),
          accountScope: String(input?.accountScope ?? ''),
          sessionId: String(input?.sessionId ?? ''),
          branch: String(input?.branch ?? 'main'),
          capturedAt: String(input?.capturedAt ?? new Date().toISOString()),
          messages: Array.isArray(input?.messages) ? input.messages : [],
        });
      },
    },

    buildCommands,
    allocateSnapshotId,
    runSweep,

    /**
     * 造一套**新的**序号分配器（**每次都重新读盘** ⇒ 天然模拟「重启」）。
     *
     * @returns {{reserve: (sessionId: string) => {seq: number, snapshotId: string}}}
     */
    buildSeqAllocator,

    /**
     * 某个会话的**认领记录**清单（路径是**相对落盘根**的）。
     *
     * @param {string} sessionId
     * @returns {Array<object>}
     */
    claimsOf(sessionId) {
      const dir = claimsDirOf(sessionId);
      if (!existsSync(dir)) return [];
      const out = [];
      for (const name of readdirSync(dir)) {
        if (!/^\d+\.json$/.test(name)) continue;
        const full = join(dir, name);
        const parsed = readJson(full);
        out.push({
          path: full.slice(root.length + 1).split('\\').join('/'),
          providerShort: String(parsed?.providerShort ?? providerId),
          sessionShort: String(parsed?.sessionShort ?? sessionShort(sessionId)),
          seq: Number(parsed?.seq ?? name.slice(0, -5)),
          claimedAt: String(parsed?.claimedAt ?? ''),
        });
      }
      return out;
    },

    /**
     * 认领目录的**绝对路径**。
     *
     * @param {string} sessionId
     * @returns {string}
     */
    claimsDirOf,

    /**
     * 跑一次 `reserve` 并**记录写盘次序**（水位 vs 认领）。
     *
     * @param {string} sessionId
     * @returns {Promise<object>}
     */
    async runReserveWithTrace(sessionId) {
      const trace = [];
      traceSink = trace;
      let seq = null;
      let threw = false;
      let outcomeCode = '';
      let outcomeMessage = '';
      try {
        const allocated = buildSeqAllocator().reserve(sessionId);
        seq = allocated.seq;
      } catch (error) {
        threw = true;
        outcomeCode = String(error?.code ?? '');
        outcomeMessage = String(error?.message ?? error);
      } finally {
        traceSink = null;
      }
      return {
        trace,
        seq,
        threw,
        outcomeCode,
        outcomeMessage,
        outcome: outcomeCode || outcomeMessage,
        broken: false,
      };
    },

    /**
     * 把盘上状态补成「**已用满**」，再跑一次 `reserve` ⇒ 看它报什么。
     *
     * ## 为什么水位和认领**两边都补**
     *
     * 「取号口径」由实现方定（看水位 / 看认领）⇒ 只补一边 ⇒ 另一种口径的实现
     * 会认为「还没到顶」⇒ 判据会拿到一个**假绿** ⇒ 两边都补，两种口径都被驱动到上限。
     *
     * @param {string} sessionId
     * @param {number} maxSeq - 上限（由判据按协议给）。
     * @returns {Promise<object>}
     */
    async runAllocateAtLimit(sessionId, maxSeq) {
      writeWatermark(sessionId, maxSeq);
      const dir = claimsDirOf(sessionId);
      mkdirSync(dir, { recursive: true, mode: 0o700 });
      const path = join(dir, `${String(maxSeq).padStart(4, '0')}.json`);
      writeFileSync(
        path,
        `${JSON.stringify(
          {
            providerShort: providerId,
            sessionShort: sessionShort(sessionId),
            seq: maxSeq,
            claimedAt: new Date().toISOString(),
          },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      let seq = null;
      let threw = false;
      let outcomeCode = '';
      let outcomeMessage = '';
      try {
        seq = buildSeqAllocator().reserve(sessionId).seq;
      } catch (error) {
        threw = true;
        outcomeCode = String(error?.code ?? '');
        outcomeMessage = String(error?.message ?? error);
      }
      return {
        seq,
        threw,
        outcomeCode,
        outcomeMessage,
        outcome: outcomeCode || outcomeMessage,
        broken: false,
      };
    },

    async runInjectedCapture({ snapshotId, corrupt = false }) {
      const dir = join(root, 'snapshots', providerId);
      mkdirSync(dir, { recursive: true, mode: 0o700 });
      const text = `${JSON.stringify(
        {
          schemaVersion: 1,
          snapshotId,
          messages: [{ text: corrupt ? 'P3 被写坏的正文' : 'P3 判据正文' }],
        },
        null,
        2,
      )}\n`;
      // **合规**：写到临时名 ⇒ 最终名不出现；**违规**：直写最终名。
      const target = bad.has('direct-publish')
        ? join(dir, `${snapshotId}.json`)
        : join(dir, `.${snapshotId}.json.tmp-9001`);
      writeFileSync(target, text.slice(0, corrupt ? 40 : 20), 'utf8');
      return { threw: true, outcome: 'EIO', files: [target] };
    },

    finalNames() {
      const dir = join(root, 'snapshots', providerId);
      if (!existsSync(dir)) return [];
      return readdirSync(dir)
        .filter((name) => name.endsWith('.json') && name.slice(0, -5).split('~').length === 3)
        .sort();
    },
    hasFinalName: (snapshotId) =>
      existsSync(join(root, 'snapshots', providerId, `${snapshotId}.json`)),
    occupancyExists: (snapshotId) => existsSync(join(root, 'refs', `${snapshotId}.json`)),
    readOccupancy(snapshotId) {
      try {
        return { ok: true, value: JSON.parse(readFileSync(join(root, 'refs', `${snapshotId}.json`), 'utf8')) };
      } catch (error) {
        return { ok: false, error: String(error?.message ?? error) };
      }
    },
    readFile: (relativePath) => readFileSync(join(root, relativePath), 'utf8'),
    exists: (relativePath) => existsSync(join(root, relativePath)),
    existsAbsolute: (absolutePath) => existsSync(absolutePath),
    removeAbsolute: (absolutePath) => rmSync(absolutePath, { force: true }),
    mtimeMs: (relativePath) => statSync(join(root, relativePath)).mtimeMs,
    pathOf: (relativePath) => join(root, relativePath),
    listDir: (relativePath) => {
      const dir = join(root, relativePath);
      if (!existsSync(dir)) return [];
      return readdirSync(dir).sort();
    },
    ensureDir: (relativePath) => mkdirSync(join(root, relativePath), { recursive: true }),
    /**
     * 目录下的**全部文件**（**递归**；相对该目录的路径 ＋ 字节数）。
     *
     * 与真实 adapter 的同名端口**同口径** ⇒ 判据的独立加总两边一致。
     *
     * @param {string} relativePath - 相对落盘根的目录。
     * @returns {Array<{path: string, size: number}>}
     */
    listFiles: (relativePath) => {
      const dir = join(root, relativePath);
      const out = [];
      const walk = (current, prefix) => {
        let entries = [];
        try {
          entries = readdirSync(current, { withFileTypes: true });
        } catch {
          return;
        }
        for (const entry of entries) {
          const full = join(current, entry.name);
          const rel = prefix === '' ? entry.name : `${prefix}/${entry.name}`;
          if (entry.isDirectory()) {
            walk(full, rel);
            continue;
          }
          try {
            const st = statSync(full);
            if (st.isFile()) out.push({ path: rel, size: st.size });
          } catch {
            /* 取不到就当它不存在 */
          }
        }
      };
      walk(dir, '');
      return out;
    },
    /**
     * 造一份**指定大小**的文件（把缓存总量「做到超预算」用）。
     *
     * @param {string} relativePath - 相对落盘根的路径。
     * @param {number} bytes - 文件长度（字节）。
     * @returns {string} 绝对路径。
     */
    writeBigFile: (relativePath, bytes) => {
      const full = join(root, relativePath);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(full, '', 'utf8');
      truncateSync(full, Number(bytes));
      return full;
    },
    writeFile(relativePath, content) {
      const full = join(root, relativePath);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(full, String(content), 'utf8');
      return full;
    },
    writeFileAbsolute(absolutePath, content) {
      writeFileSync(absolutePath, String(content), 'utf8');
      return absolutePath;
    },
    setMtimeOld(relativePath, ageMs) {
      const full = join(root, relativePath);
      const when = new Date(Date.now() - Number(ageMs));
      utimesSync(full, when, when);
    },
    modeOf: (relativePath) => statSync(join(root, relativePath)).mode,
    readRepoFile(relativePath) {
      if (bad.has('no-policy-doc')) {
        // **违规**：协议稿里没有 §S-14 的可检索条目。
        return relativePath.includes('p3-protocol') ? '# P3 协议\n\n（没有 §S-14 这一条）\n' : '# P1 协议\n';
      }
      return relativePath.includes('p3-protocol')
        ? '# P3 协议\n\n### S-14｜存储保护\n\n落盘策略正文。\n'
        : '# P1 协议\n\n另见 official-chat-p3-protocol.md 的 §S-14。\n';
    },
    /**
     * 读 host 半边的**源码**（Windows 侧「断源码形态」的输入）。
     *
     * @param {string} fileName - 文件名。
     * @returns {string} 源码正文。
     */
    readHostFile(fileName) {
      const anchors = {
        'commands.js': 'export function createCatalogCache',
        'index.js': 'export function createSnapshotStore',
        'seq-store.js': 'export function createSeqAllocator',
      };
      const anchor = anchors[String(fileName)] ?? 'export function createUnknown';
      // **违规**：写盘点不带权限参数（⇒ Unix 侧盘上权限不达标、Windows 侧源码形态也不达标）。
      if (bad.has('no-mode-args')) {
        return [
          `${anchor} {`,
          '  function write() {',
          '    mkdirSync(dir, { recursive: true });',
          "    writeFileSync(path, text, 'utf8');",
          '  }',
          '}',
        ].join('\n');
      }
      return [
        `${anchor} {`,
        '  function write() {',
        '    mkdirSync(dir, { recursive: true, mode: 0o700 });',
        "    writeFileSync(path, text, { encoding: 'utf8', mode: 0o600 });",
        '  }',
        '}',
      ].join('\n');
    },
    scanEncryptionClaims: () =>
      bad.has('encryption-claim') ? ['plugins/official-chat/host/index.js:1：正文已加密存储'] : [],

    writeSnapshot(snapshotId, fields = {}) {
      const full = join(root, 'snapshots', providerId, `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      const stored = {
        schemaVersion: 1,
        snapshotId,
        providerId,
        accountScope: 'p3-account',
        sessionId: 'p3-session',
        branch: 'main',
        messages: [{ text: 'P3 判据正文' }],
        contentDigest: '',
        capturedAt: '2026-10-21T00:00:00.000Z',
        ...fields,
      };
      writeFileSync(full, `${JSON.stringify(stored, null, 2)}\n`, { mode: 0o600 });
      return full;
    },
    writeOccupancy(snapshotId, value) {
      const full = join(root, 'refs', `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      const content = typeof value === 'string' ? value : `${JSON.stringify(value, null, 2)}\n`;
      writeFileSync(full, content, { mode: 0o600 });
      return full;
    },
    /**
     * 写一份**过期**的占用记录（协议 §S-11 R2 的夹具）。
     *
     * **三个时间信号都调旧**（`createdAt` / `capturedAt` / mtime）⇒ 任何一种
     * 「过期」口径都成立。
     */
    writeOccupancyAged(snapshotId, ageMs) {
      const iso = new Date(Date.now() - Number(ageMs)).toISOString();
      const full = join(root, 'refs', `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(
        full,
        `${JSON.stringify(
          { snapshotId, capturedAt: iso, createdAt: iso, sessionId: 'p3-session' },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      const seconds = (Date.now() - Number(ageMs)) / 1000;
      utimesSync(full, seconds, seconds);
      return full;
    },
    /** 写一份**过期**的快照（R2 的「随后可被回收」那一步的夹具）。 */
    writeSnapshotAged(snapshotId, ageMs) {
      const iso = new Date(Date.now() - Number(ageMs)).toISOString();
      const full = join(root, 'snapshots', providerId, `${snapshotId}.json`);
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(
        full,
        `${JSON.stringify(
          {
            schemaVersion: 1,
            snapshotId,
            providerId,
            accountScope: 'p3-account',
            sessionId: 'p3-session',
            branch: 'main',
            messages: [{ text: 'P3 判据正文' }],
            contentDigest: '',
            capturedAt: iso,
          },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      const seconds = (Date.now() - Number(ageMs)) / 1000;
      utimesSync(full, seconds, seconds);
      return full;
    },
    /**
     * 调用 `release` 命令（协议 §S-11 R1 写死的名字）。
     *
     * **只认 `release`**（协议写死的那个）；有别名就如实报出来让判据点名。
     */
    async callRelease({ snapshotId } = {}) {
      const commands = buildCommands();
      const available = typeof commands.release === 'function';
      const alias = available
        ? null
        : typeof commands.releaseOccupancy === 'function'
          ? 'releaseOccupancy'
          : null;
      if (!available) {
        return {
          available: false,
          alias,
          ok: null,
          released: null,
          errorCode: '',
          errorMessage: '',
          threw: false,
          broken: false,
        };
      }
      try {
        const result = await commands.release({ snapshotId });
        return {
          available: true,
          alias: null,
          ok: result?.ok === true,
          released: result?.released ?? null,
          errorCode: String(result?.error?.code ?? result?.code ?? ''),
          errorMessage: String(result?.error?.message ?? result?.detail ?? ''),
          threw: false,
          broken: false,
        };
      } catch (error) {
        return {
          available: true,
          alias: null,
          ok: null,
          released: null,
          errorCode: String(error?.code ?? ''),
          errorMessage: String(error?.message ?? error),
          threw: true,
          broken: false,
        };
      }
    },
    writeClaim(snapshotId) {
      const parts = String(snapshotId).split('~');
      const providerShort = parts[0];
      const sessionShort_ = parts[1];
      const seq = parts[2];
      const full = join(
        root,
        'seq-claims',
        providerShort,
        sessionShort_,
        `${seq.padStart(4, '0')}.json`,
      );
      mkdirSync(join(full, '..'), { recursive: true });
      writeFileSync(
        full,
        `${JSON.stringify(
          {
            providerShort,
            sessionShort: sessionShort_,
            seq: Number(seq),
            claimedAt: '2026-01-01T00:00:00.000Z',
          },
          null,
          2,
        )}\n`,
        { mode: 0o600 },
      );
      return full;
    },

    cleanup() {
      removeTempRoot(root);
    },
  };
}

/** 违规点清单（**每个违规点都必须让对应的那条判据转红**）。 */
const VIOLATIONS = [
  { violation: 'direct-publish', criterion: 'P3-1', expect: /半条记录被发布/ },
  { violation: 'direct-publish', criterion: 'P3-2', expect: /最终名仍然存在/ },
  { violation: 'no-tmp-cleanup', criterion: 'P3-3', expect: /临时残留/ },
  { violation: 'cross-boundary', criterion: 'P3-3', expect: /缓存根之外/ },
  { violation: 'no-occupancy-record', criterion: 'P3-4', expect: /占用记录/ },
  { violation: 'delete-occupied', criterion: 'P3-5', expect: /有占用记录/ },
  { violation: 'no-occupancy-scan', criterion: 'P3-5', expect: /全量扫描/ },
  { violation: 'drop-broken-occupancy', criterion: 'P3-5', expect: /写坏/ },
  { violation: 'delete-claim-when-snapshot-exists', criterion: 'P3-6', expect: /认领/ },
  { violation: 'keep-claim', criterion: 'P3-6', expect: /孤儿认领/ },
  { violation: 'log-body', criterion: 'P3-7', expect: /正文/ },
  { violation: 'encryption-claim', criterion: 'P3-7', expect: /加密/ },
  // **权限这一项在两个平台上的观测面不同**（Unix 断盘上真实权限；Windows 断源码形态）
  // ⇒ 同一个违规点**两个平台都得红** ⇒ 期望值写成「权限|mode」这种跨平台的说法。
  { violation: 'no-mode-args', criterion: 'P3-7', expect: /权限|mode: 0o/ },
  { violation: 'no-policy-doc', criterion: 'P3-8', expect: /S-14/ },
  { violation: 'scope-from-request', criterion: 'P3-9', expect: /accountScope|account_changed/ },
  // P3-10（§S-13 裁定 4）：**四种不同的「不复用」破法**，各红一处。
  { violation: 'no-watermark', criterion: 'P3-10', expect: /水位/ },
  { violation: 'claim-first', criterion: 'P3-10', expect: /晚于/ },
  { violation: 'new-limit-error', criterion: 'P3-10', expect: /不许换形态/ },
  { violation: 'wrap-at-limit', criterion: 'P3-10', expect: /不许绕/ },
  // P3-11（§S-11 R1/R2）：**释放路径**的各种破法，各红一处。
  { violation: 'release-missing', criterion: 'P3-11', expect: /没有 `release` 这个 host 命令/ },
  { violation: 'release-noop', criterion: 'P3-11', expect: /占用记录还在/ },
  { violation: 'release-cross', criterion: 'P3-11', expect: /被删了|被误删/ },
  { violation: 'release-not-idempotent', criterion: 'P3-11', expect: /重复释放同一个 id 居然失败/ },
  { violation: 'release-wrong-shape', criterion: 'P3-11', expect: /没有 `released` 这个布尔/ },
  { violation: 'no-r2-release', criterion: 'P3-11', expect: /过期的占用记录没被删/ },
  { violation: 'r2-deletes-fresh', criterion: 'P3-11', expect: /没过期的占用记录被删了/ },
  // P3-12（R2 的「本轮 / 下一轮」口径）：**两种破法**，各红一处。
  { violation: 'release-before-reclaim', criterion: 'P3-12', expect: /就把那份过期快照删了/ },
  { violation: 'no-snapshot-reclaim', criterion: 'P3-12', expect: /仍在/ },
  // P3-13（§S-16 核算 ＋ 额外淘汰）：**五种破法**，各红一处。
  // 期望值写**红字里的分层标记**（【核算范围】…）⇒ 不写成「含某串」：
  // 结论层（缺哪条协议）里也会出现「核算范围/额外淘汰/留痕」这些词 ⇒ 用它们当期望值
  // 会让每个变体都「命中」⇒ **空断言**。
  { violation: 'count-excluded', criterion: 'P3-13', expect: /【核算范围】/ },
  { violation: 'no-eviction', criterion: 'P3-13', expect: /【额外淘汰】/ },
  { violation: 'evict-occupied', criterion: 'P3-13', expect: /【被占用不删】/ },
  { violation: 'evict-unexpired', criterion: 'P3-13', expect: /【未到期不删】/ },
  { violation: 'no-trace', criterion: 'P3-13', expect: /【留痕】/ },
  // P3-14（§S-16 裁定 2 的**核算时机**）：**两种破法**，各红一处。
  { violation: 'account-before-cleanup', criterion: 'P3-14', expect: /【核算时机·清理后】/ },
  { violation: 'account-after-eviction', criterion: 'P3-14', expect: /【核算时机·淘汰前】/ },
];

// ---------------------------------------------------------------------------
// 活体注入的脚手架（副本 + 正则锚点 + 逐字节还原）
// ---------------------------------------------------------------------------

/** 本轮自测建的临时目录（收尾时**只删自己建的**）。 */
const createdDirs = [];

/**
 * 把 host 半边**整目录**拷到一次性临时目录。
 *
 * 目录里的相对 import 在副本里同样成立 ⇒ 注入能跑通。
 *
 * @param {string} tag
 * @returns {string} 副本目录。
 */
function copyHostDir(tag) {
  const dir = join(
    tmpdir(),
    `dsh-p3-selftest-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
  );
  mkdirSync(dir, { recursive: true });
  for (const name of readdirSync(HOST_DIR)) {
    copyFileSync(join(HOST_DIR, name), join(dir, name));
  }
  createdDirs.push(dir);
  return dir;
}

/**
 * 在**副本**上注入一个缺陷（锚点用**正则 + 形状容忍**）。
 *
 * @param {string} path - 副本路径。
 * @param {RegExp} pattern - 锚点正则。
 * @param {string} replacement - 替换文本。
 * @returns {number} 命中次数（0 ⇒ 锚点不见了 ⇒ 用例必须失败）。
 */
function inject(path, pattern, replacement) {
  const text = readFileSync(path, 'utf8');
  const matches = text.match(
    new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`),
  );
  if (matches === null || matches.length === 0) {
    throw new Error(`注入锚点不见了：${pattern}`);
  }
  writeFileSync(path, text.replace(pattern, replacement), 'utf8');
  return matches.length;
}

/** 文件的 SHA-256（**原实现一个字节都不碰**的证据）。 */
function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

test.after(() => {
  for (const dir of createdDirs) {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// 正反两个方向（合成适配器）
// ---------------------------------------------------------------------------

test('合成适配器（一份合规实现）⇒ 十四条判据全绿（判据是可满足的）', async () => {
  for (const spec of CRITERIA) {
    const adapter = makeSyntheticAdapter({ tag: `synth-ok-${spec.id}` });
    let result;
    try {
      result = await spec.run(adapter);
    } finally {
      adapter.cleanup();
    }
    assert.equal(result.status, PASS, `${spec.id} 对合规实现必须绿：${result.detail}`);
  }
});

for (const spec of VIOLATIONS) {
  test(`违规点「${spec.violation}」⇒ ${spec.criterion} 红并点名（合成适配器）`, async () => {
    const target = CRITERIA.find((one) => one.id === spec.criterion);
    assert.ok(target, `判据 ${spec.criterion} 不存在`);
    const adapter = makeSyntheticAdapter({
      violations: [spec.violation],
      tag: `synth-bad-${spec.violation}-${spec.criterion}`,
    });
    let result;
    try {
      result = await target.run(adapter);
    } finally {
      adapter.cleanup();
    }
    assert.equal(result.status, FAIL, `违规后判据必须红，实际：${result.status} ${result.detail}`);
    assert.match(result.detail, spec.expect, `判据必须点名缺哪一条协议：${result.detail}`);
  });
}

// ---------------------------------------------------------------------------
// 隔离性：每个违规变体**只红自己那一支**（一支红、另一支绿）
// ---------------------------------------------------------------------------

/** P3-13 / P3-14 各支断言在红字里的**分层标记**（红字可按处豁免 ⇒ 标记就是「处」）。 */
const CRITERION_MARKERS = [
  '【核算范围】',
  '【额外淘汰】',
  '【被占用不删】',
  '【未到期不删】',
  '【留痕】',
  '【核算时机·清理后】',
  '【核算时机·淘汰前】',
];

/** 违规变体 ⇒ {判据, 它**应当**红的那一支}。 */
const VIOLATION_MARKERS = {
  'count-excluded': { criterion: checkP3_13_cacheBudget, marker: '【核算范围】' },
  'no-eviction': { criterion: checkP3_13_cacheBudget, marker: '【额外淘汰】' },
  'evict-occupied': { criterion: checkP3_13_cacheBudget, marker: '【被占用不删】' },
  'evict-unexpired': { criterion: checkP3_13_cacheBudget, marker: '【未到期不删】' },
  'no-trace': { criterion: checkP3_13_cacheBudget, marker: '【留痕】' },
  'account-before-cleanup': { criterion: checkP3_14_accountTiming, marker: '【核算时机·清理后】' },
  'account-after-eviction': { criterion: checkP3_14_accountTiming, marker: '【核算时机·淘汰前】' },
};

test('每个违规变体**只红自己那一支**（一支红、另一支绿 ⇒ 各支都不是空跑）', async () => {
  for (const [violation, spec] of Object.entries(VIOLATION_MARKERS)) {
    const adapter = makeSyntheticAdapter({ violations: [violation], tag: `synth-iso-${violation}` });
    let result;
    try {
      result = await spec.criterion(adapter);
    } finally {
      adapter.cleanup();
    }
    assert.equal(result.status, FAIL, `违规「${violation}」必须让判据红：${result.detail}`);
    assert.ok(
      result.detail.includes(spec.marker),
      `违规「${violation}」必须红在「${spec.marker}」这一支：${result.detail}`,
    );
    for (const other of CRITERION_MARKERS) {
      if (other === spec.marker) continue;
      assert.ok(
        !result.detail.includes(other),
        `违规「${violation}」不该连累「${other}」这一支（否则证明不了它非空跑）：${result.detail}`,
      );
    }
  }
});

// ---------------------------------------------------------------------------
// 判据自身的钉子（**盯着判据自己有没有把期望值改松**）
// ---------------------------------------------------------------------------

test('判据写死的序号上限必须是 9999（协议 §S-1 的 4 位上限）', () => {
  // **为什么要有这颗钉子**：上限是**判据写死**的期望值（实现改常量 ⇒ 判据不改）
  // ⇒ 那就得有人盯着「判据自己有没有把上限改松」⇒ 这颗钉子断的是**判据库的常量**。
  assert.equal(CRITERION_MAX_SEQ, 9999, '判据的序号上限常量不许改（协议 §S-1）');
});

test('判据写死的缓存总量上限必须是 512 MB（协议 §4 `cacheTotalBudgetBytes`）', () => {
  // **为什么要有这颗钉子**：P3-13 的判据**自己**算「总量有没有超预算」⇒ 预算值是
  // **判据的期望值**（实现改常量 ⇒ 判据不改）⇒ 得有人盯着它没被改松。
  assert.equal(
    CRITERION_CACHE_BUDGET_BYTES,
    512 * 1024 * 1024,
    '判据的缓存总量上限常量不许改（协议 §4 `cacheTotalBudgetBytes` ＝ 512 MB）',
  );
});

// ---------------------------------------------------------------------------
// 联动：P3-6 × P3-10（**这条最容易搞错**）
// ---------------------------------------------------------------------------

test('联动：靠「保留认领」达成不复用 ⇒ P3-6 红、P3-10 红，但 P3-10 **不得指控「seq 被复用」**', async () => {
  const p36Adapter = makeSyntheticAdapter({ violations: ['keep-claim'], tag: 'synth-keep-claim-p36' });
  let p36 = null;
  try {
    p36 = await checkP3_6_orphanClaimReclaimed(p36Adapter);
  } finally {
    p36Adapter.cleanup();
  }
  const p310Adapter = makeSyntheticAdapter({ violations: ['keep-claim'], tag: 'synth-keep-claim-p310' });
  let p310 = null;
  try {
    p310 = await checkP3_10_seqNotReused(p310Adapter);
  } finally {
    p310Adapter.cleanup();
  }

  // ① P3-6 抓「孤儿认领没被删」（协议 §S-13 裁定 4①：不得靠保留认领达成不复用）。
  assert.equal(p36.status, FAIL, `保留孤儿认领必须让 P3-6 红：${p36.detail}`);
  assert.match(p36.detail, /孤儿认领/, 'P3-6 必须点名孤儿认领没被删');
  // ② P3-10 **不许报绿**（场景不成立 ≠ 通过 ⇒ 不许给一条没跑成的用例盖「通过」的章）。
  assert.equal(p310.status, FAIL, `场景不成立时 P3-10 不许报绿：${p310.detail}`);
  // ③ **但也不许反过来指控「seq 被复用」**——
  //    P3-10 断的是「`seq` 不复用」，**不是**「认领文件还在」
  //    （断「认领还在」是**断反了**：那是协议 §S-13 裁定 4① **禁止**的形态）。
  assert.doesNotMatch(
    p310.detail,
    /又被分配|又分配到了被回收的 seq/,
    `P3-10 不得指控 seq 被复用：${p310.detail}`,
  );
});

// ---------------------------------------------------------------------------
// Windows 侧的「不许空跑通过」（§S-14 裁定 2）
// ---------------------------------------------------------------------------

test(
  'Windows 上 P3-7 的「盘上权限」显式记「未执行」，且改断源码形态（不许空跑通过）',
  { skip: process.platform !== 'win32' },
  async () => {
    const adapter = makeSyntheticAdapter({ tag: 'synth-p3-7-win' });
    let result;
    try {
      result = await checkP3_7_storageProtection(adapter);
    } finally {
      adapter.cleanup();
    }
    assert.equal(result.status, PASS, `合规实现在 Windows 上也必须绿：${result.detail}`);
    // **显式记「未执行」**（不许「没有 else 的 if」那种空跑通过）。
    assert.match(result.detail, /未执行/, 'Windows 侧必须显式记「未执行」');
    // **而且不是空跑**：源码形态这条**要真的在跑**（去掉权限参数就得红）。
    const broken = makeSyntheticAdapter({ violations: ['no-mode-args'], tag: 'synth-p3-7-win-bad' });
    let brokenResult;
    try {
      brokenResult = await checkP3_7_storageProtection(broken);
    } finally {
      broken.cleanup();
    }
    assert.equal(
      brokenResult.status,
      FAIL,
      `Windows 侧去掉权限参数后必须红（空跑通过＝给没跑的用例盖了章）：${brokenResult.detail}`,
    );
  },
);

// ---------------------------------------------------------------------------
// 活体注入（对**真实实现的副本**；原实现一个字节都不碰）
// ---------------------------------------------------------------------------

test('活体注入（P3-9）：把账户维度改回「取自请求参数」⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  const original = join(HOST_DIR, 'commands.js');
  const before = sha256File(original);
  const dir = copyHostDir('inject-p3-9');
  const copyPath = join(dir, 'commands.js');

  // ① 账户维度改回取自请求参数（`catalog` ＋ `capture` 两处）。
  const scopeHits = inject(
    copyPath,
    /const scope = String\(accountScope\);/g,
    'const scope = String(request.accountScope ?? accountScope);',
  );
  assert.ok(scopeHits >= 2, `注入应当命中 catalog ＋ capture 两处，实际 ${scopeHits} 处`);
  // ② 「不符即拒」的校验失效（服务端推导值不再参与比对）。
  const guardHits = inject(
    copyPath,
    /if \(String\(provided\) === String\(derived\)\) return null;/g,
    'return null;',
  );
  assert.ok(guardHits >= 1, '注入应当命中「不符即拒」的判定行');

  const adapter = await createP3Adapter({ tag: 'p3-inject-p3-9', hostDir: dir });
  let result;
  try {
    result = await checkP3_9_accountScopeServerDerived(adapter);
  } finally {
    adapter.cleanup();
  }
  assert.equal(result.status, FAIL, `注入后判据必须红，实际：${result.status} ${result.detail}`);
  assert.match(result.detail, /accountScope|account_changed/, '判据必须点名缺哪一条协议');

  // **原实现一个字节都没碰**。
  assert.equal(sha256File(original), before, '原实现文件被改了 —— 注入只能打在副本上');
});

test('活体注入（P3-10）：把到上限的错误码换形态 ⇒ 判据点名那个新码 ⇒ 原实现逐字节未动', async () => {
  const original = join(HOST_DIR, 'seq-store.js');
  const before = sha256File(original);

  // ① **基线**：未注入的副本 ⇒ 红字里**不该**出现注入要放进去的那个码
  //    （⇒ 证明注入**制造了一个新的、可点名的红因**，不是「本来就红」）。
  const baseDir = copyHostDir('inject-p3-10-base');
  const baseAdapter = await createP3Adapter({ tag: 'p3-inject-p3-10-base', hostDir: baseDir });
  let baseResult;
  try {
    baseResult = await checkP3_10_seqNotReused(baseAdapter);
  } finally {
    baseAdapter.cleanup();
  }
  assert.doesNotMatch(
    baseResult.detail,
    /budget_exceeded/,
    `基线不该出现注入后的错误码：${baseResult.detail}`,
  );

  // ② **注入**：到上限时的错误码从 `source_unsupported` 换成 `budget_exceeded`（**换形态**）。
  const dir = copyHostDir('inject-p3-10');
  const copyPath = join(dir, 'seq-store.js');
  const hits = inject(
    copyPath,
    /throw seqError\(\s*'source_unsupported',/g,
    "throw seqError(\n      'budget_exceeded',",
  );
  assert.ok(hits >= 1, `注入应当命中断言上限错误的那一处，实际 ${hits} 处`);

  const adapter = await createP3Adapter({ tag: 'p3-inject-p3-10', hostDir: dir });
  let result;
  try {
    result = await checkP3_10_seqNotReused(adapter);
  } finally {
    adapter.cleanup();
  }
  assert.equal(result.status, FAIL, `注入后判据必须红，实际：${result.status} ${result.detail}`);
  assert.match(result.detail, /budget_exceeded/, '判据必须点名注入进去的那个新错误码');
  assert.match(result.detail, /序号已用尽|不许换形态/, '判据必须点明「既有口径不许换形态」');

  // **原实现一个字节都没碰**。
  assert.equal(sha256File(original), before, '原实现文件被改了 —— 注入只能打在副本上');
});

test('活体注入（P3-11）：让释放那一步的删除**真失败** ⇒ 判据红并点名「占用记录还在」⇒ 原实现逐字节未动', async (t) => {
  const original = join(HOST_DIR, 'commands.js');
  const before = sha256File(original);
  const dir = copyHostDir('inject-p3-11');

  // ① **基线**：未注入的副本跑一遍。
  //    ⇒ 这一步同时回答「这份实现有没有 `release` 入口」：
  //    **没有 ⇒ 这个注入用例不适用**（场景不成立 ⇒ 跳过，**不**当通过、**不**当红）。
  const baseAdapter = await createP3Adapter({ tag: 'p3-inject-p3-11-base', hostDir: dir });
  let baseResult;
  try {
    baseResult = await checkP3_11_releasePaths(baseAdapter);
  } finally {
    baseAdapter.cleanup();
  }
  if (/没有 `release` 这个 host 命令/.test(baseResult.detail)) {
    t.skip('这份实现还没有 `release` 入口 ⇒ 注入用例暂不适用（等实现落地后自动生效）');
    return;
  }

  // ② **注入**：在**子进程**里 patch `node:fs` 的 `renameSync` / `unlinkSync`
  //    ⇒ 释放那一步的**删除真失败**（EACCES）⇒ 释放**没有真的删掉**记录。
  //    ⇒ 按裁定：**判据侧制造真失败**，不要求实现加测试钩子、不改签名。
  const injectedAdapter = await createP3Adapter({
    tag: 'p3-inject-p3-11',
    hostDir: dir,
    failReleaseDelete: true,
  });
  let result;
  try {
    result = await checkP3_11_releasePaths(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(result.status, FAIL, `注入后判据必须红，实际：${result.status} ${result.detail}`);
  assert.match(result.detail, /占用记录还在/, '判据必须点名「释放之后占用记录还在」');
  // **基线里没有这条红因** ⇒ 注入制造的是**新的**、可点名的红因。
  assert.doesNotMatch(
    baseResult.detail,
    /占用记录还在/,
    `基线不该有这条红因（注入没有制造出新东西）：${baseResult.detail}`,
  );

  // **原实现一个字节都没碰**。
  assert.equal(sha256File(original), before, '原实现文件被改了 —— 注入只能打在副本上');
});

test('活体注入（P3-12）：让 `refs/` 下的删除**真失败** ⇒ R2 释放不发生 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async (t) => {
  const original = join(HOST_DIR, 'refs-store.js');
  const before = sha256File(original);
  const dir = copyHostDir('inject-p3-12');

  // ① **基线**：未注入的副本跑一遍 ⇒ 这条判据在真实实现上**本来就该是绿的**
  //    （它是**回归保护**，不是「事实登记」）⇒ 基线不绿 ⇒ 注入用例不适用。
  const baseAdapter = await createP3Adapter({ tag: 'p3-inject-p3-12-base', hostDir: dir });
  let baseResult;
  try {
    baseResult = await checkP3_12_twoRoundRelease(baseAdapter);
  } finally {
    baseAdapter.cleanup();
  }
  if (baseResult.status !== PASS) {
    t.skip(`基线不是绿的（${baseResult.status}：${String(baseResult.detail).slice(0, 80)}）⇒ 注入用例暂不适用`);
    return;
  }

  // ② **注入**：在**子进程**里 patch `node:fs` 的 `renameSync` ⇒ `refs/` 下的删除
  //    **真失败**（EACCES）⇒ **R2 释放不发生**（占用记录还在）⇒
  //    「R2 释放当轮即可断言」这一半被推翻 ⇒ 判据必须红。
  //    ⇒ 按裁定：**判据侧制造真失败**，不要求实现加测试钩子、不改签名。
  const injectedAdapter = await createP3Adapter({
    tag: 'p3-inject-p3-12',
    hostDir: dir,
    failSweepRemoveUnder: 'refs',
  });
  let result;
  try {
    result = await checkP3_12_twoRoundRelease(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(result.status, FAIL, `注入后判据必须红，实际：${result.status} ${result.detail}`);
  assert.match(result.detail, /那份过期占用记录还在/, '判据必须点名「过期占用记录没被删」');
  // **基线里没有这条红因** ⇒ 注入制造的是**新的**、可点名的红因。
  assert.doesNotMatch(
    baseResult.detail,
    /那份过期占用记录还在/,
    `基线不该有这条红因（注入没有制造出新东西）：${baseResult.detail}`,
  );

  // **原实现一个字节都没碰**。
  assert.equal(sha256File(original), before, '原实现文件被改了 —— 注入只能打在副本上');
});

test('活体注入（P3-13）：让 `catalog-cache/` 下的删除**真失败** ⇒ 额外淘汰没删掉 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async (t) => {
  const original = join(HOST_DIR, 'sweep.js');
  const before = sha256File(original);
  const dir = copyHostDir('inject-p3-13');

  // ① **基线**：未注入的副本跑一遍 ⇒ 这条判据在真实实现上**本来就该是绿的**
  //    （它是**回归保护**）⇒ 基线不绿 ⇒ 注入用例不适用（跳过，**不**当通过）。
  const baseAdapter = await createP3Adapter({ tag: 'p3-inject-p3-13-base', hostDir: dir });
  let baseResult;
  try {
    baseResult = await checkP3_13_cacheBudget(baseAdapter);
  } finally {
    baseAdapter.cleanup();
  }
  if (baseResult.status !== PASS) {
    t.skip(
      `基线不是绿的（${baseResult.status}：${String(baseResult.detail).slice(0, 120)}）⇒ 注入用例暂不适用`,
    );
    return;
  }

  // ② **注入**：在**子进程**里 patch `node:fs` 的 `renameSync` ⇒ `catalog-cache/` 下的
  //    删除**真失败**（EACCES）⇒ **额外淘汰没删掉**那些**尚未超过 TTL（5 分钟）**的目录缓存 ⇒
  //    「超预算 ⇒ 触发额外淘汰」这一支被推翻 ⇒ 判据必须红。
  //    ⇒ 按裁定：**判据侧制造真失败**，不要求实现加测试钩子、不改签名。
  const injectedAdapter = await createP3Adapter({
    tag: 'p3-inject-p3-13',
    hostDir: dir,
    failSweepRemoveUnder: 'catalog-cache',
  });
  let result;
  try {
    result = await checkP3_13_cacheBudget(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(result.status, FAIL, `注入后判据必须红，实际：${result.status} ${result.detail}`);
  assert.match(
    result.detail,
    /【额外淘汰】/,
    '判据必须点名「尚未超过 TTL（5 分钟）的目录缓存没被删（额外淘汰没生效）」',
  );
  // **基线里没有这条红因** ⇒ 注入制造的是**新的**、可点名的红因。
  assert.doesNotMatch(
    baseResult.detail,
    /【额外淘汰】/,
    `基线不该有这条红因（注入没有制造出新东西）：${baseResult.detail}`,
  );

  // **原实现一个字节都没碰**。
  assert.equal(sha256File(original), before, '原实现文件被改了 —— 注入只能打在副本上');
});

test('活体注入（P3-14）：把核算挪到**常规清理之前** ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async (t) => {
  const original = join(HOST_DIR, 'sweep.js');
  const before = sha256File(original);
  const dir = copyHostDir('inject-p3-14');

  // ① **基线**：未注入的副本跑一遍 ⇒ 这条判据在真实实现上**本来就该是绿的**
  //    （协议 §S-16 裁定 2 的口径，实现已落地）⇒ 基线不绿 ⇒ 注入用例不适用。
  const baseAdapter = await createP3Adapter({ tag: 'p3-inject-p3-14-base', hostDir: dir });
  let baseResult;
  try {
    baseResult = await checkP3_14_accountTiming(baseAdapter);
  } finally {
    baseAdapter.cleanup();
  }
  if (baseResult.status !== PASS) {
    t.skip(
      `基线不是绿的（${baseResult.status}：${String(baseResult.detail).slice(0, 120)}）⇒ 注入用例暂不适用`,
    );
    return;
  }

  // ② **注入**：在**副本**上把 `applyCacheBudget()` 挪到**常规清理之前**
  //    （协议 §S-16 裁定 2：核算排在『占用关系全量扫描 ＋ 常规清理』**之后**）
  //    ⇒ 核算读到的是**清理前**的旧数 ⇒ 判据必须红。
  const copyPath = join(dir, 'sweep.js');
  const earlyHits = inject(
    copyPath,
    /const snapshots = sweepSnapshots\(occupancy\.occupied\);/g,
    'const budgetEarly = applyCacheBudget();\n    const snapshots = sweepSnapshots(occupancy.occupied);',
  );
  assert.ok(earlyHits >= 1, '注入应当命中「回收快照」那一行');
  const lateHits = inject(
    copyPath,
    /const budget = applyCacheBudget\(\);/g,
    'const budget = budgetEarly;',
  );
  assert.ok(lateHits >= 1, '注入应当命中「核算 ＋ 额外淘汰」那一行');

  const injectedAdapter = await createP3Adapter({ tag: 'p3-inject-p3-14', hostDir: dir });
  let result;
  try {
    result = await checkP3_14_accountTiming(injectedAdapter);
  } finally {
    injectedAdapter.cleanup();
  }
  assert.equal(result.status, FAIL, `注入后判据必须红，实际：${result.status} ${result.detail}`);
  assert.match(
    result.detail,
    /【核算时机·清理后】/,
    '判据必须点名「核算读的是清理前的旧数」',
  );
  // **基线里没有这条红因** ⇒ 注入制造的是**新的**、可点名的红因。
  assert.doesNotMatch(
    baseResult.detail,
    /【核算时机·清理后】/,
    `基线不该有这条红因（注入没有制造出新东西）：${baseResult.detail}`,
  );

  // **原实现一个字节都没碰**。
  assert.equal(sha256File(original), before, '原实现文件被改了 —— 注入只能打在副本上');
});

// ---------------------------------------------------------------------------
// 真机冒烟（十四条在真实实现上都跑得起来）
// ---------------------------------------------------------------------------

test('十四条判据在真实实现上都跑得起来（不是「未执行」、也不是「判据自身抛错」）', async () => {
  for (const spec of CRITERIA) {
    const adapter = await createP3Adapter({ tag: `p3-selftest-${spec.id}` });
    let result;
    try {
      result = await spec.run(adapter);
    } finally {
      adapter.cleanup();
    }
    assert.notEqual(
      result.status,
      SKIPPED,
      `${spec.id} 不该是「未执行」：${result.detail}`,
    );
    assert.doesNotMatch(result.detail, /判据自身/, `${spec.id} 判据自身抛错：${result.detail}`);
  }
});
