# 面板决定经过（存档）

2026-10-14 从 `ui/AGENTS.md` 拆出。拆分原因是体积：运行时把项目指令渲染进上下文
有字节预算（内核配置的 `agent-instructions.maxBytes`，现为 65536），而当时这份
文件 114032 字节，**尾部（约 43%）从来没被读到过**。

这里保留每条决定的时间、来源（用户要求 / 截图 / 实测）与被否掉的方案。**规则本身
在 [ui/AGENTS.md](../../ui/AGENTS.md)**——改规则改那里，这份只作追溯用，不再更新。

---
# AGENTS.md — `ui/`（管理面板前端）

根 [AGENTS.md](../AGENTS.md) 的前端分支。根文件里的「范围」「数据目录」「发布」各章对前端同样适用，这里只收改 `ui/**` 之前要知道的约束。后端见 [src-tauri/AGENTS.md](../src-tauri/AGENTS.md)。

技术栈：Vue 3 + Element Plus 单页应用，Vite 构建到 `ui/dist/`（即 `tauri.conf.json` 的 `frontendDist`）。

## 设计系统（uiv2 改版，2026-10-07）

设计稿 `docs/ui/dsh-xlink-ui-redesign-draft.html` + 说明 `docs/ui/dsh-xlink-ui-redesign-dev-guide.md`。**实现以源码行为为准，设计稿只表达布局与信息层级**——不要为了填满卡片去加当前版本没有的状态、按钮或统计数据。

- **窗口固定 1040×748，不可缩放**（`tauri.conf.json`）。侧栏 224px，收起态 64px；主区宽决定了绝大多数页面是双栏栅格而不是单列。改窗口尺寸要连带改各面板的 `min-width`。
- **层级只有三步**：canvas（整窗底）→ surface（卡片）→ chrome（标题栏 / 侧栏）。**不再有半透明玻璃与背景网格**（旧版的 `.app-bg` 光晕 + 13.5px 网格已整体删除），卡片是不透明实色 + 1px 描边 + 8px 圆角。
- **明暗双主题，判据只有一个：`html.dark`**。`shell/theme.js` 读一次 localStorage 并落成这个 class，Element Plus 自带的暗色变量（`theme-chalk/dark/css-vars.css`，选择器同样是 `html.dark`）因此一并生效——**不要另造 `html[data-theme]`**，那会让组件库留在浅色变量上，出现「壳变了、弹窗还是白的」。theme.css 里 `:root` 放浅色 token、`html.dark` 放暗色 token，两套 Element Plus 覆写各自待在自己的选择器下，没有第二处判据。
- **token 一律用语义名**：`--surface` / `--surface-raised` / `--surface-subtle` / `--surface-loading` / `--border` / `--border-soft` / `--text` / `--text-secondary` / `--text-muted` / `--accent` / `--accent-strong` / `--accent-soft` / `--success` / `--success-strong` / `--warning` / `--danger` / `--danger-strong`（另有五个 fill 档：`--accent-fill` / `--success-fill` / `--warning-fill` / `--danger-fill` / `--builtin-fill`，`--builtin` 是内置插件的标记色，与前四个语义色同一纪律：两套主题各给一份）。旧名（`--card` / `--bg` / `--muted` / `--good` / `--bad` / `--warn`）已在本次改版里全局改名完毕，**不要再写回来**。
  - **遮罩有两种语义，不许合并成一个变量**（2026-10-09）。`--el-mask-color` 是**全屏**遮罩（`el-dialog` / `el-message-box` 的 `.el-overlay`、壳自己的 `.progress-overlay`）的**压暗**档；`--surface-loading` 是**内嵌**加载遮罩（`v-loading` 的 `.el-loading-mask`）的**盖住表面**档。EP 把两者绑在同一个 `--el-mask-color` 上，此前本仓只调了浅色那一档，于是插件中心搜索目录时整块加载区在浅色主题下被盖成深灰。判据是两条互为反向的测试，在 `ui/test/epStyles.test.js`。
    - **`.el-loading-mask` 的规则必须写在 theme.css 的全局作用域**，不能挪进任何组件的 scoped 块：`v-loading` 的 mask 是指令运行期插入的新 DOM 节点，身上没有组件的 `data-v-*`，scoped 形式匹配不到它。
- **页面原语**在 theme.css 的「新版页面原语」一节：`.page-head`（页头：标题 + 说明在左、动作在右）、`.page-head__text`（**页头里包住标题与说明的那一层**，主副标题同行的排布由它承载）、`.page-title-row`（标题与它旁边的提示图标同行）、`.head-tip-icon` 及 `--warning` 修饰（标题旁的补充 / 警告气泡触发点）、`.page-layout`（双栏栅格，`__col` 是一列，`__full` 横跨整行）、`.page-list` / `.page-row`、`.btn` 及 `--primary` / `--secondary` / `--ghost` / `--danger` / `--icon`、`.nav-item`、`.plugin-tab`。**先找现成原语，再写 scoped 样式**；每个面板自造一套的结果是六个面板六种圆角。
  - **`.page-card` / `.page-card__head` / `.page-card__body` 已删除**（2026-10-08）。它们来自设计稿，但 `ui/src` 的模板与脚本里**零引用**——实现走 `.card` / `.card-head` / `.page-list`（稿子里那几处写成 `class="card page-card"`，真正起作用的是 `.card`）。20 行死 CSS，而本清单此前还把它们登记成已有原语，正是本节下一条要避免的形状。**设计稿里有 ≠ 实现里该有**。
  - **登记进这份清单的类名，必须真有模板在用。** ce9dd08 曾把 `.health-row` / `.health-dot` / `.health-row__value` 写进 theme.css 并列进本清单，但控制塔的「系统健康」实际走的是 `diagnostics.css` 的 `.diag-row` 家族——三组规则 38 行全是死代码，2026-10-07 删除。**本清单是「已有原语」的索引，不是「打算加的原语」的许愿单**：加一条之前先 grep 一遍模板，确认真有引用再登记。原语名要和设计稿对得上（设计稿那套叫 `.health-name` / `.health-value`，和当时写进去的 `.health-row__value` 也不是一回事）。
  - **`.metrics` / `.metric` 已从全局原语清单里移除**，它们归 `OverviewPanel.vue` 的 scoped 块。原先 theme.css 那条全局 `.metric`（描边 + `--surface` 底 + `10px 12px` 内边距）与 scoped 的「上边线 + 后两格左边线」叠加，把设计稿的一条分隔线变成了三个独立卡片——**全局基线与组件覆盖各写各的，冲突只有走层叠才看得见**。
- **构建标识（dev 鲸眼红 / release Gitea 绿）收成窗顶 2px 细线**，由 `--build-color` 驱动，两种构建共用同一条 `.app-shell::before` 规则只换颜色变量。旧版那条「铺满全高的品牌色渐变带」已删除——它与卡片描边互相拍频。`ui/test/diskUsageAndChrome.test.js` 钉住了「只有这一处画法」。
- **技能页是「已安装 / 社区资源」两张卡**（2026-10-07 对齐设计稿 2817-2820 行）。原先手动安装是「已安装」卡底部的一条虚线分隔加输入框，读起来像「装完了顺手在这里补一个」，而它其实是另一件事——包的来源在社区，与本地已装状态无关。**设计稿第一张那张「技能状态」卡不画**：它写着「启用状态 4 / 4」「活动视图 已同步」，后者没有任何后端字段支撑，硬写就是编数据（设计说明「不能从设计稿推断出的内容」点名了「未在后端返回的余额、百分比、更新时间、星标数量或插件数量」）。
  - **按钮按动作类型选 loading 通道**：`installSkill` 走 `withProgress`（长任务），**不经过 `withLoading`**，所以安装按钮上挂 `isLoading('installSkill')` 永远为 false——那是一个永远不转的假 loading。在途状态由进度浮层表达，按钮只做 `globalBusy || 输入为空` 的禁用。
  - **单页专用样式留在 scoped**，共享样式优先复用。输入行直接复用全局 `.install-row`（`.el-input` 自带 `flex:1`），卡内标题与说明各合成为一条基线规则（`.community-title` / `.community-meta`），不按设计稿那样一份一套。技能社区与手动安装之间只画一道 2px 刻蚀线，由 `.community-install::before` 承载，浏览区下边框和安装区上边框显式清零。
  - 判据只看**模板块内**（注释已剥）：源文件注释里正是在解释「为什么不画技能状态卡」，按整份文件搜会把判据自己写的话算成命中。
- **无文字按钮（icon-only）：背景范围要小，且必须有 hover 说明文字**（2026-10-10 用户定规范）
  - **加 `icon-btn` 类**，尺寸 26×26（theme.css 的全局规则）。**根因是 EP 的默认值不能用**：`el-button` 基础规则带 `padding: 8px 15px` + `height: 32px`，而 `is-text` 只把背景换成透明、**padding 原样保留**——于是一个 14px 的图标被撑成约 44×32 的一块，视觉重量和旁边带文字的按钮相等，而它其实只有一个图标。用户 2026-10-10 指着内核版本页那两个按钮报的就是这个。
    - 命中区靠 `::after` 补到 32×32 **居中**外扩：视觉背景小、真实命中区不缩水。相邻两个按钮的命中区会重叠 4px，取交集后两侧各让 2px，仍在 Windows SMALL_CAPTION 的 24px 下限之上。
  - **`.entity-action` 不归这条管**：插件 / 技能列表行的紧凑动作另有 30px 规则，是 2026-10-08 用户要求「按钮放大」定下的**有意尺寸**。本条治的是**没人管过、吃 EP 默认值**的那批，一刀切缩小会把那次拍板撤销掉。
  - **`title` 与 `aria-label` 都要有，两者缺一判据就红**（`check-invariants` 的 `a11y-icon-button` 项）。这不是洁癖：`el-tooltip` **只认鼠标悬停**，键盘与读屏用户永远看不到它；`title` 是唯一对键盘也生效的那份 hover 说明，`aria-label` 是读屏的可访问名。图标本身不是文本，所以两者都不能省。
    - 判据按「**自闭合 el-button（插槽为空）且有 `:icon`**」判定 icon-only——这才是它的定义，而不是 `circle` 这一种画法。写判据时这里踩了三次坑：惰性匹配会跨过有文字的按钮吞到下一个自闭合标签（误报 20 处）、用 `>` 断配对拦不住闭标签（误报 8 处）、用 `</` 断配对又拦不住**另一个开标签**的 `<`（反过来漏报）。现在靠「捕获块内不允许出现任何 `<`」分开。**假门禁比没有门禁更坏**：它让人以为报出来的都是真问题，从而忽略真正的问题，所以每版都要反向验过（破坏真实的 icon-only 按钮，确认判据变红）。
- **判据要看「叫什么」而不是「长什么样」，同一屏里同一个东西只能有一个名字**（2026-10-07 逐页比对时清掉的三处）：
  - 内核版本页的卡头叫**「已安装 / N 个版本」**，不叫「内核版本」——页面标题已经叫内核版本了，卡头复述一遍等于把整页的名字说两遍。组内那个 `<h3>已安装</h3>` 一并去掉，否则同一个词连着出现在两行。发布列表里那个行内 `<el-tag>已安装</el-tag>` 是**另一个含义**（标出这个远端版本本机已经有了），别跟着禁。
  - 插件页外层卡叫**「插件管理」**，右栏那份 dshfind.com 目录才叫**「插件中心」**。原先外层卡叫「插件中心」、右栏叫「插件仓库」——同一份目录，同一页两个名字。
  - **提示图标的可见文字必须和它 tooltip 里的内容是同一件事。** 插件卡头原先 ⓘ 旁边写着「数据来源于 dshfind.com」，tooltip 里讲的却是插件存放路径与生效规则——停在字上弹出另一段话。现在是纯图标。
- **设置页左栏是工作台、后台常驻、环境回退与诊断，右栏是外观、任务通知**（2026-10-07 用户拍板，对齐设计稿 draft 2844 行；**2026-11-02 又移回右栏末尾，见文末「设置页两栏配平」；2026-11-05 起按平台挂载，见文末「设置页：环境回退与诊断按平台换栏」**）。原先右栏四张（任务通知 / 数据迁移 / 环境回退点 / 深入排查）把右栏拉到两屏高、左栏空一大截；后三者同属「环境出问题时才动」，合成一张之后两栏才配平。卡内保留三个 `.page-list-row`：**环境回退点 / 深入排查 / 启动诊断**；数据迁移从侧栏进入。
  - **`SnapshotCard` 与 `BisectPanel` 因此渲染成 Vue Fragment**，根节点就是那一行，明细与告警是它的兄弟节点，直接落进同一个 `.page-list` 栅格。它们曾各自是 `<div class="card …">`，套进共享卡里就会出现卡中卡。判据查的是**形状**（整页只有一张卡 / 没有两列栅格），不是某个类名——设计稿那份 `.versions-layout` 从未进过 theme.css，拿它当判据等于断言一个本仓库不存在的符号。
  - **收起的只有明细，状态不许藏**。「查看 / 收起」默认收起的是那一屏十几行列表；状态说明（`headline(view)` / `bisectHeadline(view)`）挂在 `.page-list-meta` 常驻，`drifted` 与 `conclusion` 两条告警**不进闸门**——排查跑几分钟时，用户唯一能看懂的进度恰恰不能被折叠起来。四个动作（回到良好状态 / 刷新 / 开始排查 / 停止）全部留在常驻行上，「查看」只开明细。
  - **`.page-list-meta` 故意不抄设计稿的 `nowrap` + 省略号**（draft 1459-1470）。这一处的次行是 `headline(view)` 那类整句说明，截成一行就分不出「还没成功启动过」和「文档损坏」。
  - **收尾那条的分隔线收在 `.page-list > *:last-child` 上，不是 `.page-list-row:last-child`**：Fragment 展开后明细也是列表的直接子级，按行判 `:last-child` 会把展开内容当成收尾行，连带抹掉它上面那条分隔线。设置页用 `.diagnostics-list` 的 scoped 样式将行边框替换成 2px 蚀刻线，颜色为 `--divider-strong`，六边形两端收细；通过 `:deep` 覆盖 Fragment 子组件的行，仅非收尾行画线。
  - 「启动诊断」这一行是**补上的入口**（设计说明 §设置、恢复与迁移 的覆盖清单里有「二分排查 / 启动诊断」，而设置页此前只有概览 / 进度浮层 / 事故弹窗三处入口）。它直接打开诊断窗口，不是有内容的展开层。
- **内核版本页维持「一张卡竖排」，不改设计稿的两列栅格**（2026-10-07 用户拍板，**有意保留的偏差**）。设计稿 draft 709-728 行给的是 `.versions-layout` 两列栅格。没跟过去的理由不是「懒得改」，而是这一屏有三处行为依赖当前竖排、且都已调稳：发布列表的 **z-index 层叠**、**淡出带移除后的裁切行为**、以及为 **120px 限高**量过的磁盘区 6+9px 间距。改两列要连这三条一起重调，收益只是左右各短一点。判据钉的是「整页只有一张卡、没有两列栅格」这个形状。
- **内核版本页两栏之间那道竖线不贯通**（2026-10-07 用户要求「左右功能块中间增加纵向分割线，不贯通」）。右栏是带内部滚动的长列表，它自己的 `.release-list-box` 本来就有边框与底色，边界并不缺；线一路通到卡片底边，这一屏就被读成**两个并排窗格**，而不是一张卡里的两组清单。
  - 线的高度只跟着**左栏「已安装」那几行**走：做法是左栏在非空时 `align-self: start` 按内容高度收，线挂在 `.installed-list` 右缘外侧、落进 8px 栅格缝里（`left: 100%` + `margin-left: 3px`，偏左是因为右边紧挨着 `.release-list-box` 自己的 1px 边框，贴着画会并成两条）。
  - **竖线沿用刻蚀线标准**：宽 2px、颜色 `--divider-strong`，上下两端通过六边形收细，几何与概览指标竖线一致。仍用 `::after`，不改左栏高度规则。
  - **不许用 `border-left`**：边框属于元素自身，元素被拉多高线就有多高——那正是要避开的那条通栏线。所以判据显式断言用的是 `::after` 而不是 border。
  - **空态是唯一的例外**：`.installed-list` 必须继续伸展，否则下面那条 `.installed-list:has(> .el-empty) { justify-content: center }` 没有可分配空间、变成空操作，`el-empty` 会被顶回标题下方、下面留半栏空白（它当初就是为了修这个才写的）。所以选择器带 `:not(:has(> .installed-list > .el-empty))`，空列表那一栏的线仍与整栏齐平。
  - 判据在 `ui/test/designAlignment.test.js`（2 条）。其中一条**不拿调用点传进去的选择器当证据**——那只是判据自己的字面量，改判据它照样绿；它从文件里把「真正设了 `align-self: start` 的那条规则」捞出来，再看那条规则的选择器有没有空态豁免。
- **顶部工作条（`.workspace-bar`）已整条删除**（2026-10-07 用户指着一张截图说「移除这个区域」，**覆盖设计稿**）。稿子 draft 2516 行是有这条 48px 行的（左边内核族页签、右边「当前实例 + 运行状态」），现在 `KernelTabs.vue` 整个文件、它在 `App.vue` 的挂载点、以及只为它定义的 `--workspace-bar-height` token 全部删除。
  - **它从第一天起就是空操作**，这是删它而不算丢功能的依据：页签按 `kernel_family` 去重，而后端只定义了 `dsh` 一个族（`mcode` 在 `paths.rs` 的注释与测试里都写着「将来的」）。一个族只会画出一个页签，而它必然就是当前那个，于是 `pickInstance` 每次都在 `defaultInstanceId === id` 那一步 `return false`。
  - **`instance.js` 的 `setDefaultInstance` 不许跟着删**——这一条是被测试顶回来才写下的。先前的版本把它连同 `instanceStore.switching` / `selectionRevision` 当死代码清掉，`ui/test/kernelSwitch.test.js` 当场变红（`setDefaultInstance is not a function`），顺带把整个 `test:ui` 拖挂六分钟。它不是「永远走不到的代码」，而是**没有调用方的 API**：那条测试钉着三条真实语义（读实例去重、**陈旧列表不能把一次切换的结果冲掉**、写入失败必须释放 busy），删函数就要连这套语义一起删，那是拿测试换行数。
    - **「没有调用方的 API」和「永远走不到的代码」是两回事**，前者留着、后者才清。判据钉住的不变量是：模板侧零调用 + 函数与 `selectionRevision` 仍在。
  - **删掉的是「界面上的切实例」，不是「运行状态」与「实例清单」**。后端 `set_default_instance` 仍在，运行状态胶囊（`.status-pill`，theme.css 共用词汇）照旧在概览页读同一个 `store.view.kernel`，插件页照旧按实例列出 `族名 · 实例 id`。代价：界面上不再有「当前是哪个实例」这一行常驻。
  - 真接入第二个内核族时切换器要重新做一遍，那时直接调 `setDefaultInstance` 即可，语义不必重新推敲。
- **概览「当前内核」卡：主操作上移，实例 caption 删除**（2026-10-07 用户要求，**覆盖设计稿**）。原先「工作台 / 官网网页版」两个按钮窝在 `.card-head-left` 里，落在标题下面那一行的**左侧**，视线要往下再折一次才找得到；现在卡头只有两栏——`<h2 class="kernel-title">` 与 `<div class="kernel-header-actions">` 同属一个 `.card-head`。占着右侧位置的 caption「DSH / default-dev」按同一要求删除：卡里已经有一个大号活动内核版本号和它旁边的状态胶囊，族名与实例 id 是实现细节，多实例是开发与过渡期的事实。`kernelCaption` 连同概览页对实例注册表的整条引用（`instanceStore` / `familyLabel` / `loadInstances`）一并移除——**`App.vue` 仍在启动时加载实例列表，插件页不受影响**。
  - 删 caption 时把那条教训留在原地：`KernelStatus` 结构体里既没有 `family` 也没有实例 id 字段（它的字段是 `installed` / `active` / `active_installed` …），从 kernel 上读那个字段取到的是 `undefined`，而 `undefined || …` 会静默落到下一个兜底分支——门禁 `check:invariants` 的 `[ipc-fields]` 项抓的就是这条。
- **功能块标题整体放大一档**（2026-10-07 用户要求「所有功能块的 title 字体大小都需要放大」，**覆盖设计稿**）。设计稿 draft 1975 行 `.card-title` 是 13px；理由是**一个块的边界正是靠标题建立的**，标题太小等于没有块。

  | 层级 | 原 | 现 |
  | --- | --- | --- |
  | 页面标题 `.page-title` | 18px | **21px** |
  | 块标题 `.card h2` | 14px | **17px** |
  | 块内小标题 `.card h3` | 13px | **15px** |
  | 行式条目标题 `.page-list-title` | 12px | **15px** |
  | 诊断层 `.diagnosis__title` | 15px | **17px** |
  | 诊断层 `.diag-card__title` | 13px | **15px** |
  | 标题旁的 ⓘ `.card-info-icon` | 14px | **16px** |

  - **ⓘ 必须跟着长**：留在 14px 会读成「一个更小的另一个元素」，而不是「这个标题的补充说明」。
  - **分区标题一并跟上**：`.callout-body h3` / `.debug-panel h3` / `.step-body h3` / `.history h3` / `.misplaced h3` / `.sub-section-head h3` / `.usage-section-head h3` 从 13-14px 抬到 **15px**，`.disk-usage h2` 抬到 **17px**，`.migration header h2` 抬到 **21px**（它是迁移页的页面标题）。漏掉任何一个，同屏里就会出现「块标题 17px、分区标题 13px」的倒挂层级。
  - **判据用一条「全表扫」而不是逐个断言**：`.card h2` 的主语是**裸标签** `h2`，全仓有十来条同主语规则（`.migration header h2` 特异度 102 直接盖过 101；`.callout-body h3` 等同特异度但源码在后、靠 `>=` 决胜又轮番抢走），逐条 `effectiveDeclaration` 必然答出别的规则的值。所以除了点断言，还有一条扫全表断言「没有低于 15px 的 h2 / h3」一网打尽。
- **判据扫模板，不扫全文**（`templateOf()`）。本仓每个模板都带着大段解释「为什么这么写」的中文注释，注释里经常**原样引用被判据的词**（「活动视图已同步」那条为什么不画、「插件中心那个 ⓘ」现在在哪）。按整份文件 `indexOf` / `doesNotMatch`，判据会把自己写的解释当成命中。**同一个坑本轮踩过三次，三次都是扫全文**。样式那边对应的是 `stripComments`，模板这边剥 HTML 注释（`stripComments` 只认 `/* */`，对 `<!-- -->` 无效）。
  - 同一条纪律对 **JS 文件注释也成立**：判「前端不再调 `set_default_instance`」时，裸匹配那个命令名会命中 `instance.js` 里正是在解释「这条命令还在后端、只是前端不再调」的注释——断言要写成 `invoke('set_default_instance'` 这种**真调用形态**。
  - 收尾边界取 `<style` 而不是 `<style scoped>`：`PluginsPanel` 没有自己的 scoped 块，写死 scoped 会在这类文件上直接断言挂掉。
- **「入口还在」不等于「文件还在」**。设计说明 §独立窗口 逐个列了十一个窗口 / 弹窗，判据把「文件存在 **且** 在自身目录之外还有挂载点」一起钉住（`WINDOW_ENTRY_POINTS`）。两条都是被反向验逼出来的：
  - 扫引用要带 `.js`：独立窗口的挂载点在 `main.js`（按窗口类型挑根组件），只扫 `.vue` 会把「已经挂在窗口上了」误判成「没有入口」。
  - **同目录的引用不算入口**：组件自家的 store 模块（`usage/usage.js` 里写着 `UsageWindow`）只是它自己的状态容器。第一版判据没排除它，结果把 `main.js` 的挂载点整段摘掉它照样绿。
- **侧栏整体放大一档**（2026-10-07 用户要求「放大字体、icon」，**覆盖设计稿**）。设计稿 draft 509-524 行给的是 `font-size: 13px` / `gap: 11px`，实现此前照抄；现在是 `.nav-item` **15px**、`.sidebar__section-label` **12px**、`.nav-item__badge` **12px / 20px 圆点**、`.sidebar__footer` **12px**，间距同步抬（行 `padding 8px 10px`、`gap 11px`、分组 `gap 4px`、收起态 `padding 8px 0`）。224px 侧栏仍放得下最长的一项（「数据迁移」四字），748px 高的窗口里侧栏总高约 450px，余量充足。
  - **图标必须显式写死**（`.nav-item > .el-icon { font-size: 17px }`），不能只抬 `.nav-item` 的字号：Element Plus 的 `.el-icon` 是 `font-size: inherit` + 1em，只抬父级字号等于图标跟着等比长——那不算「图标被放大」。17 vs 15 是刻意的：图标读起来要比文字再大一点点才不显矮。收起态（只剩 icon 列）读到的是同一条规则。
  - **品牌行的鲸鱼图标 34 → 42px**（2026-10-08 用户截图指过来：「黑鲸icon放大」，**覆盖设计稿**）。34px 与 15px 主名 + 11px 副名那一摞文字等高，读起来是「配图」而不是品牌主体；42px 高过文字块一截，它才在这一行里当得起标识。圆角按同比例 7 → 9，`.brand` 的 `gap` 同步 10 → 12。收起态 64px 侧栏去掉 8px 横向内边距后内容盒 48px，42px 仍放得下。
  - **`<img>` 的 `width` / `height` 属性必须与 CSS 相等**（`SideBar.vue` 的 42 与 theme.css 的 42）：CSS 只覆盖绘制，那两个属性是给读屏与布局的第一手尺寸，两处各写一遍迟早漂——漂了之后无障碍文本念出的尺寸与眼睛看到的不一样，而没有任何检查会响。同理 `flex: 0 0 42px` 少一条，图标会在 flex 行里被文字挤扁，变形而不报错。
- **窗口 chrome 有两套：主窗自绘、副窗走原生，两套都得跟应用主题对齐**（2026-10-07 用户一张截图同时报出两处）
  - **副窗的原生标题栏跟主题走，不再钉死深色**。根因是两套机制各走各的：原生 chrome 由系统按窗口的 `NSWindow.appearance` 画，主题真值在 localStorage，Rust 建窗时读不到——于是 6 扇副窗一律 `.theme(Some(tauri::Theme::Dark))`，浅色用户打开「模型用量」就得到**浅色内容配深色标题栏**（标题文字几乎看不见）。修法是 `applyTheme()` 顺手把同一个 `theme.value` 推给 `bridge.setWindowTheme()`，纠正**本窗口**的原生 appearance；4 个壳自有的 capability 加 `core:window:allow-set-theme`。
    - **`theme.js` 仍是 `html.dark` 的唯一判据**，`setTheme` 只经 `applyTheme` 落一次。`window.setTheme()` 收的必须是 `theme.value` 本身——写死字面量会让「切了主题只换页面、不换标题栏」，判据点名盯的就是这一条。
    - **只改本窗口的原生装饰，不批量改**（判据显式禁掉 `getAll` / `webview_windows` / `forEach`）：这条禁的是「Rust 侧批量把每个窗口的 appearance 换掉」——那样只换得来原生标题栏，换不来副窗的 `html.dark`，结果就是「深色内容 + 浅色标题栏」，同一道割裂换个方向。
    - **已开着的副窗要跟着换，靠广播而不是批量改 chrome**（2026-10-08 用户报「切换主题时，弹出的 window 也要跟着改变」）。根因是**链路缺失**，不是样式：主题真值在 localStorage，而 `storage` 事件**不跨 webview 生效**，四个副窗各自是独立 webview，主壳改完存储，它们加载时定下的 `html.dark` 不会动。此前 `theme.js` 注释写的「下一次绘制就跟着变」只在**窗口还没开**时成立。
      - 实现：`setTheme` 落地后 `broadcastTheme(next)` 发 `ui-theme-changed`（`bridge.emit`），各窗在 `main.js` 里 `followThemeBroadcast()` 订阅，收到后**自己跑一遍 `applyTheme()`**——内容与原生 chrome 同一步换，两个方向都不割裂。
      - **副窗不许只调 `setWindowTheme`**：那只换标题栏、留下旧内容。判据显式禁掉这个形状，它正是上面那条割裂的反向版本。
      - 订阅必须在 `createApp` 之前（与 `applyTheme()` 同处），晚一步副窗首帧会先画一帧旧主题再跳色；入口只调一次，四个副窗共用。副窗不调 `setTheme`（会互相触发）。
      - 收到的事件值必须过 `THEMES.includes`：事件可被同页脚本构造，非法值会让整窗落到既非 `dark` 也非浅色的主题上。
    - **官网页签窗固定深色；工作台由内核偏好镜像**（`harness_window.rs`、`official-chat-*`）：官网页签承载 `chat.deepseek.com` 这类别人的页面，保持固定深色且不授外观权限；工作台承载内核 webui，不钉建窗主题，由 `ui-theme` 决定并通过 `workbench-appearance.js` 镜像原生标题栏。判据要求工作台 capability 有 `allow-window-appearance`，官网页签 capability 不得有外观授权。
    - 壳自有窗口的建窗深色是**兜底**不是最终外观；工作台建窗主题返回 `None`，由内核偏好镜像；官网页签窗保持固定深色。——页面挂载到纠正之间有一小段窗口期（冷启动闪一下）。彻底消掉得让 Rust 也读得到主题真值，那是把主题搬进 `Settings` 的大改，本轮不做。
  - **自绘交通灯按 macOS 原生的实测尺寸，不按设计稿**。参照物是同一张截图里那扇走原生装饰的窗口，1:1 量得：原生**直径 12px、中心间距 23px、灯组左缘距窗缘 7px**；实现此前是外框 12px、间距 20px、左缘 12px。改 `gap 8 → 11px`、`padding 0 12px → 0 7px`。
    - **「偏大」的不是直径，是挤**——两边直径本来就都是 12px。间距少 3px 让三点连成一坨，加上每点一圈 1px 深色描边衬得更粗。
    - **描边从 `border` 换成 `box-shadow: inset`**：全局 `* { box-sizing: border-box }` 下 `border: 1px` 会从这 12px 里各吃掉一侧，着色区只剩 10px，比原生的 12px 色块小一圈。inset 把同样的深色环画在色块**之内**。
    - **判据钉关系而不是字面量**（`width + gap === 23`）：只动其中一个也必须转光。另有两条断言「不许退回 `border: 1px` / `border-color`」与「三盏灯都得有 inset 环和填充色」。
  - **窗口圆角与副窗交通灯都不是 CSS 画的，别去 CSS 里找**（2026-10-08 用户两张截图分别指出「副窗交通灯大小 / hover 要匹配系统」「window 边角圆角保持一致」）。查证结论与最终做法：
    - **圆角由 macOS 窗口系统按窗口类型决定**，`html` / `body` 上的 `border-radius` 改不了窗口外轮廓。实测此前主壳 ≈2px（不透明窗口系统不给圆角）、副窗 ≈10px 逻辑像素（走原生装饰，系统给）。
    - **已统一为「自绘」**（用户拍板「统一自绘、一起调」）：四扇壳自有窗口（主壳 + 日志 / 模型用量 / 套餐用量）建窗期一律 `decorations(false) + transparent(true) + shadow(false)`，圆角由 CSS 画。`transparent` 走的是「`setOpaque(false)` + `setBackgroundColor(clearColor)`」，**不碰 `NSWindowStyleMask`**；只有 `set_decorations` 才会重算样式位、抹掉 `Miniaturizable`（tao 0.35 `platform_impl/macos/window.rs` 实测，不是推断）。**2026-11-02 起 Windows 分支不再透明**——透明窗口会关掉 ClearType，见本文件末尾「Windows 字体发虚」那一节。
    - **`shadow: false` 不能省**：macOS 给每个窗口矩形画系统投影，窗口不透明时它藏在边框下看不见，透明 + 圆角后会画在**圆角之外的透明区域**，读作一圈黑色矩形光晕。投影改由 `#app::after` 的 `box-shadow` 补，形状跟 CSS 圆角走。
    - **底色必须跟着圆角一起从 `body` 搬到 `#app`**：留在 body 上，圆角外的四角仍是一块实色方块，圆角等于白做。这一条少做时**没有任何报错**——窗口只是「看着没圆角」，build 全绿。
    - 三扇副窗共用 `shell/ViewerShell.vue`（自绘标题栏 + 圆角 + 裁切 + 投影），不在三处各写一遍；它们的能力文件有 `allow-start-dragging`。
    - **副窗只留关闭按钮，最小化是主壳独占的动作**（2026-10-12 用户实机报「弹出 window 只保留关闭按钮，不允许用户最小化」）。此前后台常驻那套（`resident::intercept_close` 收进后台 / 托盘）是**主壳**的语义，副窗关掉就是关掉；Windows 上那枚最小化按钮调 `minimize_shell`，授权在 `allow-local-commands` 里只归主壳，副窗点了只会得到一句 `Command minimize_shell not allowed by ACL`——一枚必然失败、且不提示自己坏了的按钮。
      - `WindowTitleBar` 的 `close-only` prop 是**能力开关不是样式开关**：`ViewerShell` 传它 → macOS 只画红灯、Windows 只画 ✕；`minimizeWindow()` 里另有 `if (props.closeOnly) return`，模板日后被人加回按钮时命令仍发不出去。关闭键**必须还在**（判据两头都钉：删掉整组按钮同样能让「没有最小化」成立，而那扇窗就再也关不掉了）。
      - 绿点（禁用、窗口不可缩放）一并去掉：留着就是一枚点了永远没反应的按钮，与 Windows 上「不放最大化按钮」是同一条理由。
      - 关闭键的**悬浮说明按同一口径分档**：主壳写「收进后台 / 收纳到通知区域」，副窗只写「关闭窗口」——「程序继续运行」写在副窗上是在骗人，「去托盘找它」更是指向一扇从没被收起过的窗。
      - 系统层那一半在 Rust（`shell::window::decorate_transparent` 的 `minimizable(false)`，建窗期给），前端只管自绘 chrome：任务栏右键「最小化」、Win+D 这些入口绕得开模板。三份副窗 capability 里的 `core:window:allow-minimize` 同时删掉——留着它等于给一个不该存在的按钮预留后路。
      - 判据在 `ui/test/windowChrome.test.js`「副窗标题栏只留关闭按钮」：接线（`ViewerShell` 传了 / `App.vue` 没传）、两个平台的门控、`minimizeWindow()` 的提前返回、Rust 的 `minimizable(false)`、三份 capability **不许**有 minimize，四条变异都逐条反向验过。
    - **官网页签栏与工作台窗口不改装饰**：前者承载 `chat.deepseek.com` 等别人的页面，后者是内核 webui。判据钉住「只许排除官网页签栏」。
    - 判据：`ui/test/windowChrome.test.js` 的「窗口圆角与副窗标题栏」一组（含一条「入口 `main.js` 的具名 import 必须真的被导出」——拆分模块时写错 import 只有构建会报，UI 测试全绿）。
  - **副窗的标题栏显示功能标题，不显示应用名**（2026-10-08 用户实机跑完报「副窗的功能标题需要保留」）。副窗的 head 行（品牌图标 + 功能名 + 刷新等动作）是**内容区**的页头，在标题栏**下面**；标题栏若只写「Dsh-Xlink」，两行连着读就是「窗口叫什么 + 这窗装什么」两句话，而标题栏该回答的「我现在开着哪个功能」没有答案。`WindowTitleBar` 有 `title` prop，不传时回落到应用名（主壳不传）；三扇副窗经 `ViewerShell` 传各自的「日志 / 模型用量 / 套餐用量」，其中日志窗跟随当前文件名（`:title="activeName || '日志'"`，兜底不能省——没选中文件时标题会空白）。
  - **交通灯的 hover 是「整组浮现」，底色不变，符号分色**（2026-10-08 用户报「hover 后的效果要和 macOS 保持一致」，改的是两处真错）：
    - **组 hover 而非单颗**：macOS 上符号是悬停**整组**时一起浮现的。此前写 `.light:hover`，指针压在红上就只亮红，另外两颗会读成「不可点」。命中区靠 `.mac-titlebar__controls` 的 padding 撑开（灯间 11px、两侧各 7px），指针落在空隙里同样算「在组内」。
    - **底色在 hover 时不变**：此前是 `filter: brightness(1.08)`（灯变亮），原生不做这件事——灯一变亮，整排读成「有一枚被选中了」。判据显式禁掉 `brightness(`。
    - **三枚符号分色**：红 `#460804` / 黄 `#90591d` / 绿 `#2a6218`（macOS 原生值）。统一成一个深灰会在黄、绿上偏紫，那两处偏紫正是「不像 macOS」最显眼的地方。
    - **直径 12px / 中心间距 23px / 左缘 7px 维持不动**——那是上一轮从**真实原生窗口** 1:1 量出来的，比二手文章可靠（网上流传的「15px / 24px」是物理像素与逻辑点混算的结果，不可直接采信）。本轮用户实机截图按 2.17 倍缩放回算：直径 ≈12px、间距 ≈21px，与实现一致，所以「大小不对」不是尺寸问题而是 hover 问题。
  - **两套 chrome 各自按本平台走，不互相渗透**（2026-10-08 用户「windows 的保持 Windows 的风格一致」）。交通灯那组带 `v-if="isMacTitlebar"`、Windows 用 `.win-caption` 右侧按钮；模板一改（去掉那个 `v-if`）交通灯就会跑到 Windows 上，而那不是 Windows 的语言。判据两头都钉。
    - 灯组左缘 7px 来自实测、带 ±1px 不确定度，**故意不钉字面量**——钉一个自己都没把握的数，只会让后来的人以为它是精确的。
  - 判据在 `ui/test/windowChrome.test.js`（8 条）。它和其余样式判据一样**先剥注释再断言**：本节写下的 `border: 1px` 与 `12px` 正是描述根因的原话，不剥就会被自己当成命中。
