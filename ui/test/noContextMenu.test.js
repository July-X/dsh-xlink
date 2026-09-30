import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

import { disableContextMenu } from '../src/noContextMenu.js';

// 壳自己的窗口走 ui/src/noContextMenu.js，工作台与三个官方对话内容 webview 走
// src-tauri/src/no-context-menu.js（Rust 注入的 IIFE，没有导出，按
// harnessHealth.test.js 的做法在 vm 里造一个假页面跑它）。两侧是同一件事的两处
// 落点，断言的是**行为**——事件被取消、左键选中与复制不受影响——而不是源码里
// 有没有某个字符串。
const injectedSource = readFileSync(
  new URL('../../src-tauri/src/no-context-menu.js', import.meta.url),
  'utf8',
);

/** 一个够用的假 window：记录所有注册上来的监听器，按注册顺序派发。 */
function makeWindow({ top = 'self' } = {}) {
  const listeners = [];
  const win = {
    listeners,
    addEventListener(type, handler, options) {
      listeners.push({ type, handler, options });
    },
    dispatch(event) {
      for (const entry of listeners) {
        if (entry.type !== event.type) continue;
        // 事件一旦被 stopImmediatePropagation 掐断，同节点上后注册的监听器
        // 不会被调用——这正是本该测的那条语义，不是浏览器替我们做的事。
        if (event.immediatePropagationStopped) break;
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
    preventDefault() {
      this.defaultPrevented = true;
    },
    stopImmediatePropagation() {
      this.immediatePropagationStopped = true;
    },
  };
}

/** 跑注入脚本并返回它装好监听器的假 window。 */
function runInjected(win) {
  runInNewContext(injectedSource, { window: win });
  return win;
}

const IMPLS = [
  ['ui/src/noContextMenu.js', (win) => disableContextMenu(win)],
  ['src-tauri/src/no-context-menu.js', runInjected],
];

for (const [name, install] of IMPLS) {
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

test('注入脚本有顶帧守卫，iframe 里什么都不装', () => {
  // Tauri 会在每个 frame 里跑初始化脚本；工作台的会话内容里嵌着网页，不加
  // 守卫就会给那些第三方页面也装上一份壳的策略。
  const win = runInjected(makeWindow({ top: 'outer' }));
  assert.equal(win.listeners.length, 0);
});

test('主界面入口装的是同一份策略，且早于任何组件挂载', () => {
  // 接线形状：漏掉这一次调用不会让任何行为测试变红（它们直接对着模块跑），
  // 症状只是面板里右键菜单照弹——所以这里对着入口源码钉一次。
  const entry = readFileSync(new URL('../src/main.js', import.meta.url), 'utf8');
  assert.match(entry, /import \{ disableContextMenu \} from '\.\/noContextMenu\.js';/);
  const call = entry.indexOf('disableContextMenu();');
  assert.notEqual(call, -1, 'main.js 没有调用 disableContextMenu()');
  assert.ok(call < entry.indexOf("app.mount('#app')"), '必须在挂载组件之前装上');
});
