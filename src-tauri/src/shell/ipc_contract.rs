//! IPC 契约的 Rust 侧校验（2026-11 OPT-12，仅测试构建）。
//!
//! 单一契约是 `contracts/ipc-contract.json`。这里把四组试点 DTO 真正序列化一次，
//! 与本文件对照：字段名、必需/可空字段、枚举与 top-level 额外字段都会在这里失败。
//! 前端消费同一份 JSON（`ui/src/shell/ipc-schema.js`），E2E 也读同一份文件。

#[cfg(test)]
mod tests {
    use crate::commands::StatusView;
    use crate::kernel::lifecycle::{InstalledVersion, KernelStatus};
    use crate::memory::store::{MemoryListView, MemoryStatus};
    use crate::node::detect::NodeInfo;
    use crate::shell::paths::ShellMode;
    use crate::shell::settings::Settings;
    use crate::usage::local::UsageView;
    use serde_json::{json, Value};

    const CONTRACT: &str = include_str!("../../../contracts/ipc-contract.json");

    fn contract() -> Value {
        serde_json::from_str(CONTRACT).expect("contracts/ipc-contract.json 必须可解析")
    }

    fn response_schema(command: &str) -> Value {
        let root = contract();
        root["commands"][command]["response"].clone()
    }

    fn matches_type(kind: &str, value: &Value) -> bool {
        match kind {
            "null" => value.is_null(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_u64() || value.is_i64() || value.is_f64(),
            "number" => value.is_number(),
            "channel" => true,
            other => panic!("契约用了未实现的类型 {other}"),
        }
    }

    fn assert_schema(schema: &Value, actual: &Value, path: &str) {
        let declared = &schema["type"];
        let types: Vec<&str> = match declared {
            Value::String(s) => vec![s.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => panic!("{path}: 契约缺少 type"),
        };
        assert!(
            types.iter().any(|kind| matches_type(kind, actual)),
            "{path}: 期望 {types:?}，实际 {actual}"
        );
        if let Some(values) = schema["enum"].as_array() {
            let text = actual.as_str().unwrap_or_default();
            assert!(
                values.iter().any(|v| v.as_str() == Some(text)),
                "{path}: 期望枚举 {values:?}，实际 {text}"
            );
        }
        if actual.is_object() {
            let object = actual.as_object().unwrap();
            let properties = schema["properties"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            if let Some(required) = schema["required"].as_array() {
                for key in required.iter().filter_map(Value::as_str) {
                    assert!(object.contains_key(key), "{path}.{key}: 缺少必需字段");
                }
            }
            if schema["additionalProperties"] == json!(false) {
                for key in object.keys() {
                    assert!(
                        properties.contains_key(key),
                        "{path}.{key}: 契约未声明的额外字段（后端 DTO 漂移？）"
                    );
                }
            }
            for (key, child) in &properties {
                if let Some(value) = object.get(key) {
                    assert_schema(child, value, &format!("{path}.{key}"));
                }
            }
        }
        if let (Some(items), Some(array)) = (schema["items"].as_object(), actual.as_array()) {
            for (index, item) in array.iter().enumerate() {
                assert_schema(
                    &Value::Object(items.clone()),
                    item,
                    &format!("{path}[{index}]"),
                );
            }
        }
    }

    fn assert_response(command: &str, value: Value) {
        assert_schema(
            &response_schema(command),
            &value,
            &format!("{command}.response"),
        );
    }

    fn sample_status() -> StatusView {
        StatusView {
            shell_version: "0.0.0-test".into(),
            dev_build: true,
            shell_mode: ShellMode::Dev,
            kernel: KernelStatus {
                installed: vec![InstalledVersion {
                    version: "0.1.0".into(),
                    active: true,
                    size_bytes: 1,
                    shared_storage: false,
                }],
                active: Some("0.1.0".into()),
                active_installed: true,
                running: false,
                port: 3091,
                data_dir: "~/.dsh-xlink/dsh/desktop-dev".into(),
                xlink_home: "~/.dsh-xlink".into(),
                settings_warning: None,
                other_shell_workbench: None,
            },
            node: NodeInfo {
                path: "/usr/bin/node".into(),
                version: Some("v24.0.0".into()),
                ok: true,
                reason: String::new(),
            },
            settings: Settings::default(),
            quarantined: Vec::new(),
            last_incident: None,
            official_chat_open: false,
        }
    }

    fn sample_memory() -> MemoryListView {
        MemoryListView {
            status: MemoryStatus {
                namespace: "dsh/desktop-dev/web".into(),
                dir: "~/.dsh-xlink/dsh/desktop-dev/memory".into(),
                file: "memory.json".into(),
                schema_version: 6,
                generation: 0,
                use_approved: false,
                auto_extract: false,
                accepted: 0,
                disabled: 0,
                pending_candidates: 0,
                max_hits: 6,
                total_bytes: 0,
                warning: None,
                effect_pending: false,
            },
            memories: Vec::new(),
            candidates: Vec::new(),
            handoffs: Vec::new(),
            experiences: Vec::new(),
        }
    }

    fn sample_usage() -> UsageView {
        UsageView {
            retention_days: 90,
            last_scanned_at_ms: 0,
            tracked_files: 0,
            total_tokens: 0,
            total_requests: 0,
            today_tokens: 0,
            today_requests: 0,
            today_models: Vec::new(),
            active_days: 0,
            top_model: None,
            models: Vec::new(),
            days: Vec::new(),
            external_sources: crate::usage::external::ExternalSources::default(),
            external: Vec::new(),
        }
    }

    #[test]
    fn status_dto_matches_contract() {
        assert_response("get_status", serde_json::to_value(sample_status()).unwrap());
    }

    #[test]
    fn start_report_matches_contract() {
        let report = crate::diagnostics::guard::StartReport {
            port: 3090,
            running: false,
            safe_mode: false,
            incident: None,
            warning: None,
            run_id: None,
        };
        assert_response("start_kernel", serde_json::to_value(report).unwrap());
    }

    #[test]
    fn stop_kernel_success_is_null_matching_contract() {
        assert_response("stop_kernel", serde_json::to_value(()).unwrap());
    }

    #[test]
    fn memory_list_dto_matches_contract() {
        assert_response(
            "memory_list",
            serde_json::to_value(sample_memory()).unwrap(),
        );
    }

    #[test]
    fn usage_view_matches_contract() {
        assert_response(
            "get_model_usage",
            serde_json::to_value(sample_usage()).unwrap(),
        );
    }

    #[test]
    fn every_pilot_command_is_in_the_contract() {
        let root = contract();
        for command in [
            "get_status",
            "start_kernel",
            "stop_kernel",
            "memory_list",
            "get_model_usage",
        ] {
            assert!(root["commands"][command].is_object(), "契约缺 {command}");
        }
    }
}
