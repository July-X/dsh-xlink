//! 支撑管理面板 UI 的 Tauri 命令。
//!
//! 所有命令都针对共享的 [`AppState`]（数据目录加上正在运行的内核子
//! 进程）以及持久化的 `settings.json` 工作。长时间运行的操作（内核
//! 安装）会放到主线程之外，并通过 `tauri::ipc::Channel` 汇报进度。

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
use crate::process::{build_log_kind, read_tail, LogSpec};
use crate::quarantine;
use crate::{guard, kernel, node, plugins, releases, settings, skills, updater};

/// `open_official_chat` 加载到专用 `official-chat` webview 中的
/// DeepSeek 官方对话入口。
///
/// 该窗口不覆盖用户代理：WebView2 引擎本身就是真正的桌面 Edge/Chromium
/// 构建。覆盖 UA 字符串会在请求头里声称是 Chrome，但 `Sec-CH-UA` 客户
/// 端提示和原生 `navigator.userAgentData` 仍然报出真正的 Edge 品牌——
/// 这种跨层不一致正是环境检测会盯上的东西，所以诚实的身份也是一致
/// 的身份。
pub const OFFICIAL_CHAT_URL: &str = "https://chat.deepseek.com";

// WKWebView 在 macOS 上把 cookies 和 localStorage 存到这个标识符下。
// 在跨发布版之间保持 ID 稳定，并区分别名为 debug 与 release 的数据。
#[cfg(all(target_os = "macos", debug_assertions))]
const OFFICIAL_CHAT_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-dev-001";
#[cfg(all(target_os = "macos", not(debug_assertions)))]
const OFFICIAL_CHAT_DATA_STORE_IDENTIFIER: [u8; 16] = *b"dsh-chat-rel-001";

/// 传给 `official-chat` webview 的 Chromium feature 开关。
///
/// `additional_browser_args` 会**替换** wry 自带的默认集合
/// （`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`），
/// 因此这里把相关条目重新声明一遍，避免悄悄被重新启用：少了这些项
/// 之后，WebView2 会显示 SmartScreen 拦截页以及只有 Edge 才有的浮层
/// UI，而普通桌面 Chrome 是不会有这些东西的。在此基础上，
/// `AutomationControlled`（既作为浏览器 feature，又作为 blink runtime
/// 标志位）阻止 Chromium 在引擎层就上报 `navigator.webdriver = true`，
/// 让任何 initialization_script 都没机会遮盖它；`TranslateUI` /
/// `InterestFeedContentSuggestions` 则压制更多 Edge-only 的界面。只有
/// WebView2 后端会消费这些浏览器参数；macOS / Linux 会忽略它们，因此
/// builder 的接线不必分平台分支。同一个 user-data 目录必须配一致的参
/// 数（per-folder options），这也是 [`open_official_chat`] 把这个常
/// 量与专用 user-data 目录配对使用的原因。
pub const OFFICIAL_CHAT_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,AutomationControlled,TranslateUI,InterestFeedContentSuggestions --disable-blink-features=AutomationControlled";

/// 第二个官方对话页签：通义千问（qianwen）。
pub const OFFICIAL_CHAT_QIANWEN_URL: &str = "https://www.qianwen.com";

/// 第三个官方对话页签：MiniMax agent。
pub const OFFICIAL_CHAT_MINIMAX_URL: &str = "https://agent.minimaxi.com";

/// 官方对话窗口页签栏中按展示顺序排列的固定页签。第一个条目是打开时
/// 默认激活的页签。增加一行即可增加一个页签——strip webview 在运行
/// 时通过 [`official_chat_tabs`] 发现这份列表，而内容 webview 是在
/// 被选中时才惰性创建的，所以初次打开时不会加载任何其它站点。
pub const OFFICIAL_CHAT_TABS: &[(&str, &str)] = &[
    ("DeepSeek", OFFICIAL_CHAT_URL),
    ("千问", OFFICIAL_CHAT_QIANWEN_URL),
    ("MiniMax", OFFICIAL_CHAT_MINIMAX_URL),
];

/// 裸窗口的 label。一个 `Window`（不是 `WebviewWindow`）承载 strip 加
/// 每个页签对应的一个子 `Webview`；关窗时它们会一并被拆解。
const OFFICIAL_CHAT_WINDOW_LABEL: &str = "official-chat";
/// 用于渲染页签栏的本地 SPA webview（`index.html?chatstrip=1`）。它保
/// 留 `window.__TAURI__`——`chat-fingerprint.js` 不在这里注入——
/// 因此可以调用 [`official_chat_tabs`] / [`switch_official_chat_tab`]。
/// 拉绳小台灯也放在这里，因为在 Tauri 2.11 / wry 0.55.1 这版上，子
/// WebView 的透明效果并不可靠。紧凑的小台灯和页签控件可以共用同一个
/// 38px 高的 strip。
const OFFICIAL_CHAT_STRIP_LABEL: &str = "official-chat-strip";
/// 被钉在顶部的页签栏的逻辑高度。紧凑的 24×38 台灯 SVG 正好放进
/// 38px 高的页签栏中。
const OFFICIAL_CHAT_INITIAL_WIDTH: f64 = 1366.0;
const OFFICIAL_CHAT_INITIAL_HEIGHT: f64 = 768.0;
const OFFICIAL_CHAT_STRIP_HEIGHT: f64 = 38.0;

