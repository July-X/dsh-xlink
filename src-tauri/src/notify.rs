//! 任务完成通知：内核事件订阅 → 未读计数 → 系统角标 + 通知气泡。
//!
//! # 数据来源
//!
//! 内核（`dsh web`）把会话状态变化以 Cordis 事件的形式推给客户端：同一台
//! 机器上的工作台 webview 就是通过这条通道实时更新侧栏与会话状态的。桌面壳
//! 在这里作为**同一个内核的另一个客户端**订阅同一条流：
//!
//! ```text
//! GET  http://127.0.0.1:<port>/?token=…   → 303 + dsh-auth-* cookie
//! WS   ws://127.0.0.1:<port>/api/remote.mux
//!      → {"type":"open","streamId":…,"endpoint":"$events","payload":{"args":{}}}
//!      ← {"type":"item","value":{"type":"ready","clientId":…}}
//!      ← {"type":"item","value":{"type":"emit","event":"api-session/status","args":[<会话 id>,true]}}
//! ```
//!
//! 判据只有一条：`api-session/status` 的第二个参数从 `true` 变成 `false`，
//! 即内核里某个会话的对话任务跑完了（`@deepseek-ai/dsh-api-session-controller`
//! 把 `agent/status` 直接映射成这个事件）。
//!
//! 同一条物理连接上还开第二条逻辑流 `session/control`，只为**会话标题**：标题是
//! 投影，变化只在 control 流上推送（`$events` 里的 `api-session/added` 发在会话
//! 创建那一刻，标题还是空的）。见 `TITLES_BASELINE_TIMEOUT` 与
//! `handle_control_item` 的说明。
//!
//! 刻意**不轮询** `session/list`：那条 RPC 每次都要为全部会话重建投影，
//! 实测一次约 2.1 MB / 0.5 s 内核 CPU——按通知所需的时间粒度（秒级）轮询会
//! 常驻吃掉一个核，代价与该功能的收益完全不成比例（详见
//! `docs/notification-design.md` 的「为什么不用轮询」）。
//!
//! # 未读语义
//!
//! 角标数字 = **用户没在看工作台时完成的任务数**。任务完成的那一刻：
//!
//! - 工作台窗口在前台：用户正看着它，不算未读；`notify_away_only` 打开时
//!   连通知气泡也不弹（不打扰）。
//! - 工作台不在前台（或窗口没开）：未读 +1、角标更新、弹通知气泡。
//!
//! 未读的清除有两条路径：工作台窗口重新获得焦点（用户回来看结果了），或
//! 管理面板里的「全部已读」。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tungstenite::client::IntoClientRequest;
use tungstenite::Message;

use crate::commands::AppState;
use crate::{kernel, settings};

/// 内核 WebSocket 多路复用路由（所有 Remote 流共用这一条物理连接）。
const MUX_PATH: &str = "/api/remote.mux";
/// 逻辑流：应用转发的 Cordis 事件。
const EVENTS_ENDPOINT: &str = "$events";
/// 本壳在 `$events` 流上使用的流 id（同一条连接上唯一即可）。
const STREAM_ID: &str = "dsh-xlink-notify";
/// 逻辑流：会话控制（队列 / 任务 / 投影）。标题是**投影**，只有这条流会推它的变化。
const CONTROL_ENDPOINT: &str = "session/control";
/// 本壳在 `session/control` 流上使用的流 id。
const CONTROL_STREAM_ID: &str = "dsh-xlink-titles";
/// 等 `session/control` 的 baseline 的宽限期：超时还没来就退回 `session/list` 快照。
const TITLES_BASELINE_TIMEOUT: Duration = Duration::from_secs(5);
/// 会话状态事件：`args = [sessionId, running]`。
const EVENT_SESSION_STATUS: &str = "api-session/status";
/// 新会话事件：`args = [summary]`，用于免费拿到会话标题。
const EVENT_SESSION_ADDED: &str = "api-session/added";
/// 收不到数据时的轮询间隔：用来看一眼是否该退出（同时也是读超时）。
const READ_TICK: Duration = Duration::from_millis(400);
/// 断线后的重连退避区间。
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(20);
/// 完成记录的保留条数（面板展示最近若干条）。
const INBOX_LIMIT: usize = 8;
/// 角标与通知里显示的最大数字；超出后显示 `999+`。
const BADGE_MAX: u32 = 999;
/// 订阅就绪前，若内核入口地址还没写进日志，等待重试的总时长上限。
const CONNECT_ATTEMPTS: u32 = 3;
/// `session/list` 响应体上限：正常约 2 MB（全部会话的投影），这里放宽一档
/// 以容纳会话更多的机器，同时挡住异常响应把壳拖垮。
const TITLES_BODY_LIMIT_BYTES: u64 = 32 * 1024 * 1024;

/// 管理面板与通知气泡共用的状态快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStatus {
    /// 任务完成通知总开关。
    pub enabled: bool,
    /// 仅当工作台窗口不在前台时才提醒。
    pub notify_away_only: bool,
    /// 通知是否带提示音。
    pub sound: bool,
    /// 已完成但用户未读的任务数（即角标数字）。
    pub unread: u32,
    /// 最近的完成记录，新的在前。
    pub items: Vec<CompletedTask>,
    /// 是否已连上内核事件流（内核未运行或断线时为 `false`）。
    pub watching: bool,
    /// 最近一次订阅或通知失败的可操作说明。
    pub last_error: Option<String>,
    /// 当前运行环境的限制说明（不是错误）。例如 macOS 上未打包的 dev 构建
    /// 拿不到应用 bundle，系统通知不会以本应用的名义投递。
    pub environment_note: Option<String>,
    /// 运行平台（`macos` / `windows`）。
    pub platform: String,
}

/// 一条"任务已完成"的记录。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTask {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    /// 完成时刻（unix 毫秒）。
    pub finished_at_ms: u64,
    /// 本次任务运行时长（毫秒）；没观察到开始时刻时为 0。
    pub duration_ms: u64,
}

/// 解析后的通知设置（磁盘上的 `Option` 在此收敛成具体取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyConfig {
    pub enabled: bool,
    pub notify_away_only: bool,
    pub sound: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            notify_away_only: true,
            sound: false,
        }
    }
}

impl NotifyConfig {
    /// 磁盘现值 → 生效配置。`None` 表示用户从未设置过，走默认值。
    pub fn resolve(settings: &settings::Settings) -> Self {
        let defaults = Self::default();
        Self {
            enabled: settings.notify_enabled.unwrap_or(defaults.enabled),
            notify_away_only: settings
                .notify_away_only
                .unwrap_or(defaults.notify_away_only),
            sound: settings.notify_sound.unwrap_or(defaults.sound),
        }
    }
}

/// 正在运行的会话：记下开始时刻，完成时用来算时长。
#[derive(Debug, Clone)]
struct RunningTurn {
    started_at_ms: u64,
}

/// 标题流（`session/control`）在**当前这条连接**上的状态。重连即重置。
#[derive(Debug, Clone, Copy)]
struct TitlesStream {
    /// 打开这条流的时刻，用于"迟迟没有 baseline"的超时判断。
    opened_at: Instant,
    /// 流已报错 / 已结束：不再等它，直接走 `session/list` 兜底。
    dead: bool,
    /// 本次连接已经试过 `session/list` 兜底（失败也要等下次重连再试，不刷屏）。
    fallback_done: bool,
}

/// 通知中心的全部可变状态。
struct Center {
    unread: u32,
    items: VecDeque<CompletedTask>,
    /// 会话 id（内核事件里的裸 uuid）→ 当前这一轮的开始时刻。
    running: HashMap<String, RunningTurn>,
    /// 已知的子代理会话：它们的完成不该打扰用户。
    subagents: HashSet<String>,
    /// 会话 id → 标题。三个来源：`session/control` 的 baseline（常驻会话的权威
    /// 快照）、标题投影的变化帧（新建会话标题生成 / 手动改名 / 清空）、以及
    /// `api-session/added` 的摘要；老内核上没有控制流时退回 `session/list` 快照。
    titles: HashMap<String, String>,
    /// 会话 id → 最近一次判定的完成时刻，用于丢弃重连后重放的完成事件。
    last_finished: HashMap<String, u64>,
    watching: bool,
    last_error: Option<String>,
    /// 标题表已经由权威来源填充过（baseline 或 `session/list` 快照）。
    titles_seeded: bool,
    titles_stream: TitlesStream,
}

impl Center {
    fn new() -> Self {
        Self {
            unread: 0,
            items: VecDeque::new(),
            running: HashMap::new(),
            subagents: HashSet::new(),
            titles: HashMap::new(),
            last_finished: HashMap::new(),
            watching: false,
            last_error: None,
            titles_seeded: false,
            titles_stream: TitlesStream {
                opened_at: Instant::now(),
                dead: false,
                fallback_done: false,
            },
        }
    }
}

/// 全局状态。`OnceLock` 而不是 `static Mutex<Center>`：`HashMap::new` 不是
/// const fn，无法在静态初始化里构造。
static CENTER: std::sync::OnceLock<Mutex<Center>> = std::sync::OnceLock::new();

