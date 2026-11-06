// Observe the real client's socket. This example exposes no Tauri commands.
if (window.top === window && location.hostname === '127.0.0.1') {
  const NativeWebSocket = window.WebSocket;
  let receivedFrame = false;
  window.WebSocket = class extends NativeWebSocket {
    constructor(...args) {
      super(...args);
      if (new URL(this.url).host === location.host) {
        this.addEventListener('message', () => { receivedFrame = true; }, { once: true });
      }
    }
  };
  window.__mcodeProbe = () => {
    const html = document.documentElement;
    const preference = localStorage.getItem('webui-theme');
    return {
      connected: receivedFrame,
      rendered: Boolean(document.querySelector('#webui-root')?.childElementCount),
      darkClass: html?.classList.contains('dark') ?? false,
      themePreference: ['light', 'dark', 'system'].includes(preference) ? preference : null,
      systemDark: matchMedia('(prefers-color-scheme: dark)').matches,
    };
  };
}
