// 按钮级 loading 注册表：每个触发 IO（Tauri 命令 / 网络 / 子进程）的按钮
// 用一个稳定的 key 登记 loading 态，模板里 `:loading="isLoading('xxx')"`。
// reactive Map 的 get() 会被渲染追踪，key 翻转时按钮自动出现 / 收起 spinner。
//
// globalBusy 是计数信号量：长任务（withProgress 全程、启停工作台等）期间
// 锁住整排操作按钮，嵌套调用不会提前解锁。
import { reactive, computed, ref } from 'vue';

const loadingMap = reactive(new Map());
const busyCount = ref(0);

export const globalBusy = computed(() => busyCount.value > 0);

// 业务活动信号：任何按钮触发的 IO（withLoading 快速操作 / withProgress
// 长任务的 setBusy）在进行时为 true，驱动标题栏鲸眼脉冲的显隐。
// 后台轮询（pollStatus / 静默自检）不经过这两条路径，不会点亮脉冲。
export const ioActive = computed(() => busyCount.value > 0 || loadingMap.size > 0);

export function isLoading(key) {
  return loadingMap.get(key) === true;
}

export function setBusy(on) {
  busyCount.value = Math.max(0, busyCount.value + (on ? 1 : -1));
}

// 同一个 key 的重入直接忽略（防止双击排队两个相同的 IO 命令）。
export async function withLoading(key, fn) {
  if (loadingMap.get(key)) return undefined;
  loadingMap.set(key, true);
  try {
    return await fn();
  } finally {
    loadingMap.delete(key);
  }
}
