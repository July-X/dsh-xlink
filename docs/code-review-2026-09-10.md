# dsh-xlink 代码审查报告

审查对象：`dsh-xlink@0.1.2-rc.18`（HEAD `9b3c181`，`main` 分支，工作区干净）。
审查日期：2026-09-10。

## 修复进度

按「建议的处理顺序」逐条修复。每条修复都要求：改代码 → `cargo test` / `cargo clippy -D warnings` / `cargo fmt --check` / `npm run test:ui` 全绿 → 补回归测试。

| 编号 | 问题 | 状态 | 说明 |
| --- | --- | --- | --- |
| P0-1 | 内核运行中改端口致失联与重复启动 | ✅ 已修 | 判据去端口耦合；新增 3 个回归测试 |
| P0-2 | `install_node` 未授权 | ✅ 已修 | 补白名单 + 新增 `scripts/check-invariants.mjs` 并接入 CI |
| P0-3 | copy 模式技能卸载失效 | ✅ 已修 | 所有权改为"链接或内容指纹"；Windows 目录改 junction；新增 3 个回归测试 |
| P1-5 | `confirm_close_shell` 漏关 `official-chat` 窗口 | ✅ 已修 | 改用 `get_window`（裸窗口拿不到 `get_webview_window`） |
| P1-11 | `install_shell_update` 在 async 里调同步安装 | ✅ 已修 | `Update::install` 移入 `spawn_blocking` |
| P1-15 | `load_store` 解析失败静默回落空清单 | ✅ 已修 | 引入 `process::StateRead` 二分语义 + `load_store_checked`；写路径与清扫路径拒绝执行 |
| P1-23 | `state.json` / `store.json` 损坏被当成空 | ✅ 已修 | 同一机制覆盖 plugins / skills / patches 三处；新增 3 个回归测试（195 → 198） |
| P1-26 | 并发发布防护失灵（tag 与 dispatch 不同组） | ✅ 已修 | `concurrency.group` 改为固定 `desktop-release`，串行化全部发布 |
| P1-27 | updater 端点无单调性保证 | ✅ 已修 | preflight 断言新版本 > 线上 `latest.json` 版本（已用 6 组版本对验证比较逻辑）；publish 末尾加端到端校验 |
| P1-28 | actions 用可变 tag + 私钥在 env | ✅ 已修 | 15 处 `uses:` 全部 pin 到 40 位 commit SHA（附版本注释） |
| P1-1 | 内核子进程 PATH 缺 node 目录 | ✅ 已修 | 新增 `command_with_path_dirs`，`kernel::start` 前置 node 目录；plugins 两条 pnpm 主路径补托管 node 目录 |
| P1-2 | 远端版本号未校验就拼进路径 / JSON | ✅ 已修 | 新增 `version::is_valid_kernel_version`（三条来源共用 + 两个命令边界 + stub 改 `serde_json`） |
| P1-3 | guard 把零证据失败升级为安全模式 | ✅ 已修 | 新增 `BootVerdict::SpawnFailed`；内核没起来时不做归因、不重试、不进安全模式 |
| P1-4 | 子进程退出后仍等满 30 分钟 | ✅ 已修 | 退出后 5 秒宽限 + 500ms 轮询；本轮新增 4 个回归测试（198 → 202） |
| P1-12 | copy 模式物化短路失效（每次启动整树重拷） | ✅ 已修 | `KernelMeta` 新增 `fallback`；按记录的实际形态判定健康度；`set_mode` 删 meta 强制重新物化 |
| P1-19 | 检查更新错误被吞 + TTL 误推进 | ✅ 已修 | 抽 `should_advance_update_check`；UI 展示逐包错误并与后端同步 TTL（204 个测试） |
| P1-29 | README/troubleshooting 端口 3080 与代码矛盾 | ✅ 已修 | 5 处改为 3090/3091 并补充"运行期间不能改端口" |
| P1-6 | gzip 炸弹绕过响应体上限 | ✅ 已修 | 关闭 ureq 的 `gzip` 特性（不声明 Accept-Encoding），上限恢复为对实际字节生效 |
| P1-7 | updater 无超时，挂起端点锁死面板 | ✅ 已修 | `updater_builder().configure_client(connect 15s + read 30/60s)` |
| P1-10 | Atom 回退把所有条目标成非预发布 | ✅ 已修 | `release_from_tag` 内部叠加 `version.contains('-')` |
| P1-18 | `git clone` 走 30 秒硬超时 | ✅ 已修 | 超时参数化 + `GIT_CLONE_TIMEOUT`(600s) + 中文可操作提示；新增 releases 测试（204 → 206） |
| P1-17 | `copy_tree` 跟随仓库内 symlink 且无根约束 | ✅ 已修 | plugins 侧加源根约束（越界链接跳过）；skills 侧跳过全部链接（与扫描器语义一致）；新增测试（206 → 207） |
| P1-9 | 失败安装留下半成品内核目录 | ✅ 已修 | `install_version` 包装：全新安装失败即删除该目录（重装场景保留残骸） |
| P1-22 | 多补丁改同一文件无所有权校验 | ✅ 已修 | apply 的计划阶段加文件级占用检查，占用者名字进错误信息；两条测试均经反证（207 → 209） |
| P1-8 | 外壳下载的 tarball 无完整性校验 | ✅ 已修 | `NpmDist` 反序列化 `integrity`；新增 `releases::verify_download_integrity`（SRI sha512/sha256 + 零依赖 base64），下载后即校验、不符即拒绝并删除 |
| P1-16 | npm 候选回退吞掉"已记账"的失败 | ✅ 已修 | 只在取源阶段失败时继续下一个候选；已写 store 行则中止并给出收拾办法（210 个测试） |
| P1-13 | copy 模式更新后不重跑 profile 安装 | ✅ 已修 | copy 模式更新后无条件补跑一次幂等的 `run_profile_install` |
| P1-14 | `refresh_store_peers` 跳过"指向旧内核"的链接 | ✅ 已修 | 按 canonicalize 比对判定"已正确解析"，指向别处则重链；两条测试均经反证（209 → 211） |
| P2-46 | draft 复用可能留下多余资产 | 🚧 部分修复 | publish 末尾读回真实资产集合并断言恰好 6 个（检测并阻断；未自动删除多余资产） |
| P1-20 | apply 半失败留下半补丁内核 | ✅ 已修 | 改两阶段（先全量校验、再统一备份写入），写入失败按本次备份整体回滚 |
| P1-21 | revert 不可重入导致撤销卡死 | ✅ 已修 | 逐文件持久化进度 + 记录 `originalSha256` 以识别"已还原"；新增 2 个回归测试（193 → 195） |
| P2-13 | `search` 缺省值会损坏整文件 | ✅ 已修 | 顺手在 `plan_file` 中拒绝空 `search` |
| P1-24 | `v-loading` 指令未注册 | ✅ 已修 | `main.js` 注册 `ElLoading` + 引入其样式（打包体积 422 kB / 137 kB，均在预算内） |
| P1-25 | 进度浮层被 Element Plus 弹层压住 | ✅ 已修 | `.progress-overlay` z-index 60 → 3000；事故面板两个动作按钮补 `:loading` / `:disabled` |
| P2-25 | `get_kernel_log` 死命令 | ✅ 已修 | 实现、注册、白名单三处一并删除（注册命令 45 → 44） |
| P2-26 | `node_cache` 用裸 `lock()` | ✅ 已修 | 改用 `crate::lock`，锁被毒化时也清缓存 |
| P2-29 | `stop_kernel` 提前返回跳过 `clear_pid` | ✅ 已修 | pid 记录无条件清理，停止失败仍如实上报 |
| 【已修】P2-45 | CI 缺少测试入口 | 🚧 部分修复 | 已接入 `check:invariants` / `test:scripts`（`scripts/*.test.mjs` 全量，10 条）/ `smoke-pullstring`；`verify-*.mjs` 的版本门仍待处理（见 P2-47） | ✅ 已修：补上 `test:file-perf` 入口；`test:scripts` 把 `scripts/*.test.mjs` 全量接入 CI（早前已完成）。三个 verify 脚本需要真实内核才能跑，因此不进 CI，而是通过 P2-47 的版本门做到"在有内核的开发机上不会假失败"
| P2-49 / P2-50 | 技能文档与实现不一致 | ✅ 已修 | 随 P0-3 更新 `docs/skill-management.md` |
| P2-53 | `install.mjs` 丢弃子进程输出 | ✅ 已修 | `stdio: 'inherit'` + 失败原因不再退化成「退出码 ?」；新增 3 条反证过的测试与 `test:scripts` CI 入口 |
| P2-54 ~ P2-61 | 注释与文档一致性（8 条） | ✅ 已修 | 逐条对齐实现：strip 高度、junction、copy 重同步语义、hash 锚定、校验时机、两处启动期文案 |
| P2-1 / P2-5 / P2-8 | 僵尸句柄、netstat 子串匹配、tail 竞速 | ✅ 已修 | 见明细；共 9 条新测试，全部经反证 |
| P2-4 | 轮转日志在面板中不可见 | ✅ 已修 | 备份改名 `X.1.log`（保留 `.log` 扩展名）+ 列举排序改「新 → 旧」+ 启动期迁移旧命名；新增 9 条测试，三项反证通过 |
| P2-6 / P2-7 | 端口身份误判、归因 needle 误伤短插件名 | ✅ 已修 | `ListenerIdentity` 三态 + `open_harness` 收口；归因改锚定路径段匹配（`main` 不再命中 `main-utils`） |
| 【已修】P2-62 | logs 目录无保留策略 | ⏳ 待修 | 每天最多新增 24 MiB，日期只增不减；历史注释引用的 `cleanup_legacy_logs` 已不存在 | ✅ 已修：新增 `process::prune_old_logs`，启动时按「保留 30 天 / 总量 200 MiB、从最旧的开始丢」裁剪 logs 目录，10 分钟宽限期内的文件（多半是当前会话正在追加的日志）永不删除；`lib.rs` 接线并在删除时打一行 stderr。新增 4 条测试（天数、总量、宽限期、非 `*.log` 与目录缺失），两项反证命中
| **P1（全部 29 条）** | | ✅ **已完成** | |
| P2-10 / P2-11 / P2-16 / P2-22 | 补丁与安装的越界与承诺不一致 | ✅ 已修 | 见明细；共 6 条新测试 |
| P2-12 / P2-19 | 坏清单/坏设置无声消失或回退 | ✅ 已修 | 补丁加载告警并入 `PatchStatus.warning`；设置损坏备份 + `settings_warning` 上报面板；新增 3 条测试 |
| P2-9 / P2-14 / P2-15 | 撤销假成功、孤儿记录不可见、.pnpm 布局无法打补丁 | ✅ 已修 | 见明细；新增 4 条测试（含 1 条改写），三项反证通过 |
| P2-3 | 日志写入失败被吞、排空线程退出 | ✅ 已修 | `drain_stream` 失败继续排空 + 诊断入 trail；新增 3 条测试，反证通过 |
| P2-17 / P2-18 | 托管 Node 的残留/无法恢复/平台错配 | ✅ 已修 | 清扫全部遗留临时目录 + 发布前清残缺目录 + 装完删包；产物按 OS/ARCH 精确匹配并带真实诊断；新增 2 条测试 |
| P2-24 | 插件 id 映射非单射导致互相覆盖 | 🚧 部分修复 | 复用同一 id 且来源不同时拒绝并提示先卸载；换 id 方案需迁移既有安装，留作独立项 |
| P2-20 / P2-21 | 孤儿插件目录无人管、同名接线互相覆盖 | ✅ 已修 | reconcile 清理"有标记无记录"的目录（跳过本轮刚恢复的 id）；接线改以 id 为键并上报同名冲突；新增 4 条测试 |
| P2-27 / P2-30 / P2-31 / P2-35 / P2-37 / P2-41 / P2-42 | 锁粒度、死字段、版本不一致、UI 约定漂移 | ✅ 已修 | 见明细；版本一致性同时进 preflight 与 check-invariants（反证命中） |
| P2-45 / P2-47 / P2-48 / P2-52 | CI 入口缺失、verify 脚本假失败、坏徽章、无 push/PR 门禁 | ✅ 已修 | 新增 desktop-ci.yml；两个 verify 脚本加版本门 + try/catch；补 test:file-perf；修徽章 |
| P2-32 ~ P2-34 | registry 信任边界口径、releases 无测试/无缓存 | ✅ 已修 | AGENTS 补镜像与 SRI 说明；`select_releases` + 6 条测试（空 npm 结果改为回退）；60s TTL 缓存 |
| P2-28 / P2-62 | 日志窗口静默失败、logs 无保留策略 | ✅ 已修 | 校验合并 + 异步开窗回传错误；启动裁剪 30 天 / 200 MiB（10 分钟宽限）；新增 5 条测试 |
| P2-51 | 发布流水线不校验签名密钥是否成对 | ✅ 已修 | 新增 check-signing-keys（Ed25519 验签 + key id 比对），build job 打包前执行；5 条测试 |
| P2-38 ~ P2-40 / P2-43 / P2-44 | 日志重入被吞、窗口操作静默失败、无渲染兜底、两处文档漂移 | ✅ 已修 | 见明细；UI 测试 14 → 16 条 |
| P2-2、P2-36、P2-46 | | ⏳ 待修 | |

**当前基线**：`cargo test` 269 通过 / 0 失败 / 1 忽略；`cargo clippy --all-targets -- -D warnings` 零警告；`cargo fmt --check` 通过；`npm run test:ui` 16 通过；`npm run test:scripts` 15 通过；`node scripts/smoke-pullstring.mjs` exit 0；`npm run check:invariants` 通过。

> 本文件同时是**问题清单**与**修复台账**：正文条目保留原始分析（含 `file:line`、触发场景、影响、建议），修复完成后在对应条目标题前加【已修】并在此表登记。

## 审查范围与方法

**覆盖面**：`src-tauri/src/` 全部 21 个 Rust 模块（约 19k 行）、`ui/src/` 全部 27 个文件（含 2811 行的 `theme.css`）、`scripts/` 11 个脚本、`.github/workflows/desktop-release.yml`、`docs/` 9 篇文档、能力与权限清单。

**方法**：模块级全文精读（6 路并行深度审查）+ 跨模块交叉验证 + 机器可验证检查。下表是本次实际执行过的检查，可作为结论的可信度基线：

| 检查 | 命令 / 依据 | 结果 |
| --- | --- | --- |
| Rust lint 基线 | `cargo clippy --all-targets` | 0 警告（符合 AGENTS.md 基线） |
| Rust 测试 | `cargo test` | **187 通过 / 0 失败 / 1 忽略** |
| 前端测试 | `npm run test:ui` | **14 通过 / 0 失败** |
| 前端构建 | `npm run build:ui` | 绿（949 modules） |
| 命令注册 ↔ 授权白名单 | 脚本比对 `lib.rs` 与 `permissions/app-commands.json` | **发现 1 处越权（P0-2）** |
| UI invoke → 命令 → 授权 三方交叉 | 脚本比对 UI/注入脚本中的 41 个命令名 | 40 项正确，1 项被 ACL 拒绝 |
| capability → 权限标识引用 | 脚本校验 5 个 capability | 全部有效 |
| Tauri ACL 判定语义 | 读 `tauri-2.11.5/src/webview/mod.rs:1794-1853` | 白名单制，确认 P0-2 成立 |
| `get_webview_window` 语义 | 读 `tauri-2.11.5/src/lib.rs:576-585` | 裸窗口返回 `None`，确认 P1-5 成立 |
| ureq `limit` 与解压顺序 | 读 `ureq-3.4.0/src/body/mod.rs:744-775` | `LimitReader` 在解压器内层，确认 gzip 炸弹成立 |
| 测试入口 ↔ CI 覆盖 | 脚本比对 `package.json` 与 workflow | **3 个测试入口未进 CI（P2-31）** |

