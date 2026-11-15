<script setup>
// Hook 页：收件箱 / 接入点 / 设置。接入点的令牌只在创建与轮换时显示一次；
// 发送方地址不含令牌。接收服务默认关闭，只绑回环地址。
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import PageHeadIcon from '../shell/PageHeadIcon.vue';
import { InfoFilled, Plus, Refresh } from '@element-plus/icons-vue';
import { isLoading } from '../shell/loading.js';
import { confirmDialog, toast } from '../shell/notify.js';
import { formatBytes } from '../shell/labels.js';
import {
  hookStore,
  refreshHooksStatus,
  loadEndpoints,
  loadInbox,
  createEndpoint,
  setEndpointEnabled,
  rotateEndpointToken,
  rotateNotificationToken,
  clearCreatedEndpoint,
  clearNotificationCredential,
  notificationUrl,
  saveHooksConfig,
  copyText,
  endpointAddress,
  hookTime,
  kindLabel,
} from './hooks.js';
import HookInbox from './HookInbox.vue';

const TAB_ORDER = ['inbox', 'endpoints', 'settings'];
const activeTab = ref('inbox');

const status = computed(() => hookStore.status);
const endpoints = computed(() => hookStore.endpoints);
const created = computed(() => hookStore.created);
const createdAddress = computed(() => (created.value ? endpointAddress(created.value.endpoint) : ''));
const notification = computed(() => hookStore.notification);
const notificationAddress = computed(() =>
  notification.value ? notificationUrl(notification.value.endpoint) : ''
);
const remaining = computed(() => {
  const view = status.value;
  if (!view) return 0;
  return Math.max(0, (view.storage_budget_bytes || 0) - (view.storage_bytes || 0));
});
const endpointCount = computed(() => status.value?.endpoint_count ?? endpoints.value.length);

// 页头说明收进 ⓘ（Kami §3.1）：已入箱只表示内容完整入库，不代表已审核或安全。
const PAGE_DESC = '收下局域网发送方推来的事件与附件；「已入箱」只表示内容完整入库，不代表已审核或安全。接收端不执行内容，也不代发送方脱敏。存储超过 5 GiB 时会自动清理最早的历史事件。';

function selectTab(tab) {
  if (!TAB_ORDER.includes(tab)) return;
  activeTab.value = tab;
  if (tab === 'inbox') loadInbox();
  else if (tab === 'endpoints') loadEndpoints();
  else refreshHooksStatus();
}

function onTabKeydown(event) {
  const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
  if (!step) return;
  const buttons = Array.from(event.currentTarget.querySelectorAll('[role="tab"]'));
  const index = buttons.indexOf(event.target);
  if (index < 0) return;
  event.preventDefault();
  const next = (index + step + buttons.length) % buttons.length;
  selectTab(TAB_ORDER[next]);
  buttons[next].focus();
}

// --- 新建接入点 ---------------------------------------------------------
const createVisible = ref(false);
const draftName = ref('');
const allowText = ref(true);
const allowParameters = ref(true);

function openCreate() {
  draftName.value = '';
  allowText.value = true;
  allowParameters.value = true;
  createVisible.value = true;
}

async function submitCreate() {
  const name = draftName.value.trim();
  const kinds = [];
  if (allowText.value) kinds.push('text');
  if (allowParameters.value) kinds.push('parameters');
  if (!name) {
    toast('请先填写接入点名称', 4000, 'warning');
    return;
  }
  if (!kinds.length) {
    toast('至少允许一种内容类型', 4000, 'warning');
    return;
  }
  try {
    await createEndpoint(name, kinds);
    createVisible.value = false;
  } catch {
    // store 已 toast；对话框保留以便修正。
  }
}

function closeCreated() {
  clearCreatedEndpoint();
}

// --- 接入点动作 ---------------------------------------------------------
async function toggleEndpoint(row, enabled) {
  try {
    await setEndpointEnabled(row.id, enabled);
  } catch {
    // store 已 toast，列表保持原状态。
  }
}

async function rotateToken(row) {
  const ok = await confirmDialog(
    '轮换接入点令牌？',
    '接入点「' + row.name + '」的旧令牌会立即失效，正在上传的请求会失败；已收到的事件不受影响。' +
      '新令牌只显示一次。',
    '轮换令牌'
  );
  if (!ok) return;
  try {
    await rotateEndpointToken(row.id);
  } catch {
    // store 已 toast。
  }
}

async function removeEndpoint(row) {
  const ok = await confirmDialog(
    '删除接入点？',
    '删除「' + row.name + '」后旧令牌立即失效，发送方会收到未授权；已收到的事件仍保留在收件箱。此操作不可恢复。',
    '删除接入点'
  );
  if (!ok) return;
  try {
    await deleteEndpoint(row.id);
  } catch {
    // store 已 toast。
  }
}

