fn main() {
    // Tell cargo to re-run this build script whenever any of the bundled
    // icons change. `tauri-build` invokes the Windows resource compiler
    // with a `.rc` file pointing at `icons/icon.ico`; that step happens
    // at link time and the resulting icon is embedded into the dev exe.
    // Without these `rerun-if-changed` directives cargo's incremental
    // build only watches Rust source files, so an icon update leaves
    // the previous exe binary (with the old icon embedded) in place
    // until the user touches a `.rs` file or runs `cargo clean`. The
    // taskbar then keeps showing the old icon while the on-disk PNGs
    // are already the new design — the exact confusion this set of
    // directives removes. The ico is the one Tauri's `tauri-build`
    // actually embeds on Windows; the PNG entries cover the rest of
    // the bundle set so a macOS-only `.icns` or `.png` change also
    // forces a rebuild through the shared build script.
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
    // tauri-build parses `permissions/` into the app ACL manifest but does
    // not watch it; without this, editing an app permission would not
    // re-run the build script and the change would silently not apply.
    println!("cargo:rerun-if-changed=permissions");
    tauri_build::build()
}
