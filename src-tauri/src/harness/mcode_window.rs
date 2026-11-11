//! Independent MiniMax workbench window and browser profile.

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, OnceLock,
};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::harness::harness_window::{
    close_owned_popup_windows, close_owned_popups, is_loopback_http, new_window_response_with,
    open_external_link, popup_log_origin, title_from_document, PopupOwner, PopupPolicy,
};
use crate::shell::{appearance, paths, window};

/// 建窗等待上限：与其它三扇独立窗口一致，卡死的 WebView 不再让「打开工作台」
/// 永远转圈。
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// 本模块动作的逻辑名（落 `<kind>-mcode-window-<date>.log`，出现在日志面板里）。
const MCODE_WINDOW_LOG: &str = "mcode-window";

// 同一 label 的旧 builder 未返回前，不允许新 builder 抢占它。
// 每轮独立 Arc 是取消令牌；迟到回调只能清理自己持有的窗口。
struct OpenState<W = tauri::WebviewWindow> {
    cancelled: bool,
    building: bool,
    published: bool,
    popup_allowed: Arc<AtomicBool>,
    window: Option<W>,
}

impl<W> OpenState<W> {
    fn new() -> Self {
        Self {
            cancelled: false,
            building: true,
            published: false,
            popup_allowed: Arc::new(AtomicBool::new(false)),
            window: None,
        }
    }

    fn blocks_retry(&self, label_exists: bool) -> bool {
        self.building || (!self.cancelled && !self.published) || (self.cancelled && label_exists)
    }

    fn cancel(&mut self, discard: impl FnOnce(&W)) {
        self.cancelled = true;
        self.popup_allowed.store(false, Ordering::SeqCst);
        if let Some(window) = &self.window {
            discard(window);
        }
    }

    fn cancel_pending(&mut self, discard: impl FnOnce(&W)) {
        if !self.published {
            self.cancel(discard);
        }
    }

    fn teardown(&mut self, discard: impl FnOnce(&W)) {
        if self.published {
            // 已发布窗口由调用方销毁，最终收尾只撤销旧回调权限。
            self.cancel(|_| {});
        } else {
            self.cancel(discard);
        }
    }

    fn complete(
        &mut self,
        result: Result<W, String>,
        discard: impl FnOnce(&W),
    ) -> Result<(), String> {
        self.building = false;
        self.window = Some(result?);
        if self.cancelled {
            discard(self.window.as_ref().unwrap());
            Err("MiniMax 工作台创建已取消".into())
        } else {
            Ok(())
        }
    }

