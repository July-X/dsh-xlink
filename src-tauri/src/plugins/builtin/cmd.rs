//! `builtin_openai_status`：内嵌 openai-oauth 插件的只读状态命令。
//!
//! 照 `precheck_cmd` 的先例独立成文件（`commands.rs` 在反棘轮上，只许
//! 下调）。本命令**只读**：不打变更前快照、不写任何文件——按诊断侧的
//! 约定，读类命令打「变更前」是假的。写动作（启用/停用开关）是后续
//! 提交，届时走 `run_plugin_mutation_command` 同款纪律（实例停止判据 +
//! 变更前快照）。
//!
//! 解析链全部走生产路径：`default_instance_key` → 实例记录 → 适配器的
//! `dsh_home_for` / `resolve_install_dir`，不 hard-code 实例 id。

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::blocking;
use crate::kernel::kernel_adapter;
use crate::plugins::builtin;
use crate::plugins::center::default_instance_key;
use crate::shell::instance;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializedEntry {
    pub plugin_version: String,
    pub fingerprint: String,
    pub dir: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinOpenaiStatus {
    /// 实例 patch 里是否有本插件的接线行（不等于「本次已加载」）。
    pub wired: bool,
    pub kernel_fingerprint: Option<String>,
    pub materialized: Vec<MaterializedEntry>,
    /// 应用侧插件资源是否可解析（release 资源目录 / dev 仓库源码）。
    pub plugin_source_available: bool,
    /// 面向 UI 的一句话说明（如「实例尚未指定内核版本」）。
    pub note: String,
}

/// 插件资源目录：优先随应用打包的资源（`builtin-plugins/openai-oauth`，
/// P1 资源管线落地后存在），dev 下回退仓库源码（`plugins/openai-oauth`）。
/// 都没有时返回 `None`——状态如实报不可用，不猜路径。
fn resolve_plugin_source(app: &AppHandle) -> Option<std::path::PathBuf> {
    if let Ok(resource) = app.path().resource_dir() {
        let packaged = resource.join("builtin-plugins").join("openai-oauth");
        if packaged.join("package.json").is_file() {
            return Some(packaged);
        }
    }
    if let Ok(from_env) = std::env::var("DSH_XLINK_OPENAI_PLUGIN_SRC") {
        let candidate = std::path::PathBuf::from(from_env);
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
    }
    // `tauri dev` 的 cwd 是 src-tauri/，仓库根的插件源码在 ../../plugins/。
    for base in ["../../plugins/openai-oauth", "plugins/openai-oauth"] {
        let candidate = std::path::PathBuf::from(base);
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 内嵌 openai-oauth 插件当前状态（只读）。
#[tauri::command]
pub async fn builtin_openai_status(app: AppHandle) -> Result<BuiltinOpenaiStatus, String> {
    blocking(move || compute_status(&app)).await
}

fn compute_status(app: &AppHandle) -> Result<BuiltinOpenaiStatus, String> {
    let (family, id) = default_instance_key();
    let Some(record) = instance::load_record_from_disk(&family, &id) else {
        return Ok(BuiltinOpenaiStatus {
            wired: false,
            kernel_fingerprint: None,
            materialized: Vec::new(),
            plugin_source_available: resolve_plugin_source(app).is_some(),
            note: format!("实例 {family}/{id} 不存在；请先创建实例"),
        });
    };
    let dsh_home = kernel_adapter::DshAdapter::dsh_home_for(&record);
    let kernel_root = record.kernel_version.as_deref().and_then(|version| {
        kernel_adapter::lookup(&family).and_then(|adapter| adapter.resolve_install_dir(version))
    });
    let note = if kernel_root.is_none() {
        String::from("实例尚未指定内核版本；请在「更新」页安装并选择内核")
    } else {
        String::new()
    };
    let status = builtin::probe_status(&dsh_home, &record.profile, kernel_root.as_deref());
    Ok(BuiltinOpenaiStatus {
        wired: status.wired,
        kernel_fingerprint: status.kernel_fingerprint,
        materialized: status
            .materialized
            .into_iter()
            .map(|entry| MaterializedEntry {
                plugin_version: entry.plugin_version,
                fingerprint: entry.fingerprint,
                dir: entry.dir.to_string_lossy().into_owned(),
            })
            .collect(),
        plugin_source_available: resolve_plugin_source(app).is_some(),
        note,
    })
}
