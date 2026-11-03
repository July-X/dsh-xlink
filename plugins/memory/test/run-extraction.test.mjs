// 模型层失败的**分类**，以及它在编排结果里的去处（`host/run.js` 的 `classifyModelFinish` 与
// `runExtraction` 带回去的 `errorKind`）。
//
// 这一份刻意**不装内核**：内核把 provider 失败做成终结 finish chunk
// （`{kind:'error', failure:{code,message}}`），插件读它自己那份形状就够了——内核那边改了
// 形状，这里就该跟着改，而不是让整条提取链路一起红。
import assert from 'node:assert/strict';
import test from 'node:test';

import { ERROR_KINDS, classifyModelFinish, runExtraction } from '../host/run.js';

/** 一套能跑完七步的最小依赖；`overrides` 换掉其中一步。 */
function deps(overrides = {}) {
  return {
    system: 'SYS',
    readRange: async () => ({ ok: true, events: [{ type: 'user/message', seq: 1, data: { content: '说点话' } }] }),
    buildSlice: () => ({ text: '说点话', seqs: [1] }),
    retrieveForExtract: async () => ({ ok: true, items: [] }),
    fingerprint: () => 'f'.repeat(16),
    callModel: async () => ({ ok: true, text: '[]' }),
    parse: () => ({ ok: true, candidates: [], rejected: [] }),
    submit: async () => ({ ok: true, submitted: 0 }),
    ...overrides,
  };
}

const key = { session: { id: 's-1' }, cursor: 3, extractor: 'xlink-memory-extract/1' };

test('分类：内核失败 code → errorKind，认不出的归 unknown', () => {
  const kindOf = (code) => classifyModelFinish({ kind: 'error', failure: { code, message: '炸了' } }, '').errorKind;
  assert.equal(kindOf('TIMEOUT'), 'timeout');
  assert.equal(kindOf('TRANSPORT'), 'transport');
  assert.equal(kindOf('SERVER'), 'transport');
  assert.equal(kindOf('EMPTY_RESPONSE'), 'empty_stream');
  assert.equal(kindOf('QUOTA_EXCEEDED'), 'provider_error');
  assert.equal(kindOf('INVALID_CREDENTIAL'), 'provider_error');
  assert.equal(kindOf('RATE_LIMIT'), 'provider_error');
  assert.equal(kindOf('INVALID_REQUEST'), 'provider_error');
  assert.equal(kindOf('PI_AI_ERROR'), 'provider_error');
  // 认不出的 code **不许猜**：猜错的话排障的人会顺着错的分类去找原因。
  assert.equal(kindOf('SOMETHING_NEW'), 'unknown');
  // 缺 code 的失败也是 unknown（内核认不出的错误给的就是没有自家 code 的那一种）。
  assert.equal(classifyModelFinish({ kind: 'error' }, '').errorKind, 'unknown');
});

test('分类：finish 的形状各自归类', () => {
  assert.equal(classifyModelFinish({ kind: 'aborted' }, '').errorKind, 'timeout');
  assert.equal(classifyModelFinish({ kind: 'max-tokens' }, '').errorKind, 'truncated');
  // stop 但没有正文：模型没说话，不是有内容。
  assert.equal(classifyModelFinish({ kind: 'stop' }, '').errorKind, 'empty_stream');
  assert.equal(classifyModelFinish({ kind: 'stop' }, '   \n ').errorKind, 'empty_stream', '只有空白也算空');
  // 没有终结 chunk 时内核给的就是 `{kind:'stop'}`。
  assert.equal(classifyModelFinish(undefined, '').errorKind, 'empty_stream');
  assert.equal(classifyModelFinish(null, '').errorKind, 'empty_stream');
  // stop 且有正文：成功。
  assert.deepEqual(classifyModelFinish({ kind: 'stop' }, '[]'), { ok: true });
  assert.deepEqual(classifyModelFinish(undefined, '[]'), { ok: true });
  // 提取不传工具，收到工具调用就是异常形状——不许装作成功。
  assert.equal(classifyModelFinish({ kind: 'tool-calls' }, '[]').errorKind, 'unknown');
});

