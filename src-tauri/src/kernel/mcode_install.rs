//! Install the verified MiniMax npm artifact into the current shell's kernel tree.

use std::path::{Path, PathBuf};

use crate::kernel::{kernel_adapter::McodeAdapter, kernel_deps, lifecycle};
use crate::shell::{
    atomic_write::{self, rename_exclusive},
    paths, process, settings,
};

const PACKAGE: &str = "@minimax-webui/minimax-code";
const VERSION: &str = "0.1.7";

#[derive(serde::Serialize)]
pub(crate) struct InstalledVersion {
    pub version: String,
    pub package_path: PathBuf,
    pub supported: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct AvailableVersion {
    pub version: String,
    /// Eligibility for the existing installer, not a claim of runtime validation.
    pub installable: bool,
    pub supported: bool,
    pub has_tarball: bool,
    pub has_digest: bool,
}

/// Registry version labels are untrusted and can never contain path syntax.
fn safe_version(version: &str) -> bool {
    if version.len() > 128 || version.is_empty() || !version.is_ascii() {
        return false;
    }
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let mut parts = core.split('.');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    }) && parts.next().is_none()
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
        && crate::shell::paths::validate_id_component(version).is_ok()
}

pub(crate) fn supported_version(version: &str) -> bool {
    version == VERSION
}

/// Enumerate only package manifests actually contained in a managed version directory.
pub(crate) fn installed_versions() -> Result<Vec<InstalledVersion>, String> {
    let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("无法读取 MiniMax 安装目录：{error}")),
    };
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法解析 MiniMax 安装目录：{error}"))?;
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("无法读取 MiniMax 安装目录：{error}"))?;
        let Some(version) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !safe_version(&version)
            || !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
        {
            continue;
        }
        let dir = entry
            .path()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !dir.starts_with(&root) {
            continue;
        }
        let package = dir.join("node_modules/@minimax-webui/minimax-code");
        let Ok(package) = package.canonicalize() else {
            continue;
        };
        if !package.starts_with(&dir) {
            continue;
        }
        let Ok(manifest_path) = package.join("package.json").canonicalize() else {
            continue;
        };
        if !manifest_path.starts_with(&package) {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&manifest_path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > 128 * 1024 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if manifest["name"].as_str() != Some(PACKAGE)
            || manifest["version"].as_str() != Some(version.as_str())
        {
            continue;
        }
        result.push(InstalledVersion {
            supported: supported_version(&version)
                && McodeAdapter::validate_package(&package).is_ok(),
            version,
            package_path: package,
        });
    }
    result.sort_by(|a, b| crate::shell::version::cmp_versions(&b.version, &a.version));
    Ok(result)
}

/// Fetch the existing registry metadata; downloads still independently verify the digest.
pub(crate) fn available_versions() -> Result<Vec<AvailableVersion>, String> {
    let (_, doc) = crate::pkg::fetch::fetch_npm_doc(PACKAGE)
        .map_err(|error| format!("无法查询 MiniMax 版本，请检查网络后重试：{error}"))?;
    let mut result: Vec<_> = doc
        .versions
        .into_iter()
        .filter_map(|(version, value)| {
            if !safe_version(&version) {
                return None;
            }
            let has_digest = value.dist.as_ref().is_some_and(|dist| {
                dist.integrity
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                    || dist.shasum.as_deref().is_some_and(|s| !s.trim().is_empty())
            });
            let has_tarball = value
                .dist
                .as_ref()
                .is_some_and(|dist| !dist.tarball.trim().is_empty());
            Some(AvailableVersion {
                installable: supported_version(&version) && has_digest && has_tarball,
                supported: supported_version(&version),
                has_tarball,
                version,
                has_digest,
            })
        })
        .collect();
    if result.is_empty() {
        return Err("npm 中没有可识别的 MiniMax 版本，请稍后重试".into());
    }
    result.sort_by(|a, b| crate::shell::version::cmp_versions(&b.version, &a.version));
    Ok(result)
}

pub(crate) fn install_lock() -> Result<crate::skills::store_lock::StoreLock, String> {
    let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
    std::fs::create_dir_all(&root).map_err(|e| format!("创建 MiniMax 安装目录失败：{e}"))?;
    crate::skills::store_lock::acquire(&root, crate::skills::store_lock::StealPolicy::Fail).map_err(
        |error| {
            format!(
                "无法取得 MiniMax 安装锁：{error}。锁文件：{}",
                root.join(crate::skills::store_lock::LOCK_FILE).display()
            )
        },
    )
}

/// 已装目录不会被自动覆盖：校验不过时说清要删哪个目录、下一步做什么。
fn installed_dir_hint(error: String, dir: &Path) -> String {
    format!(
        "{error}；已安装目录不会被自动覆盖，请删除该目录后重试：{}",
        dir.display()
    )
}

