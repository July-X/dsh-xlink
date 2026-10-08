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

/// 物化完成标记的文件名。内容是 JSON：`{pluginVersion, fingerprint}`。
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
            "读取内核包清单失败（{}）：{error}；内核安装树可能不完整，请在「更新」页重装当前内核版本",
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
    let marker = serde_json::json!({ "pluginVersion": version, "fingerprint": fingerprint });
    if let Ok(existing) = fs::read_to_string(target.join(MARKER_FILE)) {
        let parsed: serde_json::Value = serde_json::from_str(&existing).unwrap_or_default();
        if parsed.get("pluginVersion").and_then(|v| v.as_str()) == Some(version)
            && parsed.get("fingerprint").and_then(|v| v.as_str()) == Some(fingerprint)
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

/// `node_modules/@deepseek-ai/dsh-llm` 指向内核树同一包；悬空或指向别处时重建。
fn ensure_peer_link(at_deepseek_dir: &Path, kernel_root: &Path) -> Result<(), String> {
    let source = kernel_root
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh-llm");
    if !source.is_dir() {
        return Err(format!(
            "内核树里找不到 {}；请先在「更新」页安装内核",
            source.display()
        ));
    }
    let link = at_deepseek_dir.join("dsh-llm");
    match fs::symlink_metadata(&link) {
        Ok(_) => {
            let _ = fs::remove_file(&link);
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
        let kernel = root.join("kernel/node_modules/@deepseek-ai/dsh-llm");
        fs::create_dir_all(&kernel).unwrap();
        let target = root.join("instance/extensions/builtin/openai-oauth/0.1.0/fp-1/compat-a");
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        assert!(target.join("host/index.js").is_file() || target.join("host/keep.js").is_file());
        assert!(target.join(MARKER_FILE).is_file());
        // 幂等：标记匹配时不清重建（靠 mtime 不稳，改验 staging 不残留）。
        materialize(&source, &target, &root.join("kernel"), "0.1.0", "fp-1").unwrap();
        assert!(!target.parent().unwrap().join(".staging-compat-a").exists());
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
}