test('分类：reason 留住内核的 code 与 message（吞掉就等于没分类）', () => {
  const failed = classifyModelFinish(
    { kind: 'error', failure: { code: 'QUOTA_EXCEEDED', message: '额度用尽' } },
    ''
  );
  assert.equal(failed.ok, false);
  assert.equal(failed.errorKind, 'provider_error');
  assert.match(failed.reason, /QUOTA_EXCEEDED/, '内核的 code 要留在原因里');
  assert.match(failed.reason, /额度用尽/, '内核的 message 要留在原因里');
  // 每种分类都有一句自己的话，别把失败原因压成同一句。
  assert.match(classifyModelFinish({ kind: 'max-tokens' }, '').reason, /token 上限/);
  assert.match(classifyModelFinish({ kind: 'stop' }, '').reason, /没有返回正文/);
  assert.match(classifyModelFinish({ kind: 'aborted' }, '').reason, /中止或超时/);
  assert.match(classifyModelFinish({ kind: 'error', failure: { code: 'TRANSPORT', message: '断线' } }, '').reason, /传输失败/);
});

test('分类：errorKind 一定是 ERROR_KINDS 里的一个', () => {
  const finishes = [
    undefined,
    { kind: 'stop' },
    { kind: 'aborted' },
    { kind: 'max-tokens' },
    { kind: 'tool-calls' },
    { kind: 'error' },
    { kind: 'error', failure: { code: 'TIMEOUT', message: 'x' } },
    { kind: 'error', failure: { code: 'WEIRD', message: 'x' } },
  ];
  for (const finish of finishes) {
    const result = classifyModelFinish(finish, '');
    if (result.ok) continue;
    assert.ok(
      ERROR_KINDS.includes(result.errorKind),
      `errorKind 不在表里：${result.errorKind}（${JSON.stringify(finish)}）`
    );
  }
});

test('编排：模型层失败时 stage 仍是 model，分类跟着结果回去', async () => {
  const result = await runExtraction({
    deps: deps({
      callModel: async () => ({
        ok: false,
        reason: '模型调用失败（QUOTA_EXCEEDED）：额度用尽',
        errorKind: 'provider_error',
      }),
    }),
    key,
    from: 1,
  });
  assert.equal(result.ok, false);
  assert.equal(result.stage, 'model', '阶段机不动：失败还是停在 model 这一步');
  assert.equal(result.errorKind, 'provider_error', '分类不能丢——丢了就与「空正文」没区别');
  assert.match(result.reason, /额度用尽/);
});

test('编排：别的阶段失败不带 errorKind（分类只是模型层的事）', async () => {
  const read = await runExtraction({
    deps: deps({ readRange: async () => ({ ok: false, reason: '没有持久化监听器' }) }),
    key,
    from: 1,
  });
  assert.equal(read.stage, 'read');
  assert.equal(read.errorKind, '');

  const slice = await runExtraction({ deps: deps({ buildSlice: () => ({ text: '', seqs: [] }) }), key, from: 1 });
  assert.equal(slice.stage, 'slice');
  assert.equal(slice.errorKind, '');

  const parse = await runExtraction({
    deps: deps({ parse: () => ({ ok: false, reason: '不合 schema' }) }),
    key,
    from: 1,
  });
  assert.equal(parse.stage, 'parse');
  assert.equal(parse.errorKind, '');

  const submit = await runExtraction({
    deps: deps({
      callModel: async () => ({ ok: true, text: '[{"title":"t"}]' }),
      parse: () => ({ ok: true, candidates: [{ title: 't' }], rejected: [] }),
      submit: async () => ({ ok: false, reason: '桥不通' }),
    }),
    key,
    from: 1,
  });
  assert.equal(submit.stage, 'submit');
  assert.equal(submit.errorKind, '');

  const controller = new AbortController();
  controller.abort();
  const cancelled = await runExtraction({ deps: deps(), key, from: 1, signal: controller.signal });
  assert.equal(cancelled.stage, 'cancelled');
  assert.equal(cancelled.errorKind, '');

  const done = await runExtraction({
    deps: deps({
      callModel: async () => ({ ok: true, text: '[{"title":"t"}]' }),
      parse: () => ({ ok: true, candidates: [{ title: 't' }], rejected: [] }),
      submit: async () => ({ ok: true, submitted: 1 }),
    }),
    key,
    from: 1,
  });
  assert.equal(done.stage, 'done');
  assert.equal(done.errorKind, '', '成功也带一个空串：调用方不必判断这个字段在不在');
});
