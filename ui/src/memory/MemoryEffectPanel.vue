<!--
  记忆效果页签（P0）：只读展示「记忆有没有被注入、被读过，以及这些数可不可信」。

  数据全部来自后端 memory_effect_report（合同 §2.2），本组件不落账、不调其它命令、
  不引入图表库——趋势图是手写 SVG（与用量页环形图同一路子）。口径三条（设计 §5.6）：
  · 「注入快照」是事件次数，压缩后原样重发也算一次；
  · 「携带记忆请求」是请求上下文里有有效快照的请求，与注入次数相加没有意义；
  · 「主动读取 / 检索」正文随注入给全时 0 次是正常的，不能当无效证据。
  缺字段一律显示「—」（见 effect.js 的 finiteNumber），不补 0。
-->
<script setup>
import { computed } from 'vue';
import { InfoFilled, Refresh } from '@element-plus/icons-vue';
import { isLoading } from '../shell/loading.js';
import {
  EFFECT_RANGES,
  MIN_SAMPLE,
  effectStore,
  formatBytes,
  formatCount,
  formatRatio,
  formatTokens,
  healthLevel,
  loadEffectReport,
  sampleEnough,
  setEffectRange,
  sharePercent,
  shortDate,
} from './effect.js';

const EMPTY = '—';

/** 读一个可能不存在的子对象/字段：缺字段返回 undefined，交给各 format 变成「—」。 */
function pick(source, key) {
  return source && typeof source === 'object' ? source[key] : undefined;
}

/** 图表数字：非有限数按「没有这一天」处理（折线断开，不当 0）。 */
function chartNumber(value) {
  if (value === null || value === undefined || value === '') return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

const report = computed(() => effectStore.report);
const loading = computed(() => effectStore.loading);
const error = computed(() => effectStore.error);
const rangeDays = computed(() => effectStore.rangeDays);

const coverage = computed(() => pick(report.value, 'coverage'));
const kpi = computed(() => pick(report.value, 'kpi'));
const funnel = computed(() => pick(report.value, 'funnel'));
const cost = computed(() => pick(report.value, 'cost'));
const health = computed(() => pick(report.value, 'health'));

const rangeLabel = computed(() => '近 ' + rangeDays.value + ' 天');

/** 时间窗（合同 window.from / window.to）。区间用「~」而不是横线：窄格里横线像负号。 */
const windowText = computed(() => {
  const range = pick(report.value, 'window');
  const from = shortDate(pick(range, 'from'));
  const to = shortDate(pick(range, 'to'));
  if (from === EMPTY && to === EMPTY) return EMPTY;
  return from + ' ~ ' + to;
});

/** generated_at（ISO）→ 本地时区的「10-31 20:40」；认不出就不显示这句。 */
const updatedText = computed(() => {
  const raw = pick(report.value, 'generated_at');
  if (!raw) return '';
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) return '';
  const pad = (value) => String(value).padStart(2, '0');
  return pad(date.getMonth() + 1) + '-' + pad(date.getDate()) + ' ' + pad(date.getHours()) + ':' + pad(date.getMinutes());
});

// —— 读数条（KPI 5 块） ------------------------------------------------------
// 触达会话的分母是 coverage.sessions_active（活跃会话）；覆盖率由 sharePercent 派生，
// 分母不足 MIN_SAMPLE 时只给计数（设计 §5.2）。合同没有下发覆盖率字段（见 effect.js）。
const coverageRate = computed(() =>
  sharePercent(pick(coverage.value, 'sessions_with_injection'), pick(coverage.value, 'sessions_active'))
);
const coverageEnough = computed(() => sampleEnough(pick(coverage.value, 'sessions_active')));

/** 「读 · 检索」两个计数：都缺字段时只显示一个「—」，不写成「— · —」。 */
function callsText(readCalls, searchCalls) {
  if (readCalls === EMPTY && searchCalls === EMPTY) return EMPTY;
  return readCalls + ' · ' + searchCalls;
}

