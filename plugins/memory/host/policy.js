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
 * 除回合预算之外，**因库变化**而允许的额外刷新次数（每会话）。
 *
 * 为什么需要这一档：回合预算挡的是「每回合的查询都不一样、正文必然不同」——没有它，
 * 一段长会话会攒下几十份内核快照。但用户在会话中途批准一条候选、删掉一条、或把开关
 * 关掉时，注入块**必须**跟着变，否则他会以为「批准了却没生效」。两件事的判据不同：
 * 前者看回合，后者看**数据代次**（`RetrievedContext.generation`，每次成功写入 +1）。
 *
 * 取 2 是给「批准一条 + 删一条」留的位置。最坏情况下一个会话 3 + 2 = 5 份快照，而用户
 * 的库编辑是低频事件；真要一路改下去，那也该新开一个会话。
 */
export const MAX_LIBRARY_REFRESHES_PER_SESSION = 2;

/**
 * 每会话**累计**注入字节的上限（设计稿 §3.1：累计不超过 36 KiB）。
 *
 * 次数封顶挡不住「每次都很长」：单次 12 KiB × (3 + 2) 次最坏是 60 KiB，而内核会为每一份
 * 不同的快照留一条历史消息到压缩为止。次数与字节两个闸都要有——只封一个都不够，这正是
 * 设计稿 §3.1 那句话的意思。
 */
export const MAX_TOTAL_BYTES_PER_SESSION = 36 * 1024;

/** UTF-8 字节数（`String.length` 是 UTF-16 码元，中文按它算会少一半）。 */
function byteLength(text) {
  return Buffer.byteLength(text, 'utf8');
}

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
  if (state.bytes >= MAX_TOTAL_BYTES_PER_SESSION) return false;
  return state.injections < maxInjections;
}

/**
 * 回合预算用完之后，这一步要不要去探一次「库的代次变了吗」。
 *
 * **判据只能是代次，不能是「取回来的文本不一样」**：查询就是用户这一回合的消息，每回合
 * 都不一样，文本变是常态——拿它当判据等于把回合预算整个绕过去，而那个预算存在的理由
 * （每份不同的快照都会在内核里留一条 user 消息）恰恰就是这件事。
 *
 * @param {{turn: number, injections: number}} state
 * @param {number} turn
 * @param {number} [maxLibrary]
 * @returns {boolean}
 */
export function shouldProbeLibrary(state, turn, maxLibrary = MAX_LIBRARY_REFRESHES_PER_SESSION) {
  if (state.turn === turn) return false;
  if (state.bytes >= MAX_TOTAL_BYTES_PER_SESSION) return false;
  if (state.injections < MAX_INJECTIONS_PER_SESSION) return false;
  return state.injections - MAX_INJECTIONS_PER_SESSION < maxLibrary;
}

/**
 * 探到的代次与上次记下的不同 → 库被改过（批准 / 删除 / 停用 / 编辑 / 关开关）。
 *
 * 拿不到代次（响应缺字段）时一律回 false：**不知道就不动**，宁可这一回合不刷新，也不要
 * 把「每回合都换的正文」当成库变化注入进去。
 *
 * @param {{revision: number|null}} state
 * @param {unknown} generation
 * @returns {boolean}
 */
export function libraryChanged(state, generation) {
  return typeof generation === 'number' && state.revision !== generation;
}

/**
 * 探过了但库没变（或探失败）：只记代次与回合，**正文与注入计数都不动**。
 *
 * @param {{turn: number, text: string, injections: number, workspace: string, revision: number|null}} state
 * @param {number} turn
 * @param {unknown} generation
 * @returns {object}
 */
export function noteProbe(state, turn, generation) {
  return {
    ...state,
    turn,
    revision: typeof generation === 'number' ? generation : state.revision,
  };
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
  return {
    turn: -1,
    text: '',
    injections: state.injections,
    bytes: state.bytes ?? 0,
    workspace,
    // 代次与累计字节都是**会话级**的，与目录无关，换目录不必丢。
    revision: state.revision,
  };
}

/**
 * 一步开始时的状态推进：**先换目录（立刻生效），再判要不要检索**。
 *
 * 这两件事的顺序、以及「换目录的结果必须写回」是整条链路里唯一会被静默做坏的地方：
 * 只在「能重新检索」的那条分支里写回时，「预算用满 + 换目录」会让上一个项目的正文
 * 一路留在这个会话里——正是 [`retarget`] 要挡的那件事，而纯函数测试照样全绿
 * （它们测的是 `retarget` 本身，不是接线）。所以这一步单独成一个纯函数并被钉住。
 *
 * @param {{turn: number, text: string, injections: number, workspace: string}} state
 * @param {string} workspace - 这一步的有效工作目录。
 * @param {number} turn
 * @param {number} [maxInjections]
 * @returns {{state: object, refresh: boolean}}
 */
