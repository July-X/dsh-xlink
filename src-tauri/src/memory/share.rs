//! Explicit, per-record grants; the source record remains authoritative.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    marker::PhantomData,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::store::{self, MemoryRecord, Namespace};
use crate::shell::{error::AppError, instance, paths, process, settings};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Grant {
    source_instance: String,
    source_profile: String,
    target_instance: String,
    target_profile: String,
    record_id: String,
    scope: String,
    workspace_path: String,
}

#[derive(Default, Deserialize, Serialize)]
struct Grants {
    version: u32,
    grants: Vec<Grant>,
}

#[derive(Serialize)]
pub(crate) struct SharedMemory {
    pub source_instance: String,
    pub source_profile: String,
    pub record: MemoryRecord,
    pub scope_valid: bool,
}

#[derive(Serialize)]
pub(crate) struct GrantView {
    pub source_instance: String,
    pub source_profile: String,
    pub record_id: String,
    pub record: Option<MemoryRecord>,
    pub scope_valid: bool,
}

#[derive(Serialize)]
pub(crate) struct GrantSummary {
    pub source_instance: String,
    pub source_profile: String,
    pub target_instance: String,
    pub target_profile: String,
    pub record_id: String,
    pub scope: String,
    pub workspace_path: String,
}

fn directory() -> PathBuf {
    paths::shell_dir(settings::current_mode()).join("memory-sharing")
}

pub(crate) fn validate_coordinate(value: &str) -> Result<(), AppError> {
    paths::validate_id_component(value).map_err(|error| AppError::Memory(error.into()))?;
    if value.contains(':') || value.ends_with('.') || value.ends_with(' ') {
        return Err(AppError::Memory("记忆共享标识包含不安全的路径字符".into()));
    }
    Ok(())
}

thread_local! {
    static ACTIVE_GRANT_LOCK: RefCell<Option<(PathBuf, usize, File)>> = const { RefCell::new(None) };
}

/// Same-thread nesting shares the cross-process lock; the guard cannot cross threads.
pub(crate) struct GrantLock {
    _not_send: PhantomData<Rc<()>>,
}

impl Drop for GrantLock {
    fn drop(&mut self) {
        ACTIVE_GRANT_LOCK.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some((_, depth, _)) = slot.as_mut() {
                *depth -= 1;
                if *depth == 0 {
                    *slot = None;
                }
            }
        });
    }
}

pub(crate) fn lock_grants() -> Result<GrantLock, AppError> {
    // Instances can be addressed by either shell, while grants remain per-shell.
    let dir = paths::kernels_root();
    let nested = ACTIVE_GRANT_LOCK.with(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_mut() {
            Some((held, depth, _)) if *held == dir => {
                *depth += 1;
                Ok(true)
            }
            Some(_) => Err(AppError::Memory("记忆共享锁目录在操作期间发生变化".into())),
            None => Ok(false),
        }
    })?;
    if nested {
        return Ok(GrantLock {
            _not_send: PhantomData,
        });
    }
    std::fs::create_dir_all(&dir).map_err(|error| AppError::Memory(error.to_string()))?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(dir.join(".memory-sharing.lock"))
        .map_err(|error| AppError::Memory(format!("无法打开记忆共享锁：{error}")))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                return Err(AppError::Memory(format!(
                    "无法锁定记忆共享授权，请稍后重试：{error}"
                )))
            }
        }
    }
    ACTIVE_GRANT_LOCK.with(|slot| *slot.borrow_mut() = Some((dir, 1, lock)));
    Ok(GrantLock {
        _not_send: PhantomData,
    })
}

