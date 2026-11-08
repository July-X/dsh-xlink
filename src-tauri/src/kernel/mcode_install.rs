//! Install the verified MiniMax npm artifact into the current shell's kernel tree.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::kernel::{kernel_adapter::McodeAdapter, kernel_deps, lifecycle};
use crate::shell::{
    atomic_write::{self, rename_exclusive},
    paths, process, settings,
};

const PACKAGE: &str = "@minimax-webui/minimax-code";
/// 默认安装版本：调用方没指定版本时用它。安装命令本身支持按版本安装，官方列表
/// 里的合法版本都能装——早期只认 0.1.7 的限制已按用户要求放宽（2026-11-08）。
const DEFAULT_VERSION: &str = "0.1.7";

pub(crate) fn default_version() -> &'static str {
    DEFAULT_VERSION
}

#[derive(serde::Serialize)]
pub(crate) struct InstalledVersion {
    pub version: String,
    pub package_path: PathBuf,
    pub supported: bool,
}

#[derive(Clone, serde::Serialize)]
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

/// 本壳能安装 / 能启动的版本。只要求版本号是合法的路径组件：重活（原生依赖
/// 能不能由当前 Node 加载）由 `McodeAdapter::validate_native` 在安装后实测，
/// 不该用一个写死的版本号来代替那道校验。
pub(crate) fn supported_version(version: &str) -> bool {
    safe_version(version)
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
            package_path: paths::plain_path(&package),
        });
    }
    result.sort_by(|a, b| crate::shell::version::cmp_versions(&b.version, &a.version));
    Ok(result)
}

/// 删除一个已装版本目录。只删 `<kernels>/<version>` 这一层；调用方负责检查
/// 它是不是当前使用中的版本、以及工作台是否已经停止。
pub(crate) fn remove_version(version: &str) -> Result<(), String> {
    if !safe_version(version) {
        return Err(format!("MiniMax 版本号不合法：{version}"));
    }
    let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
    let dir = root.join(version);
    if !dir.is_dir() {
        return Err(format!("MiniMax 版本 {version} 未安装"));
    }
    std::fs::remove_dir_all(&dir).map_err(|error| {
        format!(
            "删除 MiniMax 版本 {version} 失败：{error}；目录：{}",
            dir.display()
        )
    })
}

