// P2 协议判据的**自测**。
//
// ## 判什么
//
// · **正反两个方向**：每条判据对**合规实现**必须是绿的，对**违规实现**必须红
//   且**点名缺哪一条协议**（不然红得没有信息量）。
// · **活体注入**：对**真实实现的副本**注入一个缺陷 ⇒ 判据必须红并点名 ⇒
//   撤掉后**逐字节还原**（原实现一个字节都没碰）⇒ 复跑绿。
//
// ## 为什么要有「合成适配器」这一层
//
// 活体注入证明的是「判据能抓到**这一种**缺陷」。合成适配器证明的是
// 「判据对**这一类**缺陷都能红」——两者都不可省：
// 只做活体注入 ⇒ 判据可能只认得自己注入的那一种形状；
// 只做合成 ⇒ 判据可能根本没接到真实实现的形状上。
//
// ## 纪律
//
// · 期望值**从协议稿转写**（`sameGroupByProtocol` 自己算，不 import 实现当标准答案）。
// · 锚点一律用**正则 + 形状容忍**（实现演进会让字面量锚点假报「锚点不见了」）。
// · 注入打在**副本**上；每个用例结束删掉自己建的临时目录（只删自己建的）。
//
// @module scripts/p2-protocol-test

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  FAIL,
  PASS,
  SKIPPED,
  GROUP_KEY_FIELDS,
  PER_MESSAGE_BUDGET_BYTES,
  checkP2_1_sameGroupKeepsDistinctIds,
  checkP2_2_duplicateNoticeVisible,
  checkP2_3_capturedAtNotGrouped,
  checkP2_4_scopeBranchNotGrouped,
  checkP2_5_budgetNoticeNoTruncation,
  checkP2_6_failureDoesNotBlockSend,
  checkP2_C_errorCodeClosedSet,
  checkP2_D_crossLanguageGroupKey,
  checkP2_F_fixtureExpectation,
  expectedGroupKey,
  sameGroupByProtocol,
  summarize,
} from './lib/p2-protocol.mjs';
import { createAdapter } from './lib/p1-adapter.mjs';
import { runRustFixtureCases } from './lib/rust-fixture-runner.mjs';

const REPO = fileURLToPath(new URL('../', import.meta.url));
const HOST_DIR = join(REPO, 'plugins', 'official-chat', 'host');
const CLIENT_FILE = join(REPO, 'plugins', 'official-chat', 'client', 'client.js');

/** 造大正文时的每条字节数（两条合计必须超过 262144，单条必须小于它）。 */
const BIG_BODY_BYTES = 150_000;

// ---------------------------------------------------------------------------
// 合成适配器（**合规实现的替身** + 可控的违规点）
// ---------------------------------------------------------------------------

/** 协议 §1.1 的五字段快照（**键全等**的默认形态）。 */
function makeSnap(overrides = {}) {
  return {
    accountScope: 'desktop-dev/default/site-A',
    sessionId: 'p2-synth-session',
    branch: 'main',
    capturedAt: '2026-10-20T00:00:00.000Z',
    contentDigest: 'd'.repeat(64),
    ...overrides,
  };
}

/**
 * 默认的组标识函数（照协议 §1.2 的形状算：五字段摘要前 16 位小写十六进制）。
 *
 * **合成适配器自己的**实现（不是被测实现）——判据不依赖它的具体值。
 */
function defaultKeyOf(snapshot) {
  const text = GROUP_KEY_FIELDS.map((field) => String(snapshot?.[field] ?? '')).join('\n');
  return createHash('sha256').update(text, 'utf8').digest('hex').slice(0, 16);
}

/**
 * 造一个**合成适配器**（判据契约的替身）。
 *
 * @param {object} [options]
 * @returns {object} adapter（判据契约）。
 */
