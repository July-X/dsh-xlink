# AGENTS.md — `ui/`（管理面板前端）

根 [AGENTS.md](../AGENTS.md) 的前端分支。根文件里的「范围」「数据目录」「发布」各章对前端同样适用，这里只收改 `ui/**` 之前要知道的约束。后端见 [src-tauri/AGENTS.md](../src-tauri/AGENTS.md)。

技术栈：Vue 3 + Element Plus 单页应用，Vite 构建到 `ui/dist/`（即 `tauri.conf.json` 的 `frontendDist`）。

## 设计系统（uiv2 改版，2026-10-07）

设计稿 `docs/ui/dsh-xlink-ui-redesign-draft.html` + 说明 `docs/ui/dsh-xlink-ui-redesign-dev-guide.md`。**实现以源码行为为准，设计稿只表达布局与信息层级**——不要为了填满卡片去加当前版本没有的状态、按钮或统计数据。

- **窗口固定 1040×748，不可缩放**（`tauri.conf.json`）。侧栏 224px，收起态 64px；主区宽决定了绝大多数页面是双栏栅格而不是单列。改窗口尺寸要连带改各面板的 `min-width`。
- **层级只有三步**：canvas（整窗底）→ surface（卡片）→ chrome（标题栏 / 侧栏）。**不再有半透明玻璃与背景网格**（旧版的 `.app-bg` 光晕 + 13.5px 网格已整体删除），卡片是不透明实色 + 1px 描边 + 8px 圆角。
- **明暗双主题，判据只有一个：`html.dark`**。`shell/theme.js` 读一次 localStorage 并落成这个 class，Element Plus 自带的暗色变量（`theme-chalk/dark/css-vars.css`，选择器同样是 `html.dark`）因此一并生效——**不要另造 `html[data-theme]`**，那会让组件库留在浅色变量上，出现「壳变了、弹窗还是白的」。theme.css 里 `:root` 放浅色 token、`html.dark` 放暗色 token，两套 Element Plus 覆写各自待在自己的选择器下，没有第二处判据。
- **token 一律用语义名**：`--surface` / `--surface-raised` / `--surface-subtle` / `--border` / `--border-soft` / `--text` / `--text-secondary` / `--text-muted` / `--accent` / `--accent-strong` / `--accent-soft` / `--success` / `--warning` / `--danger`。旧名（`--card` / `--bg` / `--muted` / `--good` / `--bad` / `--warn`）已在本次改版里全局改名完毕，**不要再写回来**。
- **页面原语**在 theme.css 的「新版页面原语」一节：`.page-head`（页头：标题 + 说明在左、动作在右）、`.page-title-row`（标题与它旁边的提示图标同行）、`.head-tip-icon` 及 `--warning` 修饰（标题旁的补充 / 警告气泡触发点）、`.page-layout`（双栏栅格，`__col` 是一列，`__full` 横跨整行）、`.page-card` / `.page-card__head` / `.page-card__body`、`.page-list` / `.page-row`、`.btn` 及 `--primary` / `--secondary` / `--ghost` / `--danger` / `--icon`、`.nav-item`、`.plugin-tab`。**先找现成原语，再写 scoped 样式**；每个面板自造一套的结果是六个面板六种圆角。
  - **登记进这份清单的类名，必须真有模板在用。** ce9dd08 曾把 `.health-row` / `.health-dot` / `.health-row__value` 写进 theme.css 并列进本清单，但控制塔的「系统健康」实际走的是 `diagnostics.css` 的 `.diag-row` 家族——三组规则 38 行全是死代码，2026-10-07 删除。**本清单是「已有原语」的索引，不是「打算加的原语」的许愿单**：加一条之前先 grep 一遍模板，确认真有引用再登记。原语名要和设计稿对得上（设计稿那套叫 `.health-name` / `.health-value`，和当时写进去的 `.health-row__value` 也不是一回事）。
  - **`.metrics` / `.metric` 已从全局原语清单里移除**，它们归 `OverviewPanel.vue` 的 scoped 块。原先 theme.css 那条全局 `.metric`（描边 + `--surface` 底 + `10px 12px` 内边距）与 scoped 的「上边线 + 后两格左边线」叠加，把设计稿的一条分隔线变成了三个独立卡片——**全局基线与组件覆盖各写各的，冲突只有走层叠才看得见**。
