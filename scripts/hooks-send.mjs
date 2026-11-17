#!/usr/bin/env node
// 独立 Node 20+ 发送器；不依赖仓库包，不保存/打印凭据，不跟随重定向。
import { createHash, randomUUID } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { basename, extname } from 'node:path';
import { isIP } from 'node:net';
import { pathToFileURL } from 'node:url';

const MiB = 1024 * 1024;
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// 大文件在慢速可信局域网里可能远超 30 秒；上限与接收器 300 秒上传期限对齐。
export function uploadTimeoutMs(bytes) {
  return Math.min(300_000, Math.max(30_000, Math.ceil(bytes / 100_000) * 1000));
}

export function endpointUrl(options) {
  let url;
  try {
    url = new URL(options.url || options.base);
  } catch {
    throw new Error('请配置 --url 接入点地址，或 --base 与 --endpoint');
  }
  const parts = url.hostname.split('.').map(Number);
  const privateIp = isIP(url.hostname) === 4 && (
    parts[0] === 10 || parts[0] === 127 ||
    (parts[0] === 192 && parts[1] === 168) ||
    (parts[0] === 172 && parts[1] >= 16 && parts[1] <= 31)
  );
  if (!privateIp || !['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) {
    throw new Error('接收地址必须是明确的可信私网 IPv4（或回环）HTTP/HTTPS 地址，不能含凭据或查询参数');
  }
  if (!options.url) {
    if (!/^[A-Za-z0-9_-]+$/.test(options.endpointId || '') || !['', '/'].includes(url.pathname)) {
      throw new Error('--base 只填主机和端口，--endpoint 填接入点 ID');
    }
    url.pathname = '/hooks/v1/endpoints/' + options.endpointId;
  }
  url.pathname = url.pathname.replace(/\/+$/, '');
  if (!/^\/hooks\/v1\/endpoints\/[A-Za-z0-9_-]+$/.test(url.pathname)) {
    throw new Error('--url 需为完整接入点地址 /hooks/v1/endpoints/接入点ID（不含 /events）');
  }
  return url.href;
}

async function boundedBytes(stream, max, label) {
  const chunks = [];
  let size = 0;
  for await (const chunk of stream) {
    const bytes = Buffer.from(chunk);
    size += bytes.length;
    if (size > max) throw new Error(label + '超过大小限制');
    chunks.push(bytes);
  }
  return Buffer.concat(chunks);
}

export async function readOptions(argv, env = process.env, stdin = process.stdin) {
  const parsed = { files: [] };
  const keys = { '--url': 'url', '--base': 'base', '--endpoint': 'endpointId', '--text': 'text',
    '--title': 'title', '--external-id': 'externalId', '--occurred-at': 'occurredAt',
    '--config': 'config', '--profile': 'profile', '--encoding': 'encoding' };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') { parsed.help = true; continue; }
    if (arg === '--stdin') { parsed.stdin = true; continue; }
    const key = keys[arg];
    if (!key && arg !== '--file') throw new Error('未知参数，请使用 --help 查看支持的参数');
    const value = argv[++i];
    if (value === undefined || value.startsWith('--')) throw new Error(arg + ' 缺少值');
    if (arg === '--file') parsed.files.push(value);
    else parsed[key] = value;
  }
  if (parsed.help) return parsed;
  if (parsed.url && (parsed.base || parsed.endpointId)) {
    throw new Error('--url 不能与 --base/--endpoint 混用');
  }
  if ((parsed.base || parsed.endpointId) && (!parsed.base || !parsed.endpointId)) {
    throw new Error('命令行覆盖目标时，--base 与 --endpoint 必须一起提供');
  }
  let profile = {};
  if (parsed.config) {
    let config;
    try {
      config = JSON.parse((await boundedBytes(createReadStream(parsed.config), 64 * 1024, '配置')).toString('utf8'));
    } catch { throw new Error('无法读取 JSON 配置，请检查文件和格式'); }
    profile = config.profiles?.[parsed.profile || 'default'];
    if (!profile || typeof profile !== 'object') throw new Error('配置中没有指定的 profile');
    const allowed = ['url', 'base', 'endpointId', 'token_env'];
    if (Object.keys(profile).some((key) => !allowed.includes(key))) {
      throw new Error('profile 仅允许 url/base/endpointId/token_env；请勿在配置中保存令牌');
    }
  } else if (parsed.profile) throw new Error('--profile 需要配合 --config');
  const tokenEnv = profile.token_env || 'DSH_HOOK_TOKEN';
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(tokenEnv)) throw new Error('token_env 不是有效环境变量名');
  const result = { ...profile, ...parsed, token: env[tokenEnv] };
  // CLI 目标为整体覆盖，不能混合 profile 的地址与另一个来源 ID。
  if (parsed.url) { delete result.base; delete result.endpointId; }
  if (parsed.base) delete result.url;
  if (result.stdin) {
    if (result.text !== undefined) throw new Error('--stdin 与 --text 不能同时使用');
    result.text = (await boundedBytes(stdin, 64 * 1024, '正文')).toString('utf8');
  }
  return result;
}