fn load() -> Result<Grants, AppError> {
    let path = directory().join("grants.json");
    let grants: Grants = match process::read_state_file(&path) {
        process::StateRead::Missing => {
            return Ok(Grants {
                version: 1,
                grants: vec![],
            })
        }
        process::StateRead::Loaded(value) => value,
        process::StateRead::Corrupt { reason } => {
            return Err(AppError::Memory(format!("记忆共享授权损坏：{reason}")))
        }
    };
    if grants.version != 1 || grants.grants.len() > 4096 {
        return Err(AppError::Memory("记忆共享授权版本或数量无效".into()));
    }
    let mut unique = BTreeSet::new();
    for grant in &grants.grants {
        if !unique.insert((
            &grant.source_instance,
            &grant.source_profile,
            &grant.target_instance,
            &grant.target_profile,
            &grant.record_id,
        )) {
            return Err(AppError::Memory("记忆共享授权存在重复项".into()));
        }
        for value in [
            &grant.source_instance,
            &grant.source_profile,
            &grant.target_instance,
            &grant.target_profile,
            &grant.record_id,
        ] {
            validate_coordinate(value)?;
        }
        if grant.scope != store::SCOPE_GENERAL && grant.scope != store::SCOPE_PROJECT {
            return Err(AppError::Memory("记忆共享授权范围无效".into()));
        }
        if grant.scope == store::SCOPE_PROJECT && grant.workspace_path.is_empty() {
            return Err(AppError::Memory("项目记忆共享授权缺少路径".into()));
        }
    }
    Ok(grants)
}

fn target(instance_id: &str, profile: &str) -> Result<Namespace, AppError> {
    validate_coordinate(instance_id)?;
    validate_coordinate(profile)?;
    let configured =
        crate::kernel::mcode_service::configured_instance_id().map_err(AppError::Memory)?;
    if configured.as_deref() != Some(instance_id) {
        return Err(AppError::Memory(
            "MiniMax 实例已改变，请刷新页面后重试".into(),
        ));
    }
    let namespace = Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, instance_id);
    if namespace.profile != profile {
        return Err(AppError::Memory(
            "MiniMax 记忆空间已改变，请刷新页面后重试".into(),
        ));
    }
    Ok(namespace)
}

pub(crate) fn set(
    instance_id: &str,
    profile: &str,
    source_instance: &str,
    source_profile: &str,
    record_id: &str,
    revision: u64,
    shared: bool,
) -> Result<(), AppError> {
    let target = target(instance_id, profile)?;
    let source = Namespace::current();
    if source.family != instance::KERNEL_FAMILY_DSH {
        return Err(AppError::Memory("只能授权当前 DSH 实例的记忆".into()));
    }
    if source.instance_id != source_instance || source.profile != source_profile {
        return Err(AppError::Memory("DSH 实例已改变，请刷新页面后重试".into()));
    }
    for value in [
        &source.instance_id,
        &source.profile,
        &target.instance_id,
        &target.profile,
    ] {
        validate_coordinate(value)?;
    }
    validate_coordinate(record_id)?;
    let mut grant = Grant {
        source_instance: source.instance_id.clone(),
        source_profile: source.profile.clone(),
        target_instance: target.instance_id,
        target_profile: target.profile,
        record_id: record_id.into(),
        scope: String::new(),
        workspace_path: String::new(),
    };
    let dir = directory();
    let _lock = lock_grants()?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| AppError::Memory(format!("无法准备记忆共享目录：{error}")))?;
    let mut document = load()?;
    let _source_lock = if shared {
        Some(crate::memory::lock::WriteLock::new(&source.dir())?)
    } else {
        None
    };
    if shared {
        let record = store::find_by_id(&source, record_id, false)?
            .ok_or_else(|| AppError::Memory("记忆不存在或已停用，请刷新后重试".into()))?;
        if record.revision != revision {
            return Err(AppError::Memory(
                "记忆已变更，请刷新并确认内容后重试".into(),
            ));
        }
        if record.scope == store::SCOPE_PROJECT && record.workspace_path.is_empty() {
            return Err(AppError::Memory(
                "项目记忆缺少项目路径，无法安全共享".into(),
            ));
        }
        grant.scope = record.scope;
        grant.workspace_path = record.workspace_path;
        document.grants.retain(|item| !same_identity(item, &grant));
        if document.grants.len() >= 4096 {
            return Err(AppError::Memory("记忆共享授权已达上限".into()));
        }
        document.grants.push(grant);
    } else {
        document.grants.retain(|item| !same_identity(item, &grant));
    }
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| AppError::Memory(error.to_string()))?;
    crate::shell::atomic_write::atomic_write_private(&dir.join("grants.json"), &bytes)
        .map_err(|error| AppError::Memory(format!("无法保存记忆共享授权：{error}")))
}