- **动作按钮要跟着它作用的那个对象走**（2026-10-07 用户要求）。插件页的「刷新数据」刷的是 **dshfind.com 那份远端目录**，原先挂在整张卡（「插件管理」）的卡头靠右，等于让一个作用在右栏的按钮出现在左栏；现在落在右栏 `<h3 class="section-divider">插件中心</h3>` 那一行靠右。搬过去之后，下面那句「目录为空或加载失败，点「刷新数据」重试」才有个近处的按钮可指。
  - **按钮放在 `<h3>` 里不是新发明**：`.section-divider` 本身就是 `display: flex`，内核版本页的「官方版本」标题行里本来就有两枚图标按钮（`.list-head-with-logo`）。
  - **旧位置的规则必须删干净**：`.plugin-center-title-row .plugin-center-refresh { margin-left: auto }` 随按钮一起走。元素不在那儿了，留着就是一条没有作用方的声明——本仓已经因为「删掉声明等于让基线回来」栽过（见 §间距）。
  - 顺带改掉一条已经说谎的注释：预检开关上方原写着「与『刷新数据』同排」，而它从一开始就在独立的 `.precheck-toggle-row` 里，这次搬家之后更不成立。注释与模板不一致时**先信注释还是先信代码都要查**，这次是模板赢。
  - 判据在 `ui/test/designAlignment.test.js`（2 条）：按钮**只能有一枚**（两枚意味着同一个动作两处能点）、必须在「插件中心」那个分组标题里、不许回到 `.plugin-center-title-row`。扫的是 `templateOf()`——插件面板的注释里正是在解释这次搬家，不剥会被自己当成命中。
- **内嵌插件行并进社区插件列表，是那个列表的第一行**（2026-10-09 用户截图「符合插件安装后的 UI 风格」）。openai-oauth 此前是 `.entity-list` **外面**一条自带下缘刻蚀线的裸行，另配一套 `.builtin-strip / .builtin-name / .builtin-state`（自己的 padding、字号、字色）：没有卡片边框与圆角，扫一眼就是「另一个东西」。现在它就是一条普通的 `.entity-row.entity-row--single`，名称 / 来源 chip / 状态文字全部复用社区插件那一套。
  - **语义差别只体现在右侧动作**：没有安装 / 更新 / 卸载，保留启停开关，并提供 ChatGPT 登录、取消登录、刷新模型列表与退出登录。账号操作复用工作台的 Rust 命令与脱敏状态；启停意图与账号登录分开，停用插件不退出账号。「不进社区中央库」由「没有那些按钮」表达，不需要另立一块视觉。来源 chip 用 `Cpu` 图标 + 「内嵌」二字，与 npm / GitHub 同一枚 `.origin-chip`。
  - **那套 `.builtin-*` 样式必须删干净**：元素并进来之后它们就成了没有作用方的声明——留着会让下一个人以为内嵌行另有样式。
  - **空态判据要一起改**（`currentKernelListEmpty`）：只看社区插件的话，列表里明明列着内嵌那一行、下面却写着「当前内核尚未接入任何插件」。`is-empty` 与 `el-empty` 必须共用这一份判据，否则两处会分叉。
  - 判据在 `ui/test/designAlignment.test.js`「内置插件行是『当前内核』列表的第一行」：按**顺序**判（列表开标签 → 内置行 → 社区插件 v-for），只看「内置行存在」的话把它搬回列表外照样会绿；反向验做过（搬回列表外 → 转红）。
  - **内置插件不止一个了：那几行是 `v-for`，文案与能力都来自后端**（2026-10-12 加 Ponytail 时改的）。状态从单对象变成列表（`builtinStore.list`），`builtin_status` 一次返回全部；面板 `v-for="builtin in builtinList"`，显示名读 `builtin.displayName`、ⓘ 读 `builtin.note`、要不要画账户卡读 `builtin.hasAccountCard`——**三者全部由后端 `BuiltinSpec` 下发**（`src-tauri/src/plugins/builtin/builtins.rs`）。前端不留一个字面量：那份 `BUILTIN_NAME` 常量删掉前已经被抄在四处，而它 2026-10-09 刚改过第二次名字，抄一份就等于下次改名必漏一处，而漏掉的那处不报任何错。
    - **「按 id 取」不许写成「取第一项」**：概览页与套餐窗口的 Codex 授权入口只看 `openaiBuiltin`（`builtin.js` 的 computed），拿第一项的实现会在某个内置插件排到前面时静默返回别人的状态——症状是入口跟着一个不相干的开关走，界面上看不出异常。判据在 `ui/test/builtinPlugins.test.js`（它把 ponytail 摆在 openai 前面）。
    - loading key 按插件分（`builtinToggle:<id>`）：共用一个 key 时拨一个会让另一个的开关也转起来。
    - 判据：`designAlignment.test.js` 那条（钉 `v-for` 而不是单行 + `v-if`）+ `builtinPlugins.test.js` 4 条。三条变异逐条反向验过（退回取第一项 / loading key 不带 id / 面板退回读 `builtinStore.view`）。
  - **内置插件的来源 chip 是放大一档的紫色，此后每个内置插件都用这一枚**（2026-10-13 用户要求「icon 文字放大，调整为紫色显示，后续 dsh-xlink 内置插件都要用这个颜色标记」）。`.origin-chip-builtin` 叠在 `.origin-chip` 之上：13px（基线 12px）+ 图标显式 15px + `--builtin` 文字 / `--builtin-fill` 底 / 同色描边；npm 与 GitHub 那两枚继续走中性灰——形状一样、颜色是区分「内嵌 / 社区」的那一维。
    - **挂在那枚 chip 上，而不是挂在某个插件 id 上**：chip 在 `v-for="builtin in builtinList"` 里，后端登记表加一项就自动带上紫色；改成一个按 id 判断的分支时，下一个内置插件会静默退回中性灰，而界面上看不出任何异常。
    - **图标字号必须显式写死**（`.origin-chip-builtin .origin-chip-icon`）：EP 的 `.el-icon` 是 `font-size: inherit`，只抬 chip 字号等于图标等比跟着长，那不算「图标被放大」。15 vs 13 是刻意的，与侧栏 17 vs 15 同一口径。
    - **色值走 `--builtin` / `--builtin-fill` 且两套主题各一份**：浅色 `#7c3aed`（白卡 5.70:1）、暗色 `#a78bfa`（`#142239` 上 5.86:1），13px / 600 粗体按正文 4.5:1 算。只在一套里定义的话，另一套下 `var(--builtin)` 整条失效、退回中性灰且不报错。
    - 判据在 `ui/test/designAlignment.test.js`「内置插件的来源 chip 放大一档走紫色」：修饰类必须挂在 builtin 行里且**全模板只出现一次**（多处说明挂错到 npm / GitHub 上了）、颜色读 token、字号与图标字号的关系、两个 token 两套主题都定义。五条变异逐条反向验过；其中「文字色写成裸 `#7c3aed`」第一次没被抓住——裸正则读规则原文时 `/color:/` 命中了同一条规则里的 `border-color:`，改成走 `effectiveDeclaration` 之后才转红。
- **业务能力不因改版增减**：页签分组（工作台 / 资源 / 系统）、侧栏收起，都是把原有信息重新摆位，不是新功能。设计稿里没画的东西不要自己加；设计稿画了而我们判定不画的（顶部工作条、内核版本页两列栅格、侧栏 13px 字号、概览卡头实例 caption），要在下面单独登记成一条有意偏差并写清理由。
- **侧栏底部那一行是工具区，收起开关也在里面**（2026-10-07 用户要求，**覆盖设计稿**）：稿子的 `.sidebar-footer` 只有「标签 + 版本 + 刷新 + 主题」四件，收起开关画在 `.brand` 右端（`docs/ui/dsh-xlink-ui-redesign-draft.html` 439-468 行）。现在是「标签 + 版本 + 收起 + 刷新 + 主题」五件，收起开关排在刷新之前。取舍是品牌行只留品牌，收起开关和它收起的侧栏右侧那排控件读起来是一件事；代价：224px 里版本号省略得更多，**收起态（64px）必须竖排**（横排要 100px，内容盒只有 48px，否则溢出到主区）。竖排用 `flex-direction: column` 写死，不靠 `flex-wrap`——那会让换不换行取决于实际渲染宽度。`.brand` 的 `position: relative` 随之删除（它只为浮在那儿的开关而存在），品牌行现在只有 logo 与文字。
- **侧栏底部拆成两行：版本号独占一行（2026-10-20 用户报「左下角的版本号长度过长之后会有遮挡」）**。截图里版本号显示成 `0.5.8-r…`——`0.5.8-rc.2（dev）` 的后半段连同 dev 后缀整段被省略号吃掉。
  - 量出来的账：224px 内容盒 220px，一行里「桌 面 端」标签 36px + 三个 26px 方钮 78px + 4 道 7px 缝 28px = 142px，版本号只剩 **78px**；12px 字号下 `0.5.8-rc.2（dev）` 要约 105px，release 构建的 `0.5.8-rc.2` 也要 66px 左右——dev 构建必截、release 只是侥幸。所以这不是「字号 / 省略号调一调」能解决的，而是**那一行的宽度分配从结构上就放不下两件事**。
  - 改法：`.sidebar__footer` 改成 `flex-direction: column`，第一行 `.sidebar__footer-tools`（标签靠左、`.sidebar__footer-actions` 三枚方钮靠右），第二行版本号独占总宽。版本号可用宽度 78px → **220px**（约 31 个 ASCII 字符），dev 后缀不必再藏；`text-overflow: ellipsis` 只留作极端长度的兜底。
  - **没选**的方案：① 删掉「桌面端」标签给版本号腾 36px——这样只够当前这一版，`0.5.10-beta.12（dev）` 一类立刻再截，而且「这行数字是谁的版本」就只剩猜测；② 缩字号到 10px——同样治标不治本，还破坏底部工具区 12px 与全仓排版档位的一致性；③ 让版本号在行内换行——版本串按连字符 / 圈括号断成两截，比省略号更难读，且 footer 高度随内容变化；④ 只在放不下时才叠成两行（`flex-wrap`）——换不换行取决于实际渲染宽度，同一个版本号在不同 DPI / 字号下会是两种布局，本仓已两次拒绝这种依赖运行期宽度的排法。
  - 收起态（64px）不变：标签与版本号都隐藏，三枚方钮竖排在 `.sidebar__footer-actions` 上（`flex-direction: column`），横排要 92px 而内容盒只有 48px。
  - 判据在 `ui/test/designAlignment.test.js`「侧栏底部拆成两行」：模板侧用 `@vue/compiler-sfc` 的 AST 钉「版本号是 `.sidebar__footer` 的直接子级且排在控件行之后」、「控件行只有标签与控件组」，CSS 侧钉 footer 的 `flex-direction: column` 与工具行的 `space-between`。只钉 CSS 是假绿——把版本号塞回控件行时 flex 方向一个字都没变。改完用真实 `SideBar.vue` 在 224px 下截图核过展开 / 收起 / 浅色 / 长版本四种状态。
- **版本号前的 tag 图标走 Lucide `Tag`（2026-10-20 用户要求「版本号前面加个 tag 的 icon」）**：13px、装饰性（`aria-hidden`，语义由版本字符串本身承载），文字包进 `.version-badge__text`——图标 `flex: 0 0 auto` 不参与压缩，极端长度时被省略号收掉的是文字而不是把图标挤走。
  - **没选**复用插件版本 chip 那枚 mask SVG 图标（`.meta-version::before`）：它是填充剪影、和 chip 一起调过透明度，搬到裸文字前要么再抄一份 mask url（同一语义两处实现），要么把 chip 的规则拆成共享类——而本仓「新补的图标走 `@lucide/vue`」已经定过（EP 的 `PriceTag` 缩到 13px 糊成一颗黑点，2026-10-08 概览版本号那处踩过，注释留在 `OverviewPanel.vue`）。别名 `Tag as TagIcon` 是硬规则：面板里同时有 `el-tag` 组件与一排 `*.tag` 类名。
  - 判据在 `designAlignment.test.js`「侧栏版本号前的 tag 图标」：AST 钉图标必须是 `.version-badge` 的第一个子节点、带 `aria-hidden` 与 `:size="13"`、文字在独立元素里；CSS 侧钉 flex 行、图标不收缩与文字的省略号三件套。去掉图标 / 把 `display: flex` 改成 `block` 两条变异各反向验过转红。
- **侧栏底部再改一次排布：控件行贴左、「桌面端」标签下移与版本号同行（2026-10-21 用户截图要求）**。原文：「把『桌面端』这个文字移动到下方，和版本号放到一行。『主题、更新、收起侧栏』这些操作按钮左对齐贴到左边」。
  - 改法：`.sidebar__footer` 仍是 column，但两个子行换了职责——第一行是 `.sidebar__footer-actions`（三枚方钮，`justify-content: flex-start` 贴左缘），第二行是新增的 `.sidebar__footer-meta`（「桌面端」标签 + `.version-badge`）。原先的 `.sidebar__footer-tools` 容器删除：标签搬走后它只剩一组控件，成了单子节点容器（`.sidebar__footer-label` 在 `SideBar.vue` 的 scoped 块里另有一条规则，样式没丢）。
  - **版本号仍不与控件同行**——2026-10-20 那个「dev 后缀被截成 `0.5.8-r…`」的老问题没有回来：第二行可用宽 220px 减去标签 36px 仍有 ≈166px；实测 dev 构建的 `0.5.8-rc.2（dev）` 的 `clientWidth` = `scrollWidth` = 91（未截断）。
  - 收起态（64px）不变：三枚方钮竖排（`flex-direction: column`，横排要 92px 而内容盒只有 48px）；第二行**整行**隐藏（`.sidebar__footer-meta { display: none }`）——原先只隐版本号、标签靠 `.nav-item__label` 的通用规则隐，现在两件在同一个行容器里，隐整行才不留下一行空高（footer 的 `gap: 7px` 仍会算它）。
  - 判据在 `ui/test/designAlignment.test.js`「侧栏底部两行」：AST 钉 footer 的两个直接子级是「控件组 → 标签+版本号行」、控件组里不许混进标签；CSS 侧钉控件组 `justify-content: flex-start` 与收起态第二行 `display: none`。**四条变异逐条反向验过**（控件组改回 `flex-end` / 标签塞回控件行 / 第二行容器删掉 / 收起态不再隐藏）都转红，还原后回到基线绿。第二行容器的 `display` 走**规则文本**而不是 `effectiveDeclaration`：收起态那条祖先条件规则特异度更高，会把展开态的值盖成 `none`（该文件对祖先条件规则的既有处理，见下面「工具分不清的三类选择器」）。
  - 真机核对：真构建产物（`ui/dist`）＋ Tauri 桩在浏览器里跑，量到三枚方钮左缘与 footer 内容左缘同为 14px（贴左）、标签与版本号 `top` 同为 690（同行）、版本串未截断、footer 无横向溢出。
- **宽表格要给数字列显式列宽（`table-layout: fixed` + `nowrap`）**，并按内容量分别给值。宽版下自动布局会把表头竖排成「文件 / 数」、把 `896.7 KiB` 拆成两行；改成固定布局后列宽不再随内容生长，于是**宽度变成承重的**：给窄了不会换行，而是文字直接溢出表格右边界（迁移向导 2026-10-07 就把「大小」和「文件数」并进同一条 52px 规则，`896.7 KiB` 跑到了边框外）。**路径类长文本列不要 `nowrap`**，让它们折行——完整路径要看得见，截断比换行更容易读错。钉在 `ui/test/migrationPanel.test.js`。
  - **断言要取声明值，不要比原始子串**。同一天第一版断言比的是两段起点不同的 CSS 子串，结果把两列并进同一条规则它照样绿——**反向验时才发现它抓不到任何回归**。写成「抽 `nth-child(n)` 那条规则里的 `width` 再比值」之后，同一处改坏立刻转红。
- **设计稿对齐的判据走层叠后的生效值，规则表要带 `.vue` 的 scoped 块**（`allRulesIncludingVue()`，`ui/test/lib/css-cascade.mjs`）。面板级样式大半写在组件里，只扫 `.css` 时 `.kernel-summary` / `.plan-grid` / `.sidebar__theme-btn` 一条都查不到，「查层叠」会退化成「查全局基线」。这条工具本身也是修过的：`ruleBlocks` 原来不剥注释，而本仓大量规则前带一段长注释，注释里只要有 `{` 或换行，prelude 就从注释开头一路吃到花括号——**表现是「明明写了却查不到」，而且不报错**。
  - **工具分不清的三类选择器，判据改看 scoped 块文本**（`ui/test/designAlignment.test.js` 里都是这么写的）：`:deep(...)`、`:has(...)`、状态属性 `[aria-pressed='true']` 与后代条件 `.sidebar.is-collapsed .x`。它们的共同点是「能不能命中」取决于 DOM 而不是声明归属。想把工具改严格（区分伪元素 / 状态 / 祖先条件）会连带改掉 4 条既有测试的语义——那是另一件事，别在写判据时顺手做。
    - **「祖先条件」这一类还会主动把不相干的规则算进来，而不只是「查不到」。** 2026-10-07 写侧栏图标那条判据时用了 `effectiveDeclaration(['nav-item','el-icon'], …, 'font-size') === '17px'`——工具只认主语 `.el-icon`、忽略祖先，于是更高特异度的 `.btn-row .el-button .el-icon { font-size: 17px }`（300 > 200）被当成答案，**判据因为错误的原因而绿**：图标那条规则改成什么它都答 17px。反向验才把它逼出来。凡是主语类在全仓被多条后代规则复用（`el-icon` / `dot` / `title` / `chip` 这类通用尾巴），一律读剥注释后的规则文本，别信 `effectiveDeclaration`。同一批里 `.nav-item__badge` 也栽在同一个坑上：按主语查拿到的是收起态那个 8px / `font-size: 0` 的小红点。
  - **`effectiveDeclaration` 带伪类维度（`opts.pseudo`），且不给它时会主动排除带伪类的规则**。`.a:hover` 的主语 token 与基线 `.a` 完全相同，混进基线查询就等于把「悬停时的值」当成「静止时的值」回答出去——问 `.head-tip-icon--warning` 的基线色时返回了 hover 的加深色。只认主语**尾部**的伪类：`.a:hover .b` 不算，那是祖先的状态。
- **判「每一枚都在」要数次数，不是逐个 `assert.match`**（2026-10-08 反向验又逼出一条）。写「三枚次级按钮都不能少」时用了 `for (const label of [...]) assert.match(块, new RegExp(label))`，删掉一枚照样绿——**`官网网页版窗口` 里含着 `工作台窗口` 这个子串**，三枚里少掉两枚，剩下的仍能让三条断言全部命中。改成数**整枚按钮**（`<el-button …>文案</el-button>`）的出现次数并断言 `=== 1`，删一枚就少一次。这与「icon 计数」那条是同一个坑的两个变体：**`match` 判的是「有一个」，需求常常是「每一个」**。
- **反向验的变异必须真的命中目标**——这是同一件事的另一头。有一条变异写成「在 metrics 开标签后再复制一份 metrics 开标签」，看着像「把按钮搬下去」，实际按钮本体根本没动，判据当然绿。**绿在没生效的变异上，说明不了任何事**。剪贴搬移要用正则把整块剪出来再贴到新位置，并在事后断言「新串读得到 / 旧串已消失」，否则这一条验的就是空气。
- **这轮钉住的几个具体决定**（`ui/test/designAlignment.test.js`）：概览主栅格 `1.25fr / 0.75fr`；**末行是「最近操作（左） | 需要关注（右）」左右并排**（2026-10-07 先把异常清单从首行整宽挪到末行，2026-10-08 再把左右对调——对调的理由是「没有待处理项」这个最常见状态下「需要关注」整张卡不渲染，它原本所在的左半栏就整块空着）；版本号 20px，且与状态行**竖向堆叠**（横排时状态胶囊会随内核状态换行；2026-10-08 次级入口接进这个块后仍是竖向块内部，只是从 flex 改成了两行网格，见「概览页」一节）；套餐用量**固定三列**（1040 宽下 `auto-fill minmax(170px)` 会排成四列）；额度行是「名称 22px | 条 1fr | 百分比 auto」一行栅格、条高 6px、百分比在条外；余额总额 14px 粗体、赠金充值降到第二行 10px；侧栏主题入口是三态循环按钮（太阳 / 月亮 / 显示器对应浅色 / 深色 / 跟随系统）；内核版本页卡片标题叫「官方版本」而不是「npm 发布」（npm 只是取源渠道，写进标题会在换回 GitHub Releases 后立刻变成错的）。
- **末行两卡吃满剩余纵向高度，块内文字换行**（2026-10-08 用户要求「两个功能块都增加高度，用满纵向高度」「内部内容注意换行显示信息」）。
  - 高度：`overview-grid` 写 `grid-template-rows: auto auto minmax(min-content, 1fr)` + `flex: 1`，祖先 `.panel` 给 `min-height: 100%`（百分比高度要落在确定高度的祖先上，否则整条规则静默失效）。用 `min-content` 起底而不是 `0`：后者会在空间不够时把卡压扁、内容溢出自己的格子，看着像布局坏了。
  - 换行：`.diag-row__label` 从 `nowrap` + `text-overflow: ellipsis` 改成 `overflow-wrap: anywhere`。**这一条推翻了 2026-10-07 的相反决定**，原理由是「诊断正文可能很长，换行会让卡片高度由日志长度决定」——那个前提这次不成立了（末行吃满剩余高度后，卡片再长也不把内容推出首屏），代价正好被高度改动抵消。`anywhere` 不能省，否则一整段没有空格的错误文本会把整列撑出去，换行变成横向溢出。
- **行尾动作是带边框、带 icon 的徽章**（`.diag-row__cta`），不是纯文字箭头（2026-10-08 用户要求「操作按钮高亮、加 icon、badge 边框」）。此前那格是一段 `›` 或「查看」，在一列不能点的读数里完全看不出能不能点。语气跟着同行读数的严重程度走（`--danger-*` / `--warning-*`，为此在两套主题各加了 4 个 token），悬停加深自身色而不是换成强调蓝——accent 蓝在这套里表示「主操作」，一换就等于把每条读数都抬成主操作。
- **展开箭头与动作按钮不许共用类名**。「还有 N 项 / 收起」那行的 `⌃`/`⌄` 用独立的 `.diag-row__caret`：它标的是「这一段还有没有」，与动作按钮语气相反；一个类名表示两件事时，改其中一种会顺手改到另一种。判据里显式钉住「`.diag-row__arrow` 不许再出现」。
- **补充 / 警告类提示一律收成标题旁的图标 tooltip，不许再做成常驻整宽条**（2026-10-07 用户要求，插件页的第三方来源免责提示是第一个）。判据是这条提示**不含任何随状态变化的信息**——常驻一整行只为放一句不会变的文案，代价是把页头下面的内容整体下推。语境在哪就把图标挂哪：免责提示的语境是「插件」这个标题，就挂标题旁，不要挂到侧栏或别的页面去。三条纪律：
  - **触发元素必须可聚焦**（`tabindex="0"` + `aria-label`）。hover-only 的信息对键盘用户等于不存在，纯展示的 `div role="note"` 过去能过是因为它本来就一直显示。
  - **tooltip 的 `content` 与图标的可访问名取同一个常量**。两处各写一遍字符串，改了一处就会出现「图标说有提示、读屏说没提示」。
  - **严重程度要分色**：ⓘ 是「补充一句说明」，⚠ 是「有件事你该知道」，共用尺寸但不给同一个颜色；⚠ 悬停时也**不要**改成强调蓝，那会被读成另一个可点的东西，改成加深自身色。
- **页头的标题与说明必须同属一个子元素**。`.page-head` 是 `space-between` 的 flex 行（左标题右动作），而六个页面的模板结构是「`<div>` 包住 `.page-title` + `.page-desc`」。2026-10-07 给插件页加 `.page-title-row` 时把 `.page-desc` 提到了 `.page-head` 的直接子级，说明立刻被当成右侧动作飘到页面右缘。**判据比的是嵌套深度而不是 `</div>` 个数**——两种错结构的 `</div>` 计数一样，数个数那一版在真实回归面前是绿的。判据里两个坑：按标签配对切块（切到文件尾会把后面几十个兄弟算进栈里），以及**先剥 HTML 注释**（模板注释里会写出字面量 `<div>`，剥晚了配对就平不了）。
- **页头主副标题同行，排布由 `.page-head__text` 承载**（2026-10-10 用户对六个页面统一提的要求：「主标题的文字、大小不动，副标题缩小字号并且和主标题同一行显示」）。那一层包裹 div **不能删**——上面那条判据量的就是它的嵌套深度；现在它同时是主副标题同行的 flex 行。
  - **`align-items: baseline` 而不是 `center`**：`center` 让 12px 落在 32px 行盒的正中，与 24px 标题的光学中心勉强重合但基线不重合，行末句号会浮到标题基线之上，读起来像两行字碰巧挨着。
  - **`.page-title` 必须钉 `flex: none`**：不钉的话 flex 两侧都可压缩，副标题一长被压扁的是**标题**——标题断行比说明换行更糟（数据迁移页那句最长，约 40 字）。
  - **`.page-desc` 的下外边距清零**：块级排布下那 5px 是「标题与说明之间」的缝，同行之后由 `gap: 12px` 承担，留着会变成行盒之外的一条额外空白、把副标题往右推。
  - 判据在 `ui/test/designAlignment.test.js`「页头主副标题同行」那条：**样式与模板一起钉**。只钉 `display: flex` 是假绿——六个模板忘了挂 `page-head__text` 这个类时 theme.css 的规则照样在、样式判据照样绿，而实机上副标题退回块级另起一行。同条还钉主标题的 24px / 700 / 32px（「主标题不动」与设计 token 规范 §2 的交汇点）与副标题读的确实是 `--fs-page-desc` 那一档。
- **tooltip 浮层一律限宽并换行，选择器钉在 `[role='tooltip']`**（2026-10-10 用户报「注意宽度控制，做好换行显示，不要出现超长 tooltip」）。根因是 EP 的 `.el-popper` **只有 `min-width: 10px`、没有任何 max-width**，而浮层是 `position: absolute` 的自适应宽盒子：里面的纯文本有多长，浮层就有多宽。本仓最长的 `SubscriptionWindow` 的 `CAPABILITY_TIP` 约 140 字 ≈ 1680px，窗口只有 1040px，它会拉出一条横穿屏幕、还被窗口裁掉两头的黑条。
  - **`.el-popper` 是所有浮层的公共类，不许直接给它加宽度上限**：本仓还有 2 个 `el-select` 与 3 个 `el-dropdown` 走同一个类，一刀切下去连下拉菜单一起改掉，而菜单本来就该按内容取宽。`[role='tooltip']` 是唯一的区分点：EP 给浮层的 `role` prop **默认值就是 `"tooltip"`**，而 `el-select` 显式传 `role: "listbox"`、`el-dropdown` 传 `role: "menu"`，两者都被排除。`el-popconfirm` 内部就是 `ElTooltip` 且不覆盖 role，于是同样命中——它那几句确认说明同样会溢出，一并治掉是对的。
  - **一条全局规则胜过给 36 个 `<el-tooltip>` 逐个挂 `popper-class`**：模板里漏挂一个，那一处就漏修一个，而漏掉的那个恰好是最长的那一个（新写的 tooltip 谁记得挂）。
    - **2026-10-11 兑现了这一条**：插件中心那个预检说明气泡曾单独开 `ui/src/plugins/plugins.css` 收一条 `.precheck-tip-popper`，四条声明与全局规则**逐条同值**（还少一条 `overflow-wrap`），特异度也更低（单类 0,1,0 vs 类+属性 0,2,0）——**从加进去那天起就没赢过任何一次层叠**，是个看着在管事、实际什么也没做的规则。文件、模板上的 `popper-class`、以及它自己那条更弱的判据一并删掉。教训不止于这一次删改：**「写了一条特异度更低的规则覆盖全局基线」不会有任何报错**，它只是安静地什么都不做；判据要么断言**层叠后的生效值**（`ui/test/lib/css-cascade.mjs`），要么就别写那条规则。
  - **`overflow-wrap: anywhere` 而不是 `break-word`**：前者参与 min-content 计算，在自适应宽盒子里才真能把长路径压回上限内。本仓 `installTip` 里的路径（`~/.dsh-xlink/plugins/dsh/`）与 `quarantineNote(row)` 里的第三方隔离原因**可能全是 ASCII**——中文本来就能自然断行，而它们不能（EP 的 `break-word` 只在「这个词自己都放不下」时才断）。
  - 判据三条：`.el-popper[role=tooltip]` 规则存在且四条声明齐全（`min-width: 0` / `max-width: min(420px, calc(100vw - 32px))` / `white-space: normal` / `overflow-wrap: anywhere`）、**裸 `.el-popper` 不许带 max-width**（带了就等于把 select / dropdown 一起限宽，而那是这条选择器存在的全部理由）、以及**EP 那三个 role 默认值**（读 `node_modules/element-plus/es/...`）。第三条是前提判据：升级 EP 时它先红，那正是要看的——选择器的正确性建立在这个默认值上，前提变了就得重新确认。产物布局若变了（只有 `lib/` 没有 `es/`）就 `t.skip`，**不假装绿**：此时是「验不了」而不是「验过了」。
- **叠色一律走 token，不许写死 `rgba(255,255,255,…)`**（`--overlay-faint` / `--overlay-soft` / `--overlay-strong`，另有 `--surface-sunken` 给代码块 / 日志内容那类「凹下去」的容器）。同一个视觉意图——把当前底色往目标方向推一点——在暗色下是叠白、浅色下是叠黑，两套主题各给一份值。写死的那 45 处（2026-10-07 清完）在浅色下全部失真：白底上的白 hover 看不见、白底上的白进度条轨道消失。
  - **批量化替换叠色时会连 token 的定义行一起替换掉**，结果是 `--overlay-strong: var(--overlay-strong)` 这种自引用——「有定义」照样通过，变量却彻底失效。所以除了「有没有定义」，还要钉「值必须是字面量」。
  - 判据见 `ui/test/designAlignment.test.js` 的「叠色 token」一组；它比的是剥掉注释后的**声明**，注释里允许出现字面量（那里正是在解释当年为什么写死）。
- **accent 的半透明档也是 token（`--accent-fill` / `--accent-line`），同样不许写死 `rgba(79,140,255,…)`**。这是上面那条的同类问题、换个主语：那组值是**深色主题**的 accent（`#5b9cf8`）调出来的，浅色主题的 accent 是深蓝 `#2766d9`，同一个 alpha 叠上去是一层洗不掉的蓝雾。**两套主题必须各给一份 alpha，不能只换纯色不换 alpha**。
  - 日志阅读器（`.log*` / `.logwin*` / `.rail-toggle` / `.pane-splitter`）原来散着 7 处写死的颜色，其中 `.log-tab` 未激活态写的是 `rgba(232,236,247,.72)`——**近白**。浅色主题下它就是浅底上的浅字，整列文件名几乎看不见，而这不会让任何 build 或单测变红。
  - 判据见 `ui/test/logWindowTheme.test.js`：三条一起钉——不许写死、**引用的 token 在 `:root` 里定义过**、分组标题与文件签保持一档差。第二条的判据 2026-10-08 改过一次：原先写的是「两套主题都定义过」，那把**本来就不随主题变**的排版档位（`--fs-*` / `--action-h`，只在 `:root` 定义一次）也判成违规。真正的失效条件是「`:root` 里没有」——只在 `html.dark` 里定义的话，浅色下那条声明被浏览器丢弃、元素退回默认色，**不报错也不变红**。反向验：把 `--text-secondary` 从 `:root` 挪进 `html.dark` 仍转红。
