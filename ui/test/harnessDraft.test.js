import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

// 注入脚本本身是 IIFE，没有导出；这里按 harnessHealth.test.js 的做法在 vm 里
// 造一个假页面跑它，断言的是**行为**（什么时候存草稿、什么时候写回去），不是源码
// 里有没有某个字符串。
const source = readFileSync(new URL('../../src-tauri/src/harness-draft.js', import.meta.url), 'utf8');

/** 造一个可编辑元素。kind 为 'lexical' 时没有 value、只有 innerText——内核的会话
 *  输入框是 Lexical 富文本编辑器（contenteditable），不是 textarea。 */
function makeEditable({ kind = 'lexical', text = '', id = 'composer', visible = true } = {}) {
  const listeners = {};
  const el = {
    tagName: kind === 'textarea' ? 'TEXTAREA' : 'DIV',
    id,
    value: kind === 'textarea' ? text : undefined,
    innerText: kind === 'lexical' ? text : undefined,
    textContent: kind === 'lexical' ? text : undefined,
    isContentEditable: kind === 'lexical',
    disabled: false,
    readOnly: false,
    focused: false,
    events: [],
    focus() {
      el.focused = true;
    },
    dispatchEvent(event) {
      el.events.push(event.type);
      listeners[`${id}:${event.type}`]?.(event);
      return true;
    },
    getBoundingClientRect() {
      return visible ? { width: 400, height: 60 } : { width: 0, height: 0 };
    },
  };
  return el;
}

function makeEnv({ editables = [], active = null, takeResult = null, href = 'http://127.0.0.1:3090/?session=abc' } = {}) {
  const timers = [];
  const handlers = {};
  const invocations = [];
  const execCalls = [];

  const fakeDocument = {
    activeElement: active,
    addEventListener(name, handler) {
      handlers[`document:${name}`] = handler;
    },
    querySelectorAll() {
      return editables;
    },
    execCommand(command, showUi, value) {
      execCalls.push({ command, value });
      return true;
    },
  };

  const fakeWindow = {
    top: null,
    self: null,
    location: { href },
    __TAURI__: {
      core: {
        invoke(command, args) {
          invocations.push({ command, args });
          if (command === 'take_harness_draft') return Promise.resolve(takeResult);
          return Promise.resolve();
        },
      },
    },
    addEventListener(name, handler) {
      handlers[`window:${name}`] = handler;
    },
    setInterval(fn) {
      timers.push(fn);
      return timers.length;
    },
    clearInterval() {},
    setTimeout(fn) {
      timers.push(fn);
      return timers.length;
    },
    clearTimeout() {},
  };
  fakeWindow.top = fakeWindow;
  fakeWindow.self = fakeWindow;

  runInNewContext(source, {
    window: fakeWindow,
    document: fakeDocument,
    Promise,
    Event: class {
      constructor(type) {
        this.type = type;
      }
    },
  });

  return { handlers, invocations, execCalls, timers, editables };
}

test('停止输入后把当前输入框内容交给壳', async () => {
  const composer = makeEditable({ text: '还没发出去的一段话' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  for (const fn of env.timers) fn();

  const stash = env.invocations.find((c) => c.command === 'stash_harness_draft');
  assert.ok(stash, '停顿之后必须记一次草稿');
  assert.equal(stash.args.text, '还没发出去的一段话');
  assert.equal(stash.args.href, 'http://127.0.0.1:3090/?session=abc');
});

test('输入框是空的就不记——磁盘上不该出现空草稿', () => {
  const composer = makeEditable({ text: '   \n  ' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  for (const fn of env.timers) fn();

  assert.equal(
    env.invocations.find((c) => c.command === 'stash_harness_draft'),
    undefined,
    '用户没在输入东西时不该写草稿',
  );
});

test('恢复用 execCommand 写回，而不是直接改 DOM', async () => {
  // execCommand 触发浏览器真实的输入路径，编辑器（Lexical）会接到这次输入并更新
  // 内部 state；直接写 textContent 只是「看起来有字」，一发送就没了——那比不恢复更糟。
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '上次没发出去的话' };
  const env = makeEnv({ editables: [composer], takeResult: draft });

  for (const fn of env.timers) fn();
  // 取草稿是异步的（IPC），写回发生在它 resolve 之后。
  await Promise.resolve();
  await Promise.resolve();

  const exec = env.execCalls.find((c) => c.command === 'insertText');
  assert.ok(exec, '必须走 insertText');
  assert.equal(exec.value, '上次没发出去的话');
  assert.ok(composer.events.includes('input'), '写完要派发 input，编辑器才知道内容变了');
  assert.ok(composer.focused, '写之前要聚焦，否则编辑器可能把这次输入归到别处');
});

test('地址变了就不写回去——宁可丢掉，也不能把话写到错的会话里', async () => {
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=OLD', text: '另一个会话里的话' };
  const env = makeEnv({ editables: [composer], takeResult: draft, href: 'http://127.0.0.1:3090/?session=NEW' });

  for (const fn of env.timers) fn();
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(env.execCalls.length, 0, '地址不一致时不该写入');
});

test('绝不覆盖用户自己新敲的内容', () => {
  const composer = makeEditable({ text: '用户回来之后自己又敲了一段' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '旧草稿' };
  const env = makeEnv({ editables: [composer], takeResult: draft });

  for (const fn of env.timers) fn();

  assert.equal(env.execCalls.length, 0, '输入框非空时不得写入');
  assert.equal(
    env.invocations.find((c) => c.command === 'take_harness_draft'),
    undefined,
    '连取都不该取：take 是读走即删的，取了写不进去就等于把草稿吞了',
  );
});

test('页面上还没有可写的地方时不取草稿（取走即删，顺序反了就吞掉了）', () => {
  const env = makeEnv({ editables: [], takeResult: { href: 'x', text: 'y' } });

  for (const fn of env.timers) fn();

  assert.equal(
    env.invocations.find((c) => c.command === 'take_harness_draft'),
    undefined,
    '必须先确认有可写的地方，再去壳里取',
  );
});

test('隐藏的输入框不算（折叠面板里的那个不该被当成 composer）', () => {
  const hidden = makeEditable({ text: '折叠面板里的', visible: false });
  const env = makeEnv({ editables: [hidden], active: hidden });

  env.handlers['document:input']();
  for (const fn of env.timers) fn();

  assert.equal(env.invocations.find((c) => c.command === 'stash_harness_draft'), undefined);
});
