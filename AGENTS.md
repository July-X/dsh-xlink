# AGENTS.md — dsh-xlink

本仓库是 dsh-xlink 桌面应用的独立项目。模块布局与数据流见 [docs/architecture.md](docs/architecture.md)，用户文档见 [README.md](README.md)。

## 范围

- **独立项目**：仓库根目录就是桌面交付物，不加入任何上级 pnpm workspace，也不依赖源仓库的构建、测试或发布门禁。根目录 `pnpm-workspace.yaml` 让 pnpm 将本项目作为独立根目录处理，直接运行 `pnpm install` 即可。
- **运行时内核边界**：项目不携带或重新发布 dsh 内核代码。内核由用户从 npm registry 安装；桌面壳通过 `src-tauri/` Rust 进程和 `ui/` 管理面板管理其生命周期、配置和窗口行为。
- **绝不往已安装的内核目录里写任何东西**。npm 下载下来是什么，用户拿到的就是什么——原则上 dsh-xlink 只是替用户输入了一次 `dsh web` 启动指令。**唯一允许的写入**是 `install_version` 在装新版本时写的 pnpm stub（`package.json` / `pnpm-workspace.yaml`）与 pnpm 自己产生的 `node_modules`；`uninstall` 的整目录删除是用户显式要求的操作。**除此之外，`kernels/<版本>/node_modules/**` 里的一切都只读**。
  - stub 里那两处 `overrides`（依赖钉版）也在允许范围内——它决定的是**装哪个版本**，不是改内核代码。落点统一在 `kernel_deps.rs`，`kernel.rs` 不再自己写 stub。
  - 这条不是洁癖。2026-09-29 有人给 `dsh-client-ui-renderer/lib/client.js` 打过一个「作用域缺席时挂起而不是抛错」的补丁（随 0.3.5-rc.4 发布），很快被认定为违背定位而撤销。**用「可逆 + 有备份 + 有哈希校验」论证补丁的安全性，答的是「破坏可不可逆」，不是「该不该做」**——两者是不同的问题，后者这里已经答完了。
  - 内核侧的缺陷（启动顺序竞态、监视根过宽、渲染器 fail-loud 检查）**由内核仓库负责**。壳要做的是三件事，都不碰内核：减少触发（工作台运行期间禁止装/删内核）、出了事能自愈（页面内一次性自愈）、出了事说得清（`cause` 归因与文案）。
  - `src-tauri/resources/patches/` 这套内置补丁机制与本条冲突。目录里现存 `dsh-file-perf`（已标记 `supersededSinceKernelVersion`）。**新增补丁前先回头看这条规则**；机制是否整体废弃由维护者决定，不在代码里自作主张。
  - **从仓库里删掉补丁目录，并不会让已安装的用户机器上消失**。2026-09-29 实测：删掉两个补丁后重新装 rc.4，`<install>/patches/` 下**两个目录仍在**——更新只覆盖与新增，不清理上游删掉的东西。要真正清干净，得让应用自带一份「本次构建带了哪些补丁 id」的名单（构建期生成），再在 `load_patches` 时按它裁剪资源目录。在此之前，**删除补丁后必须在发布说明里写明它不会自动卸载**。
- **信任边界**：`@deepseek-ai` 命名空间限制**只覆盖内核与 Node 运行时两条路径，不覆盖插件与技能**；GitHub 来源同样不限制仓库归属。版本列表优先 npm registry，GitHub Releases 仅作回退。
  - **已强制的两处**：内核包名在 `kernel.rs` 硬编码为 `DSH_NPM_PACKAGE`；托管 Node 在 `node_install.rs` 硬编码版本并校验 SHA-256。
  - **未强制的两处**：`plugins.rs` 与 `skills.rs` 的 `parse_spec` 对 npm 包名只做字符类校验（`alphanumeric` 与 `-._@/`），`lodash`、`@attacker/backdoor` 均可安装。改这两处时不要误以为 `pkg.rs` 已帮你拦下——`pkg.rs` 是插件与技能**共用**的取源层，两条路径都走它，但它不检查命名空间。
  - npm 基础 URL 默认指向 **npmmirror 镜像**（`registry.rs::DEFAULT_NPM_REGISTRY`），以便国内网络无需改全局 npm 配置即可安装；需要上游 registry 的部署用 `DSH_NPM_REGISTRY` 覆盖。镜像只影响**取源**，不影响信任判定：下载的 tarball 逐字节校验（`releases::verify_download_integrity`）——优先用 npm 元数据的 `dist.integrity`（SRI，按 token 取最强且受支持的 sha512/sha256），只有它给不出可用摘要时才回退到老 packument 的 `dist.shasum`（sha1），两条都没有则拒绝安装（fail-closed）；校验失败即删除并拒绝安装。
  - 已知缺口（`code-review-2026-09-27.md` M7–M10）：`DSH_NPM_REGISTRY` 接受 `http://`（`registry.rs::resolve` 不解析 URL）；`dist.tarball` 无 host 校验，摘要可由明文 registry 对手伪造；git 来源插件不校验摘要却会执行归档内 `prepare` 脚本。

