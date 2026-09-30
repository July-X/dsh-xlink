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

// 每个会话（每次启动外壳）的首次加载强制越过缓存：Rust 侧对 expired 凭据
// 的条目不做自动重试，不强制的话「下次启动发现 key 可用」永远发现不了——
// 一次启动多打三个查询接口，换来隐藏项能自动恢复显示。
let sessionFirstLoad = true;

// 本会话内已经弹过「是否隐藏」提示的 provider：不重复打扰；查询成功后
// 清除，之后若再次失败可以再问一次。
const failurePromptAsked = new Set();

/** 该 provider 是否还需要弹「是否隐藏」提示（configured、处于失败态
 * （providerStateText 非 null）、未隐藏、本会话没问过）。 */
export function failurePromptPending(provider) {
  return (
    !!provider &&
    provider.configured === true &&
    providerStateText(provider) !== null &&
    !isProviderHidden(provider.id) &&
    !failurePromptAsked.has(provider.id)
  );
}

/** 记下「已提示过」，用户取消时也不会在本次会话内反复弹。 */
export function markFailurePrompted(id) {
  failurePromptAsked.add(id);
}

// 与 Rust 侧 kind 字段同源的展示配置。
export const PLAN_KIND = 'plan';
export const BALANCE_KIND = 'balance';

export const subscription = reactive({
  // 独立窗口 / 手动全量刷新进行中。
  loading: false,
  // 单 provider 刷新进行中的 id 列表（概览卡每个分区标题旁的刷新 icon）。
  refreshingIds: [],
  // 上一次成功拉取的 SubscriptionView；失败时不清空（keep-last-good）。
  data: null,
  // 最近一次失败的用户文案（数组，卡片/窗口内联展示）；成功时置空。
  errors: [],
  loadedAt: 0,
});

// --- 隐藏查不到数据的 provider（提示 → 隐藏 → 成功自动恢复） -----------------
// key 配了但一直查不到数据（key 错误 / 凭据失效）时，概览卡会弹确认框问用户
// 是否隐藏该项；选择存 localStorage（跨启动保留），之后某次查询一旦成功
// （包括下次启动的强制首查）就自动恢复显示并清除记录。
const HIDDEN_STORAGE_KEY = 'dshxlink.subscription.hiddenProviders';
const hiddenProviderIds = reactive(loadHiddenIds());

