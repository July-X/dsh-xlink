//! 预检物料的本地保存与复制；应用只消费已验证目录，不调用取源器。
use super::{
    center::StoreItem,
    install_plan::{self, InstallPlan},
};
use crate::shell::{paths, process};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) fn root() -> PathBuf {
    paths::plugin_precheck_dir()
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(crate) fn directory(id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("安装计划标识无效，请重新预检".into());
    }
    Ok(root().join(id))
}

/// 副本保留链接，不跟随外部目录；物料只允许树内相对链接或目标内核的绝对链接。
pub(crate) fn copy(source: &Path, target: &Path, kernel: Option<&Path>) -> io::Result<()> {
    fn walk(source: &Path, target: &Path, base: &Path, kernel: Option<&Path>) -> io::Result<()> {
        let meta = fs::symlink_metadata(source)?;
        if meta.file_type().is_symlink() {
            let link = fs::read_link(source)?;
            if let Some(kernel) = kernel {
                let resolved = fs::canonicalize(source)?;
                if !(resolved.starts_with(base) && link.is_relative()
                    || resolved.starts_with(kernel) && link.is_absolute())
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "物料链接无法安全搬迁：{}；请检查依赖后重新预检",
                            source.display()
                        ),
                    ));
                }
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, target)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::FileTypeExt;
                if meta.file_type().is_symlink_dir() {
                    super::center::make_dir_link(&link, target)?;
                } else {
                    std::os::windows::fs::symlink_file(link, target)?;
                }
            }
        } else if meta.is_dir() {
            fs::create_dir_all(target)?;
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                walk(&entry.path(), &target.join(entry.file_name()), base, kernel)?;
            }
        } else if meta.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source, target)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "物料包含非普通文件",
            ));
        }
        Ok(())
    }
    let base = fs::canonicalize(source)?;
    let kernel = kernel.map(fs::canonicalize).transpose()?;
    walk(source, target, &base, kernel.as_deref())
}

