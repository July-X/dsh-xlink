import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from '../src/official-chat/officialChatTabs.js';

const stripSource = readFileSync(
  new URL('../src/official-chat/OfficialChatTabs.vue', import.meta.url),
  'utf8'
);
// 取某条选择器的样式块（判据只查形状，注释里的数字不算）。
const cssBlock = (selector) => {
  const m = stripSource.match(new RegExp(`${selector.replace(/\./g, '\\.')}\\s*\\{([^}]*)\\}`));
  return m ? m[1] : null;
};

test('keeps the configured tabs in the fallback list', () => {
  assert.deepEqual(DEFAULT_OFFICIAL_CHAT_TABS, [
    { index: 0, title: 'DeepSeek' },
    { index: 1, title: 'MiniMax' },
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

// 2026-10-23 用户拍板：页签栏下沿那条蓝色长条两端走直角（原来的渐变淡出读作
// 圆角），高度 = 台灯高度 − 1/5（台灯 SVG 是 24×38 ⇒ 38 − 38/5 = 30.4）。
//
// 判据 `official-chat-brand-band` 查的是「前端与 Rust 布局对不对得上」；这里查的
// 是**规格本身**：两边一起改错（都写成 20px）时判据仍然是绿的，只有这条会红。
test('the brand band is a square-ended official-blue bar of lamp height minus a fifth', () => {
  const band = cssBlock('.chat-band');
  assert.ok(band, '页签栏里必须有 .chat-band 这段样式');

  assert.match(band, /height:\s*30\.4px/, '品牌带高度 = 台灯高度(38) − 1/5 = 30.4px');

  const radius = band.match(/border-radius\s*:\s*([^;]+);/);
  assert.ok(radius, '品牌带必须显式写 border-radius');
  assert.equal(
    Number.parseFloat(radius[1]),
    0,
    '品牌带两端走直角——任何非零圆角都读作圆角过渡'
  );

  // 颜色读 token 不写死：写死会与主题广播分叉（designAlignment 有一条判据
  // 专门钉「页签栏里不许出现十六进制色」）。token 的**值**在 theme.css 里，
  // 它必须仍是官方品牌蓝。
  assert.match(band, /background\s*:\s*var\(--official-brand\)/, '品牌带读 --official-brand token');
  const token = readFileSync(new URL('../src/theme.css', import.meta.url), 'utf8').match(
    /--official-brand:\s*(#[0-9a-fA-F]{3,8})/
  );
  assert.ok(token, 'theme.css 里必须有 --official-brand 的定义');
  assert.equal(token[1].toLowerCase(), '#4d6bfe', '官方品牌蓝 = #4D6BFE');
});

test('the tab row keeps the lamp height and the band sits below it', () => {
  const strip = cssBlock('.chat-strip');
  const tabs = cssBlock('.chat-tabs');
  assert.ok(strip && tabs, '页签栏必须有 .chat-strip 与 .chat-tabs 两段样式');

  assert.match(strip, /flex-direction:\s*column/, '带子必须在页签行下面（竖排）');
  assert.match(tabs, /height:\s*38px/, '页签行高度 = 台灯高度 38px');
});
