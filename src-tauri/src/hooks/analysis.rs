//! Hook → DeepSeek 分析:材料快照、交接记录与工作台派发。
//!
//! 设计契约:docs/features/hooks/deepseek-analysis-design.md。阶段 A 已对
//! 0.2.2-rc.4 实包核实:内核原生支持 `workspaces.create({path})`、
//! `uiWorkspace.connectWorkspace` 与草稿初始化
//! `conversation.input.requestDraftInitialization(binding,{prompt,whenDraftExists})`,
//! 不需要改实例 cwd。本模块只准备材料与草稿,**不调用模型**:唯一的模型调用
//! 是用户在工作台输入框显式发送。
//!
//! - 材料包:`shell/<mode>/hooks/analysis/packages/<handoff_id>/`。从权威收件箱
//!   (SQLite + blob 存储)读取并核对大小与 SHA-256 后,在同盘临时目录完成、原子
//!   发布;临时目录带归属标记,崩溃残留只清本模块自己的未发布目录。
//! - 交接记录:`handoffs.json`,不含令牌或凭据;同一 `request_id` 幂等,重复点击
//!   或响应丢失复用同一交接与材料包。
//! - 派发:经既有 open_harness / start_kernel 流程把目标注入工作台;内核关闭时
//!   受管启动,启动本身不调用模型。迟到报告按 sequence CAS,不污染新交接。
//!
//! shortcut: 材料以 Markdown 引用块进入草稿文本(内核 requestDraftInitialization
//! 的唯一跨插件内容通道),不是可折叠 quote chip;insertQuote 面待实机验证后升级。
//! shortcut: 「更改」工作空间用受校验的路径输入,不弹原生目录选择框(壳无现成选择器)。

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, WebviewWindow};

/// 单次交接材料合计上限(UTF-8 文本字节)。超限拒绝,不静默截断。
pub(crate) const INPUT_LIMIT_BYTES: u64 = 1024 * 1024;
/// 已发布材料包总预算。是资源上限,不是模型 token 容量保证。
pub(crate) const TOTAL_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;
/// 未被工作台确认的派发目标保留时长;超时按失败落账,可重试。
const PENDING_TTL: Duration = Duration::from_secs(600);
/// 会话列表桥的应答窗口;超时按「工作台未就绪」如实上报。
const SESSIONS_BRIDGE_TIMEOUT: Duration = Duration::from_secs(6);
/// 新建会话的预填问题;只在草稿为空时写入,草稿已有内容不拼接。
const QUESTION_SUGGESTION: &str =
    "分析这次构建的结果、警告和潜在问题，列出日志依据与建议排查步骤。";
/// 临时包归属标记;清理只认这个标记,不动别的目录。
const TEMP_MARKER: &str = ".xlink-analysis.json";
/// 崩溃残留的临时目录清理门槛。
const TEMP_STALE: Duration = Duration::from_secs(3600);
/// 材料引用块的可识别标记;工作台恢复时用它避免重复注入。
fn material_marker(handoff_id: &str) -> String {
    format!("【Hook 材料 {handoff_id}")
}

// ---------------------------------------------------------------------------
// 路径解析(全部经 paths::*,随 DSH_XLINK_HOME,不拼用户目录)
// ---------------------------------------------------------------------------

fn analysis_dir() -> PathBuf {
    crate::shell::paths::hooks_dir(crate::shell::settings::current_mode()).join("analysis")
}

fn workspace_dir() -> PathBuf {
    analysis_dir().join("workspace")
}

fn packages_dir() -> PathBuf {
    analysis_dir().join("packages")
}

fn temp_packages_dir() -> PathBuf {
    packages_dir().join(".tmp")
}

fn handoffs_file() -> PathBuf {
    analysis_dir().join("handoffs.json")
}

// ---------------------------------------------------------------------------
// 交接记录
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct HandoffRecord {
    pub handoff_id: String,
    pub request_id: String,
    pub created_at: i64,
    pub event_id: String,
    pub event_title: String,
    pub endpoint_name: String,
    pub external_id: String,
    pub family: String,
    pub instance: String,
    pub profile: String,
    pub shell_mode: String,
    pub workspace_path: String,
    pub session_mode: String,
    pub session_title: String,
    pub session_id: Option<String>,
    /// prepared:包已落盘;attached:工作台已确认草稿就位;failed/unavailable 如实。
    /// 不宣称 sent:发送事件只有用户与内核知道,壳不猜测。
    pub state: String,
    pub failure: Option<String>,
    pub prompt_bytes: u64,
}

impl HandoffRecord {
    fn state_label(&self) -> &'static str {
        match self.state.as_str() {
            "prepared" => "草稿未发送",
            "attached" => "草稿已就位",
            "failed" => "交接失败",
            "unavailable" => "原会话不可用",
            _ => "未知",
        }
    }
}

fn load_records() -> Result<Vec<HandoffRecord>, String> {
    let path = handoffs_file();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取交接记录失败：{error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("交接记录 {path:?} 损坏：{error}；请删除该文件后重试"))
}

fn save_records(records: &[HandoffRecord]) -> Result<(), String> {
    if let Some(parent) = handoffs_file().parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建分析数据目录失败：{error}"))?;
    }
    let bytes = serde_json::to_vec_pretty(records)
        .map_err(|error| format!("序列化交接记录失败：{error}"))?;
    crate::shell::atomic_write::atomic_write(&handoffs_file(), &bytes)
        .map_err(|error| format!("写入交接记录失败：{error}"))
}

/// 按当前壳解析交接目标(family / 实例 / profile / 壳模式)。
/// 生产调用方一律走 resolve_default,不硬编码实例 id。
fn current_target_env() -> (String, String, String, String) {
    let (family, instance) = crate::shell::instance::resolve_default();
    let profile = crate::shell::instance::load_registry()
        .ok()
        .and_then(|registry| {
            registry
                .instances
                .iter()
                .find(|record| record.id == instance)
                .map(|record| record.profile.clone())
        })
        .unwrap_or_else(|| "web".to_string());
    (
        family.to_string(),
        instance.to_string(),
        profile,
        crate::shell::settings::current_mode().as_str().to_string(),
    )
}

/// 交接目标仍属于当前实例与壳模式才允许派发/确认(A16/A17)。
fn target_compatible(record: &HandoffRecord) -> bool {
    let (family, instance, _profile, mode) = current_target_env();
    record.family == family && record.instance == instance && record.shell_mode == mode
}

