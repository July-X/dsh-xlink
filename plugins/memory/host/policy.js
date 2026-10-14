/**
 * 记忆注入的**纯策略**：从这一步的消息里取查询、决定要不要刷新、结果怎么进缓存。
 *
 * 与 `index.js` 分开的理由和 ponytail 的 `skills.js` 一样——这一半没有外部依赖，
 * `npm run check` 的冒烟测试能直接 import 并把状态机真的跑一遍。写在装配层里的话，
 * 测试只能去读源码文本，于是「第 4 次刷新照样注入」这类错会一路绿灯到真机。
 *
 * @module xlink-memory/policy
 */

/**
 * 每个会话最多注入几次。
 *
 * 内核的运行上下文快照是**内容一变就追加一条**、旧快照留到压缩为止（`dsh-agent-loop`
 * 的 `RuntimeContextProjection`），所以次数必须由我们这边封顶：一个每回合都换内容
 * 的记忆块会凭空把长会话撑大几百 KB。首次注入算第 1 次。
 */
export const MAX_INJECTIONS_PER_SESSION = 3;

/**
 * 取这一步领取到的消息里**最后一条用户消息**的纯文本。
 *
 * 只认 `source.kind === 'user'`：运行上下文、工作区指令、技能目录都是 user 角色
 * 的注入消息，把它们当查询会得到「拿自己的注入去检索」的回声。
 *
 * @param {Array} messages - `agent/pre-step` 领取到的消息。
 * @returns {string}
 */
export function userText(messages) {
  if (!Array.isArray(messages)) return '';
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message?.source?.kind !== 'user') continue;
    const blocks = Array.isArray(message.content) ? message.content : [];
    const text = blocks
      .filter((block) => block?.type === 'text' && typeof block.text === 'string')
      .map((block) => block.text)
      .join('\n')
      .trim();
    if (text) return text;
  }
  return '';
}

/**
 * 这一步要不要去检索。
 *
 * 同一个回合只检索一次（pre-step 每步都跑）；超过会话上限就不再刷新——缓存里那份
 * 继续生效，直到用户删除/停用/切目录把它清掉。
 *
 * @param {{turn: number, injections: number}} state
 * @param {number} turn
 * @param {number} [maxInjections]
 * @returns {boolean}
 */
export function shouldRefresh(state, turn, maxInjections = MAX_INJECTIONS_PER_SESSION) {
  if (state.turn === turn) return false;
  return state.injections < maxInjections;
}

/**
 * 检索成功后的缓存。文本没变就不算一次注入（内核也不会因此多一条快照）。
 *
 * @param {{turn: number, injections: number}} state
 * @param {number} turn
 * @param {string} text
 * @returns {{turn: number, text: string, injections: number}}
 */
export function applyResult(state, turn, text) {
  const next = typeof text === 'string' ? text : '';
  return {
    turn,
    text: next,
    injections: state.injections + (next === state.text ? 0 : 1),
  };
}

/**
 * 检索失败后的缓存：**清空**，不沿用上一回合的内容。
 *
 * 沿用会让「服务已经关掉/删掉了那条」的会话继续看到旧记忆——用户以为停用生效了，
 * 而模型还在照着它做事。空缓存 + 回合标记保证本回合不再重试，下一回合再试一次。
 *
 * @param {{turn: number, injections: number}} state
 * @param {number} turn
 * @returns {{turn: number, text: string, injections: number}}
 */
export function applyFailure(state, turn) {
  return { turn, text: '', injections: state.injections };
}

/**
 * 这个目录对应的内核工作区 id；拿不到就回空串（后端按路径兜底匹配）。
 *
 * **失败一律静默降级**：目录不存在时 `resolveByPath` 会 reject（它内部要做
 * `realpath`），服务还没就绪时注册表还是 null。两者都不该让一次检索失败——
 * 路径匹配本来就是可用的那一条，id 只是把「项目改名/换盘后仍能对上」这一项补上。
 *
 * @param {object|null} registry - 按需注入拿到的工作区注册表。
 * @param {string} path - 有效工作目录。
 * @returns {Promise<string>}
 */
export async function resolveWorkspaceId(registry, path) {
  if (!registry || typeof registry.resolveByPath !== 'function' || !path) return '';
  try {
    const workspace = await registry.resolveByPath(path);
    return typeof workspace?.id === 'string' ? workspace.id : '';
  } catch {
    return '';
  }
}
