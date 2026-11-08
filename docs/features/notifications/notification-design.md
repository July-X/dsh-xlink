# 任务完成通知系统设计（macOS / Windows）

本文说明 dsh-xlink 的「工作台里跑完对话任务 → 系统角标 + 通知气泡」是怎么工作的、
为什么这样设计，以及每一处取舍的代价。实现见 `src-tauri/src/notify/`。

## 1. 目标与范围

| 项 | 取值 |
| --- | --- |
| 触发事件 | **两个工作台里某个会话的对话任务跑完**（一轮 turn 结束、agent 回到 idle）：DSH 内核事件流与 MiniMax WebUI 事件流各有一条订阅，见 §2.1 |
| macOS 提示 | Dock 图标右上角的系统数字角标 + 通知中心气泡 |
| Windows 提示 | 任务栏按钮右上角的数字角标（覆盖图标）+ 通知区域气泡 |
| 角标数字 | **两个工作台合计的、已完成但用户未读的任务数** |
| 来源标识 | 完成记录带来源（`dsh` / `mcode`）；系统通知首行与设置页列表都标出 `DSH` / `MiniMax` |
| 不在范围内 | 官方对话窗口（`chat.deepseek.com` 等外部站点，不经过内核）、内核自身的更新检查、插件/技能市场动作 |

「未读」的判定必须能被用户预期，所以规则只有两条：

1. 任务完成的那一刻**它所属的工作台窗口在前台**（用户正看着它）→ 不算未读；默认也不弹气泡（不打扰）。
2. 任务完成时那个工作台不在前台、或窗口根本没开 → 未读 +1、角标刷新、弹一条气泡。

未读清零的路径同样只有两条：**对应工作台窗口重新获得焦点**（用户回来看结果了，只清该来源），
或管理面板里的「全部已读」（清全部）。

> 为什么不按消息流精确到「读到哪一条」：内核的未读状态属于内核，壳只能观察
> 到「任务结束」这一事件；把"焦点回到工作台"作为已读信号，语义清楚、行为可
> 预测，也不会让角标长期挂着不动。

## 2. 结构

```text
        ┌──────────────────── DSH 内核进程（dsh web, 127.0.0.1:<port>）────────────────────────────┐
        │  agent/status ──► api-session/status(sessionId, running)         ┐                       │
        │  session/created ──► api-session/added(summary)                  │ $events               │
        │  session/title ──► title 投影变化 ──► session/control 的投影帧   ┘ （两条逻辑流）        │
        └───────────────────────────────────────────┬──────────────────────────────────────────────┘
                                                    │ WebSocket /api/remote.mux（与工作台同一条通道）
        ┌──────────────────── MiniMax WebUI（127.0.0.1:<port>）─────────────────────────────┐
        │  session.start / created / title_updated / finish / abort / error                 │
        │  ──► watchEvents 应用事件流（protocolVersion = 1）                                │
        └───────────────────────────────────────────┬──────────────────────────────────────────────┘
                                                    │ WebSocket ws://127.0.0.1:<port>/
                    ┌───────────────────────────────▼───────────────────────────┐
                    │ notify/task.rs（DSH） 与 notify/mcode_watch.rs（MiniMax）│
                    │  · 各按自己的判据判完成   · 各自认证、去重与 1→20 s 退避 │
                    │  · 两条订阅互不影响，失败各报各的                        │
                    └───────────────────────────────┬───────────────────────────┘
                                                    │
                        ┌───────────────────────────▼────────────────────────────┐
                        │ 通知中心（全局状态）                                    │
                        │  unread（合计）/ 最近完成（≤8，带来源）/ running / 标题 / 错误  │
                        └───────┬───────────────────────────────┬────────────────┘
                                │                               │
              ┌─────────────────▼──────────────┐   ┌────────────▼─────────────────────┐
              │ 系统角标                        │   │ 通知气泡                          │
              │ macOS: NSDockTile.setBadgeLabel │   │ 平台原生接口（Rust 侧直接调用）   │
              │ Windows: SetOverlayIcon(位图)   │   │ (macOS UserNotifications /      │
              │                                 │   │  Windows WinRT toast)            │
              └─────────────────────────────────┘   └──────────────────────────────────┘
                                │
              ┌─────────────────▼──────────────────────────────────────────────────┐
              │ 管理面板「任务通知」卡片：两个内核共用一套开关 / 试听 / 未读       │
              │ 事件 `notification-status` 推送快照（含 watchingMcode / mcodeError）│
              │ （「模拟一次任务完成」只在 dev 构建里渲染）                          │
              └────────────────────────────────────────────────────────────────────┘
```

### 2.1 两个内核的任务通知（2026-11-08 起）

MiniMax 工作台（WebUI 0.1.7）与 DSH 是两套进程、两套协议，任务完成事件只能各订阅各的；
但**开关、未读账与通知出口只有一套**。此前这套功能只认 DSH，设置卡标题在实验构建里显示
「DeepSeek 任务通知」，MiniMax 跑完任务没有任何提醒。用户要求两个工作台都通知，并在界面上分得清来源。

**为什么共用一套开关。** 三个开关（总开关 / 不在前台才通知 / 声音）回答的是「用户要不要被
打扰」，与任务由哪个内核跑完无关；按内核拆成两套只会在设置页摆出六个同样的开关，而实际使用
里几乎没人会这么分。§7 的三个字段原样不动，两个工作台共用；真有按内核分开的需要再加不迟。

**两条订阅、两套判据。**

| | DSH | MiniMax |
| --- | --- | --- |
| 订阅位置 | `/api/remote.mux` 的 `$events`（+ `session/control` 取标题） | `ws://127.0.0.1:<port>/` 的 `watchEvents` 应用事件流 |
| 认证 | 内核日志里的 launch token 换 `dsh-auth-*` cookie | 无认证（只连本机工作台自己的端口） |
| 完成判据 | `api-session/status` 的第二参数从 `true` 变 `false` | `session.finish`（一轮正常结束） |
| 不算完成 | 子代理会话（`parentSessionId` / `origin == "subagent"`） | `session.abort` / `session.error`；子代理会话（`parentSessionId`） |
| 标题 | `session/control` 的 baseline + 标题投影 | `session.created` / `session.title_updated` 的 `title` |
| 去重 | `(sessionId, 完成时刻)` | `turnId`（保留最近 64 个；缺 `turnId` 时退回 `sessionId#完成时刻`） |
| 时长 | 运行中记开始时刻，完成时相减 | `session.start` 记开始时刻，`session.finish` 相减 |
| 正文额外信息 | 最近一轮提问 / 回复（`turnOutline` 投影） | 无（事件流不带会话正文） |
| 断线 | 1 s → 20 s 退避，每次重连重新认证 | 1 s → 20 s 退避；工作台没运行时只轮询状态、不连端口 |

MiniMax 侧的订阅在 `src-tauri/src/notify/mcode_watch.rs`：先把 WebUI 的私有握手封装成一条
`watchEvents` 请求（`protocolVersion = 1`、`kind = "request"`、`operation = "watchEvents"`），
收到 `kind = "response"` 的 ack 即算在线，之后按 `body.type` 消费事件。这条流是**上游私有协议**，
壳只承诺当前交付的 WebUI 0.1.7（安装与启动本来就只认这一版）：协议变了不会静默降级——连接或
解析失败写壳日志（`notification-mcode`，60 s 节流），并把「MiniMax 事件流未连接」连同原因报给
面板；DSH 通知与工作台本身都不受影响。