- **浮层的底色与遮罩各只有一个来源，不许写死**（2026-10-08 用户拿浅色主题下的进度浮层截图报「这类弹窗还是没有适配主题，注意统一调整」）。根因有两层，缺一层都看不见：
  - **四处浮层底色写死了暗色主题的值**。`.progress-body` / `.el-dialog` 都是 `#0d1428`（那是暗色主题的 `--surface`），`.debug-panel` 同色，`.render-error-fallback` 写 `var(--el-bg-color, #fff)`。浅色用户看到的是一块黑板子配深灰字，而**它不会让任何 build 或单测变红**——`#0d1428` 是合法 CSS 值。四处现在统一读 `--surface-raised`（浅色 `#fff` / 暗色 `#35343a`，与 Element Plus 自己的 `--el-bg-color-overlay` 同值）。
  - **同一个遮罩在两套主题里明暗相反**。Element Plus 在**浅色**下的默认遮罩是 `rgba(255,255,255,.9)`——一层九成白，弹窗一开背后的页面基本看不见；暗色下却是 `#000c`。壳自己的 `.progress-overlay` 又单独写死 `rgba(4,8,20,.55)`，于是「进度浮层的遮罩」与「`el-dialog` 的遮罩」是两个手调的值。`:root` 现在把 `--el-mask-color` 拉成深色半透明，`.progress-overlay` 直接读它，`.el-overlay` 读的一直是它。
  - **`--el-dialog-bg-color` 必须声明在 `.el-dialog` 自身，不能放 `:root`**：库在 `.el-dialog` 规则里就写了同名自定义属性，写在元素身上的同名声明**赢过从 `html` 继承下来的值**，放到 `:root` 等于没写——症状是「规则明明在，弹窗底色没变」。这也是那条规则写成 `.el-overlay-dialog .el-dialog` 而不是 `.el-dialog` 的原因（顺带与既有的圆角 / 描边并成一条）。
  - **`var(--已改名的旧 token, 兜底值)` 是这一类里最阴的形态**：token 改名后兜底值是近白，浅色主题下整条声明**看着还在**（CSS 不报错），元素只是退回那个近白——`--text-muted` 那种次要文字在浅底上直接消失。2026-10-08 清掉的 9 处：`theme.css` 三处（`.tab-label-sub` / `.instance-state-empty` / 一条早已被删的规则）、`MigrationPrompt.vue` 五处、另有 `--surface-soft` 与 `--warn` 各一处。
  - 判据见 `ui/test/designAlignment.test.js` 的「弹窗与浮层的主题适配」一组（3 条），已做过反向验：写回 `#0d1428`、删掉 `:root` 的 `--el-mask-color`、塞回一处 `var(--muted, …)`，三条各自转红。其中传类集合给 `effectiveDeclaration` 时**要去掉选择器里的点**（`subjectTokens('.progress-body')` 而不是 `['.progress-body']`）——后者让工具一条规则都匹配不上、安静地答 `null`，判据于是因为错误的原因而绿。

- **彩色按钮 / 徽章的文字色必须走语义 token，不许写死「为深色底调的淡彩」**（2026-10-08 用户拿浅色主题下的概览页截图报「亮色模式下，这个『官网网页版』，文字颜色不清晰」）。这是上一条的同类缺陷低一层：那条是**底色**，这条是**文字色**。
  - 那一族值来自 Tailwind 的 `emerald-300` / `red-400` / `emerald-400`（`#6ee7b7` / `#f87171` / `#34d399`）。它们在**深色**底上舒服，在**浅色**底上等于没有颜色：白卡上 `#6ee7b7` 只有 **1.52:1**、hover 的 `#a7f3d0` 1.28:1、`#f87171` 2.77:1、`#34d399` 1.70:1，而正文门槛是 4.5:1。
  - **这类值最难被现有门禁抓到，因为它们在暗色下看着是对的**——任何只跑暗色截图的检查都会放过它。判据因此钉的是「读 token」而不是「等于某个色值」：语义色天然两套主题都对，而淡彩不会。
  - 现在：`.btn-chat` → `--success` / hover `--success-strong`，`.btn-danger` → `--danger` / hover `--danger-strong`，插件页「链接」模式徽章 → `--success` + `--success-fill`。`--success-strong` / `--danger-strong` 的口径与既有的 `--accent-strong` 一致（浅色下更深、暗色下更亮）。
  - **`--success-fill` 是补齐而不是新增词汇**：`--accent-fill` / `--warning-fill` / `--danger-fill` 早就有三个，success 缺席才导致那枚徽章只能写死 `rgba(52,211,153,.12)`。**刻意不补 `--success-line`**——没有第二处用得上它，登记一个没有作用方的 token 与本仓惯例相反。
  - **`.el-icon` 不再单列选择器**：Element Plus 的 `.el-icon` 是 `--color: inherit` + `color: var(--color)`，图标本来就跟着按钮走。原先每条按钮规则都把 `.el-icon` 重抄一遍（`.btn-chat` / `.btn-danger` 各四处），纯冗余，而且**抄漏一处就只染到一半**——图标与文字不同色时看起来像渲染错位。判据显式禁掉这个形状。
  - 判据见 `ui/test/designAlignment.test.js` 的「彩色按钮」一组（3 条），已做过反向验：把 `.btn-chat` 换回 `#6ee7b7`、删掉暗色主题的 `--success-strong`、把 hover 换回 `#fca5a5` 并加回 `.el-icon` 选择器，三条各自转红。
- **药丸页签的内边距两侧都要在，选择器必须带 `.el-tabs__nav` + `:nth-child(n)`**（2026-10-08 用户报「『已安装』按钮文字未居中」）。根因是**特异度**，不是写错值：
  - Element Plus 有两条 **(0,4,0)** 的规则会单侧清零页签内边距：`.el-tabs--top > .el-tabs__header .el-tabs__item:nth-child(2) { padding-left: 0 }` 与 `… :last-child { padding-right: 0 }`。库的前提是「页签之间的缝由前一个的 padding-right 承担」——**这只在页签没有底色时成立**。
  - 本页的页签是药丸（`.is-active` 带 `--accent-soft` 底）。单侧清零在无底色时看不出来，在药丸上就是「字整块贴着一边」。按截图量：药丸 54px = 文字 42px + **单侧** 12px。
  - 本仓那条 `.installed-tabs .el-tabs__item` 只有 **(0,2,0)**，静默地输给库——**不报任何错，build 全绿**。改成 `.installed-tabs .el-tabs__nav .el-tabs__item:nth-child(n)`（**0,4,0**，同特异度时 theme.css 在组件库之后加载所以赢）。`:nth-child(n)` 匹配每一个页签，不是只补第 2 个。
  - 判据钉的是**选择器形状**（那三个片段必须都在）而不是当前几行值——把选择器写回去单测照样全绿。同一条判据还钉住「组件库里那两条规则确实存在」，否则升级 EP 之后它会变成一句空话。
- **分组标题右侧的辅助信息比标题小一档，别让它继承标题的字号**（2026-10-08 用户报「『插件中心』右侧的辅助信息文字大小缩小」）。`「来自 dshfind.com」` 那枚原先只挂 `.muted`，而 `.muted` **只管颜色**，字号直接继承 h3 的 15px——补充与标题一样大，标题就不再是这一行的主体。现走 `.section-divider__note`（12px）。
  - **字号不许加进 `.muted`**：那个类全仓 **23 处在用**，其中不少是跟着自己标题走的 `<span>`，一刀切下去会顺带改掉六个面板。判据同时钉住「`.muted` 不带 font-size」与「模板上这枚确实挂了新类」。
- **排版与控件档位收成 token，六个面板向概览页看齐**（2026-10-08 用户截图框住概览主区说「其他界面的功能块标题文字的大小、副标题、tooltip icon、按钮大小，都参考『概览』界面来做，统一 token 风格」）。改之前同一个角色在七个地方各写一个数：

  | 档位 | token | 改之前的分歧 |
  | --- | --- | --- |
  | 功能块标题 | `--fs-block-title` 17px | `.card-title`（技能页「社区资源」）**全仓没有对应规则**，字号直接继承 body 的 14px |
  | 分区 / 卡内小标题 | `--fs-subtitle` 15px | `.logwin-title` 14px、`.community-title`（技能社区 / 手动安装）**12px**、`.restore-section-title` 13px |
  | 页头副标题 | `--fs-page-desc` 12px | 原先读 `--fs-desc`（13px）。那一档是「**独立成段**的说明文字」，2026-10-10 副标题改成与 24px 主标题同行之后，13px 在 32px 行高里显得过重，且与标题的间距只剩「字高差」那一层，读起来仍像两个块。两者是不同角色，不合并 |
  | 标题旁的 ⓘ | `--fs-tip-icon` 16px | **`.card-info-icon` 16px（5 个面板）vs `.head-tip-icon` 13px（插件 / 技能）** |
  | 动作按钮 | `--action-h` 30px / `--fs-action` 13.5px | **一页之内三种**：概览「工作台」30px、同页「刷新 / 查看详情」24px、技能页「浏览 topic」24px 与「安装」32px |

  - **技能页是第一处漏网的**（用户拿它的截图说「这里没有调整完」）：上一轮只收了 `.card-head`，而「浏览 topic」在 `.community-browse-row`、「安装」在 `.install-row`、「刷新数据」在 `.section-divider`——三个容器都在 `.card-head` 之外，于是它们各留各的尺寸。共用规则因此扩到四个容器：`.card-head` / `.install-row >` / `.community-browse-row >` / `.section-divider`。**`.entity-action` 那种列表行里的圆形图标按钮不在其中**：那是「紧凑动作」，另一个角色。
  - **输入框要跟着一起压**：`.install-row` 上加 `--el-component-size: 30px`——EP 的 `.el-input` 高度就是这个变量，只压按钮的话按钮比输入框矮 2px，一高一矮看着像没对齐。**不需要**去覆写 `.el-input__wrapper` 的内部结构。
  - **卡头标题一律用 `<h2>`，别用 `<span class="card-title">`**：`.card-title` 在本仓**没有对应规则**（设计稿里有，实现里从来没接），写上去就只是继承 body 的 14px——技能页「社区资源」比同一张卡里的「技能社区」还小。改成 `<h2>` 后 `.card h2` 那一份自动生效，与其余面板一致。
  - **这些 token 只在 `:root` 定义一次，不进 `html.dark`**——字号与控件盒高在明暗下必须一样，否则同一个标题切一次主题就跳一档；两套各写一份则日后改一处忘了另一处就会跳。颜色 token 才是「两套各给一份」。这条区别顺带修掉了 `logWindowTheme.test.js` 一个**错判据**（见该测试一节的说明）。
  - 判据在 `ui/test/designAlignment.test.js`（「概览卡头的主操作…」那条钉共用规则 + **四个容器都要在** + 「不许有面板在 scoped 块里另写卡头按钮盒高」；标题那条读 token 并**顺带断言 token 的字面值**——只断言「规则里写了 `var(--fs-block-title)`」的话，把 token 值改成 12px 照样绿）。已用 headless Edge 加载真实构建产物逐面板复测。
- **同一状态的两个写法要挑「条件写进选择器」的那个**（2026-10-08）：`.btn-chat` / `.btn-danger` 原先各有一条 `:active { color: 静止色 }`，唯一作用是凭源码顺序赢过同特异度的 `:hover`。每多一个「按下时打回去」的状态就要再抄一条。改成 `:not(:active):hover` / `:not(:active):focus-visible` 之后一条就够，按下时读到的仍是静止色——行为完全一致，少 6 行声明。判据显式禁掉 `:active` 那两条。

- **官网页签窗钉死深色，且必须连广播一起不订**（2026-10-08 用户报「切主题时官网网页版 Window 的 Tab 与 title 变色没处理好」）。根因是**授权缺失造成的一道单向割裂**：`capabilities/official-chat-strip.json` 只有 `core:default` + `allow-official-chat-tabs`，**没有 `core:window:allow-set-theme`**——`applyTheme()` 里那句 `setWindowTheme()` 在这扇窗上必然被 ACL 拒、`.catch` 吞掉，于是**原生标题栏永远停在 Rust 建窗时钉的深色**，而页面 `html.dark` 却照着 localStorage 走。浅色主题下就是「深色原生标题栏 + 白色页签条 + 下方黑内容」。
  - 这扇窗的内容是 `chat.deepseek.com` 这些**别人的页面**，页面不是我们的、改不了；让它跟浅色主题走只是把割裂换个方向（黑页签栏压浅色内容）。所以官网页签栏继续固定深色，工作台由内核 `ui-theme` 决定并由 `workbench-appearance.js` 镜像到原生标题栏；`main.js` 在 `applyTheme()` 之前 `if (isChatStrip) setThemeValue('dark')`。
  - **只按初值不够**：主壳切主题时那条广播仍会把它拽走，白页签栏照样回来。所以 `followThemeBroadcast()` 也要一起排除——判据两条都钉，且显式禁掉无条件订阅那行。少一条，白切一次主题就复发。
  - **页签栏继续读 Element Plus token，不要写死颜色**：`html.dark` 恒在时 token 恒为暗色，写死后主题广播再也影响不到它，两条路会分叉。判据扫 `OfficialChatTabs.vue` 里的十六进制字面量。
  - 这与「副窗原生 chrome 跟随应用主题」不冲突：那条的前提是**壳自己的页面 + 有 set-theme 授权**，两个条件这扇窗都不满足。
- **「刻蚀线」设计标准（2026-10-08 用户定案，已推广到全部页面）**：块头与内容的分隔线一律是「**2px / 两端收细 / `--divider-strong`（明暗各一档，取值见下）**」的一根实线。**几何与颜色分家**：收细是几何，只能用 `clip-path` 六边形——渐变（含 `mask-image`）改的是透明度，两端是「淡到看不见」而不是「细」，1–3px 尺度下几乎分不出来；颜色是 token。原生 border 全部退场（**显式写 `border-*: 0`**，删掉声明会让「已退场」不可测）——border 没法 clip，直线与收细只能二选一。
  - **本体在 theme.css**：`.card-head` / `.card > h2` / `.card-head-toggle` / `.debug-panel header` 四类块头共用一条分组伪元素，六边形只写这一处。`.diag-card--tower .diag-card__title`（概览控制塔）在 diagnostics.css 自成一块、有一份带自有注释的拷贝；概览三格指标线（上边线 + 竖线）是 `OverviewPanel.vue` 的 scoped。**横线六边形因此写了三份，判据逐字比对三处必须一致**——只改一处就会出现同屏两种收细比例。
  - **适用范围是「有线的块头」**：填色工具条（`.logwin` 头 / `.subwin-head` / `.usagewin-head`）、窗口 chrome（`.titlebar`）、表格行、`.section-divider` 那条虚线是别的角色，不跟这条标准；本来就没有线的标题（5 个独立诊断窗口）也不为此加线。
  - **竖线横过来，六边形跟着转 90°，且收细比横线短**（22% vs 20%）：三格指标那一格只有几十像素高，按横线的比例去收两端就各削掉十几像素，出来是一片**叶子**而不是一道线——这条中途返工过一次。
  - **中途被否掉的两样，别再走回去**：①「一深一浅两条线」的刻蚀槽——白底上「反光」不可能比白更亮，凑不出立体感，两档拉得越开越像两根并排的线；②把它连收细一起退回成 `border-bottom: 2px solid` 的直线——被用户当场否掉（「两边细的效果没有了，必须保留」）。
  - **`--divider-strong` 明暗各一档，两套都在各自选择器里声明**。这条改判过三轮，每轮原因不同：最初明暗同值 `#212121`；2026-10-09 暗色分档——`#212121` 压在 `#2c2b30` 卡面上按文字对比度算只有 ~1.15:1（实机渲染读作一道「凹槽」，恰合「刻蚀」本意，但作为分割线等于没有），暗色改取更亮的 `#31435f`；2026-10-10 浅色也分档——无彩近黑压在白卡上是 16.1:1，比正文（`#142342`，15.6:1）还重，用户报「分割线颜色不协调」，改取主题墨色族同档的 `#51627e`（与 `--text-secondary` 同值，白卡上 6.2:1：比描边重、比正文轻，一条线退回「结构线」该在的层级）。判据（`designAlignment.test.js`「三格指标」一组）算的是**对比度**而不是字面值：两套都声明、各自压卡面 ≥1.5:1、都比描边重；浅色还多钉一头「不比正文重」——把近黑换回去会转红。
  - **判据钉高度 / 颜色 / 六边形三者**（只钉 clip-path 的话把 `height` 改 0、线就没了，判据照样绿），查规则用 `[^}]*` 切片不跨规则边界；「最近操作」空态用 `:not(--bare)` 正着写，折叠卡收起态清 `::before` 的 `content`（清 border 管不了伪元素）。
- **系统健康 / 最近操作 / 需要关注三张卡的卡头跟着概览走**（2026-10-08 用户：标题大小与概览其他位置统一、箭头指的分割线也换成刻蚀线）。它们在 `diagnostics.css` 的 `.diag-card--tower` 里，不在 `OverviewPanel` 的 scoped 块内 —— **CSS 按组件分家，横线六边形因此共写了三份**（theme.css 全局 / diagnostics.css 控制塔 / OverviewPanel 指标线），判据逐字比对三处。
  - **块标题读 `--fs-block-title`（17px）而不是写死**：此前是 15px，与同屏「当前内核 / 套餐用量」的 17px 差一档。**只改 `--tower`**：`.diag-card__title` 在 5 个独立诊断窗口里也在用，那几页不在用户点名的范围内，基线仍钉 15px。
  - **`padding-bottom` 保持 9px 不动**：那是「全前端统一」的「标题到线」留白（`ui/AGENTS.md §卡片`，`cardChrome.test.js` 钉着且要求与 `.card-head` 相等）。线从 1px 变 2px 之后总高 11px，比原来多 1px —— **宁可多这 1px，也不为总高对齐去单给控制塔改一个统一值**。
  - **`--bare` 用 `:not()` 正着写**：线画给 `.diag-card__title:not(.diag-card__title--bare)::before`。线从 border 换成伪元素之后，只清 `border-bottom: 0` 那条**照样会画出一条线** —— 而「最近操作」空态那段注释花了好几轮就是为了消掉这份空白。判据显式禁掉不带 `:not` 的那个选择器。
  - **「统一」的口径一个回合里改了两次，最终回到「全前端一个值」**：控制塔先于全局换刻蚀线时，口径一度收成「概览一屏一个值」；刻蚀线定标准并收进 theme.css 后恢复——且统一的不只是色值，还有几何（2px / 同一串六边形，三处逐字比对）。
  - **本文件的两条规则各写成一行**：`codeLineCount` 只剥 `//` 与 `/* */`，**独立的 `}` 按代码行计** —— 常规分行写要多收十几行，第一版就是这么被预算门禁挡下的。
  - **CSS 注释里不许出现字面的块注释记号**（注释不嵌套，第一个收尾符会提前截断注释，余下文本并进下一条规则的选择器、整条被浏览器丢弃）。第一版把「`//` 与块注释」这句写进注释时带了字面记号，**把紧随其后的卡头规则整个弄丢**：产物里有、浏览器里没有，text 级判据全绿，是 headless 实测 computed style（`position` 没生效）才现形的。判据按浏览器语义扫全仓 CSS 注释（`designAlignment.test.js`「CSS 注释里不许出现字面的块注释记号」），变异即红。
- **一条提示和它的动作按钮不许是两个兄弟节点**（2026-10-08 用户要求「按钮和更新信息一行显示」）。概览页的 `el-alert` 与「更新并重启」原先各自占一行，卡片凭空高一块，而两者之间没有任何语义关联——提示说的就是那枚按钮该做的事。现在同属 `.update-banner`（`display: flex` + `align-items: center`）。
  - **`.el-alert` 要 `flex: 1; min-width: 0`**：不写的话提示文字一变长，先被挤下去的是按钮，而按钮才是可点的那个。
  - 判据查的是「**同属一个 `.update-banner`**」而不是「模板里有两个元素」——后者查不出它们是不是同行；并显式禁掉「更新按钮单独占一个 `.btn-row`」这个原形状。
- **弹窗 / 浮层底部的操作排统一靠右**（2026-10-09 用户要求「这类弹窗的操作按钮统一到右侧」）。规则落在**一个共享类** `.dialog-actions`（`theme.css`，`.btn-row` 旁边），不是逐个弹窗写各自的 CSS——原先只有自绘弹窗两处没对齐：进度浮层的「关闭」与事故面板底部的「去处理」。
  - **`el-dialog` 的 `#footer` 本来就右对齐**（Element 的默认），迁移向导 / 预检 / 快照恢复 / 日志弹窗都走它，不必也不该再加。`MigrationPrompt` 自己那条 `.prompt-footer-ask { display: flex; justify-content: flex-end }` 是另一回事：footer 槽位里的按钮被它包在 `div` 里，而块级元素的 `text-align` 管不到内部按钮，得自己 flex——它已经是对的，**不要为了「统一」去动它**，更不要把它换成只管对齐的共享类（那样会丢掉 `display: flex` 与 `gap`）。
  - **不要改 `.btn-row` 本身**：它是全仓共用的容器，概览的欢迎 callout、设置页的通知行里它与旁边的说明文字**同属一段内容**，右对齐会把文字顶走。判据显式钉住 `.btn-row` 没有 `justify-content`。
  - **弹窗内部「按对象分组」的动作排不参与统一**：事故面板每个嫌疑插件块里的「隔离 / 移除」作用于具体那一条，跟着对象走才对。判据点名 `.suspect-actions` 那行仍然不带 `dialog-actions`。
  - 判据在 `ui/test/designAlignment.test.js`「弹窗底部的操作排统一靠右」，按**类**数出现次数（按文案找会踩「关闭」在多处重名的坑），两条变异反向验过：摘掉浮层的类、以及给 `.btn-row` 加右对齐，都会转红。
- **次级按钮那一排要量「余量」，不是量「放不放得下」**（2026-10-08 用户报「按钮都显示后会有点挤」）。三枚 `.btn-sub`（工作台窗口 / 刷新工作台 / 官网网页版窗口）收三处才收得动：内边距 11 → 9、字号 12 → 11.5px、**图标 17 → 13px**（图标那一项单吃每枚 4px，全局 `.btn-row .el-button .el-icon` 那条 17px 是给卡头动作按钮定的，落在 11.5px 的字上就过大）。
  - **图标那条必须比 17px 那条更晚**：两者同为 (0,3,0)，只能靠源码顺序决胜；选择器写成 `.btn-row .btn-sub .el-icon`（而非 `.btn-row .btn-sub`）是为了压过 `.btn-row .el-button` 那条，少一层就等于没写。判据钉住**先后顺序**而不只是值。
  - 实测（headless Edge 加载真实构建产物）：三枚合计 **379px → 347px**，卡内可用宽 482px，版本号「0.4.1-rc.1」约 84px + 间隙 16px → **余量 3.2px → 35.7px**。改之前那 3px 就是用户说的「挤」：版本串再长一点（`0.4.10-beta.2` 一类）就直接压上或折行。
  - **量余量必须量版本号的文字宽度，不能量它那一格**：`.kernel-summary-main` 是 `grid-template-columns: minmax(0,1fr) auto`，`1fr` 那格会把余量吃掉，拿格子宽度算出来的「需要宽」恒等于「可用宽」，余量永远显示 0——第一版就是这么量的，得出的结论是「改前改后都余 0px」。用 `Range` 取文本盒才量得出来。
- **「一行放不下」有两种病，治法相反**（2026-10-11 用户报设置页「Web UI 端口 + 保存配置」折成两行）。**余量为负**是排布本身错了，该换排布；**余量只剩几像素**是碰运气，该指定谁让位。端口行属后者，而它此前的注释把病因写成了前者（「窗口收到最窄约 460px 时按钮才会被挤到下一行」）——那个前提是这张卡占满整行，设置页 2026-10-08 改双栏栅格后就不成立了，照着注释去查窗口宽度会白查一轮。
  - 真实余量（1040 固定窗口）：`1040 − 侧栏 224 − main padding 40 = 776` → 双栏各 `(776 − 12) / 2 = 382` → 减 `.card` padding 32 = 350 → 减 `label-width: 100` = **可用 250**；这一行要 `132 + 8 + ≈109 = 249`。**余量约 1px**，任何亚像素舍入就翻行。
  - **治法是「指定谁让位」，不是继续调宽度**：这一行 `.settings-port-row { flex-wrap: nowrap }`，缺出来的由输入框吸收（`flex: 0 1 132px` + `min-width: 96px`）。按钮是动作、宽度由文案定死，让它变形等于改设计；输入框是容器，缩几像素看不出来，两者可压缩性本就不同。
  - **两半缺一，退法还不同**：只有 `nowrap` → 不折行了，但改成横向溢出，而 `main` 是 `overflow-x: hidden`，按钮右半边被裁掉，比折行更难认；只有可收缩 → 仍然折行，因为 `.btn-row` 默认 `flex-wrap: wrap` 先换行。`min-width` 是地板，flex 子项收缩下限默认 `auto`（= min-content），缺它时收缩压根轮不到输入框。
  - **不许拿全局 `.btn-row` 当杠杆**（同上文「不要改 `.btn-row` 本身」）：同页「任务通知」卡里还有带说明文字的行，那些在窄窗口下**应该**折行。判据显式钉住 `.btn-row` 仍是 `wrap`。
  - 判据在 `ui/test/designAlignment.test.js`「「Web UI 端口 + 保存配置」同行」，钉**机制**（nowrap / `flex` 收缩因子 / `min-width` / 模板类名）而**不钉宽度数字**——后者会随字号、图标、EP 版本一起漂，守它等于守一个必然会作废的巧合。五条变异反向验过（去掉 nowrap / `flex` 改 `0 0` / 删 `min-width` / 模板摘类名 / 全局 `.btn-row` 改 nowrap），逐条转红。

## 目录约定

插件页「已安装」视图：左栏只放本机插件清单，右栏通过「插件中心 / 手动安装」两个页签分别展示获取入口，默认打开插件中心，切换时保留搜索与手动输入。右栏标题和两栏分界使用 2px 刻蚀线，颜色为 `--divider-strong`，两端通过六边形收细。结果与「显示更多」共用 `.catalog-list` 滚动容器，按钮在全部已显示结果之后，等待搜索完成。安装回车仍按预检开关分支执行。

`ui/src/` **按功能分目录**，不要往根目录平铺。2026-09-30 重组过一次：当时 50 个文件（23 个 `.js` + 22 个 `.vue`）全堆在根目录与 `components/` 下，找一个面板得先把整个目录扫一遍。

```
ui/src/
├── main.js / App.vue / store.js / theme.css   # 入口、壳、全局状态，就这四个留根
├── shell/          # 壳自身：导航、概览、设置 + 跨面板共用基础设施
├── kernel/         # 内核版本与实例
├── plugins/        # 插件中心、补丁、安装预检
├── skills/         # 技能
├── diagnostics/    # 安全网 P0/P1/P2：环境回退点与二分定位
├── usage/          # 本地 token 统计
├── subscription/   # 云端套餐用量
├── incidents/      # 工作台事故与任务通知
├── logs/           # 日志查看器
├── migration/      # 数据迁移向导
└── official-chat/  # 官方对话
```

一条功能的状态、动作与面板**放在同一个目录里**。新增功能先问它属于哪一格；确实跨格的共享件放 `shell/`。

搬文件时会跟着失效的东西，**2026-10-01 已经全部改成按 basename 认了**：`scripts/lib/shell-source.mjs`（门禁脚本与 UI 测试共用的一份解析器，读壳侧 Rust / 注入脚本源码的那几条测试都走它）、`check-invariants.mjs` 的 `baseName()` / `inFile()`。`ui/test/*.test.js` 里读**本仓前端**文件的相对路径仍按 `import.meta.url` 解析，本来就不受 Rust 侧目录影响。**新增判据时照这个来**：判据要问「哪个模块」，不是「文件在哪一层」——那一次重组的实测代价是 9 项不变量转红、5 个测试 ENOENT 挂掉、`check-invariants` 自己在启动阶段就崩，红的原因与要检查的东西全都无关。移动文件时同步检查调用方、测试与文档。

## 待移除

功能不是永久的。下面是**维护者已经决定到期就删**的东西；到期时按这份清单执行，不要临场重新判断「删到什么程度算干净」。

### 数据迁移（v0.6.0 之后整体移除）

维护者 2026-10-07 决定。过渡期内它是正式功能：**侧栏「系统」组里紧跟「设置」的常驻菜单**（设计稿 2550 行），设置页入口已按维护者要求移除，迁移从侧栏进入。

要删干净一共四块，**光删前端菜单不算数**——Rust 侧的命令与 capability 还挂着，前端删了只是入口消失、数据与代码都还在：

| 位置 | 内容 | 行数 |
| --- | --- | --- |
| `ui/src/migration/` | `migration.js` / `MigrationPanel.vue` / `MigrationPrompt.vue` | ~1270 |
| `src-tauri/src/migration/` | `wizard.rs` / `home_recovery.rs` / `home_recovery_cmd.rs` / `mod.rs` | ~2845 |
| 入口 | `SideBar.vue` 系统组的菜单项、`SettingsPanel.vue` 那一行、`App.vue` 的面板挂载、`store.js` 里 `installLatestRelease` 之外的迁移调用 | — |
| 副作用 | `capabilities` / `commands.rs` 的迁移命令注册、`check-invariants` 与 `ui/test/migrationPanel.test.js`、`misplacedHome.test.js` 等判据、`AGENTS.md` 与 `src-tauri/AGENTS.md` 里关于它的所有段落 | — |

两处**不要**跟着删：`~/.dsh` 搬迁（`legacy_migration_target`，那是 v0.2.x 平铺目录的搬家，与本功能无关）、`store_relocate`（插件中央库搬迁）。名字里有 migration 但职责不同，凭名字一起删会出事。

`home_recovery`（找回历史会话）严格说依附于本功能，删时一并走；但它读的是别的实例的 `sessions/`，**动手前先确认没有用户依赖它**——那条路径是 2026-09-28 事故（dev 壳提前并走 `~/.dsh`）的回收补救。

## 与 Rust 的边界

- 全局状态在 `store.js`（留根），各功能的状态与动作在**自己那一格**（`plugins/plugins.js`、`skills/skills.js`、`logs/logs.js`…），异步样板（在途去重、静默刷新、更新检查策略）在 `shell/async.js`，组件只读状态、调动作。
- 与 Rust 的通信**只允许**走 `bridge.js` 的 `invoke` / `Channel`。组件里不直接碰 `window.__TAURI__`（注入脚本那一侧除外，且那是 `src-tauri/src/*.js`）。
- 触发 IO 的按钮必须挂 loading（`loading.js` 的 `withLoading(key, …)` + `:loading="isLoading(key)"`）；长任务走 `progress.js` 的 `withProgress`。
- 改完跑 `npm run build:ui`。

## 文案

- **用户可见文案用简体中文。** 面板里每一条面向用户的字符串都是产品的一部分。
- **数据目录不许在前端写死。** 任何展示路径的文案都要读后端返回的真实路径（技能面板读 `SkillStatus.store_root` / `skills_root`，插件面板读 `PluginStatus.store_root`），显示前过 `labels.js` 的 `tildePath` 折叠 home。2026-09-30 两次实测的漂移：先是 P5 把中央库从 `~/.dsh/skills-store/` 搬到 `~/.dsh-xlink/skills/packages/`，技能页的提示气泡还写着旧目录；同一天走查又发现**插件页**写着 `~/.dsh-xlink/dsh-plugins/`，而 `store_relocate` 早已把它整体搬进 `plugins/dsh/`，用户照着找一个不存在的目录——**同一句提示的两页只修了一页**。同一份路径在 `paths.rs` 与 Vue 里各写一遍，前端那份不参与编译也不会报错，只能靠人看出来。现由 `check-invariants` 第 ⑦ 项之六钉住：前端文案里不得出现**路径上下文**中已搬迁的目录名（`dsh-plugins` / `skills-store`），判据要求目录名前带 `~/` 或 `/`，所以迁移向导把 `skills-store` 当逻辑来源 id 用不受影响。
- 告警文案自带可执行的下一步，**模板不要再统一追加「重启应用会自动修复」**：那对「清单缺失」是假的（`reconcile` 不会凭空重建 `store.json`），对「被盖住」更是假的（重启什么都不会改变内核的 rank 次序）——一句对两条都不成立的建议，比没有更糟。

## 间距

- **功能块之间的默认间距是 12px，由容器统一提供，块自己不再带外边距。** 2026-10-06 用户拍板时是 6px；2026-10-07 uiv2 改版按设计稿（`docs/ui/dsh-xlink-ui-redesign-draft.html`，`gap: 12px`）改成 12px。**除非用户明确要求特调，否则所有新 UI 按它写**。值就是 `.panel` 的 `gap: 12px`，栅格容器 `.page-layout` 与列容器 `.page-layout__col` 同样是 12px，三处一致。
  - **缝由容器给，不由块给**：`.panel` 里的块一律 `margin-bottom: 0`，新块也不要自带 `margin-bottom` / `margin-top`。已经这么写了就不要再逐个调块的内边距去凑。
  - **下限 6px，不要更小**：卡片自带底色 + 1px 描边，描边贴着描边会读成一整块分不开的面，而不是两张卡（这条下限原本记在 `theme.css` 的 `.panel` 注释里，2026-10-06 升格为全前端约定，uiv2 改版随默认值一起提到 6px）。
  - **页签行距不许归零**（2026-10-07 用户截图：「页签像是压在内容框上」）。插件页 `.installed-tabs .el-tabs__header` 原本是 `margin-bottom: 0`，浏览器实测那条缝只剩 **4px**。清零清掉的是**行距**，而页签那颗药丸**自带底色**（`.el-tabs__item.is-active { background: var(--accent-soft) }`）——4px 的缝看着不像间距，像「被切了一截」。现在 **10px**：取与 `.card-head { padding-bottom: 9px }` 同一档，而不是 Element Plus 默认的 15px，免得在这一屏里引入第三种节奏。**与上面「归零要显式写 0」不矛盾**：那条讲的是有 `gap` 的容器里块与块之间不许再叠 margin；这条讲的是页签栏与它的内容区之间那段没有 `gap` 兜着的行距。判据在 `ui/test/designAlignment.test.js`。
  - **只有没有 gap 的容器例外**：诊断页那 5 个独立窗口装在 `.diagnosis` 里（`position: fixed` 的另一套容器，不吃 `.panel` 的 gap），它们的间距仍由 `.diag-card` 自己那 10px 提供。「块不带 margin」不是全局规则，是「**有 gap 的容器里，块不带 margin**」。
  - 由此引出一条反模式：**flex 容器的外边距不折叠，会直接叠在 gap 上。** `.panel` 的 `gap` 撞上块自己的 `margin-bottom` 就是两份，同一列里两种缝——实测（dev server + `getBoundingClientRect`）改版前的概览页正是 12/12/12/6，肉眼一眼看出不齐。改动只是把那几条 `margin-bottom` 归零，实测变得处处一致。
  - **还有一条容易漏的**：Vue 的**多根组件**（fragment）会把每个根节点提升成父容器的直接子节点，于是它们**也吃父容器的 `gap`**。`ControlTower` 就是三个根节点，于是三张卡都被算进 `.panel` 的 flex 布局——想「这三张卡不受 gap 影响」是做不到的，只能让它们归零后与 gap 等价。
  - **归零要显式写 `margin-bottom: 0`，不能删掉这条声明。** 作用域选择器只在**它自己声明过的属性上**赢过基线规则；删掉声明等于让 `.diag-card` 基线的 `margin-bottom: 10px` 原样回来。这与「保存按钮漏写 `cursor`」「计数漏写 `width`」是同一个级联陷阱。
