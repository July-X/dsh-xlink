<script setup>
import { computed, onMounted, onUnmounted } from 'vue';
import { InfoFilled } from '@element-plus/icons-vue';
import { mcodeExperience } from '../kernel/experience.js';
import { globalBusy } from '../shell/loading.js';
import { memoryPool, poolTarget, refreshMemoryPool, resetMemoryPool, requestPoolTarget, confirmPoolTarget, cancelPoolTarget } from './pool.js';

const targets = computed(() => [
  { key: 'dsh', label: 'DSH', brand: 'deepseek-wordmark' },
  ...(mcodeExperience.value ? [{ key: 'mcode', label: 'MiniMax', brand: 'minimax-wordmark' }] : []),
]);
const tip = '会话记忆先进入待审核，通过后加入公共记忆池。启用的内核按通用 / 同项目范围自动引用。关闭只停止该内核后续使用，不删除记录，也不影响另一内核；已经发送到会话的内容不能收回。标记「沿用旧范围」的开关保持升级前行为，关闭后再次启用时确认公共池范围。';
function targetText(key) {
  if (memoryPool.error || poolTarget(key)?.warning) return '状态未知';
  if (!poolTarget(key)) return '读取中';
  if (key === 'mcode' && !memoryPool.status?.sources?.some(item => item.label === 'MiniMax')) return '未配置实例';
  if (key === 'dsh' && poolTarget(key).effective) {
    if (memoryPool.dshConnection === 'unknown') return '已选择 · 接入状态未知';
    if (memoryPool.dshConnection === 'missing') return '待接入';
    if (memoryPool.dshConnection === 'restart' || memoryPool.connectionNote) return '已接入 · 需重启';
  }
  if (poolTarget(key).pool_enabled) return '已启用';
  return poolTarget(key).effective ? '沿用旧范围' : '未启用';
}
function unavailable(key) {
  return globalBusy.value || memoryPool.busy || !!memoryPool.changing || !!memoryPool.error || !poolTarget(key)
    || (!memoryPool.status?.pair_key && !poolTarget(key)?.effective)
    || (key === 'mcode' && !memoryPool.status?.sources?.some(item => item.label === 'MiniMax'));
}
onMounted(refreshMemoryPool);
onUnmounted(resetMemoryPool);
</script>

<template>
  <div class="memory-module">
    <div class="memory-module__usage">
      <span class="memory-module__label">启用记忆模块</span>
      <span v-for="target in targets" :key="target.key" class="memory-target">
        <span :class="['memory-target__name', target.brand]">{{ target.label }}</span>
        <span class="memory-target__state">{{ targetText(target.key) }}</span>
        <el-switch :model-value="!!poolTarget(target.key)?.effective" size="small"
          :loading="memoryPool.changing === target.key" :disabled="unavailable(target.key)"
          :aria-label="`为 ${target.label} 启用记忆模块`" @change="enabled => requestPoolTarget(target.key, enabled)" />

      </span>
      <el-tooltip placement="bottom-start" :show-after="80" :content="tip">
        <el-icon class="head-tip-icon" tabindex="0" :aria-label="tip"><InfoFilled /></el-icon>
      </el-tooltip>
      <el-button v-if="memoryPool.error || memoryPool.status?.warning" text :disabled="memoryPool.busy || !!memoryPool.changing" @click="refreshMemoryPool">刷新状态</el-button>
    </div>
    <p v-if="memoryPool.error || memoryPool.status?.warning" class="memory-note" role="status">{{ memoryPool.error || memoryPool.status.warning }}</p>
    <p v-if="memoryPool.connectionNote" class="memory-note" role="status">{{ memoryPool.connectionNote }}</p>
    <p v-for="target in memoryPool.targets.filter(item => item.warning)" :key="target.target" class="memory-note" role="status">{{ target.warning }}</p>
    <el-dialog :model-value="!!memoryPool.confirmation" title="启用公共记忆池" width="min(620px, 92vw)" append-to-body :close-on-click-modal="false" @update:model-value="open => !open && cancelPoolTarget()">
      <p>允许 {{ memoryPool.confirmation?.target === 'mcode' ? 'MiniMax' : 'DSH' }} 自动引用以下来源中已审核的普通记忆。通用偏好可跨项目，项目记忆仍只用于同一项目。</p>
      <p v-for="source in memoryPool.confirmation?.sources || []" :key="source.namespace" class="memory-note">{{ source.label }}：{{ source.namespace }}</p>
      <p>DSH 启用时会自动接入本地记忆插件；首次接入后需重新启动工作台。启用后也会自动提取会话候选，等待人工审核；MiniMax 暂未支持自动提取。引用的记忆会随对话请求发送给所选模型；远端模型会收到这些内容。</p>
      <p>此选择包含这些来源中现有及以后审核通过的普通记忆，另一内核的开关不会被打开。旧记录仍保存在原位置，不搬迁、不删除。MiniMax 首次启用后需要重启工作台。</p>
      <template #footer>
        <el-button @click="cancelPoolTarget">取消</el-button>
        <el-button type="primary" :disabled="globalBusy || memoryPool.busy || !!memoryPool.changing" @click="confirmPoolTarget">确认并启用</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.memory-module { flex: none; }
.memory-module__usage, .memory-target { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
.memory-module__usage { gap: 8px 16px; }
.memory-module__label, .memory-target__state { color: var(--text-secondary); font-size: var(--fs-4); }
.memory-target__name { font-size: var(--fs-5); }
</style>