const kpiCards = computed(() => {
  const data = kpi.value || {};
  const sessions = coverage.value || {};
  const sessionsHit = pick(sessions, 'sessions_with_injection');
  const sessionsActive = pick(sessions, 'sessions_active');
  const requests = pick(data, 'requests');
  const memoryRequests = pick(data, 'memory_requests');
  const rateText = coverageRate.value === null
    ? EMPTY
    : coverageEnough.value ? formatRatio(coverageRate.value) : '样本不足';
  return [
    {
      key: 'sessions',
      label: '触达会话',
      value: formatCount(sessionsHit),
      unit: '会话',
      foot: coverageEnough.value
        ? '覆盖率 ' + rateText + ' · 分母 ' + formatCount(sessionsActive) + ' 会话'
        : '覆盖率 ' + rateText + ' · 分母 ' + formatCount(sessionsActive) + ' 会话（不足 ' + MIN_SAMPLE + '）',
      hint: '有至少一次记忆快照的会话 / 活跃会话。分母不足 ' + MIN_SAMPLE + ' 时只给计数、不给百分比。',
    },
    {
      key: 'snapshots',
      label: '注入快照',
      value: formatCount(pick(data, 'injection_snapshots')),
      unit: '次',
      foot: '压缩后原样重发也计一次（是事件次数，不是采用次数）',
      hint: '会话日志里 user/message 的 runtime-context 快照中包含记忆段的次数。',
    },
    {
      key: 'bytes',
      label: '注入字节',
      value: formatBytes(pick(data, 'injected_bytes')),
      unit: '',
      foot: 'token 估算 ' + formatTokens(pick(data, 'injected_tokens_est')),
      hint: '记忆段的原始字节合计；token 由字节标定估算，只看量级。',
    },
    {
      key: 'ratio',
      label: '携带记忆请求率',
      value: formatRatio(pick(data, 'memory_request_ratio')),
      unit: '',
      foot: formatCount(memoryRequests) + ' / ' + formatCount(requests) + ' 次请求',
      hint: '请求上下文里有有效记忆快照的请求占比（压缩后原样重发的也算）。与「注入快照」是两个口径。',
    },
    {
      key: 'calls',
      label: '主动读取 · 检索',
      // 两个字段都缺时只给一个「—」；有一个就给「读 · 检索」两个位置。
      value: callsText(formatCount(pick(data, 'memory_read_calls')), formatCount(pick(data, 'memory_search_calls'))),
      unit: '次',
      foot: formatCount(pick(data, 'memory_propose_calls')) + ' 次提出候选；正文随注入给全时 0 次正常',
      hint: '模型主动调 memory_read（按 id 读全文）/ memory_search（检索）。0 次不是故障。',
    },
  ];
});

// —— 效果漏斗（4 阶段） ------------------------------------------------------
// 阶段计数是「独立记忆 / 候选」的个数（后端口径），转化率只在分母 >= MIN_SAMPLE 时给。
const FUNNEL_STAGES = [
  { key: 'candidates_created', label: '新候选' },
  { key: 'memories_created', label: '新记忆' },
  { key: 'memories_injected', label: '被注入的记忆' },
  { key: 'memories_read', label: '被主动读取' },
];

const funnelRows = computed(() => {
  const data = funnel.value || {};
  const values = FUNNEL_STAGES.map((stage) => chartNumber(pick(data, stage.key)));
  const maxValue = Math.max(1, ...values.map((value) => (value === null ? 0 : value)));
  return FUNNEL_STAGES.map((stage, index) => {
    const value = values[index];
    const previous = index > 0 ? values[index - 1] : null;
    const enough = previous !== null && previous > 0 && sampleEnough(previous);
    return {
      key: stage.key,
      label: stage.label,
      count: formatCount(pick(data, stage.key)),
      width: value === null ? 0 : Math.max(value > 0 ? 2 : 0, (value / maxValue) * 100),
      rate: enough ? formatRatio(sharePercent(value, previous)) : EMPTY,
    };
  });
});

const funnelAria = computed(() =>
  '效果漏斗：' + funnelRows.value.map((row) => row.label + ' ' + row.count).join('，')
);

// —— 趋势（手写 SVG） --------------------------------------------------------
// 柱 = 每日注入快照数（左轴）；线 = 当日携带记忆请求率（0–100%，与柱共用绘图区高度）。
// 少于 3 天数据不出图（设计 §5.3：只给上面那些总数），避免一条两点折线读成趋势。
const TREND = { width: 340, height: 132, left: 28, right: 8, top: 8, bottom: 22 };
const TREND_VIEW = '0 0 340 132';

const trendRows = computed(() => {
  const rows = pick(report.value, 'trend');
  return Array.isArray(rows) ? rows : [];
});
const trendEmpty = computed(() => trendRows.value.length < 3);

