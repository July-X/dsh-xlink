use std::path::PathBuf;

/// Common Controls v6 的应用清单（内容与 tauri-build 自带的那份一致）。
const COMMON_CONTROLS_MANIFEST: &str = include_str!("windows-app-manifest.xml");

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
    // 下面那份 Common Controls v6 清单是**我们自己**编进二进制的
    // （见 `emit_common_controls_manifest`）⇒ ⇒ 它变了也得重编。
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // tauri-build 那份 resource.lib（图标 + 版本信息）继续只交给 **bin**；
        // manifest 由我们自己编一份、用 plain link-arg 发给**所有** target。
        //
        // 为什么不能让 tauri-build 顺带把 manifest 也编进它那份 resource.lib：
        // 那份 lib 只会经 `cargo:rustc-link-arg-bins` 进 bin ⇒ ⇒ manifest 也
        // 只在 bin 里 ⇒ ⇒ example 与 lib 单测二进制拿不到 ⇒ ⇒ 而
        // `comctl32!TaskDialogIndirect`（muda 与 tauri-runtime-wry 的原生对话框
        // 要用）只有 Common Controls v6 才导出 ⇒ ⇒ 缺 manifest 时加载器会去找
        // system32 里的 v5 comctl32 ⇒ ⇒ 进程在进入 main 之前就以
        // STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139) 退出。
        let attrs = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attrs).expect("tauri-build 失败");
        emit_common_controls_manifest();
    } else {
        tauri_build::build();
    }
}

/// 把 Common Controls v6 的 manifest 编成一份**独立**资源，发给**所有** target。
///
/// cargo 没有「只给 lib 单测二进制发 link-arg」这条指令：plain
/// `cargo:rustc-link-arg` 是唯一覆盖得到它的（`cargo:rustc-link-arg-tests` 只管
/// `tests/` 下的集成测试，而本包没有集成测试 ⇒ cargo 会把那条指令整条拒收；
/// `-bins` / `-examples` 则分别只管 bin 与 example）。
///
/// plain 同时也覆盖 bin ⇒ ⇒ tauri-build 那份 resource.lib 里**不能**再带
/// manifest，否则 bin 会拿到两份 manifest ⇒ ⇒ 资源重复 ⇒ 链接期 CVT1100。
/// 这就是上面改用 `new_without_app_manifest()` 的原因。
fn emit_common_controls_manifest() {
    // manifest 必须是**纯 ASCII**。
    //
    // rc.exe 把字符串按系统 ANSI 代码页写进 .res（这台机器上是 936/GBK）——
    // 非 ASCII 的字符会变成 GBK 字节，而 SxS 只认 UTF-8 ⇒ ⇒ 混进中文会让**每一个**
    // 带这份 manifest 的二进制都以 ERROR_SXS_CANT_GEN_ACTCTX (14001) 起不来。
    // ⇒ ⇒ 清单里一个字都不能多；要写说明，写在这个文件（build.rs）里。
    assert!(
        COMMON_CONTROLS_MANIFEST.is_ascii(),
        "windows-app-manifest.xml 里出现了非 ASCII 字符 ⇒ rc.exe 会按 ANSI 代码页写进 .res ⇒ SxS 解析不了"
    );
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let rc_path = out_dir.join("common-controls-v6.rc");
    std::fs::write(&rc_path, manifest_rc()).expect("写 manifest 的 rc 失败");
    // plain（而不是 `-bins`）⇒ ⇒ 这份资源会进到 bin / example / lib 单测
    // 二进制每一个目标里。
    //
    // 编不出来就**让构建失败**（`manifest_required`）—— 静默跳过等于发出一批
    // 「进去就 0xC0000139 退出」的二进制，而那道红只在运行时才显形。
    embed_resource::compile_for_everything(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("编 Common Controls v6 的 manifest 失败");
}

/// 把 manifest 的 XML 摊成 rc 的字符串字面量（rc 里引号要写成两个）。
///
/// 直接把 XML 摊进 rc（而不是在 rc 里引一个 xml 文件）——这样 rc 里不带任何
/// 路径 ⇒ ⇒ 与 rc 编译器的工作目录无关。
fn manifest_rc() -> String {
    let mut rc = String::from("#pragma code_page(65001)\n1 24\n{\n");
    for line in COMMON_CONTROLS_MANIFEST.lines() {
        rc.push_str(&format!("\" {}\" \n", line.replace('"', "\"\"")));
    }
    rc.push_str("}\n");
    rc
}
