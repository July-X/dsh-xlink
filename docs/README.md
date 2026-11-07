# dsh-xlink 文档索引

文档按功能和使用场景分目录。想了解当前代码结构，先看架构；要实现具体功能，再看对应的设计稿和实现说明；审查与事故材料单独保留，避免和当前契约混在一起。

本轮[开发文档与代码核对](reviews/development-docs-alignment-2026-11-04.md)列出了确认的实现缺口、待验收项和已订正的旧描述；后续开发先读对应专题的当前状态。

## 推荐阅读顺序

1. [当前架构](architecture/architecture.md)：模块边界、数据目录、窗口和运行时约定。
2. [多内核设计](architecture/multi-kernel/dsh-xlink-multi-kernel-design.md)：多内核模型的设计意图。
3. [功能设计](features/)：按功能选择诊断、插件、技能、迁移、通知或用量文档。
4. [发布与排障](operations/)：构建发布流程和常见问题处理。

## 开发专题入口

根 [AGENTS.md](../AGENTS.md) 只维护全仓开发约束；按任务查阅下列专题。专题维护完整契约与操作方法，历史记录不替代当前约束。

| 任务 | 文档 |
| --- | --- |
| 内核归属、取源信任与共享服务 | [运行时边界与共享实现](architecture/runtime-boundaries.md) |
| 路径、实例、dev / release 隔离与搬迁 | [内核磁盘布局](architecture/kernel-data-layout.md#8-路径解析与隔离契约) |
| 窗口主题、托盘、退出与登录自启 | [窗口架构](architecture/window-architecture.md) |
| 本机 Codex 许可、外部工具用量与缓存诊断 | [本机用量来源契约](features/subscription/local-usage-sources.md) |
| 官网登录、浏览器档案与引用链路 | [官网对话开发指南](features/official-chat/official-chat-agent-guide.md) |
| 定向检查、实跑隔离、证据与反向验证 | [开发验证](operations/development.md) |
| 版本、tag、流水线、资产与发布后核验 | [发布手册](operations/release.md) |
| MiniMax Code 第二工作台 | [统一工作台接入计划](architecture/multi-kernel/minimax-webui-integration-plan.md)、[开发交接](architecture/multi-kernel/minimax-webui-handoff-2026-11-07.md)（实验接入，验收未完成） |
| 附件检查、预检物料复用、退出清理与通知开发 | [后续开发计划](operations/development-followup-2026-11-plan.md)（该轮排除第二内核与全历史搜索） |
| 2026-11 优化任务的执行与状态 | [优化任务状态表](operations/optimization-2026-11-status.md)（任务书[原稿](operations/optimization-2026-11-taskbook.md)） |
| 事故经过与历史证据 | [agent 复盘](reviews/agent-lessons.md)、[事故档案](incidents/) |

## 目录说明

### `architecture/`

桌面壳总体架构、运行时边界、数据隔离、窗口能力、Node 运行时和二进制内核接入。`architecture/multi-kernel/` 保存多内核设计、实施计划、阶段快照以及 P8 UI 提案。

### `features/`

- `diagnostics/`：运行诊断、安全网、快照恢复和二分定位。
- `extensions/`：插件、技能与旧补丁机制的历史设计；补丁机制已移除，当前兼容边界见[运行时边界](architecture/runtime-boundaries.md#已移除的补丁机制)。插件页精简（移除远端「插件中心」、保留手动安装与内置插件）见[设计稿](features/extensions/plugin-page-simplification-design.md)（2026-10-31 已实现）。
- `memory/`：[Codex 离线记忆分析](features/memory/codex-offline-memory-analysis.md)与[dsh-xlink 本地记忆设计](features/memory/dsh-xlink-memory-design.md)；本地记忆已落地（人工保存 + 检索注入 + 自动提取），当前进度与判据见[开发清单](features/memory/memory-progress.md)。另有[从 PowerContext 提取可复用能力的开发计划](features/memory/powercontext-extraction-plan.md)——结论是**不把 PowerContext 装成插件**（装不上、版本代差、注入重复、缺常驻服务监管），只取它缺的两样：**工作交接**与**经验**，落在既有记忆模型内。效果观测与「记忆有没有用」的度量方案见[记忆效果可视化设计](features/memory/effectiveness-dashboard-design.md)（2026-10-31 设计稿；P0/P1/P2 已实现，面板原型与指标模型仍可参照）；给 agent 读的效果快照、定位方式与口径纪律见[记忆效果快照](features/memory/agent-effect-snapshot.md)。下一步追赶主流自动记忆、修效果读数与提取可靠性的分阶段计划见[记忆能力追赶主流 Agent：分阶段开发计划](features/memory/memory-mainstream-gap-plan.md)（P0/P1 的归因与读数修复已有实现；真模型稳定性、长期使用与条件触发的召回升级仍待推进）。
- `official-chat/`：官网会话直接 `@` 引用；插件、快照存储及真实站点取数已有实现，Windows 完整链路仍在收口。接手开发先核对[当前状态](features/official-chat/official-chat-current-status.md)，再读[agent 执行指南](features/official-chat/official-chat-agent-guide.md)、[开发计划](features/official-chat/official-chat-development-plan.md)、各阶段协议和[最新进度记录](features/official-chat/official-chat-progress.md)。[早期调研](features/official-chat/official-chat-bridge-research.md)保留作背景，不能替代当前契约与验收结果。
- `notifications/`：任务完成通知。
- `subscription/`：云端套餐、余额、Token Plan 用量，以及[本机用量来源契约](features/subscription/local-usage-sources.md)。

内嵌 OpenAI OAuth（开放授权）插件的[设计文档](features/extensions/openai-oauth-design.md)、[开发计划](features/extensions/openai-oauth-development-plan.md)、[P0 发布包调查](features/extensions/openai-oauth-p0-findings.md)、[真实服务联调指南](features/extensions/openai-oauth-bringup.md)与[实机验收清单](features/extensions/openai-oauth-acceptance-checklist.md)存放在 `extensions/`。P0–P5 已有实现，后续真实账号请求曾推动授权、工具与用量修复；当次内核窗口、双平台及逐模型完整验收仍待补。P0 调查中的三版本清单是历史快照，不能当作当前最新窗口。

### `ui/`

图标母版、本地第三方标志和面板资源约定。

### `operations/`

[开发验证](operations/development.md)、[发布流水线](operations/release.md)和[运行时排障](operations/troubleshooting.md)；[2026-11 优化任务状态表](operations/optimization-2026-11-status.md)与[任务书原稿](operations/optimization-2026-11-taskbook.md)（执行状态只改状态表，任务书保持原样）。

### `reviews/`

按日期保存的代码审查与 [agent 复盘](reviews/agent-lessons.md)。这些文档反映当时的代码状态，不能替代当前架构和功能契约。

### `incidents/`

已定案事故的证据链、排查过程和可复用工具箱。

### `images/`

设计稿和真实界面截图使用的共享素材，暂不按 Markdown 文档拆分。

## 文档口径

- 设计稿描述目标和约束；实现状态以架构文档和代码为准。
- 历史计划、阶段快照和审查记录保留原日期，不回写成当前状态。
- 当前架构与边界放 `architecture/`，功能协议和设计放 `features/<功能>/`，执行步骤放 `operations/`，审查 / 复盘放 `reviews/`，独立事故证据放 `incidents/`。
- 新增专题在本索引登记，并从相关任务入口链接；同一契约只维护一份正文，其他位置用摘要与链接引用。不要将功能细节、性能样本或事故过程重新堆回 `AGENTS.md`。
- 修改文档位置后，要同步更新仓库内的 Markdown 链接、源码注释和维护说明。
