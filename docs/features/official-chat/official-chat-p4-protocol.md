# P4 协议：官网对话的「直接 @ 引用」（v0.4 草案）

> **状态**：**草案 v0.4**（PM 2026-10-21 13:2x 定）—— v0.1 把**已实测**的事实写死成条款；v0.2 补**三条本轮新实测的硬约束**（§2.5 登录态机制 · §5.3 外挂 CDP 不可行 · §8.5 取证落点改正）；v0.3 是**依据订正**（§8.5 落点依据整段换掉 ＋ B 路不需要 DevTools ＋ 取证脚本两档版）；**v0.4 是「防误读条款」** —— 补 **「顶层 `code=0` ≠ 取到数据」**（§4.2）＋ **站内码有两层**（§6.2）＋ **端点来源标注**与**两种非静态口径都补不齐字段面**（§4.1）。
> **版本线顺延（写明，免得后来的人对不上）**：**v0.1 → v0.2 ＝ 补三条硬约束** → **v0.3 ＝ 依据订正＋脚本两档版** → **v0.4 ＝ 防误读条款**（**§4.3 的待写死点仍未填**）⇒ **v0.5 ＝ 补齐字段面后的冻结稿**（**原定 v0.3 为冻结稿，先后因「改依据」与「补防误读条款」两次顺延**）。
> **证据分级**（本稿每条事实都带标记，**不混写**）：**【已实测】** ＝ 本机实测复现过；**【推定】** ＝ 有依据但**未单独隔离**；**【待实测】** ＝ 未验 ⇒ **不写成条款**，只登记为待写死点。
> **一句话**：让用户在官网对话里选一段正文，**直接 @ 引用进壳里的对话**；正文由**官网会话目录**读出（不是手工粘贴、不是测试夹具）。
> **不做什么**：不做 macOS（**已知缺口**，另排期）· 不做消息级合并（P2 议题）· **不持久化、不外传站点凭据** · 不改上游内核。

---

## 0. 范围与前提

### 0.1 首站与平台

| 项 | 值 | 证据 |
| --- | --- | --- |
| 首站 | **DeepSeek 官网对话**（`chat.deepseek.com`） | izzy 2026-10-21 12:5x 拍板 |
| 平台 | **Windows 先上**；**macOS 单独排期** | 同上 |
| 站点 URL 常量 | `src-tauri/src/harness/official_chat.rs:26` / `:75` / `:82` / `:88` / `:103` | 【已实测】（PM 一手核） |
| 现状 | 插件的 `captureSource` **仍是固定夹具**（`plugins/official-chat/host/commands.js:160-205`，注释明写「真实站点取数与登录态属 P4」）⇒ **真实取数路径尚未实现** | 【已实测】 |

### 0.2 登录态归谁

- **登录态由 izzy 建立**（用 dev 模式的测试模式）；**登录凭据的建立与使用都不由 PM 代劳** ⇒ 协议只规定**怎么在已登录的环境里读**，不规定**怎么登录**。
- **登录态有时效**：`ds_session_id` 是 **session cookie**（`is_persistent = 0`）⇒ **那个 webview 环境一关，登录态就没了** ⇒ **每次取证前必须先确认那个环境还活着**（判法见 §8.3）。
- **生产路径天然带登录态**：`src-tauri/src/official_chat/context.rs:13-19`「**必须复用同一份 user-data 目录**……换了目录就拿不到登录态」＋ `:109` `.data_directory(profile_dir)` ⇒ 壳里的官网对话窗口用的就是用户自己的那份浏览器档案 ⇒ **登录态天然可用**（【已实测】：壳的官网对话窗口本来就能看到用户已登录的站点）。

### 0.3 本稿的前提（先说边界，避免后面的人误读）

- **本稿只写「怎么取」，不写「取到了怎么用」**：正文怎么进模型上下文、怎么落快照、怎么释放引用 ⇒ **P1 / P3 协议已写死，不重开**。
- **本稿不新增错误码**：P1 §3 的 **13 码闭集**不动（`src-tauri/src/official_chat/protocol.rs:88` `ERROR_CODES: [&str; 13]`）。
- **本稿不造新的监听服务**：计划 §4「不再造一个监听服务」⇒ §5 走**现役**的内置插件回环桥。

---

## 1. 取数路线（三条，写死）

> **裁定 1**：**「页内取数」与「调站点 API」不是二选一** —— 真解是**页内同源 fetch 站点自己的 API**。
> ⇒ 下列三条**必须**同时成立，缺一条实现就会走错：

1. **执行体在壳** —— 页内求值由**壳**发起（CDP `Runtime.evaluate`）。
2. **API 调用也在页内** —— 站点 API 由**页内的同源 fetch**发起，**凭据由浏览器自动带**（HttpOnly cookie）⇒ ⇒ **页外直接调该 API 必失败**（§3.3）。
3. **回程只回结构** —— 宿主只拿到**结构**（键名 / 条数 / 布尔量），**不回正文、不回凭据**（§2.4）。

### 1.1 为什么必须是壳内求值（技术必然性）

- DeepSeek 的目录端点是 **async fetch** ⇒ **同步**的 `script_eval::read_json`（`src-tauri/src/official_chat/script_eval.rs`）**取不到结果** ⇒ **必须 `awaitPromise`** ⇒ **首选 CDP `Runtime.evaluate`**（`src-tauri/src/official_chat/cdp_eval.rs:47`，`awaitPromise` ＋ `returnByValue`）——【已实测】（P0 的 `--mode cdp` **5/5**：未开 DevTools 窗口可调、直接拿到结算值、**页面 JS 空间零痕迹**：`getOwnPropertyNames(window)` 前后 **1239/1239**、`__dsh*` 为 0）。
- **R1（页内随机名槽位）已被 P0 明文否**（`official-chat-feasibility.md:198` / `:202`）⇒ **不得用槽位**（它有痕迹：名字前缀固定 `__dsh_rt_`，页面探针扫前缀即可定位）。

---

## 2. 页内求值（执行体在壳）

### 2.1 机制

