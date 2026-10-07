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

// 这里原本还有一条「发布列表 ref 存在时才挂 ResizeObserver」的判据，它守护的
// 是发布列表上下那两条半透明淡出带（2026-10-05 用户要求加大范围与力度，
// 2026-10-07 用户要求移除：它让这一栏看起来像蒙了一层雾）。带子连同它的
// ResizeObserver 与 `.release-list--bleed` 外溢视口已整套删除，那条判据的
// 守护对象不存在，随之删除——**留着它只会在下一次有人读这个文件时误以为
// 还有一个 ResizeObserver 要挂**。

test('bridge 在没有 Tauri 时仍为 listen 返回可链式处理的 Promise', () => {
  assert.match(
    bridge,
    /if \(!tauriEvent\) return Promise\.resolve\(\);/,
    '纯浏览器模式不能让 listen 返回 undefined'
  );
  assert.match(subscriptionBlock(), /listen\('disk-usage-refreshed'/);
});

test('发布列表不再有溢出测量：淡出带与其 ResizeObserver 已整套移除', () => {
  // 这条不是「反向钉住不要加回来」，而是记下**为什么这里没有那个东西**。
  // 加回来会引入新的视觉层（半透明带），那是用户明确要求移除的；而真要改
  // 那个效果时，本文件顶部的注释会指向需求出处。
  // 查**剥掉注释后的代码**：`VersionsPanel` 的 `<script>` 注释与 `<template>`
  // 的 `<!-- -->` 里都正当地提到了 ResizeObserver（解释这次移除的理由），
  // 直接全文匹配会误判。三类注释都要剥——只剥前两类的话，它会被第三类抓住。
  const code = panel
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/.*$/gm, '')
    .replace(/<!--[\s\S]*?-->/g, '');
  assert.doesNotMatch(code, /ResizeObserver/);
  assert.doesNotMatch(code, /release-fade/);
  assert.doesNotMatch(code, /release-list--bleed/);
});
