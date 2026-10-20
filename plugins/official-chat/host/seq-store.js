/**
 * 快照 id 的**序号分配器**（协议 v0.2 §2 S-1「id 分配权归 host」）。
 *
 * ## 为什么需要它
 *
 * 协议 S-1 的格式是 `<providerShort>~<sessionShort>~<seq>`，其中 `seq` 是
 * 「该会话在**本实例内**的 pick 序号，4 位，单调递增」。v0.2 追加的那一条把
 * 分配权划给了 host：
 *
 * > 快照**落盘且跨进程可发**（S-6）→ 若 `seq` 由 client 的进程内计数生成，
 * > **dev 壳与 release 壳同开、或任一进程重启，都可能生成同一个
 * > `snapshotId` → 撞盘**（host 按「同 id 不重写」拒 → 用户看到「这次选择没
 * > 生效」）。⇒ **`seq` 由 host 侧统一分配**（host 持有并持久化已用序号，
 * > client 不得自造）；**「本实例内单调递增」必须跨进程成立**。
 *
 * 所以：**client 不再自己数序号**。它向 host 要一个 id，host 分配、host 落盘、
 * host 认账。
 *
 * ## 「同步的 pick」怎么办
 *
 * 内核的 `settle()` **同步**调用 `src.onPick(...)`（`input-trigger/client.js
 * @0.2.1-rc.2:1459-1473`），而 `onPick` 必须**同步**返回一个带 `ref` 的句柄
 * ——所以 pick 那一刻**问不到 host**（那是 Promise）。
 *
 * 解法是**把分配挪到更早的异步一步**：host 的 `catalog` 命令（协议 §5 的五个
 * 命令之一）在回目录页时，**顺带为每个目录项预分配一个 id**，客户端把它放进
 * 候选行里；pick 那一刻只是从候选行里取出那串 id。菜单是一次异步查询，
 * 所以「分配」发生在菜单打开时，「使用」发生在 pick 时——**中间没有 await**。
 *
 * ## 跨进程为什么不用「读计数器 → 加一 → 写回」
 *
 * 那个写法在两个进程同时分配时会**撞**：两个都读到 `next=5`，都写回 `next=6`，
 * 于是两个进程都以为 5 是自己的。而 dev 壳与 release 壳**可以同时开着**。
 *
 * 这里用**排他创建**（`wx`）认领：每个序号一个认领文件，谁建成谁拿到。
 * 两个进程抢同一个序号时，只有一个能建成，另一个顺着往后试下一个 ——
 * **不需要锁、不需要清理、进程崩了也不留半个状态**。
 *
 * ## 落盘位置
 *
 * `<实例 official-chat 根>/seq-claims/<providerShort>/<sessionShort>/<seq>.json`
 *
 * **跟随后端路径解析**（协议 §5：按壳模式、内核族、实例、站点账户隔离，
 * 不在代码里展开真实用户 home）——根目录由壳注入的 `DSH_HOME` 给出，
 * 与快照目录、目录缓存目录**同一棵实例根**下，但**分开的三类路径**。
 *
 * @module xlink-official-chat/seq-store
 */

import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { makeProviderShort, makeSessionShort, makeSnapshotId } from './protocol.js';

/**
 * 序号上限（协议 S-1：`seq` 是 4 位，`0001` 起）。
 *
 * **这是协议的硬上限，不是实现选的**：超了就得报错，不许绕（绕了就会出现
 * 两个同 id 的快照 = 第二份真相）。
 */
export const MAX_SEQ = 9999;

/**
 * 造一个带**稳定错误码**的错误。
 *
 * @param {string} code - 错误码。
 * @param {string} message - 人看的说明。
 * @returns {Error & {code: string}} 错误。
 */
function seqError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

/**
 * 序号分配器。
 *
 * @param {object} options
 * @param {string | null} options.root - `<实例 official-chat 根>`；null = 不可用。
 * @param {string} [options.providerId] - provider 稳定 id。
 * @param {() => number} [options.now] - 取当前时间（测试注入）。
 * @returns {{
 *   available: boolean,
 *   reserve: (sessionId: string) => {seq: number, snapshotId: string},
 *   isClaimed: (sessionShort: string, seq: number) => boolean,
 *   usedSeqs: () => Record<string, number>,
 * }}
 */