- **用生产那份代码**：`src-tauri/src/official_chat/cdp_eval.rs`（`cdp_eval::cdp_eval`，`awaitPromise: true` ＋ `returnByValue: true`）⇒ **不另造求值通道**。
- **只读上下文的三条纪律**（`src-tauri/src/official_chat/context.rs`，照抄，协议不改写）：
  1. **同一份 user-data 目录**（`:13-19`；换了目录就拿不到登录态）⇒ `profile_dir` **只能**从统一取目录函数来，调用方**不得自造路径**；
  2. **不抢焦点**（`:107` `.visible(false)`，自建读取 webview 放在不可见窗口里）；
  3. **按需创建、不预热**（官网窗口没开时才自建读取 webview）。

### 2.2 壳侧接线的前提

- `official_chat/mod.rs:15-17` 自述：「**回程机制已实现并实测（G2），但还没有生产调用方**」⇒ ⇒ **本稿要做的第一件事是给它一个生产调用方**（§5），**不是**重写回程。

### 2.3 页内脚本的形状

- 脚本**只做一件事**：在页面里 `fetch` 站点自己的目录接口（§3），然后把**结构**序列化回宿主（§2.4）。
- **不许**在页面里留任何全局变量 / 槽位 / 定时器 ⇒ **零痕迹**（P0 实测口径：`getOwnPropertyNames(window)` 前后不变）。

### 2.4 回程只回结构（白名单）

**允许回传**：顶层键名、`code` / `msg`（**原样字符串，不解释语义**，见 §6.2）、`data` 的键名数组、数组的**长度**、数组首元素的**键名**数组、条目的**类型/布尔量**（例如「是否有更多页」）。

**不许回传**：**正文**、**标题**、**会话 id**、**任何 token / cookie / 凭据**、**站点的完整响应体**。

> **为什么把「条目的键名」也限制为首元素**：键名是**契约面**（实现要按它取值），而**值**是**内容** ⇒ 键名可回、值不可回。

### 2.5 ⭐ 登录态机制 ——「执行体在壳」「API 在页内」是**硬约束**，不是偏好

> **裁定 2（本轮新增，已实测机制性结论）** —— **登录态只存在于「同一个活着的浏览器上下文」里**。

**观测与机理**（【已实测】）：

| 事实 | 值 |
| --- | --- |
| DeepSeek 的登录凭据 | **`ds_session_id`** —— **session cookie**（盘上 `is_persistent = 0`、`expires_utc = 0`） |
| 拷贝后新浏览器进程 | **不恢复非持久 cookie**（启动时按过期处理并清除）⇒ **拷过来也不带登录态** |
| 拷贝副本上的实测 | 整份 `EBWebView` 拷走（1053 文件 / 131.4 MB / 1.0s）⇒ 页面落在 `https://chat.deepseek.com/sign_in`，读数 `pageLooksLikeAuth: true`、`page_ready` 1078ms |
| 拷贝副本上的 `localStorage` | **是拷过来的**：`userToken`(30) / `settingsJwt`(30) / `__appKit_userInfo`(37) 都在 ⇒ **明文 JWT 在盘上、且尚未过期** |
| 但页外用该 JWT 打 API | **四种组合全是 `40003 INVALID_TOKEN`**（§3.2）⇒ ⇒ **站点认的是那个 session cookie，不是盘上那个 JWT** |

⇒ **两条独立事实汇合**：① **盘上的 JWT 有效（`exp` 2026-11-30、`isAnonymous = false`）但不被认**；② **session cookie 跨进程不可恢复** ⇒ ⇒ **拷贝必然不带登录态**（**机制性结论，不是偶发**）。

⇒ **因此（写死）**：

1. **登录态只能在「同一 exe ＋ 同一 profile 目录 ＋ 那个 webview 还活着」的上下文里用** ⇒ ⇒ **取数必须发生在壳自己那个官网对话窗口里**（§2.1 的三条纪律正是为此）；
2. **「执行体在壳」与「API 调用在页内」由登录态机制决定**，**不是实现偏好** ⇒ **实现方不得提出「页外调 API」「从外部进程读」这类方案**（页外已实测必失败，§3.2）；
3. **不得把「登录态已建」当成可长期复用的前提** —— 它随**那个 webview 环境**存活（§0.2）。

---

## 3. 页内同源 fetch（API 调用在页内）

### 3.1 凭据机制（写死）

| 项 | 事实 | 证据 |
| --- | --- | --- |
| 登录 cookie | **HttpOnly** ⇒ 页内 `document.cookie` **看不到**（只看到 `.thumbcache…` / `smidV2` 这类匿名项） | 【已实测】 |
| 页内取数 | 同源 fetch **由浏览器自动带** HttpOnly cookie ⇒ **页内不需要、也不应该**自己拼凭据 | 【已实测】（探针实跑 `status=200`、`code=0`） |
| 站点 JS 的鉴权 | 走 `Authorization: Bearer <token>`（`getUserTokenWithSource`）＋ `x-device-id` / `x-ds-compression` / `x-request-encryption-key` | 【已实测】（站点 JS 的请求拦截器） |
| `x-device-id` 等头是否必需 | —— | **【推定】未单独隔离** ⇒ 本稿**不要求**页内 fetch 手工补这些头（页内 fetch 由站点自己的 XHR 逻辑决定；我们要的是**同源带 cookie**） |
| 盘上明文 | `localStorage` 键名 `userToken` / `settingsJwt` / `__appKit_userInfo` / `__appKit_@deepseek/*` / `deepseek-device-id:chat`；**JWT 明文躺在 leveldb**（`Default\Local Storage\leveldb\*.ldb`）⇒ 不必解密即可读 `exp` 与 `user` | 【已实测】 |

### 3.2 页外调 API 必失败（从推断升级为实测）

- 用**登录态**的 JWT（明文可读）打 `GET /api/v0/chat_session/fetch_page?count=5`，**四种组合全是 `40003 INVALID_TOKEN`**：`Authorization: Bearer <jwt>` / `Bearer` ＋ `Origin` ＋ `Referer` / `Bearer` ＋ 伪造 `Cookie: ds_session_id=<jwt>` / `x-auth-token: <jwt>`；裸 fetch 同码。⇒ 【已实测】。
- ⇒ **协议条款**：**凭据只在浏览器里用**。实现**不许**把站点凭据读出来、落盘、外传或塞进请求头 ⇒ 这条同时是**安全条款**（§7.2）。

### 3.3 ⚠️ 写协议时最容易犯的错（必须写进条款）

