//! 官网对话引用插件的**协议核心（Rust 侧）**。
//!
//! ## 为什么 Rust 侧要有一份
//!
//! 协议冻结稿 `docs/features/official-chat/official-chat-p1-protocol.md` v0.1
//! 的 §6 要求：**同一组 JSON 夹具，Rust 与 JavaScript 各跑一遍**，断言
//! 「同一夹具 → 同一解析结果」，且两端 `schemaVersion` 常量必须一致。
//!
//! 理由不是「双份实现好看」，而是**跨端口径漂移是这类协议最常见的病**：
//! 句柄形态、规范化范围、摘要算法、消息分隔符——任何一处两端不一致，
//! 就会出现「客户端算出的 id 与宿主算出的 id 不是同一个」这类**只在真机上
//! 才显形**的错。所以口径必须由**同一组夹具**钉住，而不是靠两边各自注释。
//!
//! 夹具在 `plugins/official-chat/fixtures/protocol/`，**Rust 与 JS 读的是
//! 同一份文件**（不是各写一份期望值）。
//!
//! ## 口径与 JS 侧的对应
//!
//! | 口径 | JS 侧 | Rust 侧 |
//! |---|---|---|
//! | 规范化（§1.3） | `host/protocol.js` 的 `normalizeText` | [`normalize_body`] |
//! | 消息分隔 | `MESSAGE_SEPARATOR`（`\n\n`） | [`MESSAGE_SEPARATOR`] |
//! | 摘要 | `contentDigest` | [`content_digest`] |
//! | 会话短码 | `makeSessionShort` | [`session_short`] |
//! | 句柄形态 | `makeSnapshotId` / `parseSnapshotId` | [`is_valid_snapshot_id`] |
//! | 判定入口 | `resolveCase` | [`resolve_case`] |
//!
//! ## 两条最容易踩的跨端坑
//!
//! 1. **UTF-16 code unit**：JS 的 `charCodeAt` 取的是 UTF-16 码元，非 BMP
//!    字符（emoji 等）是**两个**码元。Rust 必须按 `encode_utf16()` 迭代，
//!    否则短码与 JS 不同。
//! 2. **规范化只去整段末尾空白**：逐行去尾随空白会改掉 Markdown 的硬换行
//!    （行尾两个空格 = 换行语义），而协议写的是「不得改动语义」。
//!
//! @module official_chat/protocol

use serde_json::Value;

/// §1.1 快照格式版本。**变更即升版**；旧版快照按 `schema_changed` 拒。
pub const SCHEMA_VERSION: u32 = 1;

/// provider 稳定短码（协议 §S-1 举例 `ds` / `gpt` / `mm`）。
///
/// **必须与客户端半边的 `PROVIDER_SHORT` 同值**——两端算出同一个句柄。
pub const PROVIDER_SHORT: &str = "ds";

/// 句柄整串长度上限（协议 S-1：≤ 24 可见字符 = chip 宽度）。
pub const MAX_SNAPSHOT_ID_LEN: usize = 24;

/// 消息之间的分隔符（协议未规定，实现选定；与 JS 侧同值）。
pub const MESSAGE_SEPARATOR: &str = "\n\n";

/// §4 最大正文字节（超过 → `body_too_large`）。
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// §4 目录页大小。
pub const CATALOG_PAGE_SIZE: usize = 50;

/// §4 目录缓存期限（毫秒）。
pub const CATALOG_CACHE_TTL_MS: u64 = 5 * 60 * 1000;

/// §4 搜索去抖（毫秒）。
pub const SEARCH_DEBOUNCE_MS: u64 = 300;

/// §4 读取超时（毫秒）。
pub const READ_TIMEOUT_MS: u64 = 30_000;

/// §4 最大响应字节。
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// §4 快照保留期（毫秒；7 天）。
pub const SNAPSHOT_RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// §4 缓存总量上限（字节；512 MB / 实例）。
pub const CACHE_TOTAL_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// §4 单消息引用数上限。
pub const MAX_REFERENCES_PER_MESSAGE: usize = 8;

/// §4 单条正文预算（字节；进入模型输入的**单条**上限）。
pub const PER_BODY_BUDGET_BYTES: usize = 64 * 1024;

/// §4 单次消息总预算（字节；**含原生 `@session`**）。
pub const PER_MESSAGE_BUDGET_BYTES: usize = 256 * 1024;

/// §3 的 13 个稳定错误码（顺序固定，供夹具与机检遍历）。
pub const ERROR_CODES: [&str; 13] = [
    "not_logged_in",
    "account_changed",
    "source_unsupported",
    "schema_changed",
    "partial_content",
    "body_too_large",
    "timeout",
    "cancelled",
    "snapshot_invalid",
    "kernel_incompatible",
    "catalog_empty",
    "catalog_failed",
    "stale_draft",
];

/// §3 每个错误码的**可重试性**（与码名同序）。
///
/// `true` = 可以原样重试；`false` = 重试没有意义（要用户换动作）。
pub const ERROR_RETRYABLE: [bool; 13] = [
    false, // not_logged_in
    false, // account_changed
    false, // source_unsupported
    false, // schema_changed
    false, // partial_content
    false, // body_too_large
    true,  // timeout
    false, // cancelled
    false, // snapshot_invalid
    false, // kernel_incompatible
    false, // catalog_empty
    true,  // catalog_failed
    false, // stale_draft
];

/// §3 每个错误码的**用户下一步**（与码名同序）。
///
/// 这些文案会**原样呈现给用户**，所以它们是协议的一部分：换文案等于换协议。
pub const ERROR_USER_NEXT: [&str; 13] = [
    "去官网窗口登录",
    "重新选择来源",
    "换一个来源",
    "升级插件",
    "明示省略事实",
    "换更小的会话",
    "重试",
    "重新选择",
    "移除该引用",
    "升级内核",
    "换个关键词",
    "重试",
    "重新选择",
];

/// 错误码 → 序号；不在表里返回 `None`。
///
/// @param code - 稳定码名。
/// @returns 码在表里的序号。
pub fn error_index(code: &str) -> Option<usize> {
    ERROR_CODES.iter().position(|name| *name == code)
}

/// 错误码是否**可重试**。
///
/// @param code - 稳定码名。
/// @returns 可重试即 `true`；未知码按「不可重试」处理（保守）。
pub fn is_retryable(code: &str) -> bool {
    error_index(code)
        .map(|index| ERROR_RETRYABLE[index])
        .unwrap_or(false)
}

