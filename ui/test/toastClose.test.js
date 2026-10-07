// toast 必须真的有关闭按钮（2026-10-07）。
//
// 起因：用户点 toast 左上角那个"×"没反应，toast 还在。那个 × 不是关闭按钮——
// 它是 Element Plus 给 `type: 'error'` 的类型图标 `CircleCloseFilled`（实心红圆
// 加白叉），长得极像。而真正的关闭按钮当时**根本没渲染**：`notify.js` 从来没传
// `showClose`，Element Plus 2.14.5 的默认值就是 `false`。于是页内唯一的关闭
// 入口不存在，用户只能干等 `duration` 走完。
//
// 这类 bug 的共同点是**静默**：传错选项名、或者干脆漏传，页面上都不报错，只是
// "点了没反应"。所以这里钉三件事——我们传的是真选项、所有调用点都走共享选项、
// 以及上游默认值与图标身份没有在我们不知情的情况下变掉。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const NOTIFY = 'ui/src/shell/notify.js';
const EP_MESSAGE = 'node_modules/element-plus/es/components/message/src/message.mjs';
const EP_ICON_MAP = 'node_modules/element-plus/es/utils/vue/icon.mjs';

const read = (path) => readFileSync(path, 'utf8');
const countOf = (text, needle) => text.split(needle).length - 1;

test('showClose 是 Element Plus 真实存在的 prop，不是被静默忽略的废键', () => {
  // 传一个上游不认的键不会报错，只会被忽略——症状与"漏传"完全一样（没有关闭
  // 按钮）。所以先确认这个键在当前版本里真的存在。
  const ep = read(EP_MESSAGE);
  assert.match(
    ep,
    /showClose:\s*\{/,
    'Element Plus 的 message props 里必须还有 showClose；否则 notify.js 传它是静默无效的'
  );
});

test('上游默认值仍是 showClose: false —— 必须显式传，不能依赖默认', () => {
  const ep = read(EP_MESSAGE);
  assert.match(
    ep,
    /showClose:\s*false/,
    'Element Plus 的 showClose 默认值已不再是 false。若上游改成了默认开启，' +
      'notify.js 里"必须显式传 showClose"的注释就该更新；即便默认开启，' +
      '显式传也仍然正确。'
  );
});

test('error 类型图标仍是 CircleCloseFilled —— 根因描述没有过期', () => {
  // notify.js 的注释把"用户点的那个 ×"归给了这个图标。哪天它换了形状，
  // 那条根因说明就该重写，而不是留着误导下一个人。
  const icons = read(EP_ICON_MAP);
  assert.match(
    icons,
    /error:\s*CircleCloseFilled/,
    'Element Plus 的 message error 类型图标已变；notify.js 里关于"用户点的是类型图标"的说明需要重写'
  );
});

test('每一个 ElMessage 调用点都通过共享选项拿到 showClose', () => {
  const notify = read(NOTIFY);

  assert.ok(
    /const toastOptions = \(ms\) => \(\{[^}]*showClose:\s*true/.test(notify),
    '共享选项里必须有 showClose: true —— 它是页内唯一的关闭入口'
  );

  const calls = countOf(notify, 'ElMessage({');
  const shared = countOf(notify, '...toastOptions(ms)');
  assert.ok(calls >= 1, '应当至少有一处 ElMessage 调用');
  assert.equal(
    shared,
    calls,
    `有 ${calls} 处 ElMessage 调用却只有 ${shared} 处走了共享 toastOptions：` +
      '漏掉的那处不会有关闭按钮（静默失效，用户看到的是"点了没反应"）'
  );

  assert.doesNotMatch(
    notify,
    /showClose:\s*false/,
    '任何地方都不该把 showClose 关掉——那等于拿掉唯一的关闭入口'
  );
});

test('共享选项仍然同时带着浮层层级，不能因为合并而丢', () => {
  // toastOptions 是从两个调用点提取出来的（P2-11 的层级修复：Element Plus 的
  // 2000 + 自增基线恒低于进度浮层，确认框会被盖住且点不到）。提取时最容易丢的
  // 就是这一项，所以单独钉住。
  const notify = read(NOTIFY);
  const options = notify.match(/const toastOptions = \(ms\) => \(\{([^}]*)\}/);
  assert.ok(options, '必须存在 toastOptions 这一个共享选项出处');
  assert.match(
    options[1],
    /zIndex:\s*NOTIFY_Z_INDEX/,
    '共享选项里必须带 zIndex：漏了它，确认框与 toast 会被进度浮层盖住（P2-11）'
  );
  assert.match(options[1], /duration:\s*ms/, '共享选项里必须带 duration');
});
