//! 包取源公共层：npm registry 文档、tarball 解包与 integrity 校验、git tag
//! 解析、semver 判定、安装 spec 拆分，以及「取源 → 校验 → 发布」三段式换名
//! 所用的暂存目录。
//!
//! 这些函数原先在 `plugins.rs`（社区插件）与 `skills.rs`（社区技能）里各存一
//! 份，逐字重复的有 `latest_tag` / `looks_like_semver` / `split_npm_spec` /
//! `fetch_npm_doc` / `extract_tarball` / `stamp_id_marker` / `remove_link` 等
//! 十余处。重复代码的真正代价不是行数，而是**漂移**：改一处 bug 时另一处
//! 悄悄保持旧行为（`is_newer_than` 与 `split_npm_spec` 都已经各自漂移过一次，
//! 只在注释里互指「与插件中央库一致」）。现在取源行为只有一份实现。
//!
//! 本模块只返回纯文本原因（`String`），错误类型由调用方决定：插件侧包成
//! `AppError::Plugin`、技能侧包成 `AppError::Skill`。这样共享层不需要知道
//! 自己是给谁用的，两边的用户可见文案仍各自可控。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::error::AppError;
use crate::process::atomic_write;
use crate::releases::{http_get_file, http_get_string, verify_download_integrity};
use crate::version::cmp_versions;

/// 每个中央库条目内的取源标记文件：记录 id / 来源 / 版本，供对账与
/// 「这个目录是谁的」判定使用。
pub const SOURCE_MARKER: &str = ".dsh-source.json";

/// 暂存目录内部的归属标记文件。对账流程据此把 `.tmp-*` / `.new-*` /
/// `.backup-*` 目录归到某个 id，而不必从目录名解析（目录名可能含 `-`）。
pub const ID_MARKER: &str = ".dsh-id";

// --- npm registry 文档 ------------------------------------------------------

/// npm registry 文档中我们关心的子集。
#[derive(Debug, Deserialize)]
pub struct NpmDoc {
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    pub versions: BTreeMap<String, NpmVersionDoc>,
}

#[derive(Debug, Deserialize)]
pub struct NpmVersionDoc {
    #[serde(default)]
    pub dist: Option<NpmDist>,
}

#[derive(Debug, Deserialize)]
pub struct NpmDist {
    #[serde(default)]
    pub tarball: String,
    /// registry 声明的 SRI 摘要（`sha512-<base64>`）。外壳自己下载 tarball，
    /// 必须据此校验一次：默认 registry 是第三方镜像，packument 与 tarball
    /// 同源，镜像可以在元数据一致的前提下替换内容，而解包后 pnpm 会执行包里的
    /// `prepare` 脚本。
    #[serde(default)]
    pub integrity: Option<String>,
}

/// 拉取某个包的 npm registry 文档。
pub fn fetch_npm_doc(name: &str) -> Result<NpmDoc, String> {
    let url = format!("{}{}", crate::registry::npm_registry_base(), name);
    let body = http_get_string(&url, None)?;
    serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())
}

/// 把 pin 解析为 `versions` 索引里具体版本号，并返回给用户看的标签。
///
/// - `pin = None` → 查 `dist-tags.latest`，缺省回退为 ""
/// - `pin = Some(tag)`，且 `tag` 命中 dist-tag → 使用该 tag 指向的版本
/// - `pin = Some(ver)`，且 `ver` 未命中任何 dist-tag → 把 pin 当字面量
///   semver 使用（调用方会以清晰的错误信息暴露 `versions[ver]` 查询失败）
///
/// 少了 dist-tag 这一步，`@latest` / `@next` 会被当成 `versions` 的 key，
/// 查询返回 `None`，用户会看到「npm 上 <包>@latest 没有可下载的 tarball」，
/// 即便该版本早已发布。
pub fn resolve_npm_version(doc: &NpmDoc, pin: Option<&str>) -> (String, String) {
    match pin {
        Some(tag) => (
            doc.dist_tags
                .get(tag)
                .cloned()
                .unwrap_or_else(|| tag.to_string()),
            tag.to_string(),
        ),
        None => (
            doc.dist_tags.get("latest").cloned().unwrap_or_default(),
            "latest".to_string(),
        ),
    }
}

