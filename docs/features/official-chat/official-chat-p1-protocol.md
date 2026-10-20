---
description: "P1 协议与状态机冻结稿：固定数据模型、句柄与快照的对应关系、错误码、门控口径与首版常量。PM 定，devloper 依此实现。"
kind: "protocol"
status: "P1 协议冻结稿 v0.1（2026-10-20 17:1x，PM 定；devloper 依此实现，tester 依此出判据）"
date: 2026-10-20
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
| `captureResult` | 捕获结果 | `ok` / `partial` / `failed` + 省略事实 + 未支持内容清单 |
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

### S-5｜失效与缺失：**一律拒 + chip 标 `invalid`**

- 快照缺失 / `sourceFingerprint` 变化（换账号）/ codec 失联 / `schemaVersion` 不支持 / 摘要不匹配 → **拒**（对应模型请求数 = **0**）+ **可见错误出口**。
- **用内核现成的 `invalid` 位，不自造**（tester 建议）：
  - `dsh-client-ui-conversation/lib/client/types/client/contract/input.d.ts:123` `readonly invalid?: boolean`（`ReferenceInsert` **插入时**就能带）
  - `types/client/input/editor/chip-node.d.ts:19` `invalid: boolean`；`:35` 注释「Owner-resolution failure flag: chip renders invalid; serialization must fail」
  - ⇒ 失效时 chip 进入 `invalid` 态 → **失效在界面上可见**；只静默拒，用户看到的是「引用凭空消失」。
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

- 每个错误码带：可重试性、用户下一步、关联 `requestId`、**脱敏**日志路径；**不得把内部异常串当成功值**（计划 §6）。
- `catalog_empty` 与 `catalog_failed` **必须分开**（计划 §6 明写「目录没有结果与目录读取失败是不同状态」）。

---

## 4. 首版常量（**工程参数**，不伪称来自站点限制）

| 常量 | 首版值 | 备注 |
| --- | --- | --- |
| 目录页大小 | 50 条 / 页 | |
| 目录缓存期限 | 5 分钟 | 换账号即失效 |
| 搜索去抖 | 300 ms | |
| 读取超时 | 30 s | G1 桩模型门控实测到 ~8 s，留余量 |
| 最大正文字节 | 256 KB | 超过 → `body_too_large` |
| 最大响应字节 | 1 MB | |
| 快照保留期 | 7 天 | 超期按 `snapshot_invalid` 拒 |
| 缓存总量上限 | 512 MB / 实例 | 含目录缓存与快照 |
| 单消息引用数上限 | 8 条 | |
| 单条正文预算 | 64 KB | 进入模型输入的**单条**上限 |
| 单次消息总预算 | 256 KB | **含原生 `@session`**，自有来源不得绕过总预算（计划 §7） |
| `schemaVersion` | 1 | |

常量**集中配置**并有边界测试（计划 §6）。

---

## 5. 命名空间与桥

- 服务端从**已绑定实例、壳模式、实际站点账户**推导命名空间；**客户端传入的会话标识用于一致性校验，不构成授权**（计划 §6）。
- 桥**不接受**任意路径、任意脚本、任意 URL、调用方指定的账户目录。
- 客户端命令职责：`capabilities` / `catalog` / `capture` / `cancel` / `preview`；host 回环路由**只按固定 id 读快照**与必要的模型可见投影。名称在实施时统一登记。
- 复用现有桥的**鉴权与轮换**，**不复制监听器**；新增 GET 查询路由用精确路径匹配 + 参数校验；若必须加请求体协议，先补**定长读取与大小上限**，不得复用只读到请求头的 `body_prefix`。
- 路径：目录缓存与正文快照**分开**；所有路径**跟随后端路径解析**（按壳模式、内核族、实例、站点账户隔离），**不在代码中展开真实用户 home**（计划 §8）。
  - 目录缓存：`<实例 official-chat 根>/catalog-cache/<provider>/`
  - 正文快照：`<实例 official-chat 根>/snapshots/<provider>/<snapshotId>.json`

---

## 6. 协议夹具与版本号

- **同一组 JSON 夹具**，Rust 与 JavaScript **各跑一遍**，断言「同一夹具 → 同一解析结果」；`schemaVersion` 两端常量必须一致。
- 夹具至少覆盖：正常快照、部分内容、超限、空目录、目录失败、超时、取消、账户已变、快照缺失、版本不支持、摘要不匹配。
- 快照文件**不可变**：同一 `snapshotId` 不重写；重新选择同一对话 → **新** `snapshotId`（计划 §8）。

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

1. **协议夹具**：Rust / JS 双端对同一组夹具结果一致；`schemaVersion` 常量一致。
2. **S-1**：同一条对话选两次 → 两个 `snapshotId` → 两个 `ref` → **第二次不覆盖第一次**（读记录不再以对话 id 为主键）。
3. **S-1 长度**：`ref` 长度 ≤ 24 可见字符（边界测试）。
4. **S-3**：落盘后**句柄状态由快照文件派生**；不得存在第二份内存真相（**机检：内存态字段只剩在途态**）。
5. **S-4（状态口径）**：**任何一次请求发生时**，该 occurrence 指向的快照已落盘，且**落盘正文与请求体逐字节相同**（比对磁盘与请求体，不是数时序点）。
6. **S-5**：快照缺失 / 指纹变化 / 摘要不匹配 → **请求数 0** + **可见错误** + **chip 处于 `invalid` 态**（节点级断言，不是文本级）。
7. **S-6**：进程重启后句柄仍可发；快照被清理后 → **拒**（`snapshot_invalid`），**不静默重读**。
8. **S-7**：同一正文引用两次 → 两个快照 id、**正文各送一份**（不隐式去重）。
9. **错误码**：13 个码**全部有稳定码名**；`catalog_empty` 与 `catalog_failed` 分离；错误里**不出现内部异常串**。
10. **命名空间**：A11——换账号 / 换实例 / dev-release 同开，目录、快照、请求、权限**不串范围**。
11. **隔离**：判据 `probe-profile-isolation` / `probe-data-directory` 仍绿；**新增代码不得让判据扫描面外泄**（新增文件若含建窗点，必须无条件给 `data_directory`）。
12. **G1-1…G1-5 缺口登记仍在**：实现**不得假设**这些缺口已解决；任何依赖「成功才插就绪 chip」的验收项按降级形态写。

---

## 9. 待 izzy 拍板（不阻塞实现，但影响后续验收口径）

- 站点登录态：**首次建立**（ChatGPT 优先）——P4 前置，不预写。
- 安装版 release 壳：按分工由 izzy 验收。
- 证据是否固化进仓库：已拍板「可以清」——**本文档与代码里的数字、字段值、判据口径**才是可长期复核的东西。
