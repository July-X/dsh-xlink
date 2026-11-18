// Hook 收件箱的共享状态与动作：接收服务设置、接入点管理、事件列表与受控预览。
// 组件只读状态、调动作；与 Rust 的通信只经 bridge。令牌只在创建 / 轮换响应里
// 出现一次，放在 created 里交给对话框显示，关闭即清空，不写进列表或日志。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { toast, toastActionError, toastSuccess } from '../shell/notify.js';
import { withLoading } from '../shell/loading.js';
import { formatBytes } from '../shell/labels.js';

export const INBOX_PAGE_SIZE = 20;

export const hookStore = reactive({
  status: null,
  statusError: '',
  endpoints: [],
  endpointsError: '',
  hasNewMessages: false,
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
  // Hook → DeepSeek 分析:准备页与交接记录。发送只发生在工作台,这里只准备。
  analysis: {
    visible: false,
    mode: 'prepare',
    plan: null,
    planError: '',
    loading: false,
    sessions: null,
    sessionsLoading: false,
    submitting: false,
    submitError: '',
    waiting: '',
  },
});

// 旧请求的结果不许覆盖新请求：切筛选、连点两行、翻页都会产生在途请求。
let inboxSeq = 0;
let detailSeq = 0;
let previewSeq = 0;
let credentialSeq = 0;
let notificationSeq = 0;
let statusSeq = 0;
let endpointsSeq = 0;
let hookNotificationsSeq = 0;
let latestNotifiedEvent;

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

export function inboxEmptyText(status, filtersActive) {
  if (filtersActive) return '没有符合筛选的事件';
  if (!status || status.running) return '还没有收到事件';
  if (status.config?.enabled) return '接收服务已暂停：请在「设置」里重新选择可信网卡并启用';
  return '接收服务未启用：在「设置」里启用后，发送方才连得上';
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
  const seq = ++endpointsSeq;
  try {
    const rows = (await invoke('hooks_list_endpoints')) || [];
    if (seq !== endpointsSeq) return false;
    hookStore.endpoints = rows;
    hookStore.endpointsError = '';
    return true;
  } catch (e) {
    if (seq !== endpointsSeq) return false;
    hookStore.endpointsError = '读取接入点失败：' + e;
    return false;
  }
}

// 独立于列表筛选和翻页，设置/接入点页签中也能发现新投递。
export async function pollHookNotifications() {
  const seq = hookNotificationsSeq;
  try {
    const result = await invoke('hooks_inbox_list', { filter: { limit: 1, offset: 0 } });
    if (seq !== hookNotificationsSeq) return;
    const latest = result.items?.[0] || null;
    if (latestNotifiedEvent === undefined) latestNotifiedEvent = latest;
    else if (latest && (!latestNotifiedEvent || (latest.id !== latestNotifiedEvent.id && latest.received_at >= latestNotifiedEvent.received_at))) {
      latestNotifiedEvent = latest;
      hookStore.hasNewMessages = true;
      toast('Hook 收到新的投递，请在收件箱查看');
    }
  } catch {
    // 后台检查失败时保留基线，下一轮重试，不打断当前操作。
  }
}

export function cancelHookNotifications() {
  hookNotificationsSeq += 1;
  latestNotifiedEvent = undefined;
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
    hookStore.hasNewMessages = false;
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
    if (sameRows && result.total === hookStore.inbox.total) {
      hookStore.inbox.items = items;
      hookStore.inbox.pendingUpdates = false;
    } else if (!hookStore.inbox.items.length) {
      hookStore.inbox.items = items;
      hookStore.inbox.total = result.total || 0;
      hookStore.inbox.pendingUpdates = false;
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
    `curl --noproxy '*' --fail -X POST '${base}/events' -H "Authorization: Bearer $HOOK_TOKEN" -H 'Content-Type: application/json' --data-binary @- <<EOF`,
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
    '  $oldProxy = [System.Net.WebRequest]::DefaultWebProxy',
    '  [System.Net.WebRequest]::DefaultWebProxy = $null',
    '  try {',
    `    if ($PSVersionTable.PSVersion.Major -ge 6) { Invoke-RestMethod -Uri '${base}/events' -Method Post -Headers $headers -ContentType 'application/json' -Body $body -NoProxy -MaximumRedirection 0 }`,
    `    else { Invoke-RestMethod -Uri '${base}/events' -Method Post -Headers $headers -ContentType 'application/json' -Body $body -MaximumRedirection 0 }`,
    '  } finally { [System.Net.WebRequest]::DefaultWebProxy = $oldProxy }',
    '} finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr); Remove-Variable token,headers,oldProxy -ErrorAction SilentlyContinue }',
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
    void refreshAnalysisPlan();
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
  analysisSeq += 1;
  hookStore.analysis.visible = false;
  hookStore.analysis.plan = null;
  hookStore.analysis.planError = '';
  hookStore.analysis.submitting = false;
  hookStore.analysis.submitError = '';
  hookStore.analysis.waiting = '';
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
    await Promise.all([loadInbox(), refreshHooksStatus(), loadEndpoints()]);
  });
}

export function createEndpoint(name, kinds) {
  return withLoading('hooksCreateEndpoint', async () => {
    const seq = ++credentialSeq;
    try {
      const created = await invoke('hooks_create_endpoint', { name, allowedKinds: kinds });
      endpointsSeq += 1;
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
      endpointsSeq += 1;
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
      endpointsSeq += 1;
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
      endpointsSeq += 1;
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
      const hint = network.disableOnly ? '检查配置目录并重试停用' : '确认可信网卡仍在线，并检查端口和允许的内网网段';
      toastActionError('保存 Hook 设置失败', e, hint);
      await refreshHooksStatus();
      return false;
    }
  });
}

