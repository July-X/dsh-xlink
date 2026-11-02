<script setup>
// 页头图标：与左侧菜单同一枚（图标表在 `pageIcons.js`，侧栏与页头共用）。
// 2026-11-02 用户要求：页头的 01/02 编号换成菜单对应的图标，两边保持一致。
import { computed } from 'vue';
import { PAGE_ICONS } from './pageIcons.js';

const props = defineProps({
  page: { type: String, required: true },
});

const icon = computed(() => PAGE_ICONS[props.page] || null);
</script>

<template>
  <el-icon v-if="icon" class="page-head__icon"><component :is="icon" /></el-icon>
</template>