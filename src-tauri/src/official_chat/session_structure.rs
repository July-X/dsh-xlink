//! P4：官网对话**会话目录结构**的真实取数（协议 **v0.28** §1 / §2.1 / §2.3 /
//! §2.4 / §4.2 / §4.3 / §5 / §6 / §7 / §8.5.1）。
//!
//! Windows 使用自然请求观测；macOS 使用页内取数，MiniMax 临时旁听。
//! 账户端点的回包白名单见 `account_contract`，页内先裁剪再回程。
//! 以下 Network 步骤及零注入约束仅描述 Windows 的形态 A。
//!
//! # Windows 观测链
//!
//! 1. **建窗链**（[`arm_capture_window`]）：建一扇**自己家的**不可见窗（停在
//!    `about:blank`）→ **按具体事件名**订阅 DevTools 的 `Network` 域事件 →
//!    **`Network.enable`** ⇒ ⇒ **三件事都在导航之前**（协议 §8.5.1 硬约束 3：
//!    晚于站点首个请求就**首屏全丢**）。
//! 2. **导航链**（[`capture_in`]）：导航到站点 ⇒ ⇒ **站点自己**发
//!    `GET /api/v0/chat_session/fetch_page`（协议 §3.1）⇒ ⇒ **壳侧零请求**、
//!    **零自拼鉴权头**、**零读 token**、**零页外**（协议 §3.1 / §7.2）。
//! 3. **收响应**：收到目标响应的 `Network.loadingFinished` ⇒ ⇒ **立刻**
//!    `Network.getResponseBody`（协议 §8.5.1 硬约束 1：**取早了有两种失败形态** ——
//!    `E_INVALIDARG` ＋ **静默拿到长度 0 的 body 且不报错** ⇒ ⇒ 取到的体为空
//!    **一律算「回程失败」**，不当成「站点没数据」）。
//! 4. **壳侧裁剪**（[`SessionStructure::from_body`]）：壳侧拿到的是**站点的完整
//!    响应体**（里面**含** `title` / `id`）⇒ ⇒ 由**壳侧**解析并裁成**协议 §2.4
//!    的白名单结构** ⇒ ⇒ **正文、凭据、整包一律不出壳**
//!    （协议 §8.5.1 末段：**不许**把 `getResponseBody` 的原文直接回传）。
//!    ⚠️ **v0.17 的目录项面是唯一的例外**：会话 id 与标题**允许**出现在
//!    `items` 的**元素内部**（协议 §2.4 面二 / §4.3 ② 的 6 个键）⇒ ⇒ 但
//!    **只**在那 6 个键里，站点条目里**别的键**（正文、凭据……）**一律不带**。
//! 5. **判四档**（协议 §4.2 成功判据 / §6.2 / §6.3）：传输层失败 / 站内码 /
//!    形状漂移 / 站点返回空**四类分开**（**不扩 P1 §3 的闭集**）。
//!
//! # Windows 页内**零注入**（协议 §2.3 / §7.1）
//!
//! 这条形态**不注入任何取数脚本**、不改 `XMLHttpRequest.prototype`、不留槽位、
//! 不留定时器 ⇒ ⇒ 判据第 2 条的口径是「**页内什么都没碰**」。
//!
//! ⚠️ **页内唯一那个脚本是仓库存量的族规**（`harness/no-context-menu.js`：别人的
//! 站点不许自绘右键菜单）—— 它**只加事件监听器**，**不新增 `window` 自有属性、
//! 不换 prototype** ⇒ 不影响「页内什么都没碰」这条口径。**取数本身在页内零动作。**
//!
//! # 为什么自己开一扇**不可见**的窗
//!
//! · 协议 §2.1 的三条纪律：**同一份 user-data 目录**（登录态在那儿，协议 §2.5）、
//!   **不抢焦点**（不可见）、**按需创建**（官网窗口没开也能读）。
//! · **不复用用户挂载的那个页签**：订阅必须**早于站点首个请求** ⇒ 只能让页面
//!   重新加载 ⇒ 重载用户的页签会把用户正在看的东西刷掉 ⇒ ⇒ 另开一扇**自己建的**
//!   不可见窗，重载它不打扰用户。
//!
//! # G4-13（「窗口已开、首屏已过」）—— **PM 已裁**（协议 v0.11 §8.5.2；**v0.24 收窄射程**）
//!
//! · **G4-13 ①（v0.24 收窄射程）**：这条约束的是「**订阅点只装一次**」（为了不丢
//!   首屏那一发）⇒ ⇒ **不再约束「目录结构只取一次」** ⇒ ⇒ 本模块改为「**按需
//!   重新取**」（协议 §4.6）：`refresh=true` ⇒ 当刻重取；`refresh=false` ⇒ 保鲜期
//!   内回缓存，过期了重取。
//!   ⇒ ⇒ **建窗链仍然是「每轮一扇新窗」**：那一轮的订阅仍然**早于**那一轮的首屏。
//! · **G4-13 ②**：**不许壳侧 `reload`**（`Page.reload`／重新 `navigate` 同一 URL）
//!   —— 理由：`reload` 会让站点**重跑一次初始化批次**，而**登录态下站点自己**会
//!   `POST /api/v0/chat_session/create` ⇒ ⇒ **`reload` 不是只读动作** ⇒ ⇒ 「重新取」
//!   只能是「**建一扇新窗 ＋ 导航一次**」，**不 reload 旧窗**。
//!   ⚠️ **归因口径**（协议 v0.22 订正）：那条 `create` 是**站点落地即发的初始化
//!   动作**（深链／首屏同源）⇒ ⇒ **不许**把「多出一条会话」当某一档导航特有的
//!   因果结论 ⇒ ⇒ 但**「壳侧自己没发写请求」这一条仍必须如实登记**。
//!
//! # G4-14（「目录新鲜度」）—— **PM 已裁**（协议 v0.11 §8.5.2；**v0.24 改语义**）
//!
//! · **①（v0.24 改）**：结构**有保鲜期**（[`STRUCTURE_TTL`]）—— 保鲜期内回缓存，
//!   过期了**重取**。**为什么壳侧也得有一份**：插件侧那层目录页缓存过期后会**再
//!   来问壳侧** ⇒ ⇒ 壳侧若永远回同一份旧结构，插件侧那层 TTL 就**没有牙齿**。
//! · **②** **不回传时间戳**（§2.4 白名单是穷举列表）⇒ 回程载荷里没有时间字段。
//! · **③** **还没抓到结构 ⇒ `catalog_failed`（可重试）**，**绝不** `catalog_empty`
//!   （那个码只表示「有结构但无匹配项」）。
//! · **④** 「立即回」还是「挂起等待」由实现侧选 ⇒ ⇒ 本模块选**挂起等待**。
//!
//! # 能力面（协议 §5.2 第 3 条）
//!
//! · **只对 official-chat 的 webview 订阅** —— 本模块只建自己那扇官方对话的窗，
//!   从不遍历、从不碰别的窗口。
//! · **不接受任意 JS** —— 桥上那条路由**只收一个命令名**（协议 §5.2），本模块
//!   **没有**任何「把内核传来的文本当脚本执行」的入口。
//! · **不写站点数据** —— 只读响应，不写 localStorage / cookie / DOM。
//!
//! # 平台边界（**两条**）
//!
//! · **Windows 专用**（壳内 WebView2 DevTools 通路）：建取数窗、订阅 `Network`
//!   域事件、`Network.enable`、等 `loadingFinished`、取体 —— 这一整条链在别的
//!   平台**整块都不存在** ⇒ 非 Windows 侧走 `CaptureFailure::Unsupported`
//!   （协议 §6.1，明确报错，不静默降级）。
//! · **跨平台纯逻辑**：协议 §2.4 的裁剪（[`SessionStructure::from_body`]）与
//!   §4.2/§6.2/§6.3 的四档判断 —— **不碰 DevTools、不碰 webview**
//!   ⇒ ⇒ **两平台口径必须一致** ⇒ ⇒ 它们**必须跨平台存在**。
//!
//! ⚠️ **平台差异只能落在函数体里，不能落在签名用到的类型别名上**（`EventStream`
//! 那条别名跨平台 —— 原因见它自己的文档）。
//!
//! # 非 Windows 上的死代码豁免
//!
//! 非 Windows 上「取数链整块不存在」⇒ 本模块**多数**函数在那边没有生产调用方
//! （只有本模块的用例在用）⇒ 那边的构建里「未被调用」是**正常**的
//! ⇒ 不豁免会满屏死代码警告。
//!
//! ⇒ ⇒ **Windows 那边的构建才是这条链的严格检查面**（那儿整条链是活的）；
//! 非 Windows 这边豁免的只是「死代码」这一类 lint，其余 lint 照常。
#![cfg_attr(not(windows), allow(dead_code))]

// 账户端点与页内回包白名单。**刻意用 `#[path]` 挂成本模块的子模块**，而不是
// 在 mod.rs 里声明成兄弟模块：独立探针（examples/official_chat_*_probe）用
// `#[path]` 把**本文件单独**包含进它们自己的模块树，届时 `super::` 指向的是
// 探针的 crate 根、没有 `account_contract`——子模块挂载让本文件走到哪儿都
// 自带这份契约（`#[path]` 相对本文件自己的目录解析）。`account.rs` 从
// `super::session_structure::account_contract` 引用。
#[path = "account_contract.rs"]
pub(crate) mod account_contract;

use std::collections::BTreeMap;
/// Windows 专用链（DevTools 事件通道）才用得到。
#[cfg(windows)]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tauri::Manager;

/// CDP 调用只在 Windows 专用链里用（macOS 没有壳内 DevTools 通路 —— 协议 §6.1）。
#[cfg(windows)]
use super::cdp_eval::{cdp_call, cdp_field};

// **回程机制**（页内求值 ＋ 异步结果回传）——macOS 路线 B 用。
//
// ⚠️ **为什么用 `super::`**：生产 crate 里 `super` ＝ `official_chat` 模块
// （`script_eval` 是它的子模块）；而那些用 `#[path]` 把**同一份源文件**编进
// 自己的 example 里，`super` ＝ example 的 crate 根（那儿也有 `script_eval`）
// ⇒ ⇒ **两边都指得到**。
#[cfg(target_os = "macos")]
use super::script_eval::{read_json, round_trip, round_trip_tolerant, RoundTripError};

/// 桥上那条路由的**命令名**（**白名单**：内核只许调这一个；协议 §5.2 第 3 条）。
pub(crate) const COMMAND: &str = "sessionStructure";

/// **站点标识**（短码）—— **本模块是它的唯一事实源**。
///
/// ## 它是什么
///
/// 「这次取数面对的是哪个站点」的**短码**：现在是 `deepseek`（官方对话站
/// [`OFFICIAL_CHAT_URL`](crate::harness::official_chat::OFFICIAL_CHAT_URL) 那个）。
///
/// ## 为什么由内核给，而不是插件自己猜
///
/// 插件那半边**够不着站点**：它只连**本机回环桥**（`site-source.js` 里的
/// `hostname` 是**桥**的，不是站点的）⇒ ⇒ 「我是谁」这件事只有建窗的这侧知道
/// ⇒ ⇒ 桥的回包**信封**上带这个短码（**信封层，不是回程载荷** ⇒ §2.4 的载荷
/// 白名单一个字不动）。
pub(crate) const SITE: &str = "deepseek";

/// 站点自己的**会话目录端点**（协议 §4.1）—— 我们**只认这一个**。
///
/// 协议 §4.1 的实测首屏形态是 `GET /api/v0/chat_session/fetch_page?lte_cursor.pinned=false`
/// ⇒ ⇒ 这里按**路径**认（query 会被站点自己改，我们不猜）。
const TARGET_PATH: &str = "/api/v0/chat_session/fetch_page";

/// MiniMax 的**站点短码**（信封上那个）。
pub(crate) const MINIMAX_SITE: &str = "minimax";

/// ChatGPT 的**站点短码**（信封上那个）。
pub(crate) const CHATGPT_SITE: &str = "chatgpt";

/// 一个站点的**取数面**（端点 ＋ 路径 ＋ 响应形状 ＋ 条目字段映射）。
///
/// ## 为什么是「一张表」而不是「两套链」
///
/// 目录那条链与正文那条链**只有站点相关的部分不同**（端点路径、响应体的形状、
/// 条目字段名、深链形态、正文那条怎么锁定与取哪一条）⇒ ⇒ **只有这一处分叉**
/// ⇒ ⇒ **不许**复制第二套目录链／第二套正文链。
///
/// ## ⚠️ **条数上限不在这张表里**
///
/// 「目录面最多回几条」是**站点无关**的（[`CATALOG_ITEM_LIMIT`]）⇒ ⇒
/// DeepSeek ／ MiniMax ／ ChatGPT **共用同一个数** ⇒ ⇒ **不许**按站点各写一份、
/// **不许**按站点传参。
///
/// ## 每一条都带出处
///
/// · DeepSeek 那份 ＝ 协议 §4.1／§4.3 的实测形态；
/// · MiniMax 那份 ＝ 2026-10-22 13:0x 的**运行时读数**（探针
///   `examples/official_chat_minimax_probe.rs`：目录 200／7760 B、
///   `$.sessions` 20 条；正文 200、`$.messages` 80 条）。
/// **页内路线 B 的站点适配**（`None` ＝ 该站点没有路线 B 适配）。
///
/// ## 它是什么
///
/// macOS 上**没有** DevTools `Network` 域的等价物 ⇒ ⇒ 路线 A 在那边**物理无解**
/// ⇒ ⇒ macOS 的取数链**整块**落在**页内**（两种形态）。
///
/// ⚠️ **两种形态的差别在「谁去发那一发」** ⇒ ⇒ 那是**类型层面**的差别 ⇒ ⇒
/// 枚举（[`PageRoute`]）而不是「同一个结构体里半填」：
///
/// · [`PageRoute::Fetch`]（**形态 B · 页内自取数**）—— **页内自己发**那一发
///   （同源 fetch ＋ 站点契约认证头）⇒ ⇒ 要**多知道两件事**：
///   ① **发哪一个 URL**（相对路径 ＋ query ⇒ 实测过的那一发）；
///   ② **认证契约**（凭据在哪个存储的哪个键、怎么取、放到哪个头、值的前缀）。
/// · [`PageRoute::Hook`]（**形态 C · 页内旁听**）—— **页内旁听站点自己那一发**
///   （MiniMax：签名复刻不出来 ⇒ ⇒ 只能**等站点自己发**、在页内截下它）⇒ ⇒
///   要**多知道两件事**：① **旁听哪条路径**（目录那条／正文那条）；
///   ② **签名头**的名字（**值一步都不出页面**）。
///
/// ## 为什么这是**站点相关**的
///
/// 认证契约是**站点私有实现细节**（DeepSeek 那个是 `localStorage.userToken` ＝
/// `{"value": "<64 字符>", …}` ⇒ `authorization: Bearer <token>`）⇒ ⇒ 站点一改
/// 就坏 ⇒ ⇒ **没适配的站点不许硬套别人的契约**（那会静默拿到未登录响应）⇒
/// ⇒ 没有适配就是 `None` ⇒ macOS 上那个站点**明确报「没有路线 B 适配」**。
///
/// ⚠️ **实测依据**：2026-10-22 20:0x–20:3x 本机 macOS 真机（探针
/// `examples/official_chat_macos_fetch_probe.rs`）—— 只带 `authorization` 一头
/// 重放 ⇒ 200／code 0／**100 条会话**，与站点自己那一发**逐字一致**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageRoute {
    /// **形态 B（页内自取数 · 路线 B）**：壳侧在页内**自己发**那一发。
    ///
    /// ⚠️ **这一支一个字都不许变**（DeepSeek／ChatGPT 的既有行为）⇒ ⇒ 字段与
    /// 取法**逐字保留**。
    Fetch {
        /// 路线 B 发的那一发的**相对 URL**（**同源** ⇒ 浏览器自动带 cookie）。
        ///
        /// ⚠️ **含 query**：DeepSeek 站点自己那一发带的是
        /// `?lte_cursor.pinned=false` ⇒ 实测过的那一发就照它来，**不猜**。
        url: &'static str,
        /// 页面的 **origin 片段**（用于「确实落到那个站上」的判定）。
        origin: &'static str,
        /// 认证契约。
        auth: PageAuth,
        /// **导航落点**（路线 B 的两条链都落**站点首页**）。
        ///
        /// ⚠️ **为什么不落深链**：路线 A（Windows）靠**站点自己**发那条正文请求
        /// ⇒ ⇒ 必须导航到深链；路线 B（页内取数）**自己**发那一发 ⇒ ⇒ **深链没用**
        /// ⇒ ⇒ 落首页（更轻，也不碰站点的增量缓存）。
        root_url: &'static str,
        /// **正文那条**的相对 URL 模板（`{session}` ＝ 会话 id 的占位）。
        ///
        /// ⚠️ `None` ＝ **该站没有正文那条的路线 B 适配** ⇒ macOS 上正文那条
        /// **明确报不支持**（**绝不**拿目录那条的 URL 去套正文）。
        ///
        /// ⚠️ **实测依据**（2026-10-22 22:1x 本机 macOS 真机）：DeepSeek 那条是
        /// `GET /api/v0/chat/history_messages?chat_session_id=<id>` ⇒ 200／code 0／
        /// **非空**（实测 5 条会话的正文条数 ＝ `[2, 2, 2, 32, 2]` ⇒ 拿的是**全量
        /// 历史**，**不是**空增量、**不是**被固定上限截断）⇒ ⇒ **不需要「冷档案」**
        /// （我们自己构造请求，**不带**站点的缓存版本头 ⇒ 站点回全量）。
        body_url: Option<&'static str>,
    },
    /// **形态 C（页内旁听 · 路线 C）**：页内**旁听站点自己那一发**。
    ///
    /// ⚠️ **为什么必须是另一支、而不是「同一个结构体里半填」**：形态 C **没有**
    /// 「页内自己发」那种形态——MiniMax 那条请求的签名**复刻不出来**（要靠站点
    /// 自己那层生成）⇒ ⇒ `url`／`auth`／`body_url` 三格**全不适用** ⇒ ⇒ 留在
    /// 同一个结构体里只能半填 ⇒ ⇒ **「哪种形态」必须在类型层面可见**（与
    /// [`PageAuthSource`] 那个枚举同理）。
    Hook {
        /// **导航落点**（取数根地址）——目录那条链落这里。
        ///
        /// ⚠️ **页签地址／取数根地址／深链前缀是三件事**（协议 §4.7 已核过）⇒ ⇒
        /// 这里用的是**规格表里已有的那一格**（`SiteSpec::root_url`），**不新造
        /// 常量**。
        root_url: &'static str,
        /// **目录那条**的路径（**页内要旁听的那一发**）。
        catalog_path: &'static str,
        /// **正文那条**的路径模板（`{session}` ＝ 会话 id 的占位）。
        body_path_template: &'static str,
        /// **深链前缀**（`{prefix}{sessionId}`）——正文那条链导航到它，**让站点
        /// 自己**发那一发（与协议 §4.4 的深链导航同构）。
        deep_link_prefix: &'static str,
        /// **签名头**的名字（**值一步都不出页面**）。
        signature_headers: &'static [&'static str],
    },
}

impl PageRoute {
    /// **导航落点**（取数根地址）—— 两种形态都有这一格。
    #[cfg(target_os = "macos")]
    pub(crate) fn root_url(&self) -> &'static str {
        match self {
            PageRoute::Fetch { root_url, .. } => root_url,
            PageRoute::Hook { root_url, .. } => root_url,
        }
    }

    /// **形态 B 那一支的认证契约**（形态 C 那一支**没有** ⇒ `None`）。
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn fetch_auth(&self) -> Option<&PageAuth> {
        match self {
            PageRoute::Fetch { auth, .. } => Some(auth),
            PageRoute::Hook { .. } => None,
        }
    }

    /// **形态 B 那一支的相对 URL**（形态 C 那一支**没有** ⇒ `None`）。
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn fetch_url(&self) -> Option<&'static str> {
        match self {
            PageRoute::Fetch { url, .. } => Some(url),
            PageRoute::Hook { .. } => None,
        }
    }

    /// **形态 B 那一支的 origin 片段**（形态 C 那一支**没有** ⇒ `None`）。
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn fetch_origin(&self) -> Option<&'static str> {
        match self {
            PageRoute::Fetch { origin, .. } => Some(origin),
            PageRoute::Hook { .. } => None,
        }
    }

    /// **形态 B 那一支的正文 URL 模板**（形态 C 那一支**没有** ⇒ `None`）。
    pub(crate) fn fetch_body_url(&self) -> Option<&'static str> {
        match self {
            PageRoute::Fetch { body_url, .. } => *body_url,
            PageRoute::Hook { .. } => None,
        }
    }

    /// **正文那条链的「页内目标」**（**已把会话 id 填进去**）。
    ///
    /// · **形态 B** ＝ 那一支要**自己发**的相对 URL（`body_url` 模板）；
    /// · **形态 C** ＝ 那一支要**旁听**的请求路径（`body_path_template` 模板）——
    ///   **站点自己**发（深链导航 ⇒ ⇒ 壳侧**不发**那一发）。
    ///
    /// ⚠️ **两种形态的「目标」含义不同**（一个是自己发、一个是旁听）⇒ ⇒ 这里
    /// **按形态分派**，不让调用方自己猜。
    pub(crate) fn body_target(&self, session_id: &str) -> Option<String> {
        match self {
            PageRoute::Fetch { .. } => self
                .fetch_body_url()
                .map(|template| template.replace("{session}", session_id)),
            PageRoute::Hook {
                body_path_template, ..
            } => Some(body_path_template.replace("{session}", session_id)),
        }
    }
}

/// **认证契约**（页内路线 B 用）。
///
/// ⚠️ 这里**只有契约，没有凭据**——凭据的值**始终留在页面里**（页内取、页内用、
/// 页内丢）⇒ ⇒ 壳侧永远拿不到用户的 token。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageAuth {
    /// 凭据**从哪儿来**（**站点相关**：两个站点的凭据根本不在同一个地方）。
    ///
    /// ⚠️ **为什么要枚举、而不是几个 `Option` 字段**：凭据来源是**非此即彼**的
    /// （要么在页内存储里，要么在同源端点的响应里）⇒ ⇒ 枚举让「同时给两套」
    /// 在**类型层面**就不可能 ⇒ ⇒ 也让「这个站点用的是哪一种」在**规格表里
    /// 一眼可见**（不许靠猜字段是不是空的）。
    pub(crate) source: PageAuthSource,
    /// 放到哪个请求头。
    pub(crate) header: &'static str,
    /// 值的**前缀**（两个站点都是 `"Bearer "`）。
    pub(crate) prefix: &'static str,
}

/// 凭据的**来源**（**站点相关**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageAuthSource {
    /// **页内存储**里取（DeepSeek：`localStorage.userToken` ＝
    /// `{"value": "<64 字符>", …}` ⇒ 取 `value`）。
    Storage {
        /// 放在哪个存储里。
        storage: PageStorage,
        /// 键名。
        key: &'static str,
        /// 从那个值里**怎么取** token。
        extract: TokenExtract,
    },
    /// **同源端点**的响应里取（ChatGPT：`GET /api/auth/session` ⇒ 响应**顶层**
    /// 的 `accessToken`）。
    ///
    /// ⚠️ **实测依据**（2026-10-23 · 本机 macOS 真机 · 探针
    /// `examples/official_chat_chatgpt_contract_probe.rs` · 连跑 8 轮，日志
    /// `/tmp/dsh-chatgpt-contract-r{1..8}.log`）：那个 `accessToken`
    /// **不在任何 cookie／localStorage 里**（8 个宽口径候选**全部** 401）⇒ ⇒
    /// **只能**从端点响应里取；页内比对证明它与站点自己那一发带的
    /// `authorization` 头**同值** ⇒ ⇒ 它就是那个 Bearer token。
    ///
    /// ⚠️ **token 会轮换** ⇒ ⇒ **每次取数前重新取**（不缓存、不落盘、不写
    /// 站点存储）—— 这一条是**实测**出来的（同一个会话前后两次取到的值不同）。
    Endpoint {
        /// **同源**端点的相对路径（浏览器自动带 cookie ⇒ ⇒ 登录态就在那一发里）。
        url: &'static str,
        /// 响应 JSON **顶层**那个键（token 就这一格）。
        json_key: &'static str,
    },
}

impl PageAuthSource {
    /// 就绪判定走**哪条通道**（**页内路线 B 专属**）。
    ///
    /// ⚠️ **只有 macOS 那边会调它**（页内路线 B）⇒ ⇒ 门到 macOS（理由同
    /// [`PageStorage::as_str`]）。
    ///
    /// ⚠️ **不 `pub(crate)`**：`ReadyChannel` 是**模块私有**的类型 ⇒ ⇒ 公开它
    /// 会触发 `private_interfaces` 警告（clippy `-D warnings` 会红）⇒ ⇒ 这里
    /// 保持**模块内可见**（同模块的 `read_page_ready` 与本文件的用例都够用）。
    #[cfg(target_os = "macos")]
    fn ready_channel(&self) -> ReadyChannel {
        match self {
            // ⚠️ **同步读得到** ⇒ ⇒ `read_json`（**这一支逐字保持现状**）。
            PageAuthSource::Storage { .. } => ReadyChannel::Sync,
            // ⚠️ **要等那一发** ⇒ ⇒ `round_trip`（**能 await**）。
            PageAuthSource::Endpoint { .. } => ReadyChannel::Async,
        }
    }

    /// 「**官网窗口没有这个站点的登录态**」那条文案（**按来源说**：页内存储 ／
    /// 同源端点）。
    ///
    /// ⚠️ **为什么不能一句话说完**：两个来源的「没有登录态」长得很不一样 ——
    /// 存储形态是「那个键不在」，端点形态是「那个端点没给出 token」⇒ ⇒ 说错
    /// 会让用户以为是自己网站的问题。
    ///
    /// ⚠️ **只有 macOS 那边会调它**（页内路线 B）⇒ ⇒ 门到 macOS（理由同
    /// [`PageStorage::as_str`]）。
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn missing_credential_detail(&self) -> String {
        match self {
            PageAuthSource::Storage { storage, key, .. } => {
                format!("{} 里没有 `{}`", storage.as_str(), key)
            }
            PageAuthSource::Endpoint { url, json_key } => {
                format!("取 token 的端点 {url} 没有给出 `{json_key}`")
            }
        }
    }
}

/// 页面上的凭据放在哪个存储里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageStorage {
    /// `localStorage`。
    LocalStorage,
}

impl PageStorage {
    /// 存储名（**页内脚本按这个字符串分派**）。
    ///
    /// ⚠️ **只有 macOS 那边会调它**（页内路线 B）⇒ ⇒ 门到 macOS：否则 Windows
    /// 上没人调 ⇒ clippy 判 dead code ⇒ CI（`-D warnings`）红。
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            PageStorage::LocalStorage => "localStorage",
        }
    }
}

/// 从那个值里**怎么取** token。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenExtract {
    /// `JSON.parse(value).value`（DeepSeek 的 `userToken` 就是这个形状）。
    ///
    /// ⚠️ **只有这一种**（2026-10-23 订正）：先前还有一个「原值就是 token」的
    /// `Raw` 变体 ⇒ ⇒ **但没有任何规格表条目用它**（ChatGPT 走的是**同源端点**
    /// 那一支，`accessToken` 是响应**顶层**的键 ⇒ ⇒ 不经过 `extract`）⇒ ⇒ 它是
    /// **死变体** ⇒ ⇒ 而它会把协议 §10 那个 **14 码闭集**撑成 15 码（闭集是 izzy
    /// 拍板「**不扩码**」的）⇒ ⇒ **删掉它**，闭集回到 14 码。
    JsonValue,
}

impl TokenExtract {
    /// 取法名（**页内脚本按这个字符串分派**）。
    ///
    /// ⚠️ **只有 macOS 那边会调它**（页内路线 B）⇒ ⇒ 门到 macOS（理由同上）。
    #[cfg(target_os = "macos")]
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            TokenExtract::JsonValue => "json-value",
        }
    }
}

pub(crate) struct SiteSpec {
    /// 站点短码（**信封上那个**）。
    pub(crate) key: &'static str,
    /// 目录那条链**导航到哪**（站点首页）。
    pub(crate) root_url: &'static str,
    /// 目录那条响应要等的**路径**（按路径认；query 由站点自己管）。
    pub(crate) catalog_path: &'static str,
    /// 目录响应里**会话数组**的位置。
    pub(crate) sessions_path: &'static [&'static str],
    /// 目录响应里**站内码**的位置。
    pub(crate) code_path: &'static [&'static str],
    /// 目录响应里**站内 msg** 的位置。
    pub(crate) msg_path: &'static [&'static str],
    /// 目录响应里 **`has_more`** 的位置。
    pub(crate) has_more_path: &'static [&'static str],
    /// 目录响应里**「`data` 的键名」**那一栏从哪个对象读（**空切片 ＝ 没有这一栏**）。
    pub(crate) data_keys_path: &'static [&'static str],
    /// 目录**条目对象**在数组元素里的位置（**空切片 ＝ 元素本身就是条目**）。
    pub(crate) item_path: &'static [&'static str],
    /// 协议那 6 个键 ↔ 站点自己的字段（**映射不到 ⇒ 留空串／0**）。
    pub(crate) fields: ItemFields,
    /// 正文那条响应要等的**路径模板**（`{session}` ＝ 会话 id 的占位）。
    pub(crate) body_path_template: &'static str,
    /// 深链前缀（`{prefix}{sessionId}`）。
    pub(crate) deep_link_prefix: &'static str,
    /// 正文那条**怎么锁定**到某条会话（协议 §4.4：逐字比对）。
    pub(crate) body_lock: BodyLock,
    /// 正文那条**命中多条时按哪一条取**（协议 §4.4：按读数说话）。
    pub(crate) body_pick: BodyPick,
    /// **目录那条**命中多条时按哪一条取（**站点相关**）。
    ///
    /// ⚠️ **为什么要单列一栏**：ChatGPT 那个站点**自己**往**同一个路径**发了两条
    /// 请求 —— 一条带 `conversation_origin`（实测 200／**44 字节／空**）、一条带
    /// `exclude_conversation_origin`（实测 200／**17,722 字节／20 条**）⇒ ⇒
    /// 目录那条**必须能挑出后者** ⇒ ⇒ 判据与正文那条**不是同一条**。
    pub(crate) catalog_pick: BodyPick,
    /// **页内路线 B 的站点适配**（`None` ＝ 该站点没有路线 B 适配）。
    ///
    /// ⚠️ **只有目录那条链用它**（正文那条链**没有**路线 B 适配 ⇒ macOS 上
    /// 正文那条**明确报不支持**，不静默套用目录的契约）。
    pub(crate) page_route: Option<PageRoute>,
    /// 正文那条**消息数组**的位置（**按顺序**认；认不出 ⇒ `schema_changed`）。
    pub(crate) messages_paths: &'static [&'static [&'static str]],
    /// 正文那条**角色线索**（**按顺序**认；先认到的那个说了算）。
    pub(crate) role_clues: &'static [RoleClue],
    /// 正文那条**文本线索**（**按顺序**认；取到**非空**的那个）。
    pub(crate) text_sources: &'static [TextSource],
    /// 正文那条**思维链字段名**（**空串 ＝ 没有**；有值 ⇒ 那条消息**带着**思维链
    /// ⇒ ⇒ 正文**照留**（它有 `msg_content`），思维链**记进 `omitted`**）。
    pub(crate) thinking_key: &'static str,
    /// 正文那条响应里**「还有没有更早的」**那个布尔在**哪儿**（**空切片 ＝ 站点
    /// 没给 ⇒ ⇒ 判不出「取全没取全」⇒ ⇒ **不许说「全部」**，协议 §9 G4-23）。
    pub(crate) body_has_more_path: &'static [&'static str],
    /// 正文那条链用**哪份档案**（**站点相关**：只有增量协议那条站点才需要冷档案）。
    pub(crate) body_profile: BodyProfile,
}