/// 取全局通知中心的锁（首次调用时初始化）。
fn center() -> std::sync::MutexGuard<'static, Center> {
    let mutex = CENTER.get_or_init(|| Mutex::new(Center::new()));
    crate::lock(mutex)
}
/// 工作台窗口当前是否在前台（由 `lib.rs` 的窗口事件维护）。
static WORKBENCH_FOCUSED: AtomicBool = AtomicBool::new(false);
/// 事件订阅线程的句柄；`None` 表示没有在跑。
static WATCHER: Mutex<Option<Watcher>> = Mutex::new(None);

struct Watcher {
    stop: Arc<AtomicBool>,
    done: mpsc::Receiver<()>,
}

/// 管理面板读取的通知状态快照。
pub fn status(app: &AppHandle) -> NotificationStatus {
    let config = current_config(app);
    let center = center();
    NotificationStatus {
        enabled: config.enabled,
        notify_away_only: config.notify_away_only,
        sound: config.sound,
        unread: center.unread,
        items: center.items.iter().cloned().collect(),
        watching: center.watching,
        last_error: center.last_error.clone(),
        environment_note: environment_note(),
        platform: std::env::consts::OS.to_string(),
    }
}

/// 中心状态的一次完整提交：改中心 → 同步角标 → 广播快照 → 返回给命令层。
///
/// 手工写这几步时，锁还在手上就调 `status` / `sync_badge` 会自锁死，必须先
/// `drop(center)` 再重新加锁——这类错误编译器看不见。收进一个函数后，锁的
/// 持有时长是确定的：只包住 `mutate`（`status` 要读设置文件，不该握着锁做）。
fn commit_center(app: &AppHandle, mutate: impl FnOnce(&mut Center)) -> NotificationStatus {
    {
        let mut center = center();
        mutate(&mut center);
    }
    let status = status(app);
    sync_badge(app);
    broadcast(app, &status);
    status
}

/// 保存通知设置。只写通知这三个字段，其余设置原样保留。
pub fn save_settings(
    app: &AppHandle,
    enabled: bool,
    notify_away_only: bool,
    sound: bool,
) -> Result<NotificationStatus, String> {
    let data_dir = data_dir(app)?;
    let mut current = settings::load(&data_dir);
    current.notify_enabled = Some(enabled);
    current.notify_away_only = Some(notify_away_only);
    current.notify_sound = Some(sound);
    settings::save(&data_dir, &current)?;
    // 关掉总开关时顺手把角标摘掉：留着一个点不动的数字比没有角标更让人困惑。
    // 中心状态本身没变（变的是磁盘上的设置），这里只是把收尾的两步走一遍。
    Ok(commit_center(app, |_| {}))
}

/// 全部标记为已读：未读归零、角标清除。
pub fn mark_all_read(app: &AppHandle) -> NotificationStatus {
    commit_center(app, |center| {
        center.unread = 0;
        center.items.clear();
    })
}

/// 自检入口：**模拟一次任务完成**——未读 +1、刷新系统角标、弹一条系统通知。
///
/// 为什么要把三件事一起做，而不是只发一条通知：通知气泡是否出现由操作系统
/// 决定（macOS 上未打包的构建根本投递不了，见 [`environment_note`]），只发通知
/// 的自检会让用户误以为"整套功能没生效"；把角标一起走一遍，用户点一次就能在
/// Dock / 任务栏上看到数字，从而把「功能没做」和「系统不放行通知」区分开。
///
/// 刻意**无视** `notify_away_only`：用户主动点的按钮必须能看到效果，哪怕工作台
/// 正在前台。总开关关闭时不做任何事，只回一条可操作的说明。
pub fn send_test(app: &AppHandle) -> NotificationStatus {
    let config = current_config(app);
    if !config.enabled {
        center().last_error = Some(
            "任务完成通知的总开关已关闭，测试不会有效果。请先打开「任务完成后通知我」再点测试。"
                .into(),
        );
        let status = status(app);
        broadcast(app, &status);
        return status;
    }

    let task = CompletedTask {
        session_id: "self-test".into(),
        title: "测试通知".into(),
        cwd: String::new(),
        finished_at_ms: crate::process::epoch_millis(),
        duration_ms: 0,
    };
    {
        let mut center = center();
        center.items.push_front(task.clone());
        while center.items.len() > INBOX_LIMIT {
            center.items.pop_back();
        }
        center.unread = center.unread.saturating_add(1);
    }
    sync_badge(app);
    let result = show_toast(app, "任务已完成", &notification_body(&task), config.sound);
    center().last_error = result.err();
    let status = status(app);
    broadcast(app, &status);
    status
}

/// 试听提示音：只播一声系统提示音，不碰未读、角标与通知气泡。
///
/// 刻意**不做成"发一条带声音的系统通知"**：macOS 上未打包的 dev 构建根本投递
/// 不了通知（见 [`environment_note`]），而试听最常在 dev 期使用——那样按一次
/// 什么都听不到，反而像是"提示音坏了"。直接调系统的提示音接口：与通知气泡用的
/// 默认提示音同源，且不依赖通知权限、不派生子进程。
pub fn play_test_sound() -> Result<(), String> {
    platform_alert_sound()
}

/// macOS：`kSystemSoundID_UserPreferredAlert`（0x1000）= 用户在「系统设置 → 声音」
/// 里选的提醒声音，正是系统通知默认提示音用的那一个。`AudioServicesPlayAlertSound`
/// 异步播放、立即返回，不需要 bundle，dev 构建同样出声。
#[cfg(target_os = "macos")]
fn platform_alert_sound() -> Result<(), String> {
    const USER_PREFERRED_ALERT: u32 = 0x0000_1000;
    // SAFETY: 纯 C 调用，实参是头文件里的常量 id（`SystemSoundID` 即 u32），
    // 无返回值、无指针、无所有权转移；该函数可从任意线程调用。
    unsafe { AudioServicesPlayAlertSound(USER_PREFERRED_ALERT) };
    Ok(())
}

// `void AudioServicesPlayAlertSound(SystemSoundID inSystemSoundID)`（AudioToolbox）。
#[cfg(target_os = "macos")]
#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioServicesPlayAlertSound(sound_id: u32);
}

/// 播放失败时给用户的说明：讲清现象，并给出可检查的下一步。
#[cfg(target_os = "windows")]
const SOUND_FAILED: &str = "系统提示音没有播放成功：没有可用的音频输出设备，或系统声音被禁用。\
                             请检查系统音量与输出设备后重试（角标与通知气泡不受影响）。";

/// Windows：`MessageBeep` 异步播放、立即返回；`MB_ICONASTERISK` 取的是用户声音
/// 方案里「通知」类的音效（Windows 10/11 默认方案下就是通知默认音效，
/// 与 WinRT toast 的 `Notification.Default` 同一个 wav）。
#[cfg(target_os = "windows")]
fn platform_alert_sound() -> Result<(), String> {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;

    // SAFETY: 纯 C 调用，实参是文档里的 MESSAGEBOX_STYLE 常量，无指针参数。
    let played = unsafe { MessageBeep(MB_ICONASTERISK) };
    beep_result(played != 0)
}

/// `MessageBeep` 返回 0 = 系统没能播放（通常是没有任何可用输出设备）。
#[cfg(target_os = "windows")]
fn beep_result(played: bool) -> Result<(), String> {
    if played {
        Ok(())
    } else {
        Err(SOUND_FAILED.into())
    }
}

/// 壳只随 macOS 与 Windows 分发（AGENTS.md 的发布平台约定），其它平台上
/// 如实说明不可用，而不是静默什么都不做。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_alert_sound() -> Result<(), String> {
    Err("当前平台不支持试听提示音：dsh-xlink 只随 macOS 与 Windows 分发。".into())
}

/// 当前运行环境对系统通知的限制（`None` 表示没有已知限制）。
///
/// macOS 的系统通知按**应用 bundle** 归属：`tauri dev` / `cargo run` 跑的是
/// `target/debug/dsh-xlink` 这个裸可执行文件，系统找不到 bundle —— 实测
/// `usernoted` 会把请求挂到父进程（Terminal）名下、`NotificationCenter` 记录
/// `Unable to find valid bundle`，通知因此不会以 dsh-xlink 的名义出现。这是平台
/// 约束，壳无法绕过，只能如实告诉用户"用安装版验证"。角标不受影响（它直接作用
/// 在 Dock / 任务栏上）。
fn environment_note() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let in_bundle = std::env::current_exe()
            .map(|path| path.to_string_lossy().contains(".app/Contents/MacOS/"))
            .unwrap_or(false);
        if !in_bundle {
            return Some(
                "当前是未打包的开发构建：macOS 按应用 bundle 归属系统通知，找不到 bundle 时\
                 通知不会以 dsh-xlink 的名义投递（角标仍然正常）。要验证通知气泡，请用安装版，\
                 或先执行 `npm run build -- --debug` 再运行 \
                 `src-tauri/target/debug/bundle/macos/dsh-xlink.app`。"
                    .into(),
            );
        }
    }
    None
}

