// 二分定位的状态与展示函数（安全网 P2）。
//
// 关键点：二分不是"点一下等结果"，是**一轮轮推进**的。面板必须能显示
// "现在在试哪一半""已排除 N 个""预计还要几轮"——否则用户对着一个不动的
// 进度条会以为卡死，然后关掉它。
//
// 措辞纪律：结论只有三种（最小坏集合 / 不在候选集合内 / 已中断），**没有
// 「找到根因」**。最小坏集合只是"能解释现象的最小组合"，组合效应仍可能
// 参与——把它说成根因会让用户去卸一个无辜的插件。
import { reactive } from 'vue';
import { invoke, makeChannel } from './bridge.js';
import { withExclusiveLoading, withLoading } from './loading.js';
import { progress } from './progress.js';
import { toastError } from './notify.js';
import { relativeTimeLabel } from './labels.js';

/** 轮次硬顶。Rust 侧 MAX_ROUNDS 是 12，这里留出余量只为防后端异常时
 *  前端无限循环——进度面板必须自己能停下来。 */
const MAX_DRIVE_ROUNDS = 16;

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
      // 排查记录读不到时保持空视图，不打断页面渲染。手动刷新时要把原因
      // 摆出来——否则面板会永远停在"正在读取"，用户既看不到记录，也
      // 不知道是没记录还是读取失败。
      if (manual) {
        bisectStore.view = null;
        toastError('读取排查记录失败：' + String(e), 6000);
      }
      return null;
    }
    return bisectStore.view;
  };
  return manual ? withLoading('bisectReload', run) : run();
}

/** 把一条通道接到进度面板上。 */
function progressChannel() {
  return makeChannel((msg) => {
    progress.appendLog(msg);
    progress.set(msg.length > 60 ? msg.slice(0, 57) + '…' : msg);
  });
}

/**
 * 发起一次排查，并**在这里把每一轮跑完**。
 *
 * Rust 侧把「试一轮」做成独立的 `bisect_probe`，是为了每轮都能播报进度、
 * 让用户看见"已排除 N 个"在增长，而不是对着一个不动的进度条。代价是调用
 * 方必须真的去驱动它——只调 `bisect_start` 不调 `bisect_probe`，得到的
 * 是一个 `running: true` 却没有 `steps` 的会话，面板会永远显示"排查进行
 * 中 … 请耐心等"，而一轮都不会完成。
 *
 * 循环以**后端返回的 `running`** 为准：用户中途点「停止」后 `running` 变
 * false，下一轮 `advance` 不会记账，循环随即自然退出。
 */
export function startBisect() {
  return withExclusiveLoading('bisectRun', async () => {
    progress.resetLog();
    progress.set('正在准备排查 …');
    let view;
    try {
      view = await invoke('bisect_start', { onEvent: progressChannel() });
      bisectStore.view = view;
    } catch (e) {
      progress.fail('排查未能开始：' + e);
      toastError('排查未能开始，详情见进度窗口与日志', 6000);
      return null;
    }

    for (let round = 0; round < MAX_DRIVE_ROUNDS; round += 1) {
      if (!view || !view.running) break;
      try {
        view = await invoke('bisect_probe', { onEvent: progressChannel() });
        bisectStore.view = view;
      } catch (e) {
        progress.fail('第 ' + (round + 1) + ' 轮排查失败：' + e);
        toastError('排查中断，详情见进度窗口与日志。已排除的结果会保留', 6000);
        // 拉一次真实状态，别让面板继续显示"排查进行中"。
        return loadBisect();
      }
    }

    if (view && view.running) {
      progress.fail(
        `已跑到前端轮次上限（${MAX_DRIVE_ROUNDS} 轮）仍未收尾，已停止驱动。已排除的结果保留。`
      );
      return view;
    }
    progress.hide();
    return view;
  });
}

/**
 * 中断排查。
 *
 * 失败**不吞**：`.catch(() => null)` 会让"停止"按钮点了没反应、没有任何
 * 提示，而面板还在显示"排查进行中 … 请耐心等"——用户既停不下来，也不知
 * 道为什么。
 */
export function abortBisect() {
  return withLoading('bisectAbort', async () => {
    try {
      const view = await invoke('bisect_abort');
      bisectStore.view = view;
      return view;
    } catch (e) {
      toastError('停止排查失败：' + String(e) + '。可稍后重试，或查看进度窗口与日志', 6000);
      return loadBisect();
    }
  });
}

// —— 纯展示函数（node --test 可直接覆盖）——

/** 三种结局的标题与配色。刻意不给「根因」这个取值。 */
export function conclusionView(conclusion) {
  if (!conclusion) {
    return { label: '排查中', type: 'info', members: [], text: '正在逐轮缩小范围 …' };
  }
  const members = conclusion.members || [];
  switch (conclusion.kind) {
    case 'minimal-bad-set':
      return {
        label: '定位到最小可疑集合',
        type: 'warning',
        members,
        // 优先用 Rust 写好的那句话：它带着"这不等于根因"的免责声明，
        // 前端另写一句很容易把这层意思丢掉。
        text: conclusion.text || `能解释现象的最小集合是 { ${members.join('、')} }。`,
      };
    case 'not-in-set':
      return {
        // 候选集现在只含插件（技能是全局的，沙盒二分不了），所以措辞也
        // 不能再说"技能"——那会让用户去一个没查过的地方找原因。
        label: '原因不在插件里',
        type: 'info',
        members: [],
        text:
          conclusion.text ||
          '把插件全停掉也起不来——问题不在插件这一层，请查内核版本、Node 环境或端口。',
      };
    case 'aborted':
      return {
        label: '排查已中断',
        type: 'info',
        members: [],
        text: conclusion.text || '排查已中断。已排除的结果保留，重新发起会接着缩小范围。',
      };
    default:
      // 未知 kind 不能被吞成"排查中"——那会让一次已结束的排查看起来还在跑。
      return {
        label: conclusion.kind || '结果未知',
        type: 'info',
        members: [],
        text: conclusion.text || `未知的排查结论：${conclusion.kind || '（空）'}`,
      };
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
    return view.candidateCount
      ? '上次排查已中断。重新发起会接着上次已排除的结果缩小范围。'
      : '还没有排查记录。点「深入排查」会按嫌疑度逐轮缩小范围。';
  }
  return conclusionView(view.conclusion).text;
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
  return view && view.startedAtMs ? relativeTimeLabel(view.startedAtMs) : '';
}
