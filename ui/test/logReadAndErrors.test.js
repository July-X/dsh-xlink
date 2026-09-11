// 覆盖两处"静默失败"修复：
// - P2-38：日志读取改为按请求序号落地，重入不再被 withLoading 吞掉；
// - P2-40：渲染期错误必须进入可展示的兜底状态，而不是让面板永久空白。
import assert from 'node:assert/strict';
import test from 'node:test';

const deferred = () => {
  let resolve;
  const promise = new Promise((res) => {
    resolve = res;
  });
  return { promise, resolve };
};

const reads = [];
const core = {
  invoke(command, args) {
    if (command === 'read_log_file') {
      const entry = deferred();
      reads.push({ name: args.name, ...entry });
      return entry.promise;
    }
    if (command === 'list_log_files') return Promise.resolve([{ name: 'kernel.log', size: 1 }]);
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
  body: { classList: { toggle() {} } },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});

test('re-issuing a log read while one is in flight is not swallowed', async () => {
  const { logModal, loadActiveLog } = await import('../src/logs.js');
  logModal.activeName = 'kernel.log';

  const first = loadActiveLog();
  assert.equal(reads.length, 1, '第一次读取必须真的发起');

  // 大日志还没读完时用户再点「刷新」：旧实现按同一个 withLoading key 直接
  // 忽略重入，界面上什么都不发生。
  const second = loadActiveLog();
  assert.equal(reads.length, 2, '重入必须再发起一次读取，而不是被静默忽略');

  // 慢的旧响应后到，不能覆盖新结果。
  reads[1].resolve('newer');
  await second;
  reads[0].resolve('older');
  await first;
  assert.equal(logModal.content, 'newer', '只有最新一次请求的结果可以落地');
  assert.equal(logModal.loadingName, null, '结束后必须清掉加载态');
});

test('render errors are captured with a readable message and can be cleared', async () => {
  const { renderErrors, reportRenderError, clearRenderError } = await import('../src/errors.js');
  clearRenderError();
  assert.equal(renderErrors.message, '');

  const originalError = console.error;
  console.error = () => {};
  try {
    reportRenderError(new TypeError("Cannot read properties of undefined (reading 'port')"), 'render');
  } finally {
    console.error = originalError;
  }

  assert.match(renderErrors.message, /reading 'port'/, '兜底文案要带上原始错误');
  assert.match(renderErrors.message, /render/, '兜底文案要带上出错阶段');
  assert.equal(renderErrors.count, 1);

  clearRenderError();
  assert.equal(renderErrors.message, '');
  assert.equal(renderErrors.count, 0);
});
