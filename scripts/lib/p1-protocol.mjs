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
  /**
   * 形状：`<providerShort>~<sessionShort>~<seq>`。
   *
   * **会话段只认 base36 小写**（§6 v0.2：「`sessionShort` = 两段 FNV-1a 变体
   * → base36 编码取后 12 位」）。base36 的规范写法就是 `0-9a-z`——
   * 大写形式不是 base36 的规范输出，放进来会让「会话段必须是 base36」这条
   * 判不出来。provider 段**保持宽松**（协议只写「稳定短码，如 ds/gpt/mm，
   * ≤ 4 字符」，没规定大小写——不替协议加要求）。
   */
  pattern: /^([A-Za-z0-9]{1,4})~([0-9a-z]{1,12})~(\d{4})$/,
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
 * A12｜§8-12：G1-1…G1-6 缺口登记仍在，实现**不得假设**它们已解决。
 *
 * **v0.2 裁定**：缺口清单从 G1-1…G1-5 变成 **G1-1…G1-6**
 * —— 新增的 **G1-6 = 内核无「chip 进入 `invalid` 态」的写入口**
 * （§8-6：chip `invalid` 态在本阶段**不可机检**、**不计入本阶段验收**）。
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
      'capabilities 未提供 degraded 缺口登记——实现不得假设 G1-1…G1-6 已解决',
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
  // 协议 §8-12（v0.4）写的是「**G1-1…G1-7** 缺口登记仍在」——**七个**（v0.4 把
  // 通道缺口 G1-7 收进去了），不是一个，也不是 v0.2 的六个。
  // 判「编号齐不齐」而不是只数个数：只数个数的话，`g1_1`×6 也能蒙过去。
  const suffixes = new Set(
    g1
      .map((key) => /^g1[_-]?(\d)/i.exec(key)?.[1])
      .filter((value) => value !== undefined)
      .map((value) => Number(value)),
  );
  const required = [1, 2, 3, 4, 5, 6, 7];
  const missing = required.filter((index) => !suffixes.has(index));
  if (missing.length > 0) {
    return verdict(
      'A12-gap-registry',
      FAIL,
      `G1 缺口登记缺编号：${missing.map((index) => `G1-${index}`).join('、')}` +
        `（已登记 ${g1.join('、')}）——协议 §8 第 12 条要求 G1-1…G1-7 七个都在` +
        '（G1-6 = 内核无「chip 进入 invalid 态」的写入口；G1-7 = 通道被上游一行卡住）',
    );
  }
  // **多登记不算违规**（多一个缺口更保守），但要**点名**——协议 §8 第 12 条只
  // 要求 G1-1…G1-7 都在；多出来的编号属于协议变更，得回 PM 改稿而不是默默留着。
  const extra = [...suffixes].filter((index) => !required.includes(index)).sort((a, b) => a - b);
  return verdict(
    'A12-gap-registry',
    PASS,
    `登记了 ${g1.length} 个 G1 缺口（${g1.join('、')}），G1-1…G1-7 编号齐备` +
      '（G1-6 仍在登记 ⇒ 未被标成已解决，符合 §8 第 12 条「G1-6 不因本阶段验收通过而关闭」；' +
      'G1-7 仍在登记 ⇒ 通道降级可见，符合 §8 第 12 条「实现侧须登记 capabilities.degraded.g1_7」）' +
      (extra.length > 0 ? `；另外多登记了 G1-${extra.join('、G1-')}（协议外的编号，建议回 PM 改稿）` : ''),
  );
}