function copyAddress(row) {
  const address = endpointAddress(row);
  if (address) copyText(address, '发送地址');
}

function copyToken() {
  if (created.value) copyText(created.value.token, '令牌');
}

function closeNotification() {
  clearNotificationCredential();
}

async function rotateNotification(row) {
  if (row.notification_enabled) {
    const ok = await confirmDialog(
      '轮换 Jenkins 通知凭据？',
      '旧的 Basic 密码会立即失效，Jenkins 里保存的 Secret text 需要同步更新；已入箱的事件不受影响。' +
        '新密码只显示一次。',
      '轮换凭据'
    );
    if (!ok) return;
  }
  try {
    await rotateNotificationToken(row.id);
  } catch {
    // store 已 toast。
  }
}

function copyNotificationPassword() {
  if (notification.value) copyText(notification.value.token, 'Basic 密码');
}

// --- 设置 ---------------------------------------------------------------
const portDraft = ref(null);

watch(
  () => status.value?.config?.port,
  (port) => {
    if (port != null) portDraft.value = port;
  },
  { immediate: true }
);

async function toggleService(enabled) {
  const port = portDraft.value || status.value?.config?.port;
  await saveHooksConfig(enabled, port);
}

async function savePort() {
  const port = Number(portDraft.value);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    toast('端口需在 1024–65535 之间', 4000, 'warning');
    return;
  }
  await saveHooksConfig(!!status.value?.config?.enabled, port);
}

onMounted(() => {
  refreshHooksStatus();
  loadEndpoints();
  loadInbox();
});

// 离开页面即丢掉还在内存里的一次性令牌：它们只该在那一个对话框里存在。
onUnmounted(() => {
  clearCreatedEndpoint();
  clearNotificationCredential();
});
</script>