标注 ✅ 的条目由我逐行复核过代码；其余条目来自模块级深度审查，同样定位到具体行号。

---

## 总体评价

工程质量明显高于同规模桌面项目的平均线，以下几点是本仓库的真实资产，改动时不应破坏：

- **ACL 设计正确且克制**。本地面板走 `allow-local-commands` 白名单；远程 webview（工作台 loopback、官方对话三站点）只拿到逐个点名的两条 / 零条命令；每个 capability 的 description 都写清了"为什么这样授"。远端页面即使被 XSS 也几乎触达不到 shell 能力面。
- **零 XSS 面**：`ui/src` 中不存在 `v-html` / `innerHTML` / `eval` / `new Function` / `postMessage`；远程来源字符串全部走文本插值。
- **IPC 单一出口**：`bridge.js` 是唯一的 `core.invoke` 入口（仅 2 处 `window.__TAURI__.window` 例外）。
- **原子写贯彻到位**：`process::atomic_write`（`process.rs:40-104`）用同目录 `create_new` 临时文件 + `write_all` + `sync_all` + rename + 父目录 fsync，settings/store/state/patch 全部经它落盘。
- **归档解包安全**：`archive.rs` 与 `node_install.rs` 拒绝越界路径、符号链接、硬链接、特殊文件，限 10 万条目 / 512 MiB 展开体积，强制唯一根目录。
- **进程回收有防护**：`pid_is_kernel`（`kernel.rs:1384-1428`）三层校验后才发信号，未发现误杀无关进程的路径。
- **命令层纪律**：涉及进程/网络/目录树的命令全部 `async` + `spawn_blocking`；`lifecycle` 锁只在 blocking worker 内获取，不跨 `await` 持锁；`lifecycle → store` 锁序全局一致，**无 ABBA 死锁，`lock_store()` 的重入路径经专项核对确认不存在**。
- **测试质量**：187 个 Rust 测试覆盖了补丁应用/撤销、技能物化与对账、插件清扫、guard 归因、原子写、PATH 合并等高风险路径。

---

## P0 —— 阻断性，建议立即处理（3 条，按后果严重度排序）

### 【已修】✅ P0-1 内核运行中修改端口 → 内核彻底脱离管理，并可被重复启动，直接损坏会话日志

**修复方式**（`src-tauri/src/kernel.rs`、`guard.rs`、`commands.rs`、`patches.rs`、`lib.rs`）：

1. 新增 `kernel::workbench_pid(data_dir, settings)` / `workbench_running(...)`，作为**唯一**的"工作台是否在运行"判据，证据链为：`kernel.pid` + `pid_is_kernel(pid, None)` 的实时身份与存活校验；仅在 pid 记录缺失/失效时才回落到"配置端口上的监听者，且监听者通过内核身份校验"。由于 `pid_is_kernel` 每次都重新查询进程是否存在，僵尸检测与存活检测合并成了同一件事。
2. `pid_is_kernel` 增加 `None` 语义说明：传 `None` 表示调用方已持有更强证据（pid 文件属于本 data dir、或已按 cwd 精确匹配），此时不再拿会随用户设置变化的端口当身份的一部分。
3. 全部判活点统一改判据：`kernel::status().running`、`ensure_workbench_stopped`（内核版本切换）、`guard::guarded_start` 的幂等预检、`commands::stop_kernel`、`commands::open_harness` 的诊断、`patches::ensure_workbench_stopped`（补丁应用/撤销）、`lib.rs` 的退出回收与关窗判断。
4. `kernel::reap_orphans` 改为 `kill_pid(pid, None)`：cwd 等于本 data dir 已是比端口更强的身份证据，继续用配置端口校验 `--port` 会让改过端口的内核恰好逃过回收。
5. `commands::save_settings` 增加守卫：`port` 变化且工作台在运行时拒绝保存，并给出可操作提示（"请先点击「关闭工作台」停止工作台，再回来保存设置"）。这从源头堵住了"内核跑在旧端口、配置已是新端口"的分裂状态。
6. `kernel::start_maybe`：端口有监听者但**不是**本 data dir 的内核时返回明确的端口冲突错误，不再静默 `Ok(None)`（原行为会让「启动工作台」报告成功，而工作台窗口打开的是别人的服务）。这同时消掉了 P2-17。
7. `lib.rs::kernel_running`：内存句柄只在 `try_wait()` 确认进程仍存活时才算"在跑"，失效句柄顺手清空（消掉 P2-1 的一半）。

**回归测试**（`kernel.rs::tests`，3 个）：
- `unrelated_listener_on_the_port_is_not_a_running_workbench`：无关进程占用端口时，`workbench_running` / `status().running` 均为 false，且 `start_maybe` 报"已被其它进程占用"。
- `stale_pid_record_is_not_a_running_workbench`：pid 记录指向不存在的进程时判为未运行。
- `switching_active_version_is_allowed_when_only_an_unrelated_listener_holds_the_port`：反向断言，无关监听者不再阻止切换内核版本。
- 既有测试 `refuses_to_change_active_version_while_workbench_is_serving` 已按新判据重写（用命令行带内核标识的占位进程模拟内核，而不是"起一个 TcpListener"）。

**验证**：`cargo test` 190 通过 / 0 失败；`cargo clippy --all-targets -- -D warnings` 零警告；`cargo fmt --all -- --check` 通过。

<details>
<summary>原始分析（修复前）</summary>

这是本次审查中后果最严重的问题，因为它造成**不可逆的数据损坏**，而触发动作只是一个日常设置操作。

| 环节 | 位置 | 行为 |
| --- | --- | --- |
| 判活 | `kernel.rs:443` | `running: port_open(settings.port)` —— 用**当前配置端口**判断内核是否在跑 |
| 同理 | `kernel.rs:451-459` | `ensure_workbench_stopped` 也只探测配置端口，于是 `set_active` 的"先停再切"守卫失效 |
| 保存 | `commands.rs:229-238` | `save_settings` 直接落盘，不检查内核是否在运行 |
| PID 校验 | `kernel.rs:1395-1419` | `pid_is_kernel` 要求命令行里的 `--port` **等于传入值**，否则判为"不是内核" |
| 回收 | `kernel.rs:1132,1183` | `reap_orphans` 同样以配置端口为判据 |
| UI 提示 | `ui/src/store.js:481` | `toastSuccess('设置已保存（重启内核后生效）')` —— 暗示无害 |

**触发路径**（内核在 3090 运行 → 设置页改成 3091 保存）：

1. `get_status` 探测 3091 → `running: false` → 概览页显示「未运行」，主按钮变回「打开工作台」。
2. 用户按提示点击 → `guarded_start` 见 3091 空闲 → `kernel::start(..., 3091)` 拉起**第二个内核**，两者 `cwd` 都是 `data_dir`。
3. 两个内核同时向同一份会话日志追加 → 正是 `kernel.rs:1109-1124` 注释里写明的 `seq gap in committed region` 损坏。
4. 此后旧内核（`--port 3090`）**再也无法被任何路径回收**：`pid_is_kernel(pid, Some(3091))` 返回 false、`reap_orphans` 用 3091、`register_child` 还会用新内核的 pid 覆盖 `kernel.pid`。
5. 更隐蔽的一条：若内核是上次壳启动留下的（内存句柄已丢），用户改端口后点「关闭工作台」→ `stop_kernel` 检查新端口空闲 → 跳过整个回收分支 → 只执行 `clear_pid`，**旧内核继续运行且追踪记录被清除**。

**建议修复**：把"工作台在跑"的判据从"配置端口有响应"改成与端口无关的活体证据（`state.running` 句柄 + `kernel.pid` + `pid_is_kernel(pid, None)` 的命令行校验），端口探测降为补充；`status()`、`ensure_workbench_stopped`、`guarded_start`、`open_harness`、`reap_orphans` 统一走这个 helper。另外当 `port` 变化且内核在跑时，`save_settings` 应拒绝保存并提示"请先关闭工作台"。

</details>

### 【已修】✅ P0-2 `install_node` 未列入授权白名单 →「帮我安装 Node.js」在真实构建中必然失败

**修复方式**：
1. `permissions/app-commands.json`：把 `install_node` 加入 `allow-local-commands`（放在 `detect_node` 之后，与 `lib.rs` 的注册顺序一致），并在该权限的 description 里注明这条不变量现在由 CI 守卫。
2. 新增 `scripts/check-invariants.mjs` + `npm run check:invariants`，把这条维护规则从"注释里的口头约定"变成**可执行的检查**。它校验四类跨文件不变量：
   - `generate_handler!` 注册集合 == `allow-local-commands` 白名单集合（双向，含重复项检测）——**P0-2 正是这条失守**；
   - 每个 capability 引用的自定义权限标识都真实存在；
   - UI（`ui/src/**`）与注入脚本（`src-tauri/src/*.js`）调用的每个命令都已注册且已授权；
   - 内置补丁清单结构有效（与 `patches.rs::validate_def` 对齐，并额外要求 copy 模式的 `from` 载荷确实存在于仓库中、补丁 id 不跨清单重复）。
3. 接入 `.github/workflows/desktop-release.yml` 的 quality job（`Check cross-file invariants`），与既有门禁同批执行。

**验证**：
- `npm run check:invariants` 通过：命令注册与白名单一致（45）、capability 权限引用有效（5 个文件）、前端与注入脚本调用的 42 个命令全部已注册且已授权、内置补丁清单有效（3 个定义）。
- **守卫有效性反证**：临时移除 `install_node` 后脚本以退出码 1 失败，并精确报出两处问题（"命令已注册但未列入 allow-local-commands" 与 "ui/src/store.js 调用了未授权的命令"）；恢复后重新通过。
- 构建产物层面确认：重新触发 tauri-build 后 `gen/schemas/acl-manifests.json` 的 `allow-local-commands` 为 45 条且包含 `install_node`。
- 全套验证绿：`cargo test` 190 通过、`cargo clippy --all-targets -- -D warnings` 零警告、`cargo fmt --check` 通过、`npm run test:ui` 16 通过、`node --test scripts/*.test.mjs` 7 通过、`node scripts/smoke-pullstring.mjs` 退出码 0。

<details>
<summary>原始分析（修复前）</summary>

| 证据 | 说明 |
| --- | --- |
| `lib.rs:109` | 命令已注册进 `generate_handler!` |
| `permissions/app-commands.json` | `allow-local-commands` 的 allow 列表**不含** `install_node`（白名单 44 项 vs 注册 45 项） |
| `gen/schemas/acl-manifests.json`（构建产物） | 展开后的列表同样不含它 |
| `ui/src/store.js:172-183`、`OverviewPanel.vue:165` | UI 确实调用该命令 |
| `tauri-2.11.5/src/webview/mod.rs:1823-1853` | `has_app_acl_manifest` 为真时 app 命令必须命中 ACL，否则 reject |

该文件自己的注释就写着这条维护规则：

> every command added to `tauri::generate_handler!` in lib.rs must be listed here, or its invoke from the panel fails with 'not allowed. Command not found'.

**影响**：这是"机器上没有 Node.js"这一首次使用场景的**唯一自助路径**。失败后 `store.js:185-207` 的 `maybePromptNodeInstall` 每 60 秒重新弹窗，形成"永远装不上、反复骚扰"的循环；release 构建下的错误文案是英文 `Command install_node not allowed by ACL`，同时违反中文文案与"错误须含可操作下一步"两条约定。

**历史**：`git log -S'install_node' -- src-tauri/permissions/app-commands.json` 为空 —— 自引入提交 `9375126` 起就从未授权，即 **`rc.11` 至 `rc.18` 的 8 个已发布版本都带此缺陷**。

</details>

### 【已修】✅ P0-3 copy 物化模式下技能卸载静默失效（Windows 默认用户必中）

**修复方式**（`src-tauri/src/skills.rs`、`docs/skill-management.md`）：

