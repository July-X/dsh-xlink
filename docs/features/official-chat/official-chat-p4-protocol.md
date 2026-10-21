# P4 协议：官网对话的「直接 @ 引用」（v0.7 冻结稿）

> **状态**：**v0.7 ＝ 冻结稿**（PM 2026-10-21 15:3x 定）—— v0.1 把**已实测**的事实写死成条款；v0.2 补**三条本轮新实测的硬约束**（§2.5 登录态机制 · §5.3 外挂 CDP 不可行 · §8.5 取证落点改正）；v0.3 是**依据订正**（§8.5 落点依据整段换掉 ＋ B 路不需要 DevTools ＋ 取证脚本两档版）；v0.4 是**防误读条款**（§4.2「顶层 `code=0` ≠ 取到数据」＋ §6.2 站内码两层 ＋ §4.1 端点来源标注）；v0.5 ＝ **G4-6 收口**（PM 一手核完浏览器里的登录态）＋ **落点收窄为「Chrome」**；v0.6 ＝ **补两条口径**（①「dev 壳可能能开」为什么只能写【推定】② B2 要重新构建安装版 ⇒ 归 izzy 自己拍板、自己测）；**v0.7 ＝ 补齐 §4.3 字段面后的冻结稿**（§4.3 四项**全部**写死 + §3.1／§6.2／§7.1／§7.2／§8.5／§8.6 依据改写 + §10 判据补第 9 条）。
> **版本线顺延（写明，免得后来的人对不上）**：**v0.1 → v0.2 ＝ 补三条硬约束** → **v0.3 ＝ 依据订正＋脚本两档版** → **v0.4 ＝ 防误读条款** → **v0.5 ＝ G4-6 收口＋落点收窄** → **v0.6 ＝ 补两条口径** → **v0.7 ＝ 补齐字段面后的冻结稿**（**原定 v0.3 为冻结稿，先后因「改依据」「补防误读条款」「收口 G4-6」「补口径」「补字段面」五次顺延；本轮不再顺延**）。
> **本稿的「冻结」是什么意思**：**§4.3 四项字段面已全部有实测出处**（§4.3）⇒ ⇒ **协议口径定死** ⇒ ⇒ **实现按本稿写，不再等协议**；**后续若要改条款 ⇒ 升版本号并写明顺延理由**（§11）。
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

> **裁定 1**：**「页内取数」与「调站点 API」不是二选一** —— 真解是**在页内发起对站点自己 API 的同源请求**。
> ⇒ 下列三条**必须**同时成立，缺一条实现就会走错：

1. **执行体在壳** —— 页内求值由**壳**发起（CDP `Runtime.evaluate`）。
2. **API 调用也在页内，且必须复用站点自己的请求/逻辑** —— ⚠️ **实测：「同源 ＋ 浏览器自动带 cookie」不被认**（登录态下裸档打 `users/current` 读数**顶层 `code=40002`**、打 `fetch_page` 读数**顶层 `code=40003`**）⇒ ⇒ **页内不能靠「自动带 cookie」取数** ⇒ ⇒ **同时也不自拼鉴权头** ⇒ ⇒ **正解＝页内复用站点自己的请求/逻辑**（**零自拼头、零读 token、零页外**，§3.1／§7.2）；**页外直接调该 API 必失败**（§3.2／§3.3）。
3. **回程只回结构** —— 宿主只拿到**结构**（键名 / 条数 / 布尔量），**不回正文、不回凭据**（§2.4）。

### 1.1 为什么必须是壳内求值（技术必然性）

- DeepSeek 的目录端点是**异步**的（**实测：站点自己的 `/api/v0/*` 走 XHR**，§3.1）⇒ **同步**的 `script_eval::read_json`（`src-tauri/src/official_chat/script_eval.rs`）**取不到结果** ⇒ **必须 `awaitPromise`** ⇒ **首选 CDP `Runtime.evaluate`**（`src-tauri/src/official_chat/cdp_eval.rs:47`，`awaitPromise` ＋ `returnByValue`）——【已实测】（P0 的 `--mode cdp` **5/5**：未开 DevTools 窗口可调、直接拿到结算值、**页面 JS 空间零痕迹**：`getOwnPropertyNames(window)` 前后 **1239/1239**、`__dsh*` 为 0）。
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

- 脚本**只做一件事**：在页面里**发起对站点自己目录接口的同源请求**（§3），然后把**结构**序列化回宿主（§2.4）。
- ⚠️ **形态不是「自己发一个 `fetch`」** —— **实测：站点自己的 `/api/v0/*` 全部走 XHR、不走 fetch**（登录态抓取批次的 `via` 分布：`xhr` 25／30 条，`fetch` 只有 1 条且**不是** `/api/v0/`）⇒ ⇒ **实现必须复用站点自己的 XHR 逻辑**（**零自拼鉴权头、零读 token**）⇒ ⇒ 「**自己拼头 ＋ 自己 `fetch`**」**不在允许形态内**（§3.1／§7.2）。
- **不许**在页面里留任何全局变量 / 槽位 / 定时器 ⇒ **零痕迹**（P0 实测口径：`getOwnPropertyNames(window)` 前后不变）；⚠️ **允许的页内痕迹形态见 §7.1**（替换 `XMLHttpRequest.prototype` 的 `open`/`send`，**取到即还原**）—— 该形态**不新增 `window` 自有属性**，故判据 2 仍成立；**「还原后逐项一致」本轮未实测 ⇒ 标【推定】**。

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

## 3. 页内同源请求（API 调用在页内，**复用站点自己的请求**）

### 3.1 凭据机制（写死）

