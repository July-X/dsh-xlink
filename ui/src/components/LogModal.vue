<script setup>
// 日志面板：顶部贴合的弹层，左侧竖排日志文件签（含大小），右侧按需读取内容。
// 文件签栏按可用宽度自动收缩成细轨（主窗口固定 480px，恒收缩），给日志正文
// 让出阅读宽度；用户可用栏顶按钮手动展开/收起（手动选择覆盖自动判定），
// 展开后点击右侧日志内容区会自动收起侧栏。
// 「全屏」打开一个独立的可缩放 OS 窗口展示同样的分类列表（见 LogViewerWindow.vue）。
// 分类侧栏的渲染由 LogSidebar 共享组件承担；侧栏与正文之间是 6px 可拖拽分隔条。
import { computed, nextTick, onBeforeUnmount, onUnmounted, ref, watch } from 'vue';
import { Refresh, Close, FullScreen } from '@element-plus/icons-vue';
import { invoke } from '../bridge.js';
import { toastError } from '../notify.js';
import { withLoading, isLoading } from '../loading.js';
import {
  bindScrollAutoHide,
  groupLogFiles,
  loadActiveLog,
  loadSidebarWidth,
  logModal,
  saveSidebarWidth,
  switchLogTab,
} from '../logs.js';
import LogSidebar from './LogSidebar.vue';
import PaneSplitter from './PaneSplitter.vue';

// 「全屏」：主壳窗口固定 480×800，日志阅读交给独立的可缩放 OS 窗口。
function openLogWindow() {
  if (!logModal.activeName) return;
  return withLoading('openLogWindow', () =>
    invoke('open_log_window', { name: logModal.activeName }).catch((e) => {
      // 后端已经给出「下一步」文案，优先原样展示。
      toastError((e && e.message) || '打开日志窗口失败：' + e);
    })
  );
}

const mainBox = ref(null);
const tabsBox = ref(null);
const groups = computed(() => groupLogFiles(logModal.files));

// 宽度低于该阈值时侧栏自动收缩（仅在没有手动覆盖时生效）。
const RAIL_COLLAPSE_WIDTH = 560;

const railOverride = ref(null); // null=自动；true/false=手动锁定
const railAuto = ref(false);
const railCollapsed = computed(() => railOverride.value ?? railAuto.value);

function toggleRail() {
  railOverride.value = !railCollapsed.value;
}

// 展开状态下点击日志内容区 → 收起侧栏，把宽度还给阅读。
function collapseRailOnContentClick() {
  if (!railCollapsed.value) {
    railOverride.value = true;
  }
}

// 侧栏宽度：拖拽期间只更新响应式宽度，松手时再写 localStorage（同
// LogViewerWindow 注释，避免每帧同步 I/O 卡住 UI）。默认 220px。
const SIDEBAR_WIDTH_KEY = 'dsh.logModal.sidebarWidth';
const SIDEBAR_WIDTH_MIN = 180;
const SIDEBAR_WIDTH_MAX = 420;

const sidebarWidth = ref(loadSidebarWidth(SIDEBAR_WIDTH_KEY, 'modal'));
function onSidebarWidthChange(next) {
  sidebarWidth.value = next;
}
function onSidebarDragEnd() {
  saveSidebarWidth(SIDEBAR_WIDTH_KEY, sidebarWidth.value);
}

let observer = null;
function disconnectRail() {
  if (!observer) return;
  observer.disconnect();
  observer = null;
}

function observeRail() {
  if (observer || !mainBox.value) return;
  observer = new ResizeObserver((entries) => {
    const width = entries[0] ? entries[0].contentRect.width : 0;
    if (width > 0) {
      railAuto.value = width < RAIL_COLLAPSE_WIDTH;
    }
  });
  observer.observe(mainBox.value);
}

// 弹层每次打开都重置为自动判定；对话框内容惰性挂载，open 后才接得上观察器。
watch(
  () => logModal.visible,
  async (visible) => {
    if (!visible) {
      disconnectRail();
      return;
    }
    railOverride.value = null;
    await nextTick();
    observeRail();
  }
);

onUnmounted(disconnectRail);

// 切签 / 列表刷新后把激活签滚进可视区。
watch(
  () => logModal.activeName,
  async () => {
    await nextTick();
    const root = tabsBox.value && tabsBox.value.$el;
    if (!root) return;
    const active = root.querySelector('.log-tab[aria-selected="true"]');
    if (active) {
      active.scrollIntoView({ block: 'nearest' });
    }
  }
);

// 滚动期间才显滚动条：与 LogViewerWindow 共用 logs.js::bindScrollAutoHide。
let unbindScroll = null;
watch(
  () => logModal.visible,
  async (visible) => {
    if (!visible) {
      if (unbindScroll) {
        unbindScroll();
        unbindScroll = null;
      }
      return;
    }
    await nextTick();
    const sidebarEl = tabsBox.value && tabsBox.value.$el;
    const contentEl = document.querySelector('.log-dialog .log-content');
    const cleanups = [bindScrollAutoHide(sidebarEl), bindScrollAutoHide(contentEl)].filter(Boolean);
    unbindScroll = () => cleanups.forEach((fn) => fn());
  }
);

onBeforeUnmount(() => {
  if (unbindScroll) unbindScroll();
});
</script>

<template>
  <el-dialog
    v-model="logModal.visible"
    title="日志"
    top="12px"
    width="min(860px, 92vw)"
    class="log-dialog"
    :show-close="false"
    append-to-body
  >
    <template #header>
      <div style="display: flex; align-items: center; gap: 8px">
        <span style="font-weight: 700; font-size: 15px">日志</span>
        <span style="flex: 1"></span>
        <el-button
          text
          :icon="FullScreen"
          :disabled="!logModal.activeName"
          :loading="isLoading('openLogWindow')"
          title="在新窗口中全屏查看当前日志"
          @click="openLogWindow"
        >
          全屏
        </el-button>
        <el-button text :icon="Refresh" :loading="logModal.loadingName === logModal.activeName && !!logModal.activeName" title="重新读取当前日志" @click="loadActiveLog">
          刷新
        </el-button>
        <el-button text :icon="Close" @click="logModal.visible = false">关闭</el-button>
      </div>
    </template>

    <div ref="mainBox" class="log-main">
      <LogSidebar
        ref="tabsBox"
        :style="{ '--sidebar-width': sidebarWidth + 'px' }"
        :groups="groups"
        :active-name="logModal.activeName"
        :rail-collapsed="railCollapsed"
        @select="switchLogTab"
        @toggle-rail="toggleRail"
      />
      <PaneSplitter
        v-if="!railCollapsed"
        :model-value="sidebarWidth"
        :min="SIDEBAR_WIDTH_MIN"
        :max="SIDEBAR_WIDTH_MAX"
        side="left"
        @update:model-value="onSidebarWidthChange"
        @drag-end="onSidebarDragEnd"
      />
      <div class="log-body" @click="collapseRailOnContentClick">
        <p v-if="railCollapsed && logModal.activeName" class="log-active-name" :title="logModal.activeName">
          {{ logModal.activeName }}
        </p>
        <pre class="log-content">{{ logModal.content }}</pre>
      </div>
    </div>
  </el-dialog>
</template>
