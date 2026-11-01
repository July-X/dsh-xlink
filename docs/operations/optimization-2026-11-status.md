# 2026-11 优化任务状态表

本文件是 [优化方案任务书](optimization-2026-11-taskbook.md)（同目录原稿）的 OPT-00 交付物，也是本轮任务状态的唯一真值。任务书里的编号、验收标准与依赖保持不变；状态、commit 与证据只记在这里。

## 1 基线与实施边界

| 项目 | 记录 |
| --- | --- |
| 审查基线 | `f448970f`（2026-11-01，任务书编制时） |
| 实施起始 | `f448970f`（`git describe` = `desktop-v0.6.5-1-gf448970f`） |
| 核对基准 | `b9c46d3d`（2026-11-01 docs 提交；`git diff f448970f..HEAD --stat` 只有 `docs/` 三处，代码与审查基线相同） |
| 最近发布 | `desktop-v0.6.5` → `78278952`（起始 commit 比发布 tag 多 1 个提交） |
| 当前版本 | `package.json` / `tauri.conf.json` / `Cargo.toml` 均为 0.6.5 |
| 分支 | `main`（未推送任何本轮改动时先按本地提交处理，推送与发布另行授权） |
| 本机平台 | Darwin x86_64（macOS 26.7.2）；Node v25.9.0；pnpm 10.34.6；rustc / cargo 1.99.1 |
| 运行壳 | release v0.6.5，内核 0.2.2-alpha.2，端口 3090（2026-11-01 15:01 启动） |
| 任务书原稿 | `optimization-2026-11-taskbook.md`，sha256 `da22c9334be704337e4db0e25f7455054e8b46d57a77b3921267470cd18512ab` |

**开工核对**：每次开工先跑 `git rev-parse HEAD`。与审查基线一致 → 直接开始；不一致 → 先核任务书「八 审查事实」里对应调用链是否漂移，把结论写进该任务回执再动代码。已修复且证据齐全的项直接关闭，不重复开发。2026-11-01 已按 HEAD `b9c46d3d` 做过一次全量核对（结论见 §3.1），后续开工只需核对与基线的差异。

