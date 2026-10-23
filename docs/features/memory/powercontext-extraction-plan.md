---
description: "从 PowerContext 提取可复用能力（交接、经验、审核后复用、修订历史）的开发计划：取什么、不取什么、为什么，以及在 dsh-xlink 既有记忆模型里的落点。"
kind: "development-plan"
status: "实施中，待完成实跑验收"
date: 2026-10-21
---

# 从 PowerContext 提取可复用能力：开发计划

依据：[PowerContext 调研结论](#1-调研结论不把-powercontext-装成插件)与本文档。用户于 2026-10-23 接受[架构决策记录](adr-20261023-handoff-experience.md)，本轮已开始实施。实际验证边界见 [memory-progress.md](memory-progress.md)。

本文件回答「从 PowerContext 取什么、怎么在 dsh-xlink 既有记忆模型里长出来」；
[dsh-xlink-memory-design.md](dsh-xlink-memory-design.md) 回答「为什么这么设计」，
[memory-progress.md](memory-progress.md) 回答「现在做到哪了」——**实现时三份都要动**。

---

## 1. 调研结论：不把 PowerContext 装成插件

2026-10-21 对 `oceanbase/powercontext`（OceanBase 出品，Apache-2.0，Python，主分支 `master`）做了只读调研。
**结论：它与 dsh-xlink 的记忆系统是竞品，不是互补；作为插件装进来的代价高于收益。**
但它有**两样 dsh-xlink 确实缺的东西**——本文要取的就是那两样。

### 1.1 为什么不能作为插件装进来

四条硬事实，每条都独立成立：

| # | 事实 | 出处 |
| --- | --- | --- |
| 1 | **插件没有发布到 npm**：`npm view powercontext-dsh` 返回 404。它只随 git 仓库分发，位于 `integrations/dsh/plugins/powercontext` 子目录。 | `npm view` 404（2026-10-21 实测） |
| 2 | **dsh-xlink 的 git 安装路径装不了它**：`pkg::fetch::shallow_clone` 克隆的是**仓库根**，`plugins::center::read_plugin_manifest` 读的也是**仓库根的 `package.json`**；而 PowerContext 仓库根是 Python 项目（`pyproject.toml`，无 `package.json`），插件在子目录里。clone 的参数只有 `--depth` / `--progress` / `--branch`，**不支持指定子目录**。 | `pkg/fetch.rs:266-283`、`plugins/center.rs:1554-1558` |
| 3 | **它需要一个壳管不了的常驻外部服务**：PowerContext 是「Server + 插件」架构（Python 3.11+ / uv / 独立进程）。dsh-xlink 侧**全仓没有 sidecar 进程监管能力**（`grep sidecar|常驻|supervis` 零命中），只有内核进程的生死管理（`kernel/lifecycle.rs`）。装上就多一个**没人负责它生死**的进程。 | `grep -r 'sidecar\|常驻\|supervis' src-tauri/src` 零命中 |
| 4 | **注入会与 `xlink.memory` 重复**：PowerContext 插件挂 `agent/pre-step`，把召回内容作为 user 消息追加；`xlink-memory` 挂 `agent/created` 注册 `xlink.memory` 上下文（order 300），在 `agent/pre-step` 里更新缓存。**两者同时开会各自注入一份记忆**，模型看到两套来源、两套预算、语义重复的回忆。而 PowerContext 那份默认 `capturePrompts: true`，会把用户提示词回传给 Server。 | `integrations/dsh/plugins/powercontext/src/index.ts`（`registerRecall`）、`src/recall.ts`（`runRecallPreStep`）、`src/config.ts`（`capturePrompts` 默认 `true`）；对照 `plugins/memory/host/index.js:569` |

### 1.2 版本代差（即便装得上也不该装）

- PowerContext 的 dsh 集成**真机验收钉在 DSH `0.1.2-rc.1`**（`docs/en/docs/integrations/dsh.md`： "The real-host acceptance suite pins DSH 0.1.2-rc.1"），集成状态标为 `community`。
- dsh-xlink 两个壳跑的是 **`0.2.1-rc.2`**（release，`active.txt`）与 **`0.2.1-alpha.2`**（dev）。
- 它的注入消息用 `createUserMessage({ source: { kind: 'plugin', ... } })`。在 `0.2.1-rc.2` 的 `dsh-llm` 里，`MessageSourceMap` 只有 `user` / `model` / `tool` / `system-prompt` 四个 kind，类型注释明写 **"there is no shared catch-all `plugin` kind"**（`dsh-llm/lib/types/message.d.ts:96-108`）。运行时 `createMessage` 不校验（只 `deepFreeze`），**所以不会崩**，但这条消息的 source 归属落不进内核的类型体系——下游按 kind 分派的消费者会走「未知 kind」分支。

### 1.3 取什么

PowerContext 的四个 Artifact 家族里，两个 dsh-xlink 缺、两个不缺：

| PowerContext 家族 | dsh-xlink 有没有 | 取不取 |
| --- | --- | --- |
| **Memory**（决定、约束、事实、状态、下一步） | **有**，而且更完整：`src-tauri/src/memory/`（Rust 唯一写入方）、三块数据模型、有界关键词检索、来源列表、冲突/取代标记、抑制指纹、导出、面板 | **不取** |
| **Skill**（受管流程，审核后才导出） | 有同构物（`skills/` 中央库 + 物化 + 接线），语义不同但不是缺口 | **不取** |
| **Handoff**（工作交接：活儿的当前状态交给下一个人） | **没有**。dsh-xlink 的记忆存的是「结论与约束」，没有「这件活的当前状态」这个概念 | **取** |
| **Experience**（情境→动作→结果→教训，审核后才进召回） | **没有**。dsh-xlink 的记忆是「一条可复用的知识」，没有「一次做法的成败记录」这个概念 | **取** |

另外两样**机制**值得一并借鉴（它们是上面两个家族的配套设施，不是独立功能）：

- **审核后才进召回**（Review Inbox → 审批 → 不可变 Revision → 只有被批准的当前 head 参与召回）。
  dsh-xlink 已有**同构物**：待确认候选 → 用户批准 → 落成记忆（`memory/candidates.rs` 那条路）。**不引入第二套审核流**，只补「经验」这一族。
- **有界召回**（PreparedContext：选定的段落 + 共享字节预算 → `ready` 或 `empty`）。
  dsh-xlink **已有等价物**（`memory/query.rs` 的 `MAX_HITS=6` / `CONTEXT_TOTAL_BYTES=12 KiB`）。**不引入第二套预算**。

### 1.4 明确不取的（及理由）

| 不取 | 理由 |
| --- | --- |
| PowerContext 的 Server 架构 | 引入一条装不上的安装路径、一套版本代差、一个管不了的外部进程。dsh-xlink 的边界是「本地、无服务端」。 |
| `mcp__powercontext__*` 工具面 | dsh-xlink 的模型面已经定型（`memory_search` / `memory_read` / `memory_propose` 三个工具 + 桥路由）。再开一套 MCP 工具面是**第二个入口**，而召回只有一个出口才是对的。 |
| 修订历史（`FAMILY/ARTIFACT_ID@REVISION` 寻址） | dsh-xlink 的 `revision` 是 CAS 用的单调版本号，不是可寻址的历史快照。给记忆做完整修订历史是独立的一等需求，本计划不含它。 |
| Topic Memory（后台从 Source 抽主题） | 与已落地的 P2 自动提取重叠，且依赖「捕获全部提示词」，与 dsh-xlink「只收长期可复用内容」的内容闸门方向相反。 |
| 跨 agent / 跨人的共享记忆 | dsh-xlink 的两个壳按设计**刻意不共享**记忆（`AGENTS.md` 范围节）。PowerContext 的核心卖点正是跨 host 共享——这条与本项目的既有决定直接冲突。 |

---

## 2. 取什么：Handoff 与 Experience

### 2.1 Handoff（工作交接）

**要解决的问题**：dsh-xlink 的记忆存「结论与约束」。「这个仓库在用 pnpm」是记忆；**「P4 协议 v0.11 已封 G4-13/14，剩 9d 转真判据与判据第 1/3/4/5/7/10/11 条落地」是交接**——它描述的是**这件活的当前状态**，会随工作推进而变，不是可长期复用的知识。

PowerContext 的四步循环：`Work Contract → Prepared Handoff → Acknowledgement → Task Outcome`。
**首版只取前两步**，后两步是多人协作形态（接收方要确认、任务要有最终结论），dsh-xlink 是单人单实例，**没有接收方**：

| 步 | 含义 | dsh-xlink 取舍 |
| --- | --- | --- |
| **Work Contract** | 这次活儿的契约：目标、范围、验收 | **取**。它是「接下来做什么」的锚。 |
| **Prepared Handoff** | 捕获一个边界处的状态：做完了什么、还剩什么、卡在哪 | **取**。核心内容。 |
| Acknowledgement | 接收方确认接手 | **不取**（无接收方） |
| Task Outcome | 任务的最终状态与检查项 | **不取**（同一理由） |

**落点**：`memory.json` 的**第三块数据**（现为 `memories` / `candidates` / `meta` 三块，见 `memory/store.rs:303-310`），新增 `handoffs` 段。

**与记忆的分工**（这是本族最要紧的一条边界）：

| | 记忆 | 交接 |
| --- | --- | --- |
| 回答的问题 | 「以后遇到这类事该怎么做」 | 「这件活现在到哪了」 |
| 时效 | 长期有效，被验证过 | **会过期**——活干完就该归档 |
| 谁写 | 用户 / 模型提取 / 面板 | **用户显式发起**（模型不自动创建交接） |
| 注入时机 | 每回合按关键词检索 | **会话开始时一次**（见 2.3） |
| 检索方式 | 关键词打分 | **不检索**——取「当前这件活」的那一条 |

**「模型不自动创建交接」是硬规则**：交接是**意图**（「我要把这活交给别人」），不是从对话里能提炼出的知识。自动提取那套（`memory/host/run.js`）不碰交接——否则每回合结束都产生一条交接，交接库立刻变噪声。

**与候选（candidates）的关系**：交接**不走待确认**。用户显式发起的交接是他自己的决定，不需要第二个人批准；但它**可以被模型读**（通过下面的工具），而模型**不能写**（与 `memory_propose` 的方向相反——那个是模型提议、用户批准）。

### 2.2 Experience（经验）

**要解决的问题**：dsh-xlink 的记忆是「一条可复用的知识」。「这个仓库在用 pnpm」是记忆；**「在这个仓库里改插件源码后必须先 `npm run prep:builtin` 再重启壳，否则跑的是旧物化产物」是经验**——它有**情境**（改插件源码）、**动作**（prep:builtin + 重启）、**结果**（否则跑旧产物）、**教训**（AGENTS.md 里那条纪律）。

PowerContext 的 Experience 结构：`situation → action → observed outcome → reusable lesson`，
且**进 Review Inbox，经审批才参与召回**。

**与记忆的分工**：

| | 记忆 | 经验 |
| --- | --- | --- |
| 断言 | 「X 是这样」 | 「**在情境 S 下做动作 A，得到了结果 O**，所以下次该这样」 |
| 成立条件 | 无条件（或按项目范围） | **有前提**（情境）——前提不成立时它**不适用** |
| 来源 | 人话、提取 | **一次做法的成败记录**（通常来自某次具体任务） |
| 生效条件 | 批准即可用 | **审核后才进召回**（与 PowerContext 一致） |

**「有前提」是这一族存在的全部理由**。dsh-xlink 现有的记忆没有前提字段——一条「本仓库用 pnpm」在别的项目里靠 `scope` 隔离，而一条经验的前提（情境）**往往跨项目成立**（「跨进程并发要用文件锁」在哪个仓库都对）。所以经验**默认是通用偏好**，项目特有的部分写进情境里。

**落点**：**独立数据段还是记忆的一个子类型？** ——**独立段 `experiences`**。理由三条：

1. **审核流不同**：经验要「审核后才进召回」，记忆是「批准即生效」，两者的状态机不同，塞进一张表会让 `state` 的取值含义漂移。
2. **字段不同**：经验要多存 `situation` / `action` / `outcome` / `lesson` 四个字段，记忆没有。多出来的字段在记忆表里永远是空的。
3. **召回不同**：经验按情境匹配，记忆按关键词打分。**两套打分不混在一张表里**（与 C6「pending 不进 text」同源纪律：不同口径不合成一段）。

**不引入第二套审核流**：经验复用现有的「待确认 → 批准」那条路（`memory/cmd.rs` 的 `memory_decide_candidate` 一族），只是待确认队列里多一种**类型**。这是本计划里最重要的一条「不新增」。

### 2.3 注入：交接不参与每回合检索

**交接是「当前这件活」，不是「相关知识」。** 每回合按关键词检索交接，会在一堆活儿里捞出最像的那条——而用户要的是**手上这件**。

落法：**会话开始时取一次当前交接**，之后只按「交接库变化」刷新（复用现有的 `MAX_LIBRARY_REFRESHES_PER_SESSION` 机制，不新增计数闸）。

**预算**：交接**单独计**一份字节预算，不占 `CONTEXT_TOTAL_BYTES`（12 KiB，那份是给记忆检索结果的）。理由：记忆是**按需**的（关键词命中才给），交接是**确定要给**的（每回合都该知道手上这件活到哪了）——两者性质不同，混在一个闸里会导致「记忆命中太多时交接被挤掉」，而那正是最不该发生的一种挤掉。

**给模型的措辞**：交接是**当前状态**，不是指令。模板必须写明「这是这件活的进度快照，可能已过期；与本次用户要求冲突时以用户要求为准」——与记忆模板同一条纪律（`memory/query.rs::render_context` 的固定开场白）。

---

## 3. 数据模型

**形态**：沿用**单文档 JSON**，不引 SQLite（§4.1 已定案：免 C 工具链、免 `rusqlite` 的 `bundled`、单写者一把锁够用）。**不新起一份文件**——`memory.json` 已经是最小可工作形态，而交接与经验都随它一起 CAS 写入。

### 3.1 权威记录的新增段

```jsonc
{
  "meta": { ... },              // 现有一块，不动
  "memories": [ ... ],          // 现有一块，不动
  "candidates": [ ... ],        // 现有一块，多一个 type 字段（见 3.3）
  "handoffs": [ ... ],          // 新增
  "experiences": [ ... ]       // 新增
}
```

**schema 抬版**：`SCHEMA_VERSION` 5 → 6。**这是必抬的**——新段落是字段级变更，而本仓的规则是「库比代码新就拒绝写入」（`store.rs:467`），不抬版会让旧版本把新段落当未知字段丢掉、再把自己的那份写回去。

**抬版必须开窗口**：按 C6 那条「三条字段级 schema 变更攒在一起做」的纪律，**5 → 6 这一批只做本计划新增的字段**；若同期还有别的字段级变更要抬，**一起做**（下一次抬版的边际成本已经在读回兼容那一套里铺好了）。

**读回兼容**：新段落用 `#[serde(default)]`（`MemoryDoc` 已经是 `#[serde(default)]` 整体兜底，新增 `Vec` 段落自动兼容 v5 库）；`candidates` 的新 `type` 字段同样 `#[serde(default)]`，旧候选读出来是 `""` ——**空值按「记忆类候选」处理**，不留第三种含义。

### 3.2 `handoffs` 段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | 短串，同 `memories` 的 id 形状 |
| `workspace_id` / `workspace_path` | string | 同 `memories`：项目坐标。**交接必须绑项目**（它是「这件活」，活儿有归属） |
| `title` | string | ≤512 B，同现有上限 |
| `body` | string | ≤8 KiB，同现有上限。交接正文 |
| `goal` | string | Work Contract 那一半：这件活要达成什么。≤2 KiB |
| `done` | `Vec<string>` | 做完了什么。每条 ≤512 B，≤32 条 |
| `todo` | `Vec<string>` | 还剩什么。同上 |
| `blocked` | `Vec<string>` | 卡在哪。同上。**空数组是正常形状**（大部分交接不卡） |
| `state` | string | `active` / `archived`。**只有一条 `active` 交接**（同一项目下）——「当前这件活」是唯一的 |
| `revision` | u64 | CAS 用，同 `memories` |
| `created_at` / `updated_at` | string | 同 `memories` |
| `archived_at` | string | 归档时间。`active` 时为空 |

**「同一项目只有一条 active」是硬约束**，与「同一件活」这个概念一致。两处要落地：写入时把旧的 `active` 置 `archived`（同一事务内）；读取时**不按关键词检索**，按项目坐标取 `active` 那条。

**归档不等于删除**：归档的交接留在库里可查（导出、面板），但**不再注入**。它与「被删除的记忆」不同——删除是不可恢复且带抑制指纹的；归档是「这件事做完了，但记录留着」。

### 3.3 `experiences` 段与候选的 `type`

**`experiences` 段**：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | |
| `title` | string | ≤512 B |
| `situation` | string | 情境。≤2 KiB。**这族的定义性字段** |
| `action` | string | 做法。≤2 KiB |
| `outcome` | string | 观察到的结果。≤2 KiB。**允许「失败了」**——失败的经验同样值钱（它防的是重复踩坑） |
| `lesson` | string | 可复用的教训。≤2 KiB。**这一条是进召回的那部分** |
| `scope` / `workspace_id` / `workspace_path` | string | 同 `memories`。**默认 `general`**（见 2.2：情境常常跨项目成立） |
| `keywords` | `Vec<String>` | 同 `memories` |
| `sources` | `Vec<MemorySource>` | 来源列表。**复用 `memory/sources.rs` 的形状**（旧单对象与新数组在读侧归一那一套） |
| `verification` | string | `verified` / `unverified` / `stale`，**复用现有三档** |
| `state` | string | `accepted` / `disabled` |
| `revision` | u64 | CAS |
| `created_at` / `updated_at` / `last_used_at` / `use_count` / `recall_count` | | 与 `memories` 同名字段同义 |

**候选的 `type` 字段**：`MemoryCandidate` 增一个 `kind: String`（默认 `""` = 记忆类候选）。取值 `memory` / `experience`。**经验类候选的额外字段**（`situation` / `action` / `outcome` / `lesson`）放在候选里**同一层**还是**嵌套一个对象**？

→ **平铺**（与 `MemoryCandidate` 现有形状一致：所有可选字段都平铺）。代价是记忆类候选那四个字段恒空——与 3.1 的判据同源：**空字段不引入第二套含义**，`kind == ""` 时它们被忽略。

**为什么不复用 `body` 装四段**：正文与 `situation` 是两回事——`body` 是「这条候选长什么样」的通用描述，而经验需要**结构化**的前置条件。塞进 `body` 就没法按情境匹配。

---

## 4. 检索与注入

### 4.1 交接：按项目取，不检索

- **取法**：`scope_matches` 判项目（**复用 `query.rs` 现有的那道门**，不新写一套），然后取该项目的 `active` 交接。
- **没有命中就不发任何文本**（与 `B14` 同纪律：没命中就不发开场白——那段开场白本身没有信息量，而运行侧会把非空文本当成一次注入）。
- **预算**：单独一份字节上限（建议 4 KiB），**不占** `CONTEXT_TOTAL_BYTES`（12 KiB）与 `MAX_TOTAL_BYTES_PER_SESSION`（36 KiB）。理由见 2.3。
- **刷新**：会话开始取一次；之后只在**交接库变化**时刷新，复用 `MAX_LIBRARY_REFRESHES_PER_SESSION`（2 次）那套计数，不新增闸。
- **换工作目录**：交接**跟着项目走**，换目录就是换一件活——**清空缓存**（与 `B6` 同纪律），但**不重置注入计数**（与 `B7` 同纪律：换目录不该是绕开预算的开关）。

### 4.2 经验：按情境匹配，与记忆分开

经验**不进** `memory/query.rs` 的关键词检索结果。理由：两套打分（关键词 vs 情境）混在一个排序里，会让「情境完全不匹配但关键词很像」的经验压过记忆。

**首版的经验召回怎么做**（诚实的边界）：**纯关键词 + 情境文本的短语匹配**。`situation` 里的词与用户输入做短语匹配——**与记忆的 `query::score` 同一套机制**（`B22` 已有「中文按明确关键词与短语匹配」）。

**这就是首版的真实能力边界**：经验的情境匹配是**词面的**，不是语义的。一条情境写「跨进程并发」而用户说「两个壳同时写」——**匹配不上**。

> **不做的**：向量检索。§6.2 已定案「首版不做向量检索」，且它需要模型与嵌入设施。经验族不改变这个决定——**它只是让「前提」这件事在数据模型里显式存在**，为将来换匹配方式留位置。

**注入位置**：经验与记忆**同一次注入**（同一段上下文、同一个 `xlink.memory` 贡献），但**渲染成分开的段落**。理由：两者的可信度不同（记忆是结论，经验是「在某情境下这么做过」）——混在一段里模型分不清哪句是规则、哪句是案例。与 C6「pending 与既有记忆合成一段会让模型混用 id」同源。

**单次注入的条数**：记忆 ≤6 条（`MAX_HITS`）；经验**另计** ≤3 条。两条上限**各是各的**（理由与 `marks.rs` 里两个标记的限额各是各的一样）。

---

## 5. 模型工具面

现有三个工具（`memory_search` / `memory_read` / `memory_propose`）覆盖不了新读面。新增第四个工具 `memory_context`，保留既有三个工具的语义。

→ **新增一个工具 `memory_context`**，一次给全：

```
memory_context({ kind: "handoff" | "experience" | "all", query?: "用于匹配经验的情境短语" })
```

返回：**当前交接**（若有）+ **匹配的经验**。

**为什么不加进 `memory_search`**：`memory_search` 的语义是「按关键词找记忆」，返回值有 `hits` 与 `id`，模型的用法是「找到 id 再 `memory_read`」。交接与经验的用法不同——**模型要的是「手上这件活」和「这类情况的经验」**，不是一串要再点开的 id。**两套用法不该挤在一个工具的参数里。**

**写入面**：模型**不能写**交接与经验。理由：

- **交接**：写它 = 表达「我要把这活交给别人」的意图，那只能是用户的意图。
- **经验**：经验来自「一次做法的成败记录」，而**成败由结果判定，不由模型自称**。模型说「我试了，成功了」不构成经验——那正是候选机制存在的理由。

→ **经验走候选**：`memory_propose` 扩一个 `kind: "experience"` 参数（默认 `memory`），壳那侧按类型把四个字段写进候选。**候选是模型的写入面，批准是用户的**——与 C3「模型不改权威记录」同纪律。

---

## 6. 管理面板

**不新开面板页。** 记忆面板已有四档视图（全部 / 按项目 / 通用偏好 / 待确认），交接与经验**各占一档**：

| 档 | 内容 |
| --- | --- |
| 交接 | 当前项目的 `active` 交接 + 归档列表 |
| 经验 | 经验列表（与记忆列表同形状：精简卡片 + 点开详情） |

**理由**：记忆面板的形态（`MemoryRow.vue` 精简卡片 + `MemoryDetail.vue` 详情弹窗 + `MemoryCandidateDetail.vue` 候选详情）已经是三套组件的复用面。**经验与记忆的记录形状几乎一样**（`MemoryRow` / `MemoryDetail` 直接复用），**交接不一样**（多三个 `done` / `todo` / `blocked` 列表）——所以交接要一个新组件 `MemoryHandoffDetail.vue`，经验**不加组件**。

**待确认队列**：经验类候选在**同一档**里，行上加一枚类型徽章（与 `MemoryCandidateRow.vue` 的关系标记同款）。

**面板的动作**：

| 动作 | 位置 | 约束 |
| --- | --- | --- |
| 新建交接 | 交接档 | 只有用户能发起（面板是用户入口） |
| 归档交接 | 交接详情 | 归档不是删除（§3.2） |
| 编辑 / 停用 / 删除经验 | 经验详情 | 复用 `memory_set_state` / `memory_delete` 一族 |
| 批准 / 忽略经验候选 | 待确认 | 复用 `memory_decide_candidate` |

**导出**：`memory/export.rs` 的 Markdown 快照要**带上交接与经验**——导出是「一次性快照」，漏掉它就比事实少一件事（与 D9 那条纪律同源）。

---

## 7. 实施阶段

| 阶段 | 产出 | 前置条件 | 完成证据 | 状态 |
| --- | --- | --- | --- | --- |
| **P0 数据模型** | schema 6、交接 / 经验两段、候选类型、旧库兼容 | 已确认升级窗口 | `concurrent_handoffs_have_one_active_head_and_old_schema_round_trips` | 已实现，定向测试通过 |
| **P1 交接** | 用户保存 / 编辑 / 归档 / 删除、项目活动交接唯一性、面板、导出 | P0 | 替换归档与并发唯一性测试、导出断言 | 已实现 |
| **P2 交接注入** | 同一上下文贡献，首次取得及库变化刷新、独立 4 KiB、换目录清空 | P1 | 装配层验证首轮、变化段落、刷新次数与换目录；真内核 `0.2.1-rc.3` 两轮请求验证首轮及未变化交接不重复 | 已实现，真内核换目录待验 |
| **P3 经验** | 四段结构、候选审核、情境匹配、面板、导出 | P0 | 未审核不召回、拒绝不入库、项目隔离、删除防再生测试 | 已实现 |
| **P4 工具面** | `memory_context` 与 `memory_propose(kind=experience)` | P1、P3 | 四工具实际调用测试、无会话拒绝、真桥权限用例 | 已实现 |
| **P5 验收** | 定向门禁与隔离真壳；发布前再跑全量门禁 | P2、P4 | 见进度表本轮验收记录 | 进行中 |

**P0 不能跳过、也不能与 P1 合并**：抬版本是**一次性**的机会（同 C6 那条纪律），窗口开了要做的事只有这几件，多塞无关字段会让「用户看不出差别」这个理由不成立。

**默认按顺序推进。** P1 与 P3 在 P0 之后可以并行（不同文件），P2 依赖 P1、P4 依赖 P3。

---

## 8. 验收判据

**必须覆盖**（沿用 §10 的口径）：

- **交接**：同一项目写第二条交接时第一条被归档；不同项目各有各的 active；归档的交接不再注入；换工作目录后不注入上一件活的交接；**同一项目并发写两条时只有一条是 active**（跨进程锁 `memory/lock.rs` 已在，这条要验它真的覆盖新写入路径）。
- **经验**：情境匹配命中、情境不匹配不命中（**「不瞎标」那一半要能造出来**——与 C5 的三个靶子同纪律，只有一个靶子时可能蒙对）；经验候选批准后进召回、被拒后不进；**未批准的经验不参与召回**；经验与记忆的命中不混在同一段里。
- **读回兼容**：v5 库读得出来、写得回去（写回去时不丢 `handoffs` / `experiences`）；`kind` 缺省的旧候选按记忆类处理。
- **注入预算**：交接的字节**不占**记忆那 12 KiB / 36 KiB；换目录**不重置**注入计数。
- **模型面**：`memory_context` 读得到；**模型没有任何写交接或写经验的入口**（判据要扫源码里那几条写入路径——与「插件里只许有一处 `prepareCall(`」同款纪律）。
- **不越权**：A 项目的交接与经验不进 B 项目（子智能体按工作区判等，`G15` 那三道夹紧对新读面同样成立）。

**每条判据都要有反向验**（改坏 → 具名断言变红 → 恢复）。**这是本仓反复栽的形状**：helper 有测试而接线没有、判据在判源码字面而值在两端之间断了（C5/C6/H7 各栽过一次）。

---

## 9. 落点（建议）

| 位置 | 职责 | 约束 |
| --- | --- | --- |
| `src-tauri/src/memory/handoff.rs`（新增） | 交接的落盘事务、active 约束、归档 | 与 `store.rs` 同一把跨进程锁；**不放进 `store.rs`**（它已到预算线，且交接是独立职责） |
| `src-tauri/src/memory/experience.rs`（新增） | 经验的落盘事务、情境匹配 | 匹配复用 `query::score` 的短语匹配，**不另写一套** |
| `src-tauri/src/memory/store.rs` | `MemoryDoc` 增两段、`SCHEMA_VERSION` 抬版、候选 `kind` | 抬版与候选字段在**同一提交** |
| `src-tauri/src/memory/query.rs` | 经验的命中与渲染段落 | **不把经验混进 `sorted_hits`** |
| `src-tauri/src/memory/cmd.rs` | 面板命令 | 复用既有命令族，**不新开一批同形命令** |
| `src-tauri/src/memory/routes.rs` | 桥路由（若 `memory_context` 需要） | 新路由写清「谁能调、最坏能造成什么」（E7） |
| `plugins/memory/host/` | 交接注入、经验段落 | 预算与换目录纪律复用 `policy.js` |
| `ui/src/memory/MemoryHandoffDetail.vue`（新增） | 交接详情弹窗 | 交接过不了「精简卡片 + 点开」那套（三个列表） |
| `ui/src/memory/` | 经验档、候选类型徽章、交接档 | 经验**复用** `MemoryRow.vue` / `MemoryDetail.vue` |

> **清单必须同步**：新增 `memory/handoff.rs` / `memory/experience.rs` / `MemoryHandoffDetail.vue` 时，
> `check-invariants` 的 **「记忆进度表覆盖所有模块」**会红（`scripts/check-invariants.mjs:2900-2931`）——
> 那不是缺陷，是提醒在 [memory-progress.md](memory-progress.md) 的模块地图里补对应行。

---

## 10. 已确认的决策（2026-10-23）

用户已接受[架构决策记录](adr-20261023-handoff-experience.md)：本轮推进 P0–P5，schema 6 只增加本计划字段；经验默认通用范围，审核时可改为来源项目；交接共用 `xlink.memory` 贡献并独立计字节，归档保留在导出中；新增 `memory_context` 工具。

实现复用 `MemoryRecord` 的公共字段并展平序列化，交接与经验分别存在自己的数据段中，不混入记忆列表。经验候选保留来源项目坐标供审核选范围，批准为通用经验时清空这些坐标。共用贡献只追加发生变化的段落，记忆刷新不会重发未变化的交接。日常验证遵守根目录规则，`npm run check` 留到发布前执行。

---

## 附：调研出处

| 结论 | 出处 |
| --- | --- |
| PowerContext 的四家族与 Scope 模型 | `oceanbase/powercontext` README（`master`） |
| dsh 集成的形状、验收版本、`/pc` 诊断面 | `docs/en/docs/integrations/dsh.md`（`master`） |
| 插件包名、peer 依赖、bundle patch | `integrations/dsh/plugins/powercontext/package.json`（`master`） |
| 注入路径（`agent/pre-step` → `prepare` → `createUserMessage`） | `src/recall.ts`、`src/index.ts`（`master`） |
| 插件未发布 npm | `npm view powercontext-dsh` → 404（2026-10-21） |
| 仓库根无 `package.json` | `gh api repos/oceanbase/powercontext/contents` → 根目录只有 `pyproject.toml` 等 |
| 内核 peer 依赖齐备 | 本机 `0.2.1-rc.2` 的 `node_modules/@deepseek-ai/` 目录清单 |
| `source.kind` 无 catch-all `plugin` | `dsh-llm/lib/types/message.d.ts:96-108`（本机 `0.2.1-rc.2`） |
| dsh-xlink 侧 git 安装读仓库根 manifest | `pkg/fetch.rs:266-283`、`plugins/center.rs:1554-1558` |
| 壳无 sidecar 监管能力 | `grep -r 'sidecar\|常驻\|supervis' src-tauri/src` 零命中 |
| 现有三块数据模型与 `SCHEMA_VERSION` | `src-tauri/src/memory/store.rs:51,303-310` |
| 现有预算常量 | `memory/query.rs:23-27`、`plugins/memory/host/policy.js:27,40,49` |
