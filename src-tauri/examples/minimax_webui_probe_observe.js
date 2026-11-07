// Observe the real client's socket. This example exposes no Tauri commands.
if (window.top === window && location.hostname === '127.0.0.1') {
  const NativeWebSocket = window.WebSocket;
  let receivedFrame = false;
  let priorLoginPhase;
  let priorLoginElement;
  let loginDismissRequested = false;
  let loginDismissed = false;
  window.WebSocket = class extends NativeWebSocket {
    constructor(...args) {
      super(...args);
      if (new URL(this.url).host === location.host) {
        this.addEventListener('message', () => { receivedFrame = true; }, { once: true });
      }
    }
  };
  function observeLogin(records = []) {
    const dialog = document.querySelector('[data-testid="account-login-page-overlay"]');
    const phaseElement = dialog?.querySelector('[data-webui-account-login-phase]');
    const phase = phaseElement?.dataset.webuiAccountLoginPhase;
    const lastChange = [...records].reverse().find((record) =>
      record.type === 'attributes' && record.target === phaseElement);
    const previousPhase = lastChange ? lastChange.oldValue : phaseElement === priorLoginElement ? priorLoginPhase : undefined;
    const justAuthenticated = phase === 'authenticated' && ['opening', 'awaiting'].includes(previousPhase);
    priorLoginPhase = phase;
    priorLoginElement = phaseElement;
    if (loginDismissRequested && (!dialog || dialog.getClientRects().length === 0)) loginDismissed = true;
    if (justAuthenticated && !loginDismissRequested) {
      // shortcut: 0.1.7 leaves the sign-in layer open. Use its Escape handler only after an observed login and with no competing modal.
      // 探针直连服务、没有壳注入，所以只能用合成按键；壳的生产路径是 src-tauri/src/harness/mcode-login.js（认证后重载页面）。
      const visibleDialogs = [...document.querySelectorAll('[role="dialog"][aria-modal="true"]')]
        .filter((element) => element.getClientRects().length > 0);
      if (visibleDialogs.length === 1 && visibleDialogs[0] === dialog && dialog.contains(document.activeElement)) {
        document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
        loginDismissRequested = true;
      }
    }
    return phase;
  }
  new MutationObserver(observeLogin).observe(document, {
    subtree: true, childList: true, attributes: true, attributeOldValue: true,
    attributeFilter: ['data-webui-account-login-phase'],
  });
  window.__mcodeProbe = () => {
    const html = document.documentElement;
    const preference = localStorage.getItem('webui-theme');
    const phase = observeLogin();
    return {
      connected: receivedFrame,
      rendered: Boolean(document.querySelector('#webui-root')?.childElementCount),
      darkClass: html?.classList.contains('dark') ?? false,
      themePreference: ['light', 'dark', 'system'].includes(preference) ? preference : null,
      systemDark: matchMedia('(prefers-color-scheme: dark)').matches,
      loginPhase: ['opening', 'awaiting', 'authenticated', 'signed-out', 'error'].includes(phase) ? phase : null,
      loginDismissRequested,
      loginDismissed,
    };
  };
}
