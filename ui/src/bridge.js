// Tauri 桥接封装：所有与 Rust 外壳的通信都经过这里的 invoke。
// 页面在纯浏览器（vite dev 单独调试）中没有 __TAURI__，此时命令直接拒绝，
// 让调用方走 catch 提示，而不是抛 TypeError。
const core = window.__TAURI__ && window.__TAURI__.core;
const tauriEvent = window.__TAURI__ && window.__TAURI__.event;
const tauriWindow = window.__TAURI__ && window.__TAURI__.window;

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

/// 当前窗口句柄；桥接未注入（纯浏览器调试）时返回 null。
function currentWindow() {
  if (!tauriWindow || typeof tauriWindow.getCurrentWindow !== 'function') return null;
  return tauriWindow.getCurrentWindow();
}

/// 是否具备窗口控制能力（自绘标题栏据此决定是否接管系统装饰）。
export function hasWindowControls() {
  return currentWindow() !== null;
}

/// 执行一个窗口操作（close / minimize / toggleMaximize / startDragging …）。
///
/// 失败**不吞**：旧实现在组件里直接读 `window.__TAURI__.window` 并
/// `.catch(() => {})`，一旦桥接缺失或调用失败，关闭/最小化按钮就毫无反应也
/// 没有任何提示（P2-39）。这里把错误抛给调用方，由它 toast 出下一步。
export function windowAction(method) {
  const win = currentWindow();
  if (!win || typeof win[method] !== 'function') {
    return Promise.reject(
      new Error(`当前窗口不支持「${method}」（请通过桌面应用运行本页面）`),
    );
  }
  return Promise.resolve(win[method]());
}

// 系统浏览器打开外部链接（opener 插件按 OS 分发）。
export function openExternal(url) {
  invoke('plugin:opener|open_url', { url }).catch(() => {});
}
