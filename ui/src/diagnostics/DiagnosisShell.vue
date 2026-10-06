<script setup>
// 诊断层外壳：覆盖当前面板，不开新窗口。
//
// **为什么不另开窗口**：启动失败时用户正要回到日志、换端口、回退快照，
// 另开窗口会让这些动作变成跨窗口来回拖。一个覆盖层留住原面板的上下文，
// 「返回」也只需一步。
//
// 头部结构固定为 [返回] 标题 [主操作] [更多]：标题单行省略，保证 480 宽下
// 不被按钮压掉。返回是纯图标按钮，所以**必须带 aria-label**——它没有
// 可见文字，靠形状猜不出是返回。
import { computed, watch } from 'vue';
import { ArrowLeft, Refresh } from '@element-plus/icons-vue';
import { store } from '../store.js';
import { closeDiagnosis, diagnosticStore, loadStartupDiagnosis } from './diagnostics.js';
import { kindLabel } from './diagnostic-labels.js';
import StartupDiagnosis from './StartupDiagnosis.vue';
import PluginDiagnosis from './PluginDiagnosis.vue';
import KernelStatusDiagnosis from './KernelStatusDiagnosis.vue';

const emit = defineEmits(['back']);

// 诊断层可能在**启动进行中**被打开（用户在进度面板里点了「查看启动诊断」）。
// 那时实时事件流还在往里灌，切页回来要接着看——所以离开页面不重置流，
// 只有「重新打开一次新的运行」才清（见 `openStartupDiagnosis`）。
// kind 切换时按需拉取。`startup` 才需要拉——它的数据来自通道攒下的运行
// 记录；插件与内核视图读的是 store 里已有的状态。
watch(
  () => diagnosticStore.active?.kind,
  (kind) => {
    if (kind === 'startup' && !diagnosticStore.currentRun) {
      loadStartupDiagnosis(diagnosticStore.active.runId);
    }
  },
  { immediate: true }
);

const title = computed(() => {
  const kind = diagnosticStore.active?.kind;
  if (kind === 'startup') return kindLabel('startup');
  if (kind === 'plugin') return kindLabel('plugin-precheck');
  return kindLabel('kernel');
});

const subtitle = computed(() => {
  if (diagnosticStore.active?.kind === 'plugin') {
    const name = diagnosticStore.active.spec?.name || diagnosticStore.active.spec?.id;
    return name ? String(name) : '';
  }
  return '';
});

const reload = computed(() => diagnosticStore.active?.kind === 'startup');

function back() {
  const panel = closeDiagnosis();
  // 回到打开诊断层之前的面板，而不是固定回概览——用户可能从「内核
  // 版本」点进来的，让他自己去点一次概览是白费一步。
  if (panel) store.activePanel = panel;
  emit('back');
}

function onRefresh() {
  if (diagnosticStore.active?.kind === 'startup') {
    loadStartupDiagnosis(diagnosticStore.active.runId, true);
  }
}

</script>

<template>
  <section class="diagnosis" aria-label="诊断">
    <header class="diagnosis__head">
      <button
        class="diagnosis__icon-btn"
        type="button"
        aria-label="返回"
        title="返回"
        @click="back"
      >
        <el-icon :size="17"><ArrowLeft /></el-icon>
      </button>
      <h2 class="diagnosis__title">
        {{ title }}<span v-if="subtitle" class="diag-card__aside"> · {{ subtitle }}</span>
      </h2>
      <button
        v-if="reload"
        class="diagnosis__icon-btn"
        type="button"
        aria-label="刷新诊断记录"
        title="刷新诊断记录"
        @click="onRefresh"
      >
        <el-icon :size="16"><Refresh /></el-icon>
      </button>
    </header>

    <div class="diagnosis__body">
      <StartupDiagnosis v-if="diagnosticStore.active?.kind === 'startup'" />
      <PluginDiagnosis v-else-if="diagnosticStore.active?.kind === 'plugin'" />
      <KernelStatusDiagnosis v-else />
    </div>
  </section>
</template>