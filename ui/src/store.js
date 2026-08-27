// 内核 / 发布 / 外壳自更新的共享状态与动作。
// 面板组件从这里读 view / releases，动作函数保留原零构建版的行为契约：
// 启动编排、启停确认、外壳更新横幅、首次运行引导、2.5s 轮询。
import { reactive } from 'vue';
import { invoke, makeChannel } from './bridge.js';
import { toast, toastSuccess, toastError, confirmDialog } from './notify.js';
import { globalBusy, isLoading, setBusy, withLoading } from './loading.js';
import { withProgress, progress } from './progress.js';
import { refreshPlugins } from './plugins.js';
import { refreshSkills } from './skills.js';
import { showLogs } from './logs.js';

export const store = reactive({
  // get_status 的完整返回：{ kernel, node, settings, shell_version, dev_build, quarantined, last_incident, official_chat_open }
  view: null,
  releases: [],
  releaseWarning: '',
  // 启动编排窗口：点击「启动工作台」到端口就绪之间为 true，
  // 2.5s 轮询不会覆盖「正在启动…」的按钮态。
  starting: false,
  // 最近一次启动容错事故，供概览横幅「查看详情」在 shell 重启后重开事故面板。
  lastIncident: null,
  // 事故面板（IncidentModal）的展示状态。
  incident: null,
  incidentVisible: false,
  // 外壳自更新：available 版本号 + 安装渠道的阶段文案。
  shellUpdateVersion: '',
  shellUpdateText: '',
  activePanel: 'overview',
  // dev 调试钩子：让 dev 构建也能临时启用 release 版的 CSS 钩子（绿渐变等），
  // 仅 store.view.dev_build 为 true 时生效。setReleasePreview() 是唯一入口。
  releasePreview: false,
});

// 上次观察到的内核运行态：只有外部来源导致的就绪迁移才弹「内核已就绪」
// （启动编排自己会 toast）；初始化为 null，首轮轮询不提示。
let lastRunning = null;

// 把 body 上的 dev-build / rel-build 类同步独立成函数：refreshAll 与 2.5s 轮询
// 都会调用，避免用户的 releasePreview 调试覆盖被下一次轮询清掉。
// dev_build=true 表示 tauri dev 调试构建；releasePreview 是 dev 期临时覆盖，
// 打开后让 dev 也能看绿渐变等 release-only 样式。
function applyBuildClass() {
  const isDev = !!store.view?.dev_build;
  const effectiveDev = isDev && !store.releasePreview;
  document.body.classList.toggle('dev-build', effectiveDev);
  document.body.classList.toggle('rel-build', !effectiveDev);
}

// dev 调试动作：切换 release 版预览。非 dev 构建直接拒绝（no-op），避免在
// 正式版里意外改出调试态。改完立即同步 body 类，不等下一次轮询。
export function setReleasePreview(value) {
  if (!store.view?.dev_build) return;
  store.releasePreview = !!value;
  applyBuildClass();
}

export function showIncident(incident) {
  if (!incident) return;
  store.incident = incident;
  store.incidentVisible = true;
}

// --- 状态读取 ---------------------------------------------------------------

export async function refreshAll() {
  try {
    const view = await invoke('get_status');
    store.view = view;
    lastRunning = view.kernel.running;
    store.lastIncident = view.last_incident || null;
    applyBuildClass();
    await Promise.all([refreshPlugins(), refreshSkills()]);
  } catch (e) {
    toastError('读取状态失败：' + e);
  }
}

// 后台轮询：失败不打扰用户，面板保留旧值，下个周期自动重试。
export async function pollStatus() {
  if (document.hidden) return;
  try {
    const view = await invoke('get_status');
    const changed = lastRunning !== null && view.kernel.running !== lastRunning;
    lastRunning = view.kernel.running;
    store.view = view;
    store.lastIncident = view.last_incident || null;
    applyBuildClass();
    if (changed && view.kernel.running && !store.starting) {
      toastSuccess('内核已就绪', 2500);
    }
  } catch {
    // 静默：下个周期自动重试。
  }
}

