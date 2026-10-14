# dsh-xlink 本地记忆系统设计

日期：2026-10-13；修订：2026-10-14。状态：设计提案，尚未实现。依据：[Codex 离线记忆系统分析](codex-offline-memory-analysis.md)。

本次修订按接入核查收紧：项目身份改用**有效工作目录**；注入预算改为**会话级累计上限**；`jobs`/租约与 Markdown 视图移出首版；路径去掉 `mode` 维；补齐物化落点、桥的威胁模型与门禁前置。

## 1. 目标与交付边界

用户在 dsh 工作台完成任务后，可以保存长期有效的偏好、项目决策和已验证的解决方法；后续在同一项目继续工作时，智能体自动得到一份有界、带来源的相关记忆，用户能查看、纠正、停用和删除它。

首版交付“明确保存 + 自动检索”：保存无需额外模型请求，检索与排序在本机完成。从会话自动提炼候选是后续阶段，只接经过验证的本地模型，不自动回退到远端模型。云同步、跨设备账号记忆、屏幕采集、自动生成技能、向量服务均不在范围内。

本地记忆不自行同步到服务端，但用户若在普通对话中选择远端模型，注入的记忆也会成为该模型的输入。启用页须说明这一点；“本地保存”不等于“上下文永不出网”。要求全程断网时，对话与候选提取都必须用可离线运行的模型，当前设计不承诺 dsh 自带这种模型。

### 1.1 关键取舍

| 选择 | 原因 |
| --- | --- |
| 随壳交付内置 Host 插件 `xlink-memory` | 用 dsh 的正式扩展面，不侵入官方内核 |
| Rust 是记忆数据的唯一写入方 | 管理面板、模型工具、后台任务共用一套隔离与删除规则；**内核没在跑时面板仍要能看、能停用**，这条决定了库不能只住在内核进程里 |
| 单文档 JSON 为权威记录（**已定案**，见 §4.1） | 单写者下的 CAS 与原子替换够用；本仓无任何 SQLite 依赖（含间接），且有“免 C 工具链”约束 |
| 按当前项目检索，用户偏好需明确批准 | 不把 A 项目的业务信息自动带到 B 项目 |
| 自动提取只形成候选 | 模型总结与第三方文字不能自行成为长期规则 |
| 复用动态运行上下文 | 不改项目 `AGENTS.md`，不把记忆冒充最高优先级指令 |
| 首版不出 Markdown 视图、不建任务租约表 | 模型用工具检索，文件视图只服务人；没有后台任务就不需要租约 |

### 1.2 与 AGENTS.md 指令链的关系

内核已有用户全局指令链（`$DSH_HOME/AGENTS.md` 与 `~/.agents/AGENTS.md`，默认开启、64 KiB 预算）和项目层 `AGENTS.md`/`CLAUDE.md` 链。启用页照此分工写指引：必须始终生效、要随仓库走的规则写 `AGENTS.md`；可检索、带来源、需要停用与删除的条目存记忆。两边重复时以 `AGENTS.md` 为准——指令链是用户直接维护的规则，记忆是本地回忆层。

## 2. 用户如何使用

管理面板增加“记忆”入口，沿用现有 Element Plus 列表、搜索、详情与确认交互；默认当前项目，另有“此实例的通用偏好”和“待确认”两个视图。页面必须标明“设计实施后才可用”，本文不能当作已上线说明。

首版支持：

- 新建或编辑一条记忆，选择当前项目或通用偏好；显示最后修改时间、来源和验证状态。
- 在工作台要求“记住这条约定”时，智能体调用 `memory_propose` 生成**待确认候选**，工具结果给出候选编号与正文摘要；用户在面板“待确认”里点“保存”。首版不做对话内卡片、不加客户端模块，需要用户切到面板——这一点写进工具结果文案。
- 下次任务自动检索已接受的记忆，工作台显示“本次使用了 N 条记忆”，可展开看内容与来源。
- 停用后，新回合不再新增记忆贡献；已准备或正在发送的请求撤不回，当前会话里的旧快照仍可能出现在请求历史中。
- 删除前显示影响范围；删除后阻止再次检索与相同来源自动重建，并保留处理会话历史的入口。
- 需要带走或人工审阅时“导出 Markdown”到用户选择的位置：一次性快照，不参与运行时，手工修改不反向入库。

写操作只有面板能发起（含批准候选、改开关、跨项目推广、清空）；模型工具只有搜索、读取、提出候选三种能力。

使用已批准记忆默认关闭，自动提取也默认关闭；后者未通过本地模型验收时显示不可用及原因，不用同一个“已开启”掩盖缺失能力。

**开关生效需重启工作台**：接线写在 `$DSH_HOME/profiles/<profile>/cordis.patch.yml`，由内核启动时读取，内核在跑时壳拒绝改这类开关（既有判据）。界面照此提示，不承诺“点一下就生效”。

## 3. 已核对的 dsh 接入合同

本机 release 的 `active.txt` 为 `0.2.1-alpha.2`。本节只读该安装包的源码与类型，未启动工作台、未调用模型；路径相对 `node_modules/@deepseek-ai/`，行号属本次样本。

