// **桩**驱���器：一个 `dsh --profile sdk` 长驻进程里跑多回合，配桩模型 + 桩桥，零模型额度。
//
// 它与 `drive-real-model.mjs` 的关系：**同一套长驻协议，不同的两端**——那个接真模型 + 真桥，
// 这个接桩模型 + 桩桥。什么时候用哪个：要看「判据质量」用真的那个（桩模型回固定内容，
// 验不了「模型提的是不是人话」）；要看「这条链长什么样」「压缩之后预算有没有被清零」
// 用这个（确定性、可反复、零额度）。
//
// ## 为什么是 `sdk` profile 而不是 `headless`
//
// `dsh headless` **一回合一个进程**，而记忆插件的注入预算（每会话 8 次）存在进程内存里——
// 于是「压缩之后预算有没有被清零」这类问题在 headless 上验不了（两组都会每回合重新开始）。
// sdk profile 是 JSON-RPC over stdio 的长驻服务，一个进程里能连跑多回合。
//
// ## 用法（见 docs/features/memory/memory-progress.md 的「怎么验」）
//
//   # ① 另一个终端：两个桩
//   node plugins/memory/test/probe/stub-model.mjs &
//   node plugins/memory/test/probe/stub-bridge.mjs &
//   # ② 这里：node plugins/memory/test/probe/drive-stub.mjs <回合数> <标签>
//   #   之后看 $PROBE_HOME/bridge.log：/context 的次数就是这个会话真实的取检索次数
//   # ③ 收工：pkill -f 'test/probe/stub-'
//
// ## 覆盖层里那三条压缩参数，每个都有来由（踩过的坑，别删）
//
// 1. `headroomTokens` 默认 **65536**——窗口两万 token 时阈值 `min(ratio×窗口, 窗口−预留−headroom)`
//    在数学上**不可能触发**，表现为「压缩永远不发生」。
// 2. `headroomTokens: 0` 会让 `maxTokens` 默认取到 **0**，而构造期断言要求它为正 ——
//    **整个压缩插件因此不激活**，只有一行 `did not activate` 的警告，很容易漏。
// 3. `retainTokens` 必须**小于**阈值（`0.001 × 20000 ≈ 20`）。
//
// ## 只写 /tmp
//
// `DSH_HOME` 指到 `PROBE_HOME`，于是用户的实例、注册表、插件中央库都不在路径上。
// 凭据与端口全走环境变量，这个文件里不含任何密钥。
import { spawn } from 'node:child_process';
import { appendFileSync, writeFileSync, mkdirSync, rmSync, cpSync, symlinkSync, existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..', '..', '..');
const TURNS = Number(process.argv[2] ?? 6);
const LABEL = process.argv[3] ?? 'stub';
const PROBE_HOME = process.env.PROBE_HOME ?? '/tmp/p2probe';
const MODEL_PORT = Number(process.env.PROBE_MODEL_PORT ?? 8791);
const BRIDGE_PORT = Number(process.env.PROBE_BRIDGE_PORT ?? 8792);
const BRIDGE_TOKEN = process.env.PROBE_BRIDGE_TOKEN ?? 'probe-token';
const LINGER_MS = Number(process.env.PROBE_LINGER_MS ?? 0);
const RPC_TIMEOUT = Number(process.env.PROBE_RPC_TIMEOUT_MS ?? 120_000);

/**
 * 当前 active 的那一版内核：读不到就报错。
 *
 * **为什么不写死版本号**：上一版驱动器写的是 `kernels/0.2.1-alpha.2`，而升级之后那个目录
 * 还在——于是它会静默跑错内核，实验结论仍然「有结果」，但那个结果是旧内核的。
 */
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

/** 备场：插件副本 + 内核包的软链。**只往 PROBE_HOME 写**（理由同真模型驱动器）。 */
function prepare(kernel) {
  const pluginDir = join(PROBE_HOME, 'plugins', 'memory');
  rmSync(pluginDir, { recursive: true, force: true });
  mkdirSync(join(pluginDir, 'node_modules', '@deepseek-ai'), { recursive: true });
  // spawn 的 cwd：**必须先建出来**。目录不存在时 Node 报的是
  // `spawn <node 绝对路径> ENOENT`（错指着 node 二进制本身），排障的第一反应
  // 全是去查 node 装没装——2026-10-24 复验时栽过这一下，老 home 里恰好留着
  // 历史目录才一直没暴露。
  mkdirSync(join(PROBE_HOME, 'work'), { recursive: true });
  cpSync(join(repoRoot, 'plugins', 'memory'), pluginDir, {
    recursive: true,
    filter: (src) => !src.includes('node_modules'),
  });
  // **插件自己的相对路径解析不了**：按包名插入的东西在 sdk profile 下找不到，
  // 而内核模板自带的那几个走安装树。
  for (const pkg of ['dsh-tools', 'dsh-llm']) {
    symlinkSync(
      join(kernel, 'node_modules', '@deepseek-ai', pkg),
      join(pluginDir, 'node_modules', '@deepseek-ai', pkg)
    );
  }
  return join(pluginDir, 'host', 'index.js');
}

/**
 * 写覆盖层。
 *
 * **为什么是生成的而不是仓里放一个 yml**：覆盖层里有内核版本相关的路径（插件入口的绝对
 * 路径）与桩的端口，写死在文件里就要手工同步三处。而「同步」这件事没有判据钉着。
 *
 * **`dsh-llm-pi-ai` 是打配置补丁而不是插一个新插件**：`@deepseek-ai/dsh-llm-deepseek`
 * 是**库不是插件**（它导出 `registerDeepSeekProvider`，没有 `name` / `apply`）。而往栈里
 * 插一个**同 id** 的适配器会把模板那一份静默顶掉，症状是
 * `NO_ADAPTER: no adapter registered for provider "probe"`。
 */
function writeOverlay(pluginEntry) {
  const yaml = [
    '# 由 drive-stub.mjs 生成——凭据与端点来自环境变量，这个文件里没有密钥。',
    '- id: agent-default-model',
    '  name: "@deepseek-ai/dsh-agent-default-model"',
    '  config:',
    '    provider: probe',
    '    model: stub-chat',
    '- id: llm-pi-ai',
    '  name: "@deepseek-ai/dsh-llm-pi-ai"',
    '  config:',
    '    providers:',
    '      probe:',
    '        apiKeyEnv: PROBE_API_KEY',
    '        api: openai-completions',
    `        baseURL: http://127.0.0.1:${MODEL_PORT}/v1`,
    '        models:',
    '          - id: stub-chat',
    '            name: Stub Chat',
    '            input: [text]',
    '            contextWindow: 20000',
    '            maxTokens: 8192',
    // 压缩三条参数：理由在文件头，逐条列了踩过的坑。
    '- id: compaction-basic',
    '  name: "@deepseek-ai/dsh-compaction-basic"',
    '  config:',
    '    thresholdRatio: 0.001',
    '    headroomTokens: 0',
    '    maxTokens: 512',
    '    retainTokens: 10',
    '- insert:',
    '    - id: xlink-memory',
    `      name: ${JSON.stringify(pluginEntry)}`,
    '',
  ].join('\n');
  const overlay = join(PROBE_HOME, 'stub-overlay.yml');
  mkdirSync(PROBE_HOME, { recursive: true });
  writeFileSync(overlay, yaml, { mode: 0o600 });
  return overlay;
}

const kernel = activeKernel();
const pluginEntry = prepare(kernel);
const overlay = writeOverlay(pluginEntry);
const LOG = join(PROBE_HOME, `sdk-${LABEL}.log`);
const log = (line) => appendFileSync(LOG, `${line}\n`);
writeFileSync(LOG, '');
log(`内核 ${kernel}`);
log(`覆盖层 ${overlay}`);
log(`回合 ${TURNS}；桩模型 :${MODEL_PORT} · 桩桥 :${BRIDGE_PORT}`);

const child = spawn(
  process.execPath,
  [join(kernel, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js'), '--profile', 'sdk', '--patch', overlay],
  {
    // cwd 指向空的临时目录：模型带着 bash 工具时，不该让它在内核安装树上跑。
    cwd: join(PROBE_HOME, 'work'),
    env: {
      ...process.env,
      DSH_HOME: PROBE_HOME,
      PROBE_API_KEY: process.env.PROBE_API_KEY ?? 'stub',
      // **默认打桩桥，但允许外面覆盖**：指向真桥时靠的就是这两个变量（第一版硬编码成
      // 桩桥，于是「指向真桥」那次运行其实还在打桩桥，而桩桥已经被关了——看起来像插件没打）。
      DSH_XLINK_BUILTIN_BRIDGE_URL:
        process.env.DSH_XLINK_BUILTIN_BRIDGE_URL ?? `http://127.0.0.1:${BRIDGE_PORT}`,
      DSH_XLINK_BUILTIN_BRIDGE_TOKEN:
        process.env.DSH_XLINK_BUILTIN_BRIDGE_TOKEN ?? BRIDGE_TOKEN,
    },
    stdio: ['pipe', 'pipe', 'pipe'],
  },
);

let nextId = 1;
const pending = new Map();
// 会话 id 由客户端给，而**它是落盘的**：同一个 id 第二次跑会撞 `already exists`。
const sessionId = `sdk-${LABEL}-${Date.now()}`;
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
      // **`did not activate` 那几行要打**：插件/压缩插件**配置被断言拒绝时是静默不激活的**，
      // 症状与「条件没到」一模一样（G12 栽过两次）。
      const data = frame.params?.event?.data;
      if (
        type === 'turn/end' ||
        type === 'compaction/start' ||
        type === 'compaction/end'
      ) {
        const reason = data?.reason;
        log(
          `event ${type} status=${typeof reason === 'string' ? reason : (reason?.kind ?? '')}` +
            (data?.error ? ` error=${data.error}` : '')
        );
      }
      if (type === 'log' || String(data?.message ?? '').includes('did not activate')) {
        log(`log: ${JSON.stringify(data).slice(0, 300)}`);
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
    log(`server notification ${frame.method ?? '?'} ${line.slice(0, 200)}`);
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
    model: 'stub-chat',
  });
  log(`initialize → ${JSON.stringify(initialized).slice(0, 300)}`);
  for (let turn = 1; turn <= TURNS; turn += 1) {
    await call('session/prompt', {
      sessionId,
      contentBlocks: [{ type: 'text', text: `第${turn}回合` }],
    });
    await call('session/wait', { sessionId });
    log(`turn ${turn} 结束`);
  }
  // 挂住那条路要等提取自己的时限（120 秒）到，**内核进程得活着**——不然定时器随进程一起没了。
  if (LINGER_MS > 0) {
    log(`多等 ${LINGER_MS} 毫秒再收工`);
    await new Promise((res) => setTimeout(res, LINGER_MS));
  }
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
