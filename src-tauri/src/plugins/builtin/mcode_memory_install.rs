//! Crash-recoverable installation of the shell's MiniMax memory hook.
//!
//! A marker inside the hook is trusted only together with an immutable receipt
//! outside it and a matching content digest. Pre-existing unmarked hooks are
//! never adopted or moved. Every publish is journaled before either rename.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::PLUGIN_FILES;

const MARKER: &str = ".xlink-owner";
const JOURNAL: &str = ".xlink-memory-transaction.json";
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Marker {
    version: u8,
    nonce: String,
    digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    version: u8,
    id: String,
    nonce: String,
    old_digest: Option<String>,
    new_digest: String,
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Marker {
    fn valid(&self) -> bool {
        self.version == 1 && is_hex(&self.nonce, 32) && is_hex(&self.digest, 64)
    }
}

impl Transaction {
    fn valid(&self) -> bool {
        self.version == 1
            && is_hex(&self.id, 32)
            && is_hex(&self.nonce, 32)
            && is_hex(&self.new_digest, 64)
            && self
                .old_digest
                .as_ref()
                .is_none_or(|d| is_hex(d, 64) && d != &self.new_digest)
    }
    fn stage(&self, home: &Path) -> PathBuf {
        home.join(format!(".xlink-memory-stage-{}", self.id))
    }
    fn backup(&self, home: &Path) -> PathBuf {
        home.join(format!(".xlink-memory-backup-{}", self.id))
    }
    fn marker(&self, digest: &str) -> Marker {
        Marker {
            version: 1,
            nonce: self.nonce.clone(),
            digest: digest.to_owned(),
        }
    }
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("无法检查记忆插件路径 {}：{e}", path.display())),
    }
}

