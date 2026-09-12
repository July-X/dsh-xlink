// 技能包管理的共享状态与动作：中央仓列表（一行一个包）、安装 / 更新 / 卸载。
// 安装与卸载对运行中的工作台即时生效，无需重启；长任务同样走 withProgress。
import { reactive } from 'vue';
import { toast } from './notify.js';
import { withProgress } from './progress.js';
import { createStatusSource, createUpdateChecker } from './async.js';
import { refreshAll, store } from './store.js';

export const skillStore = reactive({
  view: null,
  spec: '',
});

const SKILL_UPDATE_CHECK_TTL_MS = 15 * 60 * 1000;
/// 失败后的自动重试冷却（比成功 TTL 短）：手动点击不受它限制。
const SKILL_UPDATE_FAILURE_BACKOFF_MS = 5 * 60 * 1000;

// `originLabel` 已提到 `labels.js`（技能页与插件页共用），这里保留再导出，
// 避免既有调用点被迫同时改动。
export { originLabel } from './labels.js';

// 工作台是否在服务：决定动作提示是「即时生效」还是「下次启动可用」。
export function kernelRunningNow() {
  return !!(store.view && store.view.kernel && store.view.kernel.running);
}

export function effectSuffix() {
  return kernelRunningNow() ? '，对运行中的工作台即时生效' : '，下次启动工作台后可见';
}

export function installSkill() {
  const raw = skillStore.spec.trim();
  if (!raw) {
    toast('请先填写 git 仓库地址，例如 https://github.com/owner/repo.git', 4000, 'warning');
    return Promise.resolve(false);
  }
  skillStore.spec = '';
  return withProgress(
    {
      cmd: 'skill_install',
      start: '正在安装技能包 ' + raw + ' …',
      done: '技能包 ' + raw + ' 已安装' + effectSuffix() + '；新开一个工作台会话即可调用。',
      fail: '安装失败：' + raw,
    },
    (channel) => ({ spec: raw, onEvent: channel })
  );
}

export function updateSkill(id) {
  return withProgress(
    { cmd: 'skill_update', start: '正在更新技能包 …', done: '技能包已更新' + effectSuffix() },
    (channel) => ({ id, onEvent: channel })
  );
}

export function uninstallSkill(id) {
  return withProgress(
    { cmd: 'skill_uninstall', start: '正在卸载技能包 …', done: '技能包已卸载', fail: '卸载失败' },
    (channel) => ({ id, onEvent: channel })
  );
}

// 手动检查挂按钮 loading；面板进入时低频自检，启动期间失败静默。
// 策略（TTL / 逐包失败退避 / 互斥 / 去重 / 提示）见 async.js 的 createUpdateChecker。
export const checkSkillUpdates = createUpdateChecker({
  cmd: 'skill_check_updates',
  loadingKey: 'checkSkillUpdates',
  noun: '技能包',
  ttlMs: SKILL_UPDATE_CHECK_TTL_MS,
  failureBackoffMs: SKILL_UPDATE_FAILURE_BACKOFF_MS,
  itemFailureHint: '请检查网络或代理后重试；已安装技能不受影响',
  after: async (infos) => {
    await refreshAll();
    return infos;
  },
});

// refreshAll 的技能侧钩子。
export const refreshSkills = createStatusSource('skill_status', (view) => {
  skillStore.view = view;
});
