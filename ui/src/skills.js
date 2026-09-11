// 技能包管理的共享状态与动作：中央仓列表（一行一个包）、安装 / 更新 / 卸载。
// 安装与卸载对运行中的工作台即时生效，无需重启；长任务同样走 withProgress。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toast, toastSuccess, toastError } from './notify.js';
import { withLoading, withExclusive, isExclusiveBusy } from './loading.js';
import { withProgress } from './progress.js';
import { refreshAll, store } from './store.js';

export const skillStore = reactive({
  view: null,
  spec: '',
});

const SKILL_UPDATE_CHECK_TTL_MS = 15 * 60 * 1000;
let skillUpdatesInFlight = null;
let lastSkillUpdateCheckAt = 0;

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
export function checkSkillUpdates(opts = {}) {
  const force = !!opts.busy;
  const request = () => {
    if (skillUpdatesInFlight) return skillUpdatesInFlight;
    if (!force && Date.now() - lastSkillUpdateCheckAt < SKILL_UPDATE_CHECK_TTL_MS) {
      return Promise.resolve(null);
    }
    if (isExclusiveBusy()) return Promise.resolve(null);

    const run = withExclusive(async () => {
      const infos = (await invoke('skill_check_updates')) || [];
      // 逐包的错误此前被完全忽略：网络或代理异常时用户点「检查更新」什么都
      // 不会发生，也没有任何解释。这里至少把第一个具体原因说出来。
      const failed = infos.filter((i) => i.error);
      if (failed.length) {
        toastError(
          failed.length +
            ' 个技能包检查更新失败：' +
            failed[0].error +
            '；请检查网络或代理后重试',
          8000
        );
      } else {
        // TTL 只在真的查到结果时推进——与后端保持一致：失败也推进的话，
        // 15 分钟内不会再自动检查，用户手动点击也要等冷却。
        lastSkillUpdateCheckAt = Date.now();
      }
      const n = infos.filter((i) => i.latest).length;
      if (n > 0 && opts.toastOnUpdates) {
        toast('有 ' + n + ' 个技能包可更新', 5000, 'warning');
      }
      await refreshAll();
      return infos;
    });
    if (!run) return Promise.resolve(null);
    const settled = run.finally(() => {
      if (skillUpdatesInFlight === settled) skillUpdatesInFlight = null;
    });
    skillUpdatesInFlight = settled;
    return settled;
  };
  const run = () =>
    request().catch((e) => {
      if (opts.busy) {
        toastError('检查技能更新失败：' + e, 6000);
      }
      return null;
    });
  return opts.busy ? withLoading('checkSkillUpdates', run) : run();
}

// refreshAll 的技能侧钩子。
export function refreshSkills() {
  return invoke('skill_status')
    .then((view) => {
      skillStore.view = view;
    })
    .catch(() => {
      // 静默刷新：读取失败时保留旧卡片，下次 refreshAll 再试。
    });
}
