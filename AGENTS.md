# AGENTS.md — dsh-xlink

本仓库是 dsh-xlink 桌面应用的独立项目。模块布局与数据流见 [docs/architecture.md](docs/architecture.md)，用户文档见 [README.md](README.md)。

## 范围

- **独立项目**：仓库根目录就是桌面交付物，不加入任何上级 pnpm workspace，也不依赖源仓库的构建、测试或发布门禁。根目录 `pnpm-workspace.yaml` 让 pnpm 将本项目作为独立根目录处理，直接运行 `pnpm install` 即可。
- **运行时内核边界**：项目不携带或重新发布 dsh 内核代码。内核由用户从 npm registry 安装；桌面壳通过 `src-tauri/` Rust 进程和 `ui/` 管理面板管理其生命周期、配置和窗口行为。
- **信任边界**：`@deepseek-ai` 命名空间限制**只覆盖内核与 Node 运行时两条路径，不覆盖插件与技能**；GitHub 来源同样不限制仓库归属。版本列表优先 npm registry，GitHub Releases 仅作回退。
  - **已强制的两处**：内核包名在 `kernel.rs` 硬编码为 `DSH_NPM_PACKAGE`；托管 Node 在 `node_install.rs` 硬编码版本并校验 SHA-256。
  - **未强制的两处**：`plugins.rs` 与 `skills.rs` 的 `parse_spec` 对 npm 包名只做字符类校验（`alphanumeric` 与 `-._@/`），`lodash`、`@attacker/backdoor` 均可安装。改这两处时不要误以为 `pkg.rs` 已帮你拦下——`pkg.rs` 是插件与技能**共用**的取源层，两条路径都走它，但它不检查命名空间。
  - npm 基础 URL 默认指向 **npmmirror 镜像**（`registry.rs::DEFAULT_NPM_REGISTRY`），以便国内网络无需改全局 npm 配置即可安装；需要上游 registry 的部署用 `DSH_NPM_REGISTRY` 覆盖。镜像只影响**取源**，不影响信任判定：下载的 tarball 逐字节校验（`releases::verify_download_integrity`）——优先用 npm 元数据的 `dist.integrity`（SRI，按 token 取最强且受支持的 sha512/sha256），只有它给不出可用摘要时才回退到老 packument 的 `dist.shasum`（sha1），两条都没有则拒绝安装（fail-closed）；校验失败即删除并拒绝安装。
  - 已知缺口（`code-review-2026-09-27.md` M7–M10）：`DSH_NPM_REGISTRY` 接受 `http://`（`registry.rs::resolve` 不解析 URL）；`dist.tarball` 无 host 校验，摘要可由明文 registry 对手伪造；git 来源插件不校验摘要却会执行归档内 `prepare` 脚本。

## 开发规则

- 搜索文本或文件时优先使用 `rg`；仅在不可用时再使用 `grep` 等替代命令。

## 命令

```sh
npm run deps                      # 安装依赖（pnpm 优先，缺失回退 npm）
npm run dev                       # tauri dev（自动先起 vite dev server，5174 热更新）
npm run dev 5190                  # 同上但换端口；vite 的 server.port 与 tauri 的 devUrl 由 scripts/dev.mjs 统一（DSH_DEV_PORT）
npm run dev:ui                    # 只起管理面板 dev server（纯前端迭代，浏览器里无 Tauri 桥）
npm run build                     # 本机构建（.dmg / NSIS；自动先 vite build → ui/dist）
npm run build:ui                  # 只构建管理面板 → ui/dist
npm run check:code-budget         # 生产代码行数预算 + 重复区间门禁（防膨胀）
cargo check                       # 在 src-tauri/ 内：快速编译检查
cargo clippy --all-targets        # lint，零警告基线
cargo fmt                         # rustfmt 格式化
```