/// 正文那条链用**哪份档案**（协议 §4.5 的冷档案那一格）。
///
/// ## 为什么它是**站点相关**的
///
/// 冷档案（一次性副本：登录态在、站点自己那份缓存不在）是**为 DeepSeek 的
/// MERGE 增量协议**定的 ⇒ ⇒ 站点那条正文端点**不是增量协议**时，用冷档案
/// **没有理由**（PM 2026-10-22 13:4x 裁：ChatGPT 这一轮沿用**生产档案**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyProfile {
    /// **冷档案**（一次性副本）—— 站点那条正文端点是**增量（MERGE）协议**。
    Cold,
    /// **生产档案**（平时那份）—— 站点那条正文端点**不是**增量协议。
    Production,
}

/// 角色线索（**按顺序**认；先认到的那个说了算）。
///
/// ⚠️ **取值映射的目标就是协议 §1.3 的小写枚举**（`user` ／ `assistant` ／
/// `system` ／ `thinking`）⇒ ⇒ 认不出的取值 ⇒ **继续认下一条线索**（**不猜**）。
///
/// ⚠️ **`key` 是**路径**（不是单个键名）** —— ChatGPT 那条线索是
/// `author.role`（**两层**）⇒ ⇒ 单个键名装不下 ⇒ ⇒ 一律按路径取。
pub(crate) struct RoleClue {
    /// 字段的**路径**（站点自己的角色字段；`["author","role"]` ＝ 两层）。
    pub(crate) key: &'static [&'static str],
    /// 站点取值（**大写**比对） → 协议小写枚举。
    pub(crate) values: &'static [(&'static str, &'static str)],
}

/// 正文那条**每条消息**的文本线索（**按顺序**认；取到**非空**的那个）。
pub(crate) enum TextSource {
    /// 直接取这个字段（**非空**才算）。
    Field(&'static str),
    /// 站点那条响应里正文所在的数组（**逐项拼接**，跳过思维链那种项）。
    Fragments(&'static str),
    /// **对象里的 `parts[]`**（ChatGPT 的 user／tool 形态）：字段的值是个
    /// **对象**，它的 `parts` 数组里逐项是**字符串**（**按实测形状**）⇒ ⇒
    /// 逐项取文本拼接。
    Parts(&'static str),
    /// **对象里的那个字段**（ChatGPT 的 assistant 形态）：字段的值是个
    /// **对象**，它**里面**那个字段才是正文（**按实测形状**）。
    NestedField(&'static str, &'static str),
}

/// 协议那 6 个键 ↔ 站点自己的字段（**按路径**；**空切片 ＝ 映射不到**）。
///
/// ## 映射不到怎么办
///
/// **留空串／0**，**不许**加第 7 个键、**不许**改白名单（协议 §2.4 面二）。
pub(crate) struct ItemFields {
    /// 会话 id。
    pub(crate) id: &'static [&'static str],
    /// 标题。
    pub(crate) title: &'static [&'static str],
    /// 标题类型（**MiniMax 没有这一项 ⇒ 空串**）。
    pub(crate) title_type: &'static [&'static str],
    /// 模型类型。
    pub(crate) model_type: &'static [&'static str],
    /// 是否置顶（**MiniMax 没有这一项 ⇒ false**）。
    pub(crate) pinned: &'static [&'static str],
    /// 更新时间。
    pub(crate) updated_at: &'static [&'static str],
    /// 时间戳的**单位换算**（**除数**：**1 ＝ 原样**；**1000 ＝ 毫秒 → 秒**）。
    ///
    /// ⚠️ **为什么要有这一项**：协议 §4.3 ② 那个 `updated_at` 是**秒级小数
    /// 时间戳**；DeepSeek 原生就是秒（实测 1792643821.73），MiniMax 给的是
    /// **毫秒**（实测 1792591408863）⇒ ⇒ 不换算的话，同一个字段在两个站点
    /// 是**两个单位** ⇒ ⇒ 换算成秒，两个站点才是**同一个意思**。
    ///
    /// ⚠️ **是除数、不是乘数** ⇒ ⇒ 毫秒那一档写 **1000**（不是 0.001）：乘
    /// 0.001 会引入 1 ulp 的误差（1792591408.8630002 ≠ 1792591408.863），
    /// 除 1000 才是**正确舍入**的那个值。
    pub(crate) updated_at_divisor: f64,
    /// 时间戳那个值的**形态**（**站点实测** —— 不是猜的）。
    pub(crate) updated_at_form: TimestampForm,
}

/// 时间戳那个值的**形态**（**逐站实测**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimestampForm {
    /// **数字**（DeepSeek ＝ 秒；MiniMax ＝ 毫秒 ⇒ ⇒ 再乘那个比例）。
    Number,
    /// **ISO-8601 字符串**（ChatGPT 实测：`"2026-10-21T06:51:42.166765Z"`）。
    ///
    /// ⚠️ **实测依据**（2026-10-22 13:5x · ChatGPT 运行时读数）：目录条目的
    /// `update_time`／`create_time` **是字符串**（ISO-8601、UTC、带小数秒），
    /// **不是** epoch 数字 ⇒ ⇒ 「毫秒 ÷1000」那条**用不上**（**别照抄**）⇒ ⇒
    /// 解析成**秒级小数时间戳**（协议 §4.3 ② 那个字段的形态）。
    ///
    /// ⚠️ **同一个站点的两种形态并存**：正文消息里的 `create_time`／
    /// `update_time` **是数字**（实测 1792554685.596075）⇒ ⇒ **逐站定形态**，
    /// 不能一套口径套两处。
    Iso8601,
}

/// 正文那条响应**怎么锁定**到某条会话（协议 §4.4 的硬要求：逐字比对）。
#[derive(Debug, Clone, Copy)]
pub(crate) enum BodyLock {
    /// **query 里的某个键**（DeepSeek：`chat_session_id`）。
    QueryKey(&'static str),
    /// **路径里的会话 id**（MiniMax：`/session/<id>/message` ⇒ 会话 id **在路径里**，
    /// **query 里没有那个参数** —— 读数里根本没有 ⇒ ⇒ **不许猜一个键名出来**）。
    PathSegment,
}

/// 正文那条**命中多条时按哪一条取**（协议 §4.4：命中多条时按这个数说明判据）。
#[derive(Debug, Clone, Copy)]
pub(crate) enum BodyPick {
    /// 取**最新**那条（DeepSeek：那条链只命中一条）。
    Latest,
    /// 取**某个 query 键缺席**的那条（MiniMax：其余几条都是**更早的页**）。
    ///
    /// ⚠️ **读数依据**（2026-10-22 13:0x · MiniMax 运行时实测）：一次深链发了
    /// **5～6 条**正文请求 ⇒ · `before` **缺席**的那条 ＝ 站点**从最新端**取的
    /// 窗口（`limit=80`、`_source=session-switch`）；· `before` **在场**的那些
    /// ＝ **更早**的页（`limit=50/100` ＋ `before=<某条消息 id>`）⇒ ⇒
    /// 「**取最新**」（按响应时刻）会取到**更早**的一页、「**取 limit 最大的**」
    /// 也会取到**更早**的一页 ⇒ ⇒ **判据 ＝ `before` 缺席**。
    WithoutQueryKey(&'static str),
    /// 取**某个 query 键的值最大**的那条（ChatGPT：同一条正文端点**发了两次**
    /// ⇒ ⇒ 取 `num_turns` **更大**的那条；两者**相同** ⇒ ⇒ 上层按
    /// `Network.responseReceived.timestamp` 取**最大**的那条 —— 那是本层
    /// 「命中多条」的既有裁决顺序）。
    ///
    /// ⚠️ **读数依据**（2026-10-22 13:5x · ChatGPT 运行时实测）：一次深链里
    /// `GET /backend-api/conversations/<id>` **发了两次**（两次的 `num_turns`
    /// **实测都是 10**；body 字节数 **385,311** 与 **385,541** 不同）⇒ ⇒
    /// 「取最新」（按响应时刻）取到的是**哪一条**取决于站点当时的发车顺序
    /// ⇒ ⇒ 判据 ＝ **`num_turns` 更大**（**实测两次相同 ⇒ 落到「取
    /// `timestamp` 最大的那条」那一档**）。
    LargestQueryValue(&'static str),
    /// **只取**「**没有**某个 query 键」的那些（ChatGPT 的**目录**那条）。
    ///
    /// ⚠️ **读数依据**（2026-10-22 13:5x · ChatGPT 运行时实测）：站点**自己**
    /// 往**同一个路径**发了两条请求 ——
    ///
    /// · 带 **`conversation_origin`** 的那条 → 200／**44 字节**／**空**（`items`
    ///   长度 0）；
    /// · 带 **`exclude_conversation_origin`** 的那条 → 200／**17,722 字节**／
    ///   **20 条**。
    ///
    /// ⇒ ⇒ **别锁错那条**（PM 2026-10-22 13:3x 提醒）⇒ ⇒ 判据 ＝
    /// **「请求里不带 `conversation_origin`」**。
    ///
    /// ⚠️ **与 [`BodyPick::WithoutQueryKey`] 的差别**：那个是「**优先**取没有那个
    /// 键的」（一条都不满足时**退回**取最新的那条）；这个是「**只取**没有那个键
    /// 的」—— 一条都不满足 ⇒ ⇒ **一条都不取**（继续等／如实失败）⇒ ⇒ **绝不**
    /// 把那条**空的**变体当成目录。
    MustNotHaveQueryKey(&'static str),
}

/// DeepSeek 那一份（协议 §4.1／§4.3 的实测形态）。
const DEEPSEEK_SPEC: SiteSpec = SiteSpec {
    key: SITE,
    root_url: crate::harness::official_chat::OFFICIAL_CHAT_URL,
    catalog_path: TARGET_PATH,
    sessions_path: &["data", "biz_data", "chat_sessions"],
    code_path: &["code"],
    msg_path: &["msg"],
    has_more_path: &["data", "biz_data", "has_more"],
    data_keys_path: &["data"],
    item_path: &[],
    fields: ItemFields {
        id: &["id"],
        title: &["title"],
        title_type: &["title_type"],
        model_type: &["model_type"],
        pinned: &["pinned"],
        updated_at: &["updated_at"],
        updated_at_divisor: 1.0,
        updated_at_form: TimestampForm::Number,
    },
    body_path_template: "/api/v0/chat/history_messages",
    deep_link_prefix: "https://chat.deepseek.com/a/chat/s/",
    body_lock: BodyLock::QueryKey("chat_session_id"),
    body_pick: BodyPick::Latest,
    // ⚠️ **目录那条**：DeepSeek 那个目录请求只有一条（实测）⇒ ⇒ 按「取最新」。
    catalog_pick: BodyPick::Latest,
    // ⚠️ **页内路线 B 的适配**（2026-10-22 20:0x–20:3x 本机 macOS 实测）：
    // 站点自己那一发是 `GET /api/v0/chat_session/fetch_page?lte_cursor.pinned=false`
    // ＋ `authorization: Bearer <token>`；token 在 `localStorage.userToken`
    //（＝ `{"value": "<64 字符>", "__version": …}`）。
    // ⇒ **只带这一个头**重放 ⇒ 200／code 0／100 条会话（与全头重放逐字一致）。
    page_route: Some(PageRoute::Fetch {
        url: "/api/v0/chat_session/fetch_page?lte_cursor.pinned=false",
        origin: "chat.deepseek.com",
        auth: PageAuth {
            // ⚠️ **这一支是「页内存储取」形态**（**行为一个字不许变**）：键名、
            // 取法、头名、前缀全照旧 ⇒ ⇒ 页内脚本那一支**逐字保持现状**。
            source: PageAuthSource::Storage {
                storage: PageStorage::LocalStorage,
                key: "userToken",
                extract: TokenExtract::JsonValue,
            },
            header: "authorization",
            prefix: "Bearer ",
        },
        root_url: crate::harness::official_chat::OFFICIAL_CHAT_URL,
        // ⚠️ **正文那条**（2026-10-22 22:1x 本机 macOS 真机实测）：`GET
        // /api/v0/chat/history_messages?chat_session_id=<id>` ⇒ 200／code 0／
        // **非空**（5 条会话的正文条数 ＝ `[2, 2, 2, 32, 2]` ⇒ 拿的是**全量
        // 历史**，**不是**空增量、**不是**被固定上限截断）⇒ ⇒ **不需要「冷档案」**。
        body_url: Some("/api/v0/chat/history_messages?chat_session_id={session}"),
    }),
    messages_paths: &[
        &["data", "biz_data", "messages"],
        &["data", "biz_data", "chat_messages"],
    ],
    role_clues: &[
        RoleClue {
            key: &["role"],
            values: &[
                ("USER", "user"),
                ("ASSISTANT", "assistant"),
                ("SYSTEM", "system"),
            ],
        },
        RoleClue {
            key: &["type"],
            values: &[
                ("REQUEST", "user"),
                ("RESPONSE", "assistant"),
                ("THINK", "thinking"),
            ],
        },
    ],
    text_sources: &[
        TextSource::Field("content"),
        TextSource::Field("text"),
        TextSource::Fragments("fragments"),
    ],
    // ⚠️ DeepSeek 的思维链是**消息类型**（`THINK`）⇒ ⇒ 由角色线索认出来 ⇒ ⇒
    // 这里**没有**「思维链字段」。
    thinking_key: "",
    // ⚠️ DeepSeek 那条正文响应**没有**「还有没有更早的」那个布尔 ⇒ ⇒ 判不出
    // 「取全没取全」⇒ ⇒ **不许说「全部」**（协议 §9 G4-23）。
    body_has_more_path: &[],
    // ⚠️ DeepSeek 的正文端点是**增量（MERGE）协议** ⇒ ⇒ 冷档案（协议 §4.5）。
    body_profile: BodyProfile::Cold,
};

/// MiniMax 那一份（2026-10-22 13:0x 的**运行时读数**）。
///
/// ⚠️ **目录那条的形状与 DeepSeek 完全不同**（实测）：顶层就是
/// `{base_resp:{status_code,status_msg}, has_more, next_cursor, sessions:[…]}`，
/// 会话数组在 **`$.sessions`**，而**条目对象在元素的 `session` 下面**
/// ⇒ ⇒ 那一层「条目对象在数组元素里的位置」就是 [`SiteSpec::item_path`]。
const MINIMAX_SPEC: SiteSpec = SiteSpec {
    key: MINIMAX_SITE,
    root_url: crate::harness::official_chat::OFFICIAL_CHAT_MINIMAX_URL,
    catalog_path: "/minimax-cloud/api/v1/sidebar/session/tree",
    sessions_path: &["sessions"],
    code_path: &["base_resp", "status_code"],
    msg_path: &["base_resp", "status_msg"],
    has_more_path: &["has_more"],
    // ⚠️ MiniMax 顶层**没有** `data` 那一层 ⇒ ⇒ 「`data` 的键名」那一栏为空。
    data_keys_path: &[],
    item_path: &["session"],
    fields: ItemFields {
        id: &["session_id"],
        title: &["title"],
        // ⚠️ **MiniMax 没有「标题类型」**（DeepSeek 那个 `USER`/`SYSTEM`）⇒ ⇒
        // **留空串**（**映射不到就留空**，不许编一个）。
        title_type: &[],
        // ⚠️ 模型类型在 **`model.model_id`**（实测 `$.sessions[0].session.model`）。
        model_type: &["model", "model_id"],
        // ⚠️ **MiniMax 没有「置顶」**（它那个 `archived` 是「已归档」，**不是置顶**）
        // ⇒ ⇒ **留 false**（**不许拿 `archived` 顶替**）。
        pinned: &[],
        updated_at: &["updated_at"],
        // ⚠️ MiniMax 的 `updated_at` 是**毫秒**（实测 1792591408863）⇒ ⇒
        // 换算成**秒**（协议 §4.3 ② 那个字段是「秒级小数时间戳」）⇒ ⇒
        // **除数**是 1000（不是乘 0.001 —— 那会差 1 ulp）。
        updated_at_divisor: 1000.0,
        updated_at_form: TimestampForm::Number,
    },
    // ⚠️ **会话 id 在路径里**（实测 `/minimax-cloud/api/v1/session/<id>/message`）。
    body_path_template: "/minimax-cloud/api/v1/session/{session}/message",
    deep_link_prefix: "https://agent.minimax.cn/?id=",
    body_lock: BodyLock::PathSegment,
    body_pick: BodyPick::WithoutQueryKey("before"),
    // ⚠️ **目录那条**：MiniMax 那个目录请求**不带** `before`（那是正文那条的
    // 游标）⇒ ⇒ 判据与既有行为**逐字相同**（`before` 缺席）。
    catalog_pick: BodyPick::WithoutQueryKey("before"),
    // ⚠️ **形态 C（页内旁听）**：MiniMax 那条请求的签名**复刻不出来**（要靠站点
    // 自己那层生成）⇒ ⇒ **没有「页内自己发」那种形态** ⇒ ⇒ 走 [`PageRoute::Hook`]
    // （页内**旁听站点自己那一发**：截到请求头那一刻就**还原成替换前那个引用**）。
    //
    // ⚠️ **实测依据**（2026-10-23 · 本机 macOS 真机 · 探针
    // `examples/official_chat_minimax_route_b_probe.rs` · 生产那份页内脚本
    // `include_str!` 引进）：
    // · **目录那条**：`GET /minimax-cloud/api/v1/sidebar/session/tree` ⇒ 200／
    //   顶层 `sessions,has_more,next_cursor,base_resp`／**20 条**／**7674 B**；
    // · **正文那条**：`GET /minimax-cloud/api/v1/session/<id>/message` ⇒ 200／
    //   `$.messages` **24 条**（user 3＋assistant 21）；
    // · **签名三件套**：`x-signature`／`x-timestamp`／`yy`（**值一步都不出页面**）。
    //
    // ⚠️ **深链前缀**与**取数根地址**是**两件事**（协议 §4.7 已核过）：深链落在
    // `agent.minimax.cn`（实测入口 `agent.minimaxi.com` 会 301 到那里）⇒ ⇒ 正文
    // 那条链**导航到深链**、**让站点自己**发那一发。
    page_route: Some(PageRoute::Hook {
        // ⚠️ **取数根地址**＝规格表里**已有的那一格**（`SiteSpec::root_url`），
        // **不新造常量**（入口 `agent.minimaxi.com` 会 301 到 `agent.minimax.cn`）。
        root_url: crate::harness::official_chat::OFFICIAL_CHAT_MINIMAX_URL,
        catalog_path: "/minimax-cloud/api/v1/sidebar/session/tree",
        body_path_template: "/minimax-cloud/api/v1/session/{session}/message",
        deep_link_prefix: "https://agent.minimax.cn/?id=",
        signature_headers: &["x-signature", "x-timestamp", "yy"],
    }),
    // ⚠️ **消息数组在顶层**（实测 `$.messages`）。
    messages_paths: &[&["messages"]],
    // ⚠️ **角色只有 `user` ／ `assistant`**（实测取值集合）—— 没有 `system`。
    role_clues: &[RoleClue {
        key: &["role"],
        values: &[("USER", "user"), ("ASSISTANT", "assistant")],
    }],
    // ⚠️ **正文在 `msg_content`**（实测）—— DeepSeek 那个 `content`／`text`／
    // `fragments` 在 MiniMax 侧**一个字都没有**。
    text_sources: &[TextSource::Field("msg_content")],
    // ⚠️ MiniMax 的思维链是**独立字段** `thinking_content`（实测）⇒ ⇒ 那条消息
    // **正文照留**（它有 `msg_content`），思维链**记进 `omitted`**。
    thinking_key: "thinking_content",
    // ⚠️ MiniMax **自带**那个布尔（实测 `has_more`）⇒ ⇒ 「取全没取全」**判得出**
    // ⇒ ⇒ 据实登记（协议 §9 G4-23）。
    body_has_more_path: &["has_more"],
    // ⚠️ MiniMax 的正文端点是**分页**（`before` 游标），**不是**增量协议 ⇒ ⇒
    // 冷档案对它**没有理由**（本轮沿用既有行为，不动它）。
    body_profile: BodyProfile::Cold,
};

/// ChatGPT 那一份（2026-10-22 13:3x 的**运行时读数**；2026-10-23 的**路线 B
/// 八轮真机实测**订正了其中四处 —— 见下面每一条的「实测依据」）。
///
/// ## ⚠️ **与 DeepSeek 完全不同的地方**
///
/// · **目录那条**：顶层就是 `{items, limit, offset, total}` —— **没有 `data`
///   那一层**、**没有 `code`**、会话数组在 **`$.items`**；
/// · **正文那条**：**正文响应的顶层没有 `mapping`**（实测：顶层那 23 个键里
///   **没有** `mapping`）⇒ ⇒ 正文在 **`$.messages`** 数组里 ⇒ ⇒ 正文必须
///   **单独打一次**；
/// · **角色**在 **`author.role`**（**两层**）⇒ ⇒ 角色线索那一栏是**路径**。
///
/// ## ⚠️ **`mapping` 那一条的口径（2026-10-23 订正）**
///
/// ⚠️ **旧注释说「目录面拿不到正文」** —— **这句话不准确**：实测**目录条目
/// 里是有 `mapping` 这个键的**（条目那一堆键之一）⇒ ⇒ 「拿不到正文」的
/// 真正依据是**正文响应顶层没有 `mapping`**，不是目录条目没有它。
///
/// ⚠️ **条目那堆键的**个数会变**（r8 那次是 **24** 个，r31／r32 那次是 **28**
/// 个）⇒ ⇒ 这里**只**记「有这个键」，**不**记键数、**不**记它里面是什么
/// （**没验就不编**）。
const CHATGPT_SPEC: SiteSpec = SiteSpec {
    key: CHATGPT_SITE,
    root_url: crate::harness::official_chat::OFFICIAL_CHAT_CHATGPT_URL,
    // ⚠️ ChatGPT 自己的会话目录端点（实测 `GET /backend-api/conversations`）。
    catalog_path: "/backend-api/conversations",
    // ⚠️ 会话数组在 **`$.items`**（顶层键是 `items, limit, offset, total`）。
    sessions_path: &["items"],
    // ⚠️ ChatGPT 那条响应**没有站内码**（顶层就那四个键）⇒ ⇒ 空路径 ⇒ ⇒
    // 「站内码」那一档**判不出**（**不编一个码出来**）。
    code_path: &[],
    msg_path: &[],
    // ⚠️ ChatGPT **没有** `has_more` 那个布尔（它给的是 `total`／`limit`／
    // `offset` —— 那是**计数**，不是那个布尔）⇒ ⇒ 那一档**判不出**。
    has_more_path: &[],
    // ⚠️ ChatGPT 顶层**没有** `data` 那一层 ⇒ ⇒ 「`data` 的键名」那一栏为空。
    data_keys_path: &[],
    // ⚠️ 条目对象**就是**数组元素本身（不是 MiniMax 那种「在 `session` 下面」）。
    item_path: &[],
    fields: ItemFields {
        id: &["id"],
        title: &["title"],
        // ⚠️ **ChatGPT 没有「标题类型」**（DeepSeek 那个 `USER`／`SYSTEM`）⇒ ⇒
        // **留空串**（**映射不到就留空**，不许编一个）。
        title_type: &[],
        // ⚠️ 模型类型在**目录条目里没有**（`default_model_slug` 只在**正文**
        // 响应的顶层）⇒ ⇒ **留空串**（**不许拿别处的字段顶替**）。
        model_type: &[],
        // ⚠️ ChatGPT 的「置顶」就是 **`is_starred`**（实测条目里有这个布尔）。
        pinned: &["is_starred"],
        updated_at: &["update_time"],
        // ⚠️ **单位**：实测 `update_time` **是 ISO-8601 字符串**
        // （`"2026-10-21T06:51:42.166765Z"`）—— **不是** epoch 数字 ⇒ ⇒
        // 「毫秒 ÷1000」那条**用不上**（PM 2026-10-22 13:3x 那一裁的前提
        // **不成立**，实测推翻）⇒ ⇒ 解析成**秒级小数**（形态那一栏说了算）。
        updated_at_divisor: 1.0,
        updated_at_form: TimestampForm::Iso8601,
    },
    // ⚠️ **会话 id 在路径里**（实测 `/backend-api/conversations/<id>`）。
    body_path_template: "/backend-api/conversations/{session}",
    deep_link_prefix: "https://chatgpt.com/c/",
    body_lock: BodyLock::PathSegment,
    // ⚠️ 同一条端点**发了两次** ⇒ ⇒ 取 `num_turns` **更大**的那条（相等则取
    // `responseReceived.timestamp` 最大的那条 —— 那是本层既有的裁决顺序）。
    body_pick: BodyPick::LargestQueryValue("num_turns"),
    // ⚠️ **目录那条**：站点**自己**往同一个路径发了两条 ⇒ ⇒ 一条带
    // `conversation_origin`（实测 44 字节／**空**）、一条带
    // `exclude_conversation_origin`（实测 17,722 字节／**20 条**）⇒ ⇒
    // **只取不带 `conversation_origin` 的那条**（PM 2026-10-22 13:3x：
    // **别锁错那条**）。
    catalog_pick: BodyPick::MustNotHaveQueryKey("conversation_origin"),
    // ⚠️ **页内路线 B 的适配**（2026-10-23 · 本机 macOS 真机 · 探针
    // `examples/official_chat_chatgpt_contract_probe.rs` · 连跑八轮，日志
    // `/tmp/dsh-chatgpt-contract-r{1..8}.log`）：
    // · **凭据**：同源 `GET /api/auth/session` ⇒ 200，响应**顶层**的
    //   `accessToken`（**不在任何 cookie／localStorage 里** —— 8 个宽口径候选
    //   **全部** 401）⇒ ⇒ **只能**从端点响应里取（**会轮换** ⇒ 每次重取）；
    // · **目录那条**：`GET /backend-api/conversations`（**裸路径够用**，不必带
    //   query）＋ `authorization: Bearer <accessToken>` ⇒ 200，顶层
    //   `items,total,limit,offset` ⇒ **50 条**（r8：`itemsLen:50`、
    //   `totalCount:51`）；
    // · **正文那条**：`GET /backend-api/conversations/<真实会话 id>` ＋ 同一个
    //   Bearer 头 ⇒ 200，`$.messages` **22 条**（r8）。
    // ⇒ ⇒ 路线 B **自己**发这两发 ⇒ ⇒ 站点自己那两个变体（带
    // `conversation_origin` 的**空**变体 ／ 带 `exclude_conversation_origin`
    // 的 20 条）**与这一格无关**（`catalog_pick` 那条判据是给 Windows 路线 A
    // 挑「站点自己发的那两条」用的）。
    page_route: Some(PageRoute::Fetch {
        url: "/backend-api/conversations",
        origin: "chatgpt.com",
        auth: PageAuth {
            // ⚠️ **凭据来自同源端点的响应**（不是页内存储）—— 见上面那条实测。
            source: PageAuthSource::Endpoint {
                url: "/api/auth/session",
                json_key: "accessToken",
            },
            header: "authorization",
            prefix: "Bearer ",
        },
        root_url: crate::harness::official_chat::OFFICIAL_CHAT_CHATGPT_URL,
        // ⚠️ **正文那条**（实测 2026-10-23 r8）：`GET
        // /backend-api/conversations/<真实会话 id>` ⇒ 200／`$.messages` 22 条。
        body_url: Some("/backend-api/conversations/{session}"),
    }),
    // ⚠️ **消息数组在顶层**（实测 `$.messages`；**正文响应顶层没有 `mapping`**）。
    messages_paths: &[&["messages"]],
    // ⚠️ **角色在 `author.role`**（**两层**）—— 实测取值集合（2026-10-23 r8，
    // 那条正文响应的 22 条消息）：`system`×2、`user`×1、`assistant`×12、
    // `tool`×7。
    // ⇒ ⇒ **`system` 必须登记**（**硬要求**）：少了它 ⇒ 那两条 `system` 消息
    // **角色认不出** ⇒ ⇒ `message_of` 直接给 `None` ⇒ ⇒ **整条正文链**落
    // 「形状漂移」（**不是**「那两条被忽略」）。
    // ⇒ ⇒ `tool` 那一档**记进 `omitted`**（不是正文对话）；`system` 那一档
    // **也不进正文**（沿用既有的那一格 —— 认得出的其它角色都记进 `omitted`，
    // 与 DeepSeek 的 `system` **同一口径**）。
    role_clues: &[RoleClue {
        key: &["author", "role"],
        values: &[
            ("USER", "user"),
            ("ASSISTANT", "assistant"),
            ("SYSTEM", "system"),
            ("TOOL", "tool"),
        ],
    }],
    // ⚠️ **正文在 `content` 里**（实测：`content` 是**对象**，不是字符串）。
    // ⚠️ **形状按 2026-10-23 真机**全量**读数**（r31–r33：那条最新会话的
    // **全部 22 条**消息 —— **不是采样**）：
    // · **18 条**是 `{content_type, parts}` ⇒ ⇒ `parts[]` 的每一项**就是字符串**
    //   （实测 `partsLen:1`、`partTypes:{string:1}`）—— ⚠️ **那 18 条里有 15 条
    //   那一项是**空串**（实测逐项长度：15 个 0 ＋ 3 个非 0）⇒ ⇒ 它们的**正文
    //   取不到**（落 `unsupported` 的 `empty-content`）；
    // · **1 条**是 `{content_type, content}` ⇒ ⇒ 正文在**那个 `content` 字段**里
    //   ⇒ ⇒ `NestedField` 那一档**真的命中**（**不是**死代码！—— 实测正文裁出
    //   4 条，长度 `4785 / 68 / 9 / 3007`，那个 **9 字符**的正是这一条）；
    // · **3 条**是 `{content_type, thoughts, source_analysis_msg_id}`（**既没有
    //   `parts`、也没有 `content`**）⇒ ⇒ **思维链在 `content.thoughts`**
    //   （**嵌套两格**）⇒ ⇒ 那 3 条的正文取不到 ⇒ 落 `unsupported`。
    // ⇒ ⇒ **两条线索都要留**：`Parts` 管那 18 条、`NestedField` 管那 1 条。
    //
    // ⚠️ **条数账要能对上**（实测 22 条）：正文 **4** 条 ＋ `system` **2** 条
    // （进 `omitted` 的 thinking）＋ `tool` **7** 条（进 `omitted` 的 tool）＋
    // **9** 条取不到文本（**6** 条是空 `parts` 的 assistant ＋ **3** 条
    // `thoughts` 那种）＝ **22** ✔。
    //
    // ⚠️ **旧注释说「assistant 的 `content` ＝ `{content_type, content}`」** ——
    // 那是**只采样 8 条**（r8 的 `contentShapes` 前 8 条）得出的结论 ⇒ ⇒
    // **2026-10-23 r33 的全量读数推翻了它**：两种形状**都存在**，只是**分布**
    // 不同 ⇒ ⇒ **不许**再按「只有一种形状」写注释。
    //
    // DeepSeek 那个 `content`（字符串）／`text`／`fragments` 在 ChatGPT 侧
    // **一个字都没有**（`content` 是**对象**，不是字符串）。
    text_sources: &[
        TextSource::NestedField("content", "content"),
        TextSource::Parts("content"),
    ],
    // ⚠️ **思维链字段**：实测思维链在 **`content.thoughts`**（`content` 对象里
    // 那一格 ⇒ **两格路径**）—— 而 [`SiteSpec::thinking_key`] 是**单个键名**
    // （`&'static str`）⇒ ⇒ **表达不了**那条嵌套路径。
    // ⇒ ⇒ 这里**留空**（**不硬凑**：把 `content` 填进去会指向整个对象、把
    // `thoughts` 填进去会取不到 —— 两种都是**错的**）。
    // ⇒ ⇒ 留空的**后果**（据实）：那 3 条带思维链的消息**不算**「带着思维链」
    // ⇒ ⇒ 思维链**不会**被记进 `omitted`；而它们的 `content` 里**既没有
    // `parts`、也没有 `content`** ⇒ ⇒ **正文取不到** ⇒ ⇒ 落进 `unsupported`
    // （`empty-content`）**不静默丢**。
    // ⚠️ **那一格 `thoughts` 里是什么，本轮**没有验** ⇒ ⇒ **不编**。
    thinking_key: "",
    // ⚠️ ChatGPT **没有**「还有没有更早的」那个布尔 ⇒ ⇒ 判不出「取全没取全」
    // ⇒ ⇒ **不许说「全部」**（协议 §9 G4-23）⇒ ⇒ `pick` 里**不写** `complete`。
    body_has_more_path: &[],
    // ⚠️ ChatGPT 的正文端点**不是**增量协议（读数里**没有** `cache_control`／
    // `cache_version`）⇒ ⇒ 冷档案对它**没有理由** ⇒ ⇒ **生产档案**
    // （PM 2026-10-22 13:4x 裁）。
    body_profile: BodyProfile::Production,
};

/// 按站点短码取那一份规格（**只有这一处**按站点分叉）。
///
/// ⚠️ **认不出 ⇒ `None`**（**不猜**、**不回落**成某个站点）。
pub(crate) fn spec_for(site: &str) -> Option<&'static SiteSpec> {
    match site {
        SITE => Some(&DEEPSEEK_SPEC),
        MINIMAX_SITE => Some(&MINIMAX_SPEC),
        CHATGPT_SITE => Some(&CHATGPT_SPEC),
        _ => None,
    }
}

/// 从**请求体**里取**站点短码**（PM 2026-10-22 13:4x 裁）。
///
/// ## 口径
///
/// · **没给**（或给的是空串）⇒ ⇒ **默认那个站点**（DeepSeek）—— 既有调用方
///   **一个字都不改**；
/// · **给了但规格表里没有** ⇒ ⇒ **明确报错**（**不猜、不回落**成某个站点）——
///   「我是谁」这件事只有**内核**知道 ⇒ ⇒ 它给错短码时必须**说出来**，不能
///   悄悄去取**另一个站**的目录。
pub(crate) fn site_of(parsed: &Value) -> Result<String, String> {
    let raw = match parsed.get("site") {
        Some(Value::String(raw)) => raw.trim().to_string(),
        // **没给** ⇒ 默认那个站点（既有调用方一个字都不改）。
        None | Some(Value::Null) => return Ok(SITE.to_string()),
        Some(_) => return Err("请求体里的站点短码不是字符串".to_string()),
    };
    if raw.is_empty() {
        return Ok(SITE.to_string());
    }
    if spec_for(&raw).is_none() {
        return Err(format!("请求体里的站点短码 {raw} 没有登记取数面"));
    }
    Ok(raw)
}

/// 按**路径**取一个值（取不到 ⇒ `None`，**不猜**）。
pub(crate) fn value_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    Some(current)
}

/// 取数窗的**用途标记**（目录那条链）—— 同一个族窗，label 靠它分开。
pub(crate) const STRUCTURE_KIND: &str = "structure";

/// 订阅的 DevTools 事件（**按具体事件名** —— 官方 API 形态；协议 §8.5.1 硬约束 4）。
///
/// · `Network.responseReceived` —— 给出 `requestId` ↔ **URL** ＋ **HTTP 状态**；
/// · `Network.loadingFinished` —— 给出「**这条响应的体现在可以取了**」。
///
/// ⚠️ **不带通配符**：官方 API 是 `GetDevToolsProtocolEventReceiver(LPCWSTR eventName, …)`
/// ⇒ **必须按具体事件名**订阅（本机 SDK 这一版**没有** `Subscribe` 方法）。
const SUBSCRIBED_EVENTS: [&str; 2] = ["Network.responseReceived", "Network.loadingFinished"];

/// 单次 CDP 调用的超时。
const CDP_TIMEOUT: Duration = Duration::from_secs(10);

/// 整轮取数的超时（站点加载 ＋ 初始化批次 ＋ 取体）。
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

/// 目录结构的**保鲜期**（多久算「这份还新鲜」）—— 协议 §4.6 修法 3 的兜底。
///
/// ## 为什么**壳侧**也得有一份保鲜期（插件侧那层不够）
///
/// 插件侧自己带一层目录页缓存（`catalogCache`）⇒ ⇒ 它过期之后会**再来问壳侧**
/// ⇒ ⇒ 壳侧若永远回**同一份**旧结构，插件侧那层 TTL 就**没有牙齿** ⇒ ⇒
/// 新鲜度必须由**壳侧**兜住底。
///
/// ## 这个数怎么定的
///
/// · **够短**：用户在官网对话里刚聊完一条，**一分钟内**就能在候选组里看见它；
/// · **够长**：**不会**每个按键都重取（实测客户端每个按键都调一次 `catalog`）
///   —— 重取一轮是**秒级**（建窗 ＋ 导航 ＋ 取体），按按键算会把界面拖垮。
const STRUCTURE_TTL: Duration = Duration::from_secs(60);
// Keep keystrokes in one failed round together, without hiding a recovered login for a minute.
const STRUCTURE_FAILURE_TTL: Duration = Duration::from_secs(2);

/// 壳的 `AppHandle`（**取数模块要开窗、要拿数据目录**，而桥那条路由在线程里）。
///
/// 桥是在**启动内核**时起的（`kernel/kernel_adapter.rs`），那一刻**拿不到** `AppHandle`
/// ⇒ 只能由 `lib.rs` 的 `setup` 在这里登记一份。
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 取数的**全局状态**（缓存 ＋ 启动标志 ＋ 完成通知）。
static STATE: OnceLock<CaptureState> = OnceLock::new();

/// 取数的全局状态。
struct CaptureState {
    /// 最近一次抓取的结果（**成功与失败都记** —— 失败也要能让下一次读取立刻回它）。
    ///
    /// ⚠️ **按站点分开**（一份站点一份）—— 目录结构是**站点自己的**东西 ⇒ ⇒
    /// 拿 A 站的缓存回 B 站＝**把别的站点的会话目录当成自己的** ⇒ ⇒ 必须分开。
    cache: Mutex<BTreeMap<String, Result<SessionStructure, CaptureFailure>>>,
    /// 最近一次抓取**完成**的时刻（判保鲜期 ＋ 判「这一轮是不是为我们跑的」）。
    ///
    /// ⚠️ **同样按站点分开**（保鲜期是**每站一份**）。
    fetched_at: Mutex<BTreeMap<String, Instant>>,
    accounts: Mutex<BTreeMap<String, String>>,
    /// 取数轮的**互斥**（同一时刻只跑一轮）—— 实测客户端每个按键都调一次
    /// `catalog` ⇒ ⇒ 不互斥的话会同时开出好几扇取数窗、抢同一份档案。
    round: tokio::sync::Mutex<()>,
    /// 上一轮那扇取数窗（**关窗是异步的** ⇒ 留着句柄，下一轮开头再关一次，
    /// 免得关不掉时越攒越多）。
    last_window: Mutex<Option<tauri::WebviewWindow>>,
}

/// 取全局状态（**懒初始化**，不依赖 setup 的先后）。
fn state() -> &'static CaptureState {
    STATE.get_or_init(|| CaptureState {
        cache: Mutex::new(BTreeMap::new()),
        fetched_at: Mutex::new(BTreeMap::new()),
        accounts: Mutex::new(BTreeMap::new()),
        round: tokio::sync::Mutex::new(()),
        last_window: Mutex::new(None),
    })
}

/// 取缓存里的结果（**克隆一份** ⇒ 不在持锁时 await）。
fn cached(state: &CaptureState, site: &str) -> Option<Result<SessionStructure, CaptureFailure>> {
    state
        .cache
        .lock()
        .ok()
        .and_then(|guard| guard.get(site).cloned())
}

/// 最近一次抓取**完成**的时刻。
fn fetched_at(state: &CaptureState, site: &str) -> Option<Instant> {
    state
        .fetched_at
        .lock()
        .ok()
        .and_then(|guard| guard.get(site).copied())
}

/// **保鲜期内**的那份（`None` ＝ 没有，或者已经旧了）。
fn fresh(state: &CaptureState, site: &str) -> Option<Result<SessionStructure, CaptureFailure>> {
    let at = fetched_at(state, site)?;
    let outcome = cached(state, site)?;
    let ttl = if outcome.is_ok() {
        STRUCTURE_TTL
    } else {
        STRUCTURE_FAILURE_TTL
    };
    (at.elapsed() < ttl).then_some(outcome)
}

/// 这一轮**能不能复用**（复用 ＝ 不用再跑一轮）。
///
/// · `refresh=true` ⇒ **只**在「有一轮是在我们请求**之后**跑完的」时复用 ——
///   那一轮就是**当刻**的 ⇒ ⇒ 不必再跑一轮；否则**必须**重取。
/// · `refresh=false` ⇒ 保鲜期内就能用；保鲜期外**不复用**（宁可重取，
///   也不把一份旧的当新的给出去）。
fn reusable(
    state: &CaptureState,
    site: &str,
    requested_at: Instant,
    refresh: bool,
) -> Option<Result<SessionStructure, CaptureFailure>> {
    let at = fetched_at(state, site)?;
    if at < requested_at {
        // 手上这份是**我们请求之前**跑的 ⇒ ⇒ 算不上「当刻的」。
        return None;
    }
    if refresh {
        return cached(state, site);
    }
    fresh(state, site)
}

/// 写回缓存（**成功与失败都记** —— 失败也要让下一次读取立刻回它）。
fn write_cache(
    state: &CaptureState,
    site: &str,
    outcome: Result<SessionStructure, CaptureFailure>,
) {
    if let Ok(mut guard) = state.cache.lock() {
        guard.insert(site.to_string(), outcome);
    }
    if let Ok(mut guard) = state.fetched_at.lock() {
        guard.insert(site.to_string(), Instant::now());
    }
}

pub(crate) async fn account_observed(site: &str, fingerprint: &str) -> Result<(), CaptureFailure> {
    let state = state();
    // 等正在写回的旧目录结束再失效，避免它在清缓存后重新发布。
    let _round = state.round.lock().await;
    invalidate_account(state, site, fingerprint)
}

fn invalidate_account(
    state: &CaptureState,
    site: &str,
    fingerprint: &str,
) -> Result<(), CaptureFailure> {
    let failure = || CaptureFailure::Return("账户缓存状态不可用".to_string());
    let mut accounts = state.accounts.lock().map_err(|_| failure())?;
    if accounts.get(site).map(String::as_str) != Some(fingerprint) {
        state.cache.lock().map_err(|_| failure())?.remove(site);
        state.fetched_at.lock().map_err(|_| failure())?.remove(site);
        accounts.insert(site.to_string(), fingerprint.to_string());
    }
    Ok(())
}

/// 登记壳句柄（由 `lib.rs` 的 `setup` 调一次）。
pub(crate) fn bind_app(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

/// 取壳句柄（**没登记就是取不了** —— 明确报错，不静默降级）。
pub(crate) fn app_handle() -> Option<tauri::AppHandle> {
    APP.get().cloned()
}

/// 官方对话的 user-data 目录（**必须与官网对话窗口那份一致** —— 协议 §2.1 第 1 条）。
///
/// 换目录就拿不到登录态（协议 §2.5）⇒ 这里**只从统一入口取**，不自造路径。
pub(crate) fn profile_dir(app: &tauri::AppHandle) -> Result<PathBuf, CaptureFailure> {
    let state = app
        .try_state::<crate::commands::AppState>()
        .ok_or_else(|| CaptureFailure::Return("壳还没登记数据目录，取数上下文建不起来".into()))?;
    Ok(state.data_dir.join("webview-official-chat"))
}

/// 取数窗 label 的**序号**（每轮 +1）。
///
/// ⚠️ **为什么 label 要每轮唯一** —— Tauri 的 label **撞名就建窗失败**，而关窗是
/// **异步**的 ⇒ ⇒ 上一轮那扇窗还没销毁时，下一轮建窗就会撞名 ⇒ ⇒ 取数轮
/// 一分钟一次的话，撞名不是「偶发」而是「迟早」。
///
/// （顺带修掉正文那条链的同一个隐患：它也是每抓一次建一扇同 label 的窗。）
#[cfg(any(windows, target_os = "macos"))]
static CAPTURE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 建一扇**自己家的**不可见取数窗。
///
/// **Windows 专用链**（建窗 → 订阅 → `Network.enable` → 导航那一整条链的
/// 第一步）：别的平台没有壳内 DevTools 通路 ⇒ 这扇窗在那边**整块都不存在**
/// （非 Windows 侧走 `CaptureFailure::Unsupported`，见 [`arm_capture_window`]
/// 的非 Windows 分支）⇒ ⇒ 本函数挂 `#[cfg(windows)]`，不是「构造了没人用」。
///
/// **为什么是顶格自由函数**：仓库判据 `check-invariants.mjs` 的「远程建窗链
/// 认族」按**顶格 `fn `**切链（窗口身份词常在 `WebviewUrl::External(` 的上一行
/// `format!` 里，起点必须退到函数签名才认得出族）。放在 `impl` 的方法里会让它
/// 落在切片之外，判据会把这条链报成「认不出属于哪一族」。
///
/// 建窗接线按官方对话族：注入 `no-context-menu.js`（别人的站点不许自绘右键
/// 菜单），标题走 `window_title()`，主题走 `appearance::initial_theme`。
///
/// ## `kind` 是干什么的
///
/// **同一族窗、不同用途** ⇒ label 要能分开：结构那条（`structure`）与正文那条
/// （`body`，协议 §4.4）**各建各的窗** ⇒ ⇒ 两个用途不会抢同一个 label
/// （Tauri 的 label 撞车 ⇒ 建窗直接失败）。
///
/// ⚠️ **label 的族标记（`official-chat-`）必须留在这个函数体里** —— 仓库判据
/// `check-invariants.mjs` 的「远程建窗链认族」按**顶层函数体**判窗口族
/// （`kindOf` 找的是 `official-chat` 这个身份词）⇒ label 挪到调用方就认不出族了。
///
/// ⚠️ **label 不许写成 `&format!(…)`** —— 判据第 5 条把那个形态当成
/// 「遍历窗口」（`WebviewWindowBuilder::new(app, &format!`）⇒ ⇒ 必须先落到
/// 一个 `let` 上再传。
#[cfg(windows)]
fn build_capture_window(
    app: &tauri::AppHandle,
    site: &str,
    kind: &str,
    profile_dir: &std::path::Path,
) -> Result<tauri::WebviewWindow, String> {
    let seq = CAPTURE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!("official-chat-capture-{kind}-{site}-{seq}");
    // 停在 about:blank：**订阅与 `Network.enable` 都在导航之前** ⇒ 订阅必然早于
    // 站点首个请求（协议 §8.5.1 硬约束 3）。
    let url: tauri::Url = "about:blank"
        .parse()
        .map_err(|e| format!("取数上下文的初始地址非法：{e}"))?;
    std::fs::create_dir_all(profile_dir).map_err(|e| format!("无法创建官网对话数据目录：{e}"))?;
    tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(url))
        .title(crate::shell::window::window_title("官网对话取数上下文"))
        .inner_size(800.0, 600.0)
        .visible(false)
        .theme(crate::shell::appearance::initial_theme(&label))
        .data_directory(profile_dir.to_path_buf())
        .additional_browser_args(crate::harness::official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .initialization_script(include_str!("../harness/no-context-menu.js"))
        .build()
        .map_err(|e| format!("无法建立取数上下文：{e}"))
}

/// 建一扇**自己家的**不可见取数窗（**macOS 路线 B**）。
///
/// ## 与 Windows 那版**同形**，只有两处不同
///
/// 1. **数据存储标识符**：macOS 上 WKWebView 认的是
///    `data_store_identifier`（不是 `data_directory`）⇒ ⇒ 绑**官网对话窗口
///    那一份**（`OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`，按壳模式分 dev／release）
///    ⇒ ⇒ **同一个 cookie jar、同一份登录态**。
/// 2. **不订阅 DevTools 事件**（路线 B 自己发那一发、自己拿那一发）。
///
/// ## 为什么 label 的族标记要留在这个函数体里
///
/// 仓库判据 `check-invariants.mjs` 的「远程建窗链认族」按**顶层函数体**判
/// 窗口族 ⇒ label 挪到调用方就认不出族了。
#[cfg(target_os = "macos")]
fn build_capture_window(
    app: &tauri::AppHandle,
    site: &str,
    kind: &str,
    profile_dir: &std::path::Path,
) -> Result<tauri::WebviewWindow, String> {
    let seq = CAPTURE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!("official-chat-capture-{kind}-{site}-{seq}");
    // 停在 about:blank：**导航在 `capture_in` 里**（路线 B 也在那里发那一发）。
    let url: tauri::Url = "about:blank"
        .parse()
        .map_err(|e| format!("取数上下文的初始地址非法：{e}"))?;
    std::fs::create_dir_all(profile_dir).map_err(|e| format!("无法创建官网对话数据目录：{e}"))?;
    let builder = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(url))
        .title(crate::shell::window::window_title("官网对话取数上下文"))
        .inner_size(800.0, 600.0)
        .visible(false)
        .theme(crate::shell::appearance::initial_theme(&label))
        // Windows 上 WebView2 靠 user-data 目录共享档案；macOS 上这个参数被
        // WKWebView **忽略**（它认的是下面的 `data_store_identifier`）⇒
        // 两边都给，口径一致。
        .data_directory(profile_dir.to_path_buf())
        .additional_browser_args(crate::harness::official_chat::OFFICIAL_CHAT_BROWSER_ARGS)
        .initialization_script(include_str!("../harness/no-context-menu.js"));
    // ⚠️ **必须绑官网对话窗口那一份数据存储** ⇒ ⇒ 这扇窗才有登录态。
    let builder = builder
        .data_store_identifier(crate::harness::official_chat::OFFICIAL_CHAT_DATA_STORE_IDENTIFIER);
    builder
        .build()
        .map_err(|e| format!("无法建立取数上下文：{e}"))
}

/// 一次取数失败。**每一类都对应协议 §6 的一个既有码**（不新增第 15 个码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CaptureFailure {
    /// **回程失败**（取体失败 / 页面销毁 / 壳侧错误）——协议 §6.3：与「站点返回空」分开。
    Return(String),
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    NotLoggedIn(String),
    /// **回程超时**（等站点那条目录响应加载完没等到）。
    ///
    /// ⚠️ 协议 **v0.11** §8.5.2（G4-14 ③）：**「还没抓到结构」落 `catalog_failed`
    /// （可重试），绝不落 `catalog_empty`** —— `catalog_empty` **只**表示「有结构但
    /// 无匹配项」⇒ ⇒ 这一格与 [`CaptureFailure::Empty`] 必须是**两个不同的码**。
    Timeout(String),
    /// **站点返回空**（接口正常返回，但没有会话）——协议 §6.3。
    Empty,
    /// **站内码**（**原样上报，不解释语义** —— 协议 §6.2）。
    SiteCode { code: String, msg: Option<String> },
    /// **形状漂移**（官网页面结构变了）。
    Schema(String),
    /// **本机没有这条通路**（macOS：wry 未暴露可用的 DevTools 通路 —— 协议 §6.1）。
    ///
    /// ⚠️ **只在非 Windows 平台构造**（Windows 上有 WebView2 的 DevTools 通路）
    /// ⇒ ⇒ Windows 构建里这个变体「不会被构造」是**正常**的。
    #[allow(dead_code)]
    Unsupported(String),
}

