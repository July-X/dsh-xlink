# 插件页精简设计：移除插件中心，保留手动安装与内置插件

> 状态：**已实现（2026-10-31），含同日二次修订**。第一版按本文 §2–§8 落地后，用户要求移除「插件库」页签、把安装入口直接放到内置插件下方并优化排版；最终形态见紧随其后的[二次修订](#修订2026-10-31-二次调整移除插件库页签)。§9 的决策第一版全部采用默认项（页签改「插件库」、安装区块置顶、清单保持内容高、旧缓存启动时清理、GitHub→npm 捷径本次不动），其中「插件库页签」已被二次修订撤回。
>
> 关联文档：[插件管理设计](plugin-management.md)（落地后按「当前行为」重写，社区目录与 gzip 取源两章整体删除）、[内置插件通道](builtin-plugins.md)（本次不动）、[面板决定记录](../../ui/panel-decisions.md)（落地时追加一条变更记录）。原型见 [plugin-page-simplification-mockup.html](plugin-page-simplification-mockup.html)，暗色 / 浅色 / 空态截图见同目录 `plugin-page-simplification-{dark,light,empty}.png`。

## 修订：2026-10-31 二次调整（移除「插件库」页签）

用户看过第一版实现后要求：**移除「插件库」页签，让「安装插件」直接显示在内置插件下方，
做好 UI 排版优化**。最终形态改为**单视图、无页签**：

- 顶部工具条：左「当前内核（族名 · 活动版本）」，右「同步内核 / 检查更新」——原先挂在页签条
  右端的动作需要一个新落点，且必须与「当前内核」语境同行。
- 「内置插件」独立一块：块头「内置插件 · 随应用交付，启停后重启工作台生效」+ 自己的
  `.entity-list`；每行仍是 `.entity-row--single`，右侧只有账户动作 / 上游检查更新 / 启停开关。
- 安装区块紧接内置插件清单下方：`npm 包名 / Git 地址` 输入 + 「预检并安装 / 安装」按钮 +
  说明，回车与按钮走同一个 `installFromSpec`；块头沿用 2px 刻蚀线。
- 「已安装插件」= 当前内核的社区插件行（更新 / 模式 / 仓库 / 卸载动作不变），块头带
  `N 个 · 只管理当前内核`；空态只看社区行（`communityListEmpty`），加载中仍显示骨架。
- 删除：`el-tabs` / `el-tab-pane`、`installedTab` 与每实例 chip 的计算、`sortedInstances` /
  `instanceState*`、`.installed-tabs*` / `.all-instances-list` / `.instance-state-chip*` 样式、
  `main.js` 的 ElTabs / ElTabPane 注册与 `tabs` / `tab-pane` 样式导入。
- 判据：`designAlignment.test.js` 改判「工具条 → 内置插件块头 / 清单 → 安装区块 → 已安装插件
  块头 / 清单」的顺序与无页签；删除页签行距与药丸特异度两条；内置插件判据改为「单独成块、
  在安装入口之前」，空态改判 `communityListEmpty`。
- 原型与截图已按最终单视图重画；本文 §2 目标形态、§3.1 / §3.3 / §3.4、§5.2 中「双 tab /
  插件库页签 / 实例 chip」的行文是**第一版中间态**，实现以本节与源码为准。具体到规格表：
  §2.4「页签与 tab 头动作」「每实例 chip」两行、§3.3 里「保留 `.installed-tabs` /
  `.instance-state-chip*`」的条目同样作废，不要再按它们把页签加回来。§4（后端删除清单）与
  §8（风险与回滚）不受影响，仍然有效。

## 修订：2026-10-31 三次调整（安装面板加高 + 动作下沉 + 描述收进 tooltip）

用户看过第二版实现后指出三件事：手动安装区块的高度太低；「同步内核 / 检查更新」只针对
手动安装的插件，不该挂在页面顶部；描述性文本能收进 tooltip 的都收起来。最终形态：

- **删掉顶部工具条**：当前内核身份并入「内置插件」块头的 ⓘ（tooltip 里写明身份与交付方式）。
- **安装区收成一块独立面板**：`--overlay-faint` 底 + 1px 描边 + 10px 圆角 +
  14/16/12 内边距，高度从原来「块头 + 输入 + 一句说明」抬到约一顿读数的体量；
  标题「安装插件」右侧 ⓘ 承载格式 / 预检 / 生效时机说明。
- **「同步内核 / 检查更新」下沉到面板标题行右侧**（用户随后再次指认了位置），
  与「安装插件」同排、靠右，另配 ⓘ 说明「同步会重新物化中央库到所有内核；检查更新只
  检查手动安装的插件」。
- **描述性文字全部 tooltip 化**：预检开关旁不再显示「安装前先在沙盒实例里试装并启动一次」；
  内置插件块头不再显示「随应用交付 · 启停后重启工作台生效」；安装提示段、作用范围说明一并删除。
  行内状态也只留短词（「已接线」「待重新接线」），完整说明放行内 tooltip。
- **社区插件空态收成一行**（不再画带插图的 `el-empty`），把纵向空间让给安装面板。
- 判据：`designAlignment.test.js` 改判「内置插件 → 安装面板 → 已安装插件」顺序、面板
  底色 / 描边 / 内边距、动作行位置与四个 ⓘ 的接线；反向确认旧的常驻说明节点已删除。

## 0. 摘要

- **移除**：dshfind.com 远端插件目录及其全部派生能力——目录取源双源回退、gzip 大响应、6 小时缓存、壳侧搜索/分类/排序/分页、`plugin_catalog_search` 命令与 ACL、`src-tauri/src/plugins/catalog.rs`、`center.rs` 的目录段，以及相关测试与文档。
- **保留**：手动安装（npm 包名 / Git 地址，预检开关与两阶段沙盒）、已安装插件的同步/更新/模式切换/卸载/隔离、按实例状态、内置插件通道、更新检查、物化与接线。**安装、管理、预检、诊断链路没有任何一处依赖目录结构**（见 §4.2）。
- **改版**：插件页仍是「单卡 + 双 tab」，但「已安装」tab 从两栏收回单栏：右栏那份远端目录消失，手动安装从右栏页签升为**本页唯一的获取入口**，位于清单上方（输入行 + 显式按钮 + 一句说明），下方是本机插件库清单与按实例状态 chip。页头副标题与空态文案同步改写。
- **收益**：打开插件页不再有 12.7 MB 级远端目录请求（只剩可选的更新检查）、不再有 75 秒目录看门狗、两栏等高与列表内滚动那套高度链不再需要；删除约 2.4k 行代码（Rust 约 1.3k、前端与测试约 1.1k）与一条长期维护的外部数据依赖。
- **代价**：应用内不再提供社区目录浏览与一键安装。安装仍支持任何 npm 包 / Git 仓库，用户需自行知道包名或地址；这是本次明确接受的取舍。

## 1. 背景与问题

### 1.1 现状链路（2026-10-31 代码状态）

> 行号按 2026-10-31 工作树记录；工作树里可能还有其它未提交改动，随后续合并会位移，定位以符号名为准。

| 层 | 现状 | 位置 |
| --- | --- | --- |
| 页面 | 「插件管理」单卡 + 双 tab；「已安装」tab 分两栏：左 = 本机插件库清单 + 每实例 chip，右 = 「插件中心 / 手动安装」两个子页签，默认打开插件中心 | `ui/src/plugins/PluginsPanel.vue:369-760` |
| 前端状态 | 目录条目 / 命中总数 / 分类计数 / 已显示条数 / 关键词 / 排序 / 加载态，搜索走 75 秒看门狗 + `singleFlight` | `ui/src/plugins/plugins.js:13-248` |
| 后端命令 | `plugin_catalog_search`：关键词/分类/排序/limit/分类中文名一起发给壳，壳在缓存目录上筛一页 | `src-tauri/src/plugins/catalog.rs`、`lib.rs:598-601` |
| 后端取源 | dshfind.com `/api/plugins-data` 为主源、`dsh-plugin-market` 为回退源；gzip 传输、解压后上限 48 MiB、6 小时缓存 | `src-tauri/src/plugins/center.rs:2678-3014`、`pkg/releases.rs:388-505` |
| 数据 | `<xlink_home>/<family>/desktop[-dev]/plugins-catalog.json`（按壳模式、按内核族分家，所有实例共用默认族那份） | `center.rs:56-59`、`pkg/releases.rs` |
| 错误处理 | 体积超限 / 网络故障 / 解析失败分三套措辞，前端再把「目录为空或加载失败」映射成引导 | `center.rs:2983-3014`、`plugins.js:139-221` |
| 判据与文档 | 独立前端测试 226 行、面板对齐测试约 8 处、Rust 目录单测 22 个；README 与 5 份 docs 仍把目录当现行功能描述 | 见 §5、§6 |

### 1.2 为什么移除

1. **维护面与收益不匹配**：目录是一个随站点增长的外部数据契约（2026-10-24 实测 12.7 MB / 21421 条），为它单独养了 gzip 传输、独立体积上限、双源回退、三套错误措辞与一套壳侧检索。它不参与安装、校验、物化、接线、诊断任何一环，是页面里最重、耦合最浅的一块。
2. **一次体积故障就整块空白**：目录涨过通用上限后两条源同时失败，整个插件中心空白，而提示把用户引向网络（那次网络其实是好的）。这类故障的根因在远端数据增长，不在本机，只能靠一次次抬高上限追着跑。
3. **打开插件页即有网络依赖**：面板一打开就发目录请求，离线或网络差时首屏被一块加载占位占据；用户真正要用的本机插件清单反而被右栏挤到半宽。
4. **安装入口分裂**：同一页里有三处安装语义——目录条目「预检并安装 / 安装」、手动输入回车、预检报告里的「应用变更」。移除目录后只剩一条手动安装路径，语义收敛为一处。
5. **产品决定**：用户明确要求移除「插件中心」相关功能、保留手动安装与内置插件。目录是可选发现能力，不是插件管理闭环的一部分；外部站点仍可浏览，装什么由用户自己决定。

### 1.3 目标与非目标

**目标**

- 插件页只表达两条主线：**本机插件库**（装了哪些、各实例什么状态、怎么手动装）与**当前内核**（内置插件 + 本内核实际接线与动作）。
- 删除远端目录的全部代码、命令、ACL、缓存、测试与文档口径；`rg '插件中心|dshfind|plugin_catalog|plugins-catalog'` 在生产代码中归零。
- 保留且不改变：手动安装、预检两阶段、更新检查、同步、模式切换、卸载、隔离恢复、多实例 chip、内置插件启停与账户卡。
- 首屏不因移除产生新的空窗：插件库为空时，安装区块与空态文案直接给出下一步。

**非目标**

- 不改内置插件通道（`plugins/builtin/**`）、预检三态契约、物化/接线实现。
- 不改安装取源链路本身（npm registry / GitHub Release / git clone 的信任边界与校验规则）。
- 不给技能页复制本设计；不改侧栏与窗口结构。
- 不保留任何形式的远端目录入口，包括「打开 dshfind.com 浏览」外链。发现路径交回浏览器，后续如要加外链另行决定。
- 不顺带清理历史文档里的事故复盘：2026-10-24 的目录体积事故记录保留为历史证据。

## 2. 目标形态

### 2.1 信息架构

**现状（「已安装」tab）**

```text
┌ 插件管理 ⓘ ───────────────────────── [预检 ●] ⓘ 安装前先… ┐
│ [已安装] [当前内核（DSH · 0.2.2-alpha.2）]   [同步内核] [检查更新] │
│ ┌────────────────────┬──────────────────────────────────┐ │
│ │ 本机插件库清单      │ [插件中心] [手动安装]              │ │
│ │ 每行：名称+来源+    │  插件中心：搜索 / 分类 / 排序 /      │ │
│ │ 每实例状态 chip     │  条目列表 / 显示更多 / 刷新数据     │ │
│ │                    │  手动安装：一个输入框，回车即装      │ │
│ └────────────────────┴──────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

**目标（「插件库」tab）**

```text
┌ 插件管理 ⓘ ───────────────────────── [预检 ●] ⓘ 安装前先… ┐
│ [插件库] [当前内核（DSH · 0.2.2-alpha.2）]   [同步内核] [检查更新] │
│ ↓ 安装插件                              npm 包名 / GitHub 地址 │
│ ─────────────────────────────────────────────────────────── │
│ [npm 包名 · @scope/pkg@版本 · GitHub 地址 / owner/repo    ↵] [预检并安装] │
│ 支持 npm 包名（可带 @版本）与 GitHub 地址 / owner/repo；开启预检时 │
│ 先在一次性沙盒实例里试装，确认后才写入插件库。安装完成后重启工作台生效。 │
│                                                             │
│ 已安装  2 个插件 · 按实例列出装载状态                         │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ @ace-zone/dsh-market  [npm]                             │ │
│ │ [DSH · default · 已接线] [DSH · default-dev · 未同步]    │ │
│ ├─────────────────────────────────────────────────────────┤ │
│ │ dsh-lights-out  [github]                                │ │
│ │ [DSH · default · 版本不一致] [DSH · default-dev · 已接线] │ │
│ └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

「当前内核」tab 与现状一致：内置插件行（内嵌 chip、账户卡/上游更新/启停）+ 社区插件行（版本、状态、链接/复制、更新、仓库、卸载），不做改动。

### 2.2 设计稿

- 暗色：[plugin-page-simplification-dark.png](plugin-page-simplification-dark.png)
- 浅色：[plugin-page-simplification-light.png](plugin-page-simplification-light.png)
- 空态：[plugin-page-simplification-empty.png](plugin-page-simplification-empty.png)
- 可交互原型（静态、合成数据）：[plugin-page-simplification-mockup.html](plugin-page-simplification-mockup.html)（`?theme=light`、`?empty=1` 可切换）

![插件库页签（暗色）](plugin-page-simplification-dark.png)

### 2.3 与现状的差异

| 项 | 现状 | 目标 | 理由 |
| --- | --- | --- | --- |
| 「已安装」tab 布局 | 两栏（左清单 / 右获取入口） | 单栏：安装区块在上，清单在下 | 右栏内容已删；两栏的竖线与等高高度链没有第二个用途 |
| 获取入口 | 目录 + 手动安装两个子页签 | 只有「安装插件」区块 | 入口收敛为一处，不再有默认子页签与切换状态 |
| 安装输入 | 仅输入框，回车提交 | 输入框 + 显式主按钮，回车等价 | 与技能页手动安装一致；动作不再藏在一次按键里 |
| 清单标题 | 无（左栏直接列） | 「已安装 · N 个插件 · 按实例列出装载状态」 | 安装区与清单分开后，清单需要自己的块头 |
| tab 名称 | 已安装 | **插件库**（建议，见 §9-①） | 该 tab 现在同时是安装入口与中央库视图，「已安装」与清单标题重名 |
| 页头副标题 | 「查看当前实例生效的插件，并从插件中心安装或升级。」 | 「手动安装插件、检查更新，并按内核同步与启用。」 | 不再有插件中心，且页面本来就不只讲当前实例 |
| 空态 | 「…用右侧「手动安装」或插件中心获取插件。」 | 「本机插件库为空；在上方填写 npm 包名或 Git 地址安装第一个插件。」 | 指引到上方唯一的安装区块 |
| 刷新数据 / 搜索 / 分类 / 排序 / 显示更多 | 右栏 | 删除 | 全部属于远端目录 |
| 页面高度链 | 面板 → 卡片 → tab → 双栏等高 → 目录内部滚动 | 面板 → 卡片 → tab → 单栏；安装区固定高、清单内容高、超长时清单内滚 | 少一层栅格；清单仍不画空荡荡的大边框 |
| 打开页面时的网络请求 | 目录请求（可触发 12.7 MB 下载）+ 更新检查 | 只有更新检查（原有 15 分钟 TTL，失败静默） | 目录是唯一的面板内网络依赖 |

### 2.4 组件与样式规格

**复用现有原语，不新增设计 token：**

| 用途 | 复用的原语 / 规则 |
| --- | --- |
| 页头 | `.page-head` / `.page-head__text` / `.page-title-row` / `.page-title` / `.page-desc` / `.head-tip-icon--warning` |
| 卡片与卡头 | `.card` / `.entity-card` / `.card-head` / `.card-title-icon` / `.plugin-center-title`（落地时建议改为 `.plugin-card-title`，见 §3.3） |
| 预检开关行 | `.precheck-toggle-row` / `.precheck-toggle-label` |
| 页签与 tab 头动作 | `.installed-tabs-wrap` / `.installed-tabs`（药丸页签）/ `.tabs-head-actions` |
| 安装行 | 全局 `.install-row`（`input` 自动 `flex:1`）+ `.btn-action` + `type="primary"`，与技能页手动安装同一条 |
| 安装区块块头 | `.section-divider` + `.card-title-icon` + `.section-divider__note`；2px 刻蚀线沿用被删的 `.plugin-center-col .section-divider::before` 那条规则，改挂到新的 scoped 类上（净复制数不变） |
| 清单与行 | `.entity-list` / `.all-instances-list` / `.entity-row--instance-grid` / `.entity-head` / `.entity-name` / `.origin-chip` / `.instance-state-chip*` / `.entity-list.is-empty` |
| 单行动作（当前内核页签） | `.entity-row--single` / `.entity-meta` / `.entity-states` / `.state-dot` / `.entity-actions` 全部不动 |

**新增的 scoped 规则（预计 3 组，均留在 `PluginsPanel.vue`）：**

- `.plugin-install-block`：顶部安装区容器，`flex: none`，纵向 0 / 8px 间距；块头 2px 刻蚀线用 `::after` 画（与卡片头同一套六边形收细）。
- `.plugin-install-hint`：12px、`--text-muted`、`line-height: 1.6`，行宽受卡片内宽约束，不用 `nowrap`。
- `.plugin-library-head`：清单块头，`display:flex; align-items:center; gap:8px`，`border-top: 1px solid var(--border-soft)` + `padding-top: 12px`；标题走 `--fs-subtitle`，计数/说明走 `.section-divider__note` 或 12px muted。

**删除的样式**：`PluginsPanel.vue` scoped 里的 `.page-layout` 覆盖、`.plugin-center-col*`、`.catalog-*`（含 `TransitionGroup` 入退场）整段；`theme.css` 里的 `.catalog-version`（与 `.meta-version` 的合并选择器拆开）、`.plugin-center-refresh` 两条、以及「插件中心」注释段。`.section-divider__note` 规则保留——安装区块块的右侧说明继续用它。

### 2.5 交互与状态

| 场景 | 行为 |
| --- | --- |
| 页面打开 | 默认停在「插件库」tab；**不发目录请求**。原有的插件更新检查（15 分钟 TTL、失败静默、手动点击绕过 TTL）保持不变 |
| 输入为空 | 主按钮 disabled；回车沿用现有 `installPlugin` 的守卫，提示「请先填写仓库地址或 npm 包名」 |
| 预检开（默认） | 主按钮文案「预检并安装」；点击/回车 → `precheckPlugin('')` → 进度浮层（两段临时内核）→ 预检报告对话框 →「应用变更」才真正安装 |
| 预检关 | 主按钮文案「安装」；点击/回车 → `installPlugin('')` → 进度浮层 → 成功后清单刷新 |
| 安装成功 | 保持现有 toast 与「重启工作台生效」口径；清单由 `refreshAll` 重读 |
| 清单为空 | 空态：「本机插件库为空；在上方填写 npm 包名或 Git 地址安装第一个插件。」不给右侧/插件中心指路 |
| 实例注册表加载失败 | 行内保留现有「未加载实例列表（实例注册表加载失败）」提示，不新增状态 |
| 行内状态 | 每插件一行：名称 + 来源 chip（npm/github）+ 每实例一枚 chip（族名 · 实例 id · 状态）；写动作仍在「当前内核」tab，不在清单里新增按钮 |
| 警告 / 已隔离 | 沿用 `PluginStatus.warning` 的 `el-alert` 与行内 `is-warn` / 隔离 chip，位置与文案不改 |
| 键盘 | 输入回车与按钮同一动作；按钮是原生焦点顺序里的下一站；不新增 icon-only 按钮 |
| 屏幕阅读器 | 安装按钮有可读文案（「预检并安装 / 安装」）；预检开关的可访问名沿用现有说明；空态与清单块头都是普通文本 |

### 2.6 文案

| 位置 | 现行 | 目标 |
| --- | --- | --- |
| 页头副标题 | 查看当前实例生效的插件，并从插件中心安装或升级。 | 手动安装插件、检查更新，并按内核同步与启用。 |
| tab 1 | 已安装 | 插件库（建议；备选保留「已安装」，见 §9-①） |
| 安装块头 | —（右栏页签「手动安装」，块头「按地址安装」） | 安装插件 |
| 安装块头右侧 | —（右栏块头「来自 dshfind.com」） | npm 包名 / GitHub 地址 |
| 安装输入 placeholder | npm i @scope/pkg · 也支持 owner/repo、dsh add | npm 包名 · @scope/pkg@版本 · GitHub 地址 / owner/repo |
| 安装说明 | —（旧手动安装无说明） | 支持 npm 包名（可带 @版本）与 GitHub 地址 / owner/repo；开启预检时先在一次性沙盒实例里试装，确认后才写入插件库。安装完成后重启工作台生效。 |
| 清单块头 | — | 已安装 · N 个插件 · 按实例列出装载状态 |
| 清单空态 | 本机插件库为空；用右侧「手动安装」或插件中心获取插件。 | 本机插件库为空；在上方填写 npm 包名或 Git 地址安装第一个插件。 |
| 当前内核空态 | 当前内核尚未接入任何插件；先到「已安装」页签安装。 | 当前内核尚未接入任何插件；先到「插件库」页签安装。 |
| 预检失败提示（前后端共 6 处） | …请先到插件中心关闭安装预检 / 可以在插件中心关闭安装预检 | …请在插件页顶部关闭「预检」开关后重试 / 可以在插件页关闭安装预检 |
| 预检来源漂移 toast | …请到插件中心核对实际装了什么。 | …请到「已安装」清单核对实际装了什么。 |
| README / docs 中的「插件中心」措辞 | 见 §6 | 统一改为「插件页 / 安装入口」，不再出现「插件中心」这个界面名 |

### 2.7 命名

- 页面名「插件」、卡片名「插件管理」保持不变：它们描述的是整个模块，不依赖目录。
- 「插件中心」作为**界面名**整体消失，不再保留为任何分区标题、页签名或 tooltip 措辞。
- 建议把「已安装」页签改名为「插件库」：右栏消失后，这个 tab 回答的是「本机装了什么 + 从哪装」，而不是「哪些插件已安装」——后者由「当前内核」tab 的行状态表达。改名同时消除与清单块头「已安装」的重复。若不想动用户熟悉的名字，保留「已安装」并把清单块头换成「本机插件库」，见 §9-①。

### 2.8 页面高度与响应

- 继续沿用现有高度链：`.panel { height:100% }` → `.entity-card { flex:1 }` → `.installed-tabs` 拉成flex列 → `el-tabs__content` → `el-tab-pane`。安装区块 `flex: none`，清单 `min-height: 0; overflow-y: auto`。
- 清单**不**加 `flex-grow`（与现状一致）：行少时保持内容高，不画空边框；行多时在清单内部滚动，不把 `main` 撑出第二条滚动条。代价是插件少时卡片下半部留有空白，这与当前「当前内核」tab 的现状一致（用户提供的截图里也是这一形态）；若评审希望更饱满，可改为 `flex: 1 1 auto` 让清单容器撑满，见 §9-③。
- 窄窗仍靠 `.card-head` / `.page-head` 的 `flex-wrap` 自然折行；安装行 `input` 可收缩（`min-width: 0`），按钮不被压扁。
- 不新增固定宽度与断点；1040 窗口内容区约 883×710，与原型一致。

### 2.9 备选方案与取舍

| 方案 | 形态 | 优点 | 代价 | 结论 |
| --- | --- | --- | --- | --- |
| A 最小改动 | 删右栏，安装输入条留在清单**下方** | 改动最小、测试改动最少、不碰页签 | 安装入口要滚到底才看见，与技能页「安装在上」相反 | 备选（只想最小风险时） |
| **B 本设计** | 单栏，安装区块置顶 + 显式按钮 + 清单块头 | 入口最可见；与技能页手动安装同构；单栏拿到全宽 | 需改模板/脚本/样式与 6 处文案；列表短时下方留白 | **采用** |
| C 安装弹窗 | 清单撑满整卡，安装按钮开 `el-dialog` | 列表空间最大 | 多一层交互；预检本身会弹报告，弹窗套弹窗；触发按钮不能放卡头（对两个 tab 都可见） | 不采用 |
| D 两张卡去页签 | 技能页同构：已安装卡 + 当前内核卡 | 两视图同屏 | 与现有「单卡双 tab」IA 冲突；两批插件重复；`el-tabs` 注册与判据要一起删 | 否决 |

选 B 的核心理由：右栏消失后，安装是这一页唯一的「获取」动作，放在 tab 顶部最符合动作归属；A 把唯一入口藏到清单末尾，正是本次想消除的「入口分裂」的另一种形态。

## 3. 前端改造设计

### 3.1 `ui/src/plugins/PluginsPanel.vue`

**模板**

1. 页头 `page-desc` 换成 §2.6 新文案；第三方来源 ⚠ 图标与 tooltip 不变。
2. 卡头：内容与预检开关行不动，但**把标题从 `<span class="plugin-center-title">` 改成 `<h2>`**（`ui/AGENTS.md`「卡头标题一律用 `<h2>`」；当前代码是历史遗留）。`.`card h2` 已提供 `margin:0`、flex 对齐与 17px/600，`.plugin-center-title-row` / `.plugin-center-title` 两条全局规则可以一起删。
3. 「已安装」tab（建议改名「插件库」）：
   - 删除 `<div class="page-layout">` / 左栏 `<div class="page-layout__col">` 包裹，清单直接落在 pane 内。
   - 删除右栏 `<div class="page-layout__col plugin-center-col">`、`source-tabs`、`插件中心` 页签（搜索框、分类/排序下拉、`catalog-list`、条目模板、`显示更多`、`刷新数据`）。
   - 删除 `手动安装` 子页签，把输入行提升为 pane 顶部的「安装插件」区块；新增显式主按钮与说明行。
   - 清单前加 `.plugin-library-head`（「已安装 · N 个插件 · 按实例列出装载状态」）；清单本体（`entity-row--instance-grid` + 实例 chip）原样保留。
   - 空态文案换成 §2.6；`currentKernelListEmpty` 与「当前内核」空态文案同步改「插件库」。
4. 「当前内核」tab：模板零改动（内置行 + 社区行 + 动作）。
5. 顺手修正两处陈旧注释：`:428` 的「默认激活『当前内核』」与实际默认值（`installedTab = ref('all')`）不符，按「默认插件库」改写；`installedTabTip` 文案从「安装新插件」改成「插件安装与各实例装载状态」也不必，保持原句即可。

**脚本**

- 删除 `catalogLoading` / `CATALOG_PAGE` / `catalogCategories` / `categoryLabel` / `formatCount` / `formatUpdated` / `installedKeys` / `isInstalled` / `searchCatalog` / `submitCatalogSearch` / `refineCatalog` 的 import。
- 删除 catalog 相关状态与函数：`keys`、`items`、`hasMore`、`catChips`、`searchText`、`search()`、`pickCategory()`、`pickSort()`、`showMore()`、`emptyHint`、`detailUrl()`、`descText()`、`tagsTip()`、`statsText()`、`sourceTab`。
- 删除不再使用的图标 import：`Search`、`ArrowDown`（`Grid`、`TopRight`、`Download` 等仍被其它区块使用，逐项确认后再删）。
- 新增：
  ```js
  function installFromSpec() {
    return precheckOn.value ? precheckPlugin('') : installPlugin('');
  }
  ```
  要点只有一条：**回车与按钮走同一个函数**，预检开关只在一处判；空输入沿用 `installPlugin` / `precheckPlugin` 已有的「请先填写仓库地址或 npm 包名」守卫。按钮 `:disabled="globalBusy || !pluginStore.spec.trim()"`，不挂 `:loading`（`withProgress` 负责在途状态，假 loading 是技能页踩过的坑）。

**样式**

- 保留高度链与 `.entity-list` / `.installed-tabs` / `.entity-row--single` 规则。
- 删除 §2.4 列出的 catalog / 双栏样式；把旧 `.plugin-center-col .section-divider` 的刻蚀线规则搬到新的安装块头类上。
- 新增 `.plugin-install-block` / `.plugin-install-hint` / `.plugin-library-head` 三组，全部在 scoped 块内，不往 `theme.css` 加单页样式。

### 3.2 `ui/src/plugins/plugins.js`

- 删除：`CATALOG_CATEGORIES`、`CATEGORY_LABELS`、`CATALOG_PAGE`、`pluginStore` 的 `catalogItems` / `catalogTotal` / `catalogCounts` / `catalogLoaded` / `category` / `shown` / `query` / `sort` 字段、`catalogLoading`、`categoryLabel()`、`formatCount()`、`formatUpdated()`、`installedKeys()`、`isInstalled()`、`catalogCategories()`、`searchCatalog()` / `loadCatalog()` / `submitCatalogSearch()` / `refineCatalog()` 与看门狗 / `CATALOG_FALLBACK_HINT` 一整段。
- 保留：`pluginStore.view` / `pluginStore.spec`、`installPlugin`、`precheckPlugin`、`applyPluginChange` + `sourceDrift` + `showPrecheckReport`、`setPrecheckEnabled`、`updatePlugin`、`setPluginMode`、`resolvePluginQuarantine`、`syncPlugins`、`uninstallPlugin`、`checkPluginUpdates`、`refreshPlugins`。
- 清理 import：`ref`、`singleFlight`、`withLoading` 在目录移除后都没有使用方，一并删除；`toastActionError` 仍被 `setPrecheckEnabled` 使用，保留；`toast` / `toastSuccess` / `withProgress` / `createUpdateChecker` / `createStatusSource` 继续使用。
- **两处形参语义要改但能力不能删**：`precheckPlugin(specFromCatalog)` 仍被 `diagnostics/diagnostic-actions.js` 用诊断里的 spec 调用，形参改名 `specOverride`；`installPlugin(specFromCatalog)` 删目录后唯一调用方传空串，可简化成无参，但不要顺手删 `precheckPlugin` 的可选参数。
- 模块头注释从「中央仓列表、插件中心目录、安装 / 更新…」改为「插件库、安装 / 更新 / 卸载 / 同步 / 模式切换」。

### 3.3 `ui/src/theme.css`

- 删「插件中心」小节里只服务目录的规则：`.plugin-center-refresh`、`.section-divider .plugin-center-refresh`。
- `.plugin-center-title-row` / `.plugin-center-title` 随卡头改成 `<h2>` 一并删除：`.card h2` 已经给出 `margin:0` / `display:flex` / `gap:8px` / `--fs-block-title` / 600，再留一份就是重复规则。`designAlignment` 里钉 `.plugin-center-title` 字号的那条（约 2539）随之删掉或改钉 `.card h2`。
- 版本徽标的合并选择器 `.meta-version, .catalog-version` 拆成只剩 `.meta-version`，注释同步删掉 `catalog-version` 一行；`.meta-version::before` 的选择器组同样处理。
- `.section-divider__note` 保留（安装块头右说明继续用）。
- 顶部 `--surface-loading` 注释里举的「插件中心搜索时目录整块变灰」保留为历史例子即可，不影响实现；轻量顺手改可不做。

### 3.4 其它前端文件

| 文件 | 改动 |
| --- | --- |
| `ui/src/App.vue:186-195` | 删 `loadCatalog` import 与调用；插件分支只留 `checkPluginUpdates(...)`；注释改成「更新检查与插件状态互不依赖」 |
| `ui/src/plugins/PrecheckDialog.vue:62,64` | 两处「到插件中心关闭安装预检」改成「在插件页顶部关闭「预检」开关后重试」 |
| `ui/src/plugins/plugins.js:357` | 来源漂移 toast 的「请到插件中心核对」改成「请到「已安装」清单核对」 |
| `ui/src/main.js:197-201` | 注释里「插件中心的加载占位」改成通用例子（`v-loading` 仍被用量窗口 / 套餐窗口 / 记忆效果页使用，指令注册本身**不能删**） |
| `ui/src/shell/SideBar.vue` | 不改（badge 读 `view.updates`，与目录无关） |

## 4. 后端改造设计

### 4.1 删除清单

| 位置 | 内容 | 处置 |
| --- | --- | --- |
| `src-tauri/src/plugins/catalog.rs` | 整文件 529 行：`CatalogQuery` / `CatalogPage` / 搜索纯函数 / `plugin_catalog_search` / 11 个单测 | 整文件删除 |
| `src-tauri/src/plugins/mod.rs:8` | `pub(crate) mod catalog;` | 删 |
| `src-tauri/src/plugins/center.rs:50-59` | `HUB_CATALOG_URL` / `MARKET_CATALOG_URL` / `CATALOG_CACHE_FILE` / `CATALOG_TTL_SECS` | 删 |
| `center.rs:232-261` | `CatalogItem` | 删 |
| `center.rs:2678-3014` | `CatalogDoc` / `CatalogRaw` / `CatalogInstall` / `FindEnvelope` / `FindRaw` / `FindDownloads` / `into_item` / `from_market_raw` / `filter_installable` / `CATALOG_MAX_BYTES` / `fetch_catalog` / `catalog()` / `catalog_error` | 删 |
| `center.rs:33` | import `http_get_string_gzip` | 删（`http_get_string` / `http_get_file` / `http_get_npm_latest` 保留） |
| `center.rs:4072-4109, 4279-4424` | 4 个目录单测：`filter_installable_*`、`find_entry_*`（2 个）、`catalog_error_*` | 删 |
| `src-tauri/src/lib.rs:598-601`（约） | `plugin_catalog_search` handler 与注释 | 删 |
| `src-tauri/permissions/app-commands.json`（约 170 行） | `"plugin_catalog_search"` | 删 |
| `src-tauri/src/pkg/releases.rs:388-505` | `http_get_string_gzip`、`BODY_TOO_LARGE_MARKER`、`body_too_large` | 删（`MAX_HTTP_BODY_BYTES` / `http_get_string` / `serve_once` 夹具保留） |
| `releases.rs:1043-1050, 1052-1098, 1209-1311` | `gzip()` 测试夹具 + 7 个 gzip / 体积上限单测（1052-1098 三个、1209-1311 四个） | 删 |
| `src-tauri/src/commands.rs:2799-2801` | 描述该命令的历史注释 | 删或改写 |
| `src-tauri/Cargo.toml` | `flate2` | **保留**：`pkg/archive.rs` 与 `node/install.rs` 仍在用 |

### 4.2 保留与共享边界（不可误删）

- **安装链**：`commands::plugin_install` → `center::install` / `install_for_instance` / `install_unlocked` → `npm_candidates_from_github_spec` → `parse_spec` → `fetch_into_store`。目录只是批量产出 spec 的地方，安装链本身对目录零依赖。
- **`npm_candidates_from_github_spec`（center.rs:3204-3249）及其 5 个测试（4427-4477）必须保留**：手动填 GitHub URL 同样会先试 `<repo>` / `dsh-<repo>` 的 npm 包。只把注释里的「插件中心安装」改成「GitHub 来源安装」。
- **共享助手**：`github_repo_path`（`parse_spec` / `fetch_git` / `git_latest` 也在用）、`http_get_string`、`http_get_file`、`http_get_npm_latest`、`atomic_write`、`BTreeMap`、`flate2`、`serve_once` 测试夹具。
- **管理/物化/接线/预检/隔离**：`update` / `check_updates` / `git_latest` / `uninstall` / `set_mode` / `sync_all` / `status` / `materialize_*` / `ensure_wiring*` / `wire_manifest` / `sweep_instance_orphans` / `reconcile_store` / `migrate_legacy_store` / `precheck_cmd` / `precheck` / `sandbox` / `quarantine` / `builtin/**` 全部不动。
- **同名概念勿删**：`openai_catalog_refresh`、`openai/catalog.rs`（模型目录）、`official_chat` 的 `CatalogItem` / `CATALOG_PATH`、`official-chat` 的 fixtures。

### 4.3 命令与 ACL 必须同一笔改动

`check-invariants` 双向校验：① 已注册命令集合 == `allow-local-commands` 集合；② 前端 `invoke` / `cmd:` 调用的命令必须已注册且已授权。三处是同一份契约：

1. `ui/src/plugins/plugins.js` 的 `invoke('plugin_catalog_search')`（删除）；
2. `src-tauri/src/lib.rs` 的 handler 注册（删除）；
3. `src-tauri/permissions/app-commands.json` 的白名单条目（删除）。

任一处漏删，`npm run check:invariants` 都会红；不存在「先合后端、前端下次再说」的中间态。门禁位置：命令注册 ↔ ACL 双向比对在 `scripts/check-invariants.mjs:125-173`，UI 调用授权扫描在 `:192-225`。

顶层「已安装 / 当前内核」两个页签保留，因此 `main.js` 里的 `el-tabs` / `el-tab-pane` 注册与样式**不能删**（`check-invariants` 第 8 项要求「注册了必须有人用、模板用了必须已注册」）。只有把页签一起删掉时才需要连注册与样式一起清。

### 4.4 旧缓存文件

- 路径：`<xlink_home>/<family>/desktop[-dev]/plugins-catalog.json`，由目录取源写入，代码删除后无人读取。
- 建议在启动对账入口 `center::reconcile_store`（`lib.rs:312` 调用）里做一次 **best-effort 精确路径删除**：只删 `data_dir.join("plugins-catalog.json")`，不递归、不碰任何其它文件；失败静默（与 `migrate_legacy_store` 同一失败口径）。它是壳自己写的可再生缓存，不属于 `store.json`、用户插件源码或任何配置，删除不是数据迁移。
- 不新增 schema、不改 `store.json`、不写迁移向导。若决策选择「不自动删」，则在设计文档与 README 里注明该文件已无用途即可（见 §9-④）。

### 4.5 死代码与依赖核对

- 删目录后，`http_get_string_gzip` / `BODY_TOO_LARGE_MARKER` / `body_too_large` 变成零调用；crate 没有全局 `dead_code` allow，`cargo clippy --all-targets -- -D warnings` 会拦截，必须一起删。
- `flate2` 不能删：解包 tarball 与托管 Node 安装仍在用。
- 删完跑 `cargo fmt --check` / `cargo test --locked` 相关筛选 / `cargo clippy --locked --all-targets -- -D warnings`，确认无未使用 import / 函数。

## 5. 测试与判据改造

### 5.1 Rust

- 删除 `plugins::catalog` 的 11 个单测（整文件）。
- 删除 `center.rs` 的 4 个目录单测（§4.1）。
- 删除 `pkg/releases.rs` 的 gzip / 解压体积 / 明文体积共 7 个单测与 `gzip()` 夹具；`serve_once` 仍被 registry 路由测试使用，保留。
- 保留并继续跑的：`parse_spec`、`npm_candidates_*`（5 个）、安装/物化/接线/更新/预检相关用例。
- 定向：`cargo test --locked plugins::`、`cargo test --locked plugins::center`、`cargo test --locked pkg::releases`。

### 5.2 前端测试

**删除**

- `ui/test/pluginCatalogSearch.test.js`（整文件 226 行，10 个用例）。
- `ui/test/designAlignment.test.js` 中与目录/双栏绑定的用例：
  - 「插件右栏通过两个页签分别显示插件中心和手动安装」（约 1808-1843）
  - 「插件右栏标题与两栏分界采用刻蚀线」（约 1844-1858）
  - 「「刷新数据」只有一枚，且必须落在右栏远端目录那一行」与「刷新数据在分组行里靠右」（约 2353-2402）
  - 「「插件中心」右侧的辅助信息比标题小一档」（约 2908-2921）——**改写而不是删除**：`.section-divider__note` 仍被新安装块头的右侧说明使用，断言改成新模板里的那一处
  - 「插件页：外层卡叫「插件管理」，右栏那块头叫「远端目录」…」按新结构重写（保留卡名与「同一东西一个名字」的判据，删除右栏/页签断言）
- `designAlignment` 里 `.btn-action` 计数与点名（约 945）：「刷新数据」换成「预检并安装 / 安装」按钮。
- `ui/test/pluginInstallCleanup.test.js:12-20`：第一条断言钉死了 `@keyup.enter="precheckOn ? precheckPlugin('') : installPlugin('')"` 这串字面量；改成新结构后断言同一函数被按钮与回车共用（例如 `installFromSpec`），失败消息里的「插件仓库按钮」改「安装按钮」。

**保留不动（复核即可）**

- `designAlignment`：「插件页不再有整宽 notice 条」/「警告图标文案同一份」/「可聚焦」（约 1436-1480）、「插件页页签与内容之间有真间距」（2181）、「内置插件行是当前内核列表第一行」（2205）、「内置插件紫色 chip」（2290）、「药丸页签特异度」（2887）、「卡头 ⓘ 是纯图标」（1893，若改 `<h2>` 需复核）。
- `cardChrome.test.js`（插件卡刻蚀线）、`panelSpacing.test.js`（`.panel` gap 12）、`builtinPlugins.test.js`（内置行 loading key / 降级提示）、`openaiAccount.test.js`（账户卡）、`applyPluginChange.test.js`（预检两阶段）、`updateChecks.test.js`（更新检查）、`diagnosticActionBoundary.test.js`（`precheckPlugin` 边界）。
- `e2e/`：三个 spec（shell / console / memory）都没有触达插件页或目录，无需改动；实机验证按 §5.4 单独做，不写进门禁。
- `ui/test/clickHandlerArity.test.js`、`epStyles.test.js`、`diagnosticLabels.test.js` 只在注释里提到旧目录控件，落地时顺手改注释，不影响判据。

**改写 / 新增**

- 插件页模板不得再出现：`插件中心`、`dshfind`、`plugin_catalog_search`、`sourceTab`、`plugin-center-col`、`catalog-list`、`catalog-version`；`plugins.js` 不再导出 `searchCatalog` / `loadCatalog` / `catalogCategories`。
- 新结构：pane 顶部有「安装插件」块头 + `.install-row`（`v-model="pluginStore.spec"` + `.btn-action` 主按钮）+ 说明行；回车与按钮同一函数；按钮在预检开/关时文案不同。
- 单栏：插件模板内不再有 `.page-layout`；清单块头 + 空态文案断言。
- 保留并复核：「插件页页签与内容之间有真间距」（2181 附近）、「插件页不再有整宽 notice 条」（1436 附近）、「内置插件行是「当前内核」列表第一行」（2205 附近）、「内置插件来源 chip」（2290 附近）、空态描述限宽（1087 附近）。
- `designAlignment` 中凡引用 `.plugin-center-title-row` / `.plugin-center-title` 的断言，随 §3.3 删改：`:931` 的 `.btn-action` 计数与点名改为安装按钮；`:2539` 的 `.plugin-center-title` 字号断言删除或改钉 `.card h2`（`:2530` 已有同一断言）；`:2908` 的 `.section-divider__note` 改钉新安装块头。
- `ui/test/epStyles.test.js`、`ui/test/diagnosticLabels.test.js`、`ui/test/clickHandlerArity.test.js` 只在注释里提到插件中心 / `pickCategory` 这类目录控件；判据本身不依赖目录，落地时顺手改注释即可，不强制。

### 5.3 门禁与静态检查

- `npm run check:invariants`：命令注册 / ACL / 前端 invoke 三处一致（§4.3）；另查文档与源码判据中的旧路径名（`check-invariants` ⑦-之六限制前端文案出现已搬迁目录名，本次不涉及新路径）。
- `npm run check:duplicates`：删除大段后重复检测结果只会变少，不需要改判据。
- `check-invariants.mjs:1714` 的注释把「插件中心的 dshfind.com 外链」当作 `href` 允许外链的例子；功能移除后例子失效，改成别的外链例子即可（纯注释，无判据影响）。
- `check-invariants.mjs:2865-2924` 的 AGENTS.md 字节预算：`ui/AGENTS.md` 当前约 63774 字节、上限 64000，只剩约 226 字节；落地时先删 `ui/AGENTS.md:540-542`（约 388 字节）再改写 `:275` / `:552`，**不要把移除经过写进 AGENTS**（故事放 docs）。`src-tauri/AGENTS.md` 无目录相关段落且余量约 248 字节，本次不动。
- `npm run build:ui` 与 `npm run check:ui-bundle`：产物应变小，预算无需调整。
- `npm run test:ui` 按 glob 收集 `ui/test/*.test.js`：删掉 `pluginCatalogSearch.test.js` 后不会再有孤儿脚本，无需改 `package.json`。
- 日常按改动范围跑相关测试与 `cargo check`；**全量 `npm run check` 只在发布前整跑一次**（根 AGENTS.md 的口径）。

### 5.4 实机验证

按根 AGENTS.md 的壳实跑隔离要求：同时设置临时 `DSH_XLINK_HOME`、编译期 `TAURI_CONFIG` 的 `app.appDirectoriesOverride` 与独立 `DSH_XLINK_CHANNEL_SUFFIX`；探针用隔离浏览器档案，证据保存在临时 home 之外并独立命名。

最小验证清单：

1. **离线打开插件页**：断网启动壳，进入插件页，首屏不再出现目录加载占位/错误；本机清单与更新检查（静默失败）正常。
2. **手动安装（预检关）**：装一个 npm 包（如 `@ace-zone/dsh-market`）→ 进度浮层 → 清单出现 → 重启工作台生效。
3. **手动安装（预检开）**：同 spec → 沙盒两段报告 →「应用变更」→ 清单出现；报告文案不再出现「插件中心」。
4. **Git 来源**：`owner/repo` 简写与 `https://github.com/...` 各装/卸载一次，确认 GitHub→npm 候选回退仍工作。
5. **更新 / 模式 / 同步 / 卸载 / 隔离恢复**：在「当前内核」tab 各点一次，状态与产物符合预期。
6. **多实例 chip**：dev 与 release（或两个实例）状态差异正常显示。
7. **内置插件**：开关、账户卡、上游「检查更新」、已接线提示均不变。
8. **旧缓存清理**（若采纳 §4.4）：预置一份 `plugins-catalog.json` 后启动，文件被删除且启动无告警；失败路径（只读目录）不阻断启动。
9. **主题与截图**：暗色 / 浅色各截一张插件库页与当前内核页，与原型对位。

## 6. 文档更新

| 文件 | 位置 | 动作 |
| --- | --- | --- |
| `README.md` | 113-114（插件管理 / 预检 bullet） | 删掉「「插件仓库」分组对接 dshfind.com…」整句；预检「可在插件中心关闭」改「可在插件页关闭」 |
| `README.md` | 232-233（使用步骤 4） | 重写为「插件 → 插件库 → 安装插件」手动安装流程；删除搜索/分类/排序/显示更多描述 |
| `README.md` | 约 118（插件面板信息架构 bullet） | 改成单栏 + 安装区块 + 两 tab 的新描述 |
| `docs/features/extensions/plugin-management.md` | 6-21 状态段、54-56 目录布局、133-158 社区目录 | 删除社区目录 / gzip 取源整章；目录布局删 `plugins-catalog.json` 一行；补一行「2026-10-31 插件中心已移除，安装只保留手动入口」 |
| `docs/architecture/architecture.md` | 359、387、475-481 | 删缓存文件与「各写各的」表行；插件页 IA 段按 §2.1 重写；删「插件仓库 / dshfind / plugin_catalog_search」各句 |
| `docs/architecture/kernel-data-layout.md` | 30 | 删 `plugins-catalog.json` 行 |
| `docs/operations/troubleshooting.md` | 40、43 | 「插件中心 → 同步」改「插件页 → 同步内核」 |
| `docs/features/diagnostics/runtime-diagnostics-design.md` | 231、434、436、1327、2016 | 「插件中心」改为「插件页 / 安装入口」；预检入口描述同步 |
| `docs/features/diagnostics/runtime-diagnostics-design.md` | 2138 | **保留**：预检回滚事故的历史复盘，不改写 |
| `docs/features/diagnostics/safety-net-design.md` | 44 | 来源列「插件中心安装/卸载/切模式」改「插件页安装/卸载/切模式」 |
| `docs/features/extensions/skill-management.md` | 83、366 | 删「与插件侧 `plugins-catalog.json` 同构」的规划类比；改成「原插件侧目录机制已移除，技能目录若实现需另立设计」；技能自身「无社区目录」的现状不变 |
| `docs/features/extensions/plugin-internals.md` | 3 | 设计层说明删「目录浏览」 |
| `docs/features/extensions/plugin-management.md` | 165 | 安全边界删「目录条目来自社区，未验证标记原样展示」；「raw.githubusercontent 的目录缓存」若指参考市场回退则删，若指 GitHub 元数据缓存则保留并澄清 |
| `docs/architecture/architecture.md` | 24 | `plugins.rs` 职责删「社区目录」 |
| `docs/architecture/architecture.md` | 194 | 死规则排查举例里的 `.catalog-enter-*` 随 `<TransitionGroup name="catalog">` 删除而失效，换成仍存在的 Transition 例子 |
| `docs/architecture/kernel-data-layout.md` | 183 | 分家表「安装树 / `active.txt` / 社区插件缓存」删「社区插件缓存」 |
| `README.md` | 368 | 安全 bullet 里「社区目录条目保留「未验证」标记」改为「第三方插件请自行确认来源」 |
| `ui/AGENTS.md` | 275-280、540-541、552 | 同一名字规则里删「右栏 / 插件中心」表述；目录约定段按新结构重写；目录说明「插件中心、补丁、安装预检」改「插件库、补丁、安装预检」 |
| `docs/ui/panel-decisions.md` | 文末追加 | 追加一条 2026-10-31 变更记录（移除目录的取舍与理由），**不改写历史条目** |
| `docs/ui/dsh-xlink-ui-redesign-dev-guide.md` | 131、146-155 | 删掉「右栏两个页签 / 不要删成只剩粘贴地址」的旧要求，改为指向本设计稿；插件页这一段以新稿为准 |
| `docs/ui/dsh-xlink-ui-redesign-draft.html` | 2775 附近 | **不重画历史稿**；只在 dev-guide 注明该块已被本设计稿取代，避免有人按旧稿恢复目录 |
| `docs/reviews/agent-lessons.md`、`docs/reviews/code-review-*.md` | 历史记录 | 保持原样（2026-10-24 目录体积事故、`v-loading` 事故等）；可选在 agent-lessons §31 标题下加一句「插件中心已于 2026-xx 移除，本条保留为历史与通用教训」 |
| `docs/incidents/**` | 历史记录 | 保持原样（无目录命中） |
| `src-tauri/AGENTS.md` | 预检节（118-121） | **不改**：没有目录相关段落，且文件余量约 248 字节；用户可见文案改在前端与 `precheck.rs` |
| `docs/README.md` | features/extensions 索引（约 36 行） | 登记本设计稿（标注未实现）；落地后同步标注状态（按同文件「新增专题在本索引登记」的规则） |

## 7. 分阶段实施计划

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| P0 设计确认 | 确认 §9 决策；原型与文案定稿 | 用户确认 |
| P1 后端切除 | §4.1 全部删除 + §4.2 保留项核对 + §4.3 命令/ACL 同删；`reconcile_store` 缓存清理（若采纳） | `rg '插件中心|dshfind|plugin_catalog|plugins-catalog' src-tauri` 只剩注释/空；`cargo fmt --check`、定向 `cargo test`、`cargo clippy --all-targets -- -D warnings` 全绿 |
| P2 前端改造 | §3 全部改动；同步改 `ui/test`；`npm run build:ui` | 界面与原型对位；手动安装（预检开/关）、更新、同步、卸载、内置开关实机通过；相关 `ui/test` 全绿 |
| P3 文档收口 | §6 全部文档；`docs/README.md` 标注；`ui/AGENTS.md` 与新结构一致 | `rg '插件中心|dshfind|plugin_catalog'` 在代码、README、现行设计文档中归零（历史复盘除外） |
| P4 全量门禁与发布 | 版本发布前跑一次 `npm run check`；实机证据归档；发布说明写清「插件中心已移除」 | 全绿 + 实机截图 + 缓存清理验证（若做） |

建议 commit 拆分：P1 一笔（后端 + ACL + 测试）、P2 一笔（前端 + UI 测试）、P3 一笔（文档）；每笔都可独立回退。

## 8. 风险与回滚

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 失去应用内发现能力 | 用户不知道装什么 | 明确的产品取舍；README 保留「任何 npm 包 / Git 仓库」说明；外部浏览站点不受影响 |
| 命令 / ACL / UI invoke 漏一处 | `check:invariants` 红或真机 ACL 静默拒 | 三处同一提交删除；CI 门禁会拦 |
| 死代码残留 | `clippy -D warnings` 红 | §4.1 / §4.5 已列全；用 `rg` 复核 `http_get_string_gzip` / `BODY_TOO_LARGE_MARKER` 零引用 |
| 误删共享安装逻辑 | 手动安装退化或行为改变 | §4.2 白名单；`npm_candidates_*` 测试保留；安装链实测 |
| 文档 / 测试面广，漏改失真 | 用户与后续开发被旧文档误导 | §5 / §6 清单逐项勾选；P3 用 `rg` 收口 |
| 旧缓存文件残留（约 12 MB） | 白占磁盘 | 采纳 §4.4 一次性精确删除；未采纳则文档注明 |
| GitHub→npm 候选捷径仍在 | 手动填 GitHub URL 可能装到同名 npm 包 | 这是既有行为，本次只改注释；若收紧另立改动（§9-⑤） |
| 回滚 | — | 三笔独立 revert 即可；缓存删除不可逆但内容可再生，不涉及用户数据。**不要**回滚为「只删 UI、后端留着」的中间态（ACL 与代码会残留死代码）。 |

## 9. 待确认决策

1. **页签改名**：建议「已安装」→「插件库」，与清单块头「已安装」区分；备选保留「已安装」、清单块头改「本机插件库」。默认按建议执行。
2. **安装区块位置**：建议固定在「插件库」tab 顶部（本设计）；备选一是移到卡片头常驻（两 tab 都能装），备选二是弹窗（点击「安装插件」按钮再填）。后两者都会增加常驻高度或一层交互，默认不采用。
3. **清单块头**：默认保留「已安装 · N 个插件 · 按实例列出装载状态」；若嫌啰嗦可只留「已安装 · N 个插件」，实现时二选一。
4. **旧缓存文件**：默认在 `reconcile_store` 一次 best-effort 删除；备选是不自动删、只在文档说明。删除的是壳自己写的可再生缓存，风险低。
5. **GitHub→npm 候选捷径**：默认本次不动（只改注释、保留测试）；若希望手动安装严格「填什么装什么」，另立改动收紧，不在本次范围。
6. **插件少时列表留白**：默认保留内容高（与现状一致）；若希望卡片更饱满，改为清单容器 `flex: 1 1 auto` 撑满并在内部滚动，代价是空态虚线框会拉成整块区域。

## 10. 附录：改动面索引

**前端**

- `ui/src/plugins/PluginsPanel.vue`：模板 369-760、脚本 18-360、样式 964-1264 中的目录 / 双栏部分。
- `ui/src/plugins/plugins.js`：13-248（目录状态与搜索）、357（漂移 toast 文案）、模块头注释。
- `ui/src/theme.css`：2351-2374（版本徽标合并选择器）、2852-2993（插件中心小节）。
- `ui/src/App.vue`：186-195。
- `ui/src/plugins/PrecheckDialog.vue`：62、64。
- `ui/src/main.js`：197-201（注释）。
- `ui/test/pluginCatalogSearch.test.js`（整删）、`ui/test/designAlignment.test.js`（§5.2 各处）。

**后端**

- `src-tauri/src/plugins/catalog.rs`（整删）、`plugins/mod.rs:8`。
- `src-tauri/src/plugins/center.rs`：33、50-59、232-261、2678-3014、3262-3265（注释）、4072-4424（测试）。
- `src-tauri/src/lib.rs`：约 598-601。
- `src-tauri/permissions/app-commands.json`：`plugin_catalog_search` 条目（约 170 行）。
- `src-tauri/src/pkg/releases.rs`：388-505、1043-1050、1052-1098、1209-1311。
- `src-tauri/src/commands.rs`：2799-2801（注释）。
- `src-tauri/src/plugins/precheck.rs`：122（注释）、338、407、521（用户可见 hint）。

**数据**

- `<xlink_home>/<family>/desktop[-dev]/plugins-catalog.json`（一次性清理，若采纳）。
