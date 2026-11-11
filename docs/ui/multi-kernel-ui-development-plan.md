# 多内核界面设计与开发计划

状态：2026-11-08 已按本设计实现 T01–T09 的代码与测试并同步 T10 文档（实现状态见文末 §6）；**实机验收未完成**，第 5 节的门禁仍待逐条执行，MiniMax 实验能力不得据此宣称可用。第 1–3 节仍是目标设计。配套 [交互设计稿](multi-kernel-interaction-draft.html) 可直接用浏览器打开，无需启动应用；[静态预览](multi-kernel-ui-preview.png)展示内核版本页。设计稿从「设置」开始，使用模拟数据；侧栏、体验开关、内核查看、能力开关和授权说明可操作。「概览 → 系统健康 → 日志系统」可查看独立日志窗口示意。

## 1. 设计约定

- 保留两个各自独立的工作台及一套管理界面。只有管理对象随内核改变时才使用页面内的内核选择；插件、技能、记忆等公共能力使用紧凑的目标开关。
- 「MiniMax 内核体验」位于设置页「工作台」区，控制相关入口的显示。它不代表已安装、已启动、能力已生效或已授权。
- 关闭体验入口不会停止工作台、删除数据、改变各能力的选择或撤销授权。若工作台仍在运行或存在授权，确认框说明仍可能生效，提供取消和「仅隐藏入口」。状态无法确认时不能显示成已停止或无授权。
- 页面结构沿用 1040 × 748 固定窗口、157 px 侧栏、现有明暗主题和账页布局。MiniMax 与 DeepSeek 两个内核品牌词统一使用手写体：MiniMax 用现有实验色粉色，DeepSeek 用官方品牌蓝 #4D6BFE；中文说明仍用正文字体。短状态放行内，条件和生效时机放可聚焦的 ⓘ。
- 正式壳无保存偏好时默认关闭，开发壳默认开启；用户保存的选择优先，入口显示不受构建变量限制。MiniMax 实验能力的实机验收仍需通过第 5 节门禁。

## 2. 页面与状态

| 页面 | 体验开启 | 体验关闭 |
| --- | --- | --- |
| 设置 | 在「工作台」中显示体验开关和两套端口。端口保存结果按内核分别反馈。 | 保留体验开关和 DSH 端口；隐藏 MiniMax 端口，不提交隐藏字段。 |
| 概览 | 页头的本地分段控件查看两套工作台状态；仍只有一张「当前内核」卡。系统健康前两项随所选内核变化，壳级日志、快照不变。今日用量标明 DSH 来源。 | 隐藏分段控件，以 DSH 数据展示；不停止 MiniMax，也不改掉用户之前的页面选择。 |
| 内核版本 | 一张卡内切换内核，加载各自版本与动作；磁盘用量是共用区域，保留原有四类及只读下钻，能归属内核的条目明确标记 DSH 或 MiniMax。 | 隐藏分段控件与 MiniMax 版本操作；共用磁盘统计不随开关或所选内核变化。已有 MiniMax 数据仍标明来源。 |
| 插件 | 一套列表，支持的内置插件逐项显示 DSH / MiniMax 目标状态。社区插件和不预计接入的内置插件**不显示 MiniMax 控件**（只留中性「—」），避免禁用的勾选框造成误会。 | 隐藏 MiniMax 目标控件，保留插件安装、DSH 操作及原有选择。 |
| 技能 | 共享库接入 MiniMax 的总开关放在「已安装」卡头；每项技能分别控制两个目标。MiniMax 配置变更显示「待重启」或「下次启动生效」。 | 隐藏 MiniMax 总开关和逐项目标，已保存的选择与 DSH 侧不变。 |
| 记忆 | 同一页面查看不同来源的记录；各目标的使用开关与跨来源授权分开。来源、当前实例和项目范围可辨认。 | 隐藏 MiniMax 的目标开关；已有历史记录保留来源标识。存在共享授权时仍可查看和管理授权，不能因为入口隐藏而让授权不可见。 |
| 用量 | 公共页面不随内核切页。DSH 工作台用量、外部工具用量分别标来源；MiniMax 工作台用量未接入时不能显示为 0 或冒用 DSH 数据。 | 同样保留独立授权的外部来源，包括 MiniMax Desktop；它与 MiniMax 内核体验是两个功能。 |
| 日志窗口 | 可按实际来源筛选，包括 MiniMax。 | 隐藏 MiniMax 专用筛选入口并把当前来源筛选恢复为「全部」；历史日志及其来源标签仍保留，不能删除或伪装成 DSH 日志。 |

