//! macOS 菜单栏（menu bar）常驻入口。
//!
//! 与 Windows 托盘（[`crate::shell::tray`]）是同一件事的两端实现：给一个
//! **常驻图标 + 一份菜单**，点图标把管理面板叫回来、菜单里只有一条显式的
//! 「退出 dsh-xlink」才真正结束进程。行为（收起 / 恢复 / 退出）都在
//! [`crate::shell::resident`]，这里只负责图标与菜单的呈现。
//!
//! 2026-10-02 之前 macOS 完全没有常驻入口：点关闭就是退出（内核在跑时弹
//! 一次确认），最小化进 Dock。这与 Windows 上的「关窗继续在后台跑」是两种
//! 心智模型，同一份产品换个平台就得重新学一遍。统一之后，窗口消失不等于
//! 程序结束，唯一的结束入口是菜单栏图标。
//!
//! 图标是**模板图**（`menubar-22.png` / `menubar-44.png`，纯黑 + alpha）：
//! macOS 按当前菜单栏明暗自动反色，我们既不需要主题事件也拿不到它。

use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

/// 菜单栏图标 id：留一个常量是为了 `hide_menu_bar`（退出期）能稳定取回
/// 同一个托盘项。
const TRAY_ID: &str = "menubar";

/// 菜单栏图标的 1x / 2x 两档。
///
/// 菜单栏高度固定 22pt，不存在 Windows 通知区域那套「按 DPI 在 6 档里选一
/// 档」的问题：系统按当前显示缩放取合适的那一档，44 那档在 Retina 上就是
/// 2x 资源。两档都在编译期解码成 RGBA 常量，运行时无解码依赖。
static MENUBAR_FRAMES: [(i32, tauri::image::Image<'static>); 2] = [
    (22, tauri::include_image!("icons/menubar-22.png")),
    (44, tauri::include_image!("icons/menubar-44.png")),
];

/// 菜单栏的状态项。托盘图标的右键菜单由 [`super::resident::build_background_menu`]
/// 建好并常驻——菜单项只有两条且文案固定，没有理由每次弹出前重建，也**不该**
/// 在这里另建一份（两端的「退出」接线重复过一次，14 行逐字相同）。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(MENUBAR_FRAMES[0].1.clone())
        // 关键的一条：不设它，图标会带着自己的颜色被原样画在菜单栏里。菜单栏
        // 的明暗由系统决定（浅色桌面 = 菜单栏深色，反之亦然），我们既拿不到
        // 变化事件，也无法预知当前值。模板图让系统替我们反色。
        .icon_as_template(true)
        .tooltip("Dsh-Xlink 桌面管理台")
        // 右键出菜单；左键在共用接线里处理成「显示管理面板」——点菜单栏图标
        // 就把面板叫回来，比先右键再选更符合 macOS 肌肉记忆。
        .show_menu_on_left_click(false);
    // 措辞与 Windows 端不同是有意的：此时 Dock 上没有任何窗口条目，说
    // 「主界面」会让人以为那儿还有个窗口；说应用名更清楚自己在点什么。
    super::resident::build_background_menu(builder, app, "显示 Dsh-Xlink", "退出 Dsh-Xlink")?;
    Ok(())
}

/// 退出期摘掉菜单栏图标。
///
/// 进程即将结束时留着它毫无意义，而它会一直可点到「一条没有反应的退出」——
/// macOS 上这个图标属于状态项，进程一死系统就自动移除，这里只是让退出过程中
/// 那几秒里点它有明确反馈（而不是把一个已关的窗口叫起来）。
pub fn hide_menu_bar(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
}

/// 菜单栏入口是否可用。没有它 macOS 上就没有后台恢复路径。
pub fn present(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 菜单栏图标必须是**模板图**（纯黑 + alpha），否则 `icon_as_template(true)`
    /// 没有东西可反色：图标会以固定深色画在浅色菜单栏上，等于看不见。
    ///
    /// 反向验：把任一帧换成带彩色像素的图（托盘那两套就是），检查立刻红。
    /// 这条断言守的是「谁在什么时候换掉了模板图」——它一行代码就写完了，但
    /// 换掉之后的症状是「macOS 上菜单栏图标看不见」，从界面上根本看不出
    /// 是这个原因。
    #[test]
    fn frames_are_black_with_alpha_only() {
        for (size, image) in &MENUBAR_FRAMES {
            let rgba = image.rgba();
            assert_eq!(
                rgba.len(),
                (*size as usize) * (*size as usize) * 4,
                "{size}px 帧的字节数与尺寸不符"
            );
            let mut colors = std::collections::HashSet::new();
            let mut opaque = 0usize;
            for pixel in rgba.chunks_exact(4) {
                colors.insert((pixel[0], pixel[1], pixel[2]));
                if pixel[3] > 0 {
                    opaque += 1;
                }
            }
            assert!(
                colors.iter().all(|(r, g, b)| *r == 0 && *g == 0 && *b == 0),
                "{size}px 帧含非黑像素 {colors:?}：模板图只该有黑色，\
                 有彩色说明被换成了非模板图（托盘那两套或应用图标）"
            );
            assert!(
                opaque > 0 && opaque < rgba.len() / 4,
                "{size}px 帧的不透明像素 {opaque} 不在一个剪影的量级：\
                 0 = 全透明（图标消失），接近全满 = 一块实心方块（不是剪影）"
            );
        }
    }

    /// 两档必须尺寸不同且都存在。系统按显示缩放自己挑档，两档相同等于
    /// Retina 上把 1x 放大；少一档则等于默认分辨率下没有可用尺寸。
    #[test]
    fn both_scales_are_present_and_distinct() {
        let sizes: Vec<i32> = MENUBAR_FRAMES.iter().map(|(size, _)| *size).collect();
        assert_eq!(sizes, vec![22, 44], "menu bar 固定 22pt，1x/2x 两档");
    }
}
