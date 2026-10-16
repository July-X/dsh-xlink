// 真模型验收驱动器（2026-10-16）：真内核 + 真模型 + 真桥 + 真任务表 + 真记忆库。
//
// ## 为什么它在本仓而不是 /tmp
//
// 上一轮那套桩探针留在 `/tmp/p2probe/`，于是清单里引用它的文件在一次清理之后就不存在了——
// `memory-checklist-citations` 门禁直接判红（这是它该做的事）。而「到那天照着跑就行」这句话
// 要求命令是**可复跑**的，所以驱动器进仓；**它自己仍然只往 `/tmp` 写**（探针不许碰用户的实例）。
//
// ## 它做什么
//
// 1. 备场：把 `plugins/memory` 复制到 `$PROBE_HOME/plugins/memory`，并把内核树里的
//    `@deepseek-ai/dsh-tools` / `dsh-llm` 软链进去（**插件自己的相对路径解析不了**：
//    按包名插入的东西在 sdk profile 下找不到，而内核模板自带的那几个走安装树）。
// 2. 生成覆盖层：把真模型的 provider 写进 `$PROBE_HOME/real-overlay.yml`——
//    **凭据与端点全走环境变量**，所以这个文件里不含任何密钥，仓里也不必写死某一家服务。
// 3. 起一个 `--profile sdk` 的**长驻**内核进程（会话级的东西只在长驻进程里验得了：
//    `dsh headless` 一回合一个进程，插件的会话状态每回合从零开始），连跑 N 回合真约定，
//    再多等一会儿让静默期过完、提取跑完，然后收工。
//
// ## 为什么不覆盖 compaction
//
// 这一轮验的是提取：压缩会把要引用的那句原文摘要掉，而 `evidence_seq` 指的就是那句原文。
// 压缩自身的验收另有夹具（G12/G13，桩模型那套）。
//
// 用法（三条，见 docs/features/memory/memory-progress.md 的「怎么验」一节）：
//
//   # ① 另一个终端：起真桥（真的路由 / 真的任务表 / 真的记忆库），它写完握手文件就等停止文件
//   cd src-tauri && cargo test --lib manual_bridge_for_the_node_probe -- --ignored --nocapture
//   # ② 读握手、跑会话（凭据与端点走环境变量）
//   URL=$(node -e "process.stdout.write(require('/tmp/p2probe/bridge-cred.json').url)")
//   TOKEN=$(node -e "process.stdout.write(require('/tmp/p2probe/bridge-cred.json').token)")
//   PROBE_API_KEY=… PROBE_MODEL_BASE=https://…/v1 PROBE_MODEL=… \
//     PROBE_MODEL_HEADERS='{"x-…":"…"}' \
//     DSH_XLINK_BUILTIN_BRIDGE_URL="$URL" DSH_XLINK_BUILTIN_BRIDGE_TOKEN="$TOKEN" \
//     node plugins/memory/test/probe/drive-real-model.mjs real1
//   # ③ 让夹具断言它这一侧真收到了什么
//   touch /tmp/p2probe/bridge-stop
//
// 环境变量：PROBE_HOME（默认 /tmp/p2probe）· PROBE_KERNEL（默认当前 active 那一版，
// 读不到就报错而不是猜）· PROBE_TURNS（默认下面那五句）· PROBE_LINGER_MS（默认 150000）
// · PROBE_RPC_TIMEOUT_MS（默认 300000，真模型比桩慢一个数量级）
import { spawn } from 'node:child_process';
import { appendFileSync, writeFileSync, mkdirSync, rmSync, cpSync, symlinkSync, readFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..', '..', '..');
const LABEL = process.argv[2] ?? 'real';
const PROBE_HOME = process.env.PROBE_HOME ?? '/tmp/p2probe';
const LINGER_MS = Number(process.env.PROBE_LINGER_MS ?? 150_000);
// 回合之间的等待（0 = 连着说，被合并成一次提取）。**收尾那次**另算，见 `LINGER_MS`。
const BETWEEN_MS = process.env.PROBE_LINGER_BETWEEN === '1' ? LINGER_MS : 0;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const RPC_TIMEOUT = Number(process.env.PROBE_RPC_TIMEOUT_MS ?? 300_000);

/**
 * 这一轮要验的是「模型从人话里提不提取得出约定」，所以说的话是真的约定，
 * 而且是**三种不同形状**：一条流程约定、一条技术偏好、一条对写法的要求，外加一条语言偏好。
 * 第 4 句是纯确认（「那这三条我都记着了」）——它**不该**被当成一条新记忆，
 * 所以它同时也是「模型会不会把确认语当约定」这条的负样本。
 */
const TURNS = (process.env.PROBE_TURNS
  ? process.env.PROBE_TURNS.split('\n')
  : [
    '记一下我的约定：以后提交前一定要跑一遍 npm run check 这个门禁，不要跳。',
    '明白。还有：这个仓库的前端是 Vue 3 + Element Plus，你别按 React 的写法来。',
    '好。补一条：我不要提交信息里写「修复若干问题」，要写清改了哪几处。',
    '收到，那这三条我都记着了。',
    '最后一条：中文回复我。',
    ])
    .map((line) => line.trim())
    .filter(Boolean);

function requireEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} 没给——凭据与端点一律走环境变量，仓里不留`);
  return value;
}

/** 当前 active 的那一版内核：读不到就报错（写死版本号只会在下次升级时静默跑错内核）。 */
function activeKernel() {
  if (process.env.PROBE_KERNEL) return process.env.PROBE_KERNEL;
  for (const mode of ['dsh', 'dsh-dev']) {
    const active = join(process.env.HOME ?? '', '.dsh-xlink', mode, 'desktop', 'active.txt');
    if (!existsSync(active)) continue;
    const version = readFileSync(active, 'utf8').trim();
    const kernel = join(process.env.HOME ?? '', '.dsh-xlink', mode, 'desktop', 'kernels', version);
    if (existsSync(kernel)) return kernel;
  }
  throw new Error('读不到 active.txt：用 PROBE_KERNEL 显式指定内核目录');
}

/** 备场：插件副本 + 内核包的软链。**只往 PROBE_HOME 写。 */
function prepare(kernel) {
  const pluginDir = join(PROBE_HOME, 'plugins', 'memory');
  rmSync(pluginDir, { recursive: true, force: true });
  mkdirSync(join(pluginDir, 'node_modules', '@deepseek-ai'), { recursive: true });
  cpSync(join(repoRoot, 'plugins', 'memory'), pluginDir, {
    recursive: true,
    filter: (src) => !src.includes('node_modules'),
  });
  for (const pkg of ['dsh-tools', 'dsh-llm']) {
    symlinkSync(join(kernel, 'node_modules', '@deepseek-ai', pkg), join(pluginDir, 'node_modules', '@deepseek-ai', pkg));
  }
  mkdirSync(join(PROBE_HOME, 'work'), { recursive: true });
  return pluginDir;
}

/**
 * 写覆盖层。
 *
 * **为什么 provider 配了 `headers`**：`dsh-llm-pi-ai` 的 provider 配置支持自定义请求头，
 * 而真实端点常常要浏览器 UA（过 CDN 的机器人签名拦截）与一个会话头（否则 400）。
 * 两个头都由 `PROBE_MODEL_HEADERS`（JSON）传进来，所以本文件不含任何一家服务的地址。
 */
function writeOverlay(pluginEntry) {
  let headers = {};
  try {
    headers = JSON.parse(process.env.PROBE_MODEL_HEADERS ?? '{}');
  } catch (error) {
    throw new Error(`PROBE_MODEL_HEADERS 不是合法 JSON：${String(error?.message ?? error)}`);
  }
  const yaml = [
    '# 由 drive-real-model.mjs 生成，凭据与端点来自环境变量——**这个文件里没有密钥**。',
    '- id: agent-default-model',
    '  name: "@deepseek-ai/dsh-agent-default-model"',
    '  config:',
    `    provider: probe`,
    `    model: ${requireEnv('PROBE_MODEL')}`,
    '- id: llm-pi-ai',
    '  name: "@deepseek-ai/dsh-llm-pi-ai"',
    '  config:',
    '    providers:',
    '      probe:',
    '        apiKeyEnv: PROBE_API_KEY',
    '        api: openai-completions',
    `        baseURL: ${requireEnv('PROBE_MODEL_BASE')}`,
    // **必须嵌在 `headers:` 下面**：平铺到 provider 层的话 pi-ai 会把它当未知键忽略掉，
    // 请求不带那两个头——2026-10-16 第一版就是这么写的，症状是**端点 400 → 回合 status=error
    // → 按 H1 判据一次任务都不排**，而真桥那边看到的是一张空任务表（第一反应会以为又是
    // 那个静默期缺口，其实不是）。没有头可发时就整段省掉。
    ...(Object.keys(headers).length > 0
      ? ['        headers:', ...Object.entries(headers).map(([key, value]) => `          ${key}: ${JSON.stringify(String(value))}`)]
      : []),
    '        models:',
    `          - id: ${requireEnv('PROBE_MODEL')}`,
    `            name: ${requireEnv('PROBE_MODEL')}`,
    '            input: [text]',
    '            contextWindow: 128000',
    '            maxTokens: 32000',
    '# **不覆盖 compaction**：压缩会把要引用的那句原文摘要掉，而 evidence_seq 指的就是那句原文。',
    '- insert:',
    '    - id: xlink-memory',
    `      name: ${JSON.stringify(pluginEntry)}`,
    '',
  ].join('\n');
  const overlay = join(PROBE_HOME, 'real-overlay.yml');
  writeFileSync(overlay, yaml, { mode: 0o600 });
  return overlay;
}

const kernel = activeKernel();
requireEnv('PROBE_API_KEY');
const pluginEntry = join(prepare(kernel), 'host', 'index.js');
const overlay = writeOverlay(pluginEntry);
const LOG = join(PROBE_HOME, `sdk-${LABEL}.log`);
mkdirSync(PROBE_HOME, { recursive: true });
writeFileSync(LOG, '');
const log = (line) => appendFileSync(LOG, `${line}\n`);
log(`内核 ${kernel}`);
log(`覆盖层 ${overlay}`);
log(`回合 ${TURNS.length}：${TURNS.length} 句约定 + 静默期后等 ${LINGER_MS} 毫秒`);

const child = spawn(
  process.execPath,
  [join(kernel, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js'), '--profile', 'sdk', '--patch', overlay],
  {
    // **cwd 指向空的临时目录**：真模型带着 bash 工具时，不该让它在内核安装树上跑。
    cwd: join(PROBE_HOME, 'work'),
    env: {
      ...process.env,
      DSH_HOME: PROBE_HOME,
      DSH_XLINK_BUILTIN_BRIDGE_URL: process.env.DSH_XLINK_BUILTIN_BRIDGE_URL ?? '',
      DSH_XLINK_BUILTIN_BRIDGE_TOKEN: process.env.DSH_XLINK_BUILTIN_BRIDGE_TOKEN ?? '',
    },
    stdio: ['pipe', 'pipe', 'pipe'],
  },
);

let nextId = 1;
const pending = new Map();
// 会话 id 由客户端给，而**它是落盘的**：同一个 id 第二次跑会撞 `already exists`。
//
// **`PROBE_SESSION_ID` 让它可钉住**（2026-10-16 加）：**增量提取只能在同一会话跨两次跑里量**
// ——`from_cursor` 按会话各算各的，而一次运行里的多回合会被去抖合并成一次提取（那是 C6 验收
// 踩过的），所以「第二次从哪儿读」这件事，用默认的带时间戳 id 根本造不出来。
const sessionId =
  process.env.PROBE_SESSION_ID?.trim() || `sdk-${LABEL}-${Date.now()}`;
let buffer = '';

child.stdout.on('data', (chunk) => {
  buffer += chunk.toString('utf8');
  let index;
  while ((index = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    let frame;
    try {
      frame = JSON.parse(line);
    } catch {
      log(`非 JSON 行: ${line.slice(0, 200)}`);
      continue;
    }
    if (frame.method === 'session.event') {
      const type = frame.params?.event?.type;
      // **`turn/end` 要连 `status` 一起打**：插件的调度判据要求回合**正常结束**（`completed`，
      // 见 policy.js 的 SCHEDULABLE_END），而只打事件名的话，一次「任务压根没排」的问题在日志里
      // 看起来和「排了没人领」一模一样——2026-10-16 那次真的分不出来，就是这么补上的。
      // 插件的调度判据读的是 `data.reason.kind === 'completed'`（policy.js 的
      // SCHEDULABLE_END），所以这里打的是**它真正读的那个字段**。
      const reason = frame.params?.event?.data?.reason;
      const status = typeof reason === 'string' ? reason : reason?.kind;
      log(
        `event ${type}` +
          (status ? ` status=${status}` : '') +
          // 回合为什么结束是排障的第一现场：`reason` 常常带一句原因（模型报错、超时、被中断），
          // 只打 `kind` 的话「这一轮压根没排提取」还是解释不了。
          (type === 'turn/end' ? ` data=${JSON.stringify(frame.params.event.data).slice(0, 300)}` : '') +
          (frame.params?.event?.data?.error ? ` error=${frame.params.event.data.error}` : '')
      );
      // **助手的话也要能看见**：回合为什么结束、模型到底说了什么，是排障的第一现场
      // （2026-10-16 那一串「任务表空的」就是这么查出来的）。
      if (type === 'assistant/message') {
        const message = frame.params?.event?.data?.message ?? frame.params?.event?.data;
        const text = typeof message?.content === 'string' ? message.content : JSON.stringify(message?.content ?? message);
        log(`assistant: ${String(text).slice(0, 300)}`);
      }
      continue;
    }
    if (frame.id !== undefined && pending.has(frame.id)) {
      const { resolve: done, reject } = pending.get(frame.id);
      pending.delete(frame.id);
      if (frame.error) reject(new Error(JSON.stringify(frame.error)));
      else done(frame.result);
      continue;
    }
    log(`server notification ${frame.method ?? '?'} ${line.slice(0, 300)}`);
  }
});
child.stderr.on('data', (chunk) => log(`stderr: ${chunk.toString('utf8').trim().slice(0, 600)}`));

function call(method, params) {
  const id = nextId++;
  return new Promise((res, rej) => {
    pending.set(id, { resolve: res, reject: rej });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    setTimeout(() => {
      if (pending.delete(id)) rej(new Error(`${method} 超时 ${RPC_TIMEOUT}ms`));
    }, RPC_TIMEOUT);
  });
}

try {
  const initialized = await call('initialize', {
    cwd: join(PROBE_HOME, 'work'),
    provider: 'probe',
    model: requireEnv('PROBE_MODEL'),
  });
  log(`initialize → ${JSON.stringify(initialized)}`);
  for (const [index, text] of TURNS.entries()) {
    await call('session/prompt', { sessionId, contentBlocks: [{ type: 'text', text }] });
    await call('session/wait', { sessionId });
    log(`turn ${index + 1} 结束：${text.slice(0, 30)}`);
    // **回合之间也要等**（`PROBE_LINGER_BETWEEN=1`，2026-10-16 加）：默认把回合连着说完，
    // 而壳那边有 30 秒去抖，于是**同一会话的多个回合会被合并成一次提取**（C6 验收踩过——
    // 「同会话连说会被合并成一次提取、没有取代可言」）。
    //
    // **增量提取只能在「同会话、两次独立提取」里量**：`from_cursor` 按会话各算各的，
    // 而一次运行里只发生一次提取时，那个值恒是 0（那正是 H1 那条缺陷的症状）。
    // 所以要么回合之间等，要么跨两次驱动器跑同一个 session id——而后者撞
    // `session "…" already exists`（`session/prompt` 只创建不续接），所以只有这一条路。
    if (index < TURNS.length - 1 && BETWEEN_MS > 0) {
      log(`回合之间多等 ${BETWEEN_MS} 毫秒，让第一次提取先跑完`);
      await sleep(BETWEEN_MS);
    }
  }
  // 提取有 30 秒静默期（壳那边 `jobs::DEBOUNCE_MS`）加一次真模型调用；内核进程必须活着，
  // 否则定时器随进程一起没——**而插件正是靠那个定时器把任务领走的**（见 H3）。
  log(`多等 ${LINGER_MS} 毫秒让提取跑完`);
  await new Promise((res) => setTimeout(res, LINGER_MS));
  await call('shutdown', {});
  log('shutdown ok');
} catch (error) {
  log(`失败：${String(error?.message ?? error)}`);
} finally {
  child.stdin.end();
  setTimeout(() => {
    child.kill('SIGKILL');
    process.exit(0);
  }, 1500);
}