实例选择仍由各自页面管理，不能复活全局内核页签。概览的工作台操作和版本页安装入口必须带上明确的目标内核，不能把某一页的选择变成全局状态。

## 3. 开关和授权的含义

### 体验入口

体验开关即时保存，独立于端口「保存配置」。关掉时如果端口表单有未保存内容，保留本次会话中的草稿；隐藏期间保存 DSH 配置不提交 MiniMax 字段，再开启时恢复草稿。页面隐藏不能中断进行中的操作、退出清理或对真实运行状态的跟踪。

「保存配置」在点击时快照本次 DSH 与可见 MiniMax 端口，整轮提交期间防止重复提交。成功回执只确认对应编辑版本，不能清掉等待期间的新编辑（即使输入先改走又改回相同值）；后台状态刷新也不能代替本次保存确认。失败或部分成功时保留未提交、失败以及后续编辑的草稿。日志弹层和独立日志窗口在体验关闭时同步恢复「全部」来源，不让隐藏的筛选继续限制列表。

### 能力状态

安装、期望启用、实际接线、生效时机和授权是不同状态。开关写入失败时恢复原值并提示；处理中禁用对应动作。已安装但尚未接线、待重启、不可用或状态未知时显示短状态，不能用一个选中但禁用的复选框冒充「已启用」。同一能力不存在多余的重复大卡片。

- 内置插件按实际支持范围提供动作。Ponytail 的 MiniMax 接入修改需先停止对应工作台；实际运行效果仍须实机验证。其他内置或社区插件不因显示目标列就自动获得支持。
- 技能安装仍只有一份；DSH 侧现有行为保持即时，MiniMax 配置在下次启动时同步。总开关关闭后保留逐项选择；需要区分配置值与已确认的运行状态，无法确认就显示「待验证」。
- 长操作继续沿用现有进度面板和全局安全锁；不因为多了目标开关就放宽并发限制。

### 记忆与授权

统一的是管理页面，不合并物理存储。记录继续属于原内核、实例和项目；查看与编辑走来源侧。当前 DSH 与 MiniMax 的实例 / profile 组合需要明确授权才能互通普通记忆；来源变更后须重新确认。互通范围为通用记忆和匹配同一项目的普通记忆；候选、交接和经验不因该授权自动互通。历史逐条授权单独保留、展示和撤销。

每个目标开关控制该接收方能否使用记忆：关闭后该目标的自动检索和注入均不得返回记忆。开启本身不创建跨内核授权；关闭也不撤销已有授权，另一目标仍可按自己的授权使用。DSH 现有的自动提取等行为继续遵守原设置语义，不能把「阻止接收」误写成「删除记忆」或「撤销共享」。

现有代码中，MiniMax 的旧实例级 `use_approved` 检查之前已有共享模块检索路径；因此只隐藏入口或只改旧开关都不能保证停止检索。新目标开关必须在后端覆盖全部实际自动检索与注入入口。升级时保留旧配置和原有授权；旧状态若不能归结为简单开 / 关，应呈现「部分生效」或需要配置，而不是猜测开关状态。用户作出新选择之前不得静默扩大读取范围。

## 4. 开发任务

按表中顺序推进；依赖完成后再接对应页面。任务编号用于跟踪，不表示额外的架构层或独立服务。

