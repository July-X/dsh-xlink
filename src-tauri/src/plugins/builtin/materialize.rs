//! 内嵌插件的指纹计算与实例内物化。
//!
//! 物化配方来自 P0 调查 §5 的三版本实测：复制**运行时文件集**到
//! 「插件版本 + 内核依赖指纹」目录，并在目录内建 peer 链接把用到的
//! `@deepseek-ai` 包指向目标内核树的同一份实现。**文件集与 peer 列表都是
//! 参数**（2026-10-12 起随第二个内置插件进来）：它们按插件各写一份写死在
//! 顶部时，加一个不发布 client 卡、不带本地化文案的插件就得先在这里编一个
//! 假的 `client/` 目录出来。对内核安装树**只读**：peer 链接指向它，绝不写入。
//!
//! 内核树里没有的 peer 分两种情形（[`super::peers`] 的必需 / 可选语义）：必需
//! 的缺失就是装不上；可选的由 [`super::peer_supply`] 在**物化之前**从 npm 补一份
//! （版本跟内核一致），补到了就在这里**复制**进插件目录，补不到才跳过链接、由
//! host 降级。
//!
//! 写入走「staging 目录 + 原子改名」：同路径重物化（开发期同版本更新
//! 源码）时才删除旧目录，且调用方契约要求实例已停止（设计 §3.1）。

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::plugins::builtin::builtins::BuiltinSpec;
use crate::plugins::center::{copy_tree, make_dir_link};

/// 资源清单的文件名（由 `scripts/prepare-builtin-plugins.mjs` 生成）。
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

/// 物化完成标记的文件名。内容是 JSON：`{pluginVersion, fingerprint, sourceHash}`。
pub(crate) const MARKER_FILE: &str = ".xlink-materialized.json";

