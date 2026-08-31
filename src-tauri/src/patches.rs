//! 内置补丁管理：随 dsh-xlink 发布包捆绑的内核补丁 / 小插件。
//!
//! 补丁清单（`manifest.json`）与载荷文件随 app bundle 资源目录分发（见
//! `tauri.conf.json` 的 `bundle.resources`），默认**不生效**——用户在「设置 →
//! 内核补丁」页自主选择应用到当前激活内核，并可随时撤销。所有修改都以
//! 原始文件备份 + 内容哈希校验保证可逆性，目标路径被严格约束在内核目录内。
//!
//! 设计说明与开发流程见 `docs/patch-management.md`。
//!
//! 运行时状态位于 `<data_dir>/patches/`：
//!
//! ```text
//! state.json                              # 应用记录（schemaVersion 1）
//! backups/<patch-id>/<内核版本>/<相对路径>  # 应用时备份的原文件
//! ```

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::process::atomic_write;
use crate::version::cmp_versions;
use crate::{kernel, settings};
use tauri::Manager;

/// 运行时状态的子目录（位于 `data_dir` 下）。
const PATCHES_SUBDIR: &str = "patches";
/// 应用记录文件。
const STATE_FILE: &str = "state.json";
/// 原文件备份根目录。
const BACKUPS_SUBDIR: &str = "backups";
/// 当前状态 schema 版本。
const STATE_SCHEMA_VERSION: u32 = 1;
/// 当前清单 schema 版本。
const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// 补丁类型：改动内核既有文件。
const KIND_PATCH: &str = "patch";

fn default_required() -> bool {
    true
}

fn default_kind() -> String {
    KIND_PATCH.to_string()
}

/// 内置补丁清单（`resources/patches/<id>/manifest.json` 的总表）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchManifest {
    pub schema_version: u32,
    #[serde(default)]
    pub patches: Vec<PatchDef>,
}

/// 一个补丁的定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchDef {
    /// 文件系统安全、全局唯一的 id（kebab-case）。
    pub id: String,
    /// 显示名称。
    pub name: String,
    /// 补丁自身的版本。
    pub version: String,
    /// `patch`（改动内核既有文件）或 `plugin`（内置小插件）。仅作展示。
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    /// 适用内核版本范围（含端点）；`None` 表示不限。
    #[serde(default)]
    pub min_kernel_version: Option<String>,
    #[serde(default)]
    pub max_kernel_version: Option<String>,
    pub files: Vec<PatchFileDef>,
}

/// 补丁的文件操作定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchFileDef {
    /// `copy`：把 `from`（相对补丁目录）覆盖到 `to`（相对内核目录）；
    /// `replace`：对 `to` 做精确字符串全文替换（`search` → `replacement`）。
    pub mode: String,
    /// `copy` 模式的源文件，相对补丁资源目录。
    #[serde(default)]
    pub from: Option<String>,
    /// 目标路径，相对内核目录（`kernels/<版本>/`），必须通过路径约束检查。
    pub to: String,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub replacement: Option<String>,
    /// 目标缺失 / 搜索串未命中时是否必须失败。为 `false` 时跳过该文件并记录说明。
    #[serde(default = "default_required")]
    pub required: bool,
    /// copy 模式覆盖既有文件时的预期「原文件」SHA-256（小写十六进制）。
    ///
    /// 给出时，目标若已存在必须与该哈希一致才会被备份并覆盖——这是把补丁
    /// 打在 npm dist 等既有文件上时的安全闸：内核版本一旦升级、目标文件
    /// 内容漂移，应用会明确失败而不是覆盖一个未知文件。缺省时 copy 保持
    /// 「新增文件」语义（目标已存在且内容不同则拒绝）。
    #[serde(default)]
    pub expect_sha256: Option<String>,
}

/// 应用记录（`state.json` 中按「补丁 × 内核版本」组织）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchState {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub applied: Vec<AppliedPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPatch {
    pub id: String,
    #[serde(rename = "kernelVersion")]
    pub kernel_version: String,
    #[serde(rename = "appliedAt")]
    pub applied_at: String,
    /// 应用时使用的补丁定义版本。旧版 state.json 没有该字段时按过期记录处理，
    /// 避免资源载荷更新后仍把旧文件误报为「已应用」。
    #[serde(rename = "patchVersion", default)]
    pub patch_version: Option<String>,
    /// 应用过程中跳过 / 警告的说明（非必需文件未命中、备份丢失等）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub files: Vec<AppliedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFile {
    pub to: String,
    /// 应用前目标是否已存在（存在时必有备份）。
    #[serde(rename = "hadOriginal")]
    pub had_original: bool,
    /// 应用后（当前应为）文件内容的 SHA-256。
    #[serde(rename = "patchedSha256")]
    pub patched_sha256: String,
    /// 备份文件相对 `<data_dir>/patches/backups/` 的路径。
    #[serde(rename = "backupRel", skip_serializing_if = "Option::is_none")]
    pub backup_rel: Option<String>,
}

impl Default for PatchState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            applied: Vec::new(),
        }
    }
}

/// 一个补丁在设置页展示的状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_kernel_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_kernel_version: Option<String>,
    /// 状态机：no_kernel / incompatible / not_applied / applied / partial / dirty。
    pub state: String,
    /// 状态的人类可读文案（UI 直接展示）。
    pub state_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
    /// 补丁当前是否可操作（can_apply / can_revert 的前置判断由 UI 结合
    /// 工作台运行态完成；这里只表达磁盘层面的可用性）。
    pub enabled: bool,
}

/// `patch_status` 的完整返回。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchStatus {
    pub patches: Vec<PatchRow>,
}

