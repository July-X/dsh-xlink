//! 按需下载并安装官方 Node.js 托管运行时。
//!
//! 与「把 Node 二进制随安装包捆绑」相反，这里把运行时安装到数据目录
//! （`<data_dir>/tools/node/<version>/`），安装包体积不因 Node 而增大。
//! 只在用户确认后触发（见 `commands::install_node`），下载产物以官方
//! nodejs.org/dist 的 SHASUMS256.txt 做 SHA-256 校验，解包沿用
//! archive.rs 的路径约束思路（单根目录、拒绝越界与链接）。
//!
//! 布局（darwin / windows）：
//!   tools/node/v<version>/bin/node（+ bin/npm shim）+ LICENSE + lib/node_modules/npm/
//!   tools/node/v<version>/node.exe（+ npm.cmd shim）+ LICENSE + node_modules/npm/
//! 只保留运行 dsh 内核与自动安装 pnpm 所需的最小集合；npm 以 shim 形式
//! 暴露（官方产物里的 bin/npm 是符号链接，本解包器跳过符号链接）。

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::process::{build_log_kind, current_date_string, LogSpec, RotatingLog};
use crate::releases::{http_get_file, http_get_string};

/// 托管运行时的固定版本。选 v24 LTS（满足 dsh engines `^22.19 || >=24`，
/// 且 darwin-x64 / win-x64 均有官方产物）；升级由外壳发版带动。
pub const MANAGED_NODE_VERSION: &str = "24.20.0";

fn dist_base() -> String {
    format!("https://nodejs.org/dist/v{MANAGED_NODE_VERSION}")
}

fn artifact_name() -> &'static str {
    if cfg!(windows) {
        "node-v24.20.0-win-x64.zip"
    } else {
        "node-v24.20.0-darwin-x64.tar.gz"
    }
}

/// 下载体积提示文案（压缩包大小，供进度消息与弹窗使用）。
pub fn artifact_size_text() -> &'static str {
    if cfg!(windows) {
        "约 36 MB"
    } else {
        "约 52 MB"
    }
}

/// 托管运行时根目录：`<data_dir>/tools/node/`。
pub(crate) fn managed_root(data_dir: &Path) -> PathBuf {
    data_dir.join("tools").join("node")
}

/// 当前固定版本的安装目录。
pub(crate) fn managed_version_dir(data_dir: &Path) -> PathBuf {
    managed_root(data_dir).join(format!("v{MANAGED_NODE_VERSION}"))
}

/// 托管 node 可执行文件路径（存在即返回）。UI / 检测逻辑据此探测可用性。
pub(crate) fn managed_node_exe(data_dir: &Path) -> Option<PathBuf> {
    let version_dir = managed_version_dir(data_dir);
    let exe = if cfg!(windows) {
        version_dir.join("node.exe")
    } else {
        version_dir.join("bin").join("node")
    };
    exe.is_file().then_some(exe)
}

/// 从 SHASUMS256.txt 文本中提取指定产物的哈希（“<hash>  <filename>”）。
fn shasum_for(lines: &str, name: &str) -> Option<String> {
    lines.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some(hash), Some(file_name)) if file_name == name => Some(hash.to_string()),
            _ => None,
        }
    })
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| format!("打开下载文件失败：{e}"))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(|e| format!("计算哈希失败：{e}"))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 单根目录校验：所有条目必须位于同一根目录下，拒绝绝对路径 / `..` /
/// 其余特殊组件。返回根目录之下的相对路径。
fn strip_root(path: &Path, expected_root: &str) -> Result<PathBuf, String> {
    let mut components = path.components();
    let root = match components.next() {
        Some(Component::Normal(root)) => root,
        _ => return Err(format!("归档包含非法路径：{}", path.display())),
    };
    if root != std::ffi::OsStr::new(expected_root) {
        return Err(format!(
            "归档根目录必须是 {expected_root}/，收到 {}",
            path.to_string_lossy()
        ));
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => return Err(format!("归档包含越界路径：{}", path.display())),
        }
    }
    Ok(relative)
}

const MAX_ARCHIVE_ENTRIES: u64 = 100_000;
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

/// darwin：只保留 `bin/node`、`LICENSE` 与 `lib/node_modules/npm/`。
#[cfg(not(windows))]
fn keep_darwin(relative: &Path) -> bool {
    relative == Path::new("bin/node")
        || relative == Path::new("LICENSE")
        || relative.starts_with("lib/node_modules/npm")
}

/// windows：只保留 `node.exe`、`LICENSE` 与 `node_modules/npm/`。
#[cfg(windows)]
fn keep_win(relative: &Path) -> bool {
    relative == Path::new("node.exe")
        || relative == Path::new("LICENSE")
        || relative.starts_with("node_modules/npm")
}

