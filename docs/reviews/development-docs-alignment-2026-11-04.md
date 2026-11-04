# 开发文档与代码核对（2026-11-04）

核对基线：`af90517c848aa002edfcb64cb320aa5ae17f67b4`，`main`，源码版本 `0.7.4-rc.1`；开始时工作区干净。本轮扫描 `docs/` 的 68 份 Markdown 文档，按状态词、阶段表及开发入口定位，再定向核对 Rust、插件和面板接线。README 与插件说明一并检查。

这是文档与源码的一致性检查。源码、现有测试、历史运行记录分别注明；本轮没有重新操作真实账号、读取用户数据或验收安装包。历史测试数保留原轮次，不作为本次运行结果。本文是带日期的核对记录，后续进度继续写各专题入口。

## 仍有实现缺口

| 范围 | 当前事实 | 代码依据与后续入口 |
| --- | --- | --- |
| 第二种真实内核 | `McodeAdapter` 仍是 mock（占位实现），能力为空，不能启动真实 mcode；二进制 sidecar（伴随进程）设计未兑现 | [适配器](../../src-tauri/src/kernel/kernel_adapter.rs)的 `McodeAdapter` / `capabilities` / `start`；[接入设计](../architecture/kernel-binary-sidecar-seam.md) |
| 技能社区目录与实例覆盖 | 站内目录、搜索、分页没有实现；外部 topic 链接不是目录。技能启用状态与活动视图仍共享，未做每实例覆盖 | [技能管理](../../src-tauri/src/skills/manage.rs)、[面板](../../ui/src/skills/SkillsPanel.vue)；[设计与边界](../features/extensions/skill-management.md) |
| 官网引用的历史范围与正文完整性 | 当前只列最近 6 条并匹配已加载元数据；没有全历史搜索/目录游标。MiniMax 跳过带 `before` 的分页候选，没有正文分页取全 | [站点规格与取数](../../src-tauri/src/official_chat/session_structure.rs)、[正文取数](../../src-tauri/src/official_chat/session_body.rs)、[宿主命令](../../plugins/official-chat/host/commands.js)；[当前边界](../features/official-chat/official-chat-current-status.md) |
| 官网引用的发送说明与草稿生命周期 | 发送前完整展示实际接收服务商尚未完成；标签移除与发送后草稿集合无法精确观测，仍受内核接口限制 | [客户端](../../plugins/official-chat/client/client.js)、[当前状态](../features/official-chat/official-chat-current-status.md)；保留现有拒绝发送与共享读取策略，不以修改内核为前提 |
| OpenAI 账户管理扩展 | 尚无独立“测试连接”、“删除本地数据”、工作区级多注册选择；退出未接入账户级在途推理取消。退出成功会清活跃令牌，保留注册、账户条目与系统文件密钥 | [账户命令](../../src-tauri/src/openai/cmd.rs)、[授权及退出](../../src-tauri/src/openai/flow.rs)、[账户卡](../../plugins/openai-oauth/client/client.js)；[联调指南](../features/extensions/openai-oauth-bringup.md) |
| OpenAI 精确能力记录 | `CAPABILITY_TABLE` 为空、revision 为 0；展示表提供容量与档位，标为 `codex-catalog`，不能等同逐模型真实能力已验收 | [目录](../../src-tauri/src/openai/catalog.rs)的 `merge_capability`、[选择](../../src-tauri/src/openai/selection.rs)；未知模型仍可勾选，不是全部被禁用 |
| 预检物料复用 | 已绑定固定来源与摘要，但正式应用仍重新安装、事后比对整树并回滚；没有复用预检产物、固化依赖闭包及计划磁盘缓存 | [预检应用](../../src-tauri/src/plugins/precheck.rs)的 `plugin_apply` / `verify_applied_artifact`、[计划缓存](../../src-tauri/src/plugins/install_plan.rs)的 `plans`；[OPT-06 回执](../operations/optimization-2026-11-status.md#回执-opt-06) |
| 系统通知精确跳转 | 点击通知已能回到工作台窗口，尚不能定位具体会话 | [激活入口](../../src-tauri/src/notify/activate.rs)的 `focus_workbench`；[通知设计](../features/notifications/notification-design.md#8-后续可选增强) |
| 记忆检索与成本度量扩展 | 检索仍是本地确定性匹配，未接向量/混合检索；提取任务未持久记录精确 token 账。属于条件触发或后续度量，不能当作当前基础记忆不可用 | [改进计划](../features/memory/memory-mainstream-gap-plan.md)的 P3、[效果设计](../features/memory/effectiveness-dashboard-design.md)文首未做项 |

上述包含已排计划和候选增强，不自动转成开发授权。系统正式代码签名/公证已按 OPT-13 的用户决定豁免，不重新列为待开发；插件中心、旧补丁机制和迁移向导是已移除能力，也不列为缺口。

## 已实现，仍需验证

| 专题 | 已有实现 | 本次仍保留的验收范围 |
| --- | --- | --- |
| 记忆 | 人工保存、注入回执、自动提取、交接/经验、交接草稿、效果扫描/账本/实验；11 月 3 日的失败归因与任务恢复修复均在代码中 | 真模型稳定成功率、两周留存与跨项目使用样本；断网本地推理及扩展计划 P5，见[进度](../features/memory/memory-progress.md)与[可靠性计划](../features/memory/memory-mainstream-gap-plan.md)。可靠性计划 P2 与旧自动提取 P2 是两套编号 |
| 官网对话 | 三站取数、固定快照、实际账户摘要、取消等待、引用详情已有生产接线 | 逐站换号、完整工作台发送、原生资源释放与双平台安装产物；保留临时回程槽位和函数替换可检测的限制，见[当前状态](../features/official-chat/official-chat-current-status.md) |
| OpenAI 插件 | 引导注册、回调验签、凭据库、目录、推理、工具、账户卡和常用模型已有实现及历史请求记录 | 当次三内核窗口 × 两平台、逐模型能力与回放跨重启等矩阵；历史目录不可冒充“最新三个版本”，见[验收清单](../features/extensions/openai-oauth-acceptance-checklist.md) |
| 优化任务 | OPT-01～12 已有实现及回执，不能按初始任务书重复开发 | 双平台真实壳、安装产物与各任务具体未执行项；其中 OPT-06 还有上表的实现差距，见[唯一状态表](../operations/optimization-2026-11-status.md) |

诊断、安全网、托管 Node、插件安装/更新、技能接线、通知、云端套餐和本机用量都有实现入口。设计稿的早期问题列表不能直接当作现状；本轮未重新验证这些模块的全部行为。

## 已订正的过期内容

| 过期描述 | 订正 |
| --- | --- |
| 记忆改进计划仍“待确认”，自动提取/徽标仍“尚未落地” | 索引与设计改为已有实现；将稳定性、长期样本与未实现扩展分开。交接草稿已有 `memory_propose(kind=handoff)`，不再重复列为待建入口 |
| 效果页“没有账本、没有结果关联、没有对照组” | 明确这是立项时背景，现行 P0/P1/P2 已落地；保留精确成本等未做项 |
| 官网开发计划“待实施”、身份/详情/取消仍“未提交” | 按当前提交订正；保留 2026-10-23 的测试读数及平台未验范围，不改协议错误码与权限边界 |
| OpenAI“P1 骨架 / 从未真实联调”、要求注册端点 POST、旧 scope 和目录路径 | 改为浏览器引导注册与回调标识；目录为资源基址加 `/models`，权限串按 `AUTH_SCOPE`。修正退出清理边界、展示表与精确能力表的区别 |
| 只有两个内置插件、新插件只改 Rust 一处 | 登记表实际四个；Rust 与资源脚本清单须同步，不能遗漏打包侧 |
| 旧补丁仅隐藏入口、后端仍可撤销 | README 与架构改为仅历史指纹读取；应用/撤销与随包资源已移除，旧文件不自动清理 |
| 顶部内核页签、旧侧栏分组、迁移入口、旧主题仍属当前界面 | 对齐默认实例、概览/用量分析、资源组含记忆、迁移向导移除，以及 `theme.css` 的两套画布变量；用量独立窗口保留能力但无界面入口 |
| OPT 状态页的旧版本/运行壳被写成当前值，回执仍未勾选 | 改成有日期的历史基线，记录当前源码版本；已有回执勾选，待验收状态不随发布版本升级 |

历史多内核提案、官网调研与诊断设计补上适用范围说明；带日期的 review（审查）、事故、进度日志和只读优化任务书保留原证据，没有把旧失败或旧未执行记录改成通过。

## 本次验证范围

仅改文档，没有修改运行逻辑、依赖、用户数据或发布配置。`npm run check:invariants` 通过，`git diff --check` 通过；改动文档的本地链接、UTF-8 与恰好一个末尾换行检查通过。未运行全量发布门禁或重新构建壳，未进行真实账号与跨平台实机验收。