<template>
  <section class="panel">
    <div class="page-head">
      <div class="page-head__text">
        <PageHeadIcon :page="'hooks'" />
        <h1 class="page-title">Hook</h1>
        <el-tooltip placement="bottom-start" :show-after="80" :content="PAGE_DESC">
          <el-icon class="head-tip-icon" tabindex="0" role="note" :aria-label="PAGE_DESC"><InfoFilled /></el-icon>
        </el-tooltip>
      </div>
    </div>

    <div class="hook-tabs" role="tablist" aria-label="Hook 页签" @keydown="onTabKeydown($event)">
      <button
        type="button"
        class="plugin-tab"
        role="tab"
        :class="{ 'is-active': activeTab === 'inbox' }"
        :aria-selected="activeTab === 'inbox'"
        @click="selectTab('inbox')"
      >收件箱</button>
      <button
        type="button"
        class="plugin-tab"
        role="tab"
        :class="{ 'is-active': activeTab === 'endpoints' }"
        :aria-selected="activeTab === 'endpoints'"
        @click="selectTab('endpoints')"
      >接入点（{{ endpointCount }}）</button>
      <button
        type="button"
        class="plugin-tab"
        role="tab"
        :class="{ 'is-active': activeTab === 'settings' }"
        :aria-selected="activeTab === 'settings'"
        @click="selectTab('settings')"
      >设置</button>
    </div>

    <HookInbox v-if="activeTab === 'inbox'" @open-settings="selectTab('settings')" />

    <div v-else-if="activeTab === 'endpoints'" class="hook-pane">
      <div class="card">
        <div class="card-head">
          <h2>接入点</h2>
          <span class="head-meta">
            <el-button text :icon="Refresh" :loading="isLoading('hooksEndpointsRefresh')" @click="loadEndpoints">刷新</el-button>
            <el-button type="primary" :icon="Plus" @click="openCreate">新建接入点</el-button>
          </span>
        </div>
        <el-alert v-if="hookStore.endpointsError" type="error" :closable="false" :title="hookStore.endpointsError" />
        <el-alert v-if="status?.last_error" type="warning" :closable="false" :title="status.last_error" />
        <el-empty v-if="!endpoints.length" description="还没有接入点。新建一个并把令牌交给发送方，就能开始收事件。" />
        <div v-else class="page-list">
          <div v-for="row in endpoints" :key="row.id" class="page-row hook-endpoint">
            <div class="page-row__main">
              <div class="hook-endpoint__title">
                <span>{{ row.name }}</span>
                <el-tag v-if="!row.enabled" size="small" type="info">已暂停</el-tag>
                <el-tag v-else size="small" type="success">启用中</el-tag>
              </div>
              <div class="page-row__meta">
                允许：{{ row.allowed_kinds.map(kindLabel).join(' / ') }} ·
                最近接收：{{ row.last_event_at ? hookTime(row.last_event_at) : '还没有' }} ·
                占用：{{ formatBytes(row.bytes) }} · 错误 {{ row.error_count }} 次 ·
                Jenkins 通知：{{ row.notification_enabled ? '凭据已生成' : '未生成' }}
              </div>
              <div class="hook-endpoint__address">
                <span class="hook-mono" :title="endpointAddress(row)">{{ endpointAddress(row) }}</span>
                <el-button text @click="copyAddress(row)">复制地址</el-button>
              </div>
            </div>
            <div class="hook-endpoint__actions">
              <el-switch
                size="small"
                :model-value="row.enabled"
                :loading="isLoading('hooksEndpoint:' + row.id)"
                :aria-label="'启用接入点 ' + row.name"
                @change="toggleEndpoint(row, $event)"
              />
              <el-button text :loading="isLoading('hooksRotate:' + row.id)" @click="rotateToken(row)">轮换令牌</el-button>
              <el-button
                text
                :loading="isLoading('hooksNotification:' + row.id)"
                @click="rotateNotification(row)"
              >{{ row.notification_enabled ? '轮换通知凭据' : '生成通知凭据' }}</el-button>
              <el-button text type="danger" :loading="isLoading('hooksDeleteEndpoint:' + row.id)" @click="removeEndpoint(row)">删除</el-button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div v-else class="hook-pane">
      <div class="card">
        <div class="card-head">
          <h2>接收服务</h2>
          <el-tag size="small" :type="status?.running ? 'success' : 'info'">{{ status?.running ? '运行中' : '已停止' }}</el-tag>
        </div>
        <div class="hook-setting-row">
          <span>启用接收服务</span>
          <el-switch
            :model-value="!!status?.config?.enabled"
            :disabled="!status"
            :loading="isLoading('hooksSaveConfig')"
            aria-label="启用 Hook 接收服务"
            @change="toggleService"
          />
        </div>
        <div class="hook-setting-row">
          <span>监听端口</span>
          <el-input-number v-model="portDraft" :min="1024" :max="65535" :disabled="isLoading('hooksSaveConfig')" controls-position="right" />
          <el-button :disabled="!status" :loading="isLoading('hooksSaveConfig')" @click="savePort">保存</el-button>
        </div>
        <p class="hook-note">
          只监听 127.0.0.1；局域网接入需在本机加一层 HTTPS 反向代理，代理必须把上游 Host 改写为
          127.0.0.1:{{ status?.config?.port || '端口' }}（见 docs/features/hooks/lan-deployment.md）。
          应用本身不直接暴露到局域网，也不关闭系统防火墙。
        </p>
        <el-alert v-if="status?.last_error" type="error" :closable="false" :title="status.last_error" />
      </div>

      <div class="card">
        <div class="card-head">
          <h2>容量与清理</h2>
        </div>
        <dl class="hook-stats">
          <div><dt>已用</dt><dd>{{ formatBytes(status?.storage_bytes) }}</dd></div>
          <div><dt>预算</dt><dd>{{ formatBytes(status?.storage_budget_bytes) }}</dd></div>
          <div><dt>可用</dt><dd>{{ formatBytes(remaining) }}</dd></div>
          <div><dt>事件</dt><dd>{{ status?.event_count || 0 }}</dd></div>
          <div><dt>接入点</dt><dd>{{ endpointCount }}</dd></div>
        </dl>
        <p class="hook-note">
          已入箱内容不按时间删除；全局预算 5 GiB，超过时自动清理最早的历史事件（正文与附件删除，
          墓碑保留 30 天）。接收中事件 24 小时未完成即过期。清理由后台每 10 分钟执行；
          只有在没有可清理历史事件时，新事件才会被拒绝。手动永久删除在事件详情里，删除范围会在确认前列出。
        </p>
      </div>
    </div>

    <!-- 新建接入点：不展示地址与令牌，创建成功后再一次性显示。 -->
    <el-dialog v-model="createVisible" title="新建接入点" width="440px">
      <el-form label-width="84px">
        <el-form-item label="名称">
          <el-input v-model="draftName" maxlength="64" placeholder="例如：Jenkins 构建" />
        </el-form-item>
        <el-form-item label="允许内容">
          <el-checkbox v-model="allowText">文本</el-checkbox>
          <el-checkbox v-model="allowParameters">结构化参数</el-checkbox>
        </el-form-item>
      </el-form>
      <p class="hook-note">附件（图片 / 日志）跟随所属事件上传，不受这两项限制。</p>
      <template #footer>
        <div class="dialog-actions">
          <el-button @click="createVisible = false">取消</el-button>
          <el-button type="primary" :loading="isLoading('hooksCreateEndpoint')" @click="submitCreate">创建</el-button>
        </div>
      </template>
    </el-dialog>

    <!-- 令牌只显示一次；关闭对话框即从内存清掉，列表与日志永不显示。 -->
    <el-dialog
      :model-value="!!created"
      title="接入点凭据"
      width="520px"
      @update:model-value="(value) => { if (!value) closeCreated(); }"
    >
      <template v-if="created">
        <el-alert
          type="warning"
          :closable="false"
          title="令牌只显示这一次。关闭后无法再次查看，只能轮换；不要写进日志、截图或仓库。"
        />
        <div class="hook-token">
          <code>{{ created.token }}</code>
          <el-button text @click="copyToken">复制令牌</el-button>
        </div>
        <p class="hook-note">
          接入点：{{ created.endpoint.name }}；发送地址：
          <span class="hook-mono">{{ createdAddress }}</span>
          <el-button text @click="copyText(createdAddress, '发送地址')">复制地址</el-button>
        </p>
        <p v-if="!status?.running" class="hook-note">接收服务当前未运行：在「设置」里启用后发送方才连得上。</p>
      </template>
      <template #footer>
        <div class="dialog-actions">
          <el-button type="primary" @click="closeCreated">我已保存</el-button>
        </div>
      </template>
    </el-dialog>

    <!-- Jenkins Notification 的独立 Basic 密码；同样只显示一次。 -->
    <el-dialog
      :model-value="!!notification"
      title="Jenkins 通知凭据"
      width="560px"
      class="hook-notification-dialog"
      @update:model-value="(value) => { if (!value) closeNotification(); }"
    >
      <template v-if="notification">
        <el-alert
          type="warning"
          :closable="false"
          title="Basic 密码只显示这一次。关闭后只能重新生成；重新生成会让旧密码立即失效。"
        />
        <div class="hook-token">
          <code>{{ notification.token }}</code>
          <el-button text @click="copyNotificationPassword">复制密码</el-button>
        </div>
        <p class="hook-note">
          用户名固定为 jenkins，密码即上面的值。通知入口：
          <span class="hook-mono">{{ notificationAddress }}</span>
          <el-button text @click="copyText(notificationAddress, '通知入口地址')">复制地址</el-button>
        </p>
        <p class="hook-note">
          Jenkins 插件里新增 endpoint：Protocol HTTP、Format JSON、Event FINALIZED、Log lines 0；
          URL 放在 Secret text 凭据里，写成 https://jenkins:&lt;密码&gt;@&lt;你的域名&gt;/hooks/v1/endpoints/{{ notification.endpoint.id }}/jenkins-notification。
          不要把含密码的地址填进 Public URL。
        </p>
        <p class="hook-note">
          只映射 FINALIZED 元数据；构建日志、工件和负载里的 URL 不会被下载或抓取。
          reported_result 只代表 Jenkins 报告的结果，不是本机核验过的结果。
        </p>
      </template>
      <template #footer>
        <div class="dialog-actions">
          <el-button type="primary" @click="closeNotification">我已保存</el-button>
        </div>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.hook-tabs {
  display: flex;
  align-items: center;
  gap: 2px;
  border-bottom: 1px solid var(--border);
}
.hook-pane {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-height: 0;
}
.hook-endpoint__title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--fs-4);
}
.hook-endpoint__address {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  margin-top: 2px;
}
.hook-endpoint__address .hook-mono {
  color: var(--text-muted);
  font-size: var(--fs-2);
  overflow-wrap: anywhere;
}
.hook-endpoint__actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 4px 8px;
  flex: none;
}
.hook-setting-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-soft);
}
.hook-setting-row > span:first-child {
  min-width: 96px;
  color: var(--text-secondary);
  font-size: var(--fs-4);
}
.hook-stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
  gap: 10px 16px;
  margin: 4px 0 0;
}
.hook-stats dt {
  color: var(--text-muted);
  font-size: var(--fs-2);
}
.hook-stats dd {
  margin: 0;
  font-size: var(--fs-5);
  font-weight: 600;
}
.hook-note {
  margin: 8px 0 0;
  color: var(--text-muted);
  font-size: var(--fs-3);
  line-height: 1.5;
}
.hook-token {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin: 10px 0 4px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-sunken);
}
.hook-token code {
  font-family: var(--font-mono);
  font-size: var(--fs-3);
  overflow-wrap: anywhere;
}
.hook-mono {
  font-family: var(--font-mono);
}
</style>
