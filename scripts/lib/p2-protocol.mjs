// P2 协议判据库（协议稿 `docs/features/official-chat/official-chat-p2-protocol.md`
// v0.5 §5 的**六条验收判据** ＋ 三条**表外条**）。
//
// ## 与 P1 判据库的关系
//
// **独立成一套**（本文件不动 P1 的任何判据）。但 P1 立下的纪律一条不松：
//
//  · **三态 verdict**：`pass` / `fail` / `skipped`（未执行）。
//    「没触发到被测路径」必须显式报 `skipped`，不能省略、也不能记成 `fail`
//    ——否则「这条没测到」和「这条测了没过」在结果里长得一样。
//  · **期望值从协议稿逐条转写**，**不 import 任何实现模块**。
//    实现侧的 `groupKeyOf` / `duplicates` 这些函数**只作为被测对象**经
//    adapter 调用；「成组 / 不成组」的期望由本库**按 §1.1 的五字段规则
//    自己算**（`sameGroupByProtocol`），不拿实现的输出当标准答案。
//  · **判据自己红了改判据不改实现**：本文件抛错一律报「判据自身：…」
//    （与「实现尚未到位」区分开，见交付回执里的逐条归因）。
//
// ## 判据与协议条款的对应
//
// | 判据 | 协议条款 | 判什么 |
// | --- | --- | --- |
// | `P2-1` | §5 第 1 行 / §1.1 / §1.2 / §1.3 | 同组 ⇒ 仍是两个不同 `snapshotId`、都落盘、都可发；组标识**钉具体值** |
// | `P2-2` | §5 第 2 行 / §3.1-1 / §3.2 | 提示可见、可复查（输入区常驻条）、`level=info`、含条数与字节数；**再动一次引用可复现**（v0.5） |
// | `P2-3` | §5 第 3 行 / §1.1 | `capturedAt` 不同 ⇒ 不成组（负例） |
// | `P2-4` | §5 第 4 行 / §1.1 | `accountScope` / `branch` 不同 ⇒ 不成组（负例） |
// | `P2-5` | §5 第 5 行 / §4 / §3.3 | 超预算 ⇒ 发送前出现预算提示、**正文未被截断** |
// | `P2-6` | §5 第 6 行 / §2.3 | `duplicates` 失败 ⇒ 可见错误 ＋ **不额外拦截发送** ＋ **原码留痕** ＋ 码在闭集内 |
//
// **表外三条**（§5 的表只有六行，但协议正文另有硬条款 ⇒ 各落一条）：
//
// | 判据 | 协议条款 | 判什么 |
// | --- | --- | --- |
// | `P2-C` | §2.2 / P1 §3 `:162` | 错误结果对象 = **8 字段闭集**（命令层逐字段断） |
// | `P2-D` | §1.2 / §5「夹具」条款 | 同一组夹具：**协议独立转写 ＝ JS ＝ Rust** 三方 `groupKey` 一致 |
// | `P2-F` | §5「夹具」条款 | 夹具 `expected.groupKey` 与协议 §1.1 / §1.2 一致 |
//
// ## 三个「口径未定 ⇒ 判据不钉」的地方（避免假红）
//
// 1. **`groupKey` 的具体值**：~~分隔符口径待 PM 裁定 ⇒ 只断形状/稳定/区分~~
//    ⇒ **已闭环**：协议 v0.3 把分隔符裁定为**单个换行 U+000A、不含空格**
//    ⇒ 期望值可从协议稿独立转写 ⇒ 判据现在**钉具体值**（`expectedGroupKey`）。
// 2. **提示文案**：UX 文案未定稿 ⇒ 判**结构化数据**（组 / 条数 / 字节数）为主；
//    只有 §3.2 明确要求的「条数与字节数写在同一条文案里」才落到文案上。
//    **协议 v0.5 又加一条硬要求**：文案**自包含**（不依赖另一条还在）＋
//    **不得假设提示会一直在** ⇒ 判据断「**再动一次引用可复现**」
//    （见 `checkP2_2_duplicateNoticeVisible`）。
// 3. **原生 `@session` 占用**：§4 说 `projectedBytes` 要加上它，但 host 结构上
//    拿不到草稿 ⇒ **协议 v0.3 已裁定按 0 计**（方向＝宁可少报不误报）
//    ⇒ 判据只判「各引用各送一份的总字节」这一项。
//
// @module scripts/lib/p2-protocol

import { createHash } from 'node:crypto';

import { CONSTANTS as P1_CONSTANTS, ERROR_CODES } from './p1-protocol.mjs';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/**
 * 造一条 verdict。
 *
 * @param {string} name - 判据名（`P2-{n}-{条款}` 形式，便于回指协议）。
 * @param {string} status - `pass` / `fail` / `skipped`。
 * @param {string} detail - 人读的一句话（失败时必须写清**缺哪条协议**）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function verdict(name, status, detail) {
  return { name, status, detail: String(detail) };
}

/**
 * 汇总计数（与 P1 判据的产物同构）。
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
// ---------------------------------------------------------------------------

/**
 * §1.1 的**判定键**（五字段）。
 *
 * 「五字段任一不同 ⇒ 不是同一组」——本库据此**自己算**成组与否。
 */
export const GROUP_KEY_FIELDS = Object.freeze([
  'accountScope',
  'sessionId',
  'branch',
  'capturedAt',
  'contentDigest',
]);

/**
 * §1.2 的**组标识形状**（16 位小写十六进制）。
 *
 * 只判形状，**不判具体值**（分隔符口径待 PM 裁定）。
 */
export const GROUP_KEY_SHAPE = /^[0-9a-f]{16}$/;

/**
 * §1.2 的**字段连接符**（协议 v0.3 裁断：**单个换行 U+000A、不含空格**）。
 *
 * **为什么现在能钉死**：v0.1 时协议原文的行内代码里 `\n` 两侧有空格 ⇒ 分隔符
 * 口径未定 ⇒ 判据只能断形状/稳定/区分。v0.3 已裁定 ⇒ 期望值**可以**从协议稿
 * 独立转写出来 ⇒ 判据得以钉**具体值**（见 `expectedGroupKey`）。
 */
export const GROUP_KEY_SEPARATOR = '\n';

/**
 * 按协议 §1.2 **独立算出**组标识（**判据自算，不 import 实现当标准答案**）。
 *
 * `sha256(accountScope ␊ sessionId ␊ branch ␊ capturedAt ␊ contentDigest)`
 * 的**前 16 位小写十六进制**（␊ = 单个换行 U+000A，不含空格）。
 *
 * @param {object} snapshot - 快照对象。
 * @returns {string} 16 位小写十六进制组标识。
 */
export function expectedGroupKey(snapshot) {
  const text = GROUP_KEY_FIELDS.map((field) => String(snapshot?.[field] ?? '')).join(GROUP_KEY_SEPARATOR);
  return createHash('sha256').update(text, 'utf8').digest('hex').slice(0, 16);
}

/** §2.2 / §4 的单次消息总预算（P1 §4 的常量，字节）。 */
export const PER_MESSAGE_BUDGET_BYTES = 262144;

/**
 * §4 的**单条正文预算**（字节；64 KB）——**从 P1 期望侧取**（不 import 实现）。
 *
 * **为什么判据侧要这个数**：P2-5 的造数前提是「**每条正文 < 单条正文预算**」
 * ——超了就该被第 14 码 `body_budget_exceeded` **在捕获/落盘阶段拒收**、引用
 * **根本落不了盘** ⇒ 预算场景**触发不到**（PM 2026-10-21 裁）。
 *
 * **不 import 实现**：判据只认**协议写的那个值**（P1 期望侧），不认实现里
 * 碰巧写成多少。
 */
export const PER_BODY_BUDGET_BYTES = P1_CONSTANTS.perBodyBudgetBytes;

/**
 * P1 §3 的 **14 码闭集**（P2 §2.2 明写「沿用」）。
 *
 * **为什么直接 import P1 判据库的表**：闭集**只能有一份**——两份拷贝迟早漂移，
 * 而「闭集」这件事的全部价值就在于它是**同一个集合**。
 * （import 的是**常量表**，不碰 P1 的任何判据。）
 */
export const CLOSED_ERROR_CODES = Object.freeze(Object.keys(ERROR_CODES));

/**
 * P1 §3 `:162` 的**错误结果对象字段闭集**（**8 个，全部必备、不省略**）。
 *
 * P2 §2.2 明写「失败出参沿用 P1 §3 的 14 码闭集与错误结果对象闭集（**字段集以
 * P1 §3 `:162` 为准**，**本稿不复述字段个数**）」⇒ **这里就是那一份**。
 *
 * · 规则：**8 个字段一律出现**，不可得时填 **`null`**（**不省略字段、不填空串**）；
 * · 错误路径 `ok` 恒 `false`；`code` 取 **14 码闭集**；
 * · `detail` 承载脱敏后的原始码与原因线索。
 *
 * **为什么本库自己转写而不 import P1 的**：P1 判据库里没有这个常量（那边不判
 * 字段闭集）⇒ 本库**转写**一份，**并在 P2-C 判据里逐字段核对**。
 * （闭集与 14 码表一样，**只能有一份真相** ⇒ 14 码表是 import P1 的，
 * 字段闭集因 P1 侧没有对应物而在此转写 —— 两者的理由都写在各自注释里。）
 */
export const ERROR_RESULT_FIELDS = Object.freeze([
  'kind',
  'ok',
  'code',
  'retryable',
  'userNext',
  'requestId',
  'logPath',
  'detail',
]);

/**
 * 按协议 §1.1 判两条快照**是否同组**（**判据自算**）。
 *
 * 这是「标准答案」的来源：实现返回什么**不影响**这个函数的结论。
 *
 * @param {object} a - 快照对象。
 * @param {object} b - 快照对象。
 * @returns {boolean}
 */
export function sameGroupByProtocol(a, b) {
  return GROUP_KEY_FIELDS.every((field) => String(a?.[field] ?? '') === String(b?.[field] ?? ''));
}

/**
 * 取一次正文（**把「取不到」变成一个可判的事实**）。
 *
 * **为什么不让它抛**：内核在发送路径上 await `serialize`，读取失败 / 快照失效时
 * 它会 **reject**——那正是「发不出去」的实现事实 ⇒ 判据必须把它断成**红**，
 * 而不是自己抛错（那会被当成「判据自身的问题」，归因就错了）。
 *
 * @param {object} adapter
 * @param {string} ref
 * @returns {Promise<{ok: boolean, body: string, error: string}>}
 */
async function trySerialize(adapter, ref) {
  try {
    return { ok: true, body: String(await adapter.serialize(ref)), error: '' };
  } catch (error) {
    return { ok: false, body: '', error: String(error?.message ?? error) };
  }
}

/**
 * 判据自身的抛错兜底包装。
 *
 * **与「实现尚未到位」区分开**：本文件抛错一律带「判据自身：」前缀 ⇒
 * 交付回执里能一眼分清「是判据的问题」还是「是实现的问题」。
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
    return verdict(name, FAIL, `判据自身：抛错 ${String(error?.stack ?? error)}`);
  }
}

/**
 * 等某一条提示出现（客户端的核对是**异步**的：选中引用后才去问 host）。
 *
 * @param {object} adapter
 * @param {string} kind - 提示的 kind（`duplicate-groups` / `budget-exceeded` / `duplicates-failed`）。
 * @param {object} [options]
 * @param {number} [options.from] - 只看第 `from` 条**之后**的提示（区分场景）。
 * @param {number} [options.timeoutMs]
 * @returns {Promise<object|undefined>}
 */
/**
 * 等提示的**兜底预算**（毫秒）。
 *
 * **为什么放大到 30s**：机器被压满时，一轮核对（含 host 往返）可能远超 2.5s ⇒
 * 旧预算会**提前放弃** ⇒ 拿到一个**假红**（PM 2026-10-21 02:0x 登记的缺口）。
 * ⇒ **慢不是错，等不到才是错** ⇒ 预算放大；「等不到」与「没发生」在红字里**分开报**
 * （见 `noticeTimeoutWhy`）。
 *
 * **预算由 adapter 给**（`adapter.waitBudgetMs?.()`）：真机 adapter 走本默认值；
 * **合成适配器是确定性的**（提示要么被脚本投出、要么永远没有）⇒ 给短预算，
 * 免得「故意不出提示」的负例各等 30s ⇒ **门禁不被拖慢**。
 */
const NOTICE_WAIT_BUDGET_MS = 30_000;

/**
 * 取本次等待的兜底预算（毫秒）。
 *
 * @param {object} adapter
 * @param {number|null} [override] - 显式指定（优先）。
 * @returns {number}
 */
function noticeWaitBudget(adapter, override = null) {
  const given = Number(override ?? adapter?.waitBudgetMs?.() ?? NOTICE_WAIT_BUDGET_MS);
  return Number.isFinite(given) && given > 0 ? given : NOTICE_WAIT_BUDGET_MS;
}

/**
 * **等待类失败的超时口径**（PM 2026-10-21 02:5x **扩派单③①**；02:8x ⑤ **UX 定稿**收口）。
 *
 * ## 句式：现象 → 条件 → 结论，**一条直线**（UX 定稿）
 *
 * > 等预算内未等到 X 提示（**疑似负载过高**）；**若在低负载下稳定复现，才按漏报论**
 *
 * - 括号里是**现象的候选解释**（「疑似负载过高」）⇒ **不写「不是漏报」**：
 *   那样会与后半句「才按漏报论」**字面矛盾**（前半句断言不是、后半句说是）⇒
 *   读者不知道该信哪一句。
 * - 分号后是**条件 ＋ 结论**（低负载下能稳定复现 ⇒ 才按漏报论）⇒ 排查指引不丢。
 *
 * **不许**再把超时写成「没有出现 X 提示」——那是**误读**的来源，不是事实：
 * 超时只说明「预算内没等到」，**没说明**「实现没发」。
 *
 * @param {object} adapter - 适配器（决定本次的兜底预算）。
 * @param {string} what - 等的是哪条提示（人话，可直接嵌进句子）。
 * @param {object} [options]
 * @param {string} [options.verdict] - 括号里的分辨语（默认「疑似负载过高」）。
 * @param {string} [options.tail] - 分号后的「条件 ＋ 结论」
 *   （默认「若在低负载下稳定复现，才按漏报论」；个别判据可**覆盖**成自己的排查指引）。
 * @param {number|null} [options.budgetMs] - 显式预算（默认取 adapter 的）。
 * @returns {string}
 */
