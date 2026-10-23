//! 只向插件提供当前站点的账户摘要，不回传用户响应或原始标识。

use super::session_structure::account_contract::identity_path;
#[cfg(test)]
use super::session_structure::account_contract::projection_keys;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::session_structure::{
    account_observed, app_handle, arm_capture_window, capture_with_deadline, profile_dir, spec_for,
    value_at, BodyPick, CaptureTarget, PageRoute,
};

fn identity(site: &str, status: i64, body: &str) -> Value {
    let unknown = || json!({"status": "unknown", "fingerprint": ""});
    let Some((_, path)) = identity_path(site) else {
        return unknown();
    };
    if status != 200 {
        return unknown();
    }
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return unknown();
    };
    if site == "deepseek"
        && (parsed.get("code").and_then(Value::as_i64) != Some(0)
            || parsed.pointer("/data/biz_code").and_then(Value::as_i64) != Some(0))
    {
        return unknown();
    }
    if site == "minimax"
        && ["/statusInfo/code", "/base_resp/status_code"]
            .iter()
            .any(|key| {
                parsed
                    .pointer(key)
                    .is_some_and(|value| value.as_i64() != Some(0))
            })
    {
        return unknown();
    }
    let Some(id) = value_at(&parsed, path).and_then(Value::as_str) else {
        return unknown();
    };
    if id.is_empty() || id.len() > 256 || id.trim() != id || id.chars().any(char::is_control) {
        return unknown();
    }
    let digest = Sha256::digest(format!("official-account-v1\0{site}\0{id}"));
    let fingerprint = crate::shell::digest::hex(&digest[..]);
    json!({"status": "confirmed", "fingerprint": fingerprint})
}

async fn capture(site: &str) -> Result<Value, String> {
    let (path, _) = identity_path(site).ok_or("unsupported-site")?;
    let spec = spec_for(site).ok_or("unsupported-site")?;
    let app = app_handle().ok_or("app-unavailable")?;
    let profile = profile_dir(&app).map_err(|_| "profile-unavailable")?;
    // 与目录、正文窗口分开，当前身份不复用目录或历史快照的缓存。
    let (window, events) = arm_capture_window(&app, site, "account", &profile)
        .await
        .map_err(|failure| format!("window-{}", failure.code()))?;
    let page_route = spec.page_route.map(|route| match route {
        PageRoute::Fetch {
            origin,
            auth,
            root_url,
            ..
        } => PageRoute::Fetch {
            url: path,
            origin,
            auth,
            root_url,
            body_url: None,
        },
        hook => hook,
    });
    let target = CaptureTarget {
        url: spec.root_url.to_string(),
        path_template: path,
        lock: None,
        pick: BodyPick::Latest,
        page_route,
        page_body_url: Some(path.to_string()),
    };
    let captured = capture_with_deadline(&window, events, &target, spec).await;
    let _ = window.close();
    let captured = captured.map_err(|failure| format!("capture-{}", failure.code()))?;
    if captured.status != 200 {
        return Err(format!("http-{}", captured.status));
    }
    Ok(identity(site, captured.status, &captured.body))
}

pub(crate) fn handle_request(body: &[u8]) -> Value {
    let Ok(parsed) = serde_json::from_slice::<Value>(body) else {
        return json!({"ok": false, "error": {"code": "invalid_request"}});
    };
    let valid = parsed.as_object().is_some_and(|object| {
        object.len() == 2 && object.contains_key("command") && object.contains_key("site")
    }) && parsed.get("command").and_then(Value::as_str) == Some("accountIdentity");
    let site = parsed
        .get("site")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid || identity_path(site).is_none() {
        return json!({"ok": false, "error": {"code": "invalid_request"}});
    }
    let (result, stage) = tauri::async_runtime::block_on(async {
        let (result, stage) = match capture(site).await {
            Ok(result) => {
                let stage = if result["status"] == "confirmed" {
                    "confirmed"
                } else {
                    "account-unconfirmed"
                };
                (result, stage.to_string())
            }
            Err(stage) => (json!({"status": "unknown", "fingerprint": ""}), stage),
        };
        let fingerprint = result
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if account_observed(site, fingerprint).await.is_err() {
            return (
                json!({"status": "unknown", "fingerprint": ""}),
                "account-cache-unavailable".to_string(),
            );
        }
        (result, stage)
    });
    crate::shell::shell_events::record(
        "official-chat",
        &format!("account_identity site={site} stage={stage}"),
    );
    json!({"ok": true, "site": site, "identity": result})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_identity_is_scoped_and_never_returns_raw_fields() {
        let a = identity(
            "deepseek",
            200,
            r#"{"code":0,"data":{"biz_code":0,"biz_data":{"id":"account-a","token":"must-not-leave-shell","email":"private@example.test"}}}"#,
        );
        let b = identity(
            "deepseek",
            200,
            r#"{"code":0,"data":{"biz_code":0,"biz_data":{"id":"account-b"}}}"#,
        );
        let gpt = identity("chatgpt", 200, r#"{"user":{"id":"account-a"}}"#);
        assert_eq!(a["status"], "confirmed");
        assert_ne!(a["fingerprint"], b["fingerprint"]);
        assert_ne!(a["fingerprint"], gpt["fingerprint"]);
        assert_eq!(a.as_object().unwrap().len(), 2);
        assert!(!a.to_string().contains("account-a"));
        assert!(!a.to_string().contains("private"));
        assert!(!a.to_string().contains("token"));
        assert_eq!(
            identity(
                "minimax",
                200,
                r#"{"data":{"userInfo":{"realUserID":"account-a"}}}"#
            )["status"],
            "confirmed"
        );
    }

    #[test]
    fn account_unknown_and_untrusted_request_fail_closed() {
        assert!(projection_keys("/api/auth/session")
            .unwrap()
            .iter()
            .all(|key| !key.to_lowercase().contains("token")));
        assert!(projection_keys("/unregistered/path").is_none());
        for (site, status, body) in [
            (
                "deepseek",
                200,
                r#"{"code":40003,"data":{"biz_code":0,"biz_data":{"id":"stale"}}}"#,
            ),
            ("chatgpt", 401, r#"{"user":{"id":"stale"}}"#),
            (
                "chatgpt",
                200,
                r#"{"user":{"id":""},"accessToken":"not-an-id"}"#,
            ),
            ("minimax", 200, r#"{"user_id":"device-is-not-account"}"#),
        ] {
            assert_eq!(identity(site, status, body)["status"], "unknown");
        }
        for body in [
            r#"{"command":"accountIdentity","site":"unknown"}"#,
            r#"{"command":"accountIdentity","site":"deepseek","url":"https://evil.test"}"#,
            r#"{"command":"accountIdentity"}"#,
        ] {
            assert_eq!(handle_request(body.as_bytes())["ok"], false);
        }
    }
}
