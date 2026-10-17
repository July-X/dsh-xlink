//! 外壳 JSON 状态文档的读写骨架：插件/技能清单、补丁记录、隔离记录都走这里。
//!
//! 每个模块原先各抄一份 `Loaded / Missing / Corrupt` 三态 match（读两份、
//! 警告一份）和一份 pretty-JSON + `atomic_write` 的写侧。骨架相同、文案不同，
//! 于是**同一类数据丢失风险要在一个新模块里重新讲一遍**。
//!
//! 核心约定只有一条，由 [`load_checked`] 与 [`load_lossy`] 的分工强制：
//! 「文件不存在」是正常首次运行，「损坏」不是。把损坏退化成空文档，会在下一次
//! 写入时把用户真实记录覆盖成空——插件清单变空 → 内核里的物化目录被当孤儿清掉；
//! 技能清单变空 → 已装技能的链接被逐个删除；补丁记录变空 → 打过补丁的内核变成
//! 「没打过」，既撤不掉也重装不了。展示路径用 [`load_lossy`]（至少让面板还能
//! 打开），所有"读-改-写"路径必须用 [`load_checked`]。
//!
//! **唯一的例外是 [`load_rebuildable`]**：整份文档都能从别处重算出来时（当前只有
//! 模型用量账目——它由 sessions 目录里的帧重算），「损坏」只等于「从零重扫一次」，
//! 拿空文档覆盖没有任何东西可丢。这类文档必须**同时**走 [`save_timed_light`]（不为
//! 它付两次 fsync），否则省下的写入成本会在别处加倍还回去。

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::shell::error::AppError;
use crate::shell::process::{atomic_write, read_state_file, StateRead};

/// 状态文档的上下文：损坏时怎么措辞、归到哪一类错误。
///
/// 这两件事必须留在调用方——「插件清单」与「补丁记录」损坏的后果和下一步
/// 完全不同，共享层不该替它们编文案。骨架共用、文案各自所有，是这里刻意的
/// 切分线。
#[derive(Clone, Copy)]
pub struct StateCtx {
    /// 把解析失败原因渲染成用户可见文案（含可操作的下一步）。
    pub corrupt: fn(reason: &str) -> String,
    /// 损坏时抛出的错误分类。
    pub kind: fn(String) -> AppError,
}

impl StateCtx {
    /// 只有写路径、或读路径刻意容错的模块用这个：损坏原因原样透出。
    pub const fn plain(kind: fn(String) -> AppError) -> Self {
        Self {
            corrupt: |reason| reason.to_string(),
            kind,
        }
    }
}

/// 展示路径用的容错读取：文件不存在或损坏都返回空文档。
///
/// **只读**：任何"读-改-写"路径都必须用 [`load_checked`]。
pub fn load_lossy<T: DeserializeOwned + Default>(path: &Path) -> T {
    match read_state_file(path) {
        StateRead::Loaded(value) => value,
        StateRead::Missing | StateRead::Corrupt { .. } => T::default(),
    }
}

/// 读-改-写路径用的读取：文档损坏时返回可操作的错误，而不是拿空文档去覆盖
/// 用户的真实记录。
pub fn load_checked<T: DeserializeOwned + Default>(
    path: &Path,
    ctx: StateCtx,
) -> Result<T, AppError> {
    match read_state_file(path) {
        StateRead::Loaded(value) => Ok(value),
        StateRead::Missing => Ok(T::default()),
        StateRead::Corrupt { reason } => Err((ctx.kind)((ctx.corrupt)(&reason))),
    }
}

/// **可重建**派生文档的读取：与 [`load_lossy`] 一样容错，但把损坏原因交回调用方。
///
/// 两个用法的差别只在「原因要不要」：展示路径不关心，而 `usage_view` 要把它
/// 记进壳日志——账目被静默重置、历史统计从头重扫，是用户看得见的后果，得留下
/// 一行可查的记录（见 `shell_events`）。
///
/// 用它的前提**必须是**「整份文档都能重算」：能重算才敢拿空文档覆盖磁盘上那份。
/// 不能重算的（插件 / 技能清单、补丁记录、隔离记录）继续走 [`load_checked`]。
pub fn load_rebuildable<T: DeserializeOwned + Default>(path: &Path) -> (T, Option<String>) {
    match read_state_file(path) {
        StateRead::Loaded(value) => (value, None),
        StateRead::Missing => (T::default(), None),
        StateRead::Corrupt { reason } => (T::default(), Some(reason)),
    }
}