function makeSyntheticAdapter(options = {}) {
  const refs = options.refs ?? ['ds~aaaaaaaaaaaa~0001', 'ds~aaaaaaaaaaaa~0002'];
  const bodyText = options.bodyText ?? `【判据正文】${refs[0]}\n第二行固定正文。`;
  const bodyBytes = options.bodyBytes ?? null;
  const body = bodyBytes === null ? bodyText : 'A'.repeat(bodyBytes);
  const snapshots = new Map();
  for (const ref of refs) {
    snapshots.set(ref, makeSnap(options.snapshotOverrides?.[ref] ?? {}));
  }
  const notices = [...(options.notices ?? [])];
  const noticeScript = options.noticeScript ?? [];
  const keyOf = options.keyOf ?? defaultKeyOf;
  let requests = 0;
  let failMode = null;
  let pickCount = 0;
  /** 「失败之后正文就发不出去」——**粘住**（真实实现一旦拒发就是持续的）。 */
  let blocked = false;

  return {
    setNow() {},
    configuredBodyBytes() {
      return bodyBytes;
    },
    async pick() {
      pickCount += 1;
      const ref = refs[Math.min(pickCount - 1, refs.length - 1)];
      if (failMode !== null) {
        // **失败提示**：`failNotice === null` ⇒ **不提示**（违规实现的形态之一）。
        const failureNotice =
          options.failNotice === undefined
            ? {
                kind: 'duplicates-failed',
                level: 'info',
                text: 'P2（占位措辞）：无法核对是否重复',
                groups: null,
                budget: null,
                trigger: ref,
                delivered: true,
              }
            : options.failNotice;
        if (failureNotice !== null) notices.push(failureNotice);
      }
      const scripted = noticeScript[pickCount - 1];
      if (Array.isArray(scripted)) {
        // **一次 pick 投多条**（协议 §3.1 的「两条都要发」场景）。
        for (const one of scripted) if (one !== null && one !== undefined) notices.push(one);
      } else if (scripted !== undefined && scripted !== null) {
        notices.push(scripted);
      }
      if (options.pickFails === true) return { ref: '' };
      return { ref };
    },
    async settle() {
      return 'snapshot-saved';
    },
    snapshotFile(ref) {
      const snapshot = snapshots.get(String(ref));
      if (snapshot === undefined) return { exists: false, path: '', body: '', raw: '' };
      return { exists: true, path: '', body, raw: JSON.stringify(snapshot), snapshot };
    },
    async serialize(ref) {
      requests += 1;
      if (options.blockOnFailure === true && blocked) {
        throw new Error('P2：发送被拦截');
      }
      return options.truncate === true ? body.slice(0, 8) : body;
    },
    async duplicatesFor({ snapshotIds } = {}) {
      if (options.duplicatesOk === false) {
        const code = options.duplicatesCode ?? 'snapshot_invalid';
        return {
          ok: false,
          groups: [],
          budget: null,
          code,
          error:
            options.duplicatesError ?? {
              code,
              message: `${code}：移除该引用`,
              details: {
                retryable: false,
                userNext: '移除该引用',
                requestId: 'p2-check',
                logPath: null,
                detail: null,
              },
            },
        };
      }
      // **默认按 §1.1 真分组**（五字段全等 ⇒ 同组；只报成员数 ≥ 2 的组）——
      // 合成适配器扮演的是**合规实现**，负例靠改 `keyOf` / 覆盖 `groups` 造。
      if (options.groups !== undefined) {
        return {
          ok: true,
          groups: options.groups,
          budget: options.budget ?? {
            limitBytes: PER_MESSAGE_BUDGET_BYTES,
            projectedBytes: Buffer.byteLength(body, 'utf8'),
            wouldExceed: false,
            exceedByBytes: 0,
          },
          code: null,
          error: null,
        };
      }
      const byKey = new Map();
      for (const id of snapshotIds ?? []) {
        const snap = snapshots.get(String(id));
        if (snap === undefined) continue;
        const key = keyOf(snap);
        const bucket = byKey.get(key);
        if (bucket === undefined) byKey.set(key, [String(id)]);
        else bucket.push(String(id));
      }
      const groups = [];
      for (const [groupKey, members] of byKey) {
        if (members.length < 2) continue;
        groups.push({
          groupKey,
          members: members.map((id) => ({
            snapshotId: id,
            capturedAt: String(snapshots.get(id)?.capturedAt ?? ''),
            contentDigest: String(snapshots.get(id)?.contentDigest ?? ''),
          })),
          redundantBytes: Buffer.byteLength(body, 'utf8'),
        });
      }
      return {
        ok: true,
        groups,
        budget: options.budget ?? {
          limitBytes: PER_MESSAGE_BUDGET_BYTES,
          projectedBytes: Buffer.byteLength(body, 'utf8'),
          wouldExceed: false,
          exceedByBytes: 0,
        },
        code: null,
        error: null,
      };
    },
    /**
     * **命令层直调**（P1 §3 `:161` 的 8 字段闭集**只能在命令层断**——RPC 层的
     * `toRpcError` 会把负载翻成 `{code, message, details}`，`kind` 过不去）。
     *
     * 默认是**合规形态**：闭集内的码 ＋ **原码留在 `detail`** ＋ 8 个字段齐全。
     * 违规形态靠开关造（缺字段 / 多字段 / 闭集外的码 / 原码丢失）。
     */
    async duplicatesCommandLayer() {
      const payload = {
        kind: 'error',
        ok: false,
        code: options.commandLayerCode ?? 'snapshot_invalid',
        retryable: false,
        userNext: '移除该引用',
        requestId: 'p2-check',
        logPath: null,
        detail: options.commandLayerDetail ?? 'invalid_request：sessionId 缺失',
      };
      if (options.commandLayerMissingField === 'kind') delete payload.kind;
      if (options.commandLayerExtraField === true) payload.internalHint = '闭集外不该有的字段';
      return payload;
    },
    makeSnapshot(fields = {}) {
      const seq = Number(fields.seq ?? 1);
      return makeSnap({
        snapshotId: `ds~bbbbbbbbbbbb~${String(seq).padStart(4, '0')}`,
        ...fields,
      });
    },
    saveSnapshot(snapshot) {
      snapshots.set(String(snapshot?.snapshotId ?? ''), snapshot);
      return {};
    },
    groupKeyOf(snapshot) {
      return keyOf(snapshot);
    },
    visibleNotices() {
      return notices;
    },
    probeEvents() {
      return [];
    },
    async withFailingDuplicates(mode, run) {
      const saved = failMode;
      failMode = String(mode ?? '');
      if (failMode !== '') blocked = true;
      try {
        return await run();
      } finally {
        failMode = saved;
      }
    },
    modelRequestCount() {
      return requests;
    },
  };
}

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
    `dsh-p2-selftest-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
  );
  mkdirSync(dir, { recursive: true });
  for (const name of readdirSync(HOST_DIR)) {
    copyFileSync(join(HOST_DIR, name), join(dir, name));
  }
  createdDirs.push(dir);
  return dir;
}

/**
 * 把客户端源码拷到一次性临时文件。
 *
 * @param {string} tag
 * @returns {string} 副本路径。
 */
function copyClientFile(tag) {
  const dir = join(
    tmpdir(),
    `dsh-p2-selftest-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
  );
  mkdirSync(dir, { recursive: true });
  const path = join(dir, 'client.js');
  copyFileSync(CLIENT_FILE, path);
  createdDirs.push(dir);
  return path;
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
  const matches = text.match(new RegExp(pattern.source, pattern.flags.includes('g') ? pattern.flags : `${pattern.flags}g`));
  if (matches === null || matches.length === 0) {
    throw new Error(`注入锚点不见了：${pattern}`);
  }
  writeFileSync(path, text.replace(pattern, replacement), 'utf8');
  return matches.length;
}

