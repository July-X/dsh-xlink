// 二分定位的状态与展示函数（安全网 P2）。
//
// 关键点：二分不是"点一下等结果"，是**一轮轮推进**的。面板必须能显示
// "现在在试哪一半""已排除 N 个""预计还要几轮"——否则用户对着一个不动的
// 进度条会以为卡死，然后关掉它。
//
// 措辞纪律：结论只有三种（最小坏集合 / 不在候选集合内 / 已中断），**没有
// 「找到根因」**。最小坏集合只是"能解释现象的最小组合"，组合效应仍可能
// 参与——把它说成根因会��用户去卸一个无辜的插件。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { withLoading } from './loading.js';
import { withProgress } from './progress.js';
import { relativeTimeLabel } from './labels.js';

export const bisectStore = reactive({
  /** 后端返回的会话视图；null = 还没拉过。 */
  view: null,
  starting: false,
});

export function loadBisect(manual = false) {
  const run = async () => {
    try {
      bisectStore.view = await invoke('bisect_view');
    } catch (e) {
      // 排查记录读不到时保持空视图，不打断页面渲染。
      if (manual) {
        bisectStore.view = null;
      }
      return null;
    }
    return bisectStore.view;
  };
  return manual ? withLoading('bisectReload', run) : run();
}

/**
 * 发起一次排查。
 *
 * 发起后**不由前端推进轮次**：每一轮都要真的起一次临时内核，判定在后端。
 * 这里只负责开一次会话并把进度面板打开。
 */
export function startBisect() {
  return withProgress(
    {
      cmd: 'bisect_start',
      start: '正在准备排查 …',
      onResult: (view) => {
        bisectStore.view = view;
      },
    },
    (channel) => ({ onEvent: channel })
  );
}

export function abortBisect() {
  return invoke('bisect_abort')
    .then((view) => {
      bisectStore.view = view;
      return view;
    })
    .catch(() => null);
}

// —— 纯展示函数（node --test 可直接覆盖）——

/** 三种结局的标题与配色。刻意不给「根因」这个取值。 */
export function conclusionView(conclusion) {
  if (!conclusion) {
    return { label: '排查中', type: 'info', members: [] };
  }
  switch (conclusion.kind) {
    case 'minimal-bad-set':
      return { label: '定位到最小可疑集合', type: 'warning', members: conclusion.members || [] };
    case 'not-in-set':
      return { label: '原因不在插件与技能里', type: 'info', members: [] };
    case 'aborted':
      return { label: '排查已中断', type: 'info', members: [] };
    default:
      // 未知 kind 不能被吞成"排查中"——那会让一次已结束��排查看起来还在跑。
      return { label: conclusion.kind || '结果未知', type: 'info', members: [] };
  }
}

/** 「预计还要几轮」：⌈log₂n⌉，与后端同判据。 */
export function roundsLeft(remaining) {
  if (!remaining || remaining <= 1) return 0;
  let n = remaining;
  let rounds = 0;
  while (n > 1) {
    n = Math.ceil(n / 2);
    rounds += 1;
  }
  return rounds;
}

/** 面板顶部的结论句。区分"还没开始"与"已开始但没结论"。 */
export function bisectHeadline(view) {
  if (!view) return '';
  if (view.running) {
    return (
      `排查进行中：${view.remaining} 个待排除，` +
      `大约还需 ${roundsLeft(view.remaining)} 轮。每一轮都要真的启动一次临时内核，请耐心等。`
    );
  }
  if (!view.conclusion) {
    return view.candidate_count
      ? '上次排查已中断。重新发起会接着上次已排除的结果缩小范围。'
      : '还没有排查记录。点「深入排查」会按嫌疑度逐轮缩小范围。';
  }
  return conclusionView(view.conclusion).text || '';
}

/** 一轮试探的标题。 */
export function stepTitle(step) {
  const tried = (step.tried || []).join('、');
  if (step.outcome === 'fail') return `第 ${step.round} 轮：启用 ${tried} → 起不来`;
  if (step.outcome === 'pass') return `第 ${step.round} 轮：启用 ${tried} → 正常`;
  return `第 ${step.round} 轮：启用 ${tried} → 这一轮没能真正试起来`;
}

export function stepClassName(step) {
  if (step.outcome === 'fail') return 'step-fail';
  if (step.outcome === 'pass') return 'step-pass';
  return 'step-unknown';
}

export function startedLabel(view) {
  return view && view.started_at_ms ? relativeTimeLabel(view.started_at_ms) : '';
}
