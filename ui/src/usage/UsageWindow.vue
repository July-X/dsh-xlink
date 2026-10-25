<script setup>
// 独立模型用量窗口：概览卡「模型用量」按钮经 open_usage_window 弹出
// （URL ?usage=1 挂载本组件，capability `usage-viewer.json` 只授予
// `get_model_usage`）。四段展示与示意图同构——顶部摘要卡、活跃热力图
// （GitHub 式周列）、按模型堆叠的按天趋势柱状图与模型用量环形图 + 列表；
// 全部 CSS / 内联 SVG，不引图表库。支持时间范围切换（15 ~ 90 天，展示
// 层切片，不触发重扫）；趋势柱状图 hover 出模型明细浮层。窗口打开即强制
// 重扫（usage.js 的 refreshUsage）；ℹ️ tooltip 是 90 天保留策略的告知位。
import ViewerShell from '../shell/ViewerShell.vue';
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue';
import { Refresh, InfoFilled } from '@element-plus/icons-vue';
import { ioActive, isLoading, withLoading } from '../shell/loading.js';
import { bindScrollAutoHide } from '../logs/logs.js';
import {
  usage,
  refreshUsage,
  setExternalSources,
  formatTokens,
  formatPercent,
  modelColor,
  heatmapColumns,
  heatLevels,
  stackTrend,
  donutSlices,
  sliceDays,
  summarizeDays,
  todayUsage,
  modelName,
  weekdayLabel,
  RANGE_OPTIONS,
  RETENTION_DAYS,
} from './usage.js';

const RETENTION_TIP =
  `统计最近 ${RETENTION_DAYS} 天的模型用量，超过 ${RETENTION_DAYS} 天的记录自动丢弃。` +
  `账目只存按「天 × 模型」的聚合值，不保存会话内容。`;

// 外部来源的说明（挂在标题旁的 ⓘ 上）。**默认不勾**：不勾就不读那些工具
// 留在本机的目录；勾上之后壳**只读**它们，不写、不改、不刷新凭据。
const EXTERNAL_TIP =
  '额外统计本机其他 Agent 工具（Codex、MiniMax Desktop）留在本机的对话 token 用量。' +
  '默认不勾选；勾选后只读取那些工具的本地记录，不写入、不修改它们。' +
  '各工具的 token 口径不同（缓存命中算不算在内各家不一样），因此单独列出，不与上面那份相加。';

// 与管理壳一致：本窗口内 IO 进行中点亮标题栏鲸眼脉冲。
watchEffect(() => {
  document.body.classList.toggle('pulse-active', ioActive.value);
});

onMounted(() => {
  refreshUsage();
});

const data = computed(() => usage.data);

// --- 外部来源（Codex / MiniMax Desktop）------------------------------------
//
// **默认都不勾**：不勾就不读那些工具留在本机的目录。真值在 Rust 侧（按壳
// 模式持久化），这里只是镜像——每次拉取都以后端返回的 `external_sources`
// 为准，前端不另存一份，免得两边分叉。
const codexChecked = ref(false);
const minimaxChecked = ref(false);

watch(
  () => usage.data && usage.data.external_sources,
  (saved) => {
    if (!saved) return;
    codexChecked.value = !!saved.codex;
    minimaxChecked.value = !!saved.minimax;
  },
  { immediate: true, deep: true }
);

// 勾选变化：先持久化再重扫（顺序由 store 里的 setExternalSources 保证）。
async function onSourceToggle() {
  await withLoading('usageExternal', () =>
    setExternalSources({
      codex: codexChecked.value,
      minimax: minimaxChecked.value,
    })
  );
}

// 本次扫到的外部来源（勾了才有；没勾时后端不扫，返回空数组）。
const externalList = computed(() => (usage.data && usage.data.external) || []);

// 每个外部来源在**当前时间范围**下的展示数据。
//
// **汇总与柱状图都按范围算**：内核那几块（热力图 / 趋势 / 模型用量）全都跟着
// 范围切换，外部这块若还摆着 90 天合计，一张卡里就会并存两个窗口——大数字是
// 90 天的、旁边的柱状图却是近 7 天的（2026-10-24 评审发现）。
//
// 与内核那份**不共用** `rangeSummary`：那是内核的账，外部来源是另一套口径
// （provider 命名空间不同、token 口径也不同），两者不能相加也不能互相顶替。
// 每个来源**各自**算一份——几个来源之间也不相加（加出来没有意义）。
const externalCards = computed(() =>
  externalList.value.map((src) => {
    const rows = sliceDays(src.days, rangeDays.value);
    const max = Math.max(1, ...rows.map((row) => row.tokens));
    return {
      src,
      summary: summarizeDays(rows),
      bars: rows.map((row) => ({
        date: row.date,
        tokens: row.tokens,
        requests: row.requests,
        height: (row.tokens / max) * 100,
      })),
    };
  })
);

// 几个外部工具的模型明细**合到一张表**。
//
// **每行标出它来自哪个工具**：两边的 provider 命名空间不同（外部那份是
// `minimax` / `custom`，内核那份是 `minimax-cn` / `opencode-go`），合在一张
// 表里若不标来源，光看 provider 那一列读不出是哪家。
//
// **占比是「占该工具自己」的比重**，不是占全表：两个工具的 token 口径不同
// （Codex 的缓存命中是输入的**子集**、总量取自报的 `total_tokens`；MiniMax 的
// 输入**不含**缓存读、总量是四个分量相加），把它们加在一起算占比等于拿两种
// 定义做除数——那个数没有意义。
const externalModels = computed(() => {
  const rows = [];
  for (const card of externalCards.value) {
    for (const m of card.summary.models) {
      rows.push({
        key: `${card.src.source}:${m.key}`,
        model: m.model,
        provider: m.provider,
        tokens: m.tokens,
        tool: card.src.label,
        toolTotal: card.summary.tokens || 1,
      });
    }
  }
  rows.sort((a, b) => b.tokens - a.tokens || (a.key < b.key ? -1 : 1));
  return rows;
});

