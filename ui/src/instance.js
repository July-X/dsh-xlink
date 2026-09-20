// 多实例管理（dev plan §P2 + P8）：后端命令 list_instances /
// set_default_instance 已可用（commit 3eced70 / 后续实例模块）。
//
// UI 形态：嵌入式顶部 dropdown（与 WindowTitleBar 融合），状态可见
// 性优先；切实例是高频操作，top-bar 单击比侧栏 2 次点击快。
//
// 单 store，组件实例之间共享。

import { reactive } from 'vue';
import { invoke } from './bridge.js';

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
});

/** 拉取所有实例列表 + 默认实例。 */
export async function loadInstances() {
  instanceStore.loaded = true;
  try {
    const list = await invoke('list_instances');
    instanceStore.list = list || [];
    const def = instanceStore.list.find((i) => i.is_default);
    instanceStore.defaultInstanceId = def ? def.record.id : '';
    return list;
  } catch (e) {
    // 后端命令失败（注册表损坏 / 数据目录不可读）。保持 list 为空，
    // loaded=true 让 UI 切到「无实例」分支而不是无限「加载中」。
    instanceStore.list = [];
    instanceStore.defaultInstanceId = '';
    throw e;
  }
}

/** 设置默认实例（后端原子切换）。 */
export async function setDefaultInstance(id) {
  if (instanceStore.defaultInstanceId === id) return;
  instanceStore.switching = true;
  try {
    await invoke('set_default_instance', { id });
    // 后端已切；本地列表同步 is_default 标记
    instanceStore.list.forEach((it) => {
      it.is_default = it.record.id === id;
    });
    instanceStore.defaultInstanceId = id;
  } finally {
    instanceStore.switching = false;
  }
}

/** 内核族显示名（与后端 KERNEL_FAMILY_* 对齐）。 */
export function familyLabel(family) {
  if (family === 'dsh') return 'DSH';
  if (family === 'mcode') return 'mcode';
  return family || '未知';
}