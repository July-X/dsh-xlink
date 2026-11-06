//! Assemble a MiniMax-native Ponytail package into a caller-owned empty staging directory.

use std::{fs, path::Path};

use crate::pkg::fetch;

#[cfg(windows)]
#[derive(serde::Serialize, serde::Deserialize)]
struct Ownership {
    nonce: String,
    digest: String,
}

#[cfg(windows)]
#[derive(serde::Serialize, serde::Deserialize)]
struct UpgradeJournal {
    version: u32,
    transaction: String,
    nonce: String,
    old_digest: String,
    new_digest: String,
}

#[cfg(windows)]
fn upgrade_journal(home: &Path) -> Result<Option<(UpgradeJournal, Vec<u8>)>, String> {
    let path = home.join(".xlink-ponytail-upgrade.json");
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法检查 Ponytail 升级记录：{error}")),
        Ok(metadata) => metadata,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err("Ponytail 升级记录无效，已保留原内容".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let journal: UpgradeJournal = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Ponytail 升级记录损坏：{error}"))?;
    let hex = |text: &str, length: usize| {
        text.len() == length && text.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    if journal.version != 1
        || !hex(&journal.transaction, 32)
        || !hex(&journal.nonce, 32)
        || !hex(&journal.old_digest, 64)
        || !hex(&journal.new_digest, 64)
        || journal.old_digest == journal.new_digest
    {
        return Err("Ponytail 升级记录内容无效，已保留原内容".into());
    }
    Ok(Some((journal, bytes)))
}

/// Complete an interrupted, shell-owned upgrade while the instance is offline.
#[cfg(windows)]
pub(crate) fn recover_upgrade(lease: &crate::skills::mcode::InstanceLease) -> Result<(), String> {
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    let Some((journal, journal_bytes)) = upgrade_journal(home)? else {
        return Ok(());
    };
    let plugins = home.join("plugins");
    let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Ponytail 升级恢复失败：插件目录异常".into());
    }
    let target = plugins.join("xlink-ponytail");
    let stage = home.join(format!(
        ".xlink-ponytail-upgrade-stage-{}",
        journal.transaction
    ));
    let backup = home.join(format!(".xlink-ponytail-backup-{}", journal.transaction));
    let old = Ownership {
        nonce: journal.nonce.clone(),
        digest: journal.old_digest.clone(),
    };
    let new = Ownership {
        nonce: journal.nonce.clone(),
        digest: journal.new_digest.clone(),
    };
    let current = ownership(home)?.ok_or("Ponytail 归属记录缺失，无法恢复升级")?;
    if current.nonce != journal.nonce
        || (current.digest != old.digest && current.digest != new.digest)
    {
        return Err("Ponytail 升级期间归属记录已改变，已保留原内容".into());
    }
    let mut target_exists = present(&target)?;
    let mut stage_exists = present(&stage)?;
    let mut backup_exists = present(&backup)?;
    check_other_plugins(&plugins, target_exists)?;
    if stage_exists {
        verified_owned(&stage, &new)?;
    }
    if backup_exists {
        verified_owned(&backup, &old)?;
    }
    if target_exists && !backup_exists {
        verified_owned(&target, &old)?;
        if !stage_exists || current.digest != old.digest {
            return Err("Ponytail 升级暂存缺失，已保留原内容".into());
        }
        rename_exclusive(&target, &backup)
            .map_err(|error| format!("无法备份 Ponytail：{error}"))?;
        target_exists = false;
        backup_exists = true;
    }
    if !target_exists && backup_exists && stage_exists {
        verified_owned(&backup, &old)?;
        rename_exclusive(&stage, &target)
            .map_err(|error| format!("无法发布 Ponytail 升级：{error}"))?;
        target_exists = true;
        stage_exists = false;
    }
    if !target_exists || !backup_exists || stage_exists {
        return Err("Ponytail 升级状态不完整，已保留所有内容供检查".into());
    }
    verified_owned(&target, &new)?;
    verified_owned(&backup, &old)?;
    if current.digest == old.digest {
        let latest = ownership(home)?.ok_or("Ponytail 归属记录缺失，无法恢复升级")?;
        if latest.nonce != current.nonce || latest.digest != current.digest {
            return Err("Ponytail 升级期间归属记录已改变，已保留原内容".into());
        }
        let bytes = serde_json::to_vec(&new).map_err(|error| error.to_string())?;
        crate::shell::process::atomic_write(&home.join(".xlink-ponytail-owner.json"), &bytes)
            .map_err(|error| format!("无法保存 Ponytail 升级归属：{error}"))?;
    }
    let path = home.join(".xlink-ponytail-upgrade.json");
    if upgrade_journal(home)?.map(|(_, bytes)| bytes) != Some(journal_bytes) {
        return Err("Ponytail 升级记录已改变，已保留所有内容".into());
    }
    fs::remove_file(path).map_err(|error| format!("无法完成 Ponytail 升级记录：{error}"))?;
    Ok(())
}

#[cfg(windows)]
fn ownership(home: &Path) -> Result<Option<Ownership>, String> {
    let path = home.join(".xlink-ponytail-owner.json");
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取 Ponytail 归属记录：{error}")),
        Ok(metadata) => metadata,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err("Ponytail 归属记录异常，已保留现有插件".into());
    }
    let value: Ownership =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Ponytail 归属记录损坏：{error}"))?;
    if value.nonce.len() != 32
        || value.digest.len() != 64
        || !value.nonce.bytes().all(|b| b.is_ascii_hexdigit())
        || !value.digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Ponytail 归属记录无效，已保留现有插件".into());
    }
    Ok(Some(value))
}

