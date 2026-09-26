// 云端套餐用量：概览卡「套餐用量」行、设置页「测试连接」与独立「套餐用量」
// 窗口共享的状态、动作与纯展示函数。查询、缓存与 keep-last-good 的决策都在
// Rust 侧（subscription.rs）完成——前端只拉取与呈现；进度条配色、倒计时、
// 余额行与收起态摘要文案是纯函数，可被 node --test 直接覆盖。
//
// keep-last-good 在前端显式落地：失败时只写 error、绝不清空 data（Rust 侧
// 缓存同样不写不删），用户看到的数字永远是「上一次成功查询」的结果。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { formatActionError, toastActionError } from './notify.js';
import { withLoading } from './loading.js';
import { countdownFullLabel, countdownLabel, relativeAgeCompact, relativeTimeLabel } from './labels.js';

// 概览卡片摘要的最小请求间隔：与 usage 卡片同款 TTL（后端另有 5 分钟缓存）。
const SUMMARY_TTL_MS = 60_000;

// 与 Rust 侧 kind 字段同源的展示配置。
export const PLAN_KIND = 'plan';
export const BALANCE_KIND = 'balance';

export const subscription = reactive({
  // 独立窗口 / 手动刷新进行中。
  loading: false,
  // 上一次成功拉取的 SubscriptionView；失败时不清空（keep-last-good）。
  data: null,
  // 最近一次失败的用户文案（数组，卡片/窗口内联展示）；成功时置空。
  errors: [],
  loadedAt: 0,
});

// --- 纯展示函数 ---------------------------------------------------------------

/** 剩余百分比 → 配色档位：≥50 绿 / 20–49 警告 / <20 危险（与设计稿一致）。 */
export function percentLevel(percent) {
  const value = Number(percent);
  if (!Number.isFinite(value) || value < 20) return 'danger';
  if (value < 50) return 'warning';
  return 'ok';
}

/** 币种符号；未知币种退回代码本身。 */
export function currencySymbol(currency) {
  if (currency === 'CNY') return '¥';
  if (currency === 'USD') return '$';
  return currency ? `${currency} ` : '';
}

/** 一行余额：`¥110.00（赠送 ¥10.00 · 充值 ¥100.00）`；0 / 缺省的赠送充值不展示。 */
export function balanceText(balance) {
  if (!balance) return '';
  const symbol = currencySymbol(balance.currency);
  const parts = [];
  for (const key of ['granted', 'topped_up']) {
    const raw = balance[key];
    if (typeof raw !== 'string' || !raw.trim()) continue;
    if (!(Number(raw) > 0)) continue;
    parts.push(`${key === 'granted' ? '赠送' : '充值'} ${symbol}${raw.trim()}`);
  }
  const total = `${symbol}${(balance.total || '').trim()}`;
  return parts.length ? `${total}（${parts.join(' · ')}）` : total;
}

/**
 * 单个 provider 的**完整**错误文案（错误横幅用，含可操作的下一步）；
 * 返回 null 表示无错误。
 */
export function providerStateText(provider) {
  if (!provider) return null;
  if (provider.fetch_error) return provider.fetch_error;
  if (provider.credential_status === 'expired') return provider.error || '凭据已失效';
  if (provider.error) return provider.error;
  return null;
}

/**
 * 单个 provider 的**短状态词**（收起态摘要 / 分区内小字标注用，2–6 个字）。
 * 完整的可操作文案只出现在错误横幅，避免「摘要行 / 横幅 / 分区」三处重复
 * 铺满长文案。返回 null 表示正常无状态。
 */
export function providerShortState(provider) {
  if (!provider || !provider.configured) return null;
  if (provider.fetch_error) return '查询失败';
  if (provider.credential_status === 'expired') return '凭据失效';
  if (provider.error) return '查询异常';
  if (provider.kind === PLAN_KIND && provider.tiers.length === 0) return '暂无额度数据';
  return null;
}

