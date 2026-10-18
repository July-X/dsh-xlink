//! 用量统计：本地 token 账目、云端套餐、凭据只读解析。
//!
//! 子模块声明为 `pub(crate)` 而不是 `pub use x::*` 重导出：glob 重导出在两个
//! 子模块导出同名符号时会变成「歧义」，而显式路径 `crate::<组>::<模块>::X`
//! 既无歧义，也保留了「这个符号来自哪个模块」的信息。

pub(crate) mod credentials;
pub(crate) mod local;
pub(crate) mod subscription;
// 「一个会话目录里该读哪个 variant 文件」的解析 + 按目录 mtime 的记忆。
// 从 local.rs 拆出：那是一份与账目结构无关的进程内状态，独立判据见该文件头。
pub(crate) mod variant_cache;
// OpenAI（ChatGPT 套餐）用量：凭据来自壳自己的 OAuth vault 而非内核模型
// 凭据链，且需代理路由，独立成模块以免撑大 subscription.rs（反棘轮）。
pub(crate) mod subscription_codex;
pub(crate) mod subscription_openai;
// 2api.store（TokenShop）中转：内核模型设置里的用户自定义 provider，用量端点
// 由该行的 baseURL 推导。与其余六家的「端点常量」形态不同，独立成模块。
pub(crate) mod subscription_api2store;
// opencode（厂商）Go 套餐的三窗口容量：端点与响应形状自成一类，同一半理由独立成模块
// （订阅层仍是「每家一个 query + parse 纯函数」的扩展位）。
pub(crate) mod subscription_opencode;
