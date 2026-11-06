import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';
import { confirmDialog, toastActionError } from '../shell/notify.js';
import { withProgress } from '../shell/progress.js';

export const mcode = reactive({ view: null, error: '', loading: false });

let requestSequence = 0;

function invalidateStatus() {
  requestSequence += 1;
  mcode.loading = false;
}

export async function refreshMcodeStatus(detect = false) {
  const request = ++requestSequence;
  mcode.loading = true;
  try {
    const view = await invoke(detect ? 'detect_mcode_installation' : 'get_mcode_status');
    if (request === requestSequence) {
      mcode.view = view;
      mcode.error = '';
    }
  } catch (error) {
    if (request === requestSequence) mcode.error = String(error);
  } finally {
    if (request === requestSequence) mcode.loading = false;
  }
}

export function detectMcode() {
  return withLoading('detectMcode', () => refreshMcodeStatus(true));
}

export function saveMcodeInstallation(packagePath, nodePath) {
  return withLoading('saveMcodeInstallation', async () => {
    invalidateStatus();
    try {
      await invoke('save_mcode_installation', {
        packagePath: packagePath.trim() ? packagePath : null,
        nodePath: nodePath.trim() ? nodePath : null,
      });
      await refreshMcodeStatus();
      return !mcode.error;
    } catch (error) {
      toastActionError('保存 MiniMax 运行环境失败', error);
      return false;
    }
  });
}

export async function startMcode() {
  invalidateStatus();
  const started = await withProgress(
    { cmd: 'start_mcode', start: '正在启动 MiniMax 工作台…', fail: '启动 MiniMax 工作台失败' },
    (onProgress) => ({ onProgress }),
  );
  await refreshMcodeStatus();
  return started;
}

export async function installMcode() {
  invalidateStatus();
  const installed = await withProgress(
    { cmd: 'install_mcode', start: '正在安装 MiniMax Code…', fail: '安装 MiniMax Code 失败' },
    (onProgress) => ({ onProgress }),
  );
  await refreshMcodeStatus();
  return installed;
}

export function openMcode() {
  return withLoading('openMcode', async () => {
    try {
      await invoke('open_mcode_workbench');
    } catch (error) {
      toastActionError('打开 MiniMax 工作台失败', error);
    }
    await refreshMcodeStatus();
  });
}

export async function stopMcodeForExit() {
  invalidateStatus();
  await invoke('stop_mcode');
  await refreshMcodeStatus();
}

export async function stopMcode() {
  return withLoading('stopMcode', async () => {
    const confirmed = await confirmDialog(
      '停止 MiniMax 工作台？',
      '停止服务会中断 MiniMax 正在执行的任务。',
      '停止服务',
    );
    if (!confirmed) return;
    try {
      await stopMcodeForExit();
    } catch (error) {
      toastActionError('停止 MiniMax 工作台失败', error);
      await refreshMcodeStatus();
    }
  });
}