fn read_small(path: &Path) -> Result<Vec<u8>, String> {
    let md = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !md.is_file() || md.file_type().is_symlink() || md.len() > 4096 {
        return Err("记忆插件归属记录不是普通文件或过大".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}

fn receipt(home: &Path, marker: &Marker) -> PathBuf {
    home.join(format!(
        ".xlink-memory-owned-{}-{}",
        marker.nonce, marker.digest
    ))
}

/// Hash paths, directory entries and file contents, excluding only our marker.
/// Bounding the walk also prevents untrusted existing trees exhausting memory.
fn tree(path: &Path) -> Result<(Vec<String>, String), String> {
    fn walk(
        root: &Path,
        path: &Path,
        depth: usize,
        total: &mut u64,
        parts: &mut Vec<(String, u8, String)>,
    ) -> Result<(), String> {
        if depth > 32 || parts.len() >= 2048 {
            return Err("记忆插件目录层级或文件数量过多".into());
        }
        let md = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if md.file_type().is_symlink() {
            return Err("记忆插件目录包含链接".into());
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if md.is_dir() {
            if depth > 0 {
                parts.push((rel, b'd', String::new()));
            }
            for item in fs::read_dir(path).map_err(|e| e.to_string())? {
                walk(
                    root,
                    &item.map_err(|e| e.to_string())?.path(),
                    depth + 1,
                    total,
                    parts,
                )?;
            }
        } else if md.is_file() {
            if md.len() > MAX_FILE {
                return Err("记忆插件文件过大".into());
            }
            *total = total.checked_add(md.len()).ok_or("记忆插件文件过大")?;
            if *total > MAX_TOTAL {
                return Err("记忆插件总大小过大".into());
            }
            if rel == MARKER {
                return Ok(());
            }
            let digest = crate::shell::digest::hex(
                &Sha256::digest(fs::read(path).map_err(|e| e.to_string())?)[..],
            );
            parts.push((rel, b'f', digest));
        } else {
            return Err("记忆插件目录包含特殊文件".into());
        }
        Ok(())
    }
    let mut parts = Vec::new();
    walk(path, path, 0, &mut 0, &mut parts)?;
    parts.sort();
    let mut digest = Sha256::new();
    let mut files = Vec::new();
    for (name, kind, hash) in parts {
        digest.update(name.as_bytes());
        digest.update([0, kind, 0]);
        digest.update(hash.as_bytes());
        digest.update([0]);
        if kind == b'f' {
            files.push(name);
        }
    }
    Ok((files, crate::shell::digest::hex(&digest.finalize()[..])))
}

fn verify_existing(
    manifest: &serde_json::Value,
    target: &Path,
    marked: bool,
) -> Result<(), String> {
    let (found, _) = tree(target)?;
    if exists(&target.join(MARKER))? != marked {
        return Err("记忆插件归属标记不一致".into());
    }
    let mut expected = PLUGIN_FILES.to_vec();
    expected.sort();
    if found.iter().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err("已有同名记忆插件内容不属于本应用".into());
    }
    let entries = manifest
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or("记忆插件资源清单缺少文件")?;
    for name in PLUGIN_FILES {
        let expected = entries
            .iter()
            .find(|e| e.get("path").and_then(|v| v.as_str()) == Some(*name))
            .and_then(|e| e.get("sha256"))
            .and_then(|v| v.as_str())
            .ok_or("记忆插件摘要缺失")?;
        if crate::shell::digest::hex(
            &Sha256::digest(fs::read(target.join(name)).map_err(|e| e.to_string())?)[..],
        ) != expected
        {
            return Err("已有记忆插件内容与本版本不同".into());
        }
    }
    Ok(())
}

fn verify_owned(home: &Path, path: &Path) -> Result<Marker, String> {
    let raw = read_small(&path.join(MARKER))?;
    let marker: Marker = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if !marker.valid() || read_small(&receipt(home, &marker))? != raw {
        return Err("记忆插件归属凭据不匹配".into());
    }
    let (_, digest) = tree(path)?;
    if digest != marker.digest {
        return Err("记忆插件内容已修改".into());
    }
    Ok(marker)
}

fn expect_owned(home: &Path, path: &Path, marker: &Marker) -> Result<(), String> {
    if verify_owned(home, path)? != *marker {
        return Err("记忆插件归属或内容不匹配".into());
    }
    Ok(())
}

/// Publish a fully written small file without replacing any existing path.
fn create_record(home: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = home.join(format!(
        ".xlink-memory-record-{:032x}",
        rand::random::<u128>()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    match publish_exclusive(&temp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&temp);
            Err(e.to_string())
        }
    }
}

fn preserve_error(home: &Path, detail: impl std::fmt::Display) -> String {
    format!(
        "记忆插件安装或恢复未完成：{detail}。原内容和恢复文件保留在 {}，请检查插件目录及备份后重试",
        home.display()
    )
}

/// Complete a previously journaled transaction without trusting source resources.
fn recover(home: &Path) -> Result<(), String> {
    let journal_path = home.join(JOURNAL);
    if !exists(&journal_path)? {
        return Ok(());
    }
    let raw = read_small(&journal_path).map_err(|e| preserve_error(home, e))?;
    let tx: Transaction = serde_json::from_slice(&raw).map_err(|e| preserve_error(home, e))?;
    if !tx.valid() {
        return Err(preserve_error(home, "恢复记录无效"));
    }
    let target = home.join("plugins/xlink-memory");
    let stage = tx.stage(home);
    let backup = tx.backup(home);
    let new = tx.marker(&tx.new_digest);
    let old = tx.old_digest.as_ref().map(|d| tx.marker(d));
    let result = (|| -> Result<(), String> {
        let (target_exists, stage_exists, backup_exists) =
            (exists(&target)?, exists(&stage)?, exists(&backup)?);
        match (&old, target_exists, stage_exists, backup_exists) {
            (Some(old), true, true, false) => {
                expect_owned(home, &target, old)?;
                expect_owned(home, &stage, &new)?;
                publish_exclusive(&target, &backup).map_err(|e| e.to_string())?;
            }
            (Some(_), false, true, true) | (Some(_), true, false, true) => {}
            (None, false, true, false) | (None, true, false, false) => {}
            _ => return Err("安装路径出现冲突，未更改任何已有内容".into()),
        }
        if let Some(old) = &old {
            expect_owned(home, &backup, old)?;
        }
        if exists(&stage)? {
            expect_owned(home, &stage, &new)?;
            publish_exclusive(&stage, &target).map_err(|e| e.to_string())?;
        }
        expect_owned(home, &target, &new)?;
        if read_small(&journal_path)? != raw {
            return Err("恢复记录已被修改".into());
        }
        fs::remove_file(&journal_path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    result.map_err(|e| {
        let preserved = [&target, &stage, &backup]
            .into_iter()
            .filter(|path| exists(path).unwrap_or(true))
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("、");
        format!(
            "{}；保留路径：{preserved}。请检查现有插件及备份，解决冲突后重试",
            preserve_error(home, e)
        )
    })
}

pub(super) fn install_plugin(source: &Path, instance_id: &str) -> Result<(), String> {
    let home = crate::shell::paths::instance_dsh_home(
        crate::shell::instance::KERNEL_FAMILY_MCODE,
        instance_id,
    );
    let md = fs::symlink_metadata(&home).map_err(|e| format!("MiniMax 实例目录不可用：{e}"))?;
    if !md.is_dir() || md.file_type().is_symlink() {
        return Err("MiniMax 实例目录不是普通目录".into());
    }
    let home = fs::canonicalize(home).map_err(|e| e.to_string())?;
    let plugins = home.join("plugins");
    match fs::create_dir(&plugins) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("无法准备 MiniMax 插件目录：{e}")),
    }
    let md = fs::symlink_metadata(&plugins).map_err(|e| e.to_string())?;
    if !md.is_dir() || md.file_type().is_symlink() {
        return Err("MiniMax 插件目录不是普通目录".into());
    }
    recover(&home)?;
    super::super::materialize::verify_manifest(source)?;
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(source.join(super::super::materialize::MANIFEST_FILE))
            .map_err(|e| format!("记忆插件资源清单不可用：{e}；请重新构建或安装"))?,
    )
    .map_err(|e| format!("记忆插件资源清单损坏：{e}"))?;
    let files = manifest
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or("记忆插件资源清单缺少文件")?;
    for name in PLUGIN_FILES {
        if !files
            .iter()
            .any(|e| e.get("path").and_then(|v| v.as_str()) == Some(*name))
        {
            return Err(format!("记忆插件资源清单缺少 {name}；请重新构建或安装"));
        }
        let mut path = source.to_path_buf();
        for component in name.split('/') {
            path.push(component);
            let md =
                fs::symlink_metadata(&path).map_err(|e| format!("读取记忆插件资源失败：{e}"))?;
            if md.file_type().is_symlink() {
                return Err("记忆插件资源包含链接，请重新安装".into());
            }
        }
    }
    let target = plugins.join("xlink-memory");
    let old = if exists(&target)? {
        if !exists(&target.join(MARKER))? {
            return verify_existing(&manifest, &target, false)
                .map_err(|e| preserve_error(&home, e));
        }
        let marker = verify_owned(&home, &target).map_err(|e| preserve_error(&home, e))?;
        if verify_existing(&manifest, &target, true).is_ok() {
            return Ok(());
        }
        Some(marker)
    } else {
        None
    };
    let mut tx = Transaction {
        version: 1,
        id: format!("{:032x}", rand::random::<u128>()),
        nonce: old
            .as_ref()
            .map(|m| m.nonce.clone())
            .unwrap_or_else(|| format!("{:032x}", rand::random::<u128>())),
        old_digest: old.as_ref().map(|m| m.digest.clone()),
        new_digest: String::new(),
    };
    let stage = tx.stage(&home);
    fs::create_dir(&stage).map_err(|e| preserve_error(&home, e))?;
    let result = (|| -> Result<(), String> {
        for name in PLUGIN_FILES {
            let to = stage.join(name);
            fs::create_dir_all(to.parent().ok_or("记忆插件路径无效")?)
                .map_err(|e| e.to_string())?;
            fs::copy(source.join(name), to).map_err(|e| format!("复制记忆插件失败：{e}"))?;
        }
        verify_existing(&manifest, &stage, false)?;
        tx.new_digest = tree(&stage)?.1;
        if !tx.valid() {
            return Err("记忆插件内容摘要无效".into());
        }
        let marker = tx.marker(&tx.new_digest);
        let raw = serde_json::to_vec(&marker).map_err(|e| e.to_string())?;
        fs::write(stage.join(MARKER), &raw).map_err(|e| e.to_string())?;
        let receipt_path = receipt(&home, &marker);
        if exists(&receipt_path)? {
            if read_small(&receipt_path)? != raw {
                return Err("记忆插件归属凭据冲突".into());
            }
        } else {
            create_record(&home, &receipt_path, &raw)?;
        }
        expect_owned(&home, &stage, &marker)?;
        let journal = serde_json::to_vec(&tx).map_err(|e| e.to_string())?;
        create_record(&home, &home.join(JOURNAL), &journal)?;
        Ok(())
    })();
    result.map_err(|e| preserve_error(&home, e))?;
    recover(&home)
}

#[cfg(windows)]
fn publish_exclusive(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(from.as_ptr(), to.as_ptr(), 0)
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn publish_exclusive(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(source.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let to = CString::new(target.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn publish_exclusive(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("当前平台不支持安全发布 MiniMax 插件"))
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        source: PathBuf,
        guard: Option<crate::tests::EnvGuard>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mcode-memory-upgrade-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&root).unwrap();
            let guard = crate::tests::scoped_xlink_home(&root);
            let home = crate::shell::paths::instance_dsh_home("mcode", "fixture");
            #[cfg(windows)]
            crate::skills::mcode::enroll_fresh("fixture").unwrap();
            #[cfg(not(windows))]
            fs::create_dir_all(&home).unwrap();
            fs::create_dir(home.join("plugins")).unwrap();
            let source = root.join("source");
            fs::create_dir(&source).unwrap();
            let fixture = Self {
                root,
                home,
                source,
                guard: Some(guard),
            };
            fixture.write_source("v1");
            fixture
        }
        fn write_source(&self, revision: &str) {
            let mut files = Vec::new();
            for name in PLUGIN_FILES {
                let path = self.source.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                let body = format!("{revision}:{name}");
                fs::write(path, body.as_bytes()).unwrap();
                files.push(serde_json::json!({"path":name,"bytes":body.len(),"sha256":crate::shell::digest::hex(&Sha256::digest(body.as_bytes())[..])}));
            }
            fs::write(
                self.source
                    .join(super::super::super::materialize::MANIFEST_FILE),
                serde_json::to_vec(&serde_json::json!({"files":files})).unwrap(),
            )
            .unwrap();
        }
        fn install(&self) -> Result<(), String> {
            install_plugin(&self.source, "fixture")
        }
        fn target(&self) -> PathBuf {
            self.home.join("plugins/xlink-memory")
        }
        fn copy_source(&self, to: &Path) {
            copy_source(&self.source, to);
        }
        fn pending(&self, old: Option<&Marker>) -> Transaction {
            pending(&self.home, &self.source, old)
        }
    }

    fn copy_source(source: &Path, to: &Path) {
        fs::create_dir(to).unwrap();
        for name in PLUGIN_FILES {
            let path = to.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy(source.join(name), path).unwrap();
        }
    }

    fn pending(home: &Path, source: &Path, old: Option<&Marker>) -> Transaction {
        let mut tx = Transaction {
            version: 1,
            id: format!("{:032x}", rand::random::<u128>()),
            nonce: old
                .map(|m| m.nonce.clone())
                .unwrap_or_else(|| format!("{:032x}", rand::random::<u128>())),
            old_digest: old.map(|m| m.digest.clone()),
            new_digest: String::new(),
        };
        let stage = tx.stage(home);
        copy_source(source, &stage);
        tx.new_digest = tree(&stage).unwrap().1;
        assert!(tx.valid());
        let marker = tx.marker(&tx.new_digest);
        let raw = serde_json::to_vec(&marker).unwrap();
        fs::write(stage.join(MARKER), &raw).unwrap();
        create_record(home, &receipt(home, &marker), &raw).unwrap();
        create_record(home, &home.join(JOURNAL), &serde_json::to_vec(&tx).unwrap()).unwrap();
        tx
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            drop(self.guard.take());
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn installs_idempotently_and_upgrades_only_unchanged_owned_content() {
        let f = Fixture::new();
        f.install().unwrap();
        let old = verify_owned(&f.home, &f.target()).unwrap();
        f.install().unwrap();
        assert_eq!(verify_owned(&f.home, &f.target()).unwrap(), old);
        f.write_source("v2");
        f.install().unwrap();
        let new = verify_owned(&f.home, &f.target()).unwrap();
        assert_eq!(new.nonce, old.nonce);
        assert_ne!(new.digest, old.digest);
        let backups: Vec<_> = fs::read_dir(&f.home)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".xlink-memory-backup-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(verify_owned(&f.home, &backups[0]).unwrap(), old);
        assert!(!f.home.join(JOURNAL).exists());
        fs::write(f.target().join("icon.png"), b"user modification").unwrap();
        f.write_source("v3");
        let error = f.install().unwrap_err();
        assert!(error.contains("已修改"), "{error}");
        assert_eq!(
            fs::read(f.target().join("icon.png")).unwrap(),
            b"user modification"
        );
    }

    #[test]
    fn unowned_existing_tree_is_never_claimed_or_overwritten() {
        let f = Fixture::new();
        f.copy_source(&f.target());
        f.install().unwrap();
        assert!(!f.target().join(MARKER).exists());
        f.write_source("v2");
        let error = f.install().unwrap_err();
        assert!(error.contains("保留"), "{error}");
        assert_eq!(
            fs::read(f.target().join("icon.png")).unwrap(),
            b"v1:icon.png"
        );
        assert!(!f.target().join(MARKER).exists());
        assert!(!f.home.join(JOURNAL).exists());
    }

    #[test]
    fn orphan_stage_is_preserved_and_does_not_block_fresh_install() {
        let f = Fixture::new();
        let orphan = f
            .home
            .join(".xlink-memory-stage-00000000000000000000000000000000");
        fs::create_dir(&orphan).unwrap();
        fs::write(orphan.join("user.txt"), b"keep").unwrap();
        f.install().unwrap();
        verify_owned(&f.home, &f.target()).unwrap();
        assert_eq!(fs::read(orphan.join("user.txt")).unwrap(), b"keep");
    }

    #[test]
    fn changed_ownership_receipt_never_authorizes_an_upgrade() {
        let f = Fixture::new();
        f.install().unwrap();
        let old = verify_owned(&f.home, &f.target()).unwrap();
        fs::write(receipt(&f.home, &old), b"user content").unwrap();
        f.write_source("v2");
        assert!(f.install().is_err());
        assert_eq!(
            fs::read(f.target().join("icon.png")).unwrap(),
            b"v1:icon.png"
        );
        assert_eq!(fs::read(receipt(&f.home, &old)).unwrap(), b"user content");
        assert!(!f.home.join(JOURNAL).exists());
    }

    #[test]
    fn recovers_each_published_upgrade_and_first_install_interruption() {
        for point in 0..5 {
            let f = Fixture::new();
            let is_upgrade = point < 3;
            let old = if is_upgrade {
                f.install().unwrap();
                let old = verify_owned(&f.home, &f.target()).unwrap();
                f.write_source("v2");
                Some(old)
            } else {
                None
            };
            let tx = f.pending(old.as_ref());
            if point == 1 || point == 2 {
                publish_exclusive(&f.target(), &tx.backup(&f.home)).unwrap();
            }
            if point == 2 || point == 4 {
                publish_exclusive(&tx.stage(&f.home), &f.target()).unwrap();
            }
            recover(&f.home).unwrap();
            assert_eq!(
                verify_owned(&f.home, &f.target()).unwrap(),
                tx.marker(&tx.new_digest)
            );
            if let Some(old) = old {
                assert_eq!(verify_owned(&f.home, &tx.backup(&f.home)).unwrap(), old);
            } else {
                assert!(!tx.backup(&f.home).exists());
            }
            assert!(!f.home.join(JOURNAL).exists());
            assert!(!tx.stage(&f.home).exists());
            recover(&f.home).unwrap();
        }
    }

    #[test]
    fn refuses_interrupted_conflict_and_preserves_old_and_new_trees() {
        let f = Fixture::new();
        f.install().unwrap();
        let old = verify_owned(&f.home, &f.target()).unwrap();
        f.write_source("v2");
        let tx = f.pending(Some(&old));
        publish_exclusive(&f.target(), &tx.backup(&f.home)).unwrap();
        fs::write(
            tx.stage(&f.home).join("icon.png"),
            b"modified while interrupted",
        )
        .unwrap();
        let error = recover(&f.home).unwrap_err();
        assert!(error.contains("保留"), "{error}");
        assert!(!f.target().exists());
        assert_eq!(verify_owned(&f.home, &tx.backup(&f.home)).unwrap(), old);
        assert_eq!(
            fs::read(tx.stage(&f.home).join("icon.png")).unwrap(),
            b"modified while interrupted"
        );
        assert!(f.home.join(JOURNAL).exists());
    }

    #[test]
    fn refuses_fresh_install_collision_without_touching_user_files() {
        let f = Fixture::new();
        let tx = f.pending(None);
        fs::create_dir(f.target()).unwrap();
        fs::write(f.target().join("user.txt"), b"user file").unwrap();
        assert!(recover(&f.home).is_err());
        assert_eq!(fs::read(f.target().join("user.txt")).unwrap(), b"user file");
        assert_eq!(
            verify_owned(&f.home, &tx.stage(&f.home)).unwrap(),
            tx.marker(&tx.new_digest)
        );
        assert!(f.home.join(JOURNAL).exists());
    }

    #[cfg(windows)]
    #[test]
    fn production_prepare_recovers_only_with_opt_in_and_offline_lease() {
        let f = Fixture::new();
        f.install().unwrap();
        let old = verify_owned(&f.home, &f.target()).unwrap();
        f.write_source("v2");
        let tx = f.pending(Some(&old));
        publish_exclusive(&f.target(), &tx.backup(&f.home)).unwrap();
        let lease = crate::skills::mcode::acquire_lease(&f.home).unwrap();
        assert!(super::super::prepare(&f.source, &lease, false)
            .unwrap()
            .is_none());
        assert!(f.home.join(JOURNAL).exists());
        let mut bridge = super::super::prepare(&f.source, &lease, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            verify_owned(&f.home, &f.target()).unwrap(),
            tx.marker(&tx.new_digest)
        );
        assert!(!f.home.join(JOURNAL).exists());
        bridge.stop().unwrap();
        drop(bridge);
        let token = crate::skills::mcode::arm_start(&lease, 1).unwrap();
        assert!(super::super::prepare(&f.source, &lease, true).is_err());
        crate::skills::mcode::finish_start(
            &lease,
            &token,
            crate::skills::mcode::StopEvidence::NeverSpawned,
        )
        .unwrap();
        drop(lease);
    }

    #[cfg(windows)]
    #[test]
    fn lease_crash_recovers_memory_transaction_through_production_prepare() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        const CHILD_ROOT: &str = "DSH_XLINK_MCODE_MEMORY_CRASH_TEST_CHILD";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let _guard = crate::tests::scoped_xlink_home(&root);
            let home = crate::shell::paths::instance_dsh_home("mcode", "fixture");
            let lease = crate::skills::mcode::acquire_lease(&home).unwrap();
            assert!(crate::skills::mcode::offline_home(&lease).is_ok());
            let target = home.join("plugins/xlink-memory");
            let old = verify_owned(&home, &target).unwrap();
            let tx = pending(&home, &root.join("source"), Some(&old));
            publish_exclusive(&target, &tx.backup(&home)).unwrap();
            fs::write(
                root.join("transaction.json"),
                serde_json::to_vec(&tx).unwrap(),
            )
            .unwrap();
            fs::write(root.join("ready"), b"ready").unwrap();
            loop {
                std::thread::park();
            }
        }

        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let f = Fixture::new();
        let lease = crate::skills::mcode::acquire_lease(&f.home).unwrap();
        let mut bridge = super::super::prepare(&f.source, &lease, true)
            .unwrap()
            .unwrap();
        bridge.stop().unwrap();
        drop(bridge);
        drop(lease);
        let old = verify_owned(&f.home, &f.target()).unwrap();
        f.write_source("v2");

        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "plugins::builtin::mcode_memory::mcode_memory_install::tests::lease_crash_recovers_memory_transaction_through_production_prepare",
            ])
            .env(CHILD_ROOT, &f.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = ChildGuard(crate::shell::process::quiet(&mut command).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(15);
        while !f.root.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before recording transaction"
            );
            assert!(
                Instant::now() < deadline,
                "child did not record transaction"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let tx: Transaction =
            serde_json::from_slice(&fs::read(f.root.join("transaction.json")).unwrap()).unwrap();
        assert!(crate::skills::mcode::acquire_lease(&f.home).is_err());
        assert!(!f.target().exists());
        child.0.kill().unwrap();
        child.0.wait().unwrap();

        let lease = crate::skills::mcode::acquire_lease(&f.home).unwrap();
        let mut bridge = super::super::prepare(&f.source, &lease, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            verify_owned(&f.home, &f.target()).unwrap(),
            tx.marker(&tx.new_digest)
        );
        assert_eq!(verify_owned(&f.home, &tx.backup(&f.home)).unwrap(), old);
        assert!(!f.home.join(JOURNAL).exists());
        assert!(!tx.stage(&f.home).exists());
        bridge.stop().unwrap();
        drop(bridge);
        drop(lease);
        drop(child);
    }
}
