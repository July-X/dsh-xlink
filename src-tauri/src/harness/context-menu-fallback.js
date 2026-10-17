/**
 * 初始化脚本：注入到 `harness` 工作台 webview。工作台的右键策略分三层（2026-10-17）：
 *
 * 1. **页面自绘的菜单放行**：内核 UI 自己有右键功能（`ui-primitives` 的 JSON 树
 *    复制按钮用右键打开「选择复制方式」、`ui-dockkit` 的右侧栏标签页用右键打开
 *    标签菜单）。脚本挂在 `window` 的**冒泡**阶段，是传播的最后一站，页面处理器
 *    已经跑完——`event.defaultPrevented` 为真就原样收手。
 * 2. **引擎原生的菜单压掉**：页面没处理时 `preventDefault()`，系统菜单不出现。
 * 3. **没有页面菜单时，弹壳自己的编辑菜单**：可编辑区域（输入框 / textarea /
 *    contenteditable）或只读选区上右键，调 `show_workbench_edit_menu` 让 Rust 弹
 *    一个原生菜单（撤销 / 重做 / 剪切 / 复制 / 粘贴 / 全选，只读选区只给复制）。
 *    形状照内核官方桌面壳（Electron，`apps/desktop/src/main.ts` 的 `context-menu`
 *    监听）：只压不给，用户就只剩键盘快捷键。
 *
 * 三件事的顺序不能换：先放行页面、再压引擎、最后才是自己的菜单。反过来（先弹
 * 自己的）会把内核自绘菜单盖掉；不压引擎菜单则会和系统菜单叠在一起弹两次。
 *
 * **只碰 `contextmenu`，别的什么都不动**：不写 `user-select: none`，不拦 `select` /
 * `copy` / `mousedown`——左键拖选与 Ctrl/⌘+C 复制必须照旧可用。
 *
 * 另外两处落点与本文无关：官方对话的三个内容 webview 加载的是**别人的**站点，仍走
 * 一律取消的 `no-context-menu.js`；壳自己的五个窗口走
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

  /**
   * 事件目标所在的可编辑「字段」：输入框 / textarea 本身，或最近的
   * `contenteditable` 容器。找不到返回 null——只读文本也走这里，它可能带着
   * 选区（那种情况 Rust 那边只给一项「复制」）。
   */
  function fieldAt(target) {
    var el = target && target.nodeType === 1 ? target : (target && target.parentElement) || null;
    while (el) {
      if (el.isContentEditable) {
        return el;
      }
      var tag = el.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") {
        return el;
      }
      el = el.parentElement;
    }
    return null;
  }

  /** 这个字段本身能不能改：`contenteditable` 容器恒为真，输入框看 disabled / readOnly。 */
  function isEditable(field) {
    if (!field) {
      return false;
    }
    if (field.isContentEditable) {
      return true;
    }
    return !(field.disabled || field.readOnly);
  }

  /**
   * 现场有没有可复制的内容。输入框 / textarea 的选区不在 `window.getSelection()`
   * 里（它们自己存 selectionStart / selectionEnd），所以两条路分开判；只读文本走
   * 文档选区——用户先选词再在别处右键，Electron 壳也是给「复制」的。
   */
  function hasSelectedText(field) {
    if (field && (field.tagName === "INPUT" || field.tagName === "TEXTAREA")) {
      var start = field.selectionStart;
      var end = field.selectionEnd;
      return typeof start === "number" && typeof end === "number" && start !== end;
    }
    var selection = window.getSelection ? window.getSelection() : null;
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
      return false;
    }
    return String(selection).length > 0;
  }

  window.addEventListener(
    "contextmenu",
    function (event) {
      if (event.defaultPrevented) {
        return;
      }
      event.preventDefault();
      var field = fieldAt(event.target);
      var editable = isEditable(field);
      var selected = hasSelectedText(field);
      if (!editable && !selected) {
        return;
      }
      var api = window.__TAURI__ && window.__TAURI__.core;
      if (!api || typeof api.invoke !== "function") {
        return;
      }
      try {
        var pending = api.invoke("show_workbench_edit_menu", {
          editable: editable,
          hasSelection: selected,
        });
        if (pending && typeof pending.catch === "function") {
          pending.catch(function () {
            /* 壳没接住就只是没有菜单，页面不该因此报错 */
          });
        }
      } catch (error) {
        /* 同上：右键菜单失败不许把异常抛回内核页面的事件链 */
      }
    },
    false
  );
})();
