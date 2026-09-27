// 环境快照（安全网 P0）的状态与展示函数。
//
// 这一层**只读**：P0 的交付是「让用户看得见昨天那套配置是什么」，恢复动作
// 要等 P1 的差异预览 + 二次确认。因此这里刻意没有任何写入入口——提前放一个
// 「一键回退」会让用户在没看清将要发生什么的情况下丢配置。
//
// 判定与裁剪都在 Rust 侧（snapshot.rs），前端只拉取与呈现。文案相关的纯
// 函数单独导出，可被 node --test 直接覆盖。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { withLoading } from './loading.js';
import { relativeTimeLabel } from './labels.js';

export const snapshotStore = reactive({
  /** 后端返回的完整列表视图；null = 还没拉过。 */
  view: null,
  loading: false,
  /** 文档损坏提示。有它时列表可能为空，但**不等于**"从来没有过回退点"。 */
  warning: '',
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
  const parts = [`内核 ${entry.kernel_version || '未知'}`];
  parts.push(`插件 ${entry.plugin_count || 0}`);
  parts.push(`技能 ${entry.skill_count || 0}`);
  parts.push(`补丁 ${entry.patch_count || 0}`);
  return parts.join(' · ');
}

export function entryTimeLabel(entry) {
  return relativeTimeLabel(entry.created_at_ms);
}

/** 面板顶部的结论句。空列表与"有回退点"必须说不同的话。 */
export function headline(view) {
  if (!view) return '';
  if (!view.entries.length) {
    return '还没有回退点。启动一次工作台，或改动一次配置，这里就会记下当时那套配置。';
  }
  if (!view.has_last_known_good) {
    return '已经记下 ' + view.entries.length + ' 个回退点，但还没有一次「成功启动」记录——';
  }
  return '已记下 ' + view.entries.length + ' 个回退点。';
}
