// 诊断层的**动作代理**（设计 §8.2）。
//
// 与 `diagnostics.js` 分开是因为职责不同：那边管「现在在看什么」（状态、
// 事件解析、加载），这边管「用户点了会发生什么」。混在一起的后果是每次
// 加一个动作都要重新读一遍状态定义才能确认没写错层。
//
// 组件**只调这里的动作**，不直接 import store / plugins 的动作。理由有两条：
//   ① 诊断层是唯一需要在启动 / 预检前建立运行记录上下文的地方，而那条上下文
//      归诊断层管；组件直调就绕过了它。
//   ② 让「诊断层的 loading key」与「业务动作的 loading key」分开——用户点
//      诊断页的「重试启动」时，转的应该是诊断页那个，而不是概览页那个。
import { withLoading } from '../shell/loading.js';
import {
  getLastRunId,
  openHarnessWindow,
  refreshAll,
  showIncident,
  startWorkbench,
  store,
} from '../store.js';
import { applyPluginChange, precheckPlugin } from '../plugins/plugins.js';
import { showLogs } from '../logs/logs.js';
import { loadSnapshots, previewRestore } from './snapshots.js';
import {
  clearLiveEvents,
  closeDiagnosis,
  diagnosticStore,
  loadOperationDiagnosis,
  loadPluginRun,
  loadRecentRuns,
  loadStartupDiagnosis,
} from './diagnostics.js';

/** 触发启动工作台，并建立这次的运行记录上下文。 */
export function startStartupDiagnosis() {
  return withLoading('startupDiagnosisRetry', startWorkbench).then(() => {
    // 启动完再拉：这时后端已经把 runId 落进 StartReport 并推进了索引。
    // 顺带刷新最近操作——启动记录此刻才进索引，概览那张卡要跟着换。
    return Promise.all([loadStartupDiagnosis(getLastRunId()), loadRecentRuns(null)]).then(
      (first) => first[0]
    );
  });
}

/**
 * 跑一次插件预检。
 *
 * 阶段事件会经通道实时到达，**先清空**上一轮的时间线：不清的话新事件会接
 * 在旧事件后面，用户读到的是两次预检混在一起的一条线。
 *
 * `precheckPlugin` 内部已经带了 withProgress（沙盒要跑几十秒，需要全局进度
 * 面板与按钮 loading），这里**不再套一层**——两层进度会互相覆盖文案。
 */
export function runPluginPrecheck(specFromCatalog) {
  clearLiveEvents();
  return precheckPlugin(specFromCatalog).then((result) => {
    // 预检跑完索引里就多了一条，概览的「最近操作」要跟着换（审查 P1-05 的
    // 验收：切回概览后显示刚完成的那一条）。不刷的话用户看到的是上一次的
    // 结果，而那正是「用户可能还没意识到插件已装上」的那一类误导。
    loadRecentRuns(null);
    return result;
  });
}

/**
 * 打开某条证据（日志 / 沙盒日志）。
 *
 * **路径必须真的传进去**（审查 P1-02）：诊断记录里的 `kernelLog` /
 * `sandboxLog` 是这次诊断的证据路径，不传的话日志窗口只会退回列表里的第一
 * 份文件——用户看到的是一份**与这次诊断无关**的日志，而报告正文正指着它说
 * 「看这里」。沉默地打开另一份日志比打不开更伤信任。
 *
 * 没传路径时用诊断状态里已记的那一份（[`diagnosticStore.evidencePath`]，
 * 由记录加载时从 `RunEvidence` 取），这样头部的「更多 → 查看完整日志」不必
 * 知道每个视图各自有几份证据。
 *
 * **复用日志模块，不在诊断模块自己读文件**（设计 §8.2 最后一条动作）：两处
 * 各读一次就会有两套分类、大小上限和截断行为，而用户正在两份报告里比对
 * 同一段日志。
 */
export function openEvidence(path) {
  const target = String(path || diagnosticStore.evidencePath || '');
  if (target) {
    diagnosticStore.evidencePath = target;
  }
  showLogs(target);
  return target;
}

