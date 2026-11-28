//! Native session children share the management window without sharing its IPC grants.

use std::sync::{mpsc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, Webview, WebviewBuilder,
    Window, WindowBuilder, WindowEvent,
};

use super::{harness_window, official_chat};
use crate::shell::{shell_events, window};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    Harness,
    McodeWorkbench,
    OfficialChat,
}

use Kind::{Harness, McodeWorkbench, OfficialChat};

impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Harness => "harness",
            McodeWorkbench => "mcode-workbench",
            OfficialChat => official_chat::OFFICIAL_CHAT_STRIP_LABEL,
        }
    }

    pub(crate) fn host_label(self) -> &'static str {
        match self {
            Harness => "harness-popout",
            McodeWorkbench => "mcode-workbench-popout",
            OfficialChat => official_chat::OFFICIAL_CHAT_WINDOW_LABEL,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
pub(crate) struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(|n| n.is_finite() && (0.0..=32768.0).contains(&n))
            && self.width >= 1.0
            && self.height >= 1.0
    }

    pub(crate) fn rect(self) -> Rect {
        Rect {
            position: LogicalPosition::new(self.x, self.y).into(),
            size: LogicalSize::new(self.width, self.height).into(),
        }
    }
}

static SLOT: Mutex<Option<(Kind, Bounds)>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Placement {
    kind: Kind,
    popped_out: bool,
}

pub(crate) fn main(app: &AppHandle) -> Result<Window, String> {
    app.get_window("main")
        .ok_or_else(|| "管理面板未打开，请重启应用".into())
}

pub(crate) fn add_hidden(
    host: &Window,
    builder: WebviewBuilder<tauri::Wry>,
) -> tauri::Result<Webview> {
    let view = host.add_child(
        builder,
        LogicalPosition::new(-2.0, 0.0),
        LogicalSize::new(1.0, 1.0),
    )?;
    let _ = view.hide();
    Ok(view)
}

pub(crate) fn detached(app: &AppHandle, kind: Kind) -> Option<Window> {
    let win = app.get_webview(kind.label())?.window();
    (win.label() == kind.host_label()).then_some(win)
}

/// 调用方在后台等待；原生迁移在一次主线程任务里完成，期间宿主不会处理关闭请求。
pub(crate) fn on_ui<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "会话显示任务中断；请查看会话窗口日志后重试".to_string())?
}

pub(crate) fn present(app: &AppHandle, kind: Kind) -> Result<(), String> {
    let popped_out = detached(app, kind);
    let result = if let Some(host) = &popped_out {
        host.show()
            .and_then(|()| host.unminimize())
            .and_then(|()| host.set_focus())
            .map_err(|e| e.to_string())
    } else {
        let restored = crate::shell::resident::restore_main_shell(app);
        let selected = app
            .emit_to(
                tauri::EventTarget::webview("main"),
                "session-surface-opened",
                kind,
            )
            .map_err(|e| e.to_string());
        restored.and(selected)
    };
    let placement = app
        .emit_to(
            tauri::EventTarget::webview("main"),
            "session-surface-placement",
            Placement {
                kind,
                popped_out: popped_out.is_some(),
            },
        )
        .map_err(|e| e.to_string());
    result.and(placement).map_err(|e| {
        let message = format!("无法显示会话：{e}；请从概览重试，详情见 session-surface 日志");
        shell_events::record("session-surface", &message);
        message
    })
}

fn views(app: &AppHandle, kind: Kind) -> Vec<Webview> {
    if kind == OfficialChat {
        app.webviews()
            .into_values()
            .filter(|view| {
                view.label() == kind.label() || view.label().starts_with("official-chat-tab-")
            })
            .collect()
    } else {
        app.get_webview(kind.label()).into_iter().collect()
    }
}

/// 把前端上报的 slot 贴到宿主内容区左上角，并按内容区右下边界裁掉溢出部分。
///
/// 抽成纯函数是为了让这段几何换算脱离 Tauri/AppKit 单独测试：它决定子视图是否
/// 可见（不足 1 像素就等于不可见），尺寸算错在真实窗口上很难复现。
fn clamp_to_layout(layout: Bounds, slot: Bounds) -> Option<Bounds> {
    let width = slot.width.min(layout.width - slot.x);
    let height = slot.height.min(layout.height - slot.y);
    (width >= 1.0 && height >= 1.0).then_some(Bounds {
        x: layout.x + slot.x,
        y: layout.y + slot.y,
        width,
        height,
    })
}

