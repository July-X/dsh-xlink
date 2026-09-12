// 任务完成通知的共享状态与动作：总开关、免打扰策略、声音、未读角标与最近完成
// 记录。状态由 Rust 侧统一持有（角标数字与系统通知都在那边），前端只做读取 /
// 保存 / 清空，设置页打开时拉一次，不在页面里维护第二份真相。
import { reactive } from 'vue';
import { invoke } from './bridge.js';
import { toastActionError, toastSuccess } from './notify.js';
import { withLoading } from './loading.js';

// 与 Rust 侧一致：最近完成记录最多 8 条。裁剪只是兜底，防止异常载荷把设置页撑爆。
const MAX_ITEMS = 8;

// 字段缺失或类型异常时的回落值，与 Rust 侧 `NotifyConfig::default()` 一致：
// 总开关默认打开、`notifyAwayOnly` 默认 true（只在用户离开工作台时打扰），
// 提示音默认关闭。
const FALLBACKS = Object.freeze({
  enabled: true,
  notifyAwayOnly: true,
  sound: false,
});

const SAVE_KEY = 'notificationSave';
const MARK_READ_KEY = 'notificationMarkRead';
const TEST_KEY = 'notificationTest';
const REFRESH_KEY = 'notificationRefresh';

export const notificationStore = reactive({
  enabled: FALLBACKS.enabled,
  notifyAwayOnly: FALLBACKS.notifyAwayOnly,
  sound: FALLBACKS.sound,
  unread: 0,
  items: [],
  watching: false,
  lastError: null,
  environmentNote: null,
  platform: '',
});

function boolOr(value, fallback) {
  return typeof value === 'boolean' ? value : fallback;
}

function normalizeItem(raw) {
  const item = raw && typeof raw === 'object' ? raw : {};
  const title = String(item.title ?? '').trim();
  return {
    sessionId: String(item.sessionId ?? ''),
    // Rust 已把空标题回退成「未命名会话」，这里再兜一次：记录列表不该出现
    // 一行什么都没有的条目。
    title: title || '未命名会话',
    cwd: typeof item.cwd === 'string' ? item.cwd : '',
    finishedAtMs: Number.isFinite(item.finishedAtMs) ? item.finishedAtMs : 0,
    durationMs: Number.isFinite(item.durationMs) && item.durationMs > 0 ? item.durationMs : 0,
  };
}

/// 把 Rust 返回的 NotificationStatus（camelCase）规范化成设置页可直接用的形状。
export function normalizeNotificationStatus(raw) {
  const src = raw && typeof raw === 'object' ? raw : {};
  return {
    enabled: boolOr(src.enabled, FALLBACKS.enabled),
    notifyAwayOnly: boolOr(src.notifyAwayOnly, FALLBACKS.notifyAwayOnly),
    sound: boolOr(src.sound, FALLBACKS.sound),
    unread: Number.isFinite(src.unread) && src.unread > 0 ? Math.floor(src.unread) : 0,
    items: (Array.isArray(src.items) ? src.items.slice(0, MAX_ITEMS) : []).map(normalizeItem),
    watching: boolOr(src.watching, false),
    lastError: typeof src.lastError === 'string' && src.lastError.trim() ? src.lastError.trim() : null,
    // 环境限制说明（不是错误）：例如 macOS 上未打包的 dev 构建拿不到应用
    // bundle，系统通知不会以本应用的名义投递。Rust 侧生成，前端只展示。
    environmentNote:
      typeof src.environmentNote === 'string' && src.environmentNote.trim()
        ? src.environmentNote.trim()
        : null,
    platform: typeof src.platform === 'string' ? src.platform : '',
  };
}

// 载荷不是对象时返回 null：一次异常响应不该把已经显示正确的开关刷回默认值，
// 由调用方决定是保留旧值还是只做局部兜底。
function applyStatus(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const next = normalizeNotificationStatus(raw);
  Object.assign(notificationStore, next);
  return next;
}

