//! MiniMax-owned, verified Ponytail source cache; never touches the DSH cache.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    pkg::{archive, fetch},
    shell::{paths, version},
    skills::{
        manage,
        mcode::InstanceLease,
        store_lock::{self, StealPolicy},
    },
};

const PACKAGE: &str = "@dietrichgebert/ponytail";
const OWNER: &[u8] = b"xlink-mcode-ponytail-source-v1\n";

#[derive(Serialize, Deserialize)]
struct CacheRecord {
    version: String,
    digest: String,
}

#[derive(Serialize)]
pub(crate) struct UpdateStatus {
    pub installed_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub adapter_update_available: bool,
}

pub(super) fn semver_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn numeric(a: &str, b: &str) -> Ordering {
        a.len().cmp(&b.len()).then_with(|| a.cmp(b))
    }
    fn numeric_id(value: &str) -> bool {
        value.bytes().all(|byte| byte.is_ascii_digit())
    }
    let left = left.split('+').next().unwrap_or(left);
    let right = right.split('+').next().unwrap_or(right);
    let (left_core, left_pre) = left
        .split_once('-')
        .map_or((left, None), |(a, b)| (a, Some(b)));
    let (right_core, right_pre) = right
        .split_once('-')
        .map_or((right, None), |(a, b)| (a, Some(b)));
    for (a, b) in left_core.split('.').zip(right_core.split('.')) {
        let order = numeric(a, b);
        if order != Ordering::Equal {
            return order;
        }
    }
    match (left_pre, right_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let mut left = a.split('.');
            let mut right = b.split('.');
            loop {
                match (left.next(), right.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(a), Some(b)) => {
                        let order = match (numeric_id(a), numeric_id(b)) {
                            (true, true) => numeric(a, b),
                            (true, false) => Ordering::Less,
                            (false, true) => Ordering::Greater,
                            (false, false) => a.cmp(b),
                        };
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                }
            }
        }
    }
}

/// Explicit user-requested metadata check. Never changes the installed package or cache.
pub(crate) fn check_latest(
    instance_id: &str,
    source: &Path,
    progress: &mut dyn FnMut(&str),
) -> Result<UpdateStatus, String> {
    let installed = super::mcode_ponytail::active_version(instance_id)?;
    if !valid_version(&installed) {
        return Err("Ponytail 当前版本不可识别，请保留现有插件".into());
    }
    progress("正在检查 Ponytail 新版本…");
    let (_, document) = fetch::fetch_npm_doc(PACKAGE)
        .map_err(|error| format!("无法检查 Ponytail 更新：{error}"))?;
    let (latest, _) = fetch::resolve_npm_version(&document, None);
    if !valid_version(&latest) || !document.versions.contains_key(&latest) {
        return Err("Ponytail 最新版本信息无效，请稍后重试".into());
    }
    // A concurrent instance change or tampered package cannot turn this into a result for another target.
    if super::mcode_ponytail::active_version(instance_id)? != installed {
        return Err("Ponytail 状态已改变，请重新检查".into());
    }
    let adapter_update_available =
        super::mcode_ponytail::active_adapter_changed(instance_id, source)?;
    let order = semver_cmp(&latest, &installed);
    Ok(UpdateStatus {
        update_available: order.is_gt()
            || (order.is_eq() && latest == installed && adapter_update_available),
        adapter_update_available,
        installed_version: installed,
        latest_version: latest,
    })
}

pub(super) fn valid_version(value: &str) -> bool {
    if !version::is_valid_kernel_version(value) || paths::validate_id_component(value).is_err() {
        return false;
    }
    fn identifiers(value: &str, numeric_no_zero: bool) -> bool {
        !value.is_empty()
            && value.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
                    && !(numeric_no_zero
                        && part.len() > 1
                        && part.starts_with('0')
                        && part.bytes().all(|ch| ch.is_ascii_digit()))
            })
    }
    let mut with_build = value.split('+');
    let Some(main) = with_build.next() else {
        return false;
    };
    if let Some(build) = with_build.next() {
        if !identifiers(build, false) || with_build.next().is_some() {
            return false;
        }
    }
    let (core, pre) = main
        .split_once('-')
        .map_or((main, None), |(a, b)| (a, Some(b)));
    if pre.is_some_and(|part| !identifiers(part, true)) {
        return false;
    }
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|ch| ch.is_ascii_digit())
                && !(part.len() > 1 && part.starts_with('0'))
        })
}

