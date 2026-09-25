// 独立窗口通用助手：吸附定位 + 移动跟随。
//
// 「吸附」指打开时把窗口贴在主窗右侧（主窗贴近屏幕右缘时自动翻左侧）、
// 与主窗顶边对齐（底部超出行程则上移夹回屏内），让两个窗口看起来像同一
// 个工作区。「移动跟随」指主窗被拖动时，副窗通过主窗的 `on_window_event`
// 监听 `Moved` 事件按合帧窗口（~60fps）持续 `set_position`，始终保持吸附。
//
// 这两条路径被日志查看器（`commands::open_log_window`）和模型用量窗口
// （`usage::open_usage_window`）共用——它们的差别只是窗口尺寸与 label，
// 几何算法完全一致。共用的好处是 dock / 跟随的行为绝对不会「一个修一个忘」。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

/// 吸附 X（物理像素）：优先把窗口贴在主窗右侧；右侧放不下就翻到主窗左侧。
/// `mon_x + mon_w` 是当前显示器右缘（含多显示器负坐标场景）。
pub fn dock_x(main_x: i32, main_w: i32, win_w: i32, mon_x: i32, mon_w: i32) -> i32 {
    let right = main_x + main_w;
    if right + win_w <= mon_x + mon_w {
        right
    } else {
        main_x - win_w
    }
}

/// 吸附 Y（物理像素）：与主窗顶对齐；底部超出行程就上移、夹在屏内。
pub fn dock_y(main_y: i32, win_h: i32, mon_y: i32, mon_h: i32) -> i32 {
    if main_y + win_h > mon_y + mon_h {
        mon_y + mon_h - win_h
    } else {
        main_y
    }
}

/// 计算指定尺寸下的吸附位置（物理像素）。主窗不在（理论上不会）或取不
/// 到显示器信息时返回 `None`，调用方据此决定是否落到默认居中。
pub fn compute_dock_position(main: &WebviewWindow, win_w: f64, win_h: f64) -> Option<(i32, i32)> {
    let scale = main.scale_factor().ok()?;
    let pos = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    let monitor = main.current_monitor().ok().flatten()?;
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let width_phys = (win_w * scale) as i32;
    let height_phys = (win_h * scale) as i32;
    let x = dock_x(
        pos.x,
        size.width as i32,
        width_phys,
        m_pos.x,
        m_size.width as i32,
    );
    let y = dock_y(pos.y, height_phys, m_pos.y, m_size.height as i32);
    Some((x, y))
}

/// 把屏幕物理坐标换算成 builder 期望的逻辑坐标。`compute_dock_position`
/// 返回物理像素，WebviewWindowBuilder 的 `position` 要的是逻辑像素。
pub fn dock_position_logical(main: &WebviewWindow, win_w: f64, win_h: f64) -> Option<(f64, f64)> {
    let scale = main.scale_factor().ok()?;
    let (x, y) = compute_dock_position(main, win_w, win_h)?;
    Some((x as f64 / scale, y as f64 / scale))
}

/// 移动跟随监听器：主窗被拖动时，副窗（`target_label`）按 [`compute_dock_position`]
/// 重算并 `set_position`，保持吸附状态。`on_window_event` 挂在主窗上
/// （`Moved` 在拖动全程连续触发），所以开不开副窗都能安全挂载——本函数
/// 内部用 `get_webview_window(target_label)` 短路未开窗的情况。
///
/// 副窗自身的移动不经过这条路径（事件源是 `main`），不存在两窗互相拉扯
/// 的回环；主窗固定不可缩放，`Resized` 也不需要处理。
pub fn attach_dock_listener(app: &AppHandle, target_label: &'static str, win_w: f64, win_h: f64) {
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
        {
            let mut last = last_apply
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let now = Instant::now();
            if last.is_some_and(|t| now.duration_since(t) < Duration::from_millis(15)) {
                return;
            }
            *last = Some(now);
        }
        let Some((x, y)) = compute_dock_position(&main_for_handler, win_w, win_h) else {
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
}
