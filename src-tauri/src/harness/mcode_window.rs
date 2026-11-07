//! Independent MiniMax workbench window and browser profile.

use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::harness::harness_window::{
    close_owned_popup_windows, close_owned_popups, new_window_response_with, title_from_document,
    PopupOwner, PopupPolicy,
};
use crate::shell::{appearance, paths, window};

fn data_store_identifier(instance_id: &str) -> [u8; 16] {
    let digest = Sha256::digest(format!("dsh-xlink:mcode:{instance_id}").as_bytes());
    let mut identifier = [0u8; 16];
    identifier.copy_from_slice(&digest[..16]);
    identifier
}

fn popup_owner() -> &'static Mutex<Option<Arc<PopupOwner>>> {
    static OWNER: OnceLock<Mutex<Option<Arc<PopupOwner>>>> = OnceLock::new();
    OWNER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn close_popups(app: &AppHandle) -> Result<(), String> {
    let owner = crate::lock(popup_owner()).clone();
    if let Some(owner) = owner {
        close_owned_popups(app, &owner)?;
        *crate::lock(popup_owner()) = None;
    }
    Ok(())
}

/// 关掉本次工作台已打开的授权弹窗，但保留 owner 继续接收后续 `window.open`。
///
/// 登录成功后页面自己不关授权页（上游 0.1.7 的 `onAuthenticated` 只刷新账号
/// 与用量），壳在重载工作台页面收掉登录层之前先把它关掉；重载之后工作台还要
/// 能再开弹窗（终端链接等），所以这里不能用会封创建闸门的 [`close_popups`]。
pub(crate) fn dismiss_popups(app: &AppHandle) -> Result<(), String> {
    match crate::lock(popup_owner()).clone() {
        Some(owner) => close_owned_popup_windows(app, &owner),
        None => Ok(()),
    }
}

/// Opens or focuses the page after the caller has verified its service identity.
pub(crate) fn open(app: &AppHandle, instance_id: &str, port: u16) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(appearance::MCODE_WORKBENCH_LABEL) {
        existing.show().map_err(|error| error.to_string())?;
        existing.unminimize().map_err(|error| error.to_string())?;
        return existing.set_focus().map_err(|error| error.to_string());
    }
    // A previous main window may have closed while its auth popup remained open.
    close_popups(app)?;
    let profile = paths::instance_dir("mcode", instance_id).join("browser");
    std::fs::create_dir_all(&profile)
        .map_err(|error| format!("创建 MiniMax 浏览器档案失败：{error}"))?;
    let url =
        Url::parse(&format!("http://127.0.0.1:{port}/")).map_err(|error| error.to_string())?;
    let handle = app.clone();
    let owner = crate::lock(popup_owner())
        .get_or_insert_with(PopupOwner::new)
        .clone();
    let identifier = data_store_identifier(instance_id);
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("mcode-open-workbench".into())
        .spawn(move || {
            let result = build(&handle, url, profile, identifier, owner);
            let _ = tx.send(result);
        })
        .map_err(|error| format!("创建 MiniMax 工作台线程失败：{error}"))?;
    // shortcut: a stuck WebView blocks stop; use cancellable creation when supported.
    rx.recv()
        .map_err(|error| format!("打开 MiniMax 工作台中断：{error}；请查看日志后重试"))?
}

fn build(
    app: &AppHandle,
    url: Url,
    profile: PathBuf,
    identifier: [u8; 16],
    owner: Arc<PopupOwner>,
) -> Result<(), String> {
    let popup_app = app.clone();
    let policy = PopupPolicy {
        keep_login_in_shell: true,
        profile_dir: Some(profile.clone()),
        data_store_identifier: Some(identifier),
        popup_owner: Some(owner),
        ..PopupPolicy::default()
    };
    let builder = WebviewWindowBuilder::new(
        app,
        appearance::MCODE_WORKBENCH_LABEL,
        WebviewUrl::External(url),
    )
    .title(window::workbench_title("MiniMax 工作台"))
    .inner_size(1280.0, 840.0)
    // WebView2 的 Tauri 拖放处理器会截获网页原生 HTML5 拖放事件。
    .disable_drag_drop_handler()
    .background_color(window::CHROME_BACKDROP)
    .theme(crate::shell::appearance::initial_theme(
        appearance::MCODE_WORKBENCH_LABEL,
    ))
    .data_directory(profile)
    .on_new_window(move |link, features| {
        new_window_response_with(&popup_app, link, features, policy.clone())
    })
    .on_document_title_changed(|window, title| {
        let _ = window.set_title(&title_from_document(
            &title,
            "MiniMax 工作台",
            crate::shell::window::workbench_title,
        ));
    })
    .initialization_script(include_str!("mcode-appearance.js"))
    .initialization_script(include_str!("workbench-appearance.js"))
    .initialization_script(include_str!("context-menu-fallback.js"))
    // MiniMax 登录成功后页面自己不收起登录层，这里补一次重载（见脚本头注释）。
    .initialization_script(include_str!("mcode-login.js"));
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(identifier);
    #[cfg(not(target_os = "macos"))]
    let _ = identifier;
    builder
        .build()
        .map(|_| ())
        .map_err(|error| format!("打开 MiniMax 工作台失败：{error}；请查看日志后重试"))
}

#[cfg(test)]
mod tests {
    use super::data_store_identifier;

    #[test]
    fn browser_store_identifier_is_stable_and_instance_scoped() {
        assert_eq!(
            data_store_identifier("first"),
            data_store_identifier("first")
        );
        assert_ne!(
            data_store_identifier("first"),
            data_store_identifier("second")
        );
    }
}
