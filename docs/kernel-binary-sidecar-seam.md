# 内核二进制 sidecar 接缝设计（含 MiniMax web-ui 承接路径）

> 状态：设计稿 v2，与 P0–P8 配套。P3 已落实 `KernelAdapter` trait 与
> `DshAdapter`；P7 的 `McodeAdapter` 是 stub（所有方法返回 `VersionNotInstalled`）。
> 本文补二进制内核（mcode 及其他非 npm 分发内核）的 sidecar 接缝设计，并显式
> 给出 MiniMax web-ui 承接策略。
>
> 日期：2026-09-21（v1）→ 2026-09-21（v2，扩充 MiniMax web-ui 承接路径）
>
> 关联文档：
> - 主设计：[dsh-xlink-multi-kernel-design.md](dsh-xlink-multi-kernel-design.md)
> - 开发计划：[dsh-xlink-multi-kernel-development-plan.md](dsh-xlink-multi-kernel-development-plan.md)
> - 现状快照：[multi-kernel-migration-status-2026-09-19.md](multi-kernel-migration-status-2026-09-19.md)
> - 现行实现：[src-tauri/src/kernel_adapter.rs](../src-tauri/src/kernel_adapter.rs)
> - 现行实现：[src-tauri/src/kernel.rs](../src-tauri/src/kernel.rs)

## 0. TL;DR

**主线**：Xlink 不只是 mcode 二进制的"启动器"——它是 MiniMax 的 web-ui 壳。

`ui/` 目录（Vue 3 + Element Plus）是 MiniMax **当前没有官方发布的** web-ui 的事实实现。`src-tauri/src/event_normalizer.rs`（新文件）把 mcode 的 ACP 消息归一化到 `NormalizedEvent` 流，前端只消费归一化事件，**不感知内核是 dsh 还是 mcode**。这就是"内核切换 + binary sidecar 接缝"的真正含义——接缝在 normalizer 层，不在 spawn 层。

**未来如果 MiniMax 官方自己发布 web-ui**（无论是 npm 包、独立仓、还是新二进制），Xlink 的角色按 §5.5 的三档策略切换：
1. 优先：Xlink 自己作为 web-ui（**当前主线**，对 MiniMax 官方未发布 web-ui 的真空期最具价值）
2. 兼容：MiniMax 官方 web-ui 通过 settings 中的 `mcode.web_ui_source` 字段可启用，Xlink 退化为 launcher
3. 共存：两者并排，用户在 dropdown 切

这与 AGENTS.md「运行时内核边界」+「信任边界」两条铁律严格不冲突——McodeAdapter 不复制 mcode 源码、不 vendor mcode 包、不在 `pkg.rs` 引入 mcode 的 npm 命名空间。

---

## 1. 现状与缺口

### 1.1 已落地

| 能力 | 状态 | 落点 |
|---|---|---|
| `KernelAdapter` trait（10+ 方法） | ✅ | [src-tauri/src/kernel_adapter.rs:166](../src-tauri/src/kernel_adapter.rs) |
| `DshAdapter` 完整实现 | ✅ | [src-tauri/src/kernel_adapter.rs:225](../src-tauri/src/kernel_adapter.rs) |
| `McodeAdapter` 已注册到 `adapters()` | ✅ | [src-tauri/src/kernel_adapter.rs:154](../src-tauri/src/kernel_adapter.rs) |
| `McodeAdapter` stub 实现 | ✅ | [src-tauri/src/kernel_adapter.rs:432](../src-tauri/src/kernel_adapter.rs) |
| 实例注册表与端口分配 | ✅ | P2 阶段 |
| P8 #1 顶部实例 dropdown | ✅ | P8 阶段 |
| P8 #2 插件面板按实例视图 | ✅ | P8 阶段 |
| ui/ 管理面板（三栏 chat 工作台） | ✅ | `ui/src/` 已有 6 个 ES 模块 |

### 1.2 缺口（McodeAdapter stub 内的所有 TODO）

[src-tauri/src/kernel_adapter.rs:430–494](../src-tauri/src/kernel_adapter.rs) 的 `McodeAdapter` 方法目前全部返回 `AdapterError::VersionNotInstalled`：

- `resolve_install_dir("0.1.0")` → `None`
- `prepare_instance(record)` → `Err(VersionNotInstalled)`
- `start(record, install_root, node)` → `Err(VersionNotInstalled)`（注释："真实 mcode 接入时在这里 spawn `mcode web` 即可"——但 `mcode web` 这个命令当前不在 minimax-code 主仓的 `package.json` 里公开）
- `capabilities()` → `AdapterCapabilities::NONE`

**没有设计**：
1. mcode 二进制从哪里来（npm 路径不可用，AGENTS.md 信任边界硬编码 `@deepseek-ai/*`）
2. spawn 命令的具体形态（`mcode web` / `mcode serve` / `mcode --headless --acp` 都未文档化）
3. 环境变量怎么注入
4. ACP 协议归一化（minimax-code 用 ACP over stdio；DSH 用 JSON-RPC over stdio）
5. 端口分配与健康检查协议
6. `MCODE_CMD` / `MCODE_MODEL` 怎么接入 settings 层
7. `AdapterError` 需要新增 `BinaryNotFound` 变体
8. **新增**：mcode-tools-host（OAuth / 凭据 broker）怎么集成
9. **新增**：MiniMax 没有官方 web-ui，Xlink 的 ui/ 就是事实 web-ui——这层关系需要明示
10. **新增**：如果社区 fork (`mcode-webui/minimax-code-web`) 长成可用 web-ui，Xlink 怎么对接

## 2. 设计目标

在不修改 `KernelAdapter` trait 与 `adapters()` 注册表的前提下，让 `McodeAdapter` 实现以下目标：

