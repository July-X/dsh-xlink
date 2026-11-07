//! MiniMax 实例的记忆管理；内核接线与运行态证明由工作台生命周期负责。

use crate::commands::blocking;
use crate::memory::store::{
    self, ListFilter, MemoryListView, MemoryRecord, MemoryStatus, Namespace, SaveInput,
};
use crate::shell::instance::KERNEL_FAMILY_MCODE;

fn namespace(instance_id: &str, profile: &str) -> Result<Namespace, String> {
    crate::memory::share::validate_coordinate(instance_id).map_err(|error| error.to_string())?;
    let id = crate::kernel::mcode_service::configured_instance_id()?
        .ok_or("尚未配置 MiniMax 实例，请先在 MiniMax 工作台设置安装位置")?;
    if id != instance_id {
        return Err("MiniMax 实例已改变，请刷新页面后重试".into());
    }
    let current = Namespace::for_bridge(KERNEL_FAMILY_MCODE, &id);
    if current.profile != profile {
        return Err("MiniMax 记忆空间已改变，请刷新页面后重试".into());
    }
    Ok(current)
}

fn status(ns: &Namespace) -> MemoryStatus {
    let mut result = store::status(ns);
    // This signal is currently process-global and belongs to the DSH plugin.
    result.effect_pending = false;
    result
}

#[tauri::command]
pub async fn mcode_memory_status(
    instance_id: String,
    profile: String,
) -> Result<MemoryStatus, String> {
    blocking(move || Ok::<_, String>(status(&namespace(&instance_id, &profile)?))).await
}

#[tauri::command]
pub async fn mcode_memory_list(
    instance_id: String,
    profile: String,
    filter: Option<ListFilter>,
) -> Result<MemoryListView, String> {
    blocking(move || {
        let mut result = store::list(
            &namespace(&instance_id, &profile)?,
            &filter.unwrap_or_default(),
        );
        result.status.effect_pending = false;
        Ok::<_, String>(result)
    })
    .await
}