fn same_identity(left: &Grant, right: &Grant) -> bool {
    left.source_instance == right.source_instance
        && left.source_profile == right.source_profile
        && left.target_instance == right.target_instance
        && left.target_profile == right.target_profile
        && left.record_id == right.record_id
}

fn scope_allowed(grant: &Grant, record: &MemoryRecord) -> bool {
    match grant.scope.as_str() {
        store::SCOPE_GENERAL => {
            record.scope == store::SCOPE_GENERAL || record.scope == store::SCOPE_PROJECT
        }
        store::SCOPE_PROJECT => {
            record.scope == store::SCOPE_PROJECT && record.workspace_path == grant.workspace_path
        }
        _ => false,
    }
}

pub(crate) fn list(instance_id: &str, profile: &str) -> Result<Vec<GrantView>, AppError> {
    let target = target(instance_id, profile)?;
    let _lock = lock_grants()?;
    let grants = load()?;
    // A missing or damaged source must not make its authorization impossible to revoke.
    let records = authorized(&target, true).unwrap_or_default();
    Ok(grants
        .grants
        .into_iter()
        .filter(|grant| {
            grant.target_instance == target.instance_id && grant.target_profile == target.profile
        })
        .map(|grant| {
            let record = records.iter().find(|item| {
                item.source_instance == grant.source_instance
                    && item.source_profile == grant.source_profile
                    && item.record.id == grant.record_id
            });
            GrantView {
                source_instance: grant.source_instance,
                source_profile: grant.source_profile,
                record_id: grant.record_id,
                record: record.map(|item| item.record.clone()),
                scope_valid: record.is_some_and(|item| item.scope_valid),
            }
        })
        .collect())
}

pub(crate) fn list_all() -> Result<Vec<GrantSummary>, AppError> {
    let _lock = lock_grants()?;
    Ok(load()?
        .grants
        .into_iter()
        .map(|grant| GrantSummary {
            source_instance: grant.source_instance,
            source_profile: grant.source_profile,
            target_instance: grant.target_instance,
            target_profile: grant.target_profile,
            record_id: grant.record_id,
            scope: grant.scope,
            workspace_path: grant.workspace_path,
        })
        .collect())
}

pub(crate) fn revoke_any(
    target_instance: &str,
    target_profile: &str,
    source_instance: &str,
    source_profile: &str,
    record_id: &str,
) -> Result<(), AppError> {
    for value in [
        target_instance,
        target_profile,
        source_instance,
        source_profile,
        record_id,
    ] {
        validate_coordinate(value)?;
    }
    let dir = directory();
    let _lock = lock_grants()?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| AppError::Memory(format!("无法准备记忆共享目录：{error}")))?;
    let mut document = load()?;
    document.grants.retain(|grant| {
        !(grant.target_instance == target_instance
            && grant.target_profile == target_profile
            && grant.source_instance == source_instance
            && grant.source_profile == source_profile
            && grant.record_id == record_id)
    });
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| AppError::Memory(error.to_string()))?;
    crate::shell::atomic_write::atomic_write_private(&dir.join("grants.json"), &bytes)
        .map_err(|error| AppError::Memory(format!("无法撤销记忆共享授权：{error}")))
}

pub(crate) fn revoke(
    instance_id: &str,
    profile: &str,
    source_instance: &str,
    source_profile: &str,
    record_id: &str,
) -> Result<(), AppError> {
    let target = target(instance_id, profile)?;
    revoke_any(
        &target.instance_id,
        &target.profile,
        source_instance,
        source_profile,
        record_id,
    )
}

