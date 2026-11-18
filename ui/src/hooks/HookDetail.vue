<script setup>
// 事件详情与受控预览：正文 / 参数纯文本展示，图片只经本地 IPC 读取，
// 日志按后端给的页偏移分页。这里不渲染 HTML，也不把发送方内容当指令。
import { computed } from 'vue';
import { isLoading } from '../shell/loading.js';
import { formatBytes } from '../shell/labels.js';
import { confirmDialog } from '../shell/notify.js';
import {
  hookStore,
  closeDetail,
  deleteEvent,
  loadAttachmentPreview,
  kindLabel,
  stateLabel,
  stateTagType,
  hookTime,
} from './hooks.js';

const props = defineProps({ modelValue: { type: Boolean, default: false } });
const emit = defineEmits(['update:modelValue']);

const detail = computed(() => hookStore.detail);
const event = computed(() => detail.value?.event || null);
const attachments = computed(() => detail.value?.attachments || []);
const preview = computed(() => hookStore.preview);
const title = computed(() => (event.value?.title || event.value?.external_id || '事件详情'));
const parametersText = computed(() => JSON.stringify(event.value?.parameters ?? {}, null, 2));
const hasParameters = computed(() => {
  const value = event.value?.parameters;
  return !!value && typeof value === 'object' && Object.keys(value).length > 0;
});
const imageSrc = computed(() =>
  preview.value?.data_base64
    ? 'data:' + (preview.value.media_type || 'image/png') + ';base64,' + preview.value.data_base64
    : ''
);
const expiryText = computed(() => {
  const row = event.value;
  if (!row) return '—';
  if (row.state === 'receiving') return '未完成接收，' + hookTime(row.expires_at) + ' 后过期';
  if (row.state === 'ready') return '按容量保留；达到 5 GiB 或 10,000 条事件限制时可能被清理';
  if (row.state === 'evicted') return '已因容量清理';
  if (row.state === 'expired') return '接收超时，内容已清理';
  return '—';
});

function attachmentStateLabel(state) {
  return { stored: '已归档', uploading: '上传中', pending: '等待上传', failed: '上传失败' }[state] || state;
}

function close() {
  emit('update:modelValue', false);
  closeDetail();
}

function onModelValue(value) {
  emit('update:modelValue', value);
  if (!value) closeDetail();
}

function previewAttachment(attachment) {
  return loadAttachmentPreview(attachment, 0);
}

function page(offset) {
  const attachment = attachments.value.find((item) => item.id === preview.value?.attachment_id);
  if (attachment && offset != null) loadAttachmentPreview(attachment, offset);
}

