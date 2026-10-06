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
  stageProgress,
  statusMeta,
} from './diagnostic-labels.js';
import { diagnosticStore, loadOperationDiagnosis } from './diagnostics.js';
import { openEvidence } from './diagnostic-actions.js';

const props = defineProps({
  /**
   * 通用模式：不按 kind 定制标题与空态文案。
   *
   * 用于「只有一条运行记录、没有别的东西可看」的场景——从控制塔进��的插件
   * 预检就是这样（报告只存在于刚才那次预检的返回值里）。那时按 kind 硬套
   * 「恢复诊断」或「排查诊断」都是错的，标题得说清这里看的是什么。
   */
  generic: { type: Boolean, default: false },
});

const run = computed(() => diagnosticStore.currentRun);
const kind = computed(() => String(diagnosticStore.active?.kind || 'restore'));
const meta = computed(() => statusMeta(run.value?.status));
const headline = computed(() => headlineFor(run.value || {}));

// 「完成 N / M 个阶段」按**阶段集合**统计，不是事件条数（审查 P2-03）：
// 一个阶段推三条事件时，用事件数算会显示成「完成 3 / 6」而实际只走了两步。
const progress = computed(() => stageProgress(run.value?.kind || kind.value, run.value?.events));

const stageTitle = computed(() => {
  if (props.generic) return '阶段时间线';
  return kind.value === 'bisect' ? '排查过程' : '恢复过程';
});

const emptyText = computed(() => {
  if (props.generic) return '还没有这条运行记录的详情';
  return kind.value === 'bisect' ? '还没有排查诊断记录' : '还没有恢复诊断记录';
});

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
        <template v-if="progress.total"> · 阶段 {{ progress.done }} / {{ progress.total }}</template>
      </template>
      <template v-else>{{ emptyText }}</template>
    </p>
    <p v-if="run?.summary" class="diag-summary-text">{{ run.summary }}</p>
    <p v-if="nextStep" class="diag-next">{{ nextStep }}</p>
  </div>

  <p v-if="diagnosticStore.error" class="diag-error">{{ diagnosticStore.error }}</p>

  <div class="diag-card">
    <h3 class="diag-card__title">
      <span>{{ stageTitle }}</span>
      <span class="diag-card__aside">完成 {{ progress.done }} / {{ progress.total }} 个阶段</span>
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