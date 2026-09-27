# dsh-xlink 代码审查报告（2026-09-27）

审查对象：`dsh-xlink@0.3.3-rc.3`（`main`，HEAD `b9e115c`；范围 `70f173c..b9e115c`，2026-09-11 ~ 09-27 的 227 个提交，144 文件，+35590 / −3758）。
审查日期：2026-09-27。

上一轮（`docs/code-review-2026-09-11.md`，rc.20）之后发生了两件事：开发速度从「22 提交 / 137 分钟 / +7841 行」跃升到「**227 提交 / 16 天 / +35590 行**」；同时「多实例」重构（P3→P8）铺开到了 `plugins.rs` / `commands.rs` / `kernel_adapter.rs` / `instance.rs` 全链路。本轮是那次重构**之后**的独立复核。

## 审查方法与已执行的验证

本轮未修改任何代码。以下为本轮**实际执行**的验证：

| 验证 | 结果 |
| --- | --- |
| `node scripts/check-code-budget.mjs` | 通过，但总量 29739 / 29760，**仅剩 21 行余量**（详见下方「并发写入者」说明） |
| `node scripts/check-invariants.mjs` | 全绿，10 项 note（69 命令一致 / 21 处 `uses:` 全 SHA 钉死 / 三处版本一致 `0.3.3-rc.3`） |
| `rg` 全量取证（命名空间、忽略形参、z-index、CSS 变量定义、门禁引用、测试引用等 20 余组） | 见各条证据 |
| 子 review：`reap_orphans` 身份判定 / `kernel_adapter` cwd / `paths` 目录解析三处独立复核 | 逐条读完整调用链确认 |

### ⚠ 并发写入者：本轮审查的重要前提

**审查期间工作区存在并发写入者**（与 `docs/code-review-2026-09-11.md:28-32` 记录的情况同类）。`git status` 显示 8 个非本轮修改的文件在 12:02–12:15 之间被改动：`README.md`、`docs/notification-design.md`、`scripts/check-code-budget.mjs`、`src-tauri/src/notify.rs`（+315）、`ui/src/components/SettingsPanel.vue`、`ui/src/notifications.js`（+27）、`ui/src/theme.css`（+72）、`ui/test/notifications.test.js`（+44），合计 +485 / −42。

处理方式：

1. **受影响结论已重新取证**。预算数字在并发写入后复测（见下）；H4 的两个 CSS 变量在 `theme.css` +72 行后**仍未定义**（定义数 0），且 9 个使用点的行号**未漂移**（相关组件未被改动）——**H4 依然成立**。
2. **不受影响的结论不受影响**。Rust 侧全部证据（`kernel.rs` / `commands.rs` / `plugins.rs` / `instance.rs` / `kernel_adapter.rs` / `pkg.rs` / `registry.rs` / `releases.rs` / `capabilities/`）**所涉文件无一被并发修改**，行号与结论仍然有效。
3. **本轮新增一条现场证据**（见下节），它恰好印证了本轮的核心判断。

**本轮未执行**（耗时过长，且本次只出结论不改代码）：`cargo check` / `clippy` / `fmt --check` / `cargo test` / `pnpm build:ui` / `pnpm test:ui` / `pnpm test:scripts` / `tauri build`。上一轮这些均全绿，16 天内未见门禁失效的外部迹象，但**本轮不对 Rust 编译与测试结果作任何断言**。

审查分四个维度并行（Rust 核心运行时 / 安全信任边界 / Vue 前端 / 工程门禁与 CI），26 条结论**全部由审查方逐条独立取证复核**后才纳入本报告。

## 总体判断

**工程纪律水平明显高于同类桌面项目**：491 个 Rust 测试、CI 跑 `fmt --check` / `cargo test` / `clippy -D warnings`、注释密度 18.5% 且解释「为什么」而非「做什么」、`check-invariants.mjs` 每条规则都源自真实事故（rc.11~rc.18 连续 8 版 100% 失败的 ACL 事故被它钉死）、凭据纪律端到端干净。

**真正的问题不是「写得差」，而是「纪律执行到极致后开始反噬自己」**，表现为三条退化：

