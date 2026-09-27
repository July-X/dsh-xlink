// 独立窗口通用助手：吸附定位 + 移动跟随。
//
// 「吸附」指打开时把窗口贴在主窗右侧（主窗贴近屏幕右缘时自动翻左侧）、
// 与主窗顶边对齐（底部超出行程则上移夹回屏内），让两个窗口看起来像同一
// 个工作区。builder 阶段只能按逻辑目标尺寸落位；窗口建出后 [`snap_to_main`]
// 再以实测外框校正一次：外框高度对齐主壳（带装饰窗口的标题栏不再多出一截）、
// 补偿 Windows 不可见缩放边框把两窗贴紧。「移动跟随」指主窗被拖动时，副窗
// 通过主窗的 `on_window_event` 监听 `Moved` 事件按合帧窗口（~60fps）持续
// `set_position`，始终保持吸附；每轮拖动开始先把副窗提到最前（set_focus），
// 连动穿过其他应用窗口时不被压在后面。
//
// 这两条路径被日志查看器（`commands::open_log_window`）、模型用量窗口
// （`usage::open_usage_window`）与套餐用量窗口（`subscription.rs`）共用——
// 它们的差别只是窗口尺寸与 label，几何算法完全一致。共用的好处是 dock /
// 跟随的行为绝对不会「一个修一个忘」。

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// 跟随定位的合帧间隔：拖动时 `Moved` 触发频率远超显示刷新率，逐事件
/// 下发 `set_position` 会在主线程队列积压出迟滞感，压到 ~60fps 最顺滑。
const FRAME_INTERVAL: Duration = Duration::from_millis(15);
/// 拖动开始的判定：距上次跟随 tick 超过该安静期视为新一轮拖动，先把
/// 副窗提到最前再进入跟随。
const DRAG_START_QUIET: Duration = Duration::from_millis(400);

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

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
/// 套餐用量窗口（subscription.rs 弹出）。尺寸直接复用 USAGE_VIEWER_SIZE：
/// 两类窗口都是「主壳旁的只读数据面板」，没有理由长成两个尺寸。
pub const SUBSCRIPTION_VIEWER_LABEL: &str = "subscription-viewer";
pub const LOG_VIEWER_LABEL: &str = "log-viewer";
pub const LOG_VIEWER_SIZE: WindowSize = WindowSize {
    width: 960.0,
    height: 720.0,
};

/// 吸附 X（物理像素）：优先把窗口贴在主窗右侧；右侧放不下就翻到主窗左侧。
/// `inset_left` / `inset_right` 是副窗外框左右的**不可见边框**宽度（Windows
/// 带装饰窗口的 rect 左右各含约 7 逻辑像素的缩放边框，直接按外框对齐会在
/// 两窗之间留出一条可见缝隙）：右侧贴齐时副窗可见左缘落在主窗右缘，
/// 左侧贴齐时副窗可见右缘落在主窗左缘。
pub fn dock_x(
    main_x: i32,
    main_w: i32,
    win_w: i32,
    mon_x: i32,
    mon_w: i32,
    inset_left: i32,
    inset_right: i32,
) -> i32 {
    let monitor_right = mon_x.saturating_add(mon_w);
    let right = main_x.saturating_add(main_w);
    let preferred = if right.saturating_add(win_w) <= monitor_right {
        right.saturating_sub(inset_left)
    } else {
        main_x.saturating_sub(win_w.saturating_sub(inset_right))
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
/// 此时副窗尚未建出、量不到不可见边框，按 0 估计——建出后由
/// [`snap_to_main`] 以实测值校正。
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
        DockInsets::default(),
    ))
}

/// 副窗外框左右的不可见边框（物理像素）。`Default`（全 0）即「按外框对齐」。
#[derive(Clone, Copy, Default)]
pub struct DockInsets {
    left: i32,
    right: i32,
}

