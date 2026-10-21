// G1 真机实跑驱动：隔离实例 + 真内核 + 真浏览器里驱动工作台。
//
// ## 它要证明什么
//
// G1 的四条断言（计划 §5）：
//
//   a. 工作台 `@` 菜单里真的多出「Web Chat」这一组（不是「registerSource 可调用」）；
//   b. 选中后继续编辑不丢字、输入位置保持；
//   c. 读取延迟期间按发送 → 提交被挂起，正文只出现一次、角色为 user；
//   d. 读取失败 → **模型请求次数为 0**、草稿保留、可见错误出口。
//
// 每条断言都给「通过 / 失败 / 未执行」+ 原始输出片段。**没跑到的写未执行，不写通过。**
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
// ## 等东西的正确姿势（2026-10-20 第 5 轮踩过的坑，别退回旧写法）
//
// `[data-composer-card]` **立刻就有**，但里面的输入框要等内核把 input 状态建起来
// 才可编辑：内核 `ui-conversation/client.js@0.2.1-rc.2:19028` 写的是
// `phase: input?.phase ?? "inert"` —— input 还没到，编辑器就是
// `contenteditable="false" data-phase="inert"`。
//
// 第 5 轮就是**等错了东西**：等到了卡片就立刻找编辑器，于是拿到那个 inert 的壳，
// 报 `no-visible-editor`，看起来像「工作台没有编辑器」。
// 实测（2026-10-20 第 6/7 轮诊断）：页面加载后约 10 秒内编辑器变成
// `contenteditable="true" data-phase="plain"`（尺寸 651×52）。
//
// 所以这里等的是**可编辑**的编辑器，不是卡片：
//
//   `[data-composer-card] [data-composer-input][contenteditable="true"]`
//   且 `getBoundingClientRect()` 宽高都 > 0（内核会渲染一个零尺寸的隐藏
//   contenteditable，命中它等于没聚焦）。
//
// ## 输入必须用真实输入事件
//
// `@` 要用 CDP `Input.insertText`（真实 input 事件）。内核的 @ 菜单是 React
// 渲染的，行上挂了 pointer/mouse 处理器：`HTMLElement.click()` 只派发一个合成
// `click`，不产生 pointerdown/mousedown，菜单不收下这次选择——实测表现为
// 「菜单行被点中了，但插件的 onPick 根本没被调用」。所以点菜单行也用真实鼠标事件。
//
// ## 用法
//
//   node plugins/official-chat/test/drive/run-g1.mjs            # 跑一轮
//   PROBE_KEEP=1 node plugins/official-chat/test/drive/run-g1.mjs   # 保留临时目录
//   G1_ROUND=r7 node plugins/official-chat/test/drive/run-g1.mjs     # 指定轮次戳
//
// 证据按轮次独立命名（`model-requests.<round>.jsonl` / `results.<round>.json`），
// **不复用同名文件**——后一轮的空跑会把前一轮的原始依据擦掉。