- **构建标识（dev 鲸眼红 / release Gitea 绿）收成窗顶 2px 细线**，由 `--build-color` 驱动，两种构建共用同一条 `.app-shell::before` 规则只换颜色变量。旧版那条「铺满全高的品牌色渐变带」已删除——它与卡片描边互相拍频。`ui/test/diskUsageAndChrome.test.js` 钉住了「只有这一处画法」。
- **技能页是「已安装 / 社区资源」两张卡**（2026-10-07 对齐设计稿 2817-2820 行）。原先手动安装是「已安装」卡底部的一条虚线分隔加输入框，读起来像「装完了顺手在这里补一个」，而它其实是另一件事——包的来源在社区，与本地已装状态无关。**设计稿第一张那张「技能状态」卡不画**：它写着「启用状态 4 / 4」「活动视图 已同步」，后者没有任何后端字段支撑，硬写就是编数据（设计说明「不能从设计稿推断出的内容」点名了「未在后端返回的余额、百分比、更新时间、星标数量或插件数量」）。
  - **按钮按动作类型选 loading 通道**：`installSkill` 走 `withProgress`（长任务），**不经过 `withLoading`**，所以安装按钮上挂 `isLoading('installSkill')` 永远为 false——那是一个永远不转的假 loading。在途状态由进度浮层表达，按钮只做 `globalBusy || 输入为空` 的禁用。
  - **单页专用样式留在 scoped**，`theme.css` 走反棘轮只许下调。输入行直接复用全局 `.install-row`（`.el-input` 自带 `flex:1`），卡内标题与说明各合成为一条基线规则（`.community-title` / `.community-meta`），不按设计稿那样一份一套。
  - 判据只看**模板块内**（注释已剥）：源文件注释里正是在解释「为什么不画技能状态卡」，按整份文件搜会把判据自己写的话算成命中。
- **判据要看「叫什么」而不是「长什么样」，同一屏里同一个东西只能有一个名字**（2026-10-07 逐页比对时清掉的三处）：
  - 内核版本页的卡头叫**「已安装 / N 个版本」**，不叫「内核版本」——页面标题已经叫内核版本了，卡头复述一遍等于把整页的名字说两遍。组内那个 `<h3>已安装</h3>` 一并去掉，否则同一个词连着出现在两行。发布列表里那个行内 `<el-tag>已安装</el-tag>` 是**另一个含义**（标出这个远端版本本机已经有了），别跟着禁。
  - 插件页外层卡叫**「插件管理」**，右栏那份 dshfind.com 目录才叫**「插件中心」**。原先外层卡叫「插件中心」、右栏叫「插件仓库」——同一份目录，同一页两个名字。
  - **提示图标的可见文字必须和它 tooltip 里的内容是同一件事。** 插件卡头原先 ⓘ 旁边写着「数据来源于 dshfind.com」，tooltip 里讲的却是插件存放路径与生效规则——停在字上弹出另一段话。现在是纯图标。
