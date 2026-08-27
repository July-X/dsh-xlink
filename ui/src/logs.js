// 日志查看面板的状态与动作：<data_dir>/logs/ 下每个 *.log 一个侧签，
// 打开时读最新文件（按文件名排序，kernel.log 是实时输出），切签按需读取，
// 「刷新」重读当前签。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastError } from './notify.js';
import { withLoading } from './loading.js';
import { stripAnsi } from './progress.js';

export const logModal = reactive({
  visible: false,
  files: [],
  activeName: null,
  content: '',
  loading: false,
});

export function formatLogSize(bytes) {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / (1024 * 1024)).toFixed(2) + ' MB';
}

export function loadActiveLog() {
  if (!logModal.activeName) {
    logModal.content = '（暂无日志）';
    return Promise.resolve();
  }
  const target = logModal.activeName;
  logModal.content = '读取中…';
  logModal.loading = true;
  // withLoading 让「刷新 / 切签读文件」点亮标题栏脉冲，与全局面按钮语义一致。
  return withLoading('logRead:' + target, () =>
    invoke('read_log_file', { name: target })
      .then((text) => {
        // 读取期间用户可能已切签：只有仍在当前签时才落内容。
        if (logModal.activeName !== target) return;
        // 落盘日志是 pnpm/tsdown 的原始终端输出，含 ANSI 颜色码；
        // 按双轨约定磁盘保留原文、展示前剥离（同进度浮层的实时流）。
        logModal.content = stripAnsi(text || '') || '（暂无内容）';
      })
      .catch((e) => {
        logModal.content = '读取失败：' + e;
      })
      .finally(() => {
        logModal.loading = false;
      })
  );
}

export function switchLogTab(name) {
  if (name === logModal.activeName) {
    return loadActiveLog();
  }
  logModal.activeName = name;
  return loadActiveLog();
}

// 重新列文件，让新安装日志与轮转后的 kernel.log 出现；当前签还在就留在
// 原签，否则退回第一个签（或空态）。
export function refreshLogTabs() {
  return invoke('list_log_files')
    .then((files) => {
      logModal.files = files || [];
      const names = logModal.files.map((f) => f.name);
      const keep = logModal.activeName && names.includes(logModal.activeName) ? logModal.activeName : null;
      logModal.activeName = keep || names[0] || null;
      if (logModal.activeName) {
        return loadActiveLog();
      }
      logModal.content = '（暂无日志文件）';
      return null;
    })
    .catch((e) => toastError('读取日志列表失败：' + e, 4000));
}

export function showLogs() {
  logModal.visible = true;
  logModal.activeName = null;
  refreshLogTabs();
}

export function hideLogs() {
  logModal.visible = false;
}
