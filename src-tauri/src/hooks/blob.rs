//! Hook 附件文件存储：临时文件、原子归档、孤儿/临时清理与磁盘余量。
//!
//! 文件路径只由服务端生成的 evt_*/att_* 组成，**从不使用发送方提供的文件名**
//! （filename 只是显示字段）。临时文件与正式文件同在 hooks/ 下，rename 不会
//! 跨文件系统。所有目录 Unix 0700、文件 0600。

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::shell::paths;
use crate::shell::settings;

/// 上传前至少保留的磁盘余量（除本次上传之外）。P2 固定值，避免把系统盘写满。
pub(crate) const MIN_FREE_BYTES: u64 = 512 * 1024 * 1024;

fn hooks_dir() -> PathBuf {
    paths::hooks_dir(settings::current_mode())
}

pub(crate) fn root() -> PathBuf {
    hooks_dir().join("blobs")
}

pub(crate) fn temp_dir() -> PathBuf {
    hooks_dir().join("tmp")
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("无法创建 Hook 数据目录 {}：{error}", path.display()))?;
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("无法读取 Hook 数据目录 {}：{error}", path.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("Hook 数据目录 {} 不是普通目录", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

pub(crate) fn ensure_dirs() -> Result<(), String> {
    private_dir(&root())?;
    private_dir(&temp_dir())
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 64
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn validate_components(event_id: &str, attachment_id: &str) -> Result<(), String> {
    if !valid_segment(event_id) || !valid_segment(attachment_id) {
        return Err("附件存储路径包含非法组件".into());
    }
    Ok(())
}

/// 建一个同文件系统的临时文件；调用方写入后交给 place。
pub(crate) fn create_temp() -> Result<(File, PathBuf), String> {
    ensure_dirs()?;
    for _ in 0..8 {
        let path = temp_dir().join(format!("upload-{:016x}.part", rand::random::<u64>()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("无法创建上传临时文件：{error}")),
        }
    }
    Err("无法创建上传临时文件（重名次数过多）".into())
}

/// 把校验通过的临时文件原子移动到正式位置，返回相对存储路径。
pub(crate) fn place(temp: &Path, event_id: &str, attachment_id: &str) -> Result<String, String> {
    validate_components(event_id, attachment_id)?;
    let dir = root().join(event_id);
    private_dir(&dir)?;
    // 每次尝试都有独立文件；过期租约失败后的清理不能删除新持有者的附件。
    let filename = format!("{attachment_id}-{:016x}", rand::random::<u64>());
    validate_components(event_id, &filename)?;
    let target = dir.join(&filename);
    // hard_link 原子发布且不覆盖既有文件，临时文件与正式目录位于同一文件系统。
    fs::hard_link(temp, &target).map_err(|error| format!("无法归档附件：{error}。请检查数据目录权限、磁盘空间及文件系统是否支持硬链接（如 NTFS/APFS）；不支持时请迁移 Hook 数据目录后重试"))?;
    let _ = fs::remove_file(temp);
    #[cfg(unix)]
    {
        if let Ok(directory) = File::open(&dir) {
            let _ = directory.sync_all();
        }
    }
    Ok(format!("{event_id}/{filename}"))
}

pub(crate) fn path_for_rel(rel: &str) -> Result<PathBuf, String> {
    let mut parts = rel.split('/');
    let event_id = parts.next().unwrap_or_default();
    let attachment_id = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return Err("附件存储路径层级非法".into());
    }
    validate_components(event_id, attachment_id)?;
    Ok(root().join(event_id).join(attachment_id))
}

pub(crate) fn exists_with_size(rel: &str, expected_size: i64) -> Result<bool, String> {
    let path = path_for_rel(rel)?;
    match fs::metadata(&path) {
        Ok(meta) => Ok(meta.is_file() && meta.len() == expected_size.max(0) as u64),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("无法核对附件 {}：{e}", path.display())),
    }
}

pub(crate) fn remove_rel(rel: &str) -> Result<(), String> {
    let path = path_for_rel(rel)?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法删除附件文件 {}：{error}", path.display())),
    }
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(())
}

