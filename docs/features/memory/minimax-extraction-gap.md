# MiniMax 自动记忆提取：接入缺口与安全实现计划

## 当前结论

2026-11-09 核对 `@minimax-webui/minimax-code@0.1.7` 的官方 npm 发布包与本仓实现。MiniMax 已有普通记忆检索注入桥，但**尚未接入会话自动语义提取**。公共池开关、来源标签和审核入口不等于提取链路已经完成，不能以这些 UI 的存在作验收依据。

本轮没有读取真实会话、凭据或调用远端模型，也没有修改已安装内核。以下为发布包静态核查，不是 MiniMax 原生实机验证。

## 已核实的边界

| 项目 | 证据与含义 |
| --- | --- |
| 完成事件存在 | 发布包 `server/runtime.js` 的 `PluginHookRuntime.finishTurn` 会触发 `Stop` / `SubagentStop`；`serializeHookInput` 输出 `session_id`、`turn_id`、`cwd` 和可能为空的 `transcript_path`。因此缺口不是“没有回合结束事件”。 |
| Hook 不提供提取模型服务 | 同文件 `parsePluginHookDocuments` 只接受同步 `command`；非 command、`async: true`、`asyncRewake: true` 被拒绝。不能直接声明一个模型 prompt hook。 |
| Stop 输出不是后台任务 | 同文件 `parseWireDecision` 中，Stop 的 `additionalContext` 或 `decision: block` 会变成 `continuePrompt`，继续执行用户会话。用它要求模型“顺便提取记忆”会影响正常回合，并有工具执行与反复续聊风险。 |
| WebUI 的可见协议是会话操作 | 发布包 `server/server.js` 的 capability 注册包含 `getMessages`、`sendMessage`、`resumeSession`；Mini App 的 `miniAppAgentCall` 转到 `runMiniAppAgentTurn` 等完整代理能力。本轮没有找到可供 hook 使用、保证无工具且不写入用户会话的通用纯模型调用契约。 |
| DSH 提取不可直接拷贝 | 本仓 `plugins/memory/host/index.js::callExtractionModel` 使用 DSH 的 `llm.prepareCall`；调度依赖 `sessions`、`sessionPersistence`、`sessionQuery`、`agentDefaultModel`。MiniMax 命令 hook 没有这些注入服务。 |
| 现有 MiniMax 桥刻意限权 | `src-tauri/src/plugins/builtin/mcode_memory.rs` 的 `context_request_allowed` 只开放检索；`plugins/memory/hooks/context.mjs` 只做 UserPromptSubmit 注入。不能把 DSH 完整管理桥令牌发给 MiniMax hook 来“补齐功能”。 |

发布包来源：[npm 0.1.7](https://www.npmjs.com/package/@minimax-webui/minimax-code/v/0.1.7)。本次下载 tarball 的 SHA-256 为 `c3d569e288f77ec61b37b3c754c76874513d9b259d466c309f0f5af0b8dd1e85`，仅用于核对本次证据版本，不替代安装流程的 registry 摘要校验。

## 后续实现顺序

1. **确定专用提取执行器。** 在壳内复用已支持的模型 provider 通道，或使用经过明确验证的上游无工具调用接口。调用必须有独立请求用途、输出 schema、并发/时长/token 预算，且不进入用户的正常会话。不能通过提示词“请勿用工具”代替实际禁用工具。不能默认把 MiniMax 原始对话转送到 DSH/OpenAI 等另一模型服务；所用提供商和会话数据使用范围需在启用说明中明确。
2. **接入完成事件与来源切片。** Stop 仅提交有限元数据到实例绑定的提取桥，不阻塞或续跑主会话。读取范围须绑定当前实例、profile、会话和回合，校验 `transcript_path` 归属，拒绝路径跳转、符号链接逃逸、超限文件和缺少来源的输入。仅处理启用后发生的回合，不默认扫描历史数据。
3. **复用内容校验。** 将真实用户消息映射成稳定来源序号；只把用户本人陈述作为偏好证据，不把已注入记忆、工具输出或助手猜测当作新记忆。复用现有脱敏、长度预算、JSON schema、证据引用、去重与冲突标记规则。
4. **接入持久任务与候选。** 以实例/profile/session/turn/来源内容哈希/提取器版本组成去重键。失败可见、有限重试；关闭开关、切换实例或停止服务后取消未完成任务。提交端再次校验权限和来源版本；输出只能进入 MiniMax 来源的待审核队列，不能直接写 accepted。
5. **统一审核与引用。** 用户通过审核后进入公共逻辑池，沿用既有来源坐标、项目范围和接收方开关。不同物理库无需搬迁或复制。

## 必须通过的验收

- 本地合成模型与合成会话：完成回合产生一次带 MiniMax 来源的候选，重复事件不重复；未审核前两侧均检索不到，审核后启用的内核能引用。
- 关闭 MiniMax 开关、切换 profile、来源文本变化、进程停止、模型超时与损坏输出：均不产生越权候选，主对话不被阻塞或再次续跑。
- 注入记忆、工具输出和助手文本不能成为用户偏好来源；恶意 transcript 路径不得越界读取。
- 不自动发起工具、不生成用户可见的假会话、不自动转用另一个提供商；账本能区分提取成本与普通对话成本。
- Windows 与 Intel macOS 隔离实机各完成一次真实 hook → 提取 → 审核 → 引用闭环；合成协议测试通过不能替代实机验收。