const trendChart = computed(() => {
  const rows = trendRows.value;
  const plotWidth = TREND.width - TREND.left - TREND.right;
  const plotHeight = TREND.height - TREND.top - TREND.bottom;
  const injections = rows.map((row) => chartNumber(pick(row, 'injection_snapshots')));
  const maxInjections = Math.max(1, ...injections.map((value) => (value === null ? 0 : value)));
  const slot = rows.length ? plotWidth / rows.length : plotWidth;
  const barWidth = Math.max(1.5, Math.min(14, slot * 0.56));
  const bars = rows.map((row, index) => {
    const value = injections[index] === null ? 0 : Math.max(0, injections[index]);
    const height = (value / maxInjections) * plotHeight;
    return {
      key: 'bar-' + (row.date || index),
      x: TREND.left + slot * index + (slot - barWidth) / 2,
      y: TREND.top + plotHeight - height,
      width: barWidth,
      height,
      value,
      date: row.date,
    };
  });
  const dots = rows.map((row, index) => {
    const requests = chartNumber(pick(row, 'requests'));
    const memoryRequests = chartNumber(pick(row, 'memory_requests'));
    if (requests === null || requests <= 0 || memoryRequests === null) return null;
    const rate = Math.min(100, Math.max(0, (memoryRequests / requests) * 100));
    return {
      key: 'dot-' + (row.date || index),
      x: TREND.left + slot * index + slot / 2,
      y: TREND.top + plotHeight - (rate / 100) * plotHeight,
      rate,
      date: row.date,
    };
  });
  // 没有请求的那一天折线断开，不连过去（连过去等于编了一个 0%）。
  const segments = [];
  let current = [];
  for (const dot of dots) {
    if (dot) {
      current.push(dot);
    } else if (current.length) {
      segments.push(current);
      current = [];
    }
  }
  if (current.length) segments.push(current);
  const lines = segments.map((segment, index) => ({
    key: 'line-' + index,
    points: segment.map((dot) => dot.x.toFixed(1) + ',' + dot.y.toFixed(1)).join(' '),
    dots: segment,
  }));
  const gridLines = [0, 0.5, 1].map((ratio) => ({
    key: 'grid-' + ratio,
    ratio,
    y: TREND.top + plotHeight - ratio * plotHeight,
    label: formatCount(Math.round(maxInjections * ratio)),
  }));
  const labelStep = Math.max(1, Math.ceil(rows.length / 6));
  const labels = rows
    .map((row, index) => ({
      key: 'label-' + (row.date || index),
      x: TREND.left + slot * index + slot / 2,
      text: shortDate(row.date),
    }))
    .filter((label, index) => index % labelStep === 0 || index === rows.length - 1);
  return {
    plotLeft: TREND.left,
    plotRight: TREND.left + plotWidth,
    baseline: TREND.top + plotHeight,
    maxInjections,
    labelY: TREND.height - 7,
    bars,
    lines,
    gridLines,
    labels,
  };
});

const trendAria = computed(() => {
  const rows = trendRows.value;
  if (rows.length < 3) return '趋势数据不足 3 天，暂不出图';
  const total = trendChart.value.bars.reduce((sum, bar) => sum + bar.value, 0);
  return '每日注入快照与携带记忆请求率趋势：共 ' + rows.length + ' 天，注入快照合计 ' + formatCount(total) + ' 次';
});

// —— 记忆榜单 ---------------------------------------------------------------
const SCOPE_LABELS = { project: '项目', general: '通用' };
const VERIFY_LABELS = { verified: '已验证', unverified: '未验证', stale: '已过期' };

function scopeText(value) {
  return SCOPE_LABELS[value] || (value ? String(value) : EMPTY);
}

function verifyText(value) {
  return VERIFY_LABELS[value] || (value ? String(value) : EMPTY);
}

/** 后端已按注入快照数排序；前端只截前 8 条、只展示标题不展示正文。 */
const leaderboardRows = computed(() => {
  const rows = pick(report.value, 'leaderboard');
  if (!Array.isArray(rows)) return [];
  return rows.slice(0, 8).map((row, index) => ({
    key: (row && row.id) || 'rank-' + index,
    title: (row && row.title) || '（无标题）',
    meta: scopeText(pick(row, 'scope')) + ' · ' + verifyText(pick(row, 'verification')),
    snapshots: formatCount(pick(row, 'injection_snapshots')),
    exposures: formatCount(pick(row, 'memory_requests')),
    sessions: formatCount(pick(row, 'sessions')),
  }));
});

// —— 成本 -------------------------------------------------------------------
/** 「12 次」这类带单位的值；缺字段时只留「—」，不写成「— 次」。 */
function withUnit(text, unit) {
  return text === EMPTY ? EMPTY : text + unit;
}

