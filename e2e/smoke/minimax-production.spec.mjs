// Production-chain smoke. The launcher supplies a fresh home/profile and explicit
// read-only package/Node paths; never log the upstream HTML or its token.
import { browser, expect, $ } from '@wdio/globals';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { isMcodeInstanceId } from '../launcher.mjs';
import { isRealChild as inside } from '../real-child.mjs';

const root = process.env.DSH_E2E_RUN_ROOT;
const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
const packagePath = process.env.DSH_E2E_MCODE_PACKAGE;
const nodePath = process.env.DSH_E2E_MCODE_NODE;
const binary = process.env.DSH_E2E_APP_BINARY;
const expectedHome = process.env.DSH_E2E_EXPECTED_HOME;
if (!root || !evidenceDir || !packagePath || !nodePath || !binary || !expectedHome) {
  throw new Error('MiniMax smoke 必须经隔离 launcher 并指定包和 Node 路径');
}

if (!inside(root, evidenceDir) || !isAbsolute(packagePath) || !isAbsolute(nodePath)) {
  throw new Error('MiniMax smoke 隔离参数无效');
}

function invoke(command, args = {}) {
  return browser.executeAsync(
    'var done = arguments[arguments.length - 1];' +
    'window.__TAURI__.core.invoke(' + JSON.stringify(command) + ',' + JSON.stringify(args) + ')' +
    '.then(value => done({ok:true,value}), () => done({ok:false}));'
  );
}

function start() {
  return browser.executeAsync(
    'var done = arguments[arguments.length - 1];' +
    'const stages = [];' +
    'const channel = new window.__TAURI__.core.Channel();' +
    'channel.onmessage = message => {' +
    'const stage = message === "正在检测 MiniMax 安装和 Node.js…" ? "detect" :' +
    'message === "正在准备 MiniMax 实例并启动服务…" ? "prepare" :' +
    'message === "正在等待 MiniMax 页面就绪…" ? "readiness" : "other";' +
    'if (!stages.includes(stage)) stages.push(stage); };' +
    'window.__TAURI__.core.invoke("start_mcode", {onProgress: channel})' +
    '.then(() => done({ok:true,stages}), error => {' +
    'const text = String(error);' +
    'const category = /原生依赖|SQLite 或终端依赖/.test(text) ? "native_dependencies" :' +
    '/提前退出/.test(text) ? "service_exited" :' +
    '/就绪超时/.test(text) ? "readiness_timeout" :' +
    '/打开 MiniMax 工作台失败|创建 MiniMax 工作台线程失败|打开 MiniMax 工作台中断/.test(text) ? "window_open" :' +
    '/MiniMax 工作台已停止|服务状态无法确认|端口归属无法确认/.test(text) ? "service_state" :' +
    '/MiniMax 设置损坏|旧 MiniMax 进程|实例记录不存在/.test(text) ? "status_or_config" :' +
    '/端口/.test(text) ? "port" :' +
    '/Node\.js|Node.js/.test(text) ? "node" :' +
    '/启动已取消|壳正在退出/.test(text) ? "cancelled" : "other";' +
    'done({ok:false,category,stages}); });'
  );
}

function processes() {
  if (process.platform === 'win32') {
    const result = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      'Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,ExecutablePath,CreationDate | Select-Object ProcessId,ParentProcessId,ExecutablePath,CreationDate | ConvertTo-Json -Compress'],
    { encoding: 'utf8', windowsHide: true, timeout: 15000 });
    return [].concat(JSON.parse(result)).map(row => ({ pid: row.ProcessId, parent: row.ParentProcessId, path: row.ExecutablePath, created: row.CreationDate }));
  }
  if (process.platform === 'darwin') {
    const output = execFileSync('ps', ['-ax', '-o', 'pid=,ppid=,comm='], { encoding: 'utf8', timeout: 15000 });
    return output.split('\n').map(line => line.match(/^\s*(\d+)\s+(\d+)\s+(.+)$/)).filter(Boolean)
      .map(match => ({ pid: Number(match[1]), parent: Number(match[2]), path: match[3], created: null }));
  }
  throw new Error('MiniMax smoke 仅支持 Windows 和 Intel macOS');
}

function listeners(port) {
  if (process.platform === 'win32') {
    const script = `Get-NetTCPConnection -State Listen -LocalPort ${port} -ErrorAction SilentlyContinue | Select-Object LocalAddress,OwningProcess | ConvertTo-Json -Compress`;
    const output = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', windowsHide: true, timeout: 15000 }).trim();
    return output ? [].concat(JSON.parse(output)).map(row => ({ address: row.LocalAddress, pid: row.OwningProcess })) : [];
  }
  try {
    const output = execFileSync('lsof', ['-nP', '-iTCP:' + port, '-sTCP:LISTEN', '-Fpn'], { encoding: 'utf8', timeout: 15000 });
    let pid = null;
    return output.split('\n').flatMap(line => {
      if (line.startsWith('p')) pid = Number(line.slice(1));
      if (line.startsWith('n')) return [{ address: line.slice(1), pid }];
      return [];
    });
  } catch (error) {
    if (error.status === 1 && !error.stdout && !error.stderr) return [];
    throw new Error('无法核对 MiniMax 监听端口');
  }
}

