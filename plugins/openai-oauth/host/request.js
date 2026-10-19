/**
 * 请求与流的转换（设计 §8.1 / §8.3；开发计划 §7）。
 *
 * 纯函数、零依赖：`buildEnvelope` 把 dsh 的 GenerateOptions 转成桥接
 * `/v1/responses` 信封；`pumpStream` 把桥接的 NDJSON 事件流转成 dsh 的
 * StreamChunk。两头的**协议形状**与内核 `dsh-llm` 类型一一对应
 * （GenerateOptions / StreamChunk / ReplayEnvelope，P0 调查 §2）。
 *
 * 纪律（设计 §8.1）：
 * - 调用方显式配置了套餐路线无法执行的参数（temperature / maxTokens /
 *   stop）→ 抛错拒绝，不静默丢弃；
 * - 「模型默认」= 省略 reasoning 字段，绝不发送 auto/medium 之类的替身；
 * - 工具名转换可逆（模型回的 function_call 名要能映射回 dsh 工具名）；
 * - 终止分类由桥接包络承载（completed/failed/incomplete），EOF 不算成功；
 * - 用量按内核的**互斥桶**口径上报（缓存输入从 `inputTokens` 里减出来，
 *   见 `mapTokenUsage`）；额度类失败标出路由级 `QUOTA`（见
 *   `isQuotaFailureDetail`），由适配器层翻成内核 `LlmError`；
 * - 会话身份映射成稳定的 `prompt_cache_key`（见 `promptCacheKeyFor`）：
 *   不发这个键时上游会为每次请求生成随机键，请求散落在不同缓存分片上，
 *   同一段前缀只能靠运气命中。压缩调用带同一个键（它重放的就是同一段
 *   对话前缀），会话标题这类一次性小请求不带。
 */

/** 套餐路线无法执行、调用方却显式给出了的参数 → 拒绝（§8.1）。 */
const UNSUPPORTED_GENERATION_FIELDS = [
  ['temperature', '采样温度'],
  ['maxTokens', '输出上限'],
  ['stop', '停止序列'],
];

const TOOL_NAMESPACE = 'dsh';

/** 工具名 → 路线合法名（`[A-Za-z0-9_-]`）；映射表在适配器会话内持有，可逆。 */
export function toProviderToolName(name, mapping) {
  const safe = name.replace(/[^A-Za-z0-9_-]/g, '_');
  mapping.set(safe, name);
  return safe;
}

export function fromProviderToolName(name, mapping) {
  return mapping.get(name) ?? name;
}

/**
 * 组装 `/v1/responses` 信封。
 *
 * @param options dsh 的 GenerateOptions（只取用到的字段）
 * @param catalog 桥接目录视图：{ revisionCombined, entries }（combined
 *        形如 "rev|capN"，由桥接拼好，Host 原样回传做一致性校验）
 * @param client {{ hostId?: string }}
 * @returns {{ model, catalogRevision, reasoningEffort?, payload }}
 */
