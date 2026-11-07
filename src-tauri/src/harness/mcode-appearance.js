(function () {
  if (window.top !== window.self || location.hostname !== '127.0.0.1') return;
  function sync() {
    var root = document.documentElement;
    if (!root) return;
    var value;
    try { value = localStorage.getItem('webui-theme'); } catch (_) { return; }
    if (value !== 'light' && value !== 'dark' && value !== 'system') return;
    if (value === 'system') {
      // MiniMax 的「跟随系统」是跟随**操作系统**，不是跟随外壳：撤掉信号，
      // 原生外观就跟着系统走（壳的偏好只在 DSH 内核的语义里接管 system）。
      root.removeAttribute('data-ds-theme-source');
      return;
    }
    if (root.getAttribute('data-ds-theme-source') !== value) {
      root.setAttribute('data-ds-theme-source', value);
    }
  }
  sync();
  setInterval(sync, 1000);
}());