1. **二进制发现可配置**——`MCODE_CMD` 环境变量 / settings 字段 / 自动探测三档优先级
2. **spawn 协议统一**——mcode / dsh 都返回 `std::process::Child`，由通用生命周期管理
3. **IPC 协议透明**——前端只消费 `NormalizedEvent` 流，不知道内核是 dsh 还是 mcode
4. **能力位准确声明**——`AdapterCapabilities` 不再是 `NONE`，按 mcode 实际支持情况声明
5. **错误信息可操作**——`BinaryNotFound` 单独成类，给出"安装位置 / 推荐命令"提示
6. **生命周期复用**——不写一行 `if family == "dsh"` / `if family == "mcode"` 分支
7. **遵守 AGENTS.md 信任边界**——不在 `pkg.rs` 引入 mcode 的 npm 命名空间
8. **承担 MiniMax web-ui 事实壳角色**——当官方 web-ui 缺席时，Xlink 的 ui/ 即是 web-ui
9. **mcode-tools-host 集成**——OAuth 凭据 / refresh / logout 经 mcode-tools-host 走，不绕过 Xlink
10. **可降级**——mcode 不可用时 Xlink 仍能跑 dsh 实例，UI 显式提示"该实例族未安装"

## 3. 二进制检测策略（M0）

### 3.1 三档优先级

```text
1. settings 中显式配置的 mcode_cmd 路径        ← 用户主动指定
2. MCODE_CMD 环境变量                            ← 一次性覆盖
3. auto-detect（PATH + 常见安装位置）            ← 兜底
```

按 `paths::resolve_binary("mcode")` 实现，返回 `Result<PathBuf, BinaryNotFound>`。

### 3.2 auto-detect 范围

| 平台 | 探测路径 |
|---|---|
| macOS | `/usr/local/bin/mcode`、`/opt/homebrew/bin/mcode`、`~/.local/bin/mcode`、PATH 中 `mcode` |
| Windows | `C:\Program Files\mcode\mcode.exe`、Chocolatey / Scoop 路径、PATH 中 `mcode.exe` |
| Linux | `/usr/local/bin/mcode`、`/usr/bin/mcode`、`~/.local/bin/mcode`、PATH 中 `mcode` |

探测时检查：
- 文件存在
- 是 regular file（不是目录 / symlink loop）
- 在 macOS / Linux 上 `chmod +x`
- 用 `--version` 或 `--help` 在 2s 内能跑出预期输出（防止挂死文件 / 路径陷阱）

**指纹校验**：`--version` 输出必须包含 minimax-code 的官方指纹字符串（建议在 minimax-code 上游版本稳定后取一次指纹硬编码到 Xlink——或在 Xlink 第一次成功 spawn 时缓存当前指纹）。

### 3.3 AdapterError 新增变体

`src-tauri/src/kernel_adapter.rs:106` 的 `AdapterError` 枚举需要新增：

```rust
/// 二进制未找到（仅适用 binary sidecar 内核族，如 mcode）。
/// `searched` 列出实际探测过的路径，便于用户自查。
BinaryNotFound {
    family: String,
    binary: String,           // "mcode"
    searched: Vec<PathBuf>,   // 探测过的全部路径
    hint: String,             // 用户可操作的修复建议
},
/// 启动成功但 `--version` 输出指纹不匹配（防止误装第三方兼容二进制）
BinaryFingerprintMismatch {
    family: String,
    expected: String,
    got: String,
},
```

`Display` 实现统一格式：

> "未找到 mcode 二进制。已探测路径：[...]. 下一步：运行 `npm install -g @mcode/cli` 或在设置里手动指定 mcode_cmd。"

### 3.4 与 DSH 适配器的差异

DSH 用 npm + pnpm 取源，没有 "BinaryNotFound" 概念——只有 "VersionNotInstalled"。所以 `BinaryNotFound` / `BinaryFingerprintMismatch` 是 mcode 独有的错误变体，DSH 永远不返回。

## 4. spawn 协议（M1）

### 4.1 命令行形态（settings 模板化，不硬编码）

第一版按 minimax-code 当前 `package.json` 暴露的命令：

| 命令 | 来源 | 用途 |
|---|---|---|
| `mcode` / `node dist/cli.js` | `package.json:start` | 顶层 CLI（含 TUI/headless/ACP 入口） |
| `mcode --headless` | minimax-code 架构文描述 | headless 模式（TUI 不阻塞 stdout） |
| `mcode web` | **当前未公开**，社区可能补 | web 工作台模式（最像 dsh `pnpm dsh web` 的入口） |

**v2 决策**：`McodeAdapter::start` 不写死 spawn 命令。`InstanceRecord` 增加 `start_command: Vec<String>` 字段（settings 持久化），默认值为保守兜底：

```rust
// 默认 spawn 模板（settings 里可改）
let default_template = vec![
    "node".to_string(),                  // 用 node 而不是 mcode 命令——不依赖 PATH
    "dist/cli.js".to_string(),           // minimax-code 仓库的 entrypoint
    "--headless".to_string(),            // 强制 headless（TUI 不阻塞 stdio）
    "--acp-server".to_string(),          // 启用 ACP server（架构文确认存在）
    "--port".to_string(), "{port}".to_string(),
    "--workspace".to_string(), "{workspace}".to_string(),
];
```

模板里 `{port}` / `{workspace}` / `{home}` / `{model}` 在 spawn 时按 `InstanceRecord` 字段替换。settings 允许用户改这个模板——上游 mcode 命令语义变化时**只改 settings 不改 Xlink 代码**。

### 4.2 关键设计决策

