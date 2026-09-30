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

function makeEnv({
  editables = [],
  active = null,
  takeResult = null,
  href = 'http://127.0.0.1:3090/?session=abc',
  execCommandWorks = true,
  execCommandSilent = false,
  tauriReadyAfter = 0,
} = {}) {
  const timers = [];
  const handlers = {};
  const invocations = [];
  const execCalls = [];
  // `__TAURI__` 就绪时刻的探针：0 = 一开始就在；N = 第 N 次查它时才出现。
  let tauriLookups = 0;

  const fakeDocument = {
    activeElement: active,
    addEventListener(name, handler) {
      handlers[`document:${name}`] = handler;
    },
    querySelectorAll() {
      return editables;
    },
    // insertText 成功时按浏览器的行为把文字放进元素——真实编辑器随后会更新
    // 自己的内部 state。这里照做，脚本的「读回来核对」才有东西可核对。
    execCommand(command, showUi, value) {
      execCalls.push({ command, value });
      if (command !== 'insertText') return false;
      if (execCommandWorks === false) return false;
      // execCommand 报成功但编辑器其实吞掉了（beforeinput 被 preventDefault 却
      // 没有真的应用）——那正是「读回来核对」要抓的形状。
      if (execCommandSilent === true) return true;
      const target = editables.find((el) => el.focused) || editables[0];
      if (target) {
        if (target.tagName === 'TEXTAREA' || target.tagName === 'INPUT') target.value = value;
        else target.innerText = value;
      }
      return true;
    },
  };

  const fakeWindow = {
    top: null,
    self: null,
    location: { href },
    // tauriReadyAfter > 0 时先不给 __TAURI__，模拟「注入脚本跑得比 Tauri 的桥早」。
    get __TAURI__() {
      tauriLookups += 1;
      if (tauriLookups <= tauriReadyAfter) return undefined;
      return {
        core: {
          invoke(command, args) {
            invocations.push({ command, args });
            if (command === 'take_harness_draft') return Promise.resolve(takeResult);
            return Promise.resolve();
          },
        },
      };
    },
    addEventListener(name, handler) {
      handlers[`window:${name}`] = handler;
    },
    // 定时器按 kind 记：脚本用 setTimeout 做「停顿后记草稿」与退避重试，用
    // setInterval 做「轮询等 composer 出现」。不分开的話「一次引爆全部」会把
    // 「__TAURI__ 还没就绪」这个前提自己引爆掉——那种测试看着在跑，其实什么也没验。
    setInterval(fn) {
      timers.push({ kind: 'interval', fn });
      return timers.length;
    },
    clearInterval() {},
    setTimeout(fn) {
      timers.push({ kind: 'timeout', fn });
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

  /** 烧掉当前排着的 timeout（按先后），一个一个来。interval 留给别的用例。 */
  const fireTimeouts = () => {
    const pending = timers.filter((t) => t.kind === 'timeout');
    timers.length = 0;
    pending.forEach((t) => t.fn());
  };
  /** 一次性烧掉所有定时器（不关心顺序的用例用）。 */
  const fireAll = () => {
    const pending = timers.slice();
    timers.length = 0;
    pending.forEach((t) => t.fn());
  };

  return { handlers, invocations, execCalls, timers, editables, fireTimeouts, fireAll };
}

test('停止输入后把当前输入框内容交给壳', async () => {
  const composer = makeEditable({ text: '还没发出去的一段话' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  env.fireAll();

  const stash = env.invocations.find((c) => c.command === 'stash_harness_draft');
  assert.ok(stash, '停顿之后必须记一次草稿');
  assert.equal(stash.args.text, '还没发出去的一段话');
  assert.equal(stash.args.href, 'http://127.0.0.1:3090/?session=abc');
});

test('输入框是空的就不记——磁盘上不该出现空草稿', () => {
  const composer = makeEditable({ text: '   \n  ' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  env.fireAll();

  assert.equal(
    env.invocations.find((c) => c.command === 'stash_harness_draft'),
    undefined,
    '用户没在输入东西时不该写草稿',
  );
});

test('恢复用 execCommand 写回，而不是直接改 DOM', async () => {
  // execCommand 触发浏览器真实的输入路径。已装内核 0.2.0-rc.2 的
  // dsh-client-ui-conversation/lib/client.js 里，Lexical 在编辑器根上注册了
  // beforeinput 处理器并按 inputType 分支（含 insertText）——execCommand 走的正是
  // 真实打字走的那一条。直接改 textContent 只会「看起来有字」，一发送就没了。
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '上次没发出去的话' };
  const env = makeEnv({ editables: [composer], takeResult: draft });

  env.fireAll();
  // 取草稿是异步的（IPC），写回发生在它 resolve 之后。
  await Promise.resolve();
  await Promise.resolve();

  const exec = env.execCalls.find((c) => c.command === 'insertText');
  assert.ok(exec, '必须走 insertText');
  assert.equal(exec.value, '上次没发出去的话');
  assert.ok(composer.focused, '写之前要聚焦，否则编辑器可能把这次输入归到别处');
  // contenteditable 上**不**再补派 `input`：编辑器走的是 beforeinput，补一个合成
  // `input` 既多余又会让人以为「派发 input 就能让编辑器认下这段字」——那恰恰是
  // 之前那条假恢复的错处。真正确认「进去了」的是读回来核对。
  assert.equal(composer.innerText, '上次没发出去的话', '写进去之后元素里得有那段字');
});

test('execCommand 被拒时把草稿放回去——恢复失败不能等于内容丢失', async () => {
  // `take` 是读走即删的。写不进去又不还回去，用户就为了这件事抱怨过一次了。
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '上次没发出去的话' };
  const env = makeEnv({ editables: [composer], takeResult: draft, execCommandWorks: false });

  env.fireAll();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();

  const back = env.invocations.filter((c) => c.command === 'stash_harness_draft');
  assert.equal(back.length, 1, '写不进去必须把草稿放回去，等下一次页面加载再试');
  assert.equal(back[0].args.text, '上次没发出去的话');
  assert.equal(
    String(composer.innerText || '').trim(),
    '',
    '不许留下「看起来有字」的假恢复——那比不恢复更糟：用户会以为内容还在，一发送就没了',
  );
});

test('execCommand 报成功但编辑器其实吞掉了，同样要当作失败', async () => {
  // beforeinput 被 preventDefault 却没有真的应用——返回 true，元素里仍然是空的。
  // 只看返回值会把这次恢复判成成功，于是用户看着空输入框以为内容没了。
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '上次没发出去的话' };
  const env = makeEnv({ editables: [composer], takeResult: draft, execCommandSilent: true });

  env.fireAll();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();

  const back = env.invocations.filter((c) => c.command === 'stash_harness_draft');
  assert.equal(back.length, 1, '读回来核对没过就要当作失败并把草稿放回去');
});

test('地址变了就不写回去——宁可丢掉，也不能把话写到错的会话里', async () => {
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=OLD', text: '另一个会话里的话' };
  const env = makeEnv({ editables: [composer], takeResult: draft, href: 'http://127.0.0.1:3090/?session=NEW' });

  env.fireAll();
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(env.execCalls.length, 0, '地址不一致时不该写入');
});

test('绝不覆盖用户自己新敲的内容', () => {
  const composer = makeEditable({ text: '用户回来之后自己又敲了一段' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '旧草稿' };
  const env = makeEnv({ editables: [composer], takeResult: draft });

  env.fireAll();

  assert.equal(env.execCalls.length, 0, '输入框非空时不得写入');
  assert.equal(
    env.invocations.find((c) => c.command === 'take_harness_draft'),
    undefined,
    '连取都不该取：take 是读走即删的，取了写不进去就等于把草稿吞了',
  );
});

test('页面上还没有可写的地方时不取草稿（取走即删，顺序反了就吞掉了）', () => {
  const env = makeEnv({ editables: [], takeResult: { href: 'x', text: 'y' } });

  env.fireAll();

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
  env.fireAll();

  assert.equal(env.invocations.find((c) => c.command === 'stash_harness_draft'), undefined);
});

test('__TAURI__ 还没就绪时要重试，而不是静默放弃这一次的草稿', () => {
  // harness-health.js 为此专门写过 scheduleReportRetry——那份代码是踩过的证据。
  // 拿不到就当没事，等于这一次的草稿静默不落盘，正是用户抱怨的那件事。
  const composer = makeEditable({ text: '正在输入的一段话' });
  const env = makeEnv({ editables: [composer], active: composer, tauriReadyAfter: 2 });

  env.handlers['document:input']();
  env.fireAll();
  assert.equal(
    env.invocations.filter((c) => c.command === 'stash_harness_draft').length,
    0,
    '__TAURI__ 没就绪时不该硬发 invoke',
  );
  assert.ok(env.timers.length > 0, '必须排一次重试，而不是就此放弃');

  // 重试触发两轮之后 __TAURI__ 就绪 —— 这一次必须真的记下来。
  env.fireTimeouts();
  env.fireTimeouts();
  const stashed = env.invocations.filter((c) => c.command === 'stash_harness_draft');
  assert.equal(stashed.length, 1, '重试到 __TAURI__ 就绪后必须记下草稿');
  assert.equal(stashed[0].args.text, '正在输入的一段话');
});
