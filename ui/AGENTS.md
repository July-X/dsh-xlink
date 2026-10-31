# AGENTS.md — `ui/`（管理面板前端）

根 [AGENTS.md](../AGENTS.md) 的前端分支。根文件里的「范围」「数据目录」「发布」各章对前端同样适用，这里只收改 `ui/**` 之前要知道的约束。后端见 [src-tauri/AGENTS.md](../src-tauri/AGENTS.md)。

技术栈：Vue 3 + Element Plus 单页应用，Vite 构建到 `ui/dist/`（即 `tauri.conf.json` 的 `frontendDist`）。

**这里只写「现在该怎么做」。** 逐屏决定的经过、日期、截图与被否掉的方案在
[docs/ui/panel-decisions.md](../docs/ui/panel-decisions.md)（它不受指令字节预算限制）。

## 设计系统（uiv2 改版，2026-10-07）

设计稿 `docs/ui/dsh-xlink-ui-redesign-draft.html` + 说明
`docs/ui/dsh-xlink-ui-redesign-dev-guide.md`。**实现以源码行为为准，设计稿只表达布局与
信息层级**——不要为了填满卡片去加当前版本没有的状态、按钮或统计数据。设计稿画了而
我们判定不画的（顶部工作条、内核版本页两列栅格、侧栏 13px 字号、概览卡头实例
caption）都在下文逐条登记了理由。

### 骨架、主题与 token

- 窗口固定 1040×748 不可缩放（`tauri.conf.json`），侧栏 157px（2026-10-25 用户标注
  收窄，224 → 157）、收起态 64px。多数
  页面是双栏栅格而不是单列；改窗口尺寸要连带改各面板的 `min-width`。
- 层级三步：canvas（整窗底）→ surface（卡片）→ chrome（标题栏 / 侧栏）。卡片是
  不透明实色 + 1px 描边 + 8px 圆角；旧的 `.app-bg` 光晕与 13.5px 网格已整体删除。
- 明暗判据只有 `html.dark`。`shell/theme.js` 读一次 localStorage 落成这个 class，
  Element Plus 自带的暗色变量（`theme-chalk/dark/css-vars.css`，选择器同为
  `html.dark`）因此一并生效。**不要另造 `html[data-theme]`**——那会让组件库留在浅色
  变量上，出现「壳变了、弹窗还是白的」。theme.css 里 `:root` 放浅色 token、
  `html.dark` 放暗色 token，此外没有第二处判据。
- token 一律语义名：`--surface` / `--surface-raised` / `--surface-subtle` /
  `--surface-sunken` / `--surface-loading` / `--border` / `--border-soft` /
  `--divider-strong` / `--text` / `--text-secondary` / `--text-muted` / `--accent` /
  `--accent-strong` / `--accent-soft` / `--accent-fill` / `--accent-line` /
  `--success` / `--success-strong` / `--success-fill` / `--warning` /
  `--warning-fill` / `--danger` / `--danger-strong` / `--danger-fill` / `--builtin` /
  `--builtin-fill` / `--overlay-faint` / `--overlay-soft` / `--overlay-strong`。旧的
  `--card` / `--bg` / `--muted` / `--good` / `--bad` / `--warn` / `--surface-soft` /
  `--desc` 已全局改名，**不要再写回来**。
  - **颜色 token 两套主题各给一份；排版与控件盒高 token（`--fs-*` / `--action-h`）
    只在 `:root` 定义一次**，字号与盒高在明暗下必须一样，否则切一次主题跳一档。
  - **`var(--旧名, 兜底值)` 是最阴的一种形态**：token 改名后兜底值让整条声明看着还
    在（CSS 不报错），元素只是退回那个近白，次要文字在浅底上直接消失。
  - 批量替换颜色会把 token 的**定义行**一起换掉，得到
    `--overlay-strong: var(--overlay-strong)` 这种自引用；判据除了「有没有定义」
    还要钉「值必须是字面量」。
- 遮罩有两种语义，不许合并成一个变量：`--el-mask-color` 是**全屏**遮罩
  （`el-dialog` / `el-message-box` 的 `.el-overlay`、壳自己的 `.progress-overlay`）
  的压暗档，`--surface-loading` 是**内嵌**加载遮罩（`v-loading` 的 `.el-loading-mask`）
  的盖住表面档。EP 把两者绑在同一个 `--el-mask-color` 上，只调浅色那一档会让浅色
  主题下的内嵌加载区变成深灰。`.el-loading-mask` 的规则必须写在 theme.css 全局作用
  域——它是指令运行期插入的节点，身上没有组件的 `data-v-*`。判据
  `ui/test/epStyles.test.js`。
- 叠色一律走 token，不许写死 `rgba(255,255,255,…)`：同一个视觉意图在暗色下是叠白、
  浅色下是叠黑。accent 的半透明档同理（`--accent-fill` / `--accent-line`），那组 alpha
  是按深色 accent 调的，浅色 accent 是 `#2766d9`，同一 alpha 会糊成一层洗不掉的蓝雾。
  日志阅读器（`.log*` / `.logwin*` / `.rail-toggle` / `.pane-splitter`）里那几处写死色
  同理，`.log-tab` 未激活态写的 `rgba(232,236,247,.72)` 是近白，浅色下整列文件名几乎
  看不见。判据 `designAlignment.test.js`（叠色 token 一组）与 `logWindowTheme.test.js`。
- 浮层底色只有一个来源：`.progress-body` / `.el-dialog` / `.debug-panel` /
  `.render-error-fallback` 统一读 `--surface-raised`（浅色 `#fff` / 暗色 `#35343a`，
  与 EP 的 `--el-bg-color-overlay` 同值）。`--el-dialog-bg-color` 必须声明在
  `.el-dialog` 自身而不是 `:root`——库在 `.el-dialog` 规则里就写了同名自定义属性，
  元素身上的同名声明赢过从 `html` 继承的值，写在 `:root` 等于没写。遮罩统一读
  `:root` 的 `--el-mask-color`。判据 `designAlignment.test.js`「弹窗与浮层的主题适配」。
- 彩色按钮与徽章的文字色走语义 token，不许写死「为深色底调的淡彩」。那一族值（Tailwind
  的 `emerald-300` / `red-400` / `emerald-400`）在白卡上只有 1.5–2.8:1，正文门槛是
  4.5:1，而它们在暗色下看着是对的——只跑暗色截图的检查抓不到，所以判据钉的是「读
  token」而不是「等于某个色值」。`.btn-chat` → `--success`、`.btn-danger` →
  `--danger`，hover 用 `-strong` 档；`--success-fill` 是补齐既有三档，**刻意不补
  `--success-line`**（没有第二处用得上）。`.el-icon` 不再单列选择器（EP 的是
  `--color: inherit`），抄一遍只染到一半。判据 `designAlignment.test.js`「彩色按钮」。
- 「刻蚀线」是块头分隔线的统一标准：**2px、两端收细、`--divider-strong`**。收细只能
  用 `clip-path` 六边形——渐变（含 `mask-image`）改的是透明度，两端是「淡到看不见」
  而不是「细」，1–3px 尺度下几乎分不出来；颜色是 token。原生 border 全部退场并
  **显式写 `border-*: 0`**（删掉声明会让「已退场」不可测）——border 没法 clip，直线
  与收细只能二选一。
  - 横线六边形写在三处（theme.css 的 `.card-head` / `.card > h2` /
    `.card-head-toggle` / `.debug-panel header`，diagnostics.css 的
    `.diag-card--tower`，`OverviewPanel.vue` 的三格指标线），**判据逐字比对三处
    一致**，只改一处就会出现同屏两种收细比例。竖线把六边形转 90°，收细比横线短
    （22% vs 20%），按横线比例去收两端会削掉十几像素，出来是一片叶子。
  - 适用范围是「有线的块头」：填色工具条（`.logwin` 头 / `.subwin-head` /
    `.usagewin-head`）、窗口 chrome、表格行与 `.section-divider` 那条虚线是别的角色，
    5 个独立诊断窗口本来就没有线。
  - `--divider-strong` 明暗各一档：暗色 `#31435f`（`#212121` 压在 `#2c2b30` 卡面上
    约 1.15:1，作为分割线等于没有），浅色 `#51627e`（无彩近黑 16.1:1 比正文还重，
    用户报过「分割线颜色不协调」）。判据算**对比度**不算法面值：两套都声明、各自
    压卡面 ≥1.5:1、都比描边重，浅色还多钉一头「不比正文重」。