| 决策 | 理由 |
|---|---|
| 默认走 `node dist/cli.js` 而不是 `mcode web` | minimax-code 主仓官方只有 `start` 入口；`web` 命令未文档化，不写死 |
| 不继承 shell 的 PATH | mcode 可能跑出与 npm 不同的 node_modules；只给最小 PATH（node bin 目录 + 系统 bin）|
| `MCODE_HOME` 而不是 `DSH_HOME` | mcode 不读 DSH_HOME，强制自己命名空间隔离 |
| `MCODE_DSH_XLINK=1` | 让 mcode 知道是被 Xlink 启动——影响是否启 telemetry、是否启 file watcher |
| `kill_on_drop(true)` | Xlink 退出时由 `Child::drop` 兜底杀进程；正式退出走 `process::terminate_process_tree` |
| `--port` 由 Xlink 指定 | 让 Xlink 的端口分配（已有 P2 实例管理）继续是唯一真理源 |
| stdio piped | ACP over stdio（mcode 内部已实现 headless + ACP adapters）|
| `--acp-server` 默认开启 | minimax-code 架构文确认 `packages/tui` 拥有 ACP adapters |

### 4.3 模板化 spawn 的实现草图

```rust
// src-tauri/src/kernel_adapter.rs 中 McodeAdapter::start 的实现草图

fn start(
    &self,
    record: &InstanceRecord,
    install_root: &Path,
    node: &Path,
) -> Result<std::process::Child, AdapterError> {
    let mcode_root = record.home_dir.parent()
        .ok_or_else(|| AdapterError::Io("mcode root 解析失败".into()))?;
    let template = record.start_command_template
        .clone()
        .unwrap_or_else(default_mcode_template);

    // 模板替换
    let args: Vec<String> = template.iter().map(|arg| {
        arg.replace("{port}", &record.port.to_string())
           .replace("{workspace}", record.workspace.to_str().unwrap_or(""))
           .replace("{home}", record.home_dir.to_str().unwrap_or(""))
           .replace("{model}", &record.model)
    }).collect();

    let mut cmd = std::process::Command::new(&args[0]);
    cmd.args(&args[1..]);
    cmd.current_dir(mcode_root);

    // 不继承 shell PATH
    cmd.env_clear();
    cmd.env("PATH", minimal_path_for_process());
    cmd.env("MCODE_HOME", &record.home_dir);
    cmd.env("MCODE_MODEL", &record.model);
    cmd.env("MCODE_DSH_XLINK", "1");
    cmd.env("MCODE_INSTANCE_ID", &record.id);

    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let child = cmd.spawn().map_err(|e| AdapterError::Io(format!(
        "启动 mcode 失败：{e}"
    )))?;
    Ok(child)
}
```

### 4.4 与 `process.rs` 的协同

`process.rs` 已经实现了 PATH 合并、静默窗口和进程组回收（AGENTS.md 第 5 段「实现约定」）。McodeAdapter::start 不直接复用 `process::` 工具——因为 spawn 命令构造属于适配器职责。但 **start 返回 `Child` 后**，进程管理完全由 `process.rs` / `instance.rs` 接管，与 dsh 实例走同一条路径。

## 5. IPC 协议映射（M2）—— 核心接缝

### 5.1 协议现状对比

| 维度 | dsh | mcode（minimax-code） |
|---|---|---|
| 主通信 | JSON-RPC 2.0 over stdio | ACP（Agent Communication Protocol）over stdio |
| 消息格式 | newline-delimited JSON | newline-delimited JSON |
| 状态推送 | SSE over HTTP（web UI） | 由 web UI 自己从 ACP 派生 |
| Web UI 入口 | `pnpm dsh web` 启内置 HTTP server | minimax-code 主仓**未公开** web UI；TUI/CLI/ACP 是公开面 |

### 5.2 Xlink 在中间层的角色

**Xlink 不直接吃 dsh 或 mcode 的原生事件**。它在适配器层做一道归一化（normalize），向上抛 `NormalizedEvent` 流。

```text
dsh stdio JSON-RPC ─┐
                   ├─→ KernelAdapter::normalize_event ─→ NormalizedEvent ─→ ui/state-bus.js
mcode stdio ACP ───┘
```

**关键判断**：Xlink 的 `ui/` 目录就是 MiniMax **当前没有官方发布的** web-ui——这是设计 §5.5 的"主线"。

### 5.3 NormalizedEvent 形态

```rust
// src-tauri/src/event_normalizer.rs (新文件)

#[derive(Debug, Clone)]
pub enum NormalizedEvent {
    /// 实例状态变化（启动 / 停止 / 健康）
    State { instance_id: String, kind: InstanceState },
    /// 模型消息 delta（流式输出）
    ChatDelta { instance_id: String, msg_id: String, delta: String },
    /// 工具调用 / 完成
    Tool { instance_id: String, name: String, input: serde_json::Value, output: Option<serde_json::Value> },
    /// 用户需要回答问题（AskUser 语义）
    Ask { instance_id: String, request_id: String, questions: Vec<AskQuestion> },
    /// 权限请求
    Permission { instance_id: String, request_id: String, scope: String, reason: String },
    /// Usage / 计费
    Usage { instance_id: String, kind: UsageKind, count: u64 },
    /// 错误 / 致命
    Error { instance_id: String, code: String, message: String, recoverable: bool },
}
```

这个类型**与内核族无关**——dsh 和 mcode 都翻译到这个类型。`KernelAdapter` trait 新增一个方法：

```rust
/// 把内核原生日志/JSON-RPC/ACP 消息解析为 NormalizedEvent 流。
/// 返回 `Ok(None)` 表示消息被识别但不该发出（如 keepalive）。
/// 返回 `Err(_)` 表示消息格式异常但不致命——记日志后忽略。
fn normalize_event(
    &self,
    instance_id: &str,
    raw: &[u8],
) -> Result<Option<NormalizedEvent>, AdapterError>;
```

### 5.4 dsh / mcode normalizer 各自覆盖的消息类型

DSH normalizer（基于 dsh 的 JSON-RPC 通知命名）：

| dsh 通知 | NormalizedEvent |
|---|---|
| `session/state` | `State` |
| `chat/delta` | `ChatDelta` |
| `tool/call` | `Tool`（output = None） |
| `tool/result` | `Tool`（output = Some） |
| `permission/ask` | `Permission` |
| `ask/user` | `Ask` |
| `usage/update` | `Usage` |

mcode normalizer（基于 ACP 标准消息形态——minimax-code 用 `pi-mono` 提供的 ACP 实现）：

