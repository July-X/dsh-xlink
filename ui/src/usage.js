// 模型用量统计：概览卡片「近 7 天用量」与独立「模型用量」窗口的状态、
// 动作与纯展示函数。窗口由 Rust 侧 open_usage_window 弹出（?usage=1 挂载
// UsageWindow.vue），扫描与 90 天窗口聚合都在 Rust 侧（usage.rs）完成——
// 前端只拉取与呈现；B/M/K 单位、热力图分级、趋势堆叠与饼图角度是纯函数，
// 可被 node --test 直接覆盖。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastActionError } from './notify.js';
import { withLoading } from './loading.js';

// 与 Rust RETENTION_DAYS 同源的展示口径：tooltip 里的「超过 90 天自动丢弃」。
export const RETENTION_DAYS = 90;

export const usage = reactive({
  // 独立窗口内 force 重扫的 loading（首次全量可能要几秒）。
  loading: false,
  // 上次成功拉取的时间（Date.now()）；卡片用它在 60s 内免重复请求。
  loadedAt: 0,
  // get_model_usage 的返回；null = 尚未拉到过。
  data: null,
});

// 卡片摘要的最小请求间隔：概览页反复挂载也不该反复触发扫描。
const SUMMARY_TTL_MS = 60_000;

// --- 纯展示函数 ---------------------------------------------------------------

// Token 计数统一走 B / M / K 标准单位（与示意图的标注一致），原值留在
// tooltip / title 里。1e9 以上一位小数，更大数量级整数位不再膨胀。
export function formatTokens(value) {
  const n = Number(value) || 0;
  if (n >= 1e9) return trimUnit(n / 1e9) + 'B';
  if (n >= 1e6) return trimUnit(n / 1e6) + 'M';
  if (n >= 1e3) return trimUnit(n / 1e3) + 'K';
  return String(Math.round(n));
}

// 时间范围切换器的选项（15 / 30 / 60 / 90 天）。Rust 侧恒返回完整 90 天的
// 日序列，切换范围只是展示层切片，不触发重扫。短档位（当天 / 3 / 7 天）
// 已移除：760px 的窗口头部放不下，且范围小于两周时热力图只剩一两列，
// 看不出趋势。窗口默认落在 90 天（完整窗口）。
export const RANGE_OPTIONS = [
  { days: 15, label: '15 天' },
  { days: 30, label: '30 天' },
  { days: 60, label: '60 天' },
  { days: 90, label: '90 天' },
];

// 取窗口末尾的 N 天。days 恒为升序、缺日补零的完整序列（Rust 保证），
// 所以切片天然连续。
export function sliceDays(days, rangeDays) {
  const list = days || [];
  return rangeDays >= list.length ? list : list.slice(list.length - rangeDays);
}

// `YYYY-MM-DD` → 周一…周日，热力图 hover 明细用。解析按 UTC 午夜日期
// 字面量，避免本地时区把日期拨到前一天。
const WEEKDAY_LABELS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'];

export function weekdayLabel(date) {
  if (typeof date !== 'string') return '';
  const parts = date.split('-').map(Number);
  if (parts.length !== 3 || parts.some((n) => !Number.isFinite(n))) return '';
  const [year, month, day] = parts;
  return WEEKDAY_LABELS[(new Date(Date.UTC(year, month - 1, day)).getUTCDay() + 6) % 7];
}

// 把一段日序列聚合成该范围的汇总：总 tokens / 请求次数 / 活跃天数与
// 按模型合计（tokens 降序）。模型的 provider / model 从键拆回，供列表
// 行展示；范围内拿不到每模型的请求次数（日账只存 tokens），需要时再扩
// 后端 DayUsageView。
export function summarizeDays(days) {
  const perModel = new Map();
  let tokens = 0;
  let requests = 0;
  let activeDays = 0;
  for (const day of days || []) {
    tokens += day.tokens;
    requests += day.requests;
    if (day.requests > 0) activeDays += 1;
    for (const [key, modelTokens] of Object.entries(day.models || {})) {
      if (modelTokens > 0) perModel.set(key, (perModel.get(key) || 0) + modelTokens);
    }
  }
  const models = [...perModel.entries()]
    .map(([key, modelTokens]) => {
      const slash = key.indexOf('/');
      return {
        key,
        provider: slash >= 0 ? key.slice(0, slash) : key,
        model: slash >= 0 ? key.slice(slash + 1) : '',
        tokens: modelTokens,
      };
    })
    .sort((a, b) => b.tokens - a.tokens || (a.key < b.key ? -1 : 1));
  return { tokens, requests, activeDays, models };
}

function trimUnit(x) {
  if (x >= 100) return String(Math.round(x));
  const s = x.toFixed(1);
  return s.endsWith('.0') ? s.slice(0, -2) : s;
}

export function formatPercent(ratio) {
  return ((ratio || 0) * 100).toFixed(1) + '%';
}

// 模型配色：按 tokens 降序循环取用的主题同源 8 色。
const MODEL_COLORS = ['#4f8cff', '#22d3ee', '#34d399', '#fbbf24', '#f472b6', '#a78bfa', '#f87171', '#609926'];

export function modelColor(index) {
  return MODEL_COLORS[((index % MODEL_COLORS.length) + MODEL_COLORS.length) % MODEL_COLORS.length];
}

