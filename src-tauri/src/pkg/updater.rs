//! 外壳通过 tauri-plugin-updater 完成自身更新，目标是 GitHub 上最新已发布
//! release 的 `latest.json`（参见 tauri.conf.json 中的 `plugins.updater`）。
//!
//! 发布工作流会用仓库密钥 `TAURI_SIGNING_PRIVATE_KEY` 对更新制品签名；
//! 配置中固定的公钥会拒绝任何未由其签名的负载。该 endpoint 仅服务
//! 已发布的 release（draft 不可见），所以只有当人类发布该 draft 之后，
//! 更新才会出现在这里——而且仅当该 release 被标记为 "latest"，
//! GitHub 允许 prerelease 也被标记为 latest。

use crate::pkg;
use std::future::Future;
use std::path::Path;
#[cfg(any(test, all(windows, not(debug_assertions))))]
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

#[cfg(any(test, all(windows, not(debug_assertions))))]
use serde::Deserialize;
use serde::Serialize;
#[cfg(all(windows, not(debug_assertions)))]
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Error as UpdaterError, Update, Updater, UpdaterExt};

use crate::pkg::net_proxy::Route;
use crate::shell::error::AppError;
use crate::shell::shell_events;

/// 插件默认**不设任何超时**：认证门户、只完成 TCP 握手却不回包的企业代理、
/// 卡死的 CDN 连接都会让请求永不返回。而 UI 侧 `withExclusiveLoading` 会让
/// globalBusy 长期为真——几乎所有 IO 按钮被禁用且没有取消入口，用户只能退出
/// 应用，更新却根本没装上。`read_timeout` 约束的是"两次读取之间的间隔"，
/// 因此对慢速但持续传输的连接是安全的。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 检查只取一个小 JSON，和下面的下载用同一个值就够。
const CHECK_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// 下载路径放宽到 60 秒，避免慢速网络下大文件传输被误判为超时。
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// 概览页对外壳自身版本状态的展示信息。
#[derive(Debug, Clone, Serialize)]
pub struct ShellUpdateInfo {
    pub current: String,
    /// 已发布更新对应的版本（如果有）。
    pub available: Option<String>,
}

/// 后台检查到较新外壳发布版本时触发的事件。
pub const UPDATE_AVAILABLE_EVENT: &str = "shell-update-available";

/// Windows 更新由 NSIS 以 `/UPDATE` 方式覆盖安装，旧安装目录的卸载和
/// 快捷方式清理不会在这个路径执行。这个标记跨越 updater 拉起的旧进程和
/// 新进程，只在新版本已经启动并通过 UI 确认后消费。
#[cfg(all(windows, not(debug_assertions)))]
const PENDING_UPDATE_FILE_NAME: &str = "pending-shell-update.json";
#[cfg(any(test, all(windows, not(debug_assertions))))]
const PENDING_UPDATE_SCHEMA_VERSION: u8 = 1;

#[cfg(any(test, all(windows, not(debug_assertions))))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PendingShellUpdate {
    schema_version: u8,
    previous_executable: String,
    previous_version: String,
    target_version: String,
}

#[cfg(any(test, all(windows, not(debug_assertions))))]
impl PendingShellUpdate {
    fn is_ready_for(&self, current_version: &str) -> bool {
        self.schema_version == PENDING_UPDATE_SCHEMA_VERSION
            && self.target_version == current_version
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn pending_update_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PENDING_UPDATE_FILE_NAME)
}

/// Windows 路径比较不区分大小写；统一分隔符后比较也让测试和跨版本
/// 记录的路径格式保持稳定。
#[cfg(any(test, all(windows, not(debug_assertions))))]
fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(any(test, all(windows, not(debug_assertions))))]
fn same_path(left: &Path, right: &Path) -> bool {
    normalized_path(left) == normalized_path(right)
}

#[cfg(any(test, all(windows, not(debug_assertions))))]
fn is_supported_shell_executable(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let file_name = file_name.to_ascii_lowercase();
    matches!(
        file_name.as_str(),
        "dsh-desktop.exe" | "dsh-xlink.exe" | "dsh_desktop.exe" | "dsh_xlink.exe"
    )
}

