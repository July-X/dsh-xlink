// 让这条紧急渲染路径与 commands.rs 中的 OFFICIAL_CHAT_TABS 保持一致。
export const DEFAULT_OFFICIAL_CHAT_TABS = Object.freeze([
  Object.freeze({ index: 0, title: 'DeepSeek' }),
  Object.freeze({ index: 1, title: '千问' }),
  Object.freeze({ index: 2, title: 'MiniMax' }),
]);

/**
 * 解析页签栏的渲染列表：IPC 不可用时回退到默认值，避免界面空白。
 *
 * @param {unknown} ipcTabs Tauri 命令返回的原始值。
 * @returns {Array<{index: number, title: string}>} 页签栏要渲染的页签列表。
 */
export function resolveOfficialChatTabs(ipcTabs) {
  return Array.isArray(ipcTabs) && ipcTabs.length > 0
    ? ipcTabs
    : DEFAULT_OFFICIAL_CHAT_TABS;
}