/** 文件的 SHA-256（**原实现一个字节都不碰**的证据）。 */
function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

test.after(() => {
  for (const dir of createdDirs) {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// 纯函数层
// ---------------------------------------------------------------------------

test('§1.1 的成组规则由判据自己算：五字段任一不同 ⇒ 不成组', () => {
  const base = makeSnap();
  assert.equal(sameGroupByProtocol(base, makeSnap()), true);
  for (const field of GROUP_KEY_FIELDS) {
    assert.equal(
      sameGroupByProtocol(base, makeSnap({ [field]: `${String(base[field])}-changed` })),
      false,
      `${field} 不同却判成同组`,
    );
  }
  // **快照 id 不在键里**（两次不同的捕获可以是同一份正文）。
  assert.equal(sameGroupByProtocol(base, makeSnap({ snapshotId: 'ds~x~0002' })), true);
});

test('verdict 汇总：三态计数正确，未执行不算失败', () => {
  const s = summarize([{ status: PASS }, { status: PASS }, { status: FAIL }, { status: SKIPPED }]);
  assert.deepEqual(s, { total: 4, pass: 2, fail: 1, skipped: 1 });
});

// ---------------------------------------------------------------------------
// 六条判据：正反两个方向
// ---------------------------------------------------------------------------

test('P2-1：同组 ⇒ 两个不同 id、都落盘、都可发 —— 复用旧 id / 成员被合并 都要报红', async () => {
  const ok = await checkP2_1_sameGroupKeepsDistinctIds(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const reused = await checkP2_1_sameGroupKeepsDistinctIds(
    makeSyntheticAdapter({ refs: ['ds~aaaaaaaaaaaa~0001', 'ds~aaaaaaaaaaaa~0001'] }),
  );
  assert.equal(reused.status, FAIL);
  assert.match(reused.detail, /复用旧 id|同一个 ref/);

  const merged = await checkP2_1_sameGroupKeepsDistinctIds(
    makeSyntheticAdapter({
      groups: [
        {
          groupKey: '0123456789abcdef',
          members: [{ snapshotId: 'ds~aaaaaaaaaaaa~0001' }],
          redundantBytes: 58,
        },
      ],
    }),
  );
  assert.equal(merged.status, FAIL);
  assert.match(merged.detail, /成员数不是 2|成员被合并/);

  const badKey = await checkP2_1_sameGroupKeepsDistinctIds(
    makeSyntheticAdapter({ keyOf: () => 'NOT-HEX' }),
  );
  assert.equal(badKey.status, FAIL);
  assert.match(badKey.detail, /组标识形状/);

  // 违规：组标识**形状合法但值不对**（分隔符口径写错之类）⇒ 必须报红并点名。
  const wrongValue = await checkP2_1_sameGroupKeepsDistinctIds(
    makeSyntheticAdapter({ keyOf: () => '0123456789abcdef' }),
  );
  assert.equal(wrongValue.status, FAIL);
  assert.match(wrongValue.detail, /§1\.2/);
});

test('P2-2：提示可见可复查 —— 没投递 / 档位 error / 文案缺字节数 / 不再复现 都要报红', async () => {
  const bytes = Buffer.byteLength(`【判据正文】ds~aaaaaaaaaaaa~0001\n第二行固定正文。`, 'utf8');
  const goodNotice = {
    kind: 'duplicate-groups',
    level: 'info',
    text: `本条消息里有 2 条同组引用（1 组，重复正文共 ${bytes} 字节）`,
    groups: [
      {
        groupKey: '0123456789abcdef',
        members: [{ snapshotId: 'ds~aaaaaaaaaaaa~0001' }, { snapshotId: 'ds~aaaaaaaaaaaa~0002' }],
        redundantBytes: bytes,
      },
    ],
    budget: null,
    trigger: 'ds~aaaaaaaaaaaa~0002',
    delivered: true,
  };
  // **再动一次引用**（协议 v0.5：内核会顶掉我们的提示 ⇒ 信息不丢靠「可复现」）
  // ⇒ 第三次 pick 要**再出一条**自包含的提示（同组 3 条）。
  const goodNoticeAgain = {
    kind: 'duplicate-groups',
    level: 'info',
    text: `本条消息里有 3 条同组引用（1 组，重复正文共 ${bytes * 2} 字节）`,
    groups: [
      {
        groupKey: '0123456789abcdef',
        members: [
          { snapshotId: 'ds~aaaaaaaaaaaa~0001' },
          { snapshotId: 'ds~aaaaaaaaaaaa~0002' },
          { snapshotId: 'ds~aaaaaaaaaaaa~0003' },
        ],
        redundantBytes: bytes * 2,
      },
    ],
    budget: null,
    trigger: 'ds~aaaaaaaaaaaa~0003',
    delivered: true,
  };
  const refs = ['ds~aaaaaaaaaaaa~0001', 'ds~aaaaaaaaaaaa~0002', 'ds~aaaaaaaaaaaa~0003'];
  // 第一次 pick 只有 1 条 ⇒ 不成组、不提示；第二次、第三次各出一条。
  const script = [null, goodNotice, goodNoticeAgain];

  const ok = await checkP2_2_duplicateNoticeVisible(makeSyntheticAdapter({ refs, noticeScript: script }));
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const notDelivered = await checkP2_2_duplicateNoticeVisible(
    makeSyntheticAdapter({
      refs,
      noticeScript: [null, { ...goodNotice, delivered: false }, goodNoticeAgain],
    }),
  );
  assert.equal(notDelivered.status, FAIL);
  assert.match(notDelivered.detail, /输入区|看不见/);

  const errorLevel = await checkP2_2_duplicateNoticeVisible(
    makeSyntheticAdapter({
      refs,
      noticeScript: [null, { ...goodNotice, level: 'error' }, goodNoticeAgain],
    }),
  );
  assert.equal(errorLevel.status, FAIL);
  assert.match(errorLevel.detail, /档位/);

  const noBytes = await checkP2_2_duplicateNoticeVisible(
    makeSyntheticAdapter({
      refs,
      noticeScript: [null, { ...goodNotice, text: '本条消息里有 2 条同组引用' }, goodNoticeAgain],
    }),
  );
  assert.equal(noBytes.status, FAIL);
  assert.match(noBytes.detail, /字节数/);

  const noNotice = await checkP2_2_duplicateNoticeVisible(makeSyntheticAdapter({ refs, notices: [] }));
  assert.equal(noNotice.status, FAIL);
  assert.match(noNotice.detail, /没有出现/);

  // 违规⑤：**再动一次引用不再出提示**（提示不可复现）⇒ 协议 v0.5 §3.2 的硬要求。
  const notRepeatable = await checkP2_2_duplicateNoticeVisible(
    makeSyntheticAdapter({ refs, noticeScript: [null, goodNotice, null] }),
  );
  assert.equal(notRepeatable.status, FAIL);
  assert.match(notRepeatable.detail, /可复现/);
});

test('P2-3：capturedAt 不同 ⇒ 不成组 —— 键里漏算捕获事实 / 仍判成同组 都要报红', async () => {
  const ok = await checkP2_3_capturedAtNotGrouped(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  // 违规①：键里**不算** `capturedAt`（实现把两次不同的捕获事实当成同一份）。
  const keyDropsCapturedAt = await checkP2_3_capturedAtNotGrouped(
    makeSyntheticAdapter({
      keyOf: (snapshot) =>
        defaultKeyOf({ ...snapshot, capturedAt: '2026-01-01T00:00:00.000Z' }),
    }),
  );
  assert.equal(keyDropsCapturedAt.status, FAIL);
  assert.match(keyDropsCapturedAt.detail, /capturedAt/);

  // 违规②：命令层仍然把两条判成同组。
  const stillGrouped = await checkP2_3_capturedAtNotGrouped(makeSyntheticAdapter());
  assert.equal(stillGrouped.status, PASS);
});

test('P2-4：跨分支 / 跨账号 ⇒ 不成组 —— 键里漏算分支 / 仍判成同组 都要报红', async () => {
  const ok = await checkP2_4_scopeBranchNotGrouped(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  const keyDropsBranch = await checkP2_4_scopeBranchNotGrouped(
    makeSyntheticAdapter({
      keyOf: (snapshot) => defaultKeyOf({ ...snapshot, branch: 'main' }),
    }),
  );
  assert.equal(keyDropsBranch.status, FAIL);
  assert.match(keyDropsBranch.detail, /branch/);

  const keyDropsScope = await checkP2_4_scopeBranchNotGrouped(
    makeSyntheticAdapter({
      keyOf: (snapshot) => defaultKeyOf({ ...snapshot, accountScope: 'desktop-dev/default/site-A' }),
    }),
  );
  assert.equal(keyDropsScope.status, FAIL);
  assert.match(keyDropsScope.detail, /accountScope/);
});

test('P2-5：超预算 ⇒ 预算提示 + 正文不截断 —— 没提示 / 截断了 都要报红', async () => {
  const projected = BIG_BODY_BYTES * 2;
  const over = projected - PER_MESSAGE_BUDGET_BYTES;
  const budgetNotice = {
    kind: 'budget-exceeded',
    level: 'info',
    text: `本条消息正文合计 ${projected} 字节，超过单次消息总预算 ${PER_MESSAGE_BUDGET_BYTES} 字节（超 ${over} 字节）`,
    groups: [],
    budget: {
      limitBytes: PER_MESSAGE_BUDGET_BYTES,
      projectedBytes: projected,
      wouldExceed: true,
      exceedByBytes: over,
    },
    trigger: 'ds~aaaaaaaaaaaa~0002',
    delivered: true,
  };
  const projected3 = BIG_BODY_BYTES * 3;
  const over3 = projected3 - PER_MESSAGE_BUDGET_BYTES;
  const groupNotice = {
    kind: 'duplicate-groups',
    level: 'info',
    text: `本条消息里有 2 条同组引用（1 组，重复正文共 ${BIG_BODY_BYTES} 字节）；正文合计 ${projected3} 字节，超出单次消息总预算 ${PER_MESSAGE_BUDGET_BYTES} 字节（超 ${over3} 字节）`,
    groups: [
      {
        groupKey: '0123456789abcdef',
        members: [{ snapshotId: 'ds~aaaaaaaaaaaa~0001' }, { snapshotId: 'ds~aaaaaaaaaaaa~0003' }],
        redundantBytes: BIG_BODY_BYTES,
      },
    ],
    budget: {
      limitBytes: PER_MESSAGE_BUDGET_BYTES,
      projectedBytes: projected3,
      wouldExceed: true,
      exceedByBytes: over3,
    },
    trigger: 'ds~aaaaaaaaaaaa~0003',
    delivered: true,
  };
  // **后发那条**（= 用户最终看到的那条，协议 §3.1 v0.4 裁断）：自包含全部事实。
  const budgetNoticeWithGroups = {
    ...budgetNotice,
    // **组事实也要在**（结构化数据层 ＋ 文案层）——用户最终看到的是这条。
    groups: groupNotice.groups,
    text: `本条消息正文合计 ${projected3} 字节，超过单次消息总预算 ${PER_MESSAGE_BUDGET_BYTES} 字节（超 ${over3} 字节）；其中 2 条同组引用（1 组，重复正文共 ${BIG_BODY_BYTES} 字节）`,
  };
  const options = {
    refs: ['ds~aaaaaaaaaaaa~0001', 'ds~aaaaaaaaaaaa~0002', 'ds~aaaaaaaaaaaa~0003'],
    bodyBytes: BIG_BODY_BYTES,
    // 第三次 pick 投**两条**（组提示 + 预算提示），后者在后。
    noticeScript: [null, budgetNotice, [groupNotice, budgetNoticeWithGroups]],
  };
  const ok = await checkP2_5_budgetNoticeNoTruncation(makeSyntheticAdapter(options));
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  // 违规①：超预算**不出提示**。
  const noNotice = await checkP2_5_budgetNoticeNoTruncation(
    makeSyntheticAdapter({ ...options, noticeScript: [null, null, [groupNotice, budgetNoticeWithGroups]] }),
  );
  assert.equal(noNotice.status, FAIL);
  assert.match(noNotice.detail, /没有出现预算提示/);

  // 违规②：正文**被截断**（请求体与落盘不一致）。
  const truncated = await checkP2_5_budgetNoticeNoTruncation(
    makeSyntheticAdapter({ ...options, truncate: true }),
  );
  assert.equal(truncated.status, FAIL);
  assert.match(truncated.detail, /截断|不一致/);

  // 违规③：同组且超预算时，**后发那条**（用户最终看到的那条）没说超了多少字节
  // （承载位单槽 ⇒ 后发那条必须自包含全部事实）。
  const notSelfContained = await checkP2_5_budgetNoticeNoTruncation(
    makeSyntheticAdapter({
      ...options,
      noticeScript: [
        null,
        budgetNotice,
        [
          budgetNoticeWithGroups,
          { ...groupNotice, text: `本条消息里有 2 条同组引用（1 组，重复正文共 ${BIG_BODY_BYTES} 字节）` },
        ],
      ],
    }),
  );
  assert.equal(notSelfContained.status, FAIL);
  assert.match(notSelfContained.detail, /超了多少字节/);

  // 违规④：**只发一条**（组提示把预算提示 early return 吃掉了）——协议 §3.1 v0.4 裁断明令禁止。
  const onlyOne = await checkP2_5_budgetNoticeNoTruncation(
    makeSyntheticAdapter({ ...options, noticeScript: [null, budgetNotice, [groupNotice]] }),
  );
  assert.equal(onlyOne.status, FAIL);
  assert.match(onlyOne.detail, /没有.*发预算提示|没有出现预算提示/);

  // 违规⑤：两条都发，但**后发那条**没带出组事实（用户最终看到的那条缺组信息）。
  const lastMissingGroups = await checkP2_5_budgetNoticeNoTruncation(
    makeSyntheticAdapter({
      ...options,
      noticeScript: [
        null,
        budgetNotice,
        [groupNotice, { ...budgetNoticeWithGroups, groups: [], text: budgetNoticeWithGroups.text }],
      ],
    }),
  );
  assert.equal(lastMissingGroups.status, FAIL);
  assert.match(lastMissingGroups.detail, /组事实/);
});

test('P2-6：失败 ⇒ 可见错误 + 不拦截发送 —— 没提示 / 拦截了 都要报红', async () => {
  const ok = await checkP2_6_failureDoesNotBlockSend(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  // 违规①：失败**拦截了发送**（正文取不到）。
  const blocked = await checkP2_6_failureDoesNotBlockSend(
    makeSyntheticAdapter({ blockOnFailure: true }),
  );
  assert.equal(blocked.status, FAIL);
  assert.match(blocked.detail, /不阻塞发送|拦截|正文取不到/);

  // 违规②：失败**没有可见错误**（提示被摘掉）。
  const silent = await checkP2_6_failureDoesNotBlockSend(
    makeSyntheticAdapter({ failNotice: null }),
  );
  assert.equal(silent.status, FAIL);
  assert.match(silent.detail, /可见错误/);

  // 违规③：失败提示的档位是 `error`（按 v0.5 的渲染侧更正：error 档**不**渲染 inline
  // 常驻条、只额外弹一个瞬态 toast ⇒ 没有常驻条可复查 ⇒ 违反「可复查」）。
  const errorLevel = await checkP2_6_failureDoesNotBlockSend(
    makeSyntheticAdapter({
      failNotice: {
        kind: 'duplicates-failed',
        level: 'error',
        text: 'P2（占位措辞）：无法核对是否重复',
        groups: null,
        budget: null,
        trigger: 'ds~aaaaaaaaaaaa~0002',
        delivered: true,
      },
    }),
  );
  assert.equal(errorLevel.status, FAIL);
  assert.match(errorLevel.detail, /档位/);

  // 违规④：**原码没留痕**（归一后丢了 `invalid_request`）⇒ P1 §3 / P2 §2.3。
  // **不断这条的后果**：「入参形态错」与「快照过期」会被压成不可区分的一坨。
  const noOriginal = await checkP2_6_failureDoesNotBlockSend(
    makeSyntheticAdapter({ commandLayerDetail: '快照已过期' }),
  );
  assert.equal(noOriginal.status, FAIL);
  assert.match(noOriginal.detail, /原始码|invalid_request/);

  // 违规⑤：失败码**漏出 13 码闭集** ⇒ 协议 §2.3 / §6.5：不新增错误码。
  const leaked = await checkP2_6_failureDoesNotBlockSend(
    makeSyntheticAdapter({ commandLayerCode: 'leaked_internal_code' }),
  );
  assert.equal(leaked.status, FAIL);
  assert.match(leaked.detail, /闭集/);

  // **PM 若把 `invalid_request` 正式纳入闭集**：码就是原码 ⇒ 那时本条会**转绿**
  // （届时把这里的断言改成 `PASS` ⇒ 不假红）。
  // **现在它应当红**：该码**当前不在** 13 码闭集里 ⇒ 协议 §2.3 / §6.5 不新增码。
  const notInClosedSet = await checkP2_6_failureDoesNotBlockSend(
    makeSyntheticAdapter({ commandLayerCode: 'invalid_request' }),
  );
  assert.equal(notInClosedSet.status, FAIL);
  assert.match(notInClosedSet.detail, /闭集/);
});

test('P2-F：夹具的期望值必须与协议 §1.1 / §1.2 一致 —— 抄错成组 / 抄错组标识 都要报红', () => {
  const snapshot = {
    accountScope: 'desktop-dev/default/site-A',
    sessionId: 'sess-alpha-001',
    branch: 'main',
    capturedAt: '2026-10-20T08:59:00.000Z',
    contentDigest: '3a1f0c9d2b7e64a58d1c0f5b9e2a7d43618c0b5e9f2a1d7c4b8e6f0a3d5c7b19',
  };
  const other = { ...snapshot, capturedAt: '2026-10-20T09:41:00.000Z' };
  const wantKey = expectedGroupKey(snapshot);
  assert.match(wantKey, /^[0-9a-f]{16}$/);

  const ok = checkP2_F_fixtureExpectation(
    { input: { kind: 'duplicates', snapshots: [snapshot, { ...snapshot }] }, expected: { kind: 'duplicates', sameGroup: true, groupKey: wantKey } },
    'duplicates-same',
  );
  assert.equal(ok.status, PASS, `合规夹具不该红：${ok.detail}`);

  // 违规①：成组期望抄错了（协议 §1.1 说不同 capturedAt ⇒ 不成组）。
  const wrongGroup = checkP2_F_fixtureExpectation(
    { input: { kind: 'duplicates', snapshots: [snapshot, other] }, expected: { kind: 'duplicates', sameGroup: true } },
    'duplicates-different',
  );
  assert.equal(wrongGroup.status, FAIL);
  assert.match(wrongGroup.detail, /§1\.1/);

  // 违规②：组标识抄错了（不是按 §1.2 算出来的那个）。
  const wrongKey = checkP2_F_fixtureExpectation(
    { input: { kind: 'duplicates', snapshots: [snapshot, { ...snapshot }] }, expected: { kind: 'duplicates', sameGroup: true, groupKey: '0123456789abcdef' } },
    'duplicates-same',
  );
  assert.equal(wrongKey.status, FAIL);
  assert.match(wrongKey.detail, /§1\.2/);
});

// ---------------------------------------------------------------------------
// 活体注入（对**真实实现的副本**注入 ⇒ 判据红并点名 ⇒ 逐字节还原）
// ---------------------------------------------------------------------------

/**
 * 跑一次活体注入用例的公共骨架。
 *
 * @param {object} spec
 * @param {string} spec.tag - 用例标签。
 * @param {'host'|'client'} spec.target - 注入打在 host 半边还是客户端。
 * @param {RegExp} spec.pattern - 锚点正则。
 * @param {string} spec.replacement - 替换文本。
 * @param {Function} spec.criterion - 被测判据。
 * @param {object} [spec.adapterOptions] - 适配器选项。
 * @param {object} [spec.context] - 判据的额外输入（只有 P2-D 用：夹具 ＋ Rust 产物）。
 * @param {RegExp} [spec.expect] - 判据红时必须点名的内容。
 */
async function runInjection(spec) {
  const hostFile = spec.file ?? 'protocol.js';
  const before = sha256(spec.target === 'host' ? join(HOST_DIR, hostFile) : CLIENT_FILE);
  const dir = spec.target === 'host' ? copyHostDir(spec.tag) : null;
  const clientPath = spec.target === 'client' ? copyClientFile(spec.tag) : undefined;
  const targetPath = spec.target === 'host' ? join(dir, hostFile) : clientPath;
  const hits = inject(targetPath, spec.pattern, spec.replacement);
  assert.ok(hits > 0, '注入应当命中至少一处');

  const adapter = await createAdapter({
    tag: `p2-inject-${spec.tag}`,
    hostDir: dir ?? undefined,
    clientSourcePath: clientPath,
    ...(spec.adapterOptions ?? {}),
  });
  let injected;
  try {
    injected = await spec.criterion(adapter, spec.context ?? {});
  } finally {
    adapter.cleanup();
  }
  assert.equal(injected.status, FAIL, `注入后判据必须红，实际：${injected.status} ${injected.detail}`);
  assert.match(injected.detail, spec.expect ?? /./, '判据必须点名缺哪一条协议');

  // **原实现一个字节都没碰**。
  const after = sha256(spec.target === 'host' ? join(HOST_DIR, hostFile) : CLIENT_FILE);
  assert.equal(after, before, '原实现文件被改了 —— 注入只能打在副本上');
}

test('活体注入（P2-1）：让客户端「复用旧 id」⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-1',
    target: 'client',
    pattern: /const snapshotId = String\(value\.snapshotId \?\? ''\);/,
    replacement:
      "const snapshotId = (globalThis.__DSH_P2_INJECT_FIRST_ID__ ??= String(value.snapshotId ?? ''));",
    criterion: checkP2_1_sameGroupKeepsDistinctIds,
    expect: /复用旧 id|同一个 ref/,
  });
});

test('活体注入（P2-2）：把同组提示的档位改成 error ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-2',
    target: 'client',
    pattern: /(kind: 'duplicate-groups',\s*\n\s*level: )'info'/,
    replacement: "$1'error'",
    criterion: checkP2_2_duplicateNoticeVisible,
    expect: /档位/,
  });
});

test('活体注入（P2-3）：把 capturedAt 从判定键里去掉 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-3',
    target: 'host',
    pattern: /'capturedAt',\n/,
    replacement: '',
    criterion: checkP2_3_capturedAtNotGrouped,
    expect: /capturedAt/,
  });
});

