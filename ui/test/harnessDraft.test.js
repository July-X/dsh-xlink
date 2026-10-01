import assert from 'node:assert/strict';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { readShellSource } from '../../scripts/lib/shell-source.mjs';

// 注入脚本本身是 IIFE，没有导出；这里按 harnessHealth.test.js 的做法在 vm 里
// 造一个假页面跑它，断言的是**行为**（什么时候存草稿、什么时候写回去），不是源码
// 里有没有某个字符串。
const source = readShellSource('harness-draft.js');

/** 造一个可编辑元素。kind 为 'lexical' 时没有 value、只有 innerText——内核的会话
 *  输入框是 Lexical 富文本编辑器（contenteditable），不是 textarea。 */
function makeEditable({ kind = 'lexical', text = '', id = 'composer', visible = true } = {}) {
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
    // 页面挂在这个元素上的事件处理器。假内核就是靠它「接住」一次粘贴的。
    on: {},
    focus() {
      el.focused = true;
    },
    dispatchEvent(event) {
      el.events.push(event.type);
      el.on[event.type]?.(event);
      return true;
    },
    getBoundingClientRect() {
      return visible ? { width: 400, height: 60 } : { width: 0, height: 0 };
    },
  };
  return el;
}

/** 造一张附件轨道上的缩略图。src 必须是 `blob:` —— 那正是「还没发出去的草稿图」
 *  与「会话历史里的图」的分别（后者走 http(s) 地址）。 */
function makeImage({ name = 'probe.png', visible = true } = {}) {
  return {
    tagName: 'IMG',
    src: `blob:fake-${name}-${Math.random().toString(16).slice(2)}`,
    alt: name,
    getBoundingClientRect() {
      return visible ? { width: 64, height: 64 } : { width: 0, height: 0 };
    },
  };
}