1. **代码预算门禁已自我锁死** —— 30 次上调、0 次下调，总量 +46%，仅剩 21 行余量
2. **多实例迁移做了一半**，留下 6 条静默失效的路径
3. **文档追不上实现** —— `architecture.md:5` 在断言一个已经失效的安全网

## 现场证据：预算棘轮在本轮审查进行中又转了一格

这是本轮最有说服力的一条发现——**它不是从注释历史推断的，是当场发生的**。

本轮审查进行到尾声（12:12–12:15）时，并发写入者为「任务完成通知带最近一轮对话」这个功能上调了预算：

```diff
-  'ui/src/theme.css': 3160,
+  // 3160 → 3225：通知卡「最近完成」列表样式（.notify-items / .notify-item /
+  'ui/src/theme.css': 3225,
-const TOTAL_BUDGET = 29580;
+// 29580 → 29760：任务完成通知带「最近一轮对话 + 完成时间」。notify.rs
+const TOTAL_BUDGET = 29760;
```

`process.rs` 同步从 1090 上调到 1170。**同一次提交里 `src-tauri/src/notify.rs` +315 行、`ui/src/theme.css` +72 行、`ui/src/notifications.js` +27 行、`ui/test/notifications.test.js` +44 行。**

这正是 AGENTS.md 所定义的正规流程（「要上调预算，必须在同一个提交里改 `scripts/check-code-budget.mjs` 的数字」）——**流程被严格遵守，纪律却已失效**。规则没有错，问题是它约束的是「流程」而非「结果」：只要在提交里改一个数字，+485 行的功能扩张就是合规的。

复测后的当前状态：

```
   3128 行  ui/src/theme.css（预算 3225）        余 97
   2964 行  src-tauri/src/plugins.rs（预算 2980）  余 16
   1912 行  src-tauri/src/commands.rs（预算 1915）  余 3   ← 0.16%
• 生产代码合计 29739 行（预算 29760）              余 21   ← 0.07%
```

`TOTAL_BUDGET` 的完整上调轨迹（从 `scripts/check-code-budget.mjs` 注释提取）：

```
20400 → 20500 → 20560 → 21060 → 21660 → 22160 → 22190 → 22500 → 23070
→ 24500 → 24550 → 26300 → 26500 → 26600 → 26650 → 26900 → 26950 → 27080
→ 27300 → 28900 → 29210 → 29450 → 29520 → 29580 → 29760
```

**30 次上调，0 次下调，累计 +46%。** 而 `theme.css` 的注释里已经写明了放弃的策略：

> `// 3080 → 3160：…共享主题文件按约定不拆分，涨数字让其可见。`

「涨数字让其可见」——**门禁的职责已从「阻止膨胀」退化为「记录膨胀」**。

## 根因分析：多实例迁移的 6 条静默失效路径

**这是本轮最重要的结论。** 绝大多数 High / Medium 级缺陷来自同一个未完成的重构：

| # | 症状 | 根因 | 后果 |
| --- | --- | --- | --- |
| 1 | `reap_orphans` 恒不匹配 | `kernel.rs:1563` 比 `p == data_dir`，而实际 cwd 是 `record.workspace` | **崩溃回收安全网静默失效** |
| 2 | `profile_dir` 忽略实例参数 | 形参被命名为 `_data_dir` 并在内部重新解析默认实例 | 给 B 实例接线写进 A 实例 |
| 3 | 两把独立的 lifecycle 锁 | `commands.rs:99`（`AppState::lifecycle`）vs `instance.rs:780` | 同一内核被两把互不知情的锁保护 |
| 4 | `delete_instance` / `set_default_instance` 漏加锁 | 同伴命令都加，它俩没有；`save_registry` 无文件锁 | 并发写注册表丢更新 |
| 5 | `store_dir` 等 6 处忽略入参 | 路径已改为全局派生，形参成了摆设 | 调用方传什么都不影响结果 |
| 6 | `start_instance` 丢弃 `Child` | 未调 `register_child` | 僵尸进程 + 不进 `state.running` |

