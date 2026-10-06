//! 壳自身：进程、路径解析、实例、状态、窗口、托盘。
//!
//! 子模块声明为 `pub(crate)` 而不是 `pub use x::*` 重导出：glob 重导出在两个
//! 子模块导出同名符号时会变成「歧义」，而显式路径 `crate::<组>::<模块>::X`
//! 既无歧义，也保留了「这个符号来自哪个模块」的信息。

pub(crate) mod autostart;
pub(crate) mod child_priority;
pub(crate) mod env;
pub(crate) mod error;
pub(crate) mod instance;
pub(crate) mod localtime;
#[cfg(target_os = "macos")]
pub(crate) mod menu_bar;
pub(crate) mod paths;
pub(crate) mod process;
pub(crate) mod registry_split;
pub(crate) mod resident;
pub(crate) mod settings;
pub(crate) mod shell_events;
pub(crate) mod state;
pub(crate) mod store_relocate;
pub(crate) mod stream;
#[cfg(target_os = "windows")]
pub(crate) mod tray;
pub(crate) mod version;
pub(crate) mod window;
