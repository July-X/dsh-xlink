//! Hook → DeepSeek 分析:材料快照、交接记录与工作台派发。
//!
//! 设计契约:docs/features/hooks/deepseek-analysis-design.md。阶段 A 已对
//! 0.2.2-rc.4 实包核实:内核原生支持 `workspaces.create({path})`、
//! `sessions.create({workspaceId, sessionId})` 接受壳预分配的会话 ID,与草稿初始化
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
    time::Duration,
};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Webview};

/// 单次交接材料合计上限(UTF-8 文本字节)。超限拒绝,不静默截断。
pub(crate) const INPUT_LIMIT_BYTES: u64 = 1024 * 1024;
/// 已发布材料包总预算。是资源上限,不是模型 token 容量保证。
pub(crate) const TOTAL_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;
/// 未被工作台确认的派发目标保留时长;超时按失败落账,可重试。
const PENDING_TTL: Duration = Duration::from_secs(150);
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
    /// 先持久化 DSH 的预分配 ID，再允许原生创建。同一次交接重试沿用。
    pub reserved_session_id: Option<String>,
    pub request_fingerprint: String,
    pub attached_once: bool,
    /// 草稿写入前持久化。壳在回执前退出时，打开仍不自动重插可能已经发送的材料。
    pub draft_write_started: bool,
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
            "unconfirmed" => "请核对原草稿",
            "failed" => "交接失败",
            "unavailable" => "原会话不可用",
            _ => "未知",
        }
    }
}

fn load_records() -> Result<Vec<HandoffRecord>, String> {
    let path = handoffs_file();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取交接记录失败：{error}")),
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("交接记录 {path:?} 损坏：{error}；请修复存储后重试"))
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
    let mode = crate::shell::settings::current_mode();
    let profile = crate::shell::settings::load_for_shell(mode).profile;
    (
        family.to_string(),
        instance.to_string(),
        profile,
        mode.as_str().into(),
    )
}

/// 交接目标仍属于当前实例与壳模式才允许派发/确认(A16/A17)。
fn target_compatible(record: &HandoffRecord) -> bool {
    (
        record.family.clone(),
        record.instance.clone(),
        record.profile.clone(),
        record.shell_mode.clone(),
    ) == current_target_env()
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
    #[serde(default)]
    pub original_sha256: Option<String>,
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
struct LogMaterial {
    bytes: Vec<u8>,
    original_sha256: String,
    line_range: Option<(u64, u64)>,
}

fn read_log_material(
    attachment: &crate::hooks::store::AttachmentRow,
    line_range: Option<(u64, u64)>,
) -> Result<LogMaterial, String> {
    let name = &attachment.filename;
    let rel = attachment
        .storage_rel
        .as_deref()
        .ok_or_else(|| format!("日志 {name} 没有归档路径，请刷新收件箱后重试"))?;
    let path = crate::hooks::blob::path_for_rel(rel)
        .map_err(|error| format!("日志 {name} 的存储路径非法：{error}"))?;
    let length = ordinary_file_len(&path)
        .map_err(|error| format!("无法读取日志 {name}：{error}（可能已被清理）"))?;
    if u64::try_from(attachment.size_bytes) != Ok(length)
        || length > crate::hooks::protocol::MAX_LOG_BYTES
    {
        return Err(format!(
            "日志 {name} 与收件箱记录的大小不一致，请刷新后重试"
        ));
    }
    if line_range.is_none() && length > INPUT_LIMIT_BYTES {
        return Err(format!("日志 {name} 超过单次材料上限，请选择行段"));
    }
    let expected = attachment
        .sha256
        .as_deref()
        .ok_or_else(|| format!("日志 {name} 缺少收件箱 SHA-256，无法核对"))?;
    let bytes =
        read_bounded(&path, length).map_err(|error| format!("读取日志 {name} 失败：{error}"))?;
    let original_sha256 = crate::shell::digest::hex(Sha256::digest(&bytes));
    if bytes.len() as u64 != length || !original_sha256.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "日志 {name} 与收件箱记录的大小或 SHA-256 不一致，请刷新后重试"
        ));
    }
    let (bytes, line_range) = match line_range {
        None => (bytes, None),
        Some((start, end)) => {
            let (selected, actual_end) = extract_lines(&bytes, start, end, name)?;
            (selected, Some((start, actual_end)))
        }
    };
    std::str::from_utf8(&bytes)
        .map_err(|_| format!("日志 {name} 所选内容不是有效 UTF-8 文本，本期不支持分析"))?;
    Ok(LogMaterial {
        bytes,
        original_sha256,
        line_range,
    })
}

fn ordinary_file_len(path: &Path) -> Result<u64, String> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("存储路径不是普通文件".into());
    }
    Ok(meta.len())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("文件超过预期大小".into());
    }
    Ok(bytes)
}

