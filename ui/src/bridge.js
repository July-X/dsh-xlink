// Tauri 桥接封装：所有与 Rust 外壳的通信都经过这里的 invoke。
// 页面在纯浏览器（vite dev 单独调试）中没有 __TAURI__，此时命令直接拒绝，
// 让调用方走 catch 提示，而不是抛 TypeError。
const core = window.__TAURI__ && window.__TAURI__.core;
const tauriEvent = window.__TAURI__ && window.__TAURI__.event;

export function invoke(cmd, args) {
  if (!core) {
    return Promise.reject(new Error('Tauri bridge 未注入（请通过桌面应用运行本页面）'));
  }
  return core.invoke(cmd, args || {});
}

// 长任务的进度通道：Rust 侧把阶段消息 / pnpm 原始日志行推过 Channel。
export function makeChannel(onMessage) {
  if (!core) return null;
  const channel = new core.Channel();
  channel.onmessage = onMessage;
  return channel;
}

export function listen(event, handler) {
  if (!tauriEvent) return;
  return tauriEvent.listen(event, handler);
}

// 系统浏览器打开外部链接（opener 插件按 OS 分发）。
export function openExternal(url) {
  invoke('plugin:opener|open_url', { url }).catch(() => {});
}
