<script setup>
import { computed, onMounted } from 'vue';
import { mcode, setMcodePluginTarget, setMcodePonytail } from '../kernel/mcode.js';
import { globalBusy, isLoading } from '../shell/loading.js';
import { refreshMemoryTargetUse, targetUseView } from '../memory/share.js';
import { minimaxPluginTarget } from './targetState.js';
const props = defineProps({ pluginId: { type: String, required: true }, name: { type: String, required: true } });
// 记忆的「接入」就是记忆页那份授权；这里只读它，不在插件行上再开写入口。
// 去记忆页的按钮属于「操作」列，由 PluginsPanel 统一渲染。
const memoryGranted = computed(() => {
  if (props.pluginId !== 'memory') return null;
  const view = targetUseView('mcode');
  return view ? Boolean(view.effective) : null;
});
const target = computed(() => minimaxPluginTarget(props.pluginId, mcode, !!mcode.view?.instance_id, memoryGranted.value));
// ponytail 有自己那条命令（带插件树事务与进度），其余内置插件走泛化的目标命令。
const busyKey = computed(() => (props.pluginId === 'ponytail' ? 'mcodePonytail' : 'mcodeTarget:' + props.pluginId));
onMounted(() => {
  if (props.pluginId === 'memory') refreshMemoryTargetUse();
});
function change(enabled) {
  if (!target.value || target.value.disabled || globalBusy.value) return;
  return props.pluginId === 'ponytail' ? setMcodePonytail(enabled) : setMcodePluginTarget(props.pluginId, enabled);
}
</script>
<template>
  <div class="minimax-target">
    <!-- 不预计接入的插件（target === null）不给复选框：留一枚中性「—」占住格子。
         禁用的勾选框会被读成「能开但当前不可用」，与「MiniMax 侧不会出现这个能力」
         是两回事；格子必须保留，否则后面的操作列会挪进 MiniMax 列。 -->
    <template v-if="target">
      <el-tooltip :content="target.tip" placement="top">
        <span class="target-control" tabindex="0" :aria-label="'MiniMax：' + target.tip">
          <el-button v-if="target.retryDisable" size="small" :disabled="target.disabled || globalBusy" :loading="isLoading(busyKey)"
            @click="change(false)">重试清理</el-button>
          <el-checkbox v-else size="small" :model-value="target.enabled" :disabled="target.disabled || globalBusy || isLoading(busyKey)"
            :aria-label="'MiniMax 接入 ' + name" @change="change"><span class="minimax-wordmark">{{ isLoading(busyKey) ? 'MiniMax · 处理中' : 'MiniMax' }}</span></el-checkbox>
          <span class="target-state">{{ target.text }}</span>
        </span>
      </el-tooltip>
    </template>
    <span v-else class="target-empty" :aria-label="'MiniMax 不接入 ' + name">—</span>
    <!-- 这里不再有第二枚「检查更新」：内置插件的更新按钮只属于上游 Ponytail
         本身，行内那一枚就够了。dsh-xlink 负责把同一份上游内容应用到 DSH 与
         MiniMax；MiniMax 工作台运行中无法替换插件文件，等停止后再检查。 -->
  </div>
</template>
<style scoped>
.minimax-target, .target-control { display: inline-flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.target-label { color: var(--text-secondary); font-size: var(--fs-4); }
.target-state { color: var(--text-muted); font-size: var(--fs-3); }
.target-empty { color: var(--text-muted); font-size: var(--fs-3); }
</style>
