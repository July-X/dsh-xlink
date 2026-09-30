// 环境快照（安全网 P0）的状态与展示函数。
//
// 这一层**只读**：P0 的交付是「让用户看得见昨天那套配置是什么」，恢复动作
// 要等 P1 的差异预览 + 二次确认。因此这里刻意没有任何写入入口——提前放一个
// 「一键回退」会让用户在没看清将要发生什么的情况下丢配置。
//
// 判定与裁剪都在 Rust 侧（snapshot.rs），前端只拉取与呈现。文案相关的纯
// 函数单独导出，可被 node --test 直接覆盖。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';
import { withProgress } from '../shell/progress.js';
import { relativeTimeLabel } from '../shell/labels.js';

export const snapshotStore = reactive({
  /** 后端返回的完整列表视图；null = 还没拉过。 */
  view: null,
  loading: false,
  /** 文档损坏提示。有它时列表可能为空，但**不等于**"从来没有过回退点"。 */
  warning: '',
  /** 待确认的差异（恢复预览弹窗用）；null = 弹窗关闭。 */
  pendingDiff: null,
  restoreVisible: false,
  /** 上一次恢复的结果。kept / skipped 分开存，因为"没能恢复"必须能回看。 */
  lastOutcome: null,
});

/** 拉一次快照列表。静默失败：概览页不该因为一个只读卡拉不到就把横幅顶起来。 */
export function loadSnapshots(manual = false) {
  if (snapshotStore.loading) return Promise.resolve(snapshotStore.view);
  snapshotStore.loading = true;
  const run = async () => {
    try {
      const view = await invoke('snapshot_list');
      snapshotStore.view = view;
      snapshotStore.warning = view.warning || '';
    } catch (e) {
      // 拉不到就保持上一次的视图。**不清空**：一个只读卡突然变空会被读成
      // "我的回退点全没了"，而真相只是这次没拉到。
      if (manual) {
        snapshotStore.warning = '读取环境快照失败：' + String(e);
      }
    } finally {
      snapshotStore.loading = false;
    }
    return snapshotStore.view;
  };
  return manual ? withLoading('snapshotReload', run) : run();
}

// —— P1：差异预览与恢复 ——

/** 预览「回到某个回退点」将要做什么。**不**改任何东西。
 *
 * 与恢复分成两次调用是刻意的：用户要先看见将要失去什么再点确认。合一
 * 意味着要点一次「恢复」才知道后果，而那时已经点了。
 */
export async function previewRestore(id) {
  const diff = await invoke('snapshot_preview_restore', { id });
  snapshotStore.pendingDiff = diff;
  snapshotStore.restoreVisible = true;
  return diff;
}

export function closeRestorePreview() {
  snapshotStore.restoreVisible = false;
  // 只清待确认的差异，**不清** lastOutcome：用户点「知道了」之后如果还想
  // 回顾"刚才那步到底成了没"，结果必须还在。
  snapshotStore.pendingDiff = null;
}

/**
 * 执行恢复。返回结果里区分「动了什么」与「没能动什么」——后者不能被
 * 吞掉，否则用户会以为已经完全回到目标配置。
 */
export function runRestore(id) {
  return withProgress(
    {
      cmd: 'snapshot_restore',
      start: '正在恢复环境 …',
      // 结果逐条读（动了什么 / 跳过了什么 / 恢复前的备份 id），所以在
      // withProgress 的 onResult 里消费，不走 done 那句固定文案。
      onResult: (outcome) => {
        snapshotStore.pendingDiff = null;
        snapshotStore.lastOutcome = outcome;
        // 结果也要在同一个弹窗里展示，用户不该再去别处找"到底成功了没"。
        snapshotStore.restoreVisible = true;
      },
    },
    (channel) => ({ id, onEvent: channel }),
  );
}

// —— 纯展示函数（node --test 可直接覆盖）——

/** 打点原因 → 人话。未知值原样透出，不吞掉。 */
export function reasonLabel(reason) {
  switch (reason) {
    case 'startup-ok':
      return '成功启动过';
    case 'pre-change':
      return '变更之前';
    case 'manual':
      return '手动回退点';
    default:
      return reason || '未知来源';
  }
}

/** 打点原因 → tooltip 里的解释。 */
export function reasonHint(reason) {
  switch (reason) {
    case 'startup-ok':
      return '这台实例在这种配置下成功启动过，是最可信的"良好"证据。';
    case 'pre-change':
      return '你改动配置（装 / 卸 / 切版本 / 切模式）之前的现场。';
    case 'manual':
      return '你手动标记的回退点。';
    default:
      return '来源未知的快照。';
  }
}