**未登录态与「带凭据但形态不对」回的是同一个码 `40003`** ⇒ **单看状态码分不清「没登录」与「凭据形态不对」** ⇒ ⇒ **站内码只「原样上报」，不解释语义**（§6.2）。

---

## 4. 站点适配契约（DeepSeek）

### 4.1 端点与参数（【已实测】，从站点自己缓存的完整请求行取得）

| 用途 | 请求行 |
| --- | --- |
| 会话目录**首屏** | `GET /api/v0/chat_session/fetch_page?count=5` |
| 会话目录**翻页** | `GET /api/v0/chat_session/fetch_page?lte_cursor&pinned=false` ⇒ **游标参数名 ＝ `lte_cursor`**（**不是 offset**）＋ `pinned` |
| 单会话正文 | `GET /api/v0/chat/history_messages?chat_session_id=<uuid>` |
| 当前用户 | `GET /api/v0/users/current` |
| 客户端设置 | `GET /api/v0/client/settings?did=<uuid>&scope=<banner｜main｜model｜provider｜web_upgrade>` |

- **端点路径与首屏/翻页参数名 ⇒ 本层可写死。**
- **页大小上限**：`count` 的最大值 —— **二手来源**称一次最多 100 条，**未在本机复核** ⇒ **不写成条款**。
- ⚠️ **静态包 grep 补不齐这一层**（【已实测】）：470 个 JS 缓存里静态 grep `/api/v0/` 只捞到 **10 个端点**，**`chat_session/*` 与 `history_messages` 不在其中**（前端是**运行时拼接**）⇒ ⇒ **端点面只能靠运行时实测**，静态分析不能替代。
- ⚠️ **上表那 5 个端点的来源要说准**（【已实测】）：来自**站点自己浏览器缓存里的完整请求行**（Chromium 缓存条目 `data_1` 的 key 字段）⇒ **不是**「页面当场请求」的记录 ⇒ ⇒ **登录态下的 resource timing 端点清单仍未实测**。
- ⚠️ **两种「非静态」口径也都补不齐字段面**（【已实测】，**未登录态**跑的）：**运行时 resource timing** 里资源总数 **23** 个、其中 `/api/` 端点**只有 1 个**（`/api/v0/client/settings`，query 只回**键名** `["did","scope"]`）⇒ ⇒ **两种口径都只能看到端点路径，**看不到**响应里的字段面 ⇒ ⇒ **字段面仍然只能靠登录态下的结构实测**（§4.3）。

### 4.2 响应骨架（【已实测】三层）

```
{ code, msg, data: { biz_code, biz_msg, biz_data } }
```

- 探针实跑读数：`status=200`、`code=0`、`msg=""`、`dataKeys=["biz_code","biz_msg","biz_data"]`、**`biz_code=2`**、**`biz_data` 的类型读数是 `null`**。
- ⚠️ **层级容易写错**：**会话目录的条目数组在 `data.biz_data` 里面**，**不是** `data.chat_sessions`。
- ⚠️⚠️ **「顶层 `code=0` ≠ 取到数据」（最容易误判的一条，写死）** —— 上面那组读数就是**反例**：`status=200` ＋ **顶层 `code=0`** ＋ `msg=""`（看着像成功）**同时** `biz_code=2` 且 **`biz_data` 是 `null`** ⇒ ⇒ **传输层成功 ≠ 业务层成功 ≠ 拿到了数据** ⇒ ⇒ **判「成功」不能只看顶层 `code`** ⇒ **成功判据必须落到 `data.biz_data` 是否有内容**（**具体判据等 §4.3 一并写死**）⇒ ⇒ **别把 `code=0` 直接映射成「成功」**。

### 4.3 ⬜ 待写死点（本稿唯一的空洞 —— **不许先猜**）

**要定死的是**（登录态下，**尚未实测**）：

1. `data.biz_data` 内**会话列表数组的键名**（候选 `chat_sessions`）；
2. **条目对象的键名**（哪些字段是实现要取的）；
3. **条数字段**（条数从哪个键读）；
4. **游标字段名**（从响应里取 `lte_cursor` 的那个键）。

⇒ **补法（脚本已备好；⚠️ 落点本轮已订正 —— 见 §8.5 / §8.6）**：跑一段**只读**脚本（脚本正文在 [official-chat-progress.md](official-chat-progress.md) 的 **12:1x 节 ⑤ 第 2 条**，**已换成「两档自动降级」版**，逐层下钻到数组、只回结构）⇒ 一次收口**字段面** ＋ **三档错误码**。
> **⚠️ 落点不是「在壳里按 F12」** —— **release 壳里 DevTools 被 wry 关掉**（`wry-0.57.0/src/lib.rs:872-875` ＋ `tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149` ＋ 本仓 `Cargo.toml:30` 无 `devtools` feature ⇒ `SetAreDevToolsEnabled(false)`）⇒ **壳里按 F12 弹不出 DevTools**；**dev 壳即便能弹，那棵树的登录态也已失效** ⇒ **两条落点的完整依据见 §8.5**。
> ⇒ **两条落点**（§8.6）：**A**｜**平时用的浏览器**（Edge / Chrome）里**已登录**的 `chat.deepseek.com` 标签页 ⇒ **零代码改动、零授权**；**B**｜**壳内**一次性只读形状探测（**不需要 DevTools** —— 壳自己的 CDP `Runtime.evaluate` 是进程内调用）⇒ 需**一次授权**改 `src-tauri/**`。

