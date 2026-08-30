//! 安全解压 npm 与 GitHub Release 的 gzip tarball。
//!
//! npm 与 GitHub 归档都属于不可信输入。解压过程保留在 Rust 中而非借助平台 tar 进程，
//! 从而在 macOS、Windows 与 Linux 上保持一致的路径校验、链接处理与资源限制。

use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;

const ARCHIVE_ROOT: &str = "package";
const EXTRACT_DIR: &str = ".dsh-extract";
const MAX_ARCHIVE_ENTRIES: u64 = 100_000;
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

fn archive_relative_path(
    path: &Path,
    expected_root: Option<&str>,
    archive_root: &mut Option<OsString>,
) -> Result<PathBuf, String> {
    let mut components = path.components();
    let root = match components.next() {
        Some(Component::Normal(root)) => root,
        _ => return Err(format!("归档包含非法路径：{}", path.display())),
    };

    if let Some(expected) = expected_root {
        if root != std::ffi::OsStr::new(expected) {
            return Err(format!(
                "归档根目录必须是 {expected}/，收到 {}",
                root.to_string_lossy()
            ));
        }
    }
    if let Some(previous) = archive_root.as_ref() {
        if previous != root {
            return Err(format!("归档必须只包含一个根目录，发现 {}", path.display()));
        }
    } else {
        *archive_root = Some(root.to_os_string());
    }

    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => return Err(format!("归档包含越界路径：{}", path.display())),
        }
    }
    if relative == Path::new(".dsh-id") || relative.starts_with(EXTRACT_DIR) {
        return Err(format!("归档覆盖了外壳保留路径：{}", relative.display()));
    }
    Ok(relative)
}

#[cfg(test)]
fn relative_archive_path(path: &Path) -> Result<PathBuf, String> {
    let mut archive_root = None;
    archive_relative_path(path, Some(ARCHIVE_ROOT), &mut archive_root)
}

