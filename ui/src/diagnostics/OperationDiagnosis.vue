<script setup>
// 恢复 / 排查的阶段时间线。
//
// **两种 kind 共用一个组件**：它们回答的是同一个形状的问题——「这次操作停
// 在哪一步、结论是什么、下一步能做什么」。差异只在头部那句话与阶段的中文
// 名，都在 `diagnostic-labels.js` 里按 kind 查表，不各写一份模板：两份模板
// 必然漂，而漂掉的正好是用户在排查时读到的措辞。
//
// 这里**不放任何会改变状态的动作**：恢复本身、重启工作台都留在各自的面板
// 上（设计 §2.5.5）。诊断层只回答「发生了什么」，不替用户做决定——用户在
// 一个结论还不确定的页面上误点恢复，代价是真实的配置。
import { computed } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { isLoading } from '../shell/loading.js';
import RunTimeline from './RunTimeline.vue';
import {
  causeLabel,
  durationLabel,
  evidenceLabel,
  headlineFor,
  nextStepFor,
  statusMeta,
} from './diagnostic-labels.js';
import { diagnosticStore, loadOperationDiagnosis } from './diagnostics.js';
import { openEvidence } from './diagnostic-actions.js';

const run = computed(() => diagnosticStore.currentRun);
const kind = computed(() => String(diagnosticStore.active?.kind || 'restore'));
const meta = computed(() => statusMeta(run.value?.status));
const headline = computed(() => headlineFor(run.value || {}));

// 「完成 N 个阶段」这个措辞对两种 kind 都成立：它们的时间线都是若干条阶段
// 事件，不需要按 kind 分叉。
const stageCount = computed(() => (run.value?.events || []).length);
const emptyText = computed(() =>
  kind.value === 'bisect' ? '还没有排查诊断记录' : '还没有恢复诊断记录'
);

const startedAt = computed(() => Number(run.value?.startedAtMs || 0));
const finishedAt = computed(() => Number(run.value?.finishedAtMs || 0));
const cost = computed(() => {
  if (!startedAt.value) return '';
  return durationLabel((finishedAt.value || Date.now()) - startedAt.value);
});

const evidence = computed(() => run.value?.evidence || {});
const nextStep = computed(() => nextStepFor(run.value || {}));
const hasRun = computed(() => !!run.value);

function reload() {
  // 按 id 拉而不是「最近一条」：用户可能正在看一条历史记录，点刷新却换成
  // 最近那条，看到的就是另一件事了。
  return loadOperationDiagnosis(run.value?.id || diagnosticStore.active?.runId, kind.value, true);
}
</script>

<template>
  <div class="diag-card">
    <div class="diag-status">
      <span class="diag-status__dot" :class="`diag-status__dot--${meta.tone}`" aria-hidden="true"></span>
      <span>{{ headline }}</span>
      <span v-if="causeLabel(run?.cause)" class="diag-card__aside">
        {{ causeLabel(run?.cause) }}
      </span>
    </div>
    <p class="diag-meta">
      <template v-if="hasRun">
        {{ run?.kernelVersion || '未知版本' }} · 实例 {{ run?.instanceId }} · 耗时
        {{ cost || '未知' }}
        <template v-if="stageCount"> · 共 {{ stageCount }} 条记录</template>
      </template>
      <template v-else>{{ emptyText }}</template>
    </p>
    <p v-if="run?.summary" class="diag-summary-text">{{ run.summary }}</p>
    <p v-if="nextStep" class="diag-next">{{ nextStep }}</p>
  </div>

  <p v-if="diagnosticStore.error" class="diag-error">{{ diagnosticStore.error }}</p>

  <div class="diag-card">
    <h3 class="diag-card__title">
      <span>{{ kind === 'bisect' ? '排查过程' : '恢复过程' }}</span>
      <span class="diag-card__aside">共 {{ stageCount }} 个阶段</span>
    </h3>
    <RunTimeline>
      <template #row-actions="{ event }">
        <el-button
          v-if="event && event.status === 'failure'"
          text
          size="small"
          :icon="Refresh"
          @click="openEvidence()"
        >
          查看日志
        </el-button>
      </template>
    </RunTimeline>
  </div>

  <div v-if="evidence.kernelLog" class="diag-card">
    <h3 class="diag-card__title"><span>证据</span></h3>
    <p class="diag-meta">内核日志：{{ evidenceLabel(evidence.kernelLog) }}</p>
    <p v-if="evidence.sandboxLog" class="diag-meta">
      沙盒日志：{{ evidenceLabel(evidence.sandboxLog) }}
    </p>
  </div>

  <div class="diag-actions">
    <el-button :icon="Refresh" :loading="isLoading('operationDiagnosisReload')" @click="reload">
      刷新
    </el-button>
    <el-button v-if="hasRun" @click="openEvidence()">查看完整日志</el-button>
  </div>
</template>