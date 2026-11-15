// 无头 Edge + CDP：1040×748 CSS 像素、2x 缩放，真实 Vue + 公开示例桥。
// 用法：node scripts/website-preview/capture.mjs "theme=dark" website/assets/overview-dark.webp
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const dist = path.join(here, 'dist');
const [, , query = '', outPath = 'shot.png'] = process.argv;

const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2' };
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://127.0.0.1');
    const rel = decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname);
    const file = path.join(dist, rel);
    if (!file.startsWith(dist + path.sep)) throw new Error('outside');
    const body = await readFile(file);
    res.writeHead(200, { 'content-type': MIME[path.extname(file)] || 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404); res.end('not found');
  }
});
await new Promise((resolve, reject) => { server.once('error', reject); server.listen(38123, '127.0.0.1', resolve); });

const PORT = Number(process.env.PREVIEW_CDP_PORT || 39333);
const profile = mkdtempSync(path.join(os.tmpdir(), 'dsh-website-preview-'));
const EDGE = process.env.BROWSER_EXECUTABLE || 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe';
const proc = spawn(EDGE, [
  '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--no-proxy-server',
  '--remote-debugging-port=' + PORT,
  '--user-data-dir=' + profile,
  'about:blank',
], { stdio: 'inherit', windowsHide: true });

let spawnError;
proc.on('error', (error) => { spawnError = error; });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function targets() {
  for (let i = 0; i < 80; i++) {
    if (spawnError) throw spawnError;
    try {
      const res = await fetch('http://127.0.0.1:' + PORT + '/json/list', { signal: AbortSignal.timeout(1000) });
      const list = await res.json();
      if (list && list.length) return list;
    } catch { /* 端口还没起来 */ }
    await sleep(250);
  }
  throw new Error('CDP 端口没起来');
}

let ws;
try {
  const list = await targets();
  const page = list.find((t) => t.type === 'page' && t.webSocketDebuggerUrl) || list[0];
  ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = (e) => reject(new Error('WebSocket 打不开: ' + (e && e.message))); });

  let id = 0;
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const myId = ++id;
    const onMessage = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id !== myId) return;
      ws.removeEventListener('message', onMessage);
      if (msg.error) reject(new Error(method + ': ' + JSON.stringify(msg.error)));
      else resolve(msg.result);
    };
    ws.addEventListener('message', onMessage);
    ws.send(JSON.stringify({ id: myId, method, params }));
  });

  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setUserAgentOverride', {
    userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15',
    platform: 'MacIntel',
  });
  await send('Emulation.setDeviceMetricsOverride', { width: 1040, height: 748, deviceScaleFactor: 2, mobile: false });
  await send('Page.navigate', { url: 'http://127.0.0.1:38123/?' + query });

  const evalJs = async (expression) => {
    const out = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    return out && out.result ? out.result.value : undefined;
  };

  let ready = false;
  for (let i = 0; i < 80; i++) {
    ready = await evalJs("!!document.querySelector('.overview-grid') && !!document.querySelector('.nav-item')");
    if (ready) break;
    await sleep(250);
  }
  const panel=new URLSearchParams(query).get('panel');
  if(panel) {await evalJs(`Array.from(document.querySelectorAll('.nav-item')).find(e=>e.textContent.trim()===${JSON.stringify(panel)}).click()`);}
  await sleep(1800);
  if (!ready) throw new Error('界面尚未就绪，未生成截图');
  const unknown=await evalJs('window.__HARNESS_UNKNOWN__');
  if (unknown.length) throw new Error('尚未定义样例响应：'+unknown.join(','));
  const diag = await evalJs("JSON.stringify({ ready: " + ready + ", unknown: window.__HARNESS_UNKNOWN__, title: document.title, dark: document.documentElement.classList.contains('dark'), shell: !!document.querySelector('.app-shell'), panel: document.querySelector('.main-view main')?.innerText?.slice(0,80) })");
  console.log(diag);

  // 按扩展名选格式：网站资源要 webp（体积），核对用 png（无损）。
  const webp = outPath.toLowerCase().endsWith('.webp');
  const shot = await send('Page.captureScreenshot', webp ? { format: 'webp', quality: 88 } : { format: 'png' });
  writeFileSync(outPath, Buffer.from(shot.data, 'base64'));
  console.log('wrote ' + outPath + ' (' + Buffer.from(shot.data, 'base64').length + ' bytes)');
} finally {
  try { if (ws) ws.close(); } catch { /* ignore */ }
  const closed = new Promise((resolve) => { if (proc.exitCode !== null || spawnError) resolve(); else proc.once('close', resolve); });
  proc.kill();
  await Promise.race([closed, sleep(3000)]);
  server.close();
  // 仅清理由本脚本创建的临时浏览器档案，绝不使用个人档案。
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 4, retryDelay: 250 }); }
  catch { console.warn('临时浏览器档案尚被占用，请在浏览器退出后清理：' + profile); }
}
