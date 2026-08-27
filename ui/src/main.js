// 管理面板入口：Vue 3 + Element Plus（暗色主题，简体中文）。
// 与外壳的通信全部走 Tauri 命令（window.__TAURI__.core，见 bridge.js）。
// URL 带 ?log=<name> 时是 open_log_window 弹出的独立日志阅读窗口，
// 挂载 LogViewerWindow 而非管理壳（不跑轮询 / 预载等面板编排）。
// URL 带 ?chatstrip=1 时挂载 OfficialChatTabs（官方对话窗口的页签栏，
// 该 webview 同时承载拉绳挂件，不再有独立的 launcher 路由）。
import { createApp } from 'vue';
import ElementPlus from 'element-plus';
import zhCn from 'element-plus/es/locale/lang/zh-cn';
import 'element-plus/dist/index.css';
import 'element-plus/theme-chalk/dark/css-vars.css';
import './theme.css';
import App from './App.vue';
import LogViewerWindow from './LogViewerWindow.vue';
import OfficialChatTabs from './components/OfficialChatTabs.vue';

const params = new URLSearchParams(location.search);
const isLogViewer = params.has('log');
const isChatStrip = params.has('chatstrip');

const root = isLogViewer
  ? LogViewerWindow
  : isChatStrip
    ? OfficialChatTabs
    : App;

createApp(root)
  .use(ElementPlus, { locale: zhCn })
  .mount('#app');
