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

/// 窗口标题里那个应用名。
///
/// 主窗口的标题栏是 `ui/src/shell/WindowTitleBar.vue` **自绘**的（无边框，
/// `tauri.conf.json` 的 `decorations: false`），文案硬编码在那边；这里写的是
/// 同一份。两侧分处 Rust / Vue，任何一边单独改都会让主窗口与副窗的标题对不上，
/// 而那种不一致只在真机窗口标题栏上看得到——单测与 UI 测试都够不着，所以由
/// `scripts/check-invariants.mjs` 机械比对三处（这里、那个 .vue、tauri.conf.json）。
pub const APP_TITLE: &str = "Dsh-Xlink";

/// 副窗标题：`窗口名 — 应用名`（macOS 访达 / 邮件的惯例）。
///
/// 不直接写死一个标题串的原因：标题里那个应用名要跟着 [`APP_TITLE`] 走，而
/// `APP_TITLE` 已经在三处出现（见上）。在这里拼一次，下一个人改名字只改一个常量。
pub fn window_title(name: &str) -> String {
    format!("{name} — {APP_TITLE}")
}

/// 窗口/WebView 在首帧文档绘制之前使用的底色。
///
/// WebView 的默认底色是纯白：远程页面（工作台、官网网页版）在网络请求与首屏
/// 渲染完成前会有几百毫秒空白期，在深色壳里读作一记刺眼的白闪。窗口层
/// （NSWindow / HWND 背景）与 WebView 层设成同一种底色后，这段空白期呈现的是
/// 与目标页面同色系的暗底，加载完成时只是内容淡入，而不是从白到黑的跳变。
///
/// **曾经跟随系统主题**（读主壳窗口的 theme，浅色系统下用接近页面的浅灰）。
/// 2026-10-07 起不再是条件值：所有窗口都被钉死 `Theme::Dark`（见
/// [`APP_TITLE`] 那段同批改动），主窗口的标题栏更是恒定深色，跟系统主题无关。
/// 留着浅色分支只会让下一个人以为「深色窗口 + 浅色底色」是受支持的组合——那是
/// 修复前的样子，正是用户截图里「深色内容 + 浅色标题栏」的成因。
pub const CHROME_BACKDROP: tauri::webview::Color = tauri::webview::Color(0x16, 0x17, 0x1a, 0xff);

/// 三扇壳自有副窗共用的窗口装饰：无边框 + 透明 + 无系统阴影。
///
/// **三处 builder 各写一份必然漂**，所以收在这里（2026-10-08 用户要求副窗交通灯
/// 与主壳统一、窗口圆角一致时新增）。主壳走 `tauri.conf.json` 的声明式配置，
/// 那一侧同样不写在这里——它不是 builder 建的。
///
/// 为什么是「自绘标题栏 + 无边框」而不是「原生标题栏 + 系统圆角」：副窗原本走
/// 原生装饰，于是交通灯与圆角都由系统画——尺寸、间距、hover 全部不可控，与主壳
/// 那套按 macOS 实测对齐的自绘灯对不上（实测直径 12 / 间距 23 / 左缘 7）。
/// 两套不同源的 chrome 并排摆着，视觉上就是「一个是本应用的窗、一个是系统窗」。
///
/// 为什么 `shadow: false` 不能省：macOS 会给每个窗口矩形画一层系统投影。窗口不
/// 透明时那层投影藏在边框下看不见；窗口透明 + 圆角后，投影会画在**圆角之外的
/// 透明区域**，读作一圈贴着窗口外框的黑色矩形光晕。去掉之后阴影由前端 CSS 补
/// （见 `ui/src/theme.css` 的 `--window-radius` 一节），形状跟着 CSS 圆角走。
///
/// **这三项必须建窗时给，不能建完再 `set_decorations` / `set_shadow`**：
/// tao 的 `set_decorations(false)` 会把 `NSWindowStyleMask` 重算成
/// `Borderless | Resizable`，**丢掉 `Miniaturizable`**，于是 `miniaturize:` 静默
/// 失败、`minimize()` 还返回 `Ok(())`，黄灯变成点了没反应的死按钮（根因与
/// `crate::check_main_window_minimizable` 那条回归哨兵同源）。`transparent` 则
/// 走的是另一条路径——只 `setOpaque(false)` + `setBackgroundColor(clearColor)`，
/// 不碰样式位（tao 0.35 `platform_impl/macos/window.rs`），所以它是安全的。
/// 泛型保持与 `WebviewWindowBuilder` 一致（`<R: Runtime, M: Manager<R>>`）：
/// 这里原样透传调用方的运行时就��，不把它钉死成 `Wry`——三扇副窗都由同一个
/// `tauri::AppHandle` 建窗，但写死会让「换个 runtime 试一下」变成改签名。
pub fn decorate_transparent<R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: tauri::WebviewWindowBuilder<'_, R, M>,
) -> tauri::WebviewWindowBuilder<'_, R, M> {
    builder
        .decorations(false)
        .shadow(false)
        .transparent(true)
        // 透明窗的 WebView 层必须自己清成全透明，否则文档绘制前会闪一帧白
        // （`CHROME_BACKDROP` 那段注释描述的同一段空白期，只是这里不能填暗色：
        // 填了就又把圆角填死了）。
        .background_color(tauri::webview::Color(0, 0, 0, 0))
}

