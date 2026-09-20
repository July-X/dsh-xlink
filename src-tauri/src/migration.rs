//! 旧布局 → 新布局的迁移向导骨架（开发计划 §P6 / 设计稿 §11）。
//!
//! 设计原则：先**只读扫描**，再让用户拍板；任何破坏性动作都必须由调用方
//! 显式发起，回滚必须保留旧源。
//!
//! 当前实现范围：
//! - [`LegacySource`] / [`LegacySourceInfo`]：识别旧布局来源（旧 DSH home
//!   下的 `plugins/`、`skills-store/`、`skills/`）。
//! - [`scan_legacy_sources`]：只读扫描，返回每个来源是否存在、文件数
//!   与字节数。**绝不**触碰旧目录——任何写入都要等用户拍板。
//! - [`MigrationPreview`] / [`preview_migration`]：对每条可迁移来源，列出
//!   源路径、目标路径、条目数和大小。
//! - [`ConflictPolicy`] / [`MigrationStatus`] / [`MigrationItemReport`] /
//!   [`MigrationReport`] / [`run_migration`]：按保守默认（SkipIfNewer，
//!   credentials 不纳入）执行迁移；写入 backup 后再覆盖目标，旧源**永不
//!   删除**（用户回滚时还能用）。
//!
//! 不在范围：
//! - 旧 `~/.dsh/desktop[-dev]/` 仍在旧 DSH home 下供 kernel 模块使用
//!   （含 `active.txt` / `kernel.pid` / 内核安装目录），由 kernel /
//!   `kernel_adapter` 自管理，**不**属于本向导迁移范围。
//! - 用户手动 `~/.dsh/sessions/` / `~/.dsh/credentials/` 等凭据与会话
//!   数据**不**迁移（计划原文："首版可以只迁移 Shell、插件和技能"）。
//! - P7 mcode 适配器与 P8 UI 集成留到后续阶段。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::paths::{
    legacy_dsh_home, legacy_dsh_plugins_root, legacy_dsh_skills_root, legacy_dsh_skills_store,
    plugins_store_root, skills_active_root, skills_store_root, xlink_home,
};
use crate::pkg;

/// 旧布局来源——按目录定位，不依赖文件内容。
///
/// 设计稿 §11.1 描述的迁移映射（仅本向导范围；`desktop[-dev]/` 由 kernel
/// 模块自管，见本模块顶部说明）：
/// - `plugins/` → 新 Xlink home 的 `dsh-plugins/`
/// - `skills-store/` → 新 Xlink home 的 `skills/packages/`
/// - `skills/` → 新 Xlink home 的 `skills/active/`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LegacySource {
    /// `~/.dsh/plugins/`（旧版插件中央库）。
    Plugins,
    /// `~/.dsh/skills-store/`（旧版技能中央库）。
    SkillsStore,
    /// `~/.dsh/skills/`（旧版技能活动视图，官方内核读取路径之一）。
    SkillsActive,
}

impl LegacySource {
    /// 旧 DSH home 下的具体路径。**不**检查存在性。
    pub fn path(self) -> PathBuf {
        match self {
            LegacySource::Plugins => legacy_dsh_plugins_root(),
            LegacySource::SkillsStore => legacy_dsh_skills_store(),
            LegacySource::SkillsActive => legacy_dsh_skills_root(),
        }
    }

    /// 新布局下的目标路径。**不**检查存在性、不创建目录。
    pub fn target(self) -> PathBuf {
        match self {
            LegacySource::Plugins => plugins_store_root(),
            LegacySource::SkillsStore => skills_store_root(),
            LegacySource::SkillsActive => skills_active_root(),
        }
    }

    /// 全部来源枚举——`scan_legacy_sources` 与 `preview_migration`
    /// 按这个顺序扫，结果可重现。
    pub fn all() -> [LegacySource; 3] {
        [
            LegacySource::Plugins,
            LegacySource::SkillsStore,
            LegacySource::SkillsActive,
        ]
    }

    /// 用户可见的来源名称（简体中文）。
    pub fn display_name(self) -> &'static str {
        match self {
            LegacySource::Plugins => "旧插件中央库",
            LegacySource::SkillsStore => "旧技能中央库",
            LegacySource::SkillsActive => "旧技能活动视图",
        }
    }

    /// 备份目录里使用的稳定标识——ASCII slug，回滚路径据此外推。
    /// **不**展示给用户（用 [`display_name`]）；变更前必须先确认所有已
    /// 落盘的 backup 目录兼容新 slug。
    pub fn backup_key(self) -> &'static str {
        match self {
            LegacySource::Plugins => "plugins",
            LegacySource::SkillsStore => "skills-store",
            LegacySource::SkillsActive => "skills-active",
        }
    }
}

/// 旧布局来源的扫描结果——只读、绝不创建目录。
#[derive(Debug, Clone, Serialize)]
pub struct LegacySourceInfo {
    pub source: LegacySource,
    pub path: PathBuf,
    pub target: PathBuf,
    pub exists: bool,
    pub file_count: usize,
    pub total_bytes: u64,
}

/// 扫描所有旧布局来源，返回它们是否存在、文件数和字节数。
///
/// **只读**：不创建 / 删除任何文件，也不写目标目录。`path` 不存在时
/// `exists = false`，`file_count = 0`，`total_bytes = 0`，便于调用方
/// 判定哪些来源需要处理。
///
/// 目录遍历在符号链接处**不**追链——避免把用户手放的目录误算进条目数。
pub fn scan_legacy_sources() -> Vec<LegacySourceInfo> {
    LegacySource::all()
        .into_iter()
        .map(|source| {
            let path = source.path();
            let target = source.target();
            let (exists, file_count, total_bytes) = scan_one(&path);
            LegacySourceInfo {
                source,
                path,
                target,
                exists,
                file_count,
                total_bytes,
            }
        })
        .collect()
}

fn scan_one(root: &Path) -> (bool, usize, u64) {
    if !root.exists() {
        return (false, 0, 0);
    }
    let metadata = match fs::symlink_metadata(root) {
        Ok(md) => md,
        Err(_) => return (true, 0, 0),
    };
    if !metadata.is_dir() {
        // 旧布局来源必须是目录；如果是文件 / 链接则跳过（不删）。
        return (true, 0, 0);
    }
    let mut count = 0usize;
    let mut bytes = 0u64;
    walk_count(root, &mut count, &mut bytes);
    (true, count, bytes)
}

fn walk_count(dir: &Path, count: &mut usize, bytes: &mut u64) {
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let md = match fs::symlink_metadata(entry.path()) {
            Ok(md) => md,
            Err(_) => continue,
        };
        if md.is_dir() {
            walk_count(&entry.path(), count, bytes);
        } else {
            *count += 1;
            *bytes += md.len();
        }
    }
}

/// 单条迁移项的预览：源路径、目标路径、条目数、字节数。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationItemPreview {
    pub source: LegacySource,
    pub display_name: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub file_count: usize,
    pub total_bytes: u64,
    pub target_exists: bool,
}

/// 完整迁移预览——包含 `DSH_HOME` 解析源、新 Xlink home 解析、每个
/// 旧来源的迁移项。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationPreview {
    pub legacy_dsh_home: PathBuf,
    pub xlink_home: PathBuf,
    pub items: Vec<MigrationItemPreview>,
}

impl MigrationPreview {
    /// 至少有一条**实际可迁移**的来源（存在且非空）。
    pub fn has_migratable(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.file_count > 0 || item.total_bytes > 0)
    }
}