- 排版与控件档位收成 token，六个面板向概览页看齐：

  | 档位 | token | 值 | 改之前的分歧 |
  | --- | --- | --- | --- |
  | 功能块标题 | `--fs-block-title` | 17px | `.card-title`（技能页「社区资源」）**全仓没有对应规则**，字号继承 body 的 14px |
  | 分区 / 卡内小标题 | `--fs-subtitle` | 15px | `.logwin-title` 14px、`.community-title` 12px、`.restore-section-title` 13px |
  | 页头副标题 | `--fs-page-desc` | 12px | 原先读 `--fs-desc`（13px），那一档是「独立成段」的说明文字 |
  | 标题旁的 ⓘ | `--fs-tip-icon` | 16px | `.card-info-icon` 16px（5 个面板）vs `.head-tip-icon` 13px |
  | 动作按钮 | `--action-h` / `--fs-action` | 30px / 13.5px | 一页之内三种（30 / 24 / 32px） |

  - 共用规则扩到四个容器：`.card-head` / `.install-row >` /
    `.community-browse-row >` / `.section-divider`。**`.entity-action` 那种列表行里的
    圆形图标按钮不在其中**，那是「紧凑动作」，另一个角色（用户 2026-10-08 定的
    30px 有意尺寸）。
  - **输入框要跟着一起压**：`.install-row` 上加 `--el-component-size: 30px`（EP 的
    `.el-input` 高度就是这个变量），只压按钮的话按钮比输入框矮 2px。不需要覆写
    `.el-input__wrapper` 的内部结构。
  - **卡头标题一律用 `<h2>`**，别写 `<span class="card-title">`（那个类在本仓没有
    对应规则，写上去只继承 14px）。
  - 标题与状态行在概览页是**竖向块**：版本号 20px，状态行跨两列落第二行（横排时状态
    胶囊会随内核状态换行）。
- **副标题的字号档不与 `--fs-desc` 合并**：同行后 13px 在 32px 行高里过重，与标题的间距
  只剩「字高差」那一层。

### 页面原语

原语在 theme.css 的「新版页面原语」一节：`.page-head`（页头：左标题右动作）、
`.page-head__text`（页头里包住标题与说明的那一层）、`.page-title-row`、
`.head-tip-icon` 及 `--warning` 修饰、`.page-layout`（双栏栅格，`__col` 一列、
`__full` 横跨整行）、`.page-list` / `.page-row`、`.btn` 及 `--primary` /
`--secondary` / `--ghost` / `--danger` / `--icon`、`.nav-item`、`.plugin-tab`、
`.section-divider`、`install-row`。**先找现成原语，再写 scoped 样式**；每个面板自造
一套的结果是六个面板六种圆角。

- **登记进这份清单的类名必须真有模板在用。** 这是「已有原语」的索引，不是「打算加的
  原语」的许愿单：加一条之前先 grep 一遍模板。`.page-card` / `.page-card__head` /
  `.page-card__body` 已删（设计稿里有、`ui/src` 零引用）；`.health-row` /
  `.health-dot` / `.health-row__value` 曾被登记但控制塔实际走 diagnostics.css 的
  `.diag-row` 家族，38 行死 CSS 已删。**设计稿里有 ≠ 实现里该有。**
- `.metrics` / `.metric` 归 `OverviewPanel.vue` 的 scoped 块，不在全局原语清单里。
- 侧栏整体放大一档后：`.nav-item` 15px、`.sidebar__section-label` 12px、
  `.nav-item__badge` 12px / 20px 圆点、`.sidebar__footer` 12px；间距同步抬（行
  `padding 8px 10px`、`gap 11px`、分组 `gap 4px`、收起态 `padding 8px 0`）。157px
  放得下最长的一项，748px 高里侧栏总高约 450px。
  - **图标必须显式写死**（`.nav-item > .el-icon { font-size: 17px }`），只抬父级字号
    等于图标等比跟着长（EP 的 `.el-icon` 是 `font-size: inherit` + 1em）。17 vs 15
    是刻意的：图标读起来要比文字再大一点点才不显矮。
  - 品牌行鲸鱼图标 34 → 42px，圆角 7 → 9，`.brand` 的 `gap` 10 → 12。收起态去掉 8px
    横向内边距后内容盒 48px，42px 仍放得下。
  - **主名按空格折行，不许省略**（2026-10-25 用户要求）：157px 里「DeepSeek Harness」
    (~88px) 放不进 ~79px 的文字块——`white-space: normal` + `overflow-wrap: anywhere`
    折成两行；ellipsis 兜底已被用户否掉，判据反向钉住「不许再声明 text-overflow」。
  - **`<img>` 的 `width` / `height` 属性必须与 CSS 相等**（`SideBar.vue` 与
    theme.css 都是 42）：CSS 只覆盖绘制，那两个属性是给读屏与布局的第一手尺寸，两处
    各写一遍迟早漂。同理 `flex: 0 0 42px` 少一条，图标会被文字挤扁且不报错。
- 侧栏底部工具区**是两行**：第一行三枚方钮（收起 + 刷新 + 主题）**左对齐贴左缘**
  （`.sidebar__footer-actions` 的 `justify-content: flex-start`），第二行「桌面端」标签
  + 版本号同行（`.sidebar__footer-meta`）。**版本号不许回到控件同一行**——标签 + 三枚
  26px 方钮 + 4 道缝占掉 ~142px，同行时只给它留 ~78px，dev 后缀必被截掉。**dev 构建
  隐藏「桌面端」标签**（`v-if="!store.devUi"`）——「（dev）」后缀已表明身份，标签
  让位给全形态版本串的完整显示，dev 后缀不许再被截。**收起态必须竖排**（写死
  `flex-direction: column`，不靠 `flex-wrap`——那取决于渲染宽度）：三枚控件横排要 92px
  而内容盒只有 48px；**收起态第二行整行隐藏**（`.sidebar__footer-meta { display: none }`，
  不留空高）。判据在 `designAlignment.test.js`。
- 构建标识（dev 鲸眼红 / release Gitea 绿）收成**窗顶 2px 细线**，由 `--build-color`
  驱动，两种构建共用同一条 `.app-shell::before` 规则只换颜色变量。旧版那条「铺满全高
  的品牌色渐变带」已删除，它与卡片描边互相拍频。判据 `ui/test/diskUsageAndChrome.test.js`
  钉住了「只有这一处画法」。
- **宽表格要给数字列显式列宽**（`table-layout: fixed` + `nowrap`），并按内容量分别给
  值。自动布局会把表头竖排成「文件 / 数」、把 `896.7 KiB` 拆成两行；固定布局后列宽
  不再随内容生长，于是**宽度变成承重的**：给窄了不会换行，而是文字直接溢出表格右
  边界。**路径类长文本列不要 `nowrap`**，让它们折行——完整路径要看得见，截断比换行更
  容易读错。判据在 `ui/test/migrationPanel.test.js`，断言要抽 `nth-child(n)` 那条规则里的
  `width` 再比值，不要比两段起点不同的 CSS 子串。