// --- 内核版本 ---------------------------------------------------------------

export function checkUpdates() {
  return withLoading('checkUpdates', async () => {
    setBusy(true);
    try {
      const list = await invoke('fetch_releases');
      store.releases = list.releases || [];
      store.releaseWarning = list.warning || '';
      if (store.releases.length === 0) {
        toast('没有获取到官方发布，请稍后再试', 4000, 'warning');
      }
    } catch (e) {
      store.releases = [];
      toastError('获取发布失败：' + e, 6000);
    } finally {
      setBusy(false);
    }
  });
}

export function installVersion(version) {
  return withProgress(
    {
      cmd: 'install_kernel',
      start: '正在安装 ' + version + ' …',
      done: '版本 ' + version + ' 安装完成',
      fail: '安装失败',
      failToast: '安装失败，详情见进度窗口与日志',
    },
    (channel) => ({ version, onEvent: channel })
  );
}

export function activateVersion(version) {
  return withLoading('activate:' + version, async () => {
    setBusy(true);
    try {
      await invoke('activate_version', { version });
      toastSuccess('已切换活动版本为 ' + version + '（下次启动生效）');
      await refreshAll();
    } catch (e) {
      toastError('切换失败：' + e);
    } finally {
      setBusy(false);
    }
  });
}

export function removeVersion(version) {
  return withLoading('remove:' + version, async () => {
    setBusy(true);
    try {
      await invoke('remove_version', { version });
      toastSuccess('已删除版本 ' + version);
      await refreshAll();
    } catch (e) {
      toastError('删除失败：' + e);
    } finally {
      setBusy(false);
    }
  });
}

// 「安装最新版本」（首次运行引导）：拉发布列表，优先第一个稳定版，
// 全是预发布时退回最新可用版本。
export function installLatestRelease() {
  return withLoading('firstRunLatest', async () => {
    setBusy(true);
    try {
      const list = await invoke('fetch_releases');
      store.releases = list.releases || [];
      if (!store.releases.length) {
        toast('没有获取到官方发布，请稍后再试', 4000, 'warning');
        return;
      }
      const stable = store.releases.find((r) => !r.prerelease);
      await installVersion((stable || store.releases[0]).version);
    } catch (e) {
      toastError('获取发布失败：' + e, 6000);
    } finally {
      setBusy(false);
    }
  });
}

// --- 工作台启停 -------------------------------------------------------------

// 启动编排。start_kernel 内置启动看护：命令返回时端口必然就绪（或带事故
// 报告），看护重试含 pnpm 重装、可能数分钟，阶段消息经 channel 流进进度面板。
export function startWorkbench() {
  store.starting = true;
  const channel = makeChannel((msg) => {
    progress.appendLog(msg);
    progress.set(msg.length > 60 ? msg.slice(0, 57) + '…' : msg);
  });
  progress.resetLog();
  progress.set('正在启动工作台…');
  return invoke('start_kernel', channel ? { onEvent: channel } : {})
    .then((report) => {
      if (!report || !report.running) {
        const err = new Error((report && report.incident && report.incident.message) || '内核未能启动，详情见日志');
        err.report = report;
        throw err;
      }
      return invoke('open_harness').then(() => report);
    })
    .then((report) => {
      progress.hide();
      toastSuccess(report && report.incident ? '工作台已以安全模式启动' : '工作台已启动');
      if (report && report.incident) {
        showIncident(report.incident);
      }
      lastRunning = true;
    })
    .catch((e) => {
      // 失败路径：进度面板保持开放（约定），事故面板覆盖其上解释原因。
      progress.fail('启动失败：' + e.message);
      toastError('启动失败：' + e.message, 8000);
      if (e.report && e.report.incident) {
        showIncident(e.report.incident);
      } else {
        showLogs();
      }
    })
    .finally(() => {
      store.starting = false;
      refreshAll();
    });
}