1. **所有权判定改为两条证据**：新增 `entry_is_owned(target, source, entry)` —— 条目归本商店所有的条件是"链接解析到中央库源"**或**"内容与 store.json 记录的指纹一致"。`unmaterialize_entry`（卸载/停用）、`ensure_entry`（安装/启用的占用检查）与 `reconcile_home` 的健康判定全部改用它。
2. **落地时记录指纹**：`SkillEntry` 新增 `materialized_sha256`，`ensure_entry` 在链接或复制完成后计算并返回它（`Materialized { mode, fingerprint }`），由各调用方写回 store：安装、更新刷新、启用三条路径都刷新，更新路径在刷新前保留旧指纹（否则旧副本会在刷新判定时被误认为外来条目）。
3. **链接比较改用 canonicalize**：`link_resolves_to` 先 `canonicalize` 两侧再比较，避免 Windows junction 的 `\\?\` 前缀与 macOS `/var → /private/var` 这类路径形态导致漏判（漏判的后果同样是"卸载了但技能还在"）；目标断链时退回 `read_link` 字面量比较。
4. **Windows 目录改用 junction**：`make_entry_link` 在 Windows 上对目录调用 `cmd /C mklink /J`（普通用户即可创建，无需 `SeCreateSymbolicLinkPrivilege`），扁平 `.md` 文件仍用文件符号链接（无 junction 等价物，失败时降级 copy —— 而 copy 现在可以被正常卸载）。这条同时让实现与 `docs/skill-management.md` 长期声称的行为一致（P2-49）。
5. **`ensure_entry` 不再谎报模式**：短路返回时按磁盘上的真实形态回报 `link` / `copy`，而不是照抄期望模式（此前 link 降级成 copy 后，UI 会一直显示"已链接"）。
6. **孤儿清扫覆盖 copy 遗留**：`reconcile_home` 的清扫新增"内容指纹命中 store 中仍记录的指纹"这一条件，用 `remove_target`（而非只删链接的 `remove_link`）回收；指纹不在记录里的条目一律不动——可能是用户自己放的。
7. **历史数据迁移**：`reconcile_home` 对"`enabled` 且存在但缺少指纹"的条目补记一次指纹，使修复前就已 copy 落地的技能从此可以被正常停用/卸载。

**回归测试**（`skills.rs::tests`，3 个）：
- `copy_mode_entries_can_be_disabled_and_uninstalled`：copy 安装 → 停用要真的摘掉副本 → 重新启用 → 卸载后两个副本都必须消失（并断言落地的是真实目录、每个条目都有指纹）。
- `modified_copy_entry_survives_uninstall`：用户手工改写过的副本不得被当作本商店的副本删除。
- `reconcile_backfills_fingerprints_for_legacy_copy_entries`：抹掉指纹模拟旧数据 → `reconcile` 补记 → 卸载成功。

**验证**：`cargo test` 193 通过 / 0 失败。**测试有效性反证**：临时把 `entry_is_owned` 退回"只认链接"后，上述第 1、3 个测试立刻失败（第 2 个仍通过，符合预期）；恢复后全绿。`cargo clippy --all-targets -- -D warnings` 零警告、`cargo fmt --check` 通过。

<details>
<summary>原始分析（修复前）</summary>

| 位置 | 行为 |
| --- | --- |
| `skills.rs:1164-1176` | `unmaterialize_entry` 的 `owned` 判定要求目标**必须是符号链接** |
| `skills.rs:1146-1158` | 建链失败即回退 `copy`（真实目录/文件） |
| `skills.rs:1049-1061` | Windows 用 `symlink_dir`/`symlink_file`，**不是**文档写的 junction（需要 `SeCreateSymbolicLinkPrivilege`） |
| `skills.rs:1586-1603` | `reconcile` 的孤儿清扫同样只处理符号链接 |

**触发**：Windows 未开启开发者模式的普通用户装技能包 → 落 copy 模式（UI 显示「复制」chip）→ 点「卸载」→ `owned=false`，一个文件都不删，而中央库目录与 `store.json` 记录照删。

**影响**：技能永久残留在 `<DSH_HOME>/skills/<name>`，运行中的内核继续发现并注入模型上下文（用户以为已卸载）；壳已无记录、`reconcile` 也永远清不掉；重装同包会撞「技能名冲突」（`skills.rs:1119-1135`）且 UI 无补救入口。同一判定还导致 copy 模式下 `set_enabled` 无效、每次启动都判「不健康」并全量重拷贝。

**文档缺陷**：`docs/skill-management.md:78` 写的是「建 symlink（**Windows junction**）」—— 文档描述的正是本该规避此问题的那种实现。

</details>

---

## P1 —— 重要，建议本迭代内处理

### 内核、进程与更新链

**【已修】P1-1 ~ P1-4 内核与进程链四处修复**

1. **P1-1 子进程 PATH**：新增 `process::command_with_path_dirs`，`kernel::start` 用它把 `node.parent()` 前置到内核进程的 PATH——走托管安装（`<data_dir>/tools/node/<ver>/bin`）或 nvm 绝对路径时，那个 node 根本不在继承来的 PATH 上，内核派生的一切 `#!/usr/bin/env node` 子进程都会以 `env: node: No such file or directory` 失败。同时给 plugins 的两条 pnpm 主路径（`install_store_deps`、`run_profile_install`）补上托管 node 目录：npm 全局 prefix 与 node 安装目录经常不是同一个，只前置 pnpm 目录时包的生命周期脚本同样会以 127 失败。
2. **P1-2 版本号校验**：新增 `version::is_valid_kernel_version`（只接受 semver 形态字符集，显式拒绝 `.`/`..`/分隔符/引号，长度 ≤128）。接入四条路径：npm packument 过滤、`release_from_tag`（GitHub API 与 Atom 两条来源的**共同出口**）、`install_kernel` 命令边界、`kernel_plugin_list` 命令边界；内核 stub `package.json` 改由 `serde_json::json!` 构造，不再手写 `format!` 模板（引号无法闭合注入）。
3. **P1-3 启动防护归因**：新增 `BootVerdict::SpawnFailed`，`boot_once` 的 Err 分支（端口被无关进程占用、版本未安装、目录不可写…）归入该类；`guarded_start` 据此设置 `kernel_started`，为假时**不做插件归因、不进第 2/3 次尝试、不写 quarantine**。这类失败日志里根本没有插件证据，重试与"停用插件"不可能改变结果，只会在重试恰好成功时把环境故障记成插件故障。
4. **P1-4 假失败**：子进程退出后不再等满 30 分钟总超时，改为 `DRAIN_GRACE = 5s` 宽限 + 500ms 轮询（心跳消息仍按 10s 节流）；宽限到点即返回并把剩余输出标记为截断，成功路径相应地跳过 `join`（否则又变回无限等待）。

**回归测试 4 个**：`command_with_path_dirs_prepends_extra_directories`、`version::tests::accepts_only_semver_shaped_versions`、`run_with_progress_returns_soon_when_a_grandchild_holds_the_pipe`（实测 5.5s 返回，修复前是 30 分钟）、`unrelated_port_occupant_does_not_quarantine_plugins`。

**P1-3 的测试区分度经过一次修正**：最初断言"quarantine 为空"，反证时**没有失败**——因为旧行为虽然会隔离全部插件，却在重试同样失败后回滚了隔离状态，最终 quarantine 仍是空的。改为断言"启动轨迹里不得出现安全模式"后，反证立即失败（轨迹里出现「安全模式下仍失败」）。致命的其实是"重试恰好成功"那条路径：那时看护会把环境故障记成插件故障。

**【已修】✅ P1-1 内核子进程的 PATH 缺少已解析的 node 目录**
`kernel.rs:1051` 用 `process::command_with_path(node)` 启动内核，而 `process.rs:131-135` 只 stamp `env::merged_path()`（`env.rs:50-67` 证实它不含托管 node 目录）；对照安装路径 `kernel.rs:597-598`、`kernel.rs:941-942` 都显式前置了 `node_dir`，`process.rs:933` 的 `spawn` 也支持 `extra_path_dirs`。
**触发**：node 来自托管安装（`<data_dir>/tools/node/<ver>/bin`）或 nvm 绝对路径。
**影响**：内核一切 `#!/usr/bin/env node` 子进程（插件 CLI、`npm`/`npx`、工作台终端任务）报 `env: node: No such file or directory`；若 PATH 上另有别的 Node 线，还会撞 `NODE_MODULE_VERSION` 不一致。这与 `docs/embedded-node-runtime.md:17-18` 承诺的"shebang 开箱即用"矛盾。
**修复**：给 `command_with_path` 增加 `extra_path_dirs` 变体，在 `kernel::start` 传入 `node.parent()`。

**【已修】P1-2 远端版本号未做任何校验就拼进文件系统路径与 stub `package.json`**
`releases.rs:206-213`（npm 来源只过滤空串、空格、`/`，放行 `\`、`.`、`..`）、`releases.rs:138-151,286`（GitHub API/Atom 来源**完全不过滤**，只要 `dsh-v` 前缀）、`commands.rs:423-461`（`install_kernel(version)` 无校验）、`kernel.rs:202-204`（`kernels_dir.join(version)`）、`kernel.rs:554-561`（`create_dir_all` + `format!("{{\"name\":\"dsh-kernel-{}\",…}}", version.replace('.', "_"))`）。
**触发**：(a) 默认 registry 是第三方镜像 `registry.npmmirror.com`（`registry.rs:14`）且 `DSH_NPM_REGISTRY` 可指向任意主机 —— 镜像在 packument 里塞入形如 `..\..\..\Users\<user>\AppData\Roaming\...\Startup\x` 的版本键，Windows 上 `\` 是分隔符，该行会带「安装」按钮出现在版本列表；(b) 版本号含 `"` 时 `format!` 生成的 JSON 可被闭合，得到字段受远端控制的 stub manifest，pnpm 在 `add` 过程中会执行其中的 `preinstall` 脚本；(c) 连官方仓库都能无意触发：git tag 允许 `/`（`dsh-v1.0/hotfix`），GitHub 回退路径会产出 `1.0/hotfix` 并在 `kernels/` 下建出嵌套目录，`list_installed` 随后把无 `bin.js` 的 `1.0` 当成已安装版本。
**修复**：在命令边界加 `validate_kernel_version()`（`^[0-9A-Za-z][0-9A-Za-z.+-]*$`，显式拒绝 `.`/`..` 与任何分隔符），三条来源路径共用；`kernel_dir` 再做一次 canonicalize 后的包含性校验；stub manifest 改用 `serde_json::json!` 构造。

**【已修】✅ P1-3 启动防护把"零证据"失败升级为安全模式，全量停用第三方插件**
`guard.rs:562-580`：第 3 次尝试的条件是 `!store_items.is_empty()`，**与失败原因无关**；`guard.rs:507-511` 在 `attribute` 返回空时继续该分支；`guard.rs:1017-1023` 的单测证明 `EADDRINUSE` 正是"无 suspects"路径。
**影响**：落盘 `safe_mode: true, cause: "plugin"` 的 Incident，文案是「无法定位具体引发故障的插件，安全模式已停用全部第三方插件」，**并真的改写 `quarantine.json` 与 profile 接线**。用户按提示逐个处置，可能删掉无辜插件。
**修复**：为 `BootVerdict` 增加 `SpawnFailed`/`NoOutput` 分类，Err 分支不进重试阶梯；进入安全模式前要求 tail 非空且失败发生在进程真的跑起来之后；`EADDRINUSE` 单独给端口提示并直接返回。

**【已修】✅ P1-4 `run_with_progress` 在子进程已退出但孙进程仍持有管道时等满 30 分钟并报假失败**
`process.rs:869-871` 要求 `child_exited && output_closed` 才跳出，而 `output_closed` 只在 `Disconnected` 时置位；`process.rs:851` 的总 deadline 是 30 分钟。
**影响**：安装其实已成功，UI 却转 30 分钟才报失败（心跳文案还会说"子进程仍在运行"，与实际相反）；Windows 上 `taskkill /PID <已退出的 cmd> /T /F` 是 no-op，孙进程既不被杀也不回收，继续持有 `<data_dir>` 写入。
**修复**：`child_exited` 后再给 drain 2-5 秒宽限即跳出并把输出标记为截断；Windows 上给子进程加 Job Object（见 P2-2）。

**【已修】✅ P1-5 `confirm_close_shell` 漏关 `official-chat` 窗口，与函数自身声明的强不变量矛盾**
`commands.rs:1537-1541` 遍历 `["official-chat", "harness", "log-viewer"]` 时用 `app.get_webview_window(label)`，但 `official-chat` 是 `WindowBuilder` 创建的**裸窗口**（`commands.rs:1168`）—— `tauri-2.11.5/src/lib.rs:576-585` 证实 `get_webview_window` 对裸窗口返回 `None`，该 label 永远匹配不上。而函数注释（`commands.rs:1515-1522`）明确写着"已确认的退出必须自己关闭每一个窗口，而不是把这件事交给 `RunEvent::Exit`"。
**现状**：靠 `lib.rs:214-216` 的 Exit 兜底（那里用的是 `get_window`）侥幸生效。
**修复**：改用 `get_window` 或 `windows()` 遍历。

**【已修】P1-6 / P1-7 / P1-10 / P1-18**

1. **P1-6 gzip 炸弹**（`Cargo.toml`）：关闭 ureq 的 `gzip` 特性。`ureq` 的 `LimitReader` 位于解压器**内层**，请求 gzip 时 `MAX_HTTP_BODY_BYTES`(10 MiB) 与 `MAX_TARBALL_BYTES`(256 MiB) 只约束压缩后的字节——一个 9.9 MiB 的 gzip 炸弹可解出数十 GB 并触发 OOM abort（整个壳连同正在跑的内核一起消失）。不带该特性时 ureq 不会声明 `Accept-Encoding`，服务器也就不会压缩；若服务器仍强行 gzip，ureq 会原样透传而不是误解压。
2. **P1-7 updater 超时**（`updater.rs`）：两处 `app.updater()` 换成 `updater_builder().configure_client(connect 15s + read 30s/60s)`。插件默认不设任何超时，认证门户或只握手不回包的代理会让 `check_shell_update` 永不返回，而 UI 的 `withExclusiveLoading` 让 `globalBusy` 长期为真——几乎所有 IO 按钮被禁用且无取消入口。
3. **P1-10 Atom 预发布标记**（`releases.rs`）：`release_from_tag` 内部统一叠加 `version.contains('-')`。Atom 兜底路径只能从 tag 名推断，原先硬编码 `prerelease=false`，`0.1.2-rc.19` 会被当成稳定版：列表不打标签，首次运行引导的「安装最新版本」还会优先选中它。
4. **P1-18 git clone 超时**（`process.rs` + skills/plugins）：`run_capture_command_bytes` 参数化（新增 `..._with_timeout` 与公开的 `run_command_capture_with_timeout`），新增 `GIT_CLONE_TIMEOUT = 600s`，两处 `git clone` 改用它，并把超时包装成中文可操作提示。30 秒的默认值是为 `git --version`/`taskkill`/`lsof` 这类短工具准备的，而很多 dsh 插件没有 GitHub Release，clone 是主安装路径。

另外补上 `releases.rs` 的测试模块（该文件此前 0 测试，见 P2-33）：两条用例覆盖预发布判定与非法版本号过滤。

**【已修】✅ P1-6 gzip 炸弹可绕过响应体大小上限（10 MiB 上限换来数十 GB 解压）**
`releases.rs:29-30,32-40,44-57` 依赖 ureq 的 `BodyWithConfig::limit()`，但 `ureq-3.4.0/src/body/mod.rs:744-775` 显示 `LimitReader` 位于 `ContentDecoder::Gzip` 的**内层** —— limit 数的是压缩后的字节。`Cargo.toml` 开启了 ureq 的 `gzip` 特性，ureq 默认发送 `Accept-Encoding: gzip`。
**影响**：`read_to_string()` 按解压后规模增长，可触发 Rust 默认的 OOM abort 直接杀掉整个壳（连带把正在跑的内核留成孤儿）；`http_get_file` 的 256 MiB 上限同理只算压缩字节。
**修复**：加 `.header("Accept-Encoding", "identity")`（或去掉 gzip 特性）；保留 gzip 时改为 `reader()` + 循环累加解压后字节数。

**【已修】P1-7 updater 的检查与下载没有任何超时，挂起的 endpoint 会锁死整个面板**
`updater.rs:191-219`（`check()`）、`updater.rs:245-274`（`install()`），从未调用 `UpdaterBuilder::timeout`（插件默认 `timeout: None`）。
**影响**：认证门户 / 卡死代理下 `check_shell_update` 永不返回，`withExclusiveLoading` 让 `globalBusy` 长期为真，几乎所有 IO 按钮被禁用且无取消入口，用户只能退出应用。
**修复**：`configure_client` 设置 connect/read 超时。

**【已修】P1-8 / P1-16**

1. **P1-8 tarball 完整性校验**（`releases.rs` + plugins/skills）：`NpmDist` 反序列化 registry 的 `integrity`（SRI）；新增 `verify_download_integrity` —— 解析 `sha512-<base64>` / `sha256-<base64>`，用内置的零依赖 base64 解码 + `sha2` 比对下载内容，不符即拒绝并删除临时文件；不支持的算法（如 `md5-`）显式拒绝而不是放过；`integrity` 缺失（老 packument 只有 sha1 的 `shasum`）时在进度里说明"跳过内容校验"。此前外壳从第三方镜像下载 tarball 后直接解包，而解包过程中 pnpm 会执行包里的 `prepare` 脚本——等于执行未经验证的下载物。
2. **P1-16 npm 候选回退**（`plugins.rs`）：`install_unlocked` 的失败分支现在区分两种情形——**取源/校验阶段失败**（内部自带回滚，不留痕）继续尝试下一个候选；**写完 store 行之后失败**（物化、profile 接线）则立即中止，并提示"请先卸载它，再重试或改用 GitHub 来源"。旧代码一律 `if let Ok(item)` 吞掉，于是用户要的是 GitHub 仓库，却会多出一个自己没要求的 npm 插件行（已进 store.json，下次同步就参与接线），而进度里只有一句"npm 包不可用"。

**回归测试**：`download_integrity_compares_content`（用空文件的知名 sha512 向量同时验证 base64 解码与摘要比对；另覆盖摘要不符、缺失、空白、不支持算法四种分支）。