/// 运行时状态各路径。
fn state_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PATCHES_SUBDIR)
}
fn state_file(data_dir: &Path) -> PathBuf {
    state_dir(data_dir).join(STATE_FILE)
}
fn backups_root(data_dir: &Path) -> PathBuf {
    state_dir(data_dir).join(BACKUPS_SUBDIR)
}

/// 内置补丁资源根目录的候选位置。构建时经 `bundle.resources` 的 Map 形式
/// 固定复制到 `resource_dir/patches/`；这里同时接受 `resource_dir/resources/patches/`
/// 的旧式平铺语义，返回第一个存在补丁清单子目录的候选。
pub fn resource_patches_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return None;
    };
    let candidates = [
        resource_dir.join("patches"),
        resource_dir.join("resources/patches"),
    ];
    candidates.into_iter().find(|dir| {
        fs::read_dir(dir)
            .map(|mut entries| entries.any(|e| e.is_ok() && e.ok().unwrap().path().is_dir()))
            .unwrap_or(false)
    })
}

/// 读取资源目录下全部补丁清单，返回（补丁定义，补丁目录）。
pub fn load_patches(resource_root: &Path) -> Result<Vec<(PatchDef, PathBuf)>, String> {
    let mut out = Vec::new();
    let entries = fs::read_dir(resource_root).map_err(|e| {
        format!(
            "无法读取内置补丁目录 {}：{e}（请重新安装 dsh-xlink）",
            resource_root.display()
        )
    })?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest_path = dir.join("manifest.json");
        let text = match fs::read_to_string(&manifest_path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!(
                    "dsh-xlink: 跳过无效补丁目录 {}（缺少 manifest.json：{e}）",
                    dir.display()
                );
                continue;
            }
        };
        let manifest: PatchManifest = match serde_json::from_str(&text) {
            Ok(manifest) => manifest,
            Err(e) => {
                eprintln!(
                    "dsh-xlink: 跳过无效补丁清单 {}：{e}",
                    manifest_path.display()
                );
                continue;
            }
        };
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            eprintln!(
                "dsh-xlink: 跳过不支持的补丁清单版本 {}（{}）",
                manifest.schema_version,
                manifest_path.display()
            );
            continue;
        }
        for def in manifest.patches {
            if let Err(reason) = validate_def(&def) {
                eprintln!(
                    "dsh-xlink: 跳过补丁 {}：{reason}",
                    if def.id.is_empty() {
                        "<无 id>"
                    } else {
                        &def.id
                    }
                );
                continue;
            }
            out.push((def, dir.clone()));
        }
    }
    Ok(out)
}

/// 清单静态校验：id / files 非空、路径合法、模式字段自洽。运行期不依赖它，
/// 但可以在加载时就剔除明显坏掉的补丁。
fn validate_def(def: &PatchDef) -> Result<(), String> {
    if def.id.is_empty() {
        return Err("id 为空".into());
    }
    if def.id.contains(['/', '\\']) {
        return Err(format!("id 含路径分隔符：{}", def.id));
    }
    if def.files.is_empty() {
        return Err("files 不能为空".into());
    }
    for file in &def.files {
        if let Err(e) = check_target_path(&file.to) {
            return Err(format!("files[].to 非法（{}）：{e}", file.to));
        }
        match file.mode.as_str() {
            "copy" => {
                if file.from.as_deref().unwrap_or("").is_empty() {
                    return Err(format!("files[].to={} 的 copy 模式缺少 from", file.to));
                }
                if let Some(expected) = file.expect_sha256.as_deref() {
                    let hex = expected.trim().to_ascii_lowercase();
                    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(format!(
                            "files[].to={} 的 expectSha256 不是 64 位十六进制：{expected}",
                            file.to
                        ));
                    }
                }
            }
            "replace" => {
                let search_ok = file
                    .search
                    .as_deref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if !search_ok || file.replacement.is_none() {
                    return Err(format!(
                        "files[].to={} 的 replace 模式缺少 search/replacement",
                        file.to
                    ));
                }
            }
            other => return Err(format!("files[].to={} 的模式 {other} 不支持", file.to)),
        }
    }
    Ok(())
}

/// 校验目标路径是内核目录内的普通相对路径：拒绝绝对路径、父级跳转、
/// 空路径与 Windows 盘符前缀。
fn check_target_path(rel: &str) -> Result<(), String> {
    let path = Path::new(rel);
    if rel.is_empty() {
        return Err("路径为空".into());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => return Err(format!("路径含非法成分：{rel}")),
        }
    }
    if path.as_os_str().is_empty() {
        return Err("路径为空".into());
    }
    Ok(())
}

/// 拒绝通过符号链接中间目录穿透内核目录：检查目标路径从父目录到内核根
/// （含）之间每个已存在的祖先都必须是真实目录。只检查内核根以内——macOS
/// 的 `/var`、`/tmp` 等系统路径本身就是符号链接，往上再查只会误伤。
fn ensure_no_symlink_ancestors(target: &Path, kernel_root: &Path) -> Result<(), AppError> {
    let mut probe = target.parent();
    while let Some(dir) = probe {
        match fs::symlink_metadata(dir) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(AppError::Patch(format!(
                        "目标路径 {} 的祖先 {} 是符号链接，为安全起见拒绝写入",
                        target.display(),
                        dir.display()
                    )));
                }
                if !meta.is_dir() {
                    return Err(AppError::Patch(format!(
                        "目标路径 {} 的祖先 {} 不是目录",
                        target.display(),
                        dir.display()
                    )));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(AppError::Patch(format!(
                    "无法检查目标路径 {}：{e}",
                    target.display()
                )))
            }
        }
        if dir == kernel_root {
            break;
        }
        probe = dir.parent();
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String, AppError> {
    let bytes =
        fs::read(path).map_err(|e| AppError::Patch(format!("无法读取 {}：{e}", path.display())))?;
    Ok(sha256_bytes(&bytes))
}