| ACP 消息 | NormalizedEvent |
|---|---|
| `agent/state` | `State` |
| `message/delta` | `ChatDelta` |
| `tool/use` | `Tool`（output = None） |
| `tool/result` | `Tool`（output = Some） |
| `permission/request` | `Permission` |
| `question/ask` | `Ask` |
| `usage/report` | `Usage` |

> **注意**：mcode normalizer 的具体消息命名要等 minimax-code 上游公开 ACP 细节后才能定稿。本设计稿**只承诺归一化抽象**，不锁定具体消息名——M2 阶段实现时以"消息体里识别语义"为准，不要求严格按上面的名字。

### 5.5 NormalizedEvent 流 → ui/state-bus.js 消费

`ui/src/state.js` / `ui/src/render.js` **不感知内核族**。它们只看到 `state.events` 里归一化后的事件流。**这就是 Xlink 充当 MiniMax web-ui 的接缝点**——前端代码是 MiniMax web-ui 的实现，跟 dsh / mcode 都无关。

切换内核不需要重写前端组件。新增第三种内核（codeline / codex 等）只需要写 adapter + normalizer。

### 5.6 未知消息与降级

normalizer 解析失败时的兜底：
- 单条消息解析失败 → `Err(...)` → 记一行日志 + 跳过（不冒泡到 UI）
- 连续 N 条解析失败（说明协议错位）→ 触发 `NormalizedEvent::Error { recoverable: false }` → UI 显式提示"协议不兼容，请升级内核或 Xlink"
- 健康检查协议（heartbeat）解析失败 → 静默忽略

### 5.7 stdio 多路复用（mcode ACP）

mcode ACP 与 dsh JSON-RPC 都用 stdio 做双向通信——但 mcode 不只走 ACP，还用 stdio 跑交互式命令（headless 模式）。normalizer 必须**区分控制面与数据面**：

```text
stdio[0] (stdin)  ← Xlink → mcode  : JSON-RPC request / notification
stdio[1] (stdout) → Xlink ← mcode  : JSON-RPC response + 通知流
stdio[2] (stderr) → Xlink ← mcode  : 普通日志（按行读，转发给 ui/src/logs.js）
```

Xlink 在 `process.rs` 已经实现的「PATH 合并、静默窗口、进程组回收」基础上新增「stdio 三路分流」：
- stdout 按 newline 拆 JSON
- stderr 按行直接转发
- stdin 在需要发 request 时再写（normalizer 不主动写，由 `KernelAdapter::request_*` 方法触发）

## 6. 端口 / 健康检查模型（M3）

### 6.1 端口分配继续由 Xlink 负责

P2 阶段 `instance.rs` 已经实现了「实例端口从 Xlink 的可用端口范围分配，分配前同时检查注册表、实例锁和实际监听状态」。mcode 实例走同一条路径：spawn 时 `--port {port}` 传给 mcode；Xlink 在 health check 时 TCP probe 这个端口。

### 6.2 健康检查协议

DSH 的健康检查走 HTTP（SSE over HTTP）。Mcode 的健康检查**复用同一条 TCP probe 路径**——但 ACP 模式下 mcode 不一定暴露 HTTP（minimax-code architecture 说 ACP 是 stdio over process）。

**双协议健康检查**：

```rust
async fn check_instance_health(record: &InstanceRecord) -> HealthStatus {
    // 1. TCP probe record.port（兼容性检查）
    if let Ok(status) = tcp_probe(record.port, Duration::from_secs(2)).await {
        if status.is_open() { return HealthStatus::Healthy; }
    }

    // 2. 通过 stdio 发 ACP ping（主路径）
    match adapter.ping(&record).await {
        Ok(()) => HealthStatus::Healthy,
        Err(e) => HealthStatus::Failed(e.to_string()),
    }
}
```

`AdapterError::PortBusy { port, owner_pid }` 已经在 [src-tauri/src/kernel_adapter.rs:112](../src-tauri/src/kernel_adapter.rs)——新增 `PortHeldByForeignProcess` 处理端口被无关进程占用的情况：

```rust
pub enum AdapterError {
    // ... 现有 ...
    /// 端口被无关进程占用，且 owner 不是当前实例 pid
    PortHeldByForeignProcess { port: u16, owner_pid: u32, owner_cmd: String },
}
```

## 7. 设置层接入（M4）

### 7.1 settings.json 新增字段

`src-tauri/src/settings.rs` 当前按 DSH 模型管理 port / profile / model。新增 mcode 相关字段：

```json
{
  "instances": {
    "default-dsh": { "family": "dsh", "version": "0.1.5-rc.1", "port": 3090, "...": "..." },
    "mcode-default": {
      "family": "mcode",
      "mcode_cmd": null,                // null = auto-detect；字符串 = 用户指定路径
      "mcode_root": null,               // null = auto-detect mcode 仓库根（GitHub clone 后路径）
      "mcode_model": "minimax_api/MiniMax-M3",
      "port": 3091,
      "start_command_template": null,   // null = 用 default_mcode_template
      "...": "..."
    }
  },
  "default_instance_id": "default-dsh",
  "kernel_families": {
    "dsh": { "enabled": true, "default_version": "0.1.5-rc.1" },
    "mcode": {
      "enabled": true,
      "binary_discovery": "auto",      // auto / explicit
      "web_ui_source": "xlink",        // xlink / mcode-official / mcode-community
    }
  }
}
```

### 7.2 MCODE_CMD 环境变量

按 AGENTS.md「运行时内核边界」的现有约定，`MCODE_CMD` / `MCODE_MODEL` 已经在设计稿里写过（AGENTS.md 顶部），但**尚未在代码里实现**。本设计文档把它们落地到 `settings.rs`。

`env.rs`（[src-tauri/src/env.rs](../src-tauri/src/env.rs)）新增：