⇒ **取证脚本的两档打法（PM 已逐字段核过输出面）**：
· **第 1 档**：直接同源 fetch `/api/v0/chat_session/fetch_page?count=5`，分别试 **裸 / ＋Bearer / ＋Bearer＋x-device-id** 三种头 ⇒ **用返回码反推哪一档被认**（顺带拿到三档错误码）⇒ ⚠️ 会发**最多 6 个只读 GET**（2 个端点 × 3 种头；命中数组后会提前跳出），预期回 `40003` 的那几档**不解释语义**（§3.3）。
· **第 2 档（兜底）**：第 1 档拿不到数组时**挂上 `fetch` / `XMLHttpRequest` 捕获钩子**，等**站点自己**拉列表时把**它的真实响应体**截下来解析（`res.clone()` 读副本 ⇒ **不吃掉站点自己的响应体**）⇒ ⇒ **不依赖对鉴权头的猜测** ⇒ 触发动作建议用**「新对话」**（**不加载任何会话正文**，比点列表里的某个会话更干净）。
· **只回结构**：状态码 / 业务码 / 键名 / 条数 / 首条键名 / URL（uuid 与 `lte_cursor`、`chat_session_id`、`id` 已打码）⇒ **不回正文、不回标题、不回会话 id、不回任何凭据**（凭据只出长度、不出值）。
· ⚠️ **痕迹范围要说清楚**（**不是** §7.1 的对象 —— §7.1 管的是**生产**页内脚本的零痕迹）：这份脚本会**替换该标签页的 `window.fetch` 与 `XMLHttpRequest.prototype.open/send`**，并挂一个 `window.__dsDump` ⇒ **痕迹只在该标签页，关掉标签页即消失**；**不写站点数据、不写盘、不影响浏览器其他标签页**。
· ✅ **PM 一手复跑（mock 环境，**17/17 全绿**）** —— 把台账里的脚本正文抽出来，在 mock 里跑两遍：① **场景 A**（第 1 档即被认）⇒ 正确下钻出 **`arrayPath = biz_data.chat_sessions`**、`itemCount = 1`、首条键名 `id/title/updated_at/tokens`、`tier1GotArray = true`；② **场景 B**（第 1 档三档头全回 `40003`）⇒ 第 2 档钩子**从站点自己的请求**里截到同一结构（同样下钻出 `biz_data.chat_sessions` 与首条键名）⇒ ⇒ **两档都按预期工作**；③ **输出面机械核对**：序列化后的输出里**查不到** token 本体、**查不到**条目正文/标题、**查不到**会话 id（uuid 在 URL 里被打码成 `<hidden>`）⇒ ⇒ **「只回结构」不是靠读，是跑出来的**。

⇒ **在那之前**：实现**不许**写死任何字段名；适配层**必须**按「**先探结构、再取值**」写（探明路径后固化）⇒ ⇒ **本稿升 v0.5（冻结稿）的条件 ＝ §4.3 四项全部有实测出处**（v0.2 补硬约束、v0.3 改依据、v0.4 补防误读条款，**冻结稿顺延到 v0.5** —— 顺延说明见文首版本线）。

---

## 5. 接线方式（插件 → 壳）

### 5.1 走现役回环桥（不新造监听服务）

- **现役通道**：`src-tauri/src/plugins/builtin/bridge.rs` —— `:11-15` 模块自述「**为什么是「插件来请求壳」**」· `:70-73` 32 字节随机令牌 · `:77` / `:79` `DSH_XLINK_BUILTIN_BRIDGE_TOKEN` / `_URL` · `:90-93` `TcpListener::bind("127.0.0.1:0")`（随机端口）· `:144-156` **Bearer 校验**（不符即拒）· `:157-184` 路由表。
- **壳在启动内核时注入地址与令牌**：`src-tauri/src/kernel/kernel_adapter.rs:533-539` ⇒ **内核侧现役在用**（`plugins/memory/host/bridge.js:4-6`）⇒ **official-chat 也是内置插件 ⇒ 同一份 env 即可用**。
- ⇒ **裁定**：**P4 只在现役桥上加一条路由** ⇒ **复用现役服务，不是新造** ⇒ **不撞计划 §4「不再造一个监听服务」的否决**。
- ⇒ **R3（壳 → 内核 exact route）本阶段不需要**（方向不同：本协议要的是「插件 → 壳」）⇒ **不判、不做、不需新授权**。

### 5.2 加路由的三条硬约束（写死）

1. **必须 POST ＋ 支持读请求体** —— 现役桥「**只读请求头、不读请求体**」（`bridge.rs:185-187`：「GET ＋ 查询串，不读请求体」——TCP 无消息边界，body 可能没到）⇒ 而 P4 要传页内脚本 ＋ 回传结果 ⇒ **必须读体**，**并写死 body 上限**。
2. **不能走查询串** —— 现役客户端 `plugins/memory/host/bridge.js:43` `MAX_PATH_BYTES = 7000` ＋ `:29-30` `REQUEST_TIMEOUT_MS = 1500` ⇒ **页内脚本与结果都远超 URL 长度** ⇒ **必须 POST ＋ body**。
3. **最小能力面**（`bridge.rs:21-22` 自述「**这个桥的调用方是模型能触达的代码，能力面越小越好**」）—— CDP 求值等于开一个任意 JS 执行面 ⇒ **必须**写死：**只对 official-chat 的 webview 求值** · **只允许白名单命令 / 只读操作** · **不许内核传任意 JS** · **不许写站点数据** · **不许触碰非 official-chat 的窗口**。

### 5.3 ⭐ 外挂 CDP 到「活着的页面」：不可行（只有壳能求值）

> **裁定 3（本轮新增）** —— **不许**再考虑「从外部进程把 CDP 挂到已经在跑的页面上」。

**四项实测**（【已实测】，PM 一手复核）：

| 试探 | 结果 |
| --- | --- |
| 活着的 `msedgewebview2` 命令行有没有 `--remote-debugging-port` | **没有** |
| 两棵 profile 根目录有没有 `DevToolsActivePort` | **没有** |
| `msedgewebview2` 有没有监听端口（`Get-NetTCPConnection`） | **没有** |
| release 树的 `Cookies` 能不能直接 open | **`EBUSY`**（连直接 open 都拿不到） |

⇒ **外挂 CDP 不可行** ⇒ 而 `CallDevToolsProtocolMethod` 是 **`ICoreWebView2` 自己的方法**（**进程内调用**；本仓库已实测「未开 DevTools 窗口也能调 `Runtime.evaluate`」——`src-tauri/src/official_chat/cdp_eval.rs:24-27`、`examples/support/cdp_probe.rs`）⇒ ⇒ **调用方必须是那个进程自己** ⇒ ⇒ **只有壳能做页内求值**。

⇒ **与 §2.5 合起来**：**登录态只在活上下文里（§2.5）** ＋ **只有壳能求值（§5.3）** ⇒ ⇒ **「壳内求值」是唯一路径**，**没有第二条路**。

---

## 6. 错误面与失败语义

### 6.1 不扩 13 码闭集

