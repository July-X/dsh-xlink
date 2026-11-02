# dsh-xlink 显示质量优化 Handoff

-   任务：UI-DPI-01
-   平台：Windows 10/11、Intel macOS
-   技术：Tauri 2、Rust、Vue 3、Element Plus、WebView2、WKWebView
-   范围：仅显示效果、HiDPI、字体、窗口渲染、图形清晰度；不做业务功能扩展或整体
    UI 重设计。
-   执行规则：先诊断、再修复、再优化、最后实机验收。第一轮只做
    M0，未经维护者确认不得进入 M1。

## 1. 目标与边界

目标： 1. Windows 100%--200% 缩放下文字、图标和图表清晰且尺寸正确。 2.
多显示器 DPI 切换后正确适配，无持续模糊、重复缩放或布局异常。 3.
中英文数字混排自然，字体回退和字重可预测。 4.
主副窗口、标题栏、弹窗和浮层渲染稳定。 5. SVG、位图、Canvas 和图表高 DPI
清晰。 6. 保持 Kami 浅色与夜航日志暗色主题。

不做： - 不重做 UI、不更换 Element Plus、不全面迁移 TS/Pinia。 -
不调整窗口尺寸、侧栏宽度、信息架构或业务流程。 -
不以降低信息密度换清晰度，不强制跨平台使用同一字体。 -
不引入无必要的字体依赖。

## 2. 开工前阅读

`AGENTS.md`、`ui/AGENTS.md`、`src-tauri/AGENTS.md`、`ui/src/theme.css`、`ui/src/main.js`、`ui/src/shell/WindowTitleBar.vue`、`ui/src/shell/ViewerShell.vue`、`src-tauri/tauri.conf.json`、`src-tauri/tauri.windows.conf.json`、`src-tauri/src/shell/window.rs`。

记录当前
commit、分支、工作区状态、操作系统和构建模式；不得覆盖已有修改。

## 3. 不可破坏的现状约束

-   主窗口固定逻辑尺寸 1040×748；侧栏 157px，收起 64px。未经批准不得改。
-   Windows 主、副窗口均保持不透明；主窗口配置在
    `tauri.windows.conf.json`，副窗口分支在
    `shell/window.rs`。不得改回透明窗口。
-   macOS 保留透明窗口、CSS 圆角和阴影，不得为了 Windows 优化而牺牲
    Retina 表现。
-   浅色主题为 Kami，暗色主题为夜航日志；唯一暗色判据为 `html.dark`。
-   优先复用 `--font-*`、`--fs-*` 等现有
    Token；明暗主题字号与控件高度保持一致。

## 4. 执行顺序

  阶段   任务                                        优先级             估算
  ------ ------------------------------------------- --------- -------------
  M0     UI-DPI-00 基线诊断                          P0 前置        0.5 人日
  M1     UI-DPI-01 DPI；UI-DPI-02 窗口               P0          1--1.5 人日
  M2     UI-FONT-01 字体体系；UI-FONT-02 字体渲染    P0            1--2 人日
  M3     UI-CSS-01 合成；UI-CSS-02 排版              P1            1--2 人日
  M4     UI-GFX-01 SVG/位图；UI-GFX-02 Canvas/图表   P1          1--1.5 人日
  M5     UI-TEST-01 DPI；UI-TEST-02 视觉回归         P0/P1       1.5--2 人日
  M6     UI-DOC-01 文档交付                          P2             0.5 人日

总计约 6--10 人日，不含等待实机环境的时间。

``` mermaid
flowchart TD
 A["M0 基线诊断"] --> B["M1 DPI 与窗口"]
 B --> C["M2 字体"]
 C --> D["M3 CSS 与组件"]
 D --> E["M4 图形高清适配"]
 E --> F["M5 原生视觉验收"]
 F --> G{"通过？"}
 G -->|否| H["定位并返回对应阶段"]
 H --> F
 G -->|是| I["M6 文档交付"]
```

