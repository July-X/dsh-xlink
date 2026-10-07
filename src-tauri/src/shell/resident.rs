//! 后台常驻：关窗不退出，跨平台只有一份语义。
//!
//! 2026-10-02 起，Windows 与 macOS 的关闭按钮语义**统一**成「收进后台」：
//! 面板窗口消失，内核、工作台与事件流继续跑，只有托盘 / menu bar 菜单里
//! 显式的「退出 dsh-xlink」才真正结束进程。此前只有 Windows 走这条语义
//! （[`crate::shell::tray::intercept_close`］），macOS 仍是「内核在跑就弹
//! 确认框」——同一份产品在两个平台上的心智模型不一致，用户第一次换平台时
//! 只能靠试。
//!
//! **常驻的范围按用户拍板收窄到「窗口」**：内核仍随壳一起停。`RunEvent::Exit`
//! 里回收内核的逻辑一行没动，也没有引入跨进程的��主协议——那会把「壳退出后
//! 端口还被谁占着」「下次启动认不认得这个孤儿」这两件本来简单的事变成分布式
//! 问题。开机自启因此是「拉起壳并进后台」，内核另设一个开关。
//!
//! 平台实现只提供**图标与菜单**这一层，行为（收起 / 恢复 / 退出）都在这里：
//! Windows 托盘见 [`crate::shell::tray`]，macOS menu bar 见
//! [`crate::shell::menu_bar`]。两者的 `hide_to_shell` / `show_main_shell` /
//! `request_quit` 由本模块按 `cfg` 分发，业务侧只调这三个名字。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

/// 主管理窗口的 label（与 `tauri.conf.json`、其它命令保持一致）。
pub const MAIN_WINDOW: &str = "main";

/// 「从后台恢复时补发提示」每个进程最多发一次，且只在本进程发生过用户
/// 收起之后。
///
/// 恢复只可能由用户主动触发（点托盘 / menu bar 图标、点通知横幅、点 Dock
/// 图标），所以他此刻正看着窗口、也刚证明自己知道怎么把它叫回来——再讲一遍
/// 「程序还在后台、点图标可重新打开」只有遮挡之嫌。放在 Rust 侧而不是前端，
/// 是为了让「每次进程最多一次」在 webview 重新加载后依然成立。
static RESTORE_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

/// 本进程里是否发生过一次**用户主动**的收起（点关闭按钮 / Windows 最小化）。
/// 登录自启的隐藏不算——那不是用户做的动作，恢复时也没有「刚刚去哪了」
/// 可解释。2026-10-05 用户反馈「提示经常触发」后的收紧：登录自启藏起壳后
/// 的第一次唤回不再弹提示（见 [`consume_restore_hint`]）。
static HIDDEN_BY_USER: AtomicBool = AtomicBool::new(false);

/// 用户主动收起时置位。见 [`consume_restore_hint`]。
pub fn mark_hidden_by_user() {
    HIDDEN_BY_USER.store(true, Ordering::Relaxed);
}

/// 本次恢复要不要补发「收进后台」提示。三个条件缺一不发：
/// ① 本进程还没讲过（`RESTORE_HINT_SHOWN`，每个进程生命周期最多一次）；
/// ② 本进程发生过用户收起（登录自启的隐藏不算）；
/// ③ 消费式：讲过或用过即清旗，同一轮的后续恢复一律静默。
/// 拆成纯函数是为了能在无窗口的测试里钉住这组语义。
fn consume_restore_hint() -> bool {
    if RESTORE_HINT_SHOWN.swap(true, Ordering::Relaxed) {
        return false;
    }
    HIDDEN_BY_USER.swap(false, Ordering::Relaxed)
}

/// 进程是否由系统登录项拉起（`--autostart`）。
///
/// 登录项拉起时**不显示面板**，直接进后台——开机时弹一个窗口挡在用户面前，
/// 正是自动启动最招人烦的地方。菜单栏 / 托盘图标就是全部的可见痕迹，用户
/// 想用时点它。
static STARTED_BY_AUTOSTART: AtomicBool = AtomicBool::new(false);

/// 本次进程是不是自动启动拉起的。
pub fn started_by_autostart() -> bool {
    STARTED_BY_AUTOSTART.load(Ordering::Relaxed)
}

/// 记下启动来源。必须在 `setup` 里、任何窗口可见之前调用。
pub fn mark_started_by_autostart(value: bool) {
    STARTED_BY_AUTOSTART.store(value, Ordering::Relaxed);
}

/// 解析命令行里的 `--autostart` 标记。
///
/// 只认**独立的** `--autostart`：内核进程的命令行里也可能出现这个词
/// （`dsh web ...` 的参数由用户自由填写），用 `contains` 扫整串会把
/// 那些情况误判成登录项拉起，于是面板凭空消失而用户没点过任何按钮。
/// `--autostart=true` 这类写法不认——只有我们写进登录项的那一种形态算数。
pub fn detect_autostart_arg<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter().any(|arg| arg == AUTOSTART_FLAG)
}

