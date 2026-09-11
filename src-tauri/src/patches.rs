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
    /// 当补丁功能已被官方内核采纳时，声明从哪个内核版本开始不再需要应用本补丁。
    ///
    /// 该字段为 `None` 表示补丁一直有效；非 `None` 时，对当前激活版本
    /// `>=` 此值的内核，设置页会标记该补丁为「已并入官方内核」（默认折叠展示、
    /// 应用按钮禁用），但已应用到旧内核的记录仍可正常撤销。
    ///
    /// 语义区别于 `maxKernelVersion`：后者直接拒绝在更高版本上**安装**，而
    /// `supersededSinceKernelVersion` 仅表达「该版本后已不需要」，与
    /// `minKernelVersion` / `maxKernelVersion` 共存——比如
    /// `minKernelVersion: 0.1.1-rc.2`、`supersededSinceKernelVersion: 0.1.2-alpha.2`
    /// 表示仅 `0.1.1-rc.2 ~ <0.1.2-alpha.2` 真正需要手动应用。
    #[serde(default)]
    pub superseded_since_kernel_version: Option<String>,
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
    /// 应用前原文件内容的 SHA-256（应用时目标不存在则为 `None`）。
    ///
    /// 备份丢失时靠它判断"该文件是否已经回到应用前的状态"：撤销中途失败后
    /// 用户再次点「撤销」时，那些已经还原过、备份随之被删掉的文件正是靠这个
    /// 哈希被认出来的，否则会被误报成"目标已被修改"而永久卡死。
    #[serde(
        rename = "originalSha256",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_sha256: Option<String>,
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
    /// 当补丁功能已被官方内核采纳时，声明从哪个内核版本开始不再需要应用本补丁。
    /// 该字段透传自 `PatchDef.supersededSinceKernelVersion`，由 UI 决定如何
    /// 展示「已并入官方内核」徽标与默认折叠卡片。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_since_kernel_version: Option<String>,
    /// 对当前激活内核，补丁功能是否已被官方内核采纳。UI 据此渲染删除线 +
    /// 默认折叠 + 禁用「应用」按钮；已应用记录的撤销不受影响。
    pub superseded: bool,
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
    /// 补丁记录（`state.json`）读不出来时的说明。非空时 UI 必须显示它：
    /// 此时所有补丁都显示为"未应用"，而磁盘上可能仍是打过补丁的内容。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
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
/// 便捷包装：只要清单、不要告警。生产路径用
/// [`load_patches_with_warnings`]，因为被跳过的清单必须让用户看到（P2-12）；
/// 单测里只关心"加载到了哪些补丁"，因此保留这个只会编译进测试的形式。
#[cfg(test)]
pub fn load_patches(resource_root: &Path) -> Result<Vec<(PatchDef, PathBuf)>, String> {
    Ok(load_patches_with_warnings(resource_root)?.0)
}

/// 已加载的内置补丁清单，以及加载期间被跳过的条目的原因。
pub type LoadedPatches = (Vec<(PatchDef, PathBuf)>, Vec<String>);