/// 工作台窗口的前台状态变化。用户切回工作台即视为"看过结果了"，未读清零。
pub fn set_workbench_focused(app: &AppHandle, focused: bool) {
    WORKBENCH_FOCUSED.store(focused, Ordering::Relaxed);
    if !focused {
        return;
    }
    let dirty = {
        let center = center();
        center.unread > 0
    };
    if dirty {
        mark_all_read(app);
    }
}

/// 启动内核事件订阅线程。重复调用是幂等的（已有线程在跑就直接返回）。
pub fn start_watcher(app: &AppHandle) {
    {
        let mut guard = crate::lock(&WATCHER);
        if guard.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, done) = mpsc::channel();
        let thread_stop = Arc::clone(&stop);
        let handle = app.clone();
        let spawned = std::thread::Builder::new()
            .name("dsh-notify-watch".into())
            .spawn(move || {
                watch_loop(&handle, &thread_stop);
                let _ = tx.send(());
            });
        match spawned {
            Ok(_) => *guard = Some(Watcher { stop, done }),
            Err(error) => {
                let message = format!(
                    "无法启动任务完成通知的订阅线程（{error}）。任务完成后不会有通知；\
                     可重启应用重试，仍失败请在项目仓库反馈"
                );
                eprintln!("dsh-xlink: {message}");
                center().last_error = Some(message);
            }
        }
    }
}

/// 停止订阅线程并等它退出（最多 3 秒）。内核停止、壳退出时调用。
pub fn stop_watcher() {
    let watcher = crate::lock(&WATCHER).take();
    let Some(watcher) = watcher else {
        return;
    };
    watcher.stop.store(true, Ordering::Relaxed);
    let _ = watcher.done.recv_timeout(Duration::from_secs(3));
    {
        let mut center = center();
        center.watching = false;
        center.running.clear();
    }
}

// ---------------------------------------------------------------------------
// 订阅线程
// ---------------------------------------------------------------------------

/// 订阅线程主体：连上 → 读事件 → 断线退避重连，直到收到停止信号。
fn watch_loop(app: &AppHandle, stop: &AtomicBool) {
    let mut backoff = RECONNECT_MIN;
    while !stop.load(Ordering::Relaxed) {
        match subscribe_once(app, stop) {
            Ok(()) => {
                // 正常结束（停止信号或对端关闭）：不打印噪声，直接走退避。
                backoff = RECONNECT_MIN;
            }
            Err(error) => {
                let mut center = center();
                center.watching = false;
                center.last_error = Some(error.clone());
            }
        }
        broadcast_now(app);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // 分片睡眠，保证停止信号最多在一个 tick 内生效。
        let deadline = std::time::Instant::now() + backoff;
        while std::time::Instant::now() < deadline {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(READ_TICK);
        }
        backoff = (backoff * 2).min(RECONNECT_MAX);
    }
    let mut center = center();
    center.watching = false;
}

/// 一次完整的订阅会话：取认证 cookie → 建立 WebSocket → 打开 `$events` →
/// 读帧直到出错或收到停止信号。
fn subscribe_once(app: &AppHandle, stop: &AtomicBool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let data_dir = state.data_dir.clone();
    let port = settings::load(&data_dir).port;
    let launch_url =
        crate::commands::kernel_workbench_url_from_log(&data_dir, port).ok_or_else(|| {
            format!(
                "还没有拿到内核入口地址（端口 {port}）。工作台未启动时不会收到任务完成通知；\
                 请先在概览页启动工作台。日志：{}",
                kernel::current_kernel_log_path(&data_dir).display()
            )
        })?;
    let cookie = fetch_auth_cookie(&launch_url, port)?;

    // 标题来源的每次连接重置：现在优先吃 `session/control` 的 baseline（下面开
    // 流时说明），只有它不可用时才退回 `session/list` 快照。
    {
        let mut center = center();
        center.titles_stream = TitlesStream {
            opened_at: Instant::now(),
            dead: false,
            fallback_done: false,
        };
    }

    let mut request = format!("ws://127.0.0.1:{port}{MUX_PATH}")
        .into_client_request()
        .map_err(|e| format!("无法构造内核事件流的 WebSocket 请求：{e}"))?;
    {
        let headers = request.headers_mut();
        let cookie = tungstenite::http::HeaderValue::from_str(&cookie)
            .map_err(|e| format!("认证 cookie 含非法字符，无法发送：{e}"))?;
        headers.insert("Cookie", cookie);
        // 内核的 `/api` 通道校验 Origin；用工作台自己的回环 origin 声明身份。
        let origin = tungstenite::http::HeaderValue::from_str(&format!("http://127.0.0.1:{port}"))
            .map_err(|e| format!("Origin 头非法：{e}"))?;
        headers.insert("Origin", origin);
    }

    let mut attempt = 0;
    let (mut socket, _response) = loop {
        match tungstenite::connect(request.clone()) {
            Ok(pair) => break pair,
            Err(error) => {
                attempt += 1;
                if attempt >= CONNECT_ATTEMPTS || stop.load(Ordering::Relaxed) {
                    return Err(format!(
                        "无法连上内核事件流（{error}）。任务完成通知暂时不可用；\
                         内核重启后会自动重连，也可以在管理面板重新启动工作台。"
                    ));
                }
                std::thread::sleep(RECONNECT_MIN);
            }
        }
    };

    if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_ref() {
        // 读超时让线程能周期性检查停止信号；tungstenite 会把超时视为
        // WouldBlock 并在下一次 read 时从断点续读。
        let _ = tcp.set_read_timeout(Some(READ_TICK));
    }

    let open = serde_json::json!({
        "type": "open",
        "streamId": STREAM_ID,
        "endpoint": EVENTS_ENDPOINT,
        "payload": { "args": {} },
    });
    socket
        .send(Message::Text(open.to_string().into()))
        .map_err(|e| format!("无法在内核事件流上打开 $events 订阅：{e}"))?;

    // 第二条逻辑流：会话控制。**标题是投影（`title`），它的变化只在这条流上推送**
    // ——`$events` 里只有 `api-session/added`（会话创建那一刻，标题还是空的）与
    // `api-session/status`，所以只听 `$events` 的话，"创建后由模型生成标题"的会话
    // 永远查不到名字，通知只能显示「未命名会话 <短 id>」。工作台侧栏用的就是这条
    // 流：开流时先给一份 baseline（常驻会话的全部投影，含标题），之后每次投影变化
    // 推一条 `{type:"projection", sessionId, key, value}`。
    let control_open = serde_json::json!({
        "type": "open",
        "streamId": CONTROL_STREAM_ID,
        "endpoint": CONTROL_ENDPOINT,
        "payload": { "args": {} },
    });
    socket
        .send(Message::Text(control_open.to_string().into()))
        .map_err(|e| format!("无法在内核事件流上打开 session/control 订阅：{e}"))?;

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return Ok(());
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                handle_frame(app, text.as_str());
                let _ = socket.flush();
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // 本 tick 没有数据：回到循环顶部检查停止信号。
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(())
            }
            Err(error) => {
                return Err(format!(
                    "内核事件流已断开（{error}）。任务完成通知会在这个内核进程内暂停；\
                     壳会自动重连，也可以重启工作台恢复。"
                ))
            }
        }
        // 控制流不可用（老内核不认这个 endpoint、或它中途断了）或迟迟不给 baseline
        // 时，退回一次 `session/list` 全量快照；拿不到也不影响通知本身，只影响文案。
        if titles_fallback_due() {
            fallback_session_titles(&cookie, port);
        }
    }
}

/// 现在是否该走 `session/list` 标题兜底：控制流没送来 baseline，且它已经报错 /
/// 结束 / 超时；每条连接最多试一次（失败留给下次重连，避免刷屏）。
fn titles_fallback_due() -> bool {
    titles_fallback_due_at(&center(), Instant::now())
}

fn titles_fallback_due_at(center: &Center, now: Instant) -> bool {
    if center.titles_seeded || center.titles_stream.fallback_done {
        return false;
    }
    center.titles_stream.dead
        || now.duration_since(center.titles_stream.opened_at) >= TITLES_BASELINE_TIMEOUT
}

/// 标题兜底：一次性读取全部会话标题（老内核上没有 `session/control` 时用）。
fn fallback_session_titles(cookie: &str, port: u16) {
    {
        let mut center = center();
        center.titles_stream.fallback_done = true;
    }
    match fetch_session_titles(cookie, port) {
        Ok(titles) => {
            let mut center = center();
            center.titles.extend(titles);
            center.titles_seeded = true;
        }
        Err(error) => {
            // 拿不到标题不影响通知本身，只影响文案；下一个重连周期再试。
            eprintln!("dsh-xlink: 读取会话标题失败（通知将使用占位标题）：{error}");
        }
    }
}