// 每个 HTTP 尝试都有整体超时（包含响应体）；只重试传输异常、429 和 503。
async function request(url, init, options) {
  const fetchImpl = options.fetchImpl || fetch;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    let response;
    let body;
    try {
      response = await fetchImpl(url, { ...init, redirect: 'error', signal: AbortSignal.timeout(options.timeoutMs || 30_000) });
      body = (await boundedBytes(response.body || [], MiB, '响应')).toString('utf8');
    } catch {
      if (attempt === 3) throw new Error('网络请求失败或超时；请检查接收服务、地址、防火墙。投递结果可能未知，请勿直接生成新 ID 重发');
      await (options.sleepImpl || pause)(300 * 2 ** attempt);
      continue;
    }
    if (response.ok) {
      try { return JSON.parse(body); }
      catch { throw new Error('接收器返回了无效 JSON，请核对地址'); }
    }
    if ([429, 503].includes(response.status) && attempt < 3) {
      const seconds = Number(response.headers.get('retry-after'));
      await (options.sleepImpl || pause)(Number.isFinite(seconds) && seconds > 0 ? Math.min(seconds * 1000, 5000) : 300 * 2 ** attempt);
      continue;
    }
    // 不打印服务端内容，它可能回显凭据、正文或恶意控制字符。
    const hints = { 401: '请检查来源令牌', 403: '请检查允许的来源网段和来源状态', 409: '事件冲突或上传状态冲突，请检查固定 ID 和内容', 413: '内容超出接收器限制', 429: '发送过于频繁', 503: '接收器暂不可用' };
    throw new Error('HTTP ' + response.status + '：' + (hints[response.status] || '请检查接入配置与内容格式'));
  }
}

async function attachment(file, index, encoding) {
  const name = basename(file);
  const imageTypes = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp' };
  const mediaType = imageTypes[extname(name).toLowerCase()];
  if (!mediaType && !['.log', '.txt'].includes(extname(name).toLowerCase())) {
    throw new Error('附件只支持 PNG/JPEG/WebP 图片和 .log/.txt 日志');
  }
  let bytes;
  try { bytes = await boundedBytes(createReadStream(file), (mediaType ? 10 : 50) * MiB, '附件'); }
  catch { throw new Error('无法读取附件或超过大小限制（图片 10 MiB，日志 50 MiB）'); }
  if (!bytes.length) throw new Error('附件不能为空');
  return { bytes, spec: { client_id: 'file_' + index, kind: mediaType ? 'image' : 'log', filename: name,
    media_type: mediaType || 'text/plain', size_bytes: bytes.length, ...(!mediaType ? { encoding } : {}) } };
}