/// 把清单里那条指向暂存 tarball 的依赖改写成钉死的版本号。
///
/// pnpm 记的是 `file:<stage>/minimax.tgz`，而 stage 目录随安装结束被删——发布树
/// 里因此躺着一个死路径（有人在该目录跑一次 pnpm install 就会撞上）。改写发生在
/// 发布**之前**：那时失败还能整份暂存目录一起丢掉，不会留下半发布的树。
fn pin_published_dependency(kernel_dir: &Path) -> Result<(), String> {
    let manifest_path = kernel_dir.join("package.json");
    let bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("读取 MiniMax 安装清单失败：{error}"))?;
    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 MiniMax 安装清单失败：{error}"))?;
    let Some(dependencies) = manifest
        .get_mut("dependencies")
        .and_then(|value| value.as_object_mut())
    else {
        return Ok(());
    };
    if !dependencies.contains_key(PACKAGE) {
        return Ok(());
    }
    dependencies.insert(
        PACKAGE.to_string(),
        serde_json::Value::String(VERSION.to_string()),
    );
    let value = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("序列化安装清单失败：{error}"))?;
    atomic_write::atomic_write(&manifest_path, &value)
        .map_err(|error| format!("写回 MiniMax 安装清单失败：{error}"))
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
        // 已装目录不会被自动覆盖（「已安装目录只读」）：校验不过就是这份安装坏了——
        // 说清要删哪个目录，别把用户支去「重新安装」（那条路正被这里堵着）。
        McodeAdapter::validate_package(&final_package)
            .map_err(|error| installed_dir_hint(error.to_string(), &final_dir))?;
        McodeAdapter::validate_native(node, &final_package, &logs_dir)
            .map_err(|error| installed_dir_hint(error.to_string(), &final_dir))?;
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
        // 发布前把依赖钉成版本号：暂存 tarball 随安装结束被删，发布树里不该
        // 留着指向它的死路径。
        pin_published_dependency(&kernel)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_labels_cannot_escape_the_install_tree() {
        for value in ["0.1.7", "1.2.3-beta.1", "2.0.0+build"] {
            assert!(safe_version(value), "{value}");
        }
        for value in [
            "",
            "../0.1.7",
            "1.2.3/../../other",
            "1.2.3\\other",
            "1.2",
            "1.2.3:other",
        ] {
            assert!(!safe_version(value), "{value}");
        }
        assert!(supported_version("0.1.7"));
        assert!(!supported_version("0.1.8"));
    }

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

    #[test]
    fn install_lock_reclaims_a_lock_left_by_a_dead_shell() {
        // 壳崩在持锁期间（这把锁整段启动都握着）：锁文件留着、pid 已死。
        // 不回收的话，之后每次安装/启动都先等满 60 秒再失败，且没有出路。
        let home = std::env::temp_dir().join(format!(
            "mcode-install-lock-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let _home = crate::tests::scoped_xlink_home(&home);
        let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
        std::fs::create_dir_all(&root).unwrap();
        // 用一个**不可能存在**的 pid（Windows 的 pid 是 4 的倍数且远小于它，
        // Unix 的 pid 上限也远小于它）：「持有者已死」这条前提不会因为系统回收
        // pid 而被打破，用例不会偶发红。
        std::fs::write(
            root.join(crate::skills::store_lock::LOCK_FILE),
            i32::MAX.to_string(),
        )
        .unwrap();
        let started = std::time::Instant::now();
        let guard = install_lock().expect("死者的锁必须被回收");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "不能等满预算才回收死者的锁"
        );
        drop(guard);
        drop(_home);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 发布树里不能留着指向已删除暂存 tarball 的死路径：依赖必须钉成版本号。
    #[test]
    fn pinned_dependency_drops_the_dead_tarball_path() {
        let home =
            std::env::temp_dir().join(format!("mcode-pin-test-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&home).unwrap();
        let manifest = home.join("package.json");
        std::fs::write(
            &manifest,
            br#"{"name":"dsh-kernel-0_1_7","dependencies":{"@minimax-webui/minimax-code":"file:../../.install-abc/minimax.tgz"}}"#,
        )
        .unwrap();
        pin_published_dependency(&home).expect("pin");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(
            value["dependencies"]["@minimax-webui/minimax-code"].as_str(),
            Some("0.1.7"),
            "发布树里不能留着指向已删除暂存 tarball 的死路径"
        );
        // 没有这条依赖的清单不动它（不该凭空多出一条）。
        std::fs::write(&manifest, br#"{"name":"dsh-kernel-0_1_7"}"#).unwrap();
        pin_published_dependency(&home).expect("pin without dependency");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert!(value.get("dependencies").is_none());
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
