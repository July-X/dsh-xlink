// 阻止在 Windows release 构建中弹出额外的控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dsh_xlink::run()
}