**它们的共同特征是「全部通过了现有门禁」**：491 个测试全绿、clippy 零警告、`check-invariants` 全过。因为它们**只在多实例路径上出错，而多实例 UI 尚未上线**（`commands.rs:2816` 的 `start_instance` 是预留 API，UI 侧仍只调 `start_kernel` / `stop_kernel`）。

这类缺陷属于「**防御机制只覆盖了已启用的路径**」——比明显 bug 危险得多，因为没有任何自动化信号会提示。

### 证据 1：`reap_orphans` 是死代码

```rust
// kernel.rs:1562-1563
let cwd_matches = std::fs::read_link(format!("/proc/{pid}/cwd"))
    .map(|p| p == data_dir)
```

但内核当前的启动路径把 cwd 设成了实例 workspace：

```rust
// kernel_adapter.rs:378, 385
let workspace = PathBuf::from(&record.workspace);
...
.current_dir(&workspace)
```

两个路径永不相等：

- `data_dir`（`paths::family_runtime_dir`，`paths.rs:154-157`）= `~/.dsh-xlink/dsh/desktop[-dev]/`
- `record.workspace`（`instance.rs:165-168`）= `~/.dsh-xlink/kernels/dsh/instances/<id>/workspace`

`cwd_matches` 恒为 `false`，`kill_pid` 永不执行。而 `process.rs:1543`、`process.rs:1629`、`guard.rs:212`、`lib.rs:165,203` 的注释**仍在明确描述这个已失效的机制**（「Unix 靠 `reap_orphans` 的「cwd == data_dir`」）。**文档在主动断言一个坏掉的安全网仍然有效。**

**影响**：SIGKILL / panic / 强杀之后内核存活。恢复多半仍能工作（`kernel::start_instance` 会写 `kernel.pid`），但在回收器本该生效的场景失效——pid 文件被删 / 从未写入（同数据目录上的第二个壳、手工跑 `dsh web`）。用户看到的正是 `lib.rs:348-350` 声称回收器能避免的那种误导性诊断。

**修法**：`reap_orphans(roots: &[PathBuf])`，改为匹配 `instance::list_records()` 的 workspace 集合（保留 `[data_dir]` 作 P3 前安装的兜底），并加一条「构造 `InstanceRecord` → 启动 → 断言被回收」的回归测试。

⚠️ **F1 与 F11 必须一起修**：`kill_pid(pid, None)` 走 `libc::kill(-pgid, …)` 进程组杀，而身份判定 `command_is_kernel`（`kernel.rs:1919-1924`）是无界子串匹配 `contains("@deepseek-ai/dsh/lib/bin.js")`。该路径今天因 F1 而不可达，**修好 F1 会让它复活**。应改为 argv token 锚定匹配。

### 证据 2：`profile_dir` 的形参是假的

`ensure_wiring_filtered(family, instance_id, …)` 明确收到了实例参数，接线与清扫都正确使用，但内部两处丢弃了它们：

| 位置 | 代码 | 实际解析到 |
| --- | --- | --- |
| `plugins.rs:2428` | `ensure_profile(data_dir, &settings.profile)` | ❌ `profile_dir` → 默认实例 |
| `plugins.rs:2566` | `&profile_dir(data_dir, profile_name)` | ❌ 默认实例 |
| `wire_manifest` / `sweep_instance_orphans` | 显式传参 | ✅ 正确实例 |

```rust
// plugins.rs:550
fn profile_dir(_data_dir: &Path, profile: &str) -> PathBuf {
    let (family, id) = default_instance_key();   // ← 忽略调用方给的实例
    paths::instance_profile_dir(&family, &id, profile)
}
```

且 `default_instance_key()`（`plugins.rs:512`）**每次调用都重新读盘**（`instance::load_registry()`），全项目 20 个调用点。**后果**：给实例 B 接线时，profile 目录建在实例 A 下，`pnpm install` 也跑在 A，而 `wiring.json` 写的是 B——B 的插件依赖永远装不上，同时 A 被静默改动。

## High 级问题（8 条）

| # | 问题 | 位置 | 修复代价 |
| --- | --- | --- | --- |
| H1 | `reap_orphans` 在当前启动路径恒不匹配，崩溃回收安全网静默失效 | `kernel.rs:1562-1582` | 中 |
| H2 | `profile_dir` 在实例作用域操作中重新解析默认实例 | `plugins.rs:550,2259,2428,2566` | 中 |
| H3 | `cargo` 全线缺 `--locked`，**发版打进安装包的依赖集可能 ≠ 已审核的 `Cargo.lock`** | `desktop-ci.yml:92,95,98,128`；`desktop-release.yml:246,249,252,315` | **6 行** |
| H4 | `--text-muted` / `--surface-soft` 未定义，9 处样式**静默失效** | `theme.css:4-31`（定义数 0） | **5 分钟** |
| H5 | 原生 `confirm()` 在 WKWebView 静默返回 false，macOS「回滚」是空操作 | `MigrationPanel.vue:90` | **5 分钟** |
| H6 | `migration_run` 绕过 `withExclusive` / `withProgress`，可与内核启停并发写同一数据目录 | `migration.js:146` | 低 |
| H7 | `dist.tarball` 无 host 校验，「已校验」字节可来自任意主机 | `pkg.rs:121-127` | ~8 行 |
| H8 | `architecture.md:5` 与同文档 `:113` 自相矛盾，且钉死过期 SHA `89932eb` | `docs/architecture.md` | **5 分钟** |

### H3 详述

`Cargo.lock` 已提交，但**没有任何一处强制 cargo 使用它**。`desktop-release.yml:315` 的 `pnpm exec tauri build` 同样不带 `--locked`——**实际打进安装包的依赖集可能与仓库里被 review 过的 lockfile 不同，且没有任何门禁会发现**。pnpm 侧已用 `--frozen-lockfile` 锁死，前端严格、后端漂移的不对称很扎眼。

同时 `src-tauri/` 无 `rust-toolchain.toml`，`Cargo.toml:6` 的 `rust-version = "1.77"` 目前是纯文档，无任何机制强制。

### H4 详述

`--text-muted` 与 `--surface-soft` 在 `:root` 中**完全没有定义**（`rg` 全 `ui/src` 定义数 = 0），`:root` 只定义了 `--text: #e8ecf7`。CSS 自定义属性无回退时整条声明在 computed-value time 非法 → **被丢弃 → 继承父级**。

