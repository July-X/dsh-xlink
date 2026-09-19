# dsh-xlink 多内核改造开发计划

> 关联设计：[dsh-xlink 多内核数据目录与扩展管理设计](dsh-xlink-multi-kernel-design.md)
>
> 状态：计划稿，尚未开始实现

## 1. 开发原则

这次改造按数据路径、实例生命周期、内核适配和扩展管理分阶段推进。每一阶段都保留可运行的 DSH 流程，旧数据只读兼容至少覆盖一个完整发布周期。本文中的 UI 指用户界面。

依赖关系如下：

```text
P0 数据契约
  └── P1 Shell 状态解耦
        └── P2 实例注册表与生命周期
              └── P3 DSH 适配器
                    ├── P4 插件中央库迁移与实例接线
                    └── P5 共享技能接入
                          └── P6 迁移向导
                                └── P7 mcode 适配器骨架
                                      └── P8 UI、集成测试与发布准备
```

P4 和 P5 在 P3 完成后可以并行开发，但两者都依赖统一的路径解析、实例记录和锁模型。P6 只有在插件与技能的新目标路径能够独立对账后才开始，否则迁移失败很难定位是路径问题还是扩展问题。

## 2. 阶段计划

### P0：冻结路径与数据模型

**目的**：先建立可测试的路径和状态契约，不改变默认行为。

**修改范围**：

- Rust 新增统一路径模块，建议命名为 `xlink_paths.rs` 或 `paths.rs`。
- 定义 `DSH_XLINK_HOME`、默认 `~/.dsh-xlink`、`shell/release`、`shell/dev`、`kernels`、`dsh-plugins`、`skills` 和 `state` 的路径函数。
- 定义 `schema_version`、`InstanceRecord`、`ShellState`、插件清单和技能清单的目标结构。清单使用 JSON（JavaScript 对象表示法）保存。
- 为旧 `data_dir`、`DSH_DESKTOP_DATA_DIR`、`~/.dsh/plugins` 和 `~/.dsh/skills-store` 增加只读 legacy resolver，暂不切换调用方。

**依赖**：无。

**交付物**：

- 路径契约和状态类型。
- 新旧路径对照测试表。
- 设计文档中的目录树、字段和迁移约束落到代码注释或测试名称中。

**测试要求**：

- macOS、Windows 风格路径和 `~` 展开测试。
- `DSH_XLINK_HOME` 覆盖默认值的测试。
- release/dev 只影响 `shell` 路径，不影响 kernel instance 路径的测试。
- 路径穿越、空 id、保留名和重复 schema version 的拒绝测试。

**完成判据**：所有路径生成通过单元测试；旧路径解析仍与当前生产行为一致；没有模块直接拼接新的根目录字符串。

**风险**：如果路径模块仍依赖 `tauri::AppHandle`，纯单元测试会继续困难。路径解析应把环境输入和 OS app-data fallback 作为显式参数，Tauri 只在装配层提供输入。

### P1：Shell 状态与内核数据解耦

**目的**：先让 release/dev 只管理 dsh-xlink 自己的状态。

**修改范围**：

- `src-tauri/src/kernel.rs`、`lib.rs`：停止用 `desktop` / `desktop-dev` 推导内核 home。
- 新建 `shell/release` 与 `shell/dev` 的设置、UI 状态和 Shell 日志读写。
- `settings.rs`：把端口、profile 等字段逐步改为实例配置；保留旧 settings 读取作为迁移输入。
- `process.rs`、日志列表命令和 UI 日志页：区分 Shell 日志与内核实例日志。
- `ui/src/store.js`、设置页和概览页：显示“Shell 模式”和“当前实例”，不再把 dev 文字解释为 dev 内核。

**依赖**：P0。

**交付物**：

- 两种 Shell 构建模式各自有独立 Shell 状态。
- 内核日志的路径接口改为接收 instance id 或实例日志目录。
- release preview、DebugPanel 和 `dev_build` 仍只影响 UI。

**测试要求**：

