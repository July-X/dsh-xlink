import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { TOKEN_ENV, URL_ENV } from '../host/bridge-credential.js';

const pluginRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

/**
 * 这些判据钉的是**两个组件之间的契约**，不是某一个的意图：脚本按自己的位置找
 * 凭据文件（`adapter/../` 与 `adapter/../../`），插件按自己的位置写（物化根的
 * 上一级）。2026-10-12 的故障正是这条契约断了一头 —— 桥活着、内核环境里有两个
 * 变量，而盘上那份文件不存在，于是 `/ponytail-update` 恒报退出码 2。
 */
function fixture() {
  const fingerprint = mkdtempSync(join(tmpdir(), 'ponytail-update-'));
  const root = join(fingerprint, 'compat-a');
  const adapter = join(root, 'adapter');
  mkdirSync(adapter, { recursive: true });
  copyFileSync(join(pluginRoot, 'adapter', 'update.mjs'), join(adapter, 'update.mjs'));
  return {
    script: join(adapter, 'update.mjs'),
    // 壳写在物化根下的那份，与插件写在物化根上一级的那份。
    shellCredential: join(root, '.xlink-bridge.json'),
    pluginCredential: join(fingerprint, '.xlink-bridge.json'),
    cleanup: () => rmSync(fingerprint, { recursive: true, force: true }),
  };
}

/**
 * 跑脚本，并且**把内核环境里那两个变量清掉**：模型在工作台里跑它时就是这个
 * 处境（内核给子进程的环境会剔掉 `DSH_*`），所以文件那条路才是被测的那条。
 *
 * 必须**异步**跑：假桥就跑在本进程里，用同步的 `spawnSync` 会占住事件循环，
 * 于是子进程永远等不到应答 —— 测试自己造出一个死锁（第一版就是这么红的）。
 */
async function run(script, extraEnv = {}) {
  const env = { ...process.env };
  delete env[URL_ENV];
  delete env[TOKEN_ENV];
  // 注入的 env 最后合并：先删后加，否则「env 优先」那条判据注入的变量会被删掉，
  // 测的就不是 env 那条路了。
  Object.assign(env, extraEnv);
  const child = spawn(process.execPath, [script], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  let stderr = '';
  child.stdout.setEncoding('utf8');
  child.stderr.setEncoding('utf8');
  child.stdout.on('data', (chunk) => { stdout += chunk; });
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  const timer = setTimeout(() => child.kill('SIGKILL'), 30_000);
  const status = await new Promise((resolve) => child.on('close', resolve));
  clearTimeout(timer);
  return { status, stdout, stderr };
}

/** 起一个只回答这一件事的假桥；`seen` 用来在事后断言它收到了什么。 */
async function withBridge(seen, respond, body) {
  const server = createServer((request, response) => {
    seen.push({ url: request.url, authorization: request.headers.authorization });
    respond(response);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    return await body(`http://127.0.0.1:${server.address().port}`);
  } finally {
    server.closeAllConnections?.();
    await new Promise((resolve) => server.close(resolve));
  }
}

test('只有插件写在上一级的那份凭据时，也能同步并报出版本号', async () => {
  const f = fixture();
  const seen = [];
  await withBridge(
    seen,
    (response) => {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ version: '9.9.9', error: '' }));
    },
    async (url) => {
      writeFileSync(f.pluginCredential, JSON.stringify({ url, token: 'token-1' }), 'utf8');
      const out = await run(f.script);
      assert.equal(out.status, 0, out.stderr);
      assert.match(out.stdout, /已同步到 ponytail 9\.9\.9/);
      assert.match(out.stdout, /重启工作台后生效/);
    },
  );
  assert.deepEqual(seen, [
    { url: '/v1/builtin/sync?plugin=ponytail', authorization: 'Bearer token-1' },
  ]);
  f.cleanup();
});

test('env 与文件都在：env 优先，盘上那份不读', async () => {
  const f = fixture();
  const seen = [];
  await withBridge(
    seen,
    (response) => {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end('{"version":"1.0.0","error":""}');
    },
    async (url) => {
      // 盘上那份指向一个死地址：真被读到的话这次一定失败。
      writeFileSync(f.shellCredential, JSON.stringify({ url: 'http://127.0.0.1:1', token: 'stale' }));
      const out = await run(f.script, { [URL_ENV]: url, [TOKEN_ENV]: 'env-token' });
      assert.equal(out.status, 0, out.stderr);
    },
  );
  assert.equal(seen[0].authorization, 'Bearer env-token');
  f.cleanup();
});

test('401：带这枚令牌的每一份凭据都被删掉，并如实说失效', async () => {
  const f = fixture();
  await withBridge(
    [],
    (response) => {
      response.writeHead(401, { 'content-type': 'application/json' });
      response.end('{"error":"unauthorized"}');
    },
    async (url) => {
      const dead = JSON.stringify({ url, token: 'dead-token' });
      writeFileSync(f.shellCredential, dead, 'utf8');
      writeFileSync(f.pluginCredential, dead, 'utf8');
      const out = await run(f.script);
      assert.equal(out.status, 1);
      assert.match(out.stderr, /凭据已失效/);
      assert.doesNotMatch(out.stdout, /已同步到/, '不许把失败说成同步成功');
      assert.equal(existsSync(f.shellCredential), false, '读到的那份要删掉');
      assert.equal(existsSync(f.pluginCredential), false, '另一份同样失效的也要删，否则用户要多撞一次');
    },
  );
  f.cleanup();
});

test('两处都没有凭据：退出码 2，并指路面板「检查更新」', async () => {
  const f = fixture();
  const out = await run(f.script);
  assert.equal(out.status, 2);
  assert.match(out.stderr, /找不到桌面壳的同步桥/);
  assert.match(out.stderr, /检查更新/);
  f.cleanup();
});
