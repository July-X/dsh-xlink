// 内核 / 发布 / 外壳自更新的共享状态与动作。
// 面板组件从这里读 view / releases，动作函数保留原零构建版的行为契约：
// 启动编排、启停确认、外壳更新横幅、首次运行引导、2.5s 轮询。
import { reactive, computed } from 'vue';
import { invoke, makeChannel } from './shell/bridge.js';
import {
  clearLiveEvents,
  diagnosticText,
  ingestChannelMessage,
} from './diagnostics/diagnostics.js';
import { toast, toastSuccess, toastActionError, confirmDialog } from './shell/notify.js';
import { globalBusy, isLoading, withExclusive, withExclusiveLoading, withLoading, isExclusiveBusy } from './shell/loading.js';
import { withProgress, progress } from './shell/progress.js';
import { singleFlight } from './shell/async.js';
import { refreshPlugins } from './plugins/plugins.js';
import { refreshSkills } from './skills/skills.js';
import { showLogs } from './logs/logs.js';

export const store = reactive({
  // get_status 的完整返回：{ kernel, node, settings, shell_version, dev_build, shell_mode, quarantined, last_incident, official_chat_open }
  view: null,
  releases: [],
  releaseWarning: '',
  // 设置文件损坏时的提示（端口已回退到默认值）。
  settingsWarning: '',
  // 启动编排窗口：点击「启动工作台」到端口就绪之间为 true，
  // 2.5s 轮询不会覆盖「正在启动…」的按钮态。
  starting: false,
  // 最近一次启动容错事故，供概览横幅「查看详情」在 shell 重启后重开事故面板。
  lastIncident: null,
  // 事故面板（IncidentModal）的展示状态。
  incident: null,
  incidentVisible: false,
  // 插件安装预检的报告与展示状态（PrecheckDialog）。报告有三态
  // ——通过 / 未通过 / 未能验证——所以不能折成一个布尔：把「没能验证」
  // 画成「通过」会让用户以为插件已经过检验。
  precheckReport: null,
  precheckVisible: false,
  // 外壳自更新：available 版本号 + 安装渠道的阶段文案。
  shellUpdateVersion: '',
  shellUpdateText: '',
  activePanel: 'overview',
  // dev 调试钩子：让 dev 构建也能临时启用 release 版的 CSS 钩子（绿渐变等），
  // 仅 store.view.dev_build 为 true 时生效。setReleasePreview() 是唯一入口。
  releasePreview: false,
  // dev 专属界面是否生效 = `dev_build && !releasePreview`。所有「只在 dev 构建里
  // 出现」的内容（设置页的「模拟一次任务完成」、概览页版本号的「（dev）」后缀、
  // dev 面板里的参考信息）都只读这一个字段：release 预览一开，它们必须同时消失，
  // 否则预览到的只是"半个正式版"。调试浮按钮**不**走这个字段——它是切回来的
  // 唯一入口，预览期间必须留在屏幕上（见 DebugPanel.vue）。
  // applyBuildClass() 是唯一写入口。
  devUi: initialDevUi(),
});

/// `devUi` 的首帧兜底：HMR 重载时 body 上的 dev 类还在，直接读它，避免 dev
// 专属入口先隐藏、等下一次状态轮询（最多 2.5 s）才闪回来。纯浏览器 / 单元测试
// 环境拿不到 classList.contains 时按正式版处理。
function initialDevUi() {
  if (typeof document === 'undefined') return false;
  const contains = document.body && document.body.classList && document.body.classList.contains;
  return typeof contains === 'function' && document.body.classList.contains('dev-build');
}

// 上次观察到的内核运行态：只有外部来源导致的就绪迁移才弹「内核已就绪」
// （启动编排自己会 toast）；初始化为 null，首轮轮询不提示。
let lastRunning = null;

// get_status 可能同时被动作完成后的 refreshAll 与后台轮询调用。只允许最近
// 发出的请求提交快照，避免恢复动作已经清掉隔离记录后，较早的轮询响应又把
// 旧 quarantined / last_incident 写回界面。
let statusRequestSeq = 0;
let statusInFlight = null;
function beginStatusRequest() {
  statusRequestSeq += 1;
  return statusRequestSeq;
}