#[cfg(not(windows))]
fn extract_tarball(tarball: &Path, version_dir: &Path) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    const ROOT: &str = "node-v24.20.0-darwin-x64";

    let file = fs::File::open(tarball).map_err(|e| format!("打开归档失败：{e}"))?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let mut entries_seen = 0u64;
    let mut unpacked_bytes = 0u64;

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
        let relative = strip_root(&path, ROOT)?;
        let kind = entry.header().entry_type();

        // 符号链接 / 硬链接是官方 dist 的便利入口（bin/npm 等），
        // 解包器跳过它们：需要的入口由外壳以 shim 重建。
        if kind.is_symlink() || kind.is_hard_link() {
            continue;
        }
        if kind.is_dir() {
            // 目录同样按保留前缀过滤：只建需要的骨架（bin/、lib/…），
            // 避免把 include/、share/ 等不需要的空目录带进安装目录。
            if relative.as_os_str().is_empty() || keep_darwin(&relative) {
                fs::create_dir_all(version_dir.join(&relative))
                    .map_err(|e| format!("创建目录失败：{e}"))?;
            }
            continue;
        }
        if !kind.is_file() {
            return Err(format!("归档包含不支持的文件类型：{}", path.display()));
        }
        if !keep_darwin(&relative) {
            continue;
        }
        let size = entry
            .header()
            .size()
            .map_err(|e| format!("读取条目大小失败：{e}"))?;
        unpacked_bytes = unpacked_bytes.saturating_add(size);
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            return Err("解包内容超过大小上限".into());
        }
        let target = version_dir.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
        }
        // 先取模式（保留 bin/node 的可执行位），再复制内容。
        // tar 0.4 的 Header::mode() 返回 Result，读取失败按无执行位处理。
        let mode = entry.header().mode().unwrap_or(0o644);
        let mut out = fs::File::create(&target).map_err(|e| format!("写入文件失败：{e}"))?;
        io::copy(&mut entry, &mut out).map_err(|e| format!("写入文件失败：{e}"))?;
        if cfg!(unix) {
            use std::os::unix::fs::PermissionsExt;
            if mode & 0o111 != 0 {
                fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777))
                    .map_err(|e| format!("设置权限失败：{e}"))?;
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn extract_zip(zip_path: &Path, version_dir: &Path) -> Result<(), String> {
    const ROOT: &str = "node-v24.20.0-win-x64";

    let file = fs::File::open(zip_path).map_err(|e| format!("打开归档失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取 zip 失败：{e}"))?;
    if archive.len() as u64 > MAX_ARCHIVE_ENTRIES {
        return Err(format!("归档条目超过上限（{}）", MAX_ARCHIVE_ENTRIES));
    }
    let mut unpacked_bytes = 0u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("读取归档条目失败：{e}"))?;
        let Some(path) = entry.enclosed_name() else {
            return Err("归档包含越界路径".into());
        };
        let relative = strip_root(&path, ROOT)?;
        if entry.is_dir() {
            if relative.as_os_str().is_empty() || keep_win(&relative) {
                fs::create_dir_all(version_dir.join(&relative))
                    .map_err(|e| format!("创建目录失败：{e}"))?;
            }
            continue;
        }
        if !keep_win(&relative) {
            continue;
        }
        unpacked_bytes = unpacked_bytes.saturating_add(entry.size());
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            return Err("解包内容超过大小上限".into());
        }
        let target = version_dir.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
        }
        let mut out = fs::File::create(&target).map_err(|e| format!("写入文件失败：{e}"))?;
        io::copy(&mut entry, &mut out).map_err(|e| format!("写入文件失败：{e}"))?;
    }
    Ok(())
}

/// 用与官方 dist 相同的入口名重建 npm 入口（官方是符号链接，本解包器跳过）。
#[cfg(not(windows))]
fn write_darwin_shims(version_dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let bin = version_dir.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("创建 bin 目录失败：{e}"))?;
    let npm = bin.join("npm");
    // shebang 依赖 PATH 上的 node：所有调用方都把 node 目录前置到 PATH
    // （`command_with_path` + node 目录优先），与运行 pnpm 时的约定一致。
    fs::write(
        &npm,
        "#!/usr/bin/env node\nrequire('../lib/node_modules/npm/bin/npm-cli.js')\n",
    )
    .map_err(|e| format!("写入 npm shim 失败：{e}"))?;
    fs::set_permissions(&npm, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("设置 npm shim 权限失败：{e}"))?;
    Ok(())
}

