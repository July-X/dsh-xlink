<script setup>
import { computed, onMounted } from 'vue';
import { mcode, setMcodePonytail, checkMcodePonytailUpdate, upgradeMcodePonytail } from '../kernel/mcode.js';
import { globalBusy, isLoading } from '../shell/loading.js';
import { refreshMemoryTargetUse, targetUseView } from '../memory/share.js';
import { minimaxPluginTarget } from './targetState.js';
const props = defineProps({ pluginId: { type: String, required: true }, name: { type: String, required: true } });
// 记忆的「接入」就是记忆页那份授权；这里只读它，不在插件行上再开一个写入口。
const memoryGranted = computed(() => {
  if (props.pluginId !== 'memory') return null;
  const view = targetUseView('mcode');
  return view ? Boolean(view.effective) : null;
});
const target = computed(() => minimaxPluginTarget(props.pluginId, mcode, !!mcode.view?.instance_id, memoryGranted.value));
onMounted(() => {
  if (props.pluginId === 'memory') refreshMemoryTargetUse();
});
function change(enabled) {
  if (!target.value.disabled && !globalBusy.value) return setMcodePonytail(enabled);
}
</script>
<template>
  <div class="minimax-target">
    <el-tooltip :content="target.tip" placement="top">
      <span class="target-control" tabindex="0" :aria-label="'MiniMax：' + target.tip">
        <el-checkbox size="small" :model-value="target.enabled" :disabled="target.disabled || globalBusy || isLoading('mcodePonytail')"
          :aria-label="'MiniMax 接入 ' + name" @change="change"><span class="minimax-wordmark">{{ isLoading('mcodePonytail') && pluginId === 'ponytail' ? 'MiniMax · 处理中' : 'MiniMax' }}</span></el-checkbox>
        <span class="target-state">{{ target.text }}</span>
      </span>
    </el-tooltip>
    <!-- MiniMax 侧的内容更新只在这个目标真的可操作时出现：运行中的「需先停止」、
         状态不可用等禁用态下面，这枚按钮点不动，却与行内上游那枚「检查更新」
         同名同排，读起来像多出来的第二枚。文案带上 MiniMax，是因为同一行已经
         有一枚查 DSH 上游内容的「检查更新」，两件事不能共用一个名字。 -->
    <template v-if="pluginId === 'ponytail' && target.enabled && !target.disabled">
      <el-button text size="small" :disabled="globalBusy" :loading="isLoading('mcodePonytailUpdate')" aria-label="检查 MiniMax 的 Ponytail 更新" @click="checkMcodePonytailUpdate">检查 MiniMax 更新</el-button>
      <span v-if="mcode.ponytailUpdate" class="target-state">{{ mcode.ponytailUpdate.update_available ? '有更新' : '已是最新' }}</span>
      <span v-if="mcode.ponytailUpdateError" class="target-state" :title="mcode.ponytailUpdateError">检查失败</span>
      <el-button v-if="mcode.ponytailUpdate?.update_available" text size="small" :disabled="globalBusy" :loading="isLoading('mcodePonytailUpgrade')" @click="upgradeMcodePonytail">更新</el-button>
    </template>
  </div>
</template>
<style scoped>
.minimax-target, .target-control { display: inline-flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.target-label { color: var(--text-secondary); font-size: var(--fs-4); }
.target-state { color: var(--text-muted); font-size: var(--fs-3); }
</style>
