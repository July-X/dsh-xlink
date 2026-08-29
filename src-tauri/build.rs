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
    ] {
        println!("cargo:rerun-if-changed={icon}");
    }
    // tauri-build 会把 `permissions/` 解析进应用 ACL 清单，但并不
    // 监听该目录；没有下面这一行的话，修改应用权限就不会重新运行
    // 构建脚本，相关改动会被静默忽略。
    println!("cargo:rerun-if-changed=permissions");
    tauri_build::build()
}
