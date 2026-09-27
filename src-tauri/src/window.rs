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

/// 一扇窗口外框相对**可见内容**的四边不可见边框（物理像素）。
/// Windows 上两类窗口都有：带装饰窗口的 rect 左右各含约 7 逻辑像素的缩放
/// 边框 + 底部同宽、顶部标题栏；tao 对「无装饰 + 阴影」窗口（主壳就是这种）
/// 也按同样规则保留不可见缩放边框（tao `calculate_insets_for_dpi`，GPUI 同
/// 款实现）。可见内容的左缘因此在外框 rect 内缩 `left` 处，而不是外框左缘。
/// `Default`（全 0）即「外框 == 可见内容」（macOS 两种窗口横向重合，量出
/// 本来就基本为 0）。
#[derive(Clone, Copy, Default)]
pub struct DockInsets {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
}

impl DockInsets {
    fn zero() -> Self {
        Self::default()
    }
}

/// 吸附 X（物理像素）：优先把副窗贴在主窗**可见内容**右侧；右侧放不下就翻
/// 到主窗可见内容左侧。两侧都按各自的不可见边框补偿——主壳（无装饰+阴影）
/// 与副窗（带装饰）的 rect 里都有不可见缩放边框，只补任何一侧都会在两窗
/// 之间留出一条可见缝隙（Windows 真机两轮反馈的正是这条缝）。
pub fn dock_x(
    main_x: i32,
    main_w: i32,
    win_w: i32,
    mon_x: i32,
    mon_w: i32,
    main_insets: DockInsets,
    win_insets: DockInsets,
) -> i32 {
    let monitor_right = mon_x.saturating_add(mon_w);
    let main_visible_right = main_x
        .saturating_add(main_w)
        .saturating_sub(main_insets.right);
    let preferred = if main_visible_right
        .saturating_sub(win_insets.left)
        .saturating_add(win_w)
        <= monitor_right
    {
        // 右侧贴齐：副窗可见左缘落在主窗可见右缘。
        main_visible_right.saturating_sub(win_insets.left)
    } else {
        // 翻左侧：副窗可见右缘落在主窗可见左缘。
        main_x
            .saturating_add(main_insets.left)
            .saturating_sub(win_w.saturating_sub(win_insets.right))
    };
    clamp_position(preferred, mon_x, mon_w, win_w)
}

