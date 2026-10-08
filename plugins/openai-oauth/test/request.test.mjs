// request.js 的单元测试：信封组装（白名单/指令/工具/强度）与 NDJSON 流
// 转换（块生命周期/终止三分类/usage）。纯函数，无需内核与桥接。
import test from 'node:test';
import assert from 'node:assert/strict';

import { buildEnvelope, pumpStream, toProviderToolName } from '../host/request.js';

const CATALOG = {
  revisionCombined: 'rev-1|cap0',
  entries: [
    { id: 'gpt-x', name: 'GPT X', efforts: ['low', 'high'] },
    { id: 'gpt-plain', name: 'GPT Plain' },
  ],
};

test('envelope: 基本形状与双 revision 透传', () => {
  const envelope = buildEnvelope(
    { model: 'gpt-x', messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }] },
    CATALOG,
  );
  assert.equal(envelope.model, 'gpt-x');
  assert.equal(envelope.catalogRevision, 'rev-1|cap0');
  assert.deepEqual(envelope.payload.input, [
    { type: 'message', role: 'user', content: [{ type: 'input_text', text: 'hi' }] },
  ]);
  // 未显式选强度 → 不发送 reasoning 字段（「模型默认」= 省略）。
  assert(!('reasoningEffort' in envelope));
  assert(!('reasoning' in envelope.payload));
});

test('envelope: 不支持参数显式拒绝', () => {
  for (const [field, value] of [['temperature', 0.7], ['maxTokens', 100], ['stop', ['x']]]) {
    assert.throws(
      () => buildEnvelope({ model: 'gpt-x', messages: [], [field]: value }, CATALOG),
      /不支持配置/,
      field,
    );
  }
});

test('envelope: 显式强度进线格式与信封双处', () => {
  const envelope = buildEnvelope(
    {
      model: 'gpt-x',
      reasoningEffort: 'high',
      messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }],
    },
    CATALOG,
  );
  assert.equal(envelope.reasoningEffort, 'high');
  assert.deepEqual(envelope.payload.reasoning, { effort: 'high' });
  // 未选择时两处都不出现。
  const plain = buildEnvelope(
    { model: 'gpt-x', messages: [{ role: 'user', content: [{ type: 'text', text: 'hi' }] }] },
    CATALOG,
  );
  assert(!('reasoningEffort' in plain));
  assert(!('reasoning' in plain.payload));
});

test('envelope: 未知模型拒绝', () => {
  assert.throws(
    () => buildEnvelope({ model: 'gpt-none', messages: [] }, CATALOG),
    /不在当前账号目录/,
  );
});

test('envelope: system 文本进 instructions；工具结果与调用成对', () => {
  const envelope = buildEnvelope(
    {
      model: 'gpt-x',
      messages: [
        { role: 'system', content: [{ type: 'text', text: '你是助手' }] },
        { role: 'user', content: [{ type: 'text', text: '查天气' }] },
        {
          role: 'assistant',
          content: [
            { type: 'text', text: '让我查一下' },
            { type: 'tool-call', id: 'call-1', name: 'weather.city', arguments: '{"city":"上海"}' },
          ],
        },
        { role: 'tool', toolCallId: 'call-1', content: [{ type: 'text', text: '晴 25 度' }] },
      ],
      tools: [{ name: 'weather.city', description: '查城市天气', parameters: { type: 'object' } }],
    },
    CATALOG,
  );
  assert.equal(envelope.payload.instructions, '你是助手');
  assert.equal(envelope.payload.tools.length, 1);
  // 工具名转换可逆：`weather.city` → `weather_city`（模型回的名字要能映射回来）。
  assert.equal(envelope.payload.tools[0].name, 'weather_city');
  assert.equal(toProviderToolName('weather.city', new Map()), 'weather_city');
  const input = envelope.payload.input;
  assert.equal(input[2].type, 'function_call');
  assert.equal(input[2].call_id, 'call-1');
  assert.equal(input[3].type, 'function_call_output');
  assert.equal(input[3].call_id, 'call-1');
});

test('envelope: user 图片/文件块显式拒绝（首版范围外）', () => {
  assert.throws(
    () =>
      buildEnvelope(
        {
          model: 'gpt-x',
          messages: [{ role: 'user', content: [{ type: 'image', attachment: {} }] }],
        },
        CATALOG,
      ),
    /不支持图片/,
  );
});

test('pump: 文本增量 → 块生命周期 + usage + finish（带回放）', async () => {
  const lines = [
    '{"type":"response.output_text.delta","delta":"你"}',
    '{"type":"response.output_text.delta","delta":"好"}',
    '{"type":"response.completed","response":{"id":"r1","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12},"output":[]}}',
    // 桥接终止包络（bridge 侧由 classify_terminal 生成；pump 吃的是桥接输出）。
    '{"type":"bridge.terminal","status":"completed","replay":{"response":{"id":"r1","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12},"output":[]}}}',
  ];
  const chunks = [];
  for await (const chunk of pumpStream(lines)) chunks.push(chunk);
  assert.equal(chunks[0].type, 'block-start');
  assert.equal(chunks[0].blockType, 'text');
  assert.equal(chunks[1].text, '你');
  assert.equal(chunks[2].text, '好');
  const end = chunks.find((c) => c.type === 'block-end');
  assert.equal(end.block.type, 'text');
  const usage = chunks.find((c) => c.type === 'usage');
  assert.equal(usage.usage.totalTokens, 12);
  const finish = chunks.at(-1);
  assert.equal(finish.type, 'finish');
  assert.equal(finish.reason.kind, 'stop');
  assert.equal(finish.replayState.response.id, 'r1');
});

test('pump: EOF 无终止包络 → 抛错（incomplete 不算成功）', async () => {
  const lines = ['{"type":"response.output_text.delta","delta":"部分"}'];
  await assert.rejects(
    async () => {
      for await (const chunk of pumpStream(lines)) void chunk;
    },
    /按失败结算/,
  );
});

test('pump: failed 终止 → 抛错带服务端原因', async () => {
  const lines = [
    '{"type":"response.failed","response":{"error":{"message":"quota exceeded"}}}',
    '{"type":"bridge.terminal","status":"failed","replay":null,"detail":"quota exceeded"}',
  ];
  await assert.rejects(
    async () => {
      for await (const chunk of pumpStream(lines)) void chunk;
    },
    /quota exceeded/,
  );
});

test('pump: 工具调用增量与名字回映射', async () => {
  const lines = [
    '{"type":"response.output_item.added","item":{"type":"function_call","item_id":"fc1","name":"weather_city"}}',
    '{"type":"response.function_call_arguments.delta","item_id":"fc1","name":"weather_city","delta":"{\\"city\\""}',
    '{"type":"response.completed","response":{"id":"r2"}}',
    '{"type":"bridge.terminal","status":"completed","replay":{"response":{"id":"r2"}}}',
  ];
  const chunks = [];
  for await (const chunk of pumpStream(lines)) chunks.push(chunk);
  const delta = chunks.find((c) => c.type === 'tool-call-delta');
  assert.equal(delta.index !== undefined, true);
  assert.equal(delta.argumentsDelta, '{"city"');
});
