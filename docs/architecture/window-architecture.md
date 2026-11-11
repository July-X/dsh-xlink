# 窗口核心能力设计与代码审查

本文整理 dsh-xlink 当前主窗口、独立子窗口、窗口吸附和拖拽移动跟随的实现，作为后续弹出窗口的接入规范。

## 结论

当前实现已经形成一个可复用的窗口能力模块：`src-tauri/src/shell/window.rs`。日志查看器和模型用量窗口都通过它完成吸附定位与主窗移动跟随，调用方只负责窗口内容、权限和创建线程。

当前能力适合继续复用，但扩展弹窗前必须遵守两条约束：

- 窗口 label 和默认尺寸登记在 `window.rs`，创建和初始吸附不得各自维护尺寸常量。
- 吸附是“主窗口驱动的单向跟随”，不是两个窗口之间的双向绑定。当前实现中副窗口手动移动后，下一次主窗口移动仍会重新贴回；若产品需要脱离吸附，应增加显式 docking 状态。

## 窗口分层

### 主窗口 `main`

- 配置入口：`src-tauri/tauri.conf.json`（Windows 的覆盖在 `src-tauri/tauri.windows.conf.json`）。
- 固定为 1040×748、不可缩放、声明式无边框。
- macOS / Windows 的标题栏由 `ui/src/shell/WindowTitleBar.vue` 绘制。
- 标题栏通过 `data-tauri-drag-region` 把非按钮区域交给 Tauri 的系统拖动能力。
- 关闭、最小化和托盘恢复属于主窗口生命周期，不应混入通用吸附模块。
- **窗口外观按平台分叉（2026-11-02）**：macOS 用 `transparent: true` + CSS 圆角；Windows
  由平台配置覆盖成不透明窗口——透明窗口会让 WebView 关掉 ClearType、文字只剩灰度抗锯齿
  （用户报的「字体发虚」），代价是 Win10 没有系统圆角，`body.platform-windows` 把
  `--window-radius` 归零。两边的判据在 `ui/test/windowChrome.test.js`，细则见
  [ui/AGENTS.md](../../ui/AGENTS.md) 的「窗口 chrome」一节。

无边框必须在创建配置中声明。运行时再调用 `set_decorations(false)` 会改变 macOS 的窗口样式位，可能丢失可最小化能力；这属于主窗口平台适配约束，不是弹窗通用逻辑。

### 独立窗口

当前有**三种**独立窗口；其中模型用量 / 套餐用量两扇的界面入口已于 2026-10-28 移除（内容并进侧栏「工作台 → 用量分析」页），但窗口组件、Rust 命令与 capability 仍保留：

| label | 创建入口 | 内容 | 默认规格 | 能力 |
| --- | --- | --- | --- | --- |
| `usage-viewer` | `usage::open_usage_window`（无界面入口） | `?usage=1` | 760×800（`USAGE_VIEWER_SIZE`） | 吸附、跟随、可缩放 |
| `subscription-viewer` | `subscription::open_subscription_window`（无界面入口） | `?subscription=1` | 760×800（复用 `USAGE_VIEWER_SIZE`） | 吸附、跟随、可缩放 |
| `log-viewer` | `commands::open_log_window` | `?log=<name>` | 960×720（`LOG_VIEWER_SIZE`） | 吸附、跟随、可缩放 |

三者都在独立 OS 线程创建 WebView。原因是 Windows 上在命令线程同步创建 WebView 可能死锁；创建结果通过 `mpsc` 返回，调用方在 blocking worker 中等待并提供 20 秒超时。三扇窗的建窗装饰统一走 `shell::window::decorate_transparent`：无边框 + 无系统阴影 + 不可最小化，**透明/底色按平台分（macOS 透明 + CSS 圆角、Windows 不透明 + 直角）**，理由同主窗口那条。

**「界面入口移除」不等于「能力删除」**：`usage-viewer` / `subscription-viewer` 的组件、命令与 capability 全部保留，`main.js` 的窗口路由（`?usage=1` / `?subscription=1`）照旧挂载；「用量分析」页只是用 `UsageWindow` 的 `inline` 模式复用前者内容，面板不再调用两扇窗口的打开命令。先例见「刷新工作台」与 `setDefaultInstance`。

### MiniMax 实验工作台

