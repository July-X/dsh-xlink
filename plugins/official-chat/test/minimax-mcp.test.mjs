// MiniMax Code 本地插件包（官网对话引用）的判据。
//
// 两段：
// · 静态 —— manifest / MCP 文件 / skill / icon 的字段与路径符合 MiniMax 本地插件 V1
//   （规格来自本机 0.1.8：.minimax-plugin/plugin.json ＋ 包根 icon.png / *.mcp.json /
//   skills/<name>/SKILL.md）；
// · 端到端 —— 真起 mcp/server.mjs 子进程，用本地假桥（node:http，127.0.0.1 随机端口）
//   走 initialize → initialized → tools/list → tools/call → ping，断言 stdout 的
//   JSON-RPC 回包、桥收到的请求形状与各错误档都转成 isError。
//
// 不访问真实用户目录与网络，只用本地假桥。

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { createInterface } from 'node:readline';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const pluginDir = fileURLToPath(new URL('../minimax/', import.meta.url));
const serverPath = join(pluginDir, 'mcp/server.mjs');
const URL_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_URL';
const TOKEN_ENV = 'DSH_XLINK_BUILTIN_BRIDGE_TOKEN';

const MANIFEST_FIELDS = new Set([
  '$schema',
  'schemaVersion',
  'name',
  'displayName',
  'version',
  'description',
  'author',
  'icon',
  'darkIcon',
  'category',
  'exampleQueries',
  'apps',
  'mcpServers',
  'skills',
  'hooks',
  'hostBindings',
]);
const CATEGORIES = [
  'Office',
  'Studio',
  'Design & Sites',
  'Code',
  'Business',
  'Sales',
  'Productivity',
  'Science & Healthcare',
  'Education',
  'Other',
];

test('静态：manifest / MCP 文件 / skill / icon 符合 MiniMax 本地插件 V1', () => {
  const manifest = JSON.parse(readFileSync(join(pluginDir, '.minimax-plugin/plugin.json'), 'utf8'));
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.name, 'xlink-official-chat');
  assert.match(manifest.name, /^[a-z][a-z0-9]*(?:[._-][a-z0-9]+)*$/u);
  assert.match(manifest.version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/u);
  for (const key of ['displayName', 'description', 'author']) {
    assert.equal(typeof manifest[key], 'string', `${key} 必须是字符串`);
    assert.ok(manifest[key].trim() !== '', `${key} 不能为空`);
  }
  assert.ok(CATEGORIES.includes(manifest.category), 'category 必须在 MiniMax 的九类里');
  assert.ok(Array.isArray(manifest.exampleQueries) && manifest.exampleQueries.length > 0);
  for (const query of manifest.exampleQueries) {
    assert.equal(typeof query, 'string');
    assert.ok(query.trim() !== '', '示例问句不能是空白');
  }
  for (const key of Object.keys(manifest)) {
    assert.ok(MANIFEST_FIELDS.has(key), `manifest 出现未知字段 ${key}`);
  }
  assert.deepEqual(manifest.apps, []);
  assert.deepEqual(manifest.mcpServers, ['servers.mcp.json']);
  assert.deepEqual(manifest.skills, ['skills/official-chat/SKILL.md']);
  assert.equal(manifest.icon, 'icon.png');

  // icon：包根 png，读前 8 字节确认 PNG 魔数。
  const icon = readFileSync(join(pluginDir, 'icon.png'));
  assert.deepEqual([...icon.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

  // MCP 文件：只许 $schema（可选字符串）/ schemaVersion / mcpServers 三个键。
  const mcp = JSON.parse(readFileSync(join(pluginDir, 'servers.mcp.json'), 'utf8'));
  for (const key of Object.keys(mcp)) {
    assert.ok(['$schema', 'schemaVersion', 'mcpServers'].includes(key), `MCP 文件出现未知字段 ${key}`);
  }
  if (mcp.$schema !== undefined) assert.equal(typeof mcp.$schema, 'string');
  assert.equal(mcp.schemaVersion, 1);
  const servers = Object.entries(mcp.mcpServers ?? {});
  assert.equal(servers.length, 1);
  const [serverName, server] = servers[0];
  assert.equal(serverName, 'xlink-official-chat');
  assert.match(serverName, /^[a-z][a-z0-9]*(?:[._-][a-z0-9]+)*$/u);
  for (const key of Object.keys(server)) {
    assert.ok(
      ['type', 'command', 'args', 'env', 'description', 'timeout', 'tools'].includes(key),
      `stdio server 出现未知字段 ${key}`,
    );
  }
  assert.equal(server.type, 'stdio');
  assert.equal(typeof server.command, 'string');
  assert.ok(!/[/\\]/u.test(server.command), 'command 必须是不带斜杠的解释器名');
  assert.deepEqual(server.args, ['./mcp/server.mjs']);
  for (const arg of server.args) assert.equal(typeof arg, 'string');
  assert.equal(typeof server.env, 'object');
  for (const [key, value] of Object.entries(server.env)) {
    assert.equal(typeof key, 'string');
    assert.equal(typeof value, 'string');
  }
  const source = readFileSync(serverPath, 'utf8');
  const bridgeTimeout = Number(/const TIMEOUT_MS = ([\d_]+);/u.exec(source)?.[1].replaceAll('_', ''));
  assert.ok(bridgeTimeout >= 3 * 170_000, '桥端会执行两次身份检查和一次取数');
  assert.ok(Number.isSafeInteger(server.timeout) && server.timeout > bridgeTimeout, '外层超时须覆盖桥请求');
  assert.ok(statSync(serverPath).isFile(), 'args 指向的 server 必须存在');

  // skill：frontmatter name 等于目录名，description 非空，正文中文且点明两个工具。
  const skill = readFileSync(join(pluginDir, 'skills/official-chat/SKILL.md'), 'utf8');
  const frontmatter = /^---\n([\s\S]*?)\n---\n/u.exec(skill);
  assert.ok(frontmatter, 'SKILL.md 必须带 frontmatter');
  const name = /^name:\s*(.+)$/mu.exec(frontmatter[1]);
  const description = /^description:\s*(.+)$/mu.exec(frontmatter[1]);
  assert.equal(name?.[1]?.trim(), 'official-chat');
  assert.ok(description?.[1]?.trim(), 'description 不能为空');
  assert.match(skill, /[\u4e00-\u9fff]/u);
  assert.match(skill, /official_chat_list/);
  assert.match(skill, /official_chat_read/);
});

function startBridge() {
  const calls = [];
  const state = {
    respond: () => ({
      status: 200,
      body: { ok: true, site: 'deepseek', structure: { items: [], sessionsLength: 0, hasMore: false } },
    }),
  };
  const server = createServer((request, response) => {
    let body = '';
    request.setEncoding('utf8');
    request.on('data', (chunk) => {
      body += chunk;
    });
    request.on('end', () => {
      calls.push({
        method: request.method,
        url: request.url,
        authorization: request.headers.authorization,
        body,
      });
      const reply = state.respond({ url: request.url, body });
      response.writeHead(reply.status, { 'content-type': 'application/json' });
      response.end(typeof reply.body === 'string' ? reply.body : JSON.stringify(reply.body));
    });
  });
  const close = () =>
    new Promise((resolve) => {
      server.closeAllConnections?.();
      server.close(resolve);
    });
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({ url: `http://127.0.0.1:${port}`, calls, state, close });
    });
  });
}

