<script setup>
// 插件安全诊断：把预检的四段结论摊开给用户看。
//
// 这一层**不新增一次预检**，只呈现已有结论（`precheck::plugin_install`
// 已经把四段都跑完了，它的结果里有 runId 指向那条完整时间线）。
// 用户看到的是「当时发生了什么」，不是「现在再跑一遍会怎样」——后者
// 沙盒一次要几十秒，而用户在意的恰恰是他刚才那次安装。
import { computed } from 'vue';
import { showLogs } from '../logs/logs.js';
import { diagnosticStore } from './diagnostics.js';
import { causeLabel, durationLabel, evidenceLabel, nextStepFor } from './diagnostic-labels.js';

const spec = computed(() => diagnosticStore.active?.spec || {});
const report = computed(() => spec.value.report || null);
const run = computed(() => diagnosticStore.currentRun);

// 归因优先取运行记录（它是这次预检的权威归因），没有再退回报告里的
// verdict ——报告是为安装流程写的，没有归因概念。
const cause = computed(() => run.value?.cause || '');

const VERDICT_META = {
  pass: { label: '预检通过', tone: 'ok' },
  fail: { label: '预检未通过', tone: 'bad' },
  inconclusive: { label: '未能完成预检', tone: 'unknown' },
};

const verdictMeta = computed(() => {
  const verdict = String(report.value?.verdict || '');
  return VERDICT_META[verdict] || { label: verdict ? `未知结论（${verdict}）` : '未知结论', tone: 'unknown' };
});

const cost = computed(() => {
  if (report.value?.durationMs) return durationLabel(report.value.durationMs);
  return '';
});

const warnings = computed(() => report.value?.warnings || []);
const nextStep = computed(() => {
  // 报告自带 hint 时优先用它——那是预检写给用户的一句话，比通用的
  // 归因模板更贴合当前插件。
  return report.value?.hint || nextStepFor(run.value || { status: verdictMeta.value.tone === 'bad' ? 'failure' : '', cause: cause.value });
});

function viewLogs() {
  showLogs();
}
</script>

<template>
  <div class="diag-card">
    <div class="diag-status">
      <span class="diag-status__dot" :class="`diag-status__dot--${verdictMeta.tone}`" aria-hidden="true"></span>
      <span>{{ verdictMeta.label }}</span>
      <span v-if="causeLabel(cause)" class="diag-card__aside">{{ causeLabel(cause) }}</span>
    </div>
    <p class="diag-meta">
      {{ spec.name || spec.id || '未知插件' }}<template v-if="cost"> · 预检耗时 {{ cost }}</template>
    </p>
    <p v-if="report?.summary" class="diag-summary-text">{{ report.summary }}</p>
    <p v-if="nextStep" class="diag-next">{{ nextStep }}</p>
    <p v-if="report?.installed" class="diag-meta">
      这个插件已装到当前实例（预检只保证沙盒里能起来，不保证和你的其他插件不冲突）。
    </p>
  </div>

  <div v-if="warnings.length" class="diag-card">
    <h3 class="diag-card__title">
      <span>通过但有告警</span>
      <span class="diag-card__aside">{{ warnings.length }} 条</span>
    </h3>
    <ul class="diag-timeline">
      <li v-for="(w, i) in warnings" :key="i" class="diag-timeline__row">
        <span class="diag-timeline__mark diag-timeline__mark--warn" aria-hidden="true">!</span>
        <div class="diag-timeline__main">
          <p class="diag-timeline__message">{{ w }}</p>
        </div>
      </li>
    </ul>
  </div>

  <div class="diag-card">
    <h3 class="diag-card__title"><span>沙盒结论</span></h3>
    <p v-if="report?.evidence" class="diag-meta">{{ report.evidence }}</p>
    <p v-else class="diag-meta">这次预检没有留下内核侧证据。</p>
    <p v-if="report?.evidencePath" class="diag-meta">
      证据日志：{{ evidenceLabel(report.evidencePath) }}
    </p>
  </div>

  <div class="diag-actions">
    <el-button @click="viewLogs">查看日志</el-button>
  </div>
</template>