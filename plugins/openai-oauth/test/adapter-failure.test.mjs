// 适配器层的失败翻译与用量透传（2026-10-19 额度事故的两条修复的边界测试）。
//
// 为什么需要它：`request.js` 只给**路由级** code（`QUOTA` / `UPSTREAM_FAILED`），
// 而内核只信任 `HarnessError` 的 code——普通 Error 一律被 `normalizeLlmFailure`
// 记成 `UNKNOWN`，UI 的额度提示与管理入口、重试策略都认不出来。这条翻译在
// `adapter.js` 的流边界上，单测 `request.test.mjs` 覆盖不到。
//
// 用 fixtures 的桩 peer（不需要真内核），只起一个桩桥接 HTTP 服务。
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
import { createServer } from 'node:http';

import { stagePlugin } from './fixtures.mjs';

const QUOTA_DETAIL =
  'The ChatGPT user has reached their Subscription Sharing usage limit. '
  + 'Ask the user to try again after their usage limit resets or use an API key instead.';

/** 起一个桩桥接（/v1/models + /v1/responses 回一行终止包络），跑完清理。 */
async function withBridge(terminal, run) {
  const scratch = mkdtempSync(join(tmpdir(), 'oauth-adapter-'));
  let server;
  try {
    const plugin = stagePlugin(scratch);
    const { BridgeAdapter } = await import(pathToFileURL(join(plugin, 'host/adapter.js')));
    const { LlmError } = await import(
      pathToFileURL(join(plugin, 'node_modules/@deepseek-ai/dsh-llm/index.js'))
    );
    server = createServer(async (req, res) => {
      if (req.url === '/v1/models') {
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify({ revision: 'r', models: [{ id: 'gpt-x', name: 'GPT X' }] }));
        return;
      }
      for await (const chunk of req) void chunk;
      res.end(`${JSON.stringify(terminal)}\n`);
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const adapter = new BridgeAdapter(
      { url: `http://127.0.0.1:${server.address().port}`, token: 'test-bridge' },
      '0.0.0-test',
      { get: () => undefined },
    );
    await run(adapter, LlmError);
  } finally {
    if (server) await new Promise((resolve) => server.close(resolve));
    rmSync(scratch, { recursive: true, force: true });
  }
}

const MESSAGES = [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }];

async function drain(adapter) {
  const chunks = [];
  for await (const chunk of adapter.stream({ model: 'gpt-x', messages: MESSAGES })) chunks.push(chunk);
  return chunks;
}

test('额度失败 → 内核 LlmError(code=QUOTA)，UI 才拿得到额度提示', async () => {
  await withBridge({ type: 'bridge.terminal', status: 'failed', detail: QUOTA_DETAIL }, async (adapter, LlmError) => {
    await assert.rejects(drain(adapter), (error) => {
      assert.equal(error instanceof LlmError, true, '必须是内核错误类型，普通 Error 会被记成 UNKNOWN');
      assert.equal(error.code, 'QUOTA');
      assert.match(error.message, /Subscription Sharing usage limit/);
      return true;
    });
  });
});

test('非额度失败保持原样（内核仍记 UNKNOWN，与修复前一致）', async () => {
  await withBridge({ type: 'bridge.terminal', status: 'failed', detail: 'Rate limit reached. Try again later.' }, async (adapter, LlmError) => {
    await assert.rejects(drain(adapter), (error) => {
      assert.equal(error instanceof LlmError, false);
      assert.equal(error.code, 'UPSTREAM_FAILED');
      return true;
    });
  });
});

test('用量透传：缓存输入按互斥桶进 usage 块', async () => {
  const terminal = {
    type: 'bridge.terminal',
    status: 'completed',
    replay: {
      response: {
        id: 'r-usage',
        usage: {
          input_tokens: 111774,
          output_tokens: 75,
          total_tokens: 111849,
          input_tokens_details: { cached_tokens: 93361, cache_write_tokens: 0 },
          output_tokens_details: { reasoning_tokens: 30 },
        },
      },
    },
  };
  await withBridge(terminal, async (adapter) => {
    const chunks = await drain(adapter);
    const usage = chunks.find((chunk) => chunk.type === 'usage').usage;
    assert.equal(usage.inputTokens, 18413, '未缓存输入 = input_tokens - cached_tokens');
    assert.equal(usage.cacheReadTokens, 93361);
    assert.equal(usage.reasoningTokens, 30);
    assert.equal(chunks.at(-1).type, 'finish');
  });
});
