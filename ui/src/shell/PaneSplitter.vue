<script setup>
// 可拖拽的分隔条：用于两个 flex 子项之间的边界。父组件持有 `v-model` 数值
// （被调整那侧的宽度），分隔条本身只负责鼠标交互与边界值裁剪。
//
// 性能：pointer + setPointerCapture 让光标跑出元素也不丢事件；rAF 节流
// 把 1kHz mousemove 收敛到 60Hz；不在拖拽期间持久化（同步 I/O 是卡顿主因）。
import { onUnmounted, ref } from 'vue';

const props = defineProps({
  modelValue: { type: Number, required: true },
  min: { type: Number, default: 160 },
  max: { type: Number, default: 600 },
  // 哪一侧是被调整的面板：'left' = 分隔条左侧的面板变宽（如左侧栏），
  // 'right' = 分隔条右侧的面板变宽。默认是 'left'（日志侧栏模式）。
  side: { type: String, default: 'left' },
});

const emit = defineEmits(['update:modelValue', 'drag-start', 'drag-end']);

const dragging = ref(false);
let startX = 0;
let startWidth = 0;
let rafId = null;
let lastEvent = null;

function commitMove() {
  rafId = null;
  if (!lastEvent) return;
  const e = lastEvent;
  lastEvent = null;
  const delta = props.side === 'left' ? e.clientX - startX : startX - e.clientX;
  const next = Math.max(props.min, Math.min(props.max, startWidth + delta));
  emit('update:modelValue', next);
}

function onPointerMove(e) {
  if (!dragging.value) return;
  lastEvent = e;
  if (rafId !== null) return;
  rafId = requestAnimationFrame(commitMove);
}

function stopDrag() {
  if (!dragging.value) return;
  dragging.value = false;
  document.body.style.cursor = '';
  document.body.style.userSelect = '';
  document.body.classList.remove('pane-dragging');
  if (lastEvent) commitMove();
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  emit('drag-end');
  // 配合 setPointerCapture，浏览器在 pointerup/cancel 时自动释放，无需 removeEventListener。
}

function onPointerDown(e) {
  if (e.button !== undefined && e.button !== 0) return;
  dragging.value = true;
  startX = e.clientX;
  startWidth = props.modelValue;
  document.body.style.cursor = 'col-resize';
  document.body.style.userSelect = 'none';
  document.body.classList.add('pane-dragging');
  try {
    e.currentTarget.setPointerCapture(e.pointerId);
  } catch {
    // 旧 Chromium / 某些 webview 会抛 InvalidStateError，兜底走 document 监听
    document.addEventListener('pointermove', onPointerMove);
    document.addEventListener('pointerup', stopDrag);
    document.addEventListener('pointercancel', stopDrag);
  }
  emit('drag-start');
}

onUnmounted(() => {
  // 组件意外卸载时确保停掉拖拽状态并清掉可能的兜底监听
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  if (dragging.value) {
    dragging.value = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    document.body.classList.remove('pane-dragging');
    emit('drag-end');
  }
  document.removeEventListener('pointermove', onPointerMove);
  document.removeEventListener('pointerup', stopDrag);
  document.removeEventListener('pointercancel', stopDrag);
});
</script>

<template>
  <div
    class="pane-splitter"
    :class="{ dragging }"
    role="separator"
    aria-orientation="vertical"
    :aria-valuenow="modelValue"
    :aria-valuemin="min"
    :aria-valuemax="max"
    @pointerdown.prevent="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="stopDrag"
    @pointercancel="stopDrag"
  >
    <span class="pane-splitter-grip" aria-hidden="true"></span>
  </div>
</template>
