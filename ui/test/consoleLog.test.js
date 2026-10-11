import assert from 'node:assert/strict';
import test from 'node:test';

import {
  CONSOLE_CAP_DEFAULT,
  ENTRY_MAX_CHARS,
  createConsoleRecorder,
  formatConsoleArgs,
  formatConsoleValue,
  installConsoleCapture,
} from '../src/shell/consoleLog.js';

// 控制台环形缓冲的纯逻辑：这是 agent 调试主壳唯一的 console 通道
// （embedded WKWebView 上没有 getLogs），三条纪律各自有判据：
//  ① 只旁观不吞（透传原方法、原参数）；
//  ② 内存有界（环形上限 + dropped 计数 + 重复折叠）；
//  ③ 记录绝不抛错（循环引用 / 超长 payload）。

/** 假 window：只登记监听器，手动派发——测 error / unhandledrejection 两条路。 */
function fakeWindow() {
  const listeners = new Map();
  return {
    listeners,
    addEventListener(type, handler) {
      listeners.set(type, handler);
    },
    dispatch(type, event) {
      listeners.get(type)?.(event);
    },
  };
}

/** 假 console：记录透传调用，供「只旁观不吞」断言。 */
function fakeConsole() {
  const calls = [];
  const fn = (level) => (...args) => calls.push({ level, args });
  return { calls, log: fn('log'), info: fn('info'), warn: fn('warn'), error: fn('error'), debug: fn('debug') };
}

test('按序记录层级与消息，dump 给副本且带容量与丢弃计数', () => {
  const rec = createConsoleRecorder({ now: () => 1234 });
  rec.record('warn', '第一条');
  rec.record('error', '第二条');

  const dump = rec.dump();
  assert.equal(dump.cap, CONSOLE_CAP_DEFAULT);
  assert.equal(dump.dropped, 0);
  assert.deepEqual(
    dump.items.map((item) => [item.level, item.message, item.count]),
    [
      ['warn', '第一条', 1],
      ['error', '第二条', 1],
    ],
  );
  assert.equal(dump.items[0].t, 1234, '时间戳来自注入的 now');

  // dump 是副本：改它不许影响内部状态。
  dump.items.length = 0;
  assert.equal(rec.items.length, 2);
});

test('环形上限：满了丢最旧并累计 dropped', () => {
  const rec = createConsoleRecorder({ cap: 3, now: () => 0 });
  for (let i = 1; i <= 5; i += 1) rec.record('log', `m${i}`);

  assert.equal(rec.dropped, 2, '5 条进 3 格应丢 2 条');
  assert.deepEqual(
    rec.dump().items.map((item) => item.message),
    ['m3', 'm4', 'm5'],
    '留下的是最新的',
  );
});

test('连续重复折叠成一条带 count，不占环形格', () => {
  const rec = createConsoleRecorder({ cap: 3, now: () => 0 });
  rec.record('error', '同样的错');
  rec.record('error', '同样的错');
  rec.record('error', '同样的错');

  const dump = rec.dump();
  assert.equal(dump.items.length, 1, '三条重复只占一格');
  assert.equal(dump.items[0].count, 3);
  assert.equal(dump.dropped, 0, '折叠不是丢弃');

  // 消息不同就照常入列：折叠只对「连续且完全相同」生效。
  rec.record('error', '另一个错');
  assert.equal(rec.dump().items.length, 2);
});

test('超长消息截断到上限；循环引用退回 String 不抛错', () => {
  assert.ok(formatConsoleValue('x'.repeat(ENTRY_MAX_CHARS + 100)).length <= ENTRY_MAX_CHARS);

  const circular = {};
  circular.self = circular;
  assert.doesNotThrow(() => formatConsoleValue(circular));
  assert.equal(typeof formatConsoleValue(circular), 'string');

  // 数组与对象走 JSON 形态，多条实参空格拼接。
  assert.equal(formatConsoleArgs([{ code: 7 }, 'done']), '{"code":7} done');
  assert.ok(formatConsoleArgs(['y'.repeat(ENTRY_MAX_CHARS + 5)]).length <= ENTRY_MAX_CHARS);
});

test('Error 实参必须带 message：JSON.stringify(new Error) 是空壳 {}，会吞掉诊断文本', () => {
  // 反向验背景：e2e 的未处理拒绝用例真抓到过——canary 文本消失，只剩 '{}'。
  assert.equal(formatConsoleValue(new Error('e2e-unhandled-canary')), 'Error: e2e-unhandled-canary');
  assert.equal(
    formatConsoleValue(Object.assign(new TypeError('bad'), { name: 'TypeError' })),
    'TypeError: bad',
  );
  const rec = createConsoleRecorder({ now: () => 0 });
  rec.record('error', `未处理的 Promise 拒绝：${formatConsoleValue(new Error('canary'))}`);
  assert.ok(rec.dump().items[0].message.includes('canary'));
});

test('「只旁观不吞」：包装后原方法被原样调用、参数一个不少', () => {
  const win = fakeWindow();
  const cons = fakeConsole();
  installConsoleCapture({ target: win, cons });

  cons.error(' boom ', { code: 42 });

  assert.equal(
    cons.calls.length,
    1,
    '透传失败（console 不再出字）是这条通道最坏的静默形态，必须钉住',
  );
  assert.deepEqual(cons.calls[0], { level: 'error', args: [' boom ', { code: 42 }] });

  // 同时也进了缓冲，且实参被拼成一条消息。**忠实不修饰**：实参里的空格
  // 原样保留（trim 会悄悄改写日志内容，诊断时不可接受）。
  const dump = win.__DSH_CONSOLE__.dump();
  assert.equal(dump.items[0].level, 'error');
  assert.equal(dump.items[0].message, ' boom  {"code":42}');
});

test('未捕获异常与未处理拒绝都进缓冲，异常事件带位置与栈', () => {
  const win = fakeWindow();
  const cons = fakeConsole();
  installConsoleCapture({ target: win, cons });

  win.dispatch('error', {
    message: 'Cannot read properties of undefined',
    filename: 'app://x/assets/index.js',
    lineno: 12,
    colno: 34,
    error: { stack: 'TypeError: ...\n    at render' },
  });
  win.dispatch('unhandledrejection', { reason: { code: 'BRIDGE_DOWN' } });

  const items = win.__DSH_CONSOLE__.dump().items;
  assert.equal(items[0].level, 'error');
  assert.ok(items[0].message.includes('Cannot read properties of undefined'));
  assert.ok(items[0].message.includes('index.js:12:34'), '位置信息要进消息');
  assert.ok(items[0].stack.includes('at render'), '栈要单独留档');
  assert.ok(items[1].message.includes('"code":"BRIDGE_DOWN"'));
});

test('重复安装幂等：同一 target 不叠加包装、返回同一记录器', () => {
  const win = fakeWindow();
  const cons = fakeConsole();
  const first = installConsoleCapture({ target: win, cons });
  const originalError = cons.error;
  const second = installConsoleCapture({ target: win, cons });

  assert.equal(first, second, '第二次安装返回既有记录器');
  assert.equal(cons.error, originalError, '不叠加第二层包装');

  cons.error('一次记录');
  assert.equal(win.__DSH_CONSOLE__.dump().items.length, 1, '不被记两次');
});

test('node 环境（无 addEventListener）安装安全返回 null，纯函数照常可用', () => {
  assert.equal(installConsoleCapture({ target: {}, cons: fakeConsole() }), null);
  assert.doesNotThrow(() => formatConsoleValue(undefined));
});
