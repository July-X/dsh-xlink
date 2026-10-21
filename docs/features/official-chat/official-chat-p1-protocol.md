---
description: "P1 协议与状态机冻结稿：固定数据模型、句柄与快照的对应关系、错误码、门控口径与首版常量。PM 定，devloper 依此实现。"
kind: "protocol"
status: "P1 协议冻结稿 v0.10（2026-10-21 19:5x，PM 定；v0.9 → v0.10 = **回填 G3-2b 落地现状**（第 14 码 `body_budget_exceeded` 的判定点**三处已落**：`host/protocol.js:1028`／`host/index.js:395`／`protocol.rs:577`）⇒ 订正 §3「判定点尚未存在」与 §4「判定点待落地」两处**过时措辞**＋ 订正 v0.9 依据里的**两处行号引用**（实现位移后已失效）＋ 补【已实测】读数（含 Rust 侧**环境口径**）；v0.8 → v0.9 = v0.8 → v0.9 = **订正 §3 错误码分工表**（老码 `body_too_large` 的判定对象／触发时机按**实现现状**订正 ＝ **单条引用正文**·**新增引用时**；原写「单次消息总预算／发送时／拒整条消息」**与实现不符**）＋ **§4 常量表区分两个同值不同义的 256 KB**（「最大正文字节」＝**单条引用正文**／已落判定点；「单次消息总预算」＝**整条消息**／**判定点未落、登记为缺口**）＋ **两码判序裁定**（先 256 KB 后 64 KB，**反了老码成死码**）；v0.7 → v0.8 = 码闭集 13 → 14 同步（§3 扩第 14 码 `body_budget_exceeded` ＝ G3-2b）＋ §3 闭集行号随第 14 码位移（码表 `:144-158` → `:144-159`、8 字段闭集 `:161` → `:162`）⇒ 本稿不新增其它码、只同步；v0.6 → v0.7 = 记入 S-7 与 development-plan §7:155 的字面冲突及以 S-7 为准的解释；v0.5 → v0.6 = 更正「锚点锁死」措辞 + 「只记日志」口径写实为四个可观测条件）"
date: 2026-10-21
---

# P1 协议与状态机冻结稿

**性质**：这是 **PM 定的协议**（不是实现说明）。实现、夹具、判据都依这份走；**协议与实现冲突时先回来改这份**，不要让实现反过来定义协议。

**依据**（全部来自本仓库实测与已登记证据，不含推测）：

- `docs/features/official-chat/official-chat-development-plan.md` §6（P1 冻结协议与状态机）、§8（快照与回收）、§9（账号变化）、§11（验收矩阵 A06 / A10 / A11）
- `docs/features/official-chat/official-chat-feasibility.md` §6 裁定 1–6、§9 先决条件 1–3
- 内核既有契约（现役 release 树 `0.2.1-rc.2`；**引用行号必须带版本号 + 文件 SHA-256**）：
  - `dsh-client-ui-input-trigger/lib/client.js` SHA `4A9605B0A811354ED49495790B331BF4F5198FF41614920D9D0C488E22A0F2AF` / 77121 B / 1951 行：`:1250-1252` `owner.codec.serialize(ref, signal)`（`:1251` 是 `owner?.codec === void 0` 那一行）
  - `dsh-client-ui-reference/lib/client.js` SHA `3D179111C51381733C7D0EC13C60EE4D9444C1DFB474EF28A0127654B8BA28A3` / 18455 B / 465 行：`:362-365` 内核自带 source 的 `codec: { clipboardText: (ref) => ref, serialize: (ref) => Promise.resolve(ref) }`
  - `dsh-client-ui-conversation/lib/client.js` SHA `8554738CBEEB5218AD1BFA4773EF094363B066D052A1893D7A96797C5BC33A63`：`:4356` `reference.length !== reference.clipboardText.length`、`:4358` `draft.text.slice(reference.offset, end) !== reference.clipboardText`
  - `dsh-client-ui-input-trigger/lib/client/types/types.d.ts` SHA `2D5B004229C1657BC0C484D1F881E2498FA9F45549589A3F1E40E82C1F637582`：`onPick` `:161`、`matchEnter?` `:174`、`codec.serialize` `:126`、`candidates` `:147`
- 插件侧现状：`plugins/official-chat/client/client.js` `:183-185`（一个 ref 只有一次读取）、`:214-227`（内存 `state` Map：pending/ready/failed）、`:310`（`reads.set(fixture.id, read)`）、`:321`（`ref: fixture.id`）、`:329-339`（`openReference` 四态措辞）、`:362-369`（同内核形状的 codec）、`:377-386`（`clipboardText → String(ref)` + draft 校验）、`:399-420`（等/拒语义）、`:403-405`（找不到 ref 的读取 → 抛错）、`:69-80`（G1-1 退让顺序）

---

## 0. 硬约束（决定了后面所有条款的形状）

**HC-1｜句柄 → 快照的指向，只能靠 `ref` 字符串承载。**
内核发送路径只把 occurrence 的 `ref` 交给源侧（`owner.codec.serialize(ref, signal)`），`clipboardText(ref)` 返回 `String(ref)`，draft 校验 `draft.text.slice(offset, end) === clipboardText`。
⇒ **`insert` 里加新字段不会到达发送路径**（内核只认 `claim` / `text` / `insert` 三个键，值只透传 `ref`）→ **快照标识只能编码进 `ref`**，没有第二条通道。

**HC-2｜`ref` 就是 chip 的可见文本，形态与长度是产品面。**
内核自带 source 的 `clipboardText: (ref) => ref`，加上 `reference.length !== reference.clipboardText.length` ⇒ **ref 的形态与长度 = 用户编辑器里那枚 chip 的文本与宽度**。**ref 变长 = chip 变宽 = 用户直接看见。**

**HC-3｜只有 `ready` 可形成就绪 chip**（计划 §6）。G1 现接口下「成功才插就绪 chip」不可达（G1-1 缺口）→ P1 沿用 **pending 句柄**形态，但**状态映射必须清楚**（见 S-2）。

---

## 1. 数据模型

### 1.1 固定快照（落盘物）

