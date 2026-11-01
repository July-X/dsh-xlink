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

驱动桌面壳实跑时同时隔离四项：

| 配置 | 作用 |
| --- | --- |
| 临时 `DSH_XLINK_HOME` | 隔离设置、实例、内核树、插件物化 |
| 编译期 `TAURI_CONFIG` 的 `app.appDirectoriesOverride` | 覆盖 `app_local_data_dir`，隔离壳的浏览器档案 |
| 独立 `DSH_XLINK_CHANNEL_SUFFIX` | 隔离单实例通道，防止交接给已运行的安装版后退出 |
| 本轮独占内核端口 | 预写 `home/shell/dev/settings.json` 的 `port`（从 35000 起选空闲）；避免 3090/3091 上宿主内核被 `get_status` 误判成本进程的 |

Windows 的 `app_local_data_dir` 走 `SHGetKnownFolderPath`，改 `LOCALAPPDATA` 无效；配置须在 `cargo build` 时注入。即使只是打开页面，浏览器也会写 Cookies / Preferences，不能把真实档案交给探针。

`probe-profile-isolation` / `probe-data-directory` 只扫描测试与探针（`src-tauri/examples`、`src-tauri/tests`、`e2e/`、`plugins/*/test/`）。产品自身窗口使用产品默认档案是正常行为，不要扩大扫描到生产建窗代码；壳内复验的数据安全依赖上述隔离配置，不能只看静态判据是否通过。

### E2E 启动器（OPT-08）

`npm run e2e` 走 `e2e/launcher.mjs`，不要直接 `wdio run e2e/wdio.conf.mjs`——后者会因为缺少隔离变量直接报错，这是刻意的：裸跑会把真实家目录和浏览器档案交给测试壳。

launcher 每轮创建一个独占运行根（`$TMPDIR/dsh-xlink-e2e-<runId>/`）。运行前在 `home/shell/dev/settings.json` 写入本轮空闲端口，`shell.spec.mjs` 断言 `status.kernel.port` 等于该端口且 `running=false`。运行根包含：`home/`（`DSH_XLINK_HOME`）、`appdata/`（编译期 `app.appDirectoriesOverride` 的根，覆盖 config/data/localData，cache 与 log 落在其子目录）、`evidence/`（launcher / wdio 日志与 `result.json`）、`decoy-home/`（`HOME` / `USERPROFILE` / `DSH_HOME`，macOS 另设 `CFFIXED_USER_HOME`，内放哨兵文件），以及本轮独占的 `target/`。macOS 的 WKWebView 档案不认 `HOME` 与 `appDirectoriesOverride`，会落在 `decoy-home/Library/WebKit/<bundle>/WebsiteData`；Windows 的 WebView2 档案由 `appDirectoriesOverride` 覆盖。构建使用 `CARGO_TARGET_DIR=<run>/target` + 注入本轮 `TAURI_CONFIG`，不复用旧的 debug 二进制；Windows 解析 `dsh-xlink.exe`，macOS 解析 `dsh-xlink`，构建后的存在性检查与 wdio 启动共用同一路径。

启动前自检归属标记、四项隔离变量与路径都在本轮测试根内；启动后核对二进制摘要、测试 home、浏览器档案（`appdata` 或 macOS 的 `decoy-home/Library/WebKit` 至少一个有内容）以及 `DSH_HOME` / `HOME` decoy 里的哨兵文件未被改动。通过时只删除带本轮标记的目录；失败时保留整个运行根并打印路径（证据在 `home/` 之外），需要保留成功轮次可加 `--keep`。launcher 的纯函数与清理归属有 `node --test scripts/e2e-launcher.test.mjs` 判据。

### E2E 动画控制与发布矩阵

WebDriver 拉起的窗口在后台，不调度 `requestAnimationFrame`，`App.vue` 的 `<Transition mode="out-in">` leave 永不结束。E2E spec 在 `before` 里把 rAF 换成 `setTimeout`——**只作用于测试 webview**，产品代码与正式构建的动画路径一个字节都不改；普通动画另由 UI 判据与真前台窗口覆盖，不能拿 E2E 的挂载结果当作动画验证。关键面板（记忆、效果页签、用量分析）的用例必须先等标题与内容区真实挂载再断言，等不到就失败，不再 `skip`。

