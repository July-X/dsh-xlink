// 管理面板入口：Vue 3 + Element Plus（明暗双主题，简体中文）。
// 与外壳的通信全部走 Tauri 命令（window.__TAURI__.core，见 bridge.js）。
// URL 带 ?log=<name> 时是 open_log_window 弹出的独立日志阅读窗口，
// 挂载 LogViewerWindow 而非管理壳（不跑轮询 / 预载等面板编排）。
// URL 带 ?chatstrip=1 时挂载 OfficialChatTabs（官方对话窗口的页签栏，
// 该 webview 同时承载拉绳挂件，不再有独立的 launcher 路由）。
import { createApp } from 'vue';
import { reportRenderError } from './shell/errors.js';
import { ElAlert } from 'element-plus/es/components/alert/index.mjs';
import { ElButton } from 'element-plus/es/components/button/index.mjs';
import {
  ElDropdown,
  ElDropdownItem,
  ElDropdownMenu,
} from 'element-plus/es/components/dropdown/index.mjs';
import { ElCheckbox } from 'element-plus/es/components/checkbox/index.mjs';
import { provideGlobalConfig } from 'element-plus/es/components/config-provider/index.mjs';
import { ElDialog } from 'element-plus/es/components/dialog/index.mjs';
import { ElEmpty } from 'element-plus/es/components/empty/index.mjs';
import { ElForm, ElFormItem } from 'element-plus/es/components/form/index.mjs';
import { ElIcon } from 'element-plus/es/components/icon/index.mjs';
import { ElInput } from 'element-plus/es/components/input/index.mjs';
import { ElInputNumber } from 'element-plus/es/components/input-number/index.mjs';
import { ElLoading } from 'element-plus/es/components/loading/index.mjs';
import { ElOption, ElSelect } from 'element-plus/es/components/select/index.mjs';
import { ElProgress } from 'element-plus/es/components/progress/index.mjs';
import { ElRadio, ElRadioGroup } from 'element-plus/es/components/radio/index.mjs';
import { ElSkeleton } from 'element-plus/es/components/skeleton/index.mjs';
import { ElStep, ElSteps } from 'element-plus/es/components/steps/index.mjs';
import { ElSwitch } from 'element-plus/es/components/switch/index.mjs';
import { ElPopconfirm } from 'element-plus/es/components/popconfirm/index.mjs';
import { ElTabPane, ElTabs } from 'element-plus/es/components/tabs/index.mjs';
import { ElTag } from 'element-plus/es/components/tag/index.mjs';
import { ElTooltip } from 'element-plus/es/components/tooltip/index.mjs';
import zhCn from 'element-plus/es/locale/lang/zh-cn';
import { homeDir } from './shell/bridge.js';
import { setDisplayHomeDir } from './shell/labels.js';
import { disableContextMenu } from './shell/noContextMenu.js';
import { applyTheme } from './shell/theme.js';
import 'element-plus/es/components/alert/style/css.mjs';
import 'element-plus/es/components/button/style/css.mjs';
import 'element-plus/es/components/checkbox/style/css.mjs';
import 'element-plus/es/components/dialog/style/css.mjs';
// dropdown：**上面第 15 行 import 了这个组件，却漏了它的样式**，于是
// `el-dropdown.css` 从没进过产物。`.el-dropdown-menu` 与
// `.el-dropdown-menu__item` 的基础样式（padding、hover 底色、文字色、
// transition）全部缺失：菜单还能弹出来——组件 JS 在——但它是一条**没有任何
// 交互反馈的裸文字列表**，这正是「菜单太简陋、没有 hover」的全部原因。
// 它也不会以任何形式报错：dev server、typecheck、build 全绿，只有看界面才发现。
// 没有任何别处的 style/css.mjs 会传递地拉进它（tooltip / select / popconfirm
// 各自只 import base + popper），所以必须显式写这一行。
// 由 `ui/test/epStyles.test.js` 钉住：任何新 import 的 EP 组件都必须配一条 style。
import 'element-plus/es/components/dropdown/style/css.mjs';
import 'element-plus/es/components/empty/style/css.mjs';
import 'element-plus/es/components/form/style/css.mjs';
import 'element-plus/es/components/form-item/style/css.mjs';
import 'element-plus/es/components/icon/style/css.mjs';
import 'element-plus/es/components/input/style/css.mjs';
import 'element-plus/es/components/input-number/style/css.mjs';
import 'element-plus/es/components/loading/style/css.mjs';
import 'element-plus/es/components/message/style/css.mjs';
import 'element-plus/es/components/message-box/style/css.mjs';
import 'element-plus/es/components/option/style/css.mjs';
import 'element-plus/es/components/popconfirm/style/css.mjs';
import 'element-plus/es/components/progress/style/css.mjs';
import 'element-plus/es/components/radio/style/css.mjs';
import 'element-plus/es/components/select/style/css.mjs';
import 'element-plus/es/components/skeleton/style/css.mjs';
import 'element-plus/es/components/step/style/css.mjs';
import 'element-plus/es/components/steps/style/css.mjs';
import 'element-plus/es/components/switch/style/css.mjs';
import 'element-plus/es/components/tab-pane/style/css.mjs';
import 'element-plus/es/components/tabs/style/css.mjs';
import 'element-plus/es/components/tag/style/css.mjs';
import 'element-plus/es/components/tooltip/style/css.mjs';
import 'element-plus/theme-chalk/dark/css-vars.css';
import './theme.css';
// 诊断层样式独立于 theme.css：后者是反棘轮文件（只许越来越小）。
import './diagnostics/diagnostics.css';
import App from './App.vue';
import LogViewerWindow from './logs/LogViewerWindow.vue';
import OfficialChatTabs from './official-chat/OfficialChatTabs.vue';
import SubscriptionWindow from './subscription/SubscriptionWindow.vue';
import UsageWindow from './usage/UsageWindow.vue';

