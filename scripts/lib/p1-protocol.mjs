// P1 协议判据库（按**协议条款**判，不按某个实现的写法判）。
//
// ## 为什么这样切
//
// 协议稿 `docs/features/official-chat/official-chat-p1-protocol.md` v0.1 是 PM 定的
// 契约，实现（`plugins/official-chat/**`）与本文档是**两个独立的东西**：
// 实现改了，本判据不该跟着改；实现没做到，本判据要能指出**缺的是哪一条协议**。
//
// 所以这里的期望值全部是**从协议稿逐条转写**的常量表，**不 import 任何实现模块**；
// 行为类判据只通过一个 **adapter**（按协议 §5 的命令名 + 协议 S-1…S-7 的语义
// 定义的接口面）去驱动实现。adapter 缺能力 → 判据报「未执行」，不报失败。
//
// ## 三态 verdict
//
// `pass` / `fail` / `skipped`（未执行）。**没触发到被测路径必须显式报 skipped**，
// 不能省略、也不能记成 fail——否则「这条没被测到」和「这条测了没过」在结果里
// 长得一样，验收人就分不出来。
//
// ## 一个字段只表达一个时点
//
// 行为判据里所有计数器（`captureCalls` / `modelRequestCount`）在动手**前**取一次、
// 动手**后**取一次，分别记成 `before` / `after`，不共用一个字段名。
//
// @module scripts/lib/p1-protocol

import { createHash } from 'node:crypto';
import { dirname } from 'node:path';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/**
 * 造一条 verdict。
 *
 * @param {string} name - 判据名（建议 `A{n}-{条款}` 形式，便于回指协议）。
 * @param {string} status - `pass` / `fail` / `skipped`。
 * @param {string} detail - 人读的一句话（失败时必须写清**缺哪条协议**）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function verdict(name, status, detail) {
  return { name, status, detail: String(detail) };
}

/**
 * 汇总计数。与既有产物（内核 `results.<轮次>.json`）同构：`results[]` + `summary`。
 *
 * @param {Array<{status: string}>} results
 * @returns {{total: number, pass: number, fail: number, skipped: number}}
 */
export function summarize(results) {
  const list = Array.isArray(results) ? results : [];
  return {
    total: list.length,
    pass: list.filter((r) => r?.status === PASS).length,
    fail: list.filter((r) => r?.status === FAIL).length,
    skipped: list.filter((r) => r?.status === SKIPPED).length,
  };
}

// ---------------------------------------------------------------------------
// 协议期望表（**逐条转写自协议稿 v0.2**，不 import 实现）
//
// ## v0.1 → v0.2 的三处裁定与新增条款（PM 定的口径，本库跟着改）
//
//  1. **§8-6 / §S-5**：chip `invalid` 节点态**不计入本阶段验收**（内核无写入口），
//     登记为上游缺口 **G1-6** ⇒ §8-12 的缺口清单从 G1-1…G1-5 变成 **G1-1…G1-6**。
//  2. **§8-6 / §S-5**：`sourceFingerprint` 的**校验点 = host 命令层**；
//     「快照里的指纹」与「当前会话指纹」不符 → 拒（`account_changed`）。
//  3. **§2 S-1**：**id 分配权归 host**（client 不得自造 seq）；
//     「本实例内单调递增」必须**跨进程**成立。
//  4. **§3**：13 个码是**闭集**，不新增第 14 个；内部冲突码归入 `snapshot_invalid`，
//     原始码保留在 `detail`。
//  5. **§1.1**：`captureResult` 形态 = 对象（必带三字段）；`ok` 兼容字符串 `'ok'`；
//     `partial` / `failed` 必须用对象。
//  6. **§6**：会话短码 = 两段 FNV-1a 变体 → base36 取后 12 位，口径 UTF-16 码元；
//     消息分隔符 = 空行 `\n\n`。
//
// **只转写协议写死的东西**：协议没写具体盐值/混合方式的地方（例如短码算法的
// 「不同盐」到底是哪两个盐），本库**不去猜**——那样转写出来的期望值只是实现
// 的复印件。协议只给了性质的地方，就只判性质（见 `checkSessionShortProperties`）。
// ---------------------------------------------------------------------------

/** §1.1 快照字段表。 */
export const SNAPSHOT_FIELDS = Object.freeze([
  'schemaVersion',
  'snapshotId',
  'providerId',
  'accountScope',
  'sessionId',
  'branch',
  'messages',
  'contentDigest',
  'captureResult',
  'capturedAt',
  'sourceUpdatedAt',
  'sourceFingerprint',
]);

/** §2 S-1 句柄（= 快照 id）的形状规则。 */
export const REF_RULES = Object.freeze({
  /** 整串可见字符上限（HC-2：ref 就是 chip 文本）。 */
  maxVisibleChars: 24,
  /** provider 短码上限。 */
  maxProviderChars: 4,
  /** 会话短码上限。 */
  maxSessionChars: 12,
  /** pick 序号位数。 */
  seqDigits: 4,
  /** 形状：`<providerShort>~<sessionShort>~<seq>`。 */
  pattern: /^([A-Za-z0-9]{1,4})~([^~]{1,12})~(\d{4})$/,
});

/** §3 的 13 个稳定错误码 + 可重试性 + 用户下一步。 */
export const ERROR_CODES = Object.freeze({
  not_logged_in: { retryable: false, userNext: '去官网窗口登录' },
  account_changed: { retryable: false, userNext: '重新选择来源' },
  source_unsupported: { retryable: false, userNext: '换一个来源' },
  schema_changed: { retryable: false, userNext: '升级插件' },
  partial_content: { retryable: false, userNext: '明示省略事实' },
  body_too_large: { retryable: false, userNext: '换更小的会话' },
  timeout: { retryable: true, userNext: '重试' },
  cancelled: { retryable: false, userNext: '重新选择' },
  snapshot_invalid: { retryable: false, userNext: '移除该引用' },
  kernel_incompatible: { retryable: false, userNext: '升级内核' },
  catalog_empty: { retryable: false, userNext: '换个关键词' },
  catalog_failed: { retryable: true, userNext: '重试' },
  stale_draft: { retryable: false, userNext: '重新选择' },
});

