/**
 * official-chat 的**协议核心**（P1 冻结稿 v0.1 的 JS 实现）。
 *
 * ## 它是什么
 *
 * PM 定的协议是 `docs/features/official-chat/official-chat-p1-protocol.md` v0.1
 * （**PM 归属文件，本文件不定义协议，只实现它**；协议与实现冲突时先回报 PM）。
 * 本文件把协议里**不依赖具体站点、不依赖具体桥**的那一半落成可测的纯逻辑：
 *
 *   · §1.1 固定快照的数据模型（字段表逐字段落地）
 *   · §1.2 目录页（**独立缓存物**，不含正文）
 *   · §1.3 规范化与 `contentDigest` 口径
 *   · §2   S-1…S-7 里**只依赖数据**的那几条（单一 id 制度、状态机派生、失效判定）
 *   · §3   14 个稳定错误码（可重试性 / 用户下一步 / requestId / 脱敏日志路径）
 *   · §4   首版常量
 *
 * ## 为什么这份逻辑住在 host 半边
 *
 * 插件的 client 半边**必须是单文件自包含**（`client/client.js` 文件头：
 * 「多一个同级文件就是 404」——它是浏览器里的普通脚本，没有模块解析器）。
 * 于是协议核心只有两个落点：host 半边（Node，可 import）与 client 半边内联。
 * 为了不让「同一份规则写两遍、两遍漂掉」，本文件是**唯一可 import 的那一份**，
 * client 半边内联的那份由 `test/protocol.test.mjs` **断言两者输出一致**
 * （不靠人读代码，靠机检比对数据——本仓栽过「判据只判代码长得像不像」的坑）。
 *
 * ## 跨语言一致性
 *
 * §6 要求「同一组 JSON 夹具，Rust 与 JavaScript 各跑一遍」。Rust 那半在
 * `src-tauri/src/official_chat/protocol.rs`，读的是**同一份**
 * `plugins/official-chat/fixtures/protocol/` 夹具。**两边都不许私改口径**——
 * 摘要算法、规范化规则、短码算法、schemaVersion 常量都必须逐字一致。
 *
 * @module xlink-official-chat/protocol
 */

import { createHash } from 'node:crypto';

/** §1.1 快照格式版本。**变更即升版**；旧版快照按 P1-07 拒（`schema_changed`）。 */
export const SCHEMA_VERSION = 1;

/**
 * §3 的 14 个稳定错误码。
 *
 * 每个码带：**可重试性**、**用户下一步**、以及（构造错误负载时的）**脱敏日志路径**。
 *
 * **错误里不得出现内部异常串**（协议 §3）：所以 `detail` 一律不接收原始
 * `error.message`——调用方只能传**已经分类好的**信息，内部异常只能进日志文件
 * （由 `logPath` 指过去），不能进给用户看的那一层。
 */
export const ERROR_CODES = {
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
  body_budget_exceeded: { retryable: false, userNext: '选一段更短的正文' },
};

/** 全部错误码的稳定名字（顺序固定，供夹具与机检遍历）。 */
export const ERROR_CODE_NAMES = Object.freeze([
  'not_logged_in',
  'account_changed',
  'source_unsupported',
  'schema_changed',
  'partial_content',
  'body_too_large',
  'timeout',
  'cancelled',
  'snapshot_invalid',
  'kernel_incompatible',
  'catalog_empty',
  'catalog_failed',
  'stale_draft',
  'body_budget_exceeded',
]);

/**
 * 捕获失败的**原因 → 协议码**映射（协议 v0.3 §3「各码的使用边界」）。
 *
 * **捕获失败不是一个单一原因**，必须**按原因分流**：
 * · 超时 → `timeout`（**可重试**）；
 * · 官网页面结构变了 → `schema_changed`（升级插件）；
 * · 用户取消 → `cancelled`（重新选择）。
 *
 * **`source_unsupported` 只用于「这个来源根本不支持该能力」**——
 * 它**不得**兜「捕获失败」：那会让用户看到「换一个来源」而其实该**重试**，
 * 属于**掩盖真实原因**（协议 v0.3 §3 明写）。
 *
 * **原因不明 → `timeout` 兜**（可重试），`detail` 写脱敏线索。
 *
 * **这张表是闭集**：不在表里的 reason 一律当「原因不明」处理，**不外发**
 * ——免得把内部异常串当原因线索漏出去。
 */
export const CAPTURE_FAILURE_REASONS = Object.freeze({
  // 超时（可重试）
  timeout: 'timeout',
  timed_out: 'timeout',
  deadline_exceeded: 'timeout',
  // 官网页面结构变了（认不出该认的元素）
  schema_changed: 'schema_changed',
  structure_changed: 'schema_changed',
  unrecognized_selector: 'schema_changed',
  // 用户取消
  cancelled: 'cancelled',
  canceled: 'cancelled',
  user_cancelled: 'cancelled',
  // **唯一**允许用 `source_unsupported` 的场景：来源明说它不支持这个能力。
  source_unsupported: 'source_unsupported',
  capability_unsupported: 'source_unsupported',
});

/**
 * 把捕获源报的原因映射成协议码。
 *
 * @param {unknown} reason - 原因词（稳定 token，不是异常串）。
 * @returns {string} §3 的稳定码名；**原因不明时是 `timeout`**（可重试）。
 */
export function captureFailureCode(reason) {
  const raw = String(reason ?? '').trim().toLowerCase();
  if (raw === '') return 'timeout';
  // 已经是协议码 → 原样（闭集内，不会漏出第 15 个码）。
  if (ERROR_CODE_NAMES.includes(raw)) return raw;
  return CAPTURE_FAILURE_REASONS[raw] ?? 'timeout';
}

/**
 * 捕获失败的**脱敏线索**（进 `detail`）。
 *
 * **只带闭集内的原因词**：不认识的 reason 一律给「原因未明」，
 * **绝不把来源抛出来的异常串外发**（协议 §3：不得把内部异常串当成功值外发）。
 *
 * @param {unknown} reason - 原因词。
 * @returns {string} 脱敏线索。
 */
export function captureFailureDetail(reason) {
  const raw = String(reason ?? '').trim().toLowerCase();
  const known =
    raw !== '' &&
    (ERROR_CODE_NAMES.includes(raw) || CAPTURE_FAILURE_REASONS[raw] !== undefined);
  return known ? `捕获失败原因：${raw}` : '捕获失败，原因未明';
}

/**
 * §4 首版常量。
 *
 * 协议明写「**工程参数**，不伪称来自站点限制」——所以这里每个值都注明它是
 * 首版取值，不是站点上限。改动要升 `SCHEMA_VERSION` 吗？**不用**：常量不是
 * 快照格式的一部分；但**夹具会引用它们**，改了常量要同步改夹具期望值。
 */