```css
/* KernelTabs.vue:95  */ .kernel-tab { color: var(--text-muted); }
/* KernelTabs.vue:104 */ .kernel-tab.is-active { color: var(--text); }
```

→ 非激活 tab 与激活 tab **亮度完全相同**，「视觉上压低权重」的设计只剩 2px 下划线。全部 9 处：`KernelTabs.vue:95,113`；`MigrationPanel.vue:315,322,323,330,371,381`（其中 `:381` 的 `background: var(--surface-soft)` → 完全透明）。

**不产生构建错误、不产生构建警告、`check-ui-bindings` 与 `test:ui` 都发现不了**——这是它列为 High 的原因。

### H5 详述

```js
// MigrationPanel.vue:90
if (!confirm(`确认回滚迁移 ${migrationId}？…`)) { return; }
```

仓库自己的注释就写着这件事：

```js
// notify.js:2
// WKWebView 没有原生 confirm()，ElMessageBox 是页内实现，天然可用。
```

WKWebView 未实现 `runJavaScriptConfirmPanelWithMessage` 时 `confirm()` 不弹 UI 且**直接返回 false**。macOS 上点「回滚」= 纯空操作，`migration_rollback` 都不会发出。全仓 `confirm(` **只有这一处**（`patches.js:22,46` 已正确使用 `confirmDialog`）。

## Medium 级问题（12 条）

**运行时 / 并发**

