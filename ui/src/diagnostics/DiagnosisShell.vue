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
import { ArrowLeft, MoreFilled, Refresh } from '@element-plus/icons-vue';
import { store } from '../store.js';
import {
  closeDiagnosis,
  diagnosticStore,
  loadOperationDiagnosis,
  loadStartupDiagnosis,
} from './diagnostics.js';
import { moreItems, onMore } from './diagnosis-more-menu.js';
import { kindLabel } from './diagnostic-labels.js';
import StartupDiagnosis from './StartupDiagnosis.vue';
import PluginDiagnosis from './PluginDiagnosis.vue';
import KernelStatusDiagnosis from './KernelStatusDiagnosis.vue';
import OperationDiagnosis from './OperationDiagnosis.vue';

const emit = defineEmits(['back']);

// 诊断层可能在**启动进行中**被打开（用户在进度面板里点了「查看启动诊断」）。
// 那时实时事件流还在往里灌，切页回来要接着看——所以离开页面不重置流，
// 只有「重新打开一次新的运行」才清（见 `openStartupDiagnosis`）。
// kind 切换时按需拉取。`startup` 才需要拉——它的数据来自通道攒下的运行
// 记录；插件与内核视图读的是 store 里已有的状态。
//
// `restore` / `bisect` 同样要拉：它们的记录是「做完之后回头看」的，入口又
// 开在刚做完那件事旁边，进来时手上没有 id，得按 kind 去问最近一条。
const NEEDS_FETCH = ['startup', 'restore', 'bisect'];
/** 模板与 watch 共用同一份当前 kind——两份各读一次，早晚会在某次改动里
 *  读到不同的值，而症状是「点了没反应」那种极难查的错。 */
const kind = computed(() => diagnosticStore.active?.kind || '');

watch(
  () => diagnosticStore.active?.kind,
  (next) => {
    if (!NEEDS_FETCH.includes(next) || diagnosticStore.currentRun) return;
    if (next === 'startup') loadStartupDiagnosis(diagnosticStore.active.runId);
    else loadOperationDiagnosis(diagnosticStore.active.runId, next);
  },
  { immediate: true }
);

const title = computed(() => {
  if (kind.value === 'plugin') return kindLabel('plugin-precheck');
  if (kind.value === 'kernel') return kindLabel('kernel');
  // 恢复与排查共用一个视图组件，但标题必须说清是哪一次操作——两者落在
  // 同一层覆盖层里，不分标题的话用户返回时不知道自己从哪儿出来的。
  return kindLabel(kind.value);
});

const subtitle = computed(() => {
  if (diagnosticStore.active?.kind === 'plugin') {
    const name = diagnosticStore.active.spec?.name || diagnosticStore.active.spec?.id;
    return name ? String(name) : '';
  }
  return '';
});

// 恢复 / 排查的记录是在操作**做完之后**才成型的（恢复要等自检、排查要等
// 结论），所以头部这个刷新按钮对它们同样有意义——排查跑完自动回来、或者
// 用户从另一个窗口看着它跑完，都靠这一下。
const reload = computed(() => NEEDS_FETCH.includes(diagnosticStore.active?.kind));

function back() {
  const panel = closeDiagnosis();
  // 回到打开诊断层之前的面板，而不是固定回概览——用户可能从「内核
  // 版本」点进来的，让他自己去点一次概览是白费一步。
  if (panel) store.activePanel = panel;
  emit('back');
}

function onRefresh() {
  const kind = diagnosticStore.active?.kind;
  // 两种 kind 各自按 id 拉：换「最近一条」会让用户在刷新里看到另一件事。
  if (kind === 'startup') loadStartupDiagnosis(diagnosticStore.active.runId, true);
  else if (reload.value) loadOperationDiagnosis(diagnosticStore.active.runId, kind, true);
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
      <!-- 更多菜单只收当前页面的**低频**动作（设计 §2.5.5）。会改变状态的
           操作（恢复、重启、删除）绝不藏在这里——它们必须以主操作或次要
           操作明确呈现，藏在「更多」里等于让用户在不知情下改状态。 -->
      <el-dropdown trigger="click" @command="(command) => onMore(command, onRefresh)">
        <button
          class="diagnosis__icon-btn"
          type="button"
          aria-label="更多操作"
          title="更多操作"
        >
          <el-icon :size="16"><MoreFilled /></el-icon>
        </button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item v-for="item in moreItems" :key="item.key" :command="item.key">
              {{ item.label }}
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </header>

    <div class="diagnosis__body">
      <StartupDiagnosis v-if="diagnosticStore.active?.kind === 'startup'" />
      <PluginDiagnosis v-else-if="diagnosticStore.active?.kind === 'plugin'" />
      <OperationDiagnosis
        v-else-if="kind === 'restore' || kind === 'bisect'"
      />
      <KernelStatusDiagnosis v-else />
    </div>
  </section>
</template>