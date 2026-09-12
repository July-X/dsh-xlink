//! dsh-xlink：围绕 DeepSeek Harness 内核的 Tauri 壳。
//!
//! 壳负责管理固定版本的内核（通过 pnpm 从官方 `dsh-v*` 发布版本安装）、
//! 运行当前激活内核的 `dsh web` 服务器，并在专属 webview 窗口中打开其
//! UI。所有管理操作都通过 [`commands`] 中的命令，由本地 `ui/` 前端
//! 发起。

mod archive;
mod commands;
mod env;
mod error;
mod guard;
mod kernel;
mod node;
mod node_install;
mod notify;
mod patches;
mod pkg;
mod plugins;
mod process;
mod quarantine;
mod registry;
mod releases;
mod settings;
mod skills;
mod state;
#[cfg(target_os = "windows")]
mod tray;
mod updater;
mod version;

use std::sync::Mutex;

use commands::AppState;
use tauri::{Emitter, Manager, WindowEvent};

/// 锁定一个互斥锁；当另一个线程 panic 时取回内部值。
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// macOS 自检：管理面板窗口必须是「可最小化」的，否则标题栏那盏黄灯是个死按钮。
///
/// 这是「点黄灯没反应」那个 bug 的回归哨兵。根因在 AppKit 的
/// `NSWindowStyleMask`：tao 的 `set_decorations(false)` 会把样式位重算成
/// `Borderless | Resizable`（tao 0.35 `platform_impl/macos/window.rs`），
/// 其中**不含** `Miniaturizable`，而 `miniaturize:` 在窗口不带该位时静默失败
/// ——`isMiniaturized` 永远是 false、不报错，Tauri 的 `minimize()` 也把这次
/// 失败当成功返回 `Ok(())`，前端连 toast 都触发不了。修复办法是让窗口**出生
/// 即无边框**（`tauri.conf.json` 的 `decorations: false`，见那里的注释），
/// 而不是建窗后再改一次装饰：后者那次样式重算会把默认的 `Miniaturizable`
/// 一起抹掉，而且它由 tao 异步排到主线程队列，在 `setup()` 里补设也会被覆盖。
///
/// `is_minimizable()` 读的就是 AppKit 的 `isMiniaturizable`，所以这里能真实
/// 反映黄灯是否可用。只记录、不中断启动：窗口仍可用关闭按钮与 `Cmd+M` 操作，
/// 不值得为一个按钮拒绝启动。走 `spawn_blocking` 是因为该查询要同步回主线程，
/// 直接在调用线程上等待会把它占住。
#[cfg(target_os = "macos")]
fn check_main_window_minimizable(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!(
            "dsh-xlink: 找不到管理窗口（label: main），无法确认标题栏黄灯是否可用。\
             请重启应用；若问题持续，请用 `npm run dev` 在终端启动，\
             连同上面的完整输出一起反馈。"
        );
        return;
    };
    // 该查询要同步回主线程，直接在调用线程上等待会把它占住。
    tauri::async_runtime::spawn_blocking(move || {
        if window.is_minimizable().unwrap_or(false) {
            return;
        }
        eprintln!(
            "dsh-xlink: 管理窗口不是「可最小化」的，标题栏黄灯点了不会有反应。\
             请改用系统快捷键 Cmd+M；重启应用可重试这次设置。\
             若问题持续，请用 `npm run dev` 在终端启动，连同上面的完整输出一起反馈。"
        );
    });
}

