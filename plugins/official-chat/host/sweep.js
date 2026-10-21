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
 * ## 扫哪些（**六**家族，都在 `<root>` 下）
 *
 * · `snapshots/<provider>/*.json`——**正文快照**。按**文件内的 `capturedAt`** 判断
 *   是否过期（mtime 只作兜底：文件被复制/改写时 mtime 会骗人）。
 *   **被占用的快照不删**（协议 §S-11 裁定 3）：删除前**必须**查
 *   `<root>/refs/<snapshotId>.json`——**存在 ⇒ 不删**（除非它自己也过期，按 R2）。
 * · `refs/<snapshotId>.json`——**占用记录**（协议 §2.4 的第六家族）：**有 / 无**的
 *   凭证，**不是计数器**。按**记录自己的时间**判断是否过了占用保留期
 *   （§4 的 `occupancyRetentionMs`，7 天）⇒ 过期即**兜底释放**（R2）。
 *   **读坏 / 读不出 ⇒ 不删**（R3）——读不动 ⇒「这份快照被占着」无法证伪。
 * · `snapshots/<provider>/.<id>.json.tmp-<pid>-<seq>-<attempt>`——**临时残留**
 *   （协议 §S-10 的**第五家族**，§2.3 的形态）：快照改成「临时文件 → 回读校验 →
 *   rename」发布之后，崩在发布途中就会留下这种文件。**它不是快照**（发布前
 *   不进清单）⇒ 按**自己的阈值**（§4 的 `tmpResidualMs`，24 小时）清，
 *   **不与 7 天保留期混用**。
 * · `seq-claims/<provider>/<sessionShort>/*.json`——**序号认领**。按 `claimedAt`
 *   判断，**且必须确认对应的快照已经不在**才可删（认领是「这个序号用掉了」的
 *   记录；快照还在时删认领，会让序号被二次分配 ⇒ **同 id 落盘 ⇒ 撞车**）。
 *   **水位记录不在这个家族里**（它不带 `.json` 后缀 ⇒ 这条清理**碰不到它**，
 *   协议 §2.5 ②「不得被清理流程删掉」）。
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
 * · **顺序：先重建占用关系，再回收**（协议 §S-12 裁定 3）——**占用记录的全量
 *   扫描**发生在**任何快照删除之前**（次序可机检）。
 * · **被占用的快照不删**（协议 §S-11 裁定 3）：删快照前**先查**
 *   `<root>/refs/<snapshotId>.json`——**存在 ⇒ 不删**。
 * · **两步法删除**：先 `rename` 成临时名，再 `unlink`。这样即使删到一半挂掉，
 *   留下的也不是一个「看起来有效」的半截文件——**快照文件被读时还要过摘要校验**
 *   （`host/index.js` 的 `store.load`），半截文件本来就该被拒，但**先改名**
 *   让「还在原地」的窗口更短。
 * · **只动本实例根下这六家族**；**根不可得就不扫**（宁可不清理，不乱删）。
 * · **清理失败只记日志，不阻塞业务**（协议 §4.1 第 3 条）。
 *
 * ## 占用关系与「正在发送中的引用」
 *
 * 协议 §S-11 之前，这里明写「**不保护「正在发送中的引用」**」「**不引入引用
 * 计数**」。现在**多了占用记录这一层**（`refs/` 家族）：
 * · **有/无的凭证**：同一快照被引用两次**仍是一条**（**不引入引用计数**——
 *   这条纪律没变）。
 * · **保护的是「回收时不误删」**：被占用的快照**不删**。
 * · **它不是内核草稿的镜像**（协议 §S-12）：内核**从不告诉** source「引用被
 *   移除 / 草稿被放弃」（上游缺口 G1-8）⇒ 误删的后果是**该引用失效并被拒**
 *   （`snapshot_invalid`）＋ 可见错误，**不是错发**（§S-12 裁定 2）。
 *
 * @module xlink-official-chat/sweep
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { CONSTANTS, WATERMARK_FILE_NAME, isTempSnapshotName, makeSnapshotId } from './protocol.js';
import { createRefsStore } from './refs-store.js';

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
 * 一个目录里所有文件的**字节数**（**递归**；符号链接不跟）。
 *
 * **为什么递归**：快照按 **provider 目录层**组织（`snapshots/<providerId>/`），
 * 而协议 §S-16 裁定 1 的核算范围写的是 `snapshots/<providerId>/**`
 * ⇒ 一层平铺够用，但**递归**更不容易在目录形状变化时算漏
 * （而"算漏"会让总量读数偏小 ⇒ 该淘汰时不淘汰 ⇒ 强制点失效）。
 *
 * **为什么跳过符号链接**：符号链接可能指向目录环或实例根之外 ⇒ 跟进去会
 * 死循环、或把**别的**东西算进本实例的总量。`Dirent` 的 `isDirectory()`
 * 对符号链接恒为 false（它按 lstat 判定）⇒ 天然不跟。
 *
 * @param {string} dir - 目录。
 * @returns {number} 字节数；读不到就是 0。
 */
