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

function makeEvent(type) {
  return {
    type,
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
function dispatchFromPage(win, pageHandler) {
  const event = makeEvent('contextmenu');
  pageHandler?.(event);
  if (!event.propagationStopped) win.dispatch(event);
  return event;
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
