// MiniMax 入口只由用户偏好决定，不影响运行状态或退出清理。
import { computed } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toastError } from '../shell/notify.js';
import { store } from '../store.js';

// 解析规则独立成一个纯函数：显式偏好优先、没保存过按壳模式
// 兜底（开发壳默认开、正式壳默认关）。Node 测试里直接跑它，不依赖 store。
export function resolveMcodeExperience(preference, shellMode) {
  if (preference != null) return preference;
  // 状态还没回来（首帧）时按壳模式兜底：开发壳默认开，正式壳读不到偏好的那
  // 一瞬间不闪出 MiniMax 入口。不能用 store.devUi —— release 预览会把它翻成
  // false，而预览只改界面，不该改这个偏好。
  return shellMode === 'dev';
}

export const mcodeExperience = computed(() =>
  resolveMcodeExperience(
    store.view?.settings?.mcode_experience,
    store.view?.shell_mode,
  )
);

export async function setMcodeExperience(enabled) {
  try {
    const applied = await invoke('set_mcode_experience', { enabled: !!enabled });
    // 轮询下一次到达前先本地回显：开关是即时保存的，等 2.5s 才变会让人以为没生效。
    if (store.view?.settings) store.view.settings.mcode_experience = applied;
    return true;
  } catch (error) {
    toastError('保存 MiniMax 体验开关失败：' + error + ' 请重试。', 6000);
    return false;
  }
}

// 次级窗口（独立日志窗口）没有 get_status 的 ACL，用只读命令读同一份偏好的
// 解析结果。
export function loadMcodeExperience() {
  return invoke('get_mcode_experience').catch(() => false);
}