function free(port) {
  return new Promise(resolveFree => {
    const server = createServer();
    server.once('error', () => resolveFree(false));
    server.once('listening', () => server.close(() => resolveFree(true)));
    server.listen(port, '127.0.0.1');
  });
}

function identity(row) { return { pid: row.pid, created: row.created }; }
function descendants(rows, rootPid) {
  const ids = new Set([rootPid]);
  let size;
  do {
    size = ids.size;
    for (const row of rows) if (ids.has(row.parent)) ids.add(row.pid);
  } while (ids.size !== size);
  return rows.filter(row => ids.has(row.pid)).map(identity);
}

describe('MiniMax 隔离生产链 smoke', function () {
  this.timeout(240_000);
  const evidence = { schemaVersion: 1, checks: {}, observedProcessTree: [], processTreeCoverage: 'observed_only', cleanup: 'not_run' };
  let pid;
  let pidFile;
  let ports = [];
  let attempted = false;

  before(async () => {
    await browser.setTimeout({ script: 150_000 });
    await browser.waitUntil(async () => browser.execute(() => !!window.__TAURI__?.core), { timeout: 60_000 });
  });

  after(async () => {
    try {
      if (attempted) {
        // Snapshot any published PID before stop, including failures during startup.
        let snapshotFailed = false;
        try {
          if (pidFile && existsSync(pidFile)) {
            if (!inside(root, pidFile)) throw new Error('pid outside isolated root');
            const candidate = Number(readFileSync(pidFile, 'utf8').trim().split(/\s+/)[0]);
            if (!Number.isInteger(candidate) || candidate <= 0) throw new Error('invalid pid');
            pid = candidate;
            evidence.observedProcessTree = descendants(processes(), pid);
          }
        } catch { snapshotFailed = true; }
        const result = await invoke('stop_mcode');
        if (!result.ok) throw new Error('MiniMax 停止命令失败');
        const status = await invoke('get_mcode_status');
        if (!status.ok || status.value.running || status.value.window_open) throw new Error('停止后状态未收敛');
        for (const port of ports) if (!(await free(port))) throw new Error('停止后端口未释放');
        const now = processes();
        for (const old of evidence.observedProcessTree) {
          const current = now.find(row => row.pid === old.pid);
          if (current && (!old.created || !current.created || current.created === old.created)) throw new Error('已观察到的测试进程仍在运行或无法确认');
        }
        if (snapshotFailed) throw new Error('无法核对运行进程');
      }
      evidence.cleanup = attempted ? (pid ? 'pass' : 'partial') : 'not_run';
    } catch {
      evidence.cleanup = 'fail';
      throw new Error('MiniMax smoke 收尾未能确认，请保留本轮隔离目录并检查进程');
    } finally {
      writeFileSync(join(evidenceDir, 'minimax-production-smoke.json'), JSON.stringify(evidence, null, 2) + '\n');
    }
  });

  it('检测、启动、检查归属和独立窗口', async () => {
    await browser.execute(() => { window.requestAnimationFrame = fn => setTimeout(fn, 0); });
    const nav = await $('.sidebar__nav');
    await nav.waitForExist({ timeout: 60_000 });
    const entry = await $('//button[@role="tab" and normalize-space(.)="MiniMax"]');
    await entry.click();
    await browser.waitUntil(async () => browser.execute(() => {
      const tab = [...document.querySelectorAll('[role="tab"]')].find(item => item.textContent.trim() === 'MiniMax');
      return !!tab && tab.getAttribute('aria-selected') === 'true' && document.body.innerText.includes('MiniMax Code');
    }), { timeout: 30_000 });
    evidence.checks.managementPage = true;

    const saved = await invoke('save_mcode_installation', { packagePath, nodePath });
    expect(saved.ok).toBe(true);
    const detected = await invoke('detect_mcode_installation');
    expect(detected.ok).toBe(true);
    expect(detected.value.installed).toBe(true);
    expect(detected.value.package_version).toBe('0.1.7');
    expect(detected.value.running).toBe(false);
    if (!inside(root, detected.value.data_dir)) throw new Error('MiniMax 数据目录不在隔离根内');
    if (!isMcodeInstanceId(detected.value.instance_id)) throw new Error('MiniMax 实例标识无效');
    const expectedInstance = join(expectedHome, 'kernels', 'mcode', 'instances', detected.value.instance_id);
    expect(realpathSync(detected.value.data_dir)).toBe(realpathSync(join(expectedInstance, 'home')));
    ports = [detected.value.port, detected.value.miniapp_port];
    if (ports.some(port => !Number.isInteger(port) || port < 1 || port > 65535)) throw new Error('MiniMax 端口无效');
    pidFile = join(dirname(detected.value.data_dir), 'runtime', 'pid');
    evidence.checks.detected = true;

    const before = await browser.getWindowHandles();
    attempted = true;
    const startup = await start();
    evidence.startup = startup;
    expect(startup.ok).toBe(true);
    const status = await invoke('get_mcode_status');
    expect(status.ok).toBe(true);
    expect(status.value.running).toBe(true);
    expect(status.value.window_open).toBe(true);
    const handles = await browser.getWindowHandles();
    expect(handles.length).toBeGreaterThan(before.length);
    expect(handles).toContain('mcode-workbench');
    const dataDir = status.value.data_dir;
    expect(inside(root, dataDir)).toBe(true);
    const instanceDir = dirname(dataDir);
    expect(inside(root, join(instanceDir, 'browser'))).toBe(true);
    expect(realpathSync(join(instanceDir, 'browser'))).toBe(realpathSync(join(expectedInstance, 'browser')));
    if (!inside(root, pidFile)) throw new Error('MiniMax 进程记录不在隔离根内');
    const parts = readFileSync(pidFile, 'utf8').trim().split(/\s+/);
    pid = Number(parts[0]);
    expect(status.value.port).toBe(ports[0]);
    expect(status.value.miniapp_port).toBe(ports[1]);
    if (!Number.isInteger(pid) || pid <= 0 || ports.some(port => !Number.isInteger(port) || port < 1 || port > 65535)) throw new Error('MiniMax 进程记录无效');
    expect(Number(parts[1])).toBe(ports[0]);
    const rows = processes();
    const service = rows.find(row => row.pid === pid);
    if (!service) throw new Error('无法找到 MiniMax 进程');
    let ancestor = service;
    let belongs = false;
    const visited = new Set();
    while (ancestor && !visited.has(ancestor.pid)) {
      visited.add(ancestor.pid);
      if (ancestor.path && existsSync(ancestor.path) && realpathSync(ancestor.path) === realpathSync(binary)) { belongs = true; break; }
      ancestor = rows.find(row => row.pid === ancestor.parent);
    }
    expect(belongs).toBe(true);
    evidence.observedProcessTree = descendants(rows, pid);
    let listenerPid;
    for (const port of ports) {
      const bound = listeners(port);
      expect(bound.length).toBe(1);
      if (listenerPid === undefined) listenerPid = bound[0].pid;
      else expect(bound[0].pid).toBe(listenerPid);
      expect(process.platform === 'win32' ? bound[0].address === '127.0.0.1' : bound[0].address === `127.0.0.1:${port}`).toBe(true);
    }
    expect(evidence.observedProcessTree.some(process => process.pid === listenerPid)).toBe(true);
    if (process.platform === 'win32') expect(listenerPid).not.toBe(pid);
    // Production status re-queries both listeners and checks their PID is in
    // the dedicated Windows Job; parentage by itself is not proof of ownership.
    const ownership = await invoke('get_mcode_status');
    expect(ownership.ok).toBe(true);
    expect(ownership.value.running).toBe(true);
    expect(ownership.value.instance_id).toBe(status.value.instance_id);
    expect(ownership.value.port).toBe(ports[0]);
    expect(ownership.value.miniapp_port).toBe(ports[1]);
    const response = await fetch(`http://127.0.0.1:${ports[0]}/`, { redirect: 'error', signal: AbortSignal.timeout(4000) });
    expect(response.status).toBe(200);
    const html = await response.text();
    const match = html.match(/window\.__WEBUI_CONFIG__=([^<]+);<\/script>/);
    if (!match) throw new Error('MiniMax 首页缺少运行配置');
    let config;
    try { config = JSON.parse(match[1]); } catch { throw new Error('MiniMax 首页运行配置无效'); }
    let configDataDir;
    try { configDataDir = realpathSync(config.dataDir); } catch { throw new Error('MiniMax 首页数据目录无效'); }
    expect(configDataDir).toBe(realpathSync(dataDir));
    await browser.execute(() => [...document.querySelectorAll('[role="tab"]')].find(item => item.textContent.trim() === 'DSH').click());
    await browser.waitUntil(async () => browser.execute(() => {
      const tab = [...document.querySelectorAll('[role="tab"]')].find(item => item.textContent.trim() === 'DSH');
      return !!tab && tab.getAttribute('aria-selected') === 'true';
    }), { timeout: 30_000 });
    const afterSwitch = await invoke('get_mcode_status');
    expect(afterSwitch.ok).toBe(true);
    expect(afterSwitch.value.running).toBe(true);
    expect(afterSwitch.value.instance_id).toBe(status.value.instance_id);
    expect(Number(readFileSync(pidFile, 'utf8').trim().split(/\s+/)[0])).toBe(pid);
    expect(await free(ports[0])).toBe(false);
    expect(await free(ports[1])).toBe(false);
    evidence.checks.switchKeepsService = true;
    evidence.checks.serviceOwner = true;
    evidence.checks.loopbackPorts = true;
    evidence.checks.isolatedDataAndProfile = true;
    evidence.checks.window = true;
    evidence.pid = pid;
    evidence.listenerPid = listenerPid;
    evidence.ports = ports;
    evidence.nodeVersion = status.value.nodeVersion;
    evidence.packageVersion = status.value.package_version;
  });
});
