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
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { Cdp, evaluate, sleep, waitFor } from './cdp.mjs';
import { EVIDENCE_DIR, ROUND, STUB_PORT, startStubModel } from './stub-model.mjs';
import {
  CDP_PORT, DEV_PORT, INSTANCE_HOME, KERNEL_PID_FILE, KERNEL_PORT, SHELL_HOME,
} from './paths.mjs';

/** 插件的固定夹具（与 `plugins/official-chat/client/client.js` 的 FIXTURES 一致）。 */
const FIXTURE_ALPHA = 'G1 固定对话 Alpha';
const FIXTURE_BETA = 'G1 固定对话 Beta';
const BODY_ALPHA = '【G1 固定正文 Alpha】第一行固定正文。';
const BODY_BETA = '【G1 固定正文 Beta】只有一行。';

const out = { round: ROUND, steps: [], results: [] };
const step = (name, data) => {
  out.steps.push({ name, ...data });
  console.log(`\n=== ${name} ===\n` + JSON.stringify(data, null, 2).slice(0, 4000));
};

/**
 * 记一条**判定**：通过 / 失败 / 未执行。
 *
 * 形状与内核路径的 `results.<轮次>.json` 对齐（`{name, status, detail}`）——
 * 之前这里只把原始数据推进 `steps`，「全绿」只能靠人读 JSON 判；那与内核那套
 * **不是同一种绿**，引用时要能一眼看出差别。
 */
function verdict(name, status, detail) {
  out.results.push({ name, status, detail });
  console.log(`[${status}] ${name} — ${detail}`);
}

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
  // 实例目录是壳**按需**建的（插件接线时才有）：第一次跑时这个文件可能根本不存在。
  mkdirSync(join(INSTANCE_HOME, 'profiles', 'web'), { recursive: true });

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

/* ------------------------------------------- 机检：复跑前置条件（不许靠人记） */

/**
 * 壳自己的**面板** target 候选。
 *
 * **不能写死 vite 端口**：dev 壳的面板是 `http://localhost:5174/`，而 release 壳的
 * 面板走**内嵌资源**（`tauri://localhost` / `http://tauri.localhost/`）——写死端口
 * 会让 release 壳一个候选都找不到（面板被当成「不存在」）。
 *
 * 排除三类：壳打开的**工作台**（内核端口）、官方对话**条带**（`chatstrip=1`）、
 * 外部站点（官网对话窗口的内容页签）。
 */
function pickPanelTargets(targets) {
  return targets.filter((t) => {
    if (t.type !== 'page') return false;
    const url = t.url ?? '';
    if (!url) return false;
    if (url.includes(`127.0.0.1:${KERNEL_PORT}`)) return false; // 工作台
    if (url.includes('chatstrip')) return false; // 官方对话条带
    if (url.startsWith('devtools://')) return false;
    // 内嵌资源面板（release 壳）
    if (url.startsWith('tauri://') || url.includes('tauri.localhost')) return true;
    // dev 壳：vite dev server
    if (url.includes(`localhost:${DEV_PORT}`)) return true;
    return false;
  });
}

/** 读某个 pid 的完整命令行（进程不在了返回 null）。 */
function processCommandLine(pid) {
  try {
    const out = execFileSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `(Get-CimInstance Win32_Process -Filter "ProcessId=${pid}" -ErrorAction SilentlyContinue).CommandLine`,
      ],
      { encoding: 'utf8', timeout: 20000, windowsHide: true },
    );
    const text = String(out ?? '').trim();
    return text.length > 0 ? text : null;
  } catch {
    return null;
  }
}

/**
 * 停掉**本实例**的内核（改内置插件开关前必须做）。
 *
 * 壳拒改开关的原话是「内核把状态缓存在内存里，运行期间的改动会被它下一次落盘
 * 整个覆盖」——所以「改开关前先停内核」是硬前置条件，不能靠人记。
 *
 * **安全闸**：只停**命令行指向本隔离 home** 的进程。别人的内核（比如用户正在跑
 * 的 release 壳那颗）一律不碰——本脚本没有任何理由动它。
 */
