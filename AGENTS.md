# AGENTS.md — dsh-xlink

本仓库是 dsh-xlink 桌面应用的独立项目。模块布局与数据流见 [docs/architecture.md](docs/architecture.md)，用户文档见 [README.md](README.md)。

## 范围

- **独立项目**：仓库根目录就是桌面交付物，不加入任何上级 pnpm workspace，也不依赖源仓库的构建、测试或发布门禁。根目录 `pnpm-workspace.yaml` 让 pnpm 将本项目作为独立根目录处理，直接运行 `pnpm install` 即可。
- **运行时内核边界**：项目不携带或重新发布 dsh 内核代码。内核由用户从 npm registry 安装；桌面壳通过 `src-tauri/` Rust 进程和 `ui/` 管理面板管理其生命周期、配置和窗口行为。
- **信任边界**：仅信任官方 `deepseek-ai` 仓库与 npm `@deepseek-ai` 命名空间；版本列表优先 npm registry，GitHub Releases 仅作回退。

## 开发规则

- 搜索文本或文件时优先使用 `rg`；仅在不可用时再使用 `grep` 等替代命令。

## 命令

```sh
npm run deps                      # 安装依赖（pnpm 优先，缺失回退 npm）
npm run dev                       # tauri dev（自动先起 vite dev server，5173 热更新）
npm run dev:ui                    # 只起管理面板 dev server（纯前端迭代，浏览器里无 Tauri 桥）
npm run build                     # 本机构建（.dmg / NSIS；自动先 vite build → ui/dist）
npm run build:ui                  # 只构建管理面板 → ui/dist
cargo check                       # 在 src-tauri/ 内：快速编译检查
cargo clippy --all-targets        # lint，零警告基线
cargo fmt                         # rustfmt 格式化
```

UI 是 Vue 3 + Element Plus 单页应用（源码 `ui/src/`，Vite 构建到 `ui/dist/`，即 `src-tauri/tauri.conf.json` 的 `frontendDist`）。状态与动作集中在 `ui/src/store.js` / `plugins.js` / `skills.js` / `progress.js` / `logs.js`，组件只读状态、调动作；与 Rust 的通信只允许走 `ui/src/bridge.js` 的 invoke/Channel。触发 IO 的按钮必须挂 loading（`loading.js` 的 `withLoading(key, …)` + `:loading="isLoading(key)"`）；长任务走 `progress.js` 的 `withProgress`。改完 UI 跑 `npm run build:ui`；Rust 改动至少跑 `cargo check`，提交前跑 `cargo clippy --all-targets && cargo fmt`。

## 数据目录

`kernel::data_dir` 统一解析 `<dsh_home>/desktop/`（release）或 `<dsh_home>/desktop-dev/`（debug）。`lib.rs` 的 `setup()` 必须通过它取目录，不要绕回 `app_data_dir()`。debug 端口 3091，release 端口 3090（`kernel::DEFAULT_PORT`）；用户保存过的 port 优先于 `Settings::default()`。优先级与目录隔离原因见 [docs/architecture.md](docs/architecture.md)。

## 实现约定

- 用户可见文案用简体中文；错误信息必须包含可操作的下一步与相关日志路径。
- 概览页只暴露「启动工作台 / 关闭工作台」单按钮状态机；「打开工作台窗口」「查看日志」是次级入口。
- 长任务失败时进度面板保持开放，由用户手动关闭；完整原始输出始终落盘，报错信息引用日志路径。
- 所有 GUI 子进程使用 `process.rs` 的 PATH 合并、静默窗口和进程组回收策略；涉及进程、网络或目录树的 Tauri 命令必须异步执行并使用 `spawn_blocking`。
- 图标只从 `assets/whale-icon.svg` 与 `assets/whale-icon-small.svg` 生成，规则见 [docs/icon-design.md](docs/icon-design.md)。

## 发布

版本发布由 `.github/workflows/desktop-release.yml` 负责。`desktop-v<version>` tag 和手动 dispatch 只接受 `main` 分支上的 commit；发布前同步 `package.json` 与 `src-tauri/tauri.conf.json` 的 `version`。workflow 使用 `TAURI_SIGNING_PRIVATE_KEY` 给更新制品签名，`releaseDraft` 与 `prerelease` 必须保持为 `false`，以保证 updater 的 latest endpoint 可用。

- **发布平台**：dsh-xlink 只发布 Intel macOS（`macos-15-intel`）和 Windows（`windows-latest`）版本；不得添加、构建或发布任何 Linux/Ubuntu 版本、runner、制品或文案。

## 文档

修改用户可见行为、数据目录、发布流程或安全策略时，同步更新 `README.md` 和对应 `docs/` 文档。文件保持 UTF-8、恰好一个末尾换行；不要提交依赖目录和构建产物。
