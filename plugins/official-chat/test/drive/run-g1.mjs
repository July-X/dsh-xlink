// G1 真机实跑驱动：隔离实例 + 真内核 + 真浏览器里驱动工作台。
//
// ## 它要证明什么
//
// G1 的四条断言（计划 §5）：
//
//   a. 工作台 `@` 菜单里真的多出「官方对话」这一组（不是「registerSource 可调用」）；
//   b. 选中后继续编辑不丢字、输入位置保持；
//   c. 读取延迟期间按发送 → 提交被挂起，正文只出现一次、角色为 user；
//   d. 读取失败 → **模型请求次数为 0**、草稿保留、可见错误出口。
//
// ## 为什么不用 dsh-xlink 主壳
//
// 主壳那条路（`e2e/` 的 wdio）驱动的是**壳自己的面板**，不是内核工作台——
// 计划 §7 明写「在主壳调用命令成功不代替内核编辑器测试」。这里直接起内核的
// `web` profile（它服务的就是工作台那个浏览器 UI），再用 CDP 驱动真实浏览器：
// 那是内核编辑器真正运行的地方。
//
// ## 隔离纪律（AGENTS.md 硬红线）
//
//  · `DSH_HOME` 指向**一次性临时目录**：用户的实例、注册表、插件中央库都不在路径上；
//  · 浏览器用**一次性临时 profile**，绝不碰用户真实数据目录下的 WebView2 profile
//    （浏览器一启动就会写 Cookies/Preferences/History——写坏了回滚不了）；
//  · 凭据与端口全走环境变量，脚本里没有任何密钥。
//
// ## 用法
//
//   node plugins/official-chat/test/drive/run-g1.mjs            # 跑一轮
//   PROBE_KEEP=1 node plugins/official-chat/test/drive/run-g1.mjs   # 保留临时目录
//
// 输出是 JSON：每条断言带 `通过 / 失败 / 未执行` 与原始输出片段。

import { spawn } from 'node:child_process';
import { appendFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..', '..', '..');

/** 现役 release 树的 active 内核。**不写死版本号**：写死后升级了那个目录还在，
 * 于是会静默跑错内核，而实验结论仍然「有结果」。 */
function activeKernel() {
  if (process.env.PROBE_KERNEL) return process.env.PROBE_KERNEL;
  for (const mode of ['dsh', 'dsh-dev']) {
    const active = join(process.env.HOME ?? process.env.USERPROFILE ?? '', '.dsh-xlink', mode, 'desktop', 'active.txt');
    if (!existsSync(active)) continue;
    const version = readFileSync(active, 'utf8').trim();
    const kernel = join(process.env.HOME ?? process.env.USERPROFILE ?? '', '.dsh-xlink', mode, 'desktop', 'kernels', version);
    if (existsSync(kernel)) return kernel;
  }
  throw new Error('读不到 active.txt：用 PROBE_KERNEL 显式指定内核目录');
}

/** 问操作系统要一个空闲端口（避开 0.0.0.0 的监听）。 */
function freePort() {
  return new Promise((resolvePort, reject) => {
    const srv = createNetServer();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolvePort(port));
    });
  });
}

const PROBE_HOME = process.env.PROBE_HOME ?? join(process.env.TEMP ?? '.', `g1-probe-${Date.now()}`);
const MODEL_LOG = join(PROBE_HOME, 'model-requests.jsonl');
const EDGE_CANDIDATES = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
];

const results = [];
function verdict(name, status, detail) {
  results.push({ name, status, detail });
  console.log(`[${status}] ${name} — ${detail}`);
}

/* ------------------------------------------------------------------ 桩模型 */

/**
 * 最小 OpenAI-completions 桩：**把每个请求体原样记进 jsonl**。
 *
 * 为什么自己写而不复用 `plugins/memory/test/probe/stub-model.mjs`：G1 的断言 c
 * 要证明「正文只出现一次、角色为 user」，那需要**完整请求体**；memory 那份只
 * 记摘要形状。形状照它（openai-completions + SSE）。
 */
