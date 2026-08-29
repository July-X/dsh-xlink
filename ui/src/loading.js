// 按钮级 loading 注册表：每个触发 IO（Tauri 命令 / 网络 / 子进程）的按钮
// 用一个稳定的 key 登记 loading 态，模板里 `:loading="isLoading('xxx')"`。
// reactive Map 的 get() 会被渲染追踪，key 翻转时按钮自动出现 / 收起 spinner。
import { reactive, computed, ref } from 'vue';

const loadingMap = reactive(new Map());
const busyCount = ref(0);
const exclusiveActive = ref(false);

export const globalBusy = computed(() => busyCount.value > 0);

// 业务活动信号：任何按钮触发的 IO（withLoading 快速操作 / 互斥长任务）
// 在进行时为 true，驱动标题栏鲸眼脉冲的显隐。
export const ioActive = computed(() => busyCount.value > 0 || loadingMap.size > 0);

export function isLoading(key) {
  return loadingMap.get(key) === true;
}

export function setBusy(on) {
  busyCount.value = Math.max(0, busyCount.value + (on ? 1 : -1));
}

export function isExclusiveBusy() {
  return exclusiveActive.value;
}

// 同一个 key 的重入直接忽略；互斥任务则在整个异步生命周期内持有
// busy lease，避免安装、启停、更新和状态刷新互相排队或交错写状态。
export function withLoading(key, fn) {
  if (loadingMap.get(key)) return Promise.resolve(undefined);
  loadingMap.set(key, true);
  let result;
  try {
    result = Promise.resolve(fn());
  } catch (error) {
    result = Promise.reject(error);
  }
  return result.finally(() => {
    loadingMap.delete(key);
  });
}

export function withExclusive(task) {
  if (exclusiveActive.value) return undefined;
  exclusiveActive.value = true;
  setBusy(true);
  let result;
  try {
    result = Promise.resolve(task());
  } catch (error) {
    result = Promise.reject(error);
  }
  return result.finally(() => {
    exclusiveActive.value = false;
    setBusy(false);
  });
}

export function withExclusiveLoading(key, task) {
  return withLoading(key, () => withExclusive(task));
}