export const CONSTANTS = Object.freeze({
  /** 目录页大小：50 条 / 页。 */
  catalogPageSize: 50,
  /** 目录缓存期限：5 分钟（换账号即失效）。 */
  catalogCacheTtlMs: 5 * 60 * 1000,
  /** 搜索去抖：300 ms。 */
  searchDebounceMs: 300,
  /** 读取超时：30 s（G1 桩模型门控实测到 ~8 s，留余量）。 */
  readTimeoutMs: 30_000,
  /** 最大正文字节：256 KB（超过 → `body_too_large`）。 */
  maxBodyBytes: 256 * 1024,
  /** 最大响应字节：1 MB。 */
  maxResponseBytes: 1024 * 1024,
  /** 快照保留期：7 天（超期按 `snapshot_invalid` 拒）。 */
  snapshotRetentionMs: 7 * 24 * 60 * 60 * 1000,
  /**
   * **临时文件残留清理阈值**：24 小时（协议 §4 的 `tmpResidualMs`）。
   *
   * **不与 7 天保留期混用**：临时文件**不是快照**（§2.3）⇒ 它的阈值回答的是
   * 「崩溃残留多久算残留」，不是「快照留多久」⇒ 24 小时足够（临时文件的
   * 生命周期只有一次发布，发布成功即消失）。
   */
  tmpResidualMs: 24 * 60 * 60 * 1000,
  /**
   * **占用保留期**：7 天（协议 §4 的 `occupancyRetentionMs`）。
   *
   * **它回答的是「占用关系留多久」**，不是「快照留多久」——但首版取**同值**
   * （协议 §4 的表：与 `snapshotRetentionMs` 同值）。
   *
   * **谁用它**：占用记录 `refs/` 的**兜底释放**（R2）——超过这个期限的占用
   * 记录视为失效并删（**由清理流程负责**，不是释放入口负责）。
   */
  occupancyRetentionMs: 7 * 24 * 60 * 60 * 1000,
  /** 缓存总量上限：512 MB / 实例（含目录缓存与快照）。 */
  cacheTotalBudgetBytes: 512 * 1024 * 1024,
  /** 单消息引用数上限：8 条。 */
  maxReferencesPerMessage: 8,
  /** 单条正文预算：64 KB（进入模型输入的**单条**上限）。 */
  perBodyBudgetBytes: 64 * 1024,
  /** 单次消息总预算：256 KB（**含原生 `@session`**，自有来源不得绕过总预算）。 */
  perMessageBudgetBytes: 256 * 1024,
  /** 快照格式版本（与 §1.1 的 `schemaVersion` 同值）。 */
  schemaVersion: SCHEMA_VERSION,
});

/**
 * §2 S-2 状态机。
 *
 * ```text
 * selected → reading → validating → snapshot_saved → ready
 *      reading/validating → failed 或 cancelled
 *      任意未完成状态 → stale
 * ready → serialized → expanded → persisted
 * ```
 *
 * **只有 `ready` 可形成就绪 chip**（HC-3）：pending 句柄在 label、详情、
 * 任何文案里都**不得**被称为就绪。
 */
export const STATES = Object.freeze({
  selected: 'selected',
  reading: 'reading',
  validating: 'validating',
  snapshotSaved: 'snapshot_saved',
  ready: 'ready',
  failed: 'failed',
  cancelled: 'cancelled',
  stale: 'stale',
  serialized: 'serialized',
  expanded: 'expanded',
  persisted: 'persisted',
});

/** 状态机里「未完成」的状态集合（S-2：这些状态下不得形成就绪 chip）。 */
export const IN_FLIGHT_STATES = Object.freeze([
  STATES.selected,
  STATES.reading,
  STATES.validating,
  STATES.snapshotSaved,
]);

/** 终态集合。 */
export const TERMINAL_STATES = Object.freeze([
  STATES.ready,
  STATES.failed,
  STATES.cancelled,
  STATES.stale,
]);

/** 合法状态转移表（S-2 的箭头图，逐条照抄）。 */
export const TRANSITIONS = Object.freeze({
  [STATES.selected]: [STATES.reading, STATES.cancelled, STATES.stale],
  [STATES.reading]: [STATES.validating, STATES.failed, STATES.cancelled, STATES.stale],
  [STATES.validating]: [STATES.snapshotSaved, STATES.failed, STATES.cancelled, STATES.stale],
  [STATES.snapshotSaved]: [STATES.ready, STATES.failed, STATES.cancelled, STATES.stale],
  [STATES.ready]: [STATES.serialized],
  [STATES.serialized]: [STATES.expanded],
  [STATES.expanded]: [STATES.persisted],
  [STATES.failed]: [],
  [STATES.cancelled]: [],
  [STATES.stale]: [],
});

/**
 * 转移是否合法。
 *
 * @param {string} from - 起始状态。
 * @param {string} to - 目标状态。
 * @returns {boolean}
 */
export function canTransition(from, to) {
  const allowed = TRANSITIONS[from];
  return Array.isArray(allowed) && allowed.includes(to);
}

/**
 * §1.3 规范化。
 *
 * 规则（协议原文）：行尾统一 `\n`、去 BOM、去尾随空白、**保留正文内空行与
 * 缩进**（不得改动语义）、角色名小写枚举。
 *
 * **「去尾随空白」只去整段末尾，不逐行去**——理由：逐行去会改掉 Markdown 的
 * 硬换行（行尾两个空格 = 换行语义），而协议写的是「不得改动语义」。
 * **这条口径必须与 `host/index.js` 的 `normalizeBody` 一致**（那里是实际落盘
 * 的那一份），否则摘要会算在两份不同的正文上。
 *
 * @param {string} text - 原始正文。
 * @returns {string} 规范化后的正文。
 */
export function normalizeText(text) {
  return String(text ?? '')
    .replace(/^\uFEFF/, '')
    .replace(/\r\n?/g, '\n')
    .replace(/\s+$/, '');
}

/**
 * §1.3 `contentDigest`：规范化后正文的 SHA-256（十六进制小写）。
 *
 * **摘要不匹配 = 快照损坏 → 按 `snapshot_invalid` 拒**（不是重读）。
 *
 * @param {string} text - 原始正文（内部先规范化）。
 * @returns {string} 64 位十六进制小写摘要。
 */
export function contentDigest(text) {
  const normalized = normalizeText(text);
  return createHash('sha256').update(normalized, 'utf8').digest('hex');
}

/**
 * 会话短码（协议 S-1：≤ 12 字符，同一会话恒定、不同会话不撞）。
 *
 * **算法只有这一份**（协议 v0.2 §2 S-1 起）：id 的分配权归 host，客户端
 * 不再自带副本——用**两段 32 位 FNV-1a 变体**拼 base36，取**后 12 位**，不足
 * 左补 `0`。Rust 那半必须逐位一致（协议 §6 的夹具就是为它准备的）。
 *
 * **为什么不用随机数**：进程重启后必须是**同一个**短码（同一会话恒定）。
 * **为什么不用会话 id 本身**：它可能很长、可能带非法字符——`snapshotId` 会
 * 直接进文件路径。
 *
 * **UTF-16 口径**：JS 的 `charCodeAt` 取的是 **UTF-16 code unit**，所以非 BMP
 * 字符（emoji 等）是**两个** unit。Rust 那半必须按 `encode_utf16()` 迭代，
 * 否则短码会与 JS 不同——**这是跨端一致性最容易踩的一处**（§6 的夹具就是
 * 为它准备的）。
 *
 * **它不是安全哈希**：用途是「不同会话不同短码」的概率分布，不是抗碰撞。
 * 协议 §S-1 说「不同会话不撞」——碰撞概率极低但**非零**，所以快照里存
 * **完整** `sessionId`（协议原话：「映射靠落盘快照，不靠短码反解」），撞了也
 * 只是短码相同，**不会丢数据、不会错指**。
 *
 * @param {string} sessionId - 官网会话 id。
 * @returns {string} 12 字符短码。
 */
export function makeSessionShort(sessionId) {
  const text = String(sessionId ?? '');
  let a = 0x811c9dc5;
  let b = 0x9e3779b9;
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i);
    a = Math.imul(a ^ code, 0x01000193) >>> 0;
    b = Math.imul(b + code + (i + 1), 0x85ebca6b) >>> 0;
  }
  return `${a.toString(36)}${b.toString(36)}`.slice(-12).padStart(12, '0');
}

