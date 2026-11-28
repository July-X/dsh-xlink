# 窗口核心能力设计与代码审查

本文整理 dsh-xlink 的管理面板、会话子 WebView、独立查看器与授权弹窗，以及它们的布局、外观和生命周期。

## 结论

当前实现已经形成一个可复用的窗口能力模块：`src-tauri/src/shell/window.rs`。日志、模型用量和套餐用量三扇查看器都通过它完成吸附定位与主窗移动跟随，调用方只负责窗口内容、权限和创建线程。

扩展吸附查看器时遵守两条约束：

- 窗口 label 和默认尺寸登记在 `window.rs`，创建和初始吸附不得各自维护尺寸常量。
- 吸附是“主窗口驱动的单向跟随”，不是两个窗口之间的双向绑定。当前实现中副窗口手动移动后，下一次主窗口移动仍会重新贴回；若产品需要脱离吸附，应增加显式 docking 状态。

## 窗口分层

### 主窗口 `main`

- 配置入口：`src-tauri/tauri.conf.json`。默认尺寸 1040×748，支持调整大小、最大化和全屏。
- 托盘/菜单栏恢复入口、退出握手、主题来源和独立窗口吸附的锚点。
- 建窗时明确启用系统装饰和阴影，使用不透明首帧底色 `#0a100f`；窗框、标题栏和按钮由系统绘制。
- 前端只绘制内容区，顶部保留 2px `--build-color` 构建标识线，不占用系统标题栏。
- **最小化由系统处理**，可从 Windows 任务栏或 macOS Dock 恢复；**关闭请求**仍被 Rust 拦截并收进后台。

### 壳自有的独立查看器

当前有**三种**独立窗口；其中模型用量 / 套餐用量两扇的界面入口已于 2026-10-28 移除（内容并进侧栏「工作台 → 用量分析」页），但窗口组件、Rust 命令与 capability 仍保留：

| label | 创建入口 | 内容 | 默认规格 | 能力 |
| --- | --- | --- | --- | --- |
| `usage-viewer` | `usage::open_usage_window`（无界面入口） | `?usage=1` | 760×800（`USAGE_VIEWER_SIZE`） | 吸附、跟随、可缩放 |
| `subscription-viewer` | `subscription::open_subscription_window`（无界面入口） | `?subscription=1` | 760×800（复用 `USAGE_VIEWER_SIZE`） | 吸附、跟随、可缩放 |
| `log-viewer` | `commands::open_log_window` | `?log=<name>` | 960×720（`LOG_VIEWER_SIZE`） | 吸附、跟随、可缩放 |

三扇查看器都在独立 OS 线程创建 WebView，避免 Windows 上在命令线程同步创建时死锁；创建结果经 `mpsc` 回传，调用方在 blocking worker 中等待，超时为 20 秒。建窗统一走 `shell::window::decorate_native` 启用原生装饰、系统阴影和不透明窗口，并通过 builder 的 `background_color` 指定页面绘制前的过渡底色 `#0a100f`。三扇副窗都支持原生最小化、最大化、调整大小和关闭；关闭时直接关窗，不走主面板的后台常驻流程。

**「界面入口移除」不等于「能力删除」**：`usage-viewer` / `subscription-viewer` 的组件、命令与 capability 全部保留，`main.js` 的窗口路由（`?usage=1` / `?subscription=1`）照旧挂载；「用量分析」页只是用 `UsageWindow` 的 `inline` 模式复用前者内容，面板不再调用两扇窗口的打开命令。先例见「刷新工作台」与 `setDefaultInstance`。

### MiniMax 实验工作台

Unix 启动由只读内嵌的 `mcode-unix-launcher.cjs` 负责：壳持有 stdin 写端，持久化 PID 后才写入放行字节；服务不继承管道。开发重启、崩溃或强制结束壳导致 EOF 时，启动器对自身进程组发 TERM，一秒后 KILL，期间保留组长身份防止 PGID 复用。服务提前退出也触发同样收尾，不能把同组的另一监听服务留下。正常停止在发信号前补拍进程树，再核验跟踪到的脱离进程组后代及两个端口。启动器也被强杀、或异常退出前后代已自行 setsid 脱离组的情况不在父管道兜底保证内，旧记录仍保守保留，不按端口或旧 PID 杀进程。此回归已在 Linux 隔离双端口夹具验证，macOS 实包及安装产物仍待复验。

