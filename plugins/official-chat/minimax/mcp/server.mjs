// dsh-xlink「官网对话引用」的 MiniMax 本地插件 MCP server。
//
// 把壳的本地回环桥（POST /v1/official-chat/session-structure 与 …/session-body）
// 包成两个只读工具；桥地址与令牌由壳启动 MiniMax 服务进程时注入。
// stdout 只走 JSON-RPC 行，日志一律 stderr。

import { createInterface } from 'node:readline';

const URL_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_URL';
const TOKEN_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_TOKEN';
const SITES = ['deepseek', 'minimax', 'chatgpt'];
const SITE_LABELS = { deepseek: 'DeepSeek', minimax: 'MiniMax', chatgpt: 'ChatGPT' };
const SERVER_INFO = { name: 'xlink-official-chat', version: '0.1.0' };
const DEFAULT_PROTOCOL_VERSION = '2024-11-05';
const TIMEOUT_MS = 600_000;
const MAX_TEXT_CHARS = 40_000;

const TOOLS = [
  {
    name: 'official_chat_list',
    description:
      '列出用户自己登录的官网网页版会话目录（DeepSeek / MiniMax / ChatGPT）。只返回最近若干条，取回会话 id 后交给 official_chat_read 读正文。',
    inputSchema: {
      type: 'object',
      properties: {
        site: {
          type: 'string',
          enum: SITES,
          description: '要列哪个站点，缺省 deepseek。',
        },
        refresh: {
          type: 'boolean',
          description: 'true 表示强制重新取一遍当刻目录（较慢，约数秒）。',
        },
      },
      additionalProperties: false,
    },
  },
  {
    name: 'official_chat_read',
    description:
      '读取一条官网网页版会话的正文（只读）。会话 id 必须来自 official_chat_list，不要编造。正文可能不完整。',
    inputSchema: {
      type: 'object',
      properties: {
        site: {
          type: 'string',
          enum: SITES,
          description: '会话所属站点，缺省 deepseek。',
        },
        session_id: {
          type: 'string',
          description: 'official_chat_list 返回的会话 id。',
        },
      },
      required: ['session_id'],
      additionalProperties: false,
    },
  },
];

/** 校验站点参数：缺省 deepseek，白名单之外直接报错。 */
function siteOf(args) {
  const site = args?.site === undefined ? 'deepseek' : args.site;
  if (!SITES.includes(site)) {
    throw new Error(`不支持的站点：${String(site)}（可选 ${SITES.join(' / ')}）`);
  }
  return site;
}

/** 站点时间可能是秒或毫秒，能认出才格式化。 */
function formatTime(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return '';
  const date = new Date(number > 1e12 ? number : number * 1000);
  if (Number.isNaN(date.getTime())) return '';
  return `${date.toISOString().replace('T', ' ').slice(0, 16)} UTC`;
}

/** 省略 / 未支持的取值（数组或字符串）转一行可读文本。 */
function describeValue(value) {
  if (Array.isArray(value)) {
    return value.length === 0 ? '' : value.map((item) => (typeof item === 'string' ? item : JSON.stringify(item))).join('、');
  }
  if (value === undefined || value === null || value === '') return '';
  return typeof value === 'string' ? value : JSON.stringify(value);
}

function clip(text) {
  return text.length > MAX_TEXT_CHARS ? `${text.slice(0, MAX_TEXT_CHARS)}\n（已截断，原文更长）` : text;
}