UI 是 Vue 3 + Element Plus 单页应用（源码 `ui/src/`，Vite 构建到 `ui/dist/`，即 `src-tauri/tauri.conf.json` 的 `frontendDist`）。状态与动作集中在 `ui/src/store.js` / `plugins.js` / `skills.js` / `progress.js` / `logs.js`，异步样板（在途去重、静默刷新、更新检查策略）在 `async.js`，组件只读状态、调动作；与 Rust 的通信只允许走 `ui/src/bridge.js` 的 invoke/Channel。触发 IO 的按钮必须挂 loading（`loading.js` 的 `withLoading(key, …)` + `:loading="isLoading(key)"`）；长任务走 `progress.js` 的 `withProgress`。改完 UI 跑 `npm run build:ui`；Rust 改动至少跑 `cargo check`，提交前跑 `cargo clippy --all-targets && cargo fmt`。

## 数据目录

`kernel::data_dir` 按**内核族命名空间**解析：`<xlink_home>/<family>/desktop/`（release）或 `<xlink_home>/<family>/desktop-dev/`（debug），family 取实例注册表默认实例的内核族（`instance::default_family()`）；v0.2.x 的平铺目录 `<xlink_home>/desktop[-dev]/` 会在启动时自动整体搬入族目录，搬迁失败继续用旧目录。`lib.rs` 的 `setup()` 必须通过它取目录，不要绕回 `app_data_dir()`。debug 端口 3091，release 端口 3090（`kernel::DEFAULT_PORT`）；用户保存过的 port 优先于 `Settings::default()`。优先级与目录隔离原因见 [docs/architecture.md](docs/architecture.md)。

**壳的数据目录分家了，实例也必须分家**：dev 壳的默认实例是 `default-dev`，release 是 `default`（`instance::default_instance_id_for`）。两者共用一个实例时，dev 换一次内核版本、改一次插件接线就会重写共享实例的 `profiles/web/`，dev 更新中央库里的插件源码会被 link 物化直接送到 release 正在跑的内核上——工作台白屏（实测 `scope '…' rendered without an installed adapter`）。所有生产 caller 走 `instance::resolve_default()` 或 `plugins::default_instance_key()`，**不要 hard-code `DEFAULT_INSTANCE_ID`**；注册表的 `default_instance_id` 是共享的一份，**只有 release 能把它指向某个实例**（`ensure_default_registered` 只在无人认领时写），壳自己「切到哪个实例」写 `settings.current_instance_id`（`instance::set_current_instance_id`），**任何模块都不得把它指向具体 id**——清空成 `None` 是修复，不算抢；`~/.dsh` 历史数据只搬进 release 实例（`legacy_migration_target`）。两个壳指向同一实例且那个内核还活着时，插件与内核版本变更会被 `instance::ensure_instance_mutable` 拒绝。这两条纪律由 `check:invariants` 第 10 / 11 项机械兜底：① 生产代码里 `InstanceRecord::new` 的 id 实参不得是常量；② 除 `instance.rs` 外不得写 `default_instance_id = <具体 id>`。

## 实现约定