/**
 * 打开内核状态诊断并强制重新读取。
 *
 * 状态页此前只有 store 里的旧快照，头部「刷新状态」是个**无操作**按钮
 * （审查 P1-06）：点了没反应，而用户以为自己已经拿到了新状态——那比没有
 * 这个按钮更糟。`fresh: true` 让它走真正的重新读取而不是复用缓存。
 *
 * loading key 供头部 ⟳ 与「更多」菜单的 refresh 项共用：设计 §2.5.4 要求异步
 * 动作的 loading 只显示在触发它的按钮上，而这两处是**同一个动作**，两个 key
 * 会让两个按钮一起转。
 *
 * **内核状态页的底部按钮栏已于 2026-10-07 删除**（用户：与顶部按钮功能重复）。
 * 记录在这里是因为「为什么删」比「删了什么」更要紧——那一版留着两个入口，
 * 而它们的行为并不一致：
 *   · 头部 ⟳ 与「更多」菜单的 refresh 项都经 `DiagnosisShell.onRefresh()`，
 *     落到本函数；**页面底部那个按钮自己直接调本函数，绕过了 onRefresh**。
 *     `diagnosis-more-menu.js` 里写着「菜单与页面主操作是同一个动作（都走
 *     onRefresh），不是两份实现」——底部那份恰好是反例。
 *   · 「查看完整日志」：菜单第一项走 `openEvidence(diagnosticStore
 *     .evidencePath)`，页面底部那份走无参的 `openEvidence()`。本文件上方
 *     `openEvidence` 的 `path || diagnosticStore.evidencePath` 说明两者等价，
 *     所以删掉不改变行为。
 * 连带删掉的还有页面里只为该按钮服务的 `viewLogs()` 与 `Refresh` / `isLoading`
 * / `loadKernelStatusDiagnosis` / `openEvidence` 四个 import。
 *
 * `.diag-actions` 这个类**仍然保留**，另有三个诊断页在用（StartupDiagnosis 的
 * 「刷新 / 查看完整日志 / 查看事故」、OperationDiagnosis 的「刷新 / 查看完整
 * 日志」、PluginDiagnosis 的「查看日志 / 刷新 / 恢复」）——它们的动作还没进
 * 顶部菜单，收掉它们的按钮栏会直接删掉用户唯一的入口。
 */
export function loadKernelStatusDiagnosis(manual = false) {
  const task = () =>
    refreshAll({ fresh: true }).then((outcome) => {
      // 记读到的时刻，而不是记「点了刷新」的时刻：读取失败时那一栏必须
      // 继续显示上一次成功读到的时刻，否则「可能过期」这句话没有锚点。
      //
      // 所以只有 `status` 成功才推进时间戳并清错（审查 R2-P1-04）：`refreshAll`
      // 过去把失败吞成 `undefined`，这里无条件执行，于是把一次读取失败显示
      // 成「刚刚读取成功」，同时把内核状态页用来提示过期的 `error` 也清掉了。
      // 失败时保留旧时刻与旧错误，页面才会如实显示「可能已过期」。
      if (outcome?.status) {
        diagnosticStore.kernelReadAt = Date.now();
        diagnosticStore.error = '';
      } else {
        diagnosticStore.error = '读取内核状态失败，下面可能是上一次读到的内容';
      }
      return outcome;
    });
  return manual ? withLoading('kernelStatusReload', task) : task();
}

/**
 * 打开事故面板（审查 P2-02）。
 *
 * 诊断层里的「查看事故」此前由 `StartupDiagnosis` 直接 import `showIncident`，
 * 而本文件开头写着「组件只调这里的动作」——规矩只写了一半，剩下那一半靠
 * 自觉，下一个人一定会直接 import。补齐。
 */
export function showIncidentFor(incident) {
  return showIncident(incident, { force: true });
}

/**
 * 打开工作台窗口。
 *
 * 同样是因为「组件只调这里」这条约定：诊断页的主动作是「去看那个跑起来的
 * 工作台」，而它此前绕过了动作层。
 */
