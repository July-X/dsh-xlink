# dsh-xlink 修复任务 Handoff

**交付对象：** Codex / Claude Code / OpenCode 等代码 Agent\
**仓库：** `July-X/dsh-xlink`\
**审查基线：** `main`，HEAD=`b368d25d0c9003284016584f10414c889d89be99`\
**发布参照：**
`desktop-v0.7.0`，commit=`eb38213ba0b2c3cd4ee1f26da1beb4295a20ad18`

## 0. 给 Agent 的执行指令

请在仓库中直接实施下列修复并补齐回归测试。先检查
`git status --short --branch`、`git rev-parse HEAD`，阅读相关目录的
`AGENTS.md`。如果 HEAD 已变化，重新核对代码和行号，不要机械套用本文件。

约束： - 不覆盖用户未提交的修改；禁止
`git reset --hard`、`git clean -fd`。 - 不得通过放宽 HTTPS、host
allowlist、摘要校验、进程身份校验或隔离目录来让测试通过。 -
修复必须进入生产调用路径，不能只改测试。 - 不发布、不推送、不改 GitHub
Secrets。 - 进程/安装/目录测试必须使用临时目录和合成进程，不能碰真实用户
home、真实内核或真实插件库。 -
没有实际运行的测试必须标注"未运行"；静态审查不能写成 E2E 通过。 -
每项独立修改和测试；先处理 P1，再处理 P2。完成后给出 diff
摘要、测试命令、结果和未验证事项。

建议顺序：安装事务与摘要 → 逐跳重定向 → 停止状态机 → 孤儿恢复 → E2E
隔离/清理 → IPC 参数 → 加载页竞争 → CI 测试稳定性 → 全量门禁。

## 1. 任务清单

  -----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
  ID                优先级            任务                                                                     主要文件
  ----------------- ----------------- ------------------------------------------------------------------------ ----------------------------------------------------------------------------------------
  FIX-01            P1                摘要不应受 `fetchedAt` 等动态元数据影响；预检物料与应用物料必须一致      `plugins/install_plan.rs`、`pkg/fetch.rs`、`plugins/precheck.rs`

  FIX-02            P1                摘要计算失败、摘要不匹配等所有安装失败必须完整回滚，不能留下已接线插件   `plugins/precheck.rs`、`plugins/center.rs`

  FIX-03            P1                重定向必须逐跳在发请求前校验，禁止目标不能收到请求                       `pkg/releases.rs`、`pkg/registry.rs`、`pkg/fetch.rs`

  FIX-04            P1                停止失败不得丢失 Child/PID/恢复状态；等待必须有界；重试可用              `commands.rs`、`kernel/lifecycle.rs`、`shell/process.rs`、`shell/instance.rs`

  FIX-05            P1                孤儿恢复区分 Gone/Ours/NotOurs/Unknown，查询失败不能当作已退出           `kernel/lifecycle.rs`、`kernel/win_probe.rs`、`shell/instance.rs`

  FIX-06            P2                E2E 不得继承 `DSH_DESKTOP_DATA_DIR` 等目录覆盖变量逃出隔离根             `e2e/launcher.mjs`、`kernel/lifecycle.rs`

  FIX-07            P2                E2E 默认成功清理后不得再写已删除日志；无 `--keep` 必须退出 0             `e2e/launcher.mjs`、`scripts/e2e-launcher.test.mjs`

  FIX-08            P2                IPC 强制刷新字段名与 Tauri wire name 统一                                `contracts/ipc-contract.json`、`ui/src/usage/usage.js`、`src-tauri/src/usage/local.rs`

  FIX-09            P2                （作废：后台预启动已于 2026-11-02 移除）                              —

  FIX-10            P2                Defender 测试不能依赖 CI runner 的真实安全软件状态                       `shell/av_trust.rs`

  FIX-11            P2                Channel 结束测试必须观察浏览器内实时计数，而非 Node 端序列化快照         `e2e/specs/ipc.spec.mjs`
  -----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------

## 2. 具体修复要求与验收标准

### FIX-01：安装摘要稳定性

现状：`tree_digest()` 排除 `.git`、`.dsh-id`，但包含
`.dsh-source.json`；其中 `fetchedAt`
在预检与应用重新取源时变化，导致相同插件内容也可能摘要不一致并回滚。

执行： 1. 将产物内容摘要与来源/下载元数据分离。 2.
动态时间戳不得参与内容摘要；但不能因此丢失来源身份、commit、版本等校验。
3. 优先复用预检已验证的不可变物料，避免应用阶段重新下载/构建。 4.
若重新取源，必须再次核对来源身份、版本/commit、依赖闭包和内容摘要。