/// 以 Tauri managed state 形式注册的共享 Shell 状态。
pub struct AppState {
    pub data_dir: PathBuf,
    pub running: Mutex<Option<Child>>,
    /// 最近一次解析到的 Node 运行时，以配置的 node 路径为键。状态轮
    /// 询每几秒就会跑一次；如果每次轮询都重新探测 `node --version`，
    /// 就会产生进程派生（Windows 上进程创建开销大），但解析结果其实
    /// 只在设置改变或机器的 Node 安装变化时才会变。
    pub node_cache: Mutex<Option<(Option<String>, node::NodeInfo)>>,
}

/// 管理面板首次渲染所需的全部信息。
#[derive(Serialize)]
pub struct StatusView {
    /// 正在运行的 Shell 自身的版本（来自 tauri.conf.json）。
    pub shell_version: String,
    /// 在 debug 构建（`tauri dev`）下为 true。面板会用它把首列染上
    /// 鲸鱼眼红，让 dev shell 在屏幕上能一眼和已安装的 release shell
    /// 区分开。
    pub dev_build: bool,
    pub kernel: kernel::KernelStatus,
    pub node: node::NodeInfo,
    pub settings: settings::Settings,
    /// 启动防护已经停用的插件。概览页据此渲染横幅，使得即便工作台
    /// 跑在安全模式下也不会对缺失的内容保持沉默。
    pub quarantined: Vec<quarantine::QuarantineItem>,
    /// 最近一次启动防护的故障（如果有）。通过 `last-incident.json`
    /// 跨 Shell 重启保留下来，因此「查看详情」在重新启动之后仍然可
    /// 用，不只限于启动命令的响应中。
    pub last_incident: Option<guard::Incident>,
    /// 专用 `official-chat` webview 窗口当前是否已注册到应用。状态
    /// 轮询以同样的 2.5s 节奏观察这个标志，让面板按钮的文案在「打
    /// 开官方对话」和「关闭官方对话」之间切换而无需额外的 IPC 往返。
    pub official_chat_open: bool,
}

// 读取用于展示的定长文本文件尾部——已迁移到
// `crate::process::read_tail`，以便启动防护以同样的方式读取。
///
/// 不可被 UI 吞掉的 web-app 级错误前缀。
fn app_err(data_dir: &Path, e: impl std::fmt::Display) -> String {
    format!("{e}（数据目录：{}）", data_dir.display())
}

// --- 状态 --------------------------------------------------------------------

#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<StatusView, String> {
    let data_dir = state.data_dir.clone();
    // 文件探测和端口检查在 blocking worker 上运行：如果作为同步命令，
    // 这个轮询会每几秒就霸占 Tauri 的主线程。
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

/// 通过 per-app 缓存解析 Node 运行时；只有 `node_path` 设置发生变化
/// 时才会触发一次新的探测。
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
    // 检测会忽略任何已配置的路径：它报告的是环境自身的探测结果，这样
    // UI 就能据它预填设置。解析过程对每个环境候选（PATH + nvm 管理的
    // 安装 + 系统位置）可能派生一个子进程——把这些进程派生放到 Tauri
    // 主线程之外。
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
    // 仅取当天的轮转内核日志末尾。之所以只读当天是有意为之：用户在检
    // 测到空白页之后几秒就会点「查看前端自检证据」，他们想看的正是
    // 最新几行。较早的日期仍保留在目录和面板的页签列表中，要做更深
    // 入调查只需切换一个页签。
    let path = kernel::current_kernel_log_path(&state.data_dir);
    tauri::async_runtime::spawn_blocking(move || read_tail(&path, 16 * 1024))
        .await
        .map_err(|e| e.to_string())
}

/// 日志文件面板页签列表中的一项。
#[derive(Serialize)]
pub struct LogFileEntry {
    /// 仅文件 basename（例如 `release-kernel-2024-01-15.log`、
    /// `release-install-0.1.0-rc.6-2024-01-15.log`）；UI 把它回传给
    /// `read_log_file`。绝不暴露绝对路径——UI 运行在沙箱化的 webview
    /// 中，不应该需要绝对路径。
    pub name: String,
    /// 文件大小（字节）；面板会把它显示在页签名旁边。
    pub size: u64,
}

/// 列举 Shell 日志目录下的所有 `*.log` 文件，最新者优先。
///
/// `read_dir` 与 `metadata` 之间消失的文件会被静默跳过——安装日志会
/// 原地轮转，可能与本次扫描产生竞速。该列表涵盖 `RotatingLog` 写出
/// 的每一种「构建类型 + 名称 + 日期」组合，因此用户在面板中可以在
/// 同一份滚动里同时看到实时的内核日志以及昨天的安装尝试。
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

        // 命名规则会在每个文件上盖一个构建类型和递减日期，因此字典序的
        // 逆序排序会把最新的 `release-kernel-<today>.log` 排到列表头
        // 部。老式的裸 `kernel.log`（如果更老的 Shell 写过的话）按字
        // 母序排；如果它真的出现在头部，一次性清理会把它清掉——见下
        // 面的 `cleanup_legacy_logs`。
        out.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 读取 logs 目录下指定日志文件的尾部。
///
/// `name` 必须是纯文件名，不允许任何路径分隔符；本函数会拒绝其它形
/// 式以避免 UI 的页签列表越过 logs 目录。和 `get_kernel_log` 一样以
/// 16 KiB 作为尾部上限，使面板在面对大型安装日志时仍然保持响应。
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

/// 在操作系统文件管理器中显示 Shell 的数据目录。
///
/// 路径来源于 `AppState.data_dir`，由 `lib::setup` 通过 `kernel::data_dir`
/// 解析并在首次启动时创建，因此该目录在运行时始终存在。改成走服务
/// 端（而不是让 UI 直接调 `opener.open_path`）可以绕开 opener 插件的
/// IPC scope 检查——`opener:default` 只授予 `open_url` /
/// `reveal_item_in_dir` / 默认 URL，并不包括 `open_path`。作为插件底
/// 层的 `open` crate 按平台分发：macOS 上 `open` 启动 Finder 并选中
/// 父目录中的目标项；Windows 上 `cmd /C start ""` 直接打开该目录对应
/// 的资源管理器。
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