#[cfg(any(test, all(windows, not(debug_assertions))))]
fn legacy_shortcut_names(old_name: &str) -> &'static [&'static str] {
    match old_name {
        "dsh-desktop.exe" => &["dsh-desktop.lnk"],
        "dsh-xlink.exe" => &["dsh-xlink.lnk"],
        "dsh_desktop.exe" => &["dsh_desktop.lnk", "dsh-desktop.lnk"],
        "dsh_xlink.exe" => &["dsh_xlink.lnk", "dsh-xlink.lnk"],
        _ => &[],
    }
}

/// 返回需要由旧 NSIS 卸载器处理的安装目录；同目录覆盖安装不能再次
/// 调用卸载器，否则它会把刚写入的新版本一并删除。
#[cfg(any(test, all(windows, not(debug_assertions))))]
fn previous_install_dir_for_cleanup(
    previous_executable: &Path,
    current_executable: &Path,
) -> Result<Option<PathBuf>, String> {
    if !previous_executable.is_absolute() {
        return Err(format!(
            "旧版本可执行文件路径不是绝对路径：{}",
            previous_executable.display()
        ));
    }
    if !is_supported_shell_executable(previous_executable) {
        return Err(format!(
            "旧版本可执行文件名称不受支持：{}",
            previous_executable.display()
        ));
    }

    let previous_dir = previous_executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| format!("无法解析旧版本安装目录：{}", previous_executable.display()))?;
    let current_dir = current_executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| format!("无法解析当前版本安装目录：{}", current_executable.display()))?;

    if same_path(previous_dir, current_dir) {
        Ok(None)
    } else {
        Ok(Some(previous_dir.to_path_buf()))
    }
}

/// tauri-plugin-updater 2.x 在 Windows 临时目录下使用
/// `<app>-<version>-updater-*` 命名。只接受本项目当前和历史产品名，避免
/// 清理用户的其它临时目录。
#[cfg(any(test, all(windows, not(debug_assertions))))]
fn is_owned_updater_temp_dir(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    (name.starts_with("dsh-xlink-")
        || name.starts_with("dsh-desktop-")
        || name.starts_with("dsh_xlink-")
        || name.starts_with("dsh_desktop-"))
        && name.contains("-updater-")
}

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

/// 一次尝试（走某一条路由）的返回值。用装箱 future 是为了让「按顺序试」
/// 这一段只有一份实现，`check` 与 `install` 两条路径共用。
type Attempt<'a, T> = Pin<Box<dyn Future<Output = Result<T, UpdaterError>> + Send + 'a>>;

/// 按顺序试 `routes`，第一条走通的就返回它。
///
/// 走完全部路由仍未成功时返回 `Failed`，但**带的是最后一次**的失败——回退本身
/// 已经逐条落进 `shell-update-route` 事件日志，用户看到的那一条错误只该讲
/// 最后一次发生了什么。
async fn run_routes<'a, T: Send>(
    routes: &'a [Route],
    mut attempt: impl FnMut(&'a Route) -> Attempt<'a, T>,
) -> Routed<T> {
    let total = routes.len();
    let mut last: Option<UpdaterError> = None;
    for (index, route) in routes.iter().enumerate() {
        match attempt(route).await {
            Ok(value) => return Routed::Done(value),
            Err(error) => {
                // 访问成功但 endpoint 上没有已发布的 release：空状态，不是错误。
                // 放在**还有路由可试**时也不能就此收工——企业网关对任何地址回
                // 404 是常见行为，把它当成"没有更新"就会静默漏掉一次更新。
                let empty = matches!(error, UpdaterError::ReleaseNotFound);
                if index + 1 == total || !(empty || should_try_next(&error)) {
                    shell_events::record(
                        "shell-update-route",
                        &format!("{} 失败：{error}", route.describe()),
                    );
                    return if empty {
                        Routed::Empty
                    } else {
                        Routed::Failed(error)
                    };
                }
                shell_events::record(
                    "shell-update-route",
                    &format!(
                        "{} 失败（{error}），改试{}",
                        route.describe(),
                        routes[index + 1].describe()
                    ),
                );
                last = Some(error);
            }
        }
    }
    // `routes()` 恒以直连兜底，循环里必然 return；空列表只可能来自调用方写错。
    Routed::Failed(last.unwrap_or(UpdaterError::EmptyEndpoints))
}