- **「看着行距大」先分清是 gap 还是行盒，再动手。** `gap` / `margin` 只管行**与行之间**那一条缝，一组行实际占多高由**每行自己的盒高**决定；而面板里的行几乎总是「裸文本 + 挂了 EP 组件」混排——`el-button`（small 外框 24px）、`el-switch`、`.el-tag` 自带的外框高度都远大于里面的文字（按钮文字只有 12px，旁边的裸文本行是 13px × 1.5 = 19.5px）。行高被盒高最高的那一行撑开，于是缝看着大，**此时继续压 gap 没有任何视觉收益**。2026-10-03 概览「当前内核」卡就是这一种：kv 行里三个 EP small 按钮的 24px 外框把整组行撑开，把行距收小后跨度几乎没动，才回头查出根因在行盒。改法是**收盒高而不是动 gap**（`.kv .el-button { height: 20px }` + `.kv { row-gap: 4px }`，横向 padding 一格没动），五行行距 26/29/29/30 → 24/25/23/24、首行文字顶到末行文字底的跨度 126px → 109px。同一批里的开关行也是照这条处理的（收行盒而不是调 margin）。**新写列表、卡片、设置项时同理**：先把每行的盒高对齐，再谈 gap。
- **别为了「看起来紧一点」去动 EP 组件的横向 padding。** 无底色的 `text` 按钮的 padding 在页面上根本看不见，它只负责可点面积——为了排版动它，视觉零收益、点击区还倒小了。要紧就收 `height`，那是看得见的。
- **验的时候量「首行文字顶 → 末行文字底」的总跨度，不是量 `gap` 的属性值。** gap 调小 4px 而跨度没动，就说明找错了成因，该回头查行盒。同一组行之间还要逐段量间距是否齐——不齐通常是某一行的盒高特殊（多挂了个 EP 组件、多行文字），而不是 gap 写得不一致。
- **量 CSS 之前先确认 dev server 真的在服务改后的文件。** 2026-10-06 踩过：改完文件后旧的 vite 进程仍按缓存发 CSS，`curl localhost:5174/src/diagnostics/diagnostics.css` 拿到的还是改动前那条规则，页面 reload 也一样——截图里「好像有线」，那其实是 body 的背景网格，差点据此下结论。判据是 **curl 到的规则原文与磁盘一致**；不一致就换端口重起（`npx vite --port 5199 --strictPort`），不要猜「是不是 HMR 没生效」。这个坑只在**纯 CSS 改动**上出现，`.vue` 走 HMR 时反而正常，所以更容易误判成「我改错了」。

## 卡片

- **每张卡片的标题与内容之间必须有 1px 分割线。** 2026-10-06 用户拍板：标题行下面没有线，卡片就只是一块底色加一行字，读起来像「这段文字飘在卡里」而不是「这是一个标题和它的内容」。全前端只有一种写法，样式照 `theme.css` 的 `.card > h2` / `.card-head` 抄：

  ```css
  padding-bottom: 9px;
  border-bottom: 1px solid var(--border);
  ```

  - **色值只有 `--border`（`rgba(255,255,255,0.1)`）一种，不要为标题线另开一个更深的 token。** 它画在卡片**内部**、两侧同为卡片底色，看着淡；而同一个值画在卡片**边缘**（`.card` 自身描边）就相当清楚——差别在对比环境，不在线的强度。**先按 `--border` 写，用户真嫌淡再谈加深**。
  - **`padding-bottom: 9px` 与那条线是一套，不能只写 `border-bottom`。** 9px 是把标题文字和线分开的那段留白；只给线会让它紧贴文字。uiv2 改版从 6px 提到 9px：卡内边距与标题字号都变了，6px 会让线贴住标题文字。控制塔那份同步改了，`ui/test/cardChrome.test.js` 钉住两处必须相等。
  - **「线到内容」的距离不由这条规范钉死**，交给各卡片自己的间距机制：`.card` 靠容器 `gap`，`.diag-card` 靠标题的 `margin-bottom`。所以同一页里出现 4px（概览页的 `.card` 被 `OverviewPanel.vue` 的 scoped `padding: 6px 8px; gap: 4px` 收过）与 8px（其余卡片）两种是**对的**，与「各行盒高本来就不同」同理。要统一成一个数字就得反过来改容器的 gap，那会把整卡内容挤一遍。
  - **覆盖范围：概览页与四个面板里的卡，不含 5 个独立诊断窗口**（用户 2026-10-06 明确要求）。`.diag-card` / `.diag-card__title` / `.diag-row` 被那 5 页共用，它们装在 `.diagnosis` 容器里，是另一套布局与密度。**给概览页的控制塔补线要限定在 `.diag-card--tower` 上**——改基线 `.diag-card__title` 会顺带改掉那 5 个窗口，属于明令禁止的顺带变更。
  - **补线时注意 `margin-bottom` 该删覆盖还是该改值。** `.diag-card__title` 基线是 `margin: 0 0 8px`（本身没有 border）。**2026-10-07 改过方向**：这条原先要求「删掉 `--tower` 的覆盖、让标题回到基线 8px」，理由是 8 与 `.card` 基线 `gap: 8px` 是同一个数，补线时另写一个 5px 就成了凭空多出来的第二个数。现在用户要求继续压控制塔的纵向空白，整卡降到「卡内边距 4px / 行 4px」这一档，**「线到内容」也就跟着降到 6px**——挂着 8px 等于收紧只做了一半。所以 `--tower` 现在**显式写** `margin-bottom: 6px`，而基线 8px 一个字都不动（其余五个诊断窗口与别的卡片靠它）。两种「写法」都要避开：随手写 5px / 8px 这类凑出来的值，和干脆删掉让标题回基线。
  - **「标题到线」那一段（`padding-bottom: 6px`）不在上述收紧范围内。** 它是本节开头那条**全前端统一**的写法，概览页与四个面板的卡共用同一份值（`ui/test/cardChrome.test.js` 有一条断言比对控制塔标题与 `.card-head` 的 `padding-bottom` 相等）。给控制塔单独改成 4px 会让同一屏里上下相邻的卡给出两种分割线间距——而 4px 贴一条 17.55px 行高的标题也偏挤。要压就压「线到内容」与卡内边距，那两处本来就是控制塔自己的。
  - **别用「某条规则写了 `border-bottom`」当判据，要查层叠后的生效值。** 有一条规则声明了、又被同特异度的后一条盖掉，是静默的（见 `ui/test/panelSpacing.test.js` 那套层叠判据的做法）。

## 概览页

- **状态机只有一个主按钮「工作台」**：文案恒为名词，启停方向由 icon（▶ 启动 / ⏸ 停止）与 hover title 表达；同排的「官方对话」用同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」（运行后淡入）与「刷新工作台」是并列的次级入口，**不要再往主按钮旁边加会启停内核的动作**。「刷新工作台」在这排的特殊性：它是这一排里唯一的**动作**而非「把窗口带到台前」，之所以仍归这排而不是主按钮旁边，正因为它只换窗口、不碰内核状态。文案说用户得到什么，代码名说壳到底做了什么，两件事分开记。
- **次级入口这一排落在「版本号」那一行的右侧、靠右对齐**（2026-10-08 用户：「操作按钮迁移到版本号右侧，靠右显示」）。此前它们在三格指标**下方**独占一行，读数与动作被一段竖线分开，视线要往下再折一次。
  - 落位是**两行网格**（`kernel-summary-main`：`minmax(0,1fr) auto`），不是 flex + `margin-left: auto`：版本号与按钮同占第一行（按钮 `grid-column: 2` + `justify-self: end`），状态行 `grid-column: 1 / -1` 独占第二行。**理由是实测宽度**——用户 1040px 宽窗截图量得版本号 98px、三枚按钮 354px、状态行（状态胶囊 + 工作台地址 + 查看状态）287px，三者合计 739px，而卡内可用宽只有 482px。真挤在一行，状态行只剩约 110px，会折成三四行。
  - `justify-self: end` 是「靠右」的落点，不能省：网格项默认 `stretch`，不钉住这一格会被拉满整列、按钮贴着版本号而不是右边。
  - `.btn-row-sub` 的「左缩进 + 2px 竖线」随这次搬家删掉了：它表达「从属于上方那块读数」，而这一排现在与版本号同行，竖线会变成一条没有归属的孤线，还白占 16px 卡内宽度。压低权重这件事由 `.btn-sub` 的 ghost 风格继续负责。
  - 搬走后指标下方不再留空行——那一排是**移走**不是复制，`.metrics` 与后续内容自然上移。
- **判据「版本号与状态行同属竖向块」改写过一次，别按旧名字找它**。它原来断言 `.kernel-version` 与 `.kernel-status-row` 是「紧邻的两个兄弟」，按钮插进中间后那条正则必然失配。**但那条正则真正要拦的是「两者并排」，不是「两者相邻」**——把相邻当意图，就会在往块里加第三个元素时逼人改判据而不是改布局。现在改成查栅格落位（状态行跨两列、落在第二行），那才是「不并排」的结构事实。反向验见 `ui/test/designAlignment.test.js` 同名测试。
- **「查看日志」不在主操作排，在「系统健康 → 日志系统」；点开的是独立窗口**（2026-10-07 用户两次纠正，结论见下）。此前它在主操作排，且**与那一行做的事不一样**：按钮直接开独立全屏窗口（跳过列表），行打开日志弹层——同一屏两个日志入口、点开结果还不一致。删掉按钮之后又走错了第二步：把「入口统一」顺手做成了「容器统一」，让幸存的那一格也开弹层。用户原话是「查看日志按钮，应该是弹出 window 的形式查看」——**统一的是入口，不是容器**。
  - 现在这一格调 `logs.js::openLogWindow()`（不带文件名，由窗口自己挑：先 `kernel` 分组，再任意第一份；后端 `open_log_window` 为此接受空名，但 `read_log_file` 仍然拒绝空名，放宽只到开窗这一步）。
  - 事故 / 预检 / 诊断页那些**带具体某一份证据**的入口仍然走弹层：那不是「去看看有什么日志」，是「去看这一份」，弹层里能对照着切签。
  - **两个入口共用一份实现**（`logs.js::openLogWindow`）。这是本条最容易被顺手破坏的一半：两个组件各写一份 `invoke`，迟早一个改了另一个没改，于是又回到「行为不一致」——正是当初被用户点掉的状态。判据见 `logEntryPoint.test.js` 最后一条（它同时钉住 `open_log_window` 在全仓只发一次）。
  - **那一行的右侧提示必须写「查看」而不是 `›`**：它是唯一的日志入口，长成读数样式就等于把入口藏了。确认一份日志都没有（「暂无」）时该格**不可点**——开出来是一个空窗口，点了只会让人以为窗口坏了。
  - 弹层里的「全屏」按钮**不能删**：独立窗口现在有两条通路（这一格 + 它），两条都删光不会报任何错，只是用户再也开不了大屏看日志。
- 「刷新工作台」存在的理由是**看门狗覆盖不到**的那类黑屏：WebView2 渲染进程崩在页面加载完成**之后**，自动链条的判据（Started 之后没等到 Finished）永远不触发。手动路径的额度清账在 `harness_window::reset_budget`（后端），前端只负责发命令。
- **版本号是一段纯文字，不是徽标**（2026-10-08 用户截图：移除 tag icon、放大、靠最左）。设计稿 `.kernel-version` 没有边框 / 底 / 前置图形，本处取 20px / 700 / `var(--text)`（稿子的 18px 在这一格里不突出）。它此前挂在 `VersionBadge` 里，被一个 22px 胶囊关着：图形段与左右内边距把首字推离左缘，13px 的灰字也让「我现在跑的是哪个内核」像一行脚注。**去掉徽标后 `margin-left: 0` 必须显式钉住**——胶囊那段缩进没了，外层不会自动补回来。判据在 `designAlignment.test.js`（含「概览不再引 VersionBadge」，剥三种注释后才扫）。
- **三格指标的动作按钮配色归 CSS 管，模板上不许挂 `type="primary"`**：设计稿 `.button.text-action` 是 `color: var(--accent)` / 11px / 600。此前只有「模型用量」挂了 `type="primary"`、另两枚没挂，同一排三个同级入口里一枚蓝两枚黑。图标槽按 `.button-icon` 取 16px——EP 的 `.el-icon` 是 `font-size: inherit`，按钮 11px 会把图标一起缩到 11px，认不出是检测 / 图表 / 文件夹。指标标题（11px muted）与值（12px 600）本来就与稿子一致，判据按值钉住防止漂回去。
- **「数据目录」那一格指向应用根 `~/.dsh-xlink`，不是当前实例目录**（2026-10-08 用户定的）。实例目录 `~/.dsh-xlink/<family>/desktop/` 在 141px 的格子里被 `display_short` 截成 `~/.dsh-xlink/dsh/…`，尾巴上的省略号看着像一条坏掉的路径。显示读 `KernelStatus.xlink_home`，实例目录退到 `title`（**一个字都不许丢**），「打开」也开根目录——标签与动作必须指向同一个目录。

## 跨前后端的契约

这三条在两侧各有一半，改任一侧都要同时看另一份：

- **右键策略按窗口族分两份**：壳自己的五个窗口走 `ui/src/shell/noContextMenu.js`（`main.js` 在 `app.mount` 之前调一次，一律取消），这一半归本文件；官方对话三个内容 webview 加载的是**别人的**页面，走 Rust 注入的 `no-context-menu.js`（一律取消，含页面自绘菜单）；**工作台**加载的是内核 Web UI、它自己有右键功能（JSON 树「选择复制方式」、右侧栏标签页菜单），走 Rust 注入的 `context-menu-fallback.js`：放行页面自绘菜单、压掉引擎原生菜单，页面没处理且现场可编辑 / 有选区时再 invoke `show_workbench_edit_menu` 弹壳的原生编辑菜单（撤销 / 重做 / 剪切 / 复制 / 粘贴 / 全选，只读选区只给复制）。`check-invariants` 第 16 项按「每条 `WebviewUrl::External` 建窗链」逐条查**接的是不是本族那一份**——**另一半见 [src-tauri/AGENTS.md §右键策略](../src-tauri/AGENTS.md)**。
- **预检三态**：`precheck.rs` 的 `Verdict::as_str()` 与 `PrecheckDialog.vue` 的判定表靠字符串对齐，有测试钉死（`precheck::tests::verdict_strings_match_the_ui_contract`），改一边不改另一边会把"未通过"画成"通过"。判定为什么必须三态、基线为什么必须先跑，见 [src-tauri/AGENTS.md §预检](../src-tauri/AGENTS.md)。
- **「今日用量」只有一套口径**：概览卡片与独立「模型用量」窗口都读后端的 `today_tokens` / `today_requests`（`usage.js::todayUsage`）。窗口此前取 `sliceDays(days, 1)` 的最后一天，而后端会把晚于今天的异常日期追加到序列末尾——同一份统计会显示成两个数。`check-invariants` 第 ⑦ 项之五钉住接线。

## 图标

- **两套图标库并存，2026-10-03 起**：主体仍是 `@element-plus/icons-vue`（已接的图标不动，避免一次换掉几百处），**新补的图标走 `@lucide/vue`**（Lucide，ISC 授权）。选它的原因是 EP 的图标集里「标签 / 版本」这类语义只有实心造型的 `PriceTag`，缩到 11px 糊成一个黑点；Lucide 是 24 网格 2px 描边，缩到 11px 轮廓仍读得出。**按图标名 tree-shake**（`import { Tag as TagIcon } from '@lucide/vue'`），不要 `import * as`，否则 1857 枚全进包。
  - 一律**别名导入**（`Tag as TagIcon`）：面板里同时存在 `el-tag` 组件与一排 `*.tag` 类名，光看 `<Tag />` 认不出说的是哪一个。grep `TagIcon` 即为 Lucide 图标的全部用法。
  - 尺寸走**组件的 `size` 属性**，不要用 CSS 的 `font-size`：Lucide 渲染的是带 `width`/`height` 属性的 `<svg>`，字体缩不动它。颜色靠 `stroke="currentColor"` 自动继承，不用显式设。
  - **`shell/VersionBadge.vue` 已删除（2026-10-08）**：它的两个消费方先后退场——概览「当前内核」卡按用户要求把版本号改成纯文字（判据钉住概览不再引 VersionBadge），侧栏底部那一行用的是裸 `<span class="version-badge">`（样式只有 theme.css 里省略号与收起态隐藏两条）。组件成了零导入的孤儿才删的。**「同一语义不许两处各拼一个长得像的」这条纪律仍在**：将来再要版本徽标，先建组件再复用，别把样式抄进某一侧的 scoped 块或 theme.css。
- **面板里的第三方标志（npm 等）**：必须是 `ui/public/` 下的本地矢量、不许写远端 URL，并保留来源与许可声明——`tauri.conf.json` 的 `csp` 是 `null`，远端 `<img>` 出不出网完全取决于用户那台机器，取不到时页面上只剩一块白砖且没有任何报错（版本面板过去就挂着 `avatars.githubusercontent.com` 的 npm 头像）。由 `check-invariants` 第 15 项钉住。图标库里的图形组件不算「第三方标志」——它随包进产物，不发网络请求。
- 应用自身图标只从 `assets/*.svg` 母版生成，规则见 [docs/ui/icon-design.md](../docs/ui/icon-design.md)。

外观在设置页右栏提供三项选择，和侧栏循环按钮共用 `theme.js`。`theme` 保存 light / dark / system 模式，`resolvedTheme` 解析实际明暗颜色；实际颜色写入 `html.dark`；固定模式写入原生装饰，system 向原生接口传 null 解除覆盖，完成后重新读取媒体查询。每扇窗口监听系统配色，固定模式不响应系统变化，跨窗广播传递模式。旧偏好保留，默认仍为深色。

侧栏主题按钮按 `themeOptions` 的浅色、深色、跟随系统顺序循环切换；图标表示当前模式，悬停与可访问提示同时说明当前模式和下一次点击的目标。设置页仍可直接选定模式。

## 系统外观继承（2026-10-09）

macOS 的 Tauri `setTheme` 与建窗 `.theme(Some(...))` 会覆盖整个应用，不能用于窗口主题。建窗统一走 `shell::appearance::initial_theme(label)`，原生外观经 `set_window_appearance` 设置当前 `NSWindow`；壳页面按应用偏好纠正自有窗口，工作台由 `workbench-appearance.js` 按内核 `ui-theme` 偏好镜像（`system` 传 `null`，不冻住 `prefers-color-scheme`），官网页签窗不授外观权限。Windows 继续使用 Tauri 的窗口主题实现。

## 窗口 R 角裁切（2026-10-09）

四扇自绘壳窗口必须在 `body.custom-titlebar-shell` 使用 `clip-path: inset(0 round var(--window-radius))`。`#app` 的圆角与 `overflow: hidden` 无法约束越过包含块的固定定位投影；只裁切 `#app` 也约束不到 teleport 到 body 的遮罩。窗口外四角必须保持全透明，明暗主题、窗口缩放及弹出浮层均沿用同一裁切。官网页签栏与工作台保留原生窗口轮廓。

### 官网页签栏不吃 `#app` 的窗口圆角（2026-10-24）

上面那条纪律的另一半：**「保留原生窗口轮廓」不等于「可以继续吃壳那圈 CSS 圆角」**。`#app` 的 `--window-radius`（12px）是四扇自绘壳窗口的窗口轮廓，而官网页签栏是官方对话窗口顶部那条 38pt 高的页签带子——`#app` 的 `overflow: hidden` 会把它左右两端各切掉一块 12px 的弧，页签栏下沿那条 1px 横线因此在两端收成弧形而不是直角，读作「tab 左右边缘倒角」（2026-10-24 用户实机报）。原生装饰那边由系统画窗框，壳这层圆角对它没有意义。

修法是 `body.official-chat-strip #app { border-radius: 0 }`：`main.js` 在 `?chatstrip=1` 路由给 body 打 `official-chat-strip` 标记，theme.css 据此归零。**两边缺一不可**——只写 CSS 不打标记，那条规则永不生效而判据照样绿。判据在 `ui/test/windowChrome.test.js`（「官网页签栏不吃窗口圆角」），三条变异逐条反向验过：归零改回 `var(--window-radius)`、删掉 main.js 的标记、删掉整条 CSS 规则，三条都红。

注意 macOS **系统窗口圆角仍在**（官方对话窗保留原生装饰，窗口轮廓由系统画）：本条消掉的是壳这层 CSS 圆角，顶部两角仍会随系统圆角走。

### 官网页签栏：手写体 + 各站品牌色，内容页不再有壳画的条（2026-10-24）

用户一次拍板三件事：页签文字改手写体、颜色用各站品牌主色调、移除页签栏下方那条彩色条。

- **手写体用系统自带字体栈，不引 webfont**：`"Bradley Hand", "Snell Roundhand", "Segoe Script", "Comic Sans MS", cursive`。UI 产物预算的 CSS 只剩 ~11KB 空间，为一个字形引一套 webfont 不划算；而系统字体栈在 macOS 与 Windows 各有各的，跨平台零体积。字号 13px → 15px——手写体比默认无衬线小一号就发糊。
- **颜色由后端随页签下发**（Rust 侧 `official_chat::OFFICIAL_CHAT_BRAND_COLORS` → `official_chat_tabs` → 页签栏的 `--tab-color`）：页签栏是本地 SPA，不许写死十六进制（判据扫 `OfficialChatTabs.vue`）。前端兜底表带一份同值副本，两处的跨语言比对在判据第 20 项——不同值就是「IPC 一坏页签就变色」。
- **ChatGPT 取白档**：它的品牌主色是黑，而官网页签栏恒为深色（2026-10-08 用户拍板：页签栏若跟应用主题走，就是一条白条压在黑内容上），黑字在深底上读不出来——与 theme.css 里 `--window-edge` 明暗各一档是同一条纪律。
- **非激活态淡一档（opacity 0.72）**：三个站都是亮色品牌色，满色并排会互相抢；激活下划线也取该站品牌色，而不是壳自己的蓝——这一行的颜色语义已经是「哪个站」，下划线再走另一个色系会读成两件事。
- **内容页不再注入 `titlebar-pulse.js`**：那条 3px 品牌线是壳画在**别人的页面**顶端的，本就盖住人家的页头；品牌色改由页签栏文字承担。`titlebar-pulse.js` 只剩工作台用（Gitea 绿渐变）。判据第 34 项因此从「官方内容页必须注入」反转成「**不许**注入」——留着它就是那条线又回来了。

### 切页签的可见性时序：旧页保持到新页就绪 + 加载指示 + 开窗预热（2026-10-24）

用户报：「首次打开后点击 tab 切换，网页显示会延迟很久才加载出来。」取证后确认是**三层叠加**，缺一层都不会读成「卡死了」：

1. **内容 webview 按需挂载**：原先点 tab 才第一次建 webview 并现场导航，用户等的是整个站点首屏。实测 MiniMax 首屏要拉约 80 个 JS chunk + 33 个 CSS + 55 张图 + 4 个业务 API，且入口 `agent.minimaxi.com` 还会 302 跨到 `agent.minimax.cn`。
2. **新建的 webview 默认可见**：wry 的 `WebViewBuilder` 默认 `visible: true`（tauri 没覆盖它），它会在自己加载完成之前就盖在旧页面上面——用户看到的是「旧页瞬间消失 → 白屏 → 几秒后新页面才出来」。
3. **加载期零反馈**：页签栏既不转圈也不置灰。

修法三件（用户拍板 P0+P1+P2 全做）：

- **新建即隐藏，显示权交给状态机**：切页签的可见性收进 `official_chat::OfficialChatRuntime`（纯逻辑，可单测）。目标页签**未就绪时���切**，旧页面继续显示；等它的 `on_page_load(Finished)` 到了才切过去——切换点是「新页面已经好了」，而不是「用户点了」。「谁该被看见」只有状态机一处事实源，调用点不传下标。
- **加载指示**：后端随页签一起下发 `loading`（那一页还在加载），页签栏据此转一个小圈。加载期不给任何反馈时，用户读到的是「卡死了」而不是「在加载」。前端以 500ms 轮询刷新，只在**还有页签在加载**时才轮询（页签栏是常驻控件，无意义的定时器会一直占着事件循环），并设 60 次上限——站点一直加载不完时不无限轮询。
- **开窗后台预热**：其余页签在**独立线程**上后台挂载并加载（不占建窗线程，开窗不必等它），用户第一次点该页签时页面已经在了。预热失败不阻断开窗——那个页签在用户点中时会再挂一次。

判据在 `check-invariants` 第 35 项，钉的是「接线还在」：新建 webview 必须隐藏、切换路径不许自己 `show()`（显示权只走 `show_official_chat_tab`）、开窗必须预热其余页签、后端必须按状态机下发 `loading`、页签栏必须真的渲染加载指示。状态机本身的行为（未就绪不切、先加载完的页签不抢页面）由 `harness::official_chat` 的单测钉住。9 条变异逐条反向验过。

### 宽限期兜底：首屏迟迟不完成时先把页签显示出来（2026-10-30）

用户要求：「让风控挑战页显示给用户，和原生浏览器表现保持一致，让用户决定是否继续加载。」起因是 ChatGPT 页签一直转圈、内容还是上一站：2026-10-30 实机排查发现 chatgpt.com 的主框架 11:32:31 就已 commit、页面 JS 与网络一直活着，但 `NavigationCompleted` 近一小时没来——按上面那条规则，状态机就一直不切、用户永远看不到那一页。而**隐藏的 webview 里风控挑战页（Cloudflare Turnstile 这类）根本没法完成**，光等 `Finished` 是一个死结。

- **宽限期**：`OFFICIAL_CHAT_REVEAL_GRACE` = 1500ms。用户点中后到点仍未就绪，就先把这一页显示出来（页面继续加载，页签栏的转圈不收）。原生浏览器是点中页签就切过去、标签上转圈；这里多等 1.5 秒，给正常页面留出「就绪才切」的无白屏路径，超过它的页面本来就要等，先显示不会比原生差。
- **「谁该被看见」仍只有状态机一处事实源**：兜底走 `OfficialChatRuntime::reveal_after_grace`，只对**用户当前正在等**的页签生效；期间页面已经就绪、用户改点了别的页签、窗口已经关掉都 no-op。切页签路径仍不许自己 `show()`。
- **点回当前可见页签 = 取消等待**：`request_switch` 收到当前 active 时会清掉 pending——否则用户看了一会儿又点回原页签，稍后的就绪或兜底会把页面从用户手上拽走。
- **可观测**：兜底真的显示时落一条 `official-chat` 日志（`页签 N 首屏 1500ms 内未完成，已先显示该页（页面继续加载）`）；此前「一直在等」不落盘，出了事查无可查。
- 判据第 35 项补 ⑥：命令层必须派兜底、兜底必须走 `reveal_after_grace` 再 `show_official_chat_tab`（摘掉即红，已反向验）。

## 日志侧栏的分组折叠（2026-10-14）

用户要求「日志分组，增加点击收起分组的功能」。此前分组标题是纯文字，而日志随
「内核一天几份、每装一次插件一份构建日志」越长越多，整列要滚很久；「其它日志」
这类很少看的组永远占着地方。

- **标题变成 `<button>`**：整行可点，右侧一枚箭头（收起时 CSS 旋转 -90° 朝右），
  带 `aria-expanded` / `aria-controls`，键盘也能收起。收起的是**文件列**，标题常驻
  并显示这一组还剩几份——否则收起来之后连「这里还压着一类日志」都看不见。
  颜色仍走 token（`logWindowTheme.test.js` 那条「日志 UI 不许写死颜色」一并管着
  新增的 `.log-group-*` 规则）。
- **折叠状态放共享层**：`logs/logs.js` 的 `collapsedLogGroups`（`reactive(new Set())`
  + localStorage 键 `dsh.logs.collapsedGroups`），弹层与独立窗口共用同一个键。
  侧栏宽度是每个宿主的排版偏好（弹层窄、独立窗口宽），折叠不是——在一个宿主里
  收起、切到另一个又全展开，读起来就是这个开关没记住。只存**收起**的组：日后新增
  一类日志时它默认展开；反过来存「展开的组」，新分类在用户那儿默认收着，而开关
  藏在标题上，他未必知道要去点。
- **只在「换文件」那一刻自动展开**：激活的文件落在收起的组里时展开它（带证据路径
  打开日志、列表刷新后回退到第一份时都会遇到），但用户自己点收起的组不会被下一次
  列表刷新顶回来——列表重读会换掉 `groups`，每次渲染都纠正的话就成了「点了收起，
  一刷新又冒出来」。
- 判据 `ui/test/logGroupCollapse.test.js`：状态层（含 `reactive` 那一层——退回裸
  `Set` 时点一下界面纹丝不动且不报任何错）与形态层（模板按钮 + theme.css 的按钮
  重置、收起态、文件签那 2px 间隙随新容器搬家）。

## 概览页次级入口：隐藏「刷新工作台」（2026-10-15）

- **用户在一张概览页截图上指了「刷新工作台」，要求隐藏。** 它此前是次级入口那一排里
  唯一的**动作**（工作台窗口 / 官网网页版窗口都只把窗口带到台前）：它换掉整个工作台
  窗口，也就是换掉渲染进程。隐藏后那一排剩两枚、且都只做「带到台前」。
- **入口隐藏 ≠ 能力删除**（维护者选定「只隐藏入口」）。`store.js` 的
  `forceReloadHarnessWindow`、Rust 的 `harness_force_reload` 与
  `harness_window::reset_budget` **全部原样留着**。按 `instance.js::setDefaultInstance`
  那条先例——没有调用方的 API 不是「永远走不到的代码」，不许顺手当死代码清掉；要删要挪
  都是下一个决定。判据 `ui/test/designAlignment.test.js`「概览页不显示「刷新工作台」，
  一键重建那条能力不许跟着删」钉住这个现状：模板侧零调用 + 函数与那条命令仍在。
- **手动出路没丢，只是变成两步。** `open_harness` 的语义是「把窗口带到台前」，窗口开着
  时它只 show/focus、不重载；但**关掉工作台窗口再点「工作台窗口」**走的是完整建窗路径，
  同样换掉渲染进程。代价是多一次点击，以及自动重建的额度不会被清零（`reset_budget` 只
  在那条命令里调）。
- **文案跟着改的是「用户会照着做」的那一处**：跨壳提示（`instance.rs::
  other_shell_workbench_notice`）原来写「在那个壳的概览页点『刷新工作台』即可」，入口
  没了就是假指引，改成「在那个壳里关掉工作台窗口再重新打开即可」，判据也从钉某个按钮的
  文案改成钉两个动作词（它要拦的是「出路没写」，不是「出路必须长某样」）。
- **评审后补（2026-10-15）**：上面那份「本轮没动」的清单里，**用户真会照着做的那几处已经
  改掉**——`README.md:116` 与 `docs/operations/troubleshooting.md:41-42` 改成上面那条两步
  路径（原话是「优先用它」，指着一枚界面上根本没有的按钮），`harness_cmd.rs` 的拒绝文案
  与 `architecture.md:81` / `src-tauri/AGENTS.md:81` 里引用的跨壳提示原文跟着改。剩下的
  `dsh-xlink-ui-redesign-dev-guide.md:69`（设计稿说明）与几处只描述**机制**的段落
  （`reset_budget`、日志成因）仍写着那枚按钮——命令与机制都还在，它们说的是机制。

## 记忆面板页头被压扁：`flex-wrap` 补在原语上（2026-10-16）

- **用户给的截图**：记忆页整块页头被压成一条窄柱——`.page-desc` 一个字一行竖排，
  `el-switch` 的药丸叠在「记忆」上、「使用已批准的记忆」折成两行。
- **根因**：`theme.css` 的 `.page-head` 只有 `space-between`、**没有 `flex-wrap`**。
  `MemoryPanel.vue` 的 `.memory-switch { flex: 1 0 100% }` 在 nowrap 行里不退到第二行，
  而是把 `min-width: 0` 的 `.page-head__text` 压到 min-content（`.page-title` 是
  `flex: none`，被压的是副标题）。**与窗口宽度无关**——可用宽 776px 时照样塌。
- **落点选原语而不是面板 scoped**（Lead 初版写的是 scoped，被独立审计推翻）：另外六个
  页头只有一个直接子级，单个 flex item 无条件落在第一行 → 全局 wrap 在那六页**可证
  no-op**（任意宽度）；同族 `.card-head` 一直是 `space-between` + `flex-wrap: wrap`；
  成本上 theme.css 余 335 行，而 `MemoryPanel.vue` 只剩 2 行余量（scoped 版一落地
  `check:code-budget` 就红两条）。
- **判据补的是「同一个契约的两半」**：`designAlignment.test.js` 早就钉了
  `justify-content: space-between`，却没人钉 wrap——面板与判据的注释把「页头是 wrap 的
  flex 行」当事实写着，**注释不是判据**。反向验：摘掉 theme.css 那一行 → designAlignment
  红、memoryPanel 仍绿（两半各钉一半）。
- **同形扫描**（审计）：全 `ui/src` 只有这一处「basis 100% + nowrap 父」；另有 19 处
  `space-between` 且无 wrap 的容器今天都不坏（有 ellipsis 兜底，或没有不可压缩的兄弟），
  只登记、不改。
## 记忆编辑器：常驻表单改成点按钮弹出的对话框（2026-10-16）

- **用户在截图上指着右栏那张「新建记忆」卡提要求**：「新建记忆UI，调整为点击按钮后弹窗编辑」。
  落地：新组件 `ui/src/memory/MemoryEditor.vue`（弹窗 + 表单），`MemoryPanel.vue` 只留一枚
  primary「新建记忆」触发按钮；行内「编辑」走同一个弹窗（`openMemoryEditor(record)`）。
- **可见性放 store**（`memoryStore.editorVisible` + `openMemoryEditor` / `closeMemoryEditor`），
  与 `PrecheckDialog` 的 `precheckVisible`、`SnapshotRestoreDialog` 的 `restoreVisible` 同一个约定：
  面板只画按钮，判据不必挂载 Vue。
- **保存成功才关，失败一个字不动**：关掉一个装着用户刚写完正文的弹窗等于把那段文字拿走。
  所以弹窗里 `:close-on-click-modal="false"` **并且** `:close-on-press-escape="false"`
  ——遮罩与 ESC 都是「手滑」的路径，只拦一半等于同一条理由兑现一半（审计指出）。
- **按钮不放卡头那一排动作里**：三枚文字按钮已约 287px，卡内可用宽 348px；四枚并排约 390px，
  而 `.memory-head__actions` 自己不换行、EP 按钮又是 `nowrap` → 多出的 50px 会被
  `main{overflow-x:hidden}` 裁掉，最右的「刷新」直接看不见。现在它是 `.card-head` 的直接子级，
  与标题同行贴右缘。**页面头也不可行**：第二行要留给 `.memory-switch`（basis 100%），第一行
  已被标题 + 说明占满（审计量过：756/768）。
- **`.memory-note` 一族搬进 `memory.css`**：改完之后它被面板与编辑器两个组件用，而 scoped
  样式跨不过组件——正是那个文件顶部写着的落点规则。
- **预算**：新文件登记 140 行；`memory.js` 345 → 352；`TOTAL_BUDGET` 53630 → 53680（同处写账）；
  面板**净减 95 行**（493 → 398）。`docs/features/memory/memory-progress.md` 模块表补一行
  （`memory-progress[coverage]` 那条不变量会拦）。
- **判据**：`ui/test/memoryPanel.test.js` 新增「新建与编辑共用一个弹窗」（含「按钮不许塞进
  `.memory-head__actions`」这条量过的约束）与「弹窗开关与草稿：新建清空 / 编辑带出 / 关闭丢弃 /
  保存成功才关」（store 层真跑 `saveMemory` 两条路）。两条既有用例改指 `MemoryEditor.vue`。

### 弹窗那一版的评审修复（2026-10-16 晚）

三条都是弹窗引入的新问题，不是原有缺陷。