/// 从 npm registry 取一个包并解包到 `dest`：查文档 → 解析版本 → 下载 →
/// 校验 integrity → 解包。成功时返回实际安装的版本，任一步失败都返回
/// 可直接展示的中文原因。
pub fn fetch_npm_package(
    source: &str,
    pin: Option<&str>,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, String> {
    on_progress(&format!("正在查询 npm registry：{source}"));
    let doc = fetch_npm_doc(source).map_err(|e| format!("查询 npm 失败：{e}"))?;
    let (version, label) = resolve_npm_version(&doc, pin);
    if version.is_empty() {
        return Err(format!("npm 上找不到包 {source} 或其 {label} 标记"));
    }
    let dist = doc.versions.get(&version).and_then(|v| v.dist.as_ref());
    let integrity = dist.and_then(|d| d.integrity.clone());
    let tarball = dist
        .map(|d| d.tarball.clone())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| format!("npm 上 {source}@{version} 没有可下载的 tarball"))?;
    on_progress(&format!("正在下载 {source}@{version} …"));
    let tgz = dest.join(".pkg.tgz");
    http_get_file(&tarball, &tgz).map_err(|e| format!("下载失败：{e}"))?;
    match verify_download_integrity(&tgz, integrity.as_deref()) {
        Ok(Some(algorithm)) => on_progress(&format!("已校验下载内容的 integrity（{algorithm}）")),
        Ok(None) => {
            on_progress("registry 未提供 integrity（也没有可用的 shasum），本次未做内容校验")
        }
        Err(reason) => {
            let _ = fs::remove_file(&tgz);
            return Err(format!(
                "{reason}。为避免安装未经验证的内容已中止；请重试或改用官方 registry（DSH_NPM_REGISTRY）"
            ));
        }
    }
    // 通过共享的 Rust 归档处理器解包。它会校验 npm 的 `package/` 根，并把其
    // 子项发布到 `dest`，后续的校验/扫描/物化步骤就从这里读取。
    extract_tarball(&tgz, dest)
        .map_err(|e| format!("解包失败：{e}（请确认下载内容完整后重试）"))?;
    let _ = fs::remove_file(&tgz);
    Ok(version)
}

/// 将 npm tgz 解压到 `dest`，并剥掉其顶层的 `package/` 段。共享的 Rust
/// 解压器会拒绝路径穿越、链接和特殊文件，并同时限制条目数量和声明的
/// 解压后大小，再做发布。
pub fn extract_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    crate::archive::extract_gzip_tarball(tarball, dest)
}

// --- 版本判定 ---------------------------------------------------------------

/// 在 tag 候选中挑出最高版本，若无则返回 `None`。
pub fn latest_tag<'a>(tags: impl Iterator<Item = &'a str>) -> Option<String> {
    tags.filter_map(|t| {
        let stripped = t.strip_prefix('v').unwrap_or(t);
        let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
        let parts: Vec<&str> = head.split('.').collect();
        (parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok()))
            .then(|| t.to_string())
    })
    .max_by(|a, b| cmp_versions(a, b))
}

/// 给定的版本字符串是否形如 semver（例如 `v0.15.0`、`1.2.3-rc.1`），而非
/// git 短 hash（例如 `v646c91c`）。
///
/// 由 `is_newer_than` 用于识别以下罕见的回退路径：未锁定的 git 来源仓库
/// 没有任何可用的 semver tag —— 此时 `installed_version` 是克隆下来的 HEAD
/// 短 hash，而 `cmp_versions` 会单纯因为数字段数量把任何 semver tag 排在
/// 前面。先按形态过滤一次，让 `is_newer_than` 选用合适的比较方式，而不是
/// 盲目信任那种顺序。
pub fn looks_like_semver(version: &str) -> bool {
    let stripped = version.strip_prefix('v').unwrap_or(version);
    let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
    let parts: Vec<&str> = head.split('.').collect();
    parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok())
}

/// 给定来源的包，候选版本 `latest` 是否比当前已安装的 `installed` 更新。
///
/// - npm / 锁定的 git：按 `cmp_versions` 与 semver 基线排序。
/// - 未锁定 git、已安装版本呈 tag 形态（`fetch_git` 解析到最高 semver tag
///   之后的常见情况）：同样按 semver 排序。
/// - 未锁定 git、已安装版本呈 hash 形态（仓库无任何 semver tag 时的回退
///   路径）：`cmp_versions` 会单纯因为数字段数量把远端的 tag 形 `latest`
///   排在前面，因此改为字符串相等判断 —— 但仅在 `latest` 也是 hash 时生效。
///   当 hash 形 `installed` 面对 tag 形 `latest` 时，说明远端没有可比较的
///   commit 图信号，应当报告无更新，直到用户手动重新安装。
pub fn is_newer_than(latest: &str, installed: &str, origin: &str, pinned: bool) -> bool {
    if origin == "git" && !pinned && !looks_like_semver(installed) {
        if looks_like_semver(latest) {
            false
        } else {
            latest != installed
        }
    } else {
        cmp_versions(latest, installed) == std::cmp::Ordering::Greater
    }
}

