import assert from 'node:assert/strict';
import test from 'node:test';

// dev 构建专属界面的统一开关（store.devUi）：release 预览必须把「模拟一次任务
// 完成」、版本号的「（dev）」后缀和调试浮按钮一起收起来，否则预览到的只是半个
// 正式版——这条约束分散在三个组件里，只能靠状态层钉住。
const bodyClasses = new Set(['dev-build']);

const statusFor = (devBuild) => ({
  kernel: { running: false, active: '0.1.3', active_installed: true, installed: [] },
  node: { ok: true, path: '/node', version: '25.9.0' },
  settings: { port: 3090, profile: 'web' },
  shell_version: '0.1.3-rc.3',
  dev_build: devBuild,
  quarantined: [],
  last_incident: null,
  official_chat_open: false,
});

let nextStatus = statusFor(true);
const core = {
  invoke(command) {
    if (command === 'get_status') return Promise.resolve(nextStatus);
    if (command === 'plugin_status' || command === 'skill_status') return Promise.resolve({ rows: [] });
    throw new Error(`unexpected command: ${command}`);
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
  body: {
    classList: {
      // 首帧兜底读 contains：store.js 用它把 HMR 后残留的 dev 类接回来。
      contains: (name) => bodyClasses.has(name),
      toggle(name, on) {
        const next = on === undefined ? !bodyClasses.has(name) : !!on;
        if (next) bodyClasses.add(name);
        else bodyClasses.delete(name);
      },
    },
  },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
globalThis.requestAnimationFrame = (callback) => {
  callback();
  return 1;
};
globalThis.cancelAnimationFrame = () => {};

test('dev 构建：devUi 首帧就从 body 类恢复，release 预览把它收起来', async () => {
  const { store, refreshAll, setReleasePreview } = await import('../src/store.js');

  // 首帧兜底：body 上已有 dev-build（HMR 场景），导入时 store.devUi 就是 true，
  // 不需要等下一次 2.5s 轮询，dev 专属入口不会先隐藏再闪回来。
  assert.equal(store.devUi, true, 'body 上的 dev-build 类要在首帧就生效');

  nextStatus = statusFor(true);
  await refreshAll();
  assert.equal(store.devUi, true);

  setReleasePreview(true);
  assert.equal(store.releasePreview, true);
  assert.equal(store.devUi, false, 'release 预览里所有 dev 专属入口都要消失');
  assert.equal(bodyClasses.has('rel-build'), true, '背景切到 release 绿渐变');
  assert.equal(bodyClasses.has('dev-build'), false);

  setReleasePreview(false);
  assert.equal(store.devUi, true, '退出预览后 dev 入口回来');
  assert.equal(bodyClasses.has('dev-build'), true);
  assert.equal(bodyClasses.has('rel-build'), false);

  // 预览覆盖不能被下一次状态轮询冲掉：applyBuildClass 每轮都会跑，但它读的是
  // store.releasePreview，而不是重新按 dev_build 覆盖。
  setReleasePreview(true);
  await refreshAll();
  assert.equal(store.devUi, false, '轮询回来后仍然是 release 预览');
  assert.equal(bodyClasses.has('rel-build'), true);
  assert.equal(bodyClasses.has('dev-build'), false);

  // 交回 dev 态，后面的用例从干净状态开始。
  setReleasePreview(false);
  assert.equal(store.devUi, true);
});

test('正式版：devUi 恒为 false，release 预览开关是 no-op', async () => {
  const { store, refreshAll, setReleasePreview } = await import('../src/store.js');

  nextStatus = statusFor(false);
  await refreshAll();
  assert.equal(store.devUi, false);

  setReleasePreview(true);
  assert.equal(store.releasePreview, false, '正式版不得被切进调试态');
  assert.equal(store.devUi, false);
  assert.equal(bodyClasses.has('rel-build'), true);
  assert.equal(bodyClasses.has('dev-build'), false);
});