import { spawn } from 'node:child_process';
import { appendFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..', '..', '..', '..');

/** 插件的固定夹具（与 `plugins/official-chat/client/client.js` 的 FIXTURES 一致）。 */
const FIXTURE_ALPHA = 'G1 固定对话 Alpha';
const FIXTURE_BETA = 'G1 固定对话 Beta';
const BODY_ALPHA = '【G1 固定正文 Alpha】第一行固定正文。';
const BODY_BETA = '【G1 固定正文 Beta】只有一行。';

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
/** 轮次戳：**证据文件按轮次独立命名，不复用同名** —— 2026-10-20 的教训：
 * 第 4 轮的空跑把 `model.log` 覆盖成 0 字节，前一轮「请求体含夹具正文」那条
 * 结论的**原始依据就此不可复核**。 */
const ROUND = process.env.G1_ROUND ?? `r${new Date().toISOString().replace(/[-:.TZ]/g, '').slice(0, 14)}`;
/**
 * 证据目录：**放在隔离 home 之外**。
 *
 * 跑完会删掉 `PROBE_HOME`（隔离纪律），而证据（截图、结果 JSON）必须留下——
 * 第 13 轮就是这么丢的：断言 d 的 Toast 截图写进了 `PROBE_HOME`，
 * 收尾时被一起删掉，只剩控制台里的一段文字。
 */
const EVIDENCE_DIR = process.env.G1_EVIDENCE_DIR ?? join(process.env.TEMP ?? '.', 'dsh-xlink-g1', `evidence-${ROUND}`);
const MODEL_LOG = join(EVIDENCE_DIR, `model-requests.${ROUND}.jsonl`);
const RESULTS_FILE = join(EVIDENCE_DIR, `results.${ROUND}.json`);
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
      mkdirSync(EVIDENCE_DIR, { recursive: true });
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

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/* ------------------------------------------------------- 页面操作原语 */

/** 页面上有没有可编辑的编辑器（**判据见文件头**）。 */
const EDITOR_SELECTOR = '[data-composer-card] [data-composer-input][contenteditable="true"]';

/**
 * 等「可编辑」的编辑器出现。
 *
 * @returns {Promise<{ok: boolean, waitedMs: number, phase: string|null, editable: string|null, card: boolean}>}
 */
async function waitComposerEditable(cdp, timeoutMs = 120_000) {
  const t0 = Date.now();
  let last = { card: false, phase: null, editable: null };
  for (;;) {
    last = await cdp.evaluate(`
      const card = document.querySelector('[data-composer-card]');
      const input = card ? card.querySelector('[data-composer-input]') : null;
      const rect = input ? input.getBoundingClientRect() : null;
      const sized = !!rect && rect.width > 0 && rect.height > 0;
      return {
        card: !!card,
        phase: input ? input.getAttribute('data-phase') : null,
        editable: input ? input.getAttribute('contenteditable') : null,
        sized,
      };
    `);
    if (last.editable === 'true' && last.sized) {
      return { ok: true, waitedMs: Date.now() - t0, ...last };
    }
    if (Date.now() - t0 > timeoutMs) {
      return { ok: false, waitedMs: Date.now() - t0, ...last };
    }
    await sleep(500);
  }
}

/**
 * 关掉页面上的模态弹窗（预览版说明等）。**用真实鼠标事件**。
 *
 * ## 为什么要「等它出现」而不是「有就关」（2026-10-20 第 9 轮实测）
 *
 * 「预览版说明」弹窗是**延迟出现**的：插件探针亮起（≈T+3s）时它还没渲染，
 * 断言 a 那一刻（≈T+10s）它才在。于是「有就关」的写法一轮都没关掉，
 * 弹窗盖在 composer 上面，`@` 打进去没反应，菜单里当然没有「Web Chat」。
 * 所以这里**先等弹窗出现（最多 appearMs）**，关掉后再等一轮。
 *
 * ## 按钮怎么找
 *
 * 实测那个「继续」按钮是 `button`（innerText 恰为「继续」），但同页还有别的
 * 形态；候选集合取宽一点（`button,[role="button"],a,li,div[tabindex]`），
 * 文案先精确后前缀，最后再退到「关闭/Close」的 aria-label。
 */
async function dismissDialogs(cdp, { appearMs = 30_000, maxRounds = 4 } = {}) {
  const closed = [];
  const t0 = Date.now();
  const pick = `
    const norm = (el) => (el.innerText || el.textContent || '').trim().replace(/\\s+/g, ' ');
    const dialogs = [...document.querySelectorAll('[role="dialog"]')];
    const candidates = [...document.querySelectorAll('button,[role="button"],a,li,div[tabindex]')];
    const exact = /^(继续|我知道了|知道了|确定|关闭|完成|稍后|Continue|Close)$/;
    const prefix = /^(继续|我知道了|知道了|确定|关闭|完成|稍后)/;
    const inDialog = dialogs.flatMap((d) => [...d.querySelectorAll('button,[role="button"],a,li,div[tabindex]')]);
    const hit = inDialog.find((b) => exact.test(norm(b)))
      ?? candidates.find((b) => exact.test(norm(b)))
      ?? inDialog.find((b) => prefix.test(norm(b)))
      ?? candidates.find((b) => prefix.test(norm(b)))
      ?? candidates.find((b) => /^(关闭|Close)$/i.test((b.getAttribute('aria-label') ?? '').trim()));
    if (!hit) return { dialogs: dialogs.length, hit: null };
    hit.scrollIntoView({ block: 'center' });
    const r = hit.getBoundingClientRect();
    return { dialogs: dialogs.length, hit: { text: norm(hit) || (hit.getAttribute('aria-label') ?? ''), x: r.left + r.width / 2, y: r.top + r.height / 2 } };
  `;
  for (let round = 0; round < maxRounds; round += 1) {
    // 等弹窗出现（有弹窗但没找到可点的按钮 → 不再等，直接放弃这一轮）
    let found = null;
    while (Date.now() - t0 < appearMs) {
      found = await cdp.evaluate(pick);
      if (found?.hit) break;
      if ((found?.dialogs ?? 0) > 0) break;
      await sleep(400);
    }
    if (!found?.hit) break;
    const base = { x: found.hit.x, y: found.hit.y, button: 'left', clickCount: 1, buttons: 1 };
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...base, buttons: 0 });
    await sleep(80);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...base });
    await sleep(60);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...base, buttons: 0 });
    closed.push(found.hit.text);
    await sleep(800);
  }
  const left = await cdp.evaluate(`document.querySelectorAll('[role="dialog"]').length`);
  return { closed, left };
}