fn ordinary_dir(path: &Path) -> Result<(), String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("无法读取目录 {}：{e}", path.display()))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(format!("Ponytail 来源目录不是普通目录：{}", path.display()));
    }
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => ordinary_dir(path),
        Err(e) => Err(format!(
            "无法创建 Ponytail 来源目录 {}：{e}",
            path.display()
        )),
    }
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return Err(format!("Ponytail 来源文件异常：{}", path.display()));
    }
    fs::read(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))
}

fn create_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("无法创建 Ponytail 来源记录 {}：{e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("无法保存 Ponytail 来源记录 {}：{e}", path.display()))
}

fn open_root(home: &Path) -> Result<PathBuf, String> {
    let root = home.join(".xlink-ponytail-source");
    if !root.exists() {
        let stage = home.join(format!(
            ".xlink-ponytail-source-init-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&stage).map_err(|e| format!("无法创建 Ponytail 来源暂存目录：{e}"))?;
        let result =
            create_file(&stage.join("owner"), OWNER).and_then(|()| {
                match publish_exclusive(&stage, &root) {
                    Ok(()) => Ok(()),
                    Err(_) if root.exists() => Ok(()),
                    Err(error) => Err(format!("无法发布 Ponytail 来源目录：{error}")),
                }
            });
        let cleanup = match fs::symlink_metadata(&stage) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("无法检查来源暂存目录：{error}")),
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
                fs::remove_dir_all(&stage).map_err(|e| format!("无法清理来源暂存目录：{e}"))
            }
            Ok(_) => Err(format!("来源暂存目录异常，已保留：{}", stage.display())),
        };
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => return Err(format!("{error}；{cleanup}")),
            (Err(error), _) => return Err(error),
            (Ok(()), Err(cleanup)) => return Err(cleanup),
            _ => {}
        }
    }
    ordinary_dir(&root)?;
    if read_regular(&root.join("owner"), 256)? != OWNER {
        return Err(format!(
            "Ponytail 来源归属无法确认，已保留目录：{}",
            root.display()
        ));
    }
    Ok(root)
}

fn publish_record(root: &Path, target: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = root.join(format!(".current-{:032x}.tmp", rand::random::<u128>()));
    create_file(&temporary, bytes)?;
    let result = publish_exclusive(&temporary, target)
        .map_err(|e| format!("无法发布 Ponytail 来源记录：{e}"));
    match fs::symlink_metadata(&temporary) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => result,
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            if let Err(e) = fs::remove_file(&temporary) {
                return Err(format!(
                    "{}；无法清理本次来源记录暂存：{e}",
                    result.err().unwrap_or_default()
                ));
            }
            result
        }
        Ok(_) => Err(format!("来源记录暂存异常，已保留：{}", temporary.display())),
        Err(e) => Err(format!("无法检查来源记录暂存：{e}")),
    }
}

fn checked_tree(root: &Path) -> Result<(), String> {
    fn visit(path: &Path, depth: usize, entries: &mut u64, bytes: &mut u64) -> Result<(), String> {
        if depth > 128 {
            return Err("Ponytail 来源目录层级过深".into());
        }
        *entries += 1;
        if *entries > archive::MAX_ARCHIVE_ENTRIES {
            return Err("Ponytail 来源文件过多".into());
        }
        let meta =
            fs::symlink_metadata(path).map_err(|e| format!("无法检查 Ponytail 来源：{e}"))?;
        if meta.file_type().is_symlink() {
            return Err("Ponytail 来源包含链接，已保留原内容".into());
        }
        if meta.is_file() {
            *bytes = bytes
                .checked_add(meta.len())
                .ok_or("Ponytail 来源大小无效")?;
            if *bytes > archive::MAX_UNPACKED_BYTES {
                return Err("Ponytail 来源过大".into());
            }
        } else if meta.is_dir() {
            for child in fs::read_dir(path).map_err(|e| e.to_string())? {
                visit(
                    &child.map_err(|e| e.to_string())?.path(),
                    depth + 1,
                    entries,
                    bytes,
                )?;
            }
        } else {
            return Err("Ponytail 来源包含特殊文件".into());
        }
        Ok(())
    }
    let mut entries = 0;
    let mut bytes = 0;
    visit(root, 0, &mut entries, &mut bytes)
}

fn validate_package(package: &Path, expected_version: &str) -> Result<String, String> {
    ordinary_dir(package)?;
    checked_tree(package)?;
    let raw = read_regular(&package.join("package.json"), 128 * 1024)?;
    let doc: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| format!("Ponytail 包清单损坏：{e}"))?;
    if doc["name"].as_str() != Some(PACKAGE) || doc["version"].as_str() != Some(expected_version) {
        return Err("Ponytail 包身份或版本与下载记录不一致".into());
    }
    manage::fingerprint_path(package).ok_or("无法校验 Ponytail 来源内容".into())
}

