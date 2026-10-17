// 壳自己的五个窗口（主面板、日志查看器、模型用量、套餐用量、官方对话页签栏）
// 挂的是同一个 SPA，所以禁用右键菜单只需在这里装一次——`main.js` 在选完根
// 组件之前调用它，与 `?log` / `?usage` / `?subscription` / `?chatstrip` 这些
// 路由无关。另外两处落点在 Rust 注入脚本里：官方对话的内容 webview 加载的是
// **别人的**页面，走 `src-tauri/src/harness/no-context-menu.js`（一律取消）；
// 工作台加载的是内核 Web UI、它自己有右键功能，走
// `src-tauri/src/harness/context-menu-fallback.js`（冒泡兜底，只压引擎原生
// 菜单、放行页面自绘菜单）。三族窗口由 `scripts/check-invariants.mjs` 第 16 项
// 按建窗链逐条钉住。
//
// **只禁右键，别的什么都不动**：不写 `user-select: none`，不拦 `select` /
// `copy` / `mousedown`。左键拖选与 Ctrl/⌘+C 复制必须照旧可用——把「禁右键」
// 写成「禁选中」不会报任何错，坏掉的症状只是用户再也复制不出东西。
//
// 监听挂在 `window` 的**捕获**阶段（事件传播的第一站，先于 document 与目标
// 元素上的任何监听器），并用 `stopImmediatePropagation` 拦掉同一节点上后注册
// 的监听器：Element Plus 的部分组件会在 window 上挂 contextmenu 处理，
// 只写 `stopPropagation` 它们照跑。

/**
 * 在一个窗口对象上禁用右键菜单。抽成函数而不是「import 即生效」是为了能
 * 直接对着假事件流做测试——`ui/test/noContextMenu.test.js` 断言的是行为
 * （事件被取消、选中与复制不受影响），不是源码里有没有某个字符串。
 *
 * @param {EventTarget & { top?: unknown, self?: unknown }} target 默认 window
 */
export function disableContextMenu(target = window) {
  target.addEventListener(
    'contextmenu',
    (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
    },
    true,
  );
}