/** 聚焦编辑器（可编辑状态下）。 */
async function focusComposer(cdp) {
  return cdp.evaluate(`
    const el = document.querySelector(${JSON.stringify(EDITOR_SELECTOR)});
    if (!el) return { focused: false };
    el.focus();
    return { focused: document.activeElement === el, text: (el.innerText || '').slice(0, 80) };
  `);
}

/** 真实输入一段文字（CDP `Input.insertText` → 真实 input 事件）。 */
async function typeText(cdp, text) {
  await focusComposer(cdp);
  await cdp.send('Input.insertText', { text });
  await sleep(300);
}

/** 按一次真实回车。`rawKeyDown` 不带 text，字符部分靠 `text: '\r'`。 */
async function pressEnterSubmit(cdp) {
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyDown', text: '\r', unmodifiedText: '\r',
    key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13,
  });
  await sleep(60);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13,
  });
  await sleep(200);
}

/**
 * 按文案点一个元素，用**真实鼠标事件**（mousedown/mouseup）。
 *
 * 为什么不能只靠 `el.click()`：内核的 @ 菜单是 React 渲染的，行上挂了
 * pointer/mouse 处理器；`HTMLElement.click()` 只派发一个合成 `click`，
 * 不产生 pointerdown / mousedown，菜单因此不收下这次选择——
 * 实测表现为「菜单行被点中了，但插件的 onPick 根本没被调用」。
 */
async function realClickByText(cdp, label, { prefix = false, selector = 'button,[role="option"],[role="menuitem"],li,div[tabindex],[data-value]' } = {}) {
  const found = await cdp.evaluate(`
    const target = ${JSON.stringify(label)};
    const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
    const norm = (el) => (el.innerText || el.textContent || '').trim().replace(/\\s+/g, ' ');
    const hit = all.find((el) => norm(el) === target)
      ?? (${prefix} ? all.find((el) => norm(el).startsWith(target)) : undefined)
      ?? all.find((el) => norm(el).includes(target));
    if (!hit) return { ok: false, labels: all.map(norm).filter(Boolean).slice(0, 60) };
    hit.scrollIntoView({ block: 'center' });
    const r = hit.getBoundingClientRect();
    return { ok: true, text: norm(hit), x: r.left + r.width / 2, y: r.top + r.height / 2 };
  `);
  if (!found?.ok) return { clicked: false, labels: found?.labels ?? [] };
  const base = { x: found.x, y: found.y, button: 'left', clickCount: 1, buttons: 1 };
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...base, buttons: 0 });
  await sleep(80);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...base });
  await sleep(60);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...base, buttons: 0 });
  await sleep(400);
  return { clicked: true, text: found.text };
}

