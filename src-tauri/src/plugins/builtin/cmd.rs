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
    // 内核版本以 `active.txt` 为准（与 `resolve_instance_record`、插件栈的
    // `read_active` 调用方同一权威源）：「内核版本」页安装 / 切换只写
    // active.txt，实例记录要等下次启动工作台才被同步——只认记录会让
    // 「刚装好内核、还没启动过工作台」的实例在这里误报「尚未指定内核版本」。
    // active.txt 缺席时退回记录里上次同步的值。
    let kernel_root = resolve_kernel_root(&family, record.kernel_version.as_deref());
    Ok((record, kernel_root))
}

/// 内核根目录解析：`active.txt` 为准，实例记录里的版本只作回退。
/// 独立成函数是为了让测试不必经过 `default_instance_key` 与注册表。
fn resolve_kernel_root(family: &str, record_version: Option<&str>) -> Option<std::path::PathBuf> {
    let version = lifecycle::read_active(&lifecycle::data_dir(family))
        .or_else(|| record_version.map(String::from));
    version.and_then(|version| {
        kernel_adapter::lookup(family).and_then(|adapter| adapter.resolve_install_dir(&version))
    })
}

fn probe_for(app: &AppHandle) -> Result<BuiltinOpenaiStatus, String> {
    let (record, kernel_root) = resolve_target()?;
    let dsh_home = kernel_adapter::DshAdapter::dsh_home_for(&record);
    let mode = settings::current_mode().as_str();
    let status = builtin::probe_status(&dsh_home, &record.profile, kernel_root.as_deref(), mode);
    let note = if kernel_root.is_none() {
        String::from("实例尚未指定内核版本；请在「内核版本」页安装并选择内核")
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
                String::from("实例尚未指定内核版本；请在「内核版本」页安装并选择内核后再启用")
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

#[cfg(test)]
mod tests {
    #![allow(unused_variables)]

    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// 持住 `DSH_XLINK_HOME` 的临时 home；drop 时清掉自己那块目录。
    /// （AGENTS.md：按 `paths::*` 解析路径的测试必须持住 guard。）
    struct TempHome {
        root: PathBuf,
        _guard: crate::tests::EnvGuard,
    }

    impl TempHome {
        fn new(tag: &str) -> Self {
            let nano = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!(
                "dsh-builtin-cmd-test-{tag}-{}-{nano}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create test home");
            let guard = crate::tests::scoped_xlink_home(&root);
            TempHome {
                root,
                _guard: guard,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 在新布局种一棵可被 `resolve_install_dir` 认可的内核树
    /// （判据是 bin.js 存在，不需要真内核内容）。
    fn seed_installed_kernel(family: &str, version: &str) {
        let bin = crate::shell::paths::kernel_version_dir(family, version)
            .join(kernel_adapter::DshAdapter::KERNEL_BIN_REL);
        fs::create_dir_all(bin.parent().unwrap()).expect("seed kernel tree");
        fs::write(&bin, b"// test stub").expect("write bin stub");
    }

    /// 2026-10-09 用户实测：内核版本页装完内核（`active.txt` 已写），
    /// 但实例记录还没被启动工作台同步过（`kernel_version` 仍是 None），
    /// 此时启用内嵌 OpenAI 插件被误拒「实例尚未指定内核版本」。
    /// 内核版本的权威源是 `active.txt`（与 `resolve_instance_record` 一致），
    /// 记录只作回退。
    #[test]
    fn kernel_root_follows_active_txt_when_record_is_silent() {
        let home = TempHome::new("active-wins");
        let family = crate::shell::instance::KERNEL_FAMILY_DSH;
        let version = "0.2.1-alpha.1";
        seed_installed_kernel(family, version);
        lifecycle::write_active(&lifecycle::data_dir(family), Some(version))
            .expect("write active.txt");

        let root =
            resolve_kernel_root(family, None).expect("active.txt 指向已装版本时必须解析出内核根");
        assert_eq!(
            root,
            crate::shell::paths::kernel_version_dir(family, version)
        );
    }

    /// `active.txt` 缺席（该壳从未装过内核）时退回实例记录里上次同步的
    /// 版本，而不是一刀切报「尚未指定内核版本」。
    #[test]
    fn kernel_root_falls_back_to_record_version() {
        let home = TempHome::new("record-fallback");
        let family = crate::shell::instance::KERNEL_FAMILY_DSH;
        let version = "0.1.5-rc.1";
        seed_installed_kernel(family, version);

        let root =
            resolve_kernel_root(family, Some(version)).expect("记录里的版本已安装时必须回退成功");
        assert_eq!(
            root,
            crate::shell::paths::kernel_version_dir(family, version)
        );
    }

    /// 两处都没有内核版本时如实返回 `None`——状态命令靠它出
    /// 「实例尚未指定内核版本」的提示，不能猜路径。
    #[test]
    fn kernel_root_is_none_without_active_or_record() {
        let home = TempHome::new("none");
        let family = crate::shell::instance::KERNEL_FAMILY_DSH;
        assert!(resolve_kernel_root(family, None).is_none());
    }
}
