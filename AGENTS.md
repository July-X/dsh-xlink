# AGENTS.md — dsh-xlink

本仓库是 dsh-xlink 桌面应用的独立项目。模块布局与数据流见 [docs/architecture/architecture.md](docs/architecture/architecture.md)，用户文档见 [README.md](README.md)。

> **本文件的预算取 64000 字节**（第 25 项 `agents-md-byte-budget` 钉住）：运行时的
> 指令上限**由内核配置给出**——`kernels/<版本>/node_modules/@deepseek-ai/dsh-base/cordis.patch.yml`
> 里的 `agent-instructions.maxBytes = 65536`（0.2.1-alpha.2 起）。超出后尾部会被截断，
> 运行时会在渲染结果最前面插一行 `Workspace instruction budget 65536 bytes: truncated …`
> ——**可那行没人会去读**，写在这里却从没被读到过的规则比没写更坏。加内容前先
> `wc -c AGENTS.md`；写不下的搬到 [docs/reviews/agent-lessons.md](docs/reviews/agent-lessons.md)。
> **内核升版后重新读那份 `maxBytes`，别照抄这里的数**（32768 → 65536 变过一次）。

## 范围

- **独立项目**：仓库根目录就是桌面交付物，不加入任何上级 pnpm workspace，也不依赖源仓库的构建、测试或发布门禁。根目录 `pnpm-workspace.yaml` 让 pnpm 将本项目作为独立根目录处理，直接运行 `pnpm install` 即可。
- **运行时内核边界**：项目不携带或重新发布 dsh 内核代码。内核由用户从 npm registry 安装；桌面壳通过 `src-tauri/` Rust 进程和 `ui/` 管理面板管理其生命周期、配置和窗口行为。
- **绝不往已安装的内核目录里写任何东西**。npm 下载下来是什么，用户拿到的就是什么——原则上 dsh-xlink 只是替用户输入了一次 `dsh web`。**唯一允许的写入**是 `install_version` 在装新版本时写的 pnpm stub（`package.json` / `pnpm-workspace.yaml`）与 pnpm 自己产生的 `node_modules`；`uninstall` 的整目录删除是用户显式要求的操作。**除此之外，`kernels/<版本>/node_modules/**` 里的一切都只读**。
  - stub 里那两处 `overrides`（依赖钉版）也在允许范围内——它决定的是**装哪个版本**，不是改内核代码。落点统一在 `kernel_deps.rs`，`kernel.rs` 不再自己写 stub。
  - **「可逆 + 有备份 + 有哈希校验」论证的不是同一个问题**：那套论证回答的是「破坏可不可逆」，而「该不该写」在本条已经答完了。经过见 [agent-lessons §1](docs/reviews/agent-lessons.md)。
  - 内核侧的缺陷（启动顺序竞态、监视根过宽、渲染器 fail-loud 检查）**由内核仓库负责**。壳要做的是三件事，都不碰内核：减少触发（工作台运行期间禁止装/删内核）、出了事能自愈（页面内一次性自愈）、出了事说得清（`cause` 归因与文案）。
  - **补丁机制已于 2026-10-11 整体删除**。唯一内置的 `dsh-file-perf` 自 `0.1.2-alpha.2` 起被官方内核采纳，适用窗口只剩 npm 上的 `0.1.1-rc.1`/`rc.2`，而 UI 入口更早就没了。`plugins/patches.rs` 只留 `applied_patch_ids` 一条**只读缝**：打过分的老用户磁盘上那份 `state.json` 不会因为我们删代码而消失，环境指纹要把它算进去，否则那批用户升级后「回退点」的判定基准被静默改动。**新增补丁前先回头看这条规则。**
  - **删掉补丁目录不会让用户机器上消失**（更新只覆盖与新增）。已打过补丁的用户此后**无法在 UI 上撤销**——**这条必须写进发布说明**。经过见 [agent-lessons §2](docs/reviews/agent-lessons.md)。
