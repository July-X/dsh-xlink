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
use tauri::webview::Color;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, State, Webview};
use tauri::{WebviewBuilder, WebviewUrl, WebviewWindowBuilder, WindowBuilder, WindowEvent};
use url::Url;

use crate::error::AppError;
use crate::process::{build_log_kind, read_tail, LogSpec};
use crate::quarantine;
use crate::{guard, kernel, node, patches, plugins, releases, settings, skills, updater};

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
    /// 串行化会改变内核、插件接线或工作台窗口状态的长操作，避免安装、
    /// 启动、切换和停止互相观察到半完成的文件系统状态。
    pub lifecycle: Mutex<()>,
    /// 最近一次解析到的 Node 运行时，以配置的 node 路径为键。状态轮
    /// 询每几秒就会跑一次；如果每次轮询都重新探测 `node --version`，
    /// 就会产生进程派生（Windows 上进程创建开销大），但解析结果其实
    /// 只在设置改变或机器的 Node 安装变化时才会变。
    pub node_cache: Mutex<Option<(Option<String>, node::NodeInfo)>>,
    /// 工作台 webview 当前这一次加载所使用的入口 URL（含 launch token）。
    /// 「打开工作台窗口」据此判断已有窗口是否还持有有效地址：token 没变
    /// 就只把窗口带到台前，不重新导航（导航等于销毁并重建整个工作台前端
    /// 状态）。仅在内核重启签发新 token 时才需要真正跳转。
    pub harness_url: Mutex<Option<String>>,
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
/// 时才会触发一次新的探测。“帮我安装”成功后由 `install_node` 主动清缓存。
fn cached_node(state: &AppState, settings: &settings::Settings) -> node::NodeInfo {
    let data_dir = state.data_dir.clone();
    let key = settings.node_path.clone();
    // 命中缓存时立刻返回；未命中则**先释放锁**再探测。
    // `node::resolve` 会派生 `node --version` 子进程（PATH + nvm + 系统位置
    // 逐个试），在持锁期间做这件事会把状态轮询里其它 `cached_node` 调用一起
    // 堵住（P2-27）。探测本身是幂等的，重复探测只是多花一次进程派生。
    {
        let guard = crate::lock(&state.node_cache);
        if let Some((cached_key, info)) = guard.as_ref() {
            if *cached_key == key {
                return info.clone();
            }
        }
    }
    let info = node::resolve(settings, &data_dir);
    *crate::lock(&state.node_cache) = Some((key, info.clone()));
    info
}

#[tauri::command]
pub async fn detect_node(state: State<'_, AppState>) -> Result<node::NodeInfo, String> {
    // 检测会忽略任何已配置的路径：它报告的是环境自身的探测结果，这样
    // UI 就能据它预填设置。解析过程对每个环境候选（PATH + nvm 管理的
    // 安装 + 系统位置）可能派生一个子进程——把这些进程派生放到 Tauri
    // 主线程之外。
    let data_dir = state.data_dir.clone();
    let info = tauri::async_runtime::spawn_blocking(move || {
        let mut s = settings::load(&data_dir);
        s.node_path = None;
        node::resolve(&s, &data_dir)
    })
    .await
    .map_err(|e| e.to_string())?;
    // 把新鲜结果写回缓存（键与 `cached_node` 一致：此时 `node_path` 视为未配置）：
    // 「检测 Node.js」是用户装好 Node 之后的第一个动作，不回写的话随后的「启动
    // 工作台」仍会命中旧的 `ok: false`（P2-8）。只在成功时写，失败结论留给下一次
    // 真实探测。
    if info.ok {
        *crate::lock(&state.node_cache) = Some((None, info.clone()));
    }
    Ok(info)
}

