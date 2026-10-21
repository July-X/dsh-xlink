---
description: "调研：工作台（A）能否 @ 引用官方网页版（B）里的会话内容。结论、证据、方案矩阵与待验证清单。"
kind: "research"
status: "调研阶段，未实现"
date: 2026-10-20
---

# 官方网页版（B）↔ 工作台（A）打通：可行性调研

实施顺序、模块边界与验收要求见[开发计划](official-chat-development-plan.md)。

## 结论

目标体验是：**在工作台输入 `@` → 选择官网对话 → 继续输入并发送。** 系统按需读取选中对话，在内部生成固定快照。用户不用先切到官网窗口、复制内容或执行导入，也不用维护一份“已导入列表”。这是 2026-10-20 用户确定的交互要求。

代码表明内核的 `@` 菜单支持多个来源，壳随包插件有客户端与宿主侧两半，可以尝试增加“官方对话”来源。但这条完整链路仍未实测，不能把“接口路由可达”写成“引用已跑通”。

- **A 侧候选**：菜单查询官网对话的标题、时间与来源，不以本地已有快照为前提。列表按需分页，正文仅在选中后读取。
- **B 侧读取**：优先验证页内 `fetch` 与宿主异步回程。网站认证、账户范围、分页和读取完整性需要逐站验证；目前不能承诺三个站点都可用。
- **引用落地**：读取成功后自动固定快照并插入内核原生引用标签（chip）。发送时由插件在 `agent/pre-step` 展开为不受信任的 user 消息上下文。正文不进入系统提示。
- **缓存职责**：目录缓存用于加快候选查询，正文快照用于保证发送与重试读取的是同一版本。缓存不成为用户的前置任务，也不在后台收集全部历史。

| 阶段 | 内容 | 验收要求 |
| --- | --- | --- |
| v0：内部可行性验证 | 固定测试目录与正文 → `@` 选择 → 异步读取 → 引用标签 → 模型输入核对 | 验证注册来源、异步选择、读取期间发送、取消、缺失快照；测试数据只放隔离目录 |
| v1：首个站点闭环 | `@` 中直接列出真实官网对话，选中后按需读取并引用 | 先接入完成实测的一个站点；常规引用没有复制、导入或逐次确认步骤 |
| v2：扩展与兼容 | 扩展其他站点、分页搜索与内核版本兼容 | 每站独立通过目录、认证、正文与失败验收，不能沿用其他站点的成功结论 |

主路线固定为插件自有 `@` source + host 侧展开。P3“伪造内核会话文件”保留为技术备选，不作为默认实现。剪贴板只保留为用户自愿使用的故障兜底；剪贴板演示不算完成目标体验。

网站首次登录仍由用户完成。已有登录态的日常引用应在工作台内完成，官网窗口无需保持前台。页签未挂载时能否按需建立不抢焦点的读取上下文，属于发布前验证项。

按需只读仍涉及站点规则与账号风险，不能据此宣称程序化读取被允许。来源可用性与风险处理见 §2.5，完整交互见 §4。

---

## 0. 先厘清 A 与 B 是什么

这一步容易搞混，事实如下：

| | 身份 | 实现 |
| --- | --- | --- |
| **A** | **工作台**：用户在这里跟各家大模型对话 | 一个 webview，加载 **dsh 内核自带的 webui**（`http://127.0.0.1:3090` release / `3091` dev），由内核在本地提供页面 |
| **B** | **官方网页版**：另一个独立窗口，内嵌第三方站点的**原生网页** | 一个裸 `tauri::Window`（label `official-chat`）+ 顶部页签栏 webview（本地 SPA）+ 每个页签一个内容 webview |

B 的三个页签（`src-tauri/src/harness/official_chat.rs:63`）：

| 页签 | 地址 |
| --- | --- |
| DeepSeek | `https://chat.deepseek.com` |
| MiniMax | `https://agent.minimaxi.com` |
| ChatGPT | `https://chatgpt.com`（条件页签，取决于本机 Codex 登录是否可用） |

> **注意 B 不是「dsh 的网页版」**。dsh 的 webui 就是 A。B 是各模型厂商**自己的**官方网页版，壳只是把它们当网页装进了自己的窗口里。

---

## 1. A 侧：已经具备的能力

### 1.1 壳 ↔ 内核的 RPC 通道（已跑通）

`src-tauri/src/notify/task.rs` 已经是一套完整的内核 RPC 客户端：

```
GET  http://127.0.0.1:<port>/?token=…        → 303 + dsh-auth-* cookie
POST http://127.0.0.1:<port>/api/session/list  → {"type":"client-request","rpcId":…,"method":"session/list","payload":{"args":{…}}}
WS   ws://127.0.0.1:<port>/api/remote.mux     → Cordis 事件流
```

证据：`notify/task.rs:910`（换 cookie）、`notify/task.rs:941`（`session/list`）、`notify/task.rs:56-60`（mux 路由与事件端点）。

内核侧的 Remote 命名空间由 `dsh-api-session-controller` 提供（`session` / `skills` / `fileReferences`）。`session/prompt` 是其中之一。

**本机实测（2026-10-19，对运行中的内核 0.2.1-rc.2，端口 3090）**：

| 探测 | 结果 |
| --- | --- |
| `POST /api/session/doesNotExist` | **HTTP 404 `not found`** —— 不存在的方法的基线 |
| `POST /api/session/prompt` | **HTTP 200** + JSON-RPC 错误信封 —— **方法已被路由**，被网关的参数校验拦下 |
| 参数名试错 | 网关直接告知：需要 `request` 字段（不是 `_request`） |
| 用精确 schema + 假 sessionId | `{"code":"session/not-found","message":"session \"00000000-…\" not found"}` —— **打到了内核自己的会话解析层** |

**精确 wire 契约**（来自 `dsh-api-session-controller/lib/typert.host.js:1307-1325` 的 `session_prompt_parameter_0_strict` schema，strictObject）：

```jsonc
{
  "type": "client-request",
  "rpcId": "<客户端自造>",
  "method": "session/prompt",
  "payload": { "args": { "request": {
    "requestId": "probe-req-2",                       // string，客户端自造；写在被接受的那条 user 消息上（source.rpcId）
    "sessionId":  "<uuid>",
    "mode":       "queue",                            // 只接受 "queue" | "steer"
    "content": [                                      // 至少一段非空白文本或一个附件
      { "type": "text",  "text": "…" },
      { "type": "image", "mediaType": "image/png|image/jpeg|image/webp|image/gif", "data": "<base64>", "name": "…" },
      { "type": "file",  "receiptId": "<file-upload 回执>" }
    ],
    "clientTimeZone": "Asia/Shanghai"                 // 可选，须为 UTC 或合法 IANA 时区
  } } }
}
```

**这张表就是「A 侧注入」的完整契约。** 三个关键含义：

1. **纯文本注入**：`content: [{type:"text", text:"…"}]` 即可，**零内核改动、零 DOM 操作**。
2. **截图注入**：`image` 段直接吃 base64 `data`——**B 侧窗口的截图可以原样进模型上下文**，不必落盘。
3. **文件注入**：`file` 段要 `receiptId`（先走内核的文件上传通道换回执），比文本麻烦一档。

**副作用核验**：探测前后 `session/list` 均为 69 条，会话目录文件数不变——**本轮探测没有在用户数据里留下任何东西**（全程只指向一个不存在的 sessionId，失败发生在 `resolveAgent` 之前）。

> 注意 `mode` 只能是 `queue` / `steer`：这不是文档里写的，是实测撞出来的——第一次用 `"send"` 时网关报 `gateway/input-invalid: wire field "request" failed boundary validation`，从 schema 里才看到真值。

#### 1.1.1 会话控制器暴露的全部端点

内核 `dsh-api-session-controller` 的 Remote 命名空间（devloper 逐个核到 `lib/typert.remote-client.js` 的行号）：

