//! MiniMax skill view. Each replacement is journaled before publishing.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use crate::{
    pkg::fetch,
    shell::{atomic_write::rename_exclusive, error::AppError, paths},
    skills::{
        manage,
        store_lock::{self, StealPolicy},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    entries: BTreeMap<String, Entry>,
}

#[derive(Default, Serialize, Deserialize)]
struct Entry {
    fingerprint: Option<String>,
    pending: Option<Pending>,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    old: Option<String>,
    new: Option<String>,
    nonce: String,
}

fn io_error(error: io::Error) -> AppError {
    AppError::Io(error.to_string())
}

fn directory(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(md) if md.is_dir() && !md.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(AppError::Skill(format!(
            "技能目录不是普通目录：{}",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(io_error)
        }
        Err(error) => Err(io_error(error)),
    }
}

fn regular_digest(path: &Path) -> Result<Option<String>, AppError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
        Ok(md) if md.is_dir() && !md.file_type().is_symlink() => {
            reject_links(path)?;
            manage::fingerprint_path(path)
                .map(Some)
                .ok_or_else(|| AppError::Skill(format!("无法校验技能目录：{}", path.display())))
        }
        Ok(_) => Err(AppError::Skill(format!(
            "技能目标被非目录占用：{}",
            path.display()
        ))),
    }
}

fn reject_links(root: &Path) -> Result<(), AppError> {
    for child in fs::read_dir(root).map_err(io_error)? {
        let child = child.map_err(io_error)?;
        let path = child.path();
        let kind = fs::symlink_metadata(&path).map_err(io_error)?.file_type();
        if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
            return Err(AppError::Skill(format!(
                "技能目录中出现链接或特殊文件：{}",
                path.display()
            )));
        }
        if kind.is_dir() {
            reject_links(&path)?;
        }
    }
    Ok(())
}

fn checked_source(store_root: &Path, id: &str, relative: &str) -> Result<PathBuf, AppError> {
    let store = fs::canonicalize(store_root).map_err(io_error)?;
    let package = store.join(id);
    let metadata = fs::symlink_metadata(&package).map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::Skill("技能包目录不是普通目录".into()));
    }
    let package = fs::canonicalize(package).map_err(io_error)?;
    if package.parent() != Some(store.as_path()) {
        return Err(AppError::Skill("技能包不在中央库内".into()));
    }
    let mut source = package.clone();
    for component in Path::new(relative).components() {
        let Component::Normal(part) = component else {
            return Err(AppError::Skill("技能路径无效".into()));
        };
        source.push(part);
        let metadata = fs::symlink_metadata(&source).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            return Err(AppError::Skill("技能路径包含链接".into()));
        }
    }
    let source = fs::canonicalize(source).map_err(io_error)?;
    if !source.starts_with(&package) {
        return Err(AppError::Skill("技能路径不在技能包内".into()));
    }
    Ok(source)
}

fn snapshots(work: &Path) -> Result<Vec<(u64, std::path::PathBuf)>, AppError> {
    let mut result = Vec::new();
    for child in fs::read_dir(work).map_err(io_error)? {
        let child = child.map_err(io_error)?;
        let name = child.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(number) = name
            .strip_prefix("manifest-")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        if number.len() != 20 || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AppError::Skill("技能清单快照名称无效".into()));
        }
        let sequence = number
            .parse()
            .map_err(|_| AppError::Skill("技能清单快照序号无效".into()))?;
        if !child.file_type().map_err(io_error)?.is_file() {
            return Err(AppError::Skill("技能清单快照不是普通文件".into()));
        }
        result.push((sequence, child.path()));
    }
    result.sort_by_key(|item| item.0);
    Ok(result)
}

fn load(work: &Path) -> Result<Manifest, AppError> {
    let snapshots = snapshots(work)?;
    let Some((_, latest)) = snapshots.last() else {
        return Ok(Manifest::default());
    };
    let bytes = fs::read(latest).map_err(io_error)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Skill(format!("MiniMax 技能清单损坏：{error}")))
}

fn save(work: &Path, manifest: &Manifest) -> Result<(), AppError> {
    let old = snapshots(work)?;
    let next = old
        .last()
        .map_or(Some(0), |item| item.0.checked_add(1))
        .ok_or_else(|| AppError::Skill("技能清单序号已耗尽".into()))?;
    let final_path = work.join(format!("manifest-{next:020}.json"));
    let temp = work.join(format!(".manifest-{:032x}.tmp", rand::random::<u128>()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(io_error)?;
    let mut content =
        serde_json::to_vec(manifest).map_err(|error| AppError::Skill(error.to_string()))?;
    content.push(b'\n');
    file.write_all(&content).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    rename_exclusive(&temp, &final_path).map_err(io_error)?;
    #[cfg(unix)]
    fs::File::open(work)
        .and_then(|dir| dir.sync_all())
        .map_err(io_error)?;
    // shortcut: retain old snapshots until ownership-aware cleanup is available.
    Ok(())
}

fn finish(name: &str, home: &Path, manifest: &mut Manifest) -> Result<(), AppError> {
    let entry = manifest
        .entries
        .get(name)
        .ok_or_else(|| AppError::Skill("技能事务记录缺失".into()))?;
    let pending = entry
        .pending
        .as_ref()
        .ok_or_else(|| AppError::Skill("技能事务缺失".into()))?;
    if pending.nonce.len() != 32 || !pending.nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::Skill("技能事务标识无效".into()));
    }
    let work = home.join(".xlink-skill-work");
    let target = home.join("skills").join(name);
    let stage = work.join(format!("new-{}", pending.nonce));
    let backup = work.join(format!("old-{}", pending.nonce));
    let mut current = regular_digest(&target)?;
    let mut saved = regular_digest(&backup)?;
    if current.is_some() && current == pending.new && regular_digest(&stage)?.is_some() {
        return Err(AppError::Skill(format!(
            "技能目标在发布前被占用：{}",
            target.display()
        )));
    }
    if saved.is_some() && saved != pending.old {
        return Err(AppError::Skill(format!(
            "技能备份被修改，保留现场：{}",
            backup.display()
        )));
    }
    if current == pending.old && current.is_some() && saved.is_none() && current != pending.new {
        rename_exclusive(&target, &backup).map_err(io_error)?;
        saved = regular_digest(&backup)?;
        if saved != pending.old {
            let _ = rename_exclusive(&backup, &target);
            return Err(AppError::Skill(format!(
                "技能在同步期间被修改：{}",
                target.display()
            )));
        }
        current = None;
    }
    if current.is_none() && pending.old.is_some() && saved.is_none() {
        return Err(AppError::Skill(format!(
            "技能事务的原件缺失：{}",
            target.display()
        )));
    }
    if current.is_none() && pending.new.is_some() {
        if regular_digest(&stage)? != pending.new {
            return Err(AppError::Skill(format!(
                "技能待发布内容缺失或已修改：{}",
                stage.display()
            )));
        }
        rename_exclusive(&stage, &target).map_err(io_error)?;
        current = regular_digest(&target)?;
    }
    if current != pending.new {
        return Err(AppError::Skill(format!(
            "技能目标被其他内容占用：{}",
            target.display()
        )));
    }
    // Keep the original even after publication: another program may still
    // have an open handle to it and write after the digest was checked.
    // Reclamation requires a separate, explicit recovery/cleanup design.
    if saved.is_some() && regular_digest(&backup)? != pending.old {
        return Err(AppError::Skill("技能备份已被修改".into()));
    }
    let entry = manifest.entries.get_mut(name).expect("entry checked above");
    entry.fingerprint = entry.pending.take().expect("pending checked above").new;
    if entry.fingerprint.is_none() {
        manifest.entries.remove(name);
    }
    save(&home.join(".xlink-skill-work"), manifest)
}