| 项 | 事实 | 证据 |
| --- | --- | --- |
| 登录 cookie | **HttpOnly** ⇒ 页内 `document.cookie` **看不到**（只看到 `.thumbcache…` / `smidV2` 这类匿名项） | 【已实测】 |
| **页内取数（结论已改写）** | ⚠️ **「同源 ＋ 浏览器自动带 cookie」实测不被认** —— 登录态下裸档打 `users/current` 读数**顶层 `code=40002`**、打 `fetch_page` 读数**顶层 `code=40003`** ⇒ ⇒ **页内不能靠「自动带 cookie」取数**；**同时不自拼鉴权头** ⇒ ⇒ **正解＝页内复用站点自己的请求/逻辑**（§2.3）⇒ ⇒ **「页内不需要自己拼凭据」这个结论**成立，**但成立的理由是「纪律」不是「自动带 cookie 就够」** | 【已实测】（登录态裸档读数 `40002`／`40003`；**紧跟业务码**，不只看 `status=200`） |
| 站点 JS 的鉴权 | **`Authorization: Bearer <64>`**（**整头长 71**、`Bearer ` 前缀）＋ `x-device-id`（**长 36**）＋ `x-settings-token`（**长 345**） | 【已实测】（登录态抓取批次的请求头名与长度；**只测名与长度、未回值**） |
| **请求头名清单（已收窄）** | `/api/v0/` 全部请求的头名**并集 ＝ 11 个**：`accept` / `authorization` / `content-type` / `x-client-bundle-id` / `x-client-locale` / `x-client-platform` / `x-client-timezone-offset` / `x-client-version` / `x-device-id` / `x-device-model` / `x-settings-token` ⇒ ⇒ **`x-ds-compression` 与 `x-request-encryption-key` 未出现在普通 `/api/v0/` 请求头里**（只在 telemetry 侧见过）⇒ ⇒ **本稿不再要求这两个头** | 【已实测】（登录态抓取批次的头名并集） |
| **头不是每个端点都带** | `authorization`：`client/settings?scope=banner` **不带**，其余 `/api/v0/` 端点带；`x-settings-token`：**只有** `client/settings` 的 `main`/`model`/`web_upgrade`/`provider`/`report` 带，`fetch_page`／`users/current`／`chat_session/create`／`users/auth_token/check_device`／`chat/create_pow_challenge` **都不带** | 【已实测】（同上，按端点逐条核） |
| **站点自己的请求走什么** | **`/api/v0/*` 全部走 XHR、不走 fetch**（抓取批次 `via` 分布：`xhr` 25／30 条，`fetch` 仅 1 条且**不是** `/api/v0/`）⇒ ⇒ **实现形态 ＝ 复用站点自己的 XHR 逻辑**，不是自己发 `fetch` | 【已实测】 |
| `x-device-id` 的来源 | 长 **36**，与 `localStorage` 的 `deepseek-device-id:chat`（同为 36）**等值**（读数给出等值判定） | 【已实测】（同上） |
| 盘上明文 | `localStorage` 键名 `userToken` / `settingsJwt` / `__appKit_userInfo` / `__appKit_@deepseek/*` / `deepseek-device-id:chat`；**JWT 明文躺在 leveldb**（`Default\Local Storage\leveldb\*.ldb`）⇒ 不必解密即可读 `exp` 与 `user`；⚠️ **`userToken` 实测形态 ＝ 92 字符／2 个冒号／3 段（8·78·4），不是 JWT**（§7.2） | 【已实测】 |

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
| 会话目录**翻页** | `GET /api/v0/chat_session/fetch_page?lte_cursor.pinned=false` ⇒ **游标参数名 ＝ `lte_cursor`**（**不是 offset**）＋ `pinned`；⚠️ **点号语法**：`lte_cursor` 是**对象参数**、其字段用**点号路径**传（**无游标时 `lte_cursor` 无值**）；**游标值形态 ＝ 条目的 `updated_at`**（实测 `lte_cursor.updated_at=1783051054.518`） |
| 单会话正文 | `GET /api/v0/chat/history_messages?chat_session_id=<uuid>` |
| 当前用户 | `GET /api/v0/users/current` |
| 客户端设置 | `GET /api/v0/client/settings?did=<uuid>&scope=<banner｜main｜model｜provider｜web_upgrade>` |

- **端点路径与首屏/翻页参数名 ⇒ 本层可写死。**
- **页大小上限**：`count` 的最大值 —— **二手来源**称一次最多 100 条，**未在本机复核** ⇒ **不写成条款**。⚠️ **实测到的是「`chat_sessions` 数组长 100／页」**（登录态实测，见 §4.3）⇒ ⇒ **这不等于「`count` 上限 ＝ 100」** ⇒ ⇒ **两件事不许混写**。
- ⚠️ **静态包 grep 补不齐这一层**（【已实测】）：470 个 JS 缓存里静态 grep `/api/v0/` 只捞到 **10 个端点**，**`chat_session/*` 与 `history_messages` 不在其中**（前端是**运行时拼接**）⇒ ⇒ **端点面只能靠运行时实测**，静态分析不能替代。
- ⚠️ **上表前 5 行的来源要说准**（【已实测】）：来自**站点自己浏览器缓存里的完整请求行**（Chromium 缓存条目 `data_1` 的 key 字段）⇒ **不是**「页面当场请求」的记录 ⇒ ⇒ **首屏 `count=5` 与 `history_messages` 两条至今未在登录态请求批次里复现**（本轮抓到的是页面初始化批次，站点当时**未点开会话**）⇒ ⇒ **这两条仍属「缓存来源」，不标【已实测】**。
- ✅ **登录态下的端点面已由运行时实测补齐**（【已实测】，2026-10-21 15:2x–15:3x）：页面初始化批次共抓到 **12 条 `/api/v0/` 响应**，覆盖 **9 个端点形态** —— `client/settings`（`scope` ＝ `main`/`model`/`web_upgrade`/`provider`/`banner`）、`users/current`、`chat_session/fetch_page`（首屏 `?lte_cursor.pinned=false` 与翻页 `?lte_cursor.pinned=false&lte_cursor.updated_at=…`）、`chat/create_pow_challenge`（POST）、`users/auth_token/check_device`（POST）、`chat_session/create`（POST）、`client/settings/report`（POST）。
- ⚠️ **两种「非静态」口径也都补不齐字段面**（【已实测】，**未登录态**跑的）：**运行时 resource timing** 里资源总数 **23** 个、其中 `/api/` 端点**只有 1 个**（`/api/v0/client/settings`，query 只回**键名** `["did","scope"]`）⇒ ⇒ **两种口径都只能看到端点路径，**看不到**响应里的字段面 ⇒ ⇒ **字段面仍然只能靠登录态下的结构实测**（§4.3，**本稿已收口**）。

**实测响应键名（**这些不是会话目录**，列出来是为了让实现别去错取）**（【已实测】，登录态 12 条响应逐条读数）：

| 端点 | `biz_data` 键 |
| --- | --- |
| `client/settings`（各 scope） | `version` / `settings`（`settings` 内另有 `model_configs.value` 数组 len=3、`providers.value` 数组 len=4） |
| `users/current` | 15 键，含 `id_profiles`（数组 len=1）＋ `token`（string，**值未读**） |
| `chat/create_pow_challenge` | `challenge` |
| `users/auth_token/check_device` | `rotate` |
| `chat_session/create` | `chat_session` / `ttl_seconds` |
| `client/settings/report` | **键集合为空**（读数 `biz_dataKeys: null`） |

⇒ ⇒ **只有 `chat_session/fetch_page` 的 `biz_data` 里才有会话列表**（§4.3）。

