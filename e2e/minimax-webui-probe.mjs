// P0. Run with the Node executable used to install the candidate npm package.
import assert from 'node:assert/strict';
import { fork, spawn, spawnSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { copyFileSync, createWriteStream, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync } from 'node:fs';
import { finished } from 'node:stream/promises';
import { request } from 'node:http';
import { createConnection } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import {
  buildRuntimeEnv, createFileLogger, inside, isolationProblems, makeRunId,
  markerFor, mergeTauriConfig, pickFreePort, writeEvidenceResult,
} from './launcher.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const win = process.platform === 'win32';
const ip = '127.0.0.1';
const version = '0.1.7';

function processExit(child) {
  return new Promise((resolveExit, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolveExit({ code, signal }));
  });
}

function platformCommand(command, args, allowEmpty = false) {
  const run = spawnSync(command, args, { encoding: 'utf8', windowsHide: true, timeout: 15000 });
  if (allowEmpty && run.status === 1 && !run.stderr && !run.stdout) return '';
  assert.equal(run.status, 0, `platform check failed (${command})`);
  return run.stdout.trim();
}

function listeners(pid) {
  if (win) {
    return platformCommand('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      `Get-NetTCPConnection -State Listen -OwningProcess ${pid} -ErrorAction SilentlyContinue | ForEach-Object { '{0}:{1}' -f $_.LocalAddress, $_.LocalPort }; exit 0`,
    ]).split(/\r?\n/).filter(Boolean);
  }
  return platformCommand('lsof', ['-nP', '-a', '-p', String(pid), '-iTCP', '-sTCP:LISTEN', '-Fn'], true)
    .split('\n').filter((line) => line.startsWith('n')).map((line) => line.slice(1));
}

