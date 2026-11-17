<script setup>
// Hook 页：收件箱 / 接入点 / 设置。接入点的令牌只在创建与轮换时显示一次；
// 发送方地址不含令牌；接收服务只绑定用户确认的可信网卡。
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
  deleteEndpoint,
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
  testCommand,
} from './hooks.js';
import HookInbox from './HookInbox.vue';

const TAB_ORDER = ['inbox', 'endpoints', 'settings'];
const activeTab = ref('inbox');
// 确认框可能比页面活得久；离页后不再发起新的破坏性操作。
let active = true;

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
const PAGE_DESC = '收下局域网发送方推来的事件与附件；「已入箱」只表示内容完整入库，不代表已审核或安全。接收端不执行内容，也不代发送方脱敏。正文、参数、元数据与附件共用 5 GiB 预算，最多 10,000 条事件；达到限制时自动清理历史记录。';

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
const sourceType = ref('generic');

function openCreate() {
  draftName.value = '';
  sourceType.value = 'generic';
  createVisible.value = true;
}

async function submitCreate() {
  const name = draftName.value.trim();
  const template = sourceType.value;
  const kinds = ['text', 'parameters'];
  if (!name) {
    toast('请先填写接入点名称', 4000, 'warning');
    return;
  }
  try {
    const result = await createEndpoint(name, kinds);
    if (!active) return;
    createVisible.value = false;
    if (template === 'jenkins') {
      await rotateNotificationToken(result.endpoint.id);
      clearCreatedEndpoint();
    }
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
  if (!ok || !active) return;
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
  if (!ok || !active) return;
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
    if (!ok || !active) return;
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
const networkDraft = ref('');
const subnetsDraft = ref('');
const settingsDirty = ref(false);
const interfaces = computed(() => status.value?.interfaces || []);
const networkKey = (item) => JSON.stringify([item.name, item.address, item.netmask]);
const selectedNetwork = computed(() => interfaces.value.find(item => networkKey(item) === networkDraft.value));
const serviceLabel = computed(() => status.value?.running ? '接收中' : status.value?.config?.enabled ? '已暂停' : '已停止');

function restoreSavedSettings(config = status.value?.config) {
  if (!config) return;
  portDraft.value = config.port;
  networkDraft.value = config.bind_ip ? JSON.stringify([config.interface_name, config.bind_ip, config.netmask]) : (interfaces.value.length === 1 ? networkKey(interfaces.value[0]) : '');
  subnetsDraft.value = (config.allowed_subnets || []).join('\n');
  settingsDirty.value = false;
}

watch(() => status.value?.config, (config) => {
  if (!settingsDirty.value) restoreSavedSettings(config);
}, { immediate: true });

async function saveNetwork(enabled = !!status.value?.config?.enabled) {
  const port = Number(portDraft.value);
  const network = selectedNetwork.value;
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    toast('端口需在 1024–65535 之间', 4000, 'warning');
    return;
  }
  if (enabled && !network) {
    toast('请先选择当前在线的可信局域网', 4000, 'warning');
    return;
  }
  const config = status.value?.config || {};
  const ok = await saveHooksConfig(enabled, port, {
    interfaceName: network?.name || config.interface_name || '',
    bindIp: network?.address || config.bind_ip || '',
    netmask: network?.netmask || config.netmask || '',
    allowedSubnets: subnetsDraft.value.split(/[\n,]+/).map(value => value.trim()).filter(Boolean),
  });
  if (ok) settingsDirty.value = false;
}

function toggleService(enabled) {
  if (enabled) return saveNetwork(true);
  // 停止只改已保存的 enabled。无效端口 / 网段 / 离线网卡草稿不能阻止停服，
  // 也不能趁停服写入配置；dirty 草稿保留，用户可显式恢复已保存内容。
  return saveHooksConfig(false, status.value?.config?.port, { disableOnly: true });
}
function copyTest(row, shell) {
  const command = testCommand(row, shell);
  if (command) copyText(command, '测试命令');
}
function copyJenkinsSecret() {
  if (!notification.value || !notificationAddress.value) return;
  const url = new URL(notificationAddress.value);
  url.username = 'jenkins';
  url.password = notification.value.token;
  copyText(url.href, 'Jenkins Secret text');
}

let refreshTimer;
onMounted(() => {
  refreshTimer = setInterval(() => {
    if (!active || document.hidden) return;
    refreshHooksStatus();
    if (activeTab.value === 'endpoints') loadEndpoints();
  }, 5000);
});

onMounted(() => {
  refreshHooksStatus();
  loadEndpoints();
  loadInbox();
});

// 离开页面即丢掉还在内存里的一次性令牌：它们只该在那一个对话框里存在。
onUnmounted(() => {
  active = false;
  clearInterval(refreshTimer);
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

    <div class="hook-onboarding">
      <span>{{ serviceLabel }}{{ status?.lan_ip ? ' · ' + status.lan_ip : '' }}</span>
      <el-button text @click="selectTab('settings')">1. 选择网络并启用</el-button>
      <el-button text @click="openCreate">2. 新建来源</el-button>
      <span class="muted">3. 从发送机运行测试</span>
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
                <el-tag v-else size="small" :type="status?.running ? 'success' : 'warning'">{{ status?.running ? '可接收' : '服务未运行' }}</el-tag>
              </div>
              <div class="page-row__meta">
                允许：{{ row.allowed_kinds.map(kindLabel).join(' / ') }} ·
                最近接收：{{ row.last_event_at ? hookTime(row.last_event_at) : '还没有' }} ·
                占用：{{ formatBytes(row.bytes) }} · 错误 {{ row.error_count }} 次 ·
                {{ row.notification_enabled ? 'Jenkins · 用真实构建通知验证接入' : '通用脚本' }}
              </div>
              <div class="hook-endpoint__address">
                <span class="hook-mono" :title="endpointAddress(row)">{{ endpointAddress(row) || '请先启用接收服务' }}</span>
                <el-button text :disabled="!endpointAddress(row)" @click="copyAddress(row)">复制地址</el-button>
                <el-button v-if="!row.notification_enabled" text :disabled="!endpointAddress(row)" @click="copyTest(row, 'powershell')">PowerShell 测试</el-button>
                <el-button v-if="!row.notification_enabled" text :disabled="!endpointAddress(row)" @click="copyTest(row, 'bash')">Bash 测试</el-button>
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
              <el-button v-if="!row.notification_enabled" text :loading="isLoading('hooksRotate:' + row.id)" @click="rotateToken(row)">轮换令牌</el-button>
              <el-button
                v-if="row.notification_enabled"
                text
                :loading="isLoading('hooksNotification:' + row.id)"
                @click="rotateNotification(row)"
              >轮换通知凭据</el-button>
              <details v-if="!row.notification_enabled" class="hook-more"><summary>更多</summary>
                <el-button text :loading="isLoading('hooksNotification:' + row.id)" @click="rotateNotification(row)">接入 Jenkins</el-button>
              </details>
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
          <el-tag size="small" :type="status?.running ? 'success' : 'info'">{{ serviceLabel }}</el-tag>
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
          <span>可信局域网</span>
          <el-select v-model="networkDraft" placeholder="选择网卡与地址" :disabled="isLoading('hooksSaveConfig')" @change="settingsDirty = true">
            <el-option v-for="item in interfaces" :key="networkKey(item)" :value="networkKey(item)" :label="item.name + ' · ' + item.address + ' (' + item.cidr + ')'" />
          </el-select>
          <el-button :loading="isLoading('hooksSaveConfig')" @click="saveNetwork()">保存</el-button>
        </div>
        <div v-if="settingsDirty" class="hook-setting-draft">
          <span>草稿未保存；关闭服务不会应用这些修改。</span>
          <el-button text :disabled="isLoading('hooksSaveConfig')" @click="restoreSavedSettings()">恢复已保存</el-button>
        </div>
        <el-alert v-if="!interfaces.length" type="warning" :closable="false" title="没有可用的私网地址，请连接可信局域网后刷新。" />
        <details class="hook-advanced">
          <summary>高级网络设置</summary>
          <div class="hook-setting-row">
            <span>监听端口</span>
            <el-input-number v-model="portDraft" :min="1024" :max="65535" :disabled="isLoading('hooksSaveConfig')" controls-position="right" @change="settingsDirty = true" />
          </div>
          <div class="hook-setting-row">
            <span>额外可信网段</span>
            <el-input v-model="subnetsDraft" :disabled="isLoading('hooksSaveConfig')" type="textarea" :rows="2" placeholder="跨 VLAN 时填写，例如 10.20.0.0/16；每行一个" @input="settingsDirty = true" />
          </div>
          <p class="hook-note">默认允许所选网卡的实际子网。更换网络后需重新确认；不会回退监听所有网卡。HTTP 明文传输，仅用于可信内部网络。应用不会修改系统防火墙。</p>
        </details>
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
          正文、参数、元数据与附件共用 5 GiB 预算，最多 10,000 条事件；达到限制时先回收空墓碑，再清理最早的非接收中事件。
          墓碑最多保留 30 天，数量压力下可能提前回收。接收中事件 24 小时未完成即过期；后台每 10 分钟清理。
          无可清理空间时拒绝新事件。手动永久删除在事件详情里。
        </p>
      </div>
    </div>

    <!-- 新建接入点：不展示地址与令牌，创建成功后再一次性显示。 -->
    <el-dialog v-model="createVisible" title="新建接入点" width="440px">
      <el-form label-width="84px">
        <el-form-item label="名称">
          <el-input v-model="draftName" maxlength="64" placeholder="例如：Jenkins 构建" />
        </el-form-item>
        <el-form-item label="来源类型">
          <el-radio-group v-model="sourceType">
            <el-radio value="generic">通用脚本</el-radio>
            <el-radio value="jenkins">Jenkins</el-radio>
          </el-radio-group>
        </el-form-item>
      </el-form>

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
          <span class="hook-mono">{{ createdAddress || '接收服务未运行' }}</span>
          <el-button text :disabled="!createdAddress" @click="copyText(createdAddress, '发送地址')">复制地址</el-button>
        </p>
        <div class="dialog-actions">
          <el-button :disabled="!createdAddress" @click="copyTest(created.endpoint, 'powershell')">复制 PowerShell 测试</el-button>
          <el-button :disabled="!createdAddress" @click="copyTest(created.endpoint, 'bash')">复制 Bash 测试</el-button>
        </div>
        <p class="hook-note">在另一台发送机运行命令，按提示输入令牌；收件箱出现 LAN test 才说明远端投递成功。本机测试不代表局域网已打通。</p>
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
          <span class="hook-mono">{{ notificationAddress || '接收服务未运行' }}</span>
          <el-button text :disabled="!notificationAddress" @click="copyText(notificationAddress, '通知入口地址')">复制地址</el-button>
        </p>
        <p class="hook-note">
          Jenkins 插件里新增 endpoint：Protocol HTTP、Format JSON、Event FINALIZED、Log lines 0；
          将下面复制的当前局域网完整地址保存为 Jenkins Secret text 凭据。
          不要把含密码的地址填进 Public URL。
        </p>
        <el-button :disabled="!notificationAddress" @click="copyJenkinsSecret">复制 Jenkins Secret text</el-button>
        <p class="hook-note">HTTP 仅限可信局域网；发送一次真实构建通知后检查收件箱。</p>
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
.hook-onboarding { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: var(--fs-3); }
.hook-advanced { margin-top: 12px; }
.hook-setting-draft { display: flex; align-items: center; gap: 8px; color: var(--text-muted); font-size: var(--fs-3); }
.hook-advanced summary, .hook-more summary { cursor: pointer; color: var(--text-secondary); font-size: var(--fs-3); }
.hook-setting-row .el-select { flex: 1; min-width: 0; }

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