**【已修】P1-8 外壳自己下载的 npm / git tarball 不做完整性校验**
`releases.rs:61-89`（`http_get_file` 无哈希）；调用点 `plugins.rs:1253-1267`、`skills.rs:963-967`；`NpmDist` 只反序列化 `tarball`，把 registry 已给出的 `dist.integrity`/`dist.shasum` 丢掉了。对照组：`node_install.rs:347-367` 是先取 `SHASUMS256.txt` 校验 SHA-256 再解包的。
**影响**：默认 registry 为第三方镜像时，packument 与 tarball 同源，镜像可在元数据一致的前提下替换 tarball；解包后 `build_git_plugin`（`plugins.rs:1443-1483`）会跑包里的 `prepare` 脚本，等于执行未经校验的下载物。
**修复**：给 `NpmDist` 加 `integrity`，解包前按 SRI 校验 sha512；拒绝 `dist.tarball` 中的非 https URL。

**【已修】P1-9 / P1-22**

1. **P1-9 半成品内核目录**（`kernel.rs`）：`install_version` 拆成"外层包装 + `install_version_into`"，外层在**全新安装**失败时删除整个版本目录（重装场景保留残骸，用户可能想对比或手动处理）。此前 pnpm 跑到一半、原生模块没编译出来、smoke 探针失败都会留下一个目录，`list_installed` 把它当成已安装版本列出来并允许切换过去；真正的失败推迟到启动内核时（报原生模块缺失），然后被启动看护当成疑似插件问题处理几分钟——用户完全看不出根因是这个版本压根没装完。
2. **P1-22 多补丁同文件**（`patches.rs`）：apply 的计划阶段新增文件级占用检查——同一内核版本下，若某个 `to` 已被另一个补丁的应用记录占用，则拒绝并指名占用者。没有这道闸时：撤销 A 会用它自己的备份直接覆盖，把 B 的改动静默丢掉；再撤销 B 又写回 B 的备份，最终内核带着 A 的改动运行而 `state.json` 里已无任何记录（既不显示 dirty 也无法再撤销）。检查是纯校验、无副作用。

**回归测试 2 个**：`failed_fresh_install_leaves_no_half_built_version`、`a_second_patch_on_the_same_file_is_rejected`，两者都经反证确认（临时禁用守卫即失败）。

**【已修】P1-9 安装失败残留的半成品目录会被当成「已安装」并允许切换**
`kernel.rs:547-555`（先 `create_dir_all`）、`kernel.rs:614-673`（校验失败直接 `return Err`，不清理不打标）、`kernel.rs:366-390`（`list_installed` 把 `kernels/` 下任何目录都当已安装）、`kernel.rs:463-471`（`set_active` 只要求 `bin.js` 存在）。
**影响**：用户切换到这个坏版本后，内核在启动期抛原生模块缺失错误，guard 归因找不到证据 → 走 P1-3 的安全模式路径（停用全部插件 + 重连 profile + 再启动两次），几分钟后才给出 `cause=unknown` 的事故报告。
**修复**：产物先落在 `kernels/.staging-<version>/`，校验与 smoke 探针都通过后才 rename；`list_installed`/`set_active`/`start` 只接受带完成标记的目录。

**【已修】P1-10 Atom 回退把所有条目标成非预发布，首装引导会把预发布内核当稳定版**
`releases.rs:286` 硬编码 `prerelease=false`（对比 npm 路径 `releases.rs:216` 的 `version.contains('-')`）；`ui/src/store.js:286-287` 的 `releases.find(r => !r.prerelease)` 于是选中预发布；`store.releaseWarning` 只在 `VersionsPanel.vue:92` 渲染，概览页首装 callout 不显示"已回退到 Atom"。
**修复**：`release_from_tag` 内统一 `prerelease || version.contains('-')`；首装路径也展示 `releaseWarning`。

**【已修】P1-11 `install_shell_update` 在 async 上下文直接调用同步的 `update.install()`**

**修复方式**（`src-tauri/src/updater.rs`）：`Update::install` 是同步实现（落盘安装包 + 派生安装器），Windows 上还要等 NSIS 起来，可能阻塞几十秒。改为 `update.clone()` 后交给 `tauri::async_runtime::spawn_blocking`，等它返回再 `app.restart()`。`Update` 是 `Clone` 且字段全为 Send 类型，无需额外包装。
`commands.rs:353-364` 未走 `spawn_blocking`，而 `updater.rs:282-284` 的 `update.install(bytes)` 是含阻塞 fs 与进程 spawn 的同步实现。
**修复**：包一层 `spawn_blocking`。

> 注：此项是两个子代理的分歧点（一个认为已符合约定），我复核后确认**约定偏离成立**，但影响有限（阻塞一个 tokio worker 线程）。

### 插件（plugins.rs）

**【已修】P1-12 / P1-19 / P1-29**

1. **P1-12 copy 模式物化短路**（`plugins.rs`）：`KernelMeta` 新增 `fallback`（区分"link 降级为 copy"与"用户主动选 copy"）；短路判定改为按**记录的实际形态**校验健康度——copy 落地的是真实目录，旧代码"目标必须是符号链接"对它恒为假，于是每次启动内核都 `remove_materialized` + 整树 `copy_tree`（含 node_modules 的插件可达两万文件）。已降级的副本不再每次重试建链；用户主动切换模式时 `set_mode_unlocked` 删除各内核下的 `.meta` 强制重新物化。回归测试 `copy_materialization_short_circuits_on_resync` 用不在中央库里的标记文件检测"是否被重拷"——这个测试在修复完成前失败过一次，暴露出我只覆盖了降级分支、漏了"用户选 copy"分支。
2. **P1-19 检查更新的错误与 TTL**：抽出 `should_advance_update_check`（全部来源失败时不推进 `last_checked_at`，否则 15 分钟 TTL 会把一次失败伪装成"刚查过"，自动检查静默停摆）；UI 侧把逐包的 `error` 汇总成 toast，并只在成功时推进前端的 TTL。
3. **P1-29 端口文档**：`README.md` 4 处 + `docs/troubleshooting.md` 1 处的 3080 改为「release 3090 / dev 3091」，并在端口冲突条目里补上「工作台运行期间不能改端口，需先关闭工作台」。


**【已修】P1-12 copy 模式的 materialize 短路判定永远失败 → 每次启动都整树重删重拷**
`plugins.rs:1716-1734`：`fresh` 用 `meta.mode`（实际）比 `item.mode`（期望），link→copy 降级后恒 false；即使两者都是 `copy`，`target_ok` 也只接受"目标本身是 symlink"，真实目录恒 false。
**影响**：每次 `start_kernel`/`activate_version`/`sync_all` 都 `remove_materialized` + 全量 `copy_tree`（含 `node_modules` 的插件注释自述可达 2 万文件）；中途失败留下半棵树。`docs/plugin-management.md:131` 声称 copy 模式按"大小+修改时间"跳过未变化文件，代码里没有该逻辑。
**修复**：copy 模式下 `fresh && target.exists()` 直接短路，仅在期望/记录为 link 时做 double-symlink 校正。

**【已修】P1-13 / P1-14 —— P1 全部完成**

1. **P1-13 copy 模式更新后不重跑 profile 安装**（`plugins.rs`）：`update_unlocked` 在 copy 模式下额外补跑一次 `run_profile_install`。pnpm 的 `file:` 依赖是在 install 时被硬链接进 profile 的 `node_modules` 的，而接线判定只看"manifest 文本变没变 + node_modules 在不在"——两者都没变，常规接线直接跳过重装，内核继续按 profile 里的旧副本解析 bundle：UI 显示"已更新"，重启后跑的还是旧代码。补跑的 pnpm install 是幂等的，代价几秒。
2. **P1-14 peer 链接不重链**（`plugins.rs`）：`refresh_store_peers` 原来用 `dest.exists()` 判断"已在库内安装"——一条指向**上一个内核**的链接照样为真，于是被跳过、`meta.peers` 记成空数组，而早退条件只看 kernel 字段，从此每次启动都命中早退、永不重链。现在改为按 `canonicalize` 比对判定"是否已正确解析到当前内核"：已正确则记进 peers（让 meta 如实反映磁盘），指向别处则 `remove_link` 后重建；真实存在的目录/文件（npm 装进来的或 hoisted 的）仍不动。

**回归测试**：`refresh_store_peers_relinks_links_pointing_at_the_old_kernel`（两个内核各带一份 cordis，先按 0.1.1 解析、切到 0.1.2 后断言链接指向新内核）。反证：恢复 `dest.exists()` 判断后，链接仍指向 0.1.1，测试失败。

**【已修】P1-13 copy 模式更新后不重跑 profile `pnpm install`，内核继续解析旧内容**
`plugins.rs:2217-2233`、`2941-2976`：profile spec 文本不变 → `changed=false`，且 `node_modules` 存在 → `if changed || node_modules_missing` 为假 → 不跑 `run_profile_install`。
**影响**：pnpm `file:` 依赖在 install 时已硬链接进 `profiles/<p>/node_modules/`，内核按 profile 解析 bundle，因此重启后仍是旧代码，而 UI 显示「已更新」。`docs/plugin-management.md:107` 明确要求 copy 模式更新后重跑 profile install。
**修复**：让物化返回"确实重新物化"标志，并入强制安装条件。

**【已修】P1-14 `refresh_store_peers` 跳过已存在链接并在写回后早退 → 切内核后仍解析旧内核的 cordis/dsh-\***
`plugins.rs:3275-3320`：每个 peer 因 `dest.exists()`（仍指向内核 A）被 continue，随后 meta 被改写成 `{kernel: B, peers: []}` → 以后每次启动都命中早退、永不重链。
**影响**：插件继续 import 内核 A 的 `cordis`/`@deepseek-ai/*`，形成两份实例（重复注入 / 服务找不到），A 卸载后变悬空链接 —— 正是 `docs/plugin-management.md:54` 承诺要避免的。
**修复**：把"已存在"改成"已存在且解析正确"（比较 `read_link(dest)` 与期望内核路径），只在真正解析成功时写 `kernel: active`。

**【已修】P1-15 / P1-23 状态文件损坏被静默当成空清单（plugins / skills / patches 三处同一根因）**

**修复方式**：
1. `process.rs` 新增 `StateRead<T>`（`Loaded` / `Missing` / `Corrupt { reason }`）与 `read_state_file`，把"文件不存在"与"读取/解析失败"彻底分开——前者是正常的首次运行，后者绝不允许退化成空状态。损坏时**不改名、不删除原文件**：文件留在原处，保护持续有效（改名会让下一次启动看到 `Missing`，等于把静默清空推迟一次）。
2. 每个模块新增严格读取：`plugins::load_store_checked`、`skills::load_store_checked`、`patches::read_state_checked`；`load_store` / `read_state` 保留为**只读展示**路径的容错版本。
3. 写路径与清扫路径全部改用严格读取：
   - plugins：`upsert_item_unlocked`、`remove_item_unlocked`、`set_store_warning`（启动看护每轮必调，原本会用空清单覆盖 `store.json`）、`sync_all`（会 `sweep_all_kernel_orphans`）；
   - skills：`upsert_item_unlocked`、`remove_item_unlocked`、`reconcile_home`（原本会认为活动根里所有指向中央库的条目都是孤儿链接并逐个删除）；
   - patches：`apply`、`revert`。
4. 展示路径把损坏暴露成警告：`PluginStatus.warning` / `SkillStatus.warning`（面板已有渲染路径）+ 新增 `PatchStatus.warning` 并在设置页补丁卡片上方渲染 `el-alert`。

**回归测试**（3 个，全部经反证验证）：`plugins::tests::corrupt_store_is_never_treated_as_empty`、`skills::tests::corrupt_store_does_not_sweep_skill_links`、`patches::tests::corrupt_patch_state_blocks_apply_and_revert`。反证：把三处严格读取临时退回容错后，三个测试同时失败（198 → 3 failed），恢复后全绿。

**【已修】P1-15 `load_store` 解析失败静默回落空清单 → 清退接线 + 删除各内核物化产物 + 覆盖 store.json**
`plugins.rs:504-509`（`unwrap_or_default()`）、`2311-2316`（无条件写库）、`1806-1829`（sweep）、`2749-2758`。
**触发**：Windows 杀软/索引器短暂锁住文件，或手工编辑导致反序列化失败（`Store` 无 `#[serde(default)]`）。
**影响**：同一次启动里 `sweep_kernel_orphans` 删掉活动内核的所有外壳管理物化目录、`wire_manifest` 清退全部托管依赖、`set_store_warning` 用空清单覆盖 `store.json`（原清单不可恢复、无备份）。
**修复**：区分"文件不存在"与"读/解析失败"；失败时备份为 `.corrupt-<ts>` 并保留上一份，跳过 sweep 与写库。（与 P1-16 同源，建议一起修。）

**【已修】P1-16 `install_unlocked` 的 npm 候选回退吞掉全部错误，会把半成品插件留在 store/profile**
`plugins.rs:2889-2901`：失败点在 `upsert_item_unlocked` 之后的 `sync_kernels` 或 `ensure_wiring` 时错误被丢弃，继续装 git 来源。
**影响**：用户要的是 GitHub 仓库，却多出一个自己没要求的 npm 插件行（`dsh-x` 与 `github.com__owner__x` 是两个 id），它已进 `store.json`（下次同步即参与接线）。
**修复**：只对"取源/校验失败"回退；"发布/接线失败"必须上报，并在回退前回滚候选插件。

**【已修】P1-17 `copy_tree` 跟随仓库内 symlink 且无根约束**

**修复方式**：
- `plugins.rs`：`copy_tree` 计算源根的规范化路径并沿递归传递；新增 `link_escapes_root`，符号链接只有在解析后仍落在源根**之内**时才被跟随，越界链接跳过并打 warning。原先只检测环（`link_points_to_ancestor`），一个 `payload -> /Users/<user>` 的链接就会把宿主机整棵目录树拷进 `kernels/<v>/plugins/<id>/`——磁盘被填满，私有文件进入内核进程与插件代码可读范围。
- `skills.rs`：`copy_tree` 改为**跳过全部符号链接**，与技能扫描器的语义保持一致（扫描器本就不把链接视为技能入口，`git clone` 常留下 `SKILL.md -> ../../SKILL.md` 这类装饰性重定向）。旧实现在 else 分支走 `fs::copy`，遇到目录链接会让整次复制以 `os error 2` 失败。

**回归测试**：`copy_tree_skips_links_escaping_the_source_root`（构造源根之外的目录 + 指向它的链接，断言目标里没有 `payload/private/secret.txt`）。反证：临时禁用 `link_escapes_root` 守卫后该测试立即失败。

**【已修】P1-17 `copy_tree` 跟随仓库内 symlink 且无根目录约束**
`plugins.rs:1847-1940`（symlink 分支 1914-1932）、`1851-1862`（只查环）；来源 `1375-1380`（git clone 不校验 symlink，tarball 路径有 `archive.rs:92` 拒绝）。
**影响**：插件树里的符号链接（如 `payload -> /Users/<user>`）会让链接目标整棵树被拷进 `kernels/<v>/plugins/<id>/` —— 磁盘被填满（配合 P1-12 反复重放），宿主私有文件被搬进插件目录。
**修复**：canonicalize 后与源根做前缀比较，越界跳过并 warning；同一修复镜像到 `skills.rs:768`。

**【已修】P1-18 `git clone` 走 30 秒硬超时通道（插件与技能两条路径都有）**
`plugins.rs:1381-1382` + `process.rs:510`（`RUN_CAPTURE_TIMEOUT = 30s`）；技能侧 `skills.rs:1003`。
**影响**：GitHub Release 不可用时的回退路径、任意 Git 主机、较大仓库都会超时被杀；用户看到英文 `无法运行 git：git clone timed out after 30 seconds`（无下一步、无日志路径）。很多 dsh 插件没有 Release，clone 是主路径。
**修复**：把该常量参数化，clone 用独立长超时（如 600s），并把超时包装成中文 + 指向日志。

