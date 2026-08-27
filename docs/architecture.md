# dsh-desktop 架构

桌面壳的模块布局、数据流与数据目录约定。约定性约束（必须照做）见 [AGENTS.md](../AGENTS.md)。

## 模块

```
ui/src（Vue 3 SPA）──invoke(Channel)──▶ commands.rs ──▶ kernel.rs / plugins.rs ──▶ pnpm/git/tar 子进程
                                   │              │
                              settings.rs    releases.rs（npm registry → GitHub 回退）
                                   │
              ~/.dsh/desktop/{settings.json, kernels/, logs/, active.txt} + ~/.dsh/plugins/
```

- `commands.rs`：Tauri 命令层。长任务用 `spawn_blocking` + `tauri::ipc::Channel` 向 UI 推进度事件；窗口类命令（`open_harness`、`open_log_window`）在新 OS 线程上构建 webview（Windows 主线程创建会死锁）。日志「全屏」由 `open_log_window` 弹独立可缩放窗口（同一 SPA 加 `?log=<name>` 查询串，`ui/src/main.js` 分流到 `LogViewerWindow.vue`），ACL 走 `capabilities/log-viewer.json`（只放 `read_log_file`）。
- `ui/`：管理面板前端（Vue 3 + Element Plus，Vite 构建）。源码在 `ui/src/`（状态与动作集中在 `store.js` / `plugins.js` / `skills.js` / `progress.js` / `logs.js`，与 Rust 只经 `bridge.js` 的 invoke/Channel 通信）；`vite build` 产物 `ui/dist/` 是 `tauri.conf.json` 的 `frontendDist`，`tauri dev` 走 `devUrl`（vite dev server，5173）热更新。`open_official_chat` 在独立线程里 `WebviewWindowBuilder::new(...).label("official-chat").title("DeepSeek 官方对话")`，固定 `OFFICIAL_CHAT_URL`（`https://chat.deepseek.com`），构造时不再覆盖 user-agent——WebView2 引擎本身就是真实的桌面版 Edge，原生 UA、`Sec-CH-UA` 客户端提示与 `navigator.userAgentData` 天然一致；此前把 UA 改写成 Chrome 反而制造了「HTTP 层报 Edge、JS 层报 Chrome」的矛盾，正是环境检测的特征。不启用无痕模式——Windows 使用专属目录、macOS 使用稳定的数据存储标识，均为持久化配置档案；DeepSeek 登录态跨外壳重启保留，与面板/工作台窗口隔离；再调 `.additional_browser_args(OFFICIAL_CHAT_BROWSER_ARGS)` 抑制 Chromium 自报的 `navigator.webdriver = true`，同时重述 wry 默认禁用的 `msWebOOUI` / `msPdfOOUI` / `msSmartScreenProtection`——传入 browser args 会整体替换 wry 默认值，漏掉这三项 WebView2 就会重新弹出 SmartScreen 安全提醒与 Edge 专属 UI；该参数仅 WebView2 后端消费，macOS / Linux 构建忽略——且 WebView2 要求同一 user-data 目录上的环境参数完全一致，面板与工作台已在默认目录用默认参数建好环境，所以 Windows 下此窗口经 `.data_directory` 固定到专属目录 `<data_dir>/webview-official-chat`，避免环境参数冲突；macOS 下不使用该目录字段，改用稳定的 `.data_store_identifier` 保存 WebKit 登录数据；之后按 `titlebar-pulse.js` → `chat-fingerprint.js` 的顺序注入内容子 webview 各自的两个 `initialization_script`、给本地 `official-chat-strip` 页签栏 webview 注入 `pullstring-launcher.js`——拉绳属于整个官方对话窗口的 chrome，由页签栏 webview 承载，内容子 webview 不再下沉；`pullstring-launcher.js` 因此不需要 `chat-fingerprint.js` 抢跑 `window.__TAURI__` 的闭包引用，初始化顺序出错也不会影响拉绳挂件。复用既有窗口（`get_webview_window` + `set_focus`），不设 `closable(false)`——第三方 origin 的窗口不持有内核会话，OS 关闭按钮应保持有效。`open_official_chat` 是 `async` 命令，builder 结果通过 `std::sync::mpsc::channel` 回传、由 `tauri::async_runtime::spawn_blocking` 接收，命令只在线程里 `Result<WebviewWindow, _>` 真正落地之后才 `Ok(())`。配套的 `close_official_chat` 在窗口未注册时返回错误，存在的窗口走 `destroy()`；面板按钮在 `StatusView.official_chat_open` 的下一次 2.5s 轮询里把按钮文案从「打开官方对话」翻为「关闭官方对话」。
- `kernel.rs`：内核安装、active 指针、启动 / 停止、端口探测；详见下文「内核生命周期」。
- `plugins.rs`：社区插件的中央库、内核物化、profile 接线、更新检查、社区目录；实现规则见 [plugin-internals.md](plugin-internals.md)，设计层见 [plugin-management.md](plugin-management.md)。
- `releases.rs`：npm registry 全量版本 + dist-tags；registry 不可达时回退 GitHub Releases API 与 Atom feed。
- `node.rs`：Node 检测（显式配置 → PATH → nvm 管理的 Node：macOS/Linux `$NVM_DIR/versions/node/<v>/bin/node` 跟随 `alias/default` 链，Windows `%NVM_SYMLINK%` 与 `%NVM_HOME%/v*/node.exe` → 常见系统位置）、engines 校验（`^22.19 || >=24`）、pnpm/npm 解析（显式配置 → node 同目录 → PATH）；空结果文案按「完全没有 Node」与「Node 版本太老」分别给出可操作的多路径（nvm/fnm/volta、brew/winget/apt、官方安装包）。
- `settings.rs`：`settings.json` 平铺结构（`node_path` / `pnpm_path` / `port`），serde default 兼容缺字段。
- `process.rs`：所有 GUI 子进程的 `quiet()`（CREATE_NO_WINDOW）+ `command_with_path()`（一次性 sibling，盖上 `env::merged_path()`）出口。
- `updater.rs`：`tauri-plugin-updater` 包装，启动 3 秒后后台检查并 emit `shell-update-available`。
- `lib.rs`：装配 + `setup()` 取目录（必须走 `kernel::data_dir`）+ `RunEvent::Exit` 兜底回收内核进程组。`harness` 与 `official-chat` 两个 webview 窗口通过 `capabilities/harness-remote.json` / `capabilities/official-chat-remote.json` 分别绑定 ACL；拉绳挂件只需要 `allow-focus-main-shell` 这条 IPC 命令，URL 都精确钉死（`http://127.0.0.1:*` / `https://chat.deepseek.com/*` 等三个官方对话 origin，不开通 wildcard 域名）。`harness-remote.json` 直接授 `allow-focus-main-shell`，`official-chat-remote.json` 不授任何命令（拉绳属于窗口 chrome，由 `official-chat-strip` 页签栏 webview 承载、走 `allow-official-chat-tabs` 这条本地权限）。

