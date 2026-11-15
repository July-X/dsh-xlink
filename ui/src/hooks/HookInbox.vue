<script setup>
// 收件箱：来源 / 状态 / 附件类型筛选 + 标题搜索 + 分页，点一行看详情。
// 列表不读附件内容；缩略图与原图只在详情里经受控 IPC 取。
import { computed, onUnmounted, ref } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { formatBytes } from '../shell/labels.js';
import {
  hookStore,
  INBOX_PAGE_SIZE,
  loadInbox,
  setInboxFilter,
  turnInboxPage,
  openEvent,
  stateLabel,
  stateTagType,
  attachmentSummary,
  hookTime,
} from './hooks.js';
import HookDetail from './HookDetail.vue';

const emit = defineEmits(['open-settings']);

const inbox = computed(() => hookStore.inbox);
const status = computed(() => hookStore.status);
const endpoints = computed(() => hookStore.endpoints);
const detailVisible = ref(false);

const remaining = computed(() => {
  const view = status.value;
  if (!view) return 0;
  return Math.max(0, (view.storage_budget_bytes || 0) - (view.storage_bytes || 0));
});

const page = computed(() => Math.floor(inbox.value.offset / INBOX_PAGE_SIZE) + 1);
const pages = computed(() => Math.max(1, Math.ceil(inbox.value.total / INBOX_PAGE_SIZE)));

const filtersActive = computed(() => {
  const filter = inbox.value.filter;
  return !!(filter.endpoint_id || filter.state || filter.kind || filter.q.trim());
});

const emptyText = computed(() => {
  if (filtersActive.value) return '没有符合筛选的事件';
  if (status.value && !status.value.config.enabled) return '接收服务未启用：在「设置」里启用后，发送方才连得上';
  return '还没有收到事件';
});

const searchTimer = ref(null);

function applyFilter() {
  return setInboxFilter({});
}

function scheduleSearch() {
  if (searchTimer.value) clearTimeout(searchTimer.value);
  searchTimer.value = setTimeout(() => {
    searchTimer.value = null;
    applyFilter();
  }, 300);
}

function clearSearch() {
  if (searchTimer.value) {
    clearTimeout(searchTimer.value);
    searchTimer.value = null;
  }
  applyFilter();
}

function open(row) {
  detailVisible.value = true;
  openEvent(row.id);
}

function onRowKey(event, row) {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    open(row);
  }
}

onUnmounted(() => {
  if (searchTimer.value) clearTimeout(searchTimer.value);
});
</script>

<template>
  <div class="hook-inbox">
    <div class="hook-capacity">
      <span>存储 {{ formatBytes(status?.storage_bytes) }} / {{ formatBytes(status?.storage_budget_bytes) }}</span>
      <span>可用 {{ formatBytes(remaining) }}</span>
      <span>{{ status?.event_count || 0 }} 个事件 · {{ status?.endpoint_count || 0 }} 个接入点</span>
      <span :class="['hook-service', { 'is-off': !status?.running }]">{{ status?.running ? '服务运行中' : '服务已停止' }}</span>
      <el-button text @click="emit('open-settings')">容量与设置</el-button>
    </div>

    <div class="hook-filters">
      <el-select v-model="inbox.filter.endpoint_id" class="hook-select" aria-label="按来源筛选" @change="applyFilter">
        <el-option label="全部来源" value="" />
        <el-option v-for="endpoint in endpoints" :key="endpoint.id" :label="endpoint.name" :value="endpoint.id" />
      </el-select>
      <el-select v-model="inbox.filter.state" class="hook-select" aria-label="按接收状态筛选" @change="applyFilter">
        <el-option label="全部状态" value="" />
        <el-option label="接收中" value="receiving" />
        <el-option label="已入箱" value="ready" />
        <el-option label="不可用" value="unavailable" />
        <el-option label="已过期" value="expired" />
      </el-select>
      <el-select v-model="inbox.filter.kind" class="hook-select" aria-label="按附件类型筛选" @change="applyFilter">
        <el-option label="全部内容" value="" />
        <el-option label="含图片" value="image" />
        <el-option label="含日志" value="log" />
      </el-select>
      <el-input
        v-model="inbox.filter.q"
        class="hook-search"
        placeholder="搜索标题或外部 ID"
        clearable
        @input="scheduleSearch"
        @clear="clearSearch"
        @keyup.enter="applyFilter"
      />
      <el-button text :icon="Refresh" :loading="inbox.loading" @click="loadInbox">刷新</el-button>
    </div>

    <el-alert v-if="inbox.error" type="error" :closable="false" :title="inbox.error" />
    <el-alert v-if="hookStore.statusError" type="warning" :closable="false" :title="hookStore.statusError" />

    <div v-loading="inbox.loading" class="hook-list">
      <el-empty v-if="!inbox.loading && !inbox.items.length" :description="emptyText" />
      <div
        v-for="row in inbox.items"
        :key="row.id"
        class="page-row hook-row"
        role="button"
        tabindex="0"
        @click="open(row)"
        @keydown="onRowKey($event, row)"
      >
        <div class="page-row__main">
          <div class="hook-row__title">{{ row.title || row.external_id }}</div>
          <div class="page-row__meta">
            {{ row.endpoint_name }} · <span class="hook-mono">{{ row.event_type }}</span> ·
            {{ hookTime(row.received_at) }} · {{ attachmentSummary(row) }}
          </div>
        </div>
        <div class="hook-row__side">
          <el-tag size="small" :type="stateTagType(row.state)">{{ stateLabel(row.state) }}</el-tag>
          <span class="muted">{{ formatBytes(row.attachment_bytes) }}</span>
        </div>
      </div>
    </div>

    <div v-if="inbox.total > INBOX_PAGE_SIZE" class="hook-pagebar">
      <el-button text :disabled="page <= 1" @click="turnInboxPage(-1)">上一页</el-button>
      <span class="muted">第 {{ page }} / {{ pages }} 页 · 共 {{ inbox.total }} 条</span>
      <el-button text :disabled="page >= pages" @click="turnInboxPage(1)">下一页</el-button>
    </div>

    <HookDetail v-model="detailVisible" />
  </div>
</template>

<style scoped>
.hook-inbox {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
}
.hook-capacity {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 16px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border);
  font-size: var(--fs-3);
  color: var(--text-secondary);
}
.hook-service {
  color: var(--success);
}
.hook-service.is-off {
  color: var(--warning);
}
.hook-filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.hook-select {
  width: 144px;
}
.hook-search {
  flex: 1 1 180px;
  min-width: 140px;
}
.hook-list {
  min-height: 180px;
}
.hook-row {
  cursor: pointer;
}
.hook-row__title {
  font-size: var(--fs-4);
  overflow-wrap: anywhere;
}
.hook-row__side {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: none;
}
.hook-pagebar {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--border);
}
.hook-mono {
  font-family: var(--font-mono);
}
</style>