// --- Shell 自我更新 ---------------------------------------------------------

/// 从 GitHub 检查是否有新的 Shell 发行版（手动的「检查更新」按钮）。
#[tauri::command]
pub async fn check_shell_update(app: AppHandle) -> Result<updater::ShellUpdateInfo, String> {
    updater::check(&app).await.map_err(|e| e.to_string())
}

/// 下载、校验、安装挂起的 Shell 更新，然后重启。
#[tauri::command]
pub async fn install_shell_update(app: AppHandle, on_event: Channel<String>) -> Result<(), String> {
    updater::install(&app, move |line| {
        let _ = on_event.send(line.to_string());
    })
    .await
    .map_err(|e| e.to_string())
}

// --- 发行版 ------------------------------------------------------------------

/// 为更新菜单获取官方内核发行版列表。
#[tauri::command]
pub async fn fetch_releases() -> Result<releases::ReleaseList, String> {
    // ureq 是同步的；把这步会阻塞的 HTTPS 请求放到主线程之外。
    tauri::async_runtime::spawn_blocking(releases::list_releases)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 针对已经探测好的 node（调用方缓存的 `node::NodeInfo`）来解析
/// pnpm，缺失时通过 npm 自动安装。返回 (node_path, pnpm_exe)。
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
    // 自动安装日志与安装日志一同放在 Shell 日志目录下；`run_pnpm` 使用
    // 的同一个按日轮转的 writer 会把日志追加到当天的文件里。「构建类
    // 型 + 日期」前缀让偶尔同机并存的 dev 与 release 尝试不会互相手
    // 覆。
    let logs_dir = kernel::logs_dir(data_dir);
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pnpm_log_spec = LogSpec::new(build_log_kind(), format!("pnpm-install-{epoch}"));
    let pnpm = node::ensure_pnpm(&s, &node_dir, &logs_dir, &pnpm_log_spec, &mut on_progress)?;
    Ok((PathBuf::from(node_info.path.clone()), pnpm))
}

// --- 内核安装 / 切换 / 移除 ----------------------------------------------------

/// 从 npm 安装指定版本的内核，期间通过事件流推送进度。
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
            // `node_dir` 是已校验的 `node` 可执行文件所在目录。安装派生出的子
            // 进程需要它出现在 PATH 上，这样 pnpm 的
            // `#!/usr/bin/env node` shebang 以及任何 shell-out 调
            // `node` 的 lifecycle 脚本都能解析到它，即便 GUI 进程继承
            // 到的只是 macOS .app 包那种 launchd-only PATH——这是 nvm 管
            // 理的安装里很常见的场景。
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

    // 首次安装的内核会自动设为活动版本，之后的安装不再改动当前活动版
    // 本。
    if kernel::read_active(&data_dir).is_none() {
        kernel::set_active(&data_dir, &version).map_err(|e| e.to_string())?;
        let _ = on_event.send(format!("已切换到版本 {version}"));
    }
    if !kernel::port_open(settings::load(&data_dir).port) {
        let _ = on_event.send("正在启动内核…".to_string());
        // 与「启动工作台」按钮共用同一条受防护的启动流程：刚装好的
        // 插件若把内核搞坏必须落进隔离流程，而不是让用户在安装之后
        // 面对一个崩溃的工作台。
        let dir_for_start = data_dir.clone();
        // 通道与外层函数一起用于尾部状态消息；guarded-start worker 拿到自
        // 己的克隆。
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
    // 接线会用 pnpm 跑插件商店；把整个切换放到主线程之外。
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        // 切换在下一次启动时生效；正在运行的内核会继续提供服务，直
        // 到用户重启它。
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
    // 对内核目录（包括 node_modules）的 remove_dir_all 在 Windows 上
    // 可能耗时数秒；绝对不能在主线程上做。
    tauri::async_runtime::spawn_blocking(move || {
        kernel::uninstall(&data_dir, &version).map_err(|e| app_err(&data_dir, e))
    })
    .await
    .map_err(|e| e.to_string())?
}

// --- 内核生命周期 -----------------------------------------------------------

/// 为成功启动的内核子进程做注册：记录其 pid 以便后续重启后的 Shell
/// 回收，并把句柄保存在应用状态中。
fn register_child(state: &AppState, data_dir: &Path, child: Child) {
    kernel::write_pid(data_dir, child.id());
    crate::lock(&state.running).replace(child);
}

/// 在启动防护下启动当前活动的内核。幂等：如果端口已经有应答则返回
/// 一份 no-op 报告。
///
/// 防护会一直监听派生出的进程直至端口就绪；发生启动失败时，它会基于
/// 内核日志把崩溃归因到已安装的插件，隔离可疑项（必要时隔离所有第
/// 三方插件），在两次尝试之间重新接线，并无论如何都上报一份
/// [`guard::Incident`]，让 UI 能询问用户保留或移除哪些项。进度消息通
/// 过 `on_event` 流式推送，因为受防护的重试里会包含 pnpm 步骤，最坏
/// 情况下可能耗时几分钟。
#[tauri::command]
pub async fn start_kernel(
    app: AppHandle,
    on_event: Channel<String>,
) -> Result<guard::StartReport, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    // 接线和子进程派生都是阻塞的（pnpm、进程创建）；把它们放到 blocking
    // worker 上，而不是 Tauri 的主线程。
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
        // 受防护的重试会通过 pnpm 重新接线插件；预先解析 pnpm，使得工具链
        // 缺失时能在第一次尝试前就失败，而不是在流程中途才报错。
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

