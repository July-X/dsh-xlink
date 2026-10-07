// 跨窗口的主题传播**传输层**：只有事件名与收发两半，不碰主题真值。
//
// 独立成文件而不是并进 `theme.js`：那边是**主题真值**（读存储、落 `html.dark`、
// 推原生 chrome），这里是**跨窗传输**，换任何一套主题方案都要它，两者会各自
// 演进。**并且这里不许 import `theme.js`**——`theme.js` 要调本文件的
// `broadcastTheme`，反向再 import 就成了环。ESM 靠函数声明提升能扛住，但真值
// `theme` 是 const（TDZ），谁先谁后哪天就换个顺序，症状是「一进副窗就白屏」。
// 真值与校验留在 `theme.js`，由它在 `followThemeBroadcast` 里组装。
//
// 为什么需要这一层（2026-10-08 用户报「切换主题时，弹出的 window 也要跟着改变」）：
// 根因是链路缺失，不是样式。主题真值在 localStorage，而 `storage` 事件**不跨
// webview 生效**——日志 / 用量 / 套餐 / 官网页签栏四扇副窗各自是独立 webview，
// 主壳改完存储，它们加载时定下的 `html.dark` 不会动。此前 `theme.js` 注释写的
// 「下一次绘制就跟着变」只在**窗口还没开**时成立。
import { emit, listen } from './bridge.js';

// 名字带 `ui-` 前缀，与内核 / 诊断那条事件总线分开：这条只服务前端主题，
// Rust 侧不消费。
export const THEME_CHANGED_EVENT = 'ui-theme-changed';

/// 主壳侧：把新主题广播给**所有**窗口。
///
/// 只在 `setTheme` 落地之后调。副窗不调本函数（它们只订阅），否则两扇副窗
/// 会互相触发。`emit` 在纯浏览器调试下 resolve 空，不抛。
export function broadcastTheme(next) {
  void Promise.resolve(emit(THEME_CHANGED_EVENT, next)).catch(() => {});
}

/// 副窗侧：订阅主壳的主题广播，回调在**收到事件**时触发。
///
/// 返回值刻意**不暴露**给调用方：`listen` 的返回值是「取消订阅的 Promise」，
/// 而这里的窗口活到进程结束、订阅不需要解绑，`main.js` 之外没有第二个调用点。
/// 真值校验与落地由回调的提供方（`theme.js`）负责——传输层不该知道什么算合法
/// 主题。
export function subscribeThemeChanges(handler) {
  let pending;
  try {
    pending = listen(THEME_CHANGED_EVENT, (event) => handler(event && event.payload));
  } catch {
    return;
  }
  Promise.resolve(pending).catch(() => {});
}