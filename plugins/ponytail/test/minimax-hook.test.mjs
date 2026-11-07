import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { contextForHook } from '../minimax/hook.mjs';

test('MiniMax hook 只返回匹配事件的上下文，并隔离上游状态', async () => {
  const root = await mkdtemp(join(tmpdir(), 'mcode-ponytail-hook-'));
  const previousSecret = process.env.DSH_SECRET;
  process.env.DSH_SECRET = 'test-only';
  try {
    const data = join(root, 'data');
    await mkdir(join(root, 'hooks'));
    await mkdir(data);
    const hook = `
      let input = '';
      process.stdin.on('data', chunk => input += chunk);
      process.stdin.on('end', () => {
        const payload = JSON.parse(input);
        const isolated = process.env.CLAUDE_CONFIG_DIR === process.env.PLUGIN_DATA && !process.env.DSH_SECRET;
        process.stdout.write(JSON.stringify({ hookSpecificOutput: { hookEventName: payload.hook_event_name, additionalContext: isolated ? (payload.prompt || payload.hook_event_name) : '' } }));
      });
    `;
    for (const name of ['ponytail-activate.js', 'ponytail-mode-tracker.js', 'ponytail-subagent.js']) {
      await writeFile(join(root, 'hooks', name), hook);
    }
    const env = { PLUGIN_ROOT: root, PLUGIN_DATA: data };
    assert.deepEqual(await contextForHook({ hook_event_name: 'UserPromptSubmit', prompt: 'hello' }, env), {
      hookSpecificOutput: { hookEventName: 'UserPromptSubmit', additionalContext: 'hello' },
    });
    for (const event of ['SessionStart', 'SubagentStart']) {
      assert.deepEqual(await contextForHook({ hook_event_name: event }, env), {
        hookSpecificOutput: { hookEventName: event, additionalContext: event },
      });
    }
    assert.equal(await contextForHook({ hook_event_name: 'PreToolUse' }, env), null);
    assert.equal(await contextForHook({ hook_event_name: 'UserPromptSubmit' }, { PLUGIN_ROOT: 'relative', PLUGIN_DATA: data }), null);
  } finally {
    if (previousSecret === undefined) delete process.env.DSH_SECRET;
    else process.env.DSH_SECRET = previousSecret;
    await rm(root, { recursive: true, force: true });
  }
});