| 接入点 | 包文件与行号 | 确认的行为 |
| --- | --- | --- |
| 动态上下文 | `dsh-system-prompt/lib/types/index.d.ts:72–83,266` | `PromptContext` 的 `name`/`order`/`text` 必填、`text` 同步；`interpolate` 默认 true、`required` 默认 false；从 `agent.ctx` 注册即落该 agent 的 scope |
| 上下文槽位 | `dsh-system-prompt/lib/index.js:44–49,267` | `getContextOrder()` 存在，但只认四个名字：`WORKING_DIRECTORY:100`、`SANDBOX_POLICY:110`、`APPROVAL_POLICY:115`、`SUBAGENT_DELEGATION:120`。未知名字返回 `undefined`（不抛错），到 `context({order: undefined})` 才抛 TypeError。本插件用字面量 300 |
| 请求前刷新 | `dsh-system-prompt/lib/types/index.d.ts:292–301`；`dsh-agent-loop/lib/index.js:1053–1073` | 每次请求尝试前 `refreshContext()` 重解析**装配时已注册**的贡献，再提交 user-role 快照；装配后才注册的名字不会被补上（`required: true` 除外）——所以必须用 `agent/created` 注册 |
| 首步钩子 | `dsh-agent/lib/types/runtime-types.d.ts:293–310`；`dsh-agent-loop/lib/index.js:906,909–924` | `agent/pre-step` 是可替换 messages 的 waterfall，inbox 在它之前 `claim()`；保留 `next()` 的决定是插件自律，非内核强制 |
| 快照去重 | `dsh-agent-loop/lib/index.js:298–350` | 比较拼接后的**整段文本**：相同不产生新快照，不同则新发一条 `user/message`（`surfaceOp: "append"`），旧快照留到压缩才移除 |
| 初始化 | `dsh-agent/lib/types/runtime-types.d.ts:213–231` | `agent/created` 串行、按 scope 过滤，`source` 取 `startup/resume/clear/compact`，`agent.ctx` 的贡献随销毁卸载。**该监听器抛错会让 agent 创建失败** |
| 工作目录 | `dsh-working-directory/lib/types/index.d.ts:24–30,83`；`lib/index.js:122` | 有效工作目录可中途改变并发 `working-directory/change`；`session.header.cwd` 是冻结的创建元数据（“原始项目”）。运行时上下文里的 “Current working directory” 取自有效值 |
| 回合日志 | `dsh-session/lib/types/index.d.ts:51–61`；`types.d.ts:273` | `session/event` 是提交后的观察通知（fire-and-forget，监听器抛错被记录并隔离），可筛 `turn/end` |
| 持久化屏障 | `dsh-session/lib/types/types.d.ts:265–274`；`index.d.ts:483` | 回合结束不自动 flush；`ctx.sessions.flush(session)` 是唯一入口，只对 live/entered 会话有效，返回值表示是否有持久化监听器参与（false 时盘上可能没有新内容） |
| 记录读取 | `dsh-session-persistence/lib/types/index.d.ts:22–32,126,159`；`handle.d.ts:75` | 有 snapshot 的 revision/eventCount、open/list 与 `SessionHandle.read(offset,length)`。**`eventAt`/`snapshotEvents`/`ownEvents` 已标 `@deprecated … new calls are prohibited`** |
| 项目身份 | `dsh-workspace/lib/types/index.d.ts:264`；`types.d.ts:9–13,57–65` | `resolveByPath(path)` 存在：路径不存在时 reject，存在但未登记返回 `undefined`；工作区 ID 是生成的 UUID，路径经 realpath |
| 工具注册 | `dsh-tools/lib/types/index.d.ts:115–128,636` | `ctx.tools.register()` 需要 `output` 模式与 `execute`；调用 agent 可能缺失，项目读写必须拒绝这种情况 |
| 后台模型 | `dsh-llm/lib/types/index.d.ts:390,416` | `prepareCall()` / `stream()` 可用于不创建用户对话的请求 |
| 请求用途 | `dsh-llm/lib/types/types.d.ts:530` | `purpose` 是字面量联合 `'compaction' \| 'session-title'`，`memory-extract` 不存在；先省略并由插件审计用途 |
| 压缩结果 | `dsh-compaction-basic/lib/index.js:469–506` | 有 `compaction/start` 与带 `error?` 的 `compaction/end`；end 受 `closing` 门控，**失败路径可能只有 start、没有 end**，不能只等 end |

这些接口给出可行接缝，不等于功能已实现；兼容判定要检查实际服务和方法（`typeof getContextOrder === 'function'` 这种探测会通过，但 `MEMORY` 这个名字并不存在），并逐版本验收，首期只承诺已验收版本。

### 3.1 首版上下文装配

1. 在 `agent/created` 中解析项目身份并注册 `xlink.memory` 上下文贡献，声明 `inject` 所需服务。**这个监听器里的一切失败都要吞掉**（解析不到项目、桥不可用、注册失败都降级为“本次无记忆”并记状态）——抛出去会让 agent 创建失败，普通对话都起不来。
2. 注册必填的 `name`、`order`（字面量 300）与 `text`，`interpolate: false`，注册普通 context，不改其他插件的贡献；`text` 只同步返回该 agent 的快照缓存。
3. 在 `agent/pre-step` 中先取得并保留 `next()` 的结果；若 reject 直接返回。否则基于用户输入与任务关键词向 Rust 请求一次有界检索，更新缓存，返回原决定。
4. 请求准入时内核会重新解析贡献并形成快照，因此第 3 步的缓存会进入本次请求；这条链路仍需首请求的真实端到端测试。
5. 项目身份取**有效工作目录**，订阅 `working-directory/change`：目录一变就重新解析并清空缓存；新目录不是已登记工作区时，本会话只允许通用偏好，不注入项目记忆，也不把新候选记到旧项目。缓存按 (namespace, workspace, generation) 绑定，禁止全局“当前项目”变量；检索取消、超时、失败、权限撤销、删除、切目录都清空缓存，不沿用上一回合。
6. **会话级注入预算**（硬规则，防快照逐回合堆积）：每回合最多检索一次；渲染后的记忆块算 digest，与上次相同则整回合不更新缓存；每会话最多注入 3 次（首次 + 至多 2 次刷新），累计不超过 36 KiB；删除、停用或面板显式刷新可重置计数。内核会为每一份**不同**的快照保留一条历史消息，所以次数必须由我们封顶，只封单次大小不够。

