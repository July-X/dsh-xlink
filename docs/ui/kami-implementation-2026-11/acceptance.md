# 实施验收与可执行检查

本页规定**后续 UI 实施**怎样验收。本设计 PR 只执行 §1 的文档检查，不把未改的应用、概念图或静态代码检查说成视觉还原通过。所有结果记录 commit、平台、主题、窗口大小、缩放、夹具和证据路径。

## 1 本设计 PR 即可运行

在仓库根运行，无新增依赖，不调用账号、内核或真实用户目录：

```sh
node docs/ui/kami-implementation-2026-11/verify-spec.mjs
git diff --check
npm run check:invariants
npm run check:duplicates
```

verify-spec 检查 JSON、两主题键、本文局部链接、参考图摘要/尺寸、纯色与填色文字对比度。它不是浏览器测试，也不证明全部透明叠层或屏幕阅读器通过。

## 2 后续实现的静态与构建检查

按阶段运行受影响测试。下列均是现有命令/文件；更新与本方案明确冲突的视觉断言，不能删除测试以求通过。

```sh
node --test ui/test/themeContract.test.js ui/test/darkThemeB.test.js ui/test/epStyles.test.js ui/test/designAlignment.test.js ui/test/cardChrome.test.js ui/test/panelSpacing.test.js ui/test/logWindowTheme.test.js ui/test/minimaxWordmark.test.js
node --test ui/test/pluginTargets.test.js ui/test/builtinPlugins.test.js ui/test/applyPluginChange.test.js ui/test/pluginInstallCleanup.test.js ui/test/skillEnable.test.js
node --test ui/test/memoryModule.test.js ui/test/memoryPool.test.js ui/test/memoryTargetUse.test.js ui/test/memoryPanel.test.js ui/test/memoryEffect.test.js
node --test ui/test/usageExternal.test.js ui/test/usageStats.test.js ui/test/usageAnalysisPanel.test.js ui/test/usageOverviewDensity.test.js ui/test/settingsPortDrafts.test.js ui/test/settingsPortGroups.test.js ui/test/familySwitchInteraction.test.js ui/test/diskUsageTree.test.js
npm run check:invariants
npm run check:duplicates
npm run build:ui
```

跨后端能力修改还要按根 AGENTS 运行对应 Rust debug 单测、编译与 Clippy。全量 `npm run check` 仍只在发布前跑，不因文档 PR 伪造已运行结果。

必须补充的断言：

- tokens.json 中每个色彩键在实际 CSS 两主题存在，无自引用/废弃 token；geometry 不出现在 dark 选择器里。
- 主按钮深色为深文字，输入/checkbox/switch 边界不是低对比度账页线；透明叠层合成后文字仍 ≥4.5:1，控件与焦点 ≥3:1。
- 七页 page-desc 从视觉正文移到可访问提示，真实警告/许可/范围/不支持未被误删。
- 插件一个列表、两个来源类型 key 不撞、只有内置徽标；自装 0/1/20 条时内置仍出现一次。
- disabled 目标不发写请求、取消自装勾选不调用卸载、记忆目标镜像不产生第二条授权写链。
- 安装 Enter 与 click 同入口，快速重复只发一项任务，预检 planId/三态及关闭恢复保持。
- 记忆公共池来源确认、部分成功、旧范围不扩权；导出/清空依旧 DSH；技能保留包内逐项控制。

反向验证：故意去掉某个暗色 token、把 unsupported 改成 enabled、删掉一行内置徽标/多复制一行、让已选按钮白字黄底、注入超宽元素。分别确认命中目标断言，再恢复文件重新构建；编译错误不能当成视觉断言的反向命中。

## 3 隔离环境与真实 WebView