/**
 * 清空草稿（Ctrl+A 然后 Delete，走真实键盘事件）。
 *
 * 为什么需要：断言 a 会在草稿里留下一个 `@`；接着再打字会**把 @ 菜单关掉**
 * （触发词不再是有效的触发位置），于是「点菜单行」必然落空。挑选前必须先清空。
 */
async function clearDraft(cdp) {
  await focusComposer(cdp);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65, modifiers: 2,
  });
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65, modifiers: 2,
  });
  await sleep(150);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown', key: 'Delete', code: 'Delete', windowsVirtualKeyCode: 46, nativeVirtualKeyCode: 46,
  });
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'Delete', code: 'Delete', windowsVirtualKeyCode: 46, nativeVirtualKeyCode: 46,
  });
  await sleep(400);
}

/**
 * 等 @ 菜单里出现目标行。
 *
 * 菜单**先渲染、候选项后到**：插件的 `candidates()` 是异步的，刚打完 `@` 时
 * 菜单里可能还没有「Web Chat」那几行——实测（2026-10-20 壳内复验）第一次 pick
 * 就因此点不中、假红，而同一轮里第二次就正常。所以点之前先等这行出现，
 * 等不到就把菜单行原样带进结果，便于直接判读。
 */
async function waitMenuRow(cdp, title, timeoutMs = 8000) {
  const t0 = Date.now();
  let rows = [];
  while (Date.now() - t0 < timeoutMs) {
    const snapshot = await cdp.evaluate(`
      return [...document.querySelectorAll('[role="option"],[role="menuitem"],button,li')]
        .map((el) => (el.innerText || '').trim().replace(/\\s+/g, ' '))
        .filter((t) => t && t.length > 0 && t.length < 80).slice(0, 40);
    `);
    rows = Array.isArray(snapshot) ? snapshot : [];
    if (rows.some((t) => t.includes(title))) return { ok: true, waitedMs: Date.now() - t0, rows };
    await sleep(300);
  }
  return { ok: false, waitedMs: Date.now() - t0, rows };
}

/** 读草稿：整张卡片的 HTML（chip 在卡片里）+ 编辑器文本。 */
async function readDraft(cdp) {
  return cdp.evaluate(`
    const card = document.querySelector('[data-composer-card]');
    const el = card ? card.querySelector('[data-composer-input]') : null;
    return {
      draft: el ? (el.innerText || '') : null,
      editorHtml: el ? el.innerHTML.slice(0, 4000) : null,
      cardHtml: card ? card.innerHTML.slice(0, 8000) : null,
    };
  `);
}

/** 读整页可见文本（判断「消息是否发出」「有没有错误条」）。 */
async function readBody(cdp) {
  const text = await cdp.evaluate(`document.body ? (document.body.innerText || '') : ''`);
  return typeof text === 'string' ? text : '';
}

/** 设插件运行配置（读取延迟 / 强制失败）。 */
async function setConfig(cdp, config) {
  return cdp.evaluate(`
    window.__DSH_G1_CONFIG__ = Object.assign({}, window.__DSH_G1_CONFIG__ ?? {}, ${JSON.stringify(config)});
    return window.__DSH_G1_CONFIG__;
  `);
}

/** 读桩模型日志（每行一个请求）。 */
function readModelLog() {
  if (!existsSync(MODEL_LOG)) return [];
  return readFileSync(MODEL_LOG, 'utf8').split('\n').filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return { url: '(unparsed)', body: line }; }
  });
}