/// 原始行号为 1 起闭区间，返回实际末行。保留原始换行和字节。
fn extract_lines(
    bytes: &[u8],
    start: u64,
    end: u64,
    filename: &str,
) -> Result<(Vec<u8>, u64), String> {
    if start == 0 || end < start {
        return Err("行段格式非法：需要 1 起的起止行号，且结束行不小于起始行".into());
    }
    let mut selected = Vec::new();
    let mut actual_end = 0;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let number = index as u64 + 1;
        if number < start {
            continue;
        }
        if selected.len() + line.len() > INPUT_LIMIT_BYTES as usize {
            return Err(format!("日志 {filename} 所选行段超过单次材料上限"));
        }
        selected.extend_from_slice(line);
        actual_end = number;
        if number == end {
            break;
        }
    }
    if actual_end == 0 {
        return Err(format!(
            "日志 {filename} 的所选起始行 {start} 越界，请调整行号"
        ));
    }
    Ok((selected, actual_end))
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
            original_sha256: None,
            bytes: bytes.len() as u64,
            line_range: None,
            complete: true,
        };
        add(MaterialContent { entry, bytes }, &mut total)?;
    }
    let mut selected_ids = std::collections::HashSet::new();
    for selection in logs {
        if !selected_ids.insert(&selection.attachment_id) {
            return Err("同一日志只能选择一次".into());
        }
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
        let LogMaterial {
            bytes,
            original_sha256,
            line_range,
        } = read_log_material(attachment, line_range)?;
        let entry = MaterialEntry {
            kind: "log".into(),
            internal_id: attachment.id.clone(),
            display_name: attachment.filename.clone(),
            source_label: event.endpoint_name.clone(),
            external_id: event.external_id.clone(),
            snapshot_at: now,
            sha256: crate::shell::digest::hex(Sha256::digest(&bytes)),
            original_sha256: Some(original_sha256),
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
fn stage_package(
    handoff_id: &str,
    manifest: &PackageManifest,
    materials: &[MaterialContent],
) -> Result<PathBuf, String> {
    fs::create_dir_all(packages_dir()).map_err(|error| format!("创建材料包目录失败：{error}"))?;
    let manifest_bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("序列化材料清单失败：{error}"))?;
    let mark = serde_json::json!({ "owner": "hook-analysis", "handoffId": handoff_id }).to_string();
    let incoming: u64 = materials.iter().map(|item| item.bytes.len() as u64).sum();
    ensure_budget(
        &packages_dir(),
        incoming + manifest_bytes.len() as u64 + mark.len() as u64,
        TOTAL_BUDGET_BYTES,
    )?;
    let temp_root = temp_packages_dir();
    fs::create_dir_all(&temp_root).map_err(|error| format!("创建材料临时目录失败：{error}"))?;
    let temp = temp_root.join(format!("{handoff_id}-{:016x}", rand::random::<u64>()));
    fs::create_dir_all(&temp).map_err(|error| format!("创建临时材料包失败：{error}"))?;
    if let Err(error) = write_file_synced(&temp.join(TEMP_MARKER), mark.as_bytes()) {
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
    if let Err(error) = write_file_synced(&temp.join("manifest.json"), &manifest_bytes) {
        let _ = fs::remove_dir_all(&temp);
        return Err(format!("写材料清单失败：{error}"));
    }
    sync_dir(&temp);
    Ok(temp)
}

fn publish_package(handoff_id: &str, temp: &Path) -> Result<(), String> {
    let target = package_dir(handoff_id);
    if target.exists() {
        let _ = fs::remove_dir_all(temp);
        return Err("材料包目录已存在，请重试".into());
    }
    fs::rename(temp, &target).map_err(|error| {
        let _ = fs::remove_dir_all(temp);
        format!("发布材料包失败：{error}。请检查数据目录权限与磁盘空间后重试")
    })?;
    sync_dir(target.parent().unwrap_or(&packages_dir()));
    Ok(())
}

fn ensure_budget(packages: &Path, incoming: u64, budget: u64) -> Result<(), String> {
    let existing = dir_size(packages, 3);
    if existing + incoming > budget {
        return Err(format!(
            "分析材料包总预算已满（现有 {existing} 字节 + 本次 {incoming} 字节 > 上限 {budget} 字节）。请在「管理分析材料」里删除不再需要的旧材料后重试；内核会话不受删除影响"
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
    for line in header.lines().chain(content.lines()) {
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
    if let Some(hash) = &entry.original_sha256 {
        header.push_str(&format!(" · 原文件 SHA-256 {hash}"));
    }
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
fn material_blocks(handoff_id: &str, dir: &Path) -> Result<String, String> {
    if !valid_segment(handoff_id) {
        return Err("分析记录 ID 非法".into());
    }
    let manifest_path = dir.join("manifest.json");
    let manifest_len = ordinary_file_len(&manifest_path)
        .map_err(|_| format!("材料包 {handoff_id} 的清单缺失或不可读"))?;
    let manifest: PackageManifest = serde_json::from_slice(&read_bounded(
        &manifest_path,
        manifest_len.min(INPUT_LIMIT_BYTES),
    )?)
    .map_err(|error| format!("材料包 {handoff_id} 的清单损坏：{error}"))?;
    if manifest.handoff_id != handoff_id
        || manifest.schema_version != 1
        || manifest.materials.is_empty()
        || manifest.materials.len() > crate::hooks::protocol::MAX_ATTACHMENTS + 1
    {
        return Err(format!("材料包 {handoff_id} 的清单不匹配"));
    }
    let mut total = 0_u64;
    let mut ids = std::collections::HashSet::new();
    let mut blocks = Vec::new();
    for entry in &manifest.materials {
        total = total.saturating_add(entry.bytes);
        if !valid_segment(&entry.internal_id)
            || !ids.insert(&entry.internal_id)
            || total > INPUT_LIMIT_BYTES
        {
            return Err(format!("材料包 {handoff_id} 的材料 ID 或大小不合法"));
        }
        let path = dir.join(material_file_name(&entry.internal_id));
        let changed = || {
            format!(
                "材料包 {handoff_id} 的「{}」大小或 SHA-256 不一致",
                entry.display_name
            )
        };
        if ordinary_file_len(&path)? != entry.bytes {
            return Err(changed());
        }
        let bytes = read_bounded(&path, entry.bytes)?;
        if bytes.len() as u64 != entry.bytes
            || !crate::shell::digest::hex(Sha256::digest(&bytes))
                .eq_ignore_ascii_case(&entry.sha256)
        {
            return Err(changed());
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| format!("材料 {} 不再是有效 UTF-8 文本", entry.display_name))?;
        blocks.push(quote_block(&material_header(entry, handoff_id), &content));
    }
    Ok(blocks.join("\n"))
}

// ---------------------------------------------------------------------------
// 派发队列:同一时刻一个目标,sequence CAS 挡迟到响应
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisTarget {
    pub sequence: u64,
    pub handoff_id: String,
    /// prepare-new | prepare-existing | open
    pub action: String,
    pub family: String,
    pub instance: String,
    pub shell_mode: String,
    pub profile: String,
    pub expires_at: i64,
    pub reserved_session_id: Option<String>,
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

#[derive(Default)]
struct AnalysisQueue {
    sequence: u64,
    pending: Option<AnalysisTarget>,
    receipt: Option<HandoffReceipt>,
    sessions: std::collections::HashMap<String, SessionListAnswer>,
}

fn queue() -> &'static Mutex<AnalysisQueue> {
    static QUEUE: OnceLock<Mutex<AnalysisQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(AnalysisQueue::default()))
}

fn enqueue_target(queue: &mut AnalysisQueue, mut target: AnalysisTarget) -> AnalysisTarget {
    debug_assert!(queue.pending.is_none());
    queue.sequence += 1;
    target.sequence = queue.sequence;
    target.expires_at = now_ms() + PENDING_TTL.as_millis() as i64;
    queue.pending = Some(target.clone());
    target
}

fn now_ms() -> i64 {
    crate::shell::process::epoch_millis() as i64
}

fn expire_stale_pending(queue: &mut AnalysisQueue) -> Result<(), String> {
    flush_receipt(queue)?;
    if let Some(handoff_id) = queue
        .pending
        .as_ref()
        .filter(|target| now_ms() >= target.expires_at)
        .map(|target| target.handoff_id.clone())
    {
        settle_pending(
            queue,
            &handoff_id,
            None,
            "工作台长时间未确认；请从「管理分析材料」重试，材料包与记录保留",
        )?;
    }
    Ok(())
}

/// 确认必须对应当前交接，并绑定实际运行的 DSH 实例与 profile。
fn pending_for<'a>(
    queue: &'a AnalysisQueue,
    sequence: u64,
    handoff_id: &str,
) -> Option<&'a AnalysisTarget> {
    queue.pending.as_ref().filter(|target| {
        target.sequence == sequence
            && target.handoff_id == handoff_id
            && now_ms() < target.expires_at
    })
}

fn target_env_matches(target: &AnalysisTarget) -> bool {
    let (family, instance, profile, mode) = current_target_env();
    target.family == family
        && target.instance == instance
        && target.profile == profile
        && target.shell_mode == mode
}

fn verify_running_profile(app: &AppHandle, profile: &str) -> Result<(), String> {
    let (family, instance) = crate::shell::instance::resolve_default();
    let mode = crate::shell::settings::current_mode();
    let settings = crate::shell::settings::load_for_shell(mode);
    let state = app.state::<crate::AppState>();
    let data_dir = &state.data_dir;
    let pid = crate::shell::instance::read_pid_with_profile(family, instance);
    if settings.profile != profile
        || !pid.is_some_and(|(record, launched_profile)| {
            launched_profile.as_deref() == Some(profile)
                && record.shell == Some(mode.as_str())
                && record.port == Some(settings.port)
                && crate::kernel::lifecycle::workbench_pid(data_dir, &settings) == Some(record.pid)
        })
    {
        return Err("工作台的实例或 profile 无法确认，请停止工作台并按原 profile 重新启动".into());
    }
    Ok(())
}

fn dispatch_failed(target: &AnalysisTarget, error: &str) {
    let mut q = crate::lock(queue());
    if pending_for(&q, target.sequence, &target.handoff_id).is_none() {
        return;
    }
    match settle_pending(&mut q, &target.handoff_id, None, error) {
        Ok(()) => crate::shell::shell_events::record("hook-analysis-dispatch", error),
        Err(reason) => crate::shell::shell_events::record("hook-analysis-dispatch", &reason),
    }
}

fn dispatch(app: &AppHandle, target: AnalysisTarget) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if !target_env_matches(&target) {
            return dispatch_failed(&target, "当前实例或 profile 已变更，请切回原目标后重试");
        }
        if !crate::kernel_running(&handle) {
            let app = handle.clone();
            let started = crate::commands::blocking(move || {
                let data_dir = app.state::<crate::AppState>().data_dir.clone();
                let mut send = |message: &str| {
                    crate::shell::shell_events::record("hook-analysis-start", message);
                };
                crate::commands::start_kernel_blocking(&app, &data_dir, &mut send)
            })
            .await;
            match started {
                Ok(report) if report.running => {}
                Ok(_) => {
                    return dispatch_failed(
                        &target,
                        "内核未能启动，请在概览页查看启动诊断后重试；材料包与记录保留",
                    )
                }
                Err(error) => return dispatch_failed(&target, &format!("内核启动失败：{error}")),
            }
        }
        if let Err(error) = verify_running_profile(&handle, &target.profile) {
            return dispatch_failed(&target, &error);
        }
        if let Err(error) = crate::commands::open_harness(handle.clone()).await {
            return dispatch_failed(&target, &format!("打开工作台失败：{error}"));
        }
        if pending_for(&crate::lock(queue()), target.sequence, &target.handoff_id).is_none() {
            return;
        }
        let Some(window) = handle.get_webview("harness") else {
            return dispatch_failed(&target, "工作台窗口未打开");
        };
        crate::notify::activate::force_foreground(&window.window());
        match serde_json::to_string(&target) {
            Ok(json) => {
                if let Err(error) = window.eval(format!(
                    "window.dispatchEvent(new CustomEvent('dsh-xlink-hook-analysis',{{detail:{json}}}));"
                )) {
                    dispatch_failed(&target, &format!("注入交接目标失败：{error}"));
                }
            }
            Err(error) => dispatch_failed(&target, &format!("序列化交接目标失败：{error}")),
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
    pub event_title: String,
    pub instance: String,
    pub session_title: String,
    pub session_id: Option<String>,
    pub state: String,
    pub state_label: String,
    pub created_at: i64,
    pub failure: Option<String>,
    pub workspace_path: String,
    pub prompt_bytes: u64,
    pub may_have_draft: bool,
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
        event_title: record.event_title.clone(),
        instance: record.instance.clone(),
        session_title: record.session_title.clone(),
        session_id: record.session_id.clone(),
        state: record.state.clone(),
        state_label: record.state_label().to_string(),
        created_at: record.created_at,
        failure: record.failure.clone(),
        workspace_path: record.workspace_path.clone(),
        prompt_bytes: record.prompt_bytes,
        may_have_draft: record.attached_once
            || record.draft_write_started
            || record.state == "attached",
    }
}

/// 材料头部预览:只读前 256 字节,不整块搬运大文件。
fn head_preview(path: &Path, limit: u64) -> Option<String> {
    use std::io::Read;
    ordinary_file_len(path).ok()?;
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
pub(crate) async fn hooks_analysis_plan(event_id: Option<String>) -> Result<PlanView, String> {
    crate::commands::blocking(move || build_plan(event_id.as_deref())).await
}

fn build_plan(event_id: Option<&str>) -> Result<PlanView, String> {
    let _guard = crate::lock(queue());
    sweep_stale_temp_packages();
    let mut materials = Vec::new();
    if let Some(event_id) = event_id {
        let connection = open_store()?;
        if let Some(event) = crate::hooks::store::event_detail(&connection, event_id)
            .map_err(|error| format!("读取事件失败：{error}"))?
        {
            let attachments = crate::hooks::store::event_attachments(&connection, &event.id)
                .map_err(|error| format!("读取附件失败：{error}"))?;
            materials = plan_materials(&event, &attachments);
        }
    }
    let (family, instance, profile, _mode) = current_target_env();
    let mut handoffs: Vec<_> = load_records()?
        .iter()
        .filter(|record| event_id.is_none_or(|id| record.event_id == id))
        .map(plan_summary)
        .collect();
    handoffs.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    Ok(PlanView {
        default_workspace_path: crate::shell::paths::plain_path(&workspace_dir())
            .display()
            .to_string(),
        instance_label: format!("{family} · {instance} · profile {profile}"),
        input_limit_bytes: INPUT_LIMIT_BYTES,
        total_budget_bytes: TOTAL_BUDGET_BYTES,
        materials,
        handoffs,
        question_suggestion: QUESTION_SUGGESTION.into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogSelection {
    pub attachment_id: String,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceChoice {
    pub mode: String,
    pub path: Option<String>,
    pub session_id: Option<String>,
    pub session_workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// 同一把锁串行化交接记录、缺额、提交和回执；工作台操作在锁外执行。
fn prepare_handoff(
    q: &mut AnalysisQueue,
    request: AnalysisRequest,
) -> Result<(HandoffAck, Option<AnalysisTarget>), String> {
    expire_stale_pending(q)?;
    if !valid_segment(&request.request_id) {
        return Err("分析请求 ID 非法".into());
    }
    let mut records = load_records()?;
    let fingerprint = crate::shell::digest::hex(Sha256::digest(
        serde_json::to_vec(&request).map_err(|error| format!("无法校验交接请求：{error}"))?,
    ));
    let previous = records
        .iter()
        .find(|record| {
            record.request_id == request.request_id && record.event_id == request.event_id
        })
        .cloned();
    if let Some(record) = previous.as_ref() {
        if !record.request_fingerprint.is_empty() && record.request_fingerprint != fingerprint {
            return Err("同一分析请求的材料或目标已改变，请重新发起".into());
        }
        ensure_record_target(record)?;
        if package_dir(&record.handoff_id).is_dir() {
            if record.state == "attached" {
                return Ok((ack_of(record), None));
            }
            return Ok((
                ack_of(record),
                queue_record(q, records, &record.handoff_id, RecordOperation::Retry)?,
            ));
        }
    }
    ensure_idle(q)?;
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
    let now = now_ms();
    let (materials, _) = collect_materials(
        &event,
        &attachments,
        request.include_body,
        &request.logs,
        now,
    )?;
    let (family, instance, profile, mode) = current_target_env();
    let (workspace_path, session_id, session_title) = match request.workspace.mode.as_str() {
        "new" => {
            let dir = match request
                .workspace
                .path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                Some(path) => {
                    let dir = expand_home(path)?;
                    if !dir.is_dir() {
                        return Err("更改的工作空间必须是已存在的目录".into());
                    }
                    dir
                }
                None => workspace_dir(),
            };
            ensure_writable_dir(&dir)?;
            let dir = dir
                .canonicalize()
                .map_err(|error| format!("解析工作空间失败：{error}"))?;
            (
                crate::shell::paths::plain_path(&dir).display().to_string(),
                None,
                format!("Hook · {}", clean_title(&event.title)),
            )
        }
        "existing" => {
            let session_id = request
                .workspace
                .session_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .ok_or("选择已有会话时必须指定具体会话；请先在准备页选定或改选新建会话")?;
            (
                request
                    .workspace
                    .session_workspace
                    .clone()
                    .unwrap_or_default(),
                Some(session_id.into()),
                clean_title(&event.title),
            )
        }
        other => return Err(format!("不支持的会话模式 {other:?}")),
    };
    let old = records.clone();
    let handoff_id = previous
        .as_ref()
        .map_or_else(new_handoff_id, |record| record.handoff_id.clone());
    records.retain(|record| record.handoff_id != handoff_id);
    let manifest = PackageManifest {
        schema_version: 1,
        handoff_id: handoff_id.clone(),
        created_at: now,
        event_id: event.id.clone(),
        event_title: event.title.clone(),
        materials: materials.iter().map(|item| item.entry.clone()).collect(),
    };
    let staged = stage_package(&handoff_id, &manifest, &materials)?;
    let blocks = material_blocks(&handoff_id, &staged).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staged);
    })?;
    let question = (request.workspace.mode == "new").then(|| QUESTION_SUGGESTION.into());
    let mut record = HandoffRecord {
        handoff_id: handoff_id.clone(),
        request_id: request.request_id,
        created_at: now,
        event_id: event.id,
        event_title: event.title,
        endpoint_name: event.endpoint_name,
        external_id: event.external_id,
        family,
        instance,
        profile,
        shell_mode: mode,
        workspace_path,
        session_mode: request.workspace.mode,
        session_title,
        session_id: previous
            .as_ref()
            .and_then(|record| record.session_id.clone())
            .or(session_id),
        reserved_session_id: previous
            .as_ref()
            .and_then(|record| record.reserved_session_id.clone()),
        request_fingerprint: fingerprint,
        attached_once: previous
            .as_ref()
            .is_some_and(|record| record.attached_once || record.state == "attached"),
        draft_write_started: previous
            .as_ref()
            .is_some_and(|record| record.draft_write_started),
        state: "prepared".into(),
        failure: None,
        prompt_bytes: blocks.len() as u64
            + question.as_ref().map_or(0, |q: &String| q.len() as u64),
    };
    reserve_session(&mut record);
    let ack = ack_of(&record);
    let action = if record.session_mode == "new" {
        "prepare-new"
    } else {
        "prepare-existing"
    };
    let target = target_for_record(&record, action, blocks);
    records.push(record);
    // 先记账再发布，崩溃时最多留下待重试记录及有归属标记的临时包。
    save_records(&records).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staged);
    })?;
    if let Err(error) = publish_package(&handoff_id, &staged) {
        return match save_records(&old) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}；回滚交接记录失败：{rollback}")),
        };
    }
    Ok((ack, Some(enqueue_target(q, target))))
}

