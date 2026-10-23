//! P4 §4.4：**单会话正文**的真实取数（协议 §4.4 四步机制 · G4-19／G4-20 已坐实）。
//!
//! # 这条链走的是哪几步（**形态与目录那条链完全一样**）
//!
//! 1. **建窗链**（`session_structure::arm_capture_window`）：建一扇**自己家的**不可见
//!    窗（停在 `about:blank`）→ **按具体事件名**订阅 DevTools 的 `Network` 域事件 →
//!    **`Network.enable`** ⇒ ⇒ **三件事都在导航之前**（协议 §8.5.1 硬约束 3）。
//! 2. **导航链**（`session_structure::capture_in`）：导航到**那条会话的深链**
//!    （`https://chat.deepseek.com/a/chat/s/<sessionId>`，G4-19 已坐实）⇒ ⇒
//!    **站点自己**发 `GET /api/v0/chat/history_messages?chat_session_id=<uuid>` ⇒ ⇒
//!    **壳侧零请求**、**零自拼鉴权头**、**零读 token**、**零页外**。
//! 3. **收响应**：等到那条响应的 `Network.loadingFinished` ⇒ ⇒ **立刻**
//!    `Network.getResponseBody`（协议 §8.5.1 硬约束 1）⇒ ⇒ 壳侧**裁**成
//!    **消息（角色 ＋ 文本）** ⇒ ⇒ 站点响应体原文**不出壳**（协议 §8.5.1 末段）。
//!
//! # ⚠️ 正文那条响应**必须按 `chat_session_id` 锁定**（协议 §4.4 的硬要求）
//!
//! 一次深链导航里，站点**可能发多条**同类请求（实测：MiniMax 侧一次发了 5 条，
//! `limit`／`before` 各不相同）⇒ ⇒ **不许**拿「**第一条** history_messages」当正文
//! ⇒ ⇒ 必须按 query 里的 `chat_session_id` 值与目标会话 id **逐字比对**
//! （见 `session_structure::CaptureTarget::lock`）。
//!
//! # ⚠️ **零写入口径**（**必须如实登记**，一个字不许写成「零写入」）
//!
//! **壳侧只做导航、一个写请求都不发**（§1 裁定 1 第 1 条）；但**导航时站点自己**
//! 会跑初始化批次（含 `chat_session/create`）⇒ ⇒ 那是**站点落地即发、不是深链特有**
//! ⇒ ⇒ 三句真话：**① 壳侧零写请求 ② 站点自己跑初始化批次 ③ 不许说「零写入」**。
//!
//! ⚠️ **界面不据此写任何一句**（PM 2026-10-22 08:3x 裁：那一档已撤）—— 这一条
//! **只作如实登记**（文档／能力面），**不是**界面每次都要写一句。
//!
//! # 平台边界
//!
//! · **Windows 专用**（壳内 WebView2 DevTools 通路）：建窗、订阅、`Network.enable`、
//!   等 `loadingFinished`、取体 —— 别的平台**整块不存在** ⇒ 非 Windows 侧走
//!   `CaptureFailure::Unsupported`（协议 §6.1，明确报错，不静默降级）。
//! · **跨平台纯逻辑**：[`SessionBody::from_body`]（裁消息 ＋ 判四档）—— 不碰
//!   DevTools、不碰 webview ⇒ ⇒ 两平台口径必须一致 ⇒ ⇒ 它**必须跨平台存在**。
//!
//! ⚠️ **平台差异只能落在函数体里，不能落在签名用到的类型别名上**（`EventStream`
//! 那条别名跨平台 —— 原因见它自己的文档）。
#![cfg_attr(not(windows), allow(dead_code))]

use serde_json::{json, Value};

use super::session_structure::{
    app_handle, arm_capture_window, capture_in, profile_dir, spec_for, value_at, BodyLock,
    BodyLockTarget, BodyPick, CaptureFailure, CaptureTarget, CapturedResponse, RoleClue, SiteSpec,
    TextSource, SITE,
};
// ⚠️ 这两个类型**只在 Windows 分支里用**（冷档案那条链整块是 Windows 专用 ——
// macOS 上没有 DevTools 通路 ⇒ ⇒ 那一段不存在）⇒ ⇒ 非 Windows 上它们是**未用
// import** ⇒ ⇒ clippy `-D warnings` 会因此把 macOS 的门禁打红。
// ⇒ ⇒ 单独门起来（别塞进上面那条 use：那会把整条 import 变成 Windows 专用）。
#[cfg(windows)]
use super::session_structure::BodyProfile;
#[cfg(windows)]
use crate::cold_profile::ColdProfile;

/// 桥上那条路由的**命令名**（**白名单**：内核只许调这一个；协议 §5.2 第 3 条）。
pub(crate) const COMMAND: &str = "sessionBody";

/// 取数窗的**用途标记**（正文那条链）—— 与目录那条**各建各的窗**（label 不撞）。
const BODY_KIND: &str = "body";

/// 角色：**用户**。
const ROLE_USER: &str = "user";
/// 角色：**助手**。
const ROLE_ASSISTANT: &str = "assistant";
/// 角色：**工具调用**（ChatGPT 那种：不是「正文对话」⇒ ⇒ **记进 `omitted`**）。
const ROLE_TOOL: &str = "tool";
/// 角色：**思维链**（DeepSeek 那种 `THINK`）。
const ROLE_THINKING: &str = "thinking";

/// 省略事实里的**种类**（协议 §1.1 的 `captureResult.omitted` ＝ `[{kind, count}]`）。
const OMITTED_THINKING: &str = "thinking";
/// 省略事实里的**种类**：**工具调用**（ChatGPT 那种 `tool` 消息）。
///
/// ⚠️ **不许静默丢**（PM 2026-10-22 13:4x 裁）⇒ ⇒ `tool` 消息**不进正文**、
/// **记进 `omitted`**（`kind` 写 `tool`）⇒ ⇒ 正文**只留** `user`／`assistant`。
const OMITTED_TOOL: &str = "tool";

/// 一条正文消息（**角色 ＋ 文本** —— 协议 §1.1 的 `messages` 面）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BodyMessage {
    /// 角色（**小写枚举**：协议 §1.3）。
    pub(crate) role: String,
    /// 那条消息的文本。
    pub(crate) text: String,
}

/// 一次取回的**正文**（消息 ＋ 捕获结果 ＋ 命中判据）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SessionBody {
    /// 消息（**正文**——协议 §1.1 的 `messages` 面）。
    pub(crate) messages: Vec<BodyMessage>,
    /// 捕获结果（协议 §1.1 的 `captureResult` ＝ `{status, omitted, unsupported}`）。
    pub(crate) capture_result: Value,
    /// **命中判据**（协议 §4.4：正文那条响应**按 `chat_session_id` 锁定**；
    /// 命中多条时取**最新**的那条 ⇒ 回执里写清采用了哪一条）。
    pub(crate) pick: Value,
}