**来源标识。** 完成记录 `CompletedTask` 带 `kernel: "dsh" | "mcode"`（serde 小写），
`TaskKernel::label()` 给 UI 短名 `DSH` / `MiniMax`。两条订阅最后都走同一个
`record_completion()`，所以「总开关 → 前台免打扰 → 入列表 / 未读 / 系统通知」只有一份实现。
系统通知正文首行是 `📋 DSH · 会话标题 ✅` / `📋 MiniMax · 会话标题 ✅`；平台通知标题仍是
「任务已完成」，两个内核都弹时靠首行分辨。设置页「最近完成」列表在标题前放来源 chip：DSH 走
中性色，MiniMax 用现有 `.minimax-wordmark`（手写体 + 实验粉）。旧载荷没有 `kernel` 字段时前端
按 `dsh` 兜底。

**未读按内核分开。** 角标数字是两个工作台的**合计**，但清零分开：DSH 工作台窗口（`harness`）
获得焦点只清 DSH 来源，MiniMax 工作台（`mcode-workbench`）获得焦点只清 MiniMax 来源——切回
一个工作台不该替用户读过另一个工作台的记录；「全部已读」仍清全部。窗口被销毁时两扇窗口都会
显式清掉各自的前台标记，否则标记卡在 `true` 会让随后的完成永远不被当成未读。停止工作台时
同样只清自己那份未读（`stop_kernel` 清 DSH、`stop_mcode` 清 MiniMax）并停掉对应订阅线程；
壳退出（`RunEvent::Exit`）两条都停。

**点击行为不同。** DSH 通知经 `navigation.rs` 排一个目标，工作台加载完成后由内部客户端
`openSession(sessionId)` 深链到那条会话。MiniMax WebUI 没有可供壳深链的会话路由，因此 MiniMax
记录只登记「来源 = MiniMax」：点击时把 MiniMax 工作台窗口抬到前台（`show` + `unminimize` +
`set_focus`），窗口没开就回到管理面板——不凭空新开窗口，也不假装定位到了某条会话。

**生命周期。** `start_mcode` / 重新打开 MiniMax 工作台成功后调用 `mcode_watch::start()`
（幂等，重复调用不会叠线程）；`stop_mcode` 与壳退出调用 `stop()` 并清 MiniMax 未读。
工作台没起来时订阅线程每秒看一次 `mcode_service::status()`，不连端口。

## 3. 检测层：订阅内核事件流

### 3.1 为什么不用轮询

内核的 `/api/session/list` 会为**全部会话**重建投影（标题、turn 大纲、上下文
时间线……）。本机实测（193 个会话）：

```sh
# 单次调用：2 112 448 字节 / 0.47 s
# 连续 8 次：墙钟 3.73 s，而内核进程的 CPU 时间增加 3.97 s
```

即**一次轮询烧掉约 0.5 s 内核 CPU**（该接口本身把内核压在单核 100%），因为
它要重新遍历 / 解析所有会话的 Zstandard 日志头部。以通知所需的秒级延迟去轮询
它，等于给内核常驻加一个核的负载，代价与该功能的收益完全不成比例。仓库里
`dsh-session-perf` 补丁和 `scripts/verify-dsh-session-perf.mjs` 正是为了压这个
接口的成本，更不该由外壳把它重新拉起来。

### 3.2 实际采用：`/api/remote.mux` 的 `$events` 流

工作台 webview 之所以能实时刷新，靠的就是这条 WebSocket：内核的
`@deepseek-ai/dsh-api-gateway` 在 `/api/remote.mux` 上多路复用所有逻辑流，其中
`$events` 承载应用转发的 Cordis 事件。壳以**同一个内核的另一个客户端**身份
接进同一条流。

握手与帧格式（`dsh-api-gateway/lib/types/stream-protocol.js` 是权威定义）：

```text
1) GET http://127.0.0.1:<port>/?token=<launch token>   → 303 + Set-Cookie: dsh-auth-*
   （token 每行由内核打到 stdout，壳已经在读：commands::kernel_workbench_url_from_log）

2) WS  ws://127.0.0.1:<port>/api/remote.mux
   Header: Cookie: dsh-auth-…=v1.…      Origin: http://127.0.0.1:<port>

3) 客户端 → 服务端（逻辑流请求，字段必须精确匹配）
   {"type":"open","streamId":"…","endpoint":"$events","payload":{"args":{}}}

4) 服务端 → 客户端
   {"type":"item","streamId":"…","value":{"type":"ready","clientId":"…","host":{…}}}
   {"type":"item","streamId":"…","value":{"type":"emit","event":"api-session/status","args":["<sessionId>",true]}}
   {"type":"item","streamId":"…","value":{"type":"emit","event":"api-session/added","args":[{…summary…}]}}
   {"type":"end"|"error","streamId":"…",…}

5) 服务端每 30 s 发 WebSocket Ping，连续两次没收到 Pong 就断开；因此读循环
   必须持续 `read()`（tungstenite 会在读路径上把 Pong 写回去）。
```

`api-session/status` 的来源是内核自己的一行映射
（`@deepseek-ai/dsh-api-session-controller/lib/index.js`）：

```js
ctx.on("agent/status", ({ agent, status }) => {
  ctx.emit("api-session/status", agent.id, status === "running");
});
```

所以 `args[1] === false` 就是"这个会话的对话任务结束了"，这正是我们要的判据。
同一条流里还有：

| 事件 | 用途 |
| --- | --- |
| `api-session/added` | 新会话摘要（含 cwd、`parentSessionId`）→ 子代理标记的来源；**标题在这里通常是空的**，见 §3.3 |
| `api-session/activity` | 用户消息写入会话 → 只用作旁证，不参与判定 |
| `api-session/error` | 会话级错误 → 预留（当前不弹，避免与事故面板重复打扰） |

> **id 形态**：事件里的 `sessionId` 是不带前缀的裸 uuid，而 `session/control` 的
> baseline 与 `session/list` 里是 `session-<uuid>`。壳统一去掉 `session-` 前缀后再
> 建索引（`notify::normalize_session_id`）。

### 3.3 会话标题：第二条逻辑流 `session/control`

**标题不是事件，是一个投影**（`@deepseek-ai/dsh-session-title`）：

```js
const titleProjectionDefinition = {
  key: "title",
  init: () => null,                                   // 创建时为空
  apply: (state, event) => event.type === "session/title" ? event.data.title : state,
};
```

也就是说：`api-session/added` 在 `session/created` 那一刻发出（
`dsh-api-session-controller` 的 `ctx.on("session/created", …)`），此时标题投影还是
`null`；标题要等第一轮之后由模型生成、作为 `session/title` 事件写进会话日志，
投影才变成字符串。而**投影变化只推给 `session/control` 的订阅者**：

```js
ctx.sessionProjections.onChanged((session, key, value, seq) => {
  this.broadcast({ type: "projection", sessionId: session.id, key, value, seq });
});
```

于是只听 `$events` 的壳永远拿不到"连接期间新建的会话"的标题，通知只能退回
「未命名会话 <短 id>」——这正是 v0.1.3-rc.3 里的那个 bug（v0.1.4 修复）。现在壳在**同一条物理
连接**上再开一条 `session/control`，吃它的两类帧：

```text
{"type":"open","streamId":"dsh-xlink-titles","endpoint":"session/control","payload":{"args":{}}}

{"type":"item","streamId":"dsh-xlink-titles","value":{
   "type":"baseline",
   "value":{"queues":{…},"jobs":{…},"projections":{
      "session-<uuid>":{"asOfSeq":12,"values":{…,"title":"精简通知设置并显示测试按钮"}}}}}}

{"type":"item","streamId":"dsh-xlink-titles","value":{
   "type":"projection","sessionId":"session-<uuid>","key":"title","value":"新名字","seq":30}}
```