function startStubModel(port) {
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => {
      mkdirSync(PROBE_HOME, { recursive: true });
      appendFileSync(MODEL_LOG, `${JSON.stringify({ url: req.url, body })}\n`);
      const content = '桩模型回的一句话。';
      let payload = null;
      try { payload = JSON.parse(body); } catch { /* 非 JSON 请求也记下来 */ }
      if (payload?.stream === true) {
        res.writeHead(200, { 'content-type': 'text/event-stream' });
        res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { role: 'assistant' } }] })}\n\n`);
        for (const part of content.match(/.{1,200}/gs) ?? []) {
          res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: part } }] })}\n\n`);
        }
        res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\n`);
        res.end('data: [DONE]\n\n');
        return;
      }
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        id: 'stub-1',
        object: 'chat.completion',
        model: 'stub-chat',
        choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      }));
    });
  });
  return new Promise((done) => server.listen(port, '127.0.0.1', () => done(server)));
}

/* ------------------------------------------------------------ 隔离实例准备 */

/** 把插件整份复制进临时 home（内核沿 host 入口向上找 package.json）。 */
function stagePlugin(kernel) {
  const pluginDir = join(PROBE_HOME, 'plugins', 'official-chat');
  rmSync(pluginDir, { recursive: true, force: true });
  mkdirSync(join(pluginDir, 'node_modules', '@deepseek-ai'), { recursive: true });
  cpSync(join(repoRoot, 'plugins', 'official-chat'), pluginDir, {
    recursive: true,
    // `test/` 与 `node_modules/` 不进物化目录：前者是仓库里的测试资产，
    // 后者是链接目标。按**相对路径**判断，免得仓库路径里出现 'test' 字样就误伤。
    filter: (src) => {
      const rel = relative(join(repoRoot, 'plugins', 'official-chat'), src);
      return !rel.split(/[\\/]/).includes('test') && !rel.split(/[\\/]/).includes('node_modules');
    },
  });
  // 插件的 client 半边要 inject `inputTriggers` / `sessions` 两个**服务**；
  // 服务由内核自己的浏览器模块图提供，但物化目录里仍要有对应的包链接，
  // 否则 `dsh-client-modules` 解析包名时找不到它。
  for (const pkg of ['dsh-client-ui-input-trigger', 'dsh-client-ui-conversation', 'dsh-api-session-controller']) {
    const from = join(kernel, 'node_modules', '@deepseek-ai', pkg);
    if (!existsSync(from)) continue;
    try {
      symlinkSync(from, join(pluginDir, 'node_modules', '@deepseek-ai', pkg));
    } catch { /* 链接失败不致命：真跑时内核会报 did not activate */ }
  }
  return join(pluginDir, 'host', 'index.js');
}

/** 覆盖层：插插件 + 指向桩模型。**不硬编码任何凭据。** */
function writeOverlay(pluginEntry, modelPort) {
  const yaml = [
    '# 由 run-g1.mjs 生成——端口来自环境变量，这个文件里没有密钥。',
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
    `        baseURL: http://127.0.0.1:${modelPort}/v1`,
    '        models:',
    '          - id: stub-chat',
    '            name: Stub Chat',
    '            input: [text]',
    '            contextWindow: 20000',
    '            maxTokens: 8192',
    '- insert:',
    '    - id: xlink-official-chat',
    `      name: ${JSON.stringify(pluginEntry)}`,
    '',
  ].join('\n');
  mkdirSync(PROBE_HOME, { recursive: true });
  const overlay = join(PROBE_HOME, 'g1-overlay.yml');
  writeFileSync(overlay, yaml, { mode: 0o600 });
  return overlay;
}

/* ------------------------------------------------------------------ CDP */

class Cdp {
  constructor(ws) { this.ws = ws; this.id = 0; this.pending = new Map(); this.sessionId = undefined; }

