//! Install the verified MiniMax npm artifact into the current shell's kernel tree.

use std::path::{Path, PathBuf};

use crate::kernel::{kernel_adapter::McodeAdapter, kernel_deps, lifecycle};
use crate::shell::{paths, process, settings};

const PACKAGE: &str = "@minimax-webui/minimax-code";
const VERSION: &str = "0.1.7";

pub(crate) fn install_lock() -> Result<crate::skills::store_lock::StoreLock, String> {
    let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
    std::fs::create_dir_all(&root).map_err(|e| format!("创建 MiniMax 安装目录失败：{e}"))?;
    crate::skills::store_lock::acquire(&root, crate::skills::store_lock::StealPolicy::Fail)
        .map_err(|error| format!("无法取得 MiniMax 安装锁：{error}"))
}

pub(crate) fn install(
    node: &Path,
    pnpm: &Path,
    progress: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
    std::fs::create_dir_all(&root).map_err(|e| format!("创建 MiniMax 安装目录失败：{e}"))?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let final_dir = root.join(VERSION);
    let final_package = final_dir.join("node_modules/@minimax-webui/minimax-code");
    let logs_dir = paths::shell_logs_dir(settings::current_mode());
    if final_dir.exists() {
        McodeAdapter::validate_package(&final_package).map_err(|e| e.to_string())?;
        McodeAdapter::validate_native(node, &final_package, &logs_dir)
            .map_err(|e| e.to_string())?;
        return Ok(final_package);
    }

    let stage = root.join(format!(".install-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&stage).map_err(|e| format!("创建 MiniMax 安装暂存目录失败：{e}"))?;
    let result = (|| {
        let tarball = stage.join("minimax.tgz");
        let (version, registry) = crate::pkg::fetch::fetch_verified_npm_tarball(
            PACKAGE,
            Some(VERSION),
            &tarball,
            progress,
        )?;
        if version != VERSION {
            return Err("MiniMax 下载版本与请求不一致".into());
        }
        let kernel = stage.join("kernel");
        std::fs::create_dir(&kernel).map_err(|e| e.to_string())?;
        kernel_deps::write_mcode_stub(&kernel, VERSION)
            .map_err(|e| format!("准备 MiniMax 安装配置失败：{e}"))?;
        let file = tarball.to_str().ok_or("MiniMax 暂存路径无法由 pnpm 读取")?;
        #[cfg(windows)]
        let file = if let Some(unc) = file.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            file.strip_prefix(r"\\?\").unwrap_or(file).to_string()
        };
        let spec = format!("file:{file}");
        let node_dir = node.parent().ok_or("Node 路径无效")?;
        let pnpm_dir = pnpm.parent().ok_or("pnpm 路径无效")?;
        let log = process::LogSpec::new(process::build_log_kind(), "mcode-install");
        progress("正在安装 MiniMax 及其原生依赖…");
        let status = lifecycle::run_pnpm(
            pnpm,
            &[
                "add",
                "--config.node-linker=hoisted",
                "--config.package-import-method=copy",
                &spec,
            ],
            &kernel,
            &logs_dir,
            &log,
            &[node_dir, pnpm_dir],
            &registry,
            &mut *progress,
        )
        .map_err(|e| format!("启动 pnpm 失败：{e}；日志目录：{}", logs_dir.display()))?;
        if !status.success() {
            return Err(format!(
                "MiniMax 安装失败（{status}），请查看日志目录：{}",
                logs_dir.display()
            ));
        }
        let package = kernel.join("node_modules/@minimax-webui/minimax-code");
        McodeAdapter::validate_package(&package).map_err(|e| e.to_string())?;
        McodeAdapter::validate_native(node, &package, &logs_dir).map_err(|e| e.to_string())?;
        rename_exclusive(&kernel, &final_dir)
            .map_err(|e| format!("发布 MiniMax 安装失败：{e}；原有目录未覆盖"))?;
        McodeAdapter::validate_package(&final_package).map_err(|e| e.to_string())?;
        McodeAdapter::validate_native(node, &final_package, &logs_dir).map_err(|e| {
            format!(
                "发布后的 MiniMax 依赖检查失败：{e}；安装目录已保留：{}",
                final_dir.display()
            )
        })?;
        progress("MiniMax 安装完成");
        Ok(final_package)
    })();
    // Only remove the directory created by this invocation inside the canonical root.
    if stage.parent() == Some(root.as_path()) {
        if let Ok(metadata) = std::fs::symlink_metadata(&stage) {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                if let Err(error) = std::fs::remove_dir_all(&stage) {
                    let cleanup = format!(
                        "MiniMax 安装暂存目录清理失败：{error}；目录：{}",
                        stage.display()
                    );
                    return Err(match result {
                        Ok(_) => cleanup,
                        Err(original) => format!("{original}；{cleanup}"),
                    });
                }
            }
        }
    }
    result
}

#[cfg(windows)]
fn rename_exclusive(source: &Path, target: &Path) -> std::io::Result<()> {
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
fn rename_exclusive(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let target = CString::new(target.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    if unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn rename_exclusive(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("当前平台不支持 MiniMax 安装"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_installation_is_never_replaced_or_removed() {
        let home = std::env::temp_dir().join(format!(
            "mcode-install-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let _home = crate::tests::scoped_xlink_home(&home);
        let existing = paths::family_runtime_dir("mcode", settings::current_mode())
            .join("kernels")
            .join(VERSION);
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("preserve"), b"user data").unwrap();
        let result = install(
            Path::new("unused-node"),
            Path::new("unused-pnpm"),
            &mut |_| {},
        );
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(existing.join("preserve")).unwrap(),
            b"user data"
        );
        drop(_home);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn publish_refuses_to_replace_an_existing_directory() {
        let root = std::env::temp_dir().join(format!(
            "mcode-publish-test-{:032x}",
            rand::random::<u128>()
        ));
        let source = root.join("new");
        let target = root.join("existing");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("new"), b"new").unwrap();
        std::fs::write(target.join("existing"), b"existing").unwrap();
        assert!(rename_exclusive(&source, &target).is_err());
        assert_eq!(std::fs::read(source.join("new")).unwrap(), b"new");
        assert_eq!(std::fs::read(target.join("existing")).unwrap(), b"existing");
        std::fs::remove_dir_all(root).unwrap();
    }
}