- `baseline`：连上时已经在内存里的会话的投影快照 → 直接播种标题表，并置
  `titles_seeded`；标题被清空（`value: null`）时同步清掉本地缓存，让占位文案接管。
- `projection`：只认 `key == "title"`；其它投影（todos / inbox / contextTimeline
  等高频项）直接忽略，既不建索引也不广播。
- 命中标题变化时会把面板里那条"最近完成"记录的名字一起改掉再广播
  （`set_title` + `broadcast_now`），否则改名后历史列表还挂着旧名字。

**成本**：baseline 是"常驻会话的**全部**投影"，本机 4 个会话实测 215 KB（其中
大头是活跃会话的 `contextTimeline`），而 `session/list` 是 663 KB / 205 个会话、
而且是每次都要重建全部会话投影的 CPU 重活。用控制流换来的是"标题永远是最新的"，
且只在连接时读一次；没有它时（老内核）才退回 §3.1 那个快照：

| 情形 | 行为 |
| --- | --- |
| 控制流可用 | baseline 播种 + 投影帧保鲜，**不再**调 `session/list` |
| 内核不认 `session/control`（`{"type":"error"}`）、流中断、或 5 s 内没有 baseline | 退回一次 `session/list` 全量快照（每条连接最多一次；失败留给下次重连重试） |

每条连接还会顺手重置这套状态（`titles_stream`），所以内核重启后重连能重新播种。`last_turns` 同理按连接生命周期清空——**已完成任务的正文不受影响**，因为 `last_prompt` / `last_response` 在完成那一刻已经拷进 `CompletedTask`。

### 3.4 子代理会话过滤

子代理（subagent）各自是一个会话，跑完同样会发 `api-session/status(false)`，
但它们不该打扰用户。事件里没有父会话字段，因此壳依赖 `api-session/added`：
摘要里带 `parentSessionId`（或 `origin == "subagent"`）的会话被登记进
`subagents` 集合，之后其状态事件只记账、不通知。

已知边界：如果子代理在壳**连接之前**就已创建，且此后从未产生新的 `added`
事件，它的完成会被当成普通任务。代价是偶发的一条多余通知，换取的是实现简单
（不需要为每次通知去查一次 2 MB 的会话列表）。

### 3.5 标题的来源与代价（小结）

| 来源 | 覆盖 | 代价 |
| --- | --- | --- |
| `session/control` 的 `baseline` | 连接时已经在内存里的会话 | 开流时一次性读取（本机实测 4 个会话 215 KB） |
| `session/control` 的 `projection`（`key == "title"`） | 连接期间生成的标题、手动改名、清空 | 每帧一个很小的 JSON，只认 title |
| `api-session/added` | 连接期间创建 / 载入的会话（含 cwd） | 零成本（事件本来就要读） |
| `session/list` 快照 | 仅当前三条都不可用时兜底（老内核） | 每个内核进程最多一次，约 0.5 s 内核 CPU |
| 都没有 | — | 文案回退成 `未命名会话 <id 前 8 位>` |

### 3.6 认证、重连与退避

- 认证：每次连接都重新从当天内核日志里取**最后一条** launch token 并换一次
  cookie。内核重启会签发新 token，旧 cookie 立刻失效——"每次重连都重新认证"
  让内核重启后自动恢复，不需要额外状态。
- 读循环：底层 `TcpStream` 设 400 ms 读超时，超时（`WouldBlock`）只表示"这一
  tick 没有数据"，用来周期性检查停止信号；tungstenite 会从断点续读，不会丢帧。
- 断线：指数退避 1 s → 20 s 封顶，直到内核停止或壳退出。
- 停止：`stop_watcher()` 置标志并**最多等 3 秒**回收线程；内核停止、壳退出、
  以及 `RunEvent::Exit` 都会调用它。

## 4. 平台映射

### 4.1 macOS：Dock 数字角标

Tauri 的 `Window::set_badge_label(Option<String>)` 直接落到
`NSApp.dockTile.setBadgeLabel:`（tao 的 `platform_impl/macos/badge.rs`）。这就是
系统原生的角标：位置、圆角、字体、深浅色适配全部由系统绘制，**不需要自绘任何
位图**，也不会随 macOS 版本漂移。

- 未读 0 → `None`（摘掉角标，而不是显示 "0"）；
- 未读 > 999 → `999+`。

### 4.2 Windows：任务栏按钮的数字角标

Win32 的任务栏**没有**数字角标 API（那是 UWP 的 `BadgeNotification`），系统风格
的做法是覆盖图标：`ITaskbarList3::SetOverlayIcon`，Tauri 的
`Window::set_overlay_icon` 就是它。因此数字必须由壳自己画进覆盖图标里。

绘制规则（`notify::render_badge`，纯函数、可单测）：

- 画布 32×32 RGBA，除角标外**全透明**（否则任务栏上会出现一个色块）；
- 内容放在画布**右上角**，这样合成到任务栏按钮上就是"右上角数字角标"；
- 单个数字用圆形徽标（字高 15 px），多位数字自动变胶囊形并缩小字号，
  超出 999 显示 `999+`；
- 颜色：`#E81123`（Windows 状态红）+ 白色数字，与 macOS Dock 的系统红底白字
  保持同一套视觉语言；
- 边缘用 4×4 超采样做抗锯齿，避免 16 px 下数字糊成一团；
- 数字用 3×5 点阵字体硬编码在源码里：不引入图像/字体依赖，不需要在构建期
  渲染位图资源，任意位数与 DPI 都能重新排版。

尺寸取 32 而不是 16：覆盖图标在 100% DPI 下由系统按 16×16 绘制，150%/200%
DPI 下按 20/24 绘制——给一张 32×32 的源图让系统缩小，比给 16×16 让它放大清楚
得多。

数字同时落在**管理面板与工作台两个窗口**上：macOS 的 Dock 角标是全应用一个，
而 Windows 上每个窗口是一个任务栏按钮，用户看向哪一个都应该看到同一个数字。

### 4.3 通知气泡

投递统一经过 `platform.rs`：先裁剪隐私内容，再交给原生通知接口。通知使用不含会话标题、正文或项目路径的随机标识，真实目标留在壳内。

| 平台 | 当前后端 | 注意 |
| --- | --- | --- |
| macOS | UserNotifications 的 `UNUserNotificationCenter` 与自有 delegate（委托回调） | 从 `didReceiveNotificationResponse` 读取被点击请求的 identifier（标识符），只接受打开通知的默认动作；旧 Foundation 接口仅用于清理历史通知。安装产物仍需验证系统实际展示与点击行为 |
| Windows | Windows Runtime（Windows运行时）原生 toast 通知 | 使用应用标识、tag（标签）和按壳模式区分的 group（分组）；注册通知自己的激活回调 |

#### 4.4 「只有角标、没有横幅」：Windows 通知总开关

2026-09-28 实测（本机 Windows 11）：设置 → 系统 → 通知里的**总开关关着**时，
`ToastNotifier::Show` 照常返回 `S_OK`，`notify-rust` 也就照样 `Ok(())`——但气泡
不出现，通知中心里也没有记录，**壳拿不到任何错误**。同一时刻角标照常变：角标走
`ITaskbarList3::SetOverlayIcon`（任务栏 API），与通知平台毫无关系。「只有数字红点、
没有横幅」就是这么来的——两者从来不是同一套东西，排查时别把它们当成一个功能的两半。

