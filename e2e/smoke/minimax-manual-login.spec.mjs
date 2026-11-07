// Real login is performed only by the user in the isolated workbench.
// The control files and evidence contain state labels only, never page data or URLs.
import { browser, expect } from '@wdio/globals';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { dirname, isAbsolute, join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { isMcodeInstanceId } from '../launcher.mjs';
import { classifyWorkbench, parseLoginAcknowledgment } from '../manual-login-control.mjs';
import { isRealChild } from '../real-child.mjs';

const root = process.env.DSH_E2E_RUN_ROOT;
const home = process.env.DSH_E2E_EXPECTED_HOME;
const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
const packagePath = process.env.DSH_E2E_MCODE_PACKAGE;
const nodePath = process.env.DSH_E2E_MCODE_NODE;
if (!root || !home || !evidenceDir || !packagePath || !nodePath ||
    !isAbsolute(root) || !isAbsolute(home) || !isAbsolute(packagePath) || !isAbsolute(nodePath) ||
    !isRealChild(root, home) || !isRealChild(root, evidenceDir)) {
  throw new Error('人工登录验收必须使用隔离 launcher 和指定的运行环境');
}

const stageFile = join(evidenceDir, 'manual-login-stage.json');
const evidenceFile = join(evidenceDir, 'manual-login-result.json');
const acknowledgments = { login: ['login', 'callback'] };

function stage(name) {
  writeFileSync(stageFile, JSON.stringify({ phase: name }) + '\n');
}

async function waitForAcknowledgment(name) {
  const path = join(evidenceDir, `manual-login-${name}.json`);
  const deadline = Date.now() + 30 * 60_000;
  while (Date.now() < deadline) {
    if (existsSync(path)) return parseLoginAcknowledgment(readFileSync(path, 'utf8'), acknowledgments[name]);
    await delay(1000);
  }
  throw new Error(`等待 ${name} 人工确认超时`);
}

function invoke(command, args = {}, progress = false) {
  return browser.executeAsync(
    'var done=arguments[arguments.length-1];const args=' + JSON.stringify(args) + ';' +
    (progress ? 'const channel=new window.__TAURI__.core.Channel();channel.onmessage=()=>{};args.onProgress=channel;' : '') +
    'window.__TAURI__.core.invoke(' + JSON.stringify(command) + ',args)' +
    '.then(value=>done({ok:true,value}),()=>done({ok:false}));'
  );
}

function portFree(port) {
  return new Promise(resolve => {
    const server = createServer();
    server.once('error', () => resolve(false));
    server.once('listening', () => server.close(() => resolve(true)));
    server.listen(port, '127.0.0.1');
  });
}

async function workbenchEnumerated() {
  const deadline = Date.now() + 5000;
  do {
    if ((await browser.getWindowHandles()).includes('mcode-workbench')) return true;
    await delay(500);
  } while (Date.now() < deadline);
  return false;
}

// 0.1.7 exposes the account phase as a DOM attribute. Return booleans only;
// neither the account details nor the page text leave the workbench.
async function readAuthenticated() {
  return browser.executeAsync(function (done) {
    const account = () => document.querySelector('[data-testid="account-login-page"]');
    const started = Date.now();
    let requested = false;
    let finished = false;
    const finish = value => {
      if (finished) return;
      finished = true;
      observer.disconnect();
      done(value);
    };
    const observer = new MutationObserver(() => {
      if (account()?.getAttribute('data-webui-account-login-phase') === 'authenticated') finish(true);
    });
    observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true,
      attributeFilter: ['data-webui-account-login-phase'] });
    function poll() {
      if (finished) return;
      const dialog = account();
      if (dialog?.getAttribute('data-webui-account-login-phase') === 'authenticated') {
        finish(true);
        return;
      }
      if (!dialog && !requested) {
        const trigger = document.querySelector('[data-testid="sidebar-user-menu-trigger"], [data-testid="sidebar-user-menu-trigger-rail"]');
        if (trigger) { trigger.click(); requested = true; }
      }
      const item = document.querySelector('[data-testid="user-menu-account-login"]');
      if (!dialog && item) item.click();
      if (Date.now() - started >= 10_000) { finish(false); return; }
      setTimeout(poll, 100);
    }
    poll();
  });
}