/// Windows npm 入口：npm-cli.js 由解包提供，这里写一个 .cmd shim
/// （`process::script_*` 已有 ComSpec 通道，Windows 上 .cmd 可直接启动）。
#[cfg(windows)]
fn write_win_shims(version_dir: &Path) -> Result<(), String> {
    let npm_cmd = version_dir.join("npm.cmd");
    fs::write(
        &npm_cmd,
        "@echo off\r\n\"%~dp0node.exe\" \"%~dp0node_modules\\npm\\bin\\npm-cli.js\" %*\r\n",
    )
    .map_err(|e| format!("写入 npm.cmd shim 失败：{e}"))?;
    Ok(())
}

/// 解包 + 重建 npm 入口的平台分发：Windows 走 zip、其余平台走 tar.gz。
#[cfg(not(windows))]
fn extract_and_shim(archive: &Path, version_dir: &Path) -> Result<(), String> {
    extract_tarball(archive, version_dir)?;
    write_darwin_shims(version_dir)
}

#[cfg(windows)]
fn extract_and_shim(archive: &Path, version_dir: &Path) -> Result<(), String> {
    extract_zip(archive, version_dir)?;
    write_win_shims(version_dir)
}

/// 安装完成后清掉旧版本目录，避免外壳升级固定版本后磁盘上堆积多个版本。
/// 只删除 `v*` 前缀目录，不碰 tools/node 下的其他内容。
fn prune_old_versions(data_dir: &Path) {
    let root = managed_root(data_dir);
    let keep = managed_version_dir(data_dir);
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path != keep
                && path.is_dir()
                && path
                    .file_name()
                    .map(|n| n.to_string_lossy().starts_with('v'))
                    .unwrap_or(false)
            {
                let _ = fs::remove_dir_all(&path);
            }
        }
    }
}