| 字段 | 语义 | 约束 |
| --- | --- | --- |
| `schemaVersion` | 快照格式版本 | 整数，**首版 = 1**；变更即升版，**旧版快照按 P1-07 拒** |
| `snapshotId` | **不可变**快照标识 | 与官网会话 id **分开**（计划 §6）；**同时就是 `ref`**（见 S-1） |
| `providerId` | 来源 provider | 稳定 id，**不使用易变页签下标** |
| `accountScope` | 账户范围 | 由服务端从**已绑定实例 + 壳模式 + 实际站点账户**推导（计划 §6），客户端传的账户 id **只做一致性校验、不构成授权** |
| `sessionId` | 官网会话 id | **不是** `snapshotId`；同一会话可有多份快照 |
| `branch` | 有效分支 | 规范化后的分支标识 |
| `messages` | 消息角色与文本 | 角色 + 文本；**未支持内容要显式记**（不静默丢） |
| `contentDigest` | **规范化内容摘要** | 规范化规则固定（见 1.3）；用于校验完整性 |
| `captureResult` | 捕获结果 | `ok` / `partial` / `failed` + 省略事实 + 未支持内容清单；**形态 = 对象**（v0.2 裁定：对象必带三字段；**`ok` 兼容字符串 `'ok'`**（等价 `{status:'ok',omitted:[],unsupported:[]}`），`partial` / `failed` **必须用对象**——用字符串会丢「省略了什么」，违反「不静默裁掉」纪律） |
| `capturedAt` / `sourceUpdatedAt` | 捕获时刻 / 来源更新时间 | ISO 8601 UTC |
| `sourceFingerprint` | 来源指纹 | 账号指纹；变化即该快照失效（见 S-5） |

### 1.2 目录页（缓存物，与快照**分开**）

账户范围、会话 id、标题、更新时间、真实深链、游标、覆盖范围、是否还有下一页；**不含正文**。

### 1.3 规范化与摘要

- 规范化：行尾统一 `\n`、去 BOM、去尾随空白、保留正文内空行与缩进（不得改动语义）、角色名小写枚举。
- `contentDigest` = 规范化后正文的 SHA-256（十六进制小写）。**摘要口径写进夹具**，两端各验一次。
- **摘要不匹配 = 快照损坏 → 按 `snapshot_invalid` 拒**（不是重读）。

---

## 2. 句柄与快照的对应关系（本稿的核心）

### S-1｜**单一 id 制度**：`ref === snapshotId`

- 依据 HC-1（内核只透传 `ref`）→ 若 `ref` 与 `snapshotId` 是两串，就必须在插件侧维护一张 `ref → snapshotId` 映射表，而这张表本身是**第二份状态**（会漂）。
- 决定：**只有一串 id**。`snapshotId` 是落盘主键与 host 路由键，**同时就是插件交给内核的 `ref`**。
- 格式：`<providerShort>~<sessionShort>~<seq>`
  - `providerShort`：provider 的稳定短码（如 `ds` / `gpt` / `mm`），≤ 4 字符
  - `sessionShort`：官网会话 id 的**稳定短码**（同一会话恒定、不同会话不撞），≤ 12 字符
  - `seq`：该会话在**本实例内**的 pick 序号，4 位（`0001` 起），单调递增
- **长度上限：整串 ≤ 24 个可见字符**（HC-2：这是 chip 宽度）。超限的会话短码按稳定规则截断（保留尾部），并把完整会话 id 存进快照（`sessionId`）——**映射靠落盘快照，不靠短码反解**。
- **可读性的边界**：ref 会被内核原生 chip 原样渲染成文本，**我们改不了内核**（不动安装包）→ 接受「机器标识即 chip 文本」。**用户可读性由展示层承担**：chip 的 label 保持中性、详情面板显示真实来源与捕获时间（计划 §10 P5），**不靠改 ref**。
- **唯一性粒度 = 一次 pick**：同一条对话选两次 → 两个 `snapshotId`（`seq` 不同）→ **第二次不再覆盖第一次**（G1 现状 `reads` 以对话 id 为主键，选两次会互相覆盖，这是 P1 必须修掉的）。
- **id 分配权归 host**（v0.2 裁定，devloper 17:5x 提出「client 的 `seq` 是进程内计数，重启会从 1 重来 → 可能撞盘上已有 id」）：快照**落盘且跨进程可发**（S-6）→ 若 `seq` 由 client 的进程内计数生成，**dev 壳与 release 壳同开、或任一进程重启，都可能生成同一个 `snapshotId` → 撞盘**（host 按「同 id 不重写」拒 → 用户看到「这次选择没生效」）。⇒ **`seq` 由 host 侧统一分配**（host 持有并持久化已用序号，client 不得自造）；**「本实例内单调递增」必须跨进程成立**。

### S-2｜状态机（直接采用计划 §6，不自造）

```text
selected → reading → validating → snapshot_saved → ready
     reading/validating → failed 或 cancelled
     任意未完成状态 → stale（目标草稿 / 账户已变，废弃回包）
ready → serialized → expanded → persisted
```

- G1 现状的 `pending` 句柄 = 状态机的**未完成态**（`reading` / `validating` / `snapshot_saved` 之前）；`ready` = **已落盘**。
- **HC-3 落地**：只有 `ready` 才叫「就绪」。pending 句柄**不得**在 label、详情、任何文案里被称为就绪（feasibility §9 先决条件 1）。
- 一次请求只允许一个终态；**取消后的迟到结果不得创建可引用快照**（计划 §6）。
- 失败、取消、陈旧回包**均不得清空或覆盖草稿**。

### S-3｜**单一事实源 = 快照文件**；句柄状态由它派生

- 落盘后，句柄的 `pending` / `ready` / `failed` **不再另存内存状态**——由快照文件的 `captureResult` + `schemaVersion` 派生。
- 理由：内存与磁盘两处状态会漂，而「就绪」若有两个真相，先决条件 1 就守不住。
- 内存里只允许有**尚未落盘**的在途态（`reading` / `validating` / `snapshot_saved` 进行中）；**一旦落盘，状态以文件为准**。

### S-4｜**门控口径：从「时序」改成「状态」**（采纳 tester 建议）

> **任何一次请求发生时，该 occurrence 指向的快照必须已落盘，且落盘内容与模型输入里的正文逐字节相同。**

- 「落盘前 0 请求」是它的**充分非必要条件**——落盘晚没关系，只要**请求发生时**正文已固定。
- 判据从「数时序点」变成「**比对磁盘内容与请求体**」，稳得多，也覆盖「落盘后重读」这类时序测不出的形态。
- G1 已有纪律「读取完成前 0 请求」**继续有效**（dev-r6 断言 D：t=279ms 时 reqs=0 / t=7957ms 时 reqs=1），但 P1 的新增判据以**状态口径**为准。

### S-5｜失效与缺失：**一律拒**（+ 可见错误；chip `invalid` 态在内核具备前**不可达**）