function applyStatus(view, requestSeq) {
  if (requestSeq !== statusRequestSeq) return false;
  store.view = view;
  lastRunning = view.kernel.running;
  store.settingsWarning = view.kernel.settings_warning || '';
  store.lastIncident = view.last_incident || null;
  applyBuildClass();
  maybePromptNodeInstall();
  return true;
}

// 把 body 上的 dev-build / rel-build 类与 store.devUi 同步独立成函数：refreshAll
// 与 2.5s 轮询都会调用，避免用户的 releasePreview 调试覆盖被下一次轮询清掉。
// dev_build=true 表示 tauri dev 调试构建；releasePreview 是 dev 期临时覆盖，
// 打开后让 dev 也能看绿渐变等 release-only 样式——顺带把 dev 专属入口全部收起来
// （见 store.devUi 的注释）。
function applyBuildClass() {
  const isDev = !!store.view?.dev_build;
  const effectiveDev = isDev && !store.releasePreview;
  store.devUi = effectiveDev;
  document.body.classList.toggle('dev-build', effectiveDev);
  document.body.classList.toggle('rel-build', !effectiveDev);
}

// 当前 Shell 的构建模式（'release' / 'dev'），与 store.view.shell_mode 同步。
// P1 之后用来在 UI 上明确告知用户「这是 dsh-xlink 自己的 release/dev 构建」，
// 而不是把它误解释为「内核是 release/dev」。调用方应当把它读作「Shell 模式」，
// 不要把它当作 kernel/instance 维度的状态。
export const shellMode = computed(() => store.view?.shell_mode ?? 'release');

// dev 调试动作：切换 release 预览。非 dev 构建直接拒绝（no-op），避免在
// 正式版里意外改出调试态。改完立即同步 body 类与 store.devUi，不等下一次轮询。
export function setReleasePreview(value) {
  if (!store.view?.dev_build) return;
  store.releasePreview = !!value;
  applyBuildClass();
}

const VERSION_SWITCH_BLOCKED_MESSAGE = '工作台启动或运行期间不能切换内核，请先停止工作台后再切换。';
const VERSION_SWITCH_BLOCKED_TOAST_MS = 8000;

export function workbenchActiveNow() {
  return !!(store.starting || (store.view && store.view.kernel && store.view.kernel.running));
}

let shownIncidentKey = '';

function incidentKey(incident) {
  const health = incident.health || {};
  return [
    incident.cause || '',
    incident.message || '',
    incident.log_path || '',
    incident.at || '',
    health.kind || '',
    health.message || '',
    health.stack || '',
    health.page_url || '',
  ].join('|');
}

// 「前端 bundle 异常」：页面仍在运行时的前端异常（内核 client-modules 的 bundle 抛错或
// 加载失败，但证据里没有可归因的包名，既无法指认插件也无法指认内核）。这类报告没有可处置
// 的对象，弹模态框只会打断用户——只记录事实并交给概览横幅；横幅上的「查看详情」带 force
// 打开面板。有强证据（插件/内核）的报告、启动失败、以及 blank（白屏）一律照旧弹面板：
// 那里有用户能做的动作，或者页面确实不可用。
//
// `bundle-load-failure` 只在**多成员**组合路由上才会落到 `cause === 'frontend'`——
// 单成员 URL 能被归因成插件或内核，那时有真正的处置入口，照旧弹面板。
//
// `slot-assembly`（内核客户端模块装配未就绪）是另一种「没有需要用户立刻做的事」
// 的报告：页面在记下证据之后会自己重载一次，工作台会回来，弹模态框只会被 3 秒后
// 的刷新吃掉一半，体感比重载本身更糟。额度用掉后再撞上同样的错，页面会改用
// `runtime-error` 上报，那时照旧弹面板。
// 降级由 **kind** 而不是 cause 决定，因为「要不要打断用户」取决于页面会不会自己
// 恢复，而那只有上报方知道。`slot-assembly` 是唯一一种会自愈的：页面记下证据后
// 3 秒自动重载一次（额度用掉后再撞上就改报 `runtime-error`，那时照旧弹面板）。
// 其余三种是「用户看得见但没法当场处置」的前端异常，归因通常是 frontend。
export function isNonFatalFrontendIncident(incident) {
  if (!incident || incident.recovered) return false;
  const kind = (incident.health && incident.health.kind) || '';
  if (kind === 'slot-assembly') return true;
  if (incident.cause !== 'frontend') return false;
  return kind === 'unhandled-rejection' || kind === 'runtime-error' || kind === 'bundle-load-failure';
}

