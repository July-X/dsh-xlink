---
description: "P0 可行性结论：G1/G2/G3 的实际执行结果、接口缺口、协调者裁定与是否可以继续。只填实际执行结果。"
kind: "feasibility"
status: "G1/G2/G3 的机制层结论已出；真实链路（壳真实窗口、已登录态、正文读取）未闭合"
date: 2026-10-20
---

# 官网对话直接引用 · P0 可行性结论

本文只记录**已经实际执行并留下证据**的结果。规则：未执行的一律写「未执行」，尚未出的结论不预写「通过」，跳过不计为通过。实施规则见[开发计划](official-chat-development-plan.md)，方案与待验证清单见[调研](official-chat-bridge-research.md)；本文件不改那两份文档，冲突项集中记在[第 7 节](#7-文档与实测的出入)。

后续每阶段的事实进度记录在 [official-chat-progress.md](official-chat-progress.md)。

## 1. 结论摘要

| 验证项 | 状态 | 一句话结论 |
| --- | --- | --- |
| G1 内核工作台的选择与发送控制 | **通过（4/5 问通过，第 5 问判定为互斥并已选型）** | 现有扩展接口足以支撑「@ 选择 → chip 落地 → 发送时读取正文 → 等待或拒绝」；「成功才插就绪 chip」在现接口下不可达。**证据有版本**：结论取自 12:21–13:31 的三轮运行，差异见[第 7 节](#7-文档与实测的出入)第 6 条 |
| G2 异步 JavaScript 回程 | **通过（5/5）** | 延迟成功 / 拒绝 / 超时 / 导航 / 页面销毁都能拿到分类正确的结果 |
| G2 首站只读探测 | **部分通过（探测执行完毕，取数结论未成立）** | 目录接口与有效 origin 已测出，但**认证上下文不自动满足**，两站登录态已失效，真实目录字段与正文读取仍未验证 |
| G3 官网窗口未打开时仍可读取 | **机制层通过（7/7，条件受限）**；**真实官网窗口 + 已登录态链路未执行** | 7 个场景在「未登录 + 模拟页签」条件下全通；「用户开着官网窗口时壳复用它的页签」「真实账号切换」未验 |

**是否可以继续：可以继续到 P1（协议与隔离），不可以进入 P4（首站接入）。** 先决条件见[第 9 节](#9-是否可以继续)。

一句话版本：机制层面已经成立，可以开始把协议和状态机固定下来；真实站点层面还差「用户重新登录」这一道人工前提，以及 G3 在壳的真实窗口 + 已登录态下的复跑。

## 2. 环境与版本

| 项 | 值 |
| --- | --- |
| 内核 | release **0.2.1-rc.2**（dev 为 0.2.1-alpha.2；本轮全部用 release） |
| 壳 | dsh-xlink，仓库 `C:\workdir\github\dsh-xlink`，branch `main` |
| G1 驱动对象 | Edge `--headless=new --remote-debugging-port=9334`（12:21 那轮） / `9341`（13:26 真链重跑） / `9335`（13:49 壳真实窗口复验）驱动**内核自带工作台页面** `http://127.0.0.1:<port>/?token=…` |
| G1 隔离 | 内核跑在**副本** `%TEMP%\dsh-xlink-g1\dsh\desktop\kernels\0.2.1-rc.2`（528.7MB）；`DSH_HOME`/`DSH_XLINK_HOME` 均在 `%TEMP%\dsh-xlink-g1\` 下；**未碰** `C:\Users\zxx\.dsh-xlink` |
| G1 模型 | 本机桩模型（OpenAI 形状，`%TEMP%\dsh-xlink-g1\stub-model.mjs`），profile patch 加 `llm-pi-ai` 路由 `g1-stub` + `agent-default-model` 指过去。**不接真实官网、不用付费模型**；正文为固定夹具 `g1-alpha` / `g1-beta` |
| G1 启动命令原文 | `node %TEMP%\dsh-xlink-g1\dsh\desktop\kernels\0.2.1-rc.2\node_modules\@deepseek-ai\dsh\lib\bin.js web --no-open --port 3199`（env：`DSH_HOME` / `DSH_XLINK_HOME` / `G1_STUB_KEY`；13:26 那轮端口 3201/3202，桩模型 3221/3222，CDP 9341/9342） |
| G1 物化方式 | **两版并存**：12:21–13:06 那轮**按源码手工复刻**壳的物化形状到 `<DSH_HOME>/extensions/builtin/xlink-official-chat/0.1.0/g1-local/compat-a/`；**13:26–13:31 那轮（`delivery3.md`）改走壳的真实 Rust 物化链**——源 = 打包资源 `src-tauri/resources/builtin-plugins/official-chat`（`npm run prep:builtin` 产物，`verify_manifest` 一并跑到），指纹 `dsh-0.2.1-rc.2-cordis-4.0.5`（由真实内核树算出），物化目录 `<DSH_HOME>/extensions/builtin/official-chat/0.1.0/dsh-0.2.1-rc.2-cordis-4.0.5/compat-a/`，接线行由 `wiring.rs::row_block` 写进 `<DSH_HOME>/profiles/web/cordis.patch.yml` |
| G2 回程环境 | Windows；本地可控页 `127.0.0.1`，不接触真实站点 |
| G2 站点探测环境 | profile `C:\Users\zxx\.dsh-xlink\dsh\desktop-dev\webview-official-chat`（**在用户真实数据目录内**，见[第 7 节](#7-文档与实测的出入)第 2 条） |
| G3 探测环境 | 源 profile 同上，**先只读拷贝到 `%TEMP%` 一次性目录**再跑（拷贝 730 文件 / 59,632,751 字节 / 跳过 0），站点访问全部发生在拷贝上 |
| 门禁自查 | `cargo fmt --check` 通过；`clippy --locked --all-targets -D warnings` 零警告；`cargo test --locked` **1108 passed / 0 failed**；`check:invariants` 全绿；`prepare-builtin-plugins --check` exit=0（4 插件 / 38 文件） |
| 插件单测 | `node --test plugins/official-chat/test/*.test.mjs` → **26 passed / 0 failed**；`npm run test:plugin` → 206 tests / 203 pass / 0 fail / 3 skip；`npm run test:probe`（Q1/Q2 行为判据）→ 6 passed / 0 failed |

内核行号引用一律带版本号（`0.2.1-rc.2`）与文件；本轮实测涉及的关键位置：`input-trigger/client.js:1459-1473`（`settle()` 同步调 `onPick`）、`:1364-1380`（`execute()` 只认 `claim`/`text`/`insert`）、`:1251`（`codec.serialize` 装配）、`:1260-1266`（`openReference`）、`:1556`（`registerSource`）；`ui-conversation/client.js:15501`（取 occurrences）、`:15511`（`await serializeReference`）、`:15309-15315`（`insertReference` 的 draftRev CAS）、`:15805`（`bail`）、`:19709-19722`（`InputActions.captureInsertion`，渲染层 slot prop）。

## 3. G1：真实工作台内的选择和发送控制

执行者：devloper。交付原文：`%TEMP%\dsh-xlink-g1\delivery.md`（首版）、`delivery2.md`（按当前代码重跑）、`delivery3.md`（**走真实 Rust 物化链后的终版**）。证据在 `%TEMP%\dsh-xlink-g1\`：`final.out.txt`（终检 A–F 六段）、`reject.out.txt`（拒绝提示密集采样 + 草稿节点结构）、`final.json` 与 `reject-notice.json`（真链重跑产物）、`q2v2.out.txt`、`q4-codec-missing/`q4-service-missing/`q4-uninstalled/`q4-host-import-failure/`q4-svcprobe/`q4-notice` 的 `.out.txt`、`q5.out.txt`、`q5b.out.txt`、`model.log`（桩模型收到的每个请求体，**已被 13:56 的空跑覆盖，见[第 7 节](#7-文档与实测的出入)第 6 条**）。

代码形态：PM 2026-10-20 拍板 P0 只做一条路（**甲**），插件已收敛为**单变体**——`onPick` 同步返回 pending 句柄、正文由 `codec.serialize` 在发送时取、不再自插 chip。下列标注「备选形态」的测量是**该形态被否掉的实测依据**，其代码已按拍板删除；其余结论都按**当前代码**重跑过。

### Q1 异步插入 — **通过**

- 走的接口：`onPick` **同步**返回 `{insert:{source,ref,label,appearance,clipboardText}}` → 内核 `execute()` → `actx.bail(actx,'slash/input-insert-reference',{reference,span})` → 会话侧 `shell.insertReference(ref,span)`（`ui-conversation/client.js:15805` / `:15309-15315`，span 做 draftRev CAS）。
- 怎么验的：`final.out.txt` 的 `B-chip-lands` 段。实测 pick 后 chip **同一拍**落地，HTML 是内核自己的节点 `<span data-composer-chip="official-chat" contenteditable="false" data-lexical-decorator="true">`，**没有 DOM 覆盖编辑器**；`bodyInDraft:false`（正文不进草稿）。
- 草稿里没有隐藏的待完成占位符：按**节点结构**核过（`reject.out.txt` 的 `draftNodes`）——草稿只有 1 个元素节点（内核自己的 chip：`SPAN[data-composer-chip=official-chat] > SPAN > svg + SPAN.label`）加 1 个用户文本节点，**没有第二个「待完成」节点**。
- 备选形态（读取完成后自己插 chip）**实测被拒**：读取期间继续编辑（草稿 `@` → `@X`）后插入返回 `ok:false`，页面出现可见提示「旧位置插不进引用（draftRev CAS 被拒）」。这是该形态被否掉的实测依据。
- 有无其它锚点：**没有**。取 fresh span 的 `InputActions.captureInsertion()` 是**渲染层 slot prop**（`ui-conversation/client.js:19709-19722`），`UiSession.sourceFor`/`materialize` 是 private，第三方 client 插件唯一公开的只有 provide（提供侧）。

### Q2 发送门控 — **通过（等待与拒绝都实测到；漏发只出现在被否形态）**

- **等待**。怎么验的：`final.out.txt` 的 `D-send-wait` 段 + `model.log`。chip 在草稿里 + 读取要 8 秒 + 按回车 → 内核立刻 `await codec.serialize`；时间线 t=281ms 时探针末尾已是 `serialize-begin`，t=281~6100ms **桩模型 0 请求**；读取完成时 `serialize-ok`；**t=7624ms** 请求到达且**请求体含夹具正文**（`requestHasBody:true`，真链重跑轮次 7439ms，另一轮 7176ms，结论一致）。即挂起约 7~8 秒才发，不漏发。
- **拒绝**。怎么验的：`%TEMP%\dsh-xlink-g1\reject.out.txt`（250ms 密集采样）。读取失败按回车 → `serialize` 抛错 → **0 请求**、**草稿保留**（`draftRetained:true`）、页面出现可见错误「G1: 固定读取失败（failRead=1）」。提示是**短暂**的：出现于 **1340ms**，持续到约 **5031ms** 后自动消失（真链重跑轮次 1325ms→4994ms，一致）。
- **漏发**（只在被否形态里）。草稿里**没有 chip**（读取未完成）时按回车 → 内核走 `occurrences.length===0` 分支，**立刻**把纯文本发出去（1.5 秒内 2 个请求），标题请求里用户消息只有触发文本，**夹具正文没进消息**，界面零提示。
- 结论：漏发只发生在「chip 还没插进草稿」的窗口。甲形态（chip 先落、正文发送时取）把这条窗口关掉了。

### Q3 插入时机 — **失败（判定为互斥）**

- 同步约束：`settle()` 同步调 `onPick` 并同步 `execute()`（`input-trigger/client.js:1459-1473`），`execute()` 只认 `claim`/`text`/`insert` 三个键（`:1364-1380`）→ `async onPick` 返回 Promise 会掉进 insert 分支插 `undefined`。
- 互斥推导：发送路径上唯一能拦住「有 chip 但正文未就绪」的钩子是 `codec.serialize`，而它**只在草稿里已有本来源 occurrence（即 chip）时才被调用**；chip 存在 ⇒ 发送会等/拒（不漏发）；chip 不存在 ⇒ 内核把纯文本直接发出去（引用被静默丢弃＝漏发）。要「正文校验+快照保存都成功才插就绪 chip」就**必然**落进后者。
- 判定：这是**现有扩展接口下的互斥**，不是实现缺陷。解它的唯一路径是上游改动（缺口 G1-1）。
- 产品落点：见[第 6 节](#6-裁定与理由)裁定 4。

### Q4 失败可见性 — **通过（三类情况可见性完全不同）**

| 情况 | 可见性 | 怎么验的 / 证据 |
| --- | --- | --- |
| codec 缺失 | **用户可见**。发送被阻断（0 请求）+ 草稿保留 + 页面出现「slash: no serializer for reference source "official-chat"」，**281ms** 就出现并持续可见（250ms 密集采样覆盖的前 3.2 秒每次都命中）。探针与内核日志都干净——错误来自内核 | `q4-codec-missing.out.txt`、`q4-notice.out.txt` |
| 服务缺失（inject 名写错） | **内核有诊断，但代价是整个 web 工作台起不来**。页面显示「HARNESS / Failed to load plugins / web boot: 1 entry did not activate / xlink-official-chat: pending (waiting for service: g1ServiceThatDoesNotExist)」，console 同文案，composer 不出现 | `q4-service-missing.out.txt`、`q4-svcprobe.out.txt` |
| 插件未接线（＝卸载） | **完全静默**。@ 菜单里没有那一组，内核日志一个字都没有（插件根本没注册，内核无从报告）。**三类里唯一没有诊断的** | `q4-uninstalled.out.txt` |
| host 半边激活失败 | 内核日志有「dsh: warning: 1 entry did not activate」+ 堆栈，但**界面无提示**，而且**客户端半边照常加载并注册了 @ 来源**（功能其实还在） | `q4-host-import-failure.out.txt` |

独立检查路径：@ 菜单里那一组是目前**唯一**可用的独立信号（页面上看得见，可与插件自报脱钩），但**区分不了**「没装 / 装不上 / 服务缺失 / codec 缺失」。内核没有 per-plugin 注册回执 API，壳的插件开关只看 entry 在不在，界面会显示「已启用」而功能其实没加载。

### Q5 失效恢复 — **通过（安全失败 + 可恢复）**

- 卸载（摘接线行 → profile reload）后：@ 菜单里那一组**消失**（独立信号有效；干净草稿下复核：菜单其它行还在，就是我们这一组没了），**草稿里的既有 chip 仍在**。
- 此时按发送 → **被阻断**：0 请求 + 可见错误「slash: no serializer for reference source "official-chat"」+ 草稿保留 → **安全失败**（不漏发、不静默丢内容）。
- **重新装回**后：@ 菜单里那一组**回来**（可重选），但**既有 chip 仍不可用**——新模块实例没有那个 ref 的读取记录，发送被阻断并报「G1：找不到 ref "g1-alpha" 的读取」→ 用户必须删掉旧 chip 再重选。
- 失效 chip **能删**：光标紧贴 chip 后按 Backspace 即删（草稿「G1 固定对话 Alpha⏎ ABC」→「 ABC」，chip 消失）；Ctrl+A + Delete 清空整条也可用（`q5.out.txt`、`q5b.out.txt`）。

### G1 实际用到的扩展接口

| 接口 | 用途 | 依据（0.2.1-rc.2） |
| --- | --- | --- |
| `inputTriggers.registerSource(source)` | 注册 @ 来源 | `input-trigger/client.js:1556` |
| `source.onPick({candidate,session,span,action})` | 选中后**同步**返回 `{insert}` | `input-trigger/client.js:1459-1473` |
| `source.candidates(session,{query,quoted,drilled,signal})` | 候选行（自带分组标题） | `ui-reference/client.js:296-328` |
| `source.codec.clipboardText(ref)` | chip 在草稿文本里的投影 | `ui-reference/client.js:362-364` |
| `source.codec.serialize(ref, signal)` | 发送路径上的**异步闸门**（等/拒） | `input-trigger/client.js:1251`；`ui-conversation/client.js:15511` |
| `source.openReference(session, reference)` | 点 chip 报 pending/ready/failed | `input-trigger/client.js:1260-1266` |
| `conversation.input.for(actx).notify(level,text)` | 用户可见提示 | `ui-commands/client.js:1786` |
| `sessions.scope(sessionId)` | 取会话作用域 | — |
| 包形状 | `dsh.client.platform='web'` + `exports['./client']` + `dsh.client.inject=[包名]` | `dsh-client-modules/index.js:105-119,720-750` |

发送链路（实测）：选 @ 官方对话 → `onPick` 同步返回句柄 → `execute` → `bail(slash/input-insert-reference)` → `insertReference`(span CAS) → chip 落草稿 → 用户回车 → `ui-conversation:15501` 取 occurrences → 有 occurrence → `await serializeReference` → `owner.codec.serialize` → 我们的 `serialize` await 同一条读取 promise → 未完成=等、失败=reject → `settleDetachedFailure`（可见错误 + 草稿保留）→ 成功则把正文拼进草稿文本发给模型。

### G1 接口缺口（我们不改内核，只记录最小上游需求）

| 编号 | 缺口 | 解什么 |
| --- | --- | --- |
| **G1-1** | 内核 `await` `onPick` 的返回值再 `execute`，或新增 `onPickAsync` | **解 Q3 互斥的唯一办法**。有了它，「正文校验+快照保存都成功才插就绪 chip」与「不得漏发」才可能同时成立。代价：pick 路径变异步（菜单关闭时机、claim 语义要重定） |
| **G1-2** | 发送前的「引用未就绪」闸门：现在 `occurrences.length===0` 时内核把纯文本直接发出去，引用被静默丢弃且无提示。最小形式：source 能声明「有未完成引用」，内核据此拒绝并提示 | 解漏发窗口，不依赖 G1-1 |
| **G1-3** | 把 fresh span（`InputActions.captureInsertion`）开放给客户端插件（现在只是渲染层 slot prop，第三方插件读不到） | 解 Q1 的 CAS |
| **G1-4** | per-plugin 注册回执 + 客户端半边注册失败时**用户可见**的诊断 | 解 Q4 的回执缺口 |
| **G1-5** | 插件重载后既有 chip 的 ref 恢复契约（或 UI 明确把失效 chip 标成需重选） | 解 Q5 残留 |

## 4. G2：异步 JavaScript 结果与首站可读性

执行者：tester。证据在 `%TEMP%\dsh-probe\`：`A-controlled.json`、`B-deepseek.json`、`B-chatgpt.json`、`B-minimax.json`。

### A：异步回程 — **通过（5/5）**

怎么验的：`%TEMP%\dsh-probe\A-controlled.json`，本地可控页（自起 `127.0.0.1`，不接触真实站点）。

| 场景 | 期望 | 实测 |
| --- | --- | --- |
| 延迟成功 | 拿到表达式完成后的值 | **通过** — 7 轮读回，287ms，拿到完成后的结构化值 `{role:"assistant",value:42}` |
| Promise 拒绝 | 拿到 Rejected 分类与拒绝原因 | **通过** — 94ms，错误「页内请求被拒绝：probe-expected-rejection」 |
| 超时 | 超时退出并给出超时原因 | **通过** — 2006ms（阈值 2000ms），说明「页内 Promise 一直 pending」 |
| 导航 | 识别为页面被替换（Gone），不是空结果 | **通过** — 280ms，「页面文档已被替换（导航或重载）」 |
| 页面销毁 | 识别为页面不在了（Gone），不 panic、不无限挂起 | **通过** — 281ms，「结果通道已关闭（webview 被销毁）」 |

过程中**修掉一个真 bug**：WebView2 的 `TryGetResultAsString` **只在结果是字符串时才给值**，数字/布尔/对象一律返回空串。修法是页面侧统一 `JSON.stringify`，平台层只取回 JSON 文本、解析放共享层。

### B：首站只读探测 — **部分通过（探测执行完毕，取数结论未成立）**

只取目录元数据，**未读任何正文**。只记录 origin、接口路径、结构、请求数与错误码。

| 站点 | 有效 origin | 目录接口 | 认证是否自动满足 | 请求数 | 状态 |
| --- | --- | --- | --- | --- | --- |
| DeepSeek | `chat.deepseek.com` | `/api/v0/chat_session/fetch_page?count=5` 存在，结构 `{code,msg,data}` | **否** — 裸 fetch → `40002 Missing Token`；带 Bearer `userToken` → `40003 INVALID_TOKEN`；页面停在 `/sign_in` | 2 | **探测完成；登录态失效** |
| ChatGPT | `chatgpt.com` | `/backend-api/conversations?offset&limit&order&is_archived` → 200，结构 `{items,total,limit,offset}` | **否** — 裸 fetch 200 但 `items` 为空（**状态码不能当成功判据**）；凭据在 HttpOnly cookie | 1 | **探测完成；登录态失效** |
| MiniMax | **`agent.minimax.cn`**（入口 `agent.minimaxi.com` 实测 301） | **目录端点未验证** | 未登录 | 0 | **未执行取数** |

DeepSeek 侧 localStorage 命中 `userToken`（另有 `settingsJwt`、`__appKit_userInfo`），**值一律未记录**；可见 cookie 名称 `smidV2` 等同样只记名不记值。ChatGPT 侧 localStorage 只有草稿键，凭据在 HttpOnly cookie。

**Q1/Q2 判据已修**（PM 裁定，tester 复核通过）：token 按**显式优先级**取（`userToken` → `accessToken` → `sessionToken` → `_token`）并输出**实际试过的键名**（`tokenKeysTried` / `tokenKeyTried`）；`tokenUsable` 拆成 `tokenPresent`（存在性）与 `tokenAccepted`（服务端接受），`tokenKeyFound` 改名 `tokenKeyTried`，**旧字段名已删**。「服务端接受」= HTTP 2xx **且** 业务 code 成功 **且** 响应里确有内容。行为判据在 `src-tauri/examples/official_chat_probe_site.test.mjs`（6/6）：把页面脚本从 Rust 源里原样取出、按 `format!` 规则还原花括号，在 `node:vm` 的假页面里**真跑**，不靠源码字符串断言。

**阻塞**：两站登录态已失效，需用户在本机重新登录后重测；正文读取按规则阻塞（用户未指定测试对话）。MiniMax 的「已实测出 origin」是**事实**，但「支持」是另一回事——目录端点仍标未验证，本版不做，留 P7。

## 5. G3：官网窗口未打开时仍可读取

**判定：机制层通过（7/7，条件受限）；真实官网窗口 + 已登录态链路未执行。**

执行者：tester。命令：`cargo run --locked --example official_chat_probe -- --mode g3 --site deepseek --profile "<dev profile>"`。

**隔离证据（先看这条）**：`source_read_only:true`；拷贝 **730 文件 / 59,632,751 字节 / 跳过 0**；清理后 `source_deleted:false`、`source_still_exists:true`、`temp_copy_removed:true` —— **只删自己的临时拷贝，源 profile 仍在**。本轮所有站点访问都发生在临时拷贝上；跑完实测源 profile 的 `Cookies` / `Preferences` mtime 仍为 **13:24:48**（早于本轮跑批 30 分钟，不是本轮写的）。

| # | 场景 | 判定 | 实测要点 |
| --- | --- | --- | --- |
| 1 | 页签已挂载 → 复用 | ✅ | `source: mounted-tab`、`owned_by_shell: false`、profile 目录 == 官网 profile |
| 2 | 官网窗口关闭 → 按需建上下文且不抢焦点 | ✅ | `source: lazy-hidden`、`window_visible: false`、`owned_by_shell: true` |
| 3 | 尚未登录 → 明确报未登录 | ✅ | `decision: not-logged-in`、`landed_path: /sign_in` |
| 4 | 账号切换 → 旧上下文失效 | ✅ | 指纹 `69ab92b5a3a37915` → `a2ab669cc1246549` |
| 5 | 重定向 → 记录实际落点 | ✅ | `landed_origin: https://chat.deepseek.com`、`landed_path: /sign_in`、`landing_stable: true` |
| 6 | 所有权转移 | ✅ | `released_own_hidden_context: true`、`source_after_reopen: mounted-tab`、`user_tab_still_alive: true` |
| + | 闲置回收 | ✅ | 自建上下文 `idle_reclaimable: true`；**用户挂载页签 `idle_reclaimable: false`**（壳永不回收用户的页签） |

### 判定口径：7/7 是「机制层」，不是「链路」

本轮整轮跑在「**未登录 + 探针自建的模拟页签**」条件下，因此：

1. 场景 3 自己写明 `not-logged-in` → **「复用已登录态并读到目录」未验证**。
2. 场景 4 的「账号切换」是探针**自己改 localStorage 模拟**的 → 验到的是「指纹变化 → 旧上下文/缓存失效」这条**响应链**，**真实账号切换未验证**。
3. 场景 1/2/6 的「官网窗口开/关」是探针自建的**模拟页签**（`official-chat-tab-0`）→ **「用户开着官网窗口时壳复用它的页签」在壳的真实链路上未验证**。

⇒ 判定：**G3 机制层通过（7/7，未登录 + 模拟窗口条件下）；真实官网窗口 + 已登录态 = 未执行。**

### 顺带印证了一条判据边界

场景 3 里 `page_has_user_info: true` 而 `logged_in: false` —— **profile 里有 userInfo / userToken 键 ≠ 已登录**。这条边界与 G2 的 Q1/Q2 修法一致：键名只用来取候选，不代表服务端接受。

## 6. 裁定与理由

以下为协调者 2026-10-20 的裁定，写在这里作为 P1/P2 的硬性输入。

1. **形态 = 用户手动触发、逐会话、只读**。不做后台定时同步、不做全量正文预拉、不因键盘输入反复全量请求。依据调研 §2.5 / §6.4：只读不写就不碰签名与反自动化那一套。
2. **MiniMax 本版不做**，留 P7。已实测的 `agent.minimax.cn` 是**事实**，但**「支持」是另一回事**，调研 §7 #9 仍标未验证。
3. **R1 页面槽位方案不得直接带进真实站点**：它在页面 JS 空间留了一个真实存在的属性，`Object.getOwnPropertyNames(window)` 能列出来（不可枚举只挡 `for-in` 与 `JSON.stringify`），与 `harness/chat-fingerprint.js` 的既有纪律有真实冲突。首选方向是 `ICoreWebView2DevToolsProtocolController::CallDevToolsProtocolMethod("Runtime.evaluate", {awaitPromise:true, returnByValue:true})`（页面 JS 空间零痕迹、一步拿结算值），macOS 对应 `WKWebView.callAsyncJavaScript` 的 awaitPromise；备选 R2 本地回环中继；兜底才是 R1 槽位且必须写明残留与补偿。

   **评估结论（tester，2026-10-20 13:55 回；本节据此补完）**：

   - **R1 槽位在产品上站不住。** 机制在 `script_eval.rs:434-436`：`Object.defineProperty(window, SLOT, { enumerable:false })`。`enumerable:false` 只挡 `for-in` / `JSON.stringify(window)`；它**仍是 `window` 的自有属性**，`Object.getOwnPropertyNames(window)` 照样列得出来。名字前缀固定 `__dsh_rt_`（`:416`）→ 页面探针**扫前缀即可定位**，不必猜完整随机名。痕迹**短命**（取到即删）但**在读取窗口内确实存在**，与 `harness/chat-fingerprint.js`「页面探针必须看到普通浏览器里什么都没有」直接冲突。**结论：R1 只能用于探测，不得带进真实站点。**
   - **零痕迹路径在 Windows 上成立**：`--mode cdp` **5/5** —— `devtools_window_opened:false`（未开 DevTools 窗口也能调）、`Runtime.evaluate` 带 `awaitPromise:true` 拿到**结算后的值**（`{"role":"assistant","value":42}`，不是 Promise 对象）、不带 `awaitPromise` 得 `{}`（对照组）、拒绝时 `exceptionDetails` 与 `result` 同级、**页面 JS 空间零痕迹**（求值前后 `Object.getOwnPropertyNames(window).length` 不变，1239 / 1239，样本 `[]`）。
   - **接口形态提醒**：`CallDevToolsProtocolMethod` 是 **`ICoreWebView2` 自己的方法**，**不是** `ICoreWebView2DevToolsProtocolController`（WebView2 没有那个接口）。
   - **macOS 当前无通路** —— wry 0.57 `src/wkwebview/mod.rs` 只有 `evaluateJavaScript_completionHandler`（`:759` / `:772`），**没有** `callAsyncJavaScript` / `awaitPromise`；持有 raw `WKWebView` 的 `InnerWebView` 是 `pub(crate)`（`:132`，字段 `webview` 在 `:135`），外部 crate 拿不到 → 无法直接调 WKWebView 的 async 求值入口。**后果：macOS 要么退回槽位（把痕迹带回去），要么等 wry 上游暴露 async 求值。此项按 P0 未闭合项记，不与 Windows 并列成「两平台各一种」。**
   - **R2（本地回环中继）已被否**：计划 §4 要求「正文读取落在官方对话模块，**不再造一个监听服务**」，R2 要新起一个 127.0.0.1 监听端口，与该条直接冲突。
4. **Q3 互斥的产品落点 = 取「甲」形态**：pick 时插 pending 句柄，正文由 `codec.serialize` 在发送时取（等待或拒绝，绝不静默漏发）。**但 pending 句柄不得冒充「就绪」**：chip 的 label 保持中性，`openReference` 按 ref 如实报 pending / ready / failed，**「就绪」由 chip 状态与详情承担，而不是由「chip 是否存在」承担**。
   - 说明：开发计划 §2 要求「记录缺口与最小上游接口需求，**停止依赖该能力的产品实现**」。甲形态**不违反**它——本形态下 chip 是一个**真实引用句柄**（有 ref、有读取记录），发送时要么等待要么拒绝，**不静默按没有引用继续**；它**不是**「无正文的临时标签」、不是 DOM 覆盖、不是修改内核。
   - 调研 §4.2 的「只有正文校验与快照保存都成功，才插入就绪 chip」被**降级**为「**就绪状态必须可观测且不得冒充**」。这个降级**要明确写出来**，并附 **G1-1** 作为将来同时满足两者的唯一路径。
   - **落地要求**：上述纪律在 P1/P2 落为硬性验收项。

## 7. 文档与实测的出入

本节只记录出入，**不直接改**开发计划与调研原文。

1. **「验证成功后才插入就绪 chip」在现接口下不可达。** 出处：开发计划 §5 G1 第 3 条、调研 §4.2 第 1 段。实测为互斥（Q3），解法依赖上游 G1-1。按裁定 4 降级为「就绪状态必须可观测且不得冒充」。开发计划与调研的这两句在 G1-1 落地前**不能当作已满足的验收条件**。
2. **G2 站点探测用了用户真实数据目录下的 profile。** 开发计划 §12 要求「模型桩、站点夹具、**WebView profile** 和实例均使用独立临时目录」。实测证据文件记录的 profile 是 `C:\Users\zxx\.dsh-xlink\dsh\desktop-dev\webview-official-chat`——在用户真实数据目录内。本机实测该目录 `EBWebView\Default\` 下的 `Network\Cookies`、`History`、`Preferences`、`Secure Preferences` 的 mtime 均为 **2026-10-20 13:24:48**，即测试期内在写用户真实数据目录。**后续探测必须改用 `%TEMP%` 一次性 profile**；若需复用登录态，走**只读拷贝**（Windows Cookies 由 DPAPI 绑机器+用户、不绑路径）。**处理**：已落机检判据 `scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs`（10/10）；G3 那轮已按「只读拷贝 → 跑 → 删拷贝 → 断言源仍在」执行。
3. **开发计划 §3 阶段表的 P0 行仍是「待实施」。** 实际 G1/G2/G3 机制层已出结果，真实链路未闭合。该表在本轮结束后应统一更新，本文件不代改。
4. **调研 §7 待验证清单的状态列仍是 ⬜。** 本轮实测覆盖情况：#10（@ 第二个 source）、#16（拿不到 `inputTriggers` 时的表现）、#17（卸载后既有 chip 怎么办）、#20（`onPick` 异步读取与发送门控）**已由 G1 覆盖**；#3（B3 页内 fetch 拿结构化 JSON）**部分覆盖**（DeepSeek/ChatGPT 的目录接口存在性与结构已测，真实字段形态未验）；#5（R1 回程）**部分覆盖**（回程机制本身由 G2-A 的可控页验证；**零痕迹方案在 Windows 上已由 CDP 5/5 覆盖，macOS 无通路**）；#4（CSP）**未验**；#9（MiniMax 目录）**未验**；#22（按需建立读取上下文）= **G3 机制层已覆盖，真实链路未覆盖**。
5. **一致的项（已核对，无出入）**：调研 §2.5 的取数边界与裁定 1 一致；调研 §4.4「展开失败必须中止该轮模型调用，不能静默按没有引用继续」与 Q2 实测的拒绝行为一致（**在有 chip 时**）；开发计划 §11 A06「不出现就绪空标签」在甲形态下成立（chip 是 pending 句柄，不是空标签）——但同一条的「不自动降级发送」**只在有 chip 时成立**，无 chip 时内核仍会静默降级（缺口 G1-2）。
6. **G1 的证据有版本，本文件第 3 节的数字取自哪一轮必须写清。** 本轮实际发生**四次** G1 运行：
   - **12:21–13:06（`delivery.md` / `delivery2.md`）**：按源码手工复刻物化，产出 `final.out.txt`（13:01:41）、`reject.out.txt`（13:03:58）、`q2v2.out.txt`、`q4-*.out.txt`、`q5*.out.txt`；本文件第 3 节引用的 `final.out.txt` / `reject.out.txt` 即这一轮。
   - **13:09–13:15（仓库内新脚手架 `plugins/official-chat/test/drive/run-g1.mjs`）**：**未跑出 composer**（工作台首页没有会话 → 没有编辑器），该轮四条断言**未验证**。**这不推翻上一轮**（上一轮的证据仍在），但它说明**仓库里留着的这版脚手架不如旧脚手架**，须修好再当驱动用；该轮留下 **8 个 `%TEMP%\g1-probe-*` 临时目录（合计约 127MB）未清理**。
   - **13:26–13:31（`delivery3.md`）**：改走**壳的真实 Rust 物化链**（`ensure_wired` + 打包资源 + `verify_manifest`），**五问全部重跑**，数字与上一轮一致或更接近（等待 7439ms vs 7624ms；拒绝可见 1325ms→4994ms vs 1340ms→5031ms），产出 `final.json`（13:26:00）与 `reject-notice.json`（13:28:39）。
   - **13:49–13:56（`shell-dev.mjs` / `shell-verify.mjs`）**：起**壳本身**（`cargo run --features wdio` → `dsh-xlink.exe` dev 壳，`data_dir` 在 `%TEMP%\dsh-xlink-g1\shell-home\dsh\desktop-dev`，Vite 5174，WebView2 CDP 9335）做**壳真实 workbench webview 内复验** —— **但 `shell-verify.out.txt` 与 `model.log` 均为 0 字节（13:56:03），这次复验没有产出结果**，等于[第 9 节](#9-是否可以继续)先决条件 5 仍未闭合。
   - **证据生命周期问题**：`model.log`（桩模型收到的每个请求体，原为「请求体含夹具正文 / 正文只出现一次 / 角色 user」的原始依据）**已被 13:56 那次空跑覆盖为 0 字节**。目前该结论只剩 `final.out.txt` 的布尔字段 `requestHasBody:true` 支撑，**原始请求体已不可复核**。**教训：每轮跑批的证据必须写进独立文件，不得复用同名文件。**
7. **G3 场景 4/6 的「账号切换」与「官网窗口开/关」是探针自建的模拟**，不是真实账号切换、也不是壳的真实官网窗口。**这三条必须写进结论，否则下一个人会把它们当成真实链路证据。**（详见[第 5 节](#5-g3官网窗口未打开时仍可读取)的判定口径）
8. **本文件第 8 节 G1 第 2/3/4 条与[第 9 节](#9-是否可以继续)先决条件 3/4 已过时。** 13:19–13:26 期间：插件已登记进 `builtins.rs`（`OFFICIAL_CHAT` + `BuiltinSpec`，`entries:["host","client","package.json"]`、`peers: &[]`，计数测试 3→4）、`scripts/prepare-builtin-plugins.mjs` 已加 `official-chat` 条目、**根 `package.json` 的 `test:plugin` 已纳入 `plugins/official-chat/test/*.test.mjs`**（另加 `test:probe`）。`prepare-builtin-plugins --check` 现 exit=0（4 插件 / 38 文件）。**仍需注意**：G1 物化用的 `src-tauri/resources/builtin-plugins/official-chat` 是 gitignore 的构建产物，**新 clone 的 CI 必须先生成**（AGENTS.md 与 `scripts/dev-builtin.test.mjs` 已钉住这条）。

## 8. 本轮没有验证的

### G1（10 项）

1. 驱动的是**内核自带工作台页面**（Edge + CDP），**不是壳的 Tauri workbench webview**。仓库现有 e2e（`e2e/wdio.conf.mjs` + `@wdio/tauri-service`）驱动的是壳的管理面板，不是工作台 webview；本机无 chromedriver / msedgedriver / playwright / puppeteer。**「同一段链路在壳的真实窗口里也成立」未验证**——13:49 那轮起壳复验输出 0 字节（见[第 7 节](#7-文档与实测的出入)第 6 条）。
2. ~~物化与接线是按源码手工复刻~~ —— **已过时**：13:26–13:31 那轮（`delivery3.md`）改走**壳的真实 Rust 物化链**（`ensure_wired` + 打包资源 + `verify_manifest`），五问在真链产物上重跑。仍**未验证**的是「壳的真实 workbench webview 内」这条。
3. ~~插件没有登记进壳的内置插件表~~ —— **已完成**：`builtins.rs` 加 `OFFICIAL_CHAT` + `BuiltinSpec`，`prepare-builtin-plugins.mjs` 加条目，计数测试 3→4；`--check` exit=0。
4. ~~根 `package.json` 的 `test:plugin` 没加本插件的测试~~ —— **已完成**：`test:plugin` 已纳入本插件测试（206 tests / 203 pass / 0 fail / 3 skip），另加 `test:probe`（6/6）。
5. **P1 协议**（快照落盘、mention 展开、user 消息级上下文、快照版本固定）**一行没写**。
6. **真实官网、真实账号、付费模型完全没接**；正文是固定夹具。
7. 「点 chip 打开详情」的**详情面板**没做（G1 用内核 notify 报状态）；`openReference` 的四种状态文案只验了 `pending` 与 `ready`，`failed`/`unknown` 没单独跑。
8. 失效 chip 的删除只验了「光标紧贴 chip 后 Backspace」与「Ctrl+A 全选删除」两种手势；拖拽选中、剪切等未验。
9. 多会话/子代理场景、附件与引用同时存在、并发多次读取同一 ref 等未验。
10. 阶段 D 的时间线是 **500ms 采样的粗粒度**（`serialize-begin` 在首采 t=1ms 可见），不是内核内部的精确时序。

### G2（5 项）

11. **macOS 回程未验证**——本轮只验了 Windows；`WKWebView.callAsyncJavaScript` 的 awaitPromise 路径未跑，且**该路径当前在 wry 0.57 上不存在**（见[第 6 节](#6-裁定与理由)裁定 3）。
12. **ChatGPT 站点 CSP 未验证**（壳侧 403）。
13. **DeepSeek / ChatGPT 真实目录字段与分页形态未验证**——两站登录态已失效，裸 fetch 拿不到真实数据。
14. **正文读取未执行**——用户未指定测试对话，按规则阻塞。
15. **MiniMax 目录端点未验证**（本版不做，留 P7）。

### G3（4 项）

16. **G3 的真实链路未验证**。7 个场景的机制在「未登录 + 模拟页签」条件下全通（见[第 5 节](#5-g3官网窗口未打开时仍可读取)），但——
    1. **「复用已登录态并读到目录」未验证**（场景 3 自报 `not-logged-in`）；
    2. **真实账号切换未验证**（场景 4 的切换是探针自己改 localStorage 模拟的，验到的是「指纹变化 → 旧上下文/缓存失效」这条响应链）；
    3. **「用户开着官网窗口时壳复用它的页签」未验证**（场景 1/2/6 的窗口是探针自建的模拟页签 `official-chat-tab-0`）；
    4. **页签已挂载态下的真实 profile 复用未验证**（本轮用的是 `%TEMP%` 一次性拷贝）。
17. **零痕迹回程的 macOS 侧无通路**（P0 未闭合项，不与 Windows 并列）——wry 0.57 无 `callAsyncJavaScript` / `awaitPromise`，`InnerWebView` 为 `pub(crate)`，外部 crate 拿不到 raw `WKWebView`。**后果**：产品若要「两平台都零痕迹」，当前无解；要么 macOS 单独接受槽位残留并写明补偿，要么等 wry 上游。
18. **G3 的读取上下文在壳里的接线**（由谁创建、闲置回收的协调器、所有权转移的落点）**未在壳的真实链路上验证**——本轮探针自带了一套模拟实现。

### 本轮自身的判定口径说明（不是「未验证」，但必须记下来）

`final.out.txt` 里有两个字段与交付叙述不一致，经查证**都是检测口径造成的假象，不是结论矛盾**：

- `B-chip-lands.hiddenPlaceholder: true` —— 该字段的判据是 `/hidden|placeholder|pending/i.test(draftHtml)`（`g1-final.mjs:64`），chip 的 SVG 图标带 `aria-hidden`，**被粗正则命中**。权威判据是节点级的 `reject.out.txt` `draftNodes`：**没有第二个「待完成」节点**。
- `E-send-reject.errorVisible: false` —— 该字段在按回车后**固定等 5000ms** 才采样（`g1-final.mjs:127`），而拒绝提示在 1340ms 出现、**5031ms 就消失**，采样点踩在消失之后。权威判据是 `reject.out.txt` 的 250ms 密集采样。

结论本身成立，但**引用这两份文件的人必须知道它们的判据是粗的**。

## 9. 是否可以继续

**可以继续到 P1（协议与隔离）。**

理由：G1 的五问里四问通过、第五问（Q3）已判定为互斥并由协调者选型落地，机制层面不存在未知项；G3 的 7 个场景机制在受限条件下全通；P1 的前置条件「G1 通过」已满足，取数侧（P1 的协议与状态机建模）不依赖 G2/G3 的结果，可以并行推进。**G3 的机制结论不必等真实链路**：P1 建模的是协议与状态机，不依赖「已登录态能否读到目录」这个取数结论。

**不可以进入 P4（首站接入）。** P4 的前置条件是「G2/G3 通过」，而 G2 的真实取数结论因登录态失效未成立、G3 的真实链路（官网窗口 + 已登录态）未执行。P2/P3 可以在 P1 完成后按已冻结协议推进，但它们的**验收**（G4）依赖真实链路，不依赖真实站点。

### 继续的先决条件

**进入 P1 即可开工，但要满足：**

1. **就绪状态纪律先行落地**（裁定 4）：pending 句柄不得冒充就绪；label 保持中性；`openReference` 按 ref 如实报 pending/ready/failed；「就绪」由 chip 状态与详情承担，不以「chip 是否存在」承担。P1/P2 需把这条列为**硬性验收项**。
2. **G1-1…G1-5 登记为已知上游缺口**，实现中不得假设它们已解决；任何依赖「成功才插就绪 chip」的验收项，在 G1-1 落地前一律按降级形态写。

**进入 P2/P3 前需补：**

3. ~~**插件登记进内置插件表**~~ —— **已完成**（见[第 7 节](#7-文档与实测的出入)第 8 条）。
4. ~~**根 `package.json` 的 `test:plugin` 登记本插件测试**~~ —— **已完成**（`test:plugin` 已纳入本插件测试，另加 `test:probe`）。
5. **壳真实 workbench webview 内复验 G1 链路**——**仍未跑通**：13:49–13:56 那轮已能把壳起起来（`dsh-xlink.exe` dev 壳 + Vite 5174 + WebView2 CDP 9335，`data_dir` 在 `%TEMP%` 下），但 `shell-verify.out.txt` 与 `model.log` 均为 **0 字节**，这次复验**没有产出结果**。当前证据仍只来自内核自带工作台页面，不足以支撑「壳的真实窗口里也成立」。

**进入 P4 前需补：**

6. **用户在本机重新登录**（**ChatGPT 优先**：目录结构 `{items,total,limit,offset}` 已坐实、分页形态已知、凭据在 HttpOnly cookie 故页内 fetch 天然带上；DeepSeek 需额外 Bearer 路径且现有 `userToken` 已失效），否则真实目录字段与分页形态无法验证。
7. **G3 真实链路**：在**壳的真实官网窗口** + 已登录态下重跑 7 个场景（尤其场景 1/2/6 的真实页签复用与场景 4 的真实账号切换）。
8. **页内求值走零痕迹回程**（**Windows 已定**）：`ICoreWebView2::CallDevToolsProtocolMethod("Runtime.evaluate", {awaitPromise:true, returnByValue:true})`（**注意不是** `ICoreWebView2DevToolsProtocolController`——WebView2 没有那个接口）；`--mode cdp` 实测 5/5 且页面 JS 空间零痕迹。**R1 页面槽位不得直接带进真实站点**（裁定 3）。**macOS 当前无通路**（wry 0.57 无 `callAsyncJavaScript`，`InnerWebView` 为 `pub(crate)`）→ 按 P0 未闭合项处理。
9. **探测一律使用 `%TEMP%` 一次性 profile**，不再使用用户真实数据目录下的 webview profile（[第 7 节](#7-文档与实测的出入)第 2 条）。**已落机检判据**：`scripts/probe-profile-isolation.test.mjs` + `scripts/lib/probe-profile-isolation.mjs`（10/10）。

## 10. 证据与脱敏

**证据位置**（全部在临时目录，未进仓库）：

| 验证项 | 位置 |
| --- | --- |
| G1 交付原文 | `%TEMP%\dsh-xlink-g1\delivery.md`、`delivery2.md`、`delivery3.md`（真链终版） |
| G1 终检 A–F | `%TEMP%\dsh-xlink-g1\final.out.txt`（13:01 轮）、`final.json`（13:26 真链重跑） |
| G1 拒绝密集采样与草稿节点 | `%TEMP%\dsh-xlink-g1\reject.out.txt`（13:03 轮）、`reject-notice.json`（13:28 真链重跑） |
| G1 各场景 | `%TEMP%\dsh-xlink-g1\q2v2.out.txt`、`q4-codec-missing.out.txt`、`q4-service-missing.out.txt`、`q4-uninstalled.out.txt`、`q4-host-import-failure.out.txt`、`q4-svcprobe.out.txt`、`q4-notice.out.txt`、`q5.out.txt`、`q5b.out.txt` |
| G1 桩模型收到的请求体 | `%TEMP%\dsh-xlink-g1\model.log` —— ⚠️ **已被 13:56 的空跑覆盖为 0 字节，原始请求体不可复核**；该结论目前只剩 `final.out.txt` 的 `requestHasBody:true` 布尔字段支撑 |
| G1 壳真实窗口复验（未跑通） | `%TEMP%\dsh-xlink-g1\shell-dev.mjs`、`shell-verify.mjs`、`shell-dev.log`、`shell-verify.out.txt`（0 字节） |
| G2 A / B | `%TEMP%\dsh-probe\A-controlled.json`、`B-deepseek.json`、`B-chatgpt.json`、`B-minimax.json` |
| G3 | 探针跑批输出（临时目录时间戳 `1792475664714`），隔离证据：`source_read_only:true` / 拷贝 730 文件 59,632,751 字节 / `source_still_exists:true` / `temp_copy_removed:true` |

**脱敏说明**：本文件不含任何 token、cookie 值、账号标识、私密正文或真实对话标题。出现的站名与接口路径属公开信息；出现的对话标题是本轮夹具（`G1 固定对话 Alpha` / `G1 固定对话 Beta`）；凭据只记录了**键名**（`userToken`、`settingsJwt`、`__appKit_userInfo`）与 cookie **名称**，值一律未读取、未记录。正文读取全程未执行。

**提交状态**：本轮所有改动**留在工作树**，未 `git commit`、未 `git push`。