/// 处理一条服务端文本帧。两条逻辑流共用一条物理连接，按 `streamId` 分流。
fn handle_frame(app: &AppHandle, text: &str) {
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let stream_id = frame.get("streamId").and_then(Value::as_str).unwrap_or("");
    // 流级失败（end / error）先记账：控制流死了就要退回快照兜底。
    if stream_id == CONTROL_STREAM_ID {
        match frame.get("type").and_then(Value::as_str) {
            Some("end") => {
                center().titles_stream.dead = true;
                return;
            }
            Some("error") => {
                let message = frame
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                // 老内核没有 session/control：这不算故障，退回 session/list 即可，
                // 因此只记一行日志，不放 last_error 去吓用户。
                eprintln!("dsh-xlink: 会话控制流不可用（{message}），改用 session/list 读取标题");
                center().titles_stream.dead = true;
                return;
            }
            _ => {}
        }
    }
    if frame.get("type").and_then(Value::as_str) != Some("item") {
        return;
    }
    let Some(item) = frame.get("value") else {
        return;
    };
    if stream_id == CONTROL_STREAM_ID {
        handle_control_item(app, item);
        return;
    }
    match item.get("type").and_then(Value::as_str) {
        Some("ready") => {
            let mut center = center();
            center.watching = true;
            center.last_error = None;
            drop(center);
            broadcast_now(app);
        }
        Some("emit") => {
            let event = item
                .get("event")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = item.get("args");
            match event {
                EVENT_SESSION_STATUS => {
                    let Some(session_id) = args
                        .and_then(|a| a.get(0))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                    else {
                        return;
                    };
                    let Some(running) = args.and_then(|a| a.get(1)).and_then(Value::as_bool) else {
                        return;
                    };
                    on_session_status(app, &session_id, running);
                }
                EVENT_SESSION_ADDED => {
                    if let Some(summary) = args.and_then(|a| a.get(0)) {
                        let mut center = center();
                        remember_session(&mut center, summary);
                    }
                }
                _ => {}
            }
        }
        Some("error") => {
            let message = item
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            let mut center = center();
            center.watching = false;
            center.last_error = Some(format!(
                "内核事件流报错：{message}。任务完成通知暂时不可用；\
                 可重启工作台，若持续出现请在项目仓库反馈"
            ));
            drop(center);
            broadcast_now(app);
        }
        _ => {}
    }
}

/// `api-session/status`：会话在跑 / 跑完了。
fn on_session_status(app: &AppHandle, session_id: &str, running: bool) {
    let config = current_config(app);
    let id = normalize_session_id(session_id);
    let now = crate::process::epoch_millis();

    // 先把事件并入状态机（`running` 记账 + 完成判定），再决定要不要打扰用户：
    // 判定逻辑全在 `record_status` 里，因而可以脱离 AppHandle 单测。
    let completed = {
        let mut center = center();
        record_status(&mut center, &id, running, now)
    };
    let Some(task) = completed else {
        if running {
            // running 起点变化不需要打扰用户，但面板的"正在跑"提示会用到。
            broadcast_now(app);
        }
        return;
    };
    if !config.enabled {
        return;
    }
    // 工作台在前台 = 用户正看着结果，不计未读；`notify_away_only` 关闭时仍然
    // 弹一条通知，但不动角标（角标语义是"你没看见时完成了多少"）。
    let focused = WORKBENCH_FOCUSED.load(Ordering::Relaxed);
    if focused && config.notify_away_only {
        return;
    }

    {
        let mut center = center();
        center.items.push_front(task.clone());
        while center.items.len() > INBOX_LIMIT {
            center.items.pop_back();
        }
        if !focused {
            center.unread = center.unread.saturating_add(1);
        }
    }
    if !focused {
        sync_badge(app);
    }
    let body = notification_body(&task);
    let result = show_toast(app, "任务已完成", &body, config.sound);
    if let Err(error) = result {
        center().last_error = Some(error);
    }
    broadcast_now(app);
}

/// 会话状态事件的纯状态机部分。
///
/// - `running = true`：记下这一轮的开始时刻，返回 `None`（无事可报）。
/// - `running = false`：算出一条 [`CompletedTask`]；**子代理会话**（完成不该打扰
///   用户）与**重复事件**（重连后的重放）返回 `None`。
///
/// 不碰 `AppHandle`、不弹通知、不动未读计数——那些是调用方的决定，取决于
/// 用户在不在看工作台以及总开关的状态。
fn record_status(
    center: &mut Center,
    session_id: &str,
    running: bool,
    now_ms: u64,
) -> Option<CompletedTask> {
    if running {
        center.running.insert(
            session_id.to_string(),
            RunningTurn {
                started_at_ms: now_ms,
            },
        );
        return None;
    }

    // 子代理会话同样会报 running/idle，但它们不该打扰用户。事件本身不带父会话
    // 信息，因此只认已经由 `api-session/added` 标记过的 id（见 `remember_session`）。
    if center.subagents.contains(session_id) {
        center.running.remove(session_id);
        return None;
    }
    // 同一次完成被重放（断线重连后内核重发）时不要重复计数：按"会话 + 完成时刻"
    // 判重，比只看最近一条记录更稳。
    if let Some(previous) = center.last_finished.get(session_id) {
        if *previous == now_ms {
            return None;
        }
    }

    let started = center.running.remove(session_id);
    let title = center
        .titles
        .get(session_id)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| format!("未命名会话 {}", short_id(session_id)));
    let cwd = center
        .titles
        .get(&format!("{session_id}#cwd"))
        .cloned()
        .unwrap_or_default();
    center.last_finished.insert(session_id.to_string(), now_ms);
    Some(CompletedTask {
        session_id: session_id.to_string(),
        title,
        cwd,
        finished_at_ms: now_ms,
        duration_ms: started
            .map(|turn| now_ms.saturating_sub(turn.started_at_ms))
            .unwrap_or(0),
    })
}

// ---------------------------------------------------------------------------
// 与内核的 HTTP 交互
// ---------------------------------------------------------------------------

/// 用 launch token 换取浏览器认证 cookie（与工作台 webview 走同一条路）。
fn fetch_auth_cookie(launch_url: &str, port: u16) -> Result<String, String> {
    let agent = loopback_agent(Duration::from_secs(8));
    let response = agent.get(launch_url).call().map_err(|error| {
        format!(
            "无法用 launch token 完成内核认证（{error}）：端口 {port} 上可能不是本壳启动的内核。\
             请在概览页重新启动工作台"
        )
    })?;
    let raw = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            format!(
                "内核没有返回认证 cookie（HTTP {}）。请重新启动工作台；\
                 若仍失败，请把内核日志发给维护者（日志：内核日志文件）",
                response.status().as_u16()
            )
        })?;
    let pair = raw.split(';').next().unwrap_or_default().trim().to_string();
    if pair.is_empty() {
        return Err("内核返回了空的认证 cookie，请重新启动工作台".into());
    }
    Ok(pair)
}

/// 一次性读取全部会话标题（每个内核进程最多一次）。
fn fetch_session_titles(cookie: &str, port: u16) -> Result<HashMap<String, String>, String> {
    let agent = loopback_agent(Duration::from_secs(20));
    let body = serde_json::json!({
        "type": "client-request",
        "rpcId": format!("dsh-xlink-titles-{}", crate::process::epoch_millis()),
        "method": "session/list",
        "payload": { "args": { "_request": {} } },
    });
    let response = agent
        .post(format!("http://127.0.0.1:{port}/api/session/list"))
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .send(body.to_string())
        .map_err(|error| format!("session/list 请求失败：{error}"))?;
    // 该接口会为全部会话返回投影，实测约 2 MB；上限比正常值宽一档，同时挡住
    // 异常内核返回超大响应把壳拖垮。
    let mut response = response;
    let text = response
        .body_mut()
        .with_config()
        .limit(TITLES_BODY_LIMIT_BYTES)
        .read_to_string()
        .map_err(|error| format!("session/list 响应读取失败：{error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("session/list 响应不是 JSON：{error}"))?;
    let items = value
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("items"))
        .and_then(Value::as_array)
        .ok_or_else(|| "session/list 响应缺少 items 字段（内核版本可能不兼容）".to_string())?;
    let mut titles = HashMap::new();
    for item in items {
        let Some(id) = item.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let id = normalize_session_id(id);
        if let Some(title) = item
            .get("projections")
            .and_then(|p| p.get("values"))
            .and_then(|v| v.get("title"))
            .and_then(Value::as_str)
        {
            titles.insert(id.clone(), title.to_string());
        }
        if let Some(cwd) = item.get("cwd").and_then(Value::as_str) {
            titles.insert(format!("{id}#cwd"), cwd.to_string());
        }
    }
    Ok(titles)
}

/// 只访问回环地址、不跟随重定向、不读系统代理的 agent。
fn loopback_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent()
}

// ---------------------------------------------------------------------------
// 系统角标 / 通知气泡
// ---------------------------------------------------------------------------

/// 按当前未读数刷新系统角标。
pub fn sync_badge(app: &AppHandle) {
    let config = current_config(app);
    let unread = if config.enabled { center().unread } else { 0 };
    apply_badge(app, unread);
}

/// macOS：Dock 图标右上角的系统数字角标（`NSDockTile.setBadgeLabel`）。
#[cfg(target_os = "macos")]
fn apply_badge(app: &AppHandle, unread: u32) {
    let label = badge_text(unread);
    for window in badge_windows(app) {
        if window.set_badge_label(label.clone()).is_ok() {
            return;
        }
    }
}