/// 锁句柄。Windows 上就是一个独占句柄：关闭即删除锁文件。
#[cfg(windows)]
type InstanceLock = fs::File;

/// Unix 上持锁的句柄：Drop 时删掉自己那一份锁文件。
///
/// `flock` 由内核在进程退出时放掉，但锁文件不会自己消失——留着会让下一个持有者
/// 以为锁还在。只删内容是自己 pid 的那一份：别人的锁不碰。
#[cfg(unix)]
struct InstanceLock {
    _file: fs::File,
    path: PathBuf,
}

#[cfg(unix)]
impl Drop for InstanceLock {
    fn drop(&mut self) {
        let mine = fs::read_to_string(&self.path)
            .map(|text| text.trim() == std::process::id().to_string())
            .unwrap_or(false);
        if mine {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The instance must be enrolled while known never to have run:
/// `offline-v1` marks that enrollment. Before spawn the lifecycle must create
/// and flush `running` while holding this same lease, and remove it only after
/// the entire process tree has definitely exited. A crash leaves it in place.
/// Existing instances cannot be silently enrolled merely because a PID file
/// is absent. Recovery must establish that no old process remains first.
pub(crate) struct InstanceLease {
    home: PathBuf,
    // This handle owns the new-protocol lock. Closing it (including on process
    // termination) lets Windows remove the lock file and lets the kernel drop
    // the Unix flock. Never take over a file that belongs to someone else: it
    // may be an older shell or an unknown owner.
    _lock: InstanceLock,
}

#[cfg(windows)]
fn acquire_instance_lock(lease_dir: &Path) -> Result<fs::File, AppError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};

    let path = lease_dir.join(store_lock::LOCK_FILE);
    let file = match open_instance_lock(&path) {
        Ok(file) => file,
        Err(error) => {
            // 旧协议的锁是「普通文件 + 持有者 pid」：持有者崩了它就留在盘上，
            // 新协议从此永远拿不到租约，MiniMax 起不来。只有在**持有者确实已经
            // 死了**时才回收它——判据与技能中央库抢僵尸锁同一条（owner_is_dead：
            // 读不出 pid、持有者还活着，都按活着算，绝不凭「锁文件在」就删）。
            if !store_lock::owner_is_dead(&path) {
                return Err(AppError::Skill(format!(
                    "无法获取 MiniMax 实例生命周期锁 {}：{error}；请确认没有其他壳在管理该实例，并检查已有锁后重试，原有文件不会被移除",
                    path.display()
                )));
            }
            let _ = fs::remove_file(&path);
            crate::shell::shell_events::record(
                "mcode-lifecycle",
                &format!("已回收已失效的 MiniMax 生命周期锁：{}", path.display()),
            );
            open_instance_lock(&path).map_err(|retry| {
                AppError::Skill(format!(
                    "无法获取 MiniMax 实例生命周期锁 {}：{retry}；已回收的旧锁又被占用，请检查后重试",
                    path.display()
                ))
            })?
        }
    };
    // SAFETY: file owns a live Windows handle; disabling inheritance cannot
    // invalidate it. On failure the owned handle closes and removes only the
    // file this call just created.
    if unsafe { SetHandleInformation(file.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(AppError::Skill(format!(
            "无法保证 MiniMax 生命周期锁不被子进程继承：{}；请重试",
            io::Error::last_os_error()
        )));
    }
    Ok(file)
}

/// 新协议的锁：独占创建、句柄不继承、关闭即删除。
#[cfg(windows)]
fn open_instance_lock(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::GENERIC_WRITE,
        Storage::FileSystem::{DELETE, FILE_FLAG_DELETE_ON_CLOSE},
    };

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_WRITE | DELETE)
        .share_mode(0)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(path)
}