M1
两项可并行调查，但窗口配置修改由同一负责人统筹。其余阶段按顺序推进；前一阶段未达退出条件，不进入下一阶段。

## 5. M0：基线诊断（第一轮唯一任务）

禁止修改源码。

步骤： 1. 阅读三个 `AGENTS.md`。 2. 记录
commit、分支、工作区、构建模式和 OS 版本。 3.
检查主窗口与全部副窗口创建路径。 4. 核实 Windows
主副窗口不透明策略是否实际生效。 5. 检查字体 Token、字号、行高、字重及
Element Plus 继承。 6. 搜索
`transform`、`filter`、`backdrop-filter`、`opacity`、`will-change`、`translateZ`、`zoom`。
7. 搜索 Canvas、SVG、PNG、图表和动态图片。 8.
将问题区分为源码可确认、需浏览器诊断、需实机验证三类。

交付
`docs/reviews/ui-display-baseline.md`，包含渲染架构、平台窗口配置对照、字体回退风险、DPI
现状、CSS 合成风险及证据、图形资源清单、问题优先级/源码位置/复现条件、M1
实施计划。

退出条件：问题均有源码位置或复现条件；正常项与待验证项分开；没有未经批准的代码改动；维护者审阅确认后才进入
M1。

## 6. M1：DPI 与窗口

### UI-DPI-01（P0）

1.  检查 Tauri/tao 的 Windows DPI 感知和 manifest 覆盖。
2.  记录 `devicePixelRatio`、Tauri `scaleFactor()`、窗口逻辑/物理尺寸。
3.  区分系统 DPI、页面缩放、WebView 栅格化比例。
4.  检查跨屏 DPI 通知、整体 CSS 缩放和重复乘 DPR。

禁止整体 `transform: scale()`、所有 CSS 尺寸乘
DPR、强制用户固定缩放、未经证据设置 rasterization
scale、窗口创建后随意调用进程 DPI API。

测试 100%、125%、150%、175%、200%；跨屏覆盖
100→150、150→200、200→100。退出条件：DPI
可观测、无重复缩放、跨屏比例正确。未实机验证标 `not_run`。

### UI-DPI-02（P0）

检查主副窗口透明策略、背景首帧闪白、标题栏清晰度/垂直居中、边缘/阴影/圆角/裁切、不同
DPI 下边缘粗细。重点检查
`tauri.conf.json`、`tauri.windows.conf.json`、`shell/window.rs`、`WindowTitleBar.vue`、`ViewerShell.vue`、`theme.css`。不得以牺牲一个平台修复另一个平台。

## 7. M2：字体

### UI-FONT-01（P0）

先用 DevTools 的 Rendered Fonts 检查实际字体，不能只看 CSS 声明。

Windows 建议：英文 Segoe UI Variable → Segoe UI；中文 Microsoft YaHei UI
→ Microsoft YaHei；代码 Consolas，可选 Cascadia Code。 macOS
建议：英文系统 UI 字体；中文系统中文字体；代码 SF Mono → Menlo。

示例仅供评估，先复用现有 Token：

``` css
:root {
  --font-ui: "Segoe UI Variable", "Segoe UI",
    "Microsoft YaHei UI", "Microsoft YaHei", sans-serif;
  --font-mono: "Cascadia Code", Consolas, monospace;
}
```

字号评估范围：页面标题 24--31px；块标题 15--16px；正文
14--15px；辅助文字 12--13px；日志/代码
12--13px。不是全局强制替换值。检查真实 400/500/600
字重，避免合成字重。退出条件：中英文混排自然、回退可预测、Element Plus
继承明确。

### UI-FONT-02（P0）

检查字体加载失败、fallback、合成字重、小字号对比度、WebView2 Runtime
差异、透明窗口抗锯齿影响。禁止把 `-webkit-font-smoothing` 或
`-moz-osx-font-smoothing` 当 Windows
修复；不以文字阴影/滤镜/加粗/透明度掩盖根因。