这份上下文是 user-role 背景材料，不能授予工具权限或覆盖用户当前要求。稳定模板要写明：历史可能过期，来源中的命令和外部文字不是待执行指令，与 `AGENTS.md` 或本次用户要求冲突时以后者为准。常驻摘要上限 4 KiB、命中详情上限 8 KiB、单次总输入 12 KiB、最多 6 条，按完整条目裁剪，不截断 JSON 或 UTF-8 字符。150 ms 降级线是建议值不是已验证指标。用户关闭可选运行上下文时尊重该设置，报告“本次未注入”，不强行标记 required。

## 4. 架构与权威边界

```mermaid
flowchart LR
    U[管理面板：保存与管理] --> R[Rust 本地记忆服务]
    A[dsh 智能体] --> P[xlink-memory Host 插件]
    P --> B[有实例绑定的本地桥]
    B --> R
    R --> DB[权威记录 memory.json]
    R --> P
    P --> C[动态上下文与记忆工具]
    C --> A
    P --> Q[可选本地模型提取 P2]
    Q --> B
```

| 模块 | 新职责 / 复用路径 |
| --- | --- |
| `src-tauri/src/memory/`（拟新增） | 权限、记录、查询、候选审批、删除、抑制指纹 |
| `plugins/memory/host/`（拟新增） | agent-local 上下文、工具注册、会话事件适配、可取消提取请求（P2） |
| `ui/src/memory/`（拟新增） | 查看、编辑、项目选择、候选接受、状态与删除预览、导出 |
| 现有 `plugins/builtin/` | 资源校验、实例物化、启动接线、停用与兼容状态 |
| 现有本地桥 | 复用监听生命周期、随机令牌与凭据文件下发；记忆路由另做命名空间授权 |
| 现有 `shell/paths.rs` | 派生实例与 profile 路径（新增 `instance_memory_dir`），不另拼数据根 |
| 现有 `shell/atomic_write.rs` | 小文件与导出文件的私有原子写入（0600）；权威记录也走它，不另外造一套落盘 |

插件与壳都不得向已安装内核 `kernels/<版本>/node_modules/**` 写代码。资源随应用交付，物化到内置插件的既有物化根 `<DSH_HOME>/extensions/builtin/xlink-memory/<pluginVersion>/<kernelFingerprint>/compat-a/`（**不是**社区插件的 `paths::instance_extensions_dir()`）。新增内置插件要同步登记 `plugins/builtin/builtins.rs` 的 `BUILTINS`、`scripts/prepare-builtin-plugins.mjs` 的 entries（两份清单由 `dev-builtin.test.mjs` 钉死）、`plugins/memory/package.json` 的 `name = xlink-memory` 和 `package.json` 的 `test:plugin`。

### 4.1 存储选型：单文档 JSON，不引入 SQLite（2026-10-14 定案）

设计稿原先把「引入 SQLite」列为 P0 放行项，落地时的结论是**不引入**，理由三条：

1. **冲突是真实的**。本仓没有任何 SQLite 依赖（`Cargo.toml` 无、`Cargo.lock` 连间接命中都没有），而两处既有注释写明「AGENTS.md 要求在 Intel macOS / Windows 的 CI runner 上免 C 工具链构建」（WebSocket 不开 TLS、zstd 只用纯 Rust 的 `ruzstd` 都是为此）；`rusqlite` 的 `bundled` 要 C 工具链，系统 sqlite3 在 Windows 上还要随包带库。而「Windows 能不能编过」本机验证不了。
2. **P1 不需要数据库**。真实需求只有三件：单写者、事务性替换、损坏不改语义。写入全部发生在壳进程内（面板走 Tauri；内核插件经回环桥回来仍落到这个进程），一把进程内 Mutex 串行「读-改-写」就够；`shell::state` 的 `read_state_file`（区分「不存在」与「损坏」）+ `shell::atomic_write_private`（0600 临时文件 + rename）已经给了另外两件。
3. **代价可接受**。每命名空间一份 `memory.json`，规模上限每项目 1,000 条、单条正文 8 KiB，整份文档在几百 KB 量级；检索在本机做有界关键词匹配（§6.2），不需要索引。

跨进程并发（P2 的任务租约）到时再评估：那时可以给文档加一个只追加的 `jobs` 段，或换实现——权威记录的语义（CAS、代次、删除指纹）不变，换的只是承载。

**落地形状**：`memory/store.rs` 管落盘事务（读-改-写、损坏、schema 降级拒绝），`memory/query.rs` 管纯函数的检索与注入文本渲染，`memory/cmd.rs` 是面板命令层。三者与设计稿的对应关系见 §12。

## 5. 数据隔离与目录

```text
<DSH_XLINK_HOME>/kernels/<family>/instances/<id>/memory/<profile>/
├── memory.json                    权威记录、候选与元数据（0600，原子替换）
└── exports/                       用户触发的 Markdown 导出（一次性，可删）
```

