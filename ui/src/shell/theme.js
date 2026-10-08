// 明暗主题的唯一实现：读一次 localStorage，把结果落成 `<html>` 上的 `dark` class。
//
// 为什么用 class 而不是自定义属性：Element Plus 的暗色样式写在
// `element-plus/theme-chalk/dark/css-vars.css`，选择器就是 `html.dark`。自己再造
// 一套 `html[data-theme]` 会让组件库留在它的默认（浅色）变量上，主题切了却只有
// 壳跟着变——这正是「切了主题，弹窗还是白的」那类半生不熟状态的来源。
//
// 落地只剩一个动作：`documentElement.classList.toggle('dark', isDark)`。主题.css
// 里 `:root` 放浅色 token、`html.dark` 放暗色 token，两套 Element Plus 覆写也各自
// 待在自己的选择器下，没有第二处判据。
import { computed, ref } from 'vue';
import { setWindowTheme } from './bridge.js';
import { broadcastTheme, subscribeThemeChanges } from './themeSync.js';

// 键名带作用域前缀：同一台机器上本地窗口（工作台、日志、用量）都读它。
// 它只决定**各窗口启动时**的主题；运行中主壳切换主题靠 `themeSync` 的广播，
// localStorage 的 `storage` 事件不跨 webview 生效。
const STORAGE_KEY = 'dsh-xlink:ui-theme';
const THEMES = Object.freeze(['dark', 'light', 'system']);

// 默认暗色：原壳一直是暗色，且设计稿的预览默认也是暗色。突然给老用户一个浅色
// 窗口不算「按设计走」，算换了个产品。
const DEFAULT_THEME = 'dark';

function readStoredTheme() {
  try {
    const raw = window.localStorage?.getItem(STORAGE_KEY);
    return THEMES.includes(raw) ? raw : DEFAULT_THEME;
  } catch {
    // localStorage 在某些 webview 上下文里会抛（隐私模式 / 禁用存储）。
    // 读不到就按默认走，不让一次存储异常把整个应用带崩。
    return DEFAULT_THEME;
  }
}

export const theme = ref(readStoredTheme());
export const themeOptions = Object.freeze([
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
  { value: 'system', label: '跟随系统' },
]);
const systemQuery = window.matchMedia?.('(prefers-color-scheme: dark)');
const systemDark = ref(!!systemQuery?.matches);
export const resolvedTheme = computed(() => theme.value === 'system' ? (systemDark.value ? 'dark' : 'light') : theme.value);
// 每扇窗口各监听系统配色，生命周期与窗口相同；固定模式不随系统变化。
systemQuery?.addEventListener('change', ({ matches }) => {
  systemDark.value = matches;
  if (theme.value === 'system') applyTheme();
});

/// 这个值是不是一套已知主题。**两份消费方共用**：本地 `setTheme` 校验用户输入，
/// `themeSync` 的广播回调校验事件负载（事件可被同页脚本构造，非法值会让整窗
/// 落到既不是 dark 也不是浅色的主题上）。各写一份必然漂。
export function isKnownTheme(next) {
  return THEMES.includes(next);
}

/// 改内存里的主题真值，**不落盘、不广播**——给「收到广播后落地」用。
/// 主壳那条路径走 `setTheme`：它还要写 localStorage 并广播出去。
export function setThemeValue(next) {
  theme.value = next;
}

/// 把 `theme` 同步到 `<html>`。启动时调一次，之后每次 `setTheme` 再调。
///
/// 必须早于第一个组件挂载，否则首帧会先画一帧错误主题再跳色。main.js 在
/// createApp 之前就调它。
///
/// 原生装饰（标题栏）跟着同一个真值走，见 `bridge.setWindowTheme`。这里
/// **不吞异常**：调用点是挂载前，还没有 toast 能报；失败的后果只是标题栏停
/// 在旧主题（内容与标题栏短暂不同步），不值得为它把整个应用启动打断。
export function applyTheme() {
  document.documentElement.classList.toggle('dark', resolvedTheme.value === 'dark');
  // system 要清除原生外观覆盖，否则媒体查询读到的是应用强制的颜色。
  void setWindowTheme(theme.value === 'system' ? null : resolvedTheme.value).then(() => {
    if (theme.value !== 'system') return;
    // 原生调用异步完成后重新取值；切换期间用户已选固定模式则不回写。
    systemDark.value = !!systemQuery?.matches;
    document.documentElement.classList.toggle('dark', resolvedTheme.value === 'dark');
  }).catch(() => {});
}

export function setTheme(next) {
  if (!isKnownTheme(next) || next === theme.value) return theme.value;
  theme.value = next;
  try {
    window.localStorage?.setItem(STORAGE_KEY, next);
  } catch {
    // 存不下去只丢持久化，本次会话的主题照样生效；下次启动回到默认。
  }
  applyTheme();
  // 顺序是「先把自己这扇窗改对，再通知别人」：反过来主窗会慢一帧。
  broadcastTheme(next);
  return theme.value;
}

/// 副窗侧：订阅主壳的主题广播，收到就跑一次 `applyTheme`。
///
/// 必须在 `createApp` 之前调（`main.js` 与 `applyTheme()` 同处）——晚一步，
/// 首帧会先画一帧旧主题再跳色，与 `applyTheme` 自身要早于挂载是同一个理由。
///
/// 落地的三步**不能省任何一步**：只 `setThemeValue` 则内容不变、只 `applyTheme`
/// 则真值与内容脱节、只推 `setWindowTheme` 则只剩原生标题栏换色——最后那个正是
/// `bridge.setWindowTheme` 注释警告的割裂换个方向。传输层（`themeSync`）不知道
/// 什么算合法主题，校验归这里，两边各写一份必然漂。
export function followThemeBroadcast() {
  subscribeThemeChanges((next) => {
    // 事件可被同页脚本构造，非法值会让整窗落到既不是 dark 也不是浅色的主题上。
    if (!isKnownTheme(next) || next === theme.value) return;
    setThemeValue(next);
    applyTheme();
  });
}