// 时间范围：Rust 恒返回完整 90 天，这里只做展示层切片；默认落在 7 天。
const rangeDays = ref(7);
const rangeLabel = computed(
  () => (RANGE_OPTIONS.find((opt) => opt.days === rangeDays.value) || {}).label || '90 天'
);
const rangeDaysView = computed(() => sliceDays(data.value && data.value.days, rangeDays.value));
const rangeSummary = computed(() => summarizeDays(rangeDaysView.value));
const rangeModels = computed(() => rangeSummary.value.models);

const rangeText = computed(() => (rangeDays.value === 1 ? '今日' : `近 ${rangeLabel.value}`));

const summaries = computed(() => {
  const s = rangeSummary.value;
  // 今日用量与所选范围无关，取后端那一份（按本地日历日精确匹配）。此前这里
  // 取 `sliceDays(days, 1)` 的最后一天，而后端会把晚于今天的异常日期追加到
  // 序列末尾——于是窗口与概览卡片对「今日」有两套定义，同一份统计会显示两个数。
  const today = todayUsage(data.value);
  const top = s.models[0];
  return [
    {
      label: '今日用量',
      value: formatTokens(today.tokens),
      title: `今天 ${today.tokens} tokens · ${today.requests} 次`,
    },
    {
      label: '日均用量',
      value: formatTokens(s.tokens / Math.max(1, rangeDays.value)),
      title: `${rangeText.value}共 ${s.tokens} tokens`,
    },
    { label: '请求次数', value: String(s.requests), title: `${rangeText.value}共 ${s.requests} 次` },
    { label: '活跃天数', value: String(s.activeDays), title: `选定范围共 ${rangeDays.value} 天` },
    {
      label: '最常用模型',
      // 只显示模型名，provider 前缀（zai-coding-cn 这类）挪进 tooltip；
      // 完整键仍在 title 里可查。
      value: top ? top.model || top.key : '—',
      title: top ? `${top.key} · ${formatTokens(top.tokens)} tokens` : '',
      // 跨两列：与「活跃天数」同排凑满两行，不再单独占一行。
      wide: true,
    },
  ];
});

// 热力图：格子带分级与悬浮说明。
const heatmap = computed(() => {
  const days = rangeDaysView.value;
  const levels = heatLevels(days);
  return heatmapColumns(days).map((column) =>
    column.map((cell) => {
      if (!cell) return null;
      const index = days.indexOf(cell);
      return { ...cell, level: levels[index] };
    })
  );
});

// 趋势图：堆叠段 + 坐标轴刻度。
const trend = computed(() => stackTrend(rangeDaysView.value, rangeModels.value));
const trendRows = computed(() => trend.value.rows);
const trendMax = computed(() => trend.value.max);
const yTicks = computed(() => [0.25, 0.5, 0.75, 1].map((p) => ({ p, label: formatTokens(trendMax.value * p) })));
// 刻度日期：从柱槽中心取最多 6 个，M/D 短格式。
const xTicks = computed(() => {
  const rows = trendRows.value;
  if (!rows.length) return [];
  const count = Math.min(6, rows.length);
  const picked = new Set();
  for (let i = 0; i < count; i += 1) {
    picked.add(Math.round((i * (rows.length - 1)) / (count - 1 || 1)));
  }
  return [...picked].map((index) => {
    const parts = rows[index].date.split('-');
    // 柱体使用等宽槽位，刻度应落在槽位中心；单日数据自然位于 50%。
    const position = ((index + 0.5) / rows.length) * 100;
    return {
      index,
      label: `${Number(parts[1])}/${Number(parts[2])}`,
      position: Math.min(96, Math.max(4, position)),
    };
  });
});

function partColor(key) {
  if (key === '其他') return '#6b7492';
  const index = rangeModels.value.findIndex((m) => m.key === key);
  return modelColor(index);
}

// --- 趋势柱状图 hover 明细浮层 ---------------------------------------------
// 一个共享浮层跟随光标（90 根柱子各挂一个 popper 太重），内容是当天的
// 按模型明细（模型名 / 用量 / 占比），靠边自动翻转到另一侧。

const trendPlot = ref(null);
const tipEl = ref(null);

// 模型明细的滚动条只在滚动期间显形（与日志正文同一套 is-scrolling 交互，
// 共享 logs.js::bindScrollAutoHide）；列表在 data 到达后才挂载，watch 引用。
const listEl = ref(null);
let unbindListScroll = null;
watch(listEl, (el) => {
  unbindListScroll?.();
  unbindListScroll = bindScrollAutoHide(el);
});
onUnmounted(() => unbindListScroll?.());
const hover = ref(null); // { row, x, y }，x/y 为光标在 plot 内的坐标

function showHover(row, event) {
  const plot = trendPlot.value;
  if (!plot) return;
  const rect = plot.getBoundingClientRect();
  hover.value = { row, x: event.clientX - rect.left, y: event.clientY - rect.top };
}

const onBarEnter = (row, event) => showHover(row, event);
const onBarMove = (row, event) => showHover(row, event);
const onBarLeave = () => {
  hover.value = null;
};

const hoverRow = computed(() => hover.value && hover.value.row);

// 浮层水平位置按**实测宽度**钳制在绘图区内（贴光标右侧，放不下就整体
// 左移）——绝对不能越出 plot，否则会把主容器撑出横向滚动条。transform
// 只做垂直居中，不参与翻转；垂直中心夹在 72–78px，138px 高的浮层不会
// 越出 150px 高的绘图区。
const tipStyle = computed(() => {
  const state = hover.value;
  if (!state) return {};
  const plot = trendPlot.value;
  const width = plot ? plot.clientWidth : 0;
  const tipWidth = tipEl.value ? tipEl.value.offsetWidth : 240;
  const maxLeft = Math.max(8, width - tipWidth - 8);
  const left = Math.min(Math.max(state.x + 14, 8), maxLeft);
  return { left: left + 'px', top: Math.min(Math.max(state.y, 72), 78) + 'px' };
});

// 环形图：r=48，周长 2πr；stroke-dashoffset 负偏移把每段推到起点。
const CIRCUMFERENCE = 2 * Math.PI * 48;
const slices = computed(() => donutSlices(rangeModels.value));