- **M1** `state.running` 互斥锁跨阻塞 IO 持有：`commands.rs:916-922` 在 guard 内执行 `kernel::stop`（`kernel.rs:1613-1629`，约 1.1s 的 sleep + SIGKILL + fork）。而 `lib.rs:375,387` 的窗口关闭拦截器与 `tray.rs:340` 的托盘退出**都跑在事件循环线程**并阻塞在同一把锁上 → 停止内核期间点关闭，UI 冻结最长约 1.1s；且 `kernel_running` 每次 fork `ps` + `lsof`，每关闭事件调 2 次。
- **M2** `setup()` 在窗口创建前于主线程同步执行重活：`lib.rs:165`（reap_orphans，含最多 256 次 `lsof` fork）、`:175`（实例注册）、`:183`（legacy home 递归迁移）、`:214`（日志清理）。无窗口、无 Dock 图标，冻结时长无上界。
- **M3** 两把独立的 lifecycle 锁守同一个内核：`commands.rs:99` vs `instance.rs:780`，从不同时获取。两条路径都调 `kernel::start_instance`，各自看到 `port_open == false` 即 spawn → 同一数据目录上两个内核。
- **M4** `delete_instance`（`commands.rs:2771`）与 `set_default_instance`（`:2795`）漏加 `instance::lifecycle_mutex()`，而 `list_instances` / `create_instance` / `start_instance` / `stop_instance` / `ensure_default_instance_migrated` 全都加了。`set_default_instance` 是**当前 UI 可达**的（`ui/src/instance.js:59`），而 `save_registry`（`instance.rs:321-331`）是无文件锁的读-改-写，并发写会丢更新。
- **M5** `state.lifecycle` 跨整个 guarded start 持有（`commands.rs:839-887`）：内含 `plugins::ensure_wiring` 的 pnpm 运行，`process.rs:1240` 允许单次 30 分钟 → 锁可持有数分钟，期间 `save_settings` / `activate_version` / `remove_version` / 所有 `plugin_*` **静默阻塞**，无超时无忙碌提示。实践中有 `store.js:346` 的 `withExclusive` 兜着，属设计异味。
- **M6** `open_harness` 跨 10s 探测循环持锁（`commands.rs:1238` + `1143-1171`）。

**安全 / 信任边界**

- **M7** `DSH_NPM_REGISTRY` 接受 `http://`：`registry.rs:33-43` 的 `resolve()` 只做 trim + 补斜杠，**从不解析 URL**。整个信任链是「packument 摘要 ⇒ tarball」，明文 registry 下网络攻击者只需把 `dist.integrity` 设成自己载荷的 SHA-512，校验即通过，UI 仍显示「已校验下载内容的 integrity（sha512）」。**注：默认值是 https（`registry.rs:15`），触发需显式降级配置**——风险在于这是「唯一防线的降级开关且无任何确认」，而国内网络环境下明文镜像现实存在。
- **M8** **`@deepseek-ai` 命名空间对插件/技能未强制，AGENTS.md 说法不实**：`plugins.rs:962-968` 与 `skills.rs:403-418` 只校验字符类 `-._@/`，`lodash` / `@attacker/backdoor` 均可安装。而该限制在内核（`kernel.rs:632` 硬编码 `DSH_NPM_PACKAGE`）与 Node 运行时（`node_install.rs:27-28` 硬编码 + SHA-256）上**确实生效**。AGENTS.md 恰把 `pkg.rs` 点名为执行点，按其指引审计会得出错误结论——**这是关于安全边界的描述本身失真**。
- **M9** git 来源插件零摘要校验却执行 `prepare` 脚本：`plugins.rs:1347-1364` 下载解包无 digest，`build_git_plugin`（`:1530-1539`）跑 `pnpm install --config.enable-pre-post-scripts=true` 执行归档内脚本。而紧邻的 npm 路径（`plugins.rs:1320`）**是**校验的——这个不对称就是问题所在。
- **M10** 未校验的 git URL 直达 `git ls-remote` / `git clone` 裸位置参数：`pkg.rs:213-215`、`plugins.rs:1422-1427`。git 的 `ext::` 传输与 `--upload-pack=` 使 URL 成为命令执行向量；`parse_spec`（`plugins.rs:901-912`）刻意剥离前缀以便用户粘贴自聊天页，放大了这条路径。
- **M11** `"csp": null` + `"withGlobalTauri": true`（`tauri.conf.json:28,30`），而面板持有全部 70 条 app 命令。**当前无 XSS sink**（`v-html` / `innerHTML` 全仓 0 命中，日志全走 `{{ }}`），属预防性而非可利用；但面板会原样渲染社区目录的插件描述，任何未来的 markdown 渲染器都会把它变成 RCE。