- **信任边界**：`@deepseek-ai` 命名空间限制**只覆盖内核与 Node 运行时两条路径，不覆盖插件与技能**；GitHub 来源同样不限制仓库归属。版本列表优先 npm registry，GitHub Releases 仅作回退。
  - **已强制的两处**：内核包名在 `kernel.rs` 硬编码为 `DSH_NPM_PACKAGE`；托管 Node 在 `node_install.rs` 硬编码版本并校验 SHA-256。
  - **未强制的两处**：`plugins.rs` 与 `skills.rs` 的 `parse_spec` 对 npm 包名只做字符类校验（`alphanumeric` 与 `-._@/`），`lodash`、`@attacker/backdoor` 均可安装。改这两处别以为 `pkg.rs` 已拦下——它是插件与技能**共用**的取源层，两条路径都走它，但不检查命名空间。
  - npm 基础 URL 默认指向 **npmmirror 镜像**（`registry.rs::DEFAULT_NPM_REGISTRY`），需要上游 registry 的部署用 `DSH_NPM_REGISTRY` 覆盖。镜像只影响**取源**，不影响信任判定：下载的 tarball 逐字节校验（`releases::verify_download_integrity`）——优先用 `dist.integrity`（SRI，取最强且受支持的 sha512/sha256），给不出可用摘要时才回退 `dist.shasum`（sha1），两条都没有则拒绝安装（fail-closed）；校验失败即删除并拒绝安装。
  - 已知缺口（`code-review-2026-09-27.md` M7–M10）：`DSH_NPM_REGISTRY` 接受 `http://`（`registry.rs::resolve` 不解析 URL）；`dist.tarball` 无 host 校验，摘要可由明文 registry 对手伪造；git 来源插件不校验摘要却会执行归档内 `prepare` 脚本。

## 开发规则

- 搜索文本或文件时优先使用 `rg`；仅在不可用时再使用 `grep` 等替代命令。
- **测试永远不许碰用户的真实数据目录**。凡是按 `paths::*` 解析路径的测试，必须先持住 `crate::tests::scoped_xlink_home(&临时目录)`（或模块自带的 `TestHome` / `TempXlink`）把这个 guard 活到用例结束——裸 `std::env::set_var` 拦不住别的测试正持着同一把 env 锁。清理只清自己那块临时目录，永远不要 `remove_dir_all(<解析出来的家目录>)`：2026-09-29 实测过一版那样的初版测试，**一次 `cargo test` 就能把用户全部内核、实例与会话删干净**，而 `check:invariants` 当时一项都拦不住。

## 命令

```sh
npm run deps                      # 安装依赖（pnpm 优先，缺失回退 npm）
npm run dev                       # tauri dev（自动先起 vite dev server，5174 热更新）
npm run dev 5190                  # 同上但换端口；vite 的 server.port 与 tauri 的 devUrl 由 scripts/dev.mjs 统一（DSH_DEV_PORT）
npm run dev:ui                    # 只起管理面板 dev server（纯前端迭代，浏览器里无 Tauri 桥）
npm run build                     # 本机构建（.dmg / NSIS；自动先 vite build → ui/dist）
npm run build:ui                  # 只构建管理面板 → ui/dist
npm run check                     # 全量门禁：invariants + 重复代码检测 + 前端测试 + fmt/clippy/release 编译 + Rust 单测 + UI 产物预算——只在版本发布前整跑一次
cargo check                       # 在 src-tauri/ 内：快速编译检查
cargo clippy --all-targets        # lint，零警告基线
cargo fmt                         # rustfmt 格式化
```

`npm run check` 是全量门禁，**只在版本发布前整跑一次**（2026-10-17 用户拍板，取代「提交前必跑」：agent 在迭代与提交点反复整跑全量纯烧等待，实测一个 UI 任务 2 小时里 72% 的墙钟花在等待上）。日常开发与提交只跑命中的那一个面（改哪份跑哪份测试 / `check:duplicates` / `cargo check` / `build:ui`），提了什么就验那一块；提交后的兜底本来就在 CI——desktop-ci 的 quality job 一直整跑 invariants / 重复代码检测 / 前端测试 / fmt / clippy / cargo test / build:ui / UI 产物预算，发布通道再整编译 release，所以「只跑 `cargo check` + 只跑 `test:ui`」漏出去的 2026-10-01 那个 release-only 编译错误（`#[cfg]` 归属被 `use` 打断，debug 全绿、release 报常量重定义）到 CI 仍会被拦住。本地的门禁纪律附着于发布：发版那个 commit 上必须见到一次全绿，别手工重拼判据，那是上面判据内联禁令的事。`check:rust` 的组成不变（含 `cargo test` 与 `cargo check --release`）：debug 与 release 的 cfg 覆盖面不同，只有 release 会现形。

