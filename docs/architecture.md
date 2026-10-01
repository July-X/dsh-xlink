# dsh-xlink 架构

桌面壳的模块布局、数据流与数据目录约定。约定性约束（必须照做）见 [AGENTS.md](../AGENTS.md)。

多内核数据目录与扩展管理的设计稿见 [dsh-xlink 多内核数据目录与扩展管理设计](dsh-xlink-multi-kernel-design.md)，对应的实施顺序见 [开发计划](dsh-xlink-multi-kernel-development-plan.md)。这两份是**设计稿**（P0–P8 已落地），当前代码的实际布局以本文「多内核改造后的实际数据布局」一节为准。

## 模块

`src-tauri/src/` 自 2026-10-01 起**按功能分目录**（`shell/` `kernel/` `harness/` `plugins/` `skills/` `diagnostics/` `migration/` `pkg/` `node/` `notify/` `usage/`），只有 `lib.rs` / `main.rs` / `commands.rs` 留根。下面的条目按**不带目录的文件名**指代模块——目录归属与新增模块该放哪一格见 [src-tauri/AGENTS.md](../src-tauri/AGENTS.md#目录约定)。

```
ui/src（Vue 3 SPA）──invoke(Channel)──▶ commands.rs ──▶ kernel/ / plugins/ ──▶ pnpm/git 子进程
                                   │              │
                        shell/settings.rs    pkg/releases.rs（npm registry → GitHub 回退）
                                   │
              ~/.dsh-xlink/{shell/<mode>/, kernels/<family>/, plugins/<family>/, skills/}
              + ~/.dsh-xlink/<family>/desktop[-dev]/{kernels/, active.txt, patches/, quarantine.json}
```

- `commands.rs`：Tauri 命令层。长任务用 `spawn_blocking` + `tauri::ipc::Channel` 向 UI 推进度事件；`install_node`（「帮我安装」）下载官方 Node.js 到数据目录并与内核安装共用 lifecycle 锁，成功后清空 `node_cache` 让下一次 `get_status` 立刻报告新运行时；设置和日志目录读写也经 `spawn_blocking`，不占用主线程；窗口类命令（`open_harness`、`open_log_window`）在新 OS 线程上构建 webview（Windows 主线程创建会死锁）。日志「全屏」由 `open_log_window` 弹独立可缩放窗口（同一 SPA 加 `?log=<name>` 查询串，`ui/src/main.js` 分流到 `LogViewerWindow.vue`），ACL 走 `capabilities/log-viewer.json`（只放 `read_log_file`）。
- `ui/`：管理面板前端（Vue 3 + Element Plus，Vite 构建）。源码在 `ui/src/`（状态与动作集中在 `store.js` / `plugins.js` / `skills.js` / `progress.js` / `logs.js`，异步样板集中在 `async.js`：`singleFlight` 负责在途去重、`createStatusSource` 负责静默刷新、`createUpdateChecker` 负责 TTL + 逐包失败退避 + 互斥 + 提示这套更新检查策略，插件页与技能页只剩参数差异；与 Rust 只经 `bridge.js` 的 invoke/Channel 通信）；`vite build` 产物 `ui/dist/` 是 `tauri.conf.json` 的 `frontendDist`，`tauri dev` 走 `devUrl`（vite dev server，5174；端口由 `scripts/dev.mjs` 统一，三处数字由 `scripts/dev-port.test.mjs` 钉死）热更新。面板分工：设置页的「设置」卡只保留 Web UI 端口输入 + 「保存设置」（插件接线 profile 名是固定值，仍随端口一起提交）；概览页「当前内核」卡的标题旁用 ℹ️ tooltip 悬浮自动检测的 Node 环境结论（读 `StatusView.node`），并承接原设置页的「检测 Node.js」——按钮改名「重新检测」挂在同卡的 Node.js 行，探测结果就地覆盖该行显示与 tooltip 结论（Rust 侧的 `node_cache` 不因 `detect_node` 失效，离开概览页再回来即回到状态值），概览页因此不出现任何写操作；同卡下方的「套餐用量」卡展示云端套餐 / 余额（见 `subscription.rs` 条目）。`open_official_chat` 在独立线程里 `WebviewWindowBuilder::new(...).label("official-chat").title("DeepSeek 官方对话")`，固定 `OFFICIAL_CHAT_URL`（`https://chat.deepseek.com`），默认只创建 DeepSeek 内容子 webview，千问/MiniMax 在首次选择时惰性创建并在窗口生命周期内保留状态，从而降低首开 CPU、内存和网络开销；构造时不再覆盖 user-agent——WebView2 引擎本身就是真实的桌面版 Edge，原生 UA、`Sec-CH-UA` 客户端提示与 `navigator.userAgentData` 天然一致；此前把 UA 改写成 Chrome 反而制造了「HTTP 层报 Edge、JS 层报 Chrome」的矛盾，正是环境检测的特征。不启用无痕模式——Windows 使用专属目录、macOS 使用稳定的数据存储标识，均为持久化配置档案；DeepSeek 登录态跨外壳重启保留，与面板/工作台窗口隔离；再调 `.additional_browser_args(OFFICIAL_CHAT_BROWSER_ARGS)` 抑制 Chromium 自报的 `navigator.webdriver = true`，同时重述 wry 默认禁用的 `msWebOOUI` / `msPdfOOUI` / `msSmartScreenProtection`——传入 browser args 会整体替换 wry 默认值，漏掉这三项 WebView2 就会重新弹出 SmartScreen 安全提醒与 Edge 专属 UI；该参数仅 WebView2 后端消费，macOS / Linux 构建忽略——且 WebView2 要求同一 user-data 目录上的环境参数完全一致，面板与工作台已在默认目录用默认参数建好环境，所以 Windows 下此窗口经 `.data_directory` 固定到专属目录 `<data_dir>/webview-official-chat`，避免环境参数冲突；macOS 下不使用该目录字段，改用稳定的 `.data_store_identifier` 保存 WebKit 登录数据；之后按 `titlebar-pulse.js` → `chat-fingerprint.js` 的顺序注入内容子 webview 各自的两个 `initialization_script`、给本地 `official-chat-strip` 页签栏 webview 注入 `pullstring-launcher.js`——拉绳属于整个官方对话窗口的 chrome，由页签栏 webview 承载，内容子 webview 不再下沉；`pullstring-launcher.js` 因此不需要 `chat-fingerprint.js` 抢跑 `window.__TAURI__` 的闭包引用，初始化顺序出错也不会影响拉绳挂件。复用既有窗口（`get_webview_window` + `set_focus`），不设 `closable(false)`——第三方 origin 的窗口不持有内核会话，OS 关闭按钮应保持有效。`open_official_chat` 是 `async` 命令，builder 结果通过 `std::sync::mpsc::channel` 回传、由 `tauri::async_runtime::spawn_blocking` 接收，命令只在线程里 `Result<WebviewWindow, _>` 真正落地之后才 `Ok(())`。配套的 `close_official_chat` 在窗口未注册时返回错误，存在的窗口走 `destroy()`；面板按钮在 `StatusView.official_chat_open` 的下一次 2.5s 轮询里把按钮文案从「打开官方对话」翻为「关闭官方对话」。
- `kernel.rs`：内核安装、active 指针、启动 / 停止、端口探测；`get_status` 启动及状态刷新时扫描 `kernels/<版本>/` 并返回本地已安装版本，供版本面板的切换列表使用；详见下文「内核生命周期」。
- `notify.rs`：任务完成通知。以**内核的另一个客户端**身份订阅内核自己的 WebSocket 事件流（`ws://127.0.0.1:<port>/api/remote.mux` 上的 `$events` 逻辑流）：先用当天日志里的 launch token 走一次 `GET /?token=…` 换 `dsh-auth-*` cookie，再开流，判据是 `api-session/status` 的第二个参数由真变假（内核把 `agent/status` 直接映射成该事件）。**刻意不轮询 `session/list`**：实测一次约 2.1 MB / 0.5 s 内核 CPU（该接口要为全部会话重建投影），秒级轮询等于常驻吃掉一个核。未读语义：任务完成时工作台窗口不在前台（或没开）才计数并弹气泡，窗口重新获得焦点即清零；子代理会话（`api-session/added` 摘要里带 `parentSessionId` 或 `origin == "subagent"`）只记账不打扰。标题优先取 `api-session/added`，连接前就存在的会话由**每个内核进程最多一次**的 `session/list` 快照补齐。角标：macOS 走 `Window::set_badge_label`（`NSDockTile.setBadgeLabel`，系统原生绘制），Windows 走 `Window::set_overlay_icon`（`ITaskbarList3::SetOverlayIcon`，Win32 没有数字角标 API），数字由 `render_badge` 用 3×5 点阵 + 4×4 超采样画进 32×32 透明画布的右上角，无图像/字体依赖，可单测。订阅线程随内核启停（`start_kernel` 幂等启动、`stop_kernel` / `RunEvent::Exit` 停止），断线 1→20 s 退避重连并在每次重连时重新认证（内核重启会换 token）。设计、协议与实测数据见 [notification-design.md](notification-design.md)。
- `plugins.rs`：社区插件的中央库、内核物化、profile 接线、更新检查、社区目录；实现规则见 [plugin-internals.md](plugin-internals.md)，设计层见 [plugin-management.md](plugin-management.md)。
- `patches.rs`：随发布包内置的内核补丁 / 小插件（`bundle.resources` 进入 app 资源目录，运行时解析 `<resource_dir>/patches/` 与 `<resource_dir>/resources/patches/` 两个候选）。清单校验（路径约束在内核目录内、copy/replace 两种文件模式）、应用/撤销（备份到 `<data_dir>/patches/backups/`、SHA-256 内容校验兜底）、状态持久化（`<data_dir>/patches/state.json`，按「补丁 × 内核版本」）；`patch_apply` / `patch_revert` 与内核启停共用 lifecycle 锁，工作台运行期间拒绝操作；声明 `supersededSinceKernelVersion` 的补丁在当前内核版本 `>=` 该值时被 Rust 端 `is_superseded` 标为 `row.superseded=true`、`enabled=false`，`apply` 直接以「已被官方取代」拒绝，UI 把卡片折叠为「已并入官方内核」（`SettingsPanel.vue` 用 `obsoleteExpanded` Set 维护展开态），已应用到旧内核的记录仍可正常撤销；`dsh-file-perf` 已在 0.1.2-alpha.2 起标为 superseded（官方直接采纳）。设计见 [patch-management.md](patch-management.md)。
- `releases.rs`：npm registry 全量版本 + dist-tags；registry 不可达时回退 GitHub Releases API 与 Atom feed。
- `pkg.rs`：插件与技能共用的**包取源层**——npm registry 文档与 dist-tag 解析、tarball 下载 / 解包 / 摘要校验（`releases::verify_download_integrity`：优先 SRI 的 sha512/sha256，回退老 packument 的 `shasum` sha1，两条都缺则拒绝安装）、`git ls-remote --tags` 的 semver tag 判定、版本形态与新旧比较、安装 spec 拆分、中央库暂存目录（`.tmp-`/`.new-`/`.backup-` 三段式换名）与 `.dsh-source.json` 来源标记。**只返回纯文本原因**，错误分类由调用方决定（插件侧 `AppError::Plugin`、技能侧 `AppError::Skill`），因此同一份取源行为只有一处实现：`plugins.rs` 与 `skills.rs` 里各留一行包装。历史教训是这层重复会漂移——`is_newer_than` 与 `split_npm_spec` 都曾各自偏离，只在注释里互指「与插件中央库一致」。
- `state.rs`：外壳 JSON 状态文档的读写骨架（`load_lossy` / `load_checked` / `integrity_warning` / `save` / `save_best_effort`），服务插件与技能清单、补丁记录、隔离记录。核心约定是「文件不存在」与「损坏」必须分开：损坏退化成空文档会在下一次写入时覆盖用户真实记录（插件清单变空 → 内核物化目录被当孤儿清掉；技能清单变空 → 已装技能的链接被逐个删除；补丁记录变空 → 打过补丁的内核变成「没打过」，既撤不掉也重装不了）。文案与错误分类由调用方通过 `StateCtx` 提供，所以共享层不替模块编用户文案。
- `usage.rs`：模型用量统计。数据源是实例 DSH home 里的内核会话文件 `sessions/<工作区>/session-*/session[.vN].jsonl.zstd`——zstd **多帧**流，内核按帧追加；`assistant/message` 记录自带 `source`（真实模型调用含 `provider` / `model`）与 `usage`（input / output / cacheRead / cacheWrite）。扫描是**按文件增量**的：账目按「已消费压缩字节 offset」推进（ruzstd 逐帧解码，精确记帧边界），坏帧 / 半帧处停在该帧起点下一轮续扫；同一会话目录多代格式（v1/v2/v3 并存于内核迁移后）只认最高版本一份，避免同一段用量数成几倍；mtime 早于窗口的文件不解码直接把 offset 记到末尾。每个文件一份**「本地日历日 × 模型」增量账**（`<instance_dir>/usage/state.json`，`state.rs` 容错读 + 原子写），汇总视图读取时求和派生——会话删除 / 旧格式被取代时对应账目整条移除，汇总自动回落，无需反扣逻辑；每次保存剪掉 90 天窗口外的日账，体积有硬上界（约百 KB）。扫描跑在 `spawn_blocking` 内、`SCAN_LOCK` 串行化，规划 → 并行解码（`thread::scope`，至多 8 线程）→ 合账三段式：首次全量秒级、常规增量毫秒级。`get_model_usage` 带 45s 新鲜度窗口（`force` 越过，窗口刷新用）；**「今日」只有一套口径**：后端按本地日历日精确匹配出 `today_tokens` / `today_requests`（同一次匹配，两个字段无从分叉），前端 `usage.js::todayUsage` 是唯一入口，概览卡片与独立窗口都走它——日序列末尾可能是晚于今天的异常日期（时钟漂移 / 手改 session），所以「取最后一天」不是今日（`612ecde` 在后端换掉了这个算法，窗口侧当时漏改，2026-09-30 补齐）；概览卡片挂载期间 60s 对一次账、卸载即停，窗口重新可见 / 切回概览时经 `App.vue` 的 `refreshActivePanelData` 立即对账——它此前只在 `onMounted` 拉一次就再也不更新，与「每次打开都 force 重扫」的窗口必然显示两个数（实测卡片 0 tokens、窗口 12.09M）。UI 是概览「当前内核」卡的「今日用量」行 + 「模型用量」按钮，点击经 `open_usage_window` 弹出**独立可缩放窗口**（建窗与吸附细节见上文「窗口」章节的 `usage-viewer` 条目）——摘要卡 / 热力图 / 堆叠趋势 / 环形图 + 列表，纯 CSS 与内联 SVG，不引图表库，B / M / K 标准单位，90 天保留策略经 tooltip 告知。账目按实例存储；面板 v1 读取默认实例的账目，与 `get_status` 的单实例兼容口径一致。
- `subscription.rs` + `credentials.rs`：云端套餐用量（MiniMax Token Plan 双窗口进度 + DeepSeek 按量余额 + 智谱 GLM 编程套餐双窗口进度），设计稿见 [subscription-usage-design.md](subscription-usage-design.md)。**凭据复用当前内核模型设置**：外壳不收集、不存储 Key，`credentials.rs` 按 DSH 的同一语义只读解析当前实例 / profile 的凭据——profile（`<DSH_HOME>/profiles/<name>/cordis.patch.yml`）里 provider 的 `apiKeyEnv` 引用优先，没有显式声明才按内核默认规则派生引用名（route `minimax-cn` → `MINIMAX_CN_API_KEY`，`zai-coding-cn` → `ZAI_CODING_CN_API_KEY`）；引用值按「环境变量 → `.credentials.yaml` refs → `.env`」优先级解析。**未配置对应厂商凭据的 provider 在视图里 `configured = false`，前端自动隐藏其分区**。智谱额度接口（`bigmodel.cn/api/monitor/usage/quota/limit`，未文档化 Web 端点）除 raw API key 鉴权（不带 Bearer）外还要求组织 / 项目上下文头：`ZAI_CODING_CN_ORGANIZATION` / `ZAI_CODING_CN_PROJECT`（可选 `ZAI_CODING_CN_PLAN_TYPE`，个人 = 1 / 团队 = 2）与 Key 走同一条凭据链解析，未配置时按确定性失败（不标 expired，照常 TTL 刷新）透出带获取方法（DevTools Network 面板）的错误文案；其 `percentage` 是已用口径，解析时统一换算为剩余百分比后复用 MiniMax 的 tier 展示路径。原始凭据只存在于凭据读取与 HTTPS 请求头两处，不进 UI、日志、缓存或事件。数据源是各第一方但未文档化 / 轻文档化的 HTTPS 接口（ureq 3 阻塞式 + `spawn_blocking`，15s 超时），解析逐字段防御式——字段缺失或类型不合就跳过该字段，不做整体失败。缓存文档 `<实例目录>/subscription-cache.json`（`state.rs` 容错读 + 原子写，`CACHE_SCHEMA = 2`）绑定实例 / profile / 凭据指纹（SHA-256 前 16 hex，不可逆）：切实例 / 换 profile / 换凭据后旧条目作废，绝不把上一个账号的数据当当前账号展示。错误通道二分：瞬时失败（网络 / 超时）缓存不写不删，前端 keep-last-good 继续展示旧值 + 错误横幅；确定性失败（HTTP 401/403 → `credential_status: expired`、业务错误码、结构不认识）保留旧数据只更新状态与错误，且 **expired 条目不参与自动刷新**，仅 force（点刷新 / 测试连接）重试；结构不认识时把截断响应摘要写入现有 Shell 日志（`<kind>-subscription-<日期>.log`，不含凭据）。每个 provider 在视图里自带状态（多 provider 查询允许部分成功）；查询串行在 `FETCH_LOCK` 内；视图携带当前实例 / profile 供窗口展示。UI 是概览卡的「套餐用量」行（收起态一行摘要，展开态进度条 + 重置倒计时 + 余额行，进度条按剩余百分比三档配色）+ 设置页凭据状态卡（只有「测试连接」与「前往模型设置」，不复刻凭据编辑表单）+ 独立窗口（见下文「窗口」章节的 `subscription-viewer` 条目）。
- `node.rs`：Node 检测（解析顺序为 显式配置 → 托管运行时 → PATH → nvm 管理的 Node：macOS/Linux `$NVM_DIR/versions/node/<v>/bin/node` 跟随 `alias/default` 链，Windows `%NVM_SYMLINK%` 与 `%NVM_HOME%/v*/node.exe` → 常见系统位置）、engines 校验（`^22.19 || >=24`）、pnpm/npm 解析（显式配置 → node 同目录 → PATH）；空结果文案按「完全没有 Node」与「Node 版本太老」分别给出可操作的多路径（nvm/fnm/volta、brew/winget/apt、官方安装包），并优先推荐「帮我安装」。
- `node_install.rs`：托管 Node.js 运行时（`<data_dir>/tools/node/v<version>/`）——用户确认后从官方 nodejs.org/dist 下载固定版本（v24 LTS）产物与 SHASUMS256.txt，SHA-256 校验后按 archive.rs 同款路径约束最小解包（只保留 node 可执行文件、LICENSE 与 npm 树，跳过符号链接并以 shim 重建 npm 入口），发布前写入临时目录、成功后清理旧版本目录；进度落盘 `<logs_dir>/<kind>-node-install-<date>.log`。因为安装在数据目录而非 app 资源目录，`npm install -g pnpm` 之类写入无需重定向。
- `settings.rs`：`settings.json` 平铺结构（`node_path` / `pnpm_path` / `port` / 通知三开关），serde default 兼容缺字段，**不含任何模型凭据**——订阅查询凭据由 `credentials.rs` 从实例 DSH home 只读解析。通知开关是 `Option<bool>`：面板的「保存设置」只提交端口与 profile，`bool` 会让每次保存都把它们静默重置（与 `node_path` 同类问题，见 `commands::merge_settings`）。
- `process.rs`：所有 GUI 子进程的 `quiet()`（CREATE_NO_WINDOW）+ `command_with_path()`（一次性 sibling，盖上 `env::merged_path()`）出口；一次性命令并行 drain stdout/stderr，4 MiB 输出上限、30 秒 deadline，Unix 进程组超时回收；长期 pnpm/npm 任务使用日级轮转日志（详见「日志规范」）、64 KiB 行上限和 30 分钟总 deadline，静默时发 heartbeat，超时回收整个进程组；每行写完立即 flush（实时落盘），避免 SIGKILL 丢失最近 64 KiB 缓冲。

### 并发与持久化

- `AppState.lifecycle` 是内核安装、启动、停止、切换和删除共用的生命周期锁。锁只在 `spawn_blocking` worker 内获取，保证长操作不会跨 `await` 持有标准 MutexGuard；安装流程在同一个 worker 内完成工具链解析、内核安装和首次激活，安装完成后保持内核停止状态，启动由单独的 `start_kernel` 命令负责。
- `open_harness` 在独立窗口构建线程中通过 `mpsc` 回传真实的 `WebviewWindowBuilder::build()` 结果，命令只有收到成功结果才向 UI 返回成功；构建失败会保留可操作的错误信息。
- `plugins.rs` 与 `skills.rs` 各自维护进程内商店写锁。安装、更新、卸载、模式/启停变更、全量同步和启动修复在写清单时串行化；更新检查只在读取快照和最终提交时加锁，网络请求在锁外执行，并以 `installed_version` 防止旧结果覆盖新安装。
- `process::atomic_write` 把 `store.json`、设置文件和商店 `.npmrc` 先写入同目录临时文件并 `sync_all`，再替换正式文件。这样进程中断时读者只会看到旧的完整文件或新的完整文件，不会读到截断 JSON。

## 日志规范

Shell 日志目录 `~/.dsh-xlink/shell/<release|dev>/logs/`（`paths::shell_logs_dir(mode)`）下每个 `.log` 文件都按统一的命名规范落盘，便于 `list_log_files` 列表稳定、用户/支持方一眼区分 build 类型与日期。注意这个目录**不在 data_dir 里**——`kernel::logs_dir()` 刻意忽略形参直接返回 `shell_logs_dir()`，因为日志要跨内核族共享；旧版 kernel data dir 下的 `logs/` 仅作为只读兼容路径供 P6 迁移向导扫描历史日志。

- **文件名格式**：`<kind>-<name>-<YYYY-MM-DD>.log`，其中：
  - `kind` = `release`（release build）或 `dev`（`tauri dev`），与 Shell 日志目录 `shell/<release|dev>/` 及 data_dir 的 `desktop/` / `desktop-dev/` 分槽保持一致。
  - `name` = 逻辑名（`kernel` / `install-<version>` / `plugin-<id>` / `plugin-wiring` / `pnpm-install-<epoch>`）。
  - `YYYY-MM-DD` = 本地日期（用户时区，非 UTC），按 `time` crate 的 `local-offset` 计算。
- **轮转策略**：日级为主、尺寸为辅。`RotatingLog` 在每次 `write_line` 前检查本地日期与当前打开文件日期是否一致，不一致就关闭旧文件、打开新一天的文件。同一日内单文件跨 `KERNEL_LOG_MAX_BYTES`（8 MiB）则触发尺寸轮转：旧文件改名为 `<...>.1`，保留至多 `KERNEL_LOG_BACKUPS`（2）个备份。
- **实时落盘**：`write_line` 每写一行立刻 `flush()`，不再使用 64 KiB `BufWriter` 批量冲刷——SIGKILL / 内核 panic 后用户仍能在 `read_log_file` 面板里读到崩溃前最后一行。
- **install 日志清理**：`rotate_install_logs` 仍以「保留最近 9 份」为上限，新旧两种命名（`install-<version>.log` 旧名 + `<kind>-install-<version>-<date>.log` 新名）都纳入统计；`pnpm-install-*` 自动安装日志不在清理范围（每次 `npm install -g pnpm` 自带唯一 epoch 戳）。
- **例外（fixed-path 模式）**：插件构建日志位于 `<plugin_dir>/.dsh-build.log`（插件卸载时一并清理），不属于上述规范——`run_pnpm_at` / `RotatingLog::new_at_path` 仍保证实时落盘与尺寸上限，但不应用 build kind / 日期前缀。

`list_log_files` 仅按 `.log` 后缀扫描目录、按文件名字典序倒序排；最新的 `<kind>-kernel-<today>.log` 自动落到第一个 tab，符合用户「最近一次启动最相关」的预期。`read_log_file` 接受任意裸文件名（不含路径分隔符），所以新旧命名都通过同一组 Tauri 命令入口暴露给 UI。

## 桌面端自身更新

以下两条不属于日志规范，单列一节：

- `updater.rs`：`tauri-plugin-updater` 包装，启动 3 秒后后台检查并 emit `shell-update-available`。安装前先下载并校验签名，再写入 `pending-shell-update.json`，随后拉起安装器并重启。Windows 新版本管理面板完成首次状态刷新后，通过 `confirm_shell_ready` 清理 `/UPDATE` 路径跳过的旧安装和 updater 临时目录；同一安装目录不调用旧卸载器，而是直接移除历史 exe 和默认快捷方式，清理失败则保留标记等待下次启动重试。
- **出网路由：先系统代理，失败再直连**（`net_proxy.rs`，2026-09-30）。壳访问 GitHub 只有这一条路径（检查更新 + 下载更新），而它此前**完全不看系统代理**：tauri-plugin-updater 用的 reqwest 只认 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 环境变量，读 Windows 注册表 / macOS 系统网络设置的那条路（hyper-util 的 `client-proxy-system`）没开——它会带进 `windows-registry` 与 `system-configuration` 两个 crate。壳是 GUI 程序、从资源管理器启动，继承不到用户为命令行设的变量，于是「系统里明明开着代理，更新检查却直连 GitHub 然后超时」（用户实测：`error sending request for url (https://github.com/July-X/dsh-xlink/releases/latest/download/latest.json)`）。`net_proxy::routes()` 按「环境变量 → 平台系统设置」探测，返回**有序**的路由表：探测到代理是 `[代理, 直连]`，没有是 `[直连]`，末位恒为直连；`updater::run_routes` 按序试，第一条走通就返回。
  - 只回退**传输层**失败（`Reqwest` / `Network`）：签名校验、清单解析、版本号解析、URL 无效换一条路只会同样地失败，重试一遍还会让「第二次也这样」盖住第一次的真正原因。`ReleaseNotFound` 也算可回退——企业网关对任何地址回 404 是常见行为，把它当成「没有更新」就会静默漏掉一次更新。
  - 直连路由用 `no_proxy()` **显式**关掉代理（含环境变量里的那个）：回退到直连却仍被 `HTTPS_PROXY` 拉回代理，等于把同一条路试两遍。
  - 每次回退与最终失败都落 `shell-update-route` 事件日志（自动出现在「查看日志」面板）；失败文案点名试过的每一条路——用户看到的是「检查更新失败」一个现象，而下一步（起代理软件 / 关系统代理）取决于走的到底是哪条路。
  - 平台读法：Windows 读 `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 的 `ProxyEnable` + `ProxyServer`（两者必须**同一个 hive 成对读**，拿 HKCU 的开关配 HKLM 的地址会得到用户从没配过的组合），`ProxyServer` 的两种写法（`host:port` 与 `http=…;https=…;socks=…`）都认，`socks=` 跳过（reqwest 没开 socks 特性）；macOS 解析 `scutil --proxy` 的字典转储。两份解析都是纯函数，在任何平台直接测。
  - **不归这一层管**：WebView2 / WKWebView 本身就跟随系统代理，pnpm / npm 读的是 npm 自己的 proxy 配置。再来第二条 Rust 出网路径时应当复用 `net_proxy::routes()`，不要复制一份探测逻辑。
- `lib.rs`：装配 + `setup()` 取目录（必须走 `kernel::data_dir`）+ `RunEvent::Exit` 兜底回收内核进程组。`harness` 与 `official-chat` 两个 webview 窗口通过 `capabilities/harness-remote.json` / `capabilities/official-chat-remote.json` 分别绑定 ACL；拉绳挂件只需要 `allow-focus-main-shell` 这条 IPC 命令，URL 都精确钉死（`http://127.0.0.1:*` / `https://chat.deepseek.com/*` 等三个官方对话 origin，不开通 wildcard 域名）。`harness-remote.json` 直接授 `allow-focus-main-shell`，`official-chat-remote.json` 不授任何命令（拉绳属于窗口 chrome，由 `official-chat-strip` 页签栏 webview 承载、走 `allow-official-chat-tabs` 这条本地权限）。

## 内核生命周期

- 安装：在 `<data_dir>/kernels/<version>/` 写最小 stub `package.json` 与 `pnpm-workspace.yaml`（`packages: ["."]` 把内核目录锚定为 workspace 根，防上层 workspace 泄漏）后执行 `pnpm add --prefix … --config.node-linker=hoisted --reporter=append-only @deepseek-ai/dsh@<version>`。安装完成后扫描官方子包版本错位：内核 monorepo 锁步发布（所有 `@deepseek-ai/dsh*` 子包同版本），主包却用 `^` 范围声明依赖，pnpm 会浮动到范围内最新——装 alpha.1 时 alpha.2 已存在就会装出「主包 alpha.1 + 依赖 alpha.2」的混装树，启动即报 `does not provide an export named '…'`。壳的对策是把扫描出的错位子包（含传递依赖）以 `pnpm.overrides` 钉到内核精确版本（pnpm ≥10 只认 `pnpm-workspace.yaml`，旧版只认 stub 的 `pnpm` 字段，两处都写）再重装一遍并复扫，仍错位则判安装失败。npm tarball 先写 `.part` 临时文件，完整下载后才 rename。首次安装成功后只设置 `active.txt`，不会自动启动内核；启动由用户在概览页明确触发 `start_kernel`。
- **上游漏发精确钉版时的降级兜底**（`kernel_deps.rs`，2026-09-29）：锁步发布的前提是每个子包都发了，现实里会漏。实测内核 `0.2.0-rc.2` 的传递依赖 `dsh-web-app@0.2.0-rc.2` 精确钉住 `@deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2`，而后者在官方 registry 与 npmmirror 上**都不存在**（两边最新都停在 `0.2.0-rc.1`），pnpm 在解析阶段就以 `ERR_PNPM_NO_MATCHING_VERSION` 拒绝整棵依赖树，整个内核装不上。**换 registry 救不了**——这是发布事故而不是镜像延迟。壳的处理是：安装失败后从 pnpm 输出里解析出断边，查当前 registry 的已发布版本，给每条断边选一个**同一 `major.minor` 版本线内、语义化更低**的版本钉进 `overrides` 再重跑（最多 3 轮，覆盖「上游漏发多个子包」）。预检放在失败之后而不是安装之前，正常安装不多付一次网络往返。降级只对 `@deepseek-ai/` 命名空间生效；跨版本线一律不降级，宁可失败并明确告诉用户「上游发布不完整，换个内核版本」——壳无权替用户决定跨 minor 的兼容性。**稳定性层级同样不跨**：语义化里 `0.2.0-rc.2 < 0.2.0` 成立，只判「严格更低」会把正式版的需求悄悄换成 RC，而提示文案不会说这是 RC，用户拿到的内核就不是他以为的那个（`is_pre_release`）。降级是**明说**的：每条都进进度面板，并汇总进安装成功提示。
- 依赖钉版的三件事（写 stub / workspace yaml 的 overrides、装完扫锁步错位、漏发钉版的降级兜底）都在 `kernel_deps.rs`：`kernel.rs` 只负责起 pnpm 进程并按「退出码 × 产物是否就位」判成败。降级钉版必须**合并**进后续轮次的 overrides——锁步重装那一轮若覆盖写，包会被重新钉回那个不存在的版本，pnpm 立刻二次失败。
- npm 包解包由 `archive.rs` 在 Rust 内执行：只接受 `package/` 根、拒绝绝对/父级路径、符号链接/硬链接/特殊文件，最多 100,000 个条目、512 MiB 声明展开内容，并在临时目录校验后发布到目标目录。
- `node-linker=hoisted` 保证 `node_modules` 扁平，内核入口固定为 `node_modules/@deepseek-ai/dsh/lib/bin.js`（`kernel::KERNEL_BIN_REL`）；改布局必须同步该常量与 `start()`。
- `run_pnpm` 把 stdout/stderr 各用一个 drain 线程读入有界 mpsc channel，安装线程逐行回调 `on_progress` 并落盘日志——不要把两个管道放在同一线程顺序读取（会因管道缓冲区满而死锁）。
- **装包任务降优先级**（`child_priority.rs`，2026-09-29）：`run_with_progress` spawn 出子进程后，若可执行文件名是 `pnpm` / `npm` / `npx` 就设成 `BELOW_NORMAL`。起因是一次实测：dev 壳安装内核的 8 秒（19:09:08→19:09:16）里，**release 壳的工作台 webview 被重载**并撞上内核的启动顺序竞态（`renderSlot('root') before any 'root' registration`，记在 `dsh/desktop/last-incident.json` 的 `at=19:09:11`）。**不是两棵安装树互相污染**——同一时刻 release 内核一行输出都没有（最后一条停在 18:46:42，当天它启动过 12 次，每次都打一行 `dsh web: http://…`），而且 `dsh/desktop/` 与 `dsh/desktop-dev/` 物理不相交、壳代码里没有写另一个壳目录的路径。被打中的是工作台页面（真正的机制 2026-09-30 定案为 pnpm store 的 inode 侧信道，见下一条；当时归因为资源争用）。降优先级让 pnpm 在与内核 / webview 争 CPU 时让路，这一半仍是对的。**只降装包工具**：Node 探针（`smoke_load_native_modules`）保持正常优先级，它降了会误报「原生模块加载失败」，那是更糟的假阴性。
- **跨壳判据只提示，不阻断**（`kernel::warn_other_shell_workbench` → `instance::workbench_running_in_other_shell`，2026-09-30）：上一条挡不住第二次实测——dev 壳装 `0.2.0-rc.1` 的 10 秒（10:32:42→10:32:52）里 release 壳的工作台在 10:32:47 重载、10:32:48 抛 `scope 'session-maybe' rendered without an installed adapter` 并从此黑屏。上午的处置是**硬拦**（`ensure_workbench_stopped`，另一个壳的工作台在跑就拒绝装 / 删），下午改成**提示**：装 / 删照做，只把后果与出路说清楚（对面可能卡住或黑屏，点「刷新工作台」能回来）。
  - **机制（2026-09-30 定案）：pnpm store 的 inode 侧信道，不是资源争用**。pnpm 的内容寻址 store 按文件内容去重——两个壳的树与 store 里内容相同的文件是**同一个 inode**；NTFS 上硬链接数增减会更新 ChangeTime（mtime 不动），而内核 `dsh-client-hmr` 每 500ms stat 轮询每个客户端 bundle（`sameBundleStat` 比较 mtime/ctime/size），把这种噪声当成「bundle 重建」推 SSE `rebuilt` 帧给**活页面**，页面在换模块的窗口里撞槽位不变量死掉。当天五次装 / 删全部 4~6 秒内命中（触发安装只跑 9.2s、全部从 store 复用、BELOW_NORMAL 优先级），看门狗重载计数为 0、内核日志一行都没有——「资源争用」「看门狗误重载」「事件风暴惊动监视器」三个旧解释全部不成立（此前的 chokidar 审计没错，但漏了这**不是 watcher 而是 stat 轮询**，恰是 `dsh-client-hmr` 自己的 invariant 注释里写明的那一个）。
  - **为什么只提示**：根治手段（下一条 ⓪）落地后，**装**的路径已隔离；**删旧版硬链接树**仍可能惊动对面一次且自愈会兜住——为一次可自愈的惊动硬拦，代价是废掉双壳并行。**分家与这条提示不矛盾**：分家分的是**路径**（树、注册表、插件中央库、端口、实例 id），此前没分到的是 **pnpm store 的 inode**——⓪ 补的就是这一层。
  - **本壳守卫（`ensure_own_shell_stopped`）三条动作都保留**：装 / 删改的是自己脚下那棵树，运行中的内核就在里面。「切换版本连跨壳都不看」是另一层理由——它写的是本壳树里的 `active.txt` 一个文件，两棵安装树物理不相交，对方够不着。
  - `warn_other_shell_workbench` 的**返回类型刻意是 `()`**：`Result` / `Option` 都能被调用方接成一次拒绝，而那条路等于把双壳并行关掉。`scripts/check-invariants.mjs` 第 12 项把「三条动作只过本壳守卫 / 旧的跨壳硬拦入口不得复活 / 跨壳调用点恰好 2 处 / 跨壳提示返回类型必须是无」钉成机械检查——最后一条是唯一挡得住「把提示接成阻断」的判据，**只按函数名查不够**：第一版检查一路绿着，而那正是它挡不住的改法。
- **「对面装包惊动本壳工作台」分四层处理**（2026-09-30 下午定稿；完整证据链、三个被证伪的理论与排查工具箱见 [case-2026-09-30-inode-side-channel-blackscreen.md](case-2026-09-30-inode-side-channel-blackscreen.md)）。原始事故里用户丢的不只是页面，是**正在输入的那段话**（用户原话：「会导致 release 版的会话被中断，用户正在输入的东西丢失」）。四层各管一段，**只有第⓪层能让事故不发生**，后三层是兜底：
  - **⓪ 根治：内核安装用 `--config.package-import-method=copy`**。树持有全新 inode，装 / 删从此物理上碰不到对面的任何文件——连用户自己的 pnpm 项目也不再能惊动正在服务的工作台。代价：每个版本真实占盘约 450 MB（不再与 store 硬链接共享）、安装慢几秒；这是「已安装的内核是一棵独立的、不可变的树」这一语义的正确实现。**存量硬链接树不迁移**：卸载旧版树仍会惊动对面一次（自愈兜底），把该版本卸载后重装一次即彻底隔离。机械检查钉住这个参数不得被删。
  - **① 恢复动作等风停**（`package_activity.rs` + `recovery_backoff()`）。装 / 删的两端打一个跨壳信标（`~/.dsh-xlink/package-activity.json`，自过期 + 带写入方 pid + 原子写，是 AGENTS.md 那张表之外唯一一处有意的跨壳可变数据）。页面自愈刷新前经 `harness_reload_backoff` 命令问它（≤5s 一轮、上限 30 轮、IPC 失败按老行为 3s 照刷）；壳自动重建走 `recreate_when_quiet`（后台线程等风停、上限 240s、落地前重核内核在服务、单等待者）——2026-09-30 实测：落在风暴中的自愈刷新与自动重建都在几秒内又死一次，**落点比次数重要**。看门狗阈值放宽（`clamp(15s, 120s)`，只放宽不收紧、有硬顶）保留为防御。
  - **② 输入不丢**（`harness_draft.rs` + `harness_media.rs` + 注入脚本 `harness-draft.js`）。重载 / 重建前把输入框里**还没发出去的东西**交给壳落盘，页面起来后写回。四个非显然点：① 必须**经壳**——`recreate` 会换掉整个 webview，sessionStorage 直接没了；② 输入框是 **Lexical 富文本编辑器**不是 textarea，读用 `innerText`、写用 `execCommand('insertText')` + 读回来核对；③ 恢复必须**先在页面上找到能写的地方，再去壳里取**，因为 `take` 是读走即删的（否则草稿会在下次正常打开时凭空冒出来）；④ **存与作废是同一件事的两面**——发出去的那一刻盘上那一份就必须消失，否则下次打开工作台时那句已送达的话会自己坐回输入框。记草稿的时机是**停止输入 600ms 之后**而不是页面卸载时。
    - **作废怎么判**：发送时 Lexical 是**程序化**清空编辑器的，**不派发 `input`**，所以「空了」光靠监听收不到——页面每秒看一眼（只在内容真的变了时才动作，用户还在打字时让位给 600ms 停顿判据），本页存过且现在空了就走 `clear_harness_draft`；数据侧另一道：`stash` 收到「既没字也没图」即删除。**判据刻意用「可编辑元素里的字没了」而不是「点发送 / 按 Enter」**——内核换一版就可能换掉 class 名与文案。清盘有两个刻意的不作为：页面上一个可见的可编辑元素都没有时**不清**（黑屏那一刻很可能正是 composer 消失的时候，这时的「空」不是「发出去了」），本页没存过东西时**不清**（恢复失败放回盘上的那一份是用户唯一的一份）。
    - **图也一起保管**（字节与上限在 `harness_media.rs`）。图在核心里不是编辑器的一部分，而是 composer 卡片上那排 `blob:` 缩略图，**字节只活在那一个页面里**（`URL.createObjectURL`），页面上没有任何服务端副本，所以只能趁页面还活着时读出来交给壳。采集认 `[data-composer-card]` + `img[src^="blob:"]`（内核自渲染的语义钩子；会话里的图走 http(s) 媒体地址），**恢复走 paste**——2026-09-30 实测：往文件选择框塞 `DataTransfer.files` 再派发 `change`，`input.files` 确实被填上了但内核一张不收（React 的 value tracker 吃掉合成 change），往编辑器派发带 `clipboardData` 的 `paste` 则一次就中，且走的正是用户自己粘贴截图的同一条路（Lexical `PASTE_COMMAND` → `intakeFiles` → `createDrafts`）。注入后要**数一遍轨道上的图有没有变多**：没变多就是假恢复，按「文字成没成」分别处置——都没成就放回整份（输入框还空着，是干净的重试），**文字已回填就清盘**（再放回只会让下一次空的输入框收到重复文字）。上限 8 张 / 单张 4MB / 合计 12MB（比内核紧，因为它还要以 base64 过一次 IPC），存不下的**如实报出张数**并落进「查看日志」。**非图片附件不保管**：内核对它们走立即上传，页面上只剩一张文件卡片、拿不到字节，拿不到就不假装。
  - **③ 黑屏自己好，且别把额度一把梭**（`harness_window::recreate_after_fault` → `recreate_when_quiet` → `recreate`）。`report_harness_fault` 接上 `should_recreate_after_fault`；重建预算是**3 次 + 20s 冷却**（`MAX_REBUILDS` / `REBUILD_COOLDOWN`）——曾经是「每进程一次」，真机数据证伪了它：自动重建落在卸载风暴中间，新窗口 4 秒后又死、额度已尽，用户被晾在黑屏上 25 秒。手动「刷新工作台」先 `reset_budget` 清账，手动出路永远有额度。
  - **这一类改造最容易被单测骗过去**：③ 的判据是纯函数，把调用摘掉照样全绿。`check-invariants.mjs` 因此钉的是**接线形状**（命令层必须调 `recreate_after_fault`，而那个动作里必须问 `should_recreate_after_fault` 且走 `recreate_when_quiet`；安装参数必须含 copy），每个方向都反向验过。
  - **内核侧的两个缺陷仍归内核仓库**（见 AGENTS.md「范围」）：`client-hmr` 把 stat 噪声当重建推给活页面；换模块窗口里的槽位装配不变量会崩。四层是把用户看得见的损害压到接近零并消灭触发源，不是替内核修 bug。
- **壳侧事件落盘**（`shell_events.rs`，2026-09-29）：壳是 GUI 应用，`eprintln!` 在 Windows 上没有任何去处，而**只有后果、没有原因**的动作恰恰只走它（工作台窗口自动重载、给 pnpm 降优先级）。`shell_events::record(<逻辑名>, <行>)` 追加到 `<shell_logs_dir>/<kind>-<name>-<date>.log`，因此**自动出现在「查看日志」面板**（`list_log_files` 按 `.log` 收整个目录）。写失败只落 stderr，绝不阻断调用方。`harness_window` 的加载看门狗现在把「开始加载 / 加载完成 / 超时自动重载」都记进 `harness-window.log`——此前这类现象只能靠时间戳对猜。
- 切换活动内核只允许在**本壳**工作台已停止时执行：版本页在启动或运行期间禁用“切换”，`kernel::set_active` 经 `ensure_own_shell_stopped` 检查本壳的配置端口并拒绝运行中的服务，用户必须先调用 `stop_kernel`。**这条不管另一个壳**，理由见上面「跨壳判据只提示，不阻断」。

## 数据目录

外壳全部状态位于 `<dsh_xlink_home>/<family>/desktop/`（release build）或 `<dsh_xlink_home>/<family>/desktop-dev/`（debug build `tauri dev`），按**内核族命名空间**隔离（dsh 与将来的 mcode 各持一份，互不可见），由 `kernel::data_dir` 解析并在启动时创建。子结构：`kernels/<版本>/`、`logs/`、`settings.json`、`active.txt`、`kernel.pid`、`patches/`（补丁应用记录 `state.json` 与 `backups/` 原文件备份）、`tools/node/v<version>/`（托管 Node.js 运行时，见 `node_install.rs`）与 `tools/downloads/`（下载缓存）。

启动时 `setup()` 在 stderr 打印 `dsh-xlink: data_dir = <path> (family: <family>, build: dev|release)`，让用户一眼确认当前进程用的是哪个目录、哪个内核族。

### 平铺布局的一次性搬迁

v0.2.x 的平铺目录 `<dsh_xlink_home>/desktop[-dev]/` 会在启动解析 data_dir 时**整体 rename** 进 `<family>/`（同卷原子操作，active.txt / 内核安装 / quarantine 一起过去）。rename 失败时继续使用旧目录（宁可留在平铺位置也不让用户面对空的新目录）；新旧并存（上次搬迁中断）时以族目录为准、旧目录保留待手动清理。

### 优先级（`kernel::data_dir`）

1. `DSH_DESKTOP_DATA_DIR` 环境变量——完全覆盖目录路径（用于在外部盘上测试等场景）
2. `<DSH_XLINK_HOME 或 ~/.dsh-xlink>/<family>/<SHELL_SUBDIR>/`——family 来自实例注册表默认实例（`instance::default_family()`，回退 `dsh`）；`SHELL_SUBDIR` 在 release 是 `desktop`、debug 是 `desktop-dev`
3. `app_data_dir()`（OS app-data 目录）作为 xlink home 不可写时的 fallback

### 为什么 dev 和 release 用不同目录

`settings.json`（端口配置）、`active.txt`（当前激活版本）、`kernel.pid`（运行中内核的 PID）、`kernels/`（安装的内核）、loopback 端口都是**共享资源**。一个开发者同时跑 `tauri dev` 和已装的 release shell 时，两个实例会互相争端口（`port_open` 拒绝启动）、互相 kill（任意一方点"关闭工作台"就把对方的内核杀了）、互相覆盖 `active.txt` 和 `settings.json`。分目录 + 错位端口（debug 3091 / release 3090）让两边完全互不读对方的 state——dev 可以放心改端口、切内核、看 log，不会污染 release shell 的视图。

### 端口（`kernel::DEFAULT_PORT`）

- debug build：3091（release 默认 3090 + 1）
- release build：3090

`Settings::default()` 的 port 在 `settings.json` 缺失时用 `kernel::DEFAULT_PORT`；用户保存过的 port 优先。

## 窗口

窗口能力的复用接口、接入流程和当前代码审查见 [窗口核心能力设计与代码审查](window-architecture.md)。

- `main`（管理面板）：`tauri.conf.json` 里配置为主窗口；加载 `ui/` 静态资源，`capabilities/default.json` 拥有全部本地命令权限。macOS / Windows 的无边框由 `tauri.conf.json` 的 `decorations: false` 在**建窗时**给定——不要在 `setup` 里用 `set_decorations(false)` 事后改：那次改动会重算 macOS 的 `NSWindowStyleMask` 并抹掉 `Miniaturizable`，标题栏黄灯随之静默失效（根因与验证见 [troubleshooting.md](troubleshooting.md)）；前端 `WindowTitleBar.vue` 自绘窗口按钮、拖拽区和标题栏：macOS 是左上角交通灯，Windows 是右侧 46×32 的最小化 / 关闭按钮（Windows 11 标准尺寸与反馈）；release 使用从左到右 5% 到 70% 不透明度的深 Gitea 绿色笔刷，dev 使用同样规则的低亮度鲸眼红色笔刷；Linux 暂保留原生标题栏。Windows 上该窗口**常驻后台**：`tray.rs` 在 `setup` 里建立通知区域图标（`icons/tray-{dark,light}-{16,20,24,32,40,48}.png` 十二档都由 `include_image!` 编译期解码：按 `Personalize\SystemUsesLightTheme` 选浅色/深色两套（浅色套板、深色透明底 + 白边），按 `GetSystemMetricsForDpi(SM_CXSMICON, …)` 选档，`watch_theme` 的后台线程等注册表变化换帧、`WindowEvent::ScaleFactorChanged` 重取——见 [icon-design.md](icon-design.md)；右键菜单「显示主界面 / 退出 dsh-xlink」、左键单击叫回窗口）；`RunEvent::WindowEvent::CloseRequested` 先经 `tray::intercept_close` 接管——`prevent_close()` 后走 `tray::hide_to_tray()`（`set_skip_taskbar(true)` 即 `ITaskbarList::DeleteTab`，再 `hide()`；收起那一刻窗口已隐藏，页内提示谁也看不见，因此**收起时不再广播任何事件**），标题栏最小化按钮则走 `minimize_shell` 命令落到同一个 `hide_to_tray()`。因此关闭与最小化都只是收起窗口（内核、工作台、官方对话照常运行，任务栏与 Alt+Tab 不再保留一个点了没反应的条目），恢复时 `tray::show_main_shell()` 先 `set_skip_taskbar(false)` 再 `show()` + 聚焦，并按 `HIDDEN_TO_TRAY` 判定「确实是被收起的」而非从别处叫回——**每次启动只在第一次这样的恢复上**广播 `shell-restored-from-tray`（`RESTORE_HINT_SHOWN` 一票制），前端据此弹 4 秒的「刚才已收起到通知区域」页内提示（这是唯一能讲清「程序还在后台、怎么找回来」的可见时刻，久了也确实会像崩了）；恢复一定由用户点托盘图标或工作台拉绳触发，他刚证明自己知道怎么叫回来，所以第二次起一律静默。因此关闭与最小化都只是收起窗口（内核、工作台、官方对话照常运行，任务栏与 Alt+Tab 不再保留一个点了没反应的条目），恢复时 `tray::show_main_shell()` 先 `set_skip_taskbar(false)` 再 `show()` + 聚焦。退出改由托盘菜单发起：它把窗口叫回前台并广播与系统关闭按钮相同的 `request-quit-confirm`，前端确认后依次执行 `stop_kernel` → `confirm_close_shell`（销毁全部窗口 + `app.exit(0)`）。恢复动作每个平台只有一份实现（Windows 在 `tray::show_main_shell`，其它平台在 `lib::show_main_shell`），`lib::show_main_shell` 只做平台分发，工作台拉绳的 `focus_main_shell` 与托盘共用。
- dev 调试面板（仅 dev 构建，`ui/src/components/DebugPanel.vue`）：右下角 🪛 浮按钮打开，唯一动作是「模拟正式版外观」。打开后把 `rel-build` 类挂上 body（复用 release 的绿渐变背景），**并收起 dev 专属内容**——设置页的「模拟一次任务完成」、概览页版本号的「（dev）」后缀，以及面板里那份钩子速查。它们只读 `store.devUi`（= `dev_build && !releasePreview`，由 `store.applyBuildClass()` 唯一写入，首帧从 body 类兜底以免 HMR 后 dev 入口先隐藏再闪回）：新增 dev 专属内容时必须走同一字段，否则 release 预览只演出了半个正式版。**浮按钮不走这个字段**：它在两种模式下都留在右下角（预览中换成 Gitea 绿以示区分），否则切进 release 预览就没有回头的入口了——刻意不做键盘快捷键，切换靠这个按钮。预览标志只在内存里，重启应用即复位；预览只改界面，不改外壳行为（`dev_build` 仍是 true，Rust 侧一概不动）。**面板不会在切换预览时自动收起**：Element Plus 的 switch 在 `handleChange` 里先发 `change`、再用 `nextTick` 写模板 ref `input.value.checked`，同步卸载开关会让那个回调拿到 `null`，在控制台留下一条 `Unhandled Promise Rejection: Cannot set properties of null (setting 'checked')`（实测于 element-plus 2.14.5）。要收起面板请另找时机，不要挂在 `@change` 触发的 watch 上。

- `harness`（工作台）：`open_harness` 在新 OS 线程里 `WebviewWindowBuilder::new(...).label("harness")`，先对日志中的 launch-token URL（旧版内核则对裸地址）做 loopback HTTP 探针，确认可用后加载；不设 `closable(false)`——macOS 交通灯红灯与 Windows × 关闭按钮保持可用，用户可随时自己收起工作台窗口，内核与任务继续在后台运行、随时可经「打开工作台窗口」重新打开，收起窗口不会丢内核会话；由 `stop_kernel` 用 `destroy()` 主动回收窗口并停止内核。`capabilities/harness-remote.json` 仅授权 `allow-focus-main-shell` 与 `allow-report-harness-fault`，URL 锁 `http://127.0.0.1:*`；后者还会在 Rust 命令层校验 webview label 必须是 `harness`。打开前由 `kernel::prepare_workbench_source_maps` 扫描当前内核的前端 `dist`，为 npm 包中保留 `sourceMappingURL` 但缺失的 `.map` 生成最小 sidecar；不改内核 JS，目录只读时也不阻断工作台启动。
- `official-chat`（官方对话，多页签）：`open_official_chat` 在新 OS 线程里建一个裸 `WindowBuilder`（label `official-chat`，需 tauri `unstable` feature），再 `Window::add_child` 挂两类子 webview——顶部 `official-chat-strip`（加载本地 SPA `index.html?chatstrip=1` 渲染页签栏，保留 `window.__TAURI__` 故能调 `official_chat_tabs` 读页签列表 / `switch_official_chat_tab` 切换活动页签；该 webview 同时直接承载拉绳挂件——详见下面 `pullstring-launcher.js` 那条）与 `OFFICIAL_CHAT_TABS` 的内容 webview 按需挂载（`official-chat-tab-{i}`，默认只创建并显示首个，目标页签首次选择时先创建成功再切换，已创建页签隐藏而不销毁以保留状态）；`relayout_official_chat` 在窗口显示后的下一条主线程消息以及 `Resized` / `ScaleFactorChanged` / `Focused(true)` 事件中把页签栏钉顶、内容铺满下方；父窗口在创建子 WebView 前即为可见；若 AppKit 仍返回 provisional client size，初始布局使用请求的 1366×768 logical size；重排路径同样拒绝小于 38pt 的临时小尺寸，并在窗口落定后补一次延迟重排以应用真实尺寸。窗口显式启用 `TitleBarStyle::Transparent`（macOS 上 `Visible` 默认会启用 `NSWindowStyleMask::FullSizeContentView`，把内容视图延展到标题栏之下；位于 (0,0) 的页签栏 strip 会被约 28pt 的标题栏遮挡，三个页签只露出几像素；`Transparent` 保留可见标题栏但禁用 `fullsize_content_view`，让内容视图落在标题栏之下，strip 完整可见）。内容 webview 共用 `webview-official-chat` 目录（Windows）/ `OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`（macOS）持久化登录态，origin 隔离使 DeepSeek、千问、MiniMax 三页签不串数据；不覆盖 user-agent（诚实桌面版 Edge，UA / 客户端提示 / `userAgentData` 一致）、各内容 webview 统一 `OFFICIAL_CHAT_BROWSER_ARGS`（满足 WebView2 同目录同参数约束）、strip 固定为 38px 并由本地 SPA 绘制背景，不设 `closable(false)`，OS 关闭按钮正常工作，重复点击复用既有窗口并 `set_focus`，`close_official_chat` 销毁窗口即连带销毁子 webview、登录态落盘保留。`capabilities/official-chat-strip.json` 授权 `allow-official-chat-tabs`（仅本地页签栏 webview，页签命令 + 拉绳 `focus_main_shell` 一条），`capabilities/official-chat-remote.json` 锁 `https://chat.deepseek.com/*`、`https://www.qianwen.com/*` 与 `https://agent.minimaxi.com/*`、不授任何 shell 命令（拉绳属于整个官方对话窗口的 chrome，由页签栏 webview 承载，不下沉到各远程内容页）。`StatusView.official_chat_open` 由 `get_status` 在 `spawn_blocking` 内经 `get_window` 同步读取，作为面板按钮切换「打开官方对话」/「关闭官方对话」标签的信号。
- `usage-viewer`（模型用量）：`open_usage_window` 在新 OS 线程里建 `WebviewWindowBuilder`（与 `open_log_window` 同一条路：主线程同步建 webview 在 Windows 上会死锁、结果经 mpsc 回传、20s 超时兜底），已有窗口先 `destroy()` 再重建——窗口只读，重建即顺手拿到一次 force 重扫。尺寸 **760×800：高度与主壳完全一致**（main 固定 480×800），最小 720×520、可缩放；`?usage=1` 挂载 `UsageWindow.vue`，`capabilities/usage-viewer.json` 只授予 `get_model_usage`（只读）。**吸附与跟随**：初始位置与拖动跟随共用 [`dock_x` / `dock_y` / `compute_dock_position` / `dock_position_logical`]（window.rs，物理像素纯函数、`dock_*` 有单测）——默认把窗口贴在主窗右侧、顶边与主窗对齐；主窗贴近屏幕右缘、右侧放不下 760px 时翻到主窗**左侧**；垂直方向与主窗顶对齐、底部超出行程就上移夹在屏内。所有取值用主窗的 `outer_position` / `outer_size` / `current_monitor`（物理像素）计算，再除以 `scale_factor` 换算成 builder 需要的逻辑坐标。**拖动跟随**由 `window::attach_dock_listener`（lib.rs `setup` 注册，与日志查看器共用同一份实现）实现：挂主窗自己的 `on_window_event`，只认 `Moved` 事件，每次重算吸附位置后 `set_position`；**15ms 合帧**把 `set_position` 压到 ~60fps——拖动时 `Moved` 触发频率远超刷新率，逐事件下发会让指令在主线程队列积压出迟滞感，中间位置直接丢弃、松手后终点精确。每轮拖动开始的第一次 `Moved`（距上次 tick 超 400ms）先 `set_focus` 把副窗提到最前——两窗连动穿过其他应用窗口时不会被压在人家后面（tao 没有"提到最前但不抢焦点"的 API，每个拖动回合只做一次，焦点落在副窗上，点任意窗口即可收回）。只认 `main` 的 Moved，用量窗口自身移动不回环；主窗不在或取不到显示器信息时短路回退默认居中。`get_model_usage` 非 force 调用带 45s 新鲜度窗口，`force`（打开 / 刷新）越过。**建出后的吸附校正**（`window::snap_to_main`，三类查看器窗口共用）：builder 阶段只有逻辑目标尺寸，两窗的真实可见帧要等窗口建出来才能量到——`snap_to_main` 以 DWM 实测为准做两件事：把**可见内容高度对齐主壳可见内容**（`inner_size(…, 800)` 建出的窗口可见高度与主壳差出一截标题栏，Windows 上视觉尤其明显；装不下最小内框时不动尺寸）；按 `visible_frame`（Windows 走 `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`，即 DWM 实际绘制窗口边缘的位置）量出**两窗各自的可见帧**并按可见缘贴齐落位——Windows 上不止带装饰副窗，主壳（tao 的「无装饰 + 阴影」窗口）rect 里同样左右各含约 7 逻辑像素、Win11 顶部再含 1 像素的不可见缩放边框，客户区/外框坐标都推不出可见缘（只补副窗一侧会留下主窗边框宽度的缝，真机两轮反馈「没有紧贴主窗」即此因叠加），DWM 可见帧是该口径的唯一权威。拖动跟随每个 tick 也重新量两窗可见帧做同样的贴齐（跨屏拖动会换 DPI、边框宽度随之变化），避免缝隙在第一次拖动后重现。
- `subscription-viewer`（套餐用量）：`subscription::open_subscription_window` 与 `open_usage_window` 完全同一条建窗路（新 OS 线程 + mpsc 回传 + 20s 超时、已有窗口先 `destroy()` 再重建——重建即顺手拿到一次 force 全量查询），尺寸直接复用 `USAGE_VIEWER_SIZE`（760×800，min 720×520），吸附 / 拖动跟随经 `window::SUBSCRIPTION_VIEWER_LABEL` 白拿同一份 `attach_dock_listener`。`?subscription=1` 挂载 `SubscriptionWindow.vue`，`capabilities/subscription-viewer.json` 授予 `get_subscription_usage`（只读查询）、`open_usage_window`（窗口底部「本地 token 用量见模型用量窗口」互跳入口）与 `open_harness`（「前往模型设置」——凭据的唯一编辑入口在工作台模型设置），其余一概不放。概览卡的隐藏语义同样生效于本窗口的数据源：configured 但一直查不到数据的 provider 会在概览卡被提示隐藏（localStorage 持久），`collectErrors` 跳过已隐藏项，任何一次查询成功（含会话首查 force，见 `loadSubscriptionSummary`）自动恢复；本窗口作为「详情」视图仍列出全部 configured 分区。查询行为见上文 `subscription.rs` 条目。

`titlebar-pulse.js` / `pullstring-launcher.js` / `chat-fingerprint.js` / `harness-health.js` / `no-context-menu.js` 由外壳通过 `WebviewWindowBuilder::initialization_script`（harness）/ `WebviewBuilder::initialization_script`（official-chat 本地 webviews 与内容子 webview）按各自需要的集合注入：`official-chat` 内容 webview 注入 `titlebar-pulse.js` + `chat-fingerprint.js` + `no-context-menu.js`（拉绳属于窗口 chrome，不下沉），`official-chat-strip` 页签栏 WebView 注入 `pullstring-launcher.js`（拉绳挂在 strip 右上角，24×38 logical SVG 完整位于 38px HWND 内），`harness` 工作台注入 `titlebar-pulse.js` + `pullstring-launcher.js` + `harness-health.js` + `no-context-menu.js`。**壳自己的**五个窗口（`main`、日志查看器、模型用量、套餐用量、官方对话页签栏）加载的是同一个本地 SPA，右键策略因此不走注入脚本，而是由 `ui/src/noContextMenu.js` 在 `ui/src/main.js` 里挂载组件之前装一次：

- `pullstring-launcher.js`（注入到 `official-chat-strip` 页签栏 webview + harness 工作台 webview，不注入到 `official-chat` 内容子 webview）：在页面右上角挂一盏「小台灯」形状的拉绳挂件（24×38 logical 的紧凑 SVG：短拉链 + 梯形灯罩 + 灯泡 + 细杆 + 圆角底座），pull 一下点亮灯泡、调起 `focus_main_shell`。strip 时贴 `right:12px`（strip 右内沿）、cord 为 `#4D6BFE`（官方对话品牌蓝）；workbench 时贴 `left:212px`（侧栏折叠按钮旁）、cord 为 `#609926`（Gitea 绿）。surface 检测看 `window.location.search` 是否含 `chatstrip=1` 或 `chatlauncher=1`（后者已无对应路由，作为兼容保留）——任一命中就走官方配色 + 右侧锚定，workbench 不带就走绿色 + 左侧锚定。strip webview 自己就是天然 38px Tab Bar 高度，`.chat-strip` 整片 `background: var(--el-bg-color)` 覆盖住默认 WebView2 白色 HWND bg，lamp 整 36px 几何落在 strip HWND 内不溢出：cord y=2–5 / 灯罩梯形 y=5–13 / 灯泡 cy=14 r=2（部分压在灯罩下）/ 杆 y=16–30 / 圆角底座 y=30–36。`top: 0` 自然挂 strip 顶 = 窗口顶。拉绳属于整个官方对话窗口的 chrome，由 strip WebView 承载，不下沉到远程内容 WebView。`focus_main_shell` 由 `allow-official-chat-tabs`（strip 路径）授权。`official-chat` 内容 webview 不授任何 shell 命令，对应 `official-chat-remote.json` 的 `permissions` 为空。
- `titlebar-pulse.js`（注入到 harness 工作台 webview + `official-chat` 内容子 webview）：接管 chrome-row 顶部条带。`location.hostname` 命中 DeepSeek / 千问 / MiniMax 三个授权 origin 之一时使用官方品牌蓝 `#4D6BFE`（rgb 77,107,254）；否则用 Gitea 绿 `#609926`（rgb 96,152,38）。脚本只注入静态 3px 品牌线，不创建 `@keyframes`、transform、filter、box-shadow 或常驻定时器，避免流式会话更新触发持续 WebKit 布局与合成。workbench 的第二装饰节点在静态模式下隐藏，chat 页面不再创建额外节点。
- `chat-fingerprint.js`（仅注入到 `official-chat` 内容子 webview，**必须排在 `titlebar-pulse.js` 之后**）：只清除嵌入式痕迹，不再伪造浏览器指纹——把 `navigator.webdriver` 钉在 `false`（正常浏览器的值），并删除 `__TAURI__` / `__TAURI_INTERNALS__` / `__TAURI_METADATA__` / `__TAURI_IPC__` 全局（正常浏览器里它们根本不存在；暴露任何形式的 Proxy 都等于自报嵌入式身份）。其余表面保持真实：引擎是货真价实的桌面版 Edge，用普通 JS 对象冒充 `userAgentData` / plugins 等反而会被原生类检查识破。拉绳挂件由 strip WebView 承载，`chat-fingerprint.js` 仅作用于远程内容 WebView；两者不共享页面，因此不影响 Tauri bridge。
- `no-context-menu.js`（注入到 harness 工作台 webview + `official-chat` 三个内容子 webview；壳自己的窗口由 `ui/src/noContextMenu.js` 负责）：禁用**鼠标右键菜单**，只做这一件事。监听挂在 `window` 的**捕获**阶段并 `preventDefault` + `stopImmediatePropagation`——捕获是事件传播的第一站（页面自己在 document / 目标元素上挂的监听器抢不到前面），而 `stopImmediatePropagation` 连同一节点上后注册的监听器一起拦掉（只写 `stopPropagation` 的话，官方对话那三个重框架 SPA 的自绘菜单照弹）。**刻意不写 `user-select: none`、不拦 `select` / `copy` / `mousedown`**：左键拖选与 Ctrl/⌘+C 复制必须照旧可用，把「禁右键」写成「禁选中」是这条需求最常见的写错方式，而它坏掉时不报任何错——菜单确实没了，用户只是再也复制不出东西。引擎层的菜单（Wry 的 `with_default_context_menus` / WebView2 的 `AreDefaultContextMenusEnabled`）在 Tauri 2.11 上没有对外接口，因此这一层只能靠 DOM 事件取消。**验证边界如实记着**：落地时有单测（`ui/test/noContextMenu.test.js`，对着假事件流断言行为）与机械检查（第 16 项逐条查建窗链的接线），但**没有起壳在真机上逐个引擎核对**——尤其 `<input>` / `contenteditable` 这类可编辑元素上的原生菜单由引擎提供，Chromium 系上取消 `contextmenu` 应当一并压掉，仍需有人在 Windows 与 macOS 上各点一次确认。安装新版本后已存在的 WebView 不会自动替换注入脚本，需重新打开工作台 / 官方对话窗口。
- 四个脚本顶部都有 `if (window.top !== window.self) return` 顶帧守卫，避免 Tauri 在每个 iframe 都执行初始化脚本时挂出多份拉绳 / 条带 / 指纹 stub / 右键拦截；harness 工作台在嵌套预览里也可能挂多个 iframe，守卫能让顶层唯一实例化。
- `harness-health.js` 只在顶层工作台文档运行，监听 `error` / `unhandledrejection`，并在页面挂载后两次延迟检查可见内容以识别白屏；上报 `{ kind, message, stack, pageUrl }`，IPC 失败时最多重试 4 次。`error` 分两类：**可执行错误**照旧报 `runtime-error`；**内核客户端模块 bundle 的 `<script>` 加载失败**单列为 `bundle-load-failure`，把元素的 `src`（唯一带完整 `/plugins/??<包名>/client.js,…&rev=…` 组合路由的地方）同时填进 `message` 与 `stack`。这条不能省：内核对加载失败的模块行是**静默丢弃**的（`dsh-client-modules` 逐行 catch 后只写进「Web 启动审计」，页面照常起来），工作台随后只会抛 `renderSlot('root') before any 'root' registration (boot order)` 这类启动顺序错误——而那类堆栈落在多成员组合上、按下面的规则拒绝归因，于是证据里一个包名都没有。因果先于症状：一次事故只报第一条 bundle 失败（内核按启动顺序分批请求组合路由，先失败的那批才是因，后面那些改用单资源 URL 的重试失败只是连带），后续的 `runtime-error` 不覆盖它；`/plugins/` 以外的资源失败（图片、字体、第三方脚本）仍然忽略，那条路由只服务客户端模块 bundle。`message` 取异常的「类型: 消息」以及 `cause` 链，与 `stack` 分开：WebKit 的 `Error.stack` 只有帧（`fn@url:行:列`），没有 V8 那样的消息首行，只上报堆栈会让事故证据里没有任何可读原因。`report_harness_fault` 只接受 `harness` webview、限定类型和有界字符串，在 `spawn_blocking` 中把前端证据与今天的内核日志（`<kind>-kernel-<YYYY-MM-DD>.log`，见「日志规范」）合并分析：路径/模块证据指向插件时临时隔离插件，内核内置组件（`@deepseek-ai/dsh-*` 命名空间，或内核 loader / 渲染器输出的固定文案：`build-time externals drift` / `missed the module table` / `no registered package factory` / `rendered without an installed adapter`）归因到当前内核版本（不自动改插件）；这些内核自述的文案由内核写出、插件伪造不出来，因此**不受**多成员组合路由那条拒绝规则约束——内核侧与插件侧两套相反的边界判据都收在 `kernel_evidence.rs`（必须并排可读；隔着一个 900 行的文件正是当初它们被写成同一种宽进出的来源）。其中**槽位装配不变量**（`rendered without an installed adapter`、`renderSlot('root') before any …`）单独成 `cause: kernel-boot`：归因仍是内核，但第一动作是**关闭并重新打开工作台窗口**——这类启动顺序竞态刷新一次多半就好，切版本与停用第三方插件都是更重、且更可能冤枉插件的动作，仅 `blank` 探针命中且日志无明确证据时把全部已安装插件列为「软信号」嫌疑但**不自动隔离**（避免误停用），证据完全不足时回退到「unknown」分支提示用户查看日志或切换内核版本。前端堆栈里的包名只出现在内核 client-modules 的组合路由（`/plugins/??<包名>/client.js,…&rev=…`）查询串里，因此归因对 bundle 成员另有一条锚定规则：只有**单成员**组合路由（以及 map 来源名里的 `/plugins/<包名>/client.js`）才锁定到该包；多成员组合是若干插件拼成的同一个脚本，按成员逐个匹配会把同批的旁观者写进隔离清单，按内核命名空间匹配则会把插件的错记到内核头上——两类都必须拒绝。本机 WebKit 的堆栈文本还会把查询串整个丢掉（只剩 `http://127.0.0.1:3090/plugins/`），此时判为「前端 bundle 异常（未定位到包名）」：文案明确页面仍在运行，不隔离、不改插件、不弹事故面板——这类报告没有可处置的对象，打断用户没有收益，因此只记录到 `last-incident.json` 并由概览横幅提示，横幅的「查看详情」带 `force` 打开面板（`store.js` 的 `showIncident` 只对 `cause === 'frontend'` 且 `kind` 为 `unhandled-rejection` / `runtime-error` / `bundle-load-failure` 的未恢复报告降级；有强证据的插件/内核报告、启动失败与 `blank` 白屏照旧弹面板）。事故持久化到 `last-incident.json`，`load_incident` 会在读路径上把这类判据已被修复的旧记录改判为「前端 bundle」（只改判断与文案，`suspects` / `attempts` / `log_tail` 原样保留、不回写文件），管理面板展示证据并提供插件处理、日志和内核版本入口。

## 多内核改造后的实际数据布局

> 本节描述当前代码已经落地的多实例数据布局。完整设计与开发计划见
> [dsh-xlink-multi-kernel-design.md](dsh-xlink-multi-kernel-design.md) 与
> [dsh-xlink-multi-kernel-development-plan.md](dsh-xlink-multi-kernel-development-plan.md)。
> 阶段性 commit 快照见 [multi-kernel-migration-status-2026-09-19.md](multi-kernel-migration-status-2026-09-19.md)。

### 路径解析分工（`src-tauri/src/shell/paths.rs`）

`paths.rs` 是路径解析的单一入口。**两套正交环境变量**：
- `DSH_XLINK_HOME`：Xlink 自身的数据目录（`<xlink_home>/`）。中央库 / 活动视图 / 备份 / cache / state 都在这里。
- `DSH_HOME`：dsh 内核进程的用户数据根（profile / sessions / credentials）。启动实例时由 `DshAdapter` 注入为**实例 home**（`<xlink_home>/kernels/<family>/instances/<id>/home/`）；它不再指向 `~/.dsh`——那里的历史用户数据由 [`instance::migrate_legacy_dsh_home_if_needed`] 在壳启动时一次性并入实例 home。

### Xlink home（`<xlink_home>/`，由 `DSH_XLINK_HOME` 解析）

```
<DSH_XLINK_HOME>/
├── shell/<mode>/                  # release / dev Shell 自己的设置 / UI 状态 / 日志
│   ├── settings.json              # Shell 设置（不含任何模型凭据）
│   └── logs/                      # Shell 日志（含 subscription 查询错误日志）
├── kernels/<family>/              # 实例目录（family = dsh / mcode）
│   └── instances/<id>/            # 每个实例独立的 workspace + DSH home + runtime
│       ├── home/                  # 该实例的 DSH_HOME（profile / sessions / credentials）
│       ├── extensions/plugins/<id>/ # 插件物化目录（**多实例隔离**）
│       ├── extensions/wiring.json  # 该实例的 profile 接线记录
│       ├── usage/state.json       # 模型用量增量账目（usage.rs，按实例隔离）
│       ├── subscription-cache.json # 云端套餐用量缓存（subscription.rs，按实例隔离）
│       └── workspace/              # 内核进程 cwd
├── <family>/desktop[-dev]/        # Shell 运行时目录（**当前内核安装落在这里**）
│   ├── active.txt                 # 活动内核版本
│   ├── kernel.pid                 # 工作台进程锁
│   ├── plugins-catalog.json       # 社区插件目录缓存（TTL 6 小时）
│   └── kernels/<version>/         # 内核安装产物：node_modules/ + package.json + pnpm-lock.yaml
├── plugins/                          # 插件中央库的命名空间（内层按内核族 + 壳模式分家）
│   ├── dsh/                          #   release 壳的中央库（存量由 dsh-plugins/ 搬入）
│   └── dsh-dev/                      #   dev 壳的中央库（首次从 release 那份复制种子）
├── skills/
│   ├── packages/<id>/             # 技能中央库（**两个壳共享**——见下「有意共享」）
│   └── active/                    # 技能活动视图（**两个壳共享**——DSH 经 customSkillDirs 接入）
├── state/                         # 端口锁 + 实例注册表
│   ├── instances.json             #   release 壳的实例注册表（沿用已发布版本的路径）
│   └── instances-dev.json         #   dev 壳的实例注册表（2026-09-29 起按壳模式分文件）
├── cache/                         # nodejs / 内核下载缓存
├── backups/<migration_id>/        # 迁移向导的 backup（回滚时按此还原）
└── xlink.json                     # Xlink 自身的元数据
```

### 两个壳之间共享什么（2026-09-29 逐条核对）

判据只有一条：**可变的、两个进程会同时碰的状态一律分家**。逐条结果：

| 数据 | 位置 | 谁在写 |
| --- | --- | --- |
| Shell 设置 / 日志 | `shell/<mode>/` | 各写各的 |
| 内核安装树 / `active.txt` | `<family>/desktop[-dev]/` | 各写各的 |
| **实例注册表** | `state/instances.json` / `state/instances-dev.json` | **各写各的**（`registry_split`） |
| 实例目录 / DSH home / 插件物化 | `kernels/<family>/instances/<id>/` | 按实例 id 天然分开 |
| **插件中央库** | `plugins/dsh/` / `plugins/dsh-dev/` | **各写各的**（`store_relocate`） |
| 技能中央库 / 活动视图 | `skills/packages/` + `skills/active/` | **两个壳共享——有意为之** |
| 社区插件目录缓存 | `<family>/desktop[-dev]/plugins-catalog.json` | 各写各的 |

**注册表为什么必须分文件**（2026-09-29 改）：此前两个壳共用一个 `state/instances.json`，而它的互斥只到进程级（`instance::lifecycle_mutex` 是进程级 Mutex）——两个进程对同一个文件做读-改-写没有任何序列化，后写的那份会把前一份的记录整条盖掉；此外 dev 壳删一个实例会改到 release 壳顶部页签的列表。`registry_split::ensure_scoped` 在 setup 期一次性拆分：自己的文件不存在就从共享的旧文件**拷**一份（拷不是搬——对方还没认领时它也得能读到同样的记录），对方认领之后把自己这份里的对方默认实例记录删掉。规则顺序无关、可重复跑，细节见该模块文档。分完之后「只有 release 能写共享指针」那条防御性限制随之撤销——谁写都只写自己那份文件。**代价与目的同源**：顶部页签不再列出另一个壳的实例。

**技能为什么继续共享**（2026-09-29 维护者决定）：它是「可读的源码 + 启用清单」，dev 侧装一个新技能只会让 release 的技能列表多一项可见内容，**不影响工作台（webui）的会话 / 模型 / 插件**。这与插件中央库共享造成的后果是**两类不同的问题**，别混谈：插件中央库是 dev 改**源码** → 被 `link` 物化进 release 正在跑的实例 → 内核当场抛 `scope '…' rendered without an installed adapter` 白屏；技能这边只是清单里多一行。

**族解析一级都不读注册表指针**：`instance::default_family` → `kernel::data_dir` 只走本壳自己的状态（本壳当前实例 → 本壳默认实例 → `dsh`）。`data_dir` 装的是内核安装树（`active.txt` + `kernels/<version>/` 几百 MB），让它取决于「另一个壳上次写了什么」就是跨壳竞态——`check:invariants` 第 12 项禁止任何生产代码读 `default_instance_id` 做决策。

### 内核安装路径：P1 布局已备好，但安装链路仍走旧位

**这一节描述的是 v0.3.3-rc.3 的真实状态，不是设计意图。**

`paths.rs` 同时提供两套内核安装路径，**当前只有旧的那套在用**：

| | 路径 | 谁在用 | 实机 |
| --- | --- | --- | --- |
| 旧（实际生效） | `<xlink_home>/<family>/desktop[-dev]/kernels/<version>/` | `kernel::install_version` → `kernel_dir(data_dir, version)`；`list_installed` / `set_active` / `active.txt` / 前端"已安装版本"列表全走这里 | `~/.dsh-xlink/dsh/desktop/kernels/0.1.7-rc.1`、`0.1.7-rc.2` |
| 新（已备好未迁） | `<xlink_home>/kernels/<family>/versions/<version>/` | 只有 `DshAdapter::resolve_install_dir` 读它 | `~/.dsh-xlink/kernels/dsh/` 下**只有 `instances/`，`versions/` 根本不存在** |

`paths::kernels_root()` / `kernel_versions_dir()` / `kernel_version_dir()` 的注释已经写明这个状态：「P1 阶段内核安装目录**仍位于旧版 data_dir**，本函数仅用于规划新位置与将来的实例注册。调用方在 P2 之前不应往这里写。」`DshAdapter::resolve_install_dir` 内部的 legacy 兜底是**打桩**（直接返回 `None`），真正的旧位兜底在调用方 `resolve_install_root` 那一层做。

**因此排查内核安装问题时看旧位，不要去 `kernels/<family>/versions/` 找。** 把安装链路迁到新位是代码改动，不在文档对齐范围内；迁移完成前两套路径会继续并存，届时本文需要同步更新。

另一条容易被忽略的事实：**release 与 dev 壳各装一份独立内核，互不共享。** `tauri dev` 用 `desktop-dev/`，安装包用 `desktop/`，两边的 `kernels/<version>/` 与 `active.txt` 都是分开的——在 dev 壳里"安装新版本"不会影响 release 壳，反之亦然。这也是同一台机器上 `desktop/kernels/` 与 `desktop-dev/kernels/` 各有两份相同版本目录的原因。

### DSH home（`~/.dsh/`：旧版默认 home，已并入实例 home）

旧版外壳不注入 `DSH_HOME`，内核一直以默认 `~/.dsh/` 运行，这里因此积累了内核的用户数据（`profiles/`、`sessions/`、`storages/`、`synapse/`、`attachments/`、`logs/`、`.credentials.yaml`、`settings.yaml*`、`cordis.patch.yml`、`dsh-taskboard*.json`、`.anonymous-user-id`、`llm-deepseek/`、`cache/`）。壳启动时 `instance::migrate_legacy_dsh_home_if_needed` 会把这些条目**递归并入**默认实例的 `instances/<id>/home/`：目标已有的条目以目标为准（接线产物更新）、缺失的移入，`node_modules` 不动（由 `ensure_wiring` 按 package.json 重建）。迁移标记记录 schema 版本，升级后会补跑新增的数据项；活动 `settings.yaml` 缺失时，从 `settings.yaml.imported` 复制恢复，原归档保留。清单外的外壳旧目录（`desktop[-dev]/`、`plugins/`、`skills*`）原样保留，归迁移向导管：

- `<xlink_home>/<family>/desktop[-dev]/kernels/<version>/` — **当前**内核安装位置（`install_version` 写入；`resolve_install_root` 的 legacy 兜底也指向这里）
- `<xlink_home>/<family>/desktop[-dev]/{active.txt, kernel.pid, quarantine.json}` — Shell 状态 / 内核进程锁 / 隔离记录（`logs/` **不**在这里，见「日志规范」）
- `<DSH_HOME>/desktop[-dev]/plugins.json` — **旧** 插件中央库（已迁到 `<DSH_XLINK_HOME>/plugins/dsh/`）
- `<xlink_home>/dsh-plugins/` — **已发布版本**的插件中央库，两个壳曾共用（2026-09-29 由 `store_relocate` 整体搬进 `plugins/dsh/`，dev 壳复制一份到 `plugins/dsh-dev/`）
- `<DSH_HOME>/desktop[-dev]/store.json` — **旧** 技能中央库（已迁到 `skills/packages/`）
- `<xlink_home>/<family>/desktop[-dev]/skills/` — **旧** 技能活动视图（已迁到 `skills/active/`）

**旧 → 新** 路径映射由 `migration::LegacySource` 表达，迁移向导 `migration::run_migration` 按这个映射把旧布局导入新布局（**旧源永不被删除**——rollback 路径依赖）。中央库清单 `store.json` 不走整文件复制/跳过：任何冲突策略下都按条目 `id` **合并**进目标清单（目标已有条目优先、只补缺失条目），否则目标侧清单一旦比源「新」（哪怕内容是测试夹具泄漏之类的错误数据），源记录就永远迁不进来。技能活动视图里的符号链接会被解引用成**内容拷贝**落地（跨根链接不保留），这些拷贝在清单里没有物化指纹——启用时按「内容与中央库源逐字节一致即收编」自动补记账（`skills::ensure_entry` 的 `identical_unowned_copy`），内容不一致的同名条目仍按冲突拒绝。

### 「搬错实例」的历史会话回收（`home_recovery.rs`）

`legacy_migration_target()` 把 `~/.dsh` 的历史锁定只搬进 release 实例，**防的是再犯**。它防不了已经犯的：2026-09-28 11:51 dev 壳跑过旧逻辑，把用户的历史会话并进了 `default-dev`，而这道闸门 12:09 才落地（commit `713bb40`）——release 侧工作台从此是空列表，`~/.dsh` 已空、搬不动第二次，数据没丢，只是不在本实例的 `DSH_HOME` 里。

`home_recovery` 是那条缺失的回收路径，纪律与 `restore` 同一套：

- **只读扫描先行**：`scan_misplaced_home(family, id)` 列出「别的实例 home 里有、而本实例没有」的条目。持有者集合 = 另一个壳的默认实例 id（编译期常量，不依赖注册表是否完整）+ 注册表里同族的其它实例，**自己不算自己的持有者**。
- **要搬两样，少一样用户仍然看不到**：
  1. `sessions/<工作区>/session-<uuid>/`（连同 `attachments/`）——会话正文；
  2. `<home>/storages/workspace.json` 里的**工作区条目**——会话列表的来源。

  第 2 样是 2026-09-29 在本机实测出来的：只把 `sessions/` 拷过去，内核确实重新解析了那个会话（`storages/session_projcache/` 出现新条目），但工作台里**仍然不显示**——内核列会话读的不是目录，而是 `workspace.json` 的 `tables.workspaces[<wsId>].sessionIds`（目标实例那份是当天新建的，里面没有那个工作区）。因此它是 **JSON 合并**而不是目录复制，且只在「本实例确实有该会话目录」时才登记 id——收编一个指向不存在会话的条目，会留下一条打不开的历史。合并遵守：同一**路径**的工作区以目标为准、只在 `sessionIds` 尾部补缺；`defaultWorkspaceId` / `archivedSessionIds` / `pinnedSessionIds` 一个都不动；目标文件**损坏时不做任何事**（绝不拿空骨架覆盖用户的清单）；写盘走 `atomic_write`。
- **其它目录一律不碰**：`profiles/` 是接线按本实例 `package.json` 重建的 pnpm 产物（源侧那棵可能连着别的实例的依赖），`logs/` 归壳，storages 里其它内容由内核按当前工作区现写，凭据是单文件、由工作台的凭据界面单独管理。
- **复制而不是搬家**：`recover_misplaced_home` 只写目标，**源永不删除**；目标已有的同名条目一律跳过（扫描与点击之间可能有另一个壳写进了同名会话，执行前再挡一次）。
- **要求目标实例的内核未在运行**（与 `restore` 同一条纪律）：内核把 storages 缓存在内存里，运行中合并进去的清单会被它下一次落盘整个覆盖，用户看到的是「点了没反应」。判据按**实例**（`instance::instance_kernel_running`：实例 pid 文件 + `pid_is_kernel` 活体校验，pid 文件缺失时改问该实例端口的监听者——`kernel::start_instance` 写 pid 是 `let _ =`，写失败不阻断启动，只认 pid 文件会漏检）而不是本壳工作台——用户自建实例在注册表分家后有意留在**两份**注册表里，另一个壳也可能正跑着它；pid 文件里记了启动方的壳模式，命令层拒绝时按它指明「去哪个壳里停」。**同一判据与同一份文案**由 `instance::instance_kernel_running_message` 提供，`recover_misplaced_home` 与 `snapshot_restore` 共用——两处各写一份文案必然漂移。
- **只收编确实到位的会话**：工作区清单里只写本实例**确实有会话目录**的 id。部分复制（磁盘满 / 权限 / 竞争）时 `available` 少于 `all`，照样收编工作区但只写 `available`——整条 `workspace.clone()` 会把没复制成功的 id 也写进去，工作台里那是一条点不开的历史，比不收编更糟。
- **部分失败照实报**：`MisplacedRecovery` 分 `copied` / `skipped` / `workspaces` / `failed` 四段，UI 逐条显示，不假装全成。

UI 挂在「数据迁移」面板的「找回历史会话」卡片（`MigrationPanel.vue`，进入面板时只读扫描一次）。它与迁移向导那 4 个源**不是一回事**：那套处理「上一代壳留下的旧布局目录」，带 backup + 整目录覆盖语义；会话不能那样搬——`BackupAndOverwrite` 会拿新数据换掉本实例已有的会话。

### 多实例隔离

- 插件物化按 `(family, instance_id)` 隔离：`extensions/plugins/<id>/` 在每个实例独立维护；其他实例由用户创建。
- **dev 壳与 release 壳各有各的默认实例**：release 用 `default`，dev 用 `default-dev`（`instance::default_instance_id_for`）。壳自己的数据目录早就分家了（`desktop/` 与 `desktop-dev/`），实例此前没有，于是两边共用同一棵 DSH home——`profiles/web/`（profile 接线）、`extensions/plugins/`（插件物化）、会话、凭据全是同一份。实测后果：dev 装完新内核重跑一次接线就把 release 正在跑的内核的 profile 换掉（`dev-plugin-wiring` 日志 11:19:02），dev 更新中央库里的插件源码（link 物化）release 内核立刻改用新代码——工作台当场抛 `scope '…' rendered without an installed adapter`，页面白屏。分家后两边的实例端口也分开（3090 / 3091），可以同时跑。
- **注册表的 `default_instance_id` 是共享的一份，只有 release 能把它指向某个实例**——它是 `data_dir` 族解析的输入（`instance::default_family`），被改会让另一个壳下次启动指向不相干的实例，且**不可自愈**：`ensure_default_registered` 的认领条件是「无人认领」，指针一旦被抢就再也轮不到它纠正。这条曾经真的坏过两处——一个前端从不调用的死命令 `ensure_default_instance_migrated`（把 id 写死成 `DEFAULT_INSTANCE_ID`，dev 壳走到它就往共享注册表里塞一条名为 `default`、端口取 dev 的 3091、`kernel_version` 为空的幽灵记录），和 UI 可达的 `set_default_instance`（直接 `registry.default_instance_id = Some(id)`，dev 壳在顶部点一下页签就把指针永久改成 `default-dev`）。两者都已改掉，前者连命令与权限条目一并删除。两条纪律写进了 `scripts/check-invariants.mjs` 第 10 / 11 项。
- **壳内「切到哪个实例」是壳自己的状态**：存在 `settings.current_instance_id`（`shell/<mode>/settings.json`，两个壳两份文件），读写走 `instance::current_instance_id` / `set_current_instance_id`，完全不经过注册表。`list_instances` 的 `is_default` 也按它判定。读取时会对本壳注册表做**成员校验**：指向已让位（对方默认实例）或已删实例的陈旧值回退到按壳分家的默认值——注册表分家前两边共用一份文件，release 壳可能选中过 `default-dev` 并存进自己的 settings，分家收敛后那个 id 已从本壳注册表消失，照走会把「找回历史会话」回收到另一个壳的实例里；注册表读不出来（损坏等瞬态）时保留选择——读失败不等于实例不存在。删除实例时若删掉的正是壳内选中的那个，回退到按壳分家的默认值，不让壳停在一个已不存在的实例上。
- **把旧状态吸收成默认实例只有一个入口**：`instance::ensure_default_registered`，由 `setup()` 同步调用。它按当前壳决定 id（`default` / `default-dev`），且只在「无人认领」时由 release 认领共享指针。
- **历史数据只进 release 实例**（`instance::legacy_migration_target`）：`~/.dsh` 的会话/凭据若跟着当前壳走，dev 先跑就会把 release 的历史搬进 `default-dev`。
- 实例正被**另一个壳**的内核占用时，装/卸/更新插件、切物化模式、切内核版本会被拒绝（`instance::ensure_instance_mutable`）。pid 文件里记了启动方的壳模式（第三段 `release` / `dev`，旧格式读不出时按"认不出"放行），活体判定复用 `kernel::pid_is_kernel` 而不是裸的进程存在性——pid 会被系统复用。
- 技能活动视图 v1 **全局共享**（`skills/active/`），但**接线是按实例写的**：`kernel_adapter::ensure_skill_wiring` 在每次 `prepare_instance`（即每次启动工作台）时向该实例的 `$DSH_HOME/cordis.patch.yml` 追加一条 `xlink-skill-filesystem` loader 行，把活动视图作为 `customSkillDirs` 交给内核。`KernelAdapter::custom_skill_dirs` 仍返回 `vec![skills_active_root()]`，`start` 也仍写 `DSH_CUSTOM_SKILL_DIRS`——但已装内核 0.2.0-rc.2 不读那个 env（`customSkillDirs` 只从插件配置读），env 只是留给未来内核版本的兜底。共享的是「活动视图」这一份数据，接线文件本身天然属于实例目录。取舍与验证见 [skill-management.md §「技能接线」](skill-management.md#技能接线壳怎么让内核看见活动视图)。
- 实例端口 / PID / 锁由 `crate::instance::InstanceRegistry` 集中管理；`DSH_XLINK_HOME` 与 `DSH_HOME` 不在同一进程级 mutex 下，但 `start_instance` / `stop_instance` 都通过 `lifecycle_mutex()` 串行化。

### 第二内核可扩展性（`KernelAdapter` trait）

`src-tauri/src/kernel/kernel_adapter.rs` 的 `KernelAdapter` trait + `adapters()` 注册表容纳多内核族：
- `DshAdapter`（active）—— DSH 的具体实现
- `McodeAdapter`（mock，P7 阶段）—— 所有物化 / 启动返回 `VersionNotInstalled`，能力位全空；证明通用实例模型能容纳第二种内核
- 真实接入新内核只需替换对应 adapter 的方法实现，**不**需要改 `KernelAdapter` trait 或 `adapters()` 注册表

`KERNEL_FAMILY_DSH = "dsh"` / `KERNEL_FAMILY_MCODE = "mcode"` —— 路径解析、端口分配、锁、日志归属都已按 family 分流。

### 实例列表与插件面板的 per-instance 视图（P8）

P8 在「已上线」与「即将发布」两个层面把多实例状态暴露给用户：

- **顶部内核标签**（`ui/src/components/KernelTabs.vue` + `ui/src/instance.js`）：
  标签栏挂在标题栏正下方、侧栏与内容区之上——先选内核，品牌 / 菜单 / 面板都归属当前 tab。每个内核一个 tab，只显示内核族名（DSH / mcode），注册表实例 id 不对外展示。调用 `set_default_instance(id)` 保存注册表默认项，按 id 排序，选择后不移动标签。列表读取复用在途请求，过期响应不能覆盖选择；读取失败保留已有列表并提供重试。切换和后续刷新共用互斥忙碌状态，失败释放状态并提示。
  此处尚未完成旧命令的实例化：`get_status`、`start_kernel` 等仍走单实例兼容路径，不能通过改写全局 settings / active.txt 冒充实例切换，否则会破坏目录隔离。
- **插件面板单 panel + 双 tab**（commit `9df8ed8` + `83186d2`；命名随 2026-09 IA 调整）：
  - 「当前内核」tab 沿用旧 entity-row 渲染（状态走 `PluginRow` legacy 字段），只做管理（同步 / 接线 / 模式切换 / 卸载），不带安装入口
  - 「已安装」tab 是本机插件库清单 + 获取入口：每个插件 + 每个实例一枚 chip（family · id · 状态），数据来自 `PluginRow.instances: BTreeMap<instance_id, PluginInstanceState>`；手动安装与插件中心也归这页——安装针对的是插件库，不属于某个内核
  - 后端 `status_for_instance` 内部枚举 `instance::load_registry()`，每个实例算一份 state 填进 map——单次 invoke 带回全实例状态，省去切 tab 再发请求的延迟
- **默认实例解析器**（commit `1053040` 等）：`instance::resolve_default()` 返回 `(&'static str, &'static str)` 元组（family + id），9 处 production caller + test fixture 全接入；后续 `InstanceRegistry::default_instance_id` 接管时 caller 自动跟进，无需再扫

写动作（enable / disable / 模式切换 / 卸载）只在「当前内核」tab 暴露；「已安装」tab 只显示 chip 与获取入口（手动安装 / 插件中心），不暴露 per-instance 写按钮——后端 enable_plugin / disable_plugin / set_plugin_mode 命令签名仍是单实例，per-instance 重构属于后续 PR。
