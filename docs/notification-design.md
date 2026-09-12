# 任务完成通知系统设计（macOS / Windows）

本文说明 dsh-xlink 的「内核跑完对话任务 → 系统角标 + 通知气泡」是怎么工作的、
为什么这样设计，以及每一处取舍的代价。实现见 `src-tauri/src/notify.rs`。

## 1. 目标与范围

| 项 | 取值 |
| --- | --- |
| 触发事件 | **内核里某个会话的对话任务跑完**（一轮 turn 结束、agent 回到 idle） |
| macOS 提示 | Dock 图标右上角的系统数字角标 + 通知中心气泡 |
| Windows 提示 | 任务栏按钮右上角的数字角标（覆盖图标）+ 通知区域气泡 |
| 角标数字 | **已完成但用户未读的任务数** |
| 不在范围内 | 官方对话窗口（`chat.deepseek.com` 等外部站点，不经过内核）、内核自身的更新检查、插件/技能市场动作 |

「未读」的判定必须能被用户预期，所以规则只有两条：

1. 任务完成的那一刻**工作台窗口在前台**（用户正看着它）→ 不算未读；默认也不弹气泡（不打扰）。
2. 任务完成时工作台不在前台、或窗口根本没开 → 未读 +1、角标刷新、弹一条气泡。

未读清零的路径同样只有两条：**工作台窗口重新获得焦点**（用户回来看结果了），
或管理面板里的「全部已读」。

> 为什么不按消息流精确到「读到哪一条」：内核的未读状态属于内核，壳只能观察
> 到「任务结束」这一事件；把"焦点回到工作台"作为已读信号，语义清楚、行为可
> 预测，也不会让角标长期挂着不动。

## 2. 结构

```text
        ┌────────────────────────── 内核进程（dsh web, 127.0.0.1:<port>）──────────────────────────┐
        │  agent/status ──► api-session/status(sessionId, running)                                  │
        │  session/created ──► api-session/added(summary)         …经 /api/remote.mux 广播        │
        └───────────────────────────────────────────┬──────────────────────────────────────────────┘
                                                    │ WebSocket（与工作台 webview 同一条通道）
                                     ┌──────────────▼───────────────┐
                                     │ notify.rs 订阅线程            │  认证 cookie / 重连退避
                                     │  · running: true → false 判定 │
                                     │  · 子代理会话过滤             │
                                     └──────────────┬───────────────┘
                                                    │
                        ┌───────────────────────────▼────────────────────────────┐
                        │ 通知中心（全局状态）                                    │
                        │  unread / 最近完成记录（≤8） / running / 标题表 / 错误  │
                        └───────┬───────────────────────────────┬────────────────┘
                                │                               │
              ┌─────────────────▼──────────────┐   ┌────────────▼─────────────────────┐
              │ 系统角标                        │   │ 通知气泡                          │
              │ macOS: NSDockTile.setBadgeLabel │   │ notify-rust（Rust 侧直接调用）    │
              │ Windows: SetOverlayIcon(位图)   │   │ (macOS NSUserNotification /      │
              │                                 │   │  Windows WinRT toast)            │
              └─────────────────────────────────┘   └──────────────────────────────────┘
                                │
              ┌─────────────────▼──────────────────────────────────────────────────┐
              │ 管理面板「任务通知」卡片：开关 / 未读数 / 全部已读 / 测试通知         │
              │ 事件 `notification-status` 推送最新快照                              │
              └────────────────────────────────────────────────────────────────────┘
```

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
| `api-session/added` | 新会话摘要（含标题、cwd、`parentSessionId`）→ 免费拿到通知文案里的标题 |
| `api-session/activity` | 用户消息写入会话 → 只用作旁证，不参与判定 |
| `api-session/error` | 会话级错误 → 预留（当前不弹，避免与事故面板重复打扰） |

> **id 形态**：事件里的 `sessionId` 是不带前缀的裸 uuid，而 `session/list`
> 里是 `session-<uuid>`。壳统一去掉 `session-` 前缀后再建索引
> （`notify::normalize_session_id`）。

### 3.3 子代理会话过滤

子代理（subagent）各自是一个会话，跑完同样会发 `api-session/status(false)`，
但它们不该打扰用户。事件里没有父会话字段，因此壳依赖 `api-session/added`：
摘要里带 `parentSessionId`（或 `origin == "subagent"`）的会话被登记进
`subagents` 集合，之后其状态事件只记账、不通知。

已知边界：如果子代理在壳**连接之前**就已创建，且此后从未产生新的 `added`
事件，它的完成会被当成普通任务。代价是偶发的一条多余通知，换取的是实现简单
（不需要为每次通知去查一次 2 MB 的会话列表）。

### 3.4 标题的来源与代价

通知文案要写"「会话标题」已完成"，而事件本身不带标题。壳的做法是：

1. 连接期间新建的会话 → 直接用 `api-session/added` 摘要里的
   `projections.values.title`，零成本；
2. 连接之前就存在的会话 → 每个内核进程**最多一次** `session/list` 快照
   （`titles_seeded` 门控），把标题与 cwd 缓存成表。这一次 0.5 s 的代价发生
   在内核刚启动时，与内核启动本身的耗时相比可以忽略；
