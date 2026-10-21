// 壳内复验的**公共部分**：报告状态、CDP 助手、起壳前置流程。
//
// ## 为什么抽出来
//
// G1 轮（`shell-verify.mjs`）与 P1 轮（`shell-verify-p1.mjs`）驱的是**同一个**
// 隔离壳、同一套 CDP 助手、同一段前置流程。抄一份就是「两份真相」——而且
// `check:duplicates` 会直接报红。所以公共部分只留在这里，两个轮次各自 import。
//
// ## 边界
//
// 这里**只有「怎么驱动」**（连 CDP、找窗口、点菜单、按键、读探针）与
// 「怎么把壳起起来」；**「判定什么」在各自的轮次脚本里**——判据不许混进驱动层。


import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { Cdp, evaluate, sleep, waitFor } from './cdp.mjs';
import { EVIDENCE_DIR, ROUND, STUB_PORT, startStubModel } from './stub-model.mjs';
import {
  CDP_PORT, DEV_PORT, INSTANCE_HOME, KERNEL_PID_FILE, KERNEL_PORT, SHELL_HOME,
} from './paths.mjs';


/** 插件的固定夹具（与 `plugins/official-chat/client/client.js` 的 FIXTURES 一致）。 */
export const FIXTURE_ALPHA = 'G1 固定对话 Alpha';
export const FIXTURE_BETA = 'G1 固定对话 Beta';
export const BODY_ALPHA = '【G1 固定正文 Alpha】第一行固定正文。';
export const BODY_BETA = '【G1 固定正文 Beta】只有一行。';


