---
description: "P2 协议：重复识别、可见提示与预算预检的口径（第一批）。PM 定，devloper 依此实现，tester 依此落判据。"
kind: "protocol"
status: "P2 协议冻结稿 v0.1（2026-10-20 22:5x，PM 定；izzy 22:5x 点头 P2 立项）"
date: 2026-10-20
---

# P2 协议：重复识别 ＋ 可见提示 ＋ 预算预检（v0.1）

上游依据：[调研载体](official-chat-p2-research.md)（结论与依据）＋ [P1 协议](official-chat-p1-protocol.md)（数据模型、13 码闭集、首版常量、命名空间）＋ [development-plan](official-chat-development-plan.md) §7（引用展开）§8（快照与回收）。

**本稿只覆盖 P2 第一批**（izzy 22:5x 点头立项时采用的建议三条）。**明确不做**的见 §6。

## 0. 一句话

P2 第一批 = **识别「同一段正文被引用多次」→ 告诉用户 → 在超预算前提醒**；**不合并、不删 chip、不改 id、不改消息**。

## 1. 重复组

### 1.1 键

重复组的判定键 = **(accountScope, sessionId, branch, capturedAt, contentDigest) 全等**。

- 五字段**任一不同 ⇒ 不是同一组**。
- `capturedAt` 不同 ⇒ 不合并（哪怕正文逐字节相同）——两次不同的捕获事实。
- 跨会话 / 跨账号 / 跨分支 ⇒ 不成组。
- **依据**：development-plan §8 `:168`「去重不能抹掉账户、分支或捕获事实」；内核同生态先例（`dsh-agent-instructions/README.zh.md` SHA `C31D31C892F81B53F0BB7AF409289C3868321D80B939EE7568D552B6A83088CD` `:222`）也是「**同一组 ＋ 字节完全一致才折叠**」——本稿沿用其严格方向。

### 1.2 组标识

`groupKey` = `sha256(accountScope \n sessionId \n branch \n capturedAt \n contentDigest)` 的**前 16 位小写十六进制**。

- 用途：client 侧把同组引用聚在一起并给稳定标识（用于提示文案与判据比对）。
- **不用于授权**：client 提交的 `snapshotId` 仍由 host 逐个核对（照 P1 `capture`/`preview` 的核对口径），**不因同组而免核对**。

### 1.3 组内成员

每个成员 = `{ snapshotId, capturedAt, contentDigest }`。**成员不合并**——组只是「这几条是同一份」的**事实分组**，不是「它们可以互换」。

## 2. 通道与命令

### 2.1 通道

与 P1 相同：host 命令层，经内核 `connection` 服务的 **exact fetch route**（`/api/xlink-official-chat`，POST，buffered）——**不得**把 `rpc.handle` 当作已通（缺口 G1-7）。

### 2.2 命令：`duplicates`

- **入参**：`{ sessionId, snapshotIds }`——`snapshotIds` = **client 传入当前草稿里已选中的引用 id 列表**（由 client 掌握草稿状态；host **不猜**草稿）。
- **出参**（成功）：

```
{
  ok: true,
  groups: [
    { groupKey, members: [ { snapshotId, capturedAt, contentDigest } ], redundantBytes }
  ],
  budget: { limitBytes, projectedBytes, wouldExceed, exceedByBytes }
}
```

  - `redundantBytes` = `(members.length - 1) × 单条正文字节`（该组若只送一份能省下的字节数；**只是数字，不真的省**）。
  - `budget.limitBytes` = 单次消息总预算 **262144**（P1 §4 常量）。
  - `budget.projectedBytes` = 本条消息的正文总字节（**按引用次数各送一份**计，与 P1 S-7 一致）。
  - `budget.wouldExceed` = `projectedBytes > limitBytes`；`exceedByBytes` = 超出量（未超则为 0）。
- **出参**（失败）：沿用 P1 §3 的 **13 码闭集**与 8 字段错误结果对象（`ok:false, code, retryable, userNext, requestId, logPath, detail`）。

### 2.3 失败口径

| 情形 | 码 |
| --- | --- |
| 某个 `snapshotId` 不存在 / 已过期 / 摘要不符 | `snapshot_invalid` |
| 当前指纹不可得或与快照不符 | `account_changed` |
| 盘上快照不可读 | `snapshot_store_unavailable` |
| 入参形态不对（`sessionId` 缺失、`snapshotIds` 非数组） | `invalid_request` |