impl SessionBody {
    /// 回给内核的 JSON 形态（**只回消息 ＋ 捕获结果 ＋ 命中判据** ——
    /// 站点的响应体原文**不出壳**）。
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "messages": self
                .messages
                .iter()
                .map(|message| json!({ "role": message.role, "text": message.text }))
                .collect::<Vec<Value>>(),
            "captureResult": self.capture_result,
            "pick": self.pick,
        })
    }

    /// 从**壳侧拿到的完整响应体**裁出**消息**，并**顺手判掉四档**。
    ///
    /// ## 为什么裁剪在**壳侧**
    ///
    /// 协议 §8.5.1 末段：`Network` 域拿到的是**站点的完整响应体** ⇒ ⇒ **不许**把
    /// 原文直接回传 ⇒ ⇒ 由**壳侧**解析并裁成「角色 ＋ 文本」。
    ///
    /// ## 四档分开（协议 §6.2／§6.3）
    ///
    /// ① 传输层失败（`status != 200`）⇒ 回程失败；
    /// ② 站内码（顶层 `code != 0`，或业务层 `biz_code != 0`）⇒ **原样上报**；
    /// ③ 形状漂移（认不出消息数组／消息形态）⇒ `schema_changed`；
    /// ④ 站点返回空（一条消息都没有）⇒ 空档。
    /// ⚠️ **它是「默认站点」那个入口**（**跨平台用例按它断言协议口径** ⇒ ⇒
    /// **名字不许改**）；生产链走 [`SessionBody::from_body_for`]（**按站点**）。
    #[allow(dead_code)]
    fn from_body(body: &str, status: i64) -> Result<Self, CaptureFailure> {
        let spec = spec_for(SITE).ok_or_else(|| {
            CaptureFailure::Return(format!("站点 {SITE} 没有登记取数面（规格表里缺这一条）"))
        })?;
        Self::from_body_for(body, status, spec)
    }

    /// **按站点**裁消息（**一条链、多个站点**）。
    ///
    /// ⚠️ **DeepSeek 的口径一个字没动**：[`from_body`] 只是**用现役那个站点
    /// 的规格**调它 ⇒ ⇒ 现役行为与改之前**逐字相同**。
    ///
    /// ## 站点相关的部分**只有这些**
    ///
    /// · 站内码／站内 msg 在**哪儿**；
    /// · 消息数组在**哪儿**（DeepSeek `data.biz_data.messages`；MiniMax `messages`）；
    /// · **角色**怎么认（字段名 ＋ 取值映射）；
    /// · **正文**在哪个字段（DeepSeek `content`／`text`／`fragments`；
    ///   MiniMax `msg_content`）。
    pub(crate) fn from_body_for(
        body: &str,
        status: i64,
        spec: &SiteSpec,
    ) -> Result<Self, CaptureFailure> {
        let value: Value = serde_json::from_str(body)
            .map_err(|e| CaptureFailure::Return(format!("站点的响应体不是合法 JSON：{e}")))?;

        // ① 传输层（协议 §4.2 的成功判据第一条）。
        if status != 200 {
            return Err(CaptureFailure::Return(format!(
                "传输层状态码 {status}（协议 §4.2 的成功判据要求 200）"
            )));
        }

        // ② 站内码（**原样上报，不解释语义** —— 协议 §6.2）。
        let code = number_of(value_at(&value, spec.code_path));
        let msg = value_at(&value, spec.msg_path)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(code) = code {
            if code != 0 {
                return Err(CaptureFailure::SiteCode {
                    code: code.to_string(),
                    msg,
                });
            }
        }
        let data = value.get("data");
        let biz_code = number_of(data.and_then(|d| d.get("biz_code")));
        let biz_msg = data
            .and_then(|d| d.get("biz_msg"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(biz_code) = biz_code {
            if biz_code != 0 {
                return Err(CaptureFailure::SiteCode {
                    code: biz_code.to_string(),
                    msg: biz_msg,
                });
            }
        }

        // ③ 形状：消息数组（协议 §4.4：正文在站点自己那条响应里）。
        let Some(list) = message_array(&value, spec) else {
            log_structure_summary(&value, None);
            return Err(CaptureFailure::Schema(format!(
                "认不出站点的正文消息数组（站点 {}；位置：{}）",
                spec.key,
                spec.messages_paths
                    .iter()
                    .map(|path| path.join("."))
                    .collect::<Vec<_>>()
                    .join(" 或 ")
            )));
        };

        let mut messages: Vec<BodyMessage> = Vec::new();
        let mut thinking_count: usize = 0;
        let mut tool_count: usize = 0;
        let mut unsupported: Vec<String> = Vec::new();
        for item in list {
            let Some(message) = message_of(item, spec) else {
                log_structure_summary(&value, Some(list));
                return Err(CaptureFailure::Schema(
                    "站点的正文消息形态认不出（认不出角色）".to_string(),
                ));
            };
            // ⚠️ **思维链在独立字段里**（MiniMax 的 `thinking_content`）⇒ ⇒
            // **正文照留**（它有 `msg_content`），思维链**记进 `omitted`**
            // （协议 §1.1：不静默丢）。
            let carries_thinking = !spec.thinking_key.is_empty()
                && item
                    .get(spec.thinking_key)
                    .and_then(|v| v.as_str())
                    .map(|text| !text.trim().is_empty())
                    .unwrap_or(false);
            match message.role.as_str() {
                // ⚠️ **空内容不进正文**（协议 §1.1：**不静默丢** ⇒ ⇒ 显式记进
                // `unsupported`，而**不是**在正文里留一个**空泡**）—— 这一条
                // 与本模块既有注释（`message_of` 里那句「不进正文」）**一致**
                // （PM 2026-10-22 13:4x 裁：空内容 ⇒ 记进 `unsupported`、不进正文）。
                ROLE_USER => {
                    if !message.text.is_empty() {
                        messages.push(BodyMessage {
                            role: ROLE_USER.to_string(),
                            text: message.text,
                        });
                    }
                }
                ROLE_ASSISTANT => {
                    if !message.text.is_empty() {
                        messages.push(BodyMessage {
                            role: ROLE_ASSISTANT.to_string(),
                            text: message.text,
                        });
                    }
                }
                // ⚠️ **工具调用消息**（ChatGPT 那种 `tool`）：不是「正文对话」⇒ ⇒
                // **不静默丢**（协议 §1.1）⇒ ⇒ 记进 `omitted`（`kind` ＝ `tool`）。
                ROLE_TOOL => tool_count += 1,
                // **思维链（THINK）**：不是「正文对话」⇒ ⇒ **不静默丢**
                // （协议 §1.1：未支持内容要显式记）⇒ 记进 `omitted`。
                ROLE_THINKING => thinking_count += 1,
                _ => {
                    // **认得出的其它角色**（DeepSeek 那个 `system`）⇒ ⇒ 同样
                    // **不静默丢** ⇒ ⇒ 记进 `omitted`（**沿用既有的那一格**
                    // `thinking` —— 那是**既有行为**，这一轮**不动**它）。
                    thinking_count += 1;
                }
            }
            if carries_thinking {
                thinking_count += 1;
            }
            for kind in message.unsupported {
                if !unsupported.contains(&kind) {
                    unsupported.push(kind);
                }
            }
        }

        // ④ 空档：站点返回空（一条消息都没有）。
        if messages.is_empty() {
            log_structure_summary(&value, Some(list));
            return Err(CaptureFailure::Empty);
        }

        let mut omitted: Vec<Value> = Vec::new();
        if thinking_count > 0 {
            omitted.push(json!({ "kind": OMITTED_THINKING, "count": thinking_count }));
        }
        // ⚠️ **工具调用**（ChatGPT 那种 `tool` 消息）—— **不静默丢** ⇒ ⇒ 显式
        // 记进 `omitted`（`kind` 写 `tool`）。
        if tool_count > 0 {
            omitted.push(json!({ "kind": OMITTED_TOOL, "count": tool_count }));
        }
        // **状态**：`partial` ＝ 有内容被省略（协议 §1.1：省略事实要显式记）。
        let status = if omitted.is_empty() { "ok" } else { "partial" };
        let capture_result = json!({
            "status": status,
            "omitted": omitted,
            "unsupported": unsupported,
        });

        Ok(Self {
            messages,
            capture_result,
            pick: json!({ "matched": 1, "basis": "唯一命中（chat_session_id 逐字比对）" }),
        })
    }
}

/// 正文那条链**自己的**失败映射（**目录那条链的映射在 `session_structure.rs`**）。
///
/// ## 为什么两条链各有一份
///
/// 目录那条链的失败落在**目录**语义上（`catalog_failed`／`catalog_empty`）；
/// 正文那条链的失败落在**捕获**语义上（`timeout` 可重试、`schema_changed` 说
/// 「站点结构变了」）⇒ ⇒ 同一个失败在两条链上**该给不同的既有码**。
///
/// ⚠️ **闭集不扩**（协议 §6.1）：这里**只用** P1 §3 那 14 个码
/// （`official_chat::protocol::ERROR_CODES` 是那份闭集的真身）。
impl CaptureFailure {
    /// 这个失败在**正文那条链**（协议 §4.4）上对应**哪个既有协议码**。
    pub(crate) fn capture_code(&self) -> &'static str {
        match self {
            // 回程失败／超时 ⇒ `timeout`（**可重试**）：没取到正文，用户重试有意义。
            CaptureFailure::Return(_) | CaptureFailure::Timeout(_) => "timeout",
            // 站内码 ⇒ `catalog_failed`（**原样**的码与 msg 在 detail 里）。
            CaptureFailure::SiteCode { .. } => "catalog_failed",
            // 形状漂移 ⇒ `schema_changed`（用户下一步＝「升级插件」）。
            CaptureFailure::Schema(_) => "schema_changed",
            // 站点返回空正文 ⇒ 空档那一格（闭集里唯一的「空」）。
            CaptureFailure::Empty => "catalog_empty",
            // 本机没这条通路 ⇒ `source_unsupported`（协议 §6.1）。
            CaptureFailure::Unsupported(_) => "source_unsupported",
        }
    }

    /// 这个失败在**正文那条链**（协议 §4.4）上的**脱敏原因线索**。
    ///
    /// ⚠️ 与 `session_structure` 里那一条的区别只在**措辞**（那一条说的是「目录」，
    /// 这一条说的是「正文」）⇒ ⇒ 两条链的失败**各有各的措辞**，但**都不带异常串**。
    pub(crate) fn capture_detail(&self) -> String {
        match self {
            CaptureFailure::Return(detail) => format!("回程失败：{detail}"),
            CaptureFailure::Timeout(detail) => {
                format!("还没抓到站点的会话正文（可重试）：{detail}")
            }
            CaptureFailure::Empty => {
                // ⚠️ **不许断言「这条会话没有消息」** —— 实机读数坐实过：**站点自己
                // 说这条会话非空**（`chat_session.is_empty` ＝ false）**却给了一个
                // 空的消息数组** ⇒ ⇒ 这句话是**不实断言** ⇒ ⇒ 只说**壳侧看到的事实**。
                "站点返回了空正文（接口正常，但响应里没有消息元素）".to_string()
            }
            CaptureFailure::Schema(detail) => format!("页面结构与协议 §4.4 不符：{detail}"),
            CaptureFailure::Unsupported(detail) => format!("本机没有这条通路：{detail}"),
            // 站内码：**原样**（码 + 逐字 msg）——不解释语义（协议 §6.2）。
            CaptureFailure::SiteCode { code, msg } => match msg {
                Some(msg) if !msg.is_empty() => format!("站内码 {code} {msg}"),
                _ => format!("站内码 {code}"),
            },
        }
    }
}