function sliceStyle(slice) {
  const seg = Math.max(0.8, slice.ratio * CIRCUMFERENCE - 1.5);
  return {
    stroke: slice.key === '其他' ? '#6b7492' : modelColor(slices.value.indexOf(slice)),
    'stroke-dasharray': `${seg} ${CIRCUMFERENCE - seg}`,
    'stroke-dashoffset': -(slice.start * CIRCUMFERENCE),
  };
}

const listRows = computed(() => rangeModels.value);

const scannedAt = computed(() => {
  const ms = data.value && data.value.last_scanned_at_ms;
  if (!ms) return '尚未扫描';
  return new Date(ms).toLocaleString();
});

const empty = computed(() => rangeSummary.value.tokens === 0);

// --- 热力图 hover 明细浮层 ---------------------------------------------------
// 与趋势图同一思路：一个共享浮层贴着格子下方（日期 + 星期 + 总用量 +
// 请求次数）。挂在 overview-heat 容器上（它不是滚动容器，不会被裁剪），
// 水平按容器宽度钳制，绝不越出窗口造成横向滚动。

const heatBox = ref(null);
const heatHover = ref(null); // { cell, x, y }，x/y 为光标在容器内的坐标

const onHeatEnter = (cell, event) => showHeatHover(cell, event);
const onHeatMove = (cell, event) => showHeatHover(cell, event);
const onHeatLeave = () => {
  heatHover.value = null;
};

function showHeatHover(cell, event) {
  const box = heatBox.value;
  if (!box) return;
  const rect = box.getBoundingClientRect();
  heatHover.value = { cell, x: event.clientX - rect.left, y: event.clientY - rect.top };
}

const heatTipStyle = computed(() => {
  const state = heatHover.value;
  if (!state) return {};
  const box = heatBox.value;
  const width = box ? box.clientWidth : 200;
  // 浮层宽约 180，中心钳在容器内（min-width 0 的容器最窄也有 ~200）。
  const left = Math.min(Math.max(state.x, 95), Math.max(95, width - 95));
  return { left: left + 'px', top: state.y + 16 + 'px' };
});

const heatTipCell = computed(() => heatHover.value && heatHover.value.cell);
</script>