Unix 启动由只读内嵌的 `mcode-unix-launcher.cjs` 负责：壳持有 stdin 写端，持久化 PID 后才写入放行字节；服务不继承管道。开发重启、崩溃或强制结束壳导致 EOF 时，启动器对自身进程组发 TERM，一秒后 KILL，期间保留组长身份防止 PGID 复用。服务提前退出也触发同样收尾，不能把同组的另一监听服务留下。正常停止在发信号前补拍进程树，再核验跟踪到的脱离进程组后代及两个端口。启动器也被强杀、或异常退出前后代已自行 setsid 脱离组的情况不在父管道兜底保证内，旧记录仍保守保留，不按端口或旧 PID 杀进程。此回归已在 Linux 隔离双端口夹具验证，macOS 实包及安装产物仍待复验。

异常退出后不能仅凭旧 PID 已消失或双端口空闲推断整棵进程树已结束，所以 Unix 侧的跟踪器把进程树身份名册落盘到实例的 `runtime/process-tree.json`（pid + 启动时间 + 进程表来源，名册一有变化就原子写）。壳重启后先核对这份证据：名册里的身份全部不在、两个端口都空闲，就自动清理运行记录、进程树证据与运行占位并写 `mcode-lifecycle` 日志——不再让用户为「进程其实都不在了」去做无意义的重启（2026-11-09 用户要求）；名册机制上线前写的旧记录没有证据文件，改用「记录里的启动器已不在 + 它所在进程组已空 + 双端口空闲」核对后同样自动清理（启动器 setsid 当组长、服务继承同一 pgid），看不到的只有自行 setsid 脱离进程组的后代；只有证据损坏 / 仍有活进程时才保留记录并显示「重启后清理记录」，由用户确认。自动清理只删记录，不按旧 PID 或端口结束任何进程。概览的手动清理同样要求用户先重启电脑、确认没有其他窗口运行 MiniMax，再明确确认；确认绑定实例、双端口与 PID 文件和运行占位的内容快照，后端持有启动/安装锁及已登记实例的跨进程生命周期租约，再检查快照、端口、记录归属和记录中的进程是否仍在。进程查询只用于拒绝有疑点的操作，不作为后代全部退出的证明。记录变化、端口占用、损坏或归属不明时均拒绝并保留；清理先移除占位、再移除 PID，以便中断时仍有记录阻止启动。仅剩占位时也可进入恢复。Windows 依赖 Job 的 `KILL_ON_JOB_CLOSE`，壳关闭即整树结束，因此 PID 不在 + 端口空闲即可自动清理，不落证据文件。已经接入离线协议的实例无论是否启用共享技能，启动前都必须创建运行占位，不能在旧占位存在时再次启动或更改端口。旧版 socket 接管入口已移除：真实崩溃收尾后它无法可靠接管已脱离进程组的后代。


`mcode-workbench` 由 `harness/mcode_window.rs` 在服务归属及首页就绪后从回环地址直接建窗，初始尺寸 1280×840；关闭窗口保留服务，重开复用实例浏览器档案。建窗等待上限 20 秒（与其它三扇独立窗口一致），超时取消本轮发布并返回可操作错误，迟到的建窗失败仍落 `mcode-window` 日志。窗口先隐藏创建，只有等待者成功认领且本轮未取消时才显示；停止和退出先取消发布，再停止服务。迟到窗口按本轮持有的句柄销毁，旧 builder 返回且同名窗口清理完成前拒绝重建，避免重启复用旧服务地址；若 WebView 创建一直不返回，提示重启应用。未发布或已取消的窗口禁止创建授权弹窗。授权弹窗继承发起方档案，并由本轮窗口持有的归属标记在停止或重开时清理；预开窗只接受 `about:blank`，已知认证域只在 HTTPS 下留在壳内，普通 HTTP(S) 外链交系统浏览器。顶层导航只放行本工作台的回环 http(s) 页面（以及 `about:blank` / `blob:` / `data:`），外部 http(s) 外链改交系统浏览器、其它 scheme 拒绝——页面里的普通链接不能把工作台换成别人的站点，日志只记来源。页面仅有外观、编辑菜单、外链权限与 `focus_main_shell`（拉绳灯回到管理面板）一条，无管理命令权限；建窗链还注入与 dsh 工作台同一份 `pullstring-launcher.js`（先写 `window.__DSH_SHELL_SURFACE__ = "mcode"` 让它认出这一面）——灯挂在 rail 右缘内侧、紧挨「收起导航栏」按钮，粉绳与点亮后的粉光随主题取 `#ed94bd` / `#ad3b73` 两档，rail 收起时跟着 `--webui-rail-width` 左移（2026-11-09 用户要求 MiniMax webui 也有 dsh 工作台那盏拉绳灯；2026-11-10 追加要求灯光也偏粉色系）；主题从 MiniMax 的 `webui-theme` 镜像到既有单窗口外观入口：MiniMax 固定浅/深时钉同一档，选 `system` 时**跟随外壳偏好**（与 DSH 内核同一语义——壳固定浅/深就钉该档，壳也是 `system` 才解除覆盖跟随操作系统；2026-11-09 用户要求两侧同步）。窗口标题跟随页面标题，Windows 关闭 Tauri 拖放拦截以保留网页原生拖放；交互效果仍待实机验收。托盘确认退出会停止两种工作台，任一停止失败时保留壳。Windows 使用独立 Job 跟踪进程树，先请求服务正常退出，再在超时后强停并报错，需重试停止才清理记录。