/// `git ls-remote --tags` 拿到的最高 semver tag；仓库没有 tag 或命令失败时
/// 返回 `None`（调用方回退到 clone 后的 HEAD hash）。
pub fn git_latest_tag(source: &str) -> Result<Option<String>, String> {
    let (ok, out) = crate::process::run_capture("git", &["ls-remote", "--tags", source])
        .map_err(|e| e.to_string())?;
    if !ok {
        return Ok(None);
    }
    let tags: Vec<String> = out
        .lines()
        .filter_map(|line| {
            let (_, ref_part) = line.split_once('\t')?;
            let tag = ref_part.strip_prefix("refs/tags/")?.trim_end_matches("^{}");
            Some(tag.to_string())
        })
        .collect();
    Ok(latest_tag(tags.iter().map(|s| s.as_str())))
}

// --- spec 拆分 --------------------------------------------------------------

/// 把 npm spec 拆分为 (name, 可选 pin)。scope 前缀之后的最后一个 `@`
/// 用来分隔版本；`@scope/name@1.2.3` 解析为 `(@scope/name, 1.2.3)`。
/// 纯名字则原样返回。
pub fn split_npm_spec(spec: &str) -> Result<(String, Option<String>), String> {
    let s = spec.trim();
    if s.starts_with('@') {
        let (head, rest) = s
            .split_once('/')
            .ok_or_else(|| format!("非法的 npm 包名 {spec:?}"))?;
        let rest = rest.trim();
        let (name, pin) = match rest.rsplit_once('@') {
            Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
                (n, Some(p.to_string()))
            }
            _ => (rest, None),
        };
        return Ok((format!("{head}/{name}"), pin));
    }
    match s.rsplit_once('@') {
        Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
            Ok((n.to_string(), Some(p.to_string())))
        }
        _ => Ok((s.to_string(), None)),
    }
}

// --- 中央库暂存目录 ---------------------------------------------------------

/// 在 `store` 下构造一个唯一的空暂存目录。`kind` 取调用方的 `TMP_PREFIX` /
/// `NEW_PREFIX` / `BACKUP_PREFIX` 之一；`pid` 与 `nanos` 折入目录名，保证
/// 两个并发的取源（或与崩溃交错的两次更新）不会冲突。
///
/// 何时写入 [`ID_MARKER`] 由调用方决定：只有在 rename 完成后才能盖章。
/// 在 rename 目标上预先盖章是 Windows 上的故障模式——上一次尝试遗留的
/// `.new-<pid>-<ts>` 目录里既有标记文件也有中间内容，而 Windows 的
/// `fs::rename` 会以 ERROR_DIR_NOT_EMPTY 拒绝非空目标。让新路径在 rename
/// 完成前保持空、只在源端盖章，封住了这个口子。
///
/// `fs::remove_dir_all` 不再是「点了就忘」：清理旧目标的失败会暴露出来，
/// 让调用方决定是重试、上报还是回退到别的路径。目录本来就不存在（含
/// 「删除失败但目录已消失」的竞态）视为成功。
pub fn new_staging_dir(store: &Path, kind: &str) -> io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = store.join(format!("{kind}{}-{nanos}", std::process::id()));
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) if !dir.exists() => {}
        Err(e) => return Err(e),
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 在已有暂存目录里写入 [`ID_MARKER`]，方便对账流程把它与对应的正式目录
/// 归到一组。只能在 rename 成功后调用，绝不能在之前调用，这样 rename 目标
/// 在 Windows 上始终保持空目录。
pub fn stamp_id_marker(dir: &Path, id: &str) -> io::Result<()> {
    atomic_write(&dir.join(ID_MARKER), format!("{id}\n").as_bytes())
}

/// 把 git URL 归一成 `owner/repo` 形状，供调用方映射中央库 id。
///
/// 协议双斜杠、`git@host:` 里的冒号、`.git` 后缀都要先剥掉：同一个仓库写成
/// `git@github.com:owner/repo.git` 与 `https://github.com/owner/repo` 必须算出
/// 同一个 id，否则用户会看到「同一个包装了两遍」，而两份源码目录只有一份接线。
pub fn repo_id_base(url: &str) -> String {
    url.trim_start_matches("git@")
        .split("://")
        .last()
        .unwrap_or(url)
        .trim_end_matches(".git")
        .replace(':', "/")
}

