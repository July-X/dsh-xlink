import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const source = readFileSync(new URL('../src/App.vue', import.meta.url), 'utf8');
const handler = source.slice(source.indexOf('let quitConfirmPending = false;'), source.indexOf('\nfunction refreshActivePanelData'));

function fixture({ ack = true, confirm = false, stopError = false, kernelRunning = true, mcodeRunning = false, mcodeStopError = false, mcodeStale = false } = {}) {
  const calls = [];
  const messages = [];
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
    confirmDialog: async (_title, detail) => { calls.push('confirm'); messages.push(detail); return confirm; },
    toastError: () => calls.push('error'),
    stopMcodeForExit: async () => {
      calls.push('stop_mcode');
      if (mcodeStopError) throw new Error('MiniMax stop failed');
    },
  });
  vm.runInContext(handler, context);
  return {
    calls,
    messages,
    request: () => context.onQuitConfirmRequest({ payload: { request_id: 7, kernel_running: kernelRunning, mcode_running: mcodeRunning, mcode_stale_record: mcodeStale } }),
  };
}

test('超时应答及 IPC 失败不再弹前端确认框，由原生接手', async () => {
  for (const ack of [false, new Error('IPC unavailable')]) {
    const f = fixture({ ack });
    await f.request();
    assert.deepEqual(f.calls, ['acknowledge_quit_request']);
  }
});

test('退出时先停止 MiniMax，停止失败保留外壳', async () => {
  const success = fixture({ confirm: true, mcodeRunning: true });
  await success.request();
  assert.deepEqual(success.calls, ['acknowledge_quit_request', 'confirm', 'stop_mcode', 'stop_kernel', 'confirm_close_shell']);
  const failed = fixture({ confirm: true, mcodeRunning: true, mcodeStopError: true });
  await failed.request();
  assert(!failed.calls.includes('stop_kernel'));
  assert(!failed.calls.includes('confirm_close_shell'));
  assert(failed.calls.includes('error'));
});

test('MiniMax 无法确认运行归属时提供人工清理入口，其它停止态仍可重试停止，退出也先收尾', () => {
  const panel = readFileSync(new URL('../src/shell/OverviewPanel.vue', import.meta.url), 'utf8');
  const quit = readFileSync(new URL('../../src-tauri/src/shell/quit.rs', import.meta.url), 'utf8');
  // 旧运行记录无法证明后代进程已退出：壳只提供「清理旧记录」，重试停止对它无效。
  // 2026-11-07 起概览页只有一份骨架，MiniMax 的入口就在共用「当前内核」卡里。
  assert.match(panel, /<el-button\s+v-if="mcodeView\?\.recovery_snapshot"[^>]+@click="discardMcodeRunRecord"/);
  assert.match(panel, /<el-button\s+v-else-if="mcodeView\?\.stop_required"[^>]+@click="stopMcode"/);
  // 退出探针区分当前受管进程和无法确认的旧记录；后者不应被当作已结束。
  assert.match(quit, /mcode_service::exit_state\(\)/);
  assert.match(quit, /"mcode_stale_record"/);
});

test('只剩一条旧运行记录时不停止 MiniMax，退出照常走', async () => {
  const f = fixture({ confirm: true, mcodeStale: true });
  await f.request();
  assert(!f.calls.includes('stop_mcode'), '旧记录不是活进程，不该走停止路径');
  assert(f.calls.includes('confirm_close_shell'), '旧记录不该把退出堵死');
  assert.match(f.messages[0], /无法确认/);
  assert.match(f.messages[0], /保留/);
  assert.match(f.messages[0], /不会自动结束/);
  assert.doesNotMatch(f.messages[0], /进程已不在|一并清理/);
  const quit = readFileSync(new URL('../../src-tauri/src/shell/quit.rs', import.meta.url), 'utf8');
  assert.match(quit, /未知的 MiniMax 进程不会自动停止/);
  const unknown = fixture({ mcodeStale: true, kernelRunning: false });
  await unknown.request();
  assert.match(unknown.messages[0], /本窗口没有管理/);
  assert.doesNotMatch(unknown.messages[0], /当前没有运行中的工作台/);
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
