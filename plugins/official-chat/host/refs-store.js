/**
 * **占用记录 `refs/`**（协议 P3 v0.3 §2.4 / §S-11 / §S-12）。
 *
 * ## 它是什么
 *
 * 一份快照**被占着**的凭证：**有 / 无**，**不是计数器**（§S-11 裁定 4：
 * 同一快照被引用两次，**仍是一条**占用记录——id 唯一 ≠ 正文去重，S-7）。
 *
 * 它**不是**内核草稿的镜像：内核 source 契约里**查不到**「引用被移除 /
 * 草稿被放弃 / 已发送成功」的信号（§S-12 已逐条核过）⇒ 本记录是
 * **插件侧自洽的占用凭证**，只保证「回收时不误删还在用的快照」。
 *
 * ## 形态
 *
 * `<root>/refs/<snapshotId>.json`，内容**只有四个字段**
 * （`snapshotId` / `capturedAt` / `createdAt` / `sessionId`）——
 * **不含正文、不含摘要**（§S-14「日志与诊断包不收正文」的同款纪律）。
 *
 * ## 三条释放规则（§S-11 裁定 2）
 *
 * · **R1 显式释放**：host 侧**显式调用** `release()` ⇒ 删记录。
 *   ⚠️ 本阶段 R1 **只有「显式释放调用」这一条可判定形态**——触发条件
 *   （用户显式移除 / 该次 pick 被取消）在当前内核契约下**不可判定**
 *   ⇒ **client 侧不接线**（现役 client 也没有移除 chip 的入口；
 *   造那个 UI 是 P4/UX 的活，超出本批）。
 * · **R2 兜底释放**：记录**超过占用保留期**（§4 的 `occupancyRetentionMs`，
 *   7 天）⇒ 视为失效并删。**由清理流程负责**，不是释放入口负责。
 * · **R3 拒绝释放**：记录**写坏 / 读不出** ⇒ **不删**，按被占着处理
 *   （读不动 ⇒「这份快照被占着」这件事**无法证伪** ⇒ P1 §4.1
 *   「宁可不清理，不乱删」的同款口径）。
 *
 * ## 恢复顺序（§S-12 裁定 3）
 *
 * **先重建占用关系，再回收** ⇒ 回收动作之前**必须**先完成一次 `refs/` 的
 * **全量扫描**。本模块的 `scan()` 就是那一次扫描；`host/sweep.js` 在删快照
 * **之前**调用它（次序可机检）。
 *
 * @module xlink-official-chat/refs-store
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { CONSTANTS } from './protocol.js';

/**
 * 占用记录的**目录名**（协议 §2.4：`<root>/refs/<snapshotId>.json`）。
 *
 * **平铺**（没有 provider 子目录）：文件名就是快照 id ＋ `.json`
 * ⇒ 「这份快照有没有被占着」＝「`<root>/refs/<id>.json` 在不在」。
 */
export const REFS_DIR_NAME = 'refs';

/**
 * 占用记录的**四个字段**（协议 §2.4：只有这四个）。
 *
 * **为什么把它列成常量**：这个集合是**闭集**——多一个字段就是「形态不合」
 * （正文、摘要、计数一律不许进来）⇒ 写盘点与自测都从**这一份**取。
 */
export const OCCUPANCY_FIELDS = Object.freeze(['snapshotId', 'capturedAt', 'createdAt', 'sessionId']);

/**
 * 两步法删除：先 `rename` 成墓碑，再 `unlink`。
 *
 * **为什么不是直接 `unlink`**：删到一半挂掉时，原地会留下一个**看起来有效**
 * 的文件（占用记录被别的逻辑读到了 ⇒ 「这份快照被占着」会被误判）。
 * 先改名 ⇒ 「原地还有一个有效文件」的窗口最短。
 *
 * **与 `host/sweep.js` 的同名 helper 同款**（那里是清理器内部的那一份；
 * 两处各写各的是因为**互不依赖**——本模块不 import 清理器，
 * 清理器也不 import 本模块的删除细节）。
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
 * 造一个占用记录存储。
 *
 * @param {object} options
 * @param {string|null} options.root - `<实例 official-chat 根>`；null = 不可用。
 * @param {() => number} [options.now] - 取当前时间（测试注入）。
 * @param {number} [options.occupancyRetentionMs] - 占用保留期
 *   （默认 §4 的 `occupancyRetentionMs`，7 天）。
 * @returns {{
 *   available: boolean,
 *   pathOf: (snapshotId: string) => string,
 *   write: (input: {snapshotId: string, capturedAt?: string, createdAt?: string, sessionId?: string}) => {path: string, record: object},
 *   release: (snapshotId: string) => boolean,
 *   scan: () => {scanned: number, records: object[]},
 *   releaseExpired: (scan?: object) => {scanned: number, removed: number, occupied: string[]},
 *   isOccupied: (snapshotId: string) => boolean,
 * }}
 */
