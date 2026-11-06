//! 预检物料的本地保存与复制；应用只消费已验证目录，不调用取源器。
use super::{
    center::StoreItem,
    install_plan::{self, InstallPlan},
};
use crate::shell::{paths, process};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) fn root() -> PathBuf {
    paths::plugin_precheck_dir()
}
fn ensure_root() -> io::Result<()> {
    let path = root();
    fs::create_dir_all(&path)?;
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "预检缓存根目录不能是链接",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
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

/// 副本保留链接，不跟随外部目录；树内绝对链接按目标根重定位，树外只允许目标内核链接。
pub(crate) fn copy(source: &Path, target: &Path, kernel: Option<&Path>) -> io::Result<()> {
    let roots = kernel
        .map(|kernel| Ok::<_, io::Error>((source.canonicalize()?, kernel.canonicalize()?)))
        .transpose()?;
    copy_with_links(source, target, &mut |source, destination, meta| {
        let mut link = fs::read_link(source)?;
        if let Some((base, kernel)) = &roots {
            let resolved = fs::canonicalize(source)?;
            if let Ok(relative) = resolved.strip_prefix(base) {
                if link.is_absolute() {
                    link = target.join(relative);
                }
            } else if !(resolved.starts_with(kernel) && link.is_absolute()) {
                return Err(unportable(source));
            }
        }
        make_link(&link, destination, directory_link(meta))
    })
}

fn copy_with_links(
    source: &Path,
    target: &Path,
    link: &mut impl FnMut(&Path, &Path, &fs::Metadata) -> io::Result<()>,
) -> io::Result<()> {
    let meta = fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() {
        link(source, target, &meta)?;
    } else if meta.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_with_links(&entry.path(), &target.join(entry.file_name()), link)?;
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

fn unportable(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "物料链接无法安全搬迁：{}；请检查依赖后重新预检",
            path.display()
        ),
    )
}

fn directory_link(_meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        _meta.file_type().is_symlink_dir()
    }
    #[cfg(not(windows))]
    false
}

fn make_link(source: &Path, target: &Path, _dir: bool) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
    }
    #[cfg(windows)]
    if _dir && source.is_absolute() {
        crate::shell::link::make_junction(source, target)
    } else if _dir {
        crate::shell::link::make_dir_link(source, target)
    } else {
        std::os::windows::fs::symlink_file(source, target)
    }
}

