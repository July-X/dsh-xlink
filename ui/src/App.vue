<script setup>
// 应用骨架：侧栏 + 面板切换 + 全局浮层（进度 / 日志 / 事故），
// 以及启动时的事件监听、轮询与静默自检的编排。
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue';
import { invoke, listen } from './shell/bridge.js';
import { toast, toastError, confirmDialog, toastWithCheckbox } from './shell/notify.js';
import { renderErrors, clearRenderError, reloadPanel } from './shell/errors.js';
import { globalBusy, ioActive } from './shell/loading.js';
import {
  store,
  refreshAll,
  pollStatus,
  checkUpdates,
  showShellUpdateBanner,
  showIncident,
} from './store.js';
import { loadCatalog, checkPluginUpdates } from './plugins/plugins.js';
import { loadInstances } from './kernel/instance.js';
import { checkSkillUpdates } from './skills/skills.js';
import { loadUsageSummary } from './usage/usage.js';
import { applyNotificationStatus } from './incidents/notifications.js';
import { diagnosticStore } from './diagnostics/diagnostics.js';
import { maybeOpenMigrationPrompt } from './migration/migration.js';
import SideBar from './shell/SideBar.vue';
import OverviewPanel from './shell/OverviewPanel.vue';
import VersionsPanel from './kernel/VersionsPanel.vue';
import PluginsPanel from './plugins/PluginsPanel.vue';
import SkillsPanel from './skills/SkillsPanel.vue';
import SettingsPanel from './shell/SettingsPanel.vue';
import MigrationPanel from './migration/MigrationPanel.vue';
import MigrationPrompt from './migration/MigrationPrompt.vue';
import ProgressOverlay from './shell/ProgressOverlay.vue';
import LogModal from './logs/LogModal.vue';
import IncidentModal from './incidents/IncidentModal.vue';
import PrecheckDialog from './plugins/PrecheckDialog.vue';
import SnapshotRestoreDialog from './diagnostics/SnapshotRestoreDialog.vue';
import DiagnosisShell from './diagnostics/DiagnosisShell.vue';
import DebugPanel from './shell/DebugPanel.vue';
import WindowTitleBar from './shell/WindowTitleBar.vue';

const PANELS = {
  overview: OverviewPanel,
  versions: VersionsPanel,
  plugins: PluginsPanel,
  skills: SkillsPanel,
  settings: SettingsPanel,
  migration: MigrationPanel,
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
let appDisposed = false;
const appUnlisteners = [];

function registerAppListener(event, handler) {
  let pending;
  try {
    pending = listen(event, handler);
  } catch {
    return;
  }
  if (!pending) return;
  Promise.resolve(pending)
    .then((unlisten) => {
      if (typeof unlisten !== 'function') return;
      if (appDisposed) {
        Promise.resolve(unlisten()).catch(() => {});
      } else {
        appUnlisteners.push(unlisten);
      }
    })
    .catch(() => {});
}

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
  let title = '完全退出？';
  let detail;
  if (kernelRunning && chatOpen) {
    detail = '工作台与官网网页版窗口仍在运行。关闭主壳会一并关闭它们；继续吗？';
  } else if (chatOpen) {
    detail = '官网网页版窗口仍打开。关闭主壳会一并关闭它（登录状态已保留）；继续吗？';
  } else if (kernelRunning) {
    detail = '工作台仍在运行。关闭主壳前需要先关闭工作台；继续吗？';
  } else {
    // 图标菜单「退出」是无条件广播的：内核没跑、官方对话也没开时同样会走到这里，
    // 旧文案却断言"工作台仍在运行"，与实际状态相反（P1-4）。
    detail = '当前没有运行中的工作台或官网网页版窗口。退出会关闭管理面板与常驻图标；继续吗？';
  }
  // 常驻入口图标（托盘 / 菜单栏）的「退出」走同一条确认流程，但语义是终止
  // 后台常驻进程，而不是关闭一个已经收起的窗口——标题直说「退出」避免误解。
  //
  // 2026-10-02 起这条分支两平台共用：关闭按钮一律只收起窗口，退出只从这里
  // 发起（此前只有 Windows 走托盘，macOS 的关闭按钮会直接落到本函数）。
  if (payload.from_background) {
    title = '退出 Dsh-Xlink？';
    detail = '退出后内核与工作台会一并停止，常驻图标也会消失。' + detail;
  }
  quitConfirmPending = true;
  confirmDialog(title, detail, payload.from_background ? '退出' : '关闭并退出')
    .then((ok) => {
      if (!ok) return null;
      // 停内核失败时**不退出**：catch 只提示不 resolve，走下面的错误分支收口。
      // 旧写法把 catch 挂在内层，catch 返回 undefined 让 Promise 继续 resolve，
      // 于是 stop_kernel 失败后照样执行 confirm_close_shell——内核还在跑，
      // 主壳却已经销毁，用户既看不到端口被占也失去重新停止的入口。
      const stop = kernelRunning
        ? invoke('stop_kernel').catch((e) => {
            toastError('关闭工作台失败：' + e + '，已取消退出');
            throw e;
          })
        : Promise.resolve();
      return stop
        .then(() => invoke('confirm_close_shell'))
        .catch((e) => {
          if (kernelRunning) {
            toastError('工作台可能仍在运行，请从概览页停止后再退出', 6000);
            return;
          }
          toastError('退出失败：' + e + '（请手动关闭窗口）', 6000);
        });
    })
    .finally(() => {
      quitConfirmPending = false;
    });
}

