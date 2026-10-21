// P3 协议判据库（协议稿 `docs/features/official-chat/official-chat-p3-protocol.md`
// **v0.2** §5 的**九条验收判据** P3-1…P3-9）。
//
// ## 与 P1/P2 判据库的关系
//
// **独立成一套**（本文件不动 P1/P2 的任何判据）。P1/P2 立下的纪律一条不松：
//
//  · **三态 verdict**：`pass` / `fail` / `skipped`（未执行）。
//    「没触发到被测路径」必须显式报 `skipped`，不能省略、也不能记成 `fail`
//    ——否则「这条没测到」和「这条测了没过」在结果里长得一样。
//  · **期望值从协议稿逐条转写**，**不 import 任何实现模块**。
//    实现侧的 `createSnapshotStore` / `createSweeper` 这些函数**只作为被测对象**
//    经 adapter 调用；「该删哪些、该留哪些」这类期望由本库**按 §S-10 / §S-11
//    的规则自己算**，不拿实现的输出当标准答案。
//  · **判据自己红了改判据不改实现**：本文件抛错一律报「判据自身：…」
//    （与「实现尚未到位」区分开，见交付回执里的逐条归因）。
//
// ## 判据与协议条款的对应
//
// | 判据 | 协议条款 | 判什么 |
// | --- | --- | --- |
// | `P3-1` | §S-8 ＋ §S-9 | **半条记录不得发布**；发布点 = rename 成功那一刻；回读校验 |
// | `P3-2` | §S-8 | **写后回读**：写坏 ⇒ 发布被拒且最终名不存在 |
// | `P3-3` | §S-10 | 第五家族（临时残留）被专门清理；不越界；失败只记日志 |
// | `P3-4` | §S-11 | 占用记录落盘（四字段、不含正文）＋ 发布顺序 |
// | `P3-5` | §S-11 ＋ §S-12 | 被占用不回收；无占用照常回收；**先重建后回收**；R3 |
// | `P3-6` | §S-13 | 孤儿认领可回收；快照在则不删；**不得宣称事务** |
// | `P3-7` | §S-14 | Unix 私有权限；日志不收正文；**不假装加密** |
// | `P3-8` | §S-14 | **落盘策略成文**（可被检索）＋ P1 有指针 |
// | `P3-9` | §S-15 | 账户维度**不得取自请求参数**（省略/同值/不符三段可机检形态）；目录缓存键同理；不符即拒 |
//
// ## 「断性质，不断实现形态」（协议 §S-9 末尾那条注记）
//
// 本库**不**断「用了哪个原子写原语」：
//  · P3-1 断的是「**清单里有几个最终名／几个临时名**」这个**可观测事实**
//    ⇒ 实现用 `fs.rename`、用别的、或者先删后写，判据的结论一样。
//  · P3-1③ 的「回读校验」断的是「**盘上 `contentDigest` ＝ 重新计算值**」
//    ⇒ 判据**自己把文件读回来重算**，不关心实现是不是真的回读过；
//    「回读发生过」这件事由 P3-2 用**反证**（写坏 ⇒ 被拒）来断。
//
// ## 三个「口径未定 ⇒ 判据不钉」的地方（避免假红）
//
// 1. **临时文件的具体命名**：协议 §2.3 给了形态（点开头 ＋ `.tmp-…`），
//    但 §S-9 明写「协议要的是**性质**，不是命名」⇒ P3-3 断的是
//    「**清理后这个残留物还在不在**」这个**结果事实**，不是「文件名叫什么」。
// 2. **占用记录的释放时机**：内核 source 契约无「引用移除」信号（G1-8）
//    ⇒ P3-5 只断**协议明写的那三条**（R1/R2/R3）＋ **次序**，
//    **不断言**「内核引用一移除就立刻释放」。
// 3. **Windows 侧的存储保护**：§S-14 裁定「本批只做**如实记录**」
//    ⇒ P3-7 **只做 Unix 侧的可机检断言**（mode 参数存在且正确），
//    **不做**跨平台加密断言。
//
// ## 清理过程怎么观测（**不靠推断，靠真调用序列**）
//
// P3-3④（清理失败只记日志）与 P3-5③（先扫后删）问的都是
// **实现自己做过什么** ⇒ 判据要看到**真实的调用序列**，
// 而不是「前后比对盘上状态」的推断，也不是 adapter 自己伪造的事件。
// ⇒ adapter 在**子进程**里 patch `node:fs` 的 `readdirSync` / `readFileSync` /
// `renameSync` / `unlinkSync` ⇒ 判据拿到的是**真调用序列**
// （含「有没有读过 `refs/`」「删除发生在扫描之前还是之后」），
// 并可**注入一次删除失败**（EACCES）来验「失败只记日志、不阻塞整轮」。
//
// @module scripts/lib/p3-protocol

import { join } from 'node:path';

import { ERROR_CODES } from './p1-protocol.mjs';

/** verdict 三态。 */
export const PASS = 'pass';
export const FAIL = 'fail';
export const SKIPPED = 'skipped';

/**
 * 造一条 verdict。
 *
 * @param {string} name - 判据名（`P3-{n}-{条款}` 形式，便于回指协议）。
 * @param {string} status - `pass` / `fail` / `skipped`。
 * @param {string} detail - 人读的一句话（失败时必须写清**缺哪条协议**）。
 * @returns {{name: string, status: string, detail: string}}
 */
export function verdict(name, status, detail) {
  return { name, status, detail: String(detail) };
}

/**
 * 汇总计数（与 P1/P2 判据的产物同构）。
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
// 协议期望表（**逐条转写自协议稿 v0.1**，不 import 实现）
// ---------------------------------------------------------------------------

/**
 * §2.4 的**占用记录字段闭集**（四个，一个不多一个不少）。
 *
 * **为什么是闭集**：协议 §S-11 明写「**只有这四个字段**，不含正文、不含摘要」
 * ⇒ 多一个字段就是「混进了不该落盘的东西」（正文与摘要都在纪律之内）。
 */
export const OCCUPANCY_RECORD_FIELDS = Object.freeze([
  'snapshotId',
  'capturedAt',
  'createdAt',
  'sessionId',
]);

/**
 * §S-10 的**临时残留形态**判据（**性质**，不是命名）。
 *
 * ## 为什么不是「文件名以 `.tmp-` 结尾」
 *
 * §S-9 末尾的注记明写「协议要的是**性质**，不是命名」⇒ 判据只问
 * 「**这个东西是不是临时残留**」（最终名之外的物），**不问**它叫什么。
 * ⇒ 判据自己造残留物时**可以**按协议 §2.3 的形态造（那是协议给的**示例形态**），
 * 但断言的对象始终是「**它还在不在**」。
 */
export function isTmpResidualName(name) {
  const text = String(name ?? '');
  if (text.endsWith('.json')) return false;
  return text.includes('.tmp-') || text.startsWith('.');
}

/**
 * §4 的**常量**（**工程参数**，不伪称来自站点限制）。
 */
export const TMP_RESIDUAL_MS = 24 * 60 * 60 * 1000;
export const OCCUPANCY_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;
export const SNAPSHOT_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * 快照正文的**摘要**（按 P1 §1.3 的口径：`sha256(正文)` 的十六进制）。
 *
 * **为什么自己算**：判据要断「盘上 `contentDigest` ＝ **重新计算**值」
 * ⇒ 期望值必须由**判据**算，不能读实现的计算结果（否则两端一起错就查不出来）。
 *
 * @param {string} body - 模型可见正文。
 * @returns {string} 摘要（十六进制）。
 */
