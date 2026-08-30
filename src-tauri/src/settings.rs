//! 桌面外壳的持久化设置。
//!
//! 设置保存在 `<data_dir>/settings.json` 中，以扁平的 JSON 结构组织，UI 可
//! 以通过普通的命令往返来读写它们（`<data_dir>` 是 `<dsh_home>/desktop[-dev]/`，
//! 详见 [`crate::kernel::data_dir`]）。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::process::atomic_write;

/// 用户尚未保存自己的端口值时，管理面板期望 dsh web 服务器所使用的端口。
/// 这里重新导出 [`crate::kernel::DEFAULT_PORT`]，避免两个定义发生偏移——
/// debug 构建（3091）和 release 构建（3090）共用同一个回退值。
pub use crate::kernel::DEFAULT_PORT;

/// 桌面外壳运行内核所需的用户配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `node` 可执行文件的显式路径；当为空时由外壳从环境中探测。
    pub node_path: Option<String>,
    /// `pnpm` 可执行文件的显式路径；当为空时从 `node` 旁边或环境中解析。
    /// pnpm 用来安装内核版本。
    pub pnpm_path: Option<String>,
    /// `npm` 可执行文件的显式路径；当为空时从 `node` 旁边或环境中解析。
    /// 在 pnpm 缺失时，npm 是自动安装的备选，因此对于自定义安装（便携式
    /// 布局、未带 node 同伴 npm 的 nvm）需要通过此字段跳过无效的探测。
    pub npm_path: Option<String>,
    /// 内核的 web UI 监听的端口（dsh 默认 3080）。
    pub port: u16,
    /// 外壳将插件接入的 profile 名称（dsh 默认：web）。
    pub profile: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            node_path: None,
            pnpm_path: None,
            npm_path: None,
            port: DEFAULT_PORT,
            profile: crate::plugins::DEFAULT_PROFILE.to_string(),
        }
    }
}

/// 设置文件在 `data_dir` 下的路径。
pub fn settings_file(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("settings.json")
}

/// 读取设置，文件缺失或无法读取时返回默认值。
pub fn load(data_dir: &Path) -> Settings {
    let path = settings_file(data_dir);
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 持久化设置，必要时创建父目录。
pub fn save(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    let path = settings_file(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UI 只发送 `{port, profile}`；Tauri 用 serde_json::from_value 反序列化
    /// `settings` 参数，和这里的做法一致。
    #[test]
    fn ui_payload_deserializes() {
        let v = serde_json::json!({"port": 8080, "profile": "web"});
        let s: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(s.port, 8080);
        assert_eq!(s.node_path, None);
    }

    /// 超出 u16 范围的端口会在 IPC 边界被拒绝；UI 在发送前会校验
    /// 1024–65535，因此用户会得到一条可操作的提示。
    #[test]
    fn out_of_range_port_rejected() {
        let v = serde_json::json!({"port": 70000, "profile": "web"});
        assert!(serde_json::from_value::<Settings>(v).is_err());
    }
}