`check:rust` 同样**包含 `cargo test`**（2026-10-11 补入）：此前不含 851 条 Rust 单测——总闸跑绿而 CI 才红，正是「门禁不许假绿」要挡的那件事。热缓存 44 秒 → 约 100 秒。

**门禁判据不许在 workflow 里内联**。UI 产物预算此前是同一段 heredoc 在 `desktop-ci.yml` 与 `desktop-release.yml` 各写一份，已经漂移过一次。现在判据本体在 `scripts/check-ui-bundle-budget.mjs`，两个 workflow 只调它，`npm run check` 也带上了（此前只在 CI 跑，本地提交前管不到）。**产物不存在时脚本必须 exit 1 并说清前置命令**，静默跳过等于给出一道「通过」的假门禁。

UI 是 Vue 3 + Element Plus 单页应用（源码 `ui/src/`，Vite 构建到 `ui/dist/`，即 `src-tauri/tauri.conf.json` 的 `frontendDist`）。状态与动作集中在 `ui/src/store.js` 与各功能目录下的 store（`plugins/plugins.js`、`skills/skills.js`、`logs/logs.js`、`usage/usage.js`），异步样板（在途去重、静默刷新、更新检查策略）在 `shell/async.js`，组件只读状态、调动作；与 Rust 的通信只允许走 `shell/bridge.js` 的 invoke/Channel。按钮 loading 与长任务的细则、以及「改完跑 `build:ui`」都在 [ui/AGENTS.md](ui/AGENTS.md)，这里不重复。

## 数据目录

`kernel::data_dir` 按**内核族命名空间**解析：`<xlink_home>/<family>/desktop/`（release）或 `<xlink_home>/<family>/desktop-dev/`（debug），family 取实例注册表默认实例的内核族（`instance::default_family()`）；v0.2.x 的平铺目录 `<xlink_home>/desktop[-dev]/` 会在启动时自动整体搬入族目录，搬迁失败继续用旧目录。`lib.rs` 的 `setup()` 必须通过它取目录。**解析链只有两级，没有 app-data 兜底**（2026-10-08 用户拍板撤掉）：此前的兜底在 macOS 落到 `~/Library/Application Support/<id>`、Windows 落到 `%APPDATA%\<id>`——既不在 `~` 下面，也不随 `DSH_XLINK_HOME` 走，于是「数据目录恒在 `~/.dsh-xlink`」在 Windows 上根本不成立，UI 显示的路径与实际写入处也会分家。现在建不出来就打印失败原因、**仍按 `<xlink_home>` 下的族目录继续**（此后每次写入各自报错，错误指着同一条路径，比静默换到用户没预期的目录好定位）。`check-invariants` 第 24 项 `data-dir-platform-branch` 扫生产 Rust 钉住这条——它不会自己回来的理由是「跨平台一致性」，编译器看不见。debug 端口 3091，release 端口 3090（`kernel::DEFAULT_PORT`）；用户保存过的 port 优先于 `Settings::default()`。优先级与目录隔离原因见 [docs/architecture/architecture.md](docs/architecture/architecture.md)。

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

技能中央库与活动视图**继续共享**（2026-09-29 维护者决定）：它是「可读的源码 + 启用清单」，dev 侧装一个新技能只会让 release 的技能列表多一项可见内容，**不影响工作台（webui）的会话 / 模型 / 插件**。这与插件中央库共享造成的后果是两类问题，别混为一谈：那个是 dev 改**源码** → 被 `link` 物化进 release 正在跑的实例 → 内核当场白屏；这个只是清单里多一行。再讨论隔离时按这条区分。**共享的是「活动视图」这一份数据，接线文件按实例写**：`kernel_adapter::ensure_skill_wiring` 在每次 `prepare_instance`（即每次启动工作台）时向该实例的 `$DSH_HOME/cordis.patch.yml` 追加一条 `xlink-skill-filesystem` loader 行。`DSH_CUSTOM_SKILL_DIRS` 这条 env **当前内核不读**（`dsh-skill-filesystem` 的 `customSkillDirs` 只从 `cordis.patch.yml` 的插件配置读），所以「装得上、内核看不见」曾持续半年而无人察觉——**判断「内核到底认不认某个注入方式」必须实测，不能读接口名推断**，已用 `--dump-config` + `skills/list` RPC 端到端验证，见 [agent-lessons §8](docs/reviews/agent-lessons.md)。写接线时只追加不改写（patch 是后写覆盖先写）、顶层不是列表就不碰（patch 文件是内核 fail-loud 的输入，历史上一次坏模板就让内核启动即崩）、写失败只落 `shell_events` 不阻断启动。