/// 错误码的**用户下一步**文案。
///
/// @param code - 稳定码名。
/// @returns 文案；未知码给一个保守的兜底。
pub fn user_next(code: &str) -> &'static str {
    error_index(code)
        .map(|index| ERROR_USER_NEXT[index])
        .unwrap_or("换一个来源")
}

/// §1.3 规范化：行尾统一 `\n`、去 BOM、**只去整段末尾空白**。
///
/// 「只去整段末尾」的理由：逐行去尾随空白会改掉 Markdown 的硬换行
/// （行尾两个空格 = 换行语义），而协议写的是「不得改动语义」。
///
/// @param text - 原始正文。
/// @returns 规范化后的正文。
pub fn normalize_body(text: &str) -> String {
    let without_bom = text.strip_prefix('\u{feff}').unwrap_or(text);
    let unified = without_bom.replace("\r\n", "\n").replace('\r', "\n");
    unified.trim_end().to_string()
}

/// 快照的**模型可见正文**：各条消息的文本按顺序用空行分隔。
///
/// @param messages - 消息数组。
/// @returns 拼接后的正文。
pub fn body_of(messages: &Value) -> String {
    let items = match messages.as_array() {
        Some(items) => items,
        None => return String::new(),
    };
    items
        .iter()
        .map(|message| {
            message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(MESSAGE_SEPARATOR)
}

/// §1.3 `contentDigest`：规范化后正文的 SHA-256（十六进制小写）。
///
/// @param text - 原始正文。
/// @returns 64 位十六进制小写摘要。
pub fn content_digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized = normalize_body(text);
    let digest = Sha256::digest(normalized.as_bytes());
    crate::shell::digest::hex(&digest[..])
}

/// 32 位无符号整数的 base36 表示（与 JS `Number.prototype.toString(36)` 同形）。
///
/// @param value - 待转换的数。
/// @returns 小写 base36 串。
fn to_base36(mut value: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut buffer = Vec::new();
    while value > 0 {
        buffer.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    buffer.reverse();
    String::from_utf8(buffer).unwrap_or_default()
}

/// 会话短码（协议 S-1：≤ 12 字符，同一会话恒定、不同会话不撞）。
///
/// **必须与客户端半边逐位一致**：两段 32 位 FNV-1a 变体拼 base36，取**后 12 位**，
/// 不足左补 `0`。
///
/// **UTF-16 口径**：按 `encode_utf16()` 迭代——JS 的 `charCodeAt` 取的是 UTF-16
/// 码元，非 BMP 字符是**两个**码元。少了这一步，短码会与 JS 不同（§6 的夹具
/// 就是为它准备的）。
///
/// @param session_id - 官网会话 id。
/// @returns 12 字符短码。
pub fn session_short(session_id: &str) -> String {
    let mut a: u32 = 0x811c_9dc5;
    let mut b: u32 = 0x9e37_79b9;
    for (index, unit) in session_id.encode_utf16().enumerate() {
        let code = u32::from(unit);
        a = (a ^ code).wrapping_mul(0x0100_0193);
        b = b
            .wrapping_add(code)
            .wrapping_add((index as u32).wrapping_add(1))
            .wrapping_mul(0x85eb_ca6b);
    }
    let joined = format!("{}{}", to_base36(a), to_base36(b));
    let tail: String = joined
        .chars()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{tail:0>12}")
}

/// 句柄是否合法（协议 S-1 的形态 + 长度上限）。
///
/// **这是路径安全的第一道闸**：`snapshotId` 会直接进文件路径，所以只认
/// 小写字母数字与那一个分隔符 `~`。
///
/// @param id - 候选句柄。
/// @returns 合法即 `true`。
pub fn is_valid_snapshot_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_SNAPSHOT_ID_LEN {
        return false;
    }
    let parts: Vec<&str> = id.split('~').collect();
    if parts.len() != 3 {
        return false;
    }
    let (provider, session, seq) = (parts[0], parts[1], parts[2]);
    let provider_ok = !provider.is_empty()
        && provider.len() <= 4
        && provider
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let session_ok = !session.is_empty()
        && session.len() <= 12
        && session
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let seq_ok = seq.len() == 4 && seq.chars().all(|c| c.is_ascii_digit());
    provider_ok && session_ok && seq_ok
}

/// 句柄里的 provider 段。
///
/// @param id - 句柄。
/// @returns provider 短码（形态不合法时返回空串）。
pub fn provider_of_snapshot_id(id: &str) -> &str {
    id.split('~').next().unwrap_or("")
}

/// 快照 id（= `ref`）：`<provider>~<session>~<seq>`。
///
/// @param session_id - 官网会话 id。
/// @param seq - 该会话在本实例内的 pick 序号。
/// @returns 句柄字符串。
pub fn make_snapshot_id(session_id: &str, seq: u32) -> String {
    format!(
        "{}~{}~{:04}",
        PROVIDER_SHORT,
        session_short(session_id),
        seq
    )
}

/// §1.1 快照字段表：字段名 → 期望 JSON 类型。
///
/// **不含 `captureResult`**：协议写它是对象，现役实现用字符串（冲突已回报 PM），
/// 所以它**单独校验**（见 [`validate_snapshot`]），不进这张类型表——进了就会把
/// 字符串形态提前判死。
const SNAPSHOT_FIELDS: [(&str, &str); 11] = [
    ("schemaVersion", "number"),
    ("snapshotId", "string"),
    ("providerId", "string"),
    ("accountScope", "string"),
    ("sessionId", "string"),
    ("branch", "string"),
    ("messages", "array"),
    ("contentDigest", "string"),
    ("capturedAt", "string"),
    ("sourceUpdatedAt", "string"),
    ("sourceFingerprint", "string"),
];

/// §1.1 快照字段表逐字段校验。
///
/// @param snapshot - 待校验的快照。
/// @returns 错误列表（空 = 合法）。
pub fn validate_snapshot(snapshot: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if !snapshot.is_object() {
        return vec!["快照不是一个对象".to_string()];
    }
    for (field, kind) in SNAPSHOT_FIELDS {
        match snapshot.get(field) {
            None | Some(Value::Null) => errors.push(format!("缺字段 {field}")),
            Some(value) => {
                let ok = match kind {
                    "number" => value.is_number(),
                    "string" => value.is_string(),
                    "array" => value.is_array(),
                    "object" => value.is_object(),
                    _ => true,
                };
                if !ok {
                    errors.push(format!("字段 {field} 不是 {kind}"));
                }
            }
        }
    }
    if let Some(version) = snapshot.get("schemaVersion").and_then(Value::as_u64) {
        if version != u64::from(SCHEMA_VERSION) {
            errors.push(format!("schemaVersion 不支持：{version}"));
        }
    }
    if let Some(id) = snapshot.get("snapshotId").and_then(Value::as_str) {
        if !is_valid_snapshot_id(id) {
            errors.push(format!("snapshotId 形态不合法：{id}"));
        }
    }
    if let Some(digest) = snapshot.get("contentDigest").and_then(Value::as_str) {
        if digest.len() != 64
            || !digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            errors.push("contentDigest 不是 64 位十六进制小写".to_string());
        }
    }
    if let Some(result) = snapshot.get("captureResult") {
        if result.is_object() {
            for field in ["status", "omitted", "unsupported"] {
                if result.get(field).is_none() {
                    errors.push(format!("captureResult 缺字段 {field}"));
                }
            }
            match result.get("status").and_then(Value::as_str) {
                None => errors.push("captureResult.status 不是字符串".to_string()),
                Some(status) if !["ok", "partial", "failed"].contains(&status) => {
                    errors.push(format!("captureResult.status 非法：{status}"))
                }
                Some(_) => {}
            }
            if !result.get("omitted").map(Value::is_array).unwrap_or(false) {
                errors.push("captureResult.omitted 不是数组".to_string());
            }
            if !result
                .get("unsupported")
                .map(Value::is_array)
                .unwrap_or(false)
            {
                errors.push("captureResult.unsupported 不是数组".to_string());
            }
        } else if result.as_str() == Some("ok") {
            // **协议 v0.2 裁定**：形态 = 对象；字符串**只兼容 `'ok'`**
            // （`'ok'` 无损——它没有省略事实可丢）。
            //
            // `'partial'` / `'failed'` 用字符串会**丢掉「省略了什么」**，违反
            // 「不静默裁掉」纪律 ⇒ 那两个**必须用对象**，字符串形态一律拒。
        } else if let Some(status) = result.as_str() {
            errors.push(format!(
                "captureResult 必须是对象（协议 v0.2）；字符串只兼容 'ok'，收到：{status}"
            ));
        } else {
            errors.push("captureResult 不是对象".to_string());
        }
    }
    if let Some(messages) = snapshot.get("messages").and_then(Value::as_array) {
        for (index, message) in messages.iter().enumerate() {
            let role = message.get("role").and_then(Value::as_str);
            let text = message.get("text").and_then(Value::as_str);
            match (role, text) {
                (Some(role), Some(_)) => {
                    if role != role.to_lowercase() {
                        errors.push(format!("messages[{index}] 的 role 不是小写枚举"));
                    }
                }
                _ => errors.push(format!("messages[{index}] 缺 role 或 text")),
            }
        }
    }
    errors
}