describe('MiniMax 隔离人工登录与恢复', function () {
  this.timeout(100 * 60_000);
  const evidence = {
    schemaVersion: 1,
    checks: { login: 'not_run', callback: 'not_run', loginLayerClosedAtObservation: 'not_run', windowReopened: 'not_run',
      reopenedLogin: 'not_run', reopenedSession: 'not_run', serviceRestarted: 'not_run',
      restartedLogin: 'not_run', restartedSession: 'not_run' },
    observations: {},
    cleanup: 'not_run',
  };
  let attemptedStart = false;
  let ports = [];
  let instanceId;

  before(async () => {
    await browser.setTimeout({ script: 150_000 });
    await browser.waitUntil(async () => browser.execute(() => !!window.__TAURI__?.core), { timeout: 60_000 });
  });

  after(async () => {
    try {
      if (attemptedStart) {
        await browser.switchToWindow('main');
        const result = await invoke('stop_mcode');
        const status = await invoke('get_mcode_status');
        if (!result.ok || !status.ok || status.value.running || status.value.window_open) throw new Error('停止未完成');
        for (const port of ports) if (!await portFree(port)) throw new Error('端口未释放');
      }
      evidence.cleanup = attemptedStart && ports.length === 2 ? 'pass' : 'not_run';
    } catch {
      evidence.cleanup = 'fail';
      throw new Error('未确认本轮 MiniMax 已停止；请保留隔离目录排查');
    } finally {
      stage('finished');
      writeFileSync(evidenceFile, JSON.stringify(evidence, null, 2) + '\n');
    }
  });

  it('登录、重开窗口并重启服务', async () => {
    const saved = await invoke('save_mcode_installation', { packagePath, nodePath });
    expect(saved.ok).toBe(true);
    attemptedStart = true;
    expect((await invoke('start_mcode', {}, true)).ok).toBe(true);
    const initial = await invoke('get_mcode_status');
    expect(initial.ok && initial.value.running && initial.value.window_open).toBe(true);
    instanceId = initial.value.instance_id;
    if (!isMcodeInstanceId(instanceId) || !isRealChild(root, initial.value.data_dir) ||
        !isRealChild(root, join(dirname(initial.value.data_dir), 'browser'))) {
      throw new Error('MiniMax 数据目录或浏览器档案未落在隔离目录');
    }
    ports = [initial.value.port, initial.value.miniapp_port];
    if (ports.some(port => !Number.isInteger(port) || port < 1 || port > 65535)) throw new Error('MiniMax 端口无效');
    stage('login');
    const login = await waitForAcknowledgment('login');
    evidence.checks.login = login.login;
    evidence.checks.callback = login.callback;
    if (login.login !== 'pass') throw new Error('人工登录未通过');

    const afterLogin = await invoke('get_mcode_status');
    if (!afterLogin.ok) throw new Error('登录后无法读取工作台状态');
    const handles = await browser.getWindowHandles();
    if (!handles.includes('main')) throw new Error('未找到隔离管理窗口');
    let windowState = classifyWorkbench(afterLogin.value, handles);
    evidence.observations.afterLogin = {
      serviceRunning: afterLogin.value.running,
      windowOpen: afterLogin.value.window_open,
      driverEnumerated: handles.includes('mcode-workbench'),
      windowClosureReason: 'unknown',
    };
    if (windowState === 'service_stopped') throw new Error('登录后 MiniMax 服务未运行');
    if (windowState !== 'ready') {
      expect((await invoke('open_mcode_workbench')).ok).toBe(true);
      const current = await invoke('get_mcode_status');
      if (!current.ok || !current.value.running || !current.value.window_open || current.value.instance_id !== instanceId) {
        throw new Error('无法重新打开 MiniMax 工作台');
      }
      if (windowState === 'closed') evidence.checks.windowReopened = 'pass';
      windowState = await workbenchEnumerated() ? 'ready' : 'driver_missing';
    }
    if (windowState === 'ready') {
      await browser.switchToWindow('mcode-workbench');
      if (afterLogin.value.window_open) {
        evidence.checks.loginLayerClosedAtObservation = await browser.execute(() =>
          !document.querySelector('[data-testid="account-login-page-overlay"]')) ? 'pass' : 'fail';
      }
      if (evidence.checks.windowReopened !== 'pass') {
        await browser.closeWindow();
        await browser.switchToWindow('main');
        await browser.waitUntil(async () => {
          const closed = await invoke('get_mcode_status');
          return closed.ok && closed.value.running && !closed.value.window_open;
        }, { timeout: 15_000, interval: 250 });
        expect((await invoke('open_mcode_workbench')).ok).toBe(true);
        const reopened = await invoke('get_mcode_status');
        expect(reopened.ok && reopened.value.running && reopened.value.window_open && reopened.value.instance_id === instanceId).toBe(true);
        evidence.checks.windowReopened = 'pass';
        if (await workbenchEnumerated()) await browser.switchToWindow('mcode-workbench');
        else windowState = 'driver_missing';
      }
      if (windowState === 'ready') evidence.checks.reopenedLogin = await readAuthenticated() ? 'pass' : 'not_run';
      await browser.switchToWindow('main');
    }
    stage('window_reopened');

    expect((await invoke('stop_mcode')).ok).toBe(true);
    const stopped = await invoke('get_mcode_status');
    expect(stopped.ok && !stopped.value.running && !stopped.value.window_open).toBe(true);
    for (const port of ports) if (!await portFree(port)) throw new Error('服务停止后端口未释放');
    expect((await invoke('start_mcode', {}, true)).ok).toBe(true);
    const restarted = await invoke('get_mcode_status');
    expect(restarted.ok && restarted.value.running && restarted.value.window_open && restarted.value.instance_id === instanceId).toBe(true);
    evidence.checks.serviceRestarted = 'pass';
    stage('service_restarted');
    if (await workbenchEnumerated()) {
      await browser.switchToWindow('mcode-workbench');
      evidence.checks.restartedLogin = await readAuthenticated() ? 'pass' : 'not_run';
      await browser.switchToWindow('main');
    }
    if (login.callback === 'fail') throw new Error('登录回调人工验收未通过');
  });
});