fn load_record(path: &Path) -> Result<CacheRecord, String> {
    let raw = read_regular(path, 4096)?;
    let record: CacheRecord =
        serde_json::from_slice(&raw).map_err(|e| format!("Ponytail 来源记录损坏：{e}"))?;
    if !valid_version(&record.version)
        || record.digest.len() != 64
        || !record.digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Ponytail 来源记录无效，已保留原内容".into());
    }
    Ok(record)
}

fn checked_cache(versions: &Path, wanted: &CacheRecord) -> Result<PathBuf, String> {
    if !valid_version(&wanted.version) {
        return Err("Ponytail 版本号无效".into());
    }
    let target = versions.join(&wanted.version);
    ordinary_dir(&target)?;
    let saved = load_record(&target.join("source.json"))?;
    if saved.version != wanted.version || saved.digest != wanted.digest {
        return Err("Ponytail 来源记录不一致，已保留原内容".into());
    }
    let package = target.join("package");
    if validate_package(&package, &wanted.version)? != wanted.digest {
        return Err("Ponytail 来源内容已改变，已保留原内容".into());
    }
    Ok(package)
}

/// The lease and source lock remain held throughout `use_source`, including copying.
pub(crate) fn with_source<T>(
    lease: &InstanceLease,
    progress: &mut dyn FnMut(&str),
    use_source: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let home = crate::skills::mcode::offline_home(lease).map_err(|e| e.to_string())?;
    let root = open_root(home)?;
    let _lock = store_lock::acquire(&root, StealPolicy::Fail)
        .map_err(|e| format!("无法锁定 MiniMax Ponytail 来源：{e}"))?;
    crate::skills::mcode::offline_home(lease).map_err(|e| e.to_string())?;
    let versions = root.join("versions");
    ensure_dir(&versions)?;
    let current = root.join("current.json");
    match fs::symlink_metadata(&current) {
        Ok(_) => {
            let saved = load_record(&current)?;
            let package = checked_cache(&versions, &saved)?;
            return use_source(&package);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("无法检查 Ponytail 来源记录：{e}")),
    }
    let stage = root.join(format!(".tmp-{:032x}", rand::random::<u128>()));
    fs::create_dir(&stage).map_err(|e| format!("无法创建 Ponytail 暂存目录：{e}"))?;
    let result = (|| {
        let package = stage.join("package");
        fs::create_dir(&package).map_err(|e| e.to_string())?;
        let version = fetch::fetch_npm_package(PACKAGE, None, &package, progress)?;
        if !valid_version(&version) {
            return Err("Ponytail 返回了无效版本号，已取消安装".into());
        }
        let digest = validate_package(&package, &version)?;
        let record = CacheRecord { version, digest };
        let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        create_file(&stage.join("source.json"), &bytes)?;
        let target = versions.join(&record.version);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                checked_cache(&versions, &record)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                publish_exclusive(&stage, &target)
                    .map_err(|e| format!("无法发布 Ponytail 来源：{e}；原有目录未覆盖"))?;
            }
            Err(e) => return Err(format!("无法检查 Ponytail 来源目录：{e}")),
        }
        let package = checked_cache(&versions, &record)?;
        publish_record(&root, &current, &bytes)?;
        crate::skills::mcode::offline_home(lease).map_err(|e| e.to_string())?;
        use_source(&package)
    })();
    let cleanup = match fs::symlink_metadata(&stage) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "无法检查 Ponytail 暂存目录 {}：{e}",
            stage.display()
        )),
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => Err(format!(
            "Ponytail 暂存目录异常，已保留：{}",
            stage.display()
        )),
        Ok(_) => fs::remove_dir_all(&stage)
            .map_err(|e| format!("无法清理 Ponytail 暂存目录 {}：{e}", stage.display())),
    };
    match (result, cleanup) {
        (result, Ok(())) => result,
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(original), Err(cleanup)) => Err(format!("{original}；{cleanup}")),
    }
}

