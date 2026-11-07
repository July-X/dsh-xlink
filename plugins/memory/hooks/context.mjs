import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { callMemory } from '../host/bridge.js';
import { joinQueryTexts } from '../host/policy.js';

// MiniMax passes only its own environment to hooks. The shell owns this private,
// instance-specific file and removes it when the bridge is stopped.
export const CREDENTIAL_FILE = 'xlink-memory-bridge.json';

async function credentials(pluginData) {
  if (!pluginData) return null;
  try {
    const value = JSON.parse(await readFile(join(pluginData, CREDENTIAL_FILE), 'utf8'));
    const url = new URL(value.url);
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port ||
        url.username || url.password || url.pathname !== '/' || url.search || url.hash ||
        typeof value.token !== 'string' || !/^[a-f0-9]{32,}$/i.test(value.token)) return null;
    return { url: url.origin, token: value.token };
  } catch {
    return null;
  }
}

export async function contextForPrompt(input, pluginData) {
  if (input?.hook_event_name !== 'UserPromptSubmit' || typeof input.prompt !== 'string' ||
      typeof input.cwd !== 'string' || !input.cwd || typeof input.session_id !== 'string' ||
      !input.session_id) return null;
  const creds = await credentials(pluginData);
  if (!creds) return null;
  const result = await callMemory({
    credentials: creds,
    route: 'context',
    params: { q: joinQueryTexts([input.prompt]), workspace: input.cwd },
  });
  if (!result.ok || result.value?.enabled !== true) return null;
  const text = [result.value.handoff_text, result.value.text, result.value.experience_text]
    .filter((item) => typeof item === 'string' && item).join('\n\n');
  return text ? { hookSpecificOutput: { hookEventName: 'UserPromptSubmit', additionalContext: text } } : null;
}

async function main() {
  let body = '';
  for await (const chunk of process.stdin) {
    body += chunk;
    if (body.length > 1024 * 1024) return;
  }
  try {
    const output = await contextForPrompt(JSON.parse(body), process.env.PLUGIN_DATA);
    if (output) process.stdout.write(JSON.stringify(output));
  } catch {
    // A broken memory bridge must never block a user prompt or expose credentials.
  }
}

await main();