/**
 * provider 稳定短码（协议 §S-1 举例：`ds` / `gpt` / `mm`）。
 *
 * **这份是唯一的 provider 短码来源**（协议 v0.2 §2 S-1：id 由 host 分配，
 * 客户端不自造）：`host/seq-store.js` 分配序号时用它拼 id，`buildSnapshot`
 * 落盘时用它对齐。客户端半边**不再持有**这份常量。
 */
export const PROVIDER_SHORT = 'ds';

/**
 * provider 短码（协议 S-1：≤ 4 字符，稳定 id，不用易变页签下标）。
 *
 * **只保留 ASCII 小写字母与数字**：与句柄形态一致——`snapshotId` 会直接进
 * 文件路径，短码里不能再出现会被误解成分隔或路径的字符。
 *
 * @param {string} providerId - provider 稳定 id。
 * @returns {string} 1–4 字符。
 */
export function makeProviderShort(providerId) {
  const cleaned = String(providerId ?? '')
    .toLowerCase()
    .replace(/[^0-9a-z]/g, '')
    .slice(0, 4);
  return cleaned.length > 0 ? cleaned : 'prov';
}

/**
 * §2 S-1：**单一 id 制度**——`ref === snapshotId`。
 *
 * 形态：`<providerShort>~<sessionShort>~<seq>`，
 * **整串 ≤ 24 个可见字符**（HC-2：这是 chip 宽度）。
 *
 * @param {{providerShort: string, sessionShort: string, seq: number}} input
 * @returns {string} 句柄字符串。
 */
export function makeSnapshotId({ providerShort, sessionShort, seq }) {
  const provider = String(providerShort ?? '').slice(0, 4);
  const session = String(sessionShort ?? '').slice(0, 12);
  const n = Number(seq ?? 0);
  const seqText = Number.isFinite(n) && n > 0 ? String(n).padStart(4, '0') : '0000';
  return `${provider}~${session}~${seqText}`;
}

/**
 * **临时文件名形态**（协议 §2.3）。
 *
 * 形态：`.<snapshotId>.json.tmp-<pid>-<seq>-<attempt>`
 * —— **点开头**（与最终名一眼能区分「已发布」与「未发布」）。
 *
 * **为什么口径写在协议这一层**：`host/index.js`（写）与 `host/sweep.js`（清）
 * 都要认这个形态 ⇒ 两边各写一份正则，就会出现「写的那份和清的那份不一样」
 * 的形状（两份口径同时活着，谁也不知道哪份是活的）⇒ **单一真相在这里**。
 */
const TEMP_NAME_RE = /^\..+\.json\.tmp-[\w.-]+$/;

/**
 * 造一个临时文件名（协议 §2.3）。
 *
 * @param {string} snapshotId - 目标快照 id（最终名是 `<snapshotId>.json`）。
 * @param {{pid: number, seq: number|string, attempt: number}} parts - 进程号、序号、尝试号。
 * @returns {string} 临时文件名（**只是文件名**，不含目录）。
 */
export function tempSnapshotName(snapshotId, { pid, seq, attempt }) {
  const id = String(snapshotId ?? '');
  const seqText = String(seq ?? '0000').padStart(4, '0');
  return `.${id}.json.tmp-${String(pid ?? 0)}-${seqText}-${String(attempt ?? 0)}`;
}

/**
 * 这个文件名是不是**临时文件**（协议 §2.3 的第五家族）。
 *
 * @param {string} name - 文件名。
 * @returns {boolean} 是临时文件即 true。
 */
export function isTempSnapshotName(name) {
  return TEMP_NAME_RE.test(String(name ?? ''));
}

/**
 * **序号水位记录**的文件名（协议 §2.5）。
 *
 * ## 为什么不带 `.json` 后缀
 *
 * 认领文件的保留期清理（`host/sweep.js` 的 `sweepClaims`）删的是 **`*.json`**
 * ⇒ 水位记录**天然不会被连带删掉**（协议 §2.5 ②「不得被清理流程删掉」）。
 * 若它也叫 `*.json`，就得靠「按名字排除」来保命 ⇒ 多一条会失效的约定。
 *
 * ## 它为什么「独立于认领文件」（协议 §2.5 ①）
 *
 * 形状**不是** `<seq>.json` ⇒ 认领目录里「认领」与「水位」两类东西一眼能分开
 * ⇒ 也**不会**被当成一个序号用掉（`claimedSeqs` 只认 `<数字>.json`）。
 */
export const WATERMARK_FILE_NAME = 'seq-watermark';

/**
 * 造一份**序号水位记录**（协议 §2.5）。
 *
 * **内容由实现方定**（协议 §2.5：「具体文件名与格式由实现方定」）——
 * 这里只保证一件事：**里面有一个「已经发放过的最大 seq」**。
 *
 * @param {{providerShort: string, sessionShort: string, seq: number, at: string}} input
 * @returns {object} 水位记录。
 */
export function makeWatermarkRecord({ providerShort, sessionShort, seq, at }) {
  return {
    // **不是快照的 `schemaVersion`**：这是水位记录自己的版本（协议 §4 的
    // 「快照 `schemaVersion` 不因本批升版」说的是快照，两回事）。
    recordVersion: 1,
    providerShort: String(providerShort ?? ''),
    sessionShort: String(sessionShort ?? ''),
    seq: Number(seq ?? 0),
    updatedAt: String(at ?? ''),
  };
}

/**
 * 读一份**序号水位记录** ⇒ 得出「这个会话已经发放过的最大 seq」。
 *
 * @param {string} raw - 文件正文。
 * @returns {{ok: true, seq: number} | {ok: false}} **读不动就是 `ok:false`**
 *   （调用方据此**拒绝分配**：水位不明 ⇒ 无从知道发到哪了 ⇒ 不敢复用序号）。
 */
export function parseWatermarkRecord(raw) {
  let parsed;
  try {
    parsed = JSON.parse(String(raw ?? ''));
  } catch {
    return { ok: false };
  }
  const seq = Number(parsed?.seq);
  if (!Number.isFinite(seq) || seq < 0) return { ok: false };
  return { ok: true, seq };
}

/**
 * 拆句柄。**不用于反解会话**（协议 S-1：映射靠落盘快照，不靠短码反解），
 * 只用于校验形态与取出 `seq`。
 *
 * @param {string} ref - 句柄。
 * @returns {{providerShort: string, sessionShort: string, seq: number} | null}
 */
export function parseSnapshotId(ref) {
  const text = String(ref ?? '');
  const parts = text.split('~');
  if (parts.length !== 3) return null;
  const [providerShort, sessionShort, seqText] = parts;
  if (providerShort.length === 0 || providerShort.length > 4) return null;
  if (sessionShort.length === 0 || sessionShort.length > 12) return null;
  if (!/^\d{4}$/.test(seqText)) return null;
  return { providerShort, sessionShort, seq: Number(seqText) };
}

/**
 * §1.1 快照字段表：字段名 → 期望类型（`string` / `number` / `array` / `object`）。
 *
 * **不含 `captureResult`**：协议 v0.2 裁定它的形态 = **对象**（三字段齐全），
 * 字符串**只兼容 `'ok'`**——所以它**单独校验**（见 `validateSnapshot`），
 * 不进这张类型表（进了就会把字符串形态提前判死）。
 */
