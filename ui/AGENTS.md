# AGENTS.md — `ui/`（管理面板前端）

根 [AGENTS.md](../AGENTS.md) 的前端分支。根文件里的「范围」「数据目录」「发布」各章对前端同样适用，这里只收改 `ui/**` 之前要知道的约束。后端见 [src-tauri/AGENTS.md](../src-tauri/AGENTS.md)。

技术栈：Vue 3 + Element Plus 单页应用，Vite 构建到 `ui/dist/`（即 `tauri.conf.json` 的 `frontendDist`）。

本文件只写现在该怎么做。逐屏决定的经过、截图与被否掉的方案在
[docs/ui/panel-decisions.md](../docs/ui/panel-decisions.md)（不受指令字节预算限制）。

## 设计系统（uiv2）

设计稿 `docs/ui/dsh-xlink-ui-redesign-draft.html` + 说明
`docs/ui/dsh-xlink-ui-redesign-dev-guide.md` 只表达布局与信息层级，实现以源码行为为准。
不要为了填满卡片去加当前版本没有的状态或数据。设计稿画了而判定不画的（顶部工作条、
内核版本页两列栅格、侧栏 13px 字号、概览卡头实例 caption）见下文。

### 骨架、主题与 token

- 窗口固定 1040×748 不可缩放（`tauri.conf.json`）；侧栏 157px（2026-10-25 收窄自 224）、
  收起态 64px。多数页面是双栏栅格；改窗口尺寸要连带改各面板的 `min-width`。
- 层级三步：canvas（整窗底）、surface（账页段）、chrome（标题栏 / 侧栏）。两套主题共用
  「夜航日志」账页布局：透明段、1px 线、方角，只差配色。
- 明暗判据只有 `html.dark`。`shell/theme.js` 读一次 localStorage 落成这个 class，
  Element Plus 的暗色变量（`theme-chalk/dark/css-vars.css`）因此一并生效。不要另造
  `html[data-theme]`：组件库会留在浅色变量上。theme.css 里 `:root` 浅色、`html.dark`
  暗色，没有第二处判据。会改布局的声明（padding / gap / font-size / 圆角）只写一次，
  不许再放进 `html.dark`，否则切主题整页重排；判据 `darkThemeB.test.js`。
- token 一律语义名：`--surface` / `--surface-raised` / `--surface-subtle` /
  `--surface-sunken` / `--surface-loading` / `--border` / `--border-soft` /
  `--divider-strong` / `--text` / `--text-secondary` / `--text-muted` / `--accent` /
  `--accent-strong` / `--accent-soft` / `--accent-fill` / `--accent-line` /
  `--success` / `--success-strong` / `--success-fill` / `--warning` /
  `--warning-fill` / `--danger` / `--danger-strong` / `--danger-fill` / `--builtin` /
  `--builtin-fill` / `--overlay-faint` / `--overlay-soft` / `--overlay-strong`。旧名
  （`--card` / `--bg` / `--muted` / `--good` / `--bad` / `--warn` / `--surface-soft` /
  `--desc`）不要再写回来。
  - 颜色 token 两套主题各一份；`--fs-*` / `--action-h` 只在 `:root` 定义一次，字号与
    盒高在明暗下必须一样。
  - `var(--旧名, 兜底值)` 是最阴的形态：token 改名后兜底值让声明看着还在（CSS 不报错），
    元素只是退回近白，次要文字在浅底上消失。
  - 批量替换颜色会连 token 定义行一起换掉，出现 `--overlay-strong: var(--overlay-strong)`
    这种自引用；判据除「有没有定义」还要钉「值是字面量」。
- 遮罩两种语义，不许合并：`--el-mask-color` 是全屏遮罩（`el-dialog` / `el-message-box`、
  壳的 `.progress-overlay`），`--surface-loading` 是内嵌加载遮罩（`v-loading` 的
  `.el-loading-mask`）。EP 把两者绑在同一个变量上，只调浅色档会让浅色下内嵌加载区变深灰。
  `.el-loading-mask` 规则必须写在 theme.css 全局作用域（运行期插入的节点没有 `data-v-*`）。
  判据 `ui/test/epStyles.test.js`。
- 叠色一律走 token，不许写死 `rgba(255,255,255,…)`：同一意图在暗色是叠白、浅色是叠黑。
  accent 的半透明档同理（`--accent-fill` / `--accent-line`）；那组 alpha 按深色 accent
  调的，浅色 accent（`#1b365d`）套同一 alpha 会糊成蓝雾。日志阅读器里几处写死色同样
  处理，`.log-tab` 未激活态的 `rgba(232,236,247,.72)` 在浅色下几乎看不见。判据
  `designAlignment.test.js`（叠色 token）与 `logWindowTheme.test.js`。
- 浮层底色只有一个来源：`.progress-body` / `.el-dialog` / `.debug-panel` /
  `.render-error-fallback` 统一读 `--surface-raised`（与 EP 的 `--el-bg-color-overlay`
  同值）。`--el-dialog-bg-color` 必须声明在 `.el-dialog` 自身：库在组件规则里写了同名
  属性，元素身上的声明赢过从 `html` 继承的值。遮罩统一读 `:root` 的 `--el-mask-color`。
  判据 `designAlignment.test.js`「弹窗与浮层的主题适配」。
- 彩色按钮与徽章的文字色走语义 token，不许写死为深色底调的淡彩（Tailwind `emerald-300`
  / `red-400` 这类）：它们在白卡上只有 1.5–2.8:1，暗色下却看着正常，只跑暗色截图抓不到。
  `.btn-chat` → `--success`、`.btn-danger` → `--danger`，hover 用 `-strong` 档；
  `--success-fill` 是补齐既有三档，刻意不补 `--success-line`。`.el-icon` 不单列选择器
  （EP 是 `--color: inherit`）。判据 `designAlignment.test.js`「彩色按钮」。
- 「刻蚀线」只留给独立浮层与概览指标：2px、两端收细、`--divider-strong`（`.debug-panel
  header`、`OverviewPanel.vue` 两格指标线）。主内容区卡头线是 1px 账页线（`.main-view
  .card-head` / `.card > h2` / `.card-head-toggle`，读 `--border`），两套主题共用。
  收细只能用 `clip-path` 六边形：渐变（含 `mask-image`）改的是透明度，两端是淡到看不见；
  原生 border 没法 clip。`--divider-strong` 暗 `#465a50`、浅 `#6f695d`，判据算对比度。