## 内核生命周期

- 安装：在 `<data_dir>/kernels/<version>/` 写最小 stub `package.json` 后执行 `pnpm add --prefix … --ignore-workspace --config.node-linker=hoisted --reporter=append-only @deepseek-ai/dsh@<version>`。
- `node-linker=hoisted` 保证 `node_modules` 扁平，内核入口固定为 `node_modules/@deepseek-ai/dsh/lib/bin.js`（`kernel::KERNEL_BIN_REL`）；改布局必须同步该常量与 `start()`。
- `run_pnpm` 把 stdout/stderr 各用一个 drain 线程读入 mpsc channel，安装线程逐行回调 `on_progress` 并落盘日志——不要把两个管道放在同一线程顺序读取（会因管道缓冲区满而死锁）。

## 数据目录

外壳全部状态位于 `<dsh_home>/desktop/`（release build）或 `<dsh_home>/desktop-dev/`（debug build `tauri dev`），由 `kernel::data_dir` 解析并在启动时创建。子结构：`kernels/<版本>/`、`logs/`、`settings.json`、`active.txt`、`kernel.pid`。

启动时 `setup()` 在 stderr 打印 `dsh-desktop: data_dir = <path> (build: dev|release)`，让用户一眼确认当前进程用的是哪个目录。

### 优先级（`kernel::data_dir`）

1. `DSH_DESKTOP_DATA_DIR` 环境变量——完全覆盖目录路径（用于在外部盘上测试等场景）
2. `<DSH_HOME 或 ~/.dsh>/<SHELL_SUBDIR>/`——`SHELL_SUBDIR` 在 release 是 `desktop`、debug 是 `desktop-dev`
3. `app_data_dir()`（OS app-data 目录）作为只读 dsh home 的 fallback

