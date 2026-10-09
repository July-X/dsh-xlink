//! 推理转发（P4 起步，开发计划 §5「推理」行 + §8.1/§8.3）。
//!
//! 职责链：Host 把 dsh 请求转换成 Responses 载荷 → 桥接**校验**（模型在
//! 目录、双 revision 一致、强度在能力表内、载荷白名单）→ 加固定参数
//! （`store:false`、`stream:true`——设计 §7，绝不透传调用方的同名键）→
//! 带 access token 转发官方端点 → 把上游 SSE 逐事件转成 NDJSON 下发。
//!
//! **终止纪律**（设计 §7）：只有读到成功终止事件才结算成功；连接结束
//! 而没有终止事件 = 失败（已有文本不丢，但不算成功）。
//!
//! 终止事件按 Responses API 的精确类型判定；普通 output item 的 completed
//! 事件不能提前结算整条响应。

use std::io::BufRead;

use crate::openai::transport::Failure;

/// 套餐路线载荷白名单（设计 §8.1：不照搬普通 API SDK 的默认字段）。
/// 不在表里的键一律拒绝——静默丢掉等于让调用方以为限制生效了。
const PAYLOAD_WHITELIST: &[&str] = &[
    "model",
    "input",
    "instructions",
    "tools",
    "tool_choice",
    "parallel_tool_calls",
    "reasoning",
];

/// 载荷校验错误：分类给 Host 翻译（§8.3 的错误分类表）。
#[derive(Debug)]
pub(crate) enum RequestError {
    /// 模型不在当前账号目录。
    ModelNotInCatalog(String),
    /// 目录/能力表 revision 过期（Host 刷新后重试）。
    StaleRevision {
        current: String,
    },
    /// 强度不在该模型能力表内（重新选择思考强度）。
    EffortNotAvailable {
        model: String,
        effort: String,
    },
    /// 载荷带白名单外的键（无法执行的配置，拒绝并提示）。
    FieldNotAllowed(String),
    Message(String),
}

impl RequestError {
    pub(crate) fn message(&self) -> String {
        match self {
            RequestError::ModelNotInCatalog(model) => {
                format!("模型 {model} 不在当前账号目录；请刷新目录后重选")
            }
            RequestError::StaleRevision { current } => {
                format!("目录已更新（当前 {current}）；请刷新模型列表后重试")
            }
            RequestError::EffortNotAvailable { model, effort } => {
                format!("思考强度 {effort} 不在 {model} 的已验证档位内；请重新选择")
            }
            RequestError::FieldNotAllowed(field) => {
                format!("套餐路线不支持配置 {field}；该限制无法执行，已拒绝请求（不会静默忽略）")
            }
            RequestError::Message(detail) => detail.clone(),
        }
    }
}

/// 一次已校验的推理请求。
#[derive(Debug)]
pub(crate) struct ValidatedRequest {
    pub(crate) payload: serde_json::Value,
}

/// 校验 Host 送来的请求信封并产出最终上游载荷。
///
/// 信封：`{model, catalogRevision, reasoningEffort?, payload}`；`payload`
/// 是 Host 转换好的 Responses 载荷（不含 store/stream——这两个键属于
/// 路线固定参数，出现在 payload 里即拒绝）。
pub(crate) fn validate_request(
    envelope: &serde_json::Value,
    catalog: &crate::openai::catalog::Catalog,
) -> Result<ValidatedRequest, RequestError> {
    let model = envelope
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RequestError::Message("请求缺 model".into()))?;
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.id == model)
        .ok_or_else(|| RequestError::ModelNotInCatalog(model.to_string()))?;

    let current_revision = format!("{}|cap{}", catalog.revision, catalog.capability_revision);
    let got_revision = envelope
        .get("catalogRevision")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if got_revision != current_revision {
        return Err(RequestError::StaleRevision {
            current: current_revision,
        });
    }

    if let Some(effort) = envelope.get("reasoningEffort").and_then(|v| v.as_str()) {
        let allowed = entry.efforts.as_deref().unwrap_or(&[]);
        if !allowed.iter().any(|candidate| candidate == effort) {
            // 能力未验证（unknown，无档位表）时同样拒绝——不显示未验证
            // 档位的选择，也就不接受对它的显式选择（设计 §6.2）。
            return Err(RequestError::EffortNotAvailable {
                model: model.to_string(),
                effort: effort.to_string(),
            });
        }
    }

    let payload = envelope
        .get("payload")
        .cloned()
        .ok_or_else(|| RequestError::Message("请求缺 payload".into()))?;
    let Some(object) = payload.as_object() else {
        return Err(RequestError::Message("payload 必须是对象".into()));
    };
    for key in object.keys() {
        if !PAYLOAD_WHITELIST.contains(&key.as_str()) {
            return Err(RequestError::FieldNotAllowed(key.clone()));
        }
    }
    if object.contains_key("store") || object.contains_key("stream") {
        return Err(RequestError::FieldNotAllowed(
            "store/stream（路线固定参数，由服务端决定）".into(),
        ));
    }

    // 组装最终载荷：路线固定参数在这里写入，不信任调用方。
    let mut final_payload = payload;
    final_payload["store"] = serde_json::json!(false);
    final_payload["stream"] = serde_json::json!(true);
    Ok(ValidatedRequest {
        payload: final_payload,
    })
}