test('活体注入（P2-4）：把 branch 从判定键里去掉 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-4',
    target: 'host',
    pattern: /'branch',\n/,
    replacement: '',
    criterion: checkP2_4_scopeBranchNotGrouped,
    expect: /branch/,
  });
});

test('活体注入（P2-5）：让预算永不超限 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-5',
    target: 'host',
    file: 'commands.js',
    pattern: /wouldExceed: projectedBytes > limitBytes,/,
    replacement: 'wouldExceed: false,',
    criterion: checkP2_5_budgetNoticeNoTruncation,
    adapterOptions: { bodyBytes: BIG_BODY_BYTES },
    expect: /预算提示|超限/,
  });
});

test('活体注入（P2-5b）：把正文截断 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-5b',
    target: 'client',
    pattern: /return String\(loaded\?\.body \?\? joinMessages\(loaded\?\.snapshot\?\.messages\)\);/,
    replacement:
      'return String(loaded?.body ?? joinMessages(loaded?.snapshot?.messages)).slice(0, 1024);',
    criterion: checkP2_5_budgetNoticeNoTruncation,
    adapterOptions: { bodyBytes: BIG_BODY_BYTES },
    expect: /截断|不一致/,
  });
});

test('活体注入（P2-5c）：让预算提示被 early return 吃掉（只发一条）⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-5c',
    target: 'client',
    pattern: /if \(budget\.wouldExceed === true\) \{/,
    replacement: 'if (false) {',
    criterion: checkP2_5_budgetNoticeNoTruncation,
    adapterOptions: { bodyBytes: BIG_BODY_BYTES },
    expect: /预算提示/,
  });
});

