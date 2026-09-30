// 面板错误的全局兜底。
//
// 面板在渲染期抛错（例如后端返回的形状变了，模板里读 `view.kernel.port` 变成
// 读 undefined 的属性）时，Vue 会卸载那棵子树：用户看到的是**永久空白**且没有
// 任何提示，只能重启应用（P2-40）。这里把错误收进一个响应式状态，由 App.vue
// 渲染成兜底块，并提供「重新加载面板」的出口。
//
// Vue 的 `errorHandler` 并不只接渲染错误：生命周期钩子、事件处理器、watcher 里的
// **同步抛出**同样走这里（`info` 会说明是哪一个）。那些场景下面板通常仍在正常运行，
// 所以标题必须跟着阶段走——把一次点击回调的异常说成「面板渲染出错，部分界面可能
// 无法显示」既不准确，也会误导用户去重载本来没坏的面板。
import { reactive } from 'vue';

/// 只有这两个阶段会真的让组件树渲染不出来。
const RENDER_STAGES = new Set(['render function', 'component update']);

export const renderErrors = reactive({
  /** 最近一次错误的可读说明（空串表示当前没有错误）。 */
  message: '',
  /** 出现次数：同一个错误反复触发时提示用户重启而不是反复重载。 */
  count: 0,
  /** 出错阶段：`render`（渲染函数 / 组件更新）或 `runtime`（生命周期、事件、watcher）。 */
  stage: 'render',

  get title() {
    return this.stage === 'render'
      ? '面板渲染出错，部分界面可能无法显示'
      : '面板运行出错，界面可能停在出错前的状态';
  },
});

export function reportRenderError(error, info) {
  const detail = error && error.message ? error.message : String(error);
  renderErrors.message = info ? `${detail}（${info}）` : detail;
  renderErrors.stage = RENDER_STAGES.has(String(info)) ? 'render' : 'runtime';
  renderErrors.count += 1;
  // 控制台留档：UI 上看不到的调用栈只有这里能看到。
  console.error('[dsh-xlink] 面板错误：', error, info || '');
}

export function clearRenderError() {
  renderErrors.message = '';
  renderErrors.count = 0;
  renderErrors.stage = 'render';
}

/// 兜底块上的「重新加载」：整页重载是最可靠的恢复方式（面板状态全部重建）。
export function reloadPanel() {
  window.location.reload();
}