function loadHiddenIds() {
  try {
    const list = JSON.parse(window.localStorage.getItem(HIDDEN_STORAGE_KEY) || '[]');
    return Array.isArray(list) ? list.filter((id) => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

function persistHiddenIds() {
  try {
    window.localStorage.setItem(HIDDEN_STORAGE_KEY, JSON.stringify([...hiddenProviderIds]));
  } catch {
    // 持久化失败只影响跨启动记忆，会话内的行为不受影响。
  }
}

/** 该 provider 是否被用户选择隐藏（概览卡不再展示、错误横幅不再提及）。 */
export function isProviderHidden(id) {
  return hiddenProviderIds.includes(id);
}

/** 隐藏一个 provider（用户在「查不到数据」提示里确认后调用）。 */
export function hideProvider(id) {
  if (!id || hiddenProviderIds.includes(id)) return;
  hiddenProviderIds.push(id);
  persistHiddenIds();
}

function unhideProvider(id) {
  const at = hiddenProviderIds.indexOf(id);
  if (at < 0) return;
  hiddenProviderIds.splice(at, 1);
  persistHiddenIds();
}

// --- 纯展示函数 ---------------------------------------------------------------

/** 剩余百分比 → 配色档位：≥70 绿 / 40–69.99 橙 / <39.99 红。 */
export function percentLevel(percent) {
  const value = Number(percent);
  if (!Number.isFinite(value) || value < 40) return 'danger';
  if (value < 70) return 'warning';
  return 'ok';
}

/** 币种符号；未知币种退回代码本身。 */
export function currencySymbol(currency) {
  if (currency === 'CNY') return '¥';
  if (currency === 'USD') return '$';
  return currency ? `${currency} ` : '';
}

/**
 * 余额一行的展示数据（DeepSeek `/user/balance` 的 `balance_infos[]` 逐条）：
 * `main` 是总额主行，`detail` 把 `granted_balance`（赠金）与
 * `topped_up_balance`（充值）**分别列出**——三者口径不同（总额含赠金），
 * 只显示总额会让人看不出余额里有多少是会过期的赠金。`tip` 是 hover title，
 * 用等式口径把三者讲清。
 *
 * 金额是接口给的数字字符串，只做「加货币符号 + 裁空白」的透传，**不转浮点
 * 参与任何计算**（与 docs/subscription-usage-design.md 的解析规则一致）；
 * 缺失的分量（接口没给 / 不是字符串）不补零、不猜，明细行直接留空。
 */
export function balanceRow(balance) {
  if (!balance) return null;
  const symbol = currencySymbol(balance.currency);
  const amount = (value) => `${symbol}${(value || '').trim()}`;
  const parts = [
    { label: '赠金', hint: '赠金（未过期）：', value: balance.granted },
    { label: '充值', hint: '充值：', value: balance.topped_up },
  ].filter((part) => typeof part.value === 'string' && part.value.trim());
  const total = amount(balance.total);
  return {
    main: `余额：${total}`,
    detail: parts.map((part) => `${part.label} ${amount(part.value)}`).join(' · '),
    tip: parts.length
      ? `总余额 ${total} ＝ ${parts.map((part) => `${part.hint}${amount(part.value)}`).join(' ＋ ')}`
      : `总余额 ${total}（接口未返回赠送 / 充值明细）`,
  };
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

/** 从视图收集错误文案（provider 级），供横幅展示；无错误返回空数组。
 * 已被用户隐藏的 provider 不再提及——它们本来就因为「查不到数据」被隐藏，
 * 恢复显示由查询成功自动触发，横幅重复报错只会让隐藏失去意义。 */
export function collectErrors(view) {
  const errors = [];
  for (const provider of (view && view.providers) || []) {
    if (isProviderHidden(provider.id)) continue;
    const text = providerStateText(provider);
    if (text) errors.push(`${provider.label}：${text}`);
  }
  return errors;
}

function applyView(data) {
  subscription.data = data;
  for (const provider of (data && data.providers) || []) {
    // providerStateText 非 null 即「本次失败 / 凭据失效 / 业务错误」；其余
    // 视为查询成功（或 keep-last-good 且未带新错误），自动恢复被隐藏的分区。
    if (providerStateText(provider) === null) {
      unhideProvider(provider.id);
      failurePromptAsked.delete(provider.id);
    }
  }
  subscription.errors = collectErrors(data);
  subscription.loadedAt = Date.now();
}

/**
 * 把单 provider 的视图合并进现有数据（Rust 侧按 provider 查询只返回该
 * provider；其它分区保持 keep-last-good 原样不动）。没有旧数据时整体接管。
 */
function mergeProviderView(data) {
  const current = subscription.data;
  if (!current) return applyView(data);
  const byId = new Map(current.providers.map((p) => [p.id, p]));
  for (const provider of data.providers) byId.set(provider.id, provider);
  applyView({ ...current, providers: current.providers.map((p) => byId.get(p.id)) });
}

/** 某个 provider 的单分区刷新是否进行中。 */
export function isProviderRefreshing(id) {
  return subscription.refreshingIds.includes(id);
}

// --- 状态动作 ---------------------------------------------------------------

/** 概览卡片：TTL 内复用上次结果。失败只写 errors，不动 data（keep-last-good）。
 * 会话首次加载强制越过缓存（见 sessionFirstLoad）：让「key 修好了」在下一次
 * 启动时就能被发现，被隐藏的分区随之自动恢复。 */
export async function loadSubscriptionSummary() {
  if (subscription.loadedAt && Date.now() - subscription.loadedAt < SUMMARY_TTL_MS) {
    return subscription.data;
  }
  const force = sessionFirstLoad;
  sessionFirstLoad = false;
  try {
    applyView(await invoke('get_subscription_usage', { provider: null, force }));
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
  if (provider) return refreshSubscriptionProvider(provider);
  subscription.loading = true;
  try {
    applyView(
      await invoke('get_subscription_usage', { provider: null, force: true })
    );
  } catch (e) {
    subscription.errors = [formatActionError('查询套餐用量失败', e, '已保留上次结果，可点击刷新重试')];
  } finally {
    subscription.loading = false;
  }
  return subscription.data;
}

/**
 * 只刷新指定 provider（概览卡分区标题旁的刷新 icon）：在途去重，失败保留
 * 该分区上次结果并写横幅；成功只合并这一分区的数据，其它分区不动。
 */
export async function refreshSubscriptionProvider(id) {
  if (!id || isProviderRefreshing(id)) return subscription.data;
  subscription.refreshingIds.push(id);
  try {
    mergeProviderView(await invoke('get_subscription_usage', { provider: id, force: true }));
  } catch (e) {
    subscription.errors = [formatActionError('查询套餐用量失败', e, '已保留上次结果，可点击刷新重试')];
  } finally {
    subscription.refreshingIds = subscription.refreshingIds.filter((item) => item !== id);
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
