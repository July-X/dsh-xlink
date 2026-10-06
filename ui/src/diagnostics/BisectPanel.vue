<script setup>
// 排查面板（安全网 P2）：把二分定位的每一步摊开。
//
// 用户最怕的不是"排查失败"，是"卡住不动"——每一轮都要真的启动一次临时
// 内核，全程可能几十秒。所以这里刻意逐轮显示：现在在试哪一半、上一轮结果、
// 已排除多少、预计还要几轮。
//
// 结论只有三种。**没有「找到根因」**——最小坏集合只是"能解释现象的最小
// 组合"，组合效应仍可能参与；把它说成根因会让用户去卸一个无辜的插件。
import { computed, onMounted } from 'vue';
import { InfoFilled, Search, CircleClose } from '@element-plus/icons-vue';
import {
  bisectStore,
  loadBisect,
  startBisect,
  abortBisect,
  conclusionView,
  bisectHeadline,
  stepTitle,
  stepClassName,
} from './bisect.js';
import { globalBusy, isLoading } from '../shell/loading.js';
import { store, workbenchActiveNow } from '../store.js';
import { openOperationDiagnosis } from './diagnostics.js';

const view = computed(() => bisectStore.view);
const conclusion = computed(() => conclusionView(view.value && view.value.conclusion));
const steps = computed(() => (view.value && view.value.steps) || []);
const running = computed(() => !!(view.value && view.value.running));

// 工作台运行时不做排查：那时环境正在被真实内核占用，排查的结论也会和
// 用户眼前的现象对不上。恢复早就用同一条理由拒绝，这里保持一致。
const workbenchRunning = computed(() => workbenchActiveNow());

// 候选不足 3 个时后端会拒绝；按钮提前置灰并说明原因，比点了再报错好。
const canStart = computed(() => {
  if (running.value) return false;
  if (workbenchRunning.value) return false;
  if (!view.value) return true; // 还没拉过：允许点，让后端给准确原因
  return (view.value.candidateCount || 0) >= 3 || (view.value.cleared || []).length > 0;
});

const startHint = computed(() => {
  if (workbenchRunning.value) return '工作台正在运行，请先关闭工作台再排查';
  if (running.value) return '排查进行中';
  return '按嫌疑度逐轮缩小可疑范围';
});

function onStart() {
  return startBisect();
}

/**
 * 打开这次排查的运行记录。
 *
 * 只在**有结论之后**给入口：排查跑的过程中时间线还少一半，这时候把它推给
 * 用户，看到的是一条看不出所以然的半截记录。面板里那份实时视图继续负责
 * 「进行中」，诊断层负责「回头复盘」。
 */
function openRun() {
  return openOperationDiagnosis('bisect', store.activePanel);
}

onMounted(() => {
  if (!view.value) {
    loadBisect();
  }
});
</script>

<template>
  <div class="card bisect-card">
    <div class="card-head">
      <h2>
        深入排查
        <el-tooltip placement="bottom-start" :show-after="80">
          <template #content>
            <div class="card-info-tooltip">
              工作台起不来时，这里用二分法把范围缩到「能解释现象的最小集合」：
              按嫌疑度排序后每次启用一半，看内核起不来就排除另一半，
              直到剩下的就是可疑的那几个。
              <br />
              它给出的**不是根因**——组合效应（两个扩展单独都正常、一起就炸）
              会让二分停在一个不可修的答案上。每一轮都会真的启动一次临时内核，
              整个过程可能需要几分钟。
            </div>
          </template>
          <el-icon class="card-info-icon"><InfoFilled /></el-icon>
        </el-tooltip>
      </h2>
      <span class="bisect-head-actions">
        <el-button
          v-if="running"
          round
          size="small"
          :icon="CircleClose"
          :loading="isLoading('bisectAbort')"
          :disabled="globalBusy"
          title="停止排查，已排除的结果会保留"
          @click="abortBisect"
        >
          停止
        </el-button>
        <el-button
          v-else
          round
          size="small"
          type="primary"
          :icon="Search"
          :loading="isLoading('bisectRun')"
          :disabled="!canStart || globalBusy"
          :title="startHint"
          @click="onStart"
        >
          {{ view && (view.cleared || []).length ? '继续排查' : '开始排查' }}
        </el-button>
      </span>
    </div>

    <p v-if="!view" class="muted" style="margin: 0">正在读取排查记录…</p>
    <template v-else>
      <p class="bisect-headline">{{ bisectHeadline(view) }}</p>

      <el-alert
        v-if="view.conclusion"
        :title="conclusion.label + (conclusion.members.length ? '：' + conclusion.members.join('、') : '')"
        :type="conclusion.type"
        :closable="false"
        show-icon
        class="bisect-alert"
      />

      <p v-if="(view.cleared || []).length" class="bisect-cleared">
        已排除 {{ view.cleared.length }} 个：{{ view.cleared.join('、') }}
      </p>

      <details v-if="steps.length" class="bisect-steps">
        <summary>排查过程（{{ steps.length }} 轮）</summary>
        <ol class="bisect-step-list">
          <li v-for="step in steps" :key="step.round" :class="stepClassName(step)">
            <span class="bisect-step-title">{{ stepTitle(step) }}</span>
            <pre v-if="step.evidence" class="bisect-step-evidence">{{ step.evidence }}</pre>
          </li>
        </ol>
      </details>

      <!-- 排查过程面板已经能复盘每一轮，所以这个入口只补一件事：把同一场
           排查带进诊断层，和启动 / 预检的记录放在同一套口径下看。 -->
      <el-button v-if="view.conclusion" size="small" @click="openRun">查看排查诊断</el-button>
    </template>
  </div>
</template>

<style scoped>
.bisect-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.bisect-headline {
  margin: 0 0 10px;
  color: var(--text-muted);
  font-size: 13px;
  line-height: 1.6;
}

.bisect-alert {
  margin-bottom: 10px;
}

.bisect-cleared {
  margin: 0 0 10px;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.6;
  word-break: break-all;
}

.bisect-steps summary {
  cursor: pointer;
  font-size: 13px;
  color: var(--text-muted);
  margin-bottom: 8px;
}

.bisect-step-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.bisect-step-list li {
  padding: 7px 10px;
  border-radius: 8px;
  border-left: 3px solid var(--border);
  background: var(--bg-soft);
}

.bisect-step-list .step-fail {
  border-left-color: var(--bad);
}

.bisect-step-list .step-pass {
  border-left-color: var(--good);
}

.bisect-step-list .step-unknown {
  border-left-color: var(--warn);
}

.bisect-step-title {
  font-size: 13px;
  color: var(--text);
}

.bisect-step-evidence {
  margin: 6px 0 0;
  max-height: 120px;
  overflow: auto;
  font-size: 11.5px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--muted);
}
</style>