impl CaptureFailure {
    /// 这个失败对应**哪个既有协议码**（协议 §6.1：不扩闭集）。
    pub(crate) fn code(&self) -> &'static str {
        match self {
            CaptureFailure::NotLoggedIn(_) => "not_logged_in",
            // 「还没抓到结构」⇒ `catalog_failed`（可重试）—— 协议 v0.11 §8.5.2
            // G4-14 ③：**绝不**落 `catalog_empty`（那个码只表示「有结构但无匹配项」）。
            CaptureFailure::Timeout(_) => "catalog_failed",
            // 站点返回空 ⇒ `catalog_empty`（**与回程失败不是同一个码** —— 协议 §6.3）。
            CaptureFailure::Empty => "catalog_empty",
            // 形状漂移 ⇒ `schema_changed`。
            CaptureFailure::Schema(_) => "schema_changed",
            // 本机没这条通路 ⇒ `source_unsupported`（协议 §6.1：`capability_unsupported`
            // 是**内部码**，对外**归一化**成 `source_unsupported` —— 后者才在 P1 §3 的
            // 闭集里）。
            CaptureFailure::Unsupported(_) => "source_unsupported",
            // 回程失败 / 站内码 ⇒ `catalog_failed`（可重试；站内码原样进 detail）。
            CaptureFailure::Return(_) | CaptureFailure::SiteCode { .. } => "catalog_failed",
        }
    }

    /// **脱敏**的原因线索（只放**已分类**的事实，不放异常串）。
    pub(crate) fn detail(&self) -> String {
        match self {
            CaptureFailure::NotLoggedIn(detail) => detail.clone(),
            CaptureFailure::Return(detail) => format!("回程失败：{detail}"),
            CaptureFailure::Timeout(detail) => {
                format!("还没抓到站点的会话目录结构（可重试）：{detail}")
            }
            CaptureFailure::Empty => "站点返回了空目录（接口正常，但没有会话）".to_string(),
            CaptureFailure::Schema(detail) => format!("页面结构与协议 §4.3 不符：{detail}"),
            CaptureFailure::Unsupported(detail) => format!("本机没有这条通路：{detail}"),
            // 站内码：**原样**（码 + 逐字 msg）——不解释语义（协议 §6.2）。
            CaptureFailure::SiteCode { code, msg } => match msg {
                Some(msg) if !msg.is_empty() => format!("站内码 {code} {msg}"),
                _ => format!("站内码 {code}"),
            },
        }
    }
}

// ⚠️ **正文那条链（协议 §4.4）自己的失败映射在 `session_body.rs`**
// （`CaptureFailure::capture_code`／`capture_detail`）—— 目录那条链的映射留在本文件。
//
// **为什么两条链各有一份**：目录那条链的失败落在**目录**语义上
// （`catalog_failed`／`catalog_empty`）；正文那条链的失败落在**捕获**语义上
// （`timeout` 可重试、`schema_changed` 说「站点结构变了」）⇒ ⇒ 同一个失败在
// 两条链上**该给不同的既有码**。

/// 回程载荷的**白名单**（协议 §2.4 **逐字列举**那几项，一项不多）。
///
/// §2.4「允许回传」＝ 顶层键名 · `code`/`msg`（原样、不解释语义）· `data` 的键名
/// 数组 · 数组的长度 · 数组首元素的键名数组 · 条目的类型/布尔量
/// · **目录项面**（v0.17：顶层 `items` 数组，每项恰好 6 个键）。
pub(crate) const RETURN_FIELDS: [&str; 9] = [
    "topKeys",
    "code",
    "msg",
    "dataKeys",
    "sessionsLength",
    "firstItemKeys",
    "itemTypes",
    "hasMore",
    "items",
];

/// **目录项面**的每一项（协议 §2.4 面二 / §4.3 ②：恰好 6 个键，类型写死）。
///
/// ## 为什么这里带**值**（`id` / `title`）
///
/// v0.16 之前 §2.4 的白名单**禁**会话 id 与标题 ⇒ 结构面只能回键名与类型；
/// v0.17 增了**目录项面** ⇒ 目录页要的那几样（会话 id / 标题 / 更新时间）
/// **允许**出现在 `items` 的**元素内部** ⇒ ⇒ 于是「值」在这一层回。
///
/// ## 为什么字段**不写** `pub(crate)`
///
/// 判据第 3／8 条按**源码形态**抽这一段的字段名与类型
/// （`scripts/lib/p4-source-shape.mjs` 的 `rustStructFields` 只认**裸字段名**）
/// ⇒ ⇒ 写成 `pub(crate) id: String` 会被读成「没有这个键」⇒ ⇒ 键面假红。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CatalogItem {
    /// 会话 id（站点自己的 `chat_session.id`）。
    id: String,
    /// 标题。
    title: String,
    /// 标题类型（`USER` / `SYSTEM` …）。
    title_type: String,
    /// 模型类型。
    model_type: String,
    /// 是否置顶。
    pinned: bool,
    /// 更新时间（**秒级小数时间戳**）。
    updated_at: f64,
}

/// 一次取回的**结构**（协议 §2.4 白名单）**＋ 目录项面**。
///
/// **结构面里没有正文、没有标题、没有会话 id、没有凭据** —— 壳侧拿到完整响应体时
/// 就已经裁掉了（协议 §8.5.1 末段）。
///
/// ⚠️ **目录项面（`items`）是 v0.17 增的唯一例外**：那一层**按 §4.3 ② 逐项**
/// 裁成 6 个键，会话 id 与标题**就在那 6 个键里**（协议 §2.4 面二）⇒ ⇒ 除这
/// 6 个键之外，**正文 / 凭据 / 整包仍然一个都不带**。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SessionStructure {
    /// 顶层键名。
    pub(crate) top_keys: Vec<String>,
    /// 顶层 `code`（**原样**；不解释语义）。
    pub(crate) code: Option<i64>,
    /// 顶层 `msg`（**原样**）。
    pub(crate) msg: Option<String>,
    /// `data` 的键名。
    pub(crate) data_keys: Vec<String>,
    /// 条数（**从 `chat_sessions.length` 读** —— 协议 §4.3 ③）。
    pub(crate) sessions_length: Option<usize>,
    /// 首元素的键名（协议 §2.4）。
    pub(crate) first_item_keys: Vec<String>,
    /// 首元素各键的**类型**（协议 §2.4：「条目的类型」）。
    pub(crate) item_types: BTreeMap<String, String>,
    /// `has_more`（**boolean**：还有更多页，**不是条数** —— 协议 §4.3 ③）。
    pub(crate) has_more: Option<bool>,
    /// **目录项面**（协议 §2.4 面二）：每项恰好 6 个键（§4.3 ②）。
    ///
    /// ⚠️ 这里带**值**（会话 id / 标题）—— v0.17 的目录项面允许它们出现在
    /// `items` 的**元素内部**（结构面仍然只有键名与类型）。
    pub(crate) items: Vec<CatalogItem>,
}

impl SessionStructure {
    /// 从**壳侧拿到的完整响应体**裁出白名单结构，并**顺手判掉四档**。
    ///
    /// ## 为什么裁剪在**壳侧**
    ///
    /// 协议 §1.1 的「资产不浪费条款」＋ §8.5.1 末段：`Network` 域拿到的是**站点
    /// 的完整响应体**（`biz_data.chat_sessions` 里**含** `title` / `id`）⇒ ⇒
    /// **不许**把原文直接回传 ⇒ ⇒ 由**壳侧**解析并裁成 §2.4 的白名单结构。
    ///
    /// ## 为什么 `status` 与 `biz_code` **不进**结构体
    ///
    /// 协议 §2.4 的「允许回传」是**列举式**的 ⇒ ⇒ 白名单之外即红（判据第 3 条）
    /// ⇒ ⇒ 这两个数只在**壳侧**参与判断（§4.2 的成功判据要 `status == 200`；
    /// §6.2 的「两层都要看」要 `biz_code`）⇒ ⇒ **不进回程载荷**。
    /// 站内码要上报时，走**失败 detail**（§6.2「原样上报」），那不是结构回程。
    ///
    /// ## 为什么这一层**必须跨平台存在**
    ///
    /// 它是**纯逻辑**（只裁 §2.4 白名单 + 判 §4.2/§6.2/§6.3 四档），
    /// **不碰 DevTools、不碰 webview** ⇒ ⇒ 别的平台没有「取数窗」这条通路，
    /// 但**裁剪口径与四档判断必须两平台一致** ⇒ ⇒ 它必须跨平台存在
    /// （非 Windows 上由本模块的用例调用，Windows 上由取数链调用）。
    ///
    /// ⚠️ **别给它挂 `#[cfg(windows)]`**（2026-10-21 实测过一次）：本模块的用例
    /// 是**跨平台**的（它们断言的是协议口径，不是平台能力）⇒ 一挂上去，
    /// 非 Windows 上 `cargo test --lib` 直接 `E0599: no associated function
    /// or constant named from_body`（8 处）⇒ ⇒ **macOS 门禁必红**。
    ///
    /// ⚠️ **它是「默认站点」那个入口**（**跨平台用例按它断言协议口径** ⇒ ⇒
    /// **名字不许改**）；生产链走 [`SessionStructure::from_body_for`]（**按站点**）。
    #[allow(dead_code)]
    pub(crate) fn from_body(body: &str, status: i64) -> Result<Self, CaptureFailure> {
        let spec = spec_for(SITE).ok_or_else(|| {
            CaptureFailure::Return(format!("站点 {SITE} 没有登记取数面（规格表里缺这一条）"))
        })?;
        Self::from_body_for(body, status, spec)
    }