- **设置页两栏各两张卡，右栏第二张是「环境回退与诊断」**（2026-10-07 用户拍板，对齐设计稿 draft 2844 行）。原先右栏四张（任务通知 / 数据迁移 / 环境回退点 / 深入排查）把右栏拉到两屏高、左栏空一大截；后三者同属「环境出问题时才动」，合成一张之后两栏才配平。卡内是四个 `.page-list-row`：**环境回退点 / 深入排查 / 启动诊断 / 数据迁移**。
  - **`SnapshotCard` 与 `BisectPanel` 因此渲染成 Vue Fragment**，根节点就是那一行，明细与告警是它的兄弟节点，直接落进同一个 `.page-list` 栅格。它们曾各自是 `<div class="card …">`，套进共享卡里就会出现卡中卡。判据查的是**形状**（整页只有一张卡 / 没有两列栅格），不是某个类名——设计稿那份 `.versions-layout` 从未进过 theme.css，拿它当判据等于断言一个本仓库不存在的符号。
  - **收起的只有明细，状态不许藏**。「查看 / 收起」默认收起的是那一屏十几行列表；状态说明（`headline(view)` / `bisectHeadline(view)`）挂在 `.page-list-meta` 常驻，`drifted` 与 `conclusion` 两条告警**不进闸门**——排查跑几分钟时，用户唯一能看懂的进度恰恰不能被折叠起来。四个动作（回到良好状态 / 刷新 / 开始排查 / 停止）全部留在常驻行上，「查看」只开明细。
  - **`.page-list-meta` 故意不抄设计稿的 `nowrap` + 省略号**（draft 1459-1470）。这一处的次行是 `headline(view)` 那类整句说明，截成一行就分不出「还没成功启动过」和「文档损坏」。
  - **收尾那条的分隔线收在 `.page-list > *:last-child` 上，不是 `.page-list-row:last-child`**：Fragment 展开后明细也是列表的直接子级，按行判 `:last-child` 会把展开内容当成收尾行，连带抹掉它上面那条分隔线。
  - 「启动诊断」这一行是**补上的入口**（设计说明 §设置、恢复与迁移 的覆盖清单里有「二分排查 / 启动诊断」，而设置页此前只有概览 / 进度浮层 / 事故弹窗三处入口）。它和「数据迁移」一样是真跳转，不是有内容的展开层。
- **内核版本页维持「一张卡竖排」，不改设计稿的两列栅格**（2026-10-07 用户拍板，**有意保留的偏差**）。设计稿 draft 709-728 行给的是 `.versions-layout` 两列栅格。没跟过去的理由不是「懒得改」，而是这一屏有三处行为依赖当前竖排、且都已调稳：发布列表的 **z-index 层叠**、**淡出带移除后的裁切行为**、以及为 **120px 限高**量过的磁盘区 6+9px 间距。改两列要连这三条一起重调，收益只是左右各短一点。判据钉的是「整页只有一张卡、没有两列栅格」这个形状。
- **顶部工作条（`.workspace-bar`）已整条删除**（2026-10-07 用户指着一张截图说「移除这个区域」，**覆盖设计稿**）。稿子 draft 2516 行是有这条 48px 行的（左边内核族页签、右边「当前实例 + 运行状态」），现在 `KernelTabs.vue` 整个文件、它在 `App.vue` 的挂载点、以及只为它定义的 `--workspace-bar-height` token 全部删除。
  - **它从第一天起就是空操作**，这是删它而不算丢功能的依据：页签按 `kernel_family` 去重，而后端只定义了 `dsh` 一个族（`mcode` 在 `paths.rs` 的注释与测试里都写着「将来的」）。一个族只会画出一个页签，而它必然就是当前那个，于是 `pickInstance` 每次都在 `defaultInstanceId === id` 那一步 `return false`。
  - **`instance.js` 的 `setDefaultInstance` 不许跟着删**——这一条是被测试顶回来才写下的。先前的版本把它连同 `instanceStore.switching` / `selectionRevision` 当死代码清掉，`ui/test/kernelSwitch.test.js` 当场变红（`setDefaultInstance is not a function`），顺带把整个 `test:ui` 拖挂六分钟。它不是「永远走不到的代码」，而是**没有调用方的 API**：那条测试钉着三条真实语义（读实例去重、**陈旧列表不能把一次切换的结果冲掉**、写入失败必须释放 busy），删函数就要连这套语义一起删，那是拿测试换行数。
    - **「没有调用方的 API」和「永远走不到的代码」是两回事**，前者留着、后者才清。判据钉住的不变量是：模板侧零调用 + 函数与 `selectionRevision` 仍在。
  - **删掉的是「界面上的切实例」，不是「实例上下文」与「运行状态」**。后端 `set_default_instance` 仍在，`defaultInstanceId` 的读取路径一个没动：概览页「当前内核」卡头照旧渲染 `族名 / 实例 id`，插件页照旧按实例列出接线状态，运行状态胶囊（`.status-pill`，theme.css 共用词汇）照旧在概览页读同一个 `store.view.kernel`。**代价只有一个**：在概览以外的页面看不到「当前是哪个实例 / 工作台在不在跑」了。
  - 真接入第二个内核族时切换器要重新做一遍，那时直接调 `setDefaultInstance` 即可，语义不必重新推敲。
