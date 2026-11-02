# UI 显示基线诊断（M0 / UI-DPI-00）

按 [UI 显示质量优化 Handoff](dsh-xlink-ui-hidpi-font-rendering-handoff.md) §5 执行第一轮。
**本阶段禁止修改源码，本文件只记录事实与归类，不改任何生产代码。**

## 0. 基线记录

| 项 | 值 |
| --- | --- |
| 记录时间 | 2026-11-02 12:40 ~ 12:55（Asia/Shanghai） |
| commit / 分支 | `18913477` / `main`（`origin/main..HEAD` ahead 2：另一会话的 prestart 移除与侧栏「编号→图标」改造） |
| 工作区 | 干净（另有 2 个未跟踪 handoff 文档，不参与基线判定）。`18913477` 只改侧栏视觉，与字体 / DPI / 窗口路径无关 |
| 版本 | `package.json` / `tauri.conf.json` / `Cargo.toml` 均为 0.7.1-rc.1 |
| 运行中的壳 | release 安装版 `%LOCALAPPDATA%\dsh-xlink\dsh-xlink.exe`，v0.7.1-rc.1，`--autostart` 拉起（12:34）；内核 0.2.2-alpha.2，端口 3090 |
| 构建模式 | 已安装 **release**；无 WebDriver、无 DevTools 通道 → 浏览器侧诊断标 `not_run` |
| OS | Windows 11 专业工作站版，build 26300 |
| 显示 | 主屏 4K（虚拟化读数 2194×1234）@ **175%**；副屏竖屏、缩放与主屏不同；两屏不同 DPI |
| WebView2 Runtime | 156.0.4314.58 |
| 进程 DPI 感知 | `GetProcessDpiAwareness` = **2（per-monitor aware）** |

> 源码基线取 `18913477`；它与运行中的 v0.7.1-rc.1（`d0d6a650`）之间只差 prestart 移除与
> 侧栏图标改造，**均不涉及字体 / DPI / 窗口建窗路径**，因此本基线对显示面同样成立。
> 另一会话正在同一工作区并行作业，M1 开工前需重核行号与 `git rev-parse HEAD`。

## 1. 渲染架构与建窗路径

四扇壳自有窗口（主壳 + 日志 / 模型用量 / 套餐用量）统一 `decorations(false) + shadow(false)`，
自绘 chrome 由 `appearance.rs` / `ViewerShell.vue` / `WindowTitleBar.vue` 承担；透明与圆角按平台分叉。

| 窗口 | label | 建窗位置 | 逻辑尺寸 | 透明策略 | 底色 |
| --- | --- | --- | --- | --- | --- |
| 主面板 | `main` | `tauri.conf.json`（macOS 基线）+ `tauri.windows.conf.json`（Windows 覆盖） | 1040×748 固定不可缩放 | macOS `transparent:true`；Windows `transparent:false` | macOS `#00000000`；Windows `#0a100f` |
| 日志 | `log-viewer` | `commands.rs`（open_log_window，`decorate_transparent`） | 960×720，可缩放 | macOS 透明；Windows 不透明 | 同上，`decorate_transparent` 的 `#[cfg]` 分支 |
| 模型用量 | `usage-viewer` | `usage/local.rs`（`decorate_transparent`） | 760×800 | 同上 | 同上 |
| 套餐用量 | `subscription-viewer` | `usage/subscription.rs`（`decorate_transparent`） | 960×720 | 同上 | 同上 |
| 工作台 | `harness` | `harness_window.rs`（两条建窗链共用 `build`） | 1280×840，可缩放 | 常不透明 | `CHROME_BACKDROP`（#16171a，加载第三方内核页） |
| 官方对话 | `official-chat-*` | `commands.rs` 的 `WindowBuilder` + 内容 child webview | 按布局 | 常不透明 | `CHROME_BACKDROP` |
| `window.open` 弹窗 | `workbench-popup-*` | `harness_window.rs`（`NewWindowResponse::Create`） | 继承 opener | 常不透明 | `CHROME_BACKDROP` |

