// **壳内 workbench 复验**：驱动真壳（tauri dev）的 webview，证明插件在
// **壳自己起的工作台窗口**里同样成立。
//
// ## 驱动的是哪个窗口
//
// Windows 上壳的 webview 是 WebView2，而 WebView2 认
// `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`，
// 于是壳的面板与**工作台**能用同一套 CDP 代码驱动（本机没有 chromedriver /
// playwright / puppeteer，仓库 wdio 那条路缺 tauri-driver）。
// 驱动的是**壳里那个工作台窗口**（真 Tauri webview，载入内核 Web UI），
// 不是内核自带的工作台页面，也不是壳的管理面板。
//
// ## 三件事
//
//   ① `@` 菜单里出现「官方对话」这一组；
//   ② 选中后 chip 落地且是**内核自己的节点**（不是 DOM 覆盖编辑器），
//      且**正文不在草稿里**（pending 句柄不冒充就绪）；
//   ③ 读取延迟时按发送 → 桩模型在读取完成前 0 请求、完成后请求体含夹具正文；
//      读取失败 → 0 请求 + 可见错误 + 草稿保留。
//
// ## 插件是怎么接进去的
//
// 由**壳自己的物化链**：面板里调 `builtin_set_enabled`（壳的命令层 →
// `ensure_wired` → 物化 + 写接线行），启动工作台时 `runtime::refresh_enabled`
// 再刷一遍。脚本末尾核对实例里的物化产物与接线行，证明这条链真的走过。
//
// ## 踩过的坑（都在下面标注了，别退回旧写法）
//
//  ① **只能 attach 主窗口**。壳里有两个同源 webview：主窗口与官方对话条
//     （URL 带 `chatstrip=1`）。壳的能力检查只允许主窗口跑本地命令——
//     连错窗口时 `builtin_set_enabled` 被拒：
//       "builtin_set_enabled not allowed on window \"official-chat\",
//        webview \"official-chat-strip\" … allowed on: [windows: \"main\"]"
//     （2026-10-20 14:28 那轮就是这么失败的：脚本按 `localhost:5174` 找面板，
//     第一个命中的是对话条。）
//  ② **过期的 kernel.pid 会让 `start_kernel` 不起内核**。pid 文件里留着已死
//     进程的 pid 时，壳认为内核还在跑，内核端口永远不通，脚本就一直等。
//     复验前先清掉它。
//  ③ **等的是「可编辑」的编辑器**，不是 composer 卡片。`[data-composer-card]`
//     页面一开就有，但内核的 input 状态没建好时，编辑器是
//     `contenteditable="false" data-phase="inert"`（内核
//     `ui-conversation/client.js@0.2.1-rc.2:19028`：
//     `phase: input?.phase ?? "inert"`）。
//  ④ **弹窗是延迟出现的**，且要用**真实鼠标事件**关——否则弹窗盖在 composer
//     上面，`@` 打不进去，断言 ① 假红。
//  ⑤ **`@` 前面必须是行首或空白**（内核触发词正则
//     `/(?:^|\s)(@([^\s]*))$/`，`input-trigger/client.js@0.2.1-rc.2:725`）。
//
// ## 用法
//
//   # 终端 A：起隔离真壳
//   node plugins/official-chat/test/drive/shell/shell-dev.mjs
//   # 终端 B：跑复验
//   node plugins/official-chat/test/drive/shell/shell-verify.mjs
import { existsSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { Cdp, evaluate, sleep, waitFor } from './cdp.mjs';
import { EVIDENCE_DIR, ROUND, STUB_PORT, startStubModel } from './stub-model.mjs';
import {
  CDP_PORT, DEV_PORT, INSTANCE_HOME, KERNEL_PID_FILE, KERNEL_PORT,
} from './paths.mjs';

/** 插件的固定夹具（与 `plugins/official-chat/client/client.js` 的 FIXTURES 一致）。 */
const FIXTURE_ALPHA = 'G1 固定对话 Alpha';
const FIXTURE_BETA = 'G1 固定对话 Beta';
const BODY_ALPHA = '【G1 固定正文 Alpha】第一行固定正文。';
const BODY_BETA = '【G1 固定正文 Beta】只有一行。';

const out = { round: ROUND, steps: [], targets: [] };
const step = (name, data) => {
  out.steps.push({ name, ...data });
  console.log(`\n=== ${name} ===\n` + JSON.stringify(data, null, 2).slice(0, 4000));
};

/** 连上 WebView2 的 CDP 端点。 */
async function connect() {
  const info = await waitFor(
    async () => {
      const res = await fetch(`http://127.0.0.1:${CDP_PORT}/json/version`);
      return res.ok ? res.json() : null;
    },
    { timeoutMs: 180000, label: 'WebView2 CDP 端点' },
  );
  return Cdp.connect(info.webSocketDebuggerUrl);
}

/** 列出所有 target（证据里不能出现凭据：内核 Web UI 的 URL 带 token，脱敏后再落盘）。 */
async function listTargets(cdp) {
  const { targetInfos } = await cdp.send('Target.getTargets');
  return targetInfos.map((t) => ({
    id: t.targetId,
    type: t.type,
    url: String(t.url ?? '').replace(/([?&]token=)[^&]+/gi, '$1REDACTED'),
    title: t.title,
  }));
}

/** attach 到既有 target（flat session）。 */
async function attach(cdp, targetId) {
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  await cdp.send('Page.enable', {}, sessionId);
  await cdp.send('Runtime.enable', {}, sessionId);
  return sessionId;
}

/** 在页面里调壳的 Tauri 命令。 */
async function invoke(cdp, sessionId, cmd, args) {
  return evaluate(
    cdp,
    sessionId,
    `(async () => {
       try {
         const api = window.__TAURI__;
         if (!api?.core?.invoke) return { ok: false, why: '页面里没有 window.__TAURI__.core.invoke' };
         const value = await api.core.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args ?? {})});
         return { ok: true, value };
       } catch (error) {
         return { ok: false, why: String(error?.message ?? error) };
       }
     })()`,
  );
}

/* ------------------------------------------------------------ 页面操作原语 */

/**
 * 关掉页面上所有模态弹窗（预览版说明等）。**用真实鼠标事件**。
 *
 * 弹窗是**延迟出现**的（插件探针亮起时它还没渲染），所以这里**先等它出现**；
 * `.click()` 在这一条上匹配不到按钮——真实鼠标事件才收得下。
 */
async function dismissDialogs(cdp, sessionId, { appearMs = 30_000, maxRounds = 4 } = {}) {
  const closed = [];
  const t0 = Date.now();
  const pick = `(() => {
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
     if (!hit) return { left: dialogs.length, hit: null };
     hit.scrollIntoView({ block: 'center' });
     const r = hit.getBoundingClientRect();
     return { left: dialogs.length, hit: { text: norm(hit) || (hit.getAttribute('aria-label') ?? ''), x: r.left + r.width / 2, y: r.top + r.height / 2 } };
   })()`;
  for (let round = 0; round < maxRounds; round += 1) {
    let found = null;
    while (Date.now() - t0 < appearMs) {
      found = await evaluate(cdp, sessionId, pick);
      if (found?.hit) break;
      if ((found?.left ?? 0) > 0) break;
      await sleep(400);
    }
    if (!found?.hit) break;
    const base = { x: found.hit.x, y: found.hit.y, button: 'left', clickCount: 1, buttons: 1 };
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...base, buttons: 0 }, sessionId);
    await sleep(80);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...base }, sessionId);
    await sleep(60);
    await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...base, buttons: 0 }, sessionId);
    closed.push(found.hit.text);
    await sleep(800);
  }
  const left = await evaluate(cdp, sessionId, `(() => document.querySelectorAll('[role="dialog"]').length)()`);
  return { ok: (left ?? 0) === 0, closed, left };
}

