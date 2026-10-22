fn main() {
    // 通知 cargo 在任意打包图标发生变化时重新运行本构建脚本。
    // `tauri-build` 会通过一个指向 `icons/icon.ico` 的 `.rc` 文件调用
    // Windows 资源编译器；该步骤发生在链接时，最终图标会被嵌入到
    // 开发版 exe 中。如果没有这些 `rerun-if-changed` 指令，cargo
    // 的增量构建只会监听 Rust 源码文件，因此更新图标后旧版 exe
    // 二进制（仍嵌入旧图标）会原样保留，直到用户改动 `.rs` 文件
    // 或执行 `cargo clean`。此时任务栏仍显示旧图标，而磁盘上的
    // PNG 已经是新设计——这正是这一组指令所要消除的混乱。
    // ico 是 Tauri 的 `tauri-build` 在 Windows 上真正嵌入的那一个；
    // PNG 条目覆盖了其余的打包集合，以便仅 macOS 的 `.icns` 或
    // `.png` 变更也能通过共享构建脚本触发重新构建。
    for icon in [
        "icons/icon.ico",
        "icons/icon.icns",
        "icons/icon.png",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        // 通知区域图标由 `tray.rs` 的 `include_image!` 在编译期嵌入（十二档：
        // 浅色/深色任务栏各六档，运行时按主题 + 显示缩放选帧，漏掉哪一档，
        // 换掉它之后 cargo 就不会重新编译，那一档会一直停在旧图）。同样需要
        // 在这里声明，否则换掉托盘图标后 cargo 不会重新编译。
        "icons/tray-dark-16.png",
        "icons/tray-dark-20.png",
        "icons/tray-dark-24.png",
        "icons/tray-dark-32.png",
        "icons/tray-dark-40.png",
        "icons/tray-dark-48.png",
        "icons/tray-light-16.png",
        "icons/tray-light-20.png",
        "icons/tray-light-24.png",
        "icons/tray-light-32.png",
        "icons/tray-light-40.png",
        "icons/tray-light-48.png",
        // macOS 菜单栏图标（2026-10-02）：同样由 `menu_bar.rs` 的
        // `include_image!` 在编译期嵌入（1x / 2x 两档模板图，系统按菜单栏
        // 明暗自动反色）。漏声明的后果与托盘十二档一样：换掉图标后
        // cargo 不重新编译，菜单栏会一直停在旧图，而磁盘上已经是新设计。
        "icons/menubar-22.png",
        "icons/menubar-44.png",
    ] {
        println!("cargo:rerun-if-changed={icon}");
    }
    // tauri-build 会把 `permissions/` 解析进应用 ACL 清单，但并不
    // 监听该目录；没有下面这一行的话，修改应用权限就不会重新运行
    // 构建脚本，相关改动会被静默忽略。
    println!("cargo:rerun-if-changed=permissions");
    tauri_build::build();

    // examples 也需要 Windows manifest（comctl32 v6）。
    //
    // tauri-build 把图标/版本信息/manifest 编成一个 resource.lib，但它只
    // 通过 `cargo:rustc-link-arg-bins` 交给 bin 链接。example 拿不到这个
    // .lib，而 comctl32 的 TaskDialogIndirect（muda 与 tauri-runtime-wry
    // 的原生对话框要用）只有 v6 版本才导出——缺 manifest 时加载器会去找
    // system32 里的 v5 comctl32，解析不到该导出，进程在进入 main 之前
    // 就以 STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139) 退出。
    //
    // 这里把同一个 .lib 直接交给 example 的链接步骤，行为与 bin 对齐。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let resource_lib = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default())
            .join("resource.lib");
        if resource_lib.exists() {
            println!("cargo:rustc-link-arg-examples={}", resource_lib.display());
        }
    }
}