- **「检索预览」的查询不能再挂在草稿上**（评审第 1 条）。三个事实互斥：弹窗是 modal
  （遮罩挡着面板上那枚「试一次」）、关窗即丢草稿、面板上没有第二处能写草稿——于是**用户能点到
  它的时候查询必然是空串**。而后端对空查询不是「查不到」：`query.rs` 的 `score` 给所有在范围内的
  条目 **0 分**且不返回 `None`，预览会稳定报出一批与输入无关的命中，还长得很像真的。
  修法是**加第二个字段** `memoryStore.previewQuery`（编辑时同步、关窗不清、新建时清空），
  `closeMemoryEditor`「关窗即丢草稿」的语义一个字没动。面板那枚按钮改成 `:disabled="!previewQuery"`，
  空查询那一路给一句「先在编辑器里填标题或关键词」。
  **没选**「把按钮搬进弹窗 footer」：那样按钮点得到，但结果仍渲染在被遮罩盖住的面板上，
  等于换一个地方骗人；要搬得连结果一起搬，那是把「预览」从一个入口变成两个。
- **✕ 关窗先问一句**（评审第 2 条）。遮罩与 ESC 已拦，但 ✕ 是零确认路径。用 `:before-close`
  而不是 `@update:model-value`：EP 的 `handleClose` 是**先置 `visible = false` 再 emit**，
  拿后者做确认框只能假装拦一下（`MigrationPrompt` 早就用 `before-close`，照它写）。
  空表单直接关，有内容才问「放弃这条记忆？」。
- **保存期间的闸门要取真信号**（评审第 3 条）。原先「取消」写 `:disabled="globalBusy"`，而
  `globalBusy` 只由 `withExclusive` 抬——保存走的是 `withLoading('memorySave')`，**保存期间它恒为
  false**，那道闸门等于没有：点保存 → 请求在飞 → 用户以为卡住去关窗 → 草稿清空 → 随后保存失败，
  而正文已经不在任何地方。改成 `saving = computed(() => isLoading('memorySave'))`，
  「取消」`:disabled="saving"`、✕ `:show-close="!saving"`。
- **判据**：新增两条（`检索预览的查询存在 store 里…` 真跑 `editMemory → closeMemoryEditor →
  previewContext` 并断言发出去的 `queryText`；`编辑器关窗前先问一句…` 钉 `before-close` 接线与
  `saving` 信号）。**行为那半没进单测**：`beforeClose` 在 `<script setup>` 里，node 跑不到——
  留在真机 e2e。
- **预算**：`memory.js` 352 → 359、`TOTAL_BUDGET` 53680 → 53710（同处写账）。

## 磁盘用量：四张瓦片的位置固定，内核版本提到第一位（2026-10-16）

用户在截图（1.9 GB 那张）上说：「固定显示位置，不要因为空间占用改动位置。『内核版本』放到第一个位置」。

- **病症在 `diskusage.rs` 的最后一步**：`measure` 收尾有一条 `groups.sort_by(|a, b| b.bytes.cmp(&a.bytes))`，分类顺序跟着占用走。于是清理一次缓存、升一次内核，四张瓦片就重新洗牌——用户下次找「实例数据」得把整张表重扫一遍，而这类「位置即记忆」的界面最怕的就是这个。截图里 836 MB 的「插件 / 技能 / 备份」排在第一位，正是那条排序在作业。
- **改法：删掉那条排序**，顺序回到 `build_group` 的 push 顺序——内核版本 → 实例数据 → 插件 / 技能 / 备份 → 壳日志。这一顺序本身就是「①装了什么 ②你的数据 ③可重下的库 ④可随时清的日志」的读法，**首位正好是用户点名的「内核版本」**，不需要额外指定位置。
- **条目仍按字节降序**（`build_group` 里那条不动）。用户问「哪个最占地方」时从上往下扫，而那几行是同一类里可比的项。分类位置与条目排序是两件事，别一起改。
- **前端不重排**：`diskUsage.groups` 的顺序就是渲染顺序。前端再排一次会让「顺序」有两个事实源，而后端那条判据（`groups_keep_a_fixed_order_regardless_of_size`）会因此与前端各说各话。
- **列数也从 `auto-fit` 改成写死的两列**（`repeat(2, minmax(0, 1fr))`）。窗口固定 1040×748 不可缩放、卡片净宽约 784px，「恒两列」在真机上本来就成立；写死是把这份「本来就成立」变成结构，否则任何一次宽度变化（窗口尺寸、卡片内边距、侧栏收起）都可能重排成 1 列或 3 列。顺带删掉上一版那段「480px 窗口里 280×2+8 放不下两列」的调参说明——那个窗口尺寸现在不存在了。
- **判据**：`diskusage::tests::groups_keep_a_fixed_order_regardless_of_size`，**两次测量的占用刻意不同**（第二次让 `stores` 长成最大一类）。只测一次是假绿：大小不变时按大小排也不变，判据什么都没验到。反向验：把那条 `groups.sort_by` 放回去，判据红在「占用变了之后顺序不许跟着变」（实测 `stores` 跳到第一位）。

## 记忆面板：最近注入每行压回一行，这一屏不再有纵向滚动条（2026-10-16）

用户第二问：「优化 UI 界面，尽量不要出现纵向滚动条。」按用户那张截图逐像素量过：内容高 ≈772px、
可视高 710px，滚动条是真的在。

- **先量清楚是谁顶出来的**：右栏底部 ≈741px，左栏（记忆清单）底部 ≈466px——**整页高度全由右栏决定**，
  记忆列表压扁一行都省不掉。右栏三张卡里「最近注入」y104..382 最高。
- **每一行原本折成两行**：`2026-10-16 21:14 · /Users/zhongxingxing/2026/github/dsh-` + 换行
  `xlink · 注入了 1 条：开发约束`（在连字符处断）。五条形状一致：第 1 行 338px 顶满 342px 的文本框，
  第 2 行只有 157px。
- **改法是最小的一处**：项目名只显示最后一段（`labels.js::baseName`），完整路径进原生 `title`。
  去掉的前缀 `/Users/zhongxingxing/2026/github/` = 34 字符 ≈ 200px，单行降到 ≈299px < 342px ⇒
  5 条各省一行 18px、共 90px，多过溢出量 62px，滚动条消失（内容 ≈682px，余量 ≈28px）。
- **为什么不是别的**：给 `.memory-scroll` 减 `max-height`、给卡片减 padding 都只是把溢出挪个地方；
  项目名压短读起来更清楚，完整路径经 `title` 悬停就到——`f22ca5e` 那条「隐藏内容比出现滚动条更糟」
  在这里守住了：**截的是显示，不是信息**。
- **「记录文件」那行继续折行**：路径类长文本折行正是 `ui/AGENTS.md` 要的（不要 `nowrap` 截断），
  而且它省不回 200px（`tildePath` 只折 home 前缀，仍 ≈394px > 342px），折行是它唯一的出路。
- **判据两半都要**：`memoryPanel.test.js` 新增 `baseName` 单测（尾斜杠 / 反斜杠 / 只有根 / 空值 /
  缺字段），并在「最近注入」那条里钉 `baseName(note.workspace)` 与 `:title="note.workspace"`。
  只钉 helper 就是「helper 有测试、接线没有」那个坑。
- **落在 `shell/labels.js` 的理由**：`memory.js` 359/359 与 `MemoryRow.vue` 52/52 都已顶格、
  `labels.js` 没登记 `FILE_BUDGETS`；而且它与 `tildePath` 同属「窄列怎么显示路径」这一族。
- **没动的**：页面级纵向滚动仍是 `main` 的既有退路（记忆列表长到超过右栏时照旧出现滚动条），
  与概览 / 技能 / 插件同一套纪律——这一轮只消掉「固定内容也溢出」的那一段。

## 记忆面板：把「检索预览是模拟」改成说清「查询由你填、检索是真跑」（2026-10-16）

用户第四问：「如果检索预览是模拟器，那应该是一个 dev 功能，就需要在 release 版本隐藏它。」
**前提先被纠正，然后只改了措辞。**

- **「模拟」在这里指查询来源，不是假数据**：预览走的 `memory_preview_context`
  （`src-tauri/src/memory/cmd.rs:142`）与工作台真正注入时那条 `/v1/memory/context`
  （`src-tauri/src/memory/routes.rs:50`）调的是**同一个 `query::retrieve`**——同一份记忆库、
  同一套打分、同一个字节上限，只有查询词换成你填的。`cmd.rs:122` 那对词说的就是这件事：
  「模拟（拿你填的查询算一遍）vs 实况（插件报告的真实注入）」。
- **所以没进 dev 门禁**：设计文档把它列为管理页的固定面（`dsh-xlink-memory-design.md:269`），
  而且这一屏只有它把**要进上下文的正文**（`preview.text`）摊开——最近注入只报「哪一回合、
  哪个项目、注入了哪几条」，还是进程内环形缓冲（`recent.rs`，壳重启即空）。两个一起在 release
  收掉一个，冷启动打开记忆页的用户就再没有地方能回答「检索会不会命中、命中长什么样」。
- **改法是同一行替换**：`MemoryPanel.vue:359` 那句「下面那个「检索预览」是模拟——拿你填的查询算一遍。」
  →「只是查询词由你填，检索是真跑的（同一份记忆库、同一套打分）。」文件行数与生产代码总量都没变
  （当时总预算只剩 3 行）。
- **没动的**：`memory.js:65` / `cmd.rs:122` / 设计文档里的「模拟 / 实况」是**内部术语**（区分
  查询来源），对用户不可见且含义精确，不跟着改；卡片结构、按钮与既有判据一条不动。

## 记忆面板：列表内部滚动、卡片压两行、点开弹窗、按钮做成徽章（2026-10-17）

用户原话：「记忆列表做成内部滚动条，默认内容精简收缩显示的卡片，点击后弹出操作界面。界面的操作按钮默认显示按钮 badge」。
起因是这一屏的既有形状：每张卡片摊开全文，右侧还有一排常驻的文字动作链接，三条真实记忆就能把列表拉得很长。

- **滚动落在列表自己身上**：`.memory-list { max-height: 320px; overflow-y: auto }`（`memory.css:90`）。
  这条**推翻了 2026-10-16 那条「页面级纵向滚动仍是 `main` 的既有退路」**——那个说法在只有三条
  短记忆时没暴露问题，全文摊开后就变成「整页滚动条只为一段列表出现」。320px 取自同屏两个既有值：
  右栏「最近注入」用 `.memory-scroll` 224px，而整屏高度由右栏 ≈741px 决定——320px 能露出 2.5~3 张
  压扁的卡片，也不至于把右栏顶下去。**内部滚动与页面滚动不是二选一而是先后关系**：列表不再长过右栏，
  页面级那条就没有触发的机会。
- **卡片默认精简，靠行数截断而不是截字符串**：`.memory-row__body--clamp` 用 `display: -webkit-box` +
  `-webkit-box-orient: vertical` + `-webkit-line-clamp: 2` + `overflow: hidden`（`memory.css:110`），
  两条 `line-clamp` 名字都写。**截的是显示不是信息**：全文一个字不删，进详情弹窗读（与本仓
  `f22ca5e` 那条「隐藏内容比出现滚动条更糟」一致——这里没有隐藏，只是折叠）。
  `-webkit-box` 作为 subgrid 子项会不会被块化，无浏览器可验，按标准走；这条已在代码注释里写明。
  （当天的实测把它从「按标准走」变成了「验过」：见下面那条。）
- **点整块开弹窗**：`MemoryRow.vue` 根元素 `role="button"` + `tabindex="0"` + `aria-haspopup="dialog"`，
  Enter / Space 都绑。**不用原生 `<button>`**：`.memory-row` 的布局依赖 `grid` + `__main` 的 subgrid，
  这一屏已经因为动结构回归过两次（正文被挤成左窄条 `e6e2753`、候选行按钮压住标题 `379525b`）。
  行因此只发一个 `open` 事件，`edit`/`toggle`/`remove` 三个事件名从行里彻底消失。
- **动作按钮进弹窗底部，且是徽章**：`.memory-actions .el-button.memory-badge`（`memory.css:131`）——
  读法沿用控制塔的 `.diag-row__cta`（`diagnostics.css:445`），但**字号 12px 而不是 11px、`padding`
  也更大**：那个是纯 `<span>`，这个是 `el-button`（自带 32px 高、`padding: 8px 15px`），照抄尺寸会
  得到一个比行还高的胶囊。三档用 `--accent` / `--warning` / `--danger` 的 `-fill` / `-line` 令牌；
  禁用态**保持配色、只降不透明度 0.5**（(0,3,0) 的基础规则本来就压过 EP `.is-disabled` 的 (0,2,0) 灰色），
  `:not(.is-disabled)` 保住 hover 不跟着加深。`margin-left: 0` 与 `height: auto` 必须自己写：
  EP 给每个 `.el-button` 加了 `margin-left: 12px`，而 `.panel .el-button { margin-left: 0 }` 伸不进
  `append-to-body` 的弹窗。
- **`MemoryDetail.vue` 独立成组件**：`MemoryPanel.vue` 只剩 1 行余量（494/495），弹窗的模板与三个动作
  接线没有地方放。它复用 `memory-row__main` / `__title` / `__body` / `__meta`（同一族类名，样式在
  `memory.css`），只是正文不带 `--clamp`。
- **真浏览器实测**：headless Chromium 挂真实 `theme.css` + `memory.css` + Element Plus `index.css`，
  378px 宽列、6 行假数据，详情弹窗的 footer 直接挂在 body 级（模拟 `append-to-body`）。量到四件事：
  ① `.memory-list` 的 `clientHeight` 320 / `scrollHeight` 680，程序化 `scrollTop` 落到 360，而文档高度
  等于视口高度——**内部滚动真的在滚，页面级滚动条没有被触发**；② `-webkit-line-clamp: 2` 那句所在
  元素的计算 `display` 是 `flow-root`（Chromium 确实把 subgrid 子项的 `-webkit-box` 块化了），但截断
  照旧生效：实测高 39px = 2 × 19.5px 行距，`scrollHeight - clientHeight` 溢出的 137px 被藏起来；
  ③ `.memory-row__main` 的 `grid-template-columns: subgrid [] [] []` 生效（`grid-column: 1 / -1`），
  正文宽 320 / 行内宽 344，**不是上一轮那种左窄条**；④ 徽章 25.19px 高（19.2 行高 + 4 上下 `padding`
  + 2 边框）、圆角 999px、三枚的 `margin-left` **全是 0**（EP 那条 `.el-button + .el-button` 的 12px
  被压住），配色解析到 accent / warning / danger 各自的 `-fill` / `-line`；同屏对照的裸 `.el-button`
  是 32px 高 / 6px 圆角 / `padding: 8px 15px`。页脚最后一枚徽章右缘 713 = footer 右缘 713，
  `.dialog-actions` 的 `flex-end` 生效。**没量的**：悬停与禁用态（由层叠判据钉）、真跑 Vue 组件与
  teleport（静态标的结构）、378px 之外的其他宽度。
- **判据**：`memoryPanel.test.js` 新增「记忆行是精简卡片、点开才出操作：动作按钮是弹窗底部的徽章」
  一条（七段：行无按钮、可点语义、列表容器与内部滚动、`detailRow` 从 `allMemories` 取、弹窗挂载与
  四个事件、footer 三个徽章、徽章层叠生效值），并改了两条旧判据——原文钉的是「行里发 `edit` / 面板接
  `@edit="openMemoryEditor"`」，现在钉「行只发 `open`」与「编辑从详情转手 `editFromDetail`（先关详情
  再开编辑器，两个弹窗都 `append-to-body`，叠着开就是两层遮罩）」。21 处变异逐个跑过，全部被抓。
- **没动的**：候选行的三个文字按钮、`.memory-list` 之外的所有既有结构、`memory-scroll` 的 224px。

## 记忆面板：候选并进同一条列表视图，整页钉进窗口高度（2026-10-17 晚）

用户第五问（截图里用红框圈出「记忆」卡与「待确认」卡）：「这部分的 UI，需要继续优化，让布局再紧凑
一些，尽量不要出现纵向滚动条。」

- **先把页面到底有多长量出来**（无头 Chromium 驱动**真组件**：`npm run dev:ui` 起的 vite + 在
  `preload` 之前注入 `window.__TAURI__` 的 mock，数据取本机 `memory.json` / `jobs.json`，
  视口 1040×748 与窗口一致）：`main.clientHeight` 710、`scrollHeight` 1047——**页面真的溢出 337px**，
  用户截图里标题被滚出去就是这个。两列分别 929 / 599：**整页高度全由左栏决定**，而左栏的两张卡
  加起来就是 929（记忆 481 + 间距 12 + 待确认 436）。
- **上一轮那条 `max-height: 320px` 只压住了记忆列表，没压住候选卡**：候选卡挂在它下面，仍按内容
  长到 436px。两条列表各自设上限也解决不了——两张卡本身就有 169 + 83 = 252px 的固定开销，5 行内容
  再把 592px 的可用高度吃光。**候选不是第二份内容，它是同一份清单的另一档视图**，所以这一轮的正解
  是并成一张卡：筛选那一排从三档变四档（`全部 / 本项目 / 通用偏好 / 待确认（N）`），列表容器复用
  同一个 `memory-list`，候选行的正文、来源、冲突 / 取代标记与三枚动作一个字没动（整行搬进
  `MemoryCandidateRow.vue`）。
- **整页钉进窗口高度**（`MemoryPanel.vue` 的 scoped 块）：`.panel { height: 100% }` 拿满 `main` 的
  内容高度、`.page-layout { flex: 1 1 auto; align-items: stretch; grid-template-rows: minmax(0, 1fr) }`、
  左栏卡片 `min-height: 0`、`.memory-list { flex: 1 1 auto; min-height: 0; overflow-y: auto }`。
  **高度链每一级的 `min-height: 0` 都是承重的**：flex / grid 子项的默认下限是 min-content，少一级
  就会撑回内容高度、页面照样滚（这条在 `.memory-list` 的实测里验过：12 条记忆时列表 431px 内部滚动、
  `main` 的 `scrollHeight` 仍是 710）。
- **卡片高度跟着内容走，不拉到底**（`flex: 0 1 auto` + `overflow: hidden`）：3 条记忆时卡片 512.5px、
  列表 352px 全部可见；写 `flex: 1 1 auto` 会让卡片拉满 592px、下面空出一大块（截图对比过）。
- **次行的项目名只留最后一段**（`baseName`，完整路径进 `title`）：`.memory-row__meta` 在 324px 里
  需要 438px，整条路径让它折成两行——每条记忆多占 18px。这与「最近注入」那处是同一先例，
  D5 要求的五项（范围 / 来源 / 验证 / 时间 / 已用次数）一个没少。
- **行盒再压一档**：`.memory-row` 的 `padding: 10px 12px → 5px 12px`、正文与次行的 `margin`
  `4px → 3px / 2px`。一条记忆 125.5 → 112.5px，三条就省出一行的高度。
- **候选行的动作排沉底**（`.memory-row--candidate { grid-template-columns: minmax(0, 1fr) }` +
  `__actions { grid-column: 1; grid-row: 4; justify-content: flex-end }`）：动作挤在标题右侧时第 1 列
  只剩 ~190px，「已安装 cargo-cache 0.8.3」折成三行、来源标签再掉一行。来源标签同时从原样打印
  `item.source.kind`（屏幕上写着 `extract`）改成 `sourceText()` 的中文映射。
- **改法是「少掉一处入口」还是「多一个档」**：候选卡没了，但候选本身没有藏起来——数量写在第四档的
  标签上（没有候选时不画这一档，已经在那一档里时例外，否则判完最后一条就没地方切回来了）。
  与本仓 `f22ca5e` 那条「隐藏内容比出现滚动条更糟」一致：这里一屏能同时看到 3 条记忆、右栏三张卡，
  页面级滚动条 **0**。
- **实测（改后）**：`main.clientHeight 710 / scrollHeight 710`（溢出 0）；记忆视图 3 条全可见
  （列表 352 / 352）；候选视图 2 条全可见（列表 273 / 273）；12 条记忆时列表 431 内部滚动、页面仍 0；
  浅色与暗色各截一张，无配色回归。
- **判据**：`ui/test/memoryPanel.test.js` 改了 8 条、新增「窄列里的路径只显示最后一段」1 条
  （三处 `baseName` 与两处 `title`，四条变异逐个验过会转红）；`ui/AGENTS.md` 的层叠纪律用上两处——
  ① 候选那一档的接线改查 `MemoryCandidateRow.vue`（动作搬了文件，判据还在盯面板就会绿在「组件已经
  接上、面板什么都没有」上）；② `.memory-list` 的 `max-height: 320px` 换成「`flex: 1 1 auto` +
  `min-height: 0` + 不再写死上限」，并补上 scoped 高度链的四条断言（scoped 规则的祖先条件层叠工具
  读不准，按本仓规矩读剥注释后的规则文本）；③ `memory-row__actions` 的 `grid-column` 不能再走
  `effectiveDeclaration`——候选行那条后代规则与基线同主语而工具是祖先盲的，会读到候选那条的 `1`，
  改成按规则原文查（`ui/AGENTS.md` 记的就是这个坑）。反向验：把 `.memory-list` 的 `min-height: 0`
  删掉，高度链那条判据转红；把候选行的动作排规则去掉 `justify-content`，对应断言转红。
- **拆分与复用**：候选行按职责拆进新组件 `MemoryCandidateRow.vue`（正文、来源、冲突 / 取代标记、
  三枚动作都在它身上），面板只留一行 `<MemoryCandidateRow>`；它与只读、点开才出操作的
  `MemoryRow.vue` 是两种角色，合并会让「默认精简、点开再操作」失去唯一的形状。重复代码检测
  （`npm run check:duplicates`）4 处 / 允许 8 处，本次没有新增重复区间；行数与新文件登记预算
  自 2026-10-17 起已取消（改由「按职责拆分 + 评审说明」约束）。

## 记忆面板：标签写具体项目名，筛选那一档改名「按项目」（2026-10-17 晚）

用户第六问（箭头指着记忆行上的「本项目」标签）：「这里不应该是『本项目』而是具体会话的项目名。
上方选择的 radio 应该改名为『按项目』。」

- **「本项目」等于没说**：一条记忆属于哪一个项目，是这一行最要紧的身份信息，而标签复述了一遍
  当前上下文。改成 `baseName(workspace_path)`（`dsh-xlink`），完整路径进标签的 `title`；
  通用偏好仍写「通用偏好」，`title` 写明「所有项目都算」。列表行与详情弹窗**同一口径**
  （同一件事不许两个名字）。
- **次行不再重复**：标签已经说了项目名，次行原来那截「本项目 · dsh-xlink」删掉，只留
  来源 · 验证 · 更新时间 · 已用次数。**这一删顺带把次行压回一行**：实测 `memory-row__meta`
  从 36px（两行）降到 18px，一条记忆 112.5 → 94.5px，三条就省下 54px；3 条记忆时列表
  298/298 全可见，12 条时 431/1211 卡内滚，页面级滚动条仍为 0。
- **radio 改名「本项目」→「按项目」**：这一档列的是**所有**项目范围的记忆（`scope !== 'general'`），
  不是「当前这个项目」——旧名字会让人以为它按当前目录过滤。编辑器里的范围选择仍叫「本项目 /
  通用偏好」（那里问的是「这条记忆属于哪一档」，与视图筛选不是一回事）。
- **判据**：`memoryPanel.test.js` 的「面板有范围筛选…」换标签；「窄列里的路径只显示最后一段」
  那条扩成**两个组件**（`MemoryRow` / `MemoryDetail`）都钉 `baseName` + `:title="scopeTitle"`
  + 标签渲染 `projectName`，并加一条「次行不许再出现 `{{ place }}` 那种范围前缀」。

### 记忆面板整体消歧义（2026-10-18，用户要求）

用户原话：「主窗口『记忆』功能的相关 UI，也要消除歧义」。逐个界面核了一遍，**九处会读错的
地方**（另有两处顺手改的措辞），每处都对着后端事实改，没有一处是纯口味：

- **页头副标题**：原写「相关会话会带上这些内容」——用户会以为库里所有记忆都进了上下文。
  实际是 `query.rs::sorted_hits` 按当前这句话匹配、`MAX_HITS = 6` 封顶，只有 `pinned` 那条
  与查询无关。改成「按当前这句话挑出相关的（最多 6 条）带进会话，固定项每次都带；没被挑中
  的，模型可以自己按关键词查」。
- **开关的作用域**：开关文字只写「使用已批准的记忆」，而它**同时管自动提取**——
  `jobs_routes.rs` 的 enqueue 在 `!meta.use_approved` 时回 `disabled`。想停掉自动提取的人
  找不到这个开关，想留着提取的人也不知道关掉会一起停。作用域那句进 ⓘ（`switchEffectDetail`）。
- **开关的 toast 与行内说明互相打架**：toast 原写「新会话会带上相关记忆」，而行内
  `switchEffectText` 写「通常下一步检索就生效」。**行内那句是对的**（`pre-step` 每步都问一次），
  toast 改成与它一致；行内那句另补「用尽后要等**压缩**或新会话」——压缩会把预算清零
  （`policy.js::afterCompaction`），原来说「等新会话」漏了这条更常见的出路。
  **次数不写死**：那个常量在插件里（`MAX_INJECTIONS_PER_SESSION`），`MemoryStatus` 没下发，
  前端写死一个数会在下次调预算时静默过期（这一轮刚把它从 3 改成 8）。
- **`已用 N 次` → `模型读过 N 次`**：`use_count` **只由 `memory_read` 增加**
  （`store.rs::read_and_note_use`），检索命中与「点开看看」都不计数。「已用」会被读成
  「被注入过几次」，而那条信息在「最近注入」卡上。代价是次行长约 24px；
  `.memory-row__meta` 是 `overflow-wrap: anywhere`（折行不截断），信息不丢。
- **任务行的英文状态**：`jobLine` 原样拼 `job.state`，中文界面里出现 `#42 failed`，而摘要
  那句写的是「待跑 / 在跑 / 完成 / 失败」——同一屏里同一件事两个名字。改中文标签
  （`jobStateText`），`#42` 也改成「第 42 条事件」（它是会话事件序号，不是第几条任务）。
- **「自动提取（P2）」**：`P2` 是内部阶段名；那句话另外补了「提出来的先进待确认，你点了保存
  才成为记忆。它跟着上面那个开关一起开关」。
- **「按项目 / 项目记忆 / 本项目」三个名字**：筛选那一档叫「按项目」（列所有项目范围的记忆），
  编辑器单选与详情范围文案改叫「项目记忆」——编辑器那个下拉让用户**任选**工作区，选中的
  未必是「当前项目」。`projectName` / `scopeTitle` 的兜底也从「本项目」改成「项目记忆」。
- **卡头计数与列表对不上**：筛选到「按项目」时列表短了，计数还是全库的数。改成
  「记忆（显示 N / 共 M）」，只在非「全部」档显示两个数。
- **候选行的来源会话**：原来把 `session-fbb5df89-a57a-4f93-a645-8bc184dfa4b4` 整条铺在行里。
  改成短 id（`sessionText` → 「会话 fbb5df89」），完整 id 进 `title`——设计 §2 要能追回是哪次
  会话提的，所以不能只写「来自工作台」。
- **顺手两处**：「没有命中」→「没有匹配到记忆」（「命中」是检索术语，主语不明）；
  导出那行补「一次性快照，不参与检索」。

**没动、但记一笔**：编辑器三档验证范围里只给「已验证」补了「（你确认过）」——三档一起加括号
会把单选排顶成两行（弹窗 560px 减两侧内边距约放得下 26 个汉字）。另外
`README.md` 说「当前版本不做自动提取：面板上那个开关保持关闭并注明原因」，而代码里
`meta.auto_extract` **没有任何读取方**（`export.rs` 只把它写进导出头部），提取的闸门是
`use_approved`——真机 `jobs.json` 里已有 54 条任务。这是文档与实现的漂移，**属于产品口径，
不在这一轮 UI 文案的范围**，交维护者定。

**判据**：`memoryPanel.test.js` 新增「面板的文案不许把内部术语、英文状态或会读错的计数摆给
用户」「范围文案统一成『项目记忆』」「候选行的来源会话给短 id」三条，并在原有的
`injectionText` / `jobLine` / 提取任务模板三条里换掉旧字符串；`templateOf` 扫模板不扫注释，
所以解释性注释里写回被判掉的字面量不会把判据骗绿。

#### 独立验证又逮到五条，同轮收口（2026-10-18）

一位独立验证者（只读 + 两次「改坏 → 恢复」）在产物与源码上复核后，又列了五条高危歧义。
逐条收口，**其中两条改的是产品口径而不只是措辞**：

- **「打开右侧开关后」是错的方位**：`.memory-switch` 写 `flex: 1 0 100%`，开关实际在页头
  **第二行整宽**，右侧什么都没有——用户按字面往右找会找不到它。改成点名的写法
  「打开「使用已批准的记忆」开关后」（不写方位就不会再随布局漂）。
- **「最多 6 条」把 `query.rs::MAX_HITS` 写死在前端**，而 `MemoryStatus` 没下发这个字段，
  没有任何判据会在它变的时候变红——同一个形状当天刚在插件的注入预算上发生过（3 → 8）。
  收口方式是**加字段**：`MemoryStatus.max_hits`（值直接取 `query::MAX_HITS`），面板读
  `status.max_hits` 渲染，状态没读回来时只说「条数有上限」，不猜一个数。这也把本文件
  上一段写的「次数不写死」从口号变成了机制。
- **「固定项每次都带」是过度承诺**：`pinned` 只影响**排序**，它与命中项一起占那几个名额
  （pin 超过上限时排最后的那条每轮都进不去），项目范围的固定项还只在它自己那个工作区生效
  （`query.rs::scope_matches_fields`）。页头与检索预览改成「优先带上 / 排在最前」，
  编辑器里那个复选框也从「每次都带上」改成「优先带上」。
- **同一个 `stale` 两处两个意思**：候选行那个是「来源那段话被改过」（H7），记忆行那个是
  「事实可能过时」（verification）。候选行的标签从「可能已过期」改成**「来源已变」**——
  候选被批准时后端会把 verification 重置成 `verified`（`store.rs::decide_candidate`），
  按「事实过期」去理解是白担心一场。
- **失败行里夹英文阶段名**：插件把 `${stage}：${reason}` 原样拼进错误串，面板原样渲染，
  于是中文句子里出现 `model：…`。`memory.js` 加 `stageText` / `errorText` 做**显示层**映射
  （读取会话 / 切分片段 / 模型调用 / 解析输出 / 提交候选 / 已取消，取值与
  `plugins/memory/host/run.js` 的 `STAGES` 同一套），认不出的阶段原样留着；提取任务那个 ⓘ
  的阶段清单也补全（原来漏了 `slice` 与 `cancelled`，而这两条都会真的上报）。

另外两条顺手：`sourceText` / `verificationText` 的「认不出就原样显示」补上注释，与
`jobStateText` 同一口径（后者本来就有）；`store.rs` 里引用旧文案「已用 N 次」的注释同步。

**没做、留着的**（验证者列的中低危项，判断为不值得改）：行标签在路径缺失时显示范围名
（后端 `project_scope_requires_a_workspace` 决定了这条几乎走不到）；「未知项目」与
「项目记忆」分属两张卡、两件事；卡头计数含停用条目（每条停用行自带「已停用」标签，
右栏另有分开的两个数）；「memory-jobs 标签」是日志窗里真实的标签名，可执行可核对。

### 第二轮：把标准提到「精确、不过时、说清用途」（2026-10-18）

用户把标准写死了：「给用户看的面板，必须语义精确不能有歧义，不能展示过时、老旧的信息，
要让用户清晰明确的知道功能用途」。上一轮判为「不值得改」的几条因此重开，另外按
「**不许展示过时信息**」把面板之外两处用户看得见的地方也扫了。

**清掉的过时/不实信息（这轮的重点）**

- **导出 Markdown 的头部写着不实状态**：`自动提取 = 关（P2，尚未实现）`。三处都错——
  `P2` 是内部阶段名；「尚未实现」与事实相反（真机 `jobs.json` 57 条任务，done/failed 都有）；
  取值来自 `meta.auto_extract`，而那个字段**全仓没有读取方**（真正的闸门是 `use_approved`：
  `jobs_routes.rs` 的 enqueue 与 submit 各有一道）。改成两行如实描述：开关状态 + 「这个开关
  管两件事：记忆注入与自动提取」。同一份导出里 `命名空间` → `记忆库`（三段含义写进括注）、
  `已用：N 次` → `模型读过：N 次`。
- **会话头徽标的零命中分支在说错话**：`hits` 为空时原文案写「（标题取不到，只留了 id）」，
  而空数组只意味着「这一回合没有任何记忆进入会话」（标题取不到时壳会把 id 填进数组）。
  按「有命中 / 零命中 / 开关关着」三支分别如实描述；文件头那段「零命中不渲染、因为只在真
  注入时才上报」的注释也是错的（`applyStep` 在零命中与 `enabled=false` 时都回 `applied: true`），
  按实测改正并补了判据（4 种形状 × 2 语言，逐条钉该有的与**不该有**的片段）。
- **徽标正文 `记忆 N` 读不出 N 是哪一层** → `本轮记忆 N 条`（en 带单复数）。徽标旁边没有别的
  字可对照，计数必须自己说清是「这一轮注入的条数」而不是库里总数。
- **方位引用会随布局过期**：「权威记录是上面那份文件」——那份文件在下面那张卡里。改成点名
  「磁盘上的 memory.json」，不再指方位。
- **面板里的内部术语**：「壳发一个租约、插件排一条」改成用户视角（「聊完一轮之后，模型会把
  这一段里值得长期保留的东西提成候选，每一行是聊过的一段」）。

**语义精确化**

- 卡头计数含停用条目（`memory_list` 带 `include_disabled`）→ 有停用时写「记忆（N，含停用 M）」。
- 「未知项目」→「未记录项目」：前者把「没记下来」说成「项目不存在」。
- 行标签路径缺失时 → 「项目记忆（未记路径）」：那一格平时显示的是**项目名**，只写范围名会
  让同一位置出现两种语义层级。
- 开关 toast 的「按需」→「按当前对话挑出相关记忆带进会话」。
- **跨语言常量加判据**：`memory.js::MAX_BODY_BYTES` 原先是抄来的 8192（注释写「与后端逐字
  对齐」，但没有任何东西真的在比）。改成判据**读 `store.rs` 的常量源码**再比对——后端改上限
  时它会红，而不是让面板按旧值拦人。

**记在账上的（需维护者定，未擅自改）**：`README.md` 仍写「当前版本不做自动提取：面板上那个
开关保持关闭并注明原因」，`cache-impact.md` 也把 `auto_extract` 当活开关——而代码里它没有
读取方、提取跟着 `use_approved` 跑。这是**产品口径**（要么让那个字段真的管住排任务，要么删掉
字段并改文档），不是文案能收口的。`MemoryStatus.auto_extract` 现在是没人读的死字段，留着但
不该再被谁拿去展示。

## 记忆面板：候选行点开详情、动作抽共用组件、自动落地后的文案（2026-10-19）

用户三条反馈（截图指着「待确认」里那条「dsh-xlink 在 Windows 上遇到杀毒软件拦截」的候选）：
① 「待确认的记忆信息不能点击查看完整信息」；② 一次性、特定项目才有的排障事件不该被提取；
③ 既然是自动提取，为什么还要用户手动判断。①②③ 里 UI 这一半是 ① 与 ③ 的文案收口；
② 与 ③ 的行为改动在插件提示词与 Rust 落地（见 `docs/features/memory/memory-progress.md`
的 H10 / C15）。

**候选行：可点的是正文那一块，不是整行**