/// 吸附 Y（物理像素）：与主窗**可见内容**顶对齐（主壳 rect 顶部在 Win11 上
/// 也有约 1 逻辑像素的不可见内边距）；底部超出行程就上移、夹在屏内。
pub fn dock_y(main_y: i32, win_h: i32, mon_y: i32, mon_h: i32, main_insets_top: i32) -> i32 {
    let preferred = main_y.saturating_add(main_insets_top);
    let preferred = if preferred.saturating_add(win_h) > mon_y.saturating_add(mon_h) {
        mon_y.saturating_add(mon_h).saturating_sub(win_h)
    } else {
        preferred
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

/// 量出一扇窗口外框四边的不可见边框（物理像素）。`inner_position` 与
/// `outer_position` 的差是窗口 rect 到客户区的偏移；外框宽/高减内框宽/高再
/// 扣掉左/上偏移即右/下边框。任何一项取不到都按全 0 处理（退化为按外框
/// 对齐）。主壳与副窗都要量——两边的不可见边框都得补偿（见 [`dock_x`]）。
fn window_insets(win: &WebviewWindow) -> DockInsets {
    let (Ok(outer_pos), Ok(inner_pos), Ok(outer), Ok(inner)) = (
        win.outer_position(),
        win.inner_position(),
        win.outer_size(),
        win.inner_size(),
    ) else {
        return DockInsets::zero();
    };
    let left = (inner_pos.x - outer_pos.x).max(0);
    let top = (inner_pos.y - outer_pos.y).max(0);
    let right = (outer.width as i32 - inner.width as i32 - left).max(0);
    let bottom = (outer.height as i32 - inner.height as i32 - top).max(0);
    DockInsets {
        left,
        right,
        top,
        bottom,
    }
}

/// 吸附定位的纯几何输入（物理像素）。收拢成结构体让纯函数 `dock_position_for`
/// 的参数数保持克制，也方便单测直接构造。
struct DockQuery {
    main_pos: (i32, i32),
    main_size: (i32, i32),
    main_insets: DockInsets,
    win_size: (i32, i32),
    win_insets: DockInsets,
    mon_pos: (i32, i32),
    mon_size: (i32, i32),
}

fn dock_position_for(q: DockQuery) -> (i32, i32) {
    let x = dock_x(
        q.main_pos.0,
        q.main_size.0,
        q.win_size.0,
        q.mon_pos.0,
        q.mon_size.0,
        q.main_insets,
        q.win_insets,
    );
    let y = dock_y(
        q.main_pos.1,
        q.win_size.1,
        q.mon_pos.1,
        q.mon_size.1,
        q.main_insets.top,
    );
    (x, y)
}

/// 计算指定尺寸下的吸附位置（物理像素）。主窗不在（理论上不会）或取不
/// 到显示器信息时返回 `None`，调用方据此决定是否落到默认居中。
/// 副窗尚未建出、量不到自己的不可见边框，按 0 估计——建出后由
/// [`snap_to_main`] 以实测值校正；主窗已存在，它的边框照实量。
pub fn compute_dock_position(main: &WebviewWindow, window_size: WindowSize) -> Option<(i32, i32)> {
    let scale = main.scale_factor().ok()?;
    let pos = main.outer_position().ok()?;
    let main_size = main.outer_size().ok()?;
    let monitor = main.current_monitor().ok().flatten()?;
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let width_phys = (window_size.width * scale) as i32;
    let height_phys = (window_size.height * scale) as i32;
    Some(dock_position_for(DockQuery {
        main_pos: (pos.x, pos.y),
        main_size: (main_size.width as i32, main_size.height as i32),
        main_insets: window_insets(main),
        win_size: (width_phys, height_phys),
        win_insets: DockInsets::zero(),
        mon_pos: (m_pos.x, m_pos.y),
        mon_size: (m_size.width as i32, m_size.height as i32),
    }))
}

/// 与主窗**可见内容**高度对齐所需的副窗内框高度（物理像素）。
/// 主窗可见高 = 主窗外框高 − 主窗上下不可见边框；副窗顶边框是标题栏本身
/// （可见装饰），因此内框 = 主窗可见高 − 副窗顶边框。装不下 `min_inner`
/// 就返回 None，调用方保持原尺寸不动。
fn inner_height_matched_to(
    main_height: i32,
    main_insets: DockInsets,
    win_insets: DockInsets,
    min_inner: i32,
) -> Option<i32> {
    let desired = main_height
        .checked_sub(main_insets.top)?
        .checked_sub(main_insets.bottom)?
        .checked_sub(win_insets.top)?;
    (desired >= min_inner).then_some(desired)
}

/// 建窗后的吸附校正（三类查看器窗口共用）：builder 阶段只有逻辑目标尺寸，
/// 两窗的真实外框（标题栏 + 不可见缩放边框）要等窗口建出来才能量到。
/// 这里以实测为准做两件事：
/// 1. **高度对齐**：把副窗的可见内容高度调到与主窗可见内容一致——
///    `inner_size(…, 800)` 建出的窗口可见高度比主壳短/长出一截标题栏
///    （Windows 上尤其明显）；
/// 2. **贴紧**：按两窗各自实测的不可见边框做可见缘贴齐后重新落位（右侧
///    放不下翻左侧，均夹在主窗所在屏幕内）。
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
    let main_insets = window_insets(&main);
    let win_insets = window_insets(&win);
    // 1) 可见内容高度与主窗对齐（宽度不动：可缩放窗口的默认宽度已定，拉伸
    //    交给用户）。先定尺寸再落位，落位按“目标外框高度 == 主窗外框高 −
    //    主窗上下边框 + 副窗下边框”计算，不依赖尺寸调整是否已即时生效。
    if let Some(inner_h) =
        inner_height_matched_to(main_size.height as i32, main_insets, win_insets, 200)
    {
        let scale = win.scale_factor().unwrap_or(1.0);
        let _ = win.set_size(LogicalSize::new(
            win_inner.width as f64 / scale,
            inner_h as f64 / scale,
        ));
    }
    // 2) 两窗可见缘贴齐。副窗目标外框高 = 主窗外框高 − 主窗上下不可见边框
    //    + 副窗底部不可见边框（可见底对齐时外框底要再多留出副窗的下边框）。
    let win_h = main_size.height as i32 - main_insets.top - main_insets.bottom + win_insets.bottom;
    let Some(monitor) = main.current_monitor().ok().flatten() else {
        return;
    };
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let (x, y) = dock_position_for(DockQuery {
        main_pos: (main_pos.x, main_pos.y),
        main_size: (main_size.width as i32, main_size.height as i32),
        main_insets,
        win_size: (win_outer.width as i32, win_h.max(1)),
        win_insets,
        mon_pos: (m_pos.x, m_pos.y),
        mon_size: (m_size.width as i32, m_size.height as i32),
    });
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
                // 与 snap_to_main 同一套补偿：拖动跟随时两窗都按实测不可见
                // 边框做可见缘贴齐，否则开窗时贴住的缝隙会在第一次拖动后
                // 重新出现（主窗跨屏拖动还会换 DPI，边框宽度随之变化，所以
                // 每 tick 都重新量）。
                let Ok(main_size) = main_for_worker.outer_size() else {
                    continue;
                };
                let Some(monitor) = main_for_worker.current_monitor().ok().flatten() else {
                    continue;
                };
                let m_pos = monitor.position();
                let m_size = monitor.size();
                let (x, y) = dock_position_for(DockQuery {
                    main_pos: (main_position.x, main_position.y),
                    main_size: (main_size.width as i32, main_size.height as i32),
                    main_insets: window_insets(&main_for_worker),
                    win_size: (target_size.width as i32, target_size.height as i32),
                    win_insets: window_insets(&target),
                    mon_pos: (m_pos.x, m_pos.y),
                    mon_size: (m_size.width as i32, m_size.height as i32),
                });
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

    fn insets(left: i32, right: i32, top: i32, bottom: i32) -> DockInsets {
        DockInsets {
            left,
            right,
            top,
            bottom,
        }
    }

    /// 吸附 X：主窗在屏幕中部时贴主窗右侧；主窗贴近右缘时翻到主窗左侧。
    /// 基线：两窗都无不可见边框（macOS 内外框重合）。
    #[test]
    fn dock_x_prefers_right_side_and_flips_left_when_offscreen() {
        let zero = DockInsets::zero();
        // 主窗在屏幕中部：吸附到主窗右缘。
        assert_eq!(dock_x(100, 480, 760, 0, 1920, zero, zero), 580);
        // 主窗贴屏幕右缘（1440+480 = 1920）：右侧放不下 → 贴到主窗左侧。
        assert_eq!(dock_x(1440, 480, 760, 0, 1920, zero, zero), 680);
        // 副屏在左侧（负坐标）同样成立：贴主窗左侧 = main_x − 窗宽。
        assert_eq!(dock_x(-1000, 480, 760, -1920, 1920, zero, zero), -1760);
    }

    /// 可见缘贴齐（Windows 两窗 rect 里都有不可见缩放边框）：右侧贴齐时
    /// 副窗可见左缘 = 主窗可见右缘；翻左侧时副窗可见右缘 = 主窗可见左缘。
    /// 只补副窗一侧会留下主窗自己的不可见边框宽度的缝（真机第二轮反馈）。
    #[test]
    fn dock_x_aligns_visible_frames_on_both_sides() {
        // 主壳（无装饰+阴影）与副窗（带装饰）各含 8px 不可见边框。
        let main = insets(8, 8, 0, 8);
        let win = insets(8, 8, 0, 8);
        // 右侧贴齐：x = 主窗可见右缘(580−8) − 副窗左边框(8) = 564，
        // 副窗可见左缘 = 564 + 8 = 572 = 主窗可见右缘。
        assert_eq!(dock_x(100, 480, 760, 0, 1920, main, win), 564);
        // 主窗贴屏幕右缘：翻左侧。x = 主窗可见左缘(1448) − 760 + 右边框(8)
        // = 696，副窗可见右缘 = 696 + 760 − 8 = 1448 = 主窗可见左缘。
        assert_eq!(dock_x(1440, 480, 760, 0, 1920, main, win), 696);
        // DPI 1.5 下边框更宽（7 × 1.5 ≈ 11）：可见缘同样精确重合。
        let wide = insets(11, 11, 0, 11);
        assert_eq!(dock_x(100, 480, 760, 0, 1920, wide, wide), 558);
    }

    /// 吸附 Y：与主窗可见内容顶对齐（main insets.top，Win11 上约 1px）；
    /// 底部超出行程则上移夹回屏内。
    #[test]
    fn dock_y_aligns_visible_top_and_clamps_inside_monitor() {
        // 常规：与主窗可见顶对齐（主窗顶部无边框时即外框顶）。
        assert_eq!(dock_y(100, 800, 0, 1000, 0), 100);
        // Win11 主壳顶部有 1px 不可见内边距：可见顶 = main_y + 1。
        assert_eq!(dock_y(100, 800, 0, 1000, 1), 101);
        // 主窗偏下、800 高放不下：上移到屏幕底缘。
        assert_eq!(dock_y(300, 800, 0, 1000, 0), 200);
        // 恰好放得下（底边贴齐屏幕底缘）：原样对齐。
        assert_eq!(dock_y(-200, 800, -400, 1000, 0), -200);
    }

    /// 可见内容高度对齐：副窗内框 = 主窗外框高 − 主窗上下不可见边框
    /// − 副窗顶边框（标题栏）；装不下最小内框时返回 None。
    #[test]
    fn inner_height_matched_to_aligns_visible_heights() {
        // Windows：主壳外框 1000、上下各 8 不可见边框；副窗顶部标题栏 39。
        // 主窗可见高 992，副窗内框 = 992 − 39 = 953。
        assert_eq!(
            inner_height_matched_to(1000, insets(8, 8, 0, 8), insets(8, 8, 39, 8), 200),
            Some(953)
        );
        // macOS：主壳无边框，副窗顶部标题栏 28 → 800 − 28 = 772（原行为）。
        assert_eq!(
            inner_height_matched_to(800, DockInsets::zero(), insets(0, 0, 28, 0), 200),
            Some(772)
        );
        // 可见高装不下最小内框：None（保持原尺寸）。
        assert_eq!(
            inner_height_matched_to(100, insets(8, 8, 0, 8), insets(8, 8, 39, 8), 200),
            None
        );
        // 主窗边框比外框还高（不可能出现，防御 checked_sub 下溢）。
        assert_eq!(
            inner_height_matched_to(10, insets(8, 8, 4, 8), insets(0, 0, 0, 0), 0),
            None
        );
    }

    #[test]
    fn dock_position_clamps_oversized_windows_to_monitor_origin() {
        let zero = DockInsets::zero();
        assert_eq!(dock_x(100, 480, 3_000, 0, 1_920, zero, zero), 0);
        assert_eq!(dock_y(100, 2_000, 0, 1_080, 0), 0);
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