- P1 §3 的 **13 码闭集**不动（`official_chat/protocol.rs:88`）⇒ **站内码一律映射到既有码**，**不新增**。
- **能力缺口**用既有码：macOS 无零痕迹通路 ⇒ 报 `capability_unsupported`（`official_chat/protocol.rs:749`；`:777` 归一化到 `source_unsupported`）⇒ **代码里明确报错、不静默降级**（不偷偷退回带痕迹的槽位）。

### 6.2 站内码只「原样上报」，不解释语义

- **不写**「`40003` ＝ 没登录」这类映射（§3.3：未登录与凭据形态不对**回同一个码**）。
- ⚠️ **站内码有**两层**（顶层 `code` 与 `data.biz_code`）⇒ **报哪一层必须写死** —— 已实测：**未登录态**打 `fetch_page` 是**顶层 `code=40003`**（`data` 为 `null`）⇒ ⇒ **「成功」那一档不能落在顶层 `code=0`**（§4.2 有反例：`code=0` 同时 `biz_code=2`、`biz_data=null`）⇒ ⇒ **成功判据要落在 `data.biz_data` 是否有内容**。
- **三档码**（`40002` / `40003` / 成功）的语义**待实测**（与 §4.3 **同一次取证**顺带拿到；落点见 §8.6）⇒ 在那之前**不写进协议**。

### 6.3 两类错误必须分开

`official_chat/mod.rs:3-6` 已写明：「**回程失败与站点返回空是两类错误**」⇒ ⇒ **回程失败**（求值超时 / 页面销毁 / 导航 / 壳侧错误）与**站点返回空**（接口正常返回但没有会话）**不许合并成一个码、不许互相冒充**。

---

## 7. 零痕迹与只读纪律

### 7.1 零痕迹（页内）

- 页内**不留全局槽位 / 全局变量 / 定时器** ⇒ 判据口径：`getOwnPropertyNames(window)` 前后**逐项一致**、`__dsh*` 为 0（P0 实测 **1239/1239**）。
- **不用 R1 槽位**（P0 已否，§1.1）。

### 7.2 凭据零落盘、零外传

- **不许**把站点 cookie / token 读出来、落盘、外传、拼进请求头（§3.2 已把它升级为**实测事实**）⇒ **壳不持久化站点凭据**（沿用 `official-chat-bridge-research.md` 的同口径）。
- **认证失败**（接口返回错误码）**只在候选组或选中行内可见**，**不另开用户可见路径**。

### 7.3 只读

- **不点按钮、不发消息、不预拉正文、不写站点数据**（探针与生产取数**都**受这条约束）。

---

## 8. 隔离纪律（生产 vs 探针）

### 8.1 红线只管探针

- 红线「**测试/探针**不许碰用户真实数据目录下的 WebView2/Chromium profile」**管的是探针**（浏览器一启动就往那个目录写 Cookies / Preferences / Local Storage / History / IndexedDB，而 Cookies 是活文件、写坏了回滚不了）⇒ **不拦** §0.2 那条**生产路径**（用的就是用户自己开着的窗口的数据目录）。
- 机检口径已落地：`scripts/lib/probe-profile-isolation.mjs`（`check:static` 的第 30/31 条不变量）⇒ **探针侧必须过**。

### 8.2 探针要带登录态时的口径（**已改前提**）

- **旧前提**（「副本带着 `session_id` ⇒ 不需要 izzy 再提供任何登录态」）**已被一手证据反证** ⇒ 见 §8.4。
- **现口径**：**纪律本身不变**（只读拷贝 → 跑 → 删拷贝；三条护栏：复制后核源 mtime 与关键文件 SHA 未变、命中数≠1 即抛错；探测只读；副本用完删掉）⇒ **只改前提**。

### 8.3 取证前先确认源登录态还活着（只读手段）

1. 查 `msedgewebview2` 进程的 `--user-data-dir` ⇒ 有没有进程指向那棵树（**进程在 ⇒ 环境活着**）；
2. 查该树 `Default\Network\Cookies` 的 mtime / `last_access_utc` ⇒ 停在什么时候；
3. 看 `is_persistent` ⇒ **0 ＝ session cookie ⇒ 那个环境一关登录态就没**。

### 8.4 「只读拷贝能否带登录态」—— **已收口：不能**（机制性结论）

- **已观测**：副本里 `chat.deepseek.com` 的 **5 个 cookie 逐项完整在盘上**（行数、`encrypted_value` 长度与源一致）⇒ **站点仍 302 到 `/sign_in`**；整份 `EBWebView` 拷走（1053 文件 / 131.4 MB）后起浏览器 ⇒ 页面落在 **`https://chat.deepseek.com/sign_in`**（`pageLooksLikeAuth: true`）。
- **已排除**：**「加密 cookie 因 Chromium app-bound encryption 跨 exe 解不开」** —— 副本 `Local State` 的 `os_crypt.encrypted_key` 解码后前 6 字节 ＝ **`DPAPI`**（hex `44 50 41 50 50`… 实际是 `44 50 41 50 49 01`）⇒ 绑**机器+用户**、**不绑 exe**。
- **真正的机理（已定位到具体那条 cookie）**：`ds_session_id` 是 **session cookie**（`is_persistent = 0`、`expires_utc = 0`）⇒ **新浏览器进程不恢复非持久 cookie**（启动时按过期处理并清除）⇒ **拷过来也不带登录态**。
- ⇒ **结论（机制性，不是偶发）**：**只读拷贝**带**登录态** —— **不能**。⇒ **拷贝取证路线正式关闭**（不只是「拿不到源文件」，还有**拿到了也不带登录态**）。
- ⇒ 与 §2.5 同源：这就是「执行体在壳」「API 在页内」被写死成**硬约束**的原因。

### 8.5 取证落点（**本轮两轮订正：F12 落点与依据都改了**）

**先说结论**：**F12 手动跑脚本的落点 ＝ 平时用的浏览器（Edge / Chrome）里已登录的 `chat.deepseek.com` 标签页**，**不是** dsh-xlink 壳 —— **release 壳与 dev 壳都不行，但两者的理由不同**。

