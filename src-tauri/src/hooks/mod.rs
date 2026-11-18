//! 可信局域网 Hook 与素材收件箱。
//!
//! 独立于内核桥的被动接收服务：默认关闭，显式选择可信网卡与来源范围，
//! HTTP 接收文本、参数与附件并可靠入库。管理接口仅在本地 IPC；不调用模型、
//! 不执行发送方内容、不抓取发送方 URL。设计契约见 `docs/features/hooks/`。
//!
//! 责任拆分：
//! - [`protocol`]：请求/响应结构与校验、幂等指纹、错误契约；
//! - [`store`]：SQLite schema、迁移与仓库函数（无网络、无 Tauri 依赖）；
//! - [`server`]：TcpListener、工作线程、鉴权、限流与三条 HTTP 路由；
//! - [`service`]：壳级单例运行时（配置、启停、状态）；
//! - [`cmd`]：管理面板的本地 IPC 命令；
//! - [`config`]：`shell/<mode>/hooks/config.json` 读写。
//! - [`analysis`]：Hook → DeepSeek 分析的材料快照、交接记录与工作台派发
//!   （不调用模型；设计契约见 `docs/features/hooks/deepseek-analysis-design.md`）。
//!
//! 安全边界（摘自设计契约）：发送方负责脱敏，接收端只负责结构安全——令牌只存
//! 哈希、日志不落正文、不读取发送方给出的路径或 URL、不把日志文本当指令。

pub(crate) mod analysis;
pub(crate) mod blob;
pub(crate) mod cmd;
pub(crate) mod config;
pub(crate) mod media;
pub(crate) mod network;
pub(crate) mod protocol;
pub(crate) mod server;
pub(crate) mod service;
pub(crate) mod store;