function stopInstanceKernel() {
  if (!existsSync(KERNEL_PID_FILE)) {
    return { hadPidFile: false, wasAlive: false, stillAlive: false, stopped: false, why: '没有 pid 文件（本实例没起过内核）' };
  }
  const raw = readFileSync(KERNEL_PID_FILE, 'utf8').trim();
  const pid = Number(raw.split(/\s+/)[0]);
  if (!Number.isFinite(pid) || pid <= 0) {
    return { hadPidFile: true, pid: raw, wasAlive: false, stillAlive: false, stopped: false, why: 'pid 文件内容不是 pid' };
  }
  const cmdline = processCommandLine(pid);
  if (cmdline === null) {
    // 进程已经没了 → 这是**过期 pid**：壳会因为它以为内核还活着而不起内核（坑②）。
    // 文件留给 `clearStaleKernelPid` 去清（那条机检该认的就是这个场景）。
    return {
      hadPidFile: true, pid, wasAlive: false, stillAlive: false, stopped: false,
      why: 'pid 已过期（进程不在）',
    };
  }
  if (!cmdline.includes(SHELL_HOME)) {
    // **拒绝动手**：这个 pid 不是本隔离 home 的进程。
    return {
      hadPidFile: true,
      pid,
      wasAlive: true,
      stillAlive: true,
      stopped: false,
      why: '该 pid 的命令行不指向本隔离 home，拒绝停它',
      cmdline,
    };
  }
  try {
    execFileSync('taskkill.exe', ['/PID', String(pid), '/T', '/F'], {
      encoding: 'utf8',
      timeout: 30000,
      windowsHide: true,
    });
  } catch (error) {
    return {
      hadPidFile: true, pid, wasAlive: true, stillAlive: true, stopped: false,
      why: String(error?.message ?? error), cmdline,
    };
  }
  const stillAlive = processCommandLine(pid) !== null;
  // **不删 pid 文件**：那是壳自己的文件，删它的职责属于下面的
  // `clearStaleKernelPid`（「过期 pid 先清」那条机检）—— 进程刚死、文件还在，
  // 正是那条机检该认出来的场景。
  //
  // 字段名要分清：`wasAlive` = **动手前**进程在不在（决定这条机检算不算被执行过），
  // `stillAlive` = **动手后**还在不在（决定停成功没有）。早先两处都叫 `alive`，
  // 映射按「动手前」判、返回值给的是「动手后」→ 真的停掉一颗反而记成「未执行」。
  return {
    hadPidFile: true,
    pid,
    wasAlive: true,
    stillAlive,
    alive: stillAlive,
    stopped: !stillAlive,
    cmdline,
    pidFileLeftForCleanup: !stillAlive,
  };
}

/**
 * 清掉过期的 kernel.pid（见文件头 坑②）。
 *
 * 判「活没活」用**同一个**命令行探针（`processCommandLine`），不用
 * `process.kill(pid, 0)`：两处判据不一致时，「停内核」与「清过期 pid」会对
 * 同一个 pid 给出相反的结论，而这种分歧在 Windows 上不容易一眼看出来。
 */
