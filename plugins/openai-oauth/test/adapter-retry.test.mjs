// 目录 revision 过期（桥回 409）的恢复：刷新目录、用新 revision 重建信封，
// 只重试一次。桥接侧 409 的语义是「目录已更新，Host 刷新后重试」，而此前
// adapter 只把非 2xx 包成 BridgeUnavailableError，会话会一直发不出消息。
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
import { createServer } from 'node:http';

import { stagePlugin } from './fixtures.mjs';

const MESSAGES = [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }];

/**
 * 桩桥：`/v1/models` 每次给新 revision，`/v1/responses` 前 `failures` 次回 409，
 * 之后回一行成功终止包络。返回计数器与请求体，供断言「确实刷新过目录」。
 */
async function withBridge(failures, run) {
  const scratch = mkdtempSync(join(tmpdir(), 'oauth-adapter-retry-'));
  let server;
  const state = { catalogs: 0, inferences: 0, envelopes: [] };
  try {
    const plugin = stagePlugin(scratch);
    const { BridgeAdapter } = await import(pathToFileURL(join(plugin, 'host/adapter.js')));
    server = createServer(async (req, res) => {
      if (req.url === '/v1/models') {
        state.catalogs += 1;
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify({
          revision: `r${state.catalogs}`,
          models: [{ id: 'gpt-x', name: 'GPT X' }],
        }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      state.inferences += 1;
      state.envelopes.push(JSON.parse(Buffer.concat(chunks).toString('utf8')));
      if (state.inferences <= failures) {
        res.statusCode = 409;
        res.setHeader('content-type', 'application/json');
        res.end('{"error":"目录已更新，请刷新模型列表后重试"}');
        return;
      }
      res.end(`${JSON.stringify({ type: 'bridge.terminal', status: 'completed' })}\n`);
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const adapter = new BridgeAdapter(
      { url: `http://127.0.0.1:${server.address().port}`, token: 'test-bridge' },
      '0.0.0-test',
      { get: () => undefined },
    );
    await run(adapter, state);
  } finally {
    if (server) await new Promise((resolve) => server.close(resolve));
    rmSync(scratch, { recursive: true, force: true });
  }
}

async function drain(adapter) {
  const chunks = [];
  for await (const chunk of adapter.stream({ model: 'gpt-x', messages: MESSAGES })) chunks.push(chunk);
  return chunks;
}

test('409：刷新目录后用新 revision 重试一次', async () => {
  await withBridge(1, async (adapter, state) => {
    const chunks = await drain(adapter);
    assert.equal(state.inferences, 2, '应重试一次');
    assert.equal(state.catalogs, 2, '重试前必须重新拉目录（revision 变了）');
    assert.equal(state.envelopes[1].catalogRevision, 'r2', '重试的信封必须带新 revision');
    assert.equal(chunks.at(-1).type, 'finish');
  });
});

test('409 持续：只重试一次，把错误抛给内核', async () => {
  await withBridge(99, async (adapter, state) => {
    await assert.rejects(drain(adapter), /桥接返回 409/);
    assert.equal(state.inferences, 2, '不能无限重试');
    assert.equal(state.catalogs, 2);
  });
});