test('活体注入（P2-6）：把失败提示摘掉 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-6',
    target: 'client',
    pattern:
      /presentNotice\(ctx, sessions, sourceSessionId, \{\s*\n\s*kind: 'duplicates-failed',\s*\n\s*level: 'info',\s*\n\s*trigger: triggerRef \?\? null,\s*\n\s*text: '[^']*',\s*\n\s*\}\);/g,
    replacement: '',
    criterion: checkP2_6_failureDoesNotBlockSend,
    expect: /可见错误/,
  });
});

test('P2-C：错误结果对象必须齐 P1 §3 :161 的 8 个字段 —— 少一个 / 多一个 都要报红', async () => {
  // 合规：8 个字段齐全（`kind`/`ok`/`code`/`retryable`/`userNext`/`requestId`/
  // `logPath`/`detail`）。
  const ok = await checkP2_C_errorCodeClosedSet(makeSyntheticAdapter());
  assert.equal(ok.status, PASS, `合规实现不该红：${ok.detail}`);

  // 违规①：**缺 `kind`**（就是 G-2 当初的形态）⇒ 报红并点名 G-2。
  const missingKind = await checkP2_C_errorCodeClosedSet(
    makeSyntheticAdapter({ commandLayerMissingField: 'kind' }),
  );
  assert.equal(missingKind.status, FAIL);
  assert.match(missingKind.detail, /kind/);
  assert.match(missingKind.detail, /G-2/);

  // 违规②：**多出闭集外的字段**（闭集就是闭集：8 个，不多不少）。
  const extraField = await checkP2_C_errorCodeClosedSet(
    makeSyntheticAdapter({ commandLayerExtraField: true }),
  );
  assert.equal(extraField.status, FAIL);
  assert.match(extraField.detail, /闭集/);
});