命名空间是 `(family, instance_id, profile)`，项目是命名空间内的 `workspace_id`。记忆按实例隔离，而实例本身已按壳模式分家（`default` / `default-dev`），**不加 `mode` 维**：它会是全仓唯一一处实例内 mode 派生路径，且两壳被人为指向同一实例时 mode 与实例归属还会矛盾；那种反常规配置下记忆共享与共享会话、插件属同类事实，由既有“禁止改动正在运行的实例”兜底纪律覆盖变更操作。路径由 `paths::instance_memory_dir(family, id, profile)` 派生并过 `validate_id_component`；桌面恒为 `web` profile（壳不带 `--profile` 启动），保留该层以对齐内核 home 布局，多 profile 不共享记忆是预期行为。

项目解析用有效工作目录与 `resolveByPath()`，只查已存在的工作区。无工作区、目录不存在或解析失败时只允许用户批准的通用偏好，不自动建项目、不扫全机。多工作树、目录搬迁、Windows 大小写差异走现有工作区登记与用户确认的映射；不凭 Git 远端地址自动合并项目。

模型提供方切换不扩大记忆权限。沙盒预检、诊断会话、无 agent 的工具调用默认无项目记忆；子智能体最多继承父级的读取范围，不自动继承写入资格。这种隔离是服务层边界：同一操作系统用户启动的任意脚本仍可能读磁盘，模型工具的范围检查不是操作系统级沙盒。

## 6. 权威记录与检索

### 6.1 最小数据模型

每个命名空间一份文档，三块：

| 块 | 主要内容 |
| --- | --- |
| `memories` | id、项目或通用范围、正文、主题/关键词、来源、验证状态、revision、状态与使用时间 |
| `candidates` | 待确认内容、事件范围、来源 revision/hash、提取器版本、审批结果 |
| `meta` | schema 版本、数据代次（缓存失效用）、两个开关与同源抑制指纹 |

任务队列与租约属 P2（§8），首版不建。`meta.schema_version` 在“库比代码新”时只读并明确报错，绝不就地降级迁移；文档损坏时同样拒绝写入（展示路径可以空列表 + 明说，但读-改-写路径必须停下来）。

来源分两种：面板新建保存 `user_manual`、操作时间与修改来源，不虚构会话 ID；会话候选与提取结果至少含 session ID、事件起止序号、源 revision/hash、提取时间。原始会话由内核维护，不把全量日志复制进记忆库。`user_explicit`/`observed`/`session_inferred` 区分信息来源，`verified`/`unverified`/`stale` 表示验证范围，不用未校准的置信分数替代它们。

个人偏好不能从项目候选自动提升；提升前须展示正文与来源，去掉路径、客户名、仓库私有信息，并由用户明确选择通用范围。自动提取结果保持候选状态，默认不参与检索。

### 6.2 确定性检索

先过滤权限、启用状态、删除状态、来源有效性和当前项目，再匹配主题、关键词与正文；排序依次为用户固定项、明确约定、当前项目匹配度、验证状态、最近有效使用，稳定 ID 收尾。数量、扫描字节与响应字节都有硬上限。

首版不做向量检索：在规模上限内做有界关键词匹配，中文按明确关键词与短语匹配，不假设英文空格分词适用。超过建议的每项目 1,000 条或单次扫描 2 MiB 时返回可定位的缩小查询提示并评估全文索引，不静默读全库。

“展示次数”与“有效引用次数”分开：首版只记录 recall 而不用于自动删除，只有显式读取或回答中引用可核对的记忆 ID 才算使用反馈；重复请求按 turn ID 与 generation 去重，别让重试刷高重要性。

模型工具提供 `memory_search(query)`、`memory_read(id)`、`memory_propose(content, source)`——是新插件协议，不是 dsh 现有方法。工具从 agent/session 身份推导项目，不接受调用者传入实例、目录或任意路径；输出模式、输入大小、取消信号与引用范围都要校验。

## 7. 保存、更新和删除

### 7.1 保存

保存或接受候选时，用事务校验预期 revision，写入正文与来源，递增数据代次让运行缓存失效。模型不改权威记录；将来增加模型整理也只能输出结构化提案，走同一提交路径。

首版不生成 Markdown 视图、不维护视图指针：那是为“模型靠 grep 读文件”设计的（Codex 的场景），这里模型走 `memory_search/read`，文件只服务人，而面板本来就能看。需要文件时用 §2 的导出，一次性、不参与运行时与代次。

### 7.2 更新与冲突

编辑用 compare-and-swap，旧窗口、旧提取任务不能覆盖新修改。相同规范化正文可合并来源；意义冲突的记忆标“待处理冲突”，不能由模型悄悄选择胜者。用户纠正优先于旧候选；过期的技术事实要重新验证后再用。

### 7.3 删除与防止再生

删除事务立即把条目标为不可读、移除正文与候选正文、写入不含正文的同源抑制指纹、递增数据代次、使插件缓存失效。P2 的后台任务提交前检查这个指纹、开关与来源版本，不能在用户刚删除后重建同一条。

删除后立即阻止再次检索；首版没有衍生视图要清理，但用户导出过的文件在磁盘上，界面要说明“已导出的文件不会被删除”。清空前列出记录与候选范围，需用户明确确认。

