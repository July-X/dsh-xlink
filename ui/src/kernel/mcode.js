import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withExclusiveLoading, withLoading } from '../shell/loading.js';
import { confirmDialog, toast, toastActionError, toastSuccess } from '../shell/notify.js';
import { withProgress } from '../shell/progress.js';

const BUILD_MARKER = 'XLINK_BUILD_MARKER_9A3F';
if (typeof console !== 'undefined') console.warn(BUILD_MARKER);

export const mcode = reactive({ view: null, error: '', cleanupError: '', cleanupErrorSnapshot: null, loading: false, builtins: [], catalogError: '', ponytailUpdate: null, ponytailUpdateError: '', installedVersions: [], availableVersions: [], installedVersionsError: '', availableVersionsError: '', versionsLoaded: false });

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

export async function refreshMcodeStatus() {
  const request = ++requestSequence;
  mcode.loading = true;
  try {
    const view = await invoke('get_mcode_status');
    if (request === requestSequence) {
      if (mcode.view?.instance_id !== view?.instance_id || !view?.ponytail_enabled || view?.ponytail_error) {
        mcode.ponytailUpdate = null;
        mcode.ponytailUpdateError = '';
      }
      if (view?.recovery_snapshot !== mcode.cleanupErrorSnapshot) {
        mcode.cleanupError = '';
        mcode.cleanupErrorSnapshot = null;
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

/**
 * 重新探测 MiniMax 使用的 Node（概览页「重新检测」在 MiniMax 页签下）。
 *
 * 后端的解析结果按 settings.node_path 缓存在进程里，状态轮询会一直命中旧结论；
 * 所以先让后端清缓存再拉一次状态。loading key 与 DSH 的 `detectNode` 共用：
 * 按钮是同一个，转圈不该分内核。
 */
export function refreshMcodeNode() {
  return withLoading('detectNode', async () => {
    try {
      await invoke('refresh_mcode_node');
      await refreshMcodeStatus();
      toastSuccess('已重新检测 Node.js');
    } catch (error) {
      toastActionError('重新检测 Node.js 失败', error);
    }
  });
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

export async function installMcode(version) {
  invalidateStatus();
  const installed = await withProgress(
    { cmd: 'install_mcode', start: version ? `正在安装 MiniMax Code ${version}…` : '正在安装 MiniMax Code…', fail: '安装 MiniMax Code 失败' },
    (onProgress) => ({ onProgress, version }),
  );
  await refreshMcodeStatus();
  if (installed) await loadMcodeInstalledVersions();
  return installed;
}

/// 删除一个没在用的已装版本（与 DSH 内核的「删除」同一条：运行中、当前选用
/// 的版本都会被后端拒绝，这里只负责确认后的调用与列表刷新）。
export function removeMcodeVersion(version) {
  return withLoading('removeMcodeVersion:' + version, async () => {
    try {
      await invoke('remove_mcode_version', { version });
      toastSuccess('已删除 MiniMax 版本 ' + version);
      await refreshMcodeStatus();
      await loadMcodeInstalledVersions();
      await loadMcodeAvailableVersions();
      return true;
    } catch (error) {
      toastActionError('删除 MiniMax 版本失败', error, '请先停止工作台，并确认该版本没有被选用');
      return false;
    }
  });
}

export function loadMcodeInstalledVersions() {
  // 进入版本页和手动重试共用一个请求，加载期间不把空缓存当作未安装。
  return withLoading('mcodeInstalledVersions', async () => {
    try {
      mcode.installedVersions = await invoke('list_mcode_installed_versions');
      mcode.installedVersionsError = '';
    } catch (error) {
      mcode.installedVersions = [];
      mcode.installedVersionsError = String(error);
    }
  });
}

// 官方发布列表：与 DSH 的启动自检同一口径。启动时静默预取一次；成功拿到后，
// 版本页进入 / 切回只读缓存，不再自动重拉——否则每次切回都打一次网络、把
// 「正在获取官方发布列表…」再闪一遍（用户 2026-11-10 报「官方版本经常二次加载」）。
// 手动刷新传 `force` 才重新请求。
export function loadMcodeAvailableVersions({ force = false } = {}) {
  if (!force && mcode.versionsLoaded) return Promise.resolve();
  return withLoading('mcodeVersions', async () => {
    mcode.availableVersionsError = '';
    // 已有列表时刷新不清空也不收起：先把一屏版本号抹掉再等网络，切页签与点刷新
    // 都会闪一下空白。失败才回到「未获取」——留着上一份会被当成刚查到的结果。
    if (!mcode.availableVersions.length) mcode.versionsLoaded = false;
    try {
      mcode.availableVersions = await invoke('list_mcode_available_versions');
      mcode.versionsLoaded = true;
    } catch (error) {
      mcode.availableVersions = [];
      mcode.versionsLoaded = false;
      mcode.availableVersionsError = String(error);
    }
  });
}

/// 共享技能库在 MiniMax 这一路的接入状态（可见状态词）。技能页与概览的
/// 「系统健康 → 技能注册」共用这一份映射：两处各写一份，改一边另一边就会静默失配。
export function mcodeSkillsState(view) {
  const state = view?.skills_state;
  if (state === 'failed') return '同步失败';
  if (state === 'pending_verification') return '已接入 · 待验证';
  if (state === 'legacy_unavailable') return '旧实例未接入';
  if (state === 'platform_unavailable') return '平台不支持';
  return view?.running ? '已接入' : '已接入 · 待启动';
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

/// 用户确认已重启电脑后，只清理确认时的旧记录和占位。
export function discardMcodeRunRecord() {
  return withExclusiveLoading('discardMcodeRunRecord', async () => {
    const view = mcode.view;
    if (!view?.instance_id || !view.recovery_snapshot) return false;
    const target = { instanceId: view?.instance_id, expectedSnapshot: view.recovery_snapshot };
    const confirmed = await confirmDialog(
      '确认已重启电脑？',
      `实例 ${target.instanceId} 留有无法确认的旧运行记录或占位。请先重启电脑，并确保其他窗口没有运行 MiniMax。程序无法验证是否重启；此操作只删除记录和占位，不结束任何进程。未重启或仍有任务运行时不要继续。`,
      '已重启，清理记录',
    );
    if (!confirmed) return false;
    invalidateStatus();
    mcode.cleanupError = '';
    mcode.cleanupErrorSnapshot = null;
    try {
      await invoke('discard_mcode_run_record', target);
      toastSuccess('旧运行记录和占位已清理');
      return true;
    } catch (error) {
      mcode.cleanupError = String(error);
      mcode.cleanupErrorSnapshot = target.expectedSnapshot;
      toastActionError('清理旧运行记录失败', error);
      return false;
    } finally {
      await refreshMcodeStatus();
    }
  });
}