管理窗口的 DSH / MiniMax tab 仅选择页面管理对象；不会停止另一工作台，两个服务可并行。Windows 隔离工作台中，用户确认登录成功后登录层不会自己收起（上游缺陷）；壳已在认证后重载一次工作台并先关掉授权弹窗，但两项都尚未实机复验，也不注入模拟按键干预网页。完整浏览器交互（目录选择、下载、剪贴板、弹窗回调）仍未复验。MiniMax 的 macOS 任务 shell、hook 和终端可能脱离原进程组；正常停止跟踪已观察到的后代，异常退出后的恢复仍依赖上述人工确认。安装版验收尚未完成。入口由体验开关控制，正式壳默认关闭、开发壳默认开启；验收状态统一见 [MiniMax 接入计划](multi-kernel/minimax-webui-integration-plan.md)。

### 裸窗口与子 WebView

`official-chat` 是另一种形态：父窗口是裸 `tauri::Window`，页签栏和远程内容是通过 `Window::add_child` 挂载的子 WebView。它需要自己的 layout/resize 管理，不应强行接入独立窗口吸附模块。

这里要区分两个概念：

- **独立窗口吸附**：多个 OS 窗口之间通过屏幕物理坐标关联。
- **父窗口子 WebView 布局**：同一个 OS 窗口内部通过 logical position/size 排列子视图。

## 窗口标题与标题栏主题

### 结论

窗口标题栏按窗口内容分域：壳自有窗口按应用主题纠正，工作台按内核 `ui-theme` 偏好镜像（内核选 `system` 时改由壳偏好给出答案），官网页签窗保持第三方页面自己的深色外观。副窗标题统一是 `窗口名 — Dsh-Xlink`。这不是审美偏好，是窗口内容与原生 chrome 必须同色、且工作台「跟随系统」不能被壳冻住的硬要求。

### 主题：按窗口内容选择作用域

`ui/` 下 `prefers-color-scheme` **零命中**——壳内 UI 的明暗由 `html.dark` 控制。但原生装饰的窗口标题栏由操作系统绘制，它读的是窗口级 appearance。窗口主题因此分三类：

- 壳自有窗口（`main`、日志、用量、套餐）：建窗走 `shell::appearance::initial_theme(label)`，页加载与 `applyTheme()` 再按应用偏好纠正当前窗口。**Windows 上这一步实际是空操作**：它们全是自绘标题栏，窗口主题在 Windows 只会写共享 WebView2 profile（见下文）。
- 工作台 `harness`：建窗同样经过 `initial_theme(WORKBENCH_LABEL)`，但该函数返回 `None`；注入的 `workbench-appearance.js` 先看内核发布的 `html[data-ds-theme-source]`。内核偏好固定 `light` / `dark` 时钉同一档（用户在内核里的明确选择优先）；内核偏好为 `system` 时采用主壳上报的壳偏好——壳固定浅 / 深就钉该档，壳也是 `system` 就下发 `null` 解除覆盖、跟随系统。macOS / Windows 的系统级明暗切换因此在两侧同拍（壳自己有 `matchMedia` 监听）。
- 官网页签窗与工作台弹窗：承载第三方 / 授权页面，不拿壳的窗口外观权限，也不由壳主题驱动。