- **判据扫模板，不扫全文**（`templateOf()`）。本仓每个模板都带着大段解释「为什么这么写」的中文注释，注释里经常**原样引用被判据的词**（「活动视图已同步」那条为什么不画、「插件中心那个 ⓘ」现在在哪）。按整份文件 `indexOf` / `doesNotMatch`，判据会把自己写的解释当成命中。**同一个坑本轮踩过三次，三次都是扫全文**。样式那边对应的是 `stripComments`，模板这边剥 HTML 注释（`stripComments` 只认 `/* */`，对 `<!-- -->` 无效）。
  - 同一条纪律对 **JS 文件注释也成立**：判「前端不再调 `set_default_instance`」时，裸匹配那个命令名会命中 `instance.js` 里正是在解释「这条命令还在后端、只是前端不再调」的注释——断言要写成 `invoke('set_default_instance'` 这种**真调用形态**。
  - 收尾边界取 `<style` 而不是 `<style scoped>`：`PluginsPanel` 没有自己的 scoped 块，写死 scoped 会在这类文件上直接断言挂掉。
- **「入口还在」不等于「文件还在」**。设计说明 §独立窗口 逐个列了十一个窗口 / 弹窗，判据把「文件存在 **且** 在自身目录之外还有挂载点」一起钉住（`WINDOW_ENTRY_POINTS`）。两条都是被反向验逼出来的：
  - 扫引用要带 `.js`：独立窗口的挂载点在 `main.js`（按窗口类型挑根组件），只扫 `.vue` 会把「已经挂在窗口上了」误判成「没有入口」。
  - **同目录的引用不算入口**：组件自家的 store 模块（`usage/usage.js` 里写着 `UsageWindow`）只是它自己的状态容器。第一版判据没排除它，结果把 `main.js` 的挂载点整段摘掉它照样绿。
- **侧栏整体放大一档**（2026-10-07 用户要求「放大字体、icon」，**覆盖设计稿**）。设计稿 draft 509-524 行给的是 `font-size: 13px` / `gap: 11px`，实现此前照抄；现在是 `.nav-item` **15px**、`.sidebar__section-label` **12px**、`.nav-item__badge` **12px / 20px 圆点**、`.sidebar__footer` **12px**，间距同步抬（行 `padding 8px 10px`、`gap 11px`、分组 `gap 4px`、收起态 `padding 8px 0`）。224px 侧栏仍放得下最长的一项（「数据迁移」四字），748px 高的窗口里侧栏总高约 450px，余量充足。
  - **图标必须显式写死**（`.nav-item > .el-icon { font-size: 17px }`），不能只抬 `.nav-item` 的字号：Element Plus 的 `.el-icon` 是 `font-size: inherit` + 1em，只抬父级字号等于图标跟着等比长——那不算「图标被放大」。17 vs 15 是刻意的：图标读起来要比文字再大一点点才不显矮。收起态（只剩 icon 列）读到的是同一条规则。