export function noticeTimeoutWhy(
  adapter,
  what,
  {
    verdict = '疑似负载过高',
    tail = '若在低负载下稳定复现，才按漏报论',
    budgetMs = null,
  } = {},
) {
  const budget = noticeWaitBudget(adapter, budgetMs);
  return `等预算（${budget / 1000}s）内未等到${what}（${verdict}）；${tail}`;
}

/**
 * 这条提示是不是「**这一轮**」的那条（**事件式信号①**：`trigger` ＝ 对应 pick 的 ref）。
 *
 * ## 为什么用 `trigger` 定位「哪一轮」
 *
 * 等待要区分「**这一轮**的提示」与「**别的轮次**的提示」⇒ 靠序号猜不可靠
 * （机器被压满时轮次间隔会变长）⇒ 改用提示**自带的**事实：`trigger` 就是
 * 「这条提示由哪次新增引用触发」。
 * （实现侧事实：`client/client.js:794` 的 `checkDuplicates(ctx, sessions,
 * sourceSessionId, snapshotId)`，`snapshotId` ＝ **新选中的那条** ⇒ 提示里的
 * `trigger: triggerRef` ＝ **判据那次 pick 返回的 ref**
 * ⇒ 真机 adapter 的 `pick()` 返回 `insert.ref` ＝ 同一条 ⇒ 两边对得上。）
 *
 * **判据只用它定位「哪一轮」，不断言它的取值口径**（协议稿里没有 `trigger` 这个
 * 字段 ⇒ 它是**实现事实**，不是协议事实 ⇒ 不用来「断值」，只用来「归属」）。
 *
 * **没带 `trigger` 的**（`null` / 空 / 缺字段）⇒ **无法归属** ⇒ **照样接受**
 * ⇒ 不制造假红（实现侧 `client.js:1144` 就是 `trigger: triggerRef ?? null`）。
 *
 * @param {object} notice
 * @param {string} lastRef - 这一轮的触发引用（对应 pick 的 ref）；空串＝不按轮次过滤。
 * @returns {boolean}
 */
function isRoundNotice(notice, lastRef) {
  if (lastRef === '') return true;
  const trigger = String(notice?.trigger ?? '');
  if (trigger === '') return true;
  return trigger === lastRef;
}

/**
 * **这一轮到底有没有出过提示**（事件证据）。
 *
 * 超时红字若只有一句「疑似负载过高」，就分不出两种完全不同的情况：
 * ① 这一轮**压根没跑起来**（真·负载问题）② 这一轮**跑了、也出了提示**，只是没出
 * 预期的那条（⇒ 指向**实现漏报**，不是负载）⇒ 用提示自带的 `trigger` 把两者分开。
 *
 * @param {object} adapter
 * @param {object} [options]
 * @param {number} [options.from] - 只看第 `from` 条之后的提示。
 * @param {string} [options.lastRef] - 这一轮的触发引用。
 * @returns {string} 证据短语（空串＝没有证据）。
 */
function noticeRoundEvidence(adapter, { from = 0, lastRef = '' } = {}) {
  if (lastRef === '') return '';
  const notices = (adapter.visibleNotices?.() ?? []).slice(from);
  const kinds = [];
  for (const notice of notices) {
    if (String(notice?.trigger ?? '') !== lastRef) continue;
    const kind = String(notice?.kind ?? '');
    if (kind !== '' && !kinds.includes(kind)) kinds.push(kind);
  }
  return kinds.join('、');
}

/**
 * **等待类失败的超时红字**（共享口径 **或** 事件证据，**互斥**）。
 *
 * ## 两层**互斥**，不并排（PM 2026-10-21 02:8x ⑤ 的口径）
 *
 * 1. **有事件证据**（这一轮**确实出过别的提示**）⇒ 说明**这一轮跑了** ⇒
 *    「疑似负载过高」这个解释**被证据排除** ⇒ 结论落在**实现漏报**上
 *    ⇒ 红字＝**现象 ＋ 事实段（＋ 结论段）**：
 *    「等预算内未等到 X 提示（该轮确实出过提示：kind ⇒ 指向实现漏报，不是负载）」
 *    ⇒ **不出**通用后半句（「才按漏报论」）——它与更强的结论**叠读打架**。
 * 2. **没有事件证据**（这一轮压根没出提示）⇒ 只能停在**候选解释**上 ⇒
 *    走共享口径 `noticeTimeoutWhy()`：「（疑似负载过高）；若在低负载下稳定复现，才按漏报论」。
 *
 * ## 证据层 ＝ **事实段 ＋ 结论段**（结论段**可省**）
 *
 * - **事实段** ＝ `该轮确实出过提示：${evidence}`——**只陈述事实**，不含结论。
 * - **结论段** ＝ ` ⇒ 指向实现漏报，不是负载`。
 * - **该处自带结论**（`tailAlways`＝真，如 P2-5 那处）⇒ **只拼事实段**、
 *   **不拼结论段**：否则红字会**先下结论、再说要分辨**（＝逻辑倒置，
 *   与共享口径那套「候选解释 → 条件结论」不同款 ⇒ 同一机制里出现两套逻辑）。
 * - **通用形态**（`tailAlways`＝假）⇒ **两段都拼**，逐字不变。
 *
 * @param {object} adapter
 * @param {string} what - 等的是哪条提示。
 * @param {object} [options]
 * @param {number} [options.from]
 * @param {string} [options.lastRef]
 * @param {string} [options.verdict] - 无证据时的分辨语（默认「疑似负载过高」）。
 * @param {string} [options.tail] - 「条件 ＋ 结论」（可按判据**覆盖**）。
 * @param {boolean} [options.tailAlways] - **该处专属**的排查指引 ⇒ **始终写**
 *   （即使有事件证据也写）——它说的是「**这一处**要分辨什么」，不是通用兜底结论。
 *   ⇒ 同时表示「**该处自带结论**」⇒ 有事件证据时**只拼事实段**、不拼结论段
 *   （否则红字「先下结论、再说要分辨」＝ 逻辑倒置）。
 * @param {number|null} [options.budgetMs]
 * @returns {string}
 */
function noticeTimeoutDetail(
  adapter,
  what,
  {
    from = 0,
    lastRef = '',
    verdict = '疑似负载过高',
    tail = '若在低负载下稳定复现，才按漏报论',
    tailAlways = false,
    budgetMs = null,
  } = {},
) {
  const evidence = noticeRoundEvidence(adapter, { from, lastRef });
  if (evidence !== '') {
    // **证据层单独成立** ⇒ 现象 ＋ 事实段（**不**出通用后半句）。
    const budget = noticeWaitBudget(adapter, budgetMs);
    // **事实段**：只陈述「这一轮确实出过什么提示」——**不含结论**。
    const fact = `该轮确实出过提示：${evidence}`;
    // **结论段**：只在**通用形态**（该处不带自己的 tail）时拼。
    // （该处自带结论 ⇒ 证据层**只陈述事实**；否则红字会「先下结论、
    //   再说要分辨」＝ 逻辑倒置，与共享口径那套「候选解释 → 条件结论」不同款。）
    const conclusion = tailAlways ? '' : ' ⇒ 指向实现漏报，不是负载';
    const guide = tailAlways ? `；${tail}` : '';
    return (
      `等预算（${budget / 1000}s）内未等到${what}` +
      `（${fact}${conclusion}）${guide}`
    );
  }
  return noticeTimeoutWhy(adapter, what, { verdict, tail, budgetMs });
}

/**
 * 等某一条提示出现（客户端的核对是**异步**的：选中引用后才去问 host）。
 *
 * **事件式**：只认「**这一轮**」的那条（`trigger` ＝ 对应 pick 的 ref，见
 * `isRoundNotice()`）⇒ 不会把**别的轮次**的提示当成这一轮的。
 * 兜底预算见 `noticeWaitBudget()`（真机 30s / 合成 500ms / 活体注入 1s）。
 *
 * @param {object} adapter
 * @param {string} kind - 提示的 kind（`duplicate-groups` / `budget-exceeded` / `duplicates-failed`）。
 * @param {object} [options]
 * @param {number} [options.from] - 只看第 `from` 条**之后**的提示（区分场景）。
 * @param {number} [options.timeoutMs]
 * @param {string} [options.lastRef] - 这一轮的触发引用（事件式信号①）。
 * @returns {Promise<object|undefined>}
 */