#[cfg(windows)]
fn present(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("无法检查 Ponytail 目录：{error}")),
        Ok(_) => Ok(true),
    }
}

#[cfg(windows)]
fn verified_owned(path: &Path, owner: &Ownership) -> Result<(), String> {
    ordinary_tree(path)?;
    let marker = path.join(".xlink-owner");
    if fs::read(&marker).map_err(|error| error.to_string())?
        != format!("{}\n", owner.nonce).as_bytes()
        || crate::skills::manage::fingerprint_path(path).as_deref() != Some(owner.digest.as_str())
    {
        return Err(format!(
            "Ponytail 内容已更改或归属无法确认，已保留：{}",
            path.display()
        ));
    }
    Ok(())
}

/// Read the version from the verified active package without changing it.
#[cfg(windows)]
pub(crate) fn active_version(instance_id: &str) -> Result<String, String> {
    if crate::kernel::mcode_service::configured_instance_id()?.as_deref() != Some(instance_id) {
        return Err("MiniMax 实例已改变，请刷新页面后重试".into());
    }
    let home = crate::shell::paths::instance_dsh_home("mcode", instance_id);
    if upgrade_journal(&home)?.is_some() {
        return Err("Ponytail 升级尚未恢复，请先停止工作台并重试".into());
    }
    let plugins = home.join("plugins");
    let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("MiniMax 插件目录不是普通目录".into());
    }
    let target = plugins.join("xlink-ponytail");
    let owner = ownership(&home)?.ok_or("Ponytail 归属记录不存在")?;
    verified_owned(&target, &owner)?;
    let marker = target.join(".xlink-upstream-version");
    let metadata = fs::symlink_metadata(&marker)
        .map_err(|_| "Ponytail 当前版本不可识别，请保留现有插件".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 128 {
        return Err("Ponytail 当前版本记录无效".into());
    }
    let version = fs::read_to_string(marker).map_err(|error| error.to_string())?;
    verified_owned(&target, &owner)?;
    Ok(version.trim().to_string())
}

#[cfg(windows)]
fn check_other_plugins(plugins: &Path, skip_owned: bool) -> Result<(), String> {
    for entry in fs::read_dir(plugins).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if skip_owned
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("xlink-ponytail")
        {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        for manifest in [".minimax-plugin/plugin.json", ".claude-plugin/plugin.json"] {
            let path = entry.path().join(manifest);
            let parent = path.parent().ok_or("现有插件目录无效")?;
            match fs::symlink_metadata(parent) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("无法检查现有插件：{error}")),
                Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                    return Err(format!(
                        "现有插件清单目录异常，请检查：{}",
                        parent.display()
                    ));
                }
                Ok(_) => {}
            }
            let metadata = match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("无法检查现有插件：{error}")),
                Ok(metadata) => metadata,
            };
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 128 * 1024
            {
                return Err(format!("现有插件清单无效，请检查：{}", path.display()));
            }
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| format!("无法检查现有插件 {}：{error}", path.display()))?;
            if value["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("xlink-ponytail"))
            {
                return Err(format!(
                    "MiniMax 中已有同名 Ponytail 插件，原内容已保留：{}",
                    entry.path().display()
                ));
            }
        }
    }
    Ok(())
}