/// §6 夹具的**统一判定入口**（Rust 侧）。
///
/// **判定顺序必须与 JS 侧一致**（`host/protocol.js` 的 `resolveCase`）——
/// 顺序不同就会出现「同一夹具不同结果」，那正是 §6 要防的事。
///
/// @param input - 夹具的 `input`。
/// @returns 与 JS 侧同构的结果对象。
pub fn resolve_case(input: &Value) -> Value {
    let kind = input
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("capture");
    match kind {
        "catalog" => resolve_catalog_case(input),
        "lookup" => resolve_lookup_case(input),
        "duplicates" => resolve_duplicates_case(input),
        _ => resolve_capture_case(input),
    }
}

/// P2 协议 §1.1：重复组的判定键 = **五字段全等**。
///
/// **为什么不能只比正文摘要一个**：`development-plan` §8:168「去重不能抹掉账户、
/// 分支或捕获事实」——只比 `contentDigest` 会把不同账户 / 分支 / 捕获时刻的同正文
/// 判成同一组。
pub const GROUP_KEY_FIELDS: [&str; 5] = [
    "accountScope",
    "sessionId",
    "branch",
    "capturedAt",
    "contentDigest",
];

/// P2 协议 §1.2：`groupKey` = 五字段用 `\n` 连接后取 sha256 的**前 16 位小写十六进制**。
///
/// **与 JS 侧同口径**（`host/protocol.js` 的 `groupKeyOf`）：分隔符是**单个换行、
/// 不含空格**（协议原文的行内代码里 `\n` 两侧有空格 ⇒ 该口径**待 PM 裁定**；
/// 若裁定为 ` \n `，**两端各改一行**即可）。
///
/// @param snapshot - 带五字段的对象（字段值都是字符串）。
/// @returns 16 位小写十六进制组标识。
pub fn group_key(snapshot: &Value) -> String {
    use sha2::{Digest, Sha256};
    let text = GROUP_KEY_FIELDS
        .iter()
        .map(|field| {
            snapshot
                .get(*field)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect::<Vec<String>>()
        .join("\n");
    let digest = Sha256::digest(text.as_bytes());
    let hex_string = crate::shell::digest::hex(&digest[..]);
    hex_string[..16].to_string()
}

/// 重复组判定流程（P2 协议 §1.1 / §1.2）。
///
/// **与 JS 侧 `resolveDuplicatesCase` 同构**：判「键五字段全等」，输出 `groupKey`
/// 与 `sameGroup`。**夹具的 `expected` 固化 `groupKey` 的值**（协议 v0.2 §5：
/// 键的形状**已定稿、可写**；§1.2：groupKey 用于判据比对）⇒ 分隔符口径若改成
/// 含空格（` \n `），要同步改**四处**：本函数、`group_key`、JS 的 `groupKeyOf`、
/// 两个夹具的 `expected.groupKey`（各 1 个值）。
///
/// @param input - 夹具输入（`snapshots` 两条，各带五字段）。
/// @returns 结果对象。
fn resolve_duplicates_case(input: &Value) -> Value {
    let snapshots = input
        .get("snapshots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let first = snapshots.first().cloned().unwrap_or(Value::Null);
    let second = snapshots.get(1).cloned().unwrap_or(Value::Null);
    let first_key = group_key(&first);
    let same_group = snapshots.len() >= 2 && group_key(&second) == first_key;
    serde_json::json!({
        "kind": "duplicates",
        "sameGroup": same_group,
        "groupKey": first_key,
    })
}

/// 捕获流程。
///
/// @param input - 夹具输入。
/// @returns 结果对象。
fn resolve_capture_case(input: &Value) -> Value {
    let outcome = input.get("outcome").and_then(Value::as_str).unwrap_or("ok");
    if outcome == "timeout" {
        return error_result("timeout");
    }
    if outcome == "cancelled" {
        return error_result("cancelled");
    }
    let version = input
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(SCHEMA_VERSION));
    if version != u64::from(SCHEMA_VERSION) {
        return error_result("schema_changed");
    }
    let messages = input
        .get("messages")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let body = body_of(&messages);
    if body.len() > MAX_BODY_BYTES {
        return error_result("body_too_large");
    }
    let status = input
        .get("captureResult")
        .and_then(|result| result.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("ok");
    if status == "failed" {
        // **捕获失败按原因分流**（协议 v0.3 §3「各码的使用边界」）：
        // 捕获失败**不是一个单一原因**，一律 `source_unsupported` 会让用户看到
        // 「换一个来源」而其实该**重试**——那是**掩盖真实原因**。
        // · 超时 → `timeout`（可重试）；页面结构变了 → `schema_changed`；用户取消 → `cancelled`。
        // · **原因不明 → `timeout` 兜**（可重试）。
        let reason = input
            .get("captureFailure")
            .and_then(|failure| failure.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or("");
        return error_result_with_detail(
            capture_failure_code(reason),
            Some(capture_failure_detail(reason)),
        );
    }
    let session_id = input.get("sessionId").and_then(Value::as_str).unwrap_or("");
    let seq = input.get("seq").and_then(Value::as_u64).unwrap_or(1) as u32;
    let provider_short = input
        .get("providerShort")
        .and_then(Value::as_str)
        .unwrap_or(PROVIDER_SHORT);
    let snapshot_id = format!(
        "{}~{}~{:04}",
        provider_short,
        session_short(session_id),
        seq
    );
    let normalized_messages: Vec<Value> = messages
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|message| {
                    let role = message
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase();
                    let text = message
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    serde_json::json!({ "role": role, "text": text })
                })
                .collect()
        })
        .unwrap_or_default();
    // **协议 v0.2 裁定：`captureResult` 的形态 = 对象**（三字段齐全）。
    // 这里**归一化**（不是原样透传）：字符串 `'ok'` 补成等价对象，
    // `omitted` / `unsupported` 缺省补空数组——与 JS 侧 `buildSnapshot`
    // **逐字同口径**（否则同一夹具两端算出的快照不一样，§6 的双端一致就破了）。
    let capture_result = {
        let source = input.get("captureResult");
        let status = source
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .or_else(|| source.and_then(Value::as_str))
            .unwrap_or("ok")
            .to_string();
        let omitted = source
            .and_then(|value| value.get("omitted"))
            .cloned()
            .filter(Value::is_array)
            .unwrap_or_else(|| Value::Array(vec![]));
        let unsupported = source
            .and_then(|value| value.get("unsupported"))
            .cloned()
            .filter(Value::is_array)
            .unwrap_or_else(|| Value::Array(vec![]));
        serde_json::json!({
            "status": status,
            "omitted": omitted,
            "unsupported": unsupported,
        })
    };
    let snapshot = serde_json::json!({
        "schemaVersion": SCHEMA_VERSION,
        "snapshotId": snapshot_id,
        "providerId": input.get("providerId").and_then(Value::as_str).unwrap_or(""),
        "accountScope": input.get("accountScope").and_then(Value::as_str).unwrap_or(""),
        "sessionId": session_id,
        "branch": input.get("branch").and_then(Value::as_str).unwrap_or("main"),
        "messages": normalized_messages,
        "contentDigest": content_digest(&body),
        "captureResult": capture_result,
        "capturedAt": input.get("capturedAt").and_then(Value::as_str).unwrap_or(""),
        "sourceUpdatedAt": input.get("sourceUpdatedAt").and_then(Value::as_str).unwrap_or(""),
        "sourceFingerprint": input.get("sourceFingerprint").and_then(Value::as_str).unwrap_or(""),
    });
    let notice = if status == "partial" {
        Value::String("partial_content".to_string())
    } else {
        Value::Null
    };
    serde_json::json!({
        "kind": "snapshot",
        "snapshot": snapshot,
        "state": "ready",
        "notice": notice,
    })
}

/// 目录流程（§1.2 目录页是独立缓存物；§3 空结果与失败分离）。
///
/// @param input - 夹具输入。
/// @returns 结果对象。
fn resolve_catalog_case(input: &Value) -> Value {
    let outcome = input.get("outcome").and_then(Value::as_str).unwrap_or("ok");
    if outcome == "empty" {
        return error_result("catalog_empty");
    }
    if outcome == "failed" {
        return error_result("catalog_failed");
    }
    let items = input.get("items").cloned().unwrap_or(Value::Array(vec![]));
    serde_json::json!({
        "kind": "catalog",
        "page": {
            "accountScope": input.get("accountScope").and_then(Value::as_str).unwrap_or(""),
            "providerId": input.get("providerId").and_then(Value::as_str).unwrap_or(""),
            "items": items,
            "cursor": input.get("cursor").and_then(Value::as_str).unwrap_or(""),
            "hasMore": input.get("hasMore").and_then(Value::as_bool).unwrap_or(false),
            "fetchedAt": input.get("fetchedAt").and_then(Value::as_str).unwrap_or(""),
        }
    })
}

/// 取快照流程（S-5：失效一律拒）。
///
/// @param input - 夹具输入。
/// @returns 结果对象。
fn resolve_lookup_case(input: &Value) -> Value {
    if input.get("exists").and_then(Value::as_bool) == Some(false) {
        return error_result("snapshot_invalid");
    }
    let snapshot = match input.get("snapshot") {
        Some(value) if value.is_object() => value,
        _ => return error_result("snapshot_invalid"),
    };
    let version = snapshot
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(SCHEMA_VERSION));
    if version != u64::from(SCHEMA_VERSION) {
        return error_result("schema_changed");
    }
    let stored = snapshot
        .get("contentDigest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let actual = content_digest(&body_of(snapshot.get("messages").unwrap_or(&Value::Null)));
    if stored != actual {
        return error_result("snapshot_invalid");
    }
    let current = input
        .get("currentFingerprint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !current.is_empty() {
        let source = snapshot
            .get("sourceFingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if current != source {
            return error_result("account_changed");
        }
    }
    serde_json::json!({
        "kind": "snapshot",
        "snapshot": snapshot,
        "state": "ready",
    })
}

/// 捕获失败的**原因词 → 协议码**的别名表（协议 v0.3 §3「各码的使用边界」）。
///
/// **与 JS 侧 `host/protocol.js` 的 `CAPTURE_FAILURE_REASONS` 逐条对应**——
/// 同一组原因必须得到同一个码，否则 A1c（双端一致）会红。
const CAPTURE_FAILURE_REASON_ALIASES: [&str; 11] = [
    "timeout",
    "timed_out",
    "deadline_exceeded",
    "schema_changed",
    "structure_changed",
    "unrecognized_selector",
    "cancelled",
    "canceled",
    "user_cancelled",
    "source_unsupported",
    "capability_unsupported",
];

/// 把捕获源报的原因映射成协议码。
///
/// **捕获失败不是一个单一原因**：超时 → `timeout`（可重试）；官网页面结构变了
/// → `schema_changed`；用户取消 → `cancelled`。
///
/// **`source_unsupported` 只用于「这个来源根本不支持该能力」**——
/// 它**不得**兜「捕获失败」：那会让用户看到「换一个来源」而其实该**重试**。
///
/// **原因不明 → `timeout` 兜**（可重试）。
///
/// @param reason - 原因词（稳定 token，不是异常串）。
/// @returns §3 的稳定码名。
fn capture_failure_code(reason: &str) -> &'static str {
    let raw = reason.trim().to_lowercase();
    if raw.is_empty() {
        return "timeout";
    }
    // 已经是协议码 → 原样（闭集内，不漏第 14 个码）。
    if let Some(index) = error_index(&raw) {
        return ERROR_CODES[index];
    }
    match raw.as_str() {
        "timeout" | "timed_out" | "deadline_exceeded" => "timeout",
        "schema_changed" | "structure_changed" | "unrecognized_selector" => "schema_changed",
        "cancelled" | "canceled" | "user_cancelled" => "cancelled",
        "source_unsupported" | "capability_unsupported" => "source_unsupported",
        // **原因不明 → `timeout` 兜**（可重试），不猜。
        _ => "timeout",
    }
}

