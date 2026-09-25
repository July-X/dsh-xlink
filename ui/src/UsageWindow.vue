<script setup>
// 独立模型用量窗口：概览卡「模型用量」按钮经 open_usage_window 弹出
// （URL ?usage=1 挂载本组件，capability `usage-viewer.json` 只授予
// `get_model_usage`）。四段展示与示意图同构——顶部摘要卡、活跃热力图
// （GitHub 式周列）、按模型堆叠的按天趋势柱状图与模型用量环形图 + 列表；
// 全部 CSS / 内联 SVG，不引图表库。支持时间范围切换（15 ~ 90 天，展示
// 层切片，不触发重扫）；趋势柱状图 hover 出模型明细浮层。窗口打开即强制
// 重扫（usage.js 的 refreshUsage）；ℹ️ tooltip 是 90 天保留策略的告知位。
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue';
import { Refresh, InfoFilled } from '@element-plus/icons-vue';
import { ioActive, isLoading, withLoading } from './loading.js';
import { bindScrollAutoHide } from './logs.js';
import {
  usage,
  refreshUsage,
  formatTokens,
  formatPercent,
  modelColor,
  heatmapColumns,
  heatLevels,
  stackTrend,
  donutSlices,
  sliceDays,
  summarizeDays,
  weekdayLabel,
  RANGE_OPTIONS,
  RETENTION_DAYS,
} from './usage.js';

const RETENTION_TIP =
  `统计最近 ${RETENTION_DAYS} 天的模型用量，超过 ${RETENTION_DAYS} 天的记录自动丢弃。` +
  `账目只存按「天 × 模型」的聚合值，不保存会话内容。`;

// 与管理壳一致：本窗口内 IO 进行中点亮标题栏鲸眼脉冲。
watchEffect(() => {
  document.body.classList.toggle('pulse-active', ioActive.value);
});

onMounted(() => {
  refreshUsage();
});

const data = computed(() => usage.data);

// 时间范围：Rust 恒返回完整 90 天，这里只做展示层切片。
const rangeDays = ref(90);
const rangeLabel = computed(
  () => (RANGE_OPTIONS.find((opt) => opt.days === rangeDays.value) || {}).label || '90 天'
);
const rangeDaysView = computed(() => sliceDays(data.value && data.value.days, rangeDays.value));
const rangeSummary = computed(() => summarizeDays(rangeDaysView.value));
const rangeModels = computed(() => rangeSummary.value.models);

const rangeText = computed(() => `近 ${rangeLabel.value}`);