function startMcp(env) {
  const child = spawn(process.execPath, [serverPath], { env, stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  const received = [];
  let stderr = '';
  let nextId = 1;
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => {
    stderr += chunk;
  });
  createInterface({ input: child.stdout }).on('line', (line) => {
    const text = line.trim();
    if (text === '') return;
    received.push(text);
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    const resolve = pending.get(message.id);
    if (!resolve) return;
    pending.delete(message.id);
    resolve(message);
  });
  const failAll = (error) => {
    for (const [id, resolve] of pending) {
      pending.delete(id);
      resolve({ error: { code: 'client_failed', message: String(error) } });
    }
  };
  child.once('error', failAll);
  child.once('exit', (code, signal) => {
    if (pending.size > 0) failAll(`MCP server 提前退出（code=${code}, signal=${signal}）`);
  });
  const send = (message) => child.stdin.write(`${JSON.stringify(message)}\n`);
  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const id = nextId;
      nextId += 1;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(
          new Error(
            `MCP 请求超时：${method}\nstdout：${received.slice(-5).join('\n')}\nstderr：${stderr}`,
          ),
        );
      }, 15_000);
      pending.set(id, (message) => {
        clearTimeout(timer);
        resolve(message);
      });
      send({ jsonrpc: '2.0', id, method, ...(params === undefined ? {} : { params }) });
    });
  const notify = (method, params) =>
    send({ jsonrpc: '2.0', method, ...(params === undefined ? {} : { params }) });
  const stop = () =>
    new Promise((resolve) => {
      if (child.exitCode !== null) {
        resolve();
        return;
      }
      child.once('exit', resolve);
      child.kill();
    });
  return { request, notify, stop, stderr: () => stderr };
}

