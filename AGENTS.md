# AGENTS.md — dsh-xlink

本文件只保留全仓开发约束与任务入口。架构和功能细节按需阅读 [docs/README.md](docs/README.md)，用户说明见 [README.md](README.md)。

## 按改动范围阅读

| 改动范围 | 必读 |
| --- | --- |
| `ui/**` | [ui/AGENTS.md](ui/AGENTS.md) |
| `src-tauri/**` | [src-tauri/AGENTS.md](src-tauri/AGENTS.md) |
| 跨前后端 / IPC | 上述两份，契约只维护一份 |
| 内核安装、取源、依赖或共享服务 | [运行时边界](docs/architecture/runtime-boundaries.md) |
| 路径、实例、跨壳隔离或数据搬迁 | [内核磁盘布局](docs/architecture/kernel-data-layout.md)、[当前架构](docs/architecture/architecture.md) |
| 窗口、托盘、退出、主题或登录自启 | [窗口架构与生命周期](docs/architecture/window-architecture.md) |
| 本机 Codex 登录、外部工具用量 | [本机用量来源契约](docs/features/subscription/local-usage-sources.md) |
| 官网对话 | [开发执行指南](docs/features/official-chat/official-chat-agent-guide.md) |
| 图标与品牌资源 | [图标母版与本地资源规范](docs/ui/icon-design.md) |
| 测试、探针、实机复验或门禁 | [开发验证](docs/operations/development.md) |
| 发布、版本号、CI 或更新制品 | [发布手册](docs/operations/release.md) |

## 范围

- 本仓是独立桌面项目，不加入上级 pnpm workspace，不依赖源仓库门禁。安装用 `pnpm install` 或 `npm run deps`。
- 壳只管理内核生命周期、配置与窗口，不携带或重新发布 dsh 内核代码。
- **已安装内核目录只读**。仅允许 `install_version` 经 `kernel_deps.rs` 写安装 stub（含依赖 `overrides`）及 pnpm 产生依赖树；`uninstall` 的整目录删除须由用户显式要求。不得修改 `kernels/<版本>/node_modules/**`；旧补丁状态仅保留只读兼容，见运行时边界。
- 不改 `deepseek-ai/deepseek-harness` 或其任何 fork，不推上游分支、不提上游 PR。内核缺陷只在本项目规避、登记并上报维护者；实现和验收不得依赖未来上游修复。
- 插件与技能不受 `@deepseek-ai` 命名空间限制。修改取源逻辑须保持下载摘要校验与缺少摘要时拒绝安装的行为，详细信任边界见专题。

## 开发规则

