import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';
import { confirmDialog, toast, toastActionError, toastSuccess } from '../shell/notify.js';
import { withProgress } from '../shell/progress.js';

const BUILD_MARKER = 'XLINK_BUILD_MARKER_9A3F';
if (typeof console !== 'undefined') console.warn(BUILD_MARKER);

export const mcode = reactive({ view: null, error: '', loading: false, builtins: [], catalogError: '', ponytailUpdate: null, ponytailUpdateError: '', installedVersions: [], availableVersions: [], installedVersionsError: '', availableVersionsError: '', versionsLoaded: false });

let requestSequence = 0;

export async function loadMcodeBuiltins() {
  try {
    mcode.builtins = await invoke('list_mcode_builtin_catalog');
    mcode.catalogError = '';
  } catch (error) {
    mcode.catalogError = String(error);
  }
}

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
      if (mcode.view?.instance_id !== view?.instance_id || !view?.ponytail_enabled || view?.ponytail_error) {
        mcode.ponytailUpdate = null;
        mcode.ponytailUpdateError = '';
      }
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

export function saveMcodePorts(portRaw, miniappPortRaw) {
  const port = Number(portRaw);
  const miniappPort = Number(miniappPortRaw);
  if (!Number.isInteger(port) || port < 1024 || port > 65535
    || !Number.isInteger(miniappPort) || miniappPort < 1024 || miniappPort > 65535) {
    toast('端口需为 1024–65535 的整数，当前输入无法保存', 5000, 'warning');
    return Promise.resolve(false);
  }
  return withLoading('saveMcodePorts', async () => {
    try {
      await invoke('save_mcode_ports', { port, miniappPort });
      await refreshMcodeStatus();
      toastSuccess('MiniMax 端口已保存（重启工作台后生效）');
      return true;
    } catch (error) {
      toastActionError('保存 MiniMax 端口失败', error);
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
  if (installed) await loadMcodeInstalledVersions();
  return installed;
}

export async function loadMcodeInstalledVersions() {
  try {
    mcode.installedVersions = await invoke('list_mcode_installed_versions');
    mcode.installedVersionsError = '';
  } catch (error) {
    mcode.installedVersions = [];
    mcode.installedVersionsError = String(error);
  }
}

export function loadMcodeAvailableVersions() {
  return withLoading('mcodeVersions', async () => {
    mcode.availableVersionsError = '';
    mcode.availableVersions = [];
    mcode.versionsLoaded = false;
    try {
      mcode.availableVersions = await invoke('list_mcode_available_versions');
      mcode.versionsLoaded = true;
    } catch (error) {
      mcode.availableVersionsError = String(error);
    }
  });
}

export function setMcodePonytail(enabled) {
  return withLoading('mcodePonytail', async () => {
    const instanceId = mcode.view?.instance_id;
    if (!instanceId) return false;
    mcode.ponytailUpdate = null;
    mcode.ponytailUpdateError = '';
    const result = await withProgress(
      { cmd: 'set_mcode_ponytail', start: enabled ? '正在准备 MiniMax Ponytail…' : '正在撤销 MiniMax Ponytail 授权…', fail: '修改 MiniMax Ponytail 授权失败' },
      onProgress => ({ instanceId, enabled, onProgress }),
    );
    await refreshMcodeStatus();
    return result;
  });
}

export function checkMcodePonytailUpdate() {
  return withLoading('mcodePonytailUpdate', async () => {
    const instanceId = mcode.view?.instance_id;
    if (!instanceId) return false;
    mcode.ponytailUpdate = null;
    mcode.ponytailUpdateError = '';
    const result = await withProgress({ cmd: 'check_mcode_ponytail_update', start: '正在检查 Ponytail 更新…',
      fail: '检查 MiniMax Ponytail 更新失败', onResult: value => {
        if (mcode.view?.instance_id === instanceId && mcode.view?.ponytail_enabled && !mcode.view?.ponytail_error) mcode.ponytailUpdate = value;
      } }, onProgress => ({ instanceId, onProgress }));
    if (!result && mcode.view?.instance_id === instanceId) mcode.ponytailUpdateError = '更新状态不可用，请查看进度和日志。';
    return result;
  });
}

export function upgradeMcodePonytail() {
  return withLoading('mcodePonytailUpgrade', async () => {
    const instanceId = mcode.view?.instance_id;
    if (!instanceId) return false;
    let changed = false;
    const result = await withProgress(
      { cmd: 'upgrade_mcode_ponytail', start: '正在更新 MiniMax Ponytail…', fail: '更新 MiniMax Ponytail 失败', onResult: value => { changed = value; } },
      onProgress => ({ instanceId, onProgress }),
    );
    if (mcode.view?.instance_id === instanceId && result) {
      mcode.ponytailUpdate = null;
      mcode.ponytailUpdateError = '';
      toastSuccess(changed ? 'Ponytail 已更新' : 'Ponytail 已是当前版本');
    }
    await refreshMcodeStatus();
    return result;
  });
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

/// 旧运行记录无法证明后代进程已退出，只能由用户确认后清理；壳只删记录、不结束进程。
export function discardMcodeRunRecord() {
  return withLoading('discardMcodeRunRecord', async () => {
    const confirmed = await confirmDialog(
      '清理无法确认的旧运行记录？',
      '壳无法证明上次启动的 MiniMax 进程及其派生的进程都已退出。请先在任务管理器确认没有 MiniMax / node 残留进程——两个端口都释放后才会清理。这里只删除记录，不结束任何进程。',
      '确认清理',
    );
    if (!confirmed) return false;
    try {
      const pid = await invoke('discard_mcode_run_record');
      toastSuccess(pid ? '旧运行记录已清理（进程号 ' + pid + '）' : '旧运行记录已清理');
      await refreshMcodeStatus();
      return true;
    } catch (error) {
      toastActionError('清理旧运行记录失败', error);
      await refreshMcodeStatus();
      return false;
    }
  });
}
