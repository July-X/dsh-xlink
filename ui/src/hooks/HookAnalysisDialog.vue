<script setup>
// Hook → DeepSeek 分析的准备页与交接记录。只准备、只记录:唯一的模型调用
// 仍是用户在工作台输入框里显式发送(设计 §1)。选择在失败时原地保留。
import { computed, reactive, ref, watch } from 'vue';
import { ElInputNumber } from 'element-plus';
import { hookStore, hookTime, openAnalysis, closeAnalysis, loadAnalysisSessions, submitAnalysisHandoff, reopenAnalysisRecord, deleteAnalysisRecord, copyText } from './hooks.js';
import { confirmDialog, toast } from '../shell/notify.js';
import { formatBytes, tildePath } from '../shell/labels.js';
import { isLoading } from '../shell/loading.js';

const props = defineProps({ modelValue: { type: Boolean, default: false } });
const emit = defineEmits(['update:modelValue']);

const analysis = computed(() => hookStore.analysis);
const plan = computed(() => analysis.value.plan);
const isPrepare = computed(() => analysis.value.mode === 'prepare');

// 材料选择:正文默认勾;每个日志独立勾,默认勾符合限额的(设计 §3.2)。
const includeBody = ref(true);
const logSelection = reactive({});
// 会话选择:新建 / 已有,各自选择互不覆盖。
const sessionMode = ref('new');
const existingSessionId = ref('');
const customPath = ref('');

function materialRows() {
  const rows = plan.value?.materials || [];
  return rows;
}
function bodyRow() {
  return materialRows().find((row) => row.id === 'body') || null;
}
function logRows() {
  return materialRows().filter((row) => row.kind === 'log');
}
function existingSessions() {
  return hookStore.analysis.sessions?.items || [];
}
function selectedSession() {
  return existingSessions().find((row) => row.id === existingSessionId.value) || null;
}

// 打开(或计划刷新)时按后端可选性落默认值;已手选的尊重用户。
watch(
  () => [props.modelValue, plan.value?.materials?.length],
  ([visible]) => {
    if (!visible) return;
    for (const row of logRows()) {
      if (logSelection[row.id]) continue;
      logSelection[row.id] = {
        checked: row.selectable && row.sizeBytes <= (plan.value?.inputLimitBytes || 0),
        start: null,
        end: null,
      };
    }
    includeBody.value = !!bodyRow()?.selectable;
  },
  { immediate: true },
);

watch(
  () => [props.modelValue, sessionMode.value],
  ([visible, mode]) => {
    if (visible && mode === 'existing' && !hookStore.analysis.sessions && !hookStore.analysis.sessionsLoading) {
      void loadAnalysisSessions();
    }
  },
  { immediate: true },
);

function checkedLogs() {
  return logRows().filter((row) => logSelection[row.id]?.checked);
}

function selectedBytes() {
  let total = includeBody.value ? bodyRow()?.sizeBytes || 0 : 0;
  for (const row of checkedLogs()) total += row.sizeBytes || 0;
  return total;
}

function overLimit() {
  return selectedBytes() > (plan.value?.inputLimitBytes || Infinity);
}

function rangeOf(row) {
  const selection = logSelection[row.id];
  if (!selection || (selection.start == null && selection.end == null)) return null;
  return { attachmentId: row.id, startLine: selection.start, endLine: selection.end };
}

function validate() {
  if (!includeBody.value && !checkedLogs().length) {
    toast('至少选择一项材料（正文或日志）', 4000, 'warning');
    return null;
  }
  const logs = [];
  for (const row of checkedLogs()) {
    const range = rangeOf(row);
    if (range) {
      if (!range.startLine || !range.endLine || range.startLine < 1 || range.endLine < range.startLine) {
        toast('「' + row.filename + '」的行段需要 1 起且结束行不小于起始行；留空表示完整日志', 5000, 'warning');
        return null;
      }
      logs.push(range);
    } else {
      logs.push({ attachmentId: row.id, startLine: null, endLine: null });
    }
  }
  if (sessionMode.value === 'existing' && !existingSessionId.value) {
    toast('选择已有会话时必须先选定具体会话，或改选新建会话', 4000, 'warning');
    return null;
  }
  const session = selectedSession();
  if (sessionMode.value === 'existing' && session?.running) {
    toast('该会话正在生成回复，等它完成后再交接，或改选新建会话', 5000, 'warning');
    return null;
  }
  return {
    includeBody: includeBody.value,
    logs,
    workspace: sessionMode.value === 'existing'
      ? { mode: 'existing', sessionId: session.id, sessionWorkspace: session.cwd || '' }
      : { mode: 'new', path: customPath.value.trim() || null, sessionId: null },
  };
}

async function submit() {
  const payload = validate();
  if (!payload) return;
  const ok = await submitAnalysisHandoff(payload);
  if (ok) {
    close();
    // 交给工作台后刷新一次记录(不动用户在详情页的其它状态)。
    void openAnalysis('records');
    hookStore.analysis.visible = false;
  }
}