- 快照缺失 / `sourceFingerprint` 变化（换账号）/ codec 失联 / `schemaVersion` 不支持 / 摘要不匹配 → **拒**（对应模型请求数 = **0**）+ **可见错误出口**。
- **失效的可见形态只有一条**（v0.2 裁定，tester 17:1x 实测 + 内核现役 `0.2.1-rc.2` 复核）：**发送时拒 + 可见错误**。这是内核唯一通路——owner 侧 `codec.serialize` 抛错 → 拒发（请求数 0）→ 可见 error notice。**「chip 标 `invalid`」在内核具备该能力前不可达**：
  - `ReferenceInsert`（插入期）**没有** `invalid` 字段——它在 `dsh-client-ui-conversation/lib/types/client/contract/draft-editor.d.ts`（SHA `3397E3224506A8F448901C4AE3EB1932DABE3FA257B9BA9C5186EFD2AD918375`）`:12-18`；`readonly invalid?: boolean` 在 `Occurrence` 上（同文件 `:105` / `:122-123`），**不是插入期字段**。
  - `lib/types/client/input/editor/chip-node.d.ts`（SHA `816035EAA7730AD5C57334F3A261CCF9529F1A27F72069FD012C587AB03F737F`）`:21` / `:35` 有 `invalid` 语义，但 **`setInvalid` 在现役内核零调用方**；DOM 上**无 `data-invalid`**。
  - ⇒ 登记为**新上游缺口 G1-6：内核无「chip 进入 invalid 态」的写入口**（见[第 8 节](#8-验收项p1-的硬性门禁)第 12 条）。**在 G1-6 落地前，本条的验收口径 = 「请求数 0 + 可见错误」**；**不得**把「chip 标 invalid」降级写成「节点级断言」假装能测。
- **`sourceFingerprint` 的校验点 = host 命令层**（v0.2 裁定，tester 17:1x 提问）：
  - **存储层只校验 `schemaVersion` 与 `contentDigest`**，**不比对指纹**——存储层拿不到「当前指纹」。
  - **换账号的第一道防线是目录隔离**：`accountScope` 目录按站点账户隔离（[第 5 节](#5-命名空间与桥)路径规则）→ 换账号后旧快照**不在当前账号的目录里**，取不到。
  - **第二道防线在 host 命令层**（打开/读取上下文那条链——只有它能拿到当前指纹）：比对「快照里的 `sourceFingerprint`」与「当前会话指纹」；**当前指纹不可得时 → 拒**（`account_changed`），**不得放行**。
  - ⇒ **本条不删**（删掉等于放弃「换账号后旧快照不可发」这条纪律），**改的是校验位置**。
- 换账号后**已选中的快照仍保持原来源归属**，不挂到新账号名下（计划 §9）；但它**不可再发送**（指纹不符 → 拒）。
- **不得静默回退到重读**（重读 = 「最新正文」，会破坏固定版本；计划 §8 明写「codec 在重试时不得把官网 id 重新解析成最新正文」）。

### S-6｜跨进程与保留期

- 落盘的意义正是**跨进程仍可发**：进程重启后，快照仍在 → 句柄仍可用。
- 保留期与清理：见 4 的常量；清理只清**本实例自己**的快照目录，绝不按解析出来的家目录做递归删除（AGENTS.md 既有纪律）。
- 快照已被清理 / 找不到 → **拒**（`snapshot_invalid`），不静默消失、不自动重读。

### S-7｜**快照 id 唯一 ≠ 正文去重**（采纳 tester 补漏）

- 明确写成两条独立条款：**S-1 的唯一性**（一个 ref 只有一次读取）已冻结；**正文去重不在 P1 范围**。
- 同一段对话被引用两次 → **两个 `snapshotId`**，正文**按引用次数各送一份**（不隐式去重）。
- 理由：静默去重会让「用户看到的引用数」与「模型收到的内容数」不一致，违反「不静默裁掉」的纪律。
- **正文去重列为 P2 议题**（属 mention 展开与模型输入预算的范畴），P1 不预埋。
- **（PM 22:3x 口径注释 · 与 development-plan §7:155 的字面冲突）**：`development-plan` §7:155（G4 验收）写「用户原文和选中正文进入正确会话，**正文只出现一次**」，与本条**字面冲突**。**以本条为准**，并按上下文把 §7:155 的「一次」解释为 **(a) 同一份快照的正文只追加一次**（不因内核重放 / 工具循环被多次追加），而本条说的是 **(b) 同一消息内同一正文被引用两次 → 各送一份**。⇒ **建议 development-plan 按此改写**；**该文件不归 PM 改，已上报**（并记入 feasibility §7）。

---

## 3. 错误码（稳定、可机检）

| 码 | 含义 | 可重试 | 用户下一步 |
| --- | --- | --- | --- |
| `not_logged_in` | 未登录 | 否 | 去官网窗口登录 |
| `account_changed` | 账户已变（指纹不符） | 否 | 重新选择来源 |
| `source_unsupported` | 来源不支持 | 否 | 换一个来源 |
| `schema_changed` | 接口结构变化 | 否 | 升级插件 |
| `partial_content` | 部分内容（未支持内容被省略） | 否 | 明示省略事实 |
| `body_too_large` | 正文超限 | 否 | 换更小的会话 |
| `timeout` | 超时 | 是 | 重试 |
| `cancelled` | 取消 | 否 | 重新选择 |
| `snapshot_invalid` | 快照缺失 / 失效 / 摘要不匹配 / 版本不支持 | 否 | 移除该引用 |
| `kernel_incompatible` | 内核不兼容 | 否 | 升级内核 |
| `catalog_empty` | **目录没有结果** | 否 | 换个关键词 |
| `catalog_failed` | **目录读取失败** | 是 | 重试 |
| `stale_draft` | 草稿代次已变 | 否 | 重新选择 |
| `body_budget_exceeded` | **单条正文超「单条正文预算」**（`perBodyBudgetBytes` 64 KB，见 §4） | 否 | 选一段更短的正文 |

- 每个错误码带：可重试性、用户下一步、关联 `requestId`、**脱敏**日志路径；**不得把内部异常串当成功值**（计划 §6）。
- **v0.4 裁定｜错误结果对象字段闭集 = 8 个，全部必备、不省略**：`kind` / `ok` / `code` / `retryable` / `userNext` / `requestId` / `logPath` / `detail`。
  - **（PM 23:5x 核出、2026-10-21 00:1x 已闭环 · 缺口 G-2）**：此前 **JS 侧 `errorPayload` 只产 7 个（缺 `kind`）**，而 **Rust 侧 `error_result` 已产 8 个** ⇒ **本闭集条款始终不改**（是 **JS 漏字段**、不是协议写错）⇒ **JS 侧已补齐 `kind: 'error'`**（未知码归一分支与正常分支**都补**）⇒ **两端同构**。**两端各有一条自测钉住这 8 个字段**（JS `plugins/official-chat/test/protocol.test.mjs`、Rust `src-tauri/src/official_chat/protocol.rs` 的 `error_result_carries_the_eight_field_closed_set`）⇒ 谁再省一个字段当场红。
  - 依据：协议 §3 原文「每个错误码带：可重试性、用户下一步、**关联 requestId**、**脱敏日志路径**」+ Rust 侧 v0.2 注释「错误负载要带齐 §3 要求的字段，与 JS 侧 `errorPayload` 同构」。
  - 规则：① **8 个字段一律出现**，不可得时填 **`null`**（**不省略字段、不填空串**）；② 错误路径 `ok` 恒 `false`；③ `code` 取**上表 14 码闭集**；④ `detail` 承载脱敏后的原始码与原因线索（内核 `details` 是自由 record，`dsh-client-connection/lib/index.js:614`）。
  - **为什么必须写死**：JS 与 Rust 两端各写各的字段集 → 协议夹具的「双端一致」永远红，且**每端都自以为对**。**闭集是让两端能对上的前提。**（tester 17:4x A1c/A1d 报红即源于此；devloper 19:2x 曾按「Rust + 旧夹具都是 4 字段」判成 4 字段——**那是循环证据**，已撤回。）
- `catalog_empty` 与 `catalog_failed` **必须分开**（计划 §6 明写「目录没有结果与目录读取失败是不同状态」）。
- **本表是闭集，14 个码**（v0.2 裁定，devloper 17:5x 提问 `snapshot_exists`）：**内部冲突码一律归入 `snapshot_invalid`**，原始码保留在 `detail`（如「同 id 重复落盘」→ 归 `snapshot_invalid`，原始码 `snapshot_exists` 进 `detail`）。**第 14 码 `body_budget_exceeded` 是唯一的例外扩容**（PM 2026-10-21 17:0x 为 **G3-2b** 裁定，见下条）⇒ ⇒ **第 15 个码仍不新增**——码表稳定可机检是本条纪律；用户侧看到的是「这次选择没生效」，下一步是移除该引用后重选。
- **`body_budget_exceeded`｜第 14 码（PM 2026-10-21 17:0x 派单 · 落笔线定稿）** —— **触发条件**：**单条**正文（进入模型输入的**那一条**）**超过 §4 的「单条正文预算」`perBodyBudgetBytes` ＝ 64 KB** ⇒ 报此码并**拒收该条**（**新增对外拒收路径**，G3-2b）。
  - **可重试性**：**否**（重试同一条正文没有意义 —— 要用户换动作）。
  - **用户下一步**：「**选一段更短的正文**」。
  - ⚠️ **与既有 `body_too_large` 严格分工（不许混用）** —— 两者**判的是同一档东西（单条引用正文）、分属两个阈值层**（256 KB / 64 KB）：| 码 | 判定对象 | 阈值 | 触发时机 | 被拒的是什么 |
    | --- | --- | --- | --- | --- |
    | `body_too_large` | **单条引用正文**（§4「**最大正文字节**」／`MAX_BODY_BYTES`） | 256 KB | **新增引用时**（捕获那条会话时） | **那一条**引用被拒（**v0.9 订正** —— 原写「单次消息总预算／发送时／拒整条消息」，与实现不符，依据见下） |
    | `body_budget_exceeded` | **单条正文**（§4「单条正文预算」／`perBodyBudgetBytes`） | 64 KB | **新增引用时**（该条正文进模型输入之前） | **那一条**引用被拒（别的引用不受影响） |
    ⇒ ⇒ **判「一个码说不清」**：同一档判定（单条引用正文）上叠了**两个阈值** ⇒ ⇒ **同一个用户动作（引用了一段很长的会话）会得到不同的码**（**64 KB～256 KB ⇒ 第 14 码「选一段更短的正文」**；**>256 KB ⇒ 老码「换更小的会话」**）⇒ ⇒ **可见提示与判据都必须按上表取码** ⇒ ⇒ **不许拿 `body_too_large` 兼表「单条超限」**（那是**掩盖真实原因**：用户下一步一个是「换更小的会话」、一个是「这段太长」）。
  - **现状（v0.10 如实登记 · G3-2b 已落地）**：`perBodyBudgetBytes` **判定点已落三处**（本地提交 `3ce36c8`，**只本地未推**）—— `plugins/official-chat/host/protocol.js:1028`（**捕获/新增引用时**，`resolveCaptureCase`）＋ `plugins/official-chat/host/index.js:395`（**落盘前**，`writeSnapshot` ⇒ 抛出即不落盘 ⇒ **别的引用不受影响**）＋ `src-tauri/src/official_chat/protocol.rs:577`（Rust 侧同构）⇒ ⇒ **本码的码返回点已存在**，闭集与实现**已对齐**。　⚠️ **阈值单一来源**：JS 取 `CONSTANTS.perBodyBudgetBytes`（`host/index.js` 的 `PER_BODY_BUDGET_BYTES` 由它**派生**、不写死第二份）＋ Rust `PER_BODY_BUDGET_BYTES`（`64 * 1024`）⇒ **两端同值**。　⚠️ **`detail` 三数契约（PM 2026-10-21 裁 · 已落）**：界面文案要的三个数**只能由 host 侧带进 `detail`**（客户端拿不到 host 的内部变量）⇒ JS 两处与 Rust 一处**逐字同构**：「单条正文超单条正文预算：正文 X 字节，超出单条正文预算 Y 字节（超 Z 字节）」⇒ ⇒ 客户端按它解析、**取不到就退化成无数字文案**、**不许编数字**。　⚠️ **判序裁定（PM 2026-10-21 19:1x）**：两码**判定对象相同、只差阈值** ⇒ ⇒ **必须按阈值大小顺序判：先 256 KB（`body_too_large`）→ 再 64 KB（`body_budget_exceeded`）** ⇒ ⇒ **反了就出死码**（>256 KB 必然也 >64 KB ⇒ 老码永不触发 ⇒ 死码 ＝ 负资产）⇒ ⇒ **实现必须让老码先判**。　⚠️ **v0.9 订正依据（PM 一手核实现，非转述）**：老码现有两个判定点都在**捕获路径**、判的是**被捕获那条会话的消息集合** —— `plugins/official-chat/host/index.js:379-393`（`writeSnapshot` 里 `normalizeMessages(snapshot?.messages)` → `bodyBytes` > `MAX_BODY_BYTES` ⇒ throw）＋ `plugins/official-chat/host/protocol.js:1005-1019`（capture 命令层同判同对象）（**行号按 v0.10 现状**；v0.9 记的是 `:363-368`／`:1005-1009`，**实现位移后已失效** ⇒ v0.10 订正）⇒ **触发时机 ＝ 用户选中一条引用、capture 那条会话时**（`plugins/official-chat/client/client.js:778` 的 `host.call('capture', …)`，`:793` 才 `selectedRefs.add`）⇒ ⇒ **既不是「整条消息」也不是「发送时」**；且【已实测】**客户端侧零预算判定**（`plugins/official-chat/client/**` grep `maxBodyBytes|MAX_BODY|BUDGET|budgetBytes|bodyBytes` **零命中**）⇒ ⇒ 「整条消息」那一档**既无判定点也无码** ⇒ 见 §4「单次消息总预算」行的缺口登记。　【已实测（PM 一手，2026-10-21 19:5x）】：`official_chat::protocol` **15 passed / 0 failed**（含 `single_body_over_budget_is_rejected_with_the_fourteenth_code` ＋ `the_two_body_codes_are_judged_in_threshold_order`＝**判序那条也真过了**）｜全量 Rust lib **1146 passed / 0 failed / 11 ignored**（exit 0）｜JS 侧 `test:plugin` **346 pass / 0 fail**｜判据第 6 条（`codes`）**12/12 通过**、11 支 P4 判据**全绿**。　⚠️ **Rust 侧环境口径（如实登记）**：本机 lib 的 **test harness 缺 comctl32 v6 manifest** ⇒ 测试二进制**装载期**就 `0xC0000139`（**与本码无关**：HEAD 基线同样复现）⇒ ⇒ 上述 Rust 读数是**补 manifest 后**跑出的（`mt.exe` 补 manifest，**仓库零改动**）⇒ ⇒ **「编译通过」≠「断言通过」**，两者分开记。
  - **入库方式（落笔线给的口径 · PM 派单时按此写）**：本码的**表行**与下面三处**必须同笔**——① 本表加行（本笔已落）② `scripts/check-official-chat-p4-codes.mjs` 的 `EXPECTED_CODES` ＋ 其自测里「与本表逐项相等」的断言（13 → 14）③ 实现侧两端闭集（`src-tauri/src/official_chat/protocol.rs` 的 `ERROR_CODES` / `ERROR_RETRYABLE` / `ERROR_USER_NEXT` ＋ `plugins/official-chat/host/protocol.js` 的 `ERROR_CODES` / `ERROR_CODE_NAMES`）⇒ ⇒ **三处不同步 ⇒ 判据第 6 条当场红**（对账是**逐项相等**，不是子集）⇒ ⇒ **不新开一条判据**（新开会出现两条判据对同一张表各说各话）⇒ ⇒ **口径＝「同时更新判据第 6 条的期望清单」**。
- **各码的使用边界**（v0.2 补，devloper 18:2x 提出「捕获失败没有专门码」）：
  - **不新增「捕获失败」码**——捕获失败**不是一个单一原因**，必须**按原因分流**：超时 → `timeout`（可重试）；官网页面结构变了 → `schema_changed`（升级插件）；用户取消 → `cancelled`。
  - **`source_unsupported` 只用于「这个来源根本不支持该能力」**，**不得**用来兜「捕获失败」——那会让用户看到「换一个来源」而其实该**重试**，属于**掩盖真实原因**。
  - ⇒ 「捕获失败但原因不明」时用 `timeout` 兜（可重试），并在 `detail` 写明脱敏后的原因线索。

---

## 4. 首版常量（**工程参数**，不伪称来自站点限制）

| 常量 | 首版值 | 备注 |
| --- | --- | --- |
| 目录页大小 | 50 条 / 页 | |
| 目录缓存期限 | 5 分钟 | 换账号即失效 |
| 搜索去抖 | 300 ms | |
| 读取超时 | 30 s | G1 桩模型门控实测到 ~8 s，留余量 |
| 最大正文字节 | 256 KB | **单条引用正文**上限（**v0.9 订正**：原表未写清判定对象）｜超过 → `body_too_large`（**在捕获那条引用时**） |
| 最大响应字节 | 1 MB | |
| 快照保留期 | 7 天 | 超期按 `snapshot_invalid` 拒 |
| 缓存总量上限 | 512 MB / 实例 | 含目录缓存与快照 |
| 单消息引用数上限 | 8 条 | |
| 单条正文预算 | 64 KB | 进入模型输入的**单条**上限；**超 ⇒ 报 `body_budget_exceeded`**（§3 第 14 码；⚠️ **v0.10 登记：判定点已落**（`3ce36c8`）—— 三处：`host/protocol.js:1028`／`host/index.js:395`／`protocol.rs:577`（**判单条正文**，超则**拒收那一条**）） |
| 单次消息总预算 | 256 KB | **含原生 `@session`**，自有来源不得绕过总预算（计划 §7）｜⚠️ **v0.9 登记：判定点未落**（【已实测】客户端侧无任何预算判定）｜与上一行「最大正文字节」**同值不同义**（那行判**单条引用正文**、这行判**整条消息**）⇒ ⇒ **本轮不落判定点**，且**不许拿上面两档的码兼表** |
| `schemaVersion` | 1 | |

常量**集中配置**并有边界测试（计划 §6）。

### 4.1 保留期主动清理（v0.4 裁定，devloper 20:0x 提出候选口径）

**现状如实说**：`snapshotRetentionMs` 此前**只是常量、没有过期检查、没有清理任务**；认领文件已记 `claimedAt`。**本阶段补上主动清理**，口径如下（**不新增常驻定时器**——插件不常驻，定时不可靠）：

1. **触发点**：host 半边**惰性触发**，**只在 `catalog` 入口**检查一次，同实例**至多一天一次**；**`capture` 不触发**（用户主动路径更短更可控）。节流键：`<root>/sweeps/last-sweep-<YYYY-MM-DD>.json`（独立目录，不混进快照目录）。
2. **扫哪些**（四家族，均在 `<root>` 下）：快照 `snapshots/<provider>/*.json`（按文件内 `capturedAt`，mtime 兜底）；认领 `seq-claims/*`（按 `claimedAt`，**且必须确认对应快照已不在**才可删）；目录缓存 `catalog-cache/*`（5 分钟 TTL）；错误日志 `logs/official-chat.log`（按行数/大小截断，**不按 7 天删**——日志是诊断证据）。
   - **（PM 20:2x 澄清 · 判据按这两条边界写）** ① **「按文件内 `capturedAt`，mtime 兜底」= `capturedAt` 优先**：`capturedAt` 过期而 mtime 新 ⇒ **仍判过期并删**；`capturedAt` 新而 mtime 旧 ⇒ **不删**（mtime 旧不是删除理由）。mtime **仅在 `capturedAt` 缺失或不可解析时**才作兜底。② **日志「按行数/大小截断」= 两个上限都要满足**：先按行数留尾部，再按字节留尾部（按行退，不切字节数组——切半个多字节字符会毁掉诊断证据）。
3. **怎么避免误删**：只删**超过 7 天**的（`snapshotRetentionMs`）；顺序**先快照后认领**（认领先删会指向仍存在但已过期的快照）；**两步法删除**（先 rename 再 unlink），避免删到一半留下半截文件被当成有效快照；**只动本实例根下这四家族**，根不可得就**不扫**（宁可不清理，不乱删）；清理失败**只记日志**，不阻塞业务。
   - **（PM 22:2x 澄清 · 「只记日志」的口径写死）**：「只记日志」= **四个可观测条件**——① **不许越界**：删除失败**不抛异常**；② **不阻塞**：失败的那一个**不拖垮整轮**（别的过期文件照删）；③ **失败项的内容仍在磁盘上**（原路径**或**墓碑任一——两步法下 rename 成功、unlink 失败时，内容被**改名成墓碑**，不能死盯原路径；且**不许假装删掉**）；④ **必须留痕**：失败**进了日志**。**「只」修饰的是「记日志」这个动作**（相对于「阻塞业务」），**不是**把「记日志」降格成可选项。**单文件级留痕是硬要求**：只报「整轮失败」而把「哪个文件删不掉」静默吞掉 = 没人知道有文件没清掉、磁盘会一直涨 ⇒ 属**违反本纪律**，不是更宽松的写法。
   - **留痕放在调用处**（PM 22:1x 落地时已定）：调用点才知道「删的是什么」（哪个家族、哪个文件），helper 的 `catch` 只知道「这一步失败了」。**日志写相对本实例根的路径**，不写绝对路径；**日志写入本身失败时也要留痕**（不能再退化成静默）。
   - **（PM 22:2x 更正）**：本稿 v0.5 初版曾写「**已提交版**的 A17 活体注入锚点锁死『helper 内部不动 + 调用行逐字不变』」——**该说法已不成立**：A17 / A18g 的锚点已改为**形状容忍**形态（A17 = 正则匹配调用点形态；A18g = 只锚 `renameSync(path, tombstone);` 单行），helper 的调用形态再变**不会**让已提交判据报红。⇒ **实现侧不因判据而锁死形态**；留痕放调用处的**理由只剩一条**——「调用点知道删的是什么」。
4. **「正在发送中的引用」不保护、不引入引用计数**（协议没写，不自造）：后果是「该引用失效并被拒」（`snapshot_invalid`），**不是错发**——S-5 / S-6 既有语义已覆盖这个后果。
   - **（P3 起 · 本条被 `official-chat-p3-protocol.md` S-11 取代）**：P3 引入**落盘的占用记录**（`<root>/refs/<snapshotId>.json`），使「回收只处理未引用记录」可机检。
     **本条的「后果语义不变」**（被清掉的引用失效并被拒，**不是错发**），**变的是「回收前多了一步引用判定」**。
     ⇒ P3 的 **S-12** 另登记上游缺口 **G1-8**：内核 source 契约（`InputTriggerSource` 接口全集，
     现役 `0.2.1-rc.2`）**无「引用被移除／草稿被放弃／已发送成功」信号** ⇒ 占用记录是**插件侧自洽的占用凭证**，
     **不是**内核草稿的真持有关系；**不得**把「引用关系精确等于内核草稿」写成验收项。
5. **落盘策略**（P3 起 · **条款正文在 `official-chat-p3-protocol.md` §S-14**，此处**只放指针**、不复制正文，单点维护）：Unix 私有权限（快照与占用记录 `0600`、目录 `0700`）；**Windows 不声称加密**——依赖用户 profile 的默认 ACL，**不**把 `0600` 或既有凭据 vault 等同于完整跨平台加密；**日志与诊断包不收正文**（该条**已满足**，判据里有钉子防回退）。

---

## 5. 命名空间与桥

- 服务端从**已绑定实例、壳模式、实际站点账户**推导命名空间；**客户端传入的会话标识用于一致性校验，不构成授权**（计划 §6）。
- **client → host 的通道 = 内核 `connection` 服务**（**不是** cordis `ctx.set` 挂服务；**也不是**插件自建 webServer 路由）。**形态**：**长期**走 **exact fetch route**（见下条）。**不推动上游修改**（izzy 23:0x 拍板：禁止修改上游 dsh 内核、不向上游提 PR）⇒ **不打算收敛回内核 RPC**；`rpc.handle` 路径**只登记缺口、不使用、不修**。**为什么这条路**（devloper 17:5x 取证 + Chief 20:0x 源码核对，内核现役 `0.2.1-rc.2`）：
  - **cordis `ctx.set(name, service)` 不可用**——cordis 要求该属性已 `provide` 过，否则 `cannot set property "..." without provide`（`cordis/lib/index.js:782-789`）。现役 `host/index.js:407-414` 把这个抛错吞成一行 warn，**服务根本没挂上**。
  - **浏览器树 ≠ Node 进程树**——client 半边跑在浏览器页面里，host 半边跑在内核的 Node 进程里，**不在同一棵 cordis 树**；浏览器树里 `ctx.get('xlink-official-chat')` 永远解析不到。
  - ⇒ 走**内核 `connection` 服务**是**唯一可行**的通道（RPC 或 exact fetch route 二选一）。**本条是对「cordis 服务注册」与「插件自建路由」两条路的显式否决**，下一个人不必再试。
  - **v0.4 裁定｜通道形态 = 内核 `connection` 服务的 exact fetch route**（`ctx.connection.fetch.register({kind:'exact', path:'/api/xlink-official-chat', methods:[…], requestBody:'buffered', fetch})`）。**内核给三样**（现役 `0.2.1-rc.2` 源码核对）：① **路径必须在 `/api` 命名空间下**——`assertFetchRoute`（`:973-977`）用 `endpointFromPath("/api", route.path)` 校验，不过就抛；② **owner 准入由内核做**——`createSharedFetchHandler`（`:820-838`）命中 exact route 时先判 `peer.principal.kind !== "owner"` → 403；③ **与 API Gateway 的 shared interceptor 两级分发不冲突**——exact route 先查、miss 才落 interceptor。**且 `registerFetchRoute`（`:840-854`）完全不碰 webServer** ⇒ 不会触发上面那个抛错。**插件要自己做的**：RPC 信封（exact route 命中后**绕过**内核的 `rpcFetchHandler`，信封由插件实现）。**落地阻塞项**：exact path 是**全等匹配**（`fetchRoutes.get(url.pathname)`）⇒ **必须先实测浏览器侧 `rpc.call` 实际 POST 的 pathname 与 method**（`dsh-client-connection/lib/client.browser.js:1212-1231`、`:1497`），错一个字符就 404/405。**③（插件自注册 `ctx.webServer.register` + 自实现信封与鉴权）否决**——那要自造 owner 准入 = 自造安全面，且绕过内核的路径校验；**①（上游把 `owner.webServer.register(route)` 改成 `owner.get("webServer").register(route)`，与内核自己 `:745`/`:778` 写法一致）——已由 izzy 23:0x 拍板否决：不修改上游内核**（见 §9 与进度文档）⇒ **不再上报、不再推动**；exact fetch route **长期**为唯一通道形态。
  - **⚠️ v0.4 更正（devloper 20:0x 真机实测 p1-r3/r6）**：v0.2 那条「host 半边 `inject = ['connection','webServer']` 就能修好 `rpc.handle`」**已被证伪**——cordis 的属性查找落在**内核 connection 插件自己的 fiber scope**（`(ctx[symbols.shadow] ?? ctx).fiber`，`cordis/lib/index.js:683` + `createShadow` `:111-116`），而那个 scope **从不 inject `webServer`**（内核一律安全读 `this.owner.get("webServer")`，`:745`/`:778`）⇒ **插件侧 declare `webServer` 也照样抛** `cannot get property "webServer" without inject`（实测两轮无效）。**插件侧无法自修这条。**
  - **对判据的约束**：判据的 stub 是**同步服务**、RPC 是 **Promise**——判据库期望值不变，**由 adapter 吸收这个差异**（adapter 只暴露协议规定的接口面，底层是同步桩还是 RPC 由它自己吸收）。
  - **⚠️ 判据不得把 `connection.rpc.call` 直连 host handler**——那会**绕过内核 `connection` 服务的真实路由注册**，正是 A0 那个真洞藏身之处（真机全红、判据不报）。**「桩把两条本该跨进程的调用接上了」= 桩骗过自己。**
- 桥**不接受**任意路径、任意脚本、任意 URL、调用方指定的账户目录。
- 客户端命令职责：`capabilities` / `catalog` / `capture` / `cancel` / `preview` / **`duplicates`**（**`duplicates` = P2 第一批新增**，口径见 `official-chat-p2-protocol.md` §2.2；**P1 第一批不含它**）；host 回环路由**只按固定 id 读快照**与必要的模型可见投影。名称在实施时统一登记。
- 复用现有桥的**鉴权与轮换**，**不复制监听器**；新增 GET 查询路由用精确路径匹配 + 参数校验；若必须加请求体协议，先补**定长读取与大小上限**，不得复用只读到请求头的 `body_prefix`。
- 路径：目录缓存与正文快照**分开**；所有路径**跟随后端路径解析**（按壳模式、内核族、实例、站点账户隔离），**不在代码中展开真实用户 home**（计划 §8）。
  - 目录缓存：`<实例 official-chat 根>/catalog-cache/`（**平铺文件名** `<provider>__<账号范围>__<查询词>.json`，见 `host/commands.js` 的 `fileOf`；**没有 provider 子目录**——PM 20:2x 按实现对齐，§4.1 第 2 条的 `catalog-cache/*` 已是这个口径）
  - 正文快照：`<实例 official-chat 根>/snapshots/<provider>/<snapshotId>.json`

---

## 6. 协议夹具与版本号

- **同一组 JSON 夹具**，Rust 与 JavaScript **各跑一遍**，断言「同一夹具 → 同一解析结果」；`schemaVersion` 两端常量必须一致。
- 夹具至少覆盖：正常快照、部分内容、超限、空目录、目录失败、超时、取消、账户已变、快照缺失、版本不支持、摘要不匹配。
- 快照文件**不可变**：同一 `snapshotId` 不重写；重新选择同一对话 → **新** `snapshotId`（计划 §8）。
- **`expected` 的定位**（v0.2 裁定，tester 17:1x 提出 A1d 是推断口径）：夹具的 `expected` 是**参考值**。**判据侧断言的是「双端一致」（A1c）**；「实现结果 == `expected`」属**实现自测**（协议夹具 runner 的职责）——**两者都要有，但后者不是前者的替代**。**不得**把 A1c 放宽成「只要双端一致就行」而取消 `expected` 校验。
- **首版会话短码算法**（**工程参数**，不是站点限制；与[第 4 节](#4-首版常量工程参数不伪称来自站点限制)同一定性）：官网会话 id → `sessionShort` = **两段 FNV-1a 变体**（不同盐，避免不同会话撞）→ **base36 编码取后 12 位**；**口径 = UTF-16 码元**（不是 UTF-8 字节）。`providerShort` 取 provider 的稳定短码（如 `ds` / `gpt` / `mm`，≤ 4 字符）。
- **首版消息分隔符**（**工程参数**）：单条消息之间用**空行 `\n\n`** 分隔；正文内部空行与缩进**保留**（规范化只去整段末尾空白，不改语义，见[第 1.3 节](#13-规范化与摘要)）。

---

## 7. 本阶段**不动**的部分（避免范围蔓延）

- `serialize` 的**等 / 拒**语义（pending → 等、failed → 拒）
- chip **label 中性**；pending 不得称就绪
- G1-1 落地后的**退让顺序**
- 真实站点取数与登录态（属 P4，**不预写**）
- macOS 侧零痕迹回程（**已知缺口**，不是待验方案）
- 正文去重（P2 议题）、详情面板（P5）
- **不动内核安装包**；内核侧只用既有扩展接口

---

## 8. 验收项（P1 的硬性门禁）

1. **协议夹具**：Rust / JS 双端对同一组夹具结果一致；`schemaVersion` 常量一致。**实现侧另需对照夹具 `expected`**（见[第 6 节](#6-协议夹具与版本号)）——那是实现自测，与「双端一致」是两条断言，都要有。
2. **S-1**：同一条对话选两次 → 两个 `snapshotId` → 两个 `ref` → **第二次不覆盖第一次**（读记录不再以对话 id 为主键）。
3. **S-1 长度**：`ref` 长度 ≤ 24 可见字符（边界测试）。
4. **S-3**：落盘后**句柄状态由快照文件派生**；不得存在第二份内存真相（**机检：内存态字段只剩在途态**）。
5. **S-4（状态口径）**：**任何一次请求发生时**，该 occurrence 指向的快照已落盘，且**落盘正文与请求体逐字节相同**（比对磁盘与请求体，不是数时序点）。**（PM 21:5x 澄清 · 不要求唯一）** 内核会把**本轮用户输入在同一请求里重放多次**（实测 p1-r9：盘上正文在 `messages[1]/[6]/[8]/[10]` 命中 **4 处**，每处 76 字节、逐字节相同）——那是**内核的上下文组织**，**不是插件重复外发**。⇒ 判据只断「请求体里的正文与盘上正文**逐字节相同**」；**不得写「正文只出现一次」这类判据**（会被内核打红，且会把内核行为误当成插件缺陷）。**比「出现几次」更重要的是「每一处都逐字节相同」。**
6. **S-5**：快照缺失 / 指纹变化 / 摘要不匹配 → **请求数 0** + **可见错误**。**「chip 处于 `invalid` 态」在本阶段不可机检**（内核无该写入口，见 S-5）——**不计入本阶段验收**，登记为上游缺口 **G1-6**；**不得**把它降级写成「节点级断言」假装能测。**指纹比对的校验点在 host 命令层**（不是存储层），**当前指纹不可得 → 拒**（`account_changed`），不得放行。
7. **S-6**：进程重启后句柄仍可发；快照被清理后 → **拒**（`snapshot_invalid`），**不静默重读**。
8. **S-7**：同一正文引用两次 → 两个快照 id、**正文各送一份**（不隐式去重）。
9. **错误码**：14 个码**全部有稳定码名**；`catalog_empty` 与 `catalog_failed` 分离；错误里**不出现内部异常串**。
10. **命名空间**：A11——换账号 / 换实例 / dev-release 同开，目录、快照、请求、权限**不串范围**。
11. **隔离**：判据 `probe-profile-isolation` / `probe-data-directory` 仍绿；**新增代码不得让判据扫描面外泄**（新增文件若含建窗点，必须无条件给 `data_directory`）。
12. **G1-1…G1-7 缺口登记仍在**：实现**不得假设**这些缺口已解决；任何依赖「成功才插就绪 chip」的验收项按降级形态写。**G1-6 = 内核无「chip 进入 `invalid` 态」的写入口**（`ReferenceInsert` 无 `invalid` 字段、`setInvalid` 全树零调用方、DOM 无 `data-invalid`）——本阶段按「发送时拒 + 可见错误」验收，**该缺口不因本阶段验收通过而关闭**。**G1-7 = 通道被上游一行卡住**（`dsh-client-connection@0.2.1-rc.2` 的 `lib/index.js:871` 用**属性访问** `owner.webServer.register(route)`，而内核自己在 `:745`/`:778` 用的是 `this.owner.get("webServer")` ⇒ 第三方插件调 `rpc.handle` 必抛 `cannot get property "webServer" without inject` ⇒ **host 半边不激活、RPC 通道整体不通**，浏览器侧表现为 **HTTP 405**（不是 404）；**插件侧无法自修**，已实测证伪「declare `webServer` 就能修好」）。**实现侧须登记 `capabilities.degraded.g1_7`**，让通道不可用时**显式降级可见**、不静默。
    - **（PM 22:2x 上游修复曾提交到 fork → izzy 23:0x 已撤回）**：修复曾落到 `July-X/deepseek-harness` 的分支 `fix/connection-owner-webserver-get`（commit `aa7404de` → 重建为 `07aa67ac`，**内容零变化**，基于 upstream `master@687f282e`＝`release dsh@0.2.1-rc.2`）；**izzy 23:0x 拍板「撤回这个 PR，禁止修改上游的 dsh 内核」** ⇒ **该 fork 分支已删除**（`develop`/`master` 保留），**不再向上游提任何 PR**、**不再推动上游发版**：把 `packages/client/connection/src/rpc-host.ts` 里的**属性访问** `owner.webServer.register(route)` 改成 `owner.get('webServer').register(route)`——**与内核自己读 webServer 的写法一致**（`admit` / `isTrusted` / `authorizeConnect` / `authorizeIndex` 全用 `this.owner.get('webServer')`）。**根因在 cordis 源码层面坐实**（现役 `0.2.1-rc.2` 的 `@deepseek-ai/cordis/lib/index.js`）：`ctx.get(name)` 走 `this.ctx[symbols.isolate][name]` + `this.store`＝**服务注册表**（跨 scope 可达），而**属性访问**走**本 scope 的注入链**（`:887-893` 的 Proxy get）⇒ connection 插件自己的 scope 从不 inject `webServer` ⇒ 属性访问必抛。**已撤回**（izzy 23:0x）：不再走 GitHub 网页 compare 流程；fork 上那个分支已删除。⇒ **G1-7 仍不关闭**，且**不再以「等上游发版」为收敛前提**：通道**长期**走 exact fetch route（已真机全绿），`rpc.handle` 路径**不使用、不修**。
13. **通道可达性（A0 / A0s）**：
    - **A0s（仓库级，静态）**：host 半边**不得**用 cordis `ctx.set` 挂服务（那条路在浏览器树里必然失败，见[第 5 节](#5-命名空间与桥)）；**不得**把 `rpc.handle` 当作已通的通道（v0.4 已证伪）；**必须**注册 **exact fetch route**（路径在 `/api` 命名空间下，owner 准入由内核做）。**这条是仓库级可机检的**——它直接钉住 A0 那个真洞的根因与修法，防止有人把通道改回 `rpc.handle`。
    - **A0（真机级）**：client → host 经内核 `connection` 服务的**真实路由注册**跑通一次（**不得直连 handler 绕过注册**）。**仓库级 runner 跑不了这一条**（需要真实内核进程与真实 cordis 树）→ 仓库级如实报「**未执行**」并写明原因，**由真机复验轮执行**。**真机复验必须在**未打补丁的**现役内核上跑**——打过补丁的隔离副本只能证明「补丁有效」，不能证明「产品可用」。

---

## 9. 待 izzy 拍板（不阻塞实现，但影响后续验收口径）

- 站点登录态：**首次建立**（ChatGPT 优先）——P4 前置，不预写。
- 安装版 release 壳：按分工由 izzy 验收。
- **上游内核一行修复（v0.4 新增，**izzy 23:0x 已决定不采用**）**：`dsh-client-connection@0.2.1-rc.2` 的 `lib/index.js:871` 把 `owner.webServer.register(route)` 改成 `owner.get("webServer").register(route)`（与内核自己 `:745`/`:778` 的写法一致）⇒ 第三方插件的 `rpc.handle` 就能用。**隔离副本注入一行已实测有效**（P1 六条硬口径在补丁副本上 11 通过 / 0 失败 / 0 未执行）。**izzy 23:0x 拍板：禁止修改上游 dsh 内核、不向上游提 PR** ⇒ **本条作废为「不采用的备选方案」**；通道**长期**走 exact fetch route（**不等上游、不阻塞 P1**）。
- 证据是否固化进仓库：已拍板「可以清」——**本文档与代码里的数字、字段值、判据口径**才是可长期复核的东西。