const costRows = computed(() => {
  const data = cost.value || {};
  return [
    { key: 'bytes', label: '注入字节', value: formatBytes(pick(data, 'injected_bytes')), note: '事实' },
    { key: 'tokens', label: '注入 token', value: formatTokens(pick(data, 'injected_tokens_est')), note: '估算' },
    { key: 'done', label: '提取任务完成', value: withUnit(formatCount(pick(data, 'extraction_jobs_done')), ' 次'), note: '' },
    { key: 'failed', label: '提取任务失败', value: withUnit(formatCount(pick(data, 'extraction_jobs_failed')), ' 次'), note: '' },
    { key: 'upper', label: '提取 token 上界', value: formatTokens(pick(data, 'extraction_tokens_upper_est')), note: '估算上限' },
  ];
});

// —— 数据健康 ---------------------------------------------------------------
const healthRows = computed(() => {
  const sessions = coverage.value || {};
  const scanRate = sharePercent(pick(sessions, 'sessions_active'), pick(sessions, 'sessions_total'));
  const decodeFailures = chartNumber(pick(sessions, 'decode_failures'));
  const unknownRatio = chartNumber(pick(sessions, 'unknown_event_ratio'));
  return [
    {
      key: 'coverage',
      label: '扫描覆盖率',
      value: scanRate === null ? EMPTY : formatRatio(scanRate),
      level: healthLevel(scanRate, { good: 95, warn: 80 }),
    },
    {
      key: 'decode',
      label: '解码失败',
      value: formatCount(decodeFailures),
      level: healthLevel(decodeFailures, { good: 0, warn: 2, lowerIsBetter: true }),
    },
    {
      key: 'unknown',
      label: '未知事件占比',
      value: formatRatio(unknownRatio),
      level: healthLevel(unknownRatio, { good: 5, warn: 15, lowerIsBetter: true }),
    },
  ];
});

const versionsText = computed(() => {
  const versions = pick(coverage.value, 'format_versions');
  if (!versions || typeof versions !== 'object') return EMPTY;
  const keys = Object.keys(versions).sort();
  if (!keys.length) return EMPTY;
  return keys.map((key) => key + ' × ' + formatCount(versions[key])).join(' · ');
});

const sampleText = computed(() => {
  const data = health.value;
  if (!data) return '还没有样本量信息。';
  const requestsEnough = data.has_enough_requests === true;
  const injectionsEnough = data.has_enough_injections === true;
  if (requestsEnough && injectionsEnough) {
    return '请求量与注入量都已达到 ' + MIN_SAMPLE + '，比率可以直接读。';
  }
  const lacks = [];
  if (!requestsEnough) lacks.push('请求量');
  if (!injectionsEnough) lacks.push('注入量');
  return lacks.join('、') + '不足 ' + MIN_SAMPLE + '：比率降级为计数，只作事实展示。';
});

const levelClass = (level) => 'effect-level effect-level--' + level;

function changeRange(days) {
  if (days === rangeDays.value) return;
  setEffectRange(days);
}

function refresh() {
  loadEffectReport({ force: true });
}
</script>

