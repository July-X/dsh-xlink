# 开发验证

本文记录测试、探针和门禁的执行方法。开发约束入口见[根 AGENTS.md](../../AGENTS.md)，发布流程见[发布手册](release.md)。

## 按改动选择检查

日常开发和提交只执行命中的检查；同一代码状态通过后不重复运行。UI 改动按 `ui/AGENTS.md` 跑相关测试和 `npm run build:ui`；Rust 改动跑相关用例，按需执行编译检查、格式与 Clippy；规则或共享层变化补对应的 `check:invariants` / `check:duplicates`。

`npm run check` 只在版本发布前完整跑一次，发布 commit 必须全绿。判据组合以 `package.json` 为准，不手工拼一份替代门禁：

- `check:js` 覆盖静态规则、协议夹具构建、脚本 / UI / smoke / 插件 / 探针测试、UI 构建与产物预算。
- `check:rust:ci` 覆盖格式、debug 单测和 Clippy 零警告。
- `check:rust` 在 CI 共用检查上补 `cargo check --release --all-targets`。

CI 负责平台兜底；本机通过只证明当前平台与配置。debug 和 release 的条件编译覆盖面不同，release 编译检查不可移除。

### Rust 测试 profile

在 `src-tauri/` 使用 `cargo test --locked <筛选>`，不用 `--release` 跑用例。`ShellMode::current()` 随 `debug_assertions` 改变，依赖 dev 默认实例 `default-dev` 的用例在 release 下会出现语义不符的假红。release 编译使用 `cargo check --release --all-targets` 单独验证。

Windows debug 用例若加载即失败（`0xc0000139`），先清理相关编译缓存重试一次，不用 release 测试替代。背景见[复盘 §26](../reviews/agent-lessons.md)。

## 数据与浏览器档案隔离

凡经 `paths::*` 解析路径的测试，读写都持有 `crate::tests::scoped_xlink_home(&临时目录)`（或模块自带的 `TestHome` / `TempXlink`）直到用例结束。`HOME` / `DSH_HOME` / `DSH_XLINK_HOME` 只通过 `scoped_xlink_home` / `scoped_dsh_home` 修改，禁止裸 `set_var` / `remove_var`。守卫持进程级锁并恢复环境；裸修改会污染并发用例。只清理本用例创建的临时目录，不递归删除解析出的家目录。事故见[复盘 §24](../reviews/agent-lessons.md)。

驱动桌面壳实跑时同时隔离三项：

| 配置 | 作用 |
| --- | --- |
| 临时 `DSH_XLINK_HOME` | 隔离设置、实例、内核树、插件物化 |
| 编译期 `TAURI_CONFIG` 的 `app.appDirectoriesOverride` | 覆盖 `app_local_data_dir`，隔离壳的浏览器档案 |
| 独立 `DSH_XLINK_CHANNEL_SUFFIX` | 隔离单实例通道，防止交接给已运行的安装版后退出 |

Windows 的 `app_local_data_dir` 走 `SHGetKnownFolderPath`，改 `LOCALAPPDATA` 无效；配置须在 `cargo build` 时注入。即使只是打开页面，浏览器也会写 Cookies / Preferences，不能把真实档案交给探针。

`probe-profile-isolation` / `probe-data-directory` 只扫描测试与探针（`src-tauri/examples`、`src-tauri/tests`、`e2e/`、`plugins/*/test/`）。产品自身窗口使用产品默认档案是正常行为，不要扩大扫描到生产建窗代码；壳内复验的数据安全依赖上述隔离配置，不能只看静态判据是否通过。

## 产物与证据

- 每轮独立命名，例如 `results.r14.json`、`model-requests.r14.jsonl`；证据落在待清理临时 home 之外，避免覆盖或随清理丢失。
- 改内置插件后先 `npm run prep:builtin`，再重新构建、重启壳；核对源码、内嵌资源和物化文件摘要。`plugins/builtin/materialize.rs` 只在版本、内核指纹、`sourceHash`、`outputHash` 全相符时跳过复制，不能只比较版本号。
- 确需清旧物化时，仅处理已确认的隔离实例插件目录，不把清理用户生产目录当固定前置。
- 调查上游行为优先读实际源码和真实日志，只读取证；不要从接口名或通用文档推定当前链路行为。前缀缓存诊断见[本机用量来源与诊断](../features/subscription/local-usage-sources.md#前缀缓存诊断)。

## 门禁与反向验证

判据本体放 `scripts/`，CI 和本地复用同一脚本。前置产物不存在时必须 exit 1 并提示生成命令，不能跳过后报通过。

新增 `bundle.resources` 目录时确认全新 checkout 能生成它；所有会运行 Cargo 的 workflow job 必须先执行对应资源生成步骤。内置插件资源用 `npm run prep:builtin`，接线由 `scripts/dev-builtin.test.mjs` 验证。不要只在本机已有产物时验收。

源码扫描先在原文定位锚点，再对匹配区间应用 `productionRust()` 排除注释；锚点未找到必须失败。Tauri 入口是 `.setup(|app| {…})`，不能用不存在的 `fn setup(` 切区间。反向验证前先确认原始判据通过；再确认变异确实进入被测产物，失败来自目标断言，不能把编译失败当作判据命中。

恢复文件后确认重新构建。PowerShell 恢复若保留旧时间戳，可执行 `(Get-Item <file>).LastWriteTime = Get-Date` 后再跑，避免 Cargo / Vite 复用旧产物。详见[复盘 §3、§9、§11、§17–19、§29](../reviews/agent-lessons.md)。

指令文件的仓库预算为 64000 字节，由 `agents-md-byte-budget` 检查根、UI 与 Rust 三份 `AGENTS.md`。2026-10-26 整理时文档记录的运行时上限为 65536，配置位置是已装内核 `@deepseek-ai/dsh-base/cordis.patch.yml` 的 `agent-instructions.maxBytes`；升版时重新读取，不沿用历史数字。超预算优先迁出背景和专题细节。
