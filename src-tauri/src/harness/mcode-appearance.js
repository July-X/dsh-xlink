/**
 * 初始化脚本，注入到 MiniMax 工作台 webview（`src-tauri/src/harness/mcode_window.rs`）。
 *
 * 把 MiniMax 自己的外观偏好（`localStorage['webui-theme']` = light / dark / system）
 * 翻译成内核同款的只读信号 `html[data-ds-theme-source]`，剩下的交给紧随其后注入的
 * `workbench-appearance.js`：固定档钉同一档；`system` 与 DSH 内核同义——**跟随外壳**
 * 的偏好（壳固定浅/深就钉这一档，壳也是 system 才解除覆盖、跟随操作系统）。
 *
 * 2026-11-09 用户要求：MiniMax 选「跟随系统」时外观要跟 dsh-xlink 同步，不能只看 OS。
 * 此前这里对 system **撤掉**信号，于是壳切浅色而 MiniMax 仍停在系统深色；现在发布
 * system，壳偏好由 Rust 用 `eval` 推 `dsh-shell-theme` 事件 + `get_shell_theme` 轮询
 * 送到（与 DSH 工作台同一条通道）。
 */
(function () {
  if (window.top !== window.self || location.hostname !== '127.0.0.1') return;
  function sync() {
    var root = document.documentElement;
    if (!root) return;
    var value;
    try { value = localStorage.getItem('webui-theme'); } catch (_) { return; }
    if (value !== 'light' && value !== 'dark' && value !== 'system') return;
    if (root.getAttribute('data-ds-theme-source') !== value) {
      root.setAttribute('data-ds-theme-source', value);
    }
  }
  sync();
  setInterval(sync, 1000);
}());