function refreshActivePanelData() {
  if (document.hidden) return;
  if (store.activePanel === 'plugins') {
    // 目录与更新检查互不依赖（更新检查不消费 catalog 的结果），并行跑；
    // 原来的 .then 串行让冷启动多等一趟网络才发起第二趟。
    // 静默：这次搜索是打开面板带出来的，用户没做任何操作，失败不弹提示。
    loadCatalog();
    checkPluginUpdates({ busy: false, toastOnUpdates: true });
  } else if (store.activePanel === 'skills') {
    checkSkillUpdates({ busy: false, toastOnUpdates: true });
  } else if (store.activePanel === 'overview') {
    // 概览卡片上的「今日用量」：窗口重新可见 / 切回概览时立即对一次账。
    // 概览页自己挂着 60s 定时刷新，但窗口从后台回来的那一刻正是用户要读数的
    // 那一刻，等下一个 tick 只会让两个面板的数字继续差着。TTL 守卫兜底。
    loadUsageSummary();
  }
}

watch(() => store.activePanel, refreshActivePanelData);
watch(globalBusy, (busy, previous) => {
  if (previous && !busy) refreshActivePanelData();
});

function onVisibilityChange() {
  if (!document.hidden) {
    pollStatus();
    refreshActivePanelData();
  }
}

