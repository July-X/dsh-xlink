// `scripts/analyze-prompt-cache.mjs` 的用例：判据本身要能被反证。
//
// 钉三件事——① 一次调用的缓存事实取自上游响应对象（`replayState.response`），
// 不是内核的用量桶；② 汇总能分开「键每次都不同」与「前缀中途变过」两种病；
// ③ 没有带上游响应的日志要说清，而不是印一张空表。
import assert from 'node:assert/strict';
import test from 'node:test';

import {
  parseCacheFacts,
  parseCodexFacts,
  parseFacts,
  renderReport,
  resolveSessionLog,
  summarizeCacheFacts,
  verdictLines,
} from './analyze-prompt-cache.mjs';

/** 造一条带上游响应的 assistant 消息事件。 */
function assistantEvent(seq, response) {
  return JSON.stringify({
    type: 'assistant/message',
    seq,
    data: {
      turn: 1,
      step: seq,
      message: { role: 'assistant', content: [], source: { kind: 'model' } },
      stream: [{ chunk: { type: 'finish', replayState: { response } } }],
    },
  });
}

function response(overrides = {}) {
  return {
    object: 'response',
    model: 'gpt-6.1-sol',
    instructions: 'stable instructions',
    tools: [{ type: 'namespace', name: 'dsh' }],
    prompt_cache_key: 'key-1',
    prompt_cache_retention: '24h',
    usage: {
      input_tokens: 1000,
      input_tokens_details: { cached_tokens: 0, cache_write_tokens: 0 },
      attribution: {
        request_fields: {
          instructions: { input_tokens: 100, cached_tokens: 0 },
          tools: { input_tokens: 200, cached_tokens: 0 },
        },
      },
    },
    ...overrides,
  };
}

test('parseCacheFacts 从上游响应里取缓存事实，不看内核用量桶', () => {
  const text = [
    JSON.stringify({ type: 'session', id: 's1' }),
    assistantEvent(7, response()),
    assistantEvent(9, response({
      prompt_cache_key: 'key-2',
      usage: {
        input_tokens: 2000,
        input_tokens_details: { cached_tokens: 1500, cache_write_tokens: 0 },
        attribution: {
          request_fields: {
            instructions: { input_tokens: 100, cached_tokens: 100 },
            tools: { input_tokens: 200, cached_tokens: 200 },
          },
        },
      },
    })),
    'not json at all',
  ].join('\n');
  const facts = parseCacheFacts(text);
  assert.equal(facts.length, 2);
  assert.equal(facts[0].seq, 7);
  assert.equal(facts[0].key, 'key-1');
  assert.equal(facts[0].inputTokens, 1000);
  assert.equal(facts[0].cachedTokens, 0);
  assert.equal(facts[0].instructionsTokens, 100);
  assert.equal(facts[1].cachedTokens, 1500);
  assert.equal(facts[1].instructionsCached, 100);
  assert.equal(facts[1].toolsCached, 200);
  // 指纹：前缀变没变靠它
  assert.equal(facts[0].instructionsHash, facts[1].instructionsHash);
  assert.equal(facts[0].toolsHash, facts[1].toolsHash);
});

test('summarizeCacheFacts 分开「键每次都不同」与「前缀中途变过」', () => {
  const same = parseCacheFacts([
    assistantEvent(1, response()),
    assistantEvent(2, response({ prompt_cache_key: 'key-2' })),
  ].join('\n'));
  const summary = summarizeCacheFacts(same);
  assert.equal(summary.calls, 2);
  assert.equal(summary.distinctKeys, 2);
  assert.equal(summary.distinctInstructions, 1);
  assert.equal(summary.distinctTools, 1);
  const lines = verdictLines(summary).join('\n');
  assert.match(lines, /键每次都不同/);
  assert.match(lines, /前缀逐字不变/);
  assert.match(lines, /未命中 2 次/);

  const changed = parseCacheFacts([
    assistantEvent(1, response()),
    assistantEvent(2, response({ instructions: 'other instructions' })),
  ].join('\n'));
  const changedSummary = summarizeCacheFacts(changed);
  assert.equal(changedSummary.distinctInstructions, 2);
  assert.match(verdictLines(changedSummary).join('\n'), /前缀在会话中途变过/);
});

test('空日志说清「没有带上游响应的调用」，报告不印空表充数', () => {
  const facts = parseCacheFacts([JSON.stringify({ type: 'session', id: 's1' })].join('\n'));
  const summary = summarizeCacheFacts(facts);
  assert.equal(summary.calls, 0);
  assert.match(verdictLines(summary).join('\n'), /没有带上游响应的模型调用/);
  const report = renderReport(facts, summary);
  assert.match(report, /没有带上游响应的模型调用/);
});

test('resolveSessionLog 目录取最新日志，文件原样返回', () => {
  assert.equal(resolveSessionLog('/definitely/not/here'), undefined);
});

/** 造一条原生 Codex 的 token_usage_record 事件。 */
function codexUsage(ordinal, input, cached, sessionId = 'sess-1') {
  return JSON.stringify({
    timestamp: '2026-10-18T10:53:15.773Z',
    ordinal,
    type: 'token_usage_record',
    payload: {
      thread_id: sessionId,
      turn_id: `${sessionId}-turn`,
      session_id: sessionId,
      usage: {
        input_tokens: input,
        cached_input_tokens: cached,
        cache_write_input_tokens: 0,
        output_tokens: 76,
        total_tokens: input + 76,
      },
      // 累计口径在别的字段里，认错就把命中率算飞
      thread_token_usage: {
        input_tokens: 999999,
        cached_input_tokens: 888888,
      },
    },
  });
}

test('parseCodexFacts 取这次请求的用量，不拿累计口径', () => {
  const facts = parseCodexFacts(
    [
      JSON.stringify({ type: 'session_meta', payload: { id: 'sess-1' } }),
      codexUsage(20, 29761, 22912),
      codexUsage(26, 33780, 29568),
    ].join('\n'),
  );
  assert.equal(facts.length, 2);
  assert.equal(facts[0].seq, 20);
  assert.equal(facts[0].key, 'sess-1');
  assert.equal(facts[0].inputTokens, 29761);
  assert.equal(facts[0].cachedTokens, 22912);
  assert.equal(facts[0].source, 'codex');
  // 前缀指纹量不了：原生落盘日志不存请求体
  assert.equal(facts[0].instructionsHash, null);
});

test('原生日志的判读不谎称「前缀逐字不变」', () => {
  const facts = parseCodexFacts([codexUsage(20, 29761, 22912)].join('\n'));
  const summary = summarizeCacheFacts(facts);
  assert.equal(summary.source, 'codex');
  assert.equal(summary.prefixKnown, false);
  assert.equal(summary.hitCalls, 1);
  const lines = verdictLines(summary).join('\n');
  assert.match(lines, /量不了/);
  assert.doesNotMatch(lines, /前缀逐字不变/);
  assert.match(lines, /命中 1 次/);
  assert.match(renderReport(facts, summary), /原生 Codex 落盘日志/);
});

test('parseFacts 两种日志形状自动分辨', () => {
  const codex = parseFacts([codexUsage(20, 100, 50)].join('\n'));
  assert.equal(codex.length, 1);
  assert.equal(codex[0].source, 'codex');
  const dsh = parseFacts([assistantEvent(7, response())].join('\n'));
  assert.equal(dsh.length, 1);
  assert.equal(dsh[0].source, 'dsh');
});
