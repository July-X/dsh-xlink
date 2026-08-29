import assert from 'node:assert/strict';
import test from 'node:test';

const flushMicrotasks = () => new Promise((resolve) => queueMicrotask(resolve));

globalThis.window = {
  __TAURI__: {
    core: {
      Channel: class {
        onmessage = null;
      },
    },
  },
  navigator: { userAgent: 'node' },
};
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'node' },
});
globalThis.document = {
  createElement: () => ({}),
  createElementNS: () => ({}),
  createTextNode: () => ({}),
  createComment: () => ({}),
  body: { classList: { toggle() {} } },
};

test('progress log bounds individual lines and total retained text', async () => {
  const { progress } = await import('../src/progress.js');
  progress.resetLog();
  progress.appendLog('\x1b[31m' + 'x'.repeat(20 * 1024) + '\x1b[0m');
  for (let index = 0; index < 40; index += 1) {
    progress.appendLog('line-' + index + ':' + 'y'.repeat(20 * 1024));
  }
  await flushMicrotasks();

  assert.ok(progress.logText.length <= 512 * 1024);
  const lines = progress.logText.split('\n');
  assert.ok(lines.every((line) => line.length <= 16 * 1024));
  assert.match(lines[0], /line-|行已截断/);
  assert.doesNotMatch(progress.logText, /\x1B/);
  progress.close();
  await flushMicrotasks();
  assert.equal(progress.logText, '');
});
