# 2026-11 优化任务状态表

本文件是 [优化方案任务书](optimization-2026-11-taskbook.md)（同目录原稿）的 OPT-00 交付物，也是本轮任务状态的唯一真值。任务书里的编号、验收标准与依赖保持不变；状态、commit 与证据只记在这里。

## 1 基线与实施边界

| 项目 | 记录 |
| --- | --- |
| 审查基线 | `f448970f`（2026-11-01，任务书编制时） |
| 实施起始 | `f448970f`（`git describe` = `desktop-v0.6.5-1-gf448970f`） |
| 核对基准 | `b9c46d3d`（2026-11-01 docs 提交；`git diff f448970f..HEAD --stat` 只有 `docs/` 三处，代码与审查基线相同） |
| 2026-11-01 记录的发布 | `desktop-v0.7.1-rc.2` → `a3b11ab8`（上一版 `desktop-v0.7.1-rc.1` → `d0d6a650`） |
| 当轮版本 | `package.json` / `tauri.conf.json` / `Cargo.toml` 均为 0.7.1-rc.2 |
| 分支 | `main`（未推送任何本轮改动时先按本地提交处理，推送与发布另行授权） |
| 本机平台 | Darwin x86_64（macOS 26.7.2）；Node v25.9.0；pnpm 10.34.6；rustc / cargo 1.99.1 |
| 2026-11-01 观察到的运行壳 | release v0.6.5，内核 0.2.2-alpha.2，端口 3090（2026-11-01 15:01 启动） |
| 任务书原稿 | `optimization-2026-11-taskbook.md`，sha256 `da22c9334be704337e4db0e25f7455054e8b46d57a77b3921267470cd18512ab` |

**开工核对**：每次开工先跑 `git rev-parse HEAD`。与审查基线一致 → 直接开始；不一致 → 先核任务书「八 审查事实」里对应调用链是否漂移，把结论写进该任务回执再动代码。已修复且证据齐全的项直接关闭，不重复开发。2026-11-01 已按 HEAD `b9c46d3d` 做过一次全量核对（结论见 §3.1），后续开工只需核对与基线的差异。

**核对时的工作区状态**：核对基于 `b9c46d3d`（仅 docs，代码基线 `f448970f`）。当时工作区里**其他会话**的暗色主题改动随后由该会话提交为 `a9612f17 feat(ui): 暗色主题换成方案 B「夜航日志」`（`ui/**` 与 `docs/ui/**`，没有 `src-tauri/` 变更），与本轮任何任务无关。本轮证据读的是 `b9c46d3d` 版本，之后 UI 文件行号可能漂移（Rust 证据不受影响）。提交时只 `git add` 本任务文件。

2026-11-04 文档核对：当前源码版本为 `0.7.4-rc.1`（`af90517c`），上表保留 OPT 开工及分轮发布记录；没有在本次检查中重新探测运行壳或查询远端发布。OPT-01～12 的代码已落库，仍按各回执保留待验证范围，不因版本已更新自动关闭。OPT-06 的物料复用、依赖闭包固化与计划磁盘缓存仍未实现。

## 2 状态与判定规则

状态：`待实施` → `实施中` → `已实现待验证` → `已验证待合入` → `完成`；资源缺失记 `blocked_resource`，不占完成。

- 只有**同一 commit、同一平台、同一产物**上的行为证据能把状态推到「已验证待合入」；fail、skip、not_run 一律不算通过，未执行项必须写进回执。
- 生产入口必须真的改到；源码字符串匹配、编译通过、旧提交上的结果都不算行为验收。
- 改门禁 / 静态检查规则时做目标明确的反向验证：故意破坏被测产物，确认失败原因命中该规则。
- 同一任务只提交自己的源码、测试、文档与回执；多任务合入时由同一人复核调用顺序、锁顺序与资源归属。
- 任务书是只读输入，不把状态回写进任务书；状态只改本表。

## 3 任务总表

P1 = 来源可信性；P2 = 可靠性与验证闭环；P3 = 维护与交付完善。除 OPT-00（本表交付，已完成）外，初始状态均为「待实施」；「下一步」是开工的第一件事，已按 2026-11-01 的核对（§3.1）换成具体入口。

