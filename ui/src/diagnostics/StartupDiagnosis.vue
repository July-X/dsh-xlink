<script setup>
// 启动诊断：回答「这次启动停在哪一步，用户接下来能做什么」。
//
// 它**不是第二个日志查看器**，也不替代事故面板的插件处置动作。分工是：
// 这里给时间线、证据索引和下一步；事故面板负责处置嫌疑插件、恢复操作。
import { computed } from 'vue';
import { Refresh } from '@element-plus/icons-vue';
import { openHarnessWindow, showIncident, store } from '../store.js';
import { globalBusy, isLoading } from '../shell/loading.js';
import RunTimeline from './RunTimeline.vue';
import {
  causeLabel,
  durationLabel,
  evidenceLabel,
  isRetryable,
  nextStepFor,
  RUN_HEADLINE,
  statusMeta,
} from './diagnostic-labels.js';
import { diagnosticStore, getLastRunId, loadStartupDiagnosis } from './diagnostics.js';
import { openEvidence, startStartupDiagnosis } from './diagnostic-actions.js';

const run = computed(() => diagnosticStore.currentRun);
const meta = computed(() => statusMeta(run.value?.status));
// 卡片头部用整句而不是「失败」这样的短标签：那一行是用户第一眼读的
// 内容，写成「失败」等于只给了一个标签，没告诉他「什么没能启动」。
const headline = computed(() => {
  const key = String(run.value?.status || '');
  return RUN_HEADLINE[key] || meta.value.label;
});

// 事故是否还在展示。带事故启动（安全模式）时，用户要能从这里跳回事故
// 面板去看被处置的插件——启动诊断只给时间线，不重复那份处置动作。
const incident = computed(() => store.lastIncident || null);

// 「有记录 / 运行中」两种状态都还要把事件流读回来：诊断层可能在运行
// 期间才被打开，此时落盘记录尚未落定，实时流才是最新的那份。
const finishedAt = computed(() => Number(run.value?.finishedAtMs || 0));
const startedAt = computed(() => Number(run.value?.startedAtMs || 0));
const cost = computed(() => {
  if (!startedAt.value) return '';
  const end = finishedAt.value || Date.now();
  return durationLabel(end - startedAt.value);
});

const evidence = computed(() => run.value?.evidence || {});
const nextStep = computed(() => nextStepFor(run.value || {}));

// 运行中不放「重试启动」：用户正在启动中，再点一次只会排队或撞上
// 生命周期锁。按钮在运行期间换成只读提示，不隐藏——隐藏会让用户以为
// 页面坏了。
const running = computed(() => meta.value.raw === 'running' || store.starting);
// `inconclusive` 也给重试：证据不足时「再来一次」是唯一能拿到新证据的
// 办法。它不是失败，但用户确实需要一个能推进的手势。
const canRetry = computed(() => isRetryable(run.value?.status));

const hasRun = computed(() => !!run.value);

function reload() {
  // 优先按 id 拉，不按「最近一条」：用户可能正在看一条**历史**记录，
  // 此时点刷新若换成最近那条，看到的就是另一件事。
  return loadStartupDiagnosis(run.value?.id || getLastRunId(), true);
}

function viewLogs() {
  // 走诊断层的 `openEvidence`（设计 §8.2）：证据路径记在诊断状态里，
  // 日志读取仍复用日志模块——诊断层自己读文件会让两处的截断与分类漂移。
  openEvidence();
}

function openIncident() {
  showIncident(incident.value, { force: true });
}

/**
 * 主动作：成功 → 打开工作台，失败 → 重试启动。
 *
 * 设计 §5.2「顶部只保留返回和一个主要动作」：用户此刻要的不是「再诊断
 * 一次」而是「去看那个跑起来的工作台」。运行中不给主动作（诊断页的
 * 返回就是全部）。
 */
const workbenchOpen = computed(() => store.view?.kernel?.running);
function openWorkbench() {
  openHarnessWindow();
}

async function retry() {
  // 走诊断层的 `startStartupDiagnosis`（设计 §8.2）：它复用 store 的启动
  // 编排，但**用诊断层自己的 loading key**，所以按钮转的是这一页的那个。
  await startStartupDiagnosis();
  await reload();
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
        {{ run?.kernelVersion || '未知版本' }} · 实例 {{ run?.instanceId }} ·
        耗时 {{ cost || '未知' }}
        <template v-if="run?.eventCount"> · 共 {{ run?.eventCount }} 条记录</template>
      </template>
      <template v-else>还没有启动诊断记录</template>
    </p>
    <p v-if="run?.summary" class="diag-summary-text">{{ run.summary }}</p>
    <p v-if="nextStep" class="diag-next">{{ nextStep }}</p>
  </div>

  <p v-if="diagnosticStore.error" class="diag-error">{{ diagnosticStore.error }}</p>

  <div class="diag-card">
    <h3 class="diag-card__title">
      <span>启动阶段</span>
      <span class="diag-card__aside">完成 {{ hasRun ? '部分' : '0' }} 个阶段</span>
    </h3>
    <RunTimeline>
      <template #row-actions="{ event }">
        <el-button
          v-if="event && event.status === 'failure'"
          text
          size="small"
          :icon="Refresh"
          @click="viewLogs"
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
    <el-button :icon="Refresh" :loading="isLoading('startupDiagnosisReload')" @click="reload">
      刷新
    </el-button>
    <el-button v-if="hasRun" @click="viewLogs">查看完整日志</el-button>
    <!-- 事故面板负责处置嫌疑插件，启动诊断只给入口不重复那份动作。 -->
    <el-button v-if="incident" @click="openIncident">查看事故</el-button>
    <!-- 主动作只有一个（设计 §5.2）：成功去看工作台，失败再来一次。
         两个都放会让用户在最需要做决定的时刻多一次分辨。 -->
    <el-button
      v-if="!running && !workbenchOpen && canRetry"
      type="primary"
      :loading="isLoading('startupDiagnosisRetry')"
      @click="retry"
    >
      重试启动
    </el-button>
    <el-button
      v-else-if="workbenchOpen"
      type="primary"
      :disabled="globalBusy"
      @click="openWorkbench"
    >
      打开工作台
    </el-button>
  </div>
</template>