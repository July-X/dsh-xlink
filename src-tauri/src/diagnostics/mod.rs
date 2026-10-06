//! 安全网：启动看护、环境回退点、恢复后自检、二分定位。
//!
//! 子模块声明为 `pub(crate)` 而不是 `pub use x::*` 重导出：glob 重导出在两个
//! 子模块导出同名符号时会变成「歧义」，而显式路径 `crate::<组>::<模块>::X`
//! 既无歧义，也保留了「这个符号来自哪个模块」的信息。

pub(crate) mod bisect;
pub(crate) mod bisect_cmd;
pub(crate) mod guard;
pub(crate) mod operation_run;
pub(crate) mod perf;
pub(crate) mod restore;
pub(crate) mod run;
pub(crate) mod run_cmd;
pub(crate) mod snapshot;
pub(crate) mod snapshot_cmd;
pub mod startup_run;
pub(crate) mod verify;
