//! 「写临时文件 → 原子替换」的落盘原语。
//!
//! 单独成文件而不是留在 `process.rs`：那边是 1157/1157 的反棘轮文件（只许
//! 变小），而权限收紧这类与「进程/输出流」毫无关系的落盘语义加不进去。
//!
//! 入口按两条轴分工：
//!
//! **权限**（临时文件用什么权限创建）
//! - [`atomic_write`]：跟随 umask（一般 0644）。给 `settings.json` 这类
//!   用户自己也偶尔要手改的状态文件。
//! - [`atomic_write_private`]：临时文件一落地就是 0600。给含凭据的文件。
//!
//! **持久性**（要不要真的 fsync 到盘）
//! - 默认：写完 `sync_all`、rename、再 `sync_all` 父目录。给「丢了就是丢用户
//!   数据」的文件。
//! - [`atomic_write_light`]：只写临时文件 + rename，**两次 fsync 都不做**。只给
//!   **能从别处重算出来的派生账目**（当前是模型用量账目）：进程崩溃不丢
//!   （rename 之后数据在页缓存里），断电可能丢掉最新一笔、极端情况还会读到半截
//!   文件——所以调用方的读取必须容错（`state::load_rebuildable`），把「账目没了」
//!   当成「从零重扫」。2026-10-17 实测这条路径省掉的正是 save 段 44ms 里的
//!   21ms + 19ms 两次 fsync。

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 将一个小型持久化文件写入同目录临时文件，再替换正式文件。写入文件
/// 和替换动作之间不会暴露截断的 JSON；同目录临时文件也确保 rename 不会
/// 跨文件系统。Unix 直接使用 rename 的原子替换语义，Windows 在目标已存
/// 在时先移除旧文件再 rename，至少不会让读者看到半写入内容。
///
/// 需要分段耗时走 [`atomic_write_timed`]；这条入口只关心成败。
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_timed(path, contents).map(|_| ())
}

/// [`atomic_write`] 的分段计时变体。
///
/// **为什么要这条缝**：这个原语被凭据、`settings.json`、用量账目共用，而它的
/// 成本**几乎全在两次 fsync 上**——2026-10-15 实测 141,858 字节的用量账目落盘
/// 43ms 里，`write_all + sync_all` 20.35ms、目录 `sync_all` 19.43ms，
/// `rename` 只 0.50ms。不分段就只能得出「慢」这个没法行动的结论。
#[derive(Default, Clone, Copy)]
pub(crate) struct WriteTimings {
    /// 序列化成本。**由调用方填**（[`crate::shell::state::save_timed`] 在
    /// 进入本原语之前自己掐的那段），因为序列化不属于「写临时文件再替换」
    /// 这件事——单列出来，免得和 fsync 的时间混在一处对不上账。
    pub serialize_us: u128,
    /// 写临时文件 + `sync_all`（数据 fsync）。
    pub write_sync_us: u128,
    /// `rename` 换正式路径。
    pub rename_us: u128,
    /// 父目录 `sync_all`（让 rename 本身落盘）。
    pub dir_sync_us: u128,
}

impl WriteTimings {
    /// 吐成 perf 采样要的 `(字段名, 微秒)` 四元组。**由被观测方自己组装**
    /// 而不是让调用方写四行 `record_micros`——`usage/local.rs` 的预算顶在
    /// 反棘轮线上，这种逐字段登记不该由它付账。
    pub(crate) fn as_fields(&self) -> [(&'static str, u128); 4] {
        [
            ("save_serialize", self.serialize_us),
            ("save_fsync", self.write_sync_us),
            ("save_rename", self.rename_us),
            ("save_dirsync", self.dir_sync_us),
        ]
    }
}

pub(crate) fn atomic_write_timed(path: &Path, contents: &[u8]) -> io::Result<WriteTimings> {
    let mut timings = WriteTimings::default();
    atomic_write_inner(path, contents, None, true, &mut timings)?;
    Ok(timings)
}

/// 不 fsync 的原子写：只保证「读者看到的是完整的旧文件或完整的新文件」，
/// **不保证掉电后新内容还在**。
///
/// 只给**可重建的派生账目**用（见文件头「持久性」那一段）。返回的分段耗时里
/// `dir_sync_us` 恒为 0、`write_sync_us` 只是 `write_all` 的时间——不是 bug，
/// 是这条入口没有那两步；用它的人要按这个口径读 perf 日志。
pub(crate) fn atomic_write_light(path: &Path, contents: &[u8]) -> io::Result<WriteTimings> {
    let mut timings = WriteTimings::default();
    atomic_write_inner(path, contents, None, false, &mut timings)?;
    Ok(timings)
}

/// 与 [`atomic_write`] 相同，但 Unix 上临时文件**一创建出来**就是 0600，
/// 正式路径上从未出现过权限更宽的那一刻。
///
/// 为什么不能「先 rename 再 chmod」（`set_owner_only` 的老写法）：临时文件
/// 按 umask 创建（通常 0644），于是**从开始写到 rename 之后 chmod 完成**，
/// 凭据内容都以本机其他用户可读的方式躺在磁盘上；进程若在中途被杀，那份
/// 0644 就永久留下了。权限必须在 `replace_file` **之前**落到临时文件上——
/// rename 换的是 inode，临时文件自带的 0600 会一起过去，不需要事后再补。
///
/// Windows 上是 no-op：用户 profile 下的默认 ACL 即用户私有，写入路径不提权
/// （见 `openai/vault.rs` 里同一条设计说明）。
pub(crate) fn atomic_write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_inner(
        path,
        contents,
        Some(0o600),
        true,
        &mut WriteTimings::default(),
    )
}