export async function expectedDigest(body) {
  const { createHash } = await import('node:crypto');
  return createHash('sha256').update(String(body ?? ''), 'utf8').digest('hex');
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

// ---------------------------------------------------------------------------
// 共享工具（**口径只有一份**）
// ---------------------------------------------------------------------------

/**
 * 判据用的**红字分层**：**现象 ＋ 缺哪条协议**。
 *
 * ## 为什么要分层
 *
 * P1/P2 的纪律（沿用）：红字必须让看的人**一眼分清「缺哪条协议」**，
 * 不能只有一句「不通过」⇒ 分两层：
 *  · **现象层**：实测到什么（可复核的事实）；
 *  · **结论层**：缺哪条协议（点出条款号）。
 *
 * ## 怎么按处豁免
 *
 * `clause` 可以**按处传入**（各条判据点自己的条款）⇒ 共享函数里
 * **只有一套拼装逻辑**，但每处的**结论句**可以不同 ⇒ 不出现「两套逻辑」。
 *
 * @param {object} options
 * @param {string} options.what - 判据在查什么（人话）。
 * @param {string} options.observed - **现象层**：实测到的事实。
 * @param {string} options.clause - **结论层**：缺哪条协议（条款号 ＋ 口径）。
 * @param {string} [options.hint] - 补充指引（可选，**元话语不进红字**）。
 * @returns {string} 红字。
 */
export function p3Why({ what, observed, clause, hint = '' }) {
  const layers = [`${what}：${observed}`, `缺 ${clause}`];
  if (hint !== '') layers.push(hint);
  return layers.join(' ⇒ ');
}

/**
 * **清单计数**的期望值（**判据自己算**，不取实现的输出）。
 *
 * @param {object} options
 * @param {number} options.finalCount - 清单里的最终名个数。
 * @param {number} options.residualCount - 清单里的临时残留个数。
 * @param {number} options.expectFinal - 期望的最终名个数。
 * @param {number} options.expectResidual - 期望的临时残留个数。
 * @returns {{ok: boolean, why: string}}
 */
export function expectCounts({ finalCount, residualCount, expectFinal, expectResidual }) {
  const problems = [];
  if (finalCount !== expectFinal) {
    problems.push(`最终名清单里有 ${finalCount} 个，期望 ${expectFinal} 个`);
  }
  if (residualCount !== expectResidual) {
    problems.push(`临时残留有 ${residualCount} 个，期望 ${expectResidual} 个`);
  }
  return { ok: problems.length === 0, why: problems.join('；') };
}

/**
 * 读一份落盘快照的 `contentDigest`（**读不出来就如实说读不出来**）。
 *
 * @param {object} adapter
 * @param {string} snapshotId
 * @returns {{ok: true, digest: string, body: string}|{ok: false, error: string}}
 */
export function readStoredDigest(adapter, snapshotId) {
  const relative = join('snapshots', adapter.providerId, `${snapshotId}.json`);
  if (!adapter.exists(relative)) return { ok: false, error: '最终名不存在' };
  let parsed;
  try {
    parsed = JSON.parse(adapter.readFile(relative));
  } catch (error) {
    return { ok: false, error: `最终名不是合法 JSON（${String(error?.message ?? error)}）` };
  }
  const messages = Array.isArray(parsed?.messages) ? parsed.messages : [];
  const body = messages.map((one) => String(one?.text ?? '')).join('\n\n');
  return { ok: true, digest: String(parsed?.contentDigest ?? ''), body };
}

/**
 * trace 里的某个路径是不是在 `refs/` 目录下（**占用记录的全量扫描**的识别口径）。
 *
 * ## 为什么按「路径形状」识别
 *
 * 协议 §S-12③ 只说「必须先完成一次**占用记录的全量扫描**」，
 * **没说**实现用什么原语读（列目录再逐个读，还是别的）
 * ⇒ 判据只问「**有没有读过 `refs/` 下面的东西**」。
 *
 * @param {string} path - trace 里的路径。
 * @param {string} root - 落盘根。
 * @returns {boolean}
 */
export function isRefsPath(path, root) {
  const text = String(path ?? '');
  const refs = join(root, 'refs');
  return text === refs || text.startsWith(`${refs}\\`) || text.startsWith(`${refs}/`);
}

/**
 * trace 里的某一项是不是「一次删除」（`rename` 或 `unlink`）。
 *
 * ## 为什么两种都算
 *
 * 实现侧的删除是「两步法」（先 `rename` 再 `unlink`，见 `host/sweep.js`），
 * 但协议**没规定**用哪一步 ⇒ 判据断的是「**发生了删除**」这个事实。
 *
 * @param {object} entry - trace 的一项。
 * @returns {boolean}
 */
export function isRemovalEntry(entry) {
  return entry?.kind === 'rename' || entry?.kind === 'unlink';
}

/**
 * 源码里「写盘权限参数」的**出现次数**（形态容忍：`mode: 0o600` / `mode : 0o600`）。
 *
 * ## 为什么数「出现次数」而不是「有没有」
 *
 * 只判「有没有」会被**同一文件里别处的**写盘点满足 ⇒ 判据先定位
 * **那个写盘点所在的函数正文**，再在正文里数 ⇒ 断的是「**这一处**有没有带」。
 *
 * @param {string} source - 源码正文。
 * @param {string} octal - 权限的八进制字面（`600` / `700`）。
 * @returns {number}
 */
export function countModeArgs(source, octal) {
  const matches = String(source ?? '').match(new RegExp(`mode\\s*:\\s*0o${octal}\\b`, 'g'));
  return matches === null ? 0 : matches.length;
}

/**
 * 取某个**顶层函数**的源码正文（形态容忍：只认函数名，不认行号/变量名）。
 *
 * @param {string} source - 源码全文。
 * @param {string} anchor - 锚点（如 `export function createCatalogCache`）。
 * @returns {string|null} 正文；锚点找不到 ⇒ `null`（调用方退到全文）。
 */
export function functionBody(source, anchor) {
  const text = String(source ?? '');
  const start = text.indexOf(anchor);
  if (start < 0) return null;
  const rest = text.slice(start + anchor.length);
  // **下一个顶层 `function` 之前**（嵌套的函数是缩进的 ⇒ 不会误切）。
  const next = rest.search(/\n(?:export )?(?:async )?function /);
  return next < 0 ? text.slice(start) : text.slice(start, start + anchor.length + next);
}

// ---------------------------------------------------------------------------
// P3-1｜S-8 ＋ S-9：半条记录不得发布 ＋ 发布点 ＋ 回读校验
// ---------------------------------------------------------------------------

/**
 * P3-1｜§S-8 ＋ §S-9。
 *
 * **半条记录不得发布**；**发布点 = rename 成功那一刻**；**回读校验**。
 *
 * ## 怎么断「性质」而不是「实现形态」
 *
 * §S-9 末尾的注记明写：「协议要的是**性质**，不是命名」⇒
 * 本条**不**断「用了 `fs.rename` 没有」，只断三个**可观测事实**：
 *  1. **写到一半失败 ⇒ 最终名清单里没有那个 id**（半条记录没被发布）；
 *  2. **发布成功 ⇒ 最终名存在，且盘上 `contentDigest` ＝ 重新计算值**；
 *  3. **最终名存在 ⇒ 清单里就有一个最终名**（发布点即「最终名的文件存在」）。
 *
 * ## 「半截文件」怎么造出来（**这是本条最难的一处**）
 *
 * 需求是「模拟**写正文时**抛错」⇒ 盘上会留下一份**文件名已在、正文不完整**的文件。
 * ⇒ **不能用 `store` 替身**（替身在写**之前**就抛 ⇒ 盘上什么也没有 ⇒ 假绿）。
 * ⇒ 唯一可靠路径：**子进程内先 patch `node:fs` 的 `writeFileSync`**，
 * 让它**写一半再抛**，**再动态 import 插件**（`node:fs` 的 ESM facade 在首次
 * import 时快照 CJS 导出值 ⇒ 同进程 patch 不生效 ⇒ 必须另起进程；
 * 见 `p3-adapter.mjs` 的 `runInSubprocess` / `fsPatchPreamble`）。
 *
 * ## 口径依赖协议版本（**协议升版时先看这段**）
 *
 * 本条断的「最终名清单」是**协议 §2.2 的口径**：目录扫描本身就是索引，
 * **没有独立索引文件**⇒ 「已发布」＝「最终名的文件存在」。
 * ⇒ 若协议将来改成**独立索引文件**（真的建 `index.json`），
 * 本条的「清单」口径要跟着改（判据**按协议稿走**）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_1_atomicPublish(adapter) {
  return guard('P3-1-atomic-publish', async () => {
    const problems = [];

    // ① **半条记录不得发布**：写正文时抛错 ⇒ 最终名清单里没有那个 id。
    const injected = await adapter.runInjectedCapture?.({
      mode: 'throw-mid',
      snapshotId: 'ds~aaaaaaaaaaaa~0001',
    });
    if (injected === undefined) {
      return {
        status: SKIPPED,
        detail:
          '没有可用的注入观测口（`runInjectedCapture`）——造不出「写正文时抛错」这个场景 ⇒ 本条无法判定',
      };
    }
    // **场景有效性**：注入必须真的写了半个文件（写盘调用 ≥ 1 次）⇒
    // 否则「最终名不存在」说明不了任何事（可能压根没开始写）
    // ⇒ 报「**未执行**」，既不判通过、也不判红。
    if (injected.threw !== true || injected.files.length === 0) {
      return {
        status: SKIPPED,
        detail:
          `注入「写正文时抛错」没有成立（${injected.outcome ?? '未知'}；写盘调用 ${injected.files.length} 次）` +
          '——造不出「半截文件」这个场景 ⇒ 本条无法判定',
      };
    }
    const finalNames = adapter.finalNames();
    if (finalNames.includes('ds~aaaaaaaaaaaa~0001.json')) {
      // **把半截文件的内容与长度带进红字**（可复核的事实，不只是「有这个东西」）。
      let evidence = '';
      try {
        const half = adapter.readFile(
          join('snapshots', adapter.providerId, 'ds~aaaaaaaaaaaa~0001.json'),
        );
        evidence = `（盘上留下 ${half.length} 字节的半截内容：「${half}」）`;
      } catch {
        evidence = '（半截内容读不出来）';
      }
      problems.push(
        '注入「写正文时抛错」后，**最终名清单里出现了那个 id**（半条记录被发布）' +
          evidence +
          '——协议 §S-8：「失败不发布半条记录」',
      );
    }

    // ②＋③ **发布点**：正常发布 ⇒ 最终名存在，且盘上摘要 ＝ 重新计算值。
    const goodId = 'ds~aaaaaaaaaaaa~0002';
    try {
      adapter.store.save({
        snapshotId: goodId,
        providerId: adapter.providerId,
        sessionId: 'p3-session',
        branch: 'main',
        accountScope: 'p3-account',
        capturedAt: '2026-10-21T00:00:00.000Z',
        messages: [{ text: 'P3 判据正文' }],
      });
    } catch (error) {
      problems.push(`正常发布却抛错了：${String(error?.message ?? error)}`);
    }
    if (!adapter.hasFinalName(goodId)) {
      problems.push(`正常发布后最终名不存在（${goodId}.json）——发布点不对`);
    }
    const stored = readStoredDigest(adapter, goodId);
    if (!stored.ok) {
      problems.push(`正常发布后读不回最终名：${stored.error}`);
    } else {
      const want = await expectedDigest(stored.body);
      if (stored.digest !== want) {
        problems.push(
          `盘上 contentDigest（${stored.digest.slice(0, 12)}…）≠ **重新计算**值（${want.slice(0, 12)}…）` +
            '——协议 §S-8②：发布出去的快照，其盘上摘要必须等于重新计算的值',
        );
      }
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '快照落盘的原子发布（协议 §S-8 ＋ §S-9）',
          observed: problems.join('；'),
          clause: '§S-8「写入走临时文件 → 回读校验 → rename；失败不发布半条记录」',
        }),
      };
    }
    return {
      status: PASS,
      detail:
        '写到一半失败 ⇒ 最终名清单里没有那个 id（临时残留留在盘上）；' +
        '正常发布 ⇒ 最终名存在且盘上摘要 ＝ 重新计算值',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-2｜S-8：写后回读
// ---------------------------------------------------------------------------

/**
 * P3-2｜§S-8。
 *
 * **写后回读**：断言「发布前**发生过一次回读**」——用**反证**：
 * 故意把写入内容改坏 ⇒ **发布被拒且最终名不存在**。
 *
 * ## 为什么用反证而不是「数一次读」
 *
 * 「数读了多少次」是**实现形态**（读回的实现可能是 `readFileSync`、
 * 也可能是 `open`＋`read`）⇒ §S-9 的注记禁掉了这种断法。
 * ⇒ 改成断**结果性质**：「**写坏了 ⇒ 不许发布**」——
 * 这只有**真的回读校验过**才会成立（不校验就照样发布）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_2_writeBackVerify(adapter) {
  return guard('P3-2-write-back-verify', async () => {
    const injected = await adapter.runInjectedCapture?.({
      mode: 'throw-mid',
      snapshotId: 'ds~aaaaaaaaaaaa~0003',
      corrupt: true,
    });
    if (injected === undefined) {
      return {
        status: SKIPPED,
        detail: '没有可用的注入观测口（`runInjectedCapture`）——造不出「写坏内容」这个场景 ⇒ 本条无法判定',
      };
    }
    const problems = [];
    if (injected.threw !== true) {
      problems.push(`注入「写坏内容」后落盘**没有报错**（${injected.outcome ?? '未知'}）——场景不成立`);
    }
    if (adapter.hasFinalName('ds~aaaaaaaaaaaa~0003')) {
      problems.push(
        '注入「写坏内容」后，**最终名仍然存在**——' +
          '协议 §S-8②：回读校验不通过 ⇒ 不 rename ⇒ 最终名不得存在',
      );
    }
    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '写后回读校验（协议 §S-8②）',
          observed: problems.join('；'),
          clause: '§S-8②「rename 之前把临时文件读回来重算 contentDigest 并比对；不一致 ⇒ 不 rename」',
        }),
      };
    }
    return {
      status: PASS,
      detail: '故意写坏内容 ⇒ 发布被拒且最终名不存在 ⇒ 写后确实做过回读校验',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-3｜S-10：崩溃残留（第五家族）必须被专门清理
// ---------------------------------------------------------------------------

/**
 * P3-3｜§S-10。
 *
 * **第五家族被清**：造一个符合临时名形态的残留文件 ⇒ 跑一次清理 ⇒ **它被删**；
 * **不该删的不删**；**越界不发生**；**失败只记日志**。
 *
 * ## 为什么「不该删的不删」也要断
 *
 * §S-10③ 说清理条件 = 形态匹配 ＋ 在本实例根下 ＋ 超过阈值 ⇒
 * 「**阈值内的临时文件不该被删**」是条款的一部分（不是实现细节）⇒ 必须断。
 *
 * ## 「失败只记日志」怎么观测
 *
 * 实现侧的清理留痕走 `ctx.logger.warn`（**不写进 `<root>/logs/`**）⇒
 * 判据**必须注入 logger 桩**来观测（adapter 的 `runSweep` 把子进程里
 * logger 收到的行回传）⇒ 断的是「**有留痕** ＋ **不抛异常** ＋ **不拖垮整轮**」。
 *
 * ## 「删除失败」怎么造出来
 *
 * **不靠改实现**：adapter 在**子进程**里 patch `node:fs` 的 `renameSync`
 * ⇒ 指定路径的删除**抛 EACCES**（模拟「删不掉」）⇒
 * 判据看实现**自己**怎么应对（留痕？抛异常？整轮停摆？）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_3_tmpResidualSwept(adapter) {
  return guard('P3-3-tmp-residual-swept', async () => {
    const problems = [];
    const providerId = adapter.providerId;
    const snapDir = join('snapshots', providerId);

    // ① **该清的被清**：造一个**超过阈值**的临时残留。
    const oldResidual = `.ds~aaaaaaaaaaaa~0009.json.tmp-1234-0009-1`;
    const freshResidual = `.ds~aaaaaaaaaaaa~0010.json.tmp-1234-0010-1`;
    adapter.ensureDir(snapDir);
    adapter.writeFile(join(snapDir, oldResidual), '{"partial":');
    adapter.writeFile(join(snapDir, freshResidual), '{"partial":');
    // **时间戳要真的老过阈值**（阈值是 mtime 口径 ⇒ 造完再改 mtime）。
    adapter.setMtimeOld(join(snapDir, oldResidual), adapter.tmpResidualMs + 60_000);
    adapter.setMtimeOld(join(snapDir, freshResidual), 0);
    // **对照物**：一份**合法快照**（不是临时对象）⇒ 清理不该碰它。
    // ⇒ 用**新鲜的** `capturedAt` ⇒ 它不是「过期快照」⇒ 被删就一定是「越界清理」。
    const controlId = 'ds~aaaaaaaaaaaa~0011';
    adapter.writeSnapshot(controlId, { capturedAt: new Date().toISOString() });

    // **根外哨兵**：清理**绝不允许**越出缓存根（§S-10④）。
    const outsideSentinel = join(adapter.root, '..', `dsh-p3-outside-${Date.now()}.tmp-1`);
    adapter.writeFileAbsolute(outsideSentinel, 'outside');

    const sweep = await adapter.runSweep({});
    if (sweep?.broken === true) {
      return {
        status: SKIPPED,
        detail: `清理过程的观测失败（${String(sweep.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    if (sweep?.threw === true) {
      problems.push(`跑清理时抛了异常：${String(sweep.outcome ?? '')}`);
    }

    // ① 该清的**没了**。
    if (adapter.exists(join(snapDir, oldResidual))) {
      problems.push(
        '超过阈值的临时残留**没被删**——' +
          '协议 §S-10：临时文件是**第五家族**，必须被专门清理（不是等 7 天保留期兜底）',
      );
    }
    // ② 不该删的**还在**。
    if (!adapter.exists(join(snapDir, freshResidual))) {
      problems.push('**阈值内**的临时残留被删了——协议 §S-10③：清理条件含「超过阈值」');
    }
    if (!adapter.exists(join(snapDir, `${controlId}.json`))) {
      problems.push(
        '**不是临时对象**的合法快照被删了——协议 §S-10③：只清临时对象（不是「把快照目录清空」）',
      );
    }
    // ③ 越界**没发生**。
    if (!adapter.existsAbsolute(outsideSentinel)) {
      problems.push('**缓存根之外**的文件被删了——协议 §S-10④：清理不得越出缓存根');
    }
    adapter.removeAbsolute(outsideSentinel);

    // ④ **失败只记日志**：**注入一次删除失败**（EACCES，模拟「删不掉」）
    // ⇒ 断三件事：**有留痕**、**不抛异常**、**整轮继续**（后面的目标照样被清）。
    const failId = 'ds~aaaaaaaaaaaa~0012';
    const otherId = 'ds~aaaaaaaaaaaa~0013';
    adapter.writeSnapshot(failId, { capturedAt: '2026-01-01T00:00:00.000Z' });
    adapter.writeSnapshot(otherId, { capturedAt: '2026-01-01T00:00:00.000Z' });
    const failPath = adapter.pathOf(join(snapDir, `${failId}.json`));
    const failSweep = await adapter.runSweep({ failRemove: [failPath] });
    if (failSweep?.broken === true) {
      return {
        status: SKIPPED,
        detail: `删除失败场景的观测失败（${String(failSweep.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    if (failSweep?.threw === true) {
      problems.push(`删除失败时整轮抛了异常：${String(failSweep.outcome ?? '')}`);
    }
    if ((failSweep?.logs ?? []).length === 0) {
      problems.push(
        '删除失败时**没有任何留痕**——' +
          '协议 §S-10⑤ ＋ P1 §4.1 第 3 条：清理失败只记日志（必须留痕），不阻塞业务',
      );
    }
    if (adapter.exists(join(snapDir, `${otherId}.json`))) {
      problems.push(
        '一次删除失败之后，整轮清理**没有继续**（后面的过期目标也没被清）——' +
          '协议 §S-10⑤：清理失败只记日志，不阻塞业务',
      );
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '崩溃残留（第五家族）的清理（协议 §S-10）',
          observed: problems.join('；'),
          clause: '§S-10「临时文件是第五家族；只清本功能未引用的临时对象；不得越出缓存根；失败只记日志」',
        }),
      };
    }
    return {
      status: PASS,
      detail:
        '超阈值的临时残留被删；阈值内的与非临时对象形态的**没被删**；' +
        '根外哨兵**还在**；删除失败**有留痕、不抛异常、且整轮继续**',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-4｜S-11：占用记录落盘与发布顺序
// ---------------------------------------------------------------------------

/**
 * P3-4｜§S-11。
 *
 * ① **占用记录落盘**：`capture` 成功 ⇒ `<root>/refs/<snapshotId>.json` **存在**，
 *    且**只含**四个字段、**不含正文**；
 * ② **发布顺序**：占用记录的写入**不早于**快照发布（快照不存在时不应有占用记录）。
 *
 * ## 口径依赖协议版本（**协议升版时先看这段**）
 *
 * 本条断的「四个字段」是**协议 §2.4 的闭集** ⇒ 若协议将来放宽
 * （比如加一个 `sourceFingerprint`），本条的字段闭集**要跟着改**。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_4_occupancyRecorded(adapter) {
  return guard('P3-4-occupancy-recorded', async () => {
    const problems = [];

    // **先真的 capture 一次**（用 host 自己分配的 id）⇒ 才有「capture 成功」这个前提。
    // ⇒ 占用记录必须**紧跟**这次成功落盘而写。
    const snapshotId = await adapter.allocateSnapshotId('p3-session');
    const commands = adapter.buildCommands();
    const captured = await commands.capture({
      requestId: 'p3-4-capture',
      snapshotId,
      sessionId: 'p3-session',
    });
    if (captured?.ok !== true) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '占用记录的落盘与发布顺序（协议 §S-11）',
          observed: `capture 失败了（${String(captured?.code ?? '')}）⇒ 场景不成立，本条无法判定`,
          clause: '§S-11①',
        }),
      };
    }

    // ① **落盘** + **字段闭集** + **不含正文**。
    const exists = adapter.occupancyExists(snapshotId);
    if (!exists) {
      problems.push(
        `capture 成功后**没有**占用记录（refs/${snapshotId}.json）——` +
          '协议 §S-11①：capture 成功、快照发布之后**立即**写占用记录',
      );
    } else {
      const read = adapter.readOccupancy(snapshotId);
      if (!read.ok) {
        problems.push(`占用记录读不出来：${read.error}`);
      } else {
        const value = read.value ?? {};
        const keys = Object.keys(value).sort();
        const want = [...OCCUPANCY_RECORD_FIELDS].sort();
        if (keys.length !== want.length || keys.some((one, i) => one !== want[i])) {
          problems.push(
            `占用记录的字段是 [${keys.join('、')}]，协议 §2.4 要求恰好 [${want.join('、')}]`,
          );
        }
        // **不含正文**：任何一段正文都不许落在占用记录里。
        const raw = adapter.readFile(join('refs', `${snapshotId}.json`));
        if (raw.includes('messages') || raw.includes('"body"')) {
          problems.push('占用记录里**含正文**（messages/body）——协议 §S-14：日志与诊断包不收正文');
        }
      }
    }

    // ② **发布顺序**：快照不存在 ⇒ 不得有占用记录。
    const orphanId = 'ds~aaaaaaaaaaaa~0021';
    if (adapter.occupancyExists(orphanId) && !adapter.hasFinalName(orphanId)) {
      problems.push(
        `快照不存在却有占用记录（${orphanId}）——` +
          '协议 §S-11①：先写占用记录**再**对外宣告「已保存」⇒ 占用记录**不早于**快照发布',
      );
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '占用记录的落盘与发布顺序（协议 §S-11）',
          observed: problems.join('；'),
          clause: '§S-11①「capture 成功、快照发布之后立即写 refs/<snapshotId>.json；先写占用记录，再对外宣告已保存」',
        }),
      };
    }
    return {
      status: PASS,
      detail: 'capture 成功 ⇒ 占用记录存在、恰好四个字段且不含正文；快照不存在时没有占用记录',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-5｜S-11 ＋ S-12：回收与占用的关系
// ---------------------------------------------------------------------------

/**
 * P3-5｜§S-11 ＋ §S-12。
 *
 * ① **被占用的快照不被回收**；② **无占用的照常回收**；
 * ③ **先重建、后回收**（**次序**，不得只断「最后结果」）；
 * ④ **R3**：占用记录**写坏** ⇒ 快照**不被删**。
 *
 * ## ③ 为什么必须断「次序」而不是「最后结果」
 *
 * §S-12③ 明写：「**可测试规则**：『回收动作之前，必须先完成一次占用记录的全量扫描』」
 * ⇒ 「最后结果相同」的两种实现（先扫后删 vs 先删后扫）**结果可能一样** ⇒
 * 只断结果就放过了后者 ⇒ 必须断**次序**。
 *
 * ## 口径依赖协议版本（**协议升版时先看这段**）
 *
 * 本条**不断言**「内核引用一移除就立刻释放」（内核 source 契约无该信号，G1-8）
 * ⇒ 只断协议明写的那三条（R1/R2/R3）＋ 次序。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_5_reclaimRespectsOccupancy(adapter) {
  return guard('P3-5-reclaim-respects-occupancy', async () => {
    const problems = [];
    const providerId = adapter.providerId;

    // 造三份快照：① 有占用记录 ② 无占用记录 ③ 占用记录写坏。
    const occupiedId = 'ds~aaaaaaaaaaaa~0030';
    const freeId = 'ds~aaaaaaaaaaaa~0031';
    const brokenId = 'ds~aaaaaaaaaaaa~0032';
    for (const id of [occupiedId, freeId, brokenId]) {
      adapter.writeSnapshot(id, { capturedAt: '2026-01-01T00:00:00.000Z' });
      // **过期**：capturedAt 很老 ⇒ 落在保留期之外。
      adapter.setMtimeOld(join('snapshots', providerId, `${id}.json`), adapter.retentionMs + 60_000);
    }
    adapter.writeOccupancy(occupiedId, {
      snapshotId: occupiedId,
      capturedAt: '2026-01-01T00:00:00.000Z',
      createdAt: '2026-01-01T00:00:00.000Z',
      sessionId: 'p3-session',
    });
    adapter.writeOccupancy(brokenId, '{ 这不是合法 JSON');

    // **记录清理的调用次序**（子进程内 patch `node:fs` ⇒ 观测到的是**真调用序列**，
    // 不是「前后比对盘上状态」的推断）。
    const sweep = await adapter.runSweep({});
    if (sweep?.broken === true) {
      return {
        status: SKIPPED,
        detail: `清理过程的观测失败（${String(sweep.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    if (sweep?.threw === true) {
      problems.push(`跑回收时抛了异常：${String(sweep.outcome ?? '')}`);
    }

    // ① 有占用记录 ⇒ **还在**。
    if (!adapter.hasFinalName(occupiedId)) {
      problems.push(
        '**有占用记录**的过期快照被删了——' +
          '协议 §S-11③：回收只处理未引用记录（存在 ⇒ 不删，除非它自己也过期）',
      );
    }
    // ② 无占用记录 ⇒ **被删**。
    if (adapter.hasFinalName(freeId)) {
      problems.push(
        '**无占用记录**的过期快照没被删——' +
          '协议 §S-11③：回收**只**处理未引用记录 ⇒ 无占用的过期快照照常回收',
      );
    }
    // ④ **R3**：占用记录写坏 ⇒ 按被占着处理 ⇒ **不删**。
    if (!adapter.hasFinalName(brokenId)) {
      problems.push(
        '占用记录**写坏**的快照被删了——' +
          '协议 §S-11 R3：读不动 ⇒ 「被占着」无法证伪 ⇒ 按被占着处理（宁可不清理，不乱删）',
      );
    }
    // ③ **次序**：占用记录的全量扫描 **发生在** 任何快照删除 **之前**。
    const trace = Array.isArray(sweep?.trace) ? sweep.trace : [];
    const occupancyScanAt = trace.findIndex((one) => isRefsPath(one?.path, adapter.root));
    const firstRemovalAt = trace.findIndex((one) => isRemovalEntry(one));
    if (occupancyScanAt < 0) {
      problems.push(
        '回收过程里**看不到对占用记录的任何读取**——' +
          '协议 §S-12③：回收动作之前，必须先完成一次占用记录的全量扫描（先重建占用关系，再回收）',
      );
    } else if (firstRemovalAt >= 0 && occupancyScanAt > firstRemovalAt) {
      problems.push(
        '占用记录的全量扫描发生在**快照删除之后**——' +
          '协议 §S-12③：先重建占用关系，再回收（顺序是硬要求）',
      );
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '回收与占用记录的关系（协议 §S-11 ＋ §S-12）',
          observed: problems.join('；'),
          clause: '§S-11③「先查引用、再删快照」＋ §S-12③「先重建占用关系，再回收」＋ R3',
        }),
      };
    }
    return {
      status: PASS,
      detail:
        '有占用的过期快照**还在**；无占用的过期快照**被删**；占用记录写坏的**还在**；' +
        '占用记录的全量扫描**发生在**任何快照删除之前',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-6｜S-13：跨文件提交序（认领 × 快照）
// ---------------------------------------------------------------------------

/**
 * P3-6｜§S-13。
 *
 * ① **孤儿认领可回收**（认领在、快照不在 ⇒ 回收后认领被删）；
 * ② **快照还在 ⇒ 认领不被删**；
 * ③ **不得宣称事务**：快照最终名不存在时，**不得**有任何「已保存」对外结果。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_6_orphanClaimReclaimed(adapter) {
  return guard('P3-6-orphan-claim-reclaimed', async () => {
    const problems = [];
    const providerId = adapter.providerId;

    // 造两个认领：① 孤儿（有认领、无快照）② 快照还在。
    // **② 的快照必须「没过期」**——协议 §S-13② 保护的是「快照还没过期」的那一份
    // （过期快照在本轮被删后，它的过期认领**跟着**回收是**正确**行为，
    // 见 `host/sweep.js` 的「同轮效果」注释）⇒ 这里给它一个**新鲜**的 capturedAt。
    const orphan = adapter.writeClaim('ds~aaaaaaaaaaaa~0040', { orphan: true });
    const kept = adapter.writeClaim('ds~aaaaaaaaaaaa~0041', { orphan: false });
    adapter.writeSnapshot('ds~aaaaaaaaaaaa~0041', { capturedAt: new Date().toISOString() });

    const sweep = await adapter.runSweep({});
    if (sweep?.broken === true) {
      return {
        status: SKIPPED,
        detail: `清理过程的观测失败（${String(sweep.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    if (sweep?.threw === true) {
      problems.push(`跑回收时抛了异常：${String(sweep.outcome ?? '')}`);
    }

    // ① 孤儿认领**被删**。
    // **认领文件是绝对路径** ⇒ 用 `existsAbsolute` 判（拿 `join(root, 绝对路径)` 拼
    // 会拼出垃圾路径 ⇒ 「还在」永远不成立 ⇒ 假红）。
    if (adapter.existsAbsolute(orphan)) {
      problems.push(
        '孤儿认领（有认领、无快照）**没被删**——' +
          '协议 §S-13②：孤儿认领可被回收；回收后不因此产生半条记录',
      );
    }
    // ② 快照还在 ⇒ 认领**不被删**。
    if (!adapter.existsAbsolute(kept)) {
      problems.push(
        '快照还在，认领**却被删了**——' +
          '协议 §S-13② ＋ P1 §4.1：快照还在就不删认领（删了会让那个序号被二次分配）',
      );
    }

    // ③ **不得宣称事务**：快照最终名不存在 ⇒ 不得有「已保存」的对外结果。
    // **必须用 host 自己分配的 id**（`capture` 要求 id 的会话段对得上、且 seq 已被认领）
    // ⇒ 走一次 `catalog` 拿真 id ⇒ 否则这条会因「id 不合法」而**测不到真正的语义**。
    const commands = adapter.buildCommands();
    const txId = await adapter.allocateSnapshotId('p3-session');
    const result = await commands.capture({
      requestId: 'p3-6-no-transaction',
      snapshotId: txId,
      sessionId: 'p3-session',
    });
    const ok = result?.ok === true;
    if (ok) {
      const published = adapter.hasFinalName(txId);
      if (!published) {
        problems.push(
          '命令层回了「已保存」，但**快照最终名不存在**——' +
            '协议 §S-13③：「这条引用已保存」的唯一依据是快照最终名存在',
        );
      }
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '认领 × 快照的提交序（协议 §S-13）',
          observed: problems.join('；'),
          clause: '§S-13「认领在先、快照在后；孤儿认领可回收；不得出现两个文件各自原子写就宣布完成」',
        }),
      };
    }
    return {
      status: PASS,
      detail: '孤儿认领被回收；快照还在时认领保留；快照最终名不存在时没有「已保存」的对外结果',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-7｜S-14：存储保护与落盘策略
// ---------------------------------------------------------------------------

/**
 * P3-7｜§S-14。
 *
 * ① **Unix 私有权限**：目录缓存文件 `0600`、目录 `0700`（快照/认领已有 ⇒ 防回退）；
 * ② **日志不收正文**：错误日志/诊断包**不含**正文与 `messages`（**钉住现有行为**）；
 * ③ **不假装加密**：**不得**出现「已加密」类表述或把 `0600` 当加密的措辞。
 *
 * ## ③ 的扫描范围（**必须限定**）
 *
 * 「把 `0600` 当加密」这类措辞在**协议稿自己的否定句**里就有
 * （`official-chat-p3-protocol.md` 的 §S-14 与 §5 P3-7 行都在说
 * 「**不**把 `0600` 等同于加密」）⇒ 扫全仓会命中这些**否定句**而假红。
 * ⇒ 只扫**实现代码**（`plugins/**`）的**肯定式**措辞。
 *
 * ## Windows 侧（§S-14 裁定 2）：**不许空跑通过**
 *
 * `mode` 在 Windows 上是 **no-op**（Node 语义）⇒ **盘上权限断不了**
 * ⇒ 本条在 Windows 上做两件事，**两件都要**：
 *  1. **显式记「未执行」**（结论里写明「盘上权限未执行」）
 *     —— §S-14 裁定 2 本批只做如实记录，**如实记录是合规的**；
 *  2. **改断源码形态**（目录缓存／快照／认领三个写盘点有没有带
 *     `mode: 0o700` / `mode: 0o600`）⇒ **不是空跑**。
 *
 * ## 为什么不能写成 `if (win32) { …断言… }`（没有 else）
 *
 * 那样在 Windows 上那段断言**根本不会执行** ⇒ 判据照样报「通过」
 * ⇒ 等于给一条**没跑**的用例盖了「通过」的章 ⇒ **空跑通过**。
 * ⇒ 纪律：**没跑的那部分必须显式说「未执行」**，并且**另找一条能真失败的**顶上。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_7_storageProtection(adapter) {
  return guard('P3-7-storage-protection', async () => {
    const problems = [];
    /** 结论里要**显式说清**「哪一部分没跑」（不许空跑通过）。 */
    const notes = [];
    const commands = adapter.buildCommands();

    // ① **Unix 私有权限**：**走实现自己的目录缓存写入** ⇒ 再看盘上真实权限。
    //    （**不能用判据自己写的探针文件** —— 那是判据自己的权限，不是实现的。）
    await commands.catalog({ requestId: 'p3-7-perm', query: 'p3-perm-probe' });
    const cacheDir = join('catalog-cache');
    const cacheFiles = adapter.listDir(cacheDir).filter((one) => one.endsWith('.json'));
    if (cacheFiles.length === 0) {
      problems.push(
        '目录缓存目录里**没有缓存文件**——实现没写目录缓存 ⇒ 它的权限无从核对（协议 §S-14②）',
      );
    } else if (adapter.isUnix) {
      // **Unix 侧：断盘上真实权限**（协议 §S-14②）。
      const dirMode = adapter.modeOf(cacheDir);
      const fileMode = adapter.modeOf(join(cacheDir, cacheFiles[0]));
      if ((dirMode & 0o777) !== 0o700) {
        problems.push(
          `目录缓存目录的权限是 ${(dirMode & 0o777).toString(8)}，协议 §S-14 要求 0700`,
        );
      }
      if ((fileMode & 0o777) !== 0o600) {
        problems.push(
          `目录缓存文件的权限是 ${(fileMode & 0o777).toString(8)}，协议 §S-14 要求 0600`,
        );
      }
      // **防回退**：快照的私有权限**仍在**。
      const snapId = 'ds~aaaaaaaaaaaa~0050';
      adapter.store.save({
        snapshotId: snapId,
        providerId: adapter.providerId,
        sessionId: 'p3-session',
        branch: 'main',
        accountScope: 'p3-account',
        capturedAt: '2026-10-21T00:00:00.000Z',
        messages: [{ text: 'P3 判据正文' }],
      });
      const snapMode = adapter.modeOf(join('snapshots', adapter.providerId, `${snapId}.json`));
      if ((snapMode & 0o777) !== 0o600) {
        problems.push(
          `快照文件的权限是 ${(snapMode & 0o777).toString(8)}，协议 §S-14 要求 0600（防回退）`,
        );
      }
    } else {
      // **Windows 侧（§S-14 裁定 2）**：`mode` 是 no-op ⇒ **盘上权限断不了**
      // ⇒ 「盘上权限」这一项**显式记「未执行」**（本批只做如实记录），
      //    并且**改断源码形态**（写盘点有没有带权限参数）——
      //    **不许写成没有 else 的 `if (win32) {…}`** ⇒ 那在 Windows 上是**空跑通过**，
      //    等于给一条没跑的用例盖了「通过」的章。
      notes.push(
        '盘上权限**未执行**（Windows：`mode` 是 no-op；§S-14 裁定 2「本批只做如实记录」）' +
          '⇒ 改断**源码形态**（写盘点有没有带权限参数）',
      );
      const writePoints = [
        { file: 'commands.js', anchor: 'export function createCatalogCache', modes: ['700', '600'], what: '目录缓存' },
        { file: 'index.js', anchor: 'export function createSnapshotStore', modes: ['600'], what: '快照写入' },
        { file: 'seq-store.js', anchor: 'export function createSeqAllocator', modes: ['600'], what: '认领写入' },
      ];
      for (const one of writePoints) {
        const source = adapter.readHostFile?.(one.file) ?? '';
        if (source === '') {
          problems.push(`读不到实现源码（${one.file}）——Windows 侧的替代断言没法做`);
          continue;
        }
        // **只在该写盘点所在的函数正文里数**（数整份文件会被别处的写盘点满足）。
        const body = functionBody(source, one.anchor) ?? source;
        for (const mode of one.modes) {
          if (countModeArgs(body, mode) === 0) {
            problems.push(
              `${one.what}（${one.file}）的写盘点**没有** \`mode: 0o${mode}\` 参数（源码形态）——` +
                '协议 §S-14②：目录 0700 ／ 文件 0600',
            );
          }
        }
      }
    }

    // ② **日志不收正文**：错误日志只有四字段，且**不含**正文。
    const failed = await commands.capture({
      requestId: 'p3-7-log-probe',
      snapshotId: 'not-a-valid-id',
      sessionId: 'p3-session',
      accountScope: 'p3-account',
    });
    const logPath = join('logs', 'official-chat.log');
    if (!adapter.exists(logPath)) {
      problems.push('错误日志文件不存在——协议 §S-14：日志与诊断包不收正文（这条要钉住）');
    } else {
      const raw = adapter.readFile(logPath);
      if (raw.includes('messages') || raw.includes('"body"')) {
        problems.push('错误日志里**含正文**（messages/body）——协议 §S-14：日志与诊断包不收正文');
      }
      for (const line of raw.split('\n').filter((one) => one.trim() !== '')) {
        let parsed;
        try {
          parsed = JSON.parse(line);
        } catch {
          problems.push('错误日志有一行不是合法 JSON（诊断证据不可解析）');
          continue;
        }
        const keys = Object.keys(parsed).sort();
        const want = ['at', 'code', 'detail', 'requestId'];
        if (keys.length !== want.length || keys.some((one, i) => one !== want[i])) {
          problems.push(
            `错误日志那行的字段是 [${keys.join('、')}]，协议 §S-14 只许 [${want.join('、')}]`,
          );
        }
        break;
      }
    }
    void failed;

    // ③ **不假装加密**：扫**实现代码**里的肯定式措辞（协议稿里的否定句不算）。
    const encryptionClaims = adapter.scanEncryptionClaims();
    if (encryptionClaims.length > 0) {
      problems.push(
        `实现代码里出现「把 0600 当加密」的表述：${encryptionClaims.join('；')}——` +
          '协议 §S-14①：不声称跨平台加密',
      );
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail:
          p3Why({
            what: '存储保护与落盘策略（协议 §S-14）',
            observed: problems.join('；'),
            clause: '§S-14「快照与占用记录 0600/0700；目录缓存补齐；日志不收正文；不声称跨平台加密」',
          }) + (notes.length > 0 ? ` ⇒ 另：${notes.join('；')}` : ''),
      };
    }
    return {
      status: PASS,
      detail:
        (adapter.isUnix
          ? '目录缓存目录 0700 ／ 文件 0600（**盘上真实权限**）；快照 0600（防回退）；'
          : `${notes.join('；')}；`) +
        '错误日志只有 at/code/requestId/detail 四字段且不含正文；' +
        '实现代码没有把 0600 当加密的表述',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-8｜S-14：落盘策略成文
// ---------------------------------------------------------------------------

/**
 * P3-8｜§S-14。
 *
 * **落盘策略成文**：本协议 §S-14 存在**可被检索到的**条目（标题/锚点命中），
 * 且 **P1 协议里有指向它的指针** ⇒ 「策略写进决策记录」这条**可机检**。
 *
 * ## 为什么这条要机检而不是靠人读
 *
 * §S-14 的裁定原文：「落盘策略**写进** P0/P1 决策记录」⇒
 * 「写进去了」是一个**关于文档的事实**⇒ 可机检（检索命中 ＋ 指针存在）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_8_policyDocumented(adapter) {
  return guard('P3-8-policy-documented', async () => {
    const problems = [];
    const p3Doc = adapter.readRepoFile('docs/features/official-chat/official-chat-p3-protocol.md');
    const p1Doc = adapter.readRepoFile('docs/features/official-chat/official-chat-p1-protocol.md');
    if (p3Doc === null) {
      problems.push('读不到 P3 协议稿');
    } else if (!/^###\s*S-14[｜|]/m.test(p3Doc)) {
      problems.push('P3 协议稿里**没有** §S-14 的可检索条目（标题形如「### S-14｜…」）');
    }
    if (p1Doc === null) {
      problems.push('读不到 P1 协议稿');
    } else if (!p1Doc.includes('official-chat-p3-protocol.md') || !p1Doc.includes('S-14')) {
      problems.push(
        'P1 协议稿里**没有**指向 P3 §S-14 的指针——' +
          '协议 §S-14①：另在 P1 §4.1 后加指针，不复制正文',
      );
    }
    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '落盘策略成文（协议 §S-14①）',
          observed: problems.join('；'),
          clause: '§S-14①「落盘策略条目（本节即为该条目的正文；另在 P1 §4.1 后加指针，不复制正文）」',
        }),
      };
    }
    return {
      status: PASS,
      detail: 'P3 协议稿有可检索的 §S-14 条目，且 P1 协议稿里有指向它的指针',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-9｜S-15：账户维度由服务端推导
// ---------------------------------------------------------------------------

/**
 * P3-9｜§S-15。
 *
 * 按协议稿 **v0.2 §5 P3-9** 的**可机检形态**断（性质一字未放宽，只换观察方式）：
 * ① **省略 `accountScope` 调用** ⇒ 落盘字段 ＝ 服务端推导值；
 * ② **显式传服务端同值** ⇒ 落盘字段**相同**（两次相同 ⇒ 与请求参数形态无关）；
 * ③ **显式传不符值** ⇒ 拒（`account_changed`）**且** 缓存文件数不增、快照不落。
 * 另：**目录缓存键同理**（不同请求参数 ⇒ 不落到不同缓存文件）。
 *
 * ## 「服务端推导值」怎么取（**不拿实现输出当标准答案**）
 *
 * 判据**不**去读实现的闭包变量（那是实现的输出）⇒ 用**协议给的观察方式**：
 *  · 先**省略** `accountScope` 调一次 ⇒ 落盘的那个字段**就是**服务端推导值
 *    （协议 §S-15③：客户端不传时写服务端推导值）；
 *  · 再**把它原样传回去**（= 服务端同值）⇒ 必须**放行**，且落盘字段**仍是同一个**
 *    ⇒ 这就把「字段与请求参数无关」钉住了（传什么就是什么 ⇒ 字段就取了请求参数）。
 * ⇒ 推导算法可以变，**性质不变**。
 *
 * ## 为什么不是「用两个不同的值调 ⇒ 落盘字段相同」
 *
 * v0.1 的那个形态在 §S-15④（「不符即拒」）下**不可机检**：两个不同的值里
 * **至少有一个不符** ⇒ 被拒 ⇒ 无从比较两个落盘字段 ⇒ v0.2 已把它改写成上面那三段。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_9_accountScopeServerDerived(adapter) {
  return guard('P3-9-account-scope-server-derived', async () => {
    const problems = [];
    const commands = adapter.buildCommands();
    const cacheDir = join('catalog-cache');

    // ① **省略 `accountScope` 调用** ⇒ 落盘字段 ＝ 服务端推导值。
    const firstId = await adapter.allocateSnapshotId('p3-session');
    const first = await commands.capture({
      requestId: 'p3-9-omit',
      snapshotId: firstId,
      sessionId: 'p3-session',
    });
    if (first?.ok !== true) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '账户维度由服务端推导（协议 §S-15）',
          observed: `省略 accountScope 时 capture 失败了（${String(first?.code ?? first?.error?.code ?? '')}）——` +
            '协议 §S-15①：客户端不传时应当按**服务端推导**的账户维度正常落盘',
          clause: '§S-15①③',
        }),
      };
    }
    const firstRelative = join('snapshots', adapter.providerId, `${firstId}.json`);
    if (!adapter.exists(firstRelative)) {
      problems.push('省略 accountScope 的那次 capture 成功后，最终名不存在');
    }
    const derived = adapter.exists(firstRelative)
      ? String(JSON.parse(adapter.readFile(firstRelative)).accountScope ?? '')
      : '';
    if (derived === '') {
      problems.push('省略 accountScope 时落盘的 accountScope 字段是空串——服务端推导值不该为空');
    }

    // ② **显式传服务端同值**（＝上一步落盘的那个值）⇒ 放行，且落盘字段**相同**。
    const secondId = await adapter.allocateSnapshotId('p3-session-2');
    const second = await commands.capture({
      requestId: 'p3-9-same',
      snapshotId: secondId,
      sessionId: 'p3-session-2',
      accountScope: derived,
    });
    if (second?.ok !== true) {
      problems.push(
        `显式传服务端同值（${derived}）却被拒（${String(second?.code ?? second?.error?.code ?? '')}）——` +
          '协议 §S-15①：同值应当放行（只有不符才拒）',
      );
    } else {
      const secondRelative = join('snapshots', adapter.providerId, `${secondId}.json`);
      const field = adapter.exists(secondRelative)
        ? String(JSON.parse(adapter.readFile(secondRelative)).accountScope ?? '')
        : '';
      if (field !== derived) {
        problems.push(
          `显式传服务端同值时，落盘字段是「${field}」≠ 第一次落盘的「${derived}」——` +
            '协议 §S-15③：落盘字段写**服务端推导值**（不取请求参数）',
        );
      }
    }

    // ③ **显式传不符值** ⇒ 拒（`account_changed`）＋ **缓存文件数不增** ＋ **快照不落**。
    const mismatchId = await adapter.allocateSnapshotId('p3-session');
    // **基线取在分配之后**（分配本身会走一次 `catalog` ⇒ 会写缓存文件）。
    adapter.ensureDir(cacheDir);
    const before = adapter.listDir(cacheDir);
    const mismatch = await commands.capture({
      requestId: 'p3-9-mismatch',
      snapshotId: mismatchId,
      sessionId: 'p3-session',
      accountScope: 'p3-client-mismatch',
    });
    const mismatchCode = String(mismatch?.code ?? mismatch?.error?.code ?? '');
    if (mismatch?.ok === true) {
      problems.push(
        '客户端传了与服务端推导值不符的 accountScope，命令层却**放行了**——' +
          '协议 §S-15④：不符即拒（account_changed），**不静默改写**',
      );
    } else if (mismatchCode !== 'account_changed') {
      problems.push(
        `客户端传了不符的 accountScope，给的码是 ${mismatchCode || '(空)'}——` +
          '协议 §S-15④：按 account_changed 拒',
      );
    }
    if (mismatch?.ok !== true && adapter.hasFinalName(mismatchId)) {
      problems.push(
        '命令层拒了，但**快照却落盘了**——' +
          '协议 §S-15④：不符即拒 ⇒ 不得落盘（更不得静默改写成客户端的值）',
      );
    }
    const after = adapter.listDir(cacheDir);
    const added = after.filter((one) => !before.includes(one));
    if (added.length > 0) {
      problems.push(
        `不符的请求仍**落了缓存文件**（${added.join('、')}）——` +
          '协议 §S-15④：不符即拒 ⇒ 必须在**落盘/落缓存之前**就被拒',
      );
    }

    // ②（另一半）**目录缓存键同理**：不同请求参数 ⇒ 不落到不同缓存文件。
    const catalogBefore = adapter.listDir(cacheDir);
    const catalogA = await commands.catalog({
      requestId: 'p3-9-cache-a',
      accountScope: 'p3-client-a',
      query: 'p3',
    });
    const catalogB = await commands.catalog({
      requestId: 'p3-9-cache-b',
      accountScope: 'p3-client-b',
      query: 'p3',
    });
    const catalogAfter = adapter.listDir(cacheDir);
    const forked = catalogAfter.filter((one) => !catalogBefore.includes(one));
    // **两个不符的请求都应被拒**（§S-15④）⇒ 不该各自落一份缓存。
    // **错误负载是扁平的**（`{ok:false, code, detail, …}`，见 `host/commands.js`
    // 的 `fail()`）⇒ 码在 `result.code`，**不在** `result.error.code`。
    const rejected =
      String(catalogA?.code ?? catalogA?.error?.code ?? '') === 'account_changed' &&
      String(catalogB?.code ?? catalogB?.error?.code ?? '') === 'account_changed';
    if (!rejected) {
      problems.push(
        '目录缓存这一步没有按 account_changed 拒（' +
          `${String(catalogA?.code ?? '放行')} / ${String(catalogB?.code ?? '放行')}）——` +
          '协议 §S-15②③：账户维度由服务端推导；不符即拒',
      );
    }
    if (forked.length > 1) {
      problems.push(
        `两个不同的 request.accountScope 落到了**${forked.length} 个**不同的缓存文件` +
          `（${forked.join('、')}）——协议 §S-15②：目录缓存的键用**服务端推导**的账户维度`,
      );
    }
    if (!ERROR_CODES.account_changed) {
      problems.push('P1 的错误码闭集里没有 account_changed（判据自身前提被破坏）');
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail: p3Why({
          what: '账户维度由服务端推导（协议 §S-15）',
          observed: problems.join('；'),
          clause: '§S-15①③④「accountScope 不得取自请求参数；落盘写服务端推导值；不符即拒（account_changed）且盘上无分叉产物」',
        }),
      };
    }
    return {
      status: PASS,
      detail:
        '省略 accountScope ⇒ 放行且落盘的是服务端推导值；显式传服务端同值 ⇒ 放行且落盘字段相同；' +
        '显式传不符值 ⇒ 拒（account_changed）且缓存文件数不增、快照不落；' +
        '目录缓存那一步同样按 account_changed 拒 ⇒ 不落到不同缓存文件',
    };
  });
}

// ---------------------------------------------------------------------------
// P3-10｜S-13 ＋ G3-1：seq 不复用
// ---------------------------------------------------------------------------
/**
 * 协议 §S-1 / §S-13 裁定 4 的**序号上限**。
 *
 * **为什么由判据写死而不是问实现**：期望值必须**从协议稿转写**
 * ⇒ 实现改常量 ⇒ 判据不改（那是实现的回退，要被这条抓住）。
 */
export const MAX_SEQ = 9999;

/**
 * P3-10｜§S-13 裁定 4 ＋ **G3-1**。
 *
 * ① **被回收的 `seq` 不得再被分配**：认领一个 `seq`（**快照不在**）⇒ 跑回收
 *    （**认领被删**）⇒ **再分配** ⇒ 拿到的 `seq` **≠** 被回收的那个；
 * ② **重启后仍成立**：分配器**重新读盘**（模拟重启）后再分配 ⇒ 仍不复用；
 * ③ **水位优先于认领**：**「先建认领、后写水位」的顺序不得存在**；
 * ④ **上限不绕过**：到 `MAX_SEQ` ⇒ 报**既有的**「序号已用尽」错误。
 *
 * ## ⚠️ 与 P3-6 的联动（本条最容易搞错的一处）
 *
 * P3-6（已入库、已绿）要求**孤儿认领被删** ⇒ **实现不得靠「保留认领文件本体」
 * 来达成不复用**（协议 §S-13 裁定 4 的硬约束①）⇒
 * ⇒ **本条第①项断的是「`seq` 不复用」**，**不是**「认领文件还在」
 * ⇒ 断「认领文件还在」是**断反了**（那是实现被禁止的形态）。
 * ⇒ 所以本条的夹具**主动**依赖「认领被删」这件事发生（回收是场景的一部分），
 * 而**不**把「认领还在」当期望。
 *
 * ## ③ 为什么必须断「次序」而不是「最后结果」
 *
 * §2.5 的写序是**硬要求**（「不得留下『认领已建、水位未写』的窗口」）⇒
 * 只断「最后水位值对不对」会放过「先认领后水位」的实现 ⇒
 * ⇒ 判据用**子进程探针**记录**真实写盘次序**（`writeFileSync` / `appendFileSync` /
 * `renameSync` 的调用序列）⇒ 断「水位写入**早于**认领写入」。
 *
 * ## ④ 怎么把分配器驱动到上限（**不依赖水位文件的形态**）
 *
 * 水位记录的**文件名与格式由实现方定**（§2.5）⇒ 判据**不能**去写它 ⇒
 * 只能从**认领侧**驱动：子进程里把认领目录的清单**补成满 range**，
 * 并让认领写入**撞 EEXIST**（走实现真实处理「这个序号已被占」的分支）⇒
 * 分配器**必须**走到上限 ⇒ 观察它报的错。
 * ⇒ 成本 ≈ 0（本机实测真造 9999 个文件要 **6.3s** ⇒ 不可取）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_10_seqNotReused(adapter) {
  return guard('P3-10-seq-not-reused', async () => {
    const problems = [];
    /** 结论里要**显式说清**「哪一部分没判定」（不许用一句「通过」盖过去）。 */
    const notes = [];

    /**
     * 认领一个 `seq` ⇒ 把它**弄过期** ⇒ 跑一次回收 ⇒ 回收后认领还在不在。
     *
     * @param {string} sessionId - 会话 id。
     * @param {object} allocator - 分配器（①用同一个，②用全新的）。
     * @returns {Promise<{allocated: object, claim: object|null, reclaimed: boolean}>}
     */
    async function claimThenReclaim(sessionId, allocator) {
      const allocated = allocator.reserve(sessionId);
      const claim =
        adapter.claimsOf(sessionId).find((one) => one.seq === allocated.seq) ?? null;
      if (claim === null) return { allocated, claim: null, reclaimed: false };
      // **把它弄过期**（模拟过了保留期）⇒ 回收条件成立（过期 ＋ 快照不在）。
      const raw = JSON.parse(adapter.readFile(claim.path));
      adapter.writeFile(
        claim.path,
        `${JSON.stringify({ ...raw, claimedAt: '2026-01-01T00:00:00.000Z' }, null, 2)}\n`,
      );
      const sweep = await adapter.runSweep({});
      if (sweep?.broken === true) {
        throw new Error(`清理过程的观测失败：${String(sweep.outcome ?? '').slice(0, 120)}`);
      }
      if (sweep?.threw === true) {
        problems.push(`跑回收时抛了异常：${String(sweep.outcome ?? '')}`);
      }
      return { allocated, claim, reclaimed: !adapter.exists(claim.path) };
    }

    // ① **被回收的 `seq` 不得再被分配**。
    const sessionA = 'p3-seq-a';
    const allocator = adapter.buildSeqAllocator();
    let first;
    try {
      first = await claimThenReclaim(sessionA, allocator);
    } catch (error) {
      return {
        status: SKIPPED,
        detail: `造「认领 ⇒ 回收」这个场景失败：${String(error?.message ?? error)} ⇒ 本条无法判定`,
      };
    }
    if (first.claim === null) {
      problems.push(
        `分配后盘上**看不到** seq=${first.allocated.seq} 的认领记录 ⇒ 场景不成立`,
      );
    } else if (!first.reclaimed) {
      problems.push(
        '孤儿认领（有认领、无快照）**没被删** ⇒ 回收没发生 ⇒ 场景不成立' +
          '（这也违反已入库的 P3-6：孤儿认领可被回收）',
      );
    } else {
      let again = null;
      try {
        again = allocator.reserve(sessionA);
      } catch (error) {
        problems.push(`回收后再分配时抛错了：${String(error?.message ?? error)}`);
      }
      if (again !== null && again.seq === first.allocated.seq) {
        problems.push(
          `被回收的 seq（${first.allocated.seq}）**又被分配**了` +
            `（同一个 snapshotId「${first.allocated.snapshotId}」可能指向另一份内容）——` +
            '协议 §S-13 裁定 4①：`seq` 永不复用',
        );
      }
    }

    // ② **重启后仍成立**：换一套**全新的**分配器（一切重新读盘）⇒ 仍不复用。
    // ⇒ **单独用一个会话**（否则 ① 的「再分配」会把那个序号重新占上 ⇒ ② 被掩盖）。
    const sessionB = 'p3-seq-b';
    let restarted = null;
    try {
      const before = await claimThenReclaim(sessionB, adapter.buildSeqAllocator());
      if (before.claim === null) {
        problems.push('② 的夹具不成立：分配后盘上看不到对应的认领记录');
      } else if (!before.reclaimed) {
        problems.push('② 的夹具不成立：孤儿认领没被删 ⇒ 回收没发生');
      } else {
        restarted = adapter.buildSeqAllocator().reserve(sessionB);
        if (restarted.seq === before.allocated.seq) {
          problems.push(
            `重启后（分配器重新读盘）又分配到了被回收的 seq（${before.allocated.seq}）——` +
              '协议 §S-13 裁定 4：水位必须**独立落盘**（§2.5）⇒ 认领被删也不能倒退',
          );
        }
      }
    } catch (error) {
      return {
        status: SKIPPED,
        detail: `造「重启后再分配」这个场景失败：${String(error?.message ?? error)} ⇒ 本条无法判定`,
      };
    }

    // ③ **水位优先于认领**：分配时水位写入必须**早于**认领写入（断**次序**）。
    const traced = await adapter.runReserveWithTrace(sessionA);
    if (traced?.broken === true) {
      notes.push(
        `写序探针失败（${String(traced.outcome ?? '').slice(0, 100)}）⇒ ③ 未判定`,
      );
    } else {
      const trace = Array.isArray(traced.trace) ? traced.trace : [];
      const claimsDir = adapter.claimsDirOf(sessionA);
      const isUnderClaims = (path) => String(path ?? '').startsWith(claimsDir);
      // **认领文件** = 会话目录里「文件名是数字」的那份（`<seq>.json`）；
      // **水位记录** = 会话目录里的**其它**写入（**文件名与格式由实现方定**）。
      const isClaimWrite = (path) =>
        isUnderClaims(path) && /^\d+\.json$/.test(String(path ?? '').split(/[\\/]/).pop() ?? '');
      const claimIndex = trace.findIndex((one) => isClaimWrite(one?.path));
      const watermarkIndex = trace.findIndex(
        (one) => isUnderClaims(one?.path) && !isClaimWrite(one?.path),
      );
      if (claimIndex < 0) {
        problems.push('分配过程里**看不到认领文件的写入** ⇒ 场景不成立');
      } else if (watermarkIndex < 0) {
        problems.push(
          '分配过程里**看不到水位记录的写入**——协议 §2.5：水位必须**独立于认领文件**落盘' +
            '（且**先于**认领）；协议 §S-13 裁定 4②：不得靠「保留认领文件」来达成不复用',
        );
      } else if (watermarkIndex > claimIndex) {
        problems.push(
          '水位写入**晚于**认领写入——协议 §2.5 / §S-13 裁定 4②：' +
            '不得留下「认领已建、水位未写」的窗口（崩在窗口里 ⇒ 该 `seq` 仍会被复用）',
        );
      }
    }

    // ④ **上限不绕过**：到 `MAX_SEQ` ⇒ 报**既有的**「序号已用尽」错误。
    const limit = await adapter.runAllocateAtLimit('p3-seq-limit', MAX_SEQ);
    if (limit?.broken === true) {
      notes.push(
        `上限探针失败（${String(limit.outcomeMessage ?? '').slice(0, 100)}）⇒ ④ 未判定`,
      );
    } else if (limit.threw !== true) {
      problems.push(
        `序号全部用满（${MAX_SEQ}）之后，分配器却**又分配**了 seq ${limit.seq}——` +
          '协议 §S-13 裁定 4④：到上限必须报「序号已用尽」，**不许绕**',
      );
    } else {
      if (limit.outcomeCode !== 'source_unsupported') {
        problems.push(
          `到上限时给的错误码是「${limit.outcomeCode || '(空)'}」——` +
            '协议 §S-13 裁定 4④：报**既有的**「序号已用尽」错误（**不许换形态**）',
        );
      }
      if (!/序号已用尽/.test(String(limit.outcomeMessage ?? ''))) {
        problems.push(
          `到上限时的错误消息是「${String(limit.outcomeMessage ?? '').slice(0, 80)}」——` +
            '应当是既有的「序号已用尽」口径（**不许换形态**）',
        );
      }
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail:
          p3Why({
            what: '`seq` 永不复用（协议 §S-13 裁定 4 ＋ G3-1）',
            observed: problems.join('；'),
            clause:
              '§S-13 裁定 4「被回收的 `seq` 不得再被分配；水位必须先于认领落盘；到 MAX_SEQ 报既有的『序号已用尽』」',
          }) + (notes.length > 0 ? ` ⇒ 另：${notes.join('；')}` : ''),
      };
    }
    return {
      status: PASS,
      detail:
        '被回收的 `seq` 没有再被分配（同一分配器与重启后各验一次）；' +
        '水位写入**早于**认领写入；到上限报的是既有的「序号已用尽」错误' +
        (notes.length > 0 ? `；另：${notes.join('；')}` : ''),
    };
  });
}

