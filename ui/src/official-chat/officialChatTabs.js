// 让这条紧急渲染路径与 commands.rs 中的 OFFICIAL_CHAT_TABS 保持一致。
//
// **颜色也在这份兜底表里**（2026-10-24）：页签栏不许写死颜色（判据扫
// `OfficialChatTabs.vue` 里的十六进制字面量），但 IPC 不可用时颜色仍要有
// 着落——所以兜底表带一份与 Rust 侧 `OFFICIAL_CHAT_BRAND_COLORS` 同值的副
// 本。判据第 20 项跨语言比对这两处：改一处不改另一处会报红。
export const DEFAULT_OFFICIAL_CHAT_TABS = Object.freeze([
  Object.freeze({ index: 0, title: 'DeepSeek', color: '#4D6BFE' }),
  Object.freeze({ index: 1, title: 'MiniMax', color: '#E2167E' }),
]);

/**
 * 解析页签栏的渲染列表：IPC 不可用时回退到默认值，避免界面空白。
 *
 * @param {unknown} ipcTabs Tauri 命令返回的原始值。
 * @returns {Array<{index: number, title: string, color?: string}>} 页签栏要渲染的页签列表。
 */
export function resolveOfficialChatTabs(ipcTabs) {
  return Array.isArray(ipcTabs) && ipcTabs.length > 0
    ? ipcTabs
    : DEFAULT_OFFICIAL_CHAT_TABS;
}
