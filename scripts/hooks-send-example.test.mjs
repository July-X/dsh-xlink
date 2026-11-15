import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { parseArgs, sendExample } from './hooks-send-example.mjs';

function listen(handler, run) {
  return new Promise((resolve, reject) => {
    const server = createServer(handler);
    server.once('error', reject);
    server.listen(0, '127.0.0.1', async () => {
      const { port } = server.address();
      try {
        await run('http://127.0.0.1:' + port);
        resolve();
      } catch (error) {
        reject(error);
      } finally {
        server.close();
      }
    });
  });
}

function json(response, status, body) {
  response.writeHead(status, { 'content-type': 'application/json' });
  response.end(JSON.stringify(body));
}

test('文本示例一次提交，带 Bearer 且不把令牌写进负载', async () => {
  const calls = [];
  await listen(
    (request, response) => {
      let body = '';
      request.setEncoding('utf8');
      request.on('data', (chunk) => { body += chunk; });
      request.on('end', () => {
        calls.push({ method: request.method, url: request.url, headers: request.headers, body });
        json(response, 201, { event_id: 'evt_1', state: 'ready', receipt_url: '/receipt/1' });
      });
    },
    async (base) => {
      const result = await sendExample({
        base,
        endpointId: 'ep_1',
        token: 'secret-token',
        kind: 'text',
        externalId: 'example-1',
      });
      assert.equal(result.eventId, 'evt_1');
      assert.equal(result.state, 'ready');
    }
  );
  assert.equal(calls.length, 1);
  assert.equal(calls[0].method, 'POST');
  assert.equal(calls[0].url, '/hooks/v1/endpoints/ep_1/events');
  assert.equal(calls[0].headers.authorization, 'Bearer secret-token');
  const body = JSON.parse(calls[0].body);
  assert.equal(body.external_id, 'example-1');
  assert.equal(body.commit, true);
  assert.equal(body.attachments, undefined);
  assert.doesNotMatch(calls[0].body, /secret-token/);
});

test('图片示例按 创建 -> 上传 -> commit -> receipt 走完，并带哈希头', async () => {
  const calls = [];
  await listen(
    (request, response) => {
      const chunks = [];
      request.on('data', (chunk) => chunks.push(chunk));
      request.on('end', () => {
        const body = Buffer.concat(chunks);
        calls.push({ method: request.method, url: request.url, headers: request.headers, body });
        if (request.method === 'POST' && request.url.endsWith('/events')) {
          json(response, 201, {
            event_id: 'evt_img',
            state: 'receiving',
            attachments: [{ attachment_id: 'att_1', client_id: 'sample' }],
          });
        } else if (request.method === 'PUT') {
          response.writeHead(200);
          response.end('{}');
        } else if (request.method === 'POST') {
          json(response, 200, { event_id: 'evt_img', state: 'ready', receipt_url: '/r' });
        } else {
          json(response, 200, { event_id: 'evt_img', state: 'ready' });
        }
      });
    },
    async (base) => {
      const result = await sendExample({
        base,
        endpointId: 'ep_1',
        token: 'secret-token',
        kind: 'image',
        externalId: 'example-image',
      });
      assert.equal(result.eventId, 'evt_img');
      assert.equal(result.state, 'ready');
    }
  );
  assert.deepEqual(
    calls.map((call) => call.method),
    ['POST', 'PUT', 'POST', 'GET']
  );
  const upload = calls[1];
  assert.equal(upload.headers['content-type'], 'image/png');
  assert.equal(
    upload.headers['x-content-sha256'],
    createHash('sha256').update(upload.body).digest('hex')
  );
  assert.ok(upload.body.length > 0);
});

test('409 是一次性冲突，不重试', async () => {
  let calls = 0;
  await listen(
    (request, response) => {
      calls += 1;
      request.resume();
      request.on('end', () => {
        json(response, 409, { error: { code: 'event_conflict', message: '内容不一致' } });
      });
    },
    async (base) => {
      await assert.rejects(
        () =>
          sendExample({
            base,
            endpointId: 'ep_1',
            token: 'secret-token',
            kind: 'text',
            externalId: 'example-conflict',
          }),
        (error) => error.status === 409 && /event_conflict/.test(error.message)
      );
    }
  );
  assert.equal(calls, 1, '409 重试只会再冲突，不能盲重试');
});

test('429 按 Retry-After 退避后重试', async () => {
  let calls = 0;
  await listen(
    (request, response) => {
      calls += 1;
      request.resume();
      request.on('end', () => {
        if (calls === 1) {
          response.writeHead(429, { 'content-type': 'application/json', 'retry-after': '0' });
          response.end(JSON.stringify({ error: { code: 'rate_limited', message: '稍后重试' } }));
          return;
        }
        json(response, 201, { event_id: 'evt_retry', state: 'ready' });
      });
    },
    async (base) => {
      const result = await sendExample({
        base,
        endpointId: 'ep_1',
        token: 'secret-token',
        kind: 'text',
        externalId: 'example-retry',
      });
      assert.equal(result.eventId, 'evt_retry');
    }
  );
  assert.equal(calls, 2);
});

test('参数解析拒绝未知参数、缺值与缺令牌', async () => {
  assert.throws(() => parseArgs(['--nope', 'x']), /未知参数/);
  assert.throws(() => parseArgs(['--endpoint']), /缺少值/);
  assert.throws(() => parseArgs(['--endpoint', '--kind']), /缺少值/);
  await assert.rejects(
    () => sendExample({ endpointId: 'ep_1', kind: 'text' }),
    /缺少令牌/
  );
  assert.equal(parseArgs(['--endpoint', 'ep_1', '--kind', 'video']).kind, 'video');
  await assert.rejects(
    () => sendExample({ endpointId: 'ep_1', token: 't', kind: 'video' }),
    /只支持/
  );
});