/** 聚焦编辑器（要可编辑且有尺寸的那个）。 */
async function focusComposer(cdp, sessionId) {
  return evaluate(
    cdp,
    sessionId,
    `(() => {
       const card = document.querySelector('[data-composer-card]');
       const root = card ?? document;
       const edit = [...root.querySelectorAll('[contenteditable="true"],[contenteditable=""]')]
         .find((el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; });
       if (!edit) return { focused: false };
       edit.focus();
       return { focused: document.activeElement === edit };
     })()`,
  );
}

/** 读草稿（文本 + HTML）。 */
async function readDraft(cdp, sessionId) {
  return evaluate(
    cdp,
    sessionId,
    `(() => {
       const card = document.querySelector('[data-composer-card]');
       const root = card ?? document;
       const edit = root.querySelector('[contenteditable="true"],[contenteditable=""]');
       return {
         draft: edit ? (edit.innerText || '') : null,
         html: edit ? edit.innerHTML.slice(0, 4000) : null,
         cardHtml: card ? card.innerHTML.slice(0, 8000) : null,
       };
     })()`,
  );
}

/** 读整页可见文本。 */
async function readBody(cdp, sessionId) {
  const text = await evaluate(cdp, sessionId, `(() => (document.body.innerText || ''))()`);
  return typeof text === 'string' ? text : '';
}