/// Disable only this shell-owned plugin; an unknown or modified directory blocks
/// startup so that revoking authorization cannot silently leave it active.
#[cfg(windows)]
pub(crate) fn disable(lease: &crate::skills::mcode::InstanceLease) -> Result<(), String> {
    recover_upgrade(lease)?;
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    let plugins = home.join("plugins");
    if !present(&plugins)? {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("MiniMax 插件目录异常，无法确认 Ponytail 已停用".into());
    }
    let target = plugins.join("xlink-ponytail");
    let parked = home.join(".xlink-ponytail-disabled");
    let active = present(&target)?;
    let disabled = present(&parked)?;
    if active && disabled {
        return Err("Ponytail 存在两份目录，已保留内容，请检查实例数据".into());
    }
    if active || disabled {
        let owner =
            ownership(home)?.ok_or("Ponytail 归属无法确认，不能保证停用；请检查现有插件")?;
        verified_owned(if active { &target } else { &parked }, &owner)?;
    }
    check_other_plugins(&plugins, active)?;
    if active {
        crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
        rename_exclusive(&target, &parked)
            .map_err(|error| format!("无法停用 Ponytail，原内容已保留：{error}"))?;
    }
    Ok(())
}

/// Reuse a verified package without consulting any npm source. An explicitly
/// authorized disabled package can be restored while the same lease is held.
#[cfg(windows)]
pub(crate) fn verify_active(lease: &crate::skills::mcode::InstanceLease) -> Result<bool, String> {
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    if upgrade_journal(home)?.is_some() {
        return Err("Ponytail 升级尚未恢复".into());
    }
    let plugins = home.join("plugins");
    if present(&plugins)? {
        let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("MiniMax 插件目录不是普通目录，请检查实例数据".into());
        }
    }
    let target = plugins.join("xlink-ponytail");
    let parked = home.join(".xlink-ponytail-disabled");
    let active = present(&target)?;
    let disabled = present(&parked)?;
    if active && disabled {
        return Err("Ponytail 存在两份目录，已保留内容，请检查实例数据".into());
    }
    if active || disabled {
        let saved = ownership(home)?.ok_or("已有 Ponytail 目录没有可信归属记录，已保留原内容")?;
        verified_owned(if active { &target } else { &parked }, &saved)?;
    }
    if present(&plugins)? {
        check_other_plugins(&plugins, active)?;
    }
    Ok(active)
}

#[cfg(windows)]
pub(crate) fn reuse(
    lease: &crate::skills::mcode::InstanceLease,
    authorized: bool,
) -> Result<bool, String> {
    if !authorized {
        return Err("尚未授权在 MiniMax 中启用 Ponytail".into());
    }
    recover_upgrade(lease)?;
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    let plugins = home.join("plugins");
    if present(&plugins)? {
        let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("MiniMax 插件目录不是普通目录，请检查实例数据".into());
        }
    }
    let target = plugins.join("xlink-ponytail");
    let parked = home.join(".xlink-ponytail-disabled");
    let active = present(&target)?;
    let disabled = present(&parked)?;
    if active && disabled {
        return Err("Ponytail 存在两份目录，已保留内容，请检查实例数据".into());
    }
    if !active && !disabled {
        return Ok(false);
    }
    let owner = ownership(home)?.ok_or("已有 Ponytail 目录没有可信归属记录，已保留原内容")?;
    verified_owned(if active { &target } else { &parked }, &owner)?;
    match fs::create_dir(&plugins) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("无法准备 MiniMax 插件目录：{error}")),
    }
    let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("MiniMax 插件目录不是普通目录，请检查实例数据".into());
    }
    check_other_plugins(&plugins, active)?;
    if disabled {
        crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
        rename_exclusive(&parked, &target)
            .map_err(|error| format!("无法恢复 Ponytail，原内容已保留：{error}"))?;
    }
    Ok(true)
}

fn ordinary_tree(path: &Path) -> Result<(), String> {
    fn visit(path: &Path, depth: usize, count: &mut usize, bytes: &mut u64) -> Result<(), String> {
        *count += 1;
        if depth > 32 || *count > 2048 {
            return Err("Ponytail 文件数量或目录层级超限".into());
        }
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(format!(
                "Ponytail 资源包含链接或特殊文件：{}",
                path.display()
            ));
        }
        if metadata.is_file() {
            *bytes = bytes
                .checked_add(metadata.len())
                .ok_or("Ponytail 内容大小无效")?;
            if metadata.len() > 16 * 1024 * 1024 || *bytes > 64 * 1024 * 1024 {
                return Err("Ponytail 资源大小超限".into());
            }
        } else {
            for item in fs::read_dir(path).map_err(|error| error.to_string())? {
                visit(
                    &item.map_err(|error| error.to_string())?.path(),
                    depth + 1,
                    count,
                    bytes,
                )?;
            }
        }
        Ok(())
    }
    visit(path, 0, &mut 0, &mut 0)
}