function processTree(pid) {
  const rows = win
    ? platformCommand('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      "Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId | ForEach-Object { '{0} {1}' -f $_.ProcessId, $_.ParentProcessId }"])
    : platformCommand('ps', ['-ax', '-o', 'pid=,ppid=']);
  const entries = rows.split(/\r?\n/).map((line) => line.trim().split(/\s+/).map(Number));
  const pending = [pid];
  const seen = new Set(pending);
  for (const parent of pending) {
    for (const [child, owner] of entries) {
      if (owner === parent && !seen.has(child)) {
        pending.push(child);
        seen.add(child);
      }
    }
  }
  return pending;
}

function wsStatus(url, origin) {
  return new Promise((resolveStatus, reject) => {
    const req = request(url.href.replace(/^ws:/, 'http:'), { headers: {
      Origin: origin, Connection: 'Upgrade', Upgrade: 'websocket',
      'Sec-WebSocket-Version': '13', 'Sec-WebSocket-Key': randomBytes(16).toString('base64'),
    } });
    req.on('upgrade', (res, socket) => { socket.destroy(); resolveStatus(res.statusCode); });
    req.on('response', (res) => { res.resume(); resolveStatus(res.statusCode); });
    req.on('error', () => reject(new Error('WebSocket handshake request failed')));
    req.setTimeout(4000, () => req.destroy());
    req.end();
  });
}

function startService(entry, env, cwd) {
  // Windows does not deliver Unix signals to Node children. Exercise the installed
  // package's own SIGTERM shutdown handler through the probe's private IPC channel.
  const shutdown = 'process.once("message", (value) => { if (value === "stop") process.emit("SIGTERM"); });';
  const child = fork(entry, [], {
    cwd, env, detached: !win, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe', 'ipc'],
    execArgv: ['--import', 'data:text/javascript,' + encodeURIComponent(shutdown)],
  });
  const exited = processExit(child);
  // Consume both streams without writing package output, which includes launch credentials.
  child.stdout.resume();
  child.stderr.resume();
  return { child, exited };
}

function portOpen(port) {
  return new Promise((resolveOpen) => {
    const socket = createConnection({ host: ip, port });
    socket.once('connect', () => { socket.destroy(); resolveOpen(true); });
    socket.once('error', () => resolveOpen(false));
    socket.setTimeout(1000, () => { socket.destroy(); resolveOpen(false); });
  });
}

async function verifyService(service, { port, miniPort, dataDir }) {
  const started = performance.now();
  while (!(await portOpen(port) && await portOpen(miniPort))) {
    const running = await Promise.race([service.exited.then(() => false), delay(350, true)]);
    assert.ok(running, 'MiniMax WebUI exited before both listeners opened');
    assert.ok(performance.now() - started < 90000, 'MiniMax WebUI timed out before both listeners opened');
  }
  const bound = listeners(service.child.pid);
  assert.deepEqual(bound.sort(), [`${ip}:${port}`, `${ip}:${miniPort}`].sort(), 'listener ownership or binding mismatch');
  const origin = `http://${ip}:${port}`;
  const page = await fetch(origin + '/', { redirect: 'error', signal: AbortSignal.timeout(4000) });
  assert.equal(page.status, 200, 'WebUI root did not respond successfully');
  const html = await page.text();
  const match = html.match(/window\.__WEBUI_CONFIG__=([^<]+);<\/script>/);
  assert.ok(match, 'WebUI bootstrap config missing');
  let config;
  try {
    config = JSON.parse(match[1]);
  } catch {
    throw new Error('WebUI bootstrap config is not valid JSON');
  }
  assert.ok(realpathSync(config.dataDir) === dataDir, 'MiniMax wrote outside the isolated directory');
  const ws = new URL(config.websocketUrl);
  assert.ok(ws.origin === `ws://${ip}:${port}` && typeof config.token === 'string' && config.token, 'invalid WebUI bootstrap config');
  ws.searchParams.set('token', config.token);
  const sameOrigin = await wsStatus(ws, origin);
  const foreignOrigin = await wsStatus(ws, 'https://example.invalid');
  assert.equal(sameOrigin, 101);
  assert.equal(foreignOrigin, 403);
  return { pid: service.child.pid, bound, page: page.status, sameOrigin, foreignOrigin,
    readyMs: Math.round(performance.now() - started) };
}

async function stopService({ child, exited }) {
  if (!child.pid) return;
  let tree;
  let treeError;
  try {
    tree = processTree(child.pid);
  } catch (error) {
    treeError = error;
  }
  if (win && child.connected) child.send('stop', () => {});
  if (!win && child.exitCode === null) child.kill('SIGTERM');
  const result = await Promise.race([exited, delay(10000, null, { ref: false })]);
  if (!result) {
    if (win) platformCommand('taskkill.exe', ['/PID', String(child.pid), '/T', '/F']);
    else process.kill(-child.pid, 'SIGKILL');
    await exited;
  }
  if (treeError) throw treeError;
  for (const pid of tree) {
    assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' }, 'probe process tree is still running');
  }
  return { pids: tree, graceful: result?.code === 0, forced: !result };
}

async function runWindow(binary, env, stage, interactive, record) {
  const child = spawn(binary, interactive ? [] : ['--smoke'], {
    env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.resume();
  let rendered = false;
  const lines = createInterface({ input: child.stdout });
  lines.on('line', (line) => {
    if (!line.startsWith('MCODE_PROBE ')) return;
    const entry = JSON.parse(line.slice('MCODE_PROBE '.length));
    rendered ||= entry.scene?.rendered && entry.scene?.connected;
    record(stage, entry);
  });
  const result = await processExit(child);
  assert.equal(result.code, 0, `Tauri probe exited during ${stage}`);
  assert.ok(rendered, `Tauri WebUI did not render and connect during ${stage}`);
}

async function main() {
  const [packagePath, mode, ...extra] = process.argv.slice(2);
  assert.ok(packagePath && isAbsolute(packagePath) && !extra.length && (!mode || mode === '--interactive'),
    'Usage: node e2e/minimax-webui-probe.mjs <absolute package directory> [--interactive]');
  assert.ok((win || process.platform === 'darwin') && process.arch === 'x64', 'P0 requires Windows x64 or Intel macOS');
  const interactive = mode === '--interactive';
  assert.ok(existsSync(join(root, 'src-tauri', 'resources', 'builtin-plugins')),
    'Missing bundled resources. Run npm run prep:builtin first.');
  const pkg = realpathSync(packagePath);
  const packageFile = join(pkg, 'package.json');
  const metadata = JSON.parse(readFileSync(packageFile, 'utf8'));
  assert.equal(metadata.name, '@minimax-webui/minimax-code');
  assert.equal(metadata.version, version);
  assert.equal(metadata.bin?.['mcode-webui'], 'bin/mcode-webui.mjs');
  const entry = realpathSync(join(pkg, metadata.bin['mcode-webui']));
  assert.ok(inside(pkg, entry), 'MiniMax entry escapes its package directory');

  const runId = makeRunId();
  const base = join(realpathSync(tmpdir()), 'dsh-xlink-minimax-probe');
  const runRoot = join(base, 'runs', runId);
  const evidenceDir = join(base, 'evidence', runId);
  const paths = { runId, runRoot, evidenceDir, homeDir: join(runRoot, 'home'), appDataDir: join(runRoot, 'appdata'),
    decoyHome: join(runRoot, 'decoy'), tempDir: join(runRoot, 'tmp'), roamingDir: join(runRoot, 'roaming'), localDir: join(runRoot, 'local') };
  const dataDir = join(runRoot, 'minimax');
  const profileDir = join(runRoot, 'browser');
  const project = join(runRoot, 'P0 中文 项目');
  const approvalDir = join(runRoot, 'P0 审批目标');
  for (const dir of [evidenceDir, paths.homeDir, paths.appDataDir, paths.decoyHome, paths.tempDir,
    paths.roamingDir, paths.localDir, dataDir, profileDir, project, approvalDir]) mkdirSync(dir, { recursive: true });
  const marker = markerFor(runId, process.pid, root);
  writeFileSync(join(runRoot, '.mcode-probe'), JSON.stringify(marker), { flag: 'wx' });
  assert.deepEqual(isolationProblems(runRoot, marker, runId, { dataDir, profileDir, project, approvalDir, homeDir: paths.homeDir,
    appDataDir: paths.appDataDir, decoyHome: paths.decoyHome }), []);
  const log = createFileLogger(join(evidenceDir, 'probe.log'));
  const result = { runId, platform: process.platform, arch: process.arch, node: process.version, package: version,
    mode: interactive ? 'interactive' : 'smoke', status: 'blocked', events: [] };
  const record = (stage, event) => {
    result.events.push({ stage, ...event });
    log.log(JSON.stringify({ stage, ...event }));
    writeEvidenceResult(evidenceDir, result);
  };
  let service;
  try {
    const port = await pickFreePort(35000 + randomBytes(2).readUInt16BE() % 14000);
    const miniPort = await pickFreePort(port + 20);
    const targetDir = join(root, 'src-tauri', 'target', 'minimax-probe', runId);
    const binary = join(targetDir, 'debug', 'examples', win ? 'minimax_webui_probe.exe' : 'minimax_webui_probe');
    const env = { ...buildRuntimeEnv(process.env, { ...paths, port, binary }, ''),
      MINIMAX_DATA_DIR: dataDir, WEBUI_HOST: ip, WEBUI_SERVER_PORT: String(port),
      WEBUI_MINIAPP_PORT: String(miniPort), WEBUI_OPEN_BROWSER: '0',
      MCODE_PROBE_STORE_ID: JSON.stringify([...randomBytes(16)]),
    };
    record('isolation', { evidenceDir, project, approvalDir, port, miniPort });
    const native = 'const r=require("node:module").createRequire(process.argv[1]);const D=r("better-sqlite3");new D(":memory:").close();r("node-pty");';
    const nativeCheck = spawnSync(process.execPath, ['-e', native, packageFile], { env, cwd: project, timeout: 10000, windowsHide: true });
    assert.equal(nativeCheck.status, 0, 'MiniMax SQLite or terminal native dependency failed');
    record('dependencies', { status: 'pass' });
    const buildLog = join(evidenceDir, 'cargo.log');
    const output = createWriteStream(buildLog);
    const outputFinished = finished(output);
    const build = spawn('cargo', ['build', '--locked', '--example', 'minimax_webui_probe'], {
      cwd: join(root, 'src-tauri'), stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
      env: { ...process.env, CARGO_TARGET_DIR: targetDir, CARGO_PROFILE_DEV_DEBUG: '0', CARGO_INCREMENTAL: '0',
        TAURI_CONFIG: mergeTauriConfig('', paths.appDataDir) },
    });
    build.stdout.pipe(output, { end: false });
    build.stderr.pipe(output, { end: false });
    let buildResult;
    try {
      buildResult = await processExit(build);
    } finally {
      output.end();
      await outputFinished;
    }
    assert.equal(buildResult.code, 0, `Tauri probe build failed. See ${buildLog}`);
    // The GNU build puts its WebView2 loader in debug/, above Cargo's example binary.
    const loader = join(targetDir, 'debug', 'WebView2Loader.dll');
    if (win && existsSync(loader)) copyFileSync(loader, join(dirname(binary), 'WebView2Loader.dll'));
    result.binarySha256 = createHash('sha256').update(readFileSync(binary)).digest('hex');
    record('build', { sha256: result.binarySha256 });
    service = startService(entry, env, project);
    record('first-service', await verifyService(service, { port, miniPort, dataDir: realpathSync(dataDir) }));
    await runWindow(binary, env, 'first-window', interactive, record);
    if (interactive) {
      assert.ok(inside(realpathSync(runRoot), realpathSync(project)), 'test project escaped the run directory');
      assert.ok(inside(realpathSync(runRoot), realpathSync(approvalDir)), 'approval target escaped the run directory');
      const approved = join(approvalDir, 'approved.txt');
      const denied = join(approvalDir, 'denied.txt');
      const file = lstatSync(approved, { throwIfNoEntry: false });
      record('task-files', { approved: file?.isFile() === true && file.size <= 32
        && inside(realpathSync(approvalDir), realpathSync(approved)) && readFileSync(approved, 'utf8').trim() === 'P0-approved',
        deniedFileAbsent: !lstatSync(denied, { throwIfNoEntry: false }) });
      await runWindow(binary, env, 'reopened-window', true, record);
      record('first-stop', await stopService(service));
      service = undefined;
      assert.equal(await pickFreePort(port, 1), port);
      assert.equal(await pickFreePort(miniPort, 1), miniPort);
      service = startService(entry, env, project);
      record('restarted-service', await verifyService(service, { port, miniPort, dataDir: realpathSync(dataDir) }));
      await runWindow(binary, env, 'restarted-window', true, record);
    }
    const profileRoot = win ? profileDir : join(paths.decoyHome, 'Library', 'WebKit');
    assert.ok(readdirSync(profileRoot).length > 0, 'isolated browser profile was not written');
    assert.ok(readdirSync(dataDir).length > 0, 'isolated MiniMax data was not written');
    record('data', { profileRoot, dataDir, isolated: true });
    result.automatedChecks = 'pass';
    result.manualChecks = interactive ? 'requires operator confirmation' : 'not_run';
  } catch (error) {
    result.status = 'fail';
    record('error', { message: error.message });
    process.exitCode = 1;
  } finally {
    if (service) {
      try {
        const stopped = await stopService(service);
        const port = result.events[0].port;
        const miniPort = result.events[0].miniPort;
        assert.equal(await pickFreePort(port, 1), port);
        assert.equal(await pickFreePort(miniPort, 1), miniPort);
        record('final-stop', { ...stopped, portsReleased: true });
      } catch (error) {
        result.status = 'fail';
        result.automatedChecks = 'fail';
        record('stop-error', { message: error.message });
        process.exitCode = 1;
      }
    }
    log.close();
  }
}

await main();