测试： - 仅 `fetchedAt` 不同，内容摘要相同。 -
修改实际源码，摘要不同。 - 修改来源身份/commit/版本，计划校验拒绝。 -
预检与应用跨秒完成，正常安装成功。 -
预检后源内容变化，应用拒绝，不得静默安装新内容。

### FIX-02：安装必须事务化

现状：`install_for_instance()` 可能已经写入中央库、实例物化和 profile
接线，之后才计算摘要。摘要计算本身报错时，`?`
直接返回，可能留下部分安装状态。

执行：优先采用 staging 流程：取源/构建 → 校验来源 → 校验摘要 →
校验依赖和入口 → 原子发布中央库 → 实例物化 → profile 接线 →
写安装记录。若暂时无法完整
staging，至少让摘要读取失败和摘要不一致走同一回滚路径。回滚失败必须记录原始错误与回滚错误；未通过校验的插件不能被标记为可用。

测试：摘要遍历中途文件消失、读取权限错误、摘要不匹配、发布失败、profile
接线失败、store
写入失败、回滚失败。每种场景检查中央库、实例目录、store、profile
接线、staging 残留。

### FIX-03：逐跳重定向安全

现状：下载代码先 `.call()` 自动跟随重定向，返回后才检查 redirect
history；禁止目的地可能已经收到请求。锁定依赖 ureq 3.4.0 的默认
`https_only=false`，不能假设客户端自动阻止 HTTP 跳转。

执行： - 禁止自动跟随；手动解析每个 `Location`。 - 每一跳发请求前校验
HTTPS、无 userinfo、host allowlist、跳数上限。 - 相对 Location
必须正确解析；禁止允许 host 跳到未允许 host。 - registry
metadata、tarball、release 下载使用一致策略。 -
保留配置拒绝、来源拒绝、网络失败、HTTP 状态失败的错误分类。

测试：允许 HTTPS 跳转成功；HTTPS→HTTP、允许 host→未允许
host、重定向环、非法 Location 均拒绝；**断言被禁止目标的请求计数为
0**。若 CI 无法监听端口，使用 mock transport 请求计数，不得删测试。

### FIX-04：停止状态机

现状：主停止路径可能先 `guard.take()` 取走 Child，停止失败后仍清理
PID；重试可能因 Child 已不存在而误报成功。实例停止路径也可能忽略
`kill_pid()` 结果直接写 `Stopped`。进程组超时分支存在无界 `child.wait()`
风险。

执行： - 统一 `Stopped / StillAlive / Unknown` 语义。 -
只有确认进程及受管后代退出，才清理 Child、PID、实例记录和桥接资源。 -
StillAlive/Unknown 保留重试依据。 - 超时和查询失败必须有界返回，不能无界
wait。 - 主实例、指定实例、应用退出、孤儿清理采用一致语义。 -
评估锁范围，避免持有全局生命周期锁进行不可控等待，同时保证同实例操作串行化。

测试：正常停止；组长先退出但后代存活；TERM
超时；权限/查询失败；首次失败第二次成功；PID
复用不得误杀；多实例并发停止；操作有严格超时上界。

### FIX-05：孤儿恢复状态分类

现状：`!pid_is_kernel_identity()`
被当成进程已退出，但身份查询失败也可能返回 false，之后恢复代码会删除 PID
记录并记为 recovered。

执行：明确区分 Gone（确认退出）、Ours（仍是本程序）、NotOurs（PID
已被其他进程占用）、Unknown（权限/查询/解析失败）。Unknown 必须保留 PID
和证据，不得记录恢复成功。评估并增加进程创建时间、可执行文件身份、PGID
归属等 PID/PGID 复用保护；不得只凭 PID 数字发信号。

测试：PID 不存在、已退出但句柄打开、PID
被无关进程复用、权限拒绝、查询工具异常、命令行无法解析；逐项检查 PID
文件、报告及信号行为。

### FIX-06：E2E 环境隔离

现状：`buildRuntimeEnv()` 继承 `process.env`，未清理
`DSH_DESKTOP_DATA_DIR`；后端 `kernel::data_dir()`
优先使用它，可能绕过临时 `DSH_XLINK_HOME`。

执行： 1. 审查所有 home/data/profile/cache/port/registry/proxy
覆盖变量。 2. 定义允许继承的环境变量 allowlist；其余清理或重定向到
runRoot。 3. 从运行中的 Tauri 后端读取实际 data dir，canonicalize
后验证位于本轮 runRoot。 4. 路径逃逸时在测试执行前失败并保留证据。 5.
cleanup 只删除本轮明确创建且归属 runRoot 的目录。

测试：预设 `DSH_DESKTOP_DATA_DIR` 为外部目录，启动器仍必须把后端 data
dir 限定在 runRoot；路径逃逸时不得启动测试或清理外部目录。

### FIX-07：E2E 清理与日志