/// 应用入口；由 `main.rs` 调用。
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // 管理面板在 macOS / Windows 上使用本地 Vue 标题栏绘制交通灯和品牌
            // 背景：这两个平台的「无边框」由 `tauri.conf.json` 的 `decorations:
            // false` 在**建窗时**给定，这里刻意不再调 `set_decorations(false)`。
            //
            // 运行时改装饰会顺手关掉 macOS 窗口的「可最小化」样式位，黄灯随即
            // 变成点了没反应的死按钮（`miniaturize:` 静默失败，Tauri 的
            // `minimize()` 还返回 `Ok(())`，前端连提示都弹不出来）；而且这次
            // 样式重算由 tao 异步排到主线程队列，在 `setup()` 里紧接着补设
            // `set_minimizable(true)` 也会被它覆盖（已实测）。声明式配置没有
            // 这个问题：窗口出生就是无边框的，交通灯/标题栏按钮的语义位全部
            // 保留。细节见下面 `check_main_window_minimizable` 的文档注释。
            #[cfg(target_os = "macos")]
            check_main_window_minimizable(app);

            let data_dir = kernel::data_dir(app.handle());
            // 把解析出的 data dir 打到 stderr，让同时运行 `tauri dev` 与
            // 已安装 release 壳的开发者能一眼看出到底是哪一个
            // （release → `~/.dsh/desktop/`，debug → `~/.dsh/desktop-dev/`）
            // 真正拥有这个进程。这种廉价的保险能避免经典的「我在 dev 壳
            // 里改了设置，release 壳却看不到」踩坑。
            eprintln!(
                "dsh-xlink: data_dir = {} (build: {})",
                data_dir.display(),
                if cfg!(debug_assertions) {
                    "dev"
                } else {
                    "release"
                }
            );
            // 必须在任何状态管理之前回收属于本 data dir 的孤儿 dsh web
            // 内核：崩溃 / 被杀的壳会让它的内核以 cwd == data_dir 继续
            // 运行，同一项目目录上两个内核会向同一会话日志追加内容，
            // 导致日志损坏（seq gap）。必须早于 start_kernel，否则
            // start_kernel 会观察到「端口已被占用」，把孤儿当作健康实例。
            kernel::reap_orphans(&data_dir);
            app.manage(AppState {
                data_dir,
                running: Mutex::new(None),
                lifecycle: Mutex::new(()),
                node_cache: Mutex::new(None),
                harness_url: Mutex::new(None),
            });
            // 崩溃恢复：清理上一次壳运行中途死亡留下的 plugin store
            // staging 目录。正常路径下（无残留）只是一次 read_dir 扫描，
            // 因此可以无条件在这里跑，而不必加 marker 文件做门控。必须
            // 在任何 plugin 命令触及 store 之前运行，而 setup 时还没有
            // 命令这么做。
            plugins::reconcile_store(&app.state::<AppState>().data_dir);
            // skill store 的同类启动期修复：恢复 staging 交换、重新
            // 链接缺失的 active-root 条目、清理孤立的 store 链接。
            // 纯文件系统操作；失败信息会落到 skill store 的 warning
            // 字段供 UI 展示。
            skills::reconcile();
            // 日志目录的启动期修复：把旧命名（`X.log.1`，扩展名是 `1`）的
            // 轮转备份改名为 `X.1.log`，它们此前永远不出现在日志面板里。
            // 纯改名、失败即跳过，不影响启动。
            let logs_dir = kernel::logs_dir(&app.state::<AppState>().data_dir);
            crate::process::migrate_legacy_rotated_logs(&logs_dir);
            // 过期日志清理：每个「日期 × kind」三代 × 8 MiB 且日期只增不减，
            // 不裁剪会长期累积到 GB 级（P2-62）。
            let removed = crate::process::prune_old_logs(&logs_dir);
            if removed > 0 {
                eprintln!("dsh-xlink: 已清理 {removed} 个过期日志文件（保留 30 天 / 200 MiB）");
            }
            updater::spawn_background_check(app.handle());
            // Windows：建立通知区域图标。管理面板在这里是「常驻后台」的
            // ——关闭与最小化都只是收起窗口，托盘是唯一的重开与退出入口，
            // 所以它必须在任何窗口可能被收起之前就绪。失败不阻断启动：
            // 没有托盘时窗口仍可正常使用，只是「收起后只能靠重新启动找回」
            // 这一退化行为，代价写进日志供排查。
            #[cfg(target_os = "windows")]
            if let Err(error) = tray::setup(app.handle()) {
                eprintln!(
                    "dsh-xlink: 无法建立通知区域图标（{error}）；\
                     关闭按钮仍会把窗口收进后台，但届时只能通过重新启动应用找回界面。\
                     若界面显示异常，重启应用重试；仍失败请用 `npm run dev` 在终端启动，\
                     连同上面的完整输出一起反馈。"
                );
            }
            // 在 debug 构建中自动打开管理窗口的 DevTools。
            // Tauri 的 webview 快捷键（`Cmd+Option+I`、`Cmd+Shift+I`、
            // F12）在 macOS 上不一定能触达 WKWebView，因此调试入口
            // 必须从嵌入端主动打开。`setup` 在已配置窗口创建之后
            // 才触发，所以这里的 `main` webview 已经可取；
            // `#[cfg(debug_assertions)]` 门控让 release 构建
            // （其本身就 `with_devtools(false)`）免于这次调用。
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::detect_node,
            commands::install_node,
            commands::save_settings,
            commands::list_log_files,
            commands::read_log_file,
            commands::open_data_dir,
            commands::check_shell_update,
            commands::install_shell_update,
            commands::confirm_shell_ready,
            commands::fetch_releases,
            commands::install_kernel,
            commands::activate_version,
            commands::remove_version,
            commands::start_kernel,
            commands::stop_kernel,
            commands::open_harness,
            commands::report_harness_fault,
            commands::open_log_window,
            commands::open_official_chat,
            commands::close_official_chat,
            commands::official_chat_tabs,
            commands::switch_official_chat_tab,
            commands::focus_main_shell,
            commands::minimize_shell,
            commands::plugin_status,
            commands::kernel_plugin_list,
            commands::plugin_install,
            commands::plugin_update,
            commands::plugin_uninstall,
            commands::plugin_sync,
            commands::plugin_set_mode,
            commands::plugin_check_updates,
            commands::plugin_catalog,
            commands::plugin_resolve,
            commands::patch_status,
            commands::patch_apply,
            commands::patch_revert,
            commands::skill_status,
            commands::skill_install,
            commands::skill_update,
            commands::skill_uninstall,
            commands::skill_set_enabled,
            commands::skill_check_updates,
            commands::notification_status,
            commands::notification_mark_read,
            commands::notification_save_settings,
            commands::notification_test,
            commands::confirm_close_shell,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| {
            // 走到这里说明 Tauri 连应用实例都没建起来（窗口/插件/运行时初始化
            // 失败）。`generate_context!` 已在编译期把配置与前端资源清单烘焙进来，
            // 所以运行期失败基本是环境问题而非代码缺陷 —— 给出可操作的下一步，
            // 而不是把英文 panic 甩给用户。
            eprintln!(
                "dsh-xlink: 启动失败，无法创建应用窗口：{error}\n\
                 请先确认管理面板产物存在（在项目根目录执行 `npm run build:ui` 生成 ui/dist），\
                 然后重新启动应用；\n\
                 若仍失败，用 `npm run dev` 在终端启动可看到完整输出，请连同上面的信息一起反馈。"
            );
            std::process::exit(1);
        });

    // 在壳退出时回收内核，使 app 退出后不会留下仍在服务的 dsh web 进程。
    // 内存中的 child 覆盖本会话启动的内核；pid 文件覆盖上一次壳运行
    // （例如崩溃后）留下的孤儿，由 `kill_pid` 的内核检查把关。
    //
    // 管理窗口的关闭请求分两条路：
    //   · Windows（托盘常驻）：关闭只是把窗口收进通知区域，内核与工作台继续
    //     运行；只有托盘菜单的「退出」才走确认与退出流程。这条分支在
    //     `tray::intercept_close` 里实现，并且**优先**于退出确认——否则每次
    //     点 X 都会弹一次「完全退出？」，与「收起后台」的语义自相矛盾。
    //   · 其它平台：沿用原有询问语义。内核仍在运行或 official-chat 仍打开时
    //     `prevent_close()` 并通知 UI 询问用户是否完全退出；UI 接着运行
    //     `stop_kernel`（运行时）然后 `confirm_close_shell`，销毁所有窗口并
    //     退出事件循环。没有这一提示，用户可能关掉面板却留下占用端口的孤儿
    //     内核，下次启动会因为误导性的「端口已被占用」诊断而失败，直到下一次
    //     壳启动时才回收该孤儿。
    //
    // 提示路径不能依赖 `RunEvent::Exit` 来拆窗口：`confirm_close_shell`
    // 会销毁主窗口，但事件循环只在最后一个窗口消失时才结束（macOS 上
    // 即便如此也不结束——需要显式 exit），所以一个仍打开的 `official-chat`
    // 窗口会让循环（以及 app）继续存活，却无人关闭它。因此下面的 Exit
    // 分支只是绕过提示的那些退出（Cmd+Q、操作系统关机、
    // Windows/Linux 上无需警告时最后窗口的自动关闭）的回退路径。
    app.run(|handle, event| {
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } = &event
        {
            // Windows：把关闭改写成「收进托盘」，不再询问是否退出
            //（退出改由托盘菜单显式发起）。
            #[cfg(target_os = "windows")]
            if tray::intercept_close(handle, label, api) {
                return;
            }
            // 只拦截管理窗口的关闭按钮；harness 工作台 webview
            //（标签 "harness"）可以无需确认直接关闭，因为它自身不持有
            // 内核句柄。
            let official_chat_open = handle.get_window("official-chat").is_some();
            if label == "main" && (kernel_running(handle) || official_chat_open) {
                // 内核仍在运行或官方聊天窗口已打开：在拆除壳之前先询问
                // 用户——确认退出则一并关闭。prevent_close() 暂停关闭；
                // UI 要么确认（在运行时停止内核，然后调用
                // confirm_close_shell），要么取消，让所有窗口保持原样。
                // 这里不会销毁任何东西，因此取消操作不会把 official-chat
                // 窗口一起带走。
                api.prevent_close();
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.emit(
                        "request-quit-confirm",
                        serde_json::json!({
                            "kernel_running": kernel_running(handle),
                            "official_chat_open": official_chat_open,
                        }),
                    );
                }
            }
            // 不是主窗口，或内核与官方聊天都不需要警告：让关闭继续。
            // 下面的 Exit 分支会在每次真实退出时级联关闭 official-chat
            // webview，并回收上次崩溃留下的 pid 文件。
        }
        // 工作台窗口的前台状态：用户切回工作台即视为"看过结果了"，未读角标
        // 清零。只认 `harness`——用户盯着管理面板时并不算看到了对话结果，
        // 那种情况下仍然应该收到通知（见 `notify::set_workbench_focused`）。
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::Focused(focused),
            ..
        } = &event
        {
            if label == "harness" {
                notify::set_workbench_focused(handle, *focused);
            }
        }
        // 窗口被销毁（用户点系统的关闭按钮、或 `stop_kernel` 的 `destroy()`）
        // 时必须显式清掉前台标记：销毁不一定伴随 `Focused(false)`，而标记一旦
        // 卡在 true，之后所有"离开时完成"的任务都会被当成"用户正在看"而永不
        // 提醒——通知功能就此静默失效。
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::Destroyed,
            ..
        } = &event
        {
            if label == "harness" {
                notify::set_workbench_focused(handle, false);
            }
        }
        if let tauri::RunEvent::Exit = event {
            // 在绕过退出提示的那些退出路径（macOS 上的 Cmd+Q、操作系统
            // 关机、无需警告时的最后窗口自动关闭）上级联关闭 official-chat
            // 这个由面板驱动的窗口。确认退出路径已经在退出循环前通过
            // `confirm_close_shell` 销毁了所有窗口——这里的 Exit 只是
            // 回退兜底，不是主要的拆窗路径。理由与上面的 WindowEvent
            // 处理相同：official-chat 是一个由面板驱动的临时窗口，
            // 关闭面板就意味着关闭它。
            if let Some(oc) = handle.get_window("official-chat") {
                let _ = oc.destroy();
            }
            if let Some(state) = handle.try_state::<AppState>() {
                {
                    let mut guard = lock(&state.running);
                    if let Some(mut child) = guard.take() {
                        let _ = kernel::stop(&mut child);
                    }
                }
                let data_dir = state.data_dir.clone();
                let current = settings::load(&data_dir);
                // 判据同样不看配置端口：内核可能绑在用户改端口之前的那个
                // 端口上，用当前配置端口探测会漏掉它，让它继续占着端口活到
                // 下一次启动。
                if let Some(pid) = kernel::workbench_pid(&data_dir, &current) {
                    // 同 stop_kernel：带上记录里的启动端口（P2-1）。
                    kernel::kill_pid(pid, kernel::recorded_kernel_port(&data_dir));
                }
                kernel::clear_pid(&data_dir);
            }
            // 事件订阅线程是壳自己的：退出前显式停掉并等在途的重连/读循环
            // 收尾，避免进程退出时留下一个还在往已销毁的 AppHandle 上发事件
            // 的线程。
            notify::stop_watcher();
        }
    });
}