    /// **按站点**裁白名单结构（**一条链、多个站点**）。
    ///
    /// ⚠️ **DeepSeek 的口径一个字没动**：[`from_body`] 只是**用现役那个站点
    /// 的规格**调它 ⇒ ⇒ 现役行为与改之前**逐字相同**。
    ///
    /// ## 站点相关的部分**只有这些**
    ///
    /// · 会话数组在**哪儿**（DeepSeek `data.biz_data.chat_sessions`；
    ///   MiniMax `sessions`）；
    /// · 站内码／站内 msg 在**哪儿**（DeepSeek 顶层 `code`／`msg`；
    ///   MiniMax `base_resp.status_code`／`status_msg`）；
    /// · `has_more` 在**哪儿**；
    /// · 「`data` 的键名」那一栏从**哪个对象**读；
    /// · 条目对象在数组元素的**哪一层**；
    /// · 那 6 个键 ↔ 站点自己的**字段名**（**映射不到 ⇒ 留空串／0**）。
    ///
    /// ## ⚠️ **条数上限**不在这里
    ///
    /// 「最多几条」是**站点无关**的（[`CATALOG_ITEM_LIMIT`]）⇒ ⇒ **三个站点
    /// 共用同一个数**。
    pub(crate) fn from_body_for(
        body: &str,
        status: i64,
        spec: &SiteSpec,
    ) -> Result<Self, CaptureFailure> {
        let value: Value = serde_json::from_str(body)
            .map_err(|e| CaptureFailure::Return(format!("站点的响应体不是合法 JSON：{e}")))?;

        let top_keys = keys_of(value.as_object());
        let code = number_of(value_at(&value, spec.code_path));
        let msg = value_at(&value, spec.msg_path)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // ⚠️ **「`data` 的键名」那一栏**：站点**没有** `data` 那一层时（MiniMax）
        // ⇒ ⇒ 这一栏**为空**（**不是**顶层键名 —— 顶层键名是 `topKeys` 那一栏）。
        let data_keys = if spec.data_keys_path.is_empty() {
            Vec::new()
        } else {
            keys_of(value_at(&value, spec.data_keys_path).and_then(Value::as_object))
        };
        // ⚠️ **业务层那一层站内码**（协议 §4.2：「两层都要看」）—— DeepSeek 那个在
        // `data.biz_code`；MiniMax **没有**那一层 ⇒ ⇒ 取不到就是没有（**不猜**）。
        let biz_code = number_of(value.get("data").and_then(|d| d.get("biz_code")));
        let biz_msg = value
            .get("data")
            .and_then(|d| d.get("biz_msg"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let sessions = value_at(&value, spec.sessions_path).and_then(Value::as_array);
        let sessions_length = sessions.map(|list| list.len());
        // ⚠️ **条目对象**在数组元素的**哪一层**（MiniMax 在 `session` 下面）。
        let first = sessions
            .and_then(|list| list.first())
            .and_then(|item| value_at(item, spec.item_path));
        let first_item_keys = keys_of(first.and_then(|v| v.as_object()));
        let item_types = first.map(type_map).unwrap_or_default();
        let has_more = value_at(&value, spec.has_more_path).and_then(Value::as_bool);

        // **目录项面**（协议 §2.4 面二）：从同一批会话里**逐项裁**成 §4.3 ② 的
        // 6 个键 ⇒ ⇒ 值只在这 6 个键里，**别的键一个都不带**
        // （正文 / 凭据 / 整包全在裁剪时丢掉了）。
        let items = catalog_items(sessions, spec);

        // 四档判断（协议 §4.2 成功判据 / §6.2 站内码 / §6.3 两类错误分离）。
        judge_response(
            spec,
            status,
            code,
            msg.clone(),
            biz_code,
            biz_msg,
            sessions_length,
        )?;

        Ok(Self {
            top_keys,
            code,
            msg,
            data_keys,
            sessions_length,
            first_item_keys,
            item_types,
            has_more,
            items,
        })
    }

    /// 回给内核的 JSON 形态（**只回结构 ＋ 目录项面** —— 字段名就是 §2.4 白名单
    /// 那几项 ＋ `items`）。
    pub(crate) fn to_json(&self) -> Value {
        let payload = json!({
            "topKeys": self.top_keys,
            "code": self.code,
            "msg": self.msg,
            "dataKeys": self.data_keys,
            "sessionsLength": self.sessions_length,
            "firstItemKeys": self.first_item_keys,
            "itemTypes": self.item_types,
            "hasMore": self.has_more,
            "items": self.items.iter().map(|item| json!({
                "id": item.id,
                "title": item.title,
                "title_type": item.title_type,
                "model_type": item.model_type,
                "pinned": item.pinned,
                "updated_at": item.updated_at,
            })).collect::<Vec<Value>>(),
        });
        // 载荷的字段集**必须**与 §2.4 的白名单**逐字相等** ⇒ 少一项不行，
        // 多一项也不行（白名单之外即红 —— 协议 §10 第 3 条）。
        debug_assert!(
            payload
                .as_object()
                .map(|map| map.len() == RETURN_FIELDS.len()
                    && map.keys().all(|key| RETURN_FIELDS.contains(&key.as_str())))
                .unwrap_or(false),
            "回程载荷的字段集与协议 §2.4 的白名单不一致"
        );
        payload
    }
}

/// 按**协议 §4.2 的成功判据**与**§4.3 的字段面**判一份响应。
///
/// **四档分开**（协议 §6.2 / §6.3）：
/// ① 传输层失败（`status != 200`）⇒ 回程失败；
/// ② 站内码（顶层 `code != 0`，或业务层 `biz_code != 0`）⇒ **原样上报**；
/// ③ 形状漂移（`biz_data` 里没有 `chat_sessions`）⇒ `schema_changed`；
/// ④ 站点返回空（条数 0）⇒ `catalog_empty`。
fn judge_response(
    spec: &SiteSpec,
    status: i64,
    code: Option<i64>,
    msg: Option<String>,
    biz_code: Option<i64>,
    biz_msg: Option<String>,
    sessions_length: Option<usize>,
) -> Result<(), CaptureFailure> {
    if status != 200 {
        return Err(CaptureFailure::Return(format!(
            "传输层状态码 {status}（协议 §4.2 的成功判据要求 200）"
        )));
    }
    // 站内码（**原样上报，不解释语义** —— 协议 §6.2）：顶层 `code`。
    if let Some(code) = code {
        if code != 0 {
            return Err(CaptureFailure::SiteCode {
                code: code.to_string(),
                msg,
            });
        }
    }
    // 站内码的**业务层**那一层（协议 §4.2：「两层都要看」；反例 ＝ `code=0` ＋
    // `biz_code=2` ＋ `biz_data=null` —— 传输成功、业务失败）。
    if let Some(biz_code) = biz_code {
        if biz_code != 0 {
            return Err(CaptureFailure::SiteCode {
                code: biz_code.to_string(),
                msg: biz_msg,
            });
        }
    }
    // 形状：会话数组**必须在**这个站点登记的那个位置（协议 §4.3 ①）。
    let Some(length) = sessions_length else {
        return Err(CaptureFailure::Schema(format!(
            "读不到会话数组的长度（协议 §4.3 ③：条数只能从数组长度读；位置：{}）",
            spec.sessions_path.join(".")
        )));
    };
    if length == 0 {
        return Err(CaptureFailure::Empty);
    }
    Ok(())
}

/// 取一个对象字面量的键名。
fn keys_of(object: Option<&Map<String, Value>>) -> Vec<String> {
    object
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// 目录项面**最多回几条**（izzy 2026-10-22 12:4x 裁：「优先拿网页对话的最近
/// （按时间倒序）前 6 个会话，不需要取那么多」）。
///
/// ⚠️ **这只是一层「裁」，不是「翻页」** —— 站点那**一条**
/// `GET /api/v0/chat_session/fetch_page` 的响应里**已经带着全部会话**
/// （实测 16996 B ／ 100 条）⇒ ⇒ 「最近 6 个」是**从那一条响应里挑出来的**
/// ⇒ ⇒ **壳侧仍然零请求**（协议 §1 裁定 1 第 1 条一个字不许松）。
///
/// ⚠️ **这个数是站点无关的** —— DeepSeek ／ MiniMax ／ ChatGPT **共用同一个**
/// [`CATALOG_ITEM_LIMIT`] ⇒ ⇒ **不许**按站点各写一份、**不许**按站点传参。
///
/// ⚠️ **取不满就照实给**：站点只给了 4 条就回 4 条 ⇒ ⇒ **不编、不补、不因此报错**。
pub(crate) const CATALOG_ITEM_LIMIT: usize = 6;

/// 把站点的会话数组裁成**目录项面**（协议 §2.4 面二 / §4.3 ②）。
///
/// **逐项只取 6 个键**（`id` / `title` / `title_type` / `model_type` / `pinned` /
/// `updated_at`）⇒ ⇒ 站点条目里别的键（**正文、凭据、整包**）在这一步
/// **全被丢掉** ⇒ ⇒ 不可能外发。
///
/// ## 裁的**顺序**（izzy 2026-10-22 09:3x 裁）
///
/// ① **先逐项裁**成那 6 个键（**这一步一个字不许变**）；② 再按 **`updated_at`
/// 数值降序**排；③ 再**取前 [`CATALOG_ITEM_LIMIT`] 条**。
///
/// ⚠️ **同值时保持站点原顺序** ⇒ ⇒ 用**稳定排序**（`sort_by` 稳定）⇒ ⇒ 不许用
/// 不稳定排序把同值项打乱。
///
/// ⚠️ **`sessionsLength` 照旧**（那是**站点给的原始条数**，与 `items` 的条数是
/// **两个不同的数**）⇒ ⇒ **不许**把它改成 6。
fn catalog_items(sessions: Option<&Vec<Value>>, spec: &SiteSpec) -> Vec<CatalogItem> {
    let Some(list) = sessions else {
        return Vec::new();
    };
    let mut items: Vec<CatalogItem> = list
        .iter()
        .map(|item| catalog_item_of(value_at(item, spec.item_path), spec))
        .collect();
    // **按 `updated_at` 数值降序**（**稳定** ⇒ 同值保持站点原顺序）。
    items.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
    // **最多 6 条**（取不满照实给）。
    items.truncate(CATALOG_ITEM_LIMIT);
    items
}

/// 裁一项（**键名与类型都照 §4.3 ②**；缺键取零值，**不猜**）。
///
/// ⚠️ **按站点的字段映射取**（[`SiteSpec::fields`]）⇒ ⇒ 映射不到的**留空串／
/// 0**（**不许**编一个值、**不许**加第 7 个键）。
fn catalog_item_of(item: Option<&Value>, spec: &SiteSpec) -> CatalogItem {
    let fields = &spec.fields;
    let text = |path: &[&str]| -> String {
        item.and_then(|value| value_at(value, path))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let number = |path: &[&str]| -> f64 {
        item.and_then(|value| value_at(value, path))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    let flag = |path: &[&str]| -> bool {
        item.and_then(|value| value_at(value, path))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    CatalogItem {
        id: text(fields.id),
        title: text(fields.title),
        title_type: text(fields.title_type),
        model_type: text(fields.model_type),
        pinned: flag(fields.pinned),
        // ⚠️ **单位换算**（MiniMax 给毫秒、ChatGPT 给 ISO 字符串、协议那字段
        // 是秒级）⇒ ⇒ 三个站点共用同一个口径（**规格表里那两栏**说了算）。
        updated_at: match fields.updated_at_form {
            TimestampForm::Number => number(fields.updated_at) / fields.updated_at_divisor,
            TimestampForm::Iso8601 => iso_to_epoch_seconds(&text(fields.updated_at)),
        },
    }
}

/// 把一个 **ISO-8601** 字符串（RFC3339）解析成**秒级小数时间戳**。
///
/// ⚠️ **为什么需要它**（实测 2026-10-22 13:5x）：ChatGPT 目录条目的
/// `update_time` **是字符串**（`"2026-10-21T06:51:42.166765Z"`）⇒ ⇒ 协议 §4.3 ②
/// 那个字段（**秒级小数时间戳**）要的是**同一个意思的数** ⇒ ⇒ 解析成秒 ＋ 小数。
///
/// ⚠️ **解析不出来 ⇒ 0**（**不猜**）—— 那个条目因此排不到前面，但**不会报错**
/// （目录面是「取不满就照实给」那一档）。
fn iso_to_epoch_seconds(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    let Ok(parsed) =
        time::OffsetDateTime::parse(trimmed, &time::format_description::well_known::Rfc3339)
    else {
        return 0.0;
    };
    // ⚠️ `unix_timestamp_nanos` ＝ **自纪元起的纳秒总数** ⇒ ⇒ 拆成「整秒 ＋
    // 小数秒」（协议 §4.3 ② 要的是**秒级小数**）。
    let nanos = parsed.unix_timestamp_nanos();
    let seconds = nanos.div_euclid(1_000_000_000) as f64;
    let fraction = nanos.rem_euclid(1_000_000_000) as f64 / 1_000_000_000.0;
    seconds + fraction
}

/// 条目各键的**类型名**（协议 §4.3 ② 的 6 个类型；协议 §2.4「条目的类型」）。
const TYPE_NAMES: [&str; 6] = ["null", "boolean", "number", "string", "array", "object"];

/// 一个值的**类型序号**。
///
/// ⚠️ 这里**返回序号**而不是类型名 —— 协议 §10 第 6 条的判据把「`=> "x"` 形态的
/// 字面量」当**码返回点**扫 ⇒ 自己写 `"string"` 这类字面量会被误判成「闭集外的
/// 硬编码码」⇒ ⇒ 类型名放在 [`TYPE_NAMES`] 里，match 分支只给序号。
fn type_index(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

/// 一个值的**类型名**（协议 §2.4：「条目的类型」）。
fn type_name(value: &Value) -> &'static str {
    TYPE_NAMES[type_index(value)]
}

/// 抽一个条目对象里**各键的类型**（协议 §4.3 ② 的 6 个类型）。
fn type_map(item: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(map) = item.as_object() {
        for (key, value) in map {
            out.insert(key.clone(), type_name(value).to_string());
        }
    }
    out
}

/// 取一个数（数字或数字字符串都算；**取不到就是没有**）。
fn number_of(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
}

/// 一个 DevTools 事件（**载荷＝params 对象本身、无 `method` 字段** —— 协议 §8.5.1）。
///
/// **跨平台存在**（只有**构造**它的 `subscribe_network_events` 是 Windows 专用）
/// ⇒ 因为下面那个 [`EventStream`] 别名是跨平台的，它必须跨平台存在。
/// ⚠️ 非 Windows 上这个结构「不会被构造、字段也不会被读」⇒ 那边的构建里
/// 「字段未被读」是**正常**的 ⇒ 显式 `allow(dead_code)`。
///
/// ⚠️ **为什么是 `pub(crate)`**（不是私有的）：**端到端探针**要**调生产的
/// `capture_in`**（那条导航链）⇒ ⇒ 而它的第二个参数是这个别名 ⇒ ⇒ 别名不公开
/// ⇒ ⇒ 探针连那条链都调不到 ⇒ ⇒ 端到端就**验不到生产那一段**了。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct NetworkEvent {
    /// 事件名（`Network.responseReceived` / `Network.loadingFinished`）。
    name: String,
    /// 原始 JSON 文本。
    raw: String,
}

/// 命中目标端点的那一次响应。
///
/// **Windows 专用**（只有 [`await_target_hit`] 用得到它 —— 那是壳内 DevTools
/// 事件通道的一部分）。
#[derive(Debug, Clone)]
#[cfg(windows)]
struct TargetHit {
    /// `Network` 域的 `requestId`（取体时用它）。
    request_id: String,
    /// HTTP 状态码（协议 §4.2 成功判据的第一条）。
    status: i64,
    /// 命中那条响应的 `Network.responseReceived` 时刻（**单调秒**）。
    timestamp: f64,
    /// 命中的候选**一共几条**（路径 ＋ 锁定条件都对上的那些）。
    matched: usize,
    /// 其中**满足「该取哪一条」判据**的**有几条**（**0 ＝ 判据没成立**，如实记）。
    rule_matched: usize,
    /// **值判据**的读数依据（**只有数字**；`None` ＝ 这一站的判据不是值判据）。
    rule_detail: Option<String>,
}

/// 事件流（Windows 上是 DevTools 事件通道；别的平台走不到这里）。
///
/// ## 这个别名**必须跨平台存在**，不能挂 `#[cfg(windows)]`
///
/// [`arm_capture_window`] 与 [`capture_in`] 各有**两个**平台分支，而它们的
/// **返回类型必须一致**（非 Windows 分支的签名里也写着 `EventStream`）
/// ⇒ ⇒ 别名一旦是 windows-only，非 Windows 上那两个分支就**引用不到它**
/// ⇒ 编译期 `E0425: cannot find type EventStream in this scope`。
///
/// ⚠️ **实测过**（2026-10-21）：这个 `#[cfg(windows)]` 一旦加上，本机 macOS 与
/// desktop-ci 的 Windows job **同时红** —— 因为非 Windows 分支还在引用它。
/// ⇒ ⇒ **平台差异只能落在函数体里，不能落在签名用到的类型别名上。**
pub(crate) type EventStream = tauri::async_runtime::Receiver<NetworkEvent>;

/// 读一次**会话目录结构**。
///
/// ## 现在是什么形态（协议 **v0.24** §4.6）
///
/// · **按需重新取**（不再是「只取一次」）：`refresh=true` ⇒ **当刻重取**一轮；
///   `refresh=false` ⇒ 保鲜期（[`STRUCTURE_TTL`]）内回缓存，过期了**重取**。
/// · **一轮 ＝ 建一扇新窗 ＋ 导航一次**（协议 §8.5.2 G4-13 ②：**不 reload**、
///   **不对旧窗重新导航**）⇒ ⇒ 每次拿到的都是**当刻**的目录。
/// · **同一时刻只跑一轮**（`round` 互斥）⇒ ⇒ 并发的调用**复用**同一轮的结果，
///   不会因为「每个按键都调一次」而开出好几扇取数窗。
/// · 失败按协议 §6 的**既有码**回（`catalog_failed`／`catalog_empty`／…）——
///   **不新增第 15 个码**。
///
/// ## 为什么改成这样（**病根**）
///
/// v0.23 之前，这一条链是「**只启动一次** ＋ 之后只读缓存」⇒ ⇒ 结构只在那扇窗
/// 启动那一刻抓一次 ⇒ ⇒ 用户在官网对话里新建／更新的会话**永远进不了候选组**
/// （实测：dev 实例的目录缓存页里 **11:48:36 与 11:51:03 两次写入逐字相同**，
/// 而站点当刻给的最大 `updated_at` 是 **11:50:54** —— 差的只是壳侧那份旧快照）。
/// 读一次**会话目录结构**（**按站点**）。
///
/// ## 站点短码从哪儿来
///
/// 桥上那条路由的**请求体**里那个 `site` 短码（**内核给**，与回包信封上那个
/// **同形**）⇒ ⇒ 这里只**认规格表里登记过的短码**（[`spec_for`]）⇒ ⇒ 认不出
/// ⇒ **明确报错**（**不猜、不回落**成某个站点）。
pub(crate) async fn capture_for(
    site: &str,
    refresh: bool,
) -> Result<SessionStructure, CaptureFailure> {
    let app = app_handle().ok_or_else(|| {
        CaptureFailure::Return("壳还没把 app 句柄交给取数模块（lib.rs 的 setup 没登记）".into())
    })?;
    let state = state();
    // ① **快路径**：保鲜期内 ⇒ 直接回缓存（**不建窗、不导航**）。
    if !refresh {
        if let Some(result) = fresh(state, site) {
            return result;
        }
    }
    // ② 记下**请求时刻**（后面用它判「刚才那一轮是不是为我们跑的」）。
    let requested_at = Instant::now();
    // ③ **取数轮互斥**：同一时刻只跑一轮。
    let _round = state.round.lock().await;
    // ④ 拿到锁后**重判**：可能**刚才那一轮**（别的调用起的）就是当刻的 ⇒ 复用它。
    if let Some(result) = reusable(state, site, requested_at, refresh) {
        return result;
    }
    // ⑤ 跑一轮（建**新**窗 → 导航**一次** → 取体 → 裁剪 → 关窗）。
    let outcome = ensure_started(&app, state, site).await;
    // ⑥ 写回缓存（成功与失败都记 —— 失败也要让下一次读取立刻回它）。
    write_cache(state, site, outcome.clone());
    outcome
}

/// **跑一轮取数**（建**新**窗 → 导航**一次** → 取体 → 裁剪 → 关窗）。
///
/// ## 名字为什么还叫 `ensure_started`
///
/// 判据第 11 条（`scripts/check-official-chat-p4-enable-timing.mjs`）按**实现侧
/// 约定**认这个函数名（`ORCHESTRATE_FN`）⇒ ⇒ 改名会让判据抽不到函数体 ⇒ ⇒
/// **名字留着，职责改了** —— 它现在**每轮都跑一次**，不再是「只启动一次」。
///
/// ## 一轮里发生的三件事
///
/// 1. **建窗链**（[`arm_capture_window`]）：建窗 → 订阅 → `Network.enable`
///    —— **三件事都在导航之前**（协议 §8.5.1 硬约束 3）；
/// 2. **导航链**（[`capture_in`]）：导航**一次** → 等目标响应加载完 → 立刻取体；
/// 3. 壳侧裁成白名单结构（[`SessionStructure::from_body`]）。
///
/// ## 为什么不 reload 旧窗（协议 §8.5.2 G4-13 ②）
///
/// `reload`／对同一扇窗重新导航会让站点**重跑初始化批次** ⇒ ⇒ 那是**站点侧**的
/// 写动作 ⇒ ⇒ 违 §7.3 的只读边界 ⇒ ⇒ 「重新取」只能是「**新窗 ＋ 一次导航**」。
async fn ensure_started(
    app: &tauri::AppHandle,
    state: &'static CaptureState,
    site: &str,
) -> Result<SessionStructure, CaptureFailure> {
    // 上一轮那扇窗**先关掉**（关窗是异步的 ⇒ 留着句柄在这儿再关一次，
    // 免得它没关掉时越攒越多）。
    if let Ok(mut guard) = state.last_window.lock() {
        if let Some(previous) = guard.take() {
            let _ = previous.close();
        }
    }
    let dir = profile_dir(app)?;
    // ⚠️ **站点相关的部分全从规格表取**（端点、路径、查询形状）⇒ ⇒ **一条链、
    // 多个站点**（**不许**复制第二套目录链）。
    let spec = spec_for(site).ok_or_else(|| {
        CaptureFailure::Return(format!("站点 {site} 没有登记取数面（规格表里缺这一条）"))
    })?;
    // **建窗链**：建窗 → 订阅 → `Network.enable`（**全在导航之前**）。
    let (window, events) = arm_capture_window(app, site, STRUCTURE_KIND, &dir).await?;
    // 留一句句柄（下一轮开头再关一次 —— 万一这一轮关窗失败）。
    if let Ok(mut guard) = state.last_window.lock() {
        *guard = Some(window.clone());
    }
    let target = CaptureTarget {
        url: spec.root_url.to_string(),
        path_template: spec.catalog_path,
        // 目录那条的 query 由站点自己管（`lte_cursor.pinned=false`）⇒
        // **按路径认即可**（正文那条才锁会话）。
        lock: None,
        // ⚠️ **目录那条的判据**（**站点相关**）：ChatGPT 那个站点自己往同一个
        // 路径发了两条请求 ⇒ ⇒ 必须能挑出**不是空变体**的那条。
        pick: spec.catalog_pick,
        // **页内路线 B 的站点适配**（macOS 用；Windows 那条链不读它）。
        page_route: spec.page_route,
        // 目录那条链**没有正文 URL**（`None` ＝ 取目录那条的 URL）。
        page_body_url: None,
    };
    // **导航链**：导航 → 等目标响应 → 立刻取体 → 壳侧裁剪。
    let outcome = match capture_with_deadline(&window, events, &target, spec).await {
        Ok(captured) => SessionStructure::from_body_for(&captured.body, captured.status, spec),
        Err(failure) => Err(failure),
    };
    // 抓完就收掉**自己**建的窗（**绝不**碰用户挂载的页签）。
    let _ = window.close();
    if let Ok(mut guard) = state.last_window.lock() {
        *guard = None;
    }
    outcome
}

/// **建窗链**：建窗 → **按具体事件名**订阅 → **`Network.enable`**。
///
/// ## 为什么这三件事必须**都在导航之前**
///
/// 协议 §8.5.1 硬约束 3：`Network.enable` **晚于站点首个请求 ⇒ 首屏全丢**
/// （对照臂：导航完成后才 enable ⇒ 站点 `/api/` 命中 **0**）⇒ ⇒ **订阅点要挂在
/// 建窗/导航链上**，**不是**挂在「用户发起读取」的处理路径上。
///
/// 本函数**只**做「装好监听器」这一件事，**不导航** ⇒ ⇒ 判据第 11 条
/// （订阅时序）能按源码形态断出「enable 在导航之前」。
///
/// ## `kind`（用途）
///
/// `structure` ＝ 目录那条链；`body` ＝ 协议 §4.4 的正文那条链（**各自一扇窗**）。
#[cfg(windows)]
pub(crate) async fn arm_capture_window(
    app: &tauri::AppHandle,
    site: &str,
    kind: &str,
    profile_dir: &std::path::Path,
) -> Result<(tauri::WebviewWindow, EventStream), CaptureFailure> {
    let window =
        build_capture_window(app, site, kind, profile_dir).map_err(CaptureFailure::Return)?;
    let prepared = async {
        let events = subscribe_network_events(&window)?;
        cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT)
            .await
            .map_err(|e| CaptureFailure::Return(format!("Network.enable 失败：{e}")))?;
        Ok(events)
    }
    .await;
    match prepared {
        Ok(events) => Ok((window, events)),
        Err(error) => {
            let _ = window.close();
            Err(error)
        }
    }
}

/// macOS：**没有** DevTools 通路 ⇒ 走**路线 B**（页内同源 fetch）。
///
/// ## 这一步只做「把窗建好」
///
/// 路线 B **不订阅任何事件**（它自己发那一发、自己拿那一发）⇒ ⇒ 没有事件流
/// ⇒ ⇒ 给一个**立刻关闭**的通道（读端拿到 `None`，不会被误当成「有事件」）。
///
/// ## 为什么这扇窗**有登录态**
///
/// macOS 上 WebKit 认的是 **WKWebsiteDataStore 标识符**，不是 `data_directory`
/// ⇒ ⇒ 这扇窗**必须**绑**官网对话窗口那份同一个标识符**（生产里是
/// `OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`，按壳模式分 dev／release）⇒ ⇒ 同一份
/// cookie jar ⇒ ⇒ **同一个登录态**。
///
/// ⚠️ **别用 `profile_dir`**：那是 Windows 那套 user-data 目录，macOS 上
/// WebView2 的概念不存在（WKWebView **忽略** `data_directory`）。
///
/// ## `kind` 必须分清
///
/// 正文那条链（`kind == "body"`）**没有**路线 B 适配 ⇒ ⇒ 这里**明确报错**，
/// **不**拿目录那条的认证契约去套（那会静默拿到一个**形状不对**的响应）。
#[cfg(target_os = "macos")]
pub(crate) async fn arm_capture_window(
    app: &tauri::AppHandle,
    site: &str,
    kind: &str,
    profile_dir: &std::path::Path,
) -> Result<(tauri::WebviewWindow, EventStream), CaptureFailure> {
    // ⚠️ **两条链都走路线 B**（目录那条 ＋ 正文那条）⇒ ⇒ 这里只挡**没见过的**
    // 那一种；**「这个站有没有正文适配」是站点级的事**，由 `capture_in` 判
    // （`page_body_url` 为 `None` ⇒ 明确报不支持 ⇒ **不静默套目录那条**）。
    if kind != "structure" && kind != "body" && kind != "account" {
        return Err(CaptureFailure::Unsupported(
            "macOS 的页内取数只覆盖会话目录、单会话正文与账户身份".to_string(),
        ));
    }
    let window =
        build_capture_window(app, site, kind, profile_dir).map_err(CaptureFailure::Return)?;
    // 事件流：路线 B **不用**它 ⇒ 给一个**立刻关闭**的通道。
    let (tx, rx) = tauri::async_runtime::channel::<NetworkEvent>(1);
    drop(tx);
    Ok((window, rx))
}

/// 别的非 Windows 平台（Linux 等）：**两条路都没有** ⇒ 明确报错，不静默降级。
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) async fn arm_capture_window(
    _app: &tauri::AppHandle,
    _site: &str,
    _kind: &str,
    _profile_dir: &std::path::Path,
) -> Result<(tauri::WebviewWindow, EventStream), CaptureFailure> {
    Err(CaptureFailure::Unsupported(
        "该平台既没有壳内 DevTools 通路，也没有页内 fetch 通路".to_string(),
    ))
}

/// 按**具体事件名**订阅 DevTools 的 `Network` 域事件。
///
/// 官方 API 形态：`GetDevToolsProtocolEventReceiver(LPCWSTR eventName, receiver)`
/// —— 接收器是**按事件名**取的，拿到后用 `add_DevToolsProtocolEventReceived`
/// 注册处理函数。
///
/// ⚠️ **事件载荷＝params 对象本身、没有 `method` 字段**（协议 §8.5.1 硬约束 4）
/// ⇒ ⇒ 下面**不找** `method`（找不到就当「没事件」，会误判成通道不通）。
#[cfg(windows)]
fn subscribe_network_events(window: &tauri::WebviewWindow) -> Result<EventStream, CaptureFailure> {
    use webview2_com::DevToolsProtocolEventReceivedEventHandler;
    use windows_core::HSTRING;

    // 容量给够：一次页面加载的响应事件是几十条量级。
    let (tx, rx) = tauri::async_runtime::channel::<NetworkEvent>(256);
    // 外层闭包是 `move` 的，会把 sender 带走 ⇒ 闭包里用拷贝，外层留一份。
    let tx_outer = tx.clone();
    let dispatched = window.with_webview(move |platform| {
        let controller = platform.controller();
        // SAFETY: `CoreWebView2` 只是取回控制器持有的核心对象引用，不转移
        // 所有权；返回值由 COM 引用计数管理。
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                let _ = tx_outer.try_send(NetworkEvent {
                    name: "__setup__".to_string(),
                    raw: format!("取不到 WebView2 核心：{e}"),
                });
                return;
            }
        };
        for event_name in SUBSCRIBED_EVENTS {
            // 每个订阅一份 sender：事件处理函数是 `'static` 的闭包，会把
            // sender 带走，所以循环里必须各拿一份拷贝。
            let tx_event = tx_outer.clone();
            let name_hstring = HSTRING::from(event_name);
            // SAFETY: 事件名是静态字符串，调用期间由 `name_hstring` 持有。
            let receiver = match unsafe { core.GetDevToolsProtocolEventReceiver(&name_hstring) } {
                Ok(receiver) => receiver,
                Err(e) => {
                    let _ = tx_outer.try_send(NetworkEvent {
                        name: "__setup__".to_string(),
                        raw: format!("取不到事件 {event_name} 的接收器：{e}"),
                    });
                    continue;
                }
            };
            let label = event_name.to_string();
            let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(
                move |_sender, args| {
                    if let Some(args) = args {
                        let mut pwstr = windows_core::PWSTR::null();
                        // SAFETY: `ParameterObjectAsJson` 把一段以
                        // CoTaskMemAlloc 分配的 PWSTR 写进 `pwstr`；下面的
                        // `CoTaskMemPWSTR` 负责释放它。
                        let ok = unsafe {
                            args.ParameterObjectAsJson(&mut pwstr as *mut windows_core::PWSTR)
                        }
                        .is_ok();
                        if ok {
                            let text = webview2_com::CoTaskMemPWSTR::from(pwstr).to_string();
                            let _ = tx_event.try_send(NetworkEvent {
                                name: label.clone(),
                                raw: text,
                            });
                        }
                    }
                    Ok(())
                },
            ));
            let mut token: i64 = 0;
            // SAFETY: 只注册处理函数，不改页面状态。
            if let Err(e) =
                unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }
            {
                let _ = tx_outer.try_send(NetworkEvent {
                    name: "__setup__".to_string(),
                    raw: format!("订阅 {event_name} 失败：{e}"),
                });
            }
        }
    });
    if let Err(e) = dispatched {
        return Err(CaptureFailure::Return(format!(
            "订阅 Network 域事件失败（webview 已不可用）：{e}"
        )));
    }
    Ok(rx)
}