// ---------------------------------------------------------------------------
// 材料包
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MaterialEntry {
    /// body | log。
    pub kind: String,
    /// body 或附件 ID;包内文件名由它生成,不用发送方文件名。
    pub internal_id: String,
    pub display_name: String,
    pub source_label: String,
    pub external_id: String,
    pub snapshot_at: i64,
    pub sha256: String,
    pub bytes: u64,
    /// 原文件 1 起闭区间行号;完整日志为空。
    pub line_range: Option<(u64, u64)>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageManifest {
    schema_version: u64,
    handoff_id: String,
    created_at: i64,
    event_id: String,
    event_title: String,
    materials: Vec<MaterialEntry>,
}

fn package_dir(handoff_id: &str) -> PathBuf {
    packages_dir().join(handoff_id)
}

fn material_file_name(internal_id: &str) -> String {
    format!("m-{internal_id}.txt")
}

/// 递归求目录内普通文件总字节(深度上限防 symlink 环)。
fn dir_size(path: &Path, depth: u8) -> u64 {
    if depth == 0 {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                total += dir_size(&entry.path(), depth - 1);
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

/// 清理本模块带归属标记的过期临时包;已发布包与无关目录不动。
fn sweep_stale_temp_packages() {
    let dir = temp_packages_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join(TEMP_MARKER).is_file() {
            continue;
        }
        let stale = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > TEMP_STALE);
        if stale {
            let _ = fs::remove_dir_all(&path);
            crate::shell::shell_events::record(
                "hook-analysis-package",
                &format!("清理崩溃残留的未发布临时包 {}", path.display()),
            );
        }
    }
}

struct MaterialContent {
    entry: MaterialEntry,
    /// 按 UTF-8 验证过的材料字节(与包内文件逐字节相同)。
    bytes: Vec<u8>,
}

/// 打开权威收件箱读事件;库不存在时如实报错。
fn open_store() -> Result<Connection, String> {
    crate::hooks::store::open_if_exists(&crate::hooks::service::db_path())
        .map_err(|error| format!("打开 Hook 收件箱失败：{error}"))?
        .ok_or_else(|| "收件箱还没有任何事件，无法准备分析材料".to_string())
}

/// 读取一个日志附件并核对大小与 SHA-256,再按需切出行段。
/// 读取期间事件若被并发清理,核对失败按明确错误返回,不发布半包。
fn read_log_material(
    attachment: &crate::hooks::store::AttachmentRow,
    line_range: Option<(u64, u64)>,
) -> Result<Vec<u8>, String> {
    let rel = attachment.storage_rel.as_deref().ok_or_else(|| {
        format!(
            "日志 {} 没有归档路径，请刷新收件箱后重试",
            attachment.filename
        )
    })?;
    let path = crate::hooks::blob::path_for_rel(rel)
        .map_err(|error| format!("日志 {} 的存储路径非法：{error}", attachment.filename))?;
    let meta = fs::symlink_metadata(&path).map_err(|error| {
        format!(
            "无法读取日志 {}：{error}（可能已被清理）",
            attachment.filename
        )
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!(
            "日志 {} 的存储路径不是普通文件，已拒绝读取",
            attachment.filename
        ));
    }
    if attachment.size_bytes >= 0 && attachment.size_bytes as u64 != meta.len() {
        return Err(format!(
            "日志 {} 与收件箱记录的大小不一致，可能已被外部修改；请刷新收件箱后重试",
            attachment.filename
        ));
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("无法读取日志 {}：{error}", attachment.filename))?;
    if let Some(expected) = attachment.sha256.as_deref() {
        let actual = crate::shell::digest::hex(Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "日志 {} 与收件箱记录的 SHA-256 不一致，可能已被外部修改；请刷新收件箱后重试",
                attachment.filename
            ));
        }
    }
    match line_range {
        None => Ok(bytes),
        Some((start, end)) => extract_lines(&bytes, start, end, &attachment.filename),
    }
}

/// 切出原文件 1 起闭区间行段;越界的行号按实际行数收敛并如实标注。
fn extract_lines(bytes: &[u8], start: u64, end: u64, filename: &str) -> Result<Vec<u8>, String> {
    if start == 0 || end < start {
        return Err("行段格式非法：需要 1 起的起止行号，且结束行不小于起始行".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| format!("日志 {filename} 不是有效 UTF-8 文本，本期不支持分析"))?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len() as u64;
    if start > total {
        return Err(format!(
            "日志 {filename} 只有 {total} 行，所选起始行 {start} 越界，请调整行号"
        ));
    }
    let from = (start - 1) as usize;
    let to = end.min(total) as usize;
    if from >= to {
        return Err(format!(
            "日志 {filename} 只有 {total} 行，所选行段为空，请调整行号"
        ));
    }
    Ok(lines[from..to].concat().into_bytes())
}

/// 校验并组装全部材料;返回材料内容与总字节数(超限拒绝)。
/// 附件行由调用方从权威收件箱取好传入,这里不再开第二个连接。
fn collect_materials(
    event: &crate::hooks::store::EventRow,
    attachments: &[crate::hooks::store::AttachmentRow],
    include_body: bool,
    logs: &[LogSelection],
    now: i64,
) -> Result<(Vec<MaterialContent>, u64), String> {
    let mut materials: Vec<MaterialContent> = Vec::new();
    let mut total: u64 = 0;
    #[allow(clippy::redundant_closure_call)]
    let mut add = |content: MaterialContent, total: &mut u64| -> Result<(), String> {
        *total += content.bytes.len() as u64;
        if *total > INPUT_LIMIT_BYTES {
            return Err(format!(
                "所选材料合计 {total} 字节，超过单次 {} 字节上限：请改用日志行段或减少材料，不会静默截断",
                INPUT_LIMIT_BYTES
            ));
        }
        materials.push(content);
        Ok(())
    };
    if include_body {
        if event.body_text.is_empty() {
            return Err("这条事件没有正文，请在材料里至少选择一个日志".into());
        }
        let bytes = event.body_text.as_bytes().to_vec();
        let entry = MaterialEntry {
            kind: "body".into(),
            internal_id: "body".into(),
            display_name: "正文".into(),
            source_label: event.endpoint_name.clone(),
            external_id: event.external_id.clone(),
            snapshot_at: now,
            sha256: crate::shell::digest::hex(Sha256::digest(&bytes)),
            bytes: bytes.len() as u64,
            line_range: None,
            complete: true,
        };
        add(MaterialContent { entry, bytes }, &mut total)?;
    }
    for selection in logs {
        let attachment = attachments
            .iter()
            .find(|row| row.id == selection.attachment_id)
            .ok_or_else(|| "所选日志不存在（可能已被清理），请刷新收件箱后重试".to_string())?;
        if attachment.kind != crate::hooks::protocol::KIND_LOG {
            return Err(format!(
                "{} 不是日志附件，不能按日志分析",
                attachment.filename
            ));
        }
        if attachment.state != crate::hooks::store::ATTACHMENT_STORED {
            return Err(format!(
                "日志 {} 尚未归档（当前状态：{}），等接收完成后再试",
                attachment.filename, attachment.state
            ));
        }
        let line_range = match (selection.start_line, selection.end_line) {
            (Some(start), Some(end)) => Some((start, end)),
            (None, None) => None,
            _ => return Err("行段需要成对提供起始与结束行号".into()),
        };
        let bytes = read_log_material(attachment, line_range)?;
        std::str::from_utf8(&bytes).map_err(|_| {
            format!(
                "日志 {} 所选内容不是有效 UTF-8 文本，本期不支持分析",
                attachment.filename
            )
        })?;
        let entry = MaterialEntry {
            kind: "log".into(),
            internal_id: attachment.id.clone(),
            display_name: attachment.filename.clone(),
            source_label: event.endpoint_name.clone(),
            external_id: event.external_id.clone(),
            snapshot_at: now,
            sha256: crate::shell::digest::hex(Sha256::digest(&bytes)),
            bytes: bytes.len() as u64,
            line_range,
            complete: line_range.is_none(),
        };
        add(MaterialContent { entry, bytes }, &mut total)?;
    }
    if materials.is_empty() {
        return Err("至少选择一项材料（正文或日志）".into());
    }
    Ok((materials, total))
}

/// 同盘临时目录完成、逐文件 fsync、原子发布;发布前核对总预算。
fn build_package(
    handoff_id: &str,
    manifest: &PackageManifest,
    materials: &[MaterialContent],
) -> Result<(), String> {
    fs::create_dir_all(packages_dir()).map_err(|error| format!("创建材料包目录失败：{error}"))?;
    let incoming: u64 = materials.iter().map(|item| item.bytes.len() as u64).sum();
    ensure_budget(&packages_dir(), incoming, TOTAL_BUDGET_BYTES)?;
    let temp_root = temp_packages_dir();
    fs::create_dir_all(&temp_root).map_err(|error| format!("创建材料临时目录失败：{error}"))?;
    let temp = temp_root.join(format!("{handoff_id}-{:016x}", rand::random::<u64>()));
    fs::create_dir_all(&temp).map_err(|error| format!("创建临时材料包失败：{error}"))?;
    let mark = serde_json::json!({ "owner": "hook-analysis", "handoffId": handoff_id });
    if let Err(error) = write_file_synced(&temp.join(TEMP_MARKER), mark.to_string().as_bytes()) {
        let _ = fs::remove_dir_all(&temp);
        return Err(format!("写材料包归属标记失败：{error}"));
    }
    for material in materials {
        if !valid_segment(&material.entry.internal_id) {
            let _ = fs::remove_dir_all(&temp);
            return Err("材料内部 ID 含非法字符".into());
        }
        let path = temp.join(material_file_name(&material.entry.internal_id));
        if let Err(error) = write_file_synced(&path, &material.bytes) {
            let _ = fs::remove_dir_all(&temp);
            return Err(format!("写材料快照失败：{error}"));
        }
    }
    let manifest_bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("序列化材料清单失败：{error}"))?;
    if let Err(error) = write_file_synced(&temp.join("manifest.json"), &manifest_bytes) {
        let _ = fs::remove_dir_all(&temp);
        return Err(format!("写材料清单失败：{error}"));
    }
    let target = package_dir(handoff_id);
    if target.exists() {
        let _ = fs::remove_dir_all(&temp);
        return Err("材料包目录已存在，请重试".into());
    }
    fs::rename(&temp, &target).map_err(|error| {
        let _ = fs::remove_dir_all(&temp);
        format!("发布材料包失败：{error}。请检查数据目录权限与磁盘空间后重试")
    })?;
    sync_dir(target.parent().unwrap_or(&packages_dir()));
    Ok(())
}

