# DeepSeek Harness 桌面端（dsh-xlink）

[![Desktop release (dsh-xlink)](https://github.com/July-X/dsh-xlink/actions/workflows/desktop-release.yml/badge.svg)](https://github.com/July-X/dsh-xlink/actions/workflows/desktop-release.yml)

基于 [Tauri v2](https://tauri.app/zh-cn/) 的多内核桌面外壳。把不同内核的 Web UI（当前已支持 DeepSeek Harness，未来加入 mcode 等）装到桌面上、按实例跑起来、互不干扰地共存。外壳只准备路径和环境变量，不改内核代码——内核怎么跑是内核自己的事，外壳只负责「给它一个干净的家、装好扩展、看护生命」。跟着官方 [`deepseek-ai/deepseek-harness`](https://github.com/deepseek-ai/deepseek-harness) 的 `dsh-v*` tag 一键装、切换、删。

仓库根目录就是桌面项目本体（独立 pnpm 根、独立的 Tauri / Vue / 文档 / 发布配置）。

GitHub 仓库：[July-X/dsh-xlink](https://github.com/July-X/dsh-xlink)

## 它如何工作

**多内核并存**：窗口顶部一排内核 tab——DSH 与未来的 mcode 各占一个 tab，标签只显示内核族名。所有面板、侧栏与菜单都归属当前选中的内核；标签按实例标识稳定排列，切换期间禁止重复提交，列表读取失败可重试。当前实例是注册表里的默认实例；旧版概览、版本和插件写入接口仍使用兼容实例 `dsh/default`，尚未代表整套管理功能已支持任意实例。切换标签不搬实例目录、不覆盖全局设置。

**0 侵入内核**：外壳通过 `KernelAdapter` trait 与每个内核族对接——`DshAdapter`（active）负责 DeepSeek Harness 的实例准备，`McodeAdapter`（mock，已注册）证明通用实例模型能容纳第二种内核。Xlink 只负责：拉起实例前把实例 home、profile、workspace、端口、customSkillDirs 准备到位；启动时把 `DSH_HOME`、`DSH_CUSTOM_SKILL_DIRS` 等环境变量塞给进程；跑起来之后用 WebSocket / HTTP 探测健康、回收进程组、订阅事件流。内核二进制、profile 的 package.json 结构、cordis 配置、session 格式——一概不动。接入一个新内核 = 加一个 `KernelAdapter` 实现并注册进 `adapters()`，通用实例模型一行不动。

```
+------------------------------ dsh-xlink (Tauri v2) ------------------------------+
|                                                                                |
|  main window (panel)              harness window                official-chat   |
|  ui/ static page                   WebviewWindow                window          |
|  - kernel status / start           "harness"                    WebviewWindow   |
|  - update menu                     loads                        "official-chat" |
|  - settings / logs                 http://127.0.0.1:<port>      loads           |
|  - open official chat button       = kernel web UI              https://        |
|         | invoke                    ^                  ^         chat.deepseek |
|         v                           |                  |         .com         |
|  +-----------------------------------------------------------------------+     |
|  | Rust shell: kernel lifecycle via KernelAdapter trait                 |     |
|  | - DshAdapter: @deepseek-ai/dsh, prepare home/profile/workspace      |     |
|  |   -> <DSH_XLINK_HOME>/kernels/dsh/instances/<id>/                   |     |
|  | - McodeAdapter (mock): structural placeholder, same lifecycle        |     |
|  | - install: pnpm add @deepseek-ai/dsh@<version>                      |     |
|  |   (node-linker=hoisted; live log stream to UI)                       |     |
|  | - start: node .../lib/bin.js web --no-open --port <port>            |     |
|  +-----------------------------------------------------------------------+     |
|                          ^                                                      |
|     kernel data (sessions, settings, credentials) | per-instance home/         |
|                                                    extensions/plugins/<id>/    |
|                          ^                                                      |
|     shared skills active view (skills/active/) | read by every kernel          |
|                                                 via DSH_CUSTOM_SKILL_DIRS      |
+----------------------------------------------------------------------------------+
```

- **内置工作台**：外壳在本地起 `dsh web`，用专用窗口加载其 Web UI，免去手动开浏览器。打开工作台前会为发布包缺失的 source map 生成最小 sidecar，debug DevTools 不再出 404。
- **官方对话快捷入口**：管理面板「概览」页的「打开官方对话」按钮拉起独立的 `official-chat` 窗口，固定加载 [chat.deepseek.com](https://chat.deepseek.com)。默认只初始化 DeepSeek 页签，千问与 MiniMax 在首次选择时才创建并保留本窗口状态，以降低首开 CPU、内存与网络开销。外壳只注入静态 chrome-row 顶部品牌条带 + 拉绳挂件，不跑常驻动画，避免 WKWebView 空闲时持续渲染。窗口不覆盖 user-agent：WebView2 本身就是真实的桌面版 Edge，原生 UA、`Sec-CH-UA` 与 `navigator.userAgentData` 一致（曾经改写成 Chrome 反而制造「HTTP 层报 Edge、JS 层报 Chrome」的自相矛盾，正是环境检测的特征）。专属目录同时充当持久化配置档案，DeepSeek 登录态跨重启保留；同一按钮在窗口已开时变为「关闭官方对话」，复用现有窗口并 `set_focus`。设计细节与 `OFFICIAL_CHAT_BROWSER_ARGS` 见 [docs/architecture.md](docs/architecture.md)。
- **macOS / Windows 自定义标题栏**：管理面板在 macOS 和 Windows 上使用前端自绘的窗口标题栏，主色带从左到右以 5% 到 70% 的不透明度叠加深 Gitea 绿，并保留毛笔笔触纹理；dev 构建切换为同样规则的低亮度鲸眼红色系；Linux 暂保留系统标题栏。窗口按钮按各自平台惯例绘制——macOS 是左上角红黄绿交通灯；Windows 是右侧最小化 / 关闭按钮（46×32 命中区、10 px 细线字形，hover 覆浅色底，关闭 hover 变系统红；窗口不可缩放故不提供最大化）。无边框由 `tauri.conf.json` 的 `decorations: false` 在建窗时给定，macOS 的窗口按钮语义与系统原生一致。
- **Windows 常驻通知区域**：Windows 上管理面板是常驻后台的——关闭与最小化都把窗口收进通知区域，并从任务栏移除它的按钮（`ITaskbarList::DeleteTab`，任务栏与 Alt+Tab 都不再留一个点了没反应的窗口）。内核、工作台、官方对话与更新检查继续运行。托盘图标是重新打开与退出的唯一入口——右键菜单「显示主界面 / 退出 dsh-xlink」、左键单击直接叫回窗口；真正退出走托盘菜单并复用「确认退出」流程（内核运行时会先询问，确认后依次停止内核、销毁窗口、退出进程）。从通知区域重新打开时会提示一次「刚才已收起到通知区域」——窗口在隐藏状态下收不到提示，避免看起来像崩溃。
- **内核更新**：官方发布到 npm registry 的 `@deepseek-ai/dsh`（以及同名 `dsh-*` 依赖包）页面 [`https://www.npmjs.com/package/@deepseek-ai/dsh`](https://www.npmjs.com/package/@deepseek-ai/dsh) 与 GitHub `dsh-v<semver>` tag 一一对应。更新菜单直接读 npm registry 拿到全量版本与 `dist-tags`，可安装、切换、删除任意已发布版本；只有 npm registry 不可达时才回退 GitHub Releases API 与其 Atom feed。
- **多内核并存**：P0–P8 阶段已落地——内核族注册表、实例切换、插件按实例物化、技能全局共享、数据迁移向导与 release threshold 验证。`KERNEL_FAMILY_DSH = "dsh"` / `KERNEL_FAMILY_MCODE = "mcode"` 共存于 `KernelAdapter::adapters()` 注册表。阶段性状态见 [docs/multi-kernel-migration-status-2026-09-19.md](docs/multi-kernel-migration-status-2026-09-19.md)，设计稿见 [docs/dsh-xlink-multi-kernel-design.md](docs/dsh-xlink-multi-kernel-design.md)。实际数据布局以 [docs/architecture.md §「多内核改造后的实际数据布局」](docs/architecture.md) 为准。

## 功能

- **一键启动 / 停止工作台**。「概览」页主按钮切换内核状态；同排的「打开工作台窗口」「打开官方对话」「查看日志」是并列的次级入口，不改变内核状态。
- **打开官方对话**：拉起独立的官方对话窗口，按 `OFFICIAL_CHAT_TABS` 顺序排布 DeepSeek / 千问 / MiniMax 三个页签，与工作台窗口互不干扰。使用原生 Edge UA 与可持久化登录的专属 user-data 目录。窗口已开时按钮变为「关闭官方对话」并销毁当前窗口。
- **多内核并存**：窗口顶部一排内核 tab——DSH（active）、mcode（mock，结构性已就位）等内核族并列。每个内核族可同时跑多个实例；概览页底部「实例切换器」tab 直接在主页面切实例。侧栏菜单、插件页、技能页、设置页都跟随当前实例，互不串。已迁移用户在「概览」页不再显示「数据迁移」入口（数据迁移向导在「设置」页常驻，可点进查看最近一次迁移）。
- **更新菜单**：列出 npm registry [`@deepseek-ai/dsh`](https://www.npmjs.com/package/@deepseek-ai/dsh) 的所有发布版本（含预发布标记），可安装、切换活动版本、删除本地版本。
- **内核安装通过 pnpm**：`node-linker=hoisted` 保持扁平 `node_modules`，内容寻址存储让重复安装更快；安装过程逐行流式显示在进度面板中，完整日志落盘 `~/.dsh-xlink/shell/release/logs/<kind>-install-<版本>-<日期>.log`（dev 壳则是 `~/.dsh-xlink/shell/dev/logs/dev-install-<版本>-<日期>.log`，`<日期>` 为本地日期）。下载先写临时文件，成功后才发布；npm 包由外壳进行路径受限、禁止链接和有展开大小上限的 Rust 解包，无需额外安装系统 `tar`。
- **Node.js 自动检测与手动指定**：要求 `^22.19 || >=24`，与 dsh 的 engines 一致。自动发现 nvm（macOS/Linux `~/.nvm/versions/node/<v>/bin/node` 跟随 `alias/default` 链，Windows `%NVM_SYMLINK%` 与 `%NVM_HOME%/v*/node.exe`），免去 GUI 启动看不到 nvm PATH 时改手动路径的步骤。检测为空时弹窗询问是否「帮我安装」——确认后自动下载官方 Node.js（v24 LTS，SHA-256 校验）到数据目录 `tools/node/`；概览页 Node 行随时可再次触发。已安装的托管运行时优先于环境检测，显式配置的 node 路径仍最高优先。
- **pnpm 路径可配置**（默认取 node 同目录或 PATH）。
- **端口可配置**：release 默认 3090，`tauri dev` 下的 dev 壳默认 3091。设置页的「设置」卡只保留这一项可改的东西（插件接线 profile 名是固定值，跟着端口一起保存）。概览页底部的「桌面端设置」卡只读显示当前端口、profile 名与 Node 环境结论。概览页「当前内核」的 Node.js 行提供「重新检测」（重新探测本机环境、不改设置，不达标时同排还有「自动安装」）。
- **内核运行日志查看**；应用退出时自动回收内核子进程。
- **任务完成通知**：macOS Dock 图标右上角系统数字角标、Windows 任务栏图标同款数字角标（系统原生角标 + 覆盖图标），两端同时弹一条系统通知气泡（「会话标题」已完成 · 用时 N 分 N 秒）。角标数字是已完成但用户未读的任务数——工作台窗口不在前台时完成的任务才计数（正在看工作台时不打扰），切回工作台或点面板的「全部已读」即清零。检测方式是让外壳以客户端的身份订阅内核自己的 WebSocket 流（`/api/remote.mux` 的 `$events` 判完成、`session/control` 取会话标题），不轮询、不占内核 CPU，内核没开窗口也照常工作。子代理会话不打扰，通知开关与「试听」提示音都在「设置 → 任务通知」里（造一条假完成的自检按钮只在 dev 构建里显示）。设计与取舍见 [docs/notification-design.md](docs/notification-design.md)。
- **插件管理（按实例定制）**：社区插件（npm 包或 GitHub 仓库）由外壳统一管理，源存放在外壳自己的数据目录（`DSH_XLINK_HOME` 解析）。中央库一份，按实例各物化一份：每个实例把中央库的链接（默认，Windows 自动降级复制）落到自己的 `extensions/plugins/<id>/`，再由该实例自己的 `extensions/wiring.json` 记录 profile 接线——不同实例可以装不同插件、不同模式、不同启停状态，切换实例无需重装。GitHub 仓库地址安装时优先使用对应 GitHub Release 的 tarball 版本数据，Release 不可用时回退 git clone，其它 Git 地址保持原有 clone 行为。「插件中心」对接 [dshfind.com](https://dshfind.com/zh) 插件超市目录（分类 / 搜索 / 排序 / 已安装过滤，6 小时本地缓存，官方 market 兜底）。面板提供安装 / 卸载 / 更新 / 切换模式 / 同步，检测到新版本时在卡片与启动时提醒；卡片头部显示「N 个更新可用」红色数字圆点徽标。「同步」重新物化中央库中的插件，并清除外壳明确标记的已删除残留，保证外壳管理的插件状态与中央库一致。`link` 模式插件启动前会检查中央目录中的普通运行时依赖，缺失时自动用 pnpm 恢复。多实例隔离规则与权威目录布局见 [docs/architecture.md §「多内核改造后的实际数据布局」](docs/architecture.md)。
- **插件面板信息架构**：单 panel 双 tab——「当前内核」显示本实例的安装 / 启停 / 更新；「已安装」展示全部实例的插件视图（含「本实例」标签 + 悬停提示）。面板骨架屏 + 并行加载 + 收紧切换动画以提升渲染速度。
- **工作台健康自检**：工作台窗口自动监听白屏、运行时错误和未处理的 Promise 异常。外壳会把前端证据（异常类型与消息、`cause` 链、堆栈、页面地址）与今天的内核日志一起分析，归类为「疑似插件」「疑似内核」「前端 bundle 异常」「运行环境问题」或「暂未能归因」，并在事故面板展示证据和对应的处置入口。「前端 bundle 异常」不弹事故面板（页面仍在运行、这类异常没有可处置对象），只在概览页横幅提示，点「查看详情」展开完整证据——按提示先看日志、反馈错误消息，再考虑停用第三方插件或切换内核版本。完整设计见 [docs/troubleshooting.md](docs/troubleshooting.md)。
- **技能管理（全局共享）**：与插件相反——社区技能（npm 包 / GitHub 仓库 / 本地文件夹）由外壳统一管理，源存放在 `DSH_XLINK_HOME/skills/packages/`，按包安装的粒度以链接（失败降级复制）物化进一份 v1 全局共享的活动视图（`skills/active/`）。所有内核实例、DSH 与未来的 mcode 都从同一份活动视图读，由各自适配器通过 `DSH_CUSTOM_SKILL_DIRS` 注入——多实例的技能视图天然一致，不需要每个实例各自维护一份。不改 cordis 配置、不装依赖、切换实例零操作。内核对技能根做文件监视，安装 / 卸载 / 更新对运行中的工作台即时生效，无需重启。安装前逐个校验 SKILL.md frontmatter（kebab-case `name` + `description` 必填），避免「装了却不出现」。已安装卡片在包头提供逐个启用 / 停用开关（粒度是单个技能），停用只把条目移出活动视图，包仍留在中央库，随时可恢复。中央库与活动视图的条目状态包括「未同步」——本地活动根条目与中央库记录不一致（如外部修改了源目录）时显示，提示用户先「重新同步」。多实例共享活动视图的设计理由见 [docs/architecture.md §「多内核改造后的实际数据布局」](docs/architecture.md)。
- **数据迁移向导**：「设置」页常驻入口（未迁移亮色，已迁移灰色均可点进）。嵌入式 4 步向导——发现 → 选择 → 运行 → 完成 / 回滚。迁移运行期间走 `ProgressOverlay` 与安装内核 / 装插件共享同一进度 UI。凭据与会话首版不纳入迁移（旧版默认保守）；冲突策略默认 `SkipIfNewer`（保留用户后来修改）；旧源永不被删除（rollback 路径依赖）。完成后回主界面，顶部 banner 报告最近一次迁移的状态（不再展示完整历史）。完整设计见 [docs/migration-wizard-ui-proposal.md](docs/migration-wizard-ui-proposal.md)。
- **内置补丁（内核补丁 / 小插件）**：随 dsh-xlink 发布包捆绑的自研内核补丁与小插件（`src-tauri/resources/patches/<id>/`，发布时进入 app 资源目录，与社区插件不同、无需第三方信任），默认不生效。在「设置 → 内核补丁」页自主选择「应用到当前内核」或「撤销补丁」；应用前自动备份被覆盖的原文件到 `~/.dsh-xlink/dsh/desktop/patches/backups/`，撤销时从备份还原，备份丢失时以内容 SHA-256 校验兜底、绝不盲目覆盖或删除。支持 `copy`（新增 / 覆盖文件）与 `replace`（精确字符串替换）两种文件操作，目标路径严格限制在内核目录内，可按 `minKernelVersion` / `maxKernelVersion` 声明适用内核版本范围。当补丁功能被官方内核采纳后可通过 `supersededSinceKernelVersion` 字段声明「从该内核版本起已被官方取代」，UI 把对应卡片折叠为「已并入官方内核」（删除线 + 默认收起 + 应用按钮禁用），用户可手动展开查看。应用记录持久化在 `~/.dsh-xlink/dsh/desktop/patches/state.json`，按「补丁 × 内核版本」隔离；工作台运行期间禁止操作。`dsh-file-perf`（dsh `@` 引用性能修复）已被官方 0.1.2-alpha.2 起直接采纳，卡片折叠为「已并入官方内核」；`dsh-session-perf`（历史会话列表加载提速）v1.3.0 锚定官方 0.1.5-alpha.2 ~ 0.1.5-rc.2；`dsh-escalation-same-mode`（同模式 sandbox 升级短路）v1.1.0 锚定 0.1.3-alpha.2。旧内核上的已应用记录仍可撤销。设计文档见 [docs/patch-management.md](docs/patch-management.md)。

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
        ├── commands.rs       # Tauri 命令（含插件/技能/补丁/迁移与窗口操作）
        ├── kernel.rs         # 安装 / active / 启动 / 停止 / 端口探测
        ├── kernel_adapter.rs # 多内核族适配器（DSH / mcode 等）
        ├── migration.rs      # 数据迁移向导后端（preview / run / rollback / list）
        ├── notify.rs         # 任务完成通知：事件流订阅、未读角标、系统通知气泡
        ├── plugins.rs        # 插件中央库、物化、接线与更新
        ├── patches.rs        # 内置补丁：清单、备份、应用/撤销、状态
        ├── skills.rs         # 技能中央库、物化、启停与更新
        ├── releases.rs       # 官方发布列表（npm registry → GitHub 回退）
        ├── pkg.rs            # 插件与技能共用的包取源层
        ├── state.rs          # JSON 状态文档读写骨架
        ├── node.rs           # Node/pnpm 检测与版本校验
        ├── node_install.rs   # 托管 Node.js 安装（按需下载到数据目录）
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

根目录的 `pnpm-workspace.yaml` 让 pnpm 把本项目当独立根处理，直接跑 `pnpm install` 或 `npm install` 也行。

产物位于 `src-tauri/target/release/bundle/`（macOS 为 `.dmg`，Windows 为 NSIS 安装包 `.exe`）。

## 使用

1. 启动桌面应用，打开管理面板。
2. **Node.js 环境**：概览页「当前内核」的 Node.js 行显示实时检测结果，刚装完 Node 可点同排「重新检测」刷新（它只探测本机环境、不改设置）。不满足要求时点同排「自动安装」自动下载官方 Node.js 到数据目录（首次启动检测不到时会弹窗询问，点「帮我安装」同效），或手动安装 Node 22.19+、在 `<data_dir>/settings.json` 的 `node_path` 里手动指定路径。通过 nvm 管理的 Node 会被自动发现。
3. **内核更新**：应用启动时会扫描并列出本地已安装版本，进入「内核版本」页即可在左侧备用版本中切换。只有工作台已停止时才能切换；工作台启动或运行期间请先在「概览」页点击「关闭工作台」。点击「检查更新」只从 npm 获取官方发布列表，再选择未安装的版本点「安装」。安装通过 pnpm 执行，进度面板会实时滚动 pnpm 日志；pnpm 未安装时按提示 `npm install -g pnpm` 或在设置中指定 pnpm 路径。首次安装会自动成为活动版本，但不会启动内核；安装完成后请在「概览」页点击「启动工作台」。之后安装的版本不会覆盖当前活动版本，可随时在「已安装」列表中「切换」或「删除」。
4. （可选）**插件** → 在「插件中心」按分类浏览、搜索（即时过滤）、按 Star / 更新时间排序后一键安装，或手动填写 npm 包名（如 `@ace-zone/dsh-market`）/ GitHub 仓库 URL 安装。安装前自动校验插件是否符合 dsh 规范（package.json / `dsh.bundle.patch` / 入口文件），安装完成后重启工作台（关闭后重新启动）生效。点击「同步」会对所有已安装内核重新物化中央插件库，并清除外壳标记的已删除插件残留。进入「内核版本」页后，每个已安装版本旁的信息图标可悬停查看该版本实际物化的插件、版本和链接 / 拷贝模式。
5. （可选）**设置 → 内核补丁（内置）**：查看随当前 dsh-xlink 版本捆绑的内核补丁与小插件（来自本应用发布方，与社区插件不同），自主选择「应用到当前内核」或「撤销补丁」。应用前自动备份被覆盖的原文件、随时可撤销，状态与备份记录在 `~/.dsh-xlink/dsh/desktop/patches/`（dev 壳为 `~/.dsh-xlink/dsh/desktop-dev/patches/`）。工作台运行期间不能操作，请先关闭工作台；切换内核版本后需对新的活动版本重新应用。补丁与适用内核版本详见 [docs/patch-management.md](docs/patch-management.md)。
6. 在「概览」页点击「启动工作台」：自动拉起内核、等待就绪后校验当前内核的工作台地址，再打开工作台窗口进入 Harness 界面；启动失败会自动弹出事故面板和内核日志。「关闭工作台」会同时关闭工作台窗口并停止内核。工作台窗口的系统关闭按钮（macOS 交通灯红灯 / Windows ×）始终可用，只收起窗口、内核与任务继续在后台运行；内核运行中收起窗口后，随时可用「打开工作台窗口」重新打开。工作台窗口会自动进行健康自检——发现白屏、运行时错误或未处理的 Promise 异常时，事故面板会展示异常类型 / 消息 / 堆栈与页面地址，并标注归类（「疑似插件问题」「疑似内核问题」「前端 bundle 异常」「运行环境问题」「暂未能归因」）。插件问题可重新启用或移除；内核问题可先停止工作台，再打开日志并切换 / 重装版本；「运行环境问题」（端口被占用、数据目录不可写、磁盘已满等）指向设置页与日志，面板按钮会直接去设置页。工作台窗口侧栏头部右侧（品牌 logo 旁）悬浮着一个灯泡拉绳小挂件：点击（拉动）它，灯泡点亮的同时桌面端管理面板会归位到点击位置附近并提到当前桌面上方，方便随手操作；若灯泡闪红，说明与桌面壳的通信失败，可查看工作台 DevTools 控制台。
7. 「打开官方对话」：在「概览」页点击此按钮即可拉起独立的官方对话窗口（顶部条带 chrome-row 官方品牌蓝 `#4D6BFE`、拉绳挂件挂页签栏右侧 12 px；区别于工作台窗口的 Gitea 绿色 212 px 偏移），按 `OFFICIAL_CHAT_TABS` 顺序排布 DeepSeek / 千问 / MiniMax 三个页签。窗口已开时按钮变为「关闭官方对话」并销毁当前窗口。
8. （可选）**设置 → 数据迁移**：从旧版 dsh home 布局搬到新多实例布局——嵌入式 4 步向导。凭据与会话首版不纳入迁移，冲突策略默认 `SkipIfNewer`，旧源永不被删除。已迁移用户在「概览」页不再显示入口，仍可在「设置」页回查。
9. 首次使用时在 Harness 的设置页配置 DeepSeek（`DEEPSEEK_API_KEY` 等）即可开始对话。

数据目录（按内核族命名空间隔离，统一在 `~/.dsh-xlink/` 下）：

- 外壳数据（已装内核 `kernels/`、活动指针 `active.txt`、补丁 `patches/`、隔离记录 `quarantine.json` 等）：`~/.dsh-xlink/dsh/desktop/`（release 壳）或 `~/.dsh-xlink/dsh/desktop-dev/`（dev 壳）。将来接入新内核族（如 mcode）会得到各自独立的 `~/.dsh-xlink/mcode/desktop[-dev]/`。可用 `DSH_XLINK_HOME` 重定向整个根目录，`DSH_DESKTOP_DATA_DIR` 完整覆盖外壳数据目录
- 外壳自身日志与每壳设置（release / dev 分槽）：`~/.dsh-xlink/shell/<release|dev>/`（`logs/`、`settings.json`、`ui-state.json`）
- 多内核相关（内核版本与实例 `kernels/<族>/`、中央插件库 `dsh-plugins/`、技能库 `skills/`）：权威布局见 [docs/architecture.md §「多内核改造后的实际数据布局」](docs/architecture.md)
- 内核自身数据（会话、凭据、配置、profile）：实例内核 home `~/.dsh-xlink/kernels/dsh/instances/<id>/home/`——启动内核时外壳以 `DSH_HOME` 环境变量注入，内核进程的全部用户数据都落在这里，不再使用 `~/.dsh`

> 从 v0.2.x 升级：平铺的 `~/.dsh-xlink/desktop[-dev]/` 会在新版首次启动时自动整体搬进 `~/.dsh-xlink/dsh/`；搬迁失败时继续使用旧目录，数据不会丢失。更早版本（元数据在系统应用数据目录或 `~/.dsh/desktop/`）的数据不再被读取，如需保留请手动移入上述外壳数据目录。
>
> 内核数据的一次性搬迁：旧版外壳不注入 `DSH_HOME`，内核把会话、凭据、profile 写在 `~/.dsh`。新版首次启动会把其中内核拥有的数据（`profiles/`、`sessions/`、`storages/`、`attachments/`、`logs/`、`.credentials.yaml`、`settings.yaml*` 等）自动并入实例内核 home：逐项递归并入、目标已有的条目以新目录为准（不会覆盖接线产物），中断后下次启动自动续跑；`~/.dsh` 里外壳拥有的旧目录（`desktop/`、`plugins/`、`skills*` 等）不在搬迁清单内，原样保留。

## 发布（GitHub Actions）

工作流：[`.github/workflows/desktop-release.yml`](.github/workflows/desktop-release.yml)

- 支持平台：**Intel macOS**（`macos-15-intel`，`.dmg`）+ **Windows x86_64**（`windows-latest`，NSIS `.exe`）
- 触发方式：
  - 手动在 Actions 页从 `main` 触发 `workflow_dispatch`（使用当前 `package.json` 版本，推荐，后续版本可复用 Rust 编译缓存）
  - 推送 tag：先同步 `package.json` 与 `src-tauri/tauri.conf.json` 的 `version`，再 `git tag desktop-v<version>` 并推送
- 发布来源限定为 `main` 分支，产物发布为正式 release，不是 draft 或 prerelease。
- 发布前质量门禁：UI 回归测试与生产构建、JavaScript 700 kB / CSS 180 kB bundle 预算、Rust `cargo test`、`cargo fmt --check` 和 `cargo clippy -D warnings` 全部通过后才允许发布。
- 发布提速：预检通过后，质量门禁与 Intel macOS、Windows 两个构建 job 并行运行；平台 job 只上传 Actions artifact，全部成功后由独立的 publish job 一次性创建正式 Release 与 `latest.json`。`max-parallel: 2`、pnpm store、Cargo registry 和按平台隔离的 Cargo target 都启用缓存，Rust release 使用 thin LTO 与 16 个 codegen units。缓存只在 `main` 分支保存，手动发布可跨版本复用；直接推送新 tag 通常会冷启动。详细时序与排障见 [`docs/release.md`](docs/release.md)。

> GitHub Actions 首次建立缓存时仍会经历冷启动；runner 排队、缓存服务和网络波动也不属于 workflow 可控的构建时间。
>
> 签名说明：当前产物未做代码签名，Windows SmartScreen 与 macOS Gatekeeper 可能给出警告。加入签名（Apple Developer ID / Windows 代码签名证书 + 对应 secrets）后再去掉相关提示。

## 常见启动失败与处理

| 症状 | 排查 |
| --- | --- |
| `WebviewWindowBuilder` 创建工作台窗口卡死 | Tauri 2.x 在同步命令里创建 webview 窗口**会死锁**（Windows 100%；macOS/Linux 部分情况下也慢）。本项目 `open_harness` 已经把创建放在新线程（`commands.rs::open_harness`）。新增类似命令请保持同样模式。 |
| macOS 启动后访问 `http://127.0.0.1:3090`（dev 壳为 3091）失败 | Tauri 2.x 默认 WKWebView 已允许本地环回访问，不需要 `NSAppTransportSecurity` 例外；本项目移除了该字段，依赖平台默认值。 |
| 编辑器 / IDE 报 `capabilities/default.json` 找不到 `$schema` | schema 文件在首次 `tauri build` 后由 `tauri-build` 生成；本项目移除了硬编码 `$schema` 引用，避免初次克隆时编辑器红字。 |
| 升级后「已安装」列表为空 | 外壳元数据在 `~/.dsh-xlink/dsh/desktop/`（v0.2.x 平铺目录会自动搬入）；按上文「数据目录」提示确认目录位置，或重新安装内核。 |

## 性能排查

- 工作台与官方对话的顶部品牌线采用静态渲染，不会在空闲时保持 WebKit 帧循环。安装包含 `src-tauri/src/titlebar-pulse.js` 修改的桌面壳后，需要完全退出并重新启动应用，再重新打开工作台；已存在的 WebView 不会自动替换初始化脚本。
- `dsh-personal-center` 是第三方可选插件。桌面宠物的统计接口会同步读取并解压全部历史会话；会话较多时可能造成明显的 Node 磁盘 / CPU 阻塞。遇到周期性卡顿时，在个人配置中关闭桌面宠物和「会话状态」，需要统计时再手动打开 Token 用量页面。
- `patchReload: live` 会启用 client-HMR 的 500 ms bundle `stat` 轮询。日常使用可改为 `startup` 并在 profile patch 中禁用 `client-hmr`；需要调试 client plugin 时再恢复 `live`，删除该禁用项。
- 当前内核为 `0.1.5-alpha.2` ~ `0.1.5-rc.2` 时，可在「设置 → 内核补丁」停止工作台后应用 `dsh-session-perf`，减少会话列表的重复目录 / header 扫描；它不优化选中会话后的完整历史解压。

## 已知限制与后续

- **Node 运行时**：当前按需托管安装到数据目录（自动检测 → 一键装）；后续可考虑随发布包捆绑 Node sidecar（体积 +40 MB / 平台）。
- **pnpm 依赖**：内核安装依赖用户环境中的 pnpm（未捆绑）；后续可评估 `corepack` 或 sidecar 方式随应用分发。
- **端口冲突**：若 3090（dev 壳为 3091，以设置页显示为准）已被其他进程占用，先停止外部服务，或在设置页改用其它端口——注意「工作台运行期间不能改端口」，需先关闭工作台再保存。
- **安全**：应用通过 Webview 加载本地 `http://127.0.0.1` 的 Harness 页面并暴露版本管理命令；仅信任官方 `deepseek-ai` 仓库与 npm 的 `@deepseek-ai` 命名空间。插件和技能是第三方内容 / 任意代码，安装前请自行确认来源；社区目录条目保留「未验证」标记。npm 包解包拒绝绝对路径、父级路径、符号链接、硬链接和特殊文件，并限制条目数与展开体积。外壳自己下载的 tarball 一律逐字节校验：优先用 registry 元数据的 `dist.integrity`（SRI，取最强且受支持的 sha512 / sha256），只有它没有可用摘要时才回退到老 packument 的 `dist.shasum`（sha1），两条都没有则拒绝安装。镜像只影响取源，信任边界不动——包名仍限定 `@deepseek-ai` 命名空间，下载的 tarball 仍逐字节校验，因此镜像不能替代对第三方代码的审计。
- **插件链接模式**：依赖文件系统符号链接支持（Windows 需要开发者模式，失败会自动降级为复制模式并在行内显示「复制」徽标）。
- **自动更新**：桌面端使用 `tauri-plugin-updater` 下载并校验签名。Windows 更新重启后，管理面板完成首次状态刷新即会清理更新前的旧安装目录、快捷方式和 updater 临时目录；清理失败会保留标记，并在下次启动重试。