### 4.2 响应骨架（【已实测】三层）

```
{ code, msg, data: { biz_code, biz_msg, biz_data } }
```

- **反例（⚠️ 别拿它当正例）** —— 端点 **`/api/v0/client/settings`** ＋ **未登录态** ＋ **探针 A 实跑、两次独立复现一致**：`status=200`、`code=0`、`msg=""`、`dataKeys=["biz_code","biz_msg","biz_data"]`、**`biz_code=2`**、**`biz_data` 的类型读数是 `null`**。
- ⚠️ **层级容易写错**：**会话目录的条目数组在 `data.biz_data` 里面**，**不是** `data.chat_sessions`。
- ⚠️⚠️ **「顶层 `code=0` ≠ 取到数据」（最容易误判的一条，写死）** —— 上面那组读数就是**反例**：`status=200` ＋ **顶层 `code=0`** ＋ `msg=""`（看着像成功）**同时** `biz_code=2` 且 **`biz_data` 是 `null`** ⇒ ⇒ **传输层成功 ≠ 业务层成功 ≠ 拿到了数据** ⇒ ⇒ **判「成功」不能只看顶层 `code`** ⇒ ⇒ **别把 `code=0` 直接映射成「成功」**。
- ✅ **成功判据（本稿写死）** —— **三条同时成立**才算成功：**`status == 200`** **且** **顶层 `code == 0`** **且** **`data.biz_data` 非 null 且有内容**。
  · ⇒ **正例也已实测**（登录态、站点自己那 12 条 `/api/v0/` 响应）：**全部** `code=0` ＋ `msg=""` ＋ **`biz_code=0`** ＋ `biz_data` **非 null** ⇒ ⇒ **成功档形态 ＝ `code=0` ＋ `biz_code=0` ＋ `biz_data` 非 null**（【已实测】）。
  · ⚠️ **两层都要看**：`code=0` 且 `biz_code=2` ＝ **传输成功、业务失败**（反例）｜`code=0` 且 `biz_code=0` ＝ **成功**（正例）⇒ ⇒ **`biz_code` 这一层不能省**。
  · ⚠️ **顶层 `code` 的数值 ＝ 0** ⇒ **双源**（izzy 原始读数 ＋ 登录态 12 条响应逐条读数）⇒ ⇒ **可写死**；**而 `biz_code` 的数值**（成功档 ＝ 0）**只有登录态那一源**。

### 4.3 ✅ 字段面（**已写死** —— 四项全部有实测出处，2026-10-21 15:2x–15:3x 收口）

**四项写死**（**登录态**实测；**双源**：izzy 原始读数 ＋ 成员侧 CDP 一手读数，两源独立给出同一结论）：

| 项 | 写死的答案 |
| --- | --- |
| ① 会话列表数组的键名 | **`data.biz_data.chat_sessions`**（⚠️ **不是** `data.chat_sessions`） |
| ② 条目对象的键名 | **6 个**：`id`（string）／`title`（string）／`title_type`（string）／`model_type`（string）／`pinned`（boolean）／`updated_at`（number） |
| ③ 条数从哪个键读 | **`data.biz_data.chat_sessions.length`** ⇒ ⇒ **响应体里没有「条数字段」**；`data.biz_data.has_more` 的类型 ＝ **boolean**，含义 ＝ **「是否还有更多页」**，**不是条数** |
| ④ 游标字段名 | **响应体里没有游标字段**（顶层／`data`／`biz_data` **三处皆无**，全文无 `cursor` 字样）⇒ ⇒ **游标在请求 query 里**：`lte_cursor` 是**对象参数**、字段用**点号路径**传（§4.1）⇒ ⇒ **客户端自己构造下一页游标**（用条目的 `updated_at`） |

**逐项依据**（【已实测】，PM 一手核读数原文，**不采信自报**）：

- **①／②**：两源一致 —— 数组键名 ＝ `data.biz_data.chat_sessions`；条目 ＝ 上面那 **6** 个键；实测**每页 100 条**（`chat_sessions` 数组 len=100）。
- **③**：`biz_data` **恰 2 键** ＝ `chat_sessions` ＋ `has_more`；**`has_more` 的类型读数 ＝ `boolean`** ⇒ ⇒ **它不是条数字段** ⇒ ⇒ **条数只能从 `chat_sessions.length` 读**。
- **④**：响应体**三处皆无**游标键；**实测翻页请求行** ＝ `?lte_cursor.pinned=false`（无游标时 `lte_cursor` 无值）／`?lte_cursor.pinned=false&lte_cursor.updated_at=1783051054.518`（第二页）⇒ ⇒ **游标值取自条目的 `updated_at`**（形态 ＝ **秒级小数时间戳**）。
- ⚠️ **类型与数值分列（不许混写）**：**顶层 `code` 的数值 ＝ 0** 是**双源**；**`biz_code` 的数值**（成功档 ＝ 0）**只有登录态那一源**；**`has_more` 的具体取值**（`true`／`false`）**本轮未读** ⇒ ⇒ **实现只许依赖上面写死的那几项，不得对 `has_more` 的取值再作任何推断**。
- ⚠️ **每页 100 条 ≠ `count` 上限 ＝ 100**（§4.1）—— 两件事不许混写。

**成功判据**：见 §4.2（`status == 200` **且** 顶层 `code == 0` **且** `data.biz_data` 非空）。

⇒ **本节已收口** ⇒ ⇒ **实现可以按上表键名直接取值**（不再需要「先探结构、再取值」的临时写法）⇒ ⇒ **本稿升 v0.7（冻结稿）的条件已达成**（v0.2 补硬约束、v0.3 改依据、v0.4 补防误读条款、v0.5 收口 G4-6、v0.6 补口径、v0.7 补字段面 —— **顺延说明见文首版本线**）。

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
- ⚠️ **站内码有**两层**（顶层 `code` 与 `data.biz_code`）⇒ **报哪一层必须写死** —— 已实测：**未登录态**打 `fetch_page` 是**顶层 `code=40003`**（`data` 为 `null`）⇒ ⇒ **「成功」那一档不能落在顶层 `code=0`**（§4.2 有反例：`code=0` 同时 `biz_code=2`、`biz_data=null`）⇒ ⇒ **成功判据要落在 `data.biz_data` 是否有内容**（§4.2 已写死）。
- ✅ **三档码的实测状态（本稿不再挂「待实测」）** —— **【已实测】**：
  · **未登录态两档**：`40002 Missing Token`（打 `users/current`）／`40003 INVALID_TOKEN`（打 `fetch_page`）—— **msg 逐字记录**（P0）。
  · **登录态第 1 档（三档头 × 两端点）**：**6 条全部被拒** —— `users/current` 裸档回 **`40002`**；其余 **5 条**回 **`40003`**；⚠️ **两个端点的 `msg` 文本不同** ⇒ ⇒ **站内码必须按「端点 ＋ 档位」逐条写**，**不许把同一个码当成同一语义**。
  · **成功档形态**：**`code=0` ＋ `biz_code=0` ＋ `biz_data` 非 null**（§4.2）。
  · ⇒ ⇒ **三档都齐了**（`40002` / `40003` / 成功）⇒ ⇒ **本条不再是待实测项**。