<template>
  <div class="memory-effect">
    <!-- 小标题行：数据来源 + 时间窗 + 刷新 + 数据更新时刻（合同 §3.3）。 -->
    <div class="effect-toolbar">
      <p class="effect-source">
        数据来源：本机会话日志只读扫描 + 记忆库 · 不采集正文
        <template v-if="windowText !== '—'"> · 窗口 {{ windowText }}</template>
        <template v-if="updatedText"> · 更新于 {{ updatedText }}</template>
      </p>
      <div class="effect-actions">
        <div class="effect-range" role="group" aria-label="记忆效果时间范围">
          <button
            v-for="days in EFFECT_RANGES"
            :key="days"
            type="button"
            class="effect-range__btn"
            :class="{ 'is-active': days === rangeDays }"
            :aria-pressed="days === rangeDays"
            :disabled="loading"
            :title="'统计最近 ' + days + ' 天'"
            @click="changeRange(days)"
          >
            {{ days }} 天
          </button>
        </div>
        <el-button
          class="icon-btn"
          text
          :icon="Refresh"
          :loading="isLoading('memoryEffect')"
          title="刷新（重新扫描本机会话日志）"
          aria-label="刷新（重新扫描本机会话日志）"
          @click="refresh"
        />
      </div>
    </div>

    <!-- 错误态：保留上一次成功数据，只在上面挂一条可重试的提示（不静默、不白屏）。 -->
    <el-alert v-if="error" type="error" :closable="false" show-icon class="effect-error">
      <template #title>
        记忆效果数据读取失败：{{ error }}
        <el-button text type="primary" :disabled="loading" @click="refresh">重试</el-button>
      </template>
    </el-alert>

    <div v-loading="loading && !report" class="effect-body">
      <template v-if="report">
        <!-- 1) 读数条：5 块事实读数（每块都带分母 / 口径与来源角标）。 -->
        <section class="effect-kpis" aria-label="记忆效果事实读数">
          <div v-for="card in kpiCards" :key="card.key" class="card effect-kpi" :title="card.hint">
            <p class="effect-kpi__label">{{ card.label }}</p>
            <p class="effect-kpi__value">
              <span>{{ card.value }}</span>
              <small v-if="card.unit">{{ card.unit }}</small>
            </p>
            <p class="effect-kpi__foot">{{ card.foot }}</p>
          </div>
        </section>

        <div class="effect-grid">
          <!-- 2) 效果漏斗：4 阶段横向条；样本不足不给百分比。 -->
          <div class="card">
            <div class="card-head">
              <h2>
                效果漏斗
                <el-tooltip placement="bottom-start" :show-after="80">
                  <template #content>
                    <div class="card-info-tooltip">
                      从候选到主动读取，每一步都可能流失；阶段转化率在分母不足 {{ MIN_SAMPLE }} 时不显示。
                      这里的计数都是窗口内的事实（候选 / 新记忆 / 被注入的独立记忆 / 被主动读取的独立记忆），不是采用次数。
                    </div>
                  </template>
                  <el-icon class="card-info-icon" tabindex="0" aria-label="效果漏斗的口径">
                    <InfoFilled />
                  </el-icon>
                </el-tooltip>
              </h2>
              <span class="effect-pill">{{ rangeLabel }}</span>
            </div>
            <div class="effect-funnel" role="img" :aria-label="funnelAria">
              <div v-for="row in funnelRows" :key="row.key" class="effect-funnel__stage">
                <span class="effect-funnel__label">{{ row.label }}</span>
                <div class="effect-funnel__track">
                  <i :style="{ width: row.width + '%' }"></i>
                </div>
                <b class="effect-funnel__count">{{ row.count }}</b>
                <span class="effect-funnel__rate">{{ row.rate === '—' ? '' : '上一步 ' + row.rate }}</span>
              </div>
            </div>
          </div>

          <!-- 3) 趋势：手写 SVG（柱 = 每日注入快照数；线 = 当日携带记忆请求率）。 -->
          <div class="card">
            <div class="card-head">
              <h2>
                趋势（日）
                <el-tooltip placement="bottom-start" :show-after="80">
                  <template #content>
                    <div class="card-info-tooltip">
                      柱 = 每日注入快照数；线 = 当日携带记忆请求率。两者量纲不同，共用绘图区高度：
                      柱读左侧计数刻度，线按 0–100% 铺满同一高度。少于 3 天数据不出图，只给上面的总数。
                    </div>
                  </template>
                  <el-icon class="card-info-icon" tabindex="0" aria-label="趋势图的读法">
                    <InfoFilled />
                  </el-icon>
                </el-tooltip>
              </h2>
              <span class="effect-pill">{{ rangeLabel }}</span>
            </div>
            <div v-if="trendEmpty" class="effect-placeholder">
              数据不足 3 天，先看上面的总数；聊几轮之后再回来看趋势。
            </div>
            <div v-else class="effect-trend">
              <svg :viewBox="TREND_VIEW" role="img" :aria-label="trendAria" class="effect-trend__svg">
                <line
                  v-for="tick in trendChart.gridLines"
                  :key="tick.key"
                  :x1="trendChart.plotLeft"
                  :x2="trendChart.plotRight"
                  :y1="tick.y"
                  :y2="tick.y"
                  class="effect-trend__grid"
                />
                <rect
                  v-for="bar in trendChart.bars"
                  :key="bar.key"
                  :x="bar.x"
                  :y="bar.y"
                  :width="bar.width"
                  :height="bar.height"
                  rx="1.5"
                  class="effect-trend__bar"
                >
                  <title>{{ shortDate(bar.date) }}：{{ bar.value }} 次注入快照</title>
                </rect>
                <g v-for="line in trendChart.lines" :key="line.key">
                  <polyline :points="line.points" class="effect-trend__line" />
                  <circle v-for="dot in line.dots" :key="dot.key" :cx="dot.x" :cy="dot.y" r="1.8" class="effect-trend__dot" />
                </g>
                <text
                  v-for="tick in trendChart.gridLines"
                  :key="'y-' + tick.key"
                  :x="trendChart.plotLeft - 4"
                  :y="tick.y + 3"
                  text-anchor="end"
                  class="effect-trend__axis"
                >{{ tick.label }}</text>
                <text
                  v-for="label in trendChart.labels"
                  :key="label.key"
                  :x="label.x"
                  :y="trendChart.labelY"
                  text-anchor="middle"
                  class="effect-trend__axis"
                >{{ label.text }}</text>
              </svg>
              <div class="effect-legend">
                <span><i class="effect-legend__bar"></i>注入快照（次）</span>
                <span><i class="effect-legend__line"></i>携带记忆请求率（0–100%）</span>
              </div>
            </div>
          </div>

          <!-- 4) 记忆榜单：Top 8；标题来自记忆库，已删除的显示状态、不显示正文。 -->
          <div class="card">
            <div class="card-head">
              <h2>
                记忆榜单
                <el-tooltip placement="bottom-start" :show-after="80">
                  <template #content>
                    <div class="card-info-tooltip">
                      按窗口内注入快照数排序，标题来自记忆库（已删除或不活跃的只显示状态，不显示正文）。
                      「曝光」= 携带该条记忆的请求数；「会话」= 出现过它的会话数。榜单只用于人工判断，不自动删除。
                    </div>
                  </template>
                  <el-icon class="card-info-icon" tabindex="0" aria-label="记忆榜单的口径">
                    <InfoFilled />
                  </el-icon>
                </el-tooltip>
              </h2>
              <span class="effect-pill">Top 8</span>
            </div>
            <p v-if="!leaderboardRows.length" class="effect-placeholder">
              窗口内还没有记忆被注入。打开「使用已批准的记忆」并聊几轮后出现。
            </p>
            <div v-else class="effect-rank">
              <div class="effect-rank__row effect-rank__row--head">
                <span>记忆</span><span>注入</span><span>曝光</span><span>会话</span>
              </div>
              <div v-for="row in leaderboardRows" :key="row.key" class="effect-rank__row">
                <span class="effect-rank__name" :title="row.title">
                  {{ row.title }}
                  <em>{{ row.meta }}</em>
                </span>
                <span>{{ row.snapshots }}</span>
                <span>{{ row.exposures }}</span>
                <span>{{ row.sessions }}</span>
              </div>
            </div>
          </div>

          <!-- 5) 成本：注入体积 / token 估算 / 提取任务；估算值明确标注。 -->
          <div class="card">
            <div class="card-head">
              <h2>
                成本
                <el-tooltip placement="bottom-start" :show-after="80">
                  <template #content>
                    <div class="card-info-tooltip">
                      注入字节是事实；token 由字节标定估算，只看量级。提取任务计数来自提取任务表（jobs.json），
                      「提取 token 上界」是估算上限，不是精确账单。
                    </div>
                  </template>
                  <el-icon class="card-info-icon" tabindex="0" aria-label="成本的口径">
                    <InfoFilled />
                  </el-icon>
                </el-tooltip>
              </h2>
              <span class="effect-pill">{{ rangeLabel }}</span>
            </div>
            <div class="effect-cost">
              <div v-for="row in costRows" :key="row.key" class="effect-cost__row">
                <span class="effect-cost__label">{{ row.label }}</span>
                <b class="effect-cost__value">{{ row.value }}</b>
                <span v-if="row.note" class="effect-tag">{{ row.note }}</span>
                <span v-else class="effect-tag effect-tag--empty"></span>
              </div>
            </div>
          </div>

          <!-- 6) 数据健康：这些数能不能信；P1 未接入项如实写，不编数据。 -->
          <div class="card effect-grid__full">
            <div class="card-head">
              <h2>
                数据健康
                <el-tooltip placement="bottom-start" :show-after="80">
                  <template #content>
                    <div class="card-info-tooltip">
                      扫描覆盖率 = 成功解析的会话 / 窗口内会话；覆盖率不足或未知事件偏高时，上方的比率只能当估算读。
                      解码失败不静默归零，版本分布说明日志格式是否混用。
                    </div>
                  </template>
                  <el-icon class="card-info-icon" tabindex="0" aria-label="数据健康的口径">
                    <InfoFilled />
                  </el-icon>
                </el-tooltip>
              </h2>
              <span class="effect-pill">可信度</span>
            </div>
            <div class="effect-health">
              <div v-for="row in healthRows" :key="row.key" class="effect-health__item">
                <span class="effect-health__label">{{ row.label }}</span>
                <b :class="levelClass(row.level)">{{ row.value }}</b>
              </div>
              <div class="effect-health__item">
                <span class="effect-health__label">格式版本分布</span>
                <b class="effect-health__versions">{{ versionsText }}</b>
              </div>
            </div>
            <p class="effect-health__note">{{ sampleText }}</p>
            <p class="effect-health__note effect-muted">
              会话档案：{{ formatCount(coverage && coverage.sessions_total) }} 个会话，
              {{ formatCount(coverage && coverage.sessions_active) }} 个活跃。
              分组对比与「未注入原因分解」为 P1 未接入，本页不画假数据。
            </p>
          </div>
        </div>

        <p class="effect-foot">
          本页读数都是 L1 事实（计数 / 分布，只读本机会话日志与记忆库）；分组对比属 L2 关联、因果结论需实验，均为后续阶段，未接入。
        </p>
      </template>

      <!-- 空态：没有数据时给可执行的下一步，而不是一张空白卡。 -->
      <div v-else-if="!loading" class="card effect-empty">
        <h2>还没有记忆效果数据</h2>
        <p class="effect-placeholder">
          先打开「使用已批准的记忆」并在工作台里聊几轮，让记忆真的进入过会话；之后回到这里点刷新。
          统计只读本机会话日志与记忆库，不采集任何正文。
        </p>
        <el-button type="primary" :loading="isLoading('memoryEffect')" @click="refresh">刷新数据</el-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 效果页允许整页滚动：内容比一屏多（首屏读数 + 漏斗 + 趋势，第二屏榜单 / 成本 / 健康），
   滚动只发生在主区（theme.css 的 main），侧栏不动。所以这里不钉高度、不自己滚。 */
