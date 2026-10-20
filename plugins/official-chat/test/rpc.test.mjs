// **传输层**（协议 §5 v0.4 的 exact fetch route）的判据。
//
// ## 判什么
//
// · **信封层**（`createFetchRouteHandler`）：`client-request` → `server-response`
//   的解析、校验与回包；非法信封 → 400 + 传输层错误码。
// · **派发层**（`createDispatcher`）：端点名 → 命令；**命令自己失败 ⇒ 外层失败**
//   （失败不许被包成「成功值」）。
//
// ## 为什么与命令层分开测
//
// 命令层是**纯业务**（不碰传输），传输层是**信封与路由**。分开测，才能在
// 「传输换了」（v0.2 的 RPC → v0.4 的 exact fetch route）时，业务判据一条都不用改。
//
// ## 判据不替真机跑
//
// 这里证的是「信封与派发的形状对不对」。**真机上这条通道能不能通**由真机复验轮
// 执行（仓库级 runner 跑不了真实内核进程与真实 cordis 树）。

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  ENDPOINTS,
  FETCH_ROUTE_PATH,
  createDispatcher,
  createFetchRouteHandler,
  toRpcError,
} from '../host/rpc.js';

/** 与 host 半边注册的 exact route 同一个 pathname（同源请求）。 */
const ROUTE_URL = `http://127.0.0.1:51700${FETCH_ROUTE_PATH}`;

/**
 * 造一个内核那侧的请求。
 *
 * @param {object|string} body - 信封内容（字符串则原样当 body）。
 * @param {{method?: string}} [options]
 * @returns {Request}
 */
function makeRequest(body, { method = 'POST' } = {}) {
  return new Request(ROUTE_URL, {
    method,
    headers: { 'content-type': 'application/json' },
    body: typeof body === 'string' ? body : JSON.stringify(body),
  });
}

/**
 * 造一套假命令（返回命令层自己的形态）。
 *
 * @param {object} [overrides] - 覆盖某个命令。
 * @returns {object}
 */
function fakeCommands(overrides = {}) {
  return {
    capabilities: () => ({ ok: true, schemaVersion: 1, providerId: 'ds' }),
    catalog: async () => ({ ok: true, page: { items: [] }, allocations: {} }),
    capture: async () => ({ ok: true, snapshot: { snapshotId: 'ds~aaaa~0001' } }),
    cancel: () => ({ ok: true, cancelled: false }),
    preview: () => ({ ok: true, snapshot: { snapshotId: 'ds~aaaa~0001' }, body: '正文', state: 'ready' }),
    ...overrides,
  };
}

test('通道路径在 `/api` 命名空间下（内核 assertFetchRoute 的硬要求）', () => {
  assert.equal(FETCH_ROUTE_PATH, '/api/xlink-official-chat');
  assert.ok(
    FETCH_ROUTE_PATH.startsWith('/api/'),
    'exact route 不在 /api 下，内核 assertFetchRoute 会直接抛',
  );
  assert.deepEqual([...ENDPOINTS], ['capabilities', 'catalog', 'capture', 'cancel', 'preview']);
});

test('成功的命令 → server-response 信封（result.ok=true，value 就是命令返回值）', async () => {
  const handler = createFetchRouteHandler({ dispatch: createDispatcher({ commands: fakeCommands() }) });
  const response = await handler(
    makeRequest({ type: 'client-request', rpcId: 'r-1', method: 'capabilities', payload: {} }),
  );
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.type, 'server-response');
  assert.equal(body.rpcId, 'r-1', 'rpcId 必须原样回传（对端拿它关联响应）');
  assert.equal(body.result.ok, true);
  assert.equal(body.result.value.schemaVersion, 1);
});

