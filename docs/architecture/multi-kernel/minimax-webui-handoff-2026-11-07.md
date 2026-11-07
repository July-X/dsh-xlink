# MiniMax 统一工作台开发交接

交接日期：2026-11-07。本文记录当前开发快照；完整验收口径、失败与成功证据以 [MiniMax 接入计划](minimax-webui-integration-plan.md)为准。功能仍为实验入口，不能视为已经完成发布验收。

## 目标与现状

- dsh-xlink 使用同一个管理窗口，通过顶部 DSH / MiniMax tab 切换管理对象。两套内核可以同时运行、分别停止；切换 tab 不应改变另一工作台的运行状态。
- 复用概览、内核版本、技能、插件、记忆、设置、用量和日志页面；包、实例、账号、端口、工作台和浏览器档案按内核区分。MiniMax 实例不会自动获取 DSH 的历史记忆，跨内核使用需逐条授权。
- 入口只在构建时设置 `VITE_MCODE_EXPERIMENTAL=1` 后显示，常规构建隐藏。目前仅支持 `@minimax-webui/minimax-code@0.1.7`；Windows 首轮组合为 Node `26.12.0`。
- 功能分支的开发快照为 `eaea0d10`；合并前本地主干为 `e82d5f2b`、版本为 `0.7.5`。这些是交接时的历史定位，不是对合并后提交或安装包的验收结论。

## 已实现及已验证

已接入顶部切换、各内核的概览和版本管理、MiniMax 外部包检测及受管安装、独立窗口与进程、共享技能视图、Ponytail、逐条授权的受限记忆桥、共用设置和按来源筛选的日志。受管记忆插件有归属检查、升级事务和备份；Windows 新生命周期锁在持锁进程终止后由系统释放。无法确认归属的旧插件目录、旧锁和运行记录仍保守保留。

Windows 隔离**开发构建**已验证受管安装、两个内核同时运行和分别停止；用户已在隔离窗口登录，服务停止后重新启动时登录状态仍可识别，关窗重开及服务再次启停的子项也通过。技能与生命周期测试 12 项、记忆插件测试 16 项及 Ponytail 测试 7 项通过；Clippy、release 条件编译、不变量和重复代码检查通过。跨进程测试验证了持锁测试进程终止后，记忆升级事务可经生产 `prepare` 入口恢复；事务中断状态由夹具构造，未强制终止真实工作台。