fn read_state(data_dir: &Path) -> PatchState {
    fs::read_to_string(state_file(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_state(data_dir: &Path, state: &PatchState) -> Result<(), AppError> {
    let dir = state_dir(data_dir);
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Patch(format!("无法创建补丁状态目录 {}：{e}", dir.display())))?;
    let text = serde_json::to_string_pretty(state)
        .map_err(|e| AppError::Patch(format!("序列化补丁状态失败：{e}")))?;
    atomic_write(&state_file(data_dir), text.as_bytes()).map_err(|e| {
        AppError::Patch(format!(
            "无法写入补丁状态 {}：{e}",
            state_file(data_dir).display()
        ))
    })
}

/// 某补丁在某内核版本上的应用记录。
fn find_applied<'a>(
    state: &'a PatchState,
    id: &str,
    kernel_version: &str,
) -> Option<&'a AppliedPatch> {
    state
        .applied
        .iter()
        .find(|a| a.id == id && a.kernel_version == kernel_version)
}

fn backup_path(data_dir: &Path, id: &str, kernel_version: &str, to: &str) -> PathBuf {
    backups_root(data_dir)
        .join(id)
        .join(kernel_version)
        .join(to)
}

/// 应用前备份目标原文件；目标不存在则返回 `false`（无备份）。
fn backup_target(
    data_dir: &Path,
    id: &str,
    kernel_version: &str,
    to: &str,
) -> Result<bool, AppError> {
    let target = kernel::kernel_dir(data_dir, kernel_version).join(to);
    match fs::symlink_metadata(&target) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AppError::Patch(format!(
                    "目标 {} 是符号链接，拒绝覆盖",
                    target.display()
                )));
            }
            if !meta.is_file() {
                return Err(AppError::Patch(format!(
                    "目标 {} 不是普通文件，拒绝覆盖",
                    target.display()
                )));
            }
            let backup = backup_path(data_dir, id, kernel_version, to);
            if fs::symlink_metadata(&backup).is_ok() {
                return Err(AppError::Patch(format!(
                    "补丁 {id} 在 {} 上已有未清理的备份 {}，请先撤销该补丁或手动清理",
                    kernel_version,
                    backup.display()
                )));
            }
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    AppError::Patch(format!("无法创建备份目录 {}：{e}", parent.display()))
                })?;
            }
            fs::copy(&target, &backup).map_err(|e| {
                AppError::Patch(format!(
                    "无法备份 {} 到 {}：{e}",
                    target.display(),
                    backup.display()
                ))
            })?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(AppError::Patch(format!(
            "无法检查目标 {}：{e}",
            target.display()
        ))),
    }
}

/// 工作台运行期间不允许动内核目录（与「切换内核版本」同一规则）。
fn ensure_workbench_stopped(data_dir: &Path) -> Result<(), AppError> {
    if kernel::port_open(settings::load(data_dir).port) {
        return Err(AppError::Patch(
            "工作台正在启动或运行，请先点击「关闭工作台」停止后再应用或撤销补丁".into(),
        ));
    }
    Ok(())
}

fn find_patch<'a>(
    patches: &'a [(PatchDef, PathBuf)],
    id: &str,
) -> Result<&'a (PatchDef, PathBuf), AppError> {
    patches
        .iter()
        .find(|(def, _)| def.id == id)
        .ok_or_else(|| AppError::Patch(format!("补丁 {id} 不存在于当前 dsh-xlink 的内置清单中")))
}

/// 补丁的适用版本范围是否覆盖 `kernel_version`。
fn version_in_range(def: &PatchDef, kernel_version: &str) -> bool {
    let in_min = def
        .min_kernel_version
        .as_deref()
        .map(|min| cmp_versions(kernel_version, min) != std::cmp::Ordering::Less)
        .unwrap_or(true);
    let in_max = def
        .max_kernel_version
        .as_deref()
        .map(|max| cmp_versions(kernel_version, max) != std::cmp::Ordering::Greater)
        .unwrap_or(true);
    in_min && in_max
}