fn ensure_idle(q: &AnalysisQueue) -> Result<(), String> {
    if q.pending.is_some() {
        return Err("另一份 Hook 材料正在交接，请等工作台确认后重试".into());
    }
    Ok(())
}

fn ensure_record_target(record: &HandoffRecord) -> Result<(), String> {
    if !target_compatible(record) {
        return Err(format!(
            "该交接属于实例 {} / profile {}（{}），请切回原目标后重试",
            record.instance, record.profile, record.shell_mode
        ));
    }
    Ok(())
}

fn reserve_session(record: &mut HandoffRecord) {
    if record.session_mode == "new"
        && record.session_id.is_none()
        && record.reserved_session_id.is_none()
    {
        record.reserved_session_id = Some(format!(
            "session-xlink-hook-{}",
            crate::shell::digest::hex(rand::random::<[u8; 16]>())
        ));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecordOperation {
    Retry,
    Open,
    Restore,
}

fn queue_record(
    q: &mut AnalysisQueue,
    mut records: Vec<HandoffRecord>,
    handoff_id: &str,
    operation: RecordOperation,
) -> Result<Option<AnalysisTarget>, String> {
    if q.pending
        .as_ref()
        .is_some_and(|target| target.handoff_id == handoff_id)
    {
        return Ok(None);
    }
    ensure_idle(q)?;
    let record = records
        .iter_mut()
        .find(|record| record.handoff_id == handoff_id)
        .ok_or("交接记录不存在（可能已被删除）")?;
    ensure_record_target(record)?;
    record.attached_once |= record.state == "attached";
    let may_have_draft = record.attached_once || record.draft_write_started;
    let action = match operation {
        RecordOperation::Open | RecordOperation::Retry
            if may_have_draft || record.state == "unavailable" =>
        {
            "open"
        }
        RecordOperation::Restore if record.session_id.is_some() => "prepare-existing",
        _ if record.session_mode == "new" => "prepare-new",
        _ => "prepare-existing",
    };
    let materials = if action == "open" {
        String::new()
    } else {
        material_blocks(handoff_id, &package_dir(handoff_id))?
    };
    reserve_session(record);
    let target = target_for_record(record, action, materials);
    record.state = "prepared".into();
    record.failure = None;
    save_records(&records)?;
    Ok(Some(enqueue_target(q, target)))
}

fn target_for_record(record: &HandoffRecord, action: &str, materials: String) -> AnalysisTarget {
    let new = action == "prepare-new";
    AnalysisTarget {
        sequence: 0,
        handoff_id: record.handoff_id.clone(),
        action: action.into(),
        family: record.family.clone(),
        instance: record.instance.clone(),
        shell_mode: record.shell_mode.clone(),
        profile: record.profile.clone(),
        expires_at: 0,
        reserved_session_id: record.reserved_session_id.clone(),
        workspace_path: Some(record.workspace_path.clone()),
        session_title: new.then(|| record.session_title.clone()),
        session_id: record.session_id.clone(),
        question: new.then(|| QUESTION_SUGGESTION.into()),
        materials,
    }
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
    let (ack, target) =
        crate::commands::blocking(move || prepare_handoff(&mut crate::lock(queue()), request))
            .await?;
    if let Some(target) = target {
        dispatch(&app, target);
    }
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
    crate::commands::blocking(move || read_status(&mut crate::lock(queue()), &handoff_id)).await
}

fn read_status(q: &mut AnalysisQueue, handoff_id: &str) -> Result<HandoffStatus, String> {
    let durability_error = expire_stale_pending(q).err();
    if let Some(failure) = durability_error.filter(|_| {
        q.pending
            .as_ref()
            .is_some_and(|target| target.handoff_id == handoff_id)
    }) {
        return Ok(HandoffStatus {
            handoff_id: handoff_id.into(),
            state: "receipt-pending".into(),
            state_label: "工作台回执待落盘".into(),
            failure: Some(failure),
            session_id: q
                .pending
                .as_ref()
                .and_then(|target| target.session_id.clone()),
        });
    }
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
}

/// 「查看分析」恢复:同一实例/会话/工作空间,不静默换目标。
#[tauri::command]
pub(crate) async fn hooks_analysis_reopen(
    app: AppHandle,
    handoff_id: String,
    restore_materials: Option<bool>,
) -> Result<(), String> {
    let target = crate::commands::blocking(move || {
        let mut q = crate::lock(queue());
        expire_stale_pending(&mut q)?;
        let operation = if restore_materials == Some(true) {
            RecordOperation::Restore
        } else {
            RecordOperation::Open
        };
        queue_record(&mut q, load_records()?, &handoff_id, operation)
    })
    .await?;
    if let Some(target) = target {
        dispatch(&app, target);
    }
    Ok(())
}

/// 先删材料，再移除记录。发生 I/O 错误时保留用户可见的记录与重试入口。
#[tauri::command]
pub(crate) async fn hooks_analysis_delete(handoff_id: String) -> Result<(), String> {
    crate::commands::blocking(move || {
        let mut q = crate::lock(queue());
        // 其他包仍可清理，例如磁盘满导致回执暂时落不了盘时。
        let _ = expire_stale_pending(&mut q);
        delete_record(&q, &handoff_id)
    })
    .await
}

fn delete_record(q: &AnalysisQueue, handoff_id: &str) -> Result<(), String> {
    if !valid_segment(handoff_id) {
        return Err("分析记录 ID 非法".into());
    }
    if q.pending
        .as_ref()
        .is_some_and(|target| target.handoff_id == handoff_id)
    {
        return Err("这份材料正在交接，请等待结果后重试".into());
    }
    let mut records = load_records()?;
    if !records.iter().any(|record| record.handoff_id == handoff_id) {
        return Err("交接记录不存在".into());
    }
    if let Err(error) = fs::remove_dir_all(package_dir(handoff_id)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("删除材料包失败，交接记录仍保留：{error}"));
        }
    }
    records.retain(|record| record.handoff_id != handoff_id);
    save_records(&records)
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

type SessionListAnswer = tokio::sync::oneshot::Sender<Option<Vec<SessionItem>>>;

#[tauri::command]
pub(crate) async fn hooks_analysis_sessions(app: AppHandle) -> Result<SessionsView, String> {
    let unavailable = |reason: &str| SessionsView {
        available: false,
        reason: reason.into(),
        items: Vec::new(),
    };
    let Some(window) = app.get_webview("harness") else {
        return Ok(unavailable("工作台未打开，请先打开工作台或选择新建会话"));
    };
    let profile = current_target_env().2;
    let runtime_app = app.clone();
    if let Err(error) =
        crate::commands::blocking(move || verify_running_profile(&runtime_app, &profile)).await
    {
        return Ok(unavailable(&error));
    }
    let query_id = format!("haq_{:016x}", rand::random::<u64>());
    let (send, receive) = tokio::sync::oneshot::channel();
    crate::lock(queue()).sessions.insert(query_id.clone(), send);
    let dispatched = window.eval(format!(
        "window.dispatchEvent(new CustomEvent('dsh-xlink-hook-analysis-sessions',{{detail:{{queryId:'{query_id}'}}}}));"
    ));
    let result = match dispatched {
        Ok(()) => match tokio::time::timeout(SESSIONS_BRIDGE_TIMEOUT, receive).await {
            Ok(Ok(Some(items))) => Ok(SessionsView {
                available: true,
                reason: String::new(),
                items,
            }),
            _ => Ok(unavailable("工作台会话列表未就绪，请重试或选择新建会话")),
        },
        Err(error) => Err(format!("向工作台请求会话列表失败：{error}")),
    };
    crate::lock(queue()).sessions.remove(&query_id);
    result
}

#[tauri::command]
pub(crate) fn hooks_analysis_sessions_report(
    window: Webview,
    query_id: String,
    items: Option<Vec<SessionItem>>,
) -> Result<(), String> {
    require_harness(&window)?;
    if let Some(sender) = crate::lock(queue()).sessions.remove(&query_id) {
        let _ = sender.send(items);
    }
    Ok(())
}

fn require_harness(window: &Webview) -> Result<(), String> {
    if window.label() != "harness" {
        return Err("只有工作台可以操作分析交接".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn hooks_analysis_take(window: Webview) -> Result<Option<AnalysisTarget>, String> {
    require_harness(&window)?;
    let app = window.app_handle().clone();
    crate::commands::blocking(move || -> Result<_, String> {
        let mut q = crate::lock(queue());
        expire_stale_pending(&mut q)?;
        let Some(target) = q
            .pending
            .as_ref()
            .filter(|target| target_env_matches(target))
        else {
            return Ok(None);
        };
        verify_running_profile(&app, &target.profile)?;
        Ok(Some(target.clone()))
    })
    .await
}

#[tauri::command]
pub(crate) async fn hooks_analysis_checkpoint(
    window: Webview,
    sequence: u64,
    handoff_id: String,
    session_id: Option<String>,
    writing_draft: Option<bool>,
) -> Result<(), String> {
    require_harness(&window)?;
    let app = window.app_handle().clone();
    crate::commands::blocking(move || -> Result<_, String> {
        let mut q = crate::lock(queue());
        expire_stale_pending(&mut q)?;
        let Some(target) =
            pending_for(&q, sequence, &handoff_id).filter(|target| target_env_matches(target))
        else {
            return Err("交接已失效或目标已变更，未修改草稿".into());
        };
        verify_running_profile(&app, &target.profile)?;
        if let Some(session_id) = session_id {
            checkpoint_session(target, &session_id, writing_draft == Some(true))?;
            if let Some(pending) = q.pending.as_mut() {
                pending.session_id = Some(session_id);
            }
        }
        Ok(())
    })
    .await
}

fn checkpoint_session(
    target: &AnalysisTarget,
    session_id: &str,
    writing_draft: bool,
) -> Result<(), String> {
    if target
        .session_id
        .as_deref()
        .or(target.reserved_session_id.as_deref())
        != Some(session_id)
    {
        return Err("工作台返回的会话与分析目标不一致".into());
    }
    let mut records = load_records()?;
    let record = records
        .iter_mut()
        .find(|record| record.handoff_id == target.handoff_id)
        .ok_or("原交接记录不存在")?;
    if record.session_id.as_deref() != Some(session_id)
        || (writing_draft && !record.draft_write_started)
    {
        record.session_id = Some(session_id.into());
        record.draft_write_started |= writing_draft;
        save_records(&records)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn hooks_analysis_report(
    window: Webview,
    sequence: u64,
    handoff_id: String,
    session_id: Option<String>,
    error: String,
) -> Result<(), String> {
    require_harness(&window)?;
    let app = window.app_handle().clone();
    crate::commands::blocking(move || -> Result<_, String> {
        let mut q = crate::lock(queue());
        expire_stale_pending(&mut q)?;
        let Some(target) =
            pending_for(&q, sequence, &handoff_id).filter(|target| target_env_matches(target))
        else {
            return Ok(());
        };
        verify_running_profile(&app, &target.profile)?;
        if error.is_empty() && (session_id.is_none() || session_id != target.session_id) {
            return Err("工作台确认的会话与交接目标不一致".into());
        }
        settle_pending(&mut q, &handoff_id, session_id, &error)
    })
    .await
}

struct HandoffReceipt {
    handoff_id: String,
    session_id: Option<String>,
    error: String,
    opened_only: bool,
}

fn settle_pending(
    q: &mut AnalysisQueue,
    handoff_id: &str,
    session_id: Option<String>,
    error: &str,
) -> Result<(), String> {
    q.receipt = Some(HandoffReceipt {
        handoff_id: handoff_id.into(),
        session_id,
        error: error.into(),
        opened_only: q
            .pending
            .as_ref()
            .is_some_and(|target| target.action == "open"),
    });
    flush_receipt(q)
}

fn flush_receipt(q: &mut AnalysisQueue) -> Result<(), String> {
    if let Some(receipt) = q.receipt.as_ref() {
        apply_report(
            &receipt.handoff_id,
            receipt.session_id.clone(),
            &receipt.error,
            receipt.opened_only,
        )
        .map_err(|error| format!("工作台回执持久化失败，交接仍保留待重试：{error}"))?;
        q.receipt = None;
        q.pending = None;
    }
    Ok(())
}

/// 工作台确认落账:attached 只在工作台确认草稿就位后写;不宣称 sent。
fn apply_report(
    handoff_id: &str,
    session_id: Option<String>,
    error: &str,
    opened_only: bool,
) -> Result<(), String> {
    let mut records = load_records()?;
    let record = records
        .iter_mut()
        .find(|record| record.handoff_id == handoff_id)
        .ok_or_else(|| "交接记录不存在（可能已被删除）".to_string())?;
    if error.is_empty() {
        record.attached_once |= !opened_only;
        record.state = if record.attached_once {
            "attached"
        } else {
            "unconfirmed"
        }
        .into();
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
        let mut q = AnalysisQueue::default();
        let (ack, target) = prepare_handoff(&mut q, request(&case, true)).expect("建包");
        let target = target.unwrap();
        assert!(target.sequence > 0);
        assert!(target.question.is_some());
        assert!(target.materials.contains("line2 warn"));
        assert!(target.materials.contains(&material_marker(&ack.handoff_id)));
        assert!(package_dir(&ack.handoff_id).join("m-body.txt").is_file());
        assert!(Path::new(&ack.workspace_path).is_dir());
        let reserved = target.reserved_session_id.clone().unwrap();
        assert_eq!(
            load_records().unwrap()[0].reserved_session_id.as_deref(),
            Some(reserved.as_str())
        );

        let (again, dispatch) = prepare_handoff(&mut q, request(&case, true)).expect("重复请求");
        assert_eq!(again.handoff_id, ack.handoff_id);
        assert!(dispatch.is_none());
        assert_eq!(q.pending.as_ref().unwrap().sequence, target.sequence);
        let altered = prepare_handoff(&mut q, request(&case, false)).unwrap_err();
        assert!(altered.contains("材料或目标已改变"));

        checkpoint_session(&target, &reserved, false).unwrap();
        settle_pending(&mut q, &ack.handoff_id, None, "draft blocked").unwrap();
        let (again, dispatch) = prepare_handoff(&mut q, request(&case, true)).unwrap();
        assert_eq!(again.handoff_id, ack.handoff_id);
        let retry = dispatch.unwrap();
        assert_eq!(retry.session_id.as_deref(), Some(reserved.as_str()));
        assert_eq!(
            retry.reserved_session_id.as_deref(),
            Some(reserved.as_str())
        );
        assert!(retry.sequence > target.sequence);

        settle_pending(&mut q, &ack.handoff_id, Some(reserved), "").unwrap();
        let (confirmed, dispatch) = prepare_handoff(&mut q, request(&case, true)).unwrap();
        assert_eq!(confirmed.handoff_id, ack.handoff_id);
        assert!(dispatch.is_none());
        assert_eq!(load_records().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_retries_publish_once_and_keep_other_attempts_out() {
        let (case, _guard) = seed("正文", "log\n");
        let q = Mutex::new(AnalysisQueue::default());
        let barrier = std::sync::Barrier::new(2);
        let (first, second) = std::thread::scope(|scope| {
            let submit = || {
                barrier.wait();
                prepare_handoff(&mut crate::lock(&q), request(&case, true)).unwrap()
            };
            let first = scope.spawn(submit);
            let second = submit();
            (first.join().unwrap(), second)
        });
        assert_eq!(first.0.handoff_id, second.0.handoff_id);
        assert_ne!(first.1.is_some(), second.1.is_some());
        let mut other = request(&case, true);
        other.request_id = "other".into();
        assert!(prepare_handoff(&mut crate::lock(&q), other)
            .unwrap_err()
            .contains("另一份"));
        assert_eq!(load_records().unwrap().len(), 1);
        assert_eq!(
            fs::read_dir(packages_dir())
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name() != ".tmp")
                .count(),
            1
        );
    }

    #[test]
    fn line_segments_verify_original_and_restored_bytes_and_report_actual_range() {
        let log = "one\r\n二\r\nthree\nfour\n";
        let (case, _guard) = seed("正文", log);
        let mut req = request(&case, false);
        req.logs[0].start_line = Some(2);
        req.logs[0].end_line = Some(99);
        let (ack, target) = prepare_handoff(&mut AnalysisQueue::default(), req).unwrap();
        let dir = package_dir(&ack.handoff_id);
        let manifest: PackageManifest =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        let entry = &manifest.materials[0];
        assert_eq!(entry.line_range, Some((2, 4)));
        assert!(!entry.complete);
        let original_sha256 = crate::shell::digest::hex(Sha256::digest(log.as_bytes()));
        assert_eq!(
            entry.original_sha256.as_deref(),
            Some(original_sha256.as_str())
        );
        let saved = dir.join(material_file_name(&entry.internal_id));
        assert_eq!(fs::read_to_string(&saved).unwrap(), "二\r\nthree\nfour\n");
        let draft = target.unwrap().materials;
        assert!(draft.contains("原文件第 2-4 行"));
        assert!(draft.contains(&original_sha256));

        fs::write(&saved, "二\r\nthree\nfail\n").unwrap();
        assert!(material_blocks(&ack.handoff_id, &dir)
            .unwrap_err()
            .contains("SHA-256"));
        let con = open_store().unwrap();
        let attachment = crate::hooks::store::event_attachments(&con, &case.event_id)
            .unwrap()
            .remove(0);
        let source = blob::path_for_rel(attachment.storage_rel.as_deref().unwrap()).unwrap();
        fs::write(source, "one\r\n二\r\nthree\nfail\n").unwrap();
        assert!(read_log_material(&attachment, Some((2, 3)))
            .err()
            .unwrap()
            .contains("SHA-256"));
    }

    #[test]
    fn size_or_workspace_rejection_keeps_no_published_package() {
        let big = "x".repeat(600 * 1024);
        let (case, _guard) = seed(&big, &big);
        let mut q = AnalysisQueue::default();
        let error = prepare_handoff(&mut q, request(&case, true)).unwrap_err();
        assert!(error.contains("上限"), "{error}");
        let mut invalid = request(&case, false);
        invalid.workspace.path = Some("relative-path".into());
        assert!(prepare_handoff(&mut q, invalid)
            .unwrap_err()
            .contains("绝对路径"));
        assert!(!packages_dir().exists());
        assert!(load_records().unwrap().is_empty());
    }

    #[test]
    fn receipt_is_kept_until_storage_is_repaired_and_stale_reports_cannot_match() {
        let (case, _guard) = seed("正文", "log\n");
        let mut q = AnalysisQueue::default();
        let (ack, target) = prepare_handoff(&mut q, request(&case, true)).unwrap();
        let target = target.unwrap();
        let session_id = target.reserved_session_id.clone().unwrap();
        assert!(checkpoint_session(&target, "wrong-session", false).is_err());
        checkpoint_session(&target, &session_id, true).unwrap();
        let saved = handoffs_file().with_extension("saved");
        fs::rename(handoffs_file(), &saved).unwrap();
        fs::create_dir(handoffs_file()).unwrap();
        assert!(settle_pending(&mut q, &ack.handoff_id, Some(session_id.clone()), "").is_err());
        assert_eq!(
            read_status(&mut q, &ack.handoff_id).unwrap().state,
            "receipt-pending"
        );
        assert!(q.receipt.is_some());
        assert!(q.pending.is_some());
        fs::remove_dir(handoffs_file()).unwrap();
        fs::rename(saved, handoffs_file()).unwrap();
        let status = read_status(&mut q, &ack.handoff_id).unwrap();
        assert_eq!(status.state, "attached");
        assert_eq!(status.session_id, Some(session_id));
        assert!(q.pending.is_none());
        assert!(q.receipt.is_none());
        assert!(pending_for(&q, target.sequence, &ack.handoff_id).is_none());

        let open = queue_record(
            &mut q,
            load_records().unwrap(),
            &ack.handoff_id,
            RecordOperation::Open,
        )
        .unwrap()
        .unwrap();
        assert_eq!(open.action, "open");
        assert!(open.materials.is_empty());
        assert!(pending_for(&q, open.sequence, "wrong-handoff").is_none());
        assert!(pending_for(&q, target.sequence, &ack.handoff_id).is_none());
    }

    #[test]
    fn restart_after_draft_checkpoint_only_opens_until_explicit_restore() {
        let (case, _guard) = seed("正文", "log\n");
        let (ack, first) =
            prepare_handoff(&mut AnalysisQueue::default(), request(&case, true)).unwrap();
        let first = first.unwrap();
        let id = first.reserved_session_id.clone().unwrap();
        checkpoint_session(&first, &id, true).unwrap();

        let mut restarted = AnalysisQueue::default();
        let (same, dispatch) = prepare_handoff(&mut restarted, request(&case, true)).unwrap();
        assert_eq!(same.handoff_id, ack.handoff_id);
        let open = dispatch.unwrap();
        assert_eq!(open.action, "open");
        assert!(open.materials.is_empty());
        settle_pending(&mut restarted, &ack.handoff_id, Some(id), "").unwrap();
        assert_eq!(
            read_status(&mut restarted, &ack.handoff_id).unwrap().state,
            "unconfirmed"
        );

        let restore = queue_record(
            &mut restarted,
            load_records().unwrap(),
            &ack.handoff_id,
            RecordOperation::Restore,
        )
        .unwrap()
        .unwrap();
        assert_eq!(restore.action, "prepare-existing");
        assert!(restore.materials.contains("log"));
    }

    #[test]
    fn target_requires_original_profile() {
        let (case, _guard) = seed("正文", "log\n");
        let (family, instance, profile, shell_mode) = current_target_env();
        let record = HandoffRecord {
            family,
            instance,
            profile,
            shell_mode,
            ..Default::default()
        };
        assert!(target_compatible(&record));
        let switched = HandoffRecord {
            profile: "another-profile".into(),
            ..record
        };
        assert!(!target_compatible(&switched));
        assert!(ensure_record_target(&switched)
            .unwrap_err()
            .contains("another-profile"));
        assert!(case.home.is_dir());
    }

    #[test]
    fn source_deletion_keeps_snapshot_retriable_and_globally_manageable() {
        let (case, _guard) = seed("正文", "log\n");
        let mut q = AnalysisQueue::default();
        let (ack, _) = prepare_handoff(&mut q, request(&case, true)).unwrap();
        settle_pending(&mut q, &ack.handoff_id, None, "draft blocked").unwrap();
        let mut another = request(&case, true);
        another.request_id = "another".into();
        let (other, _) = prepare_handoff(&mut q, another).unwrap();
        assert!(delete_record(&q, &other.handoff_id).is_err());

        let mut con = open_store().unwrap();
        for rel in crate::hooks::store::delete_event(&mut con, &case.event_id).unwrap() {
            blob::remove_rel(&rel).unwrap();
        }
        assert!(
            material_blocks(&ack.handoff_id, &package_dir(&ack.handoff_id))
                .unwrap()
                .contains("log")
        );
        assert_eq!(build_plan(None).unwrap().handoffs.len(), 2);
        assert!(build_plan(Some(&case.event_id))
            .unwrap()
            .materials
            .is_empty());
        settle_pending(&mut q, &other.handoff_id, None, "draft blocked").unwrap();
        let (retry, dispatched) = prepare_handoff(&mut q, request(&case, true)).unwrap();
        assert_eq!(retry.handoff_id, ack.handoff_id);
        assert!(dispatched.is_some());
        settle_pending(&mut q, &ack.handoff_id, None, "draft blocked").unwrap();
        delete_record(&q, &ack.handoff_id).unwrap();
        assert!(!package_dir(&ack.handoff_id).exists());
        assert!(package_dir(&other.handoff_id).exists());
        assert_eq!(
            build_plan(None).unwrap().handoffs[0].handoff_id,
            other.handoff_id
        );
    }

    #[test]
    fn extract_lines_clamps_and_rejects_bad_ranges() {
        let bytes = b"one\ntwo\nthree\n";
        assert_eq!(
            extract_lines(bytes, 2, 99, "l").unwrap(),
            (b"two\nthree\n".to_vec(), 3)
        );
        assert_eq!(
            extract_lines(bytes, 1, 2, "l").unwrap(),
            (b"one\ntwo\n".to_vec(), 2)
        );
        assert!(extract_lines(bytes, 0, 2, "l").is_err());
        assert!(extract_lines(bytes, 3, 2, "l").is_err());
        assert!(extract_lines(bytes, 9, 12, "l").is_err());
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
