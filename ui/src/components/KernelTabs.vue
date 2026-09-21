<script setup>
// 顶部内核 tab（全局 chrome）：一排内核页签挂在标题栏正下方、侧栏与内容区
// 之上——侧栏品牌、菜单和下方所有面板都归属当前选中 tab 指向的内核；接入
// mcode 等新内核族时这里多一个 tab，布局不用再动。
//
// tab 只显示内核族名（DSH / mcode）：注册表实例 id（如 default）是实现细节，
// 多内核并存且都能同时启动时，id 不构成用户需要关心的差异，故不再展示。
import { computed, onMounted } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { store, refreshAll } from '../store.js';
import { globalBusy, isLoading, withLoading } from '../loading.js';
import { instanceStore, loadInstances, setDefaultInstance, familyLabel } from '../instance.js';
import { toastActionError } from '../notify.js';

// 标签按实例 id 稳定排序，选中项不换位，避免连续点击时目标移动。
const instanceTabs = computed(() => {
  const list = instanceStore.list.slice();
  list.sort((a, b) => {
    return a.record.id.localeCompare(b.record.id);
  });
  return list.map((it) => ({
    id: it.record.id,
    family: familyLabel(it.record.kernel_family),
  }));
});

onMounted(() => {
  loadInstances().catch(() => {});
});

async function pickInstance(id) {
  if (store.starting || globalBusy.value) return;
  try {
    await setDefaultInstance(id, () => refreshAll({ fresh: true }));
  } catch (e) {
    toastActionError('切换默认实例失败', e);
  }
}

const retryInstances = () => withLoading('loadInstances', () => loadInstances().catch((e) => {
  toastActionError('读取实例列表失败', e);
}));
</script>

<template>
  <nav class="kernel-tabs" aria-label="内核切换">
    <template v-if="instanceTabs.length > 0">
      <button
        v-for="tab in instanceTabs"
        :key="tab.id"
        type="button"
        class="kernel-tab"
        :class="{ 'is-active': tab.id === instanceStore.defaultInstanceId }"
        :disabled="instanceStore.switching || globalBusy || store.starting"
        :aria-busy="isLoading('switchInstance')"
        :aria-current="tab.id === instanceStore.defaultInstanceId ? 'true' : undefined"
        @click="pickInstance(tab.id)"
      >
        {{ tab.family }}
      </button>
    </template>
    <span v-else-if="instanceStore.loaded !== false" class="kernel-tabs__note">加载中…</span>
    <span v-else-if="instanceStore.error" class="kernel-tabs__note">内核列表读取失败</span>
    <span v-else class="kernel-tabs__note">未设置默认内核</span>
    <el-button v-if="instanceStore.error" text size="small" :icon="Refresh" :loading="isLoading('loadInstances')" :disabled="globalBusy" @click="retryInstances">重试</el-button>
  </nav>
</template>

<style scoped>
/* 下划线式页签（与 el-tabs 的视觉语言一致）：tab 行本身不自画底色和分隔线——
   dev 红 / release 绿版本带从 body 一路铺上来，这行必须彻底透明才不会把渐变
   切成两段；激活项用品牌色下划线标出下方整个界面归属的内核。 */
.kernel-tabs {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 14px;
  /* 内核多到放不下时允许横向滚动，但滚动条一律隐藏（负 margin 页签在滚动
     容器里还会产生 1px 幽灵竖向滚动条），避免顶栏右侧出现滚动槽。 */
  overflow-x: auto;
  scrollbar-width: none;
}
.kernel-tabs::-webkit-scrollbar {
  display: none;
}
.kernel-tab {
  appearance: none;
  flex-shrink: 0;
  white-space: nowrap;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  padding: 8px 14px;
  color: var(--text-muted);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: color 0.1s ease, border-color 0.1s ease;
}
.kernel-tab:hover:not(.is-active) {
  color: var(--text);
}
.kernel-tab.is-active {
  color: var(--text);
  border-bottom-color: var(--el-color-primary);
}
.kernel-tab:disabled {
  cursor: default;
  opacity: 0.6;
}
.kernel-tabs__note {
  color: var(--text-muted);
  font-size: 12px;
  padding: 8px 0;
}
</style>