export function beginStep(state, workspace, turn, maxInjections = MAX_INJECTIONS_PER_SESSION) {
  const moved = retarget(state, workspace);
  return { state: moved, refresh: shouldRefresh(moved, turn, maxInjections) };
}

/**
 * 检索成功后的缓存。文本没变就不算一次注入（内核也不会因此多一条快照）。
 *
 * @param {{turn: number, text: string, injections: number, workspace: string}} state
 * @param {number} turn
 * @param {string} text
 * @returns {{turn: number, text: string, injections: number, workspace: string}}
 */
export function applyResult(state, turn, text, generation) {
  const next = typeof text === 'string' ? text : '';
  const changed = next !== state.text;
  return {
    turn,
    text: next,
    injections: state.injections + (changed ? 1 : 0),
    // 累计注入字节：内核为每一份**不同**的快照留一条历史消息，所以按「换过几次」算，
    // 内容没变的那一次不占额度（否则重复渲染会把额度吃光而什么都没注入）。
    bytes: (state.bytes ?? 0) + (changed ? byteLength(next) : 0),
    workspace: state.workspace,
    // 记下这次正文对应的数据代次：`shouldProbeLibrary` 那条路靠它判「库变了吗」。
    revision: typeof generation === 'number' ? generation : state.revision,
  };
}

import { maskSecrets } from './mask.js';

/**
 * 提取器版本。**换版本就是换任务键**（旧结果不许覆盖新结果，§8-6）。
 *
 * **只有一份，在 `extract.js`**（那里离提示词与输出 schema 最近，改它们时最自然的落点是那儿）。
 * 这里曾经各写一份：改 `extract.js` 的提示词不会动到入队用的这份，壳按任务键写的
 * `source.revision` 也不会变——「换提取器 = 换任务键」就静默失效了（独立评审第 8 条）。
 */
export { EXTRACTOR_VERSION } from './extract.js';

/**
 * 「人类消息」的 source kind（§8-1「只安排有用户内容…的会话」）。
 *
 * 判据的依据是 `agent.inject()` 那条路：合成上下文带着**它们自己的** source kind
 * （`working-directory` / 工作区指令 / 技能 / cron 通知…），而不是 `user`——所以
 * 「一次纯注入的回合」不会被当成用户内容去排提取任务。
 */
export const HUMAN_SOURCE = 'user';

/** 只有正常结束的回合才排任务，理由见 `shouldScheduleExtraction`。 */
export const SCHEDULABLE_END = 'completed';

/**
 * 提取任务的会话状态。
 *
 * @typedef {{humanSeq: number, lastCursor: number}} ExtractState
 */

/** 初始状态（一个新会话一份）。 */
export function emptyExtractState() {
  return { humanSeq: 0, lastCursor: 0 };
}

/**
 * 记一条会话事件。只关心**真的人类消息**：它的序号是「这一段里有用户内容」的证据，
 * 也是后面判「有没有新内容」的基准。
 *
 * @param {object} state - 当前状态。
 * @param {object} event - `session/event` 的那一条。
 * @returns {object} 新状态（不改入参）。
 */
export function noteSessionEvent(state, event) {
  const current = state && typeof state === 'object' ? state : emptyExtractState();
  if (!event || typeof event !== 'object') return current;
  if (event.type !== 'user/message') return current;
  if (event.data?.source?.kind !== HUMAN_SOURCE) return current;
  const seq = Number(event.seq ?? 0);
  if (!seq) return current;
  return { ...current, humanSeq: seq };
}

/**
 * 这次 `turn/end` 要不要排一个提取任务。
 *
 * ① 回合**正常结束**（`completed`）——取消、出错、超长、被中断的回合都不排。**不会因此漏内容**：
 *    后面任何一次正常结束的回合，游标都比它们靠后，冻结的范围里自然包含那些事件；
 * ② 这段范围里**有真的人类消息**（见 `HUMAN_SOURCE`），并且是上一次排任务之后才出现的
 *    （`humanSeq > lastCursor`）——纯注入的回合不排，同一段内容也不重复排；
 * ③ `seq` 必须是个正整数：拿不到截止序号就没有「冻结输入」可言，宁可不排。
 *
 * 返回的 `cursor` 就是**要冻进任务键里的截止序号**（§8-1）。
 *
 * @param {object} state - `noteSessionEvent` 之后的状态。
 * @param {object} event - 刚收到的 `session/event`。
 * @returns {{schedule: boolean, cursor: number}}
 */
