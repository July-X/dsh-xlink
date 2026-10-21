/**
 * **保留期主动清理**（协议 §4.1，v0.4 裁定）。
 *
 * ## 为什么要有它
 *
 * 协议 §4 把「快照保留期 7 天」列为首版常量，但此前它**只是常量**：没有过期
 * 检查、没有清理任务。§4.1 把它补成**真的行为**。
 *
 * ## 为什么是「惰性触发」而不是定时器
 *
 * **插件不常驻**：host 半边是内核进程里的一个插件，可能几小时都不被调用一次；
 * 定时器在进程退出时就没了，**「定时不可靠」**。所以改成**惰性触发**：
 * host 半边每次被唤起时**顺手检查一次**，至多一天一次。
 *
 * ## 触发点只在 `catalog`
 *
 * 协议 §4.1 第 1 条：**只在 `catalog` 入口**检查一次（`capture` **不触发**——
 * 用户主动路径更短更可控）。`catalog` 是目录查询，天然是「用户在看列表」的时机。
 *
 * ## 扫哪些（**五**家族，都在 `<root>` 下）
 *
 * · `snapshots/<provider>/*.json`——**正文快照**。按**文件内的 `capturedAt`** 判断
 *   是否过期（mtime 只作兜底：文件被复制/改写时 mtime 会骗人）。
 * · `snapshots/<provider>/.<id>.json.tmp-<pid>-<seq>-<attempt>`——**临时残留**
 *   （协议 §S-10 的**第五家族**，§2.3 的形态）：快照改成「临时文件 → 回读校验 →
 *   rename」发布之后，崩在发布途中就会留下这种文件。**它不是快照**（发布前
 *   不进清单）⇒ 按**自己的阈值**（§4 的 `tmpResidualMs`，24 小时）清，
 *   **不与 7 天保留期混用**。
 * · `seq-claims/<provider>/<sessionShort>/*.json`——**序号认领**。按 `claimedAt`
 *   判断，**且必须确认对应的快照已经不在**才可删（认领是「这个序号用掉了」的
 *   记录；快照还在时删认领，会让序号被二次分配 ⇒ **同 id 落盘 ⇒ 撞车**）。
 * · `catalog-cache/*.json`——**目录缓存**，按缓存 TTL（5 分钟）过期。
 *   **注意是平铺**（**没有 provider 子目录**）：文件名 =
 *   `<provider>__<账号范围>__<查询词>.json`（见 `host/commands.js` 的 `fileOf`）。
 *   协议 §5 的路径写法已按实现对齐（PM 20:2x）。
 * · `logs/official-chat.log`——**错误日志**，按**行数/大小**截断，**不按 7 天删**
 *   （日志是诊断证据，删了就查不到了）。
 *
 * ## 怎么避免误删
 *
 * · **只删超过 `snapshotRetentionMs` 的**（§4 的 7 天）。
 * · **顺序：先快照，后认领**（见上面认领那条的理由）。
 * · **两步法删除**：先 `rename` 成临时名，再 `unlink`。这样即使删到一半挂掉，
 *   留下的也不是一个「看起来有效」的半截文件——**快照文件被读时还要过摘要校验**
 *   （`host/index.js` 的 `store.load`），半截文件本来就该被拒，但**先改名**
 *   让「还在原地」的窗口更短。
 * · **只动本实例根下这五家族**；**根不可得就不扫**（宁可不清理，不乱删）。
 * · **清理失败只记日志，不阻塞业务**（协议 §4.1 第 3 条）。
 *
 * ## 不保护「正在发送中的引用」
 *
 * 协议 §4.1 第 4 条：不引入引用计数（协议没写，不自造）。被清理掉的引用，
 * 后果是**该引用失效并被拒**（`snapshot_invalid`），**不是错发**——S-5 / S-6
 * 的既有语义已经覆盖这个后果。
 *
 * @module xlink-official-chat/sweep
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { CONSTANTS, isTempSnapshotName, makeSnapshotId } from './protocol.js';

/** 清理日志的**行数上限**（超了从头截，只留尾部）。 */
export const LOG_MAX_LINES = 2000;

/** 清理日志的**字节上限**（超了从头截，只留尾部）。 */
export const LOG_MAX_BYTES = 512 * 1024;