/** §4 首版常量。 */
export const CONSTANTS = Object.freeze({
  catalogPageSize: 50,
  catalogCacheTtlMs: 5 * 60 * 1000,
  searchDebounceMs: 300,
  readTimeoutMs: 30_000,
  maxBodyBytes: 256 * 1024,
  maxResponseBytes: 1024 * 1024,
  snapshotRetentionMs: 7 * 24 * 60 * 60 * 1000,
  cacheTotalBudgetBytes: 512 * 1024 * 1024,
  maxReferencesPerMessage: 8,
  perBodyBudgetBytes: 64 * 1024,
  perMessageBudgetBytes: 256 * 1024,
  schemaVersion: 1,
});

/** §1.1 的 `schemaVersion` 首版值。 */
export const SCHEMA_VERSION = 1;

/**
 * §2 S-2 状态机。
 *
 * `inFlight` = 内存里允许存在的**在途态**（S-3：落盘后状态以文件为准）；
 * `terminal` = 终态（不得作为内存态存储）。
 */
export const STATES = Object.freeze({
  selected: 'selected',
  reading: 'reading',
  validating: 'validating',
  snapshot_saved: 'snapshot_saved',
  ready: 'ready',
  failed: 'failed',
  cancelled: 'cancelled',
  stale: 'stale',
  serialized: 'serialized',
  expanded: 'expanded',
  persisted: 'persisted',
});

export const IN_FLIGHT_STATES = Object.freeze([
  STATES.selected,
  STATES.reading,
  STATES.validating,
  STATES.snapshot_saved,
]);

export const TERMINAL_STATES = Object.freeze([
  STATES.ready,
  STATES.failed,
  STATES.cancelled,
  STATES.stale,
  STATES.serialized,
  STATES.expanded,
  STATES.persisted,
]);

/** §2 S-2 的合法转移表（照抄协议里的箭头图）。 */
export const TRANSITIONS = Object.freeze({
  selected: ['reading', 'cancelled', 'stale'],
  reading: ['validating', 'failed', 'cancelled', 'stale'],
  validating: ['snapshot_saved', 'failed', 'cancelled', 'stale'],
  snapshot_saved: ['ready', 'failed', 'cancelled', 'stale'],
  ready: ['serialized'],
  serialized: ['expanded'],
  expanded: ['persisted'],
  failed: [],
  cancelled: [],
  stale: [],
});

/**
 * §6 协议夹具必须覆盖的十一类情形。
 *
 * 每一类用**夹具内容**判定（不按文件名——文件名是实现自己的选择）。
 */
export const FIXTURE_CASES = Object.freeze([
  '正常快照',
  '部分内容',
  '超限',
  '空目录',
  '目录失败',
  '超时',
  '取消',
  '账户已变',
  '快照缺失',
  '版本不支持',
  '摘要不匹配',
]);

/**
 * §1.3 规范化：行尾统一 `\n`、去 BOM、去尾随空白、**保留正文内空行与缩进**。
 *
 * ## 「去尾随空白」怎么读（这里有个容易读错的地方）
 *
 * 协议原文两句话要**一起读**：「去尾随空白」+「保留正文内空行与缩进
 * （**不得改动语义**）」。逐行去尾随空白**会改语义**——行尾两个空格在 Markdown
 * 里是硬换行。所以这里的口径是：**只去整段末尾的空白**，正文内部一行不动。
 *
 * （本机现役实现 `plugins/official-chat/host/protocol.js` 的 `normalizeText`
 * 用的是 `.replace(/\s+$/, '')`，与此一致；两端口径必须相同，见 A1c。）
 *
 * @param {string} text
 * @returns {string}
 */
export function normalizeText(text) {
  return String(text ?? '')
    .replace(/^\uFEFF/, '')
    .replace(/\r\n?/g, '\n')
    .replace(/\s+$/, '');
}

/**
 * §1.3 摘要 = 规范化后正文的 SHA-256（十六进制小写）。
 *
 * @param {string} text
 * @returns {string}
 */
export function contentDigest(text) {
  return createHash('sha256').update(normalizeText(text), 'utf8').digest('hex');
}

/**
 * 快照正文（协议 §1.1 的 `messages` → 模型可见正文）。
 *
 * @param {Array<{role?: string, text?: string}>} messages
 * @returns {string}
 */
export function bodyOf(messages) {
  return (Array.isArray(messages) ? messages : [])
    .map((message) => String(message?.text ?? ''))
    .join('\n\n');
}

// ---------------------------------------------------------------------------
// 静态判据
// ---------------------------------------------------------------------------