/** 一次桥请求：非 2xx / ok:false / 网络错误 / 超时都抛带原因的 Error。 */
async function post(path, payload) {
  const base = process.env[URL_ENV];
  const token = process.env[TOKEN_ENV];
  if (!base || !token) {
    const missing = !base ? URL_ENV : TOKEN_ENV;
    throw new Error(
      `取数桥不可用：本机 dsh-xlink 壳没有为这个进程注入 ${missing}。请在 dsh-xlink 里启动 MiniMax 工作台，不要单独运行这个插件。`,
    );
  }
  let response;
  try {
    response = await fetch(new URL(path, base), {
      method: 'POST',
      headers: {
        authorization: `Bearer ${token}`,
        'content-type': 'application/json',
      },
      body: JSON.stringify(payload),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch (error) {
    if (error?.name === 'TimeoutError') {
      throw new Error(`取数桥在 ${TIMEOUT_MS / 1000} 秒内没有响应（可能正在等官网窗口取数）。`);
    }
    throw new Error(`无法连接取数桥：${String(error?.message ?? error)}`);
  }
  const text = await response.text();
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error(`取数桥返回了无法解析的内容（HTTP ${response.status}）。`);
  }
  if (!response.ok) {
    const failure = parsed?.error;
    const suffix = failure?.code ? `（${failure.code}）` : '';
    const detail = failure?.detail ? `：${failure.detail}` : '';
    throw new Error(`取数桥返回 HTTP ${response.status}${suffix}${detail}`);
  }
  if (parsed?.ok !== true) {
    const failure = parsed?.error;
    throw new Error(
      `官网对话取数失败：${failure?.code ?? 'unknown'}${failure?.detail ? ` — ${failure.detail}` : ''}`,
    );
  }
  return parsed;
}

function describeList(result, site) {
  const structure = result.structure ?? {};
  const items = Array.isArray(structure.items) ? structure.items : [];
  const lines = [`站点：${SITE_LABELS[site]}（${site}）`, `本次返回：${items.length} 条`];
  if (typeof structure.sessionsLength === 'number') {
    lines[1] += `；站点原始总数：${structure.sessionsLength} 条`;
  }
  if (items.length === 0) {
    lines.push('该站点当前没有可选会话。');
    return lines.join('\n');
  }
  lines.push('');
  items.forEach((item, index) => {
    const title = String(item?.title ?? '').trim() || '(无标题)';
    const id = String(item?.id ?? '');
    const updated = formatTime(item?.updated_at);
    lines.push(`${index + 1}. ${title} — ${id}${updated ? `（${updated}）` : ''}`);
  });
  return lines.join('\n');
}

function describeRead(result, site, sessionId) {
  const capture = result.captureResult ?? {};
  const lines = [
    `站点：${SITE_LABELS[site]}（${site}）`,
    `会话 id：${sessionId}`,
    `状态：${capture.status ?? '未知'}`,
  ];
  const omitted = describeValue(capture.omitted);
  const unsupported = describeValue(capture.unsupported);
  if (omitted) lines.push(`省略：${omitted}`);
  if (unsupported) lines.push(`未支持：${unsupported}`);
  const messages = Array.isArray(result.messages) ? result.messages : [];
  const body = messages
    .map((message) => `[${String(message?.role ?? '')}]\n${String(message?.text ?? '')}`)
    .join('\n\n');
  lines.push('', body || '（这条会话没有取到正文）');
  return clip(lines.join('\n'));
}

async function toolList(args) {
  const site = siteOf(args);
  const payload = { command: 'sessionStructure', site };
  if (args?.refresh === true) payload.refresh = true;
  return describeList(await post('/v1/official-chat/session-structure', payload), site);
}

async function toolRead(args) {
  const site = siteOf(args);
  const sessionId = typeof args?.session_id === 'string' ? args.session_id.trim() : '';
  if (sessionId === '') {
    throw new Error('缺少 session_id：先用 official_chat_list 取会话 id，不要编造。');
  }
  return describeRead(await post('/v1/official-chat/session-body', {
    command: 'sessionBody',
    sessionId,
    site,
  }), site, sessionId);
}

const HANDLERS = { official_chat_list: toolList, official_chat_read: toolRead };

async function callTool(params) {
  const handler = HANDLERS[params?.name];
  try {
    if (!handler) throw new Error(`未知工具：${String(params?.name)}`);
    const text = await handler(params?.arguments ?? {});
    return { content: [{ type: 'text', text }] };
  } catch (error) {
    return { content: [{ type: 'text', text: String(error?.message ?? error) }], isError: true };
  }
}

/** 处理一条 JSON-RPC 消息；notification 返回 undefined（不回包）。 */
async function handle(message) {
  const id = message?.id;
  if (id === undefined || id === null) return undefined;
  const method = message?.method;
  const params = message?.params;
  switch (method) {
    case 'initialize':
      return {
        jsonrpc: '2.0',
        id,
        result: {
          protocolVersion:
            typeof params?.protocolVersion === 'string' && params.protocolVersion !== ''
              ? params.protocolVersion
              : DEFAULT_PROTOCOL_VERSION,
          capabilities: { tools: {} },
          serverInfo: SERVER_INFO,
        },
      };
    case 'ping':
      return { jsonrpc: '2.0', id, result: {} };
    case 'tools/list':
      return { jsonrpc: '2.0', id, result: { tools: TOOLS } };
    case 'tools/call':
      return { jsonrpc: '2.0', id, result: await callTool(params) };
    default:
      return { jsonrpc: '2.0', id, error: { code: -32601, message: `Method not found: ${String(method)}` } };
  }
}

async function main() {
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    const text = line.trim();
    if (text === '') continue;
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'Parse error' } })}\n`);
      continue;
    }
    const response = await handle(message);
    if (response) process.stdout.write(`${JSON.stringify(response)}\n`);
  }
}

await main();