| 编号 | P | 任务 | 依赖 | 状态 | 实施 commit | 证据 | 下一步 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| OPT-00 | 前置 | 锁定实施基线与证据规范 | 无 | 完成 | — | 本文件 §1、§3.1 | 已完成：13 项生产入口已按 2026-11-01 的 HEAD `b9c46d3d` 核对；后续状态只随回执更新 |
| OPT-01 | P1 | 修正 GitHub 与 npm 来源映射 | OPT-00 | 已实现待验证 | `6c11de2e` | [#回执-opt-01](#回执-opt-01) | 待真实网络安装与双平台壳回归（OPT-09）补证；代码已删除 GitHub→npm 隐式替换 |
| OPT-02 | P1 | 统一安全取源与重定向策略 | OPT-00 | 已实现待验证 | `bbd410b0` | [#回执-opt-02](#回执-opt-02) | 待真实重定向服务器 / 私服 / 双平台壳回归（OPT-09）补证；单测只覆盖纯策略函数 |
| OPT-03 | P2 | 修复桥接换代与私有凭据写入 | OPT-00 | 已实现待验证 | `98e3df53` | [#回执-opt-03](#回执-opt-03) | 待真实内核启动失败注入与双平台壳回归（OPT-09）补证；launch 换代回归已覆盖 |
| OPT-04 | P2 | 统一 Unix 进程组停止策略 | OPT-00 | 已实现待验证 | `533f01f3` | [#回执-opt-04](#回执-opt-04) | Darwin debug 真实内核停止已补证（§5）；待双平台壳回归；隔离夹具已覆盖组残留场景 |
| OPT-05 | P2 | 按实例身份恢复遗留内核 | OPT-00、04 | 已实现待验证 | `3fe089c3` | [#回执-opt-05](#回执-opt-05) | 待真实壳崩溃重启、dev/release 并行与双平台回归（OPT-09）补证；隔离夹具已覆盖身份判定 |
| OPT-06 | P2 | 绑定预检与正式安装物料 | OPT-01、02 | 已实现待验证 | `ad3b4d44` | [#回执-opt-06](#回执-opt-06) | 待真实 npm/git 端到端应用与摘要稳定性补证；依赖闭包仍靠装后摘要核对+回滚，未缓存产物复用 |
| OPT-07 | P2 | 明确兼容性预检的信任边界 | OPT-01、06 | 已实现待验证 | `cc270d29` | [#回执-opt-07](#回执-opt-07) | 待真实 git prepare 执行/无取消入口的边界与双平台文案回归（OPT-09）补证 |
| OPT-08 | 验证前置 | 建立隔离且跨平台的 E2E 启动器 | OPT-00 | 已实现待验证 | `7528fdef` | [#回执-opt-08](#回执-opt-08) | E2E 已补独立端口隔离（`616820b5`，宿主 3090/3091 不串入）；待 Windows 实机补证 |
| OPT-09 | P2 | 建立双平台真实行为与发布回归 | OPT-03、04、05、06、08 | 已实现待验证 | `1cd9b348` | [#回执-opt-09](#回执-opt-09) | Darwin debug 集成层全绿（空 home 启动已修）、真实 install/start/stop 与 memory 数据已补证（§5）、unsigned DMG 打包/挂载启动已验；待 Windows / 安装后人工门禁、托盘退出与 OPT-03～06 跨层真实壳补证 |
| OPT-10 | P3 | 校准 CSS 静态检查与真实布局验证 | OPT-08；真实布局依赖 OPT-09 | 已实现待验证 | `683ea3a7` | [#回执-opt-10](#回执-opt-10) | Darwin 已补概览/记忆弹窗（含真实长文本）/主题/注入溢出；用量长列表、日志副窗、Windows/release 待补 |
| OPT-11 | P3 | 清理迁移向导与当前状态文档 | OPT-00；原生回归依赖 OPT-08 | 已实现待验证 | `52c43ac6` | [#回执-opt-11](#回执-opt-11) | 四块已删；Darwin 已验无入口且预置迁移标记/backup 重启后逐字节不变；待 Windows/release 升级态补证 |
| OPT-12 | P3 | 渐进建立 IPC 契约与职责边界 | OPT-00；真实验证复用 OPT-09 | 已实现待验证 | `069a1f9d` | [#回执-opt-12](#回执-opt-12) | 四组契约与双向校验已落地；debug E2E 22 例、release-mode 本地 19 例通过；真实成功启动的 StartReport / Channel 事件与真实 memory_list 已补证（§5）；待 Windows / 安装产物与更丰富真实数据 |
| OPT-13 | P3 | 完善系统代码签名与发布验证 | OPT-09 及外部证书资源 | 完成（系统签名豁免） | — | [#回执-opt-13](#回执-opt-13) | 2026-11-01 用户决定不采购正式证书；只沿用现有 GitHub workflow 的 updater 签名发布，不做 OS 代码签名/公证。README 与排障页保留未受信任提示 |

### 3.1 OPT-00 逐项核对记录（2026-11-01）

核对对象：HEAD `b9c46d3d`（代码与审查基线 `f448970f` 相同，`git diff f448970f..HEAD --stat` 只有 `docs/` 三处）；工作区其他会话的暗色主题 WIP 不参与，受影响文件用 `git show HEAD:<path>` 读。

判定口径：只确认「任务书描述的生产调用点与触发条件在当前代码仍成立」。这是静态基线核对，**不是行为验收**；不改变任何任务的「待实施」状态，也不能拿来当完成证据。核对后 `a9612f17`（UI 暗色主题）合入，涉及 UI 的行号开工时重新定位；`src-tauri/` 无变更。

| 编号 | 核对结论 | 关键证据（HEAD） | 对开工的影响 |
| --- | --- | --- | --- |
| OPT-01 | 成立 | `plugins/center.rs:2854-2898` 只用 repo 名生成 npm 候选（`repo_name` / `dsh-<repo>`，owner 不参与）；`:2912-2950` 在 GitHub 安装前逐个试候选，失败再回退 git | 只覆盖无 `#` 的 GitHub spec（`:2855` 见 `#` 直接返回 `None`）；测试要含 owner 同名 npm 包、Git 失败不回退 |
| OPT-02 | 成立 | `pkg/registry.rs:44-68` 的 `candidates()` 只补末尾斜杠、不校验 scheme（显式 `DSH_NPM_REGISTRY=http://…` 会被接受）；`pkg/fetch.rs:129` 直接用 packument 的 `dist.tarball` 下载；`plugins/center.rs:867` 的 git 白名单含 `"git"`；`pkg/releases.rs:77-85` 的 agent 没配重定向策略 | 显式覆盖与 tarball 边界是两处独立入口；禁 HTTP 降级、逐跳检查重定向都要补夹具 |
| OPT-03 | 成立（是确定顺序，不是竞态） | `bridge.rs:64-72` 的 `Drop` 按 `self.credentials` 逐个 `remove_file`；`:107-131` 先 `write_all` 写新凭据（`:116`），再 `drop(old)`（`:130`）；新旧代路径由 `bridge_credential.rs` 固定为同一批 `.xlink-bridge.json` | 连续两次 launch 就会删掉刚写的新凭据；先改换代顺序（旧代只清自己的），再处理 tmp `fs::write` → `chmod 0600` → `rename`（`:68-76`）的权限窗口 |
| OPT-04 | 成立 | `lifecycle.rs:1833-1856`：先向组发 SIGTERM（`:1839`），轮询主 Child（`:1848`）；主 Child 已退出时 `exited=true`，跳过组的 SIGKILL（`:1854-1856`） | 忽略 TERM 的后代正是残留点；验收要覆盖「组长立即退出 + 后代忽略 TERM」 |
| OPT-05 | 成立 | `lib.rs:233-237` 启动恢复仍按 cwd 回收；`lifecycle.rs:1762-1816` 用 `p == data_dir` 判身份（`:1795`、`:1812`）；`kernel_adapter.rs:471` 把内核 cwd 设到实例 workspace | 默认实例 workspace 不等于 data_dir，判据已失配；身份校验不能只看端口（任务书要求） |
| OPT-06 | 成立 | `precheck.rs:602-695` 的 `plugin_apply` 收 `spec_str`，在 `:652` 重新走 `install_for_instance`；`precheck_cmd.rs:84-105` 把预检时的 spec 原样回传；`verified_at_ms` 只用于提示「多久前预检」 | `latest` / 可变 tag 会再次解析；要做不可变计划与内容摘要绑定，不能用「提示版本可能变了」兜底 |
| OPT-07 | 成立 | 沙盒边界已写在源码注释（`plugins/sandbox.rs:13-29`：能看见什么、看不见什么、不是 OS 级隔离）；Git prepare 以当前用户权限跑（`center.rs:1501` `--config.enable-pre-post-scripts=true`）；用户文案仍是「预检」（`ui/src/plugins/PluginsPanel.vue:432`、`ui/src/diagnostics/PluginDiagnosis.vue:39-41`），「兼容性预检」目前只在任务书与本表 | 文案统一 + 来源/版本/构建阶段/报告分阶段；不要加容器或系统沙盒 |
| OPT-08 | 成立 | `e2e/wdio.conf.mjs:23` 写死 `src-tauri/target/debug/dsh-xlink`（无 Windows `.exe`）；`:66-85` 直接 `prep:builtin` + `tauri build --debug`，未设置或校验 `DSH_XLINK_HOME`、`appDirectoriesOverride`、`DSH_XLINK_CHANNEL_SUFFIX`；`package.json` 的 `e2e` 是裸 `wdio run` | 先做统一 launcher 与产物身份；本任务完成前不要跑新增真实壳测试 |
| OPT-09 | 成立 | `e2e/specs/memory.spec.mjs:129-132`、`:196-199`：Transition leave 不结束时 `this.skip()`；面板未挂载时只跳过、不做真实验证 | 先修动画帧/挂载通道，再补面板与 IPC 实际挂载断言；skip 不算通过 |
| OPT-10 | 成立（三类局限都在） | `ui/test/lib/css-cascade.mjs:83-116` 把 `@media` 条件整个展开、丢掉条件；`:121-126` 的 `specificity` 把逗号当元素选择器计数；`:190` 取规则体内第一处同名声明，后面的重复声明被忽略 | 静态检查继续管 token/模板/禁止模式；显示/裁切/滚动/主题必须走真实 WebView 的 computedStyle、边界与截图 |
| OPT-11 | 成立 | `git show HEAD:ui/src/App.vue` 的 `:24` 导入 `maybeOpenMigrationPrompt`、`:33-34` 导入面板与弹窗、`:53` 注册面板、`:230` 启动调用、`:355` 挂载弹窗；`SideBar.vue:92` 侧栏菜单；`commands.rs:2701-2713` 三条命令；`lib.rs:626-628` 注册；`ui/src/migration/` 1388 行 + `src-tauri/src/migration/` 2848 行（当前 0.6.5，超过 v0.6.0 移除期限） | capabilities 里没有搜到 migration 字样；删除时仍按 invoke 调用图核对 ACL。`legacy_migration_target` / `store_relocate` 保留 |
| OPT-12 | 成立 | `ui/src/shell/bridge.js:4/20/28` 只有裸 `invoke` 与 `makeChannel`；`store.js` 各调用点手写命令名与参数（如 `:195` `get_status`、`:400` `start_kernel`）；`ui/` 没有 `tsconfig.json` / `jsconfig.json`，`package.json` 无 typescript 依赖 | 先选四组接口建单一契约；不迁 TS/Pinia，保持组件 → store → bridge 的分工 |
| OPT-13 | 成立 | `tauri.conf.json` 只有 updater `pubkey` / `endpoints`；`desktop-release.yml:280-284,331-332` 只有 `TAURI_SIGNING_PRIVATE_KEY`（更新制品签名）；`README.md:25`、`:345` 明写未做操作系统代码签名 | 当时结论：证书资源未齐记 `blocked_resource`；2026-11-01 用户决定不采购证书，OPT-13 改为 updater-only 完成（见回执），不把更新签名写成系统签名完成 |

## 4 里程碑

| 里程碑 | 包含 | 状态 | 退出条件 |
| --- | --- | --- | --- |
| M0 验证准备 | OPT-00、08 | 进行中（OPT-00 完成，OPT-08 已实现待验证） | 基线（本文件）、隔离启动器与证据模板可用，启动真实测试壳前由代码核验隔离条件 |
| M1 优先修复 | OPT-01、02、03 | 进行中（01/02/03 已实现待验证） | 对应缺陷行为回归通过，无静默换源、无新凭据误删 |
| M2 生命周期与安装闭环 | OPT-04、05、06、07 | 进行中（04/05/06/07 已实现待验证） | 停止与恢复结果诚实，应用内容与预检绑定 |
| M3 原生验证与收口 | OPT-09、10、11 | 进行中（09、10、11 已实现待验证） | 双平台必测项通过，跳过与未执行明确列出 |
| M4 后续完善 | OPT-12、13 | 进行中（12 已实现待验证；13 完成，系统签名按用户决定豁免） | 分项交付；OPT-13 降级为 updater-only 发布 |

## 5 证据与回执

- 回执写在本文件末尾（`## 回执 OPT-XX`），表格「证据」列填对应锚点如 `#回执-opt-01`。
- 回执至少包含：实现提交、审查基线、平台与构建模式、触发条件、改到的生产入口、关键行为变化、兼容影响、实际命令与退出码、反向验证、未验证范围、剩余风险、回退方式、最终状态。
- 原始日志与截图不入库；需要留图时放 `docs/operations/evidence/2026-11/OPT-XX/`（只在确有二进制证据时创建）。测试 home、浏览器档案、单实例通道一律放在仓库外的本轮临时目录，且与证据目录分离。
- 账号、token、真实会话正文、私钥不出现在回执与证据里。

**当前累计门禁**（2026-11-01，HEAD `3510d830`，代码状态同 `069a1f9d`）：`npm run check:js` 通过（静态检查、protocol fixture、scripts 400、UI 652、smoke、plugin、probe、build、UI bundle 预算）；`npm run check:rust:ci` 通过（fmt、1351 tests / 13 ignored、clippy `-D warnings`）；`cargo check --release --all-targets --locked` 通过；隔离 E2E 6 个 spec / 22 条用例通过。这些只证明代码与 Darwin debug 集成层，**不是** Windows / release 安装产物的平台验收。

**Release-mode 本机冒烟**（2026-11-01，HEAD `bf8dec2e`）：`tauri build --target x86_64-apple-darwin --bundles app` 产出 24.34 MiB `.app`（x86_64，`codesign -dv` 显示 not signed at all）；因缺 updater 私钥，bundler 在更新签名步骤报错，但 `.app` 已产出。用隔离的 `HOME` / `CFFIXED_USER_HOME` / `DSH_XLINK_HOME` / `DSH_HOME` 直接启动该 `.app`：进程存活，`CGWindowList` 看到 1040×748 主窗口，日志里的 data_dir 指向隔离根且无 panic；启动后未写入真实 `~/.dsh-xlink`（其 mtime 早于本次运行）。**这不是 release 安装产物、无 WebDriver、未验证面板 IPC / 托盘退出 / 停止失败 / 签名 / Windows**，不计入完成。

**Release-mode + WebDriver 本地 E2E 安全记录**（2026-11-01）：用 release-mode 二进制 + 本地 WebDriver 在隔离 home 跑全套，第一个 `get_status` 失败：隔离 home 为空，但配置端口仍是默认 3090，壳探针看到宿主机正在运行的 release 内核（PID 13788；`active=null`、`installed=[]`，但 `running=true`）。测试在第一条断言即中止，**没有执行 `stop_kernel` 或任何停止命令**；临时运行根与测试进程已清理，宿主机 3090 内核 PID 13788 前后均在监听，真实 `~/.dsh-xlink` 的 mtime 未变。结论：release-mode 自动化必须先给隔离实例写入非 3090 端口并确认无监听，否则不得运行启停类用例；这次结果不算验证。

**Release-mode + WebDriver 隔离端口验证**（2026-11-01，HEAD `616820b5`）：release 二进制（本地 `--features wdio` 构建，非安装产物）在临时 home 预写 `shell/release/settings.json` 端口 35001 后，按 spec 单跑：ipc 5、console 3、layout 4、memory 6、panels 1，共 **19 passed、全部 exit 0**；`shell.spec.mjs` 因 `dev_build=false` 的 debug 断言未纳入。运行期间宿主 3090 内核 PID 13788 保持监听、35001 无残留监听；临时根已清理。**这是带 WebDriver 的本地 release-mode 构建，不是无 WebDriver 的 release 安装产物，不能替代 OPT-09 发布层验收。**

**Unsigned macOS DMG 打包检查**（2026-11-01，HEAD `16e73125`）：用当前 HEAD 构建 x86_64 DMG（`--config {"bundle":{"createUpdaterArtifacts":false}}`，9.70 MiB）；`hdiutil verify` 校验 VALID，只读挂载后 `.app` 为 x86_64、`codesign -dv` 显示 not signed at all；从挂载卷直接启动，隔离 `HOME` / `DSH_XLINK_HOME` / `DSH_HOME` 后进程存活，`CGWindowList` 看到 **1040×748 主窗口**，data_dir 指向隔离根；宿主 3090 内核 PID 13788 未动，挂载卷与临时根已清理。**这是 unsigned 打包产物检查，不是 Windows 或安装后人工门禁，也未验证面板 IPC、托盘退出、停止失败。**

**真实内核安装 / 启动 / 停止（Darwin debug 补充取证）**（2026-11-01，HEAD `0bf5bdcb`，被测产物为同 commit 的 `cargo build --debug --features wdio` 二进制，sha256 `e9a04809…`；runId `e2e-20261101154345-12007-6260e7`）：在隔离 `DSH_XLINK_HOME`、decoy 家目录、独立端口 35000 下，用临时 WebDriver 探针调用真实命令；`result.json` exitCode 0、postChecks 空、浏览器档案落在 decoy-home/Library/WebKit。
- `install_kernel("0.2.2-alpha.2")`：返回 ok，51 条进度，末两条「内核安装成功」「已切换到版本 0.2.2-alpha.2」；安装日志 `Done in 15.4s using pnpm v12.9.1`（复用预热 pnpm store，`downloaded 0`）、`PROBE-OK 11 targets, 931 dependency edges`；安装树 661 MiB，`active.txt` 为 0.2.2-alpha.2。前几轮冷 store 运行是真实网络下载（599 个包 / 595 个 tarball），安装耗时 41.3s、26.7s、2m49.1s；慢网络那轮触发的是探针自身的 120s 超时（安装随后仍完成），不是产品失败。
- `start_kernel`：`StartReport` = `running=true / port=35000 / incident=null`；timing 日志 `total_ms=8486 wiring_ms=5284 spawn_ms=276 ready_ms=2609`；内核日志给出 `dsh web: http://127.0.0.1:35000/?token=…`（token 不入库）；随后 `get_status` 为 `kernel.running=true`、端口 35000。
- 真实内核的 `memory_list`：契约校验通过，返回 schema_version 6、namespace `dsh/default-dev/web`、空记忆与真实落盘路径。
- `stop_kernel`：返回 null（成功）；随后 `get_status` 为 `kernel.running=false`，35000 无监听、无残留测试进程。
- 隔离：运行前后宿主 3090 内核 PID 13788 始终监听、未触碰；四轮运行根均按本轮标记清理，临时 pnpm store / corepack 缓存已删除。

**边界**：这是 Darwin debug + 本地 WebDriver 证据，不能替代 Windows、release 安装产物或无人值守发布门禁；预热 store 的 15.4s 不代表用户冷网络首次安装的耗时（冷网络实测 27s~2m49s）。

**v0.7.0 发布**（2026-11-01 UTC；发布 commit `eb38213b`，tag `desktop-v0.7.0`）：workflow run [41324018693](https://github.com/July-X/dsh-xlink/actions/runs/41324018693) 全绿（Preflight 21s、Windows x86_64 10m2s、Quality gates 8m59s、macOS Intel 17m6s、Publish 30s）；Release 为正式版（draft=false、prerelease=false），6 个资产齐全且与计划逐名吻合（Windows `setup.exe` + `.sig`、macOS `.app.tar.gz` + `.sig`、`.dmg`、`latest.json`）；`releases/latest/download/latest.json` 报告 `0.7.0` 且四个平台键齐全；tag ref 指向 `eb38213b`。**边界**：CI 只证明双平台 release 产物可生成、可下载，未在 Windows / Intel macOS 真机上安装运行（首次启动、面板 IPC、托盘退出、停止失败、升级态仍是人工门禁）；系统签名 / 公证仍按 2026-11-01 用户决定不启用。

**v0.7.1-rc.1 发布**（2026-11-02 UTC；发布 commit `d0d6a650`，tag `desktop-v0.7.1-rc.1`）：workflow run [41385845905](https://github.com/July-X/dsh-xlink/actions/runs/41385845905) 全绿（Preflight 20s、Quality gates 19m3s、macOS Intel 16m4s、Windows x86_64 9m37s、Publish 31s）；Release 为正式版（draft=false、prerelease=false），6 个资产齐全且与计划逐名吻合（Windows `setup.exe` + `.sig`、macOS `.app.tar.gz` + `.sig`、`.dmg`、`latest.json`）；`releases/latest/download/latest.json` 报告 `0.7.1-rc.1`、四个平台键齐全且正文含「从旧版本升级」须知；tag ref 指向 `d0d6a650`。同一 commit 的 Desktop CI 双 job 全绿（Windows `cargo test` 已通过；发布前把 `av_trust` 的本机环境断言改成环境无关判据，`1d96d34a`）。**边界**：CI 只证明双平台 release 产物可生成、可下载，未在 Windows / Intel macOS 真机上安装运行（首次启动、面板 IPC、托盘退出、停止失败、升级态仍是人工门禁）；Windows 实机 UAC 改 Defender 排除项与冷启动收益也未实测；系统签名 / 公证仍不启用。

**v0.7.1-rc.2 发布**（2026-11-02 UTC；发布 commit `10dfb2d8` + 两次门禁修复 `c5496e2d`、`a3b11ab8`，tag `desktop-v0.7.1-rc.2` → `a3b11ab8`）：workflow run [41398284622](https://github.com/July-X/dsh-xlink/actions/runs/41398284622) 全绿（Preflight 27s、Quality gates 9m0s、macOS Intel 15m36s、Windows x86_64 9m32s、Publish 31s）；Release 为正式版（draft=false、prerelease=false），6 个资产齐全且与计划逐名吻合（Windows `setup.exe` + `.sig`、macOS `.app.tar.gz` + `.sig`、`.dmg`、`latest.json`）；`releases/latest/download/latest.json` 报告 `0.7.1-rc.2`、四个平台键齐全且正文含「从旧版本升级」须知；tag ref 指向 `a3b11ab8`。同一 commit 的 Desktop CI 双 job 全绿。**过程如实记**：前两次 run 分别失败于 `cfg(unix)` 分支的 E0004（`Ok((true,_))` 未穷尽）与 `clippy::redundant_pattern_matching`——Windows 本机不编译 Unix 分支，本地门禁盲区；两次都未产生正式 Release，确认 `releases/tags/desktop-v0.7.1-rc.2` 为 404 后才删旧 tag 重建。**边界**：CI 只证明双平台 release 产物可生成、可下载，未在 Windows / Intel macOS 真机上安装运行；真实网络安装端到端、`npm run e2e` 全链路、Unix 停止/孤儿回收实跑仍未验证；系统签名 / 公证仍不启用。

**剩余验收入口**：Windows x64 与 Intel macOS 同一候选 commit 的 release 安装产物（首次启动、面板/IPC、关闭/托盘/退出、停止失败、内置资源摘要）；重定向服务器、私有 registry 与真实账号场景。真实网络安装与真实内核启动/停止已补 Darwin debug 证据（见上），仍需双平台产物层复核。OPT-13 系统签名按 2026-11-01 用户决定豁免（不采购证书，发布沿用 updater-only workflow），不再列为阻塞项。

### 回执模板

```text
任务编号：
实现提交：
审查基线：
执行平台及构建模式：

问题与触发条件：
实际修改的生产入口：
关键行为变化：
兼容影响及保留路径：

验证结果：
- 用例或命令：
- 结果：pass / fail / skip / not_run
- 实际退出码或观察：
- 证据位置：

反向验证：
未验证范围：
剩余风险：
回退方式：
任务状态：
```

## 6 开工检查清单（OPT-00 验收）

- [x] 执行 commit 与审查 commit 已记录，差异影响已说明（本文件 §1）。
- [x] 工作区其他会话改动已识别并声明不参与（§1）。
- [x] 证据目录与测试 home 分离的约定已写明（§5）。
- [x] 未执行的验证没有出现在任何通过清单里（§3.1 是静态核对，不是行为验收）。
- [x] 13 个任务的生产调用点与触发条件逐项确认（§3.1，2026-11-01，HEAD `b9c46d3d`）。
- [x] 各任务回执已按 §5 模板补齐（OPT-01～13 见下文；验收未执行项仍保留）。

## 7 范围锁

本轮不做：整体 UI 重设计、全仓迁移 TypeScript/Pinia、引入新数据库、接入第二种真实内核、重写记忆系统、扩大第三方官网支持、新增 Linux 发布平台。除 OPT-13 外，外部证书与账号资源不计入代码修复完成条件。

## 回执 OPT-01

任务编号：OPT-01
实现提交：`6c11de2e`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）

问题与触发条件：未带 revision 的 GitHub spec 会按仓库末段名先试 `<repo>` / `dsh-<repo>` 的同名 npm 包，owner 不参与归属校验；npm 包通过插件校验并安装成功时，实际来源与用户选择的 GitHub 仓库不同。
实际修改的生产入口：`src-tauri/src/plugins/center.rs::install_unlocked` 删除 npm 候选优先分支与 `npm_candidates_from_github_spec`；来源解析统一走 `parse_spec`。
关键行为变化：GitHub URL / SSH URL / `owner/repo` 简写固定按 Git 安装（GitHub Release tarball 优先、clone 回退）；npm 包名仍走 npm；两种来源不再互相隐式替代。
兼容影响及保留路径：此前经隐式替换装成 npm 的条目 `source` 仍是 npm 包名，更新按记录来源继续走 npm，不自动改源/重装/删除；需要 npm 包仍直接填包名。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugins::center::tests`
- 结果：pass（63 passed）
- 实际退出码或观察：0；`test result: ok`
- 证据位置：本地执行输出（未入库）
- 另：`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 均通过。

反向验证：未做源码变异反向验证（本任务删分支 + 纯解析/更新来源判据）；判据覆盖不同 owner 同名仓库、四类 Git spec、npm spec 与历史来源更新。
未验证范围：真实网络安装（GitHub Release / clone，且 npm 存在同名包的实网场景）未执行；双平台真实壳回归依赖 OPT-09。
剩余风险：解析/更新代入测试不能证明 fetch_git 在所有网络失败情形下都不回退 npm——生产代码已无该分支，但真实安装行为需 OPT-09 证据；如有外部脚本依赖旧 npm 加速，需要显式改为 npm spec。
回退方式：revert `6c11de2e`，恢复 npm 候选优先分支。
任务状态：已实现待验证

## 回执 OPT-02

任务编号：OPT-02
实现提交：`bbd410b0`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）

问题与触发条件：显式 `DSH_NPM_REGISTRY` 只做 trim + 补斜杠，接受 HTTP 与任意 URL；packument 的 `dist.tarball` 不经校验直接下载；Git 白名单含 `git://`；取源层没有重定向策略（ureq 默认跟随，最多 10 跳）。
实际修改的生产入口：`pkg/registry.rs`（registry / 下载 URL / tarball 主机 / 重定向链策略）、`pkg/releases.rs`（`http_get_string` / `http_get_file` 统一校验，redirect history 逐跳检查，`ensure_registry_base` 返回 Result）、`pkg/fetch.rs`（tarball 下载绑定允许域）、`plugins/center.rs`（`parse_spec` 移除 `git://`、配置错误上抛）、`kernel/lifecycle.rs`、`shell/process.rs`、`node/detect.rs`（调用点传播配置错误）。
关键行为变化：显式 registry 只接受 HTTPS 且无 userinfo/query/fragment；tarball 与每跳重定向必须 HTTPS、无 userinfo，主机属于服务 packument 的 registry 或 `DSH_NPM_TARBALL_HOSTS`；重定向最多 5 跳；`git://` 来源被拒绝。配置错误在网络请求前返回可操作中文原因，不再静默回落官方源。
兼容影响及保留路径：已有 `DSH_NPM_REGISTRY=http://…` 部署会直接报错，需改 https；历史 `git://` 条目不再能被解析更新，提示迁移 https/ssh，不自动改源或重装；自定义私服的 tarball 域名需在 `DSH_NPM_TARBALL_HOSTS` 显式声明。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`
- 结果：pass（1359 passed, 13 ignored）
- 实际退出码或观察：0；`test result: ok`
- 证据位置：本地执行输出（未入库）
- 另：`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`node scripts/check-invariants.mjs` 通过。

反向验证：未做网络层反向验证；策略拒绝由 `pkg::registry` 的纯函数判据覆盖（http / userinfo / 越界主机 / 链中任一跳越界 / 非法 env 主机），不依赖真实网络。
未验证范围：真实重定向服务器（HTTPS→HTTP、循环、超 5 跳）、真实私服 + 自定义下载域、双平台壳回归均未执行；网络夹具留待 OPT-08/09。
剩余风险：ureq redirect history 只在 `http_get_*` 路径校验，其他自建 agent（如 registry probe）仍跟随默认重定向但只发无 body 的探测；`DSH_NPM_TARBALL_HOSTS` 为主机级粒度（不含端口/路径），私服若在非标准端口需同主机即可；真实网络行为需 OPT-09 证据。
回退方式：revert `bbd410b0`。
任务状态：已实现待验证

## 回执 OPT-03

任务编号：OPT-03
实现提交：`98e3df53`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）

问题与触发条件：`launch` 先向每个物化插件的固定路径写新凭据，再释放旧 Bridge；旧代 `Drop` 按路径无条件 `remove_file`，连续两次启动就会删掉刚发布的新凭据。另一处是凭据临时文件先按 umask 创建（0644）再 `chmod 0600`，进程中途被杀会留下权限过宽的凭据。
实际修改的生产入口：`plugins/builtin/bridge.rs`（Bridge 记录本代令牌、`credential_belongs_to`、`scope_key`、`shutdown`）、`plugins/builtin/bridge_credential.rs`（`atomic_write_private`）、`kernel/kernel_adapter.rs`（统一 scope key）、`kernel/lifecycle.rs`（start 失败回收、stop_instance 确认后撤销）、`commands.rs`（start_kernel 失败/取消回收、stop_kernel 停止成功才撤销）。
关键行为变化：旧代只删除令牌仍属于自己的凭据；新代凭据不会再被旧代误删。凭据临时文件从创建起就是 0600，没有先宽后窄窗口。内核启动失败 / 被取消时回收本轮起的桥；停止成功后才撤销桥代次，停止失败保留仍在运行内核的通信依据。注册表锁只覆盖替换，Drop（端口自连 + 文件 IO）在锁外执行。
兼容影响及保留路径：凭据文件路径与 JSON 字段不变，插件侧无需改；env 通道照旧；未加代次编号到文件格式，改用令牌内容归属判断；历史凭据由新代覆盖，不自动删除用户插件数据。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`
- 结果：pass（1361 passed, 13 ignored）
- 实际退出码或观察：0；`test result: ok`
- 新增判据：生产 `launch` 连续两代后凭据仍在且令牌是第二代（旧实现会失败）；旧代 Drop 不删属于下一代的凭据；`shutdown` 删除凭据且重复调用幂等。
- 另：`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 通过。

反向验证：未做故障注入；`a_second_generation_kills_the_first_one` 是在生产 `launch` 路径上的定向回归，旧实现（按路径无条件删除）会让「凭据仍在且令牌为第二代」失败。
未验证范围：真实内核 spawn / 就绪失败注入、真实壳停止与重启路径、Windows 行为回归未执行（依赖 OPT-09）；启动/停止接线目前靠编译与代码审查，没有端到端夹具。
剩余风险：停止成功后才撤销桥的判断仍沿用现有停止语义（`Unknown` 按乐观处理）；极端情况下桥已撤销而内核仍短暂存活，需要 OPT-09 真实壳证据覆盖。
回退方式：revert `98e3df53`。
任务状态：已实现待验证

## 回执 OPT-04

任务编号：OPT-04
实现提交：`533f01f3`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）

问题与触发条件：Unix 停止逻辑以主 Child 的 `try_wait` 是否成功作为收尾判据；组长先退出时直接跳过组级 SIGKILL，忽略 TERM 的后代继续运行。旧实现随后只按组长 pid 复查，无法发现「组长没了但组里还有后代」，UI 可能显示已停止。
实际修改的生产入口：`shell/process.rs`（新增 `terminate_process_group` / `GroupStopOutcome`，复用于 `terminate_process_tree`）、`kernel/lifecycle.rs`（Unix `stop` 改用该原语并按结果如实报错）。
关键行为变化：停止按进程组收尾——TERM → 最多 1s 等待组消失（期间回收组长僵尸，避免僵尸让组永远「存在」）→ 对仍存在的组 SIGKILL → 最多再等 1s；组长先退出不再等于完成。非进程组长回退单进程 kill。`StillAlive` / `Unknown` 返回可操作错误，不再乐观报成功；Windows Job Object 路径保持原实现。
兼容影响及保留路径：正常内核收到 TERM 即退出，耗时基本不变；整组忽略 TERM 的最坏情形最多多约 1s 才升级 SIGKILL。停止失败现在会向 UI 报错（旧实现可能报成功），这是本任务要修的诚实性问题，不做静默兜底。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`
- 结果：pass（1365 passed, 13 ignored）
- 实际退出码或观察：0；`test result: ok`
- 新增隔离夹具：组长先退出 + 后代忽略 TERM（旧实现会残留）、整组忽略 TERM 后 SIGKILL 收尾、普通 TERM、生产 `lifecycle::stop` 收尾组长退出后的后代；运行后无残留进程。
- 另：`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 通过。

反向验证：未单独做变异；`kills_descendant_after_leader_already_exited` 与 `lifecycle_stop_reaps_descendants_of_a_finished_leader` 在旧实现（try_wait 成功即跳过 SIGKILL）上会留下后代并使断言失败。
未验证范围：真实内核进程的停止、权限拒绝 / 不可中断进程、Windows Job Object 行为、双平台壳回归均未执行（依赖 OPT-09）。
剩余风险：PGID 复用依赖「组仍存在时内核保留该 pgid」的系统语义；若进程主动脱离进程组则不在收尾承诺内（任务书已注明可观测边界）；权限不足路径只返回 Unknown，没有专门夹具。
回退方式：revert `533f01f3`。
任务状态：已实现待验证

## 回执 OPT-05

任务编号：OPT-05
实现提交：`3fe089c3`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）

问题与触发条件：启动恢复用「进程 cwd == 壳 data_dir」识别遗留内核，而现役 DshAdapter 把内核 cwd 设为实例 workspace（用户还可以配置外部工作目录），旧判据在当前布局下恒不命中，崩溃后的孤儿内核不会被回收。dev / release 共用实例目录，若按目录扫描可能误杀另一个壳正在服务的实例。
实际修改的生产入口：`kernel/lifecycle.rs::reap_orphans`（改为逐实例按 pid 记录确认身份，新增 OrphanRecovery / process_cwd / workspace 匹配 / 僵尸判定，逐条写 shell_events）、`lib.rs`（启动恢复调用与摘要输出）、`lifecycle::start_instance`（发现本壳仍有该实例遗留内核身份时拒绝继续启动）。
关键行为变化：身份四层确认——pid 文件属于本壳模式、命令行仍是 dsh 内核且记录里的启动端口一致、进程 cwd 等于该实例 workspace、进程仍存活；四层都满足才发信号。其它壳的 pid 记录跳过；旧格式 pid（无壳归属）、只有 runtime 记录、读不到 cwd 的一律保留进程并记为「来源待确认」，不猜。成功回收后删除该实例 pid 文件；失败/待确认不动 pid、runtime 或运行状态记录，也不宣称已恢复。
兼容影响及保留路径：Windows 维持原来的 Job Object / 端口回退路径（reap_orphans 仍为空实现）；外部手工启动的 dsh（cwd 不是实例 workspace）不会被回收；旧格式 pid 文件不再被当成可证明归属的候选，需要用户手动停止或下次启动由 start 守卫提示。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`
- 结果：pass（1368 passed, 13 ignored）
- 实际退出码或观察：0；`test result: ok`
- 新增隔离夹具：本壳实例按 workspace 身份回收（旧实现恒不命中）、其它壳 pid 记录跳过且进程存活、cwd 不属于该实例时不杀。
- 另：`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 通过。

反向验证：未单独做变异；`recovers_orphan_of_current_shell_by_instance_identity` 在旧的 cwd == data_dir 扫描下找不到候选，断言会失败。
未验证范围：真实内核孤儿（kill -9 壳后重启）、dev/release 并行原生夹具、损坏或缺失 pid/runtime 记录、查询工具（lsof/ps）不可用、外部 workspace 的真实进程均未执行（依赖 OPT-09）。
剩余风险：身份依赖 pid 文件里的壳归属与 cwd 可读；旧格式记录只能保守跳过，可能留下需要手动处理的遗留内核；runtime-only 记录不会自动回收；start 守卫检查的是 pid 身份而非 workspace，极端 PID 复用下可能拒绝启动（文案给出手动处置路径）。
回退方式：revert `3fe089c3`。
任务状态：已实现待验证

## 回执 OPT-06

任务编号：OPT-06
实现提交：`ad3b4d44`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）+ Vite 构建

问题与触发条件：预检在沙盒实例里真实安装并验证一次，随后中央库按字节回滚；应用阶段只拿到原始 apply_spec 与 verified_at_ms，又走一次 `install_for_instance` 重新解析。npm 不钉版本、git 不钉 commit 时，两次解析可以去到不同产物（浮动的 latest / 分支 / 依赖解析），而旧实现只在安装后给一句「未重新验证」提示，不构成绑定。
实际修改的生产入口：新增 `plugins/install_plan.rs`（计划结构、plan_id 摘要、校验、内存缓存、树摘要、固定 spec 构造）；`plugins/precheck.rs`（预检通过后生成计划并下发、应用只消费计划并按固定 spec 安装、装后摘要核对与回滚）；`plugins/precheck_cmd.rs`（命令增加 planId / Node 版本）；`plugins/sandbox.rs`（报告带 plan）；`pkg/fetch.rs`（按完整 commit 取源）；UI `plugins.js` / `PrecheckDialog.vue`（回传 planId，无计划不可应用）。
关键行为变化：计划记录 npm 精确版本 / git 完整 commit、插件树 SHA-256（含依赖、排除 .dsh-id 与 .git）、内核版本、实例、profile、端口、物化方式、平台/架构、Node 版本与 30 分钟有效期；应用前逐项校验并通过 plan_id 检测篡改，不匹配直接拒绝并要求重新预检。应用按固定 spec 安装，装完重算摘要；不一致时回滚本次安装并返回错误。缓存为进程内、按 TTL 与 16 条上限清理；没有计划（旧报告 / 生成失败 / 壳重启 / 过期）时 UI 与后端都拒绝应用。
兼容影响及保留路径：升级后第一次应用需要重新预检（旧报告没有计划，按钮禁用并说明原因）；关闭预检开关的直接安装路径不变；计划不落盘，壳重启后失效是刻意行为；git 仍优先走 GitHub Release，钉 commit 时回退 init/fetch/checkout。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`
- 结果：pass（1375 passed, 13 ignored）
- 实际退出码或观察：0；`test result: ok`
- 新增判据：plan_id 覆盖字段且能检出篡改；过期拒绝；实例/profile/端口/物化/Node 漂移拒绝；固定 spec 用精确版本与完整 commit；树摘要忽略 .dsh-id/.git 并随内容变化；缓存可清理；无 planId 时生产 `plugin_apply` 拒绝。
- 另：`cargo check --all-targets`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`npm run build:ui` 通过。

反向验证：未做故障注入；plan_id 篡改与无计划两条判据在旧实现（只回传 verified_at_ms、按 apply_spec 重装）上没有对应拒绝路径。
未验证范围：真实 npm / git 插件从预检到应用的端到端摘要一致性未跑（无法在本机离线夹具覆盖）；依赖闭包在 lock 缺失、依赖浮动时的实际行为未测；Windows 与私有 git 源未测；依赖 OPT-09。
剩余差距（如实记）：应用仍会重新下载产物（未缓存预检验过的物料），依赖闭包没有在预检时固化锁文件/清单，而是靠装后整树摘要核对与回滚来兜底；计划缓存只存在内存，未实现磁盘缓存与清理策略。这些需要后续在 OPT-06 上继续，或并入 OPT-08/09 的隔离夹具验证。
回退方式：revert `ad3b4d44`。
任务状态：已实现待验证

## 回执 OPT-07

任务编号：OPT-07
实现提交：`cc270d29`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（cargo test / check / clippy / fmt）+ Vite 构建 + Node UI 测试

问题与触发条件：用户界面只叫「预检」，未说明它验证的是兼容性（安装、接线、启动）而不是安全性；报告只有 verdict / summary / warnings，用户看不出取源、构建、规范、接线、启动探测分别验没验；沙盒边界与 git prepare 的当前用户权限只写在源码注释里，操作前看不到；OPT-06 的安装计划也没有在界面文案里说明。
实际修改的生产入口：`plugins/sandbox.rs`（新增 PrecheckStage / stages 与状态常量）、`plugins/precheck.rs`（按沙盒失败、基线失败、候选失败/通过四条路径填充分阶段结果，构建阶段按 npm/git 区分，计划生成后补 plan 阶段）、`ui/src/plugins/PrecheckDialog.vue`（逐条展示阶段结果）、`PluginsPanel.vue` / `plugins.js` / `PluginDiagnosis.vue`（文案统一兼容性预检）、`docs/features/extensions/plugin-management.md`（信任边界）。
关键行为变化：报告新增 stages（sandbox / source / build / spec / wiring / boot / plan），各标 passed / failed / not_run / skipped 并带实际细节；npm 的构建阶段如实标 skipped，git 标 passed 并说明以当前用户权限执行依赖安装 / prepare；未执行阶段明确 not_run，不画成通过。界面、进度、toast、诊断页与后端摘要/提示统一称「兼容性预检」，报告不提供「插件安全」的整体结论。安装面板 tooltip 在操作前说明：沙盒只是一次性实例/目录/端口隔离，不是 OS 级沙盒；git prepare 以当前用户权限执行、可能访问用户可读文件；不复制真实实例凭据不等于权限隔离。
兼容影响及保留路径：verdict 取值与前后端契约不变；旧报告没有 stages 时界面不渲染分阶段区块；诊断页三态与运行记录标签照旧。预检开关保留短标签「预检」以适配 Element Plus inline prompt 的固定宽度，紧邻 tooltip 与按钮文案给出全称。

验证结果：
- 用例或命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`
- 结果：pass（1376 passed, 13 ignored）
- 实际退出码或观察：0；`test result: ok`
- 新增判据：分阶段结果覆盖七个阶段；git 构建阶段为 passed、npm 为 skipped；生成计划前 plan 必须是 not_run，填充后才 passed；未执行安装时 source/build/boot 均为 not_run。
- 另：`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`npm run test:ui`（646 passed）、`npm run build:ui`、`npm run check:invariants` 通过；同步修正了 OPT-06 后过期的两条 UI 判据（应用必须回传 planId）。

反向验证：分阶段判据在「把未执行画成通过」的旧实现上没有对应字段，无法假绿；UI 判据现在同时要求 verdict / installed / planId 三项，只保留前两项会红。
未验证范围：真实 git prepare 的构建输出与耗时、恶意插件对外部目录的实际影响、真实取消路径均未执行；本任务按约定不建设容器或系统级沙盒。现有预检没有「取消」控件（进度面板关闭不发送取消指令），因此不存在「取消后仍继续构建 / 接线」的可观测路径，未来若加取消需要重新验证。
剩余风险：沙盒隔离仍是临时实例 + 目录 + 端口，不能阻止插件读取当前用户可读文件或访问网络；文档与界面已如实说明，但用户仍可能把「兼容性通过」误读为安全背书；安装阶段整体失败（install_for_instance 返回 Err）时没有报告对象，只能在进度面板与日志里定位具体阶段。
回退方式：revert `cc270d29`。
任务状态：已实现待验证

## 回执 OPT-08

任务编号：OPT-08
实现提交：`7528fdef`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug profile（tauri build --debug --features wdio）；Node 25.9.0 / WDIO 9

问题与触发条件：`e2e/wdio.conf.mjs` 自己 `prep:builtin` + `tauri build --debug`，写死 `src-tauri/target/debug/dsh-xlink`（Windows 没有 `.exe`），没有设置或校验 `DSH_XLINK_HOME`、编译期 `appDirectoriesOverride`、`DSH_XLINK_CHANNEL_SUFFIX`；`npm run e2e` 裸调 wdio，连续两轮会复用数据目录、浏览器档案、单实例通道和可能未注入配置的旧 debug 产物。
实际修改的生产入口（测试基础设施）：新增 `e2e/launcher.mjs`；改写 `e2e/wdio.conf.mjs`（只消费 launcher 注入的变量，缺变量直接拒绝）；`package.json` 的 `e2e` 改为 `node e2e/launcher.mjs`；新增 `scripts/e2e-launcher.test.mjs`；`docs/operations/development.md` 补 E2E 启动器说明。
关键行为变化：每轮创建独占运行根，包含 home（DSH_XLINK_HOME）、appdata（编译期 appDirectoriesOverride）、evidence、decoy-home（HOME / USERPROFILE / DSH_HOME，macOS 另设 CFFIXED_USER_HOME）与本轮 target。构建用 CARGO_TARGET_DIR=<run>/target 与注入 TAURI_CONFIG，不复用旧 debug 二进制；Windows 解析 .exe，macOS 解析无后缀产物，构建存在性与 wdio 启动共用同一路径。启动前校验归属标记、三项隔离与路径都在本轮根内；启动后核对二进制 SHA-256、测试 home、浏览器档案（appdata 或 macOS decoy Library/WebKit 至少一个有内容）与 decoy 哨兵未被改动。失败保留整个运行根与日志（证据在 home 之外）；成功只删带本轮标记的目录；`--keep` 可保留成功轮次；信号 / 异常时按本轮子进程组停止。
兼容影响及保留路径：直接 `wdio run e2e/wdio.conf.mjs` 现在会明确报错，必须走 `npm run e2e`；WebDriver 仍只由显式 debug feature 启用，release 门禁未改。每轮 target 独占会让首次构建变慢（本机实跑约 4 分钟），这是隔离代价。

验证结果：
- 用例或命令：`node --test scripts/e2e-launcher.test.mjs`
- 结果：pass（6 passed）；另 `npm run test:scripts` 401 passed / 1 skipped，`npm run check:webdriver-gating`、`npm run check:invariants` 通过。
- 实机：Darwin 上连续两次 `npm run e2e -- --keep`。两次运行根不同；第二次浏览器档案落在 `<run>/decoy-home/Library/WebKit/<bundle>/WebsiteData`；DSH_HOME / HOME decoy 哨兵未改；`result.json` 的 postChecks 为空；退出后 `pgrep dsh-xlink-e2e` 无结果。console / shell 两个 spec 文件通过；memory spec 仍因面板挂载与 Tab 计数失败（OPT-09 已知范围）。
- 反向验证：`isolationProblems` 判据拒绝测试根外路径与 runId 不匹配；`cleanup` 判据拒绝归属不符目录；无隔离变量时直接 import wdio 配置得到「E2E 必须通过 npm run e2e 启动」而不是继续跑（实跑验证）。
- 后续补强（2026-11-01，实现提交 `616820b5`）：launcher 在隔离 home 的 `home/shell/dev/settings.json` 预写本轮空闲端口（35000 起做 bind 检查），并经 `DSH_E2E_EXPECTED_PORT` 传给 wdio；`shell.spec.mjs` 断言 `status.kernel.port` 等于该端口且 `kernel.running=false`。本机重跑 `npm run e2e -- --keep`：6 spec / 22 例通过，exitCode 0，`postChecks` 为空，settings 为 `{"port":35000}`；运行期间宿主 3090 内核 PID 13788 保持监听、未被触碰。此前 release-mode 自动化因隔离 home 仍用 3090 看到宿主内核 `running=true`，本补强消除该串扰。

未验证范围：Windows 实机（只覆盖 .exe 名称解析与路径逻辑）；成功路径的自动清理没有在实跑中走到（两次都因 memory spec 失败而保留目录，清理仅单元覆盖）；release 产物的 WebDriver 产物层判据未在本轮重跑（沿用现有门禁）。
剩余风险：每轮独占 target 的首轮构建成本高；macOS 浏览器档案靠 CFFIXED_USER_HOME 隔离，属已知系统行为而非产品保证；E2E 当前仍有一个 OPT-09 范围的失败，`npm run e2e` 整体退出码为 1，不能称为全绿。
回退方式：revert `7528fdef`。
任务状态：已实现待验证

## 回执 OPT-09

任务编号：OPT-09
实现提交：`1cd9b348`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；隔离 E2E（debug + wdio feature）；Node 25.9.0 / WDIO 9

问题与触发条件：`e2e/specs/memory.spec.mjs` 在 WebDriver 通道里因 Transition leave 不结束而 `this.skip()`；效果页只等数据源挂出就数 KPI，首扫期间读到 0 导致假失败；E2E 只有 shell / console / memory 三个 spec，没有面板切换 + 真实 IPC 的稳定集成路径。
实际修改的入口：`e2e/specs/memory.spec.mjs`（仅 E2E webview 注入 rAF→setTimeout，删除两处 skip，等待面板标题与 5 块 KPI 真实渲染）、新增 `e2e/specs/panels.spec.mjs`（切到用量分析，等标题/内容区/概览数据区挂载，真实 invoke `get_model_usage`，核对 DTO 与 DOM）、`docs/operations/development.md`（动画控制覆盖范围与 debug/release 发布矩阵）。
关键行为变化：关键面板未挂载时用例直接失败，不再 skip；效果页断言在 report 渲染完成后再取 DOM；新增「面板切换 + 真实 IPC + 内容区」路径。测试通道的 rAF 替换只作用于 E2E webview，产品代码与正式构建动画路径未改；普通动画仍由 UI 判据与真前台窗口覆盖，不能把 E2E 挂载结果当作动画验证。
发布矩阵：debug 集成层可用 `npm run e2e` 自动执行；release 安装产物层不启用 WebDriver，需要 Windows / Intel macOS 同一候选 commit 的人工门禁；CI 无原生桌面会话时人工门禁记 `not_run`，fail / skip / not_run 均不计完成。

验证结果：
- 用例或命令：Darwin 上 `npm run e2e -- --keep`（隔离 launcher）
- 结果：pass（4 个 spec / 13 条用例全绿：console 3、memory 6、panels 1、shell 3）
- 隔离证据：launcher `result.json` 的 postChecks 为空；浏览器档案落在 decoy-home/Library/WebKit；二进制 SHA-256 记录；运行后无遗留进程；本轮目录在检查后已清理。
- 前次失败对照：同一 spec 在修复前两次运行都在「效果页 5 块 KPI」处失败（kpis=0），修复后通过；这不是人工变异，但定位到了目标条件。

反向验证：本轮没有对产品代码做故意破坏的后向验证；「面板未挂载」现在由超时失败承载，但没有用真实损坏的面板/断开的 IPC 演练过。留作后续与 release 人工门禁一起补。
未验证范围：Windows 实机（隔离首次启动、面板切换、真实 IPC）；Intel macOS release 安装产物层；托盘恢复、关闭、退出、停止失败的用户路径；OPT-03～06 在真实壳中的跨层行为；release 产物不启用 WebDriver 的运行时确认。
- 2026-11-01 补充尝试（未计入验证）：点击标题栏关闭后 `document.visibilityState` 变为 `hidden`，说明 close→hide 生效；但 WebDriver 在隐藏窗口上的 `executeAsync` 返回 `null`，无法自动点托盘/菜单栏恢复，故关闭→恢复→退出链路仍留给 release 人工门禁。临时 spec 与运行根已删除。
- 2026-11-01 补充（macOS 打包层，未计入完成）：当前 HEAD 构建的 unsigned DMG 经 `hdiutil verify` 通过、只读挂载后从卷内 `.app` 启动，主窗口 1040×748，隔离 data_dir 正确，宿主 3090 内核未动；仍缺 Windows、安装后人工门禁、面板 IPC/托盘退出/停止失败。
- 2026-11-01 修复并验证（实现提交 `3ae0ef60`）：ipc 的 `start_kernel` 空 home 用例曾因 `guarded_start` 无条件先跑 `ensure_wiring`，在冷 Corepack/pnpm 与慢 registry 下等到 225s，被 120s 超时判失败；现在无活动内核时跳过首次 wiring。新增单测钉住 pnpm 不被调用；全量 Rust **1352 passed / 13 ignored**、clippy/fmt 通过；隔离 E2E 恢复 **6 spec / 22 例全绿**（runId `e2e-20261101142521-11134-bb8c40`，exitCode 0、postChecks 空）。
剩余风险：rAF 替换是测试通道专用但仍是「让面板挂得上」的旁路，产品动画在真前台窗口没有专门断言；E2E 全绿只证明 Darwin debug 集成层，不构成双平台或 release 完成。
回退方式：revert `1cd9b348`。
任务状态：已实现待验证

## 回执 OPT-10

任务编号：OPT-10
实现提交：`683ea3a7`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；Node UI 测试 + 隔离 E2E（debug + wdio）

问题与触发条件：CSS 层叠检查器把 `@media` 条件整个展开、丢掉条件，选择器列表的特异度按整串（含逗号）算，规则体内重复声明只取第一处；判据因此可能在错误的前提下变绿。真实显示/裁切/滚动/主题没有真实 WebView 检查，静态通过被当成渲染通过。
实际修改的入口：`ui/test/lib/css-cascade.mjs`（顶层逗号拆选择器并逐分支匹配/算特异度、重复声明取最后一条、条件 at-rule 默认不参与、`html.dark` 默认浅色且 tokens 带 `dark` 或 `opts.theme` 才计入、文件头写清语法边界）、新增 `ui/test/cssCascadeContract.test.js`、修正 `ui/test/designAlignment.test.js` 中一直读到深色覆盖的副标题断言、新增 `e2e/specs/layout.spec.mjs`、`docs/operations/development.md`（静态边界与真实布局说明）。
关键行为变化：条件规则不再被静默当成无条件规则；选择器列表逐分支判断，命中分支自己算特异度；重复声明按浏览器语义取最后一条；主题祖先默认浅色，深色必须显式带 `dark`。真实布局检查在固定 1040×748 窗口覆盖概览横向溢出/卡片边界/动作按钮可达、浅深主题 token 与宽度、记忆编辑弹窗页脚可达，并用注入 3000px 元素反向验证检测器会失败；失败信息带元素、期望值与实际值，不做整页像素快照。文档明确「静态检查通过 ≠ UI 渲染通过」。

验证结果：
- 用例或命令：`npm run test:ui`
- 结果：pass（652 passed, 0 failed）；新增 6 条层叠检查器契约判据。
- 真实布局：Darwin 隔离 E2E 共 5 个 spec / 17 条用例全过（console 3、layout 4、memory 6、panels 1、shell 3）。官方 launcher 首跑时 layout 因 `getBoundingClientRect()` 返回的 DOMRect 无法被 WebDriver 序列化而失败，改为返回纯对象后在同一 launcher 产物与隔离根上重跑全套通过。
- 反向验证：layout spec 注入 3000px 元素后检测器判定横向溢出（用例通过视为检测器有效）；主题用例确认深浅 token 变化且卡片宽度不变。
- 后续补强（2026-11-01，实现提交 `2dd83f9e`）：记忆编辑弹窗用例不再只测空表单——填入 80 字长中文标题与含 180 字符无空格 URL 的长正文，断言弹窗右缘、页脚按钮与主区域无横向溢出；release-mode 本地布局 4 例、debug 全量 E2E 6 spec / 22 例通过。
- 另：`npm run check:invariants`、`node --check e2e/specs/layout.spec.mjs` 通过。

未验证范围：用量分析长列表、日志副窗、更多弹窗的布局与滚动；Windows 与 release 安装产物层的真实布局；`:is()` / `:not()` 内部简单选择器特异度、`!important`、简写属性展开仍未建模，需要这些语义时应走真实 WebView。
剩余风险：静态检查器仍是近似模型，不能替代 computedStyle；真实布局覆盖的是代表性页面，不是整站逐元素检查；主题用例通过 class 直切而非走主题按钮，主题切换接线另由 UI 判据覆盖。
回退方式：revert `683ea3a7`。
任务状态：已实现待验证

## 回执 OPT-11

任务编号：OPT-11
实现提交：`52c43ac6`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；debug 隔离 E2E（launcher + WebDriver）

调用图与删除范围：前端入口 `App.vue` 的 `maybeOpenMigrationPrompt` / 面板与弹窗 import / `PANELS.migration` / 启动调用 / 模板挂载，`SideBar.vue` 系统组菜单；后端 `lib.rs` 的 `mod migration` 与 9 条命令注册，`commands.rs` 的 7 条迁移命令，`permissions/app-commands.json` 的 9 条 ACL；专用判据 `migrationPanel.test.js` / `misplacedHome.test.js`，以及设计对齐、暗色主题清单里对迁移文件的引用。`ui/src/migration/` 3 个文件（约 1388 行）与 `src-tauri/src/migration/` 4 个文件（约 2848 行）整体删除，`main.js` 里只被向导使用的 `el-progress` / `el-step` / `el-steps` 注册与样式一并删除；合计 39 个文件、-4849 行。
保留项（未被本次清理改动）：`instance::legacy_migration_target` 与 `~/.dsh` 启动期合并搬迁、`store_relocate` 插件中央库搬迁、历史数据目录 / 迁移标记 / backup 目录。删除的是界面入口、命令与回收逻辑，没有新增任何删除或清理用户数据的代码；`store_relocate` 仍是读取中央库时的自动搬迁路径。
当前文档：README 去掉迁移功能段、使用步骤与目录项；根 / ui / src-tauri 三份 AGENTS 去掉待移除清单与迁移段落；当前架构、内核磁盘布局、诊断设计、UI 指南改为「向导与回收入口已移除、数据与 backup 只读保留」；旧迁移提案保留原文并标注已废止（2026-11-01）。

验证结果：
- `cargo test --locked --lib`：1345 passed、0 failed、13 ignored；`cargo clippy --all-targets -- -D warnings` 与 `cargo fmt --check` 通过。
- `npm run test:ui`：642 passed；`npm run check:invariants` 通过；`npm run test:scripts` 400 passed / 1 skipped；`npm run build:ui` 由 E2E launcher 构建通过。
- 隔离 E2E：5 个 spec / 17 条用例全过（console 3、layout 4、memory 6、panels 1、shell 3）；`result.json` 的 postChecks 为空、exitCode 0，浏览器档案落在 decoy-home/Library/WebKit，运行后无遗留进程，本轮目录检查后已清理。
- 启动行为：侧栏与页面正文不再出现「数据迁移」，空 home 启动不再扫描或弹出迁移提示；`rg` 确认生产代码与权限文件无 migration 命令 / 模块引用。
- 升级态补充验证（2026-11-01）：在 launcher 保留的隔离根中预置 `home/kernels/migration-skipped.json`（拒绝迁移标记）、两组 `home/backups/Auto…/`（含 plugins/skills 与部分失败目录）和 sentinel 文件，然后用同一构建按同一隔离 env 重启壳并跑 `shell.spec.mjs`：3 passed；重启前后对 6 个 fixture 文件做 sha256，清单完全一致（无删除、无改写）；启动正文没有迁移入口或弹窗。这覆盖「已拒绝 / 已迁移 / 部分失败 / 回滚过」留下的标记与 backup 只读保留，不代表 Windows 或 release 安装产物。

反向验证：删除类改动没有可故意破坏的产品行为；本轮用编译 + 全量测试 + E2E 正文断言验证「引用已经断开、入口确实消失」。未做「把菜单加回去看 E2E 是否红」的演练。
未验证范围：Windows 与 release 安装产物；升级态只在 Darwin debug 隔离根验证（预置标记 / backup / sentinel），Windows / release 安装包的升级态未验证；真实用户目录上的旧数据保留与启动自动搬迁——没有在真实 home 上执行，只确认代码未新增删除路径。
剩余风险：E2E 只证明新装 Darwin debug 壳的入口消失；升级用户的历史 backup 与迁移标记现在没有界面入口，用户需要时只能从磁盘取；如需恢复入口，应作为新的显式工具，而不是恢复向导。
回退方式：revert `52c43ac6`。
任务状态：已实现待验证

## 回执 OPT-12

任务编号：OPT-12
实现提交：`069a1f9d`
审查基线：`f448970f`
执行平台及构建模式：Darwin x86_64；UI node:test + Rust 单测 + 隔离 E2E（debug + WebDriver）

问题与触发条件：`bridge.js` 只有裸 `invoke` / `makeChannel`，四组高频接口的请求/响应/错误/事件含义散落在 store 与 Rust DTO 两份实现里；参数名、必需字段或枚举漂移时没有任何检查会红。
实际修改的入口：新增 `contracts/ipc-contract.json`（单一契约）与 `ui/src/shell/ipc-schema.js`（通用校验器）、`ui/src/shell/ipc-contract.js`（`ipcCall` 包装）；`bridge.js` 增加 `hasBridge()`；`store.js` 的 `get_status` / `start_kernel` / `stop_kernel`、`memory.js` 的 `memory_list`、`usage.js` 的两处 `get_model_usage` 切到 `ipcCall`；Rust 新增 `shell/ipc_contract.rs`（仅测试构建）用真实 DTO 序列化对照契约；新增 `e2e/specs/ipc.spec.mjs`。
关键行为变化：请求先校验、响应后校验；失败分 `contract`（请求/响应不符，调用不会到达 Rust 或结果被拒收）/ `transport`（桥未注入）/ `business`（后端 `Err(String)` 原文保留），错误码来自契约 `errorCodes`。`start_kernel` 的 progress Channel 约定为「只发字符串、命令 settle 后不再有事件」。契约还声明了必需字段、可空字段、枚举与四组来源，均可在同一文件追踪。
兼容影响及保留路径：其余命令继续走裸 `bridge.invoke`，未全量迁移 TypeScript / Pinia、未重写 store、未新增第二套状态真值；四组原有返回结构未改，只增加边界校验与错误分类。测试夹具补齐真实 DTO 形状，不改变产品字段。

验证结果：
- `npm run test:ui`：652 passed；新增 `ipcContract` / `ipcCall` / `ipcTransport` 判据并修正受契约影响的既有夹具。
- `cargo test --locked --lib`：1351 passed / 13 ignored（含 6 条 `shell::ipc_contract` 对照测试）；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 通过。
- `npm run check:invariants`、`npm run test:scripts`（400 passed / 1 skipped）、`npm run build:ui` 通过。
- 隔离 E2E：6 个 spec / 22 条用例全过（console 3、ipc 5、layout 4、memory 6、panels 1、shell 3）；`result.json` exitCode 0、postChecks 空、浏览器档案在 decoy-home/Library/WebKit、无遗留进程，检查后清理运行根。
- 反向验证：`ipcContract` 人为把必需字段改名、请求参数名漂移、枚举改成不匹配值、删除必需字段、响应多出契约为声明字段，均按预期抛 `IpcContractError`；`ipcCall` 验证请求非法时不调用 invoke、业务失败归 business、响应漂移归 contract；`ipcTransport` 验证纯浏览器归 transport。
- 真实行为：隔离壳里 `get_status` / `stop_kernel` / `start_kernel`（空 home 返回 `running=false` 报告）/ `memory_list`（空数组）/ `get_model_usage`（90 天零值窗口）都通过同一契约校验；Channel 只发字符串且终止后不再推事件。
- 补充 release-mode 本地验证（2026-11-01，HEAD `616820b5`）：release 二进制（本地 `--features wdio` 构建，非安装产物）在临时 home 预写独立端口 35001 后，`ipc.spec.mjs` 5 passed / exit 0；`status.kernel.running=false` 且端口为隔离端口。仍是带 WebDriver 的本地构建，不替代无 WebDriver 的 release 安装产物验收。

未验证范围：Windows 与 release 安装产物层；真实成功启动内核时的 `StartReport`（含 warning/incident 组合）与完整 Channel 流；真实记忆与用量数据下的可选/可空字段组合；Tauri 对非法参数的错误文本仍不区分具体错误码，contract 只归到 transport/business/contract 三类。
剩余风险：契约校验是运行时严格校验，后端新增未声明字段会被拒收，需要同步契约（这是有意的）；`ipcCall` 只覆盖四组，其余命令仍无契约；错误码目前是三类分类而非每条业务错误的稳定码。
回退方式：revert `069a1f9d`。
任务状态：已实现待验证

## 回执 OPT-13

任务编号：OPT-13
实现提交：—（资源未齐，无代码变更；本轮只做盘点与状态登记）
审查基线：`371158f1`
执行平台及构建模式：Darwin x86_64；本机与仓库/CI 配置盘点 + updater 发布脚本判据，未构建签名产物

**2026-11-01 用户决定（范围变更）**：不采购 Apple Developer ID / Windows Authenticode 等正式证书，发布继续沿用现有 GitHub Actions workflow（只做 Tauri updater 制品签名，不做 OS 代码签名、公证或 stapling）。用户明确接受 Windows SmartScreen 与 macOS Gatekeeper 的未受信任提示。因此本任务按 **updater-only 发布** 验收，原系统签名验收项记 `not_applicable`，不伪装成 pass。

盘点结果（2026-11-01）：
- 系统签名现状：`src-tauri/tauri.conf.json` 只有 updater 的 `pubkey` / `endpoints`，`bundle.macOS` 只有 `minimumSystemVersion`，`bundle.windows.nsis` 只有 `installMode`；没有 `signingIdentity` / `certificateThumbprint` / entitlements / notarization 配置。
- CI 现状：`.github/workflows/desktop-release.yml` 的构建步骤只注入 `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 给 updater；没有 `codesign`、`notarytool`、`stapler`、`signtool` 或 `Get-AuthenticodeSignature` 步骤。runner 具备 `macos-15-intel` 与 `windows-latest`，`publish` 有 `contents: write`。
- 本机：`security find-identity -v -p codesigning` 与不带策略的 `find-identity -v` 都是 **0 valid identities**；环境变量里没有 APPLE / TAURI / WINDOWS 签名相关项。
- 仓库 Actions secrets：`gh secret list` 只有 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（外加自动 `GITHUB_TOKEN`）；没有 Apple Developer ID / 公证凭据 / Windows 代码签名证书或 HSM 配置。
- 现有发布资产（desktop-v0.6.5）包含 `.exe`、`.exe.sig`、`.app.tar.gz`、`.app.tar.gz.sig`、`.dmg`、`latest.json`；README 与排障页已如实说明操作系统未签名。

缺失资源（2026-11-01 起不采购，保留为将来可选接入清单，不再是阻塞项）：
1. Apple Developer Program 的 Developer ID Application 证书与私钥、Team ID；或将来改用 App Store 分发时的对应身份。
2. Apple notarization 凭据：App Store Connect API key（issuer / key id / .p8）或 Apple ID + app-specific password；以及 `stapler` 所需的最终产物。
3. Windows Authenticode 代码签名证书与私钥（或 Azure Trusted Signing 的账户 / 证书配置）；证书主体、指纹、有效期。
4. 证书归属人、续期负责人、首次启用日期与更换身份时的兼容影响登记责任人。
5. 受控 CI secrets 的注入方案（不入库、不进入构建日志）与带签名 / 公证的构建、验证步骤；当前尚未配置，不能声称已具备。

现有 updater 链路核对（不等于系统签名完成）：`scripts/normalize-release-assets.mjs` 要求恰好一份 macOS `.app.tar.gz.sig` 与一份 Windows `*-setup.exe.sig`；`scripts/generate-updater-manifest.mjs` 从最终 artifact 目录生成 `latest.json`，缺失或版本不符会失败。因此「更新签名失败仍发布伪装成已签名的产物」在当前流水线上会被缺失 `.sig` / 清单校验挡住；但 updater 签名只覆盖更新制品，不能替代 Windows Authenticode 或 macOS Developer ID + notarization。本轮没有修改这条链路。

验收口径（2026-11-01 用户决定后）：
- `not_applicable` Windows 最终应用与安装包的系统签名验证——用户不采购证书，不作为完成条件。
- `not_applicable` Intel macOS 最终产物的签名、公证及分发验证——同上。
- [x] 更新端使用最终字节，更新签名和清单对应同一批制品：`scripts/normalize-release-assets.mjs` 要求恰好一份 `.app.tar.gz.sig` 与 `*-setup.exe.sig`，`scripts/generate-updater-manifest.mjs` 只从最终 artifact 目录生成 `latest.json`；`npm run test:release-assets`、`npm run test:release-manifest` 通过。
- [x] 签名失败不会发布伪装为已签名的产物：缺 `.sig` 或版本不符时上述脚本失败；`node --test scripts/check-signing-keys.test.mjs` 7 项通过。
- [x] 仓库、日志、证据包不含私钥或凭据：本轮只读取 secret 名称，未读取值，未出现凭据。
- [x] 缺少资源时列明并降级范围：本回执与 README / 发布手册同步记录用户决定，不将 updater 签名等同系统签名。

现有发布资产（desktop-v0.6.5）：`.exe`、`.exe.sig`、`.app.tar.gz`、`.app.tar.gz.sig`、`.dmg`、`latest.json`；与 workflow 的 updater-only 产物一致。

未验证范围：系统签名、公证、stapling、Windows Authenticode 验证均不执行（用户决定不采购证书）；没有触发发布流水线，也没有对真实候选安装产物做人工门禁。
剩余风险：用户下载的产物仍会触发 SmartScreen / Gatekeeper 未受信任提示；updater 签名只保护更新制品，不能替代系统签名。后续若采购证书，需要重新打开本任务并执行发布手册里的签名 / 公证 / 验证清单。
回退方式：本任务无代码变更；如需恢复阻塞状态，回退本轮文档提交并把状态改回 blocked_resource。
任务状态：完成（系统签名按用户决定豁免；仅保留 updater 签名）
