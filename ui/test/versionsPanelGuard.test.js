import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const panel = readFileSync(new URL('../src/kernel/VersionsPanel.vue', import.meta.url), 'utf8');
const bridge = readFileSync(new URL('../src/shell/bridge.js', import.meta.url), 'utf8');

function subscriptionBlock() {
  const start = panel.indexOf('let diskUnlisten = null;');
  const end = panel.indexOf('// 字节 → 人类可读。', start);
  assert.ok(start >= 0 && end > start, '找不到磁盘用量事件订阅代码');
  return panel.slice(start, end);
}

function observerBlock() {
  const start = panel.indexOf('let releaseResizeObserver = null;');
  const end = panel.indexOf('onBeforeUnmount(() => releaseResizeObserver?.disconnect());', start);
  assert.ok(start >= 0 && end > start, '找不到发布列表 ResizeObserver 代码');
  return panel.slice(start, end);
}

test('bridge 在没有 Tauri 时仍为 listen 返回可链式处理的 Promise', () => {
  assert.match(
    bridge,
    /if \(!tauriEvent\) return Promise\.resolve\(\);/,
    '纯浏览器模式不能让 listen 返回 undefined'
  );
  assert.match(subscriptionBlock(), /listen\('disk-usage-refreshed'/);
});

test('VersionsPanel 只有在发布列表 ref 存在时才创建并挂载 ResizeObserver', () => {
  const source = observerBlock();
  assert.match(source, /if \(releaseListEl\.value\) releaseResizeObserver\.observe\(releaseListEl\.value\);/);
});
