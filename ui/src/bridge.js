// Tauri 桥接封装：所有与 Rust 外壳的通信都经过这里的 invoke。
// 页面在纯浏览器（vite dev 单独调试）中没有 __TAURI__，此时命令直接拒绝，
// 让调用方走 catch 提示，而不是抛 TypeError。
const core = window.__TAURI__ && window.__TAURI__.core;
const tauriEvent = window.__TAURI__ && window.__TAURI__.event;
const tauriWindow = window.__TAURI__ && window.__TAURI__.window;
const tauriPath = window.__TAURI__ && window.__TAURI__.path;

let homeDirPromise = null;

/// 当前用户 home 目录（路径显示折叠 `~` 用），返回 Promise；桥接未注入
/// 或 path API 不可用（纯浏览器调试）时 resolve null，调用方按原路径
/// 显示兜底。进程内只取一次。
export function homeDir() {
  if (!tauriPath || typeof tauriPath.homeDir !== 'function') return null;
  if (!homeDirPromise) homeDirPromise = tauriPath.homeDir().catch(() => null);
  return homeDirPromise;
}

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

/// 用系统浏览器打开外部链接（opener 插件按 OS 分发）。
///
/// 与 `windowAction` 同理，失败**不吞**：旧实现是 `.catch(() => {})`，于是没有默认
/// 浏览器、opener 被拒或桥接缺失时，用户点了「打开仓库」既没反应也没提示。错误抛给
/// 调用方；面板里的按钮请用 `notify.js` 的 `openExternalLink`，它负责给出出路。
export function openExternal(url) {
  return invoke('plugin:opener|open_url', { url });
}