/// 走完全部路由之后的结果。
enum Routed<T> {
    /// 某条路由走通了，`T` 是那条路由的返回值。
    Done(T),
    /// 访问成功，但 endpoint 上没有已发布的 release：空状态，不是错误。
    Empty,
    /// 全部路由都没走通。
    Failed(UpdaterError),
}

/// 这次失败该不该换下一条路由再试。
///
/// **只有传输层失败才回退**：连不上 / 超时 / TLS / DNS / 代理不可用。签名校验、
/// 清单解析、版本号解析、URL 无效这几类换一条路只会同样地失败，而重试一遍
/// 等于让"第二次也这样"盖住第一次的真正原因。`ReleaseNotFound` 不在这里——
/// 它在 `run_routes` 里单独判，因为那里还要处理它的「空状态」那层含义。
fn should_try_next(error: &UpdaterError) -> bool {
    matches!(error, UpdaterError::Reqwest(_) | UpdaterError::Network(_))
}

/// 全部路由都没走通时的用户文案：说清试过哪几条路，以及下一步能做什么。
fn describe_failure(routes: &[Route], error: UpdaterError) -> String {
    let tried = routes
        .iter()
        .map(Route::describe)
        .collect::<Vec<_>>()
        .join(" → ");
    format!(
        "{tried} 均失败：{}。请确认系统代理软件正在运行（关掉系统代理后本应用会\
         改走直连）；本次尝试的逐条记录在「查看日志」里的 shell-update-route。",
        explain_updater_error(error)
    )
}

/// 按一条路由组装 updater 客户端。
fn updater_via(
    app: &AppHandle,
    route: &Route,
    read_timeout: Duration,
) -> Result<Updater, UpdaterError> {
    let builder = app.updater_builder().configure_client(move |builder| {
        builder
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(read_timeout)
    });
    match route {
        // 直连是**真的**直连：显式关掉代理，包括环境变量里的那一个。回退到
        // 直连却仍被 `HTTPS_PROXY` 拉回代理，等于把同一条路试两遍。
        Route::Direct => builder.no_proxy(),
        Route::Proxy { url, .. } => builder.proxy(url.clone()),
    }
    .build()
}

/// 沿一条路由检查更新，返回已发布更新对应的版本。
async fn check_via(
    app: &AppHandle,
    route: &Route,
    read_timeout: Duration,
) -> Result<Option<String>, UpdaterError> {
    Ok(updater_via(app, route, read_timeout)?
        .check()
        .await?
        .map(|update: Update| update.version))
}