.memory-effect {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 0 0 auto;
  min-width: 0;
}

.effect-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px 12px;
}

.effect-source {
  margin: 0;
  flex: 1 1 360px;
  min-width: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.effect-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 时间窗药丸：形状与原型一致（段内 2px 内衬），选中态走 accent 语义色。 */
.effect-range {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-control);
  background: var(--overlay-faint);
}

.effect-range__btn {
  border: 0;
  padding: 4px 10px;
  border-radius: 4px;
  background: transparent;
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 12px;
  line-height: 1.4;
  cursor: pointer;
}

.effect-range__btn:hover:not(:disabled) {
  color: var(--text);
}

.effect-range__btn.is-active {
  background: var(--accent-fill);
  color: var(--accent);
  font-weight: 600;
}

.effect-range__btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.effect-range__btn:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}

.effect-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 120px;
}

/* 首屏读数条：5 块等宽；窄了折成多行（1040 窗下每块约 160px，够放「12,712」）。 */
.effect-kpis {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 12px;
}

/* 读数块复用 .card 的表面与描边，只把内边距压到「读数条」的密度：
   卡片的标题分隔线在这里不适用（没有卡头），读数块不是可折叠的功能块。 */
.effect-kpi {
  padding: 8px 11px;
  gap: 2px;
  min-width: 0;
}