这样既避免浅色系统下深色内核内容配浅色标题栏，也不会因壳在建窗时钉死一档而破坏内核「跟随系统」；产品语义是 2026-11-02 定的 B 方案——**内核选「跟随系统」才跟随外壳，内核固定档听用户在内核里的选择**。

`CHROME_BACKDROP`（首帧前的暗底过渡色）是统一的中性过渡色，不再按应用主题分支。两条工作台建窗链（`commands::open_harness` 与 `harness_window::build`）必须同步挂外观镜像脚本与主题作用域参数。

实现集中在 `shell/appearance.rs`：macOS 的 Tauri `set_theme` 会影响 `NSApplication`，因此壳自有窗口的 `initial_theme` 返回 `None`，其他平台可给深色兜底；页加载时由 app 级 `on_page_load` 逐窗口补一次深色，挂载后经 `bridge.setWindowTheme` → `set_window_appearance` 按持久化偏好纠正当前窗口，禁止用应用级主题模拟单窗外观。

Windows 的壳自有窗口全是自绘标题栏，窗口主题在它们身上只会经 tao 的 `ThemeChanged` → wry 的 `WebView::set_theme` 写进**共享的 WebView2 profile**。工作台开着时那会把内核页面的 `prefers-color-scheme` 提前改掉，而工作台标题栏还在等镜像脚本下发——两半一前一后（2026-11-02 用户报「内核界面和 titlebar 切换速度不一致」）。所以工作台开着时 `set_window_appearance` 对壳自有窗口直接跳过（`applies_native_theme`），工作台的标题栏与页面由同一次下发一起切；工作台没开时照常下发，让 profile 提前对齐，下次开窗不会先渲染一帧 OS 配色再翻。

`initial_theme` / `initialize` 跳过 `harness` 与 `workbench-popup-*`，`owns_appearance` 对这两类返回 false。工作台的镜像权限由 `capabilities/harness-remote.json` 授予（`allow-window-appearance` + `allow-get-shell-theme`）；壳偏好经 `report_shell_theme` / `get_shell_theme` 两条命令传递：主壳上报后 Rust 用 `eval` 直接推一个 `dsh-shell-theme` DOM 事件给工作台（remote origin 的 Tauri event `listen` 需要额外的 `core:event:allow-listen` 授权，漏了是静默失败），轮询兜底。内核偏好为 `system` 且壳也是 `system` 时传 `null` 解除覆盖，不能把解析后的深浅结果再钉回窗口，否则页面媒体查询会被自己冻结。静态不变量第 18 项检查两条工作台链、主题出口与镜像脚本接线；前端细则仍见 [ui/AGENTS.md](../../ui/AGENTS.md)。

### 标题：一个名字，三处副本

`Dsh-Xlink` 出现在三个地方，任何一处单独改都会让标题对不上：

| 位置 | 形态 |
| --- | --- |
| `shell/window.rs` 的 `APP_TITLE` | 副窗标题的拼装源 |
| `tauri.conf.json` 的主窗 `title` | 主窗口的原生标题（无边框时看不见，但工具链读得到） |
| `ui/src/shell/WindowTitleBar.vue` | 主窗口**自绘**标题栏上真正显示的字 |

三处分处 Rust / 配置 / Vue，单测与 UI 测试都够不着，因此由
`scripts/check-invariants.mjs` 第 18 项做机械比对；同一项还管「每扇副窗都走
`window_title()`」与工作台两条建窗链的主题作用域、外观镜像脚本接线。

副窗标题一律 `window::window_title(窗口名)`，分隔符是**两侧带空格的破折号**（不是 `-`）：
`日志 · kernel` 这类窗口名里已经带了一次分隔，再用半角连字符会出现两个形状几乎一样的短横。