### 为什么 dev 和 release 用不同目录

`settings.json`（端口配置）、`active.txt`（当前激活版本）、`kernel.pid`（运行中内核的 PID）、`kernels/<版本>/`（安装的内核）、loopback 端口都是**共享资源**。一个开发者同时跑 `tauri dev` 和已装的 release shell 时，两个实例会互相争端口（`port_open` 拒绝启动）、互相 kill（任意一方点"关闭工作台"就把对方的内核杀了）、互相覆盖 `active.txt` 和 `settings.json`。分目录 + 错位端口（debug 3091 / release 3090）让两边完全互不读对方的 state——dev 可以放心改端口、切内核、看 log，不会污染 release shell 的视图。

### 端口（`kernel::DEFAULT_PORT`）

- debug build：3091（release 默认 3090 + 1）
- release build：3090

`Settings::default()` 的 port 在 `settings.json` 缺失时用 `kernel::DEFAULT_PORT`；用户保存过的 port 优先。

## 窗口

- `main`（管理面板）：`tauri.conf.json` 里配置为主窗口；加载 `ui/` 静态资源，`capabilities/default.json` 拥有全部本地命令权限。
- `harness`（工作台）：`open_harness` 在新 OS 线程里 `WebviewWindowBuilder::new(...).label("harness")`，加载 `http://127.0.0.1:<port>`；`closable(false)` 防止误关丢内核会话，由 `stop_kernel` 用 `destroy()` 主动回收。`capabilities/harness-remote.json` 仅授权 `allow-focus-main-shell`，URL 锁 `http://127.0.0.1:*`。
- `official-chat`（官方对话，多页签）：`open_official_chat` 在新 OS 线程里建一个裸 `WindowBuilder`（label `official-chat`，需 tauri `unstable` feature），再 `Window::add_child` 挂两类子 webview——顶部 `official-chat-strip`（加载本地 SPA `index.html?chatstrip=1` 渲染页签栏，保留 `window.__TAURI__` 故能调 `official_chat_tabs` 读页签列表 / `switch_official_chat_tab` 切换活动页签；该 webview 同时直接承载拉绳挂件——详见下面 `pullstring-launcher.js` 那条）与 `OFFICIAL_CHAT_TABS` 每条一个内容 webview（`official-chat-tab-{i}`，加载对应远程 URL，首个可见、其余 `hide()`，`switch_official_chat_tab` 显示目标并 `set_focus`）；`relayout_official_chat` 在 `Resized` / `ScaleFactorChanged` 时把页签栏钉顶、内容铺满下方。内容 webview 共用 `webview-official-chat` 目录（Windows）/ `OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`（macOS）持久化登录态，origin 隔离使 DeepSeek、千问、MiniMax 三页签不串数据；不覆盖 user-agent（诚实桌面版 Edge，UA / 客户端提示 / `userAgentData` 一致）、各内容 webview 统一 `OFFICIAL_CHAT_BROWSER_ARGS`（满足 WebView2 同目录同参数约束）、strip 自己的 `background_color(Color(20,27,54,255))` 把 HWND 染成 `--el-bg-color` 让 strip 整片均匀深蓝，不设 `closable(false)`，OS 关闭按钮正常工作，重复点击复用既有窗口并 `set_focus`，`close_official_chat` 销毁窗口即连带销毁子 webview、登录态落盘保留。`capabilities/official-chat-strip.json` 授权 `allow-official-chat-tabs`（仅本地页签栏 webview，页签命令 + 拉绳 `focus_main_shell` 一条），`capabilities/official-chat-remote.json` 锁 `https://chat.deepseek.com/*`、`https://www.qianwen.com/*` 与 `https://agent.minimaxi.com/*`、不授任何 shell 命令（拉绳属于整个官方对话窗口的 chrome，由页签栏 webview 承载，不下沉到各远程内容页）。`StatusView.official_chat_open` 由 `get_status` 在 `spawn_blocking` 内经 `get_window` 同步读取，作为面板按钮切换「打开官方对话」/「关闭官方对话」标签的信号。