**【已修】P1-19 `check_updates` 的逐包错误被 UI 吞掉，且失败仍推进 `last_checked_at`**
`skills.rs:1479-1500`、`:1512`；`ui/src/skills.js:74-82` 只读 `latest` 不读 `error`。
**影响**：`ls-remote` 超时后用户点「检查更新」什么都不发生（错误不显示，15 分钟 TTL 内不再重试）。
**修复**：UI 汇总展示 `error`；检查失败不推进 `last_checked_at`。

### 内置补丁（patches.rs）

**【已修】P1-20 apply 边校验边写、失败不回滚也不留记录 → 半补丁内核 + 应用内无恢复路径**

**修复方式**（`src-tauri/src/patches.rs`）：把 `apply` 拆成两个阶段——阶段 1 用新的 `plan_file` 对全部文件做**只读校验**并产出执行计划（`Planned::Write` / `Planned::Skip`），阶段 2 用 `commit_file` 逐个备份 + 写入；任一文件执行失败即调用 `rollback_files`，按本次生成的备份把已写入的文件还原、删除新建的文件并清理本次备份目录，同时单独清理失败文件自身可能已生成的备份。错误信息明确说明"本次已写入的内核文件已回到应用前的状态"。回归测试 `failed_second_file_leaves_no_partial_patch`：第二个文件的 `expectSha256` 不匹配时，第一个文件必须仍是原文、state 无记录、备份目录不存在。
`patches.rs:610-768`（同一循环里串联校验与写入）、`:798-804`（全部成功才写 state）、`:489-496`（残留备份阻止重试）。
**触发**：多文件补丁（如 `dsh-file-perf` 的两个 copy 文件）在文件 1 已写入后，文件 2 的 `expectSha256` 不匹配。
**影响**：内核停在半补丁状态，设置页显示「未应用」，点「应用」被残留备份挡住，点「撤销」因无记录报「补丁未应用」—— **应用内没有任何恢复路径**，只能手删 `~/.dsh/desktop/patches/backups/<id>/`。
**修复**：两阶段执行（先全部纯校验，再统一备份 + 写入）；写入阶段失败时用本次备份回滚并清理。

**【已修】P1-21 revert 不可重入：首个文件成功即删备份，后续失败导致撤销永久卡死**

**修复方式**（`src-tauri/src/patches.rs`）：还原循环改为 `revert_one` 单文件函数 + 每成功一个文件就调用 `persist_revert_progress` 把"剩余待还原文件"写回 `state.json`（中途失败也先落盘再报错）。`AppliedFile` 新增 `originalSha256`（`backup_target` 改为返回原文件哈希），`handle_missing_backup` 增加"目标已回到原文件内容 → 视为已还原"分支——这是重入的第二次撤销不会被误报成"目标已被修改"的关键。回归测试 `revert_resumes_after_a_partial_failure`：第一次撤销在第二个文件失败后，记录里只剩该文件、第一个文件已还原；用户恢复该文件后第二次撤销完成。
`patches.rs:850-907`（循环 + 末尾一次性写 state）、`:861`（成功后立即删备份）、`:914-949`（备份缺失兜底）。
**影响**：再点「撤销」时文件 1 的备份已不在，而目标现在是正确的原文件内容，与 `patched_sha256` 不符 → 报「目标文件已被修改（内容与补丁记录不一致）」，**与磁盘真实状态完全相反**，且无出路。
**修复**：逐文件持久化进度；`handle_missing_backup` 把"target == 备份内容"也视为已还原。

**【已修】P1-22 同一文件被两个补丁改动时无所有权校验 → 逆序撤销静默留下已打补丁的文件**
`patches.rs:705-761`（replace 分支只看 search 是否命中）、`:850-901`（revert 有备份就直接覆盖）。
**影响**：先撤 A 会把 B 的改动一起覆盖；最终内核带着 A 的补丁运行而 `state.json` 无任何记录 —— 既不显示 dirty 也无法再撤销。当前 3 个内置补丁目标不重叠故未触发，但机制零拦截。
**修复**：文件级占用登记；revert 写回前校验 `target hash == 记录的 patched_sha256`。

**【已修】P1-23 `state.json` / `store.json` 反序列化失败被静默当成空，skills 还会据此删除自己的链接**
`skills.rs:278-283`、`patches.rs:427-432`；`skills.rs:1538-1603` 的 `reconcile` 会据此把活动根里所有指向 store 的链接判为孤儿并删除，warning 不落盘（`:1606-1610`）。
**影响**：运行中的内核**静默丢掉全部技能**，随后 `check_updates` 会用空清单覆盖损坏文件；补丁侧表现为"补丁还在、壳说没打、也撤不掉"。
**修复**：同 P1-15。

### 界面

**【已修】✅ P1-24 `v-loading` 指令从未注册 → 插件中心刷新时整块列表变成无文案空白**
`ui/src/components/PluginsPanel.vue:280` 是全仓库唯一的 `v-loading` 用法（`v-loading="true"` + `element-loading-text="目录加载中…"`），但 `ui/src/main.js:64-83` 只做 `app.component(...)`，既无 `app.directive('loading')` 也无 `app.use(ElLoading)`，且未引入 loading 样式；编译产物中只剩属性字符串。Vue 的 `withDirectives` 遇到 falsy 指令直接跳过。
**触发**：首次进入「插件」页或点「刷新目录」（`plugins.js:120` 先把 `catalogLoaded` 置 false）。
**影响**：用户看到 120px 完全空白的空洞，弱网下误以为面板坏了。
**修复**：`app.use(ElLoading)` + 引入样式；或改成不依赖指令的写法。

**【已修】✅ P1-25 进度浮层 z-index 60 被 Element Plus 弹层（2000）压住，事故面板触发的长任务没有任何可见反馈**
`theme.css:2065` 的 `.progress-overlay { z-index: 60 }` vs `.el-overlay { z-index: 2000 }`；`IncidentModal.vue:76` 与 `LogModal.vue:91` 都是 `<el-dialog>`。同因还导致 `theme.css:2202/2225` 的标题栏活动脉冲（1000）在弹层打开时不可见。
**触发**：启动失败 → 事故面板 → 点「移除插件」确认（`IncidentModal.vue:119-129`）→ `withProgress` 跑 pnpm 卸载几十秒。
**影响**：进度浮层被完全盖住（两者同宽同位置），用户点完确认后**屏幕毫无变化**；失败时按约定进度面板要保持开放由用户手动关闭，但它的文案、日志和「关闭」按钮都在遮罩下点不到。这两个按钮也没有 `:loading`/`:disabled`，可被反复点击。
**修复**：`.progress-overlay` 提到 3000 以上；`IncidentModal` 动作按钮补 loading 与 disabled。

### 构建、发布与文档

**【已修】P1-26 / P1-27 / P1-28 发布链路三处加固**（`.github/workflows/desktop-release.yml`、`docs/release.md`、`AGENTS.md`）

1. **P1-26 串行化发布**：`concurrency.group` 由 `desktop-release-${{ github.ref }}` 改为固定的 `desktop-release`。以 ref 分组时 tag push（`refs/tags/…`）与手动 dispatch（`refs/heads/main`）落在两个不同的组，同一版本可以并发跑两条完整流水线，后启动的那条会在 `Create or reuse release` 步骤撞车失败——rc.7 与 rc.13 都真实发生过，每次都白烧一次 20~30 分钟的双平台构建。
2. **P1-27 版本单调性守卫**：`preflight` 在解析版本之后拉取线上 `releases/latest/download/latest.json`，用内联 node 实现 semver 比较并断言待发布版本**严格大于**它。GitHub 的 `releases/latest` 取「最近创建的非 draft 非 prerelease release」而非 semver 最大值，给旧线发 hotfix 会让端点后退，而 Tauri 只在 `release.version > current_version` 时提示更新——高版本用户从此静默收不到更新且无任何步骤能发现。线上没有可读的 `latest.json`（首次发布）时自动跳过。比较逻辑已用 6 组版本对本地验证：`rc.19>rc.18`、`0.1.2>0.1.2-rc.18`、`1.0.0>0.9.9` 放行；`rc.18<rc.19`、`0.1.1<0.1.2`、`rc.9<rc.10` 拦下。
3. **P1-27 端到端校验**：`publish` 末尾新增 `Verify published release` —— 先用 `gh api …/releases/{id}/assets` 读回**真实**资产集合并断言恰好 6 个（此前只数本地目录、且只按文件名删同名资产，复用人工建过的 draft 时可能带出多余资产，即 P2-46 的检测部分），再拉取更新端点断言它报告的版本就是本次发布的版本、且每个平台资产都能以 HTTP 200 下载。
4. **P1-28 供应链**：15 处 `uses:` 全部固定到 40 位 commit SHA（后缀注释为对应版本，如 `actions/checkout@d23441a… # v6.1.0`，由 Dependabot 升级）。build job 会把 `TAURI_SIGNING_PRIVATE_KEY` 放进构建步骤的 environment，一个被投毒或被重指 tag 的第三方 action 足以读走私钥，从而为任意载荷签名。
5. 文档同步：`docs/release.md` 新增「并发与版本单调性」一节，`AGENTS.md` 的发布触发段补充固定并发组、单调性断言与 SHA pin。

**验证**：workflow YAML 解析通过；15 处 `uses:` 全部为 40 位 SHA；preflight 与 publish 的新步骤 shell 脚本经 `bash -n` 检查、node 内联表达式经构造 manifest 烟测（含空 `platforms` 分支）；比较逻辑 6 组用例全部符合预期。注意 publish job 跑在 macOS runner（bash 3.2），新步骤刻意避开 `mapfile` 等 bash 4+ 特性。

**【已修】P1-26 并发发布防护失灵：tag push 与 main dispatch 不共享 concurrency group，已造成多次发布失败**
`desktop-release.yml:15-17` 的 `group: desktop-release-${{ github.ref }}` 对 tag run 是 `refs/tags/desktop-v…`、对 dispatch 是 `refs/heads/main` —— 两者属于**不同 group**，且 `cancel-in-progress: false`，因此同一版本可并发跑两条完整流水线。
**已发生的证据**（GitHub API 可见）：run `33306395595`（push tag `desktop-v0.1.2-rc.7`，10:24:19Z）成功，6 秒后 run `33306399079`（同一 commit 的 `workflow_dispatch`）在 quality/build 全绿之后于 `Create or reuse release` 步骤失败；`rc.13` 也有两次同类失败（`33835186833`、`33837684943`），都只在同一步失败。
**根因**：该步骤的判定叠加了三件事 —— `select(.tag_name == TAG or (.draft == true and .name == NAME))`、多于一条即 `exit 1`、已发布即 `exit 1` —— 于是人工建的 draft（同名、tag 为 `untagged-*`）或任何并发 run 都会让整轮 20~30 分钟的双平台构建作废。AGENTS.md 只用文字提醒，workflow 没有强制。
**修复**：① concurrency 改为固定组（`group: desktop-release`，发布频率低，串行代价可接受）；② 该步骤只按 `tag_name` 查、多条取最新，并做成幂等：若"同 tag + 同 SHA + 已发布 + 6 个资产齐全"则直接成功退出。

**【已修】P1-27 rc 以 `prerelease=false` 发布，updater 端点没有单调性保证，旧线 hotfix 会把更新通道顶回去**
`desktop-release.yml:306,320,379` 恒为 `prerelease=false`（AGENTS.md 要求如此），而 endpoint 是 `tauri.conf.json:64-66` 的 `releases/latest/download/latest.json`；GitHub 的 `/releases/latest` 语义是"**最近创建**的非 draft 非 prerelease"，**不是** semver 最大值。线上实测：`releases/latest` 现为 `desktop-v0.1.2-rc.18`，`draft=false/prerelease=false`，恰好 6 个资产。
**触发**：先发 `0.1.3-rc.1`，再为旧线发一个 hotfix（如 `0.1.2-rc.19`）→ latest 后退到 `0.1.2-rc.19`；Tauri 只在 `release.version > current_version` 时提示更新（`tauri-plugin-updater-2.10.1/src/updater.rs:531-532`），于是 `0.1.3-rc.1` 的用户**再也收不到任何更新提示**，且没有任何 CI 步骤能发现。
**修复**：① preflight 里断言新版本 > 当前线上 `latest.json` 的 version（一行 curl）；② 更彻底：rc 走 `prerelease=true` + 单独的滚动通道 release 承载 `latest.json`；③ publish 末尾加发布后校验（curl endpoint 并断言 version 与资产 URL 200）。

**【已修】P1-28 全部 actions 使用可变大版本 tag，而 build job 持有更新签名私钥**
`desktop-release.yml` 中 13 处 `uses:` 全部是 `@vN`（这些 tag 目前都存在，问题在于**可变引用**）。第三方 action（`pnpm/action-setup@v6`、`Swatinem/rust-cache@v2` 等）被投毒或 tag 被重指后，可以在检出目录里改 `vite.config.mjs` / `build.rs` / `package.json`，或留一个后台进程等待 `tauri build` 步骤启动后读取其 env —— 而 `TAURI_SIGNING_PRIVATE_KEY` 正在该步骤的 env 中。
**影响**：私钥泄露 = 攻击者可为任意载荷签名，所有已安装用户的 updater 都会通过签名校验并安装恶意制品。这是本流水线最高的单点风险。
**修复**：所有 `uses:` 固定到 40 位 commit SHA（附 `# vX.Y.Z` 注释，Dependabot 可自动升级）；私钥只在最小步骤内出现。

**【已修】✅ P1-29 README / troubleshooting 的默认端口 3080 与代码 3090/3091 矛盾（排障指引错误）**
`README.md:31`（`--port 3080`）、`README.md:52`、`README.md:162`、`README.md:177`、`docs/troubleshooting.md:15` 都写 3080；代码是 `kernel.rs:60` 的 `DEFAULT_PORT = if debug { 3091 } else { 3090 }`（`docs/architecture.md:74-79` 与 `capabilities/harness-remote.json` 也是 3090/3091）。3080 是 **dsh 内核自身**的默认端口，shell 一直覆盖它。
**影响**：用户按 README:177「3080 被占用就改端口」去排查，实际冲突在 3090，问题依旧；支持方按 troubleshooting:15 `curl 127.0.0.1:3080` 得到连不上，会误判成 WKWebView 环回问题。
**修复**：五处统一改为「release 3090 / dev 3091（可在设置页修改）」。

---

## P2 —— 改进项，可按主题批量处理

### 进程、日志与孤儿回收

