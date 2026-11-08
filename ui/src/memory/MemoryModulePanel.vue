<script setup>
import { onMounted, onUnmounted, ref } from 'vue';
import { mcodeExperience } from '../kernel/experience.js';
import { globalBusy, isLoading } from '../shell/loading.js';
import {
  memoryModule, refreshMemoryModule, resetMemoryModule,
  requestMemoryModuleEnabled, confirmMemoryModuleEnable, cancelMemoryModuleEnable,
} from './module.js';

const emit = defineEmits(['manage-shares']);
const visible = ref(false);

function changeModule(enabled) {
  if (enabled) visible.value = false;
  requestMemoryModuleEnabled(enabled);
}

function manageShares() {
  visible.value = false;
  emit('manage-shares');
}

onMounted(refreshMemoryModule);
onUnmounted(resetMemoryModule);
</script>

<template>
  <div class="memory-module">
    <div class="memory-module__usage">
      <span class="memory-module__label">允许使用记忆</span>
      <slot name="usage" />
      <el-button class="memory-module__permissions" @click="visible = true">授权范围</el-button>
    </div>
    <el-dialog v-model="visible" title="授权范围" width="min(620px, 92vw)" append-to-body>
      <div class="memory-module__line">
        <span>双向普通记忆互通</span>
        <el-switch :model-value="!!memoryModule.status?.enabled"
          :loading="isLoading('memoryModuleEnable')"
          :disabled="globalBusy || memoryModule.busy || !memoryModule.status || (!memoryModule.status.enabled && (!memoryModule.status.pair_key || !mcodeExperience))"
          aria-label="授权当前实例的普通记忆互通" @change="changeModule" />
      </div>
      <p>授权后，当前两个记忆空间的普通记忆可按通用 / 同项目范围双向检索；候选、交接和经验不互通。记录仍保存在来源空间。</p>
      <p v-for="source in memoryModule.status?.sources || []" :key="source.namespace" class="memory-note">{{ source.label }}：{{ source.namespace }}</p>
      <p>关闭互通不会撤销历史逐条授权。启用后需重启 MiniMax 工作台，真实对话注入仍待验证。</p>
      <div v-if="memoryModule.error || memoryModule.status?.warning" class="memory-module__warning" role="status">
        <p class="memory-note">{{ memoryModule.error || memoryModule.status.warning }}</p>
        <el-button text type="danger" :loading="isLoading('memoryModuleEnable')" :disabled="globalBusy || memoryModule.busy" @click="requestMemoryModuleEnabled(false)">撤销模块授权</el-button>
      </div>
      <slot name="legacy" />
      <div class="memory-module__line">
        <span>历史逐条授权</span>
        <el-button text @click="manageShares">查看与管理</el-button>
      </div>
      <template #footer>
        <el-button :loading="isLoading('memoryModuleRefresh')" :disabled="globalBusy || memoryModule.busy" @click="refreshMemoryModule">刷新状态</el-button>
        <el-button @click="visible = false">关闭</el-button>
      </template>
    </el-dialog>
    <el-dialog :model-value="!!memoryModule.confirmation" title="授权记忆模块自动互通" width="min(620px, 92vw)" append-to-body :close-on-click-modal="false" @update:model-value="open => !open && cancelMemoryModuleEnable()">
      <p>允许以下两个记忆空间的普通记忆双向自动检索、注入各自的工作台。通用偏好可跨项目，项目记忆仍按项目范围匹配。无需逐条授权，内容不会搬迁。</p>
      <p v-for="source in memoryModule.confirmation?.sources || []" :key="source.namespace" class="memory-note">{{ source.label }}：{{ source.namespace }}</p>
      <p>此授权独立于旧的实例开关和逐条授权。关闭模块后，旧权限仍保留。启用后重启 MiniMax 工作台；已有桥接的会话后续回合生效。真实对话注入请以工作台记录为准。</p>
      <template #footer>
        <el-button @click="cancelMemoryModuleEnable">取消</el-button>
        <el-button type="primary" :loading="isLoading('memoryModuleEnable')" :disabled="globalBusy || memoryModule.busy" @click="confirmMemoryModuleEnable">授权并启用</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.memory-module { flex: none; }
.memory-module__usage, .memory-module__line { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 16px; }
.memory-module__label { color: var(--text-secondary); font-size: var(--fs-4); }
.memory-module__permissions { margin-left: auto; }
.memory-module__line { justify-content: space-between; }
.memory-module__warning { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
</style>