- 排版与控件档位：

  | 档位 | token | 值 |
  | --- | --- | --- |
  | 功能块标题 | `--fs-block-heading` | 15px |
  | 分区 / 卡内小标题 | `--fs-subtitle` | 15px |
  | 页头副标题 | `--fs-page-desc` | 10.5px |
  | 标题旁的 ⓘ | `--fs-tip-icon` | 16px |
  | 动作按钮 | `--action-h` / `--fs-action` | 30px / 13.5px |

  - **字体：显示文字一律跟系统走**（2026-11-02 用户报「2K 屏文字发虚」后定）。
    正文用 body 上那条栈（Segoe UI / 微软雅黑、PingFang SC…）；`.card h2` /
    `.page-title` / `.diag-card__title` 在 Windows 上由 `body.platform-windows`
    覆盖成 `inherit`——`--font-serif` 在 Windows 一个字体都不存在，中文落到
    SimSun，15px / 31px 走轮廓缩放再叠 ClearType 就是毛边。品牌名仍走衬线
    （只有拉丁字母，Georgia 渲染正常）。参照 Codex 桌面端的「字体：系统」，
    **不内嵌第三方字体**：多几 MB 安装包，还得自己扛授权与字形更新。
    判据 `designAlignment.test.js`「Windows 上中文标题不走宋体」。

  - 共用规则扩到 `.card-head` / `.install-row >` / `.community-browse-row >` /
    `.section-divider`；`.entity-action` 是列表行里的紧凑动作（30px），不在此列。
  - `.install-row` 上加 `--el-component-size: 30px`，输入框与按钮一起压；不需要覆写
    `.el-input__wrapper` 内部结构。
  - 卡头标题一律用 `<h2>`；`<span class="card-title">` 在本仓没有对应规则。
  - 概览页标题与状态行是竖向块：版本号 20px，状态行跨两列落第二行。
- 页头副标题是独立档位（`--fs-page-desc`），不要并进 `--fs-desc`：后者 13px 是独立成段
  的说明文字，放同行会过重。

### 页面原语

原语在 theme.css 的「新版页面原语」一节：`.page-head`（页头：左标题右动作）、
`.page-head__text`（包住标题与说明的那一层）、`.page-title-row`、`.head-tip-icon`
（含 `--warning` 修饰）、`.page-layout`（双栏栅格，`__col` 一列、`__full` 整行）、
`.page-list` / `.page-row`、`.btn`（`--primary` / `--secondary` / `--ghost` /
`--danger` / `--icon`）、`.nav-item`、`.plugin-tab`、`.section-divider`、`install-row`。
先找现成原语，再写 scoped 样式。

- 登记进这份清单的类名必须真有模板在用：加一条前先 grep 模板。设计稿里有不等于实现
  里该有（`.page-card` / `.health-row` 都这样删过）。
- `.metrics` / `.metric` 归 `OverviewPanel.vue` 的 scoped 块，不在全局原语清单里。
- 侧栏展开与收起都用图标认项：`.nav-item` 11.5px / 行高 34px / `gap` 10px、图标 17px、
  分组标题 8.5px；品牌主名宋体 14px/600、副名 12px；`.nav-item__badge` 12px / 20px 圆点、
  `.sidebar__footer` 12px。
  - 鲸鱼图标 42px。主名固定两行、不许省略：文字块只有 72px，`--font-serif` 在 Windows
    落到 Georgia，600/15px 的「DeepSeek」约 80.7px 放不下，`overflow-wrap: anywhere`
    会在单词内部断成「DeepSee / k / Harness」。现在主名 14px + `white-space: nowrap`，
    `Harness` 是 `display: block` 的 12px 子元素（`.brand__name__sub`）；不许再声明
    `text-overflow`，判据反向钉住。
  - `<img>` 的 `width` / `height` 属性必须与 CSS 相等（`SideBar.vue` 与 theme.css
    都是 42）：CSS 只覆盖绘制，属性是读屏与布局的第一手尺寸。`flex: 0 0 42px` 也不能少，
    否则图标会被文字挤扁且不报错。
- 侧栏底部工具区是两行：第一行三枚方钮（收起 + 刷新 + 主题）左对齐贴左缘
  （`.sidebar__footer-actions` 的 `justify-content: flex-start`），第二行「桌面端」标签
  + 版本号（`.sidebar__footer-meta`）。版本号不许回到控件同一行：标签 + 三枚 26px 方钮
  + 4 道缝占掉 ~142px，只给版本号留 ~78px，dev 后缀必被截。dev 构建隐藏「桌面端」标签
  （`v-if="!store.devUi"`），「（dev）」后缀已表明身份。收起态必须竖排（写死
  `flex-direction: column`，不靠 `flex-wrap`）：三枚控件横排要 92px，内容盒只有 48px；
  第二行整行隐藏（`.sidebar__footer-meta { display: none }`）。判据 `designAlignment.test.js`。
- 构建标识（dev 鲸眼红 / release Gitea 绿）收成窗顶 2px 细线，由 `--build-color` 驱动，
  两种构建共用 `.app-shell::before`，只换颜色变量。判据 `diskUsageAndChrome.test.js`
  钉住只有这一处画法。
- 宽表格要给数字列显式列宽（`table-layout: fixed` + `nowrap`），并按内容量分别给值：
  自动布局会把表头竖排成「文件 / 数」、把 `896.7 KiB` 拆成两行。固定布局后列宽不再随
  内容生长，宽度变成承重的，给窄了文字直接溢出。路径类长文本列不要 `nowrap`，让它们
  折行。判据要抽 `nth-child(n)` 那条规则里的 `width` 再比值，不要比两段起点不同的
  CSS 子串。

### 卡片与页头

- 主内容区的卡是**账页段**：透明底、上下 14px 内边距、底部 1px 账页线（`.main-view .card`，
  theme.css 尾段）。卡头与内容之间的线是 1px `--border` 伪元素（`.main-view .card-head` /
  `.card > h2` / `.card-head-toggle`），两套主题共用；留白面板卡 10px、控制塔 9px
  （判据 `ui/test/cardChrome.test.js`）。
  - 2px 收细刻蚀线只剩独立浮层与概览指标（`.debug-panel header`、`OverviewPanel.vue`
    两格指标线）；线色只有 `--border`，别给账页卡另开更深 token。
  - 「线到内容」的距离交给各卡片自己的机制（`.card` 靠上下 14px 内边距，`.diag-card`
    靠标题 `margin-bottom`）。
  - 覆盖概览页与四个面板里的卡，不含 5 个独立诊断窗口（它们在 `.diagnosis` 容器里，
    另一套布局）。控制塔 `--tower` 显式写 `margin-bottom: 6px`，基线 8px 不动。
  - 别用「某条规则写了 `border-bottom`」当判据，要查层叠后的生效值；同特异度后一条盖掉
    前一条是静默的（做法见 `ui/test/panelSpacing.test.js`）。
