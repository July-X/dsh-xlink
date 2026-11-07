import { readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { callMemory } from '../host/bridge.js';
import { joinQueryTexts } from '../host/query.mjs';

// MiniMax passes only its own environment to hooks. The shell owns this private,
// instance-specific file and removes it when the bridge is stopped.
export const CREDENTIAL_FILE = 'xlink-memory-bridge.json';

/**
 * 失败原因只进 stderr。
 *
 * stdout 是给内核的协议通道，混进去会被当成上下文；stderr 是内核记日志的地方，
 * 面板与排查都看得到。凭据不会漏进来：读文件与解析的错在这一层被吞掉，
 * 请求失败的原因只有「无法连接记忆桥：<原因>」这种不含令牌的说法。
 */
function report(reason) {
  process.stderr.write(`xlink-memory: ${reason}\n`);
}

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
    // 桥没启用时每个回合都会走到这里：凭据不在不是错误，不报。
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
  if (!result.ok) {
    // 静默失败过一次：面板显示「已授权」，MiniMax 那边每回合都没有记忆，
    // 两头都查不到原因。
    report(`记忆桥请求失败：${result.reason}`);
    return null;
  }
  if (result.value?.enabled !== true) return null;
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
  } catch (error) {
    // 记忆不可用不该挡住用户这一回合，但原因必须留下。
    report(String(error?.stack ?? error));
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) await main();
