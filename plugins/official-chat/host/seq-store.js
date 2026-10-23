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
 * 选择序号、水位发布和认领共用一个互斥范围，避免迟到的分配者重用已回收的号。
 *
 * ## 落盘位置
 *
 * `<实例 official-chat 根>/seq-claims/<providerShort>/<sessionShort>/<seq>.json`
 *
 * **跟随后端路径解析**（协议 §5：按壳模式、内核族、实例、站点账户隔离，
 * 不在代码里展开真实用户 home）——根目录由壳注入的 `DSH_HOME` 给出，
 * 与快照目录、目录缓存目录**同一棵实例根**下，但**分开的三类路径**。
 *
 * ## 序号水位（协议 §2.5 / §S-13 裁定 4）：**`seq` 永不复用**
 *
 * 认领文件**仍按现状被删**（保留期清理照旧）⇒ 「这个 `seq` 用掉了」这件事
 * **不能**只靠认领文件记着：认领一删，号就能被二次分配 ⇒ 同一个
 * `snapshotId` 可能指向另一份内容（缺口 G3-1）。
 *
 * ⇒ 另有一份**独立的水位记录**（`<会话目录>/seq-watermark`，**不是** `<seq>.json`
 * 那种形状 ⇒ 认领的保留期清理**碰不到它**）：
 *
 * · **语义**：「这个会话**已经发放过**的最大 `seq`」⇒ 分配器**只从水位往后取**、
 *   **永不回头**。
 * · **写序是硬要求**：**水位先于认领落盘** ⇒ 不留「认领已建、水位未写」的窗口
 *   （崩在窗口里 ⇒ 水位丢失 ⇒ 那个 `seq` 仍会被复用）。
 * · **只许前进**：写进去的值 = `max(盘上已有的, 要写的)` ⇒ 水位不会倒退。
 *
 * @module xlink-official-chat/seq-store
 */