/// 应用补丁到当前激活内核。前置：工作台已停止、内核已激活、版本在范围内。
pub fn apply(
    data_dir: &Path,
    patches: &[(PatchDef, PathBuf)],
    id: &str,
) -> Result<Vec<String>, AppError> {
    ensure_workbench_stopped(data_dir)?;
    let kernel_version = kernel::read_active(data_dir).ok_or_else(|| {
        AppError::Patch("尚未激活内核版本，请先在「内核版本」页安装并切换到某一版本".into())
    })?;
    let (def, patch_dir) = find_patch(patches, id)?;
    if !version_in_range(def, &kernel_version) {
        return Err(AppError::Patch(format!(
            "补丁 {} 不适用于内核版本 {}（适用范围：{}）",
            def.name,
            kernel_version,
            range_text(def)
        )));
    }

    let mut state = read_state(data_dir);
    if find_applied(&state, id, &kernel_version).is_some() {
        return Err(AppError::Patch(format!(
            "补丁 {} 已应用到内核版本 {}，请先撤销后再重新应用",
            def.name, kernel_version
        )));
    }
    let mut applied = AppliedPatch {
        id: def.id.clone(),
        kernel_version: kernel_version.clone(),
        applied_at: crate::process::current_date_string(),
        patch_version: Some(def.version.clone()),
        notes: Vec::new(),
        files: Vec::new(),
    };
    let kernel_root = kernel::kernel_dir(data_dir, &kernel_version);
    let mut modified_any = false;

    for file in &def.files {
        let target = kernel_root.join(&file.to);
        // 闭包只操作本文件；跳过说明通过返回值带回，避免与循环体里
        // `applied.files.push` 的可变借用冲突。
        let apply_one = || -> Result<(Option<AppliedFile>, Vec<String>), AppError> {
            match file.mode.as_str() {
                "copy" => {
                    let from = file.from.as_deref().ok_or_else(|| {
                        AppError::Patch(format!("补丁 {} 的 copy 文件缺少 from", def.id))
                    })?;
                    let source = patch_dir.join(from);
                    let source_meta = fs::symlink_metadata(&source).map_err(|e| {
                        AppError::Patch(format!(
                            "补丁 {} 的源文件 {} 不存在：{e}",
                            def.id,
                            source.display()
                        ))
                    })?;
                    if source_meta.file_type().is_symlink() || !source_meta.is_file() {
                        return Err(AppError::Patch(format!(
                            "补丁 {} 的源文件 {} 不是普通文件",
                            def.id,
                            source.display()
                        )));
                    }
                    let bytes = fs::read(&source).map_err(|e| {
                        AppError::Patch(format!("读取 {} 失败：{e}", source.display()))
                    })?;
                    let patched_sha256 = sha256_bytes(&bytes);
                    ensure_no_symlink_ancestors(&target, &kernel_root)?;
                    let existing = sha256_file(&target).ok();
                    let expect = file
                        .expect_sha256
                        .as_deref()
                        .map(str::trim)
                        .filter(|h| !h.is_empty());
                    // 先裁决「是否允许写入 / 是否跳过」，再统一走备份 + 写入，
                    // 保证备份清理规则只有一条路径。
                    let skip_note: Option<String> = match expect {
                        Some(expected) => match existing.as_deref() {
                            // 目标与预期原文件一致 → 允许覆盖。
                            Some(sha) if sha.eq_ignore_ascii_case(expected) => None,
                            // 目标已是补丁后状态（手工打过 / 上一次应用残留）→ 不动。
                            Some(sha) if sha == patched_sha256.as_str() => None,
                            None if !file.required => {
                                Some(format!("跳过 {}：目标文件不存在（非必需）", file.to))
                            }
                            None => {
                                return Err(AppError::Patch(format!(
                                    "目标 {} 不存在：内核布局与补丁预期不符（预期原文件 SHA-256 {expected}），内核版本可能已升级",
                                    target.display()
                                )));
                            }
                            Some(sha) => {
                                return Err(AppError::Patch(format!(
                                    "目标 {} 内容与补丁预期的原文件不符（预期 SHA-256 {expected}，实际 {sha}）：内核版本可能已升级或文件已被其他工具修改，请确认内核版本后重试",
                                    target.display()
                                )));
                            }
                        },
                        None => match existing.as_deref() {
                            // 无预期哈希：新增文件语义。
                            None => None,
                            Some(sha) if sha == patched_sha256.as_str() => None,
                            Some(_) => {
                                return Err(AppError::Patch(format!(
                                    "目标 {} 已存在且内容与补丁不同（可能是用户文件或已失效的旧补丁），拒绝覆盖；请先撤销旧补丁或手动处理",
                                    target.display()
                                )));
                            }
                        },
                    };
                    if let Some(note) = skip_note {
                        return Ok((None, vec![note]));
                    }
                    let had_original = backup_target(data_dir, &def.id, &kernel_version, &file.to)?;
                    // 已处于补丁后状态时不重写（保留 mtime）；其余情况原样覆盖。
                    if existing.as_deref() != Some(patched_sha256.as_str()) {
                        write_bytes_at(&target, &bytes)?;
                    }
                    let backup_rel = if had_original {
                        Some(rel_backup(data_dir, id, &kernel_version, &file.to))
                    } else {
                        None
                    };
                    Ok((
                        Some(AppliedFile {
                            to: file.to.clone(),
                            had_original,
                            patched_sha256,
                            backup_rel,
                        }),
                        Vec::new(),
                    ))
                }
                "replace" => {
                    ensure_no_symlink_ancestors(&target, &kernel_root)?;
                    let had_original = backup_target(data_dir, &def.id, &kernel_version, &file.to)?;
                    let text = match fs::read_to_string(&target) {
                        Ok(text) => text,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            if had_original {
                                return Err(AppError::Patch("内部错误：备份存在但目标缺失".into()));
                            }
                            if !file.required {
                                return Ok((
                                    None,
                                    vec![format!("跳过 {}：目标文件不存在（非必需）", file.to)],
                                ));
                            }
                            return Err(AppError::Patch(format!(
                                "replace 目标 {} 不存在",
                                target.display()
                            )));
                        }
                        Err(e) => {
                            return Err(AppError::Patch(format!(
                            "无法读取 replace 目标 {}（{e}）；replace 模式只支持 UTF-8 文本文件",
                            target.display()
                        )))
                        }
                    };
                    let search = file.search.as_deref().unwrap_or("");
                    let count = text.matches(search).count();
                    if count == 0 {
                        if !file.required {
                            return Ok((
                                None,
                                vec![format!("跳过 {}：未找到匹配内容（非必需）", file.to)],
                            ));
                        }
                        return Err(AppError::Patch(format!(
                            "replace 目标 {} 中未找到待替换内容（该内核版本可能已包含此修改）",
                            target.display()
                        )));
                    }
                    let patched = text.replace(search, file.replacement.as_deref().unwrap_or(""));
                    write_bytes_at(&target, patched.as_bytes())?;
                    let backup_rel = if had_original {
                        Some(rel_backup(data_dir, id, &kernel_version, &file.to))
                    } else {
                        None
                    };
                    Ok((
                        Some(AppliedFile {
                            to: file.to.clone(),
                            had_original,
                            patched_sha256: sha256_bytes(patched.as_bytes()),
                            backup_rel,
                        }),
                        Vec::new(),
                    ))
                }
                other => Err(AppError::Patch(format!(
                    "补丁 {} 的文件模式 {other} 不支持",
                    def.id
                ))),
            }
        };
        match apply_one() {
            Ok((record, extra_notes)) => {
                applied.notes.extend(extra_notes);
                if let Some(record) = record {
                    modified_any = true;
                    applied.files.push(record);
                }
            }
            Err(e) => {
                // 部分文件已写入但后面失败：不落 state，已写入的文件与备份
                // 残留会让下一次应用被「备份未清理」挡住——把错误信息补充上
                // 可操作的下一步。
                return Err(AppError::Patch(format!(
                    "{e}（提示：本补丁 {id} 在失败前可能已写入部分文件；先撤销（会还原已备份文件）或清理 {} 后重试）",
                    backups_root(data_dir).join(id).display()
                )));
            }
        }
    }

    if !modified_any && applied.notes.is_empty() {
        return Err(AppError::Patch(format!(
            "补丁 {} 未产生任何修改（文件清单为空或全部失败）",
            def.name
        )));
    }
    // 全部文件被跳过（非必需未命中）也算「已应用（部分）」：保留记录与
    // 说明，UI 呈现 partial 状态，用户可据此撤销记录。真正空操作（无文件
    // 也无说明）在上面已被拒绝。
    state.applied.push(applied);
    let notes = state
        .applied
        .last()
        .map(|a| a.notes.clone())
        .unwrap_or_default();
    write_state(data_dir, &state)?;
    Ok(notes)
}