export function createRefsStore({
  root,
  now = () => Date.now(),
  occupancyRetentionMs = CONSTANTS.occupancyRetentionMs,
} = {}) {
  const refsRoot = root === null || root === undefined ? null : join(root, REFS_DIR_NAME);

  /**
   * 某份快照的占用记录**路径**。
   *
   * @param {string} snapshotId - 快照 id。
   * @returns {string} 绝对路径；根不可得时是空串。
   */
  const pathOf = (snapshotId) =>
    refsRoot === null ? '' : join(refsRoot, `${String(snapshotId ?? '')}.json`);

  /**
   * **写占用记录**（协议 §S-11 裁定 1）。
   *
   * **只写四个字段**（`OCCUPANCY_FIELDS`）——正文、摘要、引用计数一律不进来。
   *
   * @param {object} input
   * @param {string} input.snapshotId - 快照 id。
   * @param {string} [input.capturedAt] - 快照的捕获时刻。
   * @param {string} [input.createdAt] - 本记录的创建时刻（默认现在）。
   * @param {string} [input.sessionId] - 官网会话 id。
   * @returns {{path: string, record: object}} 落盘结果。
   */
  const write = ({ snapshotId, capturedAt, createdAt, sessionId } = {}) => {
    if (refsRoot === null) {
      throw new Error('占用记录不可用：落盘根不可得');
    }
    const id = String(snapshotId ?? '');
    if (id === '') {
      throw new Error('占用记录不可用：快照 id 为空');
    }
    const record = {
      snapshotId: id,
      capturedAt: String(capturedAt ?? ''),
      createdAt: String(createdAt ?? new Date(now()).toISOString()),
      sessionId: String(sessionId ?? ''),
    };
    mkdirSync(refsRoot, { recursive: true, mode: 0o700 });
    const path = pathOf(id);
    // **同一条占用关系只写一份**（不引入引用计数）⇒ 同一快照被引用两次，
    // 写第二次是**覆盖同一条**，不是新增一条。
    writeFileSync(path, `${JSON.stringify(record, null, 2)}\n`, { flag: 'w', mode: 0o600 });
    return { path, record };
  };

  /**
   * **R1 显式释放**：删掉某份快照的占用记录。
   *
   * **本阶段唯一可判定的释放形态**（§S-11 裁定 2 第 1 条 + §S-12 的降级口径）：
   * 由 host 侧**显式调用**。client 侧本批**不接线**。
   *
   * @param {string} snapshotId - 快照 id。
   * @returns {boolean} 删掉了即 true（本来就没有也算「已是释放态」→ false）。
   */
  const release = (snapshotId) => {
    if (refsRoot === null) return false;
    const path = pathOf(snapshotId);
    if (!existsSync(path)) return false;
    return removeInTwoSteps(path);
  };

  /**
   * 这条记录**过期了没有**（R2 的判据）。
   *
   * ## 时间信号怎么取
   *
   * **记录里的业务时间优先，mtime 只兜底**（P1 判据 A18j 的同款口径：
   * 「`capturedAt` 优先，mtime 只兜底」——文件被复制/改写时 mtime 会骗人）。
   * 这里取 **`createdAt` → `capturedAt` → 文件 mtime** 这个**先后**：
   * `createdAt` 是「这份占用关系从什么时候开始」的准确答案；
   * `capturedAt` 兜住「记录里没写 createdAt」的形态；mtime 是最后一道。
   *
   * @param {object|null} record - 读出来的记录（读不动就是 null）。
   * @param {number} mtime - 文件 mtime（毫秒；取不到就是 0）。
   * @returns {boolean} 过期即 true。
   */
  const isExpired = (record, mtime) => {
    const cutoff = now() - occupancyRetentionMs;
    for (const field of ['createdAt', 'capturedAt']) {
      const value = Date.parse(String(record?.[field] ?? ''));
      if (Number.isFinite(value)) return value < cutoff;
    }
    // **两个业务时间都不可用** ⇒ 只看 mtime。
    if (Number.isFinite(mtime) && mtime > 0) return mtime < cutoff;
    // **一个时间信号都没有** ⇒ 判不了过期 ⇒ **不删**（宁可不清理，不乱删）。
    return false;
  };

  /**
   * **全量扫描占用记录**（协议 §S-12 裁定 3「先重建占用关系，再回收」）。
   *
   * ## 为什么「目录不存在」也要真的读一次
   *
   * 扫描这一步**必须是一次可观测的全量扫描**——不能因为「目录还没建」就
   * 跳过（那样「先扫后收」的次序就无从观测）。⇒ **无条件 `readdirSync`**，
   * 读不到就当「没有占用记录」。
   *
   * ## `occupied` 里装的是什么
   *
   * **这一轮扫到的全部占用记录**（读得动的 ＋ **读不动的**）——R3：读不动 ⇒
   * 「这份快照被占着」无法证伪 ⇒ 按被占着处理 ⇒ **也在里面**。
   *
   * **含已过期的那些**：这一轮的回收按「**本轮开始时的占用关系**」判
   * （§S-11③「存在 ⇒ 不删」）⇒ 过期记录在这一轮**仍然保护**它那份快照；
   * 它在 `releaseExpired` 里被释放之后，**从下一轮起**那份快照才可回收。
   *
   * @returns {{scanned: number, records: object[], occupied: string[]}} 扫描结果。
   */
  const scan = () => {
    const result = { scanned: 0, records: [], occupied: [] };
    if (refsRoot === null) return result;
    let names = [];
    try {
      names = readdirSync(refsRoot);
    } catch {
      // **目录还没有** ⇒ 没有占用记录（不是错误）。
      return result;
    }
    for (const name of names) {
      if (!name.endsWith('.json')) continue;
      const snapshotId = name.slice(0, -'.json'.length);
      const path = join(refsRoot, name);
      result.scanned += 1;
      let record = null;
      let readable = false;
      try {
        const parsed = JSON.parse(readFileSync(path, 'utf8'));
        if (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)) {
          record = parsed;
          readable = true;
        }
      } catch {
        // **读不动 / 读出来不是对象** ⇒ 按 R3 处理（不删、按被占着）。
        record = null;
        readable = false;
      }
      let mtime = 0;
      try {
        mtime = statSync(path).mtimeMs;
      } catch {
        mtime = 0;
      }
      result.records.push({
        snapshotId,
        path,
        record,
        readable,
        expired: readable ? isExpired(record, mtime) : false,
      });
      // **任何一条占用记录都算「占着」**（读坏的也算 —— R3）。
      result.occupied.push(snapshotId);
    }
    return result;
  };

  /**
   * **R2 兜底释放**：过期占用记录视为失效并删（**由清理流程负责**）。
   *
   * ## 顺序（这一条是硬要求）
   *
   * **先扫描、再释放** ⇒ 释放动作之前**一定**先完成了一次全量扫描
   * （§S-12 裁定 3）。
   *
   * ## 「释放了」与「不再保护」**不是同一时刻**
   *
   * 协议 §S-11③：「存在 ⇒ 不删（除非它自己也过期，按 R2）」。
   * ⇒ **这一轮的快照回收**按「**本轮开始时的占用关系**」判（上面 `scan()`
   * 的 `occupied`）⇒ **这一轮扫到的过期记录仍然保护它那份快照**；
   * 它在这里被释放之后，**从下一轮起**那份快照才可回收。
   *
   * **为什么不是「同轮释放、同轮就回收」**：那样「占用记录存在 ⇒ 快照不删」
   * 这条在记录**恰好过期**的那一轮就**形同虚设**（记录一过期，快照立刻可删）
   * ⇒ 判据 P3-5①（有占用记录的快照不许被回收）就红。
   *
   * @param {object} [scanResult] - 复用一次扫描结果（不给就现扫）。
   * @returns {{scanned: number, removed: number, released: string[]}} 结果。
   */
  const releaseExpired = (scanResult) => {
    const scanned = scanResult === undefined ? scan() : scanResult;
    const released = [];
    let removed = 0;
    for (const item of scanned.records) {
      if (!item.readable) {
        // **R3：写坏 / 读不出 ⇒ 不删**，按被占着处理。
        continue;
      }
      if (!item.expired) continue;
      if (removeInTwoSteps(item.path)) {
        removed += 1;
        released.push(item.snapshotId);
        continue;
      }
      // **删不掉就不算释放**（不许假装删了）⇒ 它仍然占着 ⇒ 快照照旧不删。
    }
    return { scanned: scanned.scanned, removed, released };
  };

  /**
   * 某份快照**是不是被占着**。
   *
   * **只认「在不在」**：读坏也算占着（R3）——「读不动 ⇒ 无法证伪」。
   *
   * @param {string} snapshotId - 快照 id。
   * @returns {boolean} 占着即 true。
   */
  const isOccupied = (snapshotId) => {
    if (refsRoot === null) return false;
    return existsSync(pathOf(snapshotId));
  };

  return { available: refsRoot !== null, pathOf, write, release, scan, releaseExpired, isOccupied };
}
