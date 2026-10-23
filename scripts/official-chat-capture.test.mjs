import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import { createAdapter } from './lib/p1-adapter.mjs';

test('登录态探测区分明确未登录与端点、存储读取失败，不返回凭据', async () => {
  const script = readFileSync(new URL('../src-tauri/src/official_chat/page_ready_script.js', import.meta.url), 'utf8');
  const location = { href: 'https://chatgpt.com/', origin: 'https://chatgpt.com' };
  for (const [status, payload, missing, available] of [
    [200, {}, true, false], [401, {}, true, false],
    [503, {}, false, false], [200, { accessToken: 'PUBLIC_TEST_TOKEN' }, false, true],
  ]) {
    const spec = { source: 'endpoint', endpointUrl: '/api/auth/session', jsonKey: 'accessToken', origin: 'chatgpt.com' };
    const result = await vm.runInNewContext(script.replaceAll('__SPEC__', JSON.stringify(spec)), {
      document: { readyState: 'complete' }, location,
      fetch: async () => ({ ok: status === 200, status, json: async () => payload }),
    });
    assert.equal(result.credentialMissing, missing);
    assert.equal(result.hasCredential, available);
    assert.equal(JSON.stringify(result).includes('PUBLIC_TEST_TOKEN'), false);
  }
  for (const inaccessible of [false, true]) {
    const spec = { source: 'storage', storage: 'localStorage', key: 'token', origin: 'chatgpt.com' };
    const result = JSON.parse(vm.runInNewContext(script.replaceAll('__SPEC__', JSON.stringify(spec)), {
      document: { readyState: 'complete' }, location,
      window: { localStorage: { getItem() { if (inaccessible) throw new Error('denied'); return null; } } },
    }));
    assert.equal(result.credentialMissing, !inaccessible);
  }
});

test('协议适配器沿站点路径取得真实引用并完成落盘', async () => {
  const adapter = await createAdapter({ waitBudgetMs: 1000 });
  try {
    for (const sessionId of ['public-regression-a', 'public-regression-b']) {
      const picked = await adapter.pick({ sessionId });
      assert.match(picked.ref, /^ds~/);
      await adapter.settle(picked.ref);
      const input = await adapter.serialize(picked.ref);
      assert.equal(adapter.readSerializedBody(picked.ref, input), adapter.snapshotFile(picked.ref).body);
      assert.equal(adapter.readSerializedBody(picked.ref, 'WRONG FILE POINTER'), 'WRONG FILE POINTER');
    }
  } finally { adapter.cleanup(); }
});