export const SNAPSHOT_FIELDS = Object.freeze({
  schemaVersion: 'number',
  snapshotId: 'string',
  providerId: 'string',
  accountScope: 'string',
  sessionId: 'string',
  branch: 'string',
  messages: 'array',
  contentDigest: 'string',
  capturedAt: 'string',
  sourceUpdatedAt: 'string',
  sourceFingerprint: 'string',
});

/** `captureResult` 里必须显式记的子字段（§1.1：未支持内容要显式记，不静默丢）。 */
export const CAPTURE_RESULT_FIELDS = Object.freeze([
  'status',
  'omitted',
  'unsupported',
]);

/** `captureResult.status` 的三个取值（§1.1：`ok` / `partial` / `failed`）。 */
export const CAPTURE_STATUSES = Object.freeze(['ok', 'partial', 'failed']);

/**
 * §1.1 快照字段表逐字段校验。
 *
 * **只判形状与必填**，不判业务语义（正文是否超限是另一层）。
 *
 * @param {unknown} snapshot - 待校验对象。
 * @returns {{ok: boolean, errors: string[]}}
 */
export function validateSnapshot(snapshot) {
  const errors = [];
  if (snapshot === null || typeof snapshot !== 'object' || Array.isArray(snapshot)) {
    return { ok: false, errors: ['快照不是一个对象'] };
  }
  for (const [field, kind] of Object.entries(SNAPSHOT_FIELDS)) {
    const value = snapshot[field];
    if (value === undefined || value === null) {
      errors.push(`缺字段 ${field}`);
      continue;
    }
    if (kind === 'array' && !Array.isArray(value)) {
      errors.push(`字段 ${field} 不是数组`);
      continue;
    }
    if (kind === 'object' && (typeof value !== 'object' || Array.isArray(value))) {
      errors.push(`字段 ${field} 不是对象`);
      continue;
    }
    if ((kind === 'string' || kind === 'number') && typeof value !== kind) {
      errors.push(`字段 ${field} 不是 ${kind}`);
    }
  }
  if (typeof snapshot.schemaVersion === 'number' && snapshot.schemaVersion !== SCHEMA_VERSION) {
    errors.push(`schemaVersion 不支持：${snapshot.schemaVersion}`);
  }
  if (typeof snapshot.snapshotId === 'string') {
    if (parseSnapshotId(snapshot.snapshotId) === null) {
      errors.push(`snapshotId 形态不合法：${snapshot.snapshotId}`);
    }
    if (snapshot.snapshotId.length > 24) {
      errors.push(`snapshotId 超过 24 个可见字符：${snapshot.snapshotId.length}`);
    }
  }
  if (typeof snapshot.contentDigest === 'string' && !/^[0-9a-f]{64}$/.test(snapshot.contentDigest)) {
    errors.push('contentDigest 不是 64 位十六进制小写');
  }
  if (snapshot.captureResult !== null && typeof snapshot.captureResult === 'object') {
    for (const field of CAPTURE_RESULT_FIELDS) {
      if (!(field in snapshot.captureResult)) {
        errors.push(`captureResult 缺字段 ${field}`);
      }
    }
    const status = snapshot.captureResult.status;
    if (typeof status !== 'string') {
      errors.push('captureResult.status 不是字符串');
    } else if (!CAPTURE_STATUSES.includes(status)) {
      errors.push(`captureResult.status 非法：${status}`);
    }
    if (!Array.isArray(snapshot.captureResult.omitted)) {
      errors.push('captureResult.omitted 不是数组');
    }
    if (!Array.isArray(snapshot.captureResult.unsupported)) {
      errors.push('captureResult.unsupported 不是数组');
    }
  } else if (snapshot.captureResult === 'ok') {
    // **协议 v0.2 裁定**：`captureResult` 的形态 = **对象**（三字段齐全）；
    // 字符串**只兼容 `'ok'`**（等价 `{status:'ok',omitted:[],unsupported:[]}`）。
    //
    // **为什么 `'ok'` 还收**：它是**无损**的——`'ok'` 没有省略事实可丢，
    // 与对象形态一一对应。而 `'partial'` / `'failed'` 用字符串会**丢掉「省略了
    // 什么」**，违反「不静默裁掉」纪律 ⇒ 那两个**必须用对象**。
  } else if (typeof snapshot.captureResult === 'string') {
    errors.push(
      `captureResult 必须是对象（协议 v0.2）；字符串只兼容 'ok'，收到：${snapshot.captureResult}`,
    );
  } else {
    errors.push('captureResult 不是对象');
  }
  if (Array.isArray(snapshot.messages)) {
    snapshot.messages.forEach((message, index) => {
      if (message === null || typeof message !== 'object') {
        errors.push(`messages[${index}] 不是对象`);
        return;
      }
      if (typeof message.role !== 'string') {
        errors.push(`messages[${index}] 缺 role`);
      }
      if (typeof message.text !== 'string') {
        errors.push(`messages[${index}] 缺 text`);
      }
      if (message.role !== String(message.role).toLowerCase()) {
        errors.push(`messages[${index}] 的 role 不是小写枚举`);
      }
    });
  }
  return { ok: errors.length === 0, errors };
}

/**
 * §1.2 目录页字段（**不含正文**）。
 *
 * 目录页是**独立缓存物**（与快照分开存），内容只有列表元数据。
 */
export const CATALOG_FIELDS = Object.freeze({
  accountScope: 'string',
  providerId: 'string',
  items: 'array',
  cursor: 'string',
  hasMore: 'boolean',
  fetchedAt: 'string',
});

/**
 * 目录项字段（协议 P4 §2.4 面二 / §4.3 ②：**恰好 6 个键**，类型写死）。
 *
 * ⚠️ **键名是「换」不是「加」**：旧形态是 `sessionId` / `title` / `updatedAt` /
 * `deepLink`（P1 §1.2 的旧目录页）⇒ ⇒ 现在换成**站点会话目录项**那 6 个键：
 * `id` 取代 `sessionId`、`updated_at` 取代 `updatedAt`、多了 `title_type` /
 * `model_type` / `pinned`、**去掉了 `deepLink`**。
 *
 * ⚠️ **类型不再全是 string**：`pinned` 是 **boolean**、`updated_at` 是 **number**
 * （秒级小数时间戳）—— 这就是旧的那句「每个字段都必须是 string」必须一起改
 * 的原因（它与新键面**正面冲突**）。
 *
 * ⚠️ **`deepLink` ＝ 已解禁、但本轮不产出**（协议 **v0.22 §2.4** 已把它列为面二的
 * **可选第 7 键**，类型 `string`，四条解禁条件全满足）⇒ ⇒ **解禁是「许可」不是
 * 「义务」** ⇒ ⇒ 本轮**没有消费方**（用户侧不上屏；正文那条链**自己造**深链）⇒ ⇒
 * 目录项里**仍然没有**深链。
 *
 * ⚠️ **`cursor` 那半仍未坐实**（缺口 **G4-8**）⇒ ⇒ 游标在目录页**顶层**那个
 * `cursor` 字段上，仍是空串。
 *
 * ⚠️ **将来真有消费方时三面一起改**（契约表 ＋ Rust 侧目录项 ＋ 插件侧构造）⇒ ⇒
 * 形态按协议 v0.22：**不许带 query／fragment／别的段**（它是条目 `id` 按 G4-19
 * 形态的派生，不承载站点的额外信息）。
 */
export const CATALOG_ITEM_FIELDS = Object.freeze({
  id: 'string',
  title: 'string',
  title_type: 'string',
  model_type: 'string',
  pinned: 'boolean',
  updated_at: 'number',
});

