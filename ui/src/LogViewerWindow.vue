<script setup>
// 独立日志阅读窗口：主面板「全屏」按钮经 open_log_window 命令弹出，
// URL 查询串 ?log=<name> 指定初始文件。窗口带分类左栏（共享 LogSidebar
// 渲染），侧栏与正文之间是 6px 可拖拽分隔条；侧栏宽度持久化到
// localStorage，再次打开时复原。页头只剩文件名 + 刷新，关闭由 OS 窗口
// chrome 承担（macOS 红绿灯 / Windows × / Alt+F4）——避免和系统 chrome
// 重复且挤掉本就给日志内容让出的横向空间。滚动条默认透明，滚动期间才显。
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, watchEffect } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { invoke } from './bridge.js';
import { toastError, toastActionError } from './notify.js';
import { ioActive, isLoading, withLoading } from './loading.js';
import {
  bindScrollAutoHide,
  categorizeLogFile,
  displayLogText,
  groupLogFiles,
  loadSidebarWidth,
  saveSidebarWidth,
} from './logs.js';
import LogSidebar from './components/LogSidebar.vue';
import PaneSplitter from './components/PaneSplitter.vue';

const initialName = new URLSearchParams(location.search).get('log') || '';
const files = ref([]);
const activeName = ref(initialName);
const content = ref('读取中…');
const groups = computed(() => groupLogFiles(files.value));
const activeCategory = computed(() =>
  activeName.value ? categorizeLogFile(activeName.value) : null
);

// 同一窗口内复用 logs.js 的读取序号：避免慢的旧响应盖掉新结果。
let readSeq = 0;

function loadFile(name) {
  if (!name) {
    content.value = '';
    return Promise.resolve();
  }
  const target = name;
  const seq = ++readSeq;
  content.value = '读取中…';
  return invoke('read_log_file', { name: target })
    .then((text) => {
      if (seq !== readSeq || activeName.value !== target) return;
      content.value = displayLogText(text);
    })
    .catch((e) => {
      if (seq !== readSeq) return;
      const detail = e && e.message ? e.message : String(e);
      content.value = '读取失败：' + detail;
      toastError('读取失败：' + detail);
    });
}

function loadActiveFile() {
  return loadFile(activeName.value);
}

function switchFile(name) {
  if (name === activeName.value) {
    return loadActiveFile();
  }
  activeName.value = name;
  return loadActiveFile();
}

// 展开状态下点击日志内容区 → 收起侧栏，把宽度还给阅读。
function collapseRailOnContentClick() {
  if (!railCollapsed.value) {
    railOverride.value = true;
  }
}

function listFiles() {
  return invoke('list_log_files')
    .then((entries) => {
      files.value = entries || [];
      const names = files.value.map((f) => f.name);
      if (activeName.value && names.includes(activeName.value)) return;
      // 没有 URL 指定或 URL 文件已不存在 → 退回 kernel 分组第一个文件；
      // 没有 kernel 日志就退回任意第一个。
      const kernelGroup = groups.value.find((g) => g.id === 'kernel');
      const fallback = (kernelGroup && kernelGroup.files[0] && kernelGroup.files[0].name) || names[0] || '';
      activeName.value = fallback;
    })
    .catch((e) =>
      toastActionError(
        '读取日志列表失败',
        e,
        '请点击「刷新」重试，或打开数据目录查看 logs/',
        4000,
      ),
    );
}

function refreshAll() {
  // 「刷新」= 重新列文件 + 重读当前文件（与主面板 refreshLogTabs 一致）
  return listFiles().then(() => {
    if (activeName.value) return loadActiveFile();
    return null;
  });
}

const tabsBox = ref(null);

// 宽度低于该阈值时侧栏自动收缩（仅在没有手动覆盖时生效）。
const RAIL_COLLAPSE_WIDTH = 560;

const railOverride = ref(null); // null=按窗口宽度自动判定；true/false=手动锁定
const railAuto = ref(false);
const railCollapsed = computed(() => railOverride.value ?? railAuto.value);

function toggleRail() {
  railOverride.value = !railCollapsed.value;
}