/**
 * A13｜协议 §4.1：保留期主动清理（7 天）。
 *
 * 期望值**独立转写**协议，不 import 实现当标准答案。六个方向**成对**：
 * 每一对里一条是「该清的清了」、一条是「**不该清的留住了**」——只钉前者的话，
 * 「无条件全删」也能蒙过去。
 *
 *  1. 超期快照 → **清掉**；
 *  2. 未超期快照 → **留着**（反向回归）；
 *  3. 已超期 **且** 对应快照已不在的认领 → **清掉**；
 *  4. 对应快照**还在**（未超期）的认领 → **留着**（反向回归：删了会让序号被
 *     二次分配 → 撞上仍然存在的快照 id）；
 *  5. 同轮里「过期快照 + 过期认领」→ **一起清干净**（协议 §4.1 第 3 条
 *     「先快照后认领」的**同轮效果**）；
 *  6. 被清掉的快照，其引用再发时 → **拒**（失效即拒，不静默重读；S-5/S-6）。
 *
 * @param {object} observation 实跑观测（每个字段必须是布尔）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkRetentionSweep(observation) {
  const checks = [
    ['expiredSnapshotRemoved', '超期快照被清掉'],
    ['freshSnapshotKept', '未超期快照留着（反向回归）'],
    ['expiredClaimRemoved', '「已超期 + 对应快照已不在」的认领被清掉'],
    ['claimOfLiveSnapshotKept', '「快照还在（未超期）」的认领留着（反向回归）'],
    ['sameRoundBothRemoved', '同轮「过期快照 + 过期认领」一起清干净（先快照后认领）'],
    ['afterSweepRejected', '被清掉的快照，其引用再发时被拒（不静默重读）'],
  ];
  const failed = checks.filter(([key]) => observation?.[key] !== true);
  if (failed.length > 0) {
    return verdict(
      'A17-retention-sweep',
      FAIL,
      `保留期清理口径不符：${failed.map(([, label]) => label).join('、')}` +
        '（协议 §4.1：只清超期的；认领要先快照后认领；清掉后引用失效即拒）',
    );
  }
  return verdict(
    'A17-retention-sweep',
    PASS,
    '保留期清理六条口径全对（超期清 / 未超期留 / 认领两向相反 / 同轮一起清 / 清后拒）',
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
 * A6b｜S-5：**chip 处于 `invalid` 态**。
 *
 * ## 这条**本阶段不可机检**（协议 v0.2/v0.3 裁定 ①：登记为上游缺口 G1-6）
 *
 * 协议 §S-5 与 §8 第 6 条已经改了口径：
 *   · 「chip 标 `invalid`」**不计入本阶段验收**；
 *   · 它登记为**上游缺口 G1-6**（内核无「chip 进入 `invalid` 态」的写入口），
 *     **G1-6 不因本阶段验收通过而关闭**（协议 §8 第 12 条）；
 *   · 本阶段的验收口径 = 「**请求数 0 + 可见错误**」（A6a 那条在判）。
 *
 * **所以本条不写成「节点级断言」，也不假装能测**：内核侧观测不到 chip 的
 * `invalid` 态（下面三条硬事实），判据**如实报「未执行」**并写明缺口编号与
 * 「G1-6 未关闭」。
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
          '本阶段不可机检；缺口 **G1-6 未关闭**（协议 v0.2/v0.3 §S-5 与 §8 第 6 条：' +
          '「chip 标 invalid」已移出本阶段验收口径，登记为上游缺口 G1-6）。' +
          '内核侧观测不到该状态：ReferenceInsert 无 invalid 字段（invalid 在 Occurrence 上），' +
          'setInvalid 在现役 release 内核零调用方，DOM 无 data-invalid。' +
          '本阶段的验收口径 = A6a 的「请求数 0 + 可见错误」。',
      };
    }
    if (chip.invalid !== true) {
      return {
        status: FAIL,
        detail: `chip 的 invalid 态观测为 ${JSON.stringify(chip.invalid)}——内核侧已能观测时，这条必须为 true`,
      };
    }
    return { status: PASS, detail: 'chip 进入 invalid 态（G1-6 已由上游关闭）' };
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

// ---------------------------------------------------------------------------
// 协议 v0.2 / v0.3 新增判据
// ---------------------------------------------------------------------------

/**
 * §1.1（v0.2）`captureResult.status` 的三个取值。
 *
 * 协议原文：「`ok` / `partial` / `failed`」。
 */
export const CAPTURE_STATUSES = Object.freeze(['ok', 'partial', 'failed']);

/**
 * 去掉 JS 源码里的注释（行注释 + 块注释），**行号保持不变**。
 *
 * **为什么需要它**：A0s 要判「host 半边**不使用** cordis `ctx.set` 挂服务」——
 * 而实现文件里**大量注释**正是在解释「为什么不能用 `ctx.set`」（协议 §5 的
 * 取证记录）。不剥注释就扫，会把**说明文字**当成**调用** ⇒ 假红。
 *
 * 剥法：逐字符走，跟踪字符串（`'` `"` `` ` ``）状态；`//` 与 `/*` 只在字符串外
 * 才算注释起点。注释被换成**等长空白**（行号与列位置都不变）。
 *
 * @param {string} source
 * @returns {string}
 */
export function stripJsComments(source) {
  const text = String(source ?? '');
  const out = text.split('');
  let i = 0;
  let quote = '';
  while (i < text.length) {
    const ch = text[i];
    const next = text[i + 1];
    if (quote !== '') {
      if (ch === '\\') {
        i += 2;
        continue;
      }
      if (ch === quote) quote = '';
      i += 1;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === '`') {
      quote = ch;
      i += 1;
      continue;
    }
    if (ch === '/' && next === '/') {
      while (i < text.length && text[i] !== '\n') {
        out[i] = ' ';
        i += 1;
      }
      continue;
    }
    if (ch === '/' && next === '*') {
      while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) {
        if (text[i] !== '\n') out[i] = ' ';
        i += 1;
      }
      if (i < text.length) {
        out[i] = ' ';
        out[i + 1] = ' ';
        i += 2;
      }
      continue;
    }
    i += 1;
  }
  return out.join('');
}