/// 停止内核并关闭工作台窗口，让 UI 的「关闭工作台」能拆掉整个工作台，
/// 而不是留下一个死掉的 webview。如果 Shell 在派生该内核之后又重启
/// 过，内存中的子进程句柄已经不在了，但 pid 文件里还记录着要回收的
/// 进程。
///
/// 工作台窗口在创建时使用 `closable(false)`（见 `open_harness`），
/// 因此系统标题栏的关闭按钮是禁用的，在任务中途意外点击不会打断用户
/// 的会话。但通过这条命令显式回到关闭路径仍然必须工作，所以窗口走
/// `destroy()`——强制让系统关掉窗口，而不理会 closable 标志——而不
/// 是 `close()`，后者会被它自己设置的标志挡住。
#[tauri::command]
pub async fn stop_kernel(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("harness") {
        let _ = window.destroy();
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    // kernel::stop 会等待子进程退出（最多等满它的 kill 超时），把这
    // 段等待放到主线程之外。
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
            // 先尝试 pid 文件——内存中的句柄已经在 Shell 重启后丢失了，但之
            // 前的 Shell 已经把 pid 写到了 <data_dir>/kernel.pid，且它
            // 派生的内核仍然绑在该端口上。kill_pid 在发信号前会先校
            // 验 pid 仍指向一个 dsh 内核，因此被回收给无关进程的 pid
            // 是一个 no-op。
            let mut killed = false;
            if let Some(pid) = kernel::read_pid(&data_dir) {
                if kernel::pid_is_kernel(pid, Some(port)) {
                    kernel::kill_pid(pid, Some(port));
                    killed = true;
                }
            }
            // 兜底：当 dev/release Shell 并存，且内存中的子进程和 pid 文件都
            // 不存在时（例如 start_maybe 因端口已被另一个 Shell 的内
            // 核占用而跳过启动），Shell 在自身记录中找不到监听者。
            // 通过端口反查拿到它的 pid，再用同样的 pid_is_kernel 防
            // 护过滤一遍，这样即便回收到的 pid 恰好指向一个无关进
            // 程，也仍然不会被误杀。
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

/// 接收来自 harness webview 的一次性健康报告，并按当前内核日志对其进
/// 行归因。返回的故障也会发送给管理面板，这样空白页面不会再无声失
/// 败。
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
/// 该命令在 blocking worker 上读取配置并探测端口。Webview 的创建仍然
/// 在一个新的 OS 线程上进行：在 Tauri 命令里同步构造 window 在
/// Windows 上可能死锁，把 builder 排除在 async executor 之外在所有平台
/// 上都更安全。
///
/// 窗口在创建时使用 `closable(false)`，因此系统标题栏的关闭按钮会被
/// 灰掉：在长任务中途意外点击就不会打断用户会话。经过 `stop_kernel`
/// 显式回到关闭路径仍然有效，因为该命令用的是 `destroy()` 而不是
/// `close()`，能在 chrome 关闭按钮被禁用的前提下，强制让系统完成拆
/// 解。Linux GTK+ 后端是文档记载的例外：它可能不会对已经可见的窗口灰
/// 掉按钮，所以在 Linux 上这只是行为暗示，不是硬保证。
/// 打开 dsh web 工作台窗口。原生标题栏保持 macOS / Windows / Linux 标
/// 准的窗口装饰，而不是用 Overlay，这样系统级的拖动 / 调整大小 / 双
/// 击最大化可以稳定工作（通过 `start_dragging` IPC 的 WKWebView 拖动
/// 区域路径在 Tauri 2.11.5 上表现不稳）。标题栏脉冲由 Shell 端而非内
/// 核的 `packages/client/web/src/base.css` 拥有，通过
/// `initialization_script(titlebar-pulse.js)` 注入；脚本会附上一个带
/// `!important` 规则的 `<style>` 节点，使得无论内核版本是什么、也不
/// 论本脚本和工作台自身样式表的加载顺序，Shell 的覆盖都能胜出。第二个
/// 注入脚本（`pullstring-launcher.js`）渲染一个浮在工作台左上角的拉
/// 绳小台灯；拉动它会调用 [`focus_main_shell`] 把管理窗口提到当前桌
/// 面之上。第三个脚本（`sourcemap-quieter.js`）会拦截工作台对
/// `.js.map` 的请求，并以一份合成空 source map 应答——dsh 内核的 npm
/// 包故意省掉了 source-map 负载（编译产物里仍保留 `sourceMappingURL`
/// 注释），因此如果没有这个拦截器，每次打开工作台 DevTools 都会记约
/// 44 行 “Failed to load resource 404”；这个覆盖在不破坏工作台功能
/// 的前提下让它们安静下来。
/// 解析工作台 webview 应该加载的 URL，优先使用内核自带的 launch-token
/// URL。
///
/// 0.1.2-alpha.1 起的内核在 browser 入口前加上一个进程级的 launch
/// token（`dsh-client-connection` BrowserAuth）：`/?token=` 用来签发
/// 会话 cookie，裸的根请求会得到 401。token 的唯一出处就是内核启动时
/// 输出的 `dsh web: http://127.0.0.1:<port>/?token=…` 这一行，Shell
/// 会把它捕获到当天的内核日志中。每次内核重启都会追加新的一行，因此
/// **最后**匹配到的那条就是当前运行进程的 token；旧版本内核不输出
/// token，会继续接受裸的源地址，所以拿不到 token 时回退到它。
fn kernel_workbench_url(data_dir: &std::path::Path, port: u16) -> String {
    let fallback = format!("http://127.0.0.1:{port}");
    let needle = format!("http://127.0.0.1:{port}/?token=");
    // 刚刚启动的内核要等它的第一轮启动流程走完之后才输出 URL 行，
    // 因此当调用方与一次新启动竞速（「启动内核」紧跟着「打开工作
    // 台」）时，最新一行可能还没落盘。短暂轮询——过期的 token 会返
    // 回 401，结果会让工作台变成空白。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let tail = loop {
        let tail = read_tail(&kernel::current_kernel_log_path(data_dir), 16 * 1024);
        if tail.contains(&needle) || std::time::Instant::now() >= deadline {
            break tail;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let Some(start) = tail.rfind(&needle) else {
        return fallback;
    };
    let rest = &tail[start + needle.len()..];
    let Some(end) = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    else {
        return format!("{needle}{rest}");
    };
    format!("{needle}{}", &rest[..end])
}

#[tauri::command]
pub async fn open_harness(app: AppHandle) -> Result<(), String> {
    let data_dir = crate::kernel::data_dir(&app);
    let url = tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::load(&data_dir);
        if !kernel::port_open(settings.port) {
            return Err(format!(
                "内核未在运行（端口 {}），请先点击「启动工作台」",
                settings.port
            ));
        }
        Ok::<String, String>(kernel_workbench_url(&data_dir, settings.port))
    })
    .await
    .map_err(|e| e.to_string())??;
    let url = Url::parse(&url).map_err(|e| e.to_string())?;
    if let Some(existing) = app.get_webview_window("harness") {
        // 内核重启时会签发一个新的 launch token，使已经打开的窗口所持
        // 有的 URL 失效；让已有窗口执行跳转，而不仅仅是聚焦它，以免
        // 重启后工作台停在过期的 token 上变成空白。
        let _ = existing.navigate(url.clone());
        let _ = existing.set_focus();
        return Ok(());
    }
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
                .initialization_script(include_str!("sourcemap-quieter.js"))
                .build();
            if let Err(e) = result {
                eprintln!("dsh-xlink: failed to open harness window: {e}");
            }
            #[cfg(debug_assertions)]
            if let Ok(window) = app.get_webview_window("harness").ok_or("no harness window") {
                window.open_devtools();
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 在专属的可调大小查看器窗口中打开日志文件。
///
/// 管理窗口被固定为 480×800（tauri.conf.json），所以日志面板的「全
/// 屏」按钮把阅读工作交给它自己的 OS 窗口，而不是把页面内的对话框拉
/// 大。构造过程与 `open_harness` 一致：webview 在新线程上构建，因为
/// 在主线程上做这件事会在 Windows 上死锁。已有的查看器会被销毁并重
/// 建，这样打开另一个文件时不需要跨窗口消息；而该窗口是只读的，丢
/// 掉一个查看器也不会损失任何东西。
///
/// 页面是同一个 SPA：`ui/src/main.js` 在 `?log=<name>` 出现时挂载
/// 的是独立查看器，而不是管理面板；查看器自己调用 `read_log_file`
/// （capability `log-viewer.json` 仅授予该命令）。名称在这里也会经
/// 过 `read_log_file` 的校验，所以错误的名字在窗口出现前就被拒掉。
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
                eprintln!("dsh-xlink: failed to open log viewer window: {e}");
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 把 Shell 的主管理窗口提到当前桌面之上。
///
/// 由注入到工作台 webview 的拉绳小台灯（`pullstring-launcher.js`）
/// 通过 `window.__TAURI__.core.invoke` 调用，所以无论工作台运行在哪
/// 个内核版本下都能工作。`show` + `unminimize` 把窗口从隐藏或最小化
/// 状态恢复之后，再由 `set_focus` 把它放到前台；窗口被设置为不可调
/// 整大小且始终存在（tauri.conf.json），所以窗口丢失属于应当冒出来
/// 写到 webview 控制台的内部错误。
///
/// `set_focus` 之前的 always-on-top 切换是 Windows 前台锁的对策：当
/// 系统判断某个进程不能抢占前台（焦点是通过 IPC 到达，而不是直接的
/// 输入事件带来的）时，`SetForegroundWindow` 会被静默忽略，导致窗口
/// 「提到前面但仍藏在背后」。把窗口置顶再立即解除，会强制让它出现在
/// 正常 z-order 的最前面；在 macOS/Linux 上这次切换只是一个无害的
/// no-op 提升操作。
///
/// `x`/`y` 是点击事件发生位置的屏幕坐标（CSS 像素，对应
/// `MouseEvent.screenX/Y`）；如果给到，窗口会先被重定位，使点击位置
/// 的 x 落在窗口的水平中心，窗口顶部则位于点击的 y 下方一点（被夹
/// 在所在显示器范围内，确保窗口完整可见），这样用户不必再到其它显示
/// 器上寻找它。两者都是可选的，所以旧版注入脚本即便不带参数调用也
/// 仍能把窗口提到原位置。
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

/// 在 worker 线程上惰性创建所请求的内容页签。`Window::add_child` 会同
/// 步分发到事件循环线程，所以从命令里直接调用会在 Windows 上死锁。
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

/// 在带页签的窗口中打开 DeepSeek 官方对话。
///
/// 一个裸露的 `tauri::Window`（label [`OFFICIAL_CHAT_WINDOW_LABEL`]）
/// 承载一个钉在顶部的页签栏 webview（[`OFFICIAL_CHAT_STRIP_LABEL`]，
/// 本地 SPA 路由 `index.html?chatstrip=1`），再加上 [`OFFICIAL_CHAT_TABS`]
/// 中每个条目对应的一个惰性创建的内容 webview。默认的内容 webview 在
/// 打开时即创建；其它远程页面在被选中时挂载。只有处于激活状态的内容
/// webview 会被显示，已挂载的页面在页签切换之间会保留其状态。
/// [`relayout_official_chat`] 在每次 resize 时把页签栏钉在顶部，让内
/// 容 webview 填满其下方的区域。
///
/// 在一个全新的 OS 线程上构建（与 [`open_harness`] 相同的 Windows 死
/// 锁考量）；`Result<(), String>` 通过 `mpsc` 通道回传，因此 `async`
/// 命令只有在窗口以及每个子 webview 都注册完成之后才会 resolve。
/// `Window::add_child` 会在内部把 webview 创建派发到主线程，因此它
/// 必须从 Tauri 命令线程之外执行——专门的 builder 线程刚好满足这点。
///
/// 登录持久化沿用单窗口时代的策略：每个内容 webview 共享
/// `<data_dir>/webview-official-chat`（Windows 上的 user-data 文件
/// 夹）/ [`OFFICIAL_CHAT_DATA_STORE_IDENTIFIER`]（macOS），所以
/// cookies、localStorage、IndexedDB 都能跨 Shell 重启保留。存储由浏览
/// 器按 origin 隔离，因此即便共享同一个存储，DeepSeek 与千问页签也
/// 不会相互冲突。WebView2 还要求同一个 user-data 目录下的所有环境配
/// 置完全一致；每个内容 webview 都传入相同的
/// [`OFFICIAL_CHAT_BROWSER_ARGS`]，所以「共享文件夹」这条约束是成立
/// 的。strip webview 是本地 SPA 内容，所以它豁免
/// `chat-fingerprint.js` 的注入，保留 `window.__TAURI__` 以便调用
/// [`official_chat_tabs`] / [`switch_official_chat_tab`]。
///
/// 该窗口**不**设置 `closable(false)`：第三方源的 webview 没有内核
/// 会话需要保护，所以系统的 chrome 关闭按钮应继续工作。重复点击会通
/// 过 `app.get_window(OFFICIAL_CHAT_WINDOW_LABEL)` 复用已有窗口并重
/// 新聚焦。
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
                // WebView2 要求同一个 user-data 目录下的所有环境必须使用完全一致
                // 的选项。官方对话的内容 webview 因此使用一个专门的
                // profile 目录。
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
                        // 让 AppKit 在挂载子 WebView 之前先把父内容视图的 frame 确定下来；
                        // post-show 那一轮再根据注册结果重新设置每个子视
                        // 图的 frame。
                        .visible(true);
                    // 默认的 `TitleBarStyle::Visible` 在 macOS 上会启用
                    // `NSWindowStyleMask::FullSizeContentView`，把窗口
                    // 的 content view 延伸到标题栏之下
                    // （tauri-runtime-wry/src/lib.rs:1200-1205）。页签
                    // 栏子 WebView 位于逻辑 (0, 0)，因此被约 28pt 高
                    // 的标题栏遮挡——三个页签（`DeepSeek` / `千问` /
                    // `MiniMax`）只剩几像素高，正好是用户反馈的现象。
                    // `Transparent` 保留标题栏的可见，但禁用了
                    // `fullsize_content_view`，于是 content view 从
                    // 标题栏下方开始；页签栏不再被遮住。
                    // `title_bar_style` 只在 macOS 上存在
                    // （`WindowBuilder` 把它包在
                    // `#[cfg(target_os = "macos")]` 下）——Windows 和
                    // Linux 上的平台默认行为保持不变。
                    #[cfg(target_os = "macos")]
                    {
                        builder = builder.title_bar_style(tauri::TitleBarStyle::Transparent);
                    }
                    builder
                        .build()
                        .map_err(|e| format!("无法创建官方对话窗口：{e}"))?
                };
                let scale = window.scale_factor().unwrap_or(1.0);
                // AppKit 在完成刚创建 content view 的布局之前，可能短暂回报一个
                // 非常小的临时 client size。
                let phys = window.inner_size().unwrap_or_default();
                let (w, h) = official_chat_initial_size(phys.width, phys.height, scale);
                let layout = official_chat_layout(w, h);
                #[cfg(debug_assertions)]
                eprintln!(
                    "dsh-xlink: official-chat created — inner={}x{}px scale={scale} → logical={w}x{h}pt",
                    phys.width, phys.height
                );

                // 在挂载子视图前先注册窗口，这样挂载期间发出的几何或焦点事件都可
                // 以被处理。聚焦那一轮会在 AppKit 完成布局之后读取最终
                // 的 content view 大小。
                let app_for_layout = handle.clone();
                window.on_window_event(move |event| {
                    if should_relayout_official_chat(event) {
                        #[cfg(debug_assertions)]
                        eprintln!("dsh-xlink: official-chat event {event:?} — relayout");
                        relayout_official_chat(&app_for_layout);
                    }
                });

                // 打开时仅创建默认的内容页签。其它远程页面在被选中时由
                // switch_official_chat_tab 按需挂载；一旦挂载，它们保
                // 持同一份持久 profile，并在该窗口的生命周期内一直挂
                // 着。
                add_official_chat_tab(&window, 0, layout, &profile_dir)?;

                // 页签栏：本地 SPA 路由渲染页签栏并保留 `window.__TAURI__`，使其能
                // 调用页签命令。拉绳小台灯也由这个 38px 高的 WebView 渲
                // 染。strip 在所有内容视图之后再添加，因此它始终位于
                // 最上层。
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

                // 把一条幂等的 show 调用放到排队到主线程的任务里，再在所有子视图
                // 注册完成后做一次 relayout。第二个排队的任务在 show 消
                // 息之后执行，从而避免在子视图创建过程中观察到
                // AppKit 的临时 frame。
                let app_for_post_show = handle.clone();
                let window_for_show = window.clone();
                let _ = window.run_on_main_thread(move || {
                    let _ = window_for_show.show();
                    let window_for_relayout = window_for_show.clone();
                    let _ = window_for_relayout.run_on_main_thread(move || {
                        relayout_official_chat(&app_for_post_show);
                    });
                });

                // AppKit 在 post-show 阶段之后仍可能持续上报临时 client size，而
                // 且不会再触发后续的 `Resized` 事件。延迟一轮重新应用稳
                // 定后的布局，使窗口不至于被卡在临时尺寸上；如果窗口
                // 已经关闭或者布局已经应用过，它是幂等的 no-op。
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
                eprintln!("dsh-xlink: failed to open official chat window: {e}");
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

/// 把 Tao 的物理 client-area 尺寸转换为逻辑点。
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

/// 判断某个逻辑布局是否合理到可以应用到子 webview。AppKit 在刚创建
/// 完一个 macOS 窗口之后可能立刻报告一个极小的临时 client size；把这
/// 种布局应用上去会在每次 relayout 时把 strip 和内容 webview 都缩回去。
/// 该判断与 [`official_chat_initial_size`] 中的初始尺寸兜底相互呼应；
/// 真正的布局 bug 修复落在 window builder 的 title-bar style 上（见
/// `open_official_chat`）。
fn official_chat_layout_plausible(layout: OfficialChatLayout) -> bool {
    layout.width >= OFFICIAL_CHAT_STRIP_HEIGHT && layout.height >= OFFICIAL_CHAT_STRIP_HEIGHT
}

/// 能使原生子视图 frame 失效的事件。
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

/// 判断某个原生 window 事件是否会改变子视图的几何信息。
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
        // AppKit 在创建后仍然上报临时 client size；保留打开时建好的子视
        // 图 frame，等之后那一轮（上面排队的那一轮，加上 1.5 秒
        // 后备那一轮）拿到稳定的尺寸再做处理。这是廉价的兜底，不是
        // 用户可见 bug 的修复——真正的 bug 是下面提到的标题栏重叠。
        #[cfg(debug_assertions)]
        eprintln!(
            "dsh-xlink: official-chat provisional inner={}x{}px scale={scale} → {w}x{h}pt; keeping existing child frames",
            phys.width, phys.height
        );
        return;
    }
    if layout.width <= 0.0 || layout.height <= 0.0 {
        return;
    }
    #[cfg(debug_assertions)]
    eprintln!(
        "dsh-xlink: official-chat relayout — inner={}x{}px scale={scale} → {w}x{h}pt, strip={}pt, content={}pt",
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

/// strip webview 渲染的官方对话页签栏里的一项。
#[derive(Serialize)]
pub struct OfficialChatTab {
    pub index: usize,
    pub title: String,
}

/// 返回 strip webview 渲染所用的固定页签列表。只读：strip 在挂载时调
/// 用它，页签点击时再调 [`switch_official_chat_tab`]。这里被定义为
/// 一条命令（而不是一份需要被 SPA 复制的编译期常量），使页签列表只
/// 存在一处。
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

/// 切换官方对话窗口中的激活页签。
///
/// 页签在首次选中时被创建。创建发生在任何已有页签被隐藏之前，因此即
/// 便某个 WebView 初始化失败也不会影响当前页面的可用性。生命周期锁
/// 把打开、切换、关闭三组操作串行化。
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

/// 如果 DeepSeek 官方对话窗口当前已打开，则关闭它（以及其全部页签
/// webview）。当窗口从未被打开过（或者已经被系统 chrome 关闭按钮提前
/// 拆解）时返回错误，让面板上的开关按钮能因此弹出一条合理的提示，
/// 而不会静默 no-op。销毁裸窗口会顺带拆解它所有的子 webview；持久
/// 化的数据存储会保留下来，因此下次打开时仍能复用已保存的登录。窗
/// 口消失后下一次状态轮询会让按钮文案重新变成「打开官方对话」。
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

/// 在用户从「确认退出」提示中确认完全退出后，拆解整个 Shell。
/// `lib::run` 中的窗口关闭拦截器会先调用 `prevent_close()`，因此这个
/// `destroy()` 才是真正让系统 X 按钮能关闭掉管理面板的唯一动作；之
/// 后 `RunEvent::Exit` 处理器会通过 pid 文件回收任何残留的内核。
///
/// 已确认的退出必须**自己**关闭每一个窗口，而不是把这件事交给
/// `RunEvent::Exit` 处理器——该事件要等到整个事件循环结束才会触发，
/// 在 Windows / Linux 上这要求最后一个窗口已经消失，在 macOS 上则
/// 只有显式退出才会发生（关掉所有窗口并不会让 app 退出）。如果把
/// `official-chat`（或任何临时窗口）丢给 Exit 分支去处理，就会在面
/// 板已经消失后把它——连同整个 macOS 上的 app——留在一侧。所以先销
/// 毁临时窗口，再到主窗口，再退出事件循环本身，这样在每个平台上
/// Exit 分支都能正常跑起来。
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

/// 移动 `window` 使其左上角位于逻辑屏幕点 `(x, y)` 的右下方一点，并
/// 夹在包含该点的显示器范围内，避免面板落到屏幕外。显示器几何按显
/// 示器换算成逻辑单位（`position` / `size` 是物理量，`scale_factor`
/// 在两者之间桥接）；当没有任何显示器包含该点（显示变更后坐标已过
/// 期）时，回退到主显示器（或第一个枚举到的显示器）。
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
    // 让面板在水平方向上以点击的 x 为中心，让拉动点落在窗口的水平中
    // 部；垂直锚点与原来一致——顶部位于点击位置下方约 12 像素处
    // （避开光标），让窗口从小台灯的下方垂下来，而不是纵向跨越台
    // 灯。`.clamp(..)` 在点击位置靠近边缘时仍保证窗口完整落在所在
    // 显示器内；`.max(m*)` 防御窗口比显示器更宽或更高的情况（不然
    // clamp 区间会反掉）。
    let nx = (x - ww / 2.0).clamp(mx, (mx + mw - ww).max(mx));
    let ny = (y + 12.0).clamp(my, (my + mh - wh).max(my));
    let _ = window.set_position(tauri::LogicalPosition::new(nx, ny));
}

// --- 插件 --------------------------------------------------------------------

/// 插件商店以及按内核粒度的物化状态快照。
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

/// 列出物化到 `kernels/<version>/plugins/` 之下的每一个插件。由版本
/// 面板里按版本悬浮提示使用，让用户能查看每个已安装内核在磁盘上实
/// 际带有什么。
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

/// 插件商店命令的共享主体：基于已经缓存好的 node 探测来解析 pnpm
/// （自动安装的进度会被转发），再在 blocking worker 上跑对应的
/// `plugins::` 操作，进度通过通道转发出去。
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

/// 把一个社区插件（npm 包名或 git URL）安装到中央商店，物化到每个内
/// 核，并完成 profile 接线。
///
/// `mode` 是安装时的物化模式。它是可选的，所以调用方不必在安装时决
/// 定——「已安装」列表拥有模式开关界面（`plugin_set_mode`），而
/// `plugins::install` 在调用方传入除 `copy` 之外的任何值时已经会回
/// 退到 `link`。
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

/// 拉取一个已安装插件的最新版本并重新物化。
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

/// 在所有位置（商店、各内核、profile 接线）卸载一个插件。
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

/// 重新物化所有内容并重新接线 profile（「同步」按钮）。
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

/// 切换一个插件的物化模式（link/copy）并重新同步。
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

/// 检查每个已安装插件在其来源处是否有更新版本。
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

/// 完整的社区目录；搜索和过滤在 UI 中基于这份缓存列表进行。`force`
/// 可以跳过缓存窗口（对应「刷新目录」）。
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

/// 处理一次启动故障中的一项被隔离插件。
///
/// - `remove`：完整卸载（商店、各内核的物化、profile 接线）；隔离
///   记录随之一起删除。
/// - `enable`：删除隔离记录并立即重新接线。已经运行的内核保持它当
///   前的插件集合，直到下次重启——UI 会向用户说明，因为重新启用
///   一个确实有问题的插件，下次重启只会再次复现启动失败（防护会再
///   跑一遍，不会丢失任何东西）。
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
                // 重新接线需要 pnpm；这条路径没有长安装，所以没有可流式推送的消
                // 息——只跑一次 profile 重新同步。
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

// --- 技能 --------------------------------------------------------------------

/// 技能商店以及按技能的 active-root 状态快照。
#[tauri::command]
pub async fn skill_status() -> Result<skills::SkillStatus, String> {
    tauri::async_runtime::spawn_blocking(skills::status)
        .await
        .map_err(|e| e.to_string())
}

/// 技能商店命令的共享主体：在 blocking worker 上运行 `skills::` 操作，
/// 并通过通道把进度转发出去。技能不需要 pnpm / profile 接线，因此这
/// 条路径比 `run_plugin_command` 更精简。
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

/// 安装一个技能包（npm spec、git URL 或本地目录路径）到中央商店，
/// 并把它包含的技能物化到内核的技能根目录。运行中的工作台通过内核
/// watcher 实时接收变更。Shell 一律请求 link 模式；`ensure_entry` 在
/// 符号链接不可用时会自己回退到 copy，而实际使用的模式会通过
/// `SkillRow.actual_mode` 回传给 UI。
#[tauri::command]
pub async fn skill_install(spec: String, on_event: Channel<String>) -> Result<(), String> {
    run_skill_command(on_event, move |progress| {
        skills::install(&spec, "link", progress).map(|_| ())
    })
    .await
}

/// 拉取一个已安装技能包的最新版本，并在 active root 中协调它的技能。
#[tauri::command]
pub async fn skill_update(id: String, on_event: Channel<String>) -> Result<(), String> {
    run_skill_command(on_event, move |progress| {
        skills::update(&id, progress).map(|_| ())
    })
    .await
}

/// 在所有位置（active root 条目 + 商店树）卸载一个技能包。
#[tauri::command]
pub async fn skill_uninstall(id: String, on_event: Channel<String>) -> Result<(), String> {
    run_skill_command(on_event, move |progress| skills::uninstall(&id, progress)).await
}

/// 启用或禁用某个包的某一个技能（在根目录中 link/unlink）。
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

/// 检查每个已安装的技能包在其来源处是否有更新版本。
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
        // AppKit 在 macOS 上创建窗口后会立刻报告几像素大小的临时 client
        // size；如果照此应用，strip 和内容 webview 都会坍缩成那条窄
        // 缝。relayout 必须保留上一次良好的 frame。
        assert!(!official_chat_layout_plausible(official_chat_layout(
            1366.0, 3.0,
        )));
        assert!(!official_chat_layout_plausible(official_chat_layout(
            4.0, 768.0,
        )));
        // 一个真实的窗口总是至少和页签栏一样大。
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
