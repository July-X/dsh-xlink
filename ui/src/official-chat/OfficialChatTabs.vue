<script setup>
// 官方对话窗口的页签栏：由 open_official_chat 在 official-chat 窗口顶部
// 挂的子 webview（label official-chat-strip）加载 index.html?chatstrip=1。
// 它是本地 SPA 内容——chat-fingerprint.js 不注入本 webview——所以保留
// window.__TAURI__，能调 official_chat_tabs 读取固定页签列表、调
// switch_official_chat_tab 切换活动内容 webview。活动页签状态本地维护
// （点击即由本组件发起），默认 0。
//
// **加载指示**（2026-10-24）：内容页签是按需挂载 + 后台预热的，某个站点的
// 首屏可能要好几秒。后端随页签一起下发 `loading`（那一页还在加载），
// 这里据此转一个小圈——加载期不给任何反馈时，用户读到的是「卡死了」。
import { onMounted, onUnmounted, ref } from 'vue';
import { invoke } from '../shell/bridge.js';
import { DEFAULT_OFFICIAL_CHAT_TABS, resolveOfficialChatTabs } from './officialChatTabs.js';

const tabs = ref([...DEFAULT_OFFICIAL_CHAT_TABS]);
const active = ref(0);
const switching = ref(false);
let pollTimer = null;
/** 连续轮询次数上限：某个站点一直加载不完时，不无限轮询下去。 */
const POLL_LIMIT = 60;
/** 已经轮询的次数。 */
let pollAttempts = 0;

function clearPollTimer() {
  if (pollTimer) {
    clearTimeout(pollTimer);
    pollTimer = null;
  }
}

/**
 * 还在加载时继续轮询；全部就绪就停。
 *
 * 只有「有页签还在加载」才轮询——页签栏是常驻的小控件，无意义的定时器
 * 会一直占着事件循环。轮询到顶（[`POLL_LIMIT`]）也停，但停之前先把转圈
 * 收掉：停在上一次读数上的「加载中」会永远转下去，读作「卡死」。真实的
 * 加载态等用户下一次点击——`select` 会重置计数并重新拉一次状态。
 */
function schedulePoll(list) {
  clearPollTimer();
  if (!list.some((tab) => tab.loading)) {
    pollAttempts = 0;
    return;
  }
  if (pollAttempts >= POLL_LIMIT) {
    tabs.value = list.map((tab) => ({ ...tab, loading: false }));
    return;
  }
  pollTimer = setTimeout(() => {
    pollAttempts += 1;
    refresh();
  }, 500);
}

/** 拉一次页签状态（颜色 + 加载态）。读不到就保持现状，下次点击再试。 */
async function refresh() {
  try {
    const next = resolveOfficialChatTabs(await invoke('official_chat_tabs'));
    tabs.value = next;
    schedulePoll(next);
  } catch {
    // IPC 不可用（浏览器里直接开这个路由）时保持兜底表。
  }
}

onMounted(refresh);

onUnmounted(clearPollTimer);

async function select(index) {
  if (switching.value || index === active.value) return;
  switching.value = true;
  const previous = active.value;
  // **乐观高亮**：用户点了就该立刻有反馈。内容区随后跟上——后端在那一页
  // 加载完成的那一刻才切换过去（见 `OfficialChatRuntime`），这段等待由
  // 转圈表达。
  active.value = index;
  try {
    await invoke('switch_official_chat_tab', { index });
    // 用户点了这个页签、正在等它就绪——轮询额度重置，接着替他盯完这一段。
    pollAttempts = 0;
    await refresh();
  } catch (e) {
    // 切换失败：回到原页签，不弹错（页签栏是无 chrome 浮层）。
    active.value = previous;
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
      :class="{ active: tab.index === active, loading: tab.loading }"
      :style="tab.color ? { '--tab-color': tab.color } : null"
      :disabled="switching"
      @click="select(tab.index)"
    >
      <span class="chat-tab-label">{{ tab.title }}</span>
      <span v-if="tab.loading" class="chat-tab-spinner" role="status" aria-label="加载中"></span>
    </button>
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
/* 加载中的页签淡一档，让转圈更显眼。 */
.chat-tab.loading .chat-tab-label {
  opacity: 0.75;
}
/* 加载指示：一个缺口圆环在转。
   它是**壳自己的 chrome**（不是注入到站点里的样式），所以可以用
   animation——站点页面里动一下就可能触发重排，那里的规矩见
   `titlebar-pulse.js` 的注释。 */
.chat-tab-spinner {
  display: inline-block;
  width: 9px;
  height: 9px;
  margin-left: 6px;
  border: 1.5px solid currentColor;
  border-right-color: transparent;
  border-radius: 50%;
  animation: chat-tab-spin 0.7s linear infinite;
  vertical-align: -1px;
}
@keyframes chat-tab-spin {
  to {
    transform: rotate(360deg);
  }
}
/* 用户要求减少动效时：不再转，但缺口圆环仍然读得出「这是个状态标记」。 */
@media (prefers-reduced-motion: reduce) {
  .chat-tab-spinner {
    animation: none;
  }
}
</style>