- **业务能力不因改版增减**：页签分组（工作台 / 资源 / 系统）、侧栏收起，都是把原有信息重新摆位，不是新功能。设计稿里没画的东西不要自己加；设计稿画了而我们判定不画的（顶部工作条、内核版本页两列栅格、侧栏 13px 字号），要在下面单独登记成一条有意偏差并写清理由。
- **侧栏底部那一行是工具区，收起开关也在里面**（2026-10-07 用户要求，**覆盖设计稿**）：稿子的 `.sidebar-footer` 只有「标签 + 版本 + 刷新 + 主题」四件，收起开关画在 `.brand` 右端（`docs/ui/dsh-xlink-ui-redesign-draft.html` 439-468 行）。现在是「标签 + 版本 + 收起 + 刷新 + 主题」五件，收起开关排在刷新之前。取舍是品牌行只留品牌，收起开关和它收起的侧栏右侧那排控件读起来是一件事；代价：224px 里版本号省略得更多，**收起态（64px）必须竖排**（横排要 100px，内容盒只有 48px，否则溢出到主区）。竖排用 `flex-direction: column` 写死，不靠 `flex-wrap`——那会让换不换行取决于实际渲染宽度。`.brand` 的 `position: relative` 随之删除（它只为浮在那儿的开关而存在），品牌行现在只有 logo 与文字。
- **宽表格要给数字列显式列宽（`table-layout: fixed` + `nowrap`）**，并按内容量分别给值。宽版下自动布局会把表头竖排成「文件 / 数」、把 `896.7 KiB` 拆成两行；改成固定布局后列宽不再随内容生长，于是**宽度变成承重的**：给窄了不会换行，而是文字直接溢出表格右边界（迁移向导 2026-10-07 就把「大小」和「文件数」并进同一条 52px 规则，`896.7 KiB` 跑到了边框外）。**路径类长文本列不要 `nowrap`**，让它们折行——完整路径要看得见，截断比换行更容易读错。钉在 `ui/test/migrationPanel.test.js`。
  - **断言要取声明值，不要比原始子串**。同一天第一版断言比的是两段起点不同的 CSS 子串，结果把两列并进同一条规则它照样绿——**反向验时才发现它抓不到任何回归**。写成「抽 `nth-child(n)` 那条规则里的 `width` 再比值」之后，同一处改坏立刻转红。
- **设计稿对齐的判据走层叠后的生效值，规则表要带 `.vue` 的 scoped 块**（`allRulesIncludingVue()`，`ui/test/lib/css-cascade.mjs`）。面板级样式大半写在组件里，只扫 `.css` 时 `.kernel-summary` / `.plan-grid` / `.sidebar__theme-btn` 一条都查不到，「查层叠」会退化成「查全局基线」。这条工具本身也是修过的：`ruleBlocks` 原来不剥注释，而本仓大量规则前带一段长注释，注释里只要有 `{` 或换行，prelude 就从注释开头一路吃到花括号——**表现是「明明写了却查不到」，而且不报错**。
  - **工具分不清的三类选择器，判据改看 scoped 块文本**（`ui/test/designAlignment.test.js` 里都是这么写的）：`:deep(...)`、`:has(...)`、状态属性 `[aria-pressed='true']` 与后代条件 `.sidebar.is-collapsed .x`。它们的共同点是「能不能命中」取决于 DOM 而不是声明归属。想把工具改严格（区分伪元素 / 状态 / 祖先条件）会连带改掉 4 条既有测试的语义——那是另一件事，别在写判据时顺手做。
    - **「祖先条件」这一类还会主动把不相干的规则算进来，而不只是「查不到」。** 2026-10-07 写侧栏图标那条判据时用了 `effectiveDeclaration(['nav-item','el-icon'], …, 'font-size') === '17px'`——工具只认主语 `.el-icon`、忽略祖先，于是更高特异度的 `.btn-row .el-button .el-icon { font-size: 17px }`（300 > 200）被当成答案，**判据因为错误的原因而绿**：图标那条规则改成什么它都答 17px。反向验才把它逼出来。凡是主语类在全仓被多条后代规则复用（`el-icon` / `dot` / `title` / `chip` 这类通用尾巴），一律读剥注释后的规则文本，别信 `effectiveDeclaration`。同一批里 `.nav-item__badge` 也栽在同一个坑上：按主语查拿到的是收起态那个 8px / `font-size: 0` 的小红点。
  - **`effectiveDeclaration` 带伪类维度（`opts.pseudo`），且不给它时会主动排除带伪类的规则**。`.a:hover` 的主语 token 与基线 `.a` 完全相同，混进基线查询就等于把「悬停时的值」当成「静止时的值」回答出去——问 `.head-tip-icon--warning` 的基线色时返回了 hover 的加深色。只认主语**尾部**的伪类：`.a:hover .b` 不算，那是祖先的状态。