fn ensure_budget(packages: &Path, incoming: u64, budget: u64) -> Result<(), String> {
    let existing = dir_size(packages, 3);
    if existing + incoming > budget {
        return Err(format!(
            "分析材料包总预算已满（现有 {existing} 字节 + 本次 {incoming} 字节 > 上限 {budget} 字节）。             请在「查看分析」里删除不再需要的旧材料后重试；内核会话不受删除影响"
        ));
    }
    Ok(())
}

fn write_file_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(unix)]
fn sync_dir(path: &Path) {
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(dir) = fs::OpenOptions::new().mode(0o700).open(path) {
        let _ = dir.sync_all();
    }
}

#[cfg(windows)]
fn sync_dir(_path: &Path) {}

/// 与 blob 存储同款路径段规则;handoff_id 与内部 ID 都按它把关。
fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 64
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

// ---------------------------------------------------------------------------
// 草稿文本组装(材料以 Markdown 引用块进入唯一的问题输入框)
// ---------------------------------------------------------------------------

fn quote_block(header: &str, content: &str) -> String {
    let mut out = String::new();
    for line in std::iter::once(header).chain(content.lines()) {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn material_header(entry: &MaterialEntry, handoff_id: &str) -> String {
    let scope = if entry.kind == "body" {
        "正文"
    } else {
        "日志"
    };
    let mut header = format!(
        "{} · {}】来源 {} · 事件 {} · 快照 {}",
        material_marker(handoff_id),
        if entry.kind == "body" {
            scope.to_string()
        } else {
            format!("{scope} {}", entry.display_name)
        },
        entry.source_label,
        entry.external_id,
        time_label(entry.snapshot_at),
    );
    if entry.kind == "log" {
        if entry.complete {
            header.push_str(&format!(" · 完整内容 · {} 字节", entry.bytes));
        } else if let Some((start, end)) = entry.line_range {
            header.push_str(&format!(
                " · 原文件第 {start}-{end} 行 · 非完整日志 · {} 字节",
                entry.bytes
            ));
        }
    }
    header
}

/// 本地日期 + 时刻;时区取不到时按模块约定退 UTC,不编假时间。
fn time_label(epoch_ms: i64) -> String {
    let at = std::time::SystemTime::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(epoch_ms.max(0) as u64))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    format!(
        "{} {}",
        crate::shell::localtime::local_date_string(at),
        crate::shell::localtime::local_clock_string(at)
    )
}

/// 从已发布材料包重建 Markdown 引用块;恢复与派发共用同一份事实。
fn material_blocks(handoff_id: &str) -> Result<String, String> {
    let dir = package_dir(handoff_id);
    let manifest: PackageManifest = serde_json::from_slice(
        &fs::read(dir.join("manifest.json"))
            .map_err(|_| format!("材料包 {handoff_id} 的清单缺失，可能已被删除"))?,
    )
    .map_err(|error| format!("材料包 {handoff_id} 的清单损坏：{error}"))?;
    let mut blocks = Vec::new();
    for entry in &manifest.materials {
        let bytes = fs::read(dir.join(material_file_name(&entry.internal_id)))
            .map_err(|_| format!("材料包 {handoff_id} 缺 {} 的快照文件", entry.display_name))?;
        let content = String::from_utf8(bytes)
            .map_err(|_| format!("材料 {} 不再是有效 UTF-8 文本", entry.display_name))?;
        blocks.push(quote_block(&material_header(entry, handoff_id), &content));
    }
    Ok(blocks.join(
        "
",
    ))
}

// ---------------------------------------------------------------------------
// 派发队列:同一时刻一个目标,sequence CAS 挡迟到响应
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisTarget {
    pub sequence: u64,
    pub handoff_id: String,
    /// prepare-new | prepare-existing | reopen
    pub action: String,
    pub family: String,
    pub instance: String,
    pub shell_mode: String,
    /// prepare-new:工作台按路径找/建工作空间。
    pub workspace_path: Option<String>,
    /// prepare-new:新会话标题。
    pub session_title: Option<String>,
    /// prepare-existing / reopen:目标会话。
    pub session_id: Option<String>,
    /// prepare-new 且草稿为空时预填的问题;草稿已有内容不拼接。
    pub question: Option<String>,
    /// 材料引用块;prepare-existing / reopen 只含材料。
    pub materials: String,
}

struct PendingTarget {
    target: AnalysisTarget,
    since: Instant,
}

#[derive(Default)]
struct AnalysisQueue {
    sequence: u64,
    pending: Option<PendingTarget>,
    sessions: Option<PendingSessions>,
}

#[derive(Default)]
struct PendingSessions {
    query_id: String,
    items: Option<Vec<SessionItem>>,
}

fn queue() -> &'static Mutex<AnalysisQueue> {
    static QUEUE: OnceLock<Mutex<AnalysisQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(AnalysisQueue::default()))
}

