<script setup>
defineProps({
  shellClass: { type: String, required: true },
  inline: { type: Boolean, default: false },
});
</script>

<template>
  <div class="viewer-shell" :class="[shellClass, { 'viewer-shell--inline': inline }]">
    <slot />
  </div>
</template>

<style scoped>
.viewer-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  min-height: 0;
  overflow: hidden;
  background-color: var(--window);
}
/* 内嵌内容由管理面板滚动，不裁切趋势图的浮层。 */
.viewer-shell--inline {
  height: auto;
  min-height: 0;
  overflow: visible;
  background-color: transparent;
}
</style>
