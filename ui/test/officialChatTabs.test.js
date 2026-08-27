import assert from 'node:assert/strict';
import test from 'node:test';

import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from '../src/components/officialChatTabs.js';

test('keeps the three configured tabs in the fallback list', () => {
  assert.deepEqual(DEFAULT_OFFICIAL_CHAT_TABS, [
    { index: 0, title: 'DeepSeek' },
    { index: 1, title: '千问' },
    { index: 2, title: 'MiniMax' },
  ]);
});

test('keeps visible defaults when the tab command has no result', () => {
  assert.deepEqual(resolveOfficialChatTabs(undefined), DEFAULT_OFFICIAL_CHAT_TABS);
  assert.deepEqual(resolveOfficialChatTabs([]), DEFAULT_OFFICIAL_CHAT_TABS);
});

test('uses the authoritative command result when available', () => {
  const tabs = [{ index: 0, title: 'DeepSeek' }];

  assert.strictEqual(resolveOfficialChatTabs(tabs), tabs);
});