/// 崩溃残留暂存目录的分类。两个中央库（插件 / 技能）用同一套三段换名术语，
/// 区别只在目录名前缀（`tmp-` 与 `.tmp-`），所以分类结果共享、前缀各自匹配。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StagingKind {
    /// 取源进行中：内容未必完整，一律丢弃。
    Tmp,
    /// 已通过校验、等待发布。
    New,
    /// 发布过程中被挪到一旁的旧活动目录。
    Backup,
}

/// 把一个 id 的残留暂存目录恢复到 `final_dir`，返回它现在是否有可用内容。
///
/// 恢复表只有一张，两个中央库共用，因为**两个状态都幸存时回滚优先**：旧版本
/// 是我们知道用户已经在跑的那一份，`.new-*` 内容只是「已校验但还没跑起来」。
/// 这张表决定的是用户插件 / 技能目录的最终内容——在两处各写一遍时，任何一次
/// 改动都可能只改一半。
///
/// 规则：
/// - `final_dir` 已存在：现有内容优先，暂存目录全部清掉；
/// - 否则以最新的 `.backup-*` 回滚，其次提升最新的 `.new-*`；
///   `tmp` 与较老的同辈一律清掉。
///
/// 目录名里编码了 pid + 时间戳，因此字典序即时间序，最新的排在最后。
pub fn recover_staging_dir(final_dir: &Path, mut items: Vec<(StagingKind, PathBuf)>) -> bool {
    items.sort_by(|a, b| a.1.file_name().cmp(&b.1.file_name()));
    if final_dir.exists() {
        for (_, path) in items {
            let _ = fs::remove_dir_all(&path);
        }
        return true;
    }
    let newest = |kind: StagingKind| {
        items
            .iter()
            .rev()
            .find(|(k, _)| *k == kind)
            .map(|(_, p)| p.clone())
    };
    let newest_new = newest(StagingKind::New);
    let newest_backup = newest(StagingKind::Backup);
    for (kind, path) in &items {
        let drop = match kind {
            StagingKind::Tmp => true,
            StagingKind::New => Some(path) != newest_new.as_ref(),
            StagingKind::Backup => Some(path) != newest_backup.as_ref(),
        };
        if drop {
            let _ = fs::remove_dir_all(path);
        }
    }
    if let Some(backup) = newest_backup {
        let _ = fs::rename(&backup, final_dir);
        if let Some(new) = newest_new {
            let _ = fs::remove_dir_all(&new);
        }
        true
    } else if let Some(new) = newest_new {
        let _ = fs::rename(&new, final_dir);
        true
    } else {
        // 只剩 `.tmp-*`，上面已经清理掉了，且没有可用的 `.dsh-id` 记录。
        false
    }
}

// --- 取源标记 ---------------------------------------------------------------

/// 写入 `.dsh-source.json`：记录这个中央库目录是谁、从哪来、什么版本。
pub fn write_source_marker(
    dest: &Path,
    id: &str,
    origin: &str,
    source: &str,
    version: &str,
) -> Result<(), AppError> {
    let marker = serde_json::json!({
        "id": id,
        "origin": origin,
        "source": source,
        "version": version,
        "fetchedAt": crate::process::epoch_secs_string(),
    });
    let text = serde_json::to_string_pretty(&marker).map_err(|e| AppError::Io(e.to_string()))?;
    atomic_write(&dest.join(SOURCE_MARKER), format!("{text}\n").as_bytes())
        .map_err(|e| AppError::Io(e.to_string()))
}

// --- 链接 -------------------------------------------------------------------

/// 删除一个文件系统 link（symlink），不动它的目标。Windows 上 `DeleteFile`
/// 会以 ERROR_ACCESS_DENIED 拒绝目录 symlink —— 只有 `RemoveDirectory`
/// 才能删除它们；文件 symlink 又需要 `DeleteFile`；两种都试一遍就覆盖了
/// 所有平台和所有形态。选错方式会让链接留在原地，之后所有操作（重建、
/// 复制）都会顺着链接追到目标里去 —— Windows 上一次插件更新正是这样演变成
/// 「把中央库目录拷给自己」并以 os error 2 失败。
pub fn remove_link(path: &Path) {
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}