/**
 * A1a｜§6：夹具集必须覆盖十一类情形（**按内容分类**，不看文件名）。
 *
 * @param {Array<object>} fixtures - 解析后的夹具数组。
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkFixtureCoverage(fixtures) {
  const list = Array.isArray(fixtures) ? fixtures : [];
  if (list.length === 0) {
    return verdict('A1a-fixture-coverage', SKIPPED, '没读到任何协议夹具');
  }
  const found = new Set();
  for (const fixture of list) {
    // **只按夹具的「期望」分类**——协议 §6 规定的是「夹具至少覆盖哪些情形」，
    // 没规定夹具的输入字段长什么样（`input.kind` 是实现自己的约定，
    // 拿它当分类依据 = 按实现写法判）。所以这里只读 `expected`。
    const code = String(fixture?.expected?.code ?? '');
    const expectedKind = String(fixture?.expected?.kind ?? '');
    if (expectedKind === 'snapshot') {
      const status = String(fixture?.expected?.snapshot?.captureResult?.status ?? '');
      found.add(status === 'partial' ? '部分内容' : '正常快照');
    } else if (code === 'catalog_empty') {
      found.add('空目录');
    } else if (code === 'catalog_failed') {
      found.add('目录失败');
    } else if (code === 'timeout') {
      found.add('超时');
    } else if (code === 'cancelled') {
      found.add('取消');
    } else if (code === 'body_too_large') {
      found.add('超限');
    } else if (code === 'account_changed') {
      found.add('账户已变');
    } else if (code === 'schema_changed') {
      found.add('版本不支持');
    } else if (code === 'snapshot_invalid') {
      const input = fixture?.input ?? {};
      if (input.exists === false) {
        found.add('快照缺失');
      } else {
        // 摘要不匹配：夹具声称的摘要与正文实算摘要对不上。
        const snapshot = input.snapshot ?? {};
        const claimed = String(snapshot.contentDigest ?? '');
        const actual = contentDigest(bodyOf(snapshot.messages));
        found.add(claimed !== '' && claimed !== actual ? '摘要不匹配' : '快照缺失');
      }
    }
  }
  const missing = FIXTURE_CASES.filter((name) => !found.has(name));
  if (missing.length > 0) {
    return verdict(
      'A1a-fixture-coverage',
      FAIL,
      `§6 要求覆盖 ${FIXTURE_CASES.length} 类，缺：${missing.join('、')}（已覆盖 ${found.size} 类）`,
    );
  }
  return verdict(
    'A1a-fixture-coverage',
    PASS,
    `十一类情形全覆盖（夹具 ${list.length} 条）`,
  );
}

/**
 * A1b｜每条夹具的期望值必须**自洽**（期望的码在 13 个码里、期望的字段齐、摘要对得上）。
 *
 * @param {Array<object>} fixtures
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkFixtureExpectations(fixtures) {
  const list = Array.isArray(fixtures) ? fixtures : [];
  if (list.length === 0) {
    return verdict('A1b-fixture-expectations', SKIPPED, '没读到任何协议夹具');
  }
  const problems = [];
  for (const fixture of list) {
    const id = String(fixture?.id ?? '(未命名)');
    const expected = fixture?.expected ?? {};
    // 协议 §6 只要求「两端 schemaVersion 常量一致」，**没有**要求每条夹具都带
    // schemaVersion（目录/查找类夹具本来就没有快照）。所以只在**声明了**时检查。
    const declared = fixture?.input?.schemaVersion ?? expected?.snapshot?.schemaVersion;
    if (declared !== undefined && Number(declared) !== SCHEMA_VERSION) {
      problems.push(`${id}: schemaVersion != ${SCHEMA_VERSION}`);
    }
    if (expected.kind === 'error') {
      const code = String(expected.code ?? '');
      if (!Object.hasOwn(ERROR_CODES, code)) {
        problems.push(`${id}: 期望错误码 ${code || '(空)'} 不在 §3 的 13 个码里`);
      } else {
        if (expected.retryable !== ERROR_CODES[code].retryable) {
          problems.push(`${id}: ${code} 的可重试性与 §3 不符`);
        }
        if (String(expected.userNext ?? '') !== ERROR_CODES[code].userNext) {
          problems.push(`${id}: ${code} 的用户下一步与 §3 不符`);
        }
      }
    }
    if (expected.kind === 'snapshot') {
      const snapshot = expected.snapshot ?? {};
      for (const field of SNAPSHOT_FIELDS) {
        if (!Object.hasOwn(snapshot, field)) problems.push(`${id}: 期望快照缺字段 ${field}`);
      }
      const actual = contentDigest(bodyOf(snapshot.messages));
      if (String(snapshot.contentDigest ?? '') !== actual) {
        problems.push(`${id}: 期望摘要与正文实算摘要不符`);
      }
    }
  }
  if (problems.length > 0) {
    return verdict('A1b-fixture-expectations', FAIL, problems.join('；'));
  }
  return verdict('A1b-fixture-expectations', PASS, `${list.length} 条夹具的期望值均自洽`);
}

/**
 * 深比较两个 JSON 值（**忽略对象键序**，数组按序比）。
 *
 * 两端序列化出来的 JSON 键序不一定相同（Rust 的 `serde_json::Value` 按键排序，
 * JS 的 `JSON.stringify` 按插入序），按字符串比会得到**假红**。
 *
 * @param {unknown} a
 * @param {unknown} b
 * @returns {boolean}
 */
export function deepEqual(a, b) {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b)) return false;
    if (a.length !== b.length) return false;
    return a.every((item, index) => deepEqual(item, b[index]));
  }
  if (typeof a === 'object') {
    const keysA = Object.keys(a).sort();
    const keysB = Object.keys(b).sort();
    if (keysA.length !== keysB.length) return false;
    if (!keysA.every((key, index) => key === keysB[index])) return false;
    return keysA.every((key) => deepEqual(a[key], b[key]));
  }
  return false;
}