async function remove(record) {
  const ok = await confirmDialog(
    '删除这份分析材料？',
    '将删除材料包「' + record.handoffId + '」与壳侧记录；工作台里的会话与历史不受影响。删除不可恢复。',
    '删除',
  );
  if (!ok) return;
  await deleteAnalysisRecord(record.handoffId);
}

async function reopen(record) {
  if (record.state === 'failed' && !record.sessionId) {
    // 从未建立会话的失败记录:重新发起,而不是指向不存在的会话。
    toast('这次交接没有建立会话；请关闭后在「交给 AI 分析」里重新发起', 5000, 'info');
    return;
  }
  await reopenAnalysisRecord(record.handoffId);
}

function onModelValue(value) {
  emit('update:modelValue', value);
  if (!value) closeAnalysis();
}

function close() {
  emit('update:modelValue', false);
  closeAnalysis();
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    width="640px"
    class="hook-analysis-dialog"
    :close-on-click-modal="false"
    @update:model-value="onModelValue"
  >
    <template #header>
      <div class="hook-analysis-head">
        <span class="hook-analysis-title">{{ isPrepare ? '准备分析' : '查看分析（' + (plan?.handoffs?.length || 0) + '）' }}</span>
        <span v-if="isPrepare" class="hook-analysis-sub">选择会话、材料与工作空间</span>
      </div>
    </template>

    <div v-if="analysis.loading" v-loading="true" class="hook-analysis-loading" aria-label="正在读取分析准备信息"></div>
    <el-alert v-else-if="analysis.planError" type="error" :closable="false" :title="analysis.planError" />

    <!-- 记录页:同一实例/会话/工作空间恢复;删除只清材料与壳侧记录。 -->
    <template v-else-if="!isPrepare">
      <p v-if="!plan?.handoffs?.length" class="muted">这条事件还没有交给过分析。</p>
      <div v-for="record in plan?.handoffs || []" :key="record.handoffId" class="hook-analysis-record">
        <div class="hook-analysis-record__main">
          <div class="hook-analysis-record__title">{{ record.sessionTitle }}</div>
          <div class="page-list-meta">
            {{ hookTime(record.createdAt) }} · {{ record.stateLabel }} · {{ formatBytes(record.promptBytes) }} 引用<template v-if="record.instance"> · {{ record.instance }}</template>
          </div>
          <div v-if="record.failure" class="hook-analysis-record__failure">{{ record.failure }}</div>
        </div>
        <div class="hook-analysis-record__actions">
          <el-button text :disabled="!record.sessionId" @click="reopen(record)">打开</el-button>
          <el-button text type="danger" plain :loading="isLoading('hooksAnalysisDelete:' + record.handoffId)" @click="remove(record)">删除</el-button>
        </div>
      </div>
    </template>

    <template v-else-if="plan">
      <dl class="hook-meta">
        <div><dt>内核</dt><dd><span class="deepseek-wordmark">DeepSeek</span> 工作台</dd></div>
        <div><dt>实例</dt><dd class="hook-mono">{{ plan.instanceLabel }}</dd></div>
      </dl>

      <section class="hook-analysis-section">
        <h3>会话</h3>
        <el-radio-group v-model="sessionMode" class="hook-analysis-radios">
          <el-radio value="new">新建会话</el-radio>
          <el-radio value="existing">已有会话</el-radio>
        </el-radio-group>
        <template v-if="sessionMode === 'existing'">
          <div v-if="analysis.sessionsLoading" class="muted">正在读取工作台的会话列表…</div>
          <el-alert v-else-if="analysis.sessions && !analysis.sessions.available" type="info" :closable="false" :title="analysis.sessions.reason" />
          <template v-else>
            <el-select v-model="existingSessionId" placeholder="选择要交接到的会话" filterable class="hook-analysis-session-select">
              <el-option v-for="row in existingSessions()" :key="row.id" :value="row.id" :label="row.title" :disabled="row.running">
                <span>{{ row.title }}</span>
                <span class="hook-analysis-option-note">{{ row.running ? '正在生成回复，暂不可交接' : tildePath(row.cwd) }}</span>
              </el-option>
            </el-select>
            <p v-if="selectedSession()" class="muted hook-analysis-workspace-note">
              原工作空间：<span class="hook-mono">{{ selectedSession().cwd || '（未记录）' }}</span>
            </p>
          </template>
        </template>
      </section>

      <section class="hook-analysis-section">
        <h3>分析来源</h3>
        <p class="muted">只读快照，单次合计上限 {{ formatBytes(plan.inputLimitBytes) }}；日志可选行段。</p>
        <label v-if="bodyRow()" class="hook-analysis-material">
          <el-checkbox v-model="includeBody" :disabled="!bodyRow().selectable">{{ bodyRow().filename }}</el-checkbox>
          <span class="page-list-meta">{{ formatBytes(bodyRow().sizeBytes) }}</span>
        </label>
        <div v-for="row in logRows()" :key="row.id" class="hook-analysis-material">
          <label class="hook-analysis-material__check">
            <el-checkbox v-model="logSelection[row.id].checked" :disabled="!row.selectable">{{ row.filename }}</el-checkbox>
            <span class="page-list-meta">{{ formatBytes(row.sizeBytes) }}<template v-if="row.note"> · {{ row.note }}</template></span>
          </label>
          <template v-if="logSelection[row.id]?.checked">
            <div class="hook-analysis-range">
              <ElInputNumber v-model="logSelection[row.id].start" :min="1" :max="999999999" :controls="false" placeholder="起始行" size="small" />
              <span class="muted">至</span>
              <ElInputNumber v-model="logSelection[row.id].end" :min="1" :max="999999999" :controls="false" placeholder="结束行" size="small" />
              <span class="muted">留空 = 完整日志</span>
            </div>
            <details v-if="row.head" class="hook-analysis-head-preview">
              <summary>预览开头</summary>
              <pre>{{ row.head }}</pre>
            </details>
          </template>
        </div>
        <p v-if="overLimit()" class="hook-analysis-warn">
          当前勾选约 {{ formatBytes(selectedBytes()) }}，超过 {{ formatBytes(plan.inputLimitBytes) }} 上限：请给大日志选行段或取消部分材料。
        </p>
      </section>

      <section class="hook-analysis-section">
        <h3>工作空间</h3>
        <template v-if="sessionMode === 'new'">
          <div class="hook-analysis-workspace">
            <span class="hook-mono">{{ tildePath(customPath.trim() || plan.defaultWorkspacePath) }}</span>
            <el-button text @click="copyText(customPath.trim() || plan.defaultWorkspacePath, '工作空间路径')">复制</el-button>
          </div>
          <el-input v-model="customPath" placeholder="默认分析工作空间；输入绝对路径可改为项目目录（可用 ~ 开头）" clearable />
          <p class="muted">仅更改本次新会话的工作空间；不改实例默认目录，不迁移已有会话。</p>
        </template>
        <p v-else class="muted hook-analysis-workspace-note">沿用所选会话的工作空间，只读。</p>
      </section>
    </template>

    <el-alert v-if="analysis.submitError" type="error" :closable="false" :title="analysis.submitError" class="hook-analysis-error" />
    <p v-if="analysis.waiting" class="muted">{{ analysis.waiting }}</p>

    <template #footer>
      <div class="dialog-actions">
        <el-button v-if="!isPrepare" @click="openAnalysis('prepare')">发起新分析</el-button>
        <el-button @click="close">{{ isPrepare ? '取消' : '关闭' }}</el-button>
        <el-button
          v-if="isPrepare"
          type="primary"
          :loading="analysis.submitting"
          :disabled="analysis.loading || !!analysis.planError"
          @click="submit"
        >打开工作台</el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.hook-analysis-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}
