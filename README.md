# DeepSeek Harness 桌面端（dsh-xlink）

[![Desktop release (dsh-xlink)](https://github.com/July-X/dsh-xlink/actions/workflows/desktop-release.yml/badge.svg?event=release)](https://github.com/July-X/dsh-xlink/actions/workflows/desktop-release.yml)

一个基于 [Tauri v2](https://tauri.app/zh-cn/) 的桌面外壳：在桌面上自由打开 DeepSeek Harness 的 Web UI，并提供**内核更新菜单**——跟随官方
`deepseek-ai/deepseek-harness` 的 GitHub Release tag（`dsh-v*`）一键安装、切换、删除、更新内核版本。

> 本仓库是独立桌面项目，仓库根目录直接包含 Tauri 外壳、Vue 管理面板、文档和发布配置。
>
> GitHub 仓库：[July-X/dsh-xlink](https://github.com/July-X/dsh-xlink)

## 它如何工作

```
+---------------------------- dsh-xlink (Tauri v2) ----------------------------+
|                                                                            |
|  main window (panel)            harness window             official-chat   |
|  ui/ static page                 WebviewWindow              window          |
|  - kernel status / start         "harness"                  WebviewWindow   |
|  - update menu                   loads                      "official-chat" |
|  - settings / logs               http://127.0.0.1:<port>    loads           |
|  - open official chat button    = dsh web UI                https://        |
|         | invoke                  ^                  ^        chat.deepseek |
|         v                         |                  |        .com         |
|  +--------------------------------------------------------------------+     |
|  | Rust shell: kernel lifecycle + version management                  |     |
|  | - install: pnpm add @deepseek-ai/dsh@<version>                    |     |
|  |   (node-linker=hoisted; live log stream to UI)                     |     |
|  |   -> ~/.dsh/desktop/kernels/<version>/                            |     |
|  | - active pointer: ~/.dsh/desktop/active.txt                       |     |
|  | - start: node .../lib/bin.js web --no-open --port 3080            |     |
|  +--------------------------------------------------------------------+     |
|                        ^                                                     |
|    kernel data (sessions, settings) | independent of shell, lives in         |
|                                       ~/.dsh                                 |
+------------------------------------------------------------------------------+
```

- **内置访问方式**：外壳在本地启动 `dsh web` 服务并用专用窗口加载其 Web UI，无需手动打开浏览器；打开工作台前会为发布包缺失的 source map 生成最小 sidecar，避免 debug DevTools 产生无意义的 404。
- **官方对话快捷入口**：管理面板「概览」页的「打开官方对话」按钮拉起独立的 `official-chat` 窗口（label `official-chat`），固定加载 DeepSeek 官方对话页 `https://chat.deepseek.com`，默认只初始化 DeepSeek 页签，千问与 MiniMax 在首次选择时才创建并在本次窗口中保留页面状态，以降低首开 CPU、内存和网络开销；由外壳注入静态 chrome-row 顶部品牌条带 + 拉绳挂件（不运行常驻动画，避免 WKWebView 空闲时持续渲染）。窗口创建时不再覆盖 user-agent——WebView2 引擎本身就是真实的桌面版 Edge，原生 UA、`Sec-CH-UA` 客户端提示与 `navigator.userAgentData` 天然一致（此前改写成 Chrome 反而制造跨层矛盾，正是环境检测的特征）；专属目录同时充当持久化配置档案，DeepSeek 登录态跨外壳重启保留；同一按钮在窗口已开时变为「关闭官方对话」，复用现有窗口并 `set_focus`，OS 关闭按钮也保持有效（该窗口不持有内核会话）。创建窗口时还经 `.additional_browser_args(OFFICIAL_CHAT_BROWSER_ARGS)` 追加 Chromium 启动开关（wry 默认 `msWebOOUI` / `msPdfOOUI` / `msSmartScreenProtection` 之上叠加 `AutomationControlled`、`TranslateUI`、`InterestFeedContentSuggestions` 与 `--disable-blink-features=AutomationControlled`）：既抑制 `navigator.webdriver = true`，也不弹 SmartScreen 安全提醒与 Edge 专属 UI；该参数仅 WebView2 后端消费，macOS / Linux 构建自动忽略；同时窗口固定使用专属 user-data 目录 `<data_dir>/webview-official-chat`（WebView2 要求同一目录上的环境参数一致，不隔离会导致窗口创建失败）；之后给 `official-chat` 内容子 webview 注入 `titlebar-pulse.js` → `chat-fingerprint.js` 两个 `initialization_script`，给本地 `official-chat-strip` 页签栏 webview 注入 `pullstring-launcher.js`：拉绳属于整个官方对话窗口的 chrome，由页签栏 webview 承载；`chat-fingerprint.js` 不再与之同链、不需要 `pullstring-launcher.js` 抢跑 `window.__TAURI__` 的闭包引用。
- **macOS / Windows 自定义标题栏（试验）**：管理面板在 macOS 和 Windows 上使用前端自绘的窗口标题栏，模拟交通灯控制，主色带从左到右以 5% 到 70% 的不透明度叠加深 Gitea 绿，并保留毛笔笔触纹理；dev 构建切换为同样规则的低亮度鲸眼红色系，Linux 暂保留系统标题栏。
- **内核更新**：官方发布到 npm registry 的 `@deepseek-ai/dsh`（以及同名 `dsh-*` 依赖包）页面 [`https://www.npmjs.com/package/@deepseek-ai/dsh`](https://www.npmjs.com/package/@deepseek-ai/dsh) 与 GitHub `dsh-v<semver>` tag 一一对应；更新菜单直接读 npm registry（`https://registry.npmjs.org/@deepseek-ai/dsh`）拿到全量版本与 `dist-tags`，可安装、切换、删除任意已发布版本；只有 npm registry 不可达时才回退 GitHub Releases API 与其 Atom feed。

## 功能

- 一键启动 / 停止 / 打开 Harness 工作台
- 「打开官方对话」：管理面板「概览」页一键拉起独立的官方对话窗口，按 `OFFICIAL_CHAT_TABS` 展示 DeepSeek、千问、MiniMax 三个页签，默认只加载 DeepSeek，其他页签首次选择时才创建并保留本窗口状态，使用原生 Edge UA、可持久化登录的专属 user-data 目录与 `OFFICIAL_CHAT_BROWSER_ARGS` 浏览器开关（隐藏 `navigator.webdriver`、关闭 SmartScreen 提醒与 Edge 专属 UI）让官方站点的环境检查把它视为普通浏览器；本地 `official-chat-strip` 页签栏 webview 注入 `pullstring-launcher.js`（拉绳属于整个官方对话窗口的 chrome，由页签栏 webview 承载），内容子 webview 注入 `titlebar-pulse.js` → `chat-fingerprint.js`（钉住 `navigator.webdriver = false` 并删除 `__TAURI_*` 嵌入式全局）（重复点击复用现有窗口并聚焦；窗口已开时按钮文案翻为「关闭官方对话」并触发 `close_official_chat`，OS 关闭按钮正常工作）
- 更新菜单：列出 npm registry [`@deepseek-ai/dsh`](https://www.npmjs.com/package/@deepseek-ai/dsh) 的所有发布版本（含预发布标记），安装、切换活动版本、删除本地版本
- 内核安装通过 pnpm 执行（`node-linker=hoisted` 保持扁平 `node_modules`，内容寻址存储让重复安装更快），安装过程逐行流式显示在进度面板中，完整日志落盘 `~/.dsh/desktop/logs/<kind>-install-<版本>-<日期>.log`（dev 壳则是 `~/.dsh/desktop-dev/logs/dev-install-<版本>-<日期>.log`，`<日期>` 为本地日期）；下载先写临时文件，成功后才发布，npm 包由外壳进行路径受限、禁止链接和有展开大小上限的 Rust 解包，无需额外安装系统 `tar`
- Node.js 自动检测与手动指定（要求 `^22.19 || >=24`，与 dsh 的 engines 一致；自动发现 nvm（macOS/Linux `~/.nvm/versions/node/<v>/bin/node` 跟随 `alias/default` 链，Windows `%NVM_SYMLINK%` 与 `%NVM_HOME%/v*/node.exe`），免去 GUI 启动看不到 nvm PATH 时改手动路径的步骤；检测为空时弹窗询问是否「帮我安装」——确认后自动下载官方 Node.js（v24 LTS，SHA-256 校验）到数据目录 `tools/node/`，概览页 Node 行随时可再次触发；已安装的托管运行时优先于环境检测，显式配置的 node 路径仍最高优先）
- pnpm 路径可配置（默认取 node 同目录或 PATH）
- 端口可配置（默认 3080）
- 内核运行日志查看；应用退出时自动回收内核子进程
- **插件管理**：社区插件（npm 包或 GitHub 仓库）统一存入 `~/.dsh/plugins/`，以**链接**（默认，Windows 自动降级**复制**）的方式进入每个已安装内核（`~/.dsh/desktop/kernels/<版本>/plugins/`），并自动接线进 profile——切换内核无需重装；GitHub 仓库地址安装时优先使用对应 GitHub Release 的 tarball 版本数据，Release 不可用时回退 git clone，其它 Git 地址保持原有 clone 行为；「插件中心」对接 [dshfind.com](https://dshfind.com/zh) 插件超市目录（分类/搜索/排序/已安装过滤，6 小时本地缓存，官方 market 兜底），安装优先尝试 npm（候选 `<repo-name>` / `dsh-<repo-name>`），npm 包不可用时回退到 GitHub Release tarball/git clone，安装前校验 dsh 规范；管理面板提供安装/卸载/更新/切换模式/同步，检测到新版本时在卡片与启动时提醒；点击「同步」会遍历所有已安装内核，重新物化中央库中的插件，并清除外壳明确标记的已删除残留，保证外壳管理的插件状态与 `~/.dsh/plugins/` 一致；启动容错面板中的「移除插件」会同时清除隔离记录，若上次卸载只完成了部分清理，重复执行也能继续收尾
- **工作台健康自检**：工作台窗口自动监听白屏、运行时错误和未处理的 Promise 异常；外壳会把前端消息、堆栈和页面地址与今天的内核日志（`<kind>-kernel-<日期>.log`）一起分析，判断为疑似插件、疑似内核或暂未能归因（白屏 + 已装第三方插件的「软信号」分支会列出所有插件作为排查候选但不会自动隔离），并在事故面板展示证据和对应的插件隔离/移除、日志、内核版本修复入口
- **技能管理**：社区技能（npm 包 / GitHub 仓库 / 本地文件夹）统一存入 `~/.dsh/skills-store/`，按包安装的粒度以链接（失败降级复制）物化进内核自带扫描的 `~/.dsh/skills/`——不改 cordis 配置、不装依赖、切换内核零操作；内核对技能根做文件监视，**安装/卸载/更新对运行中的工作台即时生效，无需重启**；安装前逐个校验 SKILL.md frontmatter（kebab-case `name` + `description` 必填），避免"装了却不出现"；本地文件夹来源支持改完点「重新同步」；启动时自动对账（补链、清扫孤儿链接、恢复中断的更新）；v1 面板只出手动安装行（git 仓库地址），社区目录卡等中心上线技能 feed 之后再启用
- **内置补丁（内核补丁 / 小插件）**：随 dsh-xlink 发布包捆绑的自研内核补丁与小插件（`src-tauri/resources/patches/<id>/`，发布时进入 app 资源目录，与社区插件不同、无需第三方信任），默认**不生效**——在「设置 → 内核补丁」页自主选择「应用到当前内核」或「撤销补丁」；应用前自动备份被覆盖的原文件到 `~/.dsh/desktop/patches/backups/`，撤销时从备份还原，备份丢失时以内容 SHA-256 校验兜底、绝不盲目覆盖或删除；支持 `copy`（新增/覆盖文件，覆盖既有文件时可用 `expectSha256` 声明预期原文件哈希作安全闸）与 `replace`（精确字符串替换）两种文件操作，目标路径严格限制在内核目录内，可按 `minKernelVersion` / `maxKernelVersion` 声明适用内核版本范围；当补丁功能被官方内核采纳后可通过 `supersededSinceKernelVersion` 字段声明「从该内核版本起已被官方取代」，UI 把对应卡片折叠为「已并入官方内核」（删除线 + 默认收起 + 应用按钮禁用），用户可手动展开查看，已应用到旧内核的记录仍可正常撤销；应用记录持久化在 `~/.dsh/desktop/patches/state.json`，按「补丁 × 内核版本」隔离；工作台运行期间禁止操作；设置页按官方采纳情况处置补丁卡片：`dsh-file-perf`（dsh `@` 引用性能修复）已被官方 0.1.2-alpha.2 起直接采纳，卡片折叠为「已并入官方内核」（降饱和 + 斜体标题 + 默认折叠 + 应用按钮禁用，即灰度不可应用状态）；`dsh-session-perf`（历史会话列表加载提速）v1.2.0 锚定官方 0.1.2-alpha.3 重新收录——官方 alpha.2/alpha.3 的持久层 lib 逐字节相同且未实现枚举缓存，设置页正常提供「应用/撤销」；`dsh-escalation-same-mode` 让相同 effective mode 的升级请求直接短路（当前载荷仅适配 `0.1.1-rc.2`）。旧内核上的已应用记录仍可撤销，设计文档见 [docs/patch-management.md](docs/patch-management.md)

## 目录结构

```text
.
├── package.json              # 独立项目脚本与前端依赖
├── pnpm-workspace.yaml       # 独立 pnpm 根（放行 esbuild）
├── ui/                       # 管理面板前端（Vue 3 + Element Plus，Vite 构建）
│   ├── index.html            # SPA 入口（加载 src/main.js）
│   ├── public/               # 静态资源：whale-icon.png 顶栏 logo
│   ├── src/                  # 源码：App.vue / 各面板组件 / store / plugins / skills / theme.css
│   └── dist/                 # vite build 产物（tauri.conf.json 的 frontendDist）
├── docs/                     # 架构、插件、技能、补丁、图标和故障排查文档
├── assets/                   # 全仓库图标母版
│   ├── whale-icon.svg        # 完整细节母版（黑鲸 + 红眼，用于 ≥128px）
│   ├── whale-icon-small.svg  # 小尺寸母版（红眼夸大版，用于 ≤64px）
│   └── whale-icon-512.png    # 512px 位图（脚本从 whale-icon.svg 渲染）
├── scripts/
│   └── build-icons.sh        # 从双 SVG 母版生成 Tauri 和面板图标
└── src-tauri/                # Tauri v2 Rust 进程
    ├── tauri.conf.json       # frontendDist → ../ui/dist；resources 捆绑 patches/
    ├── Cargo.toml / Cargo.lock
    ├── capabilities/         # 各窗口的访问权限
    ├── icons/                # 应用图标集
    ├── resources/
    │   └── patches/<id>/     # 内置补丁清单与载荷（随发布包进入 app 资源目录）
    └── src/
        ├── main.rs / lib.rs  # 入口与装配（含退出时回收内核）
        ├── commands.rs       # Tauri 命令（含插件/技能/补丁与窗口操作）
        ├── kernel.rs         # 安装 / active / 启动 / 停止 / 端口探测
        ├── plugins.rs        # 插件中央库、物化、接线与更新
        ├── patches.rs        # 内置补丁：清单、备份、应用/撤销、状态
        ├── skills.rs         # 技能中央库、物化、启停与更新
        ├── releases.rs       # 官方发布列表（API + Atom 回退）
        ├── node.rs           # Node/pnpm 检测与版本校验
        └── settings.rs       # settings.json 读写
```

## 本地构建

前提：Rust 工具链（含 `cargo`）、Node.js 22+；`scripts/install.mjs` 会自动检测 pnpm，缺失时回退到 npm。

```sh
# 安装 Tauri CLI（自动检测 pnpm，缺失时回退到 npm）
npm run deps

# 开发运行（需先安装内核，见「使用」）
npm run dev

# 本机当前架构构建
npm run build

# 指定目标平台
npm run build:mac-intel   # x86_64-apple-darwin（Intel Mac）
npm run build:win         # x86_64-pc-windows-msvc
```

> 想直接走 pnpm / npm 也行：`pnpm install` 或 `npm install`；根目录的 `pnpm-workspace.yaml` 会让 pnpm 保持本项目独立。

产物位于 `src-tauri/target/release/bundle/`（macOS 为 `.dmg`，Windows 为 NSIS 安装包 `.exe`）。

## 使用

1. 启动桌面应用，打开管理面板。
2. **设置**：确认已检测到满足要求的 Node.js（不满足时点击「帮我安装」即可自动下载官方 Node.js 到数据目录，或手动安装 Node 22.19+、手动指定路径；通过 nvm 管理的 Node 会被自动发现——macOS/Linux 读 `~/.nvm/alias/default` 与 `versions/node/*/bin/node`，Windows 读 `%NVM_SYMLINK%` 与 `%NVM_HOME%/v*/node.exe`）。
3. **内核更新**：应用启动时会扫描并列出本地已安装版本，进入「内核版本」页即可在左侧备用版本中切换；只有工作台已停止时才能切换，工作台启动或运行期间请先在「概览」页点击「关闭工作台」；点击「检查更新」只从 npm 获取官方发布列表，再选择未安装的版本点「安装」。
   - 安装通过 pnpm 执行，进度面板会实时滚动 pnpm 日志；pnpm 未安装时按提示 `npm install -g pnpm` 或在设置中指定 pnpm 路径。
   - 首次安装会自动成为活动版本并自动启动内核。
   - 之后安装的版本不会覆盖当前活动版本，可随时在「已安装」列表中「切换」或「删除」。
4. （可选）**插件** → 在「插件中心」按分类浏览、搜索（即时过滤）、按 Star/更新时间排序后一键安装，或手动填写 npm 包名（如 `@ace-zone/dsh-market`）/ GitHub 仓库 URL 安装；安装前自动校验插件是否符合 dsh 规范（package.json / `dsh.bundle.patch` / 入口文件），安装完成后重启工作台（关闭后重新启动）生效。点击「同步」会对所有已安装内核重新物化中央插件库，并清除外壳标记的已删除插件残留。进入「内核版本」页后，每个已安装版本旁的信息图标可悬停查看该版本实际物化的插件、版本和链接/拷贝模式。
5. （可选）**设置 → 内核补丁（内置）**：查看随当前 dsh-xlink 版本捆绑的内核补丁与小插件（来自本应用发布方，与社区插件不同），自主选择「应用到当前内核」或「撤销补丁」；应用前自动备份被覆盖的原文件、随时可撤销，状态与备份记录在 `~/.dsh/desktop/patches/`（dev 壳为 `desktop-dev`）。工作台运行期间不能操作，请先关闭工作台；切换内核版本后需对新的活动版本重新应用。
   - `dsh-session-perf` v1.2.0 适配 `0.1.2-alpha.3`，作用于历史会话列表的 artifact/header 扫描，应用后需重启工作台；它不会改变选中会话后的完整历史读取，也不会优化 `dsh-personal-center` 的全量统计扫描。
   - `dsh-escalation-same-mode` v1.0.0 仅适用于 `0.1.1-rc.2`：在 `@deepseek-ai/dsh-sandbox` 的 `approveEscalation` 顶部插入同模式短路，会话已处于目标模式时再送入同模式 `sandbox_permissions` 不再被「not strictly wider」击穿；其它分支保持原行为，应用后需重启工作台使新 sandbox 模块生效。
6. 在「概览」页点击「启动工作台」：自动拉起内核、等待就绪后校验当前内核的工作台地址，再打开工作台窗口进入 Harness 界面；启动失败会自动弹出事故面板和内核日志。「关闭工作台」会同时关闭工作台窗口并停止内核。工作台窗口的系统关闭按钮（macOS 交通灯红灯 / Windows ×）始终可用，只收起窗口、内核与任务继续在后台运行；内核运行中收起窗口后，随时可用「打开工作台窗口」重新打开。
   - 工作台窗口会自动进行健康自检。发现白屏、运行时错误或未处理的 Promise 异常时，事故面板会展示前端消息/堆栈，并标注「疑似插件问题」「疑似内核问题」或「暂未能归因」；插件问题可重新启用或移除，内核问题可先停止工作台，再打开日志并切换/重装版本，暂未归因时由你选择先看日志还是检查内核版本。
   - 工作台窗口侧栏头部右侧（品牌 logo 旁）悬浮着一个灯泡拉绳小挂件：点击（拉动）它，灯泡点亮的同时桌面端管理面板会归位到点击位置附近并提到当前桌面上方，方便随手操作；若灯泡闪红，说明与桌面壳的通信失败，可查看工作台 DevTools 控制台。
7. 「打开官方对话」：在「概览」页点击此按钮即可拉起独立的官方对话窗口（顶部条带 chrome-row 官方品牌蓝 `#4D6BFE`、拉绳挂件挂页签栏右侧 12px；区别于工作台窗口的 Gitea 绿色 212px 偏移），按 `OFFICIAL_CHAT_TABS` 顺序排布 DeepSeek / 千问 / MiniMax 三个页签，与工作台窗口互不干扰；窗口创建时不覆盖 user-agent（诚实呈现桌面版 Edge）、以专属目录持久化登录态，并经 `.additional_browser_args(OFFICIAL_CHAT_BROWSER_ARGS)` 注入 Chromium 开关（含 wry 默认三项与 `AutomationControlled` 等：不自报 `navigator.webdriver = true`、不弹 SmartScreen 安全提醒），随后给本地 `official-chat-strip` 页签栏 webview 注入 `pullstring-launcher.js`（拉绳属于整个窗口的 chrome，挂在页签栏右侧）、给内容子 webview 注入 `titlebar-pulse.js` → `chat-fingerprint.js`（钉住 `navigator.webdriver = false` 并删除 `__TAURI_*` 嵌入式全局——`chat-fingerprint.js` 只清嵌入式痕迹，不再伪造 `userAgentData` / `window.chrome`）；窗口已开时按钮变为「关闭官方对话」并销毁当前窗口，OS 关闭按钮始终保持有效。
8. 首次使用时在 Harness 的设置页配置 DeepSeek（`DEEPSEEK_API_KEY` 等）即可开始对话。


数据目录（统一在 dsh home 下的 `desktop/` 二级目录）：
- 外壳元数据（已装版本、活动指针、设置、日志、补丁状态）：`~/.dsh/desktop/`（`kernels/`、`logs/`、`settings.json`、`active.txt`、`patches/`（补丁应用记录与备份）；可用 `DSH_HOME` 环境变量重定向整个 dsh home）
- 内核数据（会话、配置、profile）：`~/.dsh`
> 从旧版本升级：旧版外壳把元数据存在系统应用数据目录（macOS `~/Library/Application Support/com.zhongxingxing.dsh-desktop/`）。新版启动后该处数据不再读取，请将旧目录下的 `kernels/`、`logs/`、`settings.json`、`active.txt` 手动移到 `~/.dsh/desktop/`。

## 发布（GitHub Actions）

工作流：[`.github/workflows/desktop-release.yml`](.github/workflows/desktop-release.yml)

- 支持平台：**Intel macOS**（`macos-15-intel`，`.dmg`）+ **Windows x86_64**（`windows-latest`，NSIS `.exe`）
- 触发方式：
  - 手动在 Actions 页从 `main` 触发 `workflow_dispatch`（使用当前 `package.json` 版本，推荐，后续版本可复用 Rust 编译缓存）；或
  - 推送 tag：先同步 `package.json` 与 `src-tauri/tauri.conf.json` 的 `version`，再 `git tag desktop-v<version>` 并推送。
- 发布来源限定为 `main` 分支，产物发布为正式 release，不是 draft 或 prerelease。
- 发布前质量门禁：UI 回归测试与生产构建、JavaScript 700 kB/CSS 180 kB bundle 预算、Rust `cargo test`、`cargo fmt --check` 和 `cargo clippy -D warnings` 全部通过后才允许发布。
- 发布提速：预检通过后，质量门禁与 Intel macOS、Windows 两个构建 job 并行运行；平台 job 只上传 Actions artifact，全部成功后由独立的 publish job 一次性创建正式 Release 与 `latest.json`。`max-parallel: 2`、pnpm store、Cargo registry 和按平台隔离的 Cargo target 都启用缓存，Rust release 使用 thin LTO 与 16 个 codegen units。缓存只在 `main` 分支保存，手动发布可跨版本复用；直接推送新 tag 通常会冷启动。详细时序与排障见 [`docs/release.md`](docs/release.md)。
> GitHub Actions 首次建立缓存时仍会经历冷启动；runner 排队、缓存服务和网络波动也不属于 workflow 可控的构建时间。
> 签名说明：当前产物未做代码签名，Windows SmartScreen 与 macOS Gatekeeper 可能给出警告。加入签名（Apple Developer ID / Windows 代码签名证书 + 对应 secrets）后再去掉相关提示。

## 常见启动失败与处理

| 症状 | 排查 |
| --- | --- |
| `WebviewWindowBuilder` 创建工作台窗口卡死 | Tauri 2.x 在同步命令里创建 webview 窗口**会死锁**（Windows 100%；macOS/Linux 部分情况下也慢）。本项目 `open_harness` 已经把创建放在新线程（`commands.rs::open_harness`）。新增类似命令请保持同样模式。 |
| macOS 启动后访问 `http://127.0.0.1:3080` 失败 | Tauri 2.x 默认 WKWebView 已允许本地环回访问，不需要 `NSAppTransportSecurity` 例外；本项目移除了该字段，依赖平台默认值。 |
| 编辑器/IDE 报 `capabilities/default.json` 找不到 `$schema` | schema 文件在首次 `tauri build` 后由 `tauri-build` 生成；本项目移除了硬编码 `$schema` 引用，避免初次克隆时编辑器红字。 |
| 升级后「已安装」列表为空 | 外壳元数据已迁到 `~/.dsh/desktop/`；按上文“数据目录”提示迁移旧目录内容，或重新安装内核。 |

## 性能排查

- 工作台与官方对话的顶部品牌线采用静态渲染，不会在空闲时保持 WebKit 帧循环。安装包含 `src-tauri/src/titlebar-pulse.js` 修改的桌面壳后，需要完全退出并重新启动应用，再重新打开工作台；已存在的 WebView 不会自动替换初始化脚本。
- `dsh-personal-center` 是第三方可选插件。桌面宠物的统计接口会同步读取并解压全部历史会话；会话较多时可能造成明显的 Node 磁盘/CPU 阻塞。遇到周期性卡顿时，在个人配置中关闭桌面宠物和「会话状态」，需要统计时再手动打开 Token 用量页面。
- `patchReload: live` 会启用 client-HMR 的 500ms bundle `stat` 轮询。日常使用可改为 `startup` 并在 profile patch 中禁用 `client-hmr`；需要调试 client plugin 时再恢复 `live`，删除该禁用项。
- 当前内核为 `0.1.2-alpha.3` 时，可在「设置 → 内核补丁」停止工作台后应用 `dsh-session-perf`，减少会话列表的重复目录/header 扫描；它不优化选中会话后的完整历史解压。

## 已知限制与后续

- **Node 运行时**：目前检测系统 Node 或手动指定；后续可捆绑 Node sidecar 实现开箱即用（分发体积 +40MB/平台）。
- **pnpm 依赖**：内核安装依赖用户环境中的 pnpm（未捆绑）；后续可评估 `corepack` 或 sidecar 方式随应用分发。
- **端口冲突**：若 3080 已被其他进程占用，先停止外部服务或改端口。
- **安全**：应用通过 Webview 加载本地 `http://127.0.0.1` 的 Harness 页面并暴露版本管理命令；仅信任官方 `deepseek-ai` 仓库与 npm 的 `@deepseek-ai` 命名空间。插件和技能是第三方内容/任意代码，安装前请自行确认来源；社区目录条目保留「未验证」标记。npm 包解包拒绝绝对路径、父级路径、符号链接、硬链接和特殊文件，并限制条目数与展开体积；这不能替代对第三方代码的审计。
- **插件链接模式**：依赖文件系统符号链接支持（Windows 需要开发者模式，失败会自动降级为复制模式并在行内显示「复制」徽标）。
- **自动更新**：桌面应用自身的升级可后续接入 tauri-plugin-updater；当前发布流程聚焦内核更新。
