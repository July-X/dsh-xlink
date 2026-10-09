//! 内嵌 openai-oauth 插件的指纹计算与实例内物化。
//!
//! 物化配方来自 P0 调查 §5 的三版本实测：复制**运行时文件集**
//! （`host/`、`client/`、`locales/`、`package.json`——`test/` 与
//! README 不进实例）到「插件版本 + 内核依赖指纹」目录，并在目录内建
//! peer 链接把 `@deepseek-ai/dsh-llm` 指向目标内核树的同一份实现。
//! 对内核安装树**只读**：peer 链接指向它，绝不写入。
//!
//! 写入走「staging 目录 + 原子改名」：同路径重物化（开发期同版本更新
//! 源码）时才删除旧目录，且调用方契约要求实例已停止（设计 §3.1）。

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::plugins::center::{copy_tree, make_dir_link};

/// 运行时文件集：除此之外的包内容（test/、README）不进实例。
pub(crate) const RUNTIME_ENTRIES: [&str; 4] = ["host", "client", "locales", "package.json"];

/// 物化完成标记的文件名。内容是 JSON：`{pluginVersion, fingerprint, sourceHash}`。
pub(crate) const MARKER_FILE: &str = ".xlink-materialized.json";

/// 读 `<kernel>/node_modules/@deepseek-ai/<pkg>/package.json` 的版本。
fn dep_version(kernel_root: &Path, pkg: &str) -> Result<String, String> {
    let manifest = kernel_root
        .join("node_modules")
        .join("@deepseek-ai")
        .join(pkg)
        .join("package.json");
    let text = fs::read_to_string(&manifest).map_err(|error| {
        format!(
            "读取内核包清单失败（{}）：{error}；内核安装树可能不完整，请在「内核版本」页重装当前内核版本",
            manifest.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("内核包清单不是有效 JSON（{manifest:?}）：{error}"))?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("内核包清单缺 version 字段（{manifest:?}）"))
}

/// 内核依赖指纹：`dsh-<版本>-cordis-<版本>`，非常规字符折叠为 `-`。
///
/// 只取这两个包是因为它们决定 Host 插件的加载与适配器接口（P0 调查
/// §2：三版本的模型接口逐字节一致，差异面由 cordis 版本表达）。
pub(crate) fn kernel_fingerprint(kernel_root: &Path) -> Result<String, String> {
    let dsh = dep_version(kernel_root, "dsh")?;
    let cordis = dep_version(kernel_root, "cordis")?;
    let raw = format!("dsh-{dsh}-cordis-{cordis}");
    Ok(raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect())
}

/// 插件源目录里 `package.json` 声明的版本。
pub(crate) fn plugin_version(plugin_source: &Path) -> Result<String, String> {
    let manifest = plugin_source.join("package.json");
    let text = fs::read_to_string(&manifest).map_err(|error| {
        format!(
            "读取插件清单失败（{}）：{error}；应用资源可能不完整，请重新安装 dsh-xlink",
            manifest.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("插件清单不是有效 JSON（{manifest:?}）：{error}"))?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("插件清单缺 version 字段（{manifest:?}）"))
}

/// 相对路径（POSIX 分隔符）：接线行的 `name` 以 patch 文件所在目录锚定。
pub(crate) fn relative_entry(from_dir: &Path, target: &Path) -> String {
    let from: Vec<Component> = from_dir.components().collect();
    let to: Vec<Component> = target.components().collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from.len() {
        parts.push("..".into());
    }
    for part in &to[common..] {
        parts.push(part.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// 资源清单的文件名（由 `scripts/prepare-builtin-plugins.mjs` 生成）。
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

/// 校验资源清单（fail-closed，dev plan §6.2）：清单存在时逐文件核对
/// sha256 与字节数，任何缺失或不一致都报错——启用事务宁可不发生。
/// 仓库源码目录（dev 回退路径）没有清单，跳过校验：它就是源码本身、
/// 不是分发产物；分发产物一律经 prep 管线生成并必带清单（`check:builtin`
/// 钉住「产物必有清单且与源码一致」）。
pub(crate) fn verify_manifest(plugin_source: &Path) -> Result<(), String> {
    let manifest_path = plugin_source.join(MANIFEST_FILE);
    let Ok(text) = fs::read_to_string(&manifest_path) else {
        return Ok(());
    };
    let manifest: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        format!(
            "资源清单不是有效 JSON（{}）：{error}",
            manifest_path.display()
        )
    })?;
    let files = manifest
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("资源清单缺 files 数组（{}）", manifest_path.display()))?;
    if files.is_empty() {
        return Err(format!(
            "资源清单没有文件条目（{}）",
            manifest_path.display()
        ));
    }
    for entry in files {
        let path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("资源清单条目缺 path（{}）", manifest_path.display()))?;
        let file = plugin_source.join(path);
        let bytes = fs::read(&file).map_err(|error| {
            format!(
                "资源文件缺失或不可读（{}）：{error}；请重新安装 dsh-xlink 或重跑构建",
                file.display()
            )
        })?;
        let expected = entry.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
        use sha2::{Digest, Sha256};
        let actual = Sha256::digest(&bytes);
        let actual_hex: String = actual.iter().map(|b| format!("{b:02x}")).collect();
        if actual_hex != expected {
            return Err(format!(
                "资源 {} 与清单摘要不一致（{}，清单记 {expected}）；请重新安装 dsh-xlink",
                path,
                &actual_hex[..12.min(actual_hex.len())]
            ));
        }
        if let Some(listed) = entry.get("bytes").and_then(|v| v.as_u64()) {
            if listed != bytes.len() as u64 {
                return Err(format!(
                    "资源 {path} 大小与清单不一致（{}，清单记 {listed}）",
                    bytes.len()
                ));
            }
        }
    }
    Ok(())
}

/// 物化到 `target`（含 peer 链接与完成标记）。同名 staging 残留先清。
///
/// 已存在内容一致的目标目录时是幂等 no-op（标记匹配即返回）；不一致
/// （开发期源码更新）则 staging 构建后整体替换——调用方必须保证实例
/// 已停止，`target` 是本插件自有目录。
pub(crate) fn materialize(
    plugin_source: &Path,
    target: &Path,
    kernel_root: &Path,
    version: &str,
    fingerprint: &str,
) -> Result<(), String> {
    verify_manifest(plugin_source)?;
    let source_hash = runtime_digest(plugin_source).map_err(|e| {
        format!(
            "读取插件运行时文件失败（{}）：{e}；请重新构建或安装应用",
            plugin_source.display()
        )
    })?;
    let marker = serde_json::json!({ "pluginVersion": version, "fingerprint": fingerprint, "sourceHash": source_hash });
    if let Ok(existing) = fs::read_to_string(target.join(MARKER_FILE)) {
        let parsed: serde_json::Value = serde_json::from_str(&existing).unwrap_or_default();
        if parsed.get("pluginVersion").and_then(|v| v.as_str()) == Some(version)
            && parsed.get("fingerprint").and_then(|v| v.as_str()) == Some(fingerprint)
            && parsed.get("sourceHash").and_then(|v| v.as_str()) == Some(source_hash.as_str())
            && runtime_digest(target).ok().as_deref() == Some(source_hash.as_str())
        {
            ensure_peer_link(
                &target.join("node_modules").join("@deepseek-ai"),
                kernel_root,
            )?;
            return Ok(());
        }
    }

    let parent = target.parent().ok_or("物化目标目录没有父目录")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("创建物化父目录失败（{}）：{e}", parent.display()))?;
    let staging = parent.join(format!(
        ".staging-{}",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_dir_all(&staging);
    for entry in RUNTIME_ENTRIES {
        let source = plugin_source.join(entry);
        if !source.exists() {
            return Err(format!(
                "插件资源缺失（{}）；应用资源可能不完整，请重新安装 dsh-xlink",
                source.display()
            ));
        }
        let dest = staging.join(entry);
        if source.is_dir() {
            copy_tree(&source, &dest).map_err(|e| {
                format!(
                    "复制插件目录失败（{} → {}）：{e}",
                    source.display(),
                    dest.display()
                )
            })?;
        } else {
            fs::copy(&source, &dest).map_err(|e| {
                format!(
                    "复制插件文件失败（{} → {}）：{e}",
                    source.display(),
                    dest.display()
                )
            })?;
        }
    }
    ensure_peer_link(
        &staging.join("node_modules").join("@deepseek-ai"),
        kernel_root,
    )?;
    fs::write(staging.join(MARKER_FILE), format!("{marker}\n")).map_err(|e| {
        format!(
            "写物化标记失败（{}）：{e}",
            staging.join(MARKER_FILE).display()
        )
    })?;
    let _ = fs::remove_dir_all(target);
    fs::rename(&staging, target).map_err(|e| {
        format!(
            "物化目录落位失败（{} → {}）：{e}；重试前可在「查看日志」确认没有进程占用该目录",
            staging.display(),
            target.display()
        )
    })?;
    Ok(())
}

/// 摘要覆盖路径与文件字节，目录排序保证稳定；只扫描运行时入口，不跟随 peer 链接。
fn runtime_digest(root: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    fn visit(root: &Path, path: &Path, hash: &mut Sha256) -> io::Result<()> {
        let name = path.strip_prefix(root).unwrap().to_string_lossy();
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        if path.is_dir() {
            let mut entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                visit(root, &entry.path(), hash)?;
            }
        } else {
            let bytes = fs::read(path)?;
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    for entry in RUNTIME_ENTRIES {
        visit(root, &root.join(entry), &mut hash)?;
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Host 运行时要 peer 到内核树的 `@deepseek-ai` 包：
/// - `dsh-llm`：适配器基类（`BridgeAdapter extends LlmAdapter`）。
/// - `schemastery`：Config schema 的构造器。设置命名空间视图由各 profile
///   entry 的 Config schema 派生（`dsh-settings`：「Derive editable forms
///   from plugin Config schemas」）——host 不导出 Config，设置页就没有
///   我们的命名空间，provider 行永远 not-configured：不出卡、进不了
///   添加列表，client 的账户卡无处渲染（2026-10-09 用户实测）。
pub(crate) const PEER_PACKAGES: [&str; 2] = ["dsh-llm", "schemastery"];

/// 逐个 peer 包建立 `node_modules/@deepseek-ai/<pkg>` 链接；悬空或指向
/// 别处时重建。
fn ensure_peer_link(at_deepseek_dir: &Path, kernel_root: &Path) -> Result<(), String> {
    for package in PEER_PACKAGES {
        ensure_one_peer_link(at_deepseek_dir, kernel_root, package)?;
    }
    Ok(())
}

fn ensure_one_peer_link(
    at_deepseek_dir: &Path,
    kernel_root: &Path,
    package: &str,
) -> Result<(), String> {
    let source = kernel_root
        .join("node_modules")
        .join("@deepseek-ai")
        .join(package);
    if !source.is_dir() {
        return Err(format!(
            "内核树里找不到 {}；请先在「内核版本」页安装内核",
            source.display()
        ));
    }
    let link = at_deepseek_dir.join(package);
    match fs::symlink_metadata(&link) {
        Ok(metadata) => {
            // 旧链接存在 → 摘掉再重建（悬空或指向别处时靠这里纠正）。
            // 目录符号链接必须用 remove_dir：remove_file 对它报拒绝访问，
            // 以前这里吞掉删除错误，Windows 上旧链接删不掉，下面的
            // symlink_dir 就撞「os error 183 当文件已存在」——禁用再启用
            // 永远失败（2026-10-09 用户实测；与本机 materialize / wire_unwire
            // 两条单测同因）。删除失败必须如实上报，不许静默。
            let removed = if metadata.file_type().is_symlink() {
                fs::remove_dir(&link).or_else(|_| fs::remove_file(&link))
            } else if metadata.is_dir() {
                fs::remove_dir_all(&link)
            } else {
                fs::remove_file(&link)
            };
            removed.map_err(|e| format!("移除旧 peer 链接失败（{}）：{e}", link.display()))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("检查 peer 链接失败（{}）：{error}", link.display()));
        }
    }
    fs::create_dir_all(at_deepseek_dir)
        .map_err(|e| format!("创建 peer 目录失败（{}）：{e}", at_deepseek_dir.display()))?;
    make_dir_link(&source, &link).map_err(|e| {
        format!(
            "建立 peer 链接失败（{} → {}）：{e}；Windows 上若失败请开启开发者模式后重试",
            source.display(),
            link.display()
        )
    })
}

/// 已物化条目的对外描述（状态命令载荷用）。
pub(crate) struct MaterializedEntry {
    pub(crate) plugin_version: String,
    pub(crate) fingerprint: String,
    pub(crate) dir: PathBuf,
}

/// 已物化的条目（供状态命令枚举）：`<version>/<fingerprint>/<compat>` 三层。
pub(crate) fn find_materialized(base: &Path) -> Vec<MaterializedEntry> {
    let mut found = Vec::new();
    let versions = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(_) => return found,
    };
    for version in versions.flatten() {
        if !version.path().is_dir() || version.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let fingerprints = match fs::read_dir(version.path()) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for fingerprint in fingerprints.flatten() {
            if !fingerprint.path().is_dir() {
                continue;
            }
            let comps = match fs::read_dir(fingerprint.path()) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for compat in comps.flatten() {
                let dir = compat.path();
                if dir.join("host").join("index.js").is_file() {
                    found.push(MaterializedEntry {
                        plugin_version: version.file_name().to_string_lossy().into_owned(),
                        fingerprint: fingerprint.file_name().to_string_lossy().into_owned(),
                        dir,
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oop-mat-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fingerprint_sanitizes_and_reads_versions() {
        let root = temp_dir("fp");
        for (pkg, version) in [("dsh", "0.2.1-alpha.1"), ("cordis", "4.0.5-alpha.1")] {
            let dir = root.join("node_modules/@deepseek-ai").join(pkg);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("package.json"),
                format!("{{\"version\":\"{version}\"}}"),
            )
            .unwrap();
        }
        assert_eq!(
            kernel_fingerprint(&root).unwrap(),
            "dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn relative_entry_walks_up_and_down() {
        let base = Path::new("/h");
        assert_eq!(
            relative_entry(
                &base.join("profiles/web"),
                &base.join("extensions/b/openai-oauth/0.1.0/f/c/host/index.js")
            ),
            "../../extensions/b/openai-oauth/0.1.0/f/c/host/index.js"
        );
    }

    #[test]
    fn materialize_is_idempotent_and_rebuilds_on_version_change() {
        let root = temp_dir("mat");
        let source = root.join("source");
        for entry in RUNTIME_ENTRIES {
            if entry == "package.json" {
                fs::write(
                    source.join("package.json"),
                    "{\"name\":\"xlink-openai-oauth\",\"version\":\"0.1.0\"}",
                )
                .unwrap();
            } else {
                fs::create_dir_all(source.join(entry)).unwrap();
                fs::write(source.join(entry).join("keep.js"), "// keep").unwrap();
            }
        }
        let kernel = root.join("kernel/node_modules/@deepseek-ai");
        for pkg in PEER_PACKAGES {
            fs::create_dir_all(kernel.join(pkg)).unwrap();
        }
        let target = root.join("instance/extensions/builtin/openai-oauth/0.1.0/fp-1/compat-a");
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        assert!(target.join("host/index.js").is_file() || target.join("host/keep.js").is_file());
        assert!(target.join(MARKER_FILE).is_file());
        // 幂等：标记匹配时不清重建（靠 mtime 不稳，改验 staging 不残留）。
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        assert!(!target.parent().unwrap().join(".staging-compat-a").exists());
        // 同版本源码改变也必须更新；新增图片模块不能被旧标记挡住。
        fs::write(source.join("host/keep.js"), "// updated").unwrap();
        fs::write(source.join("host/images.js"), "export {}").unwrap();
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        assert_eq!(
            fs::read_to_string(target.join("host/keep.js")).unwrap(),
            "// updated"
        );
        assert!(target.join("host/images.js").is_file());
        // 源码删文件与目标损坏都应收敛到当前运行时文件集。
        fs::remove_file(source.join("host/images.js")).unwrap();
        fs::write(target.join("host/keep.js"), "// stale").unwrap();
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        assert!(!target.join("host/images.js").exists());
        assert_eq!(
            fs::read_to_string(target.join("host/keep.js")).unwrap(),
            "// updated"
        );
        // 旧标记缺摘要，升级时重建一次，之后仍保持幂等。
        fs::write(
            target.join(MARKER_FILE),
            r#"{"pluginVersion":"0.1.0","fingerprint":"fp-1"}"#,
        )
        .unwrap();
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        let marker: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(target.join(MARKER_FILE)).unwrap()).unwrap();
        assert!(marker["sourceHash"].as_str().is_some());
        // 版本变化 → 新指纹目录，旧目录不动。
        materialize(
            &source,
            &target.with_file_name("x"),
            &root.join("kernel"),
            "0.2.0",
            "fp-2",
        )
        .unwrap();
        assert!(target.exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// 清单校验：坏摘要 / 缺文件都要拦下；没有清单（dev 源码）放行。
    #[test]
    fn manifest_verification_is_fail_closed() {
        let root = temp_dir("manifest");
        let source = root.join("source/host");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("index.js"), "export {}").unwrap();
        let digest = {
            use sha2::{Digest, Sha256};
            let bytes = fs::read(source.join("index.js")).unwrap();
            Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let manifest = serde_json::json!({
            "files": [
                { "path": "host/index.js", "sha256": digest, "bytes": 9 }
            ]
        });
        // 无清单 → 放行（dev 源码路径）。
        assert!(verify_manifest(&root.join("source")).is_ok());
        // 清单与文件一致 → 放行。
        fs::write(
            root.join("source").join(MANIFEST_FILE),
            format!("{manifest}\n"),
        )
        .unwrap();
        assert!(verify_manifest(&root.join("source")).is_ok());
        // 文件被改 → 拦下（fail-closed）。
        fs::write(source.join("index.js"), "export { tampered }").unwrap();
        let error = verify_manifest(&root.join("source")).unwrap_err();
        assert!(error.contains("摘要不一致"), "{error}");
        // 清单列了不存在的文件 → 拦下。
        fs::remove_file(source.join("index.js")).unwrap();
        let error = verify_manifest(&root.join("source")).unwrap_err();
        assert!(error.contains("缺失或不可读"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }
}