现状：成功且无 `--keep` 时删除整个 runRoot，随后 logger 仍向
`runRoot/evidence/launcher.log` 写入，导致 ENOENT；异常处理再次调用
logger，可能二次失败。测试曾因使用空 logger 而漏检。

执行：证据目录应位于清理根目录之外，或先完成日志
flush/关闭再清理，并确保清理后不再调用该
logger。错误处理本身不能依赖已删除的日志路径。

测试：用真实文件 logger 执行无 `--keep` 成功路径，检查退出码
0、清理结果正确、日志证据按策略保留；另测失败路径和 `--keep`。

### FIX-08：IPC `externalForce`

现状：契约/前端传 `external_force`，Rust Tauri 命令参数默认 wire name 为
`externalForce`。契约接受错误字段，正确字段又被额外字段校验拒绝；强制刷新可能被后端按
false 处理。

执行：统一契约、前端、Rust 参数和 fixtures 的 wire name，建议使用
`externalForce`。检查 Tauri command rename 规则，不能仅凭 Rust
snake_case 推断 wire name。

测试：通过真实 invoke 或与 Tauri 参数反序列化等价的集成测试，证明
`externalForce: true` 进入强制扫描分支；普通查询仍走缓存。

### FIX-09：后台启动与加载页竞争（作废）

2026-11-02 已移除「启动时后台预启动」（`shell/prestart.rs`、`Settings::prestart_kernel`、面板开关与 `prestart_set_kernel`）：普通启动不再在后台起内核，本条描述的竞争现场不复存在，无需实施。

### FIX-10：Defender 测试确定性

现状：Windows CI 测试
`shell::av_trust::tests::registry_reads_only_answer_presence_questions`
直接断言 runner 的 `platform::defender_realtime()` 为 true。CI runner 的
Defender 配置不保证一致。

执行：把注册表读取与业务判定分离，使用注入的注册表/平台查询夹具。真实机器探测只能作为诊断信息，不能作为单测通过条件。

测试：开关存在/缺失/关闭/注册表读取失败等确定性样例；Windows CI
不依赖实际 Defender 状态。

### FIX-11：Channel 结束测试有效性

现状：测试把浏览器的 `result.events` 序列化到 Node
后，再等待并检查同一个数组长度；这个快照不会因浏览器后续收到消息而改变，不能检测
Channel 泄漏。

执行：在浏览器上下文中维护事件计数器；命令结束后等待，再从浏览器读取计数。必要时注入结束后的事件，验证断言会失败。

测试：正常关闭后计数不再增长；故意保持订阅/注入迟到事件时测试必须失败。

------------------------------------------------------------------------

## 3. CI 与整体验收门槛

本次审查时，当前提交 Windows CI 为 **1340 passed、1 failed、14
ignored**；失败为 Defender 状态硬断言。修复后必须重新运行最新提交的完整
CI，不得引用 `0.7.0` 发布时的旧绿灯代替。

完成门槛： - \[ \] FIX-01 至 FIX-11
每项有代码修改或明确说明为何当前代码已解决。 - \[ \]
每项有针对性回归测试；测试名和覆盖的生产行为清楚。 - \[ \]
`npm run check` 全绿（若脚本仍是当前项目质量门禁）。 - \[ \] Rust
fmt、test、Clippy、release compile 检查通过。 - \[ \] Windows CI
全绿。 - \[ \] E2E 默认模式（无 `--keep`）退出码正确。 - \[ \]
安装流程验证成功和失败回滚。 - \[ \] 进程停止/恢复测试覆盖
Unknown、StillAlive、重试和 PID 复用。 - \[ \] 若没有图形桌面或真实
Windows/macOS 环境，明确列出未执行的人工门禁。

## 4. 最终交付报告模板

Agent 完成后按以下格式汇报，不要只说"已修复"：

``` text
# 修复报告

## 基线
- 原始 HEAD：
- 实际 HEAD：
- 工作区是否有用户未提交改动：

## 修复结果
| ID | 结果 | 修改文件 | 核心变化 | 测试 |
|---|---|---|---|---|

## 测试记录
| 命令 | 结果 | 通过/失败/忽略 | 环境 |
|---|---|---|---|

## 未完成/未验证
- ...

## 风险与后续
- ...

## Diff
- `git diff --stat`
- 重要 commit（如有）
```

## 5. 参考链接

-   仓库：<https://github.com/July-X/dsh-xlink>
-   当前审查提交：<https://github.com/July-X/dsh-xlink/commit/b368d25d0c9003284016584f10414c889d89be99>
-   发布版：<https://github.com/July-X/dsh-xlink/releases/tag/desktop-v0.7.0>
-   当前审查时 Windows
    CI：<https://github.com/July-X/dsh-xlink/actions/runs/41383028113>
-   当前审查时发布版
    CI：<https://github.com/July-X/dsh-xlink/actions/runs/41324006469>