/// 一条消息（**裁之前**的形态：角色线索 ＋ 文本 ＋ 未支持内容）。
struct RawMessage {
    /// 角色（**已归一**成协议的小写枚举）。
    role: String,
    /// 文本。
    text: String,
    /// 这条消息里**没被当成正文**的内容种类。
    unsupported: Vec<String>,
}

/// 从响应体里找出**消息数组**（**按站点登记的位置**，**按顺序**认）。
///
/// ## 为什么是「按登记过的位置找」而不是「猜第一个数组」
///
/// 协议 §2.3 的纪律是「**认不出就报错，不猜**」⇒ ⇒ 这里**只认这个站点
/// 登记过的位置**（DeepSeek ＝ `data.biz_data.messages` 与它的换名
/// `chat_messages`；MiniMax ＝ 顶层 `messages`）⇒ ⇒ 认不出 ⇒ `schema_changed`
/// （**不假装有正文**）。
fn message_array<'a>(value: &'a Value, spec: &SiteSpec) -> Option<&'a Vec<Value>> {
    for path in spec.messages_paths {
        if let Some(list) = value_at(value, path).and_then(Value::as_array) {
            return Some(list);
        }
    }
    None
}

/// 裁一条消息（**角色 ＋ 文本**）—— **按站点的线索**认。
///
/// ## 角色的来源（**按站点登记的线索**，**按顺序**认）
///
/// · DeepSeek ＝ `role`（`USER`／`ASSISTANT`／`SYSTEM`）**或** `type`
///   （`REQUEST`／`RESPONSE`／`THINK`）—— 两条线索**都给**时以 `role` 为准；
/// · MiniMax ＝ `role`（实测取值只有 `user`／`assistant`）；
/// · ChatGPT ＝ **`author.role`**（**两层** ⇒ 线索那一栏是**路径**）。
fn message_of(item: &Value, spec: &SiteSpec) -> Option<RawMessage> {
    let role = role_of(item, spec.role_clues)?;
    let text = text_of(item, spec.text_sources);
    let mut unsupported: Vec<String> = Vec::new();
    if text.is_empty() {
        // **空文本**＝ 这条消息的内容形态认不出（附件／图片／纯思维链……）
        // ⇒ ⇒ **显式记成未支持内容**（协议 §1.1：不静默丢），不进正文。
        unsupported.push("empty-content".to_string());
    }
    Some(RawMessage {
        role,
        text,
        unsupported,
    })
}

/// 取一条消息的角色（**归一成协议的小写枚举**）。
///
/// ⚠️ **思维链（`THINK`）不是正文角色** ⇒ ⇒ 归成 `thinking`（**不是**
/// user/assistant）⇒ 上层据此把它记进 `omitted`（**不静默丢**）。
///
/// ⚠️ **线索那一栏是**路径**（ChatGPT 那个是 `author.role` —— **两层**）⇒ ⇒
/// 认不出的取值 ⇒ **继续认下一条线索**（**不猜**）。
fn role_of(item: &Value, clues: &[RoleClue]) -> Option<String> {
    for clue in clues {
        let Some(raw) = value_at(item, clue.key).and_then(Value::as_str) else {
            continue;
        };
        let upper = raw.trim().to_ascii_uppercase();
        for (site_value, protocol_role) in clue.values {
            if *site_value == upper {
                return Some((*protocol_role).to_string());
            }
        }
    }
    None
}

/// 取一条消息的文本（**按站点登记的线索**，**按顺序**取**非空**的那个）。
///
/// DeepSeek ＝ `content` → `text` → `fragments`（数组里的内容拼接）；
/// MiniMax ＝ `msg_content`（**实测**；DeepSeek 那三个键在 MiniMax 侧**没有**）；
/// ChatGPT ＝ **`content` 对象里的 `parts[]`**（**实测**：那个 `content` **不是
/// 字符串**，是个对象）。
fn text_of(item: &Value, sources: &[TextSource]) -> String {
    let map = match item.as_object() {
        Some(map) => map,
        None => return String::new(),
    };
    for source in sources {
        match source {
            TextSource::Field(key) => {
                if let Some(text) = map.get(*key).and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        return text.to_string();
                    }
                }
            }
            TextSource::NestedField(outer, inner) => {
                // ⚠️ **形状按实测**（2026-10-22 13:5x · ChatGPT 运行时读数）：
                // assistant 消息的 `content` ＝ `{content_type, content}` ——
                // **没有 `parts`** ⇒ ⇒ 正文在**那个 `content` 字段**里。
                if let Some(text) = map
                    .get(*outer)
                    .and_then(|value| value.get(*inner))
                    .and_then(Value::as_str)
                {
                    if !text.trim().is_empty() {
                        return text.to_string();
                    }
                }
            }
            TextSource::Parts(key) => {
                // ⚠️ **形状按实测**（2026-10-22 13:5x · ChatGPT 运行时读数）：
                // user ／ tool 消息的 `content` ＝ `{content_type, parts}`，而
                // `parts[]` 的每一项**就是字符串**（实测长度 1）⇒ ⇒ 逐项取文本。
                let Some(parts) = map
                    .get(*key)
                    .and_then(|value| value.get("parts"))
                    .and_then(Value::as_array)
                else {
                    continue;
                };
                let mut texts: Vec<String> = Vec::new();
                for part in parts {
                    let text = match part {
                        // **逐项就是字符串**（实测形状）。
                        Value::String(text) => text.as_str(),
                        // **逐项是对象**（另一种形态）⇒ ⇒ 取那一项的 `text`。
                        Value::Object(part_map) => part_map
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        _ => "",
                    };
                    if !text.trim().is_empty() {
                        texts.push(text.to_string());
                    }
                }
                if !texts.is_empty() {
                    return texts.join("\n");
                }
            }
            TextSource::Fragments(key) => {
                let Some(fragments) = map.get(*key).and_then(Value::as_array) else {
                    continue;
                };
                let mut parts: Vec<String> = Vec::new();
                for fragment in fragments {
                    let Some(fragment_map) = fragment.as_object() else {
                        continue;
                    };
                    let kind = fragment_map
                        .get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_ascii_uppercase());
                    if matches!(kind.as_deref(), Some("THINK")) {
                        // 思维链**不是正文** ⇒ 不进正文（上层会把它记进 `omitted`）。
                        continue;
                    }
                    let text = fragment_map
                        .get("content")
                        .and_then(|v| v.as_str())
                        .or_else(|| fragment_map.get("text").and_then(|v| v.as_str()))
                        .unwrap_or_default();
                    if !text.trim().is_empty() {
                        parts.push(text.to_string());
                    }
                }
                if !parts.is_empty() {
                    return parts.join("\n");
                }
            }
        }
    }
    String::new()
}

/// 写一行**响应体结构摘要**到**壳侧日志**（izzy 2026-10-22 09:08 实机口径）。
///
/// ## 为什么要有它
///
/// 实机现象：取正文落 `catalog_empty`（HTTP 200、站内码 0、体也拿到了）⇒ ⇒
/// **分不清**是「站点真的返回空」／「数组在别的键下」／「我们认错了角色字段名」
/// ⇒ ⇒ **壳侧日志里有这一行，下次不用再单独跑探针**。
///
/// ## 三条硬边界（PM 09:3x notice 逐字）
///
/// ① **只进日志，不进 `detail`** —— `detail` 会进界面（客户端按 `detail` 分档
///    渲染）⇒ ⇒ 界面拿到的仍是那句**能行动的文案**（「站点返回了空正文」……）。
/// ② **不落原文** —— 只写**键名**、**数组长度**、以及 `role`／`type` 两个字段的
///    **取值集合** ⇒ ⇒ 正文原文、标题、凭据**一个字都不进日志**。
/// ③ **取值集合有上限**（每个值截断、集合去重）⇒ ⇒ 认不出的怪值也进不来。
fn log_structure_summary(value: &Value, list: Option<&Vec<Value>>) {
    eprintln!("dsh-xlink: {}", structure_summary(value, list));
}

