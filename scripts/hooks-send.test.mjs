import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { endpointUrl, readOptions, sendMessage, uploadTimeoutMs } from './hooks-send.mjs';

async function server(t, handler) {
  const http = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    handler(req, res, Buffer.concat(chunks));
  });
  await new Promise((resolve) => http.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise((resolve) => { http.close(resolve); http.closeAllConnections(); }));
  return `http://127.0.0.1:${http.address().port}/hooks/v1/endpoints/ep_test`;
}
function json(res, value, status = 200) { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(value)); }
async function directory(t) {
  const dir = await mkdtemp(join(tmpdir(), 'hooks-sender-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  return dir;
}

test('upload timeout grows with file size and caps at the server deadline', () => {
  assert.equal(uploadTimeoutMs(0), 30_000);
  assert.equal(uploadTimeoutMs(1024 * 1024), 30_000);
  assert.ok(uploadTimeoutMs(10 * 1024 * 1024) > 30_000);
  assert.equal(uploadTimeoutMs(50 * 1024 * 1024), 300_000);
});

test('LAN targets reject public/DNS/credential/redirect-shaped URLs', () => {
  for (const host of ['192.168.2.3', '10.2.3.4', '172.16.2.3', '172.31.2.3', '127.0.0.1']) {
    assert.match(endpointUrl({ base: `http://${host}:17890`, endpointId: 'ep_test' }), /endpoints\/ep_test$/);
  }
  for (const url of ['http://8.8.8.8', 'http://0.0.0.0', 'http://172.32.1.1', 'http://example.com', 'http://user:password@192.168.1.1', 'http://192.168.1.1/?token=secret']) {
    assert.throws(() => endpointUrl({ base: url, endpointId: 'ep_test' }));
  }
});

test('profile selects environment token; stdin and repeat files work without saving secrets', async (t) => {
  const path = join(await directory(t), 'hooks.json');
  await writeFile(path, JSON.stringify({ profiles: { build: { url: 'http://10.1.1.1:17890/hooks/v1/endpoints/ep_x', token_env: 'BUILD_TOKEN' } } }));
  const options = await readOptions(['--config', path, '--profile', 'build', '--stdin', '--file', 'a.log', '--file', 'b.png'], { BUILD_TOKEN: 'private' }, Readable.from(['真实正文']));
  assert.equal(options.token, 'private');
  assert.equal(options.text, '真实正文');
  assert.deepEqual(options.files, ['a.log', 'b.png']);
  await assert.rejects(readOptions(['--text', 'x', '--stdin'], {}, Readable.from([])), /不能同时/);
  await writeFile(path, JSON.stringify({ profiles: { default: { token: 'do-not-save' } } }));
  await assert.rejects(readOptions(['--config', path]), /请勿/);
});

test('real HTTP text delivery retries exact bytes and verifies receipt', async (t) => {
  const posts = [];
  const url = await server(t, (req, res, bytes) => {
    assert.equal(req.headers.authorization, 'Bearer test-token');
    if (req.method === 'POST') {
      posts.push(bytes.toString());
      if (posts.length === 1) { json(res, {}, 503); return; }
      json(res, { event_id: 'ev_1', state: 'ready' });
    } else json(res, { event_id: 'ev_1', state: 'ready' });
  });
  const result = await sendMessage({ url, token: 'test-token', text: '真实中文消息', sleepImpl: async () => {} });
  assert.equal(result.state, 'ready');
  assert.equal(posts[0], posts[1]);
  const event = JSON.parse(posts[0]);
  assert.equal(event.text, '真实中文消息');
  assert.equal(event.commit, true);
  assert.equal(event.external_id, result.externalId);
});

test('real file bytes are hashed, matched by client ID, uploaded then committed', async (t) => {
  const dir = await directory(t);
  const a = Buffer.from('真实构建日志\n');
  const b = Buffer.from('另一份日志\n');
  await writeFile(join(dir, 'a.log'), a);
  await writeFile(join(dir, 'b.txt'), b);
  const order = [];
  const url = await server(t, (req, res, bytes) => {
    order.push(req.method + ' ' + req.url.split('/').at(-1));
    if (req.url.endsWith('/events')) {
      const event = JSON.parse(bytes);
      assert.equal(event.attachments[0].size_bytes, a.length);
      assert.equal(event.attachments[0].filename, 'a.log');
      assert.equal(event.commit, false);
      json(res, { event_id: 'ev_2', state: 'receiving', attachments: [{ client_id: 'file_1', attachment_id: 'at_b' }, { client_id: 'file_0', attachment_id: 'at_a' }] });
    } else if (req.method === 'PUT') {
      const expected = req.url.endsWith('at_a') ? a : b;
      assert.deepEqual(bytes, expected);
      assert.equal(req.headers['x-content-sha256'], createHash('sha256').update(expected).digest('hex'));
      json(res, { state: 'stored' });
    } else json(res, { event_id: 'ev_2', state: 'ready', attachments: [a, b].map((bytes, index) => ({ client_id: 'file_' + index, state: 'stored', size_bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') })) });
  });
  await sendMessage({ url, token: 'x', files: [join(dir, 'a.log'), join(dir, 'b.txt')] });
  assert.deepEqual(order, ['POST events', 'PUT at_a', 'PUT at_b', 'POST commit', 'GET receipt']);
});

test('already-ready retry skips uploads and checks receipt', async (t) => {
  const dir = await directory(t);
  await writeFile(join(dir, 'a.log'), 'log');
  const methods = [];
  const url = await server(t, (req, res) => { methods.push(req.method); json(res, { event_id: 'ev_3', state: 'ready', attachments: [{ client_id: 'file_0', state: 'stored', size_bytes: 3, sha256: createHash('sha256').update('log').digest('hex') }] }); });
  await sendMessage({ url, token: 'x', files: [join(dir, 'a.log')], externalId: 'stable', occurredAt: '2026-11-17T00:00:00Z' });
  assert.deepEqual(methods, ['POST', 'GET']);
});

test('non-retryable errors never expose reflected credentials', async (t) => {
  let calls = 0;
  const url = await server(t, (_req, res) => { calls++; json(res, { error: { message: 'secret-token' } }, 401); });
  await assert.rejects(sendMessage({ url, token: 'secret-token', text: 'test' }), (error) => error.message.includes('HTTP 401') && !error.message.includes('secret-token'));
  assert.equal(calls, 1);
});

test('redirects are never followed', async (t) => {
  let followed = 0;
  const destination = await server(t, (_req, res) => { followed++; json(res, {}); });
  const url = await server(t, (_req, res) => { res.writeHead(307, { Location: destination }); res.end(); });
  await assert.rejects(sendMessage({ url, token: 'secret', text: 'test', sleepImpl: async () => {} }), /网络请求/);
  assert.equal(followed, 0);
});

test('timeout bounds attempts and a receiving receipt is not success', async (t) => {
  let calls = 0;
  const url = await server(t, () => { calls++; });
  await assert.rejects(sendMessage({ url, token: 'x', text: 'test', timeoutMs: 30, sleepImpl: async () => {} }), /超时/);
  assert.equal(calls, 4);
  const waiting = await server(t, (_req, res) => json(res, { event_id: 'ev_x', state: 'receiving' }));
  await assert.rejects(sendMessage({ url: waiting, token: 'x', text: 'test' }), /尚未就绪/);
});

test('CLI failures exit nonzero and do not print token', async () => {
  const child = spawn(process.execPath, [fileURLToPath(new URL('./hooks-send.mjs', import.meta.url)), '--url', 'http://8.8.8.8/hooks/v1/endpoints/ep_x', '--text', 'test'], { env: { ...process.env, DSH_HOOK_TOKEN: 'secret-value' } });
  let output = '';
  child.stdout.on('data', (data) => { output += data; });
  child.stderr.on('data', (data) => { output += data; });
  const code = await new Promise((resolve) => child.on('close', resolve));
  assert.equal(code, 1);
  assert.match(output, /私网/);
  assert.ok(!output.includes('secret-value'));
});

test('same ID cannot silently acknowledge different same-size attachment bytes', async (t) => {
  const dir = await directory(t);
  await writeFile(join(dir, 'a.log'), 'new');
  const url = await server(t, (_req, res) => json(res, { event_id: 'ev_same', state: 'ready', attachments: [{ client_id: 'file_0', state: 'stored', size_bytes: 3, sha256: createHash('sha256').update('old').digest('hex') }] }));
  await assert.rejects(sendMessage({ url, token: 'x', files: [join(dir, 'a.log')], externalId: 'same', occurredAt: '2026-11-17T00:00:00Z' }), /摘要/);
});

test('input validation rejects excessive text, unsupported files and missing token before HTTP', async (t) => {
  const url = 'http://127.0.0.1:17890/hooks/v1/endpoints/ep_x';
  await assert.rejects(sendMessage({ url, text: 'x' }), /令牌/);
  await assert.rejects(sendMessage({ url, token: 'x', text: 'x'.repeat(65537) }), /正文/);
  const dir = await directory(t);
  await writeFile(join(dir, 'a.exe'), 'not supported');
  await assert.rejects(sendMessage({ url, token: 'x', files: [join(dir, 'a.exe')] }), /只支持/);
});

test('CLI destination atomically overrides profile destination and ambiguous options fail', async (t) => {
  const config = join(await directory(t), 'profiles.json');
  await writeFile(config, JSON.stringify({ profiles: { default: { url: 'http://10.0.0.1:17890/hooks/v1/endpoints/ep_old' } } }));
  const options = await readOptions(['--config', config, '--base', 'http://192.168.1.2:17890', '--endpoint', 'ep_new', '--text', 'test'], { DSH_HOOK_TOKEN: 'x' });
  assert.equal(endpointUrl(options), 'http://192.168.1.2:17890/hooks/v1/endpoints/ep_new');
  assert.equal(options.url, undefined);
  await writeFile(config, JSON.stringify({ profiles: { default: { base: 'http://10.0.0.1:17890', endpointId: 'ep_old' } } }));
  const direct = 'http://192.168.1.3:17890/hooks/v1/endpoints/ep_direct';
  const overridden = await readOptions(['--config', config, '--url', direct], { DSH_HOOK_TOKEN: 'x' });
  assert.equal(endpointUrl(overridden), direct);
  assert.equal(overridden.base, undefined);
  assert.equal(overridden.endpointId, undefined);
  await assert.rejects(readOptions(['--url', direct, '--base', 'http://10.0.0.1', '--endpoint', 'ep_x']), /不能.*混用/);
  await assert.rejects(readOptions(['--config', config, '--endpoint', 'ep_new']), /必须一起/);
  await assert.rejects(readOptions(['--config', config, '--base', 'http://10.0.0.2']), /必须一起/);
});