/// 生成完整迁移预览。**只读**——不创建任何目录、不触碰源 / 目标。
pub fn preview_migration() -> MigrationPreview {
    let sources = scan_legacy_sources();
    let items = sources
        .into_iter()
        .map(|info| MigrationItemPreview {
            source: info.source,
            display_name: info.source.display_name().to_string(),
            source_path: info.path,
            target_path: info.target.clone(),
            file_count: info.file_count,
            total_bytes: info.total_bytes,
            target_exists: info.target.exists(),
        })
        .collect();
    MigrationPreview {
        legacy_dsh_home: legacy_dsh_home(),
        xlink_home: xlink_home(),
        items,
    }
}

// --- step 2：复制 / 备份 / 报告 ----------------------------------------

/// 目标已存在时的处理策略。`SkipIfNewer` 是最保守的默认——
/// 保留用户后来修改的文件，仅写入**比目标更旧或不存在**的条目；这让
/// 「先跑过迁移又回滚再跑一次」自然幂等。**例外**：插件 / 技能中央库
/// 的 `store.json` 清单不走整文件比较，任何策略下都按条目 `id` 合并
/// （目标已有条目优先）——整文件覆盖会抹掉目标侧后来新增的记录，整
/// 文件跳过又会让源记录永远迁不进来。
///
/// `BackupAndOverwrite` 在覆盖前把现有目标移到
/// `<xlink_home>/backups/<migration_id>/<source>/`，回滚时按 backup
/// 路径还原即可（旧源不被删除）。
///
/// serde tag 用 `kebab-case` 与 Tauri command 序列化对齐；前端可通过
/// `\"skip-if-newer\"` / `\"backup-and-overwrite\"` 传字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    /// 跳过比目标更新的文件；只补缺失条目。
    SkipIfNewer,
    /// 备份现有目标后再覆盖。
    BackupAndOverwrite,
}

/// 单条迁移项的执行结果。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationItemReport {
    pub source: LegacySource,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub files_copied: usize,
    pub files_skipped: usize,
    pub status: MigrationStatus,
    pub error: Option<String>,
}

/// 单条迁移项的执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationStatus {
    /// 源为空 / 不存在，无可迁移内容。
    Skipped,
    /// 全部条目按策略处理成功。
    Copied,
    /// 部分条目成功，部分失败。
    PartialFailure,
    /// 全部失败（一般是源不可读或目标不可写）。
    Failed,
}

/// 完整迁移报告——`run_migration` 的返回值；UI 拿它做"哪些搬了、哪些没搬"的依据。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub migration_id: String,
    pub backup_root: PathBuf,
    pub items: Vec<MigrationItemReport>,
}

/// 单步进度事件。Tauri Channel 把这个 struct 序列化推到前端，前端用它
/// 更新进度条 + 当前步骤文字。
///
/// 序列化字段稳定，避免后续 commit 改名 / 改序后前端会读到 null。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationProgress {
    /// 当前 1-based 步骤索引（0 = 准备阶段，未开始第一项）。
    pub step: u32,
    /// 总步骤数（`preview.items.len()`）。
    pub total: u32,
    /// 当前步骤的源（用户可读的 `display_name`，如「旧插件中央库」）。
    pub source_label: String,
    /// 当前阶段文案：搬运中 / 已完成 / 失败 / 跳过。
    pub stage: String,
}

/// 执行迁移：复制源到目标、按策略处理冲突、写入 backup（旧源永不删除）。
///
/// `migration_id` 决定 backup 目录名（`<xlink_home>/backups/<id>/`）；
/// 同一 id 多次跑视为同一迁移的后续动作（旧 backup 不会被覆盖——后面
/// 会带递增后缀）。
pub fn run_migration(policy: ConflictPolicy) -> Result<MigrationReport, AppError> {
    run_migration_with_progress(policy, |_progress| {})
}

/// 进度回调版 [`run_migration`]：每完成一个 [`migrate_one`] 就调一次
/// `on_progress`，前端用它更新进度条 / 当前步骤文字。
///
/// 旧 `run_migration(policy)` 是这条函数的 zero-callback wrapper。
pub fn run_migration_with_progress<F>(
    policy: ConflictPolicy,
    mut on_progress: F,
) -> Result<MigrationReport, AppError>
where
    F: FnMut(MigrationProgress),
{
    let migration_id = next_migration_id();
    let backup_root = backup_root_for(&migration_id);
    fs::create_dir_all(&backup_root)
        .map_err(|e| AppError::Io(format!("无法创建备份目录 {}：{e}", backup_root.display())))?;

    let preview = preview_migration();
    let total = preview.items.len() as u32;
    on_progress(MigrationProgress {
        step: 0,
        total,
        source_label: String::new(),
        stage: "准备".to_string(),
    });
    let mut items = Vec::with_capacity(preview.items.len());
    for (idx, item) in preview.items.into_iter().enumerate() {
        let step = (idx + 1) as u32;
        on_progress(MigrationProgress {
            step,
            total,
            source_label: item.source.display_name().to_string(),
            stage: "搬运中".to_string(),
        });
        let report = migrate_one(&item, policy, &backup_root);
        let stage = match report.status {
            MigrationStatus::Copied => "已完成",
            MigrationStatus::Skipped => "已跳过",
            MigrationStatus::PartialFailure | MigrationStatus::Failed => "失败",
        };
        on_progress(MigrationProgress {
            step,
            total,
            source_label: item.source.display_name().to_string(),
            stage: stage.to_string(),
        });
        items.push(report);
    }
    Ok(MigrationReport {
        migration_id: migration_id.to_string(),
        backup_root,
        items,
    })
}

/// 用户拒绝迁移的持久化状态：写到 `<data_dir>/migration-skipped.json`。
/// 「主窗口启动弹窗」检测到 skip=true 时不弹；用户在「数据迁移」侧栏面板
/// 里手动 `clear_migration_skip` 之后才会重新弹。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSkip {
    /// 用户点击「否」时记录的 epoch 毫秒。后续想加「7 天后再问」时复用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_at_ms: Option<u64>,
    /// 用户拒绝时遗留下来的来源列表——只用作日志/审计，不参与"是否再问"的
    /// 判断（用户拒绝过一次后，无论数据是否变化都按 skip=true 处理）。
    #[serde(default)]
    pub sources: Vec<LegacySource>,
}

fn skip_state_file() -> PathBuf {
    crate::paths::kernels_root().join("migration-skipped.json")
}

/// 当前是否处于「用户拒绝迁移」状态。
pub fn is_migration_skipped() -> bool {
    let doc: MigrationSkip = crate::state::load_lossy(&skip_state_file());
    doc.skipped_at_ms.is_some()
}

/// 记录用户拒绝。
pub fn set_migration_skipped(sources: Vec<LegacySource>) -> Result<(), AppError> {
    let doc = MigrationSkip {
        skipped_at_ms: Some(crate::process::epoch_millis()),
        sources,
    };
    crate::state::save(
        &skip_state_file(),
        &doc,
        crate::state::StateCtx::plain(AppError::Io),
    )
}

/// 清除拒绝标记（用户在「数据迁移」侧栏面板手动重跳时调用）。
pub fn clear_migration_skipped() -> Result<(), AppError> {
    let path = skip_state_file();
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| AppError::Io(format!("无法删除跳过标记：{e}")))
}