- 用户可见文案用简体中文；错误信息必须包含可操作的下一步与相关日志路径。
- 概览页的状态机只有一个**主按钮**「工作台」：文案恒为名词，启停方向由 icon（▶ 启动 / ⏸ 停止）与 hover title 表达；同排的「官方对话」用同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」（运行后从下方淡入）与「查看日志」是并列的次级入口（都不改变内核状态），不要再往主按钮旁边加会启停内核的动作。
- 长任务失败时进度面板保持开放，由用户手动关闭；完整原始输出始终落盘，报错信息引用日志路径。
- 所有 GUI 子进程使用 `process.rs` 的 PATH 合并、静默窗口和进程组回收策略；涉及进程、网络或目录树的 Tauri 命令必须异步执行并使用 `spawn_blocking`。
- **代码预算门禁已改成反棘轮，不要绕开它**。`npm run check-code-budget` 现在从 git 读出本文件**已提交版本**的数字当基线（HEAD 里的旧值仍然是旧值，所以「同一个提交里改数字」这个流程不会让检查失守），并强制三条规则：① 基线预算 ≥ `RATCHET_THRESHOLD` 的**大文件只许下调**（plugins.rs / theme.css / commands.rs 只能越来越小）；② **新文件**必须显式登记进 `FILE_BUDGETS` 并写清为什么该独立，且受 `HARD_FILE_CEILING`（800 行）硬顶；③ 总量 `TOTAL_BUDGET` 是一道**软上限**（刻意不做"只许下调"——试过，实践中只会逼人绕过门禁而不是真写出更少的代码）。被规则 ① 拦下时的正确反应是**把新逻辑拆出去**，不是调数字。
- **跨模块重复先提共享层**：包取源逻辑放 `pkg.rs`（插件与技能共用，只返回纯文本原因、错误分类由调用方决定），JSON 状态文档读写放 `state.rs`（容错读 / 校验读分开，文案由 `StateCtx` 提供），前端异步样板放 `ui/src/async.js`（`singleFlight` / `createStatusSource` / `createUpdateChecker`）。安装预检拆成两层：`sandbox.rs` 只管"起一个临时内核、探它、收摊"（与装什么无关，技能预检直接复用），`precheck.rs` 管两段式事务（快照 → 装进沙盒 → 探测 → 提交/回滚），`plugins.rs` 侧只暴露 `store_file` 一条可见性缝——**预检的事务主体不要写进 `plugins.rs`**，它已经 2964 行。`npm run check:code-budget` 按文件代码行数与重复区间数拦膨胀：要上调预算，必须在同一个提交里改 `scripts/check-code-budget.mjs` 的数字。
- **变更配置前必须留下回退点**。装 / 卸 / 更新插件、切物化模式、切内核版本之前调 `snapshot::record(..., reason::PRE_CHANGE)`；只有**真正起来并应答过、且没有事故**的启动才算 `startup-ok`（带事故启动的环境不算"良好"——看护停用两个插件才起来的状态，记成良好会让恢复把"被降级过的样子"当成用户原本的样子）。改动走 `run_plugin_mutation_command` 而不是 `run_plugin_command`：**读类命令（检查更新、拉目录）打「变更前」是假的**，会把真有价值的回退点挤掉。打点**绝不阻断用户操作**——快照写不进去只写 stderr。裁剪时 last-known-good 进保护区，有测试钉死（`snapshot::tests::prune_never_drops_the_last_known_good`）。P0 只有只读面，恢复属 P1，见 [docs/safety-net-design.md](docs/safety-net-design.md)。
- **二分定位的结论不得叫「根因」**。`bisect.rs` 的 `Conclusion` 只有 `minimal-bad-set` / `not-in-set` / `aborted` 三种取值，**没有"找到根因"**——组合效应（两个扩展单独都正常、一起就炸）会让二分停在一个不可修的答案上，把它说成根因会让用户去卸一个无辜的插件。试探的判据走 `verify::probe_once`，与恢复后自检**同一条**：判据一旦有两份实现就会分叉，而分叉出来的那个会让二分**静默收敛到错误答案**（把"没试成"当成"起来了"，坏的那半边被记成已排除）。`Inconclusive` **绝不等于** `Pass`。
- **恢复必须先看差异，且只改差异项**。`restore::diff` 与 `restore::restore` 是**两条命令**，不要合成一条：用户必须先看见将要失去什么再点确认，合一意味着要点一次「恢复」才知道后果。`restore::restore` 遵守四条硬规则：动手前先 `pre-restore` 备份（**备份失败必须中止**，没有回退点的恢复是单向操作）、逐条比对只改不一致的项、**只停用不卸载**（插件写 quarantine 记录，技能走 `set_enabled`，两者都不删中央库条目——卸载不可逆，让一次回退顺手做了等于用恢复换数据）、动不了的条目进 `skipped` 照实报。`Restorable: false` 的条目必须在**用户确认之前**就标注出来；事后才说等于让用户在一个不完整的承诺上点了确认。恢复后用 `verify::probe_once` 实测一遍，`verified` 必须如实反映——让"没验"看起来像"验过没问题"是最伤信任的错。恢复要求工作台已停止。
- **预检类功能必须先跑基线再判失败**。装了候选扩展起不来，不能直接判"扩展坏了"——可能是环境本来就坏了。`guard.rs` 早就为同一个问题付过代价（宁可放弃插件归因，也不肯因为环境问题停用无辜插件），预检沿用同一条纪律：不装任何东西先起一次作为基线，只有基线正常、装了候选才失败，才判 `Fail`。判定必须三态（`pass` / `fail` / `inconclusive`），Rust 侧的 `Verdict::as_str()` 与 `PrecheckDialog.vue` 的判定表靠字符串对齐，有测试钉死（`precheck::tests::verdict_strings_match_the_ui_contract`），改一边不改另一边会把"未通过"画成"通过"。
- 图标只从 `assets/whale-icon.svg`、`assets/whale-icon-small.svg` 与托盘专用的 `assets/whale-head.svg` 生成，规则见 [docs/icon-design.md](docs/icon-design.md)。

