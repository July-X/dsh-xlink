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
import { getLastRunId, startWorkbench } from '../store.js';
import { precheckPlugin } from '../plugins/plugins.js';
import { showLogs } from '../logs/logs.js';
import {
  clearLiveEvents,
  diagnosticStore,
  loadStartupDiagnosis,
} from './diagnostics.js';

/** 触发启动工作台，并建立这次的运行记录上下文。 */
export function startStartupDiagnosis() {
  return withLoading('startupDiagnosisRetry', startWorkbench).then(() => {
    // 启动完再拉：这时后端已经把 runId 落进 StartReport 并推进了索引。
    return loadStartupDiagnosis(getLastRunId());
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
  return precheckPlugin(specFromCatalog);
}

/**
 * 打开某条证据（日志 / 沙盒日志）。
 *
 * **复用日志模块，不在诊断模块自己读文件**（设计 §8.2 最后一条动作）：两处
 * 各读一次就会有两套分类、大小上限和截断行为，而用户正在两份报告里比对
 * 同一段日志。
 */
export function openEvidence(path) {
  if (path) {
    diagnosticStore.evidencePath = String(path);
  }
  showLogs();
  return diagnosticStore.evidencePath;
}