/// 备份相对路径（相对 `<data_dir>/patches/backups/`）。
fn rel_backup(data_dir: &Path, id: &str, kernel_version: &str, to: &str) -> String {
    backup_path(data_dir, id, kernel_version, to)
        .strip_prefix(backups_root(data_dir))
        .unwrap_or_else(|_| Path::new(""))
        .to_string_lossy()
        .into_owned()
}

/// 以「同目录临时文件 + rename」原子写目标文件。
fn write_bytes_at(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Patch(format!("无法创建目标目录 {}：{e}", parent.display())))?;
    }
    atomic_write(target, bytes)
        .map_err(|e| AppError::Patch(format!("无法写入 {}：{e}", target.display())))
}

/// 撤销补丁对当前激活内核的修改。
pub fn revert(
    data_dir: &Path,
    patches: &[(PatchDef, PathBuf)],
    id: &str,
) -> Result<Vec<String>, AppError> {
    ensure_workbench_stopped(data_dir)?;
    let kernel_version = kernel::read_active(data_dir).ok_or_else(|| {
        AppError::Patch("尚未激活内核版本，请先在「内核版本」页安装并切换到某一版本".into())
    })?;
    let _ = find_patch(patches, id); // 提示补丁在当前版本存在（仅用于错误文案，非必需）
    let mut state = read_state(data_dir);
    let record = {
        let found = find_applied(&state, id, &kernel_version)
            .cloned()
            .ok_or_else(|| {
                AppError::Patch(format!("补丁 {id} 未应用到内核版本 {kernel_version}"))
            })?;
        found
    };
    let mut warnings: Vec<String> = record.notes.clone();
    let kernel_root = kernel::kernel_dir(data_dir, &kernel_version);

    for file in &record.files {
        let target = kernel_root.join(&file.to);
        let target_sha = sha256_file(&target).ok();
        match &file.backup_rel {
            Some(backup_rel) => {
                let backup = backups_root(data_dir).join(backup_rel);
                match fs::read(&backup) {
                    Ok(bytes) => {
                        write_bytes_at(&target, &bytes).map_err(|e| {
                            AppError::Patch(format!("还原 {} 失败：{e}", target.display()))
                        })?;
                        let _ = fs::remove_file(&backup);
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        // 备份丢失 → 走哈希校验兜底分支。
                        warnings.push(format!(
                            "{}：原文件备份已丢失（{}），尝试按内容校验兜底",
                            file.to,
                            backup.display()
                        ));
                        handle_missing_backup(&target, file, target_sha.as_deref(), &mut warnings)?;
                    }
                    Err(e) => {
                        return Err(AppError::Patch(format!(
                            "无法读取备份 {}：{e}",
                            backup.display()
                        )))
                    }
                }
            }
            None => {
                // 无备份说明应用时目标不存在（纯新增文件）：校验后删除。
                match target_sha {
                    Some(sha) if sha == file.patched_sha256 => {
                        fs::remove_file(&target).map_err(|e| {
                            AppError::Patch(format!("无法删除补丁文件 {}：{e}", target.display()))
                        })?;
                        prune_empty_dirs(&target);
                    }
                    Some(_) => {
                        return Err(AppError::Patch(format!(
                            "补丁文件 {} 已被其他工具修改（内容与补丁不符），为安全起见不自动删除，请检查后手动处理",
                            target.display()
                        )));
                    }
                    None => {
                        // 目标已不存在：本就是新增文件，视为已还原。
                    }
                }
            }
        }
    }

    state
        .applied
        .retain(|a| a.id != id || a.kernel_version != kernel_version);
    // 尽量保留撤销后的状态文件（即便只剩空记录也写回，保证 UI 刷新一致）。
    write_state(data_dir, &state)?;
    // 清理可能的空备份目录（best-effort）。
    let _ = fs::remove_dir_all(backups_root(data_dir).join(id).join(&kernel_version));
    Ok(warnings)
}