遵守 [开发验证](../../operations/development.md#数据与浏览器档案隔离)，通过仓库 launcher 创建临时 DSH_XLINK_HOME、appDirectoriesOverride、独立通道/端口和浏览器档案。不要直接裸启动 wdio，更不能拿用户真实会话/凭据作为截图夹具。

现有真实 WebView 检查可这样执行（需要 Windows 或 Intel macOS 的桌面/工具链）：

```sh
npm run e2e -- --keep --spec e2e/specs/layout.spec.mjs
npm run e2e -- --keep --spec e2e/specs/panels.spec.mjs
npm run e2e -- --keep --spec e2e/specs/memory.spec.mjs
```

当前 layout.spec 只覆盖部分布局，不等于下述七页矩阵已经自动化。实施者应在同一 e2e 框架增加七页 screenshot/geometry 场景并从 launcher 运行，不能只留下本说明。

MiniMax 实跑走已有 smoke spec，按 launcher 当前受支持版本传受管包/Node 的绝对路径；当前基线 parser 限制 `@minimax-webui/minimax-code@0.1.7`，不能从概念图的 0.1.8 推断允许版本。调用前重查 parser；缺包或平台记 not_run，不从用户家目录找替代包。

纯 `npm run dev:ui` 可验证 CSS 和受控模拟数据，**无 Tauri 桥**，不能作为安装、授权、原生窗控或真实读写验收。模拟结果与真机结果分别存档。

## 4 统一截图夹具

由实施者在隔离测试环境或专用 UI fixture 中准备，产品代码不得落假数据。真实 IPC 测试不得靠 fixture 覆盖真实返回。所有图片标明 `fixture` 或 `real-webview`。

| 夹具 | 内容 |
| --- | --- |
| 正常 | DeepSeek 已安装停止；MiniMax 体验开且已安装停止；4 内置+2 自装插件；1 包含2技能的包；2来源各2条普通记忆；1待审核；可用套餐2家；7天模型数据 |
| 忙碌 | 内核启动中、安装进度、更新中、端口保存中；全局/单动作 loading 分开 |
| 空 | 无已装内核、无自装/技能/记忆/用量；从未配置的套餐分区不伪装0余额 |
| 失败 | 各源首次读失败与持有旧值后刷新失败；部分成功；预检失败；MiniMax unavailable |
| 边界 | 20自装、30技能、100记忆、30模型、5以上套餐窗口；100字符名称、200字符无空格路径、999999999 token；真实长中文告警 |
| 权限 | 体验关闭/构建不带、公共池沿用旧范围、首次授权取消、来源改变、旧逐条授权、外部统计未授权 |

固定测试时间与数字、禁掉光标闪烁和非必要动画仅限 fixture。fixture 内容不得使用真实账号、Key、余额、个人记忆或聊天文本。

## 5 逐页视觉矩阵

至少交付 **7页 × 2主题 = 14 张** 1040×748、展开157侧栏的正常态截图；每页再交付两主题的收起64侧栏截图（共28张正常态）。长页补底部与完整内容截图，不用首屏证明末尾控件存在。

| 页 | 首屏必须看见 | 补充证据 |
| --- | --- | --- |
| 概览 | family真实状态、当前内核、健康、今日用量 | 关注项/最近操作底部；停止/运行/启动失败；两内核选择各一张 |
| 用量 | 套餐行、许可、模型范围 | 底部趋势/模型/外部工具表；未授权、部分provider失败、空数据 |
| 内核 | 已安装/官方列表、族切换 | 磁盘四组与展开树；列表失败/空；切族后磁盘数不变 |
| 插件 | 一张统一列表、内置徽标、逐核支持状态 | 自装0与20条；用途tooltip；预检三个结果；账号动作/更多菜单 |
| 技能 | 每包内逐技能目标、来源/版本 | 无虚构目录；长列表、冲突恢复、MiniMax待生效、topic/安装入口 |
| 记忆 | DeepSeek启用词、权限范围、tabs、来源 | 待审核/效果/编辑/详情/授权确认；右侧诊断末尾；长列表 |
| 设置 | MiniMax粉色组、主题三选、端口保存 | 通知与环境区底部；Windows Defender告知；保存失败/未连接 |

另需两主题各覆盖 tooltip、select、dropdown、confirm、dialog、全屏进度、局部loading、toast/error、focus-visible、disabled（至少每类一张）。打开浮层后切主题再拍，验证 Teleport 不残留浅色底。

### 5.1 与概念图的对照方法

1. 将实际截图与对应 reference **并排**审查，不用 stretch 把生产窗口拉成图片尺寸。概念图适配到同宽显示，比较区块顺序、视觉重心、密度、颜色与留白。
2. 用 README/`tokens.json` 测量尺寸；相对区块位置允许内容产生的高度变化。不要设「生成图像素差 <1%」这种错误门槛。
3. 为每页记录 `match / deliberate_difference / defect`。`deliberate_difference` 必须指到 references.md 的纠错项，如不存在的技能搜索、假活动时间线、未支持开关。
4. 实施后经审查通过的真实同平台 fixture 截图才可成为回归 golden。Win 与 mac 分开基线，字体栈不同不能互相比像素。
5. golden 同viewport/DPR/字体/数据比较。动态时间只可在fixture固定，不能随便 mask 整个内容区。局部像素差超过0.5%须人工审查并解释，不能自动更新golden让测试变绿；几何/功能失败无论像素差多少都失败。

### 5.2 必须实际测量的几何

由浏览器 `getBoundingClientRect` / `getComputedStyle` 输出 JSON，不只用肉眼判断：

- main `scrollWidth <= clientWidth + 1`，页面/app 不得横溢出。
- main 内容左边 = sidebar右边+24（紧凑520宽时+16），标题栏高38、侧栏157/64，误差≤1px。
- 主题切换前后同数据的 h1、分区、行、按钮 rect 四维差≤1px；长列表不得重排。
- 真实按钮 scrollIntoView 后完整处于可见区，中心点 elementFromPoint 为自身/子元素；不能被透明遮罩拦住。
- 文本按钮高30，图标命中区至少32，表头32，普通行≥48；中文不裁字。
- 记忆/用量长页最后动作可达，弹窗页脚位于视口内；短窗内容由body滚而不是页脚被挤出去。
- 容器宽719/720交界、插件767/768交界、viewport899/900交界各拍一次；520×748、1040×600、1280×900仅作为浏览器/fixture压力测试，不改原生配置。
- 两平台100%与125%系统缩放；文字200%时不隐藏动作/状态，页面可滚。

## 6 行为脚本与预期

每项用真实支持的 action/合成测试数据验证。非只读动作只在本轮隔离 home，不改真实登录项/Defender/生产内核。

| 编号 | 操作 | 必须结果 |
| --- | --- | --- |
| B01 | 亮→暗→系统，重开主窗/日志/对话框；系统主题改变 | 内容与原生窗口遵循各自既有作用域，无白闪；固定模式不随系统；几何不变 |
| B02 | 七页往返，侧栏收起/展开；FamilySwitch 左右/Home/End | 所有入口保留、焦点跟随；概览切核不篡改内核版本页选择；共享数据不分叉 |
| B03 | DeepSeek/MiniMax 启停、运行时打开窗口、失败重试 | 既有状态机、进度/错误、停止后恢复成立；未返回状态不猜已停止 |
| B04 | 内置4项逐核，尤其不支持和记忆镜像；快速点两次 | 只对可用目标写一次正确动作；失败回读；镜像只导航记忆页；未支持无调用 |
| B05 | 自装+内置同名id夹具、更新/恢复/卸载取消与确认 | key不撞，动作kind分派不串；内置无卸载；取消不写；自装只读勾选不可执行卸载 |
| B06 | npm/Git 安装输入、Enter、预检三态、取消/重试、断网 | 仍按planId与真实报告apply；失败进度保持可读，日志入口可用，不双重安装 |
| B07 | 同一包两个技能分别切核；本地重同步；冲突恢复 | 保留逐技能标记；DSH即时/ MiniMax下次启动口径；不删除冲突源文件 |
| B08 | MiniMax体验关/开，未安装与状态读失败 | 能力入口/禁用原因正确，偏好不丢，不隐式安装或授权 |
| B09 | 套餐逐家失败/重试；切今日/7/90天；两个外部许可分别开关 | 缓存和强制重扫一致；未授权不扫描；不把外部量与内核量相加，今日与概览一致 |
| B10 | 公共池首次开启取消，再确认；确认中来源变化 | 取消零授权；来源变动需重新确认，不把旧范围默认为全池 |
| B11 | DSH memory插件接入成功但池写失败；关闭单核 | 部分完成而非假成功；关闭不删记录/不关另一核，回读不确定明确呈现 |
| B12 | 记忆编辑、待审核冲突、切来源、交接/经验、效果详情 | 原动作都可达；来源/expectedRevision/项目范围不丢；不编造MiniMax提取数据 |
| B13 | 导出 DSH 并核对快照范围；清空打开确认后取消；旧授权查看/撤销 | 作用域仍DSH/指定旧授权；清空取消不删；导出不新增确认；没有新逐条共享创建入口 |
| B14 | 修改三端口，后台状态刷新，保存失败再试 | dirty草稿保留；整数范围/冲突明确；保存按钮只保存端口，不影响即时开关 |
| B15 | 通知关、声音关、前后台条件、dev/release | disabled依赖正确；声音关不能试听；release无模拟按钮；来源及断流告警保留 |
| B16 | 连续打开/取消弹窗、按既有关闭规则测试Esc、切页再返回、失败后重试 | 焦点回触发者、无残留遮罩、草稿按原行为；MemoryEditor仍禁止Esc/遮罩关闭，不能绕过草稿保护；不能退到失效页面或重复IO |

## 7 无障碍与降级

- 仅键盘走完导航、两个family、插件目标、技能目标、来源筛选、tab、更多菜单、编辑保存/取消。焦点顺序必须与视觉顺序一致；CSS order 造成读屏错序需修DOM，而非隐藏焦点。
- tabs 有 tablist/tab/tabpanel 对应关系和 roving focus；checkbox/switch 的 accessible name 含对象与内核；disabled 原因由旁边可聚焦提示读取。
- tooltip 不只依赖鼠标；焦点可显示，Esc可关闭；错误文本以 aria-live/alert 适度宣告，不轮询时重复吵读屏。
- 图表必须有日期、数值、来源的键盘/文本等价信息；颜色不是唯一信息，零值/未知/无权限分开。
- Windows 高对比、prefers-reduced-motion、无 backdrop-filter：仍有边框、选中标识和焦点；glass退到实色。对透明背景先合成再算对比，不能只验原始rgba。
- Windows ClearType 与 macOS 字体分别人工观察，品牌词不截断；不要把系统字体差异误当“必须换中文字体”。

## 8 证据与交付格式

实施 PR 附一份 `validation.json`/Markdown 报告，按下面字段填写，不提交真实秘密/家目录数据：

```json
{
  "commit": "实际40位SHA",
  "platform": "Windows或Intel macOS",
  "viewport": [1040, 748],
  "scale": 1,
  "dataMode": "fixture或real-webview",
  "checks": [
    {"id": "V-plugins-dark", "status": "pass/fail/not_run", "evidence": "本轮证据相对路径", "note": "实测结果"},
    {"id": "B10", "status": "not_run", "evidence": null, "note": "明确缺失前提"}
  ]
}
```

通过条件：P1–P5 对应定向检查与构建通过；14+14正常态和必要状态截图已审；所有概念图差异有依据；行为及无障碍矩阵完整记录。真实桌面平台缺失时可提交阶段 PR，但明确 not_run，不能称“全部还原/双平台验收通过”。不合并、不发版、不改tag，除非后续另有授权。
