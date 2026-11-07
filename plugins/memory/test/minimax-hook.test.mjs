import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('../hooks/context.mjs', import.meta.url));
const token = '0123456789abcdef0123456789abcdef';
const input = { hook_event_name: 'UserPromptSubmit', session_id: 's1', cwd: 'C:\\中文 空格\\项目', prompt: '查询记忆' };

function run(data, event = input) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [script], { env: { ...process.env, PLUGIN_DATA: data } });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('error', reject);
    child.on('close', (code) => resolve({ code, stdout, stderr }));
    child.stdin.end(JSON.stringify(event));
  });
}

test('MiniMax hook reads scoped bridge context and fails closed', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'xlink-memory-hook-'));
  const requests = [];
  let enabled = true;
  const server = createServer((req, res) => {
    requests.push({ url: req.url, auth: req.headers.authorization });
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ enabled, handoff_text: '交接', text: '记忆', experience_text: '经验' }));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const port = server.address().port;
    const credential = join(dir, 'xlink-memory-bridge.json');
    await writeFile(credential, JSON.stringify({ url: `http://127.0.0.1:${port}`, token }));
    let result = await run(dir);
    assert.equal(result.code, 0);
    assert.equal(result.stderr, '');
    assert.deepEqual(JSON.parse(result.stdout), { hookSpecificOutput: { hookEventName: 'UserPromptSubmit', additionalContext: '交接\n\n记忆\n\n经验' } });
    const query = new URL(requests[0].url, 'http://127.0.0.1');
    assert.equal(query.pathname, '/v1/memory/context');
    assert.equal(query.searchParams.get('q'), input.prompt);
    assert.equal(query.searchParams.get('workspace'), input.cwd);
    assert.equal(requests[0].auth, `Bearer ${token}`);
    enabled = false;
    result = await run(dir);
    assert.equal(result.stdout, '');
    await writeFile(credential, JSON.stringify({ url: `http://localhost:${port}`, token }));
    result = await run(dir);
    assert.equal(result.stdout, '');
    assert.equal(result.stderr, '');
    assert.equal(requests.length, 2);
    await rm(credential);
    result = await run(dir);
    assert.equal(result.stdout, '');
    assert.equal(result.code, 0);
    // 桥在但请求打不通：原因必须落到 stderr。静默失败过一次——面板显示「已授权」，
    // MiniMax 那边每回合都没有记忆，两头都查不到原因。
    const dead = createServer();
    await new Promise((resolve) => dead.listen(0, '127.0.0.1', resolve));
    const deadPort = dead.address().port;
    await new Promise((resolve) => dead.close(resolve));
    await writeFile(credential, JSON.stringify({ url: 'http://127.0.0.1:' + deadPort, token }));
    result = await run(dir);
    assert.equal(result.stdout, '');
    assert.match(result.stderr, /记忆桥/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    await rm(dir, { recursive: true, force: true });
  }
});