发布回归分两层记录，不能混写：

- **debug 集成层**：`npm run e2e`（launcher）带 WebDriver，覆盖隔离首次启动、面板切换、真实 IPC、关闭/托盘/退出与停止失败路径。
- **release 安装产物层**：不启用 WebDriver、不增加控制端口；在 Windows 与 Intel macOS 用同一候选 commit 的安装产物做人工门禁（隔离首次启动、面板切换与 IPC、关闭/托盘恢复/退出、停止失败提示、内置插件源码/内嵌资源/物化摘要一致）。CI runner 没有原生桌面会话时只跑自动可执行部分，人工门禁未做就记 `not_run`；fail / skip / not_run 都不计入完成，证据必须对应同一候选 commit 与产物。

### CSS 静态检查的语法边界与真实布局

`ui/test/lib/css-cascade.mjs` 是**静态检查器**，支持的语法边界写在文件头：选择器列表按顶层逗号逐分支匹配，重复声明取最后一条，`@media` / `@supports` / `@container` 默认不参与、必须由调用方用 `conditions` 显式声明，`html.dark` 默认按浅色、tokens 带 `dark` 或 `opts.theme='dark'` 才计入。`:is()` / `:not()` 内部简单选择器特异度、`!important`、简写属性展开仍未建模。**静态检查通过不等于 UI 渲染通过**：显示、裁切、滚动、主题必须走真实 WebView。

`e2e/specs/layout.spec.mjs` 在固定 1040×748 窗口里检查概览主面板的横向溢出、卡片边界与动作按钮可达，浅/深主题的 token 生效与宽度不变，记忆编辑弹窗页脚按钮可达，并用注入的超宽元素反向验证检测器会失败。失败信息带元素、期望值与实际值，不做整页像素快照。用量长列表、日志副窗与更多弹窗仍待补。

### 四组高频 IPC 契约

`contracts/ipc-contract.json` 是唯一来源，覆盖四组：状态查询（`get_status`）、内核启停（`start_kernel` / `stop_kernel`）、记忆列表（`memory_list`）、用量查询（`get_model_usage`）。每条命令记录 Rust DTO 来源、前端消费方、请求/响应字段、必需与可空字段、枚举、错误码与 Channel 终止语义。

- Rust：`src-tauri/src/shell/ipc_contract.rs`（仅测试构建）把真实 DTO 序列化后按契约校验字段名、必需字段、枚举与额外字段；`cargo test --locked --lib ipc_contract`。
- 前端：`ui/src/shell/ipc-schema.js` 是通用校验器，`ui/src/shell/ipc-contract.js` 的 `ipcCall` 校验请求与响应；已切换的四个调用点是 `store.js` 的 `get_status` / `start_kernel` / `stop_kernel`、`memory.js` 的 `memory_list`、`usage.js` 的两处 `get_model_usage`。**其余命令仍走裸 `bridge.invoke`**，本轮不做全量迁移。
- 失败分三类：`contract`（请求/响应不符，调用不会到达 Rust 或结果被拒收）、`transport`（桥未注入 / invoke 没到 Rust）、`business`（后端 `Err(String)`，保留原文）。错误码来自契约的 `errorCodes`。
- 反向验证：`ui/test/ipcContract.test.js` 人为改参数名、必需字段与枚举必须失败；`ui/test/ipcCall.test.js` / `ipcTransport.test.js` 覆盖正常、请求非法、业务失败、响应漂移与桥未注入。
- 真实行为：`e2e/specs/ipc.spec.mjs` 在隔离壳里逐组调用并复用同一份契约校验；空数据（0 / `[]`）是合法响应，空 home 的启动失败按业务错误验证，Channel 只发字符串且命令 settle 后不再有事件。
- 改 DTO：同步契约与 Rust 对照测试；不要在前端再手抄一份字段表，也不要把四组之外的命令顺手接进来。

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
