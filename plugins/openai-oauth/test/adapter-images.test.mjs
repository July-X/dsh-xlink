// 可选内核兼容验证：只读已安装代码，插件副本与所有夹具均在临时目录。
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, cpSync, mkdirSync, symlinkSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const kernel = process.env.DSH_OAUTH_TEST_KERNEL;
test('实际内核适配器将附件字节发往桥接，并保留多轮图片', { skip: !kernel }, async () => {
  const scratch = mkdtempSync(join(tmpdir(), 'oauth-images-'));
  let server;
  try {
    cpSync(fileURLToPath(new URL('../host', import.meta.url)), join(scratch, 'host'), { recursive: true });
    mkdirSync(join(scratch, 'node_modules/@deepseek-ai'), { recursive: true });
    symlinkSync(join(kernel, 'node_modules/@deepseek-ai/dsh-llm'),
      join(scratch, 'node_modules/@deepseek-ai/dsh-llm'), process.platform === 'win32' ? 'junction' : 'dir');
    const { BridgeAdapter } = await import(pathToFileURL(join(scratch, 'host/adapter.js')));
    const received = [];
    server = createServer(async (req, res) => {
      assert.equal(req.headers.authorization, 'Bearer test-bridge');
      if (req.url === '/v1/models') {
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify({ revision: 'r', models: [{ id: 'gpt-x', name: 'GPT X' }] }));
        return;
      }
      let body = '';
      for await (const chunk of req) body += chunk;
      received.push(JSON.parse(body));
      res.end(JSON.stringify({ type: 'bridge.terminal', status: 'completed', replay: { response: {} } }) + '\n');
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const data = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64');
    let reads = 0;
    const attachments = {
      imageHostPath: () => undefined,
      async readImageRequest(ref) {
        reads++;
        return { attachment: ref, data, mediaType: 'image/png', width: 1, height: 1 };
      },
    };
    const adapter = new BridgeAdapter({ url: `http://127.0.0.1:${server.address().port}`, token: 'test-bridge' },
      '0.1.0', { get: (name) => name === 'attachments' ? attachments : undefined });
    assert.deepEqual((await adapter.resolveModel('xlink-openai-chatgpt', 'gpt-x')).inputModalities, ['text', 'image']);
    const messages = [{ role: 'user', content: [{ type: 'image', attachment: {
      attachmentId: 'a'.repeat(64), mediaType: 'image/png', bytes: data.length, width: 1, height: 1,
    } }] }];
    for await (const chunk of adapter.stream({ model: 'gpt-x', messages })) assert.equal(chunk.type, 'finish');
    messages.push({ role: 'assistant', content: [{ type: 'text', text: '看到了图片' }] },
      { role: 'user', content: [{ type: 'text', text: '继续解释' }] });
    for await (const chunk of adapter.stream({ model: 'gpt-x', messages })) assert.equal(chunk.type, 'finish');
    assert.equal(reads, 2);
    for (const envelope of received) {
      const image = envelope.payload.input[0].content.find((part) => part.type === 'input_image');
      assert.equal(image.image_url, `data:image/png;base64,${data.toString('base64')}`);
    }
    assert.equal(received.length, 2);
  } finally {
    if (server) await new Promise((resolve) => server.close(resolve));
    rmSync(scratch, { recursive: true, force: true });
  }
});
