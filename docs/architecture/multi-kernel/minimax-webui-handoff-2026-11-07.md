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

## 接手后优先处理

1. **登录与窗口**：MiniMax `0.1.7` 登录成功后，页面自己的登录层可能不自动关闭，可按 Esc。候选页面没有已确认的稳定关闭接口，不能直接把探针中的合成按键当成生产修复。窗口或服务重开后，驱动读取登录状态曾超时；保持未验状态，继续验证登录、会话及目录选择、下载、剪贴板和弹窗等外壳能力。
2. **共享记忆与插件**：当前桥已接线，但 A07 的真实模型注入验证仍为 `blocked`，并非已证明注入失败。候选包的基础工具禁用开关不能证明插件工具也关闭，且一次页面操作可能触发标题生成和模型重试；需先找到满足隔离要求的验证方式。官网引用和自动记忆提取仍标为待接入，不能把模拟测试或插件文件存在当成真实生效。
3. **异常恢复**：新锁已通过子进程终止测试，历史未知锁、旧无名 Job 以及无法确认的运行/PID 记录仍会阻止自动恢复；不能仅凭 PID 不存在或端口空闲就删除记录、认领或结束进程。真实壳崩溃与完整进程树退出仍待验证。
4. **Windows 安装包**：本机检查未找到 MSVC `link.exe`，正式安装包尚未生成。准备 Microsoft C++ 构建工具后使用仓库的 `npm run build:win`；GNU `cargo check --release` 只证明条件编译，不是安装包验证。最终候选提交需按计划执行 Windows A01–A11，逐项记录 `pass`、`fail`、`blocked` 或 `not_run`。其他平台由用户单独测试，不作为当前 Windows 工作的前置条件。

## 开发入口和操作约束

- 内核与进程：`src-tauri/src/kernel/kernel_adapter.rs`、`mcode_service.rs`、`mcode_install.rs`、`mcode_job.rs`；窗口：`src-tauri/src/harness/mcode_window.rs`。
- 技能、插件和记忆：`src-tauri/src/skills/mcode.rs`、`src-tauri/src/plugins/builtin/mcode_memory*.rs`、`mcode_ponytail*.rs`、`src-tauri/src/memory/share.rs` 和 `mcode_cmd.rs`。
- 管理页面：`ui/src/kernel/instance.js`、`mcode.js`，以及现有 `ui/src/memory/`、`plugins/`、`skills/`、`shell/` 页面。组件通过现有 store 和 `ui/src/shell/bridge.js` 调用后端。
- 真实运行必须使用独立的临时 `DSH_XLINK_HOME`、编译期 `app.appDirectoriesOverride`、独立通道和浏览器档案，证据放在临时 home 外。先读[开发验证](../../operations/development.md)、[运行时边界](../runtime-boundaries.md)和[磁盘布局](../kernel-data-layout.md)。不要修改已安装的 npm 包，也不要读取用户真实数据进行测试。
- 完成代码改动后按范围运行定向测试、`npm run check:invariants`、`npm run check:duplicates`，需要时执行 `npm run build:ui`、Cargo 格式、Clippy 和 release 编译检查。发布前的完整门禁仍按仓库规则执行。
- 原功能 worktree 中存在两份已暂存的旧实验页面草稿 `ui/src/kernel/McodeMemory.vue`、`ui/src/kernel/McodePanel.vue`，以及未暂存的 `ui/src/shell/pageIcons.js` 修改；它们不属于统一页面实现，交接时保留，不能无意带入提交或覆盖。继续按功能分批暂存和本地提交，不擅自推送或发布。