/**
 * 目录页的**来源标识**（**顶层一个字段** `source`）。
 *
 * ## 形态
 *
 * `{ kind, id, label }` 三个键：
 *
 * · `kind` —— **来源类别**：`site`（真实站点）／`fixture`（固定夹具）。
 * · `id` —— **来源短码**（`deepseek` 之类；夹具是 `fixture`）。
 * · `label` —— **展示名**（`DeepSeek`／`固定夹具`）。
 *
 * ## 为什么是三个键而不是一个字符串
 *
 * 界面要干两件事：**判类别**（这一列能不能引用）＋**显名字**（给用户看）。
 * 一个字符串只能干一件 ⇒ ⇒ 拆成「类别 ＋ 短码 ＋ 展示名」三格。
 *
 * ## 为什么 `id` 由**内核**给
 *
 * 插件那半边**够不着站点**（它只连本机回环桥）⇒ ⇒ 「我是谁」只有建窗的那侧
 * 知道 ⇒ ⇒ 短码由**内核**在桥的回包信封上给（`site-source.js` 只做形态校验）。
 */
export const CATALOG_SOURCE_KINDS = Object.freeze(['site', 'fixture']);

/**
 * 目录页 `source` 的**键面**（**恰 3 个**，协议 v0.19 §10 第 8 条 8b）。
 *
 * **穷举列举** ⇒ ⇒ 第 4 个键**也是错**（不是「至少有这几个」）。
 */
export const CATALOG_SOURCE_KEYS = Object.freeze(['kind', 'id', 'label']);

/**
 * 站点短码 → **展示名**（**品牌名只在这里写一次**；界面直接用 `label`）。
 *
 * ⚠️ **短码不在表里 ⇒ 展示名就用短码本身**（**不编一个像模像样的名字**）。
 */
export const SITE_LABELS = Object.freeze({ deepseek: 'DeepSeek' });

/**
 * 造目录页的**来源标识**。
 *
 * @param {string} kind - `site` 或 `fixture`。
 * @param {string} id - 来源短码。
 * @returns {{kind: string, id: string, label: string}}
 */
export function buildCatalogSource(kind, id) {
  const k = CATALOG_SOURCE_KINDS.includes(String(kind)) ? String(kind) : 'fixture';
  const raw = String(id ?? '').trim();
  if (k === 'fixture') {
    return { kind: 'fixture', id: 'fixture', label: '固定夹具' };
  }
  const sid = raw === '' ? 'unknown' : raw;
  return { kind: 'site', id: sid, label: SITE_LABELS[sid] ?? sid };
}

/**
 * **正文那一格还没接通**时的**如实说明**（进 `detail`）。
 *
 * ## ⚠️ **host 侧不再挂这个标记了**（协议 §4.4 的实现已接通）
 *
 * G4-19（深链形态）／G4-20（深链导航后站点确实发那条请求）**已坐实** ⇒ ⇒ 正文
 * 那一格**接的是真实源** ⇒ ⇒ `host/commands.js` 的 `capture` **不再**产这一句。
 *
 * ## 为什么**留着**这个常量
 *
 * ① **客户端那处仍按这个标记认文案**（`client/client.js` 的
 * `BODY_SOURCE_PENDING_MARK`）⇒ ⇒ 常量留着 = 客户端那套判读**不用改**
 * （界面域的文件**不在本单范围**）；
 * ② **万一**哪条链又落回「这一格还没接通」的状态 ⇒ 它**仍是一句如实说明**
 * ⇒ ⇒ 客户端**认得出**、**不会**把它误当成别的失败。
 *
 * ## 为什么码是 `source_unsupported`
 *
 * 协议 §3：这个码**只**表示「这个来源不支持该能力」⇒ ⇒ **正合适**
 * （不是超时、不是结构变了、不是取消）⇒ ⇒ 闭集 14 码里已有，**不新造**。
 *
 * ## 句首那个**机器标记**（`body-source-pending:`）
 *
 * 这一档**要能被界面认出来** ⇒ ⇒ `detail` 的**句首**放一个**稳定的机器标记**
 * （ASCII、含结尾冒号）⇒ ⇒ 界面按它 `startsWith` 就能认出「这一格还没接通」那一档，
 * **不用去认中文句子**（中文会随文案改 ⇒ 抄本会漂）。
 *
 * ⚠️ **句首之外的内容**（那句中文说明）**已按现状订正**（协议 §4.4 已成立、G4-19／
 * G4-20 均已坐实 ⇒ ⇒ 「尚未坐实」那句话是**过时陈述**，留着假陈述是隐患）⇒ ⇒
 * 改的是**句首之后**那段中文；**句首那串机器标记一个字没动**（动了客户端就静默
 * 退成通用失败文案）。
 */
export const BODY_SOURCE_PENDING_DETAIL =
  'body-source-pending:正文取得机制已坐实（协议 §4.4 已成立，G4-19／G4-20 均已坐实）⇒ ' +
  'host 侧不再产这一句；此常量仅留给将来别的站启用时用的客户端判读标记';

/**
 * §1.2 目录页校验。**正文不许出现**（协议：目录页不含正文）。
 *
 * @param {unknown} page - 待校验对象。
 * @returns {{ok: boolean, errors: string[]}}
 */
export function validateCatalogPage(page) {
  const errors = [];
  if (page === null || typeof page !== 'object' || Array.isArray(page)) {
    return { ok: false, errors: ['目录页不是一个对象'] };
  }
  for (const [field, kind] of Object.entries(CATALOG_FIELDS)) {
    const value = page[field];
    if (value === undefined || value === null) {
      errors.push(`缺字段 ${field}`);
      continue;
    }
    if (kind === 'array' && !Array.isArray(value)) {
      errors.push(`字段 ${field} 不是数组`);
    } else if (kind === 'object' && (typeof value !== 'object' || Array.isArray(value))) {
      errors.push(`字段 ${field} 不是对象`);
    } else if ((kind === 'string' || kind === 'number' || kind === 'boolean') && typeof value !== kind) {
      errors.push(`字段 ${field} 不是 ${kind}`);
    }
  }
  if (page.source !== undefined && page.source !== null) {
    // **来源标识**（可选字段）：**有就断形态**。
    //
    // ⚠️ **为什么是「可选」**：① 目录页是**独立缓存物**（5 分钟 TTL）⇒ 升级前
    // 落盘的旧缓存页**没有**这个字段 ⇒ 升级后那 5 分钟内不能因此判红；
    // ② **P1 协议夹具**（`fixtures/protocol/catalog-ok.json`）描述的是
    // **P1 §1.2 的目录页形态**（协议域）⇒ 不由本单改。
    // ⇒ ⇒ **host 新生成的每一页都带**（`commands.js` 的 `catalog`）⇒ ⇒
    // 界面上拿不到就退旧文案（**不假装知道来源**）。
    const source = page.source;
    if (typeof source !== 'object' || Array.isArray(source)) {
      errors.push('字段 source 不是对象');
    } else {
      if (!CATALOG_SOURCE_KINDS.includes(String(source.kind))) {
        errors.push(`source.kind 不是 ${CATALOG_SOURCE_KINDS.join(' / ')}`);
      }
      for (const field of ['id', 'label']) {
        if (typeof source[field] !== 'string' || source[field] === '') {
          errors.push(`source.${field} 不是非空字符串`);
        }
      }
      // **恰 3 个键**（协议 v0.19 §10 第 8 条 8b：`{kind, id, label}` —— **第 4 个
      // 键也是错**）⇒ ⇒ 白名单是**穷举列举**，不是「至少有这几个」。
      const extraSourceKeys = Object.keys(source).filter(
        (field) => !CATALOG_SOURCE_KEYS.includes(field),
      );
      if (extraSourceKeys.length > 0) {
        errors.push(`source 出现第 4 个键：${extraSourceKeys.join('、')}`);
      }
    }
  }
  if (Array.isArray(page.items)) {
    page.items.forEach((item, index) => {
      if (item === null || typeof item !== 'object') {
        errors.push(`items[${index}] 不是对象`);
        return;
      }
      // **逐键断类型**（协议 §4.3 ②）：`pinned` 是 boolean、`updated_at` 是 number，
      // 其余是 string ⇒ ⇒ **不再**是「每个字段都必须是 string」。
      for (const [field, kind] of Object.entries(CATALOG_ITEM_FIELDS)) {
        if (typeof item[field] !== kind) {
          errors.push(`items[${index}] 的 ${field} 不是 ${kind}`);
        }
      }
      // **恰好 6 个键**：白名单是穷举列举 ⇒ 第 7 个键也是错。
      const extra = Object.keys(item).filter((field) => !(field in CATALOG_ITEM_FIELDS));
      if (extra.length > 0) {
        errors.push(`items[${index}] 出现第 7 个键：${extra.join('、')}`);
      }
      if ('body' in item || 'text' in item || 'messages' in item) {
        errors.push(`items[${index}] 带了正文（目录页不含正文）`);
      }
    });
  }
  return { ok: errors.length === 0, errors };
}