- 页头的标题与说明必须同属一个子元素：`.page-head` 是 `space-between` 的 flex 行，六个
  页面都用 `<div>` 包住 `.page-title` + `.page-desc`。把说明提到直接子级，它会被当成右侧
  动作飘到页面右缘。判据比嵌套深度，不比 `</div>` 个数。
- 页头主副标题同行，由 `.page-head__text` 承载，那一层不能删（上面判据量的就是它）。
  - `align-items: baseline`：`center` 会让 10.5px 落在 31px 行盒正中，基线不齐，句号浮上去。
  - `.page-title` 必须 `flex: none`，否则副标题一长先被压扁的是标题。
  - `.page-desc` 下外边距清零，同行由 `gap: 14px` 承担。
  - 判据在 `designAlignment.test.js`「页头主副标题同行」，样式与模板一起钉；主标题读
    `--fs-page-title`（31px / 宋体 / 600），副标题读 `--fs-page-desc`。
- 提示与它的动作按钮不许是兄弟节点（概览 `el-alert` 与「更新并重启」同属
  `.update-banner`）；`.el-alert` 要 `flex: 1; min-width: 0`，否则文字一长先挤掉按钮。
- 弹窗 / 浮层底部操作排靠右，落在共享类 `.dialog-actions`（theme.css，`.btn-row` 旁）。
  - `el-dialog` 的 `#footer` 本来就右对齐，不必再加；`MigrationPrompt` 的
    `.prompt-footer-ask` 按钮包在 `div` 里，块级对齐管不到，保持原样。
  - 不要改 `.btn-row` 本身：欢迎 callout、设置页通知行里它与说明文字同属一段。判据钉住
    它没有 `justify-content` 且仍 `wrap`。
  - 事故面板里「隔离 / 移除」跟着嫌疑插件对象走（`.suspect-actions` 不带 `dialog-actions`）。
- 补充 / 警告一律收成标题旁图标 tooltip，不做常驻整宽条。触发元素必须可聚焦
  （`tabindex="0"` + `aria-label`），`content` 与可访问名取同一常量。ⓘ 是补充说明、
  ⚠ 是「有件事你该知道」；⚠ 悬停不改强调蓝。
- 页头说明与开关状态说明收进 ⓘ tooltip：删掉常驻 `.page-desc` 与开关旁的可见说明。
- 分组标题右侧辅助信息小一档（`.section-divider__note` 12px）。字号不许加进 `.muted`：
  那个类只管颜色，全仓 23 处在用。
- tooltip 浮层限宽换行，选择器钉 `[role='tooltip']`。
  - EP 的 `.el-popper` 只有 `min-width: 10px`、没有 max-width；本仓最长提示约 140 字
    ≈ 1680px，窗口只有 1040px。
  - 不给裸 `.el-popper` 加宽度上限：2 个 `el-select` 与 3 个 `el-dropdown` 共用同一类，
    菜单应按内容取宽。`[role='tooltip']` 是唯一区分点（select 传 `listbox`、dropdown 传
    `menu`，`el-popconfirm` 内部也是 ElTooltip）。
  - 一条全局规则胜过给 36 个 `el-tooltip` 逐个挂 `popper-class`，漏挂的恰好是最长那条。
  - 用 `overflow-wrap: anywhere` 而非 `break-word`：前者参与 min-content 计算。
  - 判据：规则存在且 `min-width: 0` / `max-width: min(420px, calc(100vw - 32px))` /
    `white-space: normal` / `overflow-wrap: anywhere` 齐全；裸 `.el-popper` 无 max-width；
    EP 三个 role 默认值（读依赖源码，验不了就 skip）。

### 按钮与无障碍

- 无文字按钮（icon-only）走 `icon-btn`，26×26（theme.css 全局规则）。EP 默认值不能用：
  `el-button` 带 `padding: 8px 15px` + `height: 32px`，`is-text` 只换背景，14px 图标被
  撑成约 44×32。命中区靠 `::after` 补到 32×32 居中：相邻两个重叠 4px，两侧各让 2px，
  仍在 Windows SMALL_CAPTION 的 24px 下限之上。
- `title` 与 `aria-label` 都要有，缺一判据就红（`check-invariants` 的
  `a11y-icon-button`）。`el-tooltip` 只认鼠标，键盘与读屏用户看不到；`title` 对键盘
  生效，`aria-label` 是读屏的可访问名。判据按「自闭合 `el-button`（插槽为空）且有
  `:icon`」判定，不只看 `circle`。
- 同一状态把条件写进选择器：`.btn-chat` / `.btn-danger` 用 `:not(:active):hover` /
  `:not(:active):focus-visible`，不再抄 `:active { color: 静止色 }`。
- `.btn-row` 的次级按钮量的是余量：三枚 `.btn-sub` 内边距 11 → 9、字号 12 → 11.5px、
  图标 17 → 13px。图标那条必须比 `.btn-row .el-button .el-icon` 更晚，两者同为 (0,3,0)
  只能靠源码顺序决胜，判据钉先后。量余量要量版本号的文字宽度，不能量它那一格：`1fr`
  会把余量吃掉。
- 「一行放不下」有两种病：余量为负是排布错了，该换排布；余量只剩几像素是碰运气，该指定
  谁让位。缺的部分由 `.settings-port-row { flex-wrap: nowrap }` + 输入框
  `flex: 0 1 132px; min-width: 96px` 吸收；只有 `nowrap` 会横向溢出并被 `main` 的
  `overflow-x: hidden` 裁掉，只有可收缩仍然折行。判据钉机制，不钉宽度数字。
- 同一屏里同一个东西只能有一个名字，判据看「叫什么」而不是「长什么样」。
  - 内核版本页卡头叫「已安装 / N 个版本」而不是「内核版本」（页面标题已叫内核版本），
    组内 `<h3>已安装</h3>` 一并去掉。发布列表里的行内 `<el-tag>已安装</el-tag>` 是另一
    含义（标出远端版本本机已有）。
  - 插件页外层卡叫「插件管理」，没有「插件中心」这个界面名。内核版本页卡片标题叫
    「官方版本」而不是「npm 发布」：npm 只是取源渠道。
  - 提示图标的可见文字必须和它 tooltip 的内容是同一件事。
- 动作按钮跟着作用对象走：插件页安装按钮只作用于插件库，必须留在「安装插件」区块里
  （紧接内置插件下方），不挂到卡头。元素搬走后旧位置的规则必须删干净；判据按类数出现
  次数。

### 窗口 chrome

- 「入口还在」不等于「文件还在」：`WINDOW_ENTRY_POINTS` 同时钉「文件存在」与「自身目录
  之外还有挂载点」。扫引用带 `.js`（挂载点在 `main.js`）；同目录引用不算入口。