/// 已装包自己的版本号：概览与「当前使用」标记按这个显示，不再写死 0.1.7。
pub(crate) fn package_version(package: &Path) -> Option<String> {
    let bytes = std::fs::read(package.join("package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    manifest["version"].as_str().map(str::to_owned)
}

/// 官方发布列表的进程内缓存：与 DSH 的 `list_releases` 同档（60 秒）。
///
/// 官方版本页每次切到 MiniMax 都会自动拉一次（`watch(isMcode)`），没有缓存
/// 就是每切一次打一次 registry，用户看到的是「每次切换都在转圈」。只缓存成功
/// 结果：失败必须立刻可重试，不能把一次网络抖动记成「没有版本」。
static AVAILABLE_CACHE: Mutex<Option<(Instant, Vec<AvailableVersion>)>> = Mutex::new(None);
const AVAILABLE_CACHE_TTL: Duration = Duration::from_secs(60);

fn cached_available() -> Option<Vec<AvailableVersion>> {
    let guard = AVAILABLE_CACHE.lock().ok()?;
    let (stored_at, list) = guard.as_ref()?;
    if stored_at.elapsed() < AVAILABLE_CACHE_TTL {
        Some(list.clone())
    } else {
        None
    }
}

fn store_available(list: &[AvailableVersion]) {
    if let Ok(mut guard) = AVAILABLE_CACHE.lock() {
        *guard = Some((Instant::now(), list.to_vec()));
    }
}

#[cfg(test)]
fn clear_available_cache() {
    if let Ok(mut guard) = AVAILABLE_CACHE.lock() {
        *guard = None;
    }
}

/// 缓存命中直接复用，未命中才走网络；fetch 作为参数是为了测试能注入计数闭包。
fn cached_or_fetch_available<F>(fetch: F) -> Result<Vec<AvailableVersion>, String>
where
    F: FnOnce() -> Result<Vec<AvailableVersion>, String>,
{
    if let Some(cached) = cached_available() {
        return Ok(cached);
    }
    let list = fetch()?;
    store_available(&list);
    Ok(list)
}

/// Fetch the existing registry metadata; downloads still independently verify the digest.
pub(crate) fn available_versions() -> Result<Vec<AvailableVersion>, String> {
    cached_or_fetch_available(fetch_available_versions)
}

fn fetch_available_versions() -> Result<Vec<AvailableVersion>, String> {
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
fn pin_published_dependency(kernel_dir: &Path, version: &str) -> Result<(), String> {
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
        serde_json::Value::String(version.to_string()),
    );
    let value = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("序列化安装清单失败：{error}"))?;
    atomic_write::atomic_write(&manifest_path, &value)
        .map_err(|error| format!("写回 MiniMax 安装清单失败：{error}"))
}

pub(crate) fn install(
    node: &Path,
    pnpm: &Path,
    version: &str,
    progress: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let root = paths::family_runtime_dir("mcode", settings::current_mode()).join("kernels");
    std::fs::create_dir_all(&root).map_err(|e| format!("创建 MiniMax 安装目录失败：{e}"))?;
    let root = paths::plain_path(&root.canonicalize().map_err(|e| e.to_string())?);
    let final_dir = root.join(version);
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
        let (downloaded, registry) = crate::pkg::fetch::fetch_verified_npm_tarball(
            PACKAGE,
            Some(version),
            &tarball,
            progress,
        )?;
        if downloaded != version {
            return Err("MiniMax 下载版本与请求不一致".into());
        }
        let kernel = stage.join("kernel");
        std::fs::create_dir(&kernel).map_err(|e| e.to_string())?;
        kernel_deps::write_mcode_stub(&kernel, version)
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
                // pnpm 的 side-effects 缓存不区分 Node ABI：换过 Node 版本后
                // 复用的是旧构建产物，node-pty / better-sqlite3 会加载失败
                // （2026-11-08 实测：node 26 构建的 node-pty 在 node 24 下报错，
                // 安装界面只说「原生依赖无法由当前 Node 加载」）。
                "--config.side-effects-cache=false",
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
        pin_published_dependency(&kernel, version)?;
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
        // 安装已按版本进行：合法的 npm 版本号都算 Supported，非法版本号不算。
        assert!(supported_version("0.1.6"));
        assert!(!supported_version("../0.1.7"));
    }

    #[test]
    fn available_versions_cache_reuses_successes_and_never_caches_failures() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        clear_available_cache();
        let sample = vec![AvailableVersion {
            version: "0.1.7".into(),
            installable: true,
            supported: true,
            has_tarball: true,
            has_digest: true,
        }];
        let calls = AtomicUsize::new(0);
        let ok = |calls: &AtomicUsize| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample.clone())
        };

        let first = cached_or_fetch_available(|| ok(&calls)).expect("第一次");
        let second = cached_or_fetch_available(|| ok(&calls)).expect("第二次");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "TTL 内必须复用缓存");

        // 失败不写缓存：下一次调用必须真的重试，而不是把「没有版本」记下来。
        clear_available_cache();
        let failing = || -> Result<Vec<AvailableVersion>, String> { Err("网络失败".into()) };
        assert!(cached_or_fetch_available(failing).is_err());
        let after = cached_or_fetch_available(|| ok(&calls)).expect("失败后重试");
        assert_eq!(after.len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        clear_available_cache();
    }

    #[test]
    fn package_version_reads_the_manifest() {
        let root = std::env::temp_dir().join(format!(
            "mcode-package-version-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"@minimax-webui/minimax-code","version":"0.1.6"}"#,
        )
        .unwrap();
        assert_eq!(package_version(&root).as_deref(), Some("0.1.6"));
        assert_eq!(package_version(&root.join("missing")), None);
        std::fs::remove_dir_all(root).unwrap();
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
            .join(DEFAULT_VERSION);
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("preserve"), b"user data").unwrap();
        let result = install(
            Path::new("unused-node"),
            Path::new("unused-pnpm"),
            DEFAULT_VERSION,
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
        pin_published_dependency(&home, DEFAULT_VERSION).expect("pin");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(
            value["dependencies"]["@minimax-webui/minimax-code"].as_str(),
            Some("0.1.7"),
            "发布树里不能留着指向已删除暂存 tarball 的死路径"
        );
        // 没有这条依赖的清单不动它（不该凭空多出一条）。
        std::fs::write(&manifest, br#"{"name":"dsh-kernel-0_1_7"}"#).unwrap();
        pin_published_dependency(&home, DEFAULT_VERSION).expect("pin without dependency");
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
