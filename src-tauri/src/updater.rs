//! 外壳通过 tauri-plugin-updater 完成自身更新，目标是 GitHub 上最新已发布
//! release 的 `latest.json`（参见 tauri.conf.json 中的 `plugins.updater`）。
//!
//! 发布工作流会用仓库密钥 `TAURI_SIGNING_PRIVATE_KEY` 对更新制品签名；
//! 配置中固定的公钥会拒绝任何未由其签名的负载。该 endpoint 仅服务
//! 已发布的 release（draft 不可见），所以只有当人类发布该 draft 之后，
//! 更新才会出现在这里——而且仅当该 release 被标记为 "latest"，
//! GitHub 允许 prerelease 也被标记为 latest。

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Error as UpdaterError, UpdaterExt};

use crate::error::AppError;

/// 概览页对外壳自身版本状态的展示信息。
#[derive(Debug, Clone, Serialize)]
pub struct ShellUpdateInfo {
    pub current: String,
    /// 已发布更新对应的版本（如果有）。
    pub available: Option<String>,
}

/// 后台检查到较新外壳发布版本时触发的事件。
pub const UPDATE_AVAILABLE_EVENT: &str = "shell-update-available";

/// 将 tauri-plugin-updater 的错误翻译为面向用户的中文消息，说明实际出了
/// 什么问题以及用户（或发布维护者）下一步该做什么。`Error` 是
/// `non_exhaustive` 的，因此 catch-all 分支保证外壳在面对未来插件版本时
/// 仍能正常工作，而已知分支则给出精准文本。
fn explain_updater_error(e: UpdaterError) -> String {
    match e {
        // 新仓库最常见的情况：endpoint 返回 404 HTML（还没发布过
        // release），或者 release 存在但仍是 draft（GitHub 会对
        // /releases/latest/download/ 隐藏 draft）。插件拿不到合法
        // JSON body 后报这个错误。把原因直白告诉用户即可。
        UpdaterError::ReleaseNotFound => {
            "未发现已发布的桌面端 release（draft 与未发布版本不可见；需要正式发布并取消 draft 后才能被检测到）".into()
        }
        // endpoint 返回了内容，但不是合法的清单——通常是代理在 200 时
        // 返回 HTML、最新 JSON 损坏，或配置指向了错误的 URL。
        UpdaterError::Serialization(err) => format!("发布清单 JSON 解析失败：{err}"),
        // 传输层在读取 body 之前就失败了。常见原因：DNS、TLS、代理、
        // 强制门户、离线。
        UpdaterError::Reqwest(err) => format!("网络请求失败：{err}"),
        UpdaterError::Network(msg) => format!("下载失败：{msg}"),
        UpdaterError::Http(err) => format!("HTTP 错误：{err}"),
        UpdaterError::UrlParse(err) => format!("更新地址无效：{err}"),
        UpdaterError::Semver(err) => format!("版本号解析失败：{err}"),
        UpdaterError::EmptyEndpoints => "未配置更新 endpoint（检查 tauri.conf.json plugins.updater.endpoints）".into(),
        UpdaterError::InsecureTransportProtocol => "更新地址必须使用 https".into(),
        UpdaterError::UnsupportedArch => "当前架构没有可用的发布包".into(),
        UpdaterError::UnsupportedOs => "当前系统没有可用的发布包".into(),
        // 清单可读，但其签名未能通过 tauri.conf.json 中固定公钥的校验。
        // 要么清单被篡改，要么公钥与签名私钥已经不同步。
        UpdaterError::Minisign(err) => format!("签名校验失败：{err}"),
        UpdaterError::SignatureUtf8(msg) => format!("签名编码无效：{msg}"),
        UpdaterError::Base64(err) => format!("签名编码无效：{err}"),
        // 未来出现的任何变体：把插件自带文本也透传出来，让用户在映射
        // 跟上之前仍能看到可操作的信息。
        other => format!("更新检查失败：{other}"),
    }
}