- release 与 dev 状态文件互不覆盖。
- 两个 Shell 同时读取同一个实例时不会创建第二份内核目录。
- Shell 退出不会根据构建模式清理或杀掉另一份实例。
- 日志列表不会把 Shell 日志和内核日志混在同一个归属中。

**完成判据**：代码中不再用 `cfg!(debug_assertions)` 选择 DSH_HOME、profile、session、storage 或实例端口；该判断只保留在 Shell UI 和 Shell 日志场景。

**风险**：现有命令大量接收 `data_dir`。应先引入新的上下文类型，再逐个收窄接口，不能一次性把字符串路径替换成多个临时参数。

### P2：实例注册表与生命周期

**目的**：用实例记录取代单一 `active.txt` 和 `kernel.pid`。

**修改范围**：

- 新增实例管理模块，建议包含 `InstanceRegistry`、`InstanceLock`、端口分配和状态转换。
- `state/instances.json` 使用原子写入和注册表级锁。
- 每个实例增加 `instance.json` 与 `runtime/{instance.lock,pid,port,status.json}`，其中 pid 表示进程 ID。
- `commands.rs` 的启动、停止、重启、打开工作台、日志和版本操作增加 `instance_id`。
- `notify.rs`、`guard.rs`、`quarantine.rs` 按实例绑定事件流、日志和隔离记录。
- UI 增加实例选择和实例状态列表；先保留“默认实例”作为单实例用户的快捷入口。

**依赖**：P1。

**交付物**：

- 实例创建、删除、启动、停止和健康检查流程。
- 每实例端口分配和锁占用错误。
- 旧 `active.txt` 内容可导入为一个 `default` 实例的版本属性。

**测试要求**：

- 两个实例同时启动，session/storage/logs 不串目录。
- 同一实例被两个 Shell 启动时只有一个进程组成功启动。
- stale pid、进程退出但 status 未更新、端口被外部进程占用时能恢复或给出下一步。
- stop、restart、删除和退出事件只作用于目标实例。
- 并发读写 `instances.json` 不产生截断 JSON。

**完成判据**：启动和停止命令不再读取全局 `active.txt` 或 `kernel.pid` 作为唯一事实；所有进程控制都能从 instance id 追溯到实例目录。

**风险**：旧 UI 和部分 guard 逻辑默认只有一个内核。迁移期间可以保留 `default_instance_id` 兼容入口，但内部必须立即解析为实例记录，不能继续扩大全局单例假设。

### P3：DSH 适配器与独立 `DSH_HOME`

**目的**：把现有 DSH 专有启动和目录知识集中到 `DshAdapter`。

**修改范围**：

- 新增 `kernel_adapter.rs` 或同等模块，定义适配器接口和 capability 类型。
- 将 `kernel.rs` 的 DSH 安装、版本扫描、启动参数、健康检查和日志规则移入 `DshAdapter`。
- 每个 DSH 实例使用 `kernels/dsh/instances/<id>/home` 作为 `DSH_HOME`。
- 按官方结构创建 `profiles/<name>`、`profiles/node_modules`、sessions、storages、attachments、logs、settings 和 credentials。
- 版本安装产物迁移到 `kernels/dsh/versions/<version>`；运行实例不再依赖 Shell 的 `data_dir` 推导路径。
- 保留官方 `cordis.patch.yml`、profile 模板和现有补丁功能，但把补丁状态改成实例或版本明确归属。

**依赖**：P2；需要继续对照官方仓库的 profile 和 home-paths 实现。

**交付物**：

- DSH 适配器的最小可用实现。
- 一个新的默认 DSH 实例可以安装、启动、健康检查、停止和重新打开。
- 官方 profile 目录结构测试夹具。

**测试要求**：

- 实例进程收到正确的 `DSH_HOME`、profile、workspace 和 port。
- 两个 DSH 版本或两个实例使用不同 home 时互不读写 sessions、storages 和 credentials。
- profile `package.json`、`cordis.patch.yml` 和 `pnpm-workspace.yaml` 的创建与回滚测试。
- 现有 Rust 单元测试、`cargo check`、`cargo clippy --all-targets`。

