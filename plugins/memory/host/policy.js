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
 * 送进桥的查询文本上限（字符）。
 *
 * 桥的请求头缓冲是固定 **8 KB**，而 `GET` 的查询串连同请求头都算在里面。中文经
 * `URLSearchParams` 编码后一个字符变 9 个字节（3 字节 UTF-8 × `%XX`），三倍膨胀——
 * 实测一条 1000 字的中文消息会让请求行到 9216 字节，直接越界。那之后
 * `read_request` 返回「请求头不完整」，**一个字节的响应都不写**就关连接，插件拿到
 * `socket hang up`，于是这一回合静默地没有记忆，而插件日志在这个内核里一行都不出
 * （设计稿 §9 的实测结论），用户与面板都看不见任何提示。
 *
 * **取尾部而不是头部**：最靠近当前要求的那几句才是检索要的。
 */
export const MAX_QUERY_CHARS = 500;

/**
 * 砍掉两端落单的代理。
 *
 * `slice` 按 **UTF-16 码元**切，emoji 与部分生僻汉字占 2 个码元，切点落在高/低代理
 * 之间就会留下半个。`URLSearchParams` 不会因此抛错（WHATWG 的序列化器把它替换成
 * U+FFFD，不像老的 `encodeURIComponent` 会抛 `URIError`），所以症状是**静默**的：
 * 那一个字符在检索时对不上，涉及它的关键词再也匹配不到。
 *
 * 这里取的是**尾部**，所以切出来只可能是「开头的落单低代理」（高代理被砍掉了）；
 * 末尾那一位一并检查，是因为拼接多个文本块时输入本身就可能已经带着半个。
 *
 * 砍掉那一个码元即可：少一个字词匹配，总好过多一个永远匹配不上的字符。
 *
 * @param {string} text
 * @returns {string}
 */
function trimStraySurrogates(text) {
  if (!text) return text;
  let out = text;
  // 末尾落单只可能是**高**代理：低代理落在末尾时它的搭档高代理就在前一位，是好的。
  if (isHighSurrogate(out.charCodeAt(out.length - 1))) out = out.slice(0, -1);
  // 开头落单只可能是**低**代理，同理。
  if (isLowSurrogate(out.charCodeAt(0))) out = out.slice(1);
  return out;
}

function isHighSurrogate(code) {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code) {
  return code >= 0xdc00 && code <= 0xdfff;
}

/**
 * 取这一步领取到的消息里**最后一条用户消息**的纯文本，按 [`MAX_QUERY_CHARS`] 截尾。
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
    if (text) return trimStraySurrogates(text.slice(-MAX_QUERY_CHARS));
  }
  return '';
}

/**
 * 这一步要不要去检索。
 *
 * 同一个回合只检索一次（pre-step 每步都跑）；超过会话上限就不再刷新——缓存里那份
 * 会一直生效，**直到这个会话的目录变了**（见 [`retarget`]）。
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
 * 会话中途换了工作目录时的处理：**清空缓存**。
 *
 * 这是「A 项目保存的记忆不进入 B 项目」在运行侧的最后一道。少了它，上一个项目的
 * 正文会一路留在注入块里：用户在内核里换目录是支持的（`working-directory/change`，
 * 设计稿 §3.1 第 5 条），而三条清缓存的触发里只有这一条在代码里真的存在——
 * 「用户删除/停用」同样依赖它，否则预算用完之后删掉的记忆照样每轮注入。
 *
 * **预算不归零**，这是想清楚了才这么写的：`injections` 是**会话级**的硬上限
 * （[`MAX_INJECTIONS_PER_SESSION`]），存在的理由是内核会为每一份**不同**的快照留
 * 一条 user 消息到压缩为止。归零等于把「换目录」变成一个绕开它的开关——来回切五次
 * 就是十五份快照，而用户对这件事毫无感知。
 *
 * 代价要说清：预算用完之后再换目录，这个会话就**一段记忆都不注入**了。这是**对的方向**
 * ——宁可少注入，也不要把 A 的记忆送进 B。新开会话即恢复。
 *
 * @param {{turn: number, text: string, injections: number, workspace: string}} state
 * @param {string} workspace - 这一步的**有效**工作目录。
 * @returns {{turn: number, text: string, injections: number, workspace: string}}
 */
export function retarget(state, workspace) {
  if (state.workspace === workspace) return state;
  return { turn: -1, text: '', injections: state.injections, workspace };
}

/**
 * 检索成功后的缓存。文本没变就不算一次注入（内核也不会因此多一条快照）。
 *
 * @param {{turn: number, text: string, injections: number, workspace: string}} state
 * @param {number} turn
 * @param {string} text
 * @returns {{turn: number, text: string, injections: number, workspace: string}}
 */
export function applyResult(state, turn, text) {
  const next = typeof text === 'string' ? text : '';
  return {
    turn,
    text: next,
    injections: state.injections + (next === state.text ? 0 : 1),
    workspace: state.workspace,
  };
}

/**
 * 检索失败后的缓存：**清空**，不沿用上一回合的内容。
 *
 * 沿用会让「服务已经关掉/删掉了那条」的会话继续看到旧记忆——用户以为停用生效了，
 * 而模型还在照着它做事。空缓存 + 回合标记保证本回合不再重试，下一回合再试一次。
 *
 * @param {{turn: number, text: string, injections: number, workspace: string}} state
 * @param {number} turn
 * @returns {{turn: number, text: string, injections: number, workspace: string}}
 */
export function applyFailure(state, turn) {
  return { turn, text: '', injections: state.injections, workspace: state.workspace };
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