/**
 * 一条快照的摘要文案。刻意带上"此刻是否仍生效"：指纹与当前一致意味着
 * 恢复它等于什么都不做，面板会据此把恢复动作置灰（P1 才有按钮）。
 */
export function entrySummary(entry) {
  // 计数一律走 `|| 0`：后端结构上一定给得出这些数字，但面板不该把
  // "undefined" 当成一个数量显示给用户——那比显示 0 更让人怀疑数据坏了。
  const parts = [`内核 ${entry.kernelVersion || '未知'}`];
  parts.push(`插件 ${entry.pluginCount || 0}`);
  parts.push(`技能 ${entry.skillCount || 0}`);
  parts.push(`补丁 ${entry.patchCount || 0}`);
  return parts.join(' · ');
}

export function entryTimeLabel(entry) {
  return relativeTimeLabel(entry.createdAtMs);
}

/** 差异维度的中文标签。未知值原样透出。 */
export function diffKindLabel(kind) {
  const labels = {
    kernel: '内核',
    'plugin-mode': '插件模式',
    'plugin-enable': '插件',
    'plugin-disable': '插件',
    'skill-enable': '技能',
    'skill-disable': '技能',
    'patch-apply': '补丁',
    'patch-revert': '补丁',
  };
  return labels[kind] || kind;
}

/**
 * 恢复确认框的标题。一句话讲清"从哪回到哪"，让用户不用回忆自己点的是
 * 哪一条。
 */
export function diffHeadline(diff) {
  if (!diff) return '';
  if (!diff.changes.length) {
    return '当前环境与这个回退点完全一致，恢复不会做任何改动。';
  }
  const n = diff.changes.length;
  return `将把环境恢复到这个回退点，共 ${n} 处改动。`;
}

/** 有动不了的条目时，确认框必须把这个数摆出来。 */
export function diffBlockedNote(diff) {
  if (!diff || !diff.blockedCount) return '';
  return (
    `其中 ${diff.blockedCount} 处无法自动完成` +
    '（已标注原因）。可以先恢复能恢复的部分，剩下的手动处理。'
  );
}

/**
 * 三种实测状态在界面上必须是**三种不同的东西**。
 *
 * `verified`   真的把恢复后的配置装进一次性沙盒、起过一次内核、应答了。
 * `failed`     测了，没起来。
 * `not-needed` 本次没有任何改动，因此**什么都没测**。
 *
 * 最后一态是三态里最容易被吞掉的那个：没改东西顺手当成"验过了"，就等于
 * 凭空多给了一句用户没得到过的保证。
 */
export function verificationView(outcome) {
  const state = (outcome && outcome.verification) || 'not-needed';
  switch (state) {
    case 'verified':
      return { type: 'success', label: '启动实测通过：恢复后的配置在一次性沙盒内核里正常应答。' };
    case 'failed':
      return {
        type: 'warning',
        label: '启动实测未通过：恢复后的配置在沙盒内核里没能起来。这不代表回退失败——请点「启动工作台」看真实结果，或回到更早的回退点再试。',
      };
    default:
      return {
        type: 'info',
        label: '本次没有改动任何条目，因此没有做启动实测（没有东西需要验证）。',
      };
  }
}

/**
 * 恢复结果的总结句。实测状态与"没能完成"的条数**都要说**——任一单独出现
 * 都会让用户对恢复到了什么程度产生错误印象。
 */
export function outcomeHeadline(outcome) {
  if (!outcome) return '';
  const applied = (outcome.applied || []).length;
  const skipped = (outcome.skipped || []).length;
  const base = applied
    ? `已按回退点改了 ${applied} 处`
    : '没有需要改动的条目';
  if (!skipped) return base;
  return `${base}；另有 ${skipped} 处没能完成，见下方列表。`;
}

/** 面板顶部的结论句。空列表与"有回退点"必须说不同的话。 */
export function headline(view) {
  if (!view) return '';
  if (!view.entries.length) {
    return '还没有回退点。启动一次工作台，或改动一次配置，这里就会记下当时那套配置。';
  }
  if (!view.hasLastKnownGood) {
    return '已经记下 ' + view.entries.length + ' 个回退点，但还没有一次「成功启动」记录——';
  }
  return '已记下 ' + view.entries.length + ' 个回退点。';
}
