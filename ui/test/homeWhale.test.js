// 面板背景鲸鱼：纯数学（帧号 / 淡入淡出 / 构图）与图集几何的判据。
// 渲染本体要浏览器 Canvas，这里钉的是「图集与渲染参数必须一一对应」——换错
// 图集、改窄了场景公式时先红，而不是在页面上静默画歪或画出图集外的格子。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import {
  ATLAS,
  DURATION_MS,
  attachHomeWhale,
  extractFrames,
  fadeAt,
  frameAt,
  isBlankClick,
  sceneLayout,
} from '../src/shell/homeWhale.js';

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

test('空白点击排除控件、折叠标题、下拉框、非左键与拖选文字', () => {
  const frame = { contains: (node) => node !== 'outside' };
  const blank = { closest: () => null };
  const matching = (token) => ({
    closest: (selector) => (selector.split(',').some((item) => item.trim() === token) ? ({}) : null),
  });
  const collapsed = { isCollapsed: true, anchorNode: null };
  assert.equal(isBlankClick({ button: 0, target: blank }, frame, collapsed), true, '空白处左键应触发');
  assert.equal(isBlankClick({ button: 2, target: blank }, frame, collapsed), false, '右键不算');
  assert.equal(isBlankClick({ button: 0, target: 'outside' }, frame, collapsed), false, '容器外不算');
  assert.equal(isBlankClick({ button: 0, target: matching('button') }, frame, collapsed), false, '按钮上不算空白');
  assert.equal(isBlankClick({ button: 0, target: matching('summary') }, frame, collapsed), false, '<summary> 折叠条不算空白');
  assert.equal(isBlankClick({ button: 0, target: matching('.el-select') }, frame, collapsed), false, 'el-select 外层 wrapper 不算空白');
  assert.equal(isBlankClick({ button: 0, target: matching('[role="combobox"]') }, frame, collapsed), false, 'combobox 不算空白');
  assert.equal(isBlankClick({ button: 0, target: blank }, frame, { isCollapsed: false, anchorNode: blank }), false, '拖选文字结束的那一下不算');
  assert.equal(isBlankClick({ button: 0, target: blank }, frame, { isCollapsed: false, anchorNode: 'outside' }), true, '选区在面板外时不清空触发');
});

test('稀疏解码按不透明度字节分桶且过滤不可见像素', () => {
  const width = ATLAS.width * ATLAS.columns;
  const height = ATLAS.height * Math.ceil(ATLAS.frames / ATLAS.columns);
  const pixels = new Uint8ClampedArray(width * height * 4);
  const put = (frame, x, y, r, g) => {
    const ox = (frame % ATLAS.columns) * ATLAS.width + x;
    const oy = Math.floor(frame / ATLAS.columns) * ATLAS.height + y;
    const idx = (oy * width + ox) * 4;
    pixels[idx] = r;
    pixels[idx + 1] = g;
  };
  put(0, 1, 2, 80, 3);   // alpha < 0.015，过滤
  put(0, 3, 4, 0, 120);  // 半径为 0，过滤
  put(0, 5, 6, 40, 200);
  put(0, 7, 8, 60, 50);
  put(0, 9, 10, 90, 200);

  const frames = extractFrames(pixels);
  assert.equal(frames.length, ATLAS.frames);
  assert.deepEqual(Array.from(frames[0].groups), [50, 1, 200, 3], '按绿通道升序分桶并记录累计终点');
  assert.deepEqual(Array.from(frames[0].dots), [7, 8, 60, 5, 6, 40, 9, 10, 90]);
  assert.equal(frames[1].dots.length, 0);
});

test('播放中尺寸变化重置画布宽高时保留墨色，且批绘先 moveTo 再 arc', () => {
  const width = ATLAS.width * ATLAS.columns;
  const height = ATLAS.height * Math.ceil(ATLAS.frames / ATLAS.columns);
  const atlasPixels = new Uint8ClampedArray(width * height * 4);
  atlasPixels[0] = 64;
  atlasPixels[1] = 128;
  atlasPixels[4] = 80;
  atlasPixels[5] = 128;

  const ops = [];
  const fillColors = [];
  const ctx = {
    fillStyle: '#000000',
    globalAlpha: 1,
    clearRect() {},
    setTransform() {},
    beginPath() { ops.push('beginPath'); },
    moveTo() { ops.push('moveTo'); },
    arc() { ops.push('arc'); },
    fill() { ops.push('fill'); fillColors.push(this.fillStyle); },
  };
  let w = 800;
  let h = 600;
  const canvas = {
    clientWidth: 800,
    clientHeight: 600,
    get width() { return w; },
    // 真实 Canvas 2D 重设 width/height 会把 fillStyle 洗回 #000000。
    set width(v) { w = v; ctx.fillStyle = '#000000'; },
    get height() { return h; },
    set height(v) { h = v; ctx.fillStyle = '#000000'; },
    getContext: () => ctx,
  };

  let resizeCb;
  let createdImage;
  const saved = {};
  const globals = {
    devicePixelRatio: 1,
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
    ResizeObserver: class {
      constructor(cb) { resizeCb = cb; }
      observe() {}
      disconnect() {}
    },
    Image: class {
      constructor() { createdImage = this; this.naturalWidth = width; this.naturalHeight = height; }
      removeAttribute() {}
    },
    document: {
      hidden: false,
      addEventListener() {},
      removeEventListener() {},
      createElement: () => ({
        width: 0,
        height: 0,
        getContext: () => ({
          drawImage() {},
          getImageData: () => ({ data: atlasPixels }),
        }),
      }),
    },
    getComputedStyle: () => ({ color: 'rgb(180, 190, 185)' }),
    requestAnimationFrame: () => 1,
    cancelAnimationFrame() {},
  };
  for (const [k, v] of Object.entries(globals)) {
    saved[k] = Object.getOwnPropertyDescriptor(globalThis, k);
    Object.defineProperty(globalThis, k, { value: v, configurable: true, writable: true });
  }

  try {
    const whale = attachHomeWhale(canvas, '/whale-breach.webp');
    whale.play();
    createdImage.onload();
    assert.deepEqual(ops, ['beginPath', 'moveTo', 'arc', 'moveTo', 'arc', 'fill'], '同透明度圆点合并一次 fill 且每枚先 moveTo');
    assert.equal(fillColors.at(-1), 'rgb(180, 190, 185)');

    // 模拟播放期间侧栏收起触发 ResizeObserver：canvas.width 重设后墨色不得退回 #000000。
    canvas.clientWidth = 900;
    resizeCb();
    assert.equal(fillColors.at(-1), 'rgb(180, 190, 185)', 'resize 重置画布后必须恢复当前墨色');
    whale.dispose();
  } finally {
    for (const [k, desc] of Object.entries(saved)) {
      if (desc) Object.defineProperty(globalThis, k, desc);
      else delete globalThis[k];
    }
  }
});