四扇壳自有窗口的平台分叉只有一处事实源：`shell/window.rs` 的 `decorate_transparent`
（Windows 分支 `transparent(false)` + `#0a100f`；非 Windows 分支 `transparent(true)` + 全透明）。
主窗由平台配置文件覆盖。

## 2. Windows 不透明策略生效核对（源码 + 实机样式位）

- 配置：`tauri.windows.conf.json` 把 `main` 覆盖为 `transparent:false` + `backgroundColor:#0a100f`，
  其余字段与 `tauri.conf.json` 逐项一致（平台配置整体替换数组，已核对）。
- 源码：`decorate_transparent` 的 Windows 分支同上；`ui/src/main.js` 给 `body` 打 `platform-windows`，
  `theme.css` 把 `--window-radius` 归零。
- 实机（本次运行中的 v0.7.1-rc.1，DPI-aware 探针）：
  - 主面板窗口 `('Tauri Window', 'Dsh-Xlink')`：物理 1820×1309 @1.75 → **逻辑恰好 1040×748**；
    `WS_EX_LAYERED` 未置位。
  - 工作台窗口：物理 2264×1534 → 逻辑 1293.7×876.6（用户调整过大小）；`WS_EX_LAYERED` 未置位。
  - 本进程所有顶层窗口里唯一置 `WS_EX_LAYERED` 的是托盘图标窗口（class `tray_icon_app`），
    与内容窗口无关。
- 结论：**Windows 不透明策略在运行产物里确实生效**；ClearType 的实际观感需要放大截图的
  人眼 / 截图判据（M5），本轮 `not_run`。
- macOS 透明 + CSS 圆角路径：源码可确认（`decorate_transparent` 非 Windows 分支 +
  `tauri.conf.json`），本机无 macOS → `not_run`。

## 3. DPI 现状

- 进程 per-monitor aware（=2）；Tauri 建窗尺寸按逻辑像素，实测主窗 1040×748 逻辑在 175% 下
  精确成立 → **没有「CSS 尺寸又乘一遍 DPR」**。
- 全仓 `ui/src` 搜索 `devicePixelRatio` / `scaleFactor` / `--force-device-scale-factor`：
  **0 命中**；搜索 `zoom:`：0 命中；没有整体 `transform: scale()`（见 §5）。
- 多显示器：主屏 175%，副屏竖屏且缩放不同 → 跨屏 DPI 切换是真实场景，但当前窗口都在主屏，
  **跨屏未实测（`not_run`）**。
- 100% / 125% / 150% / 200% 档位未测（主屏 175% 可测，其余需改系统设置；按纪律不动用户
  系统设置，留给 M5 或用户配合）。
- 风险判断：源码层无重复缩放；风险集中在跨屏切换后的 WebView2 栅格化与窗口尺寸，
  属「需实机验证」。

## 4. 字体体系与回退

Token（`theme.css`）：

| token | 值 | 用途 |
| --- | --- | --- |
| `--font-serif` | `"Songti SC", "STSong", "Noto Serif CJK SC", Georgia, serif` | 页标题 31px / 块标题 15px / 品牌名 |
| `--font-mono` | `ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace` | 编号、日志、代码 |
| body | `-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", "Noto Sans SC", sans-serif` | 正文 14px |
| 字号档 | `--fs-page-title:31 / --fs-block-heading:15 / --fs-block-title:16 / --fs-subtitle:15 / --fs-desc:13 / --fs-page-desc:12 / --fs-action:13.5` | 两套主题共用 |

本机（Windows 11）实际安装的字体与本机未安装的字体（读 `HKLM\...\Fonts`）：