fn extract_inner(
    tarball: &Path,
    dest: &Path,
    extract_root: &Path,
    expected_root: Option<&str>,
) -> Result<(), String> {
    let file = File::open(tarball).map_err(|e| format!("打开归档失败：{e}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut entries_seen = 0u64;
    let mut unpacked_bytes = 0u64;
    let mut archive_root: Option<OsString> = None;

    for entry_result in archive
        .entries()
        .map_err(|e| format!("读取归档目录失败：{e}"))?
    {
        entries_seen = entries_seen.saturating_add(1);
        if entries_seen > MAX_ARCHIVE_ENTRIES {
            return Err(format!("归档条目超过上限（{}）", MAX_ARCHIVE_ENTRIES));
        }
        let mut entry = entry_result.map_err(|e| format!("读取归档条目失败：{e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("读取归档路径失败：{e}"))?
            .into_owned();
        let relative = archive_relative_path(&path, expected_root, &mut archive_root)?;
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err(format!("归档不允许符号链接或硬链接：{}", path.display()));
        }
        if !kind.is_dir() && !kind.is_file() {
            return Err(format!("归档包含不支持的文件类型：{}", path.display()));
        }
        let size = entry
            .header()
            .size()
            .map_err(|e| format!("读取归档大小失败：{e}"))?;
        unpacked_bytes = unpacked_bytes
            .checked_add(size)
            .ok_or_else(|| "归档展开大小溢出".to_string())?;
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            return Err(format!(
                "归档展开大小超过上限（{} MiB）",
                MAX_UNPACKED_BYTES / (1024 * 1024)
            ));
        }

        // `tar::Entry::unpack_in` 会执行第二次规范化包含性检查。
        // 上面的显式校验会直接拒绝非法路径，而不是静默清洗它们；
        // 拒绝链接可以阻止后续文件经由归档创建的符号链接进行越界访问。
        entry
            .unpack_in(extract_root)
            .map_err(|e| format!("解包 {} 失败：{e}", path.display()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
    }

    let root = archive_root.ok_or_else(|| "归档为空".to_string())?;
    let package_root = extract_root.join(PathBuf::from(root));
    if !package_root.is_dir() {
        return Err("归档缺少顶层根目录".into());
    }

    let children = fs::read_dir(&package_root)
        .map_err(|e| format!("读取解包目录失败：{e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("读取解包条目失败：{e}"))?;
    for child in &children {
        let name = child.file_name();
        let target = dest.join(&name);
        if target.symlink_metadata().is_ok() {
            return Err(format!("解包目标已有同名路径：{}", target.display()));
        }
    }
    for child in children {
        let target = dest.join(child.file_name());
        fs::rename(child.path(), &target)
            .map_err(|e| format!("发布解包文件 {} 失败：{e}", target.display()))?;
    }
    Ok(())
}

fn extract_gzip_tarball_with_root(
    tarball: &Path,
    dest: &Path,
    expected_root: Option<&str>,
) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("创建解包目录失败：{e}"))?;
    let extract_root = dest.join(EXTRACT_DIR);
    if extract_root.symlink_metadata().is_ok() {
        return Err(format!("解包临时目录已被占用：{}", extract_root.display()));
    }
    fs::create_dir_all(&extract_root).map_err(|e| format!("创建解包临时目录失败：{e}"))?;
    let result = extract_inner(tarball, dest, &extract_root, expected_root);
    let cleanup = fs::remove_dir_all(&extract_root);
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(format!("清理解包临时目录失败：{error}")),
    }
}

/// 将 npm 风格的 `.tgz` 解压到 `dest`，并去掉其开头的 `package/`。
///
/// 归档被限制为最多 100,000 条目，以及最多 512 MiB 的声明解压体积。
/// 绝对路径、父级组件、链接以及特殊文件会在到达目标之前就被拒绝。
pub(crate) fn extract_gzip_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    extract_gzip_tarball_with_root(tarball, dest, Some(ARCHIVE_ROOT))
}

/// 将 GitHub Release 的 `.tar.gz` 解压到 `dest`，并去掉其唯一的顶层目录。
///
/// GitHub source archive 的根目录通常由仓库名和 commit 拼成，不能复用
/// npm 固定的 `package/` 规则；其余路径、链接和资源限制与 npm 归档完全相同。
pub(crate) fn extract_github_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    extract_gzip_tarball_with_root(tarball, dest, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tar::{Builder, EntryType, Header};

    static ARCHIVE_TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_root() -> PathBuf {
        let seq = ARCHIVE_TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dsh-xlink-archive-test-{}-{seq}",
            std::process::id()
        ))
    }

    #[test]
    fn rejects_archive_escape_components() {
        assert!(relative_archive_path(Path::new("package/../../outside")).is_err());
        assert!(relative_archive_path(Path::new("/tmp/outside")).is_err());
        assert!(relative_archive_path(Path::new("other/file.js")).is_err());
    }

    #[test]
    fn extracts_package_root_without_links() {
        let root = test_root();
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tarball = root.join("package.tgz");
        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();

        let file = File::create(&tarball).unwrap();
        let encoder = GzEncoder::new(file, Compression::fast());
        let mut builder = Builder::new(encoder);
        let body = b"export default 1;\n";
        let mut header = Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/lib/index.js", body.as_slice())
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        extract_gzip_tarball(&tarball, &dest).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("lib/index.js")).unwrap(),
            "export default 1;\n"
        );
        assert!(!dest.join(EXTRACT_DIR).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_github_root_without_links() {
        let root = test_root();
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tarball = root.join("github.tgz");
        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();

        let file = File::create(&tarball).unwrap();
        let encoder = GzEncoder::new(file, Compression::fast());
        let mut builder = Builder::new(encoder);
        let body = b"module.exports = {}\n";
        let mut header = Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "plugin-v1.2.3/lib/index.js", body.as_slice())
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        extract_github_tarball(&tarball, &dest).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("lib/index.js")).unwrap(),
            "module.exports = {}\n"
        );
        assert!(!dest.join(EXTRACT_DIR).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_github_archive_with_multiple_roots() {
        let root = test_root();
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tarball = root.join("multiple-roots.tgz");
        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();

        let file = File::create(&tarball).unwrap();
        let encoder = GzEncoder::new(file, Compression::fast());
        let mut builder = Builder::new(encoder);
        for path in ["one/package.json", "two/package.json"] {
            let body = b"{}";
            let mut header = Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, body.as_slice())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();

        let error = extract_github_tarball(&tarball, &dest).expect_err("multiple roots must fail");
        assert!(
            error.contains("一个根目录"),
            "error explains root constraint: {error}"
        );
        assert!(!dest.join("package.json").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_archive_links_before_publishing() {
        let root = test_root();
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tarball = root.join("link.tgz");
        let dest = root.join("dest");
        fs::create_dir_all(&dest).unwrap();

        let file = File::create(&tarball).unwrap();
        let encoder = GzEncoder::new(file, Compression::fast());
        let mut builder = Builder::new(encoder);
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_link_name("../../outside").unwrap();
        header.set_path("package/link").unwrap();
        header.set_cksum();
        builder.append(&header, &[][..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let error = extract_gzip_tarball(&tarball, &dest).expect_err("links must be rejected");
        assert!(error.contains("符号链接"));
        assert!(!dest.join("link").exists());
        assert!(!dest.join(EXTRACT_DIR).exists());
        let _ = fs::remove_dir_all(root);
    }
}