这些结果来自当时的开发构建或测试夹具，不是合并后候选提交的安装包验收。各轮次 ID、二进制摘要、未通过的步骤和证据目录见[计划中的当前证据](minimax-webui-integration-plan.md#8-当前证据与未完成项)；本机临时证据不属于仓库交付物。

## 主干合并候选验证

合并验证工作区以 `0.7.5` 主干为基础，合并提交为 `9682f818`。随后修正了实验页签相关的旧界面断言，以及退出确认可能被内核环境探测拖慢的问题：状态不明时仍保守要求停止，原生兜底从退出请求发起时计时。

Windows 源码回归中，完整 UI 测试、插件测试、相关脚本测试、MiniMax Rust 测试 63 项、主干插件预检 15 项及 IPC 契约 6 项通过；默认和实验两种 UI 构建、Clippy、release 条件编译、不变量与重复代码检查通过。未生成正式安装包，也未在合并候选上重新执行隔离工作台实机验收。

## 本轮新增改动（2026-11-07 续，全部未做实机验收）

按交接文档的优先顺序补了三处代码，每处都带定向测试；**Windows 安装包仍未生成**，三处改动都还没有在真机工作台上验过。

1. **登录层自动收起**（`src-tauri/src/harness/mcode-login.js`，挂在 MiniMax 建窗链上）。只读核对候选包 `0.1.7` 的 `client/client.js` 后确认根因是上游缺陷：登录成功后页面的 `onAuthenticated` 只刷新用量与账号，没有关掉自己的登录层；而登录层的开关 `loginOpen` 初值是 `false`（只在用户点「登录 / 切换账号」时打开），所以**重载一次页面它就不会再出现**。壳侧因此在观测到「登录层从待登录切到已认证」时重载一次工作台页面。**不用合成按键**：Esc 只是恰好触发页面自己的关闭路由，候选页面没有稳定接口。判据用页面自己的 `data-webui-account-login-phase` 属性；一分钟内最多重载一次，防止页面自己再开登录层时变成刷新循环。测试：`scripts/mcode-login.test.mjs`（5 项）。
2. **记忆桥留痕**（`src-tauri/src/plugins/builtin/mcode_memory.rs`）。桥这一跳此前**零留痕**——「注入成功」与「hook 静默失败」在壳里长得一模一样，A07 没法判。现在每次响应写一条 `mcode-memory` 壳日志（记录条数与字节数，**提问与记忆正文不落盘**）。这只证明「桥 → hook」这一跳，**不等于**真实模型请求收到了内容。
3. **旧锁回收**（`src-tauri/src/skills/mcode.rs`）。旧协议的锁是「普通文件 + 持有者 pid」，持有者崩了它就留在盘上，新协议从此拿不到租约，MiniMax 起不来。现在只在**持有者确实已经死了**时回收它，判据复用技能中央库的 `store_lock::owner_is_dead`（读不出 pid、持有者还活着都按活着算）；回收动作写 `mcode-lifecycle` 壳日志。旧无名 Job 与无法确认归属的运行/PID 记录**仍未解决**。

## 2026-11-07 续做：Unix 生命周期与共用页面（全部只有定向测试与一次 macOS 冒烟）

这一轮把「共享能力只挂在 Windows」的限制拆掉了，并按用户要求收敛共用页面：

1. **Unix 子进程树可验证**（`src-tauri/src/kernel/mcode_tree.rs`）。Windows 靠 Job Object 覆盖整棵树；Unix 没有等价物，而 MiniMax 会派生会 `setsid()` 的 hook / 终端。做法是周期性进程表快照（macOS 走 `proc_listallpids` + `proc_pidinfo`，其它 Unix 走 `ps`），把「当前仍是根进程后代」的 pid 连同启动时间记进名册；停止时先进程组 TERM→KILL，再逐个收尾**身份匹配**的残留后代，最后终局扫描确认「已知 pid 全部不在 + 表里没有根的后代」。三种结论与仓库既有 `GroupStopOutcome` 一致，**查不到结论就报 Unknown，不假装已停止**。macOS 的启动禁令与 `startup_blocked_reason` 因此删除。
2. **技能 / Ponytail / 记忆桥改成跨平台**。技能实例租约在 Unix 上用 `flock`（进程崩溃由内核放锁，锁文件按内容 pid 判归属、只删自己那份）；Ponytail 与记忆桥去掉 Windows 专属门控，凭据写入在 Unix 侧沿用 `atomic_write_private`（0600）；四份「排他改名」实现收敛到 `shell/atomic_write.rs::rename_exclusive` 一份（Windows MoveFileExW / macOS renamex_np / Linux renameat2）。`mcode_service` 里技能、记忆、Ponytail 的接线不再带平台门控。
3. **共用页面按用户要求收敛（进行中）**：用量分析改成「一个页面 + 套餐卡常驻 + 模型用量卡壳共用、内容按内核变」；设置页工作台卡改成一张共用卡，端口字段按内核分叉，MiniMax 的两个端口现在可编辑（新增 `save_mcode_ports`，运行中/占用/重复端口拒绝且不改配置）；内核版本页的共用化见计划进度表。

**Intel macOS 首次真实证据**：隔离生产链 smoke `e2e-20261107050747-89179-3cc5b7` 通过——真实壳 + 真实 `@minimax-webui/minimax-code@0.1.7`、Node v25.9.0，检测/启动/双回环端口归属/独立窗口/切 tab 服务仍在/停止后端口释放，`cleanup: pass`。另有隔离探针 smoke `e2e-20261107044641-72782-3ef552` 通过（首页 200、同源 WebSocket 101、异源 403、WebView 渲染并连接、停止后端口释放）。

仍未验证：真实登录与登录层自动收尾、目录选择、下载、剪贴板、重开窗口后的登录恢复；Windows 侧本轮改动**零编译验证**（本机 `--target x86_64-pc-windows-msvc` 会断在 `ring` 的 C 阶段），只能靠 CI 的 Windows job；Linux 只保证能编译。记忆真实注入、安装包、两平台 A01–A11 仍未完成。

## 接手后优先处理

1. **登录与窗口**：壳侧已补「认证后自动重载一次」，**待实机验证**：登录成功后登录层是否自动消失、刷新后是否仍是已登录、登录失败或取消路径不得误刷、切换账号路径不得误刷。登录用的授权弹窗在上游也不自动关闭，重载后仍留着，未处理。窗口或服务重开后驱动读取登录状态曾超时，保持未验状态；会话、目录选择、下载、剪贴板和弹窗等外壳能力仍未验收。
2. **共享记忆与插件**：壳侧已补桥的留痕，A07 的第一步（把「没注入」与「注入了没记」分开）达成；真实模型请求仍为 `blocked`。更精确的阻塞点：候选包的环境开关只清空**基础工具**，管不到回合结束的标题生成那次独立请求；`xlink-memory` 本身不声明任何工具，所以「插件工具是否也关掉」对本插件是空命题。下一步是运行期把 provider 指向本机记录器，抓承载用户提示词的那次请求体断言 `<plugin-hook-context>`。官网引用与自动记忆提取维持待接入：前者缺 MiniMax 引用菜单与发送门控接口，后者缺 MiniMax 转录适配、桥上的 jobs 路由与回合结束 hook。
3. **异常恢复**：旧协议遗留锁在持有者已死时可回收；旧无名 Job 与无法确认的运行/PID 记录仍会阻止自动恢复，不能仅凭 PID 不存在或端口空闲就删除记录、认领或结束进程。真实壳崩溃与完整进程树退出仍待验证。
4. **Windows 编译与安装包**：本轮改了 Windows 上也会编译的代码（去门控、新增分支、`save_mcode_ports`），本机无法做 Windows 类型检查，**先让 CI 的 Windows job 编过再谈安装包**。`本机检查仍未找到 MSVC `link.exe`，正式安装包尚未生成。准备 Microsoft C++ 构建工具后使用仓库的 `npm run build:win`；GNU `cargo check --release` 只证明条件编译，不是安装包验证。最终候选提交需按计划执行 Windows A01–A11，逐项记录 `pass`、`fail`、`blocked` 或 `not_run`。其他平台由用户单独测试，不作为当前 Windows 工作的前置条件。

## 开发入口和操作约束

- 内核与进程：`src-tauri/src/kernel/kernel_adapter.rs`、`mcode_service.rs`、`mcode_install.rs`、`mcode_job.rs`；窗口：`src-tauri/src/harness/mcode_window.rs`。
- 技能、插件和记忆：`src-tauri/src/skills/mcode.rs`、`src-tauri/src/plugins/builtin/mcode_memory*.rs`、`mcode_ponytail*.rs`、`src-tauri/src/memory/share.rs` 和 `mcode_cmd.rs`。
- 管理页面：`ui/src/kernel/instance.js`、`mcode.js`，以及现有 `ui/src/memory/`、`plugins/`、`skills/`、`shell/` 页面。组件通过现有 store 和 `ui/src/shell/bridge.js` 调用后端。
- 真实运行必须使用独立的临时 `DSH_XLINK_HOME`、编译期 `app.appDirectoriesOverride`、独立通道和浏览器档案，证据放在临时 home 外。先读[开发验证](../../operations/development.md)、[运行时边界](../runtime-boundaries.md)和[磁盘布局](../kernel-data-layout.md)。不要修改已安装的 npm 包，也不要读取用户真实数据进行测试。
- 完成代码改动后按范围运行定向测试、`npm run check:invariants`、`npm run check:duplicates`，需要时执行 `npm run build:ui`、Cargo 格式、Clippy 和 release 编译检查。发布前的完整门禁仍按仓库规则执行。
- 原功能 worktree 中存在两份已暂存的旧实验页面草稿 `ui/src/kernel/McodeMemory.vue`、`ui/src/kernel/McodePanel.vue`，以及未暂存的 `ui/src/shell/pageIcons.js` 修改；它们不属于统一页面实现，交接时保留，不能无意带入提交或覆盖。继续按功能分批暂存和本地提交，不擅自推送或发布。
