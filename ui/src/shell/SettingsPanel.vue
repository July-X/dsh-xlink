<script setup>
// 设置：Web UI 端口、任务完成通知、可选的数据迁移入口。套餐用量查询不设卡：
// 概览页的「套餐用量」卡已提供数据展示 / 刷新 / 测试入口，凭据本身在工作台
// 模型设置里维护。内置补丁（内核补丁 / 小插件）入口已隐藏——最新内核已包含
// 相关修复，不再需要从设置页应用；后端 `patch_status` / `patch_apply` /
// `patch_revert` 与 `ui/src/patches.js` 仍保留，便于旧内核撤销。
// 卡片顺序：设置 → 任务通知 → 数据迁移（默认折叠）→ 环境回退点 → 深入排查。
// 安全网这两张卡原本挂在概览页，但它们是**故障时才用得上的兜底**，不是日常
// 要看的东西：概览页是开机第一屏，在那儿常年摆着两个「一切正常」的空态卡
// 既占地方又稀释真正要看的内容（当前内核、用量、额度）。挪到设置页后，
// 概览保持干净，而出事时它们与「数据迁移」这条同属"环境出问题时才动"的
// 入口并排摆在一起，上下文也对得上。
import { computed, onMounted, ref, watch } from 'vue';
import { ArrowDown, ArrowUp, Bell, Check, Headset, QuestionFilled } from '@element-plus/icons-vue';
import { store, saveSettings } from '../store.js';
import { migrationStore, loadMigrationHistory } from '../migration/migration.js';
import SnapshotCard from '../diagnostics/SnapshotCard.vue';
import BisectPanel from '../diagnostics/BisectPanel.vue';
import {
  notificationStore,
  refreshNotificationStatus,
  saveNotificationSettings,
  markNotificationsRead,
  sendTestNotification,
  testNotificationSound,
  formatNotifyTime,
  formatNotifyDuration,
} from '../incidents/notifications.js';
import { globalBusy, isLoading } from './loading.js';
import {
  autostartStore,
  refreshAutostartStatus,
  setAutostartEnabled,
  setAutostartKernel,
} from './autostart.js';

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

function onSave() {
  saveSettings(port.value, profile.value);
}

// 进入设置页时刷新通知与自启状态（事件流连接可能已经变化，登录项也可能被
// 用户在系统设置里改过）。
watch(
  () => store.activePanel,
  (panel) => {
    if (panel === 'settings') {
      refreshNotificationStatus();
      refreshAutostartStatus();
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
    <div class="page-head">
      <div>
        <h1 class="page-title">设置</h1>
        <p class="page-desc">工作台端口、后台常驻与开机自启，以及模型任务的完成通知。</p>
      </div>
    </div>
    <!-- 宽版（1040）下单列会让每一张卡右边空掉半屏，所以按设计稿分两列：
         左列是「这台壳本身怎么跑」（工作台端口、后台常驻、自启），
         右列是「对内对外的表现」（任务通知、迁移入口、安全网）。两列各自
         内部竖着叠，用的是全局的 .page-layout / .page-layout__col 原语。 -->
    <div class="page-layout">
      <div class="page-layout__col">
    <div class="card">
      <h2 class="card-title-with-tip">
        工作台
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
            <!-- 文案是「保存配置」而不是「保存设置」（设计说明 §5）：它保存的是
                 **这一栏** —— 工作台 Web UI 端口 + profile 名。叫「保存设置」会
                 让人以为它管的是整个设置页，而下面那些开关是各自即时生效的。 -->
            <el-button
              type="primary"
              :icon="Check"
              :loading="isLoading('saveSettings')"
              :disabled="globalBusy"
              @click="onSave"
            >
              保存配置
            </el-button>
          </div>
        </el-form-item>
      </el-form>
    </div>

    <div class="card">
      <h2 class="card-title-with-tip">
        后台常驻
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              关闭窗口只是把它收进后台，内核与工作台继续运行。重新打开与退出都在菜单栏
              （macOS）/ 托盘（Windows）图标的右键菜单里，退出前若内核在跑会先问你一句。
              <br />
              「开机自启动」只把程序拉进后台、不显示面板。内核默认不随之启动；打开「开机启动工作台」
              才会开机即起，占着端口并订阅事件流。内核始终随程序一起退出。
            </div>
          </template>
          <el-icon class="card-info-icon"><QuestionFilled /></el-icon>
        </el-tooltip>
      </h2>
      <el-form class="notify-form" label-width="152px" label-position="left">
        <el-form-item label="关窗后留在后台">
          <el-switch :model-value="true" disabled />
        </el-form-item>
        <el-form-item label="开机自启动">
          <el-switch
            :model-value="autostartStore.enabled"
            :loading="isLoading('autostartSet')"
            @change="(value) => setAutostartEnabled(value)"
          />
        </el-form-item>
        <el-form-item label="开机启动工作台">
          <el-switch
            :model-value="autostartStore.kernel"
            :loading="isLoading('autostartSetKernel')"
            @change="(value) => setAutostartKernel(value)"
          />
        </el-form-item>
      </el-form>
      <el-alert
        v-if="autostartStore.note"
        :title="autostartStore.note"
        type="warning"
        :closable="false"
        show-icon
      />
    </div>
      </div>

      <div class="page-layout__col">
    <div class="card">
      <h2 class="card-title-with-tip">
        任务通知
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              任务完成后挂未读角标并发送系统通知气泡；通知与下方「最近完成」列表
              会带会话的最近一轮对话（问 / 答）与完成时间。
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

      <!-- 最近完成记录：每条带完成时刻与最近一轮对话（提问 + 回复预览）。
           系统通知气泡里是同一段文案；这里让用户回来也能翻到。 -->
      <ul v-if="notificationStore.items.length" class="notify-items">
        <li
          v-for="item in notificationStore.items"
          :key="item.sessionId + '-' + item.finishedAtMs"
          class="notify-item"
        >
          <div class="notify-item-head">
            <span class="notify-item-title" :title="item.cwd">{{ item.title }}</span>
            <span class="notify-item-time">
              {{ formatNotifyTime(item.finishedAtMs) || '时间未知'
                }}<template v-if="formatNotifyDuration(item.durationMs)">
                · 用时 {{ formatNotifyDuration(item.durationMs) }}</template>
            </span>
          </div>
          <p v-if="item.lastPrompt" class="notify-item-line" :title="item.lastPrompt">
            <span class="notify-item-role">问</span>{{ item.lastPrompt }}
          </p>
          <p v-if="item.lastResponse" class="notify-item-line" :title="item.lastResponse">
            <span class="notify-item-role">答</span>{{ item.lastResponse }}
          </p>
        </li>
      </ul>
      <p v-else class="muted notify-hint">
        还没有完成记录；任务完成后这里会显示会话名、最近一次对话与完成时间。
      </p>
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

    <!-- 环境回退点（安全网 P0/P1）。「数据迁移」是"环境出问题时才动"的入口，
         回退点是同一条链的下半段：先回到曾经良好的状态，回退解决不了才轮到
         下面的「深入排查」。两张卡挨着摆，顺序即救生顺序。 -->
    <SnapshotCard />

    <BisectPanel />
      </div>
    </div>
  </section>
</template>

<style scoped>
</style>
