//! Tauri commands backing the management UI.
//!
//! All commands operate against the shared [`AppState`] (data directory plus
//! the running kernel child) and the persisted `settings.json`. Long-running
//! work (kernel install) runs off the main thread and reports progress over a
//! `tauri::ipc::Channel`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{mpsc, Mutex, OnceLock};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, State, Webview};
use tauri::{WebviewBuilder, WebviewUrl, WebviewWindowBuilder, WindowBuilder, WindowEvent};
use url::Url;

use crate::error::AppError;
use crate::process::read_tail;
use crate::quarantine;
use crate::{guard, kernel, node, plugins, releases, settings, skills, updater};

/// DeepSeek official chat entrypoint that `open_official_chat` loads into
/// the dedicated `official-chat` webview.
///
/// No user-agent override for this window: the WebView2 engine IS a genuine
/// desktop Edge/Chromium build. Overriding the UA string claims Chrome in
/// the header while `Sec-CH-UA` client hints and native
/// `navigator.userAgentData` keep reporting the real Edge brand — that
/// cross-layer mismatch is exactly what environment checks flag, so the
/// honest identity is also the consistent one.
pub const OFFICIAL_CHAT_URL: &str = "https://chat.deepseek.com";

// WKWebView stores cookies and localStorage under this identifier on macOS.
// Keep the IDs stable across releases, and separate debug from release data.
#[cfg(all(target_os = "macos", debug_assertions))]
const OFFICIAL_CHAT_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-dev-001";
#[cfg(all(target_os = "macos", not(debug_assertions)))]
const OFFICIAL_CHAT_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-rel-001";

/// Chromium feature switches passed to the `official-chat` webview.
///
/// `additional_browser_args` **replaces** wry's built-in default set
/// (`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`), so
/// those entries are restated here instead of silently re-enabled: without
/// them WebView2 shows SmartScreen interstitials and Edge-only overlay UI a
/// normal desktop Chrome never has. On top of those, `AutomationControlled`
/// (both as a browser feature and as a blink runtime flag) keeps Chromium
/// from reporting `navigator.webdriver = true` at the engine level — before
/// any initialization script can mask it — and `TranslateUI` /
/// `InterestFeedContentSuggestions` suppress more Edge-only surfaces. Only
/// the WebView2 backend consumes browser args; macOS / Linux ignore them,
/// so the builder wiring stays branch-free. The same per-folder options
/// rule is why [`open_official_chat`] pairs this constant with a dedicated
/// user-data directory.
pub const OFFICIAL_CHAT_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,AutomationControlled,TranslateUI,InterestFeedContentSuggestions --disable-blink-features=AutomationControlled";

/// Second official-chat tab: 通义千问 (qianwen).
pub const OFFICIAL_CHAT_QIANWEN_URL: &str = "https://www.qianwen.com";

/// Third official-chat tab: MiniMax agent.
pub const OFFICIAL_CHAT_MINIMAX_URL: &str = "https://agent.minimaxi.com";

/// The fixed tabs rendered in the official-chat window's tab strip, in
/// display order. The first entry is the default active tab on open. Add a
/// row here to add a tab — the strip webview discovers the list at runtime
/// the strip webview discovers the list at runtime via [`official_chat_tabs`]
/// and content webviews are created lazily when selected, so no other site is
/// loaded during the initial open.
pub const OFFICIAL_CHAT_TABS: &[(&str, &str)] = &[
    ("DeepSeek", OFFICIAL_CHAT_URL),
    ("千问", OFFICIAL_CHAT_QIANWEN_URL),
    ("MiniMax", OFFICIAL_CHAT_MINIMAX_URL),
];

/// Bare-window label. A `Window` (not a `WebviewWindow`) hosts the strip
/// plus one child `Webview` per tab; closing the window tears down the lot.
const OFFICIAL_CHAT_WINDOW_LABEL: &str = "official-chat";
/// Local SPA webview that renders the tab bar (`index.html?chatstrip=1`).
/// It keeps `window.__TAURI__` — `chat-fingerprint.js` is NOT injected here —
/// so it can invoke [`official_chat_tabs`] / [`switch_official_chat_tab`].
/// The pull-string lamp lives here too because child WebView transparency is
/// unreliable on this Tauri 2.11 / wry 0.55.1 stack. The compact lamp and tab
/// controls fit in the same 38px strip.
const OFFICIAL_CHAT_STRIP_LABEL: &str = "official-chat-strip";
/// Logical height of the pinned tab strip. The compact 24x38 desk-lamp SVG
/// fits inside the natural 38px tab-bar height.
const OFFICIAL_CHAT_INITIAL_WIDTH: f64 = 1366.0;
const OFFICIAL_CHAT_INITIAL_HEIGHT: f64 = 768.0;
const OFFICIAL_CHAT_STRIP_HEIGHT: f64 = 38.0;

/// Shared shell state installed as a Tauri managed state.
pub struct AppState {
    pub data_dir: PathBuf,
    pub running: Mutex<Option<Child>>,
    /// Last resolved Node runtime, keyed by the configured node path. The
    /// status poll runs every few seconds; re-probing `node --version` each
    /// time would spawn a process per poll (slow on Windows, where process
    /// creation is expensive) for a result that only changes with the
    /// setting or the machine's Node install.
    pub node_cache: Mutex<Option<(Option<String>, node::NodeInfo)>>,
}

/// Everything the management UI needs on the first render.
#[derive(Serialize)]
pub struct StatusView {
    /// Version of the running shell itself (from tauri.conf.json).
    pub shell_version: String,
    /// True in debug builds (`tauri dev`). The panel uses it to wash its
    /// header column with the whale-eye red so the dev shell is visually
    /// distinct from an installed release shell sharing the screen.
    pub dev_build: bool,
    pub kernel: kernel::KernelStatus,
    pub node: node::NodeInfo,
    pub settings: settings::Settings,
    /// Plugins the boot guard has disabled. The overview renders a banner
    /// from this so a workbench running in safe mode is never silent about
    /// what it is missing.
    pub quarantined: Vec<quarantine::QuarantineItem>,
    /// Most recent boot-guard incident, if any. Survives shell restarts via
    /// `last-incident.json`, so「查看详情」keeps working after a relaunch
    /// instead of only in the start command's response.
    pub last_incident: Option<guard::Incident>,
    /// Whether the dedicated `official-chat` webview window is currently
    /// registered with the app. The status poll observes this on the same
    /// 2.5s cadence so the panel's button label flips between
    /// 「打开官方对话」 and 「关闭官方对话」 without an extra IPC round-trip.
    pub official_chat_open: bool,
}

// Read a bounded tail of a text file for display — moved to
// `crate::process::read_tail` so the boot guard reads the same way.
///
/// The web-app level error prefix the UI must not swallow.
fn app_err(data_dir: &Path, e: impl std::fmt::Display) -> String {
    format!("{e}（数据目录：{}）", data_dir.display())
}

// --- status ---------------------------------------------------------------