/** 读插件探针（**只作时间线，不作通过判据**）。 */
async function readProbe(cdp, sessionId) {
  return evaluate(
    cdp,
    sessionId,
    `(() => { const p = window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__; if (!p) return null; return { version: p.version, events: p.events, counters: p.counters }; })()`,
  );
}

/** 设插件运行配置（读取延迟 / 强制失败）。 */
async function setConfig(cdp, sessionId, config) {
  return evaluate(
    cdp,
    sessionId,
    `(() => { window.__DSH_G1_CONFIG__ = Object.assign({}, window.__DSH_G1_CONFIG__ ?? {}, ${JSON.stringify(config)}); return window.__DSH_G1_CONFIG__; })()`,
  );
}

/**
 * 按文案点一个元素，用**真实鼠标事件**。
 *
 * 内核的 @ 菜单是 React 渲染的，行上挂了 pointer/mouse 处理器；
 * `HTMLElement.click()` 只派发一个合成 click，不产生 pointerdown/mousedown，
 * 菜单不收下这次选择——实测表现为「菜单行被点中了，但插件的 onPick 根本没被调用」。
 */
async function realClickByText(cdp, sessionId, label, { prefix = false, selector = 'button,[role="option"],[role="menuitem"],li,div[tabindex],[data-value]' } = {}) {
  const found = await evaluate(
    cdp,
    sessionId,
    `(() => {
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
     })()`,
  );
  if (!found?.ok) return { clicked: false, labels: found?.labels ?? [] };
  const base = { x: found.x, y: found.y, button: 'left', clickCount: 1, buttons: 1 };
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...base, buttons: 0 }, sessionId);
  await sleep(80);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...base }, sessionId);
  await sleep(60);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...base, buttons: 0 }, sessionId);
  await sleep(400);
  return { clicked: true, text: found.text };
}

/**
 * 打 @ 并点中一个夹具行。
 *
 * **先等菜单里出现这一行再点**（2026-10-20 15:0x 壳内复验实测）：菜单**先渲染、
 * 候选项后到**——插件的 `candidates()` 是异步的，第一次 pick 时菜单里还没有
 * 「官方对话」那几行，点不中 → 假红（而同一轮里 D 的 pick 就正常，因为那时
 * 候选项已经在了）。所以这里轮询等这行出现，再点；等不到就把菜单行原样带进
 * 结果，便于下一次直接判读。
 */
async function pickFixture(cdp, sessionId, title) {
  await focusComposer(cdp, sessionId);
  await evaluate(cdp, sessionId, `(() => { document.execCommand('insertText', false, '@'); return true; })()`);
  let rows = [];
  let appeared = false;
  for (let i = 0; i < 20; i += 1) {
    const snapshot = await evaluate(cdp, sessionId, `(() => [...document.querySelectorAll('[role="option"],[role="menuitem"],button,li')]
      .map((el) => (el.innerText || '').trim().replace(/\\s+/g, ' '))
      .filter((t) => t && t.length > 0 && t.length < 80).slice(0, 40))()`);
    rows = Array.isArray(snapshot) ? snapshot : [];
    if (rows.some((t) => t.includes(title))) { appeared = true; break; }
    await sleep(300);
  }
  const picked = await realClickByText(cdp, sessionId, title, { prefix: true });
  return { menuRows: rows, menuHasRow: appeared, ...picked };
}