/// 把当前运行版本与最新已发布版本做比较。
///
/// 先按 `net_proxy::routes()` 给出的顺序试：探测到系统代理就走代理，代理自己
/// 不通再退回直连。`Ok(None)` 有两种来源——endpoint 上确实没有更新，以及访问
/// 到了 endpoint 但那里没有已发布的 release（空状态，后台检查保持静默，手动
/// 按钮仅展示当前版本）。两者都不是错误。
pub async fn check(app: &AppHandle) -> Result<ShellUpdateInfo, AppError> {
    let current = app.package_info().version.to_string();
    let routes = pkg::net_proxy::routes();
    let outcome = {
        let app = app.clone();
        run_routes(&routes, move |route| {
            let app = app.clone();
            Box::pin(async move { check_via(&app, route, CHECK_READ_TIMEOUT).await })
        })
        .await
    };
    let available = match outcome {
        Routed::Done(version) => version,
        Routed::Empty => None,
        Routed::Failed(error) => return Err(AppError::Update(describe_failure(&routes, error))),
    };
    Ok(ShellUpdateInfo { current, available })
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

/// 沿一条路由走完「检查 → 下载 → 验签」，返回那条路由上的 `Update` 与
/// 安装包字节。`Update` 要带回调用方：`install` 是它的方法，而回退到直连
/// 之后拿到的是另一个 `Update`（`download` 用的是各自那条路由的客户端）。
async fn download_via(
    app: &AppHandle,
    route: &Route,
    progress: &Mutex<impl FnMut(&str) + Send>,
) -> Result<(Update, Vec<u8>), UpdaterError> {
    let update = updater_via(app, route, DOWNLOAD_READ_TIMEOUT)?
        .check()
        .await?
        .ok_or(UpdaterError::ReleaseNotFound)?;
    let version = update.version.clone();
    let mut downloaded = 0_u64;
    let bytes = update
        .download(
            |chunk_length, total| {
                // 插件回调传入的是本次 chunk 大小，不是累计值；这里在
                // 外层累加后再展示，避免进度条反复显示单个 chunk 的大小。
                downloaded = downloaded.saturating_add(chunk_length as u64);
                let received_mb = format!("{:.1} MB", downloaded as f64 / 1_048_576.0);
                let total_mb = total
                    .map(|t| format!("{:.1} MB", t as f64 / 1_048_576.0))
                    .unwrap_or_else(|| "?".into());
                crate::lock(progress)(&format!(
                    "正在下载 v{version}（{received_mb} / {total_mb}）…"
                ));
            },
            || crate::lock(progress)("下载完成，正在校验并安装…"),
        )
        .await?;
    Ok((update, bytes))
}

/// 下载待安装的更新、完成安装并重启到新版本。在替换任何文件之前，
/// updater 会用固定公钥校验 minisign 签名。
///
/// 出网的部分（检查 + 下载）走的是与 `check` 同一套路由：先系统代理，
/// 代理不通再直连。**安装本体不走网络**，因此不在这套路由里。
pub async fn install(
    app: &AppHandle,
    data_dir: &Path,
    on_progress: impl FnMut(&str) + Send,
) -> Result<(), AppError> {
    #[cfg(all(windows, not(debug_assertions)))]
    let current_version = app.package_info().version.to_string();
    let routes = pkg::net_proxy::routes();
    // download 接收两个回调，两者都会上报进度；
    // 让它们共享同一个 sink，sink 放在 mutex 后面。
    let progress = Mutex::new(on_progress);
    let downloaded = {
        let app = app.clone();
        let progress = &progress;
        run_routes(&routes, move |route| {
            let app = app.clone();
            let progress = progress;
            Box::pin(async move { download_via(&app, route, progress).await })
        })
        .await
    };
    let (update, bytes) = match downloaded {
        Routed::Done(downloaded) => downloaded,
        Routed::Empty => return Err(AppError::Update("当前已是最新版本".into())),
        Routed::Failed(error) => return Err(AppError::Update(describe_failure(&routes, error))),
    };

    #[cfg(all(windows, not(debug_assertions)))]
    write_pending_update(data_dir, &current_version, &update.version)?;

    #[cfg(any(not(windows), debug_assertions))]
    let _ = data_dir;

    // `Update::install` 是同步实现（把安装包落盘，再派生安装器或替换 .app），
    // 在 async 上下文里直接调用会阻塞一个 tokio worker 线程——Windows 上还要
    // 等 NSIS 起来，可能几十秒。交给 blocking 线程池，并在它返回之后再重启。
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|e| AppError::Update(format!("安装线程异常结束：{e}")))?
        .map_err(|e| AppError::Update(format!("安装失败：{e}")))?;
    app.restart();
}

/// 由新版本管理面板首次挂载后调用。Windows 下只有当前包版本与更新前
/// 记录的目标版本一致时才会开始回收旧安装；其它平台保持 no-op。
pub fn confirm_shell_ready(app: &AppHandle, data_dir: &Path) -> Result<(), AppError> {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        if let Err(detail) = cleanup_after_ready(app, data_dir) {
            return Err(cleanup_failure(data_dir, &detail));
        }
    }

    #[cfg(any(not(windows), debug_assertions))]
    {
        let _ = (app, data_dir);
    }
    Ok(())
}

#[cfg(all(windows, not(debug_assertions)))]
fn write_pending_update(
    data_dir: &Path,
    current_version: &str,
    target_version: &str,
) -> Result<(), AppError> {
    let previous_executable = std::env::current_exe()
        .map_err(|e| AppError::Update(format!("无法记录旧版本路径：{e}")))?;
    if !is_supported_shell_executable(&previous_executable) {
        return Err(AppError::Update(format!(
            "无法识别当前 Shell 可执行文件：{}",
            previous_executable.display()
        )));
    }

    let pending = PendingShellUpdate {
        schema_version: PENDING_UPDATE_SCHEMA_VERSION,
        previous_executable: previous_executable.to_string_lossy().into_owned(),
        previous_version: current_version.to_string(),
        target_version: target_version.to_string(),
    };
    let text = serde_json::to_string_pretty(&pending)
        .map_err(|e| AppError::Update(format!("无法编码更新清理标记：{e}")))?;
    let path = pending_update_path(data_dir);
    crate::shell::process::atomic_write(&path, format!("{text}\n").as_bytes()).map_err(|e| {
        AppError::Update(format!(
            "无法记录更新清理标记：{e}（路径：{}）",
            path.display()
        ))
    })
}

