//! 用户明确启用后的本机 Codex 额度来源。只读 auth.json，不刷新、不复制令牌。
//! 独立于模型授权；许可按壳模式持久化，账号不一致时在请求前拒绝。

use std::io::Read;
use std::path::PathBuf;

use super::subscription_openai::{self, OpenAiUsage};
use crate::openai::vault::AccountTokens;
use crate::shell::{error::AppError, paths, settings, state};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Consent {
    enabled: bool,
}

fn consent_path() -> PathBuf {
    paths::shell_dir(settings::current_mode()).join("codex-usage-consent.json")
}

fn consent_ctx() -> state::StateCtx {
    state::StateCtx {
        corrupt: |_| {
            "Codex 额度查询许可文件无法读取，已停止读取 Codex 凭据。请重新设置额度查询开关；许可文件位于 shell/<mode>/codex-usage-consent.json".into()
        },
        kind: AppError::Subscription,
    }
}

pub(crate) fn enabled() -> Result<bool, String> {
    state::load_checked::<Consent>(&consent_path(), consent_ctx())
        .map(|consent| consent.enabled)
        .map_err(|e| e.to_string())
}

fn save_enabled(enabled: bool) -> Result<(), String> {
    state::save(&consent_path(), &Consent { enabled }, consent_ctx()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_codex_usage_enabled(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri::Emitter;
    crate::commands::blocking(move || -> Result<bool, String> {
        let _guard = super::subscription::FETCH_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        save_enabled(enabled)?;
        if let Err(error) = app.emit("codex-usage-consent-changed", enabled) {
            crate::shell::shell_events::record(
                "codex-usage-consent",
                &format!("许可已保存，跨窗通知失败：{error}"),
            );
        }
        Ok(enabled)
    })
    .await
}

/// 遵循 Codex 的 CODEX_HOME；未设置时使用用户 home/.codex，不搜索其它目录。
fn auth_path() -> Result<PathBuf, String> {
    let root = if let Some(root) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(root)
    } else {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(|home| PathBuf::from(home).join(".codex"))
            .ok_or_else(|| "无法定位本机 Codex 登录目录；请设置绝对路径 CODEX_HOME".to_string())?
    };
    if !root.is_absolute() {
        return Err("CODEX_HOME 必须是绝对路径，请修正后刷新".into());
    }
    Ok(root.join("auth.json"))
}

pub(crate) fn active_usage() -> Result<Option<OpenAiUsage>, String> {
    let account = subscription_openai::active_account()?
        .ok_or_else(|| "请先在本壳登录 OpenAI，才能校验 Codex 登录是否为同一账号".to_string())?;
    let path = auth_path()?;
    let file = std::fs::File::open(&path)
        .map_err(|_| format!("无法读取本机 Codex 登录文件（{}）。请先在 Codex 中使用 ChatGPT 登录；仅支持文件存储的凭据", path.display()))?;
    let mut text = String::new();
    file.take(1_048_577)
        .read_to_string(&mut text)
        .map_err(|_| "本机 Codex 登录文件读取失败，请检查文件权限后刷新".to_string())?;
    if text.len() > 1_048_576 {
        return Err("Codex 登录文件过大，已拒绝读取；请在 Codex 中重新登录".into());
    }
    parse_auth(
        &text,
        &account,
        crate::shell::process::epoch_millis() / 1000,
    )
    .map(Some)
}

// 不声明 refresh_token 字段：没有任何刷新 / 写回通道。
#[derive(Deserialize)]
struct AuthFile {
    #[serde(default)]
    auth_mode: Option<String>,
    tokens: CodexTokens,
}
#[derive(Deserialize)]
struct CodexTokens {
    access_token: String,
    id_token: String,
    account_id: String,
}

fn claims(token: &str) -> Option<serde_json::Value> {
    use base64::Engine;
    if !token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return None;
    }
    let mut parts = token.split('.');
    if parts.next()?.is_empty() {
        return None;
    }
    let payload = parts.next()?;
    if parts.next()?.is_empty() {
        return None;
    }
    if parts.next().is_some() {
        return None;
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// 本地可信文件中的声明只用于防止误用别的账号；最终授权仍由第一方接口校验。
fn parse_auth(text: &str, dsh: &AccountTokens, now_secs: u64) -> Result<OpenAiUsage, String> {
    let auth: AuthFile = serde_json::from_str(text)
        .map_err(|_| "Codex 登录文件缺少 ChatGPT 凭据或格式不正确。请在 Codex 中使用 ChatGPT 登录，不是 API Key 登录".to_string())?;
    if auth
        .auth_mode
        .as_deref()
        .is_some_and(|mode| mode != "chatgpt")
    {
        return Err("本机 Codex 当前不是 ChatGPT 登录模式，请在 Codex 中切换登录方式后刷新".into());
    }
    let tokens = auth.tokens;
    let id = claims(&tokens.id_token)
        .ok_or_else(|| "Codex 登录缺少可校验的账号信息，请在 Codex 中重新登录".to_string())?;
    let email = id
        .get("email")
        .and_then(serde_json::Value::as_str)
        .filter(|e| !e.trim().is_empty());
    if dsh.reauth_required
        || id
            .get("email_verified")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || !dsh
            .email
            .as_deref()
            .zip(email)
            .is_some_and(|(a, b)| !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim()))
    {
        return Err("Codex 与本壳 OpenAI 登录账号不一致，或无法确认账号身份；已停止查询。请在两边登录同一账号后刷新".into());
    }
    let access = claims(&tokens.access_token)
        .ok_or_else(|| "Codex 访问令牌格式不可识别，请在 Codex 中重新登录".to_string())?;
    if access
        .get("exp")
        .and_then(serde_json::Value::as_u64)
        .map_or(true, |exp| exp <= now_secs)
    {
        return Err("Codex 访问令牌已过期或缺少有效期。请先在 Codex 中刷新登录，再回来刷新额度；壳不会替 Codex 刷新令牌".into());
    }
    if tokens.account_id.is_empty()
        || !tokens
            .account_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || id
            .get("sub")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map_or(true, |sub| {
                Some(sub) != access.get("sub").and_then(serde_json::Value::as_str)
            })
        || crate::openai::auth::chatgpt_account_id(&id).as_deref()
            != Some(tokens.account_id.as_str())
        || dsh
            .chatgpt_account_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .is_some_and(|id| id != tokens.account_id)
        || crate::openai::auth::chatgpt_account_id(&access).as_deref()
            != Some(tokens.account_id.as_str())
    {
        return Err(
            "Codex 登录的账号上下文不一致，已停止查询；请确认使用同一 ChatGPT 账号及工作区后刷新"
                .into(),
        );
    }
    Ok(OpenAiUsage {
        access_token: tokens.access_token,
        account_id: Some(tokens.account_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::vault::AccountTokens;
    use serde_json::json;

    fn token(claims: serde_json::Value) -> String {
        use base64::Engine;
        format!(
            "h.{}.s",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    fn dsh() -> AccountTokens {
        AccountTokens {
            sub: "siwc-sub".into(),
            email: Some("person@example.com".into()),
            client_id: "siwc-client".into(),
            access_token: "dsh-token".into(),
            refresh_token: "dsh-refresh".into(),
            id_token: String::new(),
            scopes: vec![],
            access_expires_at: 200,
            chatgpt_account_id: None,
            reauth_required: false,
        }
    }

    fn auth(email: &str, exp: u64) -> String {
        json!({"tokens": {
            "access_token": token(json!({"sub":"codex-sub","exp":exp,"https://api.openai.com/auth":{"chatgpt_account_id":"account"}})),
            "id_token": token(json!({"sub":"codex-sub","email":email,"email_verified":true,"https://api.openai.com/auth":{"chatgpt_account_id":"account"}})),
            "account_id":"account", "refresh_token":"never-read"
        }}).to_string()
    }

    #[test]
    fn reads_matching_codex_login_without_requiring_siwc_account_id() {
        let usage = parse_auth(&auth("PERSON@example.com", 200), &dsh(), 100).unwrap();
        assert_eq!(usage.account_id.as_deref(), Some("account"));
        assert_ne!(usage.access_token, "dsh-token");
        assert_ne!(usage.access_token, "never-read");
    }

    #[test]
    fn rejects_wrong_identity_expiry_and_key_login_without_exposing_secrets() {
        for text in [
            auth("other@example.com", 200),
            auth("person@example.com", 100),
            json!({"OPENAI_API_KEY":"secret-api-key"}).to_string(),
            "secret-invalid-json".into(),
        ] {
            let error = parse_auth(&text, &dsh(), 100).err().expect("必须拒绝");
            assert!(!error.contains("secret"));
            assert!(!error.contains("other@example.com"));
            assert!(!error.contains("never-read"));
        }
        let mut account = dsh();
        account.chatgpt_account_id = Some("other-account".into());
        assert!(parse_auth(&auth("person@example.com", 200), &account, 100).is_err());
        account.chatgpt_account_id = None;
        account.email = None;
        assert!(parse_auth(&auth("person@example.com", 200), &account, 100).is_err());
    }

    #[test]
    fn rejects_mixed_codex_tokens_and_unverified_email() {
        for bad_id in [
            serde_json::json!({"sub":"other","email":"person@example.com","email_verified":true}),
            serde_json::json!({"sub":"codex-sub","email":"person@example.com","email_verified":false}),
        ] {
            let mut file: serde_json::Value =
                serde_json::from_str(&auth("person@example.com", 200)).unwrap();
            file["tokens"]["id_token"] = token(bad_id).into();
            assert!(parse_auth(&file.to_string(), &dsh(), 100).is_err());
        }
        let mut file: serde_json::Value =
            serde_json::from_str(&auth("person@example.com", 200)).unwrap();
        file["auth_mode"] = "api_key".into();
        assert!(
            parse_auth(&file.to_string(), &dsh(), 100).is_err(),
            "API Key 模式下不使用残留 ChatGPT 令牌"
        );
    }

    #[test]
    fn consent_is_default_off_persisted_and_scoped_to_shell_mode() {
        let home = std::env::temp_dir().join(format!(
            "codex-consent-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        let _guard = crate::tests::scoped_xlink_home(&home);
        assert!(!enabled().unwrap());
        save_enabled(true).unwrap();
        assert!(enabled().unwrap());
        let other = if crate::shell::settings::current_mode() == crate::shell::paths::ShellMode::Dev
        {
            crate::shell::paths::ShellMode::Release
        } else {
            crate::shell::paths::ShellMode::Dev
        };
        assert!(!crate::shell::paths::shell_dir(other)
            .join("codex-usage-consent.json")
            .exists());
        save_enabled(false).unwrap();
        assert!(!enabled().unwrap());
        std::fs::write(consent_path(), "invalid-consent").unwrap();
        assert!(enabled().is_err(), "损坏许可必须拒绝，而非默认为已同意");
        std::fs::remove_dir_all(&home).unwrap();
    }
}