**壳的数据目录分家了，实例也必须分家**：dev 壳的默认实例是 `default-dev`，release 是 `default`（`instance::default_instance_id_for`）。内核安装树按壳模式彻底分开（`data_dir` = `<xlink_home>/<family>/desktop[-dev]/`，端口 3090 / 3091），`instance::resolve_default()` 按 `current_mode()` 写死实例 id、**不读**共享注册表，因此**两个壳的内核安装 / 卸载 / 切换在代码与路径上都互不干涉**——`desktop/kernels/` 与 `desktop-dev/kernels/` 是两棵物理上不相交的树。**插件中央库也在这条隔离里**：`paths::plugins_store_root` 解析到 `<xlink_home>/plugins/dsh/`（release）/ `plugins/dsh-dev/`（dev），内层目录名即内核族，**两个壳各持一份源码**。这条隔离是 2026-09-29 补上的——补之前中央库是一份共享目录，dev 侧的改动会被 `link` 物化进 release 正在跑的内核，造成工作台白屏。存量数据由 `store_relocate::ensure_ready` 一次性接住：release 整体搬、dev 复制一份种子。三条自律：目标已存在就立刻返回（**永不覆盖**）、失败只留 stderr 不阻塞启动、**没有旧目录时什么也不做**（`store_dir` 每次解析中央库都被调，凭空造空中央库会让「装过插件」失真）。所有生产 caller 走 `instance::resolve_default()` 或 `plugins::default_instance_key()`，**不要 hard-code `DEFAULT_INSTANCE_ID`**；注册表的 `default_instance_id` **每壳各有一份**，`ensure_default_registered` 按本壳默认值认领（指错了就改回来），壳自己「切到哪个实例」写 `settings.current_instance_id`，**任何模块都不得把它指向具体 id，也不得读它做决策**（清空成 `None` 与按本壳默认值认领是修复，不算抢）；读取会对本壳注册表做成员校验，指向已让位/已删实例的陈旧值回退本壳默认（注册表读不出来时保留选择——读失败不等于实例不存在）；`~/.dsh` 历史数据只搬进 release 实例（`legacy_migration_target`）。**「只搬进 release」防的是再犯，防不了已经犯的**：2026-09-28 dev 壳在闸门落地前 15 分钟就把 `~/.dsh` 并进了 `default-dev`，release 侧工作台从此是空列表而 `~/.dsh` 已空、搬不动第二次——`home_recovery` 补的就是这条回收路径（只读扫描别的实例 home 里本实例缺的 `sessions/` 与 `attachments/`，用户拍板后**复制**过来，源永不删除、目标已有条目永不覆盖），UI 挂在「数据迁移」面板的「找回历史会话」卡片上（`scan_misplaced_home` / `recover_misplaced_home`，执行前要求**目标实例**的内核未在运行——判据按实例 pid 文件而非本壳工作台）。事故经过见 [agent-lessons §7](docs/reviews/agent-lessons.md)。

**数据迁移整个功能在 v0.6.0 之后移除**（维护者 2026-10-07 决定）。过渡期内它是正式功能：侧栏「系统」组里紧跟「设置」的**常驻菜单**，删除清单见 [ui/AGENTS.md §待移除](ui/AGENTS.md)——前后端四块约 4100 行，**光删前端菜单不算数**。两处名字带 migration 但不要跟着删：`legacy_migration_target`（v0.2.x 平铺目录搬家）与 `store_relocate`（插件中央库搬迁）。`home_recovery` 依附本功能、届时一并走，但它读别的实例的 `sessions/`，动手前先确认没有用户依赖（那是 2026-09-28 事故的回收补救路径）。`instance::ensure_instance_mutable` 仍保留，但作用已收窄为「两个壳被指到**同一个**实例、且那个内核还活着」时拒绝插件与内核版本变更——默认路径下两个壳永远落在不同实例上，它是一道兜底而不是日常必经的门。这两条纪律由 `check:invariants` 第 10 / 11 项机械兜底：① 生产代码里 `InstanceRecord::new` 的 id 实参不得是常量；② 除 `instance.rs` 外不得写 `default_instance_id = <具体 id>`。

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