| # | 位置 | 问题 | 建议 |
| --- | --- | --- | --- |
| 【已修】P2-1 | `commands.rs:564-567`、`lib.rs:241-249` | `register_child` 用 `replace` 丢弃旧 `Child` 且从不 `wait` → Unix 僵尸进程；`kernel_running()` 只看 `state.running.is_some()`，内核早已死亡仍报"运行中"，关窗时弹出与实际矛盾的确认为 | `replace` 前 `try_wait`；`get_status`/`kernel_running` 对持有句柄做一次 `try_wait`，已退出即清空 | ✅ 已修：`register_child` 改走 `replace_child_slot` —— 旧句柄先 `try_wait`（等价一次 `waitpid`，回收僵尸）；仍活着则交给后台线程 `wait` 并回报诊断，不再丢句柄。`kernel_running`/`status()` 本就走 `try_wait` + `workbench_running` 活体判据，已核对无残留。新增 2 条 Unix 测试：反证 A（丢句柄不回收）两条全挂、反证 B（不交后台线程）live 用例挂；第一版探测用 `waitpid` 自我回收导致无区分度，已改用 `kill(pid, 0)`
| P2-2 | `kernel.rs:1187-1193` | Windows 分支 `reap_orphans` 是显式 no-op，壳崩溃后内核永久残留并占端口（macOS 有回收） | `AssignProcessToJobObject` + `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`；补 Windows 孤儿扫描 |
| 【已修】P2-3 | `process.rs:498-503`、`:880` | 日志写入失败被完全吞掉（drain 线程首次出错即 break）→ 会话中途日志静默死亡，Incident 仍引用该日志路径 | drain 遇错不退出，累计错误并上报「日志写入失败：{e}」 | ✅ 已修：抽出可测的 `drain_stream` —— 写入失败**不再中断排空**（旧实现 `break` 后子进程管道会被填满，内核卡在写日志上；日志还静默死亡），改为累计诊断（进程级 `LOG_WRITE_ERROR`，读走即清，同时打到 stderr），并把诊断接入 `guarded_start` 的 trail，让事故面板说明"本次日志可能不完整"。新增 3 条测试（含并发串行锁，避免共享诊断槽互相偷读），反证（写入失败即 return）挂 2 条
| 【已修】P2-4 | `process.rs:295-299` + `commands.rs:279` | 轮转备份命名为 `X.log.1`（扩展名变 `1`），而 `list_log_files` 只收 `.log` → 前 8 MiB 日志在面板中不可见 | ✅ 已修：`rotated_log_path` 改为把代次插在扩展名之前（`X.1.log`），扩展名仍是 `log`，面板与 `read_log_file` 天然可见；列举逻辑抽成可测的 `collect_log_entries`，排序改为「基名逆序 + 代次升序」（新 → 旧，旧实现纯字典序会把 `.2.log` 排到 `.1.log` 前）；启动时一次性把旧命名 `X.log.<n>` 迁移成 `X.<n>.log`（目标已存在则跳过，不覆盖更新的那份）。新增 9 条测试（列举/排序/轮转两代/迁移含冲突与目录缺失），并以反证确认「轮转命名」「列举含备份」「迁移不覆盖」三条都有区分度（220 通过） |
| P2-62 | `commands.rs:288-296`（本次新增记录） | logs 目录**没有任何保留策略**：每个「日期 × kind」留 `KERNEL_LOG_BACKUPS + 1 = 3` 代 × 8 MiB，但日期只增不减 → 每天最多新增 24 MiB，长期使用可累积到 GB 级；且历史注释引用的 `cleanup_legacy_logs` 函数已不存在（本次已顺手改掉该误导性注释） | 启动时按总大小/天数做一次保留清理（例如保留最近 14 天或 200 MiB），并在文档里写明保留窗口 |
| 【已修】P2-5 | `kernel.rs:1310-1330`、`:1424-1427` | Windows 端口反查用子串匹配（`:3090` 会命中 `:30900`）取第一条 LISTENING → 选错 pid → `pid_is_kernel` 返回 false → `stop_kernel` 静默不动却报成功 | 按列解析 netstat：本地地址列以 `:{port}` 结尾且状态列为 LISTENING。**后果已被 P0-1 / P2-29 部分缓解**：`stop_kernel` 现在无条件清 pid 并如实上报，`start_maybe` 也会对无关占用者报错，因此剩下的症状是「误报端口被占用 / 找不到在跑的内核」，不再是「静默报成功」 | ✅ 已修：抽出平台无关的 `parse_netstat_listener_pid`，按列解析（TCP + 恰 5 列 + 本地地址以 `:PORT` 结尾 + LISTENING），`:3090` 不再命中 `:30900`。新增 3 条测试（含真实中文 Windows 输出形状），反证（退回子串匹配）命中
| 【已修】P2-6 | `guard.rs:466-478` | 端口被**无关进程**占用时直接返回 `running: true`，不校验监听者身份 → 面板显示"运行中"，`open_harness` 可能把无关网页当工作台打开 | 预检时用 `pid_is_kernel` 验证监听者，不是内核则报「端口被占用」。**已修部分**：`guarded_start` 与状态判据改走 `kernel::workbench_running` → `workbench_pid`，两条分支（pid 文件 / 端口反查）都要求 `pid_is_kernel` 成立，无关占用者不再被当成"运行中"。**残留**：`commands.rs:909` 的 `open_harness` 仍只判断 `port_open(settings.port)`，无关进程占着端口时会照旧把这个端口当工作台打开 → 下一轮按「监听者是已知的非内核进程才拒绝，pid 无法判定时保持宽松」收口 | ✅ 已修（残留已收口）：`open_harness` 增加 `harness_port_conflict(port, kernel::port_listener_identity(port))` —— **只在确证监听者不是内核时**拒绝并给出下一步；身份未知（端口空闲、查不到 pid、命令行读不出）保持原有宽松行为，避免「内核在跑但 pid 反查失败」打不开工作台。新增 `ListenerIdentity` 三态与 `command_is_kernel`，含 Windows 反斜杠/大小写与近失用例
| 【已修】P2-7 | `guard.rs:288-298`、`:521`、`:856-860` | 归因 needle 含 `format!("/{}", item.name)`，命中的是路径段前缀：插件名短（`main`/`ui`/`x`）时一条 `GET /assets/main.js 500` 错误行即可把它写进 quarantine 并停用 | 删掉 `/{name}`，改按路径段精确匹配，并要求命中行同时出现强锚定（`plugins/<id>`/`node_modules`/引号包名） | ✅ 已修：删掉裸 `/<name>` 子串规则，改为锚定形态 + **路径段边界**（`plugins/<id>`、`node_modules/<name>`、带引号包名），`main` 不再命中 `main-utils`；旧实现下 `GET /assets/main.js 500` 会把插件写进 quarantine。新增 4 条测试，反证 A（退回裸子串）挂 3 条、反证 B（去掉边界检查）挂 2 条
| 【已修】P2-8 | `process.rs:1065-1084` | `read_tail` 先 `metadata` 再 `open`：撞上日志轮转时 `seek` 越界 → 返回空 tail → `attribute` 无输入 → 触发 P1-3 的全量隔离 | 先 `open` 再对已打开 fd 取 metadata；偏移越界时从头读；空结果短重试一次 | ✅ 已修：改为先 `open` 再对**已打开的 fd** 取长度（`read_tail_from`），并按「读空且起点>0 就退回从头读」兜底，另有 1 次 20ms 短重试覆盖轮转瞬间。新增 4 条测试，两项反证通过（去掉兜底 / 退回先 metadata 再 open 各挂 1 条）

### 数据与恢复

| # | 位置 | 问题 | 建议 |
| --- | --- | --- | --- |
| 【已修】P2-9 | `patches.rs:648-703`、`:850-861` | 目标已是补丁后内容时仍把它当"原文件"备份，撤销后文件内容不变却报告「已撤销」→ 壳声称已撤销、内核仍在跑补丁代码且无记录 | 无对应应用记录时不要备份，直接报错并给出下一步 | ✅ 已修：目标已是补丁后内容时不再伪造备份（`commit_file` 直接返回 `had_original=true` + 无备份的记录），应用阶段就把"没有可恢复的原文件"写进注意事项；`revert_one` 的"无备份"分支改为先看 `had_original` —— 只有"应用前不存在"的纯新增文件才删除。**改写**了原本钉住旧行为的测试 `copy_over_existing_identical_records_no_recoverable_original`（旧断言要求存在假备份）。残留：rc.18 之前产生的假备份记录无法追溯识别（备份里就是补丁内容），会在下一批加"载荷哈希 == 记录原文件哈希"的识别告警
| 【已修】P2-10 | `patches.rs:951-965` | `prune_empty_dirs` 注释写"只删到内核根为止"，实现既无 `kernel_root` 参数也无终止条件，会一路向上删空目录（含 `<data_dir>`） | 传入 `kernel_root` 并在该处停止；补单测 | ✅ 已修：`prune_empty_dirs` 接收 `kernel_root` 并在该处停止（同时拒绝根以外的路径），`handle_missing_backup` 透传根参数；新增 2 条测试（根内空目录被清、根本身与其父保留；根以外一个都不动）
| 【已修】P2-11 | `patches.rs:620-637`、`:308-354` | 补丁源路径 `from` **完全没有**越界校验（`join("../../../../etc/passwd")` 可用，绝对路径会丢弃 `patch_dir`），与 `docs/patch-management.md:20` 的前提不符 | 对 `from` 复用 `check_target_path` | ✅ 已修：`validate_def` 对 `from` 复用 `check_target_path`，拒绝 `../` 越界与绝对路径；新增 2 条测试（`../../../../etc/passwd` 与 `/etc/passwd` 均被拒绝、正常相对路径仍可加载）
| 【已修】P2-12 | `patches.rs:260-303` | 清单校验失败（缺 manifest / JSON 坏 / schemaVersion 不符）只 `eprintln` + `continue` → 该补丁在设置页直接消失，用户无法区分"本版本没带"与"清单坏了" | 经 `patch_status` 的 warning 暴露给 UI | ✅ 已修：`load_patches_with_warnings` 收集被跳过的清单/定义原因（缺 manifest、JSON 坏、schemaVersion 不符、定义非法），`patch_status` 把它们并入 `PatchStatus.warning`，设置页已有的告警条直接展示；`load_patches` 收为测试专用包装。坏掉的补丁不再无声消失
| 【已修】P2-13 | `patches.rs:732-746` | `file.search.as_deref().unwrap_or("")`：`search` 为 null 时 `replace("", repl)` 会在每个字符间插入替换串，必然损坏目标 JS | `search` 缺失时直接返回错误 |
| 【已修】P2-14 | `patches.rs:997-1004` | `status` 只遍历当前清单定义 → 定义已被移除的历史补丁记录在 UI 中完全不可见（`revert` 其实支持） | status 额外渲染"定义已移除、仍可撤销"行 | ✅ 已修：`status` 追加 `orphan_record_rows` —— 定义已不在清单里但记录仍在当前激活内核上的补丁会显示为「已应用（定义已移除）」且可直接撤销；新增 1 条测试（清单置空后仍可见并可撤销）
| 【已修】P2-15 | `patches.rs:379-413` | "内核根以内任何祖先是符号链接即拒绝写入"过严：`.pnpm` isolated linker 布局下补丁永远无法应用且文案无下一步 | 判据改为 canonicalize 后仍在 kernel_root 之内 | ✅ 已修：`ensure_no_symlink_ancestors` 的判据从"路径中不能有符号链接"改为"canonicalize 后的真实路径仍在（真实化的）内核根之内"——pnpm isolated linker 的 `node_modules/<pkg> -> .pnpm/<pkg>@<ver>/node_modules/<pkg>` 不再被一刀切拒绝，指到内核之外的链接仍然被拒且错误信息给出下一步。新增 2 条测试（各经反证）
| 【已修】P2-16 | `node_install.rs:371-377` | SHA-256 校验失败时错误文案写「已删除无效文件，可重试」，但代码**没有**删除 tarball（对照解压失败分支 `:381-387` 确实删了） | 真的删除或改名 `.corrupt` | ✅ 已修：校验失败分支真的删除 tarball（此前只有解压失败分支删），与错误文案的承诺一致
| 【已修】P2-17 | `node_install.rs:382-403` | 临时目录名带 pid 只在同 pid 时清理（进程被杀留 ~200 MB）；下载产物 36-52 MB 装完不删；`fs::rename` 到已存在的版本目录在 Unix 报 `ENOTEMPTY` → 一旦 `tools/node/<ver>` 残缺就**永久无法重装**且无 UI 自救入口 | 启动时清理旧 `.node-tmp-*`；成功后删下载产物；失败信息给出「请删除 <path> 后重试」 | ✅ 已修：① 启动安装前清扫**所有** pid 的 `.node-tmp-*`（旧实现只清自己 pid 的，被强杀后约 200 MB 永久残留）；② 发布前先删掉残缺的同版本目录（Unix `rename` 到非空目录会 `ENOTEMPTY`，残缺目录会让安装永远无法恢复）并给出「请手动删除后重试」；③ 安装成功后删除下载产物（36–52 MB）。新增 1 条测试（反证：只清本 pid 时挂）
| 【已修】P2-18 | `node_install.rs:26-36`、`:411-424`、`node.rs:50-59` | 产物平台按 `cfg!(windows)` 二选一（非 Windows 一律 `darwin-x64`），不看 `consts::OS/ARCH`；回滚时丢弃真实 stderr，把原因错写成「当前系统可能低于其最低版本要求」 | 用 `(OS, ARCH)` 映射产物名；保留并回传 stderr | ✅ 已修：产物改由 `artifact_for_platform(OS, ARCH)` 精确匹配（win-x64 / darwin-x64 / darwin-arm64 / linux-x64 / linux-arm64），不支持的组合返回含下一步的错误而不是悄悄下载 x64 macOS 包；`artifact_size_text` 同源，不再对非 Windows 一律报「约 52 MB」；回滚原因的"当前系统可能低于其最低版本要求"改为带出真实探测输出（新增 `node::probe_failure_detail`，stderr 优先）。新增 1 条测试 + 改写产物名测试
| 【已修】P2-19 | `settings.rs:56-62` | `settings::load` 吞掉所有错误：文件损坏 / 读失败 → 默认值，用户自定义端口**无声回退**到 3090/3091，无日志无提示 | 区分不存在与解析失败，损坏时备份并上报 | ✅ 已修：新增 `settings::load_checked` 区分"文件缺失（正常首次启动）"与"损坏/读不出来"——后者备份为 `settings.json.corrupt` 并返回含下一步的中文诊断；`KernelStatus.settings_warning` 透出到面板（VersionsPanel 新增告警条），端口回退不再无声。新增 3 条测试
| 【已修】P2-20 | `plugins.rs:1053-1058`、`:2910-2918` | 中央库发布与 `store.json` 记账非原子 → 孤儿目录无 store 行：面板不显示、「同步」不管、`uninstall` 直接拒绝，只能手删 | 失败路径回滚刚发布的目录；或在 reconcile 里清理"有外壳标记但无 store 行"的目录 | ✅ 已修：`reconcile_store` 末尾新增 `sweep_unrecorded_store_dirs` —— 带外壳 id 标记、名字与标记一致、且 `store.json` 无对应记录的中央库目录会被清理（三重保险：只在 store.json 能正常读出时执行、跳过带暂存前缀的目录、没有标记的目录一律不碰）。**并修掉一个交互缺陷**：本轮恢复流程刚提升的 `.new-*` 目录会被顺手删掉（把救援动作当场撤销），因此跳过本轮 `by_id` 里出现过的 id。新增 3 条测试，两项反证命中（不做清理 / store 损坏时也清理 / 不跳过恢复中的 id）
| 【已修】P2-21 | `plugins.rs:2172`、`:2191-2197` | `specs` 以 `item.name` 为键：同名不同 id 互相覆盖接线，UI 对两行都报 wired=true | specs 以 id 为键，写 manifest 时检测同名冲突 | ✅ 已修：`specs` 改为以**插件 id** 为键的 `BTreeMap<String, WireSpec>`，`wire_manifest` 按 id 顺序取第一个占用包名的条目、其余记录为冲突说明并随 `failures` 上报（用户能看到"装了但没接线"），接线计数按去重后的包名统计；不再出现"后一条静默覆盖前一条、UI 两行都显示已接线"。新增 1 条测试（反证命中）
| 【已修】P2-22 | `plugins.rs:2377-2378`、`:853-863` | 锁定版本的 npm 插件被永久标成「有更新」，但更新必被拒（`pinned` 只在 git 分支生效） | check_updates/status 跳过 `item.pinned` | ✅ 已修：`status` / `check_updates` 跳过 `pinned` 条目（计数与行内角标），npm 与 git 一视同仁；`is_newer_than` 保持原有的"版本号比较"语义不变（git+pinned 的既有测试仍钉住该行为）。新增 1 条 status 层测试覆盖两种来源
| 【已修】P2-23 | `plugins.rs:3221-3223` | `kernel_plugin_list` 把前端传入的 `version` 直接当路径段（`../../..` 可越界枚举读取） | 入口用 `kernel::list_installed` 白名单校验 | ✅ 已在早前版本校验改造中修好：`kernel_plugin_list` 入口用 `crate::version::is_valid_kernel_version` 拒绝 `../` 等形态（`commands.rs:1726`）
| 🚧 部分修复 P2-24 | `plugins.rs:464-482` | `id_for_name` 的 `/`→`__` 映射不是单射：npm `owner__repo` 与 git `owner/repo` 撞同一 id，互相覆盖源码与 store 行 | 转义 `_` 或追加短哈希后缀 | 🚧 部分修复：`upsert_item_unlocked` 增加**冲突检测** —— 同一个 id 被不同 `source`/`name` 复用时拒绝并给出"先卸载"的下一步，不再静默覆盖前一个插件的源码与记录（新增 2 条测试，反证命中）。**残留**：`/`→`__` 的映射本身仍不是单射，彻底修需要换 id 方案 + 迁移既有安装（store 行、`kernels/<ver>/plugins/<id>`、`.dsh-xlink-meta`），单独立项处理

