// 运行诊断的状态与动作：一份状态，两个消费方（诊断层 + 进度面板）。
//
// ## 为什么单独一层
//
// 启动事件同时流向两个界面：进度浮层要**人话**（「正在启动工作台…」），
// 诊断时间线要**结构化**（stage / status / seq）。而它们在 UI 文档里
// 是同一个浮层和覆盖层——用户在排查中途点了「返回」，浮层里的时间线要
// 继续接着跑。所以事件解析只能有一份，放在这里，`ProgressOverlay` 与
// `StartupDiagnosis` 都从这里读。
//
// ## 兼容规则（不能破）
//
// 后端首期仍可能推送**纯文本**通道消息（pnpm 输出、安装日志行）。解析
// 失败的消息一律按纯文本显示——让一条看不懂格式的行把整个进度面板打挂，
// 比多显示一行原文糟糕得多。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';
import { sortedBySeq } from './diagnostic-labels.js';

export const diagnosticStore = reactive({
  /**
   * 当前打开的诊断层。
   * `{ kind: 'startup' | 'plugin' | 'kernel', runId?, spec? }`；
   * null = 诊断层关闭。
   */
  active: null,
  /** 打开诊断层之前所在的侧栏面板，返回时恢复它。 */
  sourcePanel: '',
  /** 当前运行记录详情；null = 还没拉或拉不到。 */
  currentRun: null,
  /** 最近运行记录摘要（概览「最近一次操作」与历史入口）。 */
  recentRuns: [],
  /** 实时事件流（后端 `running` 期间追加）。 */
  liveEvents: [],
  loading: false,
  error: '',
  /** 最近一次点开的证据路径（设计 §8.2 的 `openEvidence` 记在这里）。 */
  evidencePath: '',
});

/**
 * 解析一条通道消息。
 *
 * 返回 `{ event, text }`：`event` 非空表示这是一条结构化诊断事件，
 * `text` 永远是人话文本（结构化事件也从它的 `message` 取），供进度面板
 * 无条件显示。
 */
export function parseChannelMessage(msg) {
  const text = String(msg ?? '');
  if (!text.startsWith('{')) return { event: null, text };
  let payload;
  try {
    payload = JSON.parse(text);
  } catch {
    // 解析失败不是错误，是旧格式：pnpm 输出里就有大量 `{` 开头的行。
    return { event: null, text };
  }
  if (!payload || payload.type !== 'diagnostic-event' || !payload.event) {
    return { event: null, text };
  }
  return { event: payload.event, text: String(payload.event.message || text) };
}

/** 一条通道消息的人话文本。诊断事件取它的 `message`，其余原样返回。 */
export function diagnosticText(msg) {
  return parseChannelMessage(msg).text;
}

/**
 * 把一条通道消息喂进实时时间线。
 *
 * 返回 `true` 表示这是一条诊断事件（调用方据此决定要不要顺手刷新列表）。
 * 纯文本行直接返回 false，**不**进时间线——时间线只放用户能理解的阶段，
 * 把每一行 pnpm 输出都变成节点会让它彻底没法看。
 */
export function ingestChannelMessage(msg, runId) {
  const parsed = parseChannelMessage(msg);
  if (!parsed.event) return false;
  const event = { ...parsed.event };
  if (runId) event.__runId = runId;
  // 同一条 runId 的事件按 seq 追加；换了一条记录就重开时间线。
  const last = diagnosticStore.liveEvents[diagnosticStore.liveEvents.length - 1];
  if (!last || last.__runId !== event.__runId || Number(event.seq) > Number(last.seq)) {
    diagnosticStore.liveEvents.push(event);
  }
  return true;
}

/** 清掉实时事件流（打开一次新的运行、或诊断层关闭时）。 */
export function clearLiveEvents() {
  diagnosticStore.liveEvents = [];
}

/**
 * 当前应展示的事件：实时流优先，落盘记录兜底。
 *
 * 运行中的实时流是「正在发生」，落盘记录是「已经发生」。两者不该同时
 * 显示——那会让同一个阶段出现两行。
 */
export function visibleEvents() {
  if (diagnosticStore.liveEvents.length) return sortedBySeq(diagnosticStore.liveEvents);
  return sortedBySeq(diagnosticStore.currentRun?.events || []);
}

// —— 打开 / 返回 ——

/**
 * 打开启动诊断。
 *
 * `runId` 为空时先问后端「最近一次启动记录」——概览与事故横幅走的都是
 * 这条路径，它们不知道具体 id，也不该知道（id 是后端的实现细节）。
 */
