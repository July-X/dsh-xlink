import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { shouldReportBlankHarness } from '../src/harnessHealth.js';

const probeSource = readFileSync(new URL('../../src-tauri/src/harness-health.js', import.meta.url), 'utf8');

test('reports only an empty harness without meaningful rendered content', () => {
  assert.equal(shouldReportBlankHarness({}), true);
  assert.equal(shouldReportBlankHarness({ text: 'Loading' }), false);
  assert.equal(shouldReportBlankHarness({ meaningful: true }), false);
  assert.equal(shouldReportBlankHarness({ childCount: 1 }), false);
});

test('health probe sends the structured runtime report accepted by Rust', async () => {
  const handlers = {};
  const calls = [];
  const fakeWindow = {
    top: null,
    self: null,
    location: { href: 'http://127.0.0.1:3090' },
    __TAURI__: {
      core: {
        invoke(command, args) {
          calls.push({ command, args });
          return Promise.resolve({});
        },
      },
    },
    addEventListener(name, handler) {
      handlers[name] = handler;
    },
    setTimeout() {
      return 1;
    },
  };
  fakeWindow.top = fakeWindow;
  fakeWindow.self = fakeWindow;
  const fakeDocument = {
    readyState: 'loading',
    addEventListener(name, handler) {
      handlers['document:' + name] = handler;
    },
  };

  runInNewContext(probeSource, { window: fakeWindow, document: fakeDocument, Promise });
  handlers.error({
    message: '组件初始化失败',
    error: { stack: 'Error: 组件初始化失败\\n at mount (http://127.0.0.1:3090/plugins/ghost/main.js:1:1)' },
  });
  assert.equal(calls.length, 1);
  await Promise.resolve();

  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, 'report_harness_fault');
  assert.deepEqual({ ...calls[0].args }, {
    kind: 'runtime-error',
    message: '组件初始化失败',
    stack: 'Error: 组件初始化失败\\n at mount (http://127.0.0.1:3090/plugins/ghost/main.js:1:1)',
    pageUrl: 'http://127.0.0.1:3090',
  });
});

test('health probe retries a failed IPC delivery and stops after success', async () => {
  const handlers = {};
  const timers = [];
  const calls = [];
  let attempt = 0;
  const fakeWindow = {
    top: null,
    self: null,
    location: { href: 'http://127.0.0.1:3090' },
    __TAURI__: {
      core: {
        invoke(command, args) {
          calls.push({ command, args });
          attempt += 1;
          return attempt === 1 ? Promise.reject(new Error('暂时不可用')) : Promise.resolve({});
        },
      },
    },
    addEventListener(name, handler) {
      handlers[name] = handler;
    },
    setTimeout(handler, delay) {
      timers.push({ handler, delay });
      return timers.length;
    },
  };
  fakeWindow.top = fakeWindow;
  fakeWindow.self = fakeWindow;
  const fakeDocument = {
    readyState: 'loading',
    addEventListener(name, handler) {
      handlers['document:' + name] = handler;
    },
  };

  runInNewContext(probeSource, { window: fakeWindow, document: fakeDocument, Promise });
  handlers.unhandledrejection({ reason: new Error('内核响应异常') });
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(calls.length, 1);
  assert.equal(timers.length, 1);
  assert.equal(timers[0].delay, 500);

  timers.shift().handler();
  await Promise.resolve();
  assert.equal(calls.length, 2);
  handlers.unhandledrejection({ reason: new Error('再次异常') });
  assert.equal(calls.length, 2);
});

test('health probe reports a blank workbench after the second check', async () => {
  const handlers = {};
  const timers = [];
  const calls = [];
  const fakeWindow = {
    top: null,
    self: null,
    location: { href: 'http://127.0.0.1:3090' },
    __TAURI__: {
      core: {
        invoke(command, args) {
          calls.push({ command, args });
          return Promise.resolve({});
        },
      },
    },
    addEventListener(name, handler) {
      handlers[name] = handler;
    },
    setTimeout(handler, delay) {
      timers.push({ handler, delay });
      return timers.length;
    },
    getComputedStyle() {
      return { display: 'block', visibility: 'visible', opacity: '1' };
    },
  };
  fakeWindow.top = fakeWindow;
  fakeWindow.self = fakeWindow;
  const fakeDocument = {
    readyState: 'complete',
    body: {
      innerText: '',
      querySelectorAll() {
        return [];
      },
    },
  };

  runInNewContext(probeSource, { window: fakeWindow, document: fakeDocument, Promise });
  assert.equal(timers.length, 1);
  timers.shift().handler();
  assert.equal(calls.length, 0);
  assert.equal(timers.length, 1);
  timers.shift().handler();
  await Promise.resolve();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].args.kind, 'blank');
});

test('health probe source keeps its command contract and retry guards', () => {
  assert.match(probeSource, /report_harness_fault/);
  assert.match(probeSource, /kind:/);
  assert.match(probeSource, /message:/);
  assert.match(probeSource, /stack:/);
  assert.match(probeSource, /pageUrl:/);
  assert.match(probeSource, /unhandledrejection/);
  assert.match(probeSource, /runtime-error/);
  assert.match(probeSource, /reported = true/);
  assert.match(probeSource, /maxReportAttempts/);
  assert.match(probeSource, /reportInFlight/);
});
