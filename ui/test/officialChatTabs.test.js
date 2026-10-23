import assert from 'node:assert/strict';
import test from 'node:test';

import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from '../src/official-chat/officialChatTabs.js';

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