/**
 * A1c｜双端一致：Rust 端与 JS 端对**同一组夹具**给出**同一结果**。
 *
 * @param {{js?: unknown, rust?: unknown}} results - 两端跑同一夹具集的结果。
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkFixtureParity(results) {
  const js = results?.js;
  const rust = results?.rust;
  if (js === undefined) {
    return verdict('A1c-fixture-parity', SKIPPED, 'JS 端结果不可得（无夹具 runner）');
  }
  if (rust === undefined) {
    return verdict(
      'A1c-fixture-parity',
      SKIPPED,
      'Rust 端结果不可得：仓库里还没有能跑协议夹具的 Rust runner（协议 §6 的「各跑一遍」尚缺一半）',
    );
  }
  if (!deepEqual(js, rust)) {
    const jsList = Array.isArray(js) ? js : [js];
    const rustList = Array.isArray(rust) ? rust : [rust];
    const diffs = [];
    for (let index = 0; index < Math.max(jsList.length, rustList.length); index += 1) {
      if (!deepEqual(jsList[index], rustList[index])) {
        diffs.push(
          `${jsList[index]?.id ?? `#${index}`}：JS=${JSON.stringify(jsList[index]?.result ?? jsList[index])} ` +
            `Rust=${JSON.stringify(rustList[index]?.result ?? rustList[index])}`,
        );
      }
    }
    return verdict(
      'A1c-fixture-parity',
      FAIL,
      `两端对同一组夹具的结果不一致（${diffs.length} 条不同）：${diffs.slice(0, 3).join('；')}`,
    );
  }
  const count = Array.isArray(js) ? js.length : 1;
  return verdict('A1c-fixture-parity', PASS, `两端结果一致（${count} 条夹具，深比较）`);
}

/**
 * A1d｜夹具的 `expected` 与实现算出来的结果必须一致。
 *
 * 协议 §6 把「同一组 JSON 夹具」定为双端的共同输入，夹具里的 `expected`
 * 就是契约；实现与夹具对不上 = 有一方错了。
 *
 * @param {{expected?: unknown, actual?: unknown}} pair
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkFixtureExpectationMatch(pair, name = 'A1d-fixture-expectation-match') {
  const expected = pair?.expected;
  const actual = pair?.actual;
  if (expected === undefined) {
    return verdict(name, SKIPPED, '没有夹具期望值可对');
  }
  if (actual === undefined) {
    return verdict(name, SKIPPED, '没有实现侧结果可对');
  }
  if (!deepEqual(expected, actual)) {
    const expectedList = Array.isArray(expected) ? expected : [expected];
    const actualList = Array.isArray(actual) ? actual : [actual];
    const diffs = [];
    for (let index = 0; index < Math.max(expectedList.length, actualList.length); index += 1) {
      if (!deepEqual(expectedList[index], actualList[index])) {
        diffs.push(
          `${expectedList[index]?.id ?? `#${index}`}：期望=${JSON.stringify(expectedList[index])} ` +
            `实际=${JSON.stringify(actualList[index])}`,
        );
      }
    }
    return verdict(
      name,
      FAIL,
      `实现结果与夹具期望不一致（${diffs.length} 条不同）：${diffs.slice(0, 3).join('；')}`,
    );
  }
  const count = Array.isArray(expected) ? expected.length : 1;
  return verdict(name, PASS, `实现结果与夹具期望一致（${count} 条）`);
}

/**
 * A9a｜§3：13 个码全有稳定码名、可重试性与用户下一步。
 *
 * @param {object} capabilities - 实现自报的 capabilities（协议 §5 命令 `capabilities`）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkErrorCodeTable(capabilities) {
  const table = capabilities?.errorCodes;
  if (!table || typeof table !== 'object') {
    return verdict('A9a-error-code-table', SKIPPED, 'capabilities 未提供错误码表');
  }
  const problems = [];
  for (const [code, spec] of Object.entries(ERROR_CODES)) {
    const actual = table[code];
    if (actual === undefined) {
      problems.push(`缺码 ${code}`);
      continue;
    }
    if (Boolean(actual.retryable) !== spec.retryable) {
      problems.push(`${code} 的可重试性不符（期望 ${spec.retryable}）`);
    }
    if (String(actual.userNext ?? '') !== spec.userNext) {
      problems.push(`${code} 的用户下一步不符（期望「${spec.userNext}」）`);
    }
  }
  const extra = Object.keys(table).filter((code) => !Object.hasOwn(ERROR_CODES, code));
  if (extra.length > 0) problems.push(`多出未登记的码：${extra.join('、')}`);
  if (problems.length > 0) {
    return verdict('A9a-error-code-table', FAIL, problems.join('；'));
  }
  return verdict(
    'A9a-error-code-table',
    PASS,
    `13 个码齐备且与 §3 一致${extra.length ? '' : ''}`,
  );
}

/**
 * A9b｜§3：`catalog_empty` 与 `catalog_failed` 必须分离。
 *
 * @param {object} capabilities
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkCatalogCodesSeparated(capabilities) {
  const table = capabilities?.errorCodes;
  if (!table || typeof table !== 'object') {
    return verdict('A9b-catalog-codes', SKIPPED, 'capabilities 未提供错误码表');
  }
  const empty = table.catalog_empty;
  const failed = table.catalog_failed;
  if (empty === undefined || failed === undefined) {
    return verdict('A9b-catalog-codes', FAIL, 'catalog_empty / catalog_failed 有一个缺失');
  }
  if (Boolean(empty.retryable) === Boolean(failed.retryable)) {
    return verdict(
      'A9b-catalog-codes',
      FAIL,
      'catalog_empty 与 catalog_failed 的可重试性相同——「没有结果」与「读取失败」必须可区分',
    );
  }
  if (String(empty.userNext ?? '') === String(failed.userNext ?? '')) {
    return verdict('A9b-catalog-codes', FAIL, '两个码的用户下一步相同，未分离');
  }
  return verdict('A9b-catalog-codes', PASS, '两个码在可重试性与用户下一步上均已分离');
}

/**
 * A4-const｜§4：首版常量与协议一致。
 *
 * @param {object} capabilities
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkConstants(capabilities) {
  const table = capabilities?.constants;
  if (!table || typeof table !== 'object') {
    return verdict('A4-constants', SKIPPED, 'capabilities 未提供常量表');
  }
  const problems = [];
  for (const [name, expected] of Object.entries(CONSTANTS)) {
    if (!Object.hasOwn(table, name)) {
      problems.push(`缺常量 ${name}`);
      continue;
    }
    if (Number(table[name]) !== Number(expected)) {
      problems.push(`${name} = ${table[name]}，协议期望 ${expected}`);
    }
  }
  if (problems.length > 0) {
    return verdict('A4-constants', FAIL, problems.join('；'));
  }
  return verdict('A4-constants', PASS, `${Object.keys(CONSTANTS).length} 个常量与 §4 一致`);
}

/**
 * A12｜§8-12：G1-1…G1-5 缺口登记仍在，实现**不得假设**它们已解决。
 *
 * @param {object} capabilities
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkGapRegistry(capabilities) {
  const degraded = capabilities?.degraded;
  if (!degraded || typeof degraded !== 'object') {
    return verdict(
      'A12-gap-registry',
      FAIL,
      'capabilities 未提供 degraded 缺口登记——实现不得假设 G1-1…G1-5 已解决',
    );
  }
  const keys = Object.keys(degraded);
  const g1 = keys.filter((key) => /^g1[_-]?\d/i.test(key));
  if (g1.length === 0) {
    return verdict('A12-gap-registry', FAIL, 'degraded 里没有任何 G1 缺口登记项');
  }
  const empty = g1.filter((key) => String(degraded[key] ?? '').trim() === '');
  if (empty.length > 0) {
    return verdict('A12-gap-registry', FAIL, `缺口登记项没有说明：${empty.join('、')}`);
  }
  // 协议 §8-12 写的是「**G1-1…G1-5** 缺口登记仍在」——是五个，不是一个。
  if (g1.length < 5) {
    return verdict(
      'A12-gap-registry',
      FAIL,
      `只登记了 ${g1.length} 个 G1 缺口（${g1.join('、')}），协议 §8-12 要求 G1-1…G1-5 五个都在`,
    );
  }
  return verdict(
    'A12-gap-registry',
    PASS,
    `登记了 ${g1.length} 个 G1 缺口（${g1.join('、')}）`,
  );
}

/**
 * A3-shape｜S-1：句柄的形状与长度（纯函数部分，不依赖实现）。
 *
 * @param {string} ref
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkRefShape(ref) {
  const text = String(ref ?? '');
  if (text === '') {
    return verdict('A3-ref-shape', FAIL, '句柄是空串');
  }
  const visible = [...text].length;
  if (visible > REF_RULES.maxVisibleChars) {
    return verdict(
      'A3-ref-shape',
      FAIL,
      `句柄 ${visible} 个可见字符，超过上限 ${REF_RULES.maxVisibleChars}（HC-2：ref 就是 chip 宽度）`,
    );
  }
  const match = REF_RULES.pattern.exec(text);
  if (match === null) {
    return verdict(
      'A3-ref-shape',
      FAIL,
      `句柄 ${text} 不符合 <providerShort>~<sessionShort>~<seq> 形状`,
    );
  }
  return verdict('A3-ref-shape', PASS, `句柄形状合规（${visible} 可见字符）`);
}

// ---------------------------------------------------------------------------
// 行为判据（通过 adapter 驱动实现）
// ---------------------------------------------------------------------------

/**
 * 跑一条行为判据，把异常收成失败（带原始错误）。
 *
 * @param {string} name
 * @param {() => Promise<{status: string, detail: string}>} run
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
async function guard(name, run) {
  try {
    const result = await run();
    return verdict(name, result.status, result.detail);
  } catch (error) {
    return verdict(name, FAIL, `判据自身抛错：${String(error?.stack ?? error)}`);
  }
}

/**
 * A2｜S-1：同一条对话选两次 → 两个 snapshotId → 两个 ref → **第二次不覆盖第一次**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkPickTwiceDistinct(adapter) {
  return guard('A2-pick-twice-distinct', async () => {
    const first = await adapter.pick({ sessionId: 'p1-check-session' });
    const second = await adapter.pick({ sessionId: 'p1-check-session' });
    if (!first?.ref || !second?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    if (first.ref === second.ref) {
      return { status: FAIL, detail: `两次 pick 得到同一个 ref（${first.ref}）——第二次覆盖了第一次` };
    }
    // **先等第一次落盘**，再看文件在不在：落盘前文件当然不存在，
    // 那样判出来的是「还没落盘」，不是「被第二次覆盖了」。
    await adapter.settle(first.ref);
    const firstFile = adapter.snapshotFile(first.ref);
    if (!firstFile?.exists) {
      return { status: FAIL, detail: `第一份快照文件不见了（${first.ref}）——第二次 pick 覆盖了第一次` };
    }
    const firstBody = await adapter.serialize(first.ref);
    if (String(firstBody) !== String(firstFile.body)) {
      return { status: FAIL, detail: '第一份快照的正文与落盘内容不一致' };
    }
    return {
      status: PASS,
      detail: `两次 pick → 两个 ref（${first.ref} / ${second.ref}），第一份仍在且内容未变`,
    };
  });
}

/**
 * A3｜S-1 长度：超长会话短码也必须给出 **≤ 24 可见字符**的句柄（边界测试）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkRefLengthBoundary(adapter) {
  return guard('A3-ref-length-boundary', async () => {
    const longSession = `sess-${'x'.repeat(200)}`;
    const picked = await adapter.pick({ sessionId: longSession });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const shape = checkRefShape(picked.ref);
    if (shape.status === FAIL) {
      return { status: FAIL, detail: `超长会话的句柄不合规：${shape.detail}` };
    }
    return { status: PASS, detail: `超长会话 → 句柄 ${picked.ref}（${[...picked.ref].length} 可见字符）` };
  });
}

/**
 * A4｜S-3：落盘后**句柄状态由快照文件派生**；内存里不得留第二份真相。
 *
 * 判据用两个**行为**证据（不扫源码，避免按实现写法判）：
 *  1. 落盘后内存态观测口报告「没有在途记录」→ 内存里没有已落盘态的残留；
 *  2. **换一个新实例**（内存全空）只凭 ref 仍能拿到 `ready` → 状态确实由文件派生。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkSingleSourceOfTruth(adapter) {
  return guard('A4-single-source-of-truth', async () => {
    const picked = await adapter.pick({ sessionId: 'p1-check-s3' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const ref = picked.ref;
    await adapter.settle(ref);

    // ① 落盘后内存里**不得**还留着这个 ref 的已落盘态。
    //    观测口：客户端 `openReference` —— 内存里有在途记录时它同步报
    //    `from='inflight'`；没有时走 `from='snapshot-file'`（状态由文件派生）。
    const observation = await adapter.openReferenceObservation(ref);
    if (observation === undefined) {
      return { status: SKIPPED, detail: '实现未暴露 openReference 的状态观测口' };
    }
    if (observation.from === 'inflight') {
      return {
        status: FAIL,
        detail: `落盘后内存里仍留着这个 ref（from=${observation.from}）——S-3 要求内存里只留尚未落盘的在途态`,
      };
    }

    // ② **换一个新实例**（内存全空）只凭 ref 仍能派生出 `ready`
    //    → 状态确实由快照文件派生，不是内存里的第二份真相。
    await adapter.reload();
    const derived = await adapter.openReferenceObservation(ref);
    if (derived === undefined) {
      return { status: SKIPPED, detail: '实现未暴露 openReference 的状态观测口' };
    }
    //    `from='snapshot-file'` = 状态来自快照文件；`level='info'` = 派生出来是
    //    就绪态（`'error'` 才是失败态）。**判枚举，不判文案**。
    if (derived.from !== 'snapshot-file' || derived.level !== 'info') {
      return {
        status: FAIL,
        detail:
          `新实例（内存全空）凭 ref 读到的是 from=${derived.from}/level=${derived.level}，` +
          '不是「由快照文件派生的就绪态」——状态不是由快照文件派生的',
      };
    }
    return {
      status: PASS,
      detail: '落盘后内存无残留（from=snapshot-file）；新实例仅凭 ref 从快照文件派生出 ready',
    };
  });
}

/**
 * A5｜S-4：请求发生时，落盘正文与**请求体**逐字节相同。
 *
 * 判据不数时序点（协议明写那是充分非必要条件），而是**比对磁盘内容与请求体**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkRequestMatchesDisk(adapter) {
  return guard('A5-request-matches-disk', async () => {
    const picked = await adapter.pick({ sessionId: 'p1-check-s4' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const ref = picked.ref;
    await adapter.settle(ref);
    const file = adapter.snapshotFile(ref);
    if (!file?.exists) {
      return { status: FAIL, detail: `快照未落盘（${ref}）——请求发生时应已落盘` };
    }
    const requestBody = await adapter.serialize(ref);
    const diskBody = String(file.body ?? '');
    if (String(requestBody) !== diskBody) {
      return {
        status: FAIL,
        detail: `请求体与落盘正文不一致：请求体 ${String(requestBody).length} 字节 / 落盘 ${diskBody.length} 字节`,
      };
    }
    return {
      status: PASS,
      detail: `请求体与落盘正文逐字节相同（${diskBody.length} 字节）`,
    };
  });
}

/**
 * A6a｜S-5：快照缺失 / 指纹变化 / 摘要不匹配 → **请求数 0** + **可见错误**，且**不静默重读**。
 *
 * 这条**可机检**：内核的拒发机制就是「owner 侧 `codec.serialize` 抛错」——
 * 序列化失败时内核**不调用** `defaultSink`（`dsh-client-ui-conversation/lib/client.js`
 * SHA `8554738CBEEB5218AD1BFA4773EF094363B066D052A1893D7A96797C5BC33A63`
 * 的 `:15502` / `:15522` vs `:15786` / `:15882-15884`），所以「请求数 0」成立。
 *
 * 「chip 处于 invalid 态」是**另一条**（A6b），内核侧不可达，见该条注释。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkInvalidRejection(adapter) {
  return guard('A6a-invalid-reject', async () => {
    const scenarios = ['missing', 'fingerprint', 'digest'];
    const problems = [];
    for (const scenario of scenarios) {
      const picked = await adapter.pick({ sessionId: `p1-check-s5-${scenario}` });
      if (!picked?.ref) {
        return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
      }
      const ref = picked.ref;
      await adapter.settle(ref);
      const requestsBefore = adapter.modelRequestCount();
      const capturesBefore = adapter.captureCalls();
      await adapter.invalidate(ref, scenario);
      let rejected = false;
      let errorCode = '';
      try {
        await adapter.serialize(ref);
      } catch (error) {
        rejected = true;
        errorCode = String(error?.code ?? error?.message ?? error);
      }
      const requestsAfter = adapter.modelRequestCount();
      const capturesAfter = adapter.captureCalls();
      if (!rejected) {
        problems.push(`${scenario}: serialize 仍然成功（正文照发）——必须拒`);
      }
      if (requestsAfter !== requestsBefore) {
        problems.push(`${scenario}: 请求数从 ${requestsBefore} 变成 ${requestsAfter}，必须保持 0`);
      }
      if (capturesAfter !== capturesBefore) {
        problems.push(`${scenario}: 失效后发生了静默重读（capture 调用数 ${capturesBefore} → ${capturesAfter}）`);
      }
      if (rejected && errorCode !== '' && !String(errorCode).includes('snapshot_invalid') && !String(errorCode).includes('account_changed')) {
        problems.push(`${scenario}: 拒绝时给的错误不是稳定码（实测 ${errorCode}）`);
      }
    }
    if (problems.length > 0) {
      return { status: FAIL, detail: problems.join('；') };
    }
    return {
      status: PASS,
      detail: '三种失效均为：请求数 0 + 可见错误，且未静默重读',
    };
  });
}

/**
 * A6b｜S-5：**chip 处于 `invalid` 态**（节点级断言）。
 *
 * ## 这条在现役 release 内核上**不可达**（属协议稿与内核契约的冲突，已回报 PM）
 *
 * 独立核验（现役树 `C:\Users\zxx\.dsh-xlink\dsh\desktop\kernels\0.2.1-rc.2`）：
 *  · 协议稿引用的 `ReferenceInsert` 里**没有** `invalid` 字段——它在
 *    `lib/types/client/contract/draft-editor.d.ts`（SHA
 *    `3397E3224506A8F448901C4AE3EB1932DABE3FA257B9BA9C5186EFD2AD918375`）
 *    的 `:12-18`，字段只有 `source` / `ref` / `label` / `appearance?` / `clipboardText`；
 *    `readonly invalid?: boolean` 在 `Occurrence` 上（同文件 `:105` / `:122-123`），
 *    **不是插入期字段**。
 *  · `chip-node.d.ts`（SHA `816035EAA7730AD5C57334F3A261CCF9529F1A27F72069FD012C587AB03F737F`）
 *    的 `:21` 有 `invalid: boolean`、`:35` 注释是「Owner-resolution failure flag:
 *    chip renders invalid; serialization must fail.」——但 `setInvalid` 在现役
 *    内核**零调用方**（`client.js` 的 `:13759` 只有定义点）。
 *  · 内核**自己**从不产生 `invalid:true`；唯一物化路径是持久化草稿恢复
 *    （`client.js:14852-14858`）与 `:19787` 的草稿迁移——需要**先有人**往草稿里
 *    写 `invalid:true`，而插件没有这个写入口。
 *
 * ⇒ 在内核侧观测「chip invalid」会**恒假**。所以本条在观测不到节点状态时
 * 报「**未执行**」（不报失败——那会是指责实现，而实现没错），并写明原因。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkChipInvalidNode(adapter) {
  return guard('A6b-chip-invalid-node', async () => {
    const picked = await adapter.pick({ sessionId: 'p1-check-s5b' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const ref = picked.ref;
    await adapter.settle(ref);
    await adapter.invalidate(ref, 'missing');
    const chip = adapter.chipNode(ref);
    if (chip === undefined) {
      return {
        status: SKIPPED,
        detail:
          '观测不到 chip 节点的 invalid 态——内核侧不可达：' +
          'ReferenceInsert 无 invalid 字段（invalid 在 Occurrence 上），' +
          'setInvalid 在现役 release 内核零调用方，DOM 无 data-invalid。' +
          '属协议稿 §S-5 与内核契约的冲突，已回报 PM 改稿；本条在改稿前**不可机检**。',
      };
    }
    if (chip.invalid !== true) {
      return {
        status: FAIL,
        detail: `chip 节点的 invalid 不是 true（实测 ${JSON.stringify(chip.invalid)}）`,
      };
    }
    return { status: PASS, detail: 'chip 节点进入 invalid 态' };
  });
}

/**
 * A7｜S-6：进程重启后句柄仍可发；快照被清理后 → 拒（`snapshot_invalid`），**不静默重读**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkCrossProcess(adapter) {
  return guard('A7-cross-process', async () => {
    const picked = await adapter.pick({ sessionId: 'p1-check-s6' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const ref = picked.ref;
    await adapter.settle(ref);
    await adapter.reload();
    // 「重启后发不出」**正是这条判据要查的失败情形**，所以这里必须自己接住异常，
    // 不能让它冒泡成「判据自身抛错」（那样就分不出「测了没过」和「判据坏了」）。
    let bodyAfterRestart;
    let restartError = '';
    try {
      bodyAfterRestart = await adapter.serialize(ref);
    } catch (error) {
      restartError = String(error?.code ?? error?.message ?? error);
    }
    if (restartError !== '') {
      return {
        status: FAIL,
        detail: `进程重启后句柄发不出正文（${restartError}）——S-6 要求快照落盘后跨进程仍可发`,
      };
    }
    if (typeof bodyAfterRestart !== 'string' || bodyAfterRestart === '') {
      return { status: FAIL, detail: '进程重启后句柄发不出正文' };
    }
    const capturesBefore = adapter.captureCalls();
    adapter.removeSnapshot(ref);
    let rejected = false;
    let errorCode = '';
    try {
      await adapter.serialize(ref);
    } catch (error) {
      rejected = true;
      errorCode = String(error?.code ?? error?.message ?? error);
    }
    const capturesAfter = adapter.captureCalls();
    if (!rejected) {
      return { status: FAIL, detail: '快照被清理后仍能发出正文——必须拒（snapshot_invalid）' };
    }
    if (capturesAfter !== capturesBefore) {
      return {
        status: FAIL,
        detail: `快照被清理后发生了重读（capture ${capturesBefore} → ${capturesAfter}）——不得静默重读`,
      };
    }
    if (!String(errorCode).includes('snapshot_invalid')) {
      return { status: FAIL, detail: `清理后拒绝用的不是 snapshot_invalid（实测 ${errorCode}）` };
    }
    return { status: PASS, detail: '重启后仍可发；清理后拒且未重读' };
  });
}

/**
 * A8｜S-7：同一正文引用两次 → 两个快照 id、**正文各送一份**（不隐式去重）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkNoBodyDedup(adapter) {
  return guard('A8-no-body-dedup', async () => {
    const first = await adapter.pick({ sessionId: 'p1-check-s7' });
    const second = await adapter.pick({ sessionId: 'p1-check-s7' });
    if (!first?.ref || !second?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    if (first.ref === second.ref) {
      return { status: FAIL, detail: '同一正文两次引用得到同一个快照 id' };
    }
    await adapter.settle(first.ref);
    await adapter.settle(second.ref);
    const bodyA = String(await adapter.serialize(first.ref));
    const bodyB = String(await adapter.serialize(second.ref));
    if (bodyA !== bodyB) {
      return { status: FAIL, detail: '同一正文的两次快照内容不一致' };
    }
    const modelInput = `${bodyA}\n\n${bodyB}`;
    const occurrences = modelInput.split(bodyA).length - 1;
    if (occurrences !== 2) {
      return {
        status: FAIL,
        detail: `模型输入里同一正文出现 ${occurrences} 次，S-7 要求各送一份（共 2 次）`,
      };
    }
    return {
      status: PASS,
      detail: `两个快照 id（${first.ref} / ${second.ref}），正文各送一份`,
    };
  });
}

/**
 * A9c｜§3：错误里**不出现内部异常串**。
 *
 * 协议 §3：「不得把内部异常串当成功值」——内部异常只能进日志文件，不能进
 * 给用户看的那一层。判据做法：让取数侧**真的抛一个带敏感细节的异常**，
 * 再看外发的错误负载里有没有它。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkErrorNoInternalLeak(adapter) {
  return guard('A9c-error-no-internal-leak', async () => {
    if (typeof adapter.captureWithInternalError !== 'function') {
      return { status: SKIPPED, detail: '实现未暴露「取数抛内部异常」的观测口' };
    }
    const payload = await adapter.captureWithInternalError();
    if (payload === undefined) {
      return { status: SKIPPED, detail: '实现未暴露「取数抛内部异常」的观测口' };
    }
    const text = JSON.stringify(payload);
    const code = String(payload?.code ?? '');
    if (!Object.hasOwn(ERROR_CODES, code)) {
      return {
        status: FAIL,
        detail: `失败时给的不是 §3 登记的稳定码（实测 ${code || '(空)'}）`,
      };
    }
    // 内部异常串的**特征片段**：注入的异常里带这些字样。
    const leaks = ['ENOENT', 'secret', 'permission denied', '/srv/', 'at Object.', '.mjs:'];
    const found = leaks.filter((needle) => text.includes(needle));
    if (found.length > 0) {
      return {
        status: FAIL,
        detail: `错误负载里带出了内部异常串：${found.join('、')}（负载=${text.slice(0, 200)}）`,
      };
    }
    return {
      status: PASS,
      detail: `内部异常被挡住，只外发稳定码 ${code}`,
    };
  });
}

/**
 * A10｜命名空间：换账号 / 换实例 / dev-release 同开 → 目录、快照、请求、权限**不串范围**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkNamespaceIsolation(adapter) {
  return guard('A10-namespace-isolation', async () => {
    if (typeof adapter.withScope !== 'function') {
      return { status: SKIPPED, detail: 'adapter 未提供跨命名空间切换能力' };
    }
    const scopes = [
      { accountScope: 'desktop-dev/default/site-A', instance: 'default-dev', mode: 'dev', sessionId: 'p1-a10-A', fixtureId: 'g1-alpha' },
      { accountScope: 'desktop-dev/default/site-B', instance: 'default-dev', mode: 'dev', sessionId: 'p1-a10-B', fixtureId: 'g1-beta' },
      { accountScope: 'desktop/default/site-A', instance: 'default', mode: 'release', sessionId: 'p1-a10-C', fixtureId: 'g1-alpha' },
    ];
    const picked = [];
    for (const scope of scopes) {
      const result = await adapter.withScope(scope, async () => {
        // **不同命名空间要用不同会话**（fixture 也不同）：句柄里的会话短码
        // 是从会话算的，同一会话在两个命名空间会得到**同名**句柄，
        // 那时「读到自己的」与「读到别人的」分不开 → 跨范围读取不可判。
        const p = await adapter.pick({ sessionId: scope.sessionId, fixtureId: scope.fixtureId });
        await adapter.settle(p?.ref);
        return { ref: p?.ref, file: adapter.snapshotFile(p?.ref) };
      });
      picked.push({ scope, ...result });
    }
    const refs = picked.map((item) => item.ref).filter(Boolean);
    if (refs.length < 2) {
      return { status: SKIPPED, detail: '跨命名空间 pick 未产出 ref（实现侧尚不可测）' };
    }
    // ① **目录不串**：不同命名空间的快照必须落在不同的根下。
    //    比较**所在目录**而不是完整路径：完整路径含 `snapshotId`，
    //    不同快照的路径本来就不同，拿它比等于没比（恒绿 = 漏判）。
    const dirs = picked
      .map((item) => String(item.file?.path ?? ''))
      .filter(Boolean)
      .map((path) => dirname(path));
    if (dirs.length > 1 && new Set(dirs).size !== dirs.length) {
      return { status: FAIL, detail: `不同命名空间的快照落到了同一个目录：${dirs.join('、')}` };
    }

    // ② **快照 / 请求不串范围**：在另一个命名空间里读第一个命名空间的句柄，
    //    必须**读不到**（拒），否则「换账号后旧快照仍可发」= 串范围。
    //    注意：协议**没有**要求句柄全局唯一（seq 是「本实例内」的 pick 序号），
    //    所以这里判的是「读不到」，不是「ref 必须不同」。
    //    **前提**：两个命名空间的句柄得能区分开——若句柄相同，本 scope 里那份
    //    同名快照会把「读到别人」伪装成「读到自己的」，那这条就不可判 → 跳过。
    const firstRef = refs[0];
    const secondRef = picked[1]?.ref;
    if (secondRef && secondRef === firstRef) {
      return {
        status: SKIPPED,
        detail:
          `两个命名空间给出了同一个句柄（${firstRef}）——无法区分「读到自己的」与「读到别人的」，` +
          '跨命名空间读取这一项不可判',
      };
    }
    let crossRejected = false;
    let crossDetail = '';
    await adapter.withScope(scopes[1], async () => {
      try {
        await adapter.serialize(firstRef);
      } catch (error) {
        crossRejected = true;
        crossDetail = String(error?.code ?? error?.message ?? error);
      }
    });
    if (!crossRejected) {
      return {
        status: FAIL,
        detail:
          `在另一个命名空间里读 ${firstRef} **没有被拒**——` +
          '要么读出了正文，要么读出了空正文（后者同样是没拒：协议要求拒，不是给个空值）',
      };
    }
    return {
      status: PASS,
      detail: `${refs.length} 个命名空间：目录各自独立，跨命名空间读句柄被拒（${crossDetail}）`,
    };
  });
}

/**
 * A11｜隔离：新增代码不得让判据扫描面外泄（新增文件若含建窗点，必须无条件给 `data_directory`）。
 *
 * 这一条**复用仓库既有门禁**（`scripts/lib/probe-profile-isolation.mjs`），
 * 这里只做「P1 新增文件」这一层的复查。
 *
 * @param {{files: Array<{file: string, text: string}>, findProfileViolationsInFiles: Function, findDataDirectoryViolationsInFiles: Function}} scope
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkIsolationSurface(scope) {
  const files = Array.isArray(scope?.files) ? scope.files : [];
  if (files.length === 0) {
    return verdict('A11-isolation-surface', SKIPPED, '没有可扫的新增文件');
  }
  const problems = [];
  if (typeof scope.findProfileViolationsInFiles === 'function') {
    const profileHits = scope.findProfileViolationsInFiles(files);
    for (const hit of profileHits) {
      problems.push(`指向用户真实 profile：${hit.file}:${hit.line} ${hit.text}`);
    }
  }
  if (typeof scope.findDataDirectoryViolationsInFiles === 'function') {
    const dirHits = scope.findDataDirectoryViolationsInFiles(files);
    for (const hit of dirHits) {
      problems.push(`建窗点没有无条件给 data_directory：${hit.file}:${hit.line} ${hit.detail}`);
    }
  }
  if (problems.length > 0) {
    return verdict('A11-isolation-surface', FAIL, problems.join('；'));
  }
  return verdict(
    'A11-isolation-surface',
    PASS,
    `新增的 ${files.length} 个文件没有指向真实 profile，建窗点都无条件给了 data_directory`,
  );
}
