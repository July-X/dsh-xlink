//! 通用记忆模块：授权绑定当前两侧实例与 profile，不搬迁、不复制权威记录。
//!
//! 旧 use_approved 和逐条授权不代表双向全库许可；只有本文件的显式授权才启用。
use super::{
    query, share,
    store::{self, MemoryDoc, MemoryRecord, Namespace},
};
use crate::shell::{error::AppError, instance, paths, process, settings};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
struct Consent {
    version: u32,
    enabled: bool,
    pair_key: String,
}

#[derive(Serialize)]
pub(crate) struct Source {
    label: String,
    namespace: String,
}

#[derive(Serialize)]
pub(crate) struct Status {
    enabled: bool,
    pair_key: String,
    sources: Vec<Source>,
    warning: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Row {
    origin_label: String,
    source_namespace: String,
    record: MemoryRecord,
}

fn file() -> std::path::PathBuf {
    paths::shell_dir(settings::current_mode()).join("memory-sharing/module.json")
}

fn fail(reason: impl std::fmt::Display) -> AppError {
    AppError::Memory(format!(
        "通用记忆模块不可用：{reason}；请刷新记忆页面并检查 {}，原始记忆不会被修改。",
        file().display()
    ))
}

fn consent() -> Result<Consent, AppError> {
    match process::read_state_file::<Consent>(&file()) {
        process::StateRead::Missing => Ok(Consent {
            version: 1,
            ..Default::default()
        }),
        process::StateRead::Loaded(value) if value.version == 1 => Ok(value),
        process::StateRead::Loaded(_) => Err(fail("授权版本不受支持")),
        process::StateRead::Corrupt { reason } => Err(fail(reason)),
    }
}

fn pair() -> Result<[Namespace; 2], AppError> {
    let dsh = Namespace::current();
    let id = crate::kernel::mcode_service::configured_instance_id()
        .map_err(fail)?
        .ok_or_else(|| fail("请先配置 MiniMax 工作台"))?;
    let mcode = Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, &id);
    if dsh.family != instance::KERNEL_FAMILY_DSH {
        return Err(fail("默认实例不是 DSH"));
    }
    let registry = instance::load_registry().map_err(fail)?;
    for ns in [&dsh, &mcode] {
        share::validate_coordinate(&ns.instance_id)?;
        share::validate_coordinate(&ns.profile)?;
        if !registry
            .get(&ns.instance_id)
            .is_some_and(|entry| entry.kernel_family == ns.family)
        {
            return Err(fail("实例注册信息已改变"));
        }
    }
    Ok([dsh, mcode])
}

fn key(pair: &[Namespace; 2]) -> String {
    format!("{}|{}", pair[0].key(), pair[1].key())
}

fn label(ns: &Namespace) -> &'static str {
    if ns.family == instance::KERNEL_FAMILY_DSH {
        "DSH"
    } else {
        "MiniMax"
    }
}

fn checked_doc(ns: &Namespace) -> Result<MemoryDoc, AppError> {
    let doc = store::load(ns)?;
    if doc.meta.schema_version > store::SCHEMA_VERSION {
        return Err(fail(format!("{} 记忆由更新版本写入", label(ns))));
    }
    Ok(doc)
}

pub(crate) fn status() -> Status {
    let result = (|| {
        let pair = pair()?;
        let consent = consent()?;
        Ok::<_, AppError>(Status {
            enabled: consent.enabled && consent.pair_key == key(&pair),
            pair_key: key(&pair),
            sources: pair
                .iter()
                .map(|ns| Source {
                    label: label(ns).into(),
                    namespace: ns.key(),
                })
                .collect(),
            warning: if consent.enabled && consent.pair_key != key(&pair) {
                Some("实例或 profile 已改变，请重新授权当前记忆模块".into())
            } else {
                None
            },
        })
    })();
    result.unwrap_or_else(|error| Status {
        enabled: false,
        pair_key: String::new(),
        sources: vec![],
        warning: Some(error.to_string()),
    })
}

pub(crate) fn set_enabled(enabled: bool, expected_pair: &str) -> Result<Status, AppError> {
    let _lock = share::lock_grants()?;
    // 撤销不要求原实例仍存在，坏授权也可由显式关闭恢复；不修改旧逐条许可。
    let pair_key = if enabled {
        let pair = pair()?;
        if key(&pair) != expected_pair {
            return Err(fail("实例或 profile 已改变，请重新确认"));
        }
        let _previous = consent()?;
        for ns in &pair {
            checked_doc(ns)?;
        }
        key(&pair)
    } else {
        String::new()
    };
    let bytes = serde_json::to_vec_pretty(&Consent {
        version: 1,
        enabled,
        pair_key,
    })
    .map_err(fail)?;
    crate::shell::atomic_write::atomic_write_private(&file(), &bytes).map_err(fail)?;
    Ok(status())
}

