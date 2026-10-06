# AGENTS.md — `ui/`（管理面板前端）

根 [AGENTS.md](../AGENTS.md) 的前端分支。根文件里的「范围」「数据目录」「发布」各章对前端同样适用，这里只收改 `ui/**` 之前要知道的约束。后端见 [src-tauri/AGENTS.md](../src-tauri/AGENTS.md)。

技术栈：Vue 3 + Element Plus 单页应用，Vite 构建到 `ui/dist/`（即 `tauri.conf.json` 的 `frontendDist`）。

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

- **功能块之间的默认间距是 6px，由容器统一提供，块自己不再带外边距。** 这是 2026-10-06 用户拍板的默认约定，**除非用户明确要求特调，否则所有新 UI 按它写**。参照物是概览页「当前内核 ↔ 套餐用量」那一段（当时页面里最小、也最齐的一段），值就是 `.panel` 的 `gap: 6px`。
  - **缝由容器给，不由块给**：`.panel` 里的块一律 `margin-bottom: 0`，新块也不要自带 `margin-bottom` / `margin-top`。已经这么写了就不要再逐个调块的内边距去凑。
  - **下限 4px，不要更小**：卡片自带底色 + 1px 描边，描边贴着描边会读成一整块分不开的面，而不是两张卡（这条下限原本记在 `theme.css` 的 `.panel` 注释里，2026-10-06 升格为全前端约定）。
  - **只有没有 gap 的容器例外**：诊断页那 5 个独立窗口装在 `.diagnosis` 里（`position: fixed` 的另一套容器，不吃 `.panel` 的 gap），它们的间距仍由 `.diag-card` 自己那 10px 提供。「块不带 margin」不是全局规则，是「**有 gap 的容器里，块不带 margin**」。
  - 由此引出一条反模式：**flex 容器的外边距不折叠，会直接叠在 gap 上。** `.panel` 的 `gap: 6px` 撞上块自己的 `margin-bottom: 6px` 就是 12px，同一列里两种缝——实测（dev server + `getBoundingClientRect`）概览页原本正是 12/12/12/6，肉眼一眼看出不齐。改动只是把那三条 `margin-bottom` 归零，实测变成 6/6/6/6。
  - **还有一条容易漏的**：Vue 的**多根组件**（fragment）会把每个根节点提升成父容器的直接子节点，于是它们**也吃父容器的 `gap`**。`ControlTower` 就是三个根节点，于是三张卡都被算进 `.panel` 的 flex 布局——想「这三张卡不受 gap 影响」是做不到的，只能让它们归零后与 gap 等价。
  - **归零要显式写 `margin-bottom: 0`，不能删掉这条声明。** 作用域选择器只在**它自己声明过的属性上**赢过基线规则；删掉声明等于让 `.diag-card` 基线的 `margin-bottom: 10px` 原样回来。这与「保存按钮漏写 `cursor`」「计数漏写 `width`」是同一个级联陷阱。
- **「看着行距大」先分清是 gap 还是行盒，再动手。** `gap` / `margin` 只管行**与行之间**那一条缝，一组行实际占多高由**每行自己的盒高**决定；而面板里的行几乎总是「裸文本 + 挂了 EP 组件」混排——`el-button`（small 外框 24px）、`el-switch`、`.el-tag` 自带的外框高度都远大于里面的文字（按钮文字只有 12px，旁边的裸文本行是 13px × 1.5 = 19.5px）。行高被盒高最高的那一行撑开，于是缝看着大，**此时继续压 gap 没有任何视觉收益**。2026-10-03 概览「当前内核」卡就是这一种：kv 行里三个 EP small 按钮的 24px 外框把整组行撑开，把行距收小后跨度几乎没动，才回头查出根因在行盒。改法是**收盒高而不是动 gap**（`.kv .el-button { height: 20px }` + `.kv { row-gap: 4px }`，横向 padding 一格没动），五行行距 26/29/29/30 → 24/25/23/24、首行文字顶到末行文字底的跨度 126px → 109px。同一批里的开关行也是照这条处理的（收行盒而不是调 margin）。**新写列表、卡片、设置项时同理**：先把每行的盒高对齐，再谈 gap。
- **别为了「看起来紧一点」去动 EP 组件的横向 padding。** 无底色的 `text` 按钮的 padding 在页面上根本看不见，它只负责可点面积——为了排版动它，视觉零收益、点击区还倒小了。要紧就收 `height`，那是看得见的。
- **验的时候量「首行文字顶 → 末行文字底」的总跨度，不是量 `gap` 的属性值。** gap 调小 4px 而跨度没动，就说明找错了成因，该回头查行盒。同一组行之间还要逐段量间距是否齐——不齐通常是某一行的盒高特殊（多挂了个 EP 组件、多行文字），而不是 gap 写得不一致。
- **量 CSS 之前先确认 dev server 真的在服务改后的文件。** 2026-10-06 踩过：改完文件后旧的 vite 进程仍按缓存发 CSS，`curl localhost:5174/src/diagnostics/diagnostics.css` 拿到的还是改动前那条规则，页面 reload 也一样——截图里「好像有线」，那其实是 body 的背景网格，差点据此下结论。判据是 **curl 到的规则原文与磁盘一致**；不一致就换端口重起（`npx vite --port 5199 --strictPort`），不要猜「是不是 HMR 没生效」。这个坑只在**纯 CSS 改动**上出现，`.vue` 走 HMR 时反而正常，所以更容易误判成「我改错了」。