// 侧栏宽度：可拖拽分隔条驱动；持久化到 localStorage 让再次打开保持习惯。
// 默认 240px，范围 180-560 写在 logs.js::SIDEBAR_WIDTH_DEFAULTS。
//
// 关键：拖拽过程中**不**写 localStorage。`saveSidebarWidth` 是同步 I/O，
// 高 DPI 鼠标 1 秒能触发上百次 mousemove，每次都写磁盘会让拖拽明显卡顿。
// 这里只在更新响应式宽度（驱动 UI 重绘），松手时再持久化一次。
const SIDEBAR_WIDTH_KEY = 'dsh.logViewer.sidebarWidth';
const SIDEBAR_WIDTH_MIN = 180;
const SIDEBAR_WIDTH_MAX = 560;

const sidebarWidth = ref(loadSidebarWidth(SIDEBAR_WIDTH_KEY, 'window'));
function onSidebarWidthChange(next) {
  sidebarWidth.value = next;
}
function onSidebarDragEnd() {
  saveSidebarWidth(SIDEBAR_WIDTH_KEY, sidebarWidth.value);
}

// 与管理壳一致：本窗口内 IO 进行中点亮标题栏鲸眼脉冲。
watchEffect(() => {
  document.body.classList.toggle('pulse-active', ioActive.value);
});

// 切到新文件后把激活签滚进可视区（按分类定位到所在组）。
watch(activeName, async () => {
  await nextTick();
  const root = tabsBox.value && tabsBox.value.$el;
  if (!root) return;
  const active = root.querySelector('.log-tab[aria-selected="true"]');
  if (active) active.scrollIntoView({ block: 'nearest' });
});

// 滚动期间才显滚动条：scroll 事件加 `.is-scrolling`，800ms 无新滚动则移除。
// 实现走 logs.js::bindScrollAutoHide 共享（与 LogModal 共用），不再各写一份。
let unbindScroll = null;

onMounted(() => {
  refreshAll();
  // 窗口尺寸变化联动侧栏自动收缩（与 LogModal 行为一致）
  const observer = new ResizeObserver((entries) => {
    const width = entries[0] ? entries[0].contentRect.width : 0;
    if (width > 0) {
      railAuto.value = width < RAIL_COLLAPSE_WIDTH;
    }
  });
  observer.observe(document.body);

  // 挂上「滚动期间才显」行为：侧栏与正文各自独立计时
  nextTick(() => {
    const sidebarEl = tabsBox.value && tabsBox.value.$el;
    const contentEl = document.querySelector('.logwin-content');
    const cleanups = [bindScrollAutoHide(sidebarEl), bindScrollAutoHide(contentEl)].filter(Boolean);
    unbindScroll = () => cleanups.forEach((fn) => fn());
  });
});

onBeforeUnmount(() => {
  if (unbindScroll) unbindScroll();
});
</script>

<template>
  <div class="logwin">
    <header class="logwin-head">
      <img src="/whale-icon.png" alt="" width="22" height="22" />
      <span class="logwin-title" :title="activeName || ''">{{ activeName || '日志' }}</span>
      <!-- 分类徽章：只在侧栏展开时显示。收起到 34px 细轨后，文件名首段
           （kernel / install / plugin-...）已经能告诉用户这是哪类日志，
           徽章只是占横向空间让标题过早 ellipsis。 -->
      <span
        v-if="activeCategory && !railCollapsed"
        class="logwin-category"
        :title="groups.find((g) => g.id === activeCategory)?.label"
      >
        {{ groups.find((g) => g.id === activeCategory)?.label }}
      </span>
      <span style="flex: 1"></span>
      <el-button
        text
        :icon="Refresh"
        :loading="isLoading('logwinRefresh')"
        title="重新读取当前日志"
        @click="withLoading('logwinRefresh', () => refreshAll())"
      >
        刷新
      </el-button>
      <!-- 关闭键由 OS 窗口 chrome 承担（macOS 红绿灯 / Windows × / Alt+F4） -->
    </header>
    <div class="logwin-main">
      <LogSidebar
        ref="tabsBox"
        :style="{ '--sidebar-width': sidebarWidth + 'px' }"
        :groups="groups"
        :active-name="activeName"
        :rail-collapsed="railCollapsed"
        @select="switchFile"
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
      <main class="log-body logwin-body" @click="collapseRailOnContentClick">
        <pre v-if="content" class="log-content logwin-content">{{ content }}</pre>
        <div v-else class="logwin-empty">
          <span>请从左侧选择日志文件</span>
          <span class="logwin-empty-hint">或点击右上角「刷新」重读列表</span>
        </div>
      </main>
    </div>
  </div>
</template>