/**
 * 两步法删除：先改名，再删。
 *
 * **为什么不是直接 `unlink`**：删到一半挂掉时，原地会留下一个**看起来有效**的
 * 文件（快照被读时靠摘要校验兜底，但缓存/认领没有这层校验）。先改名 ⇒
 * 「原地还有一个有效文件」的窗口最短。
 *
 * **注意**：这里**不记日志**——留痕在**调用处**（`else logger?.warn?.(...)`）。
 * 理由：**删不掉这件事，调用点才知道「删的是什么」**（哪个家族、哪个文件）；
 * 而这里的 `catch` 只知道「改名/删除这一步失败了」。留痕统一放在调用处，
 * 口径才一致（§4.1 第 3 条「失败只记日志」⇒ **必须留痕**）。
 *
 * @param {string} path - 要删的文件。
 * @returns {boolean} 删掉了即 true。
 */
function removeInTwoSteps(path) {
  const tombstone = `${path}.expired-${Date.now()}`;
  try {
    renameSync(path, tombstone);
  } catch {
    return false;
  }
  try {
    unlinkSync(tombstone);
  } catch {
    return false;
  }
  return true;
}

/**
 * 取文件的时间戳（**优先用文件内的业务时间**，mtime 只作兜底）。
 *
 * @param {string} path - 文件路径。
 * @param {string} field - 业务时间字段名（如 `capturedAt` / `claimedAt`）。
 * @param {number} fallback - 读不到业务时间时的兜底（文件 mtime）。
 * @returns {number} 毫秒时间戳；取不到就是 0。
 */
function stampOf(path, field, fallback) {
  try {
    const parsed = JSON.parse(readFileSync(path, 'utf8'));
    const raw = String(parsed?.[field] ?? '');
    const value = Date.parse(raw);
    if (Number.isFinite(value)) return value;
  } catch {
    /* 读不动就退到 mtime */
  }
  return fallback;
}

/**
 * 读**序号认领文件**的内容。
 *
 * **为什么必须读文件，不能靠目录名猜**：认领文件的落盘路径
 * （`<root>/seq-claims/<providerShort>/<sessionShort>/<seq>.json`）里，
 * `providerShort` 与 `seq` 两段**只存在于文件名**、而**权威值在文件内容**
 * （`host/seq-store.js` 的 `reserve` 写的正是 `providerShort` /
 * `sessionShort` / `seq` / `claimedAt` 四项）。猜错 ⇒ 把「快照还在」误判成
 * 「已不在」⇒ **误删认领** ⇒ 那个序号被二次分配 ⇒ 撞上仍然存在的快照 id。
 *
 * @param {string} path - 认领文件路径。
 * @returns {object|null} 读不动就是 null（调用方按「保守不删」处理）。
 */