pub(crate) fn enabled_for(target: &Namespace) -> Result<bool, AppError> {
    let consent = consent()?;
    if !consent.enabled {
        return Ok(false);
    }
    let pair = pair()?;
    Ok(consent.pair_key == key(&pair) && pair.contains(target))
}

pub(crate) fn list() -> Result<Vec<Row>, AppError> {
    let _lock = share::lock_grants()?;
    let pair = pair()?;
    let mut rows = Vec::new();
    for ns in &pair {
        rows.extend(checked_doc(ns)?.memories.into_iter().map(|record| Row {
            origin_label: label(ns).into(),
            source_namespace: ns.key(),
            record,
        }));
    }
    Ok(rows)
}

/// 两边桥的统一入口。未授权返回 None，由调用者保持原先的窄授权路径。
/// 调用者须持有共享锁直到响应写出，撤销不能与发送交错。
pub(crate) fn retrieve(
    target: &Namespace,
    text: &str,
    workspace_id: &str,
    workspace_path: &str,
) -> Result<Option<query::RetrievedContext>, AppError> {
    if !enabled_for(target)? {
        return Ok(None);
    }
    let pair = pair()?;
    let workspace = super::workspaces::normalize_path(workspace_path);
    // 既有本侧许可下的交接/经验照旧；模块本身只新增普通记忆授权。
    let own = checked_doc(target)?;
    let mut combined = if own.meta.use_approved {
        own
    } else {
        MemoryDoc::default()
    };
    combined.memories.clear();
    combined.meta.generation = 0;
    for ns in &pair {
        let doc = checked_doc(ns)?;
        // 两库代次都影响插件缓存；不改变或写回原库的开关与计数。
        combined.meta.generation = combined.meta.generation.wrapping_add(doc.meta.generation);
        for mut record in doc.memories {
            if record.scope != store::SCOPE_GENERAL && record.scope != store::SCOPE_PROJECT {
                continue;
            }
            let source_workspace_id = if ns == target { workspace_id } else { "" };
            if !query::scope_matches(&record, source_workspace_id, &workspace) {
                continue;
            }
            // 先按来源判断范围，再去掉内核局部 ID，避免跨内核 ID 碰撞。
            if ns != target {
                record.workspace_id.clear();
            }
            record.id = format!("{}/{}", ns.key(), record.id);
            record.title = format!("【来源：{}】{}", label(ns), record.title);
            combined.memories.push(record);
        }
    }
    // 模块/旧路径使用不同代次区间，避免另一侧空库时切换许可被插件视为“没有变化”。
    // 保持在 JavaScript 安全整数范围内，不用 u64 高位。
    combined.meta.generation = (combined.meta.generation & ((1u64 << 52) - 1)) | (1u64 << 52);
    Ok(Some(query::render_document(
        &combined,
        text,
        workspace_id,
        &workspace,
        true,
        false,
    )))
}

#[tauri::command]
pub async fn memory_module_status() -> Result<Status, String> {
    crate::commands::blocking(|| Ok::<_, String>(status())).await
}

#[tauri::command]
pub async fn memory_module_set_enabled(
    enabled: bool,
    expected_pair: String,
) -> Result<Status, String> {
    crate::commands::blocking(move || set_enabled(enabled, &expected_pair)).await
}