| 编号 | 任务与涉及位置 | 依赖 | 完成标准 |
| --- | --- | --- | --- |
| T01 | 设置真值与兼容：在 [settings.rs](../../src-tauri/src/shell/settings.rs)、[commands.rs](../../src-tauri/src/commands.rs) 增加壳模式隔离的可选偏好；接 [bridge.js](../../ui/src/shell/bridge.js) 与现有 store。新增命令时同步注册及 capability。 | 无 | 新旧设置文件均可读取；无偏好按壳模式取默认；显式值跨重启保留；保存端口或其他设置不会覆盖偏好，保存偏好不会覆盖其他字段。 |
| T02 | 可见性与生命周期：统一使用用户体验偏好，移除构建变量限制；逐处审阅 [App.vue](../../ui/src/App.vue) 与各页的可见性用法。 | T01 | 只替换显示条件；运行状态、退出停机、错误恢复和已有任务仍按真实状态执行。独立日志窗口重新打开及已打开时都能取得正确偏好。 |
| T03 | 设置与页面选择：[SettingsPanel.vue](../../ui/src/shell/SettingsPanel.vue)、[FamilySwitch.vue](../../ui/src/kernel/FamilySwitch.vue)、[instance.js](../../ui/src/kernel/instance.js)。 | T01、T02 | 开关即时保存并按需确认；隐藏前的选择可恢复；端口草稿不误提交；DSH / MiniMax 保存部分成功时分别说明。 |
| T04 | 概览与版本：[OverviewPanel.vue](../../ui/src/shell/OverviewPanel.vue)、[ControlTower.vue](../../ui/src/diagnostics/ControlTower.vue)、[VersionsPanel.vue](../../ui/src/kernel/VersionsPanel.vue)；磁盘条目的来源信息按需核对 [diskusage.rs](../../src-tauri/src/diskusage.rs)。 | T02、T03 | 双状态来自各自最新状态，读取失败显示未知；明确目标的跳转正确，其他页选择互不影响；磁盘四类、合计和只读下钻共用，内核 / 实例来源明确且不随页头切换。 |
| T05 | 插件：[PluginsPanel.vue](../../ui/src/plugins/PluginsPanel.vue)、[targetState.js](../../ui/src/plugins/targetState.js) 及现有 MiniMax 接入组件。 | T02 | 区分安装与启用；Ponytail 按工作台状态执行，其他插件按实际支持范围显示；不得改变 DSH 的既有动作。 |
| T06 | 技能：[SkillsPanel.vue](../../ui/src/skills/SkillsPanel.vue)、[skills.js](../../ui/src/skills/skills.js)、[mcode.js](../../ui/src/kernel/mcode.js) 与 [mcode_service.rs](../../src-tauri/src/kernel/mcode_service.rs)。 | T02 | 总开关留在已安装卡头；逐项选择可保留；目标配置、运行状态与待重启状态不混淆。 |
| T07 | 记忆后端：梳理 [routes.rs](../../src-tauri/src/memory/routes.rs)、[module.rs](../../src-tauri/src/memory/module.rs)、[query.rs](../../src-tauri/src/memory/query.rs)、[share.rs](../../src-tauri/src/memory/share.rs) 和实际注入调用链，加入每个接收目标的真实使用控制。 | T01 | 所有自动检索 / 注入路径执行同一目标限制；保留旧数据、旧配置、项目范围和逐条授权；迁移前后未作新选择时不扩大访问范围。 |
| T08 | 记忆界面：[MemoryPanel.vue](../../ui/src/memory/MemoryPanel.vue)、[MemoryModulePanel.vue](../../ui/src/memory/MemoryModulePanel.vue)。 | T02、T07 | 一套页面呈现来源与目标；授权作用于明确的当前实例对；目标开关与授权分别操作，关闭互通时指出历史逐条授权仍在。 |
| T09 | 用量、日志及细节：[UsageAnalysisPanel.vue](../../ui/src/usage/UsageAnalysisPanel.vue)、[usage.js](../../ui/src/usage/usage.js)、[LogSidebar.vue](../../ui/src/logs/LogSidebar.vue)。 | T02 | 外部来源授权独立；数据按真实来源展示；关闭体验不丢历史；不支持项和失败状态有明确显示。 |
| T10 | 测试、实机和文档：更新现有测试、[面板决策](panel-decisions.md)与对应用户文档；实现稳定后再修订相关 AGENTS 约束。 | T03–T09 | 第 5 节通过；未通过的 MiniMax 实机项保持实验状态，不能以设计稿或单测代替。 |

若修改已有 IPC DTO，按 [ipc-contract.json](../../contracts/ipc-contract.json) 与前后端契约测试同步；当前 `get_status.settings` 是对象，不能假设它已经验证新增字段。设置更新沿用现有合并语义，避免其他面板保存时清空新增偏好。跨窗口状态应复用现有壳通信方式，不能依赖不同 WebView 间的 `storage` 事件。