function readClaim(path) {
  try {
    const parsed = JSON.parse(readFileSync(path, 'utf8'));
    return parsed && typeof parsed === 'object' ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * 建清理器。
 *
 * @param {object} options
 * @param {string|null} options.root - `<实例 official-chat 根>`；**不可得就不扫**。
 * @param {number} [options.retentionMs] - 保留期（默认 §4 的 `snapshotRetentionMs`）。
 * @param {number} [options.cacheTtlMs] - 目录缓存 TTL（默认 §4 的 `catalogCacheTtlMs`）。
 * @param {string} [options.providerId] - provider 短码。
 * @param {() => number} [options.now] - 当前时间（测试注入）。
 * @param {object} [options.logger] - 日志（只记失败，不记正文）。
 * @returns {object} 清理器。
 */
export function createSweeper({
  root,
  retentionMs = CONSTANTS.snapshotRetentionMs,
  cacheTtlMs = CONSTANTS.catalogCacheTtlMs,
  tmpResidualMs = CONSTANTS.tmpResidualMs,
  providerId = 'ds',
  now = () => Date.now(),
  logger,
} = {}) {
  /**
   * 本地日期（`YYYY-MM-DD`，**按本地时区**——节流键是「一天一次」，用 UTC 会
   * 让本地的早晨和傍晚各触发一次）。
   *
   * @param {number} at - 毫秒时间戳。
   * @returns {string}
   */
  const dayKey = (at) => {
    const d = new Date(at);
    const pad = (n) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };

  /**
   * 今天是否已经扫过。
   *
   * @returns {boolean}
   */
  const alreadySweptToday = () => {
    if (root === null || root === undefined) return true;
    return existsSync(join(root, 'sweeps', `last-sweep-${dayKey(now())}.json`));
  };

  /**
   * 记下「今天扫过了」。
   */
  const stampSweep = () => {
    if (root === null || root === undefined) return;
    const dir = join(root, 'sweeps');
    try {
      mkdirSync(dir, { recursive: true, mode: 0o700 });
      writeFileSync(
        join(dir, `last-sweep-${dayKey(now())}.json`),
        `${JSON.stringify({ sweptAt: new Date(now()).toISOString() }, null, 2)}\n`,
        { flag: 'w', mode: 0o600 },
      );
    } catch (error) {
      logger?.warn?.(`official-chat: 记清理节流键失败：${String(error?.message ?? error)}`);
    }
  };

  /**
   * 文件的**相对路径标签**（相对本实例根）——给日志用。
   *
   * **为什么不写绝对路径**：绝对路径带用户目录，而日志是要长期留存的诊断证据。
   *
   * @param {string} target - 绝对路径。
   * @returns {string}
   */
  const labelOf = (target) => {
    if (typeof root !== 'string' || root === '') return target;
    return target.startsWith(root) ? target.slice(root.length).replace(/^[\\/]/, '') : target;
  };

  /**
   * 清**正文快照**（过期 = 超过保留期）。
   *
   * @returns {{scanned: number, removed: number}}
   */
  const sweepSnapshots = () => {
    const result = { scanned: 0, removed: 0 };
    const dir = join(root, 'snapshots', providerId);
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return result;
    }
    const cutoff = now() - retentionMs;
    for (const name of names) {
      if (!name.endsWith('.json')) continue;
      const path = join(dir, name);
      result.scanned += 1;
      let mtime = 0;
      try {
        mtime = statSync(path).mtimeMs;
      } catch {
        continue;
      }
      if (stampOf(path, 'capturedAt', mtime) >= cutoff) continue;
      const rel = labelOf(path);
      if (removeInTwoSteps(path)) result.removed += 1;
      else logger?.warn?.(`official-chat: 保留期清理失败：${rel} 删不掉（已保留，未假装删掉）`);
    }
    return result;
  };

  /**
   * 清**临时残留**（协议 §S-10 的**第五家族**）。
   *
   * ## 清理条件（四条，缺一不可）
   *
   * 1. **文件名匹配临时名形态**（§2.3：`.` 开头、带 `.json.tmp-` 标记）
   *    ⇒ **不是临时名形态的文件一律不碰**（普通快照、认领、缓存都不归它管）。
   * 2. **在本实例根下**：删除路径由 `root` join 得出 ⇒ **不越出缓存根**。
   * 3. **超过阈值**（§4 的 `tmpResidualMs`，24 小时）⇒ **阈值内不动**。
   * 4. **不在「本轮刚写、正在发布」的窗口内**——这一条由第 3 条**顺带覆盖**：
   *    正在发布的临时文件 mtime 是最新的 ⇒ 必然落在阈值内 ⇒ 不会被删。
   *
   * ## 删除方式
   *
   * **两步法**（先 `rename` 成墓碑，再 `unlink`）——沿用本文件既有的做法
   * （见 `removeInTwoSteps`）：删到一半挂掉时，原地留下的不是一个「看起来
   * 有效」的文件。
   *
   * ## 失败怎么办
   *
   * **只记日志，不抛异常、不阻塞整轮**（协议 §4.1 第 3 条的四可观测条件）。
   *
   * @returns {{scanned: number, removed: number}}
   */
  const sweepTempResiduals = () => {
    const result = { scanned: 0, removed: 0 };
    const dir = join(root, 'snapshots', providerId);
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return result;
    }
    const cutoff = now() - tmpResidualMs;
    for (const name of names) {
      // **只清临时名形态**（条件 1）。
      if (!isTempSnapshotName(name)) continue;
      // **路径由 root join 得出**（条件 2）⇒ 不会越出缓存根。
      const path = join(dir, name);
      let st;
      try {
        st = statSync(path);
      } catch {
        continue;
      }
      if (!st.isFile()) continue;
      result.scanned += 1;
      // **阈值内不动**（条件 3 ＋ 4：正在发布的临时文件 mtime 是最新的）。
      if (st.mtimeMs >= cutoff) continue;
      const rel = labelOf(path);
      if (removeInTwoSteps(path)) result.removed += 1;
      else logger?.warn?.(`official-chat: 临时残留清理失败：${rel} 删不掉（已保留，未假装删掉）`);
    }
    return result;
  };

  /**
   * 清**序号认领**（过期 = 超过保留期，**且对应快照已经不在**）。
   *
   * @returns {{scanned: number, removed: number}}
   */
  const sweepClaims = () => {
    const result = { scanned: 0, removed: 0 };
    const claimsRoot = join(root, 'seq-claims', providerId);
    let sessions;
    try {
      sessions = readdirSync(claimsRoot, { withFileTypes: true });
    } catch {
      return result;
    }
    const cutoff = now() - retentionMs;
    for (const session of sessions) {
      if (!session.isDirectory()) continue;
      const sessionDir = join(claimsRoot, session.name);
      let names;
      try {
        names = readdirSync(sessionDir);
      } catch {
        continue;
      }
      for (const name of names) {
        if (!name.endsWith('.json')) continue;
        const path = join(sessionDir, name);
        result.scanned += 1;
        let mtime = 0;
        try {
          mtime = statSync(path).mtimeMs;
        } catch {
          continue;
        }
        if (stampOf(path, 'claimedAt', mtime) >= cutoff) continue;
        // **快照还在就不删认领**：认领是「序号用掉了」的凭证，先删它会让那个
        // 序号被二次分配 ⇒ 撞上仍然存在的快照 id ⇒ 落盘被拒。
        //
        // **快照 id 由认领文件里的字段拼**（`providerShort` / `sessionShort` /
        // `seq`，见 `host/seq-store.js` 的认领文件内容），**不靠目录名猜**——
        // 目录名只是会话短码，拼错了就会把「快照还在」误判成「已不在」⇒ 误删认领。
        const claim = readClaim(path);
        // **读不动就不删**（协议 §4.1 的「宁可不清理，不乱删」）：认领文件坏了，
        // 说明「这个序号用掉了」这件事**无法确认** ⇒ 删了就会二次分配。
        if (claim === null) continue;
        const providerShort = String(claim.providerShort ?? providerId);
        const sessionShort = String(claim.sessionShort ?? session.name);
        const seq = String(claim.seq ?? name.slice(0, -'.json'.length));
        const snapshotId = makeSnapshotId({
          providerShort,
          sessionShort,
          seq: Number(seq),
        });
        const snapshotStillThere = [
          join(root, 'snapshots', providerId, `${snapshotId}.json`),
          join(root, 'snapshots', providerShort, `${snapshotId}.json`),
        ].some((candidate) => existsSync(candidate));
        if (snapshotStillThere) continue;
        const rel = labelOf(path);
        if (removeInTwoSteps(path)) result.removed += 1;
        else logger?.warn?.(`official-chat: 保留期清理失败：${rel} 删不掉（已保留，未假装删掉）`);
      }
    }
    return result;
  };

  /**
   * 清**目录缓存**（过期 = 超过缓存 TTL）。
   *
   * @returns {{scanned: number, removed: number}}
   */
  const sweepCache = () => {
    const result = { scanned: 0, removed: 0 };
    const dir = join(root, 'catalog-cache');
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return result;
    }
    const cutoff = now() - cacheTtlMs;
    for (const name of names) {
      const path = join(dir, name);
      let st;
      try {
        st = statSync(path);
      } catch {
        continue;
      }
      if (!st.isFile()) continue;
      result.scanned += 1;
      if (st.mtimeMs >= cutoff) continue;
      const rel = labelOf(path);
      if (removeInTwoSteps(path)) result.removed += 1;
      else logger?.warn?.(`official-chat: 保留期清理失败：${rel} 删不掉（已保留，未假装删掉）`);
    }
    return result;
  };

  /**
   * **截断**错误日志（按行数/大小，**不按 7 天删**——日志是诊断证据）。
   *
   * ## 为什么两个上限都要
   *
   * 协议 §4.1 第 2 条写的是「按**行数/大小**截断」——两个上限。
   * 只按行数截断 ⇒ 一行超长的日志（堆栈、整段响应体）仍能撑爆磁盘；
   * 只按字节截断 ⇒ 一份「每行都短但行数巨大」的日志会整份被砍。
   * ⇒ **先按行数砍尾部，再按字节砍尾部**，两个上限都要满足。
   *
   * **按字节砍时按行退**，不直接切字节数组——那会把一个多字节字符切成两半，
   * 落盘变成乱码（日志是诊断证据，乱码等于毁证据）。
   *
   * @returns {{truncated: boolean}}
   */
  const sweepLog = () => {
    const path = join(root, 'logs', 'official-chat.log');
    let raw;
    try {
      raw = readFileSync(path, 'utf8');
    } catch {
      return { truncated: false };
    }
    const lines = raw.split('\n');
    const tooManyLines = lines.length > LOG_MAX_LINES;
    const tooManyBytes = Buffer.byteLength(raw, 'utf8') > LOG_MAX_BYTES;
    if (!tooManyLines && !tooManyBytes) return { truncated: false };
    // 先按行数留尾部。
    let kept = lines.slice(-LOG_MAX_LINES).join('\n');
    // 还超字节上限 ⇒ 再按行从尾部退，直到落在上限内。
    if (Buffer.byteLength(kept, 'utf8') > LOG_MAX_BYTES) {
      const keptLines = [];
      let bytes = 0;
      for (let i = lines.length - 1; i >= 0; i -= 1) {
        const lineBytes = Buffer.byteLength(lines[i], 'utf8') + 1;
        if (bytes + lineBytes > LOG_MAX_BYTES) break;
        keptLines.unshift(lines[i]);
        bytes += lineBytes;
      }
      kept = keptLines.join('\n');
    }
    try {
      writeFileSync(path, kept, { flag: 'w', mode: 0o600 });
    } catch (error) {
      // **截断失败也要留痕**（§4.1 第 3 条「失败只记日志」含「必须留痕」）：
      // 截断失败意味着日志会一直涨 ⇒ 静默吞掉 = 磁盘问题被藏起来。
      logger?.warn?.(
        `official-chat: 保留期清理：日志截断写回失败：${String(error?.message ?? error)}`,
      );
      return { truncated: false };
    }
    return { truncated: true };
  };

  /**
   * 跑一次清理。
   *
   * **只动本实例根下那四家族**；根不可得 ⇒ **不扫**（宁可不清理，不乱删）。
   *
   * @returns {{skipped: boolean, reason?: string, snapshots?: object, tmp?: object, claims?: object, cache?: object, log?: object}}
   */
  function sweep() {
    if (root === null || root === undefined) {
      return { skipped: true, reason: '落盘根不可得（内核没给实例 home）——不扫' };
    }
    const snapshots = sweepSnapshots();
    // **临时残留**（协议 §S-10 的第五家族）：与快照**同一个目录**，但**另一类**
    // （未发布的临时文件**不是快照**）⇒ 单独清，不混进快照家族。
    const tmp = sweepTempResiduals();
    // **顺序：先快照，后认领**（协议 §4.1 第 3 条）。
    //
    // **同轮效果**（别误读成「快照还在就永远删不掉认领」）：过期的快照在本轮
    // 已被删掉 ⇒ 它的过期认领随后就满足「过期**且**快照已不在」⇒ **一起清干净**。
    // 「快照还在 ⇒ 认领留着」保护的是**快照还没过期**的那一份（删认领会让序号被
    // 二次分配 ⇒ 撞上仍然存在的快照 id）。
    const claims = sweepClaims();
    const cache = sweepCache();
    const log = sweepLog();
    return { skipped: false, snapshots, tmp, claims, cache, log };
  }

  /**
   * **惰性入口**：至多一天跑一次。
   *
   * @returns {object} 同 {@link sweep}，外加 `ran`（这次是不是真跑了）。
   */
  function maybeSweep() {
    if (root === null || root === undefined) {
      return { skipped: true, ran: false, reason: '落盘根不可得——不扫' };
    }
    if (alreadySweptToday()) {
      return { skipped: true, ran: false, reason: '今天已经扫过' };
    }
    let outcome;
    try {
      outcome = sweep();
    } catch (error) {
      // **清理失败只记日志，不阻塞业务**（协议 §4.1 第 3 条）。
      logger?.warn?.(`official-chat: 保留期清理失败：${String(error?.message ?? error)}`);
      return { skipped: true, ran: false, reason: '清理失败（已记日志）' };
    }
    stampSweep();
    return { ...outcome, ran: true };
  }

  return { sweep, maybeSweep, alreadySweptToday };
}