export function buildEnvelope(options, catalog, imageParts = new Map()) {
  const entry = catalog.entries.find((item) => item.id === options.model);
  if (entry === undefined) {
    const error = new Error(`模型 ${options.model} 不在当前账号目录；请刷新模型列表后重选`);
    error.code = 'MODEL_NOT_IN_CATALOG';
    throw error;
  }
  for (const [field, label] of UNSUPPORTED_GENERATION_FIELDS) {
    if (options[field] !== undefined) {
      const error = new Error(
        `套餐路线不支持配置${label}（${field}）；该限制无法执行，已拒绝请求（不会静默忽略）`,
      );
      error.code = 'FIELD_NOT_ALLOWED';
      throw error;
    }
  }

  const payload = { model: options.model };
  const input = [];
  const toolMapping = new Map();

  // 系统指令：首条 system 的文本进 instructions（设计 §7「转换为该路线
  // 接受的指令或 developer 消息」；Responses 用 instructions）。
  let instructions;
  for (const message of options.messages ?? []) {
    if (message.role === 'system') {
      const text = textOf(message.content);
      if (text) instructions = instructions === undefined ? text : `${instructions}\n${text}`;
      continue;
    }
    if (message.role === 'developer') {
      // 工具增删块属于 developer 事件；套餐路线不支持 mid-conversation
      // 工具变更（设计 §8.1 不支持 tool_search/托管工具），显式拒绝。
      const hasToolChange = (message.content ?? []).some(
        (block) => block.type === 'tool-addition' || block.type === 'tool-removal',
      );
      if (hasToolChange) {
        const error = new Error('套餐路线不支持会话中途的工具增删；请重启会话后使用完整工具清单');
        error.code = 'FIELD_NOT_ALLOWED';
        throw error;
      }
      continue;
    }
    if (message.role === 'tool') {
      input.push({
        type: 'function_call_output',
        call_id: message.toolCallId,
        output: (message.content ?? []).some((block) => block.type === 'image')
          ? inputParts(message.content, imageParts) : textOf(message.content),
      });
      continue;
    }
    if (message.role === 'assistant') {
      for (const block of message.content ?? []) {
        if (block.type === 'text' && block.text) {
          input.push({ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: block.text }] });
        } else if (block.type === 'tool-call') {
          input.push({
            type: 'function_call',
            call_id: block.id,
            namespace: TOOL_NAMESPACE,
            name: toProviderToolName(block.name, toolMapping),
            arguments: block.arguments,
          });
        }
        // reasoning 块：公开摘要不回传上游（回放材料走桥接，设计 §7）。
      }
      continue;
    }
    // 文件已由内核投影为文字；图片由附件服务提供校验后的请求字节。
    const parts = inputParts(message.content, imageParts);
    if (parts.length > 0) input.push({ type: 'message', role: 'user', content: parts });
  }

  if (instructions !== undefined) payload.instructions = instructions;
  payload.input = input;

  // 缓存路由：把会话身份作为稳定键发给上游（内核契约
  // `GenerateOptions.sessionId` 明确允许适配器映射成 model-hidden transport
  // metadata）。不带这个键时上游给每次请求各生成一个随机键，同一会话的
  // 前缀缓存命中率会大幅波动（2026-10-19 实测 58 次请求平均 33%）。
  //
  // **这个键要发两处**：请求体的 `prompt_cache_key` 与 `session-id` 头。原生
  // Codex 两处都发（同一个值），桥接负责把头那一份补上——见
  // `src-tauri/src/openai/inference.rs` 的 `open_upstream` 注释。
  const cacheKey = promptCacheKeyFor(options);
  if (cacheKey !== undefined) payload.prompt_cache_key = cacheKey;

  if (options.tools !== undefined && options.tools.length > 0) {
    // namespace 是工具容器的 type，不能作为 function 定义上的字段。
    payload.tools = [{
      type: 'namespace',
      name: TOOL_NAMESPACE,
      description: 'Tools executed locally by dsh.',
      tools: options.tools.map((tool) => ({
        type: 'function',
        name: toProviderToolName(tool.name, toolMapping),
        description: tool.description,
        parameters: tool.parameters,
      })),
    }];
  }

  // 强度：仅当调用方显式选择时发送原始字符串；「模型默认」= 省略字段
  // （设计 §6.2；bridge 侧按目录档位再校验一道）。
  // 强度：仅当调用方显式选择时发送；线格式在 payload.reasoning.effort，
  // 信封层的 reasoningEffort 供桥接做目录档位校验（双处同值，桥接只认
  // 信封层做门，线格式由这里写入）。
  const envelope = {
    model: options.model,
    catalogRevision: catalog.revisionCombined,
    payload,
  };
  if (options.reasoningEffort !== undefined) {
    envelope.reasoningEffort = options.reasoningEffort;
    payload.reasoning = { effort: options.reasoningEffort };
  }
  return envelope;
}