test('命令自己失败 → **外层** ok:false（失败不许被包成成功值）', async () => {
  const commands = fakeCommands({
    preview: () => ({
      ok: false,
      code: 'snapshot_invalid',
      retryable: false,
      userNext: '移除该引用',
      requestId: 'r-9',
    }),
  });
  const handler = createFetchRouteHandler({ dispatch: createDispatcher({ commands }) });
  const response = await handler(
    makeRequest({ type: 'client-request', rpcId: 'r-2', method: 'preview', payload: {} }),
  );
  assert.equal(response.status, 200, '命令级失败是 200 + 外层失败，不是 HTTP 错误');
  const body = await response.json();
  assert.equal(body.result.ok, false, '失败必须落在外层——包成 ok:true 就是「吞失败」');
  assert.equal(body.result.error.code, 'snapshot_invalid');
  assert.equal(body.result.error.details.userNext, '移除该引用');
  assert.equal(body.result.error.details.requestId, 'r-9');
});

test('未知端点 → 明确失败，不许被当成成功值', async () => {
  const handler = createFetchRouteHandler({ dispatch: createDispatcher({ commands: fakeCommands() }) });
  const response = await handler(
    makeRequest({ type: 'client-request', rpcId: 'r-3', method: 'no-such-endpoint', payload: {} }),
  );
  const body = await response.json();
  assert.equal(body.result.ok, false);
  assert.equal(body.result.error.code, 'source_unsupported');
});

test('命令层抛异常 → 外层失败，**异常串不进返回值**（协议 §3）', async () => {
  const commands = fakeCommands({
    catalog: async () => {
      throw new Error('ENOENT: /srv/secret/path.json (permission denied)');
    },
  });
  const handler = createFetchRouteHandler({ dispatch: createDispatcher({ commands }) });
  const response = await handler(
    makeRequest({ type: 'client-request', rpcId: 'r-4', method: 'catalog', payload: {} }),
  );
  const body = await response.json();
  assert.equal(body.result.ok, false);
  assert.equal(body.result.error.code, 'source_unsupported');
  assert.ok(
    !/permission denied|\/srv\/secret/.test(JSON.stringify(body)),
    '内部异常串不许外泄',
  );
});

test('信封不合法 → 400 + 传输层错误码（**不进协议 §3 的 13 码闭集**）', async () => {
  const handler = createFetchRouteHandler({ dispatch: createDispatcher({ commands: fakeCommands() }) });
  for (const [label, body] of [
    ['type 不对', { type: 'not-a-client-request', rpcId: 'r-5', method: 'catalog' }],
    ['rpcId 缺失', { type: 'client-request', method: 'catalog' }],
    ['method 形状不合法', { type: 'client-request', rpcId: 'r-5', method: 'bad method' }],
  ]) {
    const response = await handler(makeRequest(body));
    assert.equal(response.status, 400, `${label} 必须是 400`);
    const parsed = await response.json();
    assert.equal(parsed.result.ok, false);
    assert.equal(parsed.result.error.code, 'gateway/bad-request', `${label} 用传输层错误码`);
    assert.ok(parsed.result.error.details.issues.length > 0, `${label} 必须说清哪里不对`);
  }
});

test('body 不是 JSON → 400', async () => {
  const handler = createFetchRouteHandler({ dispatch: createDispatcher({ commands: fakeCommands() }) });
  const response = await handler(makeRequest('not json at all'));
  assert.equal(response.status, 400);
  const parsed = await response.json();
  assert.equal(parsed.result.error.code, 'gateway/bad-request');
});

test('错误翻译：稳定码与用户下一步进 `details`，异常串不进 `message`', () => {
  const translated = toRpcError({
    code: 'timeout',
    retryable: true,
    userNext: '重试',
    requestId: 'r-1',
    detail: 'snapshot_exists',
  });
  assert.equal(translated.code, 'timeout');
  assert.equal(translated.details.retryable, true);
  assert.equal(translated.details.requestId, 'r-1');
  assert.equal(translated.details.detail, 'snapshot_exists', '原始码要穿过传输边界（协议 §3）');
  assert.ok(!/Error|at .*\.js:\d+/.test(translated.message), '异常串不许进 message');
});