- **这轮钉住的几个具体决定**（`ui/test/designAlignment.test.js` 43 条）：概览主栅格 `1.25fr / 0.75fr`；**末行是「需要关注 | 最近操作」左右并排**（2026-10-07 用户改的落位：首行让给「当前内核 / 系统健康」，异常清单挪到最下面）；版本号 18px 且与状态行**同属一个竖向块**（横排时状态胶囊会随内核状态换行）；套餐用量**固定三列**（1040 宽下 `auto-fill minmax(170px)` 会排成四列）；额度行是「名称 22px | 条 1fr | 百分比 auto」一行栅格、条高 6px、百分比在条外；余额总额 14px 粗体、赠金充值降到第二行 10px；侧栏主题开关是 **34px 无图标拨杆**（图标按钮读起来像「进设置」）；内核版本页卡片标题叫「官方版本」而不是「npm 发布」（npm 只是取源渠道，写进标题会在换回 GitHub Releases 后立刻变成错的）。
- **补充 / 警告类提示一律收成标题旁的图标 tooltip，不许再做成常驻整宽条**（2026-10-07 用户要求，插件页的第三方来源免责提示是第一个）。判据是这条提示**不含任何随状态变化的信息**——常驻一整行只为放一句不会变的文案，代价是把页头下面的内容整体下推。语境在哪就把图标挂哪：免责提示的语境是「插件」这个标题，就挂标题旁，不要挂到侧栏或别的页面去。三条纪律：
  - **触发元素必须可聚焦**（`tabindex="0"` + `aria-label`）。hover-only 的信息对键盘用户等于不存在，纯展示的 `div role="note"` 过去能过是因为它本来就一直显示。
  - **tooltip 的 `content` 与图标的可访问名取同一个常量**。两处各写一遍字符串，改了一处就会出现「图标说有提示、读屏说没提示」。
  - **严重程度要分色**：ⓘ 是「补充一句说明」，⚠ 是「有件事你该知道」，共用尺寸但不给同一个颜色；⚠ 悬停时也**不要**改成强调蓝，那会被读成另一个可点的东西，改成加深自身色。