/// Download a digest-verified candidate for an explicit upgrade without changing
/// the current source pointer; retain both old and newly published versions.
pub(crate) fn with_latest<T>(
    lease: &InstanceLease,
    progress: &mut dyn FnMut(&str),
    use_source: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    let root = open_root(home)?;
    let _lock = store_lock::acquire(&root, StealPolicy::Fail)
        .map_err(|error| format!("无法锁定 MiniMax Ponytail 来源：{error}"))?;
    let versions = root.join("versions");
    ensure_dir(&versions)?;
    let stage = root.join(format!(".tmp-{:032x}", rand::random::<u128>()));
    fs::create_dir(&stage).map_err(|error| format!("无法创建 Ponytail 升级来源暂存：{error}"))?;
    let result = (|| {
        let package = stage.join("package");
        fs::create_dir(&package).map_err(|error| error.to_string())?;
        let version = fetch::fetch_npm_package(PACKAGE, None, &package, progress)?;
        if !valid_version(&version) {
            return Err("Ponytail 返回了无效版本号，已取消升级".into());
        }
        let digest = validate_package(&package, &version)?;
        let record = CacheRecord { version, digest };
        create_file(
            &stage.join("source.json"),
            &serde_json::to_vec(&record).map_err(|error| error.to_string())?,
        )?;
        let target = versions.join(&record.version);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                checked_cache(&versions, &record)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                publish_exclusive(&stage, &target)
                    .map_err(|error| format!("无法发布 Ponytail 升级来源：{error}"))?;
            }
            Err(error) => return Err(format!("无法检查 Ponytail 升级来源：{error}")),
        }
        let package = checked_cache(&versions, &record)?;
        crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
        use_source(&package)
    })();
    let cleanup = match fs::symlink_metadata(&stage) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法检查 Ponytail 升级来源暂存：{error}")),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(&stage)
                .map_err(|error| format!("无法清理 Ponytail 升级来源暂存：{error}"))
        }
        Ok(_) => Err(format!(
            "Ponytail 升级来源暂存异常，已保留：{}",
            stage.display()
        )),
    };
    match (result, cleanup) {
        (result, Ok(())) => result,
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => Err(format!("{error}；{cleanup}")),
    }
}

#[cfg(windows)]
fn publish_exclusive(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(source.as_ptr(), target.as_ptr(), 0)
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn publish_exclusive(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let target = CString::new(target.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    if unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn publish_exclusive(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("当前平台不支持 Ponytail 来源发布"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_prereleases_and_ignores_build_metadata() {
        use std::cmp::Ordering;
        assert_eq!(semver_cmp("1.2.3+build.7", "1.2.3+other"), Ordering::Equal);
        assert_eq!(semver_cmp("1.2.3-rc.10", "1.2.3-rc.2"), Ordering::Greater);
        assert_eq!(semver_cmp("1.2.3-1", "1.2.3-alpha"), Ordering::Less);
        assert_eq!(semver_cmp("1.2.3", "1.2.3-rc.1"), Ordering::Greater);
        assert_eq!(
            semver_cmp("99999999999999999999.2.3", "2.2.3"),
            Ordering::Greater
        );
    }

    #[test]
    fn validates_version_before_using_it_as_a_path() {
        for good in ["1.2.3", "5.3.3", "1.0.0-rc.1", "1.0.0+build.7"] {
            assert!(valid_version(good), "{good}");
        }
        for bad in [
            "", "..", "../other", "C:evil", "1.2", "1.2.3.", "01.2.3", "1.0.0-01", "CON",
        ] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    #[test]
    fn cached_source_is_scoped_locked_and_rejects_changes() {
        let root = std::env::temp_dir().join(format!(
            "mcode-ponytail-source-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        crate::skills::mcode::enroll_fresh("mcode-test").unwrap();
        let home = paths::instance_dsh_home("mcode", "mcode-test");
        let lease = crate::skills::mcode::acquire_lease(&home).unwrap();
        let source_root = open_root(&home).unwrap();
        let versions = source_root.join("versions");
        fs::create_dir(&versions).unwrap();
        let target = versions.join("1.2.3");
        let package = target.join("package");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            br#"{"name":"@dietrichgebert/ponytail","version":"1.2.3"}"#,
        )
        .unwrap();
        fs::write(package.join("hook.js"), b"original").unwrap();
        let record = CacheRecord {
            version: "1.2.3".into(),
            digest: validate_package(&package, "1.2.3").unwrap(),
        };
        let bytes = serde_json::to_vec(&record).unwrap();
        create_file(&target.join("source.json"), &bytes).unwrap();
        create_file(&source_root.join("current.json"), &bytes).unwrap();
        with_source(&lease, &mut |_| {}, |found| {
            assert_eq!(
                found.canonicalize().unwrap(),
                package.canonicalize().unwrap()
            );
            assert!(store_lock::acquire_within(
                &source_root,
                StealPolicy::Fail,
                std::time::Duration::ZERO
            )
            .is_err());
            Ok(())
        })
        .unwrap();
        fs::write(package.join("hook.js"), b"user change").unwrap();
        assert!(with_source(&lease, &mut |_| {}, |_| Ok(())).is_err());
        assert_eq!(fs::read(package.join("hook.js")).unwrap(), b"user change");
        drop(lease);
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }
}