export function createSeqAllocator({ root, providerId = 'ds', now = () => Date.now() } = {}) {
  const providerShort = makeProviderShort(providerId);
  const claimsRoot = root === null || root === undefined ? null : join(root, 'seq-claims', providerShort);

  /**
   * 某个会话短码的认领目录。
   *
   * @param {string} sessionShort - 会话短码。
   * @returns {string} 目录路径。
   */
  const dirOf = (sessionShort) => join(claimsRoot, sessionShort);

  /**
   * 这个会话短码**已经认领**的序号（升序）。
   *
   * @param {string} sessionShort - 会话短码。
   * @returns {number[]} 已认领的序号。
   */
  const claimedSeqs = (sessionShort) => {
    let names;
    try {
      names = readdirSync(dirOf(sessionShort));
    } catch {
      return [];
    }
    const out = [];
    for (const name of names) {
      if (!name.endsWith('.json')) continue;
      const seq = Number(name.slice(0, -'.json'.length));
      if (Number.isFinite(seq) && seq > 0) out.push(seq);
    }
    return out.sort((a, b) => a - b);
  };

  /**
   * 分配下一个序号（**排他创建**，跨进程不撞）。
   *
   * @param {string} sessionId - 官网会话 id。
   * @returns {{seq: number, snapshotId: string}} 序号与它对应的快照 id。
   */
  const reserve = (sessionId) => {
    if (claimsRoot === null) {
      throw seqError(
        'snapshot_store_unavailable',
        '序号分配不可用：内核没有给出实例 home（DSH_HOME 缺失）',
      );
    }
    const sessionShort = makeSessionShort(sessionId);
    const dir = dirOf(sessionShort);
    mkdirSync(dir, { recursive: true, mode: 0o700 });

    const used = new Set(claimedSeqs(sessionShort));
    for (let seq = 1; seq <= MAX_SEQ; seq += 1) {
      if (used.has(seq)) continue;
      const path = join(dir, `${String(seq).padStart(4, '0')}.json`);
      try {
        // `wx` = 文件已存在就报错。**谁建成谁拿到这个序号**——两个进程同时
        // 抢同一个序号时，只有一个能建成（另一个撞 EEXIST，顺着往后试）。
        writeFileSync(
          path,
          `${JSON.stringify(
            {
              providerShort,
              sessionShort,
              seq,
              claimedAt: new Date(now()).toISOString(),
            },
            null,
            2,
          )}\n`,
          { flag: 'wx', mode: 0o600 },
        );
        return {
          seq,
          snapshotId: makeSnapshotId({ providerShort, sessionShort, seq }),
        };
      } catch (error) {
        if (error?.code === 'EEXIST') {
          // 别的进程先认领了这个 → 试下一个（**不报错**：这正是跨进程竞争的
          // 正常路径，不是失败）。
          continue;
        }
        throw seqError('snapshot_invalid', '序号认领失败：写不进认领文件');
      }
    }
    throw seqError(
      'source_unsupported',
      `这条会话的 pick 序号已用尽（协议 S-1 的 4 位上限 ${MAX_SEQ}）`,
    );
  };

  /**
   * 这个序号**是不是 host 认领过的**（客户端不能自造 seq 的守门人）。
   *
   * @param {string} sessionShort - 会话短码。
   * @param {number} seq - 序号。
   * @returns {boolean} 认领过即 true。
   */
  const isClaimed = (sessionShort, seq) => {
    if (!Number.isFinite(seq) || seq <= 0) return false;
    return existsSync(join(dirOf(sessionShort), `${String(seq).padStart(4, '0')}.json`));
  };

  /**
   * 每个会话短码已经认领的最大序号（`<providerShort>~<sessionShort>` → 最大序号）。
   *
   * **与快照目录里扫出来的最大值取大**：认领文件是「已分配」，快照是「已落盘」，
   * 两者取大 = 「这个会话用掉过的最大序号」，不会因为其中一份缺了而倒退。
   *
   * @returns {Record<string, number>} 会话键 → 已用最大序号。
   */
  const usedSeqs = () => {
    const out = {};
    if (claimsRoot === null || !existsSync(claimsRoot)) return out;
    let shorts;
    try {
      shorts = readdirSync(claimsRoot);
    } catch {
      return out;
    }
    for (const sessionShort of shorts) {
      const seqs = claimedSeqs(sessionShort);
      if (seqs.length === 0) continue;
      out[`${providerShort}~${sessionShort}`] = seqs[seqs.length - 1];
    }
    return out;
  };

  return { available: claimsRoot !== null, reserve, isClaimed, usedSeqs, providerShort };
}