- **页头的标题与说明必须同属一个子元素**。`.page-head` 是 `space-between` 的 flex 行（左标题右动作），而六个页面的模板结构是「`<div>` 包住 `.page-title` + `.page-desc`」。2026-10-07 给插件页加 `.page-title-row` 时把 `.page-desc` 提到了 `.page-head` 的直接子级，说明立刻被当成右侧动作飘到页面右缘。**判据比的是嵌套深度而不是 `</div>` 个数**——两种错结构的 `</div>` 计数一样，数个数那一版在真实回归面前是绿的。判据里两个坑：按标签配对切块（切到文件尾会把后面几十个兄弟算进栈里），以及**先剥 HTML 注释**（模板注释里会写出字面量 `<div>`，剥晚了配对就平不了）。
- **叠色一律走 token，不许写死 `rgba(255,255,255,…)`**（`--overlay-faint` / `--overlay-soft` / `--overlay-strong`，另有 `--surface-sunken` 给代码块 / 日志内容那类「凹下去」的容器）。同一个视觉意图——把当前底色往目标方向推一点——在暗色下是叠白、浅色下是叠黑，两套主题各给一份值。写死的那 45 处（2026-10-07 清完）在浅色下全部失真：白底上的白 hover 看不见、白底上的白进度条轨道消失。
  - **批量化替换叠色时会连 token 的定义行一起替换掉**，结果是 `--overlay-strong: var(--overlay-strong)` 这种自引用——「有定义」照样通过，变量却彻底失效。所以除了「有没有定义」，还要钉「值必须是字面量」。
  - 判据见 `ui/test/designAlignment.test.js` 的「叠色 token」一组；它比的是剥掉注释后的**声明**，注释里允许出现字面量（那里正是在解释当年为什么写死）。

## 目录约定

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

搬文件时会跟着失效的东西，**2026-10-01 已经全部改成按 basename 认了**：`scripts/lib/shell-source.mjs`（门禁脚本与 UI 测试共用的一份解析器，读壳侧 Rust / 注入脚本源码的那几条测试都走它）、`check-invariants.mjs` 的 `baseName()` / `inFile()`、`check-code-budget.mjs` 的 `isKnownBlob()` / `moduleId()`。`ui/test/*.test.js` 里读**本仓前端**文件的相对路径仍按 `import.meta.url` 解析，本来就不受 Rust 侧目录影响。**新增判据时照这个来**：判据要问「哪个模块」，不是「文件在哪一层」——那一次重组的实测代价是 9 项不变量转红、5 个测试 ENOENT 挂掉、`check-invariants` 自己在启动阶段就崩，红的原因与要检查的东西全都无关。`FILE_BUDGETS` 里登记的**路径**仍要跟着改（那是给人看的），但反棘轮比对按模块标识走。

## 待移除

功能不是永久的。下面是**维护者已经决定到期就删**的东西；到期时按这份清单执行，不要临场重新判断「删到什么程度算干净」。

### 数据迁移（v0.6.0 之后整体移除）

维护者 2026-10-07 决定。过渡期内它是正式功能：**侧栏「系统」组里紧跟「设置」的常驻菜单**（设计稿 2550 行），设置页另留一行轻量入口。

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

- **状态机只有一个主按钮「工作台」**：文案恒为名词，启停方向由 icon（▶ 启动 / ⏸ 停止）与 hover title 表达；同排的「官方对话」用同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」（运行后从下方淡入）与「刷新工作台」是并列的次级入口，**不要再往主按钮旁边加会启停内核的动作**。「刷新工作台」在这排的特殊性：它是这一排里唯一的**动作**而非「把窗口带到台前」，之所以仍归这排而不是主按钮旁边，正因为它只换窗口、不碰内核状态。文案说用户得到什么，代码名说壳到底做了什么，两件事分开记。
- **「查看日志」不在主操作排，在「系统健康 → 日志系统」**（2026-10-07 用户要求统一查看日志体验）。此前它在主操作排，且**与那一行做的事不一样**：按钮直接开独立全屏窗口（跳过列表），行打开日志弹层——同一屏两个日志入口、点开结果还不一致。现在全应用的日志入口统一走弹层（事故 / 预检 / 诊断页本来也都是同一个），要更大屏就在弹层里点「全屏」，那条「直接开窗」的捷径已随之删除。**代价是入口少了一个显眼按钮，所以那一行的右侧提示必须写「查看」而不是 `›`**：它是唯一入口，长成读数样式就等于把入口藏了。
- 「刷新工作台」存在的理由是**看门狗覆盖不到**的那类黑屏：WebView2 渲染进程崩在页面加载完成**之后**，自动链条的判据（Started 之后没等到 Finished）永远不触发。手动路径的额度清账在 `harness_window::reset_budget`（后端），前端只负责发命令。