原始会话、模型已收到的请求、内核历史里的运行上下文快照不会因删除记忆消失，界面必须说明并引导另行处理；要确保下一请求不含旧内容，请用新建的干净会话。不能把清空插件缓存当成旧消息已从模型输入移除，也不能擅自改写内核的用户消息。删除是「移除记录 + 记指纹 + 原子替换整份文档」：不承诺取证级擦除（文件系统的快照、日志与磨损均衡都不受我们控制），首版承诺“不再检索、清理可管理的正文副本”。

## 8. 后续自动提取流程（P2）

1. 监听 `session/event` 的 `turn/end` 并冻结截止序号；只安排有用户内容、非临时诊断且获许可的会话。要读盘上记录先 `ctx.sessions.flush(session)`，返回 false（无持久化监听器参与）就改用 live 会话的可见历史。**不用 `eventAt`/`snapshotEvents`/`ownEvents`（已废弃、新调用被禁）**，用 `sessionPersistence.open(id,'read')` 取 `SessionHandle.read(offset,length)`。
2. 本阶段新增 `jobs` 表（任务键、来源截止游标、状态、lease、所有权令牌、重试与失败原因）及租约、重试、迟到结果检查。任务键含命名空间、session ID、来源 revision/截止序号与提取器版本；短时间内连续回合合并，只留尚未开始的最新任务，运行中的任务保留自己的冻结输入。
3. Rust 原子领取租约，插件从有限事件范围构建输入：排除工具输出里的凭据、大块附件与无关正文，不任意遍历用户目录；脱敏复用 `diagnostics::run::sanitize`（凭据只留掩码），模式匹配只能减少风险、不能保证发现所有秘密。
4. 用已验收的本地模型经 `ctx.llm.prepareCall()` / `stream()` 发送有限输入，不创建用户可见对话、不执行工具。`purpose` 不能填 `memory-extract`，先省略并由插件审计用途；要正式分类得在内核仓库扩展，不能冒用 `session-title`。
5. 输出必须是有 schema 的候选与来源引用；无价值时输出空集合作为成功。工具结果里的指令不能升级为用户偏好，未验证方案不能写成成功经验。
6. 只有持有当前 lease token、来源仍有效、未被删除或撤销许可的任务能提交；来源变化时保留旧版本候选并标过期，或放弃排新任务，不能覆写更新结果。
7. 用户接受后进入权威记录；初期不做跨会话模型合并，只有重复整理真的成为瓶颈再考虑。

压缩只在成功路径发 `compaction/end`，**失败或 end 追加失败时可能只有 `compaction/start`**，所以“失败压缩”的判据要含“超时未见 end”。

建议初始预算：每命名空间一个运行任务、等待队列 20 项、连续回合去抖 30 秒、单次输入 64 KiB（超限按完整事件截取并标记不完整）、执行 deadline 120 秒（租约需续期）、瞬态失败重试两次。都是待验收参数，不是 Codex 默认值。

提取任务有自己的取消控制器：新用户回合优先让出模型资源，停用、清空、dispose、壳退出都取消提取，迟到结果还要过提交版本检查；不能借 `agent/turn-stopping` 或独占 `runMaintenance()` 做后台提取。也不得把“请求到了 127.0.0.1”当成完全离线：P2 上线前要验证提供方的实际网络行为与断网运行，没通过时自动提取不可用，也不切云端模型救场。

## 9. 本地桥与运行状态

模型工具、Host 插件、管理面板共用 Rust 的查询与提交实现。复用既有内置插件回环桥的**监听生命周期、随机 Bearer 令牌与凭据文件下发**（env 与物化目录里的 0600 `.xlink-bridge.json`），但有三条既有事实要带上：

- 桥令牌只绑 `(family, instance)` 两维，**没有 profile、没有显式 launch generation**——§3.1 的命名空间要在签发与校验两侧同时扩展；
- 桥**停机期监听仍在**，撤销语义是“下次启动换一代令牌”；所以“停用即时生效”不能靠吊销令牌，只能靠运行中插件的失效确认（§3.1 第 5 条），未确认时标为处理中；
- 桥的文件头写着：能力面越小越好（调用方是模型能触达的代码），以及**它不是针对恶意插件的沙盒**（同进程插件本就能读内核环境变量、也能直读磁盘）。新增路由都要写清“谁能调、最坏能造成什么”。

**诊断落在壳这一侧**（2026-10-14 实测）：插件里的 `ctx.logger.info/warn` 在这个内核里一行都不进 stdout（同一个进程里 `dsh:` 自己的告警照常打印），所以“记忆到底有没有被调用过”在插件那边答不了。三条记忆路由各写一行 `shell_events`（逻辑名 `memory-bridge`，**只记路由与计数、不记正文**），它同时出现在「查看日志」面板里；插件里的 `ctx.logger` 调用保留，但不能当作唯一的诊断通道。

桥只开放有限路由：查询、候选提交（只能进待确认，不落库）、P2 的任务领取与结果提交。审批、编辑、停用、清空、导出仍走 Tauri 命令，模型桥碰不到。回环地址不等于授权：拒绝过期令牌、跨命名空间请求与任意路径参数。

状态至少区分 disabled、unsupported、ready、degraded；ready 必须在插件注册上下文与工具后报告，“资源已物化”只能叫 prepared。本次检索是否注入单独记录，不由启用位推导。失败日志记命名空间、任务 ID、来源游标、代次与错误码，不含记忆正文、模型输入或凭据（脱敏复用 `diagnostics::run::sanitize`）。壳桥暂时不可用时普通对话继续（`agent/created` 不得因此抛错），记忆工具返回明确的不可用原因；越权、删除、许可撤销与命名空间不匹配一律清空旧缓存，不退回上次的结果。

