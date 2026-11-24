/** 概览页背景的像素鲸鱼：预烘焙点阵图集 + Canvas 2D 逐格画圆点。
 *
 * 资源 `ui/public/whale-breach.webp` 取自 DSH 内核 0.2.3-alpha.2 的
 * `@deepseek-ai/dsh-client-ui-brand-official`：137 帧 / 192×108 格 / 24fps / 5.7s，
 * 红通道存圆点半径、绿通道存不透明度（各 32 级量化）。淡入淡出、24fps 上限、DPR≤2
 * 与「空白点击」判定都照抄内核渲染器，只去掉第二种「代码文字」填充。
 * 为什么固化成面板自己的资源、而不是运行时读内核目录：内核版本目录只读且随版本
 * 变化，面板不能依赖某一份安装存在。来源与许可（MIT, © 2026 DeepSeek）见
 * docs/ui/icon-design.md。
 */

/** 图集几何；与 whale-breach.webp 的像素尺寸一一对应（测试钉住）。 */
export const ATLAS = { width: 192, height: 108, columns: 12, frames: 137 };

/** 一次播放的时长：内核的 137 帧 @ 24fps。 */
export const DURATION_MS = 5700;
const FPS = 24;
const OPACITY = 0.65;

/** 可交互元素：点它们是在操作面板，不是在点「空白背景」。 */
const CONTROLS =
  'button, a, input, textarea, select, label, [contenteditable], [role="button"], '
  + '[role="tab"], [role="switch"], [role="slider"], [role="menuitem"], [role="option"], '
  + '[role="dialog"], [role="menu"], [role="listbox"], .el-tabs__item';

/** 播放进度 → 图集帧号。 */
export function frameAt(progress) {
  return Math.min(ATLAS.frames - 1, Math.max(0, Math.floor(progress * ATLAS.frames)));
}

/** 播放进度 → 整体不透明度：前 380ms 淡入、最后 570ms 淡出。 */
export function fadeAt(progress) {
  return Math.min(1, progress * 15, (1 - progress) * 10);
}

/**
 * 画布尺寸 → 鲸鱼场景的尺寸与左上角。等比、左右各留 3% 边距；
 * 垂直中心落在画布 43% 高度（内核同款构图），矮画布上贴顶不越界。
 * @param {number} width 画布 CSS 宽
 * @param {number} height 画布 CSS 高
 * @returns {{sceneWidth: number, sceneHeight: number, left: number, top: number}}
 */
export function sceneLayout(width, height) {
  const sceneWidth = Math.min(width * 0.94, height * 1.6, 1120);
  const sceneHeight = (sceneWidth * ATLAS.height) / ATLAS.width;
  return {
    sceneWidth,
    sceneHeight,
    left: (width - sceneWidth) / 2,
    top: Math.min(height - sceneHeight, Math.max(0, height * 0.43 - sceneHeight / 2)),
  };
}

/**
 * 这次点击算不算「点空白背景」：左键、目标在容器内、不在控件上、
 * 也不是拖选文字结束后补发的那一下。
 * @param {MouseEvent} event 点击事件
 * @param {Element} frame 接收点击的容器
 * @param {Selection | null} selection 当前选区
 * @returns {boolean}
 */
export function isBlankClick(event, frame, selection) {
  if (event.button !== 0 || !frame.contains(event.target)) return false;
  if (event.target.closest(CONTROLS) !== null) return false;
  return selection === null || selection.isCollapsed || !frame.contains(selection.anchorNode);
}

/**
 * 把鲸鱼挂到 canvas 上：首次 play 才取图，解码一次后按帧画。
 * @param {HTMLCanvasElement} canvas 装饰画布（由页面负责定位与着色）
 * @param {string} atlasUrl 图集地址
 * @returns {{play: () => void, dispose: () => void}} 重播入口与卸载清理
 */
