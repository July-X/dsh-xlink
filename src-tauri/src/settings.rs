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
    /// 任务完成通知总开关。`None` 表示用户从未设置过（走 [`crate::notify`]
    /// 的默认值），而不是"关闭"——面板的 `save_settings` 只提交端口与
    /// profile，用 `bool` 会让每次保存设置都把这些开关静默重置。
    pub notify_enabled: Option<bool>,
    /// 仅当工作台窗口不在前台时才弹通知气泡（未读计数不受此开关影响）。
    pub notify_away_only: Option<bool>,
    /// 通知是否带提示音。
    pub notify_sound: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            node_path: None,
            pnpm_path: None,
            npm_path: None,
            port: DEFAULT_PORT,
            profile: crate::plugins::DEFAULT_PROFILE.to_string(),
            notify_enabled: None,
            notify_away_only: None,
            notify_sound: None,
        }
    }
}

/// 设置文件在 `data_dir` 下的路径。
pub fn settings_file(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("settings.json")
}

/// 读取设置。文件缺失（首次启动的正常路径）静默返回默认值；
/// **解析失败**时备份损坏文件并返回一条可操作的诊断。
///
/// 旧实现把"不存在"和"坏了/读不出来"一起吞成默认值：用户自定义的端口会
/// 无声回退到 3090/3091，面板上没有任何提示，重启后"工作台怎么跑到别的
/// 端口去了"完全无从解释（P2-19）。
pub fn load_checked(data_dir: &Path) -> (Settings, Option<String>) {
    let path = settings_file(data_dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Settings::default(), None)
        }
        Err(error) => {
            return (
                Settings::default(),
                Some(format!(
                    "设置文件无法读取（{error}），已改用默认设置（端口回到默认端口）。文件：{}",
                    path.display()
                )),
            )
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) => (settings, None),
        Err(error) => {
            // 备份而不是让下一次 save 直接覆盖：损坏内容往往是用户手改错了一
            // 个字符，留住它才能改回来。
            let backup = path.with_extension("json.corrupt");
            let backed_up = fs::copy(&path, &backup).is_ok();
            let detail = if backed_up {
                format!(
                    "原文件已备份到 {}，修好它并重启应用即可恢复",
                    backup.display()
                )
            } else {
                format!("原文件未能备份，请先自行复制 {} 再修改", path.display())
            };
            (
                Settings::default(),
                Some(format!(
                    "设置文件损坏（{error}），已改用默认设置（端口回到默认端口）。{detail}"
                )),
            )
        }
    }
}

/// 读取设置，忽略诊断信息。需要向用户展示"设置被回退了"的调用方应当用
/// [`load_checked`]。
pub fn load(data_dir: &Path) -> Settings {
    load_checked(data_dir).0
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

#[cfg(test)]
mod load_checked_tests {
    use super::*;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-settings-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_not_a_warning() {
        let dir = temp_dir("missing");
        let (settings, warning) = load_checked(&dir);
        assert_eq!(settings.port, DEFAULT_PORT);
        assert!(warning.is_none(), "首次启动的缺文件不该报警：{warning:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_is_reported_and_backed_up() {
        // P2-19：损坏的设置此前会静默回退到默认端口，用户无从得知。
        let dir = temp_dir("corrupt");
        let path = settings_file(&dir);
        std::fs::write(&path, b"{ \"port\": 3095, ").unwrap();

        let (settings, warning) = load_checked(&dir);
        let warning = warning.expect("损坏必须产生诊断");
        assert_eq!(settings.port, DEFAULT_PORT, "损坏时回退到默认设置");
        assert!(warning.contains("损坏"), "诊断要说明是损坏：{warning}");
        assert!(
            warning.contains("备份"),
            "诊断要说清原文件去哪了：{warning}"
        );
        assert!(
            dir.join("settings.json.corrupt").exists(),
            "损坏的原文件必须被备份下来"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_file_round_trips_without_warning() {
        let dir = temp_dir("valid");
        let settings = Settings {
            port: 3199,
            ..Settings::default()
        };
        save(&dir, &settings).expect("save");

        let (loaded, warning) = load_checked(&dir);
        assert_eq!(loaded.port, 3199);
        assert!(warning.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