- **Codex 额度来源是显式许可，不是模型登录的一部分**：Rust 的 `usage/subscription_codex.rs` 只在用户开启后读取本机 Codex 登录文件，账号不一致、过期或身份无法确认时拒绝；不得刷新、修改或复制凭据。许可按壳模式隔离。前端共用 `CodexUsageConsent.vue`，事件 `codex-usage-consent-changed` 同步开关；来源切换必须清掉旧额度，界面标明「Codex 额度」，不能称作 SIWC 应用用量。这是一条跨前后端契约。

- 长任务失败时进度面板保持开放，由用户手动关闭；完整原始输出始终落盘，报错信息引用日志路径。
- **按职责主动拆分，优先复用已有代码**：新增逻辑前先检查现有组件、store、服务与共享层。一个模块出现多个独立变化原因、可单独测试的职责，或跨模块相同语义的重复逻辑时，应在本次改动内做必要拆分或提取共享实现，并保持接口稳定。紧密协作、同一职责的代码可保留在一起；没有真实复用场景时不预建抽象。不设文件行数、全仓行数或新文件登记预算，历史注释里的旧行数账本也不再作为约束，不为凑行数压缩排版、强拆组件或扩大任务范围。评审需说明拆分与复用的理由，并验证受影响的调用方、测试和文档；重复代码检测保留，产物体积预算另行执行。
- **跨模块重复先提共享层**：包取源放 `pkg/`（插件与技能共用，只返回纯文本原因、错误分类由调用方决定），JSON 状态文档读写放 `shell/state.rs`（容错读 / 校验读分开，文案由 `StateCtx` 提供），前端异步样板放 `ui/src/shell/async.js`（`singleFlight` / `createStatusSource` / `createUpdateChecker`）。安装预检拆两层：`plugins/sandbox.rs` 只管"起一个临时内核、探它、收摊"（与装什么无关，技能预检直接复用），`plugins/precheck.rs` 管两段式事务（快照 → 装进沙盒 → 探测 → 提交/回滚），`plugins/center.rs` 只暴露 `store_file` 一条可见性缝——**预检的事务主体不要写进 `center.rs`**，沙盒生命周期与安装事务有不同职责，应分别维护。
- **反向验（故意弄坏 → 确认它会响）之前先确认「改动真的进了被测的那个产物」**：恢复文件后必须更新它的 mtime，否则 cargo / vite 判定无需重编译，第二次跑的还是改坏时的产物——结论会变成「恢复后仍然红」，而真相是**根本没重编**。恢复后补一句 `(Get-Item <file>).LastWriteTime = Get-Date` 再跑。同理，**反向验的红必须是「改坏的当次」的红**——中途任何一次重编失败都可能让红变成「编不过」而不是「断言不成立」，那不算验过。经过见 [agent-lessons §3](docs/reviews/agent-lessons.md)。
- **扫源码文本的判据：锚点找在原文上，匹配再过 `productionRust()`**。直接匹配原文会把注释里那一份串也算进去（`skills-reconcile-wired` 2026-10-11 假绿）；反过来先剥全文再找锚点同样坏——Tauri 入口是 `.setup(|app| {…})` **闭包**，`fn setup(` 在 `lib.rs` 里根本不存在，`indexOf` 返回 -1 后区间只剩十几字节，判据**恒红**（`plugins-reconcile-wired` 2026-10-12）。恒红的判据最危险：反向验照样「通过」，而红的是坏判据不是变异生效。**已有 27 条是否都有这类问题尚未逐条复查**，改任何一条前先反向验。
- **doc 承诺「下次启动会接手」不等于它接上了**。`skills::manage::reconcile` 与 `plugins::center::reconcile_store` 前后脚撞见同一形状——调用点全在 `#[cfg(test)]` 内，于是崩溃残留无限堆积、v0.2.x 平铺中央库的搬迁从未发生。两者现均已接进 `setup()`，由第 26 / 28 项判据钉住；判据之所以必要，是因为 crate 级 `allow(dead_code)` 静音了编译器本该指出的声音。
- 图标只从 `assets/whale-icon.svg`、`assets/whale-icon-small.svg` 与托盘专用的 `assets/whale-head.svg` 生成，规则见 [docs/ui/icon-design.md](docs/ui/icon-design.md)。**面板里的第三方标志（npm 等）另有一条规则**：必须是 `ui/public/` 下的本地矢量、不许写远端 URL，并保留来源与许可声明——`tauri.conf.json` 的 `csp` 是 `null`，远端 `<img>` 出不出网完全取决于用户那台机器，取不到时页面上只剩一块白砖且没有任何报错（版本面板过去就挂着 `avatars.githubusercontent.com` 的 npm 头像）；由 `check:invariants` 第 15 项钉住。
- **常驻行为只有一份实现，在 `shell/resident.rs`**（2026-10-02 起两平台统一）。关闭窗口一律只是收进后台（Windows 托盘 / macOS 菜单栏），真正退出只从图标菜单发起。这条纪律的代价是两处：`CloseRequested` 统一走 `resident::intercept_close`（不要按平台分叉回去），恢复动作统一走 `resident::show_main_shell`（`lib::show_main_shell` 与工作台拉绳的 `focus_main_shell` 都调它，**抄一份就会漂**——Windows 上两边互调过一次，直接栈溢出）。**收起同理只有 `resident::hide_to_shell` 一份**：macOS 上激活等级没降到 Accessory 时，进程会带着「在运行」小点的 Dock 图标、零可见窗口地挂在后台，点它还没反应（`RunEvent::Reopen` 只抬工作台，而 macOS 的激活不会替我们显示 `hide()` 掉的窗口——抬不到时必须兜底 `show_main_shell`），由 `check:invariants` 第 17 项钉住。收起那一刻窗口已隐藏、页内提示谁也看不见，所以**收起时不广播事件**，改由 Rust 在恢复时补发 `shell-restored-from-background`（每进程最多一次，且只在本进程发生过用户收起之后；登录自启的隐藏不算，`consume_restore_hint` 双旗消费制）；改事件名要同时动 `App.vue` 与本文件。**Windows 上任务栏按钮的删与补必须成对**（`resident.rs` 同一个函数里的 `set_skip_taskbar(true)` / `(false)`，由 `check:invariants` 第 17 项 ④ 钉住）：删除动作走 `ITaskbarList::DeleteTab`，`show()` 不会让它自己回来。**「回到工作台失败」按内核是否在跑决定出不出声**：Windows 上点通知横幅与点任务栏启动项，系统都按同一个快捷方式再拉起一个 exe，壳无从区分（`activate.rs`）。内核没跑时只是此刻没有工作台——概览页上就有「启动工作台」按钮，弹 8 秒红字纯属噪音（2026-10-09 用户反馈，判据见 `announce_failure`，单测 `only_a_real_fault_announces` 反向验过）；内核在跑却打不开才是真故障，广播 `workbench-activate-failed` 给 `App.vue` 的 toast。判据取 `kernel_running` 而非匹配 `open_harness` 的报错文案——改一次措辞就静默失效。**「点通知横幅回到工作台」的两条入口（Windows 管道交接 / macOS 激活观察）必须都接在 `notify::activate::serve` 上**，由 `check:invariants` 第 29 项钉住：判据是纯函数，摘掉接线后单测照样全绿。同一段的 macOS 专属符号一律挂 `#[cfg(target_os = "macos")]`——clippy 的 `-D warnings` 只在 macOS 上跑，漏挂时只有本地 Windows 的 `npm run check` 会红。经过见 [agent-lessons §6](docs/reviews/agent-lessons.md)。
- **登录自启归系统，偏好归壳，两者分开**。`shell/autostart.rs` 只写系统登录项（macOS LaunchAgent plist / Windows `HKCU\...\Run`），「登录时启动工作台」是 `Settings::autostart_kernel` 里的一个 bool。**写 / 读 / 删会碰到用户真实的登录项，测试一律不许动它**——`autostart.rs` 的测试只覆盖纯逻辑（命令串拼装、`detect_autostart_arg` 只认独立 token、路径归一化、启动判据的三个条件），写路径只能人工在装好的机器上验证。