## 跨前后端的契约

这三条在两侧各有一半，改任一侧都要同时看另一份：

- **禁右键菜单**：壳自己的五个窗口走 `ui/src/shell/noContextMenu.js`（`main.js` 在 `app.mount` 之前调一次），这一半归本文件；工作台与官方对话三个内容 webview 加载的是**别人的**页面，走 Rust 注入的 `no-context-menu.js`，`check-invariants` 第 16 项按「每条 `WebviewUrl::External` 建窗链」逐条查——**另一半见 [src-tauri/AGENTS.md §禁右键菜单](../src-tauri/AGENTS.md)**。
- **预检三态**：`precheck.rs` 的 `Verdict::as_str()` 与 `PrecheckDialog.vue` 的判定表靠字符串对齐，有测试钉死（`precheck::tests::verdict_strings_match_the_ui_contract`），改一边不改另一边会把"未通过"画成"通过"。判定为什么必须三态、基线为什么必须先跑，见 [src-tauri/AGENTS.md §预检](../src-tauri/AGENTS.md)。
- **「今日用量」只有一套口径**：概览卡片与独立「模型用量」窗口都读后端的 `today_tokens` / `today_requests`（`usage.js::todayUsage`）。窗口此前取 `sliceDays(days, 1)` 的最后一天，而后端会把晚于今天的异常日期追加到序列末尾——同一份统计会显示成两个数。`check-invariants` 第 ⑦ 项之五钉住接线。

## 图标

- **两套图标库并存，2026-10-03 起**：主体仍是 `@element-plus/icons-vue`（已接的图标不动，避免一次换掉几百处），**新补的图标走 `@lucide/vue`**（Lucide，ISC 授权）。选它的原因是 EP 的图标集里「标签 / 版本」这类语义只有实心造型的 `PriceTag`，缩到 11px 糊成一个黑点；Lucide 是 24 网格 2px 描边，缩到 11px 轮廓仍读得出。**按图标名 tree-shake**（`import { Tag as TagIcon } from '@lucide/vue'`），不要 `import * as`，否则 1857 枚全进包。
  - 一律**别名导入**（`Tag as TagIcon`）：面板里同时存在 `el-tag` 组件与一排 `*.tag` 类名，光看 `<Tag />` 认不出说的是哪一个。grep `TagIcon` 即为 Lucide 图标的全部用法。
  - 尺寸走**组件的 `size` 属性**，不要用 CSS 的 `font-size`：Lucide 渲染的是带 `width`/`height` 属性的 `<svg>`，字体缩不动它。颜色靠 `stroke="currentColor"` 自动继承，不用显式设。
  - 「版本号徽标」只有一处实现：`shell/VersionBadge.vue`——分段式（左「蓝底 + 深色 tag 图标」、右「暗底 + muted 文字」）的胶囊，**侧栏品牌区与概览「当前内核」卡共用它**。同一语义不许两处各拼一个长得像的，也别把这条样式抄进任一侧的 scoped 块或 theme.css（后者是反棘轮文件，只许越来越小）。
- **面板里的第三方标志（npm 等）**：必须是 `ui/public/` 下的本地矢量、不许写远端 URL，并保留来源与许可声明——`tauri.conf.json` 的 `csp` 是 `null`，远端 `<img>` 出不出网完全取决于用户那台机器，取不到时页面上只剩一块白砖且没有任何报错（版本面板过去就挂着 `avatars.githubusercontent.com` 的 npm 头像）。由 `check-invariants` 第 15 项钉住。图标库里的图形组件不算「第三方标志」——它随包进产物，不发网络请求。
- 应用自身图标只从 `assets/*.svg` 母版生成，规则见 [docs/ui/icon-design.md](../docs/ui/icon-design.md)。