/// 登记待派发目标并分配 sequence。
fn enqueue_target(queue: &mut AnalysisQueue, mut target: AnalysisTarget) -> AnalysisTarget {
    queue.sequence += 1;
    target.sequence = queue.sequence;
    queue.pending = Some(PendingTarget {
        target: target.clone(),
        since: Instant::now(),
    });
    target
}

/// 目标超时未确认按失败落账(后台动作必须留痕)。
fn expire_stale_pending(queue: &mut AnalysisQueue) {
    let stale = queue
        .pending
        .as_ref()
        .is_some_and(|p| p.since.elapsed() > PENDING_TTL);
    if stale {
        if let Some(pending) = queue.pending.take() {
            let reason = "工作台长时间未确认交接；请确认工作台可用后重试，材料包与记录保留";
            mark_record_failed(&pending.target.handoff_id, reason.into());
        }
    }
}

fn mark_record_failed(handoff_id: &str, reason: String) {
    let result: Result<(), String> = (|| {
        let mut records = load_records()?;
        if let Some(record) = records
            .iter_mut()
            .find(|record| record.handoff_id == handoff_id)
        {
            record.state = "failed".into();
            record.failure = Some(reason.chars().take(512).collect());
            save_records(&records)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            crate::shell::shell_events::record("hook-analysis-dispatch", &reason);
        }
        Err(error) => {
            crate::shell::shell_events::record(
                "hook-analysis-dispatch",
                &format!("{reason}；更新记录失败：{error}"),
            );
        }
    }
}

fn dispatch(app: &AppHandle, target: AnalysisTarget) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let handoff_id = target.handoff_id.clone();
        if !crate::kernel_running(&handle) {
            // 内核关闭:走既有受管启动(不调用模型),进度落日志,失败保留记录可重试。
            let data_dir = handle.state::<crate::AppState>().data_dir.clone();
            let mut send = |message: &str| {
                crate::shell::shell_events::record("hook-analysis-start", message);
            };
            match crate::commands::start_kernel_blocking(&handle, &data_dir, &mut send) {
                Ok(report) if report.running => {}
                Ok(_report) => {
                    return mark_record_failed(
                        &handoff_id,
                        "内核未能启动，分析未交付：请在概览页查看启动诊断后重试；材料包与记录保留"
                            .into(),
                    );
                }
                Err(error) => {
                    return mark_record_failed(
                        &handoff_id,
                        format!("内核启动失败，分析未交付：{error}；请在概览页排查后重试"),
                    );
                }
            }
        }
        if let Err(error) = crate::commands::open_harness(handle.clone()).await {
            return mark_record_failed(&handoff_id, format!("打开工作台失败：{error}"));
        }
        // 迟到派发不污染新选择:sequence 已被更新的目标取代时不再注入。
        let current = crate::lock(queue())
            .pending
            .as_ref()
            .map(|pending| pending.target.sequence);
        if current != Some(target.sequence) {
            return;
        }
        let Some(window) = handle.get_webview_window("harness") else {
            return mark_record_failed(&handoff_id, "工作台窗口未打开".into());
        };

        crate::notify::activate::force_foreground(&window);
        match serde_json::to_string(&target) {
            Ok(json) => {
                if let Err(error) = window.eval(format!(
                    "window.dispatchEvent(new CustomEvent('dsh-xlink-hook-analysis',{{detail:{json}}}));"
                )) {
                    mark_record_failed(&handoff_id, format!("注入交接目标失败：{error}"));
                }
            }
            Err(error) => mark_record_failed(&handoff_id, format!("序列化交接目标失败：{error}")),
        }
    });
}