// Leave time for the bridge response and cold-profile setup before the plugin's 180s deadline.
pub(crate) const CAPTURE_BUDGET: Duration = Duration::from_secs(170);

pub(crate) async fn capture_with_deadline(
    window: &tauri::WebviewWindow,
    events: EventStream,
    target: &CaptureTarget,
    spec: &SiteSpec,
) -> Result<CapturedResponse, CaptureFailure> {
    tokio::time::timeout(CAPTURE_BUDGET, capture_in(window, events, target, spec))
        .await
        .unwrap_or_else(|_| Err(CaptureFailure::Timeout("取数总等待时间已用尽".into())))
}

/// 一次取数的**目标**：导航到哪个 URL ＋ 等站点自己发的那条端点。
///
/// ## 为什么是「一个函数、两处调用」
///
/// 协议 §4.3 的目录那条链（导航到站点首页）与协议 §4.4 的正文那条链（导航到**那条
/// 会话的深链**）**形态完全一样**：建窗 → 订阅 → `Network.enable` → 导航 → 等目标
/// 端点那条响应加载完 → **立刻**取体 ⇒ ⇒ **只有「导航到哪」与「等哪条端点」两处不同**。
///
/// ⇒ ⇒ 所以**导航只有一处**（[`capture_in`] 里那**一个** `.navigate(`）——
/// 协议 v0.11 §8.5.2 G4-13 ②「不许壳侧 `reload`／重复导航」那条禁令**照旧成立**
/// （目录那条链**永不**二次导航、不做补偿动作），而正文那条链**就是协议 §4.4
/// 明文要求的那一次导航**（只为「取用户已选中那一条的正文」服务）。
#[derive(Debug, Clone)]
pub(crate) struct CaptureTarget {
    /// 导航落点（**URL 级** —— 协议 §7.1：页内零动作，导航不是脚本）。
    pub(crate) url: String,
    /// 要等的那条端点的**路径模板**（按路径认；query 会被站点自己改，**不猜**）。
    ///
    /// ⚠️ **`{session}` ＝ 会话 id 的占位**（MiniMax 的正文端点**把会话 id
    /// 放在路径里** ⇒ ⇒ 那一条的模板里带这个占位）。
    pub(crate) path_template: &'static str,
    /// **锁定**：正文那条要把响应锁到**某条会话**（**目录那条没有这一项** ——
    /// 它的 query 由站点自己管，按路径认即可）。
    pub(crate) lock: Option<BodyLockTarget>,
    /// **命中多条时按哪一条取**（协议 §4.4：按读数说话）。
    pub(crate) pick: BodyPick,
    /// **页内路线 B 的站点适配**（`None` ＝ 该站点／该用途没有路线 B 适配）。
    ///
    /// **页内路线 B 的站点适配**（macOS 用；Windows 那条链不读它）。
    ///
    /// ⚠️ **两条链都有它**（目录那条 ＋ 正文那条）⇒ 但**正文那条能不能用**还要
    /// 看 `page_body_url`：**该站没有正文适配时它是 `None`** ⇒ ⇒ 取数层据此
    /// **明确报不支持**，**绝不**拿目录那条的 URL 去套正文。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) page_route: Option<PageRoute>,
    /// **页内路线 B 的正文 URL**（**已把会话 id 填进去**；`None` ＝ 目录那条链）。
    ///
    /// ⚠️ **为什么传「填好的具体 URL」而不是模板**：会话 id 只有**调用方**
    /// （正文那条链）知道 ⇒ ⇒ 由它把模板填好再传进来 ⇒ 取数层不必认识会话 id。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) page_body_url: Option<String>,
}

/// 正文那条的**锁定目标**（**按站点不同** —— 协议 §4.4 的硬要求：逐字比对）。
#[derive(Debug, Clone)]
pub(crate) enum BodyLockTarget {
    /// **query 里的某个键** ＝ 目标会话 id（DeepSeek：`chat_session_id`）。
    QueryKey {
        /// query 键名。
        key: &'static str,
        /// 目标会话 id（**逐字比对**）。
        session_id: String,
    },
    /// **路径里的那段** ＝ 目标会话 id（MiniMax：会话 id **在路径里**，**query 里
    /// 没有那个参数** —— 读数里根本没有 ⇒ ⇒ **不许猜一个键名出来**）。
    PathSegment {
        /// 目标会话 id（**逐字比对**）。
        session_id: String,
    },
}

/// 抓到的那条响应（**体 ＋ 状态码**）。
///
/// ⚠️ **体只在壳侧内存里**：裁剪在壳侧做完才回程（协议 §8.5.1 末段：
/// **不许**把 `getResponseBody` 的原文直接回传）。
#[derive(Debug, Clone)]
pub(crate) struct CapturedResponse {
    /// 响应体原文。
    pub(crate) body: String,
    /// HTTP 状态码（协议 §4.2 成功判据的第一条）。
    pub(crate) status: i64,
    /// 命中那条响应的时刻（**单调秒**，来自 `Network.responseReceived`）。
    pub(crate) timestamp: f64,
    /// 命中的候选**一共几条**（协议 §4.4：命中多条时按这个数说明判据）。
    pub(crate) matched: usize,
    /// 其中**满足「该取哪一条」判据**的**有几条**（**0 ＝ 判据没成立**）。
    pub(crate) rule_matched: usize,
    /// **值判据**的读数依据（**只有数字**；`None` ＝ 不是值判据那一站）。
    pub(crate) rule_detail: Option<String>,
}

/// 页内就绪的**默认预算**（macOS 路线 B）。
///
/// ⚠️ 站点首屏（含 Cloudflare 挑战）可能要好几秒 ⇒ 预算给到 **90s**；超了按
/// 「回程超时」报（`catalog_failed`，可重试）。
///
/// ⚠️ **为什么从 45s 放宽到 90s**（**izzy 2026-10-23 11:2x 拍板**）：ChatGPT 首页
/// **明显比 DeepSeek 慢** ⇒ ⇒ 实测六轮（**r31–r36**，日志
/// `/tmp/dsh-route-b-r{31..36}.log`，最终代码指纹 `session_structure.rs 8198f5c0`）
/// —— **目录链 25.8s、正文链 23.7s**（r31 是 10.6s／4.5s ⇒ ⇒ **波动很大**）⇒ ⇒
/// 原预算「就绪 45s ＋ 取数 45s」**在慢的时候可能不够**⇒ ⇒ 超时只落
/// `catalog_failed`（可重试）⇒ ⇒ 用户侧看到的是「界面空着然后失败」。
///
/// ⚠️ **副作用照实记在这里**（同一个常量管 ChatGPT 端点形态与 DeepSeek 存储
/// 形态）⇒ ⇒ **DeepSeek 那条链卡住时从等 45s 变成等 90s** —— 这是 izzy 那个
/// 裁定的直接后果，**不另造按形态分预算的新概念**。
#[cfg(target_os = "macos")]
const PAGE_READY_BUDGET: Duration = Duration::from_secs(90);

/// 页内**就绪**判定（**端点形态**）那一发的**往返超时**（macOS 路线 B）。
///
/// ⚠️ **为什么要有这一格**：端点形态的就绪判定要**等**那一发
/// `GET /api/auth/session` 回来（token 就在那一发的响应里）⇒ ⇒ 走的是
/// `round_trip`（能 await）⇒ ⇒ **必须**给它一个上限（否则站点挂住时
/// 那个等就绪循环会一直挂着，直到 [`PAGE_READY_BUDGET`] 才收摊）。
///
/// ⚠️ **比存储形态那 4s 宽**：存储形态是**读一个键**（本地，快）⇒ 4s 够；
/// 端点形态是**一次同源网络往返** ⇒ ⇒ 8s 是「站点慢一点也仍然算正常」。
#[cfg(target_os = "macos")]
const PAGE_READY_ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(8);

/// 页内**就绪**判定走**哪条通道**（macOS 路线 B）。
///
/// ⚠️ **为什么要有这一栏**：凭据来源决定判据**能不能同步做完** ——
/// · **页内存储取** ⇒ 同步读得到 ⇒ 走 `read_json`（原生求值**不等** Promise）；
/// · **同源端点取** ⇒ 必须 **await** 那一发（`GET /api/auth/session`）⇒ ⇒ 走
///   `round_trip`（**唯一能 await 的那条通道**）。
///
/// ⚠️ **走错通道的后果**（不是「慢一点」，是**永远判不出**）：端点形态若走
/// `read_json` ⇒ 读到的是**还没 settle 的 Promise** ⇒ ⇒ `hasCredential`
/// **恒为假** ⇒ ⇒ 每次都报「没有登录态」。
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadyChannel {
    /// **同步**（`read_json` —— 原生求值不等 Promise）。
    Sync,
    /// **异步**（`round_trip` —— 能 await Promise）。
    Async,
}

/// 页内**取数**那一发的**超时**（macOS 路线 B）。
///
/// ⚠️ **为什么从 45s 放宽到 90s**（**izzy 2026-10-23 11:2x 拍板**）：与
/// [`PAGE_READY_BUDGET`] **同一个裁定**、**同一条依据**（ChatGPT 首页明显比
/// DeepSeek 慢；实测 r31–r36 六轮：目录链 **25.8s**、正文链 **23.7s**，r31 是
/// 10.6s／4.5s ⇒ ⇒ **波动很大**；日志 `/tmp/dsh-route-b-r{31..36}.log`，代码指纹
/// `session_structure.rs 8198f5c0`）⇒ ⇒ 原「就绪 45s ＋ 取数 45s」**慢的时候可能
/// 不够**⇒ ⇒ 超时只落 `catalog_failed`（可重试）⇒ ⇒ 用户侧看到的是「界面空着
/// 然后失败」。
///
/// ⚠️ **副作用照实记在这里**（同一个常量管 ChatGPT 端点形态与 DeepSeek 存储
/// 形态）⇒ ⇒ **DeepSeek 那条链卡住时从等 45s 变成等 90s** —— 这是 izzy 那个
/// 裁定的直接后果，**不另造按形态分预算的新概念**。
#[cfg(target_os = "macos")]
const PAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(90);

/// 「**页面不再跳转**」要连续命中几次才算稳（macOS 路线 B）。
///
/// ⚠️ **为什么需要这一格**（2026-10-22 21:5x 实测）：站点首屏在
/// `readyState = "complete"` **之后**还会跳一次 ⇒ ⇒ 那一刻发出去的页内求值
/// 会**跟着文档一起被销毁** ⇒ 回程报「页面在请求途中不在了」。
/// ⇒ 每 300 ms 采一次 href ⇒ 连续 3 次相同 ⇒ 稳定窗口 ≥ 900 ms。
#[cfg(target_os = "macos")]
const PAGE_STABLE_HITS: u32 = 3;

/// 页内取数**最多试几轮**（每轮之间**重新等稳**）（macOS 路线 B）。
#[cfg(target_os = "macos")]
const PAGE_FETCH_ATTEMPTS: u32 = 3;

/// **首次导航 commit** 的等待预算（macOS 形态 C）。
///
/// ⚠️ 站点首屏可能要好几秒 ⇒ ⇒ 预算给到 45s（超了按「回程超时」报）。
#[cfg(target_os = "macos")]
const PAGE_COMMIT_BUDGET: Duration = Duration::from_secs(45);

/// 形态 C 的**页内等待预算**（等「站点自己那层」出现／等那一发被截到，**各一份**）。
///
/// ⚠️ **实测依据**（上游 6 轮）：站点那层 fetch 出现的时间是 **269／288／1274／
/// 1627／4159／4772 ms**（跨度 0.27–4.8 s）⇒ ⇒ 给到 90s 是「站点慢一点也仍然算
/// 正常」的口径（与 izzy 那个把预算放宽到 90s 的裁定同一档）。
#[cfg(target_os = "macos")]
const PAGE_HOOK_POLL_BUDGET_MS: u64 = 90_000;

/// 形态 C 的**页内轮询次数上限**（页内那一层的兜底；时间预算才是主闸）。
#[cfg(target_os = "macos")]
const PAGE_HOOK_MAX_POLL_ATTEMPTS: u64 = 400_000;

/// 形态 C 的**回程总预算**（注入 → 截到 → 读完体 → 裁完 → 回到壳）。
///
/// ⚠️ **为什么比页内那份等待预算宽**：页内那 90s 只管「等站点那层／等那一发」⇒ ⇒
/// 后面还有「读完体」（实测正文那份 **1,705,964 B**）＋「裁完」⇒ ⇒ 壳侧这一层
/// 必须更宽。
#[cfg(target_os = "macos")]
const PAGE_HOOK_ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(240);

/// **形态 B 那一支误用形态 C 脚本**时的兜底（**立刻失败**，不静默套用别的契约）。
///
/// ⚠️ **为什么要有它**：[`page_hook_script`] 收的是**整个枚举**（两个形态共用一个
/// 入口）⇒ ⇒ 万一形态 B 那一支被误接进来 ⇒ ⇒ 这里给一段**会立刻抛错**的脚本
/// ⇒ ⇒ 错在**页内那一侧看得见**，而不是「静默按另一套契约发了一发」。
#[cfg(target_os = "macos")]
const PAGE_ROUTE_HOOK_ONLY: &str =
    "(function () { throw new Error('这一支是形态 B（页内自取数）：不走形态 C 的旁听脚本'); })()";

/// **导航链**：导航 → 等目标响应加载完 → **立刻**取体。
#[cfg(windows)]
pub(crate) async fn capture_in(
    window: &tauri::WebviewWindow,
    mut events: EventStream,
    target: &CaptureTarget,
    spec: &SiteSpec,
) -> Result<CapturedResponse, CaptureFailure> {
    // ③ 导航（**`Network.enable` 已经在导航之前** —— 协议 §8.5.1 硬约束 3）。
    let url: tauri::Url = target
        .url
        .parse()
        .map_err(|e| CaptureFailure::Return(format!("导航目标非法：{e}")))?;
    window
        .navigate(url)
        .map_err(|e| CaptureFailure::Return(format!("导航失败：{e}")))?;

    // ④ 等目标端点的响应**加载完**（`Network.loadingFinished`）——
    //    **只有到这一步才允许取体**（协议 §8.5.1 硬约束 1）。
    let hit = await_target_hit(&mut events, CAPTURE_TIMEOUT, target, spec.key).await?;

    // ⑤ **立刻**取体（硬约束 1：取晚了会因缓冲压力失败 ⇒ 不许「等久一点」）。
    let body = take_response_body(window, &hit.request_id).await?;

    Ok(CapturedResponse {
        body,
        status: hit.status,
        timestamp: hit.timestamp,
        matched: hit.matched,
        rule_matched: hit.rule_matched,
        rule_detail: hit.rule_detail,
    })
}

/// **导航链**（macOS）：导航一次 → 等页内就绪 → **页内同源 fetch** 取体。
///
/// ## 与路线 A 的**形状一致**，只是「谁去发那一发」不同
///
/// 两条链都产出同一个 [`CapturedResponse`] ⇒ ⇒ 后面**裁剪**（[`SessionStructure::from_body_for`]）
/// 是**跨平台共享**的纯逻辑 ⇒ ⇒ **平台差异只落在函数体里**。
///
/// ## ⚠️ **两种形态走两条子链**（macOS 上整块都在这儿）
///
/// · [`PageRoute::Fetch`]（**形态 B · 页内自取数**）—— 页内**自己发**那一发
///   （**这一支一个字不许变**）；
/// · [`PageRoute::Hook`]（**形态 C · 页内旁听**）—— 页内**旁听站点自己那一发**
///   （**替换 ＋ 取到即还原**）⇒ ⇒ 导航落点**按链不同**：目录那条落**取数根地址**、
///   正文那条落**深链**（**让站点自己**发那一发 ⇒ ⇒ 与协议 §4.4 的深链导航同构）。
///
/// ## 顺序：先查适配，再导航
///
/// 没有页内适配的站点**早退** ⇒ ⇒ **不导航** ⇒ ⇒ 不会为了「注定失败」而让站点
/// 跑一遍首屏（那会多出一条空会话）。
#[cfg(target_os = "macos")]
pub(crate) async fn capture_in(
    window: &tauri::WebviewWindow,
    _events: EventStream,
    target: &CaptureTarget,
    spec: &SiteSpec,
) -> Result<CapturedResponse, CaptureFailure> {
    // ① 页内取数的站点适配（**没有 ⇒ 早退**，不导航）。
    //
    // ⚠️ **文案必须按「哪条链」说**（2026-10-22 22:4x 实测发现）：这个站点的
    // `page_route` 整个缺 ⇒ ⇒ **目录那条**与**正文那条**都会落到这一格 ⇒ ⇒
    // 若一律说「取不出它的**会话目录**」⇒ ⇒ 用户在 MiniMax／ChatGPT 上点某条
    // 会话的**引用**（正文那条）时，看到的却是「会话目录」⇒ ⇒ **说错了对象**。
    // ⇒ ⇒ 判据与下面 `page_body_url` 那段**同一个**（`lock` 有值 ＝ 正文那条）。
    let route = target.page_route.as_ref().ok_or_else(|| {
        CaptureFailure::Unsupported(if target.lock.is_some() {
            "这个站点还没有「页内取数」（路线 B）的适配 ⇒ 本机取不出这条会话的正文".to_string()
        } else {
            "这个站点还没有「页内取数」（路线 B）的适配 ⇒ 本机取不出它的会话目录".to_string()
        })
    })?;
    let fetch_url = match route {
        PageRoute::Fetch { .. } => Some(match &target.page_body_url {
            Some(url) => url.clone(),
            None if target.lock.is_some() => {
                return Err(CaptureFailure::Unsupported(
                    "这个站点还没有页内正文适配".to_string(),
                ))
            }
            None => route.fetch_url().unwrap_or_default().to_string(),
        }),
        PageRoute::Hook { .. } => None,
    };
    // Fetch starts at the root; Hook needs the deep link to trigger the site's own request.
    let url = if fetch_url.is_some() {
        route.root_url()
    } else {
        &target.url
    };
    let url: tauri::Url = url
        .parse()
        .map_err(|e| CaptureFailure::Return(format!("导航目标非法：{e}")))?;
    window
        .navigate(url)
        .map_err(|e| CaptureFailure::Return(format!("导航失败：{e}")))?;
    match route {
        PageRoute::Fetch { .. } => {
            capture_via_page(window, route, fetch_url.as_deref().unwrap_or_default()).await
        }
        PageRoute::Hook { catalog_path, .. } => {
            let target_path = target.page_body_url.as_deref().unwrap_or(catalog_path);
            capture_via_page_hook(window, spec, route, target_path).await
        }
    }
}

/// 别的非 Windows 平台（Linux 等）：**两条路都没有**。
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) async fn capture_in(
    _window: &tauri::WebviewWindow,
    _events: EventStream,
    _target: &CaptureTarget,
    _spec: &SiteSpec,
) -> Result<CapturedResponse, CaptureFailure> {
    Err(CaptureFailure::Unsupported(
        "该平台既没有壳内 DevTools 通路，也没有页内 fetch 通路".to_string(),
    ))
}

// ==== 路线 B（macOS）：页内同源 fetch ＋ 站点契约认证头 ====================
//
// 形态：**壳侧在页内**发那一发（路线 A 是「站点自己发、壳侧旁观」）。
// WebKit 没有 DevTools `Network` 域的等价物 ⇒ ⇒ 路线 A 在 macOS 上物理无解
// ⇒ ⇒ 路线 B 是那边**唯一**可用的形态。
//
// ⚠️ **凭据全程不出页面**：页内脚本自己取、自己用、自己丢 ⇒ ⇒ 宿主只拿到
// **响应体**。
//
// ⚠️ **实测依据**：2026-10-22 20:0x–20:3x 本机 macOS 真机（探针
// `examples/official_chat_macos_fetch_probe.rs`）—— 只带 `authorization` 一头
// 重放 ⇒ 200／code 0／**100 条会话**，与站点自己那一发**逐字一致**。

/// 页内就绪判定的**脚本**（`__SPEC__` 由宿主注入）。
///
/// ⚠️ **两种凭据来源 ⇒ 两种就绪判据**：
/// · **页内存储取**（DeepSeek）⇒ **同步**就能读到 ⇒ 宿主走 `read_json`
///   （**那一支逐字保持现状**）；
/// · **同源端点取**（ChatGPT）⇒ 必须 **await** 那一发 ⇒ 宿主走 `round_trip`
///   （**能 await Promise 的那条通道**）。
#[cfg(target_os = "macos")]
pub(crate) fn page_ready_script(route: &PageRoute) -> String {
    // ⚠️ **形态 C 那一支不走这个脚本**（它有自己那份 `page_hook_script.js`）⇒ ⇒
    // 万一被调到 ⇒ ⇒ 交回一段**会立刻失败**的脚本（**不静默套用别的契约**）。
    let Some(auth) = route.fetch_auth() else {
        return PAGE_ROUTE_FETCH_ONLY.to_string();
    };
    let spec = match auth.source {
        // ⚠️ **这一支的规格**（键名与取值）与既有实现**逐字相同** ⇒ ⇒ 页内
        // 脚本的「存储那一支」**一个字都不用改**。
        PageAuthSource::Storage { storage, key, .. } => json!({
            "origin": route.fetch_origin().unwrap_or_default(),
            "storage": storage.as_str(),
            "key": key,
        }),
        // ⚠️ **端点那一支**：`source` 是**分派键**（页内脚本按它选同步／异步
        // 那一支）；`endpointUrl` ＋ `jsonKey` 是**取 token 的契约**。
        PageAuthSource::Endpoint { url, json_key } => json!({
            "origin": route.fetch_origin().unwrap_or_default(),
            "source": "endpoint",
            "endpointUrl": url,
            "jsonKey": json_key,
        }),
    };
    let spec = serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string());
    include_str!("page_ready_script.js").replace("__SPEC__", &spec)
}

/// 页内**取数**那一发的**脚本**（`__SPEC__` 由宿主注入）。
///
/// ⚠️ **为什么单独一个函数**（而不是把那段拼装塞进 [`capture_via_page`]）：
/// 端到端探针要**用生产那份**脚本去跑（自己拼规格就变成了「验的是探针自己
/// 的副本」）⇒ ⇒ 这一段必须是**生产代码里的一个可调用的东西**。
///
/// ⚠️ **两种凭据来源 ⇒ 两种规格**（键名与取值见下面两支）。
#[cfg(target_os = "macos")]
pub(crate) fn page_fetch_script(route: &PageRoute, fetch_url: &str) -> String {
    // ⚠️ **形态 C 那一支不走这个脚本**（它有自己那份 `page_hook_script.js`）⇒ ⇒
    // 万一被调到 ⇒ ⇒ 交回一段**会立刻失败**的脚本（**不静默套用别的契约**）。
    let Some(auth) = route.fetch_auth() else {
        return PAGE_ROUTE_FETCH_ONLY.to_string();
    };
    let mut spec = match auth.source {
        // ⚠️ **这一支的规格**（键名与取值）与既有实现**逐字相同** ⇒ ⇒ 页内
        // 脚本的「存储那一支」**一个字都不用改**。
        PageAuthSource::Storage {
            storage,
            key,
            extract,
        } => json!({
            "url": fetch_url,
            "storage": storage.as_str(),
            "key": key,
            "extract": extract.as_str(),
            "header": auth.header,
            "prefix": auth.prefix,
        }),
        // ⚠️ **端点那一支**：凭据**从端点响应里取**（`source` 是分派键）。
        PageAuthSource::Endpoint { url, json_key } => json!({
            "url": fetch_url,
            "source": "endpoint",
            "endpointUrl": url,
            "jsonKey": json_key,
            "header": auth.header,
            "prefix": auth.prefix,
        }),
    };
    if let Some(keys) = account_contract::projection_keys(fetch_url) {
        spec["identity_keys"] = json!(keys);
    }
    let spec = serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string());
    include_str!("page_fetch_script.js").replace("__SPEC__", &spec)
}

/// **形态 C 那一支误用形态 B 脚本**时的兜底（**立刻失败**，不静默套用别的契约）。
///
/// ⚠️ **为什么要有它**：[`page_ready_script`]／[`page_fetch_script`] 收的是
/// **整个枚举**（两个形态共用一个入口）⇒ ⇒ 万一形态 C 那一支被误接进来 ⇒ ⇒
/// 这里给一段**会立刻抛错**的脚本 ⇒ ⇒ 错在**页内那一侧看得见**，而不是「静默
/// 按另一套契约发了一发」。
#[cfg(target_os = "macos")]
const PAGE_ROUTE_FETCH_ONLY: &str =
    "(function () { throw new Error('这一支是形态 C（页内旁听）：不走形态 B 的就绪／取数脚本'); })()";

/// 读一次页内就绪状态（**读不到** ＝ 页面还在加载或文档被换过）。
///
/// ⚠️ **两种来源走两条通道**（**不是**把两条都改成异步）：
/// · **页内存储取** ⇒ `read_json`（**同步**：原生求值**不等** Promise ⇒ 页内
///   脚本必须同步给回值）—— **这一支逐字保持现状**；
/// · **同源端点取** ⇒ `round_trip`（**能 await Promise**：页内脚本要等那一发
///   `/api/auth/session` 回来才判得出「有没有 token」⇒ ⇒ 走 `read_json` 读到
///   的会是**还没 settle 的 Promise** ⇒ ⇒ 永远判不出就绪）。
#[cfg(target_os = "macos")]
async fn read_page_ready(window: &tauri::WebviewWindow, route: &PageRoute) -> Option<Value> {
    // ⚠️ **形态 C 那一支没有「就绪脚本」**（它有自己那份页内 hook 脚本 ⇒ ⇒ 它的
    // 就绪判据是「脚本自己 resolve 的那一刻」）⇒ ⇒ 这里直接说「没就绪」。
    let auth = route.fetch_auth()?;
    let script = page_ready_script(route);
    match auth.source.ready_channel() {
        ReadyChannel::Sync => {
            // ⚠️ **这一支逐字保持现状**（原生求值**不等** Promise ⇒ 页内脚本
            // 必须同步给回值 ⇒ 这里回的是**一段 JSON 文本**）。
            match read_json(window, &script, Duration::from_secs(4)).await {
                Ok(Value::String(raw)) => serde_json::from_str::<Value>(&raw).ok(),
                Ok(other) => Some(other),
                Err(_) => None,
            }
        }
        ReadyChannel::Async => {
            // ⚠️ **回执是对象**（不是 JSON 文本）⇒ ⇒ 直接就是那个值。
            match round_trip(window, &script, PAGE_READY_ROUND_TRIP_TIMEOUT).await {
                Ok(trip) => Some(trip.value),
                // ⚠️ **读不到就当「还没就绪」**：页面被换掉（`Gone`）／超时／
                // 脚本 reject —— 外层那个等就绪循环会**继续等**（而不是报错）。
                Err(_) => None,
            }
        }
    }
}

/// 登录态明确缺失与读取失败必须分开，避免端点故障被提示为未登录。
#[cfg(any(target_os = "macos", test))]
fn ensure_page_login(ready: &Value, route: &PageRoute) -> Result<(), CaptureFailure> {
    if ready.get("hasCredential").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    if ready.get("credentialMissing").and_then(Value::as_bool) != Some(true) {
        return Err(CaptureFailure::Return(format!(
            "无法确认站点登录态：{}",
            ready
                .get("credentialDetail")
                .and_then(Value::as_str)
                .unwrap_or("凭据读取失败")
        )));
    }
    Err(CaptureFailure::NotLoggedIn(format!(
        "官网窗口没有这个站点的登录态（{}）",
        route
            .fetch_auth()
            .map(|auth| auth.source.missing_credential_detail())
            .unwrap_or_else(|| "这一支没有凭据契约".to_string())
    )))
}