function makeEnv({
  editables = [],
  active = null,
  takeResult = null,
  href = 'http://127.0.0.1:3090/?session=abc',
  execCommandWorks = true,
  execCommandSilent = false,
  tauriReadyAfter = 0,
  hasCard = true,
  images = [],
  pasteAccepts = true,
  fetchWorks = true,
} = {}) {
  const timers = [];
  const intervals = [];
  const handlers = {};
  const invocations = [];
  const execCalls = [];
  const pastes = [];
  // `__TAURI__` 就绪时刻的探针：0 = 一开始就在；N = 第 N 次查它时才出现。
  let tauriLookups = 0;

  // composer 卡片（`[data-composer-card]`）：编辑器、附件轨道、文件选择框都在它里面。
  const card = hasCard
    ? {
        querySelectorAll(selector) {
          return selector === 'img[src^="blob:"]' ? images : [];
        },
      }
    : null;

  const fakeDocument = {
    activeElement: active,
    addEventListener(name, handler) {
      handlers[`document:${name}`] = handler;
    },
    querySelectorAll() {
      return editables;
    },
    querySelector(selector) {
      return selector === '[data-composer-card]' ? card : null;
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
    // blob: URL 只有在这个页面里读得到字节——页面一换掉就没了，所以采集必须趁
    // 页面还活着的时候做完。给一份固定的字节，方便断言。
    fetch(url) {
      if (fetchWorks === false) return Promise.reject(new Error('blob read failed'));
      return Promise.resolve({
        ok: true,
        arrayBuffer() {
          return Promise.resolve(new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]).buffer);
        },
        headers: { get: () => 'image/png' },
      });
    },
    btoa,
    atob,
    File: class {
      constructor(parts, name, options) {
        this.parts = parts;
        this.name = name;
        this.type = options?.type;
      }
    },
    DataTransfer: class {
      constructor() {
        // 真 DataTransfer 的 items 既可迭代又有 add —— 缺一个就会让「派发 paste」
        //  那条路在测试里假失败。
        const list = [];
        list.add = (file) => list.push({ kind: 'file', file });
        this.items = list;
      }
    },
    ClipboardEvent: class {
      constructor(type, init) {
        this.type = type;
        this.clipboardData = init.clipboardData;
        this.defaultPrevented = false;
      }
    },
    // 定时器按 kind 记：脚本用 setTimeout 做「停顿后记草稿」与退避重试，用
    // setInterval 做「轮询等 composer 出现」与「注入后核对」。不分开的話「一次引爆全部」
    // 会把「__TAURI__ 还没就绪」这个前提自己引爆掉——那种测试看着在跑，其实什么也没验。
    setInterval(fn) {
      const entry = { kind: 'interval', fn, id: timers.length + 1, cleared: false };
      timers.push(entry);
      intervals.push(entry);
      return entry.id;
    },
    // 真 clearInterval 真的会停：不然「注入后核对」那一圈会一遍遍回调 done()，
    // 把「只该发生一次」的放回 / 清盘变成十几次——那种测试看着在跑，其实什么也没验。
    clearInterval(id) {
      const entry = timers.find((t) => t.id === id);
      if (entry) entry.cleared = true;
    },
    setTimeout(fn) {
      timers.push({ kind: 'timeout', fn });
      return timers.length;
    },
    clearTimeout() {},
  };
  fakeWindow.top = fakeWindow;
  fakeWindow.self = fakeWindow;
  // 假内核：编辑器上的 paste 处理器把 File 变成轨道上的一张缩略图——这正是真机上
  // Lexical PASTE_COMMAND → intakeFiles → createDrafts 的结果。
  if (editables.length) {
    editables[0].on.paste = (event) => {
      pastes.push(event);
      event.defaultPrevented = true;
      if (!pasteAccepts) return;
      for (const item of event.clipboardData.items) {
        images.push(makeImage({ name: item.file.name }));
      }
    };
  }

  runInNewContext(source, {
    window: fakeWindow,
    document: fakeDocument,
    Promise,
    btoa,
    atob,
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
    pending.filter((t) => !t.cleared).forEach((t) => t.fn());
  };
  /** 让每个**没被清掉**的 interval 再走一拍，**不消费**。`fireAll` 是「烧掉排着的」，
   *  只能用来走第一拍——而「发送之后」发生的事在第二拍上（监视是轮询的）。 */
  const tickIntervals = () => {
    intervals.filter((t) => !t.cleared).forEach((t) => t.fn());
  };

  /** 切换焦点。发送后内核清空编辑器并把焦点带走，此时 `currentText()` 走的是
   *  「取所有可见可编辑元素里最长的那个」那条退路——而它正是「是否已发送」的
   *  判据，所以这个切换是复现的前提，不是可选的装饰。 */
  const setActive = (element) => {
    fakeDocument.activeElement = element;
  };

  return { handlers, invocations, execCalls, timers, editables, images, pastes, setActive, fireTimeouts, fireAll, tickIntervals };
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

/* ── 「已经发出去的消息不该再回来」这一组 ──────────────────────────────────
 *
 * 2026-09-30 用户原话：「对话回填功能，需要注意，已经发送的消息，下次打开工作台
 * 不应该再次填充在输入区域」。根因是草稿只有「写」没有「作废」：发送时编辑器被
 * 程序化清空（**不派发 input**），盘上那一份就一直留着，而恢复那侧看到的是一段
 * 「新鲜的草稿」——它无从知道那句话已经在会话里了。 */

test('用户发出去了：输入框一空就把盘上那份作废', () => {
  const composer = makeEditable({ text: '已经发出去的一句话' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  env.fireAll();
  assert.equal(
    env.invocations.filter((c) => c.command === 'stash_harness_draft').length,
    1,
    '先决条件：停顿之后确实记下了草稿，否则这个用例什么也没验',
  );

  // 按下发送：Lexical 是**程序化**清空编辑器的，没有 input 事件可听。真实编辑器
  // 是把文本节点删掉，所以 innerText 与 textContent 一起清（textOf 会回退到后者）。
  composer.innerText = '';
  composer.textContent = '';
  env.tickIntervals();

  const cleared = env.invocations.filter((c) => c.command === 'clear_harness_draft');
  assert.equal(cleared.length, 1, '必须清掉：留着它，下次打开工作台它就自己坐回输入框');
});

test('页面上一个可编辑元素都没有时**不**作废', () => {
  // 黑屏那一刻很可能正是 composer 消失的时候（作用域装配失败 → 整棵树被摘掉）。
  // 这时的「空」不是「用户把话发出去了」，在这里清盘等于亲手删掉他唯一没发出去的
  // 那一段——比多留一份已发送的消息糟糕得多。
  const composer = makeEditable({ text: '还没发出去的一段话' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  env.fireAll();
  assert.equal(env.invocations.filter((c) => c.command === 'stash_harness_draft').length, 1);

  env.editables.length = 0; // 页面塌了：composer 整个不在
  env.tickIntervals();

  assert.equal(
    env.invocations.filter((c) => c.command === 'clear_harness_draft').length,
    0,
    '页面坏掉时的「空」不是「发出去了」，草稿得留着等页面回来',
  );
});

test('本页没存过东西时不作废——恢复失败放回去的那份是唯一一份', () => {
  // 写不进去时脚本会把草稿放回盘上等下一次页面加载；那一刻本页从未存过东西，
  // 而输入框正是空的。监视若在这里清盘，删掉的就是用户唯一的一份。
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '上次没发出去的话' };
  const env = makeEnv({ editables: [], takeResult: draft, execCommandWorks: false });

  env.fireAll();
  env.editables.push(composer); // composer 这时才装配出来
  env.tickIntervals();
  return Promise.resolve()
    .then(() => Promise.resolve())
    .then(() => {
      assert.equal(
        env.invocations.filter((c) => c.command === 'stash_harness_draft').length,
        1,
        '写不进去必须先放回去',
      );
      env.tickIntervals();
      assert.equal(
        env.invocations.filter((c) => c.command === 'clear_harness_draft').length,
        0,
        '本页没存过东西就不该清——那一份是恢复失败放回去的唯一一份',
      );
    });
});

test('输入框一直有字时监视不重复写盘', () => {
  // 监视每秒钟走一次。要是每次都写，输入框里停着一段不打字的草稿就会变成每秒一
  // 次磁盘写，还把过期时间一直往后推——两个都不该发生。
  const composer = makeEditable({ text: '停在这里的一段话' });
  const env = makeEnv({ editables: [composer], active: composer });

  env.handlers['document:input']();
  env.fireAll();
  assert.equal(env.invocations.filter((c) => c.command === 'stash_harness_draft').length, 1);

  env.tickIntervals();
  env.tickIntervals();
  env.tickIntervals();

  assert.equal(
    env.invocations.filter((c) => c.command === 'stash_harness_draft').length,
    1,
    '内容没变就不该再写盘',
  );
});

/* ── 图片草稿这一组 ────────────────────────────────────────────────────────
 *
 * 图片在核心里不是编辑器的一部分，而是 composer 卡片上一排 `blob:` 缩略图，字节只
 * 活在那个页面里。所以：① 采集只能在页面还活着时读 blob；② 恢复只能走 paste
 * （真机实测：文件选择框那条路被 React 的 value tracker 吃掉）；③ 写回去与否要
 * **核对**轨道上真的多了图，不能以「事件派发成功」当作恢复成功。 */

const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

test('粘贴进来的截图会被读成字节交给壳', async () => {
  const composer = makeEditable({ text: '看看这张' });
  const env = makeEnv({
    editables: [composer],
    active: composer,
    images: [makeImage({ name: 'shot.png' })],
  });

  env.handlers['document:input']();
  env.fireAll();
  await settled();
  await settled();

  const stash = env.invocations.find((c) => c.command === 'stash_harness_draft');
  assert.ok(stash, '有图就该记一次草稿');
  assert.equal(stash.args.images.length, 1, '一张图');
  assert.equal(stash.args.images[0].name, 'shot.png');
  assert.equal(stash.args.images[0].mime, 'image/png');
  assert.equal(
    atob(stash.args.images[0].data),
    String.fromCharCode(...new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])),
    '必须是原始字节：img.src 是浏览器转码过的显示图，存它等于给用户换了一张图',
  );
});

test('只有图没打字时也要存（只发图是最容易漏的那一种）', async () => {
  const composer = makeEditable({ text: '' });
  const env = makeEnv({ editables: [composer], active: composer, images: [makeImage()] });

  env.handlers['document:input']();
  env.fireAll();
  await settled();
  await settled();

  const stash = env.invocations.find((c) => c.command === 'stash_harness_draft');
  assert.ok(stash, '没有文字不代表没有东西要保管');
  assert.equal(stash.args.text, '');
  assert.equal(stash.args.images.length, 1);
});

test('图没了（发出去或被删）要作废——哪怕一个字都没打过', async () => {
  const composer = makeEditable({ text: '' });
  const env = makeEnv({ editables: [composer], active: composer, images: [makeImage()] });

  env.handlers['document:input']();
  env.fireAll();
  await settled();
  await settled();
  assert.equal(env.invocations.filter((c) => c.command === 'stash_harness_draft').length, 1);

  env.images.length = 0; // 发送（清空编辑器）会把轨道一起清掉
  env.tickIntervals();

  assert.equal(
    env.invocations.filter((c) => c.command === 'clear_harness_draft').length,
    1,
    '只盯文字的话，图没了草稿会一直留在盘上',
  );
});

test('恢复：图回到附件轨道上，文字照旧', async () => {
  const composer = makeEditable({ text: '' });
  const draft = {
    href: 'http://127.0.0.1:3090/?session=abc',
    text: '上次没发出去的',
    images: [{ name: 'shot.png', mime: 'image/png', data: btoa('PNG-BYTES') }],
  };
  const env = makeEnv({ editables: [composer], takeResult: draft });

  env.fireAll();
  await settled();
  await settled();
  env.tickIntervals(); // 推进「注入后核对」那一拍
  await settled();

  assert.equal(env.pastes.length, 1, '必须走 paste：文件选择框那条路真机上收不下');
  const files = env.pastes[0].clipboardData.items.map((i) => i.file);
  assert.equal(files.length, 1);
  assert.equal(files[0].name, 'shot.png');
  assert.equal(files[0].type, 'image/png');
  assert.equal(env.images.length, 1, '轨道上必须真的多出一张图');
  assert.equal(composer.innerText, '上次没发出去的');
});

test('内核没收下图时：文字已回去就清盘，不留一份会重复的草稿', async () => {
  // pasteAccepts: false = 事件派发出去了，轨道上却没多出图。那就是假恢复。
  // 文字这时已经回到输入框了——再把整份放回去，只会让**下一次空的输入框**收到
  // 一份重复的文字。宁可丢掉图，也不要留一份会自己冒出来的草稿。
  const composer = makeEditable({ text: '' });
  const draft = {
    href: 'http://127.0.0.1:3090/?session=abc',
    text: '上次没发出去的',
    images: [{ name: 'shot.png', mime: 'image/png', data: btoa('PNG-BYTES') }],
  };
  const env = makeEnv({ editables: [composer], takeResult: draft, pasteAccepts: false });

  env.fireAll();
  await settled();
  await settled();
  for (let i = 0; i < 40; i += 1) {
    env.tickIntervals();
    await settled();
  }

  assert.equal(env.images.length, 0, '轨道上确实一张都没多');
  assert.equal(composer.innerText, '上次没发出去的', '文字还是要回去');
  assert.equal(
    env.invocations.filter((c) => c.command === 'stash_harness_draft' && c.args.images.length > 0).length,
    0,
    '带图的那一份绝不放回：文字已经回去了，再放回只会造成重复',
  );
  assert.ok(
    env.invocations.filter((c) => c.command === 'clear_harness_draft').length >= 1,
    '草稿要清掉：留在盘上就是下一次打开工作台时的凭空冒出',
  );
});

test('两样都没成时把整份放回（输入框还空着 ⇒ 干净的重试）', async () => {
  const composer = makeEditable({ text: '' });
  const draft = {
    href: 'http://127.0.0.1:3090/?session=abc',
    text: '上次没发出去的',
    images: [{ name: 'shot.png', mime: 'image/png', data: btoa('PNG-BYTES') }],
  };
  const env = makeEnv({
    editables: [composer],
    takeResult: draft,
    pasteAccepts: false,
    execCommandWorks: false,
  });

  env.fireAll();
  await settled();
  await settled();
  for (let i = 0; i < 40; i += 1) {
    env.tickIntervals();
    await settled();
  }

  const back = env.invocations.filter((c) => c.command === 'stash_harness_draft');
  assert.equal(back.length, 1, '什么都没恢复就必须放回去等下一次页面加载');
  assert.equal(back[0].args.text, '上次没发出去的');
  assert.equal(back[0].args.images.length, 1, '图要一起放回：只放文字等于删掉用户的截图');
});

test('没有 composer 卡片时**不取**草稿：取走一张图却没地方放等于删掉它', () => {
  const composer = makeEditable({ text: '' });
  const draft = {
    href: 'http://127.0.0.1:3090/?session=abc',
    text: '',
    images: [{ name: 'shot.png', mime: 'image/png', data: btoa('PNG-BYTES') }],
  };
  const env = makeEnv({ editables: [composer], takeResult: draft, hasCard: false });

  env.fireAll();

  assert.equal(
    env.invocations.find((c) => c.command === 'take_harness_draft'),
    undefined,
    'take 是读走即删的：没有落点就别取',
  );
});

test('取回来的草稿没有图（本次改动之前写下的）也能恢复', async () => {
  const composer = makeEditable({ text: '' });
  const draft = { href: 'http://127.0.0.1:3090/?session=abc', text: '旧草稿' };
  const env = makeEnv({ editables: [composer], takeResult: draft });

  env.fireAll();
  await settled();
  await settled();

  assert.equal(composer.innerText, '旧草稿');
  assert.equal(env.pastes.length, 0, '没有图就不该派发 paste');
});

// 2026-09-30 用户报：已经发出去的消息，重启工作台后又被填回输入框。
//
// 复现的关键是页面上**还有别的可见可编辑元素**。`currentText()` 在焦点不落在
// 可编辑元素上时，退回「取所有可见可编辑元素里文字最长的那个」——而
// `stashNow()` 判断「这句话是不是已经发出去了」用的正是它是否为空。页面上任何
// 别的输入框（搜索框里残留的旧查询最常见）都会让它恒为非空，于是
// `clear_harness_draft` 永远不触发，盘上那份已发送的草稿留到下次重启才被回填。
test('发送后清盘：页面上还有别的可见输入框时也必须触发 clear_harness_draft', async () => {
  // 搜索框：用户早先搜过，词还留在框里。composer 本身发送后是空的。
  const search = makeEditable({ id: 'search', text: '上一次的搜索词' });
  const composer = makeEditable({ id: 'composer', text: '' });
  const env = makeEnv({ editables: [search, composer], active: composer });

  // ① 用户在 composer 里打字，停顿 600ms → 落盘。
  composer.innerText = '这条已经发出去了';
  env.handlers['document:input']?.({ target: composer });
  env.fireTimeouts();
  assert.ok(
    env.invocations.some((c) => c.command === 'stash_harness_draft'),
    '前置：这条应当先被存下来',
  );

  // ② 发送：编辑器被程序化清空，焦点随之离开（内核不会派发 input 事件）。
  composer.innerText = '';
  env.setActive({ tagName: 'BODY' });

  // ③ 监视的那一拍跑起来。
  env.tickIntervals();

  assert.ok(
    env.invocations.some((c) => c.command === 'clear_harness_draft'),
    '输入框空了而本页存过东西，必须清掉盘上那份——否则下次重启会把已发送的消息填回去',
  );
});

// 恢复路径的同一根因：`emptyEditable` 过去取「DOM 顺序里最后一个空的可编辑
// 元素」。页面上方若有个空搜索框、而 composer 里已经有用户回来之后新敲的字，倒着
// 找会跳过非空的 composer、撞上那个空搜索框——草稿于是被写进搜索框，而不是那个
// 要接住它的对话输入框。用户看到的则是「草稿不见了，对话框里也不是它」。
test('composer 里已有内容时不恢复，且绝不上方那个空输入框', async () => {
  const search = makeEditable({ id: 'search', text: '' });
  const composer = makeEditable({ id: 'composer', text: '我自己刚敲的' });
  const env = makeEnv({
    editables: [search, composer],
    active: composer,
    takeResult: { href: 'http://127.0.0.1:3090/?session=abc', text: '之前没发出去的', images: [] },
  });

  env.fireAll();
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(search.innerText, '', '绝不能把草稿写进上方那个空搜索框');
  assert.equal(composer.innerText, '我自己刚敲的', '也不能覆盖用户自己刚敲的内容');
  assert.ok(
    env.invocations.some((c) => c.command === 'stash_harness_draft'),
    '写不进去就把草稿放回去，恢复失败不能等于内容丢失',
  );
});