3. 仍然查不到 → 文案回退成 `未命名会话 <id 前 8 位>`。

### 3.5 认证、重连与退避

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

壳**直接依赖 `notify-rust`**（不注册 `tauri-plugin-notification`），两端各走系统
原生通道：

| 平台 | 后端 | 注意 |
| --- | --- | --- |
| macOS | `mac-notification-sys` → `NSUserNotificationCenter` | 不需要授权弹窗，也不需要 App 已被签名/打包；未打包的 dev 二进制可能发不出去，错误会被面板显示出来 |
| Windows | WinRT toast（`tauri-winrt-notification`） | 必须带 AppUserModelID（`app.config().identifier`）；**只有安装版**（NSIS 创建的开始菜单快捷方式提供该 ID）才显示应用名与图标，开发模式会以 PowerShell 身份出现，属系统层面限制 |

提示音：`sound = true` 时请求系统默认提示音（macOS 传
`NSUserNotificationDefaultSoundName`、Windows 传 `Default`），关闭时不设置任何
声音——两端不设置即为静音。

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

壳需要的只是 Rust 侧的发送能力，而 `notify-rust` 本来就是该插件的后端，所以直接
依赖它：两端行为一致、不需要任何 capability、不往任何页面注入脚本。
（`notify-rust` 只被壳自己的模块引用，JS 侧完全够不到这条通道。）

#### macOS 必须打包才能投递（平台约束）

系统通知在 macOS 上按**应用 bundle** 归属。`tauri dev` / `cargo run` 跑的是
`target/debug/dsh-xlink` 这个裸可执行文件，系统找不到它的 bundle，实测系统日志：

```text
usernoted:  Sending request for permission for com.apple.Terminal
            with path .../src-tauri/target/debug/dsh-xlink
NotificationCenter: Unable to find valid bundle with backupPath:
            .../src-tauri/target/debug/dsh-xlink
```

也就是说：通知要么被挂到父进程（Terminal）名下，要么被直接丢弃，**nsuser
notification 的投递函数仍然返回成功**——这正是"面板说发送成功、屏幕上什么都没有"
的原因。壳无法绕过这一点，于是选择如实告知：`NotificationStatus.environmentNote`
在检测到进程不在 `.app/Contents/MacOS/` 内时给出说明（面板以灰字展示，不是错误），
并把验证路径写清楚（`npm run build -- --debug` 产出的 `.app`，或安装版）。
角标完全不受影响：`NSApp.dockTile.setBadgeLabel` 直接作用在 Dock 图标上，与
bundle 无关。

## 5. 状态模型

```rust
struct Center {
    unread: u32,                        // 角标数字
    items: VecDeque<CompletedTask>,     // 最近 ≤8 条完成记录，新的在前
    running: HashMap<SessionId, Turn>,  // running:true 时记下开始时刻
    subagents: HashSet<SessionId>,      // 不打扰的子代理会话
    titles: HashMap<SessionId, String>, // 标题 / cwd 缓存
    watching: bool,                     // $events 流是否就绪
    last_error: Option<String>,         // 最近一次失败的可操作说明
}
```

- **时长**：`running:true` 到 `running:false` 之间的墙钟差；只在连接期间观察到
  开始时刻时才显示（否则文案省略"用时"）。
- **去重**：重连后若同一条完成事件重放，按 `(sessionId, 完成时刻)` 判重。
- **上限**：`items` 最多 8 条；`unread` 只增不减，直到被显式清零。
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
| 跑的是未打包构建（`tauri dev` / `cargo run`） | 面板灰字提示"系统通知不会以本应用名义投递" | 照常尝试投递；**角标不受影响**。原因见 §4.3 |
| 未读积压 | 角标显示 `999+` | 焦点回到工作台或点「全部已读」即清零 |

## 7. 配置

`settings.json` 新增三个字段，全部是 `Option<bool>`（`None` = 用户没设置过，
走默认值）——这样面板的「保存设置」（只提交端口与 profile）不会把通知开关
静默重置，与 `node_path` 等字段的处理一致。

| 字段 | 默认 | 含义 |
| --- | --- | --- |
| `notify_enabled` | `true` | 任务完成通知总开关 |
| `notify_away_only` | `true` | 仅当工作台窗口不在前台时才弹气泡 |
| `notify_sound` | `false` | 通知是否带提示音 |

面板命令：`notification_status` / `notification_mark_read` /
`notification_save_settings` / `notification_test`，全部登记在
`permissions/app-commands.json` 的 `allow-local-commands` 里（漏登记会让按钮
在面板里直接报 "not allowed"，`scripts/check-invariants.mjs` 会在 CI 拦住这种
情况）。