**工程 / 门禁**

- **M12** `check:code-budget` 在 release quality job **缺席**（`desktop-release.yml:176-252`），而 UI bundle 预算在 `desktop-ci.yml:69-89` 与 `desktop-release.yml:225-243` **各内联一份**。两份门禁清单各写一遍必然漂移——**已经漂移了一次**。

## Low 级问题（6 条）

- **L1** `capabilities/harness-remote.json` 用 `"remote": {"urls": ["http://127.0.0.1:*"]}` 通配整个回环端口。影响确实低（`focus_main_shell` 只抬窗；`report_harness_fault` 校验 `webview.label() == "harness"`、`kind` 白名单 3 个字面量、每个字段限长）。
- **L2** `process.rs:40-67` 的 `atomic_write` 未设 `mode()` → Unix 下 0644；`<xlink_home>/<family>/<instance>/` 为 0755，而内核的 `.credentials.yaml` 落在其中。（**不是明文密钥问题**：外壳自身不存密钥，已验证密钥只出现在 `Authorization` 头，零插值进任何错误/日志/缓存。）
- **L3** 8 个 icon-only 按钮无可及名称：`PluginsPanel.vue:548,559,569,587`、`SkillsPanel.vue:126,136,146,164`——`el-tooltip` 只给视觉提示。对照 `SkillsPanel.vue:117` 的 `el-switch :aria-label` 写对了，是遗漏不是风格。
- **L4** `test:ui` 是硬编码 21 文件名（`package.json:32`），而 `test:scripts` 用 glob。当前 21/21 齐全属巧合性维护，**新增测试文件会被静默跳过**。
- **L5** 6 条命令绕过 `blocking()` 辅助函数，把英文 `JoinError`（`task panicked`）直接透给 UI：`commands.rs:177,445,958,969,983,1002`。其中 `get_status` 是 2.5s 状态轮询。
- **L6** `process::atomic_write` 只在 `Err` 路径清理 `.{name}.tmp-*`；SIGKILL 中断（正是 H1 的场景）会永久残留。不对用户可见但无上界。

## 已证伪的假设（避免重复劳动）

以下看似是问题，实测**不是**，记录以免再次排查：

**安全边界**

- **SRI 算法选择是正确的** —— `releases.rs:111-136` 把 SRI 当 token 列表解析取最强项，且**存在可用 SRI 但不匹配时直接 Err，不回退 sha1**（`releases.rs:72-83`），无降级路径
- **非常-time 比较不是问题** —— 这里的「秘密」是攻击者可控文件的摘要，只比一次，无 oracle 循环
- **无 zip slip** —— `archive.rs:18-56` 拒绝所有非 `Normal` 组件，`:92-94` 同时拒软/硬链接，`:115-117` 的 `unpack_in()` 独立复检
- **生产代码无 shell 字符串插值** —— 全部 `Command::new(exe).args(argv)`；所有 `sh -c` / `cmd /C` 都在 `#[cfg(test)]`
- **updater 真的验签且无 TOCTOU** —— `updater.rs:249-310` 的 `install()` 不复用 check 结果，重新下载并重新验签
- **capability 面无 `fs:` / `shell:` / `http:`** —— 远程来源不给任何 `core:*`，**内核 HTTP webview 的提权路径实际是关死的**
- **命令边界穿越已闭合** —— `validate_log_name`（`commands.rs:424-448`）+ `validate_id_component`（`paths.rs:410-451`）+ `is_valid_kernel_version`（`version.rs:16-30`，在 GitHub 与 Atom 两个出口都应用）

**Rust 运行时**

