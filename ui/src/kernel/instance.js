// 多实例管理（dev plan §P2 + P8）：后端命令 list_instances /
// set_default_instance 已可用（commit 3eced70 / 后续实例模块）。
//
// 单 store，组件实例之间共享。
//
// **界面侧已经没有切实例的入口**（2026-10-07）：顶部工作条那条内核族页签按用户
// 要求整条删除，`KernelTabs.vue` 连同它的 `pickInstance` 一起没了。
//
// **`setDefaultInstance` 本身留着**，因为 `ui/test/kernelSwitch.test.js` 钉着
// 它的三条语义：读实例去重、**陈旧列表不能把一次切换的结果冲掉**
// （`selectionRevision` 就是为此存在的）、以及写入失败必须释放 busy。先前一版
// 把它当死代码删了，那条测试当场变红——它不是「永远走不到的代码」，而是**没有
// 调用方的 API**。这两者的区别就是它该不该留着：删函数就要连这套语义一起删，
// 那是拿测试换行数。真接入第二个内核族时切换器重新做一遍，直接调它即可。
//
// 被删掉的那条页签**从第一天起就是空操作**，这是删 UI 不算丢功能的依据：页签按
// `kernel_family` 去重，而后端只定义了 `dsh` 一个族（`mcode` 在 paths.rs 的注释
// 与测试里都写着「将来的」）。一个族只会画出一个页签，而它必然就是当前那个，
// 于是 `pickInstance` 每次都在 `defaultInstanceId === id` 那一步 return false。

import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { singleFlight } from '../shell/async.js';
import { isExclusiveBusy, withExclusiveLoading } from '../shell/loading.js';

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