/**
 * §2 S-3：句柄状态**由快照文件派生**。
 *
 * 落盘后不再另存内存状态（内存与磁盘两处状态会漂）。所以这个函数是
 * **唯一**的状态来源：
 *
 *   · 快照不存在 → `unknown`（不可用）
 *   · 快照 `captureResult.status === 'failed'` → `failed`
 *   · 快照 `status === 'partial'` → `ready`，但**带省略事实**（S-5 口径）
 *   · 其余 → `ready`
 *
 * @param {unknown} snapshot - 快照对象（`null` 表示没找到）。
 * @param {{digestOk?: boolean, fingerprintOk?: boolean}} [checks]
 *   - `digestOk`：`contentDigest` 与正文是否一致（默认 `true`）
 *   - `fingerprintOk`：账号指纹是否与当前一致（默认 `true`）
 * @returns {{state: string, code: string|null, detail: string|null}}
 */
export function deriveState(snapshot, checks = {}) {
  if (snapshot === null || snapshot === undefined) {
    return { state: 'unknown', code: 'snapshot_invalid', detail: '快照不存在' };
  }
  if (checks.digestOk === false) {
    return { state: 'invalid', code: 'snapshot_invalid', detail: '摘要不匹配（快照损坏）' };
  }
  if (checks.fingerprintOk === false) {
    return { state: 'invalid', code: 'account_changed', detail: '账号指纹已变' };
  }
  const schemaOk = snapshot.schemaVersion === SCHEMA_VERSION;
  if (!schemaOk) {
    return { state: 'invalid', code: 'schema_changed', detail: '快照版本不支持' };
  }
  const status =
    typeof snapshot.captureResult === 'string'
      ? snapshot.captureResult
      : snapshot.captureResult?.status;
  if (status === 'failed') {
    return { state: 'failed', code: 'partial_content', detail: '捕获结果是 failed' };
  }
  return { state: STATES.ready, code: null, detail: null };
}

/**
 * 构造一个稳定的错误负载。
 *
 * **内部异常串一律不进这里**（协议 §3）：`detail` 只能传**已经分类好的**
 * 事实（如「正文 300KB > 上限 256KB」），原始 `error.message` 只能写日志。
 *
 * @param {string} code - §3 的稳定码名。
 * @param {{requestId?: string, logPath?: string, detail?: string}} [extra]
 * @returns {{kind: 'error', ok: false, code: string, retryable: boolean, userNext: string,
 *   requestId: string|null, logPath: string|null, detail: string|null}}
 */
export function errorPayload(code, extra = {}) {
  const spec = ERROR_CODES[code];
  if (spec === undefined) {
    // **协议 §3：内部冲突码一律归入 `snapshot_invalid`**，原始码保留在 `detail`。
    //
    // **不归 `source_unsupported`**：那个码只表示「这个来源根本不支持该能力」，
    // 拿它兜内部冲突会让用户去「换一个来源」，而真正该做的是移除该引用。
    return {
      kind: 'error',
      ok: false,
      code: 'snapshot_invalid',
      retryable: ERROR_CODES.snapshot_invalid.retryable,
      userNext: ERROR_CODES.snapshot_invalid.userNext,
      requestId: extra.requestId ?? null,
      logPath: extra.logPath ?? null,
      detail: `未知错误码 ${String(code)}`,
    };
  }
  return {
    // **协议 §3 v0.4 裁定：错误结果对象字段闭集 = 8 个，全部必备**（`kind` / `ok` /
    // `code` / `retryable` / `userNext` / `requestId` / `logPath` / `detail`）⇒
    // `kind` 此前**只产 7 个**（缺这一个），是 **JS 漏字段**、不是协议写错 ⇒
    // 这里补齐，**与 Rust 侧 `error_result` 同构**（两端字段集一致，协议夹具的
    // 「双端一致」才可能成立）。
    kind: 'error',
    ok: false,
    code,
    retryable: spec.retryable,
    userNext: spec.userNext,
    requestId: extra.requestId ?? null,
    logPath: extra.logPath ?? null,
    detail: extra.detail ?? null,
  };
}

/**
 * 脱敏日志路径。
 *
 * 协议 §5：「不在代码中展开真实用户 home」——所以日志路径一律用**占位符**
 * 表示实例根，绝不把真实家目录拼进要落盘/要外发的字符串里。
 *
 * @param {string} relative - 相对实例根的路径。
 * @returns {string} 形如 `<instance>/logs/official-chat.log` 的脱敏路径。
 */
export function redactLogPath(relative) {
  const text = String(relative ?? '').replace(/\\/g, '/').replace(/^\/+/, '');
  return `<instance>/${text}`;
}

/**
 * 消息之间的分隔符（**协议 §1.3 没规定，是实现选定的口径**，交付时单列需 PM 确认）。
 *
 * 取**空行**（`\n\n`）而不是单换行：一条消息的正文若以空行结尾，单换行分隔会
 * 让「消息边界」与「正文里的空行」混成一团；空行分隔在两边都能唯一还原。
 *
 * **两端必须一致**：JS 与 Rust 的 `contentDigest` 都按它拼，夹具固化其结果。
 */
export const MESSAGE_SEPARATOR = '\n\n';

/**
 * 把一组消息拼成「正文」。
 *
 * @param {Array<{role: string, text: string}>} messages - 已规范化的消息。
 * @returns {string}
 */
export function joinMessages(messages) {
  return (Array.isArray(messages) ? messages : []).map((message) => message.text).join(MESSAGE_SEPARATOR);
}

/**
 * 规范化后的正文总字节数（预算判定用）。
 *
 * @param {string} text - 正文。
 * @returns {number} UTF-8 字节数。
 */
export function byteLength(text) {
  return Buffer.byteLength(normalizeText(text), 'utf8');
}

