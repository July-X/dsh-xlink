// 内核状态诊断的「最后读取时间」只在**真的读到**时前进（审查 R2-P1-04）。
//
// 过去 `refreshAll` 只有一句 catch + toast，调用方拿到的永远是 `undefined`，
// 于是 `loadKernelStatusDiagnosis` 无条件执行 `kernelReadAt = Date.now()`：
// 一次读取失败被显示成「刚刚读取成功」，同时把内核状态页用来提示「可能已
// 过期」的 `diagnosticStore.error` 也清掉了——过期提示与它唯一的锚点同时
// 消失，页面看上去一切正常。
//
// 之所以要写测试：这两处都是「失败时不报错」的类型，单测不钉住的话，
// 下一次重构把它改回无条件赋值不会有任何人发现。
import assert from 'node:assert/strict';
import test from 'node:test';

const calls = [];
// `get_status` 失败时置 true。
let statusFails = false;
let pluginFails = false;
let skillFails = false;

globalThis.window = {
  __TAURI__: {
    core: {
      invoke(command) {
        calls.push(command);
        if (command === 'get_status') {
          if (statusFails) return Promise.reject(new Error('status unreachable'));
          return Promise.resolve({ kernel: { running: false }, node: { ok: true } });
        }
        if (command === 'plugin_status') {
          return pluginFails
            ? Promise.reject(new Error('plugin unreachable'))
            : Promise.resolve({ plugins: [] });
        }
        if (command === 'skill_status') {
          return skillFails
            ? Promise.reject(new Error('skill unreachable'))
            : Promise.resolve({ skills: [] });
        }
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
const body = makeElement();
let bodyAppends = 0;
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

const { refreshAll, store } = await import('../src/store.js');
const { loadKernelStatusDiagnosis } = await import('../src/diagnostics/diagnostic-actions.js');
const { diagnosticStore } = await import('../src/diagnostics/diagnostics.js');

function reset() {
  calls.length = 0;
  statusFails = false;
  pluginFails = false;
  skillFails = false;
  store.view = { kernel: {}, node: {} };
}

test('全量刷新逐个数据源交回成败，不是一律 undefined', async () => {
  reset();
  const ok = await refreshAll();
  assert.equal(ok.status, true, 'get_status 成功要如实报出来');
  assert.equal(ok.plugins, true);
  assert.equal(ok.skills, true);

  reset();
  statusFails = true;
  pluginFails = true;
  const bad = await refreshAll();
  assert.equal(bad.status, false, 'get_status 失败必须报出来，不能吞掉');
  assert.equal(bad.plugins, false, '插件状态失败要能与其他数据源区分开');
  assert.equal(bad.skills, true, '技能正常时不能被连坐');
});

test('get_status 失败时「最后读取时间」不前进，错误提示保留', async () => {
  reset();
  await loadKernelStatusDiagnosis();
  const goodAt = diagnosticStore.kernelReadAt;
  assert.ok(goodAt > 0, '成功读过一次就该有锚点');

  statusFails = true;
  await loadKernelStatusDiagnosis();
  assert.equal(
    diagnosticStore.kernelReadAt,
    goodAt,
    '读取失败时那一栏必须继续显示上一次成功读到的时刻'
  );
  assert.ok(
    diagnosticStore.error,
    '失败要留下可显示的提示，否则「可能已过期」既没有标记也没有锚点'
  );
});

test('插件 / 技能失败不连坐内核：状态栏照常前进', async () => {
  reset();
  // 先塞一个明显早于「此刻」的锚点，这样「有没有前进」不依赖两次
  // `Date.now()` 恰好落在同一毫秒（那种断言会偶发红，且红得莫名其妙）。
  diagnosticStore.kernelReadAt = 1;
  pluginFails = true;
  skillFails = true;
  await loadKernelStatusDiagnosis();
  assert.ok(
    diagnosticStore.kernelReadAt > 1,
    '内核状态这一路成功了，时间戳就该前进'
  );
  assert.equal(
    diagnosticStore.error,
    '',
    '插件失败不该被说成内核状态读取失败——那是两个健康项各自的不可用状态'
  );
});
