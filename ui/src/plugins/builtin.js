// 内嵌 openai-oauth 插件的共享状态与动作。
//
// 它与社区插件不是一类东西（设计 §3.1）：随应用交付、不进中央库、没有
// 安装 / 卸载 / 更新语义，只有「启用意图」一个开关。所以不进 plugins.js
// 的社区清单，独立一个小模块；命令面只有两条（status / set_enabled）。
//
// 「内核运行中不能改」由后端按实例级判据拒绝，UI 不预判（预判一份就是
// 第二份判据，会和后端漂移），只把拒绝原因原样 toast 出来。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toastSuccess, toastActionError } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import { singleFlight } from '../shell/async.js';

/**
 * 这个插件在界面上叫什么。
 *
 * **只是显示名**：包 id / 接线行 id 恒为 `openai-oauth`（物化目录
 * `extensions/builtin/openai-oauth/…`、内核模型设置页账户卡的 settingsNs 都靠
 * 它对上），改显示名不碰它们，反过来也别拿显示名去当标识符。
 *
 * 收成常量的理由是它本来被抄在四处（插件行的名称与 aria-label、两条 toast），
 * 而 2026-10-09 已经是第二次改名（「OpenAI 对话」→「OpenAI-OAuth-Plugin」）——
 * 每改一次要同时想起四个地方，就是迟早会漏一个的形状。
 */
export const BUILTIN_NAME = 'OpenAI-OAuth-Plugin';

export const builtinStore = reactive({
  // null = 尚未取回；读失败时保留上一次成功值（状态是常驻行，失败弹窗
  // 会随面板刷新变成骚扰），只把错误写进 note 供 tooltip 展示。
  view: null,
});

export const loadBuiltinStatus = singleFlight(async () => {
  try {
    builtinStore.view = await invoke('builtin_openai_status');
  } catch (error) {
    if (builtinStore.view) {
      builtinStore.view = { ...builtinStore.view, note: String(error) };
    } else {
      builtinStore.view = { note: String(error) };
    }
  }
});

/** 拨开关：成功后以服务端返回为准回填（requestedEnabled 与接线同步落盘）。 */
export async function setBuiltinEnabled(enabled) {
  const next = await withLoading('builtinOpenaiToggle', () =>
    invoke('builtin_openai_set_enabled', { enabled }),
  );
  builtinStore.view = next;
  toastSuccess(
    enabled
      ? `已启用 ${BUILTIN_NAME}，下次启动工作台生效`
      : `已停用 ${BUILTIN_NAME}，下次启动工作台生效`,
  );
}

/** 供面板调用的包装：失败时原样呈现后端原因（含「停止工作台后可修改」）。 */
export async function toggleBuiltin(enabled) {
  try {
    await setBuiltinEnabled(enabled);
  } catch (error) {
    toastActionError(`切换 ${BUILTIN_NAME} 失败`, error, '按提示处理后重试；若持续失败请查看日志');
    // 拒绝后回读一次，避免开关停在乐观值上。
    await loadBuiltinStatus();
  }
}
