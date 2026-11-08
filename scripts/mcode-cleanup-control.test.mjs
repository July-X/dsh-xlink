// 本机隔离夹具：认证控制通道不靠进程名、端口号或旧 PID 授权。
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import net from 'node:net';
import { randomBytes } from 'node:crypto';
import vm from 'node:vm';
const controlTest = (name, fn) => test(name, { skip: process.platform === 'win32' }, fn);
const control = readFileSync(new URL('../src-tauri/src/kernel/mcode-cleanup-control.cjs', import.meta.url), 'utf8');
const client = readFileSync(new URL('../src-tauri/src/kernel/mcode-cleanup-client.cjs', import.meta.url), 'utf8');
const adapter = readFileSync(new URL('../src-tauri/src/kernel/kernel_adapter.rs', import.meta.url), 'utf8');
const launcher = process.platform === 'win32'
  ? adapter.match(/MCODE_WINDOWS_LAUNCHER: &str = r#"([\s\S]*?)"#;/)[1]
  : readFileSync(new URL('../src-tauri/src/kernel/mcode-unix-launcher.cjs', import.meta.url), 'utf8');
// 模拟“父管道已断但自动收尾未完成”：只省略自动 EOF 停止，认证协议仍为生产源码。
const residualLauncher = `
const { spawn } = require('node:child_process');
let child;
function stop() {
  if (!child || child.exitCode !== null) process.exit(0);
  child.once('exit', () => process.exit(0));
  child.kill();
}
xlinkCleanupControl(stop, () => child);
process.stdin.once('data', () => {
  child = spawn(process.execPath, [process.argv[1]], { stdio: 'ignore', env: process.env });
});
process.stdin.resume();
`;
// 受限云环境可显式启用传输替身；生产源码不变，此模式不算原生管道验收。
const tcpTransport = process.env.XLINK_TEST_TCP_CONTROL === '1';
const mappedPorts = new Map();
const transportPrelude = tcpTransport ? `
const _net = require('node:net'), _fs = require('node:fs');
const _createServer = _net.createServer, _connect = _net.createConnection;
_net.createServer = (...args) => {
  const server = _createServer(...args), listen = server.listen.bind(server);
  server.listen = (_endpoint, callback) => listen(Number(process.env.XLINK_TEST_CONTROL_PORT), '127.0.0.1', callback);
  return server;
};
_net.createConnection = () => _connect({ port: Number(process.env.XLINK_TEST_CONTROL_PORT), host: '127.0.0.1' });
_fs.chmodSync = () => {};
_fs.unlinkSync = () => {};
` : '';
async function availablePort() {
  const server = net.createServer();
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(probe) {
  const deadline = Date.now() + 8000;
  while (!await probe()) {
    assert.ok(Date.now() < deadline, '夹具等待超时');
    await sleep(25);
  }
}
async function free(port) {
  return new Promise(resolve => {
    const server = net.createServer();
    server.once('error', () => resolve(false));
    server.listen(port, '127.0.0.1', () => server.close(() => resolve(true)));
  });
}
async function request(endpoint, nonce, action = 'probe') {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(tcpTransport ? { port: mappedPorts.get(endpoint), host: '127.0.0.1' } : endpoint);
    let output = '';
    socket.setTimeout(3000, () => { socket.destroy(); reject(new Error('timeout')); });
    socket.on('error', reject);
    socket.on('connect', () => socket.write(JSON.stringify({ nonce, action }) + '\n'));
    socket.on('data', bytes => { output += bytes; });
    socket.on('end', () => resolve(output ? JSON.parse(output) : null));
  });
}
async function fixture(t, count = 2, selectedLauncher = residualLauncher) {
  const dir = mkdtempSync(join(tmpdir(), 'xlink-cleanup-'));
  const nonce = randomBytes(32).toString('hex');
  const endpointId = randomBytes(16).toString('hex');
  const endpoint = process.platform === 'win32' ? '\\\\.\\pipe\\xlink-mcode-' + endpointId : '/tmp/xlink-mcode-' + endpointId + '.sock';
  const tcpPort = tcpTransport ? await availablePort() : null;
  if (tcpTransport) mappedPorts.set(endpoint, tcpPort);
  const ready = join(dir, 'ready.json');
  const entry = join(dir, 'service.cjs');
  writeFileSync(entry, `
    const net = require('node:net'), fs = require('node:fs');
    if (process.env.DSH_XLINK_MCODE_CLEANUP_NONCE || process.env.DSH_XLINK_MCODE_CLEANUP_ENDPOINT) process.exit(3);
    const servers = Array.from({length: ${count}}, () => net.createServer());
    let pending = servers.length;
    process.on('SIGTERM', () => process.exit(0));
    for (const server of servers) server.listen(0, '127.0.0.1', () => {
      if (!--pending) fs.writeFileSync(process.env.READY, JSON.stringify(servers.map(s => s.address().port)));
    });
  `);
  const env = { ...process.env, READY: ready, DSH_XLINK_MCODE_CLEANUP_NONCE: nonce,
    DSH_XLINK_MCODE_CLEANUP_ENDPOINT: endpoint, XLINK_TEST_CONTROL_PORT: String(tcpPort), WEBUI_SERVER_PORT: '4001', WEBUI_MINIAPP_PORT: '4002' };
  delete env.NODE_OPTIONS; delete env.DSH_XLINK_MCODE_NODE_OPTIONS;
  const child = spawn(process.execPath, ['-e', transportPrelude + control + '\n' + selectedLauncher, entry],
    { detached: process.platform !== 'win32', stdio: ['pipe', 'ignore', 'ignore'], env });
  const exited = once(child, 'exit');
  t.after(async () => {
    if (child.exitCode === null && child.signalCode === null) {
      child.stdin.end();
      await sleep(50);
      await request(endpoint, nonce, 'stop').catch(() => {});
      await Promise.race([exited, sleep(4000)]);
      if (child.exitCode === null && child.signalCode === null) child.kill();
    }
    await exited;
    rmSync(dir, { recursive: true, force: true });
    if (process.platform !== 'win32') rmSync(endpoint, { force: true });
  });
  await until(async () => !!await request(endpoint, nonce).catch(() => null));
  return { dir, child, exited, nonce, endpoint, ready, tcpPort };
}

