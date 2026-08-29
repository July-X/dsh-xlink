// 长任务进度面板：所有安装 / 卸载 / 同步 / 启停共用一个全屏浮层。
// Rust 侧把每条 pnpm 输出行经 Channel 推过来；行是原始终端文本，
// 展示前剥离 ANSI，完整原始日志始终落盘 <data_dir>/logs/*.log。
// 失败约定：进度面板保持开放，由用户手动点「关闭」收起。
import { reactive } from 'vue';
import { invoke, makeChannel } from './bridge.js';
import { toastSuccess, toastError } from './notify.js';
import { withExclusive } from './loading.js';
import { refreshAll } from './store.js';

const INSTALL_LOG_MAX_CHARS = 512 * 1024;
const INSTALL_LOG_MAX_LINE_CHARS = 16 * 1024;

// 日志行缓冲不进响应式系统：高频行到达时只在 rAF 里翻一次 tick，
// 同时保留一个缓存字符串，避免每次模板求值都重新 join 全部日志。
let logLines = [];
let logChars = 0;
let logText = '';
let logScheduled = false;

// CSI（ESC [ ... 字母）、OSC（ESC ] ... BEL/ESC\）与孤立的 ESC 序列。
// 磁盘文件保留原始 ANSI，展示前剥离。
export function stripAnsi(text) {
  return String(text)
    .replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)/g, '')
    .replace(/\x1B[@-_]/g, '');
}

function scheduleFlush() {
  if (logScheduled) return;
  logScheduled = true;
  const flush = () => {
    logScheduled = false;
    logText = logLines.join('\n');
    progress.logTick += 1;
  };
  if (typeof requestAnimationFrame === 'function') {
    requestAnimationFrame(flush);
  } else {
    queueMicrotask(flush);
  }
}

export const progress = reactive({
  visible: false,
  text: '',
  failed: false,
  logTick: 0,

  get logText() {
    void this.logTick;
    return logText;
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
    let clean = stripAnsi(line);
    if (!clean) return;
    if (clean.length > INSTALL_LOG_MAX_LINE_CHARS) {
      clean = clean.slice(0, INSTALL_LOG_MAX_LINE_CHARS - 12) + '… [行已截断]';
    }
    logLines.push(clean);
    logChars += clean.length + (logLines.length > 1 ? 1 : 0);
    while (logLines.length > 1 && logChars > INSTALL_LOG_MAX_CHARS) {
      const removed = logLines.shift();
      logChars -= removed.length + 1;
    }
    scheduleFlush();
  },

  resetLog() {
    logLines = [];
    logChars = 0;
    logText = '';
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
export function withProgress(labels, task, options = {}) {
  const channel = makeChannel((msg) => {
    progress.appendLog(msg);
    progress.set(msg.length > 60 ? msg.slice(0, 57) + '…' : msg);
  });
  const execute = async () => {
    progress.resetLog();
    progress.set(labels.start);
    try {
      await invoke(labels.cmd, task(channel));
      if (labels.done) {
        toastSuccess(labels.done);
      }
      await refreshAll();
      return true;
    } catch (e) {
      const failLabel = labels.fail || '操作失败';
      progress.fail(failLabel + '：' + e);
      toastError(labels.failToast || '操作失败，详情见进度窗口与日志', 6000);
      await refreshAll();
      return false;
    } finally {
      if (!progress.failed) {
        progress.hide();
      }
    }
  };
  const run = options.exclusive === false ? execute() : withExclusive(execute);
  return run === undefined ? Promise.resolve(false) : run;
}
