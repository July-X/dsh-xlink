#!/usr/bin/env node
// Hook 通用发送示例（Windows PowerShell / macOS 命令行同一份）。
//
// 用法：
//   DSH_HOOK_TOKEN=<令牌> node scripts/hooks-send-example.mjs \
//     --endpoint ep_xxx --kind text
//
// 只发送合成内容；令牌从环境变量读取，不写进命令行、日志或仓库。
// 文本一次提交；图片/日志走 创建 -> 上传附件 -> commit -> 查回执。
// 429/503 与网络错误有界退避重试；409 不盲重试（内容冲突，重试只会再冲突）。
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const DEFAULT_BASE = 'http://127.0.0.1:17890';
const MAX_ATTEMPTS = 3;
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
  'base64'
);

class HttpError extends Error {
  constructor(status, body) {
    const parsed = safeJson(body);
    const detail = parsed?.error?.code
      ? parsed.error.code + '：' + parsed.error.message
      : String(body || '').slice(0, 300);
    super('HTTP ' + status + (detail ? '（' + detail + '）' : ''));
    this.name = 'HttpError';
    this.status = status;
  }
}

function safeJson(text) {
  try {
    return text ? JSON.parse(text) : null;
  } catch {
    return null;
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function retryDelay(attempt, retryAfter) {
  const seconds = Number(retryAfter);
  if (Number.isFinite(seconds) && seconds >= 0) return Math.min(5000, seconds * 1000);
  return Math.min(5000, 300 * 2 ** attempt);
}

/// 只重试 429 / 503 与网络错误；其余 4xx（尤其 409）原样抛给调用方。
export async function request(url, init, options = {}) {
  const fetchImpl = options.fetchImpl || fetch;
  const sleepImpl = options.sleepImpl || sleep;
  let lastError = null;
  for (let attempt = 0; attempt <= MAX_ATTEMPTS; attempt += 1) {
    let response;
    try {
      response = await fetchImpl(url, init);
    } catch (error) {
      lastError = error;
      if (attempt === MAX_ATTEMPTS) break;
      await sleepImpl(retryDelay(attempt));
      continue;
    }
    const text = await response.text();
    if (response.ok) return { status: response.status, body: safeJson(text) };
    const error = new HttpError(response.status, text);
    if (response.status !== 429 && response.status !== 503) throw error;
    lastError = error;
    if (attempt === MAX_ATTEMPTS) break;
    await sleepImpl(retryDelay(attempt, response.headers.get('retry-after')));
  }
  throw lastError || new Error('请求失败');
}

function eventBody({ kind, externalId, attachment }) {
  const base = {
    schema_version: '1.0',
    external_id: externalId,
    event_type: 'example.' + kind,
    occurred_at: new Date().toISOString(),
    title: kind === 'text' ? '示例文本事件' : kind === 'image' ? '示例图片事件' : '示例日志事件',
    text: '来自 hooks-send-example 的合成内容',
    parameters: { source: 'hooks-send-example', kind },
  };
  if (!attachment) return { ...base, commit: true };
  return {
    ...base,
    commit: false,
    attachments: [
      {
        client_id: 'sample',
        kind,
        filename: kind === 'image' ? 'example.png' : 'example.log',
        media_type: kind === 'image' ? 'image/png' : 'text/plain',
        ...(kind === 'log' ? { encoding: 'utf-8' } : {}),
        size_bytes: attachment.length,
      },
    ],
  };
}

function sampleLog() {
  return Array.from({ length: 30 }, (_, index) => '[example] 合成日志第 ' + (index + 1) + ' 行').join('\n') + '\n';
}

/// 跑一次完整示例；返回 { eventId, state, receiptUrl, kind }，不打印令牌。
export async function sendExample(options) {
  const base = String(options.base || DEFAULT_BASE).replace(/\/+$/, '');
  const token = String(options.token || '');
  if (!token) throw new Error('缺少令牌：请通过 DSH_HOOK_TOKEN 环境变量传入');
  if (!options.endpointId) throw new Error('缺少 --endpoint（接入点 ID）');
  const kind = options.kind || 'text';
  if (!['text', 'image', 'log'].includes(kind)) {
    throw new Error('--kind 只支持 text / image / log');
  }
  const externalId = options.externalId || 'example-' + Date.now();
  const root = base + '/hooks/v1/endpoints/' + encodeURIComponent(options.endpointId);
  const auth = { Authorization: 'Bearer ' + token };
  const payload =
    kind === 'image' ? PNG : kind === 'log' ? Buffer.from(sampleLog(), 'utf8') : null;

  if (kind === 'text') {
    const created = await request(
      root + '/events',
      {
        method: 'POST',
        headers: { ...auth, 'Content-Type': 'application/json' },
        body: JSON.stringify(eventBody({ kind, externalId })),
      },
      options
    );
    return {
      kind,
      eventId: created.body?.event_id,
      state: created.body?.state,
      receiptUrl: created.body?.receipt_url,
    };
  }

  const created = await request(
    root + '/events',
    {
      method: 'POST',
      headers: { ...auth, 'Content-Type': 'application/json' },
      body: JSON.stringify(eventBody({ kind, externalId, attachment: payload })),
    },
    options
  );
  const eventId = created.body?.event_id;
  const attachmentId = created.body?.attachments?.[0]?.attachment_id;
  if (!eventId || !attachmentId) throw new Error('创建事件后没有拿到 event_id / attachment_id');

  await request(
    root + '/events/' + eventId + '/attachments/' + attachmentId,
    {
      method: 'PUT',
      headers: {
        ...auth,
        'Content-Type': kind === 'image' ? 'image/png' : 'text/plain',
        'X-Content-SHA256': createHash('sha256').update(payload).digest('hex'),
      },
      body: payload,
    },
    options
  );

  const committed = await request(
    root + '/events/' + eventId + '/commit',
    { method: 'POST', headers: auth },
    options
  );
  const receipt = await request(
    root + '/events/' + eventId + '/receipt',
    { method: 'GET', headers: auth },
    options
  );
  return {
    kind,
    eventId,
    state: committed.body?.state || receipt.body?.state,
    receiptUrl: committed.body?.receipt_url || receipt.body?.receipt_url,
  };
}

export function parseArgs(argv) {
  const options = { base: DEFAULT_BASE, kind: 'text' };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help' || arg === '-h') {
      options.help = true;
      continue;
    }
    const key = { '--base': 'base', '--endpoint': 'endpointId', '--kind': 'kind', '--external-id': 'externalId' }[arg];
    if (!key) throw new Error('未知参数：' + arg);
    const value = argv[++index];
    if (!value || value.startsWith('--')) throw new Error(arg + ' 缺少值');
    options[key] = value;
  }
  return options;
}

async function main() {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) {
      console.log(
        '用法：DSH_HOOK_TOKEN=<令牌> node scripts/hooks-send-example.mjs --endpoint ep_xxx --kind text|image|log [--base URL] [--external-id ID]'
      );
      return;
    }
    const result = await sendExample({ ...options, token: process.env.DSH_HOOK_TOKEN });
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error('发送失败：' + error.message);
    process.exitCode = 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