## 5. 验收

### 配置与界面

- [ ] 覆盖开发壳 / 正式壳、偏好缺失 / 开 / 关组合；重启与 dev/release 隔离后结果正确，入口不受构建变量限制。
- [ ] 开启体验本身不安装、启动、开启共享能力或创建授权。
- [ ] 正在运行的 MiniMax 关闭入口后仍运行；重新开启可管理。取消确认不改状态；退出应用时仍按真实运行状态执行原有退出确认和清理。
- [ ] 开启、关闭、重新开启后，能力选择、独立页面选择、运行状态、授权和历史数据保持一致；真实状态读取失败时显示未知。
- [ ] 有未保存的 MiniMax 端口草稿时关闭入口，保存 DSH 配置不会提交隐藏端口；重新开启保留草稿。分别验证两个端口保存的成功和失败反馈。
- [ ] 切换 DSH / MiniMax 及关闭体验入口后，磁盘用量仍是同一份真实统计；有归属的内核和实例条目显示对应名称，共享库按共享标记，不能编造没有的数据。
- [ ] MiniMax 手写体在浅色和深色中都使用现有粉色实验色，卡头芯片不覆盖字族或颜色。明暗主题、键盘焦点、开关可访问名称、确认框、窄内容区和长文本可用；1040 × 748 下无横向裁切，实际内容可滚动。
- [ ] DSH 既有工作台、插件、技能、记忆、用量、日志、端口与退出行为不回退；全局进度 / 安全锁仍有效。

### 记忆隐私与迁移

- [ ] 接收目标关闭，且共享模块授权与旧逐条授权都存在时，针对真实请求分别验证普通记忆、其他记忆和旧路径：该目标收到的自动检索 / 注入结果为空；另一目标仍遵循自己的配置。
- [ ] 接收目标开启但跨来源授权关闭时，不产生新的跨内核读取；打开普通互通只影响当前明确的实例对与允许的范围。
- [ ] 关闭普通互通后，旧逐条授权仍按既有语义独立生效，界面提示并提供管理；关闭目标使用不改变这些授权。
- [ ] 换实例、换项目、通用 / 同项目 / 不同项目、候选 / 交接 / 经验分别验证隔离；同名项目或显示列表不能代替实际检索结果断言。
- [ ] 旧设置缺字段、旧开关关闭但共享模块可读等情况不被误显示为完全关闭；用户尚未选择新设置时不扩大原有读取范围。DSH 自动提取的旧语义保留。

### 测试与发布

- [ ] 先为设置合并、页面可见性、跨窗口同步、目标记忆检索及旧授权写回归测试。更新受可见性条件影响的既有 [familySwitchInteraction.test.js](../../ui/test/familySwitchInteraction.test.js)、[logEntryPoint.test.js](../../ui/test/logEntryPoint.test.js)、[designAlignment.test.js](../../ui/test/designAlignment.test.js)，断言实际行为而非只匹配文案。
- [ ] 前端执行命中测试、`npm run build:ui`；Rust 执行相关 `cargo test --locked --lib <筛选>` 和 `cargo check --locked`；按项目规则执行静态检查。修改内置插件时先执行 `npm run prep:builtin` 并核对内嵌资源。
- [ ] 测试只使用隔离的临时 home；桌面实跑使用独立 `DSH_XLINK_HOME`、`TAURI_CONFIG` 和 `DSH_XLINK_CHANNEL_SUFFIX`。按现有 [开发验证](../operations/development.md) 记录证据。
- [ ] 在受支持的 macOS 与 Windows 上复验工作台启动 / 停止、隐藏后退出、Ponytail 实际接入、技能重启后效果、跨内核记忆授权和用量来源。检查源码、打包资源与实际物化内容一致。
- [ ] MiniMax 实验能力尚未完成上述实机验收，常规发布不得默认宣称可用。发布前再按项目要求执行完整门禁；当前设计阶段不运行全量发布检查。

## 6. 实现状态（2026-11-08）

以下为 2026-11-08 的历史快照，构建开关相关描述不代表当前规则。**这一节只记代码与单测/构建级证据；第 5 节勾选项没有任何一条因它自动通过**，实机与跨平台验收仍未执行。

