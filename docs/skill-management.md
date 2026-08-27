# dsh-desktop 技能（Skill）管理设计

> 本文档描述桌面外壳的技能管理功能：中央存储、物化到内核读取路径、启用/禁用、更新提醒与社区目录。
> 设计参照 [plugin-management.md](plugin-management.md)（社区插件管理）的同构模式，并按技能的本质差异做了简化。用户文档见 [README.md](../README.md)。

## 目标

用户可以把社区技能（GitHub 仓库、npm 包、本地文件夹）安装到本地，由桌面外壳统一管理，并且：

1. **集中管理**：所有技能源存放在 dsh home 下专属目录，绝不写入任何内核安装目录。
2. **零接线生效**：内核的 `dsh-skill-filesystem` 自带扫描 `<DSH_HOME>/skills`，物化即被读取；不改 cordis 配置、不动 profile、不装依赖。
3. **热生效**：利用内核对技能根的文件监视（chokidar → `skills/change`），安装/卸载/启用/禁用对**运行中的工作台即时生效**，无需重启内核。
4. **更新提醒**：管理界面在有新版本时提醒一键更新；支持社区目录浏览与搜索。

## 与插件管理的同与不同

桌面壳沿用插件管理的骨架（中央 store + 清单 + 物化 + 更新检查 + 目录缓存 + Tauri 命令层 + 静态面板），但技能是指令数据而非代码，四个环节显著简化：

| 维度 | 插件（plugins.rs 现状) | 技能（本设计） |
| --- | --- | --- |
| 形态 | npm 包 / git 仓库，package.json 声明 bundle 层 | 目录 `<name>/SKILL.md` 或平文件 `<name>.md`，frontmatter 声明元数据 |
| 构建 | 中央库内 pnpm install + prepare 构建 | **无构建**，Markdown 即产物 |
| 接线 | 改写 profile package.json + pnpm install 铺 node_modules | **无接线**，内核内建扫描固定根 |
| 物化目标 | 每个内核版本的 plugins/ 目录各自一份 | 所有内核共享同一个 `<DSH_HOME>/skills` → **无按内核物化** |
| 生效时机 | 重启内核（profile 层启动时快照） | 文件监视即时失效重发现 |
| 切换内核 | 补物化 + 改写依赖路径 + pnpm install | **无操作** |

## 内核侧对接面（只读依赖，不改内核）

内核 `dsh-skill-filesystem` 按 rank 升序扫描以下根，rank 小者胜同名：

| Rank | 来源 | 根 | 与本设计的关系 |
| --- | --- | --- | --- |
| 100 | project-dsh | `<projectRoot>/.dsh/skills` | 项目级覆盖全局（壳不写） |
| 200 | project-agents | `<projectRoot>/.agents/skills` | 同上 |
| 300 | custom | `Config.customSkillDirs` | 壳不使用（需改 cordis 配置） |
| 400 | **user-dsh** | **`<DSH_HOME>/skills`** | **壳的物化目标（接线点）** |
| 500 | user-agents | `<agentsHome>/skills` | 用户手放技能，壳只读展示 |
| 600 | bundled | `Config.bundledSkillDir` | 打包技能，壳不涉及 |

壳定位 dsh home 复用 `kernel::data_dir` 已建立的解析顺序（`DSH_HOME` → `~/.dsh`），保证壳写的目录与内核读的目录永远一致。

内核侧约束（壳的校验规则与其对齐，fail loud 在壳这一层完成）：

- 只发现根下**直接一层**：`<root>/<name>/SKILL.md` 或 `<root>/<name>.md`；不支持嵌套递归发现。
- 技能名取自 SKILL.md frontmatter 的 `name`，必须 kebab-case（`^[a-z0-9]+(?:-[a-z0-9]+)*$`）；`description` 必填。缺任一项内核**静默忽略**（仅日志告警）——所以壳必须在安装时预校验，否则用户会看到"装了却不出现"。
- 调用策略 frontmatter：`disable-model-invocation`、`user-invocable`（缺省均 true）。这是作者语义，壳不代改。
- 监视跟随符号链接（followSymlinks 默认开）：link 模式物化的技能同样被热发现；根的直接条目增删、`<dir>/SKILL.md` 内容变化都会触发 `skills/change`，工具消费者在下一个模型步骤前重注入 `<available_skills>` 目录。

## 目录布局

```text
~/.dsh/                              # dsh home（DSH_HOME 可重定向，与内核共用解析顺序）
├── skills-store/                    # 中央技能库（权威副本，壳独占写入）
│   ├── store.json                   # 清单：包条目（来源/版本/mode）+ 每技能条目（名称/enabled/路径）
│   └── <pkg-id>/                    # 一个包的源（npm tarball 解包或 git checkout）
│       ├── .dsh-source.json         # id/来源/版本/拉取时间
│       └── …                        # 包内容，可含一个或多个技能
├── skills/                          # 内核读取的用户级根（user-dsh, rank 400）＝物化视图
│   ├── <skill-name> → ../skills-store/<pkg-id>/<…>/   # link 模式：指向中央库内技能目录
│   └── <skill-name>.md → ../skills-store/<pkg-id>/<…>.md
└── desktop/
    └── skills-catalog.json          # 社区目录缓存（TTL 6 小时，同 plugins-catalog.json）
```

技能 fetch 没有 pnpm 输出可留（git/tar 的失败原因直接进错误消息与进度面板），因此不设 `logs/skill-*.log`，也不需要插件中央库那套 `.npmrc`——技能流程完全不经过 pnpm。

