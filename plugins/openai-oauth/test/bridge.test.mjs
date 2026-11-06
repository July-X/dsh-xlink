// 桥接客户端的 401 语义：只有握手才把 401/403 当「令牌不对」；
// 目录请求的 401 是业务错误（例如授权失效），必须带出后端 body。
import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';

const { handshake, fetchCatalog } = await import(
  new URL('../host/bridge.js', import.meta.url)
);

/** 桩桥：固定状态码与 body，顺便确认请求确实带上了 Bearer 令牌。 */
async function stubBridge(status, body) {
  const server = createServer((request, response) => {
    assert.equal(request.headers.authorization, 'Bearer test-bridge');
    response.statusCode = status;
    response.setHeader('content-type', 'application/json');
    response.end(body);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return {
    bridge: { url: `http://127.0.0.1:${server.address().port}/`, token: 'test-bridge' },
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

test('握手 401：仍是「桥接拒绝了本次连接令牌」', async () => {
  const { bridge, close } = await stubBridge(401, '{"error":"unauthorized"}');
  try {
    await assert.rejects(handshake(bridge, '0.0.0-test'), /拒绝了本次连接令牌/);
  } finally {
    await close();
  }
});

test('目录 401：带出后端文案，不再说成连接令牌被拒', async () => {
  const { bridge, close } = await stubBridge(
    401,
    '{"error":"授权已失效（刷新令牌失效）；请重新登录"}',
  );
  try {
    await assert.rejects(
      fetchCatalog(bridge),
      (error) => {
        assert.doesNotMatch(error.message, /拒绝了本次连接令牌/);
        assert.match(error.message, /授权已失效/);
        return true;
      },
    );
  } finally {
    await close();
  }
});