- 四扇壳自有窗口（主壳 + 日志 / 模型用量 / 套餐用量）建窗期一律 `decorations(false) +
  shadow(false)`，副窗另加不可最小化；透明与圆角按平台分叉：
  - macOS：`transparent(true)` + CSS 圆角。`transparent` 只做 `setOpaque(false)` +
    `setBackgroundColor(clearColor)`，不碰 `NSWindowStyleMask`；只有 `set_decorations`
    会重算样式位、抹掉 `Miniaturizable`（tao 0.35 实测）。
  - Windows：`transparent(false)` + 不透明底色。透明窗口会关掉 ClearType，只剩灰度抗
    锯齿；Win10 没有系统圆角，所以 `body.platform-windows` 把 `--window-radius` 归零。
    主窗由 `tauri.windows.conf.json` 覆盖 `transparent` / `backgroundColor`（平台配置
    整体替换数组，其余项必须与 `tauri.conf.json` 逐项一致），三扇副窗由
    `decorate_transparent` 的 `#[cfg(windows)]` 分支给。
  - `shadow: false` 不能省：透明 + 圆角后系统投影会落在圆角外的透明区，读作一圈黑框；
    投影改由 `#app::after` 的 `box-shadow` 补。底色也必须跟着圆角从 `body` 搬到 `#app`，
    否则圆角外四角仍是实色方块，且没有任何报错。
  - 三扇副窗共用 `shell/ViewerShell.vue`（自绘标题栏 + 圆角 + 裁切 + 投影），能力文件
    有 `allow-start-dragging`。
- 四扇自绘窗口在 `body.custom-titlebar-shell` 上写
  `clip-path: inset(0 round var(--window-radius))`：`#app` 的圆角与 `overflow: hidden`
  管不到固定定位投影和 teleport 到 body 的遮罩，窗口外四角必须全透明。Windows 上圆角
  为 0，这条退化成矩形裁切，仍需要。官网页签栏与工作台保留原生窗口轮廓。
- Windows 的窗口底边线抬 1px：175% 缩放下 WebView2 渲染视口比客户区高约 1 CSS px，贴底
  的 1px 线整段被裁掉。规则是
  `body.platform-windows.custom-titlebar-shell #app::after { bottom: 1px; }`，只限
  Windows（macOS 没这段裁切，抬了底边会浮动）。判据 `windowChrome.test.js`。
- 自绘交通灯按 macOS 实测：直径 12px、中心间距 23px、灯组左缘距窗缘 7px。
  - 描边用 `box-shadow: inset`：全局 `box-sizing: border-box` 下 `border: 1px` 会从
    12px 里各吃掉一侧，着色区只剩 10px。
  - hover 是整组浮现、底色不变、符号分色：单颗 hover 会让另外两颗读成不可点，命中区靠
    `.mac-titlebar__controls` 的 padding 撑开；原生不做 `brightness`，判据显式禁掉；三枚
    符号红 `#460804` / 黄 `#90591d` / 绿 `#2a6218`，统一深灰会在黄、绿上偏紫。
  - 判据钉关系不钉字面量（`width + gap === 23`），并禁止退回 `border: 1px` /
    `border-color`；灯组左缘 7px 带 ±1px 不确定度，故意不钉。
  - 交通灯带 `v-if="isMacTitlebar"`，Windows 用 `.win-caption` 右侧按钮；判据两头都钉。
- 副窗只留关闭按钮，最小化是主壳独占的动作。Windows 上副窗调 `minimize_shell` 只会得到
  `Command minimize_shell not allowed by ACL`，绿点（禁用、不可缩放）一并去掉。
  - `WindowTitleBar` 的 `close-only` 是能力开关不是样式开关：`ViewerShell` 传它 →
    macOS 只画红灯、Windows 只画 ✕；`minimizeWindow()` 里另有 `if (props.closeOnly)
    return`，模板日后加回按钮命令也发不出去。关闭键必须还在。
  - 关闭键悬浮说明分档：主壳写「收进后台 / 收纳到通知区域」，副窗只写「关闭窗口」。
  - 系统层那一半在 Rust（`decorate_transparent` 的 `minimizable(false)`），前端只管自绘
    chrome；三份副窗 capability 的 `core:window:allow-minimize` 同时删掉。判据
    `windowChrome.test.js`「副窗标题栏只留关闭按钮」：接线、平台门控、
    `minimizeWindow()` 提前返回、Rust 的 `minimizable(false)`、capability 不许有 minimize。