// 热力图 5 档：0 = 无用量；1–4 按非零日的 25/50/75 分位切分（GitHub 风格）。
// 用分位而不是 tokens/max 的比例，单日尖峰才不会把其余日子全压成最浅色。
export function heatLevels(days) {
  const nonzero = (days || [])
    .map((d) => d.tokens)
    .filter((t) => t > 0)
    .sort((a, b) => a - b);
  const pick = (p) => nonzero[Math.min(nonzero.length - 1, Math.floor(p * nonzero.length))];
  const q1 = pick(0.25);
  const q2 = pick(0.5);
  const q3 = pick(0.75);
  return (days || []).map((d) => {
    if (!(d.tokens > 0)) return 0;
    if (d.tokens <= q1) return 1;
    if (d.tokens <= q2) return 2;
    if (d.tokens <= q3) return 3;
    return 4;
  });
}

// 把升序的日序列排成 GitHub 式周列：外层是周（列），内层恒 7 格
// （行 = 周一..周日），首列按第一天的星期补 null 占位。日序列必须连续
// （Rust 侧恒补齐整个窗口的零日）。
export function heatmapColumns(days) {
  const list = days || [];
  if (!list.length) return [];
  const columns = [];
  let column = new Array(isoWeekday(list[0].date)).fill(null);
  for (const day of list) {
    column.push(day);
    if (column.length === 7) {
      columns.push(column);
      column = [];
    }
  }
  if (column.length) {
    while (column.length < 7) column.push(null);
    columns.push(column);
  }
  return columns;
}

// ISO 星期：周一=0 … 周日=6。`YYYY-MM-DD` 按日期字面量解析（UTC 午夜），
// 避免 `new Date('YYYY-MM-DD')` 在本地时区解释下的歧义。
function isoWeekday(date) {
  const [year, month, day] = date.split('-').map(Number);
  return (new Date(Date.UTC(year, month - 1, day)).getUTCDay() + 6) % 7;
}

// 趋势柱状图的堆叠：取用量最大的前 topN 个模型，其余并入「其他」。
// 返回每天的段列表（自下而上即图例顺序）与全窗口最大日用量。
export function stackTrend(days, models, topN = 5) {
  const list = days || [];
  const top = (models || []).slice(0, topN).map((m) => m.key);
  const topSet = new Set(top);
  const rows = list.map((day) => {
    const parts = [];
    let other = 0;
    for (const [key, tokens] of Object.entries(day.models || {})) {
      if (topSet.has(key)) {
        parts.push({ key, tokens });
      } else {
        other += tokens;
      }
    }
    // 段按全窗口模型排序摆放，保证同一模型在所有柱子里颜色一致。
    parts.sort((a, b) => top.indexOf(a.key) - top.indexOf(b.key));
    if (other > 0) parts.push({ key: '其他', tokens: other });
    return { date: day.date, tokens: day.tokens, requests: day.requests, parts };
  });
  const max = Math.max(1, ...rows.map((r) => r.tokens));
  return { rows, max };
}

// 环形图的扇区：前 topN 个模型 + 「其他」。offset/dash 直接是
// stroke-dasharray 可用的弧长（以半径 1 归一，乘上周长即可）。
export function donutSlices(models, topN = 6) {
  const list = models || [];
  const head = list.slice(0, topN);
  const tailTokens = list.slice(topN).reduce((sum, m) => sum + m.tokens, 0);
  const total = list.reduce((sum, m) => sum + m.tokens, 0) || 1;
  const slices = head.map((m) => ({ key: m.key, tokens: m.tokens, ratio: m.tokens / total }));
  if (tailTokens > 0) slices.push({ key: '其他', tokens: tailTokens, ratio: tailTokens / total });
  let acc = 0;
  for (const slice of slices) {
    slice.start = acc;
    acc += slice.ratio;
  }
  return slices;
}

// --- 状态动作 ---------------------------------------------------------------

// 概览卡片「模型用量」按钮：弹出独立窗口（建窗在 Rust 侧新线程完成，
// 与日志「全屏」同一条路）。只挂本按钮 loading 与失败提示——扫描进度
// 由新窗口自己的加载态呈现。
export function openUsageWindow() {
  return withLoading('openUsageWindow', () =>
    invoke('open_usage_window').catch((e) =>
      toastActionError('打开模型用量窗口失败', e, '请重试；若持续失败，请到「查看日志」了解详情', 5000)
    )
  );
}

// 卡片摘要：60s 内直接用上次结果。失败静默（卡片的数字是锦上添花，
// 独立窗口里才有真正的错误提示与重试按钮）。
export async function loadUsageSummary() {
  if (usage.loadedAt && Date.now() - usage.loadedAt < SUMMARY_TTL_MS) return usage.data;
  try {
    usage.data = await invoke('get_model_usage', { force: false });
    usage.loadedAt = Date.now();
  } catch {
    // 静默：下次进入概览页再试。
  }
  return usage.data;
}

// 独立窗口内拉取：打开与「刷新」都强制重扫（force 越过 45s 新鲜度窗口），
// 保证看到的是刚扫完的账。
export async function refreshUsage() {
  usage.loading = true;
  try {
    usage.data = await invoke('get_model_usage', { force: true });
    usage.loadedAt = Date.now();
  } catch (e) {
    toastActionError('读取模型用量失败', e, '请点击「刷新」重试；若持续失败，请到「查看日志」了解详情', 6000);
  } finally {
    usage.loading = false;
  }
}