import {
  existsSync,
  mkdirSync,
  rmdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

import {
  WATERMARK_FILE_NAME,
  makeProviderShort,
  makeSessionShort,
  makeSnapshotId,
  makeWatermarkRecord,
  parseWatermarkRecord,
} from './protocol.js';

/**
 * 序号上限（协议 S-1：`seq` 是 4 位，`0001` 起）。
 *
 * **这是协议的硬上限，不是实现选的**：超了就得报错，不许绕（绕了就会出现
 * 两个同 id 的快照 = 第二份真相）。
 */
export const MAX_SEQ = 9999;

/**
 * 临时水位文件的**序号尾巴**（进程内自增）。
 *
 * 临时文件名 = `<水位文件>.tmp-<pid>-<本计数>` ⇒ ① 同一进程内不撞
 * ② 不同进程间不撞（pid 不会同时相同）⇒ ⇒ 并发写不会互相覆盖临时文件。
 */
let tmpCounter = 0;
const lockWait = new Int32Array(new SharedArrayBuffer(4));

// The lock covers read-max-publish; atomic rename alone cannot prevent an older writer winning.
function withWatermarkLock(path, write) {
  const lockPath = `${path}.lock`;
  const deadline = Date.now() + 15000;
  for (;;) {
    try {
      mkdirSync(lockPath, { mode: 0o700 });
    } catch (error) {
      if (error?.code !== 'EEXIST') throw error;
      // shortcut: crash-left locks fail closed; remove only after confirming no allocator is running.
      if (Date.now() >= deadline) {
        throw seqError('snapshot_invalid', '序号水位锁等待超时；确认其他取数进程均已退出后检查 seq-watermark.lock');
      }
      Atomics.wait(lockWait, 0, 0, 5);
      continue;
    }
    try {
      writeFileSync(join(lockPath, 'owner'), String(process.pid), { mode: 0o600 });
      return write();
    } finally {
      unlinkSync(join(lockPath, 'owner'));
      rmdirSync(lockPath);
    }
  }
}

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
   * 读这个会话的**水位**（协议 §2.5：「这个会话**已经发放过**的最大 `seq`」）。
   *
   * **读不动有两种，不许混**：
   * · **文件不在** ⇒ 从来没分配过 ⇒ 水位 0（`ok: true`）。
   * · **文件在、但读不出来/坏了** ⇒ 水位**不明**（`ok: false`）⇒ 调用方
   *   **拒绝分配**（宁可这一条会话停发，也不冒险复用序号）。
   *
   * @param {string} sessionShort - 会话短码。
   * @returns {{ok: true, seq: number} | {ok: false}}
   */
  const readWatermark = (sessionShort) => {
    const path = join(dirOf(sessionShort), WATERMARK_FILE_NAME);
    let raw;
    try {
      raw = readFileSync(path, 'utf8');
    } catch (error) {
      if (error?.code === 'ENOENT') return { ok: true, seq: 0 };
      return { ok: false };
    }
    return parseWatermarkRecord(raw);
  };

  /**
   * **写水位**（协议 §2.5）。
   *
   * ## 只许前进
   *
   * 写进去的值 = `max(盘上已有的, 要写的)` ⇒ **水位不会倒退**——跨进程各写一份时，
   * 后写的那次**不许**把水位拉回去（拉回去 ⇒ 那个号会被再分配 ⇒ 同一个
   * `snapshotId` 指向另一份内容）。
   *
   * ## 落盘必须是**原子**的
   *
   * **不许**用 `writeFileSync(path, …, { flag: 'w' })` 直接写目标文件 —— 那是
   * **原地截断后写**：并发读者会读到**半个文件**（空的或半截 JSON）⇒
   * `parseWatermarkRecord` 判它「坏」⇒ ⇒ 读者按「水位不明」**拒绝分配**
   * ⇒ ⇒ **两个壳同时开着时，随机有一个壳这条会话直接停发**（本机实测复现率
   * 约 1/3）；崩在写一半时还会留下**永久**坏掉的水位文件（这条会话再也发不出去）。
   *
   * ⇒ ⇒ 改**先写同目录的临时文件、再 `rename` 覆盖目标**：`rename` 在**同一
   * 文件系统**内是**原子**的 ⇒ ⇒ 读者要么看到旧记录、要么看到新记录，
   * **永远看不到半个文件** ⇒ ⇒ 「水位不明 ⇒ 拒绝分配」这条 fail-closed 纪律
   * **只对真的坏文件生效**，不再被并发写误伤。
   *
   * @param {string} sessionShort - 会话短码。
   * @param {number} seq - 要记的序号。
   * @returns {number} 实际写进去的序号。
   */
  const writeWatermark = (sessionShort, seq) => {
    const current = readWatermark(sessionShort);
    if (!current.ok) {
      // **水位不明 ⇒ 不写**（写下去就是猜）⇒ 让调用方拒绝分配。
      throw seqError('snapshot_invalid', '序号水位记录不可读（拒绝分配：宁可停发也不复用序号）');
    }
    const target = Math.max(current.seq, Number(seq ?? 0));
    const path = join(dirOf(sessionShort), WATERMARK_FILE_NAME);
    const payload = `${JSON.stringify(
      makeWatermarkRecord({
        providerShort,
        sessionShort,
        seq: target,
        at: new Date(now()).toISOString(),
      }),
      null,
      2,
    )}\n`;
    // 临时文件与目标**同目录** ⇒ 同一文件系统 ⇒ `rename` 是原子的。
    // ⚠️ 临时名**不以 `.json` 结尾** ⇒ 不会被「认领文件」扫描当成一个序号。
    const tmp = `${path}.tmp-${process.pid}-${(tmpCounter += 1)}`;
    try {
      writeFileSync(tmp, payload, { flag: 'w', mode: 0o600 });
      const deadline = Date.now() + 1000;
      for (;;) {
        try { renameSync(tmp, path); break; } catch (error) {
          if (!['EPERM', 'EACCES', 'EBUSY'].includes(error?.code) || Date.now() >= deadline) throw error;
          Atomics.wait(lockWait, 0, 0, 5);
        }
      }
    } catch (error) {
      // 临时文件清不掉**不影响**分配结果（它不以 `.json` 结尾 ⇒ 没人会认它）。
      try {
        unlinkSync(tmp);
      } catch {
        /* 清理失败不影响主流程 */
      }
      throw error;
    }
    return target;
  };

  /**
   * 分配下一个序号（**只从水位往后取**；认领用**排他创建**，跨进程不撞）。
   *
   * ## 为什么要「水位」而不是「目录里最小空缺」
   *
   * 旧写法取「目录里最小的空缺号」⇒ **认领被删之后那个号就空出来了** ⇒ 会被
   * 再分配 ⇒ 同一个 `snapshotId` 可能指向另一份内容（缺口 G3-1）。
   * ⇒ 现在**只从水位往后取**（协议 §2.5 / §S-13 裁定 4）：**永不回头**。
   *
   * ## 写序是硬要求
   *
   * **① 水位先落盘 → ② 再写认领** ⇒ 不留「认领已建、水位未写」的窗口
   * （崩在窗口里 ⇒ 水位丢失 ⇒ 那个 `seq` 仍会被复用）。
   *
   * ## 撞名（EEXIST）怎么办
   *
   * 别的进程抢到了同一个号 ⇒ **这个号作废**（水位已经前进）⇒ 顺着往后试下一个。
   * **宁可烧掉一个号，也不回头**（§S-13 裁定 4 的取舍）。
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

    return withWatermarkLock(join(dir, WATERMARK_FILE_NAME), () => {
      // **水位不明 ⇒ 拒绝分配**（协议 §2.5）：水位是「不复用」的唯一载体，
      // 它读不动 ⇒ 无从知道这个会话发到哪了 ⇒ 不敢接着发。
      const watermark = readWatermark(sessionShort);
      if (!watermark.ok) {
        throw seqError('snapshot_invalid', '序号水位记录不可读（拒绝分配：宁可停发也不复用序号）');
      }

      // **认领目录里已经占着的号**（跨进程抢过的那些）——它们虽然在水位之下或
      // 之上，都**不能重复发**。
      const used = new Set(claimedSeqs(sessionShort));
      for (let seq = watermark.seq + 1; seq <= MAX_SEQ; seq += 1) {
        if (used.has(seq)) continue;
        const path = join(dir, `${String(seq).padStart(4, '0')}.json`);
        // ① **水位先落盘**（协议 §2.5 的硬要求）。
        try {
          writeWatermark(sessionShort, seq);
        } catch (error) {
          if (typeof error?.code === 'string' && error.code !== '') throw error;
          throw seqError('snapshot_invalid', '序号水位写入失败：写不进水位记录');
        }
        // ② **再写认领**（`wx` = 文件已存在就报错。**谁建成谁拿到这个序号**——
        //    两个进程同时抢同一个序号时，只有一个能建成，另一个顺着往后试下一个）。
        try {
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
            // 正常路径，不是失败）。**这个号已经作废**（水位已前进）⇒ 不回头。
            continue;
          }
          throw seqError('snapshot_invalid', '序号认领失败：写不进认领文件');
        }
      }
      throw seqError(
        'source_unsupported',
        `这条会话的 pick 序号已用尽（协议 S-1 的 4 位上限 ${MAX_SEQ}）`,
      );
    });
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
   * 每个会话短码**已经发放过的最大序号**（`<providerShort>~<sessionShort>` → 最大序号）。
   *
   * **两份取大**：序号**认领**（分配器，`seq-claims/`）与**水位**
   * （同一个目录下的 `seq-watermark`）是同一条「用掉过」的两种记录，
   * 取大 = **认领被回收之后也不会倒退**（认领删了，水位还在）。
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
      const claimedMax = seqs.length === 0 ? 0 : seqs[seqs.length - 1];
      const watermark = readWatermark(sessionShort);
      const maxSeq = Math.max(claimedMax, watermark.ok ? watermark.seq : 0);
      if (maxSeq <= 0) continue;
      out[`${providerShort}~${sessionShort}`] = maxSeq;
    }
    return out;
  };

  /**
   * 某个会话的**水位**（协议 §2.5：「已经发放过的最大 seq」）——**只读**。
   *
   * @param {string} sessionShort - 会话短码。
   * @returns {{ok: true, seq: number} | {ok: false}} 读不动就是 `ok:false`。
   */
  const watermarkOf = (sessionShort) => readWatermark(sessionShort);

  return {
    available: claimsRoot !== null,
    reserve,
    isClaimed,
    usedSeqs,
    watermarkOf,
    providerShort,
  };
}
