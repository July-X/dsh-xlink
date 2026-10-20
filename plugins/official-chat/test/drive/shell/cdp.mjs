// 最小 CDP 客户端（零依赖：Node 22+ 自带全局 WebSocket）。
//
// ## 为什么要自己写
//
// 这条链路上没有 chromedriver / playwright / puppeteer，而 Chromium 内核的浏览器
// 可以用 `--remote-debugging-port` 直接开 CDP 口子。Windows 上壳的 webview 是
// WebView2，它同样认这个环境变量（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`），
// 于是**壳自己的面板与工作台窗口**都能用同一套 CDP 代码驱动。
//
// ## 隔离
//
// `launchEdge` 的浏览器档案是**一次性临时目录**（`mkdtemp`），不指向任何用户
// 真实数据目录下的 profile——浏览器一启动就写 Cookies/Preferences/History，
// 写坏了回滚不了。
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** 本机可驱动的浏览器（按优先级探测；不写死某一个人的安装路径）。 */
const BROWSER_CANDIDATES = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
];

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** 等某个条件成立（轮询）。 */
export async function waitFor(fn, { timeoutMs = 30000, intervalMs = 250, label = 'condition' } = {}) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    try {
      last = await fn();
      if (last) return last;
    } catch (error) {
      last = String(error?.message ?? error);
    }
    await sleep(intervalMs);
  }
  throw new Error(`waitFor 超时（${label}）：${String(last ?? '')}`);
}

/** 启动浏览器（独立临时 profile，headless）。 */
export function launchBrowser({ port, profileDir, headless = true } = {}) {
  const exe = BROWSER_CANDIDATES.find((p) => existsSync(p));
  if (exe === undefined) throw new Error('本机没有可驱动的 Chromium 内核浏览器');
  const dir = profileDir ?? mkdtempSync(join(tmpdir(), 'g1-browser-'));
  const args = [
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${dir}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-networking',
    '--disable-features=Translate,MediaRouter',
    'about:blank',
  ];
  if (headless) args.unshift('--headless=new');
  const proc = spawn(exe, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  proc.stdout?.on('data', () => {});
  proc.stderr?.on('data', () => {});
  return {
    proc,
    profileDir: dir,
    cleanup: () => {
      try { proc.kill(); } catch { /* ignore */ }
      try { rmSync(dir, { recursive: true, force: true }); } catch { /* ignore */ }
    },
  };
}

/** 一个 CDP 连接（浏览器级，支持 flat session）。 */
export class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Set();
    ws.addEventListener('message', (event) => {
      const msg = JSON.parse(typeof event.data === 'string' ? event.data : String(event.data));
      if (msg.id !== undefined && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) reject(new Error(`${msg.error.message} (${msg.error.code})`));
        else resolve(msg.result);
        return;
      }
      for (const fn of this.listeners) fn(msg);
    });
  }

  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true });
      ws.addEventListener('error', () => reject(new Error(`CDP 连接失败：${url}`)), { once: true });
    });
    return new Cdp(ws);
  }

  send(method, params = {}, sessionId) {
    const id = this.nextId++;
    const payload = { id, method, params };
    if (sessionId) payload.sessionId = sessionId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify(payload));
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`CDP 超时：${method}`));
        }
      }, 120000);
    });
  }

  on(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  close() {
    try { this.ws.close(); } catch { /* ignore */ }
  }
}

/** 打开一个页面 target 并 attach（flat session）。 */
export async function openPage(cdp, url) {
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  await cdp.send('Page.enable', {}, sessionId);
  await cdp.send('Runtime.enable', {}, sessionId);
  return { targetId, sessionId };
}

/** 在页面里求值（await Promise，按值返回）。 */
export async function evaluate(cdp, sessionId, expression) {
  const res = await cdp.send(
    'Runtime.evaluate',
    { expression, awaitPromise: true, returnByValue: true, userGesture: true },
    sessionId,
  );
  if (res.exceptionDetails) {
    const text = res.exceptionDetails.exception?.description ?? res.exceptionDetails.text;
    throw new Error(`页面求值抛错：${text}`);
  }
  return res.result?.value;
}

/** 导航并等待 load 事件。 */
export async function navigate(cdp, sessionId, url) {
  const loaded = new Promise((resolve) => {
    const off = cdp.on((msg) => {
      if (msg.method === 'Page.loadEventFired' && msg.sessionId === sessionId) {
        off();
        resolve();
      }
    });
    setTimeout(() => { off(); resolve(); }, 60000);
  });
  await cdp.send('Page.navigate', { url }, sessionId);
  await loaded;
}

/**
 * 像人一样打字（逐字符 keyDown/keyUp，触发真实的 beforeinput/input）。
 *
 * 逐字符（而不是 `Input.insertText` 一次灌）是因为内核的 @ 菜单与 Lexical 编辑器
 * 对输入事件的时序敏感：一次性灌进去时菜单可能来不及开。
 */
export async function typeText(cdp, sessionId, text) {
  for (const ch of text) {
    await cdp.send('Input.dispatchKeyEvent', {
      type: 'keyDown', text: ch, unmodifiedText: ch, key: ch,
    }, sessionId);
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch }, sessionId);
    await sleep(40);
  }
}

/** 按一个键（Enter 等）。 */
export async function pressKey(cdp, sessionId, key, code, windowsVirtualKeyCode) {
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown', key, code, windowsVirtualKeyCode, nativeVirtualKeyCode: windowsVirtualKeyCode,
  }, sessionId);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp', key, code, windowsVirtualKeyCode, nativeVirtualKeyCode: windowsVirtualKeyCode,
  }, sessionId);
}

/** 回车（提交）。 */
export const pressEnter = (cdp, sessionId) => pressKey(cdp, sessionId, 'Enter', 'Enter', 13);