export async function sendMessage(options) {
  const root = endpointUrl(options);
  if (!options.token || /[\r\n]/.test(options.token)) throw new Error('缺少有效令牌，请设置 DSH_HOOK_TOKEN（或 profile 的 token_env）');
  const text = options.text || '';
  const title = options.title || '';
  if (Buffer.byteLength(text) > 64 * 1024 || Buffer.byteLength(title) > 512) throw new Error('正文最多 64 KiB，标题最多 512 字节');
  const files = options.files || [];
  if (!text.trim() && files.length === 0) throw new Error('请提供 --text、--stdin 或 --file');
  if (files.length > 10) throw new Error('每条消息最多 10 个附件');
  const encoding = options.encoding || 'utf-8';
  if (!['utf-8', 'gbk', 'gb18030'].includes(encoding)) throw new Error('--encoding 只支持 utf-8、gbk、gb18030');
  const items = [];
  let total = 0;
  for (const file of files) {
    const item = await attachment(file, items.length, encoding);
    total += item.bytes.length;
    if (total > 100 * MiB) throw new Error('附件总大小最多 100 MiB');
    items.push(item);
  }
  const externalId = options.externalId || 'send-' + randomUUID();
  const occurredAt = options.occurredAt || new Date().toISOString();
  if (!Number.isFinite(Date.parse(occurredAt))) throw new Error('--occurred-at 不是有效时间');
  const payload = JSON.stringify({ schema_version: '1.0', external_id: externalId, event_type: 'message',
    occurred_at: occurredAt, title, text, commit: items.length === 0, attachments: items.map((item) => item.spec) });
  const auth = { Authorization: 'Bearer ' + options.token };
  // ID/时间在整个投递过程中保持不变；回调只接收非敏感投递标识。
  options.onPrepared?.({ externalId, occurredAt });
  const created = await request(root + '/events', { method: 'POST', headers: { ...auth, 'Content-Type': 'application/json' }, body: payload }, options);
  if (typeof created.event_id !== 'string' || !created.event_id) throw new Error('接收器未返回事件 ID');
  const eventUrl = root + '/events/' + encodeURIComponent(created.event_id);
  if (created.state !== 'ready') {
    for (const item of items) {
      const target = created.attachments?.find((entry) => entry.client_id === item.spec.client_id);
      if (!target?.attachment_id) throw new Error('接收器未返回对应附件 ID');
      await request(eventUrl + '/attachments/' + encodeURIComponent(target.attachment_id), {
        method: 'PUT', headers: { ...auth, 'Content-Type': item.spec.media_type, 'X-Content-SHA256': createHash('sha256').update(item.bytes).digest('hex') }, body: item.bytes,
      }, { ...options, timeoutMs: uploadTimeoutMs(item.bytes.length) });
    }
    if (items.length) await request(eventUrl + '/commit', { method: 'POST', headers: auth }, options);
  }
  const receipt = await request(eventUrl + '/receipt', { headers: auth }, options);
  if (receipt.event_id !== created.event_id) throw new Error('回执事件 ID 不匹配');
  if (receipt.state !== 'ready') throw new Error('事件尚未就绪，请在收件箱检查；不要以新 ID 重发');
  for (const item of items) {
    const stored = receipt.attachments?.find((entry) => entry.client_id === item.spec.client_id);
    const hash = createHash('sha256').update(item.bytes).digest('hex');
    if (stored?.state !== 'stored' || stored.sha256 !== hash || stored.size_bytes !== item.bytes.length) {
      throw new Error('回执附件摘要与当前文件不一致；不要复用同一 ID 发送不同文件');
    }
  }
  return { externalId, occurredAt, eventId: created.event_id, state: receipt.state };
}

export const HELP = `用法（Node 20+，仅可信局域网；HTTP 明文）：
只接受私网 IPv4 或回环直连；HTTPS 反向代理请用 curl 或其他支持域名的客户端。
  node hooks-send.mjs --url http://192.168.1.10:17890/hooks/v1/endpoints/ep_x --text "构建完成"
  node hooks-send.mjs --config hooks.json --profile jenkins --title "构建失败" --file build.log
参数：--base URL --endpoint ID 可替代 --url（必须成对，不可混用）；CLI 目标整体覆盖 profile；--stdin 读取正文；--file 可重复。
令牌只从 DSH_HOOK_TOKEN 环境变量读取；profile 可指定 token_env（环境变量名）。
配置格式：{"profiles":{"jenkins":{"url":"http://192.168.1.10:17890/hooks/v1/endpoints/ep_x","token_env":"DSH_HOOK_TOKEN"}}}
日志默认 UTF-8，可用 --encoding gbk|gb18030。自动重试使用同一 ID 和内容。
失败后的手工续传须使用输出的 --external-id 和 --occurred-at，且保持正文/文件不变。
不自动保存投递队列；接收器长期离线时请由 CI 保留内容与投递标识。`;

async function main() {
  try {
    const options = await readOptions(process.argv.slice(2));
    if (options.help) { console.log(HELP); return; }
    const result = await sendMessage({ ...options, onPrepared: (id) => console.error('投递标识：' + JSON.stringify(id)) });
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error('发送失败：' + error.message);
    process.exitCode = 1;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
