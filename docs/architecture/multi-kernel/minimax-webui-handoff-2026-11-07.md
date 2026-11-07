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

仍未验证：真实登录、登录层自动收尾与授权弹窗、目录选择、下载、剪贴板、重开窗口后的登录恢复；Windows 侧本轮改动**零编译验证**（本机 `--target x86_64-pc-windows-msvc` 会断在 `ring` 的 C 阶段），只能靠 CI 的 Windows job；Linux 只保证能编译。记忆真实注入、安装包、两平台 A01–A11 仍未完成。

## 2026-11-07 续做：记忆请求验证

隔离调试壳首轮发现记忆插件找不到摘要清单：无随包资源时，资源解析直接回退到没有清单的仓库源码。调试构建现会在源码前选择已生成且带清单的资源，显式覆盖仍优先，摘要校验保持不变。相同问题也影响依赖清单的 MiniMax Ponytail；本轮没有操作 Ponytail 账号或上游资源。

复跑使用真实 MiniMax `0.1.7` 和本机模型、审核桩，检查了授权记忆、未授权记录、撤销、项目不匹配及记忆开关；这些请求级子项与清理均通过。最终一轮还确认保存的模型地址、会话模型选择及 9 个物化插件文件的摘要。各轮运行标识、二进制摘要和证据目录见[接入计划](minimax-webui-integration-plan.md#8-当前证据与未完成项)。Rust 定向单测、Clippy、本机 release 条件编译、不变量与重复代码检查通过。此结果不等于真实模型、安装包或 Windows 验收。

## 2026-11-07 续做：登录弹窗收尾（只有定向测试）

只读核对候选包 `0.1.7` 的 `client/client.js` 后确认：登录页 `signIn()` 自己 `window.open` 一个标签页、再把它导航到设备登录地址，失败 / 取消路径由页面自己 `close()`，**认证成功路径不关**（`reservedTab` 只在 error / signed-out 分支被关）。所以上一轮补的重载只收起登录层，那扇授权页仍留在后面。

壳侧现在在重载之前先调新增的 `close_mcode_popups`（`src-tauri/src/harness/mcode-login.js` + `commands.rs`）：只关**本次 MiniMax 工作台**创建的弹窗，且不封 owner 的创建闸门，之后工作台的 `window.open`（终端链接等）照常；命令失败、ACL 未授或没有壳桥时最多等 300ms 就照常重载，不把登录收尾卡住。测试：`scripts/mcode-login.test.mjs`（10 项）。Windows（`x86_64-pc-windows-gnu`）本机 `cargo check --locked`、Clippy、不变量与重复代码检查通过。

**仍未实机验证**：登录成功后登录层与授权弹窗是否都消失、失败 / 取消 / 切换账号路径不得误关或误刷。

## 2026-11-07 续做：工作台窗口边界（只有定向测试）

1. **建窗限时**（`src-tauri/src/harness/mcode_window.rs`）：`open` 的等待从无界 `recv` 改为 20 秒 `recv_timeout`，与其它三扇独立窗口一致。超时返回可操作错误；建窗线程迟到的失败另落 `mcode-window` 日志，不再丢在线程里（超时后迟到的窗口仍会出现，重试会复用它）。
2. **顶层导航闸**：同一建窗链加 `on_navigation`，只放行本工作台回环 http(s) 与 `about:blank` / `blob:` / `data:`；外部 http(s) 交系统浏览器并落日志（只记来源，不记路径与授权参数），其它 scheme 拒绝。只读核对候选 `0.1.7` 的 `client/client.js`：包里没有 iframe，`location.href=` 只出现在预开弹窗上，页面自身路由不靠顶层导航，所以拦外链不影响登录与页面内部跳转。外部链接的打开路径收敛到 `harness_window::open_external_link` 一处，弹窗路径改调它。

判据：`cargo test --locked --lib harness::mcode_window` 4 项（导航路由、建窗接线、限时等待三态）、`cargo clippy --locked --all-targets -- -D warnings`、`npm run check:invariants`、`npm run check:duplicates` 通过（本机 `x86_64-pc-windows-gnu`）。**未实机验证**：页面内普通链接是否按预期交给系统浏览器、超时后迟到窗口的实际表现；完整浏览器交互仍未验收。

## 2026-11-07 续做：旧运行记录的人工清理（只有定向测试）

壳无法证明旧运行记录对应的进程树已全部退出：Job 句柄随上次壳进程消失，旧记录也没有可核对的代际标记；按计划不能仅凭 pid 消失或端口空闲自动删记录、认领或结束进程。这一轮补的是**人工出口**：

- `status` 新增 `stale_pid`（只在没有活动实例且记录存在时有值），概览卡据此显示「清理旧记录」。
- 新命令 `discard_mcode_run_record`：用户确认后，且**两个端口都没被占**时才删掉 pid 记录；只删记录，不结束任何进程、不认领端口，动作写 `mcode-lifecycle` 壳日志。判据不可证，因此**没有**接进自动恢复。

Rust 定向单测覆盖「端口被占时拒绝、清理后状态回到可启动、重复清理报错」；UI 写入沿用现有确认框与 loading。仍未实机验证真实壳崩溃后的整条恢复路径。

## 2026-11-07 续做：Windows 安装包本机构建（不是安装版验收）

本机已装 Visual Studio Community 2026（「使用 C++ 的桌面开发」+ Windows SDK 10），`link.exe` 可用；被中断的 `1.99.1-x86_64-pc-windows-msvc` Rust 工具链重装后正常。用 `tauri build --target x86_64-pc-windows-msvc` 从干净 HEAD `12fd2565` 生成 NSIS 安装包：

- `C:\Users\zxx\dsh-xlink-msvc-target\x86_64-pc-windows-msvc\release\bundle\nsis\dsh-xlink_0.7.5_x64-setup.exe`
- 8,162,903 字节，SHA-256 `7E34B700357C5B1127F01348704487EAFD9867E103D3ED7DB649F8551E097A66`，未签名（仓库本来也不做操作系统代码签名）。

本机构建把 `bundle.createUpdaterArtifacts` 临时置 false：本地没有更新签名私钥，CI 发布仍按原配置产出签名制品。`CARGO_TARGET_DIR` 指到 `C:\Users\zxx\dsh-xlink-msvc-target`（F 盘只剩约 4 GB）。复现：`$env:RUSTUP_TOOLCHAIN='1.99.1-x86_64-pc-windows-msvc'; $env:CARGO_TARGET_DIR='C:\Users\zxx\dsh-xlink-msvc-target'; npm run prep:builtin; npx tauri build --target x86_64-pc-windows-msvc --config '{"bundle":{"createUpdaterArtifacts":false}}'`。

这份产物只证明本机能出安装包，**不是安装版验收**：首次启动、受管安装、双内核、托盘退出、关闭/重开与卸载仍要在安装产物上按 A01–A11 执行。

## 接手后优先处理

1. **登录与窗口**：壳侧已补「认证后自动重载一次」与「重载前关掉授权弹窗」，**两者都待实机验证**：登录成功后登录层是否自动消失、授权弹窗是否真的被关、刷新后是否仍是已登录、登录失败或取消路径不得误刷、切换账号路径不得误刷。建窗限时与顶层导航闸已补（见上一节），同样待实机验证。窗口或服务重开后驱动读取登录状态曾超时，保持未验状态；会话、目录选择、下载、剪贴板和弹窗等外壳能力仍未验收。
2. **共享记忆与插件**：本机记录器已收到真实 MiniMax 运行时发送的 `<plugin-hook-context>`，授权和项目范围的部分子项已验证；真实模型请求仍为 `blocked`。下一步补 Windows、真实模型以及编辑、删除、并发写入和跨实例隔离证据。官网引用与自动记忆提取维持待接入：前者缺 MiniMax 引用菜单与发送门控接口，后者缺 MiniMax 转录适配、桥上的 jobs 路由与回合结束 hook。
3. **异常恢复**：旧协议遗留锁在持有者已死时可回收；旧无名 Job 与无法确认的运行/PID 记录**仍不自动恢复**（判据不可证），但现在有用户确认的人工出口：概览卡「清理旧记录」在两个端口都释放后只删记录、不结束进程（`discard_mcode_run_record`）。真实壳崩溃与完整进程树退出仍待验证。
4. **Windows 编译与安装包**：本机已能生成 NSIS 安装包（见上一节，本地构建、未签名）；MSVC 工具链可用后，本机也做了一次完整的 Windows release 链接。安装产物的 A01–A11 场景与 macOS 安装包仍未验收。其他平台由用户单独测试，不作为当前 Windows 工作的前置条件。

## 开发入口和操作约束

- 内核与进程：`src-tauri/src/kernel/kernel_adapter.rs`、`mcode_service.rs`、`mcode_install.rs`、`mcode_job.rs`；窗口：`src-tauri/src/harness/mcode_window.rs`。
- 技能、插件和记忆：`src-tauri/src/skills/mcode.rs`、`src-tauri/src/plugins/builtin/mcode_memory*.rs`、`mcode_ponytail*.rs`、`src-tauri/src/memory/share.rs` 和 `mcode_cmd.rs`。
- 管理页面：`ui/src/kernel/instance.js`、`mcode.js`，以及现有 `ui/src/memory/`、`plugins/`、`skills/`、`shell/` 页面。组件通过现有 store 和 `ui/src/shell/bridge.js` 调用后端。
- 真实运行必须使用独立的临时 `DSH_XLINK_HOME`、编译期 `app.appDirectoriesOverride`、独立通道和浏览器档案，证据放在临时 home 外。先读[开发验证](../../operations/development.md)、[运行时边界](../runtime-boundaries.md)和[磁盘布局](../kernel-data-layout.md)。不要修改已安装的 npm 包，也不要读取用户真实数据进行测试。
- 完成代码改动后按范围运行定向测试、`npm run check:invariants`、`npm run check:duplicates`，需要时执行 `npm run build:ui`、Cargo 格式、Clippy 和 release 编译检查。发布前的完整门禁仍按仓库规则执行。
- 原功能 worktree 中存在两份已暂存的旧实验页面草稿 `ui/src/kernel/McodeMemory.vue`、`ui/src/kernel/McodePanel.vue`，以及未暂存的 `ui/src/shell/pageIcons.js` 修改；它们不属于统一页面实现，交接时保留，不能无意带入提交或覆盖。继续按功能分批暂存和本地提交，不擅自推送或发布。