/// 读 `<kernel>/node_modules/@deepseek-ai/<pkg>/package.json` 的版本。
///
/// 实现只有一份（[`super::peers::package_version`]）：指纹、peer 供货、物化三处
/// 都要读它，各拼一份路径的漂移症状是「指纹算的是一个包、链接指到另一个」。
fn dep_version(kernel_root: &Path, pkg: &str) -> Result<String, String> {
    super::peers::package_version(kernel_root, pkg)
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
/// （开发期源码更新 / 上游换了版本）则 staging 构建后整体替换——调用方必须
/// 保证实例已停止，`target` 是本插件自有目录。
///
/// `upstream_dir`：有上游的插件（ponytail）额外把那里的 `skills/` 与 `hooks/`
/// 复制进来。**它必须进摘要**（见 [`runtime_digest`]），否则同步到新上游版本后
/// 摘要不变 → 判定为「内容一致」→ 直接返回 → 用户手上的永远是旧版本——
/// 而面板与日志都说更新成功了。这正是本次改动要消灭的那类静默分叉。
pub(crate) fn materialize(
    spec: &BuiltinSpec,
    plugin_source: &Path,
    upstream_dir: Option<&Path>,
    target: &Path,
    kernel_root: &Path,
    dsh_home: &Path,
    version: &str,
    fingerprint: &str,
) -> Result<(), String> {
    let upstream_dir = match spec.upstream {
        Some(_) => Some(upstream_dir.ok_or_else(|| {
            format!(
                "{} 的内容来自上游，但还没有同步过任何版本；\
                 请先在插件面板启用它（启用时会自动下载），或点「检查更新」",
                spec.display_name
            )
        })?),
        None => None,
    };
    verify_manifest(plugin_source)?;
    let source_hash = runtime_digest(spec, plugin_source, upstream_dir).map_err(|e| {
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
            // 核验目标目录时两部分内容都在 `target` 里，所以同一个 `Some(target)`。
            && runtime_digest(spec, target, Some(target)).ok().as_deref() == Some(source_hash.as_str())
        {
            ensure_peer_link(
                spec,
                &target.join("node_modules").join("@deepseek-ai"),
                kernel_root,
                dsh_home,
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
    for entry in spec.entries {
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
    if let Some(from) = upstream_dir {
        for entry in crate::plugins::builtin::upstream::CONTENT_ENTRIES {
            let source = from.join(entry);
            if !source.exists() {
                // 上游这一版没有这个目录是**正常**的（例如某版不带 hook 脚本），
                // 不是「同步坏了」。如实跳过，别把它报成安装包不完整。
                continue;
            }
            copy_tree(&source, &staging.join(entry)).map_err(|e| {
                format!(
                    "复制上游内容失败（{} → {}）：{e}；请在插件面板点「检查更新」重试",
                    source.display(),
                    staging.join(entry).display()
                )
            })?;
        }
        // 补 CommonJS 标记：上游脚本是 require() 的 CJS，而插件根是
        // "type": "module"，Node 按**最近的** package.json 判模块类型。缺了它
        // require 抛 ERR_REQUIRE_ESM，而 **hook 失败是非阻塞的**——症状是
        // 「配好了但什么都没发生」，日志里只有一行 warn。
        let hooks = staging.join("hooks");
        if hooks.is_dir() {
            fs::write(
                hooks.join("package.json"),
                "{\n  \"type\": \"commonjs\"\n}\n",
            )
            .map_err(|e| format!("写 hooks/package.json 失败：{e}"))?;
        }
    }
    ensure_peer_link(
        spec,
        &staging.join("node_modules").join("@deepseek-ai"),
        kernel_root,
        dsh_home,
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
/// 运行时内容摘要：随包的 `entries` + （有上游的）上游内容目录。
///
/// **上游那部分必须进来**，否则「同步到了新版本」不会让摘要变化 → 幂等判定
/// 认为内容一致 → 直接返回 → 用户手上的永远是旧版本，而面板与日志都说更新成功。
///
/// `upstream_root` 为 `None` 而 spec 有上游时按「读不到」报错，不静默跳过。
fn runtime_digest(
    spec: &BuiltinSpec,
    root: &Path,
    upstream_root: Option<&Path>,
) -> io::Result<String> {
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
    for entry in spec.entries {
        visit(root, &root.join(entry), &mut hash)?;
    }
    if spec.upstream.is_some() {
        let upstream = upstream_root
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "上游内容目录未解析到"))?;
        for entry in crate::plugins::builtin::upstream::CONTENT_ENTRIES {
            let path = upstream.join(entry);
            // 上游某一版不带这个目录是正常的（与物化那步一致：缺就跳过）。
            if path.exists() {
                visit(upstream, &path, &mut hash)?;
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Host 运行时要 peer 到内核树的 `@deepseek-ai` 包，**按插件各列各的**
/// （清单在 `BuiltinSpec::peers`）。openai-oauth 要两个：
/// - `dsh-llm`：适配器基类（`BridgeAdapter extends LlmAdapter`）。
/// - `schemastery`：Config schema 的构造器。设置命名空间视图由各 profile
///   entry 的 Config schema 派生（`dsh-settings`：「Derive editable forms
///   from plugin Config schemas」）——host 不导出 Config，设置页就没有
///   我们的命名空间，provider 行永远 not-configured：不出卡、进不了
///   添加列表，client 的账户卡无处渲染（2026-10-09 用户实测）。
///
/// **不要图省事把包全列上**：多链一个包就多一条会断的链——旧内核装不上那个
/// 包时 `ensure_one_peer_link` 直接失败，于是「加了个用不到它的 peer」会让
/// 这个插件在新旧内核上彻底装不进去。
///
/// **必需 / 可选各按各的处置**（[`PeerSpec::required`]）：必需 peer 缺失是
/// 「这个内核装不上这个插件」，如实失败；**可选 peer 缺失只跳过链接**——
/// 内核会把包从 `dependencies` 降级成 `devDependencies`（实测
/// `dsh-hooks-claude-code` 在 `0.2.1-alpha.2` 就是这样），那时内核树里没有它
/// 不是用户环境坏了，而是这个内核版本不带那项能力。硬拦的代价是**整个插件
/// （含它自己的技能）一起装不进去**，而报错还把用户支去「内核版本」页重装一个
/// 没坏的内核（2026-10-12 用户实测：Ponytail 在 alpha.2 上根本打不开开关）。
/// 缺哪个由 `peers::missing_optional_peers` 报给面板，不在这里静默。
fn ensure_peer_link(
    spec: &BuiltinSpec,
    at_deepseek_dir: &Path,
    kernel_root: &Path,
    dsh_home: &Path,
) -> Result<(), String> {
    let plan = super::peer_supply::plan(spec, dsh_home, kernel_root);
    for peer in spec.peers {
        let entry = plan.iter().find(|entry| entry.package == peer.package);
        match entry {
            Some(entry) => ensure_peer(&entry.source, at_deepseek_dir, peer.package)?,
            None if peer.required => {
                return Err(format!(
                    "内核树里找不到 {}；当前内核版本不带 {} 这个包，壳也没能补上——\
                     请在「内核版本」页换一个内核版本后重试",
                    peer.path_in(kernel_root).display(),
                    peer.package
                ));
            }
            None => continue,
        }
    }
    // 补进来的包**自己**带的 peer / 依赖（`peer_supply::plan` 展开出来的那几条）：
    // 少了它们，补下来的包加载时会解析失败，而症状是「补了却还是没生效」。
    // 不在登记表里，因此缺了只跳过——能力降级那条路由 host 自己兜。
    for entry in &plan {
        if spec.peers.iter().any(|peer| peer.package == entry.package) {
            continue;
        }
        ensure_peer(&entry.source, at_deepseek_dir, &entry.package)?;
    }
    Ok(())
}

/// 按来源摆一个包：内核树里的建链接，壳补的那份**复制**。
///
/// 复制而不是链接的理由见 [`super::peer_supply::PeerSource`]：Node 按真实路径
/// 解析 import，链接过去的那份会在 `<DSH_HOME>/plugins/builtin/<id>/peers/` 下
/// 找不到 `node_modules`。
fn ensure_peer(
    source: &super::peer_supply::PeerSource,
    at_deepseek_dir: &Path,
    package: &str,
) -> Result<(), String> {
    match source {
        super::peer_supply::PeerSource::Kernel(dir) => {
            ensure_one_peer_link(dir, at_deepseek_dir, package)
        }
        super::peer_supply::PeerSource::Supplied(dir) => {
            ensure_supplied_copy(dir, at_deepseek_dir, package)
        }
    }
}

/// 把壳补的那份包复制进插件目录（版本一致时跳过，物化是幂等的）。
fn ensure_supplied_copy(
    source: &Path,
    at_deepseek_dir: &Path,
    package: &str,
) -> Result<(), String> {
    let target = at_deepseek_dir.join(package);
    if target.is_dir()
        && super::peers::manifest_version(&target).ok()
            == super::peers::manifest_version(source).ok()
    {
        return Ok(());
    }
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|e| format!("移除旧的 peer 副本失败（{}）：{e}", target.display()))?;
    }
    copy_tree(source, &target).map_err(|e| {
        format!(
            "复制内核包副本失败（{} → {}）：{e}",
            source.display(),
            target.display()
        )
    })
}

fn ensure_one_peer_link(
    source: &Path,
    at_deepseek_dir: &Path,
    package: &str,
) -> Result<(), String> {
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
    make_dir_link(source, &link).map_err(|e| {
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
        let spec = crate::plugins::builtin::builtins::find("openai-oauth").unwrap();
        let source = root.join("source");
        for entry in spec.entries {
            if *entry == "package.json" {
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
        for peer in spec.peers {
            fs::create_dir_all(kernel.join(peer.package)).unwrap();
        }
        let target = root.join("instance/extensions/builtin/openai-oauth/0.1.0/fp-1/compat-a");
        materialize(
            spec,
            &source,
            None,
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        assert!(target.join("host/index.js").is_file() || target.join("host/keep.js").is_file());
        assert!(target.join(MARKER_FILE).is_file());
        // 幂等：标记匹配时不清重建（靠 mtime 不稳，改验 staging 不残留）。
        materialize(
            spec,
            &source,
            None,
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        assert!(!target.parent().unwrap().join(".staging-compat-a").exists());
        // 同版本源码改变也必须更新；新增图片模块不能被旧标记挡住。
        fs::write(source.join("host/keep.js"), "// updated").unwrap();
        fs::write(source.join("host/images.js"), "export {}").unwrap();
        materialize(
            spec,
            &source,
            None,
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(target.join("host/keep.js")).unwrap(),
            "// updated"
        );
        assert!(target.join("host/images.js").is_file());
        // 源码删文件与目标损坏都应收敛到当前运行时文件集。
        fs::remove_file(source.join("host/images.js")).unwrap();
        fs::write(target.join("host/keep.js"), "// stale").unwrap();
        materialize(
            spec,
            &source,
            None,
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
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
        materialize(
            spec,
            &source,
            None,
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        let marker: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(target.join(MARKER_FILE)).unwrap()).unwrap();
        assert!(marker["sourceHash"].as_str().is_some());
        // 版本变化 → 新指纹目录，旧目录不动。
        materialize(
            spec,
            &source,
            None,
            &target.with_file_name("x"),
            &root.join("kernel"),
            &root,
            "0.2.0",
            "fp-2",
        )
        .unwrap();
        assert!(target.exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// 第二个内置插件的物化：**只复制它自己那份文件集**。
    ///
    /// 这条是「entries 从模块常量变成参数」的反向验。参数传错成 openai 的
    /// 那份时，`for entry in spec.entries` 会去 `source/client` 找一个根本
    /// 不存在的目录——而报错信息说的是「应用资源可能不完整，请重新安装」，
    /// 把一个源码侧的错误报成安装包坏了。
    ///
    /// 断言写的是**随包那份适配层**（`host` / `adapter`）而不是 `skills`：
    /// 2026-10-12 起上游内容不再随包走，物化的是适配层，内容另有来源。
    #[test]
    fn a_builtin_without_client_dirs_materializes_its_own_entry_set() {
        let root = temp_dir("ponytail");
        let spec = crate::plugins::builtin::builtins::find("ponytail").unwrap();
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        for entry in spec.entries {
            if *entry == "package.json" {
                fs::write(
                    source.join("package.json"),
                    "{\"name\":\"xlink-ponytail\",\"version\":\"0.1.0\"}",
                )
                .unwrap();
            } else if *entry == "LICENSE" || entry.contains('.') {
                // 文件型条目（LICENSE / package.json）：按文件写，别造一个同名
                // 目录——那样 `is_file()` 那条断言会红，而红的原因与被测行为无关。
                fs::write(source.join(entry), "MIT").unwrap();
            } else {
                fs::create_dir_all(source.join(entry)).unwrap();
                fs::write(source.join(entry).join("SKILL.md"), "# skill").unwrap();
            }
        }
        let kernel = root.join("kernel/node_modules/@deepseek-ai");
        for peer in spec.peers {
            fs::create_dir_all(kernel.join(peer.package)).unwrap();
        }
        assert!(
            !source.join("client").exists(),
            "这个夹具要真的没有 client 目录，否则测不到东西"
        );
        let target = root.join("instance/extensions/builtin/ponytail/0.1.0/fp-1/compat-a");
        // 上游内容从「已同步的那一版」取，而不是从随包资源取——这正是本次改动的
        // 核心：`source/skills` 不存在，而物化结果里**有** skills。
        let upstream = crate::plugins::builtin::upstream::seed_fixture(&root.join("home"), spec);
        assert!(
            !source.join("skills").exists(),
            "这个夹具要真的没有随包的 skills/——有它就测不出「内容来自上游」"
        );
        materialize(
            spec,
            &source,
            Some(&upstream),
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        assert!(target.join("host/SKILL.md").is_file());
        assert!(
            target.join("adapter/SKILL.md").is_file(),
            "适配层（adapter/）必须物化：它是 /ponytail-update 与技能路由的落点"
        );
        assert!(
            target.join("skills/ponytail/SKILL.md").is_file(),
            "上游内容必须物化进实例——漏掉的话工作台里没有技能，而界面上毫无异常"
        );
        assert!(
            target.join("hooks/claude-codex-hooks.json").is_file(),
            "上游的 hook 脚本也要物化：档位切换与会话级注入靠它"
        );
        assert!(
            target.join("hooks/package.json").is_file(),
            "必须补 commonjs 标记：上游脚本是 require() 的 CJS，\
             而插件根是 \"type\": \"module\"——缺了它 hook 静默失败"
        );
        assert!(
            !target.join("client").exists(),
            "别的插件才有的目录不该出现在这里"
        );
        assert!(target.join("LICENSE").is_file());
        // peer 链接**恰好**是它自己那几个：多一个就是多一条会断的链（旧内核装不上
        // 那个包时物化直接失败），少一个则是运行期 resolve 失败。这里比对整个
        // 集合而不是「第一个」——ponytail 有两个 peer，写死 `peers[0]` 的断言在
        // 加第二个 peer 时会**因为少断言而绿**。
        let mut linked: Vec<String> = fs::read_dir(target.join("node_modules/@deepseek-ai"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        linked.sort();
        let mut expected: Vec<String> = spec.peers.iter().map(|p| p.package.to_string()).collect();
        expected.sort();
        assert_eq!(linked, expected);
        let _ = fs::remove_dir_all(&root);
    }

    /// **上游换了版本必须真的重新物化。**
    ///
    /// 这是整个「跟上上游」改动里最容易被静默做坏的一处：幂等判定看的是
    /// `sourceHash`，若摘要只覆盖随包资源，那么「同步到了新版本」不会改变摘要 →
    /// 判定为内容一致 → 直接返回 → 用户手上的永远是旧版本，**而面板与日志都说
    /// 更新成功了**。这条测试就是钉住「上游内容进摘要」这件事。
    #[test]
    fn a_new_upstream_version_replaces_the_materialized_content() {
        let root = temp_dir("ponytail-bump");
        let spec = crate::plugins::builtin::builtins::find("ponytail").unwrap();
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        for entry in spec.entries {
            if *entry == "package.json" {
                fs::write(source.join("package.json"), "{}").unwrap();
            } else {
                fs::create_dir_all(source.join(entry)).unwrap();
                fs::write(source.join(entry).join("keep.js"), "// keep").unwrap();
            }
        }
        let kernel = root.join("kernel");
        let mut kernel_pkgs = kernel.join("node_modules/@deepseek-ai");
        fs::create_dir_all(&mut kernel_pkgs).unwrap();
        for peer in spec.peers {
            fs::create_dir_all(kernel.join("node_modules/@deepseek-ai").join(peer.package))
                .unwrap();
        }

        let home = root.join("home");
        let target = root.join("instance/extensions/builtin/ponytail/0.1.0/fp-1/compat-a");
        let first = crate::plugins::builtin::upstream::seed_fixture(&home, spec);
        materialize(
            spec,
            &source,
            Some(&first),
            &target,
            &kernel,
            &home,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        let before = fs::read_to_string(target.join("skills/ponytail/SKILL.md")).unwrap();
        assert!(before.contains("fixture"));

        // 同一版本再跑一次：必须是 no-op（幂等）。
        materialize(
            spec,
            &source,
            Some(&first),
            &target,
            &kernel,
            &home,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(target.join("skills/ponytail/SKILL.md")).unwrap(),
            before,
            "同一版本重复物化应当是 no-op"
        );

        // 上游换到更新的版本 → 内容必须换掉。
        let second = home.join("plugins/builtin/ponytail/upstream/9.9.10");
        fs::create_dir_all(second.join("skills/ponytail")).unwrap();
        fs::write(second.join("package.json"), "{}").unwrap();
        fs::write(
            second.join("skills/ponytail/SKILL.md"),
            "---\nname: ponytail\ndescription: 新版\n---\nnewer\n",
        )
        .unwrap();
        // 让 resolve_dir 选中新的那个（两个目录都在，版本号大的赢）。
        materialize(
            spec,
            &source,
            Some(
                crate::plugins::builtin::upstream::resolve_dir(&home, spec)
                    .unwrap()
                    .as_path(),
            ),
            &target,
            &kernel,
            &root,
            "0.1.0",
            "fp-1",
        )
        .unwrap();
        let after = fs::read_to_string(target.join("skills/ponytail/SKILL.md")).unwrap();
        assert!(
            after.contains("新版") && !after.contains("fixture"),
            "同步到新上游版本后物化内容必须换掉；实际拿到：{after:?}——\
             这就是「面板说更新成功、工作台里还是旧内容」那个静默分叉"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// 有上游的插件在**没同步过**时必须明确拒绝，而不是物化出一个空壳。
    ///
    /// 空壳的症状：工作台里没有技能、界面上看不出任何异常、用户以为插件已启用。
    /// 这是本次改动之前真实存在的形态，所以判据钉的就是「不许物化成功」。
    #[test]
    fn a_builtin_with_upstream_refuses_to_materialize_without_synced_content() {
        let root = temp_dir("ponytail-nosync");
        let spec = crate::plugins::builtin::builtins::find("ponytail").unwrap();
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        for entry in spec.entries {
            if *entry == "package.json" {
                fs::write(source.join("package.json"), "{}").unwrap();
            } else {
                fs::create_dir_all(source.join(entry)).unwrap();
                fs::write(source.join(entry).join("keep.js"), "// keep").unwrap();
            }
        }
        let kernel = root.join("kernel");
        fs::create_dir_all(kernel.join("node_modules/@deepseek-ai")).unwrap();
        for peer in spec.peers {
            fs::create_dir_all(kernel.join("node_modules/@deepseek-ai").join(peer.package))
                .unwrap();
        }
        let target = root.join("instance/extensions/builtin/ponytail/0.1.0/fp-1/compat-a");
        let err = materialize(
            spec, &source, None, &target, &kernel, &root, "0.1.0", "fp-1",
        )
        .expect_err("没有上游内容时必须拒绝物化");
        assert!(
            err.contains("上游") && err.contains("检查更新"),
            "错误信息要指出下一步（启用或点检查更新），实际：{err}"
        );
        assert!(
            !target.exists(),
            "拒绝时不该留下半个目录——用户会把它当成已启用"
        );
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

    /// ponytail 的物化夹具：源目录按 `entries` 生成，内核树里只摆 `kernel_peers`
    /// 列出的包，上游内容用 `seed_fixture`。
    fn ponytail_fixture(
        tag: &str,
        kernel_peers: &[&str],
    ) -> (PathBuf, &'static BuiltinSpec, PathBuf, PathBuf) {
        let root = temp_dir(tag);
        let spec = crate::plugins::builtin::builtins::find("ponytail").unwrap();
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        for entry in spec.entries {
            if *entry == "package.json" {
                fs::write(
                    source.join("package.json"),
                    "{\"name\":\"xlink-ponytail\",\"version\":\"0.1.0\"}",
                )
                .unwrap();
            } else if entry.contains('.') {
                fs::write(source.join(entry), "MIT").unwrap();
            } else {
                fs::create_dir_all(source.join(entry)).unwrap();
                fs::write(source.join(entry).join("keep.js"), "// keep").unwrap();
            }
        }
        let kernel = root.join("kernel");
        for package in kernel_peers {
            fs::create_dir_all(kernel.join("node_modules/@deepseek-ai").join(package)).unwrap();
        }
        let upstream = crate::plugins::builtin::upstream::seed_fixture(&root.join("home"), spec);
        (root, spec, source, upstream)
    }

    /// **内核不带可选 peer 时，物化必须照样成功**（2026-10-12 用户实测的那个 bug）。
    ///
    /// `@deepseek-ai/dsh-hooks-claude-code` 在 `0.2.1-alpha.2` 里被上游从运行时
    /// 依赖降级成 `devDependencies`，内核树里没有它。此前它是必需 peer，于是
    /// 「切换 PonyTail」整个失败——六个技能一起装不进去，而报错还把用户支去
    /// 「内核版本」页重装一个没坏的内核。这条钉住：缺的是可选 peer 时链接跳过、
    /// 物化成功，而**别的 peer 照链**（少链一个的后果是运行期 resolve 失败）。
    #[test]
    fn a_missing_optional_peer_is_skipped_instead_of_failing_the_materialize() {
        let (root, spec, source, upstream) = ponytail_fixture("optional-peer", &["dsh-skill"]);
        let target = root.join("instance/extensions/builtin/ponytail/0.1.0/fp-1/compat-a");
        materialize(
            spec,
            &source,
            Some(&upstream),
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .expect("内核不带可选 peer 时物化必须成功——否则整个插件都装不进去");
        let linked: Vec<String> = fs::read_dir(target.join("node_modules/@deepseek-ai"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            linked,
            vec!["dsh-skill".to_string()],
            "缺的可选 peer 跳过、在的 peer 照链；多链一个也会在旧内核上炸"
        );
        // 幂等那一步（标记命中后只补 peer 链接）走的是同一个函数，缺可选 peer
        // 时也不能在那里翻车。
        materialize(
            spec,
            &source,
            Some(&upstream),
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .expect("重复物化（走标记命中那条早退路径）同样不许因为缺可选 peer 而失败");
        let _ = fs::remove_dir_all(&root);
    }

    /// 壳补下来的可选 peer 要**复制**进物化目录（不是链接），而且它自己的 peer
    /// 也要一起摆好——Node 按真实路径解析 import，链接过去的那份会在
    /// `<DSH_HOME>/plugins/builtin/<id>/peers/` 下找不到 `node_modules`，症状是
    /// 「补了却还是没生效」。
    #[test]
    fn a_supplied_optional_peer_is_copied_with_its_own_peers() {
        let (root, spec, source, upstream) = ponytail_fixture("supplied-peer", &["dsh-skill"]);
        let kernel = root.join("kernel");
        // 内核版本读的是树里 `dsh` 自己的清单（补给供货目录的版本要与它一致）。
        let dsh = kernel.join("node_modules/@deepseek-ai/dsh");
        fs::create_dir_all(&dsh).unwrap();
        fs::write(dsh.join("package.json"), r#"{"version":"0.2.1-alpha.2"}"#).unwrap();
        // 内核树里**没有**桥，但壳已经补过一份（供货目录）；桥还要 protocol，
        // protocol 在树里。
        let supplied =
            root.join("plugins/builtin/ponytail/peers/dsh-hooks-claude-code@0.2.1-alpha.2");
        fs::create_dir_all(&supplied).unwrap();
        fs::write(
            supplied.join("package.json"),
            r#"{"name":"dsh-hooks-claude-code","version":"0.2.1-alpha.2",
                "peerDependencies":{"@deepseek-ai/dsh-hook-protocol":"0.2.1-alpha.2"}}"#,
        )
        .unwrap();
        fs::write(supplied.join("index.js"), "export const bridge = true\n").unwrap();
        let protocol = kernel.join("node_modules/@deepseek-ai/dsh-hook-protocol");
        fs::create_dir_all(&protocol).unwrap();
        fs::write(
            protocol.join("package.json"),
            r#"{"name":"dsh-hook-protocol","version":"0.2.1-alpha.2"}"#,
        )
        .unwrap();

        let target = root.join("instance/extensions/builtin/ponytail/0.1.0/fp-1/compat-a");
        materialize(
            spec,
            &source,
            Some(&upstream),
            &target,
            &kernel,
            &root,
            "0.1.0",
            "fp-1",
        )
        .expect("补过一份之后物化必须成功");

        let at_deepseek = target.join("node_modules/@deepseek-ai");
        let copied = at_deepseek.join("dsh-hooks-claude-code");
        assert!(
            fs::symlink_metadata(&copied).unwrap().is_dir()
                && !fs::symlink_metadata(&copied)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
            "补的那份必须是真目录：链接的真实路径在供货目录，那里没有 node_modules"
        );
        assert!(
            copied.join("index.js").is_file(),
            "复制要带上包的内容，不只是清单"
        );
        assert_eq!(
            fs::read_to_string(copied.join("index.js")).unwrap(),
            "export const bridge = true\n"
        );
        assert!(
            fs::symlink_metadata(at_deepseek.join("dsh-hook-protocol"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "桥自己的 peer 要按普通 peer 链到内核树（树里有就不用再补一份）"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// 缺**必需** peer 仍然失败，但文案要指对路：内核是装着的，别让人去重装。
    #[test]
    fn a_missing_required_peer_still_fails_with_a_message_that_fits_the_cause() {
        // 内核树里一个 peer 都不摆：`dsh-skill`（必需）缺失。
        let (root, spec, source, upstream) = ponytail_fixture("required-peer", &[]);
        let target = root.join("instance/extensions/builtin/ponytail/0.1.0/fp-1/compat-a");
        let error = materialize(
            spec,
            &source,
            Some(&upstream),
            &target,
            &root.join("kernel"),
            &root,
            "0.1.0",
            "fp-1",
        )
        .expect_err("必需 peer 缺失必须拦下：那种内核上技能根本注册不了");
        assert!(
            error.contains("dsh-skill") && error.contains("换一个内核版本"),
            "文案要点名缺哪个包、并给出正确的下一步（换内核版本），实际：{error}"
        );
        assert!(
            !error.contains("请先在「内核版本」页安装内核"),
            "内核是装着的（只是不带这个包），旧文案会把用户支去重装一个没坏的内核：{error}"
        );
        assert!(
            !target.exists(),
            "拒绝时不该留下半个目录——用户会把它当成已启用"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
