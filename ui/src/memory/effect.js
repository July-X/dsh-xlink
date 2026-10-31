// 记忆效果页签的共享状态与纯展示函数（P0：只读聚合，不落账）。
//
// 数据全部来自后端 memory_effect_report 命令（字段与口径见
// docs/features/memory/effectiveness-dashboard-design.md §4.3 / §5 与本文件各函数注释）：
// 会话日志只读扫描 + memory.json / jobs.json 聚合。前端只做格式化与展示，
// 不自行汇总或换算率值——例外只有合同没下发的「覆盖率」这一类派生读数，
// 由这里的 sharePercent 计算；分母不足时面板只显示计数（设计 §4.5）。
//
// 缺字段一律显示「—」而不是 0：「0」是「真的是零」，「—」是「这个版本没有这个数」，
// 两者在验收时读起来完全不同。
import { reactive } from 'vue';
import { invoke } from '../shell/bridge.js';
import { withLoading } from '../shell/loading.js';
import { formatBytes as formatBytesLabel } from '../shell/labels.js';

/** 时间窗只接受这三档（合同 §3.1；后端对其它值 clamp 到最近合法值）。 */
export const EFFECT_RANGES = [7, 30, 90];

/** 样本量下限：低于这个数只给计数、不给百分比（设计 §4.5 / §5.2）。 */
export const MIN_SAMPLE = 30;

const DASH = '—';

export const effectStore = reactive({
  /** memory_effect_report 的最近一次成功返回；失败时保留上一次。 */
  report: null,
  /** 请求是否在途（页面骨架屏与刷新按钮共用）。 */
  loading: false,
  /** 最近一次失败的文案；成功一次就清空。 */
  error: '',
  /** 当前时间窗（7 / 30 / 90）。 */
  rangeDays: 7,
});

/** 数字能否当读数用：null / undefined / '' / NaN / Infinity 都不算。 */
function finiteNumber(value) {
  if (value === null || value === undefined || value === '') return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

export function isRangeDay(days) {
  return EFFECT_RANGES.indexOf(Number(days)) >= 0;
}

export function formatPercent(value) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  return number.toFixed(1) + '%';
}

/**
 * 率值统一入口：后端下发的是 0–100 的百分数（合同 §2.2 的 14.4 即 14.4%），
 * 前端不再乘 100（Lead 2026-10-31 裁决）；越界先 clamp 再显示。
 */
export function formatRatio(value) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  return formatPercent(Math.min(100, Math.max(0, number)));
}

/** 计数：千分位；缺字段回「—」而不是 0。 */
export function formatCount(value) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  return Math.round(number).toLocaleString('zh-CN');
}

/**
 * token 估算：与用量页 formatTokens 同一套单位口径（B / M / K，两位小数），
 * 但缺字段回「—」——效果页所有 token 都是估算值，不能被读成精确数。
 */
export function formatTokens(value) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  const abs = Math.abs(number);
  if (abs >= 1e9) return (number / 1e9).toFixed(2) + 'B';
  if (abs >= 1e6) return (number / 1e6).toFixed(2) + 'M';
  if (abs >= 1e3) return (number / 1e3).toFixed(2) + 'K';
  return String(Math.round(number));
}

/** 字节：复用 labels.js 的 1024 进制口径，只补上「缺字段 = —」。 */
export function formatBytes(value) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  if (number <= 0) return '0 B';
  return formatBytesLabel(number);
}

const DATE_PREFIX = /^(\d{4})-(\d{2})-(\d{2})/;

/** 2026-10-25 → 10-25；认不出时原样返回（空值回「—」）。 */
export function shortDate(value) {
  const text = String(value === null || value === undefined ? '' : value).trim();
  if (!text) return DASH;
  const match = DATE_PREFIX.exec(text);
  return match ? match[2] + '-' + match[3] : text;
}

/**
 * 把一条健康读数归一成 ok / warn / bad / unknown 四档（面板据此上色与写说明）。
 * - 默认「越大越好」：值 >= good 为 ok、>= warn 为 warn、其余 bad。
 * - lowerIsBetter：解码失败数、未知事件占比这类「越小越好」的读数。
 * - 没给阈值或值不是有限数时回 unknown，不猜。
 */
export function healthLevel(value, options) {
  const number = finiteNumber(value);
  if (number === null) return 'unknown';
  const config = options || {};
  const good = finiteNumber(config.good);
  const warn = finiteNumber(config.warn);
  if (good === null && warn === null) return 'unknown';
  if (config.lowerIsBetter === true) {
    if (good !== null && number <= good) return 'ok';
    if (warn !== null && number <= warn) return 'warn';
    return 'bad';
  }
  if (good !== null && number >= good) return 'ok';
  if (warn !== null && number >= warn) return 'warn';
  return 'bad';
}

