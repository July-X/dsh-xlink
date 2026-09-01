# 预研：内嵌最小 Node.js 运行时的可行性

> 目标：让用户只选择官方内核版本，不关心 Node.js 环境。本文是面向决策的预研，
> 所有可验证事实均标注来源；体积/系统版本数字在 macOS（本机）实测，Windows 数字取自官方产物。

> **决策更新（已实施）**：不把 Node 二进制绑进安装包（避免安装包与每次更新体积增大），
> 改为「按需托管安装」：检测到缺少可用 Node 时弹窗询问用户，确认后从官方
> nodejs.org/dist 下载固定版本（v24 LTS）到数据目录 `tools/node/`，SHA-256 以官方
> SHASUMS256.txt 为准；`node.rs::resolve` 解析顺序为 显式配置 → 托管运行时 → 环境检测。
> 实现见 `node_install.rs` 与 `commands::install_node`（UI：概览页「自动安装」/ 弹窗「帮我安装」）。
> 本文保留捆绑方案的完整预研数据，供将来在「体积敏感场景」重新权衡。

## 结论

**可行，且改动面小。** 当前架构的内核启动链路已经把所有 Node 解析收敛在
`node.rs::resolve` 一个入口（`commands.rs` 用 per-app 缓存调用），全部子进程经
`process.rs` 的 `command_with_path()` 启动并注入合并 PATH（`kernel.rs` 已把 node 目录
前置，shebang 类脚本开箱即用），`patches.rs` 已有 `app.path().resource_dir()` 读
`<resource_dir>/patches/` 的先例——嵌入运行时只是给 `node.rs` 增加一个候选、并在
打包层加一个二进制资源，内核生命周期、插件、补丁、日志、更新流程零改动。

推荐形态：**安装包捆绑官方二进制 + 显式设置优先 + 环境检测兜底**，优先级为

`settings.node_path（显式） > resource_dir/embedded-node（内置，默认） > PATH/nvm/常见位置（兜底）`

## 关键事实（实测/一手来源）

| 事实 | 数值 | 来源 |
| --- | --- | --- |
| Node v24.20.0 darwin-x64 tar.gz（下载体积） | 51.5 MB | https://nodejs.org/dist/v24.20.0/ （Content-Length 实测） |
| 解包后 macOS `bin/node` 单文件 | 119 MB（124,285,824 B） | 本机解包 `du -h` 实测 |
| 解包全树（含 npm/corepack/man） | 201 MB | 同上 |
| Node v24.20.0 win-x64 zip（下载体积） | 35.8 MB | https://nodejs.org/dist/v24.20.0/ 实测 |
| 解包后 Windows `node.exe` | 89 MB（93,381,448 B） | unzip 实测 |
| 系统版本下限 | macOS **≥13.5**（LC_BUILD_VERSION minos 13.5） | `otool -l bin/node` 实测 |
| License | MIT（tarball 内含 LICENSE 文件） | 解包 LICENSE 实测 |
| 官方二进制签名 | 自带 CodeDirectory，hardened runtime 标志 | `codesign -dv` 实测 |
| 附带工具 | npm / npx / corepack 均在 dist 内（win 有 corepack.cmd） | 解包 bin/ 实测 |
| 校验 | SHASUMS256.txt + .asc/.sig 签名 | https://nodejs.org/dist/v24.20.0/ |

版本与 engines 匹配：当前约束 `^22.19 || >=24`，内置选 v24 LTS 线（latest-v24.x =
24.20.0）即满足；v24 于 2025-10 进入 LTS（按 Node 版本节奏），覆盖本产品两平台
（darwin-x64 / win-x64）均有官方产物。

## 评估四轴

### 授权与费用

- MIT，重分发免费；**触发条件**：保留 LICENSE 与第三方版权声明（tarball 内自带，
  随资源一并打包即可）。
- 无任何付费触发点；官方二进制重分发是标准做法（Electron/VS Code 等桌面软件同款路径）。

### 技术成熟度

- Node LTS 线 + 官方 dist 二进制分发，多年稳定；`nodejs.org/dist` 目录即分发标准。
- 本产品只发布 macOS Intel + Windows，恰好只需要两个平台单文件，不用引入
  node-installer 类第三方打包件。

### 能力上限（不做额外开发时）

- 单二进制即可运行 dsh 内核（`bin.js` 纯 JS；shebang 由现有 PATH 注入解决）。
- 插件原生模块（node-gyp 编译的 `.node`）不受影响：node 是独立子进程，不带本
  App 的签名，不存在 library validation 限制（官方 node 二进制自身带 hardened
  runtime，但那是 Node 自己的签名环境，加载普通 `.node` 无碍——POC 需实测验证）。
