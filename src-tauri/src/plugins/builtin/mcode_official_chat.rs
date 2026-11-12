//! 官网对话引用的 **MiniMax 本地插件**安装/撤销。
//!
//! ## 装的是什么
//!
//! 一份静态插件包（`plugins/official-chat/minimax/`，随应用内嵌）：一个 MCP
//! stdio server ＋ 一条 skill。MiniMax 工作台里的模型按 skill 指引调用工具，
//! 工具经壳的回环桥（`DSH_XLINK_BUILTIN_BRIDGE_URL/TOKEN`）读官网会话目录与
//! 正文（取数仍由壳的官网网页版窗口完成，凭据不进插件）。
//!
//! ## 为什么不做 Ponytail 那种升级事务
//!
//! 没有上游内容、没有按版本物化：只有「一份静态包」与「装/不装」两态。所以
//! 这里只需要三件事——**归属标记**（绝不认领用户自己的插件目录）、**内容摘要**
//! （只删/换自己写的那棵树）、**暂存后整目录替换**（避免半份插件被 MiniMax
//! 扫描到）。崩溃恢复只处理本模块命名过的暂存/备份/回收目录。
//!
//! ## MiniMax 默认启用本地插件
//!
//! 它把「目录里没有 disabled 记录」当启用。因此启用时先把意图写成`false`
//! 再落文件、成功后才写`true`：中途崩溃不会留下「插件在跑、壳里却查不到
//! 授权」的半状态（与 Ponytail 的同一条纪律）。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 插件行 id（与 `builtins.rs` 的 `OFFICIAL_CHAT` 一致）。
pub(crate) const TARGET_ID: &str = "official-chat";

/// MiniMax 支持本地插件 MCP 的最低版本（0.1.7 起；更高版本沿用同一结构）。
pub(crate) const MIN_VERSION: &str = "0.1.7";

/// 官方 MiniMax WebUI 还不支持 DSH 工作台那种 `@` 引用方式；2026-11-12 起
/// MiniMax 侧官网引用整体停用，等上游支持后再移植。恢复时改成 `true`，并同步
/// UI 的 `TARGETS`、插件说明与文档。
pub(crate) const MINIMAX_ENABLED: bool = false;

/// 实例 `plugins/` 下的目录名。
const TARGET_DIR: &str = "xlink-official-chat";

/// 归属标记（树内；内容摘要不含它自己）。
const OWNER_MARKER: &str = crate::plugins::owned_tree::MARKER;

/// 用户意图文件名（落在实例的 `runtime/` 下）。
const PREFERENCE_FILE: &str = "xlink-official-chat.json";

const STAGE_PREFIX: &str = ".xlink-official-chat-stage-";
const BACKUP_PREFIX: &str = ".xlink-official-chat-backup-";
const TRASH_PREFIX: &str = ".xlink-official-chat-trash-";

#[derive(Serialize, Deserialize)]
struct Preference {
    version: u32,
    enabled: bool,
}

#[derive(Serialize, Deserialize)]
struct Owner {
    version: u32,
    nonce: String,
    digest: String,
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn new_nonce() -> String {
    format!("{:032x}", rand::random::<u128>())
}

pub(crate) fn preference_path(instance_dir: &Path) -> PathBuf {
    instance_dir.join("runtime").join(PREFERENCE_FILE)
}

/// 读用户意图；`None` = 从没在本壳里拨过这个开关。
pub(crate) fn read_preference(instance_dir: &Path) -> Result<Option<bool>, String> {
    let path = preference_path(instance_dir);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取 MiniMax 官网引用设置：{error}")),
    };
    let value: Preference = serde_json::from_slice(&bytes)
        .map_err(|error| format!("MiniMax 官网引用设置损坏：{error}；请重新拨一次开关"))?;
    if value.version != 1 {
        return Err("MiniMax 官网引用设置版本不受支持".into());
    }
    Ok(Some(value.enabled))
}

pub(crate) fn save_preference(instance_dir: &Path, enabled: bool) -> Result<(), String> {
    let path = preference_path(instance_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 MiniMax 插件设置目录：{error}"))?;
    }
    let bytes = serde_json::to_vec(&Preference {
        version: 1,
        enabled,
    })
    .map_err(|error| error.to_string())?;
    crate::shell::atomic_write::atomic_write_private(&path, &bytes)
        .map_err(|error| format!("无法保存 MiniMax 官网引用设置：{error}"))
}

pub(crate) fn clear_preference(instance_dir: &Path) -> Result<(), String> {
    let path = preference_path(instance_dir);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法清除 MiniMax 官网引用设置：{error}")),
    }
}