## 10. 分阶段交付与验收

| 阶段 | 交付内容 | 放行条件 |
| --- | --- | --- |
| P0 | 临时数据根内的存储选型与真实接缝验证 | **存储选型已定**（单文档 JSON，不引入 SQLite，见 §4.1）；第一请求能收到动态上下文、`agent/created` 失败可降级、取消后无提交、压缩后可恢复；不碰用户目录 |
| P1 | 人工保存、候选确认、关键词检索、上下文与工具、管理页、导出 | 跨会话合成验收通过；文案不称自动提取可用；注入次数与累计字节不超会话级预算 |
| P2 | 本地模型自动提取、游标、租约、取消与资源预算 | 断网生成、崩溃恢复与删除防再生验收通过，无云端回退 |

实施前置（P0 前落地）：新文件登记 `FILE_BUDGETS` 且 ≤800 行；生产代码总量现为 47944 / 预算 47955，**只剩 11 行**，`TOTAL_BUDGET` 必须同提交上调并写明理由；`builtins.rs` 登记预算 73 行，加 `xlink-memory` 条目要同提交调整；UI 产物预算 JS 700 KB / CSS 256 KB，新页面落地前实测余量；资源生成步骤挂进所有会跑 Rust 构建的工作流。

必须覆盖的行为测试：

- A 项目保存的记忆不进入 B 项目；**会话中途切目录后新目录的项目记忆不再注入、候选不记到旧项目**；无工作区、路径改名与大小写差异不扩大范围。
- 第一个真实请求包含记忆；`agent/pre-step` 拒绝或取消时不提交缓存；其他插件贡献仍在；关闭可选上下文时如实未注入。
- 重复步骤不追加相同快照；**20 回合会话注入 ≤3 次、累计 ≤36 KiB**；成功压缩后能重新取得记忆；失败压缩的判据含“只有 start、没有 end”。
- `agent/created` 内解析失败或桥不可用时，会话仍能正常创建与对话；子智能体不超出父级读权限；无 agent 的工具调用被拒绝。
- 删除后搜索、详情、摘要服务、导出入口与运行缓存都不返回该条；相同来源不能自动再生；保留会话历史这一边界显示正确；已导出文件不在清理承诺内。
- 外部文档里的“忽略指令”、伪造“用户要求”、路径穿越与注入变量都拿不到更高权限；超大输入、中文匹配、损坏文档、磁盘满、并发写入都返回明确状态，不丢权威记录、不影响普通对话。
- P2：断网仍能提取；本地模型不存在时不可用；停用与退出后无提取进程或迟到写入；两个写入进程、过期租约、迟到结果、旧 revision、重复 `turn/end` 不造成覆盖或重复候选；`flush` 返回 false 的用例按 live 历史处理。

测试必须持有 `scoped_xlink_home` / TestHome 到用例结束并只清自己的目录；macOS Intel 与 Windows 分别验收，不增加 Linux 发布。

## 11. 当前验证边界

这份提案已把 Codex 的静态机制与 dsh-xlink 的实际路径、物化约定和 dsh `0.2.1-alpha.2` 类型合同对齐，并按接入核查收紧了项目身份、注入预算与首版范围。**权威记录层、管理页、内置插件与桥都已落地**（见 §12），缺的是首请求/压缩/切目录这些真实运行验收——P0 的价值就是先验证这些接缝，再承诺 P1/P2 的用户体验。

## 12. 实施进度（2026-10-14）

面板长这样（真壳截图，2026-10-14 夜；见 [memory-panel.png](memory-panel.png)）：

![记忆面板](memory-panel.png)

已落地：