| 端点 | 用途 | 对本设想是否有用 |
| --- | --- | --- |
| **`session/prompt`** | 投递一条用户消息 | ✅ **主推** |
| `session/list` | 会话列表（`{cursor}` 可选） | ✅ 壳**今天就在调** |
| `session/follow` | 流式跟随消息流 | ✅ 读回引用结果 |
| `session/page` | 冷读分页 | ✅ |
| `session/search` | 会话检索 | ⭕ 长期价值（§5 长期） |
| `session/attachment` | 附件元信息 + base64 | ⭕ |
| `session/create` / `session/fork` / `session/rename` | 生命周期 | ⭕ P3 可能用到 |
| `session/cancel` / `session/updateQueue` | 队列控制 | ❌ |
| `session/selectModel` / `session/modelCatalog` / `session/initializeDefaultModel` | 模型 | ❌ |
| `session/projections` / `session/control` | 投影 / 控制流 | ⭕ |
| `session/openWorkspacePath` / `session/canOpenWorkspacePath` / `session/workspacePathApplications` | 工作区路径 | ❌ |
| `fileReferences/list` | 枚举目标会话工作区里的候选文件 | ✅ P2 |
| `sessionReferenceResolver/candidates` | 枚举可 @ 的历史会话 | ✅ P3 |
| `skills/list` | 技能发现 | ❌ |

**文件回执的取得方式**（补齐 `{type:"file", receiptId}` 那一环）：

```
POST /api/session/uploadFileBinary?sessionId=<id>&name=<n>
content-type: application/octet-stream
body: 文件字节流
→ { ok, value: { receiptId, file } }
```

`receiptId = randomUUID()`，且**与确切会话绑定**（`dsh-client-file-upload/lib/types/http-route.js:10-56`）。

**为什么走 RPC 而不是走 DOM**——内核侧扫过一遍：Lexical 编辑器实例确实挂在元素上（`t.__lexicalEditor = e`，`dsh-client-ui-conversation/lib/client.js:10546`），但**没有暴露任何全局 composer 读写 API**：bundle 内部有 `setDraft` / `insertText` / `replaceText`，全是类私有方法；已装内核**所有包**的 26 个 `__DSH_*` 全局里，**没有任何 composer / draft / editor 相关的**。所以 DOM 路径拿不到内核的内部方法，**RPC 才是正规通道**。

> 另注：`dsh-sdk-jsonrpc-server` 是 **stdio** JSON-RPC 服务，但它跑在**另一个 runtime**（`dsh --profile sdk`）里，不是壳管理的这颗内核——`dsh-acp` / `dsh-acp-app` 同理。**别把这两条当通道。**

### 1.2 工作台输入框的程序化写入（已跑通）

`src-tauri/src/harness/harness-draft.js` 已经在做这件事，做法是**走浏览器真实输入路径**而不是改 DOM：

- 文本：`document.execCommand('insertText', false, text)`（`harness-draft.js:318-339`）。选它是因为内核 0.2.0-rc.2 的 `dsh-client-ui-conversation/lib/client.js` 里 Lexical 在编辑器根上注册了 `beforeinput` 处理器并按 `inputType` 分支——`execCommand` 走的正是真实打字走的那条。
- 图片：往编辑器派发带 `clipboardData` 的 `paste` 事件（`harness-draft.js:472-517`）。这是**用户自己粘贴截图时内核走的同一条路**（Lexical `PASTE_COMMAND` → `intakeFiles` → `createDrafts`）。文件选择框那条路实测走不通——React 的 value tracker 会把合成 change 判成「值没变」而丢弃整个事件。
- 每次写入都**读回核对**：写入成功不等于编辑器收下了。

**这意味着「往工作台输入框里塞一段文本/一个文件」这件事，壳已经做过并且踩过坑。**

但要认清它的**性质**：这条路只把东西**放进输入框**，还得用户再点一次发送；它**不产生可寻址的消息**（没有 `messageId`、没有 `source.rpcId`、在 `session/follow` 里回读不到）。**所以它适合做「让用户看一眼再决定发不发」，不适合做 A→B 的引用链主路径。** 主路径应当是 §1.1 的 `session/prompt`。

### 1.3 内核原生就有 @ 引用机制（这是本次调研最重要的发现）

内核里 `@` 补全菜单同时提供**文件与文件夹**、**对话**、**子智能体**三类候选（`dsh-client-ui-reference/lib/client.js:293-368`，`trigger: "@"`）：

| 机制 | 语法 | 语义 |
| --- | --- | --- |
| `@file` | `@path` / `@"path with spaces"` | **选中候选绝不读取或附带文件内容**；模型必须自己调用文件系统工具去看 |
| `@session` | `@[label](dsh-session:<base64url 编码的 id>)` | 发送时经 `session.prompt` 携带该 mention，`dsh-session-reference` 在 `agent/pre-step` 校验并**捕获模型上下文** |

**技能不在 `@` 下**——技能源是 `trigger: "/"`（`dsh-client-ui-skill/lib/client.js:419-422`），斜杠命令同理。给 composer 注册的输入触发源总共三个：reference（`@`）/ command（`/`）/ skill（`/`）。**这一点对交互设计有直接影响**（§4.4）。

> **补一条（2026-10-19 复核）**：上面三类候选是**内核自带那个 source** 提供的。`@` 菜单本身是**多源**的——注册表唯一性是 `(trigger, name)`，roster 按 trigger 合并，每个 source 各自一组。**插件可以注册自己的 `@` source**，这条是 A 侧 `@` 方案成立的技术前提，证据与形态见 §4.1.1。

`@file` 的内核原话（`dsh-file-reference/lib/index.js:54`）值得抄下来，它说明了 `@` 的真实契约：

> "Tokens prefixed with @ are paths the user explicitly referenced. Relative paths resolve from the workspace root; absolute paths identify files or directories on the host. … **use the read tool when its contents are needed, and do not claim to have inspected it before reading.**"

`@file` 的约束（`dsh-file-reference-local/lib/index.js:15,17,204-208,323-325`）：候选绑定 **Agent 的 cwd**；`maxResults` 默认 20、`maxEntries` 默认 50000；**遍历不得越出 workspace root**（拒绝 `..`）。所以「把 B 侧内容写成临时 md 放进该会话的 workspace cwd，prompt 里带 `@相对路径`」这条通路**完全不需要改内核**，但受这些限制约束。

`@session` 的机制细节（`dsh-session-reference/README.zh.md`）：

- 捕获到的是**有界、只读的快照**，作为**第二条 user 角色消息**紧跟在用户消息之后。
- 快照是**不受信任背景**：带固定警告，禁止模型遵循其中的指令、权限声明或工具请求。
- 预算：每个来源默认 `max(65536, floor(contextWindow × 4 × referenceContextFraction))` 字节，`referenceContextFraction` 默认 `0.2`。
- `maxReferences` 默认 `3`，**不得超过 3**。
- 截断时可选的 spill 后端会把完整 transcript 存到目标会话下，并给出精确的 `omittedMessages` / `omittedBytes` 与 `retrievalHint`。
- 捕获只投影**文本**（用户直接发送的文本、assistant 文本、带压缩标记的 user 检查点）；工具、推理等注入上下文被排除——这既防引用递归传播，也防源会话后续变更影响已存 transcript。
- 引用是**快照**，不是 fork / 恢复 / 订阅。

> **这个发现的意义**：内核已经把「跨会话引用」这件事的语义、防护与预算全想清楚了。**我们不需要自己发明一套「引用」的机制**，只需要解决「B 的第三方会话怎么变成内核认识的会话」。

### 1.4 壳已经能读内核的会话日志

`src-tauri/src/usage/local.rs` 用 `ruzstd`（纯 Rust zstd 解码器，`Cargo.toml:94`）增量扫描内核落下的 zstd 会话日志。

实测本机会话存储布局：

```
<DSH_HOME>/sessions/<cwd-slug>/session-<id>/session.v4.jsonl.zstd
<DSH_HOME>/storages/session_projcache/sessions/<id>.json
```

解压后是 JSONL，v4 格式。头部一行：

```json
{"type":"session","version":4,"id":"session-…","createdAt":…,"cwd":"…","isSeeded":false,"delegationDepth":0,"agentPreset":"standard"}
```

其后是事件行。实测观察到的 user 消息落盘形态（`agent/inbox/spliced`）：