## 开发规则

- 搜索文本或文件时优先使用 `rg`；仅在不可用时再使用 `grep` 等替代命令。
- **测试永远不许碰用户的真实数据目录**。凡是按 `paths::*` 解析路径的测试，必须先持住 `crate::tests::scoped_xlink_home(&临时目录)`（或模块自带的 `TestHome` / `TempXlink`）把这个 guard 活到用例结束——裸 `std::env::set_var` 拦不住别的测试正持着同一把 env 锁，而 env 一漏出去，写的就是用户真实的 `~/.dsh-xlink`。2026-09-29 实测：中央库搬迁的初版测试直接拿 `xlink_home()` 当夹具，结尾还 `remove_dir_all(xlink_home())`——**一次 `cargo test` 就能把用户全部内核、实例与会话删干净**，而 `check:invariants` 的 14 项一项都拦不住它。清理只清自己那块临时目录，永远不要 `remove_dir_all(<解析出来的家目录>)`。

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

**两个壳之间只共享只读数据，可变状态一律分家**（2026-09-29 逐条整理）：

| 数据 | 位置 | 谁在写 |
| --- | --- | --- |
| Shell 设置 / 日志 | `shell/<mode>/` | 各写各的 |
| 内核安装树 / `active.txt` | `<family>/desktop[-dev]/` | 各写各的 |
| 实例注册表 | `state/instances.json`（release）/ `state/instances-dev.json`（dev） | **各写各的**（`registry_split`） |
| 实例目录 / DSH home / 插件物化 | `kernels/<family>/instances/<id>/` | 按实例 id 天然分开 |
| 插件中央库 | `plugins/dsh/`（release）/ `plugins/dsh-dev/`（dev） | 各写各的（`store_relocate`） |
| 技能中央库 / 活动视图 | `skills/packages/` + `skills/active/` | **两个壳共享——这是有意选的**（见下） |
| 社区插件目录缓存 | `<family>/desktop[-dev]/plugins-catalog.json` | 各写各的 |

注册表**按壳模式分文件**（`paths::instances_registry_file_for`）。此前共用一个 `state/instances.json`，而互斥只到进程级 Mutex：两个进程读-改-写同一个文件没有任何序列化，dev 壳删一个实例就会改到 release 壳的列表。`registry_split::ensure_scoped` 在 setup 期一次性拆分（认领 / 让位 / 顺序无关 / 幂等，规则见该模块文档），分完之后原先「只有 release 能写共享指针」那条防御随之撤销——谁写都只写自己那份文件。**代价与目的都是同一个**：顶部页签不再列出另一个壳的实例，别再把两个列表合回去。族解析（`instance::default_family` → `kernel::data_dir`）**一级都不读注册表指针**，只走本壳自己的状态（本壳当前实例 → 本壳默认实例 → `dsh`）——`data_dir` 装的是内核安装树，让它取决于「另一个壳上次写了什么」就是跨壳竞态（`check:invariants` 第 12 项禁止生产代码读这个字段）。

技能中央库与活动视图**继续共享**（2026-09-29 维护者决定）：它是「可读的源码 + 启用清单」，dev 侧装一个新技能只会让 release 的技能列表多一项可见内容，**不影响工作台（webui）的会话 / 模型 / 插件**。这与插件中央库共享造成的后果是两类问题，别混为一谈：那个是 dev 改**源码** → 被 `link` 物化进 release 正在跑的实例 → 内核当场抛 `scope '…' rendered without an installed adapter` 白屏；这个只是清单里多一行。再讨论隔离时按这条区分。