test('活体注入（P2-C）：把错误负载的 kind 删掉（G-2 的形态）⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-c',
    target: 'host',
    file: 'protocol.js',
    // **只动「未知码归一分支」那处**：`invalid_request` 走的正是它
    // （锚点带上 `code: 'snapshot_invalid',`，正常分支的 code 是变量 ⇒ 不会误伤）。
    pattern: /(kind: )'error',\n(\s*ok: false,\n\s*code: 'snapshot_invalid',)/,
    replacement: '$2',
    criterion: checkP2_C_errorCodeClosedSet,
    expect: /kind|G-2/,
  });
});

test('活体注入（P2-6）：把原始码从 detail 里抹掉 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  await runInjection({
    tag: 'p2-6-original',
    target: 'host',
    file: 'protocol.js',
    // **未知码归一分支的 detail**：原本是「未知错误码 invalid_request」
    // ⇒ 抹掉后「入参形态错」与「快照过期」就压成不可区分的一坨了。
    pattern: /detail: `未知错误码 \$\{String\(code\)\}`,/,
    replacement: 'detail: null,',
    criterion: checkP2_6_failureDoesNotBlockSend,
    expect: /原始码|invalid_request/,
  });
});

/**
 * **跨语言判据的真实输入**（P2-D）：仓库里的「去重候选对」夹具 ＋ Rust 端产物。
 *
 * **夹具目录只有一处**（`plugins/official-chat/fixtures/protocol/`）——
 * Rust 端 example 读的也是它 ⇒ 两端吃同一批夹具。
 *
 * @returns {{fixtures: Array<{id: string, fixture: object}>,
 *   rustCases: Array<{id: string, result: object}>|undefined}}
 */