/// Runs on the UI thread, where the host has a stable AppKit content layout.
pub(crate) fn display_bounds(app: &AppHandle, kind: Kind) -> Option<Bounds> {
    let detached = detached(app, kind);
    let host = detached.clone().or_else(|| main(app).ok())?;
    let layout = content_area(&host)?;
    if detached.is_some() {
        return Some(layout);
    }

    let (_, slot) = crate::lock(&SLOT).filter(|(selected, _)| *selected == kind)?;
    clamp_to_layout(layout, slot)
}

#[cfg(not(target_os = "macos"))]
fn content_area(host: &Window) -> Option<Bounds> {
    let size = host
        .inner_size()
        .ok()?
        .to_logical::<f64>(host.scale_factor().ok()?);
    Some(Bounds {
        x: 0.0,
        y: 0.0,
        width: size.width,
        height: size.height,
    })
}

#[cfg(target_os = "macos")]
fn content_area(host: &Window) -> Option<Bounds> {
    use objc2::{msg_send, runtime::AnyObject, MainThreadMarker};
    use objc2_foundation::NSRect;

    let _main = MainThreadMarker::new()?;
    let window = host.ns_window().ok()?.cast::<AnyObject>();
    // SAFETY: the UI thread owns the AppKit objects; the host keeps NSWindow alive.
    // Wry sizes the child against the entire content view, including the title
    // bar on full-size-content windows. The DOM starts below the title bar.
    unsafe {
        let window = window.as_ref()?;
        let parent: *const AnyObject = msg_send![window, contentView];
        let parent = parent.as_ref()?;
        let frame: NSRect = msg_send![parent, frame];
        let layout: NSRect = msg_send![window, contentLayoutRect];
        let rect: NSRect =
            msg_send![parent, convertRect: layout, fromView: std::ptr::null::<AnyObject>()];
        let flipped: bool = msg_send![parent, isFlipped];
        Some(Bounds {
            x: rect.origin.x,
            y: if flipped {
                rect.origin.y
            } else {
                frame.size.height - rect.origin.y - rect.size.height
            },
            width: rect.size.width,
            height: rect.size.height,
        })
    }
}

pub(crate) fn host_focus_changed(app: &AppHandle, label: &str, focused: bool) {
    if label == "main"
        || [Harness, McodeWorkbench]
            .iter()
            .any(|kind| label == kind.host_label())
    {
        sync_workbench_focus(app, Some((label, focused)));
    }
}

fn sync_workbench_focus(app: &AppHandle, event: Option<(&str, bool)>) {
    let focused = |kind: Kind| {
        let Some(view) = app.get_webview(kind.label()) else {
            return false;
        };
        let host = view.window();
        let host_focused = match event {
            Some((label, focused)) if host.label() == label => focused,
            _ => host.is_focused().unwrap_or(false),
        };
        host_focused && host.is_visible().unwrap_or(false) && display_bounds(app, kind).is_some()
    };
    crate::notify::task::set_workbench_focused(app, focused(Harness));
    crate::notify::task::set_mcode_workbench_focused(app, focused(McodeWorkbench));
}

pub(crate) fn refresh(app: &AppHandle, kind: Kind) {
    if kind == OfficialChat {
        crate::commands::refresh_official_chat_surface(app);
    } else if let Some(view) = app.get_webview(kind.label()) {
        if let Some(bounds) = display_bounds(app, kind) {
            let _ = view.set_bounds(bounds.rect());
            let _ = view.show();
        } else {
            let _ = view.hide();
        }
    }
}