controlTest('畸形 JSON、null、数组和标量只断开请求，不得使启动器退出', async t => {
  const f = await fixture(t, 2, launcher);
  for (const frame of ['{', 'null', '[]', '42', 'true', '"text"', '{}',
    JSON.stringify({ nonce: f.nonce, action: [] }),
    JSON.stringify({ nonce: {}, action: 'stop' }),
    JSON.stringify({ nonce: f.nonce, action: 'unknown' })]) {
    await new Promise((resolve, reject) => {
      const socket = net.createConnection(tcpTransport
        ? { port: mappedPorts.get(f.endpoint), host: '127.0.0.1' } : f.endpoint);
      socket.setTimeout(3000, () => { socket.destroy(); reject(new Error('malformed frame timeout')); });
      socket.on('error', reject);
      socket.on('connect', () => socket.write(frame + '\n'));
      socket.on('data', () => reject(new Error('不合法帧不得获得认证响应')));
      socket.on('close', resolve);
    });
    assert.equal((await request(f.endpoint, f.nonce)).pid, f.child.pid, frame);
    assert.equal(existsSync(f.ready), false);
  }
});

controlTest('放行前可验证启动器，服务身份必须为空', async t => {
  const f = await fixture(t);
  const owner = await request(f.endpoint, f.nonce);
  assert.equal(owner.pid, f.child.pid);
  assert.equal(owner.service_pid, null);
  assert.deepEqual(owner.ports, [4001, 4002]);
  assert.equal(existsSync(f.ready), false);
});
for (const count of [1, 2]) {
  controlTest(`认证清理回收 ${count} 个服务端口，错误 nonce 不得停止服务`, async t => {
    const f = await fixture(t, count);
    f.child.stdin.write(Buffer.from([1]));
    await until(() => existsSync(f.ready));
    const ports = JSON.parse(readFileSync(f.ready, 'utf8'));
    assert.equal(await request(f.endpoint, 'wrong', 'stop'), null);
    for (const port of ports) assert.equal(await free(port), false);
    const owner = await request(f.endpoint, f.nonce);
    assert.ok(owner.service_pid && owner.service_pid !== f.child.pid);
    const denied = await request(f.endpoint, f.nonce, 'stop');
    assert.equal(denied.accepted, false, '仍由另一窗口管理时必须拒绝');
    for (const port of ports) assert.equal(await free(port), false);
    f.child.stdin.end();
    await until(async () => !(await request(f.endpoint, f.nonce)).parent_connected);
    assert.equal((await request(f.endpoint, f.nonce, 'stop')).accepted, true);
    await f.exited;
    for (const port of ports) await until(() => free(port));
  });
}
controlTest('客户端拒绝已被替换的凭据，不连接新一轮启动器', async t => {
  const f = await fixture(t);
  const path = join(f.dir, 'receipt.json');
  writeFileSync(path, JSON.stringify({ nonce: f.nonce, endpoint: f.endpoint }));
  const child = spawn(process.execPath, ['-e', transportPrelude + client, path, 'stop'], {
    env: { ...process.env, NODE_OPTIONS: '', DSH_XLINK_CLEANUP_EXPECTED: 'old-generation', DSH_XLINK_CLEANUP_ENDPOINT: f.endpoint, XLINK_TEST_CONTROL_PORT: String(f.tcpPort) },
    stdio: 'ignore',
  });
  const [code] = await once(child, 'exit');
  assert.equal(code, 1);
  assert.equal((await request(f.endpoint, f.nonce)).pid, f.child.pid);
});
controlTest('挂起端点有界失败，不伪报已清理', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'xlink-cleanup-timeout-'));
  const nonce = randomBytes(32).toString('hex');
  const endpoint = process.platform === 'win32' ? '\\\\.\\pipe\\xlink-test-' + nonce : join(dir, 'hang.sock');
  const sockets = [];
  const server = net.createServer(socket => sockets.push(socket));
  if (tcpTransport) server.listen(0, '127.0.0.1');
  else server.listen(endpoint);
  await once(server, 'listening');
  t.after(() => { sockets.forEach(s => s.destroy()); server.close(); rmSync(dir, { recursive: true, force: true }); });
  const path = join(dir, 'receipt.json');
  writeFileSync(path, JSON.stringify({ nonce, endpoint }));
  const child = spawn(process.execPath, ['-e', transportPrelude + client, path, 'probe'], {
    env: { ...process.env, NODE_OPTIONS: '', DSH_XLINK_CLEANUP_EXPECTED: nonce, DSH_XLINK_CLEANUP_ENDPOINT: endpoint, XLINK_TEST_CONTROL_PORT: tcpTransport ? String(server.address().port) : '' }, stdio: 'ignore',
  });
  const [code] = await once(child, 'exit');
  assert.equal(code, 1);
});

