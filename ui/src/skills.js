// 技能包管理的共享状态与动作：中央仓列表（一行一个包）、安装 / 更新 / 卸载。
// 安装与卸载对运行中的工作台即时生效，无需重启；长任务同样走 withProgress。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toast, toastSuccess, toastError } from './notify.js';
import { withLoading } from './loading.js';
import { withProgress } from './progress.js';
import { refreshAll, store } from './store.js';

export const skillStore = reactive({
  view: null,
  spec: '',
});

export function originLabel(origin) {
  return origin === 'local' ? '本地' : origin === 'git' ? 'git' : 'npm';
}

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

// 手动检查挂按钮 loading；启动自检静默。
export function checkSkillUpdates(opts) {
  const run = () =>
    invoke('skill_check_updates')
      .then((infos) => {
        const n = (infos || []).filter((i) => i.latest).length;
        if (n > 0 && opts.toastOnUpdates) {
          toast('有 ' + n + ' 个技能包可更新', 5000, 'warning');
        }
        return refreshAll();
      })
      .catch((e) => {
        if (opts.busy) {
          toastError('检查技能更新失败：' + e, 6000);
        }
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
