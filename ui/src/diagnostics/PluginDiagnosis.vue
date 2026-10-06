<script setup>
// 插件安全诊断：回答「这个候选插件是什么、预检做了什么、变更能不能退」。
//
// 这一层**不重跑预检**。用户在意的是他刚才那次安装，沙盒一次要几十秒，
// 而重跑得到的结论不会因为他多看一眼就改变。所以呈现的是 `report` 里
// 已有的四段结论，加上本次运行记录（`runId` 指向）里的阶段时间线。
//
// 三条不能破的规则（设计 §6.4 / §6.5）：
// ① **三态不可压成两态。** inconclusive 画成「失败」会让用户去卸一个无辜
//    的包——基线都没起来时，候选插件根本没被测过。
// ② **「通过但有告警」不得被压缩成普通「通过」。** 那条告警往往正是
//    「白屏前最后一次正常启动里也出现了」的唯一线索。
// ③ **首屏不显示凭据、会话正文与环境变量。** 折叠区里也只给路径与摘要。
import { computed, onMounted } from 'vue';
import { openEvidence, restorePreChange } from './diagnostic-actions.js';
import { diagnosticStore, loadPluginRun } from './diagnostics.js';
import { causeLabel, durationLabel, evidenceLabel, statusMeta } from './diagnostic-labels.js';
import { INTEGRITY_META, SOURCE_LABELS } from './precheck-labels.js';
import RunTimeline from './RunTimeline.vue';

const spec = computed(() => diagnosticStore.active?.spec || {});
const report = computed(() => spec.value.report || null);
const run = computed(() => diagnosticStore.currentRun);

const cause = computed(() => run.value?.cause || '');

/**
 * 预检三态 → 展示元数据。
 *
 * 颜色只是辅助——每一态都带整句文案（设计 §6.4 的表），色弱用户与截图
 * 都能读出结论。
 */
const VERDICT_META = {
  pass: { label: '预检通过', tone: 'ok' },
  fail: { label: '预检未通过', tone: 'bad' },
  inconclusive: { label: '预检未能完成', tone: 'unknown' },
};

const verdictMeta = computed(() => {
  const verdict = String(report.value?.verdict || '');
  return (
    VERDICT_META[verdict] || { label: verdict ? `未知结论（${verdict}）` : '未知结论', tone: 'unknown' }
  );
});

const warnings = computed(() => report.value?.warnings || []);
const hasWarnings = computed(() => warnings.value.length > 0);

const cost = computed(() =>
  report.value?.durationMs ? durationLabel(report.value.durationMs) : ''
);

/** 候选信息卡：设计 §6.5 要求首屏五项。 */
const sourceKind = computed(() => String(report.value?.sourceKind || ''));
const sourceLabel = computed(() => {
  const label = String(report.value?.sourceLabel || '');
  // 空值如实显示「来源未知」——**不猜**。猜错来源比不显示危险得多：
  // 用户会以为装的是官方包。
  return label || '来源未知';
});
const sourceText = computed(
  () => SOURCE_LABELS[sourceKind.value] || (sourceKind.value ? `未知来源（${sourceKind.value}）` : '来源未知')
);

const integrity = computed(() => {
  const key = String(report.value?.integrity || 'none');
  return (
    INTEGRITY_META[key] || { label: `未知的完整性状态（${key}）`, tone: 'unknown', weak: true }
  );
});

const materialize = computed(() =>
  report.value?.materialize === 'link' ? '链接（link）' : report.value?.materialize === 'copy' ? '复制（copy）' : '未知'
);

const instanceText = computed(() => {
  const name = report.value?.targetInstance || '';
  if (!name) return '未知实例';
  // 是否影响默认实例必须说清：预检会改接线，用户要知道自己动的是不是
  // 每天在用的那个实例。
  return report.value?.affectsDefaultInstance
    ? `${name}（默认实例）`
    : `${name}（非默认实例）`;
});