export function shouldScheduleExtraction(state, event) {
  const none = { schedule: false, cursor: 0 };
  if (!event || event.type !== 'turn/end') return none;
  if (event.data?.reason?.kind !== SCHEDULABLE_END) return none;
  const cursor = Number(event.seq ?? 0);
  if (!Number.isInteger(cursor) || cursor <= 0) return none;
  const current = state && typeof state === 'object' ? state : emptyExtractState();
  if (current.humanSeq <= current.lastCursor) return none;
  return { schedule: true, cursor };
}

/**
 * 这次 `session/event` 是不是一次**成功**的压缩收尾。
 *
 * 为什么要判「成功」而不是「有 `compaction/end` 就重置」：内核在**失败**时也会发 `end`，
 * 只是带上 `error` 字段（2026-10-15 真机实测，见「怎么验」那一节）——而这与设计稿当初的假设
 * （「失败或 end 追加失败时可能只有 `start`」）不一样。失败的压缩**没有丢掉任何快照**，
 * 那批记忆块还在上下文里；此时重置预算只会让下一次 pre-step 白取一次检索，而且如果这期间
 * 库里有别的记录命中了同样的查询，还会往同一段上下文里再塞一份。
 *
 * @param {object} event - 一条 `session/event`。
 * @returns {boolean}
 */
export function isCompactionDone(event) {
  if (!event || event.type !== 'compaction/end') return false;
  return !event.data?.error;
}

/**
 * 提取出来的候选该落成哪种记忆。
 *
 * **依据是壳自己的规则，不是我的偏好**：提交 `scope=project` 而没有绑定**已登记的工作区**时，
 * 壳会拒掉并给出明确理由（「项目记忆需要绑定一个已登记的工作区；没有工作区时请改存『通用偏好』」，
 * 2026-10-15 在真桥上实测）。所以一个没有工作区 id 的会话——比如在内核安装目录里跑起来的探针——
 * 只能落成通用偏好；硬报 `project` 只会让每一次提取都被拒。
 *
 * @param {string} workspaceId - 这一会话解析出来的工作区 id（空串表示没登记）。
 * @returns {'project' | 'general'}
 */
export function scopeForWorkspace(workspaceId) {
  return typeof workspaceId === 'string' && workspaceId.trim().length > 0 ? 'project' : 'general';
}

/**
 * 压缩之后的会话状态：**把注入预算清零**。
 *
 * 为什么清零不违背「每会话 3 次」那条硬规则：那条规则防的是「同一批快照越堆越多」，而压缩会把旧的
 * 运行上下文快照整段丢掉（`dsh-compaction-basic` 的 `compaction/end`）——旧快照没了，注入块也就
 * 没了，模型此刻看得到的东西里一条记忆都没有。不清零就等于「长会话压缩之后再也拿不到记忆」，而
 * 用户看到的是一段与短会话完全不同的行为（设计 §10 明确要求「成功压缩后能重新取得记忆」）。
 *
 * 保留 `workspace`（压缩不换目录，留着免得被当成换目录多清一次）与 `revision`（文档代次与会话无关）；
 * `turn` 置 -1，让本回合之后的下一步能重新检索。
 *
 * @param {{workspace: string, revision: number|null}} state
 * @returns {object}
 */
export function afterCompaction(state) {
  return {
    turn: -1,
    text: '',
    injections: 0,
    bytes: 0,
    workspace: state.workspace,
    revision: state.revision,
  };
}

/**
 * 这一步取回来的结果**怎么用**——整条链路里唯一会被静默做坏的决定，所以它是纯函数。
 *
 * 两种模式：`turn`（回合预算内）取到什么用什么；`probe`（预算用完之后）只在**数据代次
 * 变了**时才换正文。写成散在装配层里的两处 if 时，「文本变了就换」这种写法会一路绿到
 * 真机——它看着完全合理，实际把每会话三次的预算整个绕过去了（查询每回合都不同）。
 *
 * @param {object} state
 * @param {number} turn
 * @param {'turn'|'probe'} mode
 * @param {{ok: boolean, enabled?: boolean, text?: unknown, generation?: unknown}} outcome
 * @returns {{state: object, applied: boolean}}
 */