/// 造那一行**结构摘要**（**纯函数** ⇒ ⇒ 可以单测它「不含正文原文」）。
///
/// 边界见 [`log_structure_summary`]（只进日志、不进 `detail`、不落原文）。
fn structure_summary(value: &Value, list: Option<&Vec<Value>>) -> String {
    let top_keys = keys_of(value.as_object());
    let data = value.get("data");
    let data_keys = keys_of(data.and_then(|d| d.as_object()));
    let biz_data = data.and_then(|d| d.get("biz_data"));
    let biz_keys = keys_of(biz_data.and_then(|b| b.as_object()));
    // **消息数组**：认出来了就报它**在哪个键下**、有多长；没认出来就报「没认出来」。
    let msgs_key = ["messages", "chat_messages"]
        .iter()
        .find(|key| {
            biz_data
                .and_then(|b| b.get(**key))
                .and_then(|v| v.as_array())
                .is_some()
        })
        .map(|key| (*key).to_string())
        .unwrap_or_else(|| "none".to_string());
    let msgs_len = list.map(|items| items.len()).unwrap_or(0);
    // **每条消息的键名集合（去重）** ＋ **`role`／`type` 的取值集合（只这两个）**。
    let mut msg_keys: Vec<String> = Vec::new();
    let mut roles: Vec<String> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    if let Some(items) = list {
        for item in items {
            let Some(map) = item.as_object() else {
                continue;
            };
            for key in map.keys() {
                if !msg_keys.contains(key) {
                    msg_keys.push(key.clone());
                }
            }
            for (field, bucket) in [("role", &mut roles), ("type", &mut types)] {
                let Some(raw) = map.get(field).and_then(|v| v.as_str()) else {
                    continue;
                };
                let value = raw.chars().take(32).collect::<String>();
                if !bucket.contains(&value) {
                    bucket.push(value);
                }
            }
        }
    }
    // ⚠️ **一行**（日志要能一眼看完）⇒ ⇒ 键名集合也截断（只留前 24 个）。
    let trim = |list: &Vec<String>| -> String {
        let head: Vec<String> = list.iter().take(24).cloned().collect();
        let tail = if list.len() > 24 { ",…" } else { "" };
        format!("[{}{}]", head.join(","), tail)
    };
    format!(
        "official-chat 正文结构摘要 top={} data={} biz_data={} msgs_key={} msgs_len={} msg_keys={} roles={} types={}",
        trim(&top_keys),
        trim(&data_keys),
        trim(&biz_keys),
        msgs_key,
        msgs_len,
        trim(&msg_keys),
        trim(&roles),
        trim(&types),
    )
}

