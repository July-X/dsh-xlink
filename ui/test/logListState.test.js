// 日志清单是一个**独立数据源**（审查 R2-P2-03）。
//
// `logModal.files` 初始为空，而它过去只有 `showLogs()` 之后才被填。概览控制塔
// 拿 `files.length` 直接显示「暂无」——于是用户第一次打开概览看到的是「暂无」，
// 哪怕机器上有二十份日志、只是他还没打开过弹层。「暂无」是一个**结论**，
// 不能由「没问过」冒充。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const calls = [];
let listResult = [{ name: 'kernel-2026-10-07.log' }, { name: 'plugin-x.log' }];
let failNext = false;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command) {
        calls.push(command);
        if (command === 'list_log_files') {
          if (failNext) {
            failNext = false;
            return Promise.reject(new Error('logs unreachable'));
          }
          return Promise.resolve(listResult);
        }
        if (command === 'read_log') return Promise.resolve('内容');
        return Promise.resolve([]);
      },
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
  addEventListener() {},
  removeEventListener() {},
  getComputedStyle() {
    return { transitionDuration: '0s', animationDuration: '0s', transitionDelay: '0s', animationDelay: '0s' };
  },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
const makeElement = () => ({
  ownerDocument: globalThis.document,
  style: {},
  classList: { add() {}, remove() {}, contains() { return false; }, toggle() {} },
  addEventListener() {},
  removeEventListener() {},
  setAttribute() {},
  removeAttribute() {},
  appendChild() {},
  removeChild() {},
  insertBefore() {},
});
let bodyAppends = 0;
const body = makeElement();
body.appendChild = () => {
  bodyAppends += 1;
};
globalThis.document = {
  hidden: false,
  createElement: makeElement,
  createElementNS: makeElement,
  createTextNode: makeElement,
  createComment: makeElement,
  body,
  documentElement: makeElement(),
  addEventListener() {},
  removeEventListener() {},
};
globalThis.requestAnimationFrame = (callback) => {
  callback();
  return 1;
};
globalThis.cancelAnimationFrame = () => {};

const { logModal, loadLogList, refreshLogTabs } = await import('../src/logs/logs.js');
const CONTROL_TOWER = readFileSync(
  new URL('../src/diagnostics/ControlTower.vue', import.meta.url),
  'utf8'
);

const listCalls = () => calls.filter((c) => c === 'list_log_files').length;

test('初始是「尚未读取」，不是「暂无」', () => {
  assert.equal(logModal.listState, 'unloaded', 'files 为空时分不清「没有」与「没读过」，所以要一个独立状态');
});

test('静默读取把状态推进到 ready 并填上清单', async () => {
  const files = await loadLogList();
  assert.equal(logModal.listState, 'ready');
  assert.equal(logModal.listError, '');
  assert.equal(files.length, 2);
  assert.equal(logModal.files.length, 2);
});

test('读失败时状态是 failed 且带原因，不留成「空清单」', async () => {
  failNext = true;
  const files = await loadLogList();
  assert.equal(logModal.listState, 'failed', '失败绝不能退回成 ready + 空数组——那正是要修的那个谎');
  assert.equal(files, null);
  assert.match(logModal.listError, /logs unreachable/, '要留原因：用户点了重试还失败，得知道为什么');
});

test('静默读取不弹提示，失败也不出声', async () => {
  const before = bodyAppends;
  failNext = true;
  await loadLogList();
  await loadLogList();
  assert.equal(bodyAppends, before, '概览是最不该在用户没动作时弹提示的地方');
});

test('在途去重：控制塔挂载与弹层刷新不会各发一次', async () => {
  calls.length = 0;
  await Promise.all([loadLogList(), loadLogList()]);
  assert.equal(listCalls(), 1, '同一时刻只该有一次 list_log_files');
});

test('弹层的 refreshLogTabs 同样更新这组状态', async () => {
  failNext = true;
  await refreshLogTabs();
  assert.equal(logModal.listState, 'failed');
  await refreshLogTabs();
  assert.equal(logModal.listState, 'ready');
  assert.equal(logModal.listError, '');
});

test('控制塔把四种状态都当成不同的话说，不拿「暂无」冒充', () => {
  // 「暂无」是一个**结论**：确认过没有才可以说。没问过、正在问、问失败了都
  // 不是「没有」——用户看到「暂无」会以为机器上确实一份日志都没有。
  for (const [state, said] of [
    ['unloaded', '尚未读取'],
    ['loading', '读取中'],
    ['failed', '读取失败'],
  ]) {
    assert.ok(
      CONTROL_TOWER.includes(`listState === '${state}'`) &&
        CONTROL_TOWER.includes(`return { text: '${said}`),
      `${state} 状态要显示「${said}」`
    );
  }
  // 「暂无」必须排在 failed 之后：只有确认读到、且确实是空，才轮到它。
  const failedAt = CONTROL_TOWER.indexOf("listState === 'failed') return");
  const emptyAt = CONTROL_TOWER.indexOf("!logModal.files.length) return { text: '暂无'");
  assert.ok(failedAt > 0 && emptyAt > failedAt, '「暂无」只能出现在 failed 判定之后');
});

test('控制塔在挂载时读一次清单，失败时点那行就是重试', () => {
  assert.ok(CONTROL_TOWER.includes('onMounted(() => {'), '控制塔挂载时读一次清单');
  assert.ok(
    CONTROL_TOWER.includes("if (logModal.listState === 'unloaded') loadLogList();"),
    '已经读过一次就别再发一次'
  );
  assert.ok(
    CONTROL_TOWER.includes("if (logModal.listState === 'failed') return retryLogList();"),
    '失败时点那行要重试清单，而不是把用户拉进弹层'
  );
});