**完成判据**：DSH 适配器可以不依赖 release/dev 判断完成一个完整生命周期；所有 DSH 专有路径只出现在适配器和路径模块中。

**风险**：官方内核目录和配置仍可能变化。适配器需要把官方依赖路径作为可校验的契约，并在版本不兼容时停止接线，不能静默写入未知 profile 字段。

### P4：插件中央库迁移与实例接线

**目的**：把插件中央库移到 `~/.dsh-xlink/dsh-plugins`，并让插件按实例接入 DSH profile。

**修改范围**：

- `plugins.rs`：将中央库、日志和暂存路径切换到 Xlink 根目录。
- 物化目标从旧的 `desktop/kernels/<version>/plugins` 改为实例级 `extensions/plugins/<plugin-id>`。
- profile 接线改为通过 `DshAdapter` 操作指定实例，不再从一个全局活动版本推导 profile。
- 插件清单拆分全局源信息与实例接线信息；保留 link/copy 模式、内容摘要和来源校验。
- `commands.rs` 和 UI 的安装、更新、卸载、同步命令增加实例范围，显示需要重启的实例。
- `guard.rs`、`quarantine.rs`、启动修复和日志路径按实例改造。

**依赖**：P3。

**交付物**：

- `dsh-plugins/store.json` 和新源库布局。
- 一个插件可接入多个 DSH 实例，也可按版本兼容性只接入一个实例。
- profile 接线失败时恢复旧 manifest 和旧物化目录。
- 旧 `~/.dsh/plugins` 的只读导入入口，供 P6 调用。

**测试要求**：

- 插件安装、更新、卸载不会修改未选中的实例。
- link/copy 两种模式下 profile 依赖和 bundle 列表一致。
- 同名插件、跨来源 id 冲突、断链、孤儿物化和部分卸载都能对账。
- 插件更新期间某个实例运行，不会把半套 profile 暴露给内核。
- Windows junction、普通文件路径、路径分隔符和权限失败都有测试或明确降级。

**完成判据**：插件中央源库不再位于 `~/.dsh/plugins`；所有 Xlink 写入的 DSH plugin dependency 都能通过实例 id 定位；旧的单一 active kernel 接线函数被删除或只保留兼容包装。

**风险**：插件安装可能耗时较长且跨多个实例。网络取源不应持有注册表锁，实例接线必须按实例串行，并在进度和日志中记录 instance id。

### P5：共享技能接入

**目的**：把通用技能统一放入 `~/.dsh-xlink/skills`，通过官方 `customSkillDirs` 供 DSH 使用。

**修改范围**：

- `skills.rs`：将中央源库从 `skills-store` 改为 `skills/packages`，活动根改为 `skills/active`。
- 保留 frontmatter 校验、技能名冲突、link/copy、内容指纹和 watcher 对账能力。
- `DshAdapter` 为每个实例接入 `customSkillDirs`，保留官方默认 roots。
- 技能启停和更新命令改为全局共享语义，UI 明确提示会影响已接入的实例。
- 未来 mcode 的技能能力通过适配器声明，不在 `skills.rs` 中写内核名称判断。

**依赖**：P3；P4 可并行，但共享状态写入应使用统一状态模块。

**交付物**：

- `skills/store.json`、`skills/packages`、`skills/active` 的新布局。
- DSH 运行实例能发现共享技能；内容变化、启用和禁用符合 watcher 行为。
- 旧 `~/.dsh/skills-store` 和 `~/.dsh/skills` 的只读导入入口。

**测试要求**：

- DSH 通过 `customSkillDirs` 看到活动技能，且官方 `~/.dsh/skills` 或实例 home 中的手工技能不被清理。
- 运行中的两个实例都能收到共享技能变更。
- copy 模式通过内容指纹区分 Xlink 文件和用户手工文件。
- 技能包为空、frontmatter 无效、技能重名、断链和更新后技能集合变化都有测试。

