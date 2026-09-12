<script setup>
// 设置：Web UI 端口、插件接线 profile 名、Node 检测、任务完成通知，以及内置补丁
// （内核补丁 / 小插件）的应用与撤销。轮询每 2.5s 刷新 store.view，但用户正在
// 编辑的输入框不被回写（focus 守卫）。
import { computed, reactive, ref, watch } from 'vue';
import { ArrowDown, ArrowUp, Bell, Check, Monitor, Refresh } from '@element-plus/icons-vue';
import { store, detectNode, saveSettings } from '../store.js';
import { patchStore, refreshPatches, applyPatch, revertPatch } from '../patches.js';
import {
  notificationStore,
  refreshNotificationStatus,
  saveNotificationSettings,
  markNotificationsRead,
  sendTestNotification,
} from '../notifications.js';
import { globalBusy, isLoading, withLoading } from '../loading.js';

const port = ref(undefined);
const profile = ref('');
const editing = ref(false);
const nodeHint = ref('');

// 初始与外部变化时回写输入框；用户正在编辑（editing）时跳过，避免输入被回滚。
watch(
  () => store.view && store.view.settings,
  (settings) => {
    if (!settings || editing.value) return;
    port.value = settings.port;
    profile.value = settings.profile || '';
  },
  { immediate: true }
);

// 进入设置页时刷新补丁与通知状态（内核激活版本、事件流连接都可能已经变化）。
// 面板按 activePanel 作为 key 重新创建，所以「重新打开设置页」也会走到这里。
watch(
  () => store.activePanel,
  (panel) => {
    if (panel === 'settings') {
      refreshPatches();
      refreshNotificationStatus();
    }
  },
  { immediate: true }
);

const defaultNodeHint = computed(() => {
  const n = store.view && store.view.node;
  if (!n) return '';
  return n.ok ? 'node ' + n.version + ' 满足 dsh 要求（^22.19 || >=24）' : n.reason;
});

const hintText = computed(() => nodeHint.value || defaultNodeHint.value);

// 工作台运行期间禁止应用 / 撤销补丁（会写入内核目录）。
const workbenchRunning = computed(() => !!(store.view && store.view.kernel && store.view.kernel.running));

const patchRows = computed(() => (patchStore.view && patchStore.view.patches) || []);

// 已并入官方内核的补丁默认折叠。用户在卡片上点击「展开查看」后把 id 加进 Set 里；
// 切换内核或刷新时仍保持原折叠态（除非用户主动点回「收起」），避免误点展开影响阅读。
const obsoleteExpanded = reactive(new Set());
function toggleObsolete(id) {
  if (obsoleteExpanded.has(id)) obsoleteExpanded.delete(id);
  else obsoleteExpanded.add(id);
}
function isObsoleteCollapsed(row) {
  return Boolean(row.superseded) && !obsoleteExpanded.has(row.id);
}

function rangeText(row) {
  if (row.minKernelVersion && row.maxKernelVersion) {
    return 'v' + row.minKernelVersion + ' ~ v' + row.maxKernelVersion;
  }
  if (row.minKernelVersion) return 'v' + row.minKernelVersion + ' 及以上';
  if (row.maxKernelVersion) return 'v' + row.maxKernelVersion + ' 及以下';
  return '任意内核版本';
}

// 状态徽标配色：未应用 / 不适用 / 无内核 → info；已应用 → success；
// 文件未命中 → warning；文件被改动 → danger。
function stateTag(state) {
  if (state === 'applied') return 'success';
  if (state === 'partial') return 'warning';
  if (state === 'dirty') return 'danger';
  return 'info';
}

// 状态决定主操作：已应用 / 部分应用 / 文件被改动 → 撤销；其余 → 应用。
function primaryAction(row) {
  return row.state === 'applied' || row.state === 'partial' || row.state === 'dirty'
    ? 'revert'
    : 'apply';
}

function onDetectNode() {
  const info = detectNode();
  if (info) {
    info.then((result) => {
      if (result) {
        nodeHint.value = result.ok ? '检测结果：' + result.path + '  ' + result.version : result.reason;
      }
    });
  }
}

function onRefreshPatches() {
  withLoading('patchRefresh', () => refreshPatches());
}

// 角标画在哪：Rust 回报的平台字符串决定文案，未知平台退回通用说法。
const badgeTarget = computed(() => {
  if (notificationStore.platform === 'macos') return 'Dock 图标';
  if (notificationStore.platform === 'windows') return '任务栏图标';
  return '应用图标';
});