| 字体栈里的名字 | 本机安装 |
| --- | --- |
| Segoe UI Variable、Segoe UI | ✅ |
| Microsoft YaHei（含 UI 变体） | ✅ |
| Consolas、Cascadia Code | ✅ |
| Georgia、SimSun | ✅ |
| Noto Serif SC | ✅（**但栈里写的是 `Noto Serif CJK SC`，家族名不匹配**） |
| Songti SC、STSong、PingFang SC、SF Mono、Menlo | ❌ |
| Noto Sans SC | 未在字体表里命中（当前中文回退实际用 Microsoft YaHei） |

源码可确认的回退结果（Windows）：

- 正文：拉丁 = Segoe UI，中文 = Microsoft YaHei —— 与设计意图一致，无风险。
- 衬线档（页标题 / 块标题 / 品牌名）：拉丁 = **Georgia**（`ui/AGENTS.md` 已登记品牌名
  80.7px 的度量就按它算）；中文 = 系统默认衬线 **SimSun**，而不是已安装的 Noto Serif SC。
- 等宽档：`ui-monospace` 在 Chromium/WebView2 上映射到 Consolas；已装的 Cascadia Code 未被利用
  （可选优化，不是清晰度问题）。
- 字重只用 400/500/600/650/700；650 会落到静态字体的邻近档（700），不产生合成字重。
- 全仓无 `-webkit-font-smoothing` / `font-synthesis`（Windows 上那条属性本来也无效，没有用它掩盖问题）。
- Element Plus：组件不设全局 font-family，继承 body；EP 只给图标字体单独设 family；组件字号走
  EP 变量与 theme.css 的 `--el-*` 覆写，无第二套字体体系。

风险：`--font-serif` 的 CJK 名字写错（`Noto Serif CJK SC` vs 已装 `Noto Serif SC`），
当前落到 SimSun。它**看起来仍是宋体**，是否算问题取决于设计意图；**不能顺手改**——改字体族会
改变品牌名与页标题的文本度量，`ui/AGENTS.md` 里品牌宽度判据（80.7px）与 `designAlignment`
判据会一起受影响，属于 M2 的独立评估项。

## 5. CSS 合成风险（全仓搜索结果）

| 声明 | 命中 | 位置 / 性质 | 判断 |
| --- | --- | --- | --- |
| `backdrop-filter` | 2（同一处） | `theme.css:2949-2950` 进度浮层 `.progress-overlay`（`blur(6px)` + `--el-mask-color`） | 磨的是浮层**背后**，不是浮层内文字；会在大面积上建合成层，属浏览器诊断项 |
| `filter: blur(0.4px)` | 2 | 标题栏鲸眼扫光条（装饰条，非文字） | 无风险 |
| `will-change` | 1 | 日志分栏拖拽期间 `will-change: width`，松手移除 body 类 | 瞬时，无风险 |
| `transform: scale/translate` | 20+ | 入场 / 按压（`el-button:active` 0.96、`rail-toggle` 0.92 等）、居中（`translate(-50%,-50%)`）、扫光动画 | 均为动画或居中；结束回到 `translateY(0)/scale(1)`，**没有残留在文字祖先上的持久变换** |
| `zoom` / `translateZ` | 0 | — | 无 |
| `opacity` | 多 | 动画起止与禁用态 | 未见持久压低正文字层的不透明度 |

另一项待验证：自绘标题栏 38px（`--titlebar-height`）+ 1px 下边框，在 175% 下物理高度 66.5px，
**分数缩放下 1px 线可能落成 2px 或半像素灰线**；刻蚀线（`clip-path` 六边形、2px）同理。
源码无法判定，属「需实机截图」项（UI-DPI-02）。

## 6. 图形资源清单

- 位图只有 1 张：`ui/public/whale-icon.png` **128×128**。用法：侧栏 42×42、日志/用量窗口 22×22。
  175% 下需要 73.5px、2x 下需要 84px，均有余量 → **不存在低分辨率放大问题**。