/// 应用 Rust 广播的 `notification-status` 快照。
///
/// 未读计数是内核事件的产物（可能由别的窗口里跑完的任务产生），面板打开期间
/// 它必须自己长出来：`notification_status` 只在进入设置页时拉一次，光靠它会让
/// 「3 条未读」一直停在旧值，直到用户手动点「刷新」。
export function applyNotificationStatus(raw) {
  return applyStatus(raw);
}

function currentSettings() {
  return {
    enabled: notificationStore.enabled,
    notifyAwayOnly: notificationStore.notifyAwayOnly,
    sound: notificationStore.sound,
  };
}

let statusInFlight = null;

function startStatusRequest(manual) {
  const request = invoke('notification_status')
    .then((status) => applyStatus(status))
    .catch((e) => {
      // 自动路径（进入设置页）保持静默：保留上一次的值，下次打开再试。
      if (manual) {
        toastActionError('读取通知状态失败', e, '请检查内核是否在运行，然后点「刷新」重试', 6000);
      }
      return null;
    });
  const tracked = request.finally(() => {
    if (statusInFlight === tracked) statusInFlight = null;
  });
  statusInFlight = tracked;
  return tracked;
}

/// 读取通知状态；`manual` 为 true 时挂按钮 loading 并把失败讲清楚。
/// 同一时刻只保留一个请求：面板打开与手动刷新会合并成同一次读取。
export function refreshNotificationStatus(manual = false) {
  if (statusInFlight) return statusInFlight;
  const run = () => startStatusRequest(manual);
  return manual ? withLoading(REFRESH_KEY, run) : run();
}

/// 保存三个开关（未传的字段沿用当前值）。乐观回写：开关必须在点击的同一帧就
/// 变色，等一个 Rust 往返再动会像「没点动」；Rust 拒绝时回滚并说明下一步。
export function saveNotificationSettings(patch = {}) {
  return withLoading(SAVE_KEY, async () => {
    const previous = currentSettings();
    const next = {
      enabled: boolOr(patch.enabled, previous.enabled),
      notifyAwayOnly: boolOr(patch.notifyAwayOnly, previous.notifyAwayOnly),
      sound: boolOr(patch.sound, previous.sound),
    };
    Object.assign(notificationStore, next);
    try {
      const status = await invoke('notification_save_settings', next);
      if (!applyStatus(status)) Object.assign(notificationStore, next);
      return true;
    } catch (e) {
      Object.assign(notificationStore, previous);
      toastActionError('保存通知设置失败', e, '请检查内核是否在运行；开关已还原，可稍后重试', 6000);
      return false;
    }
  });
}

/// 清空未读（角标归零）。
export function markNotificationsRead() {
  return withLoading(MARK_READ_KEY, async () => {
    try {
      const status = await invoke('notification_mark_read');
      // 载荷异常时至少把角标归零：用户点「全部已读」的意图是明确的。
      if (!applyStatus(status)) notificationStore.unread = 0;
      toastSuccess('未读提醒已清空');
      return true;
    } catch (e) {
      toastActionError('清空未读提醒失败', e, '请检查内核是否在运行，然后重试', 6000);
      return false;
    }
  });
}

/// 模拟一次任务完成：Rust 侧把未读 +1、刷新系统角标、弹一条系统通知。
///
/// **不在成功路径上弹页内提示**：这个按钮的作用是让用户看到 Dock / 任务栏上的
/// 角标和系统通知气泡，弹出绿色「成功」浮层只会被误认成"通知就是这玩意儿"
/// （而且连点几次还会合并出一个计数角标，更像通知）。结果直接由卡片上的未读数
/// 与环境说明呈现；只有真出错时才提示。
export function sendTestNotification() {
  return withLoading(TEST_KEY, async () => {
    try {
      const status = applyStatus(await invoke('notification_test'));
      if (status && status.lastError) {
        toastActionError(
          '测试没有产生效果',
          new Error(status.lastError),
          '请按上面的说明处理后重试',
          8000
        );
        return false;
      }
      return true;
    } catch (e) {
      toastActionError('测试任务完成失败', e, '请重试；若反复失败，打开「查看日志」把最近的日志发给维护者', 8000);
      return false;
    }
  });
}
