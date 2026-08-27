<script setup>
// 长任务进度浮层：阶段文案 + 实时日志流 + 鲸眼扫光。
// 失败时保持开放（含「关闭」按钮），成功自动收起。
import { nextTick, ref, watch } from 'vue';
import { Close } from '@element-plus/icons-vue';
import { progress } from '../progress.js';

const logBox = ref(null);

// 新行到达时滚到底部（rAF 刷新 tick，见 progress.js）。
watch(
  () => progress.logTick,
  async () => {
    await nextTick();
    if (logBox.value) {
      logBox.value.scrollTop = logBox.value.scrollHeight;
    }
  }
);
</script>

<template>
  <div v-if="progress.visible" class="progress-overlay">
    <div class="progress-body">
      <p class="progress-text">{{ progress.text || '正在处理…' }}</p>
      <div v-if="progress.logText" ref="logBox" class="install-log">
        <pre>{{ progress.logText }}</pre>
      </div>
      <div class="progress-pulse" aria-hidden="true"></div>
      <div v-if="progress.failed" class="btn-row">
        <el-button type="primary" :icon="Close" @click="progress.close()">关闭</el-button>
      </div>
    </div>
  </div>
</template>
