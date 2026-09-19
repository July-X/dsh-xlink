# dsh-xlink 多内核数据目录与扩展管理设计

> 状态：设计稿，尚未实现
>
> 日期：2026-09-19

本文把 dsh-xlink 定义为一个可以管理多个内核的桌面 Shell。本文中的 DSH 指 DeepSeek Harness。当前实现主要围绕一个 DSH 内核工作，外壳的 release/dev 构建模式也参与了内核数据目录选择。新设计会把这两件事拆开：release 和 dev 只描述 dsh-xlink Shell，内核、版本和运行实例由独立的数据模型管理。

## 1. 结论

默认数据根目录改为：

```text
~/.dsh-xlink/
```

可通过 `DSH_XLINK_HOME` 覆盖。`DSH_HOME` 保留给具体内核使用，不再承担 Xlink 外壳根目录的含义。

目录职责如下：

| 目录 | 所有者 | 用途 |
| --- | --- | --- |
| `shell/release/` | dsh-xlink Shell | 正式版 Shell 的设置、UI（用户界面）状态和 Shell 日志 |
| `shell/dev/` | dsh-xlink Shell | `tauri dev` 的设置、UI 状态和开发日志 |
| `dsh-plugins/` | dsh-xlink 扩展管理 | DSH 插件中央源库 |
| `skills/` | dsh-xlink 扩展管理 | 可被多个内核使用的技能源库和活动视图 |
| `kernels/` | 内核实例管理 | 内核版本、实例、每个实例的官方数据目录 |
| `state/` | dsh-xlink Shell | 实例注册表、迁移记录和全局状态 |
| `cache/` | dsh-xlink Shell | 下载和 registry 缓存，可随时重建 |

`release` 和 `dev` 不再出现在 DSH 内核的 `DSH_HOME`、profile、session、storage、port 或 pid（进程 ID）路径中。

## 2. 目标与非目标

### 目标

1. 默认情况下，Xlink 管理的数据都落在 `~/.dsh-xlink`，插件和通用技能不再依附于用户的官方 `~/.dsh`。
2. 每个运行实例拥有独立的内核数据、profile、会话、存储、附件、日志、端口和运行状态。
3. 多个 DSH 实例可以同时运行。未来的 mcode 实例也使用同一套实例管理流程。
4. 保留官方 DSH 的目录语义。Xlink 通过适配器（adapter）配置和启动内核，不复制或改造内核本身。
5. 插件源库与实例 profile 接线分离。技能源库与内核读取路径分离。
6. 旧版数据迁移可预览、可备份、可中断恢复，不自动删除用户原来的 `~/.dsh`。

### 非目标

- 本阶段不实现 mcode CLI，也不假设 mcode 已经提供与 DSH 完全相同的插件或技能接口。
- 不把 `~/.dsh/plugins` 设计成官方 DSH 的自动扫描目录。官方 DSH 没有这个约定。
- 不把 release/dev 继续包装成两种内核模式。
- 不在没有用户确认的情况下复制凭据、删除旧目录或覆盖用户手工维护的 profile。
- 不要求一次性重写现有插件和技能的全部业务逻辑。迁移会先保留取源、校验、原子写入和安全解包等已有能力。

## 3. 当前实现与官方目录事实

### 3.1 当前 dsh-xlink 的数据布局

当前实现以 `DSH_HOME` 或 `~/.dsh` 为基准，再追加 Shell 构建模式目录：

```text
~/.dsh/
├── desktop/                      # release Shell
│   ├── kernels/<version>/
│   ├── logs/
│   ├── settings.json
│   ├── active.txt
│   └── kernel.pid
├── desktop-dev/                  # dev Shell
├── plugins/                      # 插件中央库
├── skills-store/                 # 技能中央库
└── skills/                       # 技能活动根
```

这套布局有两个问题：

