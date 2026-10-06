// 诊断层头部的「更多」菜单（设计 §2.5.5）。
//
// **只收低频、只读动作**：查看完整日志、复制运行记录编号、复制诊断摘要、
// 查看插件来源。会改变状态的操作（恢复、重启、删除）绝不放这里——
// 藏在「更多」里等于让用户在不知情下改状态，它们必须以主操作或次要操作
// 明确呈现。
//
// 独立成文件而不是留在 DiagnosisShell.vue：那边只管「头部结构 + 三个
// 视图的路由」，而这里是「每个页面各自有哪些低频动作」的映射表。混在一起
// 后加一项菜单要重新读一遍外壳的路由代码才能确认没写错层。
import { computed } from 'vue';
import { ElMessage } from 'element-plus';
import { diagnosticStore } from './diagnostics.js';
import { openEvidence } from './diagnostic-actions.js';
import { kindLabel, causeLabel } from './diagnostic-labels.js';

/** 当前页面的菜单项。内核页额外给「刷新状态」，插件页额外给「来源」。 */
export const moreItems = computed(() => {
  const runId = diagnosticStore.active?.runId || diagnosticStore.currentRun?.id;
  const items = [{ key: 'logs', label: '查看完整日志' }];
  if (runId) {
    items.push({ key: 'copy-id', label: '复制运行记录编号' });
    items.push({ key: 'copy-summary', label: '复制本次诊断摘要' });
  }
  // 插件页额外给「来源」：低频，但正是用户贴求助帖时最缺的一段。
  const report = diagnosticStore.active?.spec?.report;
  if (diagnosticStore.active?.kind === 'plugin' && report) {
    items.push({ key: 'copy-source', label: '查看插件来源' });
  }
  // 内核页给「刷新状态」。菜单与页面主操作是**同一个动作**（都走
  // `onRefresh`），不是两份实现——设计 §2.5.3 明确要求主操作复用同一个
  // 动作函数和 loading key。
  if (diagnosticStore.active?.kind === 'kernel') {
    items.unshift({ key: 'refresh', label: '刷新状态' });
  }
  return items;
});

/**
 * 复制。
 *
 * 用 `navigator.clipboard` 而不是自建实现：诊断摘要往往很长，用户要粘到
 * 求助帖里；手搓一份就得自己处理非安全上下文（打包后某些环境下 clipboard
 * API 不可用）——那正是让「复制」静默失败的典型原因。
 */
async function copyText(text, okMessage) {
  try {
    await navigator.clipboard.writeText(text);
    ElMessage.success(okMessage);
  } catch {
    // 复制失败必须说出来：静默失败会让用户以为编号已经复制走了，
    // 然后在求助帖里贴出一串空白。
    ElMessage.warning('复制失败，请手动选中文字复制');
  }
}

function runSummaryText() {
  const run = diagnosticStore.currentRun || {};
  return [
    `运行编号：${run.id || '—'}`,
    `类型：${kindLabel(run.kind)}`,
    `状态：${run.status || '—'}`,
    `归因：${causeLabel(run.cause) || '未归因'}`,
    `结论：${run.summary || '—'}`,
    `开始时刻：${run.startedAtMs || '—'}`,
  ].join('\n');
}

function sourceText() {
  const report = diagnosticStore.active?.spec?.report || {};
  return [
    `来源类型：${report.sourceKind || '未知'}`,
    `来源：${report.sourceLabel || '未知'}`,
    `版本钉：${report.pin || '跟随最新'}`,
    `完整性：${report.integrity || 'none'}`,
    `物化方式：${report.materialize || '未知'}`,
    `目标实例：${report.targetInstance || '未知'}`,
  ].join('\n');
}

export async function onMore(command, refresh) {
  if (command === 'logs') {
    openEvidence(diagnosticStore.evidencePath);
  } else if (command === 'copy-id') {
    await copyText(diagnosticStore.currentRun?.id || '', '运行编号已复制');
  } else if (command === 'copy-summary') {
    await copyText(runSummaryText(), '诊断摘要已复制');
  } else if (command === 'copy-source') {
    await copyText(sourceText(), '插件来源已复制');
  } else if (command === 'refresh') {
    // 菜单触发的异步动作：菜单先关闭，页面按钮自己转 loading。
    refresh();
  }
}
