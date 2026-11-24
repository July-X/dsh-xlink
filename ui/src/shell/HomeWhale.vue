<script>
// 整个 webview 生命周期只自动播一次：概览页切走再回来不重播，
// 想再看点背景空白处（或重启面板）。
let autoPlayed = false;
</script>

<script setup>
// 概览页背景的像素鲸鱼。所有样式在 theme.css（`.layout` / `.home-whale`）：
// 这是壳级装饰层，不是某个页面里的组件样式。
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { attachHomeWhale, isBlankClick } from './homeWhale.js';

const ATLAS_URL = '/whale-breach.webp';

const canvas = ref(null);
let handle;

/**
 * 面板里点空白背景 → 重播一次；控件与「拖选文字结束」的点击不算。
 * @param {MouseEvent} event 主区域容器的点击事件（currentTarget 即背景容器）
 */
function playOnClick(event) {
  if (isBlankClick(event, event.currentTarget, window.getSelection())) handle?.play();
}

onMounted(() => {
  handle = attachHomeWhale(canvas.value, ATLAS_URL);
  if (autoPlayed) return;
  autoPlayed = true;
  // 等首帧布局落定再播：mounted 里 canvas 尺寸可能还是 0。
  requestAnimationFrame(() => handle?.play());
});

onBeforeUnmount(() => {
  handle?.dispose();
  handle = undefined;
});

defineExpose({ playOnClick });
</script>

<template>
  <canvas ref="canvas" class="home-whale" aria-hidden="true" />
</template>