**窗口内自绘标题栏是另一份格式**：`WindowTitleBar.vue` 对副窗拼 `功能名@应用名`
（如 `模型用量@Dsh-Xlink`），主壳不传 title、只显示应用名。分隔符用 `@` 而不是系统标题
的 ` — `：系统标题出现在任务栏 / Dock，空间宽裕；窗口内标题栏更窄，与交通灯同一行，
` — ` 会读成两个短语。两支都由 `check-invariants` 第 18 项**真实求值** `const caption = …`
钉住（主壳等于 `APP_TITLE`，副窗等于 `功能名@APP_TITLE`），`ui/test/windowChrome.test.js`
另钉三扇副窗各自传的功能名与日志窗的兜底。

### 两条仍需关注

- **工作台窗口有两份建窗实现**。`commands::open_harness` 内联了一次
  `WebviewWindowBuilder`，`harness_window::build` 又有一次，两者都用 `harness` 这个 label。
  两条链都必须传入 `initial_theme(WORKBENCH_LABEL)`（返回 `None`，不钉建窗主题），并挂
  `workbench-appearance.js`，让内核 `ui-theme` 偏好负责标题栏外观。差异只有一条：重建那份
  多挂了 `on_navigation`（把页面导航记进 `shell_events`，正是 2026-09-30 那次黑屏的诊断手段）。
  合流会让首次打开也多记导航——那大概率是想要的，但这属于行为变更，没有并进本次改动。
  两条链的主题作用域、暗底过渡色和外观镜像接线必须同步，改一处漏一处的风险只剩在其余
  初始化脚本列表上。
- `official-chat-*` 与 `workbench-popup-*` 承载第三方 / 授权页面，不拿壳的窗口外观权限；
  工作台虽拿 `allow-window-appearance`，也只用于镜像内核偏好，不由壳主题或建窗兜底钉死。

## 生命周期与退出契约