/// The caller must hold the instance's offline lease and own `stage`. This function
/// never publishes, overwrites or removes an installed plugin.
pub(crate) fn assemble(source: &Path, upstream: &Path, stage: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(stage).map_err(|error| error.to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::read_dir(stage)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
    {
        return Err("Ponytail 暂存目录必须是本次创建的空目录".into());
    }
    super::materialize::verify_manifest(source)?;
    for name in [".minimax-plugin", "minimax", "host"] {
        ordinary_tree(&source.join(name))?;
    }
    let upstream_meta = fs::symlink_metadata(upstream).map_err(|error| error.to_string())?;
    if !upstream_meta.is_dir() || upstream_meta.file_type().is_symlink() {
        return Err("Ponytail 上游目录不是普通目录，请重新同步插件".into());
    }
    ordinary_tree(&upstream.join("package.json"))?;
    let package: serde_json::Value = serde_json::from_slice(
        &fs::read(upstream.join("package.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Ponytail 上游清单无效：{error}"))?;
    if package["name"].as_str() != Some("@dietrichgebert/ponytail") {
        return Err("Ponytail 上游包身份不匹配，请重新同步插件".into());
    }
    let upstream_version = package["version"].as_str().ok_or("Ponytail 上游版本缺失")?;
    if !crate::shell::version::is_valid_kernel_version(upstream_version) {
        return Err("Ponytail 上游版本无效".into());
    }
    fs::write(
        stage.join(".xlink-upstream-version"),
        format!("{upstream_version}\n"),
    )
    .map_err(|error| format!("无法保存 Ponytail 上游版本：{error}"))?;
    for name in [
        "ponytail-activate.js",
        "ponytail-mode-tracker.js",
        "ponytail-subagent.js",
    ] {
        let file = upstream.join("hooks").join(name);
        ordinary_tree(&file)?;
        if !file.is_file() {
            return Err(format!("Ponytail 上游缺少钩子：{name}"));
        }
    }
    for (from, name) in [
        (upstream.join("hooks"), "hooks"),
        (upstream.join("skills"), "skills"),
    ] {
        ordinary_tree(&from)?;
        if !from.is_dir() {
            return Err(format!("Ponytail 资源目录不存在：{}", from.display()));
        }
        let before =
            crate::skills::manage::fingerprint_path(&from).ok_or("无法校验 Ponytail 上游内容")?;
        // `stage` was required to be empty, so copy_tree cannot replace existing user content.
        fetch::copy_tree(&from, &stage.join(name)).map_err(|error| error.to_string())?;
        ordinary_tree(&from)?;
        if crate::skills::manage::fingerprint_path(&from).as_deref() != Some(before.as_str())
            || crate::skills::manage::fingerprint_path(&stage.join(name)).as_deref()
                != Some(before.as_str())
        {
            return Err("Ponytail 上游内容在复制期间发生变化，请重试同步".into());
        }
    }
    let hook_package = stage.join("hooks/package.json");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&hook_package)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(b"{\n  \"type\": \"commonjs\"\n}\n")
                .map_err(|error| error.to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let doc: serde_json::Value = serde_json::from_slice(
                &fs::read(&hook_package).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("Ponytail 上游 hook 配置无效：{error}"))?;
            if doc["type"].as_str() != Some("commonjs") {
                return Err("Ponytail 上游 hook 模块类型不兼容，请检查插件版本".into());
            }
        }
        Err(error) => return Err(format!("无法准备 Ponytail hook 模块：{error}")),
    }
    for name in [".minimax-plugin", "minimax", "host"] {
        fs::create_dir(stage.join(name)).map_err(|error| error.to_string())?;
    }
    for name in [
        ".minimax-plugin/plugin.json",
        "minimax/hook.mjs",
        "minimax/hooks.json",
        "host/hooks.js",
        "icon.png",
        "package.json",
    ] {
        let file = source.join(name);
        ordinary_tree(&file)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.join(name))
            .map_err(|error| error.to_string())?;
        std::io::copy(
            &mut fs::File::open(file).map_err(|error| error.to_string())?,
            &mut output,
        )
        .map_err(|error| error.to_string())?;
    }
    super::materialize::verify_manifest(source)?;
    for name in [
        ".minimax-plugin/plugin.json",
        "minimax/hook.mjs",
        "minimax/hooks.json",
        "host/hooks.js",
        "icon.png",
        "package.json",
    ] {
        if crate::skills::manage::fingerprint_path(&source.join(name))
            != crate::skills::manage::fingerprint_path(&stage.join(name))
        {
            return Err("Ponytail 内置资源在复制期间发生变化，请重试".into());
        }
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(stage.join("skills")).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Ponytail 技能名称不是 UTF-8")?;
        if !name
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
            || name.is_empty()
        {
            return Err(format!("Ponytail 技能名称不符合 MiniMax 路径规则：{name}"));
        }
        if entry.path().join("SKILL.md").is_file() {
            paths.push(format!("skills/{name}/SKILL.md"));
        }
    }
    paths.sort();
    let path = stage.join(".minimax-plugin/plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    manifest["skills"] = serde_json::json!(paths);
    let mut encoded = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    fs::write(path, encoded).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn verify_source(source: &Path) -> Result<(), String> {
    // Production resources must have a digest manifest; dev callers can prepare it first.
    let manifest_path = source.join(super::materialize::MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Err("Ponytail 内置资源缺少摘要清单，请重新构建或安装应用".into());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Ponytail 摘要清单无效：{error}"))?;
    let files = manifest["files"]
        .as_array()
        .ok_or("Ponytail 摘要清单缺少文件列表")?;
    for name in [
        ".minimax-plugin/plugin.json",
        "minimax/hook.mjs",
        "minimax/hooks.json",
        "host/hooks.js",
        "icon.png",
        "package.json",
    ] {
        if !files.iter().any(|item| item["path"].as_str() == Some(name)) {
            return Err(format!(
                "Ponytail 资源缺少摘要：{name}；请重新构建或安装应用"
            ));
        }
    }
    super::materialize::verify_manifest(source)
}

#[cfg(windows)]
fn cleanup_stage<T>(stage: &Path, result: Result<T, String>) -> Result<T, String> {
    let cleanup = match fs::symlink_metadata(stage) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "无法检查 Ponytail 暂存目录 {}：{error}",
            stage.display()
        )),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => Err(format!(
            "Ponytail 暂存目录状态异常，已保留：{}",
            stage.display()
        )),
        Ok(_) => fs::remove_dir_all(stage)
            .map_err(|error| format!("无法清理 Ponytail 暂存目录 {}：{error}", stage.display())),
    };
    match cleanup {
        Ok(()) => result,
        Err(cleanup) => Err(match result {
            Ok(_) => cleanup,
            Err(original) => format!("{original}；{cleanup}"),
        }),
    }
}

