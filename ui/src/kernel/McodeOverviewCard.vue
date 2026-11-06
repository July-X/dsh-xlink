<script setup>
import { computed, onMounted, onUnmounted } from 'vue';
import { globalBusy, isLoading } from '../shell/loading.js';
import { mcode, openMcode, refreshMcodeStatus, startMcode, stopMcode } from './mcode.js';
const emit = defineEmits(['open-install']);

const busy = computed(() => globalBusy.value || mcode.loading || isLoading('openMcode') || isLoading('stopMcode'));
const status = computed(() => {
  if (!mcode.view) return mcode.loading ? '正在检测' : '状态不可用';
  if (mcode.view.running) return '运行中';
  if (mcode.view.stop_required) return '需要完成停止';
  return mcode.view.installed ? '已停止' : '需要安装';
});
let timer;
onMounted(() => {
  refreshMcodeStatus();
  timer = setInterval(() => { if (!globalBusy.value) refreshMcodeStatus(); }, 5000);
});
onUnmounted(() => clearInterval(timer));
</script>

<template>
  <div class="card">
    <div class="card-head"><h2>当前内核</h2></div>
    <p>MiniMax Code {{ mcode.view?.package_version || '' }} · {{ status }}</p>
    <p v-if="mcode.error || mcode.view?.error" role="alert">{{ mcode.error || mcode.view?.error }}</p>
    <div class="mcode-actions">
      <el-button v-if="mcode.view?.running" :disabled="busy" :loading="isLoading('openMcode')" @click="openMcode">{{ mcode.view.window_open ? '切到工作台' : '打开工作台' }}</el-button>
      <el-button v-if="mcode.view?.stop_required" :disabled="busy" :loading="isLoading('stopMcode')" @click="stopMcode">{{ mcode.view.running ? '停止服务' : '重试停止' }}</el-button>
      <el-button v-else type="primary" :disabled="busy || !mcode.view?.installed" @click="startMcode">启动工作台</el-button>
      <el-button v-if="mcode.view && !mcode.view.installed" :disabled="busy" @click="emit('open-install')">安装内核</el-button>
    </div>
  </div>
</template>

<style scoped>
.mcode-actions { display: flex; flex-wrap: wrap; gap: 8px; }
.mcode-actions :deep(.el-button + .el-button) { margin-left: 0; }
</style>