`titlebar-pulse.js` / `pullstring-launcher.js` / `chat-fingerprint.js` 由外壳通过 `WebviewWindowBuilder::initialization_script`（harness）/ `WebviewBuilder::initialization_script`（official-chat 本地 webviews 与内容子 webview）按各自需要的集合注入：`official-chat` 内容 webview 注入 `titlebar-pulse.js` + `chat-fingerprint.js`（拉绳属于窗口 chrome，不下沉），`official-chat-strip` 页签栏 webview 注入 `pullstring-launcher.js`（拉绳挂在 strip 右上角，整片 66px 的 HWND 都能容纳 SVG），`harness` 工作台注入 `titlebar-pulse.js` + `pullstring-launcher.js`：

- `pullstring-launcher.js`（注入到 `official-chat-strip` 页签栏 webview + harness 工作台 webview，不注入到 `official-chat` 内容子 webview）：在页面右上角挂一盏「小台灯」形状的拉绳挂件（24×38 logical 的紧凑 SVG：短拉链 + 梯形灯罩 + 灯泡 + 细杆 + 圆角底座），pull 一下点亮灯泡、调起 `focus_main_shell`。strip 时贴 `right:12px`（strip 右内沿）、cord 为 `#4D6BFE`（官方对话品牌蓝）；workbench 时贴 `left:212px`（侧栏折叠按钮旁）、cord 为 `#609926`（Gitea 绿）。surface 检测看 `window.location.search` 是否含 `chatstrip=1` 或 `chatlauncher=1`（后者已无对应路由，作为兼容保留）——任一命中就走官方配色 + 右侧锚定，workbench 不带就走绿色 + 左侧锚定。strip webview 自己就是天然 38px Tab Bar 高度，`.chat-strip` 整片 `background: var(--el-bg-color)` 覆盖住默认 WebView2 白色 HWND bg，lamp 整 36px 几何落在 strip HWND 内不溢出：cord y=2–5 / 灯罩梯形 y=5–13 / 灯泡 cy=14 r=2（部分压在灯罩下）/ 杆 y=16–30 / 圆角底座 y=30–36。`top: 0` 自然挂 strip 顶 = 窗口顶。拉绳属于整个官方对话窗口的 chrome，挂在 strip webview 而不是独立 launcher——试过独立 `official-chat-launcher` webview 但 WebView2 子 HWND 在 Tauri 2.11 / wry 0.55.1 上拿不到透明背景（`background_color(Color(0,0,0,0))` 不生效），launcher 一直被画成不透明深色方块，所以拉绳搬回 strip HWND 后又把 66px SVG 缩到 38px 让 strip 不用撑高。`focus_main_shell` 由 `allow-official-chat-tabs`（strip 路径）授权。`official-chat` 内容 webview 不授任何 shell 命令，对应 `official-chat-remote.json` 的 `permissions` 为空。
- `titlebar-pulse.js`（注入到 harness 工作台 webview + `official-chat` 内容子 webview）：接管 chrome-row 顶部条带。`location.hostname` 命中 DeepSeek / 千问 / MiniMax 三个授权 origin 之一时使用官方品牌蓝 `#4D6BFE`（rgb 77,107,254）；否则用 Gitea 绿 `#609926`（rgb 96,152,38）。两个 sweep 周期相同（6.01s），半周期偏移。workbench 页面自带 `<body><div data-titlebar-pulse="2">`，chat 页面没有——脚本用 `ensureSecondBar()` 在缺失时补上。
- `chat-fingerprint.js`（仅注入到 `official-chat` 内容子 webview，**必须排在 `titlebar-pulse.js` 之后**）：只清除嵌入式痕迹，不再伪造浏览器指纹——把 `navigator.webdriver` 钉在 `false`（正常浏览器的值），并删除 `__TAURI__` / `__TAURI_INTERNALS__` / `__TAURI_METADATA__` / `__TAURI_IPC__` 全局（正常浏览器里它们根本不存在；暴露任何形式的 Proxy 都等于自报嵌入式身份）。其余表面保持真实：引擎是货真价实的桌面版 Edge，用普通 JS 对象冒充 `userAgentData` / plugins 等反而会被原生类检查识破。拉绳挂件已独立成 launcher webview，`chat-fingerprint.js` 不再与之同链，因此脚本之间不存在 `__TAURI__` 抢跑的耦合关系。
- 三个脚本顶部都有 `if (window.top !== window.self) return` 顶帧守卫，避免 Tauri 在每个 iframe 都执行初始化脚本时挂出多份拉绳 / 条带 / 指纹 stub；harness 工作台在嵌套预览里也可能挂多个 iframe，守卫能让顶层唯一实例化。