```rust
pub struct McodeEnv {
    pub mcode_cmd: Option<PathBuf>,   // env: MCODE_CMD 一次性覆盖
    pub mcode_model: Option<String>,  // env: MCODE_MODEL
    pub mcode_root: Option<PathBuf>,  // env: MCODE_ROOT 指向 minimax-code 仓库根
}
```

Settings 加载时按「settings 字段 > env 变量 > 默认值」三档解析，结果注入到 `InstanceRecord`。

### 7.3 ui/ 设置面板

`ui/src/store.js` 的 settings 视图增加 mcode 实例卡片：

- 「实例族」下拉：DSH / mcode
- 「mcode 命令路径」输入框（带"自动探测"按钮）
- 「mcode 仓库根」输入框（带"自动探测"按钮）— 指向 minimax-code 源码
- 「mcode 模型」输入框
- 「启动命令模板」高级编辑（JSON 数组）
- 「自动启动」开关
- 「实例端口」输入框（默认 3091）

按 P8 #1 已经实现的顶部实例 dropdown，用户在 dropdown 里切换实例即可看到对应内核的工作台。**无需在 dropdown 里区分内核族**——dropdown 只展示 instance_id，下拉项的副标题显示 instance 的 family。

### 7.4 mcode-tools-host 集成（M4.5）

minimax-code architecture 文里明确说：

> "mcode-tools-host provides short-lived access tokens to tool subprocesses through a local lease broker."

这是 mcode 的 OAuth / 凭据 broker。Xlink 必须正确接入这个组件，否则 mcode 子进程拿不到 API token。

**集成方式**：
- mcode-tools-host 与 mcode 内核**同进程部署**（minimax-code architecture 文说"the same runtime through local applications and services"）
- mcode 启动时自动拉起 mcode-tools-host（Xlink 不需要单独管理）
- Xlink 通过 `MCODE_TOOLS_HOST_ENDPOINT` env 告诉 mcode 这个 broker 的本地 socket / 端口

```rust
// McodeAdapter::start 的 env 注入部分
cmd.env("MCODE_TOOLS_HOST_ENDPOINT", &tools_host_endpoint);
```

`tools_host_endpoint` 由 Xlink 的 `paths::tools_host_endpoint(instance_id)` 解析——

每个实例一份 `~/.dsh-xlink/kernels/mcode/instances/<id>/tools-host.sock`（Unix）或 `tools-host.pipe`（Windows）。mcode 启动后自动 connect 这个 socket，Xlink 不直接管理 mcode-tools-host 进程，只管理 socket 路径。

### 7.5 mcode 源码 build helper（M6）

`mcode_cmd` 指向编译后的 `dist/cli.js`。但 minimax-code 主仓不直接发布 npm 包——用户得自己 `git clone` + `pnpm install` + `pnpm build`。

**Xlink 的辅助流程**：

```rust
// src-tauri/src/mcode_source_build.rs (新文件)

pub async fn ensure_mcode_built(
    mcode_root: &Path,
    progress: impl Fn(BuildProgress),
) -> Result<(), AdapterError> {
    if mcode_root.join("dist/cli.js").exists() {
        return Ok(());   // 已 build
    }
    progress(BuildProgress::Cloning("git clone ..."));
    // ... 调用 minimax-code 官方 git 仓库
    progress(BuildProgress::PnpmInstalling);
    // ... 调用 pnpm install
    progress(BuildProgress::Building);
    // ... 调用 pnpm build
    progress(BuildProgress::Done);
    Ok(())
}
```

UI 入口：`设置 → 添加 mcode 实例 → "从源码构建"` 按钮——触发后台 build，进度推送到 `ui/src/progress.js`。

`mcode_root` 默认探测顺序：
1. `MCODE_ROOT` env
2. settings 字段
3. `~/.dsh-xlink/cache/mcode-source/`（Xlink 自己的克隆缓存）
4. 用户手动指定路径

`pnpm install` 走 Xlink 嵌入式 Node 运行时（`src-tauri/src/node_install.rs` 已有）。

## 8. 能力位声明（M1 阶段产出）

`McodeAdapter::capabilities()` 当前返回 `AdapterCapabilities::NONE`。真实接入时按 mcode 实际能力声明：

```rust
fn capabilities(&self) -> AdapterCapabilities {
    AdapterCapabilities::from_iter([
        AdapterCapability::BinarySidecar,
        AdapterCapability::CustomStartArgs,
        AdapterCapability::CustomEnvVars,
        AdapterCapability::AcpStdout,           // 新增
        AdapterCapability::LeaseBrokerCompat,   // 新增（mcode-tools-host）
        AdapterCapability::HeadlessMode,        // 新增（--headless 启动）
    ])
}
```

新增 `AdapterCapability` 变体：

```rust
// src-tauri/src/kernel_adapter.rs（AdapterCapability 枚举内）

/// 内核族由二进制 sidecar 启动，不通过 npm 分发。
BinarySidecar,
/// 内核族允许自定义启动参数（--workspace、--port 等）。
CustomStartArgs,
/// 内核族允许自定义环境变量。
CustomEnvVars,
/// 内核族通过 stdio 暴露 ACP（Agent Communication Protocol）。
AcpStdout,
/// 内核族与 mcode-tools-host 凭据 / OAuth broker 兼容。
LeaseBrokerCompat,
/// 内核族有 headless 启动模式（--headless）。
HeadlessMode,
```

DSH adapter **不声明这几个新能力位**——它有 `InstallVersion` / `ProfileWiring` / `DshHomeEnv` 那一套旧能力位。

`AdapterCapability` 的对照表（UI 用这个决定渲染什么按钮）：

