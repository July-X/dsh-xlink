// 管理面板入口：Vue 3 + Element Plus（暗色主题，简体中文）。
// 与外壳的通信全部走 Tauri 命令（window.__TAURI__.core，见 bridge.js）。
// URL 带 ?log=<name> 时是 open_log_window 弹出的独立日志阅读窗口，
// 挂载 LogViewerWindow 而非管理壳（不跑轮询 / 预载等面板编排）。
// URL 带 ?chatstrip=1 时挂载 OfficialChatTabs（官方对话窗口的页签栏，
// 该 webview 同时承载拉绳挂件，不再有独立的 launcher 路由）。
import { createApp } from 'vue';
import { ElAlert } from 'element-plus/es/components/alert/index.mjs';
import { ElButton } from 'element-plus/es/components/button/index.mjs';
import { provideGlobalConfig } from 'element-plus/es/components/config-provider/index.mjs';
import { ElDialog } from 'element-plus/es/components/dialog/index.mjs';
import { ElEmpty } from 'element-plus/es/components/empty/index.mjs';
import { ElForm, ElFormItem } from 'element-plus/es/components/form/index.mjs';
import { ElIcon } from 'element-plus/es/components/icon/index.mjs';
import { ElInput } from 'element-plus/es/components/input/index.mjs';
import { ElInputNumber } from 'element-plus/es/components/input-number/index.mjs';
import { ElOption, ElSelect } from 'element-plus/es/components/select/index.mjs';
import { ElPopconfirm } from 'element-plus/es/components/popconfirm/index.mjs';
import { ElSwitch } from 'element-plus/es/components/switch/index.mjs';
import { ElTag } from 'element-plus/es/components/tag/index.mjs';
import { ElTooltip } from 'element-plus/es/components/tooltip/index.mjs';
import zhCn from 'element-plus/es/locale/lang/zh-cn';
import 'element-plus/es/components/alert/style/css.mjs';
import 'element-plus/es/components/button/style/css.mjs';
import 'element-plus/es/components/dialog/style/css.mjs';
import 'element-plus/es/components/empty/style/css.mjs';
import 'element-plus/es/components/form/style/css.mjs';
import 'element-plus/es/components/form-item/style/css.mjs';
import 'element-plus/es/components/icon/style/css.mjs';
import 'element-plus/es/components/input/style/css.mjs';
import 'element-plus/es/components/input-number/style/css.mjs';
import 'element-plus/es/components/message/style/css.mjs';
import 'element-plus/es/components/message-box/style/css.mjs';
import 'element-plus/es/components/option/style/css.mjs';
import 'element-plus/es/components/popconfirm/style/css.mjs';
import 'element-plus/es/components/select/style/css.mjs';
import 'element-plus/es/components/switch/style/css.mjs';
import 'element-plus/es/components/tag/style/css.mjs';
import 'element-plus/es/components/tooltip/style/css.mjs';
import 'element-plus/theme-chalk/dark/css-vars.css';
import './theme.css';
import App from './App.vue';
import LogViewerWindow from './LogViewerWindow.vue';
import OfficialChatTabs from './components/OfficialChatTabs.vue';

const params = new URLSearchParams(location.search);
const isLogViewer = params.has('log');
const isChatStrip = params.has('chatstrip');
const isMacOS = /Macintosh|Mac OS X/.test(navigator.userAgent);
const isWindows = /Windows NT/.test(navigator.userAgent);
const usesCustomTitlebar = isMacOS || isWindows;

// 管理面板主窗口在 macOS / Windows 使用自绘标题栏；其它本地窗口继续使用各自的布局。
if (usesCustomTitlebar && !isLogViewer && !isChatStrip) {
  document.body.classList.add('custom-titlebar-shell');
}

const root = isLogViewer
  ? LogViewerWindow
  : isChatStrip
    ? OfficialChatTabs
    : App;

const app = createApp(root);
[
  ElAlert,
  ElButton,
  ElDialog,
  ElEmpty,
  ElForm,
  ElFormItem,
  ElIcon,
  ElInput,
  ElInputNumber,
  ElOption,
  ElPopconfirm,
  ElSelect,
  ElSwitch,
  ElTag,
  ElTooltip,
].forEach((component) => app.component(component.name, component));
provideGlobalConfig({ locale: zhCn }, app, true);
app.mount('#app');
