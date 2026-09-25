<script setup>
// 设置：Web UI 端口、任务完成通知、可选的数据迁移入口。内置补丁（内核补丁 / 小插件）
// 入口已隐藏——最新内核已包含相关修复，不再需要从设置页应用；后端 `patch_status` /
// `patch_apply` / `patch_revert` 与 `ui/src/patches.js` 仍保留，便于旧内核撤销。
// 卡片顺序：设置 → 任务通知 → 数据迁移（默认折叠）。迁移是旧版兼容路径，启动期
// 一次性弹窗（MigrationPrompt）才是首次迁移的主入口，日常用不到。
import { computed, onMounted, ref, watch } from 'vue';
import { ArrowDown, ArrowUp, Bell, Check, Headset, QuestionFilled } from '@element-plus/icons-vue';
import { store, saveSettings } from '../store.js';
import { migrationStore, loadMigrationHistory } from '../migration.js';
import {
  notificationStore,
  refreshNotificationStatus,
  saveNotificationSettings,
  markNotificationsRead,
  sendTestNotification,
  testNotificationSound,
} from '../notifications.js';
import { globalBusy, isLoading } from '../loading.js';

const port = ref(undefined);
// 固定值（默认 web）：保存时仍要原样回传，否则 Rust 侧的合并会把 profile 覆盖。
const profile = ref('');
const editing = ref(false);

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

// 进入设置页时刷新通知状态（事件流连接可能已经变化）。
watch(
  () => store.activePanel,
  (panel) => {
    if (panel === 'settings') {
      refreshNotificationStatus();
    }
  },
  { immediate: true }
);

// 角标画在哪：Rust 回报的平台字符串决定文案。
const badgeTarget = computed(() => {
  if (notificationStore.platform === 'macos') return 'Dock 图标';
  if (notificationStore.platform === 'windows') return '任务栏图标';
  return '应用图标';
});

// dev 专用自检入口（`store.devUi`）：release 预览打开后这个字段也是 false。
const isDevBuild = computed(() => store.devUi);
const testNote = ref('');

async function onTestNotification() {
  testNote.value = '';
  const ok = await sendTestNotification();
  if (ok) {
    testNote.value =
      `已模拟一次任务完成：${badgeTarget.value}上的角标已更新，系统通知也已发出。` +
      '没看到气泡时，请在系统通知设置里允许 dsh-xlink（下面若有环境说明，先按它处理）。';
  }
}

function onSave() {
  saveSettings(port.value, profile.value);
}

// 迁移过 = 历史非空，未迁移 = 历史为空。
const migratedBefore = computed(() => migrationStore.history.length > 0);
// 迁移卡片默认收起——首次进设置页不该被「去迁移」CTA 抢戏（启动期一次性弹窗才是
// 主入口），日常也用不到。展开态留在组件实例里，跨切页会重置。
const migrationExpanded = ref(false);
onMounted(() => {
  loadMigrationHistory();
});
</script>

<template>
  <section class="panel">
    <div class="card">
      <h2 class="card-title-with-tip">
        设置
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              profile 名固定为 web，随端口一起保存；Node 环境与「重新检测」在概览页。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form class="settings-form" label-width="100px" label-position="left" @focusin="editing = true" @focusout="editing = false">
        <el-form-item label="Web UI 端口">
          <div class="btn-row">
            <el-input-number
              v-model="port"
              class="settings-port"
              :min="1024"
              :max="65535"
              :precision="0"
              controls-position="right"
            />
            <el-button
              type="primary"
              :icon="Check"
              :loading="isLoading('saveSettings')"
              :disabled="globalBusy"
              @click="onSave"
            >
              保存设置
            </el-button>
          </div>
        </el-form-item>
      </el-form>
    </div>

    <div class="card">
      <h2 class="card-title-with-tip">
        任务通知
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              任务完成后挂未读角标并发送系统通知气泡。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form class="notify-form" label-width="152px" label-position="left">
        <el-form-item label="任务完成后通知我">
          <el-switch
            :model-value="notificationStore.enabled"
            :loading="isLoading('notificationSave')"
            @change="(value) => saveNotificationSettings({ enabled: value })"
          />
        </el-form-item>
        <el-form-item label="工作台不在前台才通知">
          <div class="notify-inline">
            <el-switch
              :model-value="notificationStore.notifyAwayOnly"
              :disabled="!notificationStore.enabled"
              :loading="isLoading('notificationSave')"
              @change="(value) => saveNotificationSettings({ notifyAwayOnly: value })"
            />
            <span class="muted">切走或窗口被遮挡时才提醒</span>
          </div>
        </el-form-item>
        <el-form-item label="通知声音">
          <div class="notify-inline">
            <el-switch
              :model-value="notificationStore.sound"
              :disabled="!notificationStore.enabled"
              :loading="isLoading('notificationSave')"
              @change="(value) => saveNotificationSettings({ sound: value })"
            />
            <!-- 声音关着时不试听——否则开关与听到的结果自相矛盾。 -->
            <el-button
              text
              size="small"
              :icon="Headset"
              :loading="isLoading('notificationSoundTest')"
              :disabled="!notificationStore.enabled || !notificationStore.sound || globalBusy"
              @click="testNotificationSound()"
            >
              试听
            </el-button>
          </div>
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
        <template v-if="isDevBuild">
          <el-button type="primary" size="small" :icon="Bell" :loading="isLoading('notificationTest')"
            :disabled="globalBusy" @click="onTestNotification">
            模拟一次任务完成
          </el-button>
          <span class="muted notify-test-hint">dev 专用。</span>
        </template>
      </div>
      <p v-if="testNote" class="muted notify-hint">{{ testNote }}</p>
      <p v-if="!notificationStore.watching" class="muted notify-hint">
        尚未连接内核事件流：内核未运行或已断开，任务完成后不会提醒。
      </p>
      <p v-if="notificationStore.notificationsBlocked && notificationStore.environmentNote" class="muted notify-hint">
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

    <div class="card" :class="{ 'card-collapsed': !migrationExpanded }">
      <button
        type="button"
        class="card-head card-head-toggle"
        :aria-expanded="migrationExpanded"
        @click="migrationExpanded = !migrationExpanded"
      >
        <h2>数据迁移</h2>
        <span class="migration-status">{{ migratedBefore ? '已迁移' : '尚未迁移' }}</span>
        <el-icon class="migration-toggle-icon">
          <component :is="migrationExpanded ? ArrowUp : ArrowDown" />
        </el-icon>
      </button>
      <template v-if="migrationExpanded">
        <p class="muted" style="margin: 0">
          {{
            migratedBefore
              ? '已迁移过；可进入迁移页查看历史、重新运行或回滚。'
              : '把旧版 dsh-xlink 的插件 / 技能导入多实例布局；旧源不会被删除，可随时回滚。'
          }}
        </p>
        <el-button
          :type="migratedBefore ? 'default' : 'primary'"
          @click="store.activePanel = 'migration'"
        >
          {{ migratedBefore ? '查看数据迁移' : '去迁移' }}
        </el-button>
      </template>
    </div>
  </section>
</template>