export function showIncident(incident, options = {}) {
  if (!incident) return;
  if (!options.force && isNonFatalFrontendIncident(incident)) {
    store.lastIncident = incident;
    return;
  }
  const key = incidentKey(incident);
  if (store.incidentVisible && shownIncidentKey === key) return;
  shownIncidentKey = key;
  store.incident = incident;
  store.lastIncident = incident;
  store.incidentVisible = true;
}

function requestStatus(force = false, source = 'unknown') {
  if (!force && statusInFlight) return statusInFlight;
  const requestSeq = beginStatusRequest();
  const request = invoke('get_status', { source })
    .then((view) => {
      const applied = applyStatus(view, requestSeq);
      return { view, applied };
    })
    .finally(() => {
      if (statusInFlight === request) statusInFlight = null;
    });
  statusInFlight = request;
  return request;
}

// --- 状态读取 ---------------------------------------------------------------

// 同一时刻只保留一次全量刷新：动作完成后的刷新与页面进入时的刷新合并。
const runRefreshAll = singleFlight(async () => {
  try {
    await requestStatus(true, 'refresh');
    await Promise.all([refreshPlugins(), refreshSkills()]);
  } catch (e) {
    toastActionError('读取状态失败', e, '请确认应用仍在运行；若持续失败，重启应用后重试');
  }
});

export async function refreshAll({ fresh = false } = {}) {
  // 切换后的刷新不能复用切换前已发出的读取。
  if (fresh && runRefreshAll.busy()) await runRefreshAll();
  return runRefreshAll();
}

// 后台轮询：失败不打扰用户，忙时跳过；面板保留旧值，下个周期自动重试。
export async function pollStatus() {
  if (document.hidden || isExclusiveBusy() || runRefreshAll.busy()) return;
  try {
    const previousRunning = lastRunning;
    const ownsRequest = statusInFlight === null;
    const result = await requestStatus(false, 'poll');
    if (!ownsRequest || !result.applied) return;
    const changed = previousRunning !== null && result.view.kernel.running !== previousRunning;
    if (changed && result.view.kernel.running && !store.starting) {
      toastSuccess('内核已就绪', 2500);
    }
  } catch {
    // 静默：下个周期自动重试。
  }
}

// --- Node.js 自动安装 -----------------------------------------------------
// 检测到没有可用的 Node.js 时弹窗询问是否自动安装（官方二进制下载到
// 数据目录，不扩大安装包体积）。「不需要」只静默本次会话——外壳重启后
// 轮询会再次提示；安装失败后冷却 60 秒再提示，避免 2.5s 轮询刷屏。
let nodePromptDismissed = false;
let nodePromptVisible = false;
let nodeNextPromptAt = 0;
const NODE_PROMPT_COOLDOWN_MS = 60 * 1000;

export function installNode() {
  return withProgress(
    {
      cmd: 'install_node',
      start: '正在自动安装 Node.js（需联网下载官方二进制）…',
      done: 'Node.js 已安装',
      fail: 'Node.js 自动安装失败',
      failToast: 'Node.js 自动安装失败，详情见进度窗口与日志',
    },
    (channel) => ({ onEvent: channel })
  );
}