/**
 * 会话身份 → 上游 `prompt_cache_key`（稳定、可重复、每次调用一致）。
 *
 * 内核把会话身份放在 `GenerateOptions.sessionId`（「Session identity stamped
 * by the loop for request routing」，适配器可以映射成 model-hidden transport
 * metadata）。**不发送它**时上游会为每次请求生成一个新的随机键（2026-10-19
 * 实测 26/26 次回显的 `prompt_cache_key` 互不相同），请求因此散落在不同缓存
 * 分片上：同一段前缀只有落到还留着它的那台机器才命中。
 *
 * 丢的到底是「缓存内容」还是「落到哪台机器」，2026-10-19 在同一条会话上量过
 * （26 次调用，`gpt-6.1-sol`）：`instructions`（1707 token）与 `tools`
 * （7181 token）**逐字不变**，上游给的保留期是 **24h**——前缀既没被改写也没
 * 过期，20/26 次却是 0% 命中。同一账号下用稳定会话标识的原生 Codex（落盘
 * 日志实测：8 次调用全部命中，单次 67%–89%、合计 79.0%）就是「有锚」的
 * 样子。所以要修的是路由亲和性，不是缓存内容。
 *
 * 两条规则：
 * - **压缩调用带同一个键**（`purpose` = `compaction`）：内核的压缩调用**故意
 *   重放同一段对话前缀**（`dsh-compaction-basic` 明写「保留提供方的热前缀」，
 *   压缩指令追加在最后），落到同一分片才吃得到那次命中——它是整轮会话里最贵
 *   的一次调用（整段前缀重算）。会话标题这类一次性小请求不带：前缀与主对话
 *   无关，共用分片没有收益。**未知的 `purpose` 一律不带**（新出现的用途不
 *   猜它要不要共用分片）。
 * - **清洗到桥接白名单允许的形状**（1..=128 个 `[A-Za-z0-9._:-]`）：真实
 *   session id 形如 `session-<uuid>`，不会走到替换分支；写在这里是为了内核
 *   将来换 id 格式时，插件不会发出让桥接拒收的载荷。
 *
 * @param options dsh 的 GenerateOptions（只读 `sessionId` 与 `purpose`）
 * @returns 稳定的缓存键，或 `undefined`（无会话身份 / 非压缩的辅助调用 / 空值）
 */
export function promptCacheKeyFor(options) {
  const purpose = options?.purpose;
  if (purpose !== undefined && purpose !== 'compaction') return undefined;
  const raw = options?.sessionId;
  if (typeof raw !== 'string') return undefined;
  const sanitized = raw.trim().replace(/[^A-Za-z0-9._:-]/g, '-').slice(0, 128);
  return sanitized.length > 0 ? sanitized : undefined;
}

function textOf(content) {
  if (typeof content === 'string') return content;
  return (content ?? [])
    .filter((block) => block.type === 'text')
    .map((block) => block.text)
    .join('');
}

function inputParts(content, imageParts) {
  return (content ?? []).flatMap((block) => {
    if (block.type === 'text') return [{ type: 'input_text', text: block.text }];
    if (block.type === 'image' && imageParts.has(block)) return imageParts.get(block);
    if (block.type === 'image' || block.type === 'file') {
      throw Object.assign(new Error('附件未完成请求转换；请重新添加附件后重试'), { code: 'FIELD_NOT_ALLOWED' });
    }
    return [];
  });
}

/**
 * 额度类失败的文本特征。**刻意不含 rate limit**：请求速率限制是瞬时的、
 * 可重试的，把它归成额度会让用户去等一个根本没被用掉的窗口。
 *
 * 上游实测原文（session-29678872，2026-10-19）：
 * `The ChatGPT user has reached their Subscription Sharing usage limit. Ask
 *  the user to try again after their usage limit resets or use an API key
 *  instead.` —— 内核自带的 `isQuotaExceededError` 要求 "usage limit" 在前、
 * "reached" 在后，这串是反序的（"reached their … usage limit"），所以这里
 * 单独列出反序形态；其余几条与内核口径保持一致。
 */
const QUOTA_FAILURE_PATTERNS = [
  /\b(?:quota|usage[\s_-]+limit)[\s_-]+(?:exceeded|exhausted|reached)\b/i,
  /\bexceed(?:ed|s)?[\s_-]+(?:(?:your|the)[\s_-]+)?(?:current[\s_-]+)?quota\b/i,
  /\b(?:reached|hit|exceeded)\b[^.]{0,80}\busage[\s_-]+limit\b/i,
  /\binsufficient[\s_-]+(?:quota|balance|credits?)\b/i,
  /\b(?:balance|credits?)[\s_-]+(?:exhausted|depleted)\b/i,
  /\bout[\s_-]+of[\s_-]+(?:credits?|budget)\b/i,
];

/**
 * 上游失败文本 → 是否属于「额度已用尽」。
 *
 * 纯判定：调用方（`pumpStream`）据此给出路由级 `code`，再由适配器层翻成
 * 内核的 `LlmError`——内核只信任 `HarnessError` 的 code，普通 Error 一律
 * 记 UNKNOWN，那样 UI 拿不到额度提示、管理入口，重试策略也认不出来。
 *
 * @param detail 上游失败原文（可能为空）
 * @returns 命中额度特征返回 true
 */
