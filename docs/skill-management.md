# dsh-xlink 技能（Skill）管理设计

> 本文档描述桌面外壳的技能管理功能：中央存储、物化到内核读取路径、启用/禁用、更新提醒与社区目录。
> 设计参照 [plugin-management.md](plugin-management.md)（社区插件管理）的同构模式，并按技能的本质差异做了简化。用户文档见 [README.md](../README.md)。

> **状态（2026-09-30）**：多内核改造落地后，中央库已迁到 Xlink home 的 `skills/packages/`，活动视图迁到
> `skills/active/`（v1 全局共享）。内核侧的接入**已闭环**：壳在每个实例的
> `cordis.patch.yml` 里插一条自己的 `skill-filesystem` 行，把活动视图作为
> `customSkillDirs` 交给内核（`DSH_CUSTOM_SKILL_DIRS` 只是留给未来内核版本的
> 兜底，当前内核不读它）——机制、取舍与验证见
> [「技能接线」一节](#技能接线壳怎么让内核看见活动视图)。
> 权威路径说明见
> [architecture.md §「多内核改造后的实际数据布局」](architecture.md)
> 与阶段性状态快照
> [multi-kernel-migration-status-2026-09-19.md](multi-kernel-migration-status-2026-09-19.md)。
> P6 step 5 / P8 落地后本文会按新的实际行为重新校对；当前以代码为准。

## 目标

用户可以把社区技能（GitHub 仓库、npm 包、本地文件夹）安装到本地，由桌面外壳统一管理，并且：

1. **集中管理**：所有技能源存放在 dsh home 下专属目录，绝不写入任何内核安装目录。
2. **零接线生效**（设计意图，接入方式见[「技能接线」一节](#技能接线壳怎么让内核看见活动视图)）：壳把技能物化进 `skills/active/`，由内核的 `dsh-skill-filesystem` 读；不改 cordis 配置、不动 profile、不装依赖。
3. **热生效**：利用内核对技能根的文件监视（chokidar → `skills/change`），安装/卸载/启用/禁用对**运行中的工作台即时生效**，无需重启内核。
4. **更新提醒**：管理界面在有新版本时提醒一键更新；支持社区目录浏览与搜索。

## 与插件管理的同与不同

桌面壳沿用插件管理的骨架（中央 store + 清单 + 物化 + 更新检查 + 目录缓存 + Tauri 命令层 + 静态面板），但技能是指令数据而非代码，四个环节显著简化：

| 维度 | 插件（plugins.rs 现状) | 技能（本设计） |
| --- | --- | --- |
| 形态 | npm 包 / git 仓库，package.json 声明 bundle 层 | 目录 `<name>/SKILL.md` 或平文件 `<name>.md`，frontmatter 声明元数据 |
| 构建 | 中央库内 pnpm install + prepare 构建 | **无构建**，Markdown 即产物 |
| 接线 | 改写 profile package.json + pnpm install 铺 node_modules | **无接线**，内核内建扫描固定根 |
| 物化目标 | 每个实例的 extensions/plugins/ 目录各自一份 | `<DSH_XLINK_HOME>/skills/active/` 全局共享一份（v1）→ **无按实例物化** |
| 生效时机 | 重启内核（profile 层启动时快照） | 文件监视即时失效重发现 |
| 切换内核 | 补物化 + 改写依赖路径 + pnpm install | **无操作** |

## 内核侧对接面（只读依赖，不改内核）

内核 `dsh-skill-filesystem` 按 rank 升序扫描以下根，rank 小者胜同名：

| Rank | 来源 | 根 | 与本设计的关系 |
| --- | --- | --- | --- |
| 100 | project-dsh | `<projectRoot>/.dsh/skills` | 项目级覆盖全局（壳不写） |
| 200 | project-agents | `<projectRoot>/.agents/skills` | 同上 |
| 300 | custom | `Config.customSkillDirs` | **壳的接线点**（`skills/active/`，P5 起）——壳在实例 `cordis.patch.yml` 里插自己的行，见[「技能接线」一节](#技能接线壳怎么让内核看见活动视图) |
| 400 | user-dsh | `<DSH_HOME>/skills`（`DSH_HOME` = 实例 home 目录） | 内核默认根，**壳不写**（P5 之前才是壳的接线点） |
| 500 | user-agents | `<agentsHome>/skills` | 用户手放技能，壳只读展示 |
| 600 | bundled | `Config.bundledSkillDir` | 打包技能，壳不涉及 |

壳定位 dsh home 走 `kernel::data_dir` 的解析顺序（`DSH_DESKTOP_DATA_DIR` 覆写 → `<DSH_XLINK_HOME>/<family>/desktop[-dev]/` → app-data 回退），再由启动时注入 `DSH_HOME` 指向**实例**的 home 目录，保证壳写的目录与内核读的目录永远一致。

内核侧约束（壳的校验规则与其对齐，fail loud 在壳这一层完成）：

- 只发现根下**直接一层**：`<root>/<name>/SKILL.md` 或 `<root>/<name>.md`；不支持嵌套递归发现。
- 技能名取自 SKILL.md frontmatter 的 `name`，必须 kebab-case（`^[a-z0-9]+(?:-[a-z0-9]+)*$`）；`description` 必填。缺任一项内核**静默忽略**（仅日志告警）——所以壳必须在安装时预校验，否则用户会看到"装了却不出现"。
- 调用策略 frontmatter：`disable-model-invocation`、`user-invocable`（缺省均 true）。这是作者语义，壳不代改。
- 监视跟随符号链接（followSymlinks 默认开）：link 模式物化的技能同样被热发现；根的直接条目增删、`<dir>/SKILL.md` 内容变化都会触发 `skills/change`，工具消费者在下一个模型步骤前重注入 `<available_skills>` 目录。

## 目录布局

```text
<DSH_XLINK_HOME>/                   # 默认 ~/.dsh-xlink（DSH_XLINK_HOME 可重定向）
├── skills/                         # 技能的两个子层（P5 起拆分）
│   ├── packages/                   # 中央技能库（经校验的源包，壳独占写入）
│   │   ├── store.json              # 清单：包条目（来源/版本/mode）+ 每技能条目（名称/enabled/路径）
│   │   └── <pkg-id>/               # 一个包的源（npm tarball 解包或 git checkout）
│   │       ├── .dsh-source.json    # id/来源/版本/拉取时间
│   │       └── …                   # 包内容，可含一个或多个技能
│   └── active/                     # 活动视图（壳写的唯一生效面），**所有 DSH 实例共享一份**（v1）
│       ├── <skill-name> → ../packages/<pkg-id>/<…>/   # link 模式：指向中央库内技能目录
│       └── <skill-name>.md → ../packages/<pkg-id>/<…>.md
└── kernels/<family>/instances/<id>/home/
    └── skills/                     # 该实例的 DSH_HOME/skills，实例级技能根
```

三个路径层级各有归属，不要混：`packages/` 是**中央库**（壳写），`active/` 是**全局活动视图**（壳写、多个实例共读），`instances/<id>/home/skills/` 是**实例根**（内核默认技能根，壳不写）。v1 的启用状态全局共享，实例级覆盖是后续版本的事（见设计稿 §9.1）。

P5 之前的旧布局 `<xlink_home>/skills/` 既当中央库又当活动根，现已由 `legacy_dsh_skills_root` / `legacy_dsh_skills_store` 作为**只读兼容入口**取代。

**关于 `skills-catalog.json`（规划中，未实现）**：它要与插件侧的 `plugins-catalog.json` 同构，是「社区技能目录」的本地缓存。插件侧的做法是 `plugins::fetch_catalog`——优先拉 `https://dshfind.com/api/plugins-data`，不可达时回退到参考市场，`CATALOG_TTL_SECS` 内直接读缓存，过滤掉无法安装的条目后 `atomic_write` 落盘；UI 在这份缓存列表上做搜索与分类筛选，因此筛选是即时的。技能侧规划了同样的机制（`skills.rs` 里对应 `scan / materialize / check_updates / catalog / install / …` 那一列），但 `CATALOG_CACHE_FILE`、`fetch_catalog` 与浏览 UI **都还没写**——`skills.rs` 里搜不到 `skills.catalog` 任何形式的引用。所以当前技能只能靠「手动安装」行输入来源 spec 安装，没有社区目录可逛。将来实现时它会落在 `<xlink_home>/<family>/desktop[-dev]/skills-catalog.json`，与插件缓存同目录同 TTL。

技能 fetch 不经过 pnpm，git 的有限输出直接进入错误消息与进度面板；npm tarball 由 Rust 解包器校验并发布，不依赖系统 tar，因此不设 `logs/skill-*.log`。

包 id 映射与插件一致：`/` 替换为 `__`（`@ace-zone/dsh-skills` → `@ace-zone__dsh-skills`），拒绝 `..` 与空段。本地文件夹导入以文件夹名为 id，加 `local:` 来源标记。

**安装单位 = 包，物化单位 = 技能。** 这是与插件的关键差异：一个包可含多个技能（如 monorepo 仓库技能散布在子目录），物化时每个技能独立落一条链接，启停粒度也是单个技能。单技能包则整包即一个技能。

## 安装流程

以 npm/git 来源为例（本地文件夹 = 把源路径纳入中央库管理，其余相同）：

1. **fetch 进中央库**：npm 取 `dist-tags.latest`（或指定版本）下载 tarball，完整写入 `.part` 后再发布，由 Rust 解包器只接受 `package/` 根并拒绝越界路径、链接和特殊文件；git 深度克隆也通过有界输出捕获。写 `.dsh-source.json` 与 store.json。
2. **扫描与校验（fail loud）**：在包内探测技能入口——任意目录下的真实 `SKILL.md`（探测深度 ≤3 层，覆盖根即技能与常见 monorepo 布局）及顶层平铺 `*.md`；逐个解析 frontmatter，校验 kebab-case `name` + 非 `description`。符号链接（含目录与 `SKILL.md` 文件）一律不视为技能入口，避免 `git clone` 保留的装饰性重定向（如 blader/humanizer v2.11.1+ 的 `skills/<name>/SKILL.md → ../../SKILL.md`）把同一技能重复计入。一个技能都没有 → 安装失败并给出原因；包内重名（frontmatter name 冲突）→ 整包拒绝。
3. **物化到活动根**：对每个校验通过的技能，在 `<DSH_XLINK_HOME>/skills/active/` 建链接指向中央库内的技能目录/文件，条目名 = frontmatter `name`。macOS/Linux 用符号链接；Windows 上**目录用 junction**（`mklink /J`，普通用户即可创建，不需要 `SeCreateSymbolicLinkPrivilege`），**扁平 `.md` 文件用文件符号链接**。链接创建失败时降级为整树复制，实际模式记入 store.json 并回显到面板的模式 chip。
4. **所有权凭据**：无论链接还是复制，落地后都会把活动根条目的**内容指纹**（sha256）写进 store.json 的 `materialized_sha256`。判定"这个条目归本商店所有"时接受两种证据——链接解析到中央库源，或内容与记录的指纹一致；两者都不成立（用户手放的、落地后被改写过的）一律不动。指纹是 copy 模式唯一可用的凭据：没有它，复制出来的副本无法与用户自己的同名目录区分，卸载会静默失效。
5. **无第 5 步**：不跑 pnpm、不改 profile——插件流程里最重的两步在这里不存在。
6. **生效反馈**：壳探测内核端口是否在监听；运行中提示"已对工作台即时生效"，未运行提示"下次启动自动可用"。

卸载 = 反向执行：拆除该包全部技能的活动根条目（按上述所有权凭据判定）、删除中央库目录、更新 store.json。全程无进程重启。

## 启用 / 禁用

> **面板现状**：面板已接线启停。「已安装」卡片在包头提供每个包内技能的开关（`skill_set_enabled`，挂按条目的 loading key `skillEnabled:<包 id>:<技能名>`）。粒度是**单个技能**而不是整包：停用比整包卸载轻，条目留在中央库、随时可恢复。状态不一致（`enabled: true` 但条目不在活动根中）时保留「条目缺失」提示，可先「重新同步」。

- **禁用** = 从活动根摘除该技能的条目（源完好保留在中央库），store.json 记 `enabled: false`；**启用** = 重建链接或副本并刷新指纹。
- 启动对账（`reconcile`）会为历史数据补记缺失的指纹：修复前落地的 copy 条目没有这个字段，补记之后才能被正常停用与卸载。
- 不改写 SKILL.md 内容——`disable-model-invocation` 等 frontmatter 是技能作者的语义，壳的状态与之正交。
- 内核 watcher 观察到条目移除/出现后自动失效重发现，运行中的会话在下一步模型请求前看到更新后的目录。

## 切换内核 / 多内核

所有内核版本共享同一个 `<DSH_XLINK_HOME>/skills/active/`，且技能不进任何内核的 node_modules 解析路径，因此：

- `activate_version` / `start_kernel` 对技能**零操作**（插件需要的 ensure_wiring 校正在这里不存在）。
- dev 壳（desktop-dev）与 release 壳共享技能视图——用户级技能本就该全局一致，不存在 settings.json 那类争抢问题。
- 项目级技能（rank 100/200）天然覆盖壳管理的全局技能，面板在检测到同名冲突时展示"将被项目级覆盖"提示（壳只读项目根做提示，不写）。

## 更新提醒

三种来源的最新版本判定，与插件完全同模式（ureq + rustls / `git ls-remote`）：

| 来源 | 最新版本来源 |
| --- | --- |
| npm 包 | registry 文档 `dist-tags.latest` |
| git（锁定 tag） | `git ls-remote --tags` 中语义化最新的 tag |
| git（跟随分支） | `git ls-remote <url> HEAD` 与本地 sha 比较 |

「检查更新」遍历 store.json 写回结果；网络请求在商店写锁之外执行，提交时会重新读取清单并确认安装版本没有变化，避免旧检查结果覆盖并发完成的更新。UI 徽标与逐行「更新」按钮沿用插件面板。更新 = 原位重拉中央库（`.tmp-*` → `.new-*` → `.backup-*` 三段式替换）→ **重新扫描技能集合并校正物化视图**（新增补链、消失拆链并在进度里列出增删明细，幸存技能保留各自的启用状态）→ 即时生效。本地文件夹来源不做版本检查，但保留手动「重新同步」：改完源文件夹后一键重导并按同样的增删逻辑校正物化视图。

## 手动安装与社区浏览入口

v1 只提供手动安装：与插件面板同款的「`<input>` 地址 + 回车安装」一行（`#skillSpec`，右侧 `↵` 为视觉提示），标题旁的信息图标 hover 展开支持的来源说明；placeholder 只引导 git 仓库地址（`https://github.com/owner/repo.git`、`owner/repo` 简写、追加 `#tag` 锁定版本）。`installSkill()` 与插件的 `installPlugin()` 同构；解析层（`skills.rs::parse_spec`）同时接受 npm 包名（`@scope/pkg@1.2.3`）与本地文件夹路径（绝对路径 / `~/…` / `local:` 前缀 / Windows 盘符路径），但 UI 不引导这两种来源。GitHub `dsh-skill` topic 在面板下方以链接常驻，供用户浏览社区资源后把地址粘贴到手动安装行。

## 安全边界

- 技能是指令文本，会整体进入模型上下文——恶意技能等价于提示注入；其引用的 scripts/resources 还可能被 agent 后续执行。因此：安装动作逐次确认；确认前可展开预览正文与文件清单；未验证标记原样展示。
- 元数据解析只信任 npm registry 与 GitHub raw（与插件、内核更新菜单同一信任边界）。
- 活动根条目名强制 kebab-case（同时是内核要求），从源头排除路径穿越；中央库 id 映射拒绝 `..` 与空段。

## 模块映射（实现落点）

| 位置 | 内容 |
| --- | --- |
| `src-tauri/src/skills.rs`（新增） | 镜像 plugins.rs 结构：store/scan/materialize/check_updates/catalog/install/update/uninstall/set_enabled/reconcile。纯文件操作，无 pnpm 构建链（仅 git/tar 子进程，走 `process::command_with_path`） |
| `commands.rs` | `skill_status` / `skill_install` / `skill_update` / `skill_uninstall` / `skill_set_enabled` / `skill_check_updates` 命令族，全部 async + `spawn_blocking`（精简版 `run_skill_command`：技能无需 pnpm 解析），长任务走 Channel 推进度 |
| `ui/src/skills.js` + `components/SkillsPanel.vue` | 技能页签：包行展示名称、来源、版本和包头启停开关，右侧提供更新 / 仓库 / 卸载动作；手动安装行复用插件同款输入行与 `withProgress` 进度面板 |
| `settings.rs` | 无新字段（接线点固定） |

frontmatter 校验是内核规则的壳侧前置：解析器只取 frontmatter 顶层 `name` / `description`（带引号去引号），无法解析或不符合 kebab-case 的候选按"内核也会忽略"处理——安装时以警告形式展示并跳过，整包一个可用技能都没有才失败。这比内核的静默忽略更响，避免"装了却不出现"。启动对账 `skills::reconcile()`：清理三段式暂存残留、为启用技能补链/修复断链、清退停用技能的残留、清扫指向中央库但不在清单中的孤儿链接（用户手放的文件与非本库链接一律不动）；失败写入 store.warning 由面板展示。

**暂存残留的清理为什么必须发生在「读标记」之前**（`recover_staging`）：`.tmp-*` 是 fetch 阶段的暂存目录，**刻意不打 id 标记**——`stamp_id_marker` 内部走 `atomic_write`，只有 rename 成功才会出现正式的 `.dsh-id`；而提前盖章正是当初修 Windows `ERROR_DIR_NOT_EMPTY` 的方案（见 `pkg::new_staging_dir` 注释）。所以「创建暂存目录 → 写标记」之间崩溃的残留读不到 id，而 `pkg::recover_staging_dir` 里 `StagingKind::Tmp => true` 的无条件回收分支对这类残留是**够不到的死代码**。正确做法与 `plugins.rs` 一致：读不到标记的暂存目录直接回收——它还没 rename 成交付目录，从来不是用户数据。名字以暂存前缀开头的**正式**目录（npm 允许 `tmp-foo` 这类名字）靠 `id == name` 判断豁免，不能被误删。

## 技能接线：壳怎么让内核看见活动视图

> 2026-09-30 落地。P5 起壳把技能物化进 `skills/active/`，交给内核的方式是
> `DshAdapter::custom_skill_dirs()` → `start` 写 `DSH_CUSTOM_SKILL_DIRS`。**已装内核
> 0.2.0-rc.2 并不消费这个 env**：

```text
$ grep -r "CUSTOM_SKILL" <install>/node_modules/@deepseek-ai   # 3481 个 js/mjs/cjs/ts/d.ts
no CUSTOM_SKILL match
```

`dsh-skill-filesystem` 确实有 `customSkillDirs`（`Config` schema，默认 `[]`），但它
**只从插件配置读**——`cordis.patch.yml` 里某个 `skill-filesystem` 行的 `config:` 段。
对照组能说明 env 注入本身是内核认可的机制：`agentsHome` 明确回退到
`process.env.DSH_AGENTS_HOME`，技能目录没有这条回退。也就是说 P5 之后的半年里
「装得上、内核看不见」，而 README、UI 提示气泡与本文档都按已生效在写。

### 现在怎么接的

壳在**每个实例**的 `$DSH_HOME/cordis.patch.yml` 里追加一条自己的 loader 行
（`kernel_adapter::ensure_skill_wiring`，`prepare_instance` 每次启动都跑）：

```yaml
# dsh-xlink managed cordis patch
- insert:
    - id: xlink-skill-filesystem
      name: '@deepseek-ai/dsh-skill-filesystem'
      config:
        providerName: xlink
        includeDefaultRoots: false
        customSkillDirs:
          - "C:\\Users\\<user>\\.dsh-xlink\\skills\\active"
```

四条设计决定，每条都有代价：

- **插入自己的行，不改内核那一行**。`dsh-web-app` 把宿主层的 `skill-filesystem`
  明确 `disabled: true`（同文件注释：preset 拥有本地发现），按 id 打补丁只会改到
  那个被禁用的行。`dsh-web-app` 注释里点名的「deployment 级 provider —— 宿主层
  的 skill-filesystem 行」正是这条新行的角色：它注册进全局层，每个 session 的
  scope chain 都会读到。
- **`includeDefaultRoots: false`**。这一行只提供壳管理的活动视图；`<DSH_HOME>/skills`、
  `<agentsHome>/skills`、项目根与打包根由各 preset 自己的 `skill-filesystem` 行负责
  （它们各自带 `customSkillDirs` 指向 agent-preset 包内的 skills）。不去重复扫。
- **`providerName: xlink`**。同一层里两个同名 provider，后来者只会拿到一个空壳
  disposer（内核对重复 provider 名是这么处理的），与 preset 的 `filesystem` 区分开。
- **不用 `DSH_BUNDLED_SKILL_DIR`**。它是内核唯一读的技能目录 env，但对应的根带
  `trustedHost: true`——把社区技能标成「随应用打包的可信技能」是安全语义的错配。

写入规则：**只追加，不改写**。patch 按 id 定位、后写覆盖先写，所以活动视图路径变了
（`DSH_XLINK_HOME` 改了）就再追加一行，旧的自然失效；用户自己写的条目一律原样保留。
顶层不是列表时**不碰**——patch 文件是内核 fail-loud 的输入（历史上一次坏模板就让内核
启动即崩），壳没有资格把它改成另一种语法。写失败不阻断启动（少一批技能 ≠ 内核不可用），
但会走 `shell_events::record` 落到「查看日志」。

`DSH_CUSTOM_SKILL_DIRS` 仍然留着：内核哪天补上 env 回退（对齐 `agentsHome` 的既有
写法）不必改壳就能生效，在那之前它无害。

### 验证

`dsh --profile web --dump-config` 会打印**与启动同一份**组合结果（内核自己的注释：
dump "can never drift from what boots"），实测能看到上面那行；再进一步，起一次真实
内核、建一个 session、走 `skills/list` RPC，活动视图里的技能出现在该 session 的技能
目录中（探针跑在临时 `DSH_HOME` 上，结束后已清理）。

### 仍然盖不住的情况：更高优先级的同名条目

内核的 rank 定死了次序，壳改不动：`<projectRoot>/.dsh/skills`（100）与
`<projectRoot>/.agents/skills`（200）排在壳的 custom 根（300）**前面**。而内核找
项目根的方式是从会话工作目录向上找第一个 `.git`——实例工作区都在家目录之下，
所以**家目录带 `.git` 时它就是那个项目根**（2026-09-30 本机实测：确实有 `~/.git`），
于是 `~/.dsh/skills`（P5 之前的活动视图残留）与 `~/.agents/skills` 实际扮演「项目级」
角色。后果：与那里同名的技能，壳管理的更新对它不生效（内核一直在读那份旧的）。

反过来也必须成立：**家目录没有 `.git` 时什么都不报**。那时 `~/.dsh/skills` 根本不会
被扫，而 `~/.agents/skills` 只是 `user-agents`（rank **500**）——它排在 custom **之后**，
壳的同名技能赢。把 500 当成能盖住 300 的根，报出来的就是假警告，而假警告会让人去删
一个正在生效的文件。判据因此同时依赖「`~/.git` 存在」与「那份文件确实在」两件事，
`paths::shadowing_skill_roots(home)` / `shadowing_skill_entries(active, home)` 两条纯
函数把这件事做完，测试用临时家目录夹具把正反两面都钉住。

### 面板给的那条出路：改名让路，不是删除

面板顶部 warning 列出被盖住的技能与盖住它的文件。原先它只给一句话让用户自己去
删，**判据已经精确到具体文件而按钮不存在**——可执行的下一步不该只活在文档里。
现在 warning 下方多一个「移走被盖住的条目」（判据非空时才出现），后端
`skill_move_aside_shadowed` → `skill_shadow::move_aside_shadowed`：

- **只改名，不删除**。复用 `skills::keep_aside`，落点是 `humanizer.md.user-<时间戳>`：
  不再以 `.md` 结尾，所以内核的发现（只认 `*.md` 与目录包）不会再扫它，判据也不会
  再命中——移走之后告警真的会消失。回退就是把文件名改回去。
- **只动判据点名的那几条**。列表来自 `shadowing_skill_entries`，动手前再确认落点
  仍在两个高优先级根内（判据算完到动手之间文件可能已经变了），认不出来就跳过并
  如实说「判据可能已过期」。同一个根里**没有**同名活动条目的文件（用户自己放的）
  原地不动。
- **不需要停工作台**。内核的 chokidar watcher 盯着这两个根，改名会让它重新发现
  技能，壳管理的那一份当场接管。这条路径是壳**唯一一处故意写 `~/.dsh/skills`**
  的地方，出发点就是上面那句「可执行的下一步」。
- 家目录没有 `.git` 时列表恒为空，命令是空操作（报「无需处理」）而不是错误——
  与判据的算术保持一致，见上一节。

`keep_aside` 因此从私有改成 `pub(crate)` 并多收一个 `reason`：同一个动作有两个
来由（中央库同名 / 被高优先级根盖住），共用实现但不能共用失败文案。

`SkillStatus.shadowed` 是**结构化**的那份判据（`{skill, path}`，路径由后端给出），
按钮据此显隐，**前端不许解析 warning 字符串**——那是给人读的一句话。

#### 2026-09-30 实机记录：那份「盖住人的文件」到底是什么

面板第一次报出告警时，几条只有看现场才能定的事：

- `C:\Users\zxx\.git` 存在，且是个**空目录**（没有 HEAD、没有 config），像是某次
  误操作留下的残骸。正是它让家目录成了「项目根」。
- `~/.dsh/skills/humanizer.md` **不是空文件**，是一个符号链接，指向
  `~/.dsh/skills-store/github.com__blader__humanizer/SKILL.md`——**P5 之前的老中央库**。
  PowerShell 对符号链接报 `Length 0`，很容易误读成「0 字节空文件」。
- 那个旧副本还在，28728 字节，与新路径
  `~/.dsh-xlink/skills/packages/github.com__blader__humanizer/SKILL.md` 的
  **SHA256 完全一致**（`E8269E23…D5`）。所以在动手之前，「更新不生效」是无害的：
  两边内容一样。这也让「移走它」成为零风险操作。
- `~/.agents/skills` 下那 8 个是目录包形态（banner-design / design / slides …），
  与壳管理的名字不冲突，不在告警里，**不受影响**。

**不要把它 relink 到新路径。** 那样两条路径解析到同一个文件，内容确实一致、壳的
更新也确实直达，但换来两个更坏的状态：

1. **停用变成假动作**。壳的开关只摘活动视图那条（`unmaterialize_entry`），包目录
   原样保留；rank 100 那条链接还在，内核照样读得到。面板说停了，实际没停。
2. **告警永远消不掉**。判据是 `Path::exists()`，而 Rust 的 `exists()` **跟随符号
   链接**——链接有效就一直报。用户会得到一条永远挂着、又永远不该照做的告警。

relink 是让壳的两套机制互相打架，而不是让它们对齐。正确处置是让路：改名为
`humanizer.md.bak` 之后，壳管理的那一份接管，启停与更新都恢复正常。

**警告文案自带可执行的下一步，模板不要再统一追加「重启应用会自动修复」**：那对
「清单缺失」是假的（`reconcile` 不会凭空重建 `store.json`），对「被盖住」更是假的
（重启改变不了内核的 rank 次序；按钮改名的路径同样不靠重启）。一句对两条都不成立
的建议，比没有建议更糟。

### 启用失败的活动视图冲突：判据与它的出路（2026-09-30）

上一节那条「被盖住」的告警讲的是**内核 rank 更高**的根。另一类同名的死胡同完全
不同：占住位置的那份就在**活动视图自己**里，于是 `ensure_entry` 在启用时直接拒绝。

本机那次的样子：活动视图里是 `humanizer.md`，**28728 字节的 v3.0.0 普通文件**（不是
链接，清单里也没有它的 `materialized_sha256`），而中央库当天已更新到 v3.1.0
（32348 字节）。`entry_is_owned` 因无指纹而认它不归本商店所有，
`identical_unowned_copy` 又因内容不同而认不出它是迁移残留——于是点启用只弹出一个
**只有「关闭」**的对话框。这条错误过去唯一的出路是让用户自己去资源管理器里删：
判据已经精确到具体文件，而可执行的出路不存在。

`skill_conflict.rs` 把那个死胡同变成一条判据 + 一个按钮，形状与 `skill_shadow` 一致：

- **判据就是 `ensure_entry` 的拒绝条件本身**，不另立标准：
  `!entry_is_owned && !identical_unowned_copy`。少一条判据，按钮会出现在本来就能
  启用的技能上；多一条，告警会在启用根本不会失败的地方响。判据与动手**故意**留在
  同一个文件——拆开之后「按钮出现在哪里」与「动手动哪些文件」就分居两处，而那种
  分叉的后果是按钮点下去动了判据没点名的文件。
- **只改名，不删除**。`identical_unowned_copy` 那条路敢直接删，是因为内容与源逐字节
  相同（不可能是用户的工作成果）；本模块处理的正是**内容不同**的那些，用户完全
  可能改过它。落点是 `humanizer.md.user-<时间戳>`，不再以 `.md` 结尾所以内核不再扫它，
  改回原名即可恢复。复用 `skills::keep_aside`，与「被盖住」共用实现但文案分开说。
- **版本证据只用于解释，不用于判定**。两份 frontmatter 的 `metadata.version` 不同时
  文案才说「活动视图里这份是 v3.0.0，技能库里是 v3.1.0，是升级后留下的旧副本」——
  「多半是旧副本」不是收编的证据，所以**不据此自动收编**，按钮仍然只改名。
- `SkillStatus.conflicts` 是结构化的那一份（`{skill, path, detail}`），面板据此显隐，
  与 `shadowed` 同一纪律：**前端不许解析 warning 字符串**。启用被拒的报错文案与按钮
  指向同一个动作，那句话因此不会指向一个面板上不存在的按钮。

**两道机械检查，别指望单测**：判据是纯函数、8 个测试覆盖了正反两面，但把
`skills::status()` 里那一句 `skill_conflict::list()` 摘掉之后，`cargo test` 630 项
与 `check:invariants` **双双全绿**（2026-09-30 实测），而按钮再也不出现、用户重新
回到那个死胡同。因此 `check-invariants` 第 ⑦ 项钉的是**接线形状**：判据必须被
`status()` 调用、结果必须交给 `conflicts` 字段、报错文案与面板按钮必须同时在。

### `description: |` 曾被读成字面量 `"|"`（2026-09-30）

YAML 的块标量 `description: |` 加缩进正文是社区技能最常见的写法，而旧的
`parse_skill_markdown` 逐行找 `key: value` 之后**跳过所有缩进行**，于是 value 只剩下
那个 `|`。结果 `store.json` 里存着 `"description": "|"`，面板 tooltip 显示的也是
`"|"`，那几行说明整个消失。

**它几乎不报错**，这是它藏了这么久的原因：`"|"` 是非空字符串，所以
`parse_skill_markdown` 判定「frontmatter 完整」照常接受，技能照样装得上、启得动，
只有说明是空的。任何「技能被拒绝了」类的排查都碰不到它。

解析搬进 `skill_frontmatter.rs`（YAML 的一个有意受限的子集：顶层标量 + 顶层块标量 +
`metadata.version`）。搬动不是「为了塞下新代码」这一条理由——frontmatter 有**两个**
消费者（包扫描与 `skill_conflict` 的版本证据），而 `skill_conflict` 第一版自己写了
一个只认 `metadata.version` 的小解析器，那正是「判据有两份实现就会分叉」的种子。
现在两处共用同一套，搬完之后 `skills.rs` 1486 → 1454、`skill_conflict.rs` 188 → 155。

范围刻意窄：只认 `|`（literal）与 `>`（folded）加 chomping 指示符，`| extra` 这种
非法写法当普通标量处理。**解析不了就拒绝该候选**这条纪律不变（安装一个外壳自身无法
校验的技能会让用户在不知情的情况下看到一个不可见的技能），所以宁可少认，不可乱认。
块正文只剥掉**这一行真的有的**那部分缩进——按块缩进硬切会在缩进更少的行上从半个词
中间切开（`  back` 变成 `ck`）。

**存量数据不会自己变好**：清单里已经写下的 `"description": "|"` 要等下一次重扫
（更新或重装该技能包）才被改写。注意 humanizer 现在 `latest == installed`，
「更新」按钮按「有更新才出现」的规则不显示，所以要刷新得**卸载后重装**。这只影响
说明文字，技能本身一直是对的。

## 已知取舍

- link 模式省空间且更新直达，但中央库被移动/删除后断链（reconcile 会标出并提示重装）；copy 模式自包含但更新需差异复制。默认策略与插件一致：优先 link，失败降级 copy，UI 明示实际模式。
- 内核 watcher 故障时其观察变为 incomplete，模型保留 last-good 目录继续工作；壳无法从外部感知该状态，技能页提供「重启工作台」兜底文案。
- 包内技能探测深度 ≤3 层是启发式：更深层嵌套的技能不会被识别（内核的直接一层发现本来也不覆盖它们，作者平铺即可）。
- 项目级部署（把已装技能导出到某项目 `.dsh/skills`）留作后续方向：需要项目选择 UI 与覆盖确认，首版不做。
- 由插件 provider 注册或 bundled root 贡献的技能不经文件系统根，不出现在面板；技能页对这些来源仅在文档中说明，不做管理。
- 社区技能目录（按主题/分类浏览 + 一键安装）是插件中心的成熟形态，技能 v1 没复制一份独立目录，只给手动安装行与 GitHub topic 入口链接。等社区中心给技能话题上线稳定 feed，再加回缓存/搜索/分页模式与插件中心同款实现；URL 与 JSON 形状契约已在设计中预留。
