<script setup>
// 设置：Web UI 端口、任务完成通知，以及一张「环境回退与诊断」。
// 套餐用量查询不设卡：概览页的「套餐用量」卡已提供数据展示 / 刷新 / 测试入口，
// 凭据本身在工作台模型设置里维护。内置补丁（内核补丁 / 小插件）入口已隐藏——最新
// 内核已包含相关修复，不再需要从设置页应用；后端 `patch_status` / `patch_apply` /
// `patch_revert` 与 `ui/src/patches.js` 仍保留，便于旧内核撤销。
//
// 卡片顺序：设置 → 任务通知 → 环境回退与诊断。
// 后一张是**故障时才用得上的兜底**，不是日常要看的东西：概览页是开机第一屏，
// 在那儿常年摆着两个「一切正常」的空态卡既占地方又稀释真正要看的内容（当前内核、
// 用量、额度）。挪到设置页后概览保持干净，而「数据迁移 / 环境回退点 / 深入排查 /
// 启动诊断」四条同属"环境出问题时才动"，收在一张卡里上下文也对得上。
import { computed, onMounted, ref, watch } from 'vue';
import { Bell, Check, Headset, QuestionFilled, Tickets } from '@element-plus/icons-vue';
import { store, saveSettings } from '../store.js';
import { migrationStore, loadMigrationHistory } from '../migration/migration.js';
import { openStartupDiagnosis } from '../diagnostics/diagnostics.js';
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
// 迁移历史：设置页这一行要显示「已迁移 / 尚未迁移」，扫历史是唯一的判据
// （migration_list 非空即迁移过）——没有别的信号可靠，hasMigratable 只说
// 「扫得到」，迁移过的用户同样扫得到。
onMounted(() => {
  loadMigrationHistory();
});

/**
 * 「环境回退与诊断」那一行去看最近一次启动过程。
 *
 * 与概览页同一套写法：id 交给 `openStartupDiagnosis` 去问后端「最近一条」，
 * 这里不塞任何具体 id——塞进去会让用户点进来看到上一次启动的记录，而按钮上
 * 写的是「查看」。
 *
 * 不挂 `isLoading`：`reloadStartupDiagnosis(runId, false)` 那次拉取是
 * **非手动**的，不经过 `withLoading`，挂上去就是一个永远不转的 loading。
 */
function openStartupRun() {
  openStartupDiagnosis(store.lastIncident?.runId || '', 'settings');
}
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
         左列是「这台壳本身怎么跑」（工作台端口、后台常驻、自启、环境回退与诊断），
         右列是「对内对外的表现」（任务通知）。
         用的是全局的 .page-layout / .page-layout__col 原语。

         「环境回退与诊断」原先在**右**栏，2026-10-08 用户指定移到左栏、接在
         「后台常驻」下方。这次移动的账要记清楚：那张卡里四行全是
         **出问题时才动**的动作，而右栏那张「任务通知」在连不上内核事件流时会
         展开一大段环境说明 + 告警（notificationStore.environmentNote），
         两张叠在一起时右栏被拉到近两屏高、左栏却空一大截——用户是来排查问题的，
         视线先撞上的却是通知告警，而不是他正要找的排查入口。
         顺带一提，这张卡先前已被收成一张（原为右栏四张：任务通知 / 数据迁移 /
         环境回退点 / 深入排查），那一次为的是配平两栏；这次把**位置**也定下来：
         左栏三张（工作台 / 后台常驻 / 环境回退与诊断）、右栏一张（任务通知）。
         **入口一个没少**：恢复 / 开始排查 / 停止仍留在自己那一行的动作区。 -->
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

    <!-- 「数据迁移 / 环境回退点 / 深入排查」三张并排的大卡收成一张
         （设计稿 draft 2844：「环境回退与诊断」，caption「出问题时使用」）。
         原先右栏四张、左栏两张，右栏被拉到两屏高而左栏空一大截——四者同属
         「环境出问题时才动」这一组，分成四张卡只是把同一个上下文摊成四段。
         四行各自是一个 `page-list-row`：左标题 + 状态说明，右动作。
         **入口一个没少**：恢复 / 开始排查 / 停止都留在自己那一行的动作区，
         「查看」展开的是明细；启动诊断与数据迁移本来就是跳转。
         卡片顺序：设置 → 任务通知 → 环境回退与诊断。 -->
    <div class="card">
      <div class="card-head">
        <h2>环境回退与诊断</h2>
        <span class="head-meta"><span class="muted">出问题时使用</span></span>
      </div>
      <div class="page-list">
        <!-- 前两行自带 Fragment：行是常驻的，明细与告警是它们的兄弟节点。 -->
        <SnapshotCard />
        <BisectPanel />

        <div class="page-list-row">
          <div class="page-list-main">
            <h3 class="page-list-title">启动诊断</h3>
            <p class="page-list-meta">这次启动停在哪一步，以及接下来能做什么。</p>
          </div>
          <div class="page-list-actions">
            <el-button
              round
              size="small"
              :icon="Tickets"
              title="查看最近一次启动过程"
              @click="openStartupRun"
            >
              查看
            </el-button>
          </div>
        </div>

        <!-- 「数据迁移」在设置页是**一行**，不是一张独立大卡。它的主入口是
             **侧栏菜单**（设计稿 2550 行，常驻），这里留的是设计说明 §侧栏
             要求的「迁移完成后设置页仍保留进入入口」。
             整个功能在 v0.6.0 之后移除，届时这一行与侧栏菜单一并删
             （见 ui/AGENTS.md 的「待移除」一节）。 -->
        <div class="page-list-row">
          <div class="page-list-main">
            <h3 class="page-list-title">数据迁移</h3>
            <p class="page-list-meta">{{ migratedBefore ? '已迁移' : '尚未迁移' }}</p>
          </div>
          <div class="page-list-actions">
            <el-button
              round
              size="small"
              :type="migratedBefore ? 'default' : 'primary'"
              @click="store.activePanel = 'migration'"
            >
              {{ migratedBefore ? '查看数据迁移' : '去迁移' }}
            </el-button>
          </div>
        </div>
      </div>
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

      </div>
    </div>
  </section>
</template>

<style scoped>
</style>
