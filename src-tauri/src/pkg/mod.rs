//! 取源与出网：npm/GitHub、归档、发布列表、自身更新。
//!
//! 子模块声明为 `pub(crate)` 而不是 `pub use x::*` 重导出：glob 重导出在两个
//! 子模块导出同名符号时会变成「歧义」，而显式路径 `crate::<组>::<模块>::X`
//! 既无歧义，也保留了「这个符号来自哪个模块」的信息。

pub(crate) mod archive;
pub(crate) mod fetch;
pub(crate) mod net_proxy;
pub(crate) mod registry;
pub(crate) mod releases;
pub(crate) mod updater;