/// Windows：任务栏按钮的覆盖图标（`ITaskbarList3::SetOverlayIcon`）。
/// Windows 的 Win32 任务栏没有"数字角标"API（那是 UWP 的
/// `BadgeNotification`），系统风格的做法就是覆盖图标。
#[cfg(target_os = "windows")]
fn apply_badge(app: &AppHandle, unread: u32) {
    let icon = (unread > 0).then(|| badge_image(unread));
    for window in badge_windows(app) {
        let _ = window.set_overlay_icon(icon.clone());
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn apply_badge(_app: &AppHandle, _unread: u32) {}

/// 角标要落在哪些窗口上：管理面板与工作台（与 macOS 的"全应用一个 Dock
/// 角标"语义对齐；Windows 上每个窗口是一个任务栏按钮，用户看哪一个都应
/// 该看到同一个数字）。
fn badge_windows(app: &AppHandle) -> Vec<tauri::WebviewWindow> {
    ["main", "harness"]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .collect()
}

/// 角标文本。`0` 用 `None` 表示清除。
fn badge_text(unread: u32) -> Option<String> {
    match unread {
        0 => None,
        n if n > BADGE_MAX => Some(format!("{BADGE_MAX}+")),
        n => Some(n.to_string()),
    }
}

/// 弹一条系统通知气泡。
///
/// 直接用 `notify-rust`，**不用** `tauri-plugin-notification`：那个插件在
/// `init()` 里会往每一个 webview（内核工作台、三个官方对话站点、日志窗口……）
/// 注入一段 JS shim，而它在 macOS 上页面一加载就调用
/// `plugin:notification|is_permission_granted`。那些窗口的 capability 里没有这条
/// 权限，于是每次打开页面都会产生一个未处理的 Promise 拒绝——它会被
/// `harness-health.js` 当成前端故障上报，还会在官方对话页面上留下一个 Tauri
/// 味道的 `window.Notification`，正好是 `chat-fingerprint.js` 要抹掉的东西。
/// 壳只需要 Rust 侧的发送能力，而 `notify-rust` 本来就是该插件的后端，因此直接
/// 依赖它：两端行为一致，且不碰任何 webview。
fn show_toast(app: &AppHandle, title: &str, body: &str, sound: bool) -> Result<(), String> {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(body);
    // Windows 必须带上 AppUserModelID，toast 才归属到本应用；未安装的构建没有
    // 对应的快捷方式，系统会退回 PowerShell 身份（已知限制，安装版正常）。
    #[cfg(target_os = "windows")]
    notification.app_id(app.config().identifier.as_str());
    // 两端的"系统默认提示音"取值不同：macOS 认这个符号名（它就是
    // NSUserNotificationDefaultSoundName 常量的字面值），Windows 认 "Default"。
    // 不设置时两端都是静音。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if sound {
        notification.sound_name(DEFAULT_SOUND);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = app;
    notification.show().map(|_| ()).map_err(|error| {
        format!(
            "系统通知发送失败（{error}）。请检查系统的通知权限设置：\
             macOS 在「系统设置 → 通知」里允许本应用，Windows 在「设置 → 系统 → 通知」里允许；\
             开发模式（未打包 / 未安装的构建）下两端都可能发不出通知，用安装版可排除这一项"
        )
    })
}

/// 请求系统默认提示音时传给各平台的取值（见 [`show_toast`]）。
#[cfg(target_os = "macos")]
const DEFAULT_SOUND: &str = "NSUserNotificationDefaultSoundName";
#[cfg(target_os = "windows")]
const DEFAULT_SOUND: &str = "Default";

/// 通知正文：会话标题 + 用时。
fn notification_body(task: &CompletedTask) -> String {
    let duration = format_duration(task.duration_ms);
    if duration.is_empty() {
        format!("「{}」已完成", task.title)
    } else {
        format!("「{}」已完成 · 用时 {}", task.title, duration)
    }
}

/// 毫秒 → 人类可读时长；不足 1 秒或未知时返回空串。
fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        return String::new();
    }
    let seconds = ms / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else if minutes > 0 {
        format!("{minutes} 分 {seconds} 秒")
    } else {
        format!("{seconds} 秒")
    }
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.try_state::<AppState>()
        .map(|state| state.data_dir.clone())
        .ok_or_else(|| "外壳状态尚未初始化，请稍后重试".to_string())
}

fn current_config(app: &AppHandle) -> NotifyConfig {
    match app.try_state::<AppState>() {
        Some(state) => NotifyConfig::resolve(&settings::load(&state.data_dir)),
        None => NotifyConfig::default(),
    }
}

/// 内核事件里的会话 id 不带 `session-` 前缀，而 `session/list` 里的带；
/// 统一去掉前缀，保证两张表能对上。
fn normalize_session_id(raw: &str) -> String {
    raw.strip_prefix("session-").unwrap_or(raw).to_string()
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// 记下 `api-session/added` 里的会话摘要（标题、cwd、子代理标记）。
fn remember_session(center: &mut Center, summary: &Value) {
    let Some(raw_id) = summary.get("sessionId").and_then(Value::as_str) else {
        return;
    };
    let id = normalize_session_id(raw_id);
    let is_subagent = summary.get("parentSessionId").is_some_and(|v| !v.is_null())
        || summary.get("origin").and_then(Value::as_str) == Some("subagent");
    if is_subagent {
        center.subagents.insert(id);
        return;
    }
    if let Some(title) = summary
        .get("projections")
        .and_then(|p| p.get("values"))
        .and_then(|v| v.get("title"))
        .and_then(Value::as_str)
    {
        center.titles.insert(id.clone(), title.to_string());
    }
    if let Some(cwd) = summary.get("cwd").and_then(Value::as_str) {
        center.titles.insert(format!("{id}#cwd"), cwd.to_string());
    }
}

/// 处理 `session/control` 的一条 item（baseline / projection / queue / jobs）。
///
/// 只关心标题相关的两类帧，其余原样忽略——这条流很大（含队列、任务、全部投影），
/// 每一条都解析是没必要的。
fn handle_control_item(app: &AppHandle, item: &Value) {
    let changed = match item.get("type").and_then(Value::as_str) {
        Some("baseline") => {
            let mut center = center();
            seed_titles_from_baseline(&mut center, item.get("value"));
            center.titles_seeded = true;
            false
        }
        Some("projection") => {
            let mut center = center();
            apply_title_projection(&mut center, item)
        }
        _ => false,
    };
    // 投影变化也会改到面板里那条完成记录的名字（改完名字通知历史不该还是旧名）。
    if changed {
        broadcast_now(app);
    }
}

/// 用 baseline 播种标题表：`projections[<sessionId>].values.title`。
fn seed_titles_from_baseline(center: &mut Center, baseline: Option<&Value>) {
    let Some(block) = baseline
        .and_then(|b| b.get("projections"))
        .and_then(Value::as_object)
    else {
        return;
    };
    for (raw_id, entry) in block {
        let title = entry
            .get("values")
            .and_then(|v| v.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        set_title(center, raw_id, title);
    }
}

/// 应用一条标题投影变化帧：`{sessionId, key: "title", value: string | null}`。
///
/// 返回是否真的改了标题表（调用方据此决定要不要广播给面板）。
fn apply_title_projection(center: &mut Center, frame: &Value) -> bool {
    if frame.get("key").and_then(Value::as_str) != Some("title") {
        return false;
    }
    let Some(raw_id) = frame.get("sessionId").and_then(Value::as_str) else {
        return false;
    };
    let title = frame
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default();
    set_title(center, raw_id, title)
}

/// 写入 / 清除一条标题，并同步面板里那条完成记录——通知历史不该停在旧名字上
/// （会话改名后，"最近完成"列表上的名字要跟着变）。
///
/// 返回标题表是否真的变了（调用方据此决定要不要广播）。标题被清空时只清本地
/// 缓存，不动已经记下的历史条目：那是一条"当时叫什么"的记录。
fn set_title(center: &mut Center, raw_id: &str, title: &str) -> bool {
    let id = normalize_session_id(raw_id);
    let title = title.trim();
    if title.is_empty() {
        return center.titles.remove(&id).is_some();
    }
    let changed = center.titles.get(&id).map(String::as_str) != Some(title);
    if changed {
        center.titles.insert(id.clone(), title.to_string());
    }
    for item in center.items.iter_mut() {
        if item.session_id == id && item.title != title {
            item.title = title.to_string();
        }
    }
    changed
}

fn broadcast_now(app: &AppHandle) {
    let status = status(app);
    broadcast(app, &status);
}

fn broadcast(app: &AppHandle, status: &NotificationStatus) {
    let _ = app.emit("notification-status", status.clone());
}

// ---------------------------------------------------------------------------
// Windows 数字角标位图
// ---------------------------------------------------------------------------

/// 角标画布边长。Windows 覆盖图标在 100% DPI 下按 16×16 绘制，取 32 是为了
/// 在 150%/200% DPI 下由系统缩小而不是放大（放大必然糊）。
#[cfg(target_os = "windows")]
const BADGE_SIZE: u32 = 32;

/// 3×5 点阵数字（每行 3 位，高位在左），`+` 用于 `99+`。
#[cfg(target_os = "windows")]
fn glyph(c: char) -> Option<[u8; 5]> {
    Some(match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        _ => return None,
    })
}

/// 渲染"红底白字"的任务栏角标：透明画布，右上角一个圆角徽标。
///
/// 形状与颜色取 Windows 自身的状态红（`#E81123`）+ 白色数字，与 macOS 的
/// 系统 Dock 角标（红底白字）保持同一套视觉语言。
#[cfg(target_os = "windows")]
fn render_badge(unread: u32) -> (Vec<u8>, u32) {
    let text = badge_text(unread).unwrap_or_else(|| "0".to_string());
    let glyphs: Vec<[u8; 5]> = text.chars().filter_map(glyph).collect();
    let scale: i32 = if glyphs.len() <= 1 { 3 } else { 2 };
    let advance = 3 * scale + scale; // 字宽 + 字间距
    let text_w = (glyphs.len() as i32 * advance - scale).max(1);
    let text_h = 5 * scale;
    let pad_x = 2 * scale;
    let pad_y = 2 * scale;
    let badge_w = text_w + 2 * pad_x;
    let badge_h = text_h + 2 * pad_y;
    // 右上角对齐，留 1px 边距；徽标本身按需决定是圆（单字）还是胶囊（多字）。
    let x1 = BADGE_SIZE as i32 - 1;
    let x0 = x1 - badge_w;
    let y0 = 1;
    let y1 = y0 + badge_h;
    let radius = badge_h as f32 / 2.0;

    let mut rgba = vec![0u8; (BADGE_SIZE * BADGE_SIZE * 4) as usize];
    const SAMPLES: i32 = 4;
    for py in 0..BADGE_SIZE as i32 {
        for px in 0..BADGE_SIZE as i32 {
            let mut badge_hits = 0u32;
            let mut text_hits = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let fx = px as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let fy = py as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if inside_round_rect(fx, fy, x0 as f32, y0 as f32, x1 as f32, y1 as f32, radius)
                    {
                        badge_hits += 1;
                        if inside_text(
                            fx,
                            fy,
                            x0 + pad_x,
                            y0 + pad_y,
                            text_w,
                            glyphs.len() as i32,
                            scale,
                            &glyphs,
                        ) {
                            text_hits += 1;
                        }
                    }
                }
            }
            let total = (SAMPLES * SAMPLES) as u32;
            let badge_alpha = badge_hits as f32 / total as f32;
            let text_alpha = text_hits as f32 / total as f32;
            let index = ((py * BADGE_SIZE as i32 + px) * 4) as usize;
            // 白字压在红底上：先按红色铺底，再按文字覆盖率混白。
            let red = [0xE8u8, 0x11, 0x23];
            let color = [
                blend(red[0], 0xFF, text_alpha),
                blend(red[1], 0xFF, text_alpha),
                blend(red[2], 0xFF, text_alpha),
            ];
            rgba[index] = color[0];
            rgba[index + 1] = color[1];
            rgba[index + 2] = color[2];
            rgba[index + 3] = (badge_alpha * 255.0).round() as u8;
        }
    }
    (rgba, BADGE_SIZE)
}

