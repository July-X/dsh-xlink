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
mod patches;
mod plugins;
mod process;
mod quarantine;
mod registry;
mod releases;
mod settings;
mod skills;
mod updater;
mod version;

use std::sync::Mutex;

use commands::AppState;
use tauri::{Emitter, Manager, WindowEvent};

/// 锁定一个互斥锁；当另一个线程 panic 时取回内部值。
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 应用入口；由 `main.rs` 调用。
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // 管理面板在 macOS / Windows 使用本地 Vue 标题栏绘制交通灯
            // 和品牌背景。这个设置只影响 main，工作台与官方对话窗口
            // 继续沿用各自的窗口 chrome。
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(window) = app.get_webview_window("main") {
                if let Err(error) = window.set_decorations(false) {
                    // 这条设置同时服务 macOS（画交通灯）与 Windows（画品牌背景），
                    // 所以文案不能写死 macOS。失败只影响窗口外观，不影响任何功能，
                    // 因此只记录、不中断启动。
                    eprintln!(
                        "dsh-xlink: 无法为管理窗口关闭系统标题栏（set_decorations 失败）：{error}；\
                         窗口会退回系统原生标题栏，标题栏按钮与功能不受影响。\
                         若界面显示异常，重启应用重试；仍失败请用 `npm run dev` 在终端启动，\
                         连同上面的完整输出一起反馈。"
                    );
                }
            }

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
            crate::process::migrate_legacy_rotated_logs(&kernel::logs_dir(
                &app.state::<AppState>().data_dir,
            ));
            updater::spawn_background_check(app.handle());
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
    // 关闭管理窗口会在 `RunEvent::Exit` 之前触发 `WindowEvent::CloseRequested`。
    // 当内核仍在运行——或 official-chat 窗口仍打开——我们调用 `prevent_close()`
    // 并通知 UI 询问用户是否完全退出；UI 接着运行 `stop_kernel`（运行时）
    // 然后 `confirm_close_shell`，销毁所有窗口并退出事件循环。没有这一提示，
    // 用户可能关掉面板却留下占用端口的孤儿内核，下次启动会因为误导性的
    // 「端口已被占用」诊断而失败，直到下一次壳启动时才回收该孤儿。
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
                    kernel::kill_pid(pid, None);
                }
                kernel::clear_pid(&data_dir);
            }
        }
    });
}

/// 内核当前是否在对外服务。
///
/// 内存中的句柄只有在内核**确实还活着**时才算数：内核自行退出或被外部杀掉
/// 之后句柄仍留在槽位里，只看 `is_some()` 会让这里一直撒谎（关窗时弹出一个
/// 与实际状态矛盾的确认为）。句柄失效后回落到 [`kernel::workbench_running`]，
/// 它同样不以配置端口为判据——内核可能绑在用户改端口之前的那个端口上。
fn kernel_running(handle: &tauri::AppHandle) -> bool {
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