/**
 * 计数占比 → 0–100 的百分数；分母缺失或为 0 时回 null（面板显示「—」）。
 *
 * 这是唯一由前端派生的读数：合同 §2.2 只下发会话 / 注入计数，没有下发覆盖率字段
 * （设计 §5.2 的读数条要「12 / 48 会话 · 覆盖率 25%」）。公式只看这两个计数：
 * 触达会话 / 活跃会话。
 */
export function sharePercent(part, total) {
  const numerator = finiteNumber(part);
  const denominator = finiteNumber(total);
  if (numerator === null || denominator === null || denominator <= 0) return null;
  return (numerator / denominator) * 100;
}

/** 样本量够不够给百分比（设计：分母 < 30 只给计数，不给比率）。 */
export function sampleEnough(count) {
  const value = finiteNumber(count);
  return value !== null && value >= MIN_SAMPLE;
}

/** 后端错误转一句用户能读的话；命令返回的具体原因优先保留。 */
function effectErrorText(error) {
  if (typeof error === 'string' && error) return error;
  if (error && typeof error.message === 'string' && error.message) return error.message;
  return '读取记忆效果数据失败。';
}

/** 当前窗口有没有可用缓存：只有同窗口的成功报告才算（换档 / 强制刷新必重读）。 */
function canReuseReport() {
  const report = effectStore.report;
  return !!(report && report.range_days === effectStore.rangeDays);
}

/**
 * 读记忆效果报告。
 * - 首次进入 / 时间窗变化才真的发命令；同一窗口再次进入吃缓存（切页签不重扫）。
 * - options.force 是「刷新」：跳过缓存强制重扫。
 * - 失败保留上一次成功数据、只设 error，页面不会因此变空。
 * - 返回值是「这一次能展示的报告」（成功是最新那份，失败是上一次成功的那份）。
 */
export function loadEffectReport(options) {
  const force = !!(options && options.force);
  if (!isRangeDay(effectStore.rangeDays)) effectStore.rangeDays = 7;
  if (!force && canReuseReport()) return Promise.resolve(effectStore.report);
  const rangeDays = effectStore.rangeDays;
  effectStore.loading = true;
  effectStore.error = '';
  return withLoading('memoryEffect', function () {
    // Tauri 命令参数在 IPC 侧是 camelCase（与 memory_preview_context 的 queryText 同一条约定），
    // 只有返回值是后端 serde 的 snake_case。传错名字不会报错，只会让 range_days 取缺省值。
    return invoke('memory_effect_report', { rangeDays: rangeDays })
      .then(function (report) {
        if (!report || typeof report !== 'object') {
          effectStore.error = '后端没有返回效果数据。';
          return effectStore.report;
        }
        effectStore.report = report;
        return report;
      })
      .catch(function (error) {
        effectStore.error = effectErrorText(error);
        return effectStore.report;
      })
      .finally(function () {
        effectStore.loading = false;
      });
  });
}

/**
 * 切时间窗：只接受 7/30/90，其它值原样忽略（不猜也不 clamp，免得点一下跳到意外窗口）；
 * 同一窗口不重复发命令。
 */
export function setEffectRange(days) {
  if (!isRangeDay(days)) return Promise.resolve(effectStore.report);
  const next = Number(days);
  if (next === effectStore.rangeDays && canReuseReport()) return Promise.resolve(effectStore.report);
  effectStore.rangeDays = next;
  return loadEffectReport();
}

// —— P1：原因映射、账本 / 对比 / 下钻的展示函数 -----------------------------

/** 合同 §1.2 的 reason 枚举 → 中文标签；认不出的原样返回（新原因先看得见，不吞成「未知」）。 */
const REASON_LABELS = {
  turn: '新回合',
  library_change: '库变化',
  probe: '探测',
  disabled: '开关关闭',
  no_hit: '零命中',
  budget: '预算/本回合不取',
  bridge_failed: '桥失败',
  no_change: '内容未变',
};

export function reasonText(reason) {
  if (reason === null || reason === undefined || reason === '') return DASH;
  const text = String(reason);
  return REASON_LABELS[text] || text;
}

/** 账本事件类型：inject / skip。 */
const KIND_LABELS = { inject: '注入', skip: '跳过' };

export function kindText(kind) {
  if (kind === null || kind === undefined || kind === '') return DASH;
  const text = String(kind);
  return KIND_LABELS[text] || text;
}

/** 95% 置信区间文本：-0.51 ~ -0.05；缺一端回「—」。 */
export function formatCi(low, high) {
  const lower = finiteNumber(low);
  const upper = finiteNumber(high);
  if (lower === null || upper === null) return DASH;
  return lower.toFixed(2) + ' ~ ' + upper.toFixed(2);
}

/**
 * 置信区间是否跨 0（含端点）。跨 0 时面板标「未见差异」——**不做显著 / 不显著的
 * 颜色结论**：那是统计软件的话，不是这个面板该下的判断（合同 §3.1）。
 */