#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<StatusView, String> {
    let data_dir = state.data_dir.clone();
    // File probes and the port check run on a blocking worker: as a sync
    // command this poll would hold the Tauri main thread every few seconds.
    tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::load(&data_dir);
        let kernel_status = kernel::status(&data_dir, &settings);
        let quarantine_doc = quarantine::load(&data_dir);
        let state = app.state::<AppState>();
        let node_info = cached_node(&state, &settings);
        let official_chat_open = app.get_window(OFFICIAL_CHAT_WINDOW_LABEL).is_some();
        StatusView {
            shell_version: app.package_info().version.to_string(),
            dev_build: cfg!(debug_assertions),
            kernel: kernel_status,
            node: node_info,
            quarantined: quarantine_doc.items,
            last_incident: guard::load_incident(&data_dir),
            settings,
            official_chat_open,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Resolve the Node runtime through the per-app cache; only a changed
/// `node_path` setting triggers a fresh probe.
fn cached_node(state: &AppState, settings: &settings::Settings) -> node::NodeInfo {
    let key = settings.node_path.clone();
    let mut guard = crate::lock(&state.node_cache);
    if let Some((cached_key, info)) = guard.as_ref() {
        if *cached_key == key {
            return info.clone();
        }
    }
    let info = node::resolve(settings);
    *guard = Some((key, info.clone()));
    info
}

#[tauri::command]
pub async fn detect_node(state: State<'_, AppState>) -> Result<node::NodeInfo, String> {
    // Detection ignores any configured path: it reports what the
    // environment has, so the UI can pre-fill the setting. Resolving may
    // spawn one child per environment candidate (PATH + nvm-managed
    // installs + system locations) — keep those process spawns off the
    // Tauri main thread.
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = settings::load(&data_dir);
        s.node_path = None;
        node::resolve(&s)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: settings::Settings,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || settings::save(&data_dir, &settings))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_kernel_log(state: State<'_, AppState>) -> Result<String, String> {
    let path = kernel::logs_dir(&state.data_dir).join("kernel.log");
    tauri::async_runtime::spawn_blocking(move || read_tail(&path, 16 * 1024))
        .await
        .map_err(|e| e.to_string())
}

/// One entry for the log-files modal tab list.
#[derive(Serialize)]
pub struct LogFileEntry {
    /// Just the basename (e.g. `kernel.log`, `install-0.1.0-rc.6.log`); the
    /// UI passes it back to `read_log_file`. Never expose absolute paths —
    /// the UI runs in a sandboxed webview and should not need them.
    pub name: String,
    /// File size in bytes; the modal shows it next to the tab name.
    pub size: u64,
}

/// List `*.log` files under the shell's log directory, newest first.
///
/// Files that disappear between `read_dir` and `metadata` are silently
/// skipped — install logs are rotated in place and may race with this scan.
#[tauri::command]
pub async fn list_log_files(state: State<'_, AppState>) -> Result<Vec<LogFileEntry>, String> {
    let dir = kernel::logs_dir(&state.data_dir);
    tauri::async_runtime::spawn_blocking(move || {
        let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;
        let mut out: Vec<LogFileEntry> = entries
            .filter_map(|e| e.ok())
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("log") {
                    return None;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                Some(LogFileEntry { name, size })
            })
            .collect();

        // Newest first so the live `kernel.log` (touched on every status tick)
        // lands at index 0 — the modal's default tab.
        out.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Read the tail of a named log file under the logs directory.
///
/// `name` must be a bare filename with no path separators; the function
/// refuses anything else to keep the UI's tab list from escaping the
/// logs directory. The same 16 KiB tail bound used by `get_kernel_log`
/// keeps the modal responsive on large install logs.
#[tauri::command]
pub async fn read_log_file(state: State<'_, AppState>, name: String) -> Result<String, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("非法的日志文件名：{name}"));
    }
    let logs_dir = kernel::logs_dir(&state.data_dir);
    let path = logs_dir.join(&name);
    if !path.starts_with(&logs_dir) {
        return Err(format!("日志路径越界：{name}"));
    }
    tauri::async_runtime::spawn_blocking(move || read_tail(&path, 16 * 1024))
        .await
        .map_err(|e| e.to_string())
}

/// Reveal the shell's data directory in the OS file manager.
///
/// The path comes from `AppState.data_dir`, which `lib::setup` resolves
/// from `kernel::data_dir` and creates on first launch, so the directory
/// always exists at runtime. Going through the server side (instead of
/// letting the UI call `opener.open_path` directly) bypasses the opener
/// plugin's IPC scope check — `opener:default` only grants `open_url` /
/// `reveal_item_in_dir` / default URLs, not `open_path`. The `open` crate
/// that backs the plugin dispatches per-OS: `open` on macOS launches
/// Finder with the directory selected in its parent, `cmd /C start ""` on
/// Windows opens File Explorer on the directory itself.
#[tauri::command]
pub async fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|e| format!("无法打开数据目录：{e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- shell self-update -----------------------------------------------------

/// Check GitHub for a newer shell release (manual「检查更新」button).
#[tauri::command]
pub async fn check_shell_update(app: AppHandle) -> Result<updater::ShellUpdateInfo, String> {
    updater::check(&app).await.map_err(|e| e.to_string())
}

/// Download, verify, and install the pending shell update, then restart.
#[tauri::command]
pub async fn install_shell_update(app: AppHandle, on_event: Channel<String>) -> Result<(), String> {
    updater::install(&app, move |line| {
        let _ = on_event.send(line.to_string());
    })
    .await
    .map_err(|e| e.to_string())
}

// --- releases --------------------------------------------------------------

/// Fetch the official kernel release list for the update menu.
#[tauri::command]
pub async fn fetch_releases() -> Result<releases::ReleaseList, String> {
    // ureq is synchronous; keep the blocking HTTPS fetch off the main thread.
    tauri::async_runtime::spawn_blocking(releases::list_releases)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Resolve pnpm against an already-probed node (the caller's cached
/// `node::NodeInfo`), auto-installing pnpm via npm when missing. Returns
/// (node_path, pnpm_exe).
pub fn promise_pnpm(
    data_dir: &Path,
    node_info: &node::NodeInfo,
    mut on_progress: impl FnMut(&str),
) -> Result<(PathBuf, PathBuf), String> {
    if !node_info.ok {
        return Err(node_info.reason.clone());
    }
    let s = settings::load(data_dir);
    let node_dir = Path::new(&node_info.path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    // The auto-install log lives under the shell's log dir next to the
    // install logs; rotation reuses the existing kernel::logs_dir helper.
    let pnpm_log = kernel::logs_dir(data_dir).join(format!(
        "pnpm-install-{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    ));
    let pnpm = node::ensure_pnpm(&s, &node_dir, &pnpm_log, &mut on_progress)?;
    Ok((PathBuf::from(node_info.path.clone()), pnpm))
}

// --- kernel install / switch / remove --------------------------------------

/// Install a pinned kernel version from npm, streaming progress events.
#[tauri::command]
pub async fn install_kernel(
    state: State<'_, AppState>,
    version: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    let settings = settings::load(&data_dir);
    let node_info = cached_node(&state, &settings);
    let dir_for_install = data_dir.clone();
    let node_info_for_install = node_info.clone();
    let version_for_install = version.clone();
    let send_install = on_event.clone();
    let (node_path, pnpm_exe) =
        tauri::async_runtime::spawn_blocking(move || -> Result<(PathBuf, PathBuf), String> {
            let mut send = |msg: &str| {
                let _ = send_install.send(msg.to_string());
            };
            let (node_path, pnpm_exe) =
                promise_pnpm(&dir_for_install, &node_info_for_install, &mut send)?;
            // `node_dir` is the directory of the validated `node` executable.
            // Install children need it on PATH so pnpm's `#!/usr/bin/env node`
            // shebang and any lifecycle script that shells out to `node` resolve
            // it even when the GUI process inherited a launchd-only PATH (macOS
            // .app bundles) — the common case for nvm-managed installs.
            let node_dir = node_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            kernel::install_version(
                &dir_for_install,
                &node_dir,
                &pnpm_exe,
                &version_for_install,
                |msg| {
                    send(msg);
                },
            )
            .map_err(|e| e.to_string())?;
            Ok((node_path, pnpm_exe))
        })
        .await
        .map_err(|e| e.to_string())??;

    // First kernel installed becomes active automatically; later installs
    // leave the current active version untouched.
    if kernel::read_active(&data_dir).is_none() {
        kernel::set_active(&data_dir, &version).map_err(|e| e.to_string())?;
        let _ = on_event.send(format!("已切换到版本 {version}"));
    }
    if !kernel::port_open(settings::load(&data_dir).port) {
        let _ = on_event.send("正在启动内核…".to_string());
        // Same guarded boot the「启动工作台」button uses: a freshly wired
        // plugin that breaks the kernel must land in the quarantine flow,
        // not leave the user with a crashed workbench after an install.
        let dir_for_start = data_dir.clone();
        // The channel stays with the outer function for the closing status
        // messages; the guarded-start worker gets its own clone.
        let on_event_for_start = on_event.clone();
        let pnpm_exe_for_start = pnpm_exe.clone();
        let mut send = move |msg: &str| {
            let _ = on_event_for_start.send(msg.to_string());
        };
        let start_result = tauri::async_runtime::spawn_blocking(
            move || -> Result<(guard::StartReport, Option<Child>), String> {
                let settings = settings::load(&dir_for_start);
                let deps = guard::GuardDeps {
                    data_dir: &dir_for_start,
                    settings: &settings,
                    node_path: &node_path,
                    pnpm_exe: &pnpm_exe_for_start,
                };
                Ok(guard::guarded_start(&deps, &mut send))
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        let (report, child) = start_result?;
        if let Some(child) = child {
            register_child(&state, &data_dir, child);
            let _ = on_event.send("内核已启动".to_string());
        }
        if let Some(incident) = report.incident {
            let _ = on_event.send(incident.message);
            for step in &incident.attempts {
                let _ = on_event.send(format!("· {step}"));
            }
        } else if !report.running {
            let _ = on_event.send("内核未能启动，详情见日志".to_string());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn activate_version(app: AppHandle, version: String) -> Result<(), String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    // Wiring runs pnpm against the store; keep the whole switch off the
    // main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        // The switch takes effect on the next start; a running kernel keeps
        // serving until the user restarts it.
        kernel::set_active(&data_dir, &version).map_err(|e| e.to_string())?;
        // 重新接线插件到新活动内核（失败不阻断切换，原因进入插件卡片警告）
        let settings = settings::load(&data_dir);
        let state = app.state::<AppState>();
        let node_info = cached_node(&state, &settings);
        let _ = plugins::ensure_wiring_quiet(&data_dir, &settings, &node_info);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_version(state: State<'_, AppState>, version: String) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    // remove_dir_all on a kernel tree (node_modules included) can take
    // seconds on Windows; never on the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        kernel::uninstall(&data_dir, &version).map_err(|e| app_err(&data_dir, e))
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- kernel lifecycle -------------------------------------------------------

/// Register a successfully started kernel child: record its pid for later
/// teardown from a restarted shell and hold the handle in app state.
fn register_child(state: &AppState, data_dir: &Path, child: Child) {
    kernel::write_pid(data_dir, child.id());
    crate::lock(&state.running).replace(child);
}

/// Start the active kernel under the boot guard. Idempotent: if the port
/// already answers this is a no-op report.
///
/// The guard watches the spawned process until the port is ready; on a boot
/// failure it attributes the crash to installed plugins from the kernel log,
/// quarantines suspects (then, if needed, every third-party plugin), rewires
/// the profile between attempts, and reports an [`guard::Incident`] either
/// way so the UI can ask the user what to keep or remove. Progress messages
/// stream over `on_event` because guarded retries include pnpm runs and can
/// take a couple of minutes in the worst case.
#[tauri::command]
pub async fn start_kernel(
    app: AppHandle,
    on_event: Channel<String>,
) -> Result<guard::StartReport, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    // Wiring and the child spawn both block (pnpm, process creation); run
    // them on a blocking worker rather than the Tauri main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<guard::StartReport, String> {
        let settings = settings::load(&data_dir);
        let state = app.state::<AppState>();
        let node_info = cached_node(&state, &settings);
        if !node_info.ok {
            return Err(node_info.reason.clone());
        }
        let node_path = PathBuf::from(node_info.path.clone());
        let mut send = |msg: &str| {
            let _ = on_event.send(msg.to_string());
        };
        // Guarded retries rewire plugins through pnpm; resolve it up front so
        // a missing toolchain fails before any attempt rather than mid-flow.
        let (_, pnpm_exe) = promise_pnpm(&data_dir, &node_info, &mut send)?;
        let deps = guard::GuardDeps {
            data_dir: &data_dir,
            settings: &settings,
            node_path: &node_path,
            pnpm_exe: &pnpm_exe,
        };
        let (report, child) = guard::guarded_start(&deps, &mut send);
        if let Some(child) = child {
            register_child(&state, &data_dir, child);
        }
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop the kernel and close the harness window, so the UI's「关闭工作台」
/// tears down the whole workbench rather than leaving a dead webview behind.
/// When the shell restarted since it spawned the kernel, the in-memory child
/// is gone but the pid file still names the process to reap.
///
/// The harness window is created with `closable(false)` (see `open_harness`),
/// so the OS title-bar close button is disabled and an accidental click on
/// it cannot drop the user's session. The deliberate path back through this
/// command still has to work, so the window goes through `destroy()` —
/// which forces the OS to close without honoring the closable flag — rather
/// than `close()`, which would be blocked by the same flag it set.
#[tauri::command]
pub async fn stop_kernel(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("harness") {
        let _ = window.destroy();
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    // kernel::stop waits for the child to exit (up to its kill timeout);
    // keep that wait off the main thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        {
            let mut guard = crate::lock(&state.running);
            if let Some(mut child) = guard.take() {
                kernel::stop(&mut child).map_err(|e| e.to_string())?;
            }
        }
        let port = settings::load(&data_dir).port;
        if kernel::port_open(port) {
            // First try the pid file — the in-memory handle is gone
            // across a shell restart, but a previous shell wrote a
            // pid to <data_dir>/kernel.pid and the kernel it spawned
            // is still bound to this port. kill_pid already validates
            // that the pid still points at a dsh kernel before sending
            // signals, so a pid recycled to an unrelated process is
            // a no-op.
            let mut killed = false;
            if let Some(pid) = kernel::read_pid(&data_dir) {
                if kernel::pid_is_kernel(pid, Some(port)) {
                    kernel::kill_pid(pid, Some(port));
                    killed = true;
                }
            }
            // Fallback: when the dev/release shells run side-by-side
            // and the in-memory child + pid file are both missing
            // (e.g. start_maybe skipped the launch because the port
            // was already bound by the other shell's kernel), the
            // shell has no in-record way to find the listener. Walk
            // the listening port to recover its pid, then run it
            // through the same pid_is_kernel guard so a recycled
            // pid that happens to point at an unrelated process still
            // is left alone.
            if !killed {
                if let Some(pid) = kernel::port_listen_pid(port) {
                    if kernel::pid_is_kernel(pid, Some(port)) {
                        kernel::kill_pid(pid, Some(port));
                    }
                }
            }
        }
        kernel::clear_pid(&data_dir);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Receive a one-shot health report from the harness webview and attribute it
/// against the current kernel log. The returned incident is also emitted to the
/// management panel so a white page cannot fail silently.
#[tauri::command]
pub async fn report_harness_fault(
    app: AppHandle,
    webview: Webview,
    kind: String,
    message: String,
    stack: String,
    page_url: String,
) -> Result<guard::Incident, String> {
    if webview.label() != "harness" {
        return Err(String::from("工作台自检只能由 harness 窗口报告"));
    }
    let kind = bounded_health_text("类型", kind, 80, true)?;
    if !matches!(
        kind.as_str(),
        "blank" | "runtime-error" | "unhandled-rejection"
    ) {
        return Err(String::from("工作台自检类型无效，请重新打开工作台"));
    }
    let message = bounded_health_text("错误信息", message, 2_000, true)?;
    let stack = bounded_health_text("错误堆栈", stack, 8_000, false)?;
    let page_url = bounded_health_text("页面地址", page_url, 1_000, false)?;
    let report = guard::HealthReport {
        kind,
        message,
        stack,
        page_url,
    };
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let incident = guard::diagnose_runtime(&data_dir, report);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.emit("harness-fault", &incident);
        }
        Ok(incident)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn bounded_health_text(
    field: &str,
    value: String,
    max_chars: usize,
    required: bool,
) -> Result<String, String> {
    let value = value.trim().to_string();
    if required && value.is_empty() {
        return Err(format!("工作台自检的{field}不能为空"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("工作台自检的{field}过长，请重新打开工作台"));
    }
    Ok(value)
}

///
/// The command performs its settings read and port probe on a blocking worker.
/// Webview creation still happens on a fresh OS thread: synchronous construction
/// inside a Tauri command can deadlock on Windows, and keeping the builder off
/// the async executor is safer on every platform.
///
/// The window is created with `closable(false)` so the OS title-bar close
/// button is greyed out: an accidental click in the middle of a long task
/// would otherwise drop the user's session. The deliberate path back
/// through `stop_kernel` still works because that command uses `destroy()`
/// rather than `close()`, which forces the OS to honor the tear-down even
/// when the chrome close button is disabled. The Linux GTK+ backend is the
/// documented exception: it may not grey the button out for windows that
/// are already visible, so on Linux this is a behavioural hint rather than
/// a hard guarantee.
/// Open the dsh web workbench window. The native titlebar stays as
/// the standard macOS / Windows / Linux chrome rather than Overlay so
/// that the OS-level drag / resize / double-click-zoom continue to work
/// reliably (the WKWebView drag-region path through `start_dragging` IPC
/// is flaky under Tauri 2.11.5). The chrome-row pulse is owned by the
/// shell rather than the kernel's `packages/client/web/src/base.css`,
/// injected via `initialization_script(titlebar-pulse.js)`; the script
/// appends a `<style>` node with `!important` rules so the shell
/// override wins regardless of which kernel version is running and
/// regardless of load order between this script and the workbench's
/// own stylesheets. A second injected script (`pullstring-launcher.js`)
/// renders a pull-string lamp floating at the workbench's top-left edge;
/// pulling it invokes [`focus_main_shell`] to raise the management window
/// over the current desktop.
#[tauri::command]
pub async fn open_harness(app: AppHandle) -> Result<(), String> {
    let data_dir = crate::kernel::data_dir(&app);
    let port = tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::load(&data_dir);
        if !kernel::port_open(settings.port) {
            return Err(format!(
                "内核未在运行（端口 {}），请先点击「启动工作台」",
                settings.port
            ));
        }
        Ok::<u16, String>(settings.port)
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(existing) = app.get_webview_window("harness") {
        let _ = existing.set_focus();
        return Ok(());
    }
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).map_err(|e| e.to_string())?;
    let handle = app.clone();
    std::thread::Builder::new()
        .name("dsh-open-harness".into())
        .spawn(move || {
            let result = WebviewWindowBuilder::new(&handle, "harness", WebviewUrl::External(url))
                .title("DeepSeek Harness 工作台")
                .inner_size(1280.0, 840.0)
                .closable(false)
                .initialization_script(include_str!("titlebar-pulse.js"))
                .initialization_script(include_str!("pullstring-launcher.js"))
                .initialization_script(include_str!("harness-health.js"))
                .build();
            if let Err(e) = result {
                eprintln!("dsh-desktop: failed to open harness window: {e}");
            }
            #[cfg(debug_assertions)]
            if let Ok(window) = app.get_webview_window("harness").ok_or("no harness window") {
                window.open_devtools();
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Open a log file in a dedicated, resizable viewer window.
///
/// The management window is fixed at 480×800 (tauri.conf.json), so the log
/// panel's「全屏」按钮 hands reading to its own OS window instead of
/// stretching an in-page dialog. Construction mirrors `open_harness`: the
/// webview is built on a fresh thread because doing so on the main thread
/// deadlocks on Windows. An existing viewer is destroyed and recreated so
/// opening another file needs no cross-window messaging; the window is
/// read-only, so a dropped viewer loses nothing.
///
/// The page is the same SPA: `ui/src/main.js` mounts the standalone viewer
/// instead of the management shell when `?log=<name>` is present, and the
/// viewer calls `read_log_file` itself (capability `log-viewer.json` grants
/// only that command). The name gets `read_log_file`'s validation here too,
/// so a bad name fails before a window appears.
#[tauri::command]
pub fn open_log_window(app: AppHandle, name: String) -> Result<(), String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("非法的日志文件名：{name}"));
    }
    if let Some(existing) = app.get_webview_window("log-viewer") {
        let _ = existing.destroy();
    }
    let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("dsh-open-log-viewer".into())
        .spawn(move || {
            let result = WebviewWindowBuilder::new(
                &handle,
                "log-viewer",
                WebviewUrl::App(format!("index.html?log={encoded}").into()),
            )
            .title(format!("日志 - {name}"))
            .inner_size(960.0, 720.0)
            .resizable(true)
            .build();
            if let Err(e) = result {
                eprintln!("dsh-desktop: failed to open log viewer window: {e}");
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Raise the shell's main management window above the current desktop.
///
/// Invoked from the pull-string lamp injected into the workbench webview
/// (`pullstring-launcher.js`) over `window.__TAURI__.core.invoke`, so it
/// works regardless of which kernel version the workbench is running
/// against. `show` + `unminimize` recover the window from a hidden or
/// minimized state before `set_focus` moves it to the foreground; the
/// window is configured non-resizable and always exists (tauri.conf.json),
/// so a missing window is an internal error worth surfacing to the
/// webview's console.
///
/// The always-on-top toggle before `set_focus` is the Windows
/// foreground-lock workaround: `SetForegroundWindow` is silently ignored
/// when the OS decides the process may not steal foreground (focus
/// arriving over IPC rather than a direct input event), leaving the panel
/// raised-but-behind. Pinning the window topmost and immediately releasing
/// it forces it to the head of the normal z-order regardless; on
/// macOS/Linux the toggle is a harmless no-op raise.
///
/// `x`/`y` are the click's screen coordinates in CSS pixels
/// (`MouseEvent.screenX/Y`); when present the panel is first repositioned so
/// the click's x lands at the window's horizontal center while the top sits
/// just below the click's y (clamped to keep the window fully on the
/// containing monitor), so the user never has to hunt for it on another
/// monitor. They are optional so an older injected script that invokes
/// without arguments still raises the window in place.
#[tauri::command]
pub fn focus_main_shell(app: AppHandle, x: Option<f64>, y: Option<f64>) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Err("主壳窗口不存在（label: main）".to_string());
    };
    if let (Some(x), Some(y)) = (x, y) {
        reposition_near(&app, &window, x, y);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_always_on_top(false);
    window.set_focus().map_err(|e| e.to_string())
}

fn official_chat_mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn current_official_chat_layout(window: &tauri::Window) -> OfficialChatLayout {
    let scale = window.scale_factor().unwrap_or(1.0);
    let phys = window.inner_size().unwrap_or_default();
    let (width, height) = official_chat_initial_size(phys.width, phys.height, scale);
    official_chat_layout(width, height)
}

fn add_official_chat_tab(
    window: &tauri::Window,
    index: usize,
    layout: OfficialChatLayout,
    profile_dir: &Path,
) -> Result<(), String> {
    let (_, url_text) = OFFICIAL_CHAT_TABS
        .get(index)
        .ok_or_else(|| format!("官方对话页签不存在：{index}"))?;
    let label = format!("official-chat-tab-{index}");
    let url = Url::parse(url_text).map_err(|e| format!("非法页签地址：{e}"))?;
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .data_directory(profile_dir.to_path_buf())
        .additional_browser_args(OFFICIAL_CHAT_BROWSER_ARGS)
        .initialization_script(include_str!("titlebar-pulse.js"))
        .initialization_script(include_str!("chat-fingerprint.js"));
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(OFFICIAL_CHAT_DATA_STORE_IDENTIFIER);
    }
    window
        .add_child(
            builder,
            LogicalPosition::new(0.0, layout.content_y),
            LogicalSize::new(layout.width, layout.content_height),
        )
        .map_err(|e| format!("无法创建官方对话页签：{e}"))?;
    Ok(())
}

/// Lazily create a requested content tab on a worker thread. `Window::add_child`
/// synchronously dispatches to the event-loop thread, so calling it directly
/// from a command can deadlock on Windows.
fn ensure_official_chat_tab(
    app: &AppHandle,
    index: usize,
    profile_dir: &Path,
    layout: OfficialChatLayout,
) -> Result<(), String> {
    let window = app
        .get_window(OFFICIAL_CHAT_WINDOW_LABEL)
        .ok_or("官方对话窗口未打开".to_string())?;
    if app
        .get_webview(&format!("official-chat-tab-{index}"))
        .is_some()
    {
        return Ok(());
    }
    let profile_dir = profile_dir.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dsh-create-official-chat-tab".into())
        .spawn(move || {
            let result = (|| {
                fs::create_dir_all(&profile_dir)
                    .map_err(|e| format!("无法创建官方对话数据目录：{e}"))?;
                add_official_chat_tab(&window, index, layout, &profile_dir)
            })();
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "官方对话页签创建线程已结束，未返回结果".to_string())?
}

/// Open the DeepSeek official chat in a tabbed window.
///
/// One bare `tauri::Window` (label [`OFFICIAL_CHAT_WINDOW_LABEL`]) hosts a
/// pinned tab-strip webview ([`OFFICIAL_CHAT_STRIP_LABEL`], the local SPA
/// route `index.html?chatstrip=1`) plus one lazily-created content webview per
/// entry in [`OFFICIAL_CHAT_TABS`]. The default content webview is created on
/// open; other remote pages are attached when selected. Only the active
/// content webview is shown, and attached pages keep their state across tab
/// switches. [`relayout_official_chat`] keeps the strip pinned to the top and
/// the content webviews filling the area below on every resize.
///
/// Built on a fresh OS thread (same Windows-deadlock reasoning as
/// [`open_harness`]); the `Result<(), String>` ships back over an `mpsc`
/// channel so the `async` command only resolves after the window and every
/// child webview is registered. `Window::add_child` dispatches webview
/// creation to the main thread internally, so it must run off the Tauri
/// command thread — the dedicated builder thread satisfies that.
///
/// Login persistence is unchanged from the single-window era: every content
/// webview shares the `<data_dir>/webview-official-chat` user-data folder
/// (Windows) / [`OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`] (macOS), so cookies,
/// localStorage, and IndexedDB survive shell restarts. Storage is
/// origin-scoped by the browser, so the DeepSeek and Qianwen tabs do not
/// collide even though they share one store. WebView2 also requires
/// identical environment options per user-data folder; every content
/// webview passes the same [`OFFICIAL_CHAT_BROWSER_ARGS`], so the
/// shared-folder constraint holds. The strip webview is local SPA content,
/// so it is exempt from `chat-fingerprint.js` and keeps
/// `window.__TAURI__` for invoking [`official_chat_tabs`] /
/// [`switch_official_chat_tab`].
///
/// The window is **not** `closable(false)`: a webview to a third-party
/// origin has no kernel session to protect, so the OS chrome close button
/// should keep working. Repeat clicks reuse the existing window via
/// `app.get_window(OFFICIAL_CHAT_WINDOW_LABEL)` and re-focus it.
#[tauri::command]
pub async fn open_official_chat(app: AppHandle) -> Result<(), String> {
    let handle = app.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dsh-open-official-chat".into())
        .spawn(move || {
            let result: Result<(), String> = (|| {
                let _mutation_guard = official_chat_mutation_lock()
                    .lock()
                    .map_err(|_| "官方对话窗口状态锁已损坏".to_string())?;
                if let Some(existing) = handle.get_window(OFFICIAL_CHAT_WINDOW_LABEL) {
                    let _ = existing.set_focus();
                    return Ok(());
                }
                // WebView2 requires every environment on a user-data folder to
                // share identical options. The official-chat content webviews
                // therefore use a dedicated profile directory.
                let profile_dir = {
                    let state = handle.state::<AppState>();
                    state.data_dir.join("webview-official-chat")
                };
                fs::create_dir_all(&profile_dir)
                    .map_err(|e| format!("无法创建官方对话数据目录：{e}"))?;
                let window = {
                    let mut builder = WindowBuilder::new(&handle, OFFICIAL_CHAT_WINDOW_LABEL)
                        .title("DeepSeek 官方对话")
                        .inner_size(OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT)
                        .resizable(true)
                        // Let AppKit establish the parent content frame before
                        // child WebViews are attached; the post-show pass
                        // reapplies every child frame after registration.
                        .visible(true);
                    // Default `TitleBarStyle::Visible` enables
                    // `NSWindowStyleMask::FullSizeContentView` on macOS, which
                    // extends the window's content view UNDER the title bar
                    // (tauri-runtime-wry/src/lib.rs:1200-1205). The tab-strip
                    // child WebView sits at logical (0, 0) and was therefore
                    // occluded by the ~28pt title bar — the three tabs
                    // (`DeepSeek`/`千问`/`MiniMax`) showed only a few pixels
                    // tall, exactly the user-reported symptom. `Transparent`
                    // keeps the title bar visible but disables
                    // `fullsize_content_view`, so the content view starts
                    // BELOW the title bar; the strip is no longer hidden.
                    // `title_bar_style` only exists on macOS (`WindowBuilder`
                    // wraps it under `#[cfg(target_os = "macos")]`) — Windows
                    // and Linux keep the platform default behavior untouched.
                    #[cfg(target_os = "macos")]
                    {
                        builder = builder.title_bar_style(tauri::TitleBarStyle::Transparent);
                    }
                    builder
                        .build()
                        .map_err(|e| format!("无法创建官方对话窗口：{e}"))?
                };
                let scale = window.scale_factor().unwrap_or(1.0);
                // AppKit can briefly report a tiny provisional client size while
                // it finishes laying out the newly created content view.
                let phys = window.inner_size().unwrap_or_default();
                let (w, h) = official_chat_initial_size(phys.width, phys.height, scale);
                let layout = official_chat_layout(w, h);
                #[cfg(debug_assertions)]
                eprintln!(
                    "dsh-desktop: official-chat created — inner={}x{}px scale={scale} → logical={w}x{h}pt",
                    phys.width, phys.height
                );

                // Register before child creation so geometry or focus events
                // emitted during attachment are handled. The focused pass reads
                // the final content-view size after AppKit finishes its layout.
                let app_for_layout = handle.clone();
                window.on_window_event(move |event| {
                    if should_relayout_official_chat(event) {
                        #[cfg(debug_assertions)]
                        eprintln!("dsh-desktop: official-chat event {event:?} — relayout");
                        relayout_official_chat(&app_for_layout);
                    }
                });

                // Create only the default content tab on open. Other remote
                // pages are attached by switch_official_chat_tab on demand;
                // once attached they keep the same persistent profile and
                // remain mounted for the lifetime of this window.
                add_official_chat_tab(&window, 0, layout, &profile_dir)?;

                // Tab strip: local SPA route renders the tab bar and keeps
                // `window.__TAURI__` so it can invoke the tab commands. The
                // pull-string lamp is rendered in this same 38px WebView. The
                // strip is added after all content views so it remains topmost.
                let strip_builder = WebviewBuilder::new(
                    OFFICIAL_CHAT_STRIP_LABEL,
                    WebviewUrl::App("index.html?chatstrip=1".into()),
                )
                .initialization_script(include_str!("pullstring-launcher.js"));
                window
                    .add_child(
                        strip_builder,
                        LogicalPosition::new(0.0, 0.0),
                        LogicalSize::new(layout.width, layout.strip_height),
                    )
                    .map_err(|e| format!("无法创建官方对话页签栏：{e}"))?;

                // Keep an idempotent show call in the queued main-thread pass,
                // then relayout after all child views are registered. The second
                // queued task runs after the show message and avoids observing a
                // provisional AppKit frame during child creation.
                let app_for_post_show = handle.clone();
                let window_for_show = window.clone();
                let _ = window.run_on_main_thread(move || {
                    let _ = window_for_show.show();
                    let window_for_relayout = window_for_show.clone();
                    let _ = window_for_relayout.run_on_main_thread(move || {
                        relayout_official_chat(&app_for_post_show);
                    });
                });

                // AppKit can keep reporting the provisional client size through the
                // post-show pass, and the real frame settles without a
                // subsequent `Resized` event. The delayed pass reapplies the
                // settled layout so the window cannot stay stuck on the
                // provisional size; idempotent and a no-op if the window is
                // already closed or the layout was already applied.
                let app_for_settle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    let app = app_for_settle.clone();
                    let _ = app_for_settle.run_on_main_thread(move || {
                        relayout_official_chat(&app);
                    });
                });
                Ok(())
            })();
            if let Err(ref e) = result {
                eprintln!("dsh-desktop: failed to open official chat window: {e}");
            }
            let _ = tx.send(result);
        })
        .map_err(|e| e.to_string())?;
    let built = tauri::async_runtime::spawn_blocking(move || rx.recv().ok())
        .await
        .map_err(|e| e.to_string())?;
    match built {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e),
        None => Err("官方对话窗口创建线程已结束，未返回结果".to_string()),
    }
}

/// Convert Tao's physical client-area dimensions to logical points.
fn logical_window_size(width: u32, height: u32, scale: f64) -> Option<(f64, f64)> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let width = width as f64 / scale;
    let height = height as f64 / scale;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some((width, height))
}

fn official_chat_initial_size(width: u32, height: u32, scale: f64) -> (f64, f64) {
    logical_window_size(width, height, scale)
        .filter(|(width, height)| {
            *width >= OFFICIAL_CHAT_STRIP_HEIGHT && *height >= OFFICIAL_CHAT_STRIP_HEIGHT
        })
        .unwrap_or((OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OfficialChatLayout {
    width: f64,
    height: f64,
    strip_height: f64,
    content_y: f64,
    content_height: f64,
}

fn official_chat_layout(width: f64, height: f64) -> OfficialChatLayout {
    let width = width.max(0.0);
    let height = height.max(0.0);
    OfficialChatLayout {
        width,
        height,
        strip_height: OFFICIAL_CHAT_STRIP_HEIGHT.min(height),
        content_y: OFFICIAL_CHAT_STRIP_HEIGHT.min(height),
        content_height: (height - OFFICIAL_CHAT_STRIP_HEIGHT).max(0.0),
    }
}

/// Whether a logical layout is plausible enough to apply to the child
/// webviews. AppKit can report a tiny provisional client size right after
/// a macOS window is created; applying such a layout would re-shrink the
/// strip and content webviews on every relayout. Mirrors the initial-size
/// fallback in [`official_chat_initial_size`]; the real layout bug fix is
/// the title-bar style on the window builder (see `open_official_chat`).
fn official_chat_layout_plausible(layout: OfficialChatLayout) -> bool {
    layout.width >= OFFICIAL_CHAT_STRIP_HEIGHT && layout.height >= OFFICIAL_CHAT_STRIP_HEIGHT
}

/// Events that can invalidate the native child-view frames.
#[derive(Clone, Copy)]
enum OfficialChatRelayoutTrigger {
    Geometry,
    Focused(bool),
    Other,
}

fn should_relayout_for_trigger(trigger: OfficialChatRelayoutTrigger) -> bool {
    matches!(
        trigger,
        OfficialChatRelayoutTrigger::Geometry | OfficialChatRelayoutTrigger::Focused(true)
    )
}

/// Return whether a native window event can change the child-view geometry.
fn should_relayout_official_chat(event: &WindowEvent) -> bool {
    let trigger = match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            OfficialChatRelayoutTrigger::Geometry
        }
        WindowEvent::Focused(focused) => OfficialChatRelayoutTrigger::Focused(*focused),
        _ => OfficialChatRelayoutTrigger::Other,
    };
    should_relayout_for_trigger(trigger)
}

fn relayout_official_chat(app: &AppHandle) {
    let Some(window) = app.get_window(OFFICIAL_CHAT_WINDOW_LABEL) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let Some(phys) = window.inner_size().ok() else {
        return;
    };
    let Some((w, h)) = logical_window_size(phys.width, phys.height, scale) else {
        return;
    };
    let layout = official_chat_layout(w, h);
    if !official_chat_layout_plausible(layout) {
        // AppKit still reports the provisional client size right after
        // creation; keep the child frames created at open time and wait for
        // a later pass with the settled size (one is queued above, plus the
        // 1.5s settle fallback). This is cheap defense, not a fix for the
        // user-visible bug — the real bug is the title-bar overlap below.
        #[cfg(debug_assertions)]
        eprintln!(
            "dsh-desktop: official-chat provisional inner={}x{}px scale={scale} → {w}x{h}pt; keeping existing child frames",
            phys.width, phys.height
        );
        return;
    }
    if layout.width <= 0.0 || layout.height <= 0.0 {
        return;
    }
    #[cfg(debug_assertions)]
    eprintln!(
        "dsh-desktop: official-chat relayout — inner={}x{}px scale={scale} → {w}x{h}pt, strip={}pt, content={}pt",
        phys.width, phys.height, layout.strip_height, layout.content_height
    );
    if let Some(strip) = app.get_webview(OFFICIAL_CHAT_STRIP_LABEL) {
        let _ = strip.set_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(layout.width, layout.strip_height).into(),
        });
    }
    for (i, _) in OFFICIAL_CHAT_TABS.iter().enumerate() {
        if let Some(wv) = app.get_webview(&format!("official-chat-tab-{i}")) {
            let _ = wv.set_bounds(Rect {
                position: LogicalPosition::new(0.0, layout.content_y).into(),
                size: LogicalSize::new(layout.width, layout.content_height).into(),
            });
        }
    }
}

/// One tab in the official-chat tab strip, as the strip webview renders it.
#[derive(Serialize)]
pub struct OfficialChatTab {
    pub index: usize,
    pub title: String,
}

/// Return the fixed tab list for the strip webview to render. Read-only:
/// the strip calls this on mount, then [`switch_official_chat_tab`] on
/// click. Defined as a command (not a compile-time constant the SPA would
/// have to duplicate) so the tab list lives in one place.
#[tauri::command]
pub fn official_chat_tabs() -> Vec<OfficialChatTab> {
    OFFICIAL_CHAT_TABS
        .iter()
        .enumerate()
        .map(|(index, (title, _))| OfficialChatTab {
            index,
            title: (*title).to_string(),
        })
        .collect()
}

/// Switch the active tab in the official-chat window.
///
/// A tab is created on first selection. Creation happens before any existing
/// tab is hidden, so a failed WebView initialization leaves the current page
/// usable. The lifecycle lock serializes open, switch, and close operations.
#[tauri::command]
pub async fn switch_official_chat_tab(app: AppHandle, index: usize) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || switch_official_chat_tab_blocking(app, index))
        .await
        .map_err(|e| e.to_string())?
}

fn switch_official_chat_tab_blocking(app: AppHandle, index: usize) -> Result<(), String> {
    if index >= OFFICIAL_CHAT_TABS.len() {
        return Err(format!("官方对话页签不存在：{index}"));
    }
    let _mutation_guard = official_chat_mutation_lock()
        .lock()
        .map_err(|_| "官方对话窗口状态锁已损坏".to_string())?;
    let window = app
        .get_window(OFFICIAL_CHAT_WINDOW_LABEL)
        .ok_or("官方对话窗口未打开".to_string())?;
    let target_label = format!("official-chat-tab-{index}");
    if app.get_webview(&target_label).is_none() {
        let profile_dir = {
            let state = app.state::<AppState>();
            state.data_dir.join("webview-official-chat")
        };
        let layout = current_official_chat_layout(&window);
        ensure_official_chat_tab(&app, index, &profile_dir, layout)?;
    }

    for (i, _) in OFFICIAL_CHAT_TABS.iter().enumerate() {
        if let Some(wv) = app.get_webview(&format!("official-chat-tab-{i}")) {
            if i == index {
                let _ = wv.show();
                let _ = wv.set_focus();
            } else {
                let _ = wv.hide();
            }
        }
    }
    relayout_official_chat(&app);
    Ok(())
}

/// Close the DeepSeek official chat window (and all its tab webviews) if it
/// is currently open. Returns an error when the window was never opened (or
/// was already torn down by the OS chrome close button) so the panel's
/// toggle can surface a sensible message instead of silently no-op'ing.
/// Destroying the bare window tears down its child webviews; the persistent
/// data store survives, so the next open reuses the saved login. The button
/// label flips back to "打开官方对话" on the next status poll once the
/// window is gone.
#[tauri::command]
pub async fn close_official_chat(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || close_official_chat_blocking(app))
        .await
        .map_err(|e| e.to_string())?
}

fn close_official_chat_blocking(app: AppHandle) -> Result<(), String> {
    let _mutation_guard = official_chat_mutation_lock()
        .lock()
        .map_err(|_| "官方对话窗口状态锁已损坏".to_string())?;
    app.get_window(OFFICIAL_CHAT_WINDOW_LABEL)
        .ok_or("官方对话窗口未打开".to_string())?
        .destroy()
        .map_err(|e| e.to_string())
}

/// Tear down the whole shell after the user confirmed a full quit from the
/// request-quit-confirm prompt. The window-close interceptor in `lib::run`
/// calls `prevent_close()` first so this `destroy()` is the only thing
/// that lets the OS X button actually close the management panel; the
/// `RunEvent::Exit` handler then reaps any kernel leftovers via pid file.
///
/// The confirmed quit must close **every** window itself rather than
/// delegating to the `RunEvent::Exit` handler: that event only fires once
/// the whole event loop terminates, which on Windows/Linux requires the
/// last window to be gone and on macOS happens only on an explicit quit
/// (closing all windows keeps the app alive). Leaving `official-chat` (or
/// any transient window) for the Exit branch would strand it — and with it
/// the whole app on macOS — after the panel is already gone. So destroy
/// the transient windows first, then the main window, then exit the loop
/// itself so the Exit branch still runs on every platform.
#[tauri::command]
pub async fn confirm_close_shell(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || confirm_close_shell_blocking(app))
        .await
        .map_err(|e| e.to_string())?
}

fn confirm_close_shell_blocking(app: AppHandle) -> Result<(), String> {
    let _mutation_guard = official_chat_mutation_lock()
        .lock()
        .map_err(|_| "官方对话窗口状态锁已损坏".to_string())?;
    let main = app
        .get_webview_window("main")
        .ok_or("主壳窗口不存在（label: main）")?;
    for label in ["official-chat", "harness", "log-viewer"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.destroy();
        }
    }
    main.destroy().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// Move `window` so its top-left corner sits just below-right of the
/// logical screen point `(x, y)`, clamped inside the monitor containing
/// the point so the panel never lands off-screen. Monitor geometry is
/// converted to logical units per monitor (`position`/`size` are physical,
/// `scale_factor` bridges the two); when no monitor contains the point —
/// stale coordinates after a display change — the primary monitor (or the
/// first enumerated one) is used instead.
fn reposition_near(app: &AppHandle, window: &tauri::WebviewWindow, x: f64, y: f64) {
    let Ok(monitors) = app.available_monitors() else {
        return;
    };
    let containing = monitors.iter().find(|m| {
        let s = m.scale_factor();
        let p = m.position();
        let sz = m.size();
        x >= p.x as f64 / s
            && x < (p.x as f64 + sz.width as f64) / s
            && y >= p.y as f64 / s
            && y < (p.y as f64 + sz.height as f64) / s
    });
    let monitor = match containing.cloned().or_else(|| {
        app.primary_monitor()
            .ok()
            .flatten()
            .or_else(|| monitors.first().cloned())
    }) {
        Some(m) => m,
        None => return,
    };
    let s = monitor.scale_factor();
    let p = monitor.position();
    let sz = monitor.size();
    let (mx, my) = (p.x as f64 / s, p.y as f64 / s);
    let (mw, mh) = (sz.width as f64 / s, sz.height as f64 / s);
    let win = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(480, 800));
    let (ww, wh) = (win.width as f64 / s, win.height as f64 / s);
    // Center the panel horizontally on the click's x so the pull lands at
    // the window's horizontal middle; keep the vertical anchor as before —
    // the top sits ~12px below the click (clear of the cursor) — so the
    // window drops down from the lamp rather than straddling it vertically.
    // The `.clamp(..)` keeps the window fully on the containing monitor when
    // the click is near an edge; the `.max(m*)` guards against windows wider
    // or taller than the monitor (an otherwise inverted clamp range).
    let nx = (x - ww / 2.0).clamp(mx, (mx + mw - ww).max(mx));
    let ny = (y + 12.0).clamp(my, (my + mh - wh).max(my));
    let _ = window.set_position(tauri::LogicalPosition::new(nx, ny));
}

// --- plugins ---------------------------------------------------------------

/// Snapshot of the plugin store and per-kernel materialization state.
#[tauri::command]
pub async fn plugin_status(state: State<'_, AppState>) -> Result<plugins::PluginStatus, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::load(&data_dir);
        plugins::status(&data_dir, &settings)
    })
    .await
    .map_err(|e| e.to_string())
}

/// List every plugin materialized under `kernels/<version>/plugins/`. Used
/// by the per-version tooltip in the versions panel so the user can inspect
/// exactly what each installed kernel carries on disk.
#[tauri::command]
pub async fn kernel_plugin_list(
    state: State<'_, AppState>,
    version: String,
) -> Result<Vec<plugins::KernelPluginRow>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || plugins::kernel_plugin_list(&data_dir, &version))
        .await
        .map_err(|e| e.to_string())
}

/// Shared body of the plugin store commands: resolve pnpm against the
/// cached node probe (streaming any auto-install progress), then run the
/// `plugins::` operation on a blocking worker with progress forwarded over
/// the channel.
async fn run_plugin_command(
    app: AppHandle,
    on_event: Channel<String>,
    op: impl FnOnce(&Path, &settings::Settings, &Path, &mut dyn FnMut(&str)) -> Result<(), AppError>
        + Send
        + 'static,
) -> Result<(), String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let settings = settings::load(&data_dir);
        let state = app.state::<AppState>();
        let node_info = cached_node(&state, &settings);
        let promise_send = on_event.clone();
        let (_, pnpm_exe) = promise_pnpm(&data_dir, &node_info, move |msg| {
            let _ = promise_send.send(msg.to_string());
        })?;
        let mut progress = |msg: &str| {
            let _ = on_event.send(msg.to_string());
        };
        op(&data_dir, &settings, &pnpm_exe, &mut progress).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install a community plugin (npm package name or git URL) into the
/// central store, materialize it into every kernel, and wire the profile.
///
/// `mode` is the materialization mode at install time. It is optional so
/// callers do not have to pick at install time — the Installed list owns
/// the mode-toggle surface (`plugin_set_mode`), and `plugins::install`
/// already falls back to `link` when the caller passes anything other
/// than `copy`.
#[tauri::command]
pub async fn plugin_install(
    app: AppHandle,
    spec: String,
    mode: Option<String>,
    on_event: Channel<String>,
) -> Result<(), String> {
    let mode = mode.unwrap_or_else(|| String::from("link"));
    run_plugin_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, progress| {
            plugins::install(data_dir, settings, pnpm_exe, &spec, &mode, progress).map(|_| ())
        },
    )
    .await
}

/// Fetch the latest version of one installed plugin and re-materialize.
#[tauri::command]
pub async fn plugin_update(
    app: AppHandle,
    id: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_plugin_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, progress| {
            plugins::update(data_dir, settings, pnpm_exe, &id, progress).map(|_| ())
        },
    )
    .await
}

/// Uninstall a plugin everywhere (store, kernels, profile wiring).
#[tauri::command]
pub async fn plugin_uninstall(
    app: AppHandle,
    id: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_plugin_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, progress| {
            plugins::uninstall(data_dir, settings, pnpm_exe, &id, progress)
        },
    )
    .await
}

/// Re-materialize everything and re-wire the profile (「同步」button).
#[tauri::command]
pub async fn plugin_sync(app: AppHandle, on_event: Channel<String>) -> Result<(), String> {
    run_plugin_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, progress| {
            plugins::sync_all(data_dir, settings, pnpm_exe, progress)
        },
    )
    .await
}

/// Switch a plugin's materialization mode (link/copy) and re-sync.
#[tauri::command]
pub async fn plugin_set_mode(
    app: AppHandle,
    id: String,
    mode: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_plugin_command(
        app,
        on_event,
        move |data_dir, settings, pnpm_exe, progress| {
            plugins::set_mode(data_dir, settings, pnpm_exe, &id, &mode, progress)
        },
    )
    .await
}

/// Check every installed plugin against its origin for newer versions.
#[tauri::command]
pub async fn plugin_check_updates(
    state: State<'_, AppState>,
) -> Result<Vec<plugins::UpdateInfo>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || plugins::check_updates(&data_dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// The full community catalog; search and filtering happen in the UI over
/// this cached list. `force` bypasses the cache window (「刷新目录」).
#[tauri::command]
pub async fn plugin_catalog(
    state: State<'_, AppState>,
    force: bool,
) -> Result<Vec<plugins::CatalogItem>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || plugins::catalog(&data_dir, force))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Resolve one quarantined plugin after a boot incident.
///
/// - `remove`: full uninstall (store, kernel materializations, profile
///   wiring); the quarantine record goes with it.
/// - `enable`: drop the quarantine record and re-wire immediately. A running
///   kernel keeps its current plugin set until the next restart — the UI
///   tells the user so, because re-enabling a genuinely broken plugin will
///   simply reproduce the boot failure on that restart (the guard runs
///   again, nothing is lost).
#[tauri::command]
pub async fn plugin_resolve(
    app: AppHandle,
    id: String,
    action: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    match action.as_str() {
        "remove" => {
            run_plugin_command(
                app,
                on_event,
                move |data_dir, settings, pnpm_exe, progress| {
                    plugins::uninstall(data_dir, settings, pnpm_exe, &id, progress)
                },
            )
            .await
        }
        "enable" => {
            let data_dir = app.state::<AppState>().data_dir.clone();
            tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
                quarantine::remove(&data_dir, &id).map_err(|e| e.to_string())?;
                let settings = settings::load(&data_dir);
                let state = app.state::<AppState>();
                let node_info = cached_node(&state, &settings);
                // Re-wiring needs pnpm; nothing to stream since this path has
                // no long install — only the profile resync runs.
                let mut noop = |_: &str| {};
                let (_, pnpm_exe) = promise_pnpm(&data_dir, &node_info, &mut noop)?;
                plugins::ensure_wiring(&data_dir, &settings, &pnpm_exe, &mut noop)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        }
        other => Err(format!("未知操作 {other:?}，支持 remove / enable")),
    }
}

// --- skills -----------------------------------------------------------------

/// Snapshot of the skill store and per-skill active-root state.
#[tauri::command]
pub async fn skill_status() -> Result<skills::SkillStatus, String> {
    tauri::async_runtime::spawn_blocking(skills::status)
        .await
        .map_err(|e| e.to_string())
}

/// Shared body of the skill-store commands: run the `skills::` operation on
/// a blocking worker with progress forwarded over the channel. Skills need
/// no pnpm/profile wiring, so this stays leaner than `run_plugin_command`.
async fn run_skill_command(
    on_event: Channel<String>,
    op: impl FnOnce(&mut dyn FnMut(&str)) -> Result<(), AppError> + Send + 'static,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let mut progress = |msg: &str| {
            let _ = on_event.send(msg.to_string());
        };
        op(&mut progress).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install a skill package (npm spec, git URL, or local folder path) into
/// the central store and materialize its skills into the kernel skill root.
/// The running workbench picks the change up live through the kernel
/// watcher. The shell always asks for link mode; `ensure_entry` falls back
/// to copy on its own when symlinks are unavailable, and the actual mode
/// is reported back to the UI through `SkillRow.actual_mode`.
#[tauri::command]
pub async fn skill_install(spec: String, on_event: Channel<String>) -> Result<(), String> {
    run_skill_command(on_event, move |progress| {
        skills::install(&spec, "link", progress).map(|_| ())
    })
    .await
}

/// Fetch the latest version of one installed skill package and reconcile
/// its skills in the active root.
#[tauri::command]
pub async fn skill_update(id: String, on_event: Channel<String>) -> Result<(), String> {
    run_skill_command(on_event, move |progress| {
        skills::update(&id, progress).map(|_| ())
    })
    .await
}

/// Uninstall a skill package everywhere (active root entries + store tree).
#[tauri::command]
pub async fn skill_uninstall(id: String, on_event: Channel<String>) -> Result<(), String> {
    run_skill_command(on_event, move |progress| skills::uninstall(&id, progress)).await
}

/// Enable or disable one skill of one package (link/unlink in the root).
#[tauri::command]
pub async fn skill_set_enabled(
    id: String,
    name: String,
    enabled: bool,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_skill_command(on_event, move |progress| {
        skills::set_enabled(&id, &name, enabled, progress)
    })
    .await
}

/// Check every installed skill package against its origin for newer versions.
#[tauri::command]
pub async fn skill_check_updates() -> Result<Vec<skills::SkillUpdateInfo>, String> {
    tauri::async_runtime::spawn_blocking(skills::check_updates)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod official_chat_layout_tests {
    use super::*;

    #[test]
    fn converts_retina_pixels_to_logical_points_once() {
        assert_eq!(logical_window_size(2732, 1536, 2.0), Some((1366.0, 768.0)),);
        assert_eq!(logical_window_size(1366, 768, 0.0), None);
    }

    #[test]
    fn ignores_tiny_provisional_window_metrics_for_initial_layout() {
        assert_eq!(
            official_chat_initial_size(1366, 6, 1.0),
            (OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT),
        );
        assert_eq!(
            official_chat_initial_size(6, 768, 1.0),
            (OFFICIAL_CHAT_INITIAL_WIDTH, OFFICIAL_CHAT_INITIAL_HEIGHT),
        );
        assert_eq!(official_chat_initial_size(2732, 1536, 2.0), (1366.0, 768.0),);
    }

    #[test]
    fn reserves_the_strip_once_for_content() {
        let layout = official_chat_layout(1366.0, 768.0);

        assert_eq!(layout.width, 1366.0);
        assert_eq!(layout.height, 768.0);
        assert_eq!(layout.strip_height, OFFICIAL_CHAT_STRIP_HEIGHT);
        assert_eq!(layout.content_y, OFFICIAL_CHAT_STRIP_HEIGHT);
        assert_eq!(layout.content_height, 730.0);
    }

    #[test]
    fn clamps_layout_when_window_is_shorter_than_the_strip() {
        let layout = official_chat_layout(640.0, 24.0);

        assert_eq!(layout.width, 640.0);
        assert_eq!(layout.height, 24.0);
        assert_eq!(layout.strip_height, 24.0);
        assert_eq!(layout.content_y, 24.0);
        assert_eq!(layout.content_height, 0.0);
    }

    #[test]
    fn relayout_rejects_tiny_provisional_layouts_that_collapse_macos_windows() {
        // AppKit reports a few-pixel provisional client size right after
        // window creation on macOS; applying it collapses the strip and
        // content webviews to that sliver. The relayout must keep the
        // last good frames instead.
        assert!(!official_chat_layout_plausible(official_chat_layout(
            1366.0, 3.0,
        )));
        assert!(!official_chat_layout_plausible(official_chat_layout(
            4.0, 768.0,
        )));
        // A real window is always at least as large as the tab strip.
        assert!(official_chat_layout_plausible(official_chat_layout(
            1366.0, 768.0,
        )));
    }

    #[test]
    fn every_official_chat_tab_uses_the_same_content_region() {
        let layout = official_chat_layout(1366.0, 768.0);
        let regions: Vec<_> = OFFICIAL_CHAT_TABS
            .iter()
            .map(|_| (layout.width, layout.content_y, layout.content_height))
            .collect();

        assert_eq!(regions.len(), 3);
        assert!(regions.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn relayouts_for_geometry_events_but_not_focus_loss() {
        assert!(should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Geometry,
        ));
        assert!(should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Focused(true),
        ));
        assert!(!should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Focused(false),
        ));
        assert!(!should_relayout_for_trigger(
            OfficialChatRelayoutTrigger::Other,
        ));

        assert!(should_relayout_official_chat(&WindowEvent::Resized(
            tauri::PhysicalSize::new(1366, 768),
        )));
        assert!(!should_relayout_official_chat(&WindowEvent::Focused(false)));
    }
}