/// Upgrade only an authorized, verified, offline plugin. The previous version is
/// retained in the instance and a durable journal makes interrupted moves recoverable.
#[cfg(windows)]
pub(crate) fn upgrade(
    lease: &crate::skills::mcode::InstanceLease,
    authorized: bool,
    source: &Path,
    upstream: &Path,
) -> Result<bool, String> {
    use std::io::Write;
    if !authorized {
        return Err("尚未授权在 MiniMax 中启用 Ponytail".into());
    }
    recover_upgrade(lease)?;
    if !verify_active(lease)? {
        return Err("Ponytail 未启用，请先安装并授权".into());
    }
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    let owner = ownership(home)?.ok_or("Ponytail 归属记录不存在")?;
    verify_source(source)?;
    let transaction = format!("{:032x}", rand::random::<u128>());
    let stage = home.join(format!(".xlink-ponytail-upgrade-stage-{transaction}"));
    fs::create_dir(&stage).map_err(|error| format!("无法创建 Ponytail 升级暂存：{error}"))?;
    let prepared = (|| {
        assemble(source, upstream, &stage)?;
        fs::write(stage.join(".xlink-owner"), format!("{}\n", owner.nonce))
            .map_err(|error| error.to_string())?;
        ordinary_tree(&stage)?;
        let digest =
            crate::skills::manage::fingerprint_path(&stage).ok_or("无法校验 Ponytail 升级内容")?;
        if digest == owner.digest {
            return Ok(None);
        }
        let target = home.join("plugins/xlink-ponytail");
        verified_owned(&target, &owner)?;
        let old_version = fs::read_to_string(target.join(".xlink-upstream-version"))
            .map_err(|_| "Ponytail 原版本不可识别，已保留现有插件")?;
        let new_version = fs::read_to_string(stage.join(".xlink-upstream-version"))
            .map_err(|error| error.to_string())?;
        let old_version = old_version.trim();
        let new_version = new_version.trim();
        if !super::mcode_ponytail_source::valid_version(old_version)
            || !super::mcode_ponytail_source::valid_version(new_version)
            || !super::mcode_ponytail_source::semver_cmp(new_version, old_version).is_gt()
        {
            return Err("Ponytail 候选版本未高于当前版本，已保留原插件".into());
        }
        crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
        Ok(Some(UpgradeJournal {
            version: 1,
            transaction,
            nonce: owner.nonce,
            old_digest: owner.digest,
            new_digest: digest,
        }))
    })();
    let journal = match prepared {
        Ok(None) => return cleanup_stage(&stage, Ok(false)),
        Ok(Some(journal)) => journal,
        Err(error) => return cleanup_stage(&stage, Err(error)),
    };
    let journal_path = home.join(".xlink-ponytail-upgrade.json");
    let temporary = home.join(format!(
        ".xlink-ponytail-upgrade-record-{}.tmp",
        journal.transaction
    ));
    let mut created = false;
    let publish = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("无法创建 Ponytail 升级记录：{error}"))?;
        created = true;
        file.write_all(&serde_json::to_vec(&journal).map_err(|error| error.to_string())?)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("无法保存 Ponytail 升级记录：{error}"))?;
        drop(file);
        rename_exclusive(&temporary, &journal_path)
            .map_err(|error| format!("无法发布 Ponytail 升级记录：{error}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = publish {
        // The temporary name is unique to this invocation; retain unexpected entries.
        let cleanup = if !created {
            Ok(())
        } else {
            match fs::symlink_metadata(&temporary) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    fs::remove_file(&temporary).map_err(|error| error.to_string())
                }
                Ok(_) => Err(format!("升级记录暂存异常，已保留：{}", temporary.display())),
                Err(error) => Err(error.to_string()),
            }
        };
        let error = match cleanup {
            Ok(()) => error,
            Err(cleanup) => format!("{error}；清理失败：{cleanup}"),
        };
        return cleanup_stage(&stage, Err(error));
    }
    // Once published, the journal owns the stage: recovery needs both after a failure.
    recover_upgrade(lease)?;
    Ok(true)
}