## 发布

版本发布由 `.github/workflows/desktop-release.yml` 负责。发布前必须确认 `package.json` 与 `src-tauri/tauri.conf.json` 的 `version` 完全一致，并且版本提交已经推送到 `main`。workflow 使用 `TAURI_SIGNING_PRIVATE_KEY` 给更新制品签名，`releaseDraft` 与 `prerelease` 必须保持为 `false`，以保证 updater 的 latest endpoint 可用。

### 发布触发

- tag 格式固定为 `desktop-v<version>`，例如 `desktop-v0.1.2-rc.7`。
- 推荐在目标 commit 上创建 tag，再推送 tag，让 tag push 自动触发发布：

  ```sh
  git fetch origin main --no-tags
  git tag desktop-v<version> <main-commit>
  git push origin desktop-v<version>
  ```

- 创建或更新 tag 后不要再手动 dispatch 同一版本。后启动的 Run 可能在 Release 已发布后按保护逻辑失败。
- workflow 使用固定并发组（`group: desktop-release`）串行化全部发布；`preflight` 还会断言待发布版本严格大于线上 `latest.json` 的版本（`releases/latest` 取"最近创建"而非 semver 最大值，回退发布会让高版本用户静默收不到更新）。所有 `uses:` 固定到 commit SHA，升级走 Dependabot。
- 手动 dispatch 只用于已有正确 tag、且没有相同版本 Run 正在执行的情况。不要用手动 dispatch 创建缺失的 tag，否则 workflow 创建 tag 后会再次触发 tag push Run。
- `desktop-v<version>` tag 和手动 dispatch 只接受 `main` 分支上的 commit；不要从其他分支或未推送的本地 commit 发布。

- **发布平台**：dsh-xlink 只发布 Intel macOS（`macos-15-intel`）和 Windows（`windows-latest`）版本；不得添加、构建或发布任何 Linux/Ubuntu 版本、runner、制品或文案。

### Release 资产

- 发布前必须确认最终资产为 5 个平台资产加 `latest.json`，共 6 个文件：Windows 安装包及签名、Intel macOS DMG、macOS updater 包及签名、`latest.json`。
- 创建 Release 前必须先确保目标 tag 存在，避免生成 `untagged-*` Release。
- 草稿 Release 的资产操作使用 Release ID，不要依赖 tag 查找或上传资产。
- 资产接口必须使用以下路径：
  - 列表：`GET /repos/{owner}/{repo}/releases/{release_id}/assets`
  - 删除：`DELETE /repos/{owner}/{repo}/releases/assets/{asset_id}`
  - 上传：`https://uploads.github.com/repos/{owner}/{repo}/releases/{release_id}/assets`
- 上传资产时使用 `Content-Type: application/octet-stream`，并通过文件输入上传；不要把上传请求发到普通 `api.github.com` 地址。

### 失败排查与发布后验证

- workflow 失败时先定位具体 job 的失败日志，再修复或重试：

  ```sh
  gh run view <run-id> --job <job-id> --log-failed
  ```

- 发布完成后检查 Release、tag 和 updater 清单：

  ```sh
  gh api repos/July-X/dsh-xlink/releases/tags/desktop-v<version>
  gh api repos/July-X/dsh-xlink/git/ref/tags/desktop-v<version>
  curl -fsSL https://github.com/July-X/dsh-xlink/releases/latest/download/latest.json
  ```

## 文档

修改用户可见行为、数据目录、发布流程或安全策略时，同步更新 `README.md` 和对应 `docs/` 文档。文件保持 UTF-8、恰好一个末尾换行；不要提交依赖目录和构建产物。
