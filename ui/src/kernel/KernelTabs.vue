<script setup>
// 顶部工作条（全局 chrome）：48px 一行，标题栏正下方、侧栏与内容区之上——
// 侧栏品牌、菜单和下方所有面板都归属当前选中 tab 指向的内核；接入 mcode 等
// 新内核族时这里多一个 tab，布局不用再动。右半边常驻「当前实例 + 运行状态」，
// 用户在任何页面都看得到，不必先回概览。
//
// tab 只显示内核族名（DSH / mcode）：注册表实例 id（如 default）是实现细节，
// 多内核并存且都能同时启动时，id 不构成用户需要关心的差异，故不再展示。
//
// 一个族只画一个 tab。同族多实例是常态而不是边角：dev 壳的 default-dev 与
// release 壳的 default 都属 dsh 族，两个都注册进同一份注册表后，这里会画出
// 两个一模一样、看上去还都「已启用」的 DSH——既分不清谁是谁，也像有两个内
// 核并列。同族多实例时保留**当前选中的那个**：它是这一族此刻真正在服务的实
// 例，选中态不会因为去重落到别人头上。去重只作用于这排页签，插件页「所有
// 实例」仍拿到完整实例列表。
import { computed, onMounted } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { store, refreshAll } from '../store.js';
import { globalBusy, isLoading, withLoading } from '../shell/loading.js';
import { instanceStore, loadInstances, setDefaultInstance, familyLabel } from './instance.js';
import { toastActionError } from '../shell/notify.js';

// 按内核族去重，同族保留选中项；排序键从实例 id 换成族名——去重之后决定顺序的
// 是族，而族不随点击变化，连续点击仍不会让目标移位。
const instanceTabs = computed(() => {
  const byFamily = new Map();
  for (const it of instanceStore.list) {
    const family = it.record.kernel_family;
    const kept = byFamily.get(family);
    if (!kept || it.record.id === instanceStore.defaultInstanceId) {
      byFamily.set(family, it);
    }
  }
  return [...byFamily.values()]
    .map((it) => ({
      id: it.record.id,
      family: familyLabel(it.record.kernel_family),
      // 去重后同族实例的差异只能靠 hover 得知（「默认实例（dev 壳）」这类
      // 实例标签）；标签文字本身仍是族名，不因多一个实例就变宽。
      title: it.record.label || it.record.id,
    }))
    .sort((a, b) => a.family.localeCompare(b.family));
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

// 当前实例的人读名字（实例 id 是实现细节，标签才给人看）。页签显示的是内核
// 族名，而同族多实例时真正区分「在服务的是哪一个」的是实例标签——它放在工作条
// 右侧常驻，页签只管族。
const activeInstanceLabel = computed(() => {
  const current = instanceStore.list.find(
    (it) => it.record.id === instanceStore.defaultInstanceId
  );
  return current ? current.record.label || current.record.id : '';
});

// 运行状态胶囊：运行中 / 已停止 / 未安装 / 加载中。判据与概览页「当前内核」卡
// 同源（store.view.kernel），两处读同一个字段，不各算一份。
const kernelStatus = computed(() => {
  const k = store.view && store.view.kernel;
  if (!k) return { text: '加载中…', cls: '' };
  if (k.running) return { text: '运行中', cls: 'ok' };
  if (k.active && k.active_installed) return { text: '已停止', cls: 'bad' };
  return { text: '未安装', cls: '' };
});
</script>

<template>
  <div class="workspace-bar">
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
          :title="tab.title"
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
    <!-- 右半边：当前实例上下文 + 运行状态。两者都是「一句话交代此刻状态」，
         放在这里意味着用户在任何页面都看得到，不必先回概览。 -->
    <div class="workspace-bar__context">
      <span v-if="activeInstanceLabel" class="workspace-bar__instance" :title="activeInstanceLabel">
        {{ activeInstanceLabel }}
      </span>
      <span class="status-pill">
        <span class="dot" :class="kernelStatus.cls"></span>
        <span>{{ kernelStatus.text }}</span>
      </span>
    </div>
  </div>
</template>

<style scoped>
/* 工作条：48px 一行，左边内核族页签，右边当前实例 + 运行状态。
   旧版这行是「贴在标题栏下的透明页签」，靠 dev/release 背景渐变显形——那层
   渐变已随新设计删除，页签若还透明就会和侧栏底色糊在一起，所以这里给它自己的
   chrome 底与下边框。 */
.workspace-bar {
  flex: 0 0 var(--workspace-bar-height);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  height: var(--workspace-bar-height);
  padding: 0 16px;
  background: var(--window);
  border-bottom: 1px solid var(--border);
}

.workspace-bar__context {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
}

.workspace-bar__instance {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-muted);
  font-size: 12px;
}

/* 内核多到放不下时允许横向滚动，但滚动条一律隐藏（负 margin 页签在滚动
   容器里还会产生 1px 幽灵竖向滚动条），避免工作条右侧出现滚动槽。 */
.kernel-tabs {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
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
  padding: 5px 12px;
  border: none;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.14s ease, color 0.14s ease;
}
.kernel-tab:hover:not(.is-active) {
  background: var(--surface-subtle);
  color: var(--text);
}
.kernel-tab.is-active {
  background: var(--surface-subtle);
  color: var(--text);
  font-weight: 600;
}
.kernel-tab:disabled {
  cursor: default;
  opacity: 0.6;
}
.kernel-tabs__note {
  color: var(--text-muted);
  font-size: 12px;
}
</style>