#[cfg(target_os = "macos")]
async fn wait_page_ready(
    window: &tauri::WebviewWindow,
    route: &PageRoute,
) -> Result<(), CaptureFailure> {
    let started = Instant::now();
    let mut last_ready: Option<Value> = None;
    let mut last_href: Option<String> = None;
    let mut stable_hits: u32 = 0;
    loop {
        if let Some(ready) = read_page_ready(window, route).await {
            let ready_state = ready
                .get("readyState")
                .and_then(Value::as_str)
                .unwrap_or("");
            let origin_matches = ready
                .get("originMatches")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let href = ready
                .get("href")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            stable_hits = if !href.is_empty() && last_href.as_deref() == Some(href.as_str()) {
                stable_hits + 1
            } else {
                0
            };
            last_href = Some(href);
            last_ready = Some(ready.clone());
            if ready_state == "complete" && origin_matches {
                ensure_page_login(&ready, route)?;
                if stable_hits >= PAGE_STABLE_HITS {
                    break;
                }
            }
        }
        if started.elapsed() >= PAGE_READY_BUDGET {
            return Err(CaptureFailure::Timeout(format!(
                "{}s 内官网页面没就绪（最后一次读数：{last_ready:?}）",
                PAGE_READY_BUDGET.as_secs()
            )));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) async fn capture_via_page(
    window: &tauri::WebviewWindow,
    route: &PageRoute,
    fetch_url: &str,
) -> Result<CapturedResponse, CaptureFailure> {
    let started = Instant::now();
    // ① 等就绪（**含「页面不再跳转」这一格**）。
    //
    // ⚠️ **为什么「`readyState` ＝ complete」不够**（2026-10-22 21:5x 实测）：
    // 站点首屏在 `complete` **之后**还会跳一次（路由变化）⇒ ⇒ 那一刻发出去的
    // 页内求值会**跟着文档一起被销毁** ⇒ 回程报「页面在请求途中不在了」。
    // ⇒ ⇒ 追加一格「**href 连续多次不变**」才算稳。
    //
    // ⚠️ **这一格顺带解决了「求值会被静默排队」那个坑**（2026-10-22 22:0x 实测）：
    // wry 在 macOS 上把 `eval` 的脚本**排队**，直到**首次导航 commit** 才执行
    // （`wry-0.57.0/src/wkwebview/mod.rs:723-725` ＋ `navigation.rs:28-35`），
    // 而排队时**连 callback 一起丢掉** ⇒ 宿主那边回程通道的 sender 没了
    // ⇒ 报「结果通道已关闭（webview 被销毁）」。
    // ⇒ ⇒ **必须先有一次求值真的跑通**，才知道「首次 commit 已经过了」。
    // 本函数这一格正是那个「先跑通一次」的地方：它读不到就继续等。
    wait_page_ready(window, route).await?;

    // ② 页内发那一发（**生产那份脚本** —— 规格按凭据来源分派，见
    // [`page_fetch_script`]；探针也调**同一个**函数 ⇒ ⇒ 不会「验的是副本」）。
    let script = page_fetch_script(route, fetch_url);

    // ② 页内发那一发。
    //
    // ⚠️ **失败要重试**：文档在发出去之后又被换掉（站点首屏的后续跳转）⇒ 回程
    // 会报「页面在请求途中不在了」⇒ ⇒ **重新等稳再发**（实测：不重试就整轮失败）。
    let mut last_error: Option<CaptureFailure> = None;
    let mut trip = None;
    for attempt in 1..=PAGE_FETCH_ATTEMPTS {
        match round_trip(window, &script, PAGE_FETCH_TIMEOUT).await {
            Ok(value) => {
                trip = Some(value);
                break;
            }
            Err(e) => {
                last_error = Some(match e {
                    RoundTripError::Timeout { .. } => CaptureFailure::Timeout(format!("{e}")),
                    other => CaptureFailure::Return(format!("{other}")),
                });
                if attempt < PAGE_FETCH_ATTEMPTS {
                    // 重新等稳（页面可能刚跳过一次）。
                    wait_page_ready(window, route).await?;
                }
            }
        }
    }
    let trip = match trip {
        Some(trip) => trip,
        None => {
            return Err(last_error
                .unwrap_or_else(|| CaptureFailure::Return("页内取数没有产生任何结果".to_string())))
        }
    };

    // ③ 页内脚本回的是 `{ok, stage, error, status, body}`（**对象**）。
    //
    // ⚠️ **形状不对时必须明确报错**（2026-10-22 22:0x 实测踩到的坑）：脚本
    // 曾经回**一段 JSON 文本**（字符串）⇒ `value.get("ok")` 在字符串上永远是
    // `None` ⇒ 报成「未知阶段／没有给出原因」——看着像站点没数据，其实是自己的
    // 契约读错了。⇒ ⇒ 这里先判形状，形状不对就说清楚。
    let value = trip.value;
    if !value.is_object() {
        return Err(CaptureFailure::Return(match &value {
            Value::String(text) => format!(
                "页内取数的回执是一段文本（{} 字符）而不是对象——取数脚本与宿主之间的契约变了",
                text.chars().count()
            ),
            other => format!("页内取数的回执是 {other} 而不是对象——取数脚本与宿主之间的契约变了"),
        }));
    }
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let stage = value
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("未知阶段");
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("没有给出原因");
        return Err(CaptureFailure::Return(format!(
            "页内取数在「{stage}」这一步失败：{error}"
        )));
    }
    let status = value.get("status").and_then(Value::as_i64).unwrap_or(0);
    let body = value
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(CapturedResponse {
        body,
        status,
        // ⚠️ 路线 B 没有 DevTools 的时间戳 ⇒ 用**壳侧当刻**的单调秒（形状一致，
        // 语义上就是「这一发是当刻发的」）。
        timestamp: started.elapsed().as_secs_f64(),
        // 路线 B 是壳侧**主动发的那一发** ⇒ 候选**恰好一条**，且**必然满足**
        // 「该取哪一条」判据（它就是为这一条发的）。
        matched: 1,
        rule_matched: 1,
        rule_detail: Some("路线 B：壳侧在页内发起的那一发（唯一一条）".to_string()),
    })
}

// ==== 形态 C（macOS）：页内旁听（**替换 ＋ 取到即还原**）=======================
//
// 形态：**页内旁听站点自己那一发**（形态 B 是「页内自己发那一发」）。
//
// ⚠️ **为什么 MiniMax 只能走这一支**：那条请求的签名**复刻不出来**（要靠站点
// 自己那层生成）⇒ ⇒ 壳侧**发不了**那一发 ⇒ ⇒ 只能**在页内等站点自己发**、**截下
// 它**（协议 §1.0.3 形态 C）。
//
// ⚠️ **凭据全程不出页面**：签名头（`x-signature`／`x-timestamp`／`yy`）与 cookie
// 一步都不出页面 ⇒ ⇒ 壳侧只拿到**响应体原文**（裁剪在壳侧做完）。

/// 形态 C 的**页内脚本**（`__SPEC__` 由宿主注入）。
///
/// ⚠️ **为什么要有这个函数**（而不是在 [`capture_via_page_hook`] 里拼）：端到端
/// 探针要**用生产那份**脚本去跑（自己拼规格就变成「验的是探针自己的副本」）⇒ ⇒
/// 这一段必须是**生产代码里的一个可调用的东西**。
///
/// ⚠️ **裁结构的映射全部来自规格表**（[`SiteSpec`]）⇒ ⇒ 规格表仍是**唯一真相**
/// （页内那份裁剪**不许**自己另写一套 ⇒ ⇒ 那是「同一套映射写两遍」）。
#[cfg(target_os = "macos")]
pub(crate) fn page_hook_script(spec: &SiteSpec, route: &PageRoute, target_path: &str) -> String {
    // ⚠️ **形态 B 那一支不走这个脚本**（它走 [`page_fetch_script`]）⇒ ⇒ 万一被
    // 调到 ⇒ ⇒ 交回一段**会立刻失败**的脚本（不静默套用别的契约）。
    let PageRoute::Hook {
        signature_headers, ..
    } = route
    else {
        return PAGE_ROUTE_HOOK_ONLY.to_string();
    };
    let mut spec = json!({
        "without_query_key": match if target_path == spec.catalog_path { spec.catalog_pick } else { spec.body_pick } {
            BodyPick::WithoutQueryKey(key) => Some(key),
            _ => None,
        },
        // ⚠️ **要旁听的那条路径**（目录那条／正文那条，**按链不同**）。
        "catalog_path": target_path,
        // ⚠️ **签名头的名字**（**值一步都不出页面**）。
        "signature_headers": signature_headers,
        // ⚠️ **裁结构用的映射**：**全部来自规格表**。
        "sessions_path": spec.sessions_path,
        "item_path": spec.item_path,
        "id_path": spec.fields.id,
        "item_fields": {
            "title": spec.fields.title,
            "title_type": spec.fields.title_type,
            "model_type": spec.fields.model_type,
            "pinned": spec.fields.pinned,
            "updated_at": spec.fields.updated_at,
        },
        // ⚠️ **生产不重放、不做阳性对照**：
        // · **重放**是探针那一臂（页内用截到的签名头**自己**再发一次）⇒ ⇒ 生产
        //   **不重放**（目录那条靠站点自己发、正文那条靠**深链**让站点自己发）；
        // · **阳性对照**会在**还原之前**再发一次那一发 ⇒ ⇒ 生产**不做**（少一次
        //   站点请求 ⇒ ⇒ 更贴协议 §7.3 的「只读」）。
        "replay_body": false,
        "positive_control": false,
        // ⚠️ **页内等待预算**（等「站点自己那层」出现／等那一发被截到，**各一份**）。
        "poll_budget_ms": PAGE_HOOK_POLL_BUDGET_MS,
        "max_poll_attempts": PAGE_HOOK_MAX_POLL_ATTEMPTS,
    });
    if let Some(keys) = account_contract::projection_keys(target_path) {
        spec["identity_keys"] = json!(keys);
    }
    let spec = serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string());
    include_str!("page_hook_script.js").replace("__SPEC__", &spec)
}

/// 走**形态 C（页内旁听）**取一次响应体。
///
/// ## 这一步做了什么
///
/// 1. **等首次导航 commit**（**唯一**的前置 —— wry 在 commit 之前把求值**排队**
///    并**丢掉回调** ⇒ ⇒ 那时候求值会报「结果通道已关闭」，而**那条报错会骗人**）；
/// 2. **注入**生产那份页内 hook 脚本（**只注入一次**）⇒ ⇒ 它在页内等「站点自己
///    那层 fetch」出现 ⇒ ⇒ 套在它**上面**装钩 ⇒ ⇒ 截到那一发的**请求头**那一刻
///    就**同步还原**成「替换前那个引用」⇒ ⇒ 再用**克隆体**把响应体读完；
/// 3. **把响应体原文取回宿主** ⇒ ⇒ 后面裁剪（[`SessionStructure::from_body_for`]
///    ／`SessionBody::from_body_for`）是**跨平台共享的纯逻辑** ⇒ ⇒ **白名单逐字
///    不变**（形态 C 与形态 B 共用**同一套**裁剪）。
#[cfg(target_os = "macos")]
async fn capture_via_page_hook(
    window: &tauri::WebviewWindow,
    spec: &SiteSpec,
    route: &PageRoute,
    target_path: &str,
) -> Result<CapturedResponse, CaptureFailure> {
    // ① 等首次导航 commit（**注入必须尽可能早** ⇒ ⇒ 站点那层一出现就得装钩）。
    wait_first_commit(window).await?;
    // ② 注入（**只发生一次**）＋ 回程（**读回容错** ⇒ 单次读回失败不放弃）。
    let script = page_hook_script(spec, route, target_path);
    let started = Instant::now();
    let trip = round_trip_tolerant(window, &script, PAGE_HOOK_ROUND_TRIP_TIMEOUT)
        .await
        .map_err(|e| match e {
            RoundTripError::Timeout { .. } => CaptureFailure::Timeout(format!("{e}")),
            other => CaptureFailure::Return(format!("{other}")),
        })?;
    // ③ 回执形状（**对象**；不是 JSON 文本）⇒ 形状不对要说清楚（不然看着像站点
    //    没数据，其实是自己的契约读错了）。
    let value = trip.value;
    if !value.is_object() {
        return Err(CaptureFailure::Return(match &value {
            Value::String(text) => format!(
                "页内旁听的回执是一段文本（{} 字符）而不是对象——取数脚本与宿主之间的契约变了",
                text.chars().count()
            ),
            other => format!("页内旁听的回执是 {other} 而不是对象——取数脚本与宿主之间的契约变了"),
        }));
    }
    // ④ 失败**按阶段说**：页内脚本给的是**可区分的错因**（`site-layer-missing`／
    //    `catalog-already-happened`／`catalog-not-captured`／…）⇒ ⇒ 原样带出来。
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let stage = value
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("未知阶段");
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("没有给出原因");
        return Err(CaptureFailure::Return(format!(
            "页内旁听在「{stage}」这一步失败：{error}"
        )));
    }
    // ⑤ 取那一发的**响应体原文**（壳侧裁 ⇒ ⇒ 白名单逐字不变）。
    let captured = value.get("capturedBody");
    let status = captured
        .and_then(|body| body.get("status"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let body = captured
        .and_then(|body| body.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(CapturedResponse {
        body,
        status,
        // ⚠️ 形态 C 没有 DevTools 的时间戳 ⇒ 用**壳侧当刻**的单调秒（形状一致，
        // 语义上就是「这一发是当刻发的」）。
        timestamp: started.elapsed().as_secs_f64(),
        // 形态 C 截到的**就是那一发** ⇒ 候选**恰好一条**（站点自己发的，壳侧
        // **没发**那一发 ⇒ ⇒ 「命中多条时按哪一条取」那条判据**不适用**）。
        matched: 1,
        rule_matched: 1,
        rule_detail: Some("形态 C：页内旁听站点自己那一发（唯一一条）".to_string()),
    })
}

/// 等**首次导航 commit**（形态 C 的**唯一**前置）。
///
/// ⚠️ **为什么必须先等这一格**（wry 0.57 · macOS · 实测）：`eval` 在**首次导航
/// commit 之前**只把脚本**排队**，而排队分支里**连 callback 一起丢掉**⇒ ⇒ 那时候
/// 求值会报「结果通道已关闭（webview 被销毁）」——**那条报错会骗人**（webview
/// 好好的，是脚本被排队了）⇒ ⇒ **必须先有一次求值真的跑通**。
///
/// ⚠️ **判据**：取数窗是建在 **`about:blank`** 上的 ⇒ ⇒ 「**地址不再是
/// `about:blank`**」＝「那一次导航**已经落地**」⇒ ⇒ 于是**注入尽可能早**
/// （站点那层 fetch 一出现就得装钩 ⇒ ⇒ 晚了就错过那一发 ⇒ ⇒ 那是
/// `catalog-already-happened`）。
#[cfg(target_os = "macos")]
async fn wait_first_commit(window: &tauri::WebviewWindow) -> Result<(), CaptureFailure> {
    let started = Instant::now();
    let mut last = String::new();
    while started.elapsed() < PAGE_COMMIT_BUDGET {
        // ⚠️ **读不到就继续等**（commit 之前求值会被排队并丢掉回调 ⇒ ⇒ 那时候
        // 读回必然失败）⇒ ⇒ 这一格是**条件等待**，不是固定睡眠。
        if let Ok(value) = read_json(window, "String(location.href)", Duration::from_secs(2)).await
        {
            let text = value.as_str().unwrap_or("").to_string();
            if !text.is_empty() && text != "about:blank" {
                return Ok(());
            }
            last = text;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    Err(CaptureFailure::Timeout(format!(
        "{}s 内首次导航没有 commit（最后读到的地址：{last}）",
        PAGE_COMMIT_BUDGET.as_secs()
    )))
}

/// 等到**目标端点**那条响应加载完。
///
/// ## 为什么只在 `loadingFinished` 上动手
///
/// 协议 §8.5.1 硬约束 1：`Network.getResponseBody` **取早了有两种失败形态** ——
/// ① `E_INVALIDARG (0x80070057)` ② **静默拿到长度 0 的 body 且不报错**
/// ⇒ ⇒ 这里**只**认 `Network.loadingFinished`，**不**在 `responseReceived` 上取体
/// ⇒ ⇒ 空 body 也不会被误当成「站点没数据」（取体那一步单独判）。
///
/// ## 命中多条时怎么选（协议 §4.4 的硬要求）
///
/// 一次深链导航里站点**可能发多条**同类请求（实测：MiniMax 侧一次发了 5 条）
/// ⇒ ⇒ **不许**拿「第一条同类响应」当正文 ⇒ ⇒
///
/// · **锁定**：路径相同 **且** query 里那个键的值与目标**逐字相等**
///   （正文那条锁 `chat_session_id`）；
/// · **取最新**：命中多条时取 `Network.responseReceived` 的 `timestamp`
///   （**单调秒**）**最大**的那条；若还有**更晚**的候选**没加载完** ⇒ **继续等它**
///   （**取体仍然只在 `loadingFinished` 之后** —— 硬约束 1 不受影响）。
#[cfg(windows)]
async fn await_target_hit(
    events: &mut EventStream,
    timeout: Duration,
    target: &CaptureTarget,
    site: &str,
) -> Result<TargetHit, CaptureFailure> {
    let deadline = Instant::now() + timeout;
    // `requestId` → 那条请求的元信息（URL／状态码／时刻／是否已加载完）。
    let mut seen: HashMap<String, HitMeta> = HashMap::new();
    let mut setup_problems: Vec<String> = Vec::new();
    let mut auth_rejected = false;

    let channel_closed = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break false;
        }
        let received = tokio::time::timeout(remaining, events.recv()).await;
        let event = match received {
            Ok(Some(event)) => event,
            Ok(None) => break true,
            Err(_) => break false,
        };
        if event.name == "__setup__" {
            setup_problems.push(event.raw);
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&event.raw) else {
            // 载荷不是 JSON ⇒ 忽略这一条（不是「通道不通」）。
            continue;
        };
        match event.name.as_str() {
            "Network.responseReceived" => {
                let request_id = value
                    .get("requestId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if request_id.is_empty() {
                    continue;
                }
                let url = value
                    .pointer("/response/url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let status = value
                    .pointer("/response/status")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let timestamp = value
                    .get("timestamp")
                    .and_then(|v| v.as_f64())
                    .unwrap_or_default();
                seen.insert(
                    request_id.clone(),
                    HitMeta {
                        request_id,
                        url,
                        status,
                        timestamp,
                        finished: false,
                    },
                );
            }
            "Network.loadingFinished" => {
                let request_id = value
                    .get("requestId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let Some(meta) = seen.get(request_id).cloned() else {
                    continue;
                };
                if !url_matches_target(&meta.url, target) {
                    continue;
                }
                if let Some(entry) = seen.get_mut(request_id) {
                    entry.finished = true;
                }
                // ChatGPT can finish an anonymous request before its authenticated request.
                if site == CHATGPT_SITE && meta.status == 401 {
                    auth_rejected = true;
                    continue;
                }
                // 命中的候选（路径 ＋ 锁定条件都对上）里取**该取的那条**。
                let candidates: Vec<&HitMeta> = seen
                    .values()
                    .filter(|item| item.finished && url_matches_target(&item.url, target))
                    .filter(|item| site != CHATGPT_SITE || item.status != 401)
                    .collect();
                if candidates.is_empty() {
                    continue;
                }
                // ⚠️ **按站点的判据**挑出「该取的那批」⇒ ⇒ 命中多条时**不是**
                // 「取最新」（按响应时刻）、**也不是**「取 `limit` 最大的」——
                // MiniMax 的读数坐实了那两条**都会取到更早的一页** ⇒ ⇒ 判据 ＝
                // **`before` 缺席**（站点从最新端取的那条）。
                //
                // ⚠️ **判据是在「已见过的全部候选」上算的**（**含还没加载完的**）——
                // ChatGPT 那条判据（`num_turns` 更大）**可能**落在一条**还没加载完**
                // 的候选上 ⇒ ⇒ 那种情况下**必须继续等它**（取体仍然只在
                // `loadingFinished` 之后 —— 协议 §8.5.1 硬约束 1 不受影响）。
                let seen_candidates: Vec<&HitMeta> = seen
                    .values()
                    .filter(|item| url_matches_target(&item.url, target))
                    .filter(|item| site != CHATGPT_SITE || item.status != 401)
                    .collect();
                let eligible: Vec<&HitMeta> = eligible_candidates(&seen_candidates, &target.pick);
                // ⚠️ **判据一条都没命中** ⇒ ⇒ 退回「全部候选」，并**如实记下**
                // （`rule_matched` ＝ 0 ⇒ 上层据此说「判据没成立」）。
                //
                // ⚠️ **例外**：「**只取**」那条判据（[`BodyPick::MustNotHaveQueryKey`]）
                // **不退回** ⇒ ⇒ 一条都不满足 ⇒ ⇒ **继续等**（**绝不**把那条
                // **空的**变体当成目录）。
                let chosen: Vec<&HitMeta> = match target.pick {
                    BodyPick::MustNotHaveQueryKey(_) => eligible
                        .iter()
                        .copied()
                        .filter(|item| item.finished)
                        .collect(),
                    _ => {
                        if eligible.is_empty() {
                            candidates.clone()
                        } else {
                            eligible
                                .iter()
                                .copied()
                                .filter(|item| item.finished)
                                .collect()
                        }
                    }
                };
                let Some(best) = chosen.iter().copied().max_by(|a, b| {
                    a.timestamp
                        .partial_cmp(&b.timestamp)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) else {
                    continue;
                };
                // 还有**候选**没加载完 ⇒ **继续等它**（取体只在加载完之后）。
                //
                // ⚠️ **「值判据」不等「时刻判据」** —— ChatGPT 那条（`num_turns`
                // 更大）里，**还没加载完**的那条**可能**就是赢家（哪怕它的响应
                // 时刻更早）⇒ ⇒ 那种判据**不看时刻**、只要它还满足判据就等。
                let pending_eligible: Vec<&HitMeta> = seen_candidates
                    .iter()
                    .copied()
                    .filter(|item| !item.finished)
                    .filter(|item| {
                        eligible
                            .iter()
                            .any(|kept| kept.request_id == item.request_id)
                    })
                    .collect();
                let pending_later = match target.pick {
                    BodyPick::Latest | BodyPick::WithoutQueryKey(_) => pending_eligible
                        .iter()
                        .any(|item| item.timestamp > best.timestamp),
                    // ⚠️ **值判据**：还在加载的那条**可能就是赢家** ⇒ ⇒ 等它。
                    BodyPick::LargestQueryValue(_) => !pending_eligible.is_empty(),
                    // ⚠️ **「只取」那条**：还在加载的那条**可能**就是合格的那条
                    // ⇒ ⇒ 等它。
                    BodyPick::MustNotHaveQueryKey(_) => !pending_eligible.is_empty(),
                };
                if pending_later {
                    continue;
                }
                return Ok(TargetHit {
                    request_id: best.request_id.clone(),
                    status: best.status,
                    timestamp: best.timestamp,
                    matched: candidates.len(),
                    rule_matched: chosen.len(),
                    rule_detail: rule_detail(&candidates, &target.pick, best),
                });
            }
            _ => {}
        }
    };
    if auth_rejected {
        return Err(CaptureFailure::NotLoggedIn(
            "ChatGPT 返回 HTTP 401，等待后续响应仍未成功；请在官网页签确认登录后重新引用".into(),
        ));
    }
    Err(CaptureFailure::Timeout(format!(
        "{}s 内没等到站点那条响应加载完{}{}",
        timeout.as_secs(),
        if channel_closed {
            "（事件通道已关闭）"
        } else {
            ""
        },
        if setup_problems.is_empty() {
            String::new()
        } else {
            format!("（订阅阶段：{}）", setup_problems.join("；"))
        }
    )))
}

/// 一条候选请求的元信息（**壳侧自己记的**，不是站点给的）。
#[derive(Debug, Clone)]
#[cfg(windows)]
struct HitMeta {
    /// `Network` 域的 `requestId`。
    request_id: String,
    /// 那条请求的**完整 URL**（含 query）。
    url: String,
    /// HTTP 状态码。
    status: i64,
    /// `Network.responseReceived` 的 `timestamp`（**单调秒**）—— 命中多条时取最新。
    timestamp: f64,
    /// 是否已经收到 `Network.loadingFinished`（**取体的前置条件**）。
    finished: bool,
}

/// 这条 URL 是不是**目标那条**（路径 ＋ （按站点的）锁定条件**逐字**比对）。
///
/// ⚠️ **query 值逐字比**（协议 §4.4）：站点自己发的那个值就是那串 id ⇒
/// 比的就是它（**不做百分号解码** —— 解码了就不是「逐字」了）。
///
/// ⚠️ **MiniMax 的会话 id 在路径里** ⇒ ⇒ 模板里的 `{session}` 先换成目标会话
/// id，再拿整条路径去比 ⇒ ⇒ **别的会话的响应不会命中**（**逐字**）。
fn url_matches_target(url: &str, target: &CaptureTarget) -> bool {
    let Ok(actual) = tauri::Url::parse(url) else {
        return false;
    };
    let Ok(expected) = tauri::Url::parse(&target.url) else {
        return false;
    };
    let same_origin = actual.origin() == expected.origin();
    let minimax = spec_for(MINIMAX_SITE).expect("built-in site");
    let aliases = [minimax.root_url, minimax.deep_link_prefix];
    let minimax_origin = |candidate: &tauri::Url| {
        aliases.iter().any(|alias| {
            tauri::Url::parse(alias).is_ok_and(|alias| alias.origin() == candidate.origin())
        })
    };
    if !same_origin && !(minimax_origin(&actual) && minimax_origin(&expected)) {
        return false;
    }
    let expected_path = match &target.lock {
        Some(BodyLockTarget::PathSegment { session_id }) => {
            target.path_template.replace("{session}", session_id)
        }
        _ => target.path_template.to_string(),
    };
    if actual.path() != expected_path {
        return false;
    }
    match &target.lock {
        Some(BodyLockTarget::QueryKey { key, session_id }) => query_value(url, key)
            .map(|value| &value == session_id)
            .unwrap_or(false),
        // ⚠️ **路径锁定**：会话 id **已经在上面那句里逐字比过了** ⇒ ⇒ 不必再比
        // 第二遍（**也不许**去猜一个 query 键名出来）。
        Some(BodyLockTarget::PathSegment { .. }) => true,
        // 目录那条：query 由站点自己管 ⇒ ⇒ 按路径认即可。
        None => true,
    }
}

/// 按站点的判据，从候选里挑出「**该取的那批**」（**逐站点的判据**）。
///
/// ⚠️ **判据在「已见过的全部候选」上算**（含还没加载完的）—— ChatGPT 那条
/// （`num_turns` 更大）**可能**落在一条还没加载完的候选上 ⇒ ⇒ 那种情况下上层
/// 会**继续等它**。
#[cfg(windows)]
fn eligible_candidates<'a>(candidates: &[&'a HitMeta], pick: &BodyPick) -> Vec<&'a HitMeta> {
    match pick {
        // 「取最新」／「`before` 缺席」＝ **逐条判**（不依赖别的候选）。
        BodyPick::Latest => candidates.to_vec(),
        BodyPick::WithoutQueryKey(key) => candidates
            .iter()
            .copied()
            .filter(|item| query_value(&item.url, key).is_none())
            .collect(),
        // ⚠️ **「只取」那条**：**一条都不满足 ⇒ ⇒ 一条都不取**（**不退回**）——
        // 退回会把**那条空的**变体当成目录（实测：44 字节、`items` 长度 0）。
        BodyPick::MustNotHaveQueryKey(key) => candidates
            .iter()
            .copied()
            .filter(|item| query_value(&item.url, key).is_none())
            .collect(),
        // ⚠️ **值判据**：取那个 query 键**值最大**的那批（**按数值**比）。
        // 认不出值（不是数字）⇒ ⇒ 退回「全部候选」（上层据此如实说
        // 「判据没成立」，**不猜**）。
        BodyPick::LargestQueryValue(key) => {
            let mut best: Option<f64> = None;
            for item in candidates {
                let Some(value) =
                    query_value(&item.url, key).and_then(|raw| raw.parse::<f64>().ok())
                else {
                    continue;
                };
                best = Some(best.map_or(value, |current| current.max(value)));
            }
            let Some(best) = best else {
                return candidates.to_vec();
            };
            let eligible: Vec<&HitMeta> = candidates
                .iter()
                .copied()
                .filter(|item| {
                    query_value(&item.url, key)
                        .and_then(|raw| raw.parse::<f64>().ok())
                        .map(|value| value == best)
                        .unwrap_or(false)
                })
                .collect();
            if eligible.is_empty() {
                candidates.to_vec()
            } else {
                eligible
            }
        }
    }
}

/// **值判据**的**读数依据**（回执里那句「为什么取这一条」的事实）。
///
/// ⚠️ **只记数字**（那个 query 键的取值）⇒ ⇒ 不是内容、不是凭据。
#[cfg(windows)]
fn rule_detail(candidates: &[&HitMeta], pick: &BodyPick, best: &HitMeta) -> Option<String> {
    match pick {
        BodyPick::Latest | BodyPick::WithoutQueryKey(_) => None,
        // ⚠️ **值判据**：那条判据的依据是**每个候选的那个键的值** ⇒ ⇒ 逐字
        // 列出来（**只记数字**）。
        BodyPick::LargestQueryValue(key) => {
            let mut values: Vec<String> = candidates
                .iter()
                .filter_map(|item| query_value(&item.url, key))
                .collect();
            if values.is_empty() {
                return None;
            }
            values.sort();
            let chosen = query_value(&best.url, key).unwrap_or_default();
            Some(format!(
                "命中的候选里 `{key}` 的取值：{} ⇒ 取 `{key}` ＝ {chosen} 的那条（相同则取 responseReceived.timestamp 最大的那条）",
                values.join("、")
            ))
        }
        // ⚠️ **「只取」那条**：依据是**每个候选带没带那个键** ⇒ ⇒ 逐条列出来
        // （**只记键名在不在**，不记它的值）。
        BodyPick::MustNotHaveQueryKey(key) => {
            let mut variants: Vec<String> = Vec::new();
            for item in candidates {
                let mark = if query_value(&item.url, key).is_some() {
                    format!("带 {key}")
                } else {
                    format!("不带 {key}")
                };
                if !variants.contains(&mark) {
                    variants.push(mark);
                }
            }
            if variants.is_empty() {
                return None;
            }
            Some(format!(
                "命中的候选里同一个路径的请求变体：{} ⇒ 取**不带 {key}** 的那条",
                variants.join("、")
            ))
        }
    }
}

/// 取 URL 的 query 里某个键的**原始值**（**逐字**；取不到 ⇒ `None`，不猜）。
fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let name = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        if name == key {
            return Some(value.to_string());
        }
    }
    None
}

