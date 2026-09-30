<script setup>
// 启动容错事故面板：工作台启动失败被自动屏蔽后，把裁决权交给用户——
// 每个嫌疑对象可展开错误证据，并选择移除 / 重新启用；直接关闭即保持禁用。
import { computed, reactive, ref } from 'vue';
import { Document, Close, RefreshLeft, Delete, View, Hide, Connection, CopyDocument } from '@element-plus/icons-vue';
import { store, globalBusy } from '../store.js';
import { withLoading, isLoading } from '../shell/loading.js';
import { resolvePluginQuarantine } from '../plugins/plugins.js';
import { showLogs } from '../logs/logs.js';
import {
  incidentCause,
  incidentTitle,
  incidentCauseLabel,
  incidentDestination,
  incidentDestinationLabel,
  incidentHealthSections,
  incidentHealthPlainText,
} from './incidents.js';

const incident = computed(() => store.incident);

const cause = computed(() => incidentCause(incident.value));

const title = computed(() => incidentTitle(incident.value));

const causeLabel = computed(() => incidentCauseLabel(incident.value));

// 底部「下一步」按钮的落点：插件问题由每个嫌疑对象自己的按钮处置，环境问题去
// 设置页（端口 / 数据目录），其余去内核版本页。
const destination = computed(() => incidentDestination(incident.value));
const destinationLabel = computed(() => incidentDestinationLabel(destination.value));

// 当前内核版本：同一份证据在不同版本上的含义完全不同（客户端模块列表逐版变化），
// 排障时它是第一个要对照的事实，埋在证据堆里等于没有。
const kernelVersion = computed(() => {
  const kernel = store.view && store.view.kernel;
  return (kernel && kernel.active) || '';
});

// 证据拆成带标签的分段：组合路由那种一行几十个包名的地址单独放一节并列出包名，
// 其余原样保留，不丢任何原始信息。
const healthSections = computed(() => incidentHealthSections(incident.value && incident.value.health));

const copied = ref(false);

async function copyEvidence() {
  const text = incidentHealthPlainText(incident.value && incident.value.health);
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    copied.value = true;
    window.setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch {
    copied.value = false;
  }
}

function goDestination() {
  close();
  store.activePanel = destination.value;
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
  // 卸载/重新接线会跑 pnpm，可能几十秒。挂上按嫌疑对象粒度的 loading 并
  // 借用互斥租约，避免长任务期间被反复点击（进度浮层现在也在弹层之上，
  // 用户能看到它在做什么）。
  const ok = await withLoading(`incidentResolve:${id}`, () =>
    resolvePluginQuarantine(id, action)
  );
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
      <p v-if="kernelVersion" class="muted" style="margin: 0">当前内核版本：{{ kernelVersion }}</p>
      <el-tag class="incident-cause" effect="plain" size="small">{{ causeLabel }}</el-tag>

      <details v-if="healthSections.length" class="incident-health">
        <summary>查看前端自检证据</summary>
        <div class="evidence-toolbar">
          <el-button size="small" text :icon="CopyDocument" @click="copyEvidence">
            {{ copied ? '已复制' : '复制证据' }}
          </el-button>
        </div>
        <div v-for="section in healthSections" :key="section.label" class="evidence-row">
          <span class="evidence-label">{{ section.label }}</span>
          <div v-if="section.members" class="evidence-members">
            <el-tag v-for="name in section.members.slice(0, 16)" :key="name" size="small" effect="plain" type="info">
              {{ name }}
            </el-tag>
            <span v-if="section.members.length > 16" class="muted">
              …另有 {{ section.members.length - 16 }} 个，点「复制证据」取完整列表
            </span>
          </div>
          <pre v-else class="evidence-text">{{ section.text }}</pre>
        </div>
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
            <el-button
              size="small"
              text
              :icon="RefreshLeft"
              :loading="isLoading('incidentResolve:' + suspect.id)"
              :disabled="globalBusy"
              @click="resolveSuspect(suspect.id, 'enable')"
            >
              重新启用
            </el-button>
            <el-popconfirm
              title="确认移除该插件？"
              confirm-button-text="移除"
              cancel-button-text="取消"
              width="200"
              @confirm="resolveSuspect(suspect.id, 'remove')"
            >
              <template #reference>
                <el-button
                  size="small"
                  type="danger"
                  plain
                  :icon="Delete"
                  :loading="isLoading('incidentResolve:' + suspect.id)"
                  :disabled="globalBusy"
                >
                  移除插件
                </el-button>
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
        <el-button type="warning" plain :icon="Connection" @click="goDestination">{{ destinationLabel }}</el-button>
      </div>
    </div>
  </el-dialog>
</template>

<style scoped>
/* 证据分段：标签定宽右对齐，右侧内容各自滚动。放在组件内而不是 theme.css——
   后者是只许下调的反棘轮大文件，事故面板的排版不值得从它那里借预算。 */
.evidence-toolbar { display: flex; justify-content: flex-end; margin-top: 4px; }
.evidence-row { display: flex; align-items: flex-start; gap: 8px; margin-top: 8px; }
.evidence-label { flex: 0 0 104px; color: var(--muted); font-size: 12.5px; line-height: 20px; text-align: right; }
.evidence-members { display: flex; flex-wrap: wrap; gap: 4px; max-height: 220px; overflow-y: auto; }
.evidence-text { flex: 1; min-width: 0; margin-top: 0 !important; }
</style>
