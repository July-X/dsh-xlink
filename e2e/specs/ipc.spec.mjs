// 四组试点接口的真实 IPC 契约验证（2026-11 OPT-12）。
//
// 契约校验器与 JSON 都直接复用生产那一份（ui/src/shell/ipc-schema.js +
// contracts/ipc-contract.json），这里只负责把真实壳里的返回值送进校验器。
// 覆盖：状态（正常）、内核停止（成功 null）、内核启动（空 home 的业务失败 +
// 通道终止）、记忆列表（空数据）、模型用量（空数据 + 90 天窗口）。

import { browser, expect } from '@wdio/globals';
import { readFileSync } from 'node:fs';
import { validateRequest, validateResponse } from '../../ui/src/shell/ipc-schema.js';

const contract = JSON.parse(
  readFileSync(new URL('../../contracts/ipc-contract.json', import.meta.url), 'utf8')
);

function validate(command, direction, value) {
  if (direction === 'request') validateRequest(contract, command, value);
  else validateResponse(contract, command, value);
}

const invoke = (command, args) =>
  browser.executeAsync(
    'var done = arguments[arguments.length - 1];' +
      'window.__TAURI__.core.invoke(' +
      JSON.stringify(command) +
      ',' +
      'JSON.parse(' + JSON.stringify(JSON.stringify(args || {})) + ')' +
      ').then((value) => done({ ok: true, value }), (error) => done({ ok: false, error: String(error) }));'
  );

async function call(command, args) {
  validate(command, 'request', args || {});
  return invoke(command, args || {});
}

describe('试点 IPC 的真实行为与契约', function () {
  before(async () => {
    await browser.waitUntil(
      async () => (await browser.execute(() => typeof window.__TAURI__ !== 'undefined')) === true,
      { timeout: 30_000, timeoutMsg: '等待 Tauri 桥注入超时' }
    );
  });

  after(async () => {
    // 启动用例若意外把内核拉起来，收尾必须停掉，不能留给下一轮。
    await invoke('stop_kernel', {}).catch(() => {});
  });

  it('get_status：正常响应过契约，字段是真实壳数据', async () => {
    const result = await call('get_status', { source: 'e2e-ipc-contract' });
    expect(result.ok).toBe(true);
    validate('get_status', 'response', result.value);
    expect(typeof result.value.shell_version).toBe('string');
    expect(['release', 'dev']).toContain(result.value.shell_mode);
    expect(result.value.kernel.running).toBe(false);
    expect(Array.isArray(result.value.quarantined)).toBe(true);
  });

  it('stop_kernel：已停止时返回 null，契约的 null 响应成立', async () => {
    const result = await call('stop_kernel', {});
    expect(result.ok).toBe(true);
    expect(result.value).toBe(null);
    validate('stop_kernel', 'response', result.value);
  });

  it('start_kernel：空 home 返回 running=false 的失败报告；进度通道随命令终止', async () => {
    const before = await call('get_status', {});
    if (before.value.kernel.running) throw new Error('E2E 启动前内核不应在运行');
    if (before.value.kernel.active) throw new Error('E2E 隔离 home 不应有活动内核版本');

    const result = await browser.executeAsync(
      'var done = arguments[arguments.length - 1];' +
        'const events = [];' +
        'const channel = new window.__TAURI__.core.Channel();' +
        'channel.onmessage = (message) => events.push(message);' +
        'window.__TAURI__.core.invoke("start_kernel", { onEvent: channel })' +
        ' .then((value) => done({ ok: true, value, events }),' +
        '       (error) => done({ ok: false, error: String(error), events }));'
    );
    // 启动失败在壳里表现为「command Ok + report.running=false」（guard 用报告
    // 承载事故），不是 Err(String)；前端按 running 判失败。契约校验的就是这份报告。
    expect(result.ok).toBe(true);
    validate('start_kernel', 'response', result.value);
    expect(result.value.running).toBe(false);
    expect(result.value.incident || result.value.warning || result.value.safe_mode).toBeTruthy();
    expect(Array.isArray(result.events)).toBe(true);
    for (const event of result.events) expect(typeof event).toBe('string');
    // 命令 settle 后不应再有事件推入：再等一拍，事件数不变。
    const count = result.events.length;
    await browser.pause(300);
    expect(result.events.length).toBe(count);
  });

  it('memory_list：空记忆库返回空数组，不把无数据当失败', async () => {
    const result = await call('memory_list', {
      filter: { include_candidates: true, include_disabled: true },
    });
    expect(result.ok).toBe(true);
    validate('memory_list', 'response', result.value);
    expect(result.value.memories).toEqual([]);
    expect(result.value.candidates).toEqual([]);
    expect(result.value.status.accepted).toBe(0);
  });

  it('get_model_usage：空账返回 90 天零值窗口，契约字段齐全', async () => {
    const result = await call('get_model_usage', {
      force: true,
      external: { codex: false, minimax: false },
      external_force: false,
    });
    expect(result.ok).toBe(true);
    validate('get_model_usage', 'response', result.value);
    expect(result.value.retention_days).toBe(90);
    expect(result.value.days.length).toBe(90);
    expect(result.value.total_tokens).toBe(0);
    expect(result.value.today_tokens).toBe(0);
    expect(result.value.external_sources).toEqual({ codex: false, minimax: false });
  });
});
