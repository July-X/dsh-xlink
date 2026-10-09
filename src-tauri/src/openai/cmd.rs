//! openai-oauth 的 Tauri 命令层（开发计划 §4.2）。
//!
//! 四条命令围绕**授权状态机**（§4.1 的授权四态子集）：
//! `openai_account_status`（只读）、`openai_authorize_start`（后台线程跑
//! 完整授权流，立即返回；浏览器由 opener 插件打开）、
//! `openai_authorize_cancel`（置取消旗，等待循环的 1 秒切片内生效）、
//! `openai_logout`（清库）。授权流结束（成功/失败/取消）广播
//! `openai-account-changed`，UI 靠它刷新账户卡——不做轮询。
//!
//! 与 flow.rs 的分工：那边是可注入的纯编排（模拟授权服务器可测），这边
//! 只做「解析路径、组装生产环境、处理 AppHandle」；凡是能落在 flow 里的
//! 逻辑都不在这里长。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::openai::flow::{self, FlowPaths, ProductionTransport};
use crate::shell::settings;

/// 授权流程结束时的广播事件名（UI 侧订阅刷新账户卡）。
pub(crate) const ACCOUNT_EVENT: &str = "openai-account-changed";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    /// `signed-out` / `authorizing` / `authorized`（§4.1 授权四态的 P2 子集；
    /// `reauth-required` 随刷新链落地）。
    pub phase: String,
    /// 已登录账号的脱敏邮箱（`a***@example.com`）；未登录为 `None`。
    pub email: Option<String>,
    /// 后台授权流的最近一次失败原因（成功后清空）。
    pub last_error: Option<String>,
}

struct Session {
    cancel: Arc<AtomicBool>,
}

fn session() -> &'static Mutex<Option<Session>> {
    static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(None))
}

fn last_error() -> &'static Mutex<Option<String>> {
    static LAST_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    LAST_ERROR.get_or_init(|| Mutex::new(None))
}

/// 邮箱脱敏：本地部分留首字符，其余打码（设计 §3.2「当前账户」只显示
/// 脱敏描述——完整身份只活在 vault 里）。
pub(crate) fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() => {
            let keep = local.chars().next().unwrap_or('*');
            format!("{keep}***@{domain}")
        }
        _ => "***".to_string(),
    }
}

fn flow_paths() -> FlowPaths {
    flow::shell_flow_paths()
}

fn status_payload(app: &AppHandle) -> AccountStatus {
    let paths = flow_paths();
    let transport = production_transport(app);
    let mode = settings::current_mode().as_str().to_string();
    let account = flow::account_view(&paths, &transport, &mode).ok().flatten();
    let authorizing = session()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_some();
    let error = last_error()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    AccountStatus {
        phase: if authorizing {
            "authorizing".into()
        } else if account.as_ref().is_some_and(|view| view.reauth_required) {
            "reauth-required".into()
        } else if account.is_some() {
            "authorized".into()
        } else {
            "signed-out".into()
        },
        email: account.and_then(|view| view.email.map(|email| mask_email(&email))),
        last_error: if authorizing { None } else { error },
    }
}

fn production_transport(app: &AppHandle) -> ProductionTransport {
    let handle = app.clone();
    ProductionTransport {
        open_browser: Some(Box::new(move |url: &str| {
            use tauri_plugin_opener::OpenerExt;
            handle
                .opener()
                .open_url(url.to_string(), None::<&str>)
                .map_err(|error| format!("系统浏览器打开失败：{error}"))
        })),
    }
}

fn emit_account_changed(app: &AppHandle) {
    let _ = app.emit(ACCOUNT_EVENT, status_payload(app));
}

/// 当前账号状态（只读；不打快照、不写文件）。
#[tauri::command]
pub async fn openai_account_status(app: AppHandle) -> Result<AccountStatus, String> {
    crate::commands::blocking(move || Ok::<_, String>(status_payload(&app))).await
}

/// 开始登录（后台线程跑完整授权流；命令立即返回，结束广播事件）。
///
/// 已有登录流程在进行时拒绝再次开始——双开两个回调监听除了把用户搞糊涂
/// 没有任何收益。
#[tauri::command]
pub async fn openai_authorize_start(app: AppHandle) -> Result<AccountStatus, String> {
    let mut guard = session()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        return Err("登录已在进行中；请先完成或取消当前登录".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *guard = Some(Session {
        cancel: Arc::clone(&cancel),
    });
    drop(guard);

    let handle = app.clone();
    let cancel_for_thread = Arc::clone(&cancel);
    std::thread::Builder::new()
        .name("oop-authorize".into())
        .spawn(move || {
            let paths = flow_paths();
            let transport = production_transport(&handle);
            let mode = settings::current_mode().as_str().to_string();
            let result = flow::run_authorize(&paths, &transport, &mode, &cancel_for_thread);
            *last_error()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = result.err();
            *session()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            emit_account_changed(&handle);
        })
        .map_err(|error| format!("启动登录线程失败：{error}"))?;
    Ok(status_payload(&app))
}

/// 取消登录（等待循环 1 秒切片内生效）。
#[tauri::command]
pub async fn openai_authorize_cancel(app: AppHandle) -> Result<AccountStatus, String> {
    let guard = session()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(session) = guard.as_ref() {
        session.cancel.store(true, Ordering::SeqCst);
    }
    Ok(status_payload(&app))
}

/// 退出登录（清活跃账号与令牌；远端吊销未确认时文案如实说明）。
#[tauri::command]
pub async fn openai_logout(app: AppHandle) -> Result<AccountStatus, String> {
    crate::commands::blocking(move || {
        let paths = flow_paths();
        let transport = production_transport(&app);
        let mode = settings::current_mode().as_str().to_string();
        match flow::run_logout(&paths, &transport, &mode) {
            Ok(true) => {
                emit_account_changed(&app);
                Ok(status_payload(&app))
            }
            Ok(false) => Ok(status_payload(&app)),
            Err(error) => {
                // 退出失败不广播：状态未变。
                Err(format!("退出失败：{error}；可重试；若持续失败请查看日志"))
            }
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_masking_keeps_first_char_only() {
        assert_eq!(mask_email("someone@example.com"), "s***@example.com");
        assert_eq!(mask_email("a@b.co"), "a***@b.co");
        assert_eq!(mask_email("broken"), "***");
        assert_eq!(mask_email(""), "***");
    }

    /// vault 路径与分键都落在 shell/<mode>/openai-oauth/（开发计划 §3）。
    #[test]
    fn flow_paths_live_under_shell_mode_dir() {
        let paths = flow_paths();
        let text = paths.vault_file.to_string_lossy();
        assert!(text.contains("openai-oauth"), "{text}");
        assert!(text.contains("accounts.bin"), "{text}");
        assert_eq!(paths.issuer_base, crate::openai::auth::SIWC_ISSUER);
    }
}