- **不新增错误码**（13 码是闭集）。
- **`duplicates` 失败不得阻塞发送**：失败 ⇒ 提示「无法核对重复」＋ 可见错误，**正文照发**（P1 的发送语义不变）。

## 3. 可见提示（**硬要求**，界面形态由 UX 出手）

### 3.1 什么时候必须出现

1. **重复发生时**：用户**选中第二个及以后的同组引用**时（即同组成员数 ≥ 2）⇒ 立即出现提示。
2. **发送前**：若 `budget.wouldExceed` ⇒ 出现预算提示。

### 3.2 提示必须满足的约束

- **可见**：用户能看见（不接受「只写进日志」「只记进 console」）。
- **可复查**：提示承载位在引用发生处或输入区，**不接受瞬态 toast**（瞬态会消失、判据易假红、用户来不及读）。
- **可操作但不必可消除**：提示要说清「同组有几条」「省得下/超了多少字节」；**不得**提供「一键合并」（本批不做合并）。

### 3.3 不得做的事

- **不得**自动删 chip、**不得**改 chip 数、**不得**合并引用（S-7：正文按引用次数各送一份）。
- **不得**截断正文来绕过超限（截断＝静默裁掉，撞 S-7）。

## 4. 预算预检

- 触发点：**发送前**（与正文展开同一次 host 往返里算，不额外起一趟）。
- 口径：`projectedBytes` = 本条消息**各引用各送一份**的总字节（P1 S-7 口径）＋ 原生 `@session` 占用。
- 超限时：**给可见提示 ＋ 不截断**；由用户自己决定删哪条引用。

## 5. 验收判据（**tester 依此写**）

| # | 判据要点 | 期望 |
| --- | --- | --- |
| P2-1 | 同组（键全等）⇒ 仍**两个不同 `snapshotId`**、都落盘、都可发 | **不得**退化成复用旧 id |
| P2-2 | 同组成立 ⇒ 提示**可见且可复查**（承载位在引用处/输入区，非瞬态） | 提示含同组条数与字节数 |
| P2-3 | `capturedAt` 不同 ⇒ **不成组** | 负例 |
| P2-4 | `accountScope` / `branch` 不同 ⇒ **不成组** | 负例 |
| P2-5 | `projectedBytes > 262144` ⇒ 发送前出现预算提示，**正文未被截断** | 请求体正文逐字节等于各引用原文 |
| P2-6 | `duplicates` 失败 ⇒ 可见错误 ＋ **请求仍照发** | 失败不阻塞发送 |

- **夹具**：新增「**去重候选对**」夹具（两条快照 ＋ 期望是否成组），登记进 `fixtures/protocol/index.json` ⇒ **两端都读**。**键的形状以本稿 §1.1 为准**（已定稿，可写）。
- **判据数的是「判据自建的串」还是「真机请求体」**：P2-5 必须断**请求体逐字节**（与 P1 A5 同法），**不得**写「正文只出现一次」这类会被内核重放打红的判据。

## 6. 本批**明确不做**

1. **不做消息级自动合并**（不隐式去重；合并必须可见才允许，且本批不提供）。
2. **不动 chip 数**。
3. **不碰 id 分配**（host `seq-claims/` ＋ `snapshots/` 两份取大，一个字不动）。
4. **不在 `agent/pre-step` 里改消息**（那里只做引用展开，development-plan §7 `:149`）。
5. **不新增错误码**（13 码闭集）。
6. **不新建索引表**（键的五字段都已在快照里）。

## 7. 与 P1 的关系

- P1 的 S-1（两个 id）/ S-3（状态派生）/ S-4（逐字节）/ S-5·S-6（拒 + 可见错误）**全部继续有效**，本稿不放松任何一条。
- **A8（`A8-no-body-dedup`）本批不需要改**：A8 数的是判据自建的 `${bodyA}\n\n${bodyB}`（`scripts/lib/p1-protocol.mjs:1126-1127`），**不观测内核真实请求体**；本批不做合并 ⇒ 正文仍各送一份 ⇒ A8 继续绿。**只有将来 izzy 决定做消息级合并时**，A8 才必须同次改口径。

## 8. 待 izzy 拍板（**不阻塞第一批实现**）

1. 「可见提示」的最终界面形态（UX 出方案后定稿）。
2. 将来是否做消息级合并（若做 ⇒ 需 A8 同次改口径 ＋ 合并必须对用户可见 ＋ 本稿 §6.1 撤除）。