onMounted(() => {
  // 先完成首屏状态刷新，再让 Rust 确认新 Shell 已经能运行；Windows
  // 只有这一步之后才会清理更新前的安装目录和 updater 临时文件。
  refreshAll()
    .then(() => invoke('confirm_shell_ready'))
    .catch((e) => toastError('更新后的旧版本清理未完成：' + e, 6000));

  // 历史数据迁移弹窗：扫到遗留数据 + 用户未拒绝过时自动弹。
  // 失败（preview / skip 读取异常）静默忽略——主流程不受影响。
  maybeOpenMigrationPrompt().catch(() => {});

  // 实例注册表：插件页「所有实例」tab 与概览页的当前实例上下文都消费这份
  // 列表，在根组件保证它启动即加载。
  loadInstances().catch(() => {});

  // 内核发布列表：启动静默拉一次，用户进内核版本页时列表已经就位，不必先点
  // 一下「检查更新」。静默路径的失败处理见 store.js 的 checkUpdates 注释
  // （不弹提示、不清空已有列表）。后端有 60 秒进程缓存，用户刚开就手动点
  // 「检查更新」不会打第二次网络。
  //
  // 一条已知边界：`--autostart` 启动时窗口是收起的，此时发出来的 toast
  // 用户看不见、到点自消。数据本身不受影响——他打开应用进内核版本页就能
  // 看到那一版与「安装」按钮，提示只是顺手指个路，不是唯一入口。
  checkUpdates(false);

  // 状态轮询：窗口隐藏时整个跳过；重新可见时立即补一轮。
  pollTimer = setInterval(pollStatus, 2500);
  document.addEventListener('visibilitychange', onVisibilityChange);

  // 外壳后台检查到新版后广播此事件；手动按钮覆盖按需检查。
  registerAppListener('shell-update-available', (e) => showShellUpdateBanner(e.payload));
  // 两平台（2026-10-02 起统一）：标题栏的关闭都只是把窗口收进后台——macOS
  // 收进菜单栏、Windows 收进通知区域（内核与工作台继续运行，任务栏与 Dock
  // 不再保留一个点了没反应的条目）。提示只能在窗口可见时讲：收起那一刻
  // 窗口已经隐藏，页内 toast 渲染在那里没人看得见（P1-3），所以 Rust 改为在
  // **从后台恢复**时补发这个事件，此刻说清"刚才去哪了、怎么再找回来"才有意义。
  //
  // 恢复一定是用户点常驻图标（或拉工作台拉绳）主动做出来的，他刚证明自己知道
  // 怎么把窗口叫回来，重复讲只会挡视线：Rust 侧只在本进程发生过一次用户收起
  // （关窗 / Windows 最小化）之后的**第一次恢复**补发这个事件——登录自启的
  // 隐藏不算用户动作，开机后第一次唤回是静默的。其余恢复一律静默。
  //
  // toast 上带「不再提示」勾选（2026-10-05 用户要求）：勾选写 localStorage
  // （跨启动保留；dev 与 release 两个 webview 各存各的，偏好天然分壳），
  // 此后这个壳里该提示永远静默。停留给到 8 秒，用户得有时间注意到勾选框。
  const RESTORE_HINT_SUPPRESSED_KEY = 'restore-hint-suppressed';
  registerAppListener('shell-restored-from-background', () => {
    if (localStorage.getItem(RESTORE_HINT_SUPPRESSED_KEY) === '1') return;
    toastWithCheckbox(
      '刚才只是把窗口收进了后台：程序继续运行，点菜单栏 / 托盘的鲸鱼图标可重新打开，右键菜单里可退出',
      '不再提示',
      () => localStorage.setItem(RESTORE_HINT_SUPPRESSED_KEY, '1'),
      8000
    );
  });
  registerAppListener('harness-fault', (e) => {
    showIncident(e && e.payload);
    refreshAll();
  });
  registerAppListener('request-quit-confirm', onQuitConfirmRequest);
  // 任务完成通知的状态由 Rust 侧持有（角标也在那边），它每次变化都会广播
  // 一份快照；面板据此自动刷新未读数，不需要用户点「刷新」。
  registerAppListener('notification-status', (e) => applyNotificationStatus(e && e.payload));
  // 「点通知横幅回到工作台」失败（例如这期间内核已经停了）。横幅点击是静默的，
  // 窗口没动时用户只会以为"点了没反应"——Rust 已把管理面板叫回前台，这里
  // 负责把他为什么还在面板上、该做什么讲清楚。
  registerAppListener('workbench-activate-failed', (e) => {
    const reason = String((e && e.payload) || '未知原因');
    toastError(
      `回到工作台失败：${reason}。可先在概览页确认工作台状态（若已停止，重新启动后任务现场仍会保留）`,
      8000
    );
  });

  // 目录与更新检查（插件 / 技能）由 activePanel watcher 按需触发；桌面端自身的
  // 更新检查由 Rust 后台任务负责（updater::spawn_background_check，setup 里起），
  // 不走前端。内核发布列表的启动自检在上面 onMounted 里发一次。
});

onUnmounted(() => {
  appDisposed = true;
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = null;
  if (pulseOffTimer) clearTimeout(pulseOffTimer);
  pulseOffTimer = null;
  for (const unlisten of appUnlisteners.splice(0)) {
    try {
      Promise.resolve(unlisten()).catch(() => {});
    } catch {
      // 监听器可能已被 WebView 提前拆掉。
    }
  }
  document.removeEventListener('visibilitychange', onVisibilityChange);
});
</script>

<template>
  <div class="app-shell">
    <div v-if="renderErrors.message" class="render-error-fallback">
      <el-alert
        type="error"
        :closable="false"
        show-icon
        :title="renderErrors.title"
        :description="renderErrors.message"
      />
      <div class="render-error-actions">
        <el-button type="primary" size="small" @click="reloadPanel">重新加载面板</el-button>
        <el-button size="small" @click="clearRenderError">忽略并继续</el-button>
      </div>
    </div>
    <WindowTitleBar />
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
    <PrecheckDialog />
    <SnapshotRestoreDialog />
    <DebugPanel />
    <!-- 主窗口 mount 后弹窗：检测到旧版数据 + 用户未拒绝过时弹。
         MigrationPanel 是「数据迁移」侧栏面板（手动重跳 / 查历史 / 回滚），
         与本弹窗并存：弹窗负责首次发现提示，面板负责反复操作。 -->
    <MigrationPrompt />
    <!-- 诊断层覆盖当前面板：启动失败时用户正要回到日志 / 换端口 / 回退快照，
         另开窗口会让这些动作变成跨窗口来回拖。放在最后，z-index 高于
         进度浮层与事故弹窗——用户点它就是要压住那些。 -->
    <DiagnosisShell v-if="diagnosticStore.active" />
  </div>
</template>