- 页签（药丸）两侧内边距都要在，选择器必须是
  `.installed-tabs .el-tabs__nav .el-tabs__item:nth-child(n)`：EP 有两条 (0,4,0) 的
  规则会单侧清零（前提是「页签之间的缝由前一个的 padding-right 承担」，那只在页签
  没有底色时成立），本仓原来那条只有 (0,2,0) 静默输给库，**不报任何错、build 全绿**。
  `:nth-child(n)` 匹配每一个页签。判据钉的是**选择器形状**，不是当前几行值。

### 卡片与页头

- 每张卡片的标题与内容之间必须有 1px 分割线，样式照 theme.css 的 `.card > h2` /
  `.card-head` 抄：

  ```css
  padding-bottom: 9px;
  border-bottom: 1px solid var(--border);
  ```

  - **`padding-bottom: 9px` 与那条线是一套**，只给线会让它紧贴文字。控制塔那份同步改了，
    `ui/test/cardChrome.test.js` 钉住两处相等。
  - 色值只有 `--border` 一种，不要为标题线另开一个更深的 token：同一个值画在卡片边缘
    （`.card` 自身描边）就相当清楚。
  - **「线到内容」的距离不由这条规范钉死**，交给各卡片自己的间距机制（`.card` 靠容器
    `gap`，`.diag-card` 靠标题 `margin-bottom`），同页出现 4px 与 8px 两种是对的。
  - 覆盖范围是概览页与四个面板里的卡，**不含 5 个独立诊断窗口**（那 5 页在
    `.diagnosis` 容器里，是另一套布局与密度）。给控制塔补线要限定在
    `.diag-card--tower` 上：改基线 `.diag-card__title` 会顺带改掉那 5 个窗口。
  - 控制塔的 `--tower` **显式写** `margin-bottom: 6px`，基线 8px 不动（其余五个诊断窗口
    靠它）；别写 5px / 8px 这类凑值，也别删覆盖让标题回基线。`padding-bottom` 仍与全局一致。
  - **别用「某条规则写了 `border-bottom`」当判据，要查层叠后的生效值**：有一条规则声明了
    又被同特异度的后一条盖掉，是静默的（做法见 `ui/test/panelSpacing.test.js`）。
- 页头的标题与说明必须同属一个子元素：`.page-head` 是 `space-between` 的 flex 行，
  六个页面的模板结构是「`<div>` 包住 `.page-title` + `.page-desc`」。把 `.page-desc`
  提到 `.page-head` 的直接子级，说明会立刻被当成右侧动作飘到页面右缘。
  **判据比的是嵌套深度而不是 `</div>` 个数**——两种错结构的 `</div>` 计数一样。
- 页头主副标题同行，排布由 `.page-head__text` 承载（用户对六个页面统一提的要求：
  主标题文字与大小不动，副标题缩小并与主标题同一行）。那一层包裹 div **不能删**，
  上面那条判据量的就是它的嵌套深度。
  - **`align-items: baseline` 而不是 `center`**：`center` 让 12px 落在 32px 行盒正中，
    光学中心勉强重合而基线不重合，句号会浮到标题基线之上。
  - **`.page-title` 必须钉 `flex: none`**：不钉的话副标题一长被压扁的是**标题**，
    标题断行比说明换行更糟。
  - **`.page-desc` 的下外边距清零**：同行之后由 `gap: 12px` 承担，留着那 5px 会变成
    行盒之外的额外空白、把副标题往右推。
  - 判据在 `designAlignment.test.js`「页头主副标题同行」，**样式与模板一起钉**：只钉
    `display: flex` 是假绿，六个模板忘了挂 `page-head__text` 时样式判据照样绿。同条
    还钉主标题 24px / 700 / 32px 与副标题读的是 `--fs-page-desc` 那一档。
- 一条提示和它的动作按钮不许是两个兄弟节点（概览页的 `el-alert` 与「更新并重启」
  现同属 `.update-banner`）。`.el-alert` 要 `flex: 1; min-width: 0`，不写的话提示
  文字一变长先被挤下去的是按钮。判据查的是「同属一个 `.update-banner`」。
- 弹窗 / 浮层底部的操作排统一靠右，规则落在**一个共享类** `.dialog-actions`
  （theme.css，`.btn-row` 旁边）。
  - `el-dialog` 的 `#footer` 本来就右对齐，不必再加；`MigrationPrompt` 的
    `.prompt-footer-ask` 是另一回事（按钮被包在 `div` 里，块级 `text-align` 管不到），
    它已经是对的，**不要为了「统一」去动它**。
  - **不要改 `.btn-row` 本身**：概览的欢迎 callout、设置页的通知行里它与旁边的说明文字
    同属一段内容，右对齐会把文字顶走。判据钉住 `.btn-row` 没有 `justify-content`，也
    钉住仍是 `wrap`（同页「任务通知」卡里带说明文字的行**应该**折行）。
  - 弹窗内部「按对象分组」的动作排不参与统一：事故面板每个嫌疑插件块里的「隔离 /
    移除」跟着对象走才对（`.suspect-actions` 不带 `dialog-actions`）。
- 补充 / 警告类提示一律收成标题旁的图标 tooltip，不许再做成常驻整宽条，判据是这条提示
  **不含任何随状态变化的信息**，语境在哪就把图标挂哪。**触发元素必须可聚焦**
  （`tabindex="0"` + `aria-label`）：hover-only 的信息对键盘用户等于不存在；**`content`
  与图标的可访问名取同一个常量**，两处各写一遍就会出现「图标说有提示、读屏说没提示」。
  严重程度分色：ⓘ 是补充说明、⚠ 是「有件事你该知道」，且 ⚠ 悬停时不要改成强调蓝。
- 分组标题右侧的辅助信息比标题小一档（`.section-divider__note` 12px）。**字号不许
  加进 `.muted`**：那个类只管颜色，全仓 23 处在用，一刀切会顺带改掉六个面板。
- tooltip 浮层一律限宽并换行，选择器钉在 `[role='tooltip']`。
  - EP 的 `.el-popper` 只有 `min-width: 10px`、没有 max-width，而浮层是自适应宽盒子：
    本仓最长的 `CAPABILITY_TIP` 约 140 字 ≈ 1680px，窗口只有 1040px。
  - **不许直接给 `.el-popper` 加宽度上限**：本仓 2 个 `el-select` 与 3 个 `el-dropdown`
    走同一个类，菜单本来就该按内容取宽。`[role='tooltip']` 是唯一的区分点——EP 的
    `role` prop 默认就是 `"tooltip"`，`el-select` 传 `"listbox"`、`el-dropdown` 传
    `"menu"` 都被排除，`el-popconfirm` 内部就是 `ElTooltip` 且不覆盖 role。
  - **一条全局规则胜过给 36 个 `<el-tooltip>` 逐个挂 `popper-class`**：漏挂的那个恰好是
    最长的那一个。
  - **`overflow-wrap: anywhere` 而不是 `break-word`**：前者参与 min-content 计算，在
    自适应宽盒子里才真能把长路径压回上限内（路径与隔离原因可能全是 ASCII，中文能自然
    断行而它们不能）。
  - 判据三条：`.el-popper[role=tooltip]` 规则存在且四条声明齐全（`min-width: 0` /
    `max-width: min(420px, calc(100vw - 32px))` / `white-space: normal` /
    `overflow-wrap: anywhere`）、**裸 `.el-popper` 不许带 max-width**、以及 EP 那三个
    role 默认值（读 `node_modules/element-plus/es/...`）。第三条是前提判据，验不了就
    `t.skip` 并说明，**不假装绿**。

### 按钮与无障碍