/** 清空草稿（Ctrl+A 然后 Delete，走真实键盘事件）。 */
async function clearDraft(cdp, sessionId) {
  await focusComposer(cdp, sessionId);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65, modifiers: 2,
  }, sessionId);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, nativeVirtualKeyCode: 65, modifiers: 2,
  }, sessionId);
  await sleep(150);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown', key: 'Delete', code: 'Delete', windowsVirtualKeyCode: 46, nativeVirtualKeyCode: 46,
  }, sessionId);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'Delete', code: 'Delete', windowsVirtualKeyCode: 46, nativeVirtualKeyCode: 46,
  }, sessionId);
  await sleep(400);
}

/** 按一次真实回车（`rawKeyDown` 不带 text，字符部分靠 `text: '\r'`）。 */
async function pressEnterSubmit(cdp, sessionId) {
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyDown', text: '\r', unmodifiedText: '\r',
    key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13,
  }, sessionId);
  await sleep(60);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13,
  }, sessionId);
  await sleep(200);
}

/* ------------------------------------------------- 壳的实例产物与桩模型路由 */

/** 读实例里插件物化产物与接线行（证明壳自己的链走过）。 */
function inspectShellArtifacts() {
  const base = join(INSTANCE_HOME, 'extensions', 'builtin', 'official-chat');
  const dirs = existsSync(base) ? readdirSync(base) : [];
  let fingerprint = null;
  let version = null;
  if (dirs.length > 0) {
    version = dirs[0];
    const fpDir = join(base, version);
    fingerprint = existsSync(fpDir) ? readdirSync(fpDir)[0] : null;
  }
  const materialized = fingerprint ? join(base, version, fingerprint, 'compat-a') : null;
  const patchFile = join(INSTANCE_HOME, 'profiles', 'web', 'cordis.patch.yml');
  const patch = existsSync(patchFile) ? readFileSync(patchFile, 'utf8') : null;
  return {
    instanceHome: INSTANCE_HOME,
    materializedDir: materialized,
    materializedExists: materialized ? existsSync(materialized) : false,
    entries: materialized
      ? ['host/index.js', 'client/client.js', 'package.json'].map((rel) => ({
        file: rel,
        exists: existsSync(join(materialized, rel)),
      }))
      : [],
    markerExists: materialized ? existsSync(join(materialized, '.xlink-materialized.json')) : false,
    fingerprint,
    version,
    patchFile,
    patchHasRow: patch ? patch.includes('- id: xlink-official-chat') : false,
  };
}

/**
 * 把桩模型路由追加到**壳的实例** profile patch 里（保留壳自己写的插件行）。
 *
 * 为什么不走 UI（Models 页的「添加 API Key」）：那条路写的是真实厂商凭据，
 * 请求会打到真实厂商；本脚本要的是「请求到达一个我能观测的端点」。
 */
function appendStubRoute(baseURL) {
  const patchFile = join(INSTANCE_HOME, 'profiles', 'web', 'cordis.patch.yml');
  const current = existsSync(patchFile) ? readFileSync(patchFile, 'utf8') : '';

  // 壳自己写的接线块（`- insert:` 起，到下一个顶层项为止）必须原样留着——
  // 那是**壳的物化链**的产物，本脚本只负责桩模型路由，不碰它。
  const extractInsertBlock = (text) => {
    const lines = text.split(/\r?\n/);
    const start = lines.findIndex((l) => /^- insert:/.test(l));
    if (start < 0) return '';
    const block = [lines[start]];
    for (let i = start + 1; i < lines.length; i += 1) {
      if (/^- /.test(lines[i])) break;
      block.push(lines[i]);
    }
    return block.join('\n').trimEnd();
  };
  const insertBlock = extractInsertBlock(current);

  const rows = [
    '- id: ui-settings-general',
    '  name: "@deepseek-ai/dsh-client-ui-settings-general"',
    '  config:',
    '    welcomeNoticeVersion: 2026-09-28.1',
    '- id: llm-pi-ai',
    "  name: '@deepseek-ai/dsh-llm-pi-ai'",
    '  config:',
    '    providers:',
    '      g1-stub:',
    '        displayName: G1 Stub',
    '        apiKeyEnv: G1_STUB_KEY',
    '        api: openai-completions',
    `        baseURL: ${baseURL}`,
    '        models:',
    '          - id: g1-stub-model',
    '            name: G1 Stub Model',
    '            contextWindow: 32768',
    '            maxTokens: 4096',
    '- id: agent-default-model',
    "  name: '@deepseek-ai/dsh-agent-default-model'",
    '  config:',
    '    provider: g1-stub',
    '    model: g1-stub-model',
  ].join('\n');

  const next = insertBlock ? `${rows}\n\n${insertBlock}\n` : `${rows}\n`;
  writeFileSync(patchFile, next, 'utf8');
  return {
    patchFile,
    added: true,
    bytes: next.length,
    keptShellInsertBlock: insertBlock.length > 0,
    baseURL,
  };
}