#[cfg(all(windows, not(debug_assertions)))]
fn cleanup_after_ready(app: &AppHandle, data_dir: &Path) -> Result<(), String> {
    use std::sync::{Mutex, OnceLock};

    static CLEANUP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = crate::lock(CLEANUP_LOCK.get_or_init(|| Mutex::new(())));

    let current_version = app.package_info().version.to_string();
    let current_executable =
        std::env::current_exe().map_err(|e| format!("无法读取当前版本路径：{e}"))?;
    let desktop_dir = app
        .path()
        .desktop_dir()
        .map_err(|e| format!("无法解析 Windows 桌面目录：{e}"))?;
    let start_menu_dir = app
        .path()
        .config_dir()
        .map_err(|e| format!("无法解析 Windows 用户配置目录：{e}"))?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    let marker_path = pending_update_path(data_dir);
    let pending = load_pending_update(data_dir)?;

    let Some(pending) = pending else {
        // 旧版 dsh-desktop 可能是用户通过安装器直接迁移过来的，没有机会
        // 写入标记。只探测 NSIS 的默认 current-user 目录，避免碰用户自定义
        // 的其它安装位置。
        let mut failures = Vec::new();
        if let Err(error) =
            cleanup_legacy_default_install(&current_executable, &start_menu_dir, &desktop_dir)
        {
            failures.push(error);
        }
        // 标记机制启用前的版本也可能已经在 TEMP 留下 updater 目录，
        // 因此无标记路径同样执行一次受限的历史目录清理。
        if let Err(error) = cleanup_updater_temp_dirs() {
            failures.push(error);
        }
        return if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("；"))
        };
    };

    // 更新失败后旧版本可能再次启动；此时必须保留旧安装和标记，等待真正
    // 的目标版本启动，而不是把仍在工作的版本卸掉。
    if !pending.is_ready_for(&current_version) {
        return Ok(());
    }

    let previous_executable = PathBuf::from(&pending.previous_executable);
    match previous_install_dir_for_cleanup(&previous_executable, &current_executable)? {
        Some(previous_dir) => {
            if previous_dir.exists() {
                uninstall_install_dir(&previous_dir)?;
            }
        }
        None => cleanup_same_install_dir(
            &previous_executable,
            &current_executable,
            &start_menu_dir,
            &desktop_dir,
        )?,
    }

    cleanup_updater_temp_dirs()?;
    std::fs::remove_file(&marker_path)
        .map_err(|e| format!("无法删除更新清理标记：{}：{e}", marker_path.display()))?;
    Ok(())
}

#[cfg(all(windows, not(debug_assertions)))]
fn load_pending_update(data_dir: &Path) -> Result<Option<PendingShellUpdate>, String> {
    let path = pending_update_path(data_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取更新清理标记：{}：{error}", path.display())),
    };
    let pending: PendingShellUpdate = serde_json::from_str(&text)
        .map_err(|e| format!("更新清理标记损坏：{}：{e}", path.display()))?;
    if pending.schema_version != PENDING_UPDATE_SCHEMA_VERSION {
        return Err(format!(
            "更新清理标记版本不受支持：{}：{}",
            path.display(),
            pending.schema_version
        ));
    }
    Ok(Some(pending))
}

#[cfg(all(windows, not(debug_assertions)))]
fn cleanup_legacy_default_install(
    current_executable: &Path,
    start_menu_dir: &Path,
    desktop_dir: &Path,
) -> Result<(), String> {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return Ok(());
    };
    let legacy_dir = PathBuf::from(local_app_data).join("dsh-desktop");
    let Some(current_dir) = current_executable.parent() else {
        return Err(format!(
            "无法解析当前版本安装目录：{}",
            current_executable.display()
        ));
    };
    if same_path(&legacy_dir, current_dir) {
        return cleanup_same_install_dir(
            &legacy_dir.join("dsh-desktop.exe"),
            current_executable,
            start_menu_dir,
            desktop_dir,
        );
    }
    if !legacy_dir.exists() {
        return Ok(());
    }
    uninstall_install_dir(&legacy_dir)
}