  static async connect(port) {
    const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
    const ws = new WebSocket(version.webSocketDebuggerUrl);
    await new Promise((done, fail) => { ws.onopen = done; ws.onerror = fail; });
    const cdp = new Cdp(ws);
    ws.onmessage = (event) => {
      const frame = JSON.parse(event.data);
      if (frame.id !== undefined && cdp.pending.has(frame.id)) {
        const { resolve: done, reject } = cdp.pending.get(frame.id);
        cdp.pending.delete(frame.id);
        if (frame.error) reject(new Error(JSON.stringify(frame.error)));
        else done(frame.result);
      }
    };
    return cdp;
  }

  send(method, params = {}, sessionId = this.sessionId) {
    const id = ++this.id;
    const payload = { id, method, params };
    if (sessionId !== undefined) payload.sessionId = sessionId;
    this.ws.send(JSON.stringify(payload));
    return new Promise((done, reject) => {
      this.pending.set(id, { resolve: done, reject });
      setTimeout(() => { if (this.pending.delete(id)) reject(new Error(`${method} 超时`)); }, 60_000);
    });
  }

  async open(url) {
    const { targetId } = await this.send('Target.createTarget', { url });
    const { sessionId } = await this.send('Target.attachToTarget', { targetId, flatten: true });
    this.sessionId = sessionId;
    return targetId;
  }

  /** 在页面里跑一段 async 脚本并拿回值。 */
  async evaluate(body) {
    const result = await this.send('Runtime.evaluate', {
      expression: `(async () => { ${body} })()`,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? JSON.stringify(result.exceptionDetails));
    }
    return result.result?.value;
  }
}

/* ------------------------------------------------------------------ 主流程 */

const kernel = activeKernel();
const modelPort = await freePort();
const webPort = await freePort();
const cdpPort = await freePort();
const browserProfile = join(PROBE_HOME, 'browser-profile');

mkdirSync(PROBE_HOME, { recursive: true });
console.log(`内核 ${kernel}`);
console.log(`隔离 home ${PROBE_HOME}`);

const stub = await startStubModel(modelPort);
console.log(`桩模型 127.0.0.1:${modelPort} → ${MODEL_LOG}`);

const pluginEntry = stagePlugin(kernel);
const overlay = writeOverlay(pluginEntry, modelPort);
console.log(`插件入口 ${pluginEntry}`);

/**
 * 起内核用的 node 可执行文件。
 *
 * **不用 `process.execPath`**：本机它指向 `C:\Program Files\nodejs\node.exe`，
 * 那个路径不存在（node 装在别处），spawn 直接 ENOENT。走 PATH 解析最稳；
 * 需要指定别的 node 时用 `PROBE_NODE`。
 */
const NODE_BIN = process.env.PROBE_NODE ?? 'node';

const webLog = join(PROBE_HOME, 'web.log');
mkdirSync(join(PROBE_HOME, 'work'), { recursive: true });
const web = spawn(
  NODE_BIN,
  [
    join(kernel, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js'),
    // **`--patch` 是启动器的选项，必须排在 app 名（`web`）之前**：app 名之后的
    // 参数都传给 app 自己，而 `web` 不认识 `--patch`（实测报 unknown option）。
    '--patch', overlay,
    '--profile', 'web',
    '--no-open',
    '--host', '127.0.0.1',
    '--port', String(webPort),
  ],
  {
    cwd: join(PROBE_HOME, 'work'),
    env: { ...process.env, DSH_HOME: PROBE_HOME, PROBE_API_KEY: 'stub' },
    stdio: ['ignore', 'pipe', 'pipe'],
  },
);
let webOut = '';
web.stdout.on('data', (chunk) => { webOut += chunk.toString('utf8'); appendFileSync(webLog, chunk); });
web.stderr.on('data', (chunk) => { webOut += chunk.toString('utf8'); appendFileSync(webLog, chunk); });

/**
 * 等内核把 URL 打印出来。
 *
 * **必须连 query 一起拿**：工作台的 URL 带 `?token=…`（浏览器信任栅栏
 * `/api browser-trust fence` 认它）。只取到 `http://127.0.0.1:PORT/` 的实测
 * 后果是——页面开了，但 app 没初始化，插件的 client 半边自然不在（第一轮
 * 实跑就栽在这里：探针 `present: false`）。
 */
async function waitForUrl(timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const match = webOut.match(/https?:\/\/127\.0\.0\.1:\d+\/\S+/);
    if (match) return match[0].replace(/[)\],'"\s]+$/, '');
    if (web.exitCode !== null) throw new Error(`内核进程退出了（code ${web.exitCode}）：\n${webOut.slice(-2000)}`);
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`等内核起服务超时（${timeoutMs}ms）：\n${webOut.slice(-2000)}`);
}