- 与 `MemoryRow.vue` 同一形状（列表只负责读，点开才是详情）：`.memory-row__main` 上挂
  `role="button"` + `tabindex="0"` + `aria-haspopup="dialog"` + `aria-label`，`click` /
  `keydown.enter` / `keydown.space` 都 `emit('open', item)`。
- **不能挂在根上**：候选行的根还挂着动作排（保存 / 存为通用偏好 / 忽略），挂根会让「点忽略」
  也去开弹窗。可点区域里还嵌着关系块的三枚按钮（停用这条 / 删除这条 / 丢弃这条），它们的事件
  会冒泡上来——所以点击与回车共用 `event.target.closest('button, a')` 闸门；空格的
  `preventDefault` **放在闸门之后**，先拦会让那三枚按钮的键盘激活失效。
- `memory.css` 的 `.memory-row--clickable` 注释同步写明这一族类名现在挂两处（记忆行挂根、
  候选行挂 `__main`），规则值一个字没动。

**详情是新组件 `MemoryCandidateDetail.vue`，三枚动作抽成 `MemoryCandidateActions.vue`**

- 详情写法照 `MemoryDetail.vue`（`append-to-body`、`width="min(560px, 92vw)"`、
  `:close-on-click-modal="false"`），内容：完整正文（**不带** `memory-row__body--clamp`）、范围
  标签（项目名取最后一段、完整路径进 `title`）、来源 tag、**完整会话 id**（行里那份是短的）、
  创建时间、验证状态、H7 黄框与冲突 / 取代两块标记。
- **动作抽出去而不是各写一份**：行与弹窗都要裁决同一条候选，两边各写一份的结局是迟早一边漏了
  `'general'` 或 loading key，而这两种错都不报任何错。共用组件的根就是 `.memory-actions`，
  调用方只补**位置类**（行 `memory-row__actions`、弹窗 `dialog-actions`）——两处要的落点不同，
  组件里不写死 `justify-content`。
- **行内三枚动作顺带从 small 文字按钮变成徽章**（`memory-badge`，与记忆详情底部同一套语言）：
  2026-10-17 用户对记忆行提的「操作按钮默认显示 badge」在这里同样成立，而两处共用一份实现之后
  也不可能一处徽章一处文字按钮。
- **候选的 `stale` 在详情里说「来源已变」**（黄框 + 次行），不打印记忆那档的「已过期」：候选被
  批准时后端会把 verification 重置成 verified，两个 `stale` 是两件事。
- 详情挂在 `candidates` 的 pending 列表上按 id 现取：候选被处理后从 `memory_list` 里消失，
  computed 变 `null`，弹窗自己关掉（不必同步一个开关状态）。

**文案：把「先进入待确认、点保存才成为记忆」的旧说法清干净**

- 页头副标题、待确认档 ⓘ 第一句、待确认空态、「提取任务」ⓘ 四处按新事实改写：自动提取的长期
  约定**直接存成记忆**（未验证），只有与既有记忆冲突 / 推翻旧候选的才进「待确认」由用户裁决；
  `memory_propose`（会话里主动提）那条路仍进待确认。
- 「提取任务」ⓘ 里前置的「把…东西**提成候选**」一并改成「**提出来**」：不改的话，同一句后半
  与它自相矛盾（原话仍暗示提取结果先成候选）。
- 开关名「使用已批准的记忆」**不动**：它同时管注入与自动提取，开关语义没变；改的只是它管什么
  的说明。

**判据**：`ui/test/memoryPanel.test.js` 42/42（新增 4 条：正文那半可点 / 详情挂在 candidates 上 /
详情摆出完整正文与来源凭据 / 新文案逐字在且旧说法一条不留；另 7 条既有判据跟着改）。**12 个
变异反向验全部转红**（删回车入口、删 `role`、去掉冒泡闸门、空格改裸 handler、详情删共用动作排、
详情加回 clamp、页头退回旧文案、`@open` 接错、详情改从 `allMemories` 取、`stale` 直接打印
`verificationText`、删 `append-to-body`、详情挂载删 `v-if`）。`build:ui` 成功、
`check:duplicates` 与基线同为 4 处。

**未验**：弹窗在真浏览器里的排版（560px 内的换行、`__main` 在 dialog 里的单列降级、徽章行高）
——e2e 通道断言不了面板渲染（见清单 G23 的边界），本轮只有 `build:ui` 与静态判据。

**记在账上的**：`plugins/memory/test/probe/drive-real-model.mjs` 的 C6 夹具（连跑两次驱动器验
「改口被标取代」）建立在「第一次提取留下一条 pending 候选」上，而自动落地之后它不再留下——
复跑前要先给夹具一个「预置 pending 候选」的钩子（或改走 `memory_propose`）。不在门禁内。

## 逐屏决定（2026-10-25 从 ui/AGENTS.md 整段搬来，为那份的字节预算让路）

那份文件已越过运行时的 65536 字节指令上限（实测 66089 字节，尾部被截掉 870 字节——**写在那里却从没被读到过的规则比没写更坏**），而本节的内容在本文「设计系统」一节里本来就有更全的一份（含日期与被否掉的方案）。⇒ 这一段在 `ui/AGENTS.md` 里只留**规则**，**经过留在本文**。原文如下，一字未改。

### 逐屏决定

- **技能页是「已安装 / 社区资源」两张卡**。手动安装是社区来源的入口，与本地已装状态无关。
  **设计稿那张「技能状态」卡不画**：它写着「活动视图 已同步」，没有任何后端字段支撑，
  硬写就是编数据。
  - `installSkill` 走 `withProgress`（长任务）**不经过 `withLoading`**，所以安装按钮上
    挂 `isLoading('installSkill')` 永远为 false——那是一个永远不转的假 loading。在途
    状态由进度浮层表达，按钮只做 `globalBusy || 输入为空` 的禁用。
  - 单页专用样式留在 scoped。输入行复用全局 `.install-row`，卡内标题与说明各合成一条基线
    规则（`.community-title` / `.community-meta`）；技能社区与手动安装之间只画一道 2px
    刻蚀线（`.community-install::before`），两侧边框显式清零。
- **设置页左栏是工作台、后台常驻、环境回退与诊断，右栏是外观、任务通知**。卡内保留
  三个 `.page-list-row`：**环境回退点 / 深入排查 / 启动诊断**；数据迁移从侧栏进入。
  - `SnapshotCard` 与 `BisectPanel` 渲染成 Vue Fragment（根节点就是那一行），套进共享卡
    就会出现卡中卡。判据查的是**形状**（整页只有一张卡 / 没有两列栅格）而不是某个类名。
  - **收起的只有明细，状态不许藏**：状态说明（`headline(view)` / `bisectHeadline(view)`）
    挂在 `.page-list-meta` 常驻，`drifted` 与 `conclusion` 两条告警**不进闸门**——排查
    跑几分钟时用户唯一能看懂的进度恰恰不能被折叠。
  - `.page-list-meta` 故意不抄设计稿的 `nowrap` + 省略号：这里的次行是整句说明，截成
    一行就分不出「还没成功启动过」和「文档损坏」。
  - **收尾那条的分隔线收在 `.page-list > *:last-child` 上**而不是
    `.page-list-row:last-child`：Fragment 展开后明细也是列表的直接子级，按行判会把展开
    内容当成收尾行，连带抹掉它上面那条线。
  - 「启动诊断」是补上的入口，直接打开诊断窗口，不是有内容的展开层。
- **内核版本页维持「一张卡竖排」**，不改设计稿的两列栅格——这一屏有三处行为依赖当前竖排
  且已调稳（发布列表的 z-index 层叠、淡出带移除后的裁切、为 120px 限高量过的磁盘区
  6+9px 间距）。判据钉的是「整页只有一张卡、没有两列栅格」。
- **内核版本页两栏之间那道竖线不贯通**：右栏是带内部滚动的长列表，自己的
  `.release-list-box` 边框与底色不缺边界，线通到卡片底边这一屏就被读成两个并排窗格。
  - 线的高度只跟着左栏「已安装」那几行走：左栏非空时 `align-self: start` 按内容高度
    收，线挂在 `.installed-list` 右缘外侧、落进 8px 栅格缝（`left: 100%` +
    `margin-left: 3px`，偏左是因为右边紧挨着 `.release-list-box` 自己的 1px 边框）。
  - 竖线沿用刻蚀线标准（宽 2px、`--divider-strong`、六边形收细），仍用 `::after`。
    **不许用 `border-left`**：边框属于元素自身，元素被拉多高线就有多高。
  - **空态是唯一例外**：`.installed-list` 必须继续伸展，否则
    `.installed-list:has(> .el-empty) { justify-content: center }` 没有可分配空间、变成
    空操作，选择器带 `:not(:has(> .installed-list > .el-empty))`。
  - 判据在 `designAlignment.test.js`（2 条），其中一条**不拿调用点传进去的选择器当
    证据**；它从文件里把真正设了 `align-self: start` 的那条规则捞出来，再看那条规则的
    选择器有没有空态豁免。
- **顶部工作条（`.workspace-bar`）已整条删除**：页签按 `kernel_family` 去重，而后端只定义了
  `dsh` 一个族，一个族只会画出一个页签且必然是当前那个。删掉的是「界面上的切实例」，不是
  运行状态与实例清单——后端 `set_default_instance` 仍在，运行状态胶囊照旧读同一个
  `store.view.kernel`，插件页照旧按实例列出 `族名 · 实例 id`。
  - **`instance.js` 的 `setDefaultInstance` 不许跟着删**：它是**没有调用方的 API**，不是
    「永远走不到的代码」。`ui/test/kernelSwitch.test.js` 钉着三条真实语义（读实例去重、
    陈旧列表不能把一次切换的结果冲掉、写入失败必须释放 busy），删函数就要连这套语义
    一起删。**前者留着、后者才清**；判据钉住的不变量是「模板侧零调用 + 函数与
    `selectionRevision` 仍在」。真接入第二个内核族时切换器要重新做一遍。
- **概览「当前内核」卡：主操作上移，实例 caption 删除**。卡头只有 `<h2 class="kernel-title">`
  与 `<div class="kernel-header-actions">` 两栏；占着右侧的 caption「DSH / default-dev」删除，
  `kernelCaption` 连同概览页对实例注册表的整条引用（`instanceStore` / `familyLabel` /
  `loadInstances`）一并移除——**`App.vue` 仍在启动时加载实例列表，插件页不受影响**。
  教训留在原地：`KernelStatus` 结构体里既没有 `family` 也没有实例 id 字段，
  `undefined || …` 会静默落到下一个兜底分支，`check-invariants` 的 `[ipc-fields]` 项抓的
  就是这条。
- **内嵌插件行并进社区插件列表，是那个列表的第一行**：现在就是一条普通的
  `.entity-row.entity-row--single`，名称 / 来源 chip / 状态文字全部复用社区插件那一套。
  - **语义差别只体现在右侧动作**：没有安装 / 更新 / 卸载，保留启停开关，并提供 ChatGPT
    登录、取消登录、刷新模型列表与退出登录；启停意图与账号登录分开，停用插件不退出账号。
    「不进社区中央库」由「没有那些按钮」表达，来源 chip 用 `Cpu` 图标 + 「内嵌」二字，与
    npm / GitHub 同一枚 `.origin-chip`。
  - **那套 `.builtin-*` 样式必须删干净**：元素并进来之后它们就成了没有作用方的声明。
  - **空态判据要一起改**（`currentKernelListEmpty`）：只看社区插件的话，列表里明明列
    着内嵌那一行、下面却写着「当前内核尚未接入任何插件」。`is-empty` 与 `el-empty` 必须
    共用这一份。
  - 判据按**顺序**判（列表开标签 → 内置行 → 社区插件 v-for），只看「内置行存在」的
    话把它搬回列表外照样会绿。
  - **内置插件不止一个了：那几行是 `v-for`，文案与能力都来自后端**（`builtin_status`
    一次返回全部）。显示名读 `builtin.displayName`、ⓘ 读 `builtin.note`、要不要画账户卡
    读 `builtin.hasAccountCard`，**三者全部由后端 `BuiltinSpec` 下发**
    （`src-tauri/src/plugins/builtin/builtins.rs`）。前端不留一个字面量。
    - **「按 id 取」不许写成「取第一项」**：概览页与套餐窗口的 Codex 授权入口只看
      `openaiBuiltin`，拿第一项会在某个内置插件排到前面时静默返回别人的状态，判据在
      `ui/test/builtinPlugins.test.js`（它把 ponytail 摆在 openai 前面）。loading key
      按插件分（`builtinToggle:<id>`）：共用一个 key 时拨一个会让另一个的开关也转起来。
  - **内置插件的来源 chip 是放大一档的紫色，此后每个内置插件都用这一枚**：
    `.origin-chip-builtin` 叠在 `.origin-chip` 之上，13px（基线 12px）+ 图标显式 15px +
    `--builtin` 文字 / `--builtin-fill` 底 / 同色描边；npm 与 GitHub 那两枚继续走中性灰。
    - **挂在那枚 chip 上，而不是挂在某个插件 id 上**：chip 在 `v-for` 里，后端登记表加
      一项就自动带上紫色；改成一个按 id 判断的分支时，下一个内置插件会静默退回中性灰。
    - **色值走 `--builtin` / `--builtin-fill` 且两套主题各一份**：浅色 `#7c3aed`（白卡
      5.70:1）、暗色 `#a78bfa`（`#142239` 上 5.86:1），13px / 600 粗体按正文 4.5:1 算。
      只在一套里定义的话，另一套下 `var(--builtin)` 整条失效且不报错。
    - 判据钉「修饰类全模板只出现一次」、颜色读 token、字号与图标字号的关系、两个 token
      两套主题都定义。
- **业务能力不因改版增减**：页签分组（工作台 / 资源 / 系统）、侧栏收起，都是把原有
  信息重新摆位，不是新功能。
- **末行两卡吃满剩余纵向高度，块内文字换行**。高度：`overview-grid` 写
  `grid-template-rows: auto auto minmax(min-content, 1fr)` + `flex: 1`，祖先 `.panel`
  给 `min-height: 100%`（百分比高度要落在确定高度的祖先上）。用 `min-content` 起底而不是
  `0`：后者会在空间不够时把卡压扁、内容溢出自己的格子。换行：`.diag-row__label` 从
  `nowrap` + 省略号改成 `overflow-wrap: anywhere`，`anywhere` 不能省，否则一整段没有
  空格的错误文本会把整列撑出去、换行变成横向溢出。
- **行尾动作是带边框、带 icon 的徽章**（`.diag-row__cta`），不是纯文字箭头。语气跟着
  同行读数的严重程度走（`--danger-*` / `--warning-*`，为此两套主题各加了 4 个 token），
  悬停加深自身色而不是换成强调蓝——accent 蓝在这套里表示「主操作」。
  **展开箭头与动作按钮不许共用类名**（`.diag-row__caret`）：一个类名表示两件事时，改其中
  一种会顺手改到另一种。判据显式钉住「`.diag-row__arrow` 不许再出现」。
- **系统健康 / 最近操作 / 需要关注三张卡的卡头跟着概览走**：块标题读
  `--fs-block-title`（17px）而不是写死，`padding-bottom` 保持 9px 不动（宁可多这 1px
  也不为总高对齐去单给控制塔改统一值）。**`--bare` 用 `:not()` 正着写**：线从 border
  换成伪元素之后，只清 `border-bottom: 0` 那条照样会画出一条线。
- 概览主栅格 `1.25fr / 0.75fr`；**末行是「最近操作（左） | 需要关注（右）」左右
  并排**（对调的理由是「没有待处理项」这个最常见状态下「需要关注」整张卡不渲染）。
  套餐用量**固定三列**（1040 宽下 `auto-fill minmax(170px)` 会排成四列）；额度行是
  「名称 22px | 条 1fr | 百分比 auto」一行栅格、条高 6px、百分比在条外；余额总额
  14px 粗体、赠金充值降到第二行 10px。判据 `designAlignment.test.js`。

## 用量分析面板：概览与两扇独立窗口的入口收拢（2026-10-28）

用户拍板：侧栏「工作台」组新增「用量分析」（id `usage`），把原本分在三处的用量信息并成
一屏；概览页只保留「今日用量」一个用量指标。面板上方是原概览的「套餐用量」卡（整卡迁入
`ui/src/subscription/SubscriptionUsageCard.vue`，删掉「查看详情」按钮），下方是原「模型
用量」独立窗口的内容（`UsageWindow.vue` 增加 `inline` prop，页面内渲染）。

- **为什么是搬不是重画**：两块的视觉与交互此前已调稳——套餐卡三列额度行、模型用量的
  摘要卡 / 热力图 / 堆叠趋势 / 环形图——本轮只改容器与入口；模型与套餐的数据结构、统计
  口径与 Rust 侧一字未动。
- **概览页因此只剩一个用量读数**：「当前内核」卡三格指标里的「今日用量」。模型用量按钮
  删除、套餐用量卡整张搬走，概览栅格从三行收成两行（`auto minmax(min-content, 1fr)`）。
- **窗口入口移除 ≠ 能力删除**：概览「模型用量」按钮与套餐卡「查看详情」是本轮删掉的**界面
  入口**；`UsageWindow.vue` / `SubscriptionWindow.vue`、`open_usage_window` /
  `open_subscription_window` 命令与 `usage-viewer.json` / `subscription-viewer.json` 两份
  capability 全部保留，`?usage=1` / `?subscription=1` 的窗口路由照旧挂载。先例是
  「刷新工作台」（`forceReloadHarnessWindow`）与 `setDefaultInstance`：没有调用方的 API
  不当死代码清，入口隐藏与文件删除是两件事。
- **内嵌模式的边界**：`ViewerShell` 的 `inline` 只退掉窗口的四件事——自绘标题栏、定高、
  圆角、窗口底色；滚动交回主面板 `main` 那条既有规则。`UsageWindow` 内嵌时不再自己
  toggle `pulse-active`：脉冲由主壳 `App.vue` 统一接线，两个订阅方各 toggle 同一个 body
  类，先卸载的一方会把另一方的可见状态留成脏值。窗口模式行为不变。
- **没做的**：不给新面板另起一套卡片皮肤；不删两扇窗口的测试与 capability；不把「外部
  来源」勾选搬出模型用量区域；不改套餐 / 模型用量的展示口径。

## 「模型用量」卡头图标：品牌鲸鱼 → 语义柱状图（2026-10-28）

用户截图圈住「用量分析」页里「模型用量」卡头那枚小黑鱼：「不应该是小黑鱼，修改一下 icon」。
它此前读 `ui/public/whale-icon.png`（与侧栏品牌标、日志 / 套餐两扇副窗同一个文件）：鲸鱼是
应用自身的品牌标，放在区块标题上既不表意，深色卡面上黑鲸压 `#142239` 又几乎看不见。

改成与上方「套餐用量」卡头同一种形状：全局原语 `.card-title-icon`（accent 实心方块 + 白色
图形）里放 EP 的 `Histogram`。**为什么不是 `TrendCharts`**：按 14px 在方块里实测，EP 的
`TrendCharts` / `DataLine` 缩到这个尺寸只剩「一个带斜线的方框」，读成图片图标；三根柱子在
14px 下轮廓仍然清楚，也和卡里的堆叠柱状趋势对得上。侧栏「用量分析」那枚仍是 `TrendCharts`
（页面级入口，不与卡头同步）。日志 / 套餐两扇副窗的卡头仍是品牌鲸鱼，本轮不动。
判据在 `ui/test/usageAnalysisPanel.test.js`。

## 最近操作的类型页签（2026-10-28）

用户截图指认「启动 · 预检 · 恢复 · 排查」那一组词：看起来是页签，却点不动。此前实现是
静态目录（高亮最近一条所属的类型），代码注释还专门写了「不能做成真页签」的理由——这一段
只渲染最近一条记录，做成能点的会让人以为能切出历史。

用户提出后改为真页签：每类各取 `recentRuns` 里最近一条，点击切换、不发请求；默认仍停在
全类型最近一条所属的那一类，所以不动它时看到的内容与改前一致。某类没有记录时页签压暗、
正文说明「还没有这一类记录」，仍可切回。没有做成「每类历史列表」：卡片高度只容一行，
历史在诊断详情里；「清除记录」仍是清空全部。

## 两扇独立窗口的去留与套餐卡补齐（2026-10-28）

用户问「这两个入口是什么」，要求功能截图后再判断是否并进「用量分析」页。用无头
浏览器 + mock 数据把两扇窗真跑了一遍（760×800）：

- **模型用量窗**（`?usage=1`）：内容与「用量分析」页内嵌的**同一个** `UsageWindow.vue`
  逐节相同（时间范围、外部工具勾选、5 个概览瓦片、热力图、趋势、环形图、模型列表、
  外部 Agent 工具、底部统计条），只多出窗口标题栏 / 独立滚动条这类 chrome。**已 100%
  覆盖，不集成。**
- **套餐用量窗**（`?subscription=1`）：provider 数据、错误横幅、Codex 许可开关、
  单家刷新能力都与卡重复（卡还多「已用 X%」、单 provider 刷新、失败分区隐藏确认）。
  窗口多出的有效信息只有「这份余额属于哪个实例 / profile」与「从未查询成功」两种；
  其余是纵向全宽排版、tier 全名、底部状态条与「本地 token 用量见模型用量窗口」跳转。

**决定：不恢复「查看详情」/「模型用量」入口，也不新增详情弹窗**（会维护第二套布局）。
两处有效信息并进 `SubscriptionUsageCard.vue`：卡头 ⓘ tooltip 末尾补查询实例 / profile
与「失败时保留上次成功数据」；分区年龄胶囊在无查询时间时显示「尚未查询」，悬停说明
从「查询于 null」改为「尚未查询成功，点击查询」。判据在
`ui/test/usageAnalysisPanel.test.js`。两扇窗的组件 / 命令 / capability 仍保留（用户
此前选择「入口隐藏 ≠ 能力删除」），后续若要删按「待移除」清单四块一起走。

## 记忆面板：「模型读过 0 次」拆成「被注入」与「模型读过」（2026-10-31）

用户截图指着一条通用偏好的「模型读过 0 次」问「为什么通用记忆模型读取次数为 0」。查下来
不是记忆没生效：那条在「最近注入」里被 08:53 / 08:55 两次注入过；`use_count` 只由
`memory_read`（模型主动按 id 读全文）增加，而这几条正文只有 63–866 字节、注入时全文已经在
上下文里，模型没有任何理由再读一遍——这个数对短记忆因此恒为 0。真正缺的是「被注入过几次」
那一半：schema 里 `recall_count` 从第一版就有，但全仓没有写入方（死字段），注入记录只进
进程内 20 条环形。

收口两条：

- **后端**：`/v1/memory/note` 在插件回报真的注入之后调 `store::note_recall`，给命中记录
  `recall_count` +1（记忆 / 经验 / 当前交接都数）。它走同一个读-改-写事务，但**不抬
  `generation`**——那是插件注入缓存的失效键，抬了会让运行侧把一次计数当成「库变了」重新
  注入、白耗会话级预算；也**不动 `revision` / `updated_at`**（内容版本与排序依据）。写失败
  只进 memory-bridge 日志，不影响响应。
- **面板**：列表行默认写「被注入 N 次」（保持一行），`use_count > 0` 时才补「模型读过 N 次」；
  详情与交接详情两个都写；悬停共用 `memory.js::MEMORY_COUNT_TIP` 说明两个数各是什么。导出
  Markdown 同步写「被注入」「模型读过」两行。

**为什么不是把注入算进 `use_count`**：那会让「模型读过」同时表示两件事；「模型主动读全文」
仍是另一条可观察的路径（工具调用），两个数分开、各自命名，用户才能拿面板上的话解释「它为什么
照做了一条约定」与「为什么没有主动读」。**也不把计数写进注入缓存键**：两个计数的写入不能改变
`generation`，否则每注入一次就让运行侧重新注入一次，等于把注入预算花在记账上。

判据：`memoryPanel.test.js` 的「两个计数各管一件事」一组；Rust 侧
`injection_counting_is_separate_and_keeps_the_generation_still`、
`injection_counting_writes_nothing_when_no_id_matches`、
`injection_counting_covers_experiences_and_handoffs`，桥端到端
`memory_routes_work_over_the_bridge`。

**历史次数不回填**：注入记录过去只进进程内环形、不落盘，磁盘上没有任何能重算的依据，所以
升级后计数从 0 开始累计；「最近注入」卡片仍是进程内、重启即空。

## 记忆效果页落在记忆页内，作为第二个页签（2026-10-31）

用户要「记忆到底有没有用」变成看得见的读数。落点比较过三种：**A 记忆页新增「效果」页签**、B 在记忆页底部嵌一张效果卡、C 侧栏或「用量分析」新增独立入口。**决定：A。**

- **为什么不是 B**：效果内容是六块（读数条 / 效果漏斗 / 趋势 / 榜单 / 成本 / 数据健康），嵌在记忆库页面底部会把「最近注入」卡挤到第二屏；记忆库页自身已是双栏栅格 + 列表内部滚动，底下再挂一长段会变成两层滚动条。
- **为什么不是 C**：侧栏在 157px 宽度下已经有七项，再加一项会压缩其余项；而且「开关在记忆页、效果在别处」是割裂的——效果页页头要保留同一个「使用已批准的记忆」开关（它同时管注入与自动提取），用户才能把读数和刚改的开关对上。
- **页签形态**：页头下方两枚药丸页签「记忆库」「效果」，`role=tab` + `aria-selected`，默认停在「记忆库」；切到「效果」才发 `memory_effect_report`（首次或时间窗变化才重拉，手动刷新走 force），不给记忆库页增加启动开销。时间窗只给 7 / 30 / 90 天三档。
- **效果页的口径**：P0 只做**事实层**——注入快照数、注入字节与 token 估算、携带记忆的请求率、主动读取/检索、漏斗、成本与数据健康；不做分组对比、未注入原因分解、会话下钻与实验开关（P1/P2）。没有数据的块写「P1 未接入」或空态文案，不塞占位假数据。
- **与设计稿的差异**：设计稿 §5 的八卡 + 下钻是目标形态；P0 落地为六块卡（五格 KPI 收在同一张读数卡里），下钻与分组对比留给 P1——两者如有出入以实现为准。
- **没做的**：不加侧栏入口、不做单一「记忆有效性总分」、不把「被注入」当「被采用」；「最近注入」卡保持原样（它仍只看进程内环形，重启即空，与效果页的历史窗口是两种口径）。

实现：后端 `src-tauri/src/memory/effect/{mod,scan,report}.rs` 的 `memory_effect_report` 命令（只读扫描会话日志 + memory.json/jobs.json，90 天保留、增量 offset、状态文件可重建）；前端 `ui/src/memory/MemoryEffectPanel.vue` 与 `ui/src/memory/effect.js`，入口页签在 `MemoryPanel.vue`。判据见 `ui/test/memoryEffect.test.js`、`ui/test/memoryPanel.test.js` 与 Rust 的 `memory::effect` 单测；进度表在 memory-progress.md 的 P0 效果页一条。

## 记忆效果页 P1：账本口径的原因分解、只做 L2 的对比、无正文下钻（2026-10-31）

P1 在同一页签内补四件事；决定与理由：

- **未注入原因分解用注入账本口径，不用扫描推断**：扫描只能回答「这一回合有没有注入」，答不了「为什么没注入」——开关关着、零命中、预算 / 本回合不取、桥失败、内容没变是五种不同的事，混成一个「未注入」数会把人引向错误的排查方向。所以 `/v1/memory/note` 新增可选参数（kind=inject|skip、reason、turn、step、bytes、generation、workspace_id、plugin_version 等，旧插件缺省兼容），每个会话只在 skip 原因桶变化时上报一次、不逐回合打 HTTP；账本落在 `<实例记忆目录>/effect/ledger/notes-YYYY-MM.jsonl`（0600 追加、按月分文件、90 天裁剪，写入失败只记壳日志、不影响 note 响应）。账本缺失或窗口内无事件时显示「账本未启用」并退回扫描口径，不显示假的 0。
- **分组对比只做 L2，且样本不足只给计数**：同一会话内「请求上下文含有效记忆快照」与「不含」的请求做配对差（工具调用 / 请求、输入 token / 请求、输出 token / 请求、缓存命中率、工具错误率），95% CI 用配对差均值 ± 1.96×SE；同时含两组的会话数为 0 或低于下限时 `available=false`，只给计数与所需会话数。固定文案写「L2 关联，不是因果」；CI 跨 0 只写「未见差异」，不画显著性颜色、不写因果词——设计 §3.4 的「容易骗人的度量」明确禁止把关联当因果。
- **下钻只给结构节点，无正文**：新增 `memory_effect_session(sessionId)` 命令与弹窗，时间线只有注入 / 请求 / 压缩节点（时间、turn/step、暴露标记、ids 数、字节、token、工具调用与错误数），**不返回任何正文、标题或路径**；节点上限 300，超限在 note 里说清而不是假装完整。理由：用户要核对的是「这次注入发生了什么」，正文既不需要也不该进面板聚合链路。
- **提取质量与引用代理来自现有权威数据**：提取来源记忆数 / 停用 / 从未曝光 / 被读取、按提取器版本分组的任务数与失败数都从记忆库与任务表现算，不新增埋点；榜单的「引用（弱代理）」是标题子串匹配的启发式（只在内存里匹配，不把标题写进状态文件），UI 与 tooltip 明确标注是弱代理，不当成采用证据。
- **没做的（P2）**：随机影子实验与实验开关、跨版本因果对比；也不做单一「记忆有效性总分」。与设计稿的差异以实现为准。

实现：后端 `src-tauri/src/memory/effect/ledger.rs`（账本）与 `session.rs`（下钻）连同 `mod.rs` / `scan.rs` / `report.rs` 的 P1 扩展；前端 `ui/src/memory/MemoryEffectPanel.vue` / `MemoryEffectSessionModal.vue` / `effect.js`。判据与进度见 memory-progress.md 的 P1 效果页一条。

## 记忆效果页 P2：可选的会话级随机对照实验（2026-10-31）

P2 只剩一件事——给「记忆到底有没有用」一个 L3 证据来源。决定与理由：

- **放在效果页，作为独立一张「对照实验（L3）」卡**：实验的产出（随机分组后的会话级指标差）与效果页其它读数同屏才能对照，但**不与 L2 分组对比混算**——L2 是同会话内暴露 / 未暴露的伴随差异，L3 是随机分组的因果证据，两级证据混在一张表里会让人把 L2 当成因果。所以单独成卡，固定 caveat 写「L3 随机对照（会话级）；与上方 L2 对比不是同一证据级别」。
- **默认关闭 + 开始前二次确认**：实验会改变真实对话行为（对照组会话不带记忆），不能默认开、也不能点一下就开；ElMessageBox 二次确认写清比例、停止入口与影响范围（约 10% 的新会话在实验期间不会带记忆，可在效果页随时停止）。
- **默认 10% 进对照组，比例 5%–30% 可调**：对照组太少出不了数、太多会明显影响使用；10% 是「够出样本又不至于让用户频繁撞上」的折中。比例存万分比（1000 = 10%），分组走 `fnv1a(session + ":" + seed) % 10000`，同一会话同一实验内稳定。
- **只影响实验开始后新建的会话**：已存在的会话在 agent/created 时已经取过上下文，中途改变行为会破坏正在进行的工作；所以实验开始后新会话才领分组，停止后对照组会话最迟在下一次重新领取分组（每 3 个 turn 或 30 秒）时恢复正常注入。
- **对照组连自动提取一起停**：只关注入的话对照组仍会写入记忆库，两组的「记忆库存变化」就不一致了；对照组会话不注入、不排提取任务，避免交叉污染。
- **时长 1–14 天、默认 7 天、可随时停止**：到期按 `ends_at_ms` 判（不需要后台定时器）；用户点「停止实验」立即写 `stopped_at_ms` 生效，已产生的账本事件保留、不做数据删除。
- **安全 / 隐私边界**：开始与停止只有面板命令面（模型工具不可达）；桥上的分组接口只读、不写任何数据、不返回记忆内容、这次调用不算一次注入；实验配置 0600 私密原子写；账本与实验文件都不落正文 / 标题。默认关闭意味着「不配置就完全按原行为跑」。

实现：`src-tauri/src/memory/effect/experiment.rs`（配置 / 分组纯函数 / 桥响应）与 `memory_effect_report` 的 experiment 段、插件侧 agent/created 领分组与对照组分支；前端「对照实验（L3）」卡与 `memory_effect_experiment_start` / `memory_effect_experiment_stop`。判据与进度见 memory-progress.md 的 P2 效果页一条。

## 插件页精简：移除远端「插件中心」（2026-10-31）

用户决定移除插件中心相关功能，保留手动安装与内置插件。设计稿见
[plugin-page-simplification-design.md](../features/extensions/plugin-page-simplification-design.md)。

- **删除的是整个远端目录能力**：dshfind.com 主源 / 参考市场回退、gzip 大响应、6 小时缓存、
  壳侧搜索 / 分类 / 排序 / 分页、`plugin_catalog_search` 命令与 ACL、`plugins/catalog.rs`
  与 `center.rs` 的目录段，以及相关测试与文档。旧缓存 `plugins-catalog.json` 在启动对账时
  按精确路径清理一次。
- **安装入口收敛为手动安装**：插件页「插件库」tab 顶部固定一个「安装插件」区块（输入 +
  「预检并安装 / 安装」按钮 + 说明，回车与按钮同一函数）；不再有目录、搜索、刷新数据与
  「显示更多」，原右栏两栏布局随之删除。
- **保留面一个不少**：手动安装、预检两阶段、更新检查、同步、模式切换、卸载、隔离恢复、
  多实例状态 chip、内置插件的账户卡 / 上游更新 / 启停全部不动；「当前内核」tab 模板零改动。
- **命名**：「已安装」tab 改名「插件库」（该 tab 现在同时承担安装入口与中央库视图），
  清单块头仍叫「已安装 · N 个插件」；「插件中心」不再作为任何界面名出现。
- **取舍**：应用内不再提供社区目录浏览与一键安装；安装仍支持任何 npm 包 / Git 仓库，
  发现路径交回浏览器。代价是用户要自己知道包名或地址，这是本次明确接受的产品取舍。

## 插件页二次调整：移除「插件库」页签，安装入口下移到内置插件之后（2026-10-31）

用户看过第一版实现（安装区块 + 插件库清单同在一个页签）后要求：移除插件库页签，
让「安装插件」直接显示在内置插件下方，并优化排版。定案与理由：

- **单视图、无页签**：插件库页签承载的「手动安装 + 全实例状态 chip」被拆开——安装是动作、
  不是视图，放在内置插件之后更顺；每实例 chip 属于对比信息，用户当前只在默认实例上操作，
  不再单独占一屏。页签去掉后同步 / 检查更新移到工具条右侧，与「当前内核」同一行。
- **内置插件独立成块**：原先「并进社区列表当第一行」是为了让它看起来不像另一个东西；
  现在它自己有一块，并且安装入口紧跟其后，反而需要块头把「这些不可卸载」说清楚。行外壳
  仍复用 `.entity-row--single`，只是外层换成自己的 `.entity-list`。
- **顺序固定为：工具条 → 内置插件 → 安装插件 → 已安装插件**。安装区块在中间是有意的：
  往上紧接「随应用交付」的内置插件（不需要安装），往下才是用户自己装的插件（可管理）。
- **删除干净**：`el-tabs` / `el-tab-pane` 与 Element Plus tabs 注册、`installedTab`、每实例
  chip 计算与 `.installed-tabs*` / `.instance-state-chip*` 样式一起删，避免留下没有作用方的
  声明；`communityListEmpty` 只判断社区行，内置行不参与空态。
