// Run only through the isolated launcher: installation and its data stay in this run.
import { browser, expect } from '@wdio/globals';
import { writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { createServer } from 'node:net';
import { isRealChild } from '../real-child.mjs';

const home = process.env.DSH_E2E_EXPECTED_HOME;
const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
const nodePath = process.env.DSH_E2E_MCODE_NODE;
if (!home || !evidenceDir || !nodePath || !isAbsolute(home) || !isAbsolute(nodePath)) {
  throw new Error('受管安装验收必须使用隔离 launcher 和指定 Node');
}

function invoke(name, args = {}, progress = false) {
  return browser.executeAsync(
    'var done=arguments[arguments.length-1];const args=' + JSON.stringify(args) + ';' +
    (progress ? 'const onProgress=new window.__TAURI__.core.Channel();onProgress.onmessage=()=>{};args.onProgress=onProgress;' : '') +
    'window.__TAURI__.core.invoke(' + JSON.stringify(name) + ',args)' +
    '.then(value=>done({ok:true,value}),()=>done({ok:false}));'
  );
}

function free(port) {
  return new Promise(resolve => {
    const server = createServer();
    server.once('error', () => resolve(false));
    server.once('listening', () => server.close(() => resolve(true)));
    server.listen(port, '127.0.0.1');
  });
}

describe('MiniMax 隔离受管安装', function () {
  this.timeout(1_200_000);
  const evidence = { schemaVersion: 1, checks: {}, cleanup: 'not_run', processTreeCoverage: 'not_checked' };
  let ports = [];
  let attemptedStart = false;

  before(async () => {
    await browser.setTimeout({ script: 900_000 });
    await browser.waitUntil(async () => browser.execute(() => !!window.__TAURI__?.core), { timeout: 60_000 });
  });

  after(async () => {
    try {
      if (attemptedStart) {
        const stopped = await invoke('stop_mcode');
        const status = await invoke('get_mcode_status');
        if (!stopped.ok || !status.ok || status.value.running) throw new Error('stop failed');
        for (const port of ports) if (!await free(port)) throw new Error('port occupied');
        evidence.cleanup = ports.length === 2 ? 'pass' : 'partial';
      }
    } catch {
      evidence.cleanup = 'fail';
      throw new Error('未确认 MiniMax 服务已停止，请保留本轮隔离目录检查');
    } finally {
      writeFileSync(join(evidenceDir, 'minimax-managed-install.json'), JSON.stringify(evidence, null, 2) + '\n');
    }
  });

  it('校验安装目录、启动服务并停止', async () => {
    expect((await invoke('save_mcode_installation', { packagePath: null, nodePath })).ok).toBe(true);
    expect((await invoke('install_mcode', {}, true)).ok).toBe(true);
    const status = await invoke('get_mcode_status');
    expect(status.ok && status.value.installed).toBe(true);
    expect(status.value.package_version).toBe('0.1.7');
    if (!isRealChild(home, status.value.package_path)) {
      throw new Error('受管包不在本轮隔离目录内');
    }
    ports = [status.value.port, status.value.miniapp_port];
    if (ports.some(port => !Number.isInteger(port) || port < 1 || port > 65535)) throw new Error('端口无效');
    evidence.checks.installedUnderIsolatedHome = true;
    attemptedStart = true;
    expect((await invoke('start_mcode', {}, true)).ok).toBe(true);
    expect((await invoke('get_mcode_status')).value.running).toBe(true);
    evidence.checks.managedPackageStarts = true;
  });
});
