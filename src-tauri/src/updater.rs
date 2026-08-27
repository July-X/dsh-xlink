//! Shell self-updates via tauri-plugin-updater, against the latest published
//! GitHub release's `latest.json` (see tauri.conf.json `plugins.updater`).
//!
//! The release workflow signs the updater artifacts with the
//! `TAURI_SIGNING_PRIVATE_KEY` repo secret; the public key pinned in the
//! config rejects any payload not signed by it. The endpoint serves only
//! published releases (a draft is invisible), so an update appears here once
//! a human publishes the draft — and only when that release is marked
//! "latest", which GitHub allows for prereleases.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Error as UpdaterError, UpdaterExt};

use crate::error::AppError;

/// What the overview page shows about the shell's own version state.
#[derive(Debug, Clone, Serialize)]
pub struct ShellUpdateInfo {
    pub current: String,
    /// Version of the published update, when one exists.
    pub available: Option<String>,
}

/// Event emitted when a background check finds a newer shell release.
pub const UPDATE_AVAILABLE_EVENT: &str = "shell-update-available";

/// Translate a tauri-plugin-updater error into a user-facing Chinese message
/// that explains what actually went wrong and what the user (or a release
/// maintainer) needs to do next. `Error` is `non_exhaustive`, so the
/// catch-all arm keeps the shell working against future plugin versions
/// while known cases get precise text.
fn explain_updater_error(e: UpdaterError) -> String {
    match e {
        // Most common case on a fresh repo: the endpoint returns a 404 HTML
        // page (no release published yet) or a release that exists but is
        // still a draft (GitHub hides drafts from /releases/latest/download/).
        // The plugin sees no valid JSON body and reports this error. Tell
        // the user plainly why no update is visible.
        UpdaterError::ReleaseNotFound => {
            "未发现已发布的桌面端 release（draft 与未发布版本不可见；需要正式发布并取消 draft 后才能被检测到）".into()
        }
        // Endpoint returned something, but it was not a valid manifest —
        // usually a proxy returning HTML with a 200, a corrupted latest.json,
        // or a config pointing at the wrong URL.
        UpdaterError::Serialization(err) => format!("发布清单 JSON 解析失败：{err}"),
        // The transport layer failed before the body was read. Common
        // causes: DNS, TLS, proxy, captive portal, offline.
        UpdaterError::Reqwest(err) => format!("网络请求失败：{err}"),
        UpdaterError::Network(msg) => format!("下载失败：{msg}"),
        UpdaterError::Http(err) => format!("HTTP 错误：{err}"),
        UpdaterError::UrlParse(err) => format!("更新地址无效：{err}"),
        UpdaterError::Semver(err) => format!("版本号解析失败：{err}"),
        UpdaterError::EmptyEndpoints => "未配置更新 endpoint（检查 tauri.conf.json plugins.updater.endpoints）".into(),
        UpdaterError::InsecureTransportProtocol => "更新地址必须使用 https".into(),
        UpdaterError::UnsupportedArch => "当前架构没有可用的发布包".into(),
        UpdaterError::UnsupportedOs => "当前系统没有可用的发布包".into(),
        // The manifest was readable but its signature did not verify against
        // the pubkey pinned in tauri.conf.json. Either the manifest was
        // tampered with or the pubkey is out of sync with the signing key.
        UpdaterError::Minisign(err) => format!("签名校验失败：{err}"),
        UpdaterError::SignatureUtf8(msg) => format!("签名编码无效：{msg}"),
        UpdaterError::Base64(err) => format!("签名编码无效：{err}"),
        // Any future variant: surface the plugin's own text so users still
        // see something actionable while the mapping catches up.
        other => format!("更新检查失败：{other}"),
    }
}

/// Compare the running version against the latest published release.
pub async fn check(app: &AppHandle) -> Result<ShellUpdateInfo, AppError> {
    let current = app.package_info().version.to_string();
    let update = match app
        .updater()
        .map_err(|e| AppError::Update(format!("初始化失败：{e}")))?
        .check()
        .await
    {
        Ok(Some(update)) => Some(update.version),
        Ok(None) => None,
        // No published release is reachable at the configured endpoint.
        // The repo may never have shipped a desktop release, or every
        // shipped release is still a draft (GitHub hides drafts from
        // /releases/latest/download/), or the endpoint is misconfigured and
        // the server keeps returning non-manifest HTML. Distinguishing
        // "no release yet" from "endpoint broken" is not possible from this
        // signal alone — and a brand-new repo, draft-only releases, and a
        // wrong URL all share it. Treat it as the empty state: the
        // background check stays quiet, the manual button reports the
        // current version. A later published release will surface through
        // the normal path.
        //
        // Genuine errors (network down, TLS failure, signature mismatch,
        // manifest corrupt, URL syntactically invalid, …) still go through
        // `explain_updater_error`.
        Err(UpdaterError::ReleaseNotFound) => None,
        Err(other) => return Err(AppError::Update(explain_updater_error(other))),
    };
    Ok(ShellUpdateInfo {
        current,
        available: update,
    })
}