function clearStaleKernelPid() {
  if (!existsSync(KERNEL_PID_FILE)) return { existed: false };
  const raw = readFileSync(KERNEL_PID_FILE, 'utf8').trim();
  const pid = Number(raw.split(/\s+/)[0]);
  if (!Number.isFinite(pid) || pid <= 0) {
    return { existed: true, pid: raw, alive: false, removed: false, why: 'pid 文件内容不是 pid' };
  }
  const cmdline = processCommandLine(pid);
  if (cmdline !== null) return { existed: true, pid, alive: true, removed: false, cmdline };
  rmSync(KERNEL_PID_FILE, { force: true });
  return { existed: true, pid, alive: false, removed: true, why: '进程已不在，pid 文件是过期的' };
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
  //    候选不写死 vite 端口：release 壳的面板走内嵌资源（tauri://localhost）。
  const panel = await waitFor(
    async () => {
      const list = await listTargets(cdp);
      out.targets = list;
      const candidates = pickPanelTargets(list);
      return candidates[0] ?? null;
    },
    { timeoutMs: 120000, label: '壳主窗口 target' },
  );
  const panelSession = await attach(cdp, panel.id);
  step('panel-found', panel);

  // ①.5 **机检前置条件①：attach 必须是 main 面板。**
  //     壳的能力检查只允许 main 窗口跑本地命令（`capabilities/default.json` 的
  //     `"windows": ["main"]`）；连错窗口时 `builtin_set_enabled` 被拒：
  //       "builtin_set_enabled not allowed on window \"official-chat\",
  //        webview \"official-chat-strip\" … allowed on: [windows: \"main\"]"
  //     这里用只读的 `builtin_status` 先探一次，探不过就**快速失败**并把原话带上，
  //     而不是让后面某一步报一个看不出来的错。
  const mainProbe = await invoke(cdp, panelSession, 'builtin_status', {});
  step('assert-main-panel', {
    url: panel.url,
    ok: mainProbe?.ok === true,
    why: mainProbe?.why ?? null,
  });
  verdict(
    '前置①：attach 到的必须是 main 面板（只读探针 builtin_status）',
    mainProbe?.ok === true ? '通过' : '失败',
    JSON.stringify({ url: panel.url, why: mainProbe?.why ?? null }),
  );
  if (mainProbe?.ok !== true) {
    throw new Error(
      `attach 到的不是 main 面板（url=${panel.url}）：${mainProbe?.why ?? 'unknown'}`,
    );
  }

  // ② **机检前置条件②：改内置插件开关前先停掉本实例的内核。**
  //     壳拒改的原话是「内核把状态缓存在内存里，运行期间的改动会被它下一次落盘
  //     整个覆盖」。只停命令行指向**本隔离 home** 的进程，别人的内核不碰。
  const stopped = stopInstanceKernel();
  step('instance-kernel-stopped', stopped);
  // 「没内核在跑」与「真的停掉了一颗」是**两件事**：前者是空分支，标「未执行」，
  // 让「这条机检到底被压过没有」在结果里一眼可见。
  // 三种情形分开判：pid 文件都没有 / 进程早已不在（过期 pid，交给前置③清）/
  // 真的停掉了一颗 / 活着却停不掉（唯一算失败的那种）。
  // 判据用 `wasAlive`（**动手前**在不在），不是 `alive`（动手后还在不在）。
  const stopStatus = stopped.hadPidFile === false ? '未执行'
    : (stopped.wasAlive === false ? '未执行'
      : (stopped.stopped === true ? '通过' : '失败'));
  verdict(
    '前置②：改内置插件开关前停掉本实例的内核',
    stopStatus,
    JSON.stringify({
      hadPidFile: stopped.hadPidFile,
      pid: stopped.pid ?? null,
      wasAlive: stopped.wasAlive ?? null,
      stillAlive: stopped.stillAlive ?? null,
      stopped: stopped.stopped === true,
      why: stopped.why ?? null,
      cmdline: stopped.cmdline ?? null,
    }),
  );
  if (stopped.alive === true) {
    throw new Error(
      `停不掉本实例的内核（pid=${stopped.pid ?? '未知'}）：${stopped.why ?? 'unknown'}`,
    );
  }

  // ②.5 桩模型路由追加到壳的实例 patch（壳自己写的插件行保留）
  //     —— **必须在 `builtin_set_enabled` 之后**：接线行是壳自己写的，
  //     追加要把它原样留着；而实例目录（`profiles/web/`）也是壳按需建的，
  //     插件没接线之前那个文件可能根本不存在。

  // ②.6 清掉过期的 kernel.pid（坑②）
  const stale = clearStaleKernelPid();
  step('stale-kernel-pid-cleared', stale);
  verdict(
    '前置③：清掉过期的 kernel.pid',
    stale.existed === false ? '未执行' : (stale.removed === true ? '通过' : '未执行'),
    JSON.stringify(stale),
  );

  // ③ 用壳自己的命令层启用插件 → 走壳自己的物化链
  const enable = await invoke(cdp, panelSession, 'builtin_set_enabled', {
    plugin: 'official-chat',
    enabled: true,
  });
  step('builtin-set-enabled', enable);

  // ③.5 **返回值必须断言**：壳说「接上了」才算接上（`wired:true`）。
  //     只看 `ok:true` 是不够的——上一次壳内复跑就是 `ok:false` 仍然往下跑，
  //     插件最后是靠「壳自己的接线行 + 物化链」装上的，那不是本次要的路径。
  const enableValue = enable?.value ?? {};
  if (enable?.ok !== true) {
    verdict('接线：builtin_set_enabled', '失败', JSON.stringify(enable));
    throw new Error(`builtin_set_enabled 失败：${enable?.why ?? 'unknown'}`);
  }
  if (enableValue.wired !== true) {
    verdict('接线：builtin_set_enabled（wired:true）', '失败', JSON.stringify(enableValue));
    throw new Error(
      `builtin_set_enabled 返回 wired=${JSON.stringify(enableValue.wired)}：${JSON.stringify(enableValue)}`,
    );
  }
  verdict(
    '接线：builtin_set_enabled（wired:true）',
    '通过',
    JSON.stringify({
      id: enableValue.id ?? null,
      wired: enableValue.wired,
      loadState: enableValue.loadState ?? null,
      missingPeers: enableValue.missingPeers ?? null,
      kernelFingerprint: enableValue.kernelFingerprint ?? null,
    }),
  );

  // ③.6 桩模型路由追加（壳刚写的接线行必须原样保留）
  step('stub-route-appended', appendStubRoute(stub.baseURL));

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
  const aDetail = {
    hasGroup: menuBody.includes('官方对话') && menuBody.includes(FIXTURE_ALPHA),
    snippet: menuBody.split('\n').filter((l) => l.includes('官方对话') || l.includes(FIXTURE_ALPHA)).slice(0, 5),
  };
  step('A-menu-in-shell', aDetail);
  verdict('断言 A：@ 菜单里出现「官方对话」分组', aDetail.hasGroup ? '通过' : '失败', JSON.stringify(aDetail));
  await clearDraft(cdp, wb);

  // ⑦ ②：chip 落地且是内核节点（正文不在草稿里）
  await setConfig(cdp, wb, { readDelayMs: 8000, failRead: false });
  const pick = await pickFixture(cdp, wb, FIXTURE_ALPHA);
  const draftNow = await readDraft(cdp, wb);
  const bDetail = {
    picked: pick.clicked === true,
    menuHasRow: pick.menuHasRow === true,
    menuRows: pick.menuRows ?? [],
    chipIsKernelNode: (draftNow.html ?? '').includes('data-composer-chip="official-chat"'),
    bodyInDraft: (draftNow.html ?? '').includes(BODY_ALPHA),
    draftHtml: (draftNow.html ?? '').slice(0, 220),
  };
  step('B-chip-in-shell', bDetail);
  verdict(
    '断言 B：chip 落地且是内核节点（正文不在草稿里）',
    bDetail.picked && bDetail.chipIsKernelNode && bDetail.bodyInDraft === false ? '通过' : '失败',
    JSON.stringify(bDetail),
  );

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
  // **口径**：正文「只出现一次」指的是**这一次发送**在**对话请求**里出现一次。
  // 会话是跨轮复用的时，历史里本来就躺着上一轮那条 → 同一个请求里会出现两次。
  // 所以这里把「本次那条」与「历史那条」分开数，别让人拿总数当重复外发。
  const chatRequest = reqsD.find((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')) ?? null;
  const chatMessages = chatRequest?.body?.messages ?? [];
  const alphaIdx = chatMessages
    .map((m, i) => ({ i, m, c: typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? '') }))
    .filter((x) => x.c.includes(BODY_ALPHA))
    .map((x) => x.i);
  const dDetail = {
    picked: pickD.clicked === true,
    waitedMs: arrived,
    newRequests: reqsD.length,
    requestHasBody: reqsD.some((r) => JSON.stringify(r.body ?? {}).includes(BODY_ALPHA)),
    serializeOk: (await readProbe(cdp, wb))?.events?.some((e) => e.kind === 'serialize-ok') ?? false,
    draftCleared: (draftAfter.draft ?? '').trim() === '',
    sentText: bodyAfter.includes('请看这个'),
    // ↓ 口径字段：对话请求里正文出现在哪几条消息、历史里有没有上一轮的 assistant 回包
    chatRequestCount: reqsD.filter((r) => JSON.stringify(r.body ?? {}).includes('You are an AI agent')).length,
    bodyOccurrenceIndexes: alphaIdx,
    bodyOccurrencesInChatRequest: alphaIdx.length,
    thisTurnIndex: alphaIdx.length > 0 ? alphaIdx[alphaIdx.length - 1] : null,
    historyAssistantReplies: chatMessages.filter((m) => m.role === 'assistant').length,
    otherRequestPurposes: reqsD
      .filter((r) => !JSON.stringify(r.body ?? {}).includes('You are an AI agent'))
      .map((r) => (JSON.stringify(r.body ?? {}).includes('Create a concise title') ? 'session-title' : 'other')),
    timeline: timeline.filter((x, i) => i === 0 || i === timeline.length - 1 || x.reqs > 0),
  };
  step('D-send-gate-in-shell', dDetail);
  verdict(
    '断言 D：读取门控（读取完成前 0 请求，完成后请求体含夹具正文）',
    dDetail.picked && dDetail.waitedMs !== null && dDetail.requestHasBody
      && dDetail.serializeOk && dDetail.draftCleared ? '通过' : '失败',
    JSON.stringify(dDetail),
  );

  // ⑨ ③：发送门控 —— 拒绝（错误提示是**瞬态 Toast**，必须密集采样）
  //
  //    **节点级断言**（与内核 r14/r16 同一口径）：光有文本命中不算数——文本级
  //    命中证明不了「有一个可访问的、用户看得见的错误出口」。所以这里同时扫
  //    `role="alert"|role="status"|aria-live` 的节点、含错误文案的深层节点
  //    （带 tag/class 路径），并在命中时**截图**留证。
  //    内核把 `error` 级 notice 渲染成 Toast（`ui-conversation/client.js@0.2.1-rc.2:18808`
  //    `if (notice?.level === "error") showToast(notice.text)`，带 `onDone` → 自动消失），
  //    `info` 级才是内联 `role="status"`（`:18982-18986`）→ 必须密集采样。
  await clearDraft(cdp, wb);
  await setConfig(cdp, wb, { readDelayMs: 2500, failRead: true });
  const pickE = await pickFixture(cdp, wb, FIXTURE_BETA);
  await evaluate(cdp, wb, `(() => { document.execCommand('insertText', false, '请看这个'); return true; })()`);
  const beforeE = stub.requests().length;
  await pressEnterSubmit(cdp, wb);
  let errorHit = null;
  let noticeEvidence = null;
  const eSamples = [];
  for (let i = 0; i < 24; i += 1) {
    const scan = await evaluate(
      cdp,
      wb,
      `(() => {
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
         return { innerHas: inner.includes('读取失败'), contentHas: content.includes('读取失败'), live, deep: deep.slice(0, 6) };
       })()`,
    );
    const hit = scan?.innerHas === true || scan?.contentHas === true || (scan?.deep?.length ?? 0) > 0;
    eSamples.push({ atMs: i * 250, hit, liveCount: scan?.live?.length ?? 0, deepCount: scan?.deep?.length ?? 0 });
    if (hit && !errorHit) {
      errorHit = { atMs: i * 250, text: scan?.deep?.[0]?.text ?? scan?.live?.[0]?.text ?? '读取失败' };
      noticeEvidence = scan;
    }
    // 截图留证：命中那一张必拍，其余每 8 拍一张。
    if ((hit && noticeEvidence?.shot === undefined) || i % 8 === 0) {
      try {
        const shot = await cdp.send('Page.captureScreenshot', { format: 'png' }, wb);
        const file = join(EVIDENCE_DIR, `assert-e.toast.${String(i * 250).padStart(5, '0')}ms.png`);
        writeFileSync(file, Buffer.from(shot.data, 'base64'));
        if (noticeEvidence && noticeEvidence.shot === undefined) noticeEvidence.shot = file;
      } catch { /* 截图失败不影响判据 */ }
    }
    if (errorHit) break;
    await sleep(250);
  }
  const draftE = await readDraft(cdp, wb);
  const probeE = await readProbe(cdp, wb);
  const liveNodes = noticeEvidence?.live ?? [];
  const deepNodes = noticeEvidence?.deep ?? [];
  const nodeLevelVisible = liveNodes.some((x) => /读取失败|发送时会拒绝/.test(x.text ?? ''))
    || deepNodes.some((x) => /读取失败|发送时会拒绝/.test(x.text ?? ''));
  const eDetail = {
    picked: pickE.clicked === true,
    newRequests: stub.requests().length - beforeE,
    errorVisible: errorHit !== null,
    errorHit,
    nodeLevelVisible,
    liveNodes,
    deepNodes,
    screenshot: noticeEvidence?.shot ?? null,
    draftRetained: (draftE.draft ?? '').includes('请看这个'),
    serializeFailed: (probeE?.events ?? []).some((e) => e.kind === 'serialize-failed'),
    samples: eSamples.filter((s, i) => s.hit || i === 0 || i === eSamples.length - 1),
  };
  step('E-reject-in-shell', eDetail);
  verdict(
    '断言 E：读取失败则 0 请求、草稿保留、可见错误出口（节点级）',
    eDetail.picked && eDetail.newRequests === 0 && eDetail.errorVisible && eDetail.nodeLevelVisible
      && eDetail.draftRetained && eDetail.serializeFailed ? '通过' : '失败',
    JSON.stringify(eDetail),
  );

  // ⑩ 壳自己的物化链产物
  const artifacts = inspectShellArtifacts();
  step('shell-artifacts', artifacts);
  verdict(
    '物化链产物核对（物化目录 / 三份文件 / 标记 / 接线行）',
    artifacts.materializedExists && artifacts.entries.every((e) => e.exists)
      && artifacts.markerExists && artifacts.patchHasRow ? '通过' : '失败',
    JSON.stringify(artifacts),
  );
} catch (error) {
  step('failed', { error: String(error?.message ?? error), stack: String(error?.stack ?? '') });
  verdict('实跑', '失败', String(error?.message ?? error));
} finally {
  try { cdp?.close(); } catch { /* ignore */ }
  try { stub?.stop(); } catch { /* ignore */ }
  // 汇总：通过 / 失败 / 未执行各多少条（**未执行要单列**——那是「这条机检这条
  // 路径没被压过」的唯一信号，混进「通过」里就看不见了）。
  const summary = { 通过: 0, 失败: 0, 未执行: 0 };
  for (const r of out.results) summary[r.status] = (summary[r.status] ?? 0) + 1;
  out.summary = summary;
  const file = join(EVIDENCE_DIR, `shell-verify.${ROUND}.json`);
  writeFileSync(file, JSON.stringify(out, null, 2), 'utf8');
  console.log(`\n汇总 ${JSON.stringify(summary)}`);
  console.log(`结果 ${file}`);
}