| 能力位 | 触发 UI 行为 |
|---|---|
| `InstallVersion` | 显示"安装版本"按钮（dsh 走 npm 安装） |
| `ProfileWiring` | 显示"profile 接线"按钮（dsh 改 profile package.json） |
| `DshHomeEnv` | 启动时注入 DSH_HOME（dsh 专属） |
| `BinarySidecar` | 显示"指定二进制路径"按钮 + "二进制探测"按钮（mcode） |
| `CustomStartArgs` | 渲染"启动命令模板"编辑框 |
| `CustomEnvVars` | 渲染"自定义环境变量"表 |
| `AcpStdout` | 用 normalizer 解析 stdio（替代 SSE 推送） |
| `LeaseBrokerCompat` | 启动前生成 `MCODE_TOOLS_HOST_ENDPOINT` 路径 |
| `HeadlessMode` | 默认 `--headless` 注入 |

## 9. 分阶段交付

| 阶段 | 范围 | 依赖 | 验收 |
|---|---|---|---|
| **M0** | `AdapterError::{BinaryNotFound, BinaryFingerprintMismatch}` + `paths::resolve_binary("mcode")` + auto-detect + settings mcode_cmd 字段 | 无 | 单元测试覆盖：探测路径、空文件、不存在的 binary、`MCODE_CMD` 覆盖、指纹校验 |
| **M1** | `McodeAdapter::start` 真实 spawn（模板化） + `capabilities()` 真实声明 + 6 个新能力位 + `AdapterError::PortHeldByForeignProcess` | M0 | spawn 一个 stub mcode 二进制（模拟 `--version`），记录 child handle 与 env vars；`start_rejects_missing_binary` 测试扩展到 mcode |
| **M2** | `event_normalizer.rs` + dsh 与 mcode 双 normalizer + ui/src/state.js 消费 `NormalizedEvent` 流 | M1 | 集成测试：dsh 与 mcode 各自跑一个 fixture session，断言 NormalizedEvent 流等价；前端在切换 instance 时不感知 family |
| **M3** | `health.rs` 双协议健康检查（TCP probe + stdio ACP ping）+ 端口占用分支 | M1 | TCP probe 与 stdio ping 测试，端口占用分支测试 |
| **M4** | ui/ 设置面板新 mcode 实例卡片 + 顶部 dropdown 副标显示 family + `MCODE_CMD` / `MCODE_MODEL` 接入 `env.rs` + mcode-tools-host socket 路径生成 | M2 + M3 | 手动验收：用户能在 UI 创建 mcode 实例并启动；env 变量覆盖工作；tools-host socket 被 mcode 自动连接 |
| **M5** | **MiniMax web-ui 承接策略落地**——ui/ 即 mcode 官网；UiState 持久化 mcode 特有视图（model picker / workspace switch）；ui/src/i18n.js 加 mcode 命名空间 | M4 | 用户开 Xlink 直接拉起 mcode 工作台，体验与 dsh 一致；切换实例时不感知内核族 |
| **M6** | mcode 源码 build helper（M6.1 detect → M6.2 clone → M6.3 install → M6.4 build → M6.5 cache） | M4 | UI 触发后台 build，进度可观测；产物可被 M1 启动器直接消费 |

**M0–M1 是接缝设计的最小可用切片**（约 3–4 周）——完成后 `McodeAdapter` 至少能 spawn 一个 mcode 子进程。**M2–M5 是接缝真正发挥作用的部分**（约 6–8 周）——前端透明切换内核，Xlink 充当 MiniMax 的事实 web-ui。M6 是开发期体验优化，可选。

## 10. 测试与验收

### 10.1 单元测试

- `resolve_binary` 对每档优先级分别测；空 PATH 下不 panic
- `AdapterError::BinaryNotFound::Display` 输出可读中文
- `AdapterError::BinaryFingerprintMismatch::Display` 包含 expected / got
- `AdapterCapabilities::from_iter` 与 `contains` 互逆
- `McodeAdapter::capabilities()` 声明的集合符合 §8
- start_command_template 替换 `{port}` / `{workspace}` / `{home}` / `{model}` 正确

### 10.2 集成测试

- `start_rejects_missing_binary` 扩展：mcode binary 不存在时返回 `BinaryNotFound`，且 `searched` 列出实际探测路径
- mcode stub fixture（一个 echo `--version` 的 shell 脚本）能成功 spawn，`--port` / `MCODE_HOME` / `MCODE_MODEL` 等参数传递正确
- 两个实例（dsh 与 mcode 各一个）同时运行，端口不打架、pid 文件隔离、runtime/ 不互相覆盖
- dsh normalizer 把 fixture JSON-RPC 通知翻译成 `NormalizedEvent`；mcode normalizer 把 fixture ACP 消息翻译成 `NormalizedEvent`；同一 fixture（语义对应）输出**等价**的 NormalizedEvent 序列

### 10.3 端到端验收（M5 之后）

1. 全新安装 dsh-xlink，默认只创建 dsh 实例（mcode adapter 注册但不创建实例）
2. 用户在设置面板手动添加 mcode 实例（指定 mcode_cmd 路径或触发 M6 源码 build）
3. 顶部 dropdown 出现新实例，副标题显示 `mcode`
4. 启动 mcode 实例，**ui/ 的三栏 chat 工作台直接接管 ACP 事件流**——这就是"Xlink 作为 MiniMax web-ui"的现场演示
5. 与 dsh 实例并排显示，互不干扰
6. 关闭 Xlink，两个实例的子进程都被 `process::terminate_process_tree` 正确回收
7. mcode-tools-host socket 被 mcode 自动 connect，OAuth 凭据流工作

### 10.4 验收门槛（对照 AGENTS.md）

- AGENTS.md「运行时内核边界」："项目不携带或重新发布 dsh 内核代码"——**遵守**：McodeAdapter 不复制 mcode 源码、不 vendor mcode 包。M6 build helper 只 `git clone` 到 Xlink cache 目录，不入版本控制。
- AGENTS.md「信任边界」："仅信任官方 deepseek-ai 仓库与 npm @deepseek-ai 命名空间"——**遵守**：mcode 不通过 `pkg.rs` 取源；只走 binary sidecar + git clone。git clone URL 硬编码到 minimax-code 官方仓（`MiniMax-AI/minimax-code`）。
- AGENTS.md「实现约定」：`process.rs` 的 PATH 合并、静默窗口和进程组回收策略——**遵守**：所有 Tauri spawn 命令走现有 `process::` 工具。
- AGENTS.md「跨模块重复先提共享层」：`event_normalizer.rs` 模块放在共享层，dsh 与 mcode 共用 `NormalizedEvent`。