export function isQuotaFailureDetail(detail) {
  const text = typeof detail === 'string' ? detail : '';
  return QUOTA_FAILURE_PATTERNS.some((pattern) => pattern.test(text));
}

/**
 * 上游 `usage` → dsh `TokenUsage`。
 *
 * 内核契约（`dsh-llm` 的 `TokenUsage`）：**各桶互斥**——`inputTokens` 只算
 * 未命中缓存的输入，缓存输入单独报 `cacheReadTokens`/`cacheWriteTokens`
 * （计费输入 = 三者之和）。上游 Responses API 的语义相反：`input_tokens`
 * 是**含缓存**的输入总量，`input_tokens_details.cached_tokens` 是其子集。
 * 原样上报的后果不是差一点：2026-10-19 实测一条会话 4,629,002 输入里有
 * 1,525,760 是缓存命中，全被记成未缓存输入，用量面板因此系统性高估。
 *
 * 三条刻意的取舍：
 * - **不上报 `cacheWriteTokens`**：这条路线实测 `cache_write_tokens` 恒为 0，
 *   而它是否含在 `input_tokens` 内没有实证；内核 `normalizeUsage` 在
 *   `totalTokens` 与「三桶之和」不等时会把**整条用量丢掉**，发一个语义
 *   不确定的桶等于拿用量记录去赌。等有非 0 样本再补。
 * - **用总量自算 `totalTokens`**：上游缺 `total_tokens` 时可以自己算
 *   （`input + output` 就是整次调用的总量）；不补的话内核要求缓存两个桶
 *   同时在场才接受，缺失会导致用量不可用。
 * - `reasoningTokens` 必须是 ≤ `outputTokens` 的安全整数，否则省略——
 *   超界同样会被内核整条丢弃。
 *
 * @param usage 上游 `response.usage`（形状不保证，逐字段校验）
 * @returns dsh 形状的用量，或 `undefined`（输入不是合法计数时不报账）
 */
export function mapTokenUsage(usage) {
  const input = usage?.input_tokens;
  if (!Number.isSafeInteger(input) || input < 0) return undefined;
  const output =
    Number.isSafeInteger(usage.output_tokens) && usage.output_tokens >= 0 ? usage.output_tokens : 0;
  const cachedRaw = usage.input_tokens_details?.cached_tokens;
  const cached = Number.isSafeInteger(cachedRaw) && cachedRaw > 0 ? Math.min(cachedRaw, input) : 0;
  const reasoningRaw = usage.output_tokens_details?.reasoning_tokens;
  const reasoning =
    Number.isSafeInteger(reasoningRaw) && reasoningRaw >= 0 && reasoningRaw <= output
      ? reasoningRaw
      : undefined;
  const totalRaw = usage.total_tokens;
  const total =
    Number.isSafeInteger(totalRaw) && totalRaw >= input + output ? totalRaw : input + output;
  return {
    inputTokens: input - cached,
    outputTokens: output,
    totalTokens: total,
    ...(cached > 0 ? { cacheReadTokens: cached } : {}),
    ...(reasoning === undefined ? {} : { reasoningTokens: reasoning }),
  };
}

/**
 * 桥接 NDJSON 事件流 → dsh StreamChunk 序列。
 *
 * 状态机：按块类型懒发 block-start；text/reasoning 增量映射；function_call
 * 增量映射为 tool-call-delta；桥接终止包络 → usage + finish（或抛错）。
 * EOF 无终止包络 → 抛错（设计 §7：incomplete 不算成功）。
 *
 * @param lines 桥接响应体按行异步迭代（不含 HTTP 头）
 * @yields dsh StreamChunk（含末枚 finish）
 * @throws 终止 failed / EOF 无终止包络时抛错（调用方按失败结算）
 */
