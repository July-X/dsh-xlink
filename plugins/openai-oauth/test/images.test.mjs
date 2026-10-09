import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareImageParts, MAX_REQUEST_IMAGE_BYTES } from '../host/images.js';
import { buildEnvelope } from '../host/request.js';

const bytes = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64');
const image = (id = 'one') => ({ type: 'image', attachment: {
  attachmentId: id, mediaType: 'image/png', width: 1, height: 1, bytes: bytes.length,
} });
const text = (value) => ({ type: 'text', text: value });
const catalog = { revisionCombined: 'rev|cap0', entries: [{ id: 'gpt-x' }] };
function services(reads = []) {
  return {
    attachments: { async readImageRequest(ref, target, signal) {
      reads.push({ ref, target, signal });
      return { data: bytes, mediaType: 'image/png', width: 1, height: 1 };
    } },
    handleText: (ref) => `image:${ref.attachmentId}`,
    offloadedText: (ref) => `offloaded:${ref.attachmentId}`,
    resolveAccess: () => undefined,
    requiredOffload: () => 0,
  };
}

test('图片与文字保持顺序，多轮重复引用只读取一次', async () => {
  const first = image();
  const second = image('two');
  const repeated = image();
  const messages = [
    { role: 'user', content: [text('看图'), first, text('比较'), second] },
    { role: 'assistant', content: [text('看到了')] },
    { role: 'user', content: [repeated, text('再解释一下')] },
  ];
  const reads = [];
  const parts = await prepareImageParts(messages, services(reads));
  const result = buildEnvelope({ model: 'gpt-x', messages }, catalog, parts);
  assert.equal(reads.length, 2);
  assert.deepEqual(result.payload.input[0].content.map((p) => p.type),
    ['input_text', 'input_text', 'input_image', 'input_text', 'input_text', 'input_image']);
  const transmitted = result.payload.input[0].content[2];
  assert.equal(transmitted.image_url, `data:image/png;base64,${bytes.toString('base64')}`);
  assert.equal(transmitted.detail, 'auto');
  assert.equal(result.payload.input[2].content[1].image_url, transmitted.image_url);
  assert.equal(first.attachment.attachmentId, 'one');
});

test('纯图片消息与工具返回图片都传送图片字节', async () => {
  const messages = [{ role: 'user', content: [image()] },
    { role: 'tool', toolCallId: 'call-1', content: [text('截图'), image('tool')] }];
  const parts = await prepareImageParts(messages, services());
  const input = buildEnvelope({ model: 'gpt-x', messages }, catalog, parts).payload.input;
  assert.equal(input[0].content[1].type, 'input_image');
  assert.equal(input[1].call_id, 'call-1');
  assert.equal(input[1].output[2].type, 'input_image');
});

test('已卸载图片只发送占位，不重新读取', async () => {
  const block = { ...image(), offloaded: true };
  const reads = [];
  const parts = await prepareImageParts([{ role: 'user', content: [block] }], services(reads));
  assert.equal(reads.length, 0);
  assert.deepEqual(parts.get(block), [{ type: 'input_text', text: 'offloaded:one' }]);
});

test('附件服务失败和取消均保留原始原因', async () => {
  const messages = [{ role: 'user', content: [image()] }];
  const failure = new Error('image checksum mismatch');
  const config = services();
  config.attachments.readImageRequest = async () => { throw failure; };
  await assert.rejects(prepareImageParts(messages, config), (error) => error === failure);
  const controller = new AbortController();
  controller.abort(failure);
  const reads = [];
  await assert.rejects(prepareImageParts(messages, services(reads), controller.signal), (error) => error === failure);
  assert.equal(reads.length, 0);
});

test('无效图片与缺失服务明确报错，纯文字无需附件服务', async () => {
  const messages = [{ role: 'user', content: [image()] }];
  await assert.rejects(prepareImageParts(messages, {}), { code: 'ATTACHMENT_UNAVAILABLE' });
  const config = services();
  config.attachments.readImageRequest = async () => ({ data: bytes, mediaType: 'text/html' });
  await assert.rejects(prepareImageParts(messages, config), { code: 'INVALID_IMAGE' });
  assert.equal((await prepareImageParts([{ role: 'user', content: [text('hi')] }], {})).size, 0);
});

test('读取请求图遵守像素预算并传入取消信号', async () => {
  const block = image();
  block.attachment.width = 8192;
  block.attachment.height = 4096;
  const reads = [];
  const controller = new AbortController();
  await prepareImageParts([{ role: 'user', content: [block] }], services(reads), controller.signal);
  assert(reads[0].target.width * reads[0].target.height <= 4 * 1024 * 1024);
  assert.equal(reads[0].target.maxBytes, 1024 * 1024);
  assert.equal(reads[0].signal, controller.signal);
});

test('总图片预算使用实际字节并交给内核的历史卸载策略', async () => {
  const config = services();
  config.requiredOffload = (messages, budget, length) => {
    assert.equal(budget.maxBytes, MAX_REQUEST_IMAGE_BYTES);
    assert.equal(budget.representation, 'base64');
    assert.equal(length(messages[0].content[0]), bytes.length);
    return 2;
  };
  await assert.rejects(prepareImageParts([{ role: 'user', content: [image()] }], config), {
    code: 'IMAGE_OFFLOAD_REQUIRED', offloadImages: 2,
  });
});