/** 展开态「查询于 X 前」（完整中文，hover title 用）；从未成功查询时返回 null。 */
export function queriedAtLabel(provider) {
  if (!provider || !provider.queried_at_ms) return null;
  return relativeTimeLabel(provider.queried_at_ms);
}

/** 数据年龄紧凑值（`<1min` / `12min` / `3h`），配刷新 icon 组成胶囊。 */
export function queriedAgeCompact(provider) {
  if (!provider || !provider.queried_at_ms) return null;
  return relativeAgeCompact(provider.queried_at_ms);
}

/** 进度条一条 tier 的展示数据：短名（5h / 7d）、剩余百分比、档位配色与重置倒计时。 */
export function tierRow(tier, now = Date.now()) {
  if (!tier) return null;
  const name = tier.name === '5h' ? '5h' : tier.name === 'weekly' ? '7d' : tier.name;
  // 无限额度（如 MiniMax 无周限额套餐）：不渲染进度条，以 ♾️ 文本提示。
  if (tier.unlimited) {
    return { name, unlimited: true, percent: null, level: 'ok', countdown: null, countdownTitle: null, tip: '无限周额度' };
  }
  const remaining = Number(tier.remaining_percent);
  const resetsIn = tier.resets_at_ms ? tier.resets_at_ms - now : null;
  return {
    name,
    unlimited: false,
    percent: Math.round(remaining),
    level: percentLevel(remaining),
    // 紧凑倒计时（前置 icon 一起展示）；完整中文描述进 hover title。
    countdown: resetsIn == null ? null : countdownLabel(resetsIn),
    countdownTitle: resetsIn == null ? null : countdownFullLabel(resetsIn),
    // tooltip 双向注明口径（剩余百分比与已用口径相反）。
    tip: `已用 ${Math.max(0, 100 - Math.round(remaining))}% · 剩余 ${Math.round(remaining)}%`,
  };
}

/** 从视图收集错误文案（provider 级），供横幅展示；无错误返回空数组。 */
export function collectErrors(view) {
  const errors = [];
  for (const provider of (view && view.providers) || []) {
    const text = providerStateText(provider);
    if (text) errors.push(`${provider.label}：${text}`);
  }
  return errors;
}

function applyView(data) {
  subscription.data = data;
  subscription.errors = collectErrors(data);
  subscription.loadedAt = Date.now();
}

// --- 状态动作 ---------------------------------------------------------------

/** 概览卡片：TTL 内复用上次结果。失败只写 errors，不动 data（keep-last-good）。 */
export async function loadSubscriptionSummary() {
  if (subscription.loadedAt && Date.now() - subscription.loadedAt < SUMMARY_TTL_MS) {
    return subscription.data;
  }
  try {
    applyView(await invoke('get_subscription_usage', { provider: null, force: false }));
  } catch (e) {
    subscription.errors = [formatActionError('查询套餐用量失败', e, '已保留上次结果，可点击刷新重试')];
  }
  return subscription.data;
}

/**
 * 手动刷新（force 越过 5 分钟缓存）。provider 缺省时刷新全部；
 * 返回本次视图，供设置页「测试连接」读取每个 provider 的状态。
 */
export async function refreshSubscription(provider) {
  subscription.loading = true;
  try {
    applyView(
      await invoke('get_subscription_usage', { provider: provider || null, force: true })
    );
  } catch (e) {
    subscription.errors = [formatActionError('查询套餐用量失败', e, '已保留上次结果，可点击刷新重试')];
  } finally {
    subscription.loading = false;
  }
  return subscription.data;
}

/** 概览卡「查看详情」：弹出独立套餐用量窗口（建窗在 Rust 侧新线程完成）。 */
export function openSubscriptionWindow() {
  return withLoading('openSubscriptionWindow', () =>
    invoke('open_subscription_window').catch((e) =>
      toastActionError('打开套餐用量窗口失败', e, '请重试；若持续失败，请到「查看日志」了解详情', 5000)
    )
  );
}

/** 取某个 provider 的视图；尚未拉到数据时返回 null。 */
export function providerView(id) {
  return (subscription.data && subscription.data.providers.find((p) => p.id === id)) || null;
}