### 命令面与配置

| # | 位置 | 问题 | 建议 |
| --- | --- | --- | --- |
| 【已修】P2-25 | `lib.rs:111`、`commands.rs:240-250` | `get_kernel_log` 命令零调用点（UI 与文档均无引用），与 `read_log_file` 功能重叠 | 删除，或补上入口 |
| 【已修】P2-26 | `commands.rs:218` | 唯一一处用 `state.node_cache.lock()` 而非 `crate::lock()`：锁被毒化时静默跳过清缓存 | 统一 `crate::lock` |
| 【已修】P2-27 | `commands.rs:173-185` | `cached_node` 在持有 `node_cache` 锁期间执行 `node::resolve`（会 fork `node --version`），锁粒度过大 | 探测在锁外完成 | ✅ 已修：`cached_node` 命中缓存即返回，未命中先**释放锁**再 `node::resolve`（探测会派生 `node --version`，持锁期间会堵住状态轮询里的其它调用）
| 【已修】P2-28 | `commands.rs:962-991` | `open_log_window` 是同步命令且失败只 `eprintln`，不返回给 UI；日志名校验在 3 处重复 | 统一为带 mpsc 等待的异步命令 + 抽 `validate_log_name` | ✅ 已修：日志名校验合并为唯一的 `validate_log_name`（此前在读文件与开窗三处各写一遍，新增 1 条测试 + 反证）；`open_log_window` 由同步命令改为异步命令，建窗结果经 mpsc 回传（20 秒超时），失败不再是"只 eprintln、UI 收到成功"——用户点了「全屏」却毫无反应且无提示。UI 侧改为优先展示后端文案（已含下一步）
| 【已修】P2-29 | `commands.rs:629-678` | `stop_kernel` 在 `kernel::stop` 失败时提前 return，跳过 `clear_pid`，且 harness 窗口已 destroy → "窗口已关、内核仍在、pid 残留" | 用 finally 语义确保 `clear_pid` |
| 【已修】P2-30 | `kernel.rs:94,439` | `KernelStatus::ever_installed` 是死字段（UI 用 `installed.length === 0` 自行推导） | 删除字段或让 UI 使用它 | ✅ 已修：删除死字段 `KernelStatus::ever_installed`（UI 一直用 `installed.length === 0` 自行推导）
| 【已修】P2-31 | `Cargo.toml:3` | `version = "0.1.0"` 与 `package.json`/`tauri.conf.json` 的 `0.1.2-rc.18` 不一致（CI 只校验后两者）。**已有实际影响**：`releases.rs:27` 的 `concat!("dsh-xlink/", env!("CARGO_PKG_VERSION"))` 是 npm registry / GitHub 的 User-Agent，外壳对外自称 `dsh-xlink/0.1.0`。**潜在结构性风险**：`package_info().version` 优先取 `tauri.conf.json` 的 version，一旦该字段被删就会回退到 `CARGO_PKG_VERSION`，所有已装 rc.18 的应用会自报 0.1.0 并永远认为 rc.18 是新版本 → 无限更新循环 | 同步该字段，或删除它并注明以 config 为准；preflight 纳入第三个版本文件 | ✅ 已修：`src-tauri/Cargo.toml` 版本 0.1.0 → 0.1.2-rc.18（`Cargo.lock` 随之更新），`env!("CARGO_PKG_VERSION")` 拼出的 User-Agent 不再对外自称 0.1.0；preflight 增加第三个版本文件的断言，`check-invariants.mjs` 也新增"三处版本一致"检查（反证：改回 0.1.0 即报错）
| 【已修】P2-32 | `registry.rs:14-29` | 默认 registry 是第三方镜像且 `DSH_NPM_REGISTRY` 可任意覆盖，与 AGENTS.md 声明的信任边界存在口径差异；node 下载的期望摘要也在安装时从同一站点取（只防传输损坏，不防镜像投毒） | 在 AGENTS.md 明确镜像策略；如需强保证，把期望摘要编进签名包 | ✅ 已修（文档口径）：AGENTS.md 的信任边界补充说明——npm 基础 URL 默认指向 npmmirror **镜像**，可用 `DSH_NPM_REGISTRY` 覆盖；镜像只影响取源，包名仍限定 `@deepseek-ai` 命名空间，tarball 仍按 `dist.integrity`（SRI）逐字节校验、失败即删除
| 【已修】P2-33 | `releases.rs`（360 行，0 测试） | 承担"npm 优先、GitHub 回退"的信任边界与 latest 选择，却没有单元测试 | 补 `rank_releases` 与回退逻辑单测 | ✅ 已修：把三级回退链抽成接收**惰性**闭包的 `select_releases`，新增 6 条测试——npm 成功时下游源一次都不许被调用（闭包 panic 兜底）、空 npm 结果回退到 API、API 失败回退 Atom 且警告带上两侧原因与 prerelease 不完整、三源全失败时错误信息包含每个原因、缓存命中与失败不缓存。**顺带修掉一个真实缺陷**：npm 返回 200 但版本列表为空时旧实现直接报错，改为与失败同等对待并继续回退
| 【已修】P2-34 | `releases.rs`、`store.js:211-225` | `fetch_releases` 无 TTL 缓存，每次「检查更新」都重新打网络，首装引导还会立刻重复拉一次 | 统一缓存策略 | ✅ 已修：`list_releases` 增加 60 秒进程级 TTL 缓存（只缓存成功结果，失败立刻可重试），首装引导启动时连拉两次不再打两遍完整回退链；缓存逻辑抽成 `cached_or_fetch` 以便测试（两条用例 + 三条反证命中）

### 界面

| # | 位置 | 问题 | 建议 |
| --- | --- | --- | --- |
| 【已修】P2-35 | `SettingsPanel.vue:118,121` | 两个 IO 按钮缺 `:disabled="globalBusy"`：互斥租约期间点「保存设置」被静默丢弃（无 toast），2.5s 后轮询把输入框回滚，用户以为已保存 | 补 `:disabled`，并在动作开头显式提示"有其它任务正在进行" | ✅ 已修：设置页「保存设置」「检测 Node.js」补 `:disabled="globalBusy"`，互斥租约期间不再静默丢弃点击后被 2.5s 轮询回滚
| P2-36 | `store.js:136,222,256,268,374,400,421,485`；`plugins.js:129,240`；`skills.js:94`；`logs.js:43,74`；`patches.js:49,73` | 错误提示普遍缺「下一步 + 日志路径」，英文 reqwest 原始错误直出（全项目仅 3 处符合约定） | 在 `notify.js` 增加 `toastActionError(prefix, e, nextStep)` 并逐点替换 |
| 【已修】P2-37 | `VersionsPanel.vue:18,40-55` | 组件内直接 `invoke`（违反"状态与动作集中在 store"），且 catch 里也把 `slot.loaded = true` → 启动失败被永久缓存，tooltip 不再重试 | 失败不置 `loaded`；逻辑搬到 `plugins.js` | ✅ 已修：`kernel_plugin_list` 读取失败时不再置 `slot.loaded = true`，失败不会被永久缓存，再次悬浮会重试并显示错误
| 【已修】P2-38 | `logs.js:15,31,33-48`、`LogModal.vue:114` | 全局单 loading 标志 + `withLoading` 吞掉重入：大日志下切签再点「刷新」什么都没发生 | 按文件粒度绑定 key；`loading` 改为请求序号 | ✅ 已修：日志读取改为请求序号 + 按文件名记录加载态，去掉 `withLoading` 的重入吞并——大日志未读完时再点「刷新」会真正重发，且慢的旧响应不会覆盖新结果；失败文案补上重试与 logs/ 路径。新增 UI 测试（反证：退回重入忽略即失败）
| 【已修】P2-39 | `WindowTitleBar.vue:6-12`、`LogViewerWindow.vue:34-38` | 直接读 `window.__TAURI__.window` 且 `.catch(() => {})` 吞掉失败 → 关闭按钮可能毫无反应也无提示 | 在 `bridge.js` 暴露窗口操作并在失败时 toast | ✅ 已修：`bridge.js` 暴露 `windowAction`/`hasWindowControls`，标题栏与日志窗口不再直接读 `window.__TAURI__`，失败改为 toast 并给出系统快捷键出路（旧实现 `.catch(() => {})` 让按钮点了毫无反应）；`__TAURI__` 现在只出现在 bridge.js
| 【已修】P2-40 | `main.js:64-83`、`App.vue:190-205` | 没有全局渲染错误兜底：`get_status` 形状变化导致渲染期 TypeError 时，面板永久空白且无提示 | 设 `app.config.errorHandler` + `onErrorCaptured` 兜底块 | ✅ 已修：新增 `ui/src/errors.js` + `app.config.errorHandler`，渲染期错误进入响应式状态并在 App.vue 顶部渲染兜底块（重新加载面板 / 忽略并继续）；此前一次 TypeError 会让面板永久空白且无提示。新增 UI 测试（反证：不记录即失败）
| 【已修】P2-41 | `PluginsPanel.vue:157-163` | 插件来源 chip 直接渲染英文 `npm`/`git`/`local`（技能页有中文映射） | 把 `originLabel` 提到共享位置 | ✅ 已修：新增 `ui/src/labels.js` 承载 `originLabel`（未知来源原样返回），技能页改为再导出、插件页改用中文标签，两个页面不再一个中文一个英文
| 【已修】P2-42 | `OverviewPanel.vue:165` | `installNode` 按钮用 `:loading="progress.visible"`（全局进度窗可见性）而非约定的 `isLoading(key)` | 走 `withLoading` | ✅ 已修：概览页「自动安装」按钮改绑 `isLoading('installNode')` 并经 `withLoading` 包裹，不再跟随全局进度窗可见性（任何长任务都会让它转圈）
| 【已修】P2-43 | `OverviewPanel.vue:205-230` | 概览页主操作是「工作台 / 官方对话 / 查看日志」三按钮同排，与 AGENTS.md 的"只暴露单按钮状态机、其余为次级入口"漂移 | 收敛 UI 或更新 AGENTS.md | ✅ 已修（文档口径）：AGENTS.md 的概览页约定改为与实现一致——「启动/关闭工作台」是唯一主按钮，「打开工作台窗口 / 打开官方对话 / 查看日志」是并列次级入口
| 【已修】P2-44 | `ui/src/skills.js` 全文 | `skill_set_enabled` 无任何 UI 调用点，而 `docs/skill-management.md:84-86` 描述了完整启停功能（README:56 承认 v1 面板只有安装行） | 统一文档口径，或接线 | ✅ 已修（文档口径）：`docs/skill-management.md` 的「启用 / 禁用」补上面板现状说明——后端命令 `skill_set_enabled` 与对账语义已完整，v1 面板尚未接线启停按钮（与 README 一致），不再让读者以为面板已有启停

### 构建与 CI