fn atomic_write_inner(
    path: &Path,
    contents: &[u8],
    private: Option<u32>,
    sync: bool,
    timings: &mut WriteTimings,
) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic write path has no file name",
            )
        })?
        .to_string_lossy();

    for attempt in 0..100u32 {
        let sequence = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.tmp-{}-{sequence}-{attempt}",
            std::process::id()
        ));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        restrict_temp_file(&file, private)?;

        let result = (|| -> io::Result<()> {
            let started = std::time::Instant::now();
            file.write_all(contents)?;
            if sync {
                file.sync_all()?;
            }
            timings.write_sync_us = started.elapsed().as_micros();
            drop(file);
            let started = std::time::Instant::now();
            replace_file(&temporary, path)?;
            timings.rename_us = started.elapsed().as_micros();
            let started = std::time::Instant::now();
            if sync {
                sync_parent_directory(parent)?;
            }
            timings.dir_sync_us = started.elapsed().as_micros();
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary file for atomic write",
    ))
}

#[cfg(unix)]
fn restrict_temp_file(file: &fs::File, private: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match private {
        Some(mode) => file.set_permissions(fs::Permissions::from_mode(mode)),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn restrict_temp_file(_file: &fs::File, _private: Option<u32>) -> io::Result<()> {
    Ok(())
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        match fs::rename(temporary, destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                fs::remove_file(destination)?;
                fs::rename(temporary, destination)
            }
            Err(error) => Err(error),
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-atomic-write-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn replaces_the_file_without_leaving_staging_files() {
        let dir = scratch("replace");
        let path = dir.join("state.json");
        atomic_write(&path, b"old\n").expect("first write");
        atomic_write(&path, b"new\n").expect("atomic replacement");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".state.json.tmp-")
            })
            .collect();
        assert!(leftovers.is_empty(), "临时文件必须被 rename 掉");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 免 fsync 入口仍然是原子替换：内容完整换新、不留临时文件，且计时里
    /// `dir_sync_us` 恒为 0（这条入口没有那一步——读 perf 日志的人按这个口径看）。
    #[test]
    fn light_entry_replaces_atomically_without_syncing_directories() {
        let dir = scratch("light");
        let path = dir.join("usage-state.json");
        atomic_write(&path, b"old\n").expect("durable first write");
        let timings = atomic_write_light(&path, b"new\n").expect("light write");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(timings.dir_sync_us, 0, "免 fsync 入口不该有目录 fsync 段");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".usage-state.json.tmp-")
            })
            .collect();
        assert!(leftovers.is_empty(), "临时文件必须被 rename 掉");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 私有入口落盘即 0600：新建一次，以及**覆盖一个权限更宽的存量文件**。
    ///
    /// 第二条断言是有意加的：rename 换的是 inode，存量文件的旧权限不会被
    /// 继承——于是历史上遗留的 0644 凭据文件在下次保存时自愈。
    ///
    /// 这里**测不出**「权限在 rename 之前设置」这一点：那条窗口只存在于
    /// 写与替换之间的几微秒，没有观察者能稳定采到它，而且任何一种事后再补
    /// 的写法最终态同样是 0600。防回退靠的是结构而不是断言——`openai/vault.rs`
    /// 里**根本没有**事后 chmod 的代码可搬回原处。
    #[cfg(unix)]
    #[test]
    fn private_entry_lands_on_owner_only_including_over_a_wider_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        let path = dir.join("accounts.bin");
        atomic_write_private(&path, b"sealed").expect("private write");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "凭据文件必须仅属主可读写");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write_private(&path, b"sealed-again").expect("private rewrite");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "覆盖写不能把宽权限继承下去");
        let _ = fs::remove_dir_all(&dir);
    }
}