/// 用户确认后的托管 Node.js 自动安装（下载官方二进制到数据目录，
/// 不扩大安装包体积）。进度经 Channel 推送；成功后清掉 per-app node
/// 缓存，让下一次 get_status 立即报告新运行时，无需重启外壳。
#[tauri::command]
pub async fn install_node(app: AppHandle, on_event: Channel<String>) -> Result<(), String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let logs_dir = kernel::logs_dir(&data_dir);
        let mut send = |msg: &str| {
            let _ = on_event.send(msg.to_string());
        };
        let installed = crate::node_install::verify_or_install(&data_dir, &logs_dir, &mut send)?;
        if installed.is_some() {
            // 用 crate::lock 而不是裸 lock()：锁被毒化时也要把缓存清掉，
            // 否则下一次 get_status 仍会报告旧的（不存在的）Node 运行时。
            *crate::lock(&state.node_cache) = None;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: settings::Settings,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let previous = settings::load(&data_dir);
        // 面板只提交 `port` 与 `profile`，其余字段（`node_path` / `pnpm_path` /
        // `npm_path`）在请求里缺失，会被 `#[serde(default)]` 填成 `None`。直接落盘
        // 等于把用户手写在 settings.json 里的 Node 路径静默清空——而托管 Node
        // 安装失败时的提示恰好让用户去改那个字段（P2-7/P1-6）。
        let settings = merge_settings(&settings, &previous);
        // 端口只在工作台停止时才能改。运行中的内核绑在它启动时那个端口上，
        // 改掉配置端口会让状态页把"运行中"读成"未运行"；用户随后点一次
        // 「启动工作台」就会在同一 data dir 上拉起第二个内核，两个内核写
        // 同一份会话日志（`seq gap` 损坏）。其余设置可以随时改。
        if settings.port != previous.port && kernel::workbench_running(&data_dir, &previous) {
            return Err(format!(
                "工作台正在运行，无法修改端口（当前 {}，新值 {}）。请先点击「关闭工作台」停止工作台，再回来保存设置",
                previous.port, settings.port
            ));
        }
        settings::save(&data_dir, &settings).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把面板提交的设置与磁盘上的现值合并：请求里为 `None` 的路径字段继承现值。
///
/// 语义：`None` = 这次请求没有提到该字段；`Some("")` = 显式清空。面板目前只发
/// `port` / `profile`，因此手改过 `node_path` 的用户不会再被一次「保存设置」清掉
/// （P2-7）。
fn merge_settings(
    incoming: &settings::Settings,
    previous: &settings::Settings,
) -> settings::Settings {
    settings::Settings {
        node_path: incoming
            .node_path
            .clone()
            .or_else(|| previous.node_path.clone()),
        pnpm_path: incoming
            .pnpm_path
            .clone()
            .or_else(|| previous.pnpm_path.clone()),
        npm_path: incoming
            .npm_path
            .clone()
            .or_else(|| previous.npm_path.clone()),
        port: incoming.port,
        profile: incoming.profile.clone(),
    }
}

/// 把日志文件名拆成排序键 `(基名, 代次)`，用于「最新者优先」的稳定排序。
///
/// - `release-kernel-2026-09-10.log` → `("release-kernel-2026-09-10", 0)`
/// - `release-kernel-2026-09-10.1.log` → `("release-kernel-2026-09-10", 1)`
/// - 非数字后缀不算代次：`a.b.log` → `("a.b", 0)`
///
/// 代次按数值而非字符串比较，否则 `.10.log` 会排到 `.2.log` 前面。
pub fn log_sort_key(name: &str) -> (String, u32) {
    let base = name.strip_suffix(".log").unwrap_or(name);
    match base.rsplit_once('.') {
        Some((stem, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
            // 纯数字尾段当成代次；`u32` 溢出（异常长数字）时退回字符串形态，
            // 归类为「非代次」而不是 panic。
            match tail.parse::<u32>() {
                Ok(generation) => (stem.to_string(), generation),
                Err(_) => (base.to_string(), 0),
            }
        }
        _ => (base.to_string(), 0),
    }
}

/// `list_log_files` 的排序比较器：基名逆序（最新日期在前），同一基名内
/// 代次升序（`….log` → `….1.log` → `….2.log`，新 → 旧）。
pub fn compare_log_names(a: &str, b: &str) -> std::cmp::Ordering {
    let (base_a, generation_a) = log_sort_key(a);
    let (base_b, generation_b) = log_sort_key(b);
    base_b.cmp(&base_a).then(generation_a.cmp(&generation_b))
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

/// 扫描 logs 目录，收集所有 `.log` 文件（含轮转备份）并按「最新者优先」排序。
///
/// 独立成同步函数是为了能被测试直接驱动：面板可见性完全取决于这里的扩展名
/// 过滤与排序，而 `list_log_files` 本身需要 Tauri 的 `State`，单测无法构造。
fn collect_log_entries(dir: &Path) -> std::io::Result<Vec<LogFileEntry>> {
    let entries = fs::read_dir(dir)?;
    let mut out: Vec<LogFileEntry> = entries
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let path = entry.path();
            // 轮转备份命名成 `<base>.<n>.log`，扩展名仍是 `log`，因此这里
            // 天然收得到它们；只按扩展名判断即可，无需特判代次。
            if path.extension().and_then(|s| s.to_str()) != Some("log") {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Some(LogFileEntry { name, size })
        })
        .collect();

    // 排序：先按「基名」逆序，把最新的 `release-kernel-<today>.log` 排到
    // 列表头部；同一基名内再按代次升序，让同一天的日志按
    // `….log` → `….1.log` → `….2.log`（新 → 旧）排列。单纯按文件名字典序
    // 逆序会把 `.2.log` 排到 `.1.log` 前面，正好是反的。
    //
    // logs 目录目前没有任何保留策略：每个日期 × 每种 kind 最多留
    // `KERNEL_LOG_BACKUPS + 1` 代，但日期只增不减。历史上这里曾引用过一个
    // `cleanup_legacy_logs` 一次性清理函数，该函数已不存在——不要再按那个
    // 名字找清理逻辑（见台账 P2-62）。
    out.sort_by(|a, b| compare_log_names(&a.name, &b.name));
    Ok(out)
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
        collect_log_entries(&dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 校验来自 UI 的日志文件名：必须是纯文件名（无路径分隔符、无 `..`），
/// 避免页签列表把读取或开窗引到 logs 目录之外。
///
/// 这个判据此前在三处各写了一遍（读文件、开独立窗口），改一处漏一处的风险
/// 很高（P2-28），现在只有一个实现。
fn validate_log_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("非法的日志文件名：{name}"));
    }
    Ok(())
}

/// 读取 logs 目录下指定日志文件的尾部。
///
/// `name` 必须是纯文件名，不允许任何路径分隔符；本函数会拒绝其它形
/// 式以避免 UI 的页签列表越过 logs 目录。和 `get_kernel_log` 一样以
/// 16 KiB 作为尾部上限，使面板在面对大型安装日志时仍然保持响应。
#[tauri::command]
pub async fn read_log_file(state: State<'_, AppState>, name: String) -> Result<String, String> {
    validate_log_name(&name)?;
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
pub async fn install_shell_update(
    app: AppHandle,
    state: State<'_, AppState>,
    on_event: Channel<String>,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    updater::install(&app, &data_dir, move |line| {
        let _ = on_event.send(line.to_string());
    })
    .await
    .map_err(|e| e.to_string())
}

/// 新版本管理面板完成首次状态刷新后调用，确认当前 Shell 已能正常运行，
/// 再回收 Windows 上被 `/UPDATE` 跳过的旧安装和 updater 临时目录。
#[tauri::command]
pub async fn confirm_shell_ready(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        updater::confirm_shell_ready(&app, &data_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
    app: AppHandle,
    version: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    // 命令边界的最后一道闸：版本号来自 UI（最终来自远端版本列表），会被拼进
    // `kernels/<version>` 与内核 stub package.json。这里拒绝一切非 semver 形态
    // 的输入，包括路径分隔符与引号。
    if !crate::version::is_valid_kernel_version(&version) {
        return Err(format!(
            "版本号 {version:?} 形态非法，拒绝安装；请从「内核版本」页的官方发布列表中选择版本"
        ));
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    let version_for_install = version.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        // 安装前主动作废 per-app node 缓存：用户可能在 GUI 启动之后才
        // 通过 nvm/brew/官方安装包等渠道装好 node，缓存键（`node_path`）
        // 没变，`cached_node` 就会继续返回旧的 `ok: false`，把安装挡在
        // `promise_pnpm` 这一步并抛出"未检测到满足 dsh 要求的 Node.js"——
        // 但实际上 pnpm 完全能跑。强制重新探测一次（成本是一次
        // `node --version`），同时把 fresh 结果回写缓存，让本轮安装
        // 结束后的 `get_status` 轮询也立刻看到正确的 Node 状态，
        // 避免关闭失败面板后又被 node 安装引导弹窗打扰一次。
        // 同样用 `crate::lock`：锁被毒化时也要清掉缓存。
        *crate::lock(&state.node_cache) = None;
        let settings = settings::load(&data_dir);
        let node_info = cached_node(&state, &settings);
        let mut send = |msg: &str| {
            let _ = on_event.send(msg.to_string());
        };
        let (node_path, pnpm_exe) = promise_pnpm(&data_dir, &node_info, &mut send)?;
        // `install_version` 需要 node 可执行文件的完整路径——既用它本身
        // 在安装结束后启动 smoke-load 探针（见
        // `kernel::smoke_load_native_modules`），又把它所在目录前置到
        // 子进程的 PATH 上，这样 pnpm 的
        // `#!/usr/bin/env node` shebang 以及任何 shell-out 调
        // `node` 的 lifecycle 脚本都能解析到它，即便 GUI 进程继承
        // 到的只是 macOS .app 包那种 launchd-only PATH——这是 nvm 管
        // 理的安装里很常见的场景。
        kernel::install_version(
            &data_dir,
            &node_path,
            &pnpm_exe,
            &version_for_install,
            |msg| send(msg),
        )
        .map_err(|e| e.to_string())?;

        complete_kernel_install(&data_dir, &version_for_install, &mut send)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn complete_kernel_install(
    data_dir: &Path,
    version: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), String> {
    // 首次安装的内核会自动设为活动版本，之后的安装不再改动当前活动版
    // 本；安装完成后保持停止状态，由用户从「概览」页明确启动工作台。
    if kernel::read_active(data_dir).is_none() {
        kernel::set_active(data_dir, version).map_err(|e| e.to_string())?;
        on_progress(&format!("已切换到版本 {version}"));
    }
    Ok(())
}

#[tauri::command]
pub async fn activate_version(app: AppHandle, version: String) -> Result<(), String> {
    // 命令边界的最后一道闸（与 install_kernel / kernel_plugin_list 同一判据）：
    // 版本号是路径段，`".."` 之类的形态会被写进 active.txt，之后每次启动都按它
    // 去拼 `kernels/<version>/bin.js`（P1-7）。
    if !crate::version::is_valid_kernel_version(&version) {
        return Err(format!(
            "版本号 {version:?} 形态非法，拒绝切换；请从「内核版本」页的已安装列表中选择版本"
        ));
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    // 接线会用 pnpm 跑插件商店；把整个切换放到主线程之外。
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        // 切换会在下一次启动时生效，但为了避免运行中的服务与活动指针
        // 不一致，kernel::set_active 会要求工作台已经停止。
        kernel::set_active(&data_dir, &version).map_err(|e| e.to_string())?;
        // 重新接线插件到新活动内核（失败不阻断切换，原因进入插件卡片警告）
        let settings = settings::load(&data_dir);
        let node_info = cached_node(&state, &settings);
        let _ = plugins::ensure_wiring_quiet(&data_dir, &settings, &node_info);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_version(app: AppHandle, version: String) -> Result<(), String> {
    // 同 activate_version：`kernel::uninstall` 会对 `kernels/<version>` 直接
    // `remove_dir_all`，`".."` 会删掉整个数据目录（P1-7）。
    if !crate::version::is_valid_kernel_version(&version) {
        return Err(format!(
            "版本号 {version:?} 形态非法，拒绝删除；请从「内核版本」页的已安装列表中选择版本"
        ));
    }
    let data_dir = app.state::<AppState>().data_dir.clone();
    // 对内核目录（包括 node_modules）的 remove_dir_all 在 Windows 上
    // 可能耗时数秒；绝对不能在主线程上做。
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        kernel::uninstall(&data_dir, &version).map_err(|e| app_err(&data_dir, e))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod settings_merge_tests {
    use super::*;

    /// P2-7：面板只提交 `port` / `profile`，合并必须保留下手改过的路径字段，
    /// 否则一次「保存设置」就把用户配置的 Node 路径静默清空。
    #[test]
    fn merge_keeps_paths_the_panel_did_not_submit() {
        let previous = settings::Settings {
            node_path: Some("/opt/node/bin/node".into()),
            pnpm_path: Some("/opt/node/bin/pnpm".into()),
            npm_path: Some("/opt/node/bin/npm".into()),
            port: 3090,
            profile: "web".into(),
        };
        // 面板发来的请求：只有 port / profile，路径字段被 serde default 填成 None。
        let incoming = settings::Settings {
            node_path: None,
            pnpm_path: None,
            npm_path: None,
            port: 3100,
            profile: "dev".into(),
        };

        let merged = merge_settings(&incoming, &previous);
        assert_eq!(merged.node_path.as_deref(), Some("/opt/node/bin/node"));
        assert_eq!(merged.pnpm_path.as_deref(), Some("/opt/node/bin/pnpm"));
        assert_eq!(merged.npm_path.as_deref(), Some("/opt/node/bin/npm"));
        assert_eq!(merged.port, 3100, "面板提交的字段必须生效");
        assert_eq!(merged.profile, "dev");

        // 显式清空（空串）不被现值覆盖：`Some("")` 是"这次要清掉"。
        let clear = settings::Settings {
            node_path: Some(String::new()),
            ..incoming.clone()
        };
        assert_eq!(
            merge_settings(&clear, &previous).node_path.as_deref(),
            Some("")
        );
    }
}

#[cfg(test)]
mod kernel_install_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn install_completion_does_not_start_kernel() {
        let data_dir = std::env::temp_dir().join(format!(
            "dsh-xlink-install-completion-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let version = "0.1.2-alpha.test";
        let bin =
            kernel::kernel_dir(&data_dir, version).join("node_modules/@deepseek-ai/dsh/lib/bin.js");
        fs::create_dir_all(bin.parent().expect("kernel bin parent")).expect("create kernel");
        fs::write(&bin, b"// test kernel").expect("write kernel marker");
        settings::save(
            &data_dir,
            &settings::Settings {
                port: 0,
                ..settings::Settings::default()
            },
        )
        .expect("write settings");

        let mut progress = |_message: &str| {};
        let result = complete_kernel_install(&data_dir, version, &mut progress);
        let active = kernel::read_active(&data_dir);
        let port_open = kernel::port_open(0);
        let pid_file_exists = data_dir.join("kernel.pid").is_file();
        fs::remove_dir_all(&data_dir).expect("remove test data");

        result.expect("complete install");
        assert_eq!(active.as_deref(), Some(version));
        assert!(
            !port_open,
            "install completion must leave the kernel stopped"
        );
        assert!(
            !pid_file_exists,
            "install completion must not register a kernel process"
        );
    }
}

// --- 内核生命周期 -----------------------------------------------------------

/// 把新内核句柄放进槽位，并**显式回收**被替换掉的旧句柄。
///
/// `Option::replace` 会直接把旧 `Child` 丢掉，而 `std::process::Child` 的 `Drop`
/// 不会 `wait` —— 在 Unix 上那个已经退出的子进程就成了僵尸，一直挂在进程表里
/// 直到壳退出（`kernel_running` 只在句柄仍被用到时才会顺手 `try_wait`，不能
/// 指望它兜底）。
///
/// 旧句柄仍在运行时**不杀它**（它可能正服务着用户的会话），但也绝不能把句柄
/// 丢掉了事：交给一个后台线程 `wait`，让它在自己退出时被回收，同时把这件事
/// 报给调用方 —— 同一个 data dir 上不该同时存在两个内核。
fn replace_child_slot(slot: &Mutex<Option<Child>>, child: Child) -> Option<String> {
    let previous = crate::lock(slot).replace(child);
    let mut previous = previous?;
    match previous.try_wait() {
        // `try_wait` 对已退出的子进程就是一次 `waitpid`：拿到状态即已回收。
        Ok(Some(_)) => None,
        Ok(None) => {
            let pid = previous.id();
            std::thread::spawn(move || {
                let _ = previous.wait();
            });
            Some(format!(
                "上一个内核进程（pid {pid}）仍在运行，已交给后台线程等待其退出；\
                 同一数据目录下不应同时存在两个内核，请确认是否有残留进程"
            ))
        }
        Err(error) => Some(format!("回收上一个内核句柄失败：{error}")),
    }
}

/// 为成功启动的内核子进程做注册：记录其 pid 与启动端口以便后续重启后的 Shell
/// 回收，并把句柄保存在应用状态中。
///
/// 端口必须一起记（P2-1）：只凭 pid 无法区分「这还是我们那个内核」与「OS 把同一
/// 个 pid 复用给了另一个 dsh 内核」，后者会让「关闭工作台」误杀别的实例。
fn register_child(state: &AppState, data_dir: &Path, port: u16, child: Child) {
    kernel::write_pid(data_dir, child.id(), port);
    if let Some(warning) = replace_child_slot(&state.running, child) {
        eprintln!("dsh-xlink: {warning}");
    }
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
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let settings = settings::load(&data_dir);
        let mut node_info = cached_node(&state, &settings);
        if !node_info.ok {
            // 缓存可能已经过期：用户在壳运行期间用安装器 / nvm 装好了 Node，而缓存
            // 只在安装内核与托管 Node 时作废。启动是低频动作，这里强制重探一次再
            // 决定，避免「检测 Node.js」刚报成功、「启动工作台」仍拿旧结论拒绝
            // （P2-8）。
            *crate::lock(&state.node_cache) = None;
            node_info = cached_node(&state, &settings);
        }
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
            register_child(&state, &data_dir, settings.port, child);
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
/// 工作台窗口的系统关闭按钮（macOS 交通灯红灯 / Windows ×）始终可
/// 用，用户随时可以自己收起窗口——内核与进行中的任务继续在后台运
/// 行，随时可经「打开工作台窗口」重新打开，所以收起窗口不会打断用
/// 户的会话。拆掉整个工作台的唯一路径是这条命令：窗口走
/// `destroy()`——强制让系统关掉窗口，而不理会窗口是否可关闭——内核
/// 随之在下面停止。
#[tauri::command]
pub async fn stop_kernel(app: AppHandle) -> Result<(), String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    // kernel::stop 会等待子进程退出（最多等满它的 kill 超时），把这
    // 段等待放到主线程之外。
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        if let Some(window) = app.get_webview_window("harness") {
            let _ = window.destroy();
        }
        let stop_outcome = {
            let mut guard = crate::lock(&state.running);
            match guard.take() {
                Some(mut child) => kernel::stop(&mut child),
                None => Ok(()),
            }
        };
        // 端口不是判据：内核可能是在用户修改端口设置之前启动的，此时它仍然
        // 绑在旧端口上——用当前配置端口去探测会读成「没有内核在跑」，于是一个
        // 已经在服务的进程永远回收不掉。这里走与 `status()` 相同的活体判据
        // （pid 文件，或配置端口上的内核身份校验），`kill_pid` 内部还会再校
        // 验一遍 pid 仍指向 dsh 内核，因此被复用给无关进程的 pid 是 no-op。
        let current = settings::load(&data_dir);
        if let Some(pid) = kernel::workbench_pid(&data_dir, &current) {
            // 带上记录里的启动端口：完整三层校验才挡得住「pid 被复用给另一个
            // dsh 内核」这一类误杀（P2-1）。
            kernel::kill_pid(pid, kernel::recorded_kernel_port(&data_dir));
        }
        // pid 记录的清理必须无条件执行：内存句柄停止失败时若提前返回，下一次
        // 启动会带着一份陈旧的 pid 记录继续跑，而进程可能还活着。
        kernel::clear_pid(&data_dir);
        // 状态已经收敛，但把停止失败如实报给 UI——那通常意味着进程杀不掉，
        // 属于用户需要知道的事。
        stop_outcome.map_err(|e| e.to_string())
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
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
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
/// 窗口不再设置 `closable(false)`：macOS 交通灯与 Windows 关闭按钮保
/// 持可用，用户随时可以自己收起工作台窗口，内核与任务继续在后台运
/// 行，「打开工作台窗口」可随时重新打开。「关闭工作台」则经
/// `stop_kernel` 用 `destroy()` 强制拆窗口并停止内核。
/// 打开 dsh web 工作台窗口。原生标题栏保持 macOS / Windows / Linux 标
/// 准的窗口装饰，而不是用 Overlay，这样系统级的拖动 / 调整大小 / 双
/// 击最大化可以稳定工作（通过 `start_dragging` IPC 的 WKWebView 拖动
/// 区域路径在 Tauri 2.11.5 上表现不稳）。标题栏静态品牌条带由 Shell 端而非内
/// 核的 `packages/client/web/src/base.css` 拥有，通过
/// `initialization_script(titlebar-pulse.js)` 注入；脚本只创建静态 `<style>` 节点，
/// 不运行常驻 CSS 动画，避免流式会话更新时 WKWebView 持续布局和合成。规则带有
/// `!important`，使得无论内核版本是什么、也不论本脚本和工作台自身样式表的加载顺序，
/// Shell 的覆盖都能胜出。第二个
/// 注入脚本（`pullstring-launcher.js`）渲染一个浮在工作台左上角的拉
/// 绳小台灯；拉动它会调用 [`focus_main_shell`] 把管理窗口提到当前桌
/// 面之上。缺失 source map 则由打开窗口前的
/// `kernel::prepare_workbench_source_maps` 在服务端文件层补齐，避免依赖
/// 无法覆盖 DevTools 内部网络请求的页面脚本。
/// 解析工作台 webview 应该加载的 URL，优先使用内核自带的 launch-token
/// URL。
///
/// 0.1.2-alpha.1 起的内核在 browser 入口前加上一个进程级的 launch
/// token（`dsh-client-connection` BrowserAuth）：`/?token=` 用来签发
/// 会话 cookie，裸的根请求会得到 401。token 的唯一出处就是内核启动时
/// 输出的 `dsh web: http://127.0.0.1:<port>/?token=…` 这一行，Shell
/// 会把它捕获到当天的内核日志中。每次内核重启都会追加新的一行。
///
/// 当天日志可能还保留着上一个进程的 token；仅取最后一条日志行会在新进程
/// 写出 URL 前选中旧 token，WebView 随后收到 401 并停在白屏。每个候选地址
/// 都必须先通过 loopback HTTP 探针：token 地址应返回 303，旧版内核的裸地址
/// 应返回 2xx。这样既等待当前进程的 token，也保留旧版内核的兼容路径。
/// 从当天内核日志里取出最后一条 launch-token 入口 URL（不做 HTTP 探针）。
/// 「窗口已存在」的快路径用它来比对 token 是否变化：这只是一次日志尾读，
/// 不会像 [`kernel_workbench_url`] 那样最多阻塞 10 秒。
fn kernel_workbench_url_from_log(data_dir: &std::path::Path, port: u16) -> Option<String> {
    let tail = read_tail(&kernel::current_kernel_log_path(data_dir), 16 * 1024);
    let needle = format!("http://127.0.0.1:{port}/?token=");
    let start = tail.rfind(&needle)?;
    let rest = &tail[start + needle.len()..];
    let token = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map_or(rest, |end| &rest[..end]);
    if token.is_empty() {
        return None;
    }
    Some(format!("{needle}{token}"))
}

fn kernel_workbench_url(data_dir: &std::path::Path, port: u16) -> Result<String, String> {
    const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(400);
    const URL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

    let fallback = format!("http://127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + URL_TIMEOUT;
    loop {
        if let Some(candidate) = kernel_workbench_url_from_log(data_dir, port) {
            if workbench_url_responds(&candidate, PROBE_TIMEOUT) {
                return Ok(candidate);
            }
        }

        // 没有 token 的旧版内核仍接受裸地址；对 alpha.1 来说这里会返回
        // 401，因此不能把它当成当前地址，而要继续等新的 token 日志。
        if workbench_url_responds(&fallback, PROBE_TIMEOUT) {
            return Ok(fallback);
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "无法确认内核工作台地址，请打开日志后重试（日志：{}）",
                kernel::current_kernel_log_path(data_dir).display()
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 只检查工作台入口的响应状态，不跟随 303。使用独立的无 cookie agent，
/// 探针不会把认证状态留给真正的工作台 webview。
fn workbench_url_responds(url: &str, timeout: std::time::Duration) -> bool {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let Ok(response) = agent.get(url).call() else {
        return false;
    };
    let status = response.status().as_u16();
    (200..300).contains(&status) || (300..400).contains(&status)
}

/// 窗口/WebView 在首帧文档绘制之前使用的底色。
///
/// WebView 的默认底色是纯白：远程页面（工作台、官方对话）在网络请求
/// 与首屏渲染完成前会有几百毫秒的空白期，在深色的壳里读作一记刺眼的
/// 白闪。窗口层（NSWindow / HWND 背景）与 WebView 层都设成同一种底色
/// 之后，这段空白期呈现的是与目标页面同色系的暗底，加载完成时只是内容
/// 淡入，而不是从白到黑的跳变。
///
/// 颜色跟随系统主题（读主壳窗口的 theme）：浅色系统下用接近页面的浅灰
/// 而不是强行涂黑，避免把白闪换成同样刺眼的黑闪。
fn chrome_backdrop(app: &AppHandle) -> Color {
    let dark = app
        .get_webview_window("main")
        .and_then(|w| w.theme().ok())
        .map_or(true, |theme| theme == tauri::Theme::Dark);
    if dark {
        // 工作台与官方对话的深色底都在 #141414~#1B1B1F 附近。
        Color(0x16, 0x17, 0x1a, 0xff)
    } else {
        Color(0xf7, 0xf7, 0xf8, 0xff)
    }
}

/// 端口上有一个**已确证不是内核**的监听者时，返回拒绝打开工作台的理由。
///
/// 只在 [`kernel::ListenerIdentity::NotKernel`] 时拒绝：端口被无关程序占用时
/// 打开工作台会把那个程序当成工作台显示出来（P2-6 的残留）。身份未知
/// （端口空闲、查不到 pid、命令行读不出来）时返回 `None` 保持原有宽松行为——
/// 收紧到 Unknown 会让「内核在跑但 pid 反查失败」的正常场景打不开工作台，
/// 那是比误开一个网页严重得多的回归。
fn harness_port_conflict(port: u16, identity: kernel::ListenerIdentity) -> Option<String> {
    match identity {
        kernel::ListenerIdentity::NotKernel => Some(format!(
            "端口 {port} 被另一个程序占用，它不是 dsh 内核。\
             请先结束占用该端口的程序，或在「设置」里换一个端口后重试"
        )),
        kernel::ListenerIdentity::Kernel | kernel::ListenerIdentity::Unknown => None,
    }
}

#[tauri::command]
pub async fn open_harness(app: AppHandle) -> Result<(), String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let settings = settings::load(&data_dir);
        if !kernel::port_open(settings.port) {
            // 内核可能仍在**旧**端口上服务（用户改过端口设置，或 settings.json
            // 被手工改过）。这种情况下让用户反复点「启动工作台」是死路，
            // 直接给出唯一能收敛状态的下一步。
            if kernel::workbench_pid(&data_dir, &settings).is_some() {
                return Err(format!(
                    "端口 {} 上没有工作台，但检测到内核仍在运行。请先点击「关闭工作台」停止它，再重新启动工作台",
                    settings.port
                ));
            }
            return Err(format!(
                "内核未在运行（端口 {}），请先点击「启动工作台」",
                settings.port
            ));
        }
        // 端口上有监听者，但它必须是内核：无关程序占着端口时不能把它当
        // 工作台打开（P2-6）。
        if let Some(reason) =
            harness_port_conflict(settings.port, kernel::port_listener_identity(settings.port))
        {
            return Err(reason);
        }
        // 已经开着的工作台窗口：这个入口的语义是「把它带到台前」，不是
        // 重新加载。导航会丢掉工作台前端的全部运行时状态（会话滚动位置、
        // 侧栏面板、终端），用户观察到的就是「销毁旧的、重新拉起新的」。
        // 因此只有当内核重启签发了新的 launch token（日志里的入口 URL 与
        // 本次加载所用的 URL 不同）时才导航，否则只做 show + unminimize +
        // focus，并跳过探针与 source map 准备这些慢路径。
        if let Some(existing) = app.get_webview_window("harness") {
            let loaded = crate::lock(&state.harness_url).clone();
            let latest = kernel_workbench_url_from_log(&data_dir, settings.port);
            let stale = match (&loaded, &latest) {
                (Some(loaded), Some(latest)) => loaded != latest,
                // 没有记录到本次加载的地址（例如壳重启后窗口仍在）时不去
                // 猜测，宁可保留现有页面。
                _ => false,
            };
            if stale {
                if let Some(latest) = latest {
                    if let Ok(url) = Url::parse(&latest) {
                        let _ = existing.navigate(url);
                        *crate::lock(&state.harness_url) = Some(latest);
                    }
                }
            }
            let _ = existing.show();
            let _ = existing.unminimize();
            let _ = existing.set_focus();
            return Ok(());
        }

        kernel::prepare_workbench_source_maps(&data_dir);
        let resolved = kernel_workbench_url(&data_dir, settings.port)?;
        let url = Url::parse(&resolved).map_err(|e| e.to_string())?;
        *crate::lock(&state.harness_url) = Some(resolved);

        let backdrop = chrome_backdrop(&app);
        let handle = app.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dsh-open-harness".into())
            .spawn(move || {
                let result =
                    WebviewWindowBuilder::new(&handle, "harness", WebviewUrl::External(url))
                        .title("DeepSeek Harness 工作台")
                        .inner_size(1280.0, 840.0)
                        .background_color(backdrop)
                        .initialization_script(include_str!("titlebar-pulse.js"))
                        .initialization_script(include_str!("pullstring-launcher.js"))
                        .initialization_script(include_str!("harness-health.js"))
                        .initialization_script(include_str!("workbench-history-guard.js"))
                        .build()
                        .map(|_| ())
                        .map_err(|e| format!("无法创建工作台窗口：{e}"));
                if let Err(ref e) = result {
                    eprintln!("dsh-xlink: failed to open harness window: {e}");
                }
                #[cfg(debug_assertions)]
                if result.is_ok() {
                    if let Some(window) = handle.get_webview_window("harness") {
                        window.open_devtools();
                    }
                }
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|_| "工作台窗口创建线程已结束，未返回结果".to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
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
pub async fn open_log_window(app: AppHandle, name: String) -> Result<(), String> {
    validate_log_name(&name)?;
    // 建窗必须在**非主线程**上进行（Windows 上在主线程同步建 webview 会死锁，
    // 与 `open_harness` 同样的理由），因此把结果经 mpsc 回传：旧实现是同步
    // 命令 + 后台线程，失败只 `eprintln`，UI 永远返回成功——用户点了「全屏」
    // 却什么都不发生，且没有任何提示（P2-28）。
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("dsh-open-log-viewer".into())
        .spawn(move || {
            if let Some(existing) = handle.get_webview_window("log-viewer") {
                let _ = existing.destroy();
            }
            let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
            let backdrop = chrome_backdrop(&handle);
            let result = WebviewWindowBuilder::new(
                &handle,
                "log-viewer",
                WebviewUrl::App(format!("index.html?log={encoded}").into()),
            )
            .title(format!("日志 - {name}"))
            .inner_size(960.0, 720.0)
            .resizable(true)
            .background_color(backdrop)
            .build()
            .map(|_| ())
            .map_err(|e| format!("打开日志窗口失败：{e}。可改用主面板的「查看日志」弹窗，或重试"));
            let _ = tx.send(result);
        })
        .map_err(|e| format!("无法启动日志窗口线程：{e}"))?;

    // 等建窗结果。给足超时（webview 初始化在低配机器上可能偏慢），但绝不
    // 无限等待：超时按失败上报，让用户至少知道发生了什么。
    //
    // 等待必须放到 blocking 线程上：这是 async 命令，直接 `recv_timeout` 会占住
    // 一个 tokio worker 最长 20 秒，期间的其它命令都要排队（AGENTS.md 的约定是
    // 阻塞操作走 `spawn_blocking`，`open_harness` 也是这么做的，P2-17）。
    tauri::async_runtime::spawn_blocking(move || {
        match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(result) => result,
            Err(_) => {
                Err("打开日志窗口超时（20 秒）。请重试，或改用主面板的「查看日志」弹窗".into())
            }
        }
    })
    .await
    .map_err(|e| format!("等待日志窗口结果失败：{e}"))?
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
/// no-op 提升操作。恢复动作的实现在 [`crate::show_main_shell`]，与
/// Windows 托盘图标的「显示主界面」共用同一份。
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
    crate::show_main_shell(&app);
    Ok(())
}

/// Windows 标题栏的最小化按钮：与关闭一致，把窗口收进通知区域并从任务栏
/// 移除按钮（托盘是唯一的恢复入口）。
///
/// 不复用 `Window::minimize()`：那会把窗口最小化到任务栏，而这里要的是
/// 「任务栏不显示、只在托盘里」——`tray::hide_to_tray` 会
/// `ITaskbarList::DeleteTab` 再隐藏窗口。
///
/// 命令在所有平台都注册（`generate_handler!` 的条目形态要保持「一个名字
/// 一项」，`scripts/check-invariants.mjs` 是按 `,` 切分那段列表核对的），
/// 但只有 Windows 会真的动手：其它平台没有托盘，收起来就再也找不回来，
/// 所以这里直接返回，前端也只在 Windows 分支调用它。
#[tauri::command]
pub fn minimize_shell(
    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))] app: AppHandle,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if app.get_webview_window("main").is_none() {
            return Err("主壳窗口不存在（label: main）".to_string());
        }
        crate::tray::hide_to_tray(&app);
        crate::tray::notify_hidden_once(&app);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
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
    backdrop: Color,
) -> Result<(), String> {
    let (_, url_text) = OFFICIAL_CHAT_TABS
        .get(index)
        .ok_or_else(|| format!("官方对话页签不存在：{index}"))?;
    let label = format!("official-chat-tab-{index}");
    let url = Url::parse(url_text).map_err(|e| format!("非法页签地址：{e}"))?;
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .background_color(backdrop)
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
    let backdrop = chrome_backdrop(app);
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
                add_official_chat_tab(&window, index, layout, &profile_dir, backdrop)
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
    let backdrop = chrome_backdrop(&app);
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
                        .background_color(backdrop)
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
                add_official_chat_tab(&window, 0, layout, &profile_dir, backdrop)?;

                // 页签栏：本地 SPA 路由渲染页签栏并保留 `window.__TAURI__`，使其能
                // 调用页签命令。拉绳小台灯也由这个 38px 高的 WebView 渲
                // 染。strip 在所有内容视图之后再添加，因此它始终位于
                // 最上层。
                let strip_builder = WebviewBuilder::new(
                    OFFICIAL_CHAT_STRIP_LABEL,
                    WebviewUrl::App("index.html?chatstrip=1".into()),
                )
                .background_color(backdrop)
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
    // 用 `get_window`：`official-chat` 是由 WindowBuilder 创建的**裸窗口**
    // （它用 add_child 挂载多个子 webview），`get_webview_window` 对它永远
    // 返回 None，于是这个 label 会被静默跳过。以前靠 RunEvent::Exit 里的
    // 兜底掩盖着，而本函数的契约正是"已确认的退出必须自己关掉每一个窗口"。
    for label in ["official-chat", "harness", "log-viewer"] {
        if let Some(window) = app.get_window(label) {
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
    // `version` 会被当作路径段拼进 `kernels/<version>/plugins`，这里同样只接受
    // 已安装列表里的形态。
    if !crate::version::is_valid_kernel_version(&version) {
        return Err(format!("版本号 {version:?} 形态非法"));
    }
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
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let settings = settings::load(&data_dir);
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
                let state = app.state::<AppState>();
                let _lifecycle_guard = crate::lock(&state.lifecycle);
                let _store_guard = plugins::lock_store();
                quarantine::remove(&data_dir, &id).map_err(|e| e.to_string())?;
                let settings = settings::load(&data_dir);
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

// --- 内置补丁 ----------------------------------------------------------------

/// 读取随 dsh-xlink 发布包捆绑的内置补丁清单。资源目录缺失 / 清单损坏时
/// 静默降级为空列表，设置页呈现「此版本未携带任何内置补丁」。
fn load_bundled_patches(app: &AppHandle) -> (Vec<(patches::PatchDef, PathBuf)>, Vec<String>) {
    match patches::resource_patches_dir(app) {
        Some(dir) => match patches::load_patches_with_warnings(&dir) {
            Ok(loaded) => loaded,
            Err(reason) => {
                eprintln!("dsh-xlink: 读取内置补丁失败：{reason}");
                (Vec::new(), vec![format!("读取内置补丁失败：{reason}")])
            }
        },
        None => (Vec::new(), Vec::new()),
    }
}

/// 设置页「内核补丁」卡片的状态快照：每个补丁一行，状态按当前激活内核计算。
#[tauri::command]
pub async fn patch_status(app: AppHandle) -> Result<patches::PatchStatus, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (patches, load_warnings) = load_bundled_patches(&app);
        let mut view = patches::status(&data_dir, &patches);
        // 清单/定义被跳过的原因必须让用户看到：否则坏掉的补丁在设置页直接
        // 消失，用户无法区分"这个版本没带"与"它坏了"（P2-12）。
        if !load_warnings.is_empty() {
            let merged = load_warnings.join("；");
            view.warning = Some(match view.warning {
                Some(existing) => format!("{existing}；{merged}"),
                None => merged,
            });
        }
        Ok(view)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 应用一个内置补丁到当前激活内核。前置：工作台已停止、内核已激活、
/// 补丁版本范围覆盖当前内核。返回应用过程中的跳过低提示。
#[tauri::command]
pub async fn patch_apply(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<String>, String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let (patches, _warnings) = load_bundled_patches(&app);
        patches::apply(&data_dir, &patches, &id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 撤销一个内置补丁对当前激活内核的修改（从备份还原原文件）。
/// 返回撤销过程中的警告（例如备份丢失时的兜底处理说明）。
#[tauri::command]
pub async fn patch_revert(
    app: AppHandle,
    id: String,
    // `force`：用户在确认后选择"只清除记录、文件保持现状"。只对"没有可恢复的
    // 原文件"的记录有意义（P0-7）——那种记录既撤不掉也重打不了，没有这条出路
    // 就只能手改 state.json。前端只在读到「清除记录」提示时才带真值。
    force: Option<bool>,
) -> Result<Vec<String>, String> {
    let data_dir = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<String>, String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
        let (patches, _warnings) = load_bundled_patches(&app);
        patches::revert(&data_dir, &patches, &id, force.unwrap_or(false)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
    app: AppHandle,
    on_event: Channel<String>,
    op: impl FnOnce(&mut dyn FnMut(&str)) -> Result<(), AppError> + Send + 'static,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = app.state::<AppState>();
        let _lifecycle_guard = crate::lock(&state.lifecycle);
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
pub async fn skill_install(
    app: AppHandle,
    spec: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_skill_command(app, on_event, move |progress| {
        skills::install(&spec, "link", progress).map(|_| ())
    })
    .await
}

/// 拉取一个已安装技能包的最新版本，并在 active root 中协调它的技能。
#[tauri::command]
pub async fn skill_update(
    app: AppHandle,
    id: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_skill_command(app, on_event, move |progress| {
        skills::update(&id, progress).map(|_| ())
    })
    .await
}

/// 在所有位置（active root 条目 + 商店树）卸载一个技能包。
#[tauri::command]
pub async fn skill_uninstall(
    app: AppHandle,
    id: String,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_skill_command(app, on_event, move |progress| {
        skills::uninstall(&id, progress)
    })
    .await
}

/// 启用或禁用某个包的某一个技能（在根目录中 link/unlink）。
#[tauri::command]
pub async fn skill_set_enabled(
    app: AppHandle,
    id: String,
    name: String,
    enabled: bool,
    on_event: Channel<String>,
) -> Result<(), String> {
    run_skill_command(app, on_event, move |progress| {
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
mod workbench_url_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn respond(mut stream: TcpStream, new_token: &str) {
        let mut request = [0u8; 4096];
        let size = stream.read(&mut request).unwrap_or(0);
        let request = String::from_utf8_lossy(&request[..size]);
        let status = if request.contains(&format!("token={new_token}")) {
            "303 See Other"
        } else {
            "401 Unauthorized"
        };
        let response =
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = stream.write_all(response.as_bytes());
    }

    #[test]
    fn waits_for_current_token_when_daily_log_contains_previous_process_token() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe server");
        listener
            .set_nonblocking(true)
            .expect("set probe server nonblocking");
        let port = listener.local_addr().expect("probe address").port();
        let stale_token = "stale-process-token";
        let current_token = "current-process-token";
        let root = std::env::temp_dir().join(format!(
            "dsh-xlink-workbench-url-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let log_path = kernel::current_kernel_log_path(&root);
        fs::create_dir_all(log_path.parent().expect("log parent")).expect("create log dir");
        fs::write(
            &log_path,
            format!("dsh web: http://127.0.0.1:{port}/?token={stale_token}\n"),
        )
        .expect("write stale token");

        let stop_server = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop_server);
        let server = std::thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => respond(stream, current_token),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        let log_for_append = log_path.clone();
        let append = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let mut log = fs::OpenOptions::new()
                .append(true)
                .open(log_for_append)
                .expect("open log for current token");
            writeln!(
                log,
                "dsh web: http://127.0.0.1:{port}/?token={current_token}"
            )
            .expect("append current token");
        });

        let result = kernel_workbench_url(&root, port);

        append.join().expect("append thread");
        stop_server.store(true, Ordering::Relaxed);
        server.join().expect("probe server thread");
        fs::remove_dir_all(&root).expect("remove test data");
        assert_eq!(
            result,
            Ok(format!("http://127.0.0.1:{port}/?token={current_token}"))
        );
    }
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

#[cfg(test)]
mod log_listing_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut owned: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        owned.sort_by(|a, b| compare_log_names(a, b));
        owned
    }

    /// 在真实目录里跑一遍面板用的列举路径（含扩展名过滤），返回文件名字序列。
    fn listed(dir: &Path) -> Vec<String> {
        collect_log_entries(dir)
            .expect("listing logs")
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    fn temp_logs_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-logs-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).expect("create temp logs dir");
        dir
    }

    #[test]
    fn listing_includes_rotated_backups_and_skips_other_extensions() {
        // P2-4 的核心：轮转出去的 `<base>.<n>.log` 必须在面板列表里可见，
        // 而 `notes.txt`、`X.log.1`（旧命名，扩展名是 `1`）都不该混进来。
        let dir = temp_logs_dir("rotation");
        for name in [
            "release-kernel-2026-09-10.log",
            "release-kernel-2026-09-10.1.log",
            "release-kernel-2026-09-10.2.log",
            "release-kernel-2026-09-09.log",
        ] {
            fs::write(dir.join(name), b"x").expect("write log");
        }
        fs::write(dir.join("notes.txt"), b"x").expect("write txt");
        fs::write(dir.join("legacy.log.1"), b"x").expect("write legacy naming");

        assert_eq!(
            listed(&dir),
            vec![
                "release-kernel-2026-09-10.log",
                "release-kernel-2026-09-10.1.log",
                "release-kernel-2026-09-10.2.log",
                "release-kernel-2026-09-09.log",
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn listing_reports_file_sizes() {
        let dir = temp_logs_dir("sizes");
        fs::write(dir.join("release-kernel-2026-09-10.log"), b"12345").expect("write log");
        let entries = collect_log_entries(&dir).expect("listing logs");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 5);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn log_sort_key_splits_base_and_generation() {
        assert_eq!(
            log_sort_key("release-kernel-2026-09-10.log"),
            ("release-kernel-2026-09-10".to_string(), 0)
        );
        assert_eq!(
            log_sort_key("release-kernel-2026-09-10.1.log"),
            ("release-kernel-2026-09-10".to_string(), 1)
        );
        // 非数字后缀不是代次，属于基名的一部分。
        assert_eq!(log_sort_key("a.b.log"), ("a.b".to_string(), 0));
        // 数字后缀按数值比较，而不是字符串。
        assert_eq!(log_sort_key("x.10.log"), ("x".to_string(), 10));
        // 没有扩展名时整体当基名。
        assert_eq!(log_sort_key("kernel"), ("kernel".to_string(), 0));
    }

    #[test]
    fn newest_log_first_then_its_backups_in_order() {
        // 新日期在最前；同一天内 `.log` → `.1.log` → `.2.log`；老日期最后。
        // 旧实现用纯字典序逆序，会把 `.2.log` 排到 `.1.log` 前面。
        assert_eq!(
            sorted(&[
                "release-kernel-2026-09-09.log",
                "release-kernel-2026-09-10.2.log",
                "release-kernel-2026-09-10.log",
                "release-kernel-2026-09-10.1.log",
            ]),
            vec![
                "release-kernel-2026-09-10.log",
                "release-kernel-2026-09-10.1.log",
                "release-kernel-2026-09-10.2.log",
                "release-kernel-2026-09-09.log",
            ]
        );
    }

    #[test]
    fn two_digit_generations_sort_numerically() {
        assert_eq!(
            sorted(&["x.log", "x.2.log", "x.10.log"]),
            vec!["x.log", "x.2.log", "x.10.log"]
        );
    }
}

#[cfg(test)]
mod harness_open_tests {
    use super::*;

    #[test]
    fn refuses_to_open_a_port_held_by_a_known_non_kernel() {
        // P2-6 残留：端口被无关程序占用时，面板此前会把这个端口当工作台
        // 打开 —— 用户看到的是别人的网页。
        let reason = harness_port_conflict(3090, kernel::ListenerIdentity::NotKernel)
            .expect("已知的非内核监听者必须被拒绝");
        assert!(reason.contains("3090"), "理由里要带上端口：{reason}");
        assert!(reason.contains("设置"), "要给出可操作的下一步：{reason}");
    }

    #[test]
    fn allows_the_kernel_and_unknown_listeners() {
        assert!(harness_port_conflict(3090, kernel::ListenerIdentity::Kernel).is_none());
        // 身份未知时必须保持宽松：读不到命令行不等于"不是内核"，收紧会让
        // pid 反查失败的用户打不开工作台。
        assert!(harness_port_conflict(3090, kernel::ListenerIdentity::Unknown).is_none());
    }
}

#[cfg(all(test, unix))]
mod child_slot_tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn spawn_exit() -> Child {
        Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("spawn")
    }

    /// 等 `pid` 从进程表里消失（被回收）或超时。
    ///
    /// 用 `kill(pid, 0)` 探测而**不能**用 `waitpid`：`waitpid` 本身就会回收僵尸，
    /// 于是"观测"这个动作把被观测对象消掉了，测试永远看到"已回收"——该用例的
    /// 第一版正是这样失去区分度（反证时退化实现照样通过）。`kill(pid, 0)` 对
    /// 僵尸同样返回成功，只有进程真正被回收（pid 从进程表消失）后才报 ESRCH。
    fn wait_for_reap(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if unsafe { libc::kill(pid as i32, 0) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// 等 `pid` 进入僵尸态（已退出、尚未被回收），最多 5 秒。返回是否等到。
    ///
    /// `kill(pid, 0)` 对僵尸返回成功，所以它回答不了"退出了没有"；但可以反过来
    /// 用 `/proc` 式的进程状态读不出来……macOS/Linux 上通用且**不回收**的判据是
    /// 「自己看自己的子进程」：`Child::try_wait`。这里不能在用例主体里调用它
    /// ——那会把僵尸收掉，正是被测函数要做的观察——所以单独用一个 `sh -c
    /// "exit 0"` 探针来测「这个 shell 在 macOS 上进入僵尸态需要多久」，它测出来
    /// 的是环境事实而不是被测行为。
    ///
    /// 存在的理由：`replacing_a_dead_handle_reaps_it_instead_of_leaving_a_zombie`
    /// 原先固定 `sleep(300ms)` 就假定 `spawn_exit()` 的 shell 已经退出。在多核
    /// 机器上并行跑 270 多个用例时，派生一个 `sh` 偶尔会超过 300ms（rc.20 的
    /// macOS 质量门禁上真实发生过），于是 `try_wait` 返回 `Ok(None)`，用例报
    /// 「已退出的旧句柄应被静默回收」而红——被测逻辑完全正常，纯粹是等待时长
    /// 不够。固定时长换成"轮询到环境确认退出为止"即可根除。
    fn wait_for_child_exit(child: &mut Child) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn replacing_a_dead_handle_reaps_it_instead_of_leaving_a_zombie() {
        // P2-1：`Option::replace` 丢掉旧句柄而不 wait，已退出的子进程会以僵尸
        // 态留在进程表里。这里把"已退出但未回收"的句柄放进槽位，再替换它。
        let mut victim = spawn_exit();
        let pid = victim.id();
        // 用**另一个**同类 shell 探针确认这类进程在本机确实会退出（用例主体
        // 的 `try_wait` 会把僵尸收掉，不能拿被测句柄做探测），再对真正要放进
        // 槽位的句柄轮询到退出为止。
        let mut probe = spawn_exit();
        let entered = wait_for_child_exit(&mut probe);
        let _ = probe.wait();
        assert!(entered, "sh -c 'exit 0' 未在 5 秒内退出，环境异常");
        assert!(
            wait_for_child_exit(&mut victim),
            "要替换的旧句柄未在 5 秒内退出"
        );

        let slot: Mutex<Option<Child>> = Mutex::new(Some(victim));

        let warning = replace_child_slot(&slot, spawn_exit());
        assert!(warning.is_none(), "已退出的旧句柄应被静默回收：{warning:?}");
        assert!(
            wait_for_reap(pid),
            "旧子进程必须已被回收，否则它是僵尸（pid {pid}）"
        );

        if let Some(mut child) = slot.into_inner().unwrap() {
            let _ = child.wait();
        }
    }

    #[test]
    fn replacing_a_live_handle_hands_it_to_a_background_reaper() {
        // 旧句柄仍活着时不能杀（可能正在服务用户），但也不能丢句柄 —— 否则
        // 它退出时同样没人回收。
        let mut probe = spawn_exit();
        assert!(wait_for_child_exit(&mut probe), "环境异常：shell 无法退出");
        let _ = probe.wait();
        // `sleep 30` 而非 `sleep 0.3`：这个句柄必须在替换时**仍在运行**，
        // 让用例的判据只取决于被测逻辑，而不取决于调度延迟。
        let slot: Mutex<Option<Child>> = Mutex::new(Some(
            Command::new("sh")
                .arg("-c")
                .arg("sleep 30")
                .spawn()
                .expect("spawn"),
        ));
        let pid = slot.lock().unwrap().as_ref().unwrap().id();

        let warning = replace_child_slot(&slot, spawn_exit());
        let warning = warning.expect("仍在运行的旧句柄必须报告出来");
        assert!(
            warning.contains(&pid.to_string()),
            "诊断里要带上旧 pid：{warning}"
        );

        // 主动结束这个长睡进程来验证后台线程确实在等它：等它自然退出要 30 秒，
        // 测试不该依赖那个时长。SIGKILL 后仍要轮询——信号投递与进程真正消失
        // 之间有窗口，直接断言会变成新的竞速。
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        assert!(wait_for_reap(pid), "后台线程应负责回收它（pid {pid}）");

        if let Some(mut child) = slot.into_inner().unwrap() {
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod log_name_tests {
    use super::*;

    #[test]
    fn log_name_validation_rejects_anything_that_could_escape_logs_dir() {
        // P2-28：这个判据原先在三个地方各写了一遍，合并后必须逐条钉住。
        for bad in [
            "",
            "kernel.log/../../etc/passwd",
            "..\\..\\windows\\system32\\config",
            "/etc/passwd",
            "sub/dir.log",
            "sub\\dir.log",
            "..",
        ] {
            assert!(
                validate_log_name(bad).is_err(),
                "{bad:?} 必须被拒绝（否则会读到 logs 目录之外）"
            );
        }
        for good in [
            "release-kernel-2026-09-10.log",
            "release-kernel-2026-09-10.1.log",
        ] {
            assert!(validate_log_name(good).is_ok(), "{good:?} 是合法日志名");
        }
    }
}