/// 备份丢失时的兜底处理，返回后由调用方继续（或通过错误中止）。
fn handle_missing_backup(
    target: &Path,
    file: &AppliedFile,
    target_sha: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<(), AppError> {
    match target_sha {
        None => {
            if file.had_original {
                warnings.push(format!(
                    "{}：原文件备份已丢失且目标文件不存在（内核可能已重装），原文件无法恢复",
                    file.to
                ));
            }
            Ok(())
        }
        Some(sha) if sha == file.patched_sha256 => {
            if file.had_original {
                Err(AppError::Patch(format!(
                    "无法自动还原 {}：原文件备份已丢失（内核可能已重装或备份被清理），而目标仍是补丁后的内容。请重新安装该内核版本后重试，或手动处理该文件",
                    target.display()
                )))
            } else {
                // 纯新增文件：删除即还原。
                fs::remove_file(target)
                    .map_err(|e| AppError::Patch(format!("无法删除 {}：{e}", target.display())))?;
                prune_empty_dirs(target);
                Ok(())
            }
        }
        Some(_) => Err(AppError::Patch(format!(
            "无法自动还原 {}：目标文件已被修改（内容与补丁记录不一致），请检查后手动处理",
            target.display()
        ))),
    }
}

/// 删除文件后清理空目录（只删到内核根为止，不出界）。
fn prune_empty_dirs(file_path: &Path) {
    let mut dir = file_path.parent();
    while let Some(d) = dir {
        if fs::read_dir(d)
            .map(|mut e| e.next().is_none())
            .unwrap_or(false)
        {
            let _ = fs::remove_dir(d);
        } else {
            break;
        }
        dir = d.parent();
    }
}

fn range_text(def: &PatchDef) -> String {
    match (&def.min_kernel_version, &def.max_kernel_version) {
        (None, None) => "任意版本".to_string(),
        (Some(min), None) => format!("v{min} 及以上"),
        (None, Some(max)) => format!("v{max} 及以下"),
        (Some(min), Some(max)) => format!("v{min} ~ v{max}"),
    }
}

/// 计算某个应用记录在磁盘上的实际状态。
fn disk_state(data_dir: &Path, record: &AppliedPatch) -> (String, Vec<String>) {
    let kernel_root = kernel::kernel_dir(data_dir, &record.kernel_version);
    let mut problems = Vec::new();
    for file in &record.files {
        let target = kernel_root.join(&file.to);
        match sha256_file(&target) {
            Ok(sha) if sha == file.patched_sha256 => {}
            Ok(_) => problems.push(format!("{} 内容与补丁记录不一致", file.to)),
            Err(_) => problems.push(format!("{} 文件缺失", file.to)),
        }
    }
    if problems.is_empty() {
        ("applied".to_string(), Vec::new())
    } else {
        ("dirty".to_string(), problems)
    }
}

/// 组装设置页状态快照。`installed` 用于提示“无内核可应用”，`active` 是
/// 当前激活版本（可能为 None）。
pub fn status(data_dir: &Path, patches: &[(PatchDef, PathBuf)]) -> PatchStatus {
    let active = kernel::read_active(data_dir);
    let rows = patches
        .iter()
        .map(|(def, _)| row_for(data_dir, def, active.as_deref()))
        .collect();
    PatchStatus { patches: rows }
}

fn row_for(data_dir: &Path, def: &PatchDef, active: Option<&str>) -> PatchRow {
    let state = read_state(data_dir);
    let base = PatchRow {
        id: def.id.clone(),
        name: def.name.clone(),
        version: def.version.clone(),
        kind: def.kind.clone(),
        description: def.description.clone(),
        min_kernel_version: def.min_kernel_version.clone(),
        max_kernel_version: def.max_kernel_version.clone(),
        state: String::new(),
        state_text: String::new(),
        note: None,
        applied_at: None,
        enabled: false,
    };
    let Some(version) = active else {
        return PatchRow {
            state: "no_kernel".into(),
            state_text: "未安装 / 未激活内核".into(),
            note: Some("请先在「内核版本」页安装并激活一个版本".into()),
            ..base
        };
    };
    if !version_in_range(def, version) {
        let mut row = base;
        row.state = "incompatible".into();
        row.state_text = format!("不适用当前内核 v{version}");
        row.note = Some(format!("适用范围：{}", range_text(def)));
        return row;
    }
    let applied_elsewhere = state
        .applied
        .iter()
        .find(|a| a.id == def.id && a.kernel_version != version);
    let note_from_elsewhere = applied_elsewhere.map(|a| {
        format!(
            "已应用到其他内核版本 v{}，对当前内核 v{version} 未生效",
            a.kernel_version
        )
    });
    let mut row = base;
    match find_applied(&state, &def.id, version) {
        None => {
            row.state = "not_applied".into();
            row.state_text = "未应用".into();
            row.note = note_from_elsewhere;
            row.enabled = true;
        }
        Some(record) => {
            let patch_version_matches =
                record.patch_version.as_deref() == Some(def.version.as_str());
            let (state_code, mut problems) = disk_state(data_dir, record);
            if !patch_version_matches {
                problems.insert(
                    0,
                    format!(
                        "记录中的补丁版本 {} 与当前版本 {} 不一致",
                        record.patch_version.as_deref().unwrap_or("未知"),
                        def.version
                    ),
                );
            }
            row.applied_at = Some(record.applied_at.clone());
            if patch_version_matches
                && state_code == "applied"
                && record.files.is_empty()
                && !record.notes.is_empty()
            {
                // 所有文件都因未命中目标被跳过：补丁记录存在但没有实际修改。
                row.state = "partial".into();
                row.state_text = "已应用（文件未命中）".into();
                row.note = Some(record.notes.join("；"));
                row.enabled = true;
            } else if patch_version_matches && state_code == "applied" {
                row.state = "applied".into();
                row.state_text = "已应用".into();
                row.note = note_from_elsewhere;
                row.enabled = true;
            } else {
                row.state = "dirty".into();
                row.state_text = if patch_version_matches {
                    "文件已被改动".into()
                } else {
                    "补丁版本已更新".into()
                };
                row.note = Some(problems.join("；") + "（请先撤销旧补丁记录，再重新应用当前版本）");
                row.enabled = true;
            }
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个迷你「资源目录」：两个补丁（copy / replace）加一个 shell 侧测试桩。
    fn make_resource_root(root: &Path) -> PathBuf {
        let res = root.join("resources").join("patches");
        // copy 模式补丁
        let copy_dir = res.join("hello-copy");
        fs::create_dir_all(copy_dir.join("files")).unwrap();
        fs::write(
            copy_dir.join("files").join("hello.js"),
            "module.exports = 42;\n",
        )
        .unwrap();
        fs::write(
            copy_dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "hello-copy",
                    "name": "示例拷贝补丁",
                    "version": "1.0.0",
                    "kind": "plugin",
                    "description": "test",
                    "files": [{
                        "mode": "copy",
                        "from": "files/hello.js",
                        "to": "node_modules/@deepseek-ai/dsh/lib/xlink-hello.js",
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        // replace 模式补丁
        let rep_dir = res.join("anno-replace");
        fs::create_dir_all(&rep_dir).unwrap();
        fs::write(
            rep_dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "anno-replace",
                    "name": "示例替换补丁",
                    "version": "1.0.0",
                    "kind": "patch",
                    "description": "test",
                    "files": [{
                        "mode": "replace",
                        "search": "\"private\":true",
                        "replacement": "\"private\":true,\"dshXlinkPatched\":true",
                        "to": "package.json",
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        // 不适用版本的补丁
        let ver_dir = res.join("version-gated");
        fs::create_dir_all(&ver_dir).unwrap();
        fs::write(
            ver_dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "version-gated",
                    "name": "版本限定补丁",
                    "version": "1.0.0",
                    "kind": "patch",
                    "description": "test",
                    "minKernelVersion": "0.2.0",
                    "files": [{
                        "mode": "copy",
                        "from": "files/hello.js",
                        "to": "x.js",
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        res
    }

    fn setup(data_dir: &Path, version: &str) {
        fs::create_dir_all(kernel::kernel_dir(data_dir, version)).unwrap();
        // 内核 stub package.json（与 kernel.rs 安装流程写出的形状一致）
        let stub = format!(
            "{{\"name\":\"dsh-kernel-{}\",\"private\":true,\"version\":\"1.0.0\"}}\n",
            version.replace('.', "_")
        );
        fs::write(
            kernel::kernel_dir(data_dir, version).join("package.json"),
            stub,
        )
        .unwrap();
        kernel::write_active(data_dir, Some(version)).unwrap();
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "dsh-patch-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn apply_and_revert_copy_patch() {
        let root = temp_root("copy");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();

        let notes = apply(&data, &patches, "hello-copy").unwrap();
        assert!(notes.is_empty());
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh/lib/xlink-hello.js");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "module.exports = 42;\n"
        );
        // 备份目录中没有原文件（目标原本不存在，hadOriginal=false → backup_rel=None）
        let state = read_state(&data);
        let record = find_applied(&state, "hello-copy", "0.1.2").unwrap();
        assert!(!record.files[0].had_original);

        // 再应用应被拒绝（已应用）
        assert!(apply(&data, &patches, "hello-copy").is_err());

        let warnings = revert(&data, &patches, "hello-copy").unwrap();
        assert!(warnings.is_empty());
        assert!(!target.exists());
        assert!(read_state(&data).applied.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_over_existing_identical_is_idempotent_and_revert_restores() {
        let root = temp_root("idem");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh/lib/xlink-hello.js");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "module.exports = 42;\n").unwrap(); // 用户已有同名同内容文件

        apply(&data, &patches, "hello-copy").unwrap();
        let state = read_state(&data);
        let record = find_applied(&state, "hello-copy", "0.1.2").unwrap();
        assert!(record.files[0].had_original);
        assert!(record.files[0].backup_rel.is_some());

        revert(&data, &patches, "hello-copy").unwrap();
        // 原文件被还原（内容不变）
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "module.exports = 42;\n"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_refuses_overwriting_unknown_content() {
        let root = temp_root("clash");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh/lib/xlink-hello.js");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "user content\n").unwrap();

        let error = apply(&data, &patches, "hello-copy").unwrap_err();
        assert!(error.to_string().contains("拒绝覆盖"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn replace_patch_modifies_and_restores() {
        let root = temp_root("replace");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();

        apply(&data, &patches, "anno-replace").unwrap();
        let stub =
            fs::read_to_string(kernel::kernel_dir(&data, "0.1.2").join("package.json")).unwrap();
        assert!(stub.contains("\"dshXlinkPatched\":true"));

        revert(&data, &patches, "anno-replace").unwrap();
        let stub =
            fs::read_to_string(kernel::kernel_dir(&data, "0.1.2").join("package.json")).unwrap();
        assert!(!stub.contains("dshXlinkPatched"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn version_gated_patch_is_rejected_and_reported_incompatible() {
        let root = temp_root("gate");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();

        let error = apply(&data, &patches, "version-gated").unwrap_err();
        assert!(error.to_string().contains("不适用于内核版本"));

        let snapshot = status(&data, &patches);
        let row = snapshot
            .patches
            .iter()
            .find(|r| r.id == "version-gated")
            .unwrap();
        assert_eq!(row.state, "incompatible");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn revert_with_dirty_file_refuses_unknown_content() {
        let root = temp_root("dirty");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();
        apply(&data, &patches, "hello-copy").unwrap();
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh/lib/xlink-hello.js");
        // 应用后文件被用户改掉：撤销必须以内容校验兜底，拒绝盲目操作
        fs::write(&target, "user edit\n").unwrap();

        let error = revert(&data, &patches, "hello-copy").unwrap_err();
        assert!(error.to_string().contains("已被其他工具修改"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_with_expect_sha256_overwrites_and_restores() {
        let root = temp_root("expect-ok");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = root.join("res").join("patches");
        let dir = res.join("overwrite");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files").join("new.js"), "patched content\n").unwrap();
        let original = "original content\n";
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "overwrite",
                    "name": "覆盖型补丁",
                    "version": "1.0.0",
                    "files": [{
                        "mode": "copy",
                        "from": "files/new.js",
                        "to": "node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js",
                        "expectSha256": sha256_bytes(original.as_bytes()),
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let patches = load_patches(&res).unwrap();
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, original).unwrap();

        apply(&data, &patches, "overwrite").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "patched content\n");

        revert(&data, &patches, "overwrite").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_with_expect_sha256_mismatch_refuses() {
        let root = temp_root("expect-mismatch");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = root.join("res").join("patches");
        let dir = res.join("overwrite");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files").join("new.js"), "patched content\n").unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "overwrite",
                    "name": "覆盖型补丁",
                    "version": "1.0.0",
                    "files": [{
                        "mode": "copy",
                        "from": "files/new.js",
                        "to": "node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js",
                        "expectSha256": "a".repeat(64),
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let patches = load_patches(&res).unwrap();
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "other version content\n").unwrap();

        let error = apply(&data, &patches, "overwrite").unwrap_err();
        assert!(error.to_string().contains("与补丁预期的原文件不符"));
        // 拒绝后原文件必须原样保留。
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "other version content\n"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_with_expect_sha256_missing_target_required_false_skips() {
        let root = temp_root("expect-skip");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = root.join("res").join("patches");
        let dir = res.join("overwrite");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files").join("new.js"), "patched content\n").unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "overwrite",
                    "name": "覆盖型补丁",
                    "version": "1.0.0",
                    "files": [{
                        "mode": "copy",
                        "from": "files/new.js",
                        "to": "node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js",
                        "expectSha256": "a".repeat(64),
                        "required": false
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let patches = load_patches(&res).unwrap();

        let notes = apply(&data, &patches, "overwrite").unwrap();
        assert!(notes.iter().any(|n| n.contains("跳过")));
        let record = read_state(&data)
            .applied
            .into_iter()
            .find(|a| a.id == "overwrite")
            .unwrap();
        assert!(record.files.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn manifest_with_escaping_to_is_rejected() {
        let root = temp_root("escape");
        let res = root.join("res").join("patches");
        let dir = res.join("evil");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "evil",
                    "name": "evil",
                    "version": "1.0.0",
                    "files": [{
                        "mode": "copy",
                        "from": "files/a.js",
                        "to": "../../outside.js",
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let patches = load_patches(&res).unwrap();
        assert!(patches.is_empty(), "越界路径的补丁必须被拒绝");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn status_reflects_applied_and_dirty_states() {
        let root = temp_root("status");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();

        let snapshot = status(&data, &patches);
        let row = snapshot
            .patches
            .iter()
            .find(|r| r.id == "hello-copy")
            .unwrap();
        assert_eq!(row.state, "not_applied");

        apply(&data, &patches, "hello-copy").unwrap();
        let snapshot = status(&data, &patches);
        let row = snapshot
            .patches
            .iter()
            .find(|r| r.id == "hello-copy")
            .unwrap();
        assert_eq!(row.state, "applied");

        // 删除补丁文件 → dirty
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh/lib/xlink-hello.js");
        fs::remove_file(&target).unwrap();
        let snapshot = status(&data, &patches);
        let row = snapshot
            .patches
            .iter()
            .find(|r| r.id == "hello-copy")
            .unwrap();
        assert_eq!(row.state, "dirty");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn status_marks_patch_definition_update_as_dirty() {
        let root = temp_root("patch-version");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();

        apply(&data, &patches, "hello-copy").unwrap();
        let mut updated = patches.clone();
        updated
            .iter_mut()
            .find(|(def, _)| def.id == "hello-copy")
            .unwrap()
            .0
            .version = "1.1.0".into();

        let row = status(&data, &updated)
            .patches
            .iter()
            .find(|r| r.id == "hello-copy")
            .unwrap()
            .clone();
        assert_eq!(row.state, "dirty");
        assert_eq!(row.state_text, "补丁版本已更新");
        assert!(row.note.unwrap().contains("1.0.0"));

        // A stale record remains revertible so the user can install the new definition.
        revert(&data, &updated, "hello-copy").unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn legacy_patch_record_without_version_is_dirty() {
        let root = temp_root("legacy-version");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();

        apply(&data, &patches, "hello-copy").unwrap();
        let state_path = state_file(&data);
        let mut state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        state["applied"][0]
            .as_object_mut()
            .unwrap()
            .remove("patchVersion");
        fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

        let row = status(&data, &patches)
            .patches
            .iter()
            .find(|r| r.id == "hello-copy")
            .unwrap()
            .clone();
        assert_eq!(row.state, "dirty");
        assert_eq!(row.state_text, "补丁版本已更新");
        assert!(row.note.unwrap().contains("未知"));

        revert(&data, &patches, "hello-copy").unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn replace_on_missing_target_with_required_false_skips_patch() {
        let root = temp_root("skip");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = root.join("res").join("patches");
        let dir = res.join("opt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "opt",
                    "name": "可选补丁",
                    "version": "1.0.0",
                    "files": [{
                        "mode": "replace",
                        "search": "needle",
                        "replacement": "hay",
                        "to": "node_modules/@deepseek-ai/dsh/lib/missing.js",
                        "required": false
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let patches = load_patches(&res).unwrap();
        let notes = apply(&data, &patches, "opt").unwrap();
        assert!(notes.iter().any(|n| n.contains("跳过")));
        let state = read_state(&data);
        let record = find_applied(&state, "opt", "0.1.2").unwrap();
        assert!(record.files.is_empty());
        assert!(!record.notes.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}