## 发布

版本发布由 `.github/workflows/desktop-release.yml` 负责。发布前必须确认 **`package.json`、`src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 三处**的 `version` 完全一致，并且版本提交已经推送到 `main`。workflow 使用 `TAURI_SIGNING_PRIVATE_KEY` 给更新制品签名，`releaseDraft` 与 `prerelease` 必须保持为 `false`，以保证 updater 的 latest endpoint 可用。

**第三处 `Cargo.toml` 不是编译错误**：crate 版本与壳版本不同源，Tauri 打的产物元数据会带着一个对不上的壳版本号，updater 与「关于」面板显示的都可能不是这个 release。**改版本号一律三个文件一起改**，别等门禁报。经过见 [agent-lessons §4](docs/reviews/agent-lessons.md)。

**开发时默认只提交，不推送**（2026-09-30 起，2026-10-15 划清与发布的界线）。改完代码跑完门禁就 `git commit`，**用户没说发布就不推**——推远端不可逆，而且这台机器上常有并行会话往同一个工作树提交，推之前必须先看 `git log --oneline origin/main..HEAD`，确认要推的提交里**有没有别人的**（并行会话的改动会跟着一起上去）。想看未推送的有什么就 `git status -sb` 报「领先 N 个提交」，不要自作主张推。

**明确要求发布＝已经授权，不要再逐次停下来问**（2026-10-15 用户拍板）。「发布 v0.4.13」「打个 tag 发个版」这类指令里已经含着授权，照做即可：抬三处版本号 → 跑门禁 → 提交 → `git push origin main` → 在**刚推上去的那个 commit** 上建 tag → 推 tag（tag push 自动触发发布）。**报告要照给，只是不用等**：动手前先把将要推的提交列出来（几个、谁写的、本版装了什么），说清推 tag 会触发 workflow，以及有没有**没在真机上验过**的东西（那是用户该知道的风险，不是叫停的理由），然后继续做完。发布后按下面的命令验证。只有用户要推的东西与发布无关（例行开发、评审修复）时才回到上一条：那种情况提交完就停。

**`git add` 只写本次实际改过的文件，禁止 `git add -A` / `git add .` / `git commit -a`**（2026-10-07 补）。这条不是洁癖：同一台机器上有并行会话在同一个工作树里干活，它整文件 add 时会把**我还没提交的在途改动一起卷走**（已连续发生两次）。被卷走的后果不是丢失内容（它确实进了仓库），而是**那一行改动出现在一个与它毫无关系的提交标题下**，解释它的注释与守卫测试还留在工作区外，`git log` 里从此对不上号。写完代码、跑完门禁就提交，把在途窗口缩到最短；提交前用 `git diff --cached --stat` 确认暂存区里只有自己的东西。

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
- **改 `tauri.conf.json` 的 `bundle.resources` 之前，先确认新增的目录在「全新 checkout」里存在**。`src-tauri/resources/builtin-plugins/` 是构建期产物（由 `npm run prep:builtin` 从 `plugins/openai-oauth/` 生成，`.gitignore` 挡住），而 build script 校验的是登记在 `bundle.resources` 里的**每一个**路径——缺一步，CI 会死在 `resource path '…' doesn't exist`，且 quality job 与两个平台的 `tauri build` 报同一行、而 `Publish release` 被跳过；本地 `npm run dev` / `build` / `check` 都各自带着这一步，所以本地永远看不出来。`scripts/dev-builtin.test.mjs` 钉住接线形状：**任何会跑 `cargo build/check/test/clippy` 的 workflow job 都必须先生成资源**，并带「判据空转时必须红」的兜底。新增同类生成目录时把生成步骤挂进判据，不要只改 workflow。经过见 [agent-lessons §5](docs/reviews/agent-lessons.md)。
- **失败后要重发同一个版本，只能把 tag 移到新 commit**：`preflight` 断言 `refs/tags/desktop-v<version>` 必须与 `$GITHUB_SHA` 一致，所以手动 dispatch 绕不过去。正规做法是 `git push origin :refs/tags/desktop-v<version>` 后在新 commit 上重建并推送（tag push 会自动触发新 Run）。动之前先 `gh api repos/July-X/dsh-xlink/releases/tags/<tag>` 确认该版本**还没有 Release**——已发布过的 tag 删了会让 updater 指向不存在的版本。
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

修改用户可见行为、数据目录、发布流程或安全策略时同步更新 `README.md` 与对应 `docs/`。文件保持 UTF-8、恰好一个末尾换行；不提交依赖目录与构建产物。

## 窗口外观作用域（2026-10-09）

macOS 建窗用 `shell::appearance::initial_theme()`，前端 `bridge.setWindowTheme` → `set_window_appearance`，仅设置当前 `NSWindow`。**不得用 Tauri 的应用级主题覆盖**来模拟单窗口外观，否则工作台的固定深色会阻断管理窗口跟随系统。前端细则见 `ui/AGENTS.md`，原生实现在 `shell/appearance.rs`。