| 位置 | 内容 |
| --- | --- |
| `src-tauri/src/memory/store.rs` | 权威记录：命名空间解析（`default_instance_key` + `settings.profile`）、损坏即报错、schema 降级拒绝、CAS 更新、删除即记正文指纹、候选提交与批准 |
| `src-tauri/src/memory/query.rs` | 有界检索与注入文本：确定性打分（固定项 > 关键词 > 项目匹配 > 验证范围）、条数与字节预算、按 UTF-8 边界截断、关掉开关时返回 `enabled: false` |
| `src-tauri/src/memory/cmd.rs` | 十条面板命令（状态 / 列表 / 保存 / 读取 / 停用 / 删除 / 开关 / 批准候选 / 提交候选 / 预览注入 / 导出 / 打开导出目录）；**没有进 `commands.rs`**（反棘轮文件） |
| `src-tauri/src/memory/workspaces.rs` | 只读内核的 `<DSH_HOME>/storages/workspace.json`（面板下拉用）；`normalize_path` 把手填路径归成内核那种 realpath |
| `src-tauri/src/memory/input.rs` | 一条记忆/候选的**输入规范化**（去空白、限长、范围归一、关键词清理）：面板保存与模型提候选共用同一份判断 |
| `src-tauri/src/shell/paths.rs` | `instance_memory_dir` / `instance_memory_file`：`<instance_dir>/memory/<profile>/`，过 `validate_id_component` |
| `src-tauri/src/shell/error.rs` | `AppError::Memory` |
| `scripts/check-code-budget.mjs` | 新文件登记；`TOTAL_BUDGET` 47955 → 48950（记忆首块 919 行）；**并修正了计数器的假阴性**（注释里的 `/*` 会让整份文件被当成块注释吞掉，三份既有文件因此长期少算约 490 行，`ui/src/memory/memory.js` 更被算成 0 行） |
| `ui/src/memory/` | 管理页：列表 / 编辑 / 停用 / 删除确认 / 待确认候选 / 检索预览 / 总开关；`SideBar.vue`、`App.vue` 的 `PANELS` 与面板数据刷新三处接线 |
| `plugins/builtin/bridge.rs` | 回环桥上的三条记忆路由：`context`（有界检索）/ `read`（读一条并记一次显式使用）/ `propose`（只进待确认）+ 百分号解码（中文正文不编码就会以 `%E4%B8%AD…` 入库）+ 端到端测试（起真桥、真令牌、真 store） |
| `plugins/memory/` | 内置插件 `xlink-memory`：`agent/created` 里按 agent 作用域注册 `xlink.memory` 上下文贡献、`pre-step` 每回合取一次检索（会话级上限 3 次）、`memory_search` / `memory_read` / `memory_propose` 三个工具；失败一律清空缓存、绝不抛 |
| `builtins.rs` / `prepare-builtin-plugins.mjs` | 第三个内置插件的登记（`entries` 只有 host 与 package.json、`peers` 只有 `dsh-tools`），两份清单由 `dev-builtin.test.mjs` 钉死 |
| 单测 | Rust 30 条记忆单测 + 桥 7 条（含端到端）；插件 13 条纯函数测试（`test:plugin`，**已挂进 CI**） |
| 真实内核挂载验证 | 临时 `DSH_HOME` + 软链 profile 依赖启动 `dsh web`：`--dump-config` 的合成树里能看到 `xlink-memory` 行（绝对路径被规范成 `file://…`），插件 `apply()` 确实执行（标记文件），`inject` 的三个服务全部解析——**“插件级 logger 不可见”这条也是这次实测出来的**。工作区对齐那一轮又验了一次：`ctx.inject(['workspaceRegistry'])` 的回调在 4 秒内拿到服务（标记里 `registryAtApply: ready`），插件照常加载 |
| 工作区对齐（2026-10-14 晚） | `memory/workspaces.rs` **只读**内核的 `<DSH_HOME>/storages/workspace.json`；面板的「本项目」变成下拉（已知工作区 + 手填兜底），保存时带上真实的 `workspace_id`；插件经 `ctx.inject(['workspaceRegistry'])`（**按需注入，不进 `inject` 数组**）解析 id 随 `id` 参数送桥。`query::scope_matches` 因此是「先 id、再路径」：项目改名/换盘后仍能对上 |
| 面板的注入插件告警 | 开关打开而 `xlink-memory` 未启用时，面板直说「保存的记忆不会进入任何会话」并给一个启用按钮——这是两个开关，少一个整条链路不生效 |
| Markdown 导出（2026-10-14 夜） | `memory/export.rs` + 两条命令（`memory_export` / `memory_open_exports`）：一次性快照落到 `<memory>/exports/memory-<时间戳>.md`（0600、**不覆盖已有文件**、同秒加后缀）。导出的是权威记录那份列表视图（含停用项与候选），**文档读不出来时在文件头照实说**——一份看起来正常的空快照会让用户以为记忆丢了 |
| **真实运行链路验收（2026-10-14 夜）** | 临时 `DSH_HOME` + `dsh headless` 会话，profile 里挂我们的插件，桥与模型都换成本地桩（**零模型额度**）：① 桩桥收到 `GET /v1/memory/context?q=<用户消息原文>&workspace=<有效工作目录>`，带 Bearer 令牌，中文百分号编码正确；② 桩模型收到的请求体里**确实带着注入的记忆正文**（落在 user 角色的运行上下文块里）；③ 同一轮里第二次 LLM 调用（会话标题）**没有**记忆、也**没有**再取一次检索——预算是按会话记的。这条链把「用户消息 → pre-step → 桥 → 缓存 → 上下文贡献 → 模型请求」整条打通了。**又用壳体真正写出的形状跑了一遍**：插件放在物化布局 `extensions/builtin/memory/0.1.0/fp-1/compat-a/`、peer 用 node_modules 软链、接线行是相对 profile 目录的 `../../extensions/...`（`--dump-config` 里解析成对应的 `file://` 路径）——结果同上。踩过的坑记一笔：`dsh headless` 用的是它自己的 `headless` profile（不是 `web`），把接线行写进 `profiles/web/` 会得到一次「什么都没发生但退出码 0」的运行 |
| 启用路径的真壳验证（2026-10-14 夜） | 在真 app 里调 `builtin_set_enabled({plugin:'memory', enabled:true})`：`loadState` 从 `disabled` 变 `prepared`、`wired: true`、`missingPeers: []`；磁盘上物化出 `host/` + `package.json`(0600) + `node_modules/@deepseek-ai/dsh-tools` + `.xlink-materialized.json`，接线行写进 `profiles/web/cordis.patch.yml`（`../../extensions/builtin/memory/0.1.0/<指纹>/compat-a/host/index.js`，正是前面验过能跑的形状），`state.json` 的 `dev/web.requestedEnabled` 变 true。**验完立刻关回去**：接线行被删、状态回 false——用户环境不留痕 |
| 真壳冒烟（2026-10-14 夜） | `e2e/specs/memory.spec.mjs`（3 条，`npm run e2e` 会拉起真 app）：① 侧栏里有「记忆」项、点它会被标成当前项；② `memory_status` / `memory_list` / `memory_workspaces` 在真壳里都能解析（桥真的通，不是「面板画出来了」）；③ 打开记忆页没有往控制台扔错误。**边界如实写进用例**：面板内容的渲染在这条通道里验不了——WebDriver 拉起的窗口在后台，WKWebView 不跑 rAF，而 `App.vue` 的面板切换是 `<Transition mode="out-in">`，leave 永不结束、新面板不挂载；**每个面板都一样**（设置 / 插件实测同样停在 `leave-active`），禁用过渡、手动派发 `transitionend`、把窗口提到前台三种办法都试过，都不解决 |
| 按 id 读的归属校验（2026-10-14 夜） | 桥的 `read` 路由补上项目这道门：**先查、再判、最后计数**。id 猜不到，但会话历史里留着旧 id——不校验就能在 B 项目里把 A 项目的记忆读出来，而设计承诺的是「A 项目保存的记忆不进入 B 项目」。顺序反过来时被拒的读还会把「已用 N 次」加上去，统计被越权尝试污染（这条是写完测试才发现的：断言 `use_count == 1` 时拿到 3）。调用方不给坐标时项目记录一律不通过——没有坐标就没有访问权 |
| 启用路径与路径规范化（2026-10-14 夜） | ① 物化自动判据：只带一个 `host` 目录、没有上游内容的内置插件也能物化，且**必需的 peer 真的被挂上**（`materialize` 里新增一条，钉的是「启用之后内核能不能 import 它」）；② 手填项目路径在**写入口**规范化成内核那种 `realpath`（符号链接、结尾斜杠、Windows 的 `\\?\` 前缀剥离；纯函数全平台可测，因为 `#[cfg(windows)]` 分支本机编译不到），并有一条钉**调用点**的用例——helper 有测试而接线没有，是这个仓库反复栽过的形状 |
| **跨模块边界复查（2026-10-14 复核）** | 一次评审把「设计承诺过但接线没跟上」的六条找出来并补上：① **停用挡不住按 id 读**——注入表头是 `[id] 标题`，id 一直留在会话历史里，而桥的 `read` 只过项目归属那一道门；`store::find_by_id(.., false)` 现在把状态一起挡（面板自己看已停用的那条走另一条命令，不受影响）。② **切换工作目录不清缓存**——`policy::retarget` 在目录变了时清空缓存**并归零注入预算**：只清文本会让预算耗尽后的会话永远检索不到，不清文本则 A 项目的记忆一路注入 B。③ **长消息静默失效**——桥的请求头缓冲是固定 8 KB，而 `q` 装的是用户消息全文，中文编码后 3 倍膨胀：实测 1000 字即越界，越界后桥**一个字节的响应都不写**就关连接，插件只看到 `socket hang up`。`userText` 按 500 字截尾，`callMemory` 另有一道 `MAX_PATH_BYTES` 硬闸，把「太长」说成「太长」而不是「桥不可用」。④ **候选无上限**——`memory_propose` 是模型可触达的写入路径，而裁剪只删已决候选；现在待确认到 `MAX_PENDING_CANDIDATES` 就拒收，并拒收正文命中删除指纹的提交（设计 §7.3）。⑤ **`list()` 把同一份文件读两遍**，面板刷新与 `memory_read` 每次多解析一遍整份 JSON，改为 `status_from(doc)`。⑥ **`title` 没有长度上限**——正文限了标题不限，标题一长注入表头就把正文预算饱和到 0、整条被跳过。 |
| **输入规范化的共享层** | `memory/input.rs`：面板保存与模型提候选原先**各抄一份**「什么算一条合法的记忆」（去空白、限长、范围归一、关键词清理），改一边漏一边的症状是「面板存得进去、模型存不进去」。抽出来之后 `store.rs` 净减，665 行的反棘轮预算不动——**是靠删重复过的，不是抬数字**。顺带修掉一个真 bug：候选原先不要求工作区坐标，于是收下一条**谁都检索不到**的记录（`scope_matches` 两项都空一律不通过），白躺在「待确认」里。 |
| **只在 Windows 上成立的测试** | `a_hand_typed_path_is_normalized_at_the_write_entry` 与 `workspaces::normalize_path_resolves_symlinks_and_trailing_separators` 两条都把 `fs::canonicalize` 的原始结果当期望值，而 Windows 上它带 `\\?\` 前缀、`normalize_path` 会剥掉——**在 Windows 上跑必然红**。它们一直绿着是因为 Windows CI 那个 job 只跑 `cargo check --all-targets`、从不执行测试（`desktop-ci.yml` 的注释写明「不跑测试」）。两处的期望值都改成走同一个规范化函数。 |

IPC 与落盘都用 **snake_case**：`check-invariants` 的 `ipc-fields` 抓到过一版 camelCase 序列化——`total_bytes` 与既有前端读的同名字段相撞，会被判成「前端读到 undefined」。

尚未落地（按顺序）：
1. **真实模型下的行为验收**：链路已经端到端打通（见上表，桩模型），还没验的是「真模型拿到这些记忆之后会不会照着做」——那是一次真实会话的事（会用到模型额度），也是唯一需要它的部分。
2. 会话里「本次用了哪几条记忆」的回执（P2）：现在只有壳侧 `memory-bridge` 调用留痕，用户在工作台看不到。
3. 自动提取（P2）：本地模型验收通过前，面板显示「不可用及原因」，不用同一个「已开启」掩盖缺失能力。
4. 真实模型下的第一次使用：启用 → 重启工作台 → 在会话里问一句相关的问题（链路已验到模型请求，这一步验的是模型会不会照着做）。