| 落点 | DevTools 能不能用 | 为什么这个落点仍不可行 |
| --- | --- | --- |
| **release 壳**（**登录态活着的那棵**） | **不能用** | **wry 在 release 下默认把 DevTools 关掉**（依据链见下）⇒ **F12 无效、右键无「检查」** |
| **dev 壳**（`cargo run` 起的 debug 壳） | **可能能开**（debug 下 wry 默认 `devtools = true`）—— **【推定】，未实测** | **与 DevTools 无关**：**dev 树的登录态已随环境关闭失效**（`ds_session_id` 是 session cookie ＋ 官网对话 webview 已关、**0 个**进程指向该树）⇒ ⇒ 即便 F12 弹得出来，打开也是 `sign_in` |

**依据链（release 壳为什么弹不出 DevTools）**（【已实测】，PM 一手核源码）：

1. `wry-0.57.0/src/lib.rs:872-875` —— wry 的 `devtools` 属性默认值是 **`#[cfg(debug_assertions)] devtools: true` / `#[cfg(not(debug_assertions))] devtools: false`** ⇒ ⇒ **release 构建默认就是关的**；
2. `tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149` —— Tauri **只在** `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才 `with_devtools(...)` 覆盖它 ⇒ 本仓 release **无 `devtools` feature**（`src-tauri/Cargo.toml:30`）⇒ ⇒ **不会被打开**；
3. ⇒ `wry-0.57.0/src/webview2/mod.rs:640` `settings.SetAreDevToolsEnabled(attributes.devtools)?` ⇒ ⇒ **`SetAreDevToolsEnabled(false)`** ⇒ **F12 与右键「检查」都不生效**；
4. **DOM 层右键菜单另被禁**：`src-tauri/src/harness/no-context-menu.js`（捕获阶段 `preventDefault()` ＋ `stopImmediatePropagation()`）⇒ 右键菜单（本来含「检查」）**整条不出**；
5. **官网对话窗口没有程序化入口**：`src-tauri/src/official_chat/context.rs:104-113`（`build_read_window` 建窗链）**没有** `open_devtools()`；全仓 `open_devtools()` 只有三处 —— `commands.rs:1426`（工作台 `harness` 窗口、`#[cfg(debug_assertions)]`）、`lib.rs:459`（`main` 管理窗口、`#[cfg(debug_assertions)]`）、`harness/harness_window.rs:481`（工作台、`#[cfg(debug_assertions)]`）⇒ **没有一个是给官网对话页签的**。

> ⚠️ **更正一条被引用的依据（结论不变，依据要改）** —— 「**wry 关掉了浏览器加速键 ⇒ F12 不生效**」**不成立**：wry `src/webview2/mod.rs:649-653` 那句 `SetAreBrowserAcceleratorKeysEnabled(false)` 是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），而 wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`，注释「This is WebView2's default behavior」）⇒ 本仓**没有**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的**。真正关掉 DevTools 的是上面第 1–3 条（`devtools` 属性），**不是**加速键。
> ⇒ **教训**：「某能力不存在」要先找**真正控制它的那个开关**（本例是 `devtools` 属性）⇒ **别拿邻近开关的代码当依据**（本例是加速键）。

- ⚠️ **（PM 自纠第 2 次：PM 上一轮写的依据也是错的）** —— PM 上一轮把这条写成「官方对话窗口建的时候是 `with_devtools(false)`（`src-tauri/src/lib.rs:455-456`）」⇒ **依据错**：`lib.rs:450-459` 那段讲的是 **`main` 管理窗口**（且它自己就 `#[cfg(debug_assertions)]` 门控），**不是官网对话窗口** ⇒ ⇒ **结论（落点不是壳）对，依据（那句注释）不成立** ⇒ 已整段换成上面的依据链。
- **当前登录态在哪**（【已实测】，PM 一手复核）：
  - **release 壳**（`C:\Users\zxx\AppData\Local\dsh-xlink\dsh-xlink.exe`，**0.5.8-rc.2**，与仓库 `package.json` / `tauri.conf.json` 同版）的官方对话 webview **活着**（9 个 webview 进程指向该树；该树 `Cookies` 正在被写）；
  - **dev 壳的官方对话 webview 已关**（0 个进程指向该树；该树 `Cookies` 的 mtime 停在 **12:14:38**）⇒ **dev 那棵树的 session cookie 随环境关闭失效**；
  - **两棵树的账号相同**（`leveldb` 里的 JWT：`name` ＝「平常心💯」、`isAnonymous = false`、`exp` ＝ **2026-11-30**；两棵都有）⇒ **但 JWT 在盘上 ≠ 会话有效**（§2.5：站点认 session cookie）。

### 8.6 取「登录态下的字段面」的两条路（§4.3 的补法）

| 路线 | 落点 | 需要什么 | 成本 | 前提 |
| --- | --- | --- | --- | --- |
| **A｜平时用的浏览器跑只读脚本** | Edge / Chrome 里**已登录**的 `chat.deepseek.com` 标签页 | 粘贴脚本（正文在 [official-chat-progress.md](official-chat-progress.md) 的 **12:1x 节 ⑤ 第 2 条**，**已换成两档版**） | **零代码改动、零授权** | **该浏览器里确实登录着**（PM **未核** ⇒ 缺口 **G4-6**） |
| **B｜壳内一次性只读形状探测** | **壳自己那个官网对话窗口**（**不需要 DevTools**） | **一次授权**改 `src-tauri/**`（加一次性只读探测入口，不进主干） | 一次授权 ＋ 一次构建 | 那个壳的官网对话窗口**开着、且登录态有效** |

