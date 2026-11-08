<script setup>
import { onMounted, onUnmounted } from 'vue';
import { Info as InfoIcon } from '@lucide/vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import MemoryRow from './MemoryRow.vue';
import MemoryDetail from './MemoryDetail.vue';
import {
  memoryModule, moduleDetail, moduleRecordKey, refreshMemoryModule, resetMemoryModule,
  requestMemoryModuleEnabled, confirmMemoryModuleEnable, cancelMemoryModuleEnable,
  showModuleList, closeModuleList, openModuleDetail, closeModuleDetail,
} from './module.js';

const moduleTip = '独立授权当前 DSH 与 MiniMax 记忆空间后，两边工作台自动检索并注入符合项目范围的普通记忆。内容保留原位并标记来源，交接、经验和候选不在互通范围内。关闭只停止模块互通，原有实例权限及逐条授权仍保留；修改来源后需要重新授权。启用后重启 MiniMax 工作台；已有桥接的会话后续回合生效。统一列表只读，编辑请使用下方各实例面板。';
onMounted(refreshMemoryModule);
onUnmounted(resetMemoryModule);
</script>

<template>
  <div class="card memory-module">
    <div class="card-head">
      <h2>通用记忆模块
        <el-tooltip :content="moduleTip" placement="bottom-start">
          <span class="card-info-icon" tabindex="0" :aria-label="moduleTip"><InfoIcon :size="16" /></span>
        </el-tooltip>
      </h2>
      <el-switch :model-value="!!memoryModule.status?.enabled"
        active-text="授权自动互通" :loading="isLoading('memoryModuleEnable')"
        :disabled="globalBusy || memoryModule.busy || !memoryModule.status || (!memoryModule.status.enabled && !memoryModule.status.pair_key)"
        @change="requestMemoryModuleEnabled" />
      <el-button text :disabled="memoryModule.busy || !memoryModule.status" @click="showModuleList">统一记忆（{{ memoryModule.records.length }}）</el-button>
      <el-button text :loading="isLoading('memoryModuleRefresh')" :disabled="globalBusy || memoryModule.busy" @click="refreshMemoryModule">刷新</el-button>
    </div>
    <div v-if="memoryModule.error || memoryModule.status?.warning" class="memory-module__sources" role="status">
      <p class="memory-note">{{ memoryModule.error || memoryModule.status.warning }}</p>
      <el-button text type="danger" :loading="isLoading('memoryModuleEnable')" :disabled="globalBusy || memoryModule.busy" @click="requestMemoryModuleEnabled(false)">撤销模块授权</el-button>
    </div>
    <div v-else class="memory-module__sources">
      <el-tag v-for="source in memoryModule.status?.sources || []" :key="source.namespace" size="small" effect="plain" :title="source.namespace">来源：{{ source.label }}</el-tag>
      <span class="memory-note">{{ memoryModule.busy ? '正在读取…' : !memoryModule.status ? '尚未读取' : memoryModule.status.enabled ? '已授权互通' : '未授权互通' }}</span>
    </div>
    <el-dialog :model-value="!!memoryModule.confirmation" title="授权记忆模块自动互通" width="min(620px, 92vw)" append-to-body :close-on-click-modal="false" @update:model-value="open => !open && cancelMemoryModuleEnable()">
      <p>允许以下两个记忆空间的普通记忆双向自动检索、注入各自的工作台。通用偏好可跨项目，项目记忆仍按项目范围匹配。无需逐条授权，内容不会搬迁。</p>
      <p v-for="source in memoryModule.confirmation?.sources || []" :key="source.namespace" class="memory-note">{{ source.label }}：{{ source.namespace }}</p>
      <p>此授权独立于旧的实例开关和逐条授权。关闭模块后，旧权限仍保留。启用后重启 MiniMax 工作台；已有桥接的会话后续回合生效。真实对话注入请以工作台记录为准。</p>
      <template #footer>
        <el-button @click="cancelMemoryModuleEnable">取消</el-button>
        <el-button type="primary" :loading="isLoading('memoryModuleEnable')" :disabled="globalBusy || memoryModule.busy" @click="confirmMemoryModuleEnable">授权并启用</el-button>
      </template>
    </el-dialog>
    <el-dialog :model-value="memoryModule.listVisible" title="统一记忆 · 只读" width="min(720px, 92vw)" append-to-body @update:model-value="open => !open && closeModuleList()">
      <p v-if="!memoryModule.records.length" class="memory-note">{{ memoryModule.busy ? '正在读取…' : memoryModule.error ? '读取失败，请关闭窗口后刷新。' : '当前来源还没有普通记忆。' }}</p>
      <div v-else class="page-list memory-module__list">
        <div v-for="item in memoryModule.records" :key="moduleRecordKey(item)">
          <p class="memory-note" :title="item.source_namespace">来源：{{ item.origin_label }} · {{ item.source_namespace }}</p>
          <MemoryRow :row="item.record" @open="openModuleDetail(item)" />
        </div>
      </div>
    </el-dialog>
    <MemoryDetail v-if="moduleDetail" :row="moduleDetail.record" readonly :source-label="moduleDetail.origin_label" :source-namespace="moduleDetail.source_namespace" @close="closeModuleDetail" />
  </div>
</template>

<style scoped>
.memory-module { flex: none; }
.memory-module__sources { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.memory-module__list { max-height: 55vh; overflow-y: auto; }
</style>