function dirBytes(dir) {
  let total = 0;
  let entries = [];
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    // **目录还没有** ⇒ 那一片是 0（不是错误）。
    return 0;
  }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      total += dirBytes(path);
      continue;
    }
    try {
      const st = statSync(path);
      if (st.isFile()) total += st.size;
    } catch {
      // **取不到就当它不存在**（可能刚被别的流程删了）。
    }
  }
  return total;
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
 * @param {number} [options.tmpResidualMs] - 临时残留阈值（默认 §4 的 `tmpResidualMs`）。
 * @param {number} [options.occupancyRetentionMs] - 占用保留期
 *   （默认 §4 的 `occupancyRetentionMs`）。
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
  occupancyRetentionMs = CONSTANTS.occupancyRetentionMs,
  providerId = 'ds',
  now = () => Date.now(),
  logger,
} = {}) {
  /**
   * **占用记录存储**（协议 §2.4 / §S-11 / §S-12）。
   *
   * **为什么清理器自己持有一份**：占用关系是「**回收前**必须先重建」的那份事实
   * （§S-12 裁定 3）⇒ 扫描与释放都住在同一处，**次序不可能被调乱**。
   */
  const refsStore = createRefsStore({ root, now, occupancyRetentionMs });
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
   * 清**正文快照**（过期 = 超过保留期，**且没被占着**）。
   *
   * ## 「被占着 ⇒ 不删」（协议 §S-11 裁定 3）
   *
   * 删除前**必须**查 `<root>/refs/<snapshotId>.json`：**存在 ⇒ 不删**。
   * 占用集合由**本轮更早的那次全量扫描**给出（§S-12 裁定 3：先重建占用关系，
   * 再回收）⇒ 这里**不重复扫盘**（也免得两次扫描之间状态漂移）。
   *
   * @param {Set<string>|string[]} [occupied] - 仍被占着的快照 id。
   * @returns {{scanned: number, removed: number}}
   */
  const sweepSnapshots = (occupied = []) => {
    const result = { scanned: 0, removed: 0 };
    const dir = join(root, 'snapshots', providerId);
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return result;
    }
    const held = occupied instanceof Set ? occupied : new Set(occupied);
    const cutoff = now() - retentionMs;
    for (const name of names) {
      if (!name.endsWith('.json')) continue;
      const path = join(dir, name);
      result.scanned += 1;
      // **被占着 ⇒ 不删**（先查引用、再删快照；这是**硬要求**）。
      if (held.has(name.slice(0, -'.json'.length))) continue;
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
        // **水位记录不在这个家族里**（协议 §2.5 ②「不得被清理流程删掉」）：
        // 它的文件名**不带 `.json` 后缀** ⇒ 上面那行已经把它挡住了；
        // 这里再点名一次，是为了让「水位不会被连带删」这条**在源码里看得见**
        // （将来谁把清理口径改成「删目录下所有文件」，这行就是拦它的）。
        if (name === WATERMARK_FILE_NAME) continue;
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
   * **核算「缓存总量」**（协议 §S-16 裁定 1）。
   *
   * - **计入**：`snapshots/<providerId>/**`（**逐个 provider 目录**求和 ——
   *   provider 是**目录层**）＋ `catalog-cache/**`（**平铺**：文件名 =
   *   `<provider>__<账号范围>__<查询词>.json`，**没有 provider 子目录**）。
   * - **不计入**：`seq-claims/`、`refs/`、`logs/`、`sweeps/`。
   *
   * **为什么按「实例根下所有 provider 目录」求和**：上限是
   * **512 MB / 实例**（P1 §4）⇒ 算的是**这个实例**的总量，不是某个 provider 的。
   * 少算别的 provider ⇒ 该淘汰时没淘汰 ⇒ 强制点失效。
   *
   * @returns {{totalBytes: number, snapshotsBytes: number, cacheBytes: number}}
   */
  const measureCacheTotalBytes = () => {
    const result = { totalBytes: 0, snapshotsBytes: 0, cacheBytes: 0 };
    // **快照：逐个 provider 目录**求和。
    const snapshotsRoot = join(root, 'snapshots');
    let providers = [];
    try {
      providers = readdirSync(snapshotsRoot, { withFileTypes: true });
    } catch {
      providers = [];
    }
    for (const entry of providers) {
      if (!entry.isDirectory()) continue;
      result.snapshotsBytes += dirBytes(join(snapshotsRoot, entry.name));
    }
    // **目录缓存：平铺**（一个目录，没有 provider 子目录）。
    result.cacheBytes = dirBytes(join(root, 'catalog-cache'));
    result.totalBytes = result.snapshotsBytes + result.cacheBytes;
    return result;
  };

  /**
   * **额外淘汰**（协议 §S-16 裁定 3①）：删掉 **TTL 内**的目录缓存。
   *
   * **这是本单唯一新增的删除动作**（常规清理只删 TTL 已过的缓存）。
   *
   * **为什么删它无损**：目录缓存**可重建**（5 分钟后自动重新生成）且
   * **不含正文**（P1 §5「目录页不含正文」）⇒ 删了只是多一次查询。
   *
   * **为什么再判一次 TTL**：常规清理已经删过 TTL 已过的 ⇒ 这里剩下的
   * 正常都是 TTL 内的；但**删不掉的**（权限/占用）会留在原地 ⇒ 再判一次
   * TTL ⇒ **只删「TTL 内」的那些**，不去动「本该由常规清理负责」的残留。
   *
   * **删除步法**：沿用**本文件既有的两步法**（先 `rename` 墓碑、再 `unlink`）——
   * 与常规清理删缓存用的是同一套（`sweepCache` 走 `removeInTwoSteps`）
   * ⇒ 一致，且不会出现「删到一半留下一个看起来有效的缓存」。
   *
   * @returns {{scanned: number, removed: number}}
   */
  const evictFreshCatalogCache = () => {
    const result = { scanned: 0, removed: 0 };
    const dir = join(root, 'catalog-cache');
    let names = [];
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
      // **TTL 已过的不是本步该删的**（那是常规清理的事）。
      if (st.mtimeMs < cutoff) continue;
      const rel = labelOf(path);
      if (removeInTwoSteps(path)) result.removed += 1;
      else logger?.warn?.(`official-chat: 缓存总量淘汰失败：${rel} 删不掉（已保留，未假装删掉）`);
    }
    return result;
  };

  /**
   * **核算 ＋ 额外淘汰**（协议 §S-16）。
   *
   * ## 时机（裁定 2）
   *
   * 排在「**占用关系全量扫描 ＋ 常规清理**」**之后** ⇒ 核算读到的是
   * 「**本轮该回收的都回收了之后**」的总量 ⇒ 判定才准。
   *
   * ## 淘汰顺序（裁定 3）
   *
   * ① **TTL 内的目录缓存也删**（唯一新增的删除动作）；
   * ② **被占用的快照不删**（硬约束）；
   * ③ **未到保留期（7 天）的快照不删**。
   *
   * ⇒ **本步不删任何快照**：被占用的不许删（②）、未到期的不许删（③）；
   * 而**已到期且未被占用**的快照**在常规清理里已经删掉了** ⇒ 额外淘汰
   * 能动的只有**目录缓存**。⇒ 协议裁定 7（「最旧」按 `capturedAt` 判）
   * 在本单**没有落点**（它是为「日后改成硬上限」预留的判定口径）。
   *
   * ## 上限的诚实读法（裁定 5）
   *
   * 512 MB 是「**尽力而为**的上限」：删到底仍超 ⇒ **只记日志**
   * （不抛异常、不新增用户可见路径、不扩 13 码闭集）⇒ 该情形登记为 G3-5。
   *
   * ## 留痕的要素（裁定 5 明列的五项）
   *
   * **超预算就留痕**（不是「删到底仍超才留痕」）—— 否则「超预算但已回落」
   * 这一路**没有任何留痕** ⇒ 事后查不到「曾经超过」。一条留痕里含齐：
   * **核算总量**／**预算值**／**是否超预算**／**淘汰了什么**（数量＋类别）／
   * **淘汰后总量**。⇒ `totalBytes`（核算总量）与 `totalBytesAfterEviction`
   * （淘汰后总量）是**两个要素**，不合并成一个字段。
   *
   * @returns {object} 核算结果。
   */
  const applyCacheBudget = () => {
    const budgetBytes = CONSTANTS.cacheTotalBudgetBytes;
    const before = measureCacheTotalBytes();
    const state = {
      // **核算总量**（裁定 2：清理后、淘汰前）＋ 它的两部分分解。
      totalBytes: before.totalBytes,
      snapshotsBytes: before.snapshotsBytes,
      cacheBytes: before.cacheBytes,
      // **预算值** ＋ **是否超预算**（＝ 触发额外淘汰的判据）。
      budgetBytes,
      over: before.totalBytes > budgetBytes,
      // **淘汰了什么**（数量；类别恒为「目录缓存」——本单唯一的删除动作）。
      evictedCacheFiles: 0,
      // **淘汰后总量** ＋ 淘汰后是否仍超（**不得**据此声称已被强制在上限内）。
      totalBytesAfterEviction: before.totalBytes,
      stillOver: false,
    };
    if (!state.over) return state;
    let evicted = 0;
    try {
      evicted = evictFreshCatalogCache().removed;
    } catch (error) {
      // **淘汰失败只记日志，不抛异常**（§S-16 裁定 5 ＋ P1 §4.1 第 3 条）。
      logger?.warn?.(`official-chat: 缓存总量淘汰失败：${String(error?.message ?? error)}`);
      // **失败也要留痕**（五要素齐全）——「没删成」本身就是排查信息。
      logger?.warn?.(
        `official-chat: 缓存总量超预算（核算总量 ${before.totalBytes} 字节 > 预算 ${budgetBytes} 字节）——已淘汰目录缓存 0 个（失败：${String(error?.message ?? error)}）；淘汰后总量 ${before.totalBytes} 字节（尽力而为，见 §S-16 裁定 5）`,
      );
      return state;
    }
    state.evictedCacheFiles = evicted;
    const after = measureCacheTotalBytes();
    state.totalBytesAfterEviction = after.totalBytes;
    state.stillOver = after.totalBytes > budgetBytes;
    // **留痕**（五要素齐全）⇒ 供判据断「有留痕」、供事后排查。
    logger?.warn?.(
      `official-chat: 缓存总量超预算（核算总量 ${before.totalBytes} 字节 > 预算 ${budgetBytes} 字节）——已淘汰目录缓存 ${evicted} 个；淘汰后总量 ${after.totalBytes} 字节${
        state.stillOver
          ? '（仍超预算：已按协议保留未到期快照与被占用的快照——尽力而为，见 §S-16 裁定 5）'
          : '（已回到预算内）'
      }`,
    );
    return state;
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
   * **次序是硬要求**（协议 §S-12 裁定 3「先重建占用关系，再回收」）：
   * ① **占用记录的全量扫描**（＝这一轮的占用关系）
   * → ② **快照回收**（按①的占用关系判：被占着的跳过）
   * → ③ **R2 兜底释放**（过期的占用记录删掉 ⇒ **从下一轮起**不再保护）
   * → ④ 其余家族。
   *
   * **为什么 ① 必须在 ② 之前**：不先重建占用关系，就不知道哪些快照还在被引用
   * ⇒ 回收会误删 ⇒ 那个引用失效并被拒（§S-12 裁定 2 的降级口径）。
   *
   * **为什么 ③ 在 ② 之后**：§S-11③「存在 ⇒ 不删」——**这一轮扫到的占用记录
   * （含已过期的）都算数**；过期记录被释放之后，**从下一轮起**它那份快照才可
   * 回收（详见 `host/refs-store.js` 的 `releaseExpired` 注释）。
   *
   * **⑤ 核算 ＋ 额外淘汰**（协议 §S-16）：排在**常规清理之后** ⇒ 核算读到的是
   * 清理后的总量；超预算时的额外淘汰**只删 TTL 内的目录缓存**（快照一律不删：
   * 被占用的不删、未到期的不删）。
   *
   * **只动本实例根下那六家族**；根不可得 ⇒ **不扫**（宁可不清理，不乱删）。
   *
   * @returns {{skipped: boolean, reason?: string, refs?: object, snapshots?: object, tmp?: object, claims?: object, cache?: object, log?: object, budget?: object}}
   */
  function sweep() {
    if (root === null || root === undefined) {
      return { skipped: true, reason: '落盘根不可得（内核没给实例 home）——不扫' };
    }
    // ① **先重建占用关系**（协议 §S-12 裁定 3）——**全量扫描**发生在任何
    //    快照删除之前（次序可机检）。
    const occupancy = refsStore.scan();
    // ② **再回收快照**：按①的占用关系判——**被占着的跳过**（含读坏的那些）。
    const snapshots = sweepSnapshots(occupancy.occupied);
    // ③ **R2 兜底释放**：过了占用保留期的占用记录视为失效并删
    //    （**由清理流程负责**，不是释放入口负责）。
    const refs = refsStore.releaseExpired(occupancy);
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
    // ⑤ **核算 ＋ 额外淘汰**（协议 §S-16）：排在「占用关系全量扫描 ＋ 常规清理」
    //    **之后** ⇒ 核算读到的是「本轮该回收的都回收了之后」的总量。
    const budget = applyCacheBudget();
    return { skipped: false, refs, snapshots, tmp, claims, cache, log, budget };
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