**完成判据**：技能管理不再依赖 `DSH_HOME` 推导中央库；所有共享技能路径都可从 Xlink 根目录和 store 记录追溯；没有为了接入技能而修改官方内核源码。

**风险**：共享活动视图会影响所有 DSH 实例。首版必须在 UI 中显示影响范围，并在更新失败时保留旧活动视图；实例级覆盖作为后续需求，不在本阶段偷偷加入。

### P6：迁移向导与恢复流程

**目的**：让旧版用户安全地把 Shell 状态、插件、技能和选择的 DSH 数据导入新布局。

**修改范围**：

- 新增 migration 模块和 `state/migrations/<migration-id>/` 清单。
- 识别 `~/.dsh/desktop`、`desktop-dev`、`plugins`、`skills-store`、`skills` 以及用户选择的 DSH home 文件。
- 增加预览、备份、复制、校验、继续、取消和回滚命令。
- UI 增加迁移向导，明确提示 `~/.dsh` 可能被官方 CLI（命令行界面）使用，不执行整体搬迁。
- 凭据文件单独确认，复制时保留权限；失败时不能清理旧源。

**依赖**：P4、P5、P2。

**交付物**：

- 幂等迁移流程和可读的迁移报告。
- 旧插件和技能导入后的内容摘要、来源和启用状态。
- 一个可启动的新 DSH 实例作为迁移验证目标。

**测试要求**：

- 预览阶段不写旧目录。
- 复制中断、磁盘空间不足、目标文件冲突和权限失败可继续或回滚。
- 重复迁移不覆盖目标中用户后来修改的文件。
- 迁移前后插件数量、技能数量、profile 接线和健康检查结果可比较。
- 旧官方 DSH 独立运行时，迁移不删除、不改写其目录。

**完成判据**：用户确认前没有破坏性动作；用户取消后旧目录保持可用；迁移成功后默认 Shell 指向新实例，日志中包含 migration id。

**风险**：凭据和会话数据可能很大，也可能被其他程序使用。首版可以只迁移 Shell、插件和技能，把会话和 credentials 作为明确勾选项，不能在后台复制后再解释。

### P7：mcode 适配器骨架

**目的**：验证通用实例模型确实能容纳第二种内核，并把不支持的能力显式化。

**修改范围**：

- 定义 mcode 的 adapter descriptor、版本来源、启动参数、健康检查和日志能力。
- 新增 `kernels/mcode/versions` 与 `instances` 的路径测试。
- UI 实例列表按内核族展示，不把 DSH 专用字段硬编码到通用卡片。
- 插件和技能能力使用 capability 判断：不支持 profile wiring、customSkillDirs 或热更新时返回可操作提示。
- 先实现 mock 或 dry-run adapter，是否接入真实 mcode CLI 由独立需求决定。

**依赖**：P6；P3 的适配器接口已经稳定。

**交付物**：

- 可创建、查看和删除 mcode 类型实例的骨架。
- DSH 与 mcode 并行存在时，注册表、端口分配、锁和日志归属不冲突。
- 能力不支持时的 UI 和错误模型。

**测试要求**：

- mock DSH adapter 与 mock mcode adapter 同时运行。
- 相同版本字符串在不同内核族下不会碰撞。
- mcode 未安装、版本不可用和能力缺失都不会影响 DSH 实例。
- 通用实例管理代码不出现 `if kernel == dsh` 的路径分支。

**完成判据**：添加第二个适配器只需要实现适配器接口和能力声明，不需要修改中央插件、技能和注册表的基本算法。

**风险**：mcode 的真实目录和扩展协议尚未确认。这个阶段只固定 Xlink 的接口，不伪造 mcode 的运行契约；真实协议确认后再增加实现。

### P8：UI、集成测试与发布准备

**目的**：把新数据模型交付到可使用的 Shell，并清理兼容期中的文档和运维问题。

**修改范围**：