其中 `notification_test` 的语义是**模拟一次任务完成**（未读 +1 → 刷新角标 → 发一条
系统通知），而不是"只发一条通知"：通知气泡能否出现由系统决定，若自检只发通知，用户
在没有通知权限的环境里会误判成"整个功能没做"。把角标一起走一遍，点一次就能看到
Dock / 任务栏上的数字，从而把「功能没做」和「系统不放行通知」区分开。它刻意无视
`notify_away_only`（用户主动点的按钮必须看得到效果），总开关关闭时只回一条可操作
说明、不做任何事。面板**不在成功路径上弹页内浮层**——那会被误认成"通知就是它"。

## 8. 后续可选增强

按收益排序，均已留好接口，不需要改动现有判定逻辑：

1. **点击通知直接跳到对应会话**：需要内核把"聚焦某个会话"暴露成可调用入口
   （当前只能聚焦窗口）。届时在面板与通知里同步展示 `items` 的会话列表。
2. **退出后仍然记录**：当前内核随壳退出而停止，不存在"壳不在时完成的任务"，
   所以未读状态不需要落盘。若将来支持内核常驻，`Center` 可以整体序列化到
   `<data_dir>/notifications.json`。
3. **Windows 托盘图标角标**：面板收起进通知区域时任务栏按钮不存在，角标无处
   可画。可在 `tray.rs` 里用同一套 `render_badge` 输出合成到托盘图标上（需要
   一层 RGBA 合成，约 30 行、无新依赖）。
4. **按会话订阅**：`session/follow` 目前只用到全局 `$events`；若将来需要"某个
   会话开始跑/跑完"的更细粒度信号，可在同一条 WebSocket 上再开一条逻辑流。
5. **内核侧插件**：把判定搬进内核（`patches.rs` 的 `kind: "plugin"`），可以让
   通知完全不依赖客户端协议。代价是每个内核版本都要重新锚定 SHA-256 与事件
   名，而当前方案在协议漂移时只会"静默不通知"，不会影响内核本身。

## 9. 验证方法

### 9.1 自动化

```sh
# 纯逻辑：状态机（running 记账、子代理过滤、重放去重、时长）、角标文案、Windows 位图
cargo test --lib notify

# 真实内核上的集成测试（默认 #[ignore]，需要工作台正在运行）：
#   · launch token → dsh-auth cookie
#   · WebSocket 握手（自定义 Cookie / Origin 头）与 101
#   · $events 的 open 帧字段名 + 就绪帧里的 clientId
#   · session/list 标题快照
DSH_DESKTOP_DATA_DIR=~/.dsh/desktop DSH_XLINK_LIVE_PORT=<端口> \
  cargo test --lib -- --ignored live_ --nocapture

# 前端：设置页的通知动作、规范化与 loading key
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
2. 「设置 → 任务通知 → 模拟一次任务完成」；
3. 期望：Dock 图标右上角立刻出现红色数字角标（1），卡片里出现「1 条未读」；
4. 点「全部已读」→ 角标消失。

**B. 再看系统通知气泡（需要打包后的 app）**

1. `npm run build -- --debug`（产出一个 debug 版 `.app`，不签名也能投递本地通知）；
2. 运行 `src-tauri/target/debug/bundle/macos/dsh-xlink.app`（或直接用安装版 DMG）；
3. 「模拟一次任务完成」→ 期望系统通知中心弹出「任务已完成 · 「测试通知」已完成」；
   首次可能需要到「系统设置 → 通知」里允许 dsh-xlink；
4. 真实链路：启动工作台 → 发一条会跑一两分钟的任务 → **切到别的应用** → 任务跑完后
   角标 +1 且弹出系统通知 → 切回工作台 → 角标清零。

Windows 上未安装的构建会以 PowerShell 名义显示通知（系统要求开始菜单快捷方式提供
AppUserModelID），安装版（NSIS）不受影响。

## 10. 已核验的事实（复核用）

以下结论都是在本机运行的内核上实测/读源码得到的，改动本设计前建议重新验证：

```sh
# 1) launch token → cookie
curl -s -c /tmp/c.txt -o /dev/null -w '%{http_code}\n' \
  "$(rg -o 'http://127\.0\.0\.1:[0-9]+/\?token=[A-Za-z0-9_-]+' \
      ~/.dsh/desktop/logs/*kernel*.log | tail -1)"      # → 303

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
| Windows toast 必须带 AppUserModelID（未安装时退回 PowerShell 身份） | `tauri-winrt-notification`（`Toast::new(app_id)`），`notify-rust` 的 `app_id` 仅在 Windows 上存在 |
| 插件式通知会在每个 webview 注入 shim 并调用 `is_permission_granted` | `tauri-plugin-notification` 的 `src/lib.rs`（`.js_init_script`）与 `src/init-iife.js` |
| macOS 上未打包进程的通知被丢弃 / 归到父进程名下 | 系统日志：`usernoted: Sending request for permission for com.apple.Terminal with path …/target/debug/dsh-xlink`、`NotificationCenter: Unable to find valid bundle with backupPath: …` |
| `sound_name` 的"系统默认提示音"取值：macOS `NSUserNotificationDefaultSoundName`、Windows `Default` | `mac-notification-sys` 的 `Sound` 取值表、`tauri-winrt-notification` 的 `impl FromStr for Sound`（Windows 未设置声音 = `<audio silent="true" />`） |
