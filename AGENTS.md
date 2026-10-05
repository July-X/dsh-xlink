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
npm run check                     # 提交前的全量门禁：invariants + 预算 + 两套测试 + fmt/clippy/release 编译 + UI 产物预算
cargo check                       # 在 src-tauri/ 内：快速编译检查
cargo clippy --all-targets        # lint，零警告基线
cargo fmt                         # rustfmt 格式化
```

`npm run check` 是**提交前唯一该跑的那条**，别再手工拼下面这些：2026-10-01 那个 release-only 的编译错误（`#[cfg]` 归属被 `use` 打断，debug 全绿、release 报常量重定义）就是「只跑 `cargo check` + 只跑 `test:ui`」漏过去的，而那正是两个发布通道都编不出来的性质。`check:rust` 里的 `cargo check --release` 不能省——debug 与 release 的 cfg 覆盖面不同，只有 release 会现形。全量约 1 分钟（release 编译冷缓存时更久）。

**门禁判据不许在 workflow 里内联**。UI 产物预算此前是同一段 heredoc 在 `desktop-ci.yml` 与 `desktop-release.yml` 各写一份，已经漂移过一次（`desktop-ci.yml` 那份多一段来历注释、release 那份没有），`check:code-budget` 还整条缺席在发布通道上（`docs/code-review-2026-09-27.md` M12 预言的正是这件事）。现在判据本体在 `scripts/check-ui-bundle-budget.mjs`，两个 workflow 都只调它；`npm run check` 也把它带上了——它此前只在 CI 跑，本地提交前那条总闸管不到，预算超标要等 CI 才发现。**产物不存在时脚本必须 exit 1 并说清前置命令**，静默跳过等于给出一道「通过」的假门禁。

UI 是 Vue 3 + Element Plus 单页应用（源码 `ui/src/`，Vite 构建到 `ui/dist/`，即 `src-tauri/tauri.conf.json` 的 `frontendDist`）。状态与动作集中在 `ui/src/store.js` / `plugins.js` / `skills.js` / `progress.js` / `logs.js`，异步样板（在途去重、静默刷新、更新检查策略）在 `async.js`，组件只读状态、调动作；与 Rust 的通信只允许走 `ui/src/bridge.js` 的 invoke/Channel。触发 IO 的按钮必须挂 loading（`loading.js` 的 `withLoading(key, …)` + `:loading="isLoading(key)"`）；长任务走 `progress.js` 的 `withProgress`。改完 UI 跑 `npm run build:ui`；Rust 改动至少跑 `cargo check`，提交前跑 `npm run check`（它已经含 clippy 与 fmt，别再手工拼一遍）。

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

技能中央库与活动视图**继续共享**（2026-09-29 维护者决定）：它是「可读的源码 + 启用清单」，dev 侧装一个新技能只会让 release 的技能列表多一项可见内容，**不影响工作台（webui）的会话 / 模型 / 插件**。这与插件中央库共享造成的后果是两类问题，别混为一谈：那个是 dev 改**源码** → 被 `link` 物化进 release 正在跑的实例 → 内核当场抛 `scope '…' rendered without an installed adapter` 白屏；这个只是清单里多一行。再讨论隔离时按这条区分。**共享的是「活动视图」这一份数据，接线文件按实例写**：`kernel_adapter::ensure_skill_wiring` 在每次 `prepare_instance`（即每次启动工作台）时向该实例的 `$DSH_HOME/cordis.patch.yml` 追加一条 `xlink-skill-filesystem` loader 行。2026-09-30 查清并修好的那件事：`DSH_CUSTOM_SKILL_DIRS` 这条 env **当前内核不读**（`dsh-skill-filesystem` 的 `customSkillDirs` 只从 `cordis.patch.yml` 的插件配置读，0.2.0-rc.2 全树 3481 个 js/d.ts 无 `CUSTOM_SKILL` 命中），所以「装得上、内核看不见」曾持续半年而无人察觉——**判断「内核到底认不认某个注入方式」必须实测，不能读接口名推断**，已用 `--dump-config` + `skills/list` RPC 端到端验证。写接线时只追加不改写（patch 是后写覆盖先写）、顶层不是列表就不碰（patch 文件是内核 fail-loud 的输入，历史上一次坏模板就让内核启动即崩）、写失败只落 `shell_events` 不阻断启动。