异常退出后不能仅凭旧 PID 已消失或双端口空闲推断整棵进程树已结束，所以 Unix 侧的跟踪器把进程树身份名册落盘到实例的 `runtime/process-tree.json`（pid + 启动时间 + 进程表来源，名册一有变化就原子写）。壳重启后先核对这份证据：名册里的身份全部不在、两个端口都空闲，就自动清理运行记录、进程树证据与运行占位并写 `mcode-lifecycle` 日志——不再让用户为「进程其实都不在了」去做无意义的重启（2026-11-09 用户要求）；名册机制上线前写的旧记录没有证据文件，改用「记录里的启动器已不在 + 它所在进程组已空 + 双端口空闲」核对后同样自动清理（启动器 setsid 当组长、服务继承同一 pgid），看不到的只有自行 setsid 脱离进程组的后代；只有证据损坏 / 仍有活进程时才保留记录并显示「重启后清理记录」，由用户确认。自动清理只删记录，不按旧 PID 或端口结束任何进程。概览的手动清理同样要求用户先重启电脑、确认没有其他窗口运行 MiniMax，再明确确认；确认绑定实例、双端口与 PID 文件和运行占位的内容快照，后端持有启动/安装锁及已登记实例的跨进程生命周期租约，再检查快照、端口、记录归属和记录中的进程是否仍在。进程查询只用于拒绝有疑点的操作，不作为后代全部退出的证明。记录变化、端口占用、损坏或归属不明时均拒绝并保留；清理先移除占位、再移除 PID，以便中断时仍有记录阻止启动。仅剩占位时也可进入恢复。Windows 依赖 Job 的 `KILL_ON_JOB_CLOSE`，壳关闭即整树结束，因此 PID 不在 + 端口空闲即可自动清理，不落证据文件。已经接入离线协议的实例无论是否启用共享技能，启动前都必须创建运行占位，不能在旧占位存在时再次启动或更改端口。旧版 socket 接管入口已移除：真实崩溃收尾后它无法可靠接管已脱离进程组的后代。


`mcode-workbench` 由 `harness/mcode_window.rs` 在确认服务归属和首页就绪后，从回环地址创建原生子 WebView，默认挂在管理面板；重开复用实例浏览器档案。创建等待上限 20 秒，超时取消本轮发布并返回可操作错误，迟到的创建失败仍落 `mcode-window` 日志。子 WebView 先隐藏创建，只有等待者成功认领且本轮未取消时才显示；停止和退出先取消发布，再停止服务。迟到的子 WebView 按本轮持有的句柄销毁，旧 builder 返回且同名 WebView 清理完成前拒绝重建，避免重启复用旧服务地址；若 WebView 创建一直不返回，提示重启应用。未发布或已取消的 WebView 禁止创建授权弹窗。授权弹窗继承发起方档案，并由本轮 WebView 持有的归属标记在停止或重开时清理；预开窗只接受 `about:blank`，已知认证域只在 HTTPS 下留在壳内，普通 HTTP(S) 外链交系统浏览器。顶层导航只放行本工作台的回环 http(s) 页面（以及 `about:blank` / `blob:` / `data:`），外部 http(s) 外链改交系统浏览器、其它 scheme 拒绝——页面里的普通链接不能把工作台换成别人的站点，日志只记来源。页面仅有外观、编辑菜单、外链权限与 `focus_main_shell`（拉绳灯回到管理面板）一条，无管理命令权限；建窗链还注入与 dsh 工作台同一份 `pullstring-launcher.js`（先写 `window.__DSH_SHELL_SURFACE__ = "mcode"` 让它认出这一面）——灯挂在 rail 右缘内侧、紧挨「收起导航栏」按钮，粉绳与点亮后的粉光随主题取 `#ed94bd` / `#ad3b73` 两档，rail 收起时跟着 `--webui-rail-width` 左移（2026-11-09 用户要求 MiniMax webui 也有 dsh 工作台那盏拉绳灯；2026-11-10 追加要求灯光也偏粉色系）；主题从 MiniMax 的 `webui-theme` 镜像到既有单窗口外观入口：MiniMax 固定浅/深时钉同一档，选 `system` 时**跟随外壳偏好**（与 DSH 内核同一语义——壳固定浅/深就钉该档，壳也是 `system` 才解除覆盖跟随操作系统；2026-11-09 用户要求两侧同步）。弹出窗口标题跟随页面标题，Windows 关闭 Tauri 拖放拦截以保留网页原生拖放；交互效果仍待实机验收。托盘确认退出会停止两种工作台，任一停止失败时保留壳。Windows 使用独立 Job 跟踪进程树，先请求服务正常退出，再在超时后强停并报错，需重试停止才清理记录。