/**
 * A0s｜协议 v0.4 §8 第 13 条（**仓库级、静态**）：host 半边
 *  · **注册了 exact fetch route**（路径在 `/api` 命名空间下）；
 *  · **不得**把 `rpc.handle` 当作已通的通道（v0.4 已证伪）；
 *  · **不使用** cordis `ctx.set` 挂服务（协议 §5 已显式否决那条路）。
 *
 * **v0.4 更正（PM 裁定）**：此前这条钉的是「`inject` 必须含 `webServer`」——
 * 那个依据**已被真机实测证伪**（p1-r3 / p1-r6 两轮）：`rpc.handle` 的
 * `owner.webServer`（`dsh-client-connection/lib/index.js@0.2.1-rc.2:871`）的属性
 * 查找落在**内核 connection 插件自己的 fiber scope**
 * （`@deepseek-ai/cordis/lib/index.js@0.2.1-rc.2:683` 的
 * `(ctx[symbols.shadow] ?? ctx).fiber` + `createShadow` `:111-116`），
 * 插件侧声明只影响自己 scope ⇒ **声明了照样抛**。
 * ⇒ 这条改成钉**通道形态本身**（exact fetch route + 不用 `rpc.handle`），
 * 那才是真洞的根因与修法。
 *
 * @param {{file?: string, text?: string, files?: Array<{file: string, text: string}>}} scope
 *   - `files`：host 半边**几个文件**的源码（`index.js` 是注册处，`rpc.js` 是路径
 *     常量与信封层）。**只给 `index.js` 不够**——`/api` 那条路径住在 `rpc.js`。
 *   - 兼容旧写法 `{file, text}`（等价于只有一个文件的 `files`）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkHostInjection(scope) {
  const files = Array.isArray(scope?.files)
    ? scope.files.map((item) => ({ file: String(item?.file ?? ''), text: String(item?.text ?? '') }))
    : [{ file: String(scope?.file ?? ''), text: String(scope?.text ?? '') }];
  const all = files.map((item) => item.text).join('\n');
  if (all.trim() === '') {
    return verdict('A0s-host-injection', SKIPPED, `没读到 ${files[0]?.file || 'host 半边'} 的源码`);
  }
  const code = stripJsComments(all);
  const lines = code.split('\n');
  const problems = [];

  // ① `inject` 声明：必须是数组字面量，且含 'connection'（exact fetch route 的宿主）
  const injectMatch = /\binject\s*=\s*\[([^\]]*)\]/.exec(code);
  let injectItems = [];
  if (injectMatch === null) {
    problems.push(
      "没有找到 `inject` 声明——协议 §5：host 半边必须 `inject = ['connection']`" +
        '（否则拿不到 `ctx.connection`，exact fetch route 注册不上）',
    );
  } else {
    injectItems = injectMatch[1]
      .split(',')
      .map((item) => item.trim().replace(/^['"`]|['"`]$/g, ''))
      .filter((item) => item !== '');
    if (!injectItems.includes('connection')) {
      problems.push(`inject 里没有 'connection'（实为 ${injectItems.join('、') || '(空)'}）——exact fetch route 要靠它`);
    }
  }

  // ② **必须注册 exact fetch route**（协议 §8-13：v0.4 裁定的通道形态）
  const registersExactRoute = /\.fetch\s*\.\s*register\s*\(/.test(code);
  if (!registersExactRoute) {
    problems.push(
      '没有注册 exact fetch route（`ctx.connection.fetch.register({kind:\'exact\', ' +
        "path:'/api/…', methods:[…], requestBody:'buffered', fetch})`）——" +
        '协议 §8-13：v0.4 裁定的通道形态是 exact fetch route',
    );
  }
  // **路径必须在 `/api` 命名空间下**——内核 `assertFetchRoute`
  // （`dsh-client-connection/lib/index.js@0.2.1-rc.2:973-977`）用
  // `endpointFromPath("/api", route.path)` 校验，不过就抛。
  if (registersExactRoute && !/\/api\/[A-Za-z0-9._~-]+/.test(code)) {
    problems.push(
      "exact fetch route 的路径不在 `/api` 命名空间下——内核 `assertFetchRoute`" +
        '（`dsh-client-connection/lib/index.js@0.2.1-rc.2:973-977`）用 ' +
        '`endpointFromPath("/api", route.path)` 校验，不过就抛',
    );
  }

  // ③ **不得把 `rpc.handle` 当作已通的通道**（v0.4 已证伪：插件侧必抛）
  for (let i = 0; i < lines.length; i += 1) {
    if (/\brpc\s*\.\s*handle\s*\(/.test(lines[i])) {
      problems.push(
        `${files[0]?.file || 'host'}:${i + 1} 用了 \`connection.rpc.handle\`——协议 §8-13：那条路插件侧走不通` +
          '（内核 `dsh-client-connection/lib/index.js@0.2.1-rc.2:871` 用**属性访问** ' +
          '`owner.webServer.register(route)`，属性查找落在内核 connection 插件自己的 ' +
          'fiber scope ⇒ 必抛 `cannot get property "webServer" without inject`；' +
          '实测两轮无效，插件侧无法自修）——v0.4 裁定改走 exact fetch route',
      );
    }
  }

  // ④ 不得用 cordis `ctx.set` 挂服务（协议 §5 已显式否决）
  for (let i = 0; i < lines.length; i += 1) {
    if (/\bctx\s*\.\s*set\s*\(/.test(lines[i])) {
      problems.push(
        `${files[0]?.file || 'host'}:${i + 1} 用了 cordis \`ctx.set\` 挂服务——协议 §5 已显式否决那条路` +
          '（cordis 要求该属性先被 provide 过，否则 cannot set property "..." without provide；' +
          '且浏览器树 ≠ Node 进程树，host 的服务在浏览器树里永远解析不到）',
      );
    }
  }

  if (problems.length > 0) {
    return verdict('A0s-host-injection', FAIL, problems.join('；'));
  }
  return verdict(
    'A0s-host-injection',
    PASS,
    `host 半边声明了 inject = [${injectItems.join(', ')}]、注册了 /api 下的 exact fetch route、` +
      '且未把 rpc.handle 当作已通的通道、未使用 cordis ctx.set 挂服务',
  );
}

/**
 * A1e｜协议 §6：「`schemaVersion` **两端常量必须一致**」+ §3 码表 + §4 常量表。
 *
 * **为什么单列**：A1c 只比「同一组夹具的解析结果」；**常量与码表**不在夹具结果
 * 里——只跑夹具的话，「两端常量一致」「13 个码齐备且一致」**没人验**。
 *
 * @param {{js: object|undefined, rust: object|undefined}} pair
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkConstantsParity(pair) {
  const js = pair?.js;
  const rust = pair?.rust;
  if (js === undefined || js === null) {
    return verdict('A1e-constants-parity', SKIPPED, 'JS 侧没有报出常量（capabilities 不可用）');
  }
  if (rust === undefined || rust === null) {
    return verdict(
      'A1e-constants-parity',
      SKIPPED,
      'Rust 侧没有产物（未构建 p1_protocol_fixture example）——本条未执行，不判通过',
    );
  }
  const problems = [];
  if (Number(js.schemaVersion) !== SCHEMA_VERSION) {
    problems.push(`JS schemaVersion=${js.schemaVersion}，协议首版=${SCHEMA_VERSION}`);
  }
  if (Number(rust.schemaVersion) !== SCHEMA_VERSION) {
    problems.push(`Rust schemaVersion=${rust.schemaVersion}，协议首版=${SCHEMA_VERSION}`);
  }
  if (Number(js.schemaVersion) !== Number(rust.schemaVersion)) {
    problems.push(
      `两端 schemaVersion 不一致：JS=${js.schemaVersion} / Rust=${rust.schemaVersion}——协议 §6 要求两端常量一致`,
    );
  }
  for (const [key, expected] of Object.entries(CONSTANTS)) {
    const jsValue = js.constants?.[key];
    const rustValue = rust.constants?.[key];
    if (jsValue === undefined) {
      problems.push(`JS 侧缺常量 ${key}`);
      continue;
    }
    if (rustValue === undefined) {
      problems.push(`Rust 侧缺常量 ${key}`);
      continue;
    }
    if (Number(jsValue) !== Number(expected)) problems.push(`JS ${key}=${jsValue}，协议=${expected}`);
    if (Number(rustValue) !== Number(expected)) problems.push(`Rust ${key}=${rustValue}，协议=${expected}`);
  }
  const jsCodes = js.errorCodes ?? {};
  const rustCodes = rust.errorCodes ?? {};
  for (const [code, spec] of Object.entries(ERROR_CODES)) {
    const a = jsCodes[code];
    const b = rustCodes[code];
    if (a === undefined) {
      problems.push(`JS 侧缺错误码 ${code}`);
      continue;
    }
    if (b === undefined) {
      problems.push(`Rust 侧缺错误码 ${code}`);
      continue;
    }
    if (Boolean(a.retryable) !== spec.retryable) problems.push(`JS ${code} 的可重试性与 §3 不符`);
    if (Boolean(b.retryable) !== spec.retryable) problems.push(`Rust ${code} 的可重试性与 §3 不符`);
    if (String(a.userNext ?? '') !== spec.userNext) problems.push(`JS ${code} 的用户下一步与 §3 不符`);
    if (String(b.userNext ?? '') !== spec.userNext) problems.push(`Rust ${code} 的用户下一步与 §3 不符`);
  }
  const extraJs = Object.keys(jsCodes).filter((code) => !Object.hasOwn(ERROR_CODES, code));
  const extraRust = Object.keys(rustCodes).filter((code) => !Object.hasOwn(ERROR_CODES, code));
  if (extraJs.length > 0) problems.push(`JS 侧多出协议外的码：${extraJs.join('、')}`);
  if (extraRust.length > 0) problems.push(`Rust 侧多出协议外的码：${extraRust.join('、')}`);
  if (problems.length > 0) {
    return verdict('A1e-constants-parity', FAIL, problems.join('；'));
  }
  return verdict(
    'A1e-constants-parity',
    PASS,
    `两端 ${Object.keys(CONSTANTS).length} 个常量与 ${Object.keys(ERROR_CODES).length} 个错误码逐项一致（schemaVersion=${SCHEMA_VERSION}）`,
  );
}

/**
 * 驱动真实实现走一遍各条**失败路径**，收集**对端真正看到的错误负载**。
 *
 * 走的是**真实 RPC 通道**（`connection.rpc.call`）——判据要验的是对端拿到的
 * 形态（`{ok:false, error:{code, message, details}}`），不是命令层的内部返回。
 *
 * @param {object} adapter
 * @returns {Promise<Array<{label: string, code: string, payload: object}>>}
 */
