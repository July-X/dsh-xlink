// 独立日志窗口（index.html?log=…）的内容必须跟着应用主题走。
//
// 原生标题栏那一半已经由 windowChrome.test.js 钉住（`applyTheme` →
// `setWindowTheme` + capability）。这一份钉**内容**那一半——它当时是全绿
// 的：dev server、typecheck、build、现有 389 条测试全过，而用户在浅色主题
// 下看到的是一排近乎白底的近白字。
//
// 为什么这类东西不会自己暴露：写死的颜色是**按另一个主题调出来的**那一个，
// 编译、lint、单测都无从判断它对不对，只有把主题切过去用眼睛看才发现。
// 所以它必须由机械判据守。
//
// 判据本身踩过的坑（照抄 designAlignment.test.js 的教训）：
//   · **先剥注释**。本文件描述的每一个根因都写着它原来那个色值
//     （`rgba(232,236,247,.72)` 等），不剥注释就会把自己写的说明当成命中。
//   · **证据取文件里真正生效的那条规则**，并把它的选择器打进断言消息。
//     不能拿「我传给查找函数的选择器字面量」当证据——那样改判据它照样绿。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');
const stripCss = (s) => s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');

/**
 * 把样式表拆成 (selector, body) 列表。
 *
 * `[^{}]+` 天然跳过嵌套：`@keyframes` 里 `from { … }` 的选择器位会被拼上
 * `@keyframes name {`，那串文本既不含 `:root` 也不含 `.log*`，两条判据都
 * 不会误取它。
 */
function rules(css) {
  return [...stripCss(css).matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
    selector: m[1].trim(),
    body: m[2],
  }));
}

/** 日志阅读器（含它共用的分类侧栏与拖拽分隔条）自己的类名前缀。 */
const LOG_UI = /(?:^|[\s,>+~(])\.(?:log[\w-]*|rail-toggle|pane-splitter)/;
const HARDCODED = /rgba?\(|hsla?\(|#[0-9a-f]{3,8}\b/i;

test('日志窗口的规则里没有写死的颜色', () => {
  const css = read('../src/theme.css');
  const offenders = rules(css)
    .filter((r) => LOG_UI.test(r.selector) && HARDCODED.test(r.body))
    .map((r) => `${r.selector} → ${r.body.match(HARDCODED)[0]}`);
  assert.deepEqual(
    offenders,
    [],
    '这些规则属于日志阅读器，却把颜色写死了。写死的是**某个主题**调出来的值，' +
      '换一套主题就失真（浅色下的近白字几乎看不见）。请改用 token：' +
      offenders.join(' | ')
  );
});

test('日志窗口用到的每个 token，浅色与深色两套里都真的定义了', () => {
  // 这一条比「不许写死」更容易被忽略：引用一个只在 `html.dark` 里定义的
  // token，在浅色主题下**不报错、不变红**，只是那个声明整条失效——元素退回
  // 浏览器默认色。同理，`var(--x, 兜底)` 的兜底也会被当成"已经处理过"。
  const css = read('../src/theme.css');
  const declared = { light: new Set(), dark: new Set() };
  for (const { selector, body } of rules(css)) {
    const bucket = selector.includes('html.dark') ? declared.dark : declared.light;
    for (const [, name] of body.matchAll(/(--[\w-]+)\s*:/g)) bucket.add(name);
  }
  // Element Plus 自己也定义了一堆 --el-* 变量（同一批规则里），它们在两套里
  // 都齐；只报日志 UI 自己引用到的那些。
  const used = new Set();
  for (const { selector, body } of rules(css)) {
    if (!LOG_UI.test(selector)) continue;
    for (const [, name] of body.matchAll(/var\((--[\w-]+)\)/g)) used.add(name);
  }
  assert.ok(used.size > 0, '日志 UI 至少该用到几个 token，否则上一条判据形同虚设');
  const missing = [...used].filter((n) => !declared.light.has(n) || !declared.dark.has(n));
  assert.deepEqual(
    missing,
    [],
    `这些 token 没有在两套主题里都定义，只在其中一套里定义的话，另一套下该声明整条失效：${missing.join(', ')}`
  );
});

test('accent 的两档半透明是新加的，且两套主题各给一份', () => {
  // 选中态的填色 / 描边原先写死为 rgba(79,140,255,…) ——那是深色主题的 accent
  // 调出来的。浅色主题的 accent 是深蓝 #2766d9，同一个 alpha 叠上去是一层
  // 洗不掉的蓝雾，所以两套必须各给一份值，不能只换颜色不换 alpha。
  const css = read('../src/theme.css');
  const decls = {};
  for (const { selector, body } of rules(css)) {
    const bucket = selector.includes('html.dark') ? 'dark' : 'light';
    // matchAll 的第 0 项是整段匹配，两个捕获组从第 1 项起——写成
    // `const [name, value]` 会把整段当成 token 名，于是断言读到空串。
    for (const [, name, value] of body.matchAll(/(--accent-(?:fill|line))\s*:\s*([^;]+);/g)) {
      decls[`${bucket}:${name}`] = value.trim();
    }
  }
  for (const key of [
    'light:--accent-fill',
    'light:--accent-line',
    'dark:--accent-fill',
    'dark:--accent-line',
  ]) {
    assert.match(decls[key] || '', /^rgba\(/, `${key} 未定义，且必须是半透明 accent`);
  }
  assert.notEqual(
    decls['light:--accent-line'],
    decls['dark:--accent-line'],
    '两套主题的 --accent-line 必须分别取值'
  );
});

test('未激活的日志文件签仍比分组标题亮一档', () => {
  // 这条差点在改 token 时一起抹平：分组标题降到 --text-muted 之后，如果文件签
  // 也跟着用同一个，侧栏就只剩字号与字重在区分层级——几十个文件名挤在一起时
  // 「换了一类」这件事就看不见了。
  const css = read('../src/theme.css');
  const pick = (selector) => {
    const hit = rules(css).find((r) => r.selector === selector);
    assert.ok(hit, `必须能找到 ${selector}`);
    return (hit.body.match(/color:\s*(var\(--[\w-]+\))/) || [])[1];
  };
  const groupTitle = pick('.log-group-title');
  const inactiveTab = pick(".log-tab:not([aria-selected='true']):not(:hover)");
  assert.ok(groupTitle, '.log-group-title 必须用 token 声明颜色');
  assert.ok(inactiveTab, '未激活文件签必须用 token 声明颜色');
  assert.notEqual(
    groupTitle,
    inactiveTab,
    '两者必须落在不同档：分组标题更暗，文件签更亮（否则层级靠字号硬扛）'
  );
});

test('日志窗口与弹层的模板里没有内联颜色', () => {
  // 组件自身没有 <style> 块（样式全在 theme.css），模板里却可能塞 inline
  // style——inline 优先级高于任何选择器，写进去就再也回不到 token 上。
  for (const rel of ['../src/logs/LogViewerWindow.vue', '../src/logs/LogModal.vue']) {
    const src = read(rel);
    assert.doesNotMatch(
      src.replace(/<!--[\s\S]*?-->/g, '').replace(/^\s*\/\/.*$/gm, ''),
      /style\s*=\s*"[^"]*\b(color|background|background-color|border-color)\s*:/,
      `${rel.split('/').pop()} 的模板里有内联颜色，优先级高过主题 token，等于把主题钉死`
    );
  }
});