**壳的数据目录分家了，实例也必须分家**：dev 壳的默认实例是 `default-dev`，release 是 `default`（`instance::default_instance_id_for`）。内核安装树按壳模式彻底分开（`data_dir` = `<xlink_home>/<family>/desktop[-dev]/`，端口 3090 / 3091），`instance::resolve_default()` 按 `current_mode()` 写死实例 id、**不读**共享注册表，因此**两个壳的内核安装 / 卸载 / 切换在代码与路径上都互不干涉**——`desktop/kernels/` 与 `desktop-dev/kernels/` 是两棵物理上不相交的树。**插件中央库也在这条隔离里**：`paths::plugins_store_root` 解析到 `<xlink_home>/plugins/dsh/`（release）/ `plugins/dsh-dev/`（dev），内层目录名就是内核族（`instance::KERNEL_FAMILY_DSH`），**两个壳各持一份源码**。这条隔离是 2026-09-29 补上的，补之前中央库是 `<xlink_home>/dsh-plugins/` 一份共享目录：中央库里的插件源码被 `link` 物化进某个实例时，dev 侧的改动会直接送到**release 正在跑的内核**上，造成工作台白屏（实测 `scope '…' rendered without an installed adapter`）。存量数据由 `store_relocate::ensure_ready` 一次性接住：**release 整体搬**（`dsh-plugins/` → `plugins/dsh/`，跨卷退回复制后删源），**dev 复制一份种子**（优先从 `plugins/dsh/` 取，release 还没搬时回退 `dsh-plugins/`）——搬完之后原目录不复存在，而两个壳可能在同一台机器上先后启动，dev 需要的是自己那份副本，不该去动 release 的。三条自律：目标已存在就立刻返回（**永不覆盖**）、失败只留 stderr 不阻塞启动、**没有旧目录时什么也不做**（`store_dir` 每次解析中央库都被调，凭空造空中央库会让「装过插件」的判据失真）。所有生产 caller 走 `instance::resolve_default()` 或 `plugins::default_instance_key()`，**不要 hard-code `DEFAULT_INSTANCE_ID`**；注册表的 `default_instance_id` 现在**每壳各有一份**（分文件），`ensure_default_registered` 按本壳默认值认领（指错了就改回来），壳自己「切到哪个实例」写 `settings.current_instance_id`（`instance::set_current_instance_id`），**任何模块都不得把它指向具体 id，也不得读它做决策**——清空成 `None` 与按本壳默认值认领是修复，不算抢；读取（`instance::current_instance_id`）会对本壳注册表做成员校验，指向已让位/已删实例的陈旧值回退本壳默认（注册表读不出来时保留选择——读失败不等于实例不存在）；`~/.dsh` 历史数据只搬进 release 实例（`legacy_migration_target`）。**「只搬进 release」防的是再犯，防不了已经犯的**：2026-09-28 dev 壳在闸门落地（`713bb40`）前 15 分钟就把 `~/.dsh` 并进了 `default-dev`，release 侧工作台从此是空列表而 `~/.dsh` 已空、搬不动第二次——`home_recovery` 补的就是这条回收路径（只读扫描别的实例 home 里本实例缺的 `sessions/` 与 `attachments/`，用户拍板后**复制**过来，源永不删除、目标已有条目永不覆盖），UI 挂在「数据迁移」面板的「找回历史会话」卡片上（`scan_misplaced_home` / `recover_misplaced_home`，执行前要求**目标实例**的内核未在运行——判据按实例 pid 文件而非本壳工作台，用户自建实例留在两份注册表里，另一壳正跑着它同样要拒）。`instance::ensure_instance_mutable` 仍保留，但作用已收窄为「两个壳被指到**同一个**实例、且那个内核还活着」时拒绝插件与内核版本变更——默认路径下两个壳永远落在不同实例上，它是一道兜底而不是日常必经的门。这两条纪律由 `check:invariants` 第 10 / 11 项机械兜底：① 生产代码里 `InstanceRecord::new` 的 id 实参不得是常量；② 除 `instance.rs` 外不得写 `default_instance_id = <具体 id>`。