实测记录：总开关关闭时 `ToastNotifier::Setting()` 对**任意** AUMID 都返回
`DisabledForUser`——本应用 `com.zhongxingxing.dsh-xlink`、
`Microsoft.Windows.PowerShell`、`Windows.Defender.SecurityCenter`、
`com.nvidia.nvapp` 无一例外；同一时刻
`HKCU\Software\Microsoft\Windows\CurrentVersion\PushNotifications\ToastEnabled` = 0。
反过来，用 `CreateToastNotifier()`（不指定 AUMID）直接失败
（`0x80070490 找不到元素`），所以"不传 app_id 就完事"不是退路。

`notify_gate.rs` 据此在两处如实上报，而不是让失败静过去：

| 位置 | 行为 |
| --- | --- |
| `status()` | `notifications_blocked = true` + `environment_note` = 指向「设置 → 系统 → 通知」的说明，设置页以灰字展示；轮询每 2.5 s 一次，用户打开开关后说明自动消失 |
| `show_toast()` | 返回该说明作为错误，但 `remember_toast_error` **不把它记进 `lastError`**：同一句话已经由设置页的灰字展示，再记一份就是顶部 toast + 灰字 + 黄色告警框把同一句话说三遍。判据是**逐字相同**（两个常量同源），不是"环境被挡住了"——真正出乎意料的失败哪怕发生在同一个被挡住的构建里，也照实留在 `lastError` |

判定用 `ToastNotifier::Setting()` 而不是读那个 `ToastEnabled` 注册表值：前者是
系统的权威接口，一次覆盖「用户关了总开关」「只关了本应用」「组策略禁用」三种情形。
只把 `Enabled` 当放行，其余取值一律按阻塞处理。**问不到就按没被关处理**——宁可少
显示一条说明，也不谎报"你的通知被关了"，后者会让用户去翻一个根本关着的开关。

macOS 侧的同类判定（未打包构建拿不到应用 bundle）也一并搬进了 `notify_gate.rs`：
与状态机无关的"平台这一关卡没卡住"归它管，`notify.rs` 只回答"要不要弹"。

提示音：`sound = true` 时请求系统默认提示音（macOS 传
`UNNotificationSound.defaultSound`、Windows 传 `Default`），关闭时不设置任何
声音——两端不设置即为静音。

### 4.5 点击通知定位到具体会话

`platform.rs` 收到通知自身的激活回执，把随机标识交给 `navigation.rs` 查询所属会话、实例及壳模式。Windows 保留 Builder（应用构建器）之前的单实例认领与管道交接，激活参数可携带标识；普通启动项仍只唤起工作台。macOS 不再用应用激活时间或最近完成记录推断通知点击，也不再依赖旧 `didActivateNotification` 回调。新委托允许系统后台队列调用，回执处理结束必调用 completion handler（完成回调）。

工作台未打开时复用 `open_harness` 建窗。壳保留最新一次点击目标，内部 `shell-navigation` 客户端模块通过 `notification_navigation_ready` 获取目标；列表加载完成后，使用 `uiWorkspace.openSession(sessionId)`。导航请求提交后经 `notification_navigation_result` 确认，旧回执不能清除新点击。目标不存在、实例变化、内核未运行或导航服务30秒未就绪，均给可见说明，不创建会话或另启实例。旧内核缺少所需公开客户端模块时，启动链摘除内部接线并记录原因，避免影响工作台加载。

macOS 首次发送时申请系统通知授权。拒绝授权或投递失败会写入通知状态与日志，沿用现有设置页显示原因；授权状态会重新读取，用户在系统设置中允许后可重试，不新增设置页面。通知初始化与裸开发构建隔离，开发构建不请求系统权限。

