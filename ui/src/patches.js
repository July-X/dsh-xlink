// 内置补丁（随 dsh-xlink 发布包捆绑的内核补丁 / 小插件）的共享状态与动作。
// 与社区插件不同：补丁默认不生效，由用户在设置页自主选择应用到当前内核，
// 可随时撤销（从备份还原原文件）。状态与写入都在 Rust 侧完成。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toast, toastSuccess, toastError, confirmDialog } from './notify.js';
import { withExclusiveLoading } from './loading.js';

export const patchStore = reactive({
  view: null,
  loaded: false,
});

let patchesInFlight = null;

// 静默刷新：读取失败保留旧卡片，下次进入设置页自动重试。
export function refreshPatches() {
  if (patchesInFlight) return patchesInFlight;
  const request = invoke('patch_status')
    .then((view) => {
      patchStore.view = view;
      patchStore.loaded = true;
    })
    .catch(() => {});
  const tracked = request.finally(() => {
    if (patchesInFlight === tracked) patchesInFlight = null;
  });
  patchesInFlight = tracked;
  return tracked;
}

export function applyPatch(id, name) {
  return confirmDialog(
    '应用补丁？',
    '将把「' + name + '」应用到当前内核并修改内核文件（应用前会自动备份原文件，可随时撤销还原）。操作前请先关闭工作台。',
    '应用补丁'
  ).then((ok) => {
    if (!ok) return Promise.resolve(false);
    return withExclusiveLoading('patchApply:' + id, async () => {
      try {
        const notes = await invoke('patch_apply', { id });
        toastSuccess('补丁「' + name + '」已应用（重启工作台后生效）');
        if (notes && notes.length) {
          notes.forEach((n) => toast(n, 6000, 'warning'));
        }
        await refreshPatches();
        return true;
      } catch (e) {
        toastError('应用补丁失败：' + e, 8000);
        return false;
      }
    });
  });
}

export function revertPatch(id, name) {
  return confirmDialog(
    '撤销补丁？',
    '将撤销「' + name + '」对当前内核的修改，并还原补丁前的文件。操作前请先关闭工作台。',
    '撤销补丁'
  ).then((ok) => {
    if (!ok) return Promise.resolve(false);
    return withExclusiveLoading('patchRevert:' + id, async () => {
      try {
        const warnings = await invoke('patch_revert', { id });
        toastSuccess('补丁「' + name + '」已撤销');
        if (warnings && warnings.length) {
          warnings.forEach((w) => toast(w, 8000, 'warning'));
        }
        await refreshPatches();
        return true;
      } catch (e) {
        toastError('撤销补丁失败：' + e, 8000);
        return false;
      }
    });
  });
}