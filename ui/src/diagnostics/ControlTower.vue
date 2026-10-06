<script setup>
// 概览页控制塔：三块只读信息 + 诊断入口。
//
// **为什么单独一个组件**：概览页已经 900 行，而这三块与「当前内核」卡片
// 是并列关系而不是它的细节。塞进去只会让那个文件更难读，而它本来就
// 在代码预算的反棘轮上（只许越来越小）。
//
// 三块各自的取舍：
// - **需要关注**：只在真有东西要说时出现。空列表不占位置。
// - **系统健康**：只给读数，不给按钮。它回答「能不能跑」，处置在各自面板。
// - **最近操作**：回答「上次发生了什么」。失败 / 告警才可点进诊断；
//   成功记录不给按钮——它没什么可诊断的。
import { computed } from 'vue';
import { store } from '../store.js';
import { entryTimeLabel, snapshotStore } from './snapshots.js';
import { incidentCauseLabel, incidentTitle } from '../incidents/incidents.js';
import { openStartupDiagnosis, diagnosticStore, loadRecentRuns } from './diagnostics.js';
import { causeLabel, durationLabel, statusMeta } from './diagnostic-labels.js';

const emit = defineEmits(['open-incident', 'go-panel']);

const kernel = computed(() => store.view?.kernel || {});
const running = computed(() => !!kernel.value.running);
const node = computed(() => store.view?.node || {});
const nodeOk = computed(() => !!node.value.ok);
const incident = computed(() => store.view?.incident || null);

/**
 * 需要关注：只有真有东西才列。
 *
 * 判据全部来自 store 已有状态，**不做任何探测**——概览是用户随手看的
 * 页面，一进来就发起三次网络往返会让它在慢机器上明显卡顿。
 */
const attention = computed(() => {
  const list = [];
  if (incident.value) {
    list.push({
      key: 'incident',
      label: incidentTitle(incident.value),
      detail: incidentCauseLabel(incident.value),
      tone: 'bad',
      action: 'incident',
    });
  }
  if (store.settingsWarning) {
    list.push({
      key: 'settings',
      label: '设置无法读取，已回退到默认值',
      detail: store.settingsWarning,
      tone: 'warn',
      action: 'settings',
    });
  }
  if (kernel.value.other_shell_workbench) {
    list.push({
      key: 'other-shell',
      label: '另一个桌面端的工作台正在运行',
      detail: '装 / 删内核与切换端口会影响它。',
      tone: 'warn',
      action: 'other-shell',
    });
  }
  if (!nodeOk.value) {
    list.push({
      key: 'node',
      label: '未找到满足要求的 Node.js',
      detail: node.value.reason || '启动工作台前需要先准备 Node.js 环境。',
      tone: 'bad',
      action: 'node',
    });
  }
  return list;
});

/**
 * 最近快照的时间。
 *
 * **只在有 `lastKnownGood` 时显示**：随便挑一个最新的快照当"最近恢复点"
 * 会指向一个从来没被验证过的组合——恢复入口应该指向真正确认能跑的那次。
 */
const latestSnapshotLabel = computed(() => {
  const view = snapshotStore.view;
  if (!view || !view.lastKnownGoodId) return '';
  const entry = (view.entries || []).find((item) => item.id === view.lastKnownGoodId);
  return entry ? entryTimeLabel(entry) : '';
});

/** 系统健康：只给读数。 */
const health = computed(() => [
  { key: 'kernel', label: '内核', value: kernel.value.active || '未安装', tone: kernel.value.active ? '' : 'warn' },
  {
    key: 'runtime',
    label: '运行时',
    value: running.value ? '运行中' : '已停止',
    tone: running.value ? 'ok' : '',
  },
  { key: 'node', label: 'Node.js', value: nodeOk.value ? '就绪' : '未就绪', tone: nodeOk.value ? 'ok' : 'bad' },
  {
    key: 'port',
    label: '端口',
    value: kernel.value.port != null ? String(kernel.value.port) : '未设置',
  },
  {
    key: 'snapshot',
    label: '最近快照',
    value: latestSnapshotLabel.value || '暂无',
  },
]);

/** 最近操作：最近一条运行记录；失败 / 告警时可点进诊断。 */
const latestRun = computed(() => diagnosticStore.recentRuns[0] || null);

const runMeta = computed(() => statusMeta(latestRun.value?.status));
const runDetail = computed(() => {
  const run = latestRun.value;
  if (!run) return '';
  const parts = [];
  const cause = causeLabel(run.cause);
  if (cause) parts.push(cause);
  if (run.summary) parts.push(run.summary);
  else if (run.eventCount) parts.push(`${run.eventCount} 条阶段记录`);
  return parts.join(' · ');
});

// 只有失败 / 告警 / 未能完成才可点：成功的记录没有可诊断的内容，
// 给个按钮只会让用户点进去看到一句「一切正常」。
const runActionable = computed(() =>
  ['failure', 'warning', 'inconclusive'].includes(String(latestRun.value?.status || ''))
);

function onAttention(item) {
  if (item.action === 'incident') emit('open-incident');
  else if (item.action === 'settings') emit('go-panel', 'settings');
  else if (item.action === 'node') emit('go-panel', 'settings');
  else emit('go-panel', 'overview');
}

function openDiagnosis() {
  openStartupDiagnosis(latestRun.value?.id || '', 'overview');
  loadRecentRuns(null);
}
</script>

<template>
  <div v-if="attention.length" class="diag-card">
    <h3 class="diag-card__title">
      <span>需要关注</span>
      <span class="diag-card__aside">{{ attention.length }} 项</span>
    </h3>
    <div class="diag-rows">
      <button
        v-for="item in attention"
        :key="item.key"
        type="button"
        class="diag-row"
        @click="onAttention(item)"
      >
        <span class="diag-row__label">
          {{ item.label }}
          <span v-if="item.detail" class="diag-row__value">· {{ item.detail }}</span>
        </span>
        <span class="diag-row__value" :class="`diag-row__value--${item.tone}`">处理</span>
        <span class="diag-row__arrow" aria-hidden="true">›</span>
      </button>
    </div>
  </div>

  <div class="diag-card">
    <h3 class="diag-card__title"><span>系统健康</span></h3>
    <div class="diag-rows">
      <div v-for="row in health" :key="row.key" class="diag-row diag-row--static">
        <span class="diag-row__label">{{ row.label }}</span>
        <span class="diag-row__value" :class="row.tone ? `diag-row__value--${row.tone}` : ''">
          {{ row.value }}
        </span>
      </div>
    </div>
  </div>

  <div class="diag-card">
    <h3 class="diag-card__title">
      <span>最近操作</span>
      <span class="diag-card__aside">启动 / 预检 / 恢复</span>
    </h3>
    <div v-if="latestRun" class="diag-rows">
      <button
        type="button"
        class="diag-row"
        :class="{ 'diag-row--static': !runActionable }"
        @click="runActionable && openDiagnosis()"
      >
        <span class="diag-row__label">
          {{ runMeta.label }}
          <span v-if="runDetail" class="diag-row__value">· {{ runDetail }}</span>
        </span>
        <span v-if="runActionable" class="diag-row__arrow" aria-hidden="true">›</span>
      </button>
    </div>
    <p v-else class="diag-empty">还没有运行记录。启动一次工作台后，这里会显示上次的结果。</p>
  </div>
</template>