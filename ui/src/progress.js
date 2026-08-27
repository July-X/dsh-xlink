// 长任务进度面板：所有安装 / 卸载 / 同步 / 启停共用一个全屏浮层。
// Rust 侧把每条 pnpm 输出行经 Channel 推过来；行是原始终端文本，
// 展示前剥离 ANSI 转义，完整原始日志始终落盘 <data_dir>/logs/*.log。
//
// 失败约定：进度面板保持开放，由用户手动点「关闭」收起（catch 后不自动隐藏）。
import { reactive } from 'vue';
import { invoke, makeChannel } from './bridge.js';
import { toastSuccess, toastError } from './notify.js';
import { setBusy } from './loading.js';
import { refreshAll } from './store.js';

const INSTALL_LOG_MAX_LINES = 400;

// 日志行缓冲不进响应式系统：高频行到达时只在 rAF 里翻一次 tick，
// 组件按 tick 重算展示文本，避免每行触发一轮渲染。
let logLines = [];
let logScheduled = false;

// CSI（ESC [ ... 字母）、OSC（ESC ] ... BEL/ESC\）与孤立的 ESC 序列。
// 日志面板与独立阅读窗口共用：磁盘文件保留原始 ANSI，展示前剥离。
export function stripAnsi(text) {
  return text
    .replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)/g, '')
    .replace(/\x1B[@-_]/g, '');
}

function scheduleFlush() {
  if (logScheduled) return;
  logScheduled = true;
  requestAnimationFrame(() => {
    logScheduled = false;
    progress.logTick += 1;
  });
}

export const progress = reactive({
  visible: false,
  text: '',
  failed: false,
  logTick: 0,

  get logText() {
    // 读取 logTick 建立依赖；真正文本来自模块级缓冲。
    void this.logTick;
    return logLines.join('\n');
  },

  set(text) {
    this.text = text;
    this.visible = true;
  },

  hide() {
    this.visible = false;
  },

  fail(text) {
    this.failed = true;
    this.set(text);
    this.appendLog('—— ' + text + ' ——');
  },

  appendLog(line) {
    if (!line) return;
    logLines.push(stripAnsi(line));
    if (logLines.length > INSTALL_LOG_MAX_LINES) {
      logLines.splice(0, logLines.length - INSTALL_LOG_MAX_LINES);
    }
    scheduleFlush();
  },

  resetLog() {
    logLines = [];
    this.failed = false;
    scheduleFlush();
  },

  close() {
    this.failed = false;
    this.hide();
    this.resetLog();
  },
});

// 长任务统一入口：channel 阶段消息进日志区与进度文案，busy 全程持锁，
// 成功 toast + 刷新，失败保持面板开放。resolve(true/false) 供调用方追加提示。
export function withProgress(labels, task) {
  const channel = makeChannel((msg) => {
    progress.appendLog(msg);
    progress.set(msg.length > 60 ? msg.slice(0, 57) + '…' : msg);
  });
  setBusy(true);
  progress.resetLog();
  progress.set(labels.start);
  return invoke(labels.cmd, task(channel))
    .then(async () => {
      if (labels.done) {
        toastSuccess(labels.done);
      }
      await refreshAll();
      return true;
    })
    .catch(async (e) => {
      const failLabel = labels.fail || '操作失败';
      progress.fail(failLabel + '：' + e);
      toastError(labels.failToast || '操作失败，详情见进度窗口与日志', 6000);
      await refreshAll();
      return false;
    })
    .finally(() => {
      setBusy(false);
      if (!progress.failed) {
        progress.hide();
      }
    });
}