#[tauri::command]
pub async fn mcode_memory_save(
    instance_id: String,
    profile: String,
    mut input: SaveInput,
) -> Result<MemoryRecord, String> {
    blocking(move || {
        if !input.kind.is_empty() && input.kind != "memory" {
            return Err("MiniMax 暂只支持管理普通记忆".into());
        }
        if !input.workspace_id.trim().is_empty() {
            return Err("MiniMax 记忆请使用项目路径，不支持其他工作台的项目 ID".into());
        }
        if !input.id.trim().is_empty() && input.expected_revision.is_none() {
            return Err("编辑 MiniMax 记忆必须提供记录版本，请刷新列表后重试".into());
        }
        input.source = None;
        store::save(&namespace(&instance_id, &profile)?, input).map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn mcode_memory_set_enabled(
    instance_id: String,
    profile: String,
    use_approved: bool,
) -> Result<MemoryStatus, String> {
    blocking(move || {
        let ns = namespace(&instance_id, &profile)?;
        let _grant_lock = crate::memory::share::lock_grants().map_err(|error| error.to_string())?;
        store::set_use_approved(&ns, use_approved).map_err(|error| error.to_string())?;
        Ok::<_, String>(status(&ns))
    })
    .await
}

#[tauri::command]
pub async fn mcode_memory_set_state(
    instance_id: String,
    profile: String,
    id: String,
    state: String,
) -> Result<MemoryRecord, String> {
    blocking(move || {
        if state != store::STATE_ACCEPTED && state != store::STATE_DISABLED {
            return Err("MiniMax 暂只支持启用或停用记忆".into());
        }
        store::set_state(&namespace(&instance_id, &profile)?, &id, &state)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn mcode_memory_delete(
    instance_id: String,
    profile: String,
    id: String,
) -> Result<(), String> {
    blocking(move || {
        store::delete(&namespace(&instance_id, &profile)?, &id).map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_mcode_memory_shares(
    instance_id: String,
    profile: String,
) -> Result<Vec<crate::memory::share::GrantView>, String> {
    blocking(move || {
        crate::memory::share::list(&instance_id, &profile).map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_all_memory_shares() -> Result<Vec<crate::memory::share::GrantSummary>, String> {
    blocking(|| crate::memory::share::list_all().map_err(|error| error.to_string())).await
}

#[tauri::command]
pub async fn revoke_memory_share(
    target_instance: String,
    target_profile: String,
    source_instance: String,
    source_profile: String,
    record_id: String,
) -> Result<(), String> {
    blocking(move || {
        crate::memory::share::revoke_any(
            &target_instance,
            &target_profile,
            &source_instance,
            &source_profile,
            &record_id,
        )
        .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn set_mcode_memory_share(
    instance_id: String,
    profile: String,
    source_instance: String,
    source_profile: String,
    record_id: String,
    revision: u64,
    shared: bool,
) -> Result<(), String> {
    blocking(move || {
        crate::memory::share::set(
            &instance_id,
            &profile,
            &source_instance,
            &source_profile,
            &record_id,
            revision,
            shared,
        )
        .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn revoke_mcode_memory_share(
    instance_id: String,
    profile: String,
    source_instance: String,
    source_profile: String,
    record_id: String,
) -> Result<(), String> {
    blocking(move || {
        crate::memory::share::revoke(
            &instance_id,
            &profile,
            &source_instance,
            &source_profile,
            &record_id,
        )
        .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::testkit::{general_input, TempHome};
    use crate::shell::{instance, paths, settings};
    use std::fs;

    fn configure(id: &str, port: u16) {
        let path = paths::shell_dir(settings::current_mode()).join("mcode.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "instance_id": id,
                "configured_package_path": null,
                "configured_node_path": null,
                "port": port,
                "miniapp_port": port + 1,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn commands_resolve_the_configured_mcode_instance_and_preserve_dsh_data() {
        let _home = TempHome::new("mcode-command-isolation");
        let mut registry = instance::InstanceRegistry::default();
        for (id, family, port) in [
            ("mcode-a", KERNEL_FAMILY_MCODE, 30001),
            ("mcode-b", KERNEL_FAMILY_MCODE, 30003),
            ("dsh-other", instance::KERNEL_FAMILY_DSH, 30005),
        ] {
            let record = instance::InstanceRecord::new(id, family, port, 1);
            instance::save_record_to_disk(&record).unwrap();
            registry.add(record).unwrap();
        }
        instance::save_registry(&registry).unwrap();
        let dsh = Namespace::current();
        let dsh_record = store::save(&dsh, general_input("DSH secret", "DSH data")).unwrap();

        let profile = Namespace::for_bridge(KERNEL_FAMILY_MCODE, "mcode-a").profile;
        tauri::async_runtime::block_on(async {
            assert!(
                mcode_memory_status("mcode-a".into(), profile.clone())
                    .await
                    .is_err(),
                "reading must not create an instance"
            );
            configure("mcode-a", 30001);
            let status = mcode_memory_status("mcode-a".into(), profile.clone())
                .await
                .unwrap();
            assert!(
                mcode_memory_status("mcode-a".into(), "stale-profile".into())
                    .await
                    .is_err()
            );
            assert!(status.namespace.starts_with("mcode/mcode-a/"));
            assert!(
                !status.use_approved,
                "a new namespace must default to disabled"
            );
            let first = mcode_memory_save(
                "mcode-a".into(),
                profile.clone(),
                general_input("A memory", "A data"),
            )
            .await
            .unwrap();
            assert_eq!(first.sources[0].kind, "user-manual");
            let mut edit = general_input("A edited", "A edited data");
            edit.id = first.id.clone();
            assert!(
                mcode_memory_save("mcode-a".into(), profile.clone(), edit.clone())
                    .await
                    .is_err()
            );
            edit.expected_revision = Some(first.revision);
            let edited = mcode_memory_save("mcode-a".into(), profile.clone(), edit.clone())
                .await
                .unwrap();
            assert_eq!(edited.revision, first.revision + 1);
            assert_eq!(edited.body, "A edited data");
            assert_eq!(edited.sources[0].kind, "user-manual");
            assert!(
                mcode_memory_save("mcode-a".into(), profile.clone(), edit)
                    .await
                    .is_err(),
                "stale edits must be rejected"
            );
            assert!(
                mcode_memory_set_enabled("mcode-a".into(), profile.clone(), true)
                    .await
                    .unwrap()
                    .use_approved
            );

            configure("mcode-b", 30003);
            let second_status = mcode_memory_status("mcode-b".into(), profile.clone())
                .await
                .unwrap();
            assert!(second_status.namespace.starts_with("mcode/mcode-b/"));
            assert!(!second_status.use_approved);
            assert!(mcode_memory_list("mcode-b".into(), profile.clone(), None)
                .await
                .unwrap()
                .memories
                .is_empty());
            assert!(
                mcode_memory_delete("mcode-a".into(), profile.clone(), first.id.clone())
                    .await
                    .is_err()
            );
            assert!(
                mcode_memory_delete("mcode-b".into(), profile.clone(), first.id.clone())
                    .await
                    .is_err()
            );
            let mut invalid = general_input("B memory", "B data");
            invalid.workspace_id = "dsh-workspace".into();
            assert!(
                mcode_memory_save("mcode-b".into(), profile.clone(), invalid)
                    .await
                    .is_err()
            );
            let second = mcode_memory_save(
                "mcode-b".into(),
                profile.clone(),
                general_input("B memory", "B data"),
            )
            .await
            .unwrap();
            assert_eq!(
                mcode_memory_set_state(
                    "mcode-b".into(),
                    profile.clone(),
                    second.id.clone(),
                    "disabled".into()
                )
                .await
                .unwrap()
                .state,
                "disabled"
            );
            assert!(mcode_memory_set_state(
                "mcode-b".into(),
                profile.clone(),
                second.id.clone(),
                "archived".into()
            )
            .await
            .is_err());
            mcode_memory_delete("mcode-b".into(), profile.clone(), second.id)
                .await
                .unwrap();
            assert!(mcode_memory_list("mcode-b".into(), profile.clone(), None)
                .await
                .unwrap()
                .memories
                .is_empty());

            configure("dsh-other", 30005);
            assert!(
                mcode_memory_list("dsh-other".into(), profile.clone(), None)
                    .await
                    .is_err(),
                "a DSH id cannot select its data"
            );
            configure("mcode-a", 30001);
            assert_eq!(
                mcode_memory_list("mcode-a".into(), profile.clone(), None)
                    .await
                    .unwrap()
                    .memories[0]
                    .id,
                first.id
            );
            assert!(
                mcode_memory_status("mcode-a".into(), profile.clone())
                    .await
                    .unwrap()
                    .use_approved
            );
            assert!(
                !mcode_memory_set_enabled("mcode-a".into(), profile.clone(), false)
                    .await
                    .unwrap()
                    .use_approved
            );
        });
        let dsh_list = store::list(&dsh, &ListFilter::default());
        assert_eq!(dsh_list.memories.len(), 1);
        assert_eq!(dsh_list.memories[0].id, dsh_record.id);
        assert!(!store::status(&dsh).use_approved);
    }
}
