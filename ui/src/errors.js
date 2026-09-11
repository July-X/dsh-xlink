// 渲染期错误的全局兜底。
//
// 面板在渲染期抛错（例如后端返回的形状变了，模板里读 `view.kernel.port` 变成
// 读 undefined 的属性）时，Vue 会卸载那棵子树：用户看到的是**永久空白**且没有
// 任何提示，只能重启应用（P2-40）。这里把错误收进一个响应式状态，由 App.vue
// 渲染成兜底块，并提供「重新加载面板」的出口。
import { reactive } from 'vue';

export const renderErrors = reactive({
  /** 最近一次渲染错误的可读说明（空串表示当前没有错误）。 */
  message: '',
  /** 出现次数：同一个错误反复触发时提示用户重启而不是反复重载。 */
  count: 0,
});

export function reportRenderError(error, info) {
  const detail = error && error.message ? error.message : String(error);
  renderErrors.message = info ? `${detail}（${info}）` : detail;
  renderErrors.count += 1;
  // 控制台留档：UI 上看不到的调用栈只有这里能看到。
  console.error('[dsh-xlink] 渲染错误：', error, info || '');
}

export function clearRenderError() {
  renderErrors.message = '';
  renderErrors.count = 0;
}

/// 兜底块上的「重新加载」：整页重载是最可靠的恢复方式（面板状态全部重建）。
export function reloadPanel() {
  window.location.reload();
}
