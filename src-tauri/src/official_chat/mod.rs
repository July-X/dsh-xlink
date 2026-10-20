//! 官方对话（B 侧）取数的宿主侧实现。
//!
//! 本组只负责**回程**这一层：把「页面里跑完之后」的结果拿回宿主。
//! 站点适配（目录接口、正文解析、认证上下文）另立 `providers/`，与回程
//! 机制分开——**「回程失败」与「站点返回空」是两类错误**，混在一起就会
//! 把「没取到」显示成「没有内容」。
//!
//! 当前进度：P0 / G2。回程机制已实现，并由
//! `examples/official_chat_probe.rs` 在隔离的**可控页面**上实测了五种情形
//! （延迟成功 / Promise 拒绝 / 超时 / 导航 / 页面销毁）。站点侧接线尚未开始。
//!
//! G3 进展：读取上下文（官网窗口没开时也能读）与**零痕迹**页内求值
//! （DevTools 协议）已实现，见 [`context`] 与 [`cdp_eval`]。

// 预留 API：回程机制已实现并实测（G2），但**还没有生产调用方**——站点适配
// 属于 P4。按 lib.rs 记录的「预留 API」纪律，这里显式 allow(dead_code)，
// 等第一个站点适配器接上时把它删掉。
#[allow(dead_code)]
pub(crate) mod script_eval;

// G3：读取上下文（官网窗口没开时的读取通路）与零痕迹页内求值。
#[allow(dead_code)]
pub(crate) mod cdp_eval;

#[allow(dead_code)]
pub(crate) mod context;

// P1 协议核心的 **Rust 侧**（协议冻结稿 §6：同一组 JSON 夹具，Rust 与
// JavaScript 各跑一遍，断言「同一夹具 → 同一解析结果」）。
//
// 目前**没有生产调用方**——它先以「可执行的协议口径」存在：夹具一致性测试
// 就是它的作用。站点侧接线属 P4，所以这里按 lib.rs 的「预留 API」纪律
// allow(dead_code)，等第一个调用方接上时删掉。
#[allow(dead_code)]
pub(crate) mod protocol;
