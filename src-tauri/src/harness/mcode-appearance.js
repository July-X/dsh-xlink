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
