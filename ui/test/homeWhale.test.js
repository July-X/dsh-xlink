// 面板背景鲸鱼：纯数学（帧号 / 淡入淡出 / 构图）与图集几何的判据。
// 渲染本体要浏览器 Canvas，这里钉的是「图集与渲染参数必须一一对应」——换错
// 图集、改窄了场景公式时先红，而不是在页面上静默画歪或画出图集外的格子。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { ATLAS, DURATION_MS, fadeAt, frameAt, isBlankClick, sceneLayout } from '../src/shell/homeWhale.js';

test('播放进度落在图集的帧边界内', () => {
  assert.equal(frameAt(0), 0);
  assert.equal(frameAt(0.5), Math.floor(0.5 * ATLAS.frames));
  assert.equal(frameAt(0.999), ATLAS.frames - 1);
  assert.equal(frameAt(1), ATLAS.frames - 1, '进度到 1 时仍要落在最后一帧（渲染前会先判结束）');
});

test('淡入淡出时长与 137 帧 @ 24fps 对得上', () => {
  assert.equal(fadeAt(0), 0);
  assert.equal(fadeAt(0.05), 0.75, '前 380ms 线性淡入');
  assert.equal(fadeAt(0.5), 1);
  assert.ok(Math.abs(fadeAt(0.95) - 0.5) < 1e-9, '最后 570ms 线性淡出');
  assert.equal(fadeAt(1), 0);
  assert.equal(DURATION_MS, 5700, '24fps × 137 帧');
});

test('场景等比且完全落在画布内', () => {
  for (const [width, height] of [[883, 710], [400, 640], [1200, 400], [2000, 2000]]) {
    const { sceneWidth, sceneHeight, left, top } = sceneLayout(width, height);
    assert.ok(sceneWidth > 0 && sceneHeight > 0, `${width}×${height} 下场景不能是 0`);
    assert.ok(left >= -1e-9 && left + sceneWidth <= width + 1e-9, `${width}×${height} 下水平越界`);
    assert.ok(top >= -1e-9 && top + sceneHeight <= height + 1e-9, `${width}×${height} 下垂直越界`);
    assert.ok(Math.abs(sceneWidth / sceneHeight - ATLAS.width / ATLAS.height) < 1e-9, '宽高比必须与图集一致');
  }
});

test('图集资源与渲染几何一致', () => {
  const bytes = readFileSync(new URL('../public/whale-breach.webp', import.meta.url));
  assert.equal(bytes.toString('ascii', 0, 4), 'RIFF');
  assert.equal(bytes.toString('ascii', 8, 12), 'WEBP');
  assert.equal(bytes.toString('ascii', 12, 16), 'VP8L', '必须是无损 WebP：红/绿通道的值要原样保留');
  assert.equal(bytes[20], 0x2f, 'VP8L 签名');
  // VP8L 头：签名后 4 字节小端，低 14 位 = 宽-1、次 14 位 = 高-1。
  const size = bytes.readUInt32LE(21);
  const width = (size & 0x3fff) + 1;
  const height = ((size >>> 14) & 0x3fff) + 1;
  assert.equal(width, ATLAS.width * ATLAS.columns, '图集列数 × 单格宽必须等于资源宽度');
  assert.equal(height, ATLAS.height * Math.ceil(ATLAS.frames / ATLAS.columns), '图集行数 × 单格高必须等于资源高度');
});

test('空白点击排除控件、非左键与拖选文字', () => {
  const frame = { contains: (node) => node !== 'outside' };
  const blank = { closest: () => null };
  const control = { closest: () => ({}) };
  const collapsed = { isCollapsed: true, anchorNode: null };
  assert.equal(isBlankClick({ button: 0, target: blank }, frame, collapsed), true, '空白处左键应触发');
  assert.equal(isBlankClick({ button: 2, target: blank }, frame, collapsed), false, '右键不算');
  assert.equal(isBlankClick({ button: 0, target: 'outside' }, frame, collapsed), false, '容器外不算');
  assert.equal(isBlankClick({ button: 0, target: control }, frame, collapsed), false, '控件上不算空白');
  assert.equal(isBlankClick({ button: 0, target: blank }, frame, { isCollapsed: false, anchorNode: blank }), false, '拖选文字结束的那一下不算');
  assert.equal(isBlankClick({ button: 0, target: blank }, frame, { isCollapsed: false, anchorNode: 'outside' }), true, '选区在面板外时不清空触发');
});
