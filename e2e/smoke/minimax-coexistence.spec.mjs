// The launcher supplies a fresh home and an explicit read-only MiniMax package.
import { browser, expect } from '@wdio/globals';
import { writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { join } from 'node:path';

const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
const packagePath = process.env.DSH_E2E_MCODE_PACKAGE;
const nodePath = process.env.DSH_E2E_MCODE_NODE;
const dshPort = Number(process.env.DSH_E2E_EXPECTED_PORT);
if (!evidenceDir || !packagePath || !nodePath || !process.env.DSH_E2E_EXPECTED_HOME || !Number.isInteger(dshPort) || dshPort < 1 || dshPort > 65535) {
  throw new Error('并行验收必须通过隔离 launcher 运行');
}

function free(port) {
  return new Promise(resolve => {
    const server = createServer();
    server.once('error', () => resolve(false));
    server.once('listening', () => server.close(() => resolve(true)));
    server.listen(port, '127.0.0.1');
  });
}

function command(name, args = {}, progress = false) {
  return browser.executeAsync(
    'var done=arguments[arguments.length-1];const args=' + JSON.stringify(args) + ';' +
    (progress ? 'const channel=new window.__TAURI__.core.Channel();channel.onmessage=()=>{};args.onEvent=channel;' : '') +
    'window.__TAURI__.core.invoke(' + JSON.stringify(name) + ',args)' +
    '.then(value=>done({ok:true,value}),()=>done({ok:false}));'
  );
}

function startMcode() {
  return browser.executeAsync(
    'var done=arguments[arguments.length-1];const onProgress=new window.__TAURI__.core.Channel();' +
    'onProgress.onmessage=()=>{};window.__TAURI__.core.invoke("start_mcode",{onProgress})' +
    '.then(()=>done({ok:true}),()=>done({ok:false}));'
  );
}

describe('DSH 与 MiniMax 隔离并行', function () {
  this.timeout(1_200_000);
  const evidence = { schemaVersion: 1, checks: {}, processTreeCoverage: 'not_checked', cleanup: 'not_run' };
  let attempted = false;
  let mcodeAttempted = false;
  let mcodePorts = [];

  before(async () => {
    await browser.setTimeout({ script: 900_000 });
    await browser.waitUntil(async () => browser.execute(() => !!window.__TAURI__?.core), { timeout: 60_000 });
  });

  after(async () => {
    try {
      if (attempted) {
        if (mcodeAttempted && mcodePorts.length === 0) {
          const status = await command('get_mcode_status');
          const ports = [status.value?.port, status.value?.miniapp_port];
          if (status.ok && ports.every(port => Number.isInteger(port) && port > 0 && port <= 65535)) mcodePorts = ports;
        }
        const mcode = await command('stop_mcode');
        const dsh = await command('stop_kernel');
        if (!mcode.ok || !dsh.ok) throw new Error('stop failed');
        const left = await command('get_status');
        const right = await command('get_mcode_status');
        if (!left.ok || !right.ok || left.value.kernel.running || right.value.running) throw new Error('still running');
        for (const port of [dshPort, ...mcodePorts]) if (!await free(port)) throw new Error('port still bound');
      }
      evidence.cleanup = !attempted ? 'not_run' : mcodeAttempted && mcodePorts.length === 0 ? 'partial' : 'pass';
    } catch {
      evidence.cleanup = 'fail';
      throw new Error('并行验收未能确认两个服务均已停止，请保留本轮隔离目录检查');
    } finally {
      writeFileSync(join(evidenceDir, 'minimax-coexistence.json'), JSON.stringify(evidence, null, 2) + '\n');
    }
  });

  it('并行运行并分别停止，另一个工作台保持运行', async () => {
    attempted = true;
    expect((await command('install_kernel', { version: '0.2.1-rc.3' }, true)).ok).toBe(true);
    const firstDsh = await command('start_kernel', {}, true);
    expect(firstDsh.ok && firstDsh.value.running).toBe(true);
    expect((await command('save_mcode_installation', { packagePath, nodePath })).ok).toBe(true);
    mcodeAttempted = true;
    expect((await startMcode()).ok).toBe(true);
    expect((await command('get_status')).value.kernel.running).toBe(true);
    const mcode = await command('get_mcode_status');
    expect(mcode.value.running).toBe(true);
    mcodePorts = [mcode.value.port, mcode.value.miniapp_port];
    if (mcodePorts.some(port => !Number.isInteger(port) || port < 1 || port > 65535)) throw new Error('MiniMax 端口无效');
    evidence.checks.bothRunning = true;

    expect((await command('stop_mcode')).ok).toBe(true);
    expect((await command('get_status')).value.kernel.running).toBe(true);
    expect((await command('get_mcode_status')).value.running).toBe(false);
    evidence.checks.stopMcodeKeepsDsh = true;

    expect((await startMcode()).ok).toBe(true);
    expect((await command('stop_kernel')).ok).toBe(true);
    expect((await command('get_status')).value.kernel.running).toBe(false);
    expect((await command('get_mcode_status')).value.running).toBe(true);
    evidence.checks.stopDshKeepsMcode = true;
  });
});
