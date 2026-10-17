import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';
import { readShellSource, shellSourceLabel } from '../../scripts/lib/shell-source.mjs';

import { disableContextMenu } from '../src/shell/noContextMenu.js';

// 右键策略自 2026-10-17 起按窗口族分三处落点：
//   - ui/src/shell/noContextMenu.js：壳自己的五个窗口，**一律取消**；
//   - src-tauri/src/harness/no-context-menu.js：官方对话三个内容 webview
//     （别人的站点），**一律取消**（含页面自绘菜单）；
//   - src-tauri/src/harness/context-menu-fallback.js：工作台（内核 Web UI），
//     **只压引擎原生菜单**，内核自绘的菜单必须照常弹。
// 三份都是 IIFE 或纯函数，这里在假事件流上断言**行为**——菜单是被压还是被放行、
// 左键选中与复制不受影响——而不是源码里有没有某个字符串。
const strictSource = readShellSource('no-context-menu.js');
const fallbackSource = readShellSource('context-menu-fallback.js');

/** 一个够用的假 window：记录所有注册上来的监听器，按注册顺序派发。 */
function makeWindow({ top = 'self' } = {}) {
  const listeners = [];
  const win = {
    listeners,
    addEventListener(type, handler, options) {
      listeners.push({ type, handler, options, calls: 0 });
    },
    dispatch(event) {
      for (const entry of listeners) {
        if (entry.type !== event.type) continue;
        // 事件一旦被 stopImmediatePropagation 掐断，同节点上后注册的监听器
        // 不会被调用——这正是本该测的那条语义，不是浏览器替我们做的事。
        if (event.immediatePropagationStopped) break;
        entry.calls += 1;
        entry.handler(event);
      }
      return event;
    },
  };
  win.self = win;
  win.top = top === 'self' ? win : { name: 'outer frame' };
  return win;
}

function makeEvent(type, target = null) {
  return {
    type,
    target,
    defaultPrevented: false,
    immediatePropagationStopped: false,
    propagationStopped: false,
    preventDefaultCalls: 0,
    preventDefault() {
      this.defaultPrevented = true;
      this.preventDefaultCalls += 1;
    },
    stopPropagation() {
      this.propagationStopped = true;
    },
    stopImmediatePropagation() {
      this.immediatePropagationStopped = true;
    },
  };
}

/**
 * 模拟一次真实的右键传播：页面自己的处理器（目标元素 / document / React 根容器
 * 上那一层）先跑，然后才轮到 window 上的监听器。页面调过 `stopPropagation()` 时
 * window 这一站收不到事件——`ui-primitives` 的 JSON 树复制按钮就是这么写的。
 */
function dispatchFromPage(win, pageHandler, target = null) {
  const event = makeEvent('contextmenu', target);
  pageHandler?.(event);
  if (!event.propagationStopped) win.dispatch(event);
  return event;
}

/** 一个够用的假元素：脚本只看 nodeType / tagName / 可编辑性 / 选区 / 祖先链。 */
function field({
  tagName = 'DIV',
  isContentEditable = false,
  disabled = false,
  readOnly = false,
  selectionStart,
  selectionEnd,
  parentElement = null,
} = {}) {
  return {
    nodeType: 1,
    tagName,
    isContentEditable,
    disabled,
    readOnly,
    selectionStart,
    selectionEnd,
    parentElement,
  };
}

/** 给假 window 装上 Tauri 桥，返回记录下来的 invoke 调用。 */
function withTauri(win) {
  const calls = [];
  win.__TAURI__ = {
    core: {
      invoke(name, payload) {
        // 载荷是在 vm 的上下文里造出来的对象，原型与本测试不是同一个；剥一层再比，
        // 否则 deepStrictEqual 会因为「长得一样但原型不同」判红。
        calls.push({ name, payload: payload === undefined ? undefined : { ...payload } });
        return Promise.resolve();
      },
    },
  };
  return calls;
}

/** 页面里有一段只读文字的选区（window.getSelection 的形状）。 */
function withDocumentSelection(win, text = '选中的一段') {
  win.getSelection = () => ({
    rangeCount: 1,
    isCollapsed: false,
    toString: () => text,
  });
}

/** 跑一份注入脚本并返回它装好监听器的假 window。 */
function runScript(win, source) {
  runInNewContext(source, { window: win });
  return win;
}

// --- 一律取消的两份实现（壳自己的窗口 + 官方对话内容 webview）-----------------

const STRICT_IMPLS = [
  ['ui/src/shell/noContextMenu.js', (win) => disableContextMenu(win)],
  [shellSourceLabel('no-context-menu.js'), (win) => runScript(win, strictSource)],
];