test('端到端：目录 / 正文 / 参数校验 / 失败档都走本地假桥', { timeout: 60_000 }, async () => {
  const bridge = await startBridge();
  const mcp = startMcp({
    ...process.env,
    [URL_ENV]: bridge.url,
    [TOKEN_ENV]: 'test-only-token',
  });
  try {
    const init = await mcp.request('initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: 'minimax-mcp-test', version: '1.0.0' },
    });
    assert.equal(init.result.protocolVersion, '2025-06-18');
    assert.deepEqual(init.result.capabilities, { tools: {} });
    assert.equal(init.result.serverInfo.name, 'xlink-official-chat');
    assert.equal(init.result.serverInfo.version, '0.1.0');
    mcp.notify('notifications/initialized');

    const listed = await mcp.request('tools/list');
    assert.deepEqual(
      listed.result.tools.map((tool) => tool.name),
      ['official_chat_list', 'official_chat_read'],
    );
    for (const tool of listed.result.tools) {
      assert.equal(tool.inputSchema.type, 'object');
      assert.ok(tool.description.trim() !== '');
    }

    // 目录：默认站点 deepseek，标题 / 无标题 / 时间 / 总数 / 逐条 id。
    bridge.state.respond = () => ({
      status: 200,
      body: {
        ok: true,
        site: 'deepseek',
        structure: {
          items: [
            { id: 'sess-1', title: '第一条会话', title_type: 'auto', model_type: 'chat', pinned: false, updated_at: 1_700_000_000 },
            { id: 'sess-2', title: '', title_type: '', model_type: '', pinned: false, updated_at: 0 },
          ],
          sessionsLength: 37,
          hasMore: true,
        },
      },
    });
    const list = await mcp.request('tools/call', { name: 'official_chat_list', arguments: {} });
    assert.notEqual(list.result.isError, true);
    const listText = list.result.content[0].text;
    assert.match(listText, /站点：DeepSeek/);
    assert.match(listText, /本次返回：2 条；站点原始总数：37 条/u);
    assert.match(listText, /1\. 第一条会话 — sess-1（2023-11-14 22:13 UTC）/u);
    assert.match(listText, /2\. \(无标题\) — sess-2/u);
    assert.deepEqual(bridge.calls.map((call) => call.url), ['/v1/official-chat/session-structure']);
    assert.ok(bridge.calls.every((call) => call.method === 'POST' && call.authorization === 'Bearer test-only-token'));
    assert.deepEqual(JSON.parse(bridge.calls[0].body), { command: 'sessionStructure', site: 'deepseek' });

    // 目录：另一站 ＋ refresh，空列表说清楚「没有可选会话」。
    bridge.calls.length = 0;
    bridge.state.respond = () => ({
      status: 200,
      body: { ok: true, site: 'minimax', structure: { items: [], sessionsLength: 0, hasMore: false } },
    });
    const empty = await mcp.request('tools/call', {
      name: 'official_chat_list',
      arguments: { site: 'minimax', refresh: true },
    });
    assert.notEqual(empty.result.isError, true);
    assert.match(empty.result.content[0].text, /MiniMax/);
    assert.match(empty.result.content[0].text, /该站点当前没有可选会话/);
    assert.deepEqual(JSON.parse(bridge.calls[0].body), {
      command: 'sessionStructure',
      site: 'minimax',
      refresh: true,
    });

    // 正文必须先在相同站点的目录里选过。
    bridge.state.respond = () => ({
      status: 200,
      body: { ok: true, site: 'chatgpt', structure: { items: [{ id: 'sess-9', title: '会话' }] } },
    });
    await mcp.request('tools/call', { name: 'official_chat_list', arguments: { site: 'chatgpt' } });
    bridge.state.respond = () => ({
      status: 200,
      body: {
        ok: true,
        site: 'chatgpt',
        messages: [
          { role: 'user', text: '问题一' },
          { role: 'assistant', text: '回答一' },
        ],
        captureResult: { status: 'partial', omitted: ['附件 1'], unsupported: ['thinking_content'] },
        pick: { matched: 1 },
      },
    });
    const read = await mcp.request('tools/call', {
      name: 'official_chat_read',
      arguments: { site: 'chatgpt', session_id: 'sess-9' },
    });
    assert.notEqual(read.result.isError, true);
    const readText = read.result.content[0].text;
    assert.match(readText, /站点：ChatGPT/);
    assert.match(readText, /会话 id：sess-9/);
    assert.match(readText, /状态：partial/);
    assert.match(readText, /\[user\]\n问题一/u);
    assert.match(readText, /\[assistant\]\n回答一/u);
    assert.match(readText, /省略：附件 1/);
    assert.match(readText, /未支持：thinking_content/);
    assert.deepEqual(JSON.parse(bridge.calls.at(-1).body), {
      command: 'sessionBody',
      sessionId: 'sess-9',
      site: 'chatgpt',
    });

    // 参数校验：缺 session_id / 非法站点都不打桥，直接 isError。
    const before = bridge.calls.length;
    const missingId = await mcp.request('tools/call', { name: 'official_chat_read', arguments: {} });
    assert.equal(missingId.result.isError, true);
    assert.match(missingId.result.content[0].text, /session_id/);
    const badSite = await mcp.request('tools/call', {
      name: 'official_chat_list',
      arguments: { site: 'other' },
    });
    assert.equal(badSite.result.isError, true);
    assert.match(badSite.result.content[0].text, /不支持的站点/);
    const unknownTool = await mcp.request('tools/call', { name: 'official_chat_unknown', arguments: {} });
    assert.equal(unknownTool.result.isError, true);
    assert.equal(bridge.calls.length, before);

    // 失败档：ok:false 与 HTTP 非 2xx 都要带 code / detail，不许吞。
    bridge.state.respond = () => ({
      status: 200,
      body: { ok: false, error: { code: 'not_logged_in', detail: '请先在官网窗口登录' } },
    });
    const notLoggedIn = await mcp.request('tools/call', { name: 'official_chat_list', arguments: {} });
    assert.equal(notLoggedIn.result.isError, true);
    assert.match(notLoggedIn.result.content[0].text, /not_logged_in/);
    assert.match(notLoggedIn.result.content[0].text, /请先在官网窗口登录/);

    bridge.state.respond = () => ({
      status: 500,
      body: { ok: false, error: { code: 'timeout', detail: '取数窗口超时' } },
    });
    const httpError = await mcp.request('tools/call', {
      name: 'official_chat_read',
      arguments: { site: 'chatgpt', session_id: 'sess-9' },
    });
    assert.equal(httpError.result.isError, true);
    assert.match(httpError.result.content[0].text, /HTTP 500/);
    assert.match(httpError.result.content[0].text, /timeout/);
    assert.match(httpError.result.content[0].text, /取数窗口超时/);

    // JSON-RPC 自身：未知方法 -32601，ping 回空对象。
    const unknownMethod = await mcp.request('no/such/method');
    assert.equal(unknownMethod.error.code, -32601);
    const pong = await mcp.request('ping');
    assert.deepEqual(pong.result, {});
  } finally {
    await mcp.stop();
    await bridge.close();
  }
});