    fn publish(&mut self, show: impl FnOnce(&W) -> Result<(), String>) -> Result<(), String> {
        if self.cancelled {
            return Err("MiniMax 工作台创建已取消，请重新启动".into());
        }
        show(self.window.as_ref().ok_or("MiniMax 工作台未创建，请重试")?)?;
        self.published = true;
        self.popup_allowed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn open_state() -> &'static Mutex<Option<Arc<Mutex<OpenState>>>> {
    static STATE: OnceLock<Mutex<Option<Arc<Mutex<OpenState>>>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

pub(crate) struct PendingOpen {
    rx: Option<mpsc::Receiver<Result<(), String>>>,
    state: Arc<Mutex<OpenState>>,
    published: bool,
}

impl Drop for PendingOpen {
    fn drop(&mut self) {
        if !self.published {
            cancel_open(&self.state);
        }
    }
}

fn cancel_open(state: &Mutex<OpenState>) {
    crate::lock(state).cancel(discard_window);
}

fn discard_window(window: &tauri::WebviewWindow) {
    if let Err(error) = window.destroy() {
        crate::shell::shell_events::record(
            MCODE_WINDOW_LOG,
            &format!("清理已取消的 MiniMax 工作台失败：{error}"),
        );
    }
}

pub(crate) fn cancel_pending_open() {
    if let Some(state) = crate::lock(open_state()).as_ref() {
        // 已显示窗口仍由停止命令在服务停止成功后销毁，保留原有失败语义。
        crate::lock(state).cancel_pending(discard_window);
    }
}

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
    if let Some(state) = crate::lock(open_state()).as_ref() {
        crate::lock(state).teardown(discard_window);
    }
    release_owner(|owner| close_owned_popups(app, owner))
}

/// 清掉全局 owner 并关闭它名下的弹窗。
///
/// 销毁失败只落日志、不否决调用方：清弹窗是收尾动作，它失败不该让「打开工作台」
/// 失败，也不该让壳退不出去（闸门已封，不会再有新弹窗；残留弹窗下次再收）。
///
/// owner 一定要清掉——留着它，下一次重试撞的是同一条错误，工作台就再也打不开了。
fn release_owner(close: impl FnOnce(&PopupOwner) -> Result<(), String>) -> Result<(), String> {
    let owner = crate::lock(popup_owner()).take();
    let Some(owner) = owner else {
        return Ok(());
    };
    if let Err(error) = close(&owner) {
        crate::shell::shell_events::record(
            MCODE_WINDOW_LOG,
            &format!("关闭 MiniMax 授权弹窗失败：{error}"),
        );
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

/// 发起建窗，把结果经 channel 交回；**等待由调用方在不持锁时做**。
///
/// 等待放进调用方的锁里会堵住「停止 MiniMax」与退出（那两条路也要拿同一把
/// 窗口锁），建窗最多要等 20 秒。
///
/// 返回 `None` 表示窗口已经开着、已聚焦，没有要等的结果。
pub(crate) fn open(
    app: &AppHandle,
    instance_id: &str,
    port: u16,
) -> Result<Option<PendingOpen>, String> {
    if let Some(state) = crate::lock(open_state()).as_ref() {
        let state = crate::lock(state);
        if state.blocks_retry(
            app.get_webview_window(appearance::MCODE_WORKBENCH_LABEL)
                .is_some(),
        ) {
            return Err("上一轮 MiniMax 工作台仍在创建或清理，请稍后重试；若持续无法结束，请重启应用并查看 mcode-window 日志".into());
        }
    }
    if let Some(existing) = app.get_webview_window(appearance::MCODE_WORKBENCH_LABEL) {
        existing.show().map_err(|error| error.to_string())?;
        existing.unminimize().map_err(|error| error.to_string())?;
        existing.set_focus().map_err(|error| error.to_string())?;
        return Ok(None);
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
    let state = Arc::new(Mutex::new(OpenState::new()));
    *crate::lock(open_state()) = Some(state.clone());
    let worker_state = state.clone();
    let worker_owner = owner.clone();
    let popup_allowed = crate::lock(&state).popup_allowed.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("mcode-open-workbench".into())
        .spawn(move || {
            let result = build(
                &handle,
                url,
                profile,
                identifier,
                worker_owner,
                popup_allowed,
            );
            let mut state = crate::lock(&worker_state);
            let result = state.complete(result, discard_window);
            finish_build(tx, result);
        })
    {
        let mut state = crate::lock(&state);
        state.building = false;
        state.cancelled = true;
        return Err(format!("创建 MiniMax 工作台线程失败：{error}"));
    }
    Ok(Some(PendingOpen {
        rx: Some(rx),
        state,
        published: false,
    }))
}

/// 建窗线程的收尾：结果交回等待者；等待者已经走了（超时）就把失败原因落盘，
/// 不能丢在线程里。
fn finish_build(tx: mpsc::Sender<Result<(), String>>, result: Result<(), String>) {
    match result {
        Ok(()) => {
            let _ = tx.send(Ok(()));
        }
        Err(error) => {
            if tx.send(Err(error.clone())).is_err() {
                crate::shell::shell_events::record(MCODE_WINDOW_LOG, &error);
            }
        }
    }
}

/// 等建窗结果。调用方在**不持窗口锁**的时候调它。
///
/// 只在等待成功且本轮未取消时显示隐藏窗口；停止和发布共享同一把短锁。
pub(crate) fn await_open(mut pending: PendingOpen) -> Result<(), String> {
    let result = await_open_with(
        pending.rx.take().expect("建窗结果只能等待一次"),
        OPEN_TIMEOUT,
    )
    .and_then(|()| {
        crate::lock(&pending.state).publish(|window| {
            window
                .show()
                .and_then(|()| window.set_focus())
                .map_err(|error| error.to_string())
        })
    });
    pending.published = result.is_ok();
    result
}

fn await_open_with(
    rx: mpsc::Receiver<Result<(), String>>,
    timeout: Duration,
) -> Result<(), String> {
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "打开 MiniMax 工作台超时（{} 秒）。请重试；若反复超时，请查看日志定位 WebView 创建失败的原因",
            timeout.as_secs()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("创建 MiniMax 工作台的线程意外结束，请查看日志后重试".into())
        }
    }
}

/// 顶层导航去哪：工作台只加载自己的回环页面。
#[derive(Debug, PartialEq, Eq)]
enum McodeNavigation {
    /// 本工作台自己的页面（回环 http(s)），以及页面自己的 `about:blank` / `blob:` / `data:`。
    Keep,
    /// 外部 http(s)：阻止并交系统浏览器，工作台不被换走。
    External,
    /// 其它 scheme（`file:` / `tauri:` / `javascript:` 等）：拒绝。
    Reject,
}

fn navigation_route(url: &Url) -> McodeNavigation {
    if (url.scheme() == "about" && url.path() == "blank")
        || matches!(url.scheme(), "blob" | "data")
        || is_loopback_http(url)
    {
        McodeNavigation::Keep
    } else if matches!(url.scheme(), "http" | "https") {
        McodeNavigation::External
    } else {
        McodeNavigation::Reject
    }
}

/// `on_navigation` 的处置：页面里的普通外链（当前页跳转）不再把整扇工作台换成
/// 别人的站点。日志只记来源，地址里的路径与授权参数不落盘。
fn handle_navigation(app: &AppHandle, url: &Url) -> bool {
    match navigation_route(url) {
        McodeNavigation::Keep => true,
        McodeNavigation::External => {
            let opened = open_external_link(app, url);
            crate::shell::shell_events::record(
                MCODE_WINDOW_LOG,
                &format!(
                    "MiniMax 工作台顶层外链已交系统浏览器（来源：{}）{}",
                    popup_log_origin(url),
                    if opened {
                        ""
                    } else {
                        "；系统浏览器未能打开"
                    }
                ),
            );
            false
        }
        McodeNavigation::Reject => {
            crate::shell::shell_events::record(
                MCODE_WINDOW_LOG,
                &format!(
                    "MiniMax 工作台已拒绝顶层导航（来源：{}）",
                    popup_log_origin(url)
                ),
            );
            false
        }
    }
}

fn build(
    app: &AppHandle,
    url: Url,
    profile: PathBuf,
    identifier: [u8; 16],
    owner: Arc<PopupOwner>,
    popup_allowed: Arc<AtomicBool>,
) -> Result<tauri::WebviewWindow, String> {
    let popup_app = app.clone();
    let navigation_app = app.clone();
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
    .visible(false)
    // WebView2 的 Tauri 拖放处理器会截获网页原生 HTML5 拖放事件。
    .disable_drag_drop_handler()
    .background_color(window::CHROME_BACKDROP)
    .theme(crate::shell::appearance::initial_theme(
        appearance::MCODE_WORKBENCH_LABEL,
    ))
    .data_directory(profile)
    .on_new_window(move |link, features| {
        if !popup_allowed.load(Ordering::SeqCst) {
            return tauri::webview::NewWindowResponse::Deny;
        }
        new_window_response_with(&popup_app, link, features, policy.clone())
    })
    // 页面里的普通外链不许把工作台换成别人的站点（外链交系统浏览器，见处理函数）。
    .on_navigation(move |url| handle_navigation(&navigation_app, url))
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
    .initialization_script(include_str!("mcode-login.js"))
    // 拉绳灯：与 dsh 工作台共用同一份注入脚本；先写 surface 标记，让它把这
    // 一面认成 MiniMax（两者同为 loopback origin，URL 上分不出来），再按
    // `[data-webui-shell]` 与 `--webui-rail-width` 把灯挂到 rail 右缘。
    .initialization_script("window.__DSH_SHELL_SURFACE__ = 'mcode';")
    .initialization_script(include_str!("pullstring-launcher.js"));
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(identifier);
    #[cfg(not(target_os = "macos"))]
    let _ = identifier;
    builder
        .build()
        .map_err(|error| format!("打开 MiniMax 工作台失败：{error}；请查看日志后重试"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_builder_is_discarded_before_retry_and_never_published() {
        let state = Arc::new(Mutex::new(OpenState::<u32>::new()));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let worker = state.clone();
        let build = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let mut discarded = Vec::new();
            let result = crate::lock(&worker).complete(Ok(1), |id| discarded.push(*id));
            let _ = tx.send(result);
            discarded
        });
        ready_rx.recv().unwrap();
        assert!(await_open_with(rx, Duration::from_millis(1))
            .unwrap_err()
            .contains("超时"));
        crate::lock(&state).cancel(|_| panic!("builder 尚未返回"));
        assert!(crate::lock(&state).blocks_retry(false));
        release_tx.send(()).unwrap();
        assert_eq!(build.join().unwrap(), vec![1]);
        let mut old = crate::lock(&state);
        assert!(old.publish(|_| panic!("迟到窗口不能显示")).is_err());
        assert!(old.blocks_retry(true), "销毁尚未完成时不能复用旧 label");
        assert!(!old.blocks_retry(false));

        let mut restarted = OpenState::new();
        restarted
            .complete(Ok(2), |_| panic!("新一轮不应取消"))
            .unwrap();
        assert!(
            restarted.blocks_retry(true),
            "等待者认领前不能从 open 绕过发布闸门"
        );
        restarted
            .publish(|id| {
                assert_eq!(*id, 2);
                Ok(())
            })
            .unwrap();
        old.cancel(|id| assert_eq!(*id, 1));
        assert!(!restarted.cancelled, "旧等待者不得取消重启后的新一轮");
        assert!(restarted.popup_allowed.load(Ordering::SeqCst));
    }

    #[test]
    fn stop_between_build_and_publish_cleans_only_that_window() {
        let mut state = OpenState::new();
        state.complete(Ok(7), |_| panic!("尚未取消")).unwrap();
        let mut discarded = Vec::new();
        state.cancel(|id| discarded.push(*id));
        assert_eq!(discarded, vec![7]);
        assert!(state.publish(|_| panic!("停止后不能显示")).is_err());
        assert!(!state.popup_allowed.load(Ordering::SeqCst));
    }

    #[test]
    fn cancelling_pending_open_preserves_published_window_when_stop_fails() {
        let mut state = OpenState::new();
        state.complete(Ok(9), |_| panic!("尚未取消")).unwrap();
        state.publish(|_| Ok(())).unwrap();
        state.cancel_pending(|_| panic!("服务停止尚未成功，不能销毁已显示窗口"));
        assert!(!state.cancelled);
        assert!(!state.blocks_retry(true));
        assert!(state.popup_allowed.load(Ordering::SeqCst));
        assert_eq!(state.window, Some(9));
    }

    #[test]
    fn final_teardown_revokes_callbacks_without_destroying_published_window_again() {
        let mut state = OpenState::new();
        state.complete(Ok(9), |_| panic!("尚未取消")).unwrap();
        state.publish(|_| Ok(())).unwrap();
        state.teardown(|_| panic!("调用方已销毁，不能重复销毁"));
        assert!(state.cancelled);
        assert!(!state.popup_allowed.load(Ordering::SeqCst));
        assert!(!state.blocks_retry(false));

        let mut pending = OpenState::new();
        pending.complete(Ok(10), |_| panic!("尚未取消")).unwrap();
        let mut discarded = Vec::new();
        pending.teardown(|id| discarded.push(*id));
        assert_eq!(discarded, vec![10]);
        assert!(pending.publish(|_| panic!("收尾后不能发布")).is_err());
    }

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

    #[test]
    fn workbench_keeps_only_its_own_loopback_pages() {
        for url in [
            "http://127.0.0.1:17800/",
            "http://localhost:17800/session/1",
            "https://127.0.0.1:17800/",
            "about:blank",
            "blob:http://127.0.0.1:17800/0f9a",
            "data:text/plain,ok",
        ] {
            assert_eq!(
                navigation_route(&Url::parse(url).unwrap()),
                McodeNavigation::Keep,
                "{url}"
            );
        }
        for url in ["https://account.minimax.io/login", "http://example.com/"] {
            assert_eq!(
                navigation_route(&Url::parse(url).unwrap()),
                McodeNavigation::External,
                "{url}"
            );
        }
        for url in [
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "tauri://localhost/",
        ] {
            assert_eq!(
                navigation_route(&Url::parse(url).unwrap()),
                McodeNavigation::Reject,
                "{url}"
            );
        }
    }

    /// 纯函数只证明路由；这条钉接线——摘掉 `.on_navigation` 或把等待搬回
    /// 锁里时路由测试照样全绿。判据字符串拆开拼，避免匹配到本文件里的这段测试自己。
    #[test]
    fn workbench_window_wires_navigation_guard_and_bounded_wait() {
        let source = include_str!("mcode_window.rs");
        assert!(source.contains(&[".visible", "(false)"].concat()));
        assert!(source.contains(&["state.complete(result, ", "discard_window)"].concat()));
        assert!(source.contains(&["crate::lock(&pending.state)", ".publish("].concat()));
        let commands = include_str!("../commands.rs");
        assert_eq!(
            commands
                .matches(&["mcode_window::", "cancel_pending_open();"].concat())
                .count(),
            2,
            "停止和退出都必须先取消尚未发布的窗口"
        );
        assert!(
            source.contains(&[".on_navigation", "(move |url| handle_navigation("].concat()),
            "MiniMax 工作台建窗链必须挂顶层导航闸"
        );
        assert!(
            source.contains(&["Ok(Some(", "PendingOpen {"].concat()),
            "open 必须把建窗结果经 channel 交回：等待要在调用方的锁外做"
        );
        assert!(
            source.contains(&["pub(crate) fn await_open", "("].concat()),
            "等待建窗结果必须是一个独立入口"
        );
        assert!(
            !source.contains(&["await_open", "(rx)"].concat()),
            "建窗等待不能在 open 里做：调用方持着全局窗口锁，20 秒会把停止与退出一起堵住"
        );
    }

    /// 弹窗要拿到登录态，靠的是它与工作台窗口**同一份浏览器档案**：
    /// `profile_dir`（Windows）与 `data_store_identifier`（macOS）必须就是建窗
    /// 那两个变量，不能各算各的。写错不报错——认证链在弹窗里跑完、工作台拿不到
    /// 登录态，用户只看到「登录做完了但还是未登录」，壳内一行日志都没有。
    #[test]
    fn popups_inherit_the_workbench_browser_profile() {
        let source = include_str!("mcode_window.rs");
        // 判据字符串拆开拼，避免匹配到本文件里的这段判据自己。
        assert!(
            source.contains(&["profile_dir: Some(profile", ".clone())"].concat()),
            "弹窗策略必须带工作台那份 profile 目录"
        );
        assert!(
            source.contains(&["data_store_identifier: Some(", "identifier)"].concat()),
            "弹窗策略必须带工作台那份 data store 标识符"
        );
        assert!(
            source.contains(&[".data_directory", "(profile)"].concat()),
            "工作台窗口自己必须用同一份 profile 目录"
        );
        assert!(
            source.contains(&["data_store_identifier", "(identifier)"].concat()),
            "工作台窗口自己必须绑同一份 data store 标识符"
        );
    }

    /// 读壳事件日志里某个逻辑名的全部内容。
    fn log_text(logical_name: &str) -> String {
        let logs = crate::shell::process::shell_logs_dir();
        let mut text = String::new();
        for entry in std::fs::read_dir(&logs)
            .expect("日志目录应已创建")
            .flatten()
        {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.contains(logical_name) && name.ends_with(".log") {
                text.push_str(&std::fs::read_to_string(entry.path()).expect("读回事件日志"));
            }
        }
        text
    }

    /// 清理旧弹窗失败不得否决调用方，也不得把 owner 留在原地——留着它，下一次
    /// 重试撞的是同一条错误，MiniMax 工作台就再也打不开了。
    #[test]
    fn closing_popups_never_fails_and_releases_the_owner() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-popups-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        *crate::lock(popup_owner()) = Some(PopupOwner::new());
        assert!(
            release_owner(|_| Err("销毁失败".into())).is_ok(),
            "清弹窗失败不得否决调用方"
        );
        assert!(
            crate::lock(popup_owner()).is_none(),
            "owner 必须被清掉，否则下一次重试还是它"
        );
        assert!(
            log_text(MCODE_WINDOW_LOG).contains("销毁失败"),
            "销毁失败要落进 mcode-window 日志"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 建窗失败发生在等待者超时之后：原因要落盘，不能丢在线程里。
    #[test]
    fn a_late_build_failure_is_written_to_the_log() {
        let home =
            std::env::temp_dir().join(format!("xlink-mcode-late-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let (tx, rx) = mpsc::channel();
        drop(rx); // 等待者已经超时走了
        finish_build(tx, Err("建窗失败：模拟".into()));
        assert!(
            log_text(MCODE_WINDOW_LOG).contains("建窗失败：模拟"),
            "迟到的建窗失败要落进 mcode-window 日志"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn open_wait_reports_timeout_build_error_and_dead_thread() {
        let (tx, rx) = mpsc::channel();
        tx.send(Ok(())).unwrap();
        assert!(await_open_with(rx, Duration::from_millis(10)).is_ok());

        let (_tx, rx) = mpsc::channel();
        let error = await_open_with(rx, Duration::from_millis(10)).unwrap_err();
        assert!(error.contains("超时"), "{error}");

        let (tx, rx) = mpsc::channel();
        tx.send(Err("建窗失败".to_string())).unwrap();
        assert_eq!(
            await_open_with(rx, Duration::from_millis(10)).unwrap_err(),
            "建窗失败"
        );

        let (tx, rx) = mpsc::channel();
        drop(tx);
        assert!(await_open_with(rx, Duration::from_millis(10))
            .unwrap_err()
            .contains("意外结束"));
    }
}