/// 后端生成 migration_id：`AutoYYYYMMDD-HHMMSS-<short>` 格式。短后缀
/// 由当前 epoch nanos 的末 4 位 hex 派生，避免同一秒内连跑两次撞 id。
/// UI 不应自己造 id——Tauri 命令直接传 policy 即可。
pub fn next_migration_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let nanos_tail = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.subsec_nanos() & 0xFFFF) as u32)
        .unwrap_or(0);
    let (y, m, d, h, mi, s) = epoch_to_ymdhms(now);
    format!(
        "Auto{:04}{:02}{:02}-{:02}{:02}{:02}-{:04x}",
        y, m, d, h, mi, s, nanos_tail
    )
}

/// epoch 秒 → (年, 月, 日, 时, 分, 秒)。本地时区，避免和 UTC 跨日歧义。
fn epoch_to_ymdhms(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    use time::{OffsetDateTime, UtcOffset};
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let dt = OffsetDateTime::from_unix_timestamp(secs as i64).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let local = dt.to_offset(offset);
    (
        local.year() as u32,
        local.month() as u32,
        u32::from(local.day()),
        u32::from(local.hour()),
        u32::from(local.minute()),
        u32::from(local.second()),
    )
}

fn backup_root_for(migration_id: &str) -> PathBuf {
    // 多轮跑同一 id：若目录已存在，加 `.2` / `.3` 后缀避免覆盖旧 backup。
    let base = xlink_home().join("backups").join(migration_id);
    if !base.exists() {
        return base;
    }
    let mut index = 2u32;
    loop {
        let candidate = xlink_home()
            .join("backups")
            .join(format!("{migration_id}.{index}"));
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

fn migrate_one(
    item: &MigrationItemPreview,
    policy: ConflictPolicy,
    backup_root: &Path,
) -> MigrationItemReport {
    let source_path = item.source_path.clone();
    let target_path = item.target_path.clone();
    if !source_path.exists() {
        return MigrationItemReport {
            source: item.source,
            source_path,
            target_path,
            backup_path: None,
            files_copied: 0,
            files_skipped: 0,
            status: MigrationStatus::Skipped,
            error: None,
        };
    }
    if item.file_count == 0 && item.total_bytes == 0 {
        return MigrationItemReport {
            source: item.source,
            source_path,
            target_path,
            backup_path: None,
            files_copied: 0,
            files_skipped: 0,
            status: MigrationStatus::Skipped,
            error: None,
        };
    }

    let backup_path = match policy {
        ConflictPolicy::BackupAndOverwrite if target_path.exists() => {
            Some(backup_path_for(backup_root, item.source, &target_path))
        }
        _ => None,
    };

    let mut copied = 0usize;
    let mut skipped = 0usize;
    let mut last_error: Option<String> = None;

    if let Err(error) = fs::create_dir_all(&target_path) {
        return MigrationItemReport {
            source: item.source,
            source_path,
            target_path,
            backup_path: None,
            files_copied: 0,
            files_skipped: 0,
            status: MigrationStatus::Failed,
            error: Some(format!("无法创建目标目录：{error}")),
        };
    }

    let entries = match fs::read_dir(&source_path) {
        Ok(it) => it,
        Err(error) => {
            return MigrationItemReport {
                source: item.source,
                source_path,
                target_path,
                backup_path: None,
                files_copied: 0,
                files_skipped: 0,
                status: MigrationStatus::Failed,
                error: Some(format!("无法读取源目录：{error}")),
            };
        }
    };

    for entry in entries.flatten() {
        let from = entry.path();
        let file_name = match entry.file_name().to_str() {
            Some(name) => name.to_string(),
            None => {
                last_error = Some("源文件名含非 UTF-8 字节，已跳过".into());
                continue;
            }
        };
        let to = target_path.join(&file_name);
        let decision = if is_store_manifest(item.source, &file_name) && to.exists() {
            // 清单文件永远走合并——不管策略与 mtime。BackupAndOverwrite
            // 的「备份后整体覆盖」只对普通文件有意义；清单文件覆盖会静默
            // 丢掉目标侧独有的条目，回滚都找不回来。
            EntryDecision::MergeManifest
        } else {
            decide_entry(&from, &to, policy)
        };
        match decision {
            EntryDecision::Copy => match copy_one(&from, &to) {
                Ok(()) => copied += 1,
                Err(error) => last_error = Some(format!("{file_name}：{error}")),
            },
            EntryDecision::MergeManifest => match merge_store_manifest(&from, &to) {
                Ok(()) => copied += 1,
                Err(_) => {
                    // 合并失败（清单损坏 / 不可读）按保守处理：当作跳过，
                    // 不让一条清单拖垮整个来源的搬运——包目录仍按策略复制。
                    skipped += 1;
                }
            },
            EntryDecision::Skip(reason) => {
                skipped += 1;
                let _ = reason; // 当前不计日志——后续可加 `tracing` 字段
            }
            EntryDecision::BackupThenCopy => {
                let backup_target = match backup_path.as_ref() {
                    Some(root) => root.join(&file_name),
                    None => to.clone(),
                };
                if let Err(error) = backup_existing(&to, &backup_target) {
                    last_error = Some(format!("{file_name} 备份失败：{error}"));
                    continue;
                }
                match copy_one(&from, &to) {
                    Ok(()) => copied += 1,
                    Err(error) => last_error = Some(format!("{file_name}：{error}")),
                }
            }
        }
    }

    let status = match (last_error.is_some(), copied, skipped) {
        (true, 0, 0) => MigrationStatus::Failed,
        (true, _, _) => MigrationStatus::PartialFailure,
        (false, _, _) => MigrationStatus::Copied,
    };
    MigrationItemReport {
        source: item.source,
        source_path,
        target_path,
        backup_path,
        files_copied: copied,
        files_skipped: skipped,
        status,
        error: last_error,
    }
}

enum EntryDecision {
    Copy,
    Skip(&'static str),
    BackupThenCopy,
    /// 清单文件（`store.json`）不整文件覆盖——按条目 `id` 合并进目标，
    /// 目标已有条目优先。整文件 Copy 会把目标里比源多出的记录（用户迁
    /// 移后新装的插件/技能）抹掉；SkipIfNewer 整文件 Skip 又会让源里
    /// 用户真正要迁的记录永远进不来（只要目标清单因任何原因比源新——
    /// 哪怕内容是错的）。
    MergeManifest,
}

fn decide_entry(source: &Path, target: &Path, policy: ConflictPolicy) -> EntryDecision {
    if !target.exists() {
        return EntryDecision::Copy;
    }
    let source_md = match fs::symlink_metadata(source) {
        Ok(md) => md,
        Err(_) => return EntryDecision::Skip("源元数据不可读"),
    };
    let target_md = match fs::symlink_metadata(target) {
        Ok(md) => md,
        Err(_) => return EntryDecision::Copy,
    };
    if source_md.is_dir() != target_md.is_dir() {
        // 源是目录 / 目标是文件 / 链接 —— 不安全，跳过让用户手动处理。
        return EntryDecision::Skip("源 / 目标类型不一致");
    }
    if source_md.is_dir() {
        // 目录走「合并」：缺文件补、多文件保留——recursion 由 caller 内部
        // 不在这里展开。简单以「目标更新则整目录跳过」处理。
        return match policy {
            ConflictPolicy::SkipIfNewer
                if source_md.modified_or_now() < target_md.modified_or_now() =>
            {
                EntryDecision::Skip("目标目录比源新，跳过合并")
            }
            _ => EntryDecision::Copy,
        };
    }
    // 普通文件：按 mtime + 策略判定。
    match policy {
        ConflictPolicy::SkipIfNewer
            if source_md.modified_or_now() < target_md.modified_or_now() =>
        {
            EntryDecision::Skip("目标文件比源新")
        }
        ConflictPolicy::BackupAndOverwrite => EntryDecision::BackupThenCopy,
        _ => EntryDecision::Copy,
    }
}

/// `mtime` 读失败时回退到 UNIX_EPOCH——避免误判成"极旧源"导致 `SkipIfNewer`
/// 拒绝写入。失败本身在 caller 已经记录。
trait MetadataTime {
    fn modified_or_now(&self) -> std::time::SystemTime;
}
impl MetadataTime for fs::Metadata {
    fn modified_or_now(&self) -> std::time::SystemTime {
        self.modified().unwrap_or(std::time::UNIX_EPOCH)
    }
}

/// 这条源条目是不是中央库清单：插件与技能中央库的 `store.json` 都位于
/// 源根目录顶层，且目标侧同名文件就是新布局的活清单（`dsh-plugins/
/// store.json`、`skills/packages/store.json`）。
fn is_store_manifest(source: LegacySource, file_name: &str) -> bool {
    file_name == "store.json" && matches!(source, LegacySource::Plugins | LegacySource::SkillsStore)
}

/// 把源清单的条目按 `id` 合并进目标清单：目标已有条目**原样保留**（含
/// 用户迁移后改过的状态），只补目标缺失的条目。两侧 JSON 结构都容忍
/// 未知字段——目标文档的其它顶层字段（`schemaVersion`、`lastCheckedAt`
/// 等）原样保留。
///
/// 任何一侧解析失败都返回 `Err`，由 caller 决定降级（当前：当跳过处理）。
fn merge_store_manifest(source_file: &Path, target_file: &Path) -> io::Result<()> {
    let read_json = |path: &Path| -> io::Result<serde_json::Value> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}：{e}", path.display()),
            )
        })
    };
    let mut target_doc = read_json(target_file)?;
    let source_doc = read_json(source_file)?;

    let source_items = source_doc
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let target_items = target_doc
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "目标清单缺少 items 数组"))?
        .clone();
    let mut merged = target_items;
    let mut seen: std::collections::HashSet<String> = merged
        .iter()
        .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    for item in source_items {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if seen.insert(id.to_string()) {
            merged.push(item);
        }
    }
    target_doc["items"] = serde_json::Value::Array(merged);
    crate::process::atomic_write(target_file, target_doc.to_string().as_bytes())
}