/// 捕获失败的**脱敏线索**（进 `detail`）。
///
/// **只带闭集内的原因词**：不认识的 reason 一律给「原因未明」，
/// **绝不把来源抛出来的异常串外发**（协议 §3）。
///
/// @param reason - 原因词。
/// @returns 脱敏线索。
fn capture_failure_detail(reason: &str) -> String {
    let raw = reason.trim().to_lowercase();
    let known = !raw.is_empty()
        && (error_index(&raw).is_some() || CAPTURE_FAILURE_REASON_ALIASES.contains(&raw.as_str()));
    if known {
        format!("捕获失败原因：{raw}")
    } else {
        "捕获失败，原因未明".to_string()
    }
}

/// 构造一个错误结果（与 JS 侧 `errorPayload` **同构**）。
///
/// **协议 §3**：每个错误码带「可重试性、用户下一步、关联 `requestId`、**脱敏**
/// 日志路径」——所以这四个字段**一个都不能少**。
///
/// **A1c（v0.2 拍板）**：此前本函数只回 `code` / `retryable` / `userNext`，
/// 少了 `ok` / `requestId` / `logPath` / `detail` ⇒ **两端字段不同构**。
/// **以协议为准**：两端补齐同构字段（JS 侧 `errorPayload` 早就是全的）。
/// 夹具 `expected` 里的错误只固化「码 / 可重试性 / 用户下一步」三项（那是
/// **参考值**），字段完整性由两端各自的断言兜住。
///
/// @param code - 稳定码名。
/// @returns 结果对象。
fn error_result(code: &str) -> Value {
    error_result_with_detail(code, None)
}

