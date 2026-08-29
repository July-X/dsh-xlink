<script setup>
// 启动容错事故面板：工作台启动失败被自动屏蔽后，把裁决权交给用户——
// 每个嫌疑对象可展开错误证据，并选择移除 / 重新启用；直接关闭即保持禁用。
import { computed, reactive } from 'vue';
import { Document, Close, RefreshLeft, Delete, View, Hide, Connection } from '@element-plus/icons-vue';
import { store } from '../store.js';
import { resolvePluginQuarantine } from '../plugins.js';
import { showLogs } from '../logs.js';

const incident = computed(() => store.incident);

const cause = computed(() => {
  const value = incident.value && incident.value.cause;
  if (value === 'plugin' || value === 'kernel' || value === 'unknown') return value;
  const suspects = (incident.value && incident.value.suspects) || [];
  if (suspects.some((suspect) => suspect.kind === 'plugin')) return 'plugin';
  if (suspects.some((suspect) => suspect.kind === 'kernel')) return 'kernel';
  return 'unknown';
});

const title = computed(() => {
  if (!incident.value) return '工作台异常';
  if (incident.value.recovered) return '已在安全模式下启动工作台';
  if (cause.value === 'plugin') return '工作台异常：疑似插件问题';
  if (cause.value === 'kernel') return '工作台异常：疑似内核问题';
  return '工作台异常：暂未能归因';
});

const causeLabel = computed(() => {
  if (cause.value === 'plugin') return '判断：疑似插件问题';
  if (cause.value === 'kernel') return '判断：疑似内核问题';
  return '判断：暂未能归因';
});

const healthText = computed(() => {
  const health = incident.value && incident.value.health;
  if (!health) return '';
  return [
    health.kind && `类型：${health.kind}`,
    health.message && `消息：${health.message}`,
    health.stack && `堆栈：\n${health.stack}`,
    health.page_url && `页面：${health.page_url}`,
  ]
    .filter(Boolean)
    .join('\n');
});

function goKernelVersions() {
  close();
  store.activePanel = 'versions';
}

// 证据区展开状态：按嫌疑对象 id 记录。
const expanded = reactive(new Set());
function toggleEvidence(id) {
  if (expanded.has(id)) {
    expanded.delete(id);
  } else {
    expanded.add(id);
  }
}

function close() {
  store.incidentVisible = false;
}

async function resolveSuspect(id, action) {
  const ok = await resolvePluginQuarantine(id, action);
  if (ok) {
    close();
  }
}
</script>

<template>
  <el-dialog
    v-model="store.incidentVisible"
    :title="title"
    width="min(720px, 92vw)"
    :show-close="false"
    append-to-body
  >
    <template #header>
      <div style="display: flex; align-items: center; gap: 8px">
        <span style="font-weight: 700; font-size: 15px">{{ title }}</span>
        <span style="flex: 1"></span>
        <el-button text :icon="Document" @click="showLogs">打开日志</el-button>
        <el-button text :icon="Close" @click="close">关闭</el-button>
      </div>
    </template>

    <div v-if="incident" class="incident-body" style="display: flex; flex-direction: column; gap: 12px">
      <p style="margin: 0">{{ incident.message || '' }}</p>
      <el-tag class="incident-cause" effect="plain" size="small">{{ causeLabel }}</el-tag>

      <details v-if="healthText" class="incident-health">
        <summary>查看前端自检证据</summary>
        <pre>{{ healthText }}</pre>
      </details>

      <div class="incident-list">
        <p v-if="!(incident.suspects || []).length" class="muted" style="margin: 0">未定位到具体插件或内核组件。</p>
        <div v-for="suspect in incident.suspects || []" :key="suspect.id" class="suspect-item">
          <div class="suspect-head">
            <span class="suspect-name">{{ suspect.name }}</span>
            <el-tag size="small" effect="plain">{{ suspect.kind === 'kernel' ? '内核组件' : '插件' }}</el-tag>
          </div>

          <div v-if="suspect.evidence" class="suspect-evidence">
            <el-button size="small" text :icon="expanded.has(suspect.id) ? Hide : View" @click="toggleEvidence(suspect.id)">
              {{ expanded.has(suspect.id) ? '收起证据' : '错误证据' }}
            </el-button>
            <pre v-if="expanded.has(suspect.id)">{{ suspect.evidence }}</pre>
          </div>
          <p v-else class="muted" style="margin: 0">{{ suspect.kind === 'kernel' ? '暂未捕获该内核组件的直接日志证据。' : '该插件没有直接的日志证据（安全模式批量停用时无具体归因）。' }}</p>

          <div v-if="suspect.kind === 'plugin'" class="btn-row suspect-actions">
            <el-button size="small" text :icon="RefreshLeft" @click="resolveSuspect(suspect.id, 'enable')">重新启用</el-button>
            <el-popconfirm
              title="确认移除该插件？"
              confirm-button-text="移除"
              cancel-button-text="取消"
              width="200"
              @confirm="resolveSuspect(suspect.id, 'remove')"
            >
              <template #reference>
                <el-button size="small" type="danger" plain :icon="Delete">移除插件</el-button>
              </template>
            </el-popconfirm>
            <span class="muted" style="font-size: 12px">不做操作即保持禁用</span>
          </div>
        </div>

        <!-- 尝试轨迹帮助用户理解看护做了什么；折叠为可展开行避免面板过长。 -->
        <details v-if="(incident.attempts || []).length > 1" class="incident-trail">
          <summary>查看自动处理过程</summary>
          <pre>{{ (incident.attempts || []).join('\n') }}</pre>
        </details>
      </div>

      <p v-if="incident.hint" class="muted" style="margin: 0">{{ incident.hint }}</p>
      <div v-if="cause !== 'plugin' && !incident.recovered" class="btn-row">
        <el-button type="warning" plain :icon="Connection" @click="goKernelVersions">前往内核版本页</el-button>
      </div>
    </div>
  </el-dialog>
</template>