for (const [name, install] of STRICT_IMPLS) {
  test(`${name}：右键的默认行为被取消`, () => {
    const win = makeWindow();
    install(win);
    const event = win.dispatch(makeEvent('contextmenu'));
    assert.equal(event.defaultPrevented, true);
    assert.equal(event.immediatePropagationStopped, true);
  });

  test(`${name}：监听挂在 window 的捕获阶段`, () => {
    // 捕获阶段是事件传播的第一站：只有这样，页面自己在 document 或目标元素上
    // 挂的监听器才抢不到前面。冒泡阶段或挂在 document 上都拦不住它们。
    const win = makeWindow();
    install(win);
    assert.equal(win.listeners.length, 1);
    assert.equal(win.listeners[0].type, 'contextmenu');
    assert.equal(win.listeners[0].options, true);
  });

  test(`${name}：页面后注册的右键菜单监听器不会被调用`, () => {
    const win = makeWindow();
    install(win);
    let pageHandlerRan = false;
    win.addEventListener('contextmenu', () => {
      pageHandlerRan = true;
    });
    win.dispatch(makeEvent('contextmenu'));
    assert.equal(pageHandlerRan, false);
  });

  test(`${name}：左键选中与复制完全不受影响`, () => {
    // 「禁右键」最常见的写错方式是顺手补一句 user-select: none 或拦下
    // copy —— 症状是菜单确实没了，用户只是再也复制不出东西，且不报任何错。
    // 行为层的判据：只碰 contextmenu 一个事件名。
    const win = makeWindow();
    install(win);
    const registered = win.listeners.map((entry) => entry.type);
    assert.deepEqual(registered, ['contextmenu']);
    for (const type of ['select', 'selectstart', 'copy', 'cut', 'mousedown', 'dragstart']) {
      const event = win.dispatch(makeEvent(type));
      assert.equal(event.defaultPrevented, false, `${type} 不该被拦`);
    }
  });
}

// --- 工作台：只压引擎原生菜单，放行内核自绘菜单 -------------------------------

const fallbackLabel = shellSourceLabel('context-menu-fallback.js');
const installFallback = (win) => runScript(win, fallbackSource);

test(`${fallbackLabel}：监听挂在 window 的冒泡阶段`, () => {
  // 冒泡是传播的最后一站：页面处理器已经跑完，此时 preventDefault 只关掉引擎
  // 默认动作，不会碰页面已经画出来的菜单。挂回捕获阶段就是这次修复要修的病。
  const win = makeWindow();
  installFallback(win);
  assert.equal(win.listeners.length, 1);
  assert.equal(win.listeners[0].type, 'contextmenu');
  assert.equal(win.listeners[0].options, false);
});

test(`${fallbackLabel}：页面没处理时压掉引擎原生菜单`, () => {
  const win = makeWindow();
  installFallback(win);
  const event = dispatchFromPage(win, () => {});
  assert.equal(event.defaultPrevented, true);
  // 只是取消默认行为：不许掐断传播，否则页面在同一节点上后注册的处理器又被拦掉。
  assert.equal(event.immediatePropagationStopped, false);
});

test(`${fallbackLabel}：内核自绘菜单的处理器照常跑`, () => {
  // 这是这次改动的目的：JSON 树的「选择复制方式」、右侧栏标签页菜单都挂在页面
  // 自己的处理器上，旧实现下它们一个都跑不到。
  const win = makeWindow();
  installFallback(win);
  let pageHandlerRan = false;
  const event = dispatchFromPage(win, () => {
    pageHandlerRan = true;
  });
  assert.equal(pageHandlerRan, true);
  assert.equal(win.listeners[0].calls, 1, '兜底自己也应该跑，负责压掉引擎菜单');
  assert.equal(event.defaultPrevented, true);
});

test(`${fallbackLabel}：页面已 preventDefault 时不重复处理`, () => {
  const win = makeWindow();
  installFallback(win);
  const event = dispatchFromPage(win, (e) => e.preventDefault());
  assert.equal(event.defaultPrevented, true);
  assert.equal(event.preventDefaultCalls, 1, '页面已经关掉默认行为，兜底不该再关一次');
});

test(`${fallbackLabel}：页面 stopPropagation 时兜底根本不触发`, () => {
  // JSON 树复制按钮的形状：preventDefault + stopPropagation，事件到不了 window。
  const win = makeWindow();
  installFallback(win);
  const event = dispatchFromPage(win, (e) => {
    e.preventDefault();
    e.stopPropagation();
  });
  assert.equal(event.defaultPrevented, true);
  assert.equal(win.listeners[0].calls, 0);
});

test(`${fallbackLabel}：左键选中与复制完全不受影响`, () => {
  const win = makeWindow();
  installFallback(win);
  const registered = win.listeners.map((entry) => entry.type);
  assert.deepEqual(registered, ['contextmenu']);
  for (const type of ['select', 'selectstart', 'copy', 'cut', 'mousedown', 'dragstart']) {
    const event = win.dispatch(makeEvent(type));
    assert.equal(event.defaultPrevented, false, `${type} 不该被拦`);
  }
});