controlTest('两平台启动器都接入同一认证控制入口', () => {
  assert.match(launcher, /xlinkCleanupControl/);
  const service = readFileSync(new URL('../src-tauri/src/kernel/mcode_service.rs', import.meta.url), 'utf8');
  const start = service.slice(service.indexOf('pub fn start('));
  const save = start.indexOf('.save(&receipt_path)');
  const ready = start.indexOf('.wait_ready(&receipt_path)');
  const release = start.indexOf('.release()');
  assert.ok(save >= 0 && save < ready && ready < release);
});

controlTest('生产启动器的认证入口就绪后才放行，父管道断开仍回收服务', async t => {
  const f = await fixture(t, 2, launcher);
  assert.equal((await request(f.endpoint, f.nonce)).service_pid, null);
  f.child.stdin.write(Buffer.from([1]));
  await until(() => existsSync(f.ready));
  const ports = JSON.parse(readFileSync(f.ready, 'utf8'));
  f.child.stdin.end();
  await f.exited;
  for (const port of ports) await until(() => free(port));
});

test('Windows 暂不创建控制管道，并在服务启动前移除凭据', () => {
  const env = { DSH_XLINK_MCODE_CLEANUP_ENDPOINT: 'public-endpoint', DSH_XLINK_MCODE_CLEANUP_NONCE: 'private-secret' };
  let bound = false;
  vm.runInNewContext(control + '; xlinkCleanupControl(() => {}, () => null);', {
    process: { platform: 'win32', env },
    require: name => name === 'node:net' ? { createServer() { bound = true; throw new Error('不得绑定 Windows 管道'); } } : {},
  });
  assert.equal(bound, false);
  assert.deepEqual(env, {});
});

test('TERM 在 SIGKILL 前只移除本次成功绑定的端点，绑定失败不删除路径', () => {
  for (const bound of [true, false]) {
    const callbacks = new Map();
    const removed = [];
    const server = {
      on() {},
      close() {},
      listen(_endpoint, ready) { if (bound) ready(); },
    };
    vm.runInNewContext(control + '; xlinkCleanupControl(() => {}, () => null);', {
      process: {
        platform: 'darwin',
        env: { DSH_XLINK_MCODE_CLEANUP_ENDPOINT: '/tmp/owned-test.sock', DSH_XLINK_MCODE_CLEANUP_NONCE: 'private-secret' },
        on: (name, callback) => callbacks.set(name, callback),
        once: (name, callback) => callbacks.set(name, callback),
      },
      require: name => name === 'node:net' ? { createServer: () => server }
        : { chmodSync() {}, unlinkSync: path => removed.push(path) },
    });
    callbacks.get('SIGTERM')();
    callbacks.get('SIGTERM')();
    callbacks.get('exit')();
    assert.deepEqual(removed, bound ? ['/tmp/owned-test.sock'] : []);
  }
});