⇒ **B 路为什么不需要 DevTools**（B 路成立的前提，【已实测】）：P0 已实测「**不开 DevTools 窗口也能调 `Runtime.evaluate`**」—— `src-tauri/src/official_chat/cdp_eval.rs:24-27` ＋ `src-tauri/examples/support/cdp_probe.rs` ⇒ `CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法、**进程内调用** ⇒ ⇒ **B 路是「壳内求值」，不是「人工开 DevTools」** ⇒ ⇒ **B 路在 release 壳里同样成立**（**不需要 DevTools、不需要重新登录**）。
⇒ **B 路的两个子选项（都要 izzy 拍板，因为都动 izzy 的环境）**：
· **B1｜dev 壳**（`cargo run` 起的 debug 壳）—— izzy **重新登录一次**并保持官网对话页签开着 ⇒ **不碰用户安装版**；代价 ＝ 一次登录。
· **B2｜release 壳** —— 登录态现成 ⇒ **不用重新登录**；代价 ＝ 要有一个**带一次性入口的壳**跑在**用户真实那棵 release 树**上 ⇒ **要动安装版／真实树** ⇒ **先问 izzy**。
⇒ **PM 建议**：先问 izzy 一句「**平时用的浏览器里登录过 DeepSeek 吗**」（**缺口 G4-6**）⇒ 是 ⇒ 走 **A**（最省）；否 ⇒ 走 **B**（**B1 还是 B2 由 izzy 选**：B1 不碰安装版，B2 不用重新登录）。

---

## 9. 已知缺口与登记

| 编号 | 缺口 | 性质 | 证据 |
| --- | --- | --- | --- |
| G4-1 | **macOS 侧零痕迹通路不存在** —— wry 0.57 未暴露 `callAsyncJavaScript`、raw `WKWebView` 为 `pub(crate)` | **已知缺口，不是待验方案**（等上游暴露后单独拍板） | 【已实测】 |
| G4-2 | **站点适配契约的字段面未定死**（§4.3 四项） | **待实测**（补法已备好） | —— |
| G4-3 | **站内码三档语义未定死**（§6.2） | **待实测**（与 G4-2 同一次取证） | —— |
| G4-4 | ~~「只读拷贝能否带登录态」仍未验到~~ | **已收口（2026-10-21 13:0x）**：**不能** —— 机理 ＝ **session cookie 跨进程不可恢复**（§2.5 / §8.4）⇒ **拷贝取证路线正式关闭** | 【已实测】 |
| G4-5 | 页大小上限（`count` 最大值） | **二手来源，未在本机复核** ⇒ 不写成条款 | —— |
| G4-6 | **平时用的浏览器（Edge / Chrome）里是否有 DeepSeek 登录态** —— 未核 | **待 izzy 确认**（决定 §8.6 走 A 还是 B） | —— |
| G4-7 | **B 路的两个子选项要 izzy 选**（§8.6）：**B1**＝dev 壳（**重新登录一次、不碰安装版**）／**B2**＝release 壳（**不用重新登录、但要动用户真实树/安装版**） | **待 izzy 拍板**（**只在 G4-6 答「没登录过」时才需要问**） | —— |

> **不扩错误码、不改 P1 §3 的 13 码闭集、不改 P1 §4.1 的用户可见保留期承诺**。

---

## 10. 验收口径（判据清单，**待立**）

> 本稿**尚未**配判据 ⇒ 实现开工前由 PM 立 P4-1…P4-n。**建议的判据面**（供排期，不是条款）：

1. **取数路线**：实现里**存在**「页内 fetch 站点 API」这一步 ⇒ **不存在**「页外直接调站点 API」的代码路径（**按源码形态断**，不许靠运行时）。
2. **零痕迹**：页内 `getOwnPropertyNames(window)` 前后逐项一致、`__dsh*` 为 0。
3. **回程只回结构**：回程载荷里**没有**正文 / 标题 / 会话 id / 凭据 ⇒ **按回传字段清单断**（白名单之外即红）。
4. **接线**：走**现役**回环桥的一条**新路由**（不是新监听端口）⇒ 按源码形态断。
5. **能力面**：该路由**只对 official-chat 的 webview** 求值、**不接受任意 JS**、**不写站点数据** ⇒ 按源码形态断。
6. **错误面**：不新增第 14 个码（13 码闭集不动）⇒ 按常量闭集断。
7. **两类错误分离**：回程失败 ≠ 站点返回空 ⇒ 两个不同的既有码。
8. **字段面**（§4.3 定死后）：按**实测的键名**断「读到了正确的条目数与条目字段」。

---

## 11. 变更记录

| 版本 | 时间 | 内容 |
| --- | --- | --- |
| v0.1 | 2026-10-21 12:5x（PM） | **草案首版**。把**已实测**的事实写死成条款：① 取数路线三条（§1）② 页内求值机制 ＋ 三条纪律 ＋ 零痕迹（§2）③ 页内同源 fetch ＋ **页外调 API 必失败已升级为实测** ＋ **40003 不可解释成「没登录」**（§3）④ 端点表 ＋ **翻页游标名 `lte_cursor`** ＋ 响应三层骨架 ＋ 静态包 grep 不足（§4.1/4.2）⑤ 接线＝现役回环桥加一条路由 ＋ 三条硬约束（§5）⑥ 不扩 13 码闭集 ＋ 站内码只原样上报 ＋ 两类错误分离（§6）⑦ 零痕迹 / 凭据零落盘 / 只读（§7）⑧ 隔离纪律 ＋ **「只读拷贝带不带登录态」仍未验到**（§8）⑨ 缺口登记 G4-1…G4-5（§9）⑩ 判据清单（§10）。**唯一待写死点**＝§4.3（字段面 ＋ 三档码），补法已备好（只读 F12 脚本，在台账 12:1x 节 ⑤）。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.2 | 2026-10-21 13:0x（PM） | **补三条本轮新实测的硬约束** ＋ **G4-4 收口** ＋ **取证落点订正**。① **§2.5 新增（裁定 2）＝ 登录态机制** —— 登录态**只存在于「同一个活着的浏览器上下文」**（`ds_session_id` 是 **session cookie** ⇒ **新进程不恢复非持久 cookie** ⇒ **拷过来也不带登录态**；且盘上明文 JWT 虽有效到 2026-11-30 却不被认）⇒ ⇒ 「**执行体在壳**」「**API 在页内**」由机制决定，是**硬约束**、**不是偏好** ⇒ **实现方不得提「页外调 API」「从外部进程读」** ⇒ **拷贝取证路线正式关闭**；② **§5.3 新增（裁定 3）＝ 外挂 CDP 不可行** —— 四项实测（活着的 `msedgewebview2` **无** `--remote-debugging-port` / 两棵 profile 根目录**无** `DevToolsActivePort` / **无**监听端口 / release 树 `Cookies` **EBUSY**）⇒ **只有壳能求值**（`CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法、进程内调用）⇒ 与 ① 合起来 ⇒ **「壳内求值」是唯一路径**；③ **§8.4 改写 ＋ §8.5 新增 ＋ §8.6 新增** —— 「只读拷贝能否带登录态」由「**仍未验到**」**收口为「不能」**（机理＝**session cookie 跨进程不可恢复**）；**取证落点订正**（壳的官网对话窗口是 `with_devtools(false)` ⇒ **壳里按 F12 弹不出 DevTools** ⇒ PM 上一轮给 izzy 的落点**走不通**，已更正）；**两条落点 A（平时用的浏览器跑只读脚本｜零改动零授权）/ B（dev 壳内一次性只读形状探测｜需重新登录 ＋ 一次授权）** ＋ **G4-6**（平时用的浏览器里是否有 DeepSeek 登录态 ⇒ **待 izzy 确认**）。**版本线顺延**：v0.2 ＝ 补硬约束的草案稿 ⇒ **v0.3 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.2 为冻结稿）。**§4.3 仍是唯一待写死点**（四项字段面 ＋ 三档错误码）。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.4 | 2026-10-21 13:2x（PM） | **防误读条款** ＋ **端点来源标注**。① **§4.2 新增「顶层 `code=0` ≠ 取到数据」** —— 实跑读数补 **`biz_code=2`** ＋ **`biz_data` 的类型读数是 `null`** ⇒ **那组读数本身就是反例**（`status=200` ＋ **顶层 `code=0`** ＋ `msg=""` 看着像成功，**同时** `biz_code=2`、`biz_data=null`）⇒ ⇒ **传输层成功 ≠ 业务层成功 ≠ 拿到了数据** ⇒ ⇒ **判「成功」不能只看顶层 `code`** ⇒ **成功判据必须落到 `data.biz_data` 是否有内容**（具体判据等 §4.3 一并写死）⇒ ⇒ **别把 `code=0` 直接映射成「成功」** ② **§6.2 补「站内码有**两层**」** —— 顶层 `code` 与 `data.biz_code` ⇒ **报哪一层必须写死**；已实测：**未登录态**打 `fetch_page` 是**顶层 `code=40003`**（`data` 为 `null`）⇒ ⇒ **「成功」那一档不能落在顶层 `code=0`** ⇒ **成功判据落在 `data.biz_data` 是否有内容** ③ **§4.1 补来源标注与第二种口径** —— 上表那 5 个端点来自**站点自己浏览器缓存里的完整请求行**（Chromium 缓存条目 `data_1` 的 key 字段）⇒ **不是「页面当场请求」的记录** ⇒ ⇒ **登录态下的 resource timing 端点清单仍未实测**；另**运行时 resource timing**（**未登录态**）＝ 资源总数 **23** 个、其中 `/api/` 端点**只有 1 个**（`/api/v0/client/settings`，query 只回**键名** `["did","scope"]`）⇒ ⇒ **两种非静态口径都只看到端点路径、看不到字段面** ⇒ **字段面仍只能靠登录态下的结构实测**（§4.3）。**版本线顺延**：v0.4 ＝ 防误读条款 ⇒ **v0.5 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿，**两次顺延**）。**§4.3 仍是唯一待写死点**。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.3 | 2026-10-21 13:1x（PM） | **依据订正** ＋ **B 路补齐** ＋ **取证脚本换两档版**。① **§8.5 的依据整段换掉（PM 第二次自纠）** —— 旧依据「官方对话窗口建的时候是 `with_devtools(false)`（`src-tauri/src/lib.rs:455-456`）」**错**：那段讲的是 **`main` 管理窗口**、且它自己就 `#[cfg(debug_assertions)]` 门控 ⇒ ⇒ 换成**一手核过的依据链**：wry 的 `devtools` 属性默认 **debug ⇒ `true` / release ⇒ `false`**（`wry-0.57.0/src/lib.rs:872-875`）＋ Tauri **只在** `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才覆盖它（`tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149`）＋ 本仓 `src-tauri/Cargo.toml:30` **无 `devtools` feature** ⇒ ⇒ `SetAreDevToolsEnabled(false)`（`wry-0.57.0/src/webview2/mod.rs:640`）⇒ ⇒ **release 壳里 F12 无效、右键无「检查」**；另补两条：**DOM 层右键菜单被 `no-context-menu.js` 取消**（捕获阶段 `preventDefault()` ＋ `stopImmediatePropagation()`）＋ **官网对话窗口没有程序化 `open_devtools()` 入口**（`official_chat/context.rs:104-113`；全仓 `open_devtools()` 三处分别给工作台与 `main`，都 `#[cfg(debug_assertions)]`）② **撤回一条被引用的依据**：「**wry 关掉了浏览器加速键 ⇒ F12 不生效**」**不成立** —— `SetAreBrowserAcceleratorKeysEnabled(false)` 是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），而 wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`，注释「This is WebView2's default behavior」）⇒ 本仓**未**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的** ⇒ **结论（落点不是壳）不变、依据换**；**教训**：「某能力不存在」要先找**真正控制它的那个开关**，别拿邻近开关的代码当依据 ③ **落点分两档写准**：**release 壳** ＝ DevTools 被关（**真**）；**dev 壳** ＝ DevTools **可能能开**（**【推定】，未实测**）**但 dev 树的登录态已随环境关闭失效** ⇒ ⇒ **两档都不可用**，且 dev 壳的理由**与 DevTools 无关** ④ **§8.6 补 B 路的两条关键事实**：**B 路不需要 DevTools**（壳自己的 CDP `Runtime.evaluate` 是进程内调用，P0 已实测「不开 DevTools 窗口也能调」）⇒ ⇒ **B 路在 release 壳里成立、不用重新登录**；并拆成 **B1（dev 壳：重新登录一次、不碰安装版）/ B2（release 壳：不用重新登录、但要动真实树）** 供 izzy 选 ⑤ **§4.3 补法换成「两档自动降级」脚本**（第 1 档按裸/＋Bearer/＋Bearer＋`x-device-id` 三种头反推哪档被认 ＋ 第 2 档钩**站点自己**的请求、用 `res.clone()` **不吃掉站点响应体**）＋ **写清痕迹范围**（只在该标签页、关掉即消失、**不写站点数据**）＋ **第 2 档触发动作建议用「新对话」**（不加载会话正文）＋ **PM 已逐字段核过输出面**（只回结构，不回正文/标题/会话 id/凭据）。**版本线顺延**：v0.3 ＝ 依据订正稿 ⇒ **v0.4 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿）。**§4.3 仍是唯一待写死点**。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