// --- 工作台：页面没处理时，请壳弹一份原生编辑菜单 -----------------------------

test(`${fallbackLabel}：可编辑区域右键请壳弹编辑菜单`, () => {
  // 工作台把系统菜单压掉了，没有这一步用户在输入框里就没有鼠标路径复制 / 粘贴
  // （内核官方 Electron 壳在同样位置给的就是这套编辑项）。
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  const event = dispatchFromPage(win, null, field({ tagName: 'INPUT' }));
  assert.equal(event.defaultPrevented, true, '系统菜单仍然不弹');
  assert.deepEqual(calls, [
    { name: 'show_workbench_edit_menu', payload: { editable: true, hasSelection: false } },
  ]);
});

test(`${fallbackLabel}：输入框里的选区一起报给壳`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  dispatchFromPage(win, null, field({ tagName: 'TEXTAREA', selectionStart: 2, selectionEnd: 5 }));
  assert.deepEqual(calls[0].payload, { editable: true, hasSelection: true });
});

test(`${fallbackLabel}：contenteditable 里的选区算可编辑`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  withDocumentSelection(win);
  // 内核的 composer 是 Lexical 的 contenteditable：目标常常是里面的子节点，
  // 靠祖先链才认得出可编辑容器。
  const inner = field({ parentElement: field({ isContentEditable: true }) });
  dispatchFromPage(win, null, inner);
  assert.deepEqual(calls[0].payload, { editable: true, hasSelection: true });
});

test(`${fallbackLabel}：只读选区只请「复制」`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  withDocumentSelection(win);
  dispatchFromPage(win, null, field());
  assert.deepEqual(calls[0].payload, { editable: false, hasSelection: true });
});

test(`${fallbackLabel}：只读输入框里的选区也算选区`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  const readOnly = field({ tagName: 'INPUT', readOnly: true, selectionStart: 0, selectionEnd: 3 });
  dispatchFromPage(win, null, readOnly);
  assert.deepEqual(calls[0].payload, { editable: false, hasSelection: true });
});

test(`${fallbackLabel}：既不可编辑也没选区时不打扰壳`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  const event = dispatchFromPage(win, null, field());
  assert.deepEqual(calls, [], '没有可做的事就不付 IPC 往返');
  assert.equal(event.defaultPrevented, true, '系统菜单照样压掉');
});

test(`${fallbackLabel}：页面自己处理过的右键不弹壳的菜单`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  const event = dispatchFromPage(win, (e) => e.preventDefault(), field({ tagName: 'INPUT' }));
  assert.deepEqual(calls, [], '内核自绘菜单优先，壳不叠一层');
  assert.equal(event.defaultPrevented, true);
});

test(`${fallbackLabel}：页面 stopPropagation 时脚本根本没跑`, () => {
  const win = makeWindow();
  installFallback(win);
  const calls = withTauri(win);
  dispatchFromPage(win, (e) => {
    e.preventDefault();
    e.stopPropagation();
  }, field({ tagName: 'INPUT' }));
  assert.deepEqual(calls, []);
});

test(`${fallbackLabel}：壳缺席时右键仍然是「压掉系统菜单」`, () => {
  // 浏览器里直接打开内核 web UI 时没有 __TAURI__：不许抛错，事件链上游是内核页面。
  const win = makeWindow();
  installFallback(win);
  const event = dispatchFromPage(win, null, field({ tagName: 'INPUT' }));
  assert.equal(event.defaultPrevented, true);
  assert.equal(win.listeners[0].calls, 1);
});

// --- 两份注入脚本的公共形状 ---------------------------------------------------

for (const source of [strictSource, fallbackSource]) {
  test('注入脚本有顶帧守卫，iframe 里什么都不装', () => {
    // Tauri 会在 Windows 上把初始化脚本注入每个 frame；工作台的 Browser 页签与
    // HTML 预览都跑在 iframe 里，不加守卫就会给那些第三方页面也装上一份壳的策略。
    const win = runScript(makeWindow({ top: 'outer' }), source);
    assert.equal(win.listeners.length, 0);
  });
}

test('主界面入口装的是同一份策略，且早于任何组件挂载', () => {
  // 接线形状：漏掉这一次调用不会让任何行为测试变红（它们直接对着模块跑），
  // 症状只是面板里右键菜单照弹——所以这里对着入口源码钉一次。
  const entry = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8');
  assert.match(entry, /import \{ disableContextMenu \} from '[^']*noContextMenu\.js';/);
  const call = entry.indexOf('disableContextMenu();');
  assert.notEqual(call, -1, 'main.js 没有调用 disableContextMenu()');
  assert.ok(call < entry.indexOf("app.mount('#app')"), '必须在挂载组件之前装上');
});
