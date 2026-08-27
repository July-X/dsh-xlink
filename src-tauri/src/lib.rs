//! dsh-desktop: a Tauri shell around the DeepSeek Harness kernel.
//!
//! The shell manages pinned kernel versions (installed via pnpm from the
//! official `dsh-v*` releases), runs the active kernel's `dsh web` server,
//! and opens its UI in a dedicated webview window. All management happens in
//! the local `ui/` frontend through the commands in [`commands`].

mod commands;
mod env;
mod error;
mod guard;
mod kernel;
mod node;
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

/// Lock a mutex, recovering the inner value when another thread panicked.
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Application entry; invoked from `main.rs`.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = kernel::data_dir(app.handle());
            // Stamp the resolved data dir on stderr so a developer running
            // `tauri dev` alongside the installed release shell can see at
            // a glance which of the two (release → `~/.dsh/desktop/`, debug
            // → `~/.dsh/desktop-dev/`) actually owns this process. Cheap
            // insurance against the classic "I edited settings in the dev
            // shell and the release shell doesn't see it" foot-gun.
            eprintln!(
                "dsh-desktop: data_dir = {} (build: {})",
                data_dir.display(),
                if cfg!(debug_assertions) {
                    "dev"
                } else {
                    "release"
                }
            );
            // Reap orphaned dsh web kernels belonging to this data dir
            // BEFORE anything manages state: a crashed/killed shell leaves
            // its kernel running with cwd == data_dir, and two kernels on
            // the same project dir append to the same session log, which
            // corrupts it (seq gap). Must run before start_kernel can
            // observe "port already open" and report the orphan as a
            // healthy instance.
            kernel::reap_orphans(&data_dir);
            app.manage(AppState {
                data_dir,
                running: Mutex::new(None),
                node_cache: Mutex::new(None),
            });
            // Crash-recover any plugin-store staging dirs left behind by
            // an earlier shell run that died mid-update. The recovery is
            // a single read_dir scan on the happy path (no leftovers =
            // nothing to do) so it is safe to run unconditionally here
            // rather than gating on a marker file. Must run before any
            // plugin command touches the store, which it does not yet at
            // setup time.
            plugins::reconcile_store(&app.state::<AppState>().data_dir);
            // Same class of startup repair for the skill store: recover
            // staging swaps, re-link missing active-root entries, sweep
            // orphaned store links. Pure filesystem work; failures land in
            // the skill store's warning field for the UI.
            skills::reconcile();
            updater::spawn_background_check(app.handle());
            // Auto-open DevTools on the management window in debug builds.
            // Tauri's webview keyboard shortcuts (`Cmd+Option+I`,
            // `Cmd+Shift+I`, F12) do not always reach WKWebView on macOS,
            // so the inspection surface has to be opened from the
            // embedding side. `setup` fires after the configured windows
            // are created, so the `main` webview is already retrievable
            // here; the gated `#[cfg(debug_assertions)]` keeps release
            // builds (which ship `with_devtools(false)` anyway) free of
            // the call.
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::detect_node,
            commands::save_settings,
            commands::get_kernel_log,
            commands::list_log_files,
            commands::read_log_file,
            commands::open_data_dir,
            commands::check_shell_update,
            commands::install_shell_update,
            commands::fetch_releases,
            commands::install_kernel,
            commands::activate_version,
            commands::remove_version,
            commands::start_kernel,
            commands::stop_kernel,
            commands::open_harness,
            commands::open_log_window,
            commands::open_official_chat,
            commands::close_official_chat,
            commands::official_chat_tabs,
            commands::switch_official_chat_tab,
            commands::focus_main_shell,
            commands::plugin_status,
            commands::plugin_install,
            commands::plugin_update,
            commands::plugin_uninstall,
            commands::plugin_sync,
            commands::plugin_set_mode,
            commands::plugin_check_updates,
            commands::plugin_catalog,
            commands::plugin_resolve,
            commands::skill_status,
            commands::skill_install,
            commands::skill_update,
            commands::skill_uninstall,
            commands::skill_set_enabled,
            commands::skill_check_updates,
            commands::confirm_close_shell,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the dsh-desktop app");

    // Reap the kernel when the shell exits so no dsh web process is left
    // serving after the app quits. The in-memory child covers kernels this
    // session spawned; the pid file covers orphans left by an earlier shell
    // run (e.g. after a crash), guarded by `kill_pid`'s kernel check.
    //
    // Closing the management window fires `WindowEvent::CloseRequested` *before*
    // `RunEvent::Exit`. When the kernel is still running — or the official-chat
    // window is open — we `prevent_close()` and ping the UI to ask the user
    // whether to fully quit; the UI then runs `stop_kernel` (when running)
    // followed by `confirm_close_shell`, which destroys every window and exits
    // the event loop. Without this prompt the user could close the panel,
    // leave an orphan kernel holding the port, and a later start would fail
    // with the misleading "port already open" diagnostic until the orphan is
    // reaped on the next shell launch.
    //
    // The prompt path cannot rely on `RunEvent::Exit` for window teardown:
    // `confirm_close_shell` destroys the main window, but the event loop only
    // terminates once the LAST window is gone (and on macOS not even then —
    // it needs an explicit exit), so a still-open `official-chat` window would
    // keep the loop — and the app — alive with nobody left to close it. The
    // Exit branch below therefore is only the fallback for exits that bypass
    // the prompt (Cmd+Q, OS shutdown, last-window auto-close on Windows/Linux
    // when nothing needed a warning).
    app.run(|handle, event| {
        if let tauri::RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } = &event
        {
            // Only intercept the management window's X button; the harness
            // workbench webview (label "harness") is allowed to close
            // without confirmation since it has no kernel handle of its own.
            let official_chat_open = handle.get_window("official-chat").is_some();
            if label == "main" && (kernel_running(handle) || official_chat_open) {
                // Kernel running or official chat open: ask the user before
                // tearing the shell down — a confirmed quit closes both.
                // prevent_close() parks the close; the UI either confirms
                // (stops the kernel when running, then calls
                // confirm_close_shell) or cancels, leaving every window
                // exactly as it was. Nothing is destroyed here so a cancel
                // cannot take the official-chat window down with it.
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
            // Not the main window, or neither the kernel nor the official
            // chat needs a warning: let the close proceed. The Exit branch
            // below cascade-closes the official-chat webview on every real
            // exit and reaps any pid-file leftover from an earlier crash.
        }
        if let tauri::RunEvent::Exit = event {
            // Cascade-close the official-chat panel-driven window on exits
            // that bypassed the quit prompt (Cmd+Q on macOS, OS shutdown,
            // last-window auto-close when nothing needed a warning). The
            // confirmed-quit path already destroyed every window in
            // `confirm_close_shell` before exiting the loop — Exit is the
            // fallback net here, not the primary teardown. Same rationale
            // as the WindowEvent handler above: official-chat is a
            // transient panel-driven window and closing the panel implies
            // closing it.
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
                if !kernel::port_open(settings::load(&data_dir).port) {
                    // Port free: either nothing runs or stop() above reaped
                    // it — drop a stale pid record so the next start is clean.
                    kernel::clear_pid(&data_dir);
                } else if let Some(pid) = kernel::read_pid(&data_dir) {
                    kernel::kill_pid(pid);
                    kernel::clear_pid(&data_dir);
                }
            }
        }
    });
}

/// Whether the kernel is currently serving traffic. Either the in-memory
/// child handle is alive or the configured port answers — the latter catches
/// kernels an earlier shell spawned whose pid we still need to reap.
fn kernel_running(handle: &tauri::AppHandle) -> bool {
    let Some(state) = handle.try_state::<AppState>() else {
        return false;
    };
    if lock(&state.running).is_some() {
        return true;
    }
    kernel::port_open(settings::load(&state.data_dir).port)
}