- 无文字按钮（icon-only）走 `icon-btn`，尺寸 26×26（theme.css 全局规则）。**根因是
  EP 的默认值不能用**：`el-button` 基础规则带 `padding: 8px 15px` + `height: 32px`，
  而 `is-text` 只把背景换成透明、padding 原样保留，一个 14px 的图标被撑成约 44×32 的
  一块。命中区靠 `::after` 补到 32×32 **居中**外扩：相邻两个的命中区重叠 4px，取
  交集后两侧各让 2px，仍在 Windows SMALL_CAPTION 的 24px 下限之上。
- **`title` 与 `aria-label` 都要有，两者缺一判据就红**（`check-invariants` 的
  `a11y-icon-button` 项）。`el-tooltip` 只认鼠标悬停，键盘与读屏用户永远看不到它；
  `title` 是唯一对键盘也生效的那份说明，`aria-label` 是读屏的可访问名。图标本身不是
  文本。判据按「自闭合 `el-button`（插槽为空）且有 `:icon`」判定，而不是 `circle`
  这一种画法。
- **同一状态挑「条件写进选择器」的写法**：`.btn-chat` / `.btn-danger` 用
  `:not(:active):hover` / `:not(:active):focus-visible`，不要再为压过 `:hover` 抄一条
  `:active { color: 静止色 }`（判据显式禁掉那两条）。
- 药丸页签、`.btn-row` 的次级按钮那一排量的是**余量**不是「放不放得下」：三枚
  `.btn-sub` 收内边距 11 → 9、字号 12 → 11.5px、图标 17 → 13px（图标那条必须比
  `.btn-row .el-button .el-icon` 那条更晚，两者同为 (0,3,0) 只能靠源码顺序决胜，
  判据钉住先后顺序而不只是值）。**量余量必须量版本号的文字宽度，不能量它那一格**：
  `1fr` 那格会把余量吃掉，第一版就是这么量的，结论是「改前改后都余 0px」。
- 「一行放不下」有两种病，治法相反：**余量为负**是排布错了，该换排布；**余量只剩几
  像素**是碰运气，该指定谁让位（动作按钮宽度由文案定死，输入框是容器）。缺出来的部分
  由 `.settings-port-row { flex-wrap: nowrap }` + 输入框 `flex: 0 1 132px;
  min-width: 96px` 吸收，**两半缺一退法都不同**：只有 `nowrap` 会横向溢出并被 `main`
  的 `overflow-x: hidden` 裁掉，只有可收缩仍然折行。判据钉**机制**、不钉宽度数字。
- **同一屏里同一个东西只能有一个名字**，判据看「叫什么」而不是「长什么样」。
  - 内核版本页的卡头叫「已安装 / N 个版本」而不是「内核版本」（页面标题已经叫内核
    版本了），组内那个 `<h3>已安装</h3>` 一并去掉。发布列表里那个行内
    `<el-tag>已安装</el-tag>` 是另一个含义（标出这个远端版本本机已经有了）。
  - 插件页外层卡叫「插件管理」；2026-10-31 远端目录移除后不再有「插件中心」
    这个界面名。内核版本页卡片标题叫「官方版本」而不是「npm 发布」（npm 只是
    取源渠道，换回 GitHub Releases 后标题会立刻变成错的）。
  - **提示图标的可见文字必须和它 tooltip 里的内容是同一件事。**
- 动作按钮要跟着它作用的那个对象走：插件页的安装按钮只作用于插件库，必须留在
  「插件库」页签的安装区块里，不要挂到对两个 tab 都可见的卡头。**元素搬走后旧位置
  的规则必须删干净**：元素不在那儿了，留着就是一条没有作用方的声明。判据按**类**
  数出现次数（按文案找会踩「关闭」在多处重名的坑）。

### 窗口 chrome

- **「入口还在」不等于「文件还在」**：`WINDOW_ENTRY_POINTS` 把「文件存在**且**在自身目录
  之外还有挂载点」一起钉住。扫引用要带 `.js`（挂载点在 `main.js`）；**同目录的引用不算
  入口**（组件自家的 store 模块只是它自己的状态容器）。
- 四扇壳自有窗口（主壳 + 日志 / 模型用量 / 套餐用量）建窗期一律
  `decorations(false) + transparent(true) + shadow(false)`，圆角由 CSS 画。`transparent`
  走的是「`setOpaque(false)` + `setBackgroundColor(clearColor)`」，**不碰
  `NSWindowStyleMask`**；只有 `set_decorations` 才会重算样式位、抹掉
  `Miniaturizable`（tao 0.35 实测）。
  - **`shadow: false` 不能省**：透明 + 圆角后系统投影会画在圆角之外的透明区域，读作一圈
    黑色矩形光晕，投影改由 `#app::after` 的 `box-shadow` 补。**底色必须跟着圆角一起从
    `body` 搬到 `#app`**：留在 body 上，圆角外四角仍是一块实色方块，而少做这一步**没有
    任何报错**，窗口只是「看着没圆角」。
  - 三扇副窗共用 `shell/ViewerShell.vue`（自绘标题栏 + 圆角 + 裁切 + 投影），能力
    文件有 `allow-start-dragging`。
- 四扇自绘窗口必须在 `body.custom-titlebar-shell` 上
  `clip-path: inset(0 round var(--window-radius))`：`#app` 的圆角与 `overflow: hidden`
  约束不到越过包含块的固定定位投影，也约束不到 teleport 到 body 的遮罩，窗口外四角必须
  全透明。**官网页签栏与工作台保留原生窗口轮廓**（前者承载 `chat.deepseek.com` 等别人
  的页面，后者是内核 webui）。
- 自绘交通灯按 macOS 原生的**实测**尺寸，不按设计稿：直径 12px、中心间距 23px、
  灯组左缘距窗缘 7px（实现改 `gap 8 → 11px`、`padding 0 12px → 0 7px`）。
  - **描边从 `border` 换成 `box-shadow: inset`**：全局 `* { box-sizing: border-box }` 下
    `border: 1px` 会从 12px 里各吃掉一侧，着色区只剩 10px。
  - **hover 是整组浮现，底色不变，符号分色**：组 hover 而非单颗（指针压在红上就只亮
    红，另外两颗读成「不可点」），命中区靠 `.mac-titlebar__controls` 的 padding 撑开；
    底色不变（原生不做 `brightness`，灯一变亮整排读成「有一枚被选中了」，判据显式禁掉
    `brightness(`）；三枚符号红 `#460804` / 黄 `#90591d` / 绿 `#2a6218`，统一成一个
    深灰会在黄、绿上偏紫。
  - 判据钉关系而不是字面量（`width + gap === 23`），另有两条断言「不许退回 `border: 1px`
    / `border-color`」与「三盏灯都得有 inset 环和填充色」。灯组左缘 7px 带 ±1px 不确定度，
    **故意不钉字面量**。
  - 两套 chrome 各自按本平台走：交通灯那组带 `v-if="isMacTitlebar"`，Windows 用
    `.win-caption` 右侧按钮；判据两头都钉。
- **副窗只留关闭按钮，最小化是主壳独占的动作。** 收进后台 / 托盘那套是**主壳**的语义；
  Windows 上那枚最小化按钮调 `minimize_shell`，授权只归主壳，副窗点了只会得到一句
  `Command minimize_shell not allowed by ACL`——一枚必然失败、且不提示自己坏了的按钮。
  绿点（禁用、窗口不可缩放）一并去掉，同一条理由。
  - `WindowTitleBar` 的 `close-only` prop 是**能力开关不是样式开关**：`ViewerShell`
    传它 → macOS 只画红灯、Windows 只画 ✕；`minimizeWindow()` 里另有
    `if (props.closeOnly) return`，模板日后被人加回按钮时命令仍发不出去。关闭键**必须
    还在**（删掉整组按钮同样能让「没有最小化」成立，而那扇窗就再也关不掉了）。
  - 关闭键的**悬浮说明按同一口径分档**：主壳写「收进后台 / 收纳到通知区域」，副窗只写
    「关闭窗口」——「程序继续运行」写在副窗上是在骗人。
  - 系统层那一半在 Rust（`shell::window::decorate_transparent` 的 `minimizable(false)`），
    前端只管自绘 chrome：任务栏右键「最小化」、Win+D 绕得开模板。三份副窗 capability 里的
    `core:window:allow-minimize` 同时删掉——留着等于给一个不该存在的按钮预留后路。判据在
    `ui/test/windowChrome.test.js`「副窗标题栏只留关闭按钮」：接线、两个平台的门控、
    `minimizeWindow()` 的提前返回、Rust 的 `minimizable(false)`、三份 capability **不许**
    有 minimize。
