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
import { ref } from 'vue';
import { setWindowTheme } from './bridge.js';

// 键名带作用域前缀：同一台机器上本地窗口（工作台、日志、用量）都读它，所以
// 主面板切换主题时，已经开着的独立窗口下一次绘制就跟着变。
const STORAGE_KEY = 'dsh-xlink:ui-theme';
const THEMES = Object.freeze(['dark', 'light']);

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

/// 把 `theme` 同步到 `<html>`。启动时调一次，之后每次 `setTheme` 再调。
///
/// 必须早于第一个组件挂载，否则首帧会先画一帧错误主题再跳色。main.js 在
/// createApp 之前就调它。
///
/// 原生装饰（标题栏）跟着同一个真值走，见 `bridge.setWindowTheme`。这里
/// **不吞异常**：调用点是挂载前，还没有 toast 能报；失败的后果只是标题栏停
/// 在旧主题（内容与标题栏短暂不同步），不值得为它把整个应用启动打断。
export function applyTheme() {
  document.documentElement.classList.toggle('dark', theme.value === 'dark');
  void setWindowTheme(theme.value).catch(() => {});
  document.documentElement.dataset.theme = theme.value;
}

export function setTheme(next) {
  if (!THEMES.includes(next) || next === theme.value) return theme.value;
  theme.value = next;
  try {
    window.localStorage?.setItem(STORAGE_KEY, next);
  } catch {
    // 存不下去只丢持久化，本次会话的主题照样生效；下次启动回到默认。
  }
  applyTheme();
  return theme.value;
}

/// 侧栏底部开关用：暗 → 亮，亮 → 暗。
export function toggleTheme() {
  return setTheme(theme.value === 'dark' ? 'light' : 'dark');
}