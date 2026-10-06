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
import { isLoading } from '../shell/loading.js';
import {
  closeDiagnosis,
  diagnosticStore,
  loadOperationDiagnosis,
  loadPluginRun,
  loadStartupDiagnosis,
} from './diagnostics.js';
import { loadKernelStatusDiagnosis } from './diagnostic-actions.js';
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
const reload = computed(() => diagnosticStore.active?.kind !== 'plugin');

// 插件页在没有报告时退到「仅运行记录」视图（从概览进去的预检就是这种）。
// 那一栏要显示的是记录本身，说清「没有候选插件上下文」而不是显示
// 「未知插件」——后者暗示"知道是哪个但显示不出来"，而真相是不知道。
const pluginWithoutReport = computed(
  () =>
    diagnosticStore.active?.kind === 'plugin' &&
    !(diagnosticStore.active.spec && diagnosticStore.active.spec.report)
);

// 四个视图的刷新各用一个 loading key（它们读的是不同的东西），但头部只有
// 一个刷新按钮——把它做成四个条件的或会读不动，这里合成一个布尔。
const REFRESH_KEYS = [
  'startupDiagnosisReload',
  'operationDiagnosisReload',
  'kernelStatusReload',
  'precheckRunReload',
];
const refreshing = computed(() => REFRESH_KEYS.some((key) => isLoading(key)));

function back() {
  const panel = closeDiagnosis();
  // 回到打开诊断层之前的面板，而不是固定回概览——用户可能从「内核
  // 版本」点进来的，让他自己去点一次概览是白费一步。
  if (panel) store.activePanel = panel;
  emit('back');
}

/**
 * 头部的刷新。
 *
 * **四个视图共用这一个动作**（设计 §2.5.4：loading 只显示在触发它的按钮
 * 上）。此前内核视图的刷新落在这里是个**无操作**——菜单项和按钮都在，读
 * 什么也没发生（审查 P1-06）。
 */
function onRefresh() {
  const kind = diagnosticStore.active?.kind;
  // 按 id 拉：换「最近一条」会让用户在刷新里看到另一件事。
  if (kind === 'startup') loadStartupDiagnosis(diagnosticStore.active.runId, true);
  else if (kind === 'kernel') loadKernelStatusDiagnosis(true);
  else if (kind === 'plugin') loadPluginRun(diagnosticStore.active.runId, true);
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
      <!-- 头部刷新与页面内刷新是**同一个动作**（设计 §2.5.4），因此共用同一个
           loading key；此前这个按钮完全不转 loading，而页面底部的那个转，
           于是用户按了头部按钮之后没有任何反馈（审查 P2-03）。
           四个 key 都算进来：不同视图的刷新走不同的 key，图标却只有一个。 -->
      <button
        v-if="reload"
        class="diagnosis__icon-btn"
        type="button"
        :disabled="refreshing"
        :aria-busy="refreshing"
        aria-label="刷新诊断记录"
        :title="refreshing ? '正在刷新' : '刷新诊断记录'"
        @click="onRefresh"
      >
        <el-icon :size="16"><Refresh /></el-icon>
      </button>
      <!-- 更多菜单只收当前页面的**低频**动作（设计 §2.5.5）。会改变状态的
           操作（恢复、重启、删除）绝不藏在这里——它们必须以主操作或次要
           操作明确呈现，藏在「更多」里等于让用户在不知情下改状态。
           `teleported` 显式写出来而不是靠默认值：诊断层是 `position: fixed`
           且 `overflow: hidden`，弹层留在里面会被裁掉，而裁掉的样子是
           「菜单只剩最右边一条、还溢出窗口外」——比没有菜单更难懂。 -->
      <el-dropdown
        trigger="click"
        teleported
        placement="bottom-end"
        popper-class="diagnosis-more-popper"
        @command="(command) => onMore(command, onRefresh)"
      >
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
              <!-- 图标在文字**左边**，且是 flex 的固定项：菜单里三项有两项是复制，
                   只靠文字区分时「复制运行记录编号」与「复制本次诊断摘要」只差最后
                   三个字，扫读极易点错。`size` 走组件属性而不是 CSS font-size。 -->
              <el-icon v-if="item.icon" class="diagnosis-menu__icon" :size="14">
                <component :is="item.icon" />
              </el-icon>
              <span>{{ item.label }}</span>
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </header>

    <div class="diagnosis__body">
      <StartupDiagnosis v-if="kind === 'startup'" />
      <!-- 插件页只在**有报告**时渲染候选插件视图。报告只存在于刚才那次预检
           的返回值里，从概览/控制塔进来时没有——那时的正确答案是一条运行
           记录，不是「未知插件」（审查 P1-05）。 -->
      <OperationDiagnosis
        v-else-if="kind === 'plugin' && pluginWithoutReport"
        generic
      />
      <PluginDiagnosis v-else-if="kind === 'plugin'" />
      <OperationDiagnosis v-else-if="kind === 'restore' || kind === 'bisect'" />
      <KernelStatusDiagnosis v-else />
    </div>
  </section>
</template>