fn build_host(app: &AppHandle, kind: Kind) -> Result<Window, String> {
    let (title, width, height) = match kind {
        Harness => (window::workbench_title("工作台"), 1280.0, 840.0),
        McodeWorkbench => (window::workbench_title("MiniMax 工作台"), 1280.0, 840.0),
        OfficialChat => (
            window::window_title("官网网页版"),
            official_chat::OFFICIAL_CHAT_INITIAL_WIDTH,
            official_chat::OFFICIAL_CHAT_INITIAL_HEIGHT,
        ),
    };
    let builder = WindowBuilder::new(app, kind.host_label())
        .title(title)
        .theme(crate::shell::appearance::initial_theme(kind.label()))
        .inner_size(width, height)
        .background_color(window::CHROME_BACKDROP)
        .visible(false);
    #[cfg(target_os = "macos")]
    let builder = if kind == OfficialChat {
        builder.title_bar_style(tauri::TitleBarStyle::Transparent)
    } else {
        builder
    };
    let host = builder
        .build()
        .map_err(|e| format!("创建独立会话窗口失败：{e}；请查看会话窗口日志后重试"))?;
    let handle = app.clone();
    host.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = set_session_surface_popped_out(handle, kind, false).await {
                    shell_events::record("session-surface", &error);
                }
            });
        }
        WindowEvent::Resized(_)
        | WindowEvent::ScaleFactorChanged { .. }
        | WindowEvent::Focused(true)
            if detached(&handle, kind).is_some() =>
        {
            refresh(&handle, kind);
            if let Some(view) = handle
                .get_webview(kind.label())
                .filter(|_| kind != OfficialChat)
            {
                harness_window::sync_fullscreen_on_load(&view);
            }
        }
        _ => {}
    });
    Ok(host)
}

pub(crate) fn move_surface(app: &AppHandle, kind: Kind, popped_out: bool) -> Result<(), String> {
    if crate::shell::shutdown::is_shutting_down() {
        return Err("应用正在退出，请重启后再移动会话".into());
    }
    let primary = app
        .get_webview(kind.label())
        .ok_or("会话未打开，请先从概览打开")?;
    let old = primary.window();
    if (old.label() == kind.host_label()) == popped_out {
        return Ok(());
    }
    let host = if popped_out {
        build_host(app, kind)?
    } else {
        main(app)?
    };
    let children = views(app, kind);
    for child in &children {
        let _ = child.hide();
    }
    for child in &children {
        if let Err(error) = child.reparent(&host) {
            // Tauri 移动失败时不会恢复 runtime 登记；不能假装仍在旧窗口。清理
            // 整组和空宿主，允许用户重开，且绝不销毁管理面板。
            let _ = close(app, kind);
            for window in [&old, &host] {
                if window.label() != "main" {
                    let _ = window.destroy();
                }
            }
            let message =
                format!("移动会话失败：{error}；请从概览重新打开会话，详情见 session-surface 日志");
            shell_events::record("session-surface", &message);
            return Err(message);
        }
    }
    if kind != OfficialChat {
        harness_window::sync_fullscreen_on_load(&primary);
        if popped_out {
            // 文档不重载：主动重放已存在的标题和外观状态。
            let _ =
                primary.eval("window.dispatchEvent(new Event('dsh-xlink-surface-host-changed'));");
            harness_window::restore_document_title(&primary);
        }
    }
    if !popped_out {
        clear_slot(kind);
        let _ = old.destroy();
    }
    refresh(app, kind);
    let result = present(app, kind);
    if popped_out {
        refresh(app, kind);
    }
    sync_workbench_focus(app, None);
    result
}

fn clear_slot(kind: Kind) {
    let mut slot = crate::lock(&SLOT);
    if slot.is_some_and(|(selected, _)| selected == kind) {
        *slot = None;
    }
}

pub(crate) fn close(app: &AppHandle, kind: Kind) -> Result<(), String> {
    clear_slot(kind);
    let mut result = Ok(());
    let host = detached(app, kind);
    for view in views(app, kind) {
        if let Err(error) = view.close() {
            result = Err(error.to_string());
        }
    }
    if let Some(host) = host {
        if let Err(error) = host.destroy() {
            result = Err(error.to_string());
        }
    }
    let _ = app.emit_to(
        tauri::EventTarget::webview("main"),
        "session-surface-closed",
        kind,
    );
    match kind {
        Harness => crate::notify::task::set_workbench_focused(app, false),
        McodeWorkbench => crate::notify::task::set_mcode_workbench_focused(app, false),
        OfficialChat => {}
    }
    result
}

