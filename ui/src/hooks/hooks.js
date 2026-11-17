// Hook 收件箱的共享状态与动作：接收服务设置、接入点管理、事件列表与受控预览。
// 组件只读状态、调动作；与 Rust 的通信只经 bridge。令牌只在创建 / 轮换响应里
// 出现一次，放在 created 里交给对话框显示，关闭即清空，不写进列表或日志。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toastActionError, toastSuccess } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import { formatBytes } from '../shell/labels.js';

export const INBOX_PAGE_SIZE = 20;

export const hookStore = reactive({
  status: null,
  statusError: '',
  endpoints: [],
  endpointsError: '',
  inbox: {
    items: [],
    total: 0,
    loading: false,
    error: '',
    offset: 0,
    pendingUpdates: false,
    filter: { endpoint_id: '', state: '', kind: '', q: '' },
  },
  detail: null,
  detailLoading: false,
  detailError: '',
  preview: null,
  previewLoading: false,
  previewAttachmentId: null,
  previewError: '',
  created: null,
  // Jenkins Notification 的独立 Basic 密码；只在生成/轮换后显示一次。
  notification: null,
});

// 旧请求的结果不许覆盖新请求：切筛选、连点两行、翻页都会产生在途请求。
let inboxSeq = 0;
let detailSeq = 0;
let previewSeq = 0;
let credentialSeq = 0;
let notificationSeq = 0;
let statusSeq = 0;

export function stateLabel(state) {
  return {
    ready: '已入箱',
    receiving: '接收中',
    unavailable: '不可用',
    expired: '已过期',
    evicted: '已清理（容量）',
  }[state] || state || '未知';
}

export function stateTagType(state) {
  return {
    ready: 'success',
    receiving: 'warning',
    unavailable: 'danger',
    expired: 'info',
    evicted: 'info',
  }[state] || 'info';
}

export function kindLabel(kind) {
  return { image: '图片', log: '日志', text: '文本', parameters: '参数' }[kind] || kind || '附件';
}

export function hookTime(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value) || value <= 0) return '—';
  const date = new Date(value);
  const pad = (n) => String(n).padStart(2, '0');
  return date.getFullYear() + '-' + pad(date.getMonth() + 1) + '-' + pad(date.getDate()) + ' ' + pad(date.getHours()) + ':' + pad(date.getMinutes());
}

export function attachmentSummary(row) {
  if (row.state === 'receiving') return '接收中（' + (row.stored_attachments || 0) + '/' + (row.attachment_count || 0) + '）';
  if (!row.attachment_count) return '无附件';
  return row.attachment_count + ' 个附件 · ' + formatBytes(row.attachment_bytes);
}

/// 发送方要填的地址由服务实际绑定的内网 IP、端口与接入点 ID 拼出，不含令牌。
/// 未运行或地址不可用时返回空串，不允许复制不可用占位符。
export function endpointAddress(endpoint) {
  const port = hookStore.status?.config?.port;
  const host = hookStore.status?.lan_ip;
  if (!endpoint || !port || !host || !hookStore.status?.running) return '';
  return 'http://' + host + ':' + port + '/hooks/v1/endpoints/' + endpoint.id;
}

/// Jenkins Notification 的入口地址；Basic 密码不走 URL，由调用方另存。
export function notificationUrl(endpoint) {
  const base = endpointAddress(endpoint);
  return base ? base + '/jenkins-notification' : '';
}

export async function copyText(text, label) {
  try {
    await navigator.clipboard.writeText(text);
    toastSuccess(label + '已复制');
    return true;
  } catch (e) {
    toastActionError('复制' + label + '失败', e, '请手动选中文本复制');
    return false;
  }
}

export async function refreshHooksStatus() {
  const seq = ++statusSeq;
  try {
    const status = await invoke('hooks_status');
    if (seq !== statusSeq) return false;
    hookStore.status = status;
    hookStore.statusError = '';
    return true;
  } catch (e) {
    if (seq !== statusSeq) return false;
    hookStore.statusError = '读取 Hook 状态失败：' + e;
    return false;
  }
}