**壳的数据目录分家了，实例也必须分家**：dev 壳的默认实例是 `default-dev`，release 是 `default`（`instance::default_instance_id_for`）。内核安装树按壳模式彻底分开（`data_dir` = `<xlink_home>/<family>/desktop[-dev]/`，端口 3090 / 3091），`instance::resolve_default()` 按 `current_mode()` 写死实例 id、**不读**共享注册表，因此**两个壳的内核安装 / 卸载 / 切换在代码与路径上都互不干涉**——`desktop/kernels/` 与 `desktop-dev/kernels/` 是两棵物理上不相交的树。**插件中央库也在这条隔离里**：`paths::plugins_store_root` 解析到 `<xlink_home>/plugins/dsh/`（release）/ `plugins/dsh-dev/`（dev），内层目录名就是内核族（`instance::KERNEL_FAMILY_DSH`），**两个壳各持一份源码**。这条隔离是 2026-09-29 补上的，补之前中央库是 `<xlink_home>/dsh-plugins/` 一份共享目录：中央库里的插件源码被 `link` 物化进某个实例时，dev 侧的改动会直接送到**release 正在跑的内核**上，造成工作台白屏（实测 `scope '…' rendered without an installed adapter`）。存量数据由 `store_relocate::ensure_ready` 一次性接住：**release 整体搬**（`dsh-plugins/` → `plugins/dsh/`，跨卷退回复制后删源），**dev 复制一份种子**（优先从 `plugins/dsh/` 取，release 还没搬时回退 `dsh-plugins/`）——搬完之后原目录不复存在，而两个壳可能在同一台机器上先后启动，dev 需要的是自己那份副本，不该去动 release 的。三条自律：目标已存在就立刻返回（**永不覆盖**）、失败只留 stderr 不阻塞启动、**没有旧目录时什么也不做**（`store_dir` 每次解析中央库都被调，凭空造空中央库会让「装过插件」的判据失真）。所有生产 caller 走 `instance::resolve_default()` 或 `plugins::default_instance_key()`，**不要 hard-code `DEFAULT_INSTANCE_ID`**；注册表的 `default_instance_id` 现在**每壳各有一份**（分文件），`ensure_default_registered` 按本壳默认值认领（指错了就改回来），壳自己「切到哪个实例」写 `settings.current_instance_id`（`instance::set_current_instance_id`），**任何模块都不得把它指向具体 id，也不得读它做决策**——清空成 `None` 与按本壳默认值认领是修复，不算抢；读取（`instance::current_instance_id`）会对本壳注册表做成员校验，指向已让位/已删实例的陈旧值回退本壳默认（注册表读不出来时保留选择——读失败不等于实例不存在）；`~/.dsh` 历史数据只搬进 release 实例（`legacy_migration_target`）。**「只搬进 release」防的是再犯，防不了已经犯的**：2026-09-28 dev 壳在闸门落地（`713bb40`）前 15 分钟就把 `~/.dsh` 并进了 `default-dev`，release 侧工作台从此是空列表而 `~/.dsh` 已空、搬不动第二次——`home_recovery` 补的就是这条回收路径（只读扫描别的实例 home 里本实例缺的 `sessions/` 与 `attachments/`，用户拍板后**复制**过来，源永不删除、目标已有条目永不覆盖），UI 挂在「数据迁移」面板的「找回历史会话」卡片上（`scan_misplaced_home` / `recover_misplaced_home`，执行前要求**目标实例**的内核未在运行——判据按实例 pid 文件而非本壳工作台，用户自建实例留在两份注册表里，另一壳正跑着它同样要拒）。`instance::ensure_instance_mutable` 仍保留，但作用已收窄为「两个壳被指到**同一个**实例、且那个内核还活着」时拒绝插件与内核版本变更——默认路径下两个壳永远落在不同实例上，它是一道兜底而不是日常必经的门。这两条纪律由 `check:invariants` 第 10 / 11 项机械兜底：① 生产代码里 `InstanceRecord::new` 的 id 实参不得是常量；② 除 `instance.rs` 外不得写 `default_instance_id = <具体 id>`。

## 实现约定

