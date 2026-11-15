//! Hook v1 请求/响应结构与校验（P0 冻结，P1 实现）。
//!
//! 这里只做纯数据处理：不读磁盘、不联网。校验失败的返回值带 HTTP 状态与
//! 机器可读错误码，由 [`crate::hooks::server`] 转成响应；错误文案面向发送方，
//! 因此保持简短、可操作，不包含内部路径或令牌。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// P1 接受的协议版本。新增字段需要同步提升版本号，不能静默改变语义。
pub(crate) const SCHEMA_VERSION: &str = "1.0";

pub(crate) const MAX_TITLE_BYTES: usize = 512;
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024;
pub(crate) const MAX_EXTERNAL_ID_CHARS: usize = 200;
pub(crate) const MAX_EVENT_TYPE_BYTES: usize = 64;
pub(crate) const MAX_PARAMETERS_BYTES: usize = 64 * 1024;
pub(crate) const MAX_PARAMETERS_NODES: usize = 512;
pub(crate) const MAX_PARAMETERS_DEPTH: usize = 8;
pub(crate) const MAX_SENDER_POLICY_BYTES: usize = 64;

pub(crate) const STATE_RECEIVING: &str = "receiving";
pub(crate) const STATE_READY: &str = "ready";

/// 一次校验失败：HTTP 状态 + 机器码 + 给发送方的短原因。
#[derive(Debug, Clone)]
pub(crate) struct RequestError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl RequestError {
    pub(crate) fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", message)
    }

    pub(crate) fn too_large(message: impl Into<String>) -> Self {
        Self::new(413, "too_large", message)
    }

    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(415, "unsupported_media_type", message)
    }
}

/// `POST .../events` 的请求体。顶层未知字段直接拒绝：发送方拼错字段时要当场
/// 看见，而不是以为收下了、其实什么都没存。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateEventRequest {
    pub schema_version: String,
    pub external_id: String,
    pub event_type: String,
    pub occurred_at: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
    #[serde(default)]
    pub sender_policy: Option<String>,
    /// P1 只接受空数组；非空时明确拒绝，不假装收下了附件。
    #[serde(default)]
    pub attachments: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub commit: bool,
}

/// 校验、规范化后的事件。指纹与入库都以这份数据为准。
#[derive(Debug, Clone)]
pub(crate) struct NormalizedEvent {
    pub external_id: String,
    pub event_type: String,
    pub occurred_at_ms: i64,
    pub title: String,
    pub text: String,
    pub parameters: serde_json::Value,
    pub sender_policy: Option<String>,
    pub commit: bool,
    pub fingerprint: String,
}

/// 指纹的规范化形状。字段顺序固定；`parameters` 是自由 JSON，serde_json
/// 默认用 BTreeMap 存对象，因此键序不影响结果（`fingerprint_is_stable` 钉住）。
#[derive(Serialize)]
struct FingerprintPayload<'a> {
    schema_version: &'a str,
    external_id: &'a str,
    event_type: &'a str,
    occurred_at_ms: i64,
    title: &'a str,
    text: &'a str,
    parameters: &'a serde_json::Value,
    attachments: &'a [serde_json::Value],
}