/// Unix 上的实例生命周期锁：`flock` 持互斥，进程崩了内核自动放锁。
///
/// 与 Windows 的差别只有两处：锁文件不会随句柄关闭而消失（所以释放时要自己删，
/// 且只删自己那一份），以及「文件存在」不等于「被占用」——占用与否一律以
/// `flock` 为准，文件内容里的 pid 只用来判活与确认归属。
#[cfg(unix)]
fn acquire_instance_lock(lease_dir: &Path) -> Result<InstanceLock, AppError> {
    use std::os::unix::io::AsRawFd;

    let path = lease_dir.join(store_lock::LOCK_FILE);
    let (mut file, created) = open_instance_lock(&path)?;
    // SAFETY: file 持有有效 fd，flock 只改这把 fd 的锁状态。
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(AppError::Skill(format!(
                "无法获取 MiniMax 实例生命周期锁 {}：{error}；请确认没有其他壳在管理该实例，并检查已有锁后重试，原有文件不会被移除",
                path.display()
            )));
        }
        return Err(io_error(error));
    }
    // 旧协议的锁（普通文件 + 持有者 pid）不持 flock，所以拿到 flock 也不代表
    // 没人管：持有者确实死了才回收，活着就报错并保留原文件（判据与技能中央库
    // 抢僵尸锁同一条）。
    if !created {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let owner = text.trim();
        if owner.is_empty() {
            return Err(AppError::Skill(format!(
                "无法获取 MiniMax 实例生命周期锁 {}：锁文件里没有可识别的持有者；请确认没有其他壳在管理该实例，并检查已有锁后重试，原有文件不会被移除",
                path.display()
            )));
        }
        let mine = owner.parse::<u32>().ok() == Some(std::process::id());
        if !mine && !store_lock::owner_is_dead(&path) {
            return Err(AppError::Skill(format!(
                "无法获取 MiniMax 实例生命周期锁 {}：持有者（pid {owner}）仍在运行；请确认没有其他壳在管理该实例，并检查已有锁后重试，原有文件不会被移除",
                path.display()
            )));
        }
        if !mine {
            crate::shell::shell_events::record(
                "mcode-lifecycle",
                &format!("已回收已失效的 MiniMax 生命周期锁：{}", path.display()),
            );
        }
    }
    // 内容写自己的 pid：释放时据此确认「这份锁是我的」，旧协议残留也据此判活。
    file.set_len(0).map_err(io_error)?;
    file.write_all(std::process::id().to_string().as_bytes())
        .map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    Ok(InstanceLock { _file: file, path })
}

/// Unix 上的锁文件：不存在就建，存在就打开。布尔值表示「是不是我们刚建的」——
/// 只有刚建的空文件可以直接接管，已存在的空文件是认不出持有者的旧锁。
#[cfg(unix)]
fn open_instance_lock(path: &Path) -> io::Result<(fs::File, bool)> {
    match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => Ok((file, true)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok((
            fs::OpenOptions::new().read(true).write(true).open(path)?,
            false,
        )),
        Err(error) => Err(error),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct StartToken {
    nonce: String,
    generation: u64,
    owner_pid: u32,
}

/// These observations must come from the lifecycle, under this same lease.
pub(crate) enum StopEvidence {
    NeverSpawned,
    TreeStopped,
    #[cfg(test)]
    Unknown,
}

/// Only an exclusively created instance directory can establish that it never ran.
pub(crate) fn enroll_fresh(instance_id: &str) -> Result<(), AppError> {
    paths::validate_id_component(instance_id).map_err(|error| AppError::Skill(error.into()))?;
    let root = paths::kernel_instances_dir("mcode");
    fs::create_dir_all(&root).map_err(io_error)?;
    let instance = root.join(instance_id);
    match fs::create_dir(&instance) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => return Err(io_error(error)),
    }
    let home = instance.join("home");
    fs::create_dir(&home).map_err(io_error)?;
    let lifecycle = home.join(".xlink-mcode-lifecycle");
    fs::create_dir(&lifecycle).map_err(io_error)?;
    let mut marker = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lifecycle.join("offline-v1"))
        .map_err(io_error)?;
    marker.write_all(b"mcode-offline-v1\n").map_err(io_error)?;
    marker.sync_all().map_err(io_error)?;
    Ok(())
}

pub(crate) fn enrolled(instance_home: &Path) -> bool {
    !matches!(fs::symlink_metadata(instance_home.join(".xlink-mcode-lifecycle")), Err(error) if error.kind() == io::ErrorKind::NotFound)
}