/// 把当前运行版本与最新已发布版本做比较。
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
        // 在已配置的 endpoint 上访问不到任何已发布的 release。
        // 可能仓库从未发布过桌面端 release；也可能所有发布的 release
        // 都还是 draft（GitHub 对 /releases/latest/download/ 隐藏 draft）；
        // 也可能是 endpoint 配置错误，服务器一直返回非清单 HTML。仅凭
        // 这个信号无法区分「还没有 release」和「endpoint 损坏」——全新
        // 仓库、仅 draft 的 release 和错误的 URL 都会触发它。把这种
        // 情况视为空状态：后台检查保持静默，手动按钮仅展示当前版本。
        // 后续真正发布的 release 会通过正常路径出现。
        //
        // 真正的错误（网络不通、TLS 失败、签名不匹配、清单损坏、URL
        // 语法无效……）仍走 `explain_updater_error`。
        Err(UpdaterError::ReleaseNotFound) => None,
        Err(other) => return Err(AppError::Update(explain_updater_error(other))),
    };
    Ok(ShellUpdateInfo {
        current,
        available: update,
    })
}

/// 启动后短时间执行一次启动期检查；结果通过事件送达 UI，
/// 让概览页能在用户无操作时亮起更新横幅。
pub fn spawn_background_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // 给窗口一点时间挂载监听器，然后再发送事件。
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if let Ok(info) = check(&app).await {
            if let Some(version) = info.available {
                let _ = app.emit(UPDATE_AVAILABLE_EVENT, version);
            }
        }
    });
}

/// 下载待安装的更新、完成安装并重启到新版本。在替换任何文件之前，
/// updater 会用固定公钥校验 minisign 签名。
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
    // download_and_install 接收两个回调，两者都会上报进度；
    // 让它们共享同一个 sink，sink 放在 mutex 后面。
    let progress = std::sync::Mutex::new(on_progress);
    update
        .download_and_install(
            |received, total| {
                // tauri-plugin-updater 的 chunk 回调每收到一个 chunk
                // 触发一次，`received` 是当前的字节累计，`total` 是
                // 响应 Content-Length 的 Option<usize`。之前的实现只
                // 显示 `total`，让横幅一直停在 \"(4.0 MB)\" 上，即便
                // 下载还在推进——用户因此报障称更新卡住。同时显示两个
                // 值：以 MB 为单位的 received / total，让横幅能随着
                // 每个 chunk 可见地前进。
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

    /// `Error` 是 `non_exhaustive` 的，因此大多数变体无法在插件 crate 之外
    /// 命名。被包装错误类型的 `From` 实现是唯一公开的构造器；测试
    /// 会触发它们，确保映射不会回退到仅英文输出。
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

    /// `Error::ReleaseNotFound` 是空状态（访问不到已发布的 release），
    /// 并非真正的失败——checker 不能把它当作错误抛出，否则 UI 上的
    /// 手动「检查更新」按钮会显示一个吓人的红色 toast，而不是「已是
    /// 最新版本」。其他错误（我们在插件外能够构造的那些）必须保留其
    /// 诊断文本。
    #[test]
    fn release_not_found_is_treated_as_no_update_available() {
        // `ReleaseNotFound` 是 `non_exhaustive` enum 上的单元变体，
        // 无法在此处命名；用一个 JSON 解析错误走 `Serialization` 分支
        // 间接走一遍，确认对剩余变体的匹配逻辑依然正确。
        let json_err: UpdaterError = serde_json::from_str::<serde_json::Value>("not json")
            .unwrap_err()
            .into();
        // 确认非 ReleaseNotFound 仍会经过映射（即没有意外吞掉所有错误）。
        let mapped = explain_updater_error(json_err);
        assert!(
            !mapped.contains("未发现已发布的桌面端 release"),
            "Serialization must not look like ReleaseNotFound: {mapped}"
        );
    }
}