## 实现约定

> 实现约定已按前 / 后端拆开。改哪一侧就读哪一份：
>
> | 你要改的 | 必读 |
> | --- | --- |
> | `ui/**`（面板组件、store、样式） | [ui/AGENTS.md](ui/AGENTS.md) |
> | `src-tauri/**`（Rust 进程、注入脚本） | [src-tauri/AGENTS.md](src-tauri/AGENTS.md) |
> | 两边都改，或要新增一条跨前后端的规则 | 两份都读，并在本文件登记它属于哪一侧 |
>
> 下面这五条全仓共用，只留一份。复制到子文件里，两边就会各改各的。

### 通用约定

- 长任务失败时进度面板保持开放，由用户手动关闭；完整原始输出始终落盘，报错信息引用日志路径。
- **代码预算门禁已改成反棘轮，不要绕开它**。`npm run check-code-budget` 现在从 git 读出本文件**已提交版本**的数字当基线（HEAD 里的旧值仍然是旧值，所以「同一个提交里改数字」这个流程不会让检查失守），并强制三条规则：① 基线预算 ≥ `RATCHET_THRESHOLD` 的**大文件只许下调**（plugins.rs / theme.css / commands.rs 只能越来越小）；② **新文件**必须显式登记进 `FILE_BUDGETS` 并写清为什么该独立，且受 `HARD_FILE_CEILING`（800 行）硬顶；③ 总量 `TOTAL_BUDGET` 是一道**软上限**（刻意不做"只许下调"——试过，实践中只会逼人绕过门禁而不是真写出更少的代码）。被规则 ① 拦下时的正确反应是**把新逻辑拆出去**，不是调数字。
- **跨模块重复先提共享层**：包取源逻辑放 `pkg.rs`（插件与技能共用，只返回纯文本原因、错误分类由调用方决定），JSON 状态文档读写放 `state.rs`（容错读 / 校验读分开，文案由 `StateCtx` 提供），前端异步样板放 `ui/src/async.js`（`singleFlight` / `createStatusSource` / `createUpdateChecker`）。安装预检拆成两层：`sandbox.rs` 只管"起一个临时内核、探它、收摊"（与装什么无关，技能预检直接复用），`precheck.rs` 管两段式事务（快照 → 装进沙盒 → 探测 → 提交/回滚），`plugins.rs` 侧只暴露 `store_file` 一条可见性缝——**预检的事务主体不要写进 `plugins.rs`**，它已经 2964 行。`npm run check:code-budget` 按文件代码行数与重复区间数拦膨胀：要上调预算，必须在同一个提交里改 `scripts/check-code-budget.mjs` 的数字。
- **反向验之前先确认「改动真的进了被测的那个产物」**。给机械检查或单测做反向验证（同义：故意弄坏 → 确认它会响）时，PowerShell 的 `Copy-Item` **保留源文件的 `LastWriteTime`**，于是「备份 → 改坏 → 跑测试（红）→ 恢复」这四步里，恢复那一步写回去的是**比改坏那一步更早的 mtime**，cargo / vite 判定无需重编译，第二次跑的还是改坏时的产物——结论会变成「恢复后仍然红」，而真相是**根本没重编**。2026-09-30 实测踩中：恢复后 `kernel::tests` 一直红，强制重编后全绿。恢复后补一句 `(Get-Item <file>).LastWriteTime = Get-Date` 再跑。同理，**反向验的红必须是「改坏的当次」的红**——中途任何一次重编失败都可能让红变成「编不过」而不是「断言不成立」，那不算验过。
- 图标只从 `assets/whale-icon.svg`、`assets/whale-icon-small.svg` 与托盘专用的 `assets/whale-head.svg` 生成，规则见 [docs/icon-design.md](docs/icon-design.md)。**面板里的第三方标志（npm 等）另有一条规则**：必须是 `ui/public/` 下的本地矢量、不许写远端 URL，并保留来源与许可声明——`tauri.conf.json` 的 `csp` 是 `null`，远端 `<img>` 出不出网完全取决于用户那台机器，取不到时页面上只剩一块白砖且没有任何报错（版本面板过去就挂着 `avatars.githubusercontent.com` 的 npm 头像）；由 `check:invariants` 第 15 项钉住。
- **常驻行为只有一份实现，在 `shell/resident.rs`**（2026-10-02 起两平台统一）。关闭窗口一律只是收进后台（Windows 托盘 / macOS 菜单栏），真正退出只从图标菜单发起。这条纪律的代价是两处：`CloseRequested` 统一走 `resident::intercept_close`（不要按平台分叉回去），恢复动作统一走 `resident::show_main_shell`（`lib::show_main_shell` 与工作台拉绳的 `focus_main_shell` 都调它，**抄一份就会漂**——Windows 上两边互调过一次，直接 `thread 'main' has overflowed its stack`）。**收起同理只有 `resident::hide_to_shell` 一份**：2026-10-05 登录自启分支裸调 `window.hide()`，macOS 上激活等级没降到 Accessory，进程带着「在运行」小点的 Dock 图标、零可见窗口地挂在后台，点它还没反应（`RunEvent::Reopen` 只抬工作台，而 macOS 的激活不会替我们显示 `hide()` 掉的窗口——抬不到时必须兜底 `show_main_shell`），由 `check:invariants` 第 17 项钉住。收起那一刻窗口已隐藏、页内提示谁也看不见，所以**收起时不广播事件**，改由 Rust 在从后台恢复时补发 `shell-restored-from-background`（每次启动一次）；改事件名要同时动 `App.vue` 与本文件，跨前后端。
- **登录自启归系统，偏好归壳，两者分开**。`shell/autostart.rs` 只写系统登录项（macOS LaunchAgent plist / Windows `HKCU\...\Run`），「登录时启动工作台」是 `Settings::autostart_kernel` 里的一个 bool。**写 / 读 / 删会碰到用户真实的登录项，测试一律不许动它**——`autostart.rs` 的测试只覆盖纯逻辑（命令串拼装、`detect_autostart_arg` 只认独立 token、路径归一化、启动判据的三个条件），写路径只能人工在装好的机器上验证。