for (const transport of ['fetch', 'xhr']) {
 for (const mode of ['ordinary', 'account', 'account-invalid']) {
  test(`MiniMax ${transport} ${mode} 忽略先到的 before 分页，读最新窗口并恢复钩子`, async () => {
    const target = '/minimax-cloud/api/v1/session/public-review/message';
    const unrelated = [
      `https://evil.test${target}?limit=80`,
      `${target}-summary?limit=80`,
      `/other?next=${target}`,
    ];
    const older = `${target}?before=100&limit=80`;
    const latest = `${target}?limit=80`;
    const body = (url) => mode === 'account-invalid' ? 'PRIVATE_TOKEN_NOT_JSON' : JSON.stringify({
      messages: [{ role: 'user', msg_content: url === latest ? 'LATEST' : 'WRONG_RESPONSE' }],
      data: { userInfo: { realUserID: url === latest ? 'LATEST' : 'WRONG_RESPONSE', email: 'private-test@example.test' } },
      token: 'TOKEN_MUST_STAY_IN_PAGE',
    });
    const requests = [];
    class Xhr {
      open(method, url) { this.url = url; }
      setRequestHeader() {}
      addEventListener(name, fn) { (this.listeners ??= {})[name] = fn; }
      send() {
        requests.push(this.url);
        setImmediate(() => {
          this.status = 200;
          this.responseText = body(this.url);
          this.listeners?.load?.call(this);
        });
      }
    }
    const originalFetch = (url) => {
      requests.push(url);
      return Promise.resolve(new Response(body(url), { status: 200 }));
    };
    const originalOpen = Xhr.prototype.open;
    const originalSend = Xhr.prototype.send;
    const originalHeader = Xhr.prototype.setRequestHeader;
    class Channel {
      constructor() {
        this.port1 = { close() {} };
        this.port2 = { close() {}, postMessage: () => setImmediate(() => this.port1.onmessage?.()) };
      }
    }
    const sandbox = {
      fetch: originalFetch, XMLHttpRequest: Xhr, Headers, Request, Response, URL,
      MessageChannel: Channel,
      document: { readyState: 'complete' },
      location: { href: 'https://agent.minimaxi.com/', origin: 'https://agent.minimaxi.com' },
      performance: { now: () => performance.now(), getEntriesByType: () => [] },
    };
    sandbox.window = sandbox;
    const spec = {
      catalog_path: target, without_query_key: 'before', replay_body: false, positive_control: false,
      ...(mode.startsWith('account') ? { identity_keys: ['data', 'userInfo', 'realUserID'] } : {}),
      poll_budget_ms: 1000, max_poll_attempts: 100000,
      sessions_path: ['sessions'], item_path: ['session'], id_path: ['session_id'], item_fields: {},
    };
    const source = readFileSync(new URL('../src-tauri/src/official_chat/page_hook_script.js', import.meta.url), 'utf8');
    const result = vm.runInNewContext(source.replace('__SPEC__', JSON.stringify(spec)), sandbox);
    setImmediate(() => {
      for (const url of [...unrelated, older, latest]) {
        if (transport === 'fetch') void sandbox.fetch(url);
        else { const xhr = new Xhr(); xhr.open('GET', url); xhr.send(); }
      }
    });
    const reading = await result;
    if (mode === 'account-invalid') {
      assert.equal(reading.ok, false);
      assert.equal(reading.stage, 'account-projection');
      assert.ok(!JSON.stringify(reading).includes('PRIVATE_TOKEN_NOT_JSON'));
      assert.equal(sandbox.fetch, originalFetch);
      assert.equal(Xhr.prototype.open, originalOpen);
      return;
    }
    assert.equal(reading.ok, true, JSON.stringify(reading));
    const captured = JSON.parse(reading.capturedBody.text);
    assert.equal(mode === 'account' ? captured.data.userInfo.realUserID : captured.messages[0].msg_content, 'LATEST');
    if (mode === 'account') {
      assert.ok(!JSON.stringify(reading).includes('TOKEN_MUST_STAY_IN_PAGE'));
      assert.ok(!JSON.stringify(reading).includes('private-test@example.test'));
    }
    assert.deepEqual(requests, [...unrelated, older, latest]);
    assert.equal(sandbox.fetch, originalFetch);
    assert.equal(Xhr.prototype.open, originalOpen);
    assert.equal(Xhr.prototype.send, originalSend);
    assert.equal(Xhr.prototype.setRequestHeader, originalHeader);
  });
}
}


test('账户端点在页内裁剪后才返回，认证令牌与邮箱不进入回程', async () => {
  const script = readFileSync(new URL('../src-tauri/src/official_chat/page_fetch_script.js', import.meta.url), 'utf8');
  const spec = { source: 'endpoint', endpointUrl: '/api/auth/session', jsonKey: 'accessToken',
    url: '/api/auth/session', header: 'authorization', prefix: 'Bearer ', identity_keys: ['user', 'id'] };
  const secret = { accessToken: 'TOKEN_MUST_STAY_IN_PAGE', sessionToken: 'PRIVATE_SESSION', user: { id: 'public-account', email: 'private-test@example.test' } };
  const result = await vm.runInNewContext(script.replaceAll('__SPEC__', JSON.stringify(spec)), {
    fetch: async () => Response.json(secret),
  });
  assert.equal(result.ok, true);
  assert.deepEqual(JSON.parse(result.body), { user: { id: 'public-account' } });
  for (const value of ['TOKEN_MUST_STAY_IN_PAGE', 'PRIVATE_SESSION', 'private-test@example.test']) {
    assert.ok(!JSON.stringify(result).includes(value));
  }
});


test('账户响应不是 JSON 时仅回固定错误，不把解析错误中的原文带出页面', async () => {
  const script = readFileSync(new URL('../src-tauri/src/official_chat/page_fetch_script.js', import.meta.url), 'utf8');
  const spec = { source: 'endpoint', endpointUrl: '/auth', jsonKey: 'accessToken',
    url: '/account', header: 'authorization', prefix: 'Bearer ', identity_keys: ['user', 'id'] };
  const result = await vm.runInNewContext(script.replaceAll('__SPEC__', JSON.stringify(spec)), {
    fetch: async (url) => url === '/auth'
      ? Response.json({ accessToken: 'TEST_ONLY' })
      : new Response('PRIVATE_TOKEN_NOT_JSON'),
  });
  assert.equal(result.ok, false);
  assert.equal(result.stage, 'account-projection');
  assert.ok(!JSON.stringify(result).includes('PRIVATE_TOKEN_NOT_JSON'));
});