| # | 位置 | 问题 | 建议 |
| --- | --- | --- | --- |
| 【已修】P2-45 | `package.json:24-29` vs workflow:107-117 | `test:titlebar-pulse`、`test:session-perf`、`test:escalation-same-mode` 三个测试入口**不在 CI 中运行**；`smoke-pullstring.mjs` 与 `verify-dsh-file-perf.mjs` 连 npm script 都没有。其中 `titlebar-pulse`（hermetic，实测 7/7 通过、<0.4s，守护 README 承诺的"空闲不保持 WebKit 帧循环"不变量）与 `smoke-pullstring`（实测 exit 0）完全可离线跑却没有 job 跑它们 → 注入到远程页面的脚本与三个自研内核补丁缺乏自动化门禁 | quality job 增加 `test:titlebar-pulse` 与 `node scripts/smoke-pullstring.mjs`；补 `test:file-perf` 入口 |
| 【已修·部分】P2-46 | workflow:308-321,341-365 | 复用 draft 时只按**文件名**删资产，且「必须 6 个」只数本地 `artifacts/` 目录，从不读回 Release 的实际资产列表 → 若同一版本先有一次运行上传过命名不同的资产（例如人工放进 draft 的 `dsh-xlink-0.1.2-rc.18.dmg`），正式 Release 会带 7 个资产，而 AGENTS.md 要求恰好 6 个 | 上传后 `gh api releases/$ID/assets` 读回名字集合，断言恰好 6 个（多余的直接 DELETE）再 `draft=false` |
| 【已修】P2-47 | `scripts/verify-dsh-session-perf.mjs:113-132`、`scripts/verify-dsh-file-perf.mjs:242-249` | 两个内核补丁验证脚本只比较 SHA / 版本范围，没有"当前内核不是锚定版本就跳过行为测试"的分支（对照 `verify-dsh-escalation-same-mode.mjs:96-110` 有这个分支）。实测在活动内核 0.1.5-rc.1 上直接崩栈（`SyntaxError: … does not provide an export named 'DEFAULT_PREPARED_SESSION_CACHE_SIZE'` / 内核自身 lib 的 `TypeError`），而 `docs/patch-management.md:226-229,259-261` 正把它们写成验证手段 → 维护者看到的是"补丁坏了"而不是"当前内核不适用" | 行为检查前加版本门（不适用则打印并 exit 0，除非显式传内核根目录或 `--require-applied`）；行为检查包进 try/catch 并汇总退出码 | ✅ 已修：两个 verify 脚本都加了"当前内核不是锚定版本就跳过"的版本门，并把行为检查包进 try/catch 汇总退出码（实测：活动内核 0.1.5-rc.1 上默认模式从**崩栈 exit 1** 变为跳过并 exit 0；显式传内核根目录或 `--require-applied` 仍严格失败，但只打印一行诊断而不是栈）。修的过程中先写错成 `failures.push`（该变量是计数器），已改为 `failures += 1`
| 【已修】P2-48 | `README.md:3` | 徽章用 `badge.svg?event=release`，而 workflow 只监听 push(tags) 与 `workflow_dispatch` → 该 URL 实测返回 `no status`，去掉参数后返回真实状态（当前为 `failing`），一个本该暴露发布流水线红灯的信号长期失效 | 删掉 `?event=release`（或改为 `?event=push`） | ✅ 已修：README 徽章去掉 `?event=release`（该参数实测返回 no status），改为默认即反映最近一次运行
| 【已修】P2-49 | `tauri.conf.json:26-29` | `csp: null` + `withGlobalTauri: true` 让面板 webview 失去第二道防线。今日 `ui/src` 与 `index.html` 中无任何 XSS sink（纵深防御问题，非已发生的漏洞），但一旦出现 sink（社区目录、插件/技能元数据、内核日志、registry 响应都是外部字符串），无 CSP 意味着 payload 直接执行，而面板命令集包含 `plugin_install`（接受 git URL，安装流程会跑插件自己的 `prepare`）、`install_kernel`、`patch_apply` → 以用户身份任意代码执行 | 配置真实 CSP；`withGlobalTauri` 仅注入脚本需要，可收窄到必要窗口 |
| 【已修】P2-50 | `docs/release.md:13` | 称手动 dispatch 会自动补 tag，与 AGENTS.md「不要用手动 dispatch 创建缺失的 tag」相冲突 | 统一文档口径 |
| 【已修】P2-51 | workflow:187-191 | 流水线对「CI 私钥与 `tauri.conf.json:63` 公钥是否成对」没有任何校验 —— 密钥轮换时若忘记同步公钥，会发布出所有客户端都验签失败的更新（且直到用户更新时才暴露） | 发布前用私钥签名一个测试载荷并用配置公钥验签 | ✅ 已修：新增 `scripts/check-signing-keys.mjs` —— 用 CI 私钥通过 `tauri signer sign` 签一份测试载荷，再用 `tauri.conf.json` 的 `plugins.updater.pubkey` 做 Ed25519 验签（node:crypto，不依赖 minisign），并比对 key id；公钥字段是"整个 minisign 公钥文件再 base64"，脚本会先剥外层。发布 workflow 的 build job 在**打包之前**执行该校验（无密钥时跳过）。新增 5 条测试覆盖真实公钥形态、轮换后不匹配、伪造 key id、篡改载荷与畸形输入，三项反证命中
| 【已修】P2-52 | `.github/workflows/`（仅 1 个文件） | 没有 push/PR 触发的 workflow：日常提交只有在打 tag 时才第一次跑 lint/测试 | 增加 push/PR 的轻量 workflow（fmt + clippy + cargo test + test:ui） | ✅ 已修：新增 `.github/workflows/desktop-ci.yml`（push main / PR / 手动触发，并发组取消旧运行，`permissions: contents: read`，所有 `uses:` 固定 commit SHA），步骤与发布 workflow 的 quality job 对齐：不变量 / test:ui / test:scripts / smoke-pullstring / UI 构建与包体预算 / fmt / cargo test / clippy。日常提交不再等到打 tag 才第一次跑门禁
| 【已修】P2-53 | `scripts/install.mjs:21-26,49-52` | `run()` 用 `execFileSync` 且**没有** `stdio: 'inherit'`（第 49 行注释声称已设置，与实际不符）→ `pnpm install` 的进度输出全部被捕获丢弃，用户执行 `npm run deps` 后长时间无任何输出；且 `execFileSync` 默认 `maxBuffer` 为 1 MiB，依赖较多时输出超限会以 `ENOBUFS` 失败并给出难以理解的错误 | ✅ 已修：`run()` 透传 `opts`，安装调用传 `{ stdio: 'inherit' }`（实时输出 + 消除 `maxBuffer`）；catch 里把只打印 `err.status ?? '?'`（ENOENT/ENOBUFS 时退化成没有诊断价值的「退出码 ?」）改为打印 `err.message` 并补下一步。新增 `scripts/install-stdio.test.mjs`（1.8 MB 输出完整透传 / ENOENT 带出原因 / 非零退出码原样透出），三条均经反证；新增 `test:scripts` 入口把 `scripts/*.test.mjs` 全量（10 条）接入 CI |

### 注释与文档一致性

| # | 位置 | 问题 |
| --- | --- | --- |
| 【已修】P2-54 | `chat-fingerprint.js:31` | 注释称"strip 从 38px 增高到 66px"，实际 `OFFICIAL_CHAT_STRIP_HEIGHT = 38.0`（66px 是工作台 variant）。改为「strip 保持天然的 38px 标签栏高度、嵌入 24x38 紧凑台灯；24x66 拉绳灯只用于 dsh 工作台」，与 `pullstring-launcher.js:8,62-63,121-122` 的两个 variant 对齐 |
| 【已修】P2-55 | `docs/skill-management.md:78` | 称 Windows 用 junction，实现是 `symlink_dir`（需特权）—— 正是 P0-3 的触发条件。随 P0-3 一并落地：`skills.rs` 改走 `mklink /J`（`skills.rs:1142-1151`），文档描述与实现现已一致 |
| 【已修】P2-56 | `docs/plugin-management.md:131` | 称 copy 模式按"大小+修改时间"跳过未变化文件，代码无此逻辑（见 P1-12）。改为如实描述：不做逐文件比对，仅在「记录版本 + 模式 + 目标形态（真实目录而非符号链接）」三者都未变时整包短路（`plugins.rs:1789-1814`），任一项变化即整树重删重拷 |
| 【已修】P2-57 | `docs/plugin-management.md:107` | 要求 copy 更新后重跑 profile install，实现未做（见 P1-13）。P1-13 落地后（`plugins.rs:3146`）文档描述成立，文案无需改动 |
| 【已修】P2-58 | `docs/patch-management.md:255-256` | 记录的 `dsh-session-perf` 哈希与 manifest 的 `expectSha256` 及载荷实际哈希都不一致 → 按文档复验必然对不上。改为与 manifest / `verify-dsh-session-perf.mjs` 一致：锚定 `0.1.2-alpha.3`，原始 `d5ae2c7d…9a00`，v1.2.0 补丁后 `29d2501e…6299`，并注明 v1.0.1 / v1.1.0 旧载荷仍被 verify 脚本识别 |
| 【已修】P2-59 | `docs/patch-management.md:93` | 称校验"应用时执行，而非加载时"，实际 `load_patches → validate_def`（`patches.rs:304`）在加载期执行并静默跳过。改为分两层描述：加载期静态定义校验 + 应用期运行期裁决（目标哈希 / `search` 命中 / 备份占用） |
| 【已修】P2-60 | `lib.rs:49` | `cfg(any(macos, windows))` 分支里的错误文案写死「无法启用 macOS 自定义标题栏」，Windows 上同样打印。改为平台中立表述，并补上后果与下一步（退回系统原生标题栏、功能不受影响、重启或 `npm run dev` 取完整输出） |
| 【已修】P2-61 | `lib.rs:154` | `.expect("failed to build the dsh-xlink app")` 是英文且无下一步指引（启动期崩溃，用户可见）。改为 `unwrap_or_else` + 中文诊断（说明 `generate_context!` 已在编译期烘焙资源清单、指向 `npm run build:ui` 与 `npm run dev`）后 `exit(1)` |

---

## 需长期偿还的机制性债务

1. **`skills.rs` 与 `plugins.rs` 的重复实现已开始分叉**
   `id_for_name`（`plugins.rs:464` 与 `skills.rs:243`，前者已是 `pub` 却另写一份）、npm spec 解析、版本比较、`copy_tree`、暂存目录命名（`.tmp-` vs `tmp-`）各有两份。plugins 侧的 `copy_tree` 会处理悬空符号链接并有测试，skills 侧没有；P1-17 的逃逸问题因此需要修两处。建议下沉到共享模块（`version.rs` / 新 `store_staging.rs`）。

2. **"注册了就要授权""清单定义了就要校验"这类跨文件不变量没有守卫**
   P0-2 与 P2-12 属于同一类问题：一处改动，另一处忘了跟。建议补 `scripts/check-invariants.mjs` 并接进 CI，至少覆盖三项：
   - `generate_handler!` 的命令集合 == `allow-local-commands` 白名单集合（本次 P0-2 正是这条失守）；
   - 每个 capability 引用的权限标识都真实存在（本次通过，但值得固化）；
   - `resources/patches/*/manifest.json` 的所有定义都能通过 `validate_def`，且 `expectSha256` 与载荷实际哈希一致（顺带能发现 P2-52 那类文档漂移）。

3. **状态文件损坏一律按"空"处理，且随后会写回空状态**
   P1-15、P1-23、P2-19 是同一模式：`store.json` / `state.json` / `settings.json` 解析失败 → 静默当空 → 清退磁盘上的真实内容（删链接、删物化目录、清退接线、覆盖文件）。建议统一成"解析失败即备份为 `.corrupt-<ts>`、保留磁盘原状、通过 warning 暴露给 UI、跳过一切清扫动作"。

4. **失败恢复路径普遍缺少"应用内自救"**
   `install_node` 卡在残缺 `tools/node/<ver>`（P2-17）、补丁卡在残留备份（P1-20）、技能卡在同名冲突（P0-3）都是"必须手删某个目录才能继续"，而 UI 只提供"打开数据目录"。建议在设置页补一个"修复/清理"入口。

---

## 建议的处理顺序

1. **P0-1**（改端口 → 会话日志损坏）—— 后果不可逆，且触发动作是日常操作。
2. **P0-2**（`install_node` 未授权）—— 一行修复，解锁首次使用路径，8 个已发布版本受影响。
3. **P0-3**（Windows 技能卸载失效）—— 数据残留 + 内核继续加载已卸载技能。
4. **P1-20 / P1-21**（补丁 apply/revert 卡死）—— 用户最容易撞上、且应用内无出路。
5. **P1-15 / P1-23**（损坏状态文件被静默清空）—— 静默数据丢失。
6. **P1-26 / P1-27 / P1-28**（发布链路）—— P1-26 每次发作都要先烧掉 20~30 分钟的双平台构建，改 concurrency 是几行的事；P1-27 必须在**下一次给旧线发 hotfix 之前**定下来，否则高版本用户会静默失去更新；P1-28 决定签名私钥的暴露面，宜早不宜迟。
7. **P1-2 / P1-8**（版本号未校验、tarball 未校验）—— 安全类，建议在引入远端补丁清单之前完成。
8. **P1-1 / P1-3 / P1-4**（PATH、误归因、30 分钟假失败）—— 影响面广，需要一点设计取舍。
9. 其余 P1 与 P2 按模块批量处理；机制性债务建议单独立项。

---

## 附：确认无问题的方面

- **ACL / 能力面**：45 个命令的注册与授权交叉校验中，除 P0-2 外无其他越权或漏授权；capability 引用的权限标识全部有效；远端 webview 无法触达任何高权限命令；未授予任何 `shell:` / `fs:` / `process:` / `dialog:` 权限。
- **XSS**：`ui/src` 无 `v-html` / `innerHTML` / `eval` / `new Function` / `postMessage` / `localStorage`；远程来源文案全部走文本插值。
- **参数与字段对齐**：UI 的 camelCase 参数（`on_event↔onEvent` 等）与 Rust 签名全部匹配；事件名与 payload 键（`shell-update-available`、`harness-fault`、`request-quit-confirm`）对齐；无静默空字段。
- **注入**：git 参数以 argv 传入（无 `sh -c`），npm 包名有字符白名单（`skills.rs:415-420`），`env.rs` 把 `reg.exe` 固定为绝对路径以排除 PATH 上的同名 shim。
- **归档解包**：`archive.rs` 拒绝越界路径、符号链接、硬链接、特殊文件，限 10 万条目 / 512 MiB，强制唯一根目录，发布前检查同名冲突。
- **原子写**：目标文件、`state.json`、`store.json`、来源标记、`active.txt`、`kernel.pid` 全部走 `atomic_write`，无就地截断。
- **管道与缓冲**：四条子进程路径都是 stdout/stderr 各一线程并发 drain，无"同一线程顺序读两个管道"的死锁；`read_capped_line` 会整行消费超长行只保留 64 KiB + 截断标记；`read_bounded_bytes` 超限即终止进程树。
- **GUI 子进程静默**：所有 spawn 路径都经 `CREATE_NO_WINDOW`，未发现裸 `Command::spawn`。
- **锁**：`lifecycle → store` 锁序全局一致，无 ABBA；`lock_store()` 与 `skills::lock_store()` 的重入路径经专项逐点核对确认不存在（`install_unlocked` 的候选递归走的是 `*_unlocked` 变体）。
- **`data_dir` 一致性**：只有 `lib.rs:53` 调用 `kernel::data_dir(app)` 并存入 `AppState`，其余模块都用 `state.data_dir`；`desktop/` vs `desktop-dev/`、3090 vs 3091 与 `docs/architecture.md` 一致。
- **CI 发布门禁**（`quality` job 内；`preflight` 只做 `Validate source and version`）：`cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings`、`test:ui`、`test:scripts`（`scripts/*.test.mjs` 全量）、`check:invariants`、`smoke-pullstring` 全部执行，并校验两个 manifest 的版本一致与资产数量 5+`latest.json`。
- **发布链路本身是可靠的**（实测线上）：`releases/latest` = `desktop-v0.1.2-rc.18`，`draft=false/prerelease=false`，恰好 6 个资产；线上 `latest.json` 与 `generate-updater-manifest.mjs` 的输出逐字段一致，URL 全部指向真实存在的资产；manifest 的 4 个平台键与 tauri-plugin-updater 的查找顺序匹配；不存在"缺资产也发布"的路径（`if-no-files-found: error` + 资产计数 + 脚本的 `exactlyOne` 都在创建正式 Release 之前失败）；无 `set -x`、无 secrets 落日志。
- **发布脚本输入校验充分**：缺目录/缺文件/重复资产/资产名不含版本/空签名都会报错退出；`parseArgs` 拒绝未知参数；生成的 manifest URL 只来自被校验过的同一批文件。
- **配置一致性**：README/AGENTS/docs 中出现的每个 `npm run`/`pnpm run` 目标在 `package.json` 中都存在；docs 引用的文件路径全部存在；`vite.config.mjs` 的 root/outDir/base/5173 与 `tauri.conf.json` 一致；`.gitignore` 覆盖了 `node_modules`、`ui/dist`、`src-tauri/target`、`gen/schemas` 与 `.codegraph`。
- **内核安装判定**：不以 pnpm 退出码论成败，而是 `bin.js` + `NATIVE_MODULE_CHECKS` + Node 真 `require()` 探针三重判定（`kernel.rs:775-964`）。
- **updater 签名与 Windows 标记顺序**：`Update::download` 在返回字节前完成 minisign 校验；`pending-shell-update.json` 在下载成功之后、安装之前写入；只有当前包版本等于 `target_version` 时才清理旧安装。
- **版本比较语义**：`version.rs:7-50` 对 `-rc.N` 数值比较、发布版 > 预发布版、`v` 前缀等价等形态正确，三处排序复用它，没有第二套实现。已知窄偏差：`1.2.3+build` 会被解析成 `[1,2]` 从而小于 `1.2.3`（npm 上极少见，未单列为缺陷）。
