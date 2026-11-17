//! Hook 模块配置：`<xlink_home>/shell/<mode>/hooks/config.json`。
//!
//! 与全局 `settings.json` 分开：Hook 是独立能力，启停与端口不应和内核端口
//! 混在一张表单里，也不该被面板的「保存设置」顺手重置。默认关闭；文件不存在
//! 是正常的首次运行，损坏或版本更高时**不覆盖**，交给调用方显示错误。

use serde::{Deserialize, Serialize};

use crate::shell::atomic_write::atomic_write_private;
use crate::shell::paths::ShellMode;
use crate::shell::process::{read_state_file, StateRead};
use crate::shell::{paths, settings};

/// 配置 schema 版本。新增字段或改变语义时提升；更高版本拒绝打开。
pub(crate) const CONFIG_SCHEMA_VERSION: u32 = 2;

/// Hook 默认端口：release 与 dev 各一份，两个壳可以同时启用而不抢端口。
pub(crate) const DEFAULT_PORT: u16 = if cfg!(debug_assertions) { 17891 } else { 17890 };

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HooksConfig {
    pub schema_version: u32,
    /// 用户是否要求启用（期望状态）。实际是否监听由运行时另报，端口被占用时
    /// 这里仍可为 true，`hooks_status.last_error` 会说明原因。
    pub enabled: bool,
    pub port: u16,
    pub interface_name: String,
    pub bind_ip: String,
    pub netmask: String,
    pub allowed_subnets: Vec<String>,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: false,
            port: DEFAULT_PORT,
            interface_name: String::new(),
            bind_ip: String::new(),
            netmask: String::new(),
            allowed_subnets: Vec::new(),
        }
    }
}

/// 读当前壳模式的配置，区分「不存在」「可读」「损坏」。
pub(crate) fn load() -> StateRead<HooksConfig> {
    read_state_file(&paths::hooks_config_file(settings::current_mode()))
}

/// 保存配置前先确认现有文件没有损坏、也没有被更高版本写过：这两种情况下
/// 覆盖会把用户唯一能人工修复的证据抹掉。
pub(crate) fn check_writable() -> Result<(), String> {
    match load() {
        StateRead::Loaded(config) if config.schema_version > CONFIG_SCHEMA_VERSION => Err(format!(
            "Hook 配置版本 {} 高于本版本支持的 {}；请升级应用，或备份后移除 {}",
            config.schema_version,
            CONFIG_SCHEMA_VERSION,
            paths::hooks_config_file(settings::current_mode()).display()
        )),
        StateRead::Corrupt { reason } => Err(format!(
            "Hook 配置无法读取（{reason}）。请修复或删除 {} 后重启应用",
            paths::hooks_config_file(settings::current_mode()).display()
        )),
        _ => Ok(()),
    }
}

/// 校验并原子写入。目录权限在 Unix 上收紧到 0700；配置文件 0600。
pub(crate) fn save(config: &HooksConfig) -> Result<(), String> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "Hook 配置版本必须为 {CONFIG_SCHEMA_VERSION}，拒绝写入 {}",
            config.schema_version
        ));
    }
    if config.port == 0 {
        return Err("Hook 端口不能为 0；请填写 1–65535 之间的端口".into());
    }
    let dir = ensure_dir()?;
    let bytes =
        serde_json::to_vec_pretty(config).map_err(|e| format!("Hook 配置无法序列化：{e}"))?;
    atomic_write_private(&dir.join("config.json"), &bytes).map_err(|e| {
        format!(
            "无法写入 Hook 配置 {}：{e}",
            dir.join("config.json").display()
        )
    })
}

/// 建立 `hooks/` 目录并收紧权限。返回目录路径。
pub(crate) fn ensure_dir() -> Result<std::path::PathBuf, String> {
    let mode = settings::current_mode();
    let dir = paths::hooks_dir(mode);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建 Hook 数据目录 {}：{e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

/// 当前壳模式的配置路径（命令层展示用）。
pub(crate) fn config_path(mode: ShellMode) -> std::path::PathBuf {
    paths::hooks_config_file(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    fn temp_home(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-hooks-config-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("创建临时 home");
        dir
    }

    #[test]
    fn missing_file_is_default_and_disabled() {
        let home = temp_home("missing");
        let _guard = scoped_xlink_home(&home);
        assert!(matches!(load(), StateRead::Missing));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn save_then_load_round_trips_and_defaults_off() {
        let home = temp_home("roundtrip");
        let _guard = scoped_xlink_home(&home);
        let config = HooksConfig {
            enabled: true,
            port: 17999,
            ..HooksConfig::default()
        };
        save(&config).expect("保存配置");
        match load() {
            StateRead::Loaded(loaded) => assert_eq!(loaded, config),
            _ => panic!("应能读回配置"),
        }
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn corrupt_config_is_reported_and_not_overwritten_by_check() {
        let home = temp_home("corrupt");
        let _guard = scoped_xlink_home(&home);
        let dir = paths::hooks_dir(settings::current_mode());
        std::fs::create_dir_all(&dir).expect("创建目录");
        std::fs::write(dir.join("config.json"), b"{not-json").expect("写坏文件");
        assert!(matches!(load(), StateRead::Corrupt { .. }));
        assert!(check_writable().is_err(), "损坏时不允许覆盖保存");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn newer_config_version_is_refused() {
        let home = temp_home("newer");
        let _guard = scoped_xlink_home(&home);
        let dir = paths::hooks_dir(settings::current_mode());
        std::fs::create_dir_all(&dir).expect("创建目录");
        std::fs::write(
            dir.join("config.json"),
            format!(
                r#"{{"schema_version":{},"enabled":true,"port":17890}}"#,
                CONFIG_SCHEMA_VERSION + 1
            ),
        )
        .expect("写高版本配置");
        assert!(check_writable().is_err(), "更高版本不允许覆盖");
        std::fs::remove_dir_all(&home).ok();
    }

    #[cfg(unix)]
    #[test]
    fn hooks_dir_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let home = temp_home("perm");
        let _guard = scoped_xlink_home(&home);
        let dir = ensure_dir().expect("建目录");
        let mode = std::fs::metadata(&dir)
            .expect("读权限")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::remove_dir_all(&home).ok();
    }
}