let browser;
let cdp;
try {
  const url = await waitForUrl();
  console.log(`工作台 ${url}`);

  const edge = EDGE_CANDIDATES.find((p) => existsSync(p));
  if (edge === undefined) throw new Error('本机没有 Edge/Chrome 可驱动');
  browser = spawn(edge, [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions',
    `--user-data-dir=${browserProfile}`,
    `--remote-debugging-port=${cdpPort}`,
    'about:blank',
  ], { stdio: 'ignore' });

  // 等 CDP 端口起来
  const deadline = Date.now() + 60_000;
  for (;;) {
    try { await fetch(`http://127.0.0.1:${cdpPort}/json/version`); break; } catch {
      if (Date.now() > deadline) throw new Error('CDP 端口没起来');
      await new Promise((r) => setTimeout(r, 300));
    }
  }
  cdp = await Cdp.connect(cdpPort);
  await cdp.open(url);
  await cdp.send('Runtime.enable');

  // 探针出现要时间：client 模块图是懒加载的，页面刚开时可能还没有。
  // 轮询到它出现为止，超时才算失败——**不把「还没加载」记成「没加载」**。
  let probe = { present: false };
  for (let i = 0; i < 40; i += 1) {
    probe = await cdp.evaluate(`
      const p = window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
      return p ? { present: true, env: p.env, events: p.events.map((e) => e.kind) } : { present: false };
    `);
    if (probe?.present) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  if (!probe?.present) {
    // 没出现时把页面自己的状态带出来：标题、正文前若干字、ModuleLoader 在不在。
    const page = await cdp.evaluate(`
      return {
        title: document.title,
        hasLoader: typeof window.__ModuleLoader__ !== 'undefined',
        bodyStart: document.body ? document.body.innerText.slice(0, 400) : '(no body)',
      };
    `);
    verdict('客户端半边被内核加载（探针在页面里）', '失败', JSON.stringify({ probe, page }));
  } else {
    verdict('客户端半边被内核加载（探针在页面里）', '通过', JSON.stringify(probe));
  }

  if (probe?.present) {
    // 断言 a：@ 菜单里真的多出「官方对话」这一组
    //
    // **前置**：工作台页面刚打开时**没有会话**，也就没有 composer（编辑器）
    // ——实测页面上只有一个 `contenteditable="false"` 的元素。所以先开一个
    // 会话，等编辑器出现，再谈 @ 菜单。
    const menu = await cdp.evaluate(`
      // ① 首页是**空态**（没有会话）：它的提示语就是「描述你想要构建的内容,
      //    / 调用指令, @ 文件或对话」，主 CTA 是「继续」。先进工作台。
      const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
      // 编辑器可能是 contenteditable、textarea 或 role=textbox —— 先把三种
      // 都收进来，不预设。
      const editorOf = () =>
        document.querySelector('[contenteditable="true"]')
        ?? document.querySelector('textarea')
        ?? document.querySelector('[role="textbox"]');

      const clickables = () => [...document.querySelectorAll('button, a, [role="button"]')];
      const byText = (words) => {
        const direct = clickables().find((el) => {
          const t = (el.innerText || el.textContent || '').trim();
          return words.some((w) => t === w || t.startsWith(w));
        });
        if (direct) return direct;
        const deep = [...document.querySelectorAll('*')]
          .filter((el) => {
            const t = (el.innerText || '').trim();
            return words.some((w) => t === w) && el.children.length <= 1;
          })
          .pop();
        return deep;
      };

      // **先诊断**：把「像编辑器的东西」与「继续」的真实形态打出来，
      // 免得每次都靠猜。（这段跑在**页面里**，所以不能用外层的 cdp.evaluate。）
      const shape = {
        textareas: document.querySelectorAll('textarea').length,
        textboxes: document.querySelectorAll('[role="textbox"]').length,
        editable: document.querySelectorAll('[contenteditable]').length,
        inputs: document.querySelectorAll('input').length,
        continueEls: [...document.querySelectorAll('*')]
          .filter((el) => (el.innerText || '').trim() === '继续')
          .slice(0, 5)
          .map((el) => ({ tag: el.tagName, cls: String(el.className || '').slice(0, 80), kids: el.children.length, html: el.outerHTML.slice(0, 160) })),
        sample: (() => {
          const el = document.querySelector('[contenteditable]');
          return el ? { tag: el.tagName, ce: el.getAttribute('contenteditable'), cls: String(el.className || '').slice(0, 80) } : null;
        })(),
      };
      console.log('[诊断] 编辑器形态 ' + JSON.stringify(shape));

      const clicked = [];
      for (const words of [['继续', 'Continue'], ['新建', '新会话', '新对话', 'New chat', 'New Chat']]) {
        const btn = byText(words);
        if (btn) { btn.click(); clicked.push((btn.innerText || btn.textContent || '').trim()); }
        let editor = editorOf();
        for (let i = 0; i < 30 && !editor; i++) { await sleep(500); editor = editorOf(); }
        if (editor) break;
      }

      // ② 等编辑器出现（最多 20 秒）
      let editor = editorOf();
      for (let i = 0; i < 40 && !editor; i++) { await sleep(500); editor = editorOf(); }
      if (!editor) {
        return {
          error: 'no-editor',
          clicked,
          buttons: [...document.querySelectorAll('button')].map((b) => (b.innerText || '').trim()).filter(Boolean).slice(0, 25),
          bodyStart: document.body ? document.body.innerText.slice(0, 600) : '(no body)',
          href: String(location.href),
        };
      }

      // ③ 在编辑器里敲 '@'，等菜单
      editor.focus();
      document.execCommand('insertText', false, '@');
      await sleep(2500);
      const text = document.body.innerText;
      return {
        clicked,
        hasGroup: text.includes('官方对话'),
        hasCandidate: text.includes('G1 固定对话 Alpha'),
        snippet: text.slice(0, 600),
      };
    `);
    verdict('断言 a：@ 菜单里出现「官方对话」分组', menu?.hasGroup ? '通过' : '失败', JSON.stringify(menu));
  }

  const modelLog = existsSync(MODEL_LOG) ? readFileSync(MODEL_LOG, 'utf8') : '';
  verdict('模型请求条数（断言 c/d 的判据基础）', '未执行', `已记录 ${modelLog.split('\n').filter(Boolean).length} 条`);
} catch (error) {
  verdict('实跑', '失败', String(error?.message ?? error));
} finally {
  try { cdp?.ws.close(); } catch { /* ignore */ }
  try { browser?.kill('SIGKILL'); } catch { /* ignore */ }
  try { web.kill('SIGKILL'); } catch { /* ignore */ }
  try { stub.close(); } catch { /* ignore */ }
  if (!process.env.PROBE_KEEP) {
    // 临时目录可能还开着文件（内核/浏览器刚被杀，句柄未释放）：清理失败**不是**
    // 判据失败，但要说清残留在哪，别让人以为已经删干净了。
    try {
      rmSync(PROBE_HOME, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
    } catch (error) {
      console.log(`[警告] 临时目录没删干净（${error.code ?? error.message}）：${PROBE_HOME}`);
    }
  } else {
    console.log(`保留隔离 home：${PROBE_HOME}`);
  }
  console.log(JSON.stringify({ results }, null, 2));
  process.exit(results.some((r) => r.status === '失败') ? 1 : 0);
}