- 做不到的：**换运行时（Bun/Deno）跑内核**——非官方支持，破坏插件生态，不采用。

### 必需工具链

- 构建期一次性下载 + SHA-256 校验 + 解包（本项目 CI runner 是 macos-15-intel /
  windows-latest，各自取本平台产物，天然一一对应）；无新增运行时依赖。
- 中国大陆下载加速可考虑 npmmirror（`registry.npmmirror.com/-/binary/node/`）——
  这是信任决策：node 二进制供应链与本项目「只信任 deepseek-ai 官方」的信任边界是
  两回事，建议仍以 nodejs.org 的 SHASUMS256.txt（带签名）为准 pin 版本与哈希。

## 关键结论

⚠️ 嵌入方案可行，改动收敛在 `node.rs` + 打包层，推荐直接实施（捆绑版）。

信息源：本文全部实测事实 + 官方 dist URL。分量：**可继续推进**。

⚠️ macOS 系统版本基线冲突：v24.20.0 要求 macOS ≥13.5，而 `tauri.conf.json`
当前 `minimumSystemVersion: 10.13`。选择：① 基线随内置运行时提升；② 选 minos
更低的 Node 版本（22.19+，需同法 `otool` 验证，且仍在 10.13 之上）；③ 保留
10.13 支持但旧系统回落系统 Node 检测（内置 probe 失败即回落，代码天然支持）。

信息源：`otool -l` 实测 minos=13.5 vs 本仓库 tauri.conf.json。分量：**实施前必须
决策**，否则旧 Mac 用户拿到不可执行的内置运行时。

## 实施面改动清单（逐处落点）

1. **打包**：build 脚本（`beforeBuildCommand` 或 `npm run deps` 同款）下载对应
   平台 node 产物 → 校验 SHASUMS256 → 解出单文件到
   `src-tauri/resources/embedded-node/`（`.gitignore` 排除，二进制不入库）；
   `tauri.conf.json` `bundle.resources` 增加
   `{"resources/embedded-node": "embedded-node"}`（现有 `patches` 映射同款）。
2. **`node.rs::resolve`**：解析顺序改为 显式配置 > 内置（`resource_dir/embedded-node`）
   > 环境检测；内置 probe 失败（旧 macOS dyld 报错、文件缺失）静默回落，不阻断。
   同步补充单元测试（优先级 + 回落）。
3. **pnpm**：内置 node 目录在签名 App 内是**只读**，`ensure_pnpm` 的
   `npm install -g pnpm` 必须带 `--prefix <data_dir>/tools/npm-global`；或构建期
   `npm pack pnpm` 直接 vendored（纯 JS，离线可用，推荐）。corepack 自带但 cache
   同样必须落 `data_dir`。
4. **UI**：`OverviewPanel.vue` 的 Node 行与 `SettingsPanel.vue` 的检测文案区分
   「内置运行时 Node v24.20.0」与外部检测结果。
5. **版本策略**：内置版本随发行版升级（LTS pin）；未来 dsh 提高 engines 时，靠新版
   发行或用户显式设置覆盖。

## POC 验证步骤（本机 darwin 可直接执行）

1. 资源化：`mkdir -p src-tauri/resources/embedded-node` 并把
   `/tmp/node-v24.20.0-darwin-x64/bin/node` 与 `LICENSE` 拷入。
2. `node.rs` 加内置候选 + 优先级测试；`cargo check`。
3. 在**无 Node 的干净环境**（PATH 清空模拟）下 `npm run dev`：安装内核、启动
   工作台、切版本，全链路不依赖外部 Node。
4. 装一个带原生依赖的插件（如 node-pty），验证 node-gyp 构建与 `.node` 加载。
5. 打包对比：NSIS/DMG 增量约 36–52 MB（zip/tar 下载体积，NSIS 有压缩）；CI 里
   复验签名 + notarization；Windows 同法（node.exe + corepack.cmd 资源化）。

## 已知坑

- macOS 资源目录里可执行位必须保留（git filemode + 打包后复核）。
- 资源目录只读：任何 `npm -g`、corepack cache 写入必须先重定向到 `data_dir`。
- Node 自带 npm 的 `.cmd` shim 走 `process.rs` 已有 script 通道，Windows 无需新机制。
- 更新体积：升级包将随 node 二进制增大（每平台每次更新多拉 ~36–52 MB），若敏感，
  可改「首启按需下载」方案（体积换首启网络依赖，不推荐默认）。
- 内置版本冻结后与未来 engines 脱节：靠发版节奏与 fallback 缓解。

## 范围锁定（不做）

不换运行时（Bun/Deno）、不把 node 内嵌为库（node-embed/napi 单进程模型与本项目
多子进程模型不符）、不提交二进制进 git、不发布 Linux 产物（AGENTS.md 平台约束）。