pub(crate) fn normalize(request: CreateEventRequest) -> Result<NormalizedEvent, RequestError> {
    if request.schema_version != SCHEMA_VERSION {
        return Err(RequestError::invalid(format!(
            "schema_version 必须为 \"{SCHEMA_VERSION}\""
        )));
    }

    let external_id = request.external_id;
    if external_id.trim().is_empty() || external_id.chars().any(char::is_control) {
        return Err(RequestError::invalid("external_id 不能为空或包含控制字符"));
    }
    if external_id.chars().count() > MAX_EXTERNAL_ID_CHARS {
        return Err(RequestError::invalid(format!(
            "external_id 最多 {MAX_EXTERNAL_ID_CHARS} 个字符"
        )));
    }

    let event_type = request.event_type;
    if event_type.is_empty() || event_type.len() > MAX_EVENT_TYPE_BYTES {
        return Err(RequestError::invalid(format!(
            "event_type 必须为 1–{MAX_EVENT_TYPE_BYTES} 字节"
        )));
    }
    if !event_type
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(RequestError::invalid("event_type 只允许字母、数字与 . _ -"));
    }

    let occurred_at_ms = parse_rfc3339_ms(&request.occurred_at)?;

    let title = request.title.unwrap_or_default();
    if title.len() > MAX_TITLE_BYTES {
        return Err(RequestError::invalid(format!(
            "title 最多 {MAX_TITLE_BYTES} 字节"
        )));
    }

    let text = request.text.unwrap_or_default();
    if text.len() > MAX_TEXT_BYTES {
        return Err(RequestError::too_large(format!(
            "正文最多 {MAX_TEXT_BYTES} 字节；更长的内容请作为附件上传（后续版本）"
        )));
    }

    let parameters = match request.parameters {
        None | Some(serde_json::Value::Null) => serde_json::Value::Object(Default::default()),
        Some(value @ serde_json::Value::Object(_)) => value,
        Some(_) => return Err(RequestError::invalid("parameters 必须是 JSON 对象")),
    };
    let (nodes, depth) = parameter_stats(&parameters, 1);
    if depth > MAX_PARAMETERS_DEPTH {
        return Err(RequestError::invalid(format!(
            "parameters 嵌套最多 {MAX_PARAMETERS_DEPTH} 层"
        )));
    }
    if nodes > MAX_PARAMETERS_NODES {
        return Err(RequestError::invalid(format!(
            "parameters 最多 {MAX_PARAMETERS_NODES} 个值"
        )));
    }
    let parameters_bytes = serde_json::to_vec(&parameters)
        .map_err(|e| RequestError::invalid(format!("parameters 无法序列化：{e}")))?;
    if parameters_bytes.len() > MAX_PARAMETERS_BYTES {
        return Err(RequestError::too_large(format!(
            "parameters 最多 {MAX_PARAMETERS_BYTES} 字节"
        )));
    }

    let sender_policy = request.sender_policy;
    if let Some(value) = &sender_policy {
        if value.len() > MAX_SENDER_POLICY_BYTES || value.chars().any(char::is_control) {
            return Err(RequestError::invalid("sender_policy 过长或包含控制字符"));
        }
    }

    if request
        .attachments
        .as_ref()
        .is_some_and(|attachments| !attachments.is_empty())
    {
        return Err(RequestError::unsupported(
            "附件上传将在后续版本提供；P1 只接受空 attachments 数组",
        ));
    }

    let attachments: Vec<serde_json::Value> = Vec::new();
    let fingerprint = fingerprint(&FingerprintPayload {
        schema_version: SCHEMA_VERSION,
        external_id: &external_id,
        event_type: &event_type,
        occurred_at_ms,
        title: &title,
        text: &text,
        parameters: &parameters,
        attachments: &attachments,
    });

    Ok(NormalizedEvent {
        external_id,
        event_type,
        occurred_at_ms,
        title,
        text,
        parameters,
        sender_policy,
        commit: request.commit,
        fingerprint,
    })
}

fn parse_rfc3339_ms(text: &str) -> Result<i64, RequestError> {
    let parsed = time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
        .map_err(|_| RequestError::invalid("occurred_at 必须是 RFC3339 时间"))?;
    let nanos = parsed.unix_timestamp_nanos();
    i64::try_from(nanos / 1_000_000)
        .map_err(|_| RequestError::invalid("occurred_at 超出可表示范围"))
}

fn fingerprint(payload: &FingerprintPayload<'_>) -> String {
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    crate::shell::digest::hex(&Sha256::digest(&bytes)[..])
}