- 搜索优先用 `rg`。新增逻辑先查现有组件、store、服务和共享层，按职责拆分；不为行数压缩代码、强拆模块或预建抽象。不设源码行数或新文件登记预算；重复检测与产物体积预算保留。
- UI 组件只读状态、调用 store 动作；与 Rust 通信只走 `ui/src/shell/bridge.js` 的 invoke / Channel。共享层入口见[运行时边界](docs/architecture/runtime-boundaries.md#共享实现入口)。
- **测试不得访问用户真实数据目录或系统登录项**。凡经 `paths::*` 读写路径，先持有 `crate::tests::scoped_xlink_home`（或模块的 `TestHome` / `TempXlink`）直到用例结束。改 `HOME` / `DSH_HOME` / `DSH_XLINK_HOME` 只走 `scoped_xlink_home` / `scoped_dsh_home`，禁止裸 `set_var` / `remove_var`。清理只限本用例创建的临时目录，不得递归删除解析出的家目录。
- 壳实跑必须同时设置临时 `DSH_XLINK_HOME`、编译期 `TAURI_CONFIG` 的 `app.appDirectoriesOverride` 与独立 `DSH_XLINK_CHANNEL_SUFFIX`。探针另用隔离浏览器档案；每轮证据独立命名，保存在待清理的临时 home 之外。
- 改内置插件后先 `npm run prep:builtin`，再重新构建并重启壳；实机证据核对源码、内嵌资源与物化文件摘要，不能只验插件版本号。
- 长任务失败后保持进度面板开放，由用户手动关闭；完整输出落盘，错误引用日志路径。
- 新增启动恢复或后台接管逻辑时，验证生产调用链已接线，不能只验证纯函数或测试调用。
- 门禁判据放共享脚本，不在 workflow 复制或内联；缺少前置产物必须报错退出。改源码扫描判据须做反向验证，确认破坏的是本轮被测产物、失败原因命中目标断言。

## 命令与验证

在仓库根运行 npm 命令，Cargo 命令在 `src-tauri/` 运行。

| 命令 | 用途 |
| --- | --- |
| `npm run deps` | 安装依赖，优先 pnpm |
| `npm run dev` / `npm run dev 5190` | 启动桌面开发；默认端口 5174，可指定端口 |
| `npm run dev:ui` | 仅管理面板，无 Tauri 桥 |
| `npm run build:ui` | 构建面板到 `ui/dist/` |
| `npm run build` | 本机构建安装包 |
| `cargo check` / `cargo fmt` / `cargo clippy --all-targets` | 编译、格式、lint |
| `cargo test --locked <筛选>` | 相关 Rust 单测，使用 debug profile |
| `cargo check --release --all-targets` | release 条件编译检查 |
| `npm run check` | 发布前全量门禁 |

日常只跑命中的测试、`check:invariants` / `check:duplicates`、`cargo check` 或 `build:ui`；相同代码状态通过后不重复跑。**全量 `npm run check` 只在版本发布前整跑一次，发布 commit 必须全绿**。Rust 用例不改用 `--release`；release 编译检查和 Clippy 零警告基线保留。验证方法和故障处理见[开发验证](docs/operations/development.md)。

## 数据目录

- 路径统一经 `paths::*`、`kernel::data_dir` 解析，数据始终随 `DSH_XLINK_HOME`，不加平台 app-data 兜底，不在前端写死路径。
- dev / release 的内核安装树、设置、日志、注册表、默认实例和插件中央库分开；技能中央库与活动视图有意共享，接线按实例写。不得合并两壳可变状态。
- 生产调用方用 `instance::resolve_default()` / `plugins::default_instance_key()`，不硬编码默认实例 id。注册表 `default_instance_id` 仅由认领逻辑维护，不得由其他模块写具体 id 或读取作决策。
- 插件中央库搬迁不覆盖已有目标，失败记录原因；历史 `~/.dsh` 只搬入 release。恢复会话须用户确认、目标实例内核停止、只复制缺失项且不删源。具体布局与迁移边界见[磁盘布局](docs/architecture/kernel-data-layout.md#8-路径解析与隔离契约)。
- 数据迁移向导（含「找回历史会话」）是过渡功能，**v0.6.0 之后整体移除**：前端、Rust 模块、入口与命令/判据四块一起删，只删侧栏菜单不算数；执行清单见 [ui/AGENTS.md](ui/AGENTS.md#数据迁移v060-之后整体移除)，两处名字带迁移但不要跟着删的 `legacy_migration_target` / `store_relocate` 见[迁移向导设计](docs/features/migration/migration-wizard-ui-proposal.md#维护与移除边界)。

## 跨前后端契约

- 托盘退出先完成带 `request_id` 的认领应答，再显示确认；停止内核失败不得退出。关闭、收起、恢复复用 `shell/resident.rs`；详细事件与兜底见[窗口契约](docs/architecture/window-architecture.md#生命周期与退出契约)。
- 本机 Codex 登录和外部工具用量均须显式许可、只读；新增 Codex 登录消费方复用 `subscription_codex::chatgpt_web_available()`。外部用量保持独立统计口径、后端偏好单一真值、失败后重扫和缓存增量语义，见[用量契约](docs/features/subscription/local-usage-sources.md)。
- 官网认证弹窗留在壳内，继承发起方浏览器档案；网页登录不借本机 Codex 凭据替代，见[登录契约](docs/features/official-chat/official-chat-agent-guide.md#34-认证弹窗与浏览器档案)。

## 窗口外观作用域

原生外观只作用于当前窗口，由 `shell/appearance.rs` 管理；不得用应用级主题模拟单窗外观。工作台与授权弹窗不钉建窗主题；工作台镜像内核主题偏好，`system` 解除覆盖。两条建窗链同步接线，细节见[窗口主题](docs/architecture/window-architecture.md#主题按窗口内容选择作用域)。

## 发布

- 开发完成、定向验证通过后默认本地提交；用户未要求发布或推送时不推送。
- 只暂存本次文件，禁止 `git add -A` / `git add .` / `git commit -a`；提交前检查 `git diff --cached --stat`，不要卷入其他会话改动。
- 明确要求发布即已授权版本更新、门禁、提交、推送与 tag，无需逐步再问。推送前检查并报告 `git log --oneline origin/main..HEAD` 中的提交、作者、内容及尚未实机验证的风险。
- 发布同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 三处版本。只发布 Intel macOS 与 Windows，不添加 Linux / Ubuntu runner、制品或文案。
- tag、重试、资产和发布后验证按[发布手册](docs/operations/release.md)执行；查看流水线用 `gh run watch`，不自写轮询。

## 文档维护

- 修改用户可见行为、目录、发布流程或安全策略时，同步 `README.md` 与对应 `docs/`。
- `AGENTS.md` 只收可执行的开发约束和阅读入口；功能协议归 `docs/features/`，架构与边界归 `docs/architecture/`，操作步骤归 `docs/operations/`，事故与历史证据归 `docs/reviews/` 或 `docs/incidents/`。新增专题在 [docs/README.md](docs/README.md) 索引，不复制整段规则到多处。
- 指令文件保持在 `check:invariants` 的 64000 字节预算内；内核升级时核对运行时 `agent-instructions.maxBytes`，不能照抄旧上限。
- 文件用 UTF-8，恰好一个末尾换行；不提交依赖目录与构建产物。