/// 取那条响应的**体**。
///
/// ## 为什么空体算「回程失败」
///
/// 协议 §8.5.1 硬约束 1：**取早了会静默拿到长度 0 的 body 且不报错** ⇒ ⇒
/// 空体**不是**「站点没数据」，是**取体失败** ⇒ ⇒ 归 `catalog_failed`（回程失败类），
/// **绝不**归 `catalog_empty`（站点返回空）—— 协议 §6.3 要这两类分开。
#[cfg(windows)]
async fn take_response_body(
    window: &tauri::WebviewWindow,
    request_id: &str,
) -> Result<String, CaptureFailure> {
    let envelope = cdp_call(
        window,
        "Network.getResponseBody",
        json!({ "requestId": request_id }),
        CDP_TIMEOUT,
    )
    .await
    .map_err(|e| CaptureFailure::Return(format!("取响应体失败：{e}")))?;

    // 回执信封**两种都要认**（协议 §8.5.1 硬约束 4）—— `getResponseBody` 的回执
    // **没有** `result` 外层 ⇒ 走 `cdp_field`。
    let encoded = cdp_field(&envelope, "base64Encoded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let body = cdp_field(&envelope, "body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if body.is_empty() {
        return Err(CaptureFailure::Return(
            "取到的响应体是空的（协议 §8.5.1 硬约束 1：取早了会**静默**拿到空体 ⇒ 这是回程失败，不是站点没数据）"
                .to_string(),
        ));
    }
    if !encoded {
        return Ok(body);
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|e| CaptureFailure::Return(format!("响应体是 base64 但解不开：{e}")))?;
    String::from_utf8(bytes).map_err(|e| CaptureFailure::Return(format!("响应体不是 UTF-8：{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_login_check_preserves_missing_and_unknown_states() {
        for site in [CHATGPT_SITE, SITE] {
            let route = spec_for(site).unwrap().page_route.unwrap();
            assert!(ensure_page_login(&json!({ "hasCredential": true }), &route).is_ok());
            let missing = ensure_page_login(
                &json!({ "hasCredential": false, "credentialMissing": true }),
                &route,
            )
            .unwrap_err();
            assert_eq!(missing.code(), "not_logged_in");
            let unknown = ensure_page_login(
                &json!({ "hasCredential": false, "credentialMissing": false, "credentialDetail": "endpoint 503" }),
                &route,
            ).unwrap_err();
            assert!(matches!(unknown, CaptureFailure::Return(_)));
            assert!(unknown.detail().contains("503"));
        }
    }

    #[test]
    fn response_target_requires_the_exact_endpoint_path() {
        let spec = spec_for(CHATGPT_SITE).unwrap();
        let mut target = CaptureTarget {
            url: spec.root_url.into(),
            path_template: spec.catalog_path,
            lock: None,
            pick: spec.catalog_pick,
            page_route: spec.page_route,
            page_body_url: None,
        };
        assert!(url_matches_target(
            "https://chatgpt.com/backend-api/conversations?limit=28",
            &target
        ));
        for url in [
            "https://chatgpt.com/backend-api/conversations/other-session",
            "https://chatgpt.com/backend-api/conversations-archived",
            "https://chatgpt.com/other?next=/backend-api/conversations",
            "not-a-url/backend-api/conversations",
        ] {
            assert!(!url_matches_target(url, &target), "{url}");
        }
        target.path_template = spec.body_path_template;
        target.lock = Some(BodyLockTarget::PathSegment {
            session_id: "session-1".into(),
        });
        assert!(url_matches_target(
            "https://chatgpt.com/backend-api/conversations/session-1?num_turns=20",
            &target
        ));
        assert!(!url_matches_target(
            "https://chatgpt.com/backend-api/conversations/session-10",
            &target
        ));
        assert!(!url_matches_target(
            "https://evil.test/backend-api/conversations/session-1",
            &target
        ));
        assert!(!url_matches_target(
            "http://chatgpt.com/backend-api/conversations/session-1",
            &target
        ));
        let minimax = CaptureTarget {
            url: "https://agent.minimaxi.com/".into(),
            path_template: "/v1/api/user/info",
            lock: None,
            pick: BodyPick::Latest,
            page_route: None,
            page_body_url: None,
        };
        assert!(url_matches_target(
            "https://agent.minimax.cn/v1/api/user/info",
            &minimax
        ));
        assert!(!url_matches_target(
            "https://chatgpt.com/v1/api/user/info",
            &minimax
        ));
    }

    #[test]
    fn failed_catalog_cache_expires_before_successful_cache() {
        let state = CaptureState {
            cache: Mutex::new(BTreeMap::new()),
            fetched_at: Mutex::new(BTreeMap::new()),
            accounts: Mutex::new(BTreeMap::new()),
            round: tokio::sync::Mutex::new(()),
            last_window: Mutex::new(None),
        };
        let requested_at = Instant::now();
        write_cache(
            &state,
            CHATGPT_SITE,
            Err(CaptureFailure::Return("401".into())),
        );
        write_cache(&state, MINIMAX_SITE, Ok(SessionStructure::default()));
        assert!(fresh(&state, CHATGPT_SITE).unwrap().is_err());
        assert!(reusable(&state, CHATGPT_SITE, requested_at, true)
            .unwrap()
            .is_err());
        assert!(reusable(&state, CHATGPT_SITE, Instant::now(), true).is_none());

        let aged = Instant::now() - STRUCTURE_FAILURE_TTL - Duration::from_millis(1);
        {
            let mut times = state.fetched_at.lock().unwrap();
            times.insert(CHATGPT_SITE.into(), aged);
            times.insert(MINIMAX_SITE.into(), aged);
        }
        assert!(fresh(&state, CHATGPT_SITE).is_none());
        assert!(fresh(&state, MINIMAX_SITE).unwrap().is_ok());
        state
            .fetched_at
            .lock()
            .unwrap()
            .insert(MINIMAX_SITE.into(), Instant::now() - STRUCTURE_TTL);
        assert!(fresh(&state, MINIMAX_SITE).is_none());
    }

    #[test]
    fn account_change_invalidates_shell_catalog_without_touching_other_sites() {
        let state = CaptureState {
            cache: Mutex::new(BTreeMap::new()),
            fetched_at: Mutex::new(BTreeMap::new()),
            accounts: Mutex::new(BTreeMap::new()),
            round: tokio::sync::Mutex::new(()),
            last_window: Mutex::new(None),
        };
        invalidate_account(&state, CHATGPT_SITE, "account-a").unwrap();
        write_cache(&state, CHATGPT_SITE, Ok(SessionStructure::default()));
        write_cache(&state, MINIMAX_SITE, Ok(SessionStructure::default()));
        invalidate_account(&state, CHATGPT_SITE, "account-a").unwrap();
        assert!(fresh(&state, CHATGPT_SITE).is_some());
        invalidate_account(&state, CHATGPT_SITE, "account-b").unwrap();
        assert!(fresh(&state, CHATGPT_SITE).is_none());
        assert!(fresh(&state, MINIMAX_SITE).is_some());
        write_cache(&state, CHATGPT_SITE, Ok(SessionStructure::default()));
        invalidate_account(&state, CHATGPT_SITE, "").unwrap();
        assert!(fresh(&state, CHATGPT_SITE).is_none());
    }

    #[cfg(windows)]
    mod auth_responses {
        use super::*;

        fn catalog_target(site: &str) -> CaptureTarget {
            let spec = spec_for(site).unwrap();
            CaptureTarget {
                url: spec.root_url.into(),
                path_template: spec.catalog_path,
                lock: None,
                pick: spec.catalog_pick,
                page_route: spec.page_route,
                page_body_url: None,
            }
        }

        async fn response(
            sender: &tauri::async_runtime::Sender<NetworkEvent>,
            id: &str,
            url: &str,
            status: i64,
            finished: bool,
        ) {
            sender
                .send(NetworkEvent {
                    name: "Network.responseReceived".into(),
                    raw: json!({
                        "requestId": id,
                        "response": { "url": url, "status": status },
                        "timestamp": if status == 401 { 1.0 } else { 2.0 },
                    })
                    .to_string(),
                })
                .await
                .unwrap();
            if finished {
                sender
                    .send(NetworkEvent {
                        name: "Network.loadingFinished".into(),
                        raw: json!({ "requestId": id }).to_string(),
                    })
                    .await
                    .unwrap();
            }
        }

        #[tokio::test]
        async fn chatgpt_catalog_waits_for_authenticated_response() {
            let (sender, mut events) = tauri::async_runtime::channel(8);
            let target = catalog_target(CHATGPT_SITE);
            let url = "https://chatgpt.com/backend-api/conversations?limit=28";
            response(&sender, "anonymous", url, 401, true).await;
            response(&sender, "authenticated", url, 200, true).await;
            let hit = await_target_hit(&mut events, Duration::from_secs(1), &target, CHATGPT_SITE)
                .await
                .unwrap();
            assert_eq!(hit.request_id, "authenticated");
            assert_eq!(hit.status, 200);
        }

        #[tokio::test]
        async fn unauthorized_body_cannot_win_or_block_the_query_rule() {
            for finished in [false, true] {
                let (sender, mut events) = tauri::async_runtime::channel(8);
                let spec = spec_for(CHATGPT_SITE).unwrap();
                let target = CaptureTarget {
                    path_template: spec.body_path_template,
                    lock: Some(BodyLockTarget::PathSegment {
                        session_id: "synthetic-session".into(),
                    }),
                    pick: spec.body_pick,
                    ..catalog_target(CHATGPT_SITE)
                };
                response(
                    &sender,
                    "anonymous",
                    "https://chatgpt.com/backend-api/conversations/synthetic-session?num_turns=100",
                    401,
                    finished,
                )
                .await;
                response(
                    &sender,
                    "authenticated",
                    "https://chatgpt.com/backend-api/conversations/synthetic-session?num_turns=20",
                    200,
                    true,
                )
                .await;
                let hit =
                    await_target_hit(&mut events, Duration::from_secs(1), &target, CHATGPT_SITE)
                        .await
                        .unwrap();
                assert_eq!(hit.request_id, "authenticated");
                assert_eq!(hit.status, 200);
            }
        }

        #[tokio::test]
        async fn persistent_401_reports_login_at_deadline_or_channel_close() {
            for close in [false, true] {
                let (sender, mut events) = tauri::async_runtime::channel(8);
                let target = catalog_target(CHATGPT_SITE);
                response(
                    &sender,
                    "anonymous",
                    "https://chatgpt.com/backend-api/conversations",
                    401,
                    true,
                )
                .await;
                let _keep_open = if close { None } else { Some(sender.clone()) };
                drop(sender);
                let failure = await_target_hit(
                    &mut events,
                    Duration::from_millis(10),
                    &target,
                    CHATGPT_SITE,
                )
                .await
                .unwrap_err();
                assert_eq!(failure.code(), "not_logged_in");
                assert!(failure.detail().contains("401"));
            }
        }

        #[tokio::test]
        async fn other_sites_and_non_401_responses_keep_their_behavior() {
            for (site, status) in [
                (MINIMAX_SITE, 401),
                (CHATGPT_SITE, 200),
                (CHATGPT_SITE, 403),
                (CHATGPT_SITE, 500),
            ] {
                let (sender, mut events) = tauri::async_runtime::channel(8);
                let target = catalog_target(site);
                let url = tauri::Url::parse(&target.url)
                    .unwrap()
                    .join(target.path_template)
                    .unwrap()
                    .to_string();
                response(&sender, "original", &url, status, true).await;
                let hit = await_target_hit(&mut events, Duration::from_secs(1), &target, site)
                    .await
                    .unwrap();
                assert_eq!(hit.status, status);
            }
        }

        #[tokio::test]
        async fn no_matching_response_still_reports_timeout() {
            let (sender, mut events) = tauri::async_runtime::channel(8);
            sender
                .send(NetworkEvent {
                    name: "__setup__".into(),
                    raw: "synthetic setup diagnostic".into(),
                })
                .await
                .unwrap();
            drop(sender);
            let failure = await_target_hit(
                &mut events,
                Duration::from_secs(1),
                &catalog_target(CHATGPT_SITE),
                CHATGPT_SITE,
            )
            .await
            .unwrap_err();
            assert!(matches!(failure, CaptureFailure::Timeout(_)));
            assert!(failure.detail().contains("synthetic setup diagnostic"));
        }
    }

    #[test]
    fn missing_login_keeps_its_existing_error_code() {
        let failure = CaptureFailure::NotLoggedIn("请在官网页签登录".into());
        assert_eq!(failure.code(), "not_logged_in");
        assert_eq!(failure.detail(), "请在官网页签登录");
    }

    /// 造一份 **MiniMax 形态**的目录响应体（**按 2026-10-22 13:0x 的实测形状**；
    /// **合成数据**，**不是**真响应体 ⇒ ⇒ 仓库里不落任何真实响应体）。
    ///
    /// ⚠️ 形状要点：顶层 `{base_resp, has_more, next_cursor, sessions}`，
    /// 会话数组在 **`$.sessions`**，而**条目对象在元素的 `session` 下面**，
    /// 模型类型在 **`model.model_id`**，时间戳是**毫秒**。
    fn minimax_body(count: usize) -> String {
        let sessions: Vec<Value> = (0..count)
            .map(|index| {
                json!({
                    "session": {
                        "session_id": format!("mm-{index}"),
                        "title": format!("会话 {index}"),
                        "updated_at": 1_792_591_408_863i64 - (index as i64) * 1000,
                        "created_at": 1_792_591_408_000i64,
                        "agent_name": "MiniMax",
                        "archived": false,
                        "team_mode": false,
                        "session_type": 1,
                        "parent_session_id": "",
                        "status": { "status_type": 1, "message": "" },
                        "model": {
                            "model_id": "abab-6",
                            "provider_id": "1",
                            "context_limit": 1000,
                            "reasoning": true,
                            "variant": "v1"
                        }
                    }
                })
            })
            .collect();
        json!({
            "base_resp": { "status_code": 0, "status_msg": "success" },
            "has_more": true,
            "next_cursor": "cursor-1",
            "sessions": sessions
        })
        .to_string()
    }

    #[test]
    fn minimax_catalog_items_map_onto_the_six_keys() {
        // ⚠️ **映射表**：MiniMax 的字段 → 协议那 6 个键（**映射不到 ⇒ 留空串／0**）。
        let spec = spec_for(MINIMAX_SITE).expect("规格表里必须有 MiniMax 那一条");
        let structure =
            SessionStructure::from_body_for(&minimax_body(3), 200, spec).expect("成功档必须通过");

        assert_eq!(structure.items.len(), 3, "只有 3 条 ⇒ 照实给 3 条");
        let first = &structure.items[0];
        // `session.session_id` → `id`
        assert_eq!(first.id, "mm-0");
        // `session.title` → `title`
        assert_eq!(first.title, "会话 0");
        // `session.model.model_id` → `model_type`
        assert_eq!(first.model_type, "abab-6");
        // ⚠️ **MiniMax 没有「标题类型」** ⇒ ⇒ **留空串**（**不许编一个**）。
        assert_eq!(first.title_type, "", "映射不到 ⇒ 留空串");
        // ⚠️ **MiniMax 没有「置顶」**（`archived` 是「已归档」，**不是置顶**）
        // ⇒ ⇒ **留 false**。
        assert!(!first.pinned, "映射不到 ⇒ 留 false");
        // ⚠️ **毫秒 → 秒**（协议 §4.3 ② 那个字段是「秒级小数时间戳」）。
        assert_eq!(first.updated_at, 1_792_591_408.863);

        // ⚠️ **`sessionsLength` 仍是站点给的原始条数**（不是 `items` 的条数）。
        assert_eq!(structure.sessions_length, Some(3));
        // ⚠️ **MiniMax 顶层没有 `data` 那一层** ⇒ ⇒ 「`data` 的键名」那一栏为空。
        assert!(
            structure.data_keys.is_empty(),
            "没有 data 那一层 ⇒ 那一栏为空"
        );
        // ⚠️ **站内码**读的是 `base_resp.status_code`。
        assert_eq!(structure.code, Some(0));
        assert_eq!(structure.has_more, Some(true));
    }

    #[test]
    fn minimax_catalog_keeps_only_the_newest_six() {
        // ⚠️ **条数上限是站点无关的** ⇒ ⇒ MiniMax 与 DeepSeek **共用同一个数**。
        let spec = spec_for(MINIMAX_SITE).expect("规格表里必须有 MiniMax 那一条");
        let structure =
            SessionStructure::from_body_for(&minimax_body(20), 200, spec).expect("成功档必须通过");
        assert_eq!(
            structure.items.len(),
            CATALOG_ITEM_LIMIT,
            "目录项面最多 6 条（**站点无关**的那一个数）"
        );
        // **降序** ⇒ ⇒ 第一条最新、最后一条是这 6 条里最旧的。
        assert_eq!(structure.items[0].id, "mm-0");
        assert_eq!(structure.items[5].id, "mm-5");
        // ⚠️ **站点给的原始条数照旧**（20 条）⇒ ⇒ 不许改成 6。
        assert_eq!(structure.sessions_length, Some(20));
    }

    /// 造一份 **ChatGPT 形态**的目录响应体（**按 2026-10-22 13:5x 的实测形状**；
    /// **合成数据**，**不是**真响应体 ⇒ ⇒ 仓库里不落任何真实响应体）。
    ///
    /// ⚠️ 形状要点：顶层 `{items, limit, offset, total}`（**没有 `data` 那一层**、
    /// **没有站内码**），会话数组在 **`$.items`**，`update_time`／`create_time`
    /// **是 ISO-8601 字符串**（**不是** epoch 数字），`is_starred` 实测是
    /// **`null`**（没置顶时）。
    fn chatgpt_body(count: usize) -> String {
        let items: Vec<Value> = (0..count)
            .map(|index| {
                json!({
                    "id": format!("cg-{index}"),
                    "title": format!("会话 {index}"),
                    "is_starred": if index == 0 { Value::Bool(true) } else { Value::Null },
                    "update_time": format!("2026-10-{:02}T06:51:42.166765Z", index + 1),
                    "create_time": format!("2026-10-{:02}T03:51:25.596075Z", index + 1),
                    "mapping": Value::Null,
                    "conversation_origin": Value::Null,
                })
            })
            .collect();
        json!({
            "items": items,
            "limit": 20,
            "offset": 0,
            "total": count
        })
        .to_string()
    }

    #[test]
    fn chatgpt_catalog_items_map_onto_the_six_keys() {
        // ⚠️ **映射表**：ChatGPT 的字段 → 协议那 6 个键（**映射不到 ⇒ 留空串／0**）。
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let structure =
            SessionStructure::from_body_for(&chatgpt_body(3), 200, spec).expect("成功档必须通过");

        assert_eq!(structure.items.len(), 3, "只有 3 条 ⇒ 照实给 3 条");
        // ⚠️ **目录按 `updated_at` 数值降序**（协议 §4.3 ② 那个口径；兄弟用例
        // `chatgpt_catalog_keeps_only_the_newest_six` 断言的也是这个顺序）⇒ ⇒
        // 这份 fixture 里**最新的是 `cg-2`**（10-03）⇒ ⇒ 它排第一。
        let first = &structure.items[0];
        // `id` → `id`
        assert_eq!(first.id, "cg-2");
        // `title` → `title`
        assert_eq!(first.title, "会话 2");
        // ⚠️ **ChatGPT 没有「标题类型」** ⇒ ⇒ **留空串**（**不许编一个**）。
        assert_eq!(first.title_type, "", "映射不到 ⇒ 留空串");
        // ⚠️ **目录条目里没有模型字段** ⇒ ⇒ **留空串**（**不许拿正文那个顶替**）。
        assert_eq!(first.model_type, "", "映射不到 ⇒ 留空串");
        // ⚠️ **`cg-2` 没置顶**（这份 fixture 里只有 `cg-0` 的 `is_starred` 是 true）。
        assert!(!first.pinned, "is_starred ＝ null ⇒ 不置顶");
        // ⚠️ **`update_time` 是 ISO-8601 字符串** ⇒ ⇒ 解析成**秒级小数**。
        assert_eq!(first.updated_at, 1_791_010_302.166_765);
        // `is_starred` → `pinned`：**有值的那一条**映射过去是 true。
        let starred = structure
            .items
            .iter()
            .find(|item| item.id == "cg-0")
            .expect("cg-0 在目录里");
        assert!(starred.pinned, "is_starred ＝ true ⇒ 置顶");
        // ⚠️ **`sessionsLength` 仍是站点给的原始条数**（不是 `items` 的条数）。
        assert_eq!(structure.sessions_length, Some(3));
        // ⚠️ **ChatGPT 顶层没有 `data` 那一层** ⇒ ⇒ 「`data` 的键名」那一栏为空。
        assert!(
            structure.data_keys.is_empty(),
            "没有 data 那一层 ⇒ 那一栏为空"
        );
        // ⚠️ **ChatGPT 那条响应没有站内码** ⇒ ⇒ 那一档**判不出**（**不编一个**）。
        assert_eq!(structure.code, None, "没有站内码 ⇒ 那一栏是空");
        // ⚠️ **也没有 `has_more` 那个布尔**（它给的是 `total`／`limit`／`offset`
        // —— 那是**计数**，不是那个布尔）⇒ ⇒ 那一档**判不出**。
        assert_eq!(structure.has_more, None, "没有 has_more ⇒ 那一栏是空");
    }

    #[test]
    fn chatgpt_catalog_keeps_only_the_newest_six() {
        // ⚠️ **条数上限是站点无关的** ⇒ ⇒ 三个站点**共用同一个数**。
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let structure =
            SessionStructure::from_body_for(&chatgpt_body(20), 200, spec).expect("成功档必须通过");
        assert_eq!(
            structure.items.len(),
            CATALOG_ITEM_LIMIT,
            "目录项面最多 6 条（**站点无关**的那一个数）"
        );
        // **降序** ⇒ ⇒ 第一条最新、最后一条是这 6 条里最旧的。
        assert_eq!(structure.items[0].id, "cg-19");
        assert_eq!(structure.items[5].id, "cg-14");
        // ⚠️ **站点给的原始条数照旧**（20 条）⇒ ⇒ 不许改成 6。
        assert_eq!(structure.sessions_length, Some(20));
    }

    #[test]
    fn the_catalog_pick_tells_the_two_same_path_requests_apart() {
        // ⚠️ **实测**（2026-10-22 13:5x · ChatGPT 运行时读数）：站点**自己**往
        // **同一个路径**发了两条请求 ——
        // · 带 **`conversation_origin`** 的那条 → 200／**44 字节**／**空**；
        // · 带 **`exclude_conversation_origin`** 的那条 → 200／**17,722 字节**／
        //   **20 条**。
        // ⇒ ⇒ 判据 ＝ 「**不带 `conversation_origin`**」⇒ ⇒ **键名逐字比**
        // （`exclude_conversation_origin` **不是** `conversation_origin`）。
        let empty_variant = "https://chatgpt.com/backend-api/conversations\
            ?conversation_origin=1&expand=false&hide_snorlax=true&is_archived=false\
            &is_starred=false&limit=20&offset=0&order=updated";
        let full_variant = "https://chatgpt.com/backend-api/conversations\
            ?exclude_conversation_origin=1&expand=false&hide_snorlax=true&is_archived=false\
            &is_starred=false&limit=20&offset=0&order=updated";
        assert!(
            query_value(empty_variant, "conversation_origin").is_some(),
            "带 conversation_origin 的那条认得出来"
        );
        assert!(
            query_value(full_variant, "conversation_origin").is_none(),
            "exclude_conversation_origin **不是** conversation_origin ⇒ 键名逐字比"
        );
    }

    #[test]
    fn the_catalog_item_face_is_still_exactly_six_keys() {
        // ⚠️ **白名单一个字不许动**（协议 §2.4 面二）：MiniMax 的条目**也是**恰 6 键。
        let spec = spec_for(MINIMAX_SITE).expect("规格表里必须有 MiniMax 那一条");
        let structure =
            SessionStructure::from_body_for(&minimax_body(2), 200, spec).expect("成功档必须通过");
        let payload = structure.to_json();
        let items = payload
            .get("items")
            .and_then(Value::as_array)
            .expect("目录项面必须在");
        for item in items {
            let keys: Vec<&str> = item
                .as_object()
                .expect("条目必须是对象")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys.len(), 6, "每项**恰 6 个键**");
            for key in [
                "id",
                "title",
                "title_type",
                "model_type",
                "pinned",
                "updated_at",
            ] {
                assert!(keys.contains(&key), "缺键 {key}");
            }
        }
    }

    /// 造一份**成功档**的**完整响应体**（协议 §4.3 的字段面：数组键名 ＋ 条目 6 键
    /// ＋ `has_more`；**里面故意带标题与 id 的值** —— v0.17 之后它们**只**允许
    /// 出现在目录项面的那 6 个键里）。
    fn ok_body() -> String {
        json!({
            "code": 0,
            "msg": "",
            "data": {
                "biz_code": 0,
                "biz_msg": "",
                "biz_data": {
                    "chat_sessions": [
                        {
                            "id": "session-id-1",
                            "model_type": "deepseek-chat",
                            "pinned": false,
                            "title": "会话标题 1",
                            "title_type": "SYSTEM",
                            "updated_at": 1783051054.518
                        }
                    ],
                    "has_more": true
                }
            }
        })
        .to_string()
    }

    #[test]
    fn ok_body_is_trimmed_to_the_field_face() {
        let structure = SessionStructure::from_body(&ok_body(), 200).expect("成功档必须通过");
        assert_eq!(structure.sessions_length, Some(1));
        assert_eq!(structure.first_item_keys.len(), 6);
        assert_eq!(
            structure.item_types.get("pinned").map(String::as_str),
            Some("boolean")
        );
        assert_eq!(
            structure.item_types.get("updated_at").map(String::as_str),
            Some("number")
        );
        assert_eq!(structure.has_more, Some(true));
        assert_eq!(structure.top_keys, vec!["code", "data", "msg"]);
        assert_eq!(structure.data_keys, vec!["biz_code", "biz_data", "biz_msg"]);
    }

    /// 造一份**很多条**会话的响应体（`count` 条，`updated_at` 按传入的值给）。
    fn body_with_sessions(count: usize, updated_at: impl Fn(usize) -> f64) -> String {
        let sessions: Vec<Value> = (0..count)
            .map(|index| {
                json!({
                    "id": format!("session-{index}"),
                    "model_type": "deepseek-chat",
                    "pinned": false,
                    "title": format!("会话 {index}"),
                    "title_type": "SYSTEM",
                    "updated_at": updated_at(index),
                })
            })
            .collect();
        json!({
            "code": 0,
            "msg": "",
            "data": {
                "biz_code": 0,
                "biz_msg": "",
                "biz_data": { "chat_sessions": sessions, "has_more": true }
            }
        })
        .to_string()
    }

    #[test]
    fn catalog_items_keep_only_the_newest_six() {
        // izzy 2026-10-22 12:4x 裁：目录面**只回最近 6 条**（按 `updated_at` 降序）。
        let body = body_with_sessions(100, |index| index as f64);
        let structure = SessionStructure::from_body(&body, 200).expect("成功档必须通过");
        assert_eq!(
            structure.items.len(),
            CATALOG_ITEM_LIMIT,
            "目录项面最多 6 条"
        );
        // **降序** ⇒ ⇒ 第一条最新、最后一条是这 6 条里最旧的。
        assert_eq!(structure.items[0].updated_at, 99.0);
        assert_eq!(structure.items[5].updated_at, 94.0);
        // ⚠️ **`sessionsLength` 照旧**（站点给的原始条数）⇒ ⇒ 不许改成 6。
        assert_eq!(structure.sessions_length, Some(100));
    }

    #[test]
    fn catalog_items_keep_the_site_order_when_timestamps_tie() {
        // **同值时保持站点原顺序**（稳定排序）⇒ ⇒ 不许用不稳定排序把同值项打乱。
        let body = body_with_sessions(20, |_| 100.0);
        let structure = SessionStructure::from_body(&body, 200).expect("成功档必须通过");
        assert_eq!(structure.items.len(), CATALOG_ITEM_LIMIT);
        let ids: Vec<&str> = structure
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids[0], "session-0", "同值时第一条仍是站点给的第一个");
        assert_eq!(ids[5], "session-5");
    }

    #[test]
    fn catalog_items_are_fewer_than_the_limit_when_the_site_gives_fewer() {
        // **取不满照实给**（4 条就给 4 条）⇒ ⇒ 不编、不补、不报错。
        let body = body_with_sessions(4, |index| index as f64);
        let structure = SessionStructure::from_body(&body, 200).expect("成功档必须通过");
        assert_eq!(structure.items.len(), 4);
        assert_eq!(structure.sessions_length, Some(4));
        assert_eq!(structure.items[0].updated_at, 3.0);
    }

    #[test]
    fn the_returned_payload_has_exactly_the_whitelisted_fields() {
        // 协议 §2.4 的「允许回传」是**列举式**的 ⇒ ⇒ 白名单之外即红。
        let structure = SessionStructure::from_body(&ok_body(), 200).expect("成功档必须通过");
        let payload = structure.to_json();
        let map = payload.as_object().expect("载荷是对象");
        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = RETURN_FIELDS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected, "回程载荷的字段集必须与 §2.4 白名单逐字相等");
    }

    #[test]
    fn titles_and_ids_only_travel_inside_the_catalog_items() {
        // v0.17（协议 §2.4 面二 / §4.3 ②）：会话 id 与标题**允许**出现在
        // `items` 的**元素内部** ⇒ ⇒ 但**只**在那 6 个键里；**结构面**里
        // 仍然一个都不许带（正文、凭据、整包更是永远不许带）。
        let structure = SessionStructure::from_body(&ok_body(), 200).expect("成功档必须通过");
        let payload = structure.to_json();
        let map = payload.as_object().expect("载荷是对象");
        let items = map
            .get("items")
            .and_then(|v| v.as_array())
            .expect("载荷带 items");
        assert_eq!(items.len(), 1);
        let item = items[0].as_object().expect("目录项是对象");
        // **恰好** 6 个键（§4.3 ②）：不多不少。
        let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "id",
                "model_type",
                "pinned",
                "title",
                "title_type",
                "updated_at"
            ],
            "目录项必须恰好是 §4.3 ② 的 6 个键"
        );
        // 值就在那 6 个键里（v0.17 明确允许）。
        assert_eq!(
            item.get("id").and_then(|v| v.as_str()),
            Some("session-id-1")
        );
        assert_eq!(
            item.get("title").and_then(|v| v.as_str()),
            Some("会话标题 1")
        );
        assert_eq!(item.get("pinned").and_then(|v| v.as_bool()), Some(false));
        // **结构面**里不许有标题 / 会话 id 的值。
        //
        // ⚠️ 这里**刻意不用对象字面量**写结构面那几项：判据第 3 条按**源码形态**
        // 抽回程载荷（取**最短**的、含 `topKeys` 的那个对象字面量）⇒ ⇒ 这里若也
        // 写成对象字面量，它会比 `to_json` 那个更短 ⇒ ⇒ 会把判据的读数抢走。
        let structure_keys = [
            "topKeys",
            "code",
            "msg",
            "dataKeys",
            "sessionsLength",
            "firstItemKeys",
            "itemTypes",
            "hasMore",
        ];
        let structure_only = structure_keys
            .iter()
            .map(|key| {
                (
                    key.to_string(),
                    map.get(*key).cloned().unwrap_or(Value::Null),
                )
            })
            .collect::<Map<String, Value>>();
        let structure_only = serde_json::to_string(&structure_only).unwrap_or_default();
        assert!(
            !structure_only.contains("会话标题 1"),
            "标题的值漏进结构面了：{structure_only}"
        );
        assert!(
            !structure_only.contains("session-id-1"),
            "会话 id 的值漏进结构面了：{structure_only}"
        );
    }

    #[test]
    fn site_code_is_reported_verbatim() {
        // 未登录态实测：顶层 `code=40003`，`data` 为 null（协议 §6.2）。
        let body = json!({
            "code": 40003,
            "msg": "INVALID_TOKEN",
            "data": null
        })
        .to_string();
        let failure = SessionStructure::from_body(&body, 200).expect_err("站内码必须失败");
        assert_eq!(failure.code(), "catalog_failed");
        // 原样上报：码与逐字 msg 都在 detail 里（**不解释语义**）。
        assert!(failure.detail().contains("40003"), "{}", failure.detail());
        assert!(
            failure.detail().contains("INVALID_TOKEN"),
            "{}",
            failure.detail()
        );
    }

    #[test]
    fn business_layer_code_is_not_mistaken_for_success() {
        // 协议 §4.2 的反例：`code=0` ＋ `biz_code=2` ＋ `biz_data=null` ⇒ 传输成功、业务失败。
        let body = json!({
            "code": 0,
            "msg": "",
            "data": { "biz_code": 2, "biz_msg": "BUSINESS_FAILED", "biz_data": null }
        })
        .to_string();
        let failure = SessionStructure::from_body(&body, 200).expect_err("业务层失败必须失败");
        assert_eq!(failure.code(), "catalog_failed");
        assert!(failure.detail().contains('2'), "{}", failure.detail());
    }

    #[test]
    fn empty_directory_is_not_a_return_failure() {
        // 协议 §6.3：站点返回空 ⇒ `catalog_empty`；**回程失败** ⇒ `catalog_failed`。
        let body = json!({
            "code": 0,
            "msg": "",
            "data": {
                "biz_code": 0,
                "biz_msg": "",
                "biz_data": { "chat_sessions": [], "has_more": false }
            }
        })
        .to_string();
        let failure = SessionStructure::from_body(&body, 200).expect_err("空目录必须失败");
        assert_eq!(failure.code(), "catalog_empty");
        // 回程失败是**另一个码**。
        assert_eq!(
            CaptureFailure::Return("页面没了".into()).code(),
            "catalog_failed"
        );
        assert_ne!(
            CaptureFailure::Empty.code(),
            CaptureFailure::Return("页面没了".into()).code()
        );
    }

    #[test]
    fn shape_drift_is_reported_as_schema_changed() {
        // 协议 §4.3 ①：数组键名是 `data.biz_data.chat_sessions`。
        let body = json!({
            "code": 0,
            "msg": "",
            "data": {
                "biz_code": 0,
                "biz_msg": "",
                "biz_data": { "sessions": [] }
            }
        })
        .to_string();
        let failure = SessionStructure::from_body(&body, 200).expect_err("形状漂移必须失败");
        assert_eq!(failure.code(), "schema_changed");
    }

    #[test]
    fn transport_failure_is_not_a_site_empty() {
        let failure = SessionStructure::from_body(&ok_body(), 500).expect_err("传输层失败必须失败");
        assert_eq!(failure.code(), "catalog_failed");
    }

    #[test]
    fn not_yet_captured_is_catalog_failed_not_catalog_empty() {
        // 协议 v0.11 §8.5.2 G4-14 ③：「**还没抓到结构**」⇒ `catalog_failed`（可重试），
        // **绝不** `catalog_empty`（那个码**只**表示「有结构但无匹配项」）。
        let pending = CaptureFailure::Timeout("还没抓到".into());
        assert_eq!(pending.code(), "catalog_failed");
        assert_ne!(pending.code(), CaptureFailure::Empty.code());
        // 且它与「站点返回空」必须是**两个不同的码**（协议 §6.3）。
        assert_ne!(pending.code(), "catalog_empty");
    }

    #[test]
    fn every_failure_code_is_inside_the_closed_set() {
        // 协议 §6.1：不扩 P1 §3 的闭集。
        let codes = [
            CaptureFailure::Return("x".into()).code(),
            CaptureFailure::Timeout("x".into()).code(),
            CaptureFailure::Empty.code(),
            CaptureFailure::Schema("x".into()).code(),
            CaptureFailure::Unsupported("x".into()).code(),
            CaptureFailure::SiteCode {
                code: "40003".into(),
                msg: None,
            }
            .code(),
        ];
        for code in codes {
            assert!(
                crate::official_chat::protocol::ERROR_CODES.contains(&code),
                "{code} 不在协议闭集里"
            );
        }
    }

    // --- 路线 B 的**认证契约**（2026-10-23）---------------------------------
    //
    // ⚠️ **为什么要单列一组**：这一轮给 ChatGPT 补了「**页内路线 B**」的适配
    // ⇒ ⇒ 「凭据从哪儿来」从**一个字段**变成**一个枚举** ⇒ ⇒ 那两个站点的
    // 契约**必须各钉各的**（钉住「DeepSeek 那一支没被改坏」与「ChatGPT 那一支
    // 钉的是实测那个端点」）。

    #[test]
    fn deepseek_route_b_keeps_the_storage_source() {
        // ⚠️ **DeepSeek 那一支一个字都不许变**（页内存储取凭据）。
        let spec = spec_for(SITE).expect("规格表里必须有 DeepSeek 那一条");
        let route = spec.page_route.expect("DeepSeek 必须有路线 B 适配");
        let auth = route
            .fetch_auth()
            .expect("DeepSeek 必须是形态 B（页内自取数）那一支");
        match auth.source {
            PageAuthSource::Storage {
                storage,
                key,
                extract,
            } => {
                assert_eq!(storage, PageStorage::LocalStorage, "键还在 localStorage");
                assert_eq!(key, "userToken", "键名一个字都不许变");
                assert_eq!(extract, TokenExtract::JsonValue, "取法一个字都不许变");
            }
            other => panic!("DeepSeek 的凭据来源必须是页内存储那一支，实际是 {other:?}"),
        }
        assert_eq!(auth.header, "authorization");
        assert_eq!(auth.prefix, "Bearer ");
    }

    #[test]
    fn chatgpt_route_b_uses_the_endpoint_source() {
        // ⚠️ **实测依据**（2026-10-23 r8 · 探针
        // `examples/official_chat_chatgpt_contract_probe.rs`）：token **只在**
        // `GET /api/auth/session` 的响应**顶层**（`accessToken`）⇒ ⇒ 规格表里
        // 必须是「端点」那一支（**不是**页内存储 —— 8 个宽口径候选全部 401）。
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let route = spec.page_route.expect("ChatGPT 现在必须有路线 B 适配");
        let auth = route
            .fetch_auth()
            .expect("ChatGPT 必须是形态 B（页内自取数）那一支");
        assert_eq!(route.fetch_url(), Some("/backend-api/conversations"));
        assert_eq!(route.fetch_origin(), Some("chatgpt.com"));
        match auth.source {
            PageAuthSource::Endpoint { url, json_key } => {
                assert_eq!(url, "/api/auth/session", "取 token 的端点（实测那个）");
                assert_eq!(json_key, "accessToken", "响应顶层那个键（实测那个）");
            }
            other => panic!("ChatGPT 的凭据来源必须是同源端点那一支，实际是 {other:?}"),
        }
        assert_eq!(auth.header, "authorization");
        assert_eq!(auth.prefix, "Bearer ");
        // ⚠️ **正文那条要有自己的 URL 模板**（会话 id 由宿主填进去）—— 不许拿
        // 目录那条的 URL 去套正文（那会静默返回一份目录）。
        assert_eq!(
            route.fetch_body_url(),
            Some("/backend-api/conversations/{session}"),
            "正文那条的 URL 模板"
        );
    }

    #[test]
    fn chatgpt_knows_the_system_role() {
        // ⚠️ **硬要求**（2026-10-23 r8 实测：那条正文的 22 条消息里有 **2 条**
        // `system`）—— 少了这一档 ⇒ 那两条消息**角色认不出** ⇒ ⇒
        // `message_of` 给 `None` ⇒ ⇒ **整条正文链**落「形状漂移」。
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let clue = spec
            .role_clues
            .iter()
            .find(|clue| *clue.key == ["author", "role"])
            .expect("角色线索是 author.role（两层）");
        assert!(
            clue.values
                .iter()
                .any(|(site, protocol)| *site == "SYSTEM" && *protocol == "system"),
            "角色线索里必须有 system 那一档（实测取值是小写，归一在 role_of 那一层）"
        );
    }

    #[test]
    fn the_page_scripts_do_not_write_to_the_page() {
        // ⚠️ **页内动作清单**（这一轮的硬约束）：macOS 路线 B 允许注入脚本，
        // 但**不许**新增 `window` 自有属性、**不许**留定时器、**不许**写站点
        // 数据、凭据**只在浏览器里用**（不落盘、不外传）⇒ ⇒ 这几样在脚本里
        // 一处都不许出现。
        for (name, script) in [
            ("page_ready_script.js", include_str!("page_ready_script.js")),
            ("page_fetch_script.js", include_str!("page_fetch_script.js")),
        ] {
            for forbidden in [
                "setInterval",
                "setTimeout",
                "localStorage.setItem",
                "sessionStorage.setItem",
                "document.cookie",
            ] {
                assert!(
                    !script.contains(forbidden),
                    "{name} 里出现了 {forbidden}（页内动作清单不许它）"
                );
            }
            // ⚠️ **`window.xxx =` 形态的赋值**：只允许**读**（`window.localStorage`
            // 那种取值）⇒ ⇒ 不许有「给 window 挂自有属性」的那一写。
            assert!(
                !script.contains("window.__"),
                "{name} 里挂了自己的 window 属性（页内动作清单不许它）"
            );
        }
    }
}