/// 参数值的节点数与最大深度。根对象算第 1 层。
fn parameter_stats(value: &serde_json::Value, depth: usize) -> (usize, usize) {
    let mut nodes = 1usize;
    let mut max_depth = depth;
    match value {
        serde_json::Value::Object(map) => {
            for child in map.values() {
                let (child_nodes, child_depth) = parameter_stats(child, depth + 1);
                nodes += child_nodes;
                max_depth = max_depth.max(child_depth);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                let (child_nodes, child_depth) = parameter_stats(child, depth + 1);
                nodes += child_nodes;
                max_depth = max_depth.max(child_depth);
            }
        }
        _ => {}
    }
    (nodes, max_depth)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ErrorDetail {
    pub code: String,
    pub message: String,
    pub request_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ErrorResponse {
    pub error: ErrorDetail,
}

/// 创建事件与提交事件的响应形状（附件列表 P1 恒为空）。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct EventStatusResponse {
    pub event_id: String,
    pub state: String,
    pub attachments: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub receipt_url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ReceiptResponse {
    pub event_id: String,
    pub state: String,
    pub attachments: Vec<serde_json::Value>,
    pub occurred_at: i64,
    pub received_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

pub(crate) fn receipt_url(endpoint_id: &str, event_id: &str) -> String {
    format!("/hooks/v1/endpoints/{endpoint_id}/events/{event_id}/receipt")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<NormalizedEvent, RequestError> {
        let request: CreateEventRequest = serde_json::from_str(json).expect("请求样本可解析");
        normalize(request)
    }

    #[test]
    fn accepts_a_text_event_and_normalizes_defaults() {
        let event = parse(include_str!("fixtures/create_text.json")).expect("合法事件");
        assert_eq!(event.external_id, "build:Project_Plague/Win64:128");
        assert_eq!(event.title, "Project_Plague Win64 #128");
        assert!(event.text.contains("构建结果"));
        assert_eq!(
            event.parameters,
            serde_json::json!({"job": "Project_Plague/Win64", "build": 128})
        );
        assert!(event.commit);
        assert!(!event.fingerprint.is_empty());
    }

    #[test]
    fn accepts_parameters_only_event() {
        let event = parse(include_str!("fixtures/create_parameters.json")).expect("合法事件");
        assert_eq!(event.text, "");
        assert_eq!(event.parameters["result"], "FAILURE");
        assert!(!event.commit);
    }

    #[test]
    fn fingerprint_ignores_key_order_and_explicit_defaults() {
        let base = parse(include_str!("fixtures/create_text.json")).expect("基线");
        let equivalent = parse(include_str!("fixtures/retry_equivalent.json")).expect("等价重试");
        assert_eq!(base.fingerprint, equivalent.fingerprint);
    }

    #[test]
    fn fingerprint_changes_when_content_changes() {
        let base = parse(include_str!("fixtures/create_text.json")).expect("基线");
        let conflict = parse(include_str!("fixtures/retry_conflict.json")).expect("冲突样本");
        assert_ne!(base.fingerprint, conflict.fingerprint);
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/create_text.json")).expect("样本");
        value["schema_version"] = serde_json::json!("2.0");
        let request: CreateEventRequest = serde_json::from_value(value).expect("仍可解析");
        let error = normalize(request).expect_err("版本不符必须拒绝");
        assert_eq!(error.status, 400);
        assert_eq!(error.code, "invalid_request");
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/create_text.json")).expect("样本");
        value["paramters"] = serde_json::json!({});
        let error =
            serde_json::from_value::<CreateEventRequest>(value).expect_err("未知字段必须拒绝解析");
        assert!(error.to_string().contains("paramters"));
    }

    #[test]
    fn rejects_non_empty_attachments_in_p1() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/create_text.json")).expect("样本");
        value["attachments"] = serde_json::json!([{"client_id": "log"}]);
        let request: CreateEventRequest = serde_json::from_value(value).expect("仍可解析");
        let error = normalize(request).expect_err("P1 不接受附件");
        assert_eq!(error.status, 415);
    }

    #[test]
    fn rejects_parameters_that_are_not_an_object() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/create_parameters.json")).expect("样本");
        value["parameters"] = serde_json::json!([1, 2, 3]);
        let request: CreateEventRequest = serde_json::from_value(value).expect("仍可解析");
        assert_eq!(normalize(request).expect_err("数组不是对象").status, 400);
    }

    #[test]
    fn rejects_too_deep_and_too_many_parameters() {
        let mut deep = serde_json::json!({});
        for _ in 0..MAX_PARAMETERS_DEPTH {
            deep = serde_json::json!({ "next": deep });
        }
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/create_parameters.json")).expect("样本");
        value["parameters"] = deep;
        let request: CreateEventRequest = serde_json::from_value(value).expect("仍可解析");
        assert_eq!(normalize(request).expect_err("深度超限").status, 400);

        let wide = serde_json::Value::Array(
            (0..MAX_PARAMETERS_NODES + 1)
                .map(|index| serde_json::json!(index))
                .collect(),
        );
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/create_parameters.json")).expect("样本");
        value["parameters"] = wide;
        let request: CreateEventRequest = serde_json::from_value(value).expect("仍可解析");
        assert_eq!(normalize(request).expect_err("节点超限").status, 400);
    }

    #[test]
    fn error_and_receipt_fixtures_match_contract_types() {
        let receipt: ReceiptResponse =
            serde_json::from_str(include_str!("fixtures/receipt_ready.json")).expect("回执样本");
        assert_eq!(receipt.state, STATE_READY);
        let error: ErrorResponse =
            serde_json::from_str(include_str!("fixtures/error_conflict.json")).expect("错误样本");
        assert_eq!(error.error.code, "event_conflict");
        assert!(error.error.request_id.starts_with("req_"));
    }
}