// 四段实验流程的静态清单。**已完成的阶段靠运行记录的事件判断**，
// 这样「建沙盒 / 基线 / 装候选 / 探测」四行在报告被裁剪后仍然可读。
const STEPS = [
  { key: 'sandbox-create', label: '创建沙盒环境' },
  { key: 'baseline', label: '建立基线' },
  { key: 'install-candidate', label: '安装候选插件' },
  { key: 'probe-candidate', label: '启动沙盒并探测' },
  { key: 'report', label: '生成预检报告' },
];

const stages = computed(() =>
  STEPS.map((step) => {
    const events = (run.value?.events || []).filter((event) => event.stage === step.key);
    const last = events[events.length - 1] || null;
    return { ...step, events, status: last ? last.status : 'pending' };
  })
);

const STEP_META = {
  running: { label: '进行中', tone: 'active', mark: '◉' },
  success: { label: '已完成', tone: 'ok', mark: '✓' },
  warning: { label: '有告警', tone: 'warn', mark: '!' },
  failure: { label: '失败', tone: 'bad', mark: '×' },
  inconclusive: { label: '未完成', tone: 'unknown', mark: '?' },
  canceled: { label: '已取消', tone: 'muted', mark: '·' },
  // 从未发生。画成「未执行」而不是「失败」——没跑过不等于失败。
  pending: { label: '未执行', tone: 'muted', mark: '○' },
};

const stepMeta = (status) =>
  STEP_META[status] || { label: `未知状态（${status}）`, tone: 'unknown', mark: '?' };

/** 折叠区：依赖、安装脚本、证据路径。设计 §6.5 列的四项。 */
const risks = computed(() => {
  const list = [];
  if (report.value?.integrity === 'none') {
    list.push('这份包没有可用的完整性摘要，无法确认下载内容与 registry 声明一致。');
  }
  if (report.value?.evidencePath) {
    list.push(`沙盒日志：${evidenceLabel(report.value.evidencePath)}`);
  }
  if (report.value?.materialize === 'link') {
    list.push('链接模式：中央库里的源码改动会立刻反映到该实例，不需要重装。');
  }
  return list;
});

function viewLogs() {
  // 沙盒日志是这次预检的**主证据**，路径一起传过去：诊断层记下它，
  // 日志读取仍走日志模块（设计 §8.2 最后一条动作）。
  openEvidence(report.value?.evidencePath);
}

/**
 * 「恢复快照」——直接打开**这次预检之前**那份快照的差异预览。
 *
 * 此前这个按钮只 `loadSnapshots()` 然后把用户丢到设置页的快照列表：点的是
 * 恢复，得到的却是一个列表，用户得自己猜哪一份是「变更前」（审查 P1-04）。
 * 现在预检把 pre-change 快照的 id 交回报告（`preChangeSnapshotId`），这里
 * 直接调现有的 `previewRestore` 打开 `SnapshotRestoreDialog`——**不另起一套
 * 差异计算**，两处的差异口径必须一致，而用户在恢复前看的必须是同一份。
 *
 * 旧记录（快照机制之前、或打快照失败）没有这个 id：此时**如实改名**成
 * 「查看快照列表」并说明不能自动定位，而不是猜一个 id 去打开。
 */
const preChangeSnapshotId = computed(() => String(report.value?.preChangeSnapshotId || ''));
const canRestoreDirectly = computed(() => !!preChangeSnapshotId.value);

function openRestore() {
  return restorePreChange(preChangeSnapshotId.value);
}

function reload() {
  return loadPluginRun(report.value?.runId, true);
}

// 打开时拉一次。报告本身就是这次预检的结果，阶段时间线要另取那条
// 运行记录——两者可能来自不同进程（壳重启过），所以不能只信前者。
onMounted(() => {
  loadPluginRun(report.value?.runId);
});
</script>