fn plugins_dir(home: &Path) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(home).map_err(|error| format!("MiniMax 实例目录不可用：{error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("MiniMax 实例目录不是普通目录".into());
    }
    let plugins = home.join("plugins");
    match fs::create_dir(&plugins) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("无法准备 MiniMax 插件目录：{error}")),
    }
    let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("MiniMax 插件目录不是普通目录".into());
    }
    Ok(plugins)
}

fn target_dir(home: &Path) -> PathBuf {
    home.join("plugins").join(TARGET_DIR)
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("无法检查 {}：{error}", path.display())),
    }
}

fn read_owner(root: &Path) -> Result<Owner, String> {
    let path = root.join(OWNER_MARKER);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("{} 缺少归属标记：{error}", root.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err(format!("{} 的归属标记不是普通文件", root.display()));
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let owner: Owner = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{} 的归属标记损坏：{error}", root.display()))?;
    if owner.version != 1 || !is_hex(&owner.nonce, 32) || !is_hex(&owner.digest, 64) {
        return Err(format!("{} 的归属标记内容无效", root.display()));
    }
    Ok(owner)
}

/// 树的内容摘要（不含归属标记本身）。
fn tree_digest(root: &Path) -> Result<String, String> {
    let entries = crate::plugins::owned_tree::scan(root)?;
    let kept: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.path != OWNER_MARKER)
        .collect();
    Ok(crate::plugins::owned_tree::digest_entries(&kept))
}

/// 源目录（内嵌资源）的摘要与文件清单；链接/特殊文件一律拒绝。
fn source_tree(source: &Path) -> Result<(String, Vec<crate::plugins::owned_tree::Entry>), String> {
    let entries = crate::plugins::owned_tree::scan(source)
        .map_err(|error| format!("官网引用插件资源不可用：{error}；请重新构建或安装应用"))?;
    if entries.is_empty() {
        return Err("官网引用插件资源为空；请重新构建或安装应用".into());
    }
    Ok((
        crate::plugins::owned_tree::digest_entries(&entries),
        entries,
    ))
}

fn copy_tree(
    source: &Path,
    entries: &[crate::plugins::owned_tree::Entry],
    stage: &Path,
) -> Result<(), String> {
    fs::create_dir_all(stage).map_err(|error| format!("无法创建官网引用插件暂存目录：{error}"))?;
    for entry in entries {
        let to = stage.join(&entry.path);
        if entry.kind == b'd' {
            fs::create_dir_all(&to).map_err(|error| error.to_string())?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(source.join(&entry.path), &to)
                .map_err(|error| format!("复制官网引用插件文件失败：{error}"))?;
        }
    }
    Ok(())
}

fn owned_dir(root: &Path) -> Result<bool, String> {
    if !exists(root)? {
        return Ok(false);
    }
    let owner = read_owner(root)?;
    let digest = tree_digest(root)?;
    if digest != owner.digest {
        return Err(format!(
            "{} 的官网引用插件内容已修改，已保留；请检查后手动处理",
            root.display()
        ));
    }
    Ok(true)
}

/// 目标插件是否由本壳安装且内容未变。
pub(crate) fn installed(home: &Path) -> Result<bool, String> {
    owned_dir(&target_dir(home))
}

/// 崩溃恢复：只处理本模块命名过的暂存 / 备份 / 回收目录。
///
/// · 暂存目录：目标不在就顶上、在就删（内容都要先验归属）；
/// · 备份目录：目标在就删、不在就还原；
/// · 回收目录：直接删。
///
/// 无归属标记的暂存保持原样并跳过；其他校验失败保留原样并报错。
fn recover(home: &Path) -> Result<(), String> {
    let entries = match fs::read_dir(home) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("无法读取 MiniMax 实例目录：{error}")),
    };
    let target = target_dir(home);
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        if let Some(nonce) = name.strip_prefix(TRASH_PREFIX) {
            if !is_hex(nonce, 32) {
                continue;
            }
            // 回收目录也是我们写的那棵树：先验归属再删，别把用户目录当垃圾。
            let owner = read_owner(&path)?;
            if tree_digest(&path)? != owner.digest {
                return Err("官网引用插件回收目录内容被修改，已保留供检查".into());
            }
            fs::remove_dir_all(&path)
                .map_err(|error| format!("无法清理官网引用插件回收目录：{error}"))?;
        } else if let Some(nonce) = name.strip_prefix(STAGE_PREFIX) {
            if !is_hex(nonce, 32) {
                continue;
            }
            // shortcut: 未标记的中断暂存留在磁盘，明确归属后再支持自动清理。
            if !exists(&path.join(OWNER_MARKER))? {
                continue;
            }
            let owner = read_owner(&path)?;
            if tree_digest(&path)? != owner.digest {
                return Err("官网引用插件暂存目录内容被修改，已保留供检查".into());
            }
            if exists(&target)? {
                fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
            } else {
                crate::shell::atomic_write::rename_exclusive(&path, &target)
                    .map_err(|error| format!("无法恢复官网引用插件暂存目录：{error}"))?;
            }
        } else if let Some(nonce) = name.strip_prefix(BACKUP_PREFIX) {
            if !is_hex(nonce, 32) {
                continue;
            }
            let owner = read_owner(&path)?;
            if tree_digest(&path)? != owner.digest {
                return Err("官网引用插件备份目录内容被修改，已保留供检查".into());
            }
            if exists(&target)? {
                fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
            } else {
                crate::shell::atomic_write::rename_exclusive(&path, &target)
                    .map_err(|error| format!("无法还原官网引用插件备份：{error}"))?;
            }
        }
    }
    Ok(())
}