/** 在请求体里找「含夹具正文的那条 user 消息」。 */
function findUserMessageWith(request, needle) {
  let payload = null;
  try { payload = JSON.parse(request.body); } catch { return null; }
  const messages = payload?.messages ?? [];
  const hits = [];
  for (const message of messages) {
    const content = typeof message?.content === 'string'
      ? message.content
      : (message?.content ?? []).map((part) => part?.text ?? '').join('\n');
    if (String(content).includes(needle)) hits.push({ role: message?.role, content: String(content) });
  }
  return hits;
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
console.log(`轮次 ${ROUND}`);

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
    await sleep(300);
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
      await sleep(300);
    }
  }
  cdp = await Cdp.connect(cdpPort);
  await cdp.open(url);
  await cdp.send('Runtime.enable');
  // 截图取证用（断言 d 的 Toast 是自动消失的，截图是「可见」这件事的落证）。
  await cdp.send('Page.enable');

  // 探针出现要时间：client 模块图是懒加载的，页面刚开时可能还没有。
  // 轮询到它出现为止，超时才算失败——**不把「还没加载」记成「没加载」**。
  let probe = { present: false };
  for (let i = 0; i < 40; i += 1) {
    probe = await cdp.evaluate(`
      const p = window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
      return p ? { present: true, events: p.events.map((e) => e.kind) } : { present: false };
    `);
    if (probe?.present) break;
    await sleep(500);
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

  if (!probe?.present) {
    throw new Error('插件 client 半边没加载，后面的断言无从谈起');
  }

  // 关弹窗 → 等编辑器可编辑 → **再确认一次没有弹窗**。
  //
  // 弹窗是**延迟出现**的：第 9 轮在探针刚亮时就去找「继续」，一个都没关掉，
  // 断言 a 那一刻弹窗正好盖在 composer 上，`@` 打进去没反应。
  // 所以这里关完再等一次编辑器、再查一次残留。
  let dialogs = await dismissDialogs(cdp);
  let composer = await waitComposerEditable(cdp, 120_000);
  if ((dialogs.left ?? 0) > 0) {
    dialogs = await dismissDialogs(cdp);
    composer = await waitComposerEditable(cdp, 120_000);
  }
  if (!composer.ok) {
    verdict('工作台编辑器可用（可编辑且有尺寸）', '失败', JSON.stringify({ dialogs, composer }));
    throw new Error('工作台编辑器一直不可编辑，后面的断言未执行');
  }
  verdict('工作台编辑器可用（可编辑且有尺寸）', '通过', JSON.stringify({ dialogs, composer }));

  /* ---------------------------------------------------------- 断言 a */
  // @ 菜单里真的多出「Web Chat」这一组。
  await typeText(cdp, '@');
  await sleep(2000);
  const menu = await cdp.evaluate(`
    const text = document.body ? (document.body.innerText || '') : '';
    const rows = [...document.querySelectorAll('[role="option"],[role="menuitem"],button,li')]
      .map((el) => (el.innerText || '').trim().replace(/\\s+/g, ' '))
      .filter((t) => t && t.length > 0 && t.length < 80).slice(0, 40);
    return {
      hasGroup: text.includes('Web Chat'),
      hasAlpha: text.includes(${JSON.stringify(FIXTURE_ALPHA)}),
      dialogsLeft: document.querySelectorAll('[role="dialog"]').length,
      rows,
    };
  `);
  const aPass = menu?.hasGroup === true && menu?.hasAlpha === true;
  verdict('断言 a：@ 菜单里出现「Web Chat」分组', aPass ? '通过' : '失败', JSON.stringify(menu));

  /* ---------------------------------------------------------- 断言 b */
  // 选中后继续编辑不丢字、输入位置保持。
  //
  // 顺序有讲究：
  //  1. **先清空草稿**（断言 a 留下一个 `@`）；
  //  2. 前置文字**后面留一个空格**——内核的触发词正则是
  //     `/(?:^|\s)(@([^\s]*))$/`（`input-trigger/client.js@0.2.1-rc.2:725`），
  //     **`@` 前面必须是行首或空白**。写成「前置文字@」时根本不触发，
  //     菜单不开、点不中（第 11 轮实测：草稿里 `@` 原样留着）；
  //  3. 打 `@` → 等菜单 → 真实鼠标点夹具行 → 再补后置文字。
  await clearDraft(cdp);
  await typeText(cdp, '前置文字 ');
  await typeText(cdp, '@');
  const menuB = await waitMenuRow(cdp, FIXTURE_ALPHA);
  const pickB = await realClickByText(cdp, FIXTURE_ALPHA, { prefix: true });
  await sleep(800);
  const draftB1 = await readDraft(cdp);
  const chipIn = (draftB1.cardHtml ?? '').includes('data-composer-chip="official-chat"');
  await typeText(cdp, '后置文字');
  const draftB2 = await readDraft(cdp);
  const html = draftB2.cardHtml ?? '';
  const iPre = html.indexOf('前置文字');
  const iChip = html.indexOf('data-composer-chip="official-chat"');
  const iPost = html.indexOf('后置文字');
  const bDetail = {
    menuOpened: menuB?.ok === true,
    menuWaitedMs: menuB?.waitedMs ?? null,
    menuRows: menuB?.rows ?? [],
    picked: pickB?.clicked === true,
    chipLanded: chipIn,
    keptPreText: iPre >= 0,
    keptPostText: iPost >= 0,
    orderOk: iPre >= 0 && iChip > iPre && iPost > iChip,
    draftText: (draftB2.draft ?? '').slice(0, 200),
  };
  const bPass = bDetail.picked && bDetail.chipLanded && bDetail.keptPreText && bDetail.keptPostText && bDetail.orderOk;
  verdict('断言 b：选中后继续编辑不丢字、输入位置保持', bPass ? '通过' : '失败', JSON.stringify(bDetail));

  /* ---------------------------------------------------------- 断言 c */
  // 读取延迟期间按发送 → 提交被挂起，正文只出现一次、角色为 user。
  await clearDraft(cdp);
  await setConfig(cdp, { readDelayMs: 8000, failRead: false });
  await typeText(cdp, '@');
  const menuC = await waitMenuRow(cdp, FIXTURE_ALPHA);
  const pickC = await realClickByText(cdp, FIXTURE_ALPHA, { prefix: true });
  await sleep(500);
  await typeText(cdp, '请看这个');
  const beforeC = readModelLog().length;
  const t0 = Date.now();
  await pressEnterSubmit(cdp);
  let arrived = null;
  for (let i = 0; i < 60; i += 1) {
    const reqs = readModelLog().slice(beforeC);
    if (reqs.some((r) => String(r.body ?? '').includes(BODY_ALPHA))) { arrived = Date.now() - t0; break; }
    await sleep(500);
  }
  const reqsC = readModelLog().slice(beforeC);
  /**
   * 分开数「对话请求」与「其它请求」。
   *
   * 实测（第 11 轮）：一次提交会打**两条**到模型端点——
   *  1. 对话请求（system 是 `You are an AI agent…`），第一条 user 消息就是
   *     「夹具正文 + 打的字」，正文出现 **1 次**，角色 user；
   *  2. **会话标题生成**请求（system 是 `Create a concise title…`），它把
   *     human 消息序列化成 JSON 喂给模型，于是正文也出现了 1 次。
   * 第 2 条是内核自己的功能，**不是引用被发了两遍**。
   *
   * 所以判据落在**对话请求**上：条数、正文出现次数、角色；
   * 其它请求单独列出来（不隐藏），但不计入「重复发送」。
   */
  const isChatRequest = (r) => String(r.body ?? '').includes('You are an AI agent');
  const chatReqs = reqsC.filter(isChatRequest);
  const otherReqs = reqsC.filter((r) => !isChatRequest(r));
  const hitChat = chatReqs.filter((r) => String(r.body ?? '').includes(BODY_ALPHA));
  const occurrences = hitChat.map((r) => String(r.body).split(BODY_ALPHA).length - 1);
  const userHits = hitChat.flatMap((r) => findUserMessageWith(r, BODY_ALPHA) ?? []);
  const cDetail = {
    menuOpened: menuC?.ok === true,
    picked: pickC?.clicked === true,
    newRequests: reqsC.length,
    chatRequests: chatReqs.length,
    chatRequestsWithBody: hitChat.length,
    bodyOccurrences: occurrences,
    waitedMs: arrived,
    gated: arrived === null ? false : arrived >= 4000,
    userMessageRoles: userHits.map((h) => h.role),
    otherRequests: otherReqs.map((r) => ({
      purpose: String(r.body ?? '').includes('Create a concise title') ? 'session-title' : 'unknown',
      url: r.url,
      hasBody: String(r.body ?? '').includes(BODY_ALPHA),
    })),
    draftAfter: (await readDraft(cdp)).draft,
  };
  const cPass = cDetail.picked
    && cDetail.chatRequests === 1
    && cDetail.chatRequestsWithBody === 1
    && occurrences.every((n) => n === 1)
    && cDetail.userMessageRoles.includes('user')
    && cDetail.gated;
  verdict('断言 c：发送被读取门控挂起，正文只出现一次、角色为 user', cPass ? '通过' : '失败', JSON.stringify(cDetail));

  /* ---------------------------------------------------------- 断言 d */
  // 读取失败 → 模型请求次数为 0、草稿保留、可见错误出口。
  await clearDraft(cdp);
  await setConfig(cdp, { readDelayMs: 2500, failRead: true });
  await typeText(cdp, '@');
  const menuD = await waitMenuRow(cdp, FIXTURE_BETA);
  const pickD = await realClickByText(cdp, FIXTURE_BETA, { prefix: true });
  await sleep(500);
  await typeText(cdp, '这条会失败');
  const beforeD = readModelLog().length;
  await pressEnterSubmit(cdp);
  /**
   * **密集采样 + 截图**找错误提示。
   *
   * 内核把 `error` 级 notice 渲染成 **Toast**（`ui-conversation/client.js@0.2.1-rc.2:18808`
   * `if (notice?.level === "error") showToast(notice.text)`，组件带 `onDone: dismissToast`
   * → **自动消失**），`info` 级才是内联 `role="status"`（`:18982-18986`）。
   * 于是：
   *  · 单点快照必然踩在消失之后（第 11 轮实测 0 命中）→ 必须**密集采样**；
   *  · `innerText` 会漏掉被 CSS 隐藏/动画中的节点 → 同时扫 `textContent`
   *    与带 `role=alert|status|aria-live` 的节点；
   *  · 「可见」这件事最终用**截图**落证据（存 PNG，不进仓库）。
   */
  let notice = null;
  let noticeEvidence = null;
  const samples = [];
  for (let i = 0; i < 24; i += 1) {
    const scan = await cdp.evaluate(`
      const body = document.body;
      const inner = body ? (body.innerText || '') : '';
      const content = body ? (body.textContent || '') : '';
      const live = [...document.querySelectorAll('[role="alert"],[role="status"],[aria-live]')]
        .map((el) => ({
          role: el.getAttribute('role'),
          live: el.getAttribute('aria-live'),
          text: (el.textContent || '').trim().slice(0, 160),
        }))
        .filter((x) => x.text.length > 0)
        .slice(0, 6);
      const deep = [];
      for (const el of document.querySelectorAll('*')) {
        const t = el.textContent || '';
        if (t && t.length < 400 && (t.includes('读取失败') || t.includes('发送时会拒绝'))) {
          deep.push({
            tag: el.tagName,
            cls: String(el.getAttribute('class') || '').slice(0, 100),
            role: el.getAttribute('role'),
            text: t.trim().slice(0, 160),
          });
        }
      }
      return {
        innerHas: inner.includes('读取失败'),
        contentHas: content.includes('读取失败'),
        live,
        deep: deep.slice(0, 6),
        innerLength: inner.length,
      };
    `);
    const hit = scan?.innerHas === true || scan?.contentHas === true || (scan?.deep?.length ?? 0) > 0;
    samples.push({ atMs: i * 250, hit, live: scan?.live ?? [], deepCount: scan?.deep?.length ?? 0 });
    if (hit && !notice) {
      notice = (scan?.deep?.[0]?.text ?? scan?.live?.[0]?.text ?? '读取失败');
      noticeEvidence = scan;
    }
    // 截图留证（最多 3 张，只在有命中或每 8 拍一张）
    if ((hit && noticeEvidence?.shot === undefined) || i % 8 === 0) {
      try {
        const shot = await cdp.send('Page.captureScreenshot', { format: 'png' });
        const file = join(EVIDENCE_DIR, `assert-d.toast.${String(i * 250).padStart(5, '0')}ms.png`);
        writeFileSync(file, Buffer.from(shot.data, 'base64'));
        if (noticeEvidence && noticeEvidence.shot === undefined) noticeEvidence.shot = file;
      } catch { /* 截图失败不影响判据 */ }
    }
    await sleep(250);
  }
  const reqsD = readModelLog().slice(beforeD);
  const bodyD = await readBody(cdp);
  const draftD = await readDraft(cdp);
  const dDetail = {
    menuOpened: menuD?.ok === true,
    picked: pickD?.clicked === true,
    newRequests: reqsD.length,
    requestsWithBody: reqsD.filter((r) => String(r.body ?? '').includes(BODY_BETA)).length,
    draftRetained: (draftD.draft ?? '').includes('这条会失败'),
    chipRetained: (draftD.cardHtml ?? '').includes('data-composer-chip="official-chat"'),
    noticeVisible: notice !== null,
    notice,
    firstHitAtMs: samples.find((s) => s.hit)?.atMs ?? null,
    liveNodes: noticeEvidence?.live ?? [],
    deepNodes: noticeEvidence?.deep ?? [],
    screenshot: noticeEvidence?.shot ?? null,
    errorLines: (bodyD.match(/[^\n]*(失败|错误|拒绝)[^\n]*/g) ?? []).slice(0, 3),
  };
  const dPass = dDetail.picked && dDetail.newRequests === 0 && dDetail.draftRetained && dDetail.noticeVisible;
  verdict('断言 d：读取失败则 0 请求、草稿保留、可见错误出口', dPass ? '通过' : '失败', JSON.stringify(dDetail));

  /* -------------------------------------------------- 附：探针时间线 */
  const probeEnd = await cdp.evaluate(`
    const p = window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__;
    return p ? { events: p.events.map((e) => e.kind), counters: p.counters } : null;
  `);
  verdict('附：插件探针自报（只作时间线，不作通过判据）', '未执行', JSON.stringify(probeEnd));
} catch (error) {
  verdict('实跑', '失败', String(error?.message ?? error));
} finally {
  try { cdp?.ws.close(); } catch { /* ignore */ }
  try { browser?.kill('SIGKILL'); } catch { /* ignore */ }
  try { web.kill('SIGKILL'); } catch { /* ignore */ }
  try { stub.close(); } catch { /* ignore */ }
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  writeFileSync(RESULTS_FILE, JSON.stringify({ round: ROUND, results }, null, 2), 'utf8');
  console.log(`结果 ${RESULTS_FILE}`);
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
  console.log(JSON.stringify({ round: ROUND, results }, null, 2));
  process.exit(results.some((r) => r.status === '失败') ? 1 : 0);
}
