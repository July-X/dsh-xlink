import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const source = readFileSync(new URL('../src/App.vue', import.meta.url), 'utf8');
const handler = source.slice(source.indexOf('let quitConfirmPending = false;'), source.indexOf('\nfunction refreshActivePanelData'));

function fixture({ ack = true, confirm = false, stopError = false, mcodeRunning = false, mcodeStopError = false, mcodeStale = false } = {}) {
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
    stopMcodeForExit: async () => {
      calls.push('stop_mcode');
      if (mcodeStopError) throw new Error('MiniMax stop failed');
    },
  });
  vm.runInContext(handler, context);
  return {
    calls,
    request: () => context.onQuitConfirmRequest({ payload: { request_id: 7, kernel_running: true, mcode_running: mcodeRunning, mcode_stale_record: mcodeStale } }),
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
  assert.match(panel, /<el-button\s+v-if="mcodeView\?\.stale_pid"[^>]+@click="discardMcodeRunRecord"/);
  assert.match(panel, /<el-button\s+v-else-if="mcodeView\?\.stop_required"[^>]+@click="stopMcode"/);
  // 退出探针是三态：「有进程在跑」与「只剩一条旧记录」必须分开，
  // 否则旧记录会把退出永久堵死（进程早没了，退出时清掉记录即可）。
  assert.match(quit, /mcode_service::exit_state\(\)/);
  assert.match(quit, /"mcode_stale_record"/);
});

test('只剩一条旧运行记录时不停止 MiniMax，退出照常走', async () => {
  const f = fixture({ confirm: true, mcodeStale: true });
  await f.request();
  assert(!f.calls.includes('stop_mcode'), '旧记录不是活进程，不该走停止路径');
  assert(f.calls.includes('confirm_close_shell'), '旧记录不该把退出堵死');
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
