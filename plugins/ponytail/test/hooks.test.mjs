import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  HOOK_SCRIPTS,
  hookPayload,
  parseHookOutput,
  runHookScript,
} from '../host/hooks.js';

const SESSION = { session: { header: { id: 'sess-1' } } };

test('三个事件各对应上游脚本（文件名是契约，上游若改名这里要一起改）', () => {
  assert.deepEqual(HOOK_SCRIPTS, {
    SessionStart: 'ponytail-activate.js',
    UserPromptSubmit: 'ponytail-mode-tracker.js',
    SubagentStart: 'ponytail-subagent.js',
  });
});

test('parseHookOutput 只认本事件的 JSON 信封，其余一律当没有注入', () => {
  const envelope = (event, text) =>
    JSON.stringify({ hookSpecificOutput: { hookEventName: event, additionalContext: text } });

  assert.equal(parseHookOutput(envelope('SessionStart', 'RULES'), 'SessionStart'), 'RULES');
  // 上游在「什么都不做」时输出 `{}`。
  assert.equal(parseHookOutput('{}', 'SessionStart'), '');
  assert.equal(parseHookOutput('', 'SessionStart'), '');
  assert.equal(parseHookOutput('not json', 'SessionStart'), '');
  assert.equal(parseHookOutput('RULES', 'SessionStart'), '');
  // 事件名对不上 = 不能注入到错误的生命周期里（桥也是这么校验的）。
  assert.equal(
    parseHookOutput(envelope('UserPromptSubmit', 'RULES'), 'SessionStart'),
    '',
  );
  // 非字符串的 additionalContext 不认。
  assert.equal(
    parseHookOutput(JSON.stringify({ hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext: 1 } }), 'SessionStart'),
    '',
  );
});

test('hookPayload 带公共字段与事件字段（上游只读其中一部分，但字段名是协议）', () => {
  assert.deepEqual(hookPayload('SessionStart', { agent: SESSION, source: 'compact' }), {
    session_id: 'sess-1',
    transcript_path: '',
    hook_event_name: 'SessionStart',
    source: 'compact',
  });
  assert.deepEqual(hookPayload('UserPromptSubmit', { agent: SESSION, prompt: '/ponytail lite' }), {
    session_id: 'sess-1',
    transcript_path: '',
    hook_event_name: 'UserPromptSubmit',
    prompt: '/ponytail lite',
  });
  assert.deepEqual(hookPayload('SubagentStart', { agent: SESSION, subagent: { id: 'a1' } }), {
    session_id: 'sess-1',
    transcript_path: '',
    hook_event_name: 'SubagentStart',
    agent_id: 'a1',
    agent_type: 'general-purpose',
  });
  // 没有 agent（子代理查不到）时公共字段也要是字符串，不能是 undefined。
  const bare = hookPayload('SessionStart', {});
  assert.equal(bare.session_id, '');
  assert.equal(bare.source, 'startup');
});

test('runHookScript 按上游契约喂 stdin、读 stdout，成功时带回上下文', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'ponytail-hook-'));
  const script = join(dir, 'hook.js');
  writeFileSync(
    script,
    [
      "let raw = '';",
      "process.stdin.on('data', (chunk) => { raw += chunk; });",
      "process.stdin.on('end', () => {",
      '  const payload = JSON.parse(raw);',
      '  process.stdout.write(JSON.stringify({',
      '    hookSpecificOutput: {',
      '      hookEventName: payload.hook_event_name,',
      "      additionalContext: `mode:${payload.prompt}`,",
      '    },',
      '  }));',
      '});',
    ].join('\n'),
    'utf8',
  );
  const result = await runHookScript({
    event: 'UserPromptSubmit',
    script,
    payload: hookPayload('UserPromptSubmit', { agent: SESSION, prompt: '/ponytail lite' }),
    cwd: dir,
  });
  assert.equal(result.error, '');
  assert.equal(result.context, 'mode:/ponytail lite');
});

test('runHookScript 失败时 fail open：空上下文 + 错误说明', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'ponytail-hook-fail-'));
  const script = join(dir, 'hook.js');
  writeFileSync(
    script,
    "process.stderr.write('boom'); process.exit(2);\n",
    'utf8',
  );
  const result = await runHookScript({
    event: 'SessionStart',
    script,
    payload: hookPayload('SessionStart', { agent: SESSION }),
    cwd: dir,
  });
  assert.equal(result.context, '');
  assert.match(result.error, /boom/u, 'stderr 要回传给调用方记日志');
});

test('runHookScript 遇到不存在的脚本也返回空而不是抛', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'ponytail-hook-missing-'));
  const result = await runHookScript({
    event: 'SessionStart',
    script: join(dir, 'nope.js'),
    payload: hookPayload('SessionStart', { agent: SESSION }),
    cwd: dir,
  });
  assert.equal(result.context, '');
  assert.notEqual(result.error, '');
});
