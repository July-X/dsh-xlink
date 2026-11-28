export const HIDDEN_SESSION_SLOT = Object.freeze({ kind: null, bounds: null });

// Tauri 需要主 webview 的 CSS 逻辑坐标；向内取整，原生层不盖住槽位边缘。
export function sessionSlotRequest(kind, rect, viewport) {
  if (!kind || !rect) return HIDDEN_SESSION_SLOT;
  const x = Math.ceil(Math.max(0, rect.left));
  const y = Math.ceil(Math.max(0, rect.top));
  const width = Math.floor(Math.min(rect.right, viewport.width)) - x;
  const height = Math.floor(Math.min(rect.bottom, viewport.height)) - y;
  return width > 0 && height > 0 ? { kind, bounds: { x, y, width, height } } : HIDDEN_SESSION_SLOT;
}

// 侧栏动画和缩放会连续产生坐标。每次只发一条 IPC，在途时只保留最新的意图，
// 以免切页、关闭或换会话之后，旧坐标又把原生层显示出来。
export function createSessionSlotWriter(send) {
  let desired;
  let sent;
  let writing = false;
  return async function setSlot(slot) {
    desired = slot;
    if (writing) return;
    writing = true;
    while (desired) {
      const next = desired;
      desired = null;
      const key = JSON.stringify(next);
      if (key === sent) continue;
      try {
        await send(next);
        sent = key;
      } catch {
        // 不缓存失败的坐标；下一次布局或可见性变化还可重试。
        sent = undefined;
      }
    }
    writing = false;
  };
}