- 代价：不再有「所有实例装载状态」的对比视图；后端 `PluginRow.instances` 仍在，需要时可
  按本决定重新加回，而不是恢复整个页签。


## 概览页插入「今日模型用量」卡，今日用量移出当前内核卡（2026-10-31）

用户要求：概览页中间插入今日模型用量 Top 3，「今日用量」数据从「当前内核」卡移出来。
定案与理由：

- **落位：整宽第二行（`order: 3; grid-column: 1 / -1`）**。第一行仍是「当前内核 | 系统健康」，
  末行仍是「最近操作 | 需要关注」（缺席时最近操作整宽）；首次运行引导仍占首行。栅格行模板
  因此改为 `auto auto minmax(min-content, 1fr)`，末行继续吃剩余高度。
- **不并进「当前内核」**：今日用量回答的是「这个实例今天用了什么模型」，属于使用量而不是
  内核状态；放在内核卡里既挤掉 Node.js / 数据目录 的宽度，也让一块读数承担两种语义。
  内核卡收成 Node.js / 数据目录两格，中间那条竖线仍按原比例收细。
- **卡面：卡头右侧今日总 tokens + 请求次数，正文 Top 3 每行「序号 / 模型名 / 占比条 /
  tokens / 占比」**。占比以今日总 tokens 为分母（不是以 Top 1 为分母），排名与条长读的是
  同一件事；无记录时给一句空态，不隐藏卡片（用户要知道「今天确实没有」而不是以为漏加载）。
- **数据单一来源**：后端 `UsageView` 新增 `today_models`，与 `today_tokens` / `today_requests`
  由**同一次**本地日历日精确匹配算出（按 tokens 降序）；前端 `usage.js::todayTopModels` 只做
  截断与兜底。**不在前端按 `days` 自己找「今天」**——那正是 2026-09-30 两个面板显示两个数的
  成因，`check-invariants` ⑦之五同步钉住概览的 `todayUsage` / `todayTopModels` 接线。

## 插件页三次调整：安装面板加高、动作下沉、描述收进 tooltip（2026-10-31）

用户看过第二版实现后的三点要求：手动安装区高度太低；「同步内核 / 检查更新」针对的是
手动安装的插件，应整合进安装区；描述性文本能做成 tooltip 的都收起来。

- **安装区改成独立面板**：`--overlay-faint` 底 + `--border` 描边 + 10px 圆角 +
  `14px 16px 12px` 内边距，标题行 + 输入行 + 底部动作行三段；它不再是一句「块头 + 输入 +
  提示」就结束的窄条，视觉上成为本页除列表外最重的一块。社区空态从带插图的 `el-empty`
  收成一行提示，把高度让给安装区。
- **动作跟着对象走**：删掉顶部工具条（它让「同步 / 更新」看起来像在管内置插件）；两枚
  按钮收进安装面板标题行右侧（用户看过后再次指认位置），与「安装插件」同排、靠右。
  当前内核身份从工具条挪进「内置插件」块头的 ⓘ。
- **描述性文字 tooltip 化**：预检开关旁的长句、内置插件交付说明、安装格式 / 预检 /
  生效时机、同步与更新的作用范围，全部收进 ⓘ；行内状态只显示短词（「已接线」），
  完整状态说明仍在行内 tooltip。判据反向确认 `.plugin-install-hint` /
  `.plugin-install-desc` / `.plugin-install-scope` / `.precheck-toggle-label` 等常驻节点已删除。
- **代价**：不悬停时看不到安装格式与预检细节；输入框 placeholder 仍给出格式示例，按钮
  文案仍表达「预检并安装 / 安装」，关键信息不至于完全不可见。

## 记忆页页头说明收进 tooltip（2026-11-01）

用户要求顶部说明性文本都用 tooltip 收纳。原先页头有两段 `.page-desc`（记忆库一段长文、
效果页一段统计口径），开关旁还有一句可见的「已生效 / 处理中」说明；现在：

- 删除两段 `.page-desc`，按页签切换的文案移入标题「记忆」旁的 ⓘ（`pageIntro` computed），
  界面只留标题与图标。
- 开关旁那句生效说明移入开关旁的 ⓘ：`content` 用 `switchEffectDetail(status)`
  （含「同时管注入与自动提取」的范围），图标的可访问名用 `switchEffectText(status)`
  （带当前状态），不再常驻文字。
- 开关块不再 `flex: 1 0 100%` 独占第二行——说明搬走后它按自然宽度跟在标题右侧，
  窄窗仍由页头 `flex-wrap` 折行。
- 判据：记忆页头无 `.page-desc`、标题 ⓘ 接 `pageIntro`、开关 ⓘ 接
  `switchEffectDetail(status)` 且 `aria-label` 用 `switchEffectText(status)`；
  效果页签的文案仍在 `pageIntro` 里。

## 记忆效果页卡片布局：同行等高 + 账本健康两栏（2026-11-01）

用户要求「效果」页功能卡尽量对齐、减少纵向空间浪费。原先 `.effect-grid` 是
`align-items: start` 的两列栅格，同一行两张卡各按内容高，短卡在行尾留出一块空白；
账本健康 / 版本又是通栏单列（读数 → 版本表 → 最近事件 10 条），把整页撑到约 2100px。

- `.effect-grid` 改成 `align-items: stretch`：同一行的卡底边对齐，多出来的高度留在
  卡内底部，界面上的卡框是齐的。
- 账本健康 / 版本卡内部改成左右两栏：左栏「关联率 / 关联情况 / 注入·跳过 + 版本表」，
  右栏「最近事件（点行下钻）」。右栏变窄后事件行按半宽重排列宽（时间 64 / 会话 88 /
  回合 44 / 类型 34 / 原因 54 / 明细 1fr），明细与时间列 `nowrap`，避免「1 条 · 1.2 KB」
  被逐字折断。10 条事件时该卡从约 545px 降到约 200px。
- 记忆榜单行距收紧（`padding: 2px 0`、`line-height: 1.4`），8 条榜单少占约 50px。
- 结果（合成数据预览）：页面总高约从 2130px 降到 1830px；空态 / 未启用账本时各卡也在
  同一行对齐。判据见 `memoryEffect.test.js`「效果卡布局」。

## 亮色主题换成 Kami 纸墨体系（2026-11-01）

用户要求用 kami 技能优化亮色主题。kami 的文档模板与衬线排版是印刷交付物的语言，不适用于
748px 的桌面管理面板（技能自身也标注 web dynamic app UI 不在适用范围）；这里只取它的色彩
语言——暖羊皮纸、单一墨蓝 accent、带黄棕底的暖灰。暗色当时未动；随后换成方案 B，见下一节。

- **画布 / chrome / 卡片**：`--canvas` `#f5f4ed`（羊皮纸）、`--chrome` `#efece3`
  （侧栏压深）、`--surface` `#faf9f5`（象牙白卡片），`--surface-raised` `#fffdf7`。
  阶梯仍「底越高越亮」；卡片对画布只有 1.05:1，边缘靠描边 + 暖投影立。
- **文字 / 线**：`--text` `#141413`、`--text-secondary` `#45423a`、`--text-muted`
  `#6b6a64`；`--divider-strong` 从冷蓝灰 `#51627e` 换成暖墨 `#6f695d`（卡面
  5.17:1），`--border` `#e8e6dc`。主色 `--accent` 从亮蓝 `#2e76f6` 收到墨蓝
  `#1b365d`（Kami 的 accent 只占表面 ≤5%）。
- **状态色 / 紫标**：状态色降饱和到 success `#256b4d` / warning `#7d5417` /
  danger `#a6403a`（对卡面 5.8–6.3:1）；内置插件紫改暖紫 `#6b4a82`。
- **Element Plus**：浅色块的 `--el-color-primary` / 四个状态色、派生 `light-3..9` /
  `dark-2`、`--el-bg-*` / `--el-text-*` / `--el-border-*` / `--el-fill-*` 与暖投影
  一起重算；不补派生档，按钮 hover / tag 会退回库默认亮蓝与荧光绿。
- **用量热力图**：写死的三档亮蓝收成 `--heat-1/2/3`（浅色墨蓝 alpha，暗色保持原亮蓝
  阶梯），否则暖色页面上会是一排荧光蓝。
- **保留**：全站无衬线正文与现有字号 / 圆角（面板 11–24px 中文换衬线不读）；数据可视化
  的模型色板 `MODEL_COLORS` 仍按类别着色，不算主题灰阶。
- **判据**：`designAlignment.test.js` 的浅色分割线钉值从 `#51627e` 更新到 `#6f695d`
  （仍算对比度、仍「比描边重、比正文轻」）；新增 `--heat-*` 两套主题各一份；
  `npm run test:ui` 639 条全过。

## 暗色主题换成方案 B「夜航日志」（2026-11-01）

用户从三款暗色方向里选定 B：深墨绿画布、黄铜操作色、鼠尾草健康色、宋体标题、
账页式横线，不再用「不透明卡片 + 8px 圆角」组织页面。浅色主题保持 Kami 纸墨，
只改 `html.dark` 下的 token 与作用域覆写。

- **token**：`--canvas #0a100f` / `--window #070c0b` / `--chrome #0d1513` /
  `--surface #111a17` / `--surface-raised #16211d`；`--text #e9e4d7`、
  `--text-secondary #b8b4a7`、`--text-muted #85897d`；`--accent #d8a84e`
  （黄铜）、`--success #8fbfae`（鼠尾草）、`--danger #c96f5f`（锈红）；
  `--divider-strong #465a50`、`--border #2a3932`。暗色圆角收成 2px，浅色不变。
- **排版**：新增 `--font-serif`（Songti SC / STSong / Noto Serif / Georgia）与
  `--font-mono`，以及 `--fs-page-title: 31px` / `--fs-block-heading: 15px` /
  `--size-block-icon: 22px` 共享档位。两套主题页标题 31px / 600、块标题 15px 宋体、
  侧栏编号索引 11.5px；颜色与表面仍各取 palette；块标题图标收成描边方片（亮色墨蓝 / 暗色黄铜，只换 token），记忆页与「需要关注」原先没有图标的块标题也补齐（Collection / Promotion / MagicStick / Search / Filter / TrendCharts 等，逐块在模板里选，不靠 CSS 猜）。
  页头与侧栏加编号（模板 `data-page`、CSS 计数器），两套主题共用编号索引。
- **结构**：`html.dark .main-view .card` 改成透明账页段（方角、无投影、1px 外投影
  当分隔线），`.diag-card` 同待遇；插件 / 技能列表去掉外框与行圆角；版本页两栏之间
  的竖线、安装面板底色、设置页外观选项的大圆角都在暗色下撤掉。浅色规则原样保留。
- **控件**：EP primary 黄铜 + 深色文字，switch 选中改鼠尾草，输入框方角，
  按钮 / tag / 弹窗圆角 2px。
- **取舍**：宋体标题在 Windows 上会回退到 SimSun，字形与 macOS 不一致；暗色卡片
  不再有投影，层级完全靠线，低对比屏上边界会比原来弱。记忆页内容多时仍靠页面滚动，
  不为了塞进 748 删信息。
- **判据**：`designAlignment.test.js` 的页标题 / 页头副标题 / 侧栏字号改为按暗色 B
  取值，同时保留浅色基线断言；`cardChrome.test.js` 的卡标题元素集合补 `main-view`
  祖先，识别「暗色账页头用 1px 线」这一新规则。`npm run test:ui` 644 条全过，
  `check:invariants`、`build:ui`、`check:ui-bundle` 通过。

## 侧栏品牌名在 Windows 上单词内断行（2026-11-02）

用户 Windows 实机截图：品牌区断成「DeepSee / k / Harness」三行。

- **根因是宽度 + 字体度量，不是字符数**：157px 侧栏里 `.brand` 去掉 42px 鲸鱼与
  `gap: 10px` 后只剩 72px 文字块；`--font-serif` 在 Windows 落到 Georgia（Songti SC /
  STSong / Noto Serif CJK SC 都没有），600/15px 的「DeepSeek」实测约 80.7px（含
  0.04em 字距），放不下，而 `overflow-wrap: anywhere` 允许在**单词内部**断开。macOS
  的 Songti 字窄，同一段文字刚好吃得下，所以只在 Windows 复现。
- **修法是结构性的，不赌字体度量**：「Harness」拆成 `.brand__name__sub`
  （`display: block`、12px），主名降到 14px + 0.02em 字距、`white-space: nowrap`——
  两行由结构决定，任何平台都不会再把一个单词拆开；主名约 73px，仍落在品牌行右内边距里。
- **「Harness」比「DeepSeek」小一档**是用户同一条反馈里的第二项要求，字重仍 600。
- **判据**：`designAlignment.test.js` 的「品牌主名 + 副名」改为钉 `font-size 14px` /
  `white-space: nowrap` / 没有 `overflow-wrap` / 子元素 `display: block` 且字号小于
  主名，并在模板段钉住「DeepSeek <span>Harness</span>」的嵌套。原先「按空格折行」的
  2026-10-25 规则随 `anywhere` 一并撤回。

## Windows 字体发虚：壳窗口改不透明（2026-11-02）

用户 Windows 实机反馈「字体有点发虚」。根因不在字体栈，而在**窗口透明**。

- **证据（本机实测，DPI 100%）**：同一份页面放进不透明的 Edge 窗口截图，放大后品牌名
  能看到 ClearType 的彩色次像素边缘（像素 R-B 到 ±170 量级）；放进壳自己的窗口
  （`transparent: true`）同一位置是纯灰（R-B 只有 ±3 量级）。Chromium 在透明表面上会
  关闭 LCD 次像素抗锯齿，只剩灰度抗锯齿——这就是「发虚」。字体栈本身两边一样
  （Georgia / Microsoft YaHei / Consolas 都在）。
- **取舍（用户拍板：清晰优先）**：Windows 上四扇壳窗口改不透明、窗口四角变直角
  （Win10 没有系统圆角；Win11 是否补系统圆角本轮未实机验证）。macOS 保持透明 +
  CSS 圆角不变。
- **实现**：主窗由 `src-tauri/tauri.windows.conf.json` 覆盖 `transparent: false` 与
  不透明 `backgroundColor`——Tauri 的平台配置是 RFC 7396 语义、`app.windows` 数组
  **整体替换**，所以整条窗配置要抄全；判据逐项对比除透明相关字段外的其它字段，防漂。
  三扇副窗由 `shell::window::decorate_transparent` 的 `#[cfg(windows)]` 分支给同一套
  （不透明底色 + `transparent(false)`）。CSS 侧 `main.js` 按 `navigator.userAgent` 给
  `<body>` 打 `platform-windows`，theme.css 那条规则把 `--window-radius` 归零——
  `#app` / `#app::after` / `clip-path` 都读这个 token，一处归零全链生效。
- **已知代价**：首帧之前露出来的是窗口底色（写死暗色 `#0a100f`；主题真值在
  localStorage，Rust 建窗时读不到），浅色主题用户会看到一瞬深色。窗口阴影仍然关掉。
- **判据**：`windowChrome.test.js` 的建窗装饰一条改为两平台断言 + Windows 平台配置
  逐项对比 + `platform-windows` 归零与 `main.js` 接线。

## 设置页两栏配平：环境回退与诊断移回右栏（2026-11-02）

用户对着设置页截图说「调整一下UI布局」，并挂 `/humanizer 文本精简一下`。截图里左栏
（后台常驻 / Defender 排除项 / 环境回退与诊断）比右栏（任务通知）长一截，右栏底部空
出约一屏三分之一。

- **诱因是同一天加的 Defender 排除项卡**：它进左栏后左四右二，两栏高度差约 300px。
- **位置搬过两次，理由都还在**：
  - 2026-10-08 从右栏移到左栏——当时右栏是「任务通知 + 环境回退点 / 深入排查 /
    启动诊断」好几张卡，连不上内核事件流时通知卡展开环境说明与告警，右栏被拉到近两屏，
    用户来排查问题时先撞上的是通知告警。
  - 2026-11-02 移回右栏末尾——「环境回退与诊断」已合成一张卡、通知告警也只在
    `notificationsBlocked` 时出现，右栏反而成了短的那一边。现在左栏三张
    （工作台 / 后台常驻 / Defender 排除项）、右栏三张（外观 / 任务通知 /
    环境回退与诊断）。
- **同时压缩了 Defender 卡**（原来「状态」「要排除的目录」两个 15px `h3` 各占一行，
  路径与按钮左右分栏、路径被挤成两行，两条 11px 说明常驻在卡底）：
  - 状态并成标题下的一行正文；目录走三列栅格（64px 标签 / 可折行路径 / 不压缩的状态），
    路径显示过 `labels.js` 的 `tildePath`（home 折叠成 `~`，完整路径进 `title`）；
  - 「加入 / 移除排除项」收成独立的按钮行，不再与路径抢宽度；
  - 两条安全说明精简成各一句（「不再被 Defender 实时扫描」「只对 Microsoft Defender
    生效」两句必须保留在可见正文里，判据钉着）。
- **文案精简范围**：页面副标题、工作台 / 后台常驻 / Defender / 任务通知四个 ⓘ tooltip、
  通知卡的未连接 / 空态 / dev 自检说明。规则不变：描述性文字进 tooltip，状态相关的
  句子留在正文；「代价」类信息不许藏进悬停。
- **判据**：`designAlignment.test.js` 的 `settingsCardTitles()` 改为左三右三，并在
  caption 那条里同时钉两栏各三张卡；`avTrust.test.js` 的文案断言原样保留。
- **实测**（dev 壳 + 隔离 home，`main` 的 `scrollHeight`）：滚动高度 988 → 789px，
  两栏总高 586 / 694（差约 110px，原先右栏比左栏短约 300px）。页面仍需约 80px 滚动，
  环境回退点 / 深入排查展开时还会再长，属正常滚动。

## 设置页：环境回退与诊断按平台换栏（2026-11-05）

macOS 用户截图报「左侧没有 Defender 相关 UI 后，右侧 UI 没有自动布局到左侧，左侧大量空白」。

- **根因是上一版的配平只对 Windows 成立**：Defender 排除项卡只在 Windows 渲染，
  2026-11-02 把「环境回退与诊断」固定放右栏，是按「左三右三」配平的；macOS 上左栏
  只剩工作台 / 后台常驻两张，右栏三张，底部空出大半屏。
- **方案：那张卡按平台换栏**——Windows 仍留右栏末尾（左三右三，与 11-02 实测一致）；
  非 Windows 落左栏末尾补位。抽成 `shell/EnvironmentDiagnosticsCard.vue`，设置页两栏
  各挂一个互斥的条件挂载点。不做 masonry / CSS 多栏自动配平：卡片跨栏后视觉分组会
  随内容高度漂移（通知卡的 dev 说明一变，外观卡就可能换栏），行为不可预期。
- **平台判定用同步来源**：落栏用 `main.js` 打在 `body` 上的 `platform-windows` 类，
  不用异步的 `avTrustStore.supported`——后者默认 false，Windows 首帧会先把卡画在左栏、
  状态回来后再跳到右栏。
- **判据**：`designAlignment.test.js` 改为「左栏三张内联卡 / 右栏两张内联卡」＋两个互斥
  挂载点（非 Windows 在左、Windows 在右）；那张卡的 caption / 行式条目断言改读组件文件，
  `.diagnostics-list` 的 scoped 样式判据跟着搬到组件；`cardChrome.test.js` 的卡头契约
  清单加入新组件。

## 侧栏去掉编号，展开与收起都用图标（2026-11-02）

用户看侧栏截图提要求：「菜单栏的展开、收缩都需要有明确 icon，不要 01、02 这种表达方式」。
此前展开态用 CSS 计数器画 01–07 的序号，图标只在收起态出现。

- **实现**：删掉 `.sidebar__nav` 的 `counter-reset`、`.nav-item` 的 `counter-increment` 与
  `.nav-item::before` 整条；`.nav-item > .el-icon` 从 `display: none` 改成恒显示
  （17px、`flex: none`），收起态不再需要单独把图标放回来。
- **保留**：`.nav-item` 11.5px / 行高 34px / `gap: 10px`、分组标题 8.5px、角标与底部工具区
  不变。页头的 `data-page` 编号不在这条范围内——那是页面题头，不是菜单。
- **判据**：`designAlignment.test.js` 那条改为「展开与收起都用图标」：禁止 `.nav-item::before`
  与 `navnum` 回来，图标规则必须是显式 17px 且不许 `display: none`。

## 浅色也采用夜航日志账页布局 + Windows 底边线（2026-11-02）

用户报两件事：① 浅色 / 暗色主题布局不一致；② UI 底部看不到边线、内容像被割裂。
选择的方向是把浅色主题也调成暗色主题的「夜航日志」风格，即**两套主题共用同一套
布局，只换配色**。此前方案 B 只作用于暗色（`html.dark .main-view ...` 一组覆盖），
浅色仍是卡片 + 12px 缝隙的老布局，切一次主题整页重排。

- **根因（布局）**：几何声明写在 `html.dark` 作用域里。实测同一设置页：浅色 `main`
  12/20/14、卡片间隙 12、列距 12、页头副标题 12px；暗色 21/26/16、列距 34、卡片
  无内边距、副标题 10.5px。
- **做法**：把「夜航日志」的排布提升为两套主题共用——theme.css 尾段变成账页皮肤
  （`.main-view > main`、`.main-view .card` 透明段 + 上下 14px 内边距、`.page-layout`
  行缝 0 / 列距 34、页头题头线、1px `--border` 账页线），`html.dark` 只留颜色 token、
  暗色画布光晕与主操作按钮配色。侧栏、页头、页间距、控件圆角
  （`--radius-card` / `--radius-control` / EP 圆角 = 2px）、列表与安装面板等同样
  收进共用层。独立副窗（日志 / 用量 / 套餐）的 `.card` 不在这层作用域里，保留卡片外观。
- **浅色线色**：账页线读 `--border`；浅色那一档从卡片描边的 `#e8e6dc`（对画布
  1.13:1）提到 `#d4cfc0`（约 1.41:1，与暗色 `#2a3932` 对画布 1.58:1 同量级），
  `--border-soft` 提到 `#ded7c6`，否则纸面上的账页线等于没画。
- **根因（底边线）**：175% 缩放、Windows、WebView2 下实测
  `Chrome_RenderWidgetHostHWND` 高 1311 物理像素而窗口客户区只有 1309——网页布局
  视口比可见客户区高约 1 CSS px，`#app::after` 贴底的 1px 边线整段落在窗口裁切区外。
  修法是 Windows 作用域里把该伪元素 `bottom` 抬 1px；macOS 不动。
- **判据**：`darkThemeB.test.js` 新增「html.dark 只换颜色、不许出现几何声明」；
  `cardChrome.test.js` 卡头留白改为面板卡 10px / 控制塔 9px；`panelSpacing.test.js`
  的 `.page-layout` 改为 0 / 34px；`designAlignment.test.js` 更新概览栅格 gap、页头
  副标题 10.5px、安装面板与内核版本竖线；`windowChrome.test.js` 钉 Windows 底边线
  抬 1px 且不外溢到 macOS。`npm run test:ui` 662 条全绿。
## 被否掉的方案：显示字体改系统字体（2026-11-02）

**背景**：用户在 Windows 10 + 2K 屏报「文字发虚」。先按「透明窗口关掉 ClearType」修过一次
（Windows 改不透明窗口，见上一节），仍报。实测排除法：系统 ClearType 开着、进程
per-monitor-v2、面板窗口 1040×748 设备像素 1:1、截图字形边缘 100% 带子像素彩边——渲染链路
没问题。差异在**字体**：`--font-serif` 那串（Songti SC / STSong / Noto Serif CJK SC）在
Windows 上一个都不存在，中文落到 `serif` 的兜底 **SimSun（宋体）**；宋体在 15px / 31px
这两档没有内嵌点阵可用、走轮廓缩放再叠 ClearType，笔画边缘就是毛的。

**试过什么**：`.card h2` / `.page-title` / `.diag-card__title` 在 `body.platform-windows`
下 `font-family: inherit`，全站显示文字跟 body 那条系统栈（Segoe UI + 微软雅黑）走，参照
Codex 桌面端的「字体：系统」。

**结果：用户否了**，要求「字体调整为最初设计的字体」——衬线标题是设计语言的一部分，
2K 屏字小要走字号，不靠换字体。于是回滚，`--font-serif` 恢复。留在这里是因为诊断结论仍然
成立：**Windows 上宋体落到 SimSun 是已知渲染特性，不要再去「修」它**；后续若还觉得发虚，
方向是字号与对比度，不是字族。

**顺带结论：不内嵌第三方字体**。思源黑体 / MiSans 这类中文字体动辄几 MB，还要自己扛授权、
字形更新与包体积预算，而系统字体在 Windows / macOS 上都是平台最佳实践。

## 字号整体按 125% 排 + 收成一张阶梯表（2026-11-02）

**背景**：上一版试过「壳按屏宽放大窗口」和「标题换系统字体」，都被用户否了。用户要的是
**字号按 125% 排、窗口尺寸不动、字体保持设计里的衬线标题**——也就是「把字放大」，不是
「把界面放大」。

**决定**：把散落在 27 个组件里的 262 处 `font-size: Npx` 收成 `src/theme.css` 里的一张
阶梯表（`--fs-1` … `--fs-15`），值按 125% 重排；组件只写 `var(--fs-N)`。这样「调全站
字号」= 改那十五行，而不是再翻 27 个文件。语义档位（`--fs-block-heading` / `--fs-subtitle`
/ `--fs-page-desc` / `--fs-action` / `--fs-tip-icon` 等）保留成别名指回阶梯，既有调用方
不用改。

**为什么之前没这么干**：设计系统里只有十来个语义字号 token，其余字号是各组件自己写的字面量，
所以任何全局字号变化都会变成上百处替换——上一版就是这么铺开的，用户看到 diff 后指出应该
集中。这次补上了这层。

**没动**：窗口 1040×748 不动；间距 / 盒高 / 图标尺寸不跟着乘 1.25（只放字），所以版面比
125% 缩放更紧、纵向更容易出滚动条，这是这条路的必然代价。**品牌名不进阶梯**（14/12/9px
字面量，侧栏 72px 文字块的已知约束）。

**验证**：`npm run test:ui` 665 条全绿（相关判据从「字号是 15px」改成「字号读 `--fs-10`」）；
用无头 Edge 在 1040×748 下逐屏截图 + 溢出审计（`scrollWidth > clientWidth`），七个面板
无横向溢出；`check:ui-bundle` 与 `check:invariants` 通过。

## 字号只抬小字：15px 以上的回到设计值（2026-11-02 修正）

**背景**：上一版把整张阶梯按 125% 排，用户看了实际界面后指出「右侧内容区这类已经是比较大的
字体，不再放大」——被点名的都是大字：页面标题（31→39）、块标题（15→19）、provider 名称
（14→18）、分段切换与勾选标签（13→16）、KPI 读数（20→25）。大字本来就够，放大只会抢版面、
把内容挤成两屏。

**决定**：阶梯收成 11 档，**只抬小字**：

- 原来 8–12px 的中文按 125% 抬起（8→10、9.5→12、10.5→13、11.5→14、12→14），
  这是 2K@100% 屏上读不清的那一段；
- **15px 以上回到设计值**：块标题 15、强调正文 16、列表数值 18、KPI 20、页标题 31；
- 语义别名跟着改（`--fs-block-heading` = `--fs-6` 15px，`--fs-page-desc` = `--fs-4` 13px 等）。

**为什么这样做而不是继续整表乘 1.25**：字号的意义不是「一律变大」，而是「在小字可读、大字
不抢版面之间取得平衡」。整表等比放大属于机械变换，代价是版面层级被压平（标题和正文拉不开
差距）；分档抬升保住了原设计的大小关系。

**没做**：间距 / 盒高 / 图标仍不跟字号走。**判据**：`officialChatTabs.test.js` 的手写体页签
断言从「≥ --fs-10」改成「≥ --fs-6（15px）」，其余 664 条原样通过。

## 多内核共用页面：切 tab 只换数据，不换页面（2026-11-07，用户要求）

**背景**：MiniMax 接入时，多个页面用「整页 / 整卡 `v-if`」按内核分叉——用量分析整页换掉，
设置页的工作台卡换成一张只读的 MiniMax 卡，内核版本页用 63 行的独立组件顶掉整页（连与内核
无关的磁盘用量一起消失）。用户明确否掉这种做法：*「对于共用 UI 需要让 Tab 切换内核的范围
控制到最小的 UI 变化上，而不是整个功能页完全用 tab 隔离多个内核，简单粗暴的隔离会出现过多
重复、无意义的页面开发。」*

**决定**：共用页面只有**一份**骨架，切 tab 换的是**数据与少量字段**，不是页面或整张卡。

- **用量分析**：套餐卡常驻（它是壳自己的云端额度，与选中的内核无关）；模型用量卡壳只有
  一张，只有里面那份内容按内核变——MiniMax 没有可靠来源，就在同一张卡里说明原因。
- **设置页工作台卡**：一张卡、一个表单、一颗保存按钮；只有端口字段按内核分叉（DSH 是
  Web UI 端口，MiniMax 是它自己实例的工作台端口 + App 端口）。为此新增
  `save_mcode_ports`：运行中 / 启动中 / 端口被占 / 两端口相同都拒绝，且拒绝时不改配置。
- **内核版本页**：去掉整页替换与 `McodeInstallation.vue`。已安装列表共用一个行骨架（版本号
  + 状态 + 动作），DSH 与 MiniMax 各自喂数据；版本来源同一列位置（DSH 官方发布 / MiniMax
  npm 候选 + 安装与运行环境）；**磁盘用量两个内核都显示且是同一份实现**（`disk_usage`
  扫的本来就是整个 `DSH_XLINK_HOME`）。

**当时没做（已在同日续做里补上，见本节末尾）**：概览页仍是 `McodeOverviewCard v-if` +
整块 `v-else` 的分叉——它的 DSH 分支（控制塔、系统健康、最近操作、今日用量）与 MiniMax
当时没有可共用的数据。

**判据**：`ui/test/usageAnalysisPanel.test.js` 与 `ui/test/designAlignment.test.js` 里原本
钉「整卡 / 整页 v-if 分叉」的断言改成钉新形状（套餐卡常驻、卡壳常驻、只有端口字段按内核
分叉），并加了反向验证；`npm run test:ui` 701 条全绿。**产物预算**：本轮 JS 从 757,588 涨到
764,081（净增约 6.5 KB，含删掉独立组件后新增的共用骨架），`JS_BUDGET` 760,000 → 772,000 并
在脚本里记了理由。

**2026-11-07 续：概览页也收敛了**（补上本节「当时没做」那一条）。删除
`kernel/McodeOverviewCard.vue`，概览页只剩一份骨架：同一张「当前内核」卡（版本号 / 状态行 /
Node.js 与数据目录指标 / 工作台动作）与同一张「今日模型用量」卡。按内核变的是数据与动作：
MiniMax 的主操作是启动 / 打开 / 停止（含「清理旧记录」「安装内核」），DSH 保留官网网页版
与两个窗口入口；今日用量卡在 MiniMax 下说明「尚未接入可靠来源」，不拿 DSH 或其他账户的
数字顶替。

**仍然没做**：控制塔（需要关注 / 系统健康 / 最近操作）读的是 DSH 的状态与诊断记录，
MiniMax 还没有对应数据源，所以那三张卡先只在内核是 DSH 时渲染（`v-if="!isMcode"`）；
数据补齐后删掉这个条件即可，页面骨架不用再改。

**判据**：`ui/test/designAlignment.test.js` 改成钉「骨架常驻、无整页替换、控制塔不冒充
MiniMax」并带反向验证；`ui/test/quitFallback.test.js` 的人工清理入口改从
`OverviewPanel.vue` 读；`ipc-fields` 判据按 McodeStatus 的字段集合放行概览页对
`mcodeView.value` 的读取（反向验证过：`kernel.value.node_version` 仍被拦）。
`npm run test:ui` 701 条全绿，JS 768,788 字节（预算 772,000）。
---

## 内核维度从页面级降成控件级：顶部页签条整条删除（2026-11-07，用户要求）

**背景**：顶部那条「DSH | MiniMax 实验」页签条是**页面级**上下文——切换器贴在内容区顶部，
看起来像「下面这一页换内容」，实际是「整个壳换一个内核」，而且跨页、跨重启留存（落盘
`kernel-selection.json`）。用户看用量分析页时发现矛盾：那一页的套餐额度五张卡（DeepSeek /
MiniMax-CN / Codex 额度 / opencode / 2api）在两个内核下**完全一样**
（`SubscriptionUsageCard.vue` 根本不读 family），只有「模型用量」一张卡与内核有关，却因为那条
页签把整页画成了「另一个内核的数据」。用户原话：*「用量分析整体应该都属于是公共数据，为什么
还要做 Tab 页签，来区分内核呢？」*

**事实依据**：`kernel-selection.json` 的模块文档第一句就是 "The management view selection does
not change a running instance."；全仓只有 `get_kernel_selection` / `set_kernel_selection` 读它，
两个工作台的启停命令本来就分开（`start_mcode` / `stop_mcode` 对 `start_kernel` /
`stop_kernel`），进程本来就并行跑。所以「内核维度降为控件属性」是**纯前端**的事，不动数据模型。

**决定**：内核维度是**与内核指向性绑定的控件**的属性，不是整页的属性。

- 顶部页签条（`ui/src/kernel/KernelTabs.vue`）整条删除。
- 切换器改成分段控件 `ui/src/kernel/FamilySwitch.vue`，只出现在三个绑定页的页头：概览、内核
  版本、记忆。形状是药丸分段控件，不画页签条那种下划线——下划线让人读成「下面这一页换内容」，
  分段控件读起来是「这一页里这几张卡换数据源」。
- 公共页（用量分析 / 插件 / 技能 / 设置）不读内核状态，也不出现切换器。用量分析页的「模型用量」
  卡恒定渲染内核那份读数；「MiniMax 工作台自身用量尚未接入」降级成卡里的一行说明，不再是整块
  占位。
- 状态仍是全局一份（`instanceStore.family`，落盘 `kernel-selection.json`）：三个绑定页共用一个
  选择，用户不必每页重挑；公共页不读它，所以翻页不影响它们。

**公共页怎么摆两个内核的内容**：并列，不分叉。

- **用量分析**：套餐额度五张卡与模型用量卡恒定渲染；「MiniMax 工作台自身的用量尚未接入」降成
  卡里一行小字（只在实验构建出现），不再占掉一整块。
- **插件页**：MiniMax 那张卡与 DSH 那张卡在实验构建里同时在场，各自卡头挂一枚内核名芯片
  （`.origin-chip-kernel`）。已安装清单是 DSH 的那一份，文案直说 DSH（「只管理 DSH」/
  「DSH 尚未接入其他插件」）——「当前内核」在这一屏已经指代不明。
- **技能页**：MiniMax 那条同步状态恒显，DSH 专属块（遮蔽 / 冲突 / mode-chip）也恒显：技能库
  本来就是两个工作台共用的，共享库的实情不该因为内核状态而消失。
- **设置页**：工作台卡把两套端口并排列出（DSH Web UI 端口恒显，MiniMax 两个端口只在实验构建
  出现），一个保存按钮把两套各存各的。保存按钮**单独占一行**：它存的是这张卡上的全部端口，
  挂在某一行端口的格子里会让人以为它只存那一个。MiniMax 那两个端口没取到值、或改都没改过时
  不硬存——否则没装 MiniMax 的用户点一次保存就弹一句「端口需为 1024–65535 的整数」。

