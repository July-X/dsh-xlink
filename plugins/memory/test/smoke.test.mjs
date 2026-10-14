import assert from 'node:assert/strict';
import test from 'node:test';

import { TOKEN_ENV, URL_ENV, buildPath, callMemory, readCredentials } from '../host/bridge.js';
import {
  MAX_INJECTIONS_PER_SESSION,
  applyFailure,
  applyResult,
  shouldRefresh,
  userText,
} from '../host/policy.js';

/**
 * 这一半没有外部依赖，所以能真的跑：编解码、错误分支与状态机都是纯函数。
 * 装配层（`host/index.js`）反过来只留「接线」，它的问题在那边的注释里写明——
 * 这里测不到的（agent.ctx 注册、pre-step 顺序）只能在真机启动时验。
 */

test('凭据两样缺一就等于没有桥', () => {
  assert.deepEqual(readCredentials({ [URL_ENV]: 'http://127.0.0.1:1', [TOKEN_ENV]: 't' }), {
    url: 'http://127.0.0.1:1',
    token: 't',
  });
  assert.equal(readCredentials({ [URL_ENV]: 'http://127.0.0.1:1' }), null);
  assert.equal(readCredentials({ [TOKEN_ENV]: 't' }), null);
  assert.equal(readCredentials({ [URL_ENV]: '', [TOKEN_ENV]: '' }), null);
  assert.equal(readCredentials({ [URL_ENV]: 1, [TOKEN_ENV]: 't' }), null);
  assert.equal(readCredentials({}), null);
});

test('路径拼装：中文与空格必须编码，空值不进查询串', () => {
  const path = buildPath('context', { q: '构建 命令', workspace: '/tmp/my repo', empty: '' });
  assert.match(path, /^\/v1\/memory\/context\?/);
  assert.ok(!path.includes('empty='));
  const query = new URLSearchParams(path.split('?')[1]);
  assert.equal(query.get('q'), '构建 命令');
  assert.equal(query.get('workspace'), '/tmp/my repo');
  // 原始路径里不该出现裸中文（桥那边按 URL 解析，裸字节会被截断）。
  assert.ok(!/[^\x00-\x7f]/.test(path), path);
  assert.equal(buildPath('propose', {}), '/v1/memory/propose');
});

test('没有凭据时不发请求，直接给出可读原因', async () => {
  let called = 0;
  const result = await callMemory(
    { credentials: null, route: 'context', params: {} },
    () => {
      called += 1;
      return Promise.resolve({ status: 200, body: '{}' });
    }
  );
  assert.equal(called, 0);
  assert.equal(result.ok, false);
  assert.match(result.reason, /记忆桥/);
});

test('成功路径把 JSON 交给调用方', async () => {
  const result = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: { q: '中文' } },
    async ({ path, token }) => {
      assert.equal(token, 't');
      const query = new URLSearchParams(path.split('?')[1]);
      assert.equal(query.get('q'), '中文');
      return { status: 200, body: JSON.stringify({ enabled: true, text: '一条记忆', hits: [] }) };
    }
  );
  assert.equal(result.ok, true);
  assert.equal(result.value.text, '一条记忆');
});

test('业务错误原样透出，不抛异常', async () => {
  const unauthorized = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 'stale' }, route: 'context', params: {} },
    async () => ({ status: 401, body: JSON.stringify({ error: 'unauthorized' }) })
  );
  assert.equal(unauthorized.ok, false);
  assert.equal(unauthorized.reason, 'unauthorized');

  const failed = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'read', params: { id: 'x' } },
    async () => ({ status: 404, body: JSON.stringify({ error: '找不到记忆 x' }) })
  );
  assert.equal(failed.ok, false);
  assert.match(failed.reason, /找不到记忆/);

  const garbled = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: {} },
    async () => ({ status: 200, body: '<html>' })
  );
  assert.equal(garbled.ok, false);
  assert.match(garbled.reason, /无法解析|HTTP 200/);
});

test('连不上桥与超时都收敛成 ok:false', async () => {
  const offline = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: {} },
    () => Promise.reject(new Error('connect ECONNREFUSED'))
  );
  assert.equal(offline.ok, false);
  assert.match(offline.reason, /无法连接记忆桥/);

  const timeout = await callMemory(
    { credentials: { url: 'http://127.0.0.1:1', token: 't' }, route: 'context', params: {} },
    () => Promise.reject(new Error('桥在 1500ms 内没有响应'))
  );
  assert.equal(timeout.ok, false);
  assert.match(timeout.reason, /1500ms/);
});

test('查询文本只认用户消息：别的注入消息不当查询', () => {
  const messages = [
    { source: { kind: 'agent-instructions' }, content: [{ type: 'text', text: '工作区指令' }] },
    { source: { kind: 'user' }, content: [{ type: 'text', text: '  修一下构建  ' }] },
    { source: { kind: 'runtime-context' }, content: [{ type: 'text', text: '运行上下文' }] },
  ];
  assert.equal(userText(messages), '修一下构建');
  assert.equal(userText([]), '');
  assert.equal(userText(undefined), '');
  assert.equal(userText([{ source: { kind: 'user' }, content: [] }]), '');
});

test('每会话最多注入三次，同回合不重复检索', () => {
  let state = { turn: -1, text: '', injections: 0 };
  assert.equal(shouldRefresh(state, 1), true);
  state = applyResult(state, 1, '第一条');
  assert.equal(state.injections, 1);
  assert.equal(shouldRefresh(state, 1), false, '同一回合只检索一次');
  assert.equal(shouldRefresh(state, 2), true);

  state = applyResult(state, 2, '第二条');
  state = applyResult(state, 3, '第三条');
  assert.equal(state.injections, MAX_INJECTIONS_PER_SESSION);
  assert.equal(shouldRefresh(state, 4), false, '到顶后不再刷新');
});

test('文本没变不算一次注入；失败清空缓存而不是沿用', () => {
  let state = applyResult({ turn: -1, text: '', injections: 0 }, 1, '同一条');
  assert.equal(state.injections, 1);
  state = applyResult(state, 2, '同一条');
  assert.equal(state.injections, 1, '内容相同不会新增内核快照，也不该消耗配额');

  // 停用/删除/桥挂掉之后：本回合看到的是空，而不是上一次的内容。
  const failed = applyFailure({ turn: 2, text: '旧记忆', injections: 1 }, 3);
  assert.equal(failed.text, '');
  assert.equal(failed.turn, 3);
});