async function maybePromptNodeInstall() {
  const view = store.view;
  if (!view || !view.node || view.node.ok) return;
  if (nodePromptVisible || nodePromptDismissed) return;
  if (Date.now() < nodeNextPromptAt) return;
  if (store.starting || progress.visible || isExclusiveBusy()) return;
  nodePromptVisible = true;
  const reason = view.node.reason ? '（' + view.node.reason + '）' : '';
  const want = await confirmDialog(
    '需要 Node.js 环境',
    '未检测到满足 dsh 要求（^22.19 || >=24）的 Node.js。' + reason +
      '\n是否自动下载并安装官方 Node.js（需联网，安装到本机数据目录）？',
    '帮我安装',
    '不需要'
  );
  nodePromptVisible = false;
  if (!want) {
    nodePromptDismissed = true;
    return;
  }
  const ok = await installNode();
  if (!ok) nodeNextPromptAt = Date.now() + NODE_PROMPT_COOLDOWN_MS;
}

// --- 内核版本 ---------------------------------------------------------------

// 内核发布列表。手动点击与启动自检走同一条路，只有「要不要出声」不同。
//
// 手动（默认）：只挂「检查更新」按钮的 loading，不持互斥租约、不置
// globalBusy——探测期间其他面板的按钮照常可用；切换 / 安装 / 启停的互斥
// 由它们自己的租约与 workbenchActiveNow 守卫负责。失败清空列表并弹提示：
// 用户刚点的，他需要知道这次没拿到。
//
// 启动自检（`manual = false`）：静默，且失败**不清空** `store.releases`。
// 启动时网络抖一下就把上一份好数据抹掉，页面会从「有列表」跳回「点击获取」，
// 比没检查更像故障；不弹提示是因为用户此刻多半没在看内核版本页。
//
// `upgrade` 只在静默路径上报：手动点的人正盯着列表，「安装」按钮就在那一行，
// 再弹一次是重复；启动自检时人不在这一页，不说就没人知道。
export function checkUpdates(manual = true) {
  const run = async () => {
    try {
      const list = await invoke('fetch_releases');
      store.releases = list.releases || [];
      store.releaseWarning = list.warning || '';
      if (store.releases.length === 0) {
        if (manual) toast('没有获取到官方发布，请稍后再试', 4000, 'warning');
      } else if (!manual && list.upgrade) {
        toast('内核有新版本 ' + list.upgrade + '，可到「内核版本」页安装', 6000);
      }
    } catch (e) {
      if (!manual) return;
      store.releases = [];
      toastActionError('获取发布失败', e, '请检查网络或代理设置后重试；也可到 GitHub Releases 手动下载', 6000);
    }
  };
  return manual ? withLoading('checkUpdates', run) : run();
}

export function installVersion(version, options = {}) {
  return withProgress(
    {
      cmd: 'install_kernel',
      start: '正在安装 ' + version + ' …',
      done: '版本 ' + version + ' 安装完成，请在概览页手动启动工作台',
      fail: '安装失败',
      failToast: '安装失败，详情见进度窗口与日志',
    },
    (channel) => ({ version, onEvent: channel }),
    options
  );
}

export function activateVersion(version) {
  if (workbenchActiveNow()) {
    toast(VERSION_SWITCH_BLOCKED_MESSAGE, VERSION_SWITCH_BLOCKED_TOAST_MS, 'warning');
    return Promise.resolve(false);
  }
  return withExclusiveLoading('activate:' + version, async () => {
    if (workbenchActiveNow()) {
      toast(VERSION_SWITCH_BLOCKED_MESSAGE, VERSION_SWITCH_BLOCKED_TOAST_MS, 'warning');
      return false;
    }
    try {
      await invoke('activate_version', { version });
      toastSuccess('已切换活动版本为 ' + version + '（下次启动生效）');
      await refreshAll();
    } catch (e) {
      toastActionError('切换内核版本失败', e, '请先停止工作台再重试');
    }
  });
}

export function removeVersion(version) {
  return withExclusiveLoading('remove:' + version, async () => {
    try {
      await invoke('remove_version', { version });
      toastSuccess('已删除版本 ' + version);
      await refreshAll();
    } catch (e) {
      toastActionError('删除内核版本失败', e, '请先停止工作台并确认该版本未被激活');
    }
  });
}

