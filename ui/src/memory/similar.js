/**
 * 记忆库里「可能是同一条的不同说法」的前端启发式。
 *
 * ## 为什么在 UI 侧算
 *
 * 跨进程去重（`src-tauri/src/memory/duplicates.rs`）只认正文精确指纹：判「意思一样、写法
 * 不同」需要模型比对，而模型只在提取那一次拿到 6 条相关记忆，跨会话的复述照样会漏。
 * 本机 36 条记忆里已经出现 4 组近义重复（同一会话 67 分钟内生成 3 条「暗色主题方案 B」），
 * 而列表本来就把整份库读到了前端（`memory_list` 一次给全）：在已加载的数据上算一遍，
 * 不用新接口、不改库、不动跨进程判据。
 *
 * ## 纪律
 *
 * 1. **只提示，不合并、不删除**：误判的代价只是多一个提示；自动删会让用户写下的东西
 *    静默消失（与 `duplicates.rs` 拒收近似重复同一条理由）。
 * 2. **同范围才比**：项目记忆只在同一项目里比，通用偏好只与通用偏好比——把两个项目的
 *    同一条约定标成重复，比漏报更糟。
 * 3. **阈值按真实数据标定**：标题 3-gram Dice ≥ 0.45，或标题中等相似（≥ 0.25）且关键词
 *    2-gram Dice ≥ 0.6。用 2026-11-15 本机 36 条记忆回放：暗色主题 3 条、MiniMax 手写体
 *    2 条命中；「判定不了的问题」那对只共享一句套话前缀，抓到它会连带 6 对误报，故不追。
 *    不相关对（「多内核 UI 总原则」vs「任务通知」等）不误报。
 *
 * @module xlink-memory/similar
 */

/** 标题字符 3-gram Dice 直接命中。 */
export const SIMILAR_TITLE = 0.45;
/** 标题低于这个值就不再靠关键词兜底：太不像的两个标题，关键词撞上也不是同一件事。 */
export const SIMILAR_TITLE_FLOOR = 0.25;
/** 关键词字符 2-gram Dice。关键词是模型挑出来的词，短，所以用 2-gram。 */
export const SIMILAR_KEYWORDS = 0.6;

/** 归一化：小写、去掉空白与标点（「方案 B」与「方案B」要算同一个词）。 */
export function normalizeText(text) {
  return String(text ?? '').toLowerCase().replace(/[\s\p{P}]/gu, '');
}

/** 字符 n-gram 集合。 */
function grams(text, n) {
  const chars = [...normalizeText(text)];
  const set = new Set();
  for (let index = 0; index + n <= chars.length; index += 1) {
    set.add(chars.slice(index, index + n).join(''));
  }
  return set;
}

/** Dice 系数：交集的两倍 / 两边大小之和。空串或过短的文本给 0。 */
export function dice(a, b, n = 3) {
  const left = grams(a, n);
  const right = grams(b, n);
  if (!left.size || !right.size) return 0;
  let shared = 0;
  for (const gram of left) {
    if (right.has(gram)) shared += 1;
  }
  return (2 * shared) / (left.size + right.size);
}

/** 与 `query.rs::scope_matches_fields` 同一套可见性：通用只对通用，项目按 id 或路径。 */
function sameScope(left, right) {
  if (left.scope === 'general' || right.scope === 'general') {
    return left.scope === right.scope;
  }
  return Boolean(
    (left.workspace_id && left.workspace_id === right.workspace_id) ||
      (left.workspace_path && right.workspace_path && left.workspace_path === right.workspace_path)
  );
}

/** 只有生效中的普通记忆参与：交接 / 经验是另一张表，停用的不该被劝删。 */
function comparable(row) {
  return Boolean(
    row &&
      row.id &&
      row.state === 'accepted' &&
      (row.kind ?? 'memory') === 'memory'
  );
}

/**
 * 两条记忆看起来是不是同一条的不同说法。
 *
 * @param {object} left - 一条记忆（`memory_list` 的行）。
 * @param {object} right - 另一条。
 * @returns {boolean}
 */
export function looksSimilar(left, right) {
  if (!comparable(left) || !comparable(right) || left.id === right.id) return false;
  if (!sameScope(left, right)) return false;
  const title = dice(left.title, right.title);
  if (title >= SIMILAR_TITLE) return true;
  if (title < SIMILAR_TITLE_FLOOR) return false;
  return dice((left.keywords || []).join(','), (right.keywords || []).join(','), 2) >= SIMILAR_KEYWORDS;
}

/**
 * 与这条相似的其它记忆（保持传入顺序）。
 *
 * @param {object} record - 当前这条。
 * @param {object[]} records - 整份库（未筛选的那份）。
 * @returns {object[]}
 */
export function similarPeers(record, records) {
  return (Array.isArray(records) ? records : []).filter((row) => looksSimilar(record, row));
}

/**
 * 每条记忆的相似条数（id -> 数量）。面板用它给行挂提示；O(n²) 在几百条量级可接受。
 *
 * @param {object[]} records - 整份库。
 * @returns {Map<string, number>}
 */
export function similarCounts(records) {
  const rows = (Array.isArray(records) ? records : []).filter(comparable);
  const counts = new Map();
  for (let i = 0; i < rows.length; i += 1) {
    for (let j = i + 1; j < rows.length; j += 1) {
      if (!looksSimilar(rows[i], rows[j])) continue;
      counts.set(rows[i].id, (counts.get(rows[i].id) || 0) + 1);
      counts.set(rows[j].id, (counts.get(rows[j].id) || 0) + 1);
    }
  }
  return counts;
}