.hook-analysis-title {
  font-family: var(--font-serif);
  font-size: var(--fs-7);
  font-weight: 600;
}
.hook-analysis-sub {
  color: var(--text-muted);
  font-size: var(--fs-3);
}
.hook-analysis-loading {
  min-height: 140px;
}
.hook-analysis-dialog :deep(.el-dialog__body) {
  max-height: 62vh;
  overflow-y: auto;
}
.hook-meta {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px 20px;
  margin: 0 0 4px;
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
.hook-analysis-section {
  border-top: 1px solid var(--border);
  padding-top: 10px;
  margin-top: 10px;
}
.hook-analysis-section h3 {
  margin: 0 0 6px;
  font-size: var(--fs-subtitle);
  color: var(--text);
}
.hook-analysis-radios {
  margin-bottom: 8px;
}
.hook-analysis-session-select {
  width: 100%;
}
.hook-analysis-option-note {
  float: right;
  color: var(--text-muted);
  font-size: var(--fs-2);
}
.hook-analysis-material {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 5px 0;
}
.hook-analysis-material__check {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex: 1;
  min-width: 0;
}
.hook-analysis-range {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 0 4px 24px;
}
.hook-analysis-range :deep(.el-input-number) {
  width: 110px;
}
.hook-analysis-head-preview summary {
  cursor: pointer;
  color: var(--text-secondary);
  font-size: var(--fs-3);
  padding-left: 24px;
}
.hook-analysis-head-preview pre {
  margin: 6px 0 0 24px;
  max-height: 160px;
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
.hook-analysis-warn {
  margin: 6px 0 0;
  color: var(--warning);
  font-size: var(--fs-3);
}
.hook-analysis-workspace {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 6px;
}
.hook-analysis-workspace .hook-mono {
  overflow-wrap: anywhere;
  font-size: var(--fs-3);
}
.hook-analysis-record {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-soft);
}
.hook-analysis-record:last-child {
  border-bottom: 0;
}
.hook-analysis-record__main {
  min-width: 0;
}
.hook-analysis-record__title {
  font-size: var(--fs-4);
  overflow-wrap: anywhere;
}
.hook-analysis-record__failure {
  margin-top: 2px;
  color: var(--danger);
  font-size: var(--fs-3);
  overflow-wrap: anywhere;
}
.hook-mono {
  font-family: var(--font-mono);
}
</style>
