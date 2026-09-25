// 日志面板布局共享 helper：侧栏宽度持久化（localStorage）+ 滚动期间才显
// 滚动条的解绑函数。两个 helper 都要在 LogModal 与 LogViewerWindow 共用，
// 任何重复实现都是埋雷（一边改了另一边不知情）。
import assert from 'node:assert/strict';
import test from 'node:test';

const core = {
  invoke() {
    return Promise.reject(new Error('not used'));
  },
  Channel: class {
    onmessage = null;
  },
};

globalThis.window = { __TAURI__: { core }, navigator: { userAgent: 'node' } };
globalThis.document = {
  hidden: false,
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  querySelector: () => null,
  addEventListener() {},
  removeEventListener() {},
  body: { classList: { toggle() {} } },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

// localStorage 用一个纯对象 stub 替掉 Node 默认（Node 没有这个全局），
// 让日志 helper 里的 `globalThis.localStorage` 引用能落到 stub 上。
// 每个 test 自管一份 stores，clear 用 Object.keys 遍历删除。
function makeLocalStorageStub() {
  const stores = {};
  return {
    getItem(key) {
      return Object.prototype.hasOwnProperty.call(stores, key) ? stores[key] : null;
    },
    setItem(key, value) {
      stores[key] = String(value);
    },
    removeItem(key) {
      delete stores[key];
    },
    clear() {
      for (const key of Object.keys(stores)) delete stores[key];
    },
    _stores: stores,
  };
}

test('loadSidebarWidth 落回默认值：key 缺失 / 值非法 / 越界', async () => {
  const ls = makeLocalStorageStub();
  globalThis.localStorage = ls;
  const { loadSidebarWidth } = await import('../src/logs.js');

  // 没有这个 key
  assert.equal(loadSidebarWidth('absent.modal', 'modal'), 220);
  // 值不是数字
  ls.setItem('bad.modal', 'not-a-number');
  assert.equal(loadSidebarWidth('bad.modal', 'modal'), 220);
  // 越界：负数 / 大数 / 字符串数字落在区间外都该被夹到 [min, max]
  ls.setItem('low.modal', '50');
  assert.equal(loadSidebarWidth('low.modal', 'modal'), 180);
  ls.setItem('high.modal', '9999');
  assert.equal(loadSidebarWidth('high.modal', 'modal'), 420);
  // 合法值原样返回
  ls.setItem('ok.modal', '280');
  assert.equal(loadSidebarWidth('ok.modal', 'modal'), 280);
});

test('loadSidebarWidth 两个变体（modal / window）的默认值与上下限不同', async () => {
  const ls = makeLocalStorageStub();
  globalThis.localStorage = ls;
  const { loadSidebarWidth } = await import('../src/logs.js');

  ls.clear();
  // 主面板弹窗偏窄，独立窗口偏宽
  assert.equal(loadSidebarWidth('missing.modal', 'modal'), 220);
  assert.equal(loadSidebarWidth('missing.window', 'window'), 240);
  assert.equal(loadSidebarWidth('clamp.window', 'window'), 240);
  // window 变体的上限是 560（独立窗口允许拉宽）
  ls.setItem('clamp.window', '99');
  assert.equal(loadSidebarWidth('clamp.window', 'window'), 180);
  ls.setItem('clamp.window', '9999');
  assert.equal(loadSidebarWidth('clamp.window', 'window'), 560);
});

test('saveSidebarWidth 写入后 loadSidebarWidth 能读到', async () => {
  const ls = makeLocalStorageStub();
  globalThis.localStorage = ls;
  const { loadSidebarWidth, saveSidebarWidth } = await import('../src/logs.js');

  ls.clear();
  saveSidebarWidth('round.modal', 312);
  assert.equal(loadSidebarWidth('round.modal', 'modal'), 312);
});

test('localStorage 不可用（隐私模式）时 helper 落回默认值而不抛', async () => {
  // 直接置空 globalThis.localStorage 模拟「不可用」
  globalThis.localStorage = null;
  const { loadSidebarWidth, saveSidebarWidth } = await import('../src/logs.js');

  assert.doesNotThrow(() => loadSidebarWidth('any', 'window'));
  assert.equal(loadSidebarWidth('any', 'window'), 240);
  assert.doesNotThrow(() => saveSidebarWidth('any', 300));
});

test('bindScrollAutoHide 加 / 移 .is-scrolling 类 + 监听器清理', async () => {
  const { bindScrollAutoHide } = await import('../src/logs.js');

  const events = [];
  const classList = new Set();
  const el = {
    classList: {
      add: (c) => classList.add(c),
      remove: (c) => classList.delete(c),
      contains: (c) => classList.has(c),
    },
    addEventListener(name, fn) {
      events.push({ type: 'add', name, fn });
    },
    removeEventListener(name, fn) {
      events.push({ type: 'remove', name, fn });
    },
  };

  const unbind = bindScrollAutoHide(el);
  // 绑定了一个 scroll 监听器
  assert.equal(events.length, 1);
  assert.equal(events[0].type, 'add');
  assert.equal(events[0].name, 'scroll');

  // 模拟一次滚动：加 .is-scrolling 类
  events[0].fn();
  assert.ok(classList.has('is-scrolling'), '滚动期间必须加上 .is-scrolling');

  // 解绑：移除监听器
  unbind();
  assert.equal(events.length, 2);
  assert.equal(events[1].type, 'remove');
  assert.equal(events[1].name, 'scroll');
});

test('bindScrollAutoHide 对 null / undefined 容错', async () => {
  const { bindScrollAutoHide } = await import('../src/logs.js');
  // 不抛异常，返回的解绑函数也可空跑
  assert.doesNotThrow(() => bindScrollAutoHide(null));
  assert.doesNotThrow(() => bindScrollAutoHide(undefined));
  const unbind = bindScrollAutoHide(null);
  assert.doesNotThrow(() => unbind());
});