fn authorized(target: &Namespace, include_disabled: bool) -> Result<Vec<SharedMemory>, AppError> {
    validate_coordinate(&target.instance_id)?;
    validate_coordinate(&target.profile)?;
    let grants = load()?;
    let registry =
        instance::load_registry().map_err(|error| AppError::Memory(error.to_string()))?;
    let mut records = Vec::new();
    let mut documents = BTreeMap::new();
    for grant in grants.grants.into_iter().filter(|grant| {
        grant.target_instance == target.instance_id && grant.target_profile == target.profile
    }) {
        let Some(entry) = registry.get(&grant.source_instance) else {
            continue;
        };
        if entry.kernel_family != instance::KERNEL_FAMILY_DSH {
            continue;
        }
        let source = Namespace {
            family: instance::KERNEL_FAMILY_DSH.into(),
            instance_id: grant.source_instance.clone(),
            profile: grant.source_profile.clone(),
        };
        let key = (grant.source_instance.clone(), grant.source_profile.clone());
        if !documents.contains_key(&key) {
            let document = store::load(&source)?;
            if document.meta.schema_version > store::SCHEMA_VERSION {
                return Err(AppError::Memory(
                    "共享记忆由更新版本写入，暂不能读取".into(),
                ));
            }
            documents.insert(key.clone(), document);
        }
        if let Some(record) = documents[&key]
            .memories
            .iter()
            .find(|record| {
                record.id == grant.record_id
                    && (include_disabled || record.state == store::STATE_ACCEPTED)
            })
            .cloned()
        {
            if record.scope == store::SCOPE_PROJECT && record.workspace_path.is_empty() {
                continue;
            }
            let scope_valid = scope_allowed(&grant, &record);
            if !scope_valid && !include_disabled {
                continue;
            }
            records.push(SharedMemory {
                source_instance: grant.source_instance,
                source_profile: grant.source_profile,
                record,
                scope_valid,
            });
        }
    }
    Ok(records)
}