pub(crate) struct Draft {
    pub path: PathBuf,
    pub digest: String,
}
impl Draft {
    pub fn freeze(source: &Path, kernel: &Path) -> Result<Self, String> {
        fs::create_dir_all(root()).map_err(|e| e.to_string())?;
        let path = root().join(format!(".pending-{:032x}", rand::random::<u128>()));
        let digest = install_plan::tree_digest(source)?;
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        let draft = Self { path, digest };
        crate::shell::atomic_write::atomic_write_private(
            &draft.path.join(".owner"),
            b"plugin-precheck-v2",
        )
        .map_err(|e| e.to_string())?;
        copy(source, &draft.path.join("tree"), Some(kernel)).map_err(|e| e.to_string())?;
        if install_plan::tree_digest(&draft.path.join("tree"))? != draft.digest {
            return Err("预检物料复制后内容不一致".into());
        }
        Ok(draft)
    }
    pub fn publish(self, plan: &InstallPlan, item: &StoreItem) -> Result<(), String> {
        if plan.content_digest != self.digest {
            return Err("启动期间物料发生变化，请重新预检".into());
        }
        let bytes = serde_json::to_vec(item).map_err(|e| e.to_string())?;
        if plan.artifact_item != String::from_utf8(bytes.clone()).map_err(|e| e.to_string())? {
            return Err("预检物料身份不一致".into());
        }
        crate::shell::atomic_write::atomic_write_private(
            &self.path.join("plan.json"),
            &serde_json::to_vec(plan).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        prune(true)?;
        fs::rename(&self.path, directory(&plan.plan_id)?).map_err(|e| e.to_string())
    }
}
impl Drop for Draft {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn load(id: &str) -> Option<InstallPlan> {
    let dir = directory(id).ok()?;
    if fs::read(dir.join(".owner")).ok().as_deref() != Some(b"plugin-precheck-v2") {
        return None;
    }
    let plan: InstallPlan = serde_json::from_slice(&fs::read(dir.join("plan.json")).ok()?).ok()?;
    if plan.plan_id != id
        || plan.schema_version != install_plan::SCHEMA_VERSION
        || plan.verify_id().is_err()
        || plan.expires_at_ms < process::epoch_millis()
    {
        return None;
    }
    Some(plan)
}
pub(crate) fn forget(id: &str) -> Result<(), String> {
    let dir = directory(id)?;
    if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("预检缓存目录被替换成链接，已拒绝清理".into());
    }
    if dir.join("plan.json").is_file() {
        if fs::read(dir.join(".owner")).ok().as_deref() != Some(b"plugin-precheck-v2") {
            return Err("缺少本项目物料所有权标记，拒绝清理".into());
        }
        fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(crate) fn cleanup() {
    if !root().is_dir() {
        return;
    }
    let _store_guard = super::center::lock_store();
    if let Err(error) = prune(false) {
        crate::shell::shell_events::record("precheck-cache", &format!("物料清理失败：{error}"));
    }
}
fn prune(reserve_slot: bool) -> Result<(), String> {
    let mut plans = Vec::new();
    for entry in fs::read_dir(root()).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if fs::read(entry.path().join(".owner")).ok().as_deref() != Some(b"plugin-precheck-v2") {
            continue;
        }
        if id.starts_with(".pending-") {
            if entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_millis() > install_plan::TTL_MS as u128)
            {
                fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
            }
            continue;
        }
        if !valid_id(&id) || !entry.path().join("plan.json").is_file() {
            continue;
        }
        match load(&id) {
            Some(plan) => plans.push(plan),
            None => forget(&id)?,
        }
    }
    plans.sort_by_key(|p| p.created_at_ms);
    while plans.len() > install_plan::MAX_PLANS - usize::from(reserve_slot) {
        forget(&plans.remove(0).plan_id)?;
    }
    Ok(())
}

pub(crate) fn kernel_fingerprint(data_dir: &Path, version: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let root = crate::kernel::lifecycle::kernel_dir(data_dir, version);
    let mut digest = Sha256::new();
    for name in ["package.json", "pnpm-lock.yaml"] {
        digest.update(name);
        match fs::read(root.join(name)) {
            Ok(bytes) => digest.update(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound && name != "package.json" => (),
            Err(e) => return Err(format!("读取内核指纹失败：{e}")),
        }
    }
    Ok(crate::shell::digest::hex(digest.finalize()))
}

/// 只备份调用者明确指定的路径；原件留在同盘备份中，失败时用 rename 恢复。
pub(crate) struct Backup {
    dir: PathBuf,
    saved: Vec<(PathBuf, Option<PathBuf>)>,
    committed: bool,
}
impl Backup {
    pub fn capture(paths: &[PathBuf]) -> io::Result<Self> {
        fs::create_dir_all(root())?;
        let dir = root().join(format!(".rollback-{:032x}", rand::random::<u128>()));
        fs::create_dir(&dir)?;
        let mut backup = Self {
            dir,
            saved: Vec::new(),
            committed: false,
        };
        // 事务目录不在插件/profile 的清扫范围内；路径表供中断后的人工恢复。
        crate::shell::atomic_write::atomic_write_private(
            &backup.dir.join("paths.json"),
            &serde_json::to_vec(paths)?,
        )?;
        for (index, path) in paths.iter().enumerate() {
            let saved = backup.dir.join(index.to_string());
            match fs::symlink_metadata(path) {
                Ok(_) => {
                    fs::rename(path, &saved)?;
                    backup.saved.push((path.clone(), Some(saved.clone())));
                    copy(&saved, path, None)?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    backup.saved.push((path.clone(), None))
                }
                Err(e) => return Err(e),
            }
        }
        Ok(backup)
    }
    pub fn commit(mut self) {
        self.committed = true;
    }
    pub fn rollback(&mut self) -> io::Result<()> {
        while let Some((path, saved)) = self.saved.last() {
            // 先确认原件存在；部分恢复后立即出栈，Drop 重试不能删掉已恢复的数据。
            if let Some(saved) = saved {
                fs::symlink_metadata(saved)?;
            }
            remove(path)?;
            if let Some(saved) = saved {
                fs::rename(saved, path)?;
            }
            self.saved.pop();
        }
        if let Err(error) = remove(&self.dir) {
            crate::shell::shell_events::record(
                "precheck-cleanup",
                &format!("{}：{error}", self.dir.display()),
            );
        }
        Ok(())
    }
}
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
impl Drop for Backup {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(e) = self.rollback() {
                crate::shell::shell_events::record(
                    "precheck-rollback",
                    &format!("恢复失败，备份保留在 {}：{e}", self.dir.display()),
                );
            }
        } else {
            if let Err(e) = remove(&self.dir) {
                crate::shell::shell_events::record(
                    "precheck-cleanup",
                    &format!("{}：{e}", self.dir.display()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{plugins::center, shell::settings::Settings};
    fn plan(item: &StoreItem, digest: String, settings: &Settings) -> InstallPlan {
        let now = process::epoch_millis();
        InstallPlan {
            schema_version: install_plan::SCHEMA_VERSION,
            plan_id: String::new(),
            spec: "fixture@1.0.0".into(),
            pinned_spec: "fixture@1.0.0".into(),
            source_kind: "npm".into(),
            requested_pin: "1.0.0".into(),
            resolved_version: "1.0.0".into(),
            resolved_commit: String::new(),
            integrity: "fixture-digest".into(),
            content_digest: digest,
            artifact_item: serde_json::to_string(item).unwrap(),
            kernel_fingerprint: String::new(),
            kernel_version: "test".into(),
            family: "dsh".into(),
            target_instance: "work".into(),
            profile: settings.profile.clone(),
            port: settings.port,
            materialize: "link".into(),
            platform: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            node_version: "v22.0.0".into(),
            created_at_ms: now,
            expires_at_ms: now + install_plan::TTL_MS,
            evidence_run_id: "fixture".into(),
        }
        .finalize()
    }
    #[test]
    fn disk_plan_survives_memory_loss_and_refuses_tampering() {
        let home = std::env::temp_dir().join(format!(
            "prepared-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("source");
        let kernel = home.join("kernel");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&kernel).unwrap();
        fs::write(source.join("index.js"), b"verified").unwrap();
        let draft = Draft::freeze(&source, &kernel).unwrap();
        let item = StoreItem::default();
        let plan = plan(&item, draft.digest.clone(), &Settings::default());
        draft.publish(&plan, &item).unwrap();
        assert_eq!(load(&plan.plan_id).unwrap(), plan);
        let file = directory(&plan.plan_id).unwrap().join("plan.json");
        let mut tampered = plan.clone();
        tampered.profile.push_str("other");
        fs::write(&file, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(load(&plan.plan_id).is_none());
        let foreign = root().join("foreign-user-data");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("keep"), b"untouched").unwrap();
        cleanup();
        assert!(!directory(&plan.plan_id).unwrap().exists());
        assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"untouched");
        assert!(directory("../outside").is_err());
    }
    #[test]
    fn interrupted_rollback_never_deletes_an_already_restored_original() {
        let home = std::env::temp_dir().join(format!(
            "prepared-rollback-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        fs::create_dir_all(&home).unwrap();
        let a = home.join("a");
        let b = home.join("b");
        fs::write(&a, b"old-a").unwrap();
        fs::write(&b, b"old-b").unwrap();
        let mut backup = Backup::capture(&[a.clone(), b.clone()]).unwrap();
        fs::write(&a, b"new-a").unwrap();
        fs::write(&b, b"new-b").unwrap();
        fs::remove_file(backup.saved[0].1.as_ref().unwrap()).unwrap();
        assert!(backup.rollback().is_err());
        assert_eq!(fs::read(&b).unwrap(), b"old-b");
        drop(backup);
        assert_eq!(fs::read(&b).unwrap(), b"old-b");
        assert_eq!(
            fs::read(&a).unwrap(),
            b"new-a",
            "原件备份缺失时不能再破坏现有文件"
        );
    }

    #[test]
    fn persistent_cache_is_bounded_and_forgettable() {
        let home = std::env::temp_dir().join(format!(
            "prepared-bound-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("source");
        let kernel = home.join("kernel");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&kernel).unwrap();
        fs::write(source.join("index.js"), b"fixture").unwrap();
        let item = StoreItem::default();
        let mut ids = Vec::new();
        for i in 0..17 {
            let draft = Draft::freeze(&source, &kernel).unwrap();
            let mut plan = plan(&item, draft.digest.clone(), &Settings::default());
            plan.created_at_ms = process::epoch_millis() + i * 1000;
            plan = plan.finalize();
            ids.push(plan.plan_id.clone());
            draft.publish(&plan, &item).unwrap();
        }
        assert!(load(&ids[0]).is_none());
        assert_eq!(
            fs::read_dir(root()).unwrap().count(),
            install_plan::MAX_PLANS
        );
        forget(ids.last().unwrap()).unwrap();
        assert!(load(ids.last().unwrap()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn local_apply_uses_offline_wiring_and_rolls_back_failed_update() {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::temp_dir().join(format!(
            "prepared-apply-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let data = crate::kernel::lifecycle::data_dir("dsh");
        let kernel = crate::kernel::lifecycle::kernel_dir(&data, "test");
        fs::create_dir_all(&kernel).unwrap();
        fs::write(kernel.join("package.json"), b"{}").unwrap();
        crate::kernel::lifecycle::write_active(&data, Some("test")).unwrap();
        let source = home.join("candidate");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("package.json"),
            br#"{"name":"fixture","version":"1.0.0","main":"index.js"}"#,
        )
        .unwrap();
        fs::write(source.join("index.js"), b"new").unwrap();
        crate::pkg::fetch::write_source_marker(&source, "fixture", "npm", "fixture", "1.0.0")
            .unwrap();
        let item = StoreItem {
            id: "fixture".into(),
            name: "fixture".into(),
            source: "fixture".into(),
            installed_version: "1.0.0".into(),
            ..Default::default()
        };
        let settings = Settings::default();
        let draft = Draft::freeze(&source, &kernel).unwrap();
        let plan = plan(&item, draft.digest.clone(), &settings);
        draft.publish(&plan, &item).unwrap();
        let pnpm = home.join("pnpm");
        fs::write(&pnpm, "#!/bin/sh\ncase \"$*\" in *--offline*--ignore-scripts*) mkdir -p node_modules; exit 0;; *) exit 90;; esac\n").unwrap();
        fs::set_permissions(&pnpm, fs::Permissions::from_mode(0o700)).unwrap();
        center::install_prepared_for_instance(
            "dsh",
            "work",
            &data,
            &settings,
            &pnpm,
            &plan,
            &mut |_| {},
        )
        .unwrap();
        let target = center::store_dir(&data).join("fixture");
        fs::write(target.join("index.js"), b"previous-version").unwrap();
        let manifest =
            paths::instance_profile_dir("dsh", "work", &settings.profile).join("package.json");
        let before = fs::read(&manifest).unwrap();
        fs::remove_dir_all(manifest.parent().unwrap().join("node_modules")).unwrap();
        fs::write(&pnpm, "#!/bin/sh\nexit 7\n").unwrap();
        let error = center::install_prepared_for_instance(
            "dsh",
            "work",
            &data,
            &settings,
            &pnpm,
            &plan,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("已恢复"), "{error}");
        assert_eq!(
            fs::read(target.join("index.js")).unwrap(),
            b"previous-version"
        );
        assert_eq!(fs::read(manifest).unwrap(), before);
        fs::write(
            directory(&plan.plan_id).unwrap().join("tree/index.js"),
            b"tampered",
        )
        .unwrap();
        assert!(center::install_prepared_for_instance(
            "dsh",
            "work",
            &data,
            &settings,
            &pnpm,
            &plan,
            &mut |_| {}
        )
        .is_err());
        assert_eq!(
            fs::read(target.join("index.js")).unwrap(),
            b"previous-version"
        );
    }
    #[cfg(unix)]
    #[test]
    fn freeze_preserves_internal_links_and_rejects_external_files() {
        let home = std::env::temp_dir().join(format!(
            "prepared-links-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("source");
        let kernel = home.join("kernel");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&kernel).unwrap();
        fs::write(source.join("dep"), b"dependency").unwrap();
        std::os::unix::fs::symlink("dep", source.join("link")).unwrap();
        let draft = Draft::freeze(&source, &kernel).unwrap();
        assert_eq!(
            fs::read(draft.path.join("tree/link")).unwrap(),
            b"dependency"
        );
        fs::write(home.join("private"), b"private").unwrap();
        std::os::unix::fs::symlink(home.join("private"), source.join("escape")).unwrap();
        assert!(Draft::freeze(&source, &kernel).is_err());
    }
}