## 发布

版本发布由 `.github/workflows/desktop-release.yml` 负责。发布前必须确认 **`package.json`、`src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 三处**的 `version` 完全一致，并且版本提交已经推送到 `main`。workflow 使用 `TAURI_SIGNING_PRIVATE_KEY` 给更新制品签名，`releaseDraft` 与 `prerelease` 必须保持为 `false`，以保证 updater 的 latest endpoint 可用。

**第三处 `Cargo.toml` 是 2026-10-03 发 v0.3.9-rc.1 时被 `check:invariants` 的 `[versions]` 项当场抓出来的**——此前这一节只写了「两处一致」，而门禁比文档多查一处。漏掉它的后果不是编译失败：crate 版本与壳版本不同源，Tauri 打的产物里元数据会带着一个对不上的壳版本号，updater 与「关于」面板显示的都可能不是这个 release。**改版本号一律三个文件一起改**，别等门禁报。

**默认只提交，不推送**（2026-09-30 起）。改完代码跑完门禁就 `git commit`，`git push` / `git push origin <tag>` / 建远程 tag 一律**先问过用户**再做——推远端不可逆，而且这台机器上常有并行会话往同一个工作树提交，推之前必须先看 `git log --oneline origin/main..HEAD`，确认要推的提交里**有没有别人的**（并行会话的改动会跟着一起上去）。想看未推送的有什么就 `git status -sb` 报「领先 N 个提交」，不要自作主张推。

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