/// 写进系统登录项时使用的命令行标记。见 [`detect_autostart_arg`]。
pub const AUTOSTART_FLAG: &str = "--autostart";

/// 平台图标与菜单是否可用。
///
/// 两个发布平台（Windows / macOS）都返回 true。这个函数存在的意义不是
/// 「支持不支持」——而是让**共用代码**可以无条件调用 `hide_to_shell`，
/// 而不是自己写一遍 `cfg`。Linux 仍然编不过托盘（`tray.rs` 是
/// `cfg(windows)`），但项目本就不发布 Linux，这里不额外维护一条分支。
pub const fn supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// 把管理面板收进后台：隐藏窗口，并从任务栏（Dock）移除它的按钮。
///
/// Windows 上额外走 `set_skip_taskbar(true)`（`ITaskbarList::DeleteTab`）：
/// 只 `hide()` 不够，窗口虽不可见但任务栏按钮与 Alt+Tab 条目仍在，点它会得到
/// 一个空窗口。恢复时必须加回来，否则窗口回来也不在任务栏上。
pub fn hide_to_shell(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_skip_taskbar(true);
    }
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    {
        // macOS 没有「从 Dock 移除按钮」这回事，但面板不该在 Dock 里留下一个
        // 点了会凭空出现窗口的图标——隐藏窗口时同时退出 Dock 等级，恢复时
        // 加回来。menu bar 图标是唯一的常驻入口，两边语义因此对齐。
        // 注意 `set_activation_policy` 挂在 **AppHandle** 上（macOS 的
        // `NSApp.setActivationPolicy:`），不是窗口上。
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    let _ = window.emit("shell-hidden-to-background", ());
}

/// 把管理面板从后台恢复到前台。
///
/// Windows 上必须做一次 always-on-top 往返：焦点经 IPC 到达时
/// `SetForegroundWindow` 会被系统静默忽略，先置顶再解除才能让窗口真正浮到
/// 最前。macOS 上 `set_focus` 就能拿到前台。
pub fn show_main_shell(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        // 隐藏时被降到 Accessory 等级了，不加回来窗口显示出来也不在 Dock 上、
        // 也拿不到正常的前台键盘焦点。
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_always_on_top(false);
    let _ = window.set_focus();

    // 「程序还在后台、去哪找它」这条提示的补发条件（2026-10-05 收紧，见
    // `consume_restore_hint`）：本进程发生过一次用户收起之后的第一次恢复。
    // 此前是「每次启动后第一次恢复」必发——登录自启把壳藏在后台，用户开机
    // 后第一次点鲸鱼图标就被讲一遍「刚刚把窗口收进了后台」，可他本进程里
    // 根本没收起过任何东西，提示对不上动作，体感就是「经常触发」。
    if consume_restore_hint() {
        let _ = app.emit("shell-restored-from-background", ());
    }
}

/// 真正退出的发起方。
///
/// 与 `show_main_shell` 同一条纪律：**动作只能写一份**。托盘 / menu bar
/// 菜单的「显示主界面」直接调本模块，绝不回调 [`crate::show_main_shell`]。
/// 首版就是在 Windows 上互调两个「显示」函数，点图标立刻
/// `thread 'main' has overflowed its stack`。
pub fn show_from_background(app: &AppHandle) {
    show_main_shell(app);
}

/// 退出请求：复用前端已有的「确认退出」流程。
///
/// 不能在这里直接 `app.exit()`——内核仍在运行时需要先问用户，并让前端依次
/// 执行 `stop_kernel` 与 `confirm_close_shell`。所以只把窗口叫回前台并广播
/// 同一条 `request-quit-confirm` 事件，与操作系统关闭按钮走完全相同的分支。
/// 退出确认弹窗本身画在管理面板里，窗口必须可见才问得出来。
pub fn request_quit(app: &AppHandle) {
    let official_chat_open = app.get_window("official-chat").is_some();
    show_main_shell(app);
    let _ = app.emit(
        "request-quit-confirm",
        serde_json::json!({
            "kernel_running": crate::kernel_running(app),
            "official_chat_open": official_chat_open,
            "from_background": true,
        }),
    );
}

/// 关闭请求的常驻化处理：把管理面板收进后台而不是退出进程。
///
/// 返回 `true` 表示这次关闭已被接管（调用方应停止后续处理）。
/// `prevent_close()` 必须在这里就调用，否则窗口会真的开始关闭。
pub fn intercept_close(app: &AppHandle, label: &str, api: &tauri::CloseRequestApi) -> bool {
    if label != MAIN_WINDOW {
        return false;
    }
    api.prevent_close();
    hide_to_shell(app);
    // 这是**用户主动**的收起：置位后，本进程第一次恢复时会补发「收进后台」
    // 提示（登录自启的隐藏不走这里，所以开机后的第一次唤回是静默的）。
    mark_hidden_by_user();
    true
}