/** 清掉过期的 kernel.pid（见文件头 坑②）。 */
function clearStaleKernelPid() {
  if (!existsSync(KERNEL_PID_FILE)) return { existed: false };
  const raw = readFileSync(KERNEL_PID_FILE, 'utf8').trim();
  const pid = Number(raw.split(/\s+/)[0]);
  let alive = false;
  try { process.kill(pid, 0); alive = true; } catch { alive = false; }
  if (alive) return { existed: true, pid, alive: true, removed: false };
  rmSync(KERNEL_PID_FILE, { force: true });
  return { existed: true, pid, alive: false, removed: true };
}

/* ------------------------------------------------------------------ 主流程 */

let cdp;
let stub;
try {
  stub = await startStubModel({ port: STUB_PORT });
  step('stub-started', { baseURL: stub.baseURL, port: STUB_PORT, logFile: stub.logFile });

  cdp = await connect();
  step('cdp-connected', { endpoint: `http://127.0.0.1:${CDP_PORT}` });

  // ① 找到**主窗口**的面板（见文件头 坑①：不能选官方对话条）
  const panel = await waitFor(
    async () => {
      const list = await listTargets(cdp);
      out.targets = list;
      const local = list.filter((t) => t.type === 'page' && t.url.includes(`localhost:${DEV_PORT}`));
      return local.find((t) => !t.url.includes('chatstrip')) ?? null;
    },
    { timeoutMs: 120000, label: '壳主窗口 target' },
  );
  const panelSession = await attach(cdp, panel.id);
  step('panel-found', panel);

  // ② 用壳自己的命令层启用插件 → 走壳自己的物化链
  const enable = await invoke(cdp, panelSession, 'builtin_set_enabled', {
    plugin: 'official-chat',
    enabled: true,
  });
  step('builtin-set-enabled', enable);

  // ②.5 桩模型路由追加到壳的实例 patch（壳自己写的插件行保留）
  step('stub-route-appended', appendStubRoute(stub.baseURL));

  // ②.6 清掉过期的 kernel.pid（坑②）
  step('stale-kernel-pid-cleared', clearStaleKernelPid());

  // ③ 启动内核：**发起就不等**（内核起来要几十秒，而 CDP 求值超时是 120s），
  //    错误落到 window.__shellStartError 上再读。
  const start = await evaluate(
    cdp,
    panelSession,
    `(() => {
       try {
         const core = window.__TAURI__?.core;
         if (!core?.invoke) return { ok: false, why: '没有 window.__TAURI__.core' };
         const Channel = core.Channel;
         const onEvent = Channel ? new Channel() : undefined;
         window.__shellStartError = null;
         Promise.resolve(core.invoke('start_kernel', onEvent ? { onEvent } : {}))
           .then((value) => { window.__shellStartDone = value ?? true; })
           .catch((error) => { window.__shellStartError = String(error?.message ?? error); });
         return { ok: true, started: true };
       } catch (error) {
         return { ok: false, why: String(error?.message ?? error) };
       }
     })()`,
  );
  step('start-kernel', start);

  // ③.5 等内核端口起来（`start_kernel` 只负责起内核，端口通才算真起来了）
  const portUp = async (port) => {
    try {
      const ac = new AbortController();
      const timer = setTimeout(() => ac.abort(), 1500);
      await fetch(`http://127.0.0.1:${port}/`, { signal: ac.signal });
      clearTimeout(timer);
      return true;
    } catch {
      return false;
    }
  };
  let kernelUp = false;
  const portDeadline = Date.now() + 300000;
  while (Date.now() < portDeadline) {
    kernelUp = await portUp(KERNEL_PORT);
    if (kernelUp) break;
    await sleep(1000);
  }
  step('kernel-port-up', { port: KERNEL_PORT, up: kernelUp });
  if (!kernelUp) throw new Error('内核端口没起来（多半是 kernel.pid 没清干净，见文件头 坑②）');

  // ④ 开工作台窗口（**另一个命令**：`open_harness`）。
  //    `start_kernel` 只起内核，不开窗口——只发 `start_kernel` 然后轮询 target，
  //    永远等不到工作台窗口。
  const harnessRequest = await evaluate(
    cdp,
    panelSession,
    `(() => {
       try {
         const core = window.__TAURI__?.core;
         if (!core?.invoke) return { ok: false, why: '没有 window.__TAURI__.core' };
         window.__shellOpenError = null;
         Promise.resolve(core.invoke('open_harness'))
           .then((v) => { window.__shellOpenDone = v ?? true; })
           .catch((error) => { window.__shellOpenError = String(error?.message ?? error); });
         return { ok: true, requested: true };
       } catch (error) {
         return { ok: false, why: String(error?.message ?? error) };
       }
     })()`,
  );
  step('open-harness-requested', harnessRequest);

  // ⑤ 等壳里的工作台窗口（真 Tauri webview，载入内核 Web UI）
  let harness = null;
  let startError = null;
  const deadline = Date.now() + 300000;
  while (Date.now() < deadline) {
    const list = await listTargets(cdp);
    out.targets = list;
    harness = list.find((t) => t.type === 'page' && t.url.includes(`127.0.0.1:${KERNEL_PORT}`)) ?? null;
    if (harness) break;
    try {
      startError = await evaluate(
        cdp,
        panelSession,
        `(() => window.__shellStartError ?? window.__shellOpenError ?? null)()`,
      );
    } catch { /* ignore */ }
    if (startError) break;
    await sleep(1500);
  }
  step('harness-target', { found: harness !== null, startError });
  if (!harness) throw new Error(`壳里没出现工作台窗口（startError=${startError ?? 'null'}）`);
  const wb = await attach(cdp, harness.id);
  step('harness-found', harness);

  // ⑤.5 等**可编辑**的 composer（坑③），再关弹窗（坑④）
  await waitFor(
    async () => evaluate(cdp, wb, `(() => {
       const el = document.querySelector('[data-composer-card] [data-composer-input]');
       if (!el) return false;
       const r = el.getBoundingClientRect();
       return el.getAttribute('contenteditable') === 'true' && r.width > 0 && r.height > 0;
     })()`),
    { timeoutMs: 180000, label: '工作台可编辑 composer 出现' },
  );
  const dialogs = await dismissDialogs(cdp, wb);
  await sleep(1500);
  step('composer-ready', { dialogs });

  // ⑥ ①：@ 菜单里那一组
  await clearDraft(cdp, wb);
  await focusComposer(cdp, wb);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '@'); return true; })()`);
  await sleep(2500);
  const menuBody = await readBody(cdp, wb);
  step('A-menu-in-shell', {
    hasGroup: menuBody.includes('官方对话') && menuBody.includes(FIXTURE_ALPHA),
    snippet: menuBody.split('\n').filter((l) => l.includes('官方对话') || l.includes(FIXTURE_ALPHA)).slice(0, 5),
  });
  await clearDraft(cdp, wb);

  // ⑦ ②：chip 落地且是内核节点（正文不在草稿里）
  await setConfig(cdp, wb, { readDelayMs: 8000, failRead: false });
  const pick = await pickFixture(cdp, wb, FIXTURE_ALPHA);
  const draftNow = await readDraft(cdp, wb);
  step('B-chip-in-shell', {
    picked: pick.clicked === true,
    menuHasRow: pick.menuHasRow === true,
    menuRows: pick.menuRows ?? [],
    chipIsKernelNode: (draftNow.html ?? '').includes('data-composer-chip="official-chat"'),
    bodyInDraft: (draftNow.html ?? '').includes(BODY_ALPHA),
    draftHtml: (draftNow.html ?? '').slice(0, 220),
  });

  // ⑧ ③：发送门控 —— 等待
  await clearDraft(cdp, wb);
  await setConfig(cdp, wb, { readDelayMs: 8000, failRead: false });
  const pickD = await pickFixture(cdp, wb, FIXTURE_ALPHA);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '请看这个'); return true; })()`);
  const beforeD = stub.requests().length;
  const t0 = Date.now();
  await pressEnterSubmit(cdp, wb);
  const timeline = [];
  let arrived = null;
  for (let i = 0; i < 40; i += 1) {
    const probe = await readProbe(cdp, wb);
    const kinds = (probe?.events ?? []).map((e) => e.kind);
    const reqs = stub.requests().slice(beforeD);
    timeline.push({
      t: Date.now() - t0,
      reqs: reqs.length,
      hasBody: reqs.some((r) => JSON.stringify(r.body ?? {}).includes(BODY_ALPHA)),
      kinds: kinds.slice(-3),
    });
    if (reqs.length > 0) { arrived = Date.now() - t0; break; }
    await sleep(500);
  }
  const reqsD = stub.requests().slice(beforeD);
  const draftAfter = await readDraft(cdp, wb);
  const bodyAfter = await readBody(cdp, wb);
  step('D-send-gate-in-shell', {
    picked: pickD.clicked === true,
    waitedMs: arrived,
    newRequests: reqsD.length,
    requestHasBody: reqsD.some((r) => JSON.stringify(r.body ?? {}).includes(BODY_ALPHA)),
    serializeOk: (await readProbe(cdp, wb))?.events?.some((e) => e.kind === 'serialize-ok') ?? false,
    draftCleared: (draftAfter.draft ?? '').trim() === '',
    sentText: bodyAfter.includes('请看这个'),
    timeline: timeline.filter((x, i) => i === 0 || i === timeline.length - 1 || x.reqs > 0),
  });

  // ⑨ ③：发送门控 —— 拒绝（错误提示是**瞬态 Toast**，必须密集采样）
  await clearDraft(cdp, wb);
  await setConfig(cdp, wb, { readDelayMs: 2500, failRead: true });
  const pickE = await pickFixture(cdp, wb, FIXTURE_BETA);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '请看这个'); return true; })()`);
  const beforeE = stub.requests().length;
  await pressEnterSubmit(cdp, wb);
  let errorHit = null;
  for (let i = 0; i < 24; i += 1) {
    const body = await readBody(cdp, wb);
    const m = body.match(/[^\n]*(固定读取失败|正文读取失败|发送时会拒绝)[^\n]*/g);
    if (m) { errorHit = { atMs: i * 250, text: m[0] }; break; }
    await sleep(250);
  }
  const draftE = await readDraft(cdp, wb);
  const probeE = await readProbe(cdp, wb);
  step('E-reject-in-shell', {
    picked: pickE.clicked === true,
    newRequests: stub.requests().length - beforeE,
    errorVisible: errorHit !== null,
    errorHit,
    draftRetained: (draftE.draft ?? '').includes('请看这个'),
    serializeFailed: (probeE?.events ?? []).some((e) => e.kind === 'serialize-failed'),
  });

  // ⑩ 壳自己的物化链产物
  step('shell-artifacts', inspectShellArtifacts());
} catch (error) {
  step('failed', { error: String(error?.message ?? error), stack: String(error?.stack ?? '') });
} finally {
  try { cdp?.close(); } catch { /* ignore */ }
  try { stub?.stop(); } catch { /* ignore */ }
  const file = join(EVIDENCE_DIR, `shell-verify.${ROUND}.json`);
  writeFileSync(file, JSON.stringify(out, null, 2), 'utf8');
  console.log(`\n结果 ${file}`);
}