/// SIWC 推理路径：挂在资源基址 api.openai.com/v1 下，与模型目录同源。
pub(crate) const RESPONSES_PATH: &str = "/responses";

/// 桥接终止包络（桥接流的最后一行；设计 §7 的「只有成功终止事件才结算
/// 成功」由它承载：`completed` / `failed` / `incomplete`）。
pub(crate) struct TerminalEnvelope {
    pub(crate) status: &'static str,
    pub(crate) replay: Option<serde_json::Value>,
    pub(crate) detail: Option<String>,
}

impl TerminalEnvelope {
    pub(crate) fn incomplete() -> Self {
        TerminalEnvelope {
            status: "incomplete",
            replay: None,
            detail: None,
        }
    }

    pub(crate) fn failed(detail: String) -> Self {
        TerminalEnvelope {
            status: "failed",
            replay: None,
            detail: Some(detail),
        }
    }

    pub(crate) fn from(terminal: Terminal) -> Self {
        match terminal {
            Terminal::Completed { replay } => TerminalEnvelope {
                status: "completed",
                replay,
                detail: None,
            },
            Terminal::Failed { detail } => TerminalEnvelope::failed(detail),
        }
    }

    pub(crate) fn to_line(&self) -> String {
        serde_json::json!({
            "type": "bridge.terminal",
            "status": self.status,
            "replay": self.replay,
            "detail": self.detail,
        })
        .to_string()
    }
}

/// 上游 SSE 的一个事件（`data:` 行的 JSON；非 data 行与空行被跳过）。
pub(crate) struct UpstreamEvent {
    pub(crate) data: serde_json::Value,
}

/// 终止判定（§8.3 的骨架三分类；完整分类表随联调补）。
pub(crate) enum Terminal {
    /// 成功终止：`response.completed` 一类，载荷里可能带 usage 与回放材料。
    Completed { replay: Option<serde_json::Value> },
    /// 失败终止：`response.failed` / 顶层 `error`。
    Failed { detail: String },
}

pub(crate) fn classify_terminal(event: &serde_json::Value) -> Option<Terminal> {
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if event_type == "response.completed" {
        let replay = event
            .get("response")
            .map(|response| serde_json::json!({ "response": response.clone() }));
        return Some(Terminal::Completed { replay });
    }
    if event_type == "response.failed" || event_type == "error" {
        let detail = event
            .get("response")
            .and_then(|r| r.get("error"))
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .or_else(|| {
                event
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
            })
            .unwrap_or(event_type)
            .to_string();
        return Some(Terminal::Failed { detail });
    }
    None
}

/// 逐行读上游响应体的读取器（SSE）。持有它直到读到终止事件或 EOF。
pub(crate) struct UpstreamLines {
    /// 上游响应体行读取器；桥接泵送时会把它取出（同模块外的消费方）。
    pub(crate) reader: Box<dyn BufRead + Send>,
}

impl UpstreamLines {
    pub(crate) fn next_event(&mut self) -> Result<Option<UpstreamEvent>, String> {
        loop {
            let mut line = String::new();
            let read = self
                .reader
                .read_line(&mut line)
                .map_err(|error| format!("读上游流失败：{error}"))?;
            if read == 0 {
                return Ok(None); // EOF
            }
            let trimmed = line.trim_end();
            if let Some(data) = trimmed.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(None);
                }
                if let Ok(value) = serde_json::from_str(data) {
                    return Ok(Some(UpstreamEvent { data: value }));
                }
                // 非 JSON 的 data 行跳过（注释/心跳），不让整条流失败。
            }
        }
    }
}

