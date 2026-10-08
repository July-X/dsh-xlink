//! 内嵌 openai-oauth 插件的 Tauri 命令层：只读状态 + 启停开关。
//!
//! 照 `precheck_cmd` 的先例独立成文件（`commands.rs` 在反棘轮上，只许
//! 下调）。`builtin_openai_status` **只读**：不打变更前快照、不写任何
//! 文件——按诊断侧的约定，读类命令打「变更前」是假的。
//!
//! `builtin_openai_set_enabled` 是写动作，走开发计划 §4.1 的事务顺序：
//! 实例判据（dsh 族 / 存在 / 未被另一壳占用 / **内核已停止**——实例级
//! 判据，不是「本壳工作台」）→ 变更前快照（打点失败不阻断）→ 启停事务
//! → 意图落盘。停用只摘自有接线行，不删资源与账号（设计 §8）。
//!
//! 解析链全部走生产路径：`default_instance_key` → 实例记录 → 适配器的
//! `dsh_home_for` / `resolve_install_dir`，不 hard-code 实例 id。

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::blocking;
use crate::diagnostics::snapshot;
use crate::kernel::kernel_adapter;
use crate::kernel::lifecycle;
use crate::plugins::builtin;
use crate::plugins::center::default_instance_key;
use crate::shell::instance;
use crate::shell::settings;

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
    /// 用户保存的意图（state.json，按壳模式 / profile 分键）。
    pub requested_enabled: bool,
    /// `disabled` / `prepared` / `incompatible`（P1 子集，见 builtin::Status）。
    pub load_state: String,
    pub kernel_fingerprint: Option<String>,
    pub materialized: Vec<MaterializedEntry>,
    /// 应用侧插件资源是否可解析（release 资源目录 / dev 仓库源码）。
    pub plugin_source_available: bool,
    /// 面向 UI 的一句话说明（如「实例尚未指定内核版本」）。
    pub note: String,
    /// 状态文件损坏时的说明；有它时开关应禁用并提示检查。
    pub state_error: Option<String>,
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

fn status_payload(app: &AppHandle, status: builtin::Status, note: String) -> BuiltinOpenaiStatus {
    BuiltinOpenaiStatus {
        wired: status.wired,
        requested_enabled: status.requested_enabled,
        load_state: status.load_state.to_string(),
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
        state_error: status.state_error,
    }
}

/// 解析默认实例与内核根；`Err` 文案可直接给 UI。
fn resolve_target() -> Result<
    (
        crate::shell::instance::InstanceRecord,
        Option<std::path::PathBuf>,
    ),
    String,
> {
    let (family, id) = default_instance_key();
    if family != crate::shell::instance::KERNEL_FAMILY_DSH {
        return Err(format!(
            "内嵌 OpenAI 插件只在 dsh 内核族上提供（当前默认实例是 {family}）；请先切换到 dsh 实例"
        ));
    }
    let Some(record) = instance::load_record_from_disk(&family, &id) else {
        return Err(format!("实例 {family}/{id} 不存在；请先创建实例"));
    };
    let kernel_root = record.kernel_version.as_deref().and_then(|version| {
        kernel_adapter::lookup(&family).and_then(|adapter| adapter.resolve_install_dir(version))
    });
    Ok((record, kernel_root))
}

fn probe_for(app: &AppHandle) -> Result<BuiltinOpenaiStatus, String> {
    let (record, kernel_root) = resolve_target()?;
    let dsh_home = kernel_adapter::DshAdapter::dsh_home_for(&record);
    let mode = settings::current_mode().as_str();
    let status = builtin::probe_status(&dsh_home, &record.profile, kernel_root.as_deref(), mode);
    let note = if kernel_root.is_none() {
        String::from("实例尚未指定内核版本；请在「更新」页安装并选择内核")
    } else {
        String::new()
    };
    Ok(status_payload(app, status, note))
}

/// 内嵌 openai-oauth 插件当前状态（只读）。
#[tauri::command]
pub async fn builtin_openai_status(app: AppHandle) -> Result<BuiltinOpenaiStatus, String> {
    blocking(move || probe_for(&app)).await
}

/// 设置内嵌 openai-oauth 插件的启用意图（写动作）。
///
/// 守卫顺序（开发计划 §4.1）：dsh 族 → 实例存在 → 未被另一壳占用 →
/// **内核已停止**（实例级判据，用户自建实例可能正被另一个壳跑着）→
/// 变更前快照（失败只落日志，不阻断）→ 启停事务 → 意图落盘。
/// 事务失败时意图**不落盘**——开关状态与接线保持一致，失败原因原样返回。
#[tauri::command]
pub async fn builtin_openai_set_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<BuiltinOpenaiStatus, String> {
    blocking(move || {
        let (record, kernel_root) = resolve_target()?;
        let (family, id) = (record.kernel_family.clone(), record.id.clone());
        instance::ensure_instance_mutable(&family, &id, "修改内嵌 OpenAI 插件开关")?;
        if let Some(running) = instance::instance_kernel_running(&family, &id) {
            return Err(instance::instance_kernel_running_message(
                &running,
                &id,
                "修改内嵌 OpenAI 插件开关",
            ));
        }
        let dsh_home = kernel_adapter::DshAdapter::dsh_home_for(&record);
        let mode = settings::current_mode().as_str();

        // 变更前快照：打点绝不阻断用户操作（诊断侧纪律）。
        let data_dir = lifecycle::data_dir(&family);
        if let Err(error) = snapshot::record(
            &data_dir,
            &family,
            &id,
            &record.profile,
            record.port,
            snapshot::reason::PRE_CHANGE,
        ) {
            eprintln!("内置插件开关的变更前快照失败（不阻断）：{error}");
        }

        if enabled {
            let source = resolve_plugin_source(&app).ok_or_else(|| {
                String::from(
                    "找不到内嵌插件资源（builtin-plugins/openai-oauth）；请重新安装 dsh-xlink",
                )
            })?;
            let kernel_root = kernel_root.ok_or_else(|| {
                String::from("实例尚未指定内核版本；请在「更新」页安装并选择内核后再启用")
            })?;
            let wired = builtin::ensure_wired(&source, &dsh_home, &record.profile, &kernel_root)?;
            let mut entries = builtin::state::load(&dsh_home)?;
            entries.insert(
                builtin::state::state_key(mode, &record.profile),
                builtin::state::StateEntry {
                    requested_enabled: true,
                    updated_at_ms: crate::shell::process::epoch_millis(),
                    plugin_version: Some(wired.plugin_version),
                    fingerprint: Some(wired.fingerprint),
                },
            );
            builtin::state::save(&dsh_home, &entries)?;
        } else {
            builtin::ensure_unwired(&dsh_home, &record.profile)?;
            let mut entries = builtin::state::load(&dsh_home)?;
            if let Some(entry) = entries.get_mut(&builtin::state::state_key(mode, &record.profile))
            {
                entry.requested_enabled = false;
                entry.updated_at_ms = crate::shell::process::epoch_millis();
                builtin::state::save(&dsh_home, &entries)?;
            }
        }
        probe_for(&app)
    })
    .await
}