/**
 * 规范化一组消息（§1.3：角色名小写枚举）。
 *
 * **正文逐条不动**：规范化只作用在**拼接后的整体**上（`contentDigest` 里那次），
 * 与 `host/index.js` 的 `bodyOf` + `normalizeBody` 口径一致——
 * 若这里逐条去尾随空白，中间消息的行尾空白会被悄悄改掉，两侧摘要就对不上。
 *
 * @param {Array<{role: string, text: string}>} messages
 * @returns {Array<{role: string, text: string}>}
 */
export function normalizeMessages(messages) {
  if (!Array.isArray(messages)) return [];
  return messages.map((message) => ({
    role: String(message?.role ?? '').toLowerCase(),
    text: String(message?.text ?? ''),
  }));
}

/**
 * 正文合计字节数（**消息之间的 `MESSAGE_SEPARATOR` 也要计入**）。
 *
 * **为什么**：协议 §4 的「最大正文字节」指的是**正文**的字节数，而正文 =
 * 各条文本按 `MESSAGE_SEPARATOR` 拼起来的那一串（`joinMessages`）。少了
 * 分隔符 ⇒ 算出来的是「各条文本之和」，**不是正文** ⇒ 同一份正文在 JS 与
 * Rust 两侧会差 `2 × (条数 - 1)` 字节 ⇒ 临界时两端判定相反（协议 §6 的
 * 「同一夹具 → 同一结果」被破坏）。Rust 侧 `body_of(..).len()` 走的是
 * **拼接后**的字节数，两端口径必须一致。
 *
 * @param {Array<{role: string, text: string}>} messages - 已规范化的消息。
 * @returns {number} UTF-8 字节数。
 */
export function bodyBytes(messages) {
  const list = Array.isArray(messages) ? messages : [];
  if (list.length === 0) return 0;
  let total = 0;
  for (const message of list) {
    total += Buffer.byteLength(String(message?.text ?? ''), 'utf8');
  }
  return total + (list.length - 1) * MESSAGE_SEPARATOR.length;
}

/**
 * P2 协议 §1.1：重复组的判定键 = **五字段全等**。
 *
 * **为什么不能只比正文摘要一个**：`development-plan` §8:168「去重不能抹掉
 * 账户、分支或捕获事实」——只比 `contentDigest` 会把**不同账户 / 不同分支 /
 * 不同捕获时刻**的同正文判成同一组。
 */
export const GROUP_KEY_FIELDS = Object.freeze([
  'accountScope',
  'sessionId',
  'branch',
  'capturedAt',
  'contentDigest',
]);

/**
 * P2 协议 §1.2：`groupKey` = 五字段连接后取 sha256 的**前 16 位小写十六进制**。
 *
 * **分隔符口径（待 PM 确认 · 一行可回退）**：协议原文写
 * `sha256(accountScope \n sessionId \n branch \n capturedAt \n contentDigest)`，
 * 行内代码里 `\n` 两侧有空格。本实现按「**单个换行、不含空格**」理解
 * （`GROUP_KEY_FIELDS.join('\n')`）。若 PM 裁定为含空格（` \n `），
 * **只改这一行**（JS 侧）＋ Rust 侧对应一行，其余不动。
 *
 * **为什么在 host 侧算**：client 半边是浏览器页面里的**单文件自包含**脚本，
 * 不能 `require('node:crypto')` ⇒ groupKey 只能由 host 算好后随出参下发
 * （协议 §1.2：groupKey 的用途就是「client 侧把同组引用聚在一起」）。
 *
 * @param {object} snapshot - 快照对象（§1.1 字段表）。
 * @returns {string} 16 位小写十六进制组标识。
 */
export function groupKeyOf(snapshot) {
  const text = GROUP_KEY_FIELDS.map((field) => String(snapshot?.[field] ?? '')).join('\n');
  return createHash('sha256').update(text, 'utf8').digest('hex').slice(0, 16);
}

/**
 * 构造快照（§1.1 字段表逐字段落地）。
 *
 * @param {object} input - 字段来源。
 * @returns {object} 快照对象。
 */
export function buildSnapshot(input) {
  const messages = normalizeMessages(input?.messages);
  const providerShort = String(input?.providerShort ?? PROVIDER_SHORT);
  const sessionShort = makeSessionShort(input?.sessionId);
  return {
    schemaVersion: SCHEMA_VERSION,
    snapshotId: makeSnapshotId({
      providerShort,
      sessionShort,
      seq: Number(input?.seq ?? 1),
    }),
    providerId: String(input?.providerId ?? ''),
    accountScope: String(input?.accountScope ?? ''),
    sessionId: String(input?.sessionId ?? ''),
    branch: String(input?.branch ?? 'main'),
    messages,
    contentDigest: contentDigest(joinMessages(messages)),
    captureResult: {
      status: String(input?.captureResult?.status ?? 'ok'),
      omitted: Array.isArray(input?.captureResult?.omitted)
        ? [...input.captureResult.omitted]
        : [],
      unsupported: Array.isArray(input?.captureResult?.unsupported)
        ? [...input.captureResult.unsupported]
        : [],
    },
    capturedAt: String(input?.capturedAt ?? ''),
    sourceUpdatedAt: String(input?.sourceUpdatedAt ?? ''),
    sourceFingerprint: String(input?.sourceFingerprint ?? ''),
  };
}

/**
 * §6 夹具的**统一判定入口**：同一份夹具，JS 与 Rust 各跑一遍，结果必须一致。
 *
 * 三种流程（按 `input.kind` 分派），**判定顺序写死在这里**——两端照抄，
 * 顺序不同就会出现「同一夹具不同结果」，那正是 §6 要防的事。
 *
 *   · `capture`——捕获一份快照：版本 → 正文预算 → 捕获结果 → 落快照。
 *   · `catalog`——取目录页：空结果与读取失败**必须分开**（§3）。
 *   · `lookup`——用句柄取快照：缺失 / 版本 / 摘要 / 指纹，逐条判。
 *
 * @param {object} input - 夹具的 `input`。
 * @returns {object} 结果（形状见各分支注释）。
 */
export function resolveCase(input) {
  const kind = String(input?.kind ?? 'capture');
  if (kind === 'catalog') return resolveCatalogCase(input);
  if (kind === 'lookup') return resolveLookupCase(input);
  if (kind === 'duplicates') return resolveDuplicatesCase(input);
  return resolveCaptureCase(input);
}

/**
 * 重复组判定流程（P2 协议 §1.1 / §1.2）。
 *
 * **判的是「键五字段全等」**（accountScope / sessionId / branch / capturedAt /
 * contentDigest）——只比正文摘要会把**不同账户 / 不同分支 / 不同捕获时刻**的
 * 同正文判成同一组（`development-plan` §8:168「去重不能抹掉账户、分支或捕获
 * 事实」）。
 *
 * **输出带 `groupKey`，夹具的 `expected` 也固化它的值**：协议 v0.2 §5 明写
 * 「键的形状以本稿 §1.1 为准（**已定稿，可写**）」，§1.2 又说 groupKey
 * 「用于提示文案与**判据比对**」⇒ 夹具就该带上它（判据侧的期望匹配是
 * **整体深比较**，不写就判红）。
 *
 * **分隔符口径若被改，要同步改四处**：协议 §1.2 的行内代码里 `\n` 两侧有
 * 空格，本实现按「**单个换行、不含空格**」实现 ⇒ 若裁定为含空格
 * （` \n `），须同步改：JS 的 `groupKeyOf` 1 行、Rust 的 `group_key` 1 行、
 * 两个夹具的 `expected.groupKey`（各 1 个值）⇒ 漏改任何一处，夹具断言
 * 与「两端逐字段相等」判据都会报红。
 *
 * @param {object} input - 夹具的 `input`（`snapshots` 两条，各带五字段）。
 * @returns {{kind: 'duplicates', sameGroup: boolean, groupKey: string}}
 */