function crossLanguageContext() {
  const dir = join(REPO, 'plugins', 'official-chat', 'fixtures', 'protocol');
  const index = JSON.parse(readFileSync(join(dir, 'index.json'), 'utf8'));
  const files = (Array.isArray(index.cases) ? index.cases : []).filter((name) =>
    String(name).includes('duplicates'),
  );
  return {
    fixtures: files.map((file) => {
      const fixture = JSON.parse(readFileSync(join(dir, String(file)), 'utf8'));
      return { id: String(fixture.id ?? file), fixture };
    }),
    rustCases: runRustFixtureCases(REPO),
  };
}

test('P2-D：跨语言 groupKey —— 三方一致才绿；一端漂移要红；Rust 没产物报「未执行」', async () => {
  const snapshot = {
    accountScope: 'desktop-dev/default/site-A',
    sessionId: 'sess-alpha-001',
    branch: 'main',
    capturedAt: '2026-10-20T08:59:00.000Z',
    contentDigest: '3a1f0c9d2b7e64a58d1c0f5b9e2a7d43618c0b5e9f2a1d7c4b8e6f0a3d5c7b19',
  };
  const wantKey = expectedGroupKey(snapshot);
  // 判据要的是 `{id, fixture}` 的形状（`fixture` 里才是夹具内容）。
  const fixture = {
    id: 'duplicates-same',
    fixture: { input: { kind: 'duplicates', snapshots: [snapshot, { ...snapshot }] } },
  };
  const rustOk = [
    { id: 'duplicates-same', result: { kind: 'duplicates', sameGroup: true, groupKey: wantKey } },
  ];

  // 合规：协议独立转写 ＝ JS ＝ Rust。
  const ok = await checkP2_D_crossLanguageGroupKey({
    fixtures: [fixture],
    rustCases: rustOk,
    adapter: makeSyntheticAdapter(),
  });
  assert.equal(ok.status, PASS, `三方一致不该红：${ok.detail}`);

  // 违规①：**JS 端漂移**。
  const jsDrift = await checkP2_D_crossLanguageGroupKey({
    fixtures: [fixture],
    rustCases: rustOk,
    adapter: makeSyntheticAdapter({ keyOf: () => '0123456789abcdef' }),
  });
  assert.equal(jsDrift.status, FAIL);
  assert.match(jsDrift.detail, /JS 端算出/);

  // 违规②：**Rust 端漂移**。
  const rustDrift = await checkP2_D_crossLanguageGroupKey({
    fixtures: [fixture],
    rustCases: [
      { id: 'duplicates-same', result: { kind: 'duplicates', sameGroup: true, groupKey: '0123456789abcdef' } },
    ],
    adapter: makeSyntheticAdapter(),
  });
  assert.equal(rustDrift.status, FAIL);
  assert.match(rustDrift.detail, /Rust 端算出/);

  // **Rust 没产物 ⇒ 报「未执行」，不判通过**（与 P1 A1c 同一口径）。
  const noRust = await checkP2_D_crossLanguageGroupKey({
    fixtures: [fixture],
    rustCases: undefined,
    adapter: makeSyntheticAdapter(),
  });
  assert.equal(noRust.status, SKIPPED);
  assert.match(noRust.detail, /未执行/);
});