const params = new URLSearchParams(location.search);
const isLogViewer = params.has('log');
const isChatStrip = params.has('chatstrip');
const isUsageViewer = params.has('usage');
const isSubscriptionViewer = params.has('subscription');
const isMacOS = /Macintosh|Mac OS X/.test(navigator.userAgent);
const isWindows = /Windows NT/.test(navigator.userAgent);
const usesCustomTitlebar = isMacOS || isWindows;

// 管理面板主窗口在 macOS / Windows 使用自绘标题栏；其它本地窗口继续使用各自的布局。
if (usesCustomTitlebar && !isLogViewer && !isChatStrip && !isUsageViewer && !isSubscriptionViewer) {
  document.body.classList.add('custom-titlebar-shell');
}

// 右键菜单在壳自己的**所有**窗口里都关掉（主面板 / 日志 / 用量 / 套餐 / 官方
// 对话页签栏——它们共用这个入口），因此只在这里调一次，且必须早于任何组件
// 挂载：晚一步就会有一段窗口期里菜单还在。工作台与三个官方对话内容 webview
// 加载的是别人的页面，由 `src-tauri/src/no-context-menu.js` 注入。
disableContextMenu();

// 主题必须同样早于任何组件挂载：晚一步首帧会先画一帧错误主题再跳色。
applyTheme();

const root = isLogViewer
  ? LogViewerWindow
  : isChatStrip
    ? OfficialChatTabs
    : isUsageViewer
      ? UsageWindow
      : isSubscriptionViewer
        ? SubscriptionWindow
        : App;

const app = createApp(root);
[
  ElAlert,
  ElButton,
  ElDropdown,
  ElDropdownItem,
  ElDropdownMenu,
  ElCheckbox,
  ElDialog,
  ElEmpty,
  ElForm,
  ElFormItem,
  ElIcon,
  ElInput,
  ElInputNumber,
  ElOption,
  ElPopconfirm,
  ElProgress,
  ElRadio,
  ElRadioGroup,
  ElStep,
  ElSteps,
  ElSwitch,
  ElSelect,
  ElSkeleton,
  ElTabPane,
  ElTabs,
  ElTag,
  ElTooltip,
].forEach((component) => app.component(component.name, component));
// `v-loading` 是指令而不是组件：只注册组件的话它会被静默忽略
// （Vue 对解析不到的指令直接跳过，既不报错也不渲染），插件中心的加载占位
// 会退化成一块没有任何文案的空白。ElLoading 的 install 同时注册
// `v-loading` 指令与 `$loading` 服务。
app.use(ElLoading);
// 渲染期错误兜底：没有它，一次 TypeError 就会让面板永久空白且无提示。
app.config.errorHandler = (error, _instance, info) => {
  reportRenderError(error, info);
};
app.config.warnHandler = (message, _instance, trace) => {
  // 保留 Vue 的告警但不要让它静默消失：打包后控制台是唯一能看到的地方。
  console.warn('[dsh-xlink] Vue 告警：', message, trace);
};
provideGlobalConfig({ locale: zhCn }, app, true);
// 路径显示折叠 `~`：异步取 home，到达前面板照常渲染（先显示全路径）。
if (homeDir()) homeDir().then((home) => setDisplayHomeDir(home));
app.mount('#app');