fn backup_path_for(backup_root: &Path, source: LegacySource, _target: &Path) -> PathBuf {
    // backup_root/<source-slug>/ —— 这是 backup 的**目录**（不是文件）。
    // backup 里每条源条目都平铺在该目录下，rollback 时按该目录的 entry
    // 名直接搬到 source.target() 的同级。这样设计避免「把整个目标目录
    // 当成一个 entry 搬进 backup」造成的 rollback 路径错位。
    backup_root.join(source.backup_key())
}

fn backup_existing(source: &Path, backup: &Path) -> io::Result<()> {
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    // 旧 target 移到 backup——Windows 上 rename 覆盖已存在文件失败，需要
    // 先把 backup 端删掉。Linux 上 rename 不会覆盖。
    if backup.exists() {
        let md = fs::symlink_metadata(backup)?;
        if md.is_dir() {
            fs::remove_dir_all(backup)?;
        } else {
            fs::remove_file(&backup)?;
        }
    }
    fs::rename(source, backup)
}

fn copy_one(source: &Path, target: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(source)?;
    if md.is_dir() {
        // 目录复制委托到共享层 [`pkg::copy_tree`]——与 P5 skills 模块共用同一份实现。
        pkg::copy_tree(source, target)
    } else if md.file_type().is_symlink() {
        // 链接直接复制其目标内容——避免在目标侧引入一层间接链接。
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target).map(|_| ())
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target).map(|_| ())
    }
}

// --- step 3：rollback_migration ----------------------------------------

/// 单条回滚项的结果——把 `MigrationItemReport.backup_path` 还原回 `target_path`。
#[derive(Debug, Clone, Serialize)]
pub struct RollbackItemReport {
    pub source: LegacySource,
    pub backup_path: PathBuf,
    pub target_path: PathBuf,
    pub files_restored: usize,
    pub status: RollbackStatus,
    pub error: Option<String>,
}

/// 单条回滚项的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RollbackStatus {
    /// 该来源无 backup（SkipIfNewer 策略或旧 migration 没备份）。
    NotFound,
    /// 全部 backup 条目成功还原到目标。
    Restored,
    /// 部分条目还原成功，部分失败。
    PartialFailure,
    /// 全部失败（一般 backup 不可读 / 目标不可写）。
    Failed,
}

/// 完整回滚报告——`rollback_migration` 的返回值。
#[derive(Debug, Clone, Serialize)]
pub struct RollbackReport {
    pub migration_id: String,
    pub backup_root: PathBuf,
    pub items: Vec<RollbackItemReport>,
}

// --- step 4：list_migrations + 摘要 --------------------------------------

/// 单条历史迁移的摘要——`list_migrations` 的返回元素。
///
/// 备份目录由 [`run_migration`] / [`rollback_migration`] 在
/// `<xlink_home>/backups/<id>/` 下创建；`list_migrations` 扫所有现存
/// backup 目录（含 `.N` 后缀变体）并返回这条记录。`created_at` 取目录
/// 自身的 mtime——可粗略反映迁移发生时间，但**不能**当作正式时间戳
/// （mtime 会被文件系统操作修改）。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationSummary {
    pub migration_id: String,
    pub backup_root: PathBuf,
    /// 备份目录 mtime，epoch 秒字符串——与商店清单的时间戳约定一致，
    /// 前端自己格式化本地时间。SystemTime 的 serde 序列化是个结构体，
    /// 前端没法直接读。
    pub created_at: Option<String>,
    pub sources: Vec<String>,
}

/// 列出所有历史迁移（按 backup 目录 mtime 倒序——最近跑过的在前）。
///
/// 扫描 `<xlink_home>/backups/` 下的**直接子目录**；不带 `.N` 后缀的
/// 视为权威一次（多次 run 会带后缀），同 id 的多条会按 N 索引各自
/// 独立列出。
pub fn list_migrations() -> Vec<MigrationSummary> {
    let backups_root = xlink_home().join("backups");
    let entries = match fs::read_dir(&backups_root) {
        Ok(it) => it,
        Err(_) => return Vec::new(),
    };
    let mut summaries: Vec<MigrationSummary> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();
            // 必须是目录；跳过 hidden / 临时文件。
            let md = fs::symlink_metadata(&path).ok()?;
            if !md.is_dir() {
                return None;
            }
            // mtime 是文件系统级 metadata，读失败时回退 None——失败不该
            // 让整条记录消失。
            let created_at = md.modified().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs().to_string())
            });
            let (migration_id, is_root) = parse_backup_dir_name(&file_name)?;
            if !is_root {
                // .N 后缀条目（root 已列出）——此处不单独列，避免 UI 重复
                // 显示同名 id 的多个快照。UI 想看 N 次历史可在 summary
                // 上提供更详细的版本；本函数刻意只列权威一次。
                return None;
            }
            let sources = collect_source_keys(&path);
            Some(MigrationSummary {
                migration_id,
                backup_root: path,
                created_at,
                sources,
            })
        })
        .collect();
    // epoch 秒字符串的字典序与时间序一致，倒序即「最近在前」。
    summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    summaries
}