pub(crate) fn retrieve_mcode(
    target: &Namespace,
    query: &str,
    workspace_path: &str,
) -> Result<super::query::RetrievedContext, AppError> {
    if target.family != instance::KERNEL_FAMILY_MCODE {
        return Err(AppError::Memory("共享记忆目标无效".into()));
    }
    let current = store::load(target)?;
    if current.meta.schema_version > store::SCHEMA_VERSION {
        return Err(AppError::Memory(
            "MiniMax 记忆由更新版本写入，暂不能读取".into(),
        ));
    }
    if !current.meta.use_approved {
        return super::query::retrieve(target, query, "", workspace_path);
    }
    let mut shared = Vec::new();
    for item in authorized(target, false)? {
        let mut record = item.record;
        // Workspace ids belong to their original kernel; only the normalized path crosses families.
        record.workspace_id.clear();
        shared.push(record);
    }
    let workspace = super::workspaces::normalize_path(workspace_path);
    super::query::retrieve_with_shared(target, query, &workspace, shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::testkit::{general_input, TempHome};

    #[test]
    fn shared_lock_uses_instance_root_without_creating_shell_grants() {
        let _home = TempHome::new("mcode-share-cross-shell");
        let dev = paths::shell_dir(paths::ShellMode::Dev).join("memory-sharing");
        let release = paths::shell_dir(paths::ShellMode::Release).join("memory-sharing");
        assert!(!dev.exists() && !release.exists());
        let held = lock_grants().unwrap();
        let peer = OpenOptions::new()
            .write(true)
            .open(paths::kernels_root().join(".memory-sharing.lock"))
            .unwrap();
        assert!(peer.try_lock().is_err());
        assert!(!dev.exists() && !release.exists());
        drop(held);
        assert!(peer.try_lock().is_ok());
    }

    #[test]
    fn nested_grant_lock_survives_out_of_order_drop_and_blocks_writes() {
        let _home = TempHome::new("mcode-share-lock");
        let source = Namespace::current();
        let outer = lock_grants().unwrap();
        let inner = lock_grants().unwrap();
        drop(outer);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = store::save(&source, general_input("test", "content"));
            done_tx.send(result.is_ok()).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(inner);
        assert!(done_rx.recv_timeout(Duration::from_secs(3)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn explicit_grant_tracks_original_and_revocation_without_copying() {
        let _home = TempHome::new("mcode-shared-memory");
        let source = Namespace::current();
        let mut registry = instance::InstanceRegistry::default();
        let dsh = instance::InstanceRecord::new(
            &source.instance_id,
            instance::KERNEL_FAMILY_DSH,
            30001,
            1,
        );
        let mcode =
            instance::InstanceRecord::new("mcode-test", instance::KERNEL_FAMILY_MCODE, 30002, 1);
        instance::save_record_to_disk(&dsh).unwrap();
        instance::save_record_to_disk(&mcode).unwrap();
        registry.add(dsh).unwrap();
        registry.add(mcode).unwrap();
        instance::save_registry(&registry).unwrap();
        let config = paths::shell_dir(settings::current_mode()).join("mcode.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            config,
            r#"{"instance_id":"mcode-test","port":30002,"miniapp_port":30003}"#,
        )
        .unwrap();
        let target = Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, "mcode-test");
        let mut input = general_input("共享偏好", "原始内容");
        input.pinned = true;
        let record = store::save(&source, input.clone()).unwrap();
        store::set_use_approved(&target, true).unwrap();
        assert!(retrieve_mcode(&target, "test", "").unwrap().hits.is_empty());
        assert!(set(
            "mcode-test",
            &target.profile,
            &source.instance_id,
            &source.profile,
            &record.id,
            record.revision + 1,
            true
        )
        .is_err());
        set(
            "mcode-test",
            &target.profile,
            &source.instance_id,
            &source.profile,
            &record.id,
            record.revision,
            true,
        )
        .unwrap();
        assert_eq!(
            retrieve_mcode(&target, "test", "").unwrap().hits[0].body,
            "原始内容"
        );
        input.id = record.id.clone();
        input.expected_revision = Some(record.revision);
        input.body = "更新后的内容".into();
        let edited = store::save(&source, input.clone()).unwrap();
        assert_eq!(
            retrieve_mcode(&target, "test", "").unwrap().hits[0].body,
            "更新后的内容"
        );
        assert!(store::list(&target, &store::ListFilter::default())
            .memories
            .is_empty());
        input.scope = store::SCOPE_PROJECT.into();
        input.workspace_path = std::env::temp_dir()
            .join("xlink-scope-project")
            .to_string_lossy()
            .into_owned();
        input.expected_revision = Some(edited.revision);
        let project = store::save(&source, input.clone()).unwrap();
        set(
            "mcode-test",
            &target.profile,
            &source.instance_id,
            &source.profile,
            &record.id,
            project.revision,
            true,
        )
        .unwrap();
        assert_eq!(
            retrieve_mcode(&target, "test", &input.workspace_path)
                .unwrap()
                .hits
                .len(),
            1
        );
        input.scope = store::SCOPE_GENERAL.into();
        input.workspace_path.clear();
        input.expected_revision = Some(project.revision);
        store::save(&source, input).unwrap();
        assert!(retrieve_mcode(&target, "test", "").unwrap().hits.is_empty());
        assert!(!list("mcode-test", &target.profile).unwrap()[0].scope_valid);
        set(
            "mcode-test",
            &target.profile,
            &source.instance_id,
            &source.profile,
            &record.id,
            0,
            false,
        )
        .unwrap();
        assert!(retrieve_mcode(&target, "test", "").unwrap().hits.is_empty());
        assert!(store::find_by_id(&source, &record.id, true)
            .unwrap()
            .is_some());
    }
}