#[tauri::command]
pub async fn set_session_surface_slot(
    app: AppHandle,
    kind: Option<Kind>,
    bounds: Option<Bounds>,
) -> Result<(), String> {
    let slot = match (kind, bounds) {
        (None, None) => None,
        (Some(kind), Some(bounds)) if bounds.valid() => Some((kind, bounds)),
        _ => return Err("会话显示区域无效，请调整窗口大小后重试".into()),
    };
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        on_ui(&app, move || {
            *crate::lock(&SLOT) = slot;
            for kind in [Harness, McodeWorkbench, OfficialChat] {
                if detached(&handle, kind).is_none() {
                    refresh(&handle, kind);
                }
            }
            sync_workbench_focus(&handle, None);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_session_surface_popped_out(
    app: AppHandle,
    kind: Kind,
    popped_out: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = match kind {
            Harness => crate::lock(&harness_window::SURFACE_MUTATION),
            OfficialChat => crate::lock(crate::commands::official_chat_mutation_lock()),
            McodeWorkbench => crate::lock(crate::commands::mcode_window_lock()),
        };
        let handle = app.clone();
        on_ui(&app, move || move_surface(&handle, kind, popped_out))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内容区带原点偏移，才能验证结果用的是 layout 原点而不是 (0, 0)。
    fn layout() -> Bounds {
        Bounds {
            x: 100.0,
            y: 50.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn slot_that_fits_keeps_its_own_size() {
        let slot = Bounds {
            x: 40.0,
            y: 60.0,
            width: 320.0,
            height: 240.0,
        };
        let bounds = clamp_to_layout(layout(), slot).expect("放得下就该返回 Some");
        assert_eq!(bounds.x, 140.0, "x 必须是 layout 原点加 slot 偏移");
        assert_eq!(bounds.y, 110.0, "y 必须是 layout 原点加 slot 偏移");
        assert_eq!(bounds.width, 320.0, "未被裁剪时应保留 slot 自身宽度");
        assert_eq!(bounds.height, 240.0, "未被裁剪时应保留 slot 自身高度");
    }

    #[test]
    fn overflow_is_clipped_to_the_layout_edge() {
        // slot 右下角超出内容区，只剩 layout.width - slot.x / height - slot.y 可用。
        let slot = Bounds {
            x: 700.0,
            y: 500.0,
            width: 400.0,
            height: 300.0,
        };
        let bounds = clamp_to_layout(layout(), slot).expect("裁完仍有 100 像素可见");
        assert_eq!(bounds.width, 100.0);
        assert_eq!(bounds.height, 100.0);
        assert_eq!(bounds.x, 800.0);
        assert_eq!(bounds.y, 550.0);
    }

    #[test]
    fn sliver_below_one_pixel_is_invisible() {
        // slot.x 正好落在内容区右边界：剩余宽度为 0，小于 1 像素视为不可见。
        let slot = Bounds {
            x: 800.0,
            y: 0.0,
            width: 320.0,
            height: 240.0,
        };
        assert!(
            clamp_to_layout(layout(), slot).is_none(),
            "宽度不足 1 像素时子视图不可见，必须返回 None 让调用方隐藏"
        );
    }

    #[test]
    fn zero_sized_slot_is_rejected() {
        let zero_width = Bounds {
            x: 10.0,
            y: 10.0,
            width: 0.0,
            height: 240.0,
        };
        assert!(
            clamp_to_layout(layout(), zero_width).is_none(),
            "宽度为 0 的 slot 不该被当成可见"
        );
        let zero_height = Bounds {
            x: 10.0,
            y: 10.0,
            width: 320.0,
            height: 0.0,
        };
        assert!(
            clamp_to_layout(layout(), zero_height).is_none(),
            "高度为 0 的 slot 不该被当成可见"
        );
    }

    #[test]
    fn bounds_valid_accepts_only_finite_sane_values() {
        assert!(
            Bounds {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 840.0,
            }
            .valid(),
            "常规窗口尺寸必须通过校验"
        );
        assert!(
            !Bounds {
                x: f64::NAN,
                y: 0.0,
                width: 1280.0,
                height: 840.0,
            }
            .valid(),
            "NaN 参与比较永远为假，必须显式拒绝，否则几何换算会静默出错"
        );
        assert!(
            !Bounds {
                x: -1.0,
                y: 0.0,
                width: 1280.0,
                height: 840.0,
            }
            .valid(),
            "负坐标不是合法逻辑位置"
        );
        assert!(
            !Bounds {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 840.0,
            }
            .valid(),
            "宽度为 0 的窗口无意义"
        );
        assert!(
            !Bounds {
                x: 0.0,
                y: 0.0,
                width: 32769.0,
                height: 840.0,
            }
            .valid(),
            "超出 32768 的尺寸来自脏数据，必须挡在写 SLOT 之前"
        );
    }
}