/// 覆盖在角标上的文字像素：把画布坐标映射回点阵格子。
#[cfg(target_os = "windows")]
fn inside_text(
    fx: f32,
    fy: f32,
    text_x: i32,
    text_y: i32,
    text_w: i32,
    glyph_count: i32,
    scale: i32,
    glyphs: &[[u8; 5]],
) -> bool {
    if glyph_count == 0 || text_w <= 0 {
        return false;
    }
    let lx = fx - text_x as f32;
    let ly = fy - text_y as f32;
    if lx < 0.0 || ly < 0.0 || lx >= text_w as f32 || ly >= (5 * scale) as f32 {
        return false;
    }
    let advance = 3 * scale + scale;
    let slot = (lx as i32) / advance;
    if slot < 0 || slot >= glyph_count {
        return false;
    }
    let within = (lx as i32) - slot * advance;
    if within >= 3 * scale {
        return false;
    }
    let column = within / scale;
    let row = (ly as i32) / scale;
    let bits = glyphs[slot as usize][row as usize];
    bits & (1 << (2 - column)) != 0
}

/// 圆角矩形命中测试（`radius` 为圆角半径，半高即胶囊形）。
#[cfg(target_os = "windows")]
fn inside_round_rect(fx: f32, fy: f32, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> bool {
    if fx < x0 || fx > x1 || fy < y0 || fy > y1 {
        return false;
    }
    let radius = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    if radius <= 0.0 {
        return true;
    }
    let cx = fx.clamp(x0 + radius, x1 - radius);
    let cy = fy.clamp(y0 + radius, y1 - radius);
    let (dx, dy) = (fx - cx, fy - cy);
    dx * dx + dy * dy <= radius * radius
}

#[cfg(target_os = "windows")]
fn blend(base: u8, top: u8, alpha: f32) -> u8 {
    (base as f32 * (1.0 - alpha) + top as f32 * alpha).round() as u8
}

/// 生成供 `set_overlay_icon` 使用的图像。
#[cfg(target_os = "windows")]
fn badge_image(unread: u32) -> tauri::image::Image<'static> {
    let (rgba, size) = render_badge(unread);
    tauri::image::Image::new_owned(rgba, size, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_text_clears_at_zero_and_caps() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(3).as_deref(), Some("3"));
        assert_eq!(badge_text(999).as_deref(), Some("999"));
        assert_eq!(badge_text(1500).as_deref(), Some("999+"));
    }

    #[test]
    fn duration_is_human_readable() {
        assert_eq!(format_duration(0), "");
        assert_eq!(format_duration(999), "");
        assert_eq!(format_duration(45_000), "45 秒");
        assert_eq!(format_duration(252_000), "4 分 12 秒");
        assert_eq!(format_duration(3_600_000), "1 小时 0 分");
    }

    #[test]
    fn session_ids_are_normalized() {
        assert_eq!(normalize_session_id("session-abc"), "abc");
        assert_eq!(normalize_session_id("abc"), "abc");
    }

    #[test]
    fn config_falls_back_to_defaults() {
        let mut settings = settings::Settings::default();
        assert_eq!(NotifyConfig::resolve(&settings), NotifyConfig::default());
        settings.notify_enabled = Some(false);
        settings.notify_sound = Some(true);
        let config = NotifyConfig::resolve(&settings);
        assert!(!config.enabled);
        assert!(config.sound);
        assert!(
            config.notify_away_only,
            "未设置时应保持默认的「仅离开时提醒」"
        );
    }

    /// 状态机的核心判定：跑完一轮才算完成，子代理与重放事件不算。
    #[test]
    fn record_status_reports_only_real_completions() {
        let mut center = Center::new();
        center.titles.insert("s1".into(), "重构通知系统".into());
        center.titles.insert("s1#cwd".into(), "/tmp/project".into());

        // 收到 running：只记账，不产出完成记录。
        assert!(record_status(&mut center, "s1", true, 1_000).is_none());
        assert!(center.running.contains_key("s1"));

        // 跑完：产出记录，标题与 cwd 来自缓存，时长 = 结束 - 开始。
        let task = record_status(&mut center, "s1", false, 253_000).expect("应产出完成记录");
        assert_eq!(task.title, "重构通知系统");
        assert_eq!(task.cwd, "/tmp/project");
        assert_eq!(task.duration_ms, 252_000);
        assert!(!center.running.contains_key("s1"), "完成后应清掉运行态");

        // 重连后重放的同一事件：同一时刻不算第二次完成。
        assert!(record_status(&mut center, "s1", false, 253_000).is_none());

        // 子代理会话：即便标记在运行，完成也不产出记录。
        center.subagents.insert("sub".into());
        assert!(record_status(&mut center, "sub", true, 2_000).is_none());
        assert!(record_status(&mut center, "sub", false, 3_000).is_none());

        // 未知会话：标题回退成占位文案，时长未知按 0。
        let unknown =
            record_status(&mut center, "abcdef123456", false, 4_000).expect("未知会话也算完成");
        assert_eq!(unknown.title, "未命名会话 abcdef12");
        assert_eq!(unknown.duration_ms, 0);
    }

    /// 未读只在"离开时完成"计数：这条规则由 `on_session_status` 的调用方实现，
    /// 这里锁住它的前提——`record_status` 本身不碰 unread / items。
    #[test]
    fn record_status_does_not_touch_unread_or_inbox() {
        let mut center = Center::new();
        record_status(&mut center, "s1", true, 1_000);
        record_status(&mut center, "s1", false, 2_000);
        assert_eq!(center.unread, 0);
        assert!(center.items.is_empty());
    }

    /// `api-session/added` 摘要的解析：标题/cwd 入表，子代理入黑名单。
    #[test]
    fn remember_session_collects_titles_and_subagents() {
        let mut center = Center::new();
        remember_session(
            &mut center,
            &serde_json::json!({
                "sessionId": "abc",
                "cwd": "/tmp/p",
                "projections": { "values": { "title": "写文档" } },
            }),
        );
        assert_eq!(center.titles.get("abc").map(String::as_str), Some("写文档"));
        assert_eq!(
            center.titles.get("abc#cwd").map(String::as_str),
            Some("/tmp/p")
        );

        // 子代理：带 parentSessionId（且前缀形式）与 origin 两种标记都要认。
        remember_session(
            &mut center,
            &serde_json::json!({
                "sessionId": "child",
                "parentSessionId": "session-abc",
                "origin": "subagent",
            }),
        );
        assert!(center.subagents.contains("child"));
        assert!(!center.titles.contains_key("child"), "子代理不该进标题表");
    }

    /// 回归：`api-session/added` 在会话**创建**那一刻发出，此时标题投影还是
    /// `null`（标题由模型在第一轮之后生成）。曾经的实现只认这一个事件 + 开流时的
    /// 一次性 `session/list` 快照，于是"连接期间新建的会话"永远查不到名字，通知
    /// 只能显示「未命名会话 <短 id>」。这条用例钉住真正权威的来源：`session/control`
    /// 的 baseline 与标题投影变化帧。
    #[test]
    fn baseline_and_title_projection_keep_titles_fresh() {
        let mut center = Center::new();

        // 创建时的事件：标题为空 / 缺失 → 不入表（这正是 bug 的起点）。
        remember_session(
            &mut center,
            &serde_json::json!({
                "sessionId": "session-e1723676-1111-2222-3333-444455556666",
                "projections": { "values": { "title": null } },
            }),
        );
        assert!(center.titles.is_empty(), "标题未生成时不该写入占位");

        // baseline：常驻会话的投影快照，id 带 `session-` 前缀（与事件里的裸 uuid 不同）。
        seed_titles_from_baseline(
            &mut center,
            Some(&serde_json::json!({
                "queues": {},
                "projections": {
                    "session-e1723676-1111-2222-3333-444455556666": {
                        "asOfSeq": 12,
                        "values": { "title": "精简通知设置并显示测试按钮" },
                    },
                    "session-blank": { "values": { "title": null } },
                    "session-spaces": { "values": { "title": "   " } },
                },
            })),
        );
        assert_eq!(
            center
                .titles
                .get("e1723676-1111-2222-3333-444455556666")
                .map(String::as_str),
            Some("精简通知设置并显示测试按钮"),
            "baseline 里带前缀的 id 要归一化成事件里的裸 uuid"
        );
        assert!(!center.titles.contains_key("blank"), "空标题不占位");
        assert!(!center.titles.contains_key("spaces"), "纯空白标题不占位");

        // 标题生成 / 改名：投影变化帧直接把新名字写进表里。
        let mut center2 = Center::new();
        assert!(apply_title_projection(
            &mut center2,
            &serde_json::json!({
                "sessionId": "session-abc",
                "key": "title",
                "value": "新名字",
                "seq": 30,
            }),
        ));
        assert_eq!(
            center2.titles.get("abc").map(String::as_str),
            Some("新名字")
        );
        assert!(
            !apply_title_projection(
                &mut center2,
                &serde_json::json!({ "sessionId": "session-abc", "key": "title", "value": "新名字" }),
            ),
            "同名重复帧不算变化，避免无谓广播"
        );
        // 其它投影（todos / inbox / …）不参与标题，也不该被当成变化。
        assert!(!apply_title_projection(
            &mut center2,
            &serde_json::json!({ "sessionId": "session-abc", "key": "todos", "value": [] }),
        ));
        assert_eq!(
            center2.titles.get("abc").map(String::as_str),
            Some("新名字")
        );

        // 标题被清空：本地缓存跟着清掉，让「未命名会话 <短 id>」接管。
        assert!(apply_title_projection(
            &mut center2,
            &serde_json::json!({ "sessionId": "session-abc", "key": "title", "value": null }),
        ));
        assert!(!center2.titles.contains_key("abc"));
    }

    /// 完成记录里的名字要跟着投影更新走：面板展示的历史不该停在旧标题上。
    #[test]
    fn title_projection_refreshes_recorded_items() {
        let mut center = Center::new();
        center.items.push_front(CompletedTask {
            session_id: "abc".into(),
            title: "未命名会话 abc".into(),
            cwd: String::new(),
            finished_at_ms: 1,
            duration_ms: 0,
        });

        assert!(apply_title_projection(
            &mut center,
            &serde_json::json!({ "sessionId": "session-abc", "key": "title", "value": "真名" }),
        ));
        assert_eq!(center.titles.get("abc").map(String::as_str), Some("真名"));
        assert_eq!(
            center.items[0].title, "真名",
            "已经记下的完成记录也要换成新名字（面板显示的就是它）"
        );
    }

    /// 兜底时机：控制流不可用（老内核 / 报错）或迟迟不给 baseline 时才读
    /// `session/list`；已经有了权威标题表就不再全量扫一遍。
    #[test]
    fn session_list_fallback_is_bounded() {
        let mut center = Center::new();
        // 以"开流时刻"为基准：`opened_at` 由 `Center::new()` 取 `Instant::now()`，
        // 用外面更早的 `now` 会算不出超时。
        let now = center.titles_stream.opened_at;

        assert!(
            !titles_fallback_due_at(&center, now),
            "刚开流时先等 baseline，不要立刻全量扫"
        );
        assert!(
            titles_fallback_due_at(&center, now + TITLES_BASELINE_TIMEOUT),
            "超时还没 baseline：退回快照"
        );

        center.titles_stream.dead = true;
        assert!(titles_fallback_due_at(&center, now), "控制流报错立刻兜底");

        center.titles_stream.fallback_done = true;
        assert!(!titles_fallback_due_at(&center, now), "每条连接只兜底一次");

        let mut fresh = Center::new();
        fresh.titles_seeded = true;
        assert!(
            !titles_fallback_due_at(&fresh, now + TITLES_BASELINE_TIMEOUT),
            "标题表已经由 baseline 播种，不需要再扫 session/list"
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_badge_tests {
    use super::*;

    #[test]
    fn badge_bitmap_is_transparent_outside_and_opaque_inside() {
        let (rgba, size) = render_badge(3);
        assert_eq!(size, BADGE_SIZE);
        assert_eq!(rgba.len(), (size * size * 4) as usize);
        let alpha_at = |x: u32, y: u32| rgba[((y * size + x) * 4 + 3) as usize];
        // 画布左下角必须完全透明，否则任务栏上会出现一个白/红方块。
        assert_eq!(alpha_at(0, size - 1), 0);
        // 右上角是角标本体。
        assert!(alpha_at(size - 3, 3) > 200);
        // 角标中心应为白色（数字笔画）或红色（底色），但必须不透明。
        assert!(alpha_at(size - 8, 8) > 200);
    }

    #[test]
    fn zero_never_produces_a_badge() {
        assert_eq!(badge_text(0), None);
    }

    /// 多位数必须排进画布内：`99` 与 `999+` 都不该被裁掉笔画。
    #[test]
    fn multi_digit_badges_fit_the_canvas() {
        for count in [9u32, 10, 99, 100, 1000] {
            let (rgba, size) = render_badge(count);
            assert_eq!(rgba.len(), (size * size * 4) as usize);
            let opaque: Vec<u32> = rgba
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, px)| px[3] > 128)
                .map(|(i, _)| i as u32)
                .collect();
            assert!(!opaque.is_empty(), "{count} 应画出角标");
            // 所有不透明像素都落在画布内（下标由 chunks_exact 保证），且角标
            // 必须贴着右上角：最右一列与最上一行都要有内容。
            let (w, h) = (size as u32, size as u32);
            let max_x = opaque.iter().map(|i| i % w).max().unwrap();
            let min_y = opaque.iter().map(|i| i / w).min().unwrap();
            assert_eq!(max_x, w - 1, "{count} 的角标应贴住右边缘");
            assert!(min_y <= 1, "{count} 的角标应贴住上边缘");
            assert!(
                opaque.len() < (w * h) as usize / 2,
                "{count} 的角标不应铺满整个图标"
            );
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_sound_tests {
    use super::*;

    /// `MessageBeep` 的返回值翻译：失败必须变成带下一步的说明，而不是静默无声。
    #[test]
    fn beep_failure_is_reported_with_a_next_step() {
        assert!(beep_result(true).is_ok());
        let error = beep_result(false).expect_err("播放失败必须报错");
        assert!(
            error.contains("输出设备"),
            "错误要指向可检查的东西：{error}"
        );
    }
}

/// 对**真实内核**的集成测试：默认忽略，避免 CI 里必然失败。
///
/// 覆盖的是整套实现里最容易写错、又最难在别处验证的一段：launch token 换
/// cookie、WebSocket 握手（自定义 Cookie / Origin 头）、`$events` 的 open 帧
/// 字段名、以及就绪帧。协议细节来自 `@deepseek-ai/dsh-api-gateway` 源码，见
/// `docs/notification-design.md` 第 3 节。
///
/// 用法（先让工作台跑起来，端口以设置页为准）：
///
/// ```sh
/// DSH_DESKTOP_DATA_DIR=~/.dsh/desktop DSH_XLINK_LIVE_PORT=4090 \
///   cargo test --lib -- --ignored live_ --nocapture
/// ```
#[cfg(test)]
mod live_tests {
    use super::*;

    fn live_port() -> u16 {
        std::env::var("DSH_XLINK_LIVE_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .expect("需要 DSH_XLINK_LIVE_PORT=<正在运行的内核端口>")
    }

    fn live_data_dir() -> std::path::PathBuf {
        if let Some(dir) = std::env::var_os("DSH_DESKTOP_DATA_DIR") {
            return std::path::PathBuf::from(dir);
        }
        let home = std::env::var_os("DSH_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| kernel::dirs_home().join(kernel::DSH_HOME_DIR_NAME));
        home.join(if cfg!(debug_assertions) {
            "desktop-dev"
        } else {
            "desktop"
        })
    }

    /// 从 `logs/` 里最新的内核日志中取 launch-token URL。
    ///
    /// 不能用 `kernel::current_kernel_log_path`：日志文件名里的 kind 由**构建
    /// 配置**决定（debug → `dev-kernel-*`、release → `release-kernel-*`），而这条
    /// 测试是 debug 构建、跑起来的内核却通常是 release 壳启动的。运行时不存在
    /// 这个错配——壳读写的是同一个 kind。
    fn live_launch_url(data_dir: &std::path::Path, port: u16) -> Option<String> {
        let needle = format!("http://127.0.0.1:{port}/?token=");
        let mut candidates: Vec<_> = std::fs::read_dir(data_dir.join("logs"))
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".log") && name.contains("-kernel-"))
            })
            .collect();
        candidates.sort_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok()
        });
        for path in candidates.iter().rev() {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            if let Some(start) = text.rfind(&needle) {
                let rest = &text[start + needle.len()..];
                let token = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                    .map_or(rest, |end| &rest[..end]);
                if !token.is_empty() {
                    return Some(format!("{needle}{token}"));
                }
            }
        }
        None
    }

    /// 读一帧；读超时（本 tick 无数据）不算失败，直到 `deadline` 为止。
    fn read_frame_until(
        socket: &mut tungstenite::WebSocket<
            tungstenite::stream::MaybeTlsStream<std::net::TcpStream>,
        >,
        deadline: std::time::Instant,
    ) -> Option<Value> {
        while std::time::Instant::now() < deadline {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str::<Value>(text.as_str()).ok();
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => panic!("读取内核事件流失败：{error}"),
            }
        }
        None
    }

    #[test]
    #[ignore = "需要本机内核在运行；用法见模块注释"]
    fn live_event_stream_reports_ready_and_lists_titles() {
        let port = live_port();
        let data_dir = live_data_dir();
        let launch = live_launch_url(&data_dir, port).expect("日志里应有 launch token URL");
        let cookie = fetch_auth_cookie(&launch, port).expect("launch token 应能换取 cookie");

        let mut request = format!("ws://127.0.0.1:{port}{MUX_PATH}")
            .into_client_request()
            .expect("构造 WebSocket 请求");
        request.headers_mut().insert(
            "Cookie",
            tungstenite::http::HeaderValue::from_str(&cookie).expect("cookie 头合法"),
        );
        request.headers_mut().insert(
            "Origin",
            tungstenite::http::HeaderValue::from_str(&format!("http://127.0.0.1:{port}"))
                .expect("origin 头合法"),
        );
        let (mut socket, response) = tungstenite::connect(request).expect("WebSocket 握手应成功");
        assert_eq!(response.status().as_u16(), 101, "内核应接受 upgrade");
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_ref() {
            tcp.set_read_timeout(Some(Duration::from_millis(500)))
                .expect("设置读超时");
        }
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "open",
                    "streamId": STREAM_ID,
                    "endpoint": EVENTS_ENDPOINT,
                    "payload": { "args": {} },
                })
                .to_string()
                .into(),
            ))
            .expect("发送 open 帧");

        // 首帧必须是 ready：它同时证明认证、路由与 `$events` 订阅都成立。
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let ready = read_frame_until(&mut socket, deadline).expect("10 秒内应收到 ready 帧");
        assert_eq!(ready["type"], "item", "服务端帧必须是 item：{ready}");
        assert_eq!(ready["value"]["type"], "ready", "首帧必须是 ready：{ready}");
        assert!(
            ready["value"]["clientId"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
            "ready 帧应带非空 clientId：{ready}"
        );
        // 订阅成功后主动关闭，避免在真实内核上留下悬挂连接。
        let _ = socket.close(None);

        // 标题快照：同一条认证路径上的第二个接口，通知文案依赖它。
        let titles = fetch_session_titles(&cookie, port).expect("应能读取会话标题");
        assert!(
            titles.keys().any(|key| !key.ends_with("#cwd")),
            "本机应至少有一个带标题的会话"
        );
    }

    /// 会话控制流的标题播种：真实内核上的回归用例。
    ///
    /// 复现的问题：会话创建时 `api-session/added` 里标题还是空的，标题由模型在第
    /// 一轮之后生成，而它的变化**只**在 `session/control` 上推送。壳以前只听
    /// `$events`，于是新建会话的通知永远显示「未命名会话 <短 id>」。这条测试连上
    /// 真实内核、开同一条流，断言 baseline 能被解析成"事件里那种裸 uuid → 标题"
    /// 的表——也就是通知文案要用的那次查表。
    #[test]
    #[ignore = "需要本机内核在运行；用法见模块注释"]
    fn live_control_stream_seeds_titles() {
        let port = live_port();
        let data_dir = live_data_dir();
        let launch = live_launch_url(&data_dir, port).expect("日志里应有 launch token URL");
        let cookie = fetch_auth_cookie(&launch, port).expect("launch token 应能换取 cookie");

        let mut request = format!("ws://127.0.0.1:{port}{MUX_PATH}")
            .into_client_request()
            .expect("构造 WebSocket 请求");
        request.headers_mut().insert(
            "Cookie",
            tungstenite::http::HeaderValue::from_str(&cookie).expect("cookie 头合法"),
        );
        request.headers_mut().insert(
            "Origin",
            tungstenite::http::HeaderValue::from_str(&format!("http://127.0.0.1:{port}"))
                .expect("origin 头合法"),
        );
        let (mut socket, _) = tungstenite::connect(request).expect("WebSocket 握手应成功");
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_ref() {
            tcp.set_read_timeout(Some(Duration::from_millis(500)))
                .expect("设置读超时");
        }
        for (stream_id, endpoint) in [
            (STREAM_ID, EVENTS_ENDPOINT),
            (CONTROL_STREAM_ID, CONTROL_ENDPOINT),
        ] {
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "open",
                        "streamId": stream_id,
                        "endpoint": endpoint,
                        "payload": { "args": {} },
                    })
                    .to_string()
                    .into(),
                ))
                .expect("发送 open 帧");
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut baseline = None;
        while std::time::Instant::now() < deadline {
            let Some(frame) = read_frame_until(&mut socket, deadline) else {
                break;
            };
            if frame["streamId"] == CONTROL_STREAM_ID {
                if frame["type"] == "error" {
                    panic!("内核不认 session/control（协议漂移？）：{frame}");
                }
                if frame["value"]["type"] == "baseline" {
                    baseline = Some(frame["value"].clone());
                    break;
                }
            }
        }
        let _ = socket.close(None);
        let baseline = baseline.expect("15 秒内应收到 session/control 的 baseline");

        // baseline 的 id 带 `session-` 前缀，而 `api-session/status` 事件带的是裸
        // uuid：播种必须归一化，否则通知照样查不到名字。
        let mut center = Center::new();
        let before = center.titles.len();
        seed_titles_from_baseline(&mut center, Some(&baseline["value"]));
        let seeded = center.titles.len() - before;
        assert!(seeded > 0, "baseline 里应至少有一个带标题的会话");
        let (id, title) = {
            let (id, title) = center
                .titles
                .iter()
                .find(|(key, _)| !key.ends_with("#cwd"))
                .expect("应至少播种一条标题");
            (id.clone(), title.clone())
        };
        assert!(
            !id.starts_with("session-"),
            "标题表的 key 必须是裸 uuid：{id}"
        );
        assert!(!title.trim().is_empty(), "标题不该是空白：{id} => {title}");

        // 通知文案走的是同一条查表路径：拿裸 uuid 必须能查到刚播种的标题。
        assert!(
            record_status(&mut center, &id, true, 1_000).is_none(),
            "running 事件只记账"
        );
        let task = record_status(&mut center, &id, false, 3_000).expect("跑完一轮应产出完成记录");
        assert_eq!(task.title, title, "完成记录必须用真标题而不是占位文案");
        assert_eq!(task.duration_ms, 2_000, "时长按 running → idle 的墙钟差");
    }

    /// 系统通知通路自检：会在屏幕上真的弹一条通知。
    ///
    /// 用它区分"壳的逻辑问题"和"环境不放行通知"——未打包 / 未安装的构建
    /// （`cargo test` 的测试二进制、`tauri dev` 的裸可执行文件）在 macOS 与
    /// Windows 上都可能发不出通知，这时断言写得再严也没用，因此这里只把真实
    /// 结果打印出来，由人判断。
    #[test]
    #[ignore = "会在屏幕上弹出一条真实系统通知"]
    fn live_system_notification_delivery() {
        let result = notify_rust::Notification::new()
            .summary("dsh-xlink 通知自检")
            .body("看到这条通知说明当前环境的系统通知通路可用。")
            .show();
        match result {
            Ok(_) => println!("系统通知已投递（当前环境可用）"),
            Err(error) => println!("系统通知投递失败：{error}（未打包/未安装的构建属预期）"),
        }
    }
}