export async function loadEndpoints() {
  try {
    hookStore.endpoints = (await invoke('hooks_list_endpoints')) || [];
    hookStore.endpointsError = '';
    return true;
  } catch (e) {
    hookStore.endpointsError = '读取接入点失败：' + e;
    return false;
  }
}

export async function loadInbox() {
  const seq = ++inboxSeq;
  const filter = hookStore.inbox.filter;
  hookStore.inbox.loading = true;
  hookStore.inbox.pendingUpdates = false;
  hookStore.inbox.error = '';
  try {
    const result = await invoke('hooks_inbox_list', {
      filter: {
        endpoint_id: filter.endpoint_id || null,
        state: filter.state || null,
        attachment_kind: filter.kind || null,
        q: filter.q.trim() || null,
        limit: INBOX_PAGE_SIZE,
        offset: hookStore.inbox.offset,
      },
    });
    if (seq !== inboxSeq) return;
    // 删除或容量清理可能让末页消失；回退后仍沿用请求序号保护。
    const lastOffset = Math.max(0, Math.ceil((result.total || 0) / INBOX_PAGE_SIZE) - 1) * INBOX_PAGE_SIZE;
    if (hookStore.inbox.offset > lastOffset) {
      hookStore.inbox.offset = lastOffset;
      return await loadInbox();
    }
    hookStore.inbox.items = result.items || [];
    hookStore.inbox.total = result.total || 0;
  } catch (e) {
    if (seq !== inboxSeq) return;
    hookStore.inbox.error = '读取收件箱失败：' + e;
  } finally {
    if (seq === inboxSeq) hookStore.inbox.loading = false;
  }
}

// 后台检查不换页、不挪动现有行；有新增/清理时由用户点一次应用。
export async function pollInbox() {
  if (hookStore.inbox.loading) return;
  const seq = inboxSeq;
  const snapshot = JSON.stringify([hookStore.inbox.filter, hookStore.inbox.offset]);
  const filter = hookStore.inbox.filter;
  try {
    const result = await invoke('hooks_inbox_list', { filter: {
      endpoint_id: filter.endpoint_id || null, state: filter.state || null,
      attachment_kind: filter.kind || null, q: filter.q.trim() || null,
      limit: INBOX_PAGE_SIZE, offset: hookStore.inbox.offset,
    } });
    if (seq !== inboxSeq || snapshot !== JSON.stringify([hookStore.inbox.filter, hookStore.inbox.offset])) return;
    const items = result.items || [];
    const sameRows = items.map(row => row.id).join(',') === hookStore.inbox.items.map(row => row.id).join(',');
    if (sameRows && result.total === hookStore.inbox.total) hookStore.inbox.items = items;
    else if (!hookStore.inbox.items.length) {
      hookStore.inbox.items = items;
      hookStore.inbox.total = result.total || 0;
    } else hookStore.inbox.pendingUpdates = true;
  } catch {
    // 后台请求失败不盖掉用户正在查看的内容；手动刷新仍显示明确错误。
  }
}

export function cancelInboxRequests() {
  inboxSeq += 1;
  hookStore.inbox.loading = false;
}

