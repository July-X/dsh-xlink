<script setup>
import { computed, onMounted } from 'vue';
import { mcode, setMcodePonytail } from '../kernel/mcode.js';
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
    <!-- 这里不再有第二枚「检查更新」：内置插件的更新按钮只属于上游 Ponytail
         本身，行内那一枚就够了。dsh-xlink 负责把同一份上游内容应用到 DSH 与
         MiniMax；MiniMax 工作台运行中无法替换插件文件，等停止后再检查。 -->
  </div>
</template>
<style scoped>
.minimax-target, .target-control { display: inline-flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.target-label { color: var(--text-secondary); font-size: var(--fs-4); }
.target-state { color: var(--text-muted); font-size: var(--fs-3); }
</style>