管理窗口的 DSH / MiniMax tab 仅选择页面管理对象；不会停止另一工作台，两个服务可并行。Windows 隔离工作台中，用户确认登录成功后登录层不会自己收起（上游缺陷）；壳已在认证后重载一次工作台并先关掉授权弹窗，但两项都尚未实机复验，也不注入模拟按键干预网页。完整浏览器交互（目录选择、下载、剪贴板、弹窗回调）仍未复验。MiniMax 的 macOS 任务 shell、hook 和终端可能脱离原进程组；正常停止跟踪已观察到的后代，异常退出后的恢复仍依赖上述人工确认。安装版验收尚未完成。入口由体验开关控制，正式壳默认关闭、开发壳默认开启；验收状态统一见 [MiniMax 接入计划](multi-kernel/minimax-webui-integration-plan.md)。

### 会话子 WebView 与弹出窗口

DSH、MiniMax 与官网网页版首次打开时，默认显示在 `main` 管理面板的「会话」页。`session_surface::add_hidden` 用 `Window::add_child` 创建隐藏的原生子 WebView；不是 iframe，也不是把独立 OS 窗口移动到面板上方。

| 会话 | 子 WebView label | 弹出宿主的窗口 label |
| --- | --- | --- |
| DSH 工作台 | `harness` | `harness-popout` |
| MiniMax 工作台 | `mcode-workbench` | `mcode-workbench-popout` |
| 官网网页版 | 本地 `official-chat-strip` 和远程 `official-chat-tab-*` | `official-chat` |

`set_session_surface_slot` 更新面板的子视图区域，Rust 按窗口内逻辑坐标排版。离开「会话」页或切到另一个内嵌会话时，隐藏未显示的视图而非销毁。`set_session_surface_popped_out` 调用 `session_surface::move_surface`，用 `Webview::reparent` 将现有视图迁到另一宿主；官网页签栏和当前已创建的全部内容视图整组迁移，后台预热仍在同一变更锁下查找当前宿主。弹出的工作台默认 1280×840，视图随宿主尺寸、缩放和焦点事件更新；官网内容布局仍按 `OfficialChatRuntime` 选出的活动页签执行。工作台迁移时同步全屏状态，弹出时恢复缓存标题，并派发 `dsh-xlink-surface-host-changed` 使外观按新宿主同步。

「弹出为窗口」和「归位到面板」不重新加载页面、不结束会话。关闭弹出宿主时拦截 `CloseRequested` 并走归位路径；成功迁回 `main` 后销毁空宿主。若 `reparent` 失败，Tauri 不会恢复运行时登记，壳会清理受影响的 WebView 组和空宿主、保留 `main`，由用户从「概览」重开，诊断见 `session-surface` 日志。停止工作台或关闭官网网页版仍是「概览」的动作。临时授权弹窗和三个独立查看器都不参与迁移；查看器的屏幕坐标吸附仍走 `shell/window.rs`。

### 会话 IPC 权限

Capability 按稳定的 **WebView label** 匹配，不按宿主 `Window` label。弹出或归位不改变授权；不要通过 `windows` 给子视图继承面板权限，也不要为弹出宿主补权限或扩大远程来源。