/// 若托管运行时已安装则直接返回；否则下载官方产物、SHA-256 校验、
/// 解包最小集合并写入 npm 入口。进度消息同时落盘到
/// `<logs_dir>/<kind>-node-install-<date>.log` 并可经 `on_progress`
/// 推送给 UI。成功返回托管 node 可执行文件路径，失败返回带日志路径的
/// 可操作错误。
pub(crate) fn verify_or_install(
    data_dir: &Path,
    logs_dir: &Path,
    mut on_progress: impl FnMut(&str),
) -> Result<Option<PathBuf>, String> {
    if let Some(exe) = managed_node_exe(data_dir) {
        return Ok(Some(exe));
    }

    let spec = LogSpec::new(build_log_kind(), "node-install");
    let log_path = spec.path_for(logs_dir, &current_date_string());
    let mut log = RotatingLog::new(logs_dir, spec)
        .map_err(|e| format!("无法创建安装日志（{}）：{e}", log_path.display()))?;
    let mut step = |msg: &str| {
        let _ = log.write_line(msg);
        on_progress(msg);
    };

    let version_dir = managed_version_dir(data_dir);
    let artifact = artifact_name();
    let download_dir = data_dir.join("tools").join("downloads");
    fs::create_dir_all(&download_dir)
        .map_err(|e| format!("创建下载目录失败：{e}。日志：{}", log_path.display()))?;
    let tarball = download_dir.join(artifact);

    // 1) 校验文件
    step("正在获取官方校验文件 SHASUMS256.txt …");
    let shasums =
        http_get_string(&format!("{}/SHASUMS256.txt", dist_base()), None).map_err(|e| {
            format!(
                "获取 Node.js 校验文件失败：{e}。日志：{}",
                log_path.display()
            )
        })?;
    let expected = shasum_for(&shasums, artifact).ok_or_else(|| {
        format!(
            "校验文件中未找到 {}，可能是网络劫持或版本不匹配。日志：{}",
            artifact,
            log_path.display()
        )
    })?;

    // 2) 下载
    let size_hint = artifact_size_text();
    step(&format!(
        "正在下载官方 Node.js v{MANAGED_NODE_VERSION}（{size_hint}，需联网）…"
    ));
    http_get_file(&format!("{}/{}", dist_base(), artifact), &tarball)
        .map_err(|e| format!("下载 Node.js 失败：{e}。日志：{}", log_path.display()))?;

    // 3) SHA-256 校验（以官方 SHASUMS256.txt 为准）
    let actual = sha256_hex(&tarball)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "Node.js 下载校验失败（期望 {expected}，实际 {actual}）。已删除无效文件，可重试。日志：{}",
            log_path.display()
        ));
    }
    step("校验 SHA-256 通过");

    // 4) 解包到临时目录，成功后再发布
    step("正在解压并安装…");
    let temp_root = data_dir
        .join("tools")
        .join(format!(".node-tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(&temp_root)
        .map_err(|e| format!("创建临时目录失败：{e}。日志：{}", log_path.display()))?;
    let temp_version = temp_root.join(format!("v{MANAGED_NODE_VERSION}"));
    let extract_result = extract_and_shim(&tarball, &temp_version);
    if let Err(e) = extract_result {
        let _ = fs::remove_dir_all(&temp_root);
        let _ = fs::remove_file(&tarball);
        step(&format!("解压失败：{e}"));
        return Err(format!("解压失败：{e}。日志：{}", log_path.display()));
    }

    // 5) 发布 + 探测确认
    if let Some(parent) = version_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建托管目录失败：{e}。日志：{}", log_path.display()))?;
    }
    fs::rename(&temp_version, &version_dir)
        .map_err(|e| format!("发布托管运行时失败：{e}。日志：{}", log_path.display()))?;
    let _ = fs::remove_dir_all(&temp_root);
    let Some(exe) = managed_node_exe(data_dir) else {
        return Err(format!(
            "托管运行时缺少 node 可执行文件。日志：{}",
            log_path.display()
        ));
    };
    match crate::node::version_of(&exe) {
        Some(version) => {
            prune_old_versions(data_dir);
            step(&format!("Node.js {version} 已安装并校验可用"));
            Ok(Some(exe))
        }
        None => {
            let _ = fs::remove_dir_all(&version_dir);
            let _ = fs::remove_file(&tarball);
            Err(format!(
                "托管 Node.js v{MANAGED_NODE_VERSION} 无法运行（当前系统可能低于其最低版本要求），已回滚。请按提示手动安装 Node.js。日志：{}",
                log_path.display()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shasum_for_parses_two_space_separated_lines() {
        let lines = "abc  node-v24.20.0-darwin-x64.tar.gz\ndef  other.txt\n";
        assert_eq!(
            shasum_for(lines, "node-v24.20.0-darwin-x64.tar.gz"),
            Some("abc".into())
        );
        assert_eq!(shasum_for(lines, "missing"), None);
    }

    #[test]
    fn artifact_names_match_pinned_version() {
        assert_eq!(
            artifact_name(),
            if cfg!(windows) {
                "node-v24.20.0-win-x64.zip"
            } else {
                "node-v24.20.0-darwin-x64.tar.gz"
            }
        );
    }

    /// 端到端冒烟：真实下载官方产物 → SHA-256 校验 → 最小解包 → 探测，
    /// 并验证 npm shim 在 PATH 注入后可用。
    /// 默认跳过（需联网下载约 52 MB）：`cargo test managed_node_e2e -- --ignored`
    #[test]
    #[ignore = "需要联网下载官方 Node 二进制（约 52 MB）"]
    fn managed_node_e2e_downloads_installs_and_probes() {
        let root = std::env::temp_dir().join(format!("dsh-node-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let logs = root.join("logs");
        let mut messages = Vec::new();
        let exe = verify_or_install(&root, &logs, |m| messages.push(m.to_string()))
            .expect("托管安装必须成功");
        let exe = exe.expect("必须产生 node 可执行文件");
        assert!(exe.is_file());
        let info = crate::node::probe(&exe);
        assert!(info.ok, "probe 必须接受已安装的 node：{}", info.reason);
        assert!(messages.iter().any(|m| m.contains("已安装")));
        let today = crate::process::current_date_string();
        let kind = crate::process::build_log_kind();
        let log = logs.join(format!("{kind}-node-install-{today}.log"));
        assert!(log.is_file(), "安装日志必须落盘：{}", log.display());

        #[cfg(not(windows))]
        {
            // npm shim 需要 PATH 上的 node（与内核启动的注入约定一致）。
            let bin = exe.parent().expect("bin 目录");
            let npm = bin.join("npm");
            assert!(npm.is_file(), "npm shim 必须存在");
            let path = format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let out = std::process::Command::new(&npm)
                .arg("--version")
                .env("PATH", path)
                .output()
                .expect("npm shim 必须能启动");
            assert!(
                out.status.success(),
                "npm --version 失败：{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn strip_root_rejects_traversal_and_absolute() {
        assert_eq!(
            strip_root(
                Path::new("node-v24.20.0-darwin-x64/bin/node"),
                "node-v24.20.0-darwin-x64",
            )
            .unwrap(),
            Path::new("bin/node")
        );
        assert!(strip_root(Path::new("../evil"), "root").is_err());
        assert!(strip_root(Path::new("/abs"), "root").is_err());
        assert!(strip_root(Path::new("other/file"), "root").is_err());
    }
}
