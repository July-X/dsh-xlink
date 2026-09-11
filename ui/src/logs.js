// 日志查看面板的状态与动作：<data_dir>/logs/ 下每个 *.log 一个侧签，
// 打开时读最新文件（按文件名排序，kernel.log 是实时输出），切签按需读取，
// 「刷新」重读当前签。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastActionError } from './notify.js';
import { withLoading } from './loading.js';
import { stripAnsi } from './progress.js';

export const logModal = reactive({
  visible: false,
  files: [],
  activeName: null,
  content: '',
  // 正在读取的文件名（读一个签时只有那个签显示加载态）。旧实现是一个全局
  // boolean，切签或刷新时整块面板都在转圈，且 `withLoading` 对同一个 key
  // 的重入直接忽略 —— 大日志还没读完时再点「刷新」什么都不发生（P2-38）。
  loadingName: null,
});

// 请求序号：只有最后一次发起的读取可以落内容，避免慢的旧响应盖掉新结果。
let logReadSeq = 0;

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
  const seq = ++logReadSeq;
  logModal.content = '读取中…';
  logModal.loadingName = target;
  // 这里**不**用 `withLoading`：它对同一个 key 的重入直接忽略，会导致
  // 「正在读大日志时再点刷新毫无反应」。改为每次真正发起读取，用序号保证
  // 只有最新一次的结果落地（P2-38）。标题栏脉冲仍由 `withLoading` 的
  // 全局语义提供，这里用同一个 key 的空壳调用即可。
  const pulse = withLoading('logRead', () => Promise.resolve());
  return invoke('read_log_file', { name: target })
    .then((text) => {
      // 读取期间用户可能已切签，或有更新的请求在飞：只有最新一次落内容。
      if (seq !== logReadSeq || logModal.activeName !== target) return;
      // 落盘日志是 pnpm/tsdown 的原始终端输出，含 ANSI 颜色码；
      // 按双轨约定磁盘保留原文、展示前剥离（同进度浮层的实时流）。
      logModal.content = stripAnsi(text || '') || '（暂无内容）';
    })
    .catch((e) => {
      if (seq !== logReadSeq) return;
      logModal.content = '读取失败：' + e + '。可点击「刷新」重试，或打开数据目录查看 logs/。';
    })
    .finally(() => {
      if (seq === logReadSeq) logModal.loadingName = null;
      return pulse;
    });
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
    .catch((e) =>
      toastActionError('读取日志列表失败', e, '请点击「刷新」重试，或打开数据目录查看 logs/', 4000)
    );
}

export function showLogs() {
  logModal.visible = true;
  logModal.activeName = null;
  refreshLogTabs();
}

export function hideLogs() {
  logModal.visible = false;
}