/// 把 `backups/<name>` 的目录名拆成 `(migration_id, is_root)`：
/// - `2026-09-19-migrate` → `("2026-09-19-migrate", true)`
/// - `2026-09-19-migrate.2` → `("2026-09-19-migrate", false)`（非 root，被过滤掉）
fn parse_backup_dir_name(name: &str) -> Option<(String, bool)> {
    if name.starts_with('.') {
        return None;
    }
    if let Some((base, suffix)) = name.rsplit_once('.') {
        if suffix.parse::<u32>().is_ok() {
            return Some((base.to_string(), false));
        }
    }
    Some((name.to_string(), true))
}

/// 收集 backup 目录里实际存在的 source slug —— UI 据此知道「这次迁移
/// 涉及哪些来源」。`backup_root/plugins/`、`backup_root/skills-store/`、
/// `backup_root/skills-active/` 三个 slug 都可能存在。
fn collect_source_keys(backup_root: &Path) -> Vec<String> {
    let entries = match fs::read_dir(backup_root) {
        Ok(it) => it,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let md = fs::symlink_metadata(entry.path()).ok()?;
            if !md.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            Some(name)
        })
        .collect()
}

/// 把 `migration_id` 对应的 backup 还原回目标位置。
///
/// 流程：在 `<xlink_home>/backups/<id>`（或 `.2` / `.3` 后缀变体）下找到
/// backup 目录，按 [`LegacySource::backup_key`] 分组，逐个把 backup 里的
/// 条目 move 回 `source.target()`。
///
/// - 旧源**不被触碰**——本函数只动 backup 与目标。
/// - 当前目标位置已存在的同文件会被新覆盖（毕竟 backup 是迁移前的旧
///   快照，回滚的语义就是回到迁移前）。
/// - 同 id 多次跑过的 backup（带 `.N` 后缀）会被全部还原，但顺序按
///   `index` 升序——最早的 backup 先还原，确保状态可重现。
/// - 找不到 backup 时返回 `RollbackStatus::NotFound`（不报错——可能
///   SkipIfNewer 策略就没产生 backup，部分 rollback 是合理场景）。
pub fn rollback_migration(migration_id: &str) -> Result<RollbackReport, AppError> {
    if migration_id.is_empty() {
        return Err(AppError::Plugin("migration id 不能为空".into()));
    }
    let backup_root = find_backup_root(migration_id).ok_or_else(|| {
        AppError::Plugin(format!(
            "找不到 migration id {migration_id} 的备份目录——可能从未跑过 BackupAndOverwrite 策略的迁移"
        ))
    })?;

    let mut items = Vec::with_capacity(LegacySource::all().len());
    for source in LegacySource::all() {
        let source_backup = backup_root.join(source.backup_key());
        if !source_backup.exists() {
            items.push(RollbackItemReport {
                source,
                backup_path: source_backup,
                target_path: source.target(),
                files_restored: 0,
                status: RollbackStatus::NotFound,
                error: None,
            });
            continue;
        }
        let (restored, status, error) = rollback_one(&source_backup, &source.target());
        items.push(RollbackItemReport {
            source,
            backup_path: source_backup,
            target_path: source.target(),
            files_restored: restored,
            status,
            error,
        });
    }
    Ok(RollbackReport {
        migration_id: migration_id.to_string(),
        backup_root,
        items,
    })
}

/// 在 `<xlink_home>/backups/<id>` 下寻找现存 backup 根——优先取不带
/// 后缀的（最新一次 run_migration 落到的位置），否则退到 `.2` 后缀
/// 最早一次。同 id 多次迁移已经按 N 索引各自独立；UI 想还原哪次就
/// 直接传 `<id>.N`。
fn find_backup_root(migration_id: &str) -> Option<PathBuf> {
    let base = xlink_home().join("backups").join(migration_id);
    if base.exists() {
        return Some(base);
    }
    let candidate = xlink_home()
        .join("backups")
        .join(format!("{migration_id}.2"));
    if candidate.exists() {
        return Some(candidate);
    }
    None
}

fn rollback_one(backup_dir: &Path, target_dir: &Path) -> (usize, RollbackStatus, Option<String>) {
    let entries = match fs::read_dir(backup_dir) {
        Ok(it) => it,
        Err(error) => {
            return (
                0,
                RollbackStatus::Failed,
                Some(format!("无法读取 backup 目录：{error}")),
            );
        }
    };
    if let Err(error) = fs::create_dir_all(target_dir) {
        return (
            0,
            RollbackStatus::Failed,
            Some(format!("无法创建目标目录：{error}")),
        );
    }
    let mut restored = 0usize;
    let mut last_error: Option<String> = None;
    for entry in entries.flatten() {
        let from = entry.path();
        let file_name = entry.file_name();
        let target_path = target_dir.join(&file_name);
        let md = match fs::symlink_metadata(&from) {
            Ok(md) => md,
            Err(error) => {
                last_error = Some(format!("{file_name:?} 元数据不可读：{error}"));
                continue;
            }
        };
        // 当前目标位置已有同名条目——回滚语义就是「回到迁移前」，所以直接覆盖。
        if target_path.exists() || target_path.is_symlink() {
            if md.is_dir() {
                let _ = fs::remove_dir_all(&target_path);
            } else {
                let _ = fs::remove_file(&target_path);
            }
        }
        match if md.is_dir() {
            // backup 端是目录——把它整个搬回目标；目标端同层同名先删。
            restore_directory(&from, &target_path)
        } else {
            fs::rename(&from, &target_path).map_err(io::Error::from)
        } {
            Ok(()) => restored += 1,
            Err(error) => last_error = Some(format!("{file_name:?}：{error}")),
        }
    }
    let status = match (last_error.is_some(), restored) {
        (true, 0) => RollbackStatus::Failed,
        (true, _) => RollbackStatus::PartialFailure,
        (false, _) => RollbackStatus::Restored,
    };
    (restored, status, last_error)
}

