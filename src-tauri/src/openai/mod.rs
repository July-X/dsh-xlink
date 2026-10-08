//! Rust OpenAI 服务（开发计划 §2/§5，P2 起步）。
//!
//! 本组最终覆盖 auth（OAuth 动态注册与刷新）、vault（加密凭据）、catalog
//! （账号模型目录）与 bridge（本地桥接）。P2 首块交付 [`bridge`]：面向
//! 本次 dsh 子进程的本地 HTTP 服务——`127.0.0.1` 随机端口、每次启动的
//! 高熵令牌经子进程环境传入（设计 §4：不是可配置上游的公共中转）。
//!
//! 目录数据当前是**桩**（空目录、`stub-0` revision）——真实账号目录是
//! P3 交付；桥接的形状（端点、鉴权、生命周期）先行落定，让 Host 插件
//! 侧的握手 / 目录链路可以先对真实服务验证。
pub(crate) mod auth;
pub(crate) mod bridge;
pub(crate) mod jwk;