- 用户可见文案用简体中文；错误信息必须包含可操作的下一步与相关日志路径。
- 概览页的状态机只有一个**主按钮**「工作台」：文案恒为名词，启停方向由 icon（▶ 启动 / ⏸ 停止）与 hover title 表达；同排的「官方对话」用同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」（运行后从下方淡入）与「查看日志」是并列的次级入口（都不改变内核状态），不要再往主按钮旁边加会启停内核的动作。
- 长任务失败时进度面板保持开放，由用户手动关闭；完整原始输出始终落盘，报错信息引用日志路径。
- 所有 GUI 子进程使用 `process.rs` 的 PATH 合并、静默窗口和进程组回收策略；涉及进程、网络或目录树的 Tauri 命令必须异步执行并使用 `spawn_blocking`。
- **代码预算门禁已改成反棘轮，不要绕开它**。`npm run check-code-budget` 现在从 git 读出本文件**已提交版本**的数字当基线（HEAD 里的旧值仍然是旧值，所以「同一个提交里改数字」这个流程不会让检查失守），并强制三条规则：① 基线预算 ≥ `RATCHET_THRESHOLD` 的**大文件只许下调**（plugins.rs / theme.css / commands.rs 只能越来越小）；② **新文件**必须显式登记进 `FILE_BUDGETS` 并写清为什么该独立，且受 `HARD_FILE_CEILING`（800 行）硬顶；③ 总量 `TOTAL_BUDGET` 是一道**软上限**（刻意不做"只许下调"——试过，实践中只会逼人绕过门禁而不是真写出更少的代码）。被规则 ① 拦下时的正确反应是**把新逻辑拆出去**，不是调数字。
- **跨模块重复先提共享层**：包取源逻辑放 `pkg.rs`（插件与技能共用，只返回纯文本原因、错误分类由调用方决定），JSON 状态文档读写放 `state.rs`（容错读 / 校验读分开，文案由 `StateCtx` 提供），前端异步样板放 `ui/src/async.js`（`singleFlight` / `createStatusSource` / `createUpdateChecker`）。安装预检拆成两层：`sandbox.rs` 只管"起一个临时内核、探它、收摊"（与装什么无关，技能预检直接复用），`precheck.rs` 管两段式事务（快照 → 装进沙盒 → 探测 → 提交/回滚），`plugins.rs` 侧只暴露 `store_file` 一条可见性缝——**预检的事务主体不要写进 `plugins.rs`**，它已经 2964 行。`npm run check:code-budget` 按文件代码行数与重复区间数拦膨胀：要上调预算，必须在同一个提交里改 `scripts/check-code-budget.mjs` 的数字。
- **依赖钉版可以降级，但不得跨版本线，也不得静默**。内核 monorepo 锁步发布的前提是每个子包都发了，漏发时 pnpm 会在解析阶段拒绝整棵依赖树（2026-09-29 实测 `0.2.0-rc.2` 缺 `dsh-client-ui-settings-account@0.2.0-rc.2`，**官方与镜像 registry 都没有**，换 registry 救不了）。壳的处理在 `kernel_deps.rs`：失败后解析断边，查当前 registry，给每条断边选**同一 `major.minor` 版本线内、语义化更低**的已发布版本钉进 `overrides` 再重试。三条纪律：① **只退到更低、只退到同一条版本线、只退到同一稳定性层级**——更高的版本与内核其余子包的配套关系未经测试，跨 minor 的差异可能已经是不兼容，壳无权替用户决定，那种情况宁可不装并如实说「上游发布不完整，换个内核版本」；**跨稳定性层级同理**：语义化里 `0.2.0-rc.2 < 0.2.0` 成立，所以只判「严格更低」会把正式版的需求悄悄换成 RC，而提示文案只说「已改钉到同一版本线上已发布的较低版本」，用户看不出装出来的是个 RC——`is_pre_release` 那道判断不能省；② **降级必须说出口**，每条进进度面板并汇总进安装成功提示，让用户知道这份内核不是原样的、差在哪；③ **降级钉版要活过下一轮**——锁步重装写 overrides 时必须与它合并，覆盖写会把包钉回那个不存在的版本，pnpm 立刻二次失败。预检放在失败之后而非安装之前，正常安装不多付网络往返。
- **用户看得见后果的后台动作必须落盘，不能只 `eprintln!`**。壳是 GUI 应用，Windows 上 `eprintln!` 没有任何去处（没有控制台可接，dev 模式从终端起才看得到），而**恰恰是那些只有后果、没有原因的动作用了它**：工作台窗口自动重载、给 pnpm 降优先级、自愈与降级。2026-09-29 的代价：dev 壳装内核的 8 秒里 release 壳的工作台被重载并撞上内核的启动顺序竞态，壳这边**一行日志都留不下**，用户与维护者只能靠 `last-incident.json` 的时间戳对猜。凡是用户看得见现象的动作都走 `shell_events::record(<逻辑名>, <行>)`——它按日轮转落进 `shell_logs_dir`，因此**自动出现在「查看日志」面板**。写失败只落 stderr，绝不阻断调用方。**新增后台动作时先问一句「出事后怎么查」**，查不到就说明这个设计还没完成。
- **装包任务不许抢另一个壳的资源**。两棵内核安装树物理不相交（`dsh/desktop/` vs `dsh/desktop-dev/`），但**装内核会硬链接数万文件并跑 node-gyp 编译**，同一台机器上另一个壳的工作台（WebView2 渲染进程）可能因此被打崩重启，表现为页面莫名 reload 并撞上内核的启动顺序竞态。2026-09-29 实测过一次。壳的对策是给 pnpm / npm 降 `BELOW_NORMAL` 优先级（`child_priority.rs`）而不是禁止装内核——后者会让用户没法一边用 release 一边在 dev 调试新内核。**只降装包工具**：`smoke_load_native_modules` 的 Node 探针保持正常优先级，它降了会误报「原生模块加载失败」，那是比原问题更糟的假阴性。内核自身的进程不走 `run_with_progress`，不受影响。
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