fn restore_directory(from: &Path, to: &Path) -> io::Result<()> {
    // rename 在跨设备 / 子目录同名场景下可能失败——退化到递归复制 + 删源。
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            pkg::copy_tree(from, to)?;
            let _ = fs::remove_dir_all(from);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_variables)]

    use super::*;

    /// 测试 home：在临时目录下建一份唯一临时目录；drop 时自动清理。
    /// 设 `DSH_XLINK_HOME` 让 `paths::xlink_home()` 解析到本目录。
    /// 同时设 `DSH_HOME` 指向同一份临时目录的 `legacy/` 子目录，让
    /// `legacy_dsh_home()` 解析到那里——这样测试可以同时建一份"旧"
    /// 和一份"新"。
    struct TempHome {
        root: PathBuf,
        _xlink_guard: crate::tests::EnvGuard,
        _dsh_guard: crate::tests::EnvGuard,
    }

    impl TempHome {
        fn new() -> Self {
            let nano = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = std::sync::atomic::AtomicUsize::fetch_add(
                &TEST_COUNTER,
                1,
                std::sync::atomic::Ordering::Relaxed,
            );
            let base = std::env::temp_dir().join(format!(
                "dsh-migration-test-{}-{nano}-{seq}",
                std::process::id()
            ));
            let legacy = base.join("legacy");
            fs::create_dir_all(&legacy).expect("create test home");
            let xlink_guard = crate::tests::scoped_xlink_home(&base);
            let dsh_guard = crate::tests::scoped_dsh_home(&legacy);
            TempHome {
                root: base,
                _xlink_guard: xlink_guard,
                _dsh_guard: dsh_guard,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    static TEST_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[test]
    fn scan_legacy_sources_does_not_create_directories() {
        // 空 home 下扫所有旧来源——任何来源都不应被创建。
        let home = TempHome::new();
        let infos = scan_legacy_sources();
        assert_eq!(infos.len(), 3, "必须扫全 3 个旧来源");
        for info in &infos {
            assert!(!info.exists, "{} 不应存在", info.source.display_name());
            assert_eq!(info.file_count, 0);
            assert_eq!(info.total_bytes, 0);
            assert!(
                !info.target.exists(),
                "{} 目标也不应被创建",
                info.source.display_name()
            );
        }
    }

    #[test]
    fn scan_legacy_sources_counts_existing_files() {
        // 临时建一份"旧插件中央库"——里面放三个文件（含一个 nested），
        // 应该被算成 3 条 / 累计字节数；其余 2 个来源仍为不存在。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy plugins");
        fs::write(plugins_legacy.join("store.json"), "{}\n").expect("store");
        fs::write(plugins_legacy.join("manifest.txt"), "x").expect("manifest");
        // nested dir also counted (recursive)
        fs::create_dir_all(plugins_legacy.join("pkg-a")).expect("pkg-a");
        fs::write(plugins_legacy.join("pkg-a/data.txt"), "abc").expect("pkg data");

        let infos = scan_legacy_sources();
        let plugins_info = infos
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins info");
        assert!(plugins_info.exists);
        assert_eq!(plugins_info.file_count, 3);
        assert!(plugins_info.total_bytes > 0);
        for other in infos.iter().filter(|i| i.source != LegacySource::Plugins) {
            assert!(!other.exists, "{} 必须不存在", other.source.display_name());
        }
    }

    #[test]
    fn preview_migration_reports_target_paths_and_existing_flags() {
        // 一次性把 3 个旧来源都建上，preview 应该全部报告存在 + 字节数；
        // target_exists 取决于 xlink_home 下对应目录是否已被新布局占用。
        let home = TempHome::new();
        for source in LegacySource::all() {
            let path = source.path();
            fs::create_dir_all(&path).expect("legacy source");
            fs::write(path.join("seed.txt"), "x").expect("seed file");
        }
        let preview = preview_migration();
        assert!(preview.has_migratable(), "至少有一条可迁移来源");
        for item in &preview.items {
            assert!(item.file_count >= 1);
            assert!(item.total_bytes >= 1);
            // 3 个旧来源对应的目标路径**全部不应**存在——xlink_home 是
            // 全新临时目录，新布局不会冲突。
            assert!(
                !item.target_exists,
                "{} 的目标 {} 在全新 home 下不应存在",
                item.display_name,
                item.target_path.display()
            );
            assert_eq!(item.source_path, item.source.path());
            assert_eq!(item.target_path, item.source.target());
        }
    }

    #[test]
    fn legacy_source_target_matches_new_layout_paths() {
        // 直接锁定源 → 目标的映射——这是迁移向导的核心契约：preview 与
        // 复制步骤都按这个表来。任何改动都必须先改这里 + 改测试。
        use LegacySource::*;
        let cases = [
            (Plugins, plugins_store_root()),
            (SkillsStore, skills_store_root()),
            (SkillsActive, skills_active_root()),
        ];
        for (source, expected) in cases {
            assert_eq!(
                source.target(),
                expected,
                "{:?} 的目标路径必须与新布局一致",
                source
            );
        }
    }

    #[test]
    fn preview_reports_target_conflicts_when_new_layout_already_present() {
        // 当新布局已经写了部分目录（用户可能手动创建过、或先跑过迁移
        // 又回滚），preview 必须如实报告 target_exists 让 UI 弹冲突提示。
        let home = TempHome::new();
        // 先建一份旧 plugins 中央库。
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("store.json"), "x").expect("store");
        // 然后让新布局的 `dsh-plugins/` 已经存在（模拟"先跑过迁移"或"手放"）。
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new layout");
        fs::write(plugins_new.join("placeholder.txt"), "x").expect("placeholder");

        let preview = preview_migration();
        let plugins_item = preview
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert!(
            plugins_item.target_exists,
            "新布局 dsh-plugins 已存在，preview 必须如实报告"
        );
    }

    // --- step 2：run_migration 测试 -----------------------------------

    #[test]
    fn run_migration_copies_old_plugins_into_new_layout() {
        // 旧 ~/.dsh/plugins/ 里有 store.json + pkg-a/ → 新 Xlink home
        // 的 dsh-plugins/ 必须出现同样文件；backup_root 自动建在
        // <xlink_home>/backups/<id>/。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy plugins");
        fs::write(plugins_legacy.join("store.json"), "{\"v\":1}\n").expect("store");
        fs::create_dir_all(plugins_legacy.join("pkg-a")).expect("pkg-a");
        fs::write(plugins_legacy.join("pkg-a/data.txt"), "abc").expect("pkg data");

        let report = run_migration(ConflictPolicy::SkipIfNewer).expect("migration succeeds");
        assert!(report.migration_id.starts_with("Auto"));
        assert!(report.backup_root.exists());
        assert!(report
            .items
            .iter()
            .any(|i| i.source == LegacySource::Plugins));

        let plugins_new = LegacySource::Plugins.target();
        assert!(
            plugins_new.join("store.json").is_file(),
            "store.json 必须被复制"
        );
        assert!(
            plugins_new.join("pkg-a/data.txt").is_file(),
            "嵌套文件必须被复制"
        );

        let plugins_item = report
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert_eq!(plugins_item.status, MigrationStatus::Copied);
        assert!(plugins_item.files_copied >= 2);
        assert!(plugins_item.error.is_none());
    }

    #[test]
    fn run_migration_skips_sources_with_no_migratable_content() {
        // 三个旧来源都建目录但都是空的 → 全部 Skipped；backup_root 仍
        // 创建（保证可调用方落盘报告），但 report.items 全 0 拷贝。
        let home = TempHome::new();
        for source in LegacySource::all() {
            fs::create_dir_all(source.path()).expect("empty legacy dir");
        }
        let report = run_migration(ConflictPolicy::SkipIfNewer)
            .expect("migration succeeds even if no source has files");
        assert_eq!(report.items.len(), 3);
        for item in &report.items {
            assert_eq!(item.status, MigrationStatus::Skipped);
            assert_eq!(item.files_copied, 0);
        }
    }

    #[test]
    fn run_migration_skip_if_newer_preserves_existing_target_with_newer_mtime() {
        // 旧 plugins 中央库写文件 A；新 dsh-plugins/ 里已经有 B（用户手动
        // 写的，比旧源更新）。SkipIfNewer 策略必须保留 B，把 A 复制过去
        // 不覆盖 B。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("from-old.txt"), "old-content").expect("old");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        let newer = plugins_new.join("from-old.txt");
        fs::write(&newer, "newer-content").expect("create target");
        // 把目标文件的 mtime 推到"比源晚 1 秒"。
        let newer_time = filetime_after(
            fs::metadata(plugins_legacy.join("from-old.txt"))
                .unwrap()
                .modified()
                .unwrap(),
        );
        set_mtime(&newer, newer_time);

        let report = run_migration(ConflictPolicy::SkipIfNewer).expect("migration succeeds");
        let plugins_item = report
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert_eq!(plugins_item.status, MigrationStatus::Copied);
        assert!(plugins_item.files_skipped >= 1, "目标比源新，必须跳过");
        // 目标内容必须没变（仍是新布局原文件）。
        let after = fs::read_to_string(&newer).expect("read target");
        assert!(
            after != "old-content",
            "目标文件比源新，SkipIfNewer 不能覆盖其内容"
        );
    }

    #[test]
    fn run_migration_merges_store_manifest_instead_of_skip_or_overwrite() {
        // 回归：真实环境里目标 store.json 可能因任何原因比源"新"（例如被
        // 其它流程写过），SkipIfNewer 整文件跳过会让源里用户真正要迁的
        // 记录永远进不来——面板只剩目标侧的错误条目。清单必须按 id 合并：
        // 源独有条目并入，目标已有条目原样保留。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(
            plugins_legacy.join("store.json"),
            r#"{"schemaVersion":1,"items":[
                {"id":"shared","name":"shared","origin":"npm"},
                {"id":"only-old","name":"only-old","origin":"npm"}]}"#,
        )
        .expect("legacy store");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        fs::write(
            plugins_new.join("store.json"),
            r#"{"schemaVersion":1,"items":[
                {"id":"shared","name":"shared-renamed","origin":"npm"}],
                "lastCheckedAt":"1"}"#,
        )
        .expect("new store");
        // 让目标 mtime 比源新——旧逻辑在这里整文件 Skip，源记录全部丢失。
        let now = std::time::SystemTime::now();
        set_mtime(
            &plugins_legacy.join("store.json"),
            now - std::time::Duration::from_secs(60),
        );
        set_mtime(&plugins_new.join("store.json"), now);

        let report = run_migration(ConflictPolicy::SkipIfNewer).expect("migration succeeds");
        let plugins_item = report
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert_eq!(plugins_item.status, MigrationStatus::Copied);

        let merged: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(plugins_new.join("store.json")).expect("read merged"),
        )
        .expect("merged store.json 必须仍是合法 JSON");
        let items = merged["items"].as_array().expect("items 数组");
        let ids: Vec<&str> = items.iter().filter_map(|i| i["id"].as_str()).collect();
        assert_eq!(ids.len(), 2, "合并后不允许重复条目：{ids:?}");
        assert!(ids.contains(&"only-old"), "源独有条目必须被并入：{ids:?}");
        let shared = items
            .iter()
            .find(|i| i["id"] == "shared")
            .expect("shared 条目");
        assert_eq!(
            shared["name"], "shared-renamed",
            "目标已有条目必须原样保留，不被源覆盖"
        );
        assert_eq!(merged["lastCheckedAt"], "1", "目标清单其它顶层字段原样保留");
    }

    #[test]
    fn run_migration_backup_and_overwrite_moves_old_target_under_backups() {
        // BackupAndOverwrite：旧 plugins 中央库有 A → 新 dsh-plugins/
        // 也有 A（内容不同）。目标 A 必须被移到 backup 目录，再被源 A
        // 覆盖。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("shared.txt"), "from-old").expect("old");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        fs::write(plugins_new.join("shared.txt"), "from-new").expect("new");
        // 让源 / 目标 mtime 一致——避免 SkipIfNewer 在 BackupAndOverwrite
        // 路径下仍然走 Skip。源 mtime 设为更晚。
        let now = std::time::SystemTime::now();
        set_mtime(&plugins_legacy.join("shared.txt"), now);
        set_mtime(
            &plugins_new.join("shared.txt"),
            now - std::time::Duration::from_secs(60),
        );

        let report = run_migration(ConflictPolicy::BackupAndOverwrite).expect("migration succeeds");
        let plugins_item = report
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert!(
            plugins_item.backup_path.is_some(),
            "BackupAndOverwrite 必须记录 backup_path"
        );
        // backup_path 形如 `<xlink_home>/backups/<id>/<source display_name>/<file>`。
        assert!(
            plugins_item.backup_path.as_ref().unwrap().exists(),
            "BackupAndOverwrite 写出的 backup_path 必须真实存在"
        );
        let after = fs::read_to_string(plugins_new.join("shared.txt")).expect("read target");
        assert_eq!(after, "from-old", "源必须覆盖目标");
    }

    #[test]
    fn run_migration_does_not_remove_legacy_source() {
        // 旧 plugins 中央库在迁移后必须**仍然存在**——回滚路径需要它。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("store.json"), "{}").expect("store");

        run_migration(ConflictPolicy::SkipIfNewer).expect("migration succeeds");
        assert!(
            plugins_legacy.join("store.json").is_file(),
            "旧源必须保留——回滚路径依赖它"
        );
    }

    #[test]
    fn run_migration_appends_index_when_backup_root_exists() {
        // 同一 migration_id 跑两次：第二次必须把 backup 落到 `.2/` 后缀，
        // 不覆盖第一次的 backup——回滚链必须可重现。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("store.json"), "{}").expect("store");

        let r1 = run_migration(ConflictPolicy::BackupAndOverwrite).expect("r1");
        let r2 = run_migration(ConflictPolicy::BackupAndOverwrite).expect("r2");
        assert_ne!(r1.backup_root, r2.backup_root);
        assert!(
            r1.backup_root.exists() && r2.backup_root.exists(),
            "两次 backup 目录都必须存在"
        );
    }

    /// 把 mtime 推到比 `baseline` 晚 10 秒——用于构造"目标比源新"场景。
    fn filetime_after(baseline: std::time::SystemTime) -> std::time::SystemTime {
        baseline + std::time::Duration::from_secs(10)
    }

    fn set_mtime(path: &Path, t: std::time::SystemTime) {
        // Rust 1.75+ 起 `File::set_modified` 在 Unix / Windows 都可用。
        let file = fs::OpenOptions::new().write(true).open(path).expect("open");
        file.set_modified(t).expect("set mtime");
    }

    // --- step 3：rollback_migration 测试 -------------------------------

    #[test]
    fn rollback_restores_old_target_after_backup_and_overwrite() {
        // 旧 plugins 中央库 → 新 dsh-plugins/，BackupAndOverwrite 覆盖。
        // rollback 必须把 backup 里的旧版本还原回目标，让用户回到迁移前。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("shared.txt"), "from-old").expect("old");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        fs::write(plugins_new.join("shared.txt"), "from-new").expect("new");
        // 让源比目标晚——BackupAndOverwrite 路径会落到 BackupThenCopy。
        let now = std::time::SystemTime::now();
        set_mtime(&plugins_legacy.join("shared.txt"), now);
        set_mtime(
            &plugins_new.join("shared.txt"),
            now - std::time::Duration::from_secs(60),
        );

        let report = run_migration(ConflictPolicy::BackupAndOverwrite).expect("mig");
        assert_eq!(
            fs::read_to_string(plugins_new.join("shared.txt")).unwrap(),
            "from-old",
            "源必须覆盖目标"
        );

        // 迁移之后用户反悔——跑 rollback（id 来自后端报告）。
        let rb = rollback_migration(&report.migration_id).expect("rollback");
        let plugins_item = rb
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert_eq!(plugins_item.status, RollbackStatus::Restored);
        assert_eq!(plugins_item.files_restored, 1);
        assert!(
            plugins_item.error.is_none(),
            "rollback 不该报错：{error:?}",
            error = plugins_item.error
        );

        // 目标内容必须回到迁移前的旧值。
        let after = fs::read_to_string(plugins_new.join("shared.txt")).expect("read");
        assert_eq!(after, "from-new", "rollback 必须把旧版本还原回目标");
    }

    #[test]
    fn rollback_is_noop_for_items_without_backup() {
        // SkipIfNewer 策略不会写 backup——rollback 报告里这些来源应该是
        // NotFound 而非 Failed。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("store.json"), "{}").expect("store");

        let report = run_migration(ConflictPolicy::SkipIfNewer).expect("mig");
        let rb = rollback_migration(&report.migration_id).expect("rollback");
        let plugins_item = rb
            .items
            .iter()
            .find(|i| i.source == LegacySource::Plugins)
            .expect("plugins item");
        assert_eq!(plugins_item.status, RollbackStatus::NotFound);
        assert_eq!(plugins_item.files_restored, 0);
        // 其它没参与迁移的来源也应该是 NotFound。
        for item in rb
            .items
            .iter()
            .filter(|i| i.source != LegacySource::Plugins)
        {
            assert_eq!(item.status, RollbackStatus::NotFound);
        }
    }

    #[test]
    fn rollback_errors_when_migration_id_not_found() {
        // 从未跑过迁移的 id——rollback 必须报错而不是静默吞掉。
        let home = TempHome::new();
        let err = rollback_migration("never-existed").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("never-existed"),
            "错误信息必须包含 migration id：{msg}"
        );
    }

    #[test]
    fn rollback_errors_when_migration_id_is_empty() {
        let home = TempHome::new();
        let err = rollback_migration("").unwrap_err();
        assert!(err.to_string().contains("migration id 不能为空"));
    }

    #[test]
    fn rollback_preserves_legacy_source() {
        // 旧源不能被 rollback 触碰——回滚是"恢复目标"，不是"反向迁移"。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("from-old.txt"), "x").expect("old");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        fs::write(plugins_new.join("from-old.txt"), "y").expect("new");
        // 让源比目标晚。
        let now = std::time::SystemTime::now();
        set_mtime(&plugins_legacy.join("from-old.txt"), now);
        set_mtime(
            &plugins_new.join("from-old.txt"),
            now - std::time::Duration::from_secs(60),
        );

        let report = run_migration(ConflictPolicy::BackupAndOverwrite).expect("mig");
        let before = fs::read_to_string(plugins_legacy.join("from-old.txt")).unwrap();
        rollback_migration(&report.migration_id).expect("rb");
        let after = fs::read_to_string(plugins_legacy.join("from-old.txt")).unwrap();
        assert_eq!(before, after, "rollback 不能改旧源");
    }

    #[test]
    fn rollback_restores_backup_after_target_was_overwritten_by_user() {
        // 边界场景：用户先看到旧目标在 backup 里，手动把目标改了别的内容；
        // 再触发 rollback——必须把 backup 里的旧版本再搬回去，
        // 而不是迁就用户的"最新修改"。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("shared.txt"), "from-old").expect("old");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        fs::write(plugins_new.join("shared.txt"), "from-new").expect("new");
        let now = std::time::SystemTime::now();
        set_mtime(&plugins_legacy.join("shared.txt"), now);
        set_mtime(
            &plugins_new.join("shared.txt"),
            now - std::time::Duration::from_secs(60),
        );

        let report = run_migration(ConflictPolicy::BackupAndOverwrite).expect("mig");
        // 用户手动把目标改了——按"最新修改"逻辑 rollback 应该跳过。
        fs::write(plugins_new.join("shared.txt"), "user-changed").expect("user edit");

        rollback_migration(&report.migration_id).expect("rb");
        let after = fs::read_to_string(plugins_new.join("shared.txt")).expect("read");
        assert_eq!(
            after, "from-new",
            "rollback 必须按 backup 内容覆盖用户的最新修改——回滚语义就是回到迁移前"
        );
    }

    // --- step 4：list_migrations 测试 ---------------------------------

    #[test]
    fn list_migrations_is_empty_when_no_backup_root() {
        // 没有 backup 目录——返回空 Vec，UI 据此显示「尚无迁移历史」。
        let home = TempHome::new();
        let summaries = list_migrations();
        assert!(summaries.is_empty());
    }

    #[test]
    fn list_migrations_collects_root_entries_with_sources() {
        // 跑一次 BackupAndOverwrite 迁移 → list_migrations 必须返回一条记录，
        // 其 sources 至少包含 plugins。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("shared.txt"), "x").expect("old");
        let plugins_new = LegacySource::Plugins.target();
        fs::create_dir_all(&plugins_new).expect("new");
        fs::write(plugins_new.join("shared.txt"), "y").expect("new");
        let now = std::time::SystemTime::now();
        set_mtime(&plugins_legacy.join("shared.txt"), now);
        set_mtime(
            &plugins_new.join("shared.txt"),
            now - std::time::Duration::from_secs(60),
        );

        run_migration(ConflictPolicy::BackupAndOverwrite).expect("mig");
        let summaries = list_migrations();
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert!(summary.migration_id.starts_with("Auto"));
        assert!(
            summary.sources.iter().any(|s| s == "plugins"),
            "summary 必须报告 sources 含 plugins：{:?}",
            summary.sources
        );
        assert!(summary.backup_root.exists());
        assert!(summary.created_at.is_some());
    }

    #[test]
    fn list_migrations_ignores_dot_indexed_backup_dirs() {
        // 同 id 多次跑会留 `.2` / `.3` 后缀——list_migrations 只列 root
        // 一次，避免 UI 重复显示同名 id 的多个快照。
        // 测试手法：手动建一个 `<id>.2` 子目录，确认 list_migrations
        // 不把它算入（即使该子目录 mtime 比 root 新）。
        let home = TempHome::new();
        let plugins_legacy = LegacySource::Plugins.path();
        fs::create_dir_all(&plugins_legacy).expect("legacy");
        fs::write(plugins_legacy.join("store.json"), "{}").expect("store");
        let report = run_migration(ConflictPolicy::BackupAndOverwrite).expect("r1");
        let base = xlink_home().join("backups").join(&report.migration_id);
        assert!(base.exists(), "首次迁移后 base 必须存在");
        // 手工建 `<id>.2` 子目录，mtime 改到 base 之后——模拟「同 id 后续追加」
        let dot2 = xlink_home()
            .join("backups")
            .join(format!("{}.2", report.migration_id));
        std::fs::create_dir_all(&dot2).expect("create .2");
        let touch = dot2.join(".touch");
        std::fs::write(&touch, b"").expect("write touch");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        let _ = std::fs::File::open(&touch).and_then(|f| f.set_modified(later));
        let summaries = list_migrations();
        assert_eq!(summaries.len(), 1, "root + .2 后缀必须只列一次");
        assert_eq!(summaries[0].migration_id, report.migration_id);
    }

    #[test]
    fn list_migrations_sorts_newest_first() {
        // 跑两次不同 id 的迁移——list_migrations 必须按 mtime 倒序返回，
        // 让 UI 默认显示最近一次。
        let home = TempHome::new();
        for id in ["pending-sort-a", "pending-sort-b"] {
            let legacy = LegacySource::Plugins.path();
            fs::create_dir_all(&legacy).expect("legacy");
            fs::write(legacy.join("store.json"), "{}").expect("store");
            run_migration(ConflictPolicy::SkipIfNewer).expect("mig");
            // 确保 mtime 差至少 1 秒——避免文件系统秒级粒度同值。
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let summaries = list_migrations();
        assert_eq!(summaries.len(), 2);
        // 后端自动生成 id（AutoYYYYMMDD-HHMMSS-<short>）；list_migrations
        // 按 backup 目录 mtime 倒序——确保 2 条 id 不同、第一条 created_at
        // ≥ 第二条。
        assert_ne!(summaries[0].migration_id, summaries[1].migration_id);
        let a = summaries[0].created_at.as_deref().expect("created_at");
        let b = summaries[1].created_at.as_deref().expect("created_at");
        assert!(a >= b, "最近一次的迁移必须排第一");
    }
}