<template>
  <ViewerShell shell-class="usagewin" title="模型用量">
    <header class="usagewin-head">
      <img src="/whale-icon.png" alt="" width="22" height="22" />
      <span class="usagewin-title">模型用量</span>
      <el-tooltip :content="RETENTION_TIP" placement="bottom-start">
        <el-icon class="usagewin-info"><InfoFilled /></el-icon>
      </el-tooltip>
      <!-- 时间范围切换：纯展示层切片（Rust 恒返回完整 90 天），不触发重扫 -->
      <div class="usagewin-ranges" role="group" aria-label="统计时间范围">
        <button
          v-for="opt in RANGE_OPTIONS"
          :key="opt.days"
          type="button"
          class="usage-range-btn"
          :class="{ active: rangeDays === opt.days }"
          :aria-pressed="rangeDays === opt.days ? 'true' : 'false'"
          @click="rangeDays = opt.days"
        >
          {{ opt.label }}
        </button>
      </div>
      <span class="usagewin-spacer"></span>
      <el-button
        text
        :icon="Refresh"
        :loading="usage.loading"
        title="重新扫描会话记录"
        @click="withLoading('usageRefresh', () => refreshUsage(null, true))"
      >
        刷新
      </el-button>
    </header>

    <!-- 数据来源：内核用量之外，勾选哪些外部 Agent 工具的用量也算进来。
         **默认都不勾**——不勾就不读那些工具留在本机的目录。 -->
    <div class="usage-sources">
      <span class="usage-sources-label">外部工具</span>
      <el-checkbox
        v-model="codexChecked"
        label="Codex"
        :disabled="isLoading('usageExternal')"
        @change="onSourceToggle"
      />
      <el-checkbox
        v-model="minimaxChecked"
        label="MiniMax Desktop"
        :disabled="isLoading('usageExternal')"
        @change="onSourceToggle"
      />
      <el-tooltip :content="EXTERNAL_TIP" placement="bottom-start">
        <el-icon class="usagewin-info"><InfoFilled /></el-icon>
      </el-tooltip>
    </div>

    <main v-loading="usage.loading && !data" class="usagewin-main">
      <template v-if="data">
        <!-- 概览：左栏整体信息（数字瓦片），右栏活跃热力图 -->
        <div class="usage-overview">
          <div class="usage-overview-stats">
            <div
              v-for="s in summaries"
              :key="s.label"
              class="usage-summary-card"
              :class="{ wide: s.wide }"
              :title="s.title || s.value"
            >
              <div class="usage-summary-label">{{ s.label }}</div>
              <div class="usage-summary-value">{{ s.value }}</div>
            </div>
          </div>
          <div ref="heatBox" class="usage-overview-heat">
            <div class="usage-section-head">
              <h3>活跃热力图</h3>
              <span
                class="usage-source-chip"
                title="本壳内核（dsh）的对话用量——不是外部工具的数据"
              >
                内核
              </span>
              <span class="usage-legend">
                较少
                <i v-for="level in 5" :key="level" class="usage-heat-cell" :class="'heat-' + (level - 1)"></i>
                较多
              </span>
            </div>
            <div class="usage-heatmap" role="img" aria-label="按天的用量热力图">
              <div v-for="(column, ci) in heatmap" :key="ci" class="usage-heat-column">
                <template v-for="(cell, ri) in column" :key="ri">
                  <i
                    v-if="cell"
                    class="usage-heat-cell"
                    :class="'heat-' + cell.level"
                    @mouseenter="onHeatEnter(cell, $event)"
                    @mousemove="onHeatMove(cell, $event)"
                    @mouseleave="onHeatLeave"
                  ></i>
                  <i v-else class="usage-heat-cell usage-heat-blank"></i>
                </template>
              </div>
            </div>
            <!-- hover 明细：当天日期 / 星期 / 总用量 / 请求次数 -->
            <div v-if="heatTipCell" class="usage-heat-tip" :style="heatTipStyle">
              <div class="usage-heat-tip-date">{{ heatTipCell.date }} · {{ weekdayLabel(heatTipCell.date) }}</div>
              <div class="usage-heat-tip-row">
                <span class="usage-heat-tip-label">总用量</span>
                <span class="usage-heat-tip-value">{{ formatTokens(heatTipCell.tokens) }} tokens</span>
              </div>
              <div class="usage-heat-tip-row">
                <span class="usage-heat-tip-label">请求</span>
                <span class="usage-heat-tip-value">{{ heatTipCell.requests }} 次</span>
              </div>
            </div>
          </div>
        </div>

        <el-empty v-if="empty" :description="`${rangeText}暂无用量记录：切换更大范围，或使用工作台对话后回来看统计`" />

        <template v-else>
          <!-- 按天 Token 趋势 -->
          <section class="usage-section">
            <div class="usage-section-head">
              <h3>按天 Token 趋势</h3>
              <span
                class="usage-source-chip"
                title="本壳内核（dsh）的对话用量——不是外部工具的数据"
              >
                内核
              </span>
              <span class="usage-section-hint">按模型堆叠；悬停查看当日各模型明细</span>
            </div>
            <div class="usage-trend">
              <div class="usage-trend-y">
                <span v-for="t in yTicks" :key="t.p" class="usage-trend-y-label" :style="{ bottom: t.p * 100 + '%' }">
                  {{ t.label }}
                </span>
              </div>
              <div ref="trendPlot" class="usage-trend-plot">
                <i v-for="t in yTicks" :key="t.p" class="usage-trend-grid" :style="{ bottom: t.p * 100 + '%' }"></i>
                <div class="usage-trend-bars">
                  <div
                    v-for="row in trendRows"
                    :key="row.date"
                    class="usage-trend-bar"
                    @mouseenter="onBarEnter(row, $event)"
                    @mousemove="onBarMove(row, $event)"
                    @mouseleave="onBarLeave"
                  >
                    <template v-if="row.tokens > 0">
                      <i
                        v-for="(part, pi) in row.parts"
                        :key="part.key"
                        class="usage-trend-seg"
                        :style="{
                          height: (part.tokens / trendMax) * 100 + '%',
                          background: partColor(part.key),
                        }"
                      ></i>
                    </template>
                  </div>
                </div>
                <!-- hover 明细浮层：跟随光标、位置钳制在绘图区内，列出当天各模型用量 -->
                <div v-if="hoverRow" ref="tipEl" class="usage-tip" :style="tipStyle">
                  <div class="usage-tip-head">
                    <span>{{ hoverRow.date }}</span>
                    <span class="usage-tip-total">{{ formatTokens(hoverRow.tokens) }} · {{ hoverRow.requests }} 次</span>
                  </div>
                  <template v-if="hoverRow.parts.length">
                    <div v-for="part in hoverRow.parts" :key="part.key" class="usage-tip-row">
                      <i class="usage-tip-chip" :style="{ background: partColor(part.key) }"></i>
                      <span class="usage-tip-model" :title="part.key">{{ modelName(part.key) }}</span>
                      <span class="usage-tip-tokens">{{ formatTokens(part.tokens) }}</span>
                      <span class="usage-tip-pct">{{ formatPercent(part.tokens / (hoverRow.tokens || 1)) }}</span>
                    </div>
                  </template>
                  <div v-else class="usage-tip-row">
                    <span class="usage-tip-model">当天无用量记录</span>
                  </div>
                </div>
                <div class="usage-trend-x">
                  <span
                    v-for="t in xTicks"
                    :key="t.index"
                    class="usage-trend-x-label"
                    :style="{ left: t.position + '%' }"
                  >
                    {{ t.label }}
                  </span>
                </div>
              </div>
            </div>
          </section>

          <!-- 模型用量 -->
          <section class="usage-section usage-model-section">
            <div class="usage-section-head">
              <h3>模型用量</h3>
              <span
                class="usage-source-chip"
                title="本壳内核（dsh）的对话用量——内核也调过 MiniMax 的模型，但那是内核的账，不是外部工具的账"
              >
                内核
              </span>
              <span class="usage-section-hint">{{ rangeText }}合计 {{ formatTokens(rangeSummary.tokens) }} tokens</span>
            </div>
            <div class="usage-models">
              <div class="usage-donut-wrap">
                <svg class="usage-donut" viewBox="0 0 120 120" role="img" aria-label="模型用量占比">
                  <g transform="rotate(-90 60 60)">
                    <circle cx="60" cy="60" r="48" fill="none" stroke="var(--overlay-soft)" stroke-width="14" />
                    <circle
                      v-for="slice in slices"
                      :key="slice.key"
                      cx="60"
                      cy="60"
                      r="48"
                      fill="none"
                      stroke-width="14"
                      :style="sliceStyle(slice)"
                    />
                  </g>
                  <text x="60" y="57" class="usage-donut-total">{{ formatTokens(rangeSummary.tokens) }}</text>
                  <text x="60" y="72" class="usage-donut-label">Tokens 用量</text>
                </svg>
              </div>
              <div ref="listEl" class="usage-model-list">
                <div v-for="(m, index) in listRows" :key="m.key" class="usage-model-row">
                  <i class="usage-model-chip" :style="{ background: modelColor(index) }"></i>
                  <span class="usage-model-name" :title="m.key">{{ m.model || m.key }}</span>
                  <span class="usage-model-provider" :title="`内核的 provider · ${m.provider}`">
                    {{ m.provider }}
                  </span>
                  <span class="usage-model-tokens" :title="m.tokens + ' tokens'">{{ formatTokens(m.tokens) }}</span>
                  <span class="usage-model-percent">{{ formatPercent(m.tokens / (rangeSummary.tokens || 1)) }}</span>
                </div>
              </div>
            </div>
          </section>
        </template>

          <!-- 一个工具一块：它自己的合计、它自己的账。**几个工具之间不相加**——
               两边的 token 口径不同（Codex 的缓存命中是输入的子集，MiniMax 的
               输入不含缓存读），加起来是个没有意义的总数。
               数字跟着上面的范围切换器走。 -->
          <section v-if="externalCards.length" class="usage-section">
            <div class="usage-section-head">
              <h3>外部 Agent 工具用量</h3>
              <span class="usage-section-hint">
                本机其他 Agent 工具的对话用量 · {{ rangeText }} · 口径与上面那份不同，不相加
              </span>
            </div>
            <div class="usage-external">
              <div v-for="card in externalCards" :key="card.src.source" class="usage-external-tool">
                <div class="usage-external-head">
                  <span class="usage-external-name">{{ card.src.label }}</span>
                  <span
                    class="usage-external-total"
                    :title="`${rangeText}共 ${card.summary.tokens} tokens · 近 ${RETENTION_DAYS} 天合计 ${card.src.tokens} tokens（${card.src.requests} 次请求 / ${card.src.active_days} 个活跃日）`"
                  >
                    {{ formatTokens(card.summary.tokens) }}
                  </span>
                </div>
                <div class="usage-external-meta">
                  <span>{{ rangeText }}用量</span>
                  <span>{{ card.summary.requests }} 次请求</span>
                  <span>{{ card.summary.activeDays }} 个活跃日</span>
                  <!-- 会话文件数只有 Codex 那种「一个会话一个文件」的来源才有；
                       MiniMax 是一份 SQLite，没有文件概念（后端给 0，前端不显示）。 -->
                  <span v-if="card.src.tracked_files">{{ card.src.tracked_files }} 个会话文件</span>
                  <span v-if="card.src.source_path" class="usage-external-path" :title="card.src.source_path">
                    {{ card.src.source_path }}
                  </span>
                </div>
                <!-- 有话要说就说（没装 / 读失败 / 数据可能不是最新的）。 -->
                <div v-if="card.src.note" class="usage-external-note">{{ card.src.note }}</div>
                <div
                  v-else
                  class="usage-external-days"
                  role="img"
                  :aria-label="`${card.src.label} 按天用量（${rangeText}）`"
                >
                  <i
                    v-for="row in card.bars"
                    :key="row.date"
                    class="usage-external-day"
                    :style="{ height: Math.max(2, row.height) + '%' }"
                    :title="`${row.date} · ${formatTokens(row.tokens)} tokens · ${row.requests} 次`"
                  ></i>
                </div>
              </div>

              <!-- 模型明细：几个工具的行**合到一张表**。合表之后每行必须自带
                   来源（两边的 provider 命名空间不同，光看 provider 分不出是哪家），
                   占比则是「占该工具自己」的比重——两个工具的 token 口径不同，
                   合起来算占比没有意义。 -->
              <div v-if="externalModels.length" class="usage-external-models">
                <div class="usage-ext-table usage-ext-head">
                  <span></span>
                  <span>模型</span>
                  <span>工具</span>
                  <span>provider</span>
                  <span class="usage-ext-num">tokens</span>
                  <span class="usage-ext-num">占该工具</span>
                </div>
                <div
                  v-for="(m, index) in externalModels"
                  :key="m.key"
                  class="usage-ext-table usage-ext-row"
                >
                  <i class="usage-model-chip" :style="{ background: modelColor(index) }"></i>
                  <span class="usage-model-name" :title="`${m.tool} · ${m.model || m.key}`">
                    {{ m.model || m.key }}
                  </span>
                  <span class="usage-model-tool" :title="`来自 ${m.tool}`">{{ m.tool }}</span>
                  <span class="usage-model-provider" :title="`${m.tool} 的 provider · ${m.provider}`">
                    {{ m.provider }}
                  </span>
                  <span class="usage-model-tokens" :title="m.tokens + ' tokens'">
                    {{ formatTokens(m.tokens) }}
                  </span>
                  <span
                    class="usage-model-percent"
                    :title="`占 ${m.tool} 的 ${formatPercent(m.tokens / m.toolTotal)}`"
                  >
                    {{ formatPercent(m.tokens / m.toolTotal) }}
                  </span>
                </div>
              </div>
            </div>
          </section>
      </template>
    </main>

    <!-- 窗口状态条：永远吸附在最底部，不随内容区滚动。 -->
    <footer class="usage-footer">
      <template v-if="data">
        统计范围：{{ rangeText }} · 上次扫描：{{ scannedAt }} · 记录 {{ data.tracked_files }} 个会话文件 ·
        账目只保留最近 {{ RETENTION_DAYS }} 天，更早的记录已自动丢弃
      </template>
      <template v-else>正在扫描会话记录…</template>
    </footer>
  </ViewerShell>
