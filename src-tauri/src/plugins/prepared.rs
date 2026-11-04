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
                if source.is_dir() {
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
        let path = root().join(format!(
            ".pending-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let draft = Self {
            path,
            digest: install_plan::tree_digest(source)?,
        };
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
        crate::shell::atomic_write::atomic_write_private(&self.path.join("item.json"), &bytes)
            .map_err(|e| e.to_string())?;
        crate::shell::atomic_write::atomic_write_private(
            &self.path.join("plan.json"),
            &serde_json::to_vec(plan).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        prune()?;
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
pub(crate) fn forget(id: &str) {
    if let Ok(dir) = directory(id) {
        if dir.join("plan.json").is_file() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}
fn prune() -> Result<(), String> {
    let mut plans = Vec::new();
    for entry in fs::read_dir(root()).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if !valid_id(&id) || !entry.path().join("plan.json").is_file() {
            continue;
        }
        match load(&id) {
            Some(plan) => plans.push(plan),
            None => forget(&id),
        }
    }
    plans.sort_by_key(|p| p.created_at_ms);
    while plans.len() >= 16 {
        forget(&plans.remove(0).plan_id);
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
    Ok(format!("{:x}", digest.finalize()))
}

/// 只备份调用者明确指定的路径；原件留在同盘备份中，失败时用 rename 恢复。
pub(crate) struct Backup {
    saved: Vec<(PathBuf, PathBuf)>,
    committed: bool,
}
impl Backup {
    pub fn capture(paths: &[PathBuf]) -> io::Result<Self> {
        let mut backup = Self {
            saved: Vec::new(),
            committed: false,
        };
        for path in paths {
            let saved = path.with_file_name(format!(
                ".{}-precheck-{}",
                path.file_name().unwrap().to_string_lossy(),
                crate::diagnostics::run::new_run_id()
            ));
            if fs::symlink_metadata(path).is_ok() {
                fs::rename(path, &saved)?;
                backup.saved.push((path.clone(), saved.clone()));
                copy(&saved, path, None)?;
            } else {
                backup.saved.push((path.clone(), saved));
            }
        }
        Ok(backup)
    }
    pub fn commit(mut self) {
        self.committed = true;
    }
    pub fn rollback(&mut self) -> io::Result<()> {
        for (path, saved) in self.saved.iter().rev() {
            remove(path)?;
            if fs::symlink_metadata(saved).is_ok() {
                fs::rename(saved, path)?;
            }
        }
        self.saved.clear();
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
                    &format!("恢复失败，备份保留：{e}"),
                );
            }
        } else {
            for (_, saved) in &self.saved {
                if let Err(e) = remove(saved) {
                    crate::shell::shell_events::record("precheck-cleanup", &e.to_string());
                }
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
        assert!(directory("../outside").is_err());
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