/// 取一个对象的**键名**（**按出现顺序**，去重）。
fn keys_of(map: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
    let Some(map) = map else {
        return Vec::new();
    };
    let mut keys: Vec<String> = Vec::new();
    for key in map.keys() {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    keys
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

/// 拼那条会话的**深链**（协议 §4.4；G4-19 已坐实）。
///
/// ## 会话 id 是**壳侧从目录项拿到的**
///
/// 协议 §2.4 面二的 `id` ＝ 站点自己的 `chat_session.id` ⇒ ⇒ 与深链里那一段
/// **同一个值**（协议 §4.4 前置段：深链段 ＝ `chat_session_id` ＝ 会话 id）⇒ ⇒
/// **不必等 `deepLink` 键解禁**。
///
/// ⚠️ **只收「像会话 id」的字符**（字母／数字／-／_）：这一段要进 URL ⇒ ⇒
/// 认不出就报错（**不把来路不明的串拼进 URL**）。
fn deep_link(spec: &SiteSpec, session_id: &str) -> Result<String, CaptureFailure> {
    let id = session_id.trim();
    if id.is_empty() {
        return Err(CaptureFailure::Return("会话 id 为空".to_string()));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CaptureFailure::Return(
            "会话 id 的形态认不出（只收字母／数字／-／_）".to_string(),
        ));
    }
    // ⚠️ **深链前缀按站点取**（DeepSeek ＝ `/a/chat/s/<id>`；MiniMax ＝
    // `?id=<id>`，实测入口域名会 302 到 `agent.minimax.cn`）。
    Ok(format!("{}{id}", spec.deep_link_prefix))
}

/// 取一次**单会话正文**（协议 §4.4 的四步）—— **按站点**。
///
/// ## 这条链与目录那条链的关系
///
/// · **各自一扇窗**（label 靠 `kind` 分开）⇒ ⇒ 目录那条的窗**不被碰**；
/// · **导航只有一处实现**（`session_structure::capture_in`）⇒ ⇒ 目录那条链
///   **永不**二次导航（协议 v0.11 §8.5.2 G4-13 ② 那条禁令照旧成立）；
/// · **每次调用都建窗、跑完就收窗**（不长期占着一扇不可见窗）。
///
/// 成功返回**正文**（消息 ＋ 捕获结果）；失败返回**协议 §3 闭集内的既有码**。
pub(crate) async fn capture_for(
    site: &str,
    session_id: &str,
) -> Result<SessionBody, CaptureFailure> {
    let app = app_handle().ok_or_else(|| {
        CaptureFailure::Return("壳还没把 app 句柄交给取数模块（lib.rs 的 setup 没登记）".into())
    })?;
    let dir = profile_dir(&app)?;
    let spec = spec_for(site).ok_or_else(|| {
        CaptureFailure::Return(format!("站点 {site} 没有登记取数面（规格表里缺这一条）"))
    })?;
    // **冷档案**（协议 §4.4 的「冷」那一条）：站点那条正文端点是**增量
    // （MERGE）协议** —— 站点先问客户端「你缓存到第几版了」，再**只回增量**
    // ⇒ ⇒ 取数窗若复用**用户自己那份档案**，客户端缓存已是最新 ⇒ ⇒ 站点回
    // **空增量**（`chat_messages` 长度 0）⇒ ⇒ 壳侧裁出 0 条 ⇒ ⇒ 界面落
    // `catalog_empty` 档（「这条会话没有内容」）⇒ ⇒ 这就是「引用带不出正文」
    // 的**根因**。
    //
    // ⇒ ⇒ 取数窗改用一份「**登录态在、站点缓存不在**」的**一次性副本**
    // （源档案**只读**，副本**用完即删**；详见 [`ColdProfile`]）。
    //
    // ⚠️ **只在 Windows 上准备**（macOS 那条 DevTools 通路整块不存在 ⇒
    // 没必要白拷一份档案）⇒ ⇒ 冷档案的生命周期**覆盖整个取数过程**
    // （它活到函数返回 ⇒ 窗口关掉之后才删）。
    //
    // ⚠️ **哪一站用哪份档案是**站点相关**的**（[`BodyProfile`]）—— ChatGPT
    // 那条正文端点**不是**增量协议（读数里**没有** `cache_control`／
    // `cache_version`）⇒ ⇒ 冷档案对它**没有理由** ⇒ ⇒ **生产档案**
    // （PM 2026-10-22 13:4x 裁）。
    #[cfg(windows)]
    let cold = match spec.body_profile {
        BodyProfile::Cold => Some(ColdProfile::prepare(&dir).map_err(CaptureFailure::Return)?),
        BodyProfile::Production => None,
    };
    let profile = {
        #[cfg(windows)]
        {
            // ⚠️ **必须是 `as_ref()`（借用），不许写回 `.map()`** ——
            // `Option::map` 会**消费**那个 `Option` ⇒ ⇒ `cold` 当场被 drop ⇒ ⇒
            // `ColdProfile::drop` 立刻把**副本目录删掉** ⇒ ⇒ 副本在**建窗之前**
            // 就没了 ⇒ ⇒ 取数窗拿到一个空档案 ⇒ ⇒ 正文读不出来。
            // 借用的写法让 `cold` **活到函数返回**（与上面那段注释写的一致）⇒ ⇒
            // 局部变量按**逆声明顺序** drop ⇒ ⇒ `window` 先关 → `profile` 释放 →
            // **`cold` 最后 drop**（此时窗口已关，副本才删得掉）。
            cold.as_ref()
                .map(|cold| cold.dir.clone())
                .unwrap_or_else(|| dir.clone())
        }
        #[cfg(not(windows))]
        {
            dir.clone()
        }
    };
    let url = deep_link(spec, session_id)?;
    // **建窗链**：建窗 → 订阅 → `Network.enable`（**全在导航之前**）。
    let (window, events) = arm_capture_window(&app, site, BODY_KIND, &profile).await?;
    let session_id = session_id.trim().to_string();
    let target = CaptureTarget {
        url,
        path_template: spec.body_path_template,
        // ⚠️ **锁定那条响应**（协议 §4.4 的硬要求）：**按站点**逐字比对 ——
        // DeepSeek ＝ query 里的 `chat_session_id`；MiniMax ＝ **路径里**那一段
        // （实测：会话 id **在路径里**，query 里**没有**那个参数 ⇒ ⇒ **不许猜**）。
        lock: Some(match spec.body_lock {
            BodyLock::QueryKey(key) => BodyLockTarget::QueryKey {
                key,
                session_id: session_id.clone(),
            },
            BodyLock::PathSegment => BodyLockTarget::PathSegment {
                session_id: session_id.clone(),
            },
        }),
        pick: spec.body_pick,
        // **页内路线 B 的站点适配**（macOS 用；Windows 那条链不读它）。
        page_route: spec.page_route,
        // ⚠️ **正文那条的页内 URL**（**已把会话 id 填进去**）：模板在规格表里
        // （`PageRoute::body_url`）⇒ ⇒ **会话 id 只有这里知道** ⇒ 由这里填好。
        //
        // ⚠️ **该站没有正文适配时留 `None`** ⇒ macOS 上正文那条**明确报不支持**
        // （**绝不**拿目录那条的 URL 去套正文——那会静默返回一份目录）。
        page_body_url: spec
            .page_route
            .as_ref()
            .and_then(|route| route.body_url)
            .map(|template| template.replace("{session}", &session_id)),
    };
    let outcome = capture_in(&window, events, &target).await;
    // 抓完就收掉**自己**建的窗（**绝不**碰用户挂载的页签）。
    let _ = window.close();
    let captured = outcome?;
    let mut body = SessionBody::from_body_for(&captured.body, captured.status, spec)?;
    // **命中判据**（协议 §4.4）：命中几条、按哪一条取的 ⇒ ⇒ 回执里写清。
    body.pick = pick_reading(spec, &captured);
    Ok(body)
}

/// 正文那条的**命中判据**（协议 §4.4：命中多条时按这个数说明判据）。
///
/// ⚠️ **按站点说**（读数说话，不猜）：
/// · DeepSeek ＝ 那条链只命中一条 ⇒ 「唯一命中（逐字比对）」；
/// · MiniMax ＝ 一次深链发 **5～6 条** ⇒ ⇒ 「命中 N 条，其中 M 条满足
///   「`before` 缺席」那条判据 ⇒ ⇒ 取的是**从最新端**取的那条」；
/// · ChatGPT ＝ 同一条端点**发了两次** ⇒ ⇒ 「命中 N 条 ⇒ 取 `num_turns`
///   **更大**的那条（相同则取 `timestamp` 最大的那条）」。
///
/// ## ⚠️ **「取全没取全」**
///
/// 站点响应里**自带**那个布尔时（MiniMax 的 `has_more`）⇒ ⇒ 据实登记
/// `complete`；**站点没给**时（DeepSeek ／ ChatGPT）⇒ ⇒ **不写那个键**
/// （**判不出就不许说** —— 协议 §9 G4-23）。
fn pick_reading(spec: &SiteSpec, captured: &CapturedResponse) -> Value {
    let basis = match spec.body_pick {
        BodyPick::Latest => {
            if captured.matched > 1 {
                "命中多条，取 Network.responseReceived.timestamp 最大的一条".to_string()
            } else {
                "唯一命中（按路径与锁定条件逐字比对）".to_string()
            }
        }
        BodyPick::WithoutQueryKey(key) => {
            if captured.rule_matched == 0 {
                format!(
                    "命中 {} 条，但没有任何一条满足「{key} 缺席」那条判据 ⇒ 退回取最新那条",
                    captured.matched
                )
            } else {
                format!(
                    "命中 {} 条，其中 {} 条满足「{key} 缺席」⇒ 取的是站点从最新端取的那条（其余几条带 {key} ⇒ 是更早的页）",
                    captured.matched, captured.rule_matched
                )
            }
        }
        // ⚠️ **值判据**（ChatGPT）：那条判据的依据是**每个候选的那个键的值**
        // ⇒ ⇒ 上面那段读数依据（`rule_detail`）里**逐字**列出来。
        BodyPick::LargestQueryValue(_) => captured.rule_detail.clone().unwrap_or_else(|| {
            format!(
                "命中 {} 条（值判据没成立 ⇒ 退回取最新那条）",
                captured.matched
            )
        }),
        // ⚠️ **「只取」那条**（ChatGPT 的目录那条）：依据是**每个候选带没带那个键**
        // ⇒ ⇒ 同样由 `rule_detail` 逐字列出来。
        BodyPick::MustNotHaveQueryKey(_) => captured
            .rule_detail
            .clone()
            .unwrap_or_else(|| format!("命中 {} 条（那条判据没成立）", captured.matched)),
    };
    let mut pick = json!({
        "matched": captured.matched,
        "timestamp": captured.timestamp,
        "basis": basis,
    });
    // ⚠️ **「取全没取全」**（协议 §9 G4-23）：**判得出才登记**。
    let parsed: Value = serde_json::from_str(&captured.body).unwrap_or(Value::Null);
    if let Some(has_more) = value_at(&parsed, spec.body_has_more_path).and_then(Value::as_bool) {
        pick["complete"] = json!(!has_more);
        pick["hasMore"] = json!(has_more);
    }
    pick
}

/// 桥上那条路由的**请求体上限**（字节）。
///
/// **写死在这里**（协议 §5.2 第 1 条：必须读体，**并写死 body 上限**）——
/// 请求体只有一个命令名 ＋ 一个会话 id（约 70 字节）⇒ 1 KB 绰绰有余；
/// 上限存在的意义是**对端不能靠声明一个巨大的 `content-length` 把内存吃光**。
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 1024;

/// 处理桥上那条路由的**请求体** ⇒ 回**回程载荷**。
///
/// ## 能力面（协议 §5.2 第 3 条）
///
/// **只收三样**：一个**白名单里的命令名** ＋ 一个**会话 id** ＋ 一个**站点短码**。
/// **不接受任意 JS**、**不接受路径**、**不接受别的参数** ⇒ ⇒ 这条路由没有
/// 「把内核传来的文本当脚本执行」的入口。
///
/// ⚠️ **站点短码**（PM 2026-10-22 13:4x 裁）＝ **桥上那条路由的请求体加一个
/// `site` 键**（与回包信封上那个**同形**）⇒ ⇒ 壳侧据此选**哪一份规格**
/// （`spec_for`）⇒ ⇒ **缺省**是默认那个站点（DeepSeek）——**一个字都不改**
/// 既有调用方。
///
/// ## 失败一律**闭集内的既有码**
///
/// 请求体不合法 ⇒ `catalog_failed`（可重试）；取数失败 ⇒ 那一类失败各自的
/// **既有码**（见 `CaptureFailure::capture_code`）⇒ ⇒ **不新增第 15 个码**。
pub(crate) fn handle_request(body: &[u8]) -> Value {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => {
            return error_payload("catalog_failed", format!("请求体不是合法 JSON：{error}"))
        }
    };
    let command = parsed
        .get("command")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if command != COMMAND {
        return error_payload(
            "catalog_failed",
            format!("未知命令 {command}（这条路由只收 {COMMAND}）"),
        );
    }
    let session_id = parsed
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if session_id.is_empty() {
        return error_payload("catalog_failed", "请求体里没有会话 id".to_string());
    }
    // ⚠️ **站点短码**（认不出 ⇒ **明确报错**，**不回落**成某个站点）。
    let site = match super::session_structure::site_of(&parsed) {
        Ok(site) => site,
        Err(detail) => return error_payload("catalog_failed", detail),
    };
    // 取数是**慢的**（要等站点自己那条响应加载完）⇒ **同步等**（与目录那条同口径：
    // 调用方要拿到真实结果，不是一个「已受理」）。
    let outcome = tauri::async_runtime::block_on(capture_for(&site, &session_id));
    match outcome {
        Ok(body) => {
            let mut payload = body.to_json();
            // **信封上带站点短码**（与目录那条同形）：插件那半边够不着站点 ⇒ ⇒
            // 「我是谁」只有建窗的这侧知道。
            payload["site"] = json!(site);
            payload["ok"] = json!(true);
            payload
        }
        Err(failure) => error_payload(failure.capture_code(), failure.capture_detail()),
    }
}

/// 造一个**失败**回包（形状与目录那条路由**同形**）。
fn error_payload(code: &str, detail: String) -> Value {
    json!({
        "ok": false,
        "error": { "code": code, "detail": detail }
    })
}

#[cfg(test)]
mod tests {
    use super::super::session_structure::{CHATGPT_SITE, MINIMAX_SITE};
    use super::*;

    /// 造一份**成功档**的正文响应体（协议 §4.4 的消息形态）。
    fn ok_body() -> String {
        json!({
            "code": 0,
            "msg": "",
            "data": {
                "biz_code": 0,
                "biz_msg": "",
                "biz_data": {
                    "messages": [
                        { "role": "USER", "type": "REQUEST", "content": "问一句" },
                        { "role": "ASSISTANT", "type": "RESPONSE", "content": "答一句" }
                    ]
                }
            }
        })
        .to_string()
    }