export function resolveDuplicatesCase(input) {
  const snapshots = Array.isArray(input?.snapshots) ? input.snapshots : [];
  const groupKey = groupKeyOf(snapshots[0] ?? {});
  const sameGroup = snapshots.length >= 2 && groupKeyOf(snapshots[1] ?? {}) === groupKey;
  return { kind: 'duplicates', sameGroup, groupKey };
}

/**
 * 捕获流程。
 *
 * @param {object} input
 * @returns {{kind: 'snapshot', snapshot: object, state: string, notice: string|null}
 *   | {kind: 'error', code: string, retryable: boolean, userNext: string}}
 */
export function resolveCaptureCase(input) {
  // 捕获流程的**两种非正文结局**先判：超时与取消（S-2 的 `failed` / `cancelled`）。
  // 它们与「读到了但超限」不同——**没有正文，也就没有快照**。
  const outcome = String(input?.outcome ?? 'ok');
  if (outcome === 'timeout') {
    return { kind: 'error', ...errorPayload('timeout') };
  }
  if (outcome === 'cancelled') {
    return { kind: 'error', ...errorPayload('cancelled') };
  }
  const version = Number(input?.schemaVersion ?? SCHEMA_VERSION);
  if (version !== SCHEMA_VERSION) {
    return { kind: 'error', ...errorPayload('schema_changed') };
  }
  const messages = normalizeMessages(input?.messages);
  const bytes = bodyBytes(messages);
  // **单条引用正文超 256 KB ⇒ `body_too_large`**（协议 P1 §3）：
  //  · **判定对象**：**单条引用正文**（**不是「整条消息」**）——这里的 `bytes`
  //    就是**这一条引用**要进模型输入的正文量；
  //  · **判定时机**：**捕获这条引用时**（**不是「发送时」**）——正文在进模型
  //    输入之前就要判，判不过就**不落盘**；
  //  · **处置**：**拒那一条**（别的引用不受影响）。
  //
  // ⚠️ **判序裁定（P1 §3）**：第 6 码与第 14 码**判同一对象、同一时机、
  // 都是拒那一条**，**只差阈值**（256 KB vs 64 KB）⇒ ⇒ **必须按阈值大小顺序判**：
  // **先 256 KB（老码）、后 64 KB（新码）** ⇒ 反了就出死码（＞256 KB 必然也
  // ＞64 KB ⇒ 先判 64 KB ⇒ 老码永不触发 ⇒ 整条超限那档就没了）。
  if (bytes > CONSTANTS.maxBodyBytes) {
    return { kind: 'error', ...errorPayload('body_too_large') };
  }
  // **G3-2b：单条正文超「单条正文预算」64 KB ⇒ 落第 14 码 `body_budget_exceeded`
  // ＋ 拒收这一条引用**（别的引用不受影响）。
  //
  // ⚠️ **`detail` 必须带三个数**（正文字节 / 预算字节 / 超出量）——**客户端拿不到
  // host 的内部变量**（预算常量在 host 半边）⇒ ⇒ 界面文案要用的三个数**只能由
  // 这里带出去**（PM 2026-10-21 裁）⇒ 客户端按这个契约解析 ⇒ 取不到就退化成
  // 无数字文案（「这条引用太长，请移除它，换一段更短的正文。」）。
  if (bytes > CONSTANTS.perBodyBudgetBytes) {
    return {
      kind: 'error',
      ...errorPayload('body_budget_exceeded', {
        detail:
          `单条正文超单条正文预算：正文 ${bytes} 字节，` +
          `超出单条正文预算 ${CONSTANTS.perBodyBudgetBytes} 字节` +
          `（超 ${bytes - CONSTANTS.perBodyBudgetBytes} 字节）`,
      }),
    };
  }
  const status = String(input?.captureResult?.status ?? 'ok');
  if (status === 'failed') {
    // **捕获失败按原因分流**（协议 v0.3 §3「各码的使用边界」）：
    // 捕获失败**不是一个单一原因**，一律 `source_unsupported` 会让用户看到
    // 「换一个来源」而其实该**重试**——那是**掩盖真实原因**。
    // · 超时 → `timeout`（可重试）；页面结构变了 → `schema_changed`；用户取消 → `cancelled`。
    // · **原因不明 → `timeout` 兜**（可重试）。
    const reason = String(input?.captureFailure?.reason ?? '').trim().toLowerCase();
    return {
      kind: 'error',
      ...errorPayload(captureFailureCode(reason), { detail: captureFailureDetail(reason) }),
    };
  }
  const snapshot = buildSnapshot({ ...input, messages });
  const notice = status === 'partial' ? 'partial_content' : null;
  return { kind: 'snapshot', snapshot, state: STATES.ready, notice };
}

/**
 * 目录流程（§1.2 目录页是独立缓存物；§3 空结果与失败分离）。
 *
 * @param {object} input
 * @returns {{kind: 'catalog', page: object}
 *   | {kind: 'error', code: string, retryable: boolean, userNext: string}}
 */
export function resolveCatalogCase(input) {
  const outcome = String(input?.outcome ?? 'ok');
  if (outcome === 'empty') {
    return { kind: 'error', ...errorPayload('catalog_empty') };
  }
  if (outcome === 'failed') {
    return { kind: 'error', ...errorPayload('catalog_failed') };
  }
  const items = Array.isArray(input?.items) ? input.items : [];
  return {
    kind: 'catalog',
    page: {
      accountScope: String(input?.accountScope ?? ''),
      providerId: String(input?.providerId ?? ''),
      items: items.map((item) => ({
        id: String(item?.id ?? ''),
        title: String(item?.title ?? ''),
        title_type: String(item?.title_type ?? ''),
        model_type: String(item?.model_type ?? ''),
        pinned: Boolean(item?.pinned),
        updated_at: Number(item?.updated_at ?? 0),
      })),
      cursor: String(input?.cursor ?? ''),
      hasMore: Boolean(input?.hasMore),
      fetchedAt: String(input?.fetchedAt ?? ''),
    },
  };
}

/**
 * 取快照流程（S-5：失效一律拒）。
 *
 * @param {object} input
 * @returns {{kind: 'snapshot', snapshot: object, state: string}
 *   | {kind: 'error', code: string, retryable: boolean, userNext: string}}
 */
export function resolveLookupCase(input) {
  if (input?.exists === false) {
    return { kind: 'error', ...errorPayload('snapshot_invalid') };
  }
  const snapshot = input?.snapshot;
  if (snapshot === null || snapshot === undefined) {
    return { kind: 'error', ...errorPayload('snapshot_invalid') };
  }
  if (Number(snapshot.schemaVersion) !== SCHEMA_VERSION) {
    return { kind: 'error', ...errorPayload('schema_changed') };
  }
  const stored = String(snapshot.contentDigest ?? '');
  const actual = contentDigest(joinMessages(normalizeMessages(snapshot.messages)));
  if (stored !== actual) {
    return { kind: 'error', ...errorPayload('snapshot_invalid') };
  }
  const current = String(input?.currentFingerprint ?? '');
  if (current !== '' && current !== String(snapshot.sourceFingerprint ?? '')) {
    return { kind: 'error', ...errorPayload('account_changed') };
  }
  return { kind: 'snapshot', snapshot, state: STATES.ready };
}

