// 多实例管理（dev plan §P2 + P8）：后端命令 list_instances /
// set_default_instance 已可用（commit 3eced70 / 后续实例模块）。
//
// 顶部内核标签（KernelTabs）共享状态；默认项是注册表偏好，不代表旧单实例
// 命令已改址。
//
// 单 store，组件实例之间共享。

import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { singleFlight } from './async.js';
import { isExclusiveBusy, withExclusiveLoading } from './loading.js';

/** 全局实例 store。 */
export const instanceStore = reactive({
  // list 缓存
  list: [],
  // 当前默认实例 id（从 list 里 is_default=true 推；保持 id 字段以方便 UI 同步）
  defaultInstanceId: '',
  // 切实例 in-flight（避免双击）
  switching: false,
  // 加载状态：`null` = 还没拉过；`true` = 在拉；`false` = 拉过了（成功或失败都算）。
  // UI 据此区分「加载中…」与「无实例」——之前只用 list.find(...) 找默认实例，
  // list 是空数组时也会 fall through 到「加载中」分支，但实际是后端已经
  // 返回空（注册表加载失败 / 默认实例没注册）。
  loaded: null,
  error: '',
});

let selectionRevision = 0;
/** 拉取所有实例列表 + 默认实例。 */
export const loadInstances = singleFlight(async () => {
  const revision = selectionRevision;
  instanceStore.loaded = true;
  instanceStore.error = '';
  try {
    const list = await invoke('list_instances');
    if (revision !== selectionRevision) return list;
    instanceStore.list = list || [];
    const def = instanceStore.list.find((i) => i.is_default);
    instanceStore.defaultInstanceId = def ? def.record.id : '';
    return list;
  } catch (e) {
    // 保留最后一次成功状态，失败不冒充空注册表。
    if (revision === selectionRevision) instanceStore.error = String(e?.message || e);
    throw e;
  } finally {
    instanceStore.loaded = false;
  }
});

/** 设置默认实例（后端原子切换）。 */
export async function setDefaultInstance(id, refresh = async () => {}) {
  if (instanceStore.defaultInstanceId === id || instanceStore.switching || isExclusiveBusy()) return false;
  return withExclusiveLoading('switchInstance', async () => {
    instanceStore.switching = true;
    selectionRevision += 1;
    try {
      await invoke('set_default_instance', { id });
      selectionRevision += 1;
      instanceStore.list.forEach((it) => {
        it.is_default = it.record.id === id;
      });
      instanceStore.defaultInstanceId = id;
      await refresh();
      return true;
    } finally {
      selectionRevision += 1;
      instanceStore.switching = false;
    }
  });
}

/** 内核族显示名（与后端 KERNEL_FAMILY_* 对齐）。 */
export function familyLabel(family) {
  if (family === 'dsh') return 'DSH';
  if (family === 'mcode') return 'mcode';
  return family || '未知';
}