test('桥拒绝账号变更时 MCP 返回可见错误', { timeout: 30_000 }, async () => {
  const bridge = await startBridge();
  const mcp = startMcp({ ...process.env, [URL_ENV]: bridge.url, [TOKEN_ENV]: 'test-only-token' });
  try {
    bridge.state.respond = () => ({ status: 403, body: { ok: false, error: { code: 'account_changed', detail: '请重新选择会话' } } });
    const reply = await mcp.request('tools/call', { name: 'official_chat_read', arguments: { site: 'minimax', session_id: 'same-id' } });
    assert.equal(reply.result.isError, true);
    assert.match(reply.result.content[0].text, /account_changed/);
  } finally {
    await mcp.stop();
    await bridge.close();
  }
});

test('端到端：桥凭据缺失 / 连不上都转成明确的 isError', { timeout: 30_000 }, async () => {
  const missingEnv = { ...process.env };
  delete missingEnv[URL_ENV];
  delete missingEnv[TOKEN_ENV];
  const noBridge = startMcp(missingEnv);
  try {
    await noBridge.request('initialize', { protocolVersion: '2024-11-05', capabilities: {} });
    const result = await noBridge.request('tools/call', { name: 'official_chat_list', arguments: {} });
    assert.equal(result.result.isError, true);
    assert.match(result.result.content[0].text, /DSH_XLINK_BUILTIN_BRIDGE_URL/);
  } finally {
    await noBridge.stop();
  }

  // 127.0.0.1:1 上没有任何服务：应报「无法连接取数桥」而不是挂死或空回。
  const deadBridge = startMcp({
    ...process.env,
    [URL_ENV]: 'http://127.0.0.1:1',
    [TOKEN_ENV]: 'test-only-token',
  });
  try {
    await deadBridge.request('initialize', { protocolVersion: '2024-11-05', capabilities: {} });
    const result = await deadBridge.request('tools/call', {
      name: 'official_chat_read',
      arguments: { session_id: 'sess-1' },
    });
    assert.equal(result.result.isError, true);
    assert.match(result.result.content[0].text, /无法连接取数桥/);
  } finally {
    await deadBridge.stop();
  }
});