export function openStartupDiagnosis(runId, sourcePanel) {
  // 调用方没给 id 时**不猜**：交给 `loadStartupDiagnosis` 去问后端「最近一次」。
  // 在这里塞一个陈旧的 id 会让用户点进来看到上次那条。
  diagnosticStore.active = { kind: 'startup', runId: runId || '' };
  diagnosticStore.sourcePanel = sourcePanel || '';
  diagnosticStore.error = '';
  diagnosticStore.currentRun = null;
  clearLiveEvents();
  return loadStartupDiagnosis(runId).then(() => diagnosticStore.active);
}

/** 打开插件安全诊断（候选插件视图）。 */
export function openPluginDiagnosis(spec, sourcePanel) {
  diagnosticStore.active = { kind: 'plugin', spec: spec || null };
  diagnosticStore.sourcePanel = sourcePanel || '';
  diagnosticStore.error = '';
  diagnosticStore.currentRun = null;
  clearLiveEvents();
  return Promise.resolve(diagnosticStore.active);
}

/** 打开内核状态诊断（概览页的「查看状态」）。 */
export function openKernelStatusDiagnosis(sourcePanel) {
  diagnosticStore.active = { kind: 'kernel' };
  diagnosticStore.sourcePanel = sourcePanel || '';
  diagnosticStore.error = '';
  clearLiveEvents();
  return Promise.resolve(diagnosticStore.active);
}

/**
 * 返回上一个面板。
 *
 * **不清** currentRun / recentRuns：用户返回后再次打开要看的是同一条记录，
 * 而「刚看完就空了」会让人以为记录被清了（它其实还在，只是被裁剪与否要
 * 看后端）。真正该清的是实时事件流。
 */
export function closeDiagnosis() {
  diagnosticStore.active = null;
  clearLiveEvents();
  return diagnosticStore.sourcePanel || '';
}

// —— 数据加载 ——

/** 拉一条运行记录详情。手动触发（用户点刷新）才挂 loading key。 */
export function loadStartupDiagnosis(runId, manual = false) {
  const run = async () => {
    diagnosticStore.loading = true;
    try {
      const id = runId || diagnosticStore.active?.runId;
      if (!id) {
        // 没有指定 id：取最近一次启动记录。拿不到就是「还没有记录」，
        // 那是**正常**状态（用户从没启动过），不该弹错误。
        const latest = await invoke('diagnostic_run_latest', { kind: 'startup' });
        diagnosticStore.currentRun = latest
          ? await invoke('diagnostic_run_get', { runId: latest.id })
          : null;
        if (latest) diagnosticStore.active.runId = latest.id;
        return diagnosticStore.currentRun;
      }
      diagnosticStore.currentRun = await invoke('diagnostic_run_get', { runId: id });
      return diagnosticStore.currentRun;
    } catch (e) {
      // 拉不到时**保留**上一次记录：不清空。清空会被读成「记录没了」。
      diagnosticStore.error = '读取启动诊断记录失败：' + String(e);
      return null;
    } finally {
      diagnosticStore.loading = false;
    }
  };
  return manual ? withLoading('startupDiagnosisReload', run) : run();
}

/** 拉最近运行记录列表（概览「最近一次操作」用）。 */
export function loadRecentRuns(kind, manual = false) {
  const run = async () => {
    try {
      diagnosticStore.recentRuns = await invoke('diagnostic_run_list', { kind: kind || null });
    } catch (e) {
      diagnosticStore.error = '读取运行记录失败：' + String(e);
    }
    return diagnosticStore.recentRuns;
  };
  return manual ? withLoading('diagnosticRunsReload', run) : run();
}

/**
 * 拉一次运行记录详情（插件诊断用）。
 *
 * 失败时**保留**上一次记录：清空会被用户读成「记录没了」，而事实是
 * 拉取失败。`runId` 为空说明这次预检没落记录——那是正常路径（例如预检
 * 在取源阶段就失败了），不是错误，所以直接返回 null 而不弹提示。
 */
export function loadPluginRun(runId, manual = false) {
  const run = async () => {
    if (!runId) return null;
    diagnosticStore.loading = true;
    try {
      diagnosticStore.currentRun = await invoke('diagnostic_run_get', { runId });
      return diagnosticStore.currentRun;
    } catch (e) {
      diagnosticStore.error = '读取预检阶段记录失败：' + String(e);
      return null;
    } finally {
      diagnosticStore.loading = false;
    }
  };
  return manual ? withLoading('precheckRunReload', run) : run();
}

/** 供概览消费的「最近一次操作」摘要。 */
export function latestSummary() {
  return diagnosticStore.recentRuns[0] || null;
}

// 最近一次启动的运行记录 id 直接转发 `store.js` 那一份，避免组件为了读一个
// id 而跨层 import store——诊断层的所有状态入口都该是这里。
export { getLastRunId } from '../store.js';
