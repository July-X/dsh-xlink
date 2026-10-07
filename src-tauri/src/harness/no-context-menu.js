/**
 * 初始化脚本：注入到 `harness` 工作台 webview 与 `official-chat-tab-{i}` 三个
 * 官方对话内容 webview。**壳自己的**窗口（主面板 / 日志 / 用量 / 套餐 /
 * 官方对话页签栏）由 `ui/src/noContextMenu.js` 负责——两边是同一件事的两处
 * 落点，`scripts/check-invariants.mjs` 第 16 项把「三族窗口都禁了」钉成机械
 * 检查。
 *
 * **只禁右键，别的什么都不动。** 这是本脚本唯一被允许做的事：
 *
 * - 取消 `contextmenu` 的默认行为：引擎的原生菜单与页面自绘的右键菜单都因此
 *   不再出现。
 * - 刻意**不**写 `user-select: none`，不拦 `select` / `selectstart` /
 *   `copy` / `mousedown` / `dragstart`。左键拖选与 Ctrl/⌘+C 复制必须照旧可用
 *   —— 把「禁右键」写成「禁选中」是这个需求最常见的写错方式，而它坏掉的
 *   时候没有任何报错：菜单确实没了，用户只是再也复制不出东西。
 *
 * 挂在 `window` 的**捕获**阶段：事件传播的第一站就是它，先于 `document` 与
 * 目标元素上的任何监听器。初始化脚本在文档开始时注入，注册顺序天然第一，
 * 页面自己后挂的捕获监听器抢不到前面。
 *
 * 用 `stopImmediatePropagation` 而不是 `stopPropagation`：后者只拦后续节点，
 * 同一节点（window）上后注册的监听器仍会被调用，页面自绘的菜单照弹——而
 * 官方对话那三个站点都是重框架的 SPA。
 *
 * 顶帧守卫与同目录另外三个脚本一致：Tauri 会在每个 frame 里跑初始化脚本，
 * 只加固顶层文档，iframe 里的内容归它自己。
 *
 * 引擎层的菜单（Wry 的 `with_default_context_menus` /
 * WebView2 的 `AreDefaultContextMenusEnabled`）在 Tauri 2.11 上没有对外接口，
 * 所以这一层只能靠 DOM 事件取消——见 docs/architecture/architecture.md 的对应条目。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  window.addEventListener(
    "contextmenu",
    function (event) {
      event.preventDefault();
      event.stopImmediatePropagation();
    },
    true
  );
})();