export function applyStep(state, turn, mode, outcome) {
  if (!outcome?.ok) {
    // 探测失败不动缓存（一次抖动不是「用户改了库」）；回合内那条路照旧清空。
    return {
      state: mode === 'probe' ? noteProbe(state, turn, state.revision) : applyFailure(state, turn),
      applied: false,
    };
  }
  if (mode === 'probe' && !libraryChanged(state, outcome.generation)) {
    return { state: noteProbe(state, turn, outcome.generation), applied: false };
  }
  const text = outcome.enabled ? String(outcome.text ?? '') : '';
  return { state: applyResult(state, turn, text, outcome.generation), applied: true };
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
  return {
    turn,
    text: '',
    injections: state.injections,
    bytes: state.bytes ?? 0,
    workspace: state.workspace,
    revision: state.revision,
  };
}

/**
 * 这条 agent 会话的有效工作目录。
 *
 * 用**有效**目录而不是冻结的 `session.header.cwd`：用户可以在会话中途换目录
 * （`working-directory/change`），按冻结值检索会把 A 项目的记忆带进 B 项目——
 * 而那正是这套东西要防的事。服务拿不到时退到冻结值，绝不让它抛。
 *
 * 放在这一层是为了**能单测**：它决定的是「子智能体拿哪个目录」——夹紧的关键一步。
 *
 * @param {object} ctx - 插件上下文（声明了 `workingDirectory`）。
 * @param {object} agent - 会话所属 agent。
 * @returns {string}
 */
export function workingDirectory(ctx, agent) {
  try {
    const value = ctx.workingDirectory?.get?.(agent.session);
    if (typeof value === 'string' && value) return value;
  } catch {
    // 老内核没有这个服务（或会话还没登记目录）：退到冻结的项目目录。
  }
  try {
    return agent.session?.header?.cwd || '';
  } catch {
    return '';
  }
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

/**
 * 这次调用该带的**项目坐标**：目录与工作区 id 一起算。
 *
 * **为什么两件事必须一起算**：设计 §5 承诺「子智能体**最多继承父级的读取范围**」，而内核那个
 * 子智能体工具的 `cwd` 是**模型可填的字符串**（`dsh-tool-subagent/lib/index.js:325`，只有
 * description / prompt 标了 required），`:396` 原样透传，全程没有越界校验——模型可以把子智能体
 * 指到另一个项目去。于是子智能体拿到的「自己的有效目录」**不一定**在父级范围内。
 *
 * 判据取**工作区身份**，不是路径前缀：模型把一段活拆到同一个仓库的 `packages/x` 是常态，那属于
 * 同一个工作区、照常跟手；「另一个工作区」才是另一个范围（2026-10-15 维护者拍板）。
 *
 * **判不准就收窄**：父级 spawn 时的 cwd 拿不到、或两边任一侧解析不出工作区 id，都按「不在父级
 * 范围内」算，只给通用偏好。收窄成**空坐标**而不是替成父级目录——替过去会把 A 项目的记忆送进 B
 * 项目的活里，那正是 G15 ② 要挡的反面；空坐标在壳那边只匹配通用记录。
 *
 * @param {object} ctx - 插件上下文（声明了 `workingDirectory`）。
 * @param {object|null} registry - 按需注入拿到的工作区注册表。
 * @param {object} agent - 发起这次调用的 agent（子智能体给子智能体自己）。
 * @returns {Promise<{workspace: string, workspaceId: string}>}
 */
export async function memoryScope(ctx, registry, agent) {
  const workspace = workingDirectory(ctx, agent);
  const workspaceId = await resolveWorkspaceId(registry, workspace);
  if (await withinParentScope(registry, agent, workspaceId)) return { workspace, workspaceId };
  return { workspace: '', workspaceId: '' };
}

/**
 * 这个 agent 的工作区是否落在父级范围内。**不是子智能体就直接放行**——这条只管子智能体。
 *
 * 父级的范围从**子会话 header 里带过来的 `cwd`** 取：内核 `childSessionMeta` 建子会话时从
 * `parentHeader.cwd` 抄了一份（`dsh-subagent/lib/index.js:353`）。所以这一步既不需要
 * `agents` 服务，也不需要父会话还活着——那是这条最初以为「要真机才能定」时挡在前面的东西。
 *
 * @param {object|null} registry - 工作区注册表。
 * @param {object} agent - 发起这次调用的 agent。
 * @param {string} workspaceId - 这个 agent 自己目录解析出的工作区 id。
 * @returns {Promise<boolean>}
 */
async function withinParentScope(registry, agent, workspaceId) {
  const header = agent?.session?.header;
  if (header?.origin !== 'subagent' && typeof header?.parentSession !== 'string') return true;
  const parentId = await resolveWorkspaceId(registry, header.cwd);
  return Boolean(workspaceId) && workspaceId === parentId;
}
