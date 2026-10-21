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
 * ## Windows 侧（§S-14 裁定）
 *
 * `mode` 在 Windows 上是 **no-op**（Node 语义）⇒ 本条**只做 Unix 侧断言**，
 * **不做**跨平台加密断言（协议明写「本批只做如实记录」）。
 *
 * @param {object} adapter
 * @returns {Promise<{name: string, status: string, detail: string}>}
 */
export async function checkP3_7_storageProtection(adapter) {
  return guard('P3-7-storage-protection', async () => {
    const problems = [];

    // ① **Unix 私有权限**：目录缓存的目录与文件。
    const cacheDir = join('catalog-cache');
    adapter.ensureDir(cacheDir);
    const cacheFile = join(cacheDir, 'p3-perm-probe.json');
    adapter.writeFile(cacheFile, '{"page":{}}');
    const dirMode = adapter.modeOf(cacheDir);
    const fileMode = adapter.modeOf(cacheFile);
    // **Windows 上 `mode` 是 no-op** ⇒ 只在 Unix 侧断言（协议 §S-14②）。
    if (adapter.isUnix) {
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
    }

    // ① **防回退**：快照与认领的私有权限**仍在**。
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
    if (adapter.isUnix) {
      const snapMode = adapter.modeOf(join('snapshots', adapter.providerId, `${snapId}.json`));
      if ((snapMode & 0o777) !== 0o600) {
        problems.push(
          `快照文件的权限是 ${(snapMode & 0o777).toString(8)}，协议 §S-14 要求 0600（防回退）`,
        );
      }
    }

    // ② **日志不收正文**：错误日志只有四字段，且**不含**正文。
    const commands = adapter.buildCommands();
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
        detail: p3Why({
          what: '存储保护与落盘策略（协议 §S-14）',
          observed: problems.join('；'),
          clause: '§S-14「快照与占用记录 0600/0700；目录缓存补齐；日志不收正文；不声称跨平台加密」',
        }),
      };
    }
    return {
      status: PASS,
      detail: adapter.isUnix
        ? '目录缓存目录 0700 ／ 文件 0600；快照 0600；错误日志只有 at/code/requestId/detail 四字段且不含正文；实现代码没有把 0600 当加密的表述'
        : '（Windows：`mode` 是 no-op，本条只做如实记录）快照与认领的 mode 参数在位；错误日志只有四字段且不含正文；实现代码没有把 0600 当加密的表述',
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