| WebView 选择器 | 来源与权限 |
| --- | --- |
| `webviews: ["main"]` | 本地面板：`core:default`、`opener:default`、`allow-local-commands`。 |
| `webviews: ["harness"]`、`webviews: ["mcode-workbench"]` | 各自 capability 的 `remote.urls` 仅 `http://127.0.0.1:*`；只授各工作台所需的命令。 |
| `webviews: ["official-chat-strip"]` | 本地页签栏：`core:default`、`allow-official-chat-tabs`。 |
| `webviews: ["official-chat-tab-*"]` | `permissions: []`；`remote.urls` 仅 `https://chat.deepseek.com/*`、`https://www.qianwen.com/*`、`https://agent.minimaxi.com/*`、`https://chatgpt.com/*`。 |

## 窗口标题与标题栏主题

### 结论

窗口原生外观只作用于当前窗口：管理面板与查看器听壳偏好，弹出的工作台镜像工作台偏好。内嵌工作台只改自己的 WebView 底色，不接管面板原生标题栏；官网页签与授权弹窗不由壳主题驱动。

### 主题：按窗口内容选择作用域

壳内 UI 用 `html.dark` 控制明暗；`system` 偏好由 `theme.js` 等通过 `prefers-color-scheme` 解析，不靠 CSS 媒体查询另画一套主题。但原生装饰的窗口标题栏由操作系统绘制，它读的是窗口级 appearance。窗口主题因此分三类：

- 壳自有窗口（`main`、日志、用量、套餐）：建窗和页加载使用外观兜底，挂载和主题切换通过 `applyTheme()` 更新本窗口。原生外观仍由 `shell/appearance.rs` 管理，不得借应用级主题或改变其他窗口的 WebView 配色模拟单窗外观。
- DSH 和 MiniMax 工作台不钉建窗主题。`workbench-appearance.js` 镜像工作台主题：工作台固定 `light` / `dark` 时以该选择为准，选 `system` 时采用壳偏好；壳也是 `system` 就传 `null` 解除覆盖，跟随操作系统。不得把媒体查询解析出的明暗再钉回窗口。
- 官网页签栏保持自己的深色界面；官网远程内容与独立授权弹窗都不授壳的外观权限。授权弹窗不钉建窗主题。

`shell/appearance.rs::set_window_appearance` 从调用者 WebView 获取当前宿主。工作台先更新 WebView 底色，位于 `main` 时直接返回；只有弹出后才可更新其宿主的原生外观。macOS 使用当前 `NSWindow` 的 appearance，不用会影响整个 `NSApplication` 的 Tauri 应用主题。Windows 的窗口主题可能经 tao / wry 改写共享的 WebView2 profile，所以检测到 `harness` WebView 时，跳过壳自有窗口的原生主题更新；壳内容仍照常切换。

壳自有四窗首帧底色 `#0a100f` 不与工作台的 `CHROME_BACKDROP`（`#16171a`）混用。工作台浅色底色为 `#fff`，其余为 `#16171a`；页面未铺色的滚动条预留槽会露出这层底色。DSH 加载页（`ui/public/kernel-loading.html`）读壳主题同一个 localStorage 键，`system` 档按 `prefers-color-scheme` 解析；取不到偏好保持默认深色。壳偏好经 `report_shell_theme` / `get_shell_theme` 传递；Rust 直接向相应 WebView 派发 `dsh-shell-theme` DOM 事件，工作台镜像脚本还有轮询兜底，无需给远程视图开放 Tauri 事件监听权限。前端细则见 [ui/AGENTS.md](../../ui/AGENTS.md)。

### 标题：同一份原生标题

`main` 的原生标题由 `tauri.conf.json` 的 `title` 提供，与 `shell/window.rs` 的 `APP_TITLE` 一致。副窗初始标题由 `window::window_title(窗口名)` 拼出「功能名 — Dsh-Xlink」，分隔符是**两侧带空格的破折号**，不是 `-`。日志查看器保留 Rust 构造的初始标题 `日志 — Dsh-Xlink` 或 `日志 · <日志名> — Dsh-Xlink`；自动选中或切换日志后，活动日志名同步到该窗原生标题。副窗在内容区保留自己的功能页头，但不再另画一份浏览器内标题栏。