**内核名芯片**：`.origin-chip-kernel` 叠在 `.origin-chip` 之上，两处不同——大小写照原样
（`lowercase` 是给 npm / github 那种本来就小写的来源词准备的，「minimax / dsh」是把产品名写错）
+ 字族读新加的 `--font-ui`（芯片挂在卡头标题里，而标题走宋体，继承会落成 SimSun 的拉丁字）。

**被否掉的方案**：

1. **概览页拆成「DSH 一张卡 + MiniMax 一张卡」并排**——用户最初认可的方向，但与上一节
   「共用页面不整卡分叉」冲突（同为 2026-11-07 用户要求），也与「同一屏里同一个东西只能有一个
   名字」冲突。最后取「一张卡 + 页头分段控件」。
2. **把切换器挪到侧栏底部当状态栏**——只显示一处控件，但公共页上也会出现，而公共页上它没有
   作用，正是用户抱怨的那件事。
3. **删掉内核选择整条链路（Rust 命令与 `kernel-selection.json` 一起删）**——入口隐藏 ≠ 能力删除
   （本仓先例：`setDefaultInstance`）；而且三个绑定页共用一份选择，比每页各挑一次好用。
4. **设置页的端口继续按内核分叉**——这一页只剩「工作台」一张卡跟内核有关，另给它挂一个切换器
   会让整页看着像跟着内核翻；并排列出更省事，也让「保存配置」的作用对象一次说清。

**判据**：`ui/test/designAlignment.test.js` 改成钉「顶部零页签挂载、切换器只出现在三个绑定页、
公共页零切换器、实验徽记画在 `FamilySwitch` 里」，每条都带反向验证。

**2026-11-07 续（用户要求）：改成每页各一份**。上面「三个绑定页共用一份选择」不再成立：
用户要的是「在每个具体功能里各自挑内核、互不干扰」。三个绑定页各存一份（`instance.js`
的 `pageFamilies`），`FamilySwitch` 改成受控组件（`modelValue` + `update:modelValue`），
页面各调 `selectPageFamily(page, id)`。在概览切到 MiniMax，记忆页和内核版本页仍停在各自
的内核上。落盘仍只有一个 `kernel-selection.json` 值，用途降成「启动时给三页播同一份种子」；
要跨重启各记各的，得把后端单值改成按页三键。

---

## 公共能力用「接入」开关，不用内核切换器（2026-11-08，用户要求）

**背景**：插件 / 技能 / 记忆是共享能力——同一份技能库、同一批记忆记录同时服务两个内核。顶部页签
删掉后公共页不再有内核切换器，用户看不出 MiniMax 到底有没有用上这些能力。用户要求给这类公共能力
加一个「连接到新内核」的开关，并且公共能力的交互要和绑定页的内核切换器区别开。

**决定**：

- **技能页**：新增「接入共享技能库」开关，真值是 MiniMax 实例配置里的 `link_skills`（默认接入，
  老配置按接入读）。关闭后**下次启动** MiniMax 时走同一套技能事务把已同步的副本移除
  （`skills/mcode.rs::detach_locked`），共享库与 DSH 不动；再次接入后下次启动重新同步。常驻只有
  状态词（已接入 / 未接入 / 同步失败），生效时机与共享语义收进 ⓘ。
- **插件页**：沿用逐插件开关（Ponytail 已有），文案统一成「接入 / 已接入 / 未接入」，不再写
  「授权」——它和技能、记忆是同一件事。
- **记忆页**：沿用已有的「允许 MiniMax 使用已批准和已授权的记忆」开关，它就是记忆能力的接入
  开关，不另加一枚。
- 三页都不挂 `FamilySwitch`：公共能力是「一份资源 + 一条接入开关」，切内核换数据源才是绑定页
  的形状。

**被否掉的方案**：给插件 / 技能再加一个总开关——插件是逐项能力，聚合开关会和 Ponytail 的逐插件
开关重复；技能只有一份库，一枚开关就够。把开关做成页头分段控件——那是绑定页的形状，读起来像
「这一页换内核」。



## 内核安装区局部切换与 npm 入口（2026-11-08）

- DSH / MiniMax 切换器下沉到安装区卡头，只控制已安装与官方版本 / 运行环境。磁盘用量在安装区外，始终是整个壳的只读统计。
- MiniMax 参考 DSH 的官方版本列表：标题右侧 npm 来源与刷新，版本行内显示已安装、安装或不可安装原因。移除独立固定版本大按钮，仍只允许后端标记可安装且版本为 0.1.7 的候选调用当前固定版本安装命令。
- 包路径是已有安装的选择，不是 npm 下载目标；受管安装无需手填。Node 与已有包路径放在折叠的「运行环境与已有安装」中（该折叠块 2026-11-08 同日按用户要求删除，见后文）。DSH 与 MiniMax 的安装树、原生依赖检查及摘要校验保持各自既有边界，不因布局统一而合并。
- 判据：`mcodeVersionsPanel.test.js` 校验安装区 AST 范围并反向移动切换器；原有页级隔离、读取错误重试及忙碌禁用测试保留。

## 设置页：MiniMax 实验端口分组（2026-11-08）

工作台卡保留一个表单和一个保存按钮。DSH Web UI 端口属于「DSH · 稳定内核」中性边框组；
MiniMax 工作台 / App 双端口放在同一个粉色边框组，标题明确标出「MiniMax · 实验内核」，
不只用颜色传达归属。组内可见标签简写为「Web UI 端口」「App 端口」，不重复 MiniMax；
输入框可访问名仍保留 MiniMax 前缀。实验组仍仅在实验构建显示，保存按钮放在两组之外，不改变端口校验、
回填和保存逻辑。分组使用原生 fieldset / legend，输入框提供完整可访问名；
两套主题只改变实验色 token，保持同一布局和字号。


## 概览页：内核切换只作用于「当前内核 + 系统健康」（2026-11-08，用户要求）

**背景**：MiniMax 接入后，概览页把整块控制塔按内核隐藏（`v-if="!isMcode"`）——切到 MiniMax，
「系统健康」和「最近操作」一起消失；「今日模型用量」换成 MiniMax 专属占位；「官网网页版」按钮
也只在 DSH 分支里。用户要求：切换器只该管「当前内核」「系统健康」两块，下面的「今日模型用量」
「最近操作」是两内核共用的公共 UI，不应同时切换；「官网网页版」应是全局按钮，哪个内核都能开。

**决定**：

- 页头 = 壳级动作 + 切换器：`[官网网页版] [DSH | MiniMax]`。官网网页版是壳级窗口（壳自己管理，
  与内核无关），两个内核下都能开 / 关；它的次要入口「官网网页版窗口」仍落在版本号那一行右侧，
  两个内核下都渲染（「工作台窗口」仍是 DSH 专属）。
- 控制塔常驻，三张卡作用域分开：
  - **系统健康**：四行保持同一套行名与顺序，前两行按内核换数据源——DSH 读看护隔离数与技能库，
    MiniMax 读 Ponytail 接入与共享技能同步状态（状态词与技能页共用
    `kernel/mcode.js::mcodeSkillsState`）；日志系统与最近快照是壳自己的账，两个内核同一份。
  - **最近操作**：壳的诊断运行记录，两个内核共用。
  - **需要关注**：只有 DSH 的事故 / Node / 预检 / 设置告警有数据源，MiniMax 下不渲染，不拿
    DSH 的读数冒充。
- 「今日模型用量」两个内核共用同一份壳账，不按内核分叉；「MiniMax 工作台自身用量尚未接入」
  降成卡头 ⓘ 的一行说明（只在实验构建），不占正文。
- 页头副标题同步改成：「当前内核的状态与系统健康，以及共用的今日用量和最近操作。」——原句
  「这个实例今天的使用量」已经不准（今日用量是壳的账，不按实例 / 内核分）。

**为什么系统健康不整体共用**：它的前两行是**内核接线读数**（隔离 / 接入状态），共用一份就只能
显示某个内核的数，等于把切换器的作用域做回一半。日志与快照保持共用，是因为它们本来就是壳级事实。

**布局事实**：1040 宽下页头可用 831px，副标题缩短后占 364px，动作组 303px（官网网页版 121 +
切换器 174 + 间距），单行放得下、不折行；测量方式是 dev server + 真实 CSS。

**判据**：`designAlignment.test.js` 钉「控制塔常驻 + `:is-mcode`」「今日用量卡不按内核分叉」
「官网网页版在页头、不在内核卡」「控制塔模板里 isMcode 只出现在需要关注守卫」并带反向验证；
`check-invariants` 的 `[ipc-fields]` 把 `diagnostics/ControlTower.vue` 登记为 McodeStatus 的
接收方（它现在直接读 Ponytail / 技能接入状态）。

## 内核版本页：打开自动拉 npm 列表，移除「运行环境与已有安装」（2026-11-08，用户要求）

**背景**：MiniMax 的「官方版本」列表此前要点标题右侧的刷新图标才拉取，进页面看到的是
「点击标题右侧的刷新图标获取官方发布列表」；下方还挂着一个 DSH 没有的折叠块「运行环境与
已有安装」（重新检测运行环境 / 手填 npm 包目录与 Node 可执行文件 / 保存位置）。用户要求：
打开这一页就自动刷新 npm 最新版本库，并删掉这个额外块。

**决定**：

- 打开这一页、以及页内切到 MiniMax 时自动调 `loadMcodeAvailableVersions()` 拉 npm 列表；
  加载中显示「正在获取…」，失败时提示可点刷新重试。DSH 一侧沿用启动自检 + 后端 60s 缓存，
  不在这条路径上加「有新版本」的提示（用户已经站在这一页上了）。
- 删掉折叠块及其脚本状态（手填路径、编辑态标记、保存位置、重新检测按钮）和它的 scoped 样式。
  运行环境由壳管理与自动检测。「选用」是逐个已装目录的动作（与 DSH 的「切换」同形），保留；
  切目录时沿用当前 Node 配置，避免顺手把 `configured_node_path` 清掉。
- 后端 `save_mcode_installation` / `detect_mcode_installation` 命令保留（入口删除 ≠ 能力删除），
  `mcode.js` 的两个 wrapper 也不当死代码清掉。

**判据**：`mcodeVersionsPanel.test.js` 钉「内核切换监听里自动拉列表」「安装区不再有折叠块」
并带反向验证；`designAlignment.test.js` 的概览判据不受影响。

## 磁盘用量纳入新版内核的安装树（2026-11-08，用户要求）

**背景**：磁盘用量四个分类里，「内核版本」只扫 DSH 的受管安装树（`<data_dir>/kernels`）。
MiniMax 的受管安装落在 `<xlink_home>/mcode/<壳模式>/kernels/<version>/`（本机实测 190 MB），
整块没被统计，合计因此少一块——实例数据那栏本来就含 mcode 实例目录，缺的只是安装树。

**决定**：

- 「内核版本」一栏合并两个内核的已装版本：DSH 的 `<data_dir>/kernels` + MiniMax 的同壳模式
  `<home>/mcode/<mode>/kernels`。模式从 `data_dir` 的目录名读回（它本来就是
  `family_runtime_dir` 的产物）；`data_dir` 本身就是 mcode 树时不重复扫。MiniMax 行名带
  内核前缀（「MiniMax 0.1.7」）、id 带 `mcode-` 前缀（v-for key 不能和 DSH 版本撞）。
  目录不存在（没装 MiniMax）不算读失败，不记 unreadable。
- 缓存文件加 `CACHE_SCHEMA`（当前 2）：扫描口径变了就抬一版，旧格式 / 旧版本按没有缓存处理
  重扫。否则新数字要等满 24 小时才出现，用户会以为这一版没生效。
- 不新增第五个瓦片：四类位置固定、布局恒两列，把新内核并进「内核版本」一栏既符合语义也不动布局。

**判据**：`diskusage::tests::measure_counts_minimax_managed_kernels`（含「另一个壳模式的树不计入」
与「删掉 MiniMax 树后行消失」）、`cache_schema_bump_invalidates_old_reports`；README、
`docs/architecture/architecture.md` 与 `ui/AGENTS.md` 同步。

## 技能页双内核目标开关（2026-11-08，用户要求）

**问题一：技能页整页渲染报错**（「Can't find variable: mcodeSkillsState」）。上一版把
MiniMax 同步状态改成复用 `mcode.js::mcodeSkillsState`，但 import 行漏了这个名字：
Vite 构建与全部单测都不报错（模板 / `script setup` 里的自由变量在运行时才解析），
真机上一进技能页就整页渲染失败。修好 import，并在 `skillEnable.test.js` 里钉住
「用了就必须 import」这条——它是构建与门禁都盖不住的盲区。

**问题二：技能启停只有一个全局开关，两个内核一起变**。用户要求「技能的启用方式参考插件的
做法，安装本身应该是全局性质，通过启用控制它在那边内核生效」。落法：

- 安装和中央库保持全局唯一，不按内核分叉；每个技能行给 DSH / MiniMax 两个目标开关，
  对位插件页每行的 DSH / MiniMax 勾选，不引入内核切换器。
- 存储：`store.json` 的每个技能条目加 `mcode_enabled: Option<bool>`。`None` = 老清单，
  跟随 `enabled`（就是加标记之前的行为，升级后不需要迁移）。DSH 开关切换前先把当时的
  有效值钉住，两个开关从此互相独立；不清这一步的话「停用 DSH」会顺带停掉 MiniMax 那份。
- 同步：MiniMax 启动时按 `syncs_to_mcode()` 收（显式标记优先，缺省跟随 DSH），
  不再按 DSH 的 `enabled` 过滤；从中央库取源，所以「DSH 停用、MiniMax 启用」也能同步。
- 界面：开关旁带 DSH / MiniMax 标签（此前两枚无名开关分不清管哪个内核）；包里有多个
  技能时额外标出技能名；没有 MiniMax 实例或顶部「接入共享技能库」总开关关着时置灰，
  悬停说明缺的是哪一步。总开关保留：它仍是「MiniMax 读取共享库」这个能力级的闸，
  关掉后在下次启动时移除全部已同步副本。
- 生效时机如实写：DSH 即时生效，MiniMax 下次启动时同步；后者在按钮 tooltip 里。

**判据**：`skills::manage::tests::mcode_target_is_independent_from_dsh_toggle`（显式标记、
钉值、反向验）、`skills::mcode::tests::reconciles_flat_skills_without_overwriting_user_changes`
（新增「DSH 停用但 MiniMax 标记在 → 仍同步」「MiniMax 显式关闭 → 移除」「无标记 → 跟随」三段）、
`ui/test/skillEnable.test.js`（命令签名 / 面板接线 / import 回归 / 后端两个独立开关的形状）、
`designAlignment.test.js` 的共享能力用例。README、ui/AGENTS、技能管理文档与接入计划同步。

## 技能页接入开关收进卡头 + MiniMax 手写体字样（2026-11-08，用户要求）

**缩小放到标题栏**：技能页上那条独立的「MiniMax 接入共享技能库」整宽行（约 44px 高）收进
「已安装」卡头右端——芯片 + 状态词 + small 开关 + ⓘ，与「N 个可更新 / 检查更新」同一行。
整句共享语义与生效时机仍只在 ⓘ 里，卡片下方不再空出一行。判据
`ui/test/minimaxWordmark.test.js`：开关必须落在 card-head 与列表之间，旧的
`.mcode-link-row` 模板与样式都不许留（留着是一条没有宿主的选择器）。

**MiniMax 用手写体**：新增 `--font-hand` token（Bradley Hand / Segoe Print / Ink Free /
Noteworthy / cursive）与 `.minimax-wordmark` 类，挂在词本身上而不是整枚标签——同一行的
中文说明仍是正文字族。落在三处：内核切换器（`FamilySwitch.vue` 的 MiniMax 段）、插件页
每行的 MiniMax 目标勾选（`MinimaxPluginTarget.vue`）、技能页的芯片与每个技能的 MiniMax
目标标签。只用于品牌字样：正文、按钮、toast 里的 MiniMax 保持正文字族。磁盘用量行里的
「MiniMax 0.1.7」是后端拼好的整串，本轮不拆。

## 移除技能库「接入共享技能库」总开关（2026-11-09，用户要求）

**背景**：2026-11-08 的两级设计（总开关 + 每技能目标开关）在实机上被读成多余：每行已有
DSH / MiniMax 开关，总开关的效果恰好是「所有逐项开关的逻辑与」；关闭时逐项开关仍显示
「已接入」而实际不同步，等于第二层真相。

**决定**：删除总开关与其全部后端语义——配置字段 `link_skills`、命令
`set_mcode_skills_linked`、`skills/mcode.rs` 的 detach 路径（`detach_locked` / `detach`
测试入口）一并删除。MiniMax 启动恒定按逐项 `mcode_enabled` 对账；老配置里的
`link_skills:false` 被忽略，下次启动按逐项标记重新同步。逐项开关在旧实例
（`legacy_unavailable`）或平台不支持时置灰并说明原因。判据改为
`ui/test/skillEnable.test.js` 与 `designAlignment.test.js` 的「没有总开关」断言；
`minimaxWordmark.test.js` 只留手写体判据。

## MiniMax 体验入口总开关与关闭语义（2026-11-08，用户要求）

**背景**：此前 MiniMax 入口的显示条件只有构建旗子（`VITE_MCODE_EXPERIMENTAL`），用户在界面里
关不掉；而「入口存在」容易被读成「能力已生效」。用户要求设置页提供一个总开关决定是否露出
MiniMax 体验，关闭不停止运行、不删数据、不撤销授权。

**决定**：

- 真值分两层：构建可用读构建旗子；用户偏好存壳设置 `mcode_experience: Option<bool>`
  （`shell/settings.rs`）。没保存过时开发壳默认开、正式壳默认关；显式值按壳模式分文件保存，
  跨重启保留。命令 `set_mcode_experience` 即时写入，独立于端口「保存配置」；`save_settings`
  的合并语义继承该字段，保存端口不会把它退回默认。
- 前端可见性收敛到 `ui/src/kernel/experience.js` 的 `mcodeExperience`（构建可用 ∧ 用户偏好），
  页面不得直接读构建旗子；独立日志窗口没有 `get_status` 的 ACL，用只读命令
  `get_mcode_experience` 取同一份偏好，窗口聚焦时重取。
- 关闭语义：正在运行的工作台不停、数据不删、能力选择不改、授权不撤销。关闭前按真实状态确认：
  工作台运行中 / 存在共享授权 / 状态读不出来（按「仍可能生效」）三种正文，按钮只有「取消」与
  「仅隐藏入口」，取消不改状态。关闭后 MiniMax 端口组整组隐藏且不提交隐藏字段，本次会话的
  端口草稿保留到重新开启。
- 各页只改显示条件：概览页头分段控件关掉即整枚消失（`FamilySwitch` 一个落点），运行状态胶囊、
  退出停机与错误恢复仍按真实运行状态；概览同时在分段控件里汇报两套工作台状态，读不到显示
  「未知」而不是「已停止」。内核版本 / 插件 / 技能 / 用量 / 日志按同一判据隐藏对应入口。
- 品牌词补上粉色：设计 §1 要求 MiniMax 品牌字样用「手写体 + 实验色粉色」，颜色与字族都挂在
  `.minimax-wordmark` 上（两套主题各一份 `--experimental`），芯片不覆盖颜色。

**判据**：`ui/test/experienceSwitch.test.js`（开关结构、即时保存、关闭确认三态、隐藏端口与草稿）、
`ui/test/experienceVisibility.test.js`（只有 experience.js 读构建旗子、各页接线、概览双状态与轮询、
退出链未被可见性短路）、`ui/test/minimaxWordmark.test.js`（手写体 + 粉色）、Rust
`shell::settings::shell_aware_tests::mcode_experience_default_depends_on_shell_mode` 与
`commands::settings_merge_tests::merge_keeps_paths_the_panel_did_not_submit`。

## 记忆接收方开关：与跨来源授权分成两层（2026-11-08，用户要求）

**背景**：记忆页此前只有各实例旧的 `use_approved` 开关与跨来源授权，而 MiniMax 的通用模块
检索路径会绕过旧开关——只改界面或只改旧开关都停不住自动检索。用户要求每个接收目标有独立的
「能不能用记忆」开关，并与授权分开。

**决定**：

- 新增 `src-tauri/src/memory/targets.rs`：每个接收方（DSH / MiniMax）一个显式开关，状态存
  `<xlink_home>/shell/<mode>/memory-sharing/target-use.json`。关闭后该目标不返回记忆：
  DSH 的 `/v1/memory/context`（含 `context_only`）、`/v1/memory/read`（按 id 读也要拦——
  id 会从历史注入里留下来）与 MiniMax 的 `retrieve_mcode`（`memory/share.rs`）过同一道
  门禁；提取用的 `pending=1` 只要候选清单，不算使用记忆，不拦。不删记录、不改旧
  `use_approved`、不撤销通用模块授权与逐条授权。状态文件损坏时拒写（不能凭读不出来的文件
  重建、把另一侧的选择清掉）、门禁按关闭处理并留一条节流日志。
- 没显式选择时跟随旧行为（自己的 `use_approved` 或已授权的模块互通），不扩大读取范围；
  状态文件损坏时按关闭处理并在界面报「状态未知」——两种都不会把读不出来猜成开或关。
- 界面：记忆页顶部「允许使用记忆」DSH / MiniMax 两个目标开关，MiniMax 那枚受体验入口控制；
  状态词只有四种事实（已开启 / 已关闭 / 跟随原设置 / 状态未知），授权面板保持独立可见、可撤销。
- 当时的偏差：记忆内容仍按 `pageFamilies.memory` 分支浏览。后续已合并，见下节。
- **页顶精简（2026-11-08 用户要求「过于臃肿啰嗦」）**：原实现是三段——通用记忆模块卡、
  一排「来源：DSH / 来源：MiniMax / 已授权互通」chip、单独一条带边框的「允许使用记忆」横带。
  合并成一张卡两行：卡头（标题 / 授权自动互通 / 统一记忆 / 刷新）+ second row「允许使用记忆
  DSH · MiniMax」（目标开关 + 状态词）。来源 chip 与「已授权互通 / 未授权互通」状态词删掉
  ——来源名由目标名承担、授权状态由开关本身表达，来源实例 / profile 退到目标名 `title`
  （授权弹窗里仍有完整坐标）。

**判据**：`memory::targets::tests::*`（默认跟随、显式关闭只影响该目标且记录不动、MiniMax 检索
返回空、损坏按关闭并报未知、提取 pending 不拦）、`ui/test/memoryTargetUse.test.js`（命令接线、
状态词、失败恢复、与授权互不代改）。

## 记忆页改为共用列表（2026-11-08）

- 按多内核设计稿移除记忆页的内核切换器：顶部保留「新建记忆」、接收方开关与「授权范围」，
  下方为「记忆库 / 待确认 / 效果」。两侧普通记录在同一列表显示，来源、实例及项目范围可辨；
  记录仍留在各自物理库，编辑与删除走来源侧。DSH 导出和清空按钮明确标注作用范围。
- 历史逐条授权独立管理；体验入口隐藏时保留来源记录与授权撤销，MiniMax 记录只读。DSH
  候选、交接、经验、注入与提取诊断保留原有操作和来源标记。
- 目标开关与跨来源许可分开，读取失败时提示状态未知并要求刷新。隔离浏览器使用模拟数据
  检查两侧记录、同 ID、筛选与开关；体验入口隐藏或构建未包含时，历史记录保持只读、授权
  仍可撤销。真实工作台行为仍待验收。

## 任务通知多内核化（2026-11-08，用户要求）

**背景**：任务完成通知原先只订阅 DSH 内核的事件流，设置卡标题在实验构建里显示
「DeepSeek 任务通知」、文案也带 DSH 限定；MiniMax 工作台里跑完的任务不会有任何提醒。
用户要求两个工作台都通知，并且在界面与系统通知里分得清来源。

**决定**：

- **共用一套开关**：三个字段（`notify_enabled` / `notify_away_only` / `notify_sound`）原样
  不动，两个工作台共用。它们回答的是「用户要不要被打扰」，与任务由哪个内核跑完无关；按内核
  拆两套只会摆出六个同样的开关。设置卡标题恒为「任务通知」，不再按内核改文案。
- **两条订阅**：DSH 沿用 `/api/remote.mux` 的 `$events`；MiniMax 新增
  `src-tauri/src/notify/mcode_watch.rs` 订阅 WebUI 的应用事件流（`ws://127.0.0.1:<port>/`，
  `protocolVersion = 1`、`kind = "request"`、`operation = "watchEvents"`），ack 之后按
  `body.type` 消费。完成判据只有一条 `session.finish`；`session.abort` / `session.error`
  不算完成；带 `parentSessionId` 的子代理会话不打扰；`session.title_updated` /
  `session.created` 补标题；`session.start` 记时长。`turnId` 去重（保留最近 64 个），
  断线 1 s→20 s 退避，工作台未运行时只轮询状态、不连端口。
- **来源标识**：`CompletedTask` 增加 `kernel: "dsh" | "mcode"`（serde 小写），
  `TaskKernel::label()` 给短名 `DSH` / `MiniMax`。两条订阅都走同一个
  `record_completion()`，开关、免打扰、入列表、未读与系统通知只有一份实现。系统通知首行是
  `📋 DSH · 标题 ✅` / `📋 MiniMax · 标题 ✅`（平台通知标题仍是「任务已完成」）；设置页
  「最近完成」列表每行标题前放来源 chip，MiniMax 那枚用现有 `.minimax-wordmark`（手写体 +
  实验粉）。MiniMax 事件流不带会话正文，所以它的记录没有问 / 答预览，只有标题与用时。
- **未读按内核分开**：角标数字是合计；DSH 工作台（`harness`）获得焦点只清 DSH 来源，
  MiniMax 工作台（`mcode-workbench`）获得焦点只清 MiniMax 来源；「全部已读」仍清全部。
  两扇窗口被销毁时各自清掉前台标记（否则卡在 true 会让之后的完成永远不算未读）。
  `stop_kernel` / `stop_mcode` 各自停订阅并只清自己那份未读；壳退出两条都停。
- **点击差异**：DSH 通知仍深链到具体会话（`navigation.rs` + 内部客户端
  `openSession`）；MiniMax WebUI 没有可深链的会话路由，点击只把 MiniMax 工作台窗口抬到
  前台（`show` + `unminimize` + `set_focus`），窗口没开则叫回管理面板——不凭空新开窗口。
- **失败与版本边界**：MiniMax 这条是上游私有协议，壳只承诺当前交付的 WebUI 0.1.7（安装与
  启动本来就只认这一版）。连不上、error 帧或协议漂移都只写壳日志 `notification-mcode`
  （60 s 节流）并把「MiniMax 事件流未连接」与原因报给面板（`watchingMcode` /
  `mcodeError`），不影响工作台与 DSH 通知。设置页只在「体验入口开 + MiniMax 工作台运行中 +
  未连接」时显示这条提示，与 DSH 那条拆开。

**判据**：Rust `notify::mcode_watch::tests::*`（握手帧形状、`session.finish` 产出完成、
`turnId` 去重、abort / error 不算完成、子代理不打扰、缺标题退回会话 id）、
`notify::task::tests::task_kernel_labels_and_serialised_names_are_stable` 与
`mcode_body_names_its_own_source`；前端 `ui/test/notifyKernels.test.js`（标题不按内核分叉、
来源 chip 两个分支、MiniMax 未连接提示的三项条件、DSH 提示不回退，均带反向验证）与
`ui/test/notifications.test.js`。

**待验证**：真实 MiniMax 工作台跑完一轮任务后的端到端系统通知、工作台崩溃 / 重连后的续订、
点击 MiniMax 通知把工作台抬到前台，以及 macOS / Windows 安装版上的通知点击。本机已在
WebUI 0.1.7 临时实例核对 ack 帧、`session.created` 载荷与 `session.finish` 的发布形状，
这只证明协议形状，不等于实机验收。

## 概览「当前内核」卡：MiniMax 与 DSH 的主按钮 / 快捷按钮对齐（2026-11-09，用户要求）

**背景**：用户截图对比两个页签：DSH 卡有「工作台窗口」「官网网页版窗口」两枚次级快捷
按钮，MiniMax 卡没有；同时 MiniMax 运行中时主按钮位置显示「重试停止」，旁边还有一枚
「停止服务」。原话：*「在 dsh 内核是有这两个快捷按钮，MiniMax 内核没有，而且这个重试
按钮为什么会存在，不应该有它」*。

**根因**：`McodeStatus.stop_required` 的语义是「有进程需要停」（后端
`active.is_some() || error.is_some()`，供版本页 / 插件目标禁用用），**运行中也恒为
true**。概览主按钮的状态机却把它当成「上次停止失败」排在 `running` 之前，于是运行中
永远命中「重试停止」，把「打开工作台」整条挡住；MiniMax 也就只剩一枚用户看不懂的恢复
按钮。

**决定**：
- 主按钮与 DSH 同一台状态机：文案恒为「工作台」，运行中 ▶→⏸、点击停止（带确认框），
  停止态点击启动（启动本身会打开窗口）。「重试停止」只留给**不在运行、但有残留进程
  记录**的那一种状态（此时它是唯一恢复入口）；独立「停止服务」按钮并入主按钮后删除。
- 次级快捷按钮两个页签同形：工作台窗口按内核各一枚（DSH `harness` / MiniMax
  `mcode-workbench`），官网网页版窗口两内核共用；运行中淡入，仍只做「把窗口带到台前」。

**判据**：`ui/test/designAlignment.test.js`（次级排显隐条件、两枚「工作台窗口」+ 一枚
「官网网页版窗口」各自接对命令、`.btn-action` 计数 9→8）；`ui/test/quitFallback.test.js`
仍钉「清理旧记录 → 重试停止」两枚恢复入口不丢。

## 插件页只保留上游 Ponytail 一枚「检查更新」（2026-11-10，用户反馈）

**背景**：用户截图圈出 Ponytail 行尾多出的「检查更新」：行内已经有上游版本旁那枚，
MiniMax 目标后面又有一枚同名按钮；当时 MiniMax 显示「需先停止」，多出来的那枚点不动。
第一轮修复只把 MiniMax 那枚改名为「检查 MiniMax 更新」，并补了禁用态显隐。用户随后
指出这不是命名问题：dsh-xlink 提供的内置插件应当完整对 DSH / MiniMax 负责，更新按钮
是为「更新上游 Ponytail 本身」而存在的，同一插件按内核各挂一枚更新动作就是重复。
原话：*「这里的更新按钮我认为是多余的……之所以给这个按钮一个更新按钮，是为了更新上游
的 ponytail 这个插件本身」*。

**根因**：`MinimaxPluginTarget.vue` 的更新动作是 2026-11-08 合并插件清单时从原独立
MiniMax 卡搬过来的，条件只写 `pluginId === 'ponytail' && target.enabled`：只要 MiniMax
接入勾着就渲染，不看 `target.disabled`（运行中「需先停止」、状态不可用、需清理旧记录）。
于是它和行内查上游内容的那枚同名同排，读起来就是重复的第二枚按钮。

**决定**：
- 删除 MiniMax 目标后的整组更新动作（「检查 MiniMax 更新 / 有更新 / 已是最新 / 更新」），
  目标只留接入开关与状态。
- 行内上游版本旁保留唯一一枚「检查更新」：它先同步 DSH 的上游内容；若 MiniMax 的
  Ponytail 已启用且工作台已停止，再把同一份上游最新版应用到 MiniMax。运行中
  （`running` / `stop_required`）无法替换实例里的插件文件，跳过并等停止后再点同枚按钮。
- 同日前一轮「改名为检查 MiniMax 更新」的方案作废。

**判据**：`ui/test/pluginTargets.test.js` 钉 MiniMax 目标模板没有更新按钮与更新动作
引用，并钉面板把上游检查联动到 MiniMax；`mcodeStatus.test.js` 继续钉授权撤销 / 实例
切换后清掉旧更新结果。

## 设置页端口分组的 MiniMax 字样与 MiniMax 工作台灯光（2026-11-10，用户要求）

**背景**：用户截图指出设置页同一张卡里「MiniMax 内核体验」已经是手写体，下面的端口
分组 legend 却仍是正文衬线字；同时要求 MiniMax 工作台拉绳灯「拉绳效果下的灯光」也
偏粉色系——此前只有绳是粉的，点亮后的灯泡、灯丝与光晕仍沿用 DSH 的暖黄。

**决定**：
- 端口分组 legend 的品牌词挂 `.minimax-wordmark`，与体验开关、插件目标、技能页同一
  个字族与实验粉，不再各写一份。
- 拉绳脚本把点亮色抽成 `--dsh-launcher-lit-fill` / `-stroke` / `-filament` / `-glow`
  四个变量：DSH 保持暖黄；MiniMax 深色用 `#ffb8d9` / `#ffd6e8` / `#b23a78` /
  `rgba(255, 138, 192, 0.9)`，浅色用 `#e470a6` / `#ad3b73` / `#7d2a55` /
  `rgba(228, 112, 166, 0.78)`。出错时的红色闪烁不变，官方对话的台灯仍是品牌蓝。

**判据**：`ui/test/minimaxWordmark.test.js` 钉 legend 的品牌字样类；
`scripts/smoke-pullstring.mjs` 钉 MiniMax 深 / 浅两档 lit 变量与变量化后的灯丝规则。

## 插件 / 技能列表行 hover 底板留出左右内边距（2026-11-10，用户反馈）

**背景**：用户截图指出 Ponytail 行 hover 后，底板左右没有余量，第一列文字直接贴着
底色边缘，读起来像被切开。

**决定**：`.entity-row` 的内边距从 `10px 0` 改成 `10px 12px`，与 `.page-row` 同一档；
hover 底色仍铺满整行，文字与右侧动作各留 12px 呼吸位。

**判据**：`ui/test/designAlignment.test.js` 钉 `.entity-row` 层叠后的生效 padding。

## MiniMax 官方版本列表不再二次加载（2026-11-10，用户反馈）

**背景**：用户截图指出 MiniMax 的「官方版本」经常在已经显示版本行后又出现一次加载
（行还在、列表上叠一层 spinner），DSH 不会。原话：*「为什么 MiniMax 的这个『官方版本』
仓库经常出现二次加载的情况，反而 dsh 不会出现，二者的做法有什么不同吗？」*

**根因**：DSH 的发布列表在 App 启动时由 `checkUpdates(false)` 静默拉一次，版本页只读
`releases.list`，进页不重拉；MiniMax 则在 `VersionsPanel` 的 `watch(isMcode)` 里每次
进入 / 切回都调 `loadMcodeAvailableVersions()`，并用 60 秒 TTL 自动过期。超过 60 秒再
切回就会重新请求，已有行还在，于是 `v-loading` 叠出第二次加载。

**决定**：与 DSH 对齐。App 在 MiniMax 体验开启时静默预取一次；`loadMcodeAvailableVersions()`
成功拿到列表后，自动路径直接短路，只有手动刷新（`force`）才重拉。首次进入仍自动拉，
不要求用户先点刷新。

**判据**：`ui/test/mcodeStatus.test.js` 钉已加载后切回零请求；
`ui/test/mcodeVersionsPanel.test.js` 钉短路条件、App 启动预取与手动 force。