本节承接根 `AGENTS.md` 的前后端事件细节；完整运行时说明另见[当前架构的后台常驻与自启](architecture.md#登录自启shellautostart)。

- 关闭窗口统一走 `resident::intercept_close` 收入后台，真正退出从托盘 / 菜单栏图标菜单发起。恢复与收起分别复用 `resident::show_main_shell` / `resident::hide_to_shell`；macOS 收起需降到 Accessory，`RunEvent::Reopen` 无工作台可抬时兜底恢复主面板。
- Windows 收起与恢复的 `set_skip_taskbar(true)` / `(false)` 必须成对；`show()` 不会自动补回 `ITaskbarList::DeleteTab` 删除的任务栏按钮。
- 收起时不广播页内提示，恢复时才发 `shell-restored-from-background`；仅本进程发生过用户收起后的首次恢复可发，登录自启隐藏不算。事件名变更同步 `App.vue` 与本文。
- 退出由 `shell/quit.rs` 发带 `request_id` 的 `request-quit-confirm`，仅主面板经 `acknowledge_quit_request` 认领，前端等应答成功再弹确认。两秒从发起请求开始计算，包含状态读取和事件发送；发送失败立即进入原生兜底，迟到应答拒绝。MiniMax 退出状态只检查启动锁、运行槽、配置、进程记录和运行占位，不触发 Node 或安装探测；状态无法确认时按需要停止处理。正常路径与兜底都复用 `stop_kernel` / `confirm_close_shell`，停止失败不得继续退出。
- 通知激活的 Windows 管道交接和 macOS 激活观察都接 `notify::activate::serve`。回到工作台失败时，用 `kernel_running` 判断：内核未跑不提示故障，内核在跑却打不开才广播 `workbench-activate-failed` 给 `App.vue`；不匹配错误文案作判据。
- 冷启动时内核从派生到端口就绪实测 55 秒。用户在概览页点「工作台」后，`harness_cmd::harness_open_loading` 先开一扇挂壳自己加载态页（`ui/public/kernel-loading.html`，产物在 `ui/dist/`）的工作台窗；内核就绪后 `commands::open_harness` **拆掉这扇占位窗、按内核地址新开一扇**（2026-11-02 修正：不能用 `WebviewWindow::navigate`——从 `tauri://localhost` 页面发起的导航，WebKit 在 303 重定向后的 `/` 上不会带上内核刚下发的 `SameSite=Strict` 会话 cookie，页面停在 401「dsh web authentication required」；新窗口的首次加载没有发起方，cookie 照常带上）。注入脚本由 `harness_window::build` 与 `commands::open_harness` 两条建窗链各自注入——wry 走 `AddScriptToExecuteOnDocumentCreated` / `WKUserScript`，每次文档创建都执行，所以草稿 / 外观镜像 / 右键策略那些脚本在内核页面上照常在。内核起不来时 `start_kernel` 收掉这扇窗：加载态窗口的生死由启动那条路决定，别的入口（通知横幅 / 任务栏图标也会调 `open_harness`）不碰它。加载态页面没有内核 DOM，`harness-health.js` 的空白判据在它上面恒为真，脚本按地址认出加载态页并整个不装（否则会在内核还在启动的时候凭空报一桩「工作台白屏」事故）。
- 系统登录项由 `shell/autostart.rs` 管理，启动工作台偏好由 `Settings::autostart_kernel` 管理；两者分开。自动测试只测命令拼装、独立 token 判定、路径归一化和启动条件等纯逻辑，不读写删除真实登录项。系统写路径仅在安装好的机器上人工验证。

相关接线由不变量第 17 / 29 / 36 项覆盖；平台符号按目标系统门控。事故背景见[复盘 §6](../reviews/agent-lessons.md)。

## 核心模块接口

`src-tauri/src/window.rs` 当前提供四层能力。

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
- 主窗口、独立窗口和官方对话子 WebView 的职责已有清晰区别，权限也按窗口类型分开。

### 本次修正

- 原先日志和用量窗口的尺寸常量分别分散在 `commands.rs`、`usage.rs` 和 `lib.rs`。现在统一登记在 `window.rs`，避免新增弹窗时只更新了创建尺寸却忘记更新跟随尺寸。
- 原先跟随始终用初始逻辑尺寸计算。可缩放副窗调整大小后，位置会逐渐偏离主窗边缘；现在跟随优先读取目标窗口当前物理外框尺寸。
- 原先当副窗宽度或高度大于显示器时，翻转坐标可能仍落在屏幕外；现在统一经过屏幕边界夹回，并覆盖该场景的测试。

### 仍需关注

- `set_focus` 是平台能力上的折中：tao 没有“提升 z-order 但不抢焦点”的统一 API。当前行为每个拖动回合只触发一次，仍可能让焦点短暂落到副窗；若产品要求拖动主窗时焦点始终留在主窗，需要按平台增加原生 adapter，而不是在通用模块中继续堆条件分支。
- 跟随只处理 `Moved`。如果未来主窗口允许缩放，必须同时监听 `Resized`，并重新计算所有已吸附窗口；当前主窗口不可缩放，所以不监听是合理的。
- 副窗口被用户手动拖走后，下一次主窗口移动会把它重新拉回吸附位置。当前语义是“弹出窗口始终跟随”，若未来需要“用户拖走后脱离”，应增加显式 docking 状态，而不是通过坐标猜测。
- `open_log_window` / `open_usage_window` / `open_subscription_window` 各自复制了「销毁旧窗口、创建线程、20 秒等待、错误包装」的建窗样板。原先「两个调用点，暂不抽象」的判断已过期——`subscription-viewer` 出现后是**三个**。其中 `subscription-viewer` 的规格直接复用 `USAGE_VIEWER_SIZE`，正是样板里唯一真正不同的字段。提取窗口创建 adapter 的收益前提已具备，应让 adapter 明确处理 label、URL、尺寸、背景色、超时和错误文案。
- 当前没有真实多显示器/缩放比的自动化窗口测试。纯函数测试能覆盖几何规则，但不能证明 Tauri 在 macOS/Windows 上返回的 `outer_position`、`outer_size` 和 `current_monitor` 口径完全一致。发布前仍需要两平台手工验证。

## 测试与验证边界

当前可自动验证的接口是 `dock_x`、`dock_y`、规格登记和前端标题栏静态契约。以下行为需要平台测试：

- 主窗拖动时两个副窗同时跟随，且不会明显迟滞。
- 主窗在屏幕右缘时副窗翻到左侧。
- 主窗位于负坐标显示器时，副窗仍在同一显示器。
- 100%、125%、150% 缩放下初始位置与跟随位置不跳变。
- 调整日志/用量窗口大小后再次拖动主窗，副窗边缘仍与主窗边缘相接。
- 拖动主窗穿过其它应用窗口时，副窗 z-order 符合平台预期。
- 主窗收起、恢复、退出时，副窗不会阻止生命周期清理。