// 「安装最新版本」（首次运行引导）：拉发布列表，优先第一个稳定版，
// 全是预发布时退回最新可用版本。
export function installLatestRelease() {
  return withExclusiveLoading('firstRunLatest', async () => {
    let version;
    try {
      const list = await invoke('fetch_releases');
      store.releases = list.releases || [];
      store.releaseWarning = list.warning || '';
      if (!store.releases.length) {
        toast('没有获取到官方发布，请稍后再试', 4000, 'warning');
        return undefined;
      }
      const stable = store.releases.find((r) => !r.prerelease);
      version = (stable || store.releases[0]).version;
    } catch (e) {
      toastActionError('获取发布失败', e, '请检查网络或代理设置后重试；也可到 GitHub Releases 手动下载', 6000);
      return undefined;
    }
    return installVersion(version, { exclusive: false });
  });
}

// --- 工作台启停 -------------------------------------------------------------

// 启动编排。start_kernel 内置启动看护：命令返回时端口必然就绪（或带事故
// 报告），看护重试含 pnpm 重装、可能数分钟，阶段消息经 channel 流进进度面板。
export function startWorkbench() {
  const run = withExclusive(async () => {
    store.starting = true;
    const channel = makeChannel((msg) => {
      // 启动通道里现在混着两种消息：结构化诊断事件（后端阶段推送）与纯文本。
      // 两者都要能显示——诊断事件取它的 message，纯文本原样进日志区。
      // 解析规则只有一份（diagnostics.js），这里不重复实现。
      const text = ingestChannelMessage(msg) ? diagnosticText(msg) : msg;
      progress.appendLog(text);
      progress.set(text.length > 60 ? text.slice(0, 57) + '…' : text);
    });
    progress.resetLog();
    // 换一次启动就换一条时间线：上一次的事件留在流里会被读成这次也卡在
    // 同一个阶段。
    clearLiveEvents();
    progress.set('正在启动工作台…');
    try {
      const report = await invoke('start_kernel', channel ? { onEvent: channel } : {});
      if (!report || !report.running) {
        const error = new Error(
          (report && report.incident && report.incident.message) || '内核未能启动，详情见日志'
        );
        error.report = report;
        throw error;
      }
      await invoke('open_harness');
      progress.hide();
      toastSuccess(report.incident ? '工作台已以安全模式启动' : '工作台已启动');
      // 非致命异常（例如句柄槽位里原来那个内核还活着）：内核确实起来了，不该打断
      // 启动，但两个内核同时占着同一个数据目录需要用户去处理——此前它只写进 stderr，
      // 面板上完全看不到。后端文案自带下一步，原样展示。
      if (report.warning) {
        toast(report.warning, 10000, 'warning');
      }
      if (report.incident) {
        showIncident(report.incident);
      }
      lastRunning = true;
    } catch (e) {
      // 失败路径：进度面板保持开放（约定），事故面板覆盖其上解释原因。
      const message = e && e.message ? e.message : String(e);
      progress.fail('启动失败：' + message);
      toastActionError('启动失败', e, '请按事故面板给出的处置建议操作；完整输出见「查看日志」', 8000);
      if (e && e.report && e.report.incident) {
        showIncident(e.report.incident);
      } else {
        showLogs();
      }
    } finally {
      store.starting = false;
      await refreshAll();
    }
  });
  return run === undefined ? Promise.resolve(false) : run;
}

export async function stopWorkbench() {
  const proceed = () => {
    const run = withExclusive(async () => {
      try {
        await invoke('stop_kernel');
        toastSuccess('工作台已关闭');
        await refreshAll();
      } catch (e) {
        toastActionError('关闭工作台失败', e, '请稍后重试；若内核仍在运行，可在活动监视器/任务管理器里结束 node 进程');
      }
    });
    return run === undefined ? Promise.resolve(false) : run;
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
    invoke('open_harness').catch((e) => toastActionError('无法打开工作台窗口', e, '请先点击「启动工作台」，并确认设置里的端口没有被其它程序占用'))
  );
}