```json
{"type":"agent/inbox/spliced","seq":7,"time":…,"data":{"target":"next-step","start":0,
 "inserted":[{"content":[{"type":"text","text":"…"}],"source":{"kind":"user-approval"},"role":"user","id":"…"}]}}
```

> **待实测**：assistant 侧消息的事件类型在本机样本会话里没观察到。已知同族的还有 `user/message`、`turn/start`、`turn/end`（出现在 `dsh-api-session-controller/lib/types/commands.js:84-86` 的边界计算里）。**这直接决定「伪造成 dsh 会话」这条路的工作量。**

补充事实：

- 全文检索后端 `dsh-session-query-sqlite` 是**可选能力**（默认组合不含它），所以会话列表是从 JSONL 日志直接读的，不依赖 SQLite 索引。
- 磁盘布局的 `compression` 默认 `'zstd'`（带校验和帧），**也可设为 `'none'` 变成换行分隔的纯 UTF-8 文本**（`dsh-session-persistence-jsonl/README.zh.md:48`）——这一点对「造一个假会话」很有利：纯文本格式比 zstd 好写得多。
- `storages/session_projcache/sessions/<id>.json` 装的是**折叠后的投影**（title / todos / tokenUsage / turnBoundary / inbox），**不是消息流**。`inbox.next-step` 里确实能看到 `role:"user"` 的待投递消息对象，但那是**未投递的入队内容**，不是历史。
- **读消息流应当走内核支持的读路径**（`session/follow` / `session/page`），而不是自己解 zstd + 复现事件投影。壳侧的 `usage/local.rs` 是为统计用量才解 zstd 的，那是另一回事。
- `session/prompt` 投递的消息在内核里的 `source` 是 `{kind:"user", rpcId: request.requestId, principal?}` —— **就是一条普通用户消息**，`requestId` 是幂等键（同 id 重复提交直接返回 `{accepted:true}`，不重复投递）。

### 1.5 官方对话的内容 webview 可被寻址

`src-tauri/src/commands.rs:1651-1683`（`add_official_chat_tab`）：

- 内容 webview 的 label 是 `official-chat-tab-{index}`，父窗口是 `official-chat`。
- 壳侧用 `app.get_webview("official-chat-tab-{index}")` 判断页签是否已挂载（`commands.rs:1697`），**说明 Rust 侧能按 label 拿到这些 webview**。
- 建窗时挂三条 `initialization_script`：`titlebar-pulse.js`、`chat-fingerprint.js`、`no-context-menu.js`。**加一条新脚本是标准做法。**
- 所有内容 webview 共享同一个 user-data 目录 `<data_dir>/webview-official-chat`（Windows 上是 `EBWebView` 夹），cookies / localStorage / IndexedDB 跨重启保留。

---

## 2. B 侧：取数是真正的难点

### 2.1 B 的形态带来两个约束

**约束一：页面里没有可用的 Tauri 桥。** `chat-fingerprint.js` 主动删掉了 `window.__TAURI__` / `__TAURI_INTERNALS__` / `__TAURI_METADATA__` / `__TAURI_IPC__`（`chat-fingerprint.js:63-83`），理由写得很清楚：普通浏览器里这些位置什么都没有，页面探针必须看到什么都没有，「哪怕暴露一个 Proxy 让 typeof 检查通过，依然是在宣告自己是嵌入式 webview」。

这条纪律是**故意为之的**，不是疏忽。**任何在 B 页面里留下全局量或可被探测的注入物的做法，都与既有安全目标直接冲突。**

**约束二：登录态在 WebView2 的 user-data 目录里。** `~/.dsh-xlink/dsh/desktop/webview-official-chat/EBWebView`。cookies 由 WebView2 加密存放（Windows 上是 DPAPI），站点数据按 origin 隔离。

### 2.2 五条取数途径

| 途径 | 拿会话列表 | 拿完整消息 | 可靠性 | 成本 | 账号风险 |
| --- | --- | --- | --- | --- | --- |
| **B1 注入 JS 抓 DOM** | 能，但重型 SPA + 虚拟滚动，DOM 里通常只有渲染出来的那几十条 | 能，user/assistant 靠启发式分流；思维链 / tool call / 被打断的回答是独立折叠节点，容易漏 | **低—中**：选择器随版本变化，改版即静默失效 | 中：每站一套选择器 + 滚动策略 | 低（不产生额外请求） |
| **B2 Rust 侧带 cookie 调站点内部 API** | 能 | 能 | 中：JSON 比 DOM 稳 | 高：凭据提取 / 解密 / 存储 / 轮换 / 401 处理 | 中—高（指纹不一致 + 频率异常） |
| **B3 页内 fetch 代理**（优先验证） | 待逐站验证 | 待逐站验证 | 结构化 JSON 可降低解析耦合；认证与回程尚未跑通 | 每站一个适配器 + 异步回传通道 | 仍需评估，手动触发不等于站点允许 |
| **B4 剪贴板** | 不能 | 不能（只有用户手动选中的片段，无角色结构） | 对「手动复制」可靠，对「取会话」不可用 | 低 | 极低（但剪贴板可能含无关内容） |
| **B5 站点自带导出 / 分享** | 不能（单条会话） | 能，但形态受限 | 中：人工步骤多、链接可撤销 | 低—中 | 极低（走站点自备功能） |

**B2 已被实测否掉。** tester 在本机对 WebView2 profile 的 `Default\Network\Cookies` 试了四种读法（CopyFile2 / FileShare.ReadWrite\|Delete / `esentutl /y` / SQLite `immutable=1`），**全部失败于 WinError 32（运行时被独占）**。再加上：凭据不全在 cookie 里、外发请求丢浏览器指纹、凭据要落盘扩散——**这条路在本机不成立**。

B3 是优先验证路径：在站点页面内查询目录和选中对话，复用浏览器环境。它不会自动继承站点应用的全部认证逻辑；下表所列 token、账户请求头和轮换行为仍要实测。壳不持久化或外传站点凭据，认证失败在候选组或选中行内显示。

**B2 的真正遗产是「接口知识」，不是「cookie 方案」**。已知端点（**来自第三方逆向项目，属二级来源，未在本机复核**）：

> **（2026-10-21 12:4x 一手复核更新）** —— **DeepSeek 那两行已不是二级来源**：端点路径、首屏/翻页参数名（`count` / **`lte_cursor`** / `pinned`）、响应三层骨架（`{code,msg,data:{biz_code,biz_msg,biz_data}}`）都**已由一手实测复核**（P4 探针 A ＋ 站点自己缓存的请求行）⇒ **本表里 ChatGPT 与 MiniMax 两行仍是二级来源、未在本机复核**。
> **同时更正本表一处字面矛盾**：原文在表头自述「二级来源、未在本机复核」，而 DeepSeek 行却写「**实测**一次最多 100 条且**无游标**」⇒ **二手被写成了实测**；且「无游标」已被一手证据反证（站点自己的翻页请求带 **`lte_cursor`**）⇒ **已按上面那行改写**。

| 站点 | 会话列表 | 历史消息 | 备注 |
| --- | --- | --- | --- |
| ChatGPT | `GET /backend-api/conversations?offset=&limit=100&order=updated&is_archived=` | `GET /backend-api/conversation/{id}` | 需 ChatGPT 模式的 access token + `ChatGPT-Account-ID` 头；正文在 mapping 树里，`current_node` 决定活跃分支 |
| DeepSeek | `GET /api/v0/chat_session/fetch_page?count=N` | `GET /api/v0/chat/history_messages?chat_session_id=<uuid>` | 页大小上限 `count`（**二手来源**称一次最多 100 条，**未在本机复核**）；翻页用游标参数 **`lte_cursor`** ＋ `pinned`（**一手实测**：站点自己发出的翻页请求行 `fetch_page?lte_cursor&pinned=false`）；正文在 `fragments` 里（REQUEST=用户 / RESPONSE=助手 / THINK=思维链） |
| MiniMax agent | **未验证** | **未验证** | 只查到发消息端点 `/matrix/api/v1/chat/send_msg`；请求带 `x-signature` / `yy` 签名 |