- 其余全部 SVG，均有 `viewBox`：`2api-logo`(480²)、`deepseek/minimax/openai(+dark)/opencode(+dark)/zhipu`(24²)、
  `npm-logo`(0 0 128 128)、`header-ribbons`(640×170)、以及 data-URI mask 图标。未被非整数缩放的
  低清位图。
- Canvas：**0**（无 `canvas` / `getContext` 命中）；图表是手写 SVG：
  `UsageWindow.vue` 环形图 `viewBox="0 0 120 120"`、`MemoryEffectPanel.vue` 趋势 `viewBox` + `polyline`。
  → 不存在「Canvas 未按 DPR 设置内部分辨率」的问题（UI-GFX-02 无对象）。
- 图标：Element Plus（字体图标）+ Lucide（24 网格 SVG，`size` 属性控制）；图标槽显式 15/16px。
  小尺寸 1px 描边的清晰度属实机验收项。

## 7. 问题清单（按分类，不做修复）

### 源码可确认

| ID | 优先级 | 问题 | 位置 | 复现 / 影响 |
| --- | --- | --- | --- | --- |
| FONT-01 | P0 | 衬线栈 CJK 家族名与 Windows 已装字体不匹配：`Noto Serif CJK SC` vs `Noto Serif SC`，中文衬线落到 SimSun | `ui/src/theme.css:13`；使用方 `.brand__name`(:891)、`.page-title`(:1629)、块标题规则 | 任意页面标题 / 品牌名；观感差异，改动会牵动品牌宽度判据 → 留 M2 评估 |
| FONT-02 | P1 | 等宽栈未利用已装 Cascadia Code（仅设计取舍，不影响清晰度） | `ui/src/theme.css:14` | 主观观感 |
| DPI-01 | — | 已确认正常：进程 per-monitor aware、主窗逻辑尺寸精确、无 DPR 乘法 / zoom / 全局 scale | — | — |
| GFX-01 | — | 已确认正常：位图 128px 有余量、SVG 全有 viewBox、无 Canvas | — | — |

### 需浏览器诊断（DevTools：Rendered Fonts / computedStyle / 合成层）

| ID | 优先级 | 待确认 | 计划 |
| --- | --- | --- | --- |
| FONT-03 | P0 | 各元素**实际渲染字体**（Rendered Fonts），尤其衬线档与 EP 组件内文字 | dev 构建开 WebView2 远程调试后逐元素读 |
| CSS-01 | P1 | `.progress-overlay` 的 `backdrop-filter` 在低端 GPU / 远程桌面下的帧率与文字锐度 | 记录合成层数量与帧时间 |
| CSS-02 | P1 | 175% 下 1px 边框 / 2px 刻蚀线的物理落点（是否成 2px 或灰线） | 放大截图 + `getBoundingClientRect` |

### 需实机验证（本轮 `not_run`）

| ID | 优先级 | 待确认 | 原因 |
| --- | --- | --- | --- |
| DPI-02 | P0 | Windows 100/125/150/175/200% 与跨屏切换后的清晰度 / 布局 | 只测到主屏 175%；改系统缩放会动用户设置，未做 |
| DPI-03 | P0 | Windows ClearType 实际观感（不透明修复后的最终效果） | 需要放大截图或人眼验收；M5 |
| WIN-01 | P1 | 三扇副窗（日志 / 用量 / 套餐）在 175% 下的标题栏居中、边缘、圆角/直角 | 本轮未逐一打开；源码/样式位已确认不透明 |
| MAC-01 | P0 | macOS Retina、透明圆角、字体观感 | 本机无 macOS |
| FONT-04 | P2 | 中英文混排与 600/650 字重的最终观感 | 需截图 / 人眼 |

## 8. M1 实施计划（待维护者确认后执行）

