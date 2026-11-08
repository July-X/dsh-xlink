// 真实进程/端口回归，不读取用户配置，也不按端口杀进程。
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { readFileSync, writeFileSync, existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:net';

const launcher = readFileSync(new URL('../src-tauri/src/kernel/mcode-unix-launcher.cjs', import.meta.url), 'utf8');
const unix = process.platform !== 'win32';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(probe, message) {
  const deadline = Date.now() + 8000;
  while (!await probe()) {
    assert.ok(Date.now() < deadline, message);
    await sleep(25);
  }
}
async function portFree(port) {
  const server = createServer();
  return new Promise(resolve => {
    server.once('error', () => resolve(false));
    server.listen(port, '127.0.0.1', () => server.close(() => resolve(true)));
  });
}
function fixture(t, source) {
  const dir = mkdtempSync(join(tmpdir(), 'xlink-mcode-parent-'));
  const entry = join(dir, 'service.cjs');
  const ready = join(dir, 'ready.json');
  writeFileSync(entry, source);
  const env = { ...process.env, READY: ready };
  delete env.NODE_OPTIONS;
  const preload = join(dir, 'preload.cjs');
  const preloaded = join(dir, 'preloaded');
  writeFileSync(preload, "require('node:fs').writeFileSync(process.env.PRELOADED, 'yes');");
  env.PRELOADED = preloaded;
  env.DSH_XLINK_MCODE_NODE_OPTIONS = '--require=' + JSON.stringify(preload);
  const child = spawn(process.execPath, ['-e', launcher, entry], {
    detached: true, stdio: ['pipe', 'ignore', 'pipe'], env,
  });
  // 监听必须在退出前安装；只向本测试启动且仍持有的组长发信号。
  const exited = once(child, 'exit');
  t.after(async () => {
    if (child.exitCode === null && child.signalCode === null) {
      try { process.kill(-child.pid, 'SIGKILL'); } catch {}
    }
    await exited;
    rmSync(dir, { recursive: true, force: true });
  });
  return { child, exited, ready, preloaded };
}
const dualService = `
const net = require('node:net'), fs = require('node:fs');
process.on('SIGTERM', () => {});
const servers = [net.createServer(), net.createServer()];
let ready = 0;
for (const server of servers) server.listen(0, '127.0.0.1', () => {
  if (++ready === 2) fs.writeFileSync(process.env.READY, JSON.stringify(servers.map(s => s.address().port)));
});
`;

test('Unix 管道在放行前关闭，不执行服务或用户预加载', { skip: !unix }, async t => {
  const { child, exited, ready, preloaded } = fixture(t, dualService);
  await sleep(200);
  assert.equal(existsSync(ready), false);
  assert.equal(existsSync(preloaded), false);
  child.stdin.end();
  await exited;
  assert.equal(existsSync(ready), false);
  assert.equal(existsSync(preloaded), false);
});

for (const mode of ['EOF', 'SIGTERM', '重复放行']) {
  test(`Unix ${mode} 回收两个真实端口，保留无关监听者`, { skip: !unix }, async t => {
    const unrelated = createServer();
    unrelated.listen(0, '127.0.0.1');
    await once(unrelated, 'listening');
    t.after(() => unrelated.close());
    const otherPort = unrelated.address().port;
    const { child, exited, ready, preloaded } = fixture(t, dualService);
    child.stdin.write(Buffer.from([1]));
    await until(() => existsSync(ready), '服务未启动');
    assert.equal(existsSync(preloaded), true);
    const ports = JSON.parse(readFileSync(ready, 'utf8'));
    for (const port of ports) assert.equal(await portFree(port), false);
    if (mode === 'EOF') child.stdin.end();
    else if (mode === 'SIGTERM') child.kill('SIGTERM');
    else child.stdin.write(Buffer.from([1]));
    await exited;
    for (const port of ports) await until(() => portFree(port), '监听端口未释放');
    assert.equal(await portFree(otherPort), false);
  });
}

test('Unix 服务提前退出后仍回收同组后代的监听端口', { skip: !unix }, async t => {
  const source = `
const { spawn } = require('node:child_process');
const child = spawn(process.execPath, ['-e', ${JSON.stringify(dualService)}], { stdio: 'ignore' });
const timer = setInterval(() => {
  if (require('node:fs').existsSync(process.env.READY)) { clearInterval(timer); process.exit(0); }
}, 10);
`;
  const { child, exited, ready } = fixture(t, source);
  child.stdin.write(Buffer.from([1]));
  await until(() => existsSync(ready), '后代未启动');
  const ports = JSON.parse(readFileSync(ready, 'utf8'));
  await exited;
  for (const port of ports) await until(() => portFree(port), '后代监听端口未释放');
});

test('Unix 生产接线保留父管道并在发布 PID 后放行', () => {
  const adapter = readFileSync(new URL('../src-tauri/src/kernel/kernel_adapter.rs', import.meta.url), 'utf8');
  assert.ok(adapter.includes('include_str!("mcode-unix-launcher.cjs")'));
  assert.ok(adapter.includes('command.stdin(Stdio::piped())'));
  assert.ok(adapter.includes('self.tree.contains_pid(pid)'));
  assert.ok(adapter.indexOf('let capture = self.tree.capture()') < adapter.indexOf('let group = crate::kernel::lifecycle::stop'));
  const service = readFileSync(new URL('../src-tauri/src/kernel/mcode_service.rs', import.meta.url), 'utf8');
  const start = service.slice(service.indexOf('pub fn start('));
  assert.ok(start.indexOf('instance::write_pid(') < start.indexOf('.release()'));
});