</template>

<style scoped>
.usagewin {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--window);
}
.usagewin-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
  flex: none;
}
.usagewin-title {
  font-weight: 700;
  font-size: var(--fs-subtitle);
}
.usagewin-info {
  color: var(--text-secondary);
  cursor: help;
}
.usagewin-spacer {
  flex: 1;
}
/* 时间范围切换：紧凑分段控件（header 空间有限，不用大按钮组）。 */
.usagewin-ranges {
  display: inline-flex;
  gap: 2px;
  margin-left: 6px;
  padding: 2px;
  background: var(--el-fill-color-light);
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 8px;
}
.usage-range-btn {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1;
  padding: 5px 9px;
  border-radius: 6px;
  cursor: pointer;
}
.usage-range-btn:hover {
  color: var(--text);
}
.usage-range-btn.active {
  background: var(--accent);
  color: #fff;
  font-weight: 600;
}
/* 趋势图 hover 明细浮层：跟随光标，pointer-events 不挡柱子；
   transform 只做垂直居中（水平位置由脚本钳制，见 tipStyle）。 */
.usage-tip {
  position: absolute;
  z-index: 20;
  min-width: 200px;
  max-width: 280px;
  max-height: 138px;
  overflow-y: auto;
  background: var(--surface-raised);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: var(--el-box-shadow-light);
  padding: 8px 10px;
  pointer-events: none;
  font-size: 12px;
  transform: translate(0, -50%);
}
.usage-tip-head {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  color: var(--text-secondary);
  margin-bottom: 6px;
}
.usage-tip-total {
  color: var(--text);
  font-weight: 600;
  white-space: nowrap;
}
.usage-tip-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 0;
  min-width: 0;
}
.usage-tip-chip {
  width: 8px;
  height: 8px;
  border-radius: 2px;
  flex: none;
}
.usage-tip-model {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-tip-tokens {
  font-weight: 600;
  margin-left: 8px;
}
.usage-tip-pct {
  color: var(--text-secondary);
  min-width: 44px;
  text-align: right;
}
.usagewin-main {
  flex: 1;
  /* 纵向 flex：内容不足一屏时由模型明细段（flex: 1 1 0）吸收剩余
     空间，页脚贴底；超出时整体滚动。 */
  display: flex;
  flex-direction: column;
  /* 纵向**要能滚**：外部来源那一块（勾选后才有）是 main 的最后一个子元素，
     而 760×800 的窗口装不下「概览 + 趋势 + 模型明细 + 外部卡片」四段——
     这里若是 hidden，超出的那段会被整段裁掉、用户永远滚不到（2026-10-24
     实测：外部段标题露在页脚上方，两张卡片只剩一条边）。
     内部那几段仍按 flex 分配高度（模型明细段可收缩、内部自带滚动），
     所以不勾外部来源时内容装得下，滚动条不会出现。 */
  overflow-y: auto;
  /* 横向一律不出滚动条：热力图自己带滚动容器；趋势图 hover 浮层的
     位置在脚本里钳制在绘图区内，这里是最后的安全网。 */
  overflow-x: hidden;
  /* 细滚动条 + 预留槽位：滚动条出现时内容宽度不变（不勾外部来源时也
     预留，于是那一档宽度恒定，不会在勾选瞬间整块重排）。 */
  scrollbar-gutter: stable;
  scrollbar-width: thin;
  scrollbar-color: var(--overlay-strong) transparent;
  padding: 8px 14px 8px;
}
/* 概览两栏：左栏整体信息（数字瓦片），右栏活跃热力图。
   720px 最小窗宽下左 ~370 / 右 ~290，热力图 13 周列加图标配得下。 */
.usage-overview {
  display: flex;
  gap: 12px;
  align-items: stretch;
}
.usage-overview-stats {
  flex: 1;
  min-width: 0;
  display: grid;
  /* 3 列第一行（数字瓦片）+ 1 列第二行（最常用模型，整行宽）。
     删掉「总用量」瓦片后 4 → 3+1，窄窗不会被挤换行；紧凑 padding 把
     双行总高压到接近原本 4 卡单行的高度。 */
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 5px;
  align-content: start;
}
/* 右栏宽度贴热力图内容（不摊满剩余空间），空档归左栏的瓦片；
   格子 9px + 2px 间距，13 周列约 143px，整块更紧凑。 */
.usage-overview-heat {
  flex: 0 0 auto;
  min-width: 0;
  position: relative;
  border-left: 1px solid var(--el-border-color-extra-light);
  padding-left: 10px;
}
.usage-summary-card.wide {
  grid-column: span 2;
}
.usage-summary-card {
  background: var(--el-fill-color-light);
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 8px;
  /* 紧凑：4/8（之前 6/10），总高比单行 4 卡还省；
     长标签交给 ellipsis + title。 */
  padding: 4px 9px;
  min-width: 0;
}
.usage-summary-label {
  color: var(--text-secondary);
  font-size: 12px;
  /* 行高压到 1.35：全局 1.5 的行高让这行占 18px（实测），一行标签省 3px。 */
  line-height: 1.35;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-summary-value {
  font-size: 17px;
  font-weight: 700;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-section {
  margin-top: 12px;
  /* main 里的各段**都不许被压扁**（`flex-shrink: 0`）：内容超出一屏时由 main
     自己滚（`overflow-y: auto`），而不是把某一段挤成「内容比盒子高」——那一段
     的内容于是溢出它的盒子，撞上下一段。
     靠 `min-height: auto`（自动最小尺寸）托不住：自动最小尺寸按 **min-content**
     算，而外部那一段里有**百分比高度**的日柱条（高度百分比在高度不定的祖先里
     按 auto 处理），min-content 高度因此偏小，那一段照样会被压扁。
     实测：外部段 clientHeight 349 / scrollHeight 480。 */
  flex-shrink: 0;
}
.usage-model-section {
  display: flex;
  flex-direction: column;
  /* `flex: 1 1 auto` 而不是 `1 1 0`：有富余空间时这一段照样吸收（圆环保持垂直
     居中），内容超出一屏时它**不许被压扁**——`flex-shrink: 0` 加上
     `min-height: auto`（默认的自动最小尺寸）两道保险，超出部分交给
     `.usagewin-main` 统一滚。
     此前这里是 `flex: 1 1 0` + `min-height: 252px`：basis 为 0、又被 252px 的
     下限钉住，内容一多就只剩 252px，于是模型列表在自己容器里滚（内层滚动条），
     而窗口右侧同时还有外层那条——两条滚动条抢同一份内容。
     **别再加 `min-height: 0`**：那会亲手关掉自动最小尺寸，这一段立刻恢复被
     压扁的形状。 */
  flex: 1 0 auto;
}
.usage-model-section .usage-section-head {
  flex: none;
}
.usage-section-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-bottom: 10px;
}
.usage-section-head h3 {
  margin: 0;
  font-size: var(--fs-subtitle);
}
.usage-section-hint {
  color: var(--text-secondary);
  font-size: 12px;
}
/* 数据来源标记：这屏同时摆着**本壳内核**的用量与**勾选的外部工具**的用量，
   两套口径不能混。两边名字都带「MiniMax」时（内核也调过 MiniMax 的模型），
   用户分不清哪一行是谁的账——所以每一块都挂一枚来源牌。
   内核那块挂「内核」，外部那块由卡片标题给出工具名。 */
.usage-source-chip {
  display: inline-flex;
  align-items: center;
  padding: 0 7px;
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  background: var(--surface-subtle);
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 16px;
  white-space: nowrap;
}
.usage-legend {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-secondary);
  font-size: 12px;
}
/* 热力图那一栏的标题行：**只压这一栏**——`.usage-section-head` 是「按天
   趋势」「模型用量」两段共用的，压全局会把那两段的标题也变小。
   2026-10-25 真机几何实测：标题行 22.5px → 18.0px。 */
.usage-overview-heat .usage-section-head {
  margin-bottom: 5px;
  align-items: center;
}
.usage-overview-heat .usage-section-head h3 {
  /* **字号不许动**：全仓块标题（h2/h3）的下限是 15px
     （designAlignment.test.js「全仓没有低于 15px 的块标题」），那一条守的是
     屏幕的层级，不是这里这一行的高度——所以只压行高：15px × 1.5 的行盒让标题
     行占 22.5px，压到 1.2 是 18px。 */
  line-height: 1.2;
}
.usage-overview-heat .usage-source-chip {
  font-size: 10px;
  line-height: 14px;
  padding: 0 6px;
}
.usage-overview-heat .usage-legend {
  font-size: 11px;
}
.usage-heat-cell {
  /* 9px（之前 12px）：热力图是 7 行 × 13 列，格子每边少 3px 就省下
     21px 的整块高度（实测格子块 100px → 79px）。图例里那几枚也用同一条
     规则，跟着一起变小。 */
  width: 9px;
  height: 9px;
  border-radius: 2px;
  display: inline-block;
}
.usage-heat-blank {
  background: transparent;
}
.heat-0 {
  background: var(--overlay-faint);
}
.heat-1 {
  background: rgba(79, 140, 255, 0.28);
}
.heat-2 {
  background: rgba(79, 140, 255, 0.48);
}
.heat-3 {
  background: rgba(79, 140, 255, 0.72);
}
.heat-4 {
  background: var(--accent);
}
.usage-heatmap {
  display: flex;
  gap: 2px;
  overflow-x: auto;
  padding-bottom: 4px;
}
.usage-heat-column {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
/* 热力图 hover 明细：贴格子下方，水平中心钳在容器内（见 heatTipStyle），
   pointer-events 不挡格子。 */
.usage-heat-tip {
  position: absolute;
  z-index: 20;
  min-width: 172px;
  background: var(--surface-raised);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: var(--el-box-shadow-light);
  padding: 8px 10px;
  pointer-events: none;
  font-size: 12px;
  transform: translate(-50%, 0);
  white-space: nowrap;
}
.usage-heat-tip-date {
  color: var(--text);
  font-weight: 600;
  margin-bottom: 5px;
}
.usage-heat-tip-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 1px 0;
  color: var(--text-secondary);
}
.usage-heat-tip-value {
  color: var(--text);
  font-weight: 600;
}
.usage-trend {
  display: flex;
  gap: 6px;
}
.usage-trend-y {
  position: relative;
  width: 44px;
  height: 150px;
}
.usage-trend-y-label {
  position: absolute;
  right: 0;
  transform: translateY(50%);
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 0;
}
.usage-trend-plot {
  position: relative;
  flex: 1;
  height: 150px;
}
.usage-trend-grid {
  position: absolute;
  left: 0;
  right: 0;
  border-top: 1px dashed var(--el-border-color-extra-light);
}
.usage-trend-bars {
  position: absolute;
  inset: 0 0 20px 0;
  display: flex;
  align-items: flex-end;
  gap: 2px;
}
.usage-trend-bar {
  flex: 1;
  height: 100%;
  min-width: 2px;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  border-radius: 2px 2px 0 0;
  overflow: hidden;
}
.usage-trend-bar:hover {
  outline: 1px solid var(--border);
}
.usage-trend-seg {
  /* 外层 bar 保留完整点位槽位，实际堆叠柱体限制宽度并保持居中，
     这样少量日期不会把柱形无限横向拉宽。 */
  width: 100%;
  max-width: 14px;
  margin-inline: auto;
  display: block;
}
.usage-trend-x {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 30px;
}
.usage-trend-x-label {
  position: absolute;
  bottom: 2px;
  transform: translateX(-50%) rotate(-45deg);
  transform-origin: 50% 100%;
  color: var(--text-secondary);
  font-size: 11px;
  white-space: nowrap;
}
.usage-models {
  display: flex;
  gap: 24px;
  align-items: stretch;
  /* 模型区按内容高度排：内部不再自带滚动，超出部分由 `.usagewin-main` 统一滚。
     `overflow: hidden` 必须去掉——它把这个盒子变成滚动容器，而**滚动容器的
     min-content 高度按 0 算**，于是它上面那一段的自动最小尺寸塌成 0，整段被
     压扁、底部被裁。改成 `visible` 之后，自动最小尺寸按内容算，压不下去。 */
  flex: 1 1 auto;
  overflow: visible;
}
.usage-donut-wrap {
  flex: 0 0 170px;
  display: flex;
  justify-content: center;
  align-items: center;
}
.usage-donut {
  width: 170px;
  height: 170px;
}
.usage-donut circle {
  fill: none;
}
.usage-donut-total {
  fill: var(--text);
  font-size: 20px;
  font-weight: 700;
  text-anchor: middle;
}
.usage-donut-label {
  fill: var(--text-secondary);
  font-size: 10px;
  text-anchor: middle;
}
.usage-model-list {
  flex: 1;
  min-width: 0;
  /* 列表**全部展示**，不出内部纵向滚动条（2026-10-25 用户要求）。此前这里是
     `overflow-y: auto` + 一整套滚动条样式：模型行一多，列表在 760px 宽的窗口里
     只露出前几行，剩下那一截要拖一条**内嵌**滚动条才看得到，而窗口右侧同时
     还有外层那条——用户面对两条抢同一份内容的滚动条，分不清该拖哪条。
     现在列表按内容高度排，超出部分由 `.usagewin-main` 那一条滚动条负责。 */
  margin-bottom: 8px;
}
.usage-model-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 2px;
  border-bottom: 1px solid var(--el-border-color-extra-light);
  font-size: 13px;
}
.usage-model-chip {
  width: 10px;
  height: 10px;
  border-radius: 3px;
  flex: none;
}
.usage-model-name {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-model-provider {
  color: var(--text-secondary);
  font-size: 12px;
  max-width: 30%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-model-tokens {
  font-weight: 600;
  min-width: 52px;
  text-align: right;
}
.usage-model-percent {
  color: var(--text-secondary);
  min-width: 52px;
  text-align: right;
}
/* 窗口状态条：flex 布局的最后一个子项，永远吸附在窗口最底部；
   内容区滚动时它保持不动。 */
.usage-footer {
  flex: none;
  padding: 7px 14px;
  border-top: 1px solid var(--border);
  background: var(--overlay-faint);
  color: var(--text-secondary);
  font-size: 12px;
}
/* 数据来源行：勾哪些外部 Agent 工具的用量也算进来。
   放在 header 之下而不是塞进 header——header 里已经有图标、ⓘ、范围切换与
   刷新按钮，再挤两个 checkbox 就读不出「哪一格是什么」了。 */
.usage-sources {
  display: flex;
  align-items: center;
  gap: 10px;
  /* 4px：这一行只有两个勾选框，7px 的上下内边距把它撑到 47px（实测），
     标注区（这一行 + 概览）在 760×800 的副窗里占了 191.5px 的纵向。 */
  padding: 4px 14px;
  border-bottom: 1px solid var(--border);
  flex: none;
  font-size: 13px;
  /* 行高由这一行**文字的 strut** 撑着（容器字号 13px 乘全局行高 ≈ 18px，
     比 14px 的勾选框还高）——不显式压它，这一行就矮不下去。
     2026-10-25 实测：27px → 23.4px。 */
  line-height: 1.2;
}
.usage-sources-label {
  color: var(--text-secondary);
  font-size: 12px;
}
/* Element Plus 的 checkbox 自带 `height: 32px`（EP 的 .el-checkbox 规则）——
   那一行明明只有两个勾选框，却被这个 32px 撑成 47px。压掉它，行高回到
   文字本身的高度（2026-10-25 真机几何实测：47px → 27px）。 */
.usage-sources .el-checkbox {
  margin-right: 0;
  height: auto;
  min-height: 0;
  line-height: 1.2;
}
/* **必须写 `:deep()`**：scoped 的 scope id 只落在子组件的**根元素**上，
   `.el-checkbox__label` 是 el-checkbox 内部的 span，拿不到父组件的 scope id
   ——写成 `.usage-sources .el-checkbox__label` 时编译结果是
   `… .el-checkbox__label[data-v-xxx]`，那条规则永远匹配不上（2026-10-25
   在编译产物里核实：`.el-checkbox__label` 上没有 data-v 属性，
   label 的计算字号是 EP 的 14px，不是这里写的 13px）。 */
.usage-sources :deep(.el-checkbox__label) {
  font-size: 13px;
  color: var(--text);
}
/* 外部来源卡：一张卡一个工具。 */
/* 外部那几个工具**合在一张卡里**（2026-10-25 用户提的「数据多了会撑出去，
   把两个 agent 工具整合在一个里面显示」）。原先是两张卡**并排**：760px 的
   窗口一分，每张只剩 340px——90 天那条柱状图每根柱不到 2px、模型名被截成
   「MiniMax-M3.1-Fla…」、百分比贴着卡片边缘。现在一张卡占满整幅宽度，
   柱条与模型名都放得下。 */
.usage-external {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px 14px;
}
/* 工具与工具之间**：那条线是"这是另一个工具的账"，不是装饰。 */
.usage-external-tool + .usage-external-tool {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
}
.usage-external-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  padding-bottom: 9px;
  border-bottom: 1px solid var(--border);
}
.usage-external-name {
  font-size: var(--fs-subtitle);
  font-weight: 600;
}
.usage-external-total {
  font-size: 18px;
  font-weight: 700;
  color: var(--text);
}
.usage-external-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin-top: 8px;
  color: var(--text-secondary);
  font-size: 12px;
}
.usage-external-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}
/* 有话要说（没装 / 读失败 / 数据可能不是最新的）：说清楚，别让 0 读成
   「用得少」。 */