1. `desktop` 和 `desktop-dev` 把 Shell 构建模式带进了内核数据路径，导致同一个内核、profile 和会话在两个 Shell 之间出现两套状态。
2. `active.txt` 和 `kernel.pid` 默认表达单一活动内核，无法表示多个实例同时运行；插件和技能又依赖外壳推导出来的 `data_dir`。

### 3.2 官方 DSH 的目录契约

本设计依据官方仓库在 2026-09-19 检查到的 commit `ddefc45fbc7f8e46dd73185e68295696d1297887`。相关实现如下：

- [`home-paths`](https://github.com/deepseek-ai/deepseek-harness/blob/ddefc45fbc7f8e46dd73185e68295696d1297887/packages/util/home-paths/src/index.ts) 将默认 `DSH_HOME` 解析为 `~/.dsh`，并允许通过 `DSH_HOME` 覆盖。
- [`profile.ts`](https://github.com/deepseek-ai/deepseek-harness/blob/ddefc45fbc7f8e46dd73185e68295696d1297887/packages/boot/app-boot/src/profile.ts) 使用 `$DSH_HOME/profiles/<name>`。一个 profile 至少包含 `package.json`、`cordis.patch.yml`、`pnpm-workspace.yaml` 和自己的 `node_modules`；`$DSH_HOME/profiles/node_modules` 是官方共享依赖回退目录。
- 官方 base profile 默认使用 `$DSH_HOME/sessions`、`$DSH_HOME/storages`、`$DSH_HOME/attachments/v1`、`$DSH_HOME/logs`、`$DSH_HOME/settings.yaml`、`$DSH_HOME/.credentials.yaml` 和 `$DSH_HOME/cordis.patch.yml`。
- 官方没有把 `$DSH_HOME/plugins` 作为普通插件运行入口。插件通过 profile 的 package dependencies、`dsh.profile.bundles` 和 pnpm 接线。
- [`dsh-skill-filesystem`](https://github.com/deepseek-ai/deepseek-harness/blob/ddefc45fbc7f8e46dd73185e68295696d1297887/packages/skill/skill-filesystem/src/index.ts) 支持 `customSkillDirs`、`includeDefaultRoots`、`dshHome` 和文件监视。技能入口是 `<name>/SKILL.md` 或 `<name>.md`。

由此得到一个硬约束：Xlink 管理的 DSH 实例必须为每个实例设置独立的 `DSH_HOME`。不能把 `~/.dsh-xlink` 直接作为所有内核的 `DSH_HOME`，也不能把 `~/.dsh/plugins` 当作官方插件扫描入口。

## 4. 术语与层次

| 术语 | 定义 |
| --- | --- |
| Shell 构建模式 | dsh-xlink 自己的 `release` 或 `dev`。它影响 UI、Shell 设置和日志，不影响内核数据格式。 |
| 内核族 | `dsh`、未来的 `mcode` 等内核类型。每个内核族由一个适配器负责。 |
| 内核版本 | 某个内核族可安装的版本或构建产物。版本本身不承载运行中的会话。 |
| 内核实例 | 一个可独立启动、停止和连接的运行单元，有自己的 `DSH_HOME`、profile、workspace、端口和 pid。 |
| profile | 官方内核加载插件和 patch 的配置目录。一个实例可以有多个 profile，但当前 UI 先管理一个默认 profile。 |
| 中央源库 | Xlink 下载并校验后的插件或技能源文件，属于 Xlink，不属于任何一个实例。 |
| 活动视图 | 从中央源库物化出来、供内核读取的目录。技能使用共享活动视图，插件使用实例级活动视图。 |
| 适配器 | 把 Xlink 的通用实例生命周期映射到具体内核的模块。DSH 适配器是第一份实现，mcode 适配器以后加入。 |

实例是新的管理单位。UI 中的“当前内核”应逐步改成“当前实例”，版本选择只是创建或更新实例时使用的一个属性。

## 5. 目标目录树

```text
~/.dsh-xlink/
├── xlink.json                         # 格式版本、创建时间、可选迁移标记
├── shell/
│   ├── release/
│   │   ├── settings.json              # Shell 设置；不包含 DSH_HOME 内容
│   │   ├── ui-state.json              # 面板、窗口和默认实例选择
│   │   └── logs/                      # Shell 自身日志
│   └── dev/
│       ├── settings.json
│       ├── ui-state.json
│       └── logs/
├── dsh-plugins/
│   ├── store.json                     # 插件源、版本、来源和校验信息
│   ├── packages/<plugin-id>/          # 插件中央源库
│   └── logs/                          # 取源、依赖安装和接线日志
├── skills/
│   ├── store.json                     # 技能包和技能条目状态
│   ├── packages/<package-id>/         # 技能源包
│   └── active/                        # 共享活动视图，customSkillDirs 指向这里
│       ├── <skill-name>/              # 链接或复制的目录技能
│       └── <skill-name>.md            # 链接或复制的平面技能
├── state/
│   ├── instances.json                 # 所有内核实例的注册表
│   ├── migrations/                    # 迁移记录、备份清单和恢复状态
│   └── locks/                         # 注册表级锁；实例锁放在实例目录
├── kernels/
│   ├── dsh/
│   │   ├── versions/<version>/        # DSH 安装产物和版本元数据
│   │   └── instances/<instance-id>/
│   │       ├── instance.json          # 内核族、版本、profile、workspace、端口等
│   │       ├── home/                  # 传给 DSH 进程的 DSH_HOME
│   │       │   ├── profiles/<name>/
│   │       │   ├── profiles/node_modules/
│   │       │   ├── sessions/
│   │       │   ├── storages/
│   │       │   ├── attachments/v1/
│   │       │   ├── logs/
│   │       │   ├── settings.yaml
│   │       │   ├── .credentials.yaml
│   │       │   └── cordis.patch.yml
│   │       ├── extensions/plugins/<plugin-id>/  # 实例级链接或复制视图
│   │       ├── workspace/              # 默认 workspace；也可配置为外部目录
│   │       └── runtime/
│   │           ├── instance.lock       # 单实例控制锁
│   │           ├── pid                 # 当前进程 pid
│   │           ├── port                # 当前监听端口
│   │           └── status.json         # 最后一次生命周期状态
│   └── mcode/
│       ├── versions/
│       └── instances/
└── cache/
    ├── downloads/
    └── registry/
```

`versions/<version>` 是安装产物缓存，不是运行时数据目录。会话、凭据、profile 和日志只属于 `instances/<instance-id>/home`。具体内核可以在版本目录中使用自己的包布局，但不能把运行状态写回另一个实例。

`instance.json`、`instances.json`、插件 `store.json` 和技能 `store.json` 都带 `schema_version`。这些 JSON（JavaScript 对象表示法）文件写入时使用临时文件、`sync_all` 和原子替换。临时文件命名和清理规则沿用现有状态写入约定。

## 6. Shell 与内核的隔离

### 6.1 release/dev 的新定义

`release` 和 `dev` 只由 dsh-xlink Shell 的构建信息决定：

- release Shell 读取 `shell/release/`。
- `tauri dev` 读取 `shell/dev/`。
- DebugPanel、开发提示和 release 预览只改变 UI，不改变内核实例。
- Shell 日志可以保留 `release` / `dev` 标记，便于排查外壳问题。
- 内核日志写入对应实例的 `home/logs/`，不再按 Shell 构建模式分槽。

两个 Shell 可以同时打开。它们共享实例注册表和扩展源库，但每个实例的启动、停止、接线和迁移都必须经过实例锁。一个 Shell 不能因为退出或点击“关闭工作台”而杀掉另一个 Shell 正在运行的实例。

### 6.2 端口不再代表构建模式

端口属于 `instance-id`。新实例创建时从 Xlink 的可用端口范围分配，分配前同时检查注册表、实例锁和实际监听状态。

旧版的 release `3090`、dev `3091` 只作为迁移旧实例时的兼容输入，不再作为“release 内核”和“dev 内核”的定义。新设计可以把 `3090` 作为第一个默认 DSH 实例的起始端口，后续实例按可用端口递增。

### 6.3 默认实例而不是全局 active 指针

Xlink 保留“默认实例”概念，但它是 Shell 设置中的 `default_instance_id`，不是全局 `active.txt`。每个 Shell 构建模式可以记住自己的默认实例，实例本身仍由 `instances.json` 统一登记。

不再使用单一的全局 `kernel.pid`。每个实例有自己的 `runtime/pid` 和 `runtime/status.json`；pid 文件只作为启动后的辅助证据，最终状态必须通过进程检查和健康检查确认。

## 7. DSH 实例与适配器

### 7.1 DSH 实例启动环境

DSH 适配器启动实例时至少设置：

```text
DSH_HOME=<xlink_home>/kernels/dsh/instances/<instance-id>/home
profile=<instance.json 中的 profile>
port=<instance.json 中的 port>
workspace=<instance.json 中的 workspace>
```

DSH 进程继续按官方规则解析 `profiles/<name>`、sessions、storages、attachments、logs、settings 和 credentials。Xlink 只负责准备实例 home 和 profile，不把 Shell 的设置文件混入官方 YAML 文件。

### 7.2 适配器接口的职责

通用实例管理模块只依赖适配器接口，不直接调用 DSH 专有路径。第一版建议覆盖这些操作：

| 操作 | 作用 |
| --- | --- |
| `describe` | 返回内核族、能力和版本信息 |
| `install_version` | 安装或校验一个版本，写入 `versions/` |
| `prepare_instance` | 创建实例 home、profile、workspace 和运行目录 |
| `reconcile_instance` | 根据实例配置接线插件和技能 |
| `start` / `stop` | 启动、停止并回收该实例进程组 |
| `health` | 检查端口、HTTP（超文本传输协议）/API（应用程序接口）和进程状态 |
| `logs` | 返回内核日志位置和读取规则 |
| `capabilities` | 声明插件、技能、热更新和配置能力 |

DSH 适配器负责官方 profile 的 package dependencies、`dsh.profile.bundles` 和 pnpm 安装。实例管理模块负责锁、pid、端口、状态写入和 UI 进度。这样将来增加 `McodeAdapter` 时，不需要在 UI 和通用生命周期代码中散落 mcode 判断。

## 8. 插件设计

### 8.1 中央库

`~/.dsh-xlink/dsh-plugins/` 是 Xlink 管理的 DSH 插件中央库：

```text
~/.dsh-xlink/dsh-plugins/
├── store.json
├── packages/<plugin-id>/
│   ├── package.json
│   ├── .dsh-source.json
│   └── node_modules/
└── logs/
```

插件源下载、tarball 完整性校验、Git 来源回退、包 id 冲突检查、pnpm 日志和原子发布继续沿用当前实现的安全规则，只改变路径解析和归属关系。

这个目录不是官方 DSH 的扫描目录。插件要对某个实例生效，DSH 适配器需要：

1. 在实例的 `extensions/plugins/<plugin-id>` 建立 link 或 copy 视图。
2. 修改该实例 profile 的 `package.json`，写入由 Xlink 所有的 dependency spec。
3. 按插件声明维护 `dsh.profile.bundles`，保留 profile 模板和用户未托管条目。
4. 在该实例 profile 中运行 pnpm，使 `node_modules` 和 bundle 接线一致。

插件源是全局共享的，启用状态和接线结果按实例记录。这样同一个插件可以接入两个不同版本的 DSH，也可以因为版本兼容性只接入其中一个实例。新建实例时是否继承全部已安装插件，由 UI 显示确认，不隐式修改正在运行的实例。

插件更新后的默认流程是“更新中央源库，校验版本，逐个重算实例接线，提示需要重启的实例”。运行中的 DSH 不做 profile 热替换；失败时保留旧 profile 和旧物化目录，不能留下半套 bundle 配置。

### 8.2 与官方 DSH 的关系

官方 DSH 看到的是实例 profile 中的正常 package dependency 和 bundle 配置。`dsh-plugins` 只是 Xlink 的来源管理层，不能要求官方 DSH 增加一个 `plugins` 扫描约定。

## 9. 技能设计

### 9.1 共享源库与活动视图

`~/.dsh-xlink/skills/` 是跨 DSH、未来也可跨 mcode 的通用技能根目录：

```text
~/.dsh-xlink/skills/
├── store.json
├── packages/<package-id>/
└── active/
    ├── <skill-name>/SKILL.md
    └── <skill-name>.md
```

`packages/` 保存经过校验的源包，`active/` 是供内核读取的活动视图。启用和禁用只改变 `active/` 中的条目，源包不删除。目录链接失败时可以复制，并在清单中记录实际模式和内容指纹。

### 9.2 DSH 接入方式

DSH 适配器在实例配置中把 `~/.dsh-xlink/skills/active` 加入 `customSkillDirs`，同时保留官方默认技能根，等价于在官方 `dsh-skill-filesystem` 上增加一个 Xlink 管理的来源。

技能校验仍按官方约定执行：

- 目录技能使用 `<name>/SKILL.md`。
- 平面技能使用 `<name>.md`。
- frontmatter 至少有合法 kebab-case `name` 和 `description`。
- Xlink 不改写技能作者的 `disable-model-invocation`、`user-invocable` 等字段。

v1 的启用状态是全局共享的。修改 `active/` 会影响所有已经接入该目录的 DSH 实例，运行中的实例由文件监视器发现变更。实例级技能覆盖留到后续版本，避免第一版同时维护全局清单、实例清单和冲突优先级。

### 9.3 与插件的差异

| 项目 | 插件 | 技能 |
| --- | --- | --- |
| 源库存储 | `dsh-plugins/` | `skills/packages/` |
| 生效视图 | 每个实例一份 | 所有实例共享 `skills/active/` |
| 官方接入 | profile dependency、bundle、pnpm | `customSkillDirs` |
| 运行中更新 | 通常需要重启实例 | 依赖内核 watcher，可即时重新发现 |
| 状态粒度 | 源库全局，接线按实例 | v1 全局启停 |

## 10. 多实例生命周期与并发

### 10.1 实例状态

实例状态建议至少包括：`created`、`installing`、`stopped`、`starting`、`running`、`stopping`、`failed` 和 `removing`。状态写入 `instance.json` 或 `runtime/status.json`，不以某个文件是否存在作为唯一判据。

一次启动操作必须：

1. 获取实例锁并重新读取实例记录。
2. 校验版本、profile、workspace、端口和 DSH_HOME。
3. 执行插件接线和技能路径检查。
4. 启动该实例进程组，写入 pid 和端口。
5. 通过健康检查确认工作台可用后再向 UI 返回成功。

停止、重启、删除和 profile 接线使用同一实例锁。不同实例之间不共享生命周期锁，因此两个实例可以并行安装或启动，但共享的 registry/store 写入仍使用各自的商店写锁。

### 10.2 进程与日志

- Shell 日志写入 `shell/<release|dev>/logs/`。
- DSH 内核日志写入实例 `home/logs/`，日志名称可以包含实例 id。
- pid、端口和健康状态只写入实例 `runtime/`。
- Xlink 退出时按实例注册表回收自己启动的进程组，不扫描并杀掉所有 Node 进程。
- 一个实例已被另一个 Shell 管理时，第二个 Shell 只允许查看状态、打开工作台或请求停止，并在需要控制时等待或报告锁占用。

## 11. 旧数据迁移

`~/.dsh` 可能仍被官方 DSH CLI（命令行界面）使用，因此不能把整个目录直接移动、重命名或删除。迁移是一个显式的导入流程，默认只读旧数据。

### 11.1 可识别的旧来源

```text
~/.dsh/desktop/        # 旧 release Shell 数据
~/.dsh/desktop-dev/    # 旧 dev Shell 数据
~/.dsh/plugins/        # 旧插件中央库
~/.dsh/skills-store/   # 旧技能中央库
~/.dsh/skills/         # 旧技能活动根
~/.dsh/profiles/       # 可能仍被官方 DSH 使用的 profile
~/.dsh/sessions/       # 可能包含用户历史会话
```

### 11.2 迁移步骤

1. 预览：列出路径、文件数量、版本、插件、技能、运行中的 pid 和待复制的凭据文件。
2. 备份：在 `state/migrations/<migration-id>/` 写清单和校验信息；必要时将用户选择的旧目录复制到备份位置。
3. 导入：插件源进入 `dsh-plugins/packages/`，技能源进入 `skills/packages/`，活动技能按校验结果进入 `skills/active/`。
4. 创建实例：根据用户选择把旧 DSH home 复制到一个新的 `kernels/dsh/instances/<id>/home`。复制 `.credentials.yaml` 前必须单独确认并保持权限。
5. 校验：检查 JSON、profile dependency、技能 frontmatter、路径归属、版本和内容摘要。
6. 验证：以新实例启动一次健康检查，确认旧目录未被修改，再把该实例写入 Shell 的 `default_instance_id`。
7. 收尾：旧目录保留。清理旧数据是独立动作，至少等一次完整版本升级后再提供。

迁移过程可以中断并重试。重复执行时按迁移清单和摘要跳过已完成文件，不覆盖用户在目标目录中后来修改的内容。

## 12. 安全与兼容性约束

- `DSH_XLINK_HOME` 只决定 Xlink 根目录；`DSH_HOME` 只由具体内核进程使用。两者不能互相回退，避免一个环境变量改变另一个系统的归属。
- 新建目录默认使用用户私有权限；Windows 使用等价的用户 ACL。
- 所有 id、版本路径、技能名和 profile 名都做路径穿越校验。
- 插件 tarball、Git 归档和技能包继续执行完整性校验、归档路径约束和体积限制。
- 删除插件或技能前先确认 Xlink 所有权。没有链接目标、内容摘要或清单记录作为证据时，不删除用户手工文件。
- profile 写入失败时恢复旧 manifest；运行实例不接受半成品配置。
- `instances.json` 和商店清单采用 schema version，未来迁移只升级已知版本。
- mcode 不支持某项能力时返回明确的 capability 错误，不通过伪造 DSH 目录来绕过适配器。

## 13. 设计验收标准

实现完成后至少满足以下条件：

1. 全新环境只创建 `~/.dsh-xlink`，release 和 dev 的 Shell 状态分别位于 `shell/release` 与 `shell/dev`。
2. release 与 dev 同时运行时，不会互相覆盖设置、杀掉实例或生成第二份 DSH home。
3. 两个 DSH 实例可以同时运行，各自拥有独立的 `DSH_HOME`、profile、session、storage、日志、端口和 pid。
4. 插件只从 `dsh-plugins` 取源，接入哪个实例由实例 profile 明确记录；删除一个实例不会删中央插件源或其他实例的接线。
5. 技能从 `skills/active` 被 DSH 通过 `customSkillDirs` 发现；启停和内容更新不会误删用户官方技能根。
6. 没有全局 `active.txt` 或单一 `kernel.pid` 参与实例生命周期判断。
7. 迁移预览不会写旧目录；迁移中断后可以继续，失败时能从备份和旧路径恢复。
8. 没有 mcode 安装时，DSH 适配器和现有 UI 流程仍可单独运行；加入 mcode 适配器不需要修改插件和技能中央库的基本模型。
9. 文档、日志和错误信息能明确区分 Shell 构建模式、内核族、版本和实例 id。

这份文档只定义目标架构。当前代码仍使用旧的 `~/.dsh/desktop*`、`~/.dsh/plugins` 和 `~/.dsh/skills-store` 路径，必须完成开发计划后才会切换默认行为。
