import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const source = readFileSync(new URL('../src/App.vue', import.meta.url), 'utf8');
const handler = source.slice(source.indexOf('let quitConfirmPending = false;'), source.indexOf('\nfunction refreshActivePanelData'));

function fixture({ ack = true, confirm = false, stopError = false } = {}) {
  const calls = [];
  const context = vm.createContext({
    invoke: async (cmd, args) => {
      calls.push(cmd);
      if (cmd === 'acknowledge_quit_request') {
        assert.equal(args.requestId, 7);
        if (ack instanceof Error) throw ack;
        return ack;
      }
      if (cmd === 'stop_kernel' && stopError) throw new Error('stop failed');
    },
    confirmDialog: async () => { calls.push('confirm'); return confirm; },
    toastError: () => calls.push('error'),
  });
  vm.runInContext(handler, context);
  return {
    calls,
    request: () => context.onQuitConfirmRequest({ payload: { request_id: 7, kernel_running: true } }),
  };
}

test('超时应答及 IPC 失败不再弹前端确认框，由原生接手', async () => {
  for (const ack of [false, new Error('IPC unavailable')]) {
    const f = fixture({ ack });
    await f.request();
    assert.deepEqual(f.calls, ['acknowledge_quit_request']);
  }
});

test('正常页面先应答；取消确认不会停止内核或退出', async () => {
  const f = fixture();
  await f.request();
  assert.deepEqual(f.calls, ['acknowledge_quit_request', 'confirm']);
});

test('确认退出后必须先成功停止内核', async () => {
  const f = fixture({ confirm: true });
  await f.request();
  assert.deepEqual(f.calls, ['acknowledge_quit_request', 'confirm', 'stop_kernel', 'confirm_close_shell']);
  const failed = fixture({ confirm: true, stopError: true });
  await failed.request();
  assert(!failed.calls.includes('confirm_close_shell'));
  assert(failed.calls.includes('error'));
});

test('确认框打开时重复退出只应答，不叠加弹窗；取消后可重试', async () => {
  let cancel;
  const confirmation = new Promise((resolve) => { cancel = resolve; });
  const f = fixture({ confirm: confirmation });
  const first = f.request();
  await new Promise(setImmediate);
  await f.request();
  assert.equal(f.calls.filter((call) => call === 'confirm').length, 1);
  cancel(false);
  await first;
  await f.request();
  assert.equal(f.calls.filter((call) => call === 'confirm').length, 2);
  assert(!f.calls.includes('stop_kernel'));
});