包 id 映射与插件一致：`/` 替换为 `__`（`@ace-zone/dsh-skills` → `@ace-zone__dsh-skills`），拒绝 `..` 与空段。本地文件夹导入以文件夹名为 id，加 `local:` 来源标记。

**安装单位 = 包，物化单位 = 技能。** 这是与插件的关键差异：一个包可含多个技能（如 monorepo 仓库技能散布在子目录），物化时每个技能独立落一条链接，启停粒度也是单个技能。单技能包则整包即一个技能。

## 安装流程

以 npm/git 来源为例（本地文件夹 = 把源路径纳入中央库管理，其余相同）：

1. **fetch 进中央库**：npm 取 `dist-tags.latest`（或指定版本）下载 tarball 用系统 tar 解包；git 深度克隆。写 `.dsh-source.json` 与 store.json。日志落盘 `logs/skill-<id>.log`。
2. **扫描与校验（fail loud）**：在包内探测技能入口——任意目录下的真实 `SKILL.md`（探测深度 ≤3 层，覆盖根即技能与常见 monorepo 布局）及顶层平铺 `*.md`；逐个解析 frontmatter，校验 kebab-case `name` + 非 `description`。符号链接（含目录与 `SKILL.md` 文件）一律不视为技能入口，避免 `git clone` 保留的装饰性重定向（如 blader/humanizer v2.11.1+ 的 `skills/<name>/SKILL.md → ../../SKILL.md`）把同一技能重复计入。一个技能都没有 → 安装失败并给出原因；包内重名（frontmatter name 冲突）→ 整包拒绝。
3. **物化到活动根**：对每个校验通过的技能，在 `<DSH_HOME>/skills/` 建 symlink（Windows junction）指向中央库内的技能目录/文件，条目名 = frontmatter `name`。链接失败自动降级 copy（差异复制，跳过未变化文件），实际模式记入 store.json。
4. **无第 4 步**：不跑 pnpm、不改 profile——插件流程里最重的两步在这里不存在。
5. **生效反馈**：壳探测内核端口是否在监听；运行中提示"已对工作台即时生效"，未运行提示"下次启动自动可用"。

卸载 = 反向执行：拆除该包全部技能的活动根链接、删除中央库目录、更新 store.json。全程无进程重启。

## 启用 / 禁用

- **禁用** = 从活动根摘除该技能的链接（源完好保留在中央库），store.json 记 `enabled: false`；**启用** = 重建链接。
- 不改写 SKILL.md 内容——`disable-model-invocation` 等 frontmatter 是技能作者的语义，壳的状态与之正交。
- 内核 watcher 观察到条目移除/出现后自动失效重发现，运行中的会话在下一步模型请求前看到更新后的目录。

## 切换内核 / 多内核

所有内核版本共享同一个 `<DSH_HOME>/skills`，且技能不进任何内核的 node_modules 解析路径，因此：

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

「检查更新」遍历 store.json 写回结果；UI 徽标与逐行「更新」按钮沿用插件面板。更新 = 原位重拉中央库（`.tmp-*` → `.new-*` → `.backup-*` 三段式替换）→ **重新扫描技能集合并校正物化视图**（新增补链、消失拆链并在进度里列出增删明细，幸存技能保留各自的启用状态）→ 即时生效。本地文件夹来源不做版本检查，但保留手动「重新同步」：改完源文件夹后一键重导并按同样的增删逻辑校正物化视图。

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
| `ui/src/skills.js` + `components/SkillsPanel.vue` | 技能页签：包行「名称/来源/版本（左）+ tag 与更新/仓库/卸载按钮（右）」；不提供单技能启停 UI（卸载即停用，不单独造「启停」概念）；手动安装行复用插件同款输入行与 `withProgress` 进度面板 |
| `settings.rs` | 无新字段（接线点固定） |

frontmatter 校验是内核规则的壳侧前置：解析器只取 frontmatter 顶层 `name` / `description`（带引号去引号），无法解析或不符合 kebab-case 的候选按"内核也会忽略"处理——安装时以警告形式展示并跳过，整包一个可用技能都没有才失败。这比内核的静默忽略更响，避免"装了却不出现"。启动对账 `skills::reconcile()`：清理三段式暂存残留、为启用技能补链/修复断链、清退停用技能的残留、清扫指向中央库但不在清单中的孤儿链接（用户手放的文件与非本库链接一律不动）；失败写入 store.warning 由面板展示。

## 已知取舍

- link 模式省空间且更新直达，但中央库被移动/删除后断链（reconcile 会标出并提示重装）；copy 模式自包含但更新需差异复制。默认策略与插件一致：优先 link，失败降级 copy，UI 明示实际模式。
- 内核 watcher 故障时其观察变为 incomplete，模型保留 last-good 目录继续工作；壳无法从外部感知该状态，技能页提供「重启工作台」兜底文案。
- 包内技能探测深度 ≤3 层是启发式：更深层嵌套的技能不会被识别（内核的直接一层发现本来也不覆盖它们，作者平铺即可）。
- 项目级部署（把已装技能导出到某项目 `.dsh/skills`）留作后续方向：需要项目选择 UI 与覆盖确认，首版不做。
- 由插件 provider 注册或 bundled root 贡献的技能不经文件系统根，不出现在面板；技能页对这些来源仅在文档中说明，不做管理。
- 社区技能目录（按主题/分类浏览 + 一键安装）是插件中心的成熟形态，技能 v1 没复制一份独立目录，只给手动安装行与 GitHub topic 入口链接。等社区中心给技能话题上线稳定 feed，再加回缓存/搜索/分页模式与插件中心同款实现；URL 与 JSON 形状契约已在设计中预留。
