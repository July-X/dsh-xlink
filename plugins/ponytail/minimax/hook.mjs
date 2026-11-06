import { isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HOOK_SCRIPTS, runHookScript } from '../host/hooks.js';

export async function contextForHook(input, env = process.env) {
  const event = input?.hook_event_name;
  const script = Object.hasOwn(HOOK_SCRIPTS, event) ? HOOK_SCRIPTS[event] : null;
  const root = env.PLUGIN_ROOT;
  const data = env.PLUGIN_DATA;
  if (!script || typeof root !== 'string' || !isAbsolute(root) || typeof data !== 'string' || !isAbsolute(data)) return null;
  const cwd = process.cwd();
  const result = await runHookScript({
    event,
    script: join(root, 'hooks', script),
    payload: input,
    cwd,
    env: { CLAUDE_PLUGIN_ROOT: root, CLAUDE_PROJECT_DIR: cwd, CLAUDE_CONFIG_DIR: data, PLUGIN_DATA: data },
    timeoutMs: 8_000,
  });
  return result.context ? { hookSpecificOutput: { hookEventName: event, additionalContext: result.context } } : null;
}

async function main() {
  let input = '';
  for await (const chunk of process.stdin) {
    input += chunk;
    if (Buffer.byteLength(input, 'utf8') > 1024 * 1024) return;
  }
  try {
    const output = await contextForHook(JSON.parse(input));
    if (output) process.stdout.write(JSON.stringify(output));
  } catch {
    // A broken optional hook must not block the user's task.
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) await main();