点击处理依据 [Apple 的通知响应接口](https://developer.apple.com/documentation/usernotifications/unusernotificationcenterdelegate/usernotificationcenter(_:didreceive:withcompletionhandler:))；[系统授权说明](https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications)规定，显示横幅和播放提示音需要用户许可。

内部模块复用内置资源准备、物化与接线流程，不进入可选插件列表，不要求用户启用额外开关；工作台权限仅包含取目标与回执两条命令，后端仍检查窗口来源。

已对本机安装的内核 `0.2.2-alpha.2`、`0.2.2-alpha.3` 发布包做只读检查，均存在上述公开导航服务。该证据仅证明静态接口存在，不等于两个版本均完成了安装版通知点击验收。

MiniMax 的完成通知没有可深链的会话路由：点击时经 `navigation.rs` 认出「来源 = MiniMax」后，
把 MiniMax 工作台窗口抬到前台，窗口没开就回到管理面板（见 §2.1）。DSH 与 MiniMax 的点击目标
各自排队，同一时刻仍只有一个待处理目标。

### 4.6 锁屏隐私

锁屏或状态未知时，通知仅显示“任务已完成”，不含会话标题、问题、回复、项目名或路径。macOS 从当前图形会话字典读取状态，Windows 查询当前会话的 WTS（Windows终端服务）状态；应用焦点不参与锁屏判断。隐私监视器未成功启动时，也不发送敏感正文。

后台每500毫秒检查状态，锁屏时清除本应用当前壳模式已投递的敏感通知，保留通用隐私通知及未读计数；投递时和 macOS 主线程真正发送时均重查状态。解锁不补发旧正文。仅用户已点击的待处理目标会在解锁后继续导航，单独解锁不会打开未点击通知对应的会话。

500毫秒轮询及投递后的复查不能提供操作系统级的原子锁屏保证。系统实际展示、锁屏瞬间的竞态、通知中心清理和睡眠唤醒仍须在两平台安装版验证；当前自动化结果不能替代这些证据。

#### 通知正文（用户最后一条提问 + 用时）

```
📋 DSH · 会话标题 ✅
💬 <turnOutline 末项的 prompt，超 15 字截断>
用时 3 分 20 秒
```

MiniMax 记录的首行同理换成 `📋 MiniMax · 会话标题 ✅`；它的私有事件流不带会话正文，因此那条
通知只有首行与用时。系统通知**标题**统一为“任务已完成”（平台只给一个字面标题，两个内核都弹时
靠正文首行分辨来源）。解锁后的正文沿用会话标题、最近提问及用时；锁屏时正文也替换为固定提示。

**用时排在最近一次对话之后，不挂在标题行。** 曾经是「📋 会话标题 ✅ · 用时 3 分
20 秒」：时长量的就是**这一轮**，紧跟在这一轮之后才读得出"这次跑了多久"；挂在
标题行还有个更实际的问题——会话名一长，气泡里先被截掉的就是标题，后面缀着的时长
却还占着位置。测试 `duration_is_appended_after_the_last_conversation_not_to_the_title`
钉住这一点。

**完成时刻不进正文。** 曾是尾行的「用时 3 分 20 秒 · 完成于 14:32」，现在时刻整个
去掉。通知讲的是"刚刚发生的这一轮"，而时刻是它最不缺的信息：macOS 与 Windows 的
**横幅都不显示时间**（只有 macOS 通知中心里的条目带），正文再写一遍等于替系统说
它不会说的话，还白占一行。要查确切完成时间的地方是设置页「任务通知」的最近完成
列表（逐条列了完成时间与用时）。同一个测试顺带断言正文里不出现「完成于」。

那一行来自内核的 `turnOutline` 投影（每轮的 prompt / response 预览）。
数据与标题同源同路：`session/control` baseline、`turnOutline`
投影变化帧、`session/list` 快照、`api-session/added` 摘要四个来源共用一套
吸收逻辑写入 Center 的 `last_turns` 表；会话完成（`api-session/status` 转
idle）时把末项带进 `CompletedTask`。老内核没有该投影时该行省略，只剩标题与用时；
设置页「任务通知」卡的**最近完成列表**逐条展示
同一份数据（会话名、完成时刻、用时、问 / 答预览）。

两处只在**展示层**做的处理，存的始终是完整原文（`CompletedTask.last_prompt`）：

- **按字符截断到 15 字**，加省略号。按字节截会把中文切出半个字，通知里就是
  一个乱码方块；不加省略号则会被读成"他就说了这么多"。先 `trim`——提问常带
  首尾换行，不去空白的话只有空白的提问会占掉一行并被截成"…"，看起来像真的
  说了什么。macOS 通知最多三行、Windows 更短，全量塞进去会把尾行的用时挤出
  可视区，而那行恰恰是用户判断"刚才跑了多久"的关键。
- **用 💬 前缀而不是「最近对话：」文字标签**。通知正文是纯文本，没有可供排版
  的元素，"这一行是什么"必须靠前缀自己说清。文字标签既占宽度，又和上方标题
  的措辞混在一起，扫一眼分不出哪行是结论、哪行是上下文。💬 与概览页「官方
  对话」按钮同符号，词汇一致。
- **首行带来源**（2026-11-08）：两个工作台都会弹通知，光看会话标题分不出是谁跑完的；
  来源名与设置页列表的 chip 用同一套短名（`DSH` / `MiniMax`）。
- **标题行用两个符号**：`「」` 换成 📋（"这是个任务"），`已完成` 换成 ✅
  （"它完了"）。**两个都留是有意的**——📋 说"是什么"，✅ 说"怎么样"，砍掉
  任何一个都得读文字才补得回来。测试
  `title_line_carries_both_a_task_icon_and_a_done_symbol` 钉住这一点。字面量
  内联而非提成常量：本文件已顶在代码预算的反棘轮上限（1170 行）上，多一行
  常量就得从别处砍等量的逻辑。
  设置页的列表**不截断、也不加图标**：那里有空间，且行内已有 `:title` 悬浮
  全文。

#### 「试听提示音」为什么单独走一条通道（`notification_test_sound`）

面板上的「试听」按钮**不发通知**，而是直接调系统的提示音接口：

| 平台 | 调用 | 播放的是什么 |
| --- | --- | --- |
| macOS | `AudioServicesPlayAlertSound(kSystemSoundID_UserPreferredAlert = 0x1000)`（AudioToolbox，`#[link]` 直接 FFI，无新依赖） | 用户在「系统设置 → 声音 → 提醒声音」里选的那个声音，与通知气泡默认提示音同源 |
| Windows | `MessageBeep(MB_ICONASTERISK)`（`windows-sys`，仅多开 `Win32_System_Diagnostics_Debug` 特性） | 用户声音方案里「通知」类的音效（Windows 10/11 默认方案即通知默认音效） |

三个理由：

1. **dev 构建也要能听到**。macOS 上未打包的二进制投递不了通知（见下一节），
   若试听靠"发一条带声音的通知"实现，`tauri dev` 里按下去只会静默——而这恰恰
   是最常需要试听的场合。
2. **不依赖通知权限**，也不写未读、不动角标；用户听到的就是系统提示音本身。
3. 两个接口都是"交给系统后立即返回"的异步播放，不派生子进程、不阻塞命令线程
   （所以 `notification_test_sound` 是同步命令，不需要 `spawn_blocking`）。
   失败只有一种可检测的原因——Windows 上 `MessageBeep` 返回 0（没有可用输出
   设备），此时返回带下一步的中文说明；macOS 的接口没有返回值。

「试听」在声音开关关闭或总开关关闭时禁用：开关说"静音"、按钮却出声，两个信号
会自相矛盾。成功路径上给一条轻提示（`已播放系统提示音`）——声音没有画面反馈，
不提示的话用户分不清"系统静音了"和"按钮没点动"。

#### 为什么不用 `tauri-plugin-notification`

那个插件的 `init()` 会无条件 `.js_init_script(init-iife.js)`，也就是把一段
shim **注入每一个 webview**（内核工作台、三个官方对话站点、日志窗口、页签栏）。
这段 shim 在 macOS 上页面一加载就执行：

```js
"default" !== window.Notification.permission || __TEMPLATE_windows__
  ? … : await invoke("plugin:notification|is_permission_granted")
```

工作台与官方对话窗口的 capability 里没有 `notification:allow-is-permission-granted`
（这是有意的：不给远程内容发权限），于是**每次打开工作台都会产生一个未处理的
Promise 拒绝**——而工作台注入的 `harness-health.js` 正把 `unhandledrejection`
当作要上报的前端故障，用户会看到事故横幅。它还有两个副作用：

- 在官方对话页面上装了一个带 Tauri 痕迹的 `window.Notification`，与
  `chat-fingerprint.js`「做一个诚实的桌面浏览器」的目标直接冲突；
- 为了让那个探测通过就得给远程 webview 授 `notification:*` 权限。

壳只需要原生发送、点击回执与历史通知清理，不向官网页面授予系统通知能力。当前直接使用已在依赖图中的平台接口；通知导航客户端仅在本地工作台运行，不安装浏览器 `Notification` shim（兼容层）。

#### macOS 必须打包才能投递（平台约束）

系统通知在 macOS 上按**应用 bundle** 归属。`tauri dev` / `cargo run` 跑的是
`target/debug/dsh-xlink` 这个裸可执行文件，系统找不到它的 bundle，实测系统日志：

```text
usernoted:  Sending request for permission for com.apple.Terminal
            with path .../src-tauri/target/debug/dsh-xlink
NotificationCenter: Unable to find valid bundle with backupPath:
            .../src-tauri/target/debug/dsh-xlink
```

也就是说：通知要么被挂到父进程（Terminal）名下，要么被直接丢弃，旧通知后端的投递函数仍可能返回成功——这正是"面板说发送成功、屏幕上什么都没有"
的原因。当前实现还将通知中心返回空值作为不可用处理，避免启动时崩溃，并如实告知：`NotificationStatus.environmentNote`
在检测到进程不在 `.app/Contents/MacOS/` 内时给出说明（面板以灰字展示，不是错误），
并把验证路径写清楚（`npm run build -- --debug` 产出的 `.app`，或安装版）。
角标完全不受影响：`NSApp.dockTile.setBadgeLabel` 直接作用在 Dock 图标上，与
bundle 无关。

## 5. 状态模型

```rust
struct Center {
    unread: u32,                        // 角标数字（两个工作台合计）
    items: VecDeque<CompletedTask>,     // 最近 ≤8 条完成记录，新的在前（带来源）
    running: HashMap<SessionId, Turn>,  // running:true 时记下开始时刻
    subagents: HashSet<SessionId>,      // 不打扰的子代理会话
    titles: HashMap<SessionId, String>, // 标题 / cwd 缓存
    last_turns: HashMap<SessionId, LastTurn>, // 最近一轮对话（prompt / response 预览）
    watching: bool,                     // DSH 的 $events 流是否就绪
    watching_mcode: bool,               // MiniMax 事件流是否就绪（两条各报各的）
    mcode_error: Option<String>,        // MiniMax 事件流最近一次失败说明
    last_error: Option<String>,         // 最近一次失败的可操作说明
}
```

`CompletedTask` 是通知正文与设置页「最近完成列表」共用的记录：

```rust
struct CompletedTask {
    kernel: TaskKernel,     // 来源工作台：dsh | mcode（序列化小写）
    session_id: String,
    title: String,
    cwd: String,
    finished_at_ms: u64,   // 完成时刻（unix 毫秒）；设置页列表用它渲染完成时间，通知正文不用
    duration_ms: u64,      // 运行时长；没观察到开始时刻时为 0
    last_prompt: String,   // 最近一轮的用户提问；拿不到为空
    last_response: String, // 最近一轮的助手回复预览（内核已截断）；拿不到为空
}
```

`last_prompt` / `last_response` 来自 `Center::last_turns`，在任务完成那一刻拷进
`CompletedTask`——之后即使连接断开或该会话被清出，通知里已经带着的内容不会丢。
两者都拿不到时通知正文省略 💬 那一行，只留「标题 + 尾行度量」。

- **用时未知**：只在连接期间观察到开始时刻时才有 `duration_ms`，否则为 0，
  尾行整行省略（不写"用时 0 秒"）。
- **去重**：重连后若同一条完成事件重放，DSH 按 `(sessionId, 完成时刻)`、MiniMax 按 `turnId` 判重。
- **上限**：`items` 最多 8 条（两个内核共用同一份）；`unread` 只增不减，直到对应来源被聚焦或点「全部已读」。
- **降级**：总开关关闭时角标恒为空（`sync_badge` 直接按 0 处理），但历史记录
  仍保留，重新打开开关不会丢上下文。

## 6. 失败模式与用户可见行为

| 失败 | 用户看到 | 壳的行为 |
| --- | --- | --- |
| 内核没启动 | 面板提示"尚未连接内核事件流" | 订阅线程按退避重试；日志里有一条含操作的说明 |
| 内核重启（新 token） | 无感 | 下一次重连重新取 token + cookie |
| WebSocket 断开 | 面板 `watching=false` + 最近错误 | 1→20 s 退避重连；通知在该内核进程内暂时缺失 |
| 内核版本改了事件名/协议 | 无通知，面板显示 `lastError` | 只记录、不重试风暴（退避上限 20 s），不影响其它功能 |
| 系统通知被拒/不可用 | `lastError` 给出具体平台指引 | 角标仍然工作（角标不依赖通知权限） |
| **Windows 通知总开关被关** | 设置页灰字（**只此一处**，见 §4.4） | `ToastNotifier::Show` 仍返回 S_OK，只能靠 `Setting()` 主动发现；**角标不受影响** |
| 跑的是未打包构建（`tauri dev` / `cargo run`） | 面板灰字提示"系统通知不会以本应用名义投递" | 照常尝试投递；**角标不受影响**。原因见 §4.3 |
| 未读积压 | 角标显示 `999+` | 焦点回到那个工作台只清该来源，点「全部已读」清全部 |
| 内核没有 `session/control`（老版本 / 协议漂移） | 无感 | 记一行日志，退回一次 `session/list` 标题快照（§3.3） |
| 标题还没生成就完成了（首轮极短） | 通知写「未命名会话 <短 id>」 | 标题投影一到就把这条记录与面板里的名字改成真标题（`set_title`） |
| MiniMax 工作台没启动 | 设置页不显示 MiniMax 未连接提示（只在体验入口开 + 工作台运行中才提示） | 订阅线程每秒轮询一次状态、不连端口；工作台起来后自动接上 |
| MiniMax 事件流断开 / 连不上 | 设置页灰字「未连接 MiniMax 事件流」，有 `mcodeError` 时带原因；DSH 通知不受影响 | 1→20 s 退避重连；失败写壳日志 `notification-mcode`（60 s 节流） |
| MiniMax WebUI 协议变了 | 同上一行的可见说明 | 只报错、不重试风暴、不影响工作台；上游私有协议，壳只承诺 0.1.7 |
| MiniMax 任务被中止 / 报错 | 不提醒 | 清运行标记，不算完成 |

## 7. 配置

`settings.json` 新增三个字段，全部是 `Option<bool>`（`None` = 用户没设置过，
走默认值）——这样面板的「保存设置」（只提交端口与 profile）不会把通知开关
静默重置，与 `node_path` 等字段的处理一致。

| 字段 | 默认 | 含义 |
| --- | --- | --- |
| `notify_enabled` | `true` | 任务完成通知总开关 |
| `notify_away_only` | `true` | 仅当工作台窗口不在前台时才弹气泡 |
| `notify_sound` | `false` | 通知是否带提示音 |

三个字段由两个工作台共用，不按内核分开（理由见 §2.1）。保存只写这三个字段，其余设置原样保留。

面板命令：`notification_status` / `notification_mark_read` /
`notification_save_settings` / `notification_test` / `notification_test_sound`，
全部登记在 `permissions/app-commands.json` 的 `allow-local-commands` 里（漏登记
会让按钮在面板里直接报 "not allowed"，`scripts/check-invariants.mjs` 会在 CI
拦住这种情况）。

其中 `notification_test` 的语义是**模拟一次任务完成**（未读 +1 → 刷新角标 → 发一条
系统通知），而不是"只发一条通知"：通知气泡能否出现由系统决定，若自检只发通知，用户
在没有通知权限的环境里会误判成"整个功能没做"。把角标一起走一遍，点一次就能看到
Dock / 任务栏上的数字，从而把「功能没做」和「系统不放行通知」区分开。它刻意无视
`notify_away_only`（用户主动点的按钮必须看得到效果），总开关关闭时只回一条可操作
说明、不做任何事。面板**不在成功路径上弹页内浮层**——那会被误认成"通知就是它"。

这个按钮**只在 dev 构建（`StatusView.dev_build`，即 `cfg!(debug_assertions)`）、且没有
开启 release 预览（dev 调试面板里的「模拟正式版外观」）时渲染**：它会凭空给用户造一条
未读，正式版没有这个需要，用户要验证"能不能听到提示音"用旁边的「试听」即可。命令本身
仍留在 ACL 白名单里，dev 构建与排障随时可用。

## 8. 后续可选增强

按收益排序，均已留好接口，不需要改动现有判定逻辑：

点击通知定位与锁屏隐私已接线，当前待办是两平台安装版验收，见 §4.5、§4.6。

2. **退出后仍然记录**：当前内核随壳退出而停止，不存在"壳不在时完成的任务"，
   所以未读状态不需要落盘。若将来支持内核常驻，`Center` 可以整体序列化到
   `<data_dir>/notifications.json`。
3. **Windows 托盘图标角标**：面板收起进通知区域时任务栏按钮不存在，角标无处
   可画。可在 `tray.rs` 里用同一套 `render_badge` 输出合成到托盘图标上（需要
   一层 RGBA 合成，约 30 行、无新依赖）。
4. **按会话订阅**：`session/follow` 目前只用到全局 `$events`；若将来需要"某个
   会话开始跑/跑完"的更细粒度信号，可在同一条 WebSocket 上再开一条逻辑流。
5. **通知观测的替代接入**：如后续确有必要，只评估本项目内置插件与已公开的扩展点。旧稿建议通过 `patches.rs` 修改内核的路线已废止；已安装内核保持只读，不恢复补丁机制。

## 9. 验证方法

### 9.1 自动化

```sh
# 纯逻辑：DSH 状态机（running 记账、子代理过滤、重放去重、时长）、MiniMax 事件机
# （watchEvents 请求形状、session.finish 判定、turnId 去重、abort/error 不算完成、子代理不打扰）、
# 来源标签与序列化、角标文案、Windows 位图
cargo test --lib notify

# 真实内核上的集成测试（默认 #[ignore]，需要工作台正在运行）：
#   · launch token → dsh-auth cookie
#   · WebSocket 握手（自定义 Cookie / Origin 头）与 101
#   · $events 的 open 帧字段名 + 就绪帧里的 clientId
#   · session/list 标题快照（老内核的兜底路径）
#   · session/control 的 baseline → 标题表（回归：通知里的会话名）
DSH_DESKTOP_DATA_DIR=~/.dsh-xlink/dsh/desktop DSH_XLINK_LIVE_PORT=<端口> \
  cargo test --lib -- --ignored live_ --nocapture

# 前端：设置页的通知动作、规范化与 loading key、来源 chip 与两条未连接提示
# （notifyKernels.test.js）
npm run test:ui
```

Windows 位图那组断言只在 Windows 上编译（`#[cfg(all(test, target_os = "windows"))]`），
CI 的 Windows job 会跑到；本机是 macOS 时用
`cargo test --target x86_64-pc-windows-msvc --lib` 无法执行，属已知限制。

### 9.2 手工端到端（发一次真通知）

通知气泡最终由系统投递，只能在真机上确认。**先分清两件事**：角标（Dock /
任务栏）与构建是否打包无关；系统通知气泡在 macOS 上要求进程位于 `.app` bundle
内，因此 `tauri dev` 的裸二进制看不到气泡（见 §4.3）。两级验证：

**A. 先看角标（dev 壳就够）**

1. `npm run dev` 起 dev 壳（端口默认 3091，与已安装的 release 壳互不干扰）；
2. 「设置 → 任务通知 → 模拟一次任务完成」（这个按钮只在 dev 构建里显示）；
3. 期望：Dock 图标右上角立刻出现红色数字角标（1），卡片里出现「1 条未读」；
4. 点「全部已读」→ 角标消失；
5. 「设置 → 任务通知 → 通知声音 → 试听」→ 期望立刻听到系统提醒声音
   （不需要打包，也不需要通知权限）。

**B. 再看系统通知气泡（需要打包后的 app）**

1. `npm run build -- --debug`（产出一个 debug 版 `.app`，不签名也能投递本地通知）；
2. 运行 `src-tauri/target/debug/bundle/macos/dsh-xlink.app`（或直接用安装版 DMG）；
3. 「模拟一次任务完成」→ 期望系统通知中心弹出标题「任务已完成」、正文首行
   「📋 DSH · 测试通知 ✅」；
   首次可能需要到「系统设置 → 通知」里允许 dsh-xlink（debug 版 `.app` 的
   `dev_build` 仍为 true，所以这个按钮在 debug 包里也在；正式安装包里没有它，
   验证气泡请用真实任务）；
4. 真实链路：启动工作台 → 发一条会跑一两分钟的任务 → **切到别的应用** → 任务跑完后
   角标 +1 且弹出系统通知 → 切回工作台 → 角标清零。

**Windows 上先确认总开关**：设置 → 系统 → 通知里的通知开关**关着时，气泡永远不出现**，
而壳看不到任何错误（`Show` 返回 S_OK，§4.4）。dev 壳里「模拟一次任务完成」点了不会有
横幅，按钮上方/下方的灰字环境说明会讲清原因并指到那个开关——**只有那一处**。
角标与气泡是两条独立的通路：角标出现只说明状态机在工作，证明不了通知通路可用。

**C. 点横幅回到工作台（Windows，安装版）**

1. 用安装版（`npm run build` 产物或已装的 exe）启动壳并启动工作台，切到别的应用；
2. 触发一次任务完成 → 出现系统通知横幅；
3. **点横幅** → 期望：工作台窗口被拉到台前，任务栏角标清零；
4. 再点一次托盘图标确认**没有**多出第二个 dsh-xlink 进程（任务管理器 → 详细信息），
   也没有第二个管理面板窗口——这正是交接要防的那件事。

dev 壳上要验同一条链路（`npm run dev` 本身不启用单实例守卫，见 §4.5）：

```sh
# 第一个实例：显式打开守卫，拿到互斥体与管道
DSH_XLINK_SINGLE_INSTANCE=1 npm run dev
# 另开一个终端再起一次同一个 exe —— 它应当秒退，不建任何窗口
DSH_XLINK_SINGLE_INSTANCE=1 npm run dev
```

第二个进程秒退即证明交接生效（`claim_or_handoff` 排在 `tauri::Builder` 之前，
它退出的那一刻还没有任何窗口被创建）。

**D. macOS 上点横幅**（打包后的 `.app`，因为横幅本身要求 bundle）

1. 启动工作台 → 让它跑一个任务 → 切到别的应用；
2. 任务跑完后点通知横幅 → 期望：应用被带到前台，**已开着的工作台窗口一并
   抬到台前**（角标清零）；
3. 再点一次 Dock 图标 → 工作台没开过时**不应**凭空多出一个工作台窗口，但
   **管理面板应被叫回前台**（`Reopen` 走的是 `raise_workbench_if_open`，
   抬不到工作台时兜底 `show_main_shell`，见 §4.5）；
4. 登录自启拉起后（不开「开机启动工作台」）→ Dock 上**不应**有 dsh-xlink
   的图标（收起走 `hide_to_shell`，激活等级已降为 Accessory），菜单栏图标
   是唯一常驻入口。

> 这一条在 2026-10-13 做过一次真实的横幅点击，结论见 §4.5 的「验证状态」。
> 复验时留意：**工作台窗口必须已经开着**，否则第 2 步点横幅会走
> `focus_workbench` 新建窗口，页面要加载几秒——那不是回归，只是第一次打开。

**E. MiniMax 工作台的任务通知（2026-11-08 新增，待实机验收）**

1. dev 壳里启动 MiniMax 工作台（先在设置 → 工作台打开「MiniMax 内核体验」）；
2. 设置 → 任务通知确认总开关打开；等设置页不再显示「未连接 MiniMax 事件流」；
3. 在 MiniMax 工作台发一个会跑一会儿的任务，**切到别的应用**；
4. 期望：该任务结束后弹一条首行为「📋 MiniMax · 会话标题 ✅」的系统通知（MiniMax
   事件流不带会话正文，所以没有 💬 行），角标 +1；设置页「最近完成」列表该行带
   MiniMax 来源 chip；
5. 切回 MiniMax 工作台 → 只清 MiniMax 来源的未读，DSH 来源的未读不动；
6. 点这条横幅 → 期望 MiniMax 工作台被抬到前台；工作台窗口没开时应叫回管理面板。
7. 反向验证：停掉 MiniMax 工作台后设置页显示的是「未连接 DSH 事件流」与否按各自实际
   状态，不互相冒充。

> 这一组**尚未在真实工作台上验收**。2026-11-08 只在本机 WebUI 0.1.7 临时实例核对了
> 事件帧形状（§10），协议形状通过不等于端到端通知可用。

## 10. 历史核验事实（旧版复核用）

以下结论都是在本机运行的内核上实测/读源码得到的，改动本设计前建议重新验证：

```sh
# 1) launch token → cookie
curl -s -c /tmp/c.txt -o /dev/null -w '%{http_code}\n' \
  "$(rg -o 'http://127\.0\.0\.1:[0-9]+/\?token=[A-Za-z0-9_-]+' \
      ~/.dsh-xlink/shell/release/logs/*kernel*.log | tail -1)"      # → 303

# 2) $events 流（需要内核在跑）：用 Rust 集成测试代替手写脚本
#    DSH_XLINK_LIVE_PORT=<端口> cargo test --lib -- --ignored live_ --nocapture

# 3) session/list 的真实成本（不要用它做轮询）
curl -s -b /tmp/c.txt -H 'content-type: application/json' \
  -d '{"type":"client-request","rpcId":"p","method":"session/list","payload":{"args":{"_request":{}}}}' \
  -o /dev/null -w '%{size_download} bytes / %{time_total}s\n' \
  http://127.0.0.1:<port>/api/session/list            # → ≈2.1 MB / 0.47 s
```

| 结论 | 出处 |
| --- | --- |
| `api-session/status` = `(agent.id, status === "running")` | `@deepseek-ai/dsh-api-session-controller/lib/index.js`（`ctx.on("agent/status", …)`） |
| 事件经 `/api/remote.mux` 的 `$events` 流广播，`emit` 帧无需客户端回执 | `@deepseek-ai/dsh-api-gateway/lib/index.js`（`broadcastRemoteEvent`）、`lib/types/stream-protocol.js` |
| 服务端 30 s 一次 Ping，两次未 Pong 即断开 | `dsh-api-gateway/lib/types/stream-server.js` |
| 事件里的 `sessionId` 是裸 uuid，`session/list` 里带 `session-` 前缀 | 实测（`api-session/added` 的 `parentSessionId` 反而是带前缀的形式） |
| macOS 角标 = `NSDockTile.setBadgeLabel` | `tao/src/platform_impl/macos/badge.rs` |
| Windows 覆盖图标 = `ITaskbarList3::SetOverlayIcon` | `tao/src/platform_impl/windows/window.rs` |
| Windows toast 必须带 AppUserModelID（`Toast::new(app_id)`），`notify-rust` 不传时用 `Microsoft.Windows.PowerShell` 兜底 | `tauri-winrt-notification`（`Toast::new` / `Toast::POWERSHELL_APP_ID`）、`notify-rust` 的 `src/windows.rs` |
| **通知总开关关闭时 `ToastNotifier::Show` 返回 S_OK 而不显示**；`Setting()` 对任意 AUMID 都是 `DisabledForUser` | 实测（本机，2026-09-28）：`CreateToastNotifierWithId(...).Show()` → `Ok(())` + 通知中心无记录；`Setting()` → `DisabledForUser`（本应用 / PowerShell / Defender / NVIDIA 一致）；`HKCU\…\PushNotifications\ToastEnabled` = 0；`CreateToastNotifier()`（不带 AUMID）→ `0x80070490` |
| 插件式通知会在每个 webview 注入 shim 并调用 `is_permission_granted` | `tauri-plugin-notification` 的 `src/lib.rs`（`.js_init_script`）与 `src/init-iife.js` |
| macOS 上未打包进程的通知被丢弃 / 归到父进程名下 | 系统日志：`usernoted: Sending request for permission for com.apple.Terminal with path …/target/debug/dsh-xlink`、`NotificationCenter: Unable to find valid bundle with backupPath: …` |
| 系统默认提示音：macOS `UNNotificationSound.defaultSound`；Windows 默认声音，关闭时 `<audio silent="true" />` | `notify/platform.rs` 的原生载荷；[Apple 提示音接口](https://developer.apple.com/documentation/usernotifications/unnotificationsound) |
| 标题是投影（`key: "title"`），由 `session/title` 事件折叠而来，创建时为 `null` | `@deepseek-ai/dsh-session-title/lib/index.js` 的 `titleProjectionDefinition`（`init: () => null`） |
| `api-session/added` 在 `session/created` 时发出，因此创建那一刻标题还是空的 | `dsh-api-session-controller/lib/index.js`（`ctx.on("session/created", … ctx.emit("api-session/added", …))`） |
| 投影变化只在 `session/control` 上推送（`{type:"projection",sessionId,key,value,seq}`），`$events` 里没有标题事件 | `dsh-api-session-controller/lib/index.js` 的 `SessionControlController`（`ctx.sessionProjections.onChanged(…)`） |
| `session/control` 开流即给 `baseline`（`queues` / `jobs` / `projections`），投影里含每个常驻会话的 `values.title` | 实测（本机 4 个会话：baseline 215 KB，其中活跃会话的 `contextTimeline` 占大头；同机 `session/list` 为 663 KB / 205 个会话） |
| 非 `$events` 的 endpoint 都是 Remote 方法流（`{namespace}/{method}`），payload 必须恰好是 `{"args":{…}}` | `dsh-api-gateway/lib/index.js` 的 `openWireStream` / `remoteRequest` |
| 「试听提示音」两端走各自系统的提醒声接口 | `MacOSX.sdk/…/AudioToolbox.framework/Headers/AudioServices.h`（`kSystemSoundID_UserPreferredAlert = 0x00001000`、`AudioServicesPlayAlertSound`）；`windows-sys` 的 `Win32::System::Diagnostics::Debug::MessageBeep` + `Win32::UI::WindowsAndMessaging::MB_ICONASTERISK` |
| MiniMax WebUI 事件流握手：客户端发 `{protocolVersion:1, kind:"request", requestId, operation:"watchEvents", body:{}}`，服务端 ack 为 `{kind:"response", body:{stream:true}}`，之后推 `{kind:"event", body:{type, payload}}` | 实测（本机 WebUI 0.1.7 临时实例，2026-11-08） |
| MiniMax `session.created` 的 payload 含 `sessionId` / `agentName` / `title`（可为 null） | 实测（同上） |
| MiniMax `session.finish` 由 `server/runtime.js` 的 `publishTerminal` 发布，含 `sessionId` / `agentName` / `turnId` / `status: "finished"` | 本机 WebUI 0.1.7 源码 |
| MiniMax 这条是上游私有协议，没有 DSH 的 `remote.mux` 与会话深链；壳只承诺 WebUI 0.1.7 | `notify/mcode_watch.rs` 模块文档；`kernel/mcode_service.rs` 的安装与启动版本限制 |

## 11. 当前验证边界（2026-11-05；2026-11-08 补充两个内核的通知）

Rust 自动化覆盖通知状态机、A/B目标选择、过期回执及 macOS UserNotifications 原生对象脱敏、请求标识和默认动作筛选；客户端夹具覆盖列表未加载、重复/乱序通知和已删除会话。Windows通知与锁屏查询的实际生产源码已在独立类型检查夹具中通过 `x86_64-pc-windows-msvc` 检查，未替代完整产品构建。

本轮未执行真实锁屏、通知中心点击、睡眠唤醒及双平台安装版验收。旧版实测记录仅用于解释已有行为，不作为新实现通过的证据。

2026-11-08 补充（两个内核的任务通知）：MiniMax WebUI 0.1.7 的事件流形状已在本机临时实例上验证
（ack 帧、`session.created` 载荷、`session.finish` 的 `publishTerminal` 发布形状，见 §10），
Rust 自动化覆盖 `mcode_watch` 的事件状态机与 `TaskKernel` 的标签 / 序列化。**仍未验证**：真实
MiniMax 工作台跑完一轮任务后的端到端系统通知、工作台崩溃 / 重连后的续订、点击 MiniMax 通知把
工作台抬到前台，以及 macOS / Windows 安装版。上游私有事件协议变化时不承诺兼容，失败会在设置页
以「未连接 MiniMax 事件流」与具体原因显示，不影响 DSH 通知与工作台本身。

隔离壳启动补证：裸开发二进制的 `defaultUserNotificationCenter` 实际可返回空值，当前通过可空返回值接收，通知不可用时记录说明并继续启动，避免初始化时崩溃。修复后在新运行根重建并启动，6/6个界面测试文件通过；它证明启动与后端桥可用，不证明安装版通知已展示或点击成功。