- **副窗标题栏显示「功能名@应用名」，主壳只显示应用名**：`ViewerShell` 传功能名，日志窗跟随
  文件名（`:title="activeName || '日志'"`，兜底不能省——为空时标题会退成 `@Dsh-Xlink`）；
  分隔符为什么用 `@`、判据在哪，见[窗口标题](../docs/architecture/window-architecture.md#标题一个名字三处副本)。
- **副窗的原生标题栏跟应用主题走，不再钉死深色**：原生 chrome 由系统按窗口的
  `NSWindow.appearance` 画，主题真值在 localStorage，Rust 建窗时读不到，于是 6 扇副窗一律
  `.theme(Some(tauri::Theme::Dark))`。修法：`applyTheme()` 把 `theme.value` 推给
  `bridge.setWindowTheme()` 纠正**本窗口**；4 个壳自有 capability 补齐
  `set_window_appearance` 授权。
  - `window.setTheme()` 收的必须是 `theme.value` 本身，写死字面量会让「切了主题只换
    页面、不换标题栏」。
  - **只改本窗口的原生装饰，不批量改**（判据禁掉 `getAll` / `webview_windows` /
    `forEach`）：批量改只换得来标题栏，换不来副窗的 `html.dark`。
  - **已开着的副窗要跟着换，靠广播**：`storage` 事件**不跨 webview 生效**，四个副窗各是
    独立 webview。`setTheme` 落地后 `broadcastTheme(next)` 发 `ui-theme-changed`，各窗在
    `main.js` 里 `followThemeBroadcast()` 订阅并自己跑一遍 `applyTheme()`。订阅必须在
    `createApp` 之前（晚一步首帧会先画一帧旧主题再跳色）；副窗不调 `setTheme`（会互相
    触发）；事件值必须过 `THEMES.includes`。
  - **副窗不许只调 `setWindowTheme`**：那只换标题栏、留下旧内容。
  - **工作台、官网页签窗都不跟随本应用主题**（`official-chat-*` 无 set-theme）：工作台由
    **内核**的 `ui-theme` 决定、壳不钉建窗主题（见根 `AGENTS.md`）；官网页签窗装的是别人的
    站点，仍钉死深色。还须 `if (isChatStrip) setThemeValue('dark')` 且
    `followThemeBroadcast()` 一起排除——按初值不够，主壳切主题时那条广播会拽走它。判据：
    给 `official-chat-strip.json` 补授权即转红。
  - 建窗深色是**兜底**不是最终外观：挂载到纠正之间会闪一下；彻底消掉需 Rust 也读得到
    主题真值，本轮不做。
- 外观在设置页右栏三项（浅色 / 深色 / 跟随系统），与侧栏循环按钮共用 `theme.js`：
  `theme` 保存模式、`resolvedTheme` 解析实际明暗并写进 `html.dark`，固定模式写入原生
  装饰、system 向原生接口传 null 解除覆盖；侧栏按钮按 `themeOptions` 循环，图标表示
  当前模式，悬停提示同时说明当前模式与下一次点击的目标。macOS 的 `setTheme` 与建窗
  `.theme(Some(...))` 会覆盖整个应用，不能用于窗口主题——建窗统一走
  `shell::appearance::initial_theme(label)`，原生外观经 `set_window_appearance` 只设当前
  `NSWindow`（工作台那扇反过来：壳不许钉它）。Windows 继续用 Tauri 的窗口主题实现。

### 逐屏决定

> **只留规则**；各屏的经过、日期、截图与被否掉的方案在
> [docs/ui/panel-decisions.md](../docs/ui/panel-decisions.md)（2026-10-25 为字节预算搬去
> ——本文件一度越过运行时 65536 字节上限，尾部被静默截掉，**写在这里却从没被读到过的规则
> 比没写更坏**）。

- **技能页是「已安装 / 社区资源」两张卡**；设计稿那张「技能状态」卡不画（没有后端字段支撑，
  硬写就是编数据）。
  - `installSkill` 走 `withProgress`（长任务）**不经过 `withLoading`**，所以安装按钮上挂
    `isLoading('installSkill')` 永远为 false——那是一个永远不转的假 loading。在途状态由进度
    浮层表达，按钮只做 `globalBusy || 输入为空` 的禁用。
  - 单页专用样式留在 scoped。输入行复用全局 `.install-row`，卡内标题与说明各合成一条基线
    规则（`.community-title` / `.community-meta`）；技能社区与手动安装之间只画一道 2px 刻蚀
    线（`.community-install::before`），两侧边框显式清零。
- **设置页左栏是工作台、后台常驻、环境回退与诊断，右栏是外观、任务通知**。卡内保留
  三个 `.page-list-row`：**环境回退点 / 深入排查 / 启动诊断**；数据迁移从侧栏进入。
  - `SnapshotCard` 与 `BisectPanel` 渲染成 Vue Fragment，套进共享卡就会出现卡中卡。判据查的是
    **形状**（整页只有一张卡 / 没有两列栅格）而不是某个类名。
  - **收起的只有明细，状态不许藏**：状态说明（`headline(view)` / `bisectHeadline(view)`）
    挂在 `.page-list-meta` 常驻，`drifted` 与 `conclusion` 两条告警**不进闸门**。
  - `.page-list-meta` 故意不抄设计稿的 `nowrap` + 省略号：这里的次行是整句说明，截成一行
    就分不出「还没成功启动过」和「文档损坏」。
  - **收尾那条的分隔线收在 `.page-list > *:last-child` 上**而不是 `.page-list-row:last-child`：
    Fragment 展开后明细也是列表的直接子级。
  - 「启动诊断」是补上的入口，直接打开诊断窗口，不是有内容的展开层。
- **内核版本页维持「一张卡竖排」**，不改设计稿的两列栅格。判据钉的是「整页只有一张卡、
  没有两列栅格」。
- **内核版本页两栏之间那道竖线不贯通**：线的高度只跟着左栏「已安装」那几行走
  （`align-self: start` + `left: 100%` + `margin-left: 3px`）；沿用刻蚀线标准（2px、
  `--divider-strong`、六边形收细），仍用 `::after`，**不许用 `border-left`**；**空态是唯一
  例外**（`.installed-list` 必须继续伸展，选择器带 `:not(:has(> .installed-list > .el-empty))`）。
  判据在 `designAlignment.test.js`（2 条），其中一条**不拿调用点传进去的选择器当证据**。
- **顶部工作条（`.workspace-bar`）已整条删除**（删的是界面上的切实例，运行状态与实例清单
  没删）。**`instance.js` 的 `setDefaultInstance` 不许跟着删**：它是**没有调用方的 API**，
  不是「永远走不到的代码」——判据钉住的不变量是「模板侧零调用 + 函数与 `selectionRevision`
  仍在」。
- **概览「当前内核」卡：主操作上移，实例 caption 删除**。卡头只有 `<h2 class="kernel-title">`
  与 `<div class="kernel-header-actions">` 两栏；`kernelCaption` 连同概览页对实例注册表的整条
  引用一并移除——**`App.vue` 仍在启动时加载实例列表，插件页不受影响**。
- **内嵌插件行并进社区插件列表，是那个列表的第一行**：现在就是一条普通的
  `.entity-row.entity-row--single`，名称 / 来源 chip / 状态文字全部复用社区插件那一套。
  - **语义差别只体现在右侧动作**：没有安装 / 更新 / 卸载，保留启停开关，并提供 ChatGPT
    登录、取消登录、刷新模型列表与退出登录；停用插件不退出账号。
  - **那套 `.builtin-*` 样式必须删干净**：元素并进来之后它们就成了没有作用方的声明。
  - **空态判据要一起改**（`currentKernelListEmpty`）：`is-empty` 与 `el-empty` 必须共用这一份。
  - 判据按**顺序**判（列表开标签 → 内置行 → 社区插件 v-for）。
  - **内置插件不止一个了：那几行是 `v-for`，文案与能力都来自后端**（`builtin_status` 一次
    返回全部，显示名 / ⓘ / 要不要画账户卡全由后端 `BuiltinSpec` 下发），**前端不留一个字面量**。
    - **「按 id 取」不许写成「取第一项」**：拿第一项会在某个内置插件排到前面时静默返回别人的
      状态，判据在 `ui/test/builtinPlugins.test.js`（它把 ponytail 摆在 openai 前面）。
    - loading key 按插件分（`builtinToggle:<id>`）：共用一个 key 时拨一个会让另一个的开关
      也转起来。
  - **内置插件的来源 chip 是放大一档的紫色，此后每个内置插件都用这一枚**：
    `.origin-chip-builtin` 叠在 `.origin-chip` 之上，13px + 图标显式 15px + `--builtin` 文字 /
    `--builtin-fill` 底 / 同色描边；npm 与 GitHub 那两枚继续走中性灰。
    - **挂在那枚 chip 上，而不是挂在某个插件 id 上**：chip 在 `v-for` 里，后端登记表加一项就
      自动带上紫色。
    - **色值走 `--builtin` / `--builtin-fill` 且两套主题各一份**。只在一套里定义的话，另一套下
      `var(--builtin)` 整条失效且不报错。
    - 判据钉「修饰类全模板只出现一次」、颜色读 token、字号与图标字号的关系、两个 token 两套
      主题都定义。
- **业务能力不因改版增减**：页签分组（工作台 / 资源 / 系统）、侧栏收起，都是把原有信息
  重新摆位。
- **末行两卡吃满剩余纵向高度，块内文字换行**。高度：`overview-grid` 写
  `grid-template-rows: auto minmax(min-content, 1fr)` + `flex: 1`，祖先 `.panel` 给
  `min-height: 100%`；用 `min-content` 起底而不是 `0`（后者会把卡压扁、内容溢出格子）。
  换行：`.diag-row__label` 改成 `overflow-wrap: anywhere`，`anywhere` 不能省。
- **行尾动作是带边框、带 icon 的徽章**（`.diag-row__cta`），不是纯文字箭头。语气跟着同行
  读数的严重程度走（`--danger-*` / `--warning-*`），悬停加深自身色而不是换成强调蓝——accent
  蓝在这套里表示「主操作」。**展开箭头与动作按钮不许共用类名**（`.diag-row__caret`）：一个
  类名表示两件事时，改其中一种会顺手改到另一种。判据显式钉住「`.diag-row__arrow` 不许再出现」。
- **系统健康 / 最近操作 / 需要关注三张卡的卡头跟着概览走**：块标题读 `--fs-block-title`（17px），
  `padding-bottom` 保持 9px 不动。**`--bare` 用 `:not()` 正着写**：线从 border 换成伪元素之后，
  只清 `border-bottom: 0` 那条照样会画出一条线。
- **「最近操作」四个类型词是可点页签**（2026-10-28 用户要求）：各自切到该类最近一条
  （数据来自已加载的 `recentRuns`，不发请求），默认跟随全类型最近一条；空类压暗并给
  正文说明。页签是 `<button role="tab">` + `aria-selected`，样式走 `.run-tab`
  （显式 reset + `:focus-visible`）。
- 概览主栅格 `1.25fr / 0.75fr`；**末行是「最近操作（左） | 需要关注（右）」左右并排**。
  「用量分析」页的套餐卡**固定三列**；额度行是「名称 22px | 条 1fr | 百分比 auto」一行栅格、
  条高 6px、百分比在条外；余额总额 14px 粗体、赠金充值降到第二行 10px。判据 `designAlignment.test.js`。

### 概览页

- **状态机只有一个主按钮「工作台」**：文案恒为名词，启停方向由 icon（▶ / ⏸）与 hover
  title 表达；「官方对话」同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」
  （运行后淡入）是并列次级入口，**不要再往主按钮旁边加会启停内核的动作**。
  文案说用户得到什么，代码名说壳做了什么，分开记。
  - **「刷新工作台」已被用户要求隐藏**（这一排里唯一的**动作**：换掉整个工作台窗口 =
    换掉渲染进程）。**入口隐藏 ≠ 能力删除**：`store.js` 的 `forceReloadHarnessWindow` 与
    Rust 的 `harness_force_reload` / `reset_budget` 原样留着，按 `setDefaultInstance`
    那条先例——没有调用方的 API 不许当死代码清掉。手动出路变成两步：关掉工作台窗口，
    再点「工作台窗口」重新打开（同样换掉渲染进程）。判据见 `designAlignment.test.js`。
- **概览只保留「今日用量」一个用量读数**：套餐卡与「模型用量」按钮在「用量分析」页
  （内嵌 `UsageWindow`）；两扇窗口的组件 / 命令 / capability 与窗口路由保留（入口隐藏
  ≠ 能力删除）。
- **次级入口这一排落在「版本号」那一行的右侧、靠右对齐**，落位是**两行网格**
  （`kernel-summary-main`：`minmax(0,1fr) auto`），不是 flex + `margin-left: auto`。
  `justify-self: end` 不能省（网格项默认 `stretch`）。`.btn-row-sub` 的「左缩进 + 2px
  竖线」随搬家删除：它表达「从属于上方那块读数」，现在与版本号同行，竖线会变成一条
  没有归属的孤线。
  - **判据「版本号与状态行同属竖向块」改写过一次，别按旧名字找它**。那条正则真正要拦
    的是「两者并排」而不是「两者相邻」；现在查的是栅格落位（状态行跨两列、落第二行）。
- **「查看日志」不在主操作排，在「系统健康 → 日志系统」，点开的是独立窗口**。
  - 现在这一格调 `logs.js::openLogWindow()`（不带文件名，由窗口自己挑：先 `kernel`
    分组，再任意第一份；后端 `open_log_window` 为此接受空名，`read_log_file` 仍拒绝）。
  - 事故 / 预检 / 诊断页那些**带具体某一份证据**的入口仍然走弹层：那不是「去看看有什么
    日志」，是「去看这一份」。
  - **两个入口共用一份实现**，这是本条最容易被顺手破坏的一半：两个组件各写一份
    `invoke`，迟早一个改了另一个没改。判据见 `logEntryPoint.test.js`。
  - **那一行的右侧提示必须写「查看」而不是 `›`**：它是唯一的日志入口，长成读数样式就
    等于把入口藏了。一份日志都没有时该格**不可点**。
  - 弹层里的「全屏」按钮**不能删**：独立窗口有两条通路，两条都删光不会报任何错。
- 一键重建工作台（换渲染进程）针对的是**看门狗覆盖不到**的那类黑屏，额度清账在
  `harness_window::reset_budget`。入口已隐藏，机制与理由见上面那条。
- **版本号是一段纯文字，不是徽标**：20px / 700 / `var(--text)`。去掉徽标后
  `margin-left: 0` 必须显式钉住——胶囊那段缩进没了，外层不会自动补回来。判据含
  「概览不再引 VersionBadge」。
- **三格指标的动作按钮配色归 CSS 管**，模板上不许挂 `type="primary"`（同一排按钮不许
  一枚蓝两枚黑）。图标槽按 `.button-icon` 取 16px——EP 的 `.el-icon` 是
  `font-size: inherit`，按钮 11px 会把图标一起缩到 11px。指标标题 11px muted 与值
  12px 600 按值钉住防漂。
- **「数据目录」那一格指向应用根 `~/.dsh-xlink`，不是当前实例目录**：实例目录在 141px
  的格子里被 `display_short` 截成 `~/.dsh-xlink/dsh/…`，尾巴上的省略号看着像一条坏掉的
  路径。显示读 `KernelStatus.xlink_home`，实例目录退到 `title`（一个字都不许丢）。

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
7. **判据要钉结构事实而不是字面量**：页头那条比嵌套深度而不是 `</div>` 个数，药丸页签
   那条钉选择器形状而不是当前几行值，灯组那条钉 `width + gap === 23`。守住一个会随
   字号、图标、依赖版本漂移的字面量，等于守一个必然作废的巧合。

## 目录约定

插件页「插件库」视图：页签顶部是安装区块（`npm 包名 / Git 地址` 输入 + 「预检并安装 /
安装」按钮 + 说明，回车与按钮走同一个 `installFromSpec`），下方单栏本机插件清单（名称 +
来源 chip + 每实例状态 chip）。「当前内核」页签显示内置插件与本内核插件行。安装只保留手动
入口，不再有远端目录、搜索、分类、排序或分页。

`ui/src/` **按功能分目录**，不要往根目录平铺。

```
ui/src/
├── main.js / App.vue / store.js / theme.css   # 入口、壳、全局状态，就这四个留根
├── shell/          # 壳自身：导航、概览、设置 + 跨面板共用基础设施
├── kernel/         # 内核版本与实例
├── plugins/        # 插件库、补丁、安装预检
├── skills/         # 技能
├── diagnostics/    # 安全网 P0/P1/P2：环境回退点与二分定位
├── usage/          # 用量分析面板、本地 token 统计
├── subscription/   # 云端套餐用量卡与窗口
├── incidents/      # 工作台事故与任务通知
├── logs/           # 日志查看器
├── migration/      # 数据迁移向导
└── official-chat/  # 官方对话
```

一条功能的状态、动作与面板**放在同一个目录里**。新增功能先问它属于哪一格；确实跨格的
共享件放 `shell/`。

搬文件时同步更新调用方、测试和文档。源码结构判据优先按模块名定位，避免把路径写死；
前端测试的相对路径按 `import.meta.url` 解析。组件按职责拆分，状态与动作沿用现有 store，
同一语义的样式与异步逻辑优先复用；不以模板行数决定是否拆组件。

## 待移除

功能不是永久的。下面是**维护者已决定到期就删**的东西，到期时按这份清单执行。

### 数据迁移（v0.6.0 之后整体移除）

维护者 2026-10-07 决定。过渡期内它是正式功能：**侧栏「系统」组里紧跟「设置」的常驻
菜单**，设置页入口已按维护者要求移除，迁移从侧栏进入。

要删干净一共四块，**光删前端菜单不算数**——Rust 侧的命令与 capability 还挂着：

| 位置 | 内容 | 行数 |
| --- | --- | --- |
| `ui/src/migration/` | `migration.js` / `MigrationPanel.vue` / `MigrationPrompt.vue` | ~1270 |
| `src-tauri/src/migration/` | `wizard.rs` / `home_recovery.rs` / `home_recovery_cmd.rs` / `mod.rs` | ~2845 |
| 入口 | `SideBar.vue` 系统组的菜单项、`SettingsPanel.vue` 那一行、`App.vue` 的面板挂载、`store.js` 里的迁移调用 | — |
| 副作用 | `capabilities` / `commands.rs` 的迁移命令注册、`check-invariants` 与 `ui/test/migrationPanel.test.js`、`misplacedHome.test.js` 等判据、`AGENTS.md` 与 `src-tauri/AGENTS.md` 里关于它的所有段落 | — |

两处**不要**跟着删：`~/.dsh` 搬迁（`legacy_migration_target`，那是 v0.2.x 平铺目录搬家）、
`store_relocate`（插件中央库搬迁）。名字里有 migration 但职责不同，凭名字一起删会出事。

`home_recovery`（找回历史会话）依附本功能，删时一并走；但它读别的实例的 `sessions/`，
**动手前先确认没有用户依赖它**——那是 2026-09-28 事故的回收补救路径。

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

- **用户可见文案用简体中文。** 面板里每一条面向用户的字符串都是产品的一部分。
- **数据目录不许在前端写死。** 展示路径的文案要读后端返回的真实路径（技能面板读
  `SkillStatus.store_root` / `skills_root`，插件面板读 `PluginStatus.store_root`），
  显示前过 `labels.js` 的 `tildePath` 折叠 home。同一句提示的两页只修一页发生过两次：
  `~/.dsh/skills-store/` 搬家后技能页还写着旧目录，同一天插件页还写着
  `~/.dsh-xlink/dsh-plugins/` 而 `store_relocate` 早已把它搬走。现由 `check-invariants`
  第 ⑦ 项之六钉住：前端文案里不得出现**路径上下文**中已搬迁的目录名（`dsh-plugins` /
  `skills-store`），且目录名前要带 `~/` 或 `/`，所以迁移向导把 `skills-store` 当逻辑
  来源 id 用不受影响。
- 告警文案自带可执行的下一步，**模板不要再统一追加「重启应用会自动修复」**：那对
  「清单缺失」是假的（`reconcile` 不会凭空重建 `store.json`），对「被盖住」更是假的
  （重启什么都不会改变内核的 rank 次序）——一句对两条都不成立的建议，比没有更糟。

## 间距

- **功能块之间的默认间距是 12px，由容器统一提供，块自己不再带外边距。** 值就是
  `.panel` 的 `gap: 12px`，栅格容器 `.page-layout` 与列容器 `.page-layout__col` 同样
  12px。**除非用户明确要求特调，新 UI 按它写。**
  - **缝由容器给，不由块给**：`.panel` 里的块一律 `margin-bottom: 0`。
  - **下限 6px**：卡片自带底色 + 1px 描边，描边贴着描边会读成一整块分不开的面。
  - **页签行距不许归零**：插件页 `.installed-tabs .el-tabs__header` 原本是
    `margin-bottom: 0`，实测那条缝只剩 4px，而药丸自带底色，4px 看着像「被切了一截」。
    现在 **10px**，取与 `.card-head { padding-bottom: 9px }` 同一档。这与「归零要显式写
    0」不矛盾：那条讲有 `gap` 的容器里不许再叠 margin，这条讲页签栏与内容区之间那段
    没有 `gap` 兜着的行距。判据 `designAlignment.test.js`。
  - **只有没有 gap 的容器例外**：诊断页那 5 个独立窗口装在 `.diagnosis` 里，间距仍由
    `.diag-card` 自己那 10px 提供。「块不带 margin」不是全局规则，是「**有 gap 的容器
    里，块不带 margin**」。
  - **flex 容器的外边距不折叠，会直接叠在 gap 上**，同一列里出现两种缝。实测改版前的
    概览页是 12/12/12/6。
  - **Vue 的多根组件（fragment）会把每个根节点提升成父容器的直接子级，于是也吃父容器的
    `gap`**（`ControlTower` 是三个根节点）。想「不受 gap 影响」做不到，只能归零后与
    gap 等价。
  - **归零要显式写 `margin-bottom: 0`，不能删掉这条声明**：作用域选择器只在**它自己声明
    过的属性上**赢过基线规则，删掉等于让 `.diag-card` 基线的 `margin-bottom: 10px` 回来。
    这与「保存按钮漏写 `cursor`」「计数漏写 `width`」是同一个级联陷阱。
- **「看着行距大」先分清是 gap 还是行盒。** `gap` / `margin` 只管行**与行之间**那条缝，
  一组行占多高由**每行自己的盒高**决定；面板里的行几乎总是「裸文本 + EP 组件」混排，
  外框都远大于里面的文字。此时继续压 gap 没有视觉收益——改法是**收盒高**
  （`.kv .el-button { height: 20px }` + `.kv { row-gap: 4px }`，横向 padding 一格没动）。
  **别去动 EP 组件的横向 padding**：无底色 `text` 按钮的 padding 在页面上看不见，只负责
  可点面积，要紧就收 `height`。**验的时候量「首行文字顶 → 末行文字底」的总跨度**，不量
  `gap` 的属性值。新写列表 / 卡片 / 设置项时同理：先把盒高对齐，再谈 gap。
- **量 CSS 之前先确认 dev server 真的在服务改后的文件。** 旧 vite 进程按缓存发 CSS 时，
  `curl localhost:5174/src/diagnostics/diagnostics.css` 拿到的还是改动前那条规则，
  截图里「好像有线」的那条其实是 body 的背景网格。判据是 **curl 到的规则原文与磁盘
  一致**，不一致就换端口重起，不要猜「是不是 HMR 没生效」。这个坑只在**纯 CSS 改动**
  上出现，`.vue` 走 HMR 反而正常。
- **CSS 注释里不许出现字面的块注释记号**（注释不嵌套，第一个收尾符会提前截断注释，余下
  文本并进下一条规则的选择器、整条被浏览器丢弃）。判据按浏览器语义扫全仓 CSS 注释
  （`designAlignment.test.js`）。

## 跨前后端的契约

这几条在两侧各有一半，改任一侧都要同时看另一份：

- **右键策略按窗口族分两份**：壳自己的五个窗口走 `ui/src/shell/noContextMenu.js`
  （`main.js` 在 `app.mount` 之前调一次，一律取消），这一半归本文件；官方对话三个内容
  webview 加载的是**别人的**页面，走 Rust 注入的 `no-context-menu.js`（一律取消，含
  页面自绘菜单）；**工作台**加载的是内核 Web UI、它自己有右键功能，走 Rust 注入的
  `context-menu-fallback.js`：放行页面自绘菜单、压掉引擎原生菜单，页面没处理时若现场
  可编辑或有选区，再 invoke `show_workbench_edit_menu` 弹壳的原生编辑菜单（撤销 /
  重做 / 剪切 / 复制 / 粘贴 / 全选，只读选区只给复制）。
  `check-invariants` 第 16 项按「每条 `WebviewUrl::External` 建窗链」逐条查**接的是
  不是本族那一份**——**另一半见 [src-tauri/AGENTS.md §右键策略](../src-tauri/AGENTS.md)**。
- **预检三态**：`precheck.rs` 的 `Verdict::as_str()` 与 `PrecheckDialog.vue` 的判定表靠
  字符串对齐，有测试钉死（`precheck::tests::verdict_strings_match_the_ui_contract`），
  改一边不改另一边会把「未通过」画成「通过」。判定为什么必须三态、基线为什么必须先跑，
  见 [src-tauri/AGENTS.md §预检](../src-tauri/AGENTS.md)。
- **「今日用量」只有一套口径**：概览卡片与「用量分析」页内嵌的模型用量（窗口路由同源）
  都读后端的 `today_tokens` / `today_requests`（`usage.js::todayUsage`）。窗口此前取
  `sliceDays(days, 1)` 的最后一天，而后端会把晚于今天的异常日期追加到序列末尾——同一份
  统计会显示成两个数。`check-invariants` 第 ⑦ 项之五钉住接线。
- **外部 agent 工具的用量来源是显式许可，默认不扫**（另一半见根 `AGENTS.md`）。模型用量
  区域顶部一行 checkbox 勾「Codex」与「MiniMax Desktop」，**默认一个都不勾**。**勾选状态
  只有一份真值**：Rust 侧持久化的那份（`shell/<mode>/usage-external-sources.json`），前端
  只是镜像，**每次拉取以后端返回的 `external_sources` 回填**。勾选变化时**先落盘再重扫**
  （`usage.js::setExternalSources`），否则出现「界面上勾了、盘上没存」的窗口期。**`loadUsageSummary`
  显式传 `external: { codex: false, minimax: false }`**：概览卡片不显示外部来源，不该付扫描成本
  ——省略参数的语义是「按已保存的偏好扫」。外部来源在详情页里是**独立一块**、**不与内核
  那份相加**；它的数字跟时间范围切换器走（`sliceDays` + `summarizeDays`，90 天合计只留
  tooltip）。**另有 90s 新鲜度窗口**：`refreshUsage(external, externalForce)` 的第二参数即
  Rust 的 `external_force`——**进页面吃缓存**（默认 false），**「刷新」与勾选变化真重扫**
  （true）；刚勾的来源还没账，吃缓存会空卡。判据 `ui/test/usageExternal.test.js`。
- **同一屏里两套账，每块都得标来源**（2026-10-24 用户要求）。两边名字都带「MiniMax」
  （内核自己也在调它的模型；provider 列内核是 `minimax-cn`、外部是 `minimax`），
  光看列名分不出哪一行是谁的账。⇒ 内核那几块（活跃热力图 / 按天 Token 趋势 / 模型用量）
  各挂一枚 `.usage-source-chip`「内核」牌；外部那几块**不挂**「内核」牌，来源由卡片
  标题（`src.label`）给出。行的悬停说明也要带来源。判据同上那条的第 ⑧ 项。
- **模型用量区域的内容区必须能滚**。窗口 760×800（`USAGE_VIEWER_SIZE`）装不下「概览 +
  趋势 + 模型明细 + 外部卡片」四段，`.usagewin-main` 曾是 `overflow-y: hidden`，最后一段
  被**整段裁掉**。⇒ 纵向溢出**回到全局约定**（`theme.css` 里 `main { overflow-y: auto }`）；
  窗口模式保留 `scrollbar-gutter: stable` + `thin`，内嵌模式（`inline`）则把滚交回面板
  `main`（`.usagewin-main` 必须 `overflow: visible`，两处各写一条会出双滚动条）。判据
  `ui/test/usageExternal.test.js` 取**层叠后的生效值**——别的规则再写一条 `hidden` 就盖
  回去了。
  ⇒ **加新段落前先问「它落在这屏的第几屏、装得下吗」**：DOM 里有元素 ≠ 用户看得见，
  判据覆盖的是「有没有渲染」，不是「在不在可视区里」。
- **几个外部工具合在一张卡，模型明细合成一张表**（2026-10-25 用户要求；并排多挤、
  为何整段重排的经过见 [agent-lessons §37](../docs/reviews/agent-lessons.md)）。
  ⇒ `.usage-external` 是**一张卡占满整幅宽度**（不许再是 `auto-fit` / `repeat()` 的
  多列栅格），每个工具各占**一块**（自己的合计、自己的按天柱条）；**模型明细合成一张表**
  （`externalModels`），每行带一列「工具」。**占比是「占该工具自己」的比重**（两个工具
  token 口径不同，合起来算没有意义）。
  ⇒ **这张表与内核那份模型列表都不自带纵向滚动条**（2026-10-25 用户要求）：行一多把
  整页撑长，由页面 `main` 统一滚（窗口模式为 `.usagewin-main`）；**main 里各段不许被压扁**
  （`flex-shrink: 0`）。判据 `ui/test/usageExternal.test.js` 最后两条。

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