<template>
  <div class="diag-card">
    <div class="diag-status">
      <span
        class="diag-status__dot"
        :class="`diag-status__dot--${verdictMeta.tone}`"
        aria-hidden="true"
      ></span>
      <span>{{ verdictMeta.label }}</span>
      <span v-if="hasWarnings" class="diag-card__aside">但有 {{ warnings.length }} 条告警</span>
      <span v-if="causeLabel(cause)" class="diag-card__aside">{{ causeLabel(cause) }}</span>
    </div>
    <p class="diag-meta">
      {{ spec.name || spec.id || '未知插件' }}
      <template v-if="report?.pin"> · {{ report.pin }}</template>
      <template v-if="cost"> · 预检耗时 {{ cost }}</template>
    </p>
    <p v-if="report?.summary" class="diag-summary-text">{{ report.summary }}</p>
    <p v-if="report?.hint" class="diag-next">{{ report.hint }}</p>
    <!-- inconclusive 必须说清「插件装上了但没验过」，否则用户会以为它被拦下了 -->
    <p v-if="report?.installed && report?.verdict !== 'pass'" class="diag-error">
      这个插件已装到 {{ instanceText }}，但没有经过启动验证——上面的结论不适用于它。
    </p>
  </div>

  <div class="diag-card">
    <h3 class="diag-card__title"><span>候选插件</span></h3>
    <div class="diag-rows">
      <div class="diag-row diag-row--static">
        <span class="diag-row__label">来源</span>
        <span class="diag-row__value">{{ sourceText }} · {{ sourceLabel }}</span>
      </div>
      <div class="diag-row diag-row--static">
        <span class="diag-row__label">完整性</span>
        <span
          class="diag-row__value"
          :class="integrity.tone ? `diag-row__value--${integrity.tone}` : ''"
        >{{ integrity.label }}</span>
      </div>
      <div class="diag-row diag-row--static">
        <span class="diag-row__label">物化方式</span>
        <span class="diag-row__value">{{ materialize }}</span>
      </div>
      <div class="diag-row diag-row--static">
        <span class="diag-row__label">影响范围</span>
        <span class="diag-row__value">{{ instanceText }}</span>
      </div>
    </div>
  </div>

  <div class="diag-card">
    <h3 class="diag-card__title">
      <span>实验流程</span>
      <span class="diag-card__aside">沙盒内一次真安装、一次真启动</span>
    </h3>
    <ul class="diag-timeline">
      <li v-for="step in stages" :key="step.key" class="diag-timeline__row">
        <span
          class="diag-timeline__mark"
          :class="`diag-timeline__mark--${stepMeta(step.status).tone}`"
          aria-hidden="true"
        >{{ stepMeta(step.status).mark }}</span>
        <div class="diag-timeline__main">
          <div class="diag-timeline__stage">
            <span>{{ step.label }}</span>
            <span class="diag-timeline__cost">{{ stepMeta(step.status).label }}</span>
          </div>
          <p v-if="step.events.length" class="diag-timeline__message">
            {{ step.events[step.events.length - 1].message }}
          </p>
        </div>
      </li>
    </ul>
  </div>

  <div v-if="hasWarnings" class="diag-card">
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

  <!-- 详细事件默认收起。页面上此前是「静态五段流程」+「RunTimeline」两条
       时间线连排，表达的是同一批事件，480×800 下会把底部的主动作推出首屏
       （审查 P2-06）。现在留下带「未执行」态的那条做主线（它回答「走到哪、
       哪一步没走」），逐条事件收进折叠区（回答「那一步具体说了什么」）。 -->
  <details v-if="(run?.events || []).length" class="diag-card">
    <summary>详细事件（{{ (run?.events || []).length }} 条）</summary>
    <RunTimeline />
  </details>

  <details v-if="risks.length" class="diag-card">
    <summary>证据与风险</summary>
    <ul class="diag-timeline">
      <li v-for="(item, i) in risks" :key="i" class="diag-timeline__row">
        <div class="diag-timeline__main">
          <p class="diag-timeline__message">{{ item }}</p>
        </div>
      </li>
    </ul>
  </details>

  <div class="diag-actions">
    <el-button @click="viewLogs">查看日志</el-button>
    <el-button :loading="diagnosticStore.loading" @click="reload">刷新</el-button>
    <!-- 只给已安装的插件：没装上的插件没有「退回到变更前」这回事。
         没有 pre-change 快照 id 时改名成「查看快照列表」——按钮名必须等于
         实际效果，不能写着「恢复」却只给一个列表（审查 P1-04）。 -->
    <el-button v-if="report?.installed" @click="openRestore">
      {{ canRestoreDirectly ? '恢复变更前状态' : '查看快照列表' }}
    </el-button>
  </div>
</template>
