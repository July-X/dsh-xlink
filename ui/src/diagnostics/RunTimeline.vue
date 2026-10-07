<script setup>
// 启动与预检共用的阶段时间线。
//
// 三条不能破的规则：
// ① **按 seq 排序，不按字符串排**。后端给的是数字 seq，同一毫秒内靠它
//    定序；按字符串排会把 10 排到 2 前面，时间线在刷新时会跳。
// ② **默认只展开第一个失败阶段**。六个阶段全展开会把说明一次性推下去，
//    用户反而找不到出问题的那一行。
// ③ **状态不能只靠颜色**。每个阶段都带文字（`statusLabel`），失败行的
//    说明文字本身就是结论。
import { computed, ref, watch } from 'vue';
import { Check, Close, Loading } from '@element-plus/icons-vue';
import {
  collapseSupersededRunning,
  durationLabel,
  firstFailedEvent,
  stageLabel,
  statusMeta,
} from './diagnostic-labels.js';
import { visibleEvents } from './diagnostics.js';

const props = defineProps({
  /** 是否允许展开某一行（默认只展开失败行）。 */
  expandable: { type: Boolean, default: true },
});

// **必须经 `collapseSupersededRunning`**：直接渲染 `visibleEvents()` 会让每个
// 阶段的「进行中」行永久留着——它记录的是阶段开始，而那不是当前状态。四个
// 诊断窗口共用这一个组件，这条折叠是它们共同的前端契约。
const events = computed(() => collapseSupersededRunning(visibleEvents()));

// 默认展开项：第一个失败阶段。数据换了（用户点了刷新、或实时流接管）
// 就重新选一次——展开项跟着旧的失败阶段不动，会让新记录看起来「什么都没发生」。
const failedSeq = computed(() => {
  const failed = firstFailedEvent(events.value);
  return failed ? Number(failed.seq) : null;
});

const expanded = ref(new Set());

watch(
  events,
  (list) => {
    const target = new Set();
    const failed = firstFailedEvent(list);
    if (failed) target.add(Number(failed.seq));
    expanded.value = target;
  },
  { immediate: true }
);

function isExpanded(seq) {
  return expanded.value.has(Number(seq));
}

function toggle(row) {
  if (!props.expandable) return;
  const seq = Number(row.seq);
  const next = new Set(expanded.value);
  if (next.has(seq)) next.delete(seq);
  else next.add(seq);
  expanded.value = next;
}

const MARK = { ok: Check, bad: Close, warn: Close, active: Loading, unknown: '?', muted: '·' };

function markOf(status) {
  return MARK[statusMeta(status).tone] || '·';
}

function stageOf(event) {
  return stageLabel(event.stage);
}

function statusOf(event) {
  return statusMeta(event.status);
}
</script>

<template>
  <ul class="diag-timeline">
    <li v-for="(event, index) in events" :key="event.seq" class="diag-timeline__row">
      <span
        class="diag-timeline__mark"
        :class="`diag-timeline__mark--${statusOf(event).tone}`"
        :aria-hidden="true"
      >
        <el-icon v-if="typeof markOf(event.status) !== 'string'">
          <component :is="markOf(event.status)" />
        </el-icon>
        <template v-else>{{ markOf(event.status) }}</template>
      </span>
      <div class="diag-timeline__main">
        <div class="diag-timeline__stage">
          <span>
            {{ index + 1 }}. {{ stageOf(event) }}
            <!-- 看护重试的「第 N 次尝试」。少了它，三次尝试会被读成一条
                 连续流程，用户看到「成功了」却不知道自己前面失败过。 -->
            <span v-if="event.attempt && event.attempt > 1" class="diag-timeline__attempt">
              第 {{ event.attempt }} 次尝试
            </span>
            <span class="diag-timeline__attempt"> · {{ statusOf(event).label }}</span>
          </span>
          <span v-if="event.durationMs" class="diag-timeline__cost">
            {{ durationLabel(event.durationMs) }}
          </span>
        </div>
        <p v-if="event.message" class="diag-timeline__message">{{ event.message }}</p>
        <div v-if="expandable && isExpanded(event.seq)" class="diag-timeline__actions">
          <slot name="row-actions" :event="event" />
        </div>
      </div>
    </li>
    <li v-if="!events.length" class="diag-empty">还没有阶段记录</li>
  </ul>
</template>