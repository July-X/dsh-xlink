// 独立窗口通用助手：吸附定位 + 移动跟随。
//
// 「吸附」指打开时把窗口贴在主窗右侧（主窗贴近屏幕右缘时自动翻左侧）、
// 与主窗顶边对齐（底部超出行程则上移夹回屏内），让两个窗口看起来像同一
// 个工作区。「移动跟随」指主窗被拖动时，副窗通过主窗的 `on_window_event`
// 监听 `Moved` 事件按合帧窗口（~60fps）持续 `set_position`，始终保持吸附；
// 每轮拖动开始先把副窗提到最前（set_focus），连动穿过其他应用窗口时不被压在后面。
//
// 这两条路径被日志查看器（`commands::open_log_window`）和模型用量窗口
// （`usage::open_usage_window`）共用——它们的差别只是窗口尺寸与 label，
// 几何算法完全一致。共用的好处是 dock / 跟随的行为绝对不会「一个修一个忘」。

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 跟随定位的合帧间隔：拖动时 `Moved` 触发频率远超显示刷新率，逐事件
/// 下发 `set_position` 会在主线程队列积压出迟滞感，压到 ~60fps 最顺滑。
const FRAME_INTERVAL: Duration = Duration::from_millis(15);
/// 拖动开始的判定：距上次跟随 tick 超过该安静期视为新一轮拖动，先把
/// 副窗提到最前再进入跟随。
const DRAG_START_QUIET: Duration = Duration::from_millis(400);

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

pub const USAGE_VIEWER_LABEL: &str = "usage-viewer";
pub const USAGE_VIEWER_SIZE: WindowSize = WindowSize {
    width: 760.0,
    height: 800.0,
};
pub const LOG_VIEWER_LABEL: &str = "log-viewer";
pub const LOG_VIEWER_SIZE: WindowSize = WindowSize {
    width: 960.0,
    height: 720.0,
};

/// 吸附 X（物理像素）：优先把窗口贴在主窗右侧；右侧放不下就翻到主窗左侧。
pub fn dock_x(main_x: i32, main_w: i32, win_w: i32, mon_x: i32, mon_w: i32) -> i32 {
    let monitor_right = mon_x.saturating_add(mon_w);
    let right = main_x.saturating_add(main_w);
    let preferred = if right.saturating_add(win_w) <= monitor_right {
        right
    } else {
        main_x.saturating_sub(win_w)
    };
    clamp_position(preferred, mon_x, mon_w, win_w)
}

/// 吸附 Y（物理像素）：与主窗顶对齐；底部超出行程就上移、夹在屏内。
pub fn dock_y(main_y: i32, win_h: i32, mon_y: i32, mon_h: i32) -> i32 {
    let preferred = if main_y.saturating_add(win_h) > mon_y.saturating_add(mon_h) {
        mon_y.saturating_add(mon_h).saturating_sub(win_h)
    } else {
        main_y
    };
    clamp_position(preferred, mon_y, mon_h, win_h)
}

fn clamp_position(position: i32, monitor_start: i32, monitor_size: i32, window_size: i32) -> i32 {
    let monitor_end = monitor_start.saturating_add(monitor_size);
    let max_position = monitor_end.saturating_sub(window_size);
    if max_position < monitor_start {
        monitor_start
    } else {
        position.clamp(monitor_start, max_position)
    }
}

/// 计算指定尺寸下的吸附位置（物理像素）。主窗不在（理论上不会）或取不
/// 到显示器信息时返回 `None`，调用方据此决定是否落到默认居中。
pub fn compute_dock_position(main: &WebviewWindow, window_size: WindowSize) -> Option<(i32, i32)> {
    let scale = main.scale_factor().ok()?;
    let pos = main.outer_position().ok()?;
    let main_size = main.outer_size().ok()?;
    let monitor = main.current_monitor().ok().flatten()?;
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let width_phys = (window_size.width * scale) as i32;
    let height_phys = (window_size.height * scale) as i32;
    Some(compute_dock_position_physical(
        pos,
        main_size,
        width_phys,
        height_phys,
        *m_pos,
        *m_size,
    ))
}

fn compute_dock_position_physical(
    main_position: PhysicalPosition<i32>,
    main_size: tauri::PhysicalSize<u32>,
    window_width: i32,
    window_height: i32,
    monitor_position: PhysicalPosition<i32>,
    monitor_size: tauri::PhysicalSize<u32>,
) -> (i32, i32) {
    let x = dock_x(
        main_position.x,
        main_size.width as i32,
        window_width,
        monitor_position.x,
        monitor_size.width as i32,
    );
    let y = dock_y(
        main_position.y,
        window_height,
        monitor_position.y,
        monitor_size.height as i32,
    );
    (x, y)
}

fn compute_dock_position_for_target(
    main: &WebviewWindow,
    target_size: tauri::PhysicalSize<u32>,
) -> Option<(i32, i32)> {
    let main_position = main.outer_position().ok()?;
    let main_size = main.outer_size().ok()?;
    let monitor = main.current_monitor().ok().flatten()?;
    Some(compute_dock_position_physical(
        main_position,
        main_size,
        target_size.width as i32,
        target_size.height as i32,
        *monitor.position(),
        *monitor.size(),
    ))
}

