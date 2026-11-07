// Runs the real shell and MiniMax package with synthetic memories and loopback services.
import { browser, expect } from '@wdio/globals';
import { createServer } from 'node:http';
import { mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import { isAbsolute, join, toNamespacedPath } from 'node:path';
import { isMcodeInstanceId } from '../launcher.mjs';
import { isRealChild } from '../real-child.mjs';

const root = process.env.DSH_E2E_RUN_ROOT;
const home = process.env.DSH_E2E_EXPECTED_HOME;
const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
const packagePath = process.env.DSH_E2E_MCODE_PACKAGE;
const nodePath = process.env.DSH_E2E_MCODE_NODE;
if (![root, home, evidenceDir, packagePath, nodePath].every(value => value && isAbsolute(value)) ||
    !isRealChild(root, home) || !isRealChild(root, evidenceDir) ||
    process.env.MAVIS_REGION !== 'cn' || process.env.MAVIS_BUILD_ENV !== 'test' ||
    process.env.MAVIS_LOCAL_RUNTIME_DISABLE_TOOLS !== '1') {
  throw new Error('MiniMax 记忆探针必须使用隔离 launcher 和测试环境');
}

function invoke(command, args = {}, progress = false) {
  return browser.executeAsync(
    'var done=arguments[arguments.length-1];const args=' + JSON.stringify(args) + ';' +
    (progress ? 'const channel=new window.__TAURI__.core.Channel();channel.onmessage=()=>{};args.onProgress=channel;' : '') +
    'window.__TAURI__.core.invoke(' + JSON.stringify(command) + ',args)' +
    '.then(value=>done({ok:true,value}),()=>done({ok:false}));'
  ).then(result => {
    if (!result.ok) throw new Error(command + ' 失败');
    return result.value;
  });
}

function recorder() {
  const models = [];
  const safety = [];
  const server = createServer(async (req, res) => {
    let raw = '';
    for await (const chunk of req) {
      raw += chunk;
      if (raw.length > 2_000_000) { res.writeHead(413).end(); return; }
    }
    let body;
    try { body = JSON.parse(raw); } catch { res.writeHead(400).end(); return; }
    if (req.url.startsWith('/mavis/api/v2/content')) {
      safety.push(body);
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ action: 1, errorCode: 0 }));
      return;
    }
    if (req.url !== '/v1/chat/completions') { res.writeHead(404).end(); return; }
    models.push(body);
    if (body.stream) {
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      for (const [delta, finishReason] of [[{ role: 'assistant', content: 'OK' }, null], [{}, 'stop']]) {
        res.write('data: ' + JSON.stringify({ id: 'stub', object: 'chat.completion.chunk', created: 1,
          model: 'xlink-probe', choices: [{ index: 0, delta, finish_reason: finishReason }] }) + '\n\n');
      }
      res.end('data: [DONE]\n\n');
    } else {
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({
        id: 'stub', object: 'chat.completion', created: 1, model: 'xlink-probe',
        choices: [{ index: 0, message: { role: 'assistant', content: 'OK' }, finish_reason: 'stop' }],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      }));
    }
  });
  return { server, models, safety };
}

function connect(url) {
  const socket = new WebSocket(url);
  return new Promise((resolve, reject) => {
    socket.addEventListener('open', () => resolve(socket), { once: true });
    socket.addEventListener('error', () => reject(new Error('MiniMax 连接失败')), { once: true });
  });
}

let sequence = 0;
function pathKey(path) {
  const real = realpathSync(path);
  return process.platform === 'win32' ? toNamespacedPath(real).toLowerCase() : real;
}

function request(socket, operation, body, stream = false) {
  const requestId = String(++sequence);
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => finish(new Error(operation + ' 超时')), 45_000);
    function finish(error, value) {
      clearTimeout(timeout);
      socket.removeEventListener('message', onMessage);
      if (error) reject(error); else resolve(value);
    }
    function onMessage(event) {
      const frame = JSON.parse(event.data);
      if (frame.requestId !== requestId) return;
      if (frame.kind === 'error') return finish(new Error(operation + ' 失败：' + frame.code));
      if (stream ? frame.kind === 'event' && frame.body?.dataJson === '[DONE]' : frame.kind === 'response') {
        finish(null, frame.body);
      }
    }
    socket.addEventListener('message', onMessage);
    socket.send(JSON.stringify({ protocolVersion: 1, kind: 'request', requestId, operation, body }));
  });
}

