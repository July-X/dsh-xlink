<script setup>
// 长任务进度浮层：阶段文案 + 实时日志流 + 鲸眼扫光。
// 失败时保持开放（含「关闭」按钮），成功自动收起。
import { nextTick, ref, watch } from 'vue';
import { Close } from '@element-plus/icons-vue';
import { store } from '../store.js';
import { progress } from './progress.js';
import { openStartupDiagnosis, diagnosticStore } from '../diagnostics/diagnostics.js';

/**
 * 启动任务进行中，且此刻有结构化阶段事件时，给一个「查看启动诊断」入口。
 *
 * 实时事件流不在浮层里展示——它按文档是**独立页面**的内容。浮层只给入口，
 * 这样用户在启动还没结束时就能看到已经走到哪一步，不必等失败弹窗。
 */
function openDiagnosis() {
  openStartupDiagnosis(diagnosticStore.active?.runId || '', store.activePanel);
}

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
      <!-- 日志区**不随有无输出出现 / 消失**：浮层高度是固定的（见 theme.css
           的 `.progress-body`），这里凭空少一块会让第一帧留出一段空白，等日志
           到达再补上又是一次跳动。空的时候放一句占位。 -->
      <div ref="logBox" class="install-log">
        <pre v-if="progress.logText">{{ progress.logText }}</pre>
        <p v-else class="muted install-log-empty">等待输出…</p>
      </div>
      <div class="progress-pulse" aria-hidden="true"></div>
      <div class="btn-row dialog-actions">
        <el-button
          v-if="progress.failed && diagnosticStore.liveEvents.length"
          text
          @click="openDiagnosis"
        >
          查看启动诊断
        </el-button>
        <el-button
          :type="progress.failed ? 'primary' : 'default'"
          :icon="Close"
          @click="progress.close()"
        >
          关闭
        </el-button>
      </div>
    </div>
  </div>
</template>
