//! 局域网 Hook 与素材收件箱（P1：回环接收器 + 纯文本入箱）。
//!
//! 设计契约与阶段计划见 `docs/features/hooks/`。本模块是独立于内核桥的壳级服务：
//! 默认关闭，启用后只绑回环地址，收下外部系统发来的事件并可靠入库，供后续业务
//! 界面选择素材。P1 只实现纯文本与结构化参数；附件、局域网与模型处理分别属于
//! P2/P4 之后的阶段，**不要在这个模块里调用模型或执行发送方给的内容**。
//!
//! 责任拆分：
//! - [`protocol`]：请求/响应结构与校验、幂等指纹、错误契约；
//! - [`store`]：SQLite schema、迁移与仓库函数（无网络、无 Tauri 依赖）；
//! - [`server`]：TcpListener、工作线程、鉴权、限流与三条 HTTP 路由；
//! - [`service`]：壳级单例运行时（配置、启停、状态）；
//! - [`cmd`]：管理面板的本地 IPC 命令；
//! - [`config`]：`shell/<mode>/hooks/config.json` 读写。
//!
//! 安全边界（摘自设计契约）：发送方负责脱敏，接收端只负责结构安全——令牌只存
//! 哈希、日志不落正文、不读取发送方给出的路径或 URL、不把日志文本当指令。

pub(crate) mod blob;
pub(crate) mod cmd;
pub(crate) mod config;
pub(crate) mod media;
pub(crate) mod protocol;
pub(crate) mod server;
pub(crate) mod service;
pub(crate) mod store;
