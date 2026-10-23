<script setup>
// 官方对话窗口的页签栏：由 open_official_chat 在 official-chat 窗口顶部
// 挂的子 webview（label official-chat-strip）加载 index.html?chatstrip=1。
// 它是本地 SPA 内容——chat-fingerprint.js 不注入本 webview——所以保留
// window.__TAURI__，能调 official_chat_tabs 读取固定页签列表、调
// switch_official_chat_tab 切换活动内容 webview。活动页签状态本地维护
// （点击即由本组件发起），默认 0。
//
// 这条 strip 分两段（见 Rust 的 OFFICIAL_CHAT_STRIP_HEIGHT）：
//   · 上半段「页签行」：页签控件 + 右上角那盏紧凑小台灯（台灯是注入脚本
//     画的，24×38，正好等于页签行高度）。
//   · 下半段「品牌带」：一条实心的官方蓝长条。2026-10-23 用户拍板：两端
//     走直角（不用渐变淡出，那读作圆角），高度取台灯高度减去五分之一。
//     品牌带属于壳自己的 chrome，内容页因此被整体下移——不会有任何一条
//     蓝带压到 chatgpt.com / chat.deepseek.com 自己的页头上。
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
    <div class="chat-tabs">
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
    <!-- 品牌带只是装饰，不承载任何交互，因此对辅助技术隐藏。 -->
    <div class="chat-band" aria-hidden="true"></div>
  </div>
</template>

<style scoped>
.chat-strip {
  display: flex;
  /* 竖排：上半段页签行，下半段品牌带（带子必须在页签**下面**）。 */
  flex-direction: column;
  height: 100%;
  margin: 0;
  padding: 0;
  background: var(--el-bg-color);
  user-select: none;
  font-size: 13px;
  overflow: hidden;
}
.chat-tabs {
  display: flex;
  align-items: stretch;
  /* 页签行高度 = 台灯高度（Rust 的 OFFICIAL_CHAT_TAB_ROW_HEIGHT）。 */
  height: 38px;
  flex: none;
  margin: 0;
  padding: 0 8px;
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
.chat-band {
  /* 品牌带高度 = 台灯高度(38) − 1/5 = 30.4（Rust 的
     OFFICIAL_CHAT_BAND_HEIGHT 是同一个数；两边对不上就会在带子下沿露出
     一条缝或压住内容页，判据 official-chat-brand-band 会比对）。 */
  height: 30.4px;
  flex: none;
  /* 两端走直角：这里不能出现任何非零圆角，否则读作圆角过渡。 */
  border-radius: 0;
  /* 官方品牌蓝（theme.css 的 --official-brand，与拉绳小台灯的绳色同值；
     判据会比对这两个数）。这里读 token 不写死颜色——写死后主题广播与
     品牌带就会分叉（designAlignment 有一条判据专门钉这个）。 */
  background: var(--official-brand);
}
</style>