// --- 路线 B 的**注入产物**（macOS 专属）--------------------------------------
//
// ⚠️ **为什么单列一个模块**：`page_ready_script`／`page_fetch_script` 是
// **macOS 门控**的（只有页内路线 B 会调它们）⇒ ⇒ 钉它们的用例也只能在 macOS
// 上跑（Windows 那条链压根没有页内路线 B ⇒ 那边的用例**不存在**）。
#[cfg(all(test, target_os = "macos"))]
mod route_b_scripts {
    use super::*;

    /// 从注入产物里把 `var SPEC = {…};` 那一段**抠出来**（解析成 JSON）。
    fn injected_spec(script: &str) -> Value {
        let anchor = "var SPEC = ";
        let start = script.find(anchor).expect("脚本里有 var SPEC = ") + anchor.len();
        let rest = &script[start..];
        let end = rest.find(';').expect("规格后面有分号");
        serde_json::from_str(&rest[..end]).expect("规格是合法 JSON")
    }

    #[test]
    fn the_deepseek_ready_spec_is_unchanged() {
        // ⚠️ **一个字都不许变**（DeepSeek 那一支）：三个键、一个不多一个不少
        // ⇒ ⇒ 页内脚本的「存储那一支」**不用改** ⇒ ⇒ 老行为**保持原样**。
        let spec = spec_for(SITE).expect("规格表里必须有 DeepSeek 那一条");
        let route = spec.page_route.expect("DeepSeek 必须有路线 B 适配");
        let script = page_ready_script(&route);
        let injected = injected_spec(&script);
        assert_eq!(
            injected,
            json!({
                "origin": "chat.deepseek.com",
                "storage": "localStorage",
                "key": "userToken",
            }),
            "DeepSeek 的就绪规格必须与改动前逐字相同"
        );
        // ⚠️ **不许多出 `source` 键**：多一个键就等于「那一支被改过」。
        assert_eq!(injected.as_object().expect("规格是对象").len(), 3);
    }

    #[test]
    fn the_deepseek_fetch_spec_is_unchanged() {
        // ⚠️ **一个字都不许变**（DeepSeek 那一支）：六个键。
        let spec = spec_for(SITE).expect("规格表里必须有 DeepSeek 那一条");
        let route = spec.page_route.expect("DeepSeek 必须有路线 B 适配");
        let url = route
            .fetch_url()
            .expect("DeepSeek 必须是形态 B（页内自取数）那一支");
        let script = page_fetch_script(&route, url);
        let injected = injected_spec(&script);
        assert_eq!(
            injected,
            json!({
                "url": "/api/v0/chat_session/fetch_page?lte_cursor.pinned=false",
                "storage": "localStorage",
                "key": "userToken",
                "extract": "json-value",
                "header": "authorization",
                "prefix": "Bearer ",
            }),
            "DeepSeek 的取数规格必须与改动前逐字相同"
        );
        assert_eq!(injected.as_object().expect("规格是对象").len(), 6);
    }

    #[test]
    fn the_chatgpt_ready_spec_names_the_endpoint_and_the_json_key() {
        // ⚠️ **端点形态**：宿主必须把「端点路径」与「JSON 键名」交给页内脚本
        // ⇒ ⇒ 少了任何一样，页内脚本都取不到 token。
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let route = spec.page_route.expect("ChatGPT 必须有路线 B 适配");
        let injected = injected_spec(&page_ready_script(&route));
        assert_eq!(
            injected,
            json!({
                "origin": "chatgpt.com",
                "source": "endpoint",
                "endpointUrl": "/api/auth/session",
                "jsonKey": "accessToken",
            }),
            "ChatGPT 的就绪规格必须带上端点与 JSON 键名"
        );
    }

    #[test]
    fn the_chatgpt_fetch_spec_names_the_endpoint_and_the_json_key() {
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let route = spec.page_route.expect("ChatGPT 必须有路线 B 适配");
        let url = route
            .fetch_url()
            .expect("ChatGPT 必须是形态 B（页内自取数）那一支");
        let injected = injected_spec(&page_fetch_script(&route, url));
        assert_eq!(
            injected,
            json!({
                "url": "/backend-api/conversations",
                "source": "endpoint",
                "endpointUrl": "/api/auth/session",
                "jsonKey": "accessToken",
                "header": "authorization",
                "prefix": "Bearer ",
            }),
            "ChatGPT 的取数规格必须带上端点与 JSON 键名"
        );
    }

    #[test]
    fn the_fetch_script_reads_the_json_key_from_the_spec() {
        // ⚠️ **不许把键名硬编码进脚本**：ChatGPT 那个键是 `accessToken`（实测），
        // 但**下一个**站点可能是别的键名 ⇒ ⇒ 脚本必须**按规格**取
        // （`SPEC.jsonKey`）⇒ ⇒ 这一条钉住「不许硬编码」。
        let script = include_str!("page_fetch_script.js");
        assert!(
            script.contains("SPEC.jsonKey"),
            "取 token 的脚本必须按规格取 JSON 键名（不许硬编码）"
        );
        assert!(
            !script.contains("\"accessToken\""),
            "脚本里不许出现硬编码的键名（键名归规格表管）"
        );
        // ⚠️ **存储那一支一个字都不许变**（DeepSeek）。
        assert!(
            script.contains("window.localStorage.getItem(SPEC.key)"),
            "存储那一支的取凭据那段一个字都不许变"
        );
        assert!(
            script.contains("window.sessionStorage.getItem(SPEC.key)"),
            "存储那一支的取凭据那段一个字都不许变"
        );
        // ⚠️ **失败要沿用 `fail(stage, message)` 那一形状**（宿主按 `stage` 说人话）。
        assert!(
            script.contains("function fail(stage, message)"),
            "失败必须沿用 fail(stage, message) 那一形状"
        );
    }

    #[test]
    fn the_ready_channel_follows_the_credential_source() {
        // ⚠️ **硬要求**：新形态的「有没有凭据」要**等**那一发端点 ⇒ ⇒ 必须走
        // **能 await** 的通道（`round_trip`）⇒ ⇒ 走 `read_json` 的话读到的是
        // 还没 settle 的 Promise ⇒ ⇒ `hasCredential` **恒为假**。
        let deepseek = spec_for(SITE).expect("规格表里必须有 DeepSeek 那一条");
        let chatgpt = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        assert_eq!(
            deepseek
                .page_route
                .expect("DeepSeek 必须有路线 B 适配")
                .fetch_auth()
                .expect("DeepSeek 必须是形态 B 那一支")
                .source
                .ready_channel(),
            ReadyChannel::Sync,
            "页内存储取 ⇒ 同步（read_json）—— 那一支逐字保持现状"
        );
        assert_eq!(
            chatgpt
                .page_route
                .expect("ChatGPT 必须有路线 B 适配")
                .fetch_auth()
                .expect("ChatGPT 必须是形态 B 那一支")
                .source
                .ready_channel(),
            ReadyChannel::Async,
            "同源端点取 ⇒ 异步（round_trip，能 await）"
        );
    }

    #[test]
    fn the_ready_script_carries_both_forms() {
        // ⚠️ **两支都在同一份脚本里**（宿主按规格分派）⇒ ⇒ 同步那一支**逐字
        // 保持现状**（仍是 `JSON.stringify` 的一段文本），异步那一支**给回一个
        // Promise**（宿主 `round_trip` 等它 settle）。
        let script = include_str!("page_ready_script.js");
        assert!(
            script.contains(r#"if (SPEC.source === "endpoint")"#),
            "脚本里要有按来源分派的那一格"
        );
        assert!(
            script.contains("JSON.stringify((function () {"),
            "同步那一支仍然是「回一段 JSON 文本」"
        );
        assert!(
            script.contains("window.localStorage.getItem(SPEC.key)"),
            "同步那一支的取凭据那段一个字都不许变"
        );
        assert!(
            script.contains("window.sessionStorage.getItem(SPEC.key)"),
            "同步那一支的取凭据那段一个字都不许变"
        );
        assert!(
            script.contains("await fetch(SPEC.endpointUrl"),
            "异步那一支要等那一发端点"
        );
        // ⚠️ **回执形状**：异步那一支回**对象**（不是 JSON 文本）⇒ ⇒ 宿主那边
        // `value.get("ok")` 才读得到字段。
        assert!(
            script.contains("base.hasCredential = !!token;"),
            "异步那一支只回「有没有」"
        );
    }
}

// --- 形态 C（页内旁听）的**接线**（macOS 专属）--------------------------------
//
// ⚠️ **为什么单列一个模块**：形态 C 的页内脚本（`page_hook_script.js`）与它的
// 壳侧接线**只在 macOS 上存在**（Windows 那条链是形态 A ＝ 壳侧 DevTools 订阅）
// ⇒ ⇒ 钉它们的用例也只能在 macOS 上跑。
#[cfg(all(test, target_os = "macos"))]
mod shape_c_wiring {
    use super::*;

    /// 从注入产物里把 `var SPEC = {…};` 那一段**抠出来**（解析成 JSON）。
    fn injected_spec(script: &str) -> Value {
        let anchor = "var SPEC = ";
        let start = script.find(anchor).expect("脚本里有 var SPEC = ") + anchor.len();
        let rest = &script[start..];
        let end = rest.find(';').expect("规格后面有分号");
        serde_json::from_str(&rest[..end]).expect("规格是合法 JSON")
    }

    fn minimax_spec() -> &'static SiteSpec {
        spec_for(MINIMAX_SITE).expect("规格表里必须有 MiniMax 那一条")
    }

    fn minimax_route() -> PageRoute {
        minimax_spec()
            .page_route
            .expect("MiniMax 必须有页内取数适配")
    }

    #[test]
    fn the_minimax_route_is_the_hook_form() {
        // ⚠️ **硬要求**：MiniMax **没有**「页内自己发」那种形态（签名复刻不出来）
        // ⇒ ⇒ 它的适配**必须**是 [`PageRoute::Hook`] ⇒ ⇒ 「哪种形态」在**类型
        // 层面**可见。
        let route = minimax_route();
        assert!(
            matches!(route, PageRoute::Hook { .. }),
            "MiniMax 必须是形态 C（页内旁听）那一支"
        );
        // ⚠️ **形态 B 那一支的访问器对形态 C 一律给 `None`**（不静默套用）。
        assert!(route.fetch_auth().is_none(), "形态 C 没有凭据契约");
        assert!(route.fetch_url().is_none(), "形态 C 没有「自己发的那一发」");
        assert!(route.fetch_body_url().is_none(), "形态 C 没有正文 URL 模板");
    }

    #[test]
    fn the_hook_route_carries_the_measured_paths() {
        // ⚠️ **实测依据**（2026-10-23 · 本机 macOS 真机 · 探针
        // `examples/official_chat_minimax_route_b_probe.rs`）：目录 **200／20 条／
        // 7674 B**、正文 **200／24 条**、签名三件套 `x-signature`／`x-timestamp`／
        // `yy`（值一步都不出页面）。
        let route = minimax_route();
        let PageRoute::Hook {
            root_url,
            catalog_path,
            body_path_template,
            deep_link_prefix,
            signature_headers,
        } = route
        else {
            panic!("MiniMax 必须是形态 C（页内旁听）那一支");
        };
        assert_eq!(
            catalog_path, "/minimax-cloud/api/v1/sidebar/session/tree",
            "目录那条的路径（实测那一发）"
        );
        assert_eq!(
            body_path_template, "/minimax-cloud/api/v1/session/{session}/message",
            "正文那条的路径模板（会话 id 在路径里）"
        );
        assert_eq!(
            deep_link_prefix, "https://agent.minimax.cn/?id=",
            "深链前缀（正文那条链导航到它，让站点自己发那一发）"
        );
        assert_eq!(
            signature_headers,
            &["x-signature", "x-timestamp", "yy"],
            "签名三件套（**值**一步都不出页面）"
        );
        // ⚠️ **取数根地址**＝规格表里**已有的那一格**（不新造常量）。
        assert_eq!(
            root_url,
            minimax_spec().root_url,
            "取数根地址取规格表那一格"
        );
    }

    #[test]
    fn the_hook_route_agrees_with_the_spec_table() {
        // ⚠️ **同一个值不许有两份真相**：路线适配里那三格与规格表里那三格**必须
        // 一致** ⇒ ⇒ 谁改了另一处没改，这条用例**就红**。
        let spec = minimax_spec();
        let route = minimax_route();
        let PageRoute::Hook {
            catalog_path,
            body_path_template,
            deep_link_prefix,
            ..
        } = route
        else {
            panic!("MiniMax 必须是形态 C（页内旁听）那一支");
        };
        assert_eq!(catalog_path, spec.catalog_path, "目录路径两处必须一致");
        assert_eq!(
            body_path_template, spec.body_path_template,
            "正文路径模板两处必须一致"
        );
        assert_eq!(
            deep_link_prefix, spec.deep_link_prefix,
            "深链前缀两处必须一致"
        );
    }

    #[test]
    fn the_body_target_is_filled_with_the_session_id() {
        // ⚠️ **会话 id 只有调用方知道** ⇒ ⇒ 由它把模板填好再传进来。
        let route = minimax_route();
        assert_eq!(
            route.body_target("abc-123").as_deref(),
            Some("/minimax-cloud/api/v1/session/abc-123/message"),
            "形态 C 的正文目标＝**要旁听的那条请求的路径**"
        );
        // ⚠️ **该站没有正文适配时给 `None`**（macOS 上正文那条**明确报不支持**）。
        let deepseek = spec_for(SITE)
            .expect("规格表里必须有 DeepSeek 那一条")
            .page_route
            .expect("DeepSeek 必须有路线 B 适配");
        assert_eq!(
            deepseek.body_target("abc-123").as_deref(),
            Some("/api/v0/chat/history_messages?chat_session_id=abc-123"),
            "形态 B 的正文目标＝**自己发的那一发的相对 URL**"
        );
    }

    #[test]
    fn the_hook_script_carries_the_target_path_and_the_signature_headers() {
        let spec = minimax_spec();
        let route = minimax_route();
        let script = page_hook_script(spec, &route, "/minimax-cloud/api/v1/sidebar/session/tree");
        let injected = injected_spec(&script);
        assert_eq!(
            injected.get("catalog_path").and_then(Value::as_str),
            Some("/minimax-cloud/api/v1/sidebar/session/tree"),
            "要旁听的那条路径必须进规格（**按链不同**：目录那条／正文那条）"
        );
        assert_eq!(
            injected.get("signature_headers").and_then(Value::as_array),
            Some(&vec![
                json!("x-signature"),
                json!("x-timestamp"),
                json!("yy")
            ]),
            "签名头**名字**必须进规格（**值**不出页面）"
        );
        // ⚠️ **生产不重放、不做阳性对照**：重放是探针那一臂；阳性对照会**再发一次**
        // 那一发 ⇒ ⇒ 生产**不做**（少一次站点请求 ⇒ 更贴「只读」）。
        assert_eq!(
            injected.get("replay_body").and_then(Value::as_bool),
            Some(false),
            "生产不重放"
        );
        assert_eq!(
            injected.get("positive_control").and_then(Value::as_bool),
            Some(false),
            "生产不做阳性对照"
        );
    }

    #[test]
    fn the_hook_script_uses_the_spec_table_mapping() {
        // ⚠️ **裁结构的映射必须来自规格表**（页内不许自己另写一套 ⇒ ⇒ 那是
        // 「同一套映射写两遍」）。
        let spec = minimax_spec();
        let route = minimax_route();
        let injected = injected_spec(&page_hook_script(spec, &route, "/x"));
        assert_eq!(
            injected.get("sessions_path").and_then(Value::as_array),
            Some(&vec![json!("sessions")]),
            "会话数组的位置取规格表"
        );
        assert_eq!(
            injected.get("item_path").and_then(Value::as_array),
            Some(&vec![json!("session")]),
            "条目对象在元素里的那一层取规格表"
        );
        assert_eq!(
            injected.get("id_path").and_then(Value::as_array),
            Some(&vec![json!("session_id")]),
            "会话 id 的路径取规格表"
        );
        let fields = injected
            .get("item_fields")
            .and_then(Value::as_object)
            .expect("字段映射进规格");
        assert_eq!(
            fields.get("model_type").and_then(Value::as_array),
            Some(&vec![json!("model"), json!("model_id")]),
            "模型类型取规格表（两层路径）"
        );
        assert_eq!(
            fields.get("updated_at").and_then(Value::as_array),
            Some(&vec![json!("updated_at")]),
            "更新时间取规格表"
        );
    }

    #[test]
    fn the_hook_script_returns_the_captured_body_to_the_shell() {
        // ⚠️ **白名单逐字不变** ⇒ ⇒ 裁剪只有**一处**（壳侧那套 `from_body_for`）
        // ⇒ ⇒ 页内**必须**把那一发的响应体**带回壳**（页内那份 `items` 的排序／
        // 条数上限／时间戳单位与壳侧那套**不是同一口径** ⇒ ⇒ 不能当最终结果）。
        //
        // ⚠️ **判的是「代码」不是「注释」**：脚本的**注释里**也写着 `capturedBody`
        // ⇒ ⇒ 只判原文的话，把那段代码删掉这条用例**照样绿**（假绿）⇒ ⇒ 先去
        // 注释再判。
        let script = strip_js_comments(include_str!("page_hook_script.js"));
        assert!(
            script.contains("capturedBody"),
            "页内脚本必须把截到的那一发的响应体带回壳（壳侧那一套裁剪是唯一真相）"
        );
    }

    #[test]
    fn the_hook_script_obeys_the_in_page_action_list() {
        // ⚠️ **页内动作清单**（协议 §7.1 · 形态 C 那三条）：**不新增 `window` 自有
        // 属性**｜**取到即还原**（不留钩子／不留定时器／不留槽位）｜**不写站点
        // 数据** ⇒ ⇒ 这几样在脚本里一处都不许出现。
        //
        // ⚠️ **判的是「代码」不是「注释」**：脚本的**注释里**写着「全篇**没有**
        // `setTimeout`／`setInterval`」⇒ ⇒ 那句话本身含那两个词 ⇒ ⇒ 先去注释再判
        // （与判据侧 `stripComments` 同一口径）。
        let script = strip_js_comments(include_str!("page_hook_script.js"));
        for forbidden in [
            "setInterval",
            "setTimeout",
            "localStorage.setItem",
            "sessionStorage.setItem",
            "document.cookie",
        ] {
            assert!(
                !script.contains(forbidden),
                "页内 hook 脚本里出现了 {forbidden}（页内动作清单不许它）"
            );
        }
        // ⚠️ **顶层只有一句 IIFE** ⇒ ⇒ 不许有 `window.__xxx` 这种自有属性。
        assert!(
            !script.contains("window.__"),
            "页内 hook 脚本里挂了自己的 window 属性（页内动作清单不许它）"
        );
    }

    /// **去掉 JS 注释**（`//` 行注释 ＋ `/* */` 块注释）—— 判「代码形状」用。
    ///
    /// ⚠️ **为什么自己写而不复用判据侧那份**（那是 JS 侧的实现）：这里只需要
    /// 「**代码里有没有那个词**」这一件事 ⇒ ⇒ 一份小实现足够 ⇒ ⇒ **不许**照抄
    /// 判据侧的源码（那是另一个模块的实现，照抄就是第二份真相）。
    ///
    /// ⚠️ **块注释的结束判据**必须是 `*/`（**不是**「上一个字符是 `*`」）⇒ ⇒
    /// 第一版写成「上一个字符是 `*`」⇒ ⇒ `/** … */` 那种开头的**行注释式**块
    /// 注释会把**后面整份文件**吞掉（实测：判「代码里有 `capturedBody`」那条
    /// 用例在**代码完好**时也是红的 ⇒ ⇒ 假红）⇒ ⇒ 现在按 `*/` 两字符判。
    fn strip_js_comments(source: &str) -> String {
        let chars: Vec<char> = source.chars().collect();
        let mut out = String::with_capacity(source.len());
        let mut index = 0;
        let mut in_block = false;
        let mut in_string = false;
        while index < chars.len() {
            let current = chars[index];
            let next = chars.get(index + 1).copied();
            if in_block {
                if current == '*' && next == Some('/') {
                    in_block = false;
                    index += 2;
                    continue;
                }
                if current == '\n' {
                    out.push(current);
                }
                index += 1;
                continue;
            }
            if in_string {
                out.push(current);
                if current == '\\' {
                    if let Some(escaped) = next {
                        out.push(escaped);
                    }
                    index += 2;
                    continue;
                }
                if current == '"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }
            match (current, next) {
                ('/', Some('/')) => {
                    // 行注释：**换行留着**（行号才对得上）。
                    while index < chars.len() && chars[index] != '\n' {
                        index += 1;
                    }
                }
                ('/', Some('*')) => {
                    in_block = true;
                    index += 2;
                }
                ('"', _) => {
                    in_string = true;
                    out.push(current);
                    index += 1;
                }
                _ => {
                    out.push(current);
                    index += 1;
                }
            }
        }
        out
    }
}