/// 把管理面板主窗口恢复到前台（`show` + 取消最小化 + 前台聚焦）。
///
/// 按平台分发到唯一一份实现：Windows 用 [`tray::show_main_shell`]（与托盘图标
/// 的「显示主界面」共用同一份），其它平台就地实现。**不要把实现挪回本函数、
/// 再让 `tray::show_main_shell` 回调它**——那会构成无限递归，Windows 上点托盘
/// 图标会直接 `thread 'main' has overflowed its stack`（首版即如此）。
///
/// Windows 上必须做一次 always-on-top 往返：焦点经 IPC 到达时
/// `SetForegroundWindow` 会被系统静默忽略，先置顶再解除才能让窗口真正浮到
/// 最前；其它平台是无害的 no-op。
pub(crate) fn show_main_shell(handle: &tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    tray::show_main_shell(handle);
    #[cfg(not(target_os = "windows"))]
    {
        let Some(window) = handle.get_webview_window("main") else {
            return;
        };
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_always_on_top(true);
        let _ = window.set_always_on_top(false);
        let _ = window.set_focus();
    }
}

/// 内核当前是否在对外服务。
///
/// 内存中的句柄只有在内核**确实还活着**时才算数：内核自行退出或被外部杀掉
/// 之后句柄仍留在槽位里，只看 `is_some()` 会让这里一直撒谎（关窗时弹出一个
/// 与实际状态矛盾的确认为）。句柄失效后回落到 [`kernel::workbench_running`]，
/// 它同样不以配置端口为判据——内核可能绑在用户改端口之前的那个端口上。
///
/// 对 crate 内公开：托盘菜单的「退出」要用它判断是否需要先弹确认。
pub(crate) fn kernel_running(handle: &tauri::AppHandle) -> bool {
    let Some(state) = handle.try_state::<AppState>() else {
        return false;
    };
    {
        let mut guard = lock(&state.running);
        let alive = guard
            .as_mut()
            .map(|child| child.try_wait().is_ok_and(|status| status.is_none()))
            .unwrap_or(false);
        if alive {
            return true;
        }
        if guard.is_some() {
            // 句柄还在但进程已经退出（或查询失败）：清掉僵尸句柄，
            // 避免它继续影响状态判断。
            *guard = None;
        }
    }
    kernel::workbench_running(&state.data_dir, &settings::load(&state.data_dir))
}