/// 发起上游调用并返回行读取器（未消费响应体；由调用方读到终止）。
/// 流式读取是传输层的具体能力（`Box<dyn BufRead>`），不进 FlowTransport
/// 抽象——测试的流解析用 `UpstreamLines { reader }` 直接注入。
pub(crate) fn open_upstream(
    url: &str,
    access_token: &str,
    payload: &serde_json::Value,
) -> Result<UpstreamLines, Failure> {
    Ok(UpstreamLines {
        reader: crate::openai::transport::post_stream(url, access_token, &payload.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::catalog::{CatalogEntry, CAPABILITY_REVISION};

    fn catalog() -> crate::openai::catalog::Catalog {
        crate::openai::catalog::Catalog {
            revision: "rev-1".into(),
            capability_revision: CAPABILITY_REVISION,
            fetched_at: 0,
            entries: vec![
                CatalogEntry {
                    id: "gpt-x".into(),
                    name: "GPT X".into(),
                    context_window: Some(128000),
                    efforts: Some(vec!["low".into(), "high".into()]),
                    capability: "verified".into(),
                },
                CatalogEntry {
                    id: "gpt-plain".into(),
                    name: "GPT Plain".into(),
                    context_window: None,
                    efforts: None,
                    capability: "unknown".into(),
                },
            ],
        }
    }

    fn envelope(
        model: &str,
        effort: Option<&str>,
        payload: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "model": model,
            "catalogRevision": format!("rev-1|cap{CAPABILITY_REVISION}"),
            "reasoningEffort": effort,
            "payload": payload,
        })
    }

    #[test]
    fn whitelist_enforced_and_fixed_params_added() {
        let ok = validate_request(
            &envelope(
                "gpt-x",
                Some("low"),
                serde_json::json!({"model": "gpt-x", "input": []}),
            ),
            &catalog(),
        )
        .unwrap();
        assert_eq!(ok.payload["store"], serde_json::json!(false));
        assert_eq!(ok.payload["stream"], serde_json::json!(true));
        assert!(ok.payload.get("input").is_some());

        for banned in ["temperature", "max_output_tokens", "metadata"] {
            let mut payload = serde_json::Map::new();
            payload.insert("model".into(), serde_json::json!("gpt-x"));
            payload.insert(banned.into(), serde_json::json!(1));
            let error = validate_request(
                &envelope("gpt-x", None, serde_json::Value::Object(payload)),
                &catalog(),
            )
            .unwrap_err();
            assert!(
                matches!(error, RequestError::FieldNotAllowed(_)),
                "{banned}"
            );
            assert!(error.message().contains("不会静默忽略"));
        }
        // 调用方自带 store/stream → 拒绝（路线固定参数只由服务端写）。
        for fixed in ["store", "stream"] {
            let mut payload = serde_json::Map::new();
            payload.insert("model".into(), serde_json::json!("gpt-x"));
            payload.insert(fixed.into(), serde_json::json!(true));
            let error = validate_request(
                &envelope("gpt-x", None, serde_json::Value::Object(payload)),
                &catalog(),
            )
            .unwrap_err();
            assert!(matches!(error, RequestError::FieldNotAllowed(_)));
        }
    }

    #[test]
    fn revision_model_and_effort_gates() {
        let stale = serde_json::json!({
            "model": "gpt-x", "catalogRevision": "old|cap0",
            "payload": {"model": "gpt-x"},
        });
        assert!(matches!(
            validate_request(&stale, &catalog()).unwrap_err(),
            RequestError::StaleRevision { .. }
        ));
        let missing = serde_json::json!({
            "model": "gpt-none", "catalogRevision": format!("rev-1|cap{CAPABILITY_REVISION}"),
            "payload": {"model": "gpt-none"},
        });
        assert!(matches!(
            validate_request(&missing, &catalog()).unwrap_err(),
            RequestError::ModelNotInCatalog(_)
        ));
        // 未验证能力的模型：任何显式强度都被拒（不显示未验证档位 = 不接受选择）。
        let error = validate_request(
            &envelope(
                "gpt-plain",
                Some("low"),
                serde_json::json!({"model": "gpt-plain"}),
            ),
            &catalog(),
        )
        .unwrap_err();
        assert!(matches!(error, RequestError::EffortNotAvailable { .. }));
        // 已验证档位之外同样拒绝。
        let error = validate_request(
            &envelope(
                "gpt-x",
                Some("medium"),
                serde_json::json!({"model": "gpt-x"}),
            ),
            &catalog(),
        )
        .unwrap_err();
        assert!(matches!(error, RequestError::EffortNotAvailable { .. }));
        // 「不选」永远合法（模型默认 = 省略字段）。
        assert!(validate_request(
            &envelope("gpt-plain", None, serde_json::json!({"model": "gpt-plain"})),
            &catalog(),
        )
        .is_ok());
    }

    #[test]
    fn terminal_classification_and_stream_reading() {
        assert!(matches!(
            classify_terminal(
                &serde_json::json!({"type": "response.completed", "response": {"id": "r1"}})
            ),
            Some(Terminal::Completed { .. })
        ));
        assert!(matches!(
            classify_terminal(
                &serde_json::json!({"type": "response.failed", "response": {"error": {"message": "quota"}}})
            ),
            Some(Terminal::Failed { .. })
        ));
        assert!(classify_terminal(&serde_json::json!({
            "type": "response.output_item.done",
            "item": {"type": "function_call"}
        }))
        .is_none());
        assert!(
            classify_terminal(&serde_json::json!({"type": "response.output_text.delta"})).is_none()
        );

        // SSE 解析：data 行取 JSON、非 data 行与 [DONE] 跳过。
        let body = "event: x\ndata: {\"type\":\"response.output_text.delta\"}\n\ndata: {\"type\":\"response.completed\"}\ndata: [DONE]\n";
        let mut lines = UpstreamLines {
            reader: Box::new(std::io::Cursor::new(body.as_bytes())),
        };
        let first = lines.next_event().unwrap().unwrap();
        assert_eq!(first.data["type"], "response.output_text.delta");
        let second = lines.next_event().unwrap().unwrap();
        assert!(matches!(
            classify_terminal(&second.data),
            Some(Terminal::Completed { .. })
        ));
        assert!(
            lines.next_event().unwrap().is_none(),
            "[DONE] 与 EOF 都是 None"
        );
    }
}