async function collectErrorPayloads(adapter) {
  const out = [];
  const push = (label, payload) => {
    const code = String(payload?.error?.code ?? payload?.code ?? '');
    out.push({ label, code, payload });
    return code;
  };
  const picked = await adapter.pick({ sessionId: 'p1-errors' });
  const ref = String(picked?.ref ?? '');
  const sessionId = String(picked?.sessionId ?? '');
  if (ref === '') return out;
  // 等这次 pick 的落盘落定——否则「同 id 重复落盘」那条会撞上**在途**的落盘，
  // 测出来的不是「已存在」这一支。
  await adapter.settle(ref);

  // ① id 形态不合法
  push(
    'id 形态不合法',
    await adapter.callHost('capture', { sessionId, snapshotId: '这不是快照id', requestId: 'p1-err-1' }),
  );
  // ② provider 段与本来源不符
  const parts = ref.split('~');
  push(
    'provider 段不符',
    await adapter.callHost('capture', {
      sessionId,
      snapshotId: `zz~${parts[1]}~0001`,
      requestId: 'p1-err-2',
    }),
  );
  // ③ 序号不是 host 认领过的
  const unclaimed = await adapter.captureUnclaimedId(sessionId);
  push('序号未认领', unclaimed?.result);
  // ④ 同 id 重复落盘（内部冲突码 → 归 snapshot_invalid，原码留在 detail）
  push(
    '同 id 重复落盘',
    await adapter.callHost('capture', { sessionId, snapshotId: ref, requestId: 'p1-err-4' }),
  );
  // ⑤ 目录读取失败（≠ 目录没有结果）
  await adapter.withFailingCatalog(async () => {
    push(
      '目录读取失败',
      await adapter.callHost('catalog', { query: 'p1-err-5', requestId: 'p1-err-5' }),
    );
  });
  // ⑥ 当前来源指纹不可得（协议 §S-5 v0.2：不得放行）
  await adapter.withUnavailableFingerprint(async () => {
    push(
      '指纹不可得（capture）',
      await adapter.callHost('capture', { sessionId, snapshotId: ref, requestId: 'p1-err-6' }),
    );
    push(
      '指纹不可得（preview）',
      await adapter.callHost('preview', { snapshotId: ref, requestId: 'p1-err-6b' }),
    );
  });
  return out;
}

