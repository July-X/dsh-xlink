// 内置插件的共享状态与动作。
//
// 它与社区插件不是一类东西（设计 §3.1）：随应用交付、不进中央库、没有
// 安装 / 卸载 / 更新语义，只有「启用意图」一个开关。所以不进 plugins.js
// 的社区清单，独立一个小模块；命令面只有两条（status / set_enabled）。
//
// **状态是列表不是单个对象**（2026-10-12）：过去这里是 `view`，因为只有
// openai-oauth 一个内置插件。现在登记表里有 N 个，命令一次返回全部，前端画
// 一张列表——分两次取就要处理「一部分到了另一部分没到」的中间态，而那个中间态
// 的表现是「先出现 OpenAI、ponytail 过一会儿才冒出来」。
//
// 「内核运行中不能改」由后端按实例级判据拒绝，UI 不预判（预判一份就是
// 第二份判据，会和后端漂移），只把拒绝原因原样 toast 出来。
import { computed, reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toastSuccess, toastActionError } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import { singleFlight } from '../shell/async.js';

/**
 * openai-oauth 的**插件 id**。
 *
 * **只是取状态用的键**：包 id / 接线行 id / settingsNs 恒为 `xlink-openai-oauth`
 * （物化目录 `extensions/builtin/openai-oauth/…`、内核模型设置页的账户卡都靠它
 * 对上）。显示名由后端随状态一起给（`displayName`）——那个名字 2026-10-09 已经
 * 改过两次（「OpenAI 对话」→「OpenAI-OAuth-Plugin」），每改一次要同时想起四个
 * 地方，就是迟早会漏一个的形状；收成后端常量正是为了结束这种形状。
 */
export const OPENAI_BUILTIN_ID = 'openai-oauth';

export const builtinStore = reactive({
  // null = 尚未取回。读失败时**保留上一次成功值**（状态是常驻行，失败弹窗会
  // 随面板刷新变成骚扰），错误单独放 error —— 它是整体性的，不是某一个插件的。
  list: null,
  error: '',
});

/** 按 id 取某个内置插件的状态；未取回或没有这一项时返回 null。 */
export function builtinById(id) {
  return builtinStore.list ? builtinStore.list.find((view) => view.id === id) || null : null;
}

/**
 * openai-oauth 的状态。
 *
 * 概览页与套餐窗口都靠它决定「要不要露 Codex 授权开关」——那件事只与
 * openai-oauth 有关，与其它内置插件无关，所以这里按 id 取而不是拿第一项。
 * 拿第一项的话，加一个排在它前面的内置插件就会把这两个入口悄悄关掉。
 */
export const openaiBuiltin = computed(() => builtinById(OPENAI_BUILTIN_ID));

export const loadBuiltinStatus = singleFlight(async () => {
  try {
    builtinStore.list = await invoke('builtin_status');
    builtinStore.error = '';
  } catch (error) {
    builtinStore.error = String(error?.message ?? error);
  }
});

/** 行内状态文字。`view` 为 null（加载中）时返回空串，不画「未知」。 */
export function builtinStateText(view) {
  if (!view) return '';
  if (view.stateError) return '状态文件异常，悬停查看';
  if (view.noteStatus) return view.noteStatus;
  if (view.loadState === 'prepared') return '已接线，下次启动工作台生效';
  if (view.loadState === 'incompatible') return '内核已切换，重新拨一次开关修复';
  return '未启用';
}

/** 行内状态文字的 tooltip：异常 / 实例说明优先，否则回退插件自带的一句话介绍。 */
export function builtinStateTip(view) {
  if (!view) return '';
  return view.stateError || view.noteStatus || view.summary || '';
}

/** 拨开关：成功后以服务端返回的那一项为准回填（requestedEnabled 与接线同步落盘）。 */
export async function setBuiltinEnabled(id, enabled) {
  const next = await withLoading(`builtinToggle:${id}`, () =>
    invoke('builtin_set_enabled', { plugin: id, enabled }),
  );
  if (builtinStore.list) {
    builtinStore.list = builtinStore.list.map((view) => (view.id === id ? next : view));
  }
  toastSuccess(`${enabled ? '已启用' : '已停用'} ${next.displayName}，下次启动工作台生效`);
}

/** 供面板调用的包装：失败时原样呈现后端原因（含「停止工作台后可修改」）。 */
export async function toggleBuiltin(id, enabled) {
  const view = builtinById(id);
  const name = view ? view.displayName : id;
  try {
    await setBuiltinEnabled(id, enabled);
  } catch (error) {
    toastActionError(`切换 ${name} 失败`, error, '按提示处理后重试；若持续失败请查看日志');
    // 拒绝后回读一次，避免开关停在乐观值上。
    await loadBuiltinStatus();
  }
}