export function ciCrossesZero(low, high) {
  const lower = finiteNumber(low);
  const upper = finiteNumber(high);
  if (lower === null || upper === null) return false;
  return lower <= 0 && upper >= 0;
}

/** 对比指标按 unit 格式化：%/token/次；其它单位给两位小数。 */
export function formatMetricValue(value, unit) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  if (unit === '%') return number.toFixed(1) + '%';
  if (unit === 'token') return formatTokens(number);
  if (unit === '次') return number.toFixed(2) + ' 次';
  return number.toFixed(2);
}

/** 带符号的差值：+0.28 / -0.28；0.00 不带符号（"-0.00" 读起来像 bug）。 */
export function formatSigned(value) {
  const number = finiteNumber(value);
  if (number === null) return DASH;
  const fixed = number.toFixed(2);
  if (Number(fixed) === 0) return fixed;
  return number > 0 ? '+' + fixed : fixed;
}

/** 会话 id 短串：session-00a67abf-… → 00a67abf（完整 id 进 title / 下钻命令）。 */
export function shortSessionId(value) {
  const text = String(value === null || value === undefined ? '' : value).trim();
  if (!text) return DASH;
  const prefixed = /^session-([0-9a-f]{8})/i.exec(text);
  if (prefixed) return prefixed[1];
  const bareHex = /^([0-9a-f]{8})(?:-|$)/i.exec(text);
  if (bareHex) return bareHex[1];
  const bare = text.startsWith('session-') ? text.slice('session-'.length) : text;
  return bare.slice(0, 8) || DASH;
}

/**
 * 时间短串：ISO / 毫秒时间戳 → 本地「10-31 20:40」；认不出回「—」。
 * 与 memory.js 的 shortTime 不同：这里不带年份（账本最近事件都在窗口内），缺省是「—」。
 */
export function timestampShort(value) {
  if (value === null || value === undefined || value === '') return DASH;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return DASH;
  const pad = function (n) { return String(n).padStart(2, '0'); };
  return pad(date.getMonth() + 1) + '-' + pad(date.getDate()) + ' ' + pad(date.getHours()) + ':' + pad(date.getMinutes());
}

/**
 * 版本短串：插件 / 内核版本表列窄，只做「去掉提取器公共前缀 + unknown 归一」，
 * 不做截断（版本号截断后两个版本会看起来一样）。
 */
export function shortVersion(value) {
  const text = String(value === null || value === undefined ? '' : value).trim();
  if (!text) return DASH;
  if (text === 'unknown') return '未知';
  const prefix = 'xlink-memory-';
  return text.startsWith(prefix) ? text.slice(prefix.length) : text;
}

// —— 会话下钻（memory_effect_session） --------------------------------------

export const sessionStore = reactive({
  /** 当前请求的会话 id（原始 id，进命令）。 */
  sessionId: '',
  /** memory_effect_session 的最近一次成功返回；失败保留同会话的下钻数据。 */
  timeline: null,
  loading: false,
  error: '',
});

/** 清空下钻状态（关闭弹窗时调，避免下次打开先闪一眼上一个会话）。 */
export function resetSessionTimeline() {
  sessionStore.sessionId = '';
  sessionStore.timeline = null;
  sessionStore.loading = false;
  sessionStore.error = '';
}

/**
 * 读一个会话的时间线（只读、无正文）。
 * - 同一会话再次打开吃缓存；force 是弹窗里的「重试 / 刷新」。
 * - 换会话时先清掉上一条时间线（两个会话的节点混在一张表里比空态更糟）。
 * - 失败保留同会话的上一次数据、只设 error。
 */
export function loadSessionTimeline(sessionId, options) {
  const id = String(sessionId === null || sessionId === undefined ? '' : sessionId).trim();
  if (!id) return Promise.resolve(null);
  const force = !!(options && options.force);
  if (sessionStore.sessionId !== id) sessionStore.timeline = null;
  if (!force && sessionStore.sessionId === id && sessionStore.timeline) {
    return Promise.resolve(sessionStore.timeline);
  }
  sessionStore.sessionId = id;
  sessionStore.loading = true;
  sessionStore.error = '';
  return withLoading('memoryEffectSession', function () {
    // IPC 参数是 camelCase（合同 §5 / 与 memory_effect_report 的 rangeDays 同一条约定）。
    return invoke('memory_effect_session', { sessionId: id })
      .then(function (timeline) {
        if (!timeline || typeof timeline !== 'object') {
          sessionStore.error = '后端没有返回会话时间线。';
          return sessionStore.timeline;
        }
        sessionStore.timeline = timeline;
        return timeline;
      })
      .catch(function (error) {
        sessionStore.error = effectErrorText(error);
        return sessionStore.timeline;
      })
      .finally(function () {
        sessionStore.loading = false;
      });
  });
}
