//! 官网对话引用的壳侧取数、账户身份和协议校验。
//!
//! 目录与正文共用站点规格表。Windows 观察站点自然请求；macOS 使用页内
//! 同源取数，MiniMax 使用临时旁听。平台边界和已知限制见 P4 当前协议及
//! docs/features/official-chat/official-chat-current-status.md。
//! 账户回包只包含站点及摘要；原始用户响应不交给插件。

// 页内回程由 macOS 生产路径使用，其他平台仍有独立探针调用。
#[allow(dead_code)]
pub(crate) mod cdp_eval;
#[allow(dead_code)]
pub(crate) mod context;
#[allow(dead_code)]
pub(crate) mod script_eval;

pub(crate) mod account;
pub(crate) mod session_body;
// `account_contract` 不在这里声明：它经 `#[path]` 挂成 session_structure 的
// 子模块（原因见那条声明处的注释），`account` 从那边引用。
pub(crate) mod session_structure;

// 双端夹具使用这份协议实现，生产快照由插件 host 管理。
#[allow(dead_code)]
pub(crate) mod protocol;
