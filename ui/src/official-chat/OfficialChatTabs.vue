<script setup>
// 官方对话窗口的页签栏：由 open_official_chat 在 official-chat 窗口顶部
// 挂的子 webview（label official-chat-strip）加载 index.html?chatstrip=1。
// 它是本地 SPA 内容——chat-fingerprint.js 不注入本 webview——所以保留
// window.__TAURI__，能调 official_chat_tabs 读取固定页签列表、调
// switch_official_chat_tab 切换活动内容 webview。活动页签状态本地维护
// （点击即由本组件发起），默认 0。
import { onMounted, ref } from 'vue';
import { invoke } from '../shell/bridge.js';
import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from './officialChatTabs.js';

const tabs = ref([...DEFAULT_OFFICIAL_CHAT_TABS]);
const active = ref(0);
const switching = ref(false);

onMounted(async () => {
  try {
    tabs.value = resolveOfficialChatTabs(await invoke('official_chat_tabs'));
  } catch {
    // 较旧或受限的构建拒绝 IPC 时，回退标签保持可见。
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
      :style="tab.color ? { '--tab-color': tab.color } : null"
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
  /* 品牌主色由**后端随页签一起下发**（`--tab-color`，见 `official_chat_tabs`
     命令）；这里给的是没有品牌色时的兜底。页签栏是本地 SPA，不许出现十六
     进制字面量——写死后两处事实源就会分叉。 */
  color: var(--tab-color, var(--el-text-color-secondary));
  padding: 0 16px;
  cursor: pointer;
  border-bottom: 2px solid transparent;
  white-space: nowrap;
  transition: color 0.15s;
  /* 手写体（2026-10-24 用户拍板）：**全部用系统自带字体**，零产物增长，
     也不必为一个字形去引一套 webfont。macOS 走 Bradley Hand（清晰）/ Snell
     Roundhand（更圆润），Windows 走 Segoe Script / Comic Sans MS，最后由
     generic `cursive` 兜底。 */
  font-family: "Bradley Hand", "Snell Roundhand", "Segoe Script", "Comic Sans MS", cursive;
  /* 手写体比默认无衬线小一号就发糊，13px → 15px。 */
  font-size: 15px;
  /* 非激活态淡一档：三个站都是亮色品牌色，满色并排会互相抢。 */
  opacity: 0.72;
}
.chat-tab:hover {
  color: var(--tab-color, var(--el-text-color-primary));
  opacity: 0.9;
}
.chat-tab.active {
  color: var(--tab-color, var(--el-text-color-primary));
  /* 激活下划线与**该站的品牌色**同色，而不是壳自己的蓝：这一行的颜色语义
     已经是「哪个站」，下划线再走另一个色系会读成两件事。 */
  border-bottom-color: var(--tab-color, var(--el-color-primary));
  opacity: 1;
}
.chat-tab:disabled {
  cursor: default;
  opacity: 0.6;
}
</style>