## 11. 与上游的依赖

### 11.1 当前阻塞项

| 项 | 状态 | 影响 |
|---|---|---|
| mcode `web` 子命令 | **未公开** | §4.1 用 `node dist/cli.js --headless --acp-server` 兜底 |
| mcode ACP 消息命名 | 部分公开（架构文确认存在） | §5.4 表格需要 M2 实现时实测校准 |
| minimax-code npm 发布 | **未发布**（`@mavis/*` 私有） | §7.5 用 git clone + 源码 build 兜底 |
| mcode-tools-host socket 命名 | 文档不公开 | §7.4 默认 `~/.dsh-xlink/kernels/mcode/instances/<id>/tools-host.sock`，mcode 上游文档化后可调 |
| minimax-code 官方指纹字符串 | 未公开 | §3.2 第一次成功 spawn 时缓存指纹 |

### 11.2 监控上游变化（决策触发器）

| 上游信号 | Xlink 行动 |
|---|---|
| minimax-code 主仓发 `mcode web` 文档 | 把 `default_mcode_template` 默认值从 `--headless --acp-server` 改成 `web --port ...` |
| minimax-code 主仓发 npm 包（`@MiniMax-AI/minimax-code-kernel` 等） | `pkg.rs` 扩白名单；`BinarySidecar` 适配器转 `InstallVersion` 适配器 |
| MiniMax 发布官方 web-ui（独立仓 / npm 包） | §5.5 三档策略：用户可在 settings 切 `web_ui_source: xlink → mcode-official` |
| minimax-code 文档化 mcode-tools-host | 把 socket 路径 hardcode 改成 env 注入 |
| minimax-code 上游 PR 被 `mcode-webui` fork 维护者合并 | §6.5 社区 fork 路径触发；评估 fork 是否能作为 default mcode 启动目标 |
| `mavis-code/Mavis-CLI` 出现独立 web-ui 子项目 | 评估是否成为第四个 `KernelFamily`（目前 KERNEL_FAMILY_DSH / KERNEL_FAMILY_MCODE 两档，需要扩到 3 档） |

### 11.3 MiniMax 官方发布 web-ui 的应对策略

如果 MiniMax 官方发布 web-ui（无论是 npm 包、镜像、新二进制还是独立仓），按 §5.5 的三档策略：

1. **首选 Xlink 作为 web-ui（主线）**——保持当前 ui/ 是 MiniMax web-ui 的事实实现
2. **兼容 MiniMax 官方 web-ui**——`kernel_families.mcode.web_ui_source` 切到 `mcode-official`，Xlink 的 ui/ 退化为 launcher / plugin manager / settings 面板
3. **共存**——两者并排，dropdown 切实例族

**原则**：Xlink 永远不强行用 MiniMax 官方 web-ui 替换自己的 ui/——因为官方 web-ui 可能不带 plugin / skills 管理、Xlink 的扩展管理体验无法被官方复刻。

## 12. 风险与开放问题

| 风险 | 触发条件 | 缓解 |
|---|---|---|
| mcode 上游 web 命令语义与本设计不一致 | 上游发版改了命令名 / 参数名 | start_command 走 settings 配置，不硬编码；`default_mcode_template` 默认值随上游变 |
| ACP 协议细节与 dsh JSON-RPC 不对称 | normalizer 翻译逻辑复杂 | M2 阶段先实现 dsh normalizer，mcode normalizer 等有真 binary 再写 |
| mcode binary 升级破坏 Xlink 兼容 | 上游改了 stdout 协议 / env 语义 | normalizer 容错：解析失败时返回 `NormalizedEvent::Error` 而不是 panic；指纹校验失败走显式升级提示 |
| 用户误装第三方 mcode 兼容二进制 | 路径探测命中非官方 binary | `--version` 输出指纹校验（首次 spawn 缓存指纹） |
| mcode-tools-host socket 路径冲突 | 两个 mcode 实例共用一个 socket | 每个实例独立 socket 路径（§7.4） |
| mcode 源码 build 时间长（pnpm install 几分钟） | M6 触发时用户等待 | 进度推送 + 用户可取消；构建产物进 Xlink cache 目录共享 |
| MiniMax 官方发 npm 包，Xlink 走 `pkg.rs` 取源 | 信任边界需要扩展 | §11.2 监控；扩白名单需要单独的 ADR 决策 |
| `mcode-webui/minimax-code-web` fork 长成社区事实标准 | 社区维护者持续 push | §6.5 触发 |
| mcode ACP 用 binary 协议（不是 JSON） | 协议假设错了 | normalizer 失败时降级为 raw passthrough；UI 显式"协议未知" |

## 13. MiniMax web-ui 承接路线图（本文档的主线）

### 13.1 三档承接策略（详细）

| 场景 | `web_ui_source` | Xlink 角色 | UI 行为 |
|---|---|---|---|
| MiniMax 未发 web-ui（当前） | `xlink` | **事实 web-ui 壳** | ui/ 直接拉起 mcode 工作台，正常聊天 / 工具 / ask / permission |
| MiniMax 发官方 web-ui | `mcode-official` | Launcher + Plugin Manager | ui/ 显示 MiniMax 官方 web-ui 的 launcher + 设置面板 + 实例管理 |
| 社区 fork 维护 | `mcode-community` | 双 web-ui 共存 | dropdown 多一项"社区 web-ui 实例"，Xlink 充当 launcher 跑社区 web-ui |
| Xlink 自身作为 web-ui | （默认） | 主线 | ui/ 是事实 web-ui |

**核心原则**：