export function openWorkbenchWindow() {
  return openHarnessWindow();
}

/**
 * 插件诊断页的「恢复变更前状态」。
 *
 * 有快照 id 就直接开**那一份**的差异预览；没有（快照机制之前、或打快照
 * 失败）就退回「打开快照列表」——按钮名在组件里会跟着改，所以这里退的是
 * 一条**如实命名**的路径，而不是一个写着「恢复」却只给列表的动作
 * （审查 P1-04）。
 *
 * 面板跳转也在这里做：组件只管调动作，跳到哪个面板是这条动作的一部分。
 */
export function restorePreChange(snapshotId) {
  if (snapshotId) return previewRestore(snapshotId);
  loadSnapshots();
  store.activePanel = 'settings';
  return closeDiagnosis();
}

/**
 * 从一条运行记录里取「最该看的那份证据」。
 *
 * 沙盒日志优先：它是这次操作**直接产生**的日志；内核日志是常驻的那一份，
 * 排障时往往已经被别的运行覆盖过。
 */
export function primaryEvidenceFor(run) {
  const evidence = (run && run.evidence) || {};
  return String(evidence.sandboxLog || evidence.kernelLog || '');
}

// —— 诊断页的加载与「应用变更」（审查 R2-P2-02）———————————————
//
// 上一批（`showIncidentFor` / `openWorkbenchWindow`）补的是「打开某处」这类
// 动作。这一批补的是**加载**与**写入**：它们此前由各诊断组件直接 import，
// 于是 loading key、错误处理、以及「这条记录属于哪次运行」这件事分散在五个
// 组件里——改一个 loading key 要改五处，而漏掉的那一处只会表现成「有时转圈
// 有时不转」。
//
// ## 边界：概览页可以直接用业务模块的纯动作
//
// `ControlTower`（概览页）仍然直接 import `showLogs` / `loadLogList`，这是
// **有意保留**的：概览不是诊断层，不建立运行记录上下文，也不需要与诊断页
// 共用 loading key。它要的是「点这一行去看日志」这一个动作。为此把它收进
// 代理只会让概览页依赖一个它不需要的层。
//
// 反过来，**四个诊断页**（`StartupDiagnosis` / `OperationDiagnosis` /
// `PluginDiagnosis` / `DiagnosisShell`）一律走这里。

// 三条加载的形状都是「按 id 拉 + 可选手动」，所以一个 `manual` 开关就够：
// `manual = true` 挂 loading 并强制重新读取（用户点了刷新），`false` 静默
// （切 kind 时自动拉一次）。分成六个函数只会让调用方挑错。
//
// **一律按 id 拉**，不取「最近一条」：刷新里换一条记录，用户会以为刷新失败。

/** 重新拉取启动诊断。`manual` 为真时挂 loading 并强制重新读取。 */
export function reloadStartupDiagnosis(runId, manual = true) {
  return loadStartupDiagnosis(runId || getLastRunId(), manual);
}

/** 重新拉取恢复 / 排查诊断。 */
export function reloadOperationDiagnosis(runId, kind, manual = true) {
  return loadOperationDiagnosis(runId || diagnosticStore.active?.runId, kind, manual);
}

/** 重新拉取插件预检诊断。 */
export function reloadPluginDiagnosis(runId, manual = true) {
  return loadPluginRun(runId, manual);
}

/**
 * 应用预检通过的插件（两阶段契约的第二阶段）。
 *
 * 预检弹窗与插件诊断页是**同一个动作**的两个入口（评审 R2-P2-02 指出它们各自
 * 处理了一遍）。收在这里之后，两处的 loading key、失败处理、以及「应用后把
 * 报告换成已安装那份」是同一份实现——过去改一处忘一处，漂掉的恰恰是
 * `preChangeSnapshotId` 那个字段，而它决定「恢复变更前状态」按钮是否可用。
 */
export function applyPrecheckChange(report) {
  return applyPluginChange(report);
}