/// 文档读不出来时的说明，供设置页/概览横幅展示；文件正常或不存在时返回
/// `None`（不存在不是故障）。
pub fn integrity_warning<T: DeserializeOwned>(path: &Path, ctx: StateCtx) -> Option<String> {
    match read_state_file::<T>(path) {
        StateRead::Corrupt { reason } => Some((ctx.corrupt)(&reason)),
        StateRead::Loaded(_) | StateRead::Missing => None,
    }
}

/// 把文档写回磁盘：建目录 → pretty JSON（补一个末尾换行）→ `atomic_write`
/// （同目录临时文件 + `sync_all` + rename）。进程中断时读者只会看到旧的完整
/// 文件或新的完整文件，不会读到截断的 JSON。
pub fn save<T: Serialize>(path: &Path, value: &T, ctx: StateCtx) -> Result<(), AppError> {
    save_timed(path, value, ctx).map(|_| ())
}

/// [`save`] 的分段计时变体：返回「序列化」与「落盘」各花多久，落盘再拆成
/// 数据 fsync / rename / 目录 fsync。给用量账目那条热路径用——它是被
/// [`crate::usage::local::refresh_doc`] 反复调的那个 `save`，其余调用方
/// 继续用 [`save`]。
pub fn save_timed<T: Serialize>(
    path: &Path,
    value: &T,
    ctx: StateCtx,
) -> Result<crate::shell::atomic_write::WriteTimings, AppError> {
    save_with(
        path,
        value,
        ctx,
        crate::shell::atomic_write::atomic_write_timed,
    )
}

/// [`save_timed`] 的**不 fsync** 变体：给可重建的派生账目用（见 [`load_rebuildable`]）。
///
/// 与 [`save_timed`] 只差落盘那一步：写临时文件 + rename，不 `sync_all`、也不
/// `sync_all` 父目录。**调用方必须配 [`load_rebuildable`]**——这条路径换来的是
/// 每次落盘省掉两次 fsync（2026-10-17 实测 141KB 账目上 p50 21ms + 19ms），
/// 代价是掉电时可能留下半截文件，得由读取侧当成「从零重扫」接住。
pub fn save_timed_light<T: Serialize>(
    path: &Path,
    value: &T,
    ctx: StateCtx,
) -> Result<crate::shell::atomic_write::WriteTimings, AppError> {
    save_with(
        path,
        value,
        ctx,
        crate::shell::atomic_write::atomic_write_light,
    )
}

/// 两个 `save_timed*` 共用的那一半：建目录 → 序列化 → 交给给定的原子写入口。
///
/// 落盘入口作为函数指针传进来而不是复制两遍「序列化 + 建目录 + 错误文案」：
/// 两条路的差别**只在 fsync**，其余一字不差；复制第二份时任何一处文案改动
/// 都会漂成两套（本仓的门禁也会把它算成重复区间）。
fn save_with<T: Serialize>(
    path: &Path,
    value: &T,
    ctx: StateCtx,
    write: fn(&Path, &[u8]) -> std::io::Result<crate::shell::atomic_write::WriteTimings>,
) -> Result<crate::shell::atomic_write::WriteTimings, AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| (ctx.kind)(format!("无法创建目录 {}：{e}", parent.display())))?;
    }
    let started = std::time::Instant::now();
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| (ctx.kind)(format!("序列化状态失败：{e}")))?;
    let serialize_us = started.elapsed().as_micros();
    let mut timings = write(path, format!("{text}\n").as_bytes())
        .map_err(|e| (ctx.kind)(format!("无法写入 {}：{e}", path.display())))?;
    timings.serialize_us = serialize_us;
    Ok(timings)
}

/// 写侧的"尽力而为"变体：只记录失败，不打断调用方。
///
/// 用于故障报告本身（`last-incident.json`）——写不进去不能掩盖用户正在等待的
/// 启动结果。
pub fn save_best_effort<T: Serialize>(path: &Path, value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = atomic_write(path, format!("{text}\n").as_bytes());
    }
}