/// 一扇窗口在屏幕上**实际可见**的边界（绝对物理坐标）：含标题栏与可见
/// 边框，不含阴影与不可见缩放边框。吸附对齐的唯一权威口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// 可见帧相对外框 rect 的四边内缩（即不可见缩放边框 / 阴影留白的宽度）。
/// `Default`（全 0）即「外框 == 可见帧」。
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

/// 量出窗口的可见帧。Windows 走 DWM 扩展帧边界
/// （`DWMWA_EXTENDED_FRAME_BOUNDS`）——这是 DWM 实际绘制窗口边缘的位置：
/// 主壳（tao 的「无装饰 + 阴影」窗口）与副窗（带装饰）的 rect 里都各含
/// 约 7 逻辑像素不可见缩放边框，客户区/外框坐标都推不出可见缘（客户区
/// 偏移还随窗口样式与 Win10/11 变化，两轮真机残缝都出自这里）。其余平台
/// 退回外框 rect（macOS 无不可见边框，可见 == 外框）。
fn visible_frame(win: &WebviewWindow) -> Option<Frame> {
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    let fallback = Frame {
        left: pos.x,
        top: pos.y,
        right: pos.x + size.width as i32,
        bottom: pos.y + size.height as i32,
    };
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{HWND, RECT};
        use windows_sys::Win32::Graphics::Dwm::{
            DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS,
        };
        let tauri_hwnd = win.hwnd().ok()?;
        // tauri 返回 windows crate 的 HWND（指针的透明包装）；windows-sys 的
        // HWND 只是裸指针别名，经 isize 中转兼容两种表示。
        let hwnd: HWND = tauri_hwnd.0 as isize as *mut core::ffi::c_void;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let hr = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                &mut rect as *mut RECT as *mut core::ffi::c_void,
                std::mem::size_of::<RECT>() as u32,
            )
        };
        if hr == 0 {
            return Some(Frame {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            });
        }
    }
    Some(fallback)
}

/// 可见帧相对外框 rect 的四边内缩（物理像素）。
fn frame_insets(win: &WebviewWindow, frame: &Frame) -> DockInsets {
    let Ok(pos) = win.outer_position() else {
        return DockInsets::zero();
    };
    let Ok(size) = win.outer_size() else {
        return DockInsets::zero();
    };
    DockInsets {
        left: (frame.left - pos.x).max(0),
        right: (pos.x + size.width as i32 - frame.right).max(0),
        top: (frame.top - pos.y).max(0),
        bottom: (pos.y + size.height as i32 - frame.bottom).max(0),
    }
}

/// 吸附 X（物理像素）：优先把副窗的**可见帧**贴在主窗可见帧右侧；右侧
/// 放不下就翻到主窗可见帧左侧。副窗外框落点按其可见帧相对外框的内缩
/// 反推——两侧都用 DWM 实测可见帧，缝隙不再依赖任何窗口样式推断。
fn dock_x(main_frame: &Frame, win_w: i32, win_insets: DockInsets, mon_x: i32, mon_w: i32) -> i32 {
    let monitor_right = mon_x.saturating_add(mon_w);
    let preferred = if main_frame
        .right
        .saturating_sub(win_insets.left)
        .saturating_add(win_w)
        <= monitor_right
    {
        // 右侧贴齐：副窗可见左缘落在主窗可见右缘。
        main_frame.right.saturating_sub(win_insets.left)
    } else {
        // 翻左侧：副窗可见右缘落在主窗可见左缘。
        main_frame
            .left
            .saturating_sub(win_w.saturating_sub(win_insets.right))
    };
    clamp_position(preferred, mon_x, mon_w, win_w)
}