- 副窗标题栏显示「功能名@应用名」，主壳只显示应用名：`ViewerShell` 传功能名，日志窗跟随
  文件名（`:title="activeName || '日志'"`，兜底不能省，否则标题退成 `@Dsh-Xlink`）。
  分隔符与判据见[窗口标题](../docs/architecture/window-architecture.md#标题一个名字三处副本)。
- 副窗原生标题栏跟应用主题走：原生 chrome 由 `NSWindow.appearance` 画，主题真值在
  localStorage，Rust 建窗时读不到，所以建窗一律深色兜底，挂载后由 `applyTheme()` 把
  `theme.value` 推给 `bridge.setWindowTheme()` 纠正本窗口；4 个壳自有 capability 补齐
  `set_window_appearance`。
  - `window.setTheme()` 收 `theme.value` 本身，写死字面量会让「切了主题只换页面、不换
    标题栏」。
  - 只改本窗口的原生装饰，不批量改（判据禁掉 `getAll` / `webview_windows` / `forEach`）。
  - 已开着的副窗靠广播：`storage` 事件不跨 webview。`setTheme` 落地后
    `broadcastTheme(next)` 发 `ui-theme-changed`，各窗在 `main.js` 里
    `followThemeBroadcast()` 订阅并自己跑 `applyTheme()`。订阅必须在 `createApp` 之前；
    副窗不调 `setTheme`（会互相触发）；事件值过 `THEMES.includes`。
  - 副窗不许只调 `setWindowTheme`，那只换标题栏、留下旧内容。
  - 工作台不钉建窗主题（见根 `AGENTS.md`），外观由 `src-tauri/src/harness/workbench-appearance.js`
    决定：内核 `ui-theme` 固定档听内核，内核选 `system` 时跟壳上报的偏好（`report_shell_theme`，
    `system` 传 `null` 解除覆盖）；官网页签窗装别人的站点，钉死深色，且
    `if (isChatStrip) setThemeValue('dark')` 与 `followThemeBroadcast()` 一起排除。
    给 `official-chat-strip.json` 补授权即转红。
  - 建窗深色是兜底不是最终外观：挂载到纠正之间会闪一下，彻底消掉需 Rust 也读得到主题
    真值，本轮不做。
- 外观在设置页右栏三项（浅色 / 深色 / 跟随系统），与侧栏循环按钮共用 `theme.js`：
  `theme` 保存模式，`resolvedTheme` 解析明暗写进 `html.dark`；固定模式写原生装饰，system
  传 null 解除覆盖；侧栏按钮按 `themeOptions` 循环，图标表示当前模式，悬停说明当前模式与
  下一次点击目标。macOS 的 `setTheme` 与建窗 `.theme(Some(...))` 会覆盖整个应用，不能用于
  窗口主题：建窗统一走 `shell::appearance::initial_theme(label)`，原生外观经
  `set_window_appearance` 只设当前 `NSWindow`（工作台那扇反过来，壳不许钉它）。Windows
  继续用 Tauri 的窗口主题实现。

### 逐屏决定

> 只留规则；各屏的经过、日期、截图与被否掉的方案在
> [docs/ui/panel-decisions.md](../docs/ui/panel-decisions.md)（2026-10-25 为字节预算搬去，
> 本文件曾越过运行时 65536 字节上限、尾部被静默截掉）。

- 技能页是「已安装 / 社区资源」两张卡；设计稿的「技能状态」卡不画（没有后端字段支撑）。
  - `installSkill` 走 `withProgress` 不经过 `withLoading`，按钮上挂
    `isLoading('installSkill')` 永远为 false，是假 loading。在途状态由进度浮层表达，
    按钮只按 `globalBusy || 输入为空` 禁用。
  - 单页样式留在 scoped。输入行复用 `.install-row`，卡内标题与说明合成
    `.community-title` / `.community-meta`；技能社区与手动安装之间只画一道 2px 刻蚀线
    （`.community-install::before`），两侧边框显式清零。
- 设置页两栏各三张：左栏工作台 / 后台常驻 / Defender 排除项，右栏外观 / 任务通知 /
  环境回退与诊断。卡内三个 `.page-list-row`：环境回退点 / 深入排查 / 启动诊断。
  - `SnapshotCard` 与 `BisectPanel` 渲染成 Vue Fragment，套进共享卡就是卡中卡。判据查
    形状（整页只有一张卡 / 没有两列栅格），不查类名。
  - 收起的只有明细，状态不许藏：`headline(view)` / `bisectHeadline(view)` 挂在
    `.page-list-meta` 常驻，`drifted` 与 `conclusion` 两条告警不进闸门。
  - `.page-list-meta` 不抄设计稿的 `nowrap` + 省略号：次行是整句说明，截成一行会分不出
    「还没成功启动过」和「文档损坏」。
  - 收尾分隔线收在 `.page-list > *:last-child` 上，不是 `.page-list-row:last-child`：
    Fragment 展开后明细也是列表直接子级。
  - 「启动诊断」是补的入口，直接打开诊断窗口，不是展开层。
- 内核版本页维持「一张卡竖排」，不改设计稿的两列栅格；判据钉「整页只有一张卡、没有两列
  栅格」。两栏之间的竖线已删除（账页皮肤统一后不画）：左栏非空时按内容高度收
  （`align-self: start`，空态例外，选择器带
  `:not(:has(> .installed-list > .el-empty))`），不许用 `border-left` 补回来。
- 顶部工作条（`.workspace-bar`）已整条删除。`instance.js` 的 `setDefaultInstance` 不许
  跟着删：它是没有调用方的 API，判据钉「模板侧零调用 + 函数与 `selectionRevision` 仍在」。
- 概览「当前内核」卡主操作上移、实例 caption 删除：卡头只有 `<h2 class="kernel-title">`
  与 `.kernel-header-actions` 两栏；`kernelCaption` 与概览页对实例注册表的引用一并移除，
  `App.vue` 仍在启动时加载实例列表，插件页不受影响。
- 内置插件单独成块，紧接安装入口上方：每行仍是 `.entity-row--single`，名称 / chip / 状态
  复用社区插件那一套，外层包在自己的 `.entity-list` 里。
  - 差别只在右侧动作：没有安装 / 更新 / 卸载，保留启停开关，并提供 ChatGPT 登录 / 取消 /
    刷新模型列表 / 退出登录；停用不退出账号。
  - `.builtin-*` 样式必须删干净：元素并进列表后它们没有作用方。
  - 社区清单空态只看社区行（`communityListEmpty`），内置行不参与判断。
  - 判据按顺序：内置块头 → 内置清单 → 安装区块 → 社区清单。
  - 那几行是 `v-for`，文案与能力都来自后端：`builtin_status` 一次返回全部，显示名 / ⓘ /
    账户卡由 `BuiltinSpec` 下发，前端不留字面量。
    - 「按 id 取」不许写成「取第一项」：某个内置插件排到前面时会静默拿到别人的状态，判据
      在 `builtinPlugins.test.js`（它把 ponytail 摆在 openai 前面）。
    - loading key 按插件分（`builtinToggle:<id>`），共用一个会让另一个的开关也转。
  - 内置插件来源 chip 是放大一档的紫色：`.origin-chip-builtin` 叠在 `.origin-chip` 之上，
    13px + 图标 15px + `--builtin` 文字 / `--builtin-fill` 底 / 同色描边；npm 与 GitHub
    仍走中性灰。
    - 挂在那枚 chip 上而不是某个插件 id 上：chip 在 `v-for` 里，后端加一项就自动带紫色。
    - 色值走 `--builtin` / `--builtin-fill` 且两套主题各一份，只定义一套时另一套下整条失效。
    - 判据：修饰类全模板只出现一次、颜色读 token、字号关系、两个 token 两套主题都定义。
- 末行两卡吃满剩余高度、块内文字换行：`overview-grid` 写
  `grid-template-rows: auto auto minmax(min-content, 1fr)` + `flex: 1`，祖先 `.panel` 给
  `min-height: 100%`；第二行今日模型用量，末行「最近操作 | 需要关注」。用 `min-content`
  起底而不是 `0`（后者压扁卡、内容溢出）。`.diag-row__label` 用 `overflow-wrap: anywhere`。
- 行尾动作是带边框、带 icon 的徽章（`.diag-row__cta`），不是纯文字箭头；语气跟同行读数
  走 `--danger-*` / `--warning-*`，悬停加深自身色，不换强调蓝（accent 表示主操作）。展开
  箭头与动作按钮不共用类名（`.diag-row__caret`），判据禁止 `.diag-row__arrow` 回来。
- 系统健康 / 最近操作 / 需要关注三张卡的卡头跟概览走：块标题 `--fs-block-heading`（15px），
  `padding-bottom` 保持 9px。`--bare` 用 `:not()` 正着写：线从 border 换成伪元素后，只清
  `border-bottom: 0` 仍会画出线。
- 「最近操作」四个类型词是可点页签：各切到该类最近一条（数据来自已加载的 `recentRuns`，
  不发请求），默认跟随全类型最近一条；空类压暗并给正文说明。页签是 `<button role="tab">`
  + `aria-selected`，样式走 `.run-tab`。
- 概览主栅格 `1.25fr / 0.75fr`；第二行整宽今日模型用量，末行「最近操作（左） | 需要关注
  （右）」。用量分析页套餐卡固定三列；额度行「名称 22px | 条 1fr | 百分比 auto」，条高
  6px、百分比在条外；余额总额 14px 粗体、赠金充值第二行 10px。判据 `designAlignment.test.js`。

### 概览页

- 状态机只有一个主按钮「工作台」：文案恒为名词，启停方向由 icon（▶ / ⏸）与 hover title
  表达；「官方对话」同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」（运行后
  淡入）是并列次级入口，不要往主按钮旁加会启停内核的动作。
  - 「刷新工作台」已隐藏（这一排唯一动作：换掉整个工作台窗口 = 换掉渲染进程）。入口隐藏
    ≠ 能力删除：`store.js` 的 `forceReloadHarnessWindow` 与 Rust 的
    `harness_force_reload` / `reset_budget` 原样留着，按 `setDefaultInstance` 先例——
    没有调用方的 API 不许当死代码清掉。手动出路：关掉工作台窗口，再点「工作台窗口」
    重新打开。判据 `designAlignment.test.js`。
- 概览只保留一张「今日模型用量」卡：整宽第二行，卡头右侧是今日总 tokens 与请求次数，
  正文 Top 3 模型（序号 / 模型名 / 占比条 / tokens / 占比）；内核卡只剩 Node.js 与数据
  目录两格。
  - 数据只走后端同一次日期命中的 `today_tokens` / `today_requests` / `today_models`
    （`usage.js::todayUsage` / `todayTopModels`），不许前端按 `days` 找「最后一天」。
  - 套餐卡与「模型用量」按钮在「用量分析」页（内嵌 `UsageWindow`）；组件 / 命令 /
    capability 与窗口路由保留。
- 次级入口落在版本号那一行右侧、靠右对齐，落位是两行网格（`kernel-summary-main`：
  `minmax(0,1fr) auto`），不是 flex + `margin-left: auto`；`justify-self: end` 不能省。
  `.btn-row-sub` 的左缩进与 2px 竖线随搬家删除。判据查栅格落位（状态行跨两列、落第二行），
  别再按「版本号与状态行同属竖向块」找它。
- 「查看日志」在「系统健康 → 日志系统」，点开独立窗口。
  - 这一格调 `logs.js::openLogWindow()`（不带文件名，窗口自己挑：先 `kernel` 分组，再
    任意第一份；后端 `open_log_window` 接受空名，`read_log_file` 仍拒绝）。
  - 带具体某一份证据的入口（事故 / 预检 / 诊断）仍走弹层。
  - 两个入口共用一份实现，判据 `logEntryPoint.test.js`。右侧提示必须写「查看」而不是
    `›`；一份日志都没有时该格不可点。弹层里的「全屏」按钮不能删。
- 一键重建工作台针对看门狗覆盖不到的黑屏，额度清账在 `harness_window::reset_budget`；
  入口已隐藏。
- 版本号是纯文字：20px / 700 / `var(--text)`；去掉徽标后 `margin-left: 0` 必须显式钉住。
  判据含「概览不再引 VersionBadge」。
- 指标格的动作按钮配色归 CSS，模板不挂 `type="primary"`；图标槽按 `.button-icon` 取
  16px（EP `.el-icon` 是 `font-size: inherit`，按钮 11px 会把图标也缩到 11px）。指标标题
  11px muted、值 12px 600 按值钉住。
- 「数据目录」那一格指向应用根 `~/.dsh-xlink`：实例目录在 141px 里被 `display_short` 截成
  `~/.dsh-xlink/dsh/…`，尾巴的省略号像坏路径。显示读 `KernelStatus.xlink_home`，实例
  目录退到 `title`。

## 判据怎么写

写 UI 测试前先读这一节。这些坑每一条都是真回归顶出来的，症状一律是「判据绿着而功能
已经坏了」。

1. **扫模板不扫全文**（`templateOf()`）。本仓模板带大段解释「为什么这么写」的中文注释，
   注释里经常原样引用被判据的词，按整份文件 `indexOf` 会把自己写的解释当成命中。样式那边
   对应 `stripComments`，模板这边要剥 HTML 注释（它只认 `/* */`）。同一条纪律对 JS 注释
   也成立：判「前端不再调 `set_default_instance`」要写成 `invoke('set_default_instance'`
   这种真调用形态。收尾边界取 `<style` 而不是 `<style scoped>`。
2. **读层叠后的生效值，不比原始子串**（`allRulesIncludingVue()`、`effectiveDeclaration`，
   工具在 `ui/test/lib/css-cascade.mjs`）。面板级样式大半写在组件里，只扫 `.css` 一条都
   查不到；`ruleBlocks` 原来不剥注释，注释里只要有 `{` 或换行就一路吃到花括号——
   **表现是「明明写了却查不到」，而且不报错**。
   - 工具分不清的三类选择器（`:deep(...)`、`:has(...)`、状态属性与祖先条件如
     `.sidebar.is-collapsed .x`）改看 scoped 块文本；想把工具改严格会连带改掉 4 条既有
     测试的语义，那是另一件事。
   - **祖先条件那一类还会主动把不相干的规则算进来**：工具只认主语 `.el-icon`、忽略祖先，
     于是更高特异度的 `.btn-row .el-button .el-icon` 被当成答案，**判据因为错误的原因
     而绿**。主语类被多条后代规则复用（`el-icon` / `dot` / `title` / `chip` 这类通用
     尾巴）时，一律读剥注释后的规则文本。传类集合给 `effectiveDeclaration` 要**去掉选择器
     里的点**（`subjectTokens('.progress-body')`），否则它一条都匹配不上、安静地答 `null`。
   - **`effectiveDeclaration` 带伪类维度**（`opts.pseudo`），不给它时会排除带伪类的规则，
     且只认主语**尾部**的伪类——`.a:hover .b` 不算，那是祖先的状态。
3. **判「每一枚都在」要数次数，不是逐个 `assert.match`**。`官网网页版窗口` 里含着
   `工作台窗口` 这个子串，三枚里少掉两枚，剩下的仍能让三条断言全部命中。改成数**整枚
   按钮**（`<el-button …>文案</el-button>`）的出现次数并断言 `=== 1`——`match` 判的是
   「有一个」，需求常常是「每一个」。
4. **反向验的变异必须真的命中目标**：有一条变异写成「在 metrics 开标签后再复制一份
   metrics 开标签」，看着像「把按钮搬下去」，实际按钮根本没动。剪贴搬移要用正则把整块
   剪出来再贴到新位置，事后断言「新串读得到 / 旧串已消失」。**绿在没生效的变异上，
   说明不了任何事。**
5. **假门禁比没有门禁更坏**。写判据时踩过的坑：惰性匹配跨过有文字的按钮吞到下一个自闭合
   标签（误报 20 处）、用 `>` 断配对拦不住闭标签（误报 8 处）、用 `</` 断配对又拦不住另一个
   开标签的 `<`（反过来漏报）——现在靠「捕获块内不允许出现任何 `<`」分开。
6. **前提变了要重新确认**：选择器的正确性常常建立在某个默认值上（如 EP 给浮层的 `role`
   默认是 `"tooltip"`）。把它写成一条判据，升级依赖时它先红；验不了就 `t.skip` 并说明，
   **不假装绿**。
7. **判据要钉结构事实而不是字面量**：页头那条比嵌套深度而不是 `</div>` 个数，内核
   版本页两栏竖线那条钉伪元素与收细比例而不是当前几行值，灯组那条钉
   `width + gap === 23`。守住一个会随字号、图标、依赖版本漂移的字面量，等于守一个
   必然作废的巧合。

## 目录约定

插件页单视图（无页签）：内置插件清单 → 安装面板 → 当前内核已安装插件清单。安装面板含
`npm 包名 / Git 地址` 输入 + 「预检并安装 / 安装」（回车与按钮同走 `installFromSpec`），
标题行右侧「同步内核 / 检查更新」只作用于手动安装的插件。不再有远端目录、搜索、分类、
排序、分页或 per-instance 状态 chip。

- **描述性文字一律收进 ⓘ tooltip**（2026-10-31 用户要求）：面板只常驻标题、输入框
  placeholder、状态词与按钮文案；安装格式 / 预检 / 生效时机 / 同步范围 / 内置插件交付方式
  都不铺成整段说明。行内状态只留短词（如「已接线」），解释放 tooltip。触发元素必须
  `tabindex="0"` + `aria-label`；判据见 `designAlignment.test.js`「安装面板的说明收进 ⓘ tooltip」。

`ui/src/` **按功能分目录**，不要往根目录平铺。

```
ui/src/
├── main.js / App.vue / store.js / theme.css   # 入口、壳、全局状态，就这四个留根
├── shell/          # 壳自身：导航、概览、设置 + 跨面板共用基础设施
├── kernel/         # 内核版本与实例
├── plugins/        # 插件安装、补丁、安装预检
├── skills/         # 技能
├── diagnostics/    # 安全网 P0/P1/P2：环境回退点与二分定位
├── usage/          # 用量分析面板、本地 token 统计
├── subscription/   # 云端套餐用量卡与窗口
├── incidents/      # 工作台事故与任务通知
├── logs/           # 日志查看器
└── official-chat/  # 官方对话
```

一条功能的状态、动作与面板**放在同一个目录里**。新增功能先问它属于哪一格；确实跨格的
共享件放 `shell/`。

搬文件时同步更新调用方、测试和文档。源码结构判据优先按模块名定位，避免把路径写死；
前端测试的相对路径按 `import.meta.url` 解析。组件按职责拆分，状态与动作沿用现有 store，
同一语义的样式与异步逻辑优先复用；不以模板行数决定是否拆组件。

## 与 Rust 的边界

- 全局状态在 `store.js`（留根），各功能的状态与动作在**自己那一格**
  （`plugins/plugins.js`、`skills/skills.js`、`logs/logs.js`…），异步样板在
  `shell/async.js`，组件只读状态、调动作。
- 与 Rust 的通信**只允许**走 `bridge.js` 的 `invoke` / `Channel`。组件里不直接碰
  `window.__TAURI__`（注入脚本那一侧除外，且那是 `src-tauri/src/*.js`）。
- 触发 IO 的按钮必须挂 loading（`loading.js` 的 `withLoading(key, …)` +
  `:loading="isLoading(key)"`）；长任务走 `progress.js` 的 `withProgress`。
- 改完跑 `npm run build:ui`。

## 文案

- 用户可见文案用简体中文。面板里每一条面向用户的字符串都是产品的一部分。
- 数据目录不许在前端写死：展示路径的文案读后端返回的真实路径（技能面板读
  `SkillStatus.store_root` / `skills_root`，插件面板读 `PluginStatus.store_root`），
  显示前过 `labels.js` 的 `tildePath` 折叠 home。`check-invariants` 第 ⑦ 项之六钉住：
  前端文案不得出现路径上下文中已搬迁的目录名（`dsh-plugins` / `skills-store`），且目录名
  前要带 `~/` 或 `/`，所以历史逻辑来源 id `skills-store` 不受影响。
- 告警文案自带可执行的下一步，模板不要再统一追加「重启应用会自动修复」：「清单缺失」时
  `reconcile` 不会凭空重建 `store.json`，「被盖住」时重启也不会改变内核 rank 次序；
  一句对两条都不成立的建议比没有更糟。

## 间距

- 功能块之间的默认间距是 12px，由容器统一提供，块自己不再带外边距（`.panel` 的
  `gap: 12px`，含「页头 ↔ 正文」）。除非用户明确要求特调，新 UI 按它写。主内容区两列
  栅格是账页皮肤：`.page-layout` 行缝 0、列缝 34px，`.page-layout__col` 不叠 gap，段与段
  之间由卡片上下 14px 内边距 + 1px 账页线承担。
- `.panel` 里的块一律 `margin-bottom: 0`。有 gap 的容器里块不带 margin；没有 gap 的
  `.diagnosis` 仍由 `.diag-card` 自己的 10px 提供。
- Vue 多根组件会把每个根节点提升成父容器的直接子级，也会吃父容器的 `gap`。归零要显式写
  `margin-bottom: 0`：作用域选择器只赢它声明过的属性，删掉这条会让基线 10px 回来。
- 「看着行距大」先分清 gap 还是行盒。行高由每行自己的盒高决定；面板里的行多是「裸文本
  + EP 组件」，外框远大于文字，先收盒高（`.kv .el-button { height: 20px }` +
  `.kv { row-gap: 4px }`），别动 EP 横向 padding，那在页面上看不见、只负责可点面积。
  验的时候量「首行文字顶 → 末行文字底」的总跨度，不量 `gap` 的属性值。
- 量 CSS 前先确认 dev server 服务的是改后文件：`curl` 到的规则原文必须与磁盘一致，不一致
  换端口重起，别猜 HMR。这个坑只在纯 CSS 改动上出现，`.vue` 走 HMR 正常。
- CSS 注释里不许出现字面的块注释记号：注释不嵌套，第一个收尾符会截断注释，余下文本并进
  下一条规则的选择器、整条被浏览器丢弃。判据扫全仓 CSS 注释（`designAlignment.test.js`）。

## 跨前后端的契约

这几条在两侧各有一半，改任一侧都要同时看另一份：

- 右键策略按窗口族分两份：壳自己的五个窗口走 `ui/src/shell/noContextMenu.js`（`main.js`
  在 `app.mount` 前调一次，一律取消）；官方对话三个内容 webview 加载别人的页面，走 Rust
  注入的 `no-context-menu.js`（一律取消，含页面自绘菜单）；工作台加载内核 Web UI、自己有
  右键功能，走 Rust 注入的 `context-menu-fallback.js`：放行页面自绘菜单、压掉引擎原生
  菜单，页面没处理时若现场可编辑或有选区，再 invoke `show_workbench_edit_menu` 弹壳的
  原生编辑菜单（撤销 / 重做 / 剪切 / 复制 / 粘贴 / 全选，只读选区只给复制）。
  `check-invariants` 第 16 项按「每条 `WebviewUrl::External` 建窗链」查接的是不是本族
  那一份——另一半见 [src-tauri/AGENTS.md §右键策略](../src-tauri/AGENTS.md)。
- 四组高频 IPC 走单一契约（OPT-12）：`contracts/ipc-contract.json` 是唯一来源，覆盖
  `get_status` / `start_kernel` / `stop_kernel` / `memory_list` / `get_model_usage`。
  前端 `shell/ipc-contract.js` 的 `ipcCall` 校验请求与响应，失败分 `contract` /
  `transport` / `business` 三类并带契约错误码；Rust 侧 `shell/ipc_contract.rs` 用真实 DTO
  序列化对照同一份文件。改 DTO 必须同步契约与对照测试；其余命令仍走裸 `bridge.invoke`。
- 预检三态：`precheck.rs` 的 `Verdict::as_str()` 与 `PrecheckDialog.vue` 的判定表靠字符串
  对齐，有测试钉死（`precheck::tests::verdict_strings_match_the_ui_contract`）；判定为什么
  必须三态、基线为什么先跑，见 [src-tauri/AGENTS.md §预检](../src-tauri/AGENTS.md)。
- 「今日用量」只有一套口径：概览的「今日模型用量」卡与「用量分析」页内嵌的模型用量都读
  后端同一次日期命中的 `today_tokens` / `today_requests` / `today_models`
  （`usage.js::todayUsage` / `todayTopModels`），不许前端按 `days` 找「最后一天」；
  `check-invariants` 第 ⑦ 项之五钉住接线。
- 外部 agent 工具的用量来源是显式许可，默认不扫（另一半见根 `AGENTS.md`）：模型用量顶部
  一行 checkbox 勾「Codex」与「MiniMax Desktop」，默认一个都不勾。勾选状态只有一份真值
  （Rust 侧 `shell/<mode>/usage-external-sources.json`），前端只是镜像，每次拉取以后端
  返回的 `external_sources` 回填；变化时先落盘再重扫（`usage.js::setExternalSources`）。
  `loadUsageSummary` 显式传 `external: { codex: false, minimax: false }`：概览不显示外部
  来源，不该付扫描成本；省略参数表示按已保存偏好扫。外部来源在详情页是独立一块、不与内核
  那份相加，数字跟时间范围走（`sliceDays` + `summarizeDays`，90 天合计只留 tooltip）。
  另有 90s 新鲜度窗口：`refreshUsage(external, externalForce)` 的第二参数即 Rust 的
  `external_force`——进页面吃缓存（false），「刷新」与勾选变化真重扫（true）。判据
  `usageExternal.test.js`。
- 同一屏里两套账，每块都得标来源：两边名字都带「MiniMax」（内核 provider 是
  `minimax-cn`、外部是 `minimax`），光看列名分不出谁的账。内核那几块（活跃热力图 / 按天
  Token 趋势 / 模型用量）各挂 `.usage-source-chip`「内核」牌；外部那几块不挂牌，来源由
  卡片标题（`src.label`）给出，行悬停说明也要带来源。判据同上第 ⑧ 项。
- 模型用量内容区必须能滚：窗口 760×800（`USAGE_VIEWER_SIZE`）装不下四段，`.usagewin-main`
  曾写 `overflow-y: hidden`，最后一段被整段裁掉。纵向溢出回到全局约定（theme.css 的
  `main { overflow-y: auto }`）；窗口模式保留 `scrollbar-gutter: stable` + `thin`，内嵌
  模式把滚交回面板 `main`（`.usagewin-main` 必须 `overflow: visible`，两处各写一条会出
  双滚动条）。判据取层叠后的生效值。加段落前先问「它落在这屏第几屏、装得下吗」。
- 几个外部工具合一张卡、模型明细合成一张表：`.usage-external` 一张卡占满整幅宽度（不许
  `auto-fit` / `repeat()` 多列），每个工具各占一块（自己的合计、自己的按天柱条）；模型
  明细合成 `externalModels` 一张表，每行带「工具」列。占比是「占该工具自己」的比重。
  这张表与内核模型列表都不自带纵向滚动条，行多把整页撑长由 `main` 统一滚，且 main 里
  各段不许被压扁（`flex-shrink: 0`）。判据 `usageExternal.test.js` 最后两条。

## 图标

- **两套图标库并存，2026-10-03 起**：主体仍是 `@element-plus/icons-vue`（已接的不动，
  避免一次换掉几百处），**新补的走 `@lucide/vue`**（ISC 授权）。EP 的图标集里「标签 /
  版本」这类语义只有实心造型的 `PriceTag`，缩到 11px 糊成一个黑点；Lucide 是 24 网格
  2px 描边，缩到 11px 轮廓仍读得出。
  - **按图标名 tree-shake**（`import { Tag as TagIcon } from '@lucide/vue'`），不要
    `import * as`，否则 1857 枚全进包。
  - 一律**别名导入**：面板里同时存在 `el-tag` 组件与一排 `*.tag` 类名，光看 `<Tag />`
    认不出说的是哪一个。grep `TagIcon` 即为 Lucide 图标的全部用法。
  - 尺寸走**组件的 `size` 属性**，不要用 CSS 的 `font-size`：Lucide 渲染的是带
    `width`/`height` 属性的 `<svg>`，字体缩不动它。颜色靠 `stroke="currentColor"` 继承。
  - **同一语义不许两处各拼一个长得像的**：将来再要版本徽标，先建组件再复用，别把样式抄进
    某一侧的 scoped 块或 theme.css。
- **面板里的第三方标志（npm 等）与应用自身图标的两条规则见根
  [AGENTS.md](../AGENTS.md)「通用约定」**（本仓共用，只留一份），细节见
  [docs/ui/icon-design.md](../docs/ui/icon-design.md)。