describe('MiniMax 共享记忆请求探针', function () {
  this.timeout(300_000);
  const evidence = { schemaVersion: 1, checks: {}, cleanup: 'not_run' };
  const local = recorder();
  let socket;
  let attempted = false;
  let ports = [];

  after(async () => {
    try {
      socket?.close();
      if (attempted) {
        await invoke('stop_mcode');
        const status = await invoke('get_mcode_status');
        if (status.running || status.stop_required || status.window_open) throw new Error('MiniMax 停止状态未收敛');
        for (const port of ports) {
          await new Promise((resolve, reject) => {
            const probe = createServer();
            probe.once('error', reject);
            probe.listen(port, '127.0.0.1', () => probe.close(resolve));
          });
        }
        evidence.cleanup = 'pass';
      }
    } catch (error) {
      evidence.cleanup = 'fail';
      throw error;
    } finally {
      if (local.server.listening) await new Promise(resolve => local.server.close(resolve));
      evidence.modelRequests = local.models.length;
      evidence.safetyRequests = local.safety.length;
      writeFileSync(join(evidenceDir, 'minimax-memory.json'), JSON.stringify(evidence, null, 2) + '\n');
    }
  });

  it('只将已授权且适用的记忆送入模型请求', async () => {
    await browser.setTimeout({ script: 150_000 });
    await browser.waitUntil(async () => browser.execute(() => !!window.__TAURI__?.core), { timeout: 60_000 });
    await new Promise(resolve => local.server.listen(0, '127.0.0.1', resolve));
    const localOrigin = `http://127.0.0.1:${local.server.address().port}`;

    await invoke('save_mcode_installation', { packagePath, nodePath });
    const configured = await invoke('get_mcode_status');
    const instanceId = configured.instance_id;
    const profile = configured.memory_profile;
    if (!isMcodeInstanceId(instanceId) || !isRealChild(home, configured.data_dir) ||
        pathKey(configured.data_dir) !== pathKey(join(home, 'kernels', 'mcode', 'instances', instanceId, 'home'))) {
      throw new Error('MiniMax 数据目录不属于隔离实例');
    }
    ports = [configured.port, configured.miniapp_port];
    const target = { instanceId, profile };
    await invoke('mcode_memory_set_enabled', { ...target, useApproved: true });
    writeFileSync(join(configured.data_dir, 'config.yaml'), JSON.stringify({
      contentReview: { testBaseURL: localOrigin }, sessionTitle: { enabled: false },
    }), { flag: 'wx', mode: 0o600 });
    const project = join(root, 'memory-project');
    const otherProject = join(root, 'memory-other-project');
    mkdirSync(project);
    mkdirSync(otherProject);
    attempted = true;
    await invoke('start_mcode', {}, true);
    const started = await invoke('get_mcode_status');
    expect(started.memory_state).toBe('pending_verification');

    const page = await fetch(`http://127.0.0.1:${configured.port}/`, { signal: AbortSignal.timeout(5000) });
    const html = await page.text();
    const match = html.match(/window\.__WEBUI_CONFIG__=([^<]+);<\/script>/);
    if (!match) throw new Error('MiniMax 工作台缺少连接配置');
    const bootstrap = JSON.parse(match[1]);
    if (pathKey(bootstrap.dataDir) !== pathKey(configured.data_dir)) throw new Error('工作台数据目录不匹配');
    const ws = new URL(bootstrap.websocketUrl);
    if (ws.protocol !== 'ws:' || ws.hostname !== '127.0.0.1' || Number(ws.port) !== configured.port) {
      throw new Error('工作台连接不是本轮回环端口');
    }
    ws.searchParams.set('token', bootstrap.token);
    socket = await connect(ws);
    const candidate = { name: 'xlink-probe', baseUrl: localOrigin + '/v1', apiKey: 'xlink-test-key',
      apiFormat: 'openai-completions', models: [{ modelId: 'xlink-probe', reasoning: false,
        toolCall: false, limit: { context: 4096, output: 256 } }] };
    const saved = await request(socket, 'saveUserModelProviderCandidate', { candidate, modelId: 'xlink-probe', saveAndUse: true });
    expect(saved.success).toBe(true);
    expect(saved.provider.baseUrl).toBe(localOrigin + '/v1');
    expect(saved.provider.apiFormat).toBe('openai-completions');

    async function turn(marker, workspace) {
      const session = await request(socket, 'createSession', { name: 'mavis', workspaceDir: workspace, teamModeOff: true });
      const selection = await request(socket, 'selectModel', { providerId: saved.provider.providerId, modelId: 'xlink-probe', sessionId: session.sessionId });
      expect(selection.success).toBe(true);
      await request(socket, 'sendMessage', { id: session.sessionId, content: marker }, true);
      expect(local.safety.some(body => body.content_text === marker && body.scene === 110)).toBe(true);
      const matched = local.models.filter(body => JSON.stringify(body).includes(marker));
      expect(matched.length).toBe(1);
      return JSON.stringify(matched[0]);
    }

    await turn('XLINK_A07_PREFLIGHT', project);
    evidence.checks.localModelAndSafety = true;

    const sourceStatus = await invoke('memory_status');
    const [family, sourceInstance, sourceProfile, extra] = sourceStatus.namespace.split('/');
    if (family !== 'dsh' || !sourceInstance || !sourceProfile || extra) throw new Error('DSH 记忆空间无效');
    const save = (title, body, scope = 'general', workspacePath = '') => invoke('memory_save', {
      input: { kind: 'memory', scope, workspace_path: workspacePath, title, body, pinned: true },
    });
    const shared = await save('授权记忆', 'XLINK_A07_SHARED');
    await save('未授权记忆', 'XLINK_A07_PRIVATE');
    const projectMemory = await save('项目记忆', 'XLINK_A07_PROJECT', 'project', project);
    await invoke('mcode_memory_save', { ...target, input: {
      kind: 'memory', scope: 'general', title: '本实例记忆', body: 'XLINK_A07_OWN', pinned: true,
    } });
    const grant = (row, sharedValue) => invoke('set_mcode_memory_share', { ...target, sourceInstance, sourceProfile,
      recordId: row.id, revision: row.revision, shared: sharedValue });
    await grant(shared, true);
    await grant(projectMemory, true);

    const allowed = await turn('XLINK_A07_ALLOWED', project);
    for (const marker of ['<plugin-hook-context>', 'XLINK_A07_SHARED', 'XLINK_A07_PROJECT', 'XLINK_A07_OWN']) expect(allowed).toContain(marker);
    expect(allowed).not.toContain('XLINK_A07_PRIVATE');
    evidence.checks.authorizedAndPrivate = true;

    await grant(shared, false);
    const revoked = await turn('XLINK_A07_REVOKED', otherProject);
    expect(revoked).toContain('XLINK_A07_OWN');
    for (const marker of ['XLINK_A07_SHARED', 'XLINK_A07_PROJECT', 'XLINK_A07_PRIVATE']) expect(revoked).not.toContain(marker);
    evidence.checks.revokedAndProjectMismatch = true;

    await invoke('mcode_memory_set_enabled', { ...target, useApproved: false });
    const disabled = await turn('XLINK_A07_DISABLED', project);
    for (const marker of ['XLINK_A07_SHARED', 'XLINK_A07_PROJECT', 'XLINK_A07_OWN', 'XLINK_A07_PRIVATE']) expect(disabled).not.toContain(marker);
    evidence.checks.disabled = true;
    evidence.packageVersion = started.package_version;
    evidence.nodeVersion = started.nodeVersion;
  });
});
