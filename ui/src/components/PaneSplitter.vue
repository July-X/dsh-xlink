<script setup>
// 可拖拽的分隔条：用于两个 flex 子项之间的边界。父组件持有 `v-model` 数值
// （被调整那侧的宽度），分隔条本身只负责鼠标交互与边界值裁剪。
//
// 性能要点：
// - pointer events + setPointerCapture：让浏览器把后续 move/up 全部派发到
//   这个元素上（即使光标跑出元素 / 出窗口），不再走 document 全局监听。
//   也避免 macOS 拖出浏览器标题栏后丢事件的常见坑。
// - requestAnimationFrame 节流：mousemove 在高 DPI 鼠标下能跑到 1000Hz，
//   但屏幕刷新 60Hz——多余的更新只会被下一帧覆盖掉；rAF 把 emit 收敛到每帧
//   一次，配合 transform / width 的 layout pass 也只走一次。
// - 不在拖拽期间持久化：父组件只更新响应式宽度，松手时再写 localStorage。
//   拖 1 秒可能产生 100 次 localStorage.setItem（同步 I/O），是最主要的卡顿源。
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
  // 拖向「被调整面板的更宽方向」为正 delta：左侧面板 = 向右拖为正
  const delta = props.side === 'left' ? e.clientX - startX : startX - e.clientX;
  const next = Math.max(props.min, Math.min(props.max, startWidth + delta));
  emit('update:modelValue', next);
}

function onPointerMove(e) {
  if (!dragging.value) return;
  lastEvent = e;
  // 已经排到下一帧就只更新坐标，等 rAF 触发；不重复排队
  if (rafId !== null) return;
  rafId = requestAnimationFrame(commitMove);
}

function stopDrag(e) {
  if (!dragging.value) return;
  dragging.value = false;
  document.body.style.cursor = '';
  document.body.style.userSelect = '';
  // 解除前先 flush 一次：避免最后一次 pointermove 没进 rAF 就被丢弃
  if (lastEvent) {
    commitMove();
  }
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  emit('drag-end');
  // 注意：不再主动 removeEventListener —— 配合 setPointerCapture 释放
  // 由 releasePointerCapture 完成，浏览器在 pointerup/cancel 时自动释放。
}

function onPointerDown(e) {
  // 只接受主键（左键 / 触屏单点）
  if (e.button !== undefined && e.button !== 0) return;
  dragging.value = true;
  startX = e.clientX;
  startWidth = props.modelValue;
  document.body.style.cursor = 'col-resize';
  document.body.style.userSelect = 'none';
  // 让浏览器把后续 pointermove / pointerup 都派发到这个分隔条上，光标
  // 跑出元素 / 跑出窗口都不丢事件；松手 / cancel 时浏览器自动释放。
  try {
    e.currentTarget.setPointerCapture(e.pointerId);
  } catch {
    // setPointerCapture 在某些 webview / 旧 Chromium 上会抛 InvalidStateError，
    // 兜底走 document 监听。
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
