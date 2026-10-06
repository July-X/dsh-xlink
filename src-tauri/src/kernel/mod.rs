//! 内核生命周期：安装 / 钉版 / 适配器 / 看护退避。
//!
//! 子模块声明为 `pub(crate)` 而不是 `pub use x::*` 重导出：glob 重导出在两个
//! 子模块导出同名符号时会变成「歧义」，而显式路径 `crate::<组>::<模块>::X`
//! 既无歧义，也保留了「这个符号来自哪个模块」的信息。

pub(crate) mod install_isolation;
pub(crate) mod kernel_adapter;
pub(crate) mod kernel_deps;
pub(crate) mod kernel_evidence;
pub(crate) mod lifecycle;
pub(crate) mod package_activity;
pub(crate) mod profile_manifest;