pub(crate) fn acquire_lease(instance_home: &Path) -> Result<InstanceLease, AppError> {
    let home_md = fs::symlink_metadata(instance_home).map_err(io_error)?;
    if !home_md.is_dir() || home_md.file_type().is_symlink() {
        return Err(AppError::Skill("MiniMax 实例 home 不是普通目录".into()));
    }
    let home = fs::canonicalize(instance_home).map_err(io_error)?;
    let root = fs::canonicalize(paths::kernel_instances_dir("mcode")).map_err(io_error)?;
    if home.file_name().and_then(|v| v.to_str()) != Some("home")
        || home.parent().and_then(Path::parent) != Some(root.as_path())
    {
        return Err(AppError::Skill("MiniMax 技能目标不在实例目录内".into()));
    }
    let lease_dir = home.join(".xlink-mcode-lifecycle");
    let metadata = fs::symlink_metadata(&lease_dir)
        .map_err(|_| AppError::Skill("MiniMax 实例尚未启用安全离线协议，技能对账已跳过".into()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::Skill("MiniMax 生命周期目录无效".into()));
    }
    let lease = acquire_instance_lock(&lease_dir)?;
    let enrolled = lease_dir.join("offline-v1");
    let metadata = fs::symlink_metadata(&enrolled)
        .map_err(|_| AppError::Skill("MiniMax 实例尚未启用安全离线协议，技能对账已跳过".into()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || fs::read(&enrolled).map_err(io_error)? != b"mcode-offline-v1\n"
    {
        return Err(AppError::Skill("MiniMax 离线协议记录损坏".into()));
    }
    Ok(InstanceLease { home, _lock: lease })
}

fn require_offline(lease: &InstanceLease) -> Result<(), AppError> {
    match fs::symlink_metadata(lease.home.join(".xlink-mcode-lifecycle/running")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
        Ok(_) => {
            return Err(AppError::Skill(
                "MiniMax 可能仍在运行，技能对账已跳过".into(),
            ))
        }
    }
    let id = lease
        .home
        .parent()
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        .ok_or_else(|| AppError::Skill("MiniMax 实例标识无效".into()))?;
    match fs::symlink_metadata(paths::instance_pid_file("mcode", id)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
        Ok(_) => {
            return Err(AppError::Skill(
                "MiniMax 进程记录尚未清理，技能对账已跳过".into(),
            ))
        }
    }
    Ok(())
}

pub(crate) fn offline_home(lease: &InstanceLease) -> Result<&Path, AppError> {
    require_offline(lease)?;
    Ok(&lease.home)
}

#[cfg(test)]
fn offline_lease(home: &Path) -> Result<InstanceLease, AppError> {
    let lease = acquire_lease(home)?;
    require_offline(&lease)?;
    Ok(lease)
}

/// Convenience entry point for offline-only callers, never while holding a lease.
#[cfg(test)]
pub(crate) fn reconcile(instance_home: &Path) -> Result<(), AppError> {
    let lease = offline_lease(instance_home)?;
    reconcile_locked(&lease)
}

/// 测试入口：断开共享技能库（移除清单管理的副本，用户自己的文件不动）。
#[cfg(test)]
pub(crate) fn detach(instance_home: &Path) -> Result<(), AppError> {
    let lease = offline_lease(instance_home)?;
    detach_locked(&lease)
}

pub(crate) fn running_path(home: &Path) -> PathBuf {
    home.join(".xlink-mcode-lifecycle/running")
}

/// The caller holds the lease through reconciliation and this reservation,
/// then releases it before waiting for the shell spawn gate.
pub(crate) fn arm_start(lease: &InstanceLease, generation: u64) -> Result<StartToken, AppError> {
    require_offline(lease)?;
    let token = StartToken {
        nonce: format!("{:032x}", rand::random::<u128>()),
        generation,
        owner_pid: std::process::id(),
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(running_path(&lease.home))
        .map_err(io_error)?;
    let mut data =
        serde_json::to_vec(&token).map_err(|error| AppError::Skill(error.to_string()))?;
    data.push(b'\n');
    file.write_all(&data).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    #[cfg(unix)]
    fs::File::open(lease.home.join(".xlink-mcode-lifecycle"))
        .and_then(|dir| dir.sync_all())
        .map_err(io_error)?;
    Ok(token)
}

fn check_token(lease: &InstanceLease, token: &StartToken) -> Result<(), AppError> {
    let path = running_path(&lease.home);
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(AppError::Skill("MiniMax 运行占位无效".into()));
    }
    let saved: StartToken = serde_json::from_slice(&fs::read(path).map_err(io_error)?)
        .map_err(|_| AppError::Skill("MiniMax 运行占位损坏".into()))?;
    if &saved != token || token.owner_pid != std::process::id() {
        return Err(AppError::Skill("MiniMax 运行占位不属于本次启动".into()));
    }
    Ok(())
}

pub(crate) fn discard_stale_running(lease: &InstanceLease) -> Result<(), AppError> {
    let path = running_path(&lease.home);
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
        Ok(metadata) => metadata,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(AppError::Skill(format!(
            "MiniMax 运行占位不是普通文件，请检查：{}",
            path.display()
        )));
    }
    let token: StartToken =
        serde_json::from_slice(&fs::read(&path).map_err(io_error)?).map_err(|_| {
            AppError::Skill(format!("MiniMax 运行占位损坏，已保留：{}", path.display()))
        })?;
    if token.nonce.len() != 32
        || !token.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || token.owner_pid == 0
    {
        return Err(AppError::Skill(format!(
            "MiniMax 运行占位无效，已保留：{}",
            path.display()
        )));
    }
    if !crate::kernel::lifecycle::process_is_definitely_gone(token.owner_pid) {
        return Err(AppError::Skill(format!(
            "旧窗口进程 {} 仍在或无法确认，请关闭其他窗口后重试",
            token.owner_pid
        )));
    }
    fs::remove_file(&path).map_err(io_error)?;
    #[cfg(unix)]
    fs::File::open(lease.home.join(".xlink-mcode-lifecycle"))
        .and_then(|dir| dir.sync_all())
        .map_err(io_error)?;
    Ok(())
}

/// Call while holding spawn_gate followed by the same instance lease, just
/// before spawn. A cancellation generation mismatch must never spawn a child.
pub(crate) fn verify_start(
    lease: &InstanceLease,
    token: &StartToken,
    generation: u64,
) -> Result<(), AppError> {
    check_token(lease, token)?;
    if token.generation != generation {
        return Err(AppError::Skill("MiniMax 启动已取消".into()));
    }
    let id = lease
        .home
        .parent()
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        .ok_or_else(|| AppError::Skill("MiniMax 实例标识无效".into()))?;
    match fs::symlink_metadata(paths::instance_pid_file("mcode", id)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
        Ok(_) => Err(AppError::Skill("MiniMax 进程记录尚未清理".into())),
    }
}

/// The caller must prove no child was ever created or that the complete tree
/// is gone, and remove the PID first. Unknown always preserves the reservation.
pub(crate) fn finish_start(
    lease: &InstanceLease,
    token: &StartToken,
    evidence: StopEvidence,
) -> Result<(), AppError> {
    check_token(lease, token)?;
    match evidence {
        StopEvidence::NeverSpawned | StopEvidence::TreeStopped => {}
        #[cfg(test)]
        StopEvidence::Unknown => {
            return Err(AppError::Skill(
                "无法确认 MiniMax 进程树已退出，保留运行占位".into(),
            ));
        }
    }
    let id = lease
        .home
        .parent()
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        .ok_or_else(|| AppError::Skill("MiniMax 实例标识无效".into()))?;
    match fs::symlink_metadata(paths::instance_pid_file("mcode", id)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
        Ok(_) => return Err(AppError::Skill("MiniMax 进程记录尚未清理".into())),
    }
    fs::remove_file(running_path(&lease.home)).map_err(io_error)?;
    #[cfg(unix)]
    fs::File::open(lease.home.join(".xlink-mcode-lifecycle"))
        .and_then(|dir| dir.sync_all())
        .map_err(io_error)?;
    Ok(())
}

/// Caller holds the instance lease, and must not hold the shared store lock.
pub(crate) fn reconcile_locked(lease: &InstanceLease) -> Result<(), AppError> {
    reconcile_inner(lease, true)
}

/// 断开共享技能库：把清单管理的技能副本全部移除，用户自己的文件不动。
/// 与同步走同一条事务路径，离线租约与锁的判据不变。
pub(crate) fn detach_locked(lease: &InstanceLease) -> Result<(), AppError> {
    reconcile_inner(lease, false)
}

fn reconcile_inner(lease: &InstanceLease, linked: bool) -> Result<(), AppError> {
    require_offline(lease)?;
    let home = &lease.home;
    directory(&home.join("skills"))?;
    directory(&home.join(".xlink-skill-work"))?;
    let store_root = manage::store_dir(home);
    fs::create_dir_all(&store_root).map_err(io_error)?;
    let _guard = store_lock::lock_both(&store_root, StealPolicy::Fail).map_err(AppError::Skill)?;
    let store = manage::load_store_checked(home)?;
    let work = home.join(".xlink-skill-work");
    let mut manifest = load(&work)?;
    if !store_root.join("store.json").exists() && !manifest.entries.is_empty() {
        return Err(AppError::Skill(
            "技能中央库清单缺失，已保留 MiniMax 技能视图".into(),
        ));
    }
    for name in manifest.entries.keys() {
        paths::validate_id_component(name).map_err(|e| AppError::Skill(e.into()))?;
        if !manage::is_kebab_case(name) {
            return Err(AppError::Skill(format!(
                "MiniMax 技能清单中的名称无效：{name}"
            )));
        }
    }
    let pending: Vec<String> = manifest
        .entries
        .iter()
        .filter(|(_, entry)| entry.pending.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    for name in pending {
        finish(&name, home, &mut manifest)?;
    }
    // 断开时 desired 为空：下面的收尾循环会把清单里的副本全部移除。
    let mut desired = BTreeMap::new();
    if linked {
        for item in store.items {
            paths::validate_id_component(&item.id).map_err(|e| AppError::Skill(e.into()))?;
            for entry in item.skills.into_iter().filter(|entry| entry.enabled) {
                paths::validate_id_component(&entry.name).map_err(|e| AppError::Skill(e.into()))?;
                if !manage::is_kebab_case(&entry.name) {
                    return Err(AppError::Skill(format!("技能名称无效：{}", entry.name)));
                }
                if Path::new(&entry.path)
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
                {
                    return Err(AppError::Skill(format!("技能路径无效：{}", entry.name)));
                }
                let source = checked_source(&store_root, &item.id, &entry.path)?;
                let flat = Path::new(&entry.path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
                if desired.insert(entry.name.clone(), (source, flat)).is_some() {
                    return Err(AppError::Skill(format!("重复的技能名称：{}", entry.name)));
                }
            }
        }
    }
    let names: Vec<String> = desired
        .keys()
        .chain(manifest.entries.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    for name in names {
        let current = regular_digest(&home.join("skills").join(&name))?;
        let old = manifest
            .entries
            .get(&name)
            .and_then(|entry| entry.fingerprint.clone());
        if current != old {
            return Err(AppError::Skill(format!(
                "MiniMax 技能名称冲突或已被修改：{name}"
            )));
        }
        let nonce = format!("{:032x}", rand::random::<u128>());
        let stage = home.join(".xlink-skill-work").join(format!("new-{nonce}"));
        let new = if let Some((source, flat)) = desired.get(&name) {
            fs::create_dir(&stage).map_err(io_error)?;
            if *flat {
                fs::copy(source, stage.join("SKILL.md")).map_err(io_error)?;
            } else {
                // copy_tree skips links; refusing them prevents silently missing resources.
                reject_links(source)?;
                fetch::copy_tree(source, &stage).map_err(io_error)?;
            }
            if !stage.join("SKILL.md").is_file() {
                return Err(AppError::Skill(format!("技能缺少 SKILL.md：{name}")));
            }
            let markdown = fs::read_to_string(stage.join("SKILL.md")).map_err(io_error)?;
            if manage::parse_skill_markdown(&markdown).map(|(parsed, _)| parsed)
                != Some(name.clone())
            {
                return Err(AppError::Skill(format!("技能内容与名称不一致：{name}")));
            }
            regular_digest(&stage)?
        } else {
            None
        };
        if old == new {
            if new.is_some() {
                fs::remove_dir_all(&stage).map_err(io_error)?;
            }
            continue;
        }
        manifest.entries.entry(name.clone()).or_default().pending =
            Some(Pending { old, new, nonce });
        save(&work, &manifest)?;
        finish(&name, home, &mut manifest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrollment_only_marks_a_new_instance() {
        let root =
            std::env::temp_dir().join(format!("mcode-enrollment-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let existing = paths::instance_dsh_home("mcode", "existing");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("keep"), b"user data").unwrap();
        enroll_fresh("existing").unwrap();
        assert!(!enrolled(&existing));
        assert_eq!(fs::read(existing.join("keep")).unwrap(), b"user data");
        enroll_fresh("new").unwrap();
        let home = paths::instance_dsh_home("mcode", "new");
        assert!(enrolled(&home));
        assert!(acquire_lease(&home).is_ok());
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_is_confined_to_its_central_package() {
        let root =
            std::env::temp_dir().join(format!("mcode-source-{:032x}", rand::random::<u128>()));
        fs::create_dir_all(root.join("package/skill")).unwrap();
        fs::write(root.join("package/skill/SKILL.md"), b"skill").unwrap();
        fs::write(root.join("outside.md"), b"other data").unwrap();
        assert!(checked_source(&root, "package", "skill/SKILL.md").is_ok());
        assert!(checked_source(&root, "package", "../outside.md").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("outside.md"), root.join("package/link.md"))
                .unwrap();
            assert!(checked_source(&root, "package", "link.md").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn start_reservation_requires_matching_nonce_generation_and_stop_evidence() {
        let root = std::env::temp_dir().join(format!(
            "mcode-start-reservation-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        let home = paths::instance_dsh_home("mcode", "test");
        let dir = home.join(".xlink-mcode-lifecycle");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("offline-v1"), b"mcode-offline-v1\n").unwrap();
        let lease = acquire_lease(&home).unwrap();
        let token = arm_start(&lease, 12).unwrap();
        assert!(verify_start(&lease, &token, 12).is_ok());
        assert!(verify_start(&lease, &token, 13).is_err());
        assert!(arm_start(&lease, 13).is_err());
        assert!(reconcile_locked(&lease).is_err());
        let mut other = token.clone();
        other.nonce = "different".into();
        assert!(finish_start(&lease, &other, StopEvidence::NeverSpawned).is_err());
        assert!(finish_start(&lease, &token, StopEvidence::Unknown).is_err());
        assert!(running_path(&lease.home).exists());
        let pid = paths::instance_pid_file("mcode", "test");
        fs::create_dir_all(pid.parent().unwrap()).unwrap();
        fs::write(&pid, "unverified").unwrap();
        assert!(finish_start(&lease, &token, StopEvidence::NeverSpawned).is_err());
        assert!(running_path(&lease.home).exists());
        fs::remove_file(pid).unwrap();
        // A generation cancellation does not prevent safe pre-spawn cleanup.
        finish_start(&lease, &token, StopEvidence::NeverSpawned).unwrap();
        assert!(!running_path(&lease.home).exists());
        let token = arm_start(&lease, 14).unwrap();
        finish_start(&lease, &token, StopEvidence::TreeStopped).unwrap();
        fs::write(running_path(&lease.home), "crash residue").unwrap();
        assert!(arm_start(&lease, 15).is_err());
        assert_eq!(
            fs::read_to_string(running_path(&lease.home)).unwrap(),
            "crash residue"
        );
        drop(lease);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cross_process_lease() {
        const CHILD_ROOT: &str = "DSH_XLINK_MCODE_LEASE_TEST_CHILD";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = std::path::PathBuf::from(root);
            let _guard = crate::tests::scoped_xlink_home(&root);
            let home = paths::instance_dsh_home("mcode", "test");
            assert!(offline_lease(&home).is_err());
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "mcode-skill-lease-process-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        let home = paths::instance_dsh_home("mcode", "test");
        let dir = home.join(".xlink-mcode-lifecycle");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("offline-v1"), b"mcode-offline-v1\n").unwrap();
        let lease = offline_lease(&home).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "skills::mcode::tests::cross_process_lease"])
            .env(CHILD_ROOT, &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(lease);
        assert!(offline_lease(&home).is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    // Windows 专属：句柄独占（share_mode(0)）本身就是互斥，Unix 侧另有形状。
    #[cfg(windows)]
    #[test]
    fn lifecycle_lock_remains_compatible_with_legacy_shell() {
        use std::os::windows::io::AsRawHandle;
        use std::time::Duration;
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};

        let root = std::env::temp_dir().join(format!(
            "mcode-legacy-lease-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        enroll_fresh("test").unwrap();
        let home = paths::instance_dsh_home("mcode", "test");
        let dir = home.join(".xlink-mcode-lifecycle");
        let path = dir.join(store_lock::LOCK_FILE);

        // A legacy shell uses this exact file and must exclude the new lease.
        let old = store_lock::acquire_within(&dir, StealPolicy::Fail, Duration::ZERO).unwrap();
        assert!(acquire_lease(&home).is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            std::process::id().to_string()
        );
        drop(old);

        let new = acquire_lease(&home).unwrap();
        let mut flags = 0;
        // SAFETY: new owns a live handle and flags is a valid output pointer.
        assert_ne!(
            unsafe { GetHandleInformation(new._lock.as_raw_handle(), &mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        assert!(store_lock::acquire_within(&dir, StealPolicy::Fail, Duration::ZERO).is_err());
        assert!(acquire_lease(&home).is_err());
        assert!(fs::remove_file(&path).is_err());
        assert!(fs::rename(&path, dir.join("unexpected-lock-move")).is_err());
        drop(new);
        let old = store_lock::acquire_within(&dir, StealPolicy::Fail, Duration::ZERO).unwrap();
        drop(old);

        // 旧协议的锁在持有者崩溃后留在盘上：持有者确实死了才回收它。
        let mut dead = crate::skills::store_owner::spawn_exiting();
        fs::write(&path, dead.id().to_string()).unwrap();
        assert!(
            acquire_lease(&home).is_ok(),
            "持有者已死的旧锁必须能回收，否则 MiniMax 永远起不来"
        );
        let _ = dead.wait();

        // A legacy crash or an unknown lock is not proof of ownership.
        fs::write(&path, b"historical-or-unknown").unwrap();
        assert!(acquire_lease(&home).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"historical-or-unknown");
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    /// Unix 上的租约：`flock` 互斥、持有者死了内核放锁、旧锁按 pid 判活。
    #[cfg(unix)]
    #[test]
    fn unix_lease_excludes_the_second_holder_and_recovers_after_the_holder_dies() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        const CHILD_ROOT: &str = "DSH_XLINK_MCODE_UNIX_LEASE_TEST_CHILD";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let _guard = crate::tests::scoped_xlink_home(&root);
            let home = paths::instance_dsh_home("mcode", "test");
            let _lease = acquire_lease(&home).unwrap();
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

        let root =
            std::env::temp_dir().join(format!("mcode-unix-lease-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        enroll_fresh("test").unwrap();
        let home = paths::instance_dsh_home("mcode", "test");
        let dir = home.join(".xlink-mcode-lifecycle");
        let path = dir.join(store_lock::LOCK_FILE);

        // 另一个壳在管：子进程持锁时本进程拿不到。
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "skills::mcode::tests::unix_lease_excludes_the_second_holder_and_recovers_after_the_holder_dies",
            ])
            .env(CHILD_ROOT, &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = ChildGuard(crate::shell::process::quiet(&mut command).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(15);
        while !root.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before acquiring lock"
            );
            assert!(Instant::now() < deadline, "child did not acquire lock");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(acquire_lease(&home).is_err(), "另一个壳持锁时不得抢到租约");
        // 把文件里的 pid 换成已死的持有者：按 pid 判活这时会放行，只有 flock
        // 能挡住第二个持有者——flock 失效时这条会红。
        let mut stale = crate::skills::store_owner::spawn_exiting();
        fs::write(&path, stale.id().to_string()).unwrap();
        assert!(
            acquire_lease(&home).is_err(),
            "锁文件里的 pid 已死也不许抢：占用与否只认 flock"
        );
        let _ = stale.wait();

        // 持有者被杀：flock 由内核放掉，锁可重新获取。
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let lease = acquire_lease(&home).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            std::process::id().to_string(),
            "接管后锁文件里必须是自己的 pid"
        );
        drop(lease);
        assert!(!path.exists(), "释放时必须删掉自己那一份锁文件");

        // 释放时只删自己那一份：内容不是自己的 pid 就不动它。
        let lease = acquire_lease(&home).unwrap();
        fs::write(&path, b"someone-else").unwrap();
        drop(lease);
        assert!(path.exists(), "不是自己的锁文件不许删");
        fs::remove_file(&path).unwrap();

        // 读不出 pid 的旧锁按活着处理：不删、不抢。
        fs::write(&path, b"historical-or-unknown").unwrap();
        assert!(acquire_lease(&home).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"historical-or-unknown");

        // 旧锁的持有者确实死了才回收。
        let mut dead = crate::skills::store_owner::spawn_exiting();
        fs::write(&path, dead.id().to_string()).unwrap();
        assert!(
            acquire_lease(&home).is_ok(),
            "持有者已死的旧锁必须能回收，否则 MiniMax 永远起不来"
        );
        let _ = dead.wait();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lifecycle_lock_releases_after_process_killed_but_reservation_remains() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        const CHILD_ROOT: &str = "DSH_XLINK_MCODE_LEASE_CRASH_TEST_CHILD";
        const CHILD_RESERVE: &str = "DSH_XLINK_MCODE_LEASE_CRASH_TEST_RESERVE";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let _guard = crate::tests::scoped_xlink_home(&root);
            let home = paths::instance_dsh_home("mcode", "test");
            let lease = acquire_lease(&home).unwrap();
            if std::env::var_os(CHILD_RESERVE).is_some() {
                arm_start(&lease, 1).unwrap();
            }
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

        for with_reservation in [false, true] {
            let root = std::env::temp_dir()
                .join(format!("mcode-lease-crash-{:032x}", rand::random::<u128>()));
            fs::create_dir(&root).unwrap();
            let guard = crate::tests::scoped_xlink_home(&root);
            enroll_fresh("test").unwrap();
            let home = paths::instance_dsh_home("mcode", "test");
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "skills::mcode::tests::lifecycle_lock_releases_after_process_killed_but_reservation_remains",
                ])
                .env(CHILD_ROOT, &root)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if with_reservation {
                command.env(CHILD_RESERVE, "1");
            } else {
                command.env_remove(CHILD_RESERVE);
            }
            let mut child = ChildGuard(crate::shell::process::quiet(&mut command).spawn().unwrap());
            let deadline = Instant::now() + Duration::from_secs(15);
            while !root.join("ready").exists() {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "child exited before acquiring lock"
                );
                assert!(Instant::now() < deadline, "child did not acquire lock");
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(acquire_lease(&home).is_err());
            let running = home.join(".xlink-mcode-lifecycle/running");
            let reservation = with_reservation.then(|| fs::read(&running).unwrap());
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            let lease = acquire_lease(&home).unwrap();
            if let Some(reservation) = reservation {
                assert!(offline_home(&lease).is_err());
                assert_eq!(fs::read(&running).unwrap(), reservation);
            } else {
                assert!(offline_home(&lease).is_ok());
            }
            drop(lease);
            drop(child);
            drop(guard);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn offline_lease_refuses_unknown_running_and_concurrent_instances() {
        let root =
            std::env::temp_dir().join(format!("mcode-skill-lease-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        let home = paths::instance_dsh_home("mcode", "test");
        fs::create_dir_all(home.join("skills/user")).unwrap();
        fs::write(home.join("skills/user/SKILL.md"), "user content").unwrap();
        assert!(reconcile(&home).is_err());
        let dir = home.join(".xlink-mcode-lifecycle");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("offline-v1"), b"mcode-offline-v1\n").unwrap();
        fs::write(dir.join("running"), "crash residue").unwrap();
        assert!(reconcile(&home).is_err());
        fs::remove_file(dir.join("running")).unwrap();
        let lease = offline_lease(&home).unwrap();
        assert!(reconcile(&home).is_err());
        drop(lease);
        let pid = paths::instance_pid_file("mcode", "test");
        fs::create_dir_all(pid.parent().unwrap()).unwrap();
        fs::write(&pid, "unknown").unwrap();
        assert!(reconcile(&home).is_err());
        assert_eq!(
            fs::read_to_string(home.join("skills/user/SKILL.md")).unwrap(),
            "user content"
        );
        fs::remove_file(pid).unwrap();
        assert!(offline_lease(&home).is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reconciles_flat_skills_without_overwriting_user_changes() {
        let root =
            std::env::temp_dir().join(format!("mcode-skill-test-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        let home = paths::instance_dsh_home("mcode", "test");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir(home.join(".xlink-mcode-lifecycle")).unwrap();
        fs::write(
            home.join(".xlink-mcode-lifecycle/offline-v1"),
            b"mcode-offline-v1\n",
        )
        .unwrap();
        let store = paths::skills_store_root();
        fs::create_dir_all(store.join("package")).unwrap();
        fs::write(
            store.join("package/skill.md"),
            "---\nname: sample\ndescription: fixture\n---\nfirst\n",
        )
        .unwrap();
        let mut document = serde_json::json!({"schemaVersion":1,"items":[{
            "id":"package","name":"package","origin":"local","source":"fixture",
            "installed_version":"1","mode":"copy","actual_mode":"copy","pinned":true,
            "installed_at":"1","updated_at":"1","skills":[{"name":"sample","description":"fixture","path":"skill.md","enabled":true}]
        }]});
        fs::create_dir(store.join("package/folder")).unwrap();
        fs::write(
            store.join("package/folder/SKILL.md"),
            "---\nname: folder\ndescription: fixture\n---\nbody\n",
        )
        .unwrap();
        fs::write(
            store.join("package/upper.MD"),
            "---\nname: upper\ndescription: fixture\n---\nbody\n",
        )
        .unwrap();
        document["items"][0]["skills"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name":"folder","description":"fixture","path":"folder","enabled":true
            }));
        document["items"][0]["skills"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name":"upper","description":"fixture","path":"upper.MD","enabled":true
            }));
        fs::write(
            store.join("store.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        reconcile(&home).unwrap();
        let target = home.join("skills/sample/SKILL.md");
        assert!(fs::read_to_string(&target).unwrap().contains("first"));
        assert!(home.join("skills/folder/SKILL.md").is_file());
        assert!(home.join("skills/upper/SKILL.md").is_file());
        fs::write(
            store.join("package/skill.md"),
            "---\nname: sample\ndescription: fixture\n---\nsecond\n",
        )
        .unwrap();
        reconcile(&home).unwrap();
        assert!(fs::read_to_string(&target).unwrap().contains("second"));
        // 断开：清单管理的三份副本移除，用户自己的文件不动；再次断开幂等，重新接入能恢复。
        detach(&home).unwrap();
        assert!(!target.exists());
        assert!(!home.join("skills/folder").exists());
        assert!(!home.join("skills/upper").exists());
        fs::write(home.join("skills/user-note.md"), "mine").unwrap();
        detach(&home).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("skills/user-note.md")).unwrap(),
            "mine"
        );
        reconcile(&home).unwrap();
        assert!(fs::read_to_string(&target).unwrap().contains("second"));
        document["items"][0]["skills"][0]["enabled"] = false.into();
        fs::write(
            store.join("store.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        reconcile(&home).unwrap();
        assert!(!target.exists());
        document["items"][0]["skills"][0]["enabled"] = true.into();
        fs::write(
            store.join("store.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        reconcile(&home).unwrap();
        fs::write(&target, "user edit").unwrap();
        document["items"][0]["skills"][0]["enabled"] = false.into();
        fs::write(
            store.join("store.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        assert!(reconcile(&home).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "user edit");
        fs::remove_file(store.join("store.json")).unwrap();
        assert!(reconcile(&home).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "user edit");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recovers_interrupted_replacement_and_preserves_collisions() {
        let root =
            std::env::temp_dir().join(format!("mcode-skill-test-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        let home = root.join("instance");
        fs::create_dir_all(home.join("skills/sample")).unwrap();
        fs::create_dir_all(home.join(".xlink-skill-work/new-00000000000000000000000000000001"))
            .unwrap();
        let target = home.join("skills/sample");
        let stage = home.join(".xlink-skill-work/new-00000000000000000000000000000001");
        fs::write(target.join("SKILL.md"), "old").unwrap();
        fs::write(stage.join("SKILL.md"), "new").unwrap();
        let old = regular_digest(&target).unwrap();
        let new = regular_digest(&stage).unwrap();
        let mut manifest = Manifest::default();
        manifest.entries.insert(
            "sample".into(),
            Entry {
                fingerprint: old.clone(),
                pending: Some(Pending {
                    old,
                    new,
                    nonce: "00000000000000000000000000000001".into(),
                }),
            },
        );
        save(&home.join(".xlink-skill-work"), &manifest).unwrap();
        rename_exclusive(
            &target,
            &home.join(".xlink-skill-work/old-00000000000000000000000000000001"),
        )
        .unwrap();
        finish("sample", &home, &mut manifest).unwrap();
        assert_eq!(fs::read_to_string(target.join("SKILL.md")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(
                home.join(".xlink-skill-work/old-00000000000000000000000000000001/SKILL.md")
            )
            .unwrap(),
            "old"
        );
        fs::create_dir_all(home.join("other")).unwrap();
        fs::write(home.join("other/keep"), "user").unwrap();
        assert!(rename_exclusive(&target, &home.join("other")).is_err());
        assert_eq!(fs::read_to_string(home.join("other/keep")).unwrap(), "user");
        let duplicate = home.join("skills/duplicate");
        let duplicate_stage = home.join(".xlink-skill-work/new-00000000000000000000000000000002");
        fs::create_dir(&duplicate).unwrap();
        fs::create_dir(&duplicate_stage).unwrap();
        fs::write(duplicate.join("SKILL.md"), "user").unwrap();
        fs::write(duplicate_stage.join("SKILL.md"), "user").unwrap();
        let digest = regular_digest(&duplicate_stage).unwrap();
        manifest.entries.insert(
            "duplicate".into(),
            Entry {
                fingerprint: None,
                pending: Some(Pending {
                    old: None,
                    new: digest,
                    nonce: "00000000000000000000000000000002".into(),
                }),
            },
        );
        assert!(finish("duplicate", &home, &mut manifest).is_err());
        assert_eq!(
            fs::read_to_string(duplicate.join("SKILL.md")).unwrap(),
            "user"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn interrupted_removal_rejects_missing_original_and_backup() {
        let root =
            std::env::temp_dir().join(format!("mcode-removal-{:032x}", rand::random::<u128>()));
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::create_dir_all(root.join(".xlink-skill-work")).unwrap();
        let mut manifest = Manifest::default();
        manifest.entries.insert(
            "sample".into(),
            Entry {
                fingerprint: Some("old-digest".into()),
                pending: Some(Pending {
                    old: Some("old-digest".into()),
                    new: None,
                    nonce: "00000000000000000000000000000001".into(),
                }),
            },
        );
        assert!(finish("sample", &root, &mut manifest).is_err());
        assert!(manifest.entries["sample"].pending.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_manifest_names_never_reach_skill_paths() {
        let root = std::env::temp_dir().join(format!(
            "mcode-invalid-name-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let home = paths::instance_dsh_home("mcode", "instance");
        fs::create_dir_all(home.join(".xlink-mcode-lifecycle")).unwrap();
        fs::write(
            home.join(".xlink-mcode-lifecycle/offline-v1"),
            b"mcode-offline-v1\n",
        )
        .unwrap();
        fs::create_dir_all(home.join(".xlink-skill-work")).unwrap();
        fs::create_dir_all(home.join("skills/keep")).unwrap();
        fs::write(home.join("skills/keep/SKILL.md"), b"user data").unwrap();
        let mut manifest = Manifest::default();
        manifest.entries.insert(
            "C:escape".into(),
            Entry {
                fingerprint: Some("unknown".into()),
                pending: None,
            },
        );
        save(&home.join(".xlink-skill-work"), &manifest).unwrap();
        assert!(reconcile(&home).is_err());
        assert_eq!(
            fs::read(home.join("skills/keep/SKILL.md")).unwrap(),
            b"user data"
        );
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_publication_keeps_previous_generation_and_rejects_corruption() {
        let root =
            std::env::temp_dir().join(format!("mcode-skill-test-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&root);
        let mut manifest = Manifest::default();
        save(&root, &manifest).unwrap();
        fs::write(root.join(".manifest-interrupted.tmp"), "partial").unwrap();
        assert!(load(&root).unwrap().entries.is_empty());
        manifest.entries.insert(
            "sample".into(),
            Entry {
                fingerprint: Some("test".into()),
                pending: None,
            },
        );
        save(&root, &manifest).unwrap();
        assert_eq!(snapshots(&root).unwrap().len(), 2);
        let latest = snapshots(&root).unwrap().pop().unwrap().1;
        fs::write(latest, "broken").unwrap();
        assert!(load(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