async function waitForNotice(
  adapter,
  kind,
  { from = 0, timeoutMs = null, lastRef = '' } = {},
) {
  const deadline = Date.now() + noticeWaitBudget(adapter, timeoutMs);
  while (Date.now() < deadline) {
    const notices = (adapter.visibleNotices?.() ?? []).slice(from);
    const hit = notices.find(
      (notice) => String(notice?.kind ?? '') === kind && isRoundNotice(notice, lastRef),
    );
    if (hit !== undefined) return hit;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return undefined;
}

/**
 * 等「**最后一轮**」的那条提示（多组场景用）—— **事件式**，不用墙钟猜。
 *
 * ## 为什么要换掉「等组数稳定 400ms」
 *
 * 原实现用**墙钟**判定「轮次已跑完（组数不再增长）」⇒ 机器被压满时，两轮之间的
 * 间隔可能超过 400ms ⇒ **提前认定跑完** ⇒ 拿到**中间那条**（只有 1 个组）⇒
 * 红字写「只报了 1 个组（应有 2 个）」⇒ **把负载问题说成实现漏报**
 * （PM 2026-10-21 02:0x 登记的缺口：重负载下会假红，且红字误导）。
 *
 * ## 事件式的完成信号（任一命中即视为「最后一轮已跑完」）
 *
 * 1. **`trigger` ＝ 最后一次 pick 的 ref**（最可靠）：提示载荷里的 `trigger` 是
 *    「**这条提示由哪次新增引用触发**」⇒ 它等于最后一次 pick ⇒ 那一轮确实跑完了。
 *    （实现侧事实：`client/client.js` 的 `checkDuplicates(ctx, sessions, sessionId,
 *    triggerRef)`，`triggerRef` ＝ 新选中的那条 ⇒ 提示里的 `trigger: triggerRef`。
 *    **判据只用它定位「哪一轮」，不断言它的取值口径**。）
 * 2. **`groups.length` ＝ 期望组数**（报满）：组数到顶 ⇒ 终态。
 *
 * 两条都命中不了 ⇒ **超预算** ⇒ 红字只说「疑似负载过高」，**不**断言漏报
 * （PM 的交办方向②：别把超时说成「只报了 N 个组」）。
 *
 * ## `expectedGroups` 传 0 时**只剩信号①**（**有意为之，不是漏写**）
 *
 * 信号②要拿 `groups.length` **等于某个期望值**才算命中 ⇒ **单组 / 单条**场景里
 * `groups.length` 恒为 1 ⇒ 期望值传 1 也会「碰巧命中」＝**没有判别力** ⇒
 * 那种场景应当**传 0**（= 不参与信号②，只剩信号① `trigger`）。
 * （PM 2026-10-21 02:7x 已核：P2-2 / P2-5 / P2-6 / P2-J 都是单组/单条场景 ⇒
 * **那四条只实现信号①** ⇒ 走 `waitForNotice()` / `waitForBudgetNotice()` 的
 * `lastRef` 归属，**不**照抄本函数的双信号结构留死代码。
 * 本函数只服务 **P2-H**（**多组**场景）⇒ 那里 `expectedGroups` ＝ 期望组数
 * （≥ 2）⇒ 信号②**有判别力**、是有效的第二道完成信号。）
 *
 * @param {object} adapter
 * @param {string} kind - 提示的 kind。
 * @param {object} [options]
 * @param {string} [options.lastRef] - 最后一次 pick 的 ref（完成信号①）。
 * @param {number} [options.expectedGroups] - 期望组数（完成信号②）；**0 ＝ 不参与**
 *   （单组/单条场景 ⇒ 只剩信号①，**有意为之**）。
 * @param {number} [options.timeoutMs] - 总预算（**兜底**，不是「稳定」的判据）。
 * @returns {Promise<{notice: object|undefined, maxGroups: number, count: number, timedOut: boolean}>}
 */
async function waitForLastRoundNotice(
  adapter,
  kind,
  { lastRef = '', expectedGroups = 0, timeoutMs = 30_000 } = {},
) {
  const deadline = Date.now() + timeoutMs;
  let maxGroups = 0;
  let count = 0;
  while (Date.now() < deadline) {
    const notices = (adapter.visibleNotices?.() ?? []).filter(
      (notice) => String(notice?.kind ?? '') === kind,
    );
    count = notices.length;
    for (const notice of notices) {
      const size = Array.isArray(notice?.groups) ? notice.groups.length : 0;
      if (size > maxGroups) maxGroups = size;
    }
    const done =
      (lastRef !== '' && notices.some((notice) => String(notice?.trigger ?? '') === lastRef)) ||
      (expectedGroups > 0 &&
        notices.some(
          (notice) => Array.isArray(notice?.groups) && notice.groups.length === expectedGroups,
        ));
    if (done) {
      return { notice: notices[notices.length - 1], maxGroups, count, timedOut: false };
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return { notice: undefined, maxGroups, count, timedOut: true };
}

/**
 * 等「**带出预算事实**」的那条提示出现。
 *
 * **按数据找，不按 kind 找**：协议 §3.1-2 要求的是「出现预算提示」，
 * **不是**某一种 kind 名；而且内核 notice 是**单槽**（最新一条覆盖旧条）⇒
 * 实现可能把预算信息**并进**同组提示里。
 *
 * @param {object} adapter
 * @param {object} [options]
 * @param {number} [options.from]
 * @param {number} [options.timeoutMs]
 * @param {string} [options.lastRef] - 这一轮的触发引用（事件式信号①）。
 * @returns {Promise<object|undefined>}
 */
async function waitForBudgetNotice(
  adapter,
  { from = 0, timeoutMs = null, lastRef = '' } = {},
) {
  const deadline = Date.now() + noticeWaitBudget(adapter, timeoutMs);
  while (Date.now() < deadline) {
    const notices = (adapter.visibleNotices?.() ?? []).slice(from);
    const hit = notices.find(
      (notice) => notice?.budget?.wouldExceed === true && isRoundNotice(notice, lastRef),
    );
    if (hit !== undefined) return hit;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return undefined;
}

/**
 * 提示的**承载位**检查（协议 §3.2）。
 *
 * · `delivered` ⇒ 这条提示经**输入区通道**投出去了
 *   （内核 `conversation.input.for(actx).notify(level, text)`）
 *   ⇒ 承载位 = 输入区常驻条（不是浏览器 toast）。
 * · `level === 'info'` ⇒ 内核渲染 inline 常驻条 ⇒ **可复查**。
 *   （**按协议 v0.5 的渲染侧更正**：`client.js:18807-18809` 是**独立 `useEffect`**
 *   ⇒ 准确说法是「`error` 档**额外**弹一个瞬态 toast、**不**渲染 inline 常驻条；
 *   `info` 档**只**渲染 inline 常驻条」⇒ 本批必须用 `info` 档的**理由不变**：
 *   要常驻条、不要瞬态 toast。）
 *
 * @param {object} notice
 * @returns {{ok: boolean, why: string}}
 */
function checkNoticeCarrier(notice) {
  if (notice?.delivered !== true) {
    return { ok: false, why: '提示没经输入区通道投递 ⇒ 用户看不见（协议 §3.2：承载位 = 输入区常驻条）' };
  }
  if (String(notice?.level ?? '') !== 'info') {
    return {
      ok: false,
      why:
        `提示档位是 ${String(notice?.level ?? '')}——协议 §3.2 要求本批所有提示用 info 档` +
        '（按 v0.5 的渲染侧更正：error 档**额外**弹一个瞬态 toast、**不**渲染 inline 常驻条' +
        '⇒ 没有常驻条可复查 ⇒ 当场违反「不接受瞬态 toast」）',
    };
  }
  return { ok: true, why: '' };
}

/**
 * 协议 §3.2 错误态：文案**不含**这三个词（发送结果字样）。
 *
 * 提在模块级 ⇒ P2-6（失败文案）与 P2-J（发送后仍留在用户眼前的那条）**共用同一份**，
 * 免得两处各抄一份、日后改一处忘一处。
 */
const FORBIDDEN_SEND_RESULT_WORDS = ['已发送', '发送失败', '发送被阻止'];

/**
 * 协议 §3.2「抗 stale 的锚」：**条会被内核顶掉、且撤不掉** ⇒ 文案**不得**写成
 * 「当前状态」的断言（协议点名用「本次核对」不用「现在」）。
 *
 * 「当前」一并禁：它同样把**历史核对**说成**当前状态**（协议禁止的是「当前状态
 * 的断言」这个**形状**，不只是「现在」这个词）。
 * ⚠️ 这是**词面近似**：协议只点名了「现在」⇒ 若将来协议放宽口径，**判据要同步放宽**
 * （放宽必须配反向用例）。
 */
const STALE_ASSERTION_WORDS = ['现在', '当前'];

/**
 * 断「**文案自包含**」（协议 §3.2）。
 *
 * 内核 notice 是**单槽**（最新一条覆盖旧条），而且内核随时可能发一条 error
 * notice 把我们的 inline 条**整个顶掉**（协议 v0.5 §3.2 的第六处裁定）⇒
 * **每一条提示都必须自包含全部事实**，不能依赖「另一条还在」或「内核没插一脚」。
 *
 * 只判**数字在不在**，不判动词措辞（成品文案由 UX 定稿，协议 §3.2 不锁措辞）；
 * 且数字**必须落在该组的语境片段内**（PM 2026-10-21 01:4x 授权再收紧）。
 *
 * @param {string} text - 提示文案。
 * @param {{members: number, redundant: number}} facts - 该条提示必须带的事实。
 * @returns {{ok: boolean, why: string}}
 */
/**
 * 定位**某个组的语境片段**（协议 §3.2「数字来源」＋「多组形态」）。
 *
 * ## 为什么需要片段（PM 2026-10-21 01:4x 授权再收紧）
 *
 * 只做**全文**匹配 ⇒「**组数恰好等于条数**」时会误命中：2 组各 2 条 ⇒ 文案里
 * 「2 组重复」那个 `2` 会被当成「含条数」⇒ 一条**漏写「第 1 组 2 条」**的文案
 * 也被判成自包含 ⇒ **又一处假绿口子**。
 * ⇒ 判据须**在该组的语境片段内**匹配该组条数与该组 R。
 *
 * ## 片段怎么切
 *
 * 成品文案是**分句**的：多组形态里**每组一句**（`第 1 组 …；第 2 组 …`），
 * 组数与预算各占一句 ⇒ 以 **`；`** 切分即得「每组一句」。
 *
 * ## 组锚点（协议 §3.2 定稿形态）
 *
 * · **多组形态**：`第 i 组`（先说组数、再逐组编号）。
 * · **单组形态**：`同组`（`同组 N 条内容完全相同`）。
 *
 * 两种形态都取**「含锚点的那一句」**（**不是**从锚点往后截）⇒ 锚点写在
 * 「2 条同组引用」这种语序时也不假红——判据只关心**数字属于哪一句**。
 *
 * ## 找不到锚点 ⇒ 判红（fail closed）
 *
 * 连组锚点都没有 ⇒ 看不出哪一句在说这一组 ⇒ **无法确认条数落在哪一组**
 * ⇒ 判红（不是放过）。
 *
 * @param {string} text - 提示文案。
 * @param {number} ordinal - 组号（1 起）。
 * @returns {{ok: boolean, clause: string, why: string}}
 */
function groupContextClause(text, ordinal = 1) {
  const clauses = String(text ?? '')
    .split('；')
    .map((clause) => clause.trim())
    .filter((clause) => clause !== '');
  const numbered = clauses.filter((clause) => /第\s*\d+\s*组/.test(clause));
  if (numbered.length > 0) {
    const hit = numbered.find((clause) => new RegExp(`第\\s*${ordinal}\\s*组`).test(clause));
    if (hit === undefined) {
      return {
        ok: false,
        clause: '',
        why: `文案里没有「第 ${ordinal} 组」这一句（多组时必须逐组列，协议 §3.2）`,
      };
    }
    return { ok: true, clause: hit, why: '' };
  }
  const single = clauses.find((clause) => clause.includes('同组'));
  if (single === undefined) {
    return {
      ok: false,
      clause: '',
      why: '文案里既没有「第 i 组」也没有「同组」⇒ 看不出哪一句在说这一组',
    };
  }
  return { ok: true, clause: single, why: '' };
}

/**
 * 文案里有没有「**独立出现 + 带单位词**」的这个数字（不是别的数字的一部分）。
 *
 * ## 为什么不能用 `String(x).includes(...)`（**子串包含有假绿口子**）
 *
 * 协议 v0.6 的成品文案形如「同组 2 条内容完全相同，重复可省 4216 字节」——
 * `text.includes('2')` 会被「**4216**」里的 `2` 满足 ⇒ 一条**只写了字节数**、
 * 根本没写条数的文案，也会被判成「文案自包含」⇒ **假绿**。
 * （PM 2026-10-21 01:3x 登记为「已知、不算缺陷」，是否收紧由 tester 决定 ⇒ **收紧**。）
 *
 * ## 为什么**还要带单位词**（PM 2026-10-21 01:4x）
 *
 * 只要求「独立出现」仍不够：**组数**那个数字也是独立出现的（`2 组重复`）⇒
 * 它会顶替条数 ⇒ **又一处假绿口子**。⇒ 数字必须**紧跟自己的单位词**：
 * 条数跟「条」、字节数跟「字节」⇒「2 组」永远顶不了「2 条」。
 *
 * ## 边界怎么定
 *
 * 数字的**前面**不能是数字或千分位逗号：
 * · `'4216'` 里的 `'2'` 前面是 `'4'`（数字）⇒ 排除 ✔
 * · `'4,216'` 里的 `'2'` 前面是 `','`（千分位）⇒ 排除 ✔
 * · 独立出现的 `'3'`（前面是空格/中文）⇒ 命中 ✔
 *
 * 数字的**后面**由单位词定死（`条` / `字节`）⇒ 不会出现 `'421'` 顶替 `'42'`。
 *
 * ## 千分位形态
 *
 * 协议 v0.6 明写「**数字一律不加千分位**」⇒ 文案里是裸数字；这里**同时**接受
 * 千分位形态（`'4,216'`）⇒ 万一实现将来加了千分位，判据不会假红。
 *
 * @param {string} text - 待查文案（**已限定在该组语境片段内**）。
 * @param {number} value - 要找的数字。
 * @param {string} unit - 单位词（`条` / `字节`）。
 * @returns {boolean}
 */
function hasNumberWithUnit(text, value, unit) {
  const number = Number(value);
  if (!Number.isFinite(number)) return false;
  const plain = String(Math.abs(number));
  const grouped = plain.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  for (const form of new Set([plain, grouped])) {
    const escaped = form.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    if (new RegExp(`(?<![\\d,])${escaped}\\s*${unit}`).test(String(text ?? ''))) return true;
  }
  return false;
}

export function checkSelfContainedText(text, { members, redundant }) {
  const segment = groupContextClause(text, 1);
  if (!segment.ok) {
    return {
      ok: false,
      why:
        `${segment.why}——协议 §3.2：条数（${members}）与字节数（${redundant}）必须落在` +
        '**该组的语境片段**内（单组＝「同组 N 条」、多组＝「第 i 组 N 条」）',
    };
  }
  const clause = segment.clause;
  if (!hasNumberWithUnit(clause, members, '条')) {
    return {
      ok: false,
      why:
        `该组那一句里没有「${members} 条」——协议 §3.2：文案必须自包含该组条数；` +
        '**子串不算**（「只写了字节数、条数恰好出现在字节数里」不算数）、' +
        '**别的组的条数不算**（条数必须落在该组那一句里）',
    };
  }
  if (!hasNumberWithUnit(clause, redundant, '字节')) {
    return {
      ok: false,
      why:
        `该组那一句里没有「${redundant} 字节」——协议 §3.2：文案必须自包含该组的字节数；` +
        '**子串不算**',
    };
  }
  return { ok: true, why: '' };
}

// ---------------------------------------------------------------------------
// P2-1｜同组 ⇒ 仍是两个不同 snapshotId、都落盘、都可发
// ---------------------------------------------------------------------------

/**
 * P2-1｜§5 第 1 行 / §1.1 / §1.2 / §1.3。
 *
 * **同组（键全等）⇒ 仍是两个不同 `snapshotId`、都落盘、都可发**——
 * 不得退化成「复用旧 id」（那会把两次捕获事实抹成一次）。
 *
 * ## 为什么**冻结时钟**
 *
 * §1.1 的键含 `capturedAt`（毫秒精度）⇒ 两次独立捕获几乎必然不同 ⇒
 * 「同组成立」这个前提**必须**靠冻结时钟才造得出来。
 * **这是造输入，不是造标准答案**：期望的「成组」仍由本库按五字段规则自算。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_1_sameGroupKeepsDistinctIds(adapter) {
  return guard('P2-1-same-group-distinct-ids', async () => {
    const sessionId = 'p2-check-same';
    adapter.setNow?.(1_767_000_000_000);
    const first = await adapter.pick({ sessionId });
    const second = await adapter.pick({ sessionId });
    if (!first?.ref || !second?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    await adapter.settle(first.ref);
    await adapter.settle(second.ref);

    // ① 两个不同 `snapshotId`（§5 第 1 行的核心：**不得复用旧 id**）。
    if (first.ref === second.ref) {
      return {
        status: FAIL,
        detail: `两次 pick 得到同一个 ref（${first.ref}）——成组后退化成了「复用旧 id」`,
      };
    }

    // ② 都落盘。
    const fileA = adapter.snapshotFile(first.ref);
    const fileB = adapter.snapshotFile(second.ref);
    if (!fileA?.exists) {
      return { status: FAIL, detail: `第一份快照没落盘（${first.ref}）` };
    }
    if (!fileB?.exists) {
      return { status: FAIL, detail: `第二份快照没落盘（${second.ref}）——被第一份覆盖了` };
    }

    // ③ 都可发：正文取得到，且与盘上**逐字节**相同（P1 S-4 的同一把尺）。
    const bodyA = await trySerialize(adapter, first.ref);
    const bodyB = await trySerialize(adapter, second.ref);
    if (!bodyA.ok) {
      return { status: FAIL, detail: `第一份的正文取不到（发不出去）：${bodyA.error}` };
    }
    if (!bodyB.ok) {
      return { status: FAIL, detail: `第二份的正文取不到（发不出去）：${bodyB.error}` };
    }
    if (bodyA.body !== String(fileA.body)) {
      return { status: FAIL, detail: '第一份的正文与落盘内容不一致（发不出去）' };
    }
    if (bodyB.body !== String(fileB.body)) {
      return { status: FAIL, detail: '第二份的正文与落盘内容不一致（发不出去）' };
    }

    // ④ 前提核对：**本库自己**按 §1.1 算同组（不拿实现的输出当标准答案）。
    if (!sameGroupByProtocol(fileA.snapshot, fileB.snapshot)) {
      const diff = GROUP_KEY_FIELDS.filter(
        (field) => String(fileA.snapshot?.[field] ?? '') !== String(fileB.snapshot?.[field] ?? ''),
      );
      return {
        status: FAIL,
        detail: `判据自身：造出来的两条不满足「键全等」（差在 ${diff.join('/')}）⇒ 前提不成立`,
      };
    }

    // ⑤ 实现侧也要把它们认成同一组（§1.1）。
    const result = await adapter.duplicatesFor({ sessionId, snapshotIds: [first.ref, second.ref] });
    if (!result.ok) {
      return { status: FAIL, detail: `duplicates 核对失败（code=${result.code}）——同组的两条没被核对` };
    }
    if (result.groups.length !== 1) {
      return {
        status: FAIL,
        detail: `键全等的两条没被认成同一组（groups=${result.groups.length}）——协议 §1.1`,
      };
    }
    const members = Array.isArray(result.groups[0]?.members) ? result.groups[0].members : [];
    if (members.length !== 2) {
      return { status: FAIL, detail: `同组成员数不是 2（${members.length}）` };
    }
    const ids = members.map((member) => String(member?.snapshotId ?? ''));
    if (!ids.includes(first.ref) || !ids.includes(second.ref)) {
      return {
        status: FAIL,
        detail: `同组成员不是那两个 id（${ids.join(' / ')}）——成员被合并了（协议 §1.3：成员不合并）`,
      };
    }

    // ⑥ §1.2 的组标识：**形状 + 稳定 + 与协议独立算出的值一致**。
    //
    //    · **形状**：16 位小写十六进制；
    //    · **稳定**：同组两次一致；
    //    · **值**：协议 v0.3 已把分隔符裁定为**单个换行 U+000A、不含空格**
    //      ⇒ 期望值**可以**从协议稿独立转写 ⇒ 判据得以钉**具体值**
    //      （不钉的话，「实现把分隔符写成 ` \n `」这类偏差判不出来）。
    const keyA = adapter.groupKeyOf(fileA.snapshot);
    const keyB = adapter.groupKeyOf(fileB.snapshot);
    if (!GROUP_KEY_SHAPE.test(keyA)) {
      return {
        status: FAIL,
        detail: `组标识形状不对（${keyA}）——协议 §1.2 要 16 位小写十六进制`,
      };
    }
    if (keyA !== keyB) {
      return { status: FAIL, detail: '同组两条的组标识不一致——协议 §1.2：组标识要稳定' };
    }
    const wantKey = expectedGroupKey(fileA.snapshot);
    if (keyA !== wantKey) {
      return {
        status: FAIL,
        detail:
          `组标识与协议 §1.2 算出的不符（实现 ${keyA} / 按协议算 ${wantKey}）——` +
          '§1.2 ＋ v0.3 裁断：五字段用**单个换行 U+000A、不含空格**连接后取 sha256 前 16 位',
      };
    }

    return {
      status: PASS,
      detail:
        `同组（键全等）⇒ 两个不同 id（${first.ref} / ${second.ref}）、都落盘、都可发；` +
        `组标识 ${keyA}（16 位小写十六进制、同组一致、且与按协议 §1.2 独立算出的值相同）`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-2｜同组成立 ⇒ 提示可见且可复查
// ---------------------------------------------------------------------------

/**
 * P2-2｜§5 第 2 行 / §3.1-1 / §3.2。
 *
 * 同组成立 ⇒ 提示**可见**（用户能看见）、**可复查**（不是瞬态 toast），
 * 承载位 = **输入区常驻条**，且 `level` 必须是 `info`；提示里要有
 * **同组条数**与**字节数**。
 *
 * ## 判据怎么看得见「承载位」
 *
 * · `delivered` ⇒ 这条提示经**输入区通道**投出去了
 *   （内核 `conversation.input.for(actx).notify(level, text)`）；
 * · `level === 'info'` ⇒ 内核渲染 inline 常驻条；`error` 档**不**渲染 inline 常驻条、
 *   只**额外**弹一个瞬态 toast（协议 §3.2 引的内核契约 ＋ **v0.5 的渲染侧更正**：
 *   `dsh-client-ui-conversation/lib/client.js`
 *   SHA `8554738CBEEB5218AD1BFA4773EF094363B066D052A1893D7A96797C5BC33A63`
 *   的 `:18982-18986` 只渲染 `level === 'info'`；`:18807-18809` 是**独立 `useEffect`**，
 *   对 `error` 档 `showToast` ⇒ 准确说法是「error 档**额外**弹瞬态 toast，
 *   info 档**只**渲染 inline 常驻条」）。
 *
 * ## 判「数据」也判「文案自包含」
 *
 * 条数与字节数判**结构化数据**（`groups`）；另外 §3.2 明确要求「内核 notice 是
 * 单槽 ⇒ 每条提示的文案必须自包含（条数与字节数写在同一条里）」⇒ 文案里
 * **必须出现这两个数**（不判措辞，只判数字在不在）。
 *
 * ## 判「可复现」（协议 v0.5 §3.2 的第六处裁定）
 *
 * 内核 `notices.set` 有两个入口、且内核多处用 `notify('error', …)` ⇒ **内核会
 * 把我们的 inline 条整个顶掉**（单槽，最新覆盖旧条）⇒ 我们的提示**不保证常驻**
 * ⇒ 协议明写「**不得假设提示会一直在**」，信息不永久丢失靠的是
 * 「**再动一次引用 ⇒ 重新触发一轮核对、重新发出一条提示**」。
 * ⇒ 本判据断的正是这条：**再动一次引用 ⇒ 又产生一条 info 档、自包含的提示**。
 *
 * **为什么这条能机检、而「常驻」不能**：再触发由实现自己驱动（客户端每次新增
 * 引用都会重跑一轮核对）⇒ 观测得到；而「提示会不会被内核顶掉」取决于内核与
 * 用户的时序 ⇒ **不可机检** ⇒ 协议本身就明写**不得假设**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_2_duplicateNoticeVisible(adapter) {
  return guard('P2-2-duplicate-notice-visible', async () => {
    const sessionId = 'p2-check-notice';
    adapter.setNow?.(1_767_000_000_000);
    const first = await adapter.pick({ sessionId });
    const second = await adapter.pick({ sessionId });
    if (!first?.ref || !second?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    await adapter.settle(first.ref);
    await adapter.settle(second.ref);

    const notice = await waitForNotice(adapter, 'duplicate-groups', { lastRef: second.ref });
    if (notice === undefined) {
      return {
        status: FAIL,
        detail:
          '同组成立后——协议 §3.1-1 要求出现「重复组」提示；' +
          noticeTimeoutDetail(adapter, '「重复组」提示', { lastRef: second.ref }),
      };
    }

    const carrier = checkNoticeCarrier(notice);
    if (!carrier.ok) {
      return { status: FAIL, detail: carrier.why };
    }

    // 数据：组 / 条数 / 字节数。
    const groups = Array.isArray(notice.groups) ? notice.groups : [];
    if (groups.length === 0) {
      return { status: FAIL, detail: '提示里没有组数据（协议 §3.2：要说清同组有几条）' };
    }
    const group = groups[0];
    const members = Array.isArray(group?.members) ? group.members : [];
    if (members.length !== 2) {
      return { status: FAIL, detail: `提示里的同组条数不对（${members.length}，应为 2）` };
    }
    const bodyBytes = Buffer.byteLength(String(adapter.snapshotFile(first.ref)?.body ?? ''), 'utf8');
    const redundant = Number(group?.redundantBytes ?? -1);
    if (redundant !== bodyBytes) {
      return {
        status: FAIL,
        detail: `提示里的字节数不对（冗余 ${redundant}，应为单条正文 ${bodyBytes}）——协议 §3.2`,
      };
    }

    // 文案自包含（§3.2：内核 notice 是单槽 ⇒ 同一条里必须带条数与字节数）。
    const selfContained = checkSelfContainedText(notice.text, {
      members: members.length,
      redundant,
    });
    if (!selfContained.ok) {
      return { status: FAIL, detail: selfContained.why };
    }

    // —— **可复现**（协议 v0.5 §3.2 硬要求③ ＋ 可用性推论）——
    //
    // 内核随时可能发一条 error notice 把我们的 inline 条**整个顶掉**（单槽）
    // ⇒ 我们的提示**不保证常驻** ⇒ 协议明写「**不得假设提示会一直在**」，
    // 信息不永久丢失靠的是「**用户再动一次引用** ⇒ 重新触发一轮核对、
    // 重新发出一条提示」。
    //
    // ⇒ 本判据断的正是这条：**再动一次引用 ⇒ 又产生一条 info 档、自包含的提示**。
    // ⇒ **刻意不断**「提示在两次触发之间一直存在」——那是协议明写**不得假设**的
    //   （内核会顶掉它 ⇒ 断了就是假红）。同理，判据读的是**客户端自己的记录**
    //   （`record('notice', …)`），**不去观测内核的 notice 槽** ⇒ 天然不依赖
    //   内核 toast/notice 的出现时机与顺序（v0.5 硬要求②）。
    const beforeRetrigger = (adapter.visibleNotices?.() ?? []).length;
    const third = await adapter.pick({ sessionId });
    if (!third?.ref) {
      return { status: FAIL, detail: '再动一次引用时 pick 未返回 ref——判据自身的复现前提不成立' };
    }
    await adapter.settle(third.ref);
    const again = await waitForNotice(adapter, 'duplicate-groups', {
      from: beforeRetrigger,
      lastRef: third.ref,
    });
    if (again === undefined) {
      return {
        status: FAIL,
        detail:
          '再动一次引用那一轮——协议 §3.2（v0.5）：内核会顶掉我们的提示，' +
          '信息不丢靠的是「再动一次引用就重新触发」⇒ 提示必须可复现；' +
          noticeTimeoutDetail(adapter, '「重复组」提示', {
            from: beforeRetrigger,
            lastRef: third.ref,
          }),
      };
    }
    const againCarrier = checkNoticeCarrier(again);
    if (!againCarrier.ok) {
      return { status: FAIL, detail: `重新触发的那条：${againCarrier.why}` };
    }
    const againGroups = Array.isArray(again.groups) ? again.groups : [];
    const againMembers = Array.isArray(againGroups[0]?.members) ? againGroups[0].members : [];
    if (againMembers.length < 2) {
      return {
        status: FAIL,
        detail: `重新触发的那条里同组条数不对（${againMembers.length}）——协议 §3.2：要说清同组有几条`,
      };
    }
    const againRedundant = Number(againGroups[0]?.redundantBytes ?? -1);
    const againSelfContained = checkSelfContainedText(again.text, {
      members: againMembers.length,
      redundant: againRedundant,
    });
    if (!againSelfContained.ok) {
      return { status: FAIL, detail: `重新触发的那条：${againSelfContained.why}` };
    }

    return {
      status: PASS,
      detail:
        `提示可见且可复查：输入区通道投递、level=info（常驻条，非瞬态 toast）；` +
        `含同组条数 ${members.length} 与字节数 ${redundant}；` +
        `再动一次引用可复现（重新出一条 info 档自包含提示，同组 ${againMembers.length} 条）`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-3｜capturedAt 不同 ⇒ 不成组（负例）
// ---------------------------------------------------------------------------

/**
 * P2-3｜§5 第 3 行 / §1.1。
 *
 * `capturedAt` 不同 ⇒ **不成组**（哪怕正文逐字节相同——两次不同的捕获事实）。
 *
 * ## 为什么走**命令层**
 *
 * 真实链路里 `capturedAt` 来自捕获时刻 ⇒ 造「不同」容易、造「相同」要冻结时钟；
 * 但**命令层能把五个字段全部控住** ⇒ 负例放在命令层，判得更准。
 *
 * ## 负例必须带**正对照**
 *
 * 只断「不成组」是不够的：实现若**一律不成组**（甚至一律失败），负例也会绿。
 * ⇒ 本判据先跑一组**键全等 ⇒ 必须成组**的正对照；对照不成立 ⇒ 负例无意义
 * ⇒ 直接报红（并写明是「对照组不成立」，不是「负例成立」）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_3_capturedAtNotGrouped(adapter) {
  return guard('P2-3-captured-at-not-grouped', async () => {
    const sessionId = 'p2-craft-captured-at';
    const text = 'P2 判据正文（capturedAt 场景）。';
    const at1 = '2026-10-20T08:59:00.000Z';
    const at2 = '2026-10-20T09:41:00.000Z';
    const base = adapter.makeSnapshot({ seq: 1, sessionId, branch: 'main', capturedAt: at1, text });
    const same = adapter.makeSnapshot({ seq: 2, sessionId, branch: 'main', capturedAt: at1, text });
    const diff = adapter.makeSnapshot({ seq: 3, sessionId, branch: 'main', capturedAt: at2, text });
    adapter.saveSnapshot(base);
    adapter.saveSnapshot(same);
    adapter.saveSnapshot(diff);

    // 正对照：五字段全等（只有 `snapshotId` 不同）⇒ 必须成组。
    if (!sameGroupByProtocol(base, same)) {
      return { status: FAIL, detail: '判据自身：造出来的对照组不满足「键全等」⇒ 对照无效' };
    }
    const control = await adapter.duplicatesFor({ sessionId, snapshotIds: [base.snapshotId, same.snapshotId] });
    if (!control.ok) {
      return { status: FAIL, detail: `对照组不成立（duplicates 失败：code=${control.code}）⇒ 负例无意义` };
    }
    if (control.groups.length !== 1 || (control.groups[0]?.members ?? []).length !== 2) {
      return { status: FAIL, detail: '对照组不成立（五字段全等的两条没被认成同一组）⇒ 负例无意义' };
    }

    // 键层：`capturedAt` 不同 ⇒ 键必须不同。
    if (adapter.groupKeyOf(base) === adapter.groupKeyOf(diff)) {
      return {
        status: FAIL,
        detail: 'capturedAt 不同但组标识相同 ⇒ 键里没算「捕获事实」（协议 §1.1）',
      };
    }

    // 负例：命令层必须**只是不成组**（成功 + 空组），不是失败。
    const negative = await adapter.duplicatesFor({ sessionId, snapshotIds: [base.snapshotId, diff.snapshotId] });
    if (!negative.ok) {
      return {
        status: FAIL,
        detail: `capturedAt 不同的两条核对失败（code=${negative.code}）——应当只是「不成组」`,
      };
    }
    if (negative.groups.length > 0) {
      return {
        status: FAIL,
        detail: 'capturedAt 不同的两条被认成了同一组——协议 §1.1：两次不同的捕获事实不成组',
      };
    }

    return {
      status: PASS,
      detail: `capturedAt 不同（${at1} vs ${at2}）⇒ 不成组（键不同、命令层空组）；正对照（五字段全等）成组`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-4｜accountScope / branch 不同 ⇒ 不成组（负例）
// ---------------------------------------------------------------------------

/**
 * P2-4｜§5 第 4 行 / §1.1。
 *
 * `accountScope` / `branch` 不同 ⇒ **不成组**（跨账号 / 跨分支不能成组）。
 *
 * ## 「不成组」的两种合法形态
 *
 * · **分开**（成功 + 空组）——最直接的形态；
 * · **更严格地拒**（失败）——跨账号时 host 的指纹闸（P1 S-5）会先拒，
 *   这是**比「不成组」更强**的约束，同样满足 §1.1「不成组」的要求。
 *
 * 两种都算过；但**必须有正对照**（键全等 ⇒ 必须成组）⇒ 否则「一律失败」
 * 也会让负例变绿。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_4_scopeBranchNotGrouped(adapter) {
  return guard('P2-4-scope-branch-not-grouped', async () => {
    const sessionId = 'p2-craft-scope-branch';
    const text = 'P2 判据正文（跨账号 / 跨分支场景）。';
    const base = adapter.makeSnapshot({ seq: 1, sessionId, branch: 'main', text });
    const same = adapter.makeSnapshot({ seq: 2, sessionId, branch: 'main', text });
    const otherBranch = adapter.makeSnapshot({ seq: 3, sessionId, branch: 'feature-x', text });
    const otherScope = adapter.makeSnapshot({
      seq: 4,
      sessionId,
      branch: 'main',
      accountScope: 'desktop-dev/default/site-B',
      text,
    });
    for (const one of [base, same, otherBranch, otherScope]) adapter.saveSnapshot(one);

    // 正对照：键全等 ⇒ 必须成组。
    if (!sameGroupByProtocol(base, same)) {
      return { status: FAIL, detail: '判据自身：造出来的对照组不满足「键全等」⇒ 对照无效' };
    }
    const control = await adapter.duplicatesFor({ sessionId, snapshotIds: [base.snapshotId, same.snapshotId] });
    if (!control.ok) {
      return { status: FAIL, detail: `对照组不成立（duplicates 失败：code=${control.code}）⇒ 负例无意义` };
    }
    if (control.groups.length !== 1 || (control.groups[0]?.members ?? []).length !== 2) {
      return { status: FAIL, detail: '对照组不成立（五字段全等的两条没被认成同一组）⇒ 负例无意义' };
    }

    // 负例 ①：跨分支。
    if (adapter.groupKeyOf(base) === adapter.groupKeyOf(otherBranch)) {
      return { status: FAIL, detail: 'branch 不同但组标识相同 ⇒ 键里没算分支（协议 §1.1）' };
    }
    const branchResult = await adapter.duplicatesFor({
      sessionId,
      snapshotIds: [base.snapshotId, otherBranch.snapshotId],
    });
    if (branchResult.ok && branchResult.groups.length > 0) {
      return { status: FAIL, detail: 'branch 不同的两条被认成了同一组——协议 §1.1：跨分支不成组' };
    }
    if (!branchResult.ok && branchResult.code !== 'account_changed') {
      return {
        status: FAIL,
        detail: `跨分支的两条核对失败（code=${branchResult.code}）——应当只是「不成组」`,
      };
    }

    // 负例 ②：跨账号。
    if (adapter.groupKeyOf(base) === adapter.groupKeyOf(otherScope)) {
      return { status: FAIL, detail: 'accountScope 不同但组标识相同 ⇒ 键里没算账户（协议 §1.1）' };
    }
    const scopeResult = await adapter.duplicatesFor({
      sessionId,
      snapshotIds: [base.snapshotId, otherScope.snapshotId],
    });
    if (scopeResult.ok && scopeResult.groups.length > 0) {
      return { status: FAIL, detail: 'accountScope 不同的两条被认成了同一组——协议 §1.1：跨账号不成组' };
    }

    const branchHow = branchResult.ok ? '分开（空组）' : `更严格地拒（code=${branchResult.code}）`;
    const scopeHow = scopeResult.ok ? '分开（空组）' : `更严格地拒（code=${scopeResult.code}）`;
    return {
      status: PASS,
      detail: `跨分支 ⇒ 不成组（${branchHow}）；跨账号 ⇒ 不成组（${scopeHow}）；正对照（五字段全等）成组`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-5｜超预算 ⇒ 发送前出现预算提示，正文未被截断
// ---------------------------------------------------------------------------

/**
 * P2-5｜§5 第 5 行 / §4 / §3.3。
 *
 * `projectedBytes > 262144` ⇒ **发送前**出现预算提示，**正文未被截断**；
 * 请求体正文**逐字节**等于各引用原文。
 *
 * ## 两个场景都跑
 *
 * 1. **纯预算场景**：两条引用**不同组**（`capturedAt` 不同）⇒ 只该出预算提示。
 * 2. **同组 + 超预算**：两条引用**同组**且超预算 ⇒ 按 **PM v0.4 裁断**
 *    （协议 §3.1 的括注）：内核 notice 是**单槽**（最新一条覆盖旧条）⇒
 *    「两个触发点各自独立」的准确含义是「**两条都要发**（任一条都不得被 early
 *    return 吃掉）」，**不是**「用户同时看到两条」；而**用户最终看到的是最后
 *    那一条** ⇒ **后发那条必须自包含全部事实**（组事实 ＋ 预算事实）。
 *
 * ## 口径依赖协议版本（**协议升版时先看这段**）
 *
 * 本条断的「**两条都发 ＋ 后发那条自包含**」是**协议 v0.4 / v0.5 的口径**
 * （§3.1 明写「两条都要发」）。
 * ⇒ **若 PM 将来把呈现形态裁定成「合成一条」**（协议随之升版、§3.1 改写），
 *   本条的「两条都发」断点、连同 P2-2 的「可复现」断点**都要跟着改写**——
 *   判据**按协议稿走**，不预设任何一种呈现形态。
 * ⇒ 升版落地时的分工：**判据侧改判据，不改实现**（实现按协议稿改）。
 *
 * ## 「逐字节」怎么断（**不写「正文只出现一次」**）
 *
 * 照 P1 A5 的法：断「**请求体与落盘正文逐字节相同**」＋「落盘正文 == 完整原文
 * 的字节数」。内核会把本轮输入在同一请求里重放多次（P1 A5 的注释已记）⇒
 * **数出现次数**的判据会被内核行为打红 ⇒ 本库不数。
 *
 * ## 口径说明
 *
 * §4 的 `projectedBytes` 还含「原生 `@session` 占用」，但 host 结构上拿不到
 * 草稿 ⇒ 实现按 0 计（本库只判**可观测的那部分**：各引用各送一份的总字节）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_5_budgetNoticeNoTruncation(adapter) {
  return guard('P2-5-budget-notice-no-truncation', async () => {
    const sessionId = 'p2-check-budget';
    const bodyBytes = adapter.configuredBodyBytes?.() ?? null;
    if (bodyBytes === null) {
      return { status: SKIPPED, detail: '本轮没造大正文（`bodyBytes` 未给）⇒ 预算场景触发不到' };
    }

    // **判据自身的口径检查（造数前提）**：每条正文必须 **< 单条正文预算 64 KB**。
    //
    // **为什么**：协议 P1 §3 第 14 码 `body_budget_exceeded` ＝ **单条正文超
    // 64 KB ⇒ 拒收那一条引用**（捕获/落盘阶段就拒）⇒ ⇒ 每条 ≥ 64 KB 的正文
    // **根本落不了盘** ⇒ 预算场景**触发不到**（只会看到「大正文没落盘」）。
    //
    // ⇒ 每条 < 65536 ⇒ 4 条最多 4×65535 = 262140 **< 262144** ⇒ ⇒ **至少 5 条**
    // 才够把合计顶过单次消息总预算（PM 2026-10-21 裁：**每条 ≤ 64 KB、多条
    // 合计 > 262144**）。
    if (bodyBytes >= PER_BODY_BUDGET_BYTES) {
      return {
        status: FAIL,
        detail:
          `判据自身：每条正文 ${bodyBytes} 字节 ≥ 单条正文预算 ${PER_BODY_BUDGET_BYTES} 字节` +
          ' ⇒ 会被第 14 码拒收、引用落不了盘 ⇒ 场景不成立',
      };
    }

    // 场景 ①：**五条不同组**（`capturedAt` 各差 500ms）⇒ 合计超预算 ⇒ 只该出
    // 预算那条提示。
    //
    // **为什么是五条**：每条 < 64 KB ⇒ 4 条最多 262140 < 262144 ⇒ **5 条**才够
    // 触发超预算（见上面的口径检查）。
    const t0 = 1_767_000_000_000;
    adapter.setNow?.(t0);
    const first = await adapter.pick({ sessionId });
    await adapter.settle(first.ref);
    const mark = adapter.visibleNotices?.().length ?? 0;
    /** 场景 ① 的五条引用（第 1 条 ＋ 后面四条**各自不同组**）。 */
    const refs = [first];
    for (let index = 1; index < 5; index += 1) {
      adapter.setNow?.(t0 + index * 500);
      const picked = await adapter.pick({ sessionId });
      await adapter.settle(picked.ref);
      refs.push(picked);
    }
    if (refs.some((one) => !one?.ref)) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }

    const files = refs.map((one) => adapter.snapshotFile(one.ref));
    if (files.some((file) => !file?.exists)) {
      return { status: FAIL, detail: '大正文没落盘（预算场景触发不到）' };
    }
    const sizes = files.map((file) => Buffer.byteLength(String(file.body ?? ''), 'utf8'));
    const bytesA = sizes[0];
    const bytesB = sizes[1];
    const projected = sizes.reduce((sum, size) => sum + size, 0);

    // 判据自身的口径检查：造出来的正文必须**够大**（否则测不到超限）。
    if (sizes.some((size) => size !== bodyBytes)) {
      return {
        status: FAIL,
        detail: `判据自身：落盘正文字节数不对（${sizes.join(' / ')}，应为 ${bodyBytes}）⇒ 场景不成立`,
      };
    }
    if (projected <= PER_MESSAGE_BUDGET_BYTES) {
      return {
        status: FAIL,
        detail: `判据自身：五条合计 ${projected} 字节 ≤ 预算 ${PER_MESSAGE_BUDGET_BYTES} ⇒ 超限场景不成立`,
      };
    }

    // 预算提示必须出现（§3.1-2 / §4）——**按数据找**（带出 `wouldExceed` 事实）。
    //
    // **归属轮次＝第 5 条那条**（超预算是在合计顶过 262144 时才成立的 ⇒ 那是
    // 第 5 条触发的那一轮）。
    const notice = await waitForBudgetNotice(adapter, { from: mark, lastRef: refs[refs.length - 1].ref });
    if (notice === undefined) {
      return {
        status: FAIL,
        detail:
          `预算超限（${projected} > ${PER_MESSAGE_BUDGET_BYTES}）——协议 §3.1-2 / §4 ` +
          `要求出现预算提示；${noticeTimeoutDetail(adapter, '预算提示', {
            from: mark,
            lastRef: refs[refs.length - 1].ref,
          })}`,
      };
    }
    const carrier = checkNoticeCarrier(notice);
    if (!carrier.ok) {
      return { status: FAIL, detail: carrier.why };
    }

    // 提示里的数字必须对（§3.2：要说清超了多少字节）。
    const budget = notice.budget ?? {};
    if (Number(budget.limitBytes ?? -1) !== PER_MESSAGE_BUDGET_BYTES) {
      return {
        status: FAIL,
        detail: `提示里的预算上限不对（${budget.limitBytes}，应为 ${PER_MESSAGE_BUDGET_BYTES}）`,
      };
    }
    if (Number(budget.projectedBytes ?? -1) !== projected) {
      return {
        status: FAIL,
        detail: `提示里的预估字节数不对（${budget.projectedBytes}，应为 ${projected}）`,
      };
    }
    if (budget.wouldExceed !== true) {
      return { status: FAIL, detail: `提示里没有标出「会超限」（wouldExceed=${budget.wouldExceed}）` };
    }
    const over = projected - PER_MESSAGE_BUDGET_BYTES;
    if (Number(budget.exceedByBytes ?? -1) !== over) {
      return {
        status: FAIL,
        detail: `提示里的超出字节数不对（${budget.exceedByBytes}，应为 ${over}）`,
      };
    }
    const text = String(notice.text ?? '');
    // 协议 §3.2「数字来源」：**数字独立出现且紧跟单位词**（PM 02:5x 派单①加严）。
    //
    // **为什么不用 `text.includes`**（子串假绿口子）：`includes('37856')` 会被
    // 「**137856**」满足 ⇒ 一条只写了别的字节数、根本没写「超了多少字节」的文案
    // 也判成「文案自包含」⇒ 假绿。
    if (!hasNumberWithUnit(text, projected, '字节')) {
      return {
        status: FAIL,
        detail: `提示文案里没有「${projected} 字节」——协议 §3.2：文案必须自包含；**子串不算**`,
      };
    }
    if (!hasNumberWithUnit(text, over, '字节')) {
      return {
        status: FAIL,
        detail:
          `提示文案里没有「${over} 字节」——协议 §3.2：文案必须自包含**超了多少字节**；` +
          '**子串不算**（「137856」里的「37856」不算「写了 37856 字节」）',
      };
    }

    // 正文**未被截断**（§3.3）：请求体与落盘正文逐字节相同，且落盘正文是完整原文。
    const requestA = await trySerialize(adapter, first.ref);
    const requestB = await trySerialize(adapter, refs[1].ref);
    if (!requestA.ok) {
      return { status: FAIL, detail: `第一条的请求体取不到（发不出去）：${requestA.error}` };
    }
    if (!requestB.ok) {
      return { status: FAIL, detail: `第二条的请求体取不到（发不出去）：${requestB.error}` };
    }
    if (requestA.body !== String(files[0].body)) {
      return { status: FAIL, detail: '第一条的请求体与落盘正文不一致（正文被改了/被截断了）' };
    }
    if (requestB.body !== String(files[1].body)) {
      return { status: FAIL, detail: '第二条的请求体与落盘正文不一致（正文被改了/被截断了）' };
    }
    if (bytesA !== bodyBytes || bytesB !== bodyBytes) {
      return {
        status: FAIL,
        detail: `落盘正文被截断了（${bytesA} / ${bytesB}，完整原文应为 ${bodyBytes}）——协议 §3.3`,
      };
    }

    // 场景 ②：**第六条**引用**回到第一条的捕获时刻** ⇒ 与第一条**同组**；
    // 草稿里六条引用合计超预算 ⇒ 「重复」与「超预算」**同时成立**。
    //
    // ## **PM v0.4 裁断**（协议 §3.1 的括注）——本判据按它写
    //
    // 内核 notice 是**单槽**（最新一条覆盖旧条）⇒
    // · 「两个触发点各自独立」的准确含义是「**两条都要发**（任一条都不得被
    //   early return 吃掉）」，**不是**「用户同时看到两条」；
    // · **用户最终看到的是最后那一条** ⇒ **后发那条必须自包含全部事实**
    //   （组事实 ＋ 预算事实都在它身上）。
    //
    // ⇒ 本场景断三件事：① 组提示发了 ② 预算提示**也**发了
    //   ③ **后发那条**既带出组事实、也带出预算事实（文案里有「超了多少字节」）。
    const mark2 = adapter.visibleNotices?.().length ?? 0;
    adapter.setNow?.(t0);
    const sixth = await adapter.pick({ sessionId });
    await adapter.settle(sixth.ref);
    if (!sixth?.ref) {
      return { status: SKIPPED, detail: '第六个 pick 未返回 ref（同组场景触发不到）' };
    }
    const fileF = adapter.snapshotFile(sixth.ref);
    const bytesF = Buffer.byteLength(String(fileF?.body ?? ''), 'utf8');
    const projected6 = projected + bytesF;
    if (projected6 <= PER_MESSAGE_BUDGET_BYTES) {
      return { status: SKIPPED, detail: '同组场景的合计没超预算 ⇒ 该场景触发不到' };
    }
    const groupNotice = await waitForNotice(adapter, 'duplicate-groups', {
      from: mark2,
      lastRef: sixth.ref,
    });
    if (groupNotice === undefined) {
      return {
        status: FAIL,
        detail:
          '同组场景——协议 §3.1-1 要求出现「重复组」提示；' +
          noticeTimeoutDetail(adapter, '「重复组」提示', { from: mark2, lastRef: sixth.ref }),
      };
    }
    // ① 两条都要发：预算提示**不得**被组提示的 early return 吃掉。
    const budgetNotice = await waitForNotice(adapter, 'budget-exceeded', {
      from: mark2,
      lastRef: sixth.ref,
    });
    if (budgetNotice === undefined) {
      return {
        status: FAIL,
        detail:
          '同组且超预算时——协议 §3.1（v0.4 裁断）：两个触发点各自独立 ＝ 两条都要发，' +
          '任一条都不得被 early return 吃掉；' +
          noticeTimeoutDetail(adapter, '预算提示', {
            from: mark2,
            lastRef: sixth.ref,
            // **UX 逐字定稿**（PM 2026-10-21 02:8x ⑤②）：这一处的排查指引
            // **覆盖**通用后半句 ⇒ 「再分辨是漏报，还是只发了一条（被 early return 吃掉）」。
            // （**元话语不进红字**：原句里那句「这一处另注」是给实现者看的说明，
            //   对排查者是噪声 ⇒ 整段删掉，改为上面这条**注释**。）
            tail: '若在低负载下稳定复现，再分辨是漏报，还是只发了一条（被 early return 吃掉）',
            // 这一句是**该处专属**的排查指引（「这一处要分辨什么」）⇒ 始终写。
            tailAlways: true,
          }),
      };
    }
    // ② **后发那条 ＝ 用户最终看到的那条**（单槽覆盖）⇒ 必须自包含全部事实。
    const delivered = (adapter.visibleNotices?.() ?? []).slice(mark2);
    const last = delivered[delivered.length - 1];
    if (last === undefined) {
      return { status: FAIL, detail: '判据自身：没读到这一轮的提示投递序列' };
    }
    const over6 = projected6 - PER_MESSAGE_BUDGET_BYTES;
    const lastText = String(last.text ?? '');
    if (!lastText.includes(String(over6))) {
      return {
        status: FAIL,
        detail:
          `后发那条提示（${last.kind}）里没有「超了多少字节」（应为 ${over6}）——` +
          '协议 §3.1（v0.4 裁断）：内核 notice 单槽 ⇒ 用户最终看到的就是后发那条，它必须自包含全部事实',
      };
    }
    const lastGroups = Array.isArray(last.groups) ? last.groups : [];
    const lastMembers = lastGroups.reduce(
      (sum, group) => sum + (Array.isArray(group?.members) ? group.members.length : 0),
      0,
    );
    if (lastGroups.length === 0 || lastMembers < 2) {
      return {
        status: FAIL,
        detail:
          `后发那条提示（${last.kind}）里没有带出组事实（同组 ${lastMembers} 条 / ${lastGroups.length} 组）——` +
          '协议 §3.1（v0.4 裁断）：后发那条必须自包含全部事实（组事实 ＋ 预算事实）',
      };
    }

    return {
      status: PASS,
      detail:
        `超预算（${projected} > ${PER_MESSAGE_BUDGET_BYTES}）⇒ 发送前出现预算提示（level=info、输入区常驻条）；` +
        `正文未被截断（五条各 ${bodyBytes} 字节，请求体与落盘逐字节相同）；` +
        `同组+超预算时两条都发、后发那条自包含（超 ${over6} 字节 ＋ 组事实）`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-F｜夹具的期望值必须与协议 §1.1 / §1.2 一致（§5「夹具」条款）
// ---------------------------------------------------------------------------

/**
 * P2-F｜协议 §5 表下的「夹具」条款：**新增「去重候选对」夹具（两条快照 ＋ 期望
 * 是否成组），登记进 `fixtures/protocol/index.json` ⇒ 两端都读**。
 *
 * ## 判的是「**夹具的期望值对不对**」
 *
 * 夹具的 `expected` 若是从**实现**抄回来的，它就只是实现的复印件 ⇒ 判据毫无意义。
 * ⇒ 本判据**自己**按 §1.1（五字段全等）与 §1.2（分隔符 ＋ sha256 前 16 位）
 * **独立转写**期望值，再与夹具写的比。
 *
 * @param {object} fixture - 夹具对象（`{input:{kind,snapshots}, expected:{…}}`）。
 * @param {string} [label] - 夹具名（写进 detail，便于定位）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function checkP2_F_fixtureExpectation(fixture, label = 'fixture') {
  const name = 'P2-F-fixture-expectation-match';
  const snapshots = Array.isArray(fixture?.input?.snapshots) ? fixture.input.snapshots : [];
  if (snapshots.length < 2) {
    return verdict(name, SKIPPED, `${label}：夹具里不足两条快照`);
  }
  const expected = fixture?.expected?.sameGroup;
  const actual = sameGroupByProtocol(snapshots[0], snapshots[1]);
  if (expected !== actual) {
    return verdict(
      name,
      FAIL,
      `${label}：期望 sameGroup=${String(expected)}，按协议 §1.1（五字段全等）应为 ${String(actual)}`,
    );
  }
  // **组标识**：协议 v0.3 已把分隔符裁定为**单个换行 U+000A、不含空格**
  // ⇒ 期望值可从协议稿**独立转写** ⇒ 夹具里固化的 `expected.groupKey`
  // 必须是「按 §1.2 算出来的那个」，而不是「实现算出来的那个」。
  const expectedKey = fixture?.expected?.groupKey;
  if (expectedKey !== undefined) {
    const wantKey = expectedGroupKey(snapshots[0]);
    if (expectedKey !== wantKey) {
      return verdict(
        name,
        FAIL,
        `${label}：夹具的 expected.groupKey=${String(expectedKey)}，按协议 §1.2 应为 ${wantKey}`,
      );
    }
  }
  return verdict(name, PASS, `${label}：期望值与协议 §1.1 / §1.2 一致`);
}

// ---------------------------------------------------------------------------
// P2-C｜duplicates 失败 ⇒ 码在 14 码闭集内，原码留痕（§2.2 / §2.3 / §6.5）
// ---------------------------------------------------------------------------

/**
 * P2-C｜协议 §2.2「出参（失败）：沿用 P1 §3 的 **14 码闭集**与 8 字段错误结果对象」
 * ＋ §2.3「**不新增错误码**（14 码是闭集）」＋ §6.5。
 *
 * ## 为什么这条是「表外的一条」
 *
 * §5 的表只有六行，失败码不在其中；但 §2.2/§2.3/§6.5 都写了它 ⇒ 落一条判据。
 *
 * ## 协议内部的矛盾（**本判据不替 PM 选边**）
 *
 * §2.3 的表把「盘上快照不可读」写成 `snapshot_store_unavailable`、「入参形态不对」
 * 写成 `invalid_request`——**这两个码不在 14 码闭集里**；而同一条的下一行又写
 * 「**不新增错误码**（14 码是闭集）」，§6.5 也写「不新增错误码」。
 *
 * ⇒ 本判据判的是**闭集那一侧**（说了两次的那一侧）：失败码**必须**是 14 码
 * 之一；闭集外的原始码**必须**保留在 `detail` 里（P1 §3 的规则，折叠不丢信息）。
 * ⇒ 若 PM 裁定把那两个码**正式纳入**闭集，本判据的「原码留痕」那一条会自动
 * 放行（码本身就是原码）——**两种裁定都绿**，不会因为协议改稿而假红。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
/**
 * P2-C｜协议 §2.2「出参（失败）：沿用 P1 §3 的 **14 码闭集**与 **8 字段错误结果对象**」。
 *
 * ## 这条断的是「**8 字段闭集**」（PM v0.3 §六 的裁断）
 *
 * P1 §3 `:162`：**错误结果对象字段闭集 = 8 个，全部必备、不省略**——
 * `kind` / `ok` / `code` / `retryable` / `userNext` / `requestId` / `logPath` / `detail`。
 *
 * ⇒ **8 个字段逐个断**，**不**断「必备字段在不在」这种弱口径（那会把「7 个」
 * 固化成正确口径）。
 *
 * ## 必须在**命令层**断（`kind` 穿不过 RPC 层）
 *
 * RPC 层（`host/rpc.js` 的 `toRpcError`）把错误负载翻译成 `{code, message, details}`
 * ⇒ **`kind` 在传输层根本不存在** ⇒ 要断 8 字段，只能看**命令层自己的返回**
 * （adapter 的 `duplicatesCommandLayer` 观测口）。
 *
 * ## **G-2 的历史**（**已于 2026-10-21 00:1x 销号**）
 *
 * 这条判据当初会红，是因为 **G-2**：Rust 侧 `error_result` 产 8 个（含
 * `kind: "error"`），JS 侧 `errorPayload` 只产 **7 个**（缺 `kind`）⇒ 两端不同构。
 * **协议闭集始终未改**（是 **JS 漏字段**、不是协议写错）⇒ 由 JS 侧补 `kind`。
 *
 * **G-2 已闭环**（JS 两个分支都补了 `kind: 'error'`，PM 已复核）⇒ 本条现在应当
 * **转绿**；**若再红 ⇒ 说明 G-2 回退**（JS 侧又把 `kind` 省了），
 * **不是判据口径问题** ⇒ 判据一个字不改。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_C_errorCodeClosedSet(adapter) {
  return guard('P2-C-duplicates-error-code-closed-set', async () => {
    if (typeof adapter.duplicatesCommandLayer !== 'function') {
      return { status: SKIPPED, detail: 'adapter 未提供「命令层直调」观测口——拿不到 8 字段的原始负载' };
    }
    // **入参形态不对**（§2.3 表的最后一行）⇒ 命令层**真的**失败 ⇒ 原始负载。
    const raw = await adapter.duplicatesCommandLayer({ sessionId: '', snapshotIds: 'not-an-array' });
    if (raw?.ok !== false) {
      return { status: FAIL, detail: '入参形态不对却回了 ok=true——协议 §2.3' };
    }
    const missing = ERROR_RESULT_FIELDS.filter((field) => !Object.hasOwn(raw, field));
    if (missing.length > 0) {
      return {
        status: FAIL,
        detail:
          `错误结果对象缺字段：${missing.join('、')}——P1 §3 :162 的 8 字段闭集全部必备、不省略` +
          '（若缺的是 kind ⇒ 那是 **G-2**：JS 侧 `errorPayload` 未按 P1 §3 补 `kind`，已派 devloper；' +
          '**不是判据错**）',
      };
    }
    // **闭集就是闭集**：8 个，不多不少 ⇒ 多出来的字段同样违约
    // （协议 §3 的「字段闭集」若允许多出，就不是闭集了）。
    const extra = Object.keys(raw).filter((key) => !ERROR_RESULT_FIELDS.includes(key));
    if (extra.length > 0) {
      return {
        status: FAIL,
        detail:
          `错误结果对象多出闭集外的字段：${extra.join('、')}——P1 §3 :162 的字段闭集是 8 个，` +
          '**不多不少**（多出来的字段会让两端「同构」这件事失去意义）',
      };
    }
    return {
      status: PASS,
      detail: `错误结果对象 8 个字段齐全（${ERROR_RESULT_FIELDS.join('、')}）`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-6｜duplicates 失败 ⇒ 可见错误 ＋ 不额外拦截发送
// ---------------------------------------------------------------------------

/**
 * P2-6｜§5 第 6 行 / §2.3。
 *
 * `duplicates` 失败 ⇒ **可见错误** ＋ **不额外拦截发送**；模型请求数与失败前
 * 相同（未被拦截）。
 *
 * ## 三块断言
 *
 * 1. **可见错误 ＋ 不额外拦截**（客户端侧）：两种失败形态（命令自己失败 /
 *    送不到）都要出可见提示（输入区、`info` 档、文案不含「已发送/发送失败/
 *    发送被阻止」），且正文照发。
 * 2. **失败负载的原始码留痕**（PM v0.3 加严点 ①）：入参形态不对 ⇒ 真实命令层
 *    失败 ⇒ 闭集外的原始码（`invalid_request`）**必须保留在 `detail`**。
 *    **为什么必须断**：不断就断 —— 归一后的 `snapshot_invalid` 会把
 *    「入参形态错」与「快照过期」压成**不可区分**的一坨 ⇒ **假绿**。
 * 3. **失败码在 14 码闭集内**（P1 §3 / P2 §2.3 / §6.5）。
 *
 * ## 两种失败形态都跑
 *
 * · `'command'` —— **命令自己说失败**（`{ok:false, code, …}` 的错误负载）；
 * · `'transport'` —— **送不到**（HTTP 500）。
 *
 * 客户端对两种失败的处理必须一致（都只影响提示，不影响正文）。
 *
 * ## 「不额外拦截」怎么断
 *
 * · 正文**照发**：`serialize(ref)` 仍取得到**逐字节相同**的正文；
 * · 模型请求数：失败前后**恰好 +1**（对应那一次正文产出）⇒ 既没被拦（不是 0），
 *   也没多发（不是 2）。
 *
 * ## 措辞纪律（§2.3）
 *
 * 界面文案**不得**写「已发送」「发送失败」「发送被阻止」——失败文案只说
 * 「无法核对是否重复」。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_6_failureDoesNotBlockSend(adapter) {
  return guard('P2-6-failure-does-not-block-send', async () => {
    // —— 断言 2/3：**真实命令层失败**的负载（码闭集内 ＋ 原码留痕）——
    //
    // **为什么用真实失败而不是注入的失败**：注入的负载里 `detail` 是判据自己写的
    // ⇒ 断它等于自证 ⇒ **没有意义**。这里让命令层**真的**因为「入参形态不对」
    // 而失败（协议 §2.3 的最后一行），拿它的**原始负载**来断。
    const raw = await adapter.duplicatesCommandLayer?.({ sessionId: '', snapshotIds: 'not-an-array' });
    if (raw === undefined) {
      return { status: SKIPPED, detail: 'adapter 未提供「命令层直调」观测口——拿不到原始错误负载' };
    }
    if (raw?.ok !== false) {
      return { status: FAIL, detail: '入参形态不对却回了 ok=true——协议 §2.3' };
    }
    const rawCode = String(raw.code ?? '');
    if (!CLOSED_ERROR_CODES.includes(rawCode)) {
      return {
        status: FAIL,
        detail: `失败码 ${rawCode} 不在 14 码闭集里——协议 §2.3 / §6.5：不新增错误码`,
      };
    }
    const rawDetail = String(raw.detail ?? '');
    if (!rawDetail.includes('invalid_request')) {
      return {
        status: FAIL,
        detail:
          `失败码归成了 ${rawCode}，但 detail 里没有原始码 invalid_request（实测 detail=` +
          `${rawDetail || '(空)'}）——P1 §3：原始码要保留在 detail；` +
          '不断这条，「入参形态错」与「快照过期」会被压成不可区分的一坨（假绿）',
      };
    }

    const sessionId = 'p2-check-failure';
    const forbidden = FORBIDDEN_SEND_RESULT_WORDS;
    const observed = [];

    for (const mode of ['command', 'transport']) {
      adapter.setNow?.(1_767_000_000_000);
      const picked = await adapter.pick({ sessionId });
      if (!picked?.ref) {
        return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
      }
      await adapter.settle(picked.ref);

      // 基线：失败前正文取得到、请求数 +1。
      const before = adapter.modelRequestCount();
      const baselineBody = await trySerialize(adapter, picked.ref);
      const after = adapter.modelRequestCount();
      if (!baselineBody.ok) {
        return { status: FAIL, detail: `失败前的正文取不到（判据自身的基线不成立）：${baselineBody.error}` };
      }
      if (after !== before + 1) {
        return {
          status: FAIL,
          detail: `失败前的模型请求数不是 +1（${before} → ${after}）——判据自身的基线不成立`,
        };
      }
      const file = adapter.snapshotFile(picked.ref);
      if (baselineBody.body !== String(file?.body ?? '')) {
        return { status: FAIL, detail: '失败前的正文与落盘不一致（判据自身的基线不成立）' };
      }

      // 制造 `duplicates` 失败，并再选一条引用触发这次核对。
      let notice = undefined;
      // **这一轮的触发引用**（事件式信号①要用的）⇒ 提到闭包外（红字要用）。
      let failedRef = '';
      await adapter.withFailingDuplicates(mode, async () => {
        const second = await adapter.pick({ sessionId });
        if (!second?.ref) return;
        await adapter.settle(second.ref);
        failedRef = second.ref;
        notice = await waitForNotice(adapter, 'duplicates-failed', { lastRef: second.ref });
      });
      if (notice === undefined) {
        return {
          status: FAIL,
          detail:
            `duplicates 失败（${mode}）后——协议 §2.3 要求出现可见错误提示；` +
            noticeTimeoutDetail(adapter, '可见错误提示', { lastRef: failedRef }),
        };
      }
      const carrier = checkNoticeCarrier(notice);
      if (!carrier.ok) {
        return { status: FAIL, detail: `${mode}：${carrier.why}` };
      }
      const text = String(notice.text ?? '');
      for (const phrase of forbidden) {
        if (text.includes(phrase)) {
          return {
            status: FAIL,
            detail: `失败文案里出现了「${phrase}」——协议 §2.3：失败文案只说「无法核对是否重复」`,
          };
        }
      }

      // 不额外拦截：正文照发、请求数不增不减地 +1。
      const before2 = adapter.modelRequestCount();
      const body = await trySerialize(adapter, picked.ref);
      const after2 = adapter.modelRequestCount();
      if (!body.ok) {
        return {
          status: FAIL,
          detail: `失败后正文取不到（${body.error}）——协议 §2.3：失败不阻塞发送，正文照发`,
        };
      }
      if (after2 !== before2 + 1) {
        return {
          status: FAIL,
          detail: `失败后模型请求数不是 +1（${before2} → ${after2}）——协议 §5 第 6 行：未被拦截`,
        };
      }
      if (body.body !== String(file?.body ?? '')) {
        return {
          status: FAIL,
          detail: '失败后正文与落盘不一致——协议 §2.3：失败不阻塞发送，正文照发',
        };
      }
      observed.push(`${mode}：可见错误 + 正文照发（请求数 +1）`);
    }

    return { status: PASS, detail: observed.join('；') };
  });
}

// ---------------------------------------------------------------------------
// P2-D｜跨语言：两端算出的 groupKey 必须相等（§1.2 / §6「两端都读」）
// ---------------------------------------------------------------------------

/**
 * P2-D｜协议 §1.2 ＋ §5「夹具」条款：**同一组夹具，两端都读** ⇒ 两端算出的
 * `groupKey` 必须**逐字节相等**。
 *
 * ## 断的是「三个值一致」
 *
 * · **协议值**：判据**自己**按 §1.2 独立转写（五字段按顺序、单个换行 U+000A
 *   连接、sha256 前 16 位小写十六进制）⇒ **标准答案不来自任何一端**；
 * · **JS 值**：实现 `groupKeyOf` 的输出（经 adapter 观测口）；
 * · **Rust 值**：`p1_protocol_fixture` example 的输出。
 *
 * 三个相等 ⇒ 任何一端漂移都立刻现形（只断 JS==Rust 会放过「两端一起错」）。
 *
 * ## Rust 端没构建 ⇒ **报「未执行」**，不判通过
 *
 * （与 P1 A1c 的既有口径一致：Rust 侧没有产物 ⇒ 本条未执行。）
 *
 * @param {object} input
 * @param {Array<{id: string, fixture: object}>} input.fixtures - 夹具（含 `duplicates` kind）。
 * @param {Array<{id: string, result: object}>|undefined} input.rustCases - Rust 端产物。
 * @param {object} adapter - 观测口（`groupKeyOf`）。
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_D_crossLanguageGroupKey({ fixtures = [], rustCases, adapter } = {}) {
  return guard('P2-D-cross-language-group-key', async () => {
    const duplicates = (fixtures ?? []).filter((item) => String(item?.fixture?.input?.kind) === 'duplicates');
    if (duplicates.length === 0) {
      return { status: SKIPPED, detail: '没有「去重候选对」夹具' };
    }
    if (!Array.isArray(rustCases)) {
      return {
        status: SKIPPED,
        detail:
          'Rust 侧没有产物（未构建 p1_protocol_fixture example）——本条未执行，不判通过；' +
          '构建：`cargo build --manifest-path src-tauri/Cargo.toml --example p1_protocol_fixture`',
      };
    }
    const problems = [];
    let checked = 0;
    for (const item of duplicates) {
      const snapshot = item.fixture?.input?.snapshots?.[0];
      if (snapshot === undefined) {
        problems.push(`${item.id}：夹具里没有快照`);
        continue;
      }
      const protocolKey = expectedGroupKey(snapshot);
      const jsKey = String(adapter.groupKeyOf(snapshot));
      const rustCase = (rustCases ?? []).find((one) => String(one?.id) === String(item.id));
      const rustKey = String(rustCase?.result?.groupKey ?? '');
      if (jsKey !== protocolKey) {
        problems.push(`${item.id}：JS 端算出 ${jsKey}，按协议 §1.2 应为 ${protocolKey}`);
      }
      if (rustKey === '') {
        problems.push(`${item.id}：Rust 端没有这个夹具的结果`);
        continue;
      }
      if (rustKey !== protocolKey) {
        problems.push(`${item.id}：Rust 端算出 ${rustKey}，按协议 §1.2 应为 ${protocolKey}`);
      }
      if (jsKey !== rustKey) {
        problems.push(`${item.id}：两端不相等（JS ${jsKey} / Rust ${rustKey}）`);
      }
      if (jsKey === protocolKey && rustKey === protocolKey) checked += 1;
    }
    if (problems.length > 0) {
      return { status: FAIL, detail: problems.join('；') };
    }
    return {
      status: PASS,
      detail: `${checked} 条夹具的三方 groupKey 一致（协议独立转写 ＝ JS ＝ Rust）`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-H｜多组场景：groups ≥ 2 ⇒ 不漏报 ＋ 顺序稳定 ＋ 文案含第一组的事实
// ---------------------------------------------------------------------------

/**
 * P2-H｜协议 **v0.6 §3.2「多组形态」**（§5 表外的一条）。
 *
 * `groups` ≥ 2（两个不同的组同时成立）⇒ 三条断言：
 *
 * 1. **不漏报**：提示的 `groups` 数组**逐组齐全** —— 组数、各组成员数、成员 id
 *    集合，都要与判据**自己**按 §1.1 算出来的一致 ⇒ 少报一组、少一个成员都要红。
 * 2. **顺序稳定**：组按 **`groupKey` 升序**（协议 v0.6：排序落点在 **host 侧**
 *    ⇒ 任何消费方都拿到稳定顺序；顺序抖动会让「后发那条」的内容不稳定）。
 *    **口径**：小写十六进制 ⇒ 按**字节序**比（与 host 的比较、及 Rust 侧
 *    `sort_by` 一致）。
 * 3. **文案含 `groups[0]` 的条数与该组 R**（协议 v0.6「数字来源」：判据取
 *    `groups[0]`）—— 多组文案**逐组列** ⇒ 第一组那两个数必然在文案里。
 *
 * ## 造景（**实机可造**，不是只在合成适配器里成立）
 *
 * · **两个不同 `sessionId`** ⇒ 正文不同（adapter 的正文按 sessionId 生成）
 *   ⇒ §1.1 键里的 `sessionId` 与 `contentDigest` 都不同 ⇒ **两个不同的组**；
 * · **冻结时钟** ⇒ 同组内多次捕获的 `capturedAt` 相同（否则同组不成立）；
 * · 组 A 取 3 次、组 B 取 2 次 ⇒ `groups.length === 2`。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_H_multiGroupNotDropped(adapter) {
  return guard('P2-H-multi-group-not-dropped', async () => {
    adapter.setNow?.(1_767_000_000_000);
    const sessions = [
      { sessionId: 'p2-check-multi-a', count: 3 },
      { sessionId: 'p2-check-multi-b', count: 2 },
    ];
    const refs = [];
    for (const one of sessions) {
      for (let i = 0; i < one.count; i += 1) {
        const picked = await adapter.pick({ sessionId: one.sessionId });
        if (!picked?.ref) {
          return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
        }
        await adapter.settle(picked.ref);
        refs.push(picked.ref);
      }
    }

    // **期望由判据自己算**（§1.1 五字段 ＋ §1.2 的 groupKey）——
    // **不拿实现的输出当标准答案**（这与 P2-1 / P2-3 / P2-4 同一把尺）。
    const expected = new Map();
    for (const ref of refs) {
      const snapshot = adapter.snapshotFile(ref)?.snapshot;
      if (snapshot === undefined) {
        return { status: SKIPPED, detail: `拿不到 ${ref} 的快照（落盘不可观测）` };
      }
      const key = expectedGroupKey(snapshot);
      if (!expected.has(key)) expected.set(key, []);
      expected.get(key).push(ref);
    }
    // **升序 = 字节序**（小写十六进制 ⇒ 码位序与字节序一致）。
    const wantKeys = [...expected.keys()].sort();
    if (wantKeys.length < 2) {
      return {
        status: FAIL,
        detail: `判据自身：造出来只有 ${wantKeys.length} 个组（键没分开）——多组场景的前提不成立`,
      };
    }

    // 提示：**每次新增引用都会重跑一轮核对** ⇒ 连着出好几条 ⇒
    // ⇒ 必须取**最后一轮**那条（否则会拿中间那条「1 个组」当终态）。
    //
    // **等法是事件式的**（PM 02:0x 登记的缺口：墙钟「等稳定 400ms」在重负载下会
    // 假红，且红字会把负载问题说成实现漏报）：
    // ① `trigger` ＝ 最后一次 pick 的 ref ⇒ 那一轮确实跑完了；
    // ② `groups.length` ＝ 期望组数（报满）⇒ 终态。
    const waited = await waitForLastRoundNotice(adapter, 'duplicate-groups', {
      lastRef: refs[refs.length - 1] ?? '',
      // **信号②在本判据里有判别力**（**多组**场景，期望组数 ≥ 2）⇒ 传真实期望值；
      // 单组/单条场景才应该传 0（那时信号②恒不命中 ⇒ 只剩信号①，见该函数注释）。
      expectedGroups: wantKeys.length,
      timeoutMs: 30_000,
    });
    if (waited.notice === undefined) {
      // **超预算 ≠ 漏报**：只报「没等到」，**不**断言「只报了 N 个组」。
      //
      // **为什么不能提前判成漏报**（PM 02:0x 登记的缺口）：这一条等待原来是**墙钟**
      // （「等稳定 400ms」）⇒ 机器被压满时两轮间隔可能超过 400ms ⇒ **提前认定跑完**
      // ⇒ 拿到中间那条（只有 1 个组）⇒ 红字把**负载问题**说成**实现漏报**。
      // ⇒ 现在是**事件式**等待（`trigger` ＝ 最后一次 pick 的 ref ＋ 组数达期望值），
      // 30s 只作**兜底** ⇒ 兜底耗尽时**只**说「没等到」＋「疑似负载过高」。
      //
      // **来源引用不进红字**（PM 02:8x ⑤③）：上面这段来历写在这里（给维护者看），
      // 红字只留**现象 → 条件 → 结论**一条直线。
      return {
        status: FAIL,
        detail: noticeTimeoutWhy(adapter, '「最后一轮」的重复组提示', {
          budgetMs: 30_000,
          // **UX 定稿句式**（PM 02:8x ⑤③）：条件 ＋ 结论之后接「期间见到…」的观测。
          tail:
            `若在低负载下稳定复现，才按漏报论——期间见到最多 ${waited.maxGroups} 个组、` +
            `${waited.count} 条提示`,
        }),
      };
    }
    const notice = waited.notice;
    const carrier = checkNoticeCarrier(notice);
    if (!carrier.ok) {
      return { status: FAIL, detail: carrier.why };
    }

    const groups = Array.isArray(notice.groups) ? notice.groups : [];
    if (groups.length !== wantKeys.length) {
      return {
        status: FAIL,
        detail:
          `提示里只报了 ${groups.length} 个组（应有 ${wantKeys.length} 个）——` +
          '多组时不得漏报（协议 v0.6 §3.2）',
      };
    }
    for (let i = 0; i < wantKeys.length; i += 1) {
      const wantKey = wantKeys[i];
      const wantMembers = [...expected.get(wantKey)].sort();
      const group = groups[i] ?? {};
      if (String(group.groupKey ?? '') !== wantKey) {
        return {
          status: FAIL,
          detail:
            `第 ${i + 1} 组的组标识是 ${String(group.groupKey ?? '')}，按协议 §1.2 应为 ${wantKey}` +
            '（协议 v0.6：组按 `groupKey` 升序 ⇒ 顺序也要对）',
        };
      }
      const gotMembers = Array.isArray(group.members)
        ? group.members.map((member) => String(member?.snapshotId ?? '')).sort()
        : [];
      if (gotMembers.join('|') !== wantMembers.join('|')) {
        return {
          status: FAIL,
          detail:
            `第 ${i + 1} 组成员是 [${gotMembers.join('、')}]，应有 [${wantMembers.join('、')}]——` +
            '多组时不得漏成员（协议 v0.6 §3.2）',
        };
      }
    }

    // 文案：含 **`groups[0]`** 的条数与该组 R（协议 v0.6「数字来源」）。
    const first = groups[0] ?? {};
    const firstMembers = Array.isArray(first.members) ? first.members.length : 0;
    const firstRedundant = Number(first.redundantBytes ?? -1);
    const selfContained = checkSelfContainedText(notice.text, {
      members: firstMembers,
      redundant: firstRedundant,
    });
    if (!selfContained.ok) {
      return { status: FAIL, detail: `多组文案：${selfContained.why}` };
    }

    return {
      status: PASS,
      detail:
        `多组场景（${wantKeys.length} 个组同时成立）⇒ 不漏报（各组 ` +
        `${wantKeys.map((key) => `${expected.get(key).length} 条`).join(' / ')}）、` +
        `组按 groupKey 升序、文案含第一组事实（${firstMembers} 条 / ${firstRedundant} 字节）`,
    };
  });
}

// ---------------------------------------------------------------------------
// P2-J｜发送后：用户眼前那条提示仍不失真
// ---------------------------------------------------------------------------

/**
 * P2-J｜协议 §3.1（后发那条必须把先发那条的事实也带上）＋ §3.2（抗 stale 的锚 ／
 * 错误态 forbidden ／ 必须 `info` 档）。
 *
 * ## 这条回答的问题（UX 提出、PM 02:0x 交回给 tester）
 *
 * **消息发出后，前一轮那条 inline 常驻条还挂着吗**（用户编下一条消息时会不会
 * 看到过期提示）。
 *
 * ## 实测事实与**可测边界**（先划清，再断言）
 *
 * · **客户端侧那条仍在**（发送后客户端的可见提示记录里，那条还在）。
 * · **内核 notice 槽的存续不可机检**（adapter 是假 ctx、没有内核单槽）
 *   ⇒ **不用客户端记录冒充内核槽**：本条断的是「客户端投递的**最后一条**是什么」。
 *
 * ## 为什么断「最后一条」而不是「还在不在」
 *
 * 内核 notice 是**单槽**（最新一条覆盖旧条）⇒ 用户眼前只可能是**最后投递的那条**
 * ⇒ 「那条还在不在」等价于「最后一条是不是它」。
 * 而 §3.1 要求「**后发那条必须把先发那条的事实也带上**」＋ 两条**逐字相同**
 * ⇒ 发送动作**不得**把用户眼前那条换成别的（尤其不得换成发送结果字样）。
 *
 * ## 「不失真」的四条（**全部是协议明文**）
 *
 * 1. **逐字相同**：发送后用户看到的最后一条＝发送前那条（§3.1）。
 * 2. **`level` 仍是 `info`**（§3.2：常驻条必须是 `info` 档，`error` 档只弹瞬态 toast）。
 * 3. **不含发送结果字样**（§3.2 错误态 forbidden 三词）。
 * 4. **不把「本次核对」写成「当前状态」**（§3.2 抗 stale 的锚：条会被内核顶掉、
 *    且**撤不掉** ⇒ 不得写成当前状态的断言）⇒ 禁「现在」「当前」。
 *    另：**仍自包含该组事实**（条数 ＋ 该组 R）⇒ 用户眼前那条仍带完整事实。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP2_J_noticeStaysTruthfulAfterSend(adapter) {
  return guard('P2-J-notice-truthful-after-send', async () => {
    const sessionId = 'p2-check-send-then-strip';
    adapter.setNow?.(1_767_000_000_000);
    const first = await adapter.pick({ sessionId });
    const second = await adapter.pick({ sessionId });
    if (!first?.ref || !second?.ref) {
      return { status: SKIPPED, detail: 'pick 未返回 ref（实现侧尚不可测）' };
    }
    await adapter.settle(first.ref);
    await adapter.settle(second.ref);

    // **发送前**用户看到的那条。
    const before = await waitForNotice(adapter, 'duplicate-groups', { lastRef: second.ref });
    if (before === undefined) {
      return {
        status: FAIL,
        detail:
          '同组成立后——协议 §3.1-1 要求出现「重复组」提示；' +
          noticeTimeoutDetail(adapter, '「重复组」提示', { lastRef: second.ref }),
      };
    }
    const carrier = checkNoticeCarrier(before);
    if (!carrier.ok) {
      return { status: FAIL, detail: carrier.why };
    }

    // —— **发送** ——
    //
    // 判据环境里「发送」的可观测动作＝**产出正文并进模型请求**
    // （P1 A5 / A8、P2-6 同款口径）⇒ 这一步证明「发送」这条路径真的走过了
    // （否则本条的前提不成立 ⇒ 报红并写明是**前提**不成立，不是缺陷）。
    const requestsBefore = adapter.modelRequestCount();
    const sent = await trySerialize(adapter, first.ref);
    const requestsAfter = adapter.modelRequestCount();
    if (!sent.ok) {
      return {
        status: FAIL,
        detail: `发送时正文取不到（${sent.error}）——判据自身的发送前提不成立`,
      };
    }
    if (requestsAfter !== requestsBefore + 1) {
      return {
        status: FAIL,
        detail: `发送后模型请求数不是 +1（${requestsBefore} → ${requestsAfter}）——判据自身的发送前提不成立`,
      };
    }

    // —— **发送之后**，用户眼前的那条（= 客户端投递的**最后一条**）——
    const visible = adapter.visibleNotices?.() ?? [];
    if (visible.length === 0) {
      return {
        status: FAIL,
        detail:
          '发送后客户端的可见提示记录空了——协议 §3.2「可复查」：' +
          '提示的历史不得因发送而消失（否则用户眼前那条无从判断）',
      };
    }
    const after = visible[visible.length - 1];
    const afterText = String(after.text ?? '');

    // ① 逐字相同（§3.1：后发那条必须把先发那条的事实也带上 ＋ 两条逐字相同）
    if (afterText !== String(before.text ?? '')) {
      return {
        status: FAIL,
        detail:
          `发送后用户看到的最后一条**不是**发送前那条（发送前=「${before.text}」／` +
          `发送后=「${afterText}」）——协议 §3.1：后发那条必须把先发那条的事实也带上；` +
          '发送不得把用户眼前那条换成别的',
      };
    }

    // ② 仍是 `info` 档（§3.2：常驻条必须是 `info` 档）
    if (String(after.level ?? '') !== 'info') {
      return {
        status: FAIL,
        detail:
          `发送后用户看到的那条档位是 ${String(after.level ?? '')}——协议 §3.2：` +
          '常驻条必须是 info 档（error 档只弹瞬态 toast、不渲染常驻条）',
      };
    }

    // ③ 不含发送结果字样（§3.2 错误态）
    for (const phrase of FORBIDDEN_SEND_RESULT_WORDS) {
      if (afterText.includes(phrase)) {
        return {
          status: FAIL,
          detail: `发送后用户看到的那条里出现了「${phrase}」——协议 §3.2：提示文案不含发送结果字样`,
        };
      }
    }

    // ④ 不把「本次核对」写成「当前状态」（§3.2 抗 stale 的锚：条撤不掉）
    for (const phrase of STALE_ASSERTION_WORDS) {
      if (afterText.includes(phrase)) {
        return {
          status: FAIL,
          detail:
            `发送后用户看到的那条里出现了「${phrase}」——协议 §3.2：条会被内核顶掉且撤不掉 ` +
            '⇒ 文案不得把「本次核对」写成当前状态的断言',
        };
      }
    }

    // ⑤ 仍自包含该组事实（§3.1 后发自包含 ＋ §3.2 数字口径）
    const afterGroups = Array.isArray(after.groups) ? after.groups : [];
    const afterGroup = afterGroups[0] ?? {};
    const afterMembers = Array.isArray(afterGroup.members) ? afterGroup.members.length : 0;
    const afterRedundant = Number(afterGroup.redundantBytes ?? -1);
    const selfContained = checkSelfContainedText(afterText, {
      members: afterMembers,
      redundant: afterRedundant,
    });
    if (!selfContained.ok) {
      return { status: FAIL, detail: `发送后用户看到的那条：${selfContained.why}` };
    }

    return {
      status: PASS,
      detail:
        `发送后用户看到的最后一条仍是那条重复核对（与发送前**逐字相同**、level=info、` +
        `不含发送结果字样、不把「本次核对」写成当前状态、仍带该组事实 ` +
        `${afterMembers} 条 / ${afterRedundant} 字节）；` +
        '实测事实：客户端侧那条**仍在**；内核 notice 槽的存续**不可机检**' +
        '（adapter 是假 ctx、无内核单槽）⇒ 本条断的是客户端投递侧可观测的部分',
    };
  });
}