// 命令本身不含秘密：发送机交互式输入已复制的令牌，不进入历史。
export function testCommand(endpoint, shell = 'powershell') {
  const base = endpointAddress(endpoint);
  if (!base) return '';
  if (shell === 'bash') return [
    'read -rsp "Hook token: " HOOK_TOKEN; echo',
    'HOOK_TEST_ID="test-$(date +%s)-$RANDOM"',
    `curl --fail-with-body -X POST '${base}/events' -H "Authorization: Bearer $HOOK_TOKEN" -H 'Content-Type: application/json' --data-binary @- <<EOF`,
    '{"schema_version":"1.0","external_id":"$HOOK_TEST_ID","event_type":"text","occurred_at":"$(date -u +%Y-%m-%dT%H:%M:%SZ)","title":"LAN test","text":"LAN connection test","commit":true}',
    'EOF',
    'unset HOOK_TOKEN',
  ].join('\n');
  return [
    "$token = Read-Host 'Hook token' -AsSecureString",
    "$body = @{ schema_version = '1.0'; external_id = [guid]::NewGuid().ToString(); event_type = 'text'; occurred_at = [DateTime]::UtcNow.ToString('o'); title = 'LAN test'; text = 'LAN connection test'; commit = $true } | ConvertTo-Json",
    "$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($token)",
    'try {',
    '  $headers = @{ Authorization = "Bearer " + [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) }',
    `  Invoke-RestMethod -Uri '${base}/events' -Method Post -Headers $headers -ContentType 'application/json' -Body $body`,
    '} finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr); Remove-Variable token,headers -ErrorAction SilentlyContinue }',
  ].join('\n');
}

export function refreshInbox() {
  return Promise.all([loadInbox(), refreshHooksStatus(), loadEndpoints()]);
}

export function setInboxFilter(patch = {}) {
  Object.assign(hookStore.inbox.filter, patch);
  hookStore.inbox.offset = 0;
  return loadInbox();
}

export function turnInboxPage(delta) {
  const total = hookStore.inbox.total;
  const offset = hookStore.inbox.offset + delta * INBOX_PAGE_SIZE;
  if (offset < 0 || offset >= total) return Promise.resolve();
  hookStore.inbox.offset = offset;
  return loadInbox();
}

export async function openEvent(id) {
  const seq = ++detailSeq;
  previewSeq += 1;
  hookStore.detail = null;
  hookStore.detailError = '';
  hookStore.preview = null;
  hookStore.previewError = '';
  hookStore.previewLoading = false;
  hookStore.previewAttachmentId = null;
  hookStore.detailLoading = true;
  try {
    const detail = await invoke('hooks_event_detail', { eventId: id });
    if (seq !== detailSeq) return;
    hookStore.detail = detail;
  } catch (e) {
    if (seq !== detailSeq) return;
    hookStore.detailError = '读取事件失败：' + e;
  } finally {
    if (seq === detailSeq) hookStore.detailLoading = false;
  }
}

/// 关窗即作废在途响应：否则迟到的详情会落到下一件打开的事件上。
export function closeDetail() {
  detailSeq += 1;
  previewSeq += 1;
  hookStore.detail = null;
  hookStore.detailError = '';
  hookStore.detailLoading = false;
  hookStore.preview = null;
  hookStore.previewError = '';
  hookStore.previewLoading = false;
  hookStore.previewAttachmentId = null;
}

export async function loadAttachmentPreview(attachment, offset = 0) {
  const eventId = hookStore.detail?.event?.id;
  if (!eventId) return;
  const seq = ++previewSeq;
  hookStore.previewLoading = true;
  hookStore.previewAttachmentId = attachment.id;
  hookStore.previewError = '';
  try {
    const preview = await invoke('hooks_attachment_preview', {
      eventId,
      attachmentId: attachment.id,
      offset,
    });
    if (seq !== previewSeq) return;
    hookStore.preview = { ...preview, attachment_id: attachment.id, filename: attachment.filename, encoding: attachment.encoding };
  } catch (e) {
    if (seq !== previewSeq) return;
    hookStore.previewError = '读取附件失败：' + e;
  } finally {
    if (seq === previewSeq) {
      hookStore.previewLoading = false;
      hookStore.previewAttachmentId = null;
    }
  }
}

export function deleteEvent(id) {
  return withLoading('hooksDeleteEvent', async () => {
    try {
      await invoke('hooks_delete_event', { eventId: id });
    } catch (e) {
      toastActionError('删除事件失败', e, '刷新收件箱后重试');
      throw e;
    }
    toastSuccess('事件与附件文件已永久删除');
    closeDetail();
    await Promise.all([loadInbox(), refreshHooksStatus(), loadEndpoints()]);
  });
}