// ---------------------------------------------------------------------------
// 面板命令
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanMaterial {
    pub id: String,
    /// body | log | image | 其它附件类型。
    pub kind: String,
    pub filename: String,
    pub size_bytes: u64,
    pub state: String,
    pub selectable: bool,
    pub note: String,
    /// 256 字节损失预览;完整内容仍走详情页受控预览。
    pub head: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffSummary {
    pub handoff_id: String,
    pub session_title: String,
    pub session_id: Option<String>,
    pub state: String,
    pub state_label: String,
    pub created_at: i64,
    pub failure: Option<String>,
    pub workspace_path: String,
    pub prompt_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanView {
    pub default_workspace_path: String,
    pub instance_label: String,
    pub input_limit_bytes: u64,
    pub total_budget_bytes: u64,
    pub materials: Vec<PlanMaterial>,
    pub handoffs: Vec<HandoffSummary>,
    pub question_suggestion: String,
}

fn plan_summary(record: &HandoffRecord) -> HandoffSummary {
    HandoffSummary {
        handoff_id: record.handoff_id.clone(),
        session_title: record.session_title.clone(),
        session_id: record.session_id.clone(),
        state: record.state.clone(),
        state_label: record.state_label().to_string(),
        created_at: record.created_at,
        failure: record.failure.clone(),
        workspace_path: record.workspace_path.clone(),
        prompt_bytes: record.prompt_bytes,
    }
}

/// 材料头部预览:只读前 256 字节,不整块搬运大文件。
fn head_preview(path: &Path, limit: u64) -> Option<String> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; limit as usize];
    let read = file.read(&mut buffer).unwrap_or(0);
    buffer.truncate(read);
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

fn plan_materials(
    event: &crate::hooks::store::EventRow,
    attachments: &[crate::hooks::store::AttachmentRow],
) -> Vec<PlanMaterial> {
    let mut rows = Vec::new();
    if !event.body_text.is_empty() {
        rows.push(PlanMaterial {
            id: "body".into(),
            kind: "body".into(),
            filename: "正文".into(),
            size_bytes: event.body_text.len() as u64,
            state: "stored".into(),
            selectable: event.state == "ready",
            note: if event.state == "ready" {
                String::new()
            } else {
                "事件当前状态不可分析".into()
            },
            head: Some(event.body_text.chars().take(200).collect()),
        });
    }
    for attachment in attachments {
        let (selectable, note) = if attachment.kind == crate::hooks::protocol::KIND_LOG {
            if attachment.state == crate::hooks::store::ATTACHMENT_STORED {
                (true, String::new())
            } else {
                (
                    false,
                    format!("日志尚未归档（{}），等接收完成后再选", attachment.state),
                )
            }
        } else if attachment.kind == crate::hooks::protocol::KIND_IMAGE {
            (false, "图片附件本期不支持分析".into())
        } else {
            (
                false,
                format!("该附件类型（{}）不支持分析", attachment.kind),
            )
        };
        let head = (attachment.kind == crate::hooks::protocol::KIND_LOG
            && attachment.state == crate::hooks::store::ATTACHMENT_STORED)
            .then(|| {
                attachment
                    .storage_rel
                    .as_deref()
                    .and_then(|rel| crate::hooks::blob::path_for_rel(rel).ok())
                    .and_then(|path| head_preview(&path, 256))
            })
            .flatten();
        rows.push(PlanMaterial {
            id: attachment.id.clone(),
            kind: attachment.kind.clone(),
            filename: attachment.filename.clone(),
            size_bytes: attachment.size_bytes.max(0) as u64,
            state: attachment.state.clone(),
            selectable,
            note,
            head,
        });
    }
    rows
}

#[tauri::command]
pub(crate) async fn hooks_analysis_plan(event_id: String) -> Result<PlanView, String> {
    crate::commands::blocking(move || -> Result<PlanView, String> {
        sweep_stale_temp_packages();
        let connection = open_store()?;
        let event = crate::hooks::store::event_detail(&connection, &event_id)
            .map_err(|error| format!("读取事件失败：{error}"))?
            .ok_or_else(|| "事件不存在（可能已被清理）".to_string())?;
        let attachments = crate::hooks::store::event_attachments(&connection, &event.id)
            .map_err(|error| format!("读取附件失败：{error}"))?;
        let (family, instance, profile, _mode) = current_target_env();
        let records = load_records()?;
        let handoffs = records
            .iter()
            .filter(|record| record.event_id == event.id)
            .map(plan_summary)
            .collect();
        Ok(PlanView {
            default_workspace_path: crate::shell::paths::plain_path(&workspace_dir())
                .display()
                .to_string(),
            instance_label: format!("{family} · {instance} · profile {profile}"),
            input_limit_bytes: INPUT_LIMIT_BYTES,
            total_budget_bytes: TOTAL_BUDGET_BYTES,
            materials: plan_materials(&event, &attachments),
            handoffs,
            question_suggestion: QUESTION_SUGGESTION.into(),
        })
    })
    .await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogSelection {
    pub attachment_id: String,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceChoice {
    /// new | existing。
    pub mode: String,
    /// new:自定义工作空间绝对路径;缺省用默认分析空间。
    pub path: Option<String>,
    /// existing:目标会话及其工作空间(只读沿用,不移动会话)。
    pub session_id: Option<String>,
    pub session_workspace: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisRequest {
    pub event_id: String,
    pub request_id: String,
    pub include_body: bool,
    #[serde(default)]
    pub logs: Vec<LogSelection>,
    pub workspace: WorkspaceChoice,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffAck {
    pub handoff_id: String,
    pub session_title: String,
    pub workspace_path: String,
}

fn clean_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let collapsed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(60).collect()
}

fn new_handoff_id() -> String {
    format!("ha_{:016x}", rand::random::<u64>())
}

/// 校验并落盘一次交接:幂等、原子、限额齐全。纯函数 + 文件,便于单测。
fn prepare_handoff(request: AnalysisRequest) -> Result<(HandoffAck, AnalysisTarget), String> {
    let now = crate::shell::process::epoch_millis() as i64;
    sweep_stale_temp_packages();
    let connection = open_store()?;
    let event = crate::hooks::store::event_detail(&connection, &request.event_id)
        .map_err(|error| format!("读取事件失败：{error}"))?
        .ok_or_else(|| "事件不存在（可能已被清理）".to_string())?;
    if event.state != "ready" {
        return Err(format!(
            "事件当前状态为「{}」，只有完整入箱的事件才能分析",
            event.state
        ));
    }
    let attachments = crate::hooks::store::event_attachments(&connection, &event.id)
        .map_err(|error| format!("读取附件失败：{error}"))?;
    drop(connection);

    // 幂等:同一 request_id 复用同一交接与材料包(A09)。
    let mut records = load_records()?;
    if let Some(existing) = records
        .iter()
        .find(|record| {
            record.request_id == request.request_id && record.event_id == request.event_id
        })
        .cloned()
    {
        if package_dir(&existing.handoff_id).is_dir() {
            let target = target_for_record(&existing, &request)?;
            let mut q = crate::lock(queue());
            expire_stale_pending(&mut q);
            let target = enqueue_target(&mut q, target);
            drop(q);
            return Ok((ack_of(&existing), target));
        }
        // 材料包已被删除:旧记录失去事实源,移除后按新交接重建。
        records.retain(|record| record.handoff_id != existing.handoff_id);
    }

    let (materials, _input_bytes) = collect_materials(
        &event,
        &attachments,
        request.include_body,
        &request.logs,
        now,
    )?;
    let handoff_id = new_handoff_id();
    let manifest = PackageManifest {
        schema_version: 1,
        handoff_id: handoff_id.clone(),
        created_at: now,
        event_id: event.id.clone(),
        event_title: event.title.clone(),
        materials: materials.iter().map(|item| item.entry.clone()).collect(),
    };
    build_package(&handoff_id, &manifest, &materials)?;

    let (family, instance, profile, mode) = current_target_env();
    let (workspace_path, session_mode, session_id, session_title) =
        match request.workspace.mode.as_str() {
            "new" => {
                let base = request
                    .workspace
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty());
                let path = match base {
                    Some(custom) => {
                        let expanded = expand_home(custom)?;
                        ensure_writable_dir(&expanded)?;
                        crate::shell::paths::plain_path(&expanded)
                            .display()
                            .to_string()
                    }
                    None => {
                        let dir = workspace_dir();
                        ensure_writable_dir(&dir)?;
                        crate::shell::paths::plain_path(&dir).display().to_string()
                    }
                };
                (
                    path,
                    "new".to_string(),
                    None,
                    format!("Hook · {}", clean_title(&event.title)),
                )
            }
            "existing" => {
                let session_id = request
                    .workspace
                    .session_id
                    .clone()
                    .ok_or("选择已有会话时必须指定具体会话；请先在准备页选定或改选新建会话")?;
                let session_workspace = request
                    .workspace
                    .session_workspace
                    .clone()
                    .unwrap_or_default();
                (
                    session_workspace,
                    "existing".to_string(),
                    Some(session_id),
                    clean_title(&event.title),
                )
            }
            other => return Err(format!("不支持的会话模式 {other:?}")),
        };

    let blocks = material_blocks(&handoff_id)?;
    let question: Option<String> = if session_mode == "new" {
        Some(QUESTION_SUGGESTION.into())
    } else {
        None
    };
    let record = HandoffRecord {
        handoff_id: handoff_id.clone(),
        request_id: request.request_id.clone(),
        created_at: now,
        event_id: event.id.clone(),
        event_title: event.title.clone(),
        endpoint_name: event.endpoint_name.clone(),
        external_id: event.external_id.clone(),
        family: family.clone(),
        instance: instance.clone(),
        profile: profile.clone(),
        shell_mode: mode.clone(),
        workspace_path: workspace_path.clone(),
        session_mode: session_mode.clone(),
        session_title: session_title.clone(),
        session_id,
        state: "prepared".into(),
        failure: None,
        prompt_bytes: blocks.len() as u64 + question.as_ref().map(|q| q.len() as u64).unwrap_or(0),
    };
    let target = AnalysisTarget {
        sequence: 0,
        handoff_id: handoff_id.clone(),
        action: if session_mode == "new" {
            "prepare-new"
        } else {
            "prepare-existing"
        }
        .into(),
        family,
        instance,
        shell_mode: mode,
        workspace_path: (session_mode == "new").then(|| workspace_path.clone()),
        session_title: (session_mode == "new").then(|| session_title.clone()),
        session_id: record.session_id.clone(),
        question,
        materials: blocks,
    };
    records.push(record);
    save_records(&records)?;

    // 登记待派发目标并分配 sequence;漏了这一步,派发侧的 sequence CAS 会把
    // 目标当成已被取代而静默丢弃(单测钉住)。
    let mut q = crate::lock(queue());
    expire_stale_pending(&mut q);
    let target = enqueue_target(&mut q, target);
    drop(q);
    let ack = HandoffAck {
        handoff_id,
        session_title,
        workspace_path,
    };
    Ok((ack, target))
}

/// 从既有记录重建派发目标(幂等重发/恢复)。
fn target_for_record(
    record: &HandoffRecord,
    _request: &AnalysisRequest,
) -> Result<AnalysisTarget, String> {
    if !target_compatible(record) {
        return Err(format!(
            "该交接属于实例 {}（{} / {}），当前壳服务的是别的实例；请切回原实例后重试，不换实例投递",
            record.instance, record.family, record.shell_mode
        ));
    }
    let blocks = material_blocks(&record.handoff_id)?;
    Ok(AnalysisTarget {
        sequence: 0,
        handoff_id: record.handoff_id.clone(),
        action: if record.session_mode == "new" {
            "prepare-new"
        } else {
            "prepare-existing"
        }
        .into(),
        family: record.family.clone(),
        instance: record.instance.clone(),
        shell_mode: record.shell_mode.clone(),
        workspace_path: (record.session_mode == "new").then(|| record.workspace_path.clone()),
        session_title: (record.session_mode == "new").then(|| record.session_title.clone()),
        session_id: record.session_id.clone(),
        question: (record.session_mode == "new").then(|| QUESTION_SUGGESTION.into()),
        materials: blocks,
    })
}

fn ack_of(record: &HandoffRecord) -> HandoffAck {
    HandoffAck {
        handoff_id: record.handoff_id.clone(),
        session_title: record.session_title.clone(),
        workspace_path: record.workspace_path.clone(),
    }
}

/// ~ 开头的自定义工作空间按用户 home 展开;其余要求绝对路径。
fn expand_home(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    // 只认单个 ~ 前缀(后接任一分隔符),不做变量插值。
    if let Some(rest) = raw.strip_prefix('~') {
        if rest.is_empty() {
            return Ok(crate::shell::paths::dirs_home());
        }
        let first = rest.chars().next().unwrap_or(' ');
        if first == '/' || first == std::path::MAIN_SEPARATOR {
            return Ok(crate::shell::paths::dirs_home().join(&rest[1..]));
        }
        return Err("~ 后必须跟路径分隔符（例如 ~/projects）".into());
    }
    if !path.is_absolute() {
        return Err("自定义工作空间必须是绝对路径（可用 ~ 开头）".into());
    }
    Ok(path.to_path_buf())
}

/// 工作空间目录存在且可写;首次交接才创建,取消不落盘。
fn ensure_writable_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|error| format!("创建工作空间 {} 失败：{error}", dir.display()))?;
    let probe = dir.join(format!(".xlink-probe-{:016x}", rand::random::<u64>()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(error) => Err(format!(
            "工作空间 {} 不可写：{error}；请更换目录，不会回退到默认位置",
            dir.display()
        )),
    }
}

#[tauri::command]
pub(crate) async fn hooks_analysis_handoff(
    app: AppHandle,
    request: AnalysisRequest,
) -> Result<HandoffAck, String> {
    let (ack, target) = crate::commands::blocking(move || prepare_handoff(request)).await?;
    dispatch(&app, target);
    Ok(ack)
}

/// 准备页轮询交接状态:attached 关窗,failed 保留选择提示重试。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffStatus {
    pub handoff_id: String,
    pub state: String,
    pub state_label: String,
    pub failure: Option<String>,
    pub session_id: Option<String>,
}

#[tauri::command]
pub(crate) async fn hooks_analysis_status(handoff_id: String) -> Result<HandoffStatus, String> {
    crate::commands::blocking(move || -> Result<HandoffStatus, String> {
        let mut q = crate::lock(queue());
        expire_stale_pending(&mut q);
        drop(q);
        let records = load_records()?;
        let record = records
            .iter()
            .find(|record| record.handoff_id == handoff_id)
            .ok_or_else(|| "交接记录不存在（可能已被删除）".to_string())?;
        Ok(HandoffStatus {
            handoff_id: record.handoff_id.clone(),
            state: record.state.clone(),
            state_label: record.state_label().to_string(),
            failure: record.failure.clone(),
            session_id: record.session_id.clone(),
        })
    })
    .await
}

/// 「查看分析」恢复:同一实例/会话/工作空间,不静默换目标。
#[tauri::command]
pub(crate) async fn hooks_analysis_reopen(
    app: AppHandle,
    handoff_id: String,
) -> Result<(), String> {
    let target = crate::commands::blocking(move || -> Result<AnalysisTarget, String> {
        let records = load_records()?;
        let record = records
            .iter()
            .find(|record| record.handoff_id == handoff_id)
            .ok_or_else(|| "交接记录不存在（可能已被删除）".to_string())?
            .clone();
        if record.session_id.is_none() {
            return Err("这次交接还没有建立会话，请在 Hook 详情重新发起分析".into());
        }
        if !target_compatible(&record) {
            return Err(format!(
                "该交接属于实例 {}（{} / {}），当前壳服务的是别的实例；请切回原实例，不换实例投递",
                record.instance, record.family, record.shell_mode
            ));
        }
        let blocks = material_blocks(&record.handoff_id)?;
        Ok(AnalysisTarget {
            sequence: 0,
            handoff_id: record.handoff_id.clone(),
            action: "reopen".into(),
            family: record.family.clone(),
            instance: record.instance.clone(),
            shell_mode: record.shell_mode.clone(),
            workspace_path: None,
            session_title: None,
            session_id: record.session_id.clone(),
            question: None,
            materials: blocks,
        })
    })
    .await?;
    let mut q = crate::lock(queue());
    expire_stale_pending(&mut q);
    let target = enqueue_target(&mut q, target);
    drop(q);
    dispatch(&app, target);
    Ok(())
}

/// 删除材料包与壳侧记录;内核会话与工作台历史不动。
#[tauri::command]
pub(crate) async fn hooks_analysis_delete(handoff_id: String) -> Result<(), String> {
    crate::commands::blocking(move || -> Result<(), String> {
        let mut records = load_records()?;
        let existed = records.iter().any(|record| record.handoff_id == handoff_id);
        if !existed {
            return Err("交接记录不存在".into());
        }
        records.retain(|record| record.handoff_id != handoff_id);
        save_records(&records)?;
        let dir = package_dir(&handoff_id);
        if dir.is_dir() {
            fs::remove_dir_all(&dir).map_err(|error| {
                format!(
                    "删除材料包失败：{error}；记录已移除，请手动清理 {}",
                    dir.display()
                )
            })?;
        }
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------
// 会话列表桥:工作台插件回传当前 DSH 实例可访问的会话
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionItem {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub updated_at: i64,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionsView {
    /// 工作台是否可用并在应答窗口内回传了列表。
    pub available: bool,
    pub reason: String,
    pub items: Vec<SessionItem>,
}

#[tauri::command]
pub(crate) async fn hooks_analysis_sessions(app: AppHandle) -> Result<SessionsView, String> {
    // 内核未运行时如实说不可用;准备页引导先启动工作台或选新建。
    if !crate::kernel_running(&app) {
        return Ok(SessionsView {
            available: false,
            reason: "内核未运行：启动工作台后可选择已有会话，或先新建会话".into(),
            items: Vec::new(),
        });
    }
    let Some(window) = app.get_webview_window("harness") else {
        return Ok(SessionsView {
            available: false,
            reason: "工作台窗口未打开：请先打开工作台，或选择新建会话".into(),
            items: Vec::new(),
        });
    };
    let query_id = format!("haq_{:016x}", rand::random::<u64>());
    {
        let mut q = crate::lock(queue());
        q.sessions = Some(PendingSessions {
            query_id: query_id.clone(),
            items: None,
        });
    }
    let script = format!(
        "window.dispatchEvent(new CustomEvent('dsh-xlink-hook-analysis-sessions',{{detail:{{queryId:'{query_id}'}}}}));"
    );
    window
        .eval(script.as_str())
        .map_err(|error| format!("向工作台请求会话列表失败：{error}"))?;
    let deadline = Instant::now() + SESSIONS_BRIDGE_TIMEOUT;
    loop {
        let answered = {
            let q = crate::lock(queue());
            q.sessions
                .as_ref()
                .filter(|pending| pending.query_id == query_id)
                .and_then(|pending| pending.items.clone())
        };
        if let Some(items) = answered {
            crate::lock(queue()).sessions = None;
            return Ok(SessionsView {
                available: true,
                reason: String::new(),
                items,
            });
        }
        if Instant::now() >= deadline {
            crate::lock(queue()).sessions = None;
            return Ok(SessionsView {
                available: false,
                reason: "工作台未就绪（未确认接收插件或正在加载）：稍后重试，或选择新建会话".into(),
                items: Vec::new(),
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tauri::command]
pub(crate) fn hooks_analysis_sessions_report(
    window: WebviewWindow,
    query_id: String,
    items: Vec<SessionItem>,
) -> Result<(), String> {
    if window.label() != "harness" {
        return Err("只有工作台可以回传会话列表".into());
    }
    let mut q = crate::lock(queue());
    if let Some(pending) = q
        .sessions
        .as_mut()
        .filter(|pending| pending.query_id == query_id)
    {
        pending.items = Some(items);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 工作台插件命令(仅 harness 窗口可调)
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) fn hooks_analysis_take(window: WebviewWindow) -> Result<Option<AnalysisTarget>, String> {
    if window.label() != "harness" {
        return Err("只有工作台可以领取交接目标".into());
    }
    let mut q = crate::lock(queue());
    expire_stale_pending(&mut q);
    Ok(q.pending
        .as_ref()
        .filter(|pending| {
            let t = &pending.target;
            let (family, instance, _profile, mode) = current_target_env();
            t.family == family && t.instance == instance && t.shell_mode == mode
        })
        .map(|pending| pending.target.clone()))
}

#[tauri::command]
pub(crate) fn hooks_analysis_report(
    window: WebviewWindow,
    sequence: u64,
    handoff_id: String,
    session_id: Option<String>,
    error: String,
) -> Result<(), String> {
    if window.label() != "harness" {
        return Err("只有工作台可以确认交接".into());
    }
    // sequence CAS:迟到的确认不得改写后来新交接的状态。
    let confirmed = {
        let mut q = crate::lock(queue());
        q.pending
            .as_ref()
            .is_some_and(|pending| pending.target.sequence == sequence)
            && {
                q.pending = None;
                true
            }
    };
    if !confirmed {
        return Ok(());
    }
    if let Err(save_error) = apply_report(&handoff_id, session_id, &error) {
        crate::shell::shell_events::record("hook-analysis-report", &save_error);
    } else if !error.is_empty() {
        crate::shell::shell_events::record("hook-analysis-report", &error);
    }
    Ok(())
}

/// 工作台确认落账:attached 只在工作台确认草稿就位后写;不宣称 sent。
fn apply_report(handoff_id: &str, session_id: Option<String>, error: &str) -> Result<(), String> {
    let mut records = load_records()?;
    let record = records
        .iter_mut()
        .find(|record| record.handoff_id == handoff_id)
        .ok_or_else(|| "交接记录不存在（可能已被删除）".to_string())?;
    if error.is_empty() {
        record.state = "attached".into();
        if let Some(id) = session_id {
            record.session_id = Some(id);
        }
        record.failure = None;
    } else if error.contains("session-unavailable") {
        record.state = "unavailable".into();
        record.failure = Some(error.chars().take(512).collect());
    } else {
        record.state = "failed".into();
        record.failure = Some(error.chars().take(512).collect());
    }
    save_records(&records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::blob;
    use crate::hooks::store::{NewAttachment, NewEvent};
    use crate::tests::{scoped_xlink_home, EnvGuard};

    struct Case {
        home: PathBuf,
        event_id: String,
        attachment_id: String,
    }

    impl Drop for Case {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    /// 临时 home + 一个 ready 事件:正文 + 已归档日志。
    fn seed(body: &str, log: &str) -> (Case, EnvGuard) {
        let home = std::env::temp_dir().join(format!(
            "dsh-xlink-hooks-analysis-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&home).expect("创建临时 home");
        let guard = scoped_xlink_home(&home);
        let mut connection =
            crate::hooks::store::open(&crate::hooks::service::db_path()).expect("打开测试库");
        let (endpoint, _) =
            crate::hooks::store::create_endpoint(&connection, "分析", &["text".to_string()])
                .expect("接入点");
        let log_bytes = log.as_bytes();
        let spec = NewAttachment {
            client_id: "log-a",
            kind: crate::hooks::protocol::KIND_LOG,
            filename: "build.log",
            media_type: "text/plain",
            encoding: Some("utf-8"),
            size_bytes: log_bytes.len() as i64,
        };
        let parameters = serde_json::json!({});
        let event = NewEvent {
            external_id: "analysis-1",
            fingerprint: "analysis-1",
            event_type: "build",
            occurred_at: 0,
            title: "nightly 失败",
            text: body,
            parameters: &parameters,
            sender_policy: None,
            source_ip: None,
            commit: true,
            attachments: std::slice::from_ref(&spec),
        };
        let (event, _) =
            crate::hooks::store::create_event(&mut connection, &endpoint.id, &event, 1_000)
                .expect("创建事件");
        let attachment = crate::hooks::store::event_attachments(&connection, &event.id)
            .expect("附件")
            .remove(0);
        crate::hooks::store::begin_upload(
            &connection,
            &event.id,
            &attachment.id,
            2_000,
            "test-attempt",
        )
        .expect("租约");
        let (mut file, temp) = blob::create_temp().expect("临时文件");
        file.write_all(log_bytes).expect("写日志");
        file.sync_all().expect("刷盘");
        drop(file);
        let placed = blob::place(&temp, &event.id, &attachment.id).expect("归档");
        crate::hooks::store::finish_upload(
            &connection,
            &event.id,
            &attachment.id,
            &crate::shell::digest::hex(Sha256::digest(log_bytes)),
            log_bytes.len() as i64,
            &placed,
            2_000,
            "test-attempt",
        )
        .expect("完成上传");
        drop(connection);
        (
            Case {
                home,
                event_id: event.id,
                attachment_id: attachment.id,
            },
            guard,
        )
    }

    fn request(case: &Case, include_body: bool) -> AnalysisRequest {
        AnalysisRequest {
            event_id: case.event_id.clone(),
            request_id: "req-1".into(),
            include_body,
            logs: vec![LogSelection {
                attachment_id: case.attachment_id.clone(),
                start_line: None,
                end_line: None,
            }],
            workspace: WorkspaceChoice {
                mode: "new".into(),
                path: None,
                session_id: None,
                session_workspace: None,
            },
        }
    }

    #[test]
    fn handoff_builds_package_and_record_and_is_idempotent() {
        let (case, _guard) = seed("构建失败了", "line1 ok\nline2 warn\n");
        let (ack, target) = prepare_handoff(request(&case, true)).expect("建包");
        assert!(target.sequence > 0, "派发目标已登记 sequence");
        assert!(target.question.is_some(), "新建会话预填建议问题");
        assert!(target.materials.contains("line2 warn"), "日志进材料");
        assert!(target.materials.contains(&material_marker(&ack.handoff_id)));
        let dir = package_dir(&ack.handoff_id);
        assert!(dir.join("manifest.json").is_file());
        assert!(dir.join("m-body.txt").is_file());
        assert!(dir.join(format!("m-{}.txt", case.attachment_id)).is_file());
        assert!(
            Path::new(&ack.workspace_path).is_dir(),
            "默认工作空间已创建"
        );
        let records = load_records().expect("记录");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, "prepared");
        assert_eq!(records[0].event_id, case.event_id);

        // 同一 request_id 幂等:同一交接、同一材料包,不重复建。
        let (again, again_target) = prepare_handoff(request(&case, true)).expect("幂等重发");
        assert_eq!(again.handoff_id, ack.handoff_id);
        assert!(
            again_target.sequence > target.sequence,
            "重发登记新 sequence"
        );
        let mut published = 0;
        if let Ok(items) = fs::read_dir(packages_dir()) {
            published = items
                .flatten()
                .filter(|entry| entry.file_name() != ".tmp")
                .count();
        }
        assert_eq!(published, 1, "幂等重发不新建材料包（.tmp 临时根不计）");
        assert_eq!(load_records().expect("记录").len(), 1);
    }

    #[test]
    fn line_segments_are_extracted_and_marked_incomplete() {
        let log = "one\ntwo\nthree\nfour\n";
        let (case, _guard) = seed("正文", log);
        let mut req = request(&case, false);
        req.logs[0].start_line = Some(2);
        req.logs[0].end_line = Some(3);
        let (ack, target) = prepare_handoff(req).expect("建包");
        let manifest: PackageManifest = serde_json::from_slice(
            &fs::read(package_dir(&ack.handoff_id).join("manifest.json")).expect("清单"),
        )
        .expect("解析");
        let log_entry = manifest
            .materials
            .iter()
            .find(|entry| entry.kind == "log")
            .expect("日志材料");
        assert_eq!(log_entry.line_range, Some((2, 3)));
        assert!(!log_entry.complete, "选段不冒充完整日志");
        let content = fs::read_to_string(
            package_dir(&ack.handoff_id).join(material_file_name(&log_entry.internal_id)),
        )
        .expect("选段文件");
        assert_eq!(content, "two\nthree\n");
        assert!(
            target.materials.contains("原文件第 2-3 行"),
            "模型侧保留选段限定"
        );
    }

    #[test]
    fn over_limit_is_rejected_without_publishing_a_package() {
        let big = "x".repeat(600 * 1024);
        let (case, _guard) = seed(&big, &big);
        let result = prepare_handoff(request(&case, true));
        let error = result.expect_err("超限必须拒绝");
        assert!(error.contains("上限"), "错误要说清上限与下一步：{error}");
        assert!(!packages_dir().join("ha_anything").exists());
        let mut published = 0;
        if let Ok(items) = fs::read_dir(packages_dir()) {
            published = items.flatten().filter(|e| e.path().is_dir()).count();
        }
        assert_eq!(published, 0, "失败不发布半包");
        assert!(load_records().expect("记录").is_empty(), "失败不留半记录");
    }

    #[test]
    fn report_transitions_records_and_rejects_unknown_handoffs() {
        let (case, _guard) = seed("正文", "log\n");
        let (ack, _) = prepare_handoff(request(&case, true)).expect("建包");
        apply_report(&ack.handoff_id, Some("session-7".into()), "").expect("确认");
        let records = load_records().expect("记录");
        assert_eq!(records[0].state, "attached");
        assert_eq!(records[0].session_id.as_deref(), Some("session-7"));
        assert!(records[0].failure.is_none());

        apply_report(&ack.handoff_id, None, "session-unavailable: 目标会话已删除").expect("落账");
        let records = load_records().expect("记录");
        assert_eq!(records[0].state, "unavailable");

        apply_report(&ack.handoff_id, None, "draft blocked").expect("落账");
        let records = load_records().expect("记录");
        assert_eq!(records[0].state, "failed");
        assert!(records[0].failure.is_some());

        assert!(
            apply_report("ha_missing", None, "").is_err(),
            "未知交接不得静默成功"
        );
    }

    #[test]
    fn delete_removes_package_and_record_only_for_that_handoff() {
        let (case, _guard) = seed("正文", "log\n");
        let (ack, _) = prepare_handoff(request(&case, true)).expect("建包");
        let (other, _) = prepare_handoff({
            let mut req = request(&case, true);
            req.request_id = "req-2".into();
            req
        })
        .expect("第二个交接");
        let remove = |id: &str| -> Result<(), String> {
            let mut records = load_records()?;
            let existed = records.iter().any(|record| record.handoff_id == id);
            if !existed {
                return Err("交接记录不存在".into());
            }
            records.retain(|record| record.handoff_id != id);
            save_records(&records)?;
            let dir = package_dir(id);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).map_err(|error| error.to_string())?;
            }
            Ok(())
        };
        remove(&ack.handoff_id).expect("删除");
        assert!(!package_dir(&ack.handoff_id).exists());
        assert!(package_dir(&other.handoff_id).exists(), "只删选定的材料包");
        let records = load_records().expect("记录");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].handoff_id, other.handoff_id);
    }

    #[test]
    fn extract_lines_clamps_and_rejects_bad_ranges() {
        let bytes = b"one\ntwo\nthree\n";
        assert_eq!(extract_lines(bytes, 2, 99, "l").unwrap(), b"two\nthree\n");
        assert_eq!(extract_lines(bytes, 1, 2, "l").unwrap(), b"one\ntwo\n");
        assert!(extract_lines(bytes, 0, 2, "l").is_err(), "0 起行号非法");
        assert!(extract_lines(bytes, 3, 2, "l").is_err(), "起大于止非法");
        assert!(
            extract_lines(bytes, 9, 12, "l").is_err(),
            "越界选段为空要报错"
        );
    }

    #[test]
    fn budget_check_is_a_pure_guard() {
        let root = std::env::temp_dir().join(format!(
            "dsh-xlink-analysis-budget-{}-{}",
            std::process::id(),
            crate::shell::process::epoch_millis()
        ));
        fs::create_dir_all(root.join("ha_a")).expect("建目录");
        fs::write(root.join("ha_a/m-body.txt"), vec![0u8; 100]).expect("写文件");
        assert!(ensure_budget(&root, 10, 200).is_ok(), "预算内放行");
        let rejected = ensure_budget(&root, 101, 200).expect_err("超预算拒绝");
        assert!(
            rejected.contains("总预算"),
            "拒绝文案指向管理入口：{rejected}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