- ⚠️ **仍然不许做的事**：**站内码只「原样上报」，不解释语义**（§3.3）—— 上面逐字记录 `msg` 是为了**区分端点/档位**，**不是**给 `40003` 下「＝ 没登录」这种定义。

### 6.3 两类错误必须分开

`official_chat/mod.rs:3-6` 已写明：「**回程失败与站点返回空是两类错误**」⇒ ⇒ **回程失败**（求值超时 / 页面销毁 / 导航 / 壳侧错误）与**站点返回空**（接口正常返回但没有会话）**不许合并成一个码、不许互相冒充**。

---

## 7. 零痕迹与只读纪律

### 7.1 零痕迹（页内）

- 页内**不留全局槽位 / 全局变量 / 定时器** ⇒ 判据口径：`getOwnPropertyNames(window)` 前后**逐项一致**、`__dsh*` 为 0（P0 实测 **1239/1239**）。
- **不用 R1 槽位**（P0 已否，§1.1）。
- ✅ **允许的页内痕迹形态（PM 裁定，v0.7 新增）** —— **实测：站点自己的 `/api/v0/*` 全部走 XHR**（§3.1）⇒ ⇒ **要复用站点自己的请求，就必然会碰 `XMLHttpRequest.prototype`** ⇒ ⇒ **裁定：允许「替换 `XMLHttpRequest.prototype` 的 `open` / `send` ＋ 取到即还原」这一种形态**。
  · ⇒ **约束三条**：① **不新增 `window` 自有属性**（⇒ 判据 `getOwnPropertyNames(window)` 前后一致**仍然成立**）② **取到即还原**（不留钩子、不留定时器、不留槽位）③ **不写站点数据**（§7.3）。
  · ⚠️ **「还原后逐项一致」本轮未实测 ⇒ 标【推定】** —— 实现须自证还原彻底（判据 2 按此口径写）。
  · ⇒ **其余形态一律仍不允许**（挂 `window.__xx` 槽位、挂全局对象、留定时器）。

### 7.2 凭据零落盘、零外传

- **不许**把站点 cookie / token 读出来、落盘、外传、拼进请求头（§3.2 已把它升级为**实测事实**）⇒ **壳不持久化站点凭据**（沿用 `official-chat-bridge-research.md` 的同口径）。
- ⚠️ **这条是「纯纪律」，不是「技术保证」** —— **实测**：`authorization` 头里 `Bearer ` 的**裸值 64 字符**，**就是 `localStorage.userToken`（92 字符／2 个冒号／3 段 8·78·4）的子串**（读数给出包含关系判定）⇒ ⇒ **技术上完全可自拼** ⇒ ⇒ **正是不许拼**（正解 ＝ 页内复用站点自己的请求，§2.3）⇒ ⇒ **§10 判据第 9 条（9a 自拼凭据头／9b 读站点凭据）正是这条纪律的机检**。
  · ⇒ ⇒ **「形态不是 JWT ⇒ 它不是 token」是无效推理**（JWT 只是格式、不是判据；**判据是「一个值是不是另一个值的子串」**）—— 实现与评审一律按子串判。
  · ⇒ 另：站点**自己**就把 `token` 放在 `users/current` 的响应里（`biz_data` 的 `token` 键，**值未读**）⇒ ⇒ 「技术上做不到」**更不成立**。
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

**先说结论**：**「F12 手动跑脚本」这条路已结案** —— ⚠️ **不是「落点选错了」，是「探法本身不成立」**：靠页面里挂 `fetch`／`XHR` 钩子等站点自己拉列表，**时序上不可达**（钩子挂上时，站点自己的初始化批次**已经发完**）⇒ ⇒ **实际取到字段面的落点 ＝ CDP 抓取批次**（`Page.addScriptToEvaluateOnNewDocument` 注入 ＋ `Network` 域读请求/响应）⇒ ⇒ **§4.3 已按该落点收口**。
**⇒ 落点本身仍然不是 dsh-xlink 壳** —— **release 壳与 dev 壳都不行，但两者的理由不同**（下表）。

| 落点 | DevTools 能不能用 | 为什么这个落点仍不可行 |
| --- | --- | --- |
| **release 壳**（**登录态活着的那棵**） | **不能用** | **wry 在 release 下默认把 DevTools 关掉**（依据链见下）⇒ **F12 无效、右键无「检查」** |
| **dev 壳**（`cargo run` 起的 debug 壳） | **可能能开**（debug 下 wry 默认 `devtools = true`）—— **【推定】，未实测** | **与 DevTools 无关**：**dev 树的登录态已随环境关闭失效**（`ds_session_id` 是 session cookie ＋ 官网对话 webview 已关、**0 个**进程指向该树）⇒ ⇒ 即便 F12 弹得出来，打开也是 `sign_in` |

> **⇒ 为什么 dev 壳这一档只能写「可能能开」而不是「能开」** —— **wry 0.57 自己不绑 F12**：全仓 grep 不到任何 `F12`／`AcceleratorKeyPressed` 处理（加速键相关的**只有** `browser_accelerator_keys` 那个开关、且默认 `true`）⇒ ⇒ **F12 能不能触达完全取决于 WebView2 自身行为**，不是 wry 提供的 ⇒ ⇒ **「【推定】未实测」这个标注是准确的，不宜写成「能开」**（成员补的支撑，PM 一手核过 wry 源码后采纳）。

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

### 8.6 取「登录态下的字段面」的两条路（**§4.3 已按此收口 —— 本节改为结论留档**）

**结论先说**：**两条路都跑过了，字段面已收口**（§4.3）⇒ ⇒ **本节不再是「怎么做」的待办，是「怎么做的」的留档**（免得后来的人重走一遍）。