async function remove() {
  const row = event.value;
  if (!row) return;
  const size = attachments.value.reduce((sum, item) => sum + Math.max(0, item.size_bytes || 0), 0);
  const ok = await confirmDialog(
    '永久删除这条事件？',
    '将删除事件「' + (row.title || row.external_id) + '」、' +
      attachments.value.length + ' 个附件文件（合计 ' + formatBytes(size) + '）与它的登记记录。\n' +
      '删除不可恢复，也不进入墓碑；同 ID 再发送会创建新事件。',
    '永久删除'
  );
  if (!ok) return;
  try {
    await deleteEvent(row.id);
    if (event.value?.id === row.id) close();
  } catch {
    // toast 已由 store 给出；对话框保留，用户可重试或关闭。
  }
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    width="720px"
    class="hook-detail-dialog"
    @update:model-value="onModelValue"
  >
    <template #header>
      <div class="hook-detail-head">
        <span class="hook-detail-title">{{ title }}</span>
        <!-- 对端 IP 由服务端在收请求时观测，旧事件没有这项数据时如实标「未记录」。 -->
        <span
          v-if="event"
          class="hook-detail-source"
          :title="event.source_ip ? '服务端观测到的投递方 IP（TCP 对端）' : '升级前收到的事件没有记录投递方 IP'"
        >投递方 IP <span class="hook-mono">{{ event.source_ip || '未记录' }}</span></span>
        <el-tag v-if="event" size="small" :type="stateTagType(event.state)">{{ stateLabel(event.state) }}</el-tag>
      </div>
    </template>

    <div v-if="hookStore.detailLoading" v-loading="true" class="hook-detail-loading" aria-label="正在读取事件"></div>
    <el-alert v-else-if="hookStore.detailError" type="error" :closable="false" :title="hookStore.detailError" />

    <template v-else-if="event">
      <el-alert
        v-if="event.state === 'unavailable'"
        type="error"
        :closable="false"
        title="附件文件缺失或与记录不一致，内容已标为不可用；已收到的事件不会显示为完整。"
      />
      <el-alert
        v-else-if="event.state === 'expired'"
        type="info"
        :closable="false"
        title="接收超时：正文与附件已清理，仅保留最小登记（墓碑）用于解释重试。"
      />
      <el-alert
        v-else-if="event.state === 'evicted'"
        type="info"
        :closable="false"
        title="容量或事件数量达到限制：正文与附件已清理；墓碑最多保留 30 天，数量压力下可能提前回收。"
      />

      <dl class="hook-meta">
        <div><dt>来源</dt><dd>{{ event.endpoint_name }}</dd></div>
        <div><dt>事件类型</dt><dd class="hook-mono">{{ event.event_type }}</dd></div>
        <div><dt>接收时间</dt><dd>{{ hookTime(event.received_at) }}</dd></div>
        <div><dt>生命周期</dt><dd>{{ expiryText }}</dd></div>
      </dl>

      <section v-if="event.body_text" class="hook-section">
        <h3>正文</h3>
        <pre class="hook-text">{{ event.body_text }}</pre>
      </section>

      <section v-if="hasParameters" class="hook-section">
        <h3>参数</h3>
        <pre class="hook-text">{{ parametersText }}</pre>
      </section>

      <section class="hook-section">
        <h3>附件（{{ attachments.length }}）</h3>
        <p v-if="!attachments.length" class="muted">没有附件。</p>
        <div v-for="attachment in attachments" :key="attachment.id" class="hook-attachment">
          <div class="hook-attachment__main">
            <div class="hook-attachment__name">{{ attachment.filename }}</div>
            <div class="page-list-meta">
              {{ kindLabel(attachment.kind) }} · {{ formatBytes(attachment.size_bytes) }} ·
              {{ attachmentStateLabel(attachment.state) }}<template v-if="attachment.encoding"> · {{ attachment.encoding }}</template>
            </div>
          </div>
          <el-button
            text
            :disabled="attachment.state !== 'stored'"
            :loading="hookStore.previewLoading && hookStore.previewAttachmentId === attachment.id"
            @click="previewAttachment(attachment)"
          >预览</el-button>
        </div>
      </section>

      <section v-if="hookStore.previewError || preview" class="hook-section hook-preview">
        <div class="hook-preview-head">
          <h3>{{ preview ? kindLabel(preview.kind) + '预览 · ' + preview.filename : '附件预览' }}</h3>
          <span v-if="preview" class="muted">{{ formatBytes(preview.offset) }} 起 · 共 {{ formatBytes(preview.size_bytes) }}</span>
        </div>
        <el-alert v-if="hookStore.previewError" type="error" :closable="false" :title="hookStore.previewError" />
        <template v-else-if="preview">
          <el-alert
            v-if="preview.invalid_utf8"
            type="warning"
            :closable="false"
            title="这一页有无法按 UTF-8 解码的字节：可能是编码声明不是 UTF-8，或分页切断了多字节字符。原字节仍保留。"
          />
          <img v-if="imageSrc" :src="imageSrc" :alt="'图片预览：' + preview.filename" class="hook-image" />
          <pre v-else class="hook-log">{{ preview.text || '（空）' }}</pre>
          <div v-if="preview.kind === 'log'" class="hook-pager">
            <el-button text :disabled="preview.prev_offset == null" @click="page(preview.prev_offset)">上一页</el-button>
            <span class="muted">{{ preview.truncated ? '这是文件的一部分' : '已显示整个文件' }}</span>
            <el-button text :disabled="preview.next_offset == null" @click="page(preview.next_offset)">下一页</el-button>
          </div>
          <p v-if="preview.encoding" class="muted">声明编码：{{ preview.encoding }}；面板按 UTF-8 预览，原字节保留。</p>
        </template>
      </section>

      <details class="hook-tech">
        <summary>技术信息</summary>
        <dl>
          <div><dt>事件 ID</dt><dd class="hook-mono">{{ event.id }}</dd></div>
          <div><dt>外部 ID</dt><dd class="hook-mono">{{ event.external_id }}</dd></div>
          <div><dt>接入点 ID</dt><dd class="hook-mono">{{ event.endpoint_id }}</dd></div>
          <div v-if="event.sender_policy"><dt>发送方声明</dt><dd>{{ event.sender_policy }}</dd></div>
          <div v-for="attachment in attachments" :key="'tech-' + attachment.id">
            <dt>{{ attachment.filename }} 的 SHA-256</dt>
            <dd class="hook-mono">{{ attachment.sha256 || '未记录' }}</dd>
          </div>
        </dl>
      </details>
    </template>

    <template #footer>
      <div class="dialog-actions">
        <el-button @click="close">关闭</el-button>
        <el-button
          type="danger"
          plain
          :disabled="!event"
          :loading="isLoading('hooksDeleteEvent')"
          @click="remove"
        >永久删除</el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.hook-detail-head {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.hook-detail-title {
  font-family: var(--font-serif);
  font-size: var(--fs-7);
  font-weight: 600;
  overflow-wrap: anywhere;
}
.hook-detail-source {
  display: inline-flex;
  align-items: baseline;
  gap: 4px;
  flex: none;
  color: var(--text-muted);
  font-size: var(--fs-3);
}
.hook-detail-source .hook-mono {
  color: var(--text-secondary);
  font-size: var(--fs-3);
}
.hook-detail-loading {
  min-height: 140px;
}
/* 10 个附件 + 长正文/参数会超过固定 748 高的窗口；只滚正文区，页脚按钮仍在。 */
.hook-detail-dialog :deep(.el-dialog__body) {
  max-height: 64vh;
  overflow-y: auto;
}
.hook-meta {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px 20px;
  margin: 0 0 12px;
}
.hook-meta div {
  display: flex;
  gap: 8px;
  min-width: 0;
}
.hook-meta dt {
  flex: none;
  color: var(--text-muted);
  font-size: var(--fs-3);
}
.hook-meta dd {
  margin: 0;
  min-width: 0;
  overflow-wrap: anywhere;
  font-size: var(--fs-3);
  color: var(--text-secondary);
}
.hook-section {
  border-top: 1px solid var(--border);
  padding-top: 10px;
  margin-top: 10px;
}
.hook-section h3 {
  margin: 0 0 6px;
  font-size: var(--fs-subtitle);
  color: var(--text);
}
.hook-text,
.hook-log {
  margin: 0;
  max-height: 240px;
  overflow: auto;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-sunken);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--fs-3);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.hook-log {
  max-height: 320px;
}
.hook-attachment {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 0;
  border-bottom: 1px solid var(--border-soft);
}
.hook-attachment:last-child {
  border-bottom: 0;
}
.hook-attachment__main {
  min-width: 0;
}
.hook-attachment__name {
  font-size: var(--fs-4);
  overflow-wrap: anywhere;
}
.hook-preview-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
}
.hook-preview-head h3 {
  margin: 0;
}
.hook-image {
  display: block;
  max-width: 100%;
  max-height: 360px;
  border: 1px solid var(--border);
  border-radius: 6px;
  object-fit: contain;
}
.hook-pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: 6px;
}
.hook-tech {
  margin-top: 12px;
  border-top: 1px solid var(--border);
  padding-top: 8px;
  font-size: var(--fs-3);
}
.hook-tech summary {
  cursor: pointer;
  color: var(--text-secondary);
}
.hook-tech dl {
  margin: 8px 0 0;
}
.hook-tech div {
  display: flex;
  gap: 8px;
  min-width: 0;
}
.hook-tech dt {
  flex: none;
  color: var(--text-muted);
}
.hook-tech dd {
  margin: 0;
  min-width: 0;
  overflow-wrap: anywhere;
}
.hook-mono {
  font-family: var(--font-mono);
}
</style>