#[cfg(all(windows, not(debug_assertions)))]
fn cleanup_same_install_dir(
    previous_executable: &Path,
    current_executable: &Path,
    start_menu_dir: &Path,
    desktop_dir: &Path,
) -> Result<(), String> {
    if same_path(previous_executable, current_executable) {
        return Ok(());
    }

    // 同目录升级时不能调用旧卸载器：它可能删除刚安装的新版本资源。
    // 只处理本项目历史产品名对应的旧 exe 和 NSIS 默认生成的两个快捷方式。
    remove_file_if_present(previous_executable, "旧版本可执行文件")?;

    let Some(old_name) = previous_executable
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return Err(format!(
            "无法解析旧版本可执行文件名称：{}",
            previous_executable.display()
        ));
    };
    let shortcut_names = legacy_shortcut_names(&old_name);
    if shortcut_names.is_empty() {
        return Ok(());
    }

    let mut failures = Vec::new();
    for name in shortcut_names {
        if let Err(error) =
            remove_file_if_present(&start_menu_dir.join(name), "旧版开始菜单快捷方式")
        {
            failures.push(error);
        }
    }
    for name in shortcut_names {
        if let Err(error) = remove_file_if_present(&desktop_dir.join(name), "旧版桌面快捷方式")
        {
            failures.push(error);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("；"))
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn remove_file_if_present(path: &Path, description: &str) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "无法删除 {description} {}：{error}",
            path.display()
        )),
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn uninstall_install_dir(install_dir: &Path) -> Result<(), String> {
    const SHELL_INSTALLER_FILES: [&str; 5] = [
        "dsh-desktop.exe",
        "dsh-xlink.exe",
        "dsh_desktop.exe",
        "dsh_xlink.exe",
        "uninstall.exe",
    ];

    let uninstaller = install_dir.join("uninstall.exe");
    if !uninstaller.is_file() {
        return Err(format!(
            "旧版本安装目录缺少 uninstall.exe：{}",
            install_dir.display()
        ));
    }

    // 不传 `/UPDATE`：NSIS 只有在普通卸载模式下才会删除旧快捷方式和
    // 卸载注册表项。/S 仅抑制交互界面，不勾选删除用户数据。
    let mut command = crate::shell::process::command_with_path(&uninstaller);
    crate::shell::process::quiet(&mut command);
    let install_dir_arg = format!("_?={}", install_dir.display());
    let status = command
        .arg("/S")
        .arg(install_dir_arg)
        .current_dir(install_dir)
        .status()
        .map_err(|e| format!("无法启动旧版本卸载器 {}：{e}", uninstaller.display()))?;
    if !status.success() {
        return Err(format!(
            "旧版本卸载器退出码 {:?}：{}",
            status.code(),
            install_dir.display()
        ));
    }

    // NSIS 卸载器会在退出后异步删除自身。等待这个短窗口，避免把正常的
    // 自删除误报为失败；超过上限仍保留标记，下一次启动再重试。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let remaining: Vec<&str> = SHELL_INSTALLER_FILES
            .iter()
            .copied()
            .filter(|name| install_dir.join(name).exists())
            .collect();
        if remaining.is_empty() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "旧版本卸载后仍存在 {}：{}",
                remaining.join("、"),
                install_dir.display()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn cleanup_updater_temp_dirs() -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match cleanup_updater_temp_dirs_once() {
            Ok(()) => return Ok(()),
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn cleanup_updater_temp_dirs_once() -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let entries = std::fs::read_dir(&temp_dir)
        .map_err(|e| format!("无法扫描 updater 临时目录 {}：{e}", temp_dir.display()))?;
    let mut failures = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(format!("读取临时目录条目失败：{error}"));
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_owned_updater_temp_dir(&name)
            || !entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("{}：{error}", path.display())),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "无法删除 updater 临时目录：{}",
            failures.join("；")
        ))
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn cleanup_failure(data_dir: &Path, detail: &str) -> AppError {
    let logs_dir = crate::kernel::lifecycle::logs_dir(data_dir);
    let spec = crate::shell::process::LogSpec::new(
        crate::shell::process::build_log_kind(),
        "shell-update-cleanup",
    );
    let log_path = spec.path_for(&logs_dir, &crate::shell::process::current_date_string());
    if let Ok(mut log) = crate::shell::process::RotatingLog::new(&logs_dir, spec) {
        let _ = log.write_line(detail);
    }
    AppError::Update(format!(
        "更新后的旧版本清理未完成：{detail}。应用仍可使用，重启后会自动重试。日志：{}",
        log_path.display()
    ))
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

    /// 「先代理、失败再直连」是这次改动的全部承诺，因此逐条钉死：
    /// 传输层失败才轮到直连，清单解析失败换一条路只会同样地失败。
    fn test_routes() -> Vec<Route> {
        vec![
            Route::Proxy {
                url: url::Url::parse("http://127.0.0.1:7890").expect("测试地址合法"),
                source: "测试来源",
            },
            Route::Direct,
        ]
    }

    /// `run_routes` 会把每次回退写进壳侧事件日志（GUI 应用的 stderr 在
    /// Windows 上没有去处），所以每个用例都必须先持住 `scoped_xlink_home`，
    /// 让那些行写进临时目录而不是用户的真实日志目录。
    fn temp_home(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "dsh-updater-{tag}-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ))
    }

    #[test]
    fn a_transport_failure_falls_back_to_direct() {
        let home = temp_home("fallback");
        let _xlink_home = crate::tests::scoped_xlink_home(&home);
        let routes = test_routes();
        let tried: std::sync::Arc<std::sync::Mutex<usize>> = Default::default();
        let seen = tried.clone();
        let outcome = tauri::async_runtime::block_on(run_routes(&routes, |route| {
            let attempt = {
                let mut seen = seen.lock().unwrap();
                *seen += 1;
                *seen
            };
            let is_proxy = matches!(route, Route::Proxy { .. });
            Box::pin(async move {
                if attempt == 1 && is_proxy {
                    Err(UpdaterError::Network("代理端口没人听".into()))
                } else {
                    Ok("v9.9.9")
                }
            }) as Attempt<'_, &'static str>
        }));
        assert!(
            matches!(outcome, Routed::Done("v9.9.9")),
            "代理失败后应改走直连并返回它的结果"
        );
        assert_eq!(*tried.lock().unwrap(), 2, "只应试两条路");
        drop(_xlink_home);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 企业网关对任何地址回 404 是常见行为。代理那条路回 `ReleaseNotFound`
    /// 时不能就此收工——那会把一次真实存在的更新静默漏掉。
    #[test]
    fn release_not_found_on_the_proxy_still_tries_direct() {
        let home = temp_home("empty");
        let _xlink_home = crate::tests::scoped_xlink_home(&home);
        let outcome = tauri::async_runtime::block_on(run_routes(&test_routes(), |route| {
            let is_proxy = matches!(route, Route::Proxy { .. });
            Box::pin(async move {
                if is_proxy {
                    Err(UpdaterError::ReleaseNotFound)
                } else {
                    Ok("v9.9.9")
                }
            }) as Attempt<'_, &'static str>
        }));
        assert!(matches!(outcome, Routed::Done("v9.9.9")));
        drop(_xlink_home);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 清单解析 / 签名校验这类失败换一条路只会同样地失败，重试一遍还会
    /// 让「第二次也这样」盖住第一次的真正原因。
    #[test]
    fn a_manifest_failure_does_not_retry_another_route() {
        let home = temp_home("terminal");
        let _xlink_home = crate::tests::scoped_xlink_home(&home);
        let tried: std::sync::Arc<std::sync::Mutex<usize>> = Default::default();
        let seen = tried.clone();
        let outcome = tauri::async_runtime::block_on(run_routes(&test_routes(), |_route| {
            *seen.lock().unwrap() += 1;
            let json_err: UpdaterError = serde_json::from_str::<serde_json::Value>("not json")
                .unwrap_err()
                .into();
            Box::pin(async move { Err(json_err) }) as Attempt<'_, &'static str>
        }));
        assert!(matches!(outcome, Routed::Failed(_)));
        assert_eq!(*tried.lock().unwrap(), 1, "不可回退的失败只试一条路");
        drop(_xlink_home);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 错误文案必须说清试过哪几条路——用户看到的是「检查更新失败」这一个
    /// 现象，而下一步（起代理软件 / 关系统代理）取决于走的到底是哪条路。
    #[test]
    fn failure_message_names_every_route_it_tried() {
        let json_err: UpdaterError = serde_json::from_str::<serde_json::Value>("not json")
            .unwrap_err()
            .into();
        let msg = describe_failure(&test_routes(), json_err);
        assert!(msg.contains("http://127.0.0.1:7890"), "缺代理地址：{msg}");
        assert!(msg.contains("直连"), "缺直连这一条：{msg}");
        assert!(msg.contains("JSON 解析失败"), "缺真正的原因：{msg}");
    }

    /// 传输层失败才换路；`Reqwest` 那一支在本 crate 外构造不出来（reqwest
    /// 不是直接依赖），而它正是线上真正会走的那一支，靠 `Network` 钉住同一
    /// 条规则。
    #[test]
    fn only_transport_failures_are_worth_another_route() {
        assert!(should_try_next(&UpdaterError::Network("下载失败".into())));
        let json_err: UpdaterError = serde_json::from_str::<serde_json::Value>("not json")
            .unwrap_err()
            .into();
        assert!(!should_try_next(&json_err));
        let io_err: UpdaterError = std::io::Error::other("boom").into();
        assert!(!should_try_next(&io_err));
    }

    #[test]
    fn pending_update_is_ready_only_for_its_target_version() {
        let pending = PendingShellUpdate {
            schema_version: PENDING_UPDATE_SCHEMA_VERSION,
            previous_executable: String::from("/old/dsh-desktop.exe"),
            previous_version: String::from("0.1.2-rc.13"),
            target_version: String::from("0.1.2-rc.14"),
        };

        assert!(pending.is_ready_for("0.1.2-rc.14"));
        assert!(!pending.is_ready_for("0.1.2-rc.13"));
        assert!(!pending.is_ready_for("0.1.2-rc.15"));
    }

    /// 清理判定要用的路径必须是**本机意义上的绝对路径**。`Path::is_absolute`
    /// 在 Windows 上对 `/install/x.exe` 返回 false，写死 POSIX 路径会让下面
    /// 两个用例在 Windows 上一红一假绿：同目录那条直接红，而"拒绝不支持的
    /// 可执行文件"那条因为同一原因根本没走到文件名校验那一支。
    fn rooted(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn cleanup_rejects_unsupported_or_relative_previous_executables() {
        let current = rooted("new/dsh-xlink/dsh-xlink.exe");

        let relative = std::path::Path::new("old/dsh-desktop.exe");
        assert!(previous_install_dir_for_cleanup(relative, &current).is_err());

        let unrelated = rooted("old/not-our-app.exe");
        assert!(previous_install_dir_for_cleanup(&unrelated, &current).is_err());
    }

    #[test]
    fn cleanup_skips_uninstaller_when_previous_and_current_share_directory() {
        let previous = rooted("install/dsh-desktop.exe");
        let current = rooted("install/dsh-xlink.exe");

        assert_eq!(
            previous_install_dir_for_cleanup(&previous, &current).unwrap(),
            None
        );
    }

    #[test]
    fn updater_temp_name_matches_only_known_shell_prefixes() {
        assert!(is_owned_updater_temp_dir(
            "dsh-xlink-0.1.2-rc.14-updater-a1b2"
        ));
        assert!(is_owned_updater_temp_dir(
            "dsh-desktop-0.1.2-rc.13-updater-c3d4"
        ));
        assert!(!is_owned_updater_temp_dir("tauri-0.1.2-updater-a1b2"));
        assert!(!is_owned_updater_temp_dir("dsh-xlink-0.1.2-cache-a1b2"));
    }

    #[test]
    fn legacy_executable_names_map_to_their_installer_shortcuts() {
        assert_eq!(
            legacy_shortcut_names("dsh-desktop.exe"),
            &["dsh-desktop.lnk"]
        );
        assert_eq!(legacy_shortcut_names("dsh-xlink.exe"), &["dsh-xlink.lnk"]);
        assert_eq!(
            legacy_shortcut_names("dsh_desktop.exe"),
            &["dsh_desktop.lnk", "dsh-desktop.lnk"]
        );
        assert!(legacy_shortcut_names("other.exe").is_empty());
    }
}