| 路线 | 落点 | 需要什么 | 成本 | 实际结果 |
| --- | --- | --- | --- | --- |
| **A｜平时用的浏览器跑只读脚本** | **Chrome** 里**已登录**的 `chat.deepseek.com` 标签页（**Edge 已排除** —— §9 G4-6） | 粘贴脚本（正文在 [official-chat-progress.md](official-chat-progress.md) 的 **12:1x 节 ⑤ 第 2 条**，**两档版**） | **零代码改动、零授权** | ⚠️ **脚本本身不成立** —— 页内 `fetch`／`XHR` 钩子**时序不可达**（钩子挂上时，站点自己的初始化批次**已经发完**）⇒ ⇒ **A 路升级为「CDP 抓取批次」**（`Page.addScriptToEvaluateOnNewDocument` 注入 ＋ `Network` 域读请求/响应）⇒ ⇒ **零改动、零授权成立**，**§4.3 由此收口** |
| **B｜壳内一次性只读形状探测** | **壳自己那个官网对话窗口**（**不需要 DevTools**） | **一次授权**改 `src-tauri/**`（加一次性只读探测入口，不进主干） | 一次授权 ＋ 一次构建 | **未启用** —— 字段面已由 A 路收口 ⇒ ⇒ **本轮不需要 izzy 拍板** |