fn compute_dock_position_physical(
    main_position: PhysicalPosition<i32>,
    main_size: tauri::PhysicalSize<u32>,
    window_width: i32,
    window_height: i32,
    monitor_position: PhysicalPosition<i32>,
    monitor_size: tauri::PhysicalSize<u32>,
    insets: DockInsets,
) -> (i32, i32) {
    let x = dock_x(
        main_position.x,
        main_size.width as i32,
        window_width,
        monitor_position.x,
        monitor_size.width as i32,
        insets.left,
        insets.right,
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
    main_position: PhysicalPosition<i32>,
    target_size: tauri::PhysicalSize<u32>,
    insets: DockInsets,
) -> Option<(i32, i32)> {
    let main_size = main.outer_size().ok()?;
    let monitor = main.current_monitor().ok().flatten()?;
    Some(compute_dock_position_physical(
        main_position,
        main_size,
        target_size.width as i32,
        target_size.height as i32,
        *monitor.position(),
        *monitor.size(),
        insets,
    ))
}

/// 量出副窗外框左右的不可见边框（物理像素，见 [`dock_x`]）。`inner_position`
/// 与 `outer_position` 的差是窗口 rect 到客户区的偏移，外框宽减内框宽再扣掉
/// 左偏移即右边框。macOS 带装饰窗口横向内外重合，量出本来就是 0；任何一项
/// 取不到都按全 0 处理（退化为按外框对齐）。
pub fn dock_insets(win: &WebviewWindow) -> DockInsets {
    let (Ok(outer_pos), Ok(inner_pos), Ok(outer), Ok(inner)) = (
        win.outer_position(),
        win.inner_position(),
        win.outer_size(),
        win.inner_size(),
    ) else {
        return DockInsets::default();
    };
    let left = (inner_pos.x - outer_pos.x).max(0);
    let right = (outer.width as i32 - inner.width as i32 - left).max(0);
    DockInsets { left, right }
}

/// 与主窗外框高度对齐所需的副窗内框高度（物理像素）。`chrome_height` 是副窗
/// 自身的装饰高度（外框 − 内框，Windows 上为标题栏 + 边框）。装饰吃掉空间
/// 后装不下 `min_inner` 就返回 None，调用方保持原尺寸不动。
fn inner_height_matched_to(main_height: i32, chrome_height: i32, min_inner: i32) -> Option<i32> {
    let desired = main_height.checked_sub(chrome_height)?;
    (desired >= min_inner).then_some(desired)
}

/// 建窗后的吸附校正（三类查看器窗口共用）：builder 阶段只有逻辑目标尺寸，
/// 带装饰窗口的真实外框（标题栏 + 不可见缩放边框）要等窗口建出来才能量到。
/// 这里以实测为准做两件事：
/// 1. **高度对齐**：把副窗外框高度调到与主窗外框一致——`inner_size(…, 800)`
///    建出的窗口外框比 800 高出整个标题栏，视觉上就比主壳长出一截（Windows
///    上尤其明显）；
/// 2. **贴紧**：按 [`dock_insets`] 补偿不可见边框后重新落位（右侧放不下翻
///    左侧，均夹在主窗所在屏幕内）。
///
/// 主窗不在（理论不会）或量不到尺寸时保持 builder 落下的位置不动。
pub fn snap_to_main(handle: &AppHandle, label: &'static str) {
    let (Some(main), Some(win)) = (
        handle.get_webview_window("main"),
        handle.get_webview_window(label),
    ) else {
        return;
    };
    let Ok(main_pos) = main.outer_position() else {
        return;
    };
    let Ok(main_size) = main.outer_size() else {
        return;
    };
    let Ok(win_outer) = win.outer_size() else {
        return;
    };
    let Ok(win_inner) = win.inner_size() else {
        return;
    };
    // 1) 外框高度与主窗对齐（宽度不动：可缩放窗口的默认宽度已定，拉伸交给
    //    用户）。先定尺寸再落位，落位按“目标外框高度 == 主窗外框高度”计算，
    //    不依赖尺寸调整是否已即时生效。
    let chrome = win_outer.height.saturating_sub(win_inner.height) as i32;
    if let Some(inner_h) = inner_height_matched_to(main_size.height as i32, chrome, 200) {
        let scale = win.scale_factor().unwrap_or(1.0);
        let _ = win.set_size(LogicalSize::new(
            win_inner.width as f64 / scale,
            inner_h as f64 / scale,
        ));
    }
    // 2) 补偿不可见边框贴紧主窗。
    let insets = dock_insets(&win);
    let Some((x, y)) = compute_dock_position_for_target(
        &main,
        main_pos,
        tauri::PhysicalSize::new(win_outer.width, main_size.height),
        insets,
    ) else {
        return;
    };
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// 返回物理像素，WebviewWindowBuilder 的 `position` 要的是逻辑像素。
pub fn dock_position_logical(main: &WebviewWindow, size: WindowSize) -> Option<(f64, f64)> {
    let scale = main.scale_factor().ok()?;
    let (x, y) = compute_dock_position(main, size)?;
    Some((x as f64 / scale, y as f64 / scale))
}

/// 移动跟随监听器：主窗被拖动时，副窗（`target_label`）按 [`compute_dock_position`]
/// 重算并 `set_position`，保持吸附状态。主窗的 `Moved` 回调只记录最新物理坐标，
/// 实际窗口查询与定位在独立跟随线程中按 [`FRAME_INTERVAL`] 合帧执行，避免原生
/// 窗口调用阻塞主窗拖动事件。未开的副窗不会触发定位，打开后可直接接收后续位置。
///
/// 副窗自身的移动不经过这条路径（事件源是 `main`），不存在两窗互相拉扯
/// 的回环；主窗固定不可缩放，`Resized` 也不需要处理。
pub fn attach_dock_listener(app: &AppHandle, target_label: &'static str) {
    let Some(main) = app.get_webview_window("main") else {
        return;
    };

    // 只保留最新位置：拖动事件的生产速度可能高于窗口系统的处理速度，
    // 丢弃过时位置比排队 set_position 更顺滑，也不会让副窗越跟越远。
    let pending = Arc::new((Mutex::new(None::<PhysicalPosition<i32>>), Condvar::new()));
    let worker_pending = Arc::clone(&pending);
    let worker_handle = app.clone();
    let main_for_worker = main.clone();
    let worker_label = target_label;
    let _ = std::thread::Builder::new()
        .name(format!("dsh-dock-{worker_label}"))
        .spawn(move || {
            let mut last_apply: Option<Instant> = None;
            loop {
                let main_position = {
                    let (position_lock, wake) = &*worker_pending;
                    let mut latest = position_lock
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    loop {
                        if let Some(last) = last_apply {
                            let remaining = FRAME_INTERVAL.saturating_sub(last.elapsed());
                            if !remaining.is_zero() {
                                let (next, timeout) = wake
                                    .wait_timeout(latest, remaining)
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                latest = next;
                                if !timeout.timed_out() {
                                    // 有更新位置到达，继续等待本帧截止时间，
                                    // 最终只取最新值。
                                    continue;
                                }
                            }
                        }
                        if let Some(position) = latest.take() {
                            break position;
                        }
                        latest = wake
                            .wait(latest)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                };

                let Some(target) = worker_handle.get_webview_window(worker_label) else {
                    continue;
                };
                let now = Instant::now();
                let drag_start = last_apply
                    .map(|last| now.duration_since(last) >= DRAG_START_QUIET)
                    .unwrap_or(true);
                if drag_start {
                    // 每轮拖动只提到最前一次，避免两窗穿过其他应用时发生穿插。
                    let _ = target.set_focus();
                }
                let Ok(target_size) = target.outer_size() else {
                    continue;
                };
                // 与 snap_to_main 同一套补偿：拖动跟随时也按不可见边框贴紧，
                // 否则开窗时贴住的缝隙会在第一次拖动后重新出现。
                let insets = dock_insets(&target);
                let Some((x, y)) = compute_dock_position_for_target(
                    &main_for_worker,
                    main_position,
                    target_size,
                    insets,
                ) else {
                    continue;
                };
                last_apply = Some(now);
                let _ = target.set_position(PhysicalPosition::new(x, y));
            }
        });

    let pending_for_handler = Arc::clone(&pending);
    main.on_window_event(move |event| {
        let WindowEvent::Moved(position) = event else {
            return;
        };
        let (position_lock, wake) = &*pending_for_handler;
        *position_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(*position);
        wake.notify_one();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 吸附 X：主窗在屏幕中部时贴主窗右侧；主窗贴近右缘时翻到主窗左侧。
    #[test]
    fn dock_x_prefers_right_side_and_flips_left_when_offscreen() {
        // 无不可见边框（macOS 内外框重合）时的基线。
        // 主窗在屏幕中部：吸附到主窗右缘。
        assert_eq!(dock_x(100, 480, 760, 0, 1920, 0, 0), 580);
        // 主窗贴屏幕右缘（1440+480 = 1920）：右侧放不下 → 贴到主窗左侧。
        assert_eq!(dock_x(1440, 480, 760, 0, 1920, 0, 0), 680);
        // 副屏在左侧（负坐标）同样成立：贴主窗左侧 = main_x − 窗宽。
        assert_eq!(dock_x(-1000, 480, 760, -1920, 1920, 0, 0), -1760);
    }

    /// 不可见边框补偿（Windows 带装饰窗口）：右侧贴齐时把可见左缘压到主窗
    /// 右缘（x = 右缘 − 左边框）；翻左侧时把可见右缘压到主窗左缘
    /// （x = 主窗 x − 窗宽 + 右边框）。
    #[test]
    fn dock_x_compensates_invisible_borders() {
        // 右侧贴齐：可见左缘 = 572 + 8 = 580 = 主窗右缘。
        assert_eq!(dock_x(100, 480, 760, 0, 1920, 8, 8), 572);
        // 主窗贴屏幕右缘：翻左侧，可见右缘 = 688 + 760 − 8 = 1440 = 主窗 x。
        assert_eq!(dock_x(1440, 480, 760, 0, 1920, 8, 8), 688);
        // DPI 1.5 下边框更宽：7 × 1.5 ≈ 10，补偿量同样生效。
        assert_eq!(dock_x(100, 480, 760, 0, 1920, 11, 11), 569);
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

    /// 外框高度对齐：内框 = 主窗外框高度 − 副窗装饰高度；装饰吃掉空间后
    /// 装不下最小内框时返回 None（保持原尺寸，避免负内框 / 过分压缩）。
    #[test]
    fn inner_height_matched_to_subtracts_chrome_and_gives_up_when_too_small() {
        // Windows：主壳外框 800 逻辑像素，副窗标题栏 + 边框吃掉 32 物理像素。
        assert_eq!(inner_height_matched_to(800, 32, 200), Some(768));
        // 恰好用尽：内框为 0 也不该给（远小于最小内框）。
        assert_eq!(inner_height_matched_to(32, 32, 200), None);
        // 装饰比主窗还高（不可能出现，防御 checked_sub 下溢）。
        assert_eq!(inner_height_matched_to(100, 120, 200), None);
    }

    #[test]
    fn dock_position_clamps_oversized_windows_to_monitor_origin() {
        assert_eq!(dock_x(100, 480, 3_000, 0, 1_920, 0, 0), 0);
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
        assert_eq!(SUBSCRIPTION_VIEWER_LABEL, "subscription-viewer");
    }
}
