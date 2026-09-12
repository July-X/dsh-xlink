// 内置补丁（随 dsh-xlink 发布包捆绑的内核补丁 / 小插件）的共享状态与动作。
// 与社区插件不同：补丁默认不生效，由用户在设置页自主选择应用到当前内核，
// 可随时撤销（从备份还原原文件）。状态与写入都在 Rust 侧完成。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toast, toastSuccess, toastActionError, confirmDialog } from './notify.js';
import { withExclusiveLoading } from './loading.js';
import { createStatusSource } from './async.js';

export const patchStore = reactive({
  view: null,
  loaded: false,
});

// 静默刷新：读取失败保留旧卡片，下次进入设置页自动重试。
export const refreshPatches = createStatusSource('patch_status', (view) => {
  patchStore.view = view;
  patchStore.loaded = true;
});

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
        toastActionError('应用补丁失败', e, '请确认工作台已停止、内核版本在补丁适用范围内后重试', 8000);
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
    const run = (force) =>
      withExclusiveLoading('patchRevert:' + id, async () => {
        try {
          const warnings = await invoke('patch_revert', { id, force });
          toastSuccess(force ? '补丁「' + name + '」的记录已清除' : '补丁「' + name + '」已撤销');
          if (warnings && warnings.length) {
            warnings.forEach((w) => toast(w, 8000, 'warning'));
          }
          await refreshPatches();
          return true;
        } catch (e) {
          const raw = e && e.message ? e.message : String(e);
          // "没有可恢复的原文件"是唯一需要二次确认的分支：这类记录既撤销不掉、
          // 也无法重新应用（apply 要求先撤销），而后端建议的"重装内核版本"同样
          // 无效（重装后内容既不是补丁内容、也没有原始哈希可比），用户会永久
          // 卡在一条无法处置的记录上（P0-7）。出路是"只清除记录、文件保持现状"。
          if (!force && raw.includes('清除记录')) {
            const clear = await confirmDialog(
              '无法还原原文件，只清除记录？',
              '这条补丁记录里没有可恢复的原文件备份，无法自动还原。继续只会清除应用记录，' +
                '内核文件保持现状；如果它仍是补丁内容，建议随后重新安装该内核版本。',
              '清除记录'
            );
            if (clear) return run(true);
            return false;
          }
          toastActionError('撤销补丁失败', e, '请确认工作台已停止；备份丢失的文件需要重新安装该内核版本', 8000);
          return false;
        }
      });
    return run(false);
  });
}