- UI：实例列表、实例创建、启动/停止、工作台入口、端口、日志、插件实例范围和技能全局范围。
- `ui/src/bridge.js`：所有实例命令携带 instance id，长任务继续使用 `withProgress` 和 loading 状态。
- `ui/src/store.js`、`plugins.js`、`skills.js`：状态按实例或全局源库拆分，不再用一个 active kernel 状态承载所有数据。
- `README.md`、`docs/architecture.md`、插件/技能管理文档和故障排查文档同步当前已实现行为。
- 增加 fresh install、升级、迁移、双实例、release/dev 并行和失败恢复的集成测试。

**依赖**：P7；P4、P5、P6 的接口和状态格式稳定。

**交付物**：

- 新用户默认使用 `~/.dsh-xlink`。
- 单实例用户不需要理解实例 id 也能完成安装和启动。
- 多实例用户可以明确选择目标实例，不会误停或误删另一个实例。
- release/dev 的文案和日志准确表示 Shell 构建模式。
- 发布包、升级器、日志收集和文档不再假设 `~/.dsh/desktop*` 是新默认路径。

**测试要求**：

- `npm run build:ui`。
- `cargo fmt --check`、`cargo check`、`cargo clippy --all-targets` 和 Rust 测试。
- 代码预算检查和已有 UI 回归测试。
- 至少一次真实的本地双实例启动与停止；没有真实 mcode 时使用 mock adapter，不把 mock 结果写成 mcode 已支持。
- 迁移后重启 Shell、重启实例和升级 Shell 都要重新验证路径、锁、日志和 updater 行为。

**完成判据**：设计文档中的验收标准全部有自动化或手工证据；新默认路径、迁移入口、失败恢复和旧路径兼容策略在 README 与故障排查文档中一致。

**风险**：数据目录变化会影响升级和支持排障。发布前必须保留旧路径只读探测和明确迁移日志，至少等一版稳定发布后再考虑移除旧兼容代码。

## 3. 跨阶段的模块接口

为了让模块有足够的深度，调用方只应依赖下面几类接口：

| 模块 | 接口应隐藏的实现细节 | 主要调用方 |
| --- | --- | --- |
| `XlinkPaths` | 默认根、Shell 路径、实例路径、legacy 路径和平台差异 | 所有 Rust 模块 |
| `InstanceRegistry` | JSON 读写、schema 升级、默认实例和并发锁 | 命令层、UI 状态、迁移 |
| `InstanceManager` | 状态转换、端口、pid、进程组、健康检查和日志归属 | Tauri 命令、通知、看护 |
| `KernelAdapter` | 某种内核的安装、profile、启动参数、扩展能力和日志 | InstanceManager、插件、技能 |
| `PluginStore` | 下载、校验、中央源库、版本和实例接线计划 | 插件命令与 UI |
| `SkillStore` | 技能扫描、frontmatter、活动视图和所有权凭据 | 技能命令与 UI |
| `MigrationPlan` | 预览、备份、复制、摘要、恢复和迁移报告 | 迁移命令与 UI |

这些接口的测试面就是各模块的主要验收面。插件和技能不应再次各自实现一份根目录解析、原子状态写入或来源校验。

## 4. 发布门槛与回滚

每个阶段合并前都要保留一个可启动的 DSH 实例。出现问题时按下面的顺序回滚：

1. 停止受影响的实例，保留实例目录、runtime 状态和日志。
2. 恢复 profile manifest、插件物化目录或技能活动视图的上一份原子快照。
3. 保留迁移清单和错误日志，不删除旧源数据。
4. 如果是路径解析问题，使用 `DSH_XLINK_HOME` 指向隔离测试目录复现，不直接改用户的 `~/.dsh`。
5. 只有确认旧路径和新路径都可恢复后，才推进下一阶段。

最终发布前需要检查：

- package 与 Tauri 版本一致，现有发布平台和 updater 流程没有把新目录当作安装目录。
- README、架构、插件、技能和故障排查文档中的路径一致。
- 日志中可以区分 `shell_mode`、`kernel_family`、`kernel_version` 和 `instance_id`。
- 没有把 `~/.dsh` 整体迁移或删除的脚本，也没有通过宽泛进程匹配杀掉用户的官方 DSH。