// ---------------------------------------------------------------------------
// Hook → DeepSeek 分析(设计契约见 docs/features/hooks/deepseek-analysis-design.md)
// ---------------------------------------------------------------------------

let analysisSeq = 0;

/// 读权威计划(材料可选性、默认工作空间、实例与既有交接);由打开方控制可见性。
async function fetchPlan(seq) {
  const eventId = hookStore.detail?.event?.id;
  if (!eventId) return;
  const analysis = hookStore.analysis;
  analysis.loading = true;
  analysis.planError = '';
  try {
    const plan = await invoke('hooks_analysis_plan', { eventId });
    if (seq !== analysisSeq) return;
    analysis.plan = plan;
  } catch (e) {
    if (seq !== analysisSeq) return;
    analysis.plan = null;
    analysis.planError = '读取分析准备信息失败：' + e;
  } finally {
    if (seq === analysisSeq) analysis.loading = false;
  }
}

/// 打开准备页 / 记录页。
export async function openAnalysis(mode = 'prepare') {
  const analysis = hookStore.analysis;
  analysis.visible = true;
  analysis.mode = mode;
  analysis.submitError = '';
  analysis.waiting = '';
  await fetchPlan(++analysisSeq);
}

/// 详情打开后的后台刷新:只为「查看分析（N）」按钮取记录数,不开窗、不报错打断。
export async function refreshAnalysisPlan() {
  try {
    await fetchPlan(++analysisSeq);
  } catch {
    // 后台刷新失败不影响详情;按钮按 0 处理。
  }
}

export function closeAnalysis() {
  analysisSeq += 1;
  hookStore.analysis.visible = false;
  hookStore.analysis.submitting = false;
  hookStore.analysis.submitError = '';
  hookStore.analysis.waiting = '';
}

/// 「已有会话」列表:经工作台插件桥接;内核未运行时如实说不可用。
export async function loadAnalysisSessions() {
  const seq = analysisSeq;
  hookStore.analysis.sessionsLoading = true;
  try {
    const view = await invoke('hooks_analysis_sessions');
    if (seq !== analysisSeq) return;
    hookStore.analysis.sessions = view;
  } catch (e) {
    if (seq !== analysisSeq) return;
    hookStore.analysis.sessions = { available: false, reason: String(e), items: [] };
  } finally {
    if (seq === analysisSeq) hookStore.analysis.sessionsLoading = false;
  }
}

/// 提交一次交接:同一 requestId 幂等(双击/超时重试复用同一包与记录)。
/// 成功后轮询工作台确认;失败原地保留全部选择。
export async function submitAnalysisHandoff({ includeBody, logs, workspace }) {
  const event = hookStore.detail?.event;
  if (!event) return false;
  const seq = analysisSeq;
  const analysis = hookStore.analysis;
  const requestId =
    'ha-req-' + event.id + '-' + Date.now() + '-' + Math.floor(Math.random() * 1e9);
  analysis.submitting = true;
  analysis.submitError = '';
  analysis.waiting = '';
  try {
    const ack = await invoke('hooks_analysis_handoff', {
      request: { eventId: event.id, requestId, includeBody, logs, workspace },
    });
    if (seq !== analysisSeq) return false;
    // 工作台确认是异步的(可能还在启动内核):轮询交接状态,attached 才算交付。
    const deadline = Date.now() + 150_000;
    for (;;) {
      await new Promise((resolve) => setTimeout(resolve, 1000));
      if (seq !== analysisSeq || !hookStore.analysis.visible) return false;
      let status;
      try {
        status = await invoke('hooks_analysis_status', { handoffId: ack.handoffId });
      } catch {
        if (Date.now() >= deadline) break;
        continue; // 状态读失败按未决继续,deadline 兜底
      }
      if (seq !== analysisSeq) return false;
      if (status.state === 'attached') {
        analysis.waiting = '';
        return true;
      }
      if (status.state === 'failed' || status.state === 'unavailable') {
        analysis.submitError =
          (status.failure || '工作台未完成交接') + '；材料与选择已保留，可重试或改选会话';
        return false;
      }
      analysis.waiting =
        '正在把材料交给工作台（内核启动中可能需要等待）…已等待 ' +
        Math.round((Date.now() + 150_000 - deadline) / 1000) +
        ' 秒';
      if (Date.now() >= deadline) break;
    }
    analysis.submitError =
      '工作台长时间未确认交接；材料包与记录已保留，请确认工作台可用后从「查看分析」重试';
    return false;
  } catch (e) {
    if (seq !== analysisSeq) return false;
    analysis.submitError = '准备分析材料失败：' + e + '；选择已保留，可调整后重试';
    return false;
  } finally {
    if (seq === analysisSeq) analysis.submitting = false;
  }
}

/// 「查看分析」:同一实例/会话/工作空间恢复;失败如实提示,不换目标。
export async function reopenAnalysisRecord(handoffId) {
  try {
    await invoke('hooks_analysis_reopen', { handoffId });
    toastSuccess('正在打开工作台并定位原会话');
    return true;
  } catch (e) {
    toastActionError('无法恢复这次分析', e, '请确认当前壳与实例仍是发起分析的那一个');
    return false;
  }
}

/// 删除材料包与壳侧记录;内核会话与工作台历史不动。
export function deleteAnalysisRecord(handoffId) {
  return withLoading('hooksAnalysisDelete:' + handoffId, async () => {
    try {
      await invoke('hooks_analysis_delete', { handoffId });
    } catch (e) {
      toastActionError('删除分析材料失败', e, '请检查数据目录权限后重试');
      throw e;
    }
    toastSuccess('材料包与记录已删除；工作台里的会话不受影响');
    await openAnalysis(hookStore.analysis.mode);
  });
}