/// 删除超过保留期的临时上传文件（半截上传的回收路径）。
pub(crate) fn cleanup_temp(older_than_ms: i64) -> usize {
    let Ok(entries) = fs::read_dir(temp_dir()) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if modified_ms(&path).is_some_and(|mtime| mtime <= older_than_ms)
            && fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

/// 列出正式存储下的文件：(相对路径, mtime ms)。不跟随符号链接。
pub(crate) fn list_files() -> Vec<(String, i64)> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(root()) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let Some(event_dir) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            let Ok(child_meta) = fs::symlink_metadata(&child_path) else {
                continue;
            };
            if !child_meta.is_file() {
                continue;
            }
            let Some(name) = child_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let mtime = modified_ms(&child_path).unwrap_or(0);
            files.push((format!("{event_dir}/{name}"), mtime));
        }
    }
    files
}

/// 删除数据库里没有登记、且超过宽限期的孤儿文件。返回删除数量。
pub(crate) fn remove_orphans(known: &HashSet<String>, older_than_ms: i64) -> usize {
    let mut removed = 0;
    for (rel, mtime) in list_files() {
        if known.contains(&rel) || mtime > older_than_ms {
            continue;
        }
        if remove_rel(&rel).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

#[cfg(test)]
static FREE_SPACE_OVERRIDE: std::sync::Mutex<Vec<(PathBuf, u64)>> =
    std::sync::Mutex::new(Vec::new());

/// 测试专用：按 hooks 根路径模拟剩余空间，避免为了打穿磁盘满去写满真实磁盘。
#[cfg(test)]
pub(crate) fn set_free_space_override(path: PathBuf, bytes: u64) {
    FREE_SPACE_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((path, bytes));
}

#[cfg(test)]
pub(crate) fn clear_free_space_override(path: &Path) {
    FREE_SPACE_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|(candidate, _)| candidate != path);
}

#[cfg(test)]
fn free_space_override(path: &Path) -> Option<u64> {
    FREE_SPACE_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .rev()
        .find(|(candidate, _)| candidate == path)
        .map(|(_, bytes)| *bytes)
}

/// 上传前的磁盘余量检查：至少保留 MIN_FREE_BYTES，且余量不低于本次大小的两倍。
pub(crate) fn disk_has_room(required: u64) -> Result<(), String> {
    // 目录可能还没建过；Windows 的 GetDiskFreeSpaceExW 要求路径存在。
    ensure_dirs()?;
    #[cfg(test)]
    let free = free_space_override(&root()).or_else(|| available_bytes(&root()));
    #[cfg(not(test))]
    let free = available_bytes(&root());
    let Some(free) = free else {
        return Err("无法读取磁盘剩余空间，已拒绝上传".into());
    };
    let needed = required.saturating_mul(2).max(MIN_FREE_BYTES);
    if free < needed {
        return Err(format!(
            "磁盘剩余空间不足（剩余 {} 字节，至少需要 {} 字节）",
            free, needed
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn available_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let raw = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(raw.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(windows)]
fn available_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut free = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_for_same_attachment_never_replace_each_others_files() {
        use std::io::Write;
        let home =
            std::env::temp_dir().join(format!("hook-blob-attempt-{:016x}", rand::random::<u64>()));
        let guard = crate::tests::scoped_xlink_home(&home);
        let (mut first, first_temp) = create_temp().unwrap();
        first.write_all(b"first").unwrap();
        drop(first);
        let first_rel = place(&first_temp, "evt_race", "att_race").unwrap();
        let (mut second, second_temp) = create_temp().unwrap();
        second.write_all(b"second").unwrap();
        drop(second);
        let second_rel = place(&second_temp, "evt_race", "att_race").unwrap();
        assert_ne!(first_rel, second_rel);
        assert_eq!(
            fs::read(path_for_rel(&first_rel).unwrap()).unwrap(),
            b"first"
        );
        remove_rel(&first_rel).unwrap();
        assert_eq!(
            fs::read(path_for_rel(&second_rel).unwrap()).unwrap(),
            b"second"
        );
        assert!(exists_with_size("../bad", 5).is_err());
        assert!(!exists_with_size(&first_rel, 5).unwrap());
        assert!(exists_with_size(&second_rel, 6).unwrap());
        drop(guard);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn path_components_reject_traversal() {
        assert!(path_for_rel("../secret").is_err());
        assert!(path_for_rel("evt_a/../secret").is_err());
        assert!(path_for_rel("evt_a/att_b/extra").is_err());
        assert!(path_for_rel("evt_a/att_b").is_ok());
    }

    #[test]
    fn segments_only_allow_generated_ids() {
        assert!(!valid_segment(""));
        assert!(!valid_segment("../x"));
        assert!(valid_segment("evt_0123"));
    }
}
