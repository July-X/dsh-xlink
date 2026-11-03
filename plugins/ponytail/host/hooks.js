/**
 * Ponytail 的 hook 执行体：**不依赖 `@deepseek-ai/dsh-hooks-claude-code`**。
 *
 * 上游 ponytail 的规则注入/切档/子代理注入是三份 Claude Code 形态的 node 脚本
 * （`hooks/ponytail-activate.js` 等）。内核的原生扩展点（`agent/created`、
 * `agent/pre-step`、`subagent/start`）由 `index.js` 直接监听；本文件只负责
 * 「按上游的 stdin/stdout 契约跑那个脚本、把 `additionalContext` 取回来」。
 *
 * 上游的钩子机制按稳定契约对待（2026-11-03 用户确认）：脚本路径、stdin 字段、
 * `hookSpecificOutput.additionalContext` 输出都取自 `@dietrichgebert/ponytail`
 * 的固定形状，版本跟随由壳的上游同步负责。若上游改这三样，这里会**输出空、
 * 只在日志留一行 warn**（fail open，与桥的行为一致）——所以改动上游契约时
 * 必须同步本文件与 `test/hooks.test.mjs`。
 *
 * 为什么不用桥：桥是「Claude Code 配置格式 → 内核扩展点」的通用适配器，内核
 * 不保证每个版本带它（alpha.2 起是 devDependency）；我们只需要三个固定事件，
 * 自己接线就没有那层可选依赖（2026-11-03 用户要求）。
 */
import { spawn } from 'node:child_process';

/** 三个事件各自对应的上游脚本（相对 `<插件根>/hooks/`）。 */
export const HOOK_SCRIPTS = Object.freeze({
  SessionStart: 'ponytail-activate.js',
  UserPromptSubmit: 'ponytail-mode-tracker.js',
  SubagentStart: 'ponytail-subagent.js',
});

/** 与上游 `claude-codex-hooks.json` 的 `timeout` 同值。 */
const HOOK_TIMEOUT_MS = 15_000;
/** 宿主契约：stdout ≤ 64 KiB；我们只留到上限，超出部分丢掉。 */
const MAX_STDOUT = 64 * 1024;
const MAX_STDERR = 4 * 1024;

/**
 * 解析上游 hook 的 stdout。只接受 JSON 信封，且 `hookEventName` 必须与调用的
 * 事件一致——与桥的校验相同；其余一律当「没有注入」（fail open）。
 *
 * @param {string} stdout
 * @param {string} expectedEvent
 * @returns {string} `additionalContext`，没有则为空串
 */
export function parseHookOutput(stdout, expectedEvent) {
  const text = String(stdout ?? '').replace(/^\uFEFF/, '').trim();
  if (!text) return '';
  let output;
  try {
    output = JSON.parse(text);
  } catch {
    return '';
  }
  const hook = output?.hookSpecificOutput;
  if (!hook || hook.hookEventName !== expectedEvent) return '';
  return typeof hook.additionalContext === 'string' ? hook.additionalContext : '';
}

/**
 * 构造上游脚本的 stdin 载荷（Claude Code hook 契约的公共字段 + 事件字段）。
 * 上游只用到其中一部分，但字段名必须对——它们是协议，不是注释。
 *
 * @param {string} event
 * @param {{ agent?: object, source?: string, prompt?: string, subagent?: { id?: string, type?: string } }} input
 */
export function hookPayload(event, { agent, source, prompt, subagent } = {}) {
  const payload = {
    session_id: agent?.session?.header?.id ?? '',
    transcript_path: '',
    hook_event_name: event,
  };
  if (event === 'SessionStart') payload.source = source || 'startup';
  if (event === 'UserPromptSubmit') payload.prompt = prompt ?? '';
  if (event === 'SubagentStart') {
    payload.agent_id = subagent?.id ?? '';
    // 与桥一致：内核子代理接缝没有按 kind 分的标签，用 Claude Code 默认值。
    payload.agent_type = subagent?.type || 'general-purpose';
  }
  return payload;
}

/**
 * 跑一个上游 hook 脚本，返回它要注入的上下文。
 *
 * 失败（退出码非 0 / 超时 / abort / 输出解析失败）一律返回空串，由调用方决定
 * 是否记日志——hook 失败不能阻塞会话，这是上游与桥共同的契约。
 *
 * @param {{
 *   event: string,
 *   script: string,
 *   payload: object,
 *   cwd: string,
 *   env?: Record<string, string>,
 *   signal?: AbortSignal,
 *   timeoutMs?: number,
 * }} options
 * @returns {Promise<{ context: string, error: string }>}
 */
export function runHookScript({
  event,
  script,
  payload,
  cwd,
  env,
  signal,
  timeoutMs = HOOK_TIMEOUT_MS,
}) {
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(process.execPath, [script], {
        cwd,
        env: { ...process.env, ...env },
        stdio: ['pipe', 'pipe', 'pipe'],
        windowsHide: true,
      });
    } catch (error) {
      resolve({ context: '', error: String(error) });
      return;
    }

    let stdout = '';
    let stderr = '';
    let settled = false;
    let timer;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener?.('abort', onAbort);
      resolve(result);
    };
    const onAbort = () => {
      child.kill();
      finish({ context: '', error: 'aborted' });
    };

    timer = setTimeout(() => {
      child.kill();
      finish({ context: '', error: `hook timed out after ${timeoutMs}ms` });
    }, timeoutMs);
    if (signal) {
      if (signal.aborted) {
        onAbort();
        return;
      }
      signal.addEventListener('abort', onAbort, { once: true });
    }

    child.stdout.on('data', (chunk) => {
      if (stdout.length < MAX_STDOUT) stdout += chunk.toString('utf8');
    });
    child.stderr.on('data', (chunk) => {
      if (stderr.length < MAX_STDERR) stderr += chunk.toString('utf8');
    });
    child.on('error', (error) => finish({ context: '', error: String(error) }));
    child.on('close', (code) => {
      if (code !== 0) {
        finish({ context: '', error: stderr.trim() || `hook exited with ${code}` });
        return;
      }
      finish({ context: parseHookOutput(stdout, event), error: '' });
    });

    try {
      child.stdin.end(JSON.stringify(payload));
    } catch {
      // stdin 已关闭：脚本没有输入就跑（上游多数脚本不依赖 stdin）。
    }
  });
}