export async function stopWorkbench() {
  const proceed = async () => {
    setBusy(true);
    try {
      await invoke('stop_kernel');
      toastSuccess('工作台已关闭');
      await refreshAll();
    } catch (e) {
      toastError('关闭失败：' + e);
    } finally {
      setBusy(false);
    }
  };
  const running = !!(store.view && store.view.kernel && store.view.kernel.running);
  if (!running) {
    // 内核未运行：只是关掉残留的工作台窗口，无需确认。
    return proceed();
  }
  // 运行中才确认：内核可能正在思考，停止会中断未完成的回复。
  const ok = await confirmDialog(
    '确认停止内核？',
    '内核正在运行。如果它正在思考或处理任务，停止将中断未完成的回复。',
    '停止内核'
  );
  if (ok) {
    return proceed();
  }
}

export function openHarnessWindow() {
  return withLoading('openHarness', () =>
    invoke('open_harness').catch((e) => toastError('无法打开工作台窗口：' + e))
  );
}

// 官方会话窗口由 Rust 管理；命令完成后立即刷新状态，让按钮文案同步窗口实际状态。
export function toggleOfficialChat() {
  const open = !!(store.view && store.view.official_chat_open);
  const command = open ? 'close_official_chat' : 'open_official_chat';
  const errorPrefix = open ? '关闭官方对话窗口失败：' : '无法打开官方对话窗口：';
  return withLoading('officialChat', () =>
    invoke(command)
      .then(() => refreshAll())
      .catch((e) => toastError(errorPrefix + e, 5000))
  );
}

export function openDataDir() {
  return withLoading('openDataDir', () =>
    invoke('open_data_dir').catch((e) => toastError('打开数据目录失败：' + e))
  );
}

// --- 外壳自更新 -------------------------------------------------------------

export function checkShellUpdate(manual) {
  const run = () =>
    invoke('check_shell_update')
      .then((info) => {
        if (info.available) {
          showShellUpdateBanner(info.available);
        } else if (manual) {
          toastSuccess('桌面端已是最新（v' + info.current + '）');
        }
      })
      .catch((e) => {
        if (manual) {
          toastError('检查桌面端更新失败：' + e);
        }
      });
  // 启动时的后台检查不挂按钮 loading，手动点才挂。
  return manual ? withLoading('checkShellUpdate', run) : run();
}

export function showShellUpdateBanner(version) {
  store.shellUpdateVersion = version;
  store.shellUpdateText =
    '发现桌面端新版本 v' + version + '（当前 v' + (store.view ? store.view.shell_version : '?') + '）';
}

export function installShellUpdate() {
  const channel = makeChannel((msg) => {
    store.shellUpdateText = msg;
  });
  return withLoading('installShellUpdate', () =>
    invoke('install_shell_update', { onEvent: channel }).catch((e) => {
      toastError('桌面端更新失败：' + e, 6000);
    })
    // 成功时应用直接重启进新版本，无事可做。
  );
}

// --- 设置 -------------------------------------------------------------------

export function detectNode() {
  return withLoading('detectNode', async () => {
    setBusy(true);
    try {
      const info = await invoke('detect_node');
      if (info.ok) {
        toastSuccess('已检测到 node');
      }
      return info;
    } catch (e) {
      toastError('检测失败：' + e, 4000);
      return null;
    } finally {
      setBusy(false);
    }
  });
}

export function saveSettings(portRaw, profileRaw) {
  const portText = String(portRaw ?? '').trim();
  const port = Number(portText);
  if (!/^\d+$/.test(portText) || port < 1024 || port > 65535) {
    toast('端口需为 1024–65535 的整数，当前输入：' + (portText || '（空）'), 5000, 'warning');
    return Promise.resolve(false);
  }
  const settings = { port, profile: (profileRaw || '').trim() || 'web' };
  return withLoading('saveSettings', async () => {
    setBusy(true);
    try {
      await invoke('save_settings', { settings });
      toastSuccess('设置已保存（重启内核后生效）');
      await refreshAll();
      return true;
    } catch (e) {
      toastError('保存失败：' + e);
      return false;
    } finally {
      setBusy(false);
    }
  });
}

// 供组件绑定：:disabled="globalBusy" / :loading="isLoading(key)"
export { globalBusy, isLoading };