/// 与 [`load_patches`] 相同，但把每个被跳过的清单/定义的原因也返回给调用方。
///
/// 旧行为只把这些原因 `eprintln` 掉：清单坏掉的补丁在设置页直接**消失**，
/// 用户无法区分"这个版本没带它"与"它坏了"（P2-12）。
pub fn load_patches_with_warnings(resource_root: &Path) -> Result<LoadedPatches, String> {
    let mut warnings: Vec<String> = Vec::new();
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
                let reason = format!("补丁目录 {} 缺少 manifest.json（{e}）", dir.display());
                eprintln!("dsh-xlink: 跳过无效补丁目录：{reason}");
                warnings.push(reason);
                continue;
            }
        };
        let manifest: PatchManifest = match serde_json::from_str(&text) {
            Ok(manifest) => manifest,
            Err(e) => {
                let reason = format!("补丁清单 {} 无法解析：{e}", manifest_path.display());
                eprintln!("dsh-xlink: 跳过无效补丁清单：{reason}");
                warnings.push(reason);
                continue;
            }
        };
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            let reason = format!(
                "补丁清单 {} 的 schemaVersion 是 {}，本版本只支持 {}",
                manifest_path.display(),
                manifest.schema_version,
                MANIFEST_SCHEMA_VERSION
            );
            eprintln!("dsh-xlink: 跳过不支持的补丁清单版本：{reason}");
            warnings.push(reason);
            continue;
        }
        for def in manifest.patches {
            if let Err(reason) = validate_def(&def) {
                let label = if def.id.is_empty() {
                    "<无 id>".to_string()
                } else {
                    def.id.clone()
                };
                eprintln!("dsh-xlink: 跳过补丁 {label}：{reason}");
                warnings.push(format!("补丁 {label} 定义非法：{reason}"));
                continue;
            }
            out.push((def, dir.clone()));
        }
    }
    Ok((out, warnings))
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
                let from = file.from.as_deref().unwrap_or("");
                if from.is_empty() {
                    return Err(format!("files[].to={} 的 copy 模式缺少 from", file.to));
                }
                // `from` 是相对补丁目录的**资源**路径，和 `to` 一样必须留在
                // 补丁目录内。旧实现只校验 `to`：`from: "../../../../etc/passwd"`
                // 会被 `patch_dir.join(from)` 解析到补丁目录之外，绝对路径更会
                // 直接丢弃 `patch_dir`，与文档「载荷按包名保存在 files/ 下」的
                // 前提不符（P2-11）。
                if let Err(e) = check_target_path(from) {
                    return Err(format!("files[].from 非法（{from}）：{e}"));
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

/// 拒绝让补丁写到内核目录之外：判据是**真实路径仍然落在内核根之内**，而不是
/// "路径里不能出现符号链接"。
///
/// 旧实现见链接即拒，在 pnpm 的 isolated linker 布局下等于永远无法应用补丁：
/// `node_modules/<pkg>` 本身就是指向
/// `node_modules/.pnpm/<pkg>@<ver>/node_modules/<pkg>` 的符号链接，目标明明还
/// 在同一个内核目录里，却会被一刀切拒绝，而且文案没给出任何下一步（P2-15）。
///
/// 现在允许指向内核内部的链接，仍然挡住任何指到内核之外的（例如
/// `node_modules/<pkg> -> /etc`）。目标本身可能还不存在（copy 新增文件），
/// 因此从最深的已存在祖先开始解析。
fn ensure_no_symlink_ancestors(target: &Path, kernel_root: &Path) -> Result<(), AppError> {
    let mut probe = target.parent();
    let existing = loop {
        match probe {
            Some(dir) if dir.exists() => break dir.to_path_buf(),
            Some(dir) => probe = dir.parent(),
            None => return Ok(()),
        }
    };
    let real_root = kernel_root
        .canonicalize()
        .unwrap_or_else(|_| kernel_root.to_path_buf());
    let real_existing = existing.canonicalize().map_err(|e| {
        AppError::Patch(format!(
            "无法解析 {} 的真实路径：{e}（请检查该目录是否存在且可访问）",
            existing.display()
        ))
    })?;
    if !real_existing.starts_with(&real_root) {
        return Err(AppError::Patch(format!(
            "拒绝写入 {}：它经由 {} 解析到内核目录之外（{}）。请重新安装该内核版本以恢复目录结构",
            target.display(),
            existing.display(),
            real_existing.display()
        )));
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

/// 展示路径用的容错读取：文件不存在或损坏都返回空状态。
///
/// **只读**：`apply` / `revert` 这类"读-改-写"路径必须用 [`read_state_checked`]，
/// 否则一次解析失败就会让所有应用记录消失——磁盘上是补丁后的内容、备份还在，
/// 而壳认为"没打过补丁"：既不会显示 dirty，也无法撤销，重装还会被残留备份挡住。
fn read_state(data_dir: &Path) -> PatchState {
    match crate::process::read_state_file(&state_file(data_dir)) {
        crate::process::StateRead::Loaded(state) => state,
        crate::process::StateRead::Missing | crate::process::StateRead::Corrupt { .. } => {
            PatchState::default()
        }
    }
}

/// 读-改-写路径用的读取：记录损坏时返回可操作的错误。
fn read_state_checked(data_dir: &Path) -> Result<PatchState, AppError> {
    match crate::process::read_state_file(&state_file(data_dir)) {
        crate::process::StateRead::Loaded(state) => Ok(state),
        crate::process::StateRead::Missing => Ok(PatchState::default()),
        crate::process::StateRead::Corrupt { reason } => Err(AppError::Patch(format!(
            "补丁记录损坏，为避免丢掉「哪些补丁已应用」的信息，本次操作已中止（{reason}）。请修复或删除该文件后重试",
        ))),
    }
}

/// 记录文件读不出来时的说明，供设置页横幅展示。
fn state_integrity_warning(data_dir: &Path) -> Option<String> {
    match crate::process::read_state_file::<PatchState>(&state_file(data_dir)) {
        crate::process::StateRead::Corrupt { reason } => Some(format!(
            "补丁记录损坏，暂时无法确认哪些补丁已应用（{reason}）。内核里可能仍是打过补丁的内容——请修复或删除该文件，或直接重装该内核版本；在修复之前不会写入任何补丁记录"
        )),
        _ => None,
    }
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
/// 应用前备份目标原文件。返回原文件内容的 SHA-256；目标原本不存在时返回
/// `None`（无备份，撤销时按"新增文件"语义删除）。
fn backup_target(
    data_dir: &Path,
    id: &str,
    kernel_version: &str,
    to: &str,
) -> Result<Option<String>, AppError> {
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
            // 备份内容的哈希就是原文件的哈希；应用记录里带上它，撤销才可能
            // 在备份丢失后判断出"这个文件已经还原过了"。
            let original_sha256 = sha256_file(&backup)
                .map_err(|e| AppError::Patch(format!("无法校验备份 {}：{e}", backup.display())))?;
            Ok(Some(original_sha256))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::Patch(format!(
            "无法检查目标 {}：{e}",
            target.display()
        ))),
    }
}

/// 工作台运行期间不允许动内核目录（与「切换内核版本」同一规则）。
///
/// 判据不看配置端口：内核可能是在用户修改端口设置之前启动的，此刻它仍然
/// 绑在旧端口上——以配置端口探测会误判成"已停止"，于是补丁会在一个正在
/// 服务的内核文件树里写入，重启后内核加载到的是半新半旧的混合状态。
fn ensure_workbench_stopped(data_dir: &Path) -> Result<(), AppError> {
    let current = settings::load(data_dir);
    if kernel::workbench_running(data_dir, &current) {
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

/// 补丁对当前内核是否已被官方采纳——当 `superseded_since_kernel_version` 声明且
/// 当前内核版本 `>=` 该值时返回 `true`。
fn is_superseded(def: &PatchDef, kernel_version: &str) -> bool {
    def.superseded_since_kernel_version
        .as_deref()
        .map(|bound| cmp_versions(kernel_version, bound) != std::cmp::Ordering::Less)
        .unwrap_or(false)
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
    if is_superseded(def, &kernel_version) {
        return Err(AppError::Patch(format!(
            "补丁 {} 已被官方内核 v{} 及以上版本取代，无需手动应用；设置页已折叠该条目",
            def.name,
            def.superseded_since_kernel_version
                .as_deref()
                .unwrap_or("?"),
        )));
    }

    let mut state = read_state_checked(data_dir)?;
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

    // --- 阶段 1：纯校验，不碰磁盘 -------------------------------------------
    //
    // 校验与写入必须分开。边校验边写时，靠后的文件校验失败会把前面已经写入
    // 的文件留在内核里，而 state.json 不落盘——用户手里是一个"半补丁"内核，
    // 且应用内没有任何恢复路径：撤销找不到记录（只有残留备份），重试又被那份
    // 残留备份挡住。
    let mut plan: Vec<PlannedFile> = Vec::new();
    let mut skip_notes: Vec<String> = Vec::new();
    for file in &def.files {
        match plan_file(def, patch_dir, &kernel_root, file)? {
            Planned::Skip(note) => skip_notes.push(note),
            Planned::Write(planned) => {
                if planned.already_patched {
                    skip_notes.push(format!(
                        "{}：目标已是补丁后的内容，本次未重写；撤销时没有可恢复的原文件（需要还原请重新安装该内核版本）",
                        planned.to
                    ));
                }
                plan.push(planned);
            }
        }
    }
    if plan.is_empty() && skip_notes.is_empty() {
        return Err(AppError::Patch(format!(
            "补丁 {} 未产生任何修改（文件清单为空或全部失败）",
            def.name
        )));
    }

    // 文件级占用检查：同一个文件不允许被两个补丁同时持有。
    //
    // 没有这道闸时，A、B 两个补丁可以先后改同一个文件：撤销 A 会用它自己的
    // 备份直接覆盖，把 B 的改动静默丢掉；再撤销 B 又写回 B 的备份，最终内核
    // 带着 A 的改动运行，而 state.json 里已经没有任何记录——既不显示 dirty，
    // 也无法再撤销。纯校验，不产生任何副作用。
    {
        let occupied: Vec<(String, String)> = state
            .applied
            .iter()
            .filter(|applied| applied.kernel_version == kernel_version && applied.id != id)
            .flat_map(|applied| {
                applied
                    .files
                    .iter()
                    .map(|file| (file.to.clone(), applied.id.clone()))
            })
            .collect();
        for planned in &plan {
            if let Some((_, other)) = occupied.iter().find(|(to, _)| to == &planned.to) {
                return Err(AppError::Patch(format!(
                    "目标 {} 已被补丁 {other} 应用过：同一文件不能被两个补丁同时修改（撤销其中一个会静默覆盖另一个的改动）。请先撤销 {other}，再应用本补丁",
                    planned.to
                )));
            }
        }
    }

    // --- 阶段 2：执行；任何一步失败都按本次备份整体回滚 ----------------------
    for planned in &plan {
        match commit_file(data_dir, id, &kernel_version, &kernel_root, planned) {
            Ok(record) => applied.files.push(record),
            Err(error) => {
                // 失败文件自身的备份（写入阶段才失败时它已经生成）不在
                // committed 列表里，单独清理，避免残留备份挡住下一次应用。
                let _ = fs::remove_file(backup_path(data_dir, id, &kernel_version, &planned.to));
                let rollback =
                    rollback_files(data_dir, id, &kernel_version, &kernel_root, &applied.files);
                return Err(AppError::Patch(format!(
                    "{error}{rollback}（补丁 {id} 未应用，本次已写入的内核文件已回到应用前的状态）"
                )));
            }
        }
    }

    // 全部文件被跳过（非必需未命中）也算「已应用（部分）」：保留记录与说明，
    // UI 呈现 partial 状态，用户可据此撤销记录。真正空操作（无文件也无说明）
    // 在上面已被拒绝。
    applied.notes = skip_notes;
    let notes = applied.notes.clone();
    state.applied.push(applied);
    write_state(data_dir, &state)?;
    Ok(notes)
}

/// 校验阶段产出的单个文件执行计划。计划阶段只读，不改动内核目录。
struct PlannedFile {
    to: String,
    /// 要写入目标文件的完整内容。
    payload: Vec<u8>,
    /// 写入后目标应有的内容哈希（记入 state）。
    patched_sha256: String,
    /// 目标当前已是补丁后的内容：执行阶段跳过写入以保留 mtime。
    already_patched: bool,
}

enum Planned {
    Skip(String),
    Write(PlannedFile),
}

/// 校验一个文件能否安全应用，并算出要写入的内容。不产生任何副作用。
fn plan_file(
    def: &PatchDef,
    patch_dir: &Path,
    kernel_root: &Path,
    file: &PatchFileDef,
) -> Result<Planned, AppError> {
    let target = kernel_root.join(&file.to);
    match file.mode.as_str() {
        "copy" => {
            let from = file
                .from
                .as_deref()
                .ok_or_else(|| AppError::Patch(format!("补丁 {} 的 copy 文件缺少 from", def.id)))?;
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
            let bytes = fs::read(&source)
                .map_err(|e| AppError::Patch(format!("读取 {} 失败：{e}", source.display())))?;
            let patched_sha256 = sha256_bytes(&bytes);
            ensure_no_symlink_ancestors(&target, kernel_root)?;
            let existing = sha256_file(&target).ok();
            let expect = file
                .expect_sha256
                .as_deref()
                .map(str::trim)
                .filter(|h| !h.is_empty());
            // 目标已是补丁后状态（手工打过 / 上一次应用残留）→ 允许通过，
            // 但执行阶段不重写，保留 mtime。
            let already_patched = existing.as_deref() == Some(patched_sha256.as_str());
            match expect {
                Some(expected) => match existing.as_deref() {
                    // 目标与预期原文件一致 → 允许覆盖。
                    Some(sha) if sha.eq_ignore_ascii_case(expected) => {}
                    Some(_) if already_patched => {}
                    None if !file.required => {
                        return Ok(Planned::Skip(format!(
                            "跳过 {}：目标文件不存在（非必需）",
                            file.to
                        )))
                    }
                    None => {
                        return Err(AppError::Patch(format!(
                            "目标 {} 不存在：内核布局与补丁预期不符（预期原文件 SHA-256 {expected}），内核版本可能已升级",
                            target.display()
                        )))
                    }
                    Some(sha) => {
                        return Err(AppError::Patch(format!(
                            "目标 {} 内容与补丁预期的原文件不符（预期 SHA-256 {expected}，实际 {sha}）：内核版本可能已升级或文件已被其他工具修改，请确认内核版本后重试",
                            target.display()
                        )))
                    }
                },
                None => match existing.as_deref() {
                    // 无预期哈希：新增文件语义。
                    None => {}
                    Some(_) if already_patched => {}
                    Some(_) => {
                        return Err(AppError::Patch(format!(
                            "目标 {} 已存在且内容与补丁不同（可能是用户文件或已失效的旧补丁），拒绝覆盖；请先撤销旧补丁或手动处理",
                            target.display()
                        )))
                    }
                },
            }
            Ok(Planned::Write(PlannedFile {
                to: file.to.clone(),
                payload: bytes,
                patched_sha256,
                already_patched,
            }))
        }
        "replace" => {
            ensure_no_symlink_ancestors(&target, kernel_root)?;
            let text = match fs::read_to_string(&target) {
                Ok(text) => text,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    if !file.required {
                        return Ok(Planned::Skip(format!(
                            "跳过 {}：目标文件不存在（非必需）",
                            file.to
                        )));
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
            // 空搜索串会让 `replace` 在每个字符之间插入替换文本，必然毁掉整个
            // 文件；缺失 search 属于清单错误，直接拒绝而不是"尽力而为"。
            if search.is_empty() {
                return Err(AppError::Patch(format!(
                    "补丁 {} 的目标 {} 缺少 search 字符串，拒绝执行（空搜索串会破坏整个文件）",
                    def.id, file.to
                )));
            }
            if !text.contains(search) {
                if !file.required {
                    return Ok(Planned::Skip(format!(
                        "跳过 {}：未找到匹配内容（非必需）",
                        file.to
                    )));
                }
                return Err(AppError::Patch(format!(
                    "replace 目标 {} 中未找到待替换内容（该内核版本可能已包含此修改）",
                    target.display()
                )));
            }
            let patched = text.replace(search, file.replacement.as_deref().unwrap_or(""));
            let payload = patched.into_bytes();
            let patched_sha256 = sha256_bytes(&payload);
            Ok(Planned::Write(PlannedFile {
                to: file.to.clone(),
                payload,
                patched_sha256,
                already_patched: false,
            }))
        }
        other => Err(AppError::Patch(format!(
            "补丁 {} 的文件模式 {other} 不支持",
            def.id
        ))),
    }
}

/// 执行阶段：备份 + 写入，返回写入记录。
fn commit_file(
    data_dir: &Path,
    id: &str,
    kernel_version: &str,
    kernel_root: &Path,
    planned: &PlannedFile,
) -> Result<AppliedFile, AppError> {
    let target = kernel_root.join(&planned.to);
    // 目标在本次应用**之前**就已经是补丁后的内容（手工打过 / 上一次应用的
    // 记录丢了）：绝不能给它做备份。备份下来的会是补丁内容本身，于是"撤销"
    // 把补丁内容原样写回去、报告「已撤销」，而内核仍在跑补丁代码、记录却已
    // 删除 —— 状态与磁盘彻底脱节（P2-9）。这里改为不写备份、不写文件，
    // 并以 `had_original = true` 记录"应用前它已存在但没有可恢复的原文件"，
    // 撤销时会走 `handle_missing_backup` 的「原文件备份已丢失」分支，如实
    // 告诉用户需要重装内核版本。
    if planned.already_patched {
        return Ok(AppliedFile {
            to: planned.to.clone(),
            had_original: sha256_file(&target).ok().is_some(),
            patched_sha256: planned.patched_sha256.clone(),
            backup_rel: None,
            original_sha256: None,
        });
    }
    let original_sha256 = backup_target(data_dir, id, kernel_version, &planned.to)?;
    write_bytes_at(&target, &planned.payload)?;
    Ok(AppliedFile {
        to: planned.to.clone(),
        had_original: original_sha256.is_some(),
        patched_sha256: planned.patched_sha256.clone(),
        backup_rel: original_sha256
            .as_ref()
            .map(|_| rel_backup(data_dir, id, kernel_version, &planned.to)),
        original_sha256,
    })
}

/// 回滚本次执行已经写入的文件，并清理本次产生的备份。返回需要用户知道的
/// 残留问题（空串表示回滚干净）。
fn rollback_files(
    data_dir: &Path,
    id: &str,
    kernel_version: &str,
    kernel_root: &Path,
    committed: &[AppliedFile],
) -> String {
    let mut problems: Vec<String> = Vec::new();
    // 应用前就已存在、但本次没有写入过的文件（`already_patched`）。它们没有
    // 备份，也**不属于本次改动**，回滚必须原样留着。
    let mut untouched: usize = 0;
    for applied in committed.iter().rev() {
        let target = kernel_root.join(&applied.to);
        match &applied.backup_rel {
            Some(rel) => {
                let backup = backups_root(data_dir).join(rel);
                match fs::read(&backup) {
                    Ok(bytes) => {
                        if let Err(e) = write_bytes_at(&target, &bytes) {
                            problems.push(format!("{}：{e}", applied.to));
                        }
                        let _ = fs::remove_file(&backup);
                    }
                    Err(e) => problems.push(format!("{}：备份不可读（{e}）", applied.to)),
                }
            }
            // 无备份有两种来源，绝不能都当成"本次新建的文件"：
            // 1. 应用前目标不存在（`had_original == false`）→ 删掉我们新建的文件；
            // 2. 应用前目标**已经**是补丁内容（`had_original == true`，见
            //    `commit_file` 的 `already_patched` 分支）→ 本次一个字节都没写过
            //    它，删掉它就是拿回滚当删除用：内核里的既有文件凭空消失，而错误
            //    文案还写着"已回到应用前的状态"（P0-1）。
            None => {
                if applied.had_original {
                    untouched += 1;
                    continue;
                }
                let _ = fs::remove_file(&target);
                prune_empty_dirs(&target, kernel_root);
            }
        }
    }
    let _ = fs::remove_dir_all(backups_root(data_dir).join(id).join(kernel_version));
    let mut note = String::new();
    if untouched > 0 {
        note.push_str(&format!(
            "；另有 {untouched} 个文件在应用前就已存在且本次未写入，回滚保留原样"
        ));
    }
    if problems.is_empty() {
        note
    } else {
        format!(
            "{note}；回滚未完成的文件：{}（请手动检查）",
            problems.join("、")
        )
    }
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
    let mut state = read_state_checked(data_dir)?;
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

    // 逐个文件还原，并且**每还原一个就把进度写回 state.json**。
    //
    // 一次性"全成功才写 state"是撤销卡死的根因：第一个文件还原成功后备份
    // 被删除，第二个文件失败时记录仍然完整，用户再次点「撤销」会从第一个
    // 文件重来——它的备份已经不在了，而目标现在是正确的原文内容，于是被
    // 误报成"目标已被修改（内容与补丁记录不一致）"，与磁盘真实状态完全相反。
    let mut remaining: Vec<AppliedFile> = record.files.clone();
    while !remaining.is_empty() {
        let file = remaining[0].clone();
        if let Err(error) = revert_one(data_dir, &kernel_root, &file, &mut warnings) {
            // 先把已完成的进度落盘，再报告失败。
            persist_revert_progress(data_dir, &mut state, id, &kernel_version, &remaining);
            return Err(error);
        }
        remaining.remove(0);
        persist_revert_progress(data_dir, &mut state, id, &kernel_version, &remaining);
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

/// 还原单个文件。备份存在时从备份还原；备份丢失时按内容哈希兜底。
fn revert_one(
    data_dir: &Path,
    kernel_root: &Path,
    file: &AppliedFile,
    warnings: &mut Vec<String>,
) -> Result<(), AppError> {
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
                    Ok(())
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // 备份丢失 → 走哈希校验兜底分支。
                    warnings.push(format!(
                        "{}：原文件备份已丢失（{}），尝试按内容校验兜底",
                        file.to,
                        backup.display()
                    ));
                    handle_missing_backup(
                        &target,
                        kernel_root,
                        file,
                        target_sha.as_deref(),
                        warnings,
                    )
                }
                Err(e) => Err(AppError::Patch(format!(
                    "无法读取备份 {}：{e}",
                    backup.display()
                ))),
            }
        }
        None => {
            // 无备份有两种来源，必须区分：
            // 1. 应用时目标不存在（纯新增文件）→ 校验后删除即还原；
            // 2. 应用时目标**已经**是补丁后的内容（P2-9：手工打过 / 上次
            //    记录丢失）→ 这个文件本来就是用户的，我们既没写过它、也没
            //    有它的原文件，删掉它就是破坏。交给 `handle_missing_backup`
            //    如实报告"没有可恢复的原文件"。
            if file.had_original {
                return handle_missing_backup(
                    &target,
                    kernel_root,
                    file,
                    target_sha.as_deref(),
                    warnings,
                );
            }
            match target_sha {
                Some(sha) if sha == file.patched_sha256 => {
                    fs::remove_file(&target).map_err(|e| {
                        AppError::Patch(format!("无法删除补丁文件 {}：{e}", target.display()))
                    })?;
                    prune_empty_dirs(&target, kernel_root);
                    Ok(())
                }
                Some(_) => Err(AppError::Patch(format!(
                    "补丁文件 {} 已被其他工具修改（内容与补丁不符），为安全起见不自动删除，请检查后手动处理",
                    target.display()
                ))),
                // 目标已不存在：本就是新增文件，视为已还原。
                None => Ok(()),
            }
        }
    }
}

/// 把撤销进度写回 `state.json`：记录里只保留尚未还原的文件。
fn persist_revert_progress(
    data_dir: &Path,
    state: &mut PatchState,
    id: &str,
    kernel_version: &str,
    remaining: &[AppliedFile],
) {
    if let Some(applied) = state
        .applied
        .iter_mut()
        .find(|a| a.id == id && a.kernel_version == kernel_version)
    {
        applied.files = remaining.to_vec();
    }
    // 写盘失败不改变"文件已经还原"这个事实：下一次撤销会靠 `original_sha256`
    // 兜底认出已还原的文件，因此这里只做 best-effort。
    let _ = write_state(data_dir, state);
}

/// 备份丢失时的兜底处理，返回后由调用方继续（或通过错误中止）。
fn handle_missing_backup(
    target: &Path,
    kernel_root: &Path,
    file: &AppliedFile,
    target_sha: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<(), AppError> {
    match target_sha {
        // 目标已经回到应用前的内容：说明这个文件在之前的一次撤销里已经还原
        // 过（备份随之被删除）。这是撤销可重入的关键判据——没有它，重入的
        // 第二次撤销会被最后那个分支误报成"目标已被修改"而永久卡住。
        Some(sha) if file.original_sha256.as_deref() == Some(sha) => {
            warnings.push(format!("{}：已是应用前的原文件内容，视为已还原", file.to));
            Ok(())
        }
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
                prune_empty_dirs(target, kernel_root);
                Ok(())
            }
        }
        Some(_) => Err(AppError::Patch(format!(
            "无法自动还原 {}：目标文件已被修改（内容与补丁记录不一致），请检查后手动处理",
            target.display()
        ))),
    }
}

/// 删除文件后清理空目录，**只在内核根以内、且不删内核根本身**。
///
/// 旧实现既不接收根、也没有任何终止条件：`while let Some(d) = d.parent()`
/// 会一路向上删空目录，包括 `<data_dir>/kernels`、`<data_dir>` 甚至更高层
/// ——注释写着"只删到内核根为止"，实现里却没有这个界限（P2-10）。
fn prune_empty_dirs(file_path: &Path, kernel_root: &Path) {
    let mut dir = file_path.parent();
    while let Some(d) = dir {
        if d == kernel_root || !d.starts_with(kernel_root) {
            break;
        }
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
    let mut rows: Vec<PatchRow> = patches
        .iter()
        .map(|(def, _)| row_for(data_dir, def, active.as_deref()))
        .collect();
    rows.extend(orphan_record_rows(data_dir, patches, active.as_deref()));
    PatchStatus {
        patches: rows,
        warning: state_integrity_warning(data_dir),
    }
}

/// 为"定义已不在当前清单里、但记录仍存在"的补丁补一行。
///
/// 壳升级后不再携带某个补丁（或清单损坏被跳过）时，旧实现只遍历当前定义，
/// 这些记录在设置页完全不可见 —— 而 `revert` 其实支持撤销它们，用户没有任何
/// 入口，只能手改 `state.json`（P2-14）。
fn orphan_record_rows(
    data_dir: &Path,
    patches: &[(PatchDef, PathBuf)],
    active: Option<&str>,
) -> Vec<PatchRow> {
    let Some(version) = active else {
        return Vec::new();
    };
    let state = read_state(data_dir);
    state
        .applied
        .iter()
        .filter(|record| record.kernel_version == version)
        .filter(|record| !patches.iter().any(|(def, _)| def.id == record.id))
        .map(|record| {
            let (state_code, problems) = disk_state(data_dir, record);
            let mut row = PatchRow {
                id: record.id.clone(),
                name: record.id.clone(),
                version: record
                    .patch_version
                    .clone()
                    .unwrap_or_else(|| "未知".to_string()),
                kind: String::new(),
                description: "该补丁的定义已不在当前版本的清单中，磁盘上仍保留应用记录".to_string(),
                min_kernel_version: None,
                max_kernel_version: None,
                superseded_since_kernel_version: None,
                superseded: false,
                state: String::new(),
                state_text: String::new(),
                note: None,
                applied_at: Some(record.applied_at.clone()),
                enabled: true,
            };
            if state_code == "applied" {
                row.state = "applied".into();
                row.state_text = "已应用（定义已移除）".into();
                row.note = Some("可以直接撤销；如需重新应用，请安装携带该补丁定义的壳版本".into());
            } else {
                row.state = "dirty".into();
                row.state_text = "文件已被改动（定义已移除）".into();
                row.note = Some(format!(
                    "{}（请先撤销该记录再手动处理文件）",
                    problems.join("；")
                ));
            }
            row
        })
        .collect()
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
        superseded_since_kernel_version: def.superseded_since_kernel_version.clone(),
        superseded: false,
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
    let superseded = is_superseded(def, version);
    row.superseded = superseded;
    match find_applied(&state, &def.id, version) {
        None => {
            row.state = "not_applied".into();
            if superseded {
                row.state_text = "已并入官方内核".into();
                row.note = note_from_elsewhere.or_else(|| {
                    def.superseded_since_kernel_version.as_deref().map(|bound| {
                        format!(
                            "官方内核 v{} 起已包含本补丁的修复；如需旧版本兼容，可点击「展开查看」",
                            bound
                        )
                    })
                });
                row.enabled = false;
            } else {
                row.state_text = "未应用".into();
                row.note = note_from_elsewhere;
                row.enabled = true;
            }
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
    pub(super) fn make_resource_root(root: &Path) -> PathBuf {
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

    pub(super) fn setup(data_dir: &Path, version: &str) {
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
    fn copy_over_existing_identical_records_no_recoverable_original() {
        // P2-9：目标已经是补丁后的内容时，旧实现把**补丁内容**当成"原文件"
        // 备份下来，于是"撤销"会把补丁内容原样写回、报告「已撤销」，而内核
        // 仍在跑补丁代码、记录已被删除 —— 状态与磁盘彻底脱节。
        let root = temp_root("idem");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();
        let target = kernel::kernel_dir(&data, "0.1.2")
            .join("node_modules/@deepseek-ai/dsh/lib/xlink-hello.js");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "module.exports = 42;\n").unwrap(); // 用户已有同名同内容文件

        let notes = apply(&data, &patches, "hello-copy").unwrap();
        assert!(
            notes.iter().any(|n| n.contains("没有可恢复的原文件")),
            "应用时就该说明这次没有可恢复的原文件：{notes:?}"
        );
        let state = read_state(&data);
        let record = find_applied(&state, "hello-copy", "0.1.2").unwrap();
        assert!(record.files[0].had_original, "文件在应用前确实存在");
        assert!(
            record.files[0].backup_rel.is_none(),
            "不得为已打过补丁的目标伪造备份"
        );
        assert!(record.files[0].original_sha256.is_none());
        assert!(
            !backups_root(&data).join("hello-copy").exists(),
            "磁盘上也不该留下这份无意义的备份"
        );

        // 撤销必须如实拒绝：没有可恢复的原文件，不能假装"已撤销"。
        let error = revert(&data, &patches, "hello-copy").unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("原文件备份已丢失") || text.contains("无法自动还原"),
            "应明确说明无原文件可恢复：{text}"
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "module.exports = 42;\n",
            "内容不应被改写"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rollback_keeps_preexisting_file_when_a_later_file_fails() {
        // P0-1：执行阶段失败时按本次备份整体回滚。`already_patched` 的文件既没有
        // 备份、也不在本次的写入集合里（`had_original = true`）——旧实现把它和
        // "本次新建的文件"混为一谈，回滚时直接删掉，等于拿回滚当删除用。
        let root = temp_root("rollback-keep");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = root.join("res");
        let dir = res.join("probe");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files").join("a.js"), "PATCHED\n").unwrap();
        fs::write(dir.join("files").join("b.js"), "B\n").unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "probe",
                    "name": "回滚探针",
                    "version": "1.0.0",
                    "kind": "patch",
                    "description": "test",
                    "files": [
                        {"mode": "copy", "from": "files/a.js", "to": "node_modules/x/a.js", "required": true},
                        {"mode": "copy", "from": "files/b.js", "to": "blocker/b.js", "required": true}
                    ]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let patches = load_patches(&res).unwrap();
        let kernel_root = kernel::kernel_dir(&data, "0.1.2");
        let first = kernel_root.join("node_modules/x/a.js");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        // 应用前它就是补丁内容：本次不会写它，回滚也不许删它。
        fs::write(&first, "PATCHED\n").unwrap();
        // 让第二个文件在执行阶段失败：父路径是一个普通文件。
        fs::write(kernel_root.join("blocker"), "not a dir\n").unwrap();

        let error = apply(&data, &patches, "probe").unwrap_err().to_string();
        assert!(
            error.contains("回滚保留原样"),
            "回滚说明必须点出保留的文件：{error}"
        );
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "PATCHED\n",
            "应用前就已存在的文件不得被回滚删除"
        );
        assert!(
            read_state(&data).applied.is_empty(),
            "失败的应用不得留下记录"
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

    /// 写一个独立的 superseded 资源根：单个 copy 补丁带 `supersededSinceKernelVersion`。
    fn make_superseded_resource_root(root: &Path, bound: &str) -> PathBuf {
        let res = root.join("resources").join("patches");
        let dir = res.join("obsolete-copy");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files").join("x.js"), "patched\n").unwrap();
        let original = "original\n";
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "obsolete-copy",
                    "name": "已被官方取代的补丁",
                    "version": "1.0.0",
                    "kind": "patch",
                    "description": "已被官方内核采纳",
                    "supersededSinceKernelVersion": bound,
                    "files": [{
                        "mode": "copy",
                        "from": "files/x.js",
                        "to": "node_modules/@deepseek-ai/dsh/lib/obsolete.js",
                        "expectSha256": sha256_bytes(original.as_bytes()),
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let _ = original; // 留作显式期望：expect_sha256 真实指向原文件
        res
    }

    #[test]
    fn superseded_status_reflects_bound_and_disables_apply() {
        let root = temp_root("superseded");
        let data = root.join("data");
        setup(&data, "0.1.2-alpha.2");
        let res = make_superseded_resource_root(&root, "0.1.2-alpha.2");
        let patches = load_patches(&res).unwrap();

        // 当前内核版本正好等于 supersededSinceKernelVersion → 视为已过时。
        let row = status(&data, &patches)
            .patches
            .iter()
            .find(|r| r.id == "obsolete-copy")
            .unwrap()
            .clone();
        assert!(row.superseded);
        assert_eq!(row.state, "not_applied");
        assert_eq!(row.state_text, "已并入官方内核");
        assert!(!row.enabled);
        assert!(row.note.unwrap().contains("0.1.2-alpha.2"));
        assert_eq!(
            row.superseded_since_kernel_version.as_deref(),
            Some("0.1.2-alpha.2")
        );

        // 仍可在 UI 上撤销（此处根本没有应用记录，revert 应报「未应用」）
        // 应用必须被拒绝。
        let err = apply(&data, &patches, "obsolete-copy").unwrap_err();
        assert!(err.to_string().contains("已被官方内核"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn superseded_below_bound_does_not_supersede() {
        let root = temp_root("superseded-below");
        let data = root.join("data");
        setup(&data, "0.1.1-rc.2");
        let res = make_superseded_resource_root(&root, "0.1.2-alpha.2");
        let patches = load_patches(&res).unwrap();

        let row = status(&data, &patches)
            .patches
            .iter()
            .find(|r| r.id == "obsolete-copy")
            .unwrap()
            .clone();
        assert!(!row.superseded);
        assert_eq!(row.state, "not_applied");
        assert!(row.enabled);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn superseded_record_remains_revertible() {
        // 即便当前内核版本已 superseded，曾经在旧版本上应用的记录依然可撤销：
        // 模拟「在旧 rc.2 内核上应用 → 切到 alpha.2 看到 superseded 状态 →
        // 切回 rc.2 仍能正常撤销」的清理流程。
        let root = temp_root("superseded-revert");
        let data = root.join("data");
        setup(&data, "0.1.1-rc.2");
        let res = make_superseded_resource_root(&root, "0.1.2-alpha.2");
        let patches = load_patches(&res).unwrap();

        let target = kernel::kernel_dir(&data, "0.1.1-rc.2")
            .join("node_modules/@deepseek-ai/dsh/lib/obsolete.js");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "original\n").unwrap();

        apply(&data, &patches, "obsolete-copy").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "patched\n");

        // 切到 alpha.2：当前激活版本无应用记录 → row 展示为「已并入官方内核」、
        // 应用被 Rust 端拒绝。state.json 里 rc.2 上的应用记录依然保留。
        kernel::write_active(&data, Some("0.1.2-alpha.2")).unwrap();
        let row = status(&data, &patches)
            .patches
            .iter()
            .find(|r| r.id == "obsolete-copy")
            .unwrap()
            .clone();
        assert!(row.superseded);
        assert_eq!(row.state, "not_applied");
        assert!(!row.enabled);
        assert!(apply(&data, &patches, "obsolete-copy").is_err());
        assert!(read_state(&data)
            .applied
            .iter()
            .any(|a| a.id == "obsolete-copy" && a.kernel_version == "0.1.1-rc.2"));

        // 切回 rc.2：旧应用记录仍可撤销。
        kernel::write_active(&data, Some("0.1.1-rc.2")).unwrap();
        revert(&data, &patches, "obsolete-copy").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "original\n");
        fs::remove_dir_all(&root).unwrap();
    }

    /// PatchRow 必须以 camelCase 输出关键字段（`#[serde(rename_all = "camelCase")]`），
    /// 否则前端 `row.stateText` / `row.supersededSinceKernelVersion` 之类全部是
    /// undefined，会导致「已并入官方内核」卡片折叠逻辑与 v-if/v-else 切换全部失效。
    /// 回归测试：每次改 struct 字段名时必须主动 review 这一行。
    #[test]
    fn patch_row_serde_keys_are_camel_case() {
        let row = PatchRow {
            id: "x".into(),
            name: "n".into(),
            version: "1.0.0".into(),
            kind: "patch".into(),
            description: "".into(),
            min_kernel_version: Some("0.1.1-rc.2".into()),
            // 必须 Some 才能让 skip_serializing_if 不跳过、JSON 里有键可验。
            max_kernel_version: Some("0.1.2-alpha.1".into()),
            superseded_since_kernel_version: Some("0.1.2-alpha.2".into()),
            superseded: true,
            state: "not_applied".into(),
            state_text: "已并入官方内核".into(),
            note: None,
            applied_at: None,
            enabled: false,
        };
        let json = serde_json::to_string(&row).expect("PatchRow should serialize");
        // 必须用驼峰命名，否则 UI 端 v-if / {{ row.stateText }} 等都拿不到值，
        // 「已并入官方内核」卡片折叠逻辑会沉默失效。
        for key in [
            "\"minKernelVersion\"",
            "\"maxKernelVersion\"",
            "\"supersededSinceKernelVersion\"",
            "\"superseded\"",
            "\"stateText\"",
            "\"state\"",
            "\"kind\"",
        ] {
            assert!(
                json.contains(key),
                "PatchRow JSON missing camelCase key {key}; full={json}"
            );
        }
        // 反例：绝不能输出 snake_case，否则前端 `row.state_text` 等访问全部是 undefined。
        for bad in [
            "min_kernel_version\"",
            "max_kernel_version\"",
            "superseded_since_kernel_version\"",
            "state_text\"",
        ] {
            assert!(
                !json.contains(bad),
                "PatchRow JSON unexpectedly contains snake_case key {bad}; rename_all lost? full={json}"
            );
        }
    }

    /// 构造一个「两个文件都覆盖既有文件」的补丁（真实场景：把补丁打在 npm
    /// dist 的既有文件上，用 expectSha256 声明预期原文件哈希）。
    fn make_two_file_patch(root: &Path) -> PathBuf {
        let res = root.join("resources").join("patches");
        let dir = res.join("two-files");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files/a.js"), "patched A\n").unwrap();
        fs::write(dir.join("files/b.js"), "patched B\n").unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "two-files",
                    "name": "两文件补丁",
                    "version": "1.0.0",
                    "kind": "patch",
                    "description": "test",
                    "files": [
                        {
                            "mode": "copy",
                            "from": "files/a.js",
                            "to": "a.js",
                            "expectSha256": sha256_bytes(b"original A\n"),
                            "required": true
                        },
                        {
                            "mode": "copy",
                            "from": "files/b.js",
                            "to": "b.js",
                            "expectSha256": sha256_bytes(b"original B\n"),
                            "required": true
                        }
                    ]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        res
    }

    /// 多文件补丁在靠后的文件校验失败时，不得把靠前的文件写进内核。
    ///
    /// 修复前是「边校验边写」：第一个文件写入成功、第二个文件因 expectSha256
    /// 不匹配而失败，于是 state.json 从不落盘（没有记录可撤销），而内核里已经
    /// 躺着一个半补丁——重试又被残留备份挡住，应用内没有任何恢复路径。
    #[test]
    fn failed_second_file_leaves_no_partial_patch() {
        let root = temp_root("partial");
        let data = root.join("data");
        let res = make_two_file_patch(&root);
        setup(&data, "0.1.2");
        let kernel_root = kernel::kernel_dir(&data, "0.1.2");
        fs::write(kernel_root.join("a.js"), "original A\n").unwrap();
        // b.js 的内容与补丁声明的原文件不符 → 第二个文件必然校验失败。
        fs::write(kernel_root.join("b.js"), "someone else's B\n").unwrap();

        let patches = load_patches(&res).unwrap();
        let error = apply(&data, &patches, "two-files").expect_err("第二个文件必须让整次应用失败");
        assert!(
            error.to_string().contains("内容与补丁预期的原文件不符"),
            "错误信息应说明原文件不匹配，实际：{error}"
        );

        assert_eq!(
            fs::read_to_string(kernel_root.join("a.js")).unwrap(),
            "original A\n",
            "校验失败时第一个文件不得被写入（不允许半补丁）"
        );
        assert!(
            read_state(&data).applied.is_empty(),
            "失败的应用不得留下记录"
        );
        assert!(
            !backups_root(&data).join("two-files").exists(),
            "失败的应用不得留下备份（否则会挡住下一次重试）"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// 撤销中途失败后再次点「撤销」，必须从剩下的文件继续。
    ///
    /// 修复前是「全部成功才写 state」：第一个文件还原成功后备份即被删除，
    /// 第二个文件失败时记录仍然完整，于是再次撤销会从第一个文件重来——它的
    /// 备份已经不在，目标又是正确的原文内容，于是被误报成「目标已被修改」，
    /// 与磁盘真实状态完全相反，撤销永久卡死。
    #[test]
    fn revert_resumes_after_a_partial_failure() {
        let root = temp_root("revert-resume");
        let data = root.join("data");
        let res = make_two_file_patch(&root);
        setup(&data, "0.1.2");
        let kernel_root = kernel::kernel_dir(&data, "0.1.2");
        fs::write(kernel_root.join("a.js"), "original A\n").unwrap();
        fs::write(kernel_root.join("b.js"), "original B\n").unwrap();

        let patches = load_patches(&res).unwrap();
        apply(&data, &patches, "two-files").expect("apply succeeds");
        assert_eq!(
            fs::read_to_string(kernel_root.join("b.js")).unwrap(),
            "patched B\n"
        );

        // 让 b.js 的备份消失、内容漂移：撤销会在第二个文件上失败。
        fs::remove_file(backup_path(&data, "two-files", "0.1.2", "b.js")).unwrap();
        fs::write(kernel_root.join("b.js"), "hand edited\n").unwrap();

        let error = revert(&data, &patches, "two-files").expect_err("第二个文件应导致撤销失败");
        assert!(
            error.to_string().contains("已被修改"),
            "应报告目标被改写，实际：{error}"
        );

        // 第一个文件已经还原，且进度已落盘：记录里只剩第二个文件。
        assert_eq!(
            fs::read_to_string(kernel_root.join("a.js")).unwrap(),
            "original A\n"
        );
        let state = read_state(&data);
        let record = state
            .applied
            .iter()
            .find(|a| a.id == "two-files")
            .expect("失败后记录必须保留，让用户能继续撤销");
        assert_eq!(record.files.len(), 1, "已还原的文件必须从记录里移除");
        assert_eq!(record.files[0].to, "b.js");

        // 用户按提示把文件恢复成原文之后再次撤销：必须能完成，而不是卡在
        // 「目标已被修改」上（备份已丢失，靠原始哈希认出"已还原"）。
        fs::write(kernel_root.join("b.js"), "original B\n").unwrap();
        revert(&data, &patches, "two-files").expect("第二次撤销必须能从剩下的文件继续");
        assert!(
            !read_state(&data)
                .applied
                .iter()
                .any(|a| a.id == "two-files"),
            "撤销完成后记录必须被清除"
        );
        assert_eq!(
            fs::read_to_string(kernel_root.join("b.js")).unwrap(),
            "original B\n"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// 同一个文件不能被两个补丁同时持有。
    ///
    /// 没有这道闸时：撤销 A 会用它自己的备份直接覆盖，把 B 的改动静默丢掉；
    /// 再撤销 B 又写回 B 的备份，最终内核带着 A 的改动运行，而 state.json 里
    /// 已经没有任何记录——既不显示 dirty，也无法再撤销。
    #[test]
    fn a_second_patch_on_the_same_file_is_rejected() {
        let root = temp_root("file-ownership");
        let data = root.join("data");
        let res = root.join("resources").join("patches");
        // 两个都改 package.json 的 replace 补丁，search 串互不重叠。
        let cases = [
            (
                "first-touch",
                "\"private\":true",
                "\"private\":true,\"first\":true",
            ),
            (
                "second-touch",
                "\"version\":\"1.0.0\"",
                "\"version\":\"1.0.1\"",
            ),
        ];
        for (id, search, replacement) in cases {
            let dir = res.join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("manifest.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "schemaVersion": 1,
                    "patches": [{
                        "id": id,
                        "name": id,
                        "version": "1.0.0",
                        "kind": "patch",
                        "description": "test",
                        "files": [{
                            "mode": "replace",
                            "search": search,
                            "replacement": replacement,
                            "to": "package.json",
                            "required": true
                        }]
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
        }

        setup(&data, "0.1.2");
        let patches = load_patches(&res).unwrap();
        apply(&data, &patches, "first-touch").expect("第一个补丁应当应用成功");

        let error =
            apply(&data, &patches, "second-touch").expect_err("同一文件上的第二个补丁必须被拒绝");
        assert!(
            error.to_string().contains("已被补丁 first-touch 应用过"),
            "错误信息应指明占用者，实际：{error}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// 补丁记录损坏时，apply / revert 必须拒绝执行，而不是当成"没有任何应用
    /// 记录"——那会让磁盘上打过补丁的文件既不可见（没有 dirty）也不可撤销。
    #[test]
    fn corrupt_patch_state_blocks_apply_and_revert() {
        let root = temp_root("corrupt-state");
        let data = root.join("data");
        let res = make_two_file_patch(&root);
        setup(&data, "0.1.2");
        let kernel_root = kernel::kernel_dir(&data, "0.1.2");
        fs::write(kernel_root.join("a.js"), "original A\n").unwrap();
        fs::write(kernel_root.join("b.js"), "original B\n").unwrap();

        fs::create_dir_all(state_dir(&data)).unwrap();
        let damaged = "{ not json";
        fs::write(state_file(&data), damaged).unwrap();

        let patches = load_patches(&res).unwrap();
        let error = apply(&data, &patches, "two-files").expect_err("记录损坏时必须拒绝写入");
        assert!(
            error.to_string().contains("损坏"),
            "错误信息应说明记录损坏，实际：{error}"
        );
        assert_eq!(
            fs::read_to_string(state_file(&data)).unwrap(),
            damaged,
            "损坏的原文件不得被覆盖"
        );
        assert_eq!(
            fs::read_to_string(kernel_root.join("a.js")).unwrap(),
            "original A\n",
            "记录损坏时不得改动内核文件"
        );
        // 展示路径仍能渲染，但必须带上警告。
        let snapshot = status(&data, &patches);
        assert!(
            snapshot.warning.is_some(),
            "损坏的记录必须在状态快照里暴露为警告"
        );

        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod hardening_tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-patch-hardening-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prune_empty_dirs_stops_at_the_kernel_root() {
        // P2-10：旧实现没有终止条件，会把空目录一路删到 <data_dir> 甚至更高。
        let root = temp_root("prune");
        let kernel_root = root.join("kernels").join("0.1.5");
        let deep = kernel_root.join("node_modules").join("pkg").join("lib");
        fs::create_dir_all(&deep).unwrap();
        let target = deep.join("index.js");
        fs::write(&target, b"x").unwrap();
        fs::remove_file(&target).unwrap();

        prune_empty_dirs(&target, &kernel_root);

        assert!(!deep.exists(), "内核根以内的空目录应被清掉");
        assert!(
            kernel_root.exists(),
            "内核根本身必须保留（旧实现会把它删掉）"
        );
        assert!(root.join("kernels").exists(), "内核根的父目录不能被删");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn prune_empty_dirs_never_walks_outside_the_kernel_root() {
        // 传进来的路径若不在根以内（例如调用方弄错了根），一个目录都不该动。
        let root = temp_root("prune-outside");
        let kernel_root = root.join("kernels").join("0.1.5");
        let outside = root.join("logs");
        fs::create_dir_all(&kernel_root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("a.log");
        fs::write(&file, b"x").unwrap();
        fs::remove_file(&file).unwrap();

        prune_empty_dirs(&file, &kernel_root);

        assert!(outside.exists(), "根以外的目录不允许被删除");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn manifest_with_traversing_from_is_rejected() {
        // P2-11：`from` 完全没有越界校验，`../../../../etc/passwd` 可读到补丁
        // 目录之外；绝对路径更会直接丢弃 patch_dir。
        for evil in ["../../../../etc/passwd", "/etc/passwd"] {
            let root = temp_root("from");
            let res = root.join("resources").join("patches");
            let dir = res.join("evil");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("manifest.json"),
                serde_json::to_string(&serde_json::json!({
                    "schemaVersion": 1,
                    "patches": [{
                        "id": "evil",
                        "name": "越界补丁",
                        "version": "1.0.0",
                        "kind": "plugin",
                        "description": "test",
                        "files": [{
                            "mode": "copy",
                            "from": evil,
                            "to": "node_modules/@deepseek-ai/dsh/lib/x.js",
                            "required": true
                        }]
                    }]
                }))
                .unwrap(),
            )
            .unwrap();

            let loaded = load_patches(&res).expect("扫描本身不该失败");
            assert!(
                loaded.iter().all(|(def, _)| def.id != "evil"),
                "from={evil} 的清单必须被拒绝，实际加载了 {}",
                loaded.len()
            );
            fs::remove_dir_all(&root).ok();
        }
    }

    #[test]
    fn legitimate_relative_from_still_loads() {
        // 收紧之后正常形态不能被误伤。
        let root = temp_root("from-ok");
        let res = root.join("resources").join("patches");
        let dir = res.join("ok");
        fs::create_dir_all(dir.join("files")).unwrap();
        fs::write(dir.join("files").join("x.js"), b"module.exports = 1;\n").unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string(&serde_json::json!({
                "schemaVersion": 1,
                "patches": [{
                    "id": "ok",
                    "name": "正常补丁",
                    "version": "1.0.0",
                    "kind": "plugin",
                    "description": "test",
                    "files": [{
                        "mode": "copy",
                        "from": "files/x.js",
                        "to": "node_modules/@deepseek-ai/dsh/lib/x.js",
                        "required": true
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let loaded = load_patches(&res).expect("扫描本身不该失败");
        assert_eq!(loaded.len(), 1, "正常的相对 from 必须能加载");
        assert_eq!(loaded[0].0.id, "ok");
        fs::remove_dir_all(&root).ok();
    }
}

#[cfg(all(test, unix))]
mod link_and_orphan_tests {
    use super::tests::{make_resource_root, setup};
    use super::*;
    use std::os::unix::fs::symlink;

    fn temp_root(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-patch-links-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pnpm_isolated_linker_layout_is_writable() {
        // P2-15：pnpm 的 isolated linker 把 node_modules/<pkg> 做成指向
        // node_modules/.pnpm/<pkg>@<ver>/node_modules/<pkg> 的**符号链接**。
        // 旧实现见链接即拒 → 在这种布局下补丁永远无法应用。
        let root = temp_root("pnpm");
        let kernel_root = root.join("kernels").join("0.1.5");
        let real_pkg = kernel_root
            .join("node_modules")
            .join(".pnpm")
            .join("pkg@1.0.0")
            .join("node_modules")
            .join("pkg");
        fs::create_dir_all(&real_pkg).unwrap();
        let linked = kernel_root.join("node_modules").join("pkg");
        symlink(&real_pkg, &linked).unwrap();
        let target = linked.join("lib").join("index.js");

        ensure_no_symlink_ancestors(&target, &kernel_root)
            .expect("指向内核内部真实目录的链接必须被允许");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn links_escaping_the_kernel_root_are_still_rejected() {
        // 收紧不能放过真正的逃逸：node_modules/<pkg> -> /tmp/outside。
        let root = temp_root("escape");
        let kernel_root = root.join("kernels").join("0.1.5");
        let outside = root.join("outside");
        fs::create_dir_all(&kernel_root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let linked = kernel_root.join("node_modules").join("pkg");
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        symlink(&outside, &linked).unwrap();

        let error = ensure_no_symlink_ancestors(&linked.join("index.js"), &kernel_root)
            .expect_err("指到内核之外的链接必须被拒绝");
        let text = error.to_string();
        assert!(text.contains("内核目录之外"), "错误要说明原因：{text}");
        assert!(text.contains("重新安装"), "错误要给出下一步：{text}");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn removed_definition_with_live_record_is_still_listed() {
        // P2-14：定义已不在清单里、记录仍在 → 必须在 status 里出现且可撤销。
        let root = temp_root("orphan");
        let data = root.join("data");
        setup(&data, "0.1.2");
        let res = make_resource_root(&root);
        let patches = load_patches(&res).unwrap();
        apply(&data, &patches, "hello-copy").unwrap();

        // 模拟"壳升级后不再携带这个补丁"：清单为空。
        let view = status(&data, &[]);
        assert_eq!(view.patches.len(), 1, "孤儿记录必须出现在列表里");
        let row = &view.patches[0];
        assert_eq!(row.id, "hello-copy");
        assert_eq!(row.state, "applied");
        assert!(row.enabled, "必须允许撤销");
        assert!(
            row.state_text.contains("定义已移除"),
            "状态文案要说明定义已移除：{}",
            row.state_text
        );

        // 而且真的能撤销。
        revert(&data, &patches, "hello-copy").expect("孤儿记录也必须可撤销");
        assert!(read_state(&data).applied.is_empty());
        fs::remove_dir_all(&root).ok();
    }
}
