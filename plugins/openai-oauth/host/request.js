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
 * - 终止分类由桥接包络承载（completed/failed/incomplete），EOF 不算成功。
 */

/** 套餐路线无法执行、调用方却显式给出了的参数 → 拒绝（§8.1）。 */
const UNSUPPORTED_GENERATION_FIELDS = [
  ['temperature', '采样温度'],
  ['maxTokens', '输出上限'],
  ['stop', '停止序列'],
];

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
            name: fromProviderToolName(block.name, toolMapping),
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

  if (options.tools !== undefined && options.tools.length > 0) {
    payload.tools = options.tools.map((tool) => ({
      type: 'function',
      name: toProviderToolName(tool.name, toolMapping),
      description: tool.description,
      parameters: tool.parameters,
    }));
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
export async function* pumpStream(lines) {
  let nextIndex = 0;
  // 已开始的块：blockType → { index, text, tool }。text/reasoning 累积
  // 文本（block-end 的块内容以此为准——装配器可能不信任增量求和）；
  // tool-call 累积参数与身份。懒分配 block-start，成对收尾。
  const openBlocks = new Map();
  const toolNameByCallId = new Map();

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
        const error = new Error(event.detail ?? '推理失败');
        error.code = 'UPSTREAM_FAILED';
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
      const usage = event.replay?.response?.usage;
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
      if (usage && typeof usage.input_tokens === 'number') {
        yield {
          type: 'usage',
          usage: {
            inputTokens: usage.input_tokens,
            outputTokens: usage.output_tokens ?? 0,
            ...(typeof usage.total_tokens === 'number' ? { totalTokens: usage.total_tokens } : {}),
          },
        };
      }
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
      const name = providerName !== undefined ? fromProviderToolName(providerName, toolNameByCallId) : undefined;
      state.tool = {
        id: event.item.id ?? state.tool?.id ?? '',
        ...(name !== undefined ? { name } : {}),
      };
      if (providerName !== undefined && event.item.id !== undefined) {
        toolNameByCallId.set(event.item.id, name ?? providerName);
      }
      if (event.item.arguments) state.text += event.item.arguments;
      continue;
    }
    if (type.endsWith('function_call_arguments.delta')) {
      const { index, started } = nextBlock('tool-call');
      if (started) yield { type: 'block-start', index, blockType: 'tool-call' };
      const state = openBlocks.get('tool-call');
      const name =
        event.name !== undefined
          ? fromProviderToolName(event.name, toolNameByCallId)
          : state.tool?.name;
      if (event.item_id !== undefined && state.tool?.id === undefined) {
        state.tool = { id: event.item_id, ...(name !== undefined ? { name } : {}) };
      }
      state.text += event.delta ?? '';
      yield {
        type: 'tool-call-delta',
        index,
        id: event.item_id ?? event.call_id ?? '',
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