#[derive(Serialize, Deserialize)]
struct ProfileLink {
    at: PathBuf,
    root: LinkRoot,
    target: PathBuf,
    directory: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum LinkRoot {
    Modules,
    Extensions,
    Store,
    Kernel,
}

/// 保存实际启动过的依赖树。用路径记录表示链接，发布目录改名也不会使 junction 悬空。
pub(super) fn capture_profile(
    profile: &Path,
    extensions: &Path,
    store: &Path,
    kernel: &Path,
    artifact: &Path,
) -> io::Result<()> {
    let modules = fs::canonicalize(profile.join("node_modules"))?;
    let roots = [
        (LinkRoot::Modules, Some(modules.clone())),
        (LinkRoot::Extensions, fs::canonicalize(extensions).ok()),
        (LinkRoot::Store, fs::canonicalize(store).ok()),
        (LinkRoot::Kernel, Some(fs::canonicalize(kernel)?)),
    ];
    let mut links = Vec::new();
    copy_with_links(
        &modules,
        &artifact.join("node_modules"),
        &mut |source, _, meta| {
            let raw = source.parent().unwrap().join(fs::read_link(source)?);
            let resolved = fs::canonicalize(source)?;
            // 保留通往实例物化目录的边界，即使物化目录本身也是中央库的链接。
            let direct = fs::canonicalize(raw.parent().ok_or_else(|| unportable(source))?)?
                .join(raw.file_name().ok_or_else(|| unportable(source))?);
            let (root, target) = [&direct, &resolved]
                .into_iter()
                .find_map(|path| {
                    roots.iter().find_map(|(kind, base)| {
                        path.strip_prefix(base.as_ref()?)
                            .ok()
                            .map(|relative| (*kind, relative.to_path_buf()))
                    })
                })
                .ok_or_else(|| unportable(source))?;
            links.push(ProfileLink {
                at: source.strip_prefix(&modules).unwrap().to_path_buf(),
                root,
                target,
                directory: directory_link(meta),
            });
            Ok(())
        },
    )?;
    for name in ["package.json", "pnpm-lock.yaml"] {
        match fs::copy(profile.join(name), artifact.join(name)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound && name == "pnpm-lock.yaml" => (),
            result => {
                result?;
            }
        }
    }
    links.sort_by(|a, b| a.at.cmp(&b.at));
    crate::shell::atomic_write::atomic_write_private(
        &artifact.join("links.json"),
        &serde_json::to_vec(&links)?,
    )
}

pub(super) fn apply_profile(
    artifact: &Path,
    profile: &Path,
    extensions: &Path,
    store: &Path,
    kernel: &Path,
    manifest: &serde_json::Value,
) -> io::Result<()> {
    let prepared: serde_json::Value =
        serde_json::from_slice(&fs::read(artifact.join("package.json"))?)?;
    if &prepared != manifest {
        return Err(io::Error::other("插件接线与预检时不同，请重新预检"));
    }
    let links: Vec<ProfileLink> = serde_json::from_slice(&fs::read(artifact.join("links.json"))?)?;
    let modules = profile.join("node_modules");
    remove(&modules)?;
    copy_with_links(
        &artifact.join("node_modules"),
        &modules,
        &mut |source, _, _| Err(unportable(source)),
    )?;
    let canonical_modules = modules.canonicalize()?;
    for link in &links {
        let local = |p: &Path| {
            p.components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        };
        if link.at.as_os_str().is_empty() || !local(&link.at) || !local(&link.target) {
            return Err(unportable(&link.at));
        }
        let at = modules.join(&link.at);
        let parent = at.parent().unwrap().canonicalize()?;
        if !parent.starts_with(&canonical_modules) {
            return Err(unportable(&link.at));
        }
    }
    for link in &links {
        let root = match link.root {
            LinkRoot::Modules => &modules,
            LinkRoot::Extensions => extensions,
            LinkRoot::Store => store,
            LinkRoot::Kernel => kernel,
        };
        make_link(
            &std::path::absolute(root.join(&link.target))?,
            &modules.join(&link.at),
            link.directory,
        )?;
    }
    for link in &links {
        fs::metadata(modules.join(&link.at))?;
    }
    let lock = profile.join("pnpm-lock.yaml");
    match fs::read(artifact.join("pnpm-lock.yaml")) {
        Ok(bytes) => crate::shell::atomic_write::atomic_write_private(&lock, &bytes)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => remove(&lock)?,
        Err(e) => return Err(e),
    }
    // 下次普通 pnpm 同步重建内部安装元数据（其中可能含沙盒绝对路径）。
    crate::shell::atomic_write::atomic_write_private(&profile.join(".xlink-prepared-store"), b"4")
}

pub(crate) struct Draft {
    pub path: PathBuf,
    pub digest: String,
    kernel: PathBuf,
}
impl Draft {
    pub fn new() -> Result<Self, String> {
        ensure_root().map_err(|e| e.to_string())?;
        let path = root().join(format!(".pending-{:032x}", rand::random::<u128>()));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        let draft = Self {
            path,
            digest: String::new(),
            kernel: PathBuf::new(),
        };
        fs::create_dir(draft.path.join("profile")).map_err(|e| e.to_string())?;
        crate::shell::atomic_write::atomic_write_private(
            &draft.path.join(".owner"),
            b"plugin-precheck-v4",
        )
        .map_err(|e| e.to_string())?;
        Ok(draft)
    }
    #[cfg(test)]
    pub fn freeze(source: &Path, kernel: &Path) -> Result<Self, String> {
        let mut draft = Self::new()?;
        draft.capture(source, kernel)?;
        Ok(draft)
    }
    pub fn capture(&mut self, source: &Path, kernel: &Path) -> Result<(), String> {
        let digest = install_plan::tree_digest(source)?;
        copy(source, &self.path.join("tree"), Some(kernel)).map_err(|e| e.to_string())?;
        if install_plan::tree_digest(&self.path.join("tree"))? != digest {
            return Err("预检物料复制后内容不一致".into());
        }
        self.digest = digest;
        self.kernel = kernel.to_path_buf();
        Ok(())
    }
    pub fn publish(self, plan: &InstallPlan, item: &StoreItem) -> Result<(), String> {
        if install_plan::full_tree_digest(&self.path.join("profile"))?
            != plan.profile_artifact_digest
        {
            return Err("预检依赖物料已变化，请重新预检".into());
        }
        if plan.content_digest != self.digest {
            return Err("启动期间物料发生变化，请重新预检".into());
        }
        let bytes = serde_json::to_vec(item).map_err(|e| e.to_string())?;
        if plan.artifact_item_digest != install_plan::hex_sha256(&bytes) {
            return Err("预检物料身份不一致".into());
        }
        prune(true)?;
        let target = directory(&plan.plan_id)?;
        fs::create_dir(&target).map_err(|e| e.to_string())?;
        let publish = || -> Result<(), String> {
            let private = crate::shell::atomic_write::atomic_write_private;
            private(&target.join(".owner"), b"plugin-precheck-v4").map_err(|e| e.to_string())?;
            // 必须按最终路径创建绝对 junction；直接改名会使它仍指向暂存目录。
            copy(
                &self.path.join("tree"),
                &target.join("tree"),
                Some(&self.kernel),
            )
            .map_err(|e| e.to_string())?;
            if install_plan::tree_digest(&target.join("tree"))? != self.digest {
                return Err("已发布的插件物料与预检不一致".into());
            }
            fs::rename(self.path.join("profile"), target.join("profile"))
                .map_err(|e| e.to_string())?;
            private(&target.join("item.json"), &bytes).map_err(|e| e.to_string())?;
            private(
                &target.join("plan.json"),
                &serde_json::to_vec(plan).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        };
        let result = publish();
        if result.is_err() {
            let _ = remove(&target);
        }
        result
    }
}
impl Drop for Draft {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn owned_cache(dir: &Path) -> bool {
    fs::read(dir.join(".owner")).ok().is_some_and(|bytes| {
        [
            b"plugin-precheck-v2",
            b"plugin-precheck-v3",
            b"plugin-precheck-v4",
        ]
        .iter()
        .any(|marker| bytes == *marker)
    })
}

fn rollback_target(path: &Path, index: usize) -> bool {
    let mode = paths::ShellMode::current();
    let family = crate::shell::instance::KERNEL_FAMILY_DSH;
    let valid = |part: &std::ffi::OsStr| {
        part.to_str()
            .is_some_and(|id| paths::validate_id_component(id).is_ok())
    };
    match index {
        0 => {
            path.parent() == Some(paths::plugins_store_root().as_path())
                && path.file_name().is_some_and(valid)
        }
        1 => path == paths::plugins_store_root().join("store.json"),
        2 | 3 => {
            let root = paths::kernel_instances_dir(family);
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            let parts: Vec<_> = relative.iter().collect();
            match parts.as_slice() {
                [id, ext, plugins] if index == 2 => {
                    valid(id) && *ext == "extensions" && *plugins == "plugins"
                }
                [id, home, profiles, profile] if index == 3 => {
                    valid(id) && *home == "home" && *profiles == "profiles" && valid(profile)
                }
                _ => false,
            }
        }
        4 => path == paths::family_runtime_dir(family, mode).join("quarantine.json"),
        _ => false,
    }
}

pub(crate) fn load(id: &str) -> Option<InstallPlan> {
    let dir = directory(id).ok()?;
    if fs::read(dir.join(".owner")).ok().as_deref() != Some(b"plugin-precheck-v4") {
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
        if !owned_cache(&dir) {
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

/// 在插件启动对账前恢复被移走且正式位置仍空缺的原件。
pub(crate) fn recover_interrupted() {
    let _guard = super::center::lock_store();
    let root = root();
    if !fs::symlink_metadata(&root).is_ok_and(|meta| meta.file_type().is_dir()) {
        return;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let home = paths::xlink_home();
    let Ok(canonical_home) = home.canonicalize() else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|s| s.strip_prefix(".rollback-")) else {
            continue;
        };
        if id.len() != 32
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
            || !entry.file_type().is_ok_and(|t| t.is_dir())
        {
            continue;
        }
        let dir = entry.path();
        if dir.join(".committed").is_file() {
            continue;
        }
        let mut restored = 0;
        if dir.join(".pending").is_file() {
            let targets = fs::read(dir.join("paths.json"))
                .ok()
                .and_then(|data| serde_json::from_slice::<Vec<PathBuf>>(&data).ok());
            if let Some(targets) = targets {
                for (index, target) in targets.iter().enumerate().rev() {
                    let saved = dir.join(index.to_string());
                    let managed = rollback_target(target, index)
                        && target
                            .parent()
                            .and_then(|parent| parent.canonicalize().ok())
                            .is_some_and(|parent| parent.starts_with(&canonical_home));
                    if managed
                        && fs::symlink_metadata(target)
                            .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                        && fs::symlink_metadata(&saved).is_ok()
                        && fs::rename(&saved, target).is_ok()
                    {
                        restored += 1;
                    }
                }
            }
        }
        crate::shell::shell_events::record(
            "precheck-rollback",
            &format!("发现中断的插件事务，已恢复 {restored} 个缺失路径。请查看备份 {} 与其中的 paths.json，核对其余原件", dir.display()),
        );
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
        if !owned_cache(&entry.path()) {
            continue;
        }
        if id.starts_with(".pending-")
            || (valid_id(&id) && !entry.path().join("plan.json").is_file())
        {
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

pub(crate) fn profile_fingerprint(
    data_dir: &Path,
    family: &str,
    instance: &str,
    profile: &str,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let root = paths::instance_profile_dir(family, instance, profile);
    let mut digest = Sha256::new();
    for name in [
        "package.json",
        "pnpm-workspace.yaml",
        "pnpm-lock.yaml",
        ".npmrc",
        "cordis.patch.yml",
    ] {
        digest.update(name);
        match fs::read(root.join(name)) {
            Ok(bytes) => {
                digest.update([1]);
                digest.update((bytes.len() as u64).to_le_bytes());
                digest.update(bytes);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => digest.update([0]),
            Err(error) => return Err(format!("读取目标运行配置失败：{error}")),
        }
    }
    // 复制模式的运行树也包含已安装的插件；同一路径升级不会改 profile 清单。
    let store = super::center::load_store_checked(data_dir).map_err(|e| e.to_string())?;
    let mut plugins: Vec<_> = store
        .items
        .iter()
        .map(|item| (&item.id, &item.name, &item.installed_version, &item.mode))
        .collect();
    plugins.sort_unstable();
    let mut quarantined: Vec<_> = super::quarantine::ids(data_dir).into_iter().collect();
    quarantined.sort_unstable();
    digest.update(serde_json::to_vec(&(plugins, quarantined)).map_err(|e| e.to_string())?);
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
        ensure_root()?;
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
        crate::shell::atomic_write::atomic_write_private(&backup.dir.join(".pending"), b"1")?;
        for (index, path) in paths.iter().enumerate() {
            let saved = backup.dir.join(index.to_string());
            match fs::symlink_metadata(path) {
                Ok(_) => {
                    let working = backup.dir.join("working");
                    // 复制失败或复制期间中断时，正式位置仍是完整的原件。
                    copy(path, &working, None)?;
                    fs::rename(path, &saved)?;
                    backup.saved.push((path.clone(), Some(saved.clone())));
                    fs::rename(&working, path)?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    backup.saved.push((path.clone(), None))
                }
                Err(e) => return Err(e),
            }
        }
        Ok(backup)
    }
    pub fn commit(mut self) -> io::Result<()> {
        crate::shell::atomic_write::atomic_write_private(&self.dir.join(".committed"), b"1")?;
        self.committed = true;
        Ok(())
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
        #[cfg(windows)]
        Ok(meta) if directory_link(&meta) => crate::shell::link::remove_dir_link(path),
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
    use crate::plugins::center;
    use crate::shell::settings::Settings;
    fn plan(item: &StoreItem, draft: &Draft, settings: &Settings) -> InstallPlan {
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
            content_digest: draft.digest.clone(),
            artifact_item_digest: install_plan::hex_sha256(&serde_json::to_vec(item).unwrap()),
            profile_artifact_digest: install_plan::full_tree_digest(&draft.path.join("profile"))
                .unwrap(),
            profile_fingerprint: String::new(),
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
        let item = StoreItem {
            source: "https://fixture-secret@example.invalid/repo".into(),
            ..Default::default()
        };
        let plan = plan(&item, &draft, &Settings::default());
        assert!(!serde_json::to_string(&plan)
            .unwrap()
            .contains("fixture-secret"));
        draft.publish(&plan, &item).unwrap();
        assert_eq!(load(&plan.plan_id).unwrap(), plan);
        assert!(
            fs::read_to_string(directory(&plan.plan_id).unwrap().join("item.json"))
                .unwrap()
                .contains("fixture-secret")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(directory(&plan.plan_id).unwrap().join("item.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let file = directory(&plan.plan_id).unwrap().join("plan.json");
        let mut tampered = plan.clone();
        tampered.profile.push_str("other");
        fs::write(&file, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(load(&plan.plan_id).is_none());
        fs::write(
            directory(&plan.plan_id).unwrap().join(".owner"),
            b"plugin-precheck-v2",
        )
        .unwrap();
        assert!(load(&plan.plan_id).is_none(), "旧物料不能进入应用链");
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
    fn startup_recovers_missing_original_without_replacing_live_paths() {
        let home = std::env::temp_dir().join(format!(
            "prepared-startup-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let store = paths::plugins_store_root();
        let plugin = store.join("fixture");
        let manifest = store.join("store.json");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("index.js"), b"original").unwrap();
        fs::write(&manifest, b"old manifest").unwrap();
        let backup = Backup::capture(&[plugin.clone(), manifest.clone()]).unwrap();
        let dir = backup.dir.clone();
        fs::rename(&plugin, dir.join("working")).unwrap();
        fs::write(&manifest, b"live manifest").unwrap();
        std::mem::forget(backup);

        recover_interrupted();

        assert_eq!(fs::read(plugin.join("index.js")).unwrap(), b"original");
        assert_eq!(fs::read(&manifest).unwrap(), b"live manifest");
        assert_eq!(fs::read(dir.join("1")).unwrap(), b"old manifest");
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
            let mut plan = plan(&item, &draft, &Settings::default());
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

    #[test]
    fn local_apply_uses_offline_wiring_and_rolls_back_failed_update() {
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
        let profile = home.join("sandbox-profile");
        let dep = profile.join("node_modules/profile-only-dep");
        fs::create_dir_all(&dep).unwrap();
        fs::write(dep.join("generated.cjs"), b"module.exports = 42").unwrap();
        make_link(&dep, &profile.join("node_modules/linked-dep"), true).unwrap();
        let sandbox_extensions = home.join("sandbox-extensions");
        fs::create_dir_all(sandbox_extensions.join("existing-copy")).unwrap();
        make_link(
            &sandbox_extensions.join("existing-copy"),
            &profile.join("node_modules/linked-plugin"),
            true,
        )
        .unwrap();
        let mut expected =
            crate::kernel::profile_manifest::initial_manifest(&settings.profile, None);
        expected["dependencies"] = serde_json::json!({
            "existing-copy": "file:../../../extensions/plugins/existing-copy",
            "fixture": "file:../../../extensions/plugins/fixture",
        });
        fs::write(profile.join("package.json"), expected.to_string()).unwrap();
        capture_profile(
            &profile,
            &sandbox_extensions,
            &center::store_dir(&data),
            &kernel,
            &draft.path.join("profile"),
        )
        .unwrap();
        let mut plan = plan(&item, &draft, &settings);
        plan.materialize = "copy".into();
        let plan = plan.finalize();
        draft.publish(&plan, &item).unwrap();
        let existing = center::store_dir(&data).join("existing-copy");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("package.json"), br#"{"name":"existing-copy","main":"index.js","dependencies":{"profile-only-dep":"1.0.0"}}"#).unwrap();
        fs::write(existing.join("index.js"), b"old copy").unwrap();
        fs::write(center::store_file(&data), br#"{"schemaVersion":1,"items":[{"id":"existing-copy","name":"existing-copy","source":"existing-copy","installed_version":"1.0.0","mode":"copy"}]}"#).unwrap();
        center::install_prepared_for_instance("dsh", "work", &data, &settings, &plan, &mut |_| {})
            .unwrap();
        let target = center::store_dir(&data).join("fixture");
        fs::write(target.join("index.js"), b"previous-version").unwrap();
        let manifest =
            paths::instance_profile_dir("dsh", "work", &settings.profile).join("package.json");
        let applied_dep = manifest
            .parent()
            .unwrap()
            .join("node_modules/profile-only-dep/generated.cjs");
        assert_eq!(fs::read(&applied_dep).unwrap(), b"module.exports = 42");
        assert_eq!(
            fs::read(
                manifest
                    .with_file_name("node_modules")
                    .join("linked-dep/generated.cjs")
            )
            .unwrap(),
            b"module.exports = 42"
        );
        assert_eq!(
            fs::read(
                manifest
                    .with_file_name("node_modules")
                    .join("linked-plugin/index.js")
            )
            .unwrap(),
            b"old copy"
        );
        expected["dependencies"]["user-added"] = "1.0.0".into();
        fs::write(&manifest, expected.to_string()).unwrap();
        let before = fs::read(&manifest).unwrap();
        assert!(manifest
            .parent()
            .unwrap()
            .join(".xlink-prepared-store")
            .is_file());
        let error = center::install_prepared_for_instance(
            "dsh",
            "work",
            &data,
            &settings,
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
        assert_eq!(fs::read(applied_dep).unwrap(), b"module.exports = 42");
        let saved_dep = directory(&plan.plan_id)
            .unwrap()
            .join("profile/node_modules/profile-only-dep/generated.cjs");
        let saved_dep_before = fs::read(&saved_dep).unwrap();
        fs::write(&saved_dep, b"tampered").unwrap();
        let error = center::install_prepared_for_instance(
            "dsh",
            "work",
            &data,
            &settings,
            &plan,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("依赖物料已变化"));
        assert_eq!(
            fs::read(target.join("index.js")).unwrap(),
            b"previous-version"
        );
        fs::write(saved_dep, saved_dep_before).unwrap();
        let item_file = directory(&plan.plan_id).unwrap().join("item.json");
        let item_before = fs::read(&item_file).unwrap();
        fs::write(&item_file, b"{}").unwrap();
        let error = center::install_prepared_for_instance(
            "dsh",
            "work",
            &data,
            &settings,
            &plan,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("身份记录已变化"), "{error}");
        assert_eq!(
            fs::read(target.join("index.js")).unwrap(),
            b"previous-version"
        );
        fs::write(item_file, item_before).unwrap();
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
            &plan,
            &mut |_| {}
        )
        .is_err());
        assert_eq!(
            fs::read(target.join("index.js")).unwrap(),
            b"previous-version"
        );
    }
    #[test]
    fn materialization_metadata_is_excluded_only_at_the_plugin_root() {
        let home = std::env::temp_dir().join(format!(
            "prepared-meta-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("plugin");
        fs::create_dir_all(source.join("runtime")).unwrap();
        fs::write(source.join("index.js"), b"verified").unwrap();
        let before = install_plan::tree_digest(&source).unwrap();
        fs::write(source.join(".dsh-meta.json"), b"shell timestamp").unwrap();
        assert_eq!(install_plan::tree_digest(&source).unwrap(), before);
        fs::write(source.join("runtime/.dsh-meta.json"), b"runtime data").unwrap();
        assert_ne!(install_plan::tree_digest(&source).unwrap(), before);
    }

    #[test]
    fn changing_target_profile_dependencies_invalidates_the_plan() {
        let home = std::env::temp_dir().join(format!(
            "prepared-profile-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("source");
        let kernel = home.join("kernel");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&kernel).unwrap();
        fs::write(source.join("index.js"), b"verified").unwrap();
        let settings = Settings::default();
        let data = crate::kernel::lifecycle::data_dir("dsh");
        crate::kernel::lifecycle::write_active(&data, Some("test")).unwrap();
        let draft = Draft::freeze(&source, &kernel).unwrap();
        let mut plan = plan(&StoreItem::default(), &draft, &settings);
        plan.profile_fingerprint =
            profile_fingerprint(&data, "dsh", "work", &settings.profile).unwrap();
        plan = plan.finalize();
        plan.validate_for("dsh", "work", &data, &settings, "link", "v22.0.0")
            .unwrap();
        let profile = paths::instance_profile_dir("dsh", "work", &settings.profile);
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join("package.json"), b"{}").unwrap();
        assert!(plan
            .validate_for("dsh", "work", &data, &settings, "link", "v22.0.0")
            .unwrap_err()
            .contains("运行配置"));
    }

    #[cfg(unix)]
    #[test]
    fn internal_absolute_links_survive_freeze_and_relocation() {
        let home = std::env::temp_dir().join(format!(
            "prepared-absolute-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("source");
        let kernel = home.join("kernel");
        fs::create_dir_all(source.join("dep")).unwrap();
        fs::create_dir_all(&kernel).unwrap();
        fs::write(source.join("dep/index.js"), b"dependency").unwrap();
        std::os::unix::fs::symlink(source.join("dep"), source.join("link")).unwrap();
        let draft = Draft::freeze(&source, &kernel).unwrap();
        let target = home.join("applied");
        copy(&draft.path.join("tree"), &target, Some(&kernel)).unwrap();
        fs::remove_dir_all(source).unwrap();
        assert_eq!(
            fs::read(target.join("link/index.js")).unwrap(),
            b"dependency"
        );
        assert_eq!(install_plan::tree_digest(&target).unwrap(), draft.digest);
    }

    #[cfg(windows)]
    #[test]
    fn internal_windows_junctions_survive_relocation_without_symlink_privilege() {
        let home = std::env::temp_dir().join(format!(
            "prepared-junction-{}",
            crate::diagnostics::run::new_run_id()
        ));
        let _home = crate::tests::scoped_xlink_home(&home);
        let source = home.join("source & % !");
        let kernel = home.join("kernel");
        fs::create_dir_all(source.join("dep")).unwrap();
        fs::create_dir_all(&kernel).unwrap();
        fs::write(source.join("dep/index.js"), b"dependency").unwrap();
        crate::shell::link::make_junction(&source.join("dep"), &source.join("link")).unwrap();
        let draft = Draft::freeze(&source, &kernel).unwrap();
        let target = home.join("applied");
        copy(&draft.path.join("tree"), &target, Some(&kernel)).unwrap();
        fs::remove_dir_all(source).unwrap();
        assert_eq!(
            fs::read(target.join("link/index.js")).unwrap(),
            b"dependency"
        );
        assert_eq!(install_plan::tree_digest(&target).unwrap(), draft.digest);
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
