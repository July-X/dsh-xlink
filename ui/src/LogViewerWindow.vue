<script setup>
// 独立日志阅读窗口：主面板「全屏」按钮经 open_log_window 命令弹出，
// URL 查询串 ?log=<name> 指定文件（main.js 据此挂载本页而非管理壳）。
// 页头是文件名 + 刷新 / 关闭；刷新走 read_log_file 重读文件尾部。
import { onMounted, ref, watchEffect } from 'vue';
import { Refresh, Close } from '@element-plus/icons-vue';
import { invoke, windowAction } from './bridge.js';
import { toastError } from './notify.js';
import { ioActive, isLoading, withLoading } from './loading.js';
import { displayLogText } from './logs.js';

const name = new URLSearchParams(location.search).get('log') || '';
const content = ref('读取中…');

function load() {
  if (!name) {
    content.value = '（缺少日志文件名）';
    return Promise.resolve();
  }
  return withLoading('logwinRead', () =>
    invoke('read_log_file', { name })
      .then((text) => {
        content.value = displayLogText(text);
      })
      .catch((e) => {
        content.value = '读取失败：' + e;
        toastError('读取失败：' + e);
      })
  );
}

function closeWindow() {
  // 与标题栏一致：走 bridge 并在失败时提示，而不是静默什么都不做（P2-39）。
  windowAction('close').catch((error) => {
    const detail = error && error.message ? error.message : String(error);
    toastError(`关闭窗口失败：${detail}。可改用系统快捷键（macOS Cmd+W，Windows Alt+F4）。`);
  });
}

// 与管理壳一致：本窗口内 IO 进行中点亮标题栏鲸眼脉冲。
watchEffect(() => {
  document.body.classList.toggle('pulse-active', ioActive.value);
});

onMounted(load);
</script>

<template>
  <div class="logwin">
    <header class="logwin-head">
      <img src="/whale-icon.png" alt="" width="22" height="22" />
      <span class="logwin-title" :title="name">{{ name || '日志' }}</span>
      <span style="flex: 1"></span>
      <el-button text :icon="Refresh" :loading="isLoading('logwinRead')" title="重新读取当前日志" @click="load">
        刷新
      </el-button>
      <el-button text :icon="Close" @click="closeWindow">关闭</el-button>
    </header>
    <pre class="log-content logwin-content">{{ content }}</pre>
  </div>
</template>