**前置（不改产品代码）**：dev 构建以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`
启动，采集 `window.devicePixelRatio`、`window.innerWidth/outerWidth`、`screen.width/availWidth`、
目标元素的 `getBoundingClientRect()` / `getComputedStyle().fontFamily` / Rendered Fonts。

**UI-DPI-01（P0）**
1. 在 5 个缩放档记录：`devicePixelRatio` 是否等于系统缩放（本机 1.75）、`innerWidth` 是否等于
   窗口逻辑宽（主窗 1040）、窗口物理/逻辑尺寸换算是否只发生一次。
2. 跨屏 100↔150、150↔200、200↔100 三组移动后重复记录；确认无整页重排、无模糊、无二次缩放。
3. 判据落地（`ui/test`，静态）：源码不得出现 `zoom`、根级 `transform: scale()`、CSS 内乘 DPR；
   Tauri 窗口尺寸与 DPI 换算只走现有封装。实测数据另写入 M5 报告，`not_run` 不冒充通过。
4. 退出条件：DPI 可观测、无重复缩放、跨屏比例正确；未测的档位如实标 `not_run`。

**UI-DPI-02（P0）**
1. 四扇壳窗口各开一次，采集 `getBoundingClientRect` 与标题栏文字垂直居中量；Windows 不透明
   已确认（§2），补 ClearType 放大截图。
2. 检查 1px 边框 / 刻蚀线 / 窗口边线在 175% 下的物理落点；必要时只修**落点**（不改布局尺寸）。
3. 不牺牲 macOS：透明 + CSS 圆角路径保持不动，macOS 项标 `not_run`。
4. 退出条件：主副窗渲染稳定、边缘粗细可解释；截图与读数入库（不入 bin）。

**M1 不做**：字体栈替换（M2）、组件排版（M3）、图形改造（M4）。M1 的改动只应触及
`ui/src/theme.css` / `ui/test/*` / 必要的窗口配置，且每项独立可回退。

## 9. 未验证（not_run，不计通过）

- macOS Retina / 透明圆角 / 字体观感（无 macOS 环境）。
- Windows 除 175% 外的缩放档位、跨屏 DPI 切换（未改用户系统缩放）。
- ClearType 的放大截图与肉眼验收。
- 日志 / 用量 / 套餐三扇副窗的真实渲染（本轮未打开）。
- 所有 DevTools 侧读数（release 构建无调试通道）。

## 10. 复现命令（本轮实际使用）

```powershell
# 版本 / 工作区 / OS
git rev-parse HEAD; git status --short --branch; (Get-CimInstance Win32_OperatingSystem).Caption

# 进程 DPI 感知 + 窗口物理/逻辑尺寸 + 样式位（DPI-aware 探针）
#   SetProcessDpiAwarenessContext(-4) → EnumWindows(pid) → GetDpiForWindow / GetWindowRect / GetWindowLong(GWL_EXSTYLE)
# WebView2 版本
(Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}').pv

# 字体
(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts')

# 源码扫描
rg -n "backdrop-filter|will-change|translateZ|transform:\s*scale|zoom:|filter:\s*(blur|drop-shadow)" ui/src
rg -n "devicePixelRatio|scaleFactor|font-smoothing|font-synthesis" ui/src
rg -n "<canvas|getContext\(" ui/src
```

## 11. 结论与下一步

- 显示面**没有发现「源码层确定坏了」的 P0 缺陷**：DPI 感知、窗口尺寸换算、Windows 不透明策略、
  位图/SVG/Canvas 资源都成立；唯一源码可确认的字体问题是衬线档的 Windows 家族名不匹配（FONT-01，
  观感/设计问题，不是渲染故障）。
- 「字体发虚」的窗口透明根因已在 v0.7.1-rc.1 修复（c3f82d4d），本轮实机样式位与该版本一致；
  **剩余风险都要靠放大截图 / 跨 DPI 实机验收**，不能再用源码推断代替。
- 按 handoff §12 纪律：**本轮到此为止，未经维护者确认不进入 M1，不改任何源码**。
