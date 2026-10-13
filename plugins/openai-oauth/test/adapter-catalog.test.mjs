// 目录 → 会话模型列表的映射（`selected` 过滤）回归。
//
// 背景：账号目录里混着几十条实验模型，会话选择器此前把它们全列出来。现在
// 壳在 `/v1/models` 载荷上多带一个 `selected`（用户在设置页勾选的常用模型），
// Host 适配器用它过滤 `listModels`。三条语义**不能只靠代码读一遍**：
//   ① 只有 `selected` 里的条目进列表；
//   ② `resolveModel` **不受过滤影响**——取消勾选不该让正在用该模型的会话
//      报 MODEL_NOT_IN_CATALOG（那条会话的报错出现在别处，原因在设置页）；
//   ③ 载荷不带 / 空 `selected` 时不过滤（旧壳与冒烟桩的兼容路径）。
import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { stagePlugin } from './fixtures.mjs';

/** 桩桥接：一个固定目录载荷；`/v1/handshake` 只是为了贴近真实契约。 */
async function stubBridge(catalog) {
  const server = createServer((request, response) => {
    assert.equal(request.headers.authorization, 'Bearer test-bridge');
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify(catalog));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return {
    bridge: { url: `http://127.0.0.1:${server.address().port}/`, token: 'test-bridge' },
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

const CATALOG = {
  revision: 'r1|cap0',
  selected: ['gpt-6-astra'],
  models: [
    { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol', contextWindow: 272000, efforts: [{ id: 'low', name: 'low' }] },
    { id: 'gpt-6-astra', name: 'GPT-6 Astra' },
    { id: 'gpt-reserve', name: 'Reserve' },
  ],
};

async function withAdapter(catalog, run) {
  const scratch = mkdtempSync(join(tmpdir(), 'oauth-adapter-'));
  const { bridge, close } = await stubBridge(catalog);
  try {
    const dir = stagePlugin(scratch);
    const { BridgeAdapter } = await import(pathToFileURL(join(dir, 'host', 'adapter.js')));
    await run(new BridgeAdapter(bridge, '0.0.0-test', { get: () => undefined }));
  } finally {
    await close();
    rmSync(scratch, { recursive: true, force: true });
  }
}

test('目录：只把 selected 里的模型交给会话选择器', async () => {
  await withAdapter(CATALOG, async (adapter) => {
    const models = await adapter.listModels('xlink-openai-chatgpt');
    assert.deepEqual(models.map((model) => model.id), ['gpt-6-astra']);
    assert.equal(models[0].name, 'GPT-6 Astra');
    assert.deepEqual(models[0].inputModalities, ['text', 'image']);

    // 未勾选的模型仍可解析（含上下文容量与强度档位）——这是「取消勾选」与
    // 「模型不存在」的分界线。
    const resolved = await adapter.resolveModel('xlink-openai-chatgpt', 'gpt-6.1-sol');
    assert.equal(resolved.name, 'GPT-6.1 Sol');
    assert.equal(resolved.context.contextWindow, 272000);
    assert.deepEqual(resolved.reasoning.efforts.map((effort) => effort.id), ['low']);
    const missing = await adapter
      .resolveModel('xlink-openai-chatgpt', 'gpt-not-in-catalog')
      .then(() => null, (error) => error);
    assert.equal(missing?.code, 'MODEL_NOT_IN_CATALOG');
  });
});

test('目录：载荷不带 selected（旧壳 / 冒烟桩）或 selected 为空时不过滤', async () => {
  const { selected, ...withoutSelection } = CATALOG;
  assert.deepEqual(selected, ['gpt-6-astra'], '夹具前提：原始载荷确实带着 selected');
  await withAdapter(withoutSelection, async (adapter) => {
    assert.deepEqual(
      (await adapter.listModels('xlink-openai-chatgpt')).map((model) => model.id),
      ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-reserve'],
    );
  });
  await withAdapter({ ...CATALOG, selected: [] }, async (adapter) => {
    assert.deepEqual(
      (await adapter.listModels('xlink-openai-chatgpt')).map((model) => model.id),
      ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-reserve'],
      '空集合不是「一条都不列」：空集只可能来自壳的空目录，那种情况下 models 本身也是空的',
    );
  });
});