// ---------------------------------------------------------------------------
// P3-11｜S-11（R1/R2）：释放路径
// ---------------------------------------------------------------------------

/**
 * 协议 §4 的**占用保留期**（`occupancyRetentionMs` ＝ 7 天）——**判据自己转写**。
 *
 * ## 为什么不取 adapter 传进来的那个值
 *
 * adapter 传进来的 `occupancyRetentionMs` 是**用来配置实现**的 ⇒ 拿它当期望值
 * ⇒ 实现把保留期改错时判据会跟着改 ⇒ **假绿**。⇒ 这里**自己写死 7 天**
 * （协议 §4 的表）⇒ 夹具按它造「过期」的记录。
 */
const P3_11_OCCUPANCY_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * P3-11｜§S-11 R1 ＋ R2（协议 v0.5 §5 判据表新增的那一行）。
 *
 * ① **R1 显式释放**：调用 host 命令 **`release`**（入参 `{ snapshotId }`）释放某个
 *    `snapshotId` ⇒ 它的占用记录**不在**（且**别的**快照的占用记录**不受影响**）；
 * ② **R2 兜底释放**：占用记录**过期**（超 §4 `occupancyRetentionMs`）⇒ 跑一次清理
 *    ⇒ **它被删**（⇒ 该快照随后可被回收）；
 * ③ **R1 不得越界**：释放**别的**快照 ⇒ **不得**误删本快照的占用记录。
 *
 * ## 本条断什么、**不断**什么
 *
 * · 断的是**释放路径本身**（调用释放入口 ⇒ 占用记录真的没了；清理 ⇒ 过期记录真的
 *   被删）；**不断**「内核何时调用释放」——内核契约里**没有**这个信号
 *   （§S-12、§7.1 G1-8）⇒ 判据只**直接调用** host 的 `release` 命令，
 *   **不**去断言「内核移除引用后立刻释放」。
 * · **R3（记录写坏 ⇒ 不删）已由 P3-5④ 覆盖** ⇒ 本条**不重复断**，夹具里也
 *   **不放**写坏的记录。
 * · **不断**释放入口的内部形态（函数名怎么起、删文件的顺序、用不用两步法）
 *   ⇒ 只看**可观测结果**：占用记录还在不在、命令返回什么。
 *
 * ## 幂等与返回形态为什么也断
 *
 * §S-11 R1（v0.5）把释放入口的**协议口径**写死了：入参 `{ snapshotId }`、
 * 成功返回 `{ ok: true, released: boolean }`、**幂等**（该 id 本来就没有占用记录
 * ⇒ 仍成功、`released: false`）⇒ §5 那一行又明写「协议 §S-11 R1」⇒
 * ⇒ 这三条是**协议明写的可观测性质**，不是实现形态 ⇒ 断它们。
 * （「不新增错误码」同理属于 P1 §3 的闭集，**不在本条** ⇒ 本条不断。）
 *
 * ## 红字怎么分层
 *
 * · ① 缺**入口**（命令不存在）⇒ 红，并点明「协议写死了入口名」；
 * · ① 入口在、但**记录没被删** ⇒ 红，并点明是「说了删了没删」还是「自己说没删」；
 * · ② 清理过程本身跑挂了 ⇒ **未判定**（记在 notes 里，**不**当通过、**不**当红）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_11_releasePaths(adapter) {
  return guard('P3-11-release-paths', async () => {
    const problems = [];
    /** 结论里要**显式说清**「哪一部分没判定」。 */
    const notes = [];

    // ── 夹具（协议 §2.4 的占用记录 ＋ §4 的保留期）────────────────────────
    const ID_TARGET = 'ds~p3rel00000001~0001';
    const ID_OTHER = 'ds~p3rel00000001~0002';
    const ID_KEEP = 'ds~p3rel00000001~0003';
    const ID_EXPIRED = 'ds~p3rel00000001~0004';
    // **过期多少**：比 §4 的 7 天再多一天 ⇒ 任何口径都算「过期」。
    const AGE_MS = P3_11_OCCUPANCY_RETENTION_MS + 24 * 60 * 60 * 1000;
    const freshIso = new Date().toISOString();

    /** 写一条占用记录（**只写 §2.4 的四个字段**）。 */
    const putOccupancy = (snapshotId) => {
      adapter.writeOccupancy(snapshotId, {
        snapshotId,
        capturedAt: freshIso,
        createdAt: freshIso,
        sessionId: 'p3-release-session',
      });
    };

    // 三条**未过期**的占用记录（① 的目标 ＋ ① 的「别的」 ＋ ③ 的「不得误删」），
    // 外加一条**过期**的（② 的对象）。
    for (const id of [ID_TARGET, ID_OTHER, ID_KEEP]) {
      adapter.writeSnapshot(id);
      putOccupancy(id);
    }
    adapter.writeSnapshotAged(ID_EXPIRED, AGE_MS);
    adapter.writeOccupancyAged(ID_EXPIRED, AGE_MS);

    // ── ① R1 显式释放 ───────────────────────────────────────────────────
    const first = await adapter.callRelease({ snapshotId: ID_TARGET });
    if (first?.broken === true) {
      // **探针自己跑挂了** ⇒ ① 没法判 ⇒ 记「未判定」，不当通过也不当红。
      notes.push(`释放命令的观测失败（${String(first.errorMessage ?? '').slice(0, 100)}）⇒ ① 未判定`);
    } else if (first?.available !== true) {
      const aliasHint =
        first?.alias === null || first?.alias === undefined
          ? ''
          : `（实现侧只有别名 \`${first.alias}\`，协议写死的名字是 \`release\`）`;
      problems.push(
        `**没有 \`release\` 这个 host 命令**${aliasHint}——协议 §S-11 R1：释放入口 ＝ ` +
          '新增 host 命令 `release`（入参 `{ snapshotId }`）',
      );
    } else {
      if (first.ok !== true) {
        problems.push(
          `释放命令**失败**（${first.errorCode || '无码'}：${String(first.errorMessage ?? '').slice(0, 80)}）——` +
            '协议 §S-11 R1：成功返回 `{ ok: true, released: boolean }`',
        );
      }
      if (typeof first.released !== 'boolean') {
        problems.push(
          '释放命令的返回里**没有 `released` 这个布尔**——协议 §S-11 R1：' +
            '`released` ＝ 这次是否真的删掉了一条',
        );
      }
      if (adapter.occupancyExists(ID_TARGET)) {
        const pretended =
          first.ok === true && first.released === true
            ? '（命令报 `released: true` ⇒ **不许假装删了**）'
            : first.ok === true && first.released === false
              ? '（命令自己报 `released: false` ⇒ 它自己说没删掉）'
              : '';
        problems.push(
          `释放之后，**占用记录还在**${pretended}——协议 §S-11 R1：显式释放 ⇒ 删占用记录`,
        );
      }
      if (!adapter.occupancyExists(ID_OTHER)) {
        problems.push(
          '释放这一个之后，**别的快照的占用记录被删了**——协议 §S-11 R1：' +
            '释放只作用于**被点名的那一条**',
        );
      }
    }

    // ── 幂等（§S-11 R1：**该 id 本来就没有占用记录 ⇒ 仍成功、`released: false`**）──
    // ⇒ 只在「第一次释放确实删掉了」时才有这个场景（否则它不是幂等问题，是①的问题）。
    if (first?.broken !== true && first?.available === true && !adapter.occupancyExists(ID_TARGET)) {
      const again = await adapter.callRelease({ snapshotId: ID_TARGET });
      if (again?.broken === true) {
        notes.push(`重复释放的观测失败（${String(again.errorMessage ?? '').slice(0, 100)}）⇒ 幂等未判定`);
      } else {
        if (again.ok !== true) {
          problems.push(
            `**重复释放同一个 id 居然失败**（${again.errorCode || '无码'}）——协议 §S-11 R1：` +
              '幂等（该 id 本来就没有占用记录 ⇒ 仍成功，`released: false`）',
          );
        }
        if (again.released !== false) {
          problems.push(
            `重复释放同一个 id 时报 \`released: ${JSON.stringify(again.released)}\`——协议 §S-11 R1：` +
              '幂等 ⇒ 该 id 本来就没有占用记录 ⇒ 仍成功且 `released: false`',
          );
        }
      }
    } else {
      notes.push('第一次释放没把记录删掉 ⇒ 「重复释放」这个场景不成立 ⇒ 幂等未判定');
    }

    // ── ③ R1 不得越界 ───────────────────────────────────────────────────
    if (first?.broken === true) {
      notes.push('① 未判定 ⇒ ③（不得越界）依赖同一条释放路径 ⇒ 未判定');
    } else if (first?.available !== true) {
      notes.push('没有 `release` 入口 ⇒ ③（不得越界）无法判定');
    } else {
      const cross = await adapter.callRelease({ snapshotId: ID_OTHER });
      if (cross?.broken === true) {
        notes.push(`释放别的快照时的观测失败（${String(cross.errorMessage ?? '').slice(0, 100)}）⇒ ③ 未判定`);
      } else if (!adapter.occupancyExists(ID_KEEP)) {
        problems.push(
          '释放**别的**快照之后，**没被点名的那条的占用记录被误删了**——协议 §S-11 R1：' +
            '释放只作用于**被点名的那一条**',
        );
      }
    }

    // ── ② R2 兜底释放 ───────────────────────────────────────────────────
    const sweep = await adapter.runSweep({});
    if (sweep?.broken === true) {
      notes.push(
        `清理过程的观测失败（${String(sweep.outcome ?? '').slice(0, 100)}）⇒ ② 未判定`,
      );
    } else {
      if (adapter.occupancyExists(ID_EXPIRED)) {
        problems.push(
          '**过期的占用记录没被删**——协议 §S-11 R2：占用记录超过占用保留期' +
            '（§4 `occupancyRetentionMs` ＝ 7 天）⇒ 视为失效并删（**由清理流程负责**）',
        );
      }
      if (!adapter.occupancyExists(ID_KEEP)) {
        problems.push(
          '清理之后，**没过期的占用记录被删了**——协议 §S-11 R2：只有**过期**的占用记录' +
            '才被兜底释放',
        );
      }
      // **⇒ 该快照随后可被回收**：占用关系没了之后，过期的快照就能被回收了。
      // ⇒ 跑**第二次**清理再断言（不依赖「同一轮里删」还是「下一轮删」的实现次序）。
      const sweep2 = await adapter.runSweep({});
      if (sweep2?.broken === true) {
        notes.push(
          `第二次清理的观测失败（${String(sweep2.outcome ?? '').slice(0, 100)}）⇒ 「随后可被回收」未判定`,
        );
      } else if (adapter.hasFinalName(ID_EXPIRED)) {
        problems.push(
          '占用记录释放之后，**过期快照仍然在**——协议 §S-11 R2：⇒ 该快照随后可被回收',
        );
      }
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail:
          p3Why({
            what: '释放路径（协议 §S-11 R1 显式释放 ＋ R2 兜底释放）',
            observed: problems.join('；'),
            clause:
              '§S-11 R1「释放入口 ＝ host 命令 `release`（入参 `{ snapshotId }`；成功返回 ' +
              '`{ ok: true, released: boolean }`；幂等）；释放 ⇒ 删占用记录；只作用于被点名的那一条」' +
              '＋ §S-11 R2「占用记录超过 §4 `occupancyRetentionMs`（7 天）⇒ 视为失效并删」',
          }) + (notes.length > 0 ? ` ⇒ 另：${notes.join('；')}` : ''),
      };
    }
    return {
      status: PASS,
      detail:
        '`release` 释放后占用记录**不在**、**别的**快照的占用记录**不受影响**；' +
        '重复释放仍成功且 `released: false`（幂等）；释放别的快照**不会**误删本条；' +
        '**过期**的占用记录跑一次清理**被删**（**未过期**的仍在）⇒ 该快照随后可被回收' +
        (notes.length > 0 ? `；另：${notes.join('；')}` : ''),
    };
  });
}