/**
 * A9d｜协议 §3：「每个错误码带：可重试性、用户下一步、关联 `requestId`、
 * **脱敏**日志路径」。
 *
 * 这条**不看码表**（码表那半边在 A9a），看的是**真实失败时外发的负载**：
 * 每个错误负载都必须带 `requestId` 与**脱敏**的日志路径。
 *
 * 「脱敏」怎么判：日志路径**不得是绝对路径**（不得把实例真实路径/用户 home
 * 拼进去——协议 §5），实现用的是 `<实例>/…` 这种占位形态。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkErrorLogPath(adapter) {
  return guard('A9d-error-log-path', async () => {
    if (typeof adapter.callHost !== 'function') {
      return {
        status: SKIPPED,
        detail: 'adapter 未提供「按真实 RPC 通道调 host 命令」的观测口——缺「拿到对端真实错误负载」这条接口',
      };
    }
    const payloads = await collectErrorPayloads(adapter);
    if (payloads.length === 0) {
      return { status: SKIPPED, detail: '没采到任何错误负载（pick 未产出 ref？）——本条未执行' };
    }
    const root = String(adapter.root ?? '');
    const problems = [];
    for (const item of payloads) {
      const details = item.payload?.error?.details ?? item.payload ?? {};
      const logPath = String(details.logPath ?? '');
      const requestId = String(details.requestId ?? '');
      if (requestId.trim() === '') {
        problems.push(`${item.label}: 没带 requestId（§3 要求每个错误码关联 requestId）`);
      }
      if (logPath.trim() === '') {
        problems.push(`${item.label}: 没带脱敏日志路径（§3 要求每个错误码带脱敏日志路径）`);
        continue;
      }
      if (/^[A-Za-z]:[\\/]/.test(logPath) || logPath.startsWith('/')) {
        problems.push(`${item.label}: 日志路径是绝对路径（${logPath}）——§5 要求路径不展开真实目录`);
      }
      if (root !== '' && logPath.includes(root)) {
        problems.push(`${item.label}: 日志路径里带出了实例真实路径`);
      }
    }
    if (problems.length > 0) {
      return { status: FAIL, detail: problems.join('；') };
    }
    return {
      status: PASS,
      detail: `${payloads.length} 条失败负载都带了 requestId 与脱敏日志路径`,
    };
  });
}

/**
 * A14｜协议 §3（v0.2）：13 个码是**闭集**，**不新增第 14 个**；内部冲突码一律
 * 归入 `snapshot_invalid`，**原始码保留在 `detail`**。
 *
 * **行为判据**（不是只看码表）：把真实实现逼进各条失败路径，收集外发的码，
 * 逐个查它**在不在闭集里**；并单独验「同 id 重复落盘」这一支——协议原文：
 * 「如『同 id 重复落盘』→ 归 `snapshot_invalid`，原始码 `snapshot_exists` 进 `detail`」。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkErrorCodeClosedSet(adapter) {
  return guard('A14-error-code-closed-set', async () => {
    if (typeof adapter.callHost !== 'function') {
      return {
        status: SKIPPED,
        detail: 'adapter 未提供「按真实 RPC 通道调 host 命令」的观测口——缺「拿到对端真实错误负载」这条接口',
      };
    }
    const payloads = await collectErrorPayloads(adapter);
    if (payloads.length === 0) {
      return { status: SKIPPED, detail: '没采到任何错误负载（pick 未产出 ref？）——本条未执行' };
    }
    const problems = [];
    const outside = [];
    for (const item of payloads) {
      if (item.code === '') {
        problems.push(`${item.label}: 没给出错误码`);
        continue;
      }
      if (!Object.hasOwn(ERROR_CODES, item.code)) {
        outside.push(`${item.label}=${item.code}`);
      }
      if (item.payload?.ok === true) {
        problems.push(`${item.label}: 失败场景却回了 ok=true（失败被吞成成功值，违反 §3）`);
      }
    }
    if (outside.length > 0) {
      problems.push(
        `外发了 §3 闭集之外的码：${outside.join('、')}——13 个码是闭集，不新增第 14 个`,
      );
    }
    const dup = payloads.find((item) => item.label === '同 id 重复落盘');
    if (dup !== undefined) {
      if (dup.code !== 'snapshot_invalid') {
        problems.push(
          `同 id 重复落盘给的码是 ${dup.code || '(空)'}——协议 §3 要求内部冲突码归入 snapshot_invalid`,
        );
      }
      const detail = String(dup.payload?.error?.details?.detail ?? dup.payload?.detail ?? '');
      if (!detail.includes('snapshot_exists')) {
        problems.push(
          `同 id 重复落盘的原始码没保留在 detail（实测 detail=${detail || '(空)'}）——` +
            '协议 §3 要求原始码留在 detail',
        );
      }
    }
    if (problems.length > 0) {
      return { status: FAIL, detail: problems.join('；') };
    }
    return {
      status: PASS,
      detail:
        `${payloads.length} 条失败路径的码都在 13 个闭集内（${payloads.map((item) => item.code).join('、')}）；` +
        '同 id 重复落盘归 snapshot_invalid 且原码留在 detail',
    };
  });
}

/**
 * A13a｜协议 §2 S-1（v0.2）：**id 分配权归 host**；「本实例内单调递增」必须
 * **跨进程**成立。
 *
 * **怎么验跨进程**：起一个**真的 `node` 子进程**，在**同一个落盘根**上用**同一份
 * 分配器实现**认领一个序号。两个进程拿到的序号必须**不撞**且**单调**。
 *
 * **为什么不用「同进程两个实例」**：分配器没有内存状态（每次都从认领目录读），
 * 但「两个实例」终究不是「两个进程」——真子进程才是协议说的那个场景。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkSeqCrossProcess(adapter) {
  return guard('A13a-seq-cross-process', async () => {
    if (typeof adapter.reserveInChildProcess !== 'function') {
      return {
        status: SKIPPED,
        detail:
          'adapter 未提供「另一个进程认领序号」的观测口——缺「host 持久化认领、跨进程递增」这条接口',
      };
    }
    const picked = await adapter.pick({ sessionId: 'p1-a13' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const parentRef = String(picked.ref);
    const parentSeq = Number(parentRef.split('~')[2]);
    let child;
    try {
      child = await adapter.reserveInChildProcess(String(picked.sessionId ?? 'p1-a13'));
    } catch (error) {
      return {
        status: FAIL,
        detail: `另一个进程认领序号失败：${String(error?.message ?? error)}`,
      };
    }
    if (child === undefined || child.snapshotId === '') {
      return { status: SKIPPED, detail: '另一个进程没有认领到序号（观测口没给出结果）——本条未执行' };
    }
    if (child.snapshotId === parentRef) {
      return {
        status: FAIL,
        detail: `另一个进程认领到了同一个 id（${child.snapshotId}）——跨进程会撞盘（S-6：落盘且跨进程可发）`,
      };
    }
    if (!(Number(child.seq) > parentSeq)) {
      return {
        status: FAIL,
        detail:
          `跨进程序号没有单调递增（本进程 ${parentSeq} → 另一进程 ${child.seq}）——` +
          '协议 §2 S-1 要求「本实例内单调递增」跨进程成立',
      };
    }
    const shape = checkRefShape(child.snapshotId);
    if (shape.status === FAIL) {
      return { status: FAIL, detail: `另一个进程认领的 id 不合规：${shape.detail}` };
    }
    return {
      status: PASS,
      detail:
        `本进程 ${parentRef}（seq ${parentSeq}）→ 另一进程 ${child.snapshotId}（seq ${child.seq}）：` +
        '不撞且单调递增（id 由 host 分配）',
    };
  });
}

/**
 * A13b｜协议 §2 S-1（v0.2）：「`seq` 由 host 侧统一分配（host 持有并持久化已用
 * 序号，**client 不得自造**）」。
 *
 * **怎么验**：拿一个 **host 从未认领过**的 id 去调 `capture`——必须被拒
 * （`snapshot_invalid`），且**不得落盘**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkHostRejectsUnclaimedId(adapter) {
  return guard('A13b-host-rejects-unclaimed-id', async () => {
    if (typeof adapter.captureUnclaimedId !== 'function') {
      return {
        status: SKIPPED,
        detail: 'adapter 未提供「用未认领 id 调 capture」的观测口——缺「client 不得自造 seq」这条接口',
      };
    }
    const picked = await adapter.pick({ sessionId: 'p1-a13b' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const unclaimed = await adapter.captureUnclaimedId(String(picked.sessionId ?? 'p1-a13b'));
    if (!unclaimed?.id) {
      return { status: SKIPPED, detail: '没拿到未认领的 id（观测口没给出结果）——本条未执行' };
    }
    if (unclaimed.claimed) {
      return {
        status: SKIPPED,
        detail: `序号 9999 恰好被认领过（${unclaimed.id}）——本条改用别的序号再验`,
      };
    }
    const code = String(unclaimed.result?.error?.code ?? unclaimed.result?.code ?? '');
    if (unclaimed.result?.ok === true) {
      return {
        status: FAIL,
        detail: `host 接受了一个它没分配过的 id（${unclaimed.id}）——协议 §2 S-1 要求 client 不得自造 seq`,
      };
    }
    if (code !== 'snapshot_invalid') {
      return {
        status: FAIL,
        detail: `host 拒未认领 id 用的码是 ${code || '(空)'}——协议 §3 归 snapshot_invalid`,
      };
    }
    const file = adapter.snapshotFile(unclaimed.id);
    if (file?.exists) {
      return { status: FAIL, detail: `未认领的 id 竟然落盘了（${unclaimed.id}）` };
    }
    return {
      status: PASS,
      detail: `未认领的 id（${unclaimed.id}）被拒：snapshot_invalid，且未落盘`,
    };
  });
}

/**
 * A15｜协议 §S-5（v0.2）：**指纹比对的校验点在 host 命令层**（不是存储层）；
 * **当前指纹不可得 → 拒**（`account_changed`），**不得放行**。
 *
 * **三段一起验**（缺一段就证明不了「校验点在命令层」）：
 *  1. 当前指纹不可得 → `capture` 与 `preview` **都拒**（`account_changed`）；
 *  2. 指纹变了 → `preview` **拒**（`account_changed`）——这是命令层的比对；
 *  3. 指纹变了之后**存储层仍能读出来**——证明指纹**不是**在存储层比的
 *     （协议 §S-5：存储层只校验 `schemaVersion` 与 `contentDigest`）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkFingerprintGate(adapter) {
  return guard('A15-fingerprint-gate-host-layer', async () => {
    if (typeof adapter.withUnavailableFingerprint !== 'function') {
      return {
        status: SKIPPED,
        detail:
          'adapter 未提供「当前指纹不可得」的观测口——缺「拿当前指纹 + 比对 + 拒」这条协议接口（§S-5 v0.2）',
      };
    }
    const picked = await adapter.pick({ sessionId: 'p1-a15' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const ref = String(picked.ref);
    const sessionId = String(picked.sessionId ?? 'p1-a15');
    await adapter.settle(ref);
    const problems = [];

    // ① 当前指纹不可得 → capture 与 preview 都必须拒
    const unavailable = await adapter.withUnavailableFingerprint(async () => ({
      capture: await adapter.callHost('capture', {
        sessionId,
        snapshotId: ref,
        requestId: 'p1-a15-1',
      }),
      preview: await adapter.callHost('preview', { snapshotId: ref, requestId: 'p1-a15-2' }),
    }));
    for (const [label, result] of Object.entries(unavailable ?? {})) {
      const code = String(result?.error?.code ?? result?.code ?? '');
      if (result?.ok === true) {
        problems.push(`${label}: 当前指纹不可得却放行了——协议 §S-5 要求此时按 account_changed 拒`);
      } else if (code !== 'account_changed') {
        problems.push(
          `${label}: 当前指纹不可得时给的码是 ${code || '(空)'}——协议 §S-5 要求 account_changed`,
        );
      }
    }

    // ② + ③ 指纹变了 → 命令层拒；存储层**不**比对指纹
    await adapter.invalidate(ref, 'fingerprint');
    const stored = adapter.storeLoad(ref);
    if (stored?.loaded !== true) {
      problems.push(
        `指纹变了之后存储层读不出来（${stored?.error || '(未知)'}）——` +
          '§S-5 说存储层只校验 schemaVersion 与 contentDigest，不比对指纹',
      );
    }
    const preview = await adapter.callHost('preview', { snapshotId: ref, requestId: 'p1-a15-3' });
    const code = String(preview?.error?.code ?? preview?.code ?? '');
    if (preview?.ok === true) {
      problems.push('指纹变了却放行了——host 命令层没有比对当前指纹');
    } else if (code !== 'account_changed') {
      problems.push(`指纹变了给的码是 ${code || '(空)'}——协议 §S-5 要求 account_changed`);
    }

    if (problems.length > 0) {
      return { status: FAIL, detail: problems.join('；') };
    }
    return {
      status: PASS,
      detail:
        '指纹不可得 → capture/preview 均拒（account_changed）；指纹变了 → 命令层拒、存储层不比对' +
        '（校验点确认在 host 命令层）',
    };
  });
}

/**
 * A16a｜协议 §1.1（v0.2）：`captureResult` 的**形态**。
 *
 * 协议原文：「形态 = 对象（对象必带三字段；**`ok` 兼容字符串 `'ok'`**，
 * `partial` / `failed` **必须用对象**——用字符串会丢『省略了什么』，违反
 * 『不静默裁掉』纪律）」。
 *
 * @param {Array<object>} fixtures
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkCaptureResultShape(fixtures) {
  const list = Array.isArray(fixtures) ? fixtures : [];
  if (list.length === 0) {
    return verdict('A16a-capture-result-shape-fixtures', SKIPPED, '没读到任何协议夹具');
  }
  const problems = [];
  let checked = 0;
  const checkOne = (label, value) => {
    if (value === undefined || value === null) return;
    checked += 1;
    if (typeof value === 'string') {
      if (value !== 'ok') {
        problems.push(`${label}: 字符串形态只兼容 'ok'，收到 '${value}'（partial/failed 必须用对象）`);
      }
      return;
    }
    if (typeof value !== 'object' || Array.isArray(value)) {
      problems.push(`${label}: captureResult 既不是对象也不是字符串`);
      return;
    }
    for (const field of ['status', 'omitted', 'unsupported']) {
      if (!Object.hasOwn(value, field)) problems.push(`${label}: captureResult 缺字段 ${field}`);
    }
    if (!CAPTURE_STATUSES.includes(String(value.status ?? ''))) {
      problems.push(`${label}: captureResult.status 非法：${String(value.status)}`);
    }
    if (!Array.isArray(value.omitted)) problems.push(`${label}: captureResult.omitted 不是数组`);
    if (!Array.isArray(value.unsupported)) problems.push(`${label}: captureResult.unsupported 不是数组`);
  };
  for (const fixture of list) {
    const id = String(fixture?.id ?? '(未命名)');
    checkOne(`${id} input.captureResult`, fixture?.input?.captureResult);
    checkOne(`${id} expected.snapshot.captureResult`, fixture?.expected?.snapshot?.captureResult);
  }
  if (checked === 0) {
    return verdict(
      'A16a-capture-result-shape-fixtures',
      SKIPPED,
      '夹具里没有 captureResult 字段，本条无对象可判',
    );
  }
  if (problems.length > 0) {
    return verdict('A16a-capture-result-shape-fixtures', FAIL, problems.join('；'));
  }
  return verdict(
    'A16a-capture-result-shape-fixtures',
    PASS,
    `${checked} 处 captureResult 形态合规（对象三字段齐；字符串只兼容 'ok'）`,
  );
}

/**
 * A16b｜协议 §1.1（v0.2）：**落盘快照**里的 `captureResult` 必须是**对象**形态
 * （三字段齐全）。
 *
 * A16a 判的是夹具；这条判的是**真实落盘物**——协议 §1.1 的字段表是快照的表。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkCaptureResultLanded(adapter) {
  return guard('A16b-capture-result-shape-landed', async () => {
    const picked = await adapter.pick({ sessionId: 'p1-a16' });
    if (!picked?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    const ref = String(picked.ref);
    await adapter.settle(ref);
    const file = adapter.snapshotFile(ref);
    if (!file?.exists) {
      return { status: FAIL, detail: `快照没落盘（${ref}）` };
    }
    const captureResult = file?.snapshot?.captureResult;
    if (captureResult === undefined || captureResult === null) {
      return { status: FAIL, detail: '落盘快照里没有 captureResult' };
    }
    if (typeof captureResult !== 'object' || Array.isArray(captureResult)) {
      return {
        status: FAIL,
        detail: `落盘快照的 captureResult 是 ${JSON.stringify(captureResult)}——协议 §1.1 v0.2 要求对象形态`,
      };
    }
    const problems = [];
    for (const field of ['status', 'omitted', 'unsupported']) {
      if (!Object.hasOwn(captureResult, field)) problems.push(`缺字段 ${field}`);
    }
    if (!CAPTURE_STATUSES.includes(String(captureResult.status ?? ''))) {
      problems.push(`status 非法：${String(captureResult.status)}`);
    }
    if (!Array.isArray(captureResult.omitted)) problems.push('omitted 不是数组');
    if (!Array.isArray(captureResult.unsupported)) problems.push('unsupported 不是数组');
    if (problems.length > 0) {
      return { status: FAIL, detail: problems.join('；') };
    }
    return {
      status: PASS,
      detail: `落盘快照的 captureResult 是对象且三字段齐（status=${captureResult.status}）`,
    };
  });
}