const summaries = computed(() => {
  const s = rangeSummary.value;
  // 今日用量与所选范围无关：恒取日序列的最后一天（Rust 恒补齐到今天）。
  const today = summarizeDays(sliceDays(data.value && data.value.days, 1));
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
      value: top ? top.key : '—',
      title: top ? `${formatTokens(top.tokens)} tokens` : '',
      // 模型键太长容易溢出，整行宽（瓦片网格的第二行）。4 列变 3+1 比 4
      // 列窄窗更稳，不会被挤到换行；紧凑 padding 让两行总高比单行还省。
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
// 刻度日期：整窗均分 6 个（含首尾），M/D 短格式。
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
    return { index, label: `${Number(parts[1])}/${Number(parts[2])}` };
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
  <div class="usagewin">
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
        @click="withLoading('usageRefresh', () => refreshUsage())"
      >
        刷新
      </el-button>
    </header>

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
                      <span class="usage-tip-model" :title="part.key">{{ part.key }}</span>
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
                    :style="{ left: (t.index / Math.max(1, trendRows.length - 1)) * 100 + '%' }"
                  >
                    {{ t.label }}
                  </span>
                </div>
              </div>
            </div>
          </section>

          <!-- 模型用量 -->
          <section class="usage-section">
            <div class="usage-section-head">
              <h3>模型用量</h3>
              <span class="usage-section-hint">{{ rangeText }}合计 {{ formatTokens(rangeSummary.tokens) }} tokens</span>
            </div>
            <div class="usage-models">
              <div class="usage-donut-wrap">
                <svg class="usage-donut" viewBox="0 0 120 120" role="img" aria-label="模型用量占比">
                  <g transform="rotate(-90 60 60)">
                    <circle cx="60" cy="60" r="48" fill="none" stroke="rgba(255,255,255,0.06)" stroke-width="14" />
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
                  <span class="usage-model-provider">{{ m.provider }}</span>
                  <span class="usage-model-tokens" :title="m.tokens + ' tokens'">{{ formatTokens(m.tokens) }}</span>
                  <span class="usage-model-percent">{{ formatPercent(m.tokens / (rangeSummary.tokens || 1)) }}</span>
                </div>
              </div>
            </div>
          </section>
        </template>
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
  </div>
</template>

<style scoped>
.usagewin {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg);
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
  font-size: 15px;
}
.usagewin-info {
  color: var(--muted);
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
  color: var(--muted);
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
  background: var(--tooltip-bg);
  border: 1px solid var(--tooltip-border);
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
  color: var(--muted);
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
  color: var(--muted);
  min-width: 44px;
  text-align: right;
}
.usagewin-main {
  flex: 1;
  /* 纵向 flex：内容不足一屏时由模型明细段（flex: 1 0 auto）吸收剩余
     空间，页脚贴底；超出时照常整体滚动。 */
  display: flex;
  flex-direction: column;
  /* 纵向同样不出滚动条：各段高度由 flex 分配（模型明细段可收缩，
     内部自带滚动），极端小窗下宁可裁切也不顶出整窗滚动条。 */
  overflow-y: hidden;
  /* 横向一律不出滚动条：热力图自己带滚动容器；趋势图 hover 浮层的
     位置在脚本里钳制在绘图区内，这里是最后的安全网。 */
  overflow-x: hidden;
  padding: 12px 16px 8px;
}
/* 概览两栏：左栏整体信息（数字瓦片），右栏活跃热力图。
   720px 最小窗宽下左 ~370 / 右 ~290，热力图 13 周列加图标配得下。 */
.usage-overview {
  display: flex;
  gap: 16px;
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
  gap: 6px;
  align-content: start;
}
/* 右栏宽度贴热力图内容（不摊满剩余空间），空档归左栏的瓦片；
   格子 12px + 2px 间距，13 周列约 180px，整块更紧凑。 */
.usage-overview-heat {
  flex: 0 0 auto;
  min-width: 0;
  position: relative;
  border-left: 1px solid var(--el-border-color-extra-light);
  padding-left: 14px;
}
.usage-summary-card.wide {
  grid-column: 1 / -1;
}
.usage-summary-card {
  background: var(--el-fill-color-light);
  border: 1px solid var(--el-border-color-extra-light);
  border-radius: 10px;
  /* 紧凑：6/10（之前 8/10），总高比单行 4 卡还省；
     长标签交给 ellipsis + title。 */
  padding: 6px 10px;
  min-width: 0;
}
.usage-summary-label {
  color: var(--muted);
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-summary-value {
  font-size: 18px;
  font-weight: 700;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.usage-section {
  margin-top: 12px;
}
.usage-section-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin-bottom: 10px;
}
.usage-section-head h3 {
  margin: 0;
  font-size: 14px;
}
.usage-section-hint {
  color: var(--muted);
  font-size: 12px;
}
.usage-legend {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--muted);
  font-size: 12px;
}
.usage-heat-cell {
  width: 12px;
  height: 12px;
  border-radius: 3px;
  display: inline-block;
}
.usage-heat-blank {
  background: transparent;
}
.heat-0 {
  background: rgba(255, 255, 255, 0.06);
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
  background: var(--tooltip-bg);
  border: 1px solid var(--tooltip-border);
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
  color: var(--muted);
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
  color: var(--muted);
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
  outline: 1px solid var(--tooltip-border);
}
.usage-trend-seg {
  width: 100%;
  display: block;
}
.usage-trend-x {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 18px;
}
.usage-trend-x-label {
  position: absolute;
  transform: translateX(-50%);
  color: var(--muted);
  font-size: 11px;
  white-space: nowrap;
}
.usage-models {
  display: flex;
  gap: 24px;
  align-items: stretch;
  /* 尽可能吃满剩余高度（grow），空间不足时收缩（shrink）让整窗始终
     无纵向滚动条；自身超高由内部列表滚动消化。 */
  flex: 1 1 0;
  min-height: 0;
  overflow: hidden;
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
  fill: var(--muted);
  font-size: 10px;
  text-anchor: middle;
}
.usage-model-list {
  flex: 1;
  min-width: 0;
  /* 高度完全跟随父段：空间多就多显示几行，空间少就少显示几行，
     行数超出在列表内滚动——整窗不出现纵向滚动条。 */
  min-height: 0;
  overflow-y: auto;
  /* 滚动期间才显滚动条（`.is-scrolling` 由 bindScrollAutoHide 加上，
     800ms 无滚动移除），与日志正文同一套交互节奏。 */
  scrollbar-width: thin;
  scrollbar-color: transparent transparent;
}
.usage-model-list.is-scrolling {
  scrollbar-color: rgba(255, 255, 255, 0.22) transparent;
}
.usage-model-list::-webkit-scrollbar {
  width: 8px;
}
.usage-model-list::-webkit-scrollbar-thumb {
  background: transparent;
  border-radius: 4px;
  border: 2px solid transparent;
  background-clip: padding-box;
  transition: background 0.2s ease;
}
.usage-model-list.is-scrolling::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.22);
}
.usage-model-list.is-scrolling::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.32);
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
  color: var(--muted);
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
  color: var(--muted);
  min-width: 52px;
  text-align: right;
}
/* 窗口状态条：flex 布局的最后一个子项，永远吸附在窗口最底部；
   内容区滚动时它保持不动。 */
.usage-footer {
  flex: none;
  padding: 7px 14px;
  border-top: 1px solid var(--border);
  background: rgba(0, 0, 0, 0.18);
  color: var(--muted);
  font-size: 12px;
}
</style>