    #[test]
    fn messages_are_trimmed_to_role_and_text() {
        let body = SessionBody::from_body(&ok_body(), 200).expect("成功档必须通过");
        assert_eq!(body.messages.len(), 2);
        assert_eq!(body.messages[0].role, ROLE_USER);
        assert_eq!(body.messages[0].text, "问一句");
        assert_eq!(body.messages[1].role, ROLE_ASSISTANT);
        assert_eq!(body.messages[1].text, "答一句");
        assert_eq!(
            body.capture_result.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
    }

    #[test]
    fn thinking_chain_is_omitted_explicitly_not_silently_dropped() {
        // 协议 §1.1：未支持内容**要显式记**（不静默丢）⇒ ⇒ 思维链进 `omitted`。
        let raw = json!({
            "code": 0,
            "data": {
                "biz_code": 0,
                "biz_data": {
                    "messages": [
                        { "type": "THINK", "content": "想了一遍" },
                        { "role": "USER", "content": "问一句" }
                    ]
                }
            }
        })
        .to_string();
        let body = SessionBody::from_body(&raw, 200).expect("成功档必须通过");
        assert_eq!(body.messages.len(), 1, "思维链不进正文");
        assert_eq!(
            body.capture_result.get("status").and_then(|v| v.as_str()),
            Some("partial"),
            "有省略事实 ⇒ 状态是 partial"
        );
        let omitted = body
            .capture_result
            .get("omitted")
            .and_then(|v| v.as_array())
            .expect("省略事实是数组");
        assert_eq!(omitted.len(), 1);
        assert_eq!(
            omitted[0].get("kind").and_then(|v| v.as_str()),
            Some(OMITTED_THINKING)
        );
        assert_eq!(omitted[0].get("count").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn shape_drift_is_schema_changed() {
        let raw = json!({ "code": 0, "data": { "biz_code": 0, "biz_data": { "other": [] } } })
            .to_string();
        let failure = SessionBody::from_body(&raw, 200).expect_err("形状漂移必须失败");
        assert_eq!(failure.capture_code(), "schema_changed");
    }

    #[test]
    fn site_code_is_reported_verbatim() {
        let raw = json!({ "code": 40003, "msg": "INVALID_TOKEN", "data": null }).to_string();
        let failure = SessionBody::from_body(&raw, 200).expect_err("站内码必须失败");
        assert!(failure.detail().contains("40003"), "{}", failure.detail());
        assert!(
            failure.detail().contains("INVALID_TOKEN"),
            "{}",
            failure.detail()
        );
    }

    /// 造一份 **MiniMax 形态**的正文响应体（**按 2026-10-22 13:0x 的实测形状**；
    /// **合成数据**，**不是**真响应体 ⇒ ⇒ 仓库里不落任何真实响应体）。
    fn minimax_body() -> String {
        json!({
            "base_resp": { "status_code": 0, "status_msg": "success" },
            "has_more": true,
            "last_msg_id": "msg-1",
            "next_cursor": "cursor-1",
            "messages": [
                { "msg_id": "m1", "role": "user", "msg_content": "问的那句", "timestamp": 1 },
                { "msg_id": "m2", "role": "assistant", "msg_content": "答的那句", "timestamp": 2 },
                {
                    "msg_id": "m3", "role": "assistant", "msg_content": "带思维链的答",
                    "thinking_content": "想的那段", "timestamp": 3
                }
            ],
            "query_collapse_views": []
        })
        .to_string()
    }

    #[test]
    fn minimax_messages_are_cut_from_msg_content() {
        let spec = spec_for(MINIMAX_SITE).expect("规格表里必须有 MiniMax 那一条");
        let body = SessionBody::from_body_for(&minimax_body(), 200, spec).expect("成功档必须通过");
        // ⚠️ **角色取值** ＝ 实测那套（`user` ／ `assistant`）。
        assert_eq!(body.messages.len(), 3, "三条都在（思维链**不静默丢**）");
        assert_eq!(body.messages[0].role, "user");
        assert_eq!(body.messages[0].text, "问的那句");
        assert_eq!(body.messages[1].role, "assistant");
        assert_eq!(body.messages[1].text, "答的那句");
        // ⚠️ **思维链在独立字段** ⇒ ⇒ **正文照留**，思维链**记进 `omitted`**。
        assert_eq!(body.messages[2].text, "带思维链的答");
        assert_eq!(
            body.capture_result["status"], "partial",
            "有内容被省略 ⇒ 状态是 partial"
        );
        assert_eq!(body.capture_result["omitted"][0]["kind"], "thinking");
    }

    #[test]
    fn minimax_site_code_is_read_from_base_resp() {
        let spec = spec_for(MINIMAX_SITE).expect("规格表里必须有 MiniMax 那一条");
        let raw = json!({
            "base_resp": { "status_code": 40003, "status_msg": "INVALID_TOKEN" },
            "messages": []
        })
        .to_string();
        let failure = SessionBody::from_body_for(&raw, 200, spec).expect_err("站内码必须失败");
        assert_eq!(failure.capture_code(), "catalog_failed");
        assert!(failure.detail().contains("40003"), "{}", failure.detail());
    }

    /// 造一份 **ChatGPT 形态**的正文响应体（**按 2026-10-22 13:5x 的实测形状**；
    /// **合成数据**，**不是**真响应体 ⇒ ⇒ 仓库里不落任何真实响应体）。
    ///
    /// ⚠️ 形状要点：顶层**没有 `mapping`**（正文在 **`$.messages`** 数组里）、
    /// 角色在 **`author.role`**（**两层**）、**user ／ tool** 的 `content` ＝
    /// `{content_type, parts}`（`parts[]` 每一项**是字符串**）、**assistant** 的
    /// `content` ＝ `{content_type, content}`（**没有 `parts`**）。
    fn chatgpt_body() -> String {
        json!({
            "conversation_id": "cg-1",
            "title": "一条会话",
            "current_node": "node-9",
            "default_model_slug": "gpt-5",
            "messages": [
                {
                    "id": "m1",
                    "author": { "role": "user" },
                    "content": { "content_type": "text", "parts": ["问的那句"] },
                    "create_time": 1_792_554_685.596075,
                    "status": "finished_successfully"
                },
                {
                    "id": "m2",
                    "author": { "role": "assistant" },
                    "content": { "content_type": "text", "content": "答的那句" },
                    "create_time": 1_792_555_244.256764,
                    "status": "finished_successfully"
                },
                {
                    "id": "m3",
                    "author": { "role": "tool" },
                    "content": { "content_type": "text", "parts": ["工具的输出"] },
                    "create_time": 1_792_555_250.0,
                    "status": "finished_successfully"
                },
                {
                    "id": "m4",
                    "author": { "role": "assistant" },
                    "content": { "content_type": "text", "content": "" },
                    "create_time": 1_792_555_260.0,
                    "status": "finished_successfully"
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn chatgpt_messages_are_cut_from_content_and_tool_ones_are_omitted() {
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        let body = SessionBody::from_body_for(&chatgpt_body(), 200, spec).expect("成功档必须通过");
        // ⚠️ **正文只留 `user` ／ `assistant`**（`tool` 消息**不静默丢** ⇒ 记进
        // `omitted`）。
        assert_eq!(
            body.messages.len(),
            2,
            "tool 消息不进正文、空内容也不进正文"
        );
        assert_eq!(body.messages[0].role, "user");
        assert_eq!(
            body.messages[0].text, "问的那句",
            "user 形态：content.parts[]"
        );
        assert_eq!(body.messages[1].role, "assistant");
        assert_eq!(
            body.messages[1].text, "答的那句",
            "assistant 形态：content.content"
        );
        // ⚠️ **`tool` 消息记进 `omitted`**（`kind` 写 `tool`）。
        assert_eq!(body.capture_result["status"], "partial");
        let omitted = body.capture_result["omitted"]
            .as_array()
            .expect("omitted 必须是数组");
        let tool = omitted
            .iter()
            .find(|entry| entry["kind"] == "tool")
            .expect("tool 消息必须记进 omitted");
        assert_eq!(tool["count"], 1, "一条 tool 消息");
        // ⚠️ **空内容**记进 `unsupported`（**不进正文**）—— 与 DeepSeek 那边同处理。
        let unsupported = body.capture_result["unsupported"]
            .as_array()
            .expect("unsupported 必须是数组");
        assert!(
            unsupported.iter().any(|entry| entry == "empty-content"),
            "空内容必须显式记成未支持内容：{unsupported:?}"
        );
    }

    #[test]
    fn chatgpt_has_no_site_code_and_no_has_more() {
        let spec = spec_for(CHATGPT_SITE).expect("规格表里必须有 ChatGPT 那一条");
        // ⚠️ **站内码**：ChatGPT 那条响应**没有**站内码 ⇒ ⇒ **不编一个**出来。
        let body = SessionBody::from_body_for(&chatgpt_body(), 200, spec).expect("成功档必须通过");
        assert_eq!(body.capture_result["status"], "partial");
        // ⚠️ **「取全没取全」**：站点**没给**那个布尔 ⇒ ⇒ `pick` 里**不写**
        // `complete`（**判不出就不许说**，协议 §9 G4-23）。
        let pick = &body.pick;
        assert!(pick.get("complete").is_none(), "判不出 ⇒ 不许写 complete");
    }

    #[test]
    fn every_failure_code_is_inside_the_closed_set() {
        let codes = [
            CaptureFailure::Return("x".into()).capture_code(),
            CaptureFailure::Timeout("x".into()).capture_code(),
            CaptureFailure::Empty.capture_code(),
            CaptureFailure::Schema("x".into()).capture_code(),
            CaptureFailure::Unsupported("x".into()).capture_code(),
            CaptureFailure::SiteCode {
                code: "40003".into(),
                msg: None,
            }
            .capture_code(),
        ];
        for code in codes {
            assert!(
                crate::official_chat::protocol::ERROR_CODES.contains(&code),
                "{code} 不在协议闭集里"
            );
        }
    }

    #[test]
    fn deep_link_is_built_from_the_session_id() {
        let spec = spec_for(SITE).expect("规格表里必须有现役那个站点");
        assert_eq!(
            deep_link(spec, "a19c1570-5ce6-4c6a-ba25-af38af0da4e5").expect("合法 id"),
            format!(
                "{}a19c1570-5ce6-4c6a-ba25-af38af0da4e5",
                spec.deep_link_prefix
            )
        );
        // 形态认不出 ⇒ **报错**（不把来路不明的串拼进 URL）。
        assert!(deep_link(spec, "../../evil").is_err());
        assert!(deep_link(spec, "").is_err());
    }

    #[test]
    fn the_structure_summary_carries_shape_but_never_the_body_text() {
        // **真机口径**（izzy 2026-10-22 09:08）：裁不出消息时，壳侧日志里要有一行
        // **结构摘要** ⇒ ⇒ 但**不落原文** ⇒ ⇒ 摘要里不许出现正文与标题。
        let raw = json!({
            "code": 0,
            "msg": "",
            "data": {
                "biz_code": 0,
                "biz_msg": "",
                "biz_data": {
                    "cache_control": "xxx",
                    "chat_messages": [
                        { "role": "USER", "type": "REQUEST", "content": "这是正文原文" },
                        { "role": "ASSISTANT", "type": "RESPONSE", "content": "这是回答原文" }
                    ],
                    "chat_session": { "title": "这是会话标题", "id": "x" }
                }
            }
        })
        .to_string();
        let value: Value = serde_json::from_str(&raw).expect("夹具是合法 JSON");
        let list = value
            .get("data")
            .and_then(|d| d.get("biz_data"))
            .and_then(|b| b.get("chat_messages"))
            .and_then(|v| v.as_array())
            .expect("消息数组");
        let summary = structure_summary(&value, Some(list));
        // **形状**在。
        assert!(summary.contains("top="), "{summary}");
        assert!(summary.contains("biz_data="), "{summary}");
        assert!(summary.contains("msgs_key=chat_messages"), "{summary}");
        assert!(summary.contains("msgs_len=2"), "{summary}");
        assert!(summary.contains("roles=[USER,ASSISTANT]"), "{summary}");
        assert!(summary.contains("types=[REQUEST,RESPONSE]"), "{summary}");
        assert!(
            summary.contains("msg_keys=[content,role,type]"),
            "{summary}"
        );
        // ⚠️ **原文一个字都不许在里面**。
        assert!(!summary.contains("这是正文原文"), "{summary}");
        assert!(!summary.contains("这是回答原文"), "{summary}");
        assert!(!summary.contains("这是会话标题"), "{summary}");
    }
}

/// **真机轨**（**手动跑**）：协议 §4.4 的正文那条链 ＋ 目录那条链的**条数读数**。
///
/// # 为什么它是 `#[ignore]`
///
/// 它要**真的**起一扇 WebView2 窗、**真的**登录态、**真的**站点请求 ⇒ ⇒ 单元测试
/// 跑它没有意义 ⇒ ⇒ 默认 `#[ignore]`，手动跑。
///
/// # 怎么跑
///
/// ```text
/// $env:DSH_LIVE_PROFILE = "<一次性临时目录下的 user-data 目录副本>"
/// $env:DSH_LIVE_SESSION = "<会话 id>"
/// cargo test --locked --lib official_chat::session_body::live_probe -- --ignored --nocapture
/// ```
///
/// ⚠️ **profile 必须是副本**（仓库硬不变量 `scripts/lib/probe-profile-isolation.mjs`：
/// 探针代码里不许出现产品 profile 目录名）⇒ ⇒ 这里**只从环境变量读**，源码里
/// **一个路径字面量都没有**。
///
/// # 它报什么（**只报计数与判据，不报正文原文**）
///
/// · **目录那条**（协议 §4.3）：站点响应体里 `chat_sessions` 的条数、壳侧裁出来的
///   `items` 条数、以及**递归**数出来的会话 id 总数 ⇒ ⇒ 三个数放一起 ⇒ 「条数对不上
///   是差在哪一步」一眼可见。
/// · **正文那条**（协议 §4.4）：命中几条、状态码、正文字节数、消息条数、每条的
///   角色与字符数、思维链（省略事实）条数 ⇒ ⇒ **正文原文一个字都不进读数**。
#[cfg(all(test, windows))]
mod live_probe {
    use serde_json::{json, Value};

    use super::super::session_structure::{
        arm_capture_window, capture_in, spec_for, CaptureFailure, CaptureTarget, SessionStructure,
        SITE,
    };
    use super::{deep_link, SessionBody};

    /// 登录态副本的 user-data 目录（**只从环境变量读** —— 源码里不写路径）。
    const ENV_PROFILE: &str = "DSH_LIVE_PROFILE";
    /// 要取正文的那条会话 id（**只从环境变量读**）。
    const ENV_SESSION: &str = "DSH_LIVE_SESSION";
    /// 目录那条链要等的端点（**站点自己的**会话目录端点；按路径认，query 不猜）。
    const CATALOG_PATH: &str = "/api/v0/chat_session/fetch_page";
    /// 目录那条链的导航落点（站点首页 —— 目录请求是首屏那一发）。
    const SITE_ROOT: &str = "https://chat.deepseek.com/";

    /// 递归数出响应体里**所有** `chat_sessions` 数组里的会话 id 条数。
    /// ⚠️ **这个口径对齐 G4-20 那个探针**（它是**递归**找任意 `chat_sessions`
    /// 数组）⇒ ⇒ 与「壳侧只读 `data.biz_data.chat_sessions`」的读数放在一起 ⇒ ⇒
    /// 两个数**不等**时，差的就是「壳侧读的是哪一个数组」那一步。
    fn count_all_session_ids(value: &Value) -> usize {
        match value {
            Value::Object(map) => map
                .iter()
                .map(|(key, child)| {
                    if key == "chat_sessions" {
                        child
                            .as_array()
                            .map(|list| {
                                list.iter()
                                    .filter(|item| {
                                        item.as_object()
                                            .map(|obj| obj.contains_key("id"))
                                            .unwrap_or(false)
                                    })
                                    .count()
                            })
                            .unwrap_or(0)
                    } else {
                        count_all_session_ids(child)
                    }
                })
                .sum(),
            Value::Array(list) => list.iter().map(count_all_session_ids).sum(),
            _ => 0,
        }
    }

    /// 「目录面只回最近 6 条」的**自证读数**（**只有时间戳，没有标题、没有 id**）。
    ///
    /// ⚠️ **响应体只在内存里**：这一段**不落盘**（PM 09:3x ④：那份响应体只在你
    /// 进程内存里、不许落进仓库）⇒ ⇒ 读数里只有**数字**（`updated_at`）。
    fn newest6_reading(raw: &Value, structure: &Result<SessionStructure, CaptureFailure>) -> Value {
        // **站点给的全部会话的 `updated_at`**（按数值降序）。
        let mut all: Vec<f64> = raw
            .get("data")
            .and_then(|d| d.get("biz_data"))
            .and_then(|b| b.get("chat_sessions"))
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|item| {
                        item.get("updated_at").and_then(|v| v.as_f64()).or_else(|| {
                            item.get("updated_at")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        all.sort_by(|left, right| right.total_cmp(left));
        let kept: Vec<f64> = structure
            .as_ref()
            .map(|one| {
                // ⚠️ **从生产载荷里读**（`to_json()` ⇒ ⇒ 就是**真回给内核的那一份**）
                // ⇒ ⇒ 目录项的字段是私有的（判据要裸字段名）⇒ ⇒ 这里只能走载荷。
                one.to_json()
                    .get("items")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("updated_at").and_then(|v| v.as_f64()))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let min_kept = kept.iter().copied().fold(f64::INFINITY, f64::min);
        let max_dropped = all.get(kept.len()).copied().unwrap_or(f64::NEG_INFINITY);
        json!({
            "site_total": all.len(),
            "kept_count": kept.len(),
            "kept_updated_at": kept,
            "min_kept": min_kept,
            "max_dropped": max_dropped,
            // ⚠️ **判定**：保留的确实是**最近**的（最小的保留 ≥ 最大的丢掉）。
            "is_newest": min_kept >= max_dropped,
        })
    }

    /// 把一段 JSON 描述成**键名与类型**的形状（**值一个字都不带**）。
    ///
    /// ⚠️ **为什么要它**：真机读数只许带**结构**，不许带正文原文 ⇒ ⇒ 这一层
    /// 只吐「哪个键、是什么类型、数组多长、数组首元素有哪些键」。
    fn describe(value: &Value, depth: usize) -> Value {
        if depth == 0 {
            return json!("…");
        }
        match value {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (key, child) in map {
                    out.insert(key.clone(), describe(child, depth - 1));
                }
                Value::Object(out)
            }
            Value::Array(list) => json!({
                "__len": list.len(),
                "__first": list.first().map(|one| describe(one, depth - 1)),
            }),
            Value::String(_) => json!("string"),
            Value::Number(_) => json!("number"),
            Value::Bool(_) => json!("bool"),
            Value::Null => json!("null"),
        }
    }

    /// 往**日志文件**里追加一行（**只从环境变量读路径** ⇒ 源码里不写路径）。
    ///
    /// ⚠️ **为什么要有它**：`handle.exit(0)` 走 `std::process::exit` ⇒ ⇒ 管道里的
    /// stdout 可能**一个字都看不到** ⇒ ⇒ 读数**必须落盘**才算数。
    fn note(step: &str) {
        let Ok(path) = std::env::var("DSH_LIVE_OUT") else {
            return;
        };
        let log = format!("{path}.log");
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            let _ = writeln!(file, "{step}");
            let _ = file.flush();
        }
    }

    /// 跑两条链，把读数拼成一份 JSON 文本。
    async fn run_live(app: &tauri::AppHandle, profile: &std::path::Path, session: &str) -> String {
        let mut readings = json!({});
        // ⚠️ **两阶段期间不能关窗**：tauri v2 里**最后一扇窗关掉 ⇒ 进程直接退出**
        // ⇒ ⇒ 所以关窗统一放在两阶段都跑完之后（读数写盘之后）。
        let mut keep: Vec<tauri::WebviewWindow> = Vec::new();

        // ---- ① 目录那条链（协议 §4.3）：**条数**读数 ----
        note("start:catalog");
        match arm_capture_window(app, SITE, "live-catalog", profile).await {
            Ok((window, events)) => {
                let target = CaptureTarget {
                    url: SITE_ROOT.to_string(),
                    path_template: CATALOG_PATH,
                    lock: None,
                    pick: super::super::session_structure::BodyPick::Latest,
                    page_route: None,
                    page_body_url: None,
                };
                let captured = capture_in(&window, events, &target).await;
                // ⚠️ **这里不能关窗**：tauri v2 里**最后一扇窗关掉 ⇒ 进程直接退出**
                // ⇒ ⇒ 上一阶段的窗**留到两阶段都跑完**再一起关（见函数末尾）。
                keep.push(window.clone());
                match captured {
                    Ok(captured) => {
                        let raw: Value =
                            serde_json::from_str(&captured.body).unwrap_or(Value::Null);
                        let structure =
                            SessionStructure::from_body(&captured.body, captured.status);
                        // **四个读数分列**（哪一步少了几条，一眼可见）。
                        let (sessions_len, items_len, has_more, error) = match &structure {
                            Ok(one) => (one.sessions_length, one.items.len(), one.has_more, None),
                            Err(failure) => (None, 0, None, Some(failure.capture_detail())),
                        };
                        readings["catalog"] = json!({
                            "status": captured.status,
                            "body_bytes": captured.body.len(),
                            "matched": captured.matched,
                            "chat_sessions_len": sessions_len,
                            "items_len": items_len,
                            "all_session_ids_recursive": count_all_session_ids(&raw),
                            "has_more": has_more,
                            "error": error,
                            // ⚠️ **「最近 6 条」的自证读数**（izzy 12:4x 那条裁）：
                            // **保留的 6 条的 `updated_at`** ＋ **被丢掉那批里最新的一条**
                            // ⇒ ⇒ 前者应当**不小于**后者 ⇒ ⇒ 保留的确实是**最近**的。
                            "newest6": newest6_reading(&raw, &structure),
                        });
                    }
                    Err(failure) => {
                        readings["catalog"] = json!({
                            "error": failure.capture_detail(),
                            "code": failure.capture_code(),
                        });
                    }
                }
            }
            Err(failure) => {
                readings["catalog"] = json!({
                    "error": failure.capture_detail(),
                    "code": failure.capture_code(),
                });
            }
        }

        // ---- ② 正文那条链（协议 §4.4）：**命中判据 ＋ 条数** ----
        note("catalog-done:start:body");
        match arm_capture_window(app, SITE, "live-body", profile).await {
            Ok((window, events)) => {
                let spec = spec_for(SITE).expect("规格表里必须有现役那个站点");
                let target = CaptureTarget {
                    url: deep_link(spec, session).unwrap_or_default(),
                    path_template: spec.body_path_template,
                    lock: Some(super::super::session_structure::BodyLockTarget::QueryKey {
                        key: "chat_session_id",
                        session_id: session.to_string(),
                    }),
                    pick: spec.body_pick,
                    page_route: spec.page_route,
                    page_body_url: spec
                        .page_route
                        .as_ref()
                        .and_then(|route| route.body_url)
                        .map(|template| template.replace("{session}", session)),
                };
                let captured = capture_in(&window, events, &target).await;
                // 同上：**关窗统一放到两阶段之后**（最后一扇窗关掉 ⇒ 进程退出）。
                keep.push(window.clone());
                match captured {
                    Ok(captured) => {
                        // ⚠️ **响应体形状**（只吐键名与类型，**值一个字都不带**）——
                        // 解析器认不出时，这是唯一能据以订正的依据。
                        let raw: Value =
                            serde_json::from_str(&captured.body).unwrap_or(Value::Null);
                        let parsed = SessionBody::from_body(&captured.body, captured.status);
                        match parsed {
                            Ok(body) => {
                                readings["body"] = json!({
                                    "status": captured.status,
                                    "matched": captured.matched,
                                    "timestamp": captured.timestamp,
                                    "body_bytes": captured.body.len(),
                                    "message_count": body.messages.len(),
                                    "roles": body
                                        .messages
                                        .iter()
                                        .map(|m| m.role.clone())
                                        .collect::<Vec<String>>(),
                                    "text_chars": body
                                        .messages
                                        .iter()
                                        .map(|m| m.text.chars().count())
                                        .collect::<Vec<usize>>(),
                                    "capture_result": body.capture_result,
                                });
                            }
                            Err(failure) => {
                                readings["body"] = json!({
                                    "status": captured.status,
                                    "matched": captured.matched,
                                    "body_bytes": captured.body.len(),
                                    "code": failure.capture_code(),
                                    "error": failure.capture_detail(),
                                    "shape": describe(&raw, 5),
                                });
                            }
                        }
                    }
                    Err(failure) => {
                        readings["body"] = json!({
                            "code": failure.capture_code(),
                            "error": failure.capture_detail(),
                        });
                    }
                }
            }
            Err(failure) => {
                readings["body"] = json!({
                    "code": failure.capture_code(),
                    "error": failure.capture_detail(),
                });
            }
        }

        note("body-done");
        let report = serde_json::to_string_pretty(&readings).unwrap_or_else(|_| "{}".to_string());
        // 两阶段都跑完了 ⇒ ⇒ 这时候关窗（关最后一扇 ⇒ 进程退出 ⇒ 但读数已写盘）。
        for window in &keep {
            let _ = window.close();
        }
        report
    }

    #[test]
    #[ignore = "真机轨：需要登录态 profile 副本（手动跑）"]
    fn live_readings() {
        let profile = std::env::var(ENV_PROFILE).expect(
            "必须给 DSH_LIVE_PROFILE=<一次性临时目录下的 user-data 目录副本>（不给就不跑）",
        );
        let session = std::env::var(ENV_SESSION).expect("必须给 DSH_LIVE_SESSION=<会话 id>");
        let profile = std::path::PathBuf::from(profile);

        let app = tauri::Builder::default()
            // ⚠️ **`any_thread()`**：tao 的事件循环**默认只能在主线程建** ⇒ ⇒ 而
            // libtest 把每个用例放在**工作线程**上跑 ⇒ ⇒ 不开这一条 ⇒ 事件循环
            // 在 `main` 之前就 panic（`Initializing the event loop outside of the
            // main thread`）⇒ ⇒ 这条是**让真机轨能在测试里跑**的唯一开关。
            .any_thread()
            .setup(move |app| {
                let handle = app.handle().clone();
                let profile = profile.clone();
                let session = session.clone();
                std::thread::Builder::new()
                    .name("official-chat-live-probe".into())
                    .spawn(move || {
                        note("thread-started");
                        let report =
                            tauri::async_runtime::block_on(run_live(&handle, &profile, &session));
                        println!("LIVE_READINGS_BEGIN");
                        println!("{report}");
                        println!("LIVE_READINGS_END");
                        // ⚠️ **读数必须落盘**（`handle.exit(0)` 走
                        // `std::process::exit` ⇒ ⇒ 管道里的 stdout **不会**被
                        // 冲出去 ⇒ ⇒ 不落盘的话**读数一个字都看不到**）。
                        if let Ok(path) = std::env::var("DSH_LIVE_OUT") {
                            let _ = std::fs::write(&path, format!("{report}\n"));
                            note("readings-written");
                        }
                        {
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                            let _ = std::io::stderr().flush();
                        }
                        handle.exit(0);
                    })
                    .map_err(|e| e.to_string())?;
                Ok(())
            })
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("真机轨：建不起 mock app");
        app.run(|_app, _event| {});
    }
}
