<script setup>
// 官方对话窗口的页签栏：由 open_official_chat 在 official-chat 窗口顶部
// 挂的子 webview（label official-chat-strip）加载 index.html?chatstrip=1。
// 它是本地 SPA 内容——chat-fingerprint.js 不注入本 webview——所以保留
// window.__TAURI__，能调 official_chat_tabs 读取固定页签列表、调
// switch_official_chat_tab 切换活动内容 webview。活动页签状态本地维护
// （点击即由本组件发起），默认 0。
import { onMounted, ref } from 'vue';
import { invoke } from '../bridge.js';
import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from './officialChatTabs.js';

const tabs = ref([...DEFAULT_OFFICIAL_CHAT_TABS]);
const active = ref(0);
const switching = ref(false);

onMounted(async () => {
  try {
    tabs.value = resolveOfficialChatTabs(await invoke('official_chat_tabs'));
  } catch {
    // Keep the fallback labels visible when an older or restricted build rejects IPC.
  }
});

async function select(index) {
  if (switching.value || index === active.value) return;
  switching.value = true;
  try {
    await invoke('switch_official_chat_tab', { index });
    active.value = index;
  } catch (e) {
    // 切换失败：保持当前页签，不弹错（页签栏是无 chrome 浮层）。
  } finally {
    switching.value = false;
  }
}
</script>

<template>
  <div class="chat-strip">
    <button
      v-for="tab in tabs"
      :key="tab.index"
      type="button"
      class="chat-tab"
      :class="{ active: tab.index === active }"
      :disabled="switching"
      @click="select(tab.index)"
    >{{ tab.title }}</button>
  </div>
</template>

<style scoped>
.chat-strip {
  display: flex;
  align-items: stretch;
  height: 100%;
  margin: 0;
  padding: 0 8px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color);
  user-select: none;
  font-size: 13px;
  overflow: hidden;
}
.chat-tab {
  appearance: none;
  border: none;
  background: transparent;
  color: var(--el-text-color-secondary);
  padding: 0 16px;
  cursor: pointer;
  border-bottom: 2px solid transparent;
  white-space: nowrap;
  transition: color 0.15s;
}
.chat-tab:hover {
  color: var(--el-text-color-primary);
}
.chat-tab.active {
  color: var(--el-text-color-primary);
  border-bottom-color: var(--el-color-primary);
}
.chat-tab:disabled {
  cursor: default;
  opacity: 0.6;
}
</style>