#[tauri::command]
pub async fn memory_module_list() -> Result<Vec<Row>, String> {
    crate::commands::blocking(list).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::testkit::{general_input, TempHome};

    fn fixture(tag: &str) -> (TempHome, [Namespace; 2]) {
        let home = TempHome::new(tag);
        let dsh = Namespace::current();
        let mcode = Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, "module-mini");
        let mut registry = instance::InstanceRegistry::default();
        for (ns, port) in [(&dsh, 31001), (&mcode, 31002)] {
            let entry = instance::InstanceRecord::new(&ns.instance_id, &ns.family, port, 1);
            instance::save_record_to_disk(&entry).unwrap();
            registry.add(entry).unwrap();
        }
        instance::save_registry(&registry).unwrap();
        let path = paths::shell_dir(settings::current_mode()).join("mcode.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{"instance_id":"module-mini","port":31002,"miniapp_port":31003}"#,
        )
        .unwrap();
        (home, [dsh, mcode])
    }

    fn seed(ns: &Namespace, text: &str) -> MemoryRecord {
        let mut input = general_input(text, text);
        input.pinned = true;
        store::save(ns, input).unwrap()
    }

    #[test]
    fn explicit_module_consent_is_bidirectional_and_preserves_legacy_bytes() {
        let (_home, pair) = fixture("module-bidirectional");
        let first = seed(&pair[0], "来自 DSH 的偏好");
        seed(&pair[1], "来自 MiniMax 的偏好");
        // 合成两库相同 ID，注入身份仍必须按来源区分。
        let mut peer = checked_doc(&pair[1]).unwrap();
        peer.memories[0].id = first.id;
        std::fs::write(pair[1].file(), serde_json::to_vec(&peer).unwrap()).unwrap();
        store::set_use_approved(&pair[0], true).unwrap();
        let before: Vec<_> = pair
            .iter()
            .map(|ns| std::fs::read(ns.file()).unwrap())
            .collect();
        assert!(
            retrieve(&pair[0], "", "", "").unwrap().is_none(),
            "旧单侧许可不能升级为共享"
        );
        assert!(set_enabled(true, "过期的配对").is_err());
        set_enabled(true, &key(&pair)).unwrap();
        for target in &pair {
            let context = retrieve(target, "", "", "").unwrap().unwrap();
            assert!(context.enabled);
            assert!(context.generation >= 1u64 << 52);
            assert_eq!(context.hits.len(), 2);
            assert_ne!(context.hits[0].id, context.hits[1].id);
            assert!(context.text.contains("来源：DSH"));
            assert!(context.text.contains("来源：MiniMax"));
            assert!(!context.text.contains("memory_read"));
        }
        assert_eq!(list().unwrap().len(), 2);
        set_enabled(false, "").unwrap();
        assert!(retrieve(&pair[1], "", "", "").unwrap().is_none());
        for (ns, bytes) in pair.iter().zip(before) {
            assert_eq!(std::fs::read(ns.file()).unwrap(), bytes);
        }
    }

    #[test]
    fn project_scope_disabled_records_and_namespace_changes_fail_closed() {
        let (_home, pair) = fixture("module-scope");
        let workspace = std::env::temp_dir()
            .join("module-project-a")
            .to_string_lossy()
            .into_owned();
        let mut input = general_input("项目约束", "项目私有正文");
        input.scope = store::SCOPE_PROJECT.into();
        input.workspace_path = workspace.clone();
        input.pinned = true;
        let project = store::save(&pair[0], input).unwrap();
        let disabled = seed(&pair[1], "不应注入");
        store::set_state(&pair[1], &disabled.id, store::STATE_DISABLED).unwrap();
        set_enabled(true, &key(&pair)).unwrap();
        assert!(retrieve(&pair[1], "", "", "")
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        assert!(retrieve(&pair[1], "", "", "/other-project")
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        assert_eq!(
            retrieve(&pair[1], "", "", &workspace)
                .unwrap()
                .unwrap()
                .hits
                .len(),
            1
        );
        store::delete(&pair[0], &project.id).unwrap();
        assert!(retrieve(&pair[1], "", "", &workspace)
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        let mut wrong_profile = pair[1].clone();
        wrong_profile.profile = "other".into();
        assert!(!enabled_for(&wrong_profile).unwrap());
        let mut wrong_instance = pair[1].clone();
        wrong_instance.instance_id = "another-instance".into();
        assert!(!enabled_for(&wrong_instance).unwrap());
        std::fs::write(
            file(),
            r#"{"version":2,"enabled":true,"pair_key":"future"}"#,
        )
        .unwrap();
        assert!(retrieve(&pair[1], "", "", "").is_err());
        set_enabled(false, "").unwrap();
        assert!(!status().enabled);
    }

    #[test]
    fn corrupt_source_does_not_get_overwritten_or_silently_shared() {
        let (_home, pair) = fixture("module-corrupt-source");
        seed(&pair[0], "偏好");
        seed(&pair[1], "偏好");
        set_enabled(true, &key(&pair)).unwrap();
        std::fs::write(pair[1].file(), "broken").unwrap();
        assert!(retrieve(&pair[0], "", "", "").is_err());
        assert!(list().is_err());
        set_enabled(false, "").unwrap();
        assert_eq!(std::fs::read_to_string(pair[1].file()).unwrap(), "broken");
    }
}
