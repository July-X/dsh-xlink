// MiniMax 多内核体验入口的唯一可见性判据：构建包含这些能力，且用户选择了体验。
//
// 两件事分开判断（见 docs/ui/multi-kernel-ui-development-plan.md §1）：
// · 构建可用 = VITE_MCODE_EXPERIMENTAL（构建时决定，用户改不了）；
// · 用户偏好 = 壳设置 mcode_experience（按 dev / release 分文件存；没保存过时
//   开发壳默认开、正式壳默认关，见 settings::mcode_experience_enabled）。
// 实际显示条件 = 构建可用且用户已选择。运行状态、退出停机、错误恢复与已有任务
// 不读这个值——入口隐藏不等于能力撤销。
import { computed } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toastError } from '../shell/notify.js';
import { store } from '../store.js';

// ui/test 里有几组用例直接 import store.js（它会经 skills.js 带到本模块），而
// Node 里没有 import.meta.env：按「构建不含能力」兜底。Vite 构建仍把这行里的
// 旗子静态替换成布尔，不留运行时判断。
export const MCODE_BUILD_AVAILABLE = (() => {
  try {
    return import.meta.env.VITE_MCODE_EXPERIMENTAL === '1';
  } catch {
    return false;
  }
})();

// 解析规则独立成一个纯函数：构建不可用短路、显式偏好优先、没保存过按壳模式
// 兜底（开发壳默认开、正式壳默认关）。Node 测试里直接跑它，不依赖 store。
export function resolveMcodeExperience(buildAvailable, preference, shellMode) {
  if (!buildAvailable) return false;
  if (preference != null) return preference;
  // 状态还没回来（首帧）时按壳模式兜底：开发壳默认开，正式壳读不到偏好的那
  // 一瞬间不闪出 MiniMax 入口。不能用 store.devUi —— release 预览会把它翻成
  // false，而预览只改界面，不该改这个偏好。
  return shellMode === 'dev';
}

export const mcodeExperience = computed(() =>
  resolveMcodeExperience(
    MCODE_BUILD_AVAILABLE,
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
// 解析结果；构建能力仍由本模块的 VITE 旗子判断。
export function loadMcodeExperience() {
  if (!MCODE_BUILD_AVAILABLE) return Promise.resolve(false);
  return invoke('get_mcode_experience').catch(() => false);
}