/// 返回物理像素，WebviewWindowBuilder 的 `position` 要的是逻辑像素。
pub fn dock_position_logical(main: &WebviewWindow, size: WindowSize) -> Option<(f64, f64)> {
    let scale = main.scale_factor().ok()?;
    let (x, y) = compute_dock_position(main, size)?;
    Some((x as f64 / scale, y as f64 / scale))
}

/// 移动跟随监听器：主窗被拖动时，副窗（`target_label`）按 [`compute_dock_position`]
/// 重算并 `set_position`，保持吸附状态。`on_window_event` 挂在主窗上
/// （`Moved` 在拖动全程连续触发），所以开不开副窗都能安全挂载——本函数
/// 内部用 `get_webview_window(target_label)` 短路未开窗的情况。
///
/// 副窗自身的移动不经过这条路径（事件源是 `main`），不存在两窗互相拉扯
/// 的回环；主窗固定不可缩放，`Resized` 也不需要处理。
pub fn attach_dock_listener(app: &AppHandle, target_label: &'static str) {
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    let handle = app.clone();
    let main_for_handler = main.clone();
    // 合帧节流：Moved 触发频率远超显示刷新率，把 set_position 压到 ~60fps，
    // 中间位置直接丢弃（后续事件总会带上最新值）。逐事件下发会让指令
    // 在主线程队列里积压，跟随看起来迟滞、卡顿。
    let last_apply: Mutex<Option<Instant>> = Mutex::new(None);
    main.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Moved(_)) {
            return;
        }
        let Some(target) = handle.get_webview_window(target_label) else {
            return;
        };
        let now = Instant::now();
        let elapsed = match last_apply
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            Some(t) => now.duration_since(*t),
            None => Duration::MAX,
        };
        // 拖动开始的第一次 Moved（距上次 tick 超过安静期）：先把副窗提到
        // 最前（set_focus），否则两窗连动穿过其他应用窗口时副窗会被压在
        // 人家后面，出现"一半在别窗前后"的穿插。tao 没有"提到最前但不抢
        // 焦点"的 API（macOS 的 set_visible 也是 makeKeyAndOrderFront），
        // 所以每个拖动回合只做一次，焦点落在副窗上，点任意窗口即可收回。
        let drag_start = elapsed >= DRAG_START_QUIET;
        if drag_start {
            let _ = target.set_focus();
        }
        // 合帧节流：Moved 触发频率远超显示刷新率，把 set_position 压到
        // ~60fps，中间位置直接丢弃（后续事件总会带上最新值）。逐事件下发
        // 会让指令在主线程队列里积压，跟随看起来迟滞、卡顿。
        if elapsed < FRAME_INTERVAL && !drag_start {
            return;
        }
        *last_apply
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(now);
        let Ok(target_size) = target.outer_size() else {
            return;
        };
        let Some((x, y)) = compute_dock_position_for_target(&main_for_handler, target_size) else {
            return;
        };
        let _ = target.set_position(PhysicalPosition::new(x, y));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 吸附 X：主窗在屏幕中部时贴主窗右侧；主窗贴近右缘时翻到主窗左侧。
    #[test]
    fn dock_x_prefers_right_side_and_flips_left_when_offscreen() {
        // 主窗在屏幕中部：吸附到主窗右缘。
        assert_eq!(dock_x(100, 480, 760, 0, 1920), 580);
        // 主窗贴屏幕右缘（1440+480 = 1920）：右侧放不下 → 贴到主窗左侧。
        assert_eq!(dock_x(1440, 480, 760, 0, 1920), 680);
        // 副屏在左侧（负坐标）同样成立：贴主窗左侧 = main_x − 窗宽。
        assert_eq!(dock_x(-1000, 480, 760, -1920, 1920), -1760);
    }

    /// 吸附 Y：与主窗顶对齐；底部超出行程则上移夹回屏内。
    #[test]
    fn dock_y_aligns_top_and_clamps_inside_monitor() {
        // 常规：与主窗顶对齐。
        assert_eq!(dock_y(100, 800, 0, 1000), 100);
        // 主窗偏下、800 高放不下：上移到屏幕底缘。
        assert_eq!(dock_y(300, 800, 0, 1000), 200);
        // 恰好放得下（底边贴齐屏幕底缘）：原样对齐。
        assert_eq!(dock_y(-200, 800, -400, 1000), -200);
    }

    #[test]
    fn dock_position_clamps_oversized_windows_to_monitor_origin() {
        assert_eq!(dock_x(100, 480, 3_000, 0, 1_920), 0);
        assert_eq!(dock_y(100, 2_000, 0, 1_080), 0);
    }

    #[test]
    fn registered_window_specs_keep_labels_and_default_sizes_together() {
        assert_eq!(USAGE_VIEWER_LABEL, "usage-viewer");
        assert_eq!(
            USAGE_VIEWER_SIZE,
            WindowSize {
                width: 760.0,
                height: 800.0
            }
        );
        assert_eq!(LOG_VIEWER_LABEL, "log-viewer");
        assert_eq!(
            LOG_VIEWER_SIZE,
            WindowSize {
                width: 960.0,
                height: 720.0
            }
        );
    }
}