工作台通过 `on_document_title_changed` 按 WebView 缓存页面标题；内嵌时不修改 `main` 的原生标题，弹出时把缓存标题写到工作台自己的宿主。

### DSH 创建路径

初次打开、启动加载态及自愈 / 手动重建共用 `harness_window::open_at` → `build`。构造 `harness` 子 WebView 的同一 builder 接好授权弹窗路由、标题回调、草稿、右键兜底、外观镜像、诊断与页加载后的全屏同步。修改注入脚本或回调只改这个入口；弹出和归位不会走 builder。

## 生命周期与退出契约

本节承接根 `AGENTS.md` 的前后端事件细节；完整运行时说明另见[当前架构的后台常驻与自启](architecture.md#登录自启shellautostart)。

`lib.rs` 将窗口关闭请求交给 `resident::intercept_close`；其中仅 `main` 拦截关闭并走 `hide_to_shell`。弹出会话宿主另有关闭处理，只触发归位；系统最小化与独立查看器关闭不走后台常驻流程。

- Windows 收起与恢复的 `set_skip_taskbar(true)` / `(false)` 必须成对；`show()` 不会自动补回 `ITaskbarList::DeleteTab` 删除的任务栏按钮。
- 收起时不广播页内提示，恢复时才发 `shell-restored-from-background`；仅本进程发生过用户收起后的首次恢复可发，登录自启隐藏不算。事件名变更同步 `App.vue` 与本文。
- 退出由 `shell/quit.rs` 发带 `request_id` 的 `request-quit-confirm`，仅主面板经 `acknowledge_quit_request` 认领，前端等应答成功再弹确认。两秒从发起请求开始计算，包含状态读取和事件发送；发送失败立即进入原生兜底，迟到应答拒绝。MiniMax 退出状态只检查启动锁、运行槽、配置、进程记录和运行占位，不触发 Node 或安装探测；状态无法确认时按需要停止处理。正常路径与兜底都复用 `stop_kernel` / `confirm_close_shell`，停止失败不得继续退出。
- 通知激活的 Windows 管道交接和 macOS 激活观察都接 `notify::activate::serve`。回到工作台失败时，用 `kernel_running` 判断：内核未跑不提示故障，内核在跑却打不开才广播 `workbench-activate-failed` 给 `App.vue`；不匹配错误文案作判据。
- 冷启动时 `harness_cmd::harness_open_loading` 先用 `kernel-loading.html` 创建加载态子 WebView。内核就绪后 `commands::open_harness` 关闭占位视图，按内核地址用同一 builder 新建工作台，保留此前的弹出状态。不能用 `navigate()` 替代：此前从 `tauri://localhost` 导航，WKWebView 在 303 重定向后的 `/` 没有发送新下发的 `SameSite=Strict` 会话 cookie，停在 401「dsh web authentication required」；新视图首次加载则正常。启动失败时由启动链清理占位视图；`harness-health.js` 跳过加载页，避免把无内核 DOM 判作白屏。占位替换及显式重建是重新创建 WebView 的路径，与平常的弹出和归位不同。
- 系统登录项由 `shell/autostart.rs` 管理，启动工作台偏好由 `Settings::autostart_kernel` 管理；两者分开。自动测试只测命令拼装、独立 token 判定、路径归一化和启动条件等纯逻辑，不读写删除真实登录项。系统写路径仅在安装好的机器上人工验证。

相关接线由不变量第 17 / 29 / 36 项覆盖；平台符号按目标系统门控。事故背景见[复盘 §6](../reviews/agent-lessons.md)。

## 核心模块接口

`src-tauri/src/shell/window.rs` 提供窗口规格、吸附几何与移动跟随等能力。

### 窗口规格

`WindowSize` 表达逻辑像素下的默认宽高；`*_VIEWER_LABEL` 和 `*_VIEWER_SIZE` 是当前弹窗登记表。

窗口规格服务两个位置：

1. WebView builder 的初始 `inner_size`。
2. 初次打开时的吸附计算。

新增弹窗时先登记规格，再在创建命令中引用它。不要在 `lib.rs`、创建命令和文档中分别写一组数字。

### 几何计算

几何计算全部使用物理像素：

- `dock_x`：优先放主窗右侧，右侧放不下则翻到左侧。
- `dock_y`：顶边对齐，底部超出显示器时上移。
- `clamp_position`：将结果夹在显示器范围内；当窗口大于显示器时退化到显示器起点。
- `compute_dock_position`：读取主窗位置、外框尺寸、缩放比和当前显示器，计算默认规格下的物理坐标。
- `dock_position_logical`：将物理坐标换算为 builder 所需的逻辑坐标。

物理坐标是必要的：主窗口可能位于负坐标的左侧显示器，且主窗口与子窗口可能处于不同缩放比的显示器边界附近。不要在 UI 层用 `window.innerWidth` 或 CSS 坐标推算屏幕位置。

### 移动跟随

`attach_dock_listener` 只监听 `main` 的 `Moved` 事件：

1. 目标窗口不存在时直接返回，setup 阶段即可注册，不要求弹窗已打开。
2. 首个移动事件或距离上一次应用超过 400ms 时调用一次 `set_focus`，让副窗不被其它应用压在后面。
3. 15ms 合帧一次 `set_position`，避免将高频 `Moved` 事件全部排入主线程队列。
4. 跟随时读取目标窗口当前 `outer_size`，所以用户调整可缩放副窗后，后续跟随仍使用真实尺寸；读取失败则跳过这一帧。
5. 只从主窗事件驱动，不监听副窗移动，因此不会形成两个窗口互相拉扯的回环。

这套实现的接口很小，但实现承担了平台事件频率、显示器坐标、缩放换算、z-order 和窗口生命周期等复杂度，符合一个有价值的深模块：调用方只需提供 label，不必理解这些细节。

## 新弹窗接入流程

1. 在 `window.rs` 登记唯一 label 和默认 `WindowSize`。
2. 确定弹窗是否真的是独立 OS 窗口。若只是父窗口内的子 WebView，应使用独立 layout 模块，不接入 dock。
3. 在 `setup` 中调用 `attach_dock_listener`。
4. 创建窗口时通过 `dock_position_logical` 设置初始位置；取不到主窗或显示器信息时允许 builder 默认居中。
5. 创建窗口使用独立 OS 线程，并将 build 结果返回给 UI。
6. 将窗口专属命令放入独立 capability，遵循最小权限原则。
7. 为几何纯函数补测试，至少覆盖：右侧空间足够、右侧翻左、负坐标显示器、垂直夹回、窗口大于显示器。
8. 在 `docs/architecture/architecture.md` 和用户文档中记录窗口 label、尺寸、关闭语义和是否保留独立状态。

建议的调用形态：

```rust
window::attach_dock_listener(app.handle(), window::NEW_VIEWER_LABEL);

let dock = handle.get_webview_window("main").and_then(|main| {
    crate::window::dock_position_logical(&main, crate::window::NEW_VIEWER_SIZE)
});
```

## 当前代码审查

### 已完成且可复用

- 日志查看器已经从用量窗口复用同一套吸附和跟随实现，不再存在两份 dock 算法。
- 监听器对未打开的目标窗口短路，主窗口启动时即可注册，不需要额外的“窗口打开后绑定”状态机。
- 物理坐标和逻辑坐标的转换集中在 Rust，避免 UI 与 Rust 对屏幕坐标各算一遍。
- 移动跟随有 15ms 合帧，且拖动回合开始会提升副窗 z-order，解决了高频事件堆积和跨应用移动时的穿插问题。
- 主窗口、独立查看器与可迁移的会话子 WebView 各有独立职责；会话 IPC 按 WebView label 隔离，不随窗口宿主改变。

### 本次修正

- 原先日志和用量窗口的尺寸常量分别分散在 `commands.rs`、`usage.rs` 和 `lib.rs`。现在统一登记在 `window.rs`，避免新增弹窗时只更新了创建尺寸却忘记更新跟随尺寸。
- 原先跟随始终用初始逻辑尺寸计算。可缩放副窗调整大小后，位置会逐渐偏离主窗边缘；现在跟随优先读取目标窗口当前物理外框尺寸。
- 原先当副窗宽度或高度大于显示器时，翻转坐标可能仍落在屏幕外；现在统一经过屏幕边界夹回，并覆盖该场景的测试。

### 仍需关注

- `set_focus` 是平台能力上的折中：tao 没有“提升 z-order 但不抢焦点”的统一 API。当前行为每个拖动回合只触发一次，仍可能让焦点短暂落到副窗；若产品要求拖动主窗时焦点始终留在主窗，需要按平台增加原生 adapter，而不是在通用模块中继续堆条件分支。
- 跟随目前只处理 `Moved`。主窗缩放若没有同时产生移动事件，不会立即重新吸附；下一次移动会按当前外框重新计算位置。
- 副窗口被用户手动拖走后，下一次主窗口移动会把它重新拉回吸附位置。当前语义是“独立查看器始终跟随”，若未来需要“用户拖走后脱离”，应增加显式 docking 状态，而不是通过坐标猜测。
- `open_log_window` / `open_usage_window` / `open_subscription_window` 各自复制了「销毁旧窗口、创建线程、20 秒等待、错误包装」的建窗样板。原先「两个调用点，暂不抽象」的判断已过期——`subscription-viewer` 出现后是**三个**。其中 `subscription-viewer` 的规格直接复用 `USAGE_VIEWER_SIZE`，正是样板里唯一真正不同的字段。提取窗口创建 adapter 的收益前提已具备，应让 adapter 明确处理 label、URL、尺寸、背景色、超时和错误文案。
- 当前没有真实多显示器/缩放比的自动化窗口测试。纯函数测试能覆盖几何规则，但不能证明 Tauri 在 macOS/Windows 上返回的 `outer_position`、`outer_size` 和 `current_monitor` 口径完全一致。发布前仍需要两平台手工验证。

## 测试与验证边界

静态检查覆盖建窗配置、标题接线、内容区标识线和吸附几何规则，不能证明系统窗口按钮、首帧与实际坐标的表现。以下行为需要分别在 macOS 与 Windows 实机验证：

- 四扇壳自有窗口使用系统标题栏、控件和阴影，首帧没有白底，管理面板的构建标识线位于内容区。
- 主窗能拖边调整大小、最大化、进入与退出全屏；窄窗下内容仍可访问。
- 四扇窗口原生最小化后从任务栏 / Dock 恢复；三扇查看器可最大化和拖边调整大小。关闭主窗再从托盘 / 菜单栏恢复；关闭查看器后可重新创建。
- 初次打开时查看器按允许的尺寸对齐主窗口；主窗拖动时查看器跟随，靠右时能翻到左边，在负坐标显示器上仍吸附到同一屏。仅调整大小而未触发移动时，不要求立即重新吸附。
- 100%、125%、150% 缩放下初始位置与跟随位置不跳变；查看器变更尺寸后再次拖动主窗，按实际外框贴边。
- 拖动主窗穿过其他应用窗口时，副窗层级符合平台预期；主窗收起、恢复、退出时，副窗不阻碍生命周期清理。
- 分别用指定日志和空日志名打开日志窗，确认初始标题正确；自动选中或切换日志后，原生标题显示当前活动日志。
- 三类会话从「概览」初次打开都落在面板「会话」页；切换会话及离开页面时隐藏非活动视图，不覆盖旁边的面板内容。分别弹出、归位、再通过系统关闭按钮归位，确认草稿、登录态和正在进行的对话保留，无重新加载。官网页签栏和各内容页整组移动，预热、转圈和页签切换仍正常。
- 每个弹出宿主验证缩放、全屏、焦点和输入；DSH 与 MiniMax 的标题只更新自己的弹出窗口。分别切换浅色、深色与跟随系统，确认内嵌工作台不改主窗原生主题；Windows 上另确认 `harness` 打开时壳窗口的主题切换不改写工作台配色。
- 验证授权弹窗仍独立且继承发起页档案，选区复制与工作台编辑菜单在迁移前后可用；通过「概览」停止工作台或关闭官网网页版后，无残留会话宿主。