/// 平台图标与菜单的建立。必须在事件循环启动前的 `setup` 里调用。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "windows")]
    crate::shell::tray::setup(app)?;
    #[cfg(target_os = "macos")]
    crate::shell::menu_bar::setup(app)?;
    Ok(())
}

/// 建立「显示 / 退出」两��菜单，并挂上左键叫回窗口的处理器。
///
/// 托盘与菜单栏的**行为**完全一样（同一组动作、同一份恢复提示、同一件事都
/// 发生在同一个 `AppHandle` 上），差别只在图标与平台细节。所以这段共用逻辑
/// 写在这里，两端各传自己的菜单文案与图标——而不是各写一份：2026-10-02 的
/// 检查就抓到过 `menu_bar.rs` 与 `tray.rs` 有 14 行逐字重复，而重复的两份
/// 「退出」接线一旦只有一端改了菜单 id，用户就会点到一个不响应的菜单项。
///
/// `show_label` / `quit_label` 允许两端措辞不同：Windows 托盘说「显示主界面」
/// （它确实有个任务栏按钮的语义在背后），macOS 菜单栏说「显示 Dsh-Xlink」
/// （Dock 上此时什么都没有，名称比「主界面」更让人知道自己在点什么）。
pub(crate) fn build_background_menu(
    builder: tauri::tray::TrayIconBuilder<tauri::Wry>,
    app: &AppHandle,
    show_label: &str,
    quit_label: &str,
) -> tauri::Result<tauri::tray::TrayIcon> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

    let show = MenuItem::with_id(app, MENU_SHOW, show_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    builder
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_from_background(app),
            MENU_QUIT => request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_from_background(tray.app_handle());
            }
        })
        .build(app)
}

/// 菜单项 id 的命名空间。托盘与菜单栏共用——它们永远不会同时存在
/// （各自 `cfg` 到一个平台），共用命名空间换来的是上面那段共用接线。
const MENU_SHOW: &str = "xlink-background-show";
const MENU_QUIT: &str = "xlink-background-quit";

#[cfg(test)]
mod tests {
    use super::*;

    /// 自动启动标记只认独立的 `--autostart`。
    ///
    /// 反向验：带空格的 `--autostart --hide` 必须**不**被认成登录项拉起，
    /// 而独立传入时必须被认出来。用 `contains("autostart")` 那种实现会让
    /// 前者变成 true，于是用户手敲一条带该词的参数就让面板凭空消失。
    #[test]
    fn autostart_flag_must_be_standalone() {
        assert!(detect_autostart_arg([
            "dsh-xlink".to_string(),
            AUTOSTART_FLAG.to_string()
        ]));
        assert!(!detect_autostart_arg([
            "dsh-xlink".to_string(),
            "--autostart --hide".to_string(),
        ]));
        assert!(!detect_autostart_arg([
            "dsh-xlink".to_string(),
            format!("{AUTOSTART_FLAG}=true"),
        ]));
        assert!(!detect_autostart_arg(["dsh-xlink".to_string()]));
    }

    /// 两个发布平台都必须有后台入口，否则「关窗不退出」等于把窗口弄丢。
    #[test]
    fn both_release_platforms_are_supported() {
        assert!(supported(), "Windows 与 macOS 都必须有托盘 / menu bar 入口");
    }

    /// 补发判据（`consume_restore_hint`）的完整语义表：没收起过 → 静默
    /// （登录自启后第一次唤回就在这条路上）；收起过 → 第一次恢复补发、同
    /// 进程后续恢复静默；进程讲过一次后，再收起也不再发。直接操作两个全局
    /// 旗（本文件外没有别的测试碰它们），结束前清干净。
    #[test]
    fn restore_hint_needs_a_session_hide_and_fires_once() {
        // 没收起过：恢复也静默。
        RESTORE_HINT_SHOWN.store(false, Ordering::Relaxed);
        HIDDEN_BY_USER.store(false, Ordering::Relaxed);
        assert!(!consume_restore_hint());
        // 收起过：第一次恢复补发，同进程后续恢复静默。
        mark_hidden_by_user();
        RESTORE_HINT_SHOWN.store(false, Ordering::Relaxed);
        assert!(consume_restore_hint());
        assert!(!consume_restore_hint());
        // 进程已经讲过：再收起也不再发。
        // 必须是 **true**——「讲过」是 `RESTORE_HINT_SHOWN` 为真，写成 `false`
        // 摆的是「本进程还没讲过」，于是 `swap(true)` 返回 false 而落到
        // `HIDDEN_BY_USER` 上，这条断言会变成在检查相反的语义。这里原来正是
        // `false`，测试一直红着——`npm run check` 的 `check:rust` 只跑 fmt /
        // clippy / check --release，**不含 cargo test**，所以没人看见。
        RESTORE_HINT_SHOWN.store(true, Ordering::Relaxed);
        mark_hidden_by_user();
        assert!(!consume_restore_hint());
        // 收尾清旗，不给其它测试留状态。
        RESTORE_HINT_SHOWN.store(false, Ordering::Relaxed);
        HIDDEN_BY_USER.store(false, Ordering::Relaxed);
    }
}