// ---------------------------------------------------------------------------
// P3-12｜S-11（R2 的「本轮 / 下一轮」口径）
// ---------------------------------------------------------------------------

/**
 * 协议 §4 的**占用保留期**（`occupancyRetentionMs` ＝ 7 天）——**判据自己转写**。
 *
 * ## 为什么单独写一份（不与 P3-11 共用常量）
 *
 * P3-11 那份常量在**已验收入库**的代码里 ⇒ 改它就是动已验收的判据 ⇒
 * ⇒ 这里单独写一份（**同一个协议值**）⇒ P3-1…P3-11 一个字不动。
 */
const P3_12_OCCUPANCY_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * P3-12｜§S-11 R2 的**补充条款**（协议 v0.6 写死、v0.8 钉成判据的那条保守口径）。
 *
 * ① **本轮不删**：占用记录**自己也过期**（超 §4 `occupancyRetentionMs`）**且**它占着的
 *    快照也过期 ⇒ 跑**一次**清理 ⇒ **那份占用记录被删**（R2 释放当轮即可断言）
 *    **且那份快照仍在**（本轮按「**本轮开始时**」的占用关系判）；
 * ② **下一轮起可回收**：**再跑一次**清理 ⇒ **那份快照被删**。
 *
 * ## 为什么断的是「**两轮之间的差别**」而不是「最后状态」
 *
 * 只断「最后快照没了」⇒ 一个「**同轮就删**」的实现（这一轮既释放占用、又顺手删掉
 * 刚被释放的那份快照）也能通过 ⇒ 那正是协议 §S-11③「存在 ⇒ 不删」被架空的情形
 * （判据 P3-5① 会红）⇒ ⇒ 本条**必须**断「**本轮仍在**」这一半。
 *
 * ## 夹具要求（两条，写错就假红）
 *
 * · **快照与占用记录都要过期** ⇒ 快照的过期判定看**文件内的 `capturedAt`**
 *   （mtime 只作兜底）⇒ **光改 mtime 不算过期**；本条把两个时间信号都调旧。
 * · **两轮之间不重造夹具** ⇒ 要断的是**同一份快照**在两轮里的差别 ⇒
 *   两次清理跑在**同一个落盘根**上（中间只清掉该清的，不补新的夹具）。
 *
 * ## 不断什么
 *
 * · **R1 仍由 P3-11① 覆盖**、**R3 仍由 P3-5④ 覆盖** ⇒ 本条**不重复断**。
 * · **不断**「内核何时调用释放」（内核契约里没有这个信号，§S-12、§7.1 G1-8）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_12_twoRoundRelease(adapter) {
  return guard('P3-12-two-round-release', async () => {
    const problems = [];
    /** 结论里要**显式说清**「哪一部分没判定」。 */
    const notes = [];

    // ── 夹具：快照与占用记录**都过期**（比 §4 的 7 天再多一天）────────────
    const ID = 'ds~p3rel00000002~0001';
    const AGE_MS = P3_12_OCCUPANCY_RETENTION_MS + 24 * 60 * 60 * 1000;
    adapter.writeSnapshotAged(ID, AGE_MS);
    adapter.writeOccupancyAged(ID, AGE_MS);

    // ── ① 本轮：占用记录被删（R2 释放当轮可断言）＋ 那份快照**仍在** ──────
    const first = await adapter.runSweep({});
    if (first?.broken === true) {
      return {
        status: SKIPPED,
        detail: `第一轮清理的观测失败（${String(first.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    if (adapter.occupancyExists(ID)) {
      problems.push(
        '跑完**一轮**清理，**那份过期占用记录还在**——协议 §S-11 R2：占用记录超过占用保留期' +
          '（§4 `occupancyRetentionMs` ＝ 7 天）⇒ 视为失效并删；§S-11 R2 补充条款：' +
          '**R2 释放（占用记录被删）当轮即可断言**',
      );
    }
    if (!adapter.hasFinalName(ID)) {
      problems.push(
        '**本轮**就把那份过期快照删了——协议 §S-11 R2 补充条款：本轮的回收按' +
          '**「本轮开始时」的占用关系**判 ⇒ 那份刚被释放的快照**本轮仍不删**，' +
          '**从下一轮起**才可被回收',
      );
    }

    // ── ② 下一轮起可回收：**再跑一次**清理（同一份夹具，不重造）──────────
    const second = await adapter.runSweep({});
    if (second?.broken === true) {
      notes.push(
        `第二轮清理的观测失败（${String(second.outcome ?? '').slice(0, 100)}）⇒ ② 未判定`,
      );
    } else if (adapter.hasFinalName(ID)) {
      problems.push(
        '**再跑一次**清理之后，那份过期快照**仍在**——协议 §S-11 R2 补充条款：' +
          '**从下一轮起**才可被回收',
      );
    }

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail:
          p3Why({
            what: 'R2 释放的「本轮 / 下一轮」口径（协议 §S-11 R2 补充条款）',
            observed: problems.join('；'),
            clause:
              '§S-11 R2「占用记录超过 §4 `occupancyRetentionMs`（7 天）⇒ 视为失效并删（当轮即可断言）」' +
              '＋ 补充条款「本轮按『本轮开始时』的占用关系判 ⇒ 那份快照**本轮不删**；**从下一轮起**才可被回收」',
          }) + (notes.length > 0 ? ` ⇒ 另：${notes.join('；')}` : ''),
      };
    }
    return {
      status: PASS,
      detail:
        '第一轮清理：**过期占用记录被删**（R2 释放当轮即可断言）**且那份快照仍在**' +
        '（本轮按「本轮开始时」的占用关系判）；第二轮清理：那份过期快照**被删**' +
        '（从下一轮起可被回收）' + (notes.length > 0 ? `；另：${notes.join('；')}` : ''),
    };
  });
}

// ---------------------------------------------------------------------------
// P3-13｜§S-16 缓存总量核算 ＋ 额外淘汰
// ---------------------------------------------------------------------------

/**
 * §4 的**缓存总量上限**（512 MB / 实例）——**判据自己写死**。
 *
 * ## 为什么不从实现取这个数
 *
 * 判据的期望值**独立算**（不 import 实现当标准答案）⇒ 预算值在这里**逐字转写**
 * 协议 §4 ⇒ 实现把常量改错时，判据不会跟着改。
 */
export const P3_13_CACHE_BUDGET_BYTES = 512 * 1024 * 1024;

/**
 * 目录缓存 TTL（5 分钟）—— §S-16 裁定 3①「**尚未超过 TTL（5 分钟）**的目录缓存也删」。
 *
 * ## 它只是判据的**口径说明**
 *
 * 判据**不**去断「实现的 TTL 常量是不是 5 分钟」（那是实现内部参数）⇒
 * 这里写它是为了让「**尚未超过 TTL（5 分钟）**」这个说法有**可复核的出处**（协议 §S-16 裁定 3①）。
 */
const P3_13_CACHE_TTL_MS = 5 * 60 * 1000;

/**
 * 把一段文本里的**数字**抽出来（千分位逗号先去掉）。
 *
 * ## 为什么抽数字而不是「含某串」
 *
 * 「日志里含某个字符串」当唯一断言 ⇒ **子串包含会造成假绿**（措辞一改就漏判），
 * 而且措辞本身是实现自由 ⇒ 判据只认**数值相等**：留痕里必须出现「核算总量」
 * 「预算值」「淘汰后总量」这三个**数**。
 *
 * @param {string} text
 * @returns {number[]}
 */
function numbersIn(text) {
  return (String(text ?? '')
    .match(/\d[\d,]*|\d/g) ?? [])
    .map((token) => Number(token.replace(/,/g, '')))
    .filter((value) => Number.isFinite(value));
}

/**
 * 判据自己算的**核算总量**（协议 §S-16 裁定 1）。
 *
 * - **计入**：`snapshots/**`（**逐 provider 目录**求和）＋ `catalog-cache/**`（**平铺**）；
 * - **不计入**：`seq-claims/`、`refs/`、`logs/`、`sweeps/`。
 *
 * ## 为什么自己算
 *
 * 「数字对」**不等于**「扫的是这几个文件」⇒ 期望值必须由判据按协议口径**自己加总**
 * （适配器只给「盘上事实」：每个文件的路径与字节数）⇒ 实现多算一个目录、少算一个
 * provider 目录，数字就**对不上** ⇒ 判据立刻红。
 *
 * @param {object} adapter
 * @returns {number} 字节数。
 */
function countedTotalBytes(adapter) {
  let total = 0;
  for (const dir of ['snapshots', 'catalog-cache']) {
    for (const file of adapter.listFiles(dir)) total += Number(file?.size ?? 0);
  }
  return total;
}

/**
 * 实现**自报的核算读数**（读不到就是 null —— 如实说「读不到」）。
 *
 * ## 为什么优先读「实现自报的读数」而不是留痕
 *
 * 留痕**只在超预算时才有** ⇒ 非超预算的场景（核算范围那一支）读不到留痕
 * ⇒ 这里读 `sweep()` 结果里的核算读数；**读不到就如实报 null**（那一半记「未判定」，
 * **不当通过、不当红**）。
 *
 * @param {object} outcome - `sweep()` 的返回值。
 * @returns {{value: number|null, source: string}}
 */
function readBudgetReading(outcome) {
  const value = Number(outcome?.budget?.totalBytes);
  if (Number.isFinite(value) && value >= 0) {
    return { value, source: 'sweep 结果里的核算读数' };
  }
  return { value: null, source: '' };
}

/**
 * P3-13｜§S-16 的**缓存总量核算 ＋ 额外淘汰**。
 *
 * ① **核算范围**：计入快照（**逐 provider 目录**求和）＋目录缓存（**平铺**）；
 *    **不计入** `seq-claims/`、`refs/`、`logs/`、`sweeps/`
 *    （造一份**只存在于被排除目录**的文件 ⇒ **总量读数不变**）。
 * ② **超预算 ⇒ 触发额外淘汰**：把总量做到超预算 ⇒ 跑清理 ⇒
 *    **尚未超过 TTL（5 分钟）的目录缓存被删**（额外淘汰生效）且**总量回落**。
 * ③ **被占用的不删**：造一份「**未到期 ＋ 有占用记录**」的快照并把总量做到超预算
 *    ⇒ 跑清理 ⇒ **它还在**（不为腾空间绕开 §S-11）。
 * ④ **未到期的不删**：造一份「**未到期 ＋ 无占用记录**」的快照并把总量做到超预算
 *    ⇒ 跑清理 ⇒ **它还在**（**不得**按最旧删未到期快照 ⇒ P1 §4.1 第 3 条）。
 * ⑤ **删到底仍超 ⇒ 有留痕**：⇒ **不抛异常、整轮继续**，且**有留痕**
 *    ⇒ **不得**声称总量已被强制在上限内（缺口 **G3-5**）。
 *
 * ## 怎么把总量「做到超预算」
 *
 * 预算是**协议 §4 的常量**（512 MB）、实现里**没有可注入的口** ⇒ 判据不能
 * 「把预算调小」来造超预算 ⇒ 只能**真的把盘上字节顶上去**（`adapter.writeBigFile`）。
 * ⇒ 那个大文件**放在清理流程不会读它内容**的位置（快照目录下**非 `.json`** 的文件、
 * 目录缓存里只被 `stat` 的文件）⇒ 没有人把 512 MB 读进内存。
 *
 * ## 四个场景与五支断言的对应
 *
 * · **场景 A**（读数侧）⇒ ① 的「计入什么／不计入什么」＋「被排除目录的文件 ⇒ 读数不变」；
 * · **场景 B** ⇒ ②（尚未超过 TTL（5 分钟）的目录缓存被删 ＋ 总量回落）；
 * · **场景 C**（行为侧，逐个被排除目录）⇒ ① 的「被排除目录**不该**触发额外淘汰」；
 * · **场景 D** ⇒ ③ ＋ ④ ＋ ⑤（超预算的来源是**另一个 provider 目录**下的文件 ⇒
 *   「为腾空间删快照」这条错路一旦被走，③④ 立刻红）。
 *
 * ## 不断什么
 *
 * · **不断**「核算用什么 API 实现」（`readdirSync`＋`statSync` 或等价物均可）——
 *   协议 §5 的 P3-13 末尾明写；
 * · **不断**「最旧」怎么判（§S-16 裁定 7：本阶段**无落点**）；
 * · **不断**「删到底仍超」时用户可见什么（G3-5：只记日志，不扩 14 码闭集）——
 *   本条只断「**不抛异常 ＋ 有留痕 ＋ 不得声称已在上限内**」。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_13_cacheBudget(adapter) {
  return guard('P3-13-cache-budget', async () => {
    const problems = [];
    /** 结论里要**显式说清**「哪一部分没判定」。 */
    const notes = [];
    /** 造「超预算」用的大文件长度（比预算多 1 字节 ⇒ 一定超）。 */
    const bigBytes = P3_13_CACHE_BUDGET_BYTES + 1;
    const nowIso = new Date().toISOString();

    /** 判据独立算的核算总量。 */
    const total = () => countedTotalBytes(adapter);
    /** 删掉判据自己造的夹具（**只删自己造的**；实现已经删掉的就算了）。 */
    const drop = (relativePath) => {
      try {
        adapter.removeAbsolute(adapter.pathOf(relativePath));
      } catch {
        /* 已经被实现删掉了 */
      }
    };
    /** 造一条**未过期**的占用记录（这一轮它占着那份快照）。 */
    const hold = (snapshotId) =>
      adapter.writeOccupancy(snapshotId, {
        snapshotId,
        capturedAt: nowIso,
        createdAt: nowIso,
        sessionId: 'p3-session',
      });

    // ── 场景 A｜① 核算范围（**读数侧**）──────────────────────────────────────
    // 夹具：**两个 provider 目录**的快照（逐 provider 目录求和）＋**平铺**的目录缓存
    //       ＋**四个被排除目录**各一份文件。
    const ID_DS = 'ds~p3budg00000001~0001';
    const ID_ZZ = 'zz~p3budg00000002~0001';
    const ID_EXCLUDED = 'ds~p3budg00000003~0001';
    adapter.writeSnapshot(ID_DS, { capturedAt: nowIso });
    adapter.writeFile(
      join('snapshots', 'zz', `${ID_ZZ}.json`),
      `${JSON.stringify({ schemaVersion: 1, snapshotId: ID_ZZ, capturedAt: nowIso }, null, 2)}\n`,
    );
    adapter.writeFile(join('catalog-cache', 'ds__p3scope__q.json'), '{"items":[]}');
    hold(ID_EXCLUDED);
    adapter.writeFile(join('seq-claims', 'p3-excluded.dat'), 'x');
    adapter.writeFile(join('logs', 'p3-excluded.dat'), 'x');
    adapter.writeFile(join('sweeps', 'p3-excluded.dat'), 'x');

    const sweepA = await adapter.runSweep({});
    if (sweepA?.broken === true) {
      return {
        status: SKIPPED,
        detail: `第一轮清理的观测失败（${String(sweepA.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    const readingA = readBudgetReading(sweepA?.outcome);
    const expectedA = total();
    if (readingA.value === null) {
      notes.push(
        '① 核算范围（读数侧）：实现没有暴露核算读数 ⇒ 这一半**未判定**（不判通过、不判红）',
      );
    } else if (readingA.value !== expectedA) {
      problems.push(
        `【核算范围】总量读数 ${readingA.value} 字节 ≠ 判据独立算的 ${expectedA} 字节` +
          '（计入：snapshots/**（**逐 provider 目录**求和）＋ catalog-cache/**（**平铺**）；' +
          '不计入：seq-claims/、refs/、logs/、sweeps/）',
      );
    }

    // ① 的第二半：**只存在于被排除目录**的文件 ⇒ **总量读数不变**。
    adapter.writeFile(join('logs', 'p3-excluded-2.dat'), 'yy');
    const sweepA2 = await adapter.runSweep({});
    const readingB = readBudgetReading(sweepA2?.outcome);
    if (readingA.value !== null && readingB.value !== null && readingB.value !== readingA.value) {
      problems.push(
        `【核算范围】只存在于被排除目录（logs/）的文件让总量读数从 ${readingA.value} 变成 ` +
          `${readingB.value} 字节 ⇒ 被排除目录被算进总量了`,
      );
    }
    // 场景 A 的夹具收尾。
    drop(join('snapshots', adapter.providerId, `${ID_DS}.json`));
    drop(join('snapshots', 'zz', `${ID_ZZ}.json`));
    drop(join('catalog-cache', 'ds__p3scope__q.json'));
    drop(join('refs', `${ID_EXCLUDED}.json`));
    drop(join('seq-claims', 'p3-excluded.dat'));
    drop(join('logs', 'p3-excluded.dat'));
    drop(join('logs', 'p3-excluded-2.dat'));
    drop(join('sweeps', 'p3-excluded.dat'));

    // ── 场景 B｜② 超预算 ⇒ 触发额外淘汰 ─────────────────────────────────────
    // **尚未超过 TTL（5 分钟）**的目录缓存：常规清理只删 TTL **已过**的 ⇒ 它的消失**只能**来自额外淘汰。
    const BIG_CACHE = join('catalog-cache', 'ds__p3big__q.json');
    adapter.writeBigFile(BIG_CACHE, bigBytes);
    const beforeB = total();
    const sweepB = await adapter.runSweep({});
    if (sweepB?.broken === true) {
      notes.push(
        `第二轮清理的观测失败（${String(sweepB.outcome ?? '').slice(0, 100)}）⇒ ② 未判定`,
      );
    } else {
      if (adapter.exists(BIG_CACHE)) {
        problems.push(
          '【额外淘汰】超预算之后，**尚未超过 TTL（5 分钟）**的目录缓存还在 —— 协议 §S-16 裁定 3①：' +
            '超预算时**尚未超过 TTL（5 分钟）的目录缓存也要删**（常规清理只删 TTL 已过的 ⇒ ' +
            '它的消失只能来自额外淘汰）',
        );
      }
      const afterB = total();
      if (!(afterB < beforeB)) {
        problems.push(`【额外淘汰】总量没有回落（${beforeB} → ${afterB} 字节）`);
      }
    }
    drop(BIG_CACHE);

    // ── 场景 C｜① 不计入被排除目录（**行为侧**，逐个目录）────────────────────
    // 把一份**超预算**的大文件放进被排除目录 ⇒ **不该**触发额外淘汰
    // ⇒ 那份**尚未超过 TTL（5 分钟）**的目录缓存必须**还在**。
    for (const dir of ['seq-claims', 'refs', 'logs', 'sweeps']) {
      const big = join(dir, 'p3-budget-big.dat');
      const probe = join('catalog-cache', 'p3-budget-probe.json');
      adapter.writeBigFile(big, bigBytes);
      adapter.writeFile(probe, '{"probe":1}');
      const sweepC = await adapter.runSweep({});
      if (sweepC?.broken === true) {
        notes.push(`清理（${dir}/ 夹具）的观测失败 ⇒ ① 的这一处**未判定**`);
      } else if (!adapter.exists(probe)) {
        problems.push(
          `【核算范围】把 ${dir}/ 里的 ${bigBytes} 字节算进了总量 ⇒ 触发了额外淘汰` +
            `（**尚未超过 TTL（5 分钟）**的目录缓存被删）—— 协议 §S-16 裁定 1：**${dir}/ 不计入**总量`,
        );
      }
      drop(big);
      drop(probe);
    }

    // ── 场景 D｜③ 被占用不删 ＋ ④ 未到期不删 ＋ ⑤ 留痕 ──────────────────────
    // 超预算的来源是**另一个 provider 目录**下的文件（计入、且没有任何清理步骤会删它）
    // ⇒ 「为腾空间删快照」这条错路一旦被走，③④ 立刻红。
    const BULK = join('snapshots', 'zz', 'p3-budget-bulk.dat');
    const ID_HELD = 'ds~p3budg00000004~0001';
    const ID_FREE = 'ds~p3budg00000005~0001';
    const FRESH_CACHE = join('catalog-cache', 'p3-budget-fresh.json');
    adapter.writeBigFile(BULK, bigBytes);
    adapter.writeSnapshot(ID_HELD, { capturedAt: nowIso });
    hold(ID_HELD);
    adapter.writeSnapshot(ID_FREE, { capturedAt: nowIso });
    adapter.writeFile(FRESH_CACHE, '{"fresh":1}');
    const beforeD = total();
    const sweepD = await adapter.runSweep({});
    if (sweepD?.broken === true) {
      notes.push(
        `最后一轮清理的观测失败（${String(sweepD.outcome ?? '').slice(0, 100)}）⇒ ③④⑤ **未判定**`,
      );
    } else {
      if (sweepD.threw === true) {
        problems.push(
          '【留痕】整轮清理抛了异常 —— 协议 §S-16 裁定 5：删到底仍超 ⇒ **只记日志**，' +
            '不抛异常、不阻塞整轮',
        );
      }
      if (!adapter.hasFinalName(ID_HELD)) {
        problems.push(
          '【被占用不删】**被占用的**未到期快照被删了 —— 协议 §S-11 裁定 3 ＋ §S-16 裁定 4：' +
            '被占用的快照**任何情况下都不删**（不得为腾空间绕开它）',
        );
      }
      if (!adapter.hasFinalName(ID_FREE)) {
        problems.push(
          '【未到期不删】**未到期**（未满 7 天）的快照被删了 —— P1 §4.1 第 3 条：只删超过 7 天的；' +
            '协议 §S-16 裁定 3③：未到保留期的快照不删（**不得**按最旧删未到期快照）',
        );
      }
      if (adapter.exists(FRESH_CACHE)) {
        problems.push(
          '【额外淘汰】超预算之后**尚未超过 TTL（5 分钟）**的目录缓存还在 ⇒ 额外淘汰没触发（§S-16 裁定 3①）',
        );
      }
      // ⑤ **留痕**：五要素里**可数值校验**的三项（核算总量 / 预算值 / 淘汰后总量）。
      //    期望值优先用**实现自报的数**（它自己记的数必须与它自己留的痕一致）
      //    ⇒ 「核算范围」这一支的偏差不会连累「留痕」这一支。
      const afterD = total();
      const logsD = (sweepD.logs ?? []).join('\n');
      const ownBefore = Number(sweepD?.outcome?.budget?.totalBytes);
      const ownAfter = Number(sweepD?.outcome?.budget?.totalBytesAfterEviction);
      const traceBefore = Number.isFinite(ownBefore) ? ownBefore : beforeD;
      const traceAfter = Number.isFinite(ownAfter) ? ownAfter : afterD;
      const numbers = numbersIn(logsD);
      const missing = [traceBefore, P3_13_CACHE_BUDGET_BYTES, traceAfter].filter(
        (value) => !numbers.includes(value),
      );
      if (missing.length > 0) {
        problems.push(
          '【留痕】没有留下可读的「这次超预算」留痕（协议 §S-16 裁定 5：至少记下核算总量、预算值、' +
            `是否超预算、淘汰了什么、淘汰后总量）—— 留痕里缺这些数字：${missing.join('、')}` +
            `（留痕里的数字：${numbers.join('、') || '（无）'}）`,
        );
      } else if (
        traceAfter > P3_13_CACHE_BUDGET_BYTES &&
        /已(?:回到|在|被强制在)(?:预算|上限)内/.test(logsD)
      ) {
        problems.push(
          `【留痕】删到底仍超预算（淘汰后总量 ${traceAfter} > 预算 ${P3_13_CACHE_BUDGET_BYTES} 字节），` +
            '留痕却声称已在上限内 —— 协议 §S-16 裁定 5：512 MB 是「**尽力而为**的上限」，' +
            '**不得**声称总量已被强制在上限内（缺口 G3-5）',
        );
      }
    }
    drop(BULK);
    drop(join('snapshots', adapter.providerId, `${ID_HELD}.json`));
    drop(join('snapshots', adapter.providerId, `${ID_FREE}.json`));
    drop(join('refs', `${ID_HELD}.json`));
    drop(FRESH_CACHE);

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail:
          p3Why({
            what: '缓存总量的核算 ＋ 超预算时的额外淘汰（协议 §S-16）',
            observed: problems.join('；'),
            clause:
              '§S-16 裁定 1（核算范围：计入 snapshots/**（逐 provider 目录）＋ catalog-cache/**（平铺）；' +
              '不计入 seq-claims/、refs/、logs/、sweeps/）＋ 裁定 3（超预算 ⇒ 尚未超过 TTL（5 分钟）的目录缓存也删；' +
              '被占用的不删；未到期的不删）＋ 裁定 4（被占用的任何情况下都不删）' +
              '＋ 裁定 5（删到底仍超 ⇒ 只记日志 ＋ 可读留痕；不得声称总量已被强制在上限内）',
          }) + (notes.length > 0 ? ` ⇒ 另：${notes.join('；')}` : ''),
      };
    }
    return {
      status: PASS,
      detail:
        '① 核算范围：总量读数与判据独立算的一致（计入 snapshots/** ＋ catalog-cache/**；' +
        '被排除目录里的文件不改变读数；被排除目录里放大文件也不触发额外淘汰）；' +
        '② 超预算 ⇒ **尚未超过 TTL（5 分钟）的目录缓存被删**（额外淘汰生效）且总量回落；' +
        '③ **被占用的**未到期快照还在；④ **未到期**的快照还在；' +
        '⑤ 删到底仍超 ⇒ 整轮没抛异常 ＋ 有可读留痕（核算总量/预算值/淘汰后总量对得上）' +
        (notes.length > 0 ? `；另：${notes.join('；')}` : ''),
    };
  });
}

// ---------------------------------------------------------------------------
// P3-14｜§S-16 裁定 2 的核算时机
// ---------------------------------------------------------------------------

/**
 * P3-14 的夹具大小：一份「**会被删掉**」的目录缓存的字节数（**100 MB**）。
 *
 * ## 为什么是 100 MB
 *
 * 「核算读的是清理**前**还是清理**后**」的可观测差 ＝ 这份文件的字节数 ⇒
 * 差值必须**大到无法被当成舍入误差** ⇒ 100 MB。
 *
 * ## 为什么造它几乎不花钱
 *
 * · `writeBigFile` 只撑长度、不写内容（`truncateSync`）⇒ 本机实测 ≈ 9ms/次；
 * · 常规清理删目录缓存时**只 `stat`、不读内容**（`host/sweep.js` 的 `sweepCache`）
 *   ⇒ 没有人把 100 MB 读进内存。
 */
const P3_14_FIXTURE_BYTES = 100 * 1024 * 1024;

/**
 * 读实现自报的**核算总量**（读不到就是 null —— 如实说「读不到」）。
 *
 * ## 两条来源
 *
 * ① **主**：`sweep()` 结果里的核算读数；
 * ② **兜底**：超预算时**留痕**里的**最大数字** —— 核算总量 > 预算 ≥ 淘汰后总量
 *    ⇒ 留痕里最大的那个数就是核算总量（**不依赖措辞**）。
 *
 * ⚠️ 这只是**观测值**；期望值由判据**自己**加总（见 `countedTotalBytes`）
 * ⇒ **不自比**（自比 ⇒ 假绿）。
 *
 * @param {object} sweep - `runSweep()` 的结果。
 * @returns {{value: number|null, source: string}}
 */
function readAccountedTotal(sweep) {
  const direct = readBudgetReading(sweep?.outcome);
  if (direct.value !== null) return direct;
  const numbers = numbersIn((sweep?.logs ?? []).join('\n'));
  if (numbers.length === 0) return { value: null, source: '' };
  return { value: Math.max(...numbers), source: '留痕里的最大数字（＝核算总量）' };
}

/**
 * P3-14｜§S-16 裁定 2 的**核算时机**。
 *
 * 裁定 2 把核算排在**两个位置之间**：
 * 「**占用关系全量扫描 ＋ 常规清理**」**之后**、「**额外淘汰**」**之前**
 * ⇒ 核算读到的应是「**本轮该回收的都回收了之后**」的总量（不是清理前的旧数）。
 *
 * ## 两支断言（两个场景，各差一个「会被删掉的字节数」）
 *
 * ① **核算在常规清理之后**（场景 A）：造一份 **TTL 已过**（**常规清理**会删）的
 *    目录缓存 ⇒ 实现自报的**核算总量**必须 == 判据**独立算**的「清理后总量」
 *    （**不含**那份已被回收的缓存）⇒ 若核算排在常规清理**之前**，实现自报的数
 *    会**多出**那 100 MB ⇒ 不等 ⇒ 红。
 * ② **核算在额外淘汰之前**（场景 B）：造一份 **TTL 内**（**只有额外淘汰**会删）的
 *    目录缓存 ⇒ 实现自报的**核算总量**必须 == 判据**独立算**的
 *    「清理后、**淘汰前**」总量（**含**那份尚未被淘汰的缓存）⇒ 若核算排在额外淘汰
 *    **之后**，实现自报的数会**少掉**那 100 MB ⇒ 不等 ⇒ 红。
 *
 * ## 期望值必须**独立算**（本条最容易被写坏的地方）
 *
 * 期望值 ＝ 判据**自己**加总的盘上字节（计入 `snapshots/**` ＋ `catalog-cache/**`）
 * ⇒ **不复述实现自报的数** ⇒ 否则是「自己跟自己比」⇒ 假绿。
 * ⇒ 实现自报的数只当**观测值**用；读不到就如实记「未判定」（不当通过、不当红）。
 *
 * ## 场景成立的护栏（缺了就是空跑）
 *
 * · 场景 A：那份 **TTL 已过**的缓存必须**真的被回收** ⇒ 否则「核算时机」无从观测
 *   ⇒ 报 **skipped**（场景不成立），**不**报红、**不**报通过；
 * · 场景 B：额外淘汰若**没删**那份 TTL 内的缓存 ⇒ 期望值就退化成「清理后总量」
 *   ⇒ 判据据此自动兼容（不冤枉实现）。
 *
 * ## 不断什么
 *
 * · **不断**核算用什么 API 实现（协议 §5 P3-13 末尾同款纪律）；
 * · **不断**超预算后删什么（那是 P3-13②③④⑤）；
 * · **不断**「最旧」怎么判（裁定 7 已定不启用）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_14_accountTiming(adapter) {
  return guard('P3-14-account-timing', async () => {
    const problems = [];
    /** 结论里要**显式说清**「哪一部分没判定」。 */
    const notes = [];

    /** 删掉判据自己造的夹具（**只删自己造的**；实现已经删掉的就算了）。 */
    const drop = (relativePath) => {
      try {
        adapter.removeAbsolute(adapter.pathOf(relativePath));
      } catch {
        /* 已经被实现删掉了 */
      }
    };
    /**
     * 造「超预算」的大文件：放在**另一个 provider 目录**下的**非 `.json`** 文件
     * ⇒ **计入**总量（逐 provider 目录求和）＋**没有任何清理步骤会删它**
     * ⇒ 「总量超预算」这个前提在整个场景里**始终成立**（清理后也超）。
     */
    const makeBulk = (name) => {
      const path = join('snapshots', 'zz', name);
      adapter.writeBigFile(path, P3_13_CACHE_BUDGET_BYTES + 1);
      return path;
    };

    // ── 场景 A｜① 核算在**常规清理之后** ────────────────────────────────────
    const expiredCache = join('catalog-cache', 'p3-account-expired.json');
    adapter.writeBigFile(expiredCache, P3_14_FIXTURE_BYTES);
    // **TTL 已过**（mtime 调旧 1 小时 > 目录缓存 TTL 5 分钟）⇒ 常规清理会删它。
    adapter.setMtimeOld(expiredCache, 60 * 60 * 1000);
    const bulkA = makeBulk('p3-account-bulk-a.dat');

    const sweepA = await adapter.runSweep({});
    if (sweepA?.broken === true) {
      return {
        status: SKIPPED,
        detail: `场景 A 的观测失败（${String(sweepA.outcome ?? '').slice(0, 120)}）⇒ 本条无法判定`,
      };
    }
    // **场景成立的护栏**：那份 TTL 已过的缓存必须**真的被回收**。
    if (adapter.exists(expiredCache)) {
      drop(bulkA);
      return {
        status: SKIPPED,
        detail:
          '那份 **TTL 已过**的目录缓存跑完清理**还在** ⇒ 常规清理没回收它 ⇒ ' +
          '「核算时机」这条的场景**不成立**（不判通过、也不判红）',
      };
    }
    const observedA = readAccountedTotal(sweepA);
    if (observedA.value === null) {
      notes.push('① 核算在常规清理之后：读不到实现的核算总量 ⇒ 这一半**未判定**');
    } else {
      // **期望值：判据自己加总的「清理后总量」**（不复述实现自报的数）。
      const expectedA = countedTotalBytes(adapter);
      if (observedA.value !== expectedA) {
        problems.push(
          `【核算时机·清理后】核算总量 ${observedA.value} 字节 ≠ 判据独立算的清理后总量 ` +
            `${expectedA} 字节（差 ${Math.abs(observedA.value - expectedA)} 字节）`,
        );
      }
    }
    drop(bulkA);
    drop(expiredCache);

    // ── 场景 B｜② 核算在**额外淘汰之前** ────────────────────────────────────
    const freshCache = join('catalog-cache', 'p3-account-fresh.json');
    adapter.writeBigFile(freshCache, P3_14_FIXTURE_BYTES); // **TTL 内**（mtime ＝ 现在）
    const bulkB = makeBulk('p3-account-bulk-b.dat');

    const sweepB = await adapter.runSweep({});
    if (sweepB?.broken === true) {
      notes.push(
        `场景 B 的观测失败（${String(sweepB.outcome ?? '').slice(0, 100)}）⇒ ② 未判定`,
      );
    } else {
      const observedB = readAccountedTotal(sweepB);
      if (observedB.value === null) {
        notes.push('② 核算在额外淘汰之前：读不到实现的核算总量 ⇒ 这一半**未判定**');
      } else {
        // 判据独立算的「清理后、**淘汰前**」总量 ＝ 清理后总量 ＋（若已被淘汰）那份缓存的字节数。
        const afterSweep = countedTotalBytes(adapter);
        const evictedBytes = adapter.exists(freshCache) ? 0 : P3_14_FIXTURE_BYTES;
        const expectedB = afterSweep + evictedBytes;
        if (observedB.value !== expectedB) {
          problems.push(
            `【核算时机·淘汰前】核算总量 ${observedB.value} 字节 ≠ 判据独立算的「清理后、淘汰前」总量 ` +
              `${expectedB} 字节（差 ${Math.abs(observedB.value - expectedB)} 字节）`,
          );
        }
      }
    }
    drop(bulkB);
    drop(freshCache);

    if (problems.length > 0) {
      return {
        status: FAIL,
        detail:
          p3Why({
            what: '缓存总量核算的**时机**（协议 §S-16 裁定 2）',
            observed: problems.join('；'),
            clause:
              '§S-16 裁定 2「核算排在『占用关系全量扫描 ＋ 常规清理』**之后**、『额外淘汰』**之前**' +
              ' ⇒ 核算读到的是**本轮该回收的都回收了之后**的总量（不是清理前的旧数）」',
          }) + (notes.length > 0 ? ` ⇒ 另：${notes.join('；')}` : ''),
      };
    }
    return {
      status: PASS,
      detail:
        '① 核算总量 == 判据独立算的**清理后总量**（那份 TTL 已过的 100 MB 目录缓存已被常规清理' +
        '回收 ⇒ 核算读的是清理后的数）；② 核算总量 == 判据独立算的「清理后、**淘汰前**」总量' +
        '（那份 TTL 内的 100 MB 目录缓存尚未被额外淘汰 ⇒ 核算读的是淘汰前的数）' +
        (notes.length > 0 ? `；另：${notes.join('；')}` : ''),
    };
  });
}
