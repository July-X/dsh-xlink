import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from '../src/official-chat/officialChatTabs.js';

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8');

test('keeps the configured tabs in the fallback list', () => {
  // 兜底表带**颜色**（2026-10-24）：页签栏不许写死十六进制，但 IPC 不可用时
  // 颜色仍要有着落，所以这里带一份与 Rust 侧 `OFFICIAL_CHAT_BRAND_COLORS`
  // 同值的副本。判据第 20 项跨语言比对这两处。
  //
  // `loading` 是**运行期**状态（那一页的浏览器还在加载），只有壳知道。兜底
  // 表里恒为 `false`：IPC 不可用时我们连页面都没打开，谈不上加载中。
  assert.deepEqual(DEFAULT_OFFICIAL_CHAT_TABS, [
    { index: 0, title: 'DeepSeek', color: '#4D6BFE', loading: false },
    { index: 1, title: 'MiniMax', color: '#E2167E', loading: false },
  ]);
});

test('the fallback list never claims the conditional ChatGPT tab', () => {
  // 兜底路径存在的理由正是「问不到后端」（IPC 被拒 / 较旧构建），此时无从
  // 判断本机 Codex 登录是否可用。把 ChatGPT 写进兜底表会让它在**没有判据**
  // 的情况下也出现，等于绕过登录态校验。
  assert.ok(
    !DEFAULT_OFFICIAL_CHAT_TABS.some((tab) => tab.title === 'ChatGPT'),
    'ChatGPT 是条件页签，不该出现在 IPC 不可用时的兜底表里'
  );
});

test('keeps visible defaults when the tab command has no result', () => {
  assert.deepEqual(resolveOfficialChatTabs(undefined), DEFAULT_OFFICIAL_CHAT_TABS);
  assert.deepEqual(resolveOfficialChatTabs([]), DEFAULT_OFFICIAL_CHAT_TABS);
});

test('uses the authoritative command result when available', () => {
  const tabs = [{ index: 0, title: 'DeepSeek' }];

  assert.strictEqual(resolveOfficialChatTabs(tabs), tabs);
});

test('keeps the brand color the backend sent with each tab', () => {
  // 颜色随页签一起下发（Rust 侧品牌色表）。前端**原样透传**——它不参与判定，
  // 也不许自己改写颜色。
  const tabs = [
    { index: 0, title: 'DeepSeek', color: '#4D6BFE' },
    { index: 2, title: 'ChatGPT', color: '#FFFFFF' },
  ];

  assert.strictEqual(resolveOfficialChatTabs(tabs), tabs);
  assert.equal(resolveOfficialChatTabs(tabs)[1].color, '#FFFFFF');
});

test('renders the conditional ChatGPT tab appended by the backend', () => {
  // 后端在固定页签之后追加条件页签。前端原样透传即可，条带不参与判定。
  const tabs = [
    { index: 0, title: 'DeepSeek' },
    { index: 1, title: 'MiniMax' },
    { index: 2, title: 'ChatGPT' },
  ];

  assert.strictEqual(resolveOfficialChatTabs(tabs), tabs);
  assert.deepEqual(
    resolveOfficialChatTabs(tabs).map((tab) => tab.index),
    [0, 1, 2],
    '下标必须连续且与后端一致——它就是内容 webview 的 label 后缀'
  );
});

test('页签文字用手写体，颜色由后端下发（前端不写死色值）', () => {
  // 2026-10-24 用户拍板：页签文字改手写体、颜色用各站品牌主色调。
  const vue = read('../src/official-chat/OfficialChatTabs.vue');
  const style = vue.slice(vue.indexOf('<style'));
  // 只取 `.chat-tab` 这一条规则：`.chat-strip` 里也有 font-size（条带整体
  // 13px），拿全文匹配会量错对象。
  const tabRule = /\.chat-tab\s*\{([\s\S]*?)\}/.exec(style);
  assert.ok(tabRule, '必须能找到 .chat-tab 规则');

  // 手写体：全部用系统自带字体，零产物增长，也不必为一个字形引一套 webfont。
  assert.match(
    tabRule[1],
    /font-family:[^;]*"Bradley Hand"/,
    '页签文字要用手写体（系统字体，不引 webfont）'
  );
  assert.match(
    tabRule[1],
    /font-family:[^;]*cursive/,
    '字体栈末尾要有 cursive 兜底——Windows 上前面的系统字体可能一个都没有'
  );
  // 手写体比默认无衬线小一号就发糊。
  const size = /font-size:\s*(\d+)px/.exec(tabRule[1]);
  assert.ok(size, '页签文字必须显式定字号');
  assert.ok(Number(size[1]) >= 15, `手写体 ${size[1]}px 偏小，至少 15px`);

  // 颜色：后端随页签下发 → 绑进 CSS 变量 → 规则读变量。
  assert.match(
    vue,
    /'--tab-color':\s*tab\.color/,
    '页签颜色必须由后端下发的 tab.color 绑进 --tab-color'
  );
  assert.match(tabRule[1], /var\(--tab-color/, '文字颜色要读 --tab-color');
  assert.doesNotMatch(
    vue,
    /#[0-9a-f]{3,8}\b/i,
    '页签栏不许写死十六进制色——事实源在 Rust 侧的品牌色表（判据第 20 项跨语言比对）'
  );
});

test('keeps the loading flag the backend sent with each tab', () => {
  // 加载态是**运行期**的：内容页签按需挂载 + 后台预热，某个站点的首屏可能
  // 要好几秒。后端随页签一起下发 `loading`，前端原样透传。
  const tabs = [
    { index: 0, title: 'DeepSeek', color: '#4D6BFE', loading: false },
    { index: 1, title: 'MiniMax', color: '#E2167E', loading: true },
  ];

  assert.strictEqual(resolveOfficialChatTabs(tabs), tabs);
  assert.equal(resolveOfficialChatTabs(tabs)[1].loading, true);
});

test('页签栏按 loading 态显示加载指示（加载期不给反馈读作卡死）', () => {
  // 2026-10-24 取证：切页签时旧页面会瞬间消失、换成一片白屏，加载期又
  // 没有任何指示——用户读到的是「卡死了」。转圈是最小成本的诚实反馈。
  const vue = read('../src/official-chat/OfficialChatTabs.vue');

  assert.match(vue, /v-if="tab\.loading"/, '加载中的页签要渲染加载指示');
  assert.match(vue, /chat-tab-spinner/, '加载指示要有对应的样式');
  const spinner = /\.chat-tab-spinner\s*\{([\s\S]*?)\}/.exec(vue);
  assert.ok(spinner, '必须能找到 .chat-tab-spinner 规则');
  assert.match(
    spinner[1],
    /animation:[^;]*chat-tab-spin/,
    '加载指示要动起来——一个不动的图标读不出「还在加载」'
  );
  // 指示用 currentColor 跟着品牌色走，不许写死颜色。
  assert.match(spinner[1], /currentColor/, '加载指示的颜色要跟着页签的品牌色');
  assert.doesNotMatch(
    spinner[1],
    /#[0-9a-f]{3,8}\b/i,
    '加载指示不许写死十六进制色'
  );
});