- Xlink 的 ui/ **永远不应该被 MiniMax 官方 web-ui 替换**——Xlink 承担 plugin / skills / instance / settings 这些通用管理职责，这些是 MiniMax 官方 web-ui 不太可能复刻的。
- 当用户用 `web_ui_source: mcode-official`，Xlink 在 Tauri WebView 里加载 MiniMax 官方 web-ui URL（`http://127.0.0.1:<port>`），同时 Xlink 的 plugin / settings 面板作为侧栏常驻。
- 当用户用 `web_ui_source: xlink`，Xlink 的 ui/ 直接接管（当前默认）。

### 13.2 McodeAdapter 与 ui/ 的契约

```rust
// KernelAdapter trait 新增（与 normalize_event 配套）

/// 返回这个实例族对应的 web-ui 入口 URL（Xlink 用 WebView 加载）。
fn web_ui_entry(
    &self,
    instance: &InstanceRecord,
    health: &HealthStatus,
) -> Option<WebUiEntry>;

pub struct WebUiEntry {
    pub source: WebUiSource,           // xlink / mcode-official / mcode-community
    pub url: String,                   // http://127.0.0.1:<port>
    pub title: String,
    pub consumes_events: bool,         // true = UI 接管 ACP 事件；false = UI 仅显示 launcher
}
```

DSH adapter 返回 `WebUiEntry { source: xlink, ... }`（因为 dsh 的 web UI 在 dsh 内部，Xlink 的 ui/ 替代）。
McodeAdapter 返回 `WebUiEntry { source: mcode-official }`（当 settings 切到官方 web-ui 时）或 `WebUiEntry { source: xlink }`（默认——Xlink ui/ 充当 web-ui）。

### 13.3 路线图

```text
T0（当前，2026-09-21）
  └─ Xlink 仅支持 dsh。McodeAdapter 是 stub。

T1（M0–M1，~3 周后）
  └─ Xlink 能 spawn mcode 子进程，但 UI 仍是 dsh 三栏。
  └─ mcode 实例的"打开工作台"按钮 = 浏览器新窗口加载 `http://127.0.0.1:3091/`
  └─ 这是最低限度集成。

T2（M2–M4，~8 周后）
  └─ NormalizedEvent 流接入 ui/state-bus.js。
  └─ mcode 实例的"打开工作台"按钮 = **Tauri WebView 加载 Xlink 自己的 ui/ 工作台**，事件流来自 mcode ACP。
  └─ **Xlink ui/ 正式充当 MiniMax web-ui**。

T3（M5，~10 周后）
  └─ 完整 ui/ 适配 mcode：model picker / workspace switch / mcode 特有 UI 控件。
  └─ dsh 与 mcode 在 ui/ 层体验一致——切实例不切 UI。

T4（MiniMax 官方发 web-ui 后）
  └─ §13.1 三档策略落地。
  └─ Xlink 在 Tauri WebView 加载 MiniMax 官方 web-ui；Xlink 侧栏常驻 plugin / skills 管理。

T5（McodeAdapter 长期演进）
  └─ 当 minimax-code 发 npm 包，`BinarySidecar` 路径可降级为 `InstallVersion` 路径。
  └─ 当 mcode-tools-host 文档化，socket 路径改为 env 注入。
  └─ 当 minimax-code 上游 `pnpm mcode web` 文档化，§4.1 默认模板从 `--headless --acp-server` 改成 `web`。
```

### 13.4 与现有设计文档的关系

本设计文档**不替换** `dsh-xlink-multi-kernel-design.md`，只补 §7.2（适配器接口的职责）的 mcode 侧细节。原设计 §7.2 描述的是抽象接口；本文档描述的是第一个非 dsh 实现的具体形态，并显式给出了 MiniMax web-ui 承接策略。

如果上游 mcode 演进后需要修改本设计，按 dsh-xlink 项目的修改规则同步更新 `dsh-xlink-multi-kernel-design.md` 与本文档，保持两份文档引用一致。

---

## 附录 A：术语对照表

| 术语 | 含义 |
|---|---|
| Shell 构建模式 | dsh-xlink 自己的 `release` 或 `dev` |
| 内核族 | `dsh` / `mcode`（当前）/ 其他未来 |
| 内核实例 | 一个可独立启动、停止、连接、内核族一致的运行单元 |
| Binary sidecar | Xlink spawn 的非 npm 分发二进制子进程（mcode 等） |
| ACP | Agent Communication Protocol，minimax-code 的 stdio 协议 |
| mcode-tools-host | minimax-code 的 OAuth / 凭据 broker（与 mcode 同进程部署） |
| NormalizedEvent | Xlink 适配器层归一化的事件流，前端只读这个 |
| WebUiSource | `xlink` / `mcode-official` / `mcode-community` 三档 web-ui 来源 |

## 附录 B：与既有文档的引用图

```text
AGENTS.md                                  ← 顶层规约（信任边界 / 内核边界）
   │
   ├── docs/architecture.md                ← 顶层架构图（已含多内核数据布局）
   │
   ├── docs/dsh-xlink-multi-kernel-design.md        ← 主设计（P0–P8 + 数据目录 + 适配器抽象）
   │      │
   │      ├── docs/dsh-xlink-multi-kernel-development-plan.md   ← 实施计划
   │      ├── docs/multi-kernel-migration-status-2026-09-19.md  ← 状态快照
   │      │
   │      └── docs/kernel-binary-sidecar-seam.md    ← **本文档**（补 mcode 侧的具体形态）
   │             │
   │             ├── src-tauri/src/kernel_adapter.rs ← KernelAdapter trait
   │             ├── src-tauri/src/kernel.rs         ← DSH 适配器落地
   │             ├── src-tauri/src/event_normalizer.rs  ← M2 新增
   │             ├── src-tauri/src/mcode_source_build.rs ← M6 新增
   │             └── ui/src/{state,render,events}.js ← 前端归一化消费
   │
   └── docs/{patch-management,plugin-management,skill-management,...}.md  ← 各子系统设计
```