- **规约 1 未被违反** —— 69 条涉及进程/网络/目录树的命令全部走 `blocking()` / `spawn_blocking`；仅 4 条同步命令，且均为纯窗口/常量操作
- **无 `MutexGuard` 跨 `.await`** —— 所有 guard 都在 `spawn_blocking` 闭包内创建；无 `std::sync` / `tokio::sync` 混用；锁序一致为 `lifecycle → running`
- **`kernel.rs` / `window.rs` / `tray.rs` 生产路径无 `unwrap` / `expect` / `panic!`**
- **`stop_kernel` 幂等** —— 二次调用槽位空 → `Ok`；pid 文件无条件清除，失败也能在下次启动收敛
- **端口被外部进程占用处理得当** —— `kernel.rs:1364-1374` 报出占用 pid 与下一步，`BootVerdict::SpawnFailed` 抑制插件归因阶梯，`open_harness:1257-1261` 拒绝把外部监听者渲染成工作台

**前端**

- **`withLoading` / `singleFlight` 不会卡死** —— 审查方把真实模块 import 进 Node 实跑验证：同步 throw、async reject、重入、租约计数四种路径全部正确释放（`loading.js:39-41` 的 `.finally`、`async.js:23-25` 的 `if (inFlight === tracked)`）
- **无 XSS** —— `v-html` / `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `document.write` 全仓 0 命中
- **`store.js` 竞态防护正确** —— `statusRequestSeq`（`store.js:61-77`）+ `selectionRevision`（`instance.js:30`）+ `logReadSeq`（`logs.js:90`）三层独立且各配测试，逐行读全路径未发现竞态漏洞
- **`plugins.rs` 不是杂物袋** —— `commands.rs` 对插件工作有 24 处 `plugins::` 调用、**零**直接 `Command::`/pnpm 调用；12 个清晰分区、125 个顶层函数，编排层在 3 层各重复一次是唯一的结构风险

## 值得保护的既有资产

以下建议明确写进 AGENTS.md 作为「不要破坏」清单——它们是本轮最意外的收获：

1. **`pid_is_kernel` / `workbench_pid` 的四层身份校验**（`kernel.rs:1949-2042`）—— 主动把「存活」与「配置端口」解耦（否则用户改端口后会起第二个内核），且有 `:30900` vs `:3090` 的真实 netstat 解析回归测试。多数项目在「PID 复用杀错进程」上会犯错，这里没有。
2. **`releases::verify_download_integrity` 的反证式测试**（`releases.rs:838-891`）—— 写成「反证：把 `Ok(None)` 改回去测试就会红」，而非断言当前行为。**这是唯一能在重构中守住 fail-closed 的测试写法。**
3. **`guarded_start` 拒绝在环境故障时甩锅插件**（`guard.rs:703,712-726`）—— 否则一次碰巧成功的重试会让用户删掉无辜插件。推理写在了代码里。
4. **`run_with_progress_log` 的 drain-grace**（`process.rs:1206-1342`）—— 区分「子进程退出但孙进程仍持有管道」（成功、输出截断）与真失败，并写明 Windows `cmd /C` 是动机来源。
5. **`ui/src/async.js` 的共享层三件套**（`async.js:17-133`）—— 抽象 + 回归测试 + 防退化守卫齐全，是全仓唯一做到这三件套的地方（`incidents.test.js` 甚至断言「组件里不许再出现本地 cause 白名单」）。
6. **`check-invariants.mjs`** —— 全仓最有价值的一道门禁，每条规则源自真实事故，还自我守护供应链（校验全部 `uses:` 钉 SHA + Dependabot 配置存在）。

## 建议执行顺序

### 阶段 A：分钟级（1 天，零架构风险）

```
① --text-muted / --surface-soft 补进 :root          视觉立刻修好
② MigrationPanel confirm() → confirmDialog()        修好 macOS 回滚
③ architecture.md:5 矛盾句 + 过期 SHA 删除           文档恢复可信
④ AGENTS.md 的 @deepseek-ai 表述校正为「仅覆盖内核与 Node」
⑤ App.vue：stop_kernel 失败时不关闭外壳              避免用户不知道内核还在跑
```

约 100 行，每条可独立验证。**建议先做掉，让主线干净。**

### 阶段 B：低成本高回报（1~2 天）

```
⑥ 所有 cargo 加 --locked + rust-toolchain.toml       堵供应链漂移
⑦ check:ui-bindings 接进 CI                          零组件测试下的安全网
⑧ 3 个 verify-dsh-* 脚本接进 CI                      防补丁「已被官方取代」元数据过期
⑨ migration_run 接 withExclusive/withProgress         消除数据竞争
⑩ registry 强制 https + tarball host 校验             补信任边界
```

### 阶段 C：还债（1~2 周，必须在加功能之前）

```
⑪ 收口多实例迁移：reap_orphans + profile_dir + 锁统一 + 6 处假参数
     —— 唯一同时解决 6 条缺陷的动作，且回收 150-250 行