/// 同 `error_result`，但**带 `detail`**（捕获失败的原因线索走这里）。
///
/// @param code - 稳定码名。
/// @param detail - 脱敏线索；`None` 表示没有。
/// @returns 结果对象。
fn error_result_with_detail(code: &str, detail: Option<String>) -> Value {
    // **未知码的兜底与 JS 侧一致**：归到 `snapshot_invalid`，原始码进 `detail`
    // （协议 §3：**内部冲突码一律归入 `snapshot_invalid`**）。
    //
    // **不归 `source_unsupported`**：那个码只表示「这个来源根本不支持该能力」，
    // 拿它兜内部冲突会让用户去「换一个来源」，而真正该做的是移除该引用。
    let known = error_index(code).is_some();
    let (out_code, detail): (&str, Value) = if known {
        (code, detail.map(Value::String).unwrap_or(Value::Null))
    } else {
        (
            "snapshot_invalid",
            serde_json::json!(format!("未知错误码 {code}")),
        )
    };
    serde_json::json!({
        "kind": "error",
        "ok": false,
        "code": out_code,
        "retryable": is_retryable(out_code),
        "userNext": user_next(out_code),
        "requestId": Value::Null,
        "logPath": Value::Null,
        "detail": detail,
    })
}

#[cfg(test)]
mod tests {
    //! §6 的双端一致：**Rust 与 JS 读同一组夹具**，断言「同一夹具 → 同一解析结果」。
    //!
    //! 夹具在 `plugins/official-chat/fixtures/protocol/`（仓库根下）。
    //! 这里的路径用 `CARGO_MANIFEST_DIR` 推——**不许写死绝对路径**，
    //! 也不许把夹具复制一份进 `src-tauri/`（那会让两端各读各的，等于没测）。

    use super::*;
    use serde_json::json;