// 自检结果就地显示，不弹页内浮层：那个浮层会被误认成"通知就是它"。
const testNote = ref('');

async function onTestNotification() {
  testNote.value = '';
  const ok = await sendTestNotification();
  if (ok) {
    testNote.value =
      `已模拟一次任务完成：${badgeTarget.value}上的角标已更新，系统通知也已发出。` +
      '若没有看到通知气泡，请在系统的通知设置里确认 dsh-xlink 已被允许（下面若有环境说明，请先按它处理）。';
  }
}

function onSave() {
  saveSettings(port.value, profile.value);
}
</script>

<template>
  <section class="panel">
    <div class="card">
      <h2>设置</h2>
      <el-form label-width="140px" label-position="left" @focusin="editing = true" @focusout="editing = false">
        <el-form-item label="Web UI 端口">
          <el-input-number v-model="port" :min="1024" :max="65535" :precision="0" controls-position="right" />
        </el-form-item>
        <el-form-item label="插件接线 profile 名">
          <!-- 固定值，不允许修改：保存设置时原样回传当前配置（默认 web）。 -->
          <code class="profile-fixed">{{ profile || 'web' }}</code>
        </el-form-item>
      </el-form>
      <div class="btn-row">
        <el-button
          type="primary"
          :icon="Check"
          :loading="isLoading('saveSettings')"
          :disabled="globalBusy"
          @click="onSave"
        >
          保存设置
        </el-button>
        <el-button
          text
          :icon="Monitor"
          :loading="isLoading('detectNode')"
          :disabled="globalBusy"
          @click="onDetectNode"
        >
          检测 Node.js
        </el-button>
      </div>
      <p class="muted" style="margin: 0">{{ hintText }}</p>
    </div>

    <div class="card">
      <div class="card-head">
        <h2>任务通知</h2>
        <!-- 手动刷新：读取失败要说清下一步；进入设置页的自动刷新保持静默。 -->
        <el-button text size="small" :icon="Refresh" :loading="isLoading('notificationRefresh')"
          @click="refreshNotificationStatus(true)">
          刷新
        </el-button>
      </div>
      <p class="muted notify-section-hint">
        会话里的对话任务跑完后：图标右上角挂上未读数字角标（macOS 在 Dock、Windows 在任务栏），
        同时由系统弹一条通知气泡——通知来自操作系统，不是这个面板里的浮层。
      </p>
      <el-form label-width="200px" label-position="left">
        <el-form-item label="任务完成后通知我">
          <el-switch
            :model-value="notificationStore.enabled"
            :loading="isLoading('notificationSave')"
            @change="(value) => saveNotificationSettings({ enabled: value })"
          />
        </el-form-item>
        <el-form-item label="仅当工作台窗口不在前台时通知">
          <div class="notify-field">
            <el-switch
              :model-value="notificationStore.notifyAwayOnly"
              :disabled="!notificationStore.enabled"
              :loading="isLoading('notificationSave')"
              @change="(value) => saveNotificationSettings({ notifyAwayOnly: value })"
            />
            <span class="muted">
              前台指你正在看着工作台，这时完成的任务不打扰你；切走或窗口被遮挡时才提醒。
            </span>
          </div>
        </el-form-item>
        <el-form-item label="通知声音">
          <el-switch
            :model-value="notificationStore.sound"
            :disabled="!notificationStore.enabled"
            :loading="isLoading('notificationSave')"
            @change="(value) => saveNotificationSettings({ sound: value })"
          />
        </el-form-item>
      </el-form>
      <div class="btn-row">
        <template v-if="notificationStore.unread > 0">
          <span class="notify-unread">{{ notificationStore.unread }} 条未读</span>
          <el-button text size="small" :loading="isLoading('notificationMarkRead')"
            :disabled="globalBusy" @click="markNotificationsRead()">
            全部已读
          </el-button>
        </template>
        <el-button type="primary" size="small" :icon="Bell" :loading="isLoading('notificationTest')"
          :disabled="globalBusy" @click="onTestNotification">
          模拟一次任务完成
        </el-button>
        <span class="muted notify-test-hint">
          未读 +1、角标刷新、发一条系统通知；看完点「全部已读」即可清零。
        </span>
      </div>
      <p v-if="testNote" class="muted notify-hint">{{ testNote }}</p>
      <p v-if="!notificationStore.watching" class="muted notify-hint">
        尚未连接内核事件流：内核未运行或已断开，任务完成后不会提醒；启动工作台后点上方「刷新」重试。
      </p>
      <!-- 环境限制（不是错误）：例如 macOS 上未打包的 dev 构建无法投递系统通知，
           角标仍然正常。用灰字而不是警告色，避免把平台约束说成故障。 -->
      <p v-if="notificationStore.environmentNote" class="muted notify-hint">
        {{ notificationStore.environmentNote }}
      </p>
      <el-alert
        v-if="notificationStore.lastError"
        :title="notificationStore.lastError"
        type="warning"
        :closable="false"
        show-icon
      />
    </div>

    <div class="card">
      <div class="card-head">
        <h2>内置补丁</h2>
        <el-button text size="small" :icon="Refresh" :loading="isLoading('patchRefresh')"
          @click="onRefreshPatches">
          刷新
        </el-button>
      </div>
      <p class="muted patch-section-hint">
        随 dsh-xlink 内置，默认不生效；应用前自动备份，可随时撤销。
      </p>
      <el-alert
        v-if="patchStore.view && patchStore.view.warning"
        :title="patchStore.view.warning"
        type="warning"
        :closable="false"
        show-icon
      />
      <div v-if="!patchStore.loaded" class="patch-empty">补丁状态加载中…</div>
      <div v-else-if="!patchRows.length" class="patch-empty">此版本未携带任何内置补丁。</div>
      <div v-else class="patch-list">
        <div v-for="row in patchRows" :key="row.id" class="patch-item"
             :class="{ 'patch-item-obsolete': row.superseded, 'patch-item-collapsed': isObsoleteCollapsed(row) }">
          <div class="patch-item-main">
            <div class="patch-item-title">
              <strong :class="{ 'patch-name-obsolete': row.superseded }">{{ row.name }}</strong>
              <el-tag v-if="row.kind === 'plugin'" size="small" type="success" effect="plain">插件</el-tag>
              <el-tag v-else size="small" type="primary" effect="plain">补丁</el-tag>
              <el-tag size="small" effect="plain">v{{ row.version }}</el-tag>
              <el-tag v-if="row.superseded && row.supersededSinceKernelVersion"
                       size="small" effect="plain" class="patch-superseded-tag">
                已并入 v{{ row.supersededSinceKernelVersion }}
              </el-tag>
              <el-tag :type="stateTag(row.state)" size="small" effect="dark" class="patch-state">
                {{ row.stateText }}
              </el-tag>
              <el-button v-if="row.superseded" link size="small" class="patch-toggle"
                          @click="toggleObsolete(row.id)">
                <el-icon><component :is="isObsoleteCollapsed(row) ? ArrowDown : ArrowUp" /></el-icon>
                {{ isObsoleteCollapsed(row) ? '详情' : '收起' }}
              </el-button>
            </div>
            <p v-if="isObsoleteCollapsed(row) && row.supersededSinceKernelVersion"
               class="muted patch-desc patch-obsolete-summary">
              v{{ row.supersededSinceKernelVersion }} 起已合并，无需手动应用。
            </p>
            <template v-else>
              <p class="muted patch-desc">{{ row.description }}</p>
              <p class="patch-meta">
                <span>适用：{{ rangeText(row) }}</span>
                <span v-if="row.appliedAt">已应用：{{ row.appliedAt }}</span>
              </p>
              <p v-if="row.note" class="patch-note">{{ row.note }}</p>
            </template>
          </div>
          <div v-if="!isObsoleteCollapsed(row)" class="patch-item-actions">
            <el-button
              v-if="primaryAction(row) === 'apply'"
              type="primary"
              size="small"
              :disabled="!row.enabled || workbenchRunning"
              :loading="isLoading('patchApply:' + row.id)"
              @click="applyPatch(row.id, row.name)"
            >
              应用
            </el-button>
            <el-button
              v-else
              type="danger"
              plain
              size="small"
              :disabled="!row.enabled || workbenchRunning"
              :loading="isLoading('patchRevert:' + row.id)"
              @click="revertPatch(row.id, row.name)"
            >
              撤销
            </el-button>
          </div>
        </div>
      </div>
      <p v-if="workbenchRunning" class="patch-note">
        工作台运行期间不能应用或撤销补丁，请先关闭工作台后再操作。
      </p>
    </div>
  </section>
</template>