// 「刷新工作台」（按钮文案）对应的动作：换掉整个窗口（内核侧换掉整个 webview，
// 因此也换掉了渲染进程）。它内部叫 forceReload 是因为机制确实是强制重载而不是
// 普通刷新——**文案说用户得到什么，代码名说它到底做了什么**，两件事分开记。
// 它是**看门狗覆盖不到**的那类黑屏的手动出路——渲染进程被打崩时页面
// 早就加载完成过，看门狗的判据（Started 之后没等到 Finished）永远不会触发，而
// `reload()` 落在同一块死掉的文档上仍然是黑的。换窗口会丢页面里的前端运行时状态
// （滚动位置、侧栏、终端），会话在服务端不受影响，所以失败/成功提示要说清。
export function forceReloadHarnessWindow() {
  return withLoading('forceReloadHarness', () =>
    invoke('harness_force_reload').catch((e) =>
      toastActionError(
        '刷新工作台失败',
        e,
        '若工作台窗口已经关闭，请改用「工作台窗口」重新打开；内核未运行时先启动工作台',
        6000
      )
    )
  );
}

// 官方会话窗口由 Rust 管理；命令完成后立即刷新状态，让按钮文案同步窗口实际状态。
export function toggleOfficialChat() {
  const open = !!(store.view && store.view.official_chat_open);
  const command = open ? 'close_official_chat' : 'open_official_chat';
  return withLoading('officialChat', () =>
    invoke(command)
      .then(() => refreshAll())
      .catch((e) =>
        toastActionError(open ? '关闭官方对话窗口失败' : '无法打开官方对话窗口', e, '请确认内核正在运行后重试', 5000)
      )
  );
}

// 「打开官方对话窗口」专用入口：只触发 show/focus，不切换关闭态。
// Rust 侧 open_official_chat 在窗口已存在时只会 set_focus，所以重复点击安全。
export function openOfficialChatWindow() {
  return withLoading('openOfficialChatWindow', () =>
    invoke('open_official_chat').catch((e) => toastActionError('打开官方对话窗口失败', e, '请确认内核正在运行后重试', 5000))
  );
}

export function openDataDir() {
  return withLoading('openDataDir', () =>
    invoke('open_data_dir').catch((e) => toastActionError('打开数据目录失败', e, '请按设置页显示的数据目录路径手动打开'))
  );
}

// --- 外壳自更新 -------------------------------------------------------------

// 检查本身去重：自动检查与手动点击合并成一次请求。
const runShellUpdateCheck = singleFlight(async (manual) => {
  try {
    const info = await invoke('check_shell_update');
    if (info.available) {
      showShellUpdateBanner(info.available);
    } else if (manual) {
      toastSuccess('桌面端已是最新（v' + info.current + '）');
    }
  } catch (e) {
    if (manual) {
      toastActionError('检查桌面端更新失败', e, '请检查网络或代理设置后重试');
    }
  }
});

export function checkShellUpdate(manual) {
  return manual
    ? withLoading('checkShellUpdate', () => runShellUpdateCheck(true))
    : runShellUpdateCheck(false);
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
  return withExclusiveLoading(
    'installShellUpdate',
    () =>
      invoke('install_shell_update', { onEvent: channel }).catch((e) => {
        toastActionError('桌面端更新失败', e, '请稍后重试；也可到 GitHub Releases 手动下载安装包', 6000);
      })
      // 成功时应用直接重启进新版本，无事可做。
  );
}

// --- 设置 -------------------------------------------------------------------

export function detectNode() {
  return withExclusiveLoading('detectNode', async () => {
    try {
      const info = await invoke('detect_node');
      if (info.ok) {
        toastSuccess('已检测到 node');
      }
      return info;
    } catch (e) {
      toastActionError('检测 Node.js 失败', e, '请确认 Node.js 已安装且可执行；也可在 settings.json 里手动指定 node_path 后重试', 4000);
      return null;
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
  return withExclusiveLoading('saveSettings', async () => {
    try {
      await invoke('save_settings', { settings });
      toastSuccess('设置已保存（重启内核后生效）');
      await refreshAll();
      return true;
    } catch (e) {
      toastActionError('保存设置失败', e, '请检查端口是否被占用、取值是否合法后重试');
      return false;
    }
  });
}

// 供组件绑定：:disabled="globalBusy" / :loading="isLoading(key)"
export { globalBusy, isLoading };