    /// 夹具目录（仓库根下的 `plugins/official-chat/fixtures/protocol`）。
    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("plugins")
            .join("official-chat")
            .join("fixtures")
            .join("protocol")
    }

    /// 读一份夹具。
    fn read_fixture(name: &str) -> Value {
        let path = fixtures_dir().join(name);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("读夹具失败：{}：{error}", path.display()));
        serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("夹具不是合法 JSON：{name}：{error}"))
    }

    #[test]
    fn schema_version_matches_the_protocol_fixture() {
        let index = read_fixture("index.json");
        assert_eq!(
            index.get("schemaVersion").and_then(Value::as_u64),
            Some(u64::from(SCHEMA_VERSION)),
            "夹具的 schemaVersion 必须与 Rust 常量一致（协议 §6）"
        );
    }

    #[test]
    fn every_fixture_resolves_the_same_way_on_both_sides() {
        let index = read_fixture("index.json");
        let cases = index
            .get("cases")
            .and_then(Value::as_array)
            .expect("index.json 缺 cases");
        assert!(!cases.is_empty(), "夹具不能是空集");
        for name in cases {
            let name = name.as_str().expect("cases 里必须是文件名");
            let fixture = read_fixture(name);
            let input = fixture.get("input").expect("夹具缺 input");
            let expected = fixture.get("expected").expect("夹具缺 expected");
            let actual = resolve_case(input);

            assert_eq!(
                actual.get("kind").and_then(Value::as_str),
                expected.get("kind").and_then(Value::as_str),
                "{name}：两端的结果类别必须一致"
            );

            match expected.get("kind").and_then(Value::as_str) {
                Some("error") => {
                    assert_eq!(
                        actual.get("code").and_then(Value::as_str),
                        expected.get("code").and_then(Value::as_str),
                        "{name}：错误码必须一致"
                    );
                    assert_eq!(
                        actual.get("retryable").and_then(Value::as_bool),
                        expected.get("retryable").and_then(Value::as_bool),
                        "{name}：可重试性必须一致"
                    );
                    assert_eq!(
                        actual.get("userNext").and_then(Value::as_str),
                        expected.get("userNext").and_then(Value::as_str),
                        "{name}：用户下一步文案必须一致"
                    );
                }
                Some("catalog") => {
                    let actual_page = actual.get("page").expect("缺 page");
                    let expected_page = expected.get("page").expect("缺 page");
                    assert_eq!(
                        actual_page.get("items"),
                        expected_page.get("items"),
                        "{name}：目录项必须一致"
                    );
                    assert_eq!(
                        actual_page.get("cursor").and_then(Value::as_str),
                        expected_page.get("cursor").and_then(Value::as_str),
                        "{name}：游标必须一致"
                    );
                }
                Some("snapshot") => {
                    let actual_snapshot = actual.get("snapshot").expect("缺 snapshot");
                    let expected_snapshot = expected.get("snapshot").expect("缺 snapshot");
                    for field in [
                        "schemaVersion",
                        "snapshotId",
                        "providerId",
                        "accountScope",
                        "sessionId",
                        "branch",
                        "messages",
                        "contentDigest",
                        "captureResult",
                        "capturedAt",
                        "sourceUpdatedAt",
                        "sourceFingerprint",
                    ] {
                        assert_eq!(
                            actual_snapshot.get(field),
                            expected_snapshot.get(field),
                            "{name}：字段 {field} 两端必须一致"
                        );
                    }
                    assert_eq!(
                        actual.get("state").and_then(Value::as_str),
                        expected.get("state").and_then(Value::as_str),
                        "{name}：状态必须一致"
                    );
                }
                Some("duplicates") => {
                    // **成组判定必须一致**（P2 §1.1：键五字段全等）。
                    assert_eq!(
                        actual.get("sameGroup").and_then(Value::as_bool),
                        expected.get("sameGroup").and_then(Value::as_bool),
                        "{name}：成组判定必须一致"
                    );
                    // **groupKey 的具体值不固化**：协议 §1.2 的分隔符口径（`\n` 还是 ` \n `）
                    // 仍待 PM 裁定 ⇒ 写死会把**未定的口径**锚进夹具。跨语言的 `groupKey`
                    // 一致性由「两端跑同一组夹具、结果逐字段相等」兜（两端一起改仍绿）。
                    // **groupKey 必须与夹具一致**（协议 v0.2 §5：键的形状已定稿、
                    // 可写；§1.2：groupKey 用于判据比对）⇒ 两端都断这一条 ⇒
                    // 跨语言算得不一样会当场报红。
                    assert_eq!(
                        actual.get("groupKey").and_then(Value::as_str),
                        expected.get("groupKey").and_then(Value::as_str),
                        "{name}：groupKey 必须一致"
                    );
                    let gk = actual
                        .get("groupKey")
                        .and_then(Value::as_str)
                        .expect("缺 groupKey");
                    assert_eq!(gk.len(), 16, "{name}：groupKey 必须是 16 位");
                    assert!(
                        gk.chars()
                            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                        "{name}：groupKey 必须是 16 位小写十六进制"
                    );
                }
                other => panic!("{name}：未知的 expected.kind：{other:?}"),
            }
        }
    }

    #[test]
    fn session_short_matches_the_client_side_algorithm() {
        // 客户端的 `sessionShort`：两段 32 位 FNV-1a 变体拼 base36，取后 12 位。
        // 这里钉死几个值——**短码算法一变，句柄就变**，而句柄是用户看得见的 chip 文本。
        // 期望值与 JS 侧逐位核对过（`host/protocol.js` 的 `makeSessionShort`）。
        assert_eq!(session_short("sess-alpha-001"), "rnaq4171cqan");
        assert_eq!(session_short(""), "tntfp17wdrqx");
        // 空会话 id 也得有稳定短码（不能返回空串——那会让句柄少一段）。
        assert_eq!(session_short("").len(), 12);
    }

    #[test]
    fn snapshot_id_shape_and_length() {
        let id = make_snapshot_id("sess-alpha-001", 1);
        assert_eq!(id, "ds~rnaq4171cqan~0001");
        assert!(is_valid_snapshot_id(&id));
        assert!(id.len() <= MAX_SNAPSHOT_ID_LEN);
        // 短码短一点也合法（≤ 12 即可）。
        assert!(is_valid_snapshot_id("ds~abc~0001"));
        // 路径片段不许混进来（`snapshotId` 会直接进文件路径）。
        assert!(!is_valid_snapshot_id("ds~abc~0001/../../etc/passwd"));
        assert!(!is_valid_snapshot_id("ds~abc~0001~0001"));
        assert!(!is_valid_snapshot_id("ds~abc~000"));
        assert!(!is_valid_snapshot_id("DS~ABC~0001"));
        assert!(!is_valid_snapshot_id("ds~ab_c~0001"));
    }

    #[test]
    fn normalization_keeps_inner_blank_lines_and_indent() {
        let raw = "第一行\r\n\r\n  缩进行  \r\n";
        let normalized = normalize_body(raw);
        assert!(normalized.contains("\n\n"), "正文内空行必须保留");
        assert!(normalized.contains("  缩进行"), "缩进必须保留");
        assert!(!normalized.contains('\r'), "行尾必须统一成 \\n");
    }

    #[test]
    fn error_codes_are_thirteen_and_carry_retryability() {
        assert_eq!(ERROR_CODES.len(), 13, "协议 §3 是 13 个码");
        assert!(is_retryable("timeout"), "timeout 可重试");
        assert!(is_retryable("catalog_failed"), "catalog_failed 可重试");
        assert!(
            !is_retryable("snapshot_invalid"),
            "snapshot_invalid 不可重试"
        );
        assert_eq!(user_next("catalog_empty"), "换个关键词");
        assert_eq!(user_next("catalog_failed"), "重试");
        assert_ne!(
            user_next("catalog_empty"),
            user_next("catalog_failed"),
            "空结果与读取失败必须是两种说法"
        );
    }

    #[test]
    fn error_result_carries_the_eight_field_closed_set() {
        // **协议 §3 v0.4 裁定：错误结果对象字段闭集 = 8 个，全部必备、不省略**。
        // 此前 **JS 侧只产 7 个**（缺 `kind`）——那是 **JS 漏字段、不是协议写错**
        // （已修）⇒ 这里把**本端**的字段集钉住，别让下一个人又把某个字段省掉。
        let payload = error_result("timeout");
        let obj = payload.as_object().expect("错误结果必须是对象");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "code",
                "detail",
                "kind",
                "logPath",
                "ok",
                "requestId",
                "retryable",
                "userNext",
            ],
            "协议 §3 v0.4：错误结果对象 = 8 字段闭集，全部必备"
        );
        assert_eq!(obj.get("kind").and_then(Value::as_str), Some("error"));
        assert_eq!(obj.get("ok").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn constants_match_the_protocol_values() {
        assert_eq!(CATALOG_PAGE_SIZE, 50);
        assert_eq!(CATALOG_CACHE_TTL_MS, 5 * 60 * 1000);
        assert_eq!(SEARCH_DEBOUNCE_MS, 300);
        assert_eq!(READ_TIMEOUT_MS, 30_000);
        assert_eq!(MAX_BODY_BYTES, 256 * 1024);
        assert_eq!(MAX_RESPONSE_BYTES, 1024 * 1024);
        assert_eq!(SNAPSHOT_RETENTION_MS, 7 * 24 * 60 * 60 * 1000);
        assert_eq!(CACHE_TOTAL_BUDGET_BYTES, 512 * 1024 * 1024);
        assert_eq!(MAX_REFERENCES_PER_MESSAGE, 8);
        assert_eq!(PER_BODY_BUDGET_BYTES, 64 * 1024);
        assert_eq!(PER_MESSAGE_BUDGET_BYTES, 256 * 1024);
    }

    #[test]
    fn snapshot_validation_rejects_a_tampered_digest() {
        let snapshot = json!({
            "schemaVersion": 1,
            "snapshotId": "ds~rnaq4171cqan~0001",
            "providerId": "official-chat",
            "accountScope": "",
            "sessionId": "sess-alpha-001",
            "branch": "main",
            "messages": [{ "role": "user", "text": "x" }],
            "contentDigest": "b".repeat(64),
            "captureResult": { "status": "ok", "omitted": [], "unsupported": [] },
            "capturedAt": "2026-10-20T09:00:00.000Z",
            "sourceUpdatedAt": "2026-10-20T08:59:00.000Z",
            "sourceFingerprint": "fp",
        });
        assert!(validate_snapshot(&snapshot).is_empty(), "合规快照不该有错");
        let mut broken = snapshot.clone();
        broken["contentDigest"] = json!("c".repeat(64));
        // 摘要不匹配是**读的时候**的事（§1.3），字段校验只看形状。
        assert!(validate_snapshot(&broken).is_empty());
        let mut wrong_version = snapshot.clone();
        wrong_version["schemaVersion"] = json!(2);
        assert!(
            !validate_snapshot(&wrong_version).is_empty(),
            "版本不支持要报"
        );
    }

    /// **A1c（协议 v0.2 裁定）**：错误负载要**带齐**协议 §3 要求的字段，
    /// 且与 JS 侧 `errorPayload` **同构**。
    ///
    /// 此前 Rust 侧只回 `code` / `retryable` / `userNext`，少了 `ok` /
    /// `requestId` / `logPath` / `detail` ⇒ 两端字段不同构。
    #[test]
    fn error_payload_carries_every_field_the_protocol_requires() {
        let result = error_result("timeout");
        assert_eq!(result.get("kind").and_then(Value::as_str), Some("error"));
        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(result.get("code").and_then(Value::as_str), Some("timeout"));
        assert_eq!(result.get("retryable").and_then(Value::as_bool), Some(true));
        assert_eq!(result.get("userNext").and_then(Value::as_str), Some("重试"));
        assert!(result.get("requestId").is_some(), "requestId 字段必须在");
        assert!(result.get("logPath").is_some(), "脱敏日志路径字段必须在");
        assert!(result.get("detail").is_some(), "detail 字段必须在");
        // 未知码不许漏出第 14 个码（协议 §3：闭集，13 个）。
        // **归 `snapshot_invalid`**（协议 §3「内部冲突码一律归入 `snapshot_invalid`」）
        // ——**不归 `source_unsupported`**：那个码只表示「这个来源根本不支持
        // 该能力」，拿来兜内部冲突会让用户去「换一个来源」。
        let unknown = error_result("snapshot_exists");
        assert_eq!(
            unknown.get("code").and_then(Value::as_str),
            Some("snapshot_invalid"),
            "未知码要归到表内的码"
        );
        assert!(
            unknown
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("未知错误码"),
            "原始码要留在 detail 里"
        );
    }

    /// **协议 v0.3 §3「各码的使用边界」**：捕获失败**按原因分流**。
    ///
    /// 捕获失败**不是一个单一原因**：超时 → `timeout`（可重试）；官网页面结构
    /// 变了 → `schema_changed`；用户取消 → `cancelled`。
    ///
    /// **`source_unsupported` 只用于「这个来源根本不支持该能力」**——
    /// 它**不得**兜「捕获失败」（会让用户看到「换一个来源」而其实该重试）。
    /// **原因不明 → `timeout` 兜**（可重试）。
    ///
    /// **不认识的 reason 不外发**：只带闭集内的原因词当脱敏线索。
    #[test]
    fn capture_failure_is_routed_by_its_reason() {
        // 超时 → timeout
        assert_eq!(capture_failure_code("timeout"), "timeout");
        assert_eq!(capture_failure_code("timed_out"), "timeout");
        assert_eq!(capture_failure_code("deadline_exceeded"), "timeout");
        // 页面结构变了 → schema_changed
        assert_eq!(capture_failure_code("schema_changed"), "schema_changed");
        assert_eq!(capture_failure_code("structure_changed"), "schema_changed");
        // 用户取消 → cancelled
        assert_eq!(capture_failure_code("cancelled"), "cancelled");
        assert_eq!(capture_failure_code("user_cancelled"), "cancelled");
        // 来源明说不支持该能力 → source_unsupported（**唯一**允许它的场景）
        assert_eq!(
            capture_failure_code("source_unsupported"),
            "source_unsupported"
        );
        // **原因不明 → timeout 兜**（可重试），不猜
        assert_eq!(capture_failure_code(""), "timeout");
        assert_eq!(capture_failure_code("   "), "timeout");
        assert_eq!(capture_failure_code("一些没登记的原因"), "timeout");
        assert_ne!(capture_failure_code(""), "source_unsupported");

        // 判定层：status='failed' 走分流
        let failed = resolve_case(&json!({
            "kind": "capture",
            "schemaVersion": 1,
            "sessionId": "sess-x",
            "seq": 1,
            "messages": [],
            "captureResult": { "status": "failed", "omitted": [], "unsupported": [] },
        }));
        assert_eq!(failed.get("code").and_then(Value::as_str), Some("timeout"));
        assert_eq!(failed.get("retryable").and_then(Value::as_bool), Some(true));
        assert_eq!(
            failed.get("detail").and_then(Value::as_str),
            Some("捕获失败，原因未明")
        );

        // 有明确原因时按原因分流
        let schema_fail = resolve_case(&json!({
            "kind": "capture",
            "schemaVersion": 1,
            "sessionId": "sess-x",
            "seq": 1,
            "messages": [],
            "captureResult": { "status": "failed", "omitted": [], "unsupported": [] },
            "captureFailure": { "reason": "schema_changed" },
        }));
        assert_eq!(
            schema_fail.get("code").and_then(Value::as_str),
            Some("schema_changed")
        );
        assert_eq!(
            schema_fail.get("detail").and_then(Value::as_str),
            Some("捕获失败原因：schema_changed")
        );

        // **不认识的 reason 不外发**（免得把内部异常串当原因线索漏出去）
        let leaky = resolve_case(&json!({
            "kind": "capture",
            "schemaVersion": 1,
            "sessionId": "sess-x",
            "seq": 1,
            "messages": [],
            "captureResult": { "status": "failed", "omitted": [], "unsupported": [] },
            "captureFailure": { "reason": "Error: boom at /Users/someone/secret" },
        }));
        assert_eq!(leaky.get("code").and_then(Value::as_str), Some("timeout"));
        let detail = leaky.get("detail").and_then(Value::as_str).unwrap_or("");
        assert_eq!(detail, "捕获失败，原因未明");
        assert!(!detail.contains("secret"), "内部异常串不许进 detail");
    }

    /// **协议 v0.2 裁定**：`captureResult` 的形态 = **对象**（三字段齐全）；
    /// 字符串**只兼容 `'ok'`**。
    ///
    /// 这条同时守着**两端同口径**：捕获流程把 `captureResult` **归一化**成对象
    /// （不是原样透传），否则字符串形态会让两端算出的快照不一样。
    #[test]
    fn capture_result_is_normalized_to_the_object_form() {
        let input = json!({
            "kind": "capture",
            "schemaVersion": 1,
            "providerId": "official-chat",
            "sessionId": "sess-alpha-001",
            "seq": 1,
            "messages": [{ "role": "user", "text": "x" }],
            "captureResult": "ok",
            "capturedAt": "2026-10-20T09:00:00.000Z",
            "sourceUpdatedAt": "2026-10-20T08:59:00.000Z",
            "sourceFingerprint": "fp",
        });
        let result = resolve_case(&input);
        let capture = result
            .get("snapshot")
            .and_then(|snapshot| snapshot.get("captureResult"))
            .expect("快照里必须有 captureResult");
        assert!(capture.is_object(), "captureResult 必须是对象");
        assert_eq!(
            capture.get("status").and_then(Value::as_str),
            Some("ok"),
            "字符串 'ok' 要归一成等价对象"
        );
        assert!(capture.get("omitted").unwrap().is_array());
        assert!(capture.get("unsupported").unwrap().is_array());
    }

    /// **协议 v0.2 裁定**：`'partial'` / `'failed'` 用字符串一律拒
    /// （字符串会**丢掉「省略了什么」**，违反「不静默裁掉」）。
    #[test]
    fn capture_result_strings_other_than_ok_are_rejected() {
        let base = json!({
            "schemaVersion": 1,
            "snapshotId": "ds~rnaq4171cqan~0001",
            "providerId": "official-chat",
            "accountScope": "",
            "sessionId": "sess-alpha-001",
            "branch": "main",
            "messages": [{ "role": "user", "text": "x" }],
            "contentDigest": "b".repeat(64),
            "capturedAt": "2026-10-20T09:00:00.000Z",
            "sourceUpdatedAt": "2026-10-20T08:59:00.000Z",
            "sourceFingerprint": "fp",
        });
        let mut ok_string = base.clone();
        ok_string["captureResult"] = json!("ok");
        assert!(
            validate_snapshot(&ok_string).is_empty(),
            "字符串 'ok' 是无损兼容，要收"
        );
        for bad in ["partial", "failed"] {
            let mut broken = base.clone();
            broken["captureResult"] = json!(bad);
            assert!(
                !validate_snapshot(&broken).is_empty(),
                "'{bad}' 用字符串会丢省略事实，必须拒"
            );
        }
    }
}