⇒ **B 路为什么不需要 DevTools**（B 路成立的前提，【已实测】）：P0 已实测「**不开 DevTools 窗口也能调 `Runtime.evaluate`**」—— `src-tauri/src/official_chat/cdp_eval.rs:24-27` ＋ `src-tauri/examples/support/cdp_probe.rs` ⇒ `CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法、**进程内调用** ⇒ ⇒ **B 路是「壳内求值」，不是「人工开 DevTools」** ⇒ ⇒ **B 路在 release 壳里同样成立**（**不需要 DevTools、不需要重新登录**）。
⇒ **B 路的两个子选项（**保留档；本轮不需要选**）**：
· **B1｜dev 壳**（`cargo run` 起的 debug 壳）—— izzy **重新登录一次**并保持官网对话页签开着 ⇒ **不碰用户安装版**；代价 ＝ 一次登录。
· **B2｜release 壳** —— 登录态现成 ⇒ **不用重新登录**；代价 ＝ 要有一个**带一次性入口的壳**跑在**用户真实那棵 release 树**上 ⇒ **要动安装版／真实树** ⇒ **先问 izzy**。
  · ⚠️ **B2 要「重新构建安装版」⇒ 那不是零成本**（成员提的操作性提醒，PM 采纳写进协议）：装那个一次性入口意味着**改动 `src-tauri/**` ＋ 重新构建 ＋ 装到用户真实那棵树** ⇒ ⇒ **属于「动用户安装版」的动作 ⇒ 归 izzy 自己拍板、自己测**（**成员的分工只到 dev**）⇒ **协议与派单里不许把它写成「零成本」**。
⇒ **当前状态**：**A 路（CDP 抓取批次）已把字段面收口**（§4.3）⇒ ⇒ **本节无待办**；**B 路只在「需要在壳内复现字段面」时才启用**，届时**才需要 izzy 在 B1／B2 里选**。

---

## 9. 已知缺口与登记

| 编号 | 缺口 | 性质 | 证据 |
| --- | --- | --- | --- |
| G4-1 | **macOS 侧零痕迹通路不存在** —— wry 0.57 未暴露 `callAsyncJavaScript`、raw `WKWebView` 为 `pub(crate)` | **已知缺口，不是待验方案**（等上游暴露后单独拍板） | 【已实测】 |
| G4-2 | ~~**站点适配契约的字段面未定死**（§4.3 四项）~~ | **已收口（2026-10-21 15:3x）** —— **四项全部有实测出处**（§4.3）：① `data.biz_data.chat_sessions`（**双源一致**）② 条目 **6 键**（`id`／`title`／`title_type`／`model_type`／`pinned`／`updated_at`，**双源一致**）③ **响应未回带条数字段** ⇒ 条数从 `data.biz_data.chat_sessions.length` 读（`has_more` 类型 ＝ **boolean** ＝「还有更多」标志）④ **响应体三处皆无游标键** ⇒ 游标在**请求 query**（`lte_cursor` 对象参数、**点号路径**、值取自条目 `updated_at`） | 【已实测】 |
| G4-3 | ~~**站内码三档语义未定死**（§6.2）~~ | **已收口（2026-10-21 15:3x）** —— **三档齐**（§6.2）：未登录态 `40002 Missing Token`／`40003 INVALID_TOKEN`（msg 逐字）＋ 登录态第 1 档 **6 条全拒**（`users/current` → `40002`，其余 5 条 → `40003`，**两端 msg 不同**）＋ **成功档形态** ＝ `code=0` ＋ `biz_code=0` ＋ `biz_data` 非 null ⇒ ⚠️ **语义仍不解释**（只原样上报） | 【已实测】 |
| G4-4 | ~~「只读拷贝能否带登录态」仍未验到~~ | **已收口（2026-10-21 13:0x）**：**不能** —— 机理 ＝ **session cookie 跨进程不可恢复**（§2.5 / §8.4）⇒ **拷贝取证路线正式关闭** | 【已实测】 |
| G4-5 | 页大小上限（`count` 最大值） | **二手来源，未在本机复核** ⇒ 不写成条款 | —— |
| G4-6 | ~~**平时用的浏览器（Edge / Chrome）里是否有 DeepSeek 登录态** —— 未核~~ | **已收口（2026-10-21 13:3x）：PM 已一手核完** —— **Chrome（Default profile）有**：`localStorage` 的 leveldb 里 **`userToken` ＋ `__appKit_userInfo` 都在**，且 `userToken` 与 origin 字符串 **同一条记录**（偏移相距 **155 字节**）⇒ **登录痕迹在**；leveldb 最新 mtime **2026-10-21 12:08:39**。**Edge 排除**：只有 14 处 `chat.deepseek.com` 访问痕迹、**没有** `userToken`／`__appKit_userInfo`，leveldb 最新 mtime **2026-10-16 09:27:37**。⇒ ⇒ **落点 A ＝ Chrome**；**残留**：Chrome 的 `Cookies` 表**读不到**（被独占锁）⇒ 「痕迹在」**≠**「此刻 session 仍有效」⇒ **最后一次仍要 izzy 跑脚本确认** | 【已实测】（PM 一手只读核查；**未解 cookie 值、未写用户目录**） |
| G4-7 | **B 路的两个子选项要 izzy 选**（§8.6）：**B1**＝dev 壳（**重新登录一次、不碰安装版**）／**B2**＝release 壳（**不用重新登录、但要动用户真实树/安装版**） | **已消解（2026-10-21 15:3x）** —— **字段面已由 A 路（CDP 抓取批次）收口**（§4.3）⇒ ⇒ **本轮不需要 izzy 选**；**仅当**需要在壳内复现字段面时才重新启用这个问题 | —— |

> **不扩错误码、不改 P1 §3 的 13 码闭集、不改 P1 §4.1 的用户可见保留期承诺**。

---

## 10. 验收口径（判据清单，**已立 9 条**）

> **判据单已立，共 9 条**（PM 立，2026-10-21）⇒ ⇒ **实现开工前按这 9 条验收**。**判据脚本归成员域（`scripts/**`），PM 不替改** ⇒ ⇒ **PM 复跑验收**（口径：**自测全绿** ＋ 判据末尾「**扫了 N 个文件**」与扫描面文件数**对得上** —— 对不上就是**假绿**，一律退回）。
> **第 8 条与第 9 条的判据脚本已入库**（`scripts/check-official-chat-p4-credential.mjs` ＋ `.test.mjs`）⇒ ⇒ **第 9 条的 9d「通道形态」恒 `skipped` 占位**，等 §7.1 的通道形态落地后再补 ⇒ ⇒ **不许自行发明形态**。

1. **取数路线**：实现里**存在**「页内发起对站点自己 API 的同源请求」这一步（且**复用站点自己的 XHR 逻辑**，§2.3）⇒ **不存在**「页外直接调站点 API」的代码路径（**按源码形态断**，不许靠运行时）。
2. **零痕迹**：页内 `getOwnPropertyNames(window)` 前后逐项一致、`__dsh*` 为 0；**允许的痕迹形态 ＝ 替换 `XMLHttpRequest.prototype` 的 `open`/`send` ＋ 取到即还原**（§7.1）⇒ ⇒ **「不新增 `window` 自有属性」这一条机检断**（「还原后逐项一致」标【推定】，实现须自证还原彻底）。
3. **回程只回结构**：回程载荷里**没有**正文 / 标题 / 会话 id / 凭据 ⇒ **按回传字段清单断**（白名单之外即红）。
4. **接线**：走**现役**回环桥的一条**新路由**（不是新监听端口）⇒ 按源码形态断。
5. **能力面**：该路由**只对 official-chat 的 webview** 求值、**不接受任意 JS**、**不写站点数据** ⇒ 按源码形态断。
6. **错误面**：不新增第 14 个码（13 码闭集不动）⇒ 按常量闭集断。
7. **两类错误分离**：回程失败 ≠ 站点返回空 ⇒ 两个不同的既有码。
8. **字段面**（§4.3 已定死）：按**实测的键名**断 —— 数组键名 ＝ `data.biz_data.chat_sessions`；条目 ＝ 6 键（`id`／`title`／`title_type`／`model_type`／`pinned`／`updated_at`）；**条数从 `data.biz_data.chat_sessions.length` 读**（**不是**从 `has_more` 读）；**成功判据 ＝ `status==200` 且顶层 `code==0` 且 `data.biz_data` 非空** ⇒ ⚠️ **不断言绝对条数**、**不断 `biz_data` 键集合相等**（成员的设计修正，PM 采纳）。
9. **凭据机制（v0.7 新增，三条否定式 ＋ 一条占位）** ——
   · **9a｜不自拼凭据头**：实现里**不存在**任何把站点凭据拼进请求头的代码路径 ⇒ **按源码形态断**。
   · **9b｜不读站点凭据**：实现**不读** `localStorage`／`document.cookie` 里的站点凭据 ⇒ **按源码形态断**。
   · **9c｜页外不调站点 API**：实现**不存在**从页外直连 `chat.deepseek.com/api/v0/*` 的代码路径 ⇒ **按源码形态断**。
   · **9d｜通道形态**：**恒占位 `skipped`** —— 形态随 §7.1 的裁定落地后再定 ⇒ ⇒ **不许自行发明形态**。

---

## 11. 变更记录

| 版本 | 时间 | 内容 |
| --- | --- | --- |
| v0.1 | 2026-10-21 12:5x（PM） | **草案首版**。把**已实测**的事实写死成条款：① 取数路线三条（§1）② 页内求值机制 ＋ 三条纪律 ＋ 零痕迹（§2）③ 页内同源 fetch ＋ **页外调 API 必失败已升级为实测** ＋ **40003 不可解释成「没登录」**（§3）④ 端点表 ＋ **翻页游标名 `lte_cursor`** ＋ 响应三层骨架 ＋ 静态包 grep 不足（§4.1/4.2）⑤ 接线＝现役回环桥加一条路由 ＋ 三条硬约束（§5）⑥ 不扩 13 码闭集 ＋ 站内码只原样上报 ＋ 两类错误分离（§6）⑦ 零痕迹 / 凭据零落盘 / 只读（§7）⑧ 隔离纪律 ＋ **「只读拷贝带不带登录态」仍未验到**（§8）⑨ 缺口登记 G4-1…G4-5（§9）⑩ 判据清单（§10）。**唯一待写死点**＝§4.3（字段面 ＋ 三档码），补法已备好（只读 F12 脚本，在台账 12:1x 节 ⑤）。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.2 | 2026-10-21 13:0x（PM） | **补三条本轮新实测的硬约束** ＋ **G4-4 收口** ＋ **取证落点订正**。① **§2.5 新增（裁定 2）＝ 登录态机制** —— 登录态**只存在于「同一个活着的浏览器上下文」**（`ds_session_id` 是 **session cookie** ⇒ **新进程不恢复非持久 cookie** ⇒ **拷过来也不带登录态**；且盘上明文 JWT 虽有效到 2026-11-30 却不被认）⇒ ⇒ 「**执行体在壳**」「**API 在页内**」由机制决定，是**硬约束**、**不是偏好** ⇒ **实现方不得提「页外调 API」「从外部进程读」** ⇒ **拷贝取证路线正式关闭**；② **§5.3 新增（裁定 3）＝ 外挂 CDP 不可行** —— 四项实测（活着的 `msedgewebview2` **无** `--remote-debugging-port` / 两棵 profile 根目录**无** `DevToolsActivePort` / **无**监听端口 / release 树 `Cookies` **EBUSY**）⇒ **只有壳能求值**（`CallDevToolsProtocolMethod` 是 `ICoreWebView2` 自己的方法、进程内调用）⇒ 与 ① 合起来 ⇒ **「壳内求值」是唯一路径**；③ **§8.4 改写 ＋ §8.5 新增 ＋ §8.6 新增** —— 「只读拷贝能否带登录态」由「**仍未验到**」**收口为「不能」**（机理＝**session cookie 跨进程不可恢复**）；**取证落点订正**（壳的官网对话窗口是 `with_devtools(false)` ⇒ **壳里按 F12 弹不出 DevTools** ⇒ PM 上一轮给 izzy 的落点**走不通**，已更正）；**两条落点 A（平时用的浏览器跑只读脚本｜零改动零授权）/ B（dev 壳内一次性只读形状探测｜需重新登录 ＋ 一次授权）** ＋ **G4-6**（平时用的浏览器里是否有 DeepSeek 登录态 ⇒ **待 izzy 确认**）。**版本线顺延**：v0.2 ＝ 补硬约束的草案稿 ⇒ **v0.3 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.2 为冻结稿）。**§4.3 仍是唯一待写死点**（四项字段面 ＋ 三档错误码）。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.7 | 2026-10-21 15:3x（PM） | **冻结稿**（§4.3 收口 ＋ 五处依据改写 ＋ 判据立 9 条）。① **§4.3 整节替换为「已写死」** —— 四项全部有实测出处：**① 数组键名 ＝ `data.biz_data.chat_sessions`（双源一致）② 条目 6 键**（`id`／`title`／`title_type`／`model_type`／`pinned`／`updated_at`，**双源一致**）**③ 条数从 `data.biz_data.chat_sessions.length` 读**（`has_more` 类型 ＝ **boolean** ＝「还有更多」标志、**不是条数**）**④ 响应体三处皆无游标键 ⇒ 游标在请求 query**（`lte_cursor` 对象参数、**点号路径**、值取自条目 `updated_at`）；**「每页 100 条」≠「`count` 上限 100」** ② **§3.1 整表改写** —— 「同源 ＋ 自动带 cookie 就不需要拼凭据」的**依据换成登录态裸档被拒**（`users/current` → `40002`；`fetch_page` → `40003`）；**请求头名清单收窄为 11 个并集**（**`x-ds-compression`／`x-request-encryption-key` 未出现在普通 `/api/v0/` 请求头里**）；**头不是每端点都带**（`banner` scope 不带 `authorization`；`x-settings-token` 只在部分 `client/settings` scope 带）；**实测 `/api/v0/*` 全部走 XHR、不走 fetch** ③ **§4.1 翻页请求行订正为点号语法** ＋ 追加实测端点面（12 条响应／9 个端点形态）与实测响应键名表 ④ **§4.2 成功判据写死**（`status==200` 且顶层 `code==0` 且 `data.biz_data` 非空）＋ 正例（`code=0` ＋ `biz_code=0` ＋ `biz_data` 非 null）＋ 反例补端点/登录态/复现次数 ⑤ **§6.2 三档码由「待实测」改为「已实测」**（未登录态 `40002`／`40003` ＋ 登录态 6 条全拒 ＋ 成功档形态）⑥ **§7.1 新增「允许的页内痕迹形态」裁定**（替换 `XMLHttpRequest.prototype` 的 `open`/`send` ＋ 取到即还原；「还原后逐项一致」标【推定】）⑦ **§7.2 性质由「技术保证」改为「纯纪律」**（`authorization` 裸值 64 字符是 `localStorage.userToken` 92 字符的**子串** ⇒ 技术上可自拼 ⇒ 靠纪律 ＋ 判据 9a/9b 挡）⑧ **§1 裁定 1 与 §2.3 页内脚本形态改写**（「同源 fetch ＋ 自动带 cookie」不成立 ⇒ 必须**复用站点自己的 XHR 逻辑**）⑨ **§8.5／§8.6 换道**（页内钩子探法**时序不可达**、结案 ⇒ 实际落点 ＝ CDP 抓取批次 ⇒ 字段面已收口 ⇒ B 路本轮不需 izzy 选）⑩ **§10 判据立 9 条**（第 8 条按 §4.3 可执行化 ＋ **新增第 9 条**：9a 不自拼凭据头／9b 不读站点凭据／9c 页外不调站点 API／9d 通道形态恒占位）⑪ **§9 缺口收口**（G4-2、G4-3 已收口；G4-7 已消解）。**版本线第五次顺延**：v0.7 ＝ 补齐字段面后的**冻结稿**（原定 v0.3 为冻结稿）⇒ ⇒ **本轮不再顺延**。**未动**：P1／P2／P3 协议、13 码闭集、上游内核 |
| v0.6 | 2026-10-21 13:4x（PM） | **补两条口径**（**成员提、PM 一手核过后落**）。① **§8.5 补「为什么 dev 壳那档只能写【推定】」** —— **wry 0.57 自己不绑 F12**：全仓 grep 不到任何 `F12`／`AcceleratorKeyPressed` 处理（加速键相关的**只有** `browser_accelerator_keys` 那个开关、且默认 `true`）⇒ ⇒ **F12 能否触达完全取决于 WebView2 自身行为**，不是 wry 提供的 ⇒ ⇒ **「【推定】未实测」这个标注准确，不宜写成「能开」** ② **§8.6 给 B2 补归属与成本** —— 装那个一次性入口意味着**改动 `src-tauri/**` ＋ 重新构建 ＋ 装到用户真实那棵树** ⇒ ⇒ **属于「动用户安装版」的动作 ⇒ 归 izzy 自己拍板、自己测**（**成员的分工只到 dev**）⇒ ⇒ **协议与派单里不许把 B2 写成「零成本」** ③ **P0 §7 的旧依据句首已改写**（不再以 `with_devtools(false)`（`lib.rs:455-456`）起句，改成「**release 壳里 DevTools 被 wry 关掉**」并把订正说明压进去）④ **版本线第四次顺延**：v0.6 ＝ 补口径 ⇒ **v0.7 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿）。**§4.3 仍是唯一待写死点**。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.5 | 2026-10-21 13:3x（PM） | **G4-6 收口（PM 一手核完浏览器里的登录态）** ＋ **落点收窄为「Chrome」**。① **PM 一手只读核查（不解 cookie 值、不写用户目录、不启动浏览器）** —— **Chrome（Default profile）有登录痕迹**：`localStorage` 的 leveldb 里 **`userToken` ＋ `__appKit_userInfo` 都在**，且 `userToken` 与 origin 字符串落在**同一条记录**（偏移相距 **155 字节**）⇒ ⇒ **曾登录且未清理**；leveldb 最新 mtime **2026-10-21 12:08:39**。**Edge 排除**：只有 **14 处** `chat.deepseek.com` 访问痕迹、**没有** `userToken`／`__appKit_userInfo`，leveldb 最新 mtime **2026-10-16 09:27:37**；且 Edge 的 `Cookies` 里 `chat.deepseek.com` **只有两个匿名项**（`.thumbcache_…`／`smidV2`）⇒ **无登录 cookie**。② **Chrome 的 `Cookies` 表读不到**（被独占锁：copy 与只读 open 都 `WinError 32`）⇒ ⇒ **「痕迹在」≠「此刻 session 仍有效」** ⇒ ⇒ **最后一步仍要 izzy 跑一次脚本**（§8.6 A 路）。③ **落点由「Edge / Chrome」收窄为「Chrome」**（§4.3 补法 ＋ §8.5 结论 ＋ §8.6 表格同步）④ **G4-6 由「待 izzy 确认」改为「已收口」**；**G4-7（B1/B2）保留为条件项**（**只在 A 路失效时才需要问**）⑤ **版本线顺延**：v0.5 ＝ G4-6 收口＋落点收窄 ⇒ **v0.6 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿，**三次顺延**）。**§4.3 仍是唯一待写死点**。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.4 | 2026-10-21 13:2x（PM） | **防误读条款** ＋ **端点来源标注**。① **§4.2 新增「顶层 `code=0` ≠ 取到数据」** —— 实跑读数补 **`biz_code=2`** ＋ **`biz_data` 的类型读数是 `null`** ⇒ **那组读数本身就是反例**（`status=200` ＋ **顶层 `code=0`** ＋ `msg=""` 看着像成功，**同时** `biz_code=2`、`biz_data=null`）⇒ ⇒ **传输层成功 ≠ 业务层成功 ≠ 拿到了数据** ⇒ ⇒ **判「成功」不能只看顶层 `code`** ⇒ **成功判据必须落到 `data.biz_data` 是否有内容**（具体判据等 §4.3 一并写死）⇒ ⇒ **别把 `code=0` 直接映射成「成功」** ② **§6.2 补「站内码有**两层**」** —— 顶层 `code` 与 `data.biz_code` ⇒ **报哪一层必须写死**；已实测：**未登录态**打 `fetch_page` 是**顶层 `code=40003`**（`data` 为 `null`）⇒ ⇒ **「成功」那一档不能落在顶层 `code=0`** ⇒ **成功判据落在 `data.biz_data` 是否有内容** ③ **§4.1 补来源标注与第二种口径** —— 上表那 5 个端点来自**站点自己浏览器缓存里的完整请求行**（Chromium 缓存条目 `data_1` 的 key 字段）⇒ **不是「页面当场请求」的记录** ⇒ ⇒ **登录态下的 resource timing 端点清单仍未实测**；另**运行时 resource timing**（**未登录态**）＝ 资源总数 **23** 个、其中 `/api/` 端点**只有 1 个**（`/api/v0/client/settings`，query 只回**键名** `["did","scope"]`）⇒ ⇒ **两种非静态口径都只看到端点路径、看不到字段面** ⇒ **字段面仍只能靠登录态下的结构实测**（§4.3）。**版本线顺延**：v0.4 ＝ 防误读条款 ⇒ **v0.5 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿，**两次顺延**）。**§4.3 仍是唯一待写死点**。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
| v0.3 | 2026-10-21 13:1x（PM） | **依据订正** ＋ **B 路补齐** ＋ **取证脚本换两档版**。① **§8.5 的依据整段换掉（PM 第二次自纠）** —— 旧依据「官方对话窗口建的时候是 `with_devtools(false)`（`src-tauri/src/lib.rs:455-456`）」**错**：那段讲的是 **`main` 管理窗口**、且它自己就 `#[cfg(debug_assertions)]` 门控 ⇒ ⇒ 换成**一手核过的依据链**：wry 的 `devtools` 属性默认 **debug ⇒ `true` / release ⇒ `false`**（`wry-0.57.0/src/lib.rs:872-875`）＋ Tauri **只在** `#[cfg(any(debug_assertions, feature = "devtools"))]` 下才覆盖它（`tauri-runtime-wry-2.12.1/src/lib.rs:5146-5149`）＋ 本仓 `src-tauri/Cargo.toml:30` **无 `devtools` feature** ⇒ ⇒ `SetAreDevToolsEnabled(false)`（`wry-0.57.0/src/webview2/mod.rs:640`）⇒ ⇒ **release 壳里 F12 无效、右键无「检查」**；另补两条：**DOM 层右键菜单被 `no-context-menu.js` 取消**（捕获阶段 `preventDefault()` ＋ `stopImmediatePropagation()`）＋ **官网对话窗口没有程序化 `open_devtools()` 入口**（`official_chat/context.rs:104-113`；全仓 `open_devtools()` 三处分别给工作台与 `main`，都 `#[cfg(debug_assertions)]`）② **撤回一条被引用的依据**：「**wry 关掉了浏览器加速键 ⇒ F12 不生效**」**不成立** —— `SetAreBrowserAcceleratorKeysEnabled(false)` 是**有条件的**（`if !pl_attrs.browser_accelerator_keys`），而 wry 默认 **`browser_accelerator_keys: true`**（`lib.rs:1768`，注释「This is WebView2's default behavior」）⇒ 本仓**未**调用 `with_browser_accelerator_keys(false)` ⇒ ⇒ **加速键是开着的** ⇒ **结论（落点不是壳）不变、依据换**；**教训**：「某能力不存在」要先找**真正控制它的那个开关**，别拿邻近开关的代码当依据 ③ **落点分两档写准**：**release 壳** ＝ DevTools 被关（**真**）；**dev 壳** ＝ DevTools **可能能开**（**【推定】，未实测**）**但 dev 树的登录态已随环境关闭失效** ⇒ ⇒ **两档都不可用**，且 dev 壳的理由**与 DevTools 无关** ④ **§8.6 补 B 路的两条关键事实**：**B 路不需要 DevTools**（壳自己的 CDP `Runtime.evaluate` 是进程内调用，P0 已实测「不开 DevTools 窗口也能调」）⇒ ⇒ **B 路在 release 壳里成立、不用重新登录**；并拆成 **B1（dev 壳：重新登录一次、不碰安装版）/ B2（release 壳：不用重新登录、但要动真实树）** 供 izzy 选 ⑤ **§4.3 补法换成「两档自动降级」脚本**（第 1 档按裸/＋Bearer/＋Bearer＋`x-device-id` 三种头反推哪档被认 ＋ 第 2 档钩**站点自己**的请求、用 `res.clone()` **不吃掉站点响应体**）＋ **写清痕迹范围**（只在该标签页、关掉即消失、**不写站点数据**）＋ **第 2 档触发动作建议用「新对话」**（不加载会话正文）＋ **PM 已逐字段核过输出面**（只回结构，不回正文/标题/会话 id/凭据）。**版本线顺延**：v0.3 ＝ 依据订正稿 ⇒ **v0.4 ＝ 补齐 §4.3 字段面后的冻结稿**（原定 v0.3 为冻结稿）。**§4.3 仍是唯一待写死点**。**未动**：P1 / P2 / P3 协议、13 码闭集、上游内核 |
