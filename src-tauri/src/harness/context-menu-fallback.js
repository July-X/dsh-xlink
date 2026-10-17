/**
 * 初始化脚本：注入到 `harness` 工作台 webview。**只压引擎的原生右键菜单，
 * 放行内核页面自绘的菜单**（2026-10-17 起；此前工作台用的是下面那份一律取消的
 * `no-context-menu.js`，把内核自己的右键功能一并拦死了）。
 *
 * 为什么挂**冒泡**阶段而不是捕获：
 *
 * - 捕获阶段是事件传播的第一站，在这里 `preventDefault` + `stopImmediatePropagation`
 *   会让页面自己的处理器（目标元素 / document / React 根容器上那一层）一个都跑不到。
 *   内核的右键功能正是页面处理器：`ui-primitives` 的 JSON 树复制按钮用右键打开
 *   「选择复制方式」菜单，`ui-dockkit` 的右侧栏标签页用右键打开标签菜单。它们因此
 *   在旧实现下全部失效——菜单没弹，也没有任何报错。
 * - 冒泡阶段是传播的最后一站：页面处理器已经跑完，此时再 `preventDefault()` 只关掉
 *   引擎默认动作（原生菜单），不会影响页面已经画出来的菜单。页面没处理时照样压掉
 *   系统菜单，工作台「不弹系统菜单」的口径不变。
 *
 * 三种形状都考虑到了：
 *
 * - 页面调过 `preventDefault()`：不再重复处理（`defaultPrevented` 已经是 true）。
 * - 页面调过 `stopPropagation()`：事件根本到不了这里——`ui-primitives` 的复制按钮
 *   就是这么写的（它同时 `preventDefault`，默认动作已经被它自己关掉）。
 * - 页面什么都没做：这里 `preventDefault()`，引擎原生菜单不出现。
 *
 * **只碰 `contextmenu`，别的什么都不动**：不写 `user-select: none`，不拦 `select` /
 * `copy` / `mousedown`——左键拖选与 Ctrl/⌘+C 复制必须照旧可用。
 *
 * 另外两处落点与本文无关，别顺手改：官方对话的三个内容 webview 加载的是**别人的**
 * 站点，仍走一律取消的 `no-context-menu.js`；壳自己的五个窗口走
 * `ui/src/shell/noContextMenu.js`。三族窗口由 `scripts/check-invariants.mjs` 第 16 项
 * 按建窗链逐条钉住（工作台两条链必须见到本文件、官方对话那条必须见到 strict 那份）。
 *
 * 顶帧守卫与同目录另外几个脚本一致：Tauri 在 Windows 上会把初始化脚本注入每个
 * frame（wry 的 `for_main_frame_only` 在 Windows 上不生效），iframe 里的内容归它
 * 自己——工作台里的 Browser 页签与 HTML 预览都跑在 iframe 里，不受本脚本管辖。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  window.addEventListener(
    "contextmenu",
    function (event) {
      if (event.defaultPrevented) {
        return;
      }
      event.preventDefault();
    },
    false
  );
})();
