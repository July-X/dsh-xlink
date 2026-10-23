//! 本地记忆：权威记录、候选与有界检索。
//!
//! 子模块声明为 `pub(crate)` 而不是 `pub use x::*` 重导出（理由同 `shell/`）：
//! 显式路径 `crate::memory::store::X` 保留了「这个符号来自哪个模块」的信息。
//!
//! 边界：这里只管**记录与查询**。运行侧的注入（动态上下文）由随壳交付的
//! `xlink-memory` 内置插件经回环桥调用同一个 store 完成，面板走 Tauri 命令——
//! 两条路都落到本模块，因此写入天然只有一个进程、一把锁。

pub(crate) mod cmd;
pub(crate) mod duplicates;
pub(crate) mod experience;
pub(crate) mod export;
pub(crate) mod handoff;
pub(crate) mod input;
pub(crate) mod jobs;
pub(crate) mod jobs_routes;
pub(crate) mod lock;
pub(crate) mod marks;
pub(crate) mod prune;
pub(crate) mod query;
pub(crate) mod recent;
pub(crate) mod routes;
pub(crate) mod sources;
pub(crate) mod store;
pub(crate) mod workspaces;