## 8. M3：CSS 与组件

### UI-CSS-01（P1）

检查文字及祖先的
`transform`、`filter`、`backdrop-filter`、`opacity`、`will-change`、`translateZ`、`zoom`；重点排查动画结束后的残留变换和不必要合成层。不可全局删除，只修复有对照证据的问题。

### UI-CSS-02（P1）

检查字号、行高、字重、字间距、数字对齐、长中文/英文/路径换行，以及
Element Plus
输入框、按钮、下拉、Tooltip、Dialog、Loading、滚动条、Hover、选中态。浅深主题分别验证。普通文字对比度原则上
≥4.5:1，大号文字 ≥3:1。

## 9. M4：图形高清

### UI-GFX-01（P1）

检查低分辨率 PNG、SVG
`viewBox`/宽高/描边、非整数像素缩放、位图放大拉伸。优先
SVG；位图需有足够分辨率；不把普通文字烘焙进低分辨率图片。

### UI-GFX-02（P1）

Canvas 内部像素按 DPR 设置，CSS 尺寸保持逻辑像素：

``` ts
const dpr = window.devicePixelRatio || 1;
canvas.width = Math.round(cssWidth * dpr);
canvas.height = Math.round(cssHeight * dpr);
canvas.style.width = `${cssWidth}px`;
canvas.style.height = `${cssHeight}px`;
```

DPR 或组件尺寸变化后重绘；绘图坐标保持 CSS 像素。先检查图表库是否已管理
DPR，避免重复放大。

## 10. M5：实机验收

Windows 必测 100%、125%、150%、200%，175% 建议；macOS 测
Retina、浅色/暗色、主副窗口。跨屏至少使用两种缩放比例。

重点页面：概览、内核版本、插件列表、用量分析、日志窗口、记忆详情弹窗。

结合正式 Tauri WebView
截图、`getComputedStyle()`、`getBoundingClientRect()`、DPR/scaleFactor、Canvas
实际像素尺寸和 Rendered
Fonts。检查信息完整、无裁切/重叠/异常换行、图标清晰、层级一致。不同 DPI
的物理像素截图不要求完全一致。

交付原始截图、诊断日志、关键元素计算样式/尺寸、问题清单和修复前后对照。无实机环境标
`not_run`，不得用浏览器模拟冒充通过。

## 11. M6：文档交付

交付： - `docs/reviews/ui-display-baseline.md` -
`docs/reviews/ui-display-optimization-report.md` -
`docs/reviews/ui-display-validation.md`

按需更新 `ui/AGENTS.md` 中需要长期保留的规则；临时诊断过程不要全部塞进
AGENTS。

## 12. Agent 执行纪律

1.  一次只执行一个阶段；阶段结束提交报告并等待维护者确认。
2.  不允许无证据修改；字体发虚先区分 DPI、字体回退、CSS 合成、窗口透明和
    Runtime。
3.  不做整体 UI 重设计，不擅自改窗口、侧栏、主题或组件结构。
4.  每个问题独立修复，避免同时改字体、颜色、布局和动画。
5.  记录 `pass`、`fail`、`skip`、`not_run`；skip/not_run 不算通过。
6.  每阶段保持可回退，不一次性提交全部视觉改动。
7.  使用项目规定的隔离测试 home、实例和通道，不访问真实用户数据。
8.  浏览器预览不能代替原生 WebView 验收。

## 13. 最终验收权重

  项目                   权重
  -------------------- ------
  Windows 字体清晰度      25%
  DPI 缩放正确性          20%
  macOS Retina 效果       15%
  中英文排版              15%
  窗口及弹窗渲染          10%
  图标、图表清晰度        10%
  明暗主题一致性           5%

最终要求：Windows/macOS
上文字、图标、图表和窗口元素清晰、自然、稳定；不依赖用户手工修改系统设置，不以牺牲布局密度换清晰度，不破坏既有功能和主题。
