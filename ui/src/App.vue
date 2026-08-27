<script setup>
// 应用骨架：侧栏 + 面板切换 + 全局浮层（进度 / 日志 / 事故），
// 以及启动时的事件监听、轮询与静默自检的编排。
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue';
import { invoke, listen } from './bridge.js';
import { toastError, confirmDialog } from './notify.js';
import { ioActive } from './loading.js';
import {
  store,
  refreshAll,
  pollStatus,
  checkShellUpdate,
  showShellUpdateBanner,
} from './store.js';
import { loadCatalog, checkPluginUpdates } from './plugins.js';
import { checkSkillUpdates } from './skills.js';
import SideBar from './components/SideBar.vue';
import OverviewPanel from './components/OverviewPanel.vue';
import VersionsPanel from './components/VersionsPanel.vue';
import PluginsPanel from './components/PluginsPanel.vue';
import SkillsPanel from './components/SkillsPanel.vue';
import SettingsPanel from './components/SettingsPanel.vue';
import ProgressOverlay from './components/ProgressOverlay.vue';
import LogModal from './components/LogModal.vue';
import IncidentModal from './components/IncidentModal.vue';
import DebugPanel from './components/DebugPanel.vue';

const PANELS = {
  overview: OverviewPanel,
  versions: VersionsPanel,
  plugins: PluginsPanel,
  skills: SkillsPanel,
  settings: SettingsPanel,
};

// 标题栏鲸眼脉冲 = 业务活动指示：按钮触发的 IO（withLoading / withProgress）
// 或工作台启动编排进行中时点亮，全部结束后消失。后台轮询与静默自检不点亮。
//
// 扫光周期 7.68s、前 ~15%（约 1.15s）还在左侧淡入——若操作几百毫秒就完成，
// 脉冲会在扫进可视区之前被摘除（点了像没反应）。所以点亮后至少保持一个
// 最小可见窗口；操作本身超过该窗口时结束即消失。
const PULSE_MIN_MS = 2400;

const pulseOn = ref(false);
let pulseActivatedAt = 0;
let pulseOffTimer = null;

const busySignal = computed(() => ioActive.value || store.starting);
watch(busySignal, (active) => {
  if (active) {
    pulseActivatedAt = Date.now();
    if (pulseOffTimer) {
      clearTimeout(pulseOffTimer);
      pulseOffTimer = null;
    }
    pulseOn.value = true;
    return;
  }
  if (!pulseOn.value) return;
  const remain = Math.max(0, PULSE_MIN_MS - (Date.now() - pulseActivatedAt));
  pulseOffTimer = setTimeout(() => {
    pulseOffTimer = null;
    pulseOn.value = false;
  }, remain);
});

watchEffect(() => {
  document.body.classList.toggle('pulse-active', pulseOn.value);
});

let pollTimer = null;
const startupTimers = [];

// 完全退出确认：Rust 侧在内核运行或官方对话打开时拦截主窗口关闭（prevent_close），
// 由这里弹确认框；用户确认后先停内核（释放端口），再经 confirm_close_shell
// 销毁全部窗口并退出（Rust 侧负责收尾，不依赖 RunEvent::Exit 关窗）。
// pending 标记压住用户在弹窗期间连续点 X 的重入。
let quitConfirmPending = false;
function onQuitConfirmRequest(event) {
  if (quitConfirmPending) return;
  const payload = event && event.payload ? event.payload : {};
  const kernelRunning = !!payload.kernel_running;
  const chatOpen = !!payload.official_chat_open;
  let detail;
  if (kernelRunning && chatOpen) {
    detail = '工作台与官方对话窗口仍在运行。关闭主壳会一并关闭它们；继续吗？';
  } else if (chatOpen) {
    detail = '官方对话窗口仍打开。关闭主壳会一并关闭它（登录状态已保留）；继续吗？';
  } else {
    detail = '工作台仍在运行。关闭主壳前需要先关闭工作台；继续吗？';
  }
  quitConfirmPending = true;
  confirmDialog('完全退出？', detail, '关闭并退出')
    .then((ok) => {
      if (!ok) return null;
      const stop = kernelRunning
        ? invoke('stop_kernel').catch((e) => toastError('关闭工作台失败：' + e))
        : Promise.resolve();
      return stop
        .then(() => invoke('confirm_close_shell'))
        .catch((e) => toastError('退出失败：' + e + '（请手动关闭窗口）', 6000));
    })
    .finally(() => {
      quitConfirmPending = false;
    });
}

function onVisibilityChange() {
  if (!document.hidden) {
    pollStatus();
  }
}

onMounted(() => {
  refreshAll();

  // 状态轮询：窗口隐藏时整个跳过；重新可见时立即补一轮。
  pollTimer = setInterval(pollStatus, 2500);
  document.addEventListener('visibilitychange', onVisibilityChange);

  // 外壳后台检查到新版后广播此事件；手动按钮覆盖按需检查。
  listen('shell-update-available', (e) => showShellUpdateBanner(e.payload));
  listen('request-quit-confirm', onQuitConfirmRequest);

  // 启动后的静默预载 / 自检：失败均不打断，用户可在对应面板手动重试。
  startupTimers.push(setTimeout(() => loadCatalog(false), 1200));
  startupTimers.push(setTimeout(() => checkPluginUpdates({ busy: false, toastOnUpdates: true }), 3500));
  startupTimers.push(setTimeout(() => checkSkillUpdates({ busy: false, toastOnUpdates: true }), 4200));
  startupTimers.push(setTimeout(() => checkShellUpdate(false), 2500));
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
  startupTimers.forEach(clearTimeout);
  document.removeEventListener('visibilitychange', onVisibilityChange);
});
</script>

<template>
  <div class="layout">
    <SideBar />
    <main>
      <Transition name="panel" mode="out-in">
        <component :is="PANELS[store.activePanel]" :key="store.activePanel" />
      </Transition>
    </main>
  </div>
  <ProgressOverlay />
  <LogModal />
  <IncidentModal />
  <DebugPanel />
</template>