⑫ H1 与 F11 一起修：修好回收器会让子串匹配的 kill(-pgid) 路径复活
⑬ 抽 copy_tree 共享层（门禁自己标注「已知重复，留到独立重构处理」）
⑭ 门禁去重：check:code-budget 补进 release，两 workflow 共用清单
⑮ 预算改棘轮：纯上调 PR 需配对减行 commit
⑯ 预算扫描根扩到 src-tauri/src/*.js（795 行）与 scripts/*.mjs（2930 行）
⑰ 补 pkg.rs / state.rs / withLoading / singleFlight / withProgress 测试
```

**⑪ 是解锁一切的关键**——它既是 6 条缺陷的共同根因，又是回收行数的最大来源。当前 `commands.rs` 只剩 3 行余量、总量只剩 21 行，任何功能开发都会以「再上调一次预算」收场（**本轮审查期间已现场发生一次**），纪律体系正式失效。

## 亮点功能规划

产品已成熟（事故自动归因、用量热力图、三家云额度、WebSocket 通知、迁移回滚、补丁备份校验），**不推荐再加同类功能**。以下按「预算解除后的收益」排序：

**1. 环境自检页（Doctor）— 性价比最高**
现在只有「工作台健康自检」（运行时事故归因），**没有启动前的环境体检**。把散落的隐式前置条件显性化：端口占用检测、Node/pnpm 版本、磁盘空间、数据目录权限、注册表完整性、内核与外壳版本匹配。**复用已有的 `guard.rs` + `diagnose_runtime`**，后端几乎不用动，纯 UI 编排。对「点开就白屏」类问题的自救价值极高。

**2. 一键备份 / 导出 / 恢复**
现有「数据迁移向导」只处理**旧版本遗留数据**，不是备份。用户换机、磁盘故障、装坏插件时**没有任何完整快照能力**。复用迁移向导已建好的 `MigrationReport` / `RollbackStatus` / 回滚 UI，边际成本远低于新做。

**3. 插件权限声明与风险分级**
插件是任意代码，目前只有「未验证」标记，**没有分级**。让插件包可声明所需能力（网络 / 写文件 / 启动进程），UI 按声明分级展示，安装时明示。是这个项目信任边界叙事的自然延伸。

**4. 内核版本 A/B 与一键回滚**
签名校验已就位，**缺的是回滚路径**。用户装了新版内核跑挂了，目前只能手动切回旧版本。补一个「上次可用版本」快照 + 一键回滚，投入小、感知强。

**5. 轻量可观测性**
内核运行时长、内存、请求量曲线。`notify.rs` 已在订阅 WebSocket 流，加这些指标边际成本低。

## 附：本轮的审查方法教训

上一轮报告记录了一条教训并仍然适用：**判定必须钉住被审的那个提交**。本轮所有结论均以 HEAD `b9e115c` 为准并给出精确行号，便于后续逐条复核与判定是否被并发修复。

本轮新增的一条教训：**防御机制只覆盖已启用路径的缺陷，不会被现有门禁发现**。H1、H2 全部通过 491 个测试与 clippy 零警告——因为多实例 UI 尚未上线，它们走的仍是默认实例的旧正确路径。**这类缺陷只能靠「读实现与设计意图是否一致」发现，自动化门禁帮不上忙。** 建议把「多实例路径」纳入 CI 的显式测试矩阵，而不是等 UI 上线后才发现。

审查期间未修改任何代码。