export const out = { round: ROUND, steps: [], results: [] };
export const step = (name, data) => {
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
export function verdict(name, status, detail) {
  out.results.push({ name, status, detail });
  console.log(`[${status}] ${name} — ${detail}`);
}


/** 连上 WebView2 的 CDP 端点。 */
export async function connect() {
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
export async function listTargets(cdp) {
  const { targetInfos } = await cdp.send('Target.getTargets');
  return targetInfos.map((t) => ({
    id: t.targetId,
    type: t.type,
    url: String(t.url ?? '').replace(/([?&]token=)[^&]+/gi, '$1REDACTED'),
    title: t.title,
  }));
}

/** attach 到既有 target（flat session）。 */
export async function attach(cdp, targetId) {
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  await cdp.send('Page.enable', {}, sessionId);
  await cdp.send('Runtime.enable', {}, sessionId);
  return sessionId;
}

/** 在页面里调壳的 Tauri 命令。 */
export async function invoke(cdp, sessionId, cmd, args) {
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
export async function dismissDialogs(cdp, sessionId, { appearMs = 30_000, maxRounds = 4 } = {}) {
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
export async function focusComposer(cdp, sessionId) {
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
export async function readDraft(cdp, sessionId) {
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
export async function readBody(cdp, sessionId) {
  const text = await evaluate(cdp, sessionId, `(() => (document.body.innerText || ''))()`);
  return typeof text === 'string' ? text : '';
}

/** 读插件探针（**只作时间线，不作通过判据**）。 */
export async function readProbe(cdp, sessionId) {
  return evaluate(
    cdp,
    sessionId,
    `(() => { const p = window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__; if (!p) return null; return { version: p.version, events: p.events, counters: p.counters }; })()`,
  );
}

/** 设插件运行配置（读取延迟 / 强制失败）。 */
export async function setConfig(cdp, sessionId, config) {
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
export async function realClickByText(cdp, sessionId, label, { prefix = false, selector = 'button,[role="option"],[role="menuitem"],li,div[tabindex],[data-value]' } = {}) {
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
 * 「Web Chat」那几行，点不中 → 假红（而同一轮里 D 的 pick 就正常，因为那时
 * 候选项已经在了）。所以这里轮询等这行出现，再点；等不到就把菜单行原样带进
 * 结果，便于下一次直接判读。
 */
export async function pickFixture(cdp, sessionId, title) {
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
export async function clearDraft(cdp, sessionId) {
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
export async function pressEnterSubmit(cdp, sessionId) {
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
export function inspectShellArtifacts() {
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
export function appendStubRoute(baseURL) {
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
export function pickPanelTargets(targets) {
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
export function processCommandLine(pid) {
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
export function stopInstanceKernel() {
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
    // **这个 pid 不是本隔离 home 的进程** ⇒ 本实例的内核**早退了**，pid 号被
    // 别的进程复用（Windows 上很常见——比如被 WebView2 的子进程顶上了）。
    // ⇒ **pid 文件本身已过期**（坑②的另一种形态）。
    //
    // **为什么这里不判「停不掉内核=失败」**：本实例根本没有内核在跑；那个还在
    // 活着的是**别人的进程**，而我们本来就不该动它（安全闸，见文件头）。
    // 真正要处置的是**那份过期的 pid 文件**——留着它，壳会以为内核还活着而不真起
    // 内核（端口不通，驱动脚本会一直等）。「过期 pid 先清」那条机检认的就是这个
    // 场景（见 `clearStaleKernelPid`）。
    //
    // **踩过的坑**（2026-10-20 p1-r7）：这里原本判「失败」，于是一个**过期 pid**
    // 把整轮汇总染成 1 失败 + 1 未执行，真机结论反而被噪声淹没。
    return {
      hadPidFile: true,
      pid,
      // **进程确实还在**（但不是我们的）——`wasAlive` 说的是「动手前在不在」，不能撒谎。
      wasAlive: true,
      stillAlive: true,
      stopped: false,
      stalePid: true,
      why: '该 pid 不是本隔离 home 的进程（内核早退、pid 被别的进程复用）⇒ pid 文件已过期',
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
export function clearStaleKernelPid() {
  if (!existsSync(KERNEL_PID_FILE)) return { existed: false };
  const raw = readFileSync(KERNEL_PID_FILE, 'utf8').trim();
  const pid = Number(raw.split(/\s+/)[0]);
  if (!Number.isFinite(pid) || pid <= 0) {
    return { existed: true, pid: raw, alive: false, removed: false, why: 'pid 文件内容不是 pid' };
  }
  const cmdline = processCommandLine(pid);
  // **本实例的内核还在跑** ⇒ 不能清（清了壳会以为没内核在跑）。
  if (cmdline !== null && cmdline.includes(SHELL_HOME)) {
    return { existed: true, pid, alive: true, removed: false, cmdline, why: '本实例的内核还在跑，不该清' };
  }
  // **两种「过期」形态都要清**：
  // ① 进程已经没了（`processCommandLine` 拿不到）；
  // ② 进程还在，但**不是本隔离 home 的**——内核早退了，pid 号被别的进程复用。
  //    留着文件，壳会以为内核还活着而不真起内核（坑②）。
  rmSync(KERNEL_PID_FILE, { force: true });
  return {
    existed: true,
    pid,
    alive: cmdline !== null,
    removed: true,
    cmdline: cmdline ?? undefined,
    why: cmdline === null
      ? '进程已不在，pid 文件是过期的'
      : '该 pid 不是本隔离 home 的进程（内核早退、pid 被复用）⇒ pid 文件已过期',
  };
}

/**
 * 清掉本实例里 **official-chat 的物化产物**（插件代码）。
 *
 * ## 为什么每轮真机前都要清
 *
 * 壳的物化按「**插件版本 + 内核指纹**」去重：代码改了、版本号没变 ⇒ 壳判定
 * 「这一版已经物化过了」⇒ **不会重新拷贝**。于是真机复验会**跑旧插件**——
 * 拿旧代码的失败当新代码的结论。
 *
 * **实测踩过**（2026-10-20 p1-r7）：仓库里 client 半边已改成 v0.4 的
 * exact fetch route，实例里物化的却还是 18:53 的旧版（仍走
 * `connection.rpc.call`）⇒ 浏览器侧打 `/xlink-official-chat/capabilities`
 * ⇒ **405**。那个 405 是**旧插件**的 405，不是新通道的 405。
 *
 * @returns {{root: string, existed: boolean, removed: boolean}} 清的结果。
 */
export function clearMaterializedPlugin() {
  const root = join(INSTANCE_HOME, 'extensions', 'builtin', 'official-chat');
  if (!existsSync(root)) return { root, existed: false, removed: false };
  rmSync(root, { recursive: true, force: true });
  return { root, existed: true, removed: !existsSync(root) };
}


/**
 * 起好隔离壳的前置流程：桩模型 → CDP → 主面板 attach → 停本实例内核 →
 * 开内置插件 → 追加桩路由 → 起内核 → 开工作台窗口 → 等可编辑 composer。
 *
 * **两条硬前置条件照做**（PM 2026-10-20 派活）：
 * ① attach 必须是 **main 面板**（壳的能力检查只允许 main 窗口跑本地命令）；
 * ② **改内置插件开关前先停该实例内核**（内核把开关缓存在内存，运行期改动会被
 *    它下一次落盘整个覆盖）。
 *
 * @returns {{cdp: object, stub: object, panelSession: string, wb: string}}
 */
export async function prepareShell() {
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
    // **过期 pid 文件（stalePid）不算失败**：本实例**没有内核可停**（内核早退、
    // pid 号被别的进程复用），真正要处置的是那份过期 pid 文件——由前置③清。
    // 只有「内核确实在跑、却停不掉」才是失败。
    const stopStatus = stopped.hadPidFile === false ? '未执行'
      : (stopped.wasAlive === false ? '未执行'
        : (stopped.stalePid === true ? '未执行'
          : (stopped.stopped === true ? '通过' : '失败')));
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

    // ②.7 **清掉本插件的物化产物**：壳的物化按「插件版本 + 内核指纹」去重——
    //     **代码改了、版本号没变 ⇒ 壳判定「已物化」⇒ 不会重新拷贝** ⇒ 真机
    //     会**跑旧插件**，拿旧代码的结论当新代码的结论
    //     （2026-10-20 p1-r7 实踩：跑的是 18:53 的旧 client 半边，
    //      那个 405 是**旧插件**的 405，不是新通道的 405）。
    //     每轮先清 ⇒ **跑的一定是当前仓库代码**。
    const materialized = clearMaterializedPlugin();
    step('materialized-plugin-cleared', materialized);
    verdict(
      '前置②.7：清掉旧物化产物（保证真机跑的是当前仓库代码）',
      materialized.existed === false || materialized.removed === true ? '通过' : '失败',
      JSON.stringify(materialized),
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

    return { cdp, stub, panelSession, wb };
  } catch (error) {
    throw error;
  }
}