| 编号 | 状态 | 落点与判据 |
| --- | --- | --- |
| T01 | 已实现 | 壳设置 `mcode_experience: Option<bool>`（`shell/settings.rs`，未保存时开发壳默认开、正式壳默认关）；命令 `set_mcode_experience` / `get_mcode_experience`（`commands.rs`、ACL 两处）；`merge_settings` 继承该字段。判据 `shell::settings::*mcode_experience*`、`commands::settings_merge_tests::merge_keeps_paths_the_panel_did_not_submit`。 |
| T02 | 已实现 | 可见性唯一真值 `ui/src/kernel/experience.js`（`mcodeExperience` = 构建可用 ∧ 用户偏好）；App / 概览 / 版本 / 插件 / 技能 / 用量 / 日志 / 记忆全部改读它，页面不再直接读 `VITE_MCODE_EXPERIMENTAL`。独立日志窗口用只读命令 `get_mcode_experience` 取同一份偏好。判据 `ui/test/experienceVisibility.test.js`。 |
| T03 | 已实现 | 设置「工作台」卡新增体验开关（构建不可用禁用并显示「当前版本不可用」）、关闭确认三态（运行 / 授权 / 无法确认）、隐藏端口不提交且草稿保留、两套端口分别反馈。判据 `ui/test/experienceSwitch.test.js`、`settingsPortGroups.test.js`。 |
| T04 | 已实现 | 概览页头分段控件显示两套工作台状态（读不到显示「未知」，不猜成已停止），MiniMax 状态在体验开启时挂载即刷新并每 5s 轮询；磁盘用量共用统计不随切换变化。判据 `ui/test/experienceVisibility.test.js` 的概览段。 |
| T05 | 已实现 | 插件页 MiniMax 目标控件与状态随体验开关显示；安装 / DSH 动作 / 原有选择不变。判据同上「插件 / 技能 / 用量 / 内核版本」条。 |
| T06 | 已实现 | 技能页总开关与逐项 MiniMax 目标随体验开关显示；已保存选择与 DSH 侧行为不变。判据同上。 |
| T07 | 已实现 | `memory/targets.rs`：DSH / MiniMax 接收方开关（`<xlink_home>/shell/<mode>/memory-sharing/target-use.json`），`/v1/memory/context`（含 `context_only`）、`/v1/memory/read` 与 `retrieve_mcode` 同一道门禁（提取用的 `pending=1` 不拦）；不删记录、不改旧 `use_approved`、不撤销授权；无显式选择时跟随旧行为，文件损坏按关闭处理并在界面报未知。判据 `memory::targets::tests::*`。 |
| T08 | 已实现 | 共用记忆页提供 DSH / MiniMax 来源标记与筛选，页顶接收方开关与「授权范围」分开；编辑按来源写回，历史逐条授权和 DSH 候选、交接、经验仍可管理。体验入口隐藏时历史来源与授权保留，MiniMax 记录只读。判据 `ui/test/memoryPanel.test.js`、`memoryTargetUse.test.js`。模拟数据的浏览器交互已验证；真实工作台验收仍待完成。 |
| T09 | 已实现 | 用量页 MiniMax 说明随体验开关；日志来源筛选由宿主传 `sourceFilterEnabled`（独立窗口读只读偏好，窗口聚焦时重取）；历史日志与来源标签不删不改。判据 `ui/test/logEntryPoint.test.js`、`experienceVisibility.test.js` 日志段。 |
| T10 | 部分实现 | README、docs 索引、`ui/AGENTS.md`、`panel-decisions` 与[记忆进度表](../features/memory/memory-progress.md) 已同步；**第 5 节验收清单仍是未勾选状态**（缺实机与 Windows 证据）。 |

已知偏差与后续项：

- 品牌词手写体挂在 `.minimax-wordmark`（粉，两套主题各有一份 `--experimental`）与 `.deepseek-wordmark`（蓝，`--brand-deepseek`；2026-11-08 DSH 侧也改手写体并把切换器文案写全 DeepSeek；同一天用户要求取色对齐官网对话品牌蓝 `#4D6BFE`，两套主题同值），判据并入 `ui/test/minimaxWordmark.test.js`。
- 独立日志窗口依赖 `window.focus` 在副窗触发来刷新偏好；重新打开窗口这条已覆盖，已打开窗口的实时跟随需实机确认。
