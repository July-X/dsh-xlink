// Keep this emergency render path aligned with OFFICIAL_CHAT_TABS in commands.rs.
export const DEFAULT_OFFICIAL_CHAT_TABS = Object.freeze([
  Object.freeze({ index: 0, title: 'DeepSeek' }),
  Object.freeze({ index: 1, title: '千问' }),
  Object.freeze({ index: 2, title: 'MiniMax' }),
]);

/**
 * Resolve the strip's render list without leaving it empty when IPC is unavailable.
 *
 * @param {unknown} ipcTabs The value returned by the Tauri command.
 * @returns {Array<{index: number, title: string}>} Tabs for the strip.
 */
export function resolveOfficialChatTabs(ipcTabs) {
  return Array.isArray(ipcTabs) && ipcTabs.length > 0
    ? ipcTabs
    : DEFAULT_OFFICIAL_CHAT_TABS;
}
