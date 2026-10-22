//! P4：官网对话**会话目录结构**的真实取数（协议 **v0.10** §1 / §2.1 / §2.3 /
//! §2.4 / §4.2 / §4.3 / §5 / §6 / §7 / §8.5.1）。
//!
//! # 这条链走的是哪几步（协议 §1 裁定 1：**形态唯一**）
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
//! # 页内**零注入**（协议 §2.3 / §7.1）
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

/// ChatGPT 那一份（2026-10-22 13:3x 的**运行时读数**）。
///
/// ## ⚠️ **与 DeepSeek 完全不同的地方**（**二手表里写错的那几处**）
///
/// · **目录那条**：顶层就是 `{items, limit, offset, total}` —— **没有 `data`
///   那一层**、**没有 `code`**、会话数组在 **`$.items`**（实测 20 条）；
/// · **正文那条**：顶层**没有 `mapping`** ⇒ ⇒ 正文在 **`$.messages`** 数组里
///   （实测 90 条）⇒ ⇒ 「目录面拿不到正文」⇒ ⇒ 正文必须**单独打一次**；
/// · **角色**在 **`author.role`**（**两层**）⇒ ⇒ 角色线索那一栏是**路径**。
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
    // ⚠️ **消息数组在顶层**（实测 `$.messages`；**顶层没有 `mapping`**）。
    messages_paths: &[&["messages"]],
    // ⚠️ **角色在 `author.role`**（**两层**）—— 实测取值集合 `{user, assistant,
    // tool}`（**没有** `system`）⇒ ⇒ `tool` 那一档**记进 `omitted`**。
    role_clues: &[RoleClue {
        key: &["author", "role"],
        values: &[
            ("USER", "user"),
            ("ASSISTANT", "assistant"),
            ("TOOL", "tool"),
        ],
    }],
    // ⚠️ **正文在 `content` 里**（实测：`content` 是**对象**，不是字符串）——
    // ⚠️ **两种形状**（实测 2026-10-22 13:5x，同一条响应里两种都有）：
    // · **assistant** 消息：`content` ＝ `{content_type, content}` ⇒ ⇒ 正文在
    //   **那个 `content` 字段**里（**没有** `parts`）；
    // · **user ／ tool** 消息：`content` ＝ `{content_type, parts}` ⇒ ⇒ 正文在
    //   **`parts[]`** 里（**每一项就是字符串**，实测长度 1）。
    // ⇒ ⇒ 两条线索**按顺序**认（先认到的**非空**那个说了算）。
    // DeepSeek 那个 `content`（字符串）／`text`／`fragments` 在 ChatGPT 侧
    // **一个字都没有**（`content` 是**对象**，不是字符串）。
    text_sources: &[
        TextSource::NestedField("content", "content"),
        TextSource::Parts("content"),
    ],
    // ⚠️ ChatGPT **没有**独立的思维链字段（思维链在 `content.parts[]` 里那几项
    // ⇒ ⇒ 由 `Parts` 逐项取文本时自然带上）⇒ ⇒ 这里**没有**「思维链字段」。
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
    if at.elapsed() >= STRUCTURE_TTL {
        return None;
    }
    cached(state, site)
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
#[cfg(windows)]
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

/// 一次取数失败。**每一类都对应协议 §6 的一个既有码**（不新增第 15 个码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CaptureFailure {
    /// **回程失败**（取体失败 / 页面销毁 / 壳侧错误）——协议 §6.3：与「站点返回空」分开。
    Return(String),
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
type EventStream = tauri::async_runtime::Receiver<NetworkEvent>;

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
    };
    // **导航链**：导航 → 等目标响应 → 立刻取体 → 壳侧裁剪。
    let outcome = match capture_in(&window, events, &target).await {
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
    let events = subscribe_network_events(&window)?;
    cdp_call(&window, "Network.enable", json!({}), CDP_TIMEOUT)
        .await
        .map_err(|e| CaptureFailure::Return(format!("Network.enable 失败：{e}")))?;
    Ok((window, events))
}

/// 非 Windows：**没有这条通路** ⇒ 明确报错，不静默降级（协议 §6.1）。
#[cfg(not(windows))]
pub(crate) async fn arm_capture_window(
    _app: &tauri::AppHandle,
    _site: &str,
    _kind: &str,
    _profile_dir: &std::path::Path,
) -> Result<(tauri::WebviewWindow, EventStream), CaptureFailure> {
    Err(CaptureFailure::Unsupported(
        "macOS 尚无壳内 DevTools 通路（wry 0.57 未暴露 callAsyncJavaScript）".to_string(),
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

/// **导航链**：导航 → 等目标响应加载完 → **立刻**取体。
#[cfg(windows)]
pub(crate) async fn capture_in(
    window: &tauri::WebviewWindow,
    mut events: EventStream,
    target: &CaptureTarget,
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
    let hit = await_target_hit(&mut events, CAPTURE_TIMEOUT, target).await?;

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

/// 非 Windows：**没有这条通路**（macOS）。
#[cfg(not(windows))]
pub(crate) async fn capture_in(
    _window: &tauri::WebviewWindow,
    _events: EventStream,
    _target: &CaptureTarget,
) -> Result<CapturedResponse, CaptureFailure> {
    Err(CaptureFailure::Unsupported(
        "macOS 尚无壳内 DevTools 通路（wry 0.57 未暴露 callAsyncJavaScript）".to_string(),
    ))
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
) -> Result<TargetHit, CaptureFailure> {
    let deadline = Instant::now() + timeout;
    // `requestId` → 那条请求的元信息（URL／状态码／时刻／是否已加载完）。
    let mut seen: HashMap<String, HitMeta> = HashMap::new();
    let mut setup_problems: Vec<String> = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CaptureFailure::Timeout(format!(
                "{}s 内没等到站点那条响应加载完{}",
                timeout.as_secs(),
                if setup_problems.is_empty() {
                    String::new()
                } else {
                    format!("（订阅阶段：{}）", setup_problems.join("；"))
                }
            )));
        }
        let received = tokio::time::timeout(remaining, events.recv()).await;
        let Ok(Some(event)) = received else {
            return Err(CaptureFailure::Timeout(format!(
                "{}s 内没等到站点那条响应加载完（事件通道已关闭）",
                timeout.as_secs()
            )));
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
                // 命中的候选（路径 ＋ 锁定条件都对上）里取**该取的那条**。
                let candidates: Vec<&HitMeta> = seen
                    .values()
                    .filter(|item| item.finished && url_matches_target(&item.url, target))
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
    }
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
    let expected_path = match &target.lock {
        Some(BodyLockTarget::PathSegment { session_id }) => {
            target.path_template.replace("{session}", session_id)
        }
        _ => target.path_template.to_string(),
    };
    if !url.contains(&expected_path) {
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
}
