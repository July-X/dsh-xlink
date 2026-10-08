# dsh-xlink 文档索引

文档按功能和使用场景分目录。想了解当前代码结构，先看架构；要实现具体功能，再看对应的设计稿和实现说明；审查与事故材料单独保留，避免和当前契约混在一起。

## 推荐阅读顺序

1. [当前架构](architecture/architecture.md)：模块边界、数据目录、窗口和运行时约定。
2. [多内核设计](architecture/multi-kernel/dsh-xlink-multi-kernel-design.md)：多内核模型的设计意图。
3. [功能设计](features/)：按功能选择诊断、插件、技能、迁移、通知或用量文档。
4. [发布与排障](operations/)：构建发布流程和常见问题处理。

## 目录说明

### `architecture/`

桌面壳总体架构、窗口能力、Node 运行时和二进制内核接入。`architecture/multi-kernel/` 保存多内核设计、实施计划、阶段快照以及 P8 UI 提案。

### `features/`

- `diagnostics/`：运行诊断、安全网、快照恢复和二分定位。
- `extensions/`：插件、技能和内置补丁。
- `migration/`：数据迁移向导。
- `notifications/`：任务完成通知。
- `subscription/`：云端套餐、余额和 Token Plan 用量。

内嵌 OpenAI OAuth（开放授权）插件的[设计文档](features/extensions/openai-oauth-design.md)、[开发计划](features/extensions/openai-oauth-development-plan.md)与 [P0 发布包调查](features/extensions/openai-oauth-p0-findings.md)存放在 `extensions/`。前两份为待实现方案，兼容范围是最近 3 个官方发布的 dsh 内核版本；调查报告记录窗口内各版本的接口证据。

### `ui/`

图标母版、本地第三方标志和面板资源约定。

### `operations/`

发布流水线和运行时排障手册。

### `reviews/`

按日期保存的代码审查记录。审查文档反映当时的代码状态，不能替代当前架构和功能设计。

### `incidents/`

已定案事故的证据链、排查过程和可复用工具箱。

### `images/`

设计稿和真实界面截图使用的共享素材，暂不按 Markdown 文档拆分。

## 文档口径

- 设计稿描述目标和约束；实现状态以架构文档和代码为准。
- 历史计划、阶段快照和审查记录保留原日期，不回写成当前状态。
- 修改文档位置后，要同步更新仓库内的 Markdown 链接、源码注释和维护说明。