## 卡片

- **每张卡片的标题与内容之间必须有 1px 分割线。** 2026-10-06 用户拍板：标题行下面没有线，卡片就只是一块底色加一行字，读起来像「这段文字飘在卡里」而不是「这是一个标题和它的内容」。全前端只有一种写法，样式照 `theme.css` 的 `.card > h2` / `.card-head` 抄：

  ```css
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  ```

  - **色值只有 `--border`（`rgba(255,255,255,0.1)`）一种，不要为标题线另开一个更深的 token。** 它画在卡片**内部**、两侧同为卡片底色，看着淡；而同一个值画在卡片**边缘**（`.card` 自身描边）就相当清楚——差别在对比环境，不在线的强度。**先按 `--border` 写，用户真嫌淡再谈加深**。
  - **`padding-bottom: 6px` 与那条线是一套，不能只写 `border-bottom`。** 6px 是把标题文字和线分开的那段留白；只给线会让它紧贴文字。
  - **「线到内容」的距离不由这条规范钉死**，交给各卡片自己的间距机制：`.card` 靠容器 `gap`，`.diag-card` 靠标题的 `margin-bottom`。所以同一页里出现 4px（概览页的 `.card` 被 `OverviewPanel.vue` 的 scoped `padding: 6px 8px; gap: 4px` 收过）与 8px（其余卡片）两种是**对的**，与「各行盒高本来就不同」同理。要统一成一个数字就得反过来改容器的 gap，那会把整卡内容挤一遍。
  - **覆盖范围：概览页与四个面板里的卡，不含 5 个独立诊断窗口**（用户 2026-10-06 明确要求）。`.diag-card` / `.diag-card__title` / `.diag-row` 被那 5 页共用，它们装在 `.diagnosis` 容器里，是另一套布局与密度。**给概览页的控制塔补线要限定在 `.diag-card--tower` 上**——改基线 `.diag-card__title` 会顺带改掉那 5 个窗口，属于明令禁止的顺带变更。
  - **补线时注意 `margin-bottom` 该删覆盖还是该改值。** `.diag-card__title` 基线是 `margin: 0 0 8px`（本身没有 border），所以 `--tower` 版本补上 `padding-bottom` + 线之后，那个 `margin-bottom: 5px` 覆盖应当**删掉**、让它回到基线 8px，而不是另写一个数字。8 不是新数——它就是 `.card` 基线 `gap: 8px` 的同一个数在另一处的落点。
  - **别用「某条规则写了 `border-bottom`」当判据，要查层叠后的生效值。** 有一条规则声明了、又被同特异度的后一条盖掉，是静默的（见 `ui/test/panelSpacing.test.js` 那套层叠判据的做法）。

## 概览页

- **状态机只有一个主按钮「工作台」**：文案恒为名词，启停方向由 icon（▶ 启动 / ⏸ 停止）与 hover title 表达；同排的「官方对话」用同规则（💬 打开 / ⏹ 关闭），「工作台窗口」「官方对话窗口」（运行后从下方淡入）与「查看日志」是并列的次级入口（都不改变内核状态），不要再往主按钮旁边加会启停内核的动作。**「刷新工作台」也在这排**：它是这一排里唯一的**动作**而非「把窗口带到台前」，之所以仍归这排而不是主按钮旁边，正因为它只换窗口、不碰内核状态。按钮文案是「刷新工作台」、后端命令名是 force reload——文案说用户得到什么，代码名说壳到底做了什么，两件事分开记。
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
- 应用自身图标只从 `assets/*.svg` 母版生成，规则见 [docs/icon-design.md](../docs/icon-design.md)。
