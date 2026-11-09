import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';

import { readShellSource } from './lib/shell-source.mjs';

const source = readShellSource('mcode-appearance.js');

/**
 * 在假 DOM 上跑一遍注入脚本：MiniMax 的主题档由用例给，返回 <html> 的属性。
 */
function scene(theme) {
  const attrs = new Map();
  const root = {
    getAttribute: (name) => (attrs.has(name) ? attrs.get(name) : null),
    setAttribute: (name, value) => attrs.set(name, value),
    removeAttribute: (name) => attrs.delete(name),
  };
  const window = { localStorage: { getItem: () => theme } };
  window.top = window;
  window.self = window;
  vm.runInNewContext(source, {
    window,
    document: { documentElement: root },
    location: { hostname: '127.0.0.1' },
    localStorage: window.localStorage,
    setInterval: () => {},
  });
  return {
    attribute: (name) => (attrs.has(name) ? attrs.get(name) : null),
  };
}

test('MiniMax 选跟随系统时发布 system 信号（由 workbench-appearance.js 改用壳的偏好）', () => {
  const { attribute } = scene('system');
  assert.equal(
    attribute('data-ds-theme-source'),
    'system',
    'system 必须发布出去：workbench-appearance.js 读到 system 才会跟壳偏好走，撤掉信号就退回跟 OS',
  );
});

test('MiniMax 选固定档时发布同一档信号', () => {
  assert.equal(scene('dark').attribute('data-ds-theme-source'), 'dark');
  assert.equal(scene('light').attribute('data-ds-theme-source'), 'light');
});