test('活体注入（P2-D）：把 JS 端的分隔符写成含空格 ⇒ 跨语言漂移 ⇒ 判据红并点名 ⇒ 原实现逐字节未动', async () => {
  const context = crossLanguageContext();
  assert.ok(
    Array.isArray(context.rustCases),
    '需要先构建 Rust 端 example：cargo build --manifest-path src-tauri/Cargo.toml --example p1_protocol_fixture',
  );
  await runInjection({
    tag: 'p2-d',
    target: 'host',
    file: 'protocol.js',
    // **协议 v0.3 裁断**：五字段用**单个换行 U+000A、不含空格**连接。
    pattern: /\.map\(\(field\) => String\(snapshot\?\.\[field\] \?\? ''\)\)\.join\('\\n'\)/,
    replacement: ".map((field) => String(snapshot?.[field] ?? '')).join(' \\n ')",
    criterion: (adapter, ctx) => checkP2_D_crossLanguageGroupKey({ ...ctx, adapter }),
    context,
    expect: /JS 端算出|§1\.2/,
  });
});

// ---------------------------------------------------------------------------
// 真机实现上的「判据自身没问题」检查
// ---------------------------------------------------------------------------

test('九条判据在真实实现上都跑得起来（不是「未执行」、也不是「判据自身抛错」）', async () => {
  const cases = [
    { name: 'P2-1', run: checkP2_1_sameGroupKeepsDistinctIds, options: {} },
    { name: 'P2-2', run: checkP2_2_duplicateNoticeVisible, options: {} },
    { name: 'P2-3', run: checkP2_3_capturedAtNotGrouped, options: {} },
    { name: 'P2-4', run: checkP2_4_scopeBranchNotGrouped, options: {} },
    { name: 'P2-5', run: checkP2_5_budgetNoticeNoTruncation, options: { bodyBytes: BIG_BODY_BYTES } },
    { name: 'P2-6', run: checkP2_6_failureDoesNotBlockSend, options: {} },
    { name: 'P2-C', run: checkP2_C_errorCodeClosedSet, options: {} },
    {
      name: 'P2-D',
      run: (adapter, context) => checkP2_D_crossLanguageGroupKey({ ...context, adapter }),
      options: {},
      context: crossLanguageContext(),
    },
  ];
  for (const spec of cases) {
    const adapter = await createAdapter({ tag: `p2-selftest-${spec.name}`, ...spec.options });
    let result;
    try {
      result = await spec.run(adapter, spec.context ?? {});
    } finally {
      adapter.cleanup();
    }
    assert.notEqual(
      result.status,
      SKIPPED,
      `${spec.name} 不该是「未执行」：${result.detail}` +
        '（P2-D 需要先构建 Rust 端 example：cargo build --manifest-path src-tauri/Cargo.toml --example p1_protocol_fixture）',
    );
    assert.doesNotMatch(result.detail, /判据自身/, `${spec.name} 判据自身抛错：${result.detail}`);
  }
});