export async function* pumpStream(lines, toolMapping = new Map()) {
  let nextIndex = 0;
  // 已开始的块：blockType → { index, text, tool }。text/reasoning 累积
  // 文本（block-end 的块内容以此为准——装配器可能不信任增量求和）；
  // tool-call 累积参数与身份。懒分配 block-start，成对收尾。
  const openBlocks = new Map();

  const nextBlock = (blockType) => {
    const existing = openBlocks.get(blockType);
    if (existing !== undefined) return { index: existing.index, started: false };
    const index = nextIndex++;
    openBlocks.set(blockType, { index, text: "", tool: null });
    return { index, started: true };
  };

  let finish = undefined;
  for await (const line of lines) {
    if (!line.trim()) continue;
    const event = JSON.parse(line);
    const type = event.type ?? '';
    if (type === 'bridge.terminal') {
      if (event.status === 'failed') {
        const detail = event.detail ?? '推理失败';
        const error = new Error(detail);
        // 路由级 code：额度类失败单独标出，由适配器层翻成内核的 LlmError；
        // 其余保持通用上游失败（内核记 UNKNOWN，与修复前行为一致）。
        error.code = isQuotaFailureDetail(detail) ? 'QUOTA' : 'UPSTREAM_FAILED';
        throw error;
      }
      if (event.status === 'incomplete') {
        const error = new Error('上游连接中断且未收到终止事件；已输出的文本保留，本次调用按失败结算');
        error.code = 'STREAM_INCOMPLETE';
        throw error;
      }
      // completed：收尾所有开块 → usage → finish（带回放）。
      for (const [blockType, state] of [...openBlocks.entries()]) {
        if (blockType === 'tool-call') {
          yield {
            type: 'block-end',
            index: state.index,
            block: { type: 'tool-call', id: state.tool?.id ?? '', name: state.tool?.name ?? '', arguments: state.text },
          };
        } else {
          yield { type: 'block-end', index: state.index, block: { type: blockType, text: state.text } };
        }
      }
      openBlocks.clear();
      const usage = mapTokenUsage(event.replay?.response?.usage);
      if (usage !== undefined) yield { type: 'usage', usage };
      finish = {
        type: 'finish',
        reason: { kind: 'stop' },
        replayState: { response: event.replay?.response ?? {}, blocks: [] },
      };
      break;
    }
    if (type.endsWith('output_text.delta')) {
      const { index, started } = nextBlock('text');
      if (started) yield { type: 'block-start', index, blockType: 'text' };
      openBlocks.get('text').text += event.delta ?? '';
      yield { type: 'text-delta', index, text: event.delta ?? '' };
      continue;
    }
    if (type.endsWith('reasoning_text.delta') || type.endsWith('reasoning_summary_text.delta')) {
      const { index, started } = nextBlock('reasoning');
      if (started) yield { type: 'block-start', index, blockType: 'reasoning' };
      openBlocks.get('reasoning').text += event.delta ?? '';
      yield { type: 'reasoning-delta', index, text: event.delta ?? '' };
      continue;
    }
    if (type === 'response.output_item.added' && event.item?.type === 'function_call') {
      // 真实 API 的身份（call id 与名字）在 added 条目上：先播种工具块，
      // 后续 delta 只带 item_id 与参数增量。
      const { index, started } = nextBlock('tool-call');
      if (started) yield { type: 'block-start', index, blockType: 'tool-call' };
      const state = openBlocks.get('tool-call');
      const providerName = event.item.name;
      const name = providerName !== undefined ? fromProviderToolName(providerName, toolMapping) : undefined;
      state.tool = {
        id: event.item.call_id ?? event.item.id ?? state.tool?.id ?? '',
        ...(name !== undefined ? { name } : {}),
      };
      if (event.item.arguments) state.text += event.item.arguments;
      continue;
    }
    if (type.endsWith('function_call_arguments.delta')) {
      const { index, started } = nextBlock('tool-call');
      if (started) yield { type: 'block-start', index, blockType: 'tool-call' };
      const state = openBlocks.get('tool-call');
      const name =
        event.name !== undefined
          ? fromProviderToolName(event.name, toolMapping)
          : state.tool?.name;
      if (event.item_id !== undefined && state.tool?.id === undefined) {
        state.tool = { id: event.item_id, ...(name !== undefined ? { name } : {}) };
      }
      state.text += event.delta ?? '';
      yield {
        type: 'tool-call-delta',
        index,
        id: state.tool?.id ?? event.call_id ?? event.item_id ?? '',
        ...(name !== undefined ? { name } : {}),
        argumentsDelta: event.delta ?? '',
      };
      continue;
    }
    // 其它事件（output_item.added 等）P4 后续轮次按需接入。
  }
  if (finish === undefined) {
    const error = new Error('桥接流在终止包络前结束（EOF）；按失败结算');
    error.code = 'STREAM_INCOMPLETE';
    throw error;
  }
  yield finish;
}