**难度排序：ChatGPT → DeepSeek → MiniMax agent（需先探一次真实请求）。**

> **一个必须自己复核的细节**：壳里写的是 `https://agent.minimaxi.com`，但它 **301 跳到 `https://agent.minimax.cn`**（实测两个域名都返回 200、字节数相同）。所以 **MiniMax 页签的真实 origin 是 `agent.minimax.cn`**——页内 fetch 的目标路径、origin 作用域的存储（实测 localStorage 里确有 `agent.minimax.cn` 与 `account.minimax.cn`）都要按 `agent.minimax.cn` 算。**壳里的常量本身不是 bug**（重定向接住了），但做 B3 时不能照着常量里的域名写。

### 2.3 数据怎么从 B 回到壳

B1 / B3 都要解决「从远程页面把数据取回宿主」这个回程问题。候选：

| 回程 | 机制 | 代价 |
| --- | --- | --- |
| **R1 宿主侧 eval-with-result** | Tauri 2 的 `with_webview` 拿到平台 webview：Windows 上是 `ICoreWebView2::ExecuteScriptWithResult`，macOS 上是 `WKWebView.evaluateJavaScript` | 平台相关代码。`webview2-com` 0.39.1 已在 `Cargo.lock` 里（wry 的传递依赖），但 Rust 2018+ 要直接用需在 `Cargo.toml` 里显式加一条 Windows-only 依赖 |
| **R2 本地中继 HTTP** | 注入脚本 `fetch('http://127.0.0.1:<port>', {mode:'no-cors'})`，壳跑一个小回环服务接 | **可能被页面 CSP 的 `connect-src` 拦掉**（待实测）；且引入一个监听端口 |
| **R3 剪贴板 / 用户手动粘贴** | 不需要回程 | 粒度粗，要用户动手 |

R1 优先验证。原生回调异步不代表会等待 JavaScript Promise 完成，必须验证 `fetch` 成功、拒绝、超时和导航取消都能返回结构化结果。macOS 需评估 `callAsyncJavaScript`；Windows 需验证具备等待 Promise 语义的回程。不得为回程恢复页面全局 Tauri 桥，也不得把返回 Promise 对象当成读到了正文。

> tester 指出一个同源问题：B3 的页内 `fetch` 打的是站点自己的接口，**同源、带 cookie、带指纹**，这是它相对 B2 的全部优势所在。**回程（R1/R2）是「把 B3 的结果拿回壳」，与 B3 本身是两件事**——不要把两者混成一条路。

### 2.4 B 侧风险清单

1. **账号风控**：风险从高到低是「Rust 侧高频外发 + 缺真实指纹」（最高）>「重放站点签名算法做写操作」>「低频、只读、用户手动触发」（最低）。
   - **只读不写，就不要碰签名与反自动化那一套**（ChatGPT 的 sentinel PoW / Turnstile / conduit token、MiniMax 的 `x-signature`）。
   - **反常识的一点**：壳现在抹平 webdriver 痕迹是「让行为像真人」；但风控主要看**请求频率 / 指纹 / 行为模式**，webdriver 标记只是其中一项。走 B3 时请求就是那个真实 webview 发的，**天然不需要伪装**——**伪装不是风控的主要矛盾，取数方式才是**。
2. **ToS 与合规红线**（见 §2.5）。
3. **静默失效**：DOM 选择器变了、接口加了签名、凭据位置漂移——最坏表现是「抽取失败但界面上看起来成功」。**判据：宁可明确报错，也不要塞一段空内容或半截内容。**
4. **CSP 阻断**（影响 R2，以及页内 eval 能否落地）。
5. **认证与账户切换**：B3 仍要处理失效登录与账户变化。每次读取校验当前账户范围；换账号后取消旧请求、作废旧目录缓存，不能把上一账号的候选套到新账号上。壳不接管凭据轮换。

### 2.5 取数边界与站点风险

用户在 `@` 中浏览来源或选择对话，是本次按需读取的触发点。目录请求只取必要元数据，选中后才读取该对话正文；关闭菜单停止目录请求，已经选中后的正文读取继续；只有取消选择才停止该次正文请求。不定时同步，不提前下载所有会话正文，不因键盘输入而反复全量请求。