**核对时的工作区状态**：核对基于 `b9c46d3d`（仅 docs，代码基线 `f448970f`）。当时工作区里**其他会话**的暗色主题改动随后由该会话提交为 `a9612f17 feat(ui): 暗色主题换成方案 B「夜航日志」`（`ui/**` 与 `docs/ui/**`，没有 `src-tauri/` 变更），与本轮任何任务无关。本轮证据读的是 `b9c46d3d` 版本，之后 UI 文件行号可能漂移（Rust 证据不受影响）。提交时只 `git add` 本任务文件。

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
| OPT-02 | P1 | 统一安全取源与重定向策略 | OPT-00 | 待实施 | — | — | `pkg/registry.rs:44-68` 补 env 覆盖的 scheme 校验；`pkg/fetch.rs:129` 校验 tarball URL 与重定向；`plugins/center.rs:867` 收紧 git 白名单 |
| OPT-03 | P2 | 修复桥接换代与私有凭据写入 | OPT-00 | 待实施 | — | — | `plugins/builtin/bridge.rs:107-131` 换代时旧代只清自己的凭据；`bridge_credential.rs:68-76` 改私有原子写（tmp → chmod → rename） |
| OPT-04 | P2 | 统一 Unix 进程组停止策略 | OPT-00 | 待实施 | — | — | `kernel/lifecycle.rs:1833-1856`：组长退出后仍收尾组内存活后代，不能跳过组 SIGKILL；夹具覆盖「组长立即退出 + 后代忽略 TERM」 |
| OPT-05 | P2 | 按实例身份恢复遗留内核 | OPT-00、04 | 待实施 | — | — | `lib.rs:233-237` / `lifecycle.rs:1795,1812`：用实例 PID/runtime 记录替换 `cwd == data_dir` 判据，默认与外部 workspace 各一组夹具 |
| OPT-06 | P2 | 绑定预检与正式安装物料 | OPT-01、02 | 待实施 | — | — | `plugins/precheck.rs:602-695`：应用改为消费不可变计划；现在 `:652` 仍按 spec 重新 `install_for_instance`，`verified_at_ms` 只用于提示 |
| OPT-07 | P2 | 明确兼容性预检的信任边界 | OPT-01、06 | 待实施 | — | — | 文案统一「兼容性预检」（现 `PluginsPanel.vue:432` / `PluginDiagnosis.vue:39-41` 仍叫「预检」）；报告分阶段，prepare 权限说明见 `center.rs:1501`、边界见 `sandbox.rs:13-29` |
| OPT-08 | 验证前置 | 建立隔离且跨平台的 E2E 启动器 | OPT-00 | 待实施 | — | — | `e2e/wdio.conf.mjs:23-85`：统一 launcher 自建测试根/档案/通道，产物名含 Windows `.exe`，并拒绝复用未隔离的旧 debug 产物 |
| OPT-09 | P2 | 建立双平台真实行为与发布回归 | OPT-03、04、05、06、08 | 待实施 | — | — | 先修 `e2e/specs/memory.spec.mjs:129-132,196-199` 的 Transition 挂载 skip；再补面板/IPC 实际挂载断言，skip 不算通过 |
| OPT-10 | P3 | 校准 CSS 静态检查与真实布局验证 | OPT-08；真实布局依赖 OPT-09 | 待实施 | — | — | 校准 `ui/test/lib/css-cascade.mjs:83-126,190`（media 条件被展开、选择器列表按逗号计数、重复声明只取第一处）；真实渲染走 WebView computedStyle/边界/截图 |
| OPT-11 | P3 | 清理迁移向导与当前状态文档 | OPT-00；原生回归依赖 OPT-08 | 待实施 | — | — | 按调用图删四块（HEAD 证据见 §3.1）；`legacy_migration_target` / `store_relocate` 保留，capabilities 按 invoke 图核对 |
| OPT-12 | P3 | 渐进建立 IPC 契约与职责边界 | OPT-00；真实验证复用 OPT-09 | 待实施 | — | — | 四组接口建单一契约；`ui/src/shell/bridge.js:20-33` 现在只有裸 `invoke` / `makeChannel`，无类型/契约来源 |
| OPT-13 | P3 | 完善系统代码签名与发布验证 | OPT-09 及外部证书资源 | 待实施 | — | — | 补系统签名与公证；现在只有 updater 签名（`desktop-release.yml:280-284`），README 明说未做 OS 代码签名；资源未齐记 `blocked_resource` |

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
| OPT-13 | 成立 | `tauri.conf.json` 只有 updater `pubkey` / `endpoints`；`desktop-release.yml:280-284,331-332` 只有 `TAURI_SIGNING_PRIVATE_KEY`（更新制品签名）；`README.md:25`、`:345` 明写未做操作系统代码签名 | 证书资源未齐记 `blocked_resource`；不要把更新签名通过写成系统签名完成 |

## 4 里程碑

| 里程碑 | 包含 | 状态 | 退出条件 |
| --- | --- | --- | --- |
| M0 验证准备 | OPT-00、08 | 进行中（OPT-00 完成，OPT-08 待实施） | 基线（本文件）、隔离启动器与证据模板可用，启动真实测试壳前由代码核验隔离条件 |
| M1 优先修复 | OPT-01、02、03 | 未开始 | 对应缺陷行为回归通过，无静默换源、无新凭据误删 |
| M2 生命周期与安装闭环 | OPT-04、05、06、07 | 未开始 | 停止与恢复结果诚实，应用内容与预检绑定 |
| M3 原生验证与收口 | OPT-09、10、11 | 未开始 | 双平台必测项通过，跳过与未执行明确列出 |
| M4 后续完善 | OPT-12、13 | 未开始 | 分项交付；缺证书不阻塞前面代码修复 |

## 5 证据与回执

- 回执写在本文件末尾（`## 回执 OPT-XX`），表格「证据」列填对应锚点如 `#回执-opt-01`。
- 回执至少包含：实现提交、审查基线、平台与构建模式、触发条件、改到的生产入口、关键行为变化、兼容影响、实际命令与退出码、反向验证、未验证范围、剩余风险、回退方式、最终状态。
- 原始日志与截图不入库；需要留图时放 `docs/operations/evidence/2026-11/OPT-XX/`（只在确有二进制证据时创建）。测试 home、浏览器档案、单实例通道一律放在仓库外的本轮临时目录，且与证据目录分离。
- 账号、token、真实会话正文、私钥不出现在回执与证据里。

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
- [ ] 各任务回执在首次实施后按 §5 模板补齐。

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