.effect-kpi__label {
  margin: 0;
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 1.4;
}

.effect-kpi__value {
  margin: 2px 0 0;
  display: flex;
  align-items: baseline;
  gap: 5px;
  color: var(--text);
  font-size: 20px;
  font-weight: 650;
  line-height: 1.25;
  letter-spacing: 0.2px;
  font-variant-numeric: tabular-nums;
  overflow-wrap: anywhere;
}

.effect-kpi__value small {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 500;
}

.effect-kpi__foot {
  margin: 0;
  color: var(--text-muted);
  font-size: 10.5px;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

/* 2×2 卡片栅格 + 一行通栏；gap 与 .panel 的 12px 同一档。 */
.effect-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  align-items: start;
  gap: 12px;
}

.effect-grid__full {
  grid-column: 1 / -1;
}

.effect-pill {
  flex: none;
  padding: 1px 7px;
  border-radius: 8px;
  background: var(--overlay-faint);
  color: var(--text-secondary);
  font-size: 10.5px;
  line-height: 1.6;
  white-space: nowrap;
}

.effect-placeholder {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.55;
  overflow-wrap: anywhere;
}

/* 漏斗：标签 / 条 / 计数三列，条是 100% 基准上的实心段。 */
.effect-funnel {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.effect-funnel__stage {
  display: grid;
  grid-template-columns: 84px minmax(0, 1fr) 56px;
  grid-template-rows: auto auto;
  align-items: center;
  column-gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}

.effect-funnel__label {
  grid-column: 1;
  grid-row: 1;
  overflow-wrap: anywhere;
}

.effect-funnel__track {
  grid-column: 2;
  grid-row: 1;
  height: 16px;
  border-radius: 3px;
  background: var(--overlay-faint);
  overflow: hidden;
}

.effect-funnel__track i {
  display: block;
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  opacity: 0.8;
}

.effect-funnel__count {
  grid-column: 3;
  grid-row: 1;
  color: var(--text);
  font-size: 12px;
  font-weight: 600;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.effect-funnel__rate {
  grid-column: 2;
  grid-row: 2;
  color: var(--text-muted);
  font-size: 10.5px;
  line-height: 1.5;
}

/* 趋势：SVG 铺满卡宽；高度写死一档（viewBox 比例），窄了只缩不塌。 */
.effect-trend {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.effect-trend__svg {
  width: 100%;
  height: 150px;
  display: block;
}

.effect-trend__grid {
  stroke: var(--border);
  stroke-width: 1;
  stroke-dasharray: 2 4;
}

.effect-trend__bar {
  fill: var(--accent);
  opacity: 0.78;
}

.effect-trend__line {
  fill: none;
  stroke: var(--warning);
  stroke-width: 2;
  stroke-linejoin: round;
  stroke-linecap: round;
}

.effect-trend__dot {
  fill: var(--warning);
}

.effect-trend__axis {
  fill: var(--text-muted);
  font-size: 8px;
}

.effect-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 14px;
  color: var(--text-muted);
  font-size: 10.5px;
}

.effect-legend i {
  display: inline-block;
  width: 8px;
  height: 8px;
  margin-right: 4px;
  border-radius: 2px;
  vertical-align: -1px;
}

.effect-legend__bar {
  background: var(--accent);
  opacity: 0.78;
}

.effect-legend__line {
  height: 0 !important;
  width: 12px !important;
  border-top: 2px solid var(--warning);
  border-radius: 0;
}

/* 榜单：四列（名称吃剩余宽，三个计数窄列）；名称过长折行而不是截断。 */
.effect-rank {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.effect-rank__row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 46px 46px 46px;
  align-items: baseline;
  gap: 8px;
  padding: 3px 0;
  font-size: 12px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}

.effect-rank__row + .effect-rank__row {
  border-top: 1px solid var(--border-soft);
}

.effect-rank__row--head {
  color: var(--text-muted);
  font-size: 10.5px;
}

.effect-rank__row span:not(:first-child) {
  text-align: right;
}

.effect-rank__name {
  min-width: 0;
  color: var(--text);
  overflow-wrap: anywhere;
}

.effect-rank__name em {
  margin-left: 4px;
  color: var(--text-muted);
  font-size: 10.5px;
  font-style: normal;
  white-space: nowrap;
}

/* 成本：标签 / 值 / 估算角标三列。 */
.effect-cost {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.effect-cost__row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto 56px;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}

.effect-cost__value {
  color: var(--text);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.effect-tag {
  justify-self: end;
  padding: 1px 6px;
  border-radius: 8px;
  background: var(--warning-fill);
  color: var(--warning);
  font-size: 10px;
  line-height: 1.6;
  white-space: nowrap;
}

.effect-tag--empty {
  background: transparent;
}

/* 健康：两列小格 + 版本分布独占一行 + 两段说明。 */
.effect-health {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}

.effect-health__item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
  padding: 4px 8px;
  border-radius: 5px;
  background: var(--overlay-faint);
  font-size: 12px;
}

.effect-health__label {
  min-width: 0;
  color: var(--text-secondary);
  overflow-wrap: anywhere;
}

.effect-health__versions {
  max-width: 60%;
  text-align: right;
  color: var(--text);
  font-weight: 600;
  overflow-wrap: anywhere;
}

.effect-level {
  flex: none;
  color: var(--text);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.effect-level--ok { color: var(--success); }
.effect-level--warn { color: var(--warning); }
.effect-level--bad { color: var(--danger); }
.effect-level--unknown { color: var(--text-muted); }

.effect-health__note {
  margin: 0;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.effect-muted {
  font-size: 10.5px;
}

.effect-foot {
  margin: 0;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.5;
}

.effect-empty {
  align-items: flex-start;
  gap: 8px;
}

.effect-empty h2 {
  font-size: var(--fs-block-title);
  font-weight: 600;
}

.effect-error {
  margin: 0;
}
</style>