export function createEndpoint(name, kinds) {
  return withLoading('hooksCreateEndpoint', async () => {
    const seq = ++credentialSeq;
    try {
      const created = await invoke('hooks_create_endpoint', { name, allowedKinds: kinds });
      if (seq === credentialSeq) hookStore.created = created;
      await Promise.all([loadEndpoints(), refreshHooksStatus()]);
      return created;
    } catch (e) {
      toastActionError('创建接入点失败', e, '名称最多 64 个字符；至少允许一种内容类型');
      throw e;
    }
  });
}

export function setEndpointEnabled(id, enabled) {
  return withLoading('hooksEndpoint:' + id, async () => {
    try {
      const row = await invoke('hooks_set_endpoint_enabled', { id, enabled });
      hookStore.endpoints = hookStore.endpoints.map((item) => (item.id === row.id ? row : item));
    } catch (e) {
      toastActionError(enabled ? '启用接入点失败' : '暂停接入点失败', e, '刷新接入点列表后重试');
      throw e;
    }
  });
}

export function rotateEndpointToken(id) {
  return withLoading('hooksRotate:' + id, async () => {
    const seq = ++credentialSeq;
    try {
      const created = await invoke('hooks_rotate_endpoint_token', { id });
      if (seq === credentialSeq) hookStore.created = created;
      return created;
    } catch (e) {
      toastActionError('轮换令牌失败', e, '刷新接入点列表后重试');
      throw e;
    }
  });
}

export function rotateNotificationToken(id) {
  return withLoading('hooksNotification:' + id, async () => {
    const seq = ++notificationSeq;
    try {
      const created = await invoke('hooks_rotate_endpoint_notification_token', { id });
      if (seq === notificationSeq) hookStore.notification = created;
      hookStore.endpoints = hookStore.endpoints.map((item) =>
        item.id === created.endpoint.id ? created.endpoint : item
      );
      return created;
    } catch (e) {
      toastActionError('生成通知凭据失败', e, '刷新接入点列表后重试');
      throw e;
    }
  });
}

export function clearNotificationCredential() {
  notificationSeq += 1;
  hookStore.notification = null;
}

export function deleteEndpoint(id) {
  return withLoading('hooksDeleteEndpoint:' + id, async () => {
    try {
      await invoke('hooks_delete_endpoint', { id });
    } catch (e) {
      toastActionError('删除接入点失败', e, '刷新接入点列表后重试');
      throw e;
    }
    toastSuccess('接入点已删除，旧令牌立即失效；已收到的事件仍保留在收件箱');
    await Promise.all([loadEndpoints(), refreshHooksStatus()]);
  });
}

// 关闭或卸载后作废在途凭据；后台操作仍完成，但秘密不回填到新页面。
export function clearCreatedEndpoint() {
  credentialSeq += 1;
  hookStore.created = null;
}

export function saveHooksConfig(enabled, port, network = {}) {
  return withLoading('hooksSaveConfig', async () => {
    try {
      statusSeq += 1;
      hookStore.status = await invoke('hooks_save_config', { enabled, port, ...network });
      statusSeq += 1;
      hookStore.statusError = '';
      if (hookStore.status?.running) toastSuccess('Hook 接收服务运行中');
      else if (enabled) toastActionError('Hook 接收服务已暂停', hookStore.status?.last_error || '当前没有可用的可信监听地址', '重新选择在线的可信网卡并保存');
      else toastSuccess('Hook 接收服务已停止');
      return true;
    } catch (e) {
      toastActionError('保存 Hook 设置失败', e, '确认可信网卡仍在线，并检查端口和允许的内网网段');
      await refreshHooksStatus();
      return false;
    }
  });
}