/// The caller supplies the persisted, explicit opt-in and holds the instance lease
/// throughout the operation. Existing plugin directories are never replaced.
#[cfg(windows)]
pub(crate) fn install(
    lease: &crate::skills::mcode::InstanceLease,
    authorized: bool,
    source: &Path,
    upstream: &Path,
) -> Result<(), String> {
    if !authorized {
        return Err("尚未授权在 MiniMax 中启用 Ponytail".into());
    }
    recover_upgrade(lease)?;
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    let plugins = home.join("plugins");
    match fs::create_dir(&plugins) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("无法准备 MiniMax 插件目录：{error}")),
    }
    let metadata = fs::symlink_metadata(&plugins).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("MiniMax 插件目录不是普通目录，请检查实例数据".into());
    }
    let target = plugins.join("xlink-ponytail");
    let parked = home.join(".xlink-ponytail-disabled");
    let owner = ownership(home)?;
    let active = present(&target)?;
    let disabled = present(&parked)?;
    if active && disabled {
        return Err("Ponytail 存在两份目录，已保留内容，请检查实例数据".into());
    }
    if active || disabled {
        let saved = owner
            .as_ref()
            .ok_or("已有 Ponytail 目录没有可信归属记录，已保留原内容")?;
        verified_owned(if active { &target } else { &parked }, saved)?;
    }
    check_other_plugins(&plugins, active)?;
    if active {
        return Ok(());
    }
    verify_source(source)?;
    let stage = home.join(format!(".xlink-ponytail-{:032x}", rand::random::<u128>()));
    fs::create_dir(&stage).map_err(|error| format!("无法创建 Ponytail 暂存目录：{error}"))?;
    let result = assemble(source, upstream, &stage).and_then(|()| {
        let nonce = owner
            .as_ref()
            .map(|owner| owner.nonce.clone())
            .unwrap_or_else(|| format!("{:032x}", rand::random::<u128>()));
        fs::write(stage.join(".xlink-owner"), format!("{nonce}\n"))
            .map_err(|error| format!("无法保存 Ponytail 归属标记：{error}"))?;
        ordinary_tree(&stage)?;
        let digest =
            crate::skills::manage::fingerprint_path(&stage).ok_or("无法校验 Ponytail 内容")?;
        if let Some(saved) = &owner {
            if saved.digest != digest {
                return Err("Ponytail 资源已变化，现有插件未覆盖；升级适配尚未完成".into());
            }
        } else {
            use std::io::Write;
            let record = Ownership { nonce, digest };
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(home.join(".xlink-ponytail-owner.json"))
                .map_err(|error| format!("无法创建 Ponytail 归属记录：{error}"))?;
            file.write_all(&serde_json::to_vec(&record).map_err(|error| error.to_string())?)
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("无法保存 Ponytail 归属记录：{error}"))?;
        }
        crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
        if active {
            return Ok(());
        }
        let from = if disabled { &parked } else { &stage };
        rename_exclusive(from, &target)
            .map_err(|error| format!("无法发布 Ponytail，原插件已保留：{error}"))
    });
    cleanup_stage(&stage, result)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn owned_plugin_is_idempotent_and_revocation_preserves_modified_content() {
        use crate::skills::mcode::{acquire_lease, enroll_fresh};
        use sha2::{Digest, Sha256};
        let root = std::env::temp_dir().join(format!(
            "mcode-ponytail-state-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        enroll_fresh("instance").unwrap();
        let home = crate::shell::paths::instance_dsh_home("mcode", "instance");
        let lease = acquire_lease(&home).unwrap();
        let source = root.join("source");
        let upstream = root.join("upstream");
        for path in [
            source.join(".minimax-plugin"),
            source.join("minimax"),
            source.join("host"),
            upstream.join("hooks"),
            upstream.join("skills/ponytail"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        for (name, bytes) in [
            (
                ".minimax-plugin/plugin.json",
                br#"{"skills":[]}"#.as_slice(),
            ),
            ("minimax/hook.mjs", b"hook".as_slice()),
            ("minimax/hooks.json", b"{}".as_slice()),
            ("host/hooks.js", b"host".as_slice()),
            ("package.json", br#"{"type":"module"}"#.as_slice()),
            ("icon.png", b"icon".as_slice()),
        ] {
            fs::write(source.join(name), bytes).unwrap();
        }
        let files: Vec<_> = [".minimax-plugin/plugin.json", "minimax/hook.mjs", "minimax/hooks.json", "host/hooks.js", "package.json", "icon.png"]
            .into_iter().map(|name| {
                let content = fs::read(source.join(name)).unwrap();
                serde_json::json!({ "path": name, "sha256": crate::shell::digest::hex(&Sha256::digest(&content)), "bytes": content.len() })
            }).collect();
        fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({ "files": files })).unwrap(),
        )
        .unwrap();
        fs::write(
            upstream.join("package.json"),
            br#"{"name":"@dietrichgebert/ponytail","version":"1.2.3"}"#,
        )
        .unwrap();
        for name in [
            "ponytail-activate.js",
            "ponytail-mode-tracker.js",
            "ponytail-subagent.js",
        ] {
            fs::write(upstream.join("hooks").join(name), b"hook").unwrap();
        }
        fs::write(upstream.join("skills/ponytail/SKILL.md"), b"skill").unwrap();
        install(&lease, true, &source, &upstream).unwrap();
        install(&lease, true, &source, &upstream).unwrap();
        assert!(verify_active(&lease).unwrap());
        fs::write(upstream.join("skills/ponytail/SKILL.md"), b"updated skill").unwrap();
        fs::write(
            upstream.join("package.json"),
            br#"{"name":"@dietrichgebert/ponytail","version":"1.2.4"}"#,
        )
        .unwrap();
        let missing = root.join("unavailable-source");
        assert!(reuse(&lease, true).unwrap());
        install(&lease, true, &missing, &missing).unwrap();
        let target = home.join("plugins/xlink-ponytail");
        assert!(upgrade(&lease, false, &source, &upstream).is_err());
        assert!(upgrade(&lease, true, &source, &upstream).unwrap());
        assert!(!upgrade(&lease, true, &source, &upstream).unwrap());
        let backups: Vec<_> = fs::read_dir(&home)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".xlink-ponytail-backup-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(backups[0].path().join("skills/ponytail/SKILL.md")).unwrap(),
            b"skill"
        );
        assert_eq!(
            fs::read(target.join("skills/ponytail/SKILL.md")).unwrap(),
            b"updated skill"
        );
        fs::write(upstream.join("skills/ponytail/SKILL.md"), b"second update").unwrap();
        fs::write(
            upstream.join("package.json"),
            br#"{"name":"@dietrichgebert/ponytail","version":"1.2.5"}"#,
        )
        .unwrap();
        let old = ownership(&home).unwrap().unwrap();
        let transaction = "a".repeat(32);
        let stage = home.join(format!(".xlink-ponytail-upgrade-stage-{transaction}"));
        let backup = home.join(format!(".xlink-ponytail-backup-{transaction}"));
        fs::create_dir(&stage).unwrap();
        assemble(&source, &upstream, &stage).unwrap();
        fs::write(stage.join(".xlink-owner"), format!("{}\n", old.nonce)).unwrap();
        let journal = UpgradeJournal {
            version: 1,
            transaction,
            nonce: old.nonce,
            old_digest: old.digest,
            new_digest: crate::skills::manage::fingerprint_path(&stage).unwrap(),
        };
        fs::write(
            home.join(".xlink-ponytail-upgrade.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        rename_exclusive(&target, &backup).unwrap();
        recover_upgrade(&lease).unwrap();
        assert_eq!(
            fs::read(target.join("skills/ponytail/SKILL.md")).unwrap(),
            b"second update"
        );
        assert_eq!(
            fs::read(backup.join("skills/ponytail/SKILL.md")).unwrap(),
            b"updated skill"
        );
        assert!(!home.join(".xlink-ponytail-upgrade.json").exists());
        disable(&lease).unwrap();
        disable(&lease).unwrap();
        assert!(!target.exists());
        assert!(home.join(".xlink-ponytail-disabled").is_dir());
        assert!(reuse(&lease, true).unwrap());
        fs::write(target.join("skills/ponytail/SKILL.md"), b"edited by user").unwrap();
        assert!(disable(&lease).is_err());
        assert_eq!(
            fs::read(target.join("skills/ponytail/SKILL.md")).unwrap(),
            b"edited by user"
        );
        drop(lease);
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn publication_requires_consent_and_offline_lease_and_preserves_conflicts() {
        use crate::skills::mcode::{
            acquire_lease, arm_start, enroll_fresh, finish_start, StopEvidence,
        };
        let root = std::env::temp_dir().join(format!(
            "mcode-ponytail-publish-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        enroll_fresh("instance").unwrap();
        let home = crate::shell::paths::instance_dsh_home("mcode", "instance");
        let lease = acquire_lease(&home).unwrap();
        let missing = root.join("unused");
        assert!(install(&lease, false, &missing, &missing).is_err());
        assert!(!home.join("plugins").exists());
        let target = home.join("plugins/xlink-ponytail");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep"), b"user content").unwrap();
        assert!(install(&lease, true, &missing, &missing).is_err());
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"user content");
        let token = arm_start(&lease, 1).unwrap();
        assert!(install(&lease, true, &missing, &missing).is_err());
        finish_start(&lease, &token, StopEvidence::NeverSpawned).unwrap();
        drop(lease);
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn assembly_preserves_existing_targets_and_advertises_only_discovered_skills() {
        let root =
            std::env::temp_dir().join(format!("mcode-ponytail-{:032x}", rand::random::<u128>()));
        let source = root.join("source");
        let upstream = root.join("upstream");
        let stage = root.join("stage");
        for path in [
            source.join(".minimax-plugin"),
            source.join("minimax"),
            source.join("host"),
            upstream.join("hooks"),
            upstream.join("skills/ponytail"),
            stage.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            source.join(".minimax-plugin/plugin.json"),
            br#"{"skills":[]}"#,
        )
        .unwrap();
        fs::write(source.join("minimax/hook.mjs"), b"adapter").unwrap();
        fs::write(source.join("minimax/hooks.json"), b"{}").unwrap();
        fs::write(source.join("package.json"), br#"{"type":"module"}"#).unwrap();
        fs::write(source.join("icon.png"), b"icon").unwrap();
        fs::write(source.join("host/hooks.js"), b"host hook").unwrap();
        fs::write(
            upstream.join("package.json"),
            br#"{"name":"@dietrichgebert/ponytail","version":"1.2.3"}"#,
        )
        .unwrap();
        for name in [
            "ponytail-activate.js",
            "ponytail-mode-tracker.js",
            "ponytail-subagent.js",
        ] {
            fs::write(upstream.join("hooks").join(name), b"hook").unwrap();
        }
        fs::write(upstream.join("skills/ponytail/SKILL.md"), b"skill").unwrap();
        assemble(&source, &upstream, &stage).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(stage.join(".minimax-plugin/plugin.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["skills"],
            serde_json::json!(["skills/ponytail/SKILL.md"])
        );
        assert!(assemble(&source, &upstream, &stage).is_err());
        assert_eq!(
            fs::read(stage.join("skills/ponytail/SKILL.md")).unwrap(),
            b"skill"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