.usage-external-note {
  margin-top: 8px;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.6;
}
/* 按天小柱条：一眼看出活跃日与尖峰，不做交互（明细在下面的模型列表里）。 */
.usage-external-days {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 44px;
  margin-top: 10px;
}
.usage-external-day {
  flex: 1 1 0;
  min-width: 2px;
  background: var(--accent-fill);
  border-radius: 2px 2px 0 0;
}
.usage-external-models {
  margin-top: 14px;
  padding-top: 10px;
  border-top: 1px solid var(--border);
  /* 这张表**全部展示**，不出内部纵向滚动条（2026-10-25 用户要求，与内核那份
     模型列表一致）：行一多就把整页撑长，由 `.usagewin-main` 那一条滚动条负责。
     此前这里是 `max-height: 216px` + `overflow-y: auto`（表自己滚）——于是窗口里
     同时出现两条纵向滚动条：表一条、外层一条，用户分不清该拖哪条，而且外层
     那条拖到底也还是只露出表的前几行。 */
}
/* 表头与行用**同一份网格**：flex 的弹性列做不到表头与行逐列对齐。 */
.usage-ext-table {
  display: grid;
  grid-template-columns: 10px minmax(0, 1fr) 104px 84px 78px 58px;
  gap: 8px;
  align-items: center;
}
.usage-ext-head {
  padding: 0 2px 5px;
  color: var(--text-secondary);
  font-size: 11px;
}
.usage-ext-num {
  text-align: right;
}
.usage-ext-row {
  padding: 3px 2px;
  border-bottom: 1px solid var(--el-border-color-extra-light);
  font-size: 13px;
}
/* 网格列宽已经定死：那个 30% 是给 flex 布局用的，在这里会把 provider
   压成一条。 */
.usage-ext-row .usage-model-provider {
  max-width: none;
}
.usage-model-tool {
  color: var(--text-secondary);
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