/// 装（或更新到）这份资源。幂等：内容已一致时什么都不做。
///
/// 目标目录存在但**没有**本壳标记时返回错误并原样保留——用户自己的同名插件
/// 不该被覆盖。
pub(crate) fn install(source: &Path, home: &Path) -> Result<(), String> {
    let (_wanted_digest, entries) = source_tree(source)?;
    plugins_dir(home)?;
    recover(home)?;
    let target = target_dir(home);
    if exists(&target)? {
        let owner = read_owner(&target)?;
        if tree_digest(&target)? != owner.digest {
            return Err(
                "MiniMax 里的官网引用插件内容被修改，已保留原样；请检查或移除后重试".into(),
            );
        }
        if owner.digest == _wanted_digest {
            return Ok(());
        }
    }
    let nonce = new_nonce();
    let stage = home.join(format!("{STAGE_PREFIX}{nonce}"));
    let result = (|| -> Result<(), String> {
        copy_tree(source, &entries, &stage)?;
        let digest = tree_digest(&stage)?;
        let owner = Owner {
            version: 1,
            nonce: nonce.clone(),
            digest: digest.clone(),
        };
        let bytes = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
        fs::write(stage.join(OWNER_MARKER), &bytes).map_err(|error| error.to_string())?;
        if tree_digest(&stage)? != digest {
            return Err("官网引用插件暂存内容与摘要不一致".into());
        }
        let backup = home.join(format!("{BACKUP_PREFIX}{nonce}"));
        if exists(&target)? {
            crate::shell::atomic_write::rename_exclusive(&target, &backup)
                .map_err(|error| format!("无法备份旧官网引用插件：{error}"))?;
        }
        if let Err(error) = crate::shell::atomic_write::rename_exclusive(&stage, &target) {
            if exists(&backup)? {
                let _ = crate::shell::atomic_write::rename_exclusive(&backup, &target);
            }
            return Err(format!("无法发布官网引用插件：{error}"));
        }
        if exists(&backup)? {
            fs::remove_dir_all(&backup)
                .map_err(|error| format!("无法删除官网引用插件旧版本：{error}"))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

/// 撤销：只删「归属标记与摘要都对得上」的那棵树。没装过时是空操作。
pub(crate) fn remove(home: &Path) -> Result<(), String> {
    let target = target_dir(home);
    recover(home)?;
    if !exists(&target)? {
        return Ok(());
    }
    let owner = read_owner(&target)?;
    if tree_digest(&target)? != owner.digest {
        return Err("MiniMax 里的官网引用插件内容被修改，已保留原样；请检查后手动移除".into());
    }
    let trash = home.join(format!("{TRASH_PREFIX}{}", new_nonce()));
    crate::shell::atomic_write::rename_exclusive(&target, &trash)
        .map_err(|error| format!("无法回收官网引用插件：{error}"))?;
    if let Err(error) = fs::remove_dir_all(&trash) {
        crate::shell::shell_events::record(
            "mcode-plugin-target",
            &format!(
                "官网引用插件已停用，但回收目录清理失败（{}）：{error}",
                trash.display()
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        source: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "xlink-mcode-official-chat-{}-{tag}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            let home = root.join("home");
            let source = root.join("source");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(source.join("mcp")).unwrap();
            fs::write(source.join("mcp/server.mjs"), "// server\n").unwrap();
            fs::write(source.join("icon.png"), b"png").unwrap();
            Self { root, home, source }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn installs_then_is_idempotent_and_removable() {
        let f = Fixture::new("roundtrip");
        assert!(!installed(&f.home).unwrap());
        install(&f.source, &f.home).unwrap();
        assert!(installed(&f.home).unwrap());
        assert!(target_dir(&f.home).join("mcp/server.mjs").is_file());
        install(&f.source, &f.home).unwrap();
        remove(&f.home).unwrap();
        assert!(!installed(&f.home).unwrap());
        assert!(!exists(&target_dir(&f.home)).unwrap());
        // 再撤一次是空操作，不是错误。
        remove(&f.home).unwrap();
    }

    #[test]
    fn updates_when_the_shipped_content_changes() {
        let f = Fixture::new("update");
        install(&f.source, &f.home).unwrap();
        let before = tree_digest(&target_dir(&f.home)).unwrap();
        fs::write(f.source.join("mcp/server.mjs"), "// server v2\n").unwrap();
        install(&f.source, &f.home).unwrap();
        let after = tree_digest(&target_dir(&f.home)).unwrap();
        assert_ne!(before, after);
        assert_eq!(
            fs::read_to_string(target_dir(&f.home).join("mcp/server.mjs")).unwrap(),
            "// server v2\n"
        );
        assert!(!fs::read_dir(&f.home).unwrap().flatten().any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(BACKUP_PREFIX)));
    }

    #[test]
    fn never_claims_a_foreign_plugin_directory() {
        let f = Fixture::new("foreign");
        let target = target_dir(&f.home);
        fs::create_dir_all(target.join("mcp")).unwrap();
        fs::write(target.join("mcp/server.mjs"), "// user's own\n").unwrap();
        assert!(install(&f.source, &f.home).is_err());
        assert!(remove(&f.home).is_err());
        assert!(
            target.join("mcp/server.mjs").is_file(),
            "用户自己的插件必须原样保留"
        );
    }

    #[test]
    fn refuses_to_delete_a_modified_owned_tree() {
        let f = Fixture::new("modified");
        install(&f.source, &f.home).unwrap();
        let target = target_dir(&f.home);
        fs::write(target.join("mcp/server.mjs"), "// tampered\n").unwrap();
        assert!(installed(&f.home).unwrap_err().contains("已修改"));
        assert!(remove(&f.home).is_err());
        assert!(target.exists(), "内容被改过就不删");
    }

    #[test]
    fn incomplete_unowned_stage_does_not_block_install_or_remove() {
        let f = Fixture::new("interrupted-stage");
        let stage = f.home.join(format!("{STAGE_PREFIX}{}", new_nonce()));
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("partial"), "keep").unwrap();
        install(&f.source, &f.home).unwrap();
        assert!(installed(&f.home).unwrap());
        remove(&f.home).unwrap();
        assert_eq!(fs::read_to_string(stage.join("partial")).unwrap(), "keep");
    }

    #[test]
    fn recovery_promotes_a_staged_tree_and_clears_backups() {
        let f = Fixture::new("recover");
        install(&f.source, &f.home).unwrap();
        // 模拟：旧树已备份、新树停在暂存，目标目录不在。
        let target = target_dir(&f.home);
        let stage = f.home.join(format!("{STAGE_PREFIX}{}", new_nonce()));
        fs::rename(
            &target,
            f.home.join(format!("{BACKUP_PREFIX}{}", new_nonce())),
        )
        .unwrap();
        fs::create_dir_all(&stage).unwrap();
        copy_tree(&f.source, &source_tree(&f.source).unwrap().1, &stage).unwrap();
        let digest = tree_digest(&stage).unwrap();
        let bytes = serde_json::to_vec(&Owner {
            version: 1,
            nonce: new_nonce(),
            digest,
        })
        .unwrap();
        fs::write(stage.join(OWNER_MARKER), bytes).unwrap();
        recover(&f.home).unwrap();
        assert!(installed(&f.home).unwrap());
        assert!(!fs::read_dir(&f.home).unwrap().flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.starts_with(BACKUP_PREFIX) || name.starts_with(STAGE_PREFIX)
        }));
    }

    #[test]
    fn preference_roundtrip_and_clear() {
        let f = Fixture::new("pref");
        let instance_dir = f.root.join("instance");
        assert_eq!(read_preference(&instance_dir).unwrap(), None);
        save_preference(&instance_dir, true).unwrap();
        assert_eq!(read_preference(&instance_dir).unwrap(), Some(true));
        clear_preference(&instance_dir).unwrap();
        assert_eq!(read_preference(&instance_dir).unwrap(), None);
        fs::write(preference_path(&instance_dir), b"{bad").unwrap();
        assert!(read_preference(&instance_dir).is_err());
    }
}