/// 吸附 Y（物理像素）：副窗可见帧顶与主窗可见帧顶对齐；底部超出行程就
/// 上移、夹在屏内。
fn dock_y(main_frame_top: i32, win_h: i32, mon_y: i32, mon_h: i32, win_insets_top: i32) -> i32 {
    let preferred = main_frame_top.saturating_sub(win_insets_top);
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

/// 吸附定位的纯几何输入（物理像素）。收拢成结构体让纯函数 `dock_position_for`
/// 的参数数保持克制，也方便单测直接构造。
struct DockQuery {
    main_frame: Frame,
    win_size: (i32, i32),
    win_insets: DockInsets,
    mon_pos: (i32, i32),
    mon_size: (i32, i32),
}

fn dock_position_for(q: DockQuery) -> (i32, i32) {
    let x = dock_x(
        &q.main_frame,
        q.win_size.0,
        q.win_insets,
        q.mon_pos.0,
        q.mon_size.0,
    );
    let y = dock_y(
        q.main_frame.top,
        q.win_size.1,
        q.mon_pos.1,
        q.mon_size.1,
        q.win_insets.top,
    );
    (x, y)
}

/// 计算指定尺寸下的吸附位置（物理像素）。主窗不在（理论上不会）或取不
/// 到显示器信息时返回 `None`，调用方据此决定是否落到默认居中。
/// 副窗尚未建出、量不到自己的可见帧，内缩按 0 估计——建出后由
/// [`snap_to_main`] 以实测值校正；主窗已存在，它的可见帧照实量。
pub fn compute_dock_position(main: &WebviewWindow, window_size: WindowSize) -> Option<(i32, i32)> {
    let scale = main.scale_factor().ok()?;
    let main_frame = visible_frame(main)?;
    let monitor = main.current_monitor().ok().flatten()?;
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let width_phys = (window_size.width * scale) as i32;
    let height_phys = (window_size.height * scale) as i32;
    Some(dock_position_for(DockQuery {
        main_frame,
        win_size: (width_phys, height_phys),
        win_insets: DockInsets::zero(),
        mon_pos: (m_pos.x, m_pos.y),
        mon_size: (m_size.width as i32, m_size.height as i32),
    }))
}

/// 与主窗**可见内容**高度对齐所需的副窗内框高度（物理像素）。
/// 副窗外框高 = 主窗可见高 + 副窗可见帧上下内缩；内框 = 外框高 − 副窗
/// 纵向装饰（标题栏 + 边框，实测 `win_chrome_v`）。装不下 `min_inner`
/// 就返回 None，调用方保持原尺寸不动。
fn inner_height_matched_to(
    main_visible_h: i32,
    win_insets: DockInsets,
    win_chrome_v: i32,
    min_inner: i32,
) -> Option<i32> {
    let outer = main_visible_h
        .checked_add(win_insets.top)?
        .checked_add(win_insets.bottom)?;
    let inner = outer.checked_sub(win_chrome_v)?;
    (inner >= min_inner).then_some(inner)
}

/// 建窗后的吸附校正（三类查看器窗口共用）：builder 阶段只有逻辑目标尺寸，
/// 两窗的真实可见帧（标题栏 + 不可见缩放边框 + 阴影留白）要等窗口建出来
/// 才能量到。这里以 DWM 实测为准做两件事：
/// 1. **可见内容高度对齐**：`inner_size(…, 800)` 建出的窗口可见高度与主壳
///    差出一截标题栏，Windows 上视觉尤其明显；装不下最小内框时不动尺寸；
/// 2. **可见缘贴齐**：右侧放不下翻左侧，均夹在主窗所在屏幕内。
///
/// 主窗不在（理论不会）或量不到尺寸时保持 builder 落下的位置不动。
pub fn snap_to_main(handle: &AppHandle, label: &'static str) {
    let (Some(main), Some(win)) = (
        handle.get_webview_window("main"),
        handle.get_webview_window(label),
    ) else {
        return;
    };
    let Some(main_frame) = visible_frame(&main) else {
        return;
    };
    let Some(win_frame) = visible_frame(&win) else {
        return;
    };
    let Ok(win_outer) = win.outer_size() else {
        return;
    };
    let Ok(win_inner) = win.inner_size() else {
        return;
    };
    let win_insets = frame_insets(&win, &win_frame);
    // 1) 可见内容高度与主窗对齐（宽度不动：可缩放窗口的默认宽度已定，拉伸
    //    交给用户）。先定尺寸再落位，落位按目标可见高推外框高，不依赖尺寸
    //    调整是否已即时生效。
    let main_visible_h = main_frame.bottom.saturating_sub(main_frame.top);
    let chrome_v = win_outer.height.saturating_sub(win_inner.height) as i32;
    if let Some(inner_h) = inner_height_matched_to(main_visible_h, win_insets, chrome_v, 200) {
        let scale = win.scale_factor().unwrap_or(1.0);
        let _ = win.set_size(LogicalSize::new(
            win_inner.width as f64 / scale,
            inner_h as f64 / scale,
        ));
    }
    // 2) 可见缘贴齐。副窗目标外框高 = 主窗可见高 + 副窗可见帧上下内缩
    //    （可见底对齐时外框底要再多留出副窗的下边框）。
    let Some(monitor) = main.current_monitor().ok().flatten() else {
        return;
    };
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let win_h = main_visible_h + win_insets.top + win_insets.bottom;
    let (x, y) = dock_position_for(DockQuery {
        main_frame,
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
                // 只取唤醒时机：可见帧在 tick 内对两窗实时测量（与显示器同
                // 一时刻的实测值），Moved 事件携带的坐标不再单独使用。
                let _ = {
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
                // 与 snap_to_main 同一套可见帧测量：拖动跟随时两窗都按 DWM
                // 实测可见缘贴齐，否则开窗时贴住的缝隙会在第一次拖动后重新
                // 出现（主窗跨屏拖动还会换 DPI，不可见边框宽度随之变化，所
                // 以每 tick 都重新量）。
                let Some(main_frame) = visible_frame(&main_for_worker) else {
                    continue;
                };
                let Some(win_frame) = visible_frame(&target) else {
                    continue;
                };
                let Some(monitor) = main_for_worker.current_monitor().ok().flatten() else {
                    continue;
                };
                let m_pos = monitor.position();
                let m_size = monitor.size();
                let (x, y) = dock_position_for(DockQuery {
                    main_frame,
                    win_size: (target_size.width as i32, target_size.height as i32),
                    win_insets: frame_insets(&target, &win_frame),
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

    fn frame(left: i32, top: i32, right: i32, bottom: i32) -> Frame {
        Frame {
            left,
            top,
            right,
            bottom,
        }
    }

    fn insets(left: i32, right: i32, top: i32, bottom: i32) -> DockInsets {
        DockInsets {
            left,
            right,
            top,
            bottom,
        }
    }

    /// 吸附 X：主窗在屏幕中部时贴主窗右侧；主窗贴近右缘时翻到主窗左侧。
    /// 基线：副窗无不可见边框（macOS 外框即可见帧）。
    #[test]
    fn dock_x_prefers_right_side_and_flips_left_when_offscreen() {
        let zero = DockInsets::zero();
        // 主窗在屏幕中部（可见右缘 580）：吸附到主窗右缘。
        assert_eq!(dock_x(&frame(100, 0, 580, 800), 760, zero, 0, 1920), 580);
        // 主窗贴屏幕右缘：右侧放不下 → 贴到主窗左侧。
        assert_eq!(dock_x(&frame(1440, 0, 1920, 800), 760, zero, 0, 1920), 680);
        // 副屏在左侧（负坐标）同样成立：贴主窗左侧 = 主窗可见左缘 − 窗宽。
        assert_eq!(
            dock_x(&frame(-1000, 0, -520, 800), 760, zero, -1920, 1920),
            -1760
        );
    }

    /// 可见缘贴齐（Windows 两窗 rect 里都有不可见缩放边框，DWM 实测可见帧
    /// 后按可见缘对齐）：右侧贴齐时副窗可见左缘 = 主窗可见右缘；翻左侧时
    /// 副窗可见右缘 = 主窗可见左缘。这是两轮真机残缝（只补副窗 / 主窗自
    /// 身边框未补）的最终修正。
    #[test]
    fn dock_x_aligns_visible_frames_on_both_sides() {
        // 主窗可见右缘 572（外框 580 内缩 8）；副窗可见左缘内缩 8。
        // 右侧贴齐：x = 572 − 8 = 564，副窗可见左缘 = 564 + 8 = 572 ✓。
        assert_eq!(
            dock_x(&frame(100, 0, 572, 800), 760, insets(8, 8, 0, 8), 0, 1920),
            564
        );
        // 主窗贴屏幕右缘：翻左侧。主窗可见左缘 1448；x = 1448 − 760 + 8 =
        // 696，副窗可见右缘 = 696 + 760 − 8 = 1448 ✓。
        assert_eq!(
            dock_x(&frame(1448, 0, 1920, 800), 760, insets(8, 8, 0, 8), 0, 1920),
            696
        );
        // DPI 1.5 下边框更宽（7 × 1.5 ≈ 11）：可见缘同样精确重合。
        assert_eq!(
            dock_x(
                &frame(100, 0, 569, 800),
                760,
                insets(11, 11, 0, 11),
                0,
                1920
            ),
            558
        );
    }

    /// 吸附 Y：副窗可见帧顶与主窗可见帧顶对齐（带装饰副窗的顶部内缩为 0，
    /// 标题栏本身可见）；底部超出行程则上移夹回屏内。
    #[test]
    fn dock_y_aligns_visible_top_and_clamps_inside_monitor() {
        // 常规：与主窗可见顶对齐。
        assert_eq!(dock_y(100, 800, 0, 1000, 0), 100);
        // 副窗可见帧相对外框顶部有内缩时，外框顶再上移同样距离。
        assert_eq!(dock_y(100, 800, 0, 1000, 5), 95);
        // 主窗偏下、800 高放不下：上移到屏幕底缘。
        assert_eq!(dock_y(300, 800, 0, 1000, 0), 200);
        // 恰好放得下（底边贴齐屏幕底缘）：原样对齐。
        assert_eq!(dock_y(-200, 800, -400, 1000, 0), -200);
    }

    /// 可见内容高度对齐：副窗外框高 = 主窗可见高 + 副窗可见帧上下内缩，
    /// 内框 = 外框高 − 纵向装饰（标题栏 + 边框）；装不下最小内框返回 None。
    #[test]
    fn inner_height_matched_to_aligns_visible_heights() {
        // Windows：主窗可见高 800；副窗上下内缩各 8、纵向装饰 47（标题栏
        // + 边框）。外框 = 800 + 0 + 8 = 808，内框 = 808 − 47 = 761。
        assert_eq!(
            inner_height_matched_to(800, insets(8, 8, 0, 8), 47, 200),
            Some(761)
        );
        // macOS：副窗无内缩、纵向装饰 28（标题栏）→ 800 − 28 = 772（原行为）。
        assert_eq!(
            inner_height_matched_to(800, DockInsets::zero(), 28, 200),
            Some(772)
        );
        // 可见高装不下最小内框：None（保持原尺寸）。
        assert_eq!(
            inner_height_matched_to(100, insets(8, 8, 0, 8), 47, 200),
            None
        );
        // 装饰比可见高还大（不可能出现，防御 checked_sub 下溢）。
        assert_eq!(inner_height_matched_to(10, DockInsets::zero(), 47, 0), None);
    }

    /// 纯几何入口 dock_position_for：X/Y 一把算出，且钳在屏幕内。
    #[test]
    fn dock_position_for_combines_axes_and_clamps() {
        let (x, y) = dock_position_for(DockQuery {
            main_frame: frame(100, 0, 572, 800),
            win_size: (760, 808),
            win_insets: insets(8, 8, 0, 8),
            mon_pos: (0, 0),
            mon_size: (1920, 1000),
        });
        // 副窗可见左缘 572 = 主窗可见右缘；可见顶 0 = 主窗可见顶。
        assert_eq!((x, y), (564, 0));
        let (x, y) = dock_position_for(DockQuery {
            main_frame: frame(100, 900, 572, 1700),
            win_size: (760, 808),
            win_insets: insets(8, 8, 0, 8),
            mon_pos: (0, 0),
            mon_size: (1920, 1000),
        });
        // 主窗偏下放不下：X 照常贴齐，Y 钳到屏幕底缘。
        assert_eq!((x, y), (564, 192));
    }

    #[test]
    fn dock_position_clamps_oversized_windows_to_monitor_origin() {
        let zero = DockInsets::zero();
        assert_eq!(dock_x(&frame(100, 0, 580, 800), 3_000, zero, 0, 1_920), 0);
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

    /// 副窗标题的**形状**，不是某一句话。`check-invariants` 第 18 项管的是
    /// 「三处名字一致 + 每扇窗都钉死主题」（跨文件），这里管「窗口名与应用名
    /// 之间那个分隔符长什么样」——它是纯本地断言，单测最省事。
    ///
    /// 分隔符钉成破折号两侧各一个空格而不是 `-`：标题栏里已经出现了一次
    /// 分隔（`日志 · kernel`），再用半角连字符会连着出现两个同样形状的短横，
    /// 而 macOS 的标题栏字号下两者几乎分辨不出。
    #[test]
    fn window_title_joins_name_and_app_with_a_spaced_em_dash() {
        assert_eq!(window_title("套餐用量"), "套餐用量 — Dsh-Xlink");
        // 名字里自带分隔符时也不会拼出两个同款短横。
        assert_eq!(window_title("日志 · kernel"), "日志 · kernel — Dsh-Xlink");
        // 应用名跟着常量走，改名时这条断言会跟着指出所有窗口标题。
        assert!(window_title("x").ends_with(APP_TITLE));
    }
}
