<script setup>
// 排查面板（安全网 P2）：把二分定位的每一步摊开。
//
// 设计稿 draft 2844 里它是「环境回退与诊断」那张卡的一行，与环境回退点同级。
// 本组件渲染成 Fragment：一行常驻，逐轮明细是兄弟节点。
//
// 用户最怕的不是"排查失败"，是"卡住不动"——每一轮都要真的启动一次临时
// 内核，全程可能几十秒。所以运行时那一行直接把「停止」摆在明面上，而不是
// 一起收进展开层。
//
// 结论只有三种。**没有「找到根因」**——最小坏集合只是"能解释现象的最小
// 组合"，组合效应仍可能参与；把它说成根因会让用户去卸一个无辜的插件。
import { computed, onMounted, ref } from 'vue';
import { ArrowDown, ArrowUp, InfoFilled, Search, CircleClose } from '@element-plus/icons-vue';
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
// 这一行与结论一样在展开闸门之外常驻（"排除了哪些"是排查进行中唯一能看懂的
// 进度），所以先把可能还没拉到的 view 挡在外面，别在模板里写 `view && view.cleared`。
const cleared = computed(() => (view.value && view.value.cleared) || []);

// 工作台运行时不做排查：那时环境正在被真实内核占用，排查的结论也会和
// 用户眼前的现象对不上。恢复早就用同一条理由拒绝，这里保持一致。
const workbenchRunning = computed(() => workbenchActiveNow());

// 候选不足 3 个时后端会拒绝；按钮提前置灰并说明原因，比点了再报错好。
const canStart = computed(() => {
  if (running.value) return false;
  if (workbenchRunning.value) return false;
  if (!view.value) return true; // 还没拉过：允许点，让后端给准确原因
  return (view.value.candidateCount || 0) >= 3 || cleared.value.length > 0;
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

// 逐轮明细默认收起。**收起不等于藏起来**：进度、结论、已排除哪些都在闸门之外
// 常驻——排查跑几分钟，用户需要的恰恰是随时知道「还在跑 / 排除了什么」。
const open = ref(false);

onMounted(() => {
  if (!view.value) {
    loadBisect();
  }
});
</script>

<template>
  <div class="page-list-row">
    <div class="page-list-main">
      <h3 class="page-list-title">
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
      </h3>
      <p class="page-list-meta">{{ view ? bisectHeadline(view) : '正在读取…' }}</p>
    </div>
    <div class="page-list-actions">
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
        {{ cleared.length ? '继续排查' : '开始排查' }}
      </el-button>
      <el-button
        round
        size="small"
        :icon="open ? ArrowUp : ArrowDown"
        :title="open ? '收起逐轮明细' : '展开逐轮明细'"
        @click="open = !open"
      >
        {{ open ? '收起' : '查看' }}
      </el-button>
    </div>
  </div>

  <el-alert
    v-if="view && view.conclusion"
    :title="conclusion.label + (conclusion.members.length ? '：' + conclusion.members.join('、') : '')"
    :type="conclusion.type"
    :closable="false"
    show-icon
    class="bisect-alert"
  />

  <p v-if="cleared.length" class="bisect-cleared">
    已排除 {{ cleared.length }} 个：{{ cleared.join('、') }}
  </p>

  <details v-if="open && steps.length" class="bisect-steps">
    <summary>排查过程（{{ steps.length }} 轮）</summary>
    <ol class="bisect-step-list">
      <li v-for="step in steps" :key="step.round" :class="stepClassName(step)">
        <span class="bisect-step-title">{{ stepTitle(step) }}</span>
        <pre v-if="step.evidence" class="bisect-step-evidence">{{ step.evidence }}</pre>
      </li>
    </ol>
  </details>

  <!-- 排查过程面板已经能复盘每一轮，所以这个入口只补一件事：把同一场
       排查带进诊断层，和启动 / 预检的记录放在同一套口径下看。它跟着
       逐轮明细一起收起——结论本身在闸门之外常驻，用户不会以为没跑过。 -->
  <el-button v-if="open && view && view.conclusion" size="small" @click="openRun">
    查看排查诊断
  </el-button>
</template>

<style scoped>
.bisect-cleared {
  margin: 0;
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
  background: var(--surface-subtle);
}

.bisect-step-list .step-fail {
  border-left-color: var(--danger);
}

.bisect-step-list .step-pass {
  border-left-color: var(--success);
}

.bisect-step-list .step-unknown {
  border-left-color: var(--warning);
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
  color: var(--text-secondary);
}
</style>