export function attachHomeWhale(canvas, atlasUrl) {
  const ctx = canvas.getContext('2d');
  if (ctx === null) return { play() {}, dispose() {} };
  const media = matchMedia('(prefers-reduced-motion: reduce)');
  const image = new Image();
  let pixels;
  let started;
  let raf = 0;
  let last = -Infinity;
  let width = 0;
  let height = 0;
  let layout = { sceneWidth: 0, left: 0, top: 0 };
  let disposed = false;
  let loading = false;
  let failed = false;

  const clear = () => ctx.clearRect(0, 0, width, height);
  const drawable = () => !disposed && !document.hidden && width > 0 && height > 0;

  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
    started = undefined;
    clear();
  }

  function fail(error) {
    if (disposed) return;
    failed = true;
    stop();
    console.warn('概览页鲸鱼动画不可用，其余界面不受影响。', error);
  }

  function paint(now) {
    clear();
    if (pixels === undefined || started === undefined) return;
    const progress = (now - started) / DURATION_MS;
    if (progress >= 1) {
      started = undefined;
      return;
    }
    const index = frameAt(progress);
    const offsetX = (index % ATLAS.columns) * ATLAS.width;
    const offsetY = Math.floor(index / ATLAS.columns) * ATLAS.height;
    const stride = ATLAS.width * ATLAS.columns;
    const cell = layout.sceneWidth / ATLAS.width;
    const fade = fadeAt(progress);
    for (let y = 0; y < ATLAS.height; y++) {
      for (let x = 0; x < ATLAS.width; x++) {
        const offset = ((offsetY + y) * stride + offsetX + x) * 4;
        const alpha = pixels[offset + 1] / 255;
        if (alpha < 0.015) continue;
        ctx.globalAlpha = alpha * OPACITY * fade * 0.8;
        ctx.beginPath();
        ctx.arc(layout.left + (x + 0.5) * cell, layout.top + (y + 0.5) * cell, (pixels[offset] / 255) * cell, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    ctx.globalAlpha = 1;
  }

  function tick(now) {
    raf = 0;
    if (!drawable()) return;
    if (media.matches) { stop(); return; }
    if (now - last >= 1000 / FPS - 0.5) {
      paint(now);
      last = now;
    }
    if (started !== undefined) raf = requestAnimationFrame(tick);
  }

  function resume() {
    cancelAnimationFrame(raf);
    raf = 0;
    if (disposed || !drawable()) return;
    if (media.matches) { stop(); return; }
    if (started === undefined) { clear(); return; }
    paint(performance.now());
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }

  function resize() {
    width = canvas.clientWidth;
    height = canvas.clientHeight;
    const dpr = Math.min(devicePixelRatio, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    layout = sceneLayout(width, height);
    resume();
  }

  function begin() {
    // 每次播放都重取一次墨色：主题可能在上次播放之后切换过。
    ctx.fillStyle = getComputedStyle(canvas).color;
    started = performance.now();
    resume();
  }

  image.onload = () => {
    if (disposed) return;
    const source = document.createElement('canvas');
    source.width = image.naturalWidth;
    source.height = image.naturalHeight;
    try {
      if (source.width !== ATLAS.width * ATLAS.columns
        || source.height !== ATLAS.height * Math.ceil(ATLAS.frames / ATLAS.columns)) {
        throw new Error('鲸鱼图集尺寸与帧几何不符');
      }
      const reader = source.getContext('2d');
      if (reader === null) throw new Error('鲸鱼图集离屏画布不可用');
      reader.drawImage(image, 0, 0);
      pixels = reader.getImageData(0, 0, source.width, source.height).data;
    } catch (error) {
      fail(error);
      return;
    } finally {
      source.width = source.height = 0;
      image.removeAttribute('src');
    }
    begin();
  };
  image.onerror = () => fail(new Error('鲸鱼图集解码失败'));

  function play() {
    if (failed || disposed || started !== undefined || media.matches || !drawable()) return;
    if (pixels !== undefined) { begin(); return; }
    if (loading) return;
    loading = true;
    image.src = atlasUrl;
  }

  const sizeObserver = new ResizeObserver(resize);
  sizeObserver.observe(canvas);
  document.addEventListener('visibilitychange', resume);
  media.addEventListener('change', resume);
  resize();

  return {
    play,
    dispose() {
      disposed = true;
      stop();
      image.onload = image.onerror = null;
      image.removeAttribute('src');
      pixels = undefined;
      sizeObserver.disconnect();
      document.removeEventListener('visibilitychange', resume);
      media.removeEventListener('change', resume);
    },
  };
}
