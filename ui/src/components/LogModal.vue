<script setup>
// 日志面板：顶部贴合的弹层，左侧竖排日志文件签（含大小），右侧按需读取内容。
// 文件签栏按可用宽度自动收缩成细轨（主窗口固定 480px，恒收缩），给日志正文
// 让出阅读宽度；用户可用栏顶按钮手动展开/收起（手动选择覆盖自动判定），
// 展开后点击右侧日志内容区会自动收起侧栏。
// 「全屏」打开一个独立的满窗弹层展示当前文件内容，自带刷新 / 关闭按钮。
import { computed, nextTick, ref, watch } from 'vue';
import { Refresh, Close, FullScreen, Fold, Expand } from '@element-plus/icons-vue';
import { invoke } from '../bridge.js';
import { toastError } from '../notify.js';
import { withLoading, isLoading } from '../loading.js';
import { logModal, formatLogSize, switchLogTab, loadActiveLog } from '../logs.js';

// 「全屏」：主壳窗口固定 480×800，日志阅读交给独立的可缩放 OS 窗口。
function openLogWindow() {
  if (!logModal.activeName) return;
  return withLoading('openLogWindow', () =>
    invoke('open_log_window', { name: logModal.activeName }).catch((e) => toastError('打开日志窗口失败：' + e))
  );
}

const tabsBox = ref(null);
const mainBox = ref(null);

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

let observer = null;
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
    if (!visible) return;
    railOverride.value = null;
    await nextTick();
    observeRail();
  }
);

// 切签 / 列表刷新后把激活签滚进可视区。
watch(
  () => logModal.activeName,
  async () => {
    await nextTick();
    const active = tabsBox.value && tabsBox.value.querySelector('.log-tab[aria-selected="true"]');
    if (active) {
      active.scrollIntoView({ block: 'nearest' });
    }
  }
);
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
        <el-button text :icon="Refresh" :loading="logModal.loading" title="重新读取当前日志" @click="loadActiveLog">
          刷新
        </el-button>
        <el-button text :icon="Close" @click="logModal.visible = false">关闭</el-button>
      </div>
    </template>

    <div ref="mainBox" class="log-main">
      <div ref="tabsBox" class="log-tabs" :class="{ collapsed: railCollapsed }" role="tablist" aria-orientation="vertical">
        <button
          type="button"
          class="rail-toggle"
          :title="railCollapsed ? '展开日志列表' : '收起日志列表'"
          @click="toggleRail"
        >
          <el-icon><Expand v-if="railCollapsed" /><Fold v-else /></el-icon>
        </button>
        <template v-if="!railCollapsed">
          <span v-if="!logModal.files.length" class="log-tab-size">（暂无日志文件）</span>
          <button
            v-for="f in logModal.files"
            :key="f.name"
            type="button"
            class="log-tab"
            role="tab"
            :aria-selected="f.name === logModal.activeName ? 'true' : 'false'"
            :title="f.name"
            @click="switchLogTab(f.name)"
          >
            <span>{{ f.name }}</span>
            <span v-if="typeof f.size === 'number'" class="log-tab-size">{{ formatLogSize(f.size) }}</span>
          </button>
        </template>
      </div>
      <div class="log-body" @click="collapseRailOnContentClick">
        <p v-if="railCollapsed && logModal.activeName" class="log-active-name" :title="logModal.activeName">
          {{ logModal.activeName }}
        </p>
        <pre class="log-content">{{ logModal.content }}</pre>
      </div>
    </div>
  </el-dialog>
</template>