“用户触发、逐会话、只读”是限制请求范围的工程约束，不能等同于站点允许程序化提取。OpenAI 使用条款对程序化提取和绕过保护有独立限制，也没有给出“只读即可绕过保护”的例外。各站点适用条款和实际接口都需要单独评估，不能承诺零账号风险。[OpenAI 使用条款](https://openai.com/policies/terms-of-use/)

不伪造站点签名、验证挑战或反自动化机制，不导出凭据，不对外分发会话库。站点禁止或无法支持的读取应明确显示不可用；可提供用户自愿复制粘贴的兜底，但不能把额外导入步骤放回正常流程。

选中的内容会随工作台消息发往当前模型服务商。这是用户使用引用的目的，界面需要准确标明来源与实际接收方；不得悄悄上传其他未选中对话。发送与缓存边界见 §4.5。

---

## 3. 方案矩阵：A 侧形态 × B 侧取数

### P0 可选兜底：用户自己复制粘贴

用户可以自行从 B 复制并粘贴到 A，沿用现有输入能力。纯文本没有可靠的角色边界，不自动补造角色或声称复制完整。该路径只作自愿兜底，不计入直接 `@` 引用的验收。

### P1 纯文本注入（技术验证与可选兜底）

```
B 的会话内容 → 抽取成带 role 标记的纯文本 → 壳调 session/prompt（text part）→ 模型直接看到
```

- **A 侧**：`session/prompt` 的 `content: [{type:"text", text:"…"}]`。零内核改动、零 DOM 操作、零格式耦合。
- **投递出来的是一条真正的用户消息**（`source = {kind:"user", rpcId: requestId}`），可以在 `session/follow` 里回读、可与 `@` 引用区分。
- **优点**：最快跑通；壳完全掌控内容形态（可加来源抬头、角色标记、截断）。
- **缺点**：**没有「引用」的语义**——不是可点击的原子 mention，模型无法区分「这是用户的话」和「这是从别处引来的内容」，**提示注入防护要壳自己写**（内核的 `@session` 快照有固定警告，P1 没有）。

### P1′ 图片与附件注入（P1 的变体，同一条通道）

- **截图**：`content: [{type:"image", mediaType, data:<base64>}]` —— **B 侧窗口的截图可以原样进模型上下文**，不必落盘、不必先上传。媒体类型限 png/jpeg/webp/gif 四种。
- **文件**：`POST /api/session/uploadFileBinary` 换 `receiptId`，再 `content: [{type:"file", receiptId}]`。回执**与确切会话绑定**。
- 这两条与 P1 共用同一次 `session/prompt` 调用，可以混在一条消息里。

### P1\* 原生 mention + host 侧展开（A 侧 `@` 的主路线）

```
A 输入 @ → 按需查询官网对话目录 → 用户选中 → 读取该对话正文
         → 自动固定快照 → 内核 reference chip（@[label](official-chat:<snapshotId>)）
         → 发送时 host 半边在 agent/pre-step 展开有界、不受信任的快照
```

- **A 侧**：内核 `@` 菜单多一个 source（§4.1.1 ①②），chip 由内核渲染，展开由壳的 host 半边在 `agent/pre-step` 做（照 `dsh-session-reference` 的语义）。**不改内核一行。**
- **优点**：引用体验与 `@session` 同构（用户挑一条、chip 常驻、来源清楚）；不受信任警告 / 预算 / 截断 / 溢出通知由**壳照抄内核那套**产出；不污染用户的会话列表。
- **缺点**：要写**第一个「挂在输入触发源上」的壳插件**（client 半边注册 source + host 半边展开）；`appearance` 与 chip 的渲染细节**待实测**（§7 #10）；自己那套 scheme 要在 host 侧被正确识别与展开（§7 #11）。
- **兼容性**：`registerSource`、候选字段、`codec` 与 `appearance`/`icon` 都需要按目标内核版本验证，并提供来源失效诊断（§4.7）。P3 则依赖事件格式与投影发现，迁移包存在不等于外部写入兼容；不能据此认定哪条路线更稳定。

### P2 落文件 + `@file`

```
B 的会话内容 → 写成 md/txt 到工作区 → 用户 @ 该文件 → 模型自己 read
```

- **优点**：复用内核现成的 `@file` 语义（内核会给模型装 `FILE_REFERENCE_PROMPT` 提示先读再答）。
- **缺点**：**多一次工具往返，模型可能不读**（内核原话是「use the read tool when its contents are needed, and do not claim to have inspected it before reading」）；文件留在工作区要清理；候选绑定 Agent 的 cwd，且**遍历不得越出 workspace root**。

### P3 伪造成 dsh 会话，走内核原生 `@session`（技术备选）

```
B 的会话内容 → 生成一个 dsh v4 会话文件 → 内核 @ 菜单里出现它 → 用户 @ 它
             → 内核用**自己的**引用机制捕获上下文（不可信警告 / 预算 / 截断 / spill 全套）
```

- **优点**：语义最漂亮——`@` 菜单里天然就是「引用一个会话」；**提示注入防护、预算、截断、溢出落盘全部由内核已经想清楚的机制承担**；用户侧的心智模型与内核已有的 `@session` 完全一致；**壳不用写任何展开逻辑**。
- **缺点**：
  1. 要复刻 v4 事件格式，与内核版本耦合（内核自带 `dsh-session-format-v3-to-v4` 迁移包，说明格式在演进）。**缓解手段**：磁盘 `compression` 可设为 `'none'`，写纯 UTF-8 JSONL 就行。
  2. 会污染用户的会话列表。
  3. 冷会话的标签来自**持久检查点**，投影缓存是 `storages/session_projcache/` 下的 JSON——外部写入的会话是否会被内核纳入索引与投影，**必须实测**。
  4. assistant 侧消息的事件类型尚未观察到（§1.4）。
  5. `maxReferences` 不得超过 3。

### 对比

| 路线 | 用户操作 | 主要代价 | 定位 |
| --- | --- | --- | --- |
| P0 剪贴板 | 自行复制粘贴 | 内容结构与完整性不可保证 | 自愿兜底 |
| P1/P1′ 直接发消息 | 由壳投递文本或附件 | 不经过正常的引用草稿流程 | 技术验证与可选兜底 |
| P1\* 自有 `@` 来源 | 在工作台选择官网对话 | 站点适配、客户端扩展与 host 展开都要验证 | 主路线 |
| P2 工作区文件 | 写文件后再引用 | 增加操作与工作区文件 | 可选归档 |
| P3 生成内核会话 | `@session` 引用 | 私有事件格式、投影发现与会话列表污染 | 保留技术研究，不作默认路线 |

站点规则与账号风险取决于取数方式，不能只按 A 侧引用形态给出“低风险”结论。P1\* 依赖前端扩展接口，P3 依赖持久化事件格式；两者都有版本耦合，目前没有证据作绝对排序。

---

## 4. 交互形态：在工作台直接选择官网对话

### 4.1 主流程

```text
在工作台输入 @
  → “官方对话”组按来源显示最近对话，可搜索、分页
  → 选择一条对话，当前行显示读取进度，可取消
  → 读取成功后自动固定快照，插入带来源与标题的引用标签
  → 用户继续输入，按正常方式发送
```

候选显示来源、标题和更新时间。用户选择的是官网对话，候选不以“是否已导入”为筛选条件。B 侧页签栏不增加导入按钮，A 侧也不增加引用前的导入面板。

首次使用尚未登录的站点，在该来源内显示“去登录”；登录成功后回到原草稿并重试原操作。已经登录时不切换前台窗口。没有挂载的站点按需初始化读取上下文，必须验证可以复用正确 profile、处理重定向且不抢焦点；不在启动时预建全部站点页面。

候选与正文分别请求。优先使用当前来源的有效目录缓存，再按需更新标题与时间；只有用户浏览该来源或翻页时继续查询。各来源独立展示加载与错误，不能让一个慢站点阻塞文件、内核会话或其他来源候选。搜索不等于下载全部历史正文；站点不支持完整搜索或分页时，明确显示当前覆盖范围。

---

### 4.1.1 内核侧机制（代码证据，2026-10-19）

**① `@` 菜单是多源的。** `dsh-client-ui-input-trigger/lib/client.js:1552-1580`：

```js
registerSource(src) {
    if (live.sources.some((s) => s.trigger === src.trigger && s.name === src.name)) throw new Error(...);
    live.sources.push(src);
    for (const controller of live.controllers.values) try { controller.sourceAdded(src); } ...
}
```

唯一性是 **`(trigger, name)`**——同一个 `trigger` 下可以有多个 `name` 不同的 source。roster 按 trigger 合并并按 `order` 排序（`:1596`），菜单按 `state.groups` **逐组渲染**（`:1740`、`:1795`）。`showGroupTitle` 默认开着，内核自带的 reference source 显式关掉它（`dsh-client-ui-reference/lib/client.js:836` 附近的 `showGroupTitle: false`）——**也就是说「一个 `@` 菜单里两个分组，其中一组带标题」是内核支持的形态**。

**② 客户端插件可以注册 source。** 客户端模块的 `apply(ctx)` 拿到的 ctx 是服务代理，`inject` 数组声明要注入哪些服务；内核自带技能插件的声明就是：

```js
const inject = ["inputTriggers", "sessions", "slots", "locale", "remote", "remote.skills", "sidebarRight", "uiConversation"];
```

（`dsh-client-ui-skill/lib/client.js`）——它用这套服务注册了 `/` 菜单的 source。壳随包分发的内核插件走的是同一套客户端模块机制（`plugins/<id>/client/client.js` 的 `window.__ModuleLoader__.load({id, factory})`），`openai-oauth` 与 `memory` 两个插件都有 `client/client.js`。

> **红队修正（2026-10-20）**：初版此处写的是「openai-oauth / memory / **ponytail**」——**ponytail 只有 host 半边，没有 `client/client.js`**，它不参与客户端模块这条链。
>
> **更要紧的是：现有两个壳插件的 client 半边都只 `inject: ["slots"]`，没有任何一个注入过 `inputTriggers`。** 也就是说「壳分发的 client bundle 能不能拿到 `inputTriggers` 服务」**在本仓尚无先例**——内核自带技能插件能拿到，不等于壳分发的 bundle 也能拿到（两者的分发路径不同）。
>
> **而这条的失败模式是静默的**：`plugins/memory/client/client.js` 自己的注释写着「**投机加一个当前拿不到的服务，会让整枚徽标静默不加载**」——服务拿不到时，runner 会 parking 等它，**不报错、不进日志**。对 P1\* 而言，后果就是「`@` 菜单里静默地没有『官方对话』这一组」，用户只会以为功能不存在。**因此 §7 #10 的验收判据必须是「菜单里出现该分组」，而不是「插件加载没有报错」**；并且「静默不出现」必须被改造成可见的诊断（见 §4.7 第 1 项）。

**③ source 是「一等公民」，不是补丁。** 一个 source 可以给：`candidates(session, {query, quoted, drilled, signal})`（候选行，字段 `name` / `description` / `icon` / `section` / `value`）、`header()`（面包屑）、`onPick()`（选中后插什么）、`openReference()`（点 chip 时打开什么）、`codec`（剪贴板文本与序列化）、`lexicon`（chip 归属与失效通知）、`showGroupTitle`（分组标题）。

**④ 图标不必复用内核的。** 候选行的 `icon` 既可以是字符串（走 `ReferenceIconRegular kind=...`），**也可以直接是一个 React 组件**（`dsh-client-ui-input-trigger/lib/client.js:1804-1810`）——站点品牌图标可以由壳的插件自己画，不必冒充「文件」或「会话」。

**⑤ mention 的展开可以在 host 侧做。** 内核的 `dsh-session-reference` 是**宿主侧插件**，靠两个事件干活（`dsh-session-reference/lib/index.js:459`、`:470`）：

```js
ctx.on("system-prompt/assemble", async (_assembly, context, next) => { ... });
ctx.on("agent/pre-step", async ({ agent, signal }, next) => { ... });
```

它在 `agent/pre-step` 扫出消息里的 `dsh-session:` mention，为每个被引用的会话取一份**有界、只读、带固定不受信任警告**的快照，作为第二条 user 消息挂上去。**壳的 host 半边是同样的内核插件**（`plugins/<id>/host/index.js`），因此可以挂同样的事件、认自己的 scheme（例如 `official-chat:<id>`）、产出同样的快照语义。

**⑥ 引用在 composer 里是什么形态。** 选中候选后 `onPick` 返回 `{insert: {source: "reference", ref, label, appearance, clipboardText}}`——落成内核的 reference 节点（composer 里显示成 chip），发送时按 `codec.serialize` 序列化成文本。chip 提供来源与标签的可见入口；模型侧的角色与不受信任边界仍由 host 展开保证，不能靠标签外观推断。

> 由此，初版 §4.2 的「注入文本四要素（来源抬头 / 不受信任声明 / role 标注 / 自报截断）」**职责要搬家**：来源与条数进 chip 与候选行；不受信任声明、role 标注、截断与溢出通知**全部由 host 侧的快照产出承担**；自有来源需要实现并验证这些行为，不能把内核原生来源的成功当成已经复用完成。


### 4.2 选择、读取与固定版本

`onPick` 触发正文读取。读取期间保留当前草稿，允许继续编辑和取消；只对这份存在待完成引用的草稿限制发送，不能让用户选中了引用却发出一条遗漏该引用的消息。能否通过内核现有扩展完成异步选择与发送拦截，必须先实测，见 §7 #20；不得假设接口已经具备这些能力。

2026-10-20 补充代码证据：本机 rc.2 的 `dsh-client-ui-input-trigger/lib/client.js` 中，`settle()` 同步调用 `onPick` 后直接执行结果，没有等待 Promise。因此不能直接写成 `async onPick()`；先按开发计划 G1 验证现有扩展能否完成异步选择和发送控制，再进入依赖它的实现。

只有正文校验与快照保存都成功，才插入就绪的 chip。失败不插空标签，不清草稿，也不自动发送。关闭候选菜单只取消目录查询；因选中而收起菜单不能取消已经开始的正文读取。会话切换、账户切换及用户取消必须使正文请求失效；旧回包不能插到新会话或新草稿。

快照记录站点、账户范围、官网会话 id、分支、捕获时间、来源更新时间、结构化消息、完整性和内容摘要。chip 绑定不可变 `snapshotId`，不直接把可变官网会话 id 当作正文版本。正文和索引原子提交，重试不得把已有版本替换成另一份内容。

官网后续追加内容不改写已选引用。再次选择时读取当前内容并生成新版本；点击已有 chip 可查看实际引用内容、捕获时间与来源链接，并主动更新。预览是可选查看动作，不是引用前必走的确认面板。多媒体、工具与思维内容的支持范围需要明确；不能把未支持的内容冒充完整文本。

### 4.3 壳与插件之间的数据契约

复用 `plugins/builtin/bridge.rs` 的实例级回环桥和鉴权机制，按职责新增独立的官方对话路由模块（设计选择，尚未接线）。客户端经最小授权的壳命令查询候选、执行选中读取；host 经回环桥按固定快照 id 读取正文。桥接令牌不传给官网页面或客户端插件。

目录、捕获、正文读取与失效通知使用同一份来源和快照标识。命名空间由服务端根据壳模式、内核族、实例与站点账户确定，不允许调用方指定任意目录。网页只参与本站目录与选中正文的读取，不获得壳的缓存读写能力。

请求携带草稿/会话标识、请求代次与取消信号；响应区分就绪、未登录、账号已变、站点不支持、部分内容、超时和取消。状态通过原草稿附近呈现，日志只记录来源、请求标识、耗时和错误分类，不落 token 或对话正文。

### 4.4 发送与失败处理

发送时 `codec` 序列化固定快照引用，host 在 `agent/pre-step` 展开为紧随对应用户消息的不受信任 user 消息。按内核 `dsh-session-reference` 的捕获语义实现角色边界、引用数与字节预算、截断通知及必要的溢出检索。正文不得放进 `systemPrompt.context()`。

发送和同一次重试均读取固定版本，不重新访问官网。展开后保存实际模型可见的快照与省略事实，不能在每个模型步骤重复追加同一份引用。内容需要转义，避免来源标题或正文伪造包装边界。

| 情况 | 用户看到的行为 |
| --- | --- |
| 目录未加载 | 官方对话组显示加载状态，其他候选照常可用 |
| 未登录或登录过期 | 对应来源显示登录入口，不反复弹窗 |
| 读取失败或超时 | 原位置可重试、取消；草稿保留，不要求先导入 |
| 只拿到部分消息或分支不明 | 不标为完整成功；只有用户明确选择引用可用部分时才继续 |
| 内容超预算 | 按消息边界裁剪并在标签详情、模型上下文中明确省略；不能只在日志里说明 |
| 快照缺失或 owner/codec 失联 | 阻止这一条消息外发，显示具体失效引用，允许移除或重新选择 |
| 用户取消或切换会话 | 废弃旧回包，不插入引用，不清空新草稿 |

展开失败必须中止该轮模型调用，不能静默按没有引用继续。纯文本投递只在用户主动选择兜底时使用，不能以降级为由替用户发消息。

### 4.5 来源与数据流向

chip 的文字显示站点与标题，详情显示捕获时间、角色/内容范围和省略情况。接收方取实际模型路由的服务商/端点身份，不能只按模型名称推断，也不能在 B 侧提前记成某个固定会话的接收方。

正常的 `@` 选择不增加导入确认或逐次跨站弹窗。发送前以行内文字说明引用会随本条消息发送给当前服务商；用户可以查看、移除后再发送。模型切换时同步更新接收方，队列重试或路由变化时重新核对；不能用旧接收方信息掩盖新的外发目标。首次使用说明本机缓存与发送边界即可，不重复打断每次引用。

### 4.6 自动缓存与数据归属

目录缓存仅保存必要元数据；正文仅保存用户选中过的对话。缓存放在按壳模式与实例隔离的壳数据目录中，再按站点账户分区，不放入内核安装树，不写进项目工作区，也不伪造成内核会话。

未被草稿或在途请求引用的缓存自动按保留时间与容量上限回收。草稿仍引用的固定版本不能被普通缓存淘汰；恢复不了时明确失效，不能拿最新官网内容悄悄替代。具体容量、期限、落盘保护与崩溃恢复策略在实现前确定并测试。

用户可在设置中清理缓存，但不需要整理引用库。清理仍被草稿引用的数据时明确提示影响。已经随消息持久化的引用内容属于目标会话记录，清缓存不会删除已发送记录或撤回模型服务商已收到的数据。官网删除与退出账号也不等于撤回已发送引用。

### 4.7 尚需解决的工程缺口

1. **客户端扩展可用性**：现有壳插件只注入过 `slots`，`inputTriggers` 需要实际注册与选择验证。依赖缺失可能使 `apply` 根本不执行；由独立的壳侧检查注册回执与超时，不能仅靠失败插件自报。见 §7 #10/#16。
2. **异步选择与发送门控**：必须验证选中读取期间的编辑、取消与发送行为，以及旧响应不会污染另一个草稿。无法实现时先解决扩展接口问题，不恢复已被用户拒绝的前置导入。见 §7 #20/#21。
3. **host 展开与失效恢复**：memory 插件证明本仓能挂 pre-step，但其系统提示注入不是本功能的参照实现。固定版本、user 消息插入、错误可见和 owner/codec 失联处理仍需实测。见 §7 #11/#14/#15/#17。
4. **站点取数与账户范围**：浏览器登录态不等于任意 `fetch` 自动具备认证头。目录、分页、正文活跃分支、异步回程与账户变化分别验。见 §7 #3–#5/#8/#9/#22/#23。
5. **缓存与权限边界**：快照不再是用户管理的导入库，持久化、回收、草稿保留和跨进程鉴权仍是实现责任。见 §7 #18/#24。
6. **版本兼容**：P1\* 的候选、codec 与 chip 依赖内核扩展接口；需明确支持版本并检测能力。内核原生 `@session` 的成功不能替代自有来源测试。

---

## 5. 推荐路线

### v0：隔离环境验证完整选择链路

使用测试目录与固定结构化对话，验证 `@` 中可以出现来源、异步选中、固定版本、正常发送及 user 消息级展开。核对的是模型实际收到的正文和来源，不是标签显示成功。覆盖延迟、失败、取消、会话切换、重复选择、插件失联和快照缺失，测试不碰用户真实数据。

v0 只验证机制，不作为需要手动导入的产品版本发布。开发用固定数据不变成用户的操作要求。

### v1：一个站点的真实闭环

先选择完成认证、目录、正文与回程验证的站点，打通 `@` 按需列目录和选中读取。站点顺序由实测结果决定，不再把“接口最干净”当作未验证的承诺。已登录、已有对话的用户无需切窗、复制、导入或预览确认即可引用。

验收必须包含官网窗口不在前台与页签尚未挂载的情况、目录覆盖范围、读取期间发送门控、实际服务商展示、固定版本重试和缓存恢复。只完成剪贴板或纯文本投递不算通过。

### v2：其他站点与目录搜索

逐站增加支持，复用目录/捕获/快照契约与同一套 `@` 交互。搜索与分页按站点能力实现，不为补搜索而在后台下载全部正文。MiniMax 尚未验证，不能因为前两个站点成功就直接标为支持。

P3 保留为技术备选；不通过伪造会话文件把官网历史混入内核会话列表，也不默认交给模型主动召回。将来若增加主动召回，需要单独定义范围与用户控制。

---

## 6. 明确不做的事

1. **不往内核安装树写任何东西**（既有纪律）。`kernels/<版本>/node_modules/**` 只读。
2. **不在 B 页面里留下任何可被探测的注入物**——`chat-fingerprint.js` 删 `__TAURI__` 的既有目标不因为这个功能而放宽。取数走宿主侧执行（R1），而不是「往页面塞一个全局」。
3. **不抠 cookie**（B2）：本机实测运行时被独占锁死，且凭据要落盘扩散、指纹不一致。
4. **不做后台定时同步 / 全量正文拉取**：仅在用户浏览目录时按需取元数据，选中后读取该条正文；按需读取仍需单独评估站点规则。
5. **不伪造站点签名 / PoW / Turnstile**，只做只读。
6. **不为了这个功能改动已安装内核**。扩展能力不足时明确能力缺口，不直接修改安装树。
7. **不增加前置导入步骤**。正常流程只有工作台 `@` 选择与发送；剪贴板保持为用户自愿的故障兜底。

---

## 7. 待验证清单（动手前必须实测）

| # | 待验证 | 为什么重要 | 怎么验 | 状态 |
| --- | --- | --- | --- | --- |
| 1 | `session/prompt` 能否走 `POST /api/session/prompt` | 决定 P1 是否成立 | 用 `notify/task.rs` 的 cookie 流程试发 | ✅ **已实测通过**（2026-10-19，rc.2） |
| 2 | `session/prompt` 对**从未打开过**的会话是否需要前置调用 | 决定要不要先 `session/open` | 造一个空会话试一次 | ⬜ |
| 3 | B3 页内 fetch 能否拿到结构化 JSON（DeepSeek / ChatGPT） | 决定 B 侧主路径 | 注入脚本 + 页内 `fetch` | ⬜ |
| 4 | 三个站点的 CSP 是否放行页内 `fetch` 打自己的接口 | 决定 B3 是否成立 | 看响应头 `connect-src` | ⬜ |
| 5 | R1（`with_webview` + `ExecuteScriptWithResult`）在当前锁定的 Tauri/wry 上能否回传异步 fetch 的完整结果 | 决定 B3 的**回程** | Windows/macOS 各验延迟成功、Promise 拒绝、超时和导航取消；回传 JSON 而非 Promise 对象 | ⬜ |
| 6 | 外部写入的 v4 会话文件会不会被内核纳入会话列表与投影 | **决定 P3 是否成立** | 造一个最小会话文件，看 `session/list` 与 @ 菜单 | ⬜ |
| 7 | assistant 消息在 v4 里的事件类型 | 决定 P3 的工作量 | 造一个有真实对话的会话再解包看 | ⬜ |
| 8 | 三个站点各自的会话深链 URL 是否稳定 | 决定「引用哪一条」的定位方式 | 手测 | ⬜ |
| 9 | MiniMax agent 的会话列表 / 历史接口 | 决定第三个站点做不做 | 探一次真实请求 | ⬜ |
| 10 | 壳的客户端插件能否注入 `inputTriggers` 并注册 `@` 的第二个 source（**代码路径已核对**：`registerSource` 唯一性是 `(trigger, name)`、roster 按 trigger 合并、菜单逐组渲染） | **决定 A 侧 `@` 是否成立** | 写一个只注册 source、不含 host 半边的最小插件，看 `@` 菜单里是否多出一组 | ⬜ |
| 11 | 壳的 host 半边能否挂 `agent/pre-step` 并把自己的 scheme 展开成快照 | 决定 P1\* 主路线是否成立 | 照 `dsh-session-reference` 的两个事件写一个最小 host 插件 | ⬜ |
| 12 | 未知的 `appearance` / 自定义 `icon` 组件在 reference chip 与菜单行里的渲染 | 决定「来源一眼可辨」能不能做到 | 插一个自定义 appearance + 自定义图标组件看效果 | ⬜ |
| 13 | 内核对**未知 scheme** 的 mention 是否按普通文本处理（`dsh-session-reference` README 称「空或只含标点符号的 scheme mention 仍是普通讨论文本」） | 决定壳自己的 scheme 会不会被内核误解析 | 发一条带 `official-chat:` 的消息看它到模型手里是什么样 | ⬜ |
| 14 | host 展开后是否为 user 消息级上下文，且保留角色、预算与省略事实 | 决策已固定为 user 消息；需验证实现，没有系统提示分支 | 核对模型实际收到的完整消息，不调用真实用户会话 | ⬜ |
| 15 | `agent/pre-step` 展开失败时，**能否中止该轮次并让用户看见错误**（`dsh-session-reference` 的 README 称「准备失败…会终止该 agent 轮次」） | §4.6 要求「展开失败必须报错」，但没说怎么报；不验这一条，这条纪律落不了地 | 故意让快照缺失，发一条带 mention 的消息看用户看到什么 | ⬜ |
| 16 | client 半边**拿不到 `inputTriggers` 时**的表现 | 决定 #10 的失败是不是静默的（§4.7 第 1 项）；`plugins/memory/client/client.js` 的注释已写明这类失败「静默不加载」 | 故意 inject 一个拿不到的服务，看徽标/分组是否消失且无报错 | ⬜ |
| 17 | 插件升级 / 卸载时，composer 里**已存在的 chip** 怎么办 | 决定「发不出去」这个卡死场景有没有用户可见出口（§4.7 第 3 项） | 装 chip → 停掉插件 → 试发送 | ⬜ |
| 18 | 缓存落盘保护、期限、容量与草稿恢复 | 按壳模式、实例、站点账户隔离；自动回收不能删除在用版本 | 用临时目录验证隔离、容量淘汰、草稿保留与清缓存后的失效行为 | ⬜ |
| 19 | `session/prompt` 在**会话正忙**时的行为（内核有 `session/agent-busy` 错误码） | 决定 v1 降级路径（纯文本注入）在用户正聊着的时候能不能用 | 对一个正在跑的会话试发一次 | ⬜ |
| 20 | `onPick` 异步读取、取消与草稿发送门控 | 无导入交互的关键：不能选中了引用却漏发正文 | 延迟读取时继续编辑并尝试发送，取消/失败后可正常恢复；不清草稿 | ⬜ |
| 21 | 请求取消、菜单关闭、会话切换与乱序回包 | 旧响应不得插入新草稿或其他会话 | 控制响应乱序，核对目标会话、草稿代次与请求生命周期 | ⬜ |
| 22 | 站点未挂载时按需建立读取上下文 | 已登录用户无需先打开官网窗口 | Windows/macOS 验证 profile、真实 origin、初始化失败与无焦点切换 | ⬜ |
| 23 | 认证头、账户切换、目录分页与搜索覆盖 | 同源 fetch 不自动继承应用认证；目录不能冒充全部历史 | 各站分别验证切号、过期、翻页及活跃分支，明确部分覆盖 | ⬜ |
| 24 | 客户端命令与 host 回环桥共享正确的快照范围 | 候选和正文不能串壳、串实例、串账户 | 隔离环境验证鉴权拒绝、固定快照版本、重试与会话已存引用回读 | ⬜ |

> **内核版本**：本机 release 激活的是 **`0.2.1-rc.2`**（`active.txt` + 进程命令行 `kernels/0.2.1-rc.2/…/lib/bin.js web --no-open --port 3090`），dev 激活的是 `0.2.1-alpha.2`。本文引用内核包 README 的地方，已在 **rc.2** 上复核过引用机制部分（与 alpha.4 语义一致）；**其余内核行为随版本变化，仍要按「本文写作时的版本」理解。**

---

## 8. 资料来源

**本仓代码**

- `src-tauri/src/harness/official_chat.rs` — 页签表、条件页签、布局
- `src-tauri/src/harness/chat-fingerprint.js` — 页面指纹策略与 `__TAURI__` 删除
- `src-tauri/src/harness/harness-draft.js` — composer 写入（文本 / 粘贴附件）与读回核对
- `src-tauri/src/notify/task.rs` — 内核 RPC 客户端（cookie / `/api/<method>` / mux）
- `src-tauri/src/usage/local.rs` — zstd 会话日志增量扫描
- `src-tauri/src/commands.rs` — `add_official_chat_tab`、`ensure_official_chat_tab`
- `src-tauri/Cargo.toml` — tauri 2（unstable）、tungstenite、ureq、ruzstd

**已安装内核包**（`~/.dsh-xlink/dsh/desktop/kernels/0.2.1-rc.2/node_modules/@deepseek-ai/`，与 alpha.4 对照阅读）

- `dsh-client-ui-reference/README.zh.md` + `lib/client.js` — `@` 菜单的三类候选、两个 Remote 取数调用、插入形态
- `dsh-client-ui-input-trigger/lib/client.js` — **`@` 菜单的多源机制**：`registerSource` 的 `(trigger, name)` 唯一性、roster 合并与 `order`、逐组渲染、`showGroupTitle`、候选行字段（`name` / `description` / `icon` / `section` / `value`）、`icon` 可传 React 组件（§4.1.1）
- `dsh-client-ui-skill/lib/client.js` — **客户端插件注入 `inputTriggers` 的先例**（`inject` 数组含 `inputTriggers` / `sessions` / `slots` / `locale` / `remote` …）
- `dsh-client-modules/lib/index.js` — 客户端模块系统（`window.__ModuleLoader__.load({id, factory})`、注册与物化）
- `dsh-agent-preset/skills/cordis-plugin-development/templates/decoration/client.js` — 插件 client 半边的官方模板（`inject` + `apply(ctx)` + `ctx.slots`）
- `dsh-file-reference/README.zh.md` + `lib/index.js:54` — `@file` 语法与内核给模型的原话
- `dsh-file-reference-local/README.zh.md` + `lib/index.js` — 本地文件引用提供方、cwd 绑定与遍历限制
- `dsh-session-reference/README.zh.md` + `lib/typert.remote-client.js` — `@session` 捕获语义、预算、不受信任警告、spill
- `dsh-api-session-controller/README.zh.md` — Remote 命名空间与访问控制
- `dsh-api-session-controller/lib/typert.host.js:1307-1325` — **`session/prompt` 的 strict wire schema**
- `dsh-api-session-controller/lib/typert.remote-client.js` — 全部端点定义与行号
- `dsh-api-session-controller/lib/index.js:3218-3231, 925-927` — `prompt()` 主机侧实现、幂等、source 构造
- `dsh-api-session-controller/lib/types/commands.js` — `prompt()` 的 `content` 形态、`user/message` / `turn/*` 事件名
- `dsh-client-ui-conversation/lib/client.js` — Lexical 实例挂载点、附件四入口汇聚到 `intakeFiles`
- `dsh-client-file-upload/lib/types/http-route.js` — `uploadFileBinary` 路由
- `dsh-session-format/README.zh.md` — v4 格式与迁移链
- `dsh-session-persistence-jsonl/README.zh.md` — 磁盘布局、`compression` 可设为 `'none'`
- `dsh-session-query-sqlite/README.zh.md` — 全文搜索是可选能力

**本机实测**

- `POST /api/session/doesNotExist` → 404；`POST /api/session/prompt` → 200 + 逐级参数错误 + `session/not-found`（**§1.1 的判定表**）
- `~/.dsh/sessions/<cwd-slug>/session-<id>/session.v4.jsonl.zstd` 解压内容（头部 + `agent/inbox/spliced` 事件行）
- `~/.dsh/storages/session_projcache/sessions/*.json` 存在性
- `~/.dsh-xlink/dsh/desktop/kernels/` 的版本目录；`active.txt`（release=rc.2 / dev=alpha.2）
- 运行中内核进程命令行（`kernels/0.2.1-rc.2/…/lib/bin.js web --no-open --port 3090`）
- `~/.dsh-xlink/dsh/desktop/webview-official-chat/EBWebView` 存在性；其 `IndexedDB` / `Local Storage` 的 origin 清单
- `agent.minimaxi.com` 301 → `agent.minimax.cn`（两个域名均 200、字节数相同）
- `src-tauri/Cargo.lock` 中 `webview2-com` 0.39.1 / `windows` 的存在

**外部来源（官方条款与文档、第三方接口资料；站点接口未在本机复核）**

- DeepSeek Terms of Use（Last Update 2026-03-27）<https://cdn.deepseek.com/policies/en-US/deepseek-terms-of-use.html> —— 1.6 分享 URL、3.3 行为审查、3.5(3) 禁止自动装置抓取、8.2 封号措施
- OpenAI 使用条款（生效 2026-01-01）<https://openai.com/policies/terms-of-use/> —— 禁止「自动或以编程方式提取数据」、禁止「绕过保护措施」、8.2 暂停/终止
- OpenAI 帮助中心「在 ChatGPT 分享对话及排程任务」<https://help.openai.com/zh-hant-hk/articles/7925741-sharing-conversations-and-scheduled-tasks-in-chatgpt> —— 分享链接形态、快照语义、撤销路径
- DeepSeek 内部接口 <https://git.bytes.zone/Map9876/DeepSeek-backup-web> —— 两个 GET 端点、Referer 形态
- DSH 插件 `deepseek-web-import` <https://github.com/wpc0323/deepseek-web-import> —— 接口逆向确认、`userToken` 时效、`fetch_page` 100 条上限、fragments 结构
- ChatGPT 读取路径 <https://github.com/skillsynchq/txcript/blob/a8c8fc5c20112708ad07cf18a7bc60e02bdcbc836a8b5/docs/formats/chatgpt.md> —— `/backend-api/*`、mapping/current_node 结构
- MiniMax agent 接口 <https://gitgub.com/xiaoY233/MiniMax-Free-API> —— `/matrix/api/v1/chat/send_msg`、「原项目作者账号被封」；<https://github.com/root520521/hot-apis> —— `x-signature`/`yy` 签名
- Tauri 2 `WebviewWindowBuilder`（`initialization_script` / `on_navigation` / `data_directory`）<https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindowBuilder.html>
- 剪贴板插件 <https://crates.io/crates/tauri-plugin-clipboard-manager>

**协作分工**

本轮调研由三个 Bot 并行完成：devloper（内核侧 @ 机制与注入通道）、tester（B 侧取数途径与风险）、UX高手（交互形态）。A 侧的 wire 契约与全部本机实测由主会话完成并复核。**各 Bot 的结论都标了证据等级**——凡未在本机复核的都标了「未验证」。