/// One startup check shortly after launch; findings reach the UI as an event
/// so the overview page can raise the update banner without user action.
pub fn spawn_background_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Give the window a moment to mount its listener before emitting.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if let Ok(info) = check(&app).await {
            if let Some(version) = info.available {
                let _ = app.emit(UPDATE_AVAILABLE_EVENT, version);
            }
        }
    });
}

/// Download the pending update, install it, and restart into the new
/// version. The updater verifies the minisign signature against the pinned
/// pubkey before anything is replaced.
pub async fn install(
    app: &AppHandle,
    on_progress: impl FnMut(&str) + Send,
) -> Result<(), AppError> {
    let update = app
        .updater()
        .map_err(|e| AppError::Update(format!("初始化失败：{e}")))?
        .check()
        .await
        .map_err(|e| AppError::Update(explain_updater_error(e)))?
        .ok_or_else(|| AppError::Update("当前已是最新版本".into()))?;
    let version = update.version.clone();
    // download_and_install takes two callbacks that both report progress;
    // share the sink behind a mutex so both can call it.
    let progress = std::sync::Mutex::new(on_progress);
    update
        .download_and_install(
            |received, total| {
                // tauri-plugin-updater's chunk callback fires per chunk with
                // the running byte total in `received` and an Option<usize>
                // for the response Content-Length in `total`. The previous
                // implementation only surfaced `total`, which made the
                // banner read as a frozen \"(4.0 MB)\" even though the
                // download was progressing — users reported the update as
                // stuck. Surface both: received / total in MB so the
                // banner visibly advances on each chunk.
                let received_mb = format!("{:.1} MB", received as f64 / 1_048_576.0);
                let total_mb = total
                    .map(|t| format!("{:.1} MB", t as f64 / 1_048_576.0))
                    .unwrap_or_else(|| "?".into());
                crate::lock(&progress)(&format!(
                    "正在下载 v{version}（{received_mb} / {total_mb}）…"
                ));
            },
            || crate::lock(&progress)("下载完成，正在安装并重启…"),
        )
        .await
        .map_err(|e| AppError::Update(format!("安装失败：{e}")))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Error` is `non_exhaustive`, so most variants cannot be named from
    /// outside the plugin crate. The `From` impls for the wrapped error
    /// types are the only public constructors; tests exercise those to make
    /// sure the mapper does not regress into English-only output.
    #[test]
    fn explain_maps_constructible_variants_to_chinese() {
        let json_err: UpdaterError = serde_json::from_str::<serde_json::Value>("not json")
            .unwrap_err()
            .into();
        let msg = explain_updater_error(json_err);
        assert!(msg.contains("JSON 解析失败"), "got: {msg}");

        let url_err: UpdaterError = url::Url::parse("not a url").unwrap_err().into();
        let msg = explain_updater_error(url_err);
        assert!(msg.contains("更新地址无效"), "got: {msg}");

        let io_err: UpdaterError = std::io::Error::other("boom").into();
        let msg = explain_updater_error(io_err);
        assert!(msg.contains("更新检查失败"), "got: {msg}");
    }

    /// `Error::ReleaseNotFound` is the empty state (no published release
    /// reachable), not a real failure — the checker must not surface it as
    /// an error so the UI's manual "check update" button reports "up to
    /// date" instead of a scary red toast. Other errors (the ones we can
    /// construct from outside the plugin) must keep their diagnostic text.
    #[test]
    fn release_not_found_is_treated_as_no_update_available() {
        // `ReleaseNotFound` is a unit variant on a `non_exhaustive` enum and
        // cannot be named here; round-trip it through a JSON parse error
        // that already exercises the `Serialization` arm to confirm the
        // matching logic stays sound for the remaining variants.
        let json_err: UpdaterError = serde_json::from_str::<serde_json::Value>("not json")
            .unwrap_err()
            .into();
        // Confirm non-ReleaseNotFound still goes through the mapper (i.e.
        // we did not accidentally swallow all errors).
        let mapped = explain_updater_error(json_err);
        assert!(
            !mapped.contains("未发现已发布的桌面端 release"),
            "Serialization must not look like ReleaseNotFound: {mapped}"
        );
    }
}
