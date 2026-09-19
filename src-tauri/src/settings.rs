//! 桌面外壳的持久化设置。
//!
//! P1 之后，**Shell 设置**保存到 `<xlink_home>/shell/<mode>/settings.json`，
//! 由 [`load_for_shell`] / [`save_for_shell`] 读写。旧版「按 data_dir 读写」
//! 的 [`load`] / [`save`] 仍保留，作为只读兼容入口供迁移期使用：P6 之前
//! 不应再调用它们。
//!
//! 旧布局是 `<dsh_home>/desktop[-dev]/settings.json`（参见
//! [`crate::kernel::data_dir`]），路径在 [`settings_file`]；新布局由
//! [`crate::paths::shell_settings_file`] 给出。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths::{self, ShellMode};
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

/// 设置文件在 `data_dir` 下的路径。**仅用于旧版 data_dir 解析**——P1 之后
/// 新写入请用 [`paths::shell_settings_file`]。
#[cfg_attr(not(test), allow(dead_code))]
pub fn settings_file(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("settings.json")
}

/// 读取设置。文件缺失（首次启动的正常路径）静默返回默认值；
/// **解析失败**时备份损坏文件并返回一条可操作的诊断。
///
/// 旧实现把"不存在"和"坏了/读不出来"一起吞成默认值：用户自定义的端口会
/// 无声回退到 3090/3091，面板上没有任何提示，重启后"工作台怎么跑到别的
/// 端口去了"完全无从解释（P2-19）。
///
/// **仅用于旧版 data_dir 解析**。P1 之后新代码请用 [`load_for_shell`]。
#[cfg_attr(not(test), allow(dead_code))]
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
            //
            // 只在备份内容与当前损坏文件不同时才复制：
            // ① `kernel::status()` 每 2.5s 轮询一次、每次都走 `load_checked`，
            //    无条件 `fs::copy` 会在用户按提示修好文件之前反复重写同一个备份
            //    （白写盘，面板上的备份 mtime 也一直在动，看起来像"又在损坏"）；
            // ② 内容相同就跳过，因此同一份损坏只写一次；用户真的又改出新内容时
            //    才重新覆盖，备份始终对应磁盘上当前那份损坏文件。
            let backup = path.with_extension("json.corrupt");
            let already_saved = fs::read(&backup)
                .map(|previous| previous == text.as_bytes())
                .unwrap_or(false);
            let backed_up = already_saved || fs::copy(&path, &backup).is_ok();
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
///
/// **仅用于旧版 data_dir 解析**。P1 之后新代码请用 [`load_for_shell`]。
#[cfg_attr(not(test), allow(dead_code))]
pub fn load(data_dir: &Path) -> Settings {
    load_checked(data_dir).0
}

/// 持久化设置，必要时创建父目录。
///
/// **仅用于旧版 data_dir 解析**。P1 之后新代码请用 [`save_for_shell`]。
#[cfg_attr(not(test), allow(dead_code))]
pub fn save(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    let path = settings_file(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// 读取当前 Shell 模式下的设置。文件位于
/// `<xlink_home>/shell/<mode>/settings.json`；首次启动或模式全新时
/// 文件不存在，返回默认值与 `None` 诊断。
///
/// 与 [`load_checked`] 共享损坏备份策略——新写入与新损坏都走同一条
/// 「不反复重写备份」的逻辑，保证面板上看到的 mtime 与磁盘一致。
pub fn load_checked_for_shell(mode: ShellMode) -> (Settings, Option<String>) {
    let path = paths::shell_settings_file(mode);
    load_checked_inner(&path, mode)
}

/// 读取当前 Shell 模式的设置，忽略诊断信息。
pub fn load_for_shell(mode: ShellMode) -> Settings {
    load_checked_for_shell(mode).0
}

/// 持久化当前 Shell 模式的设置。
///
/// 写之前确保父目录存在；与旧 [`save`] 同样使用 [`atomic_write`]
/// 避免写盘过程中读到截断 JSON。
pub fn save_for_shell(mode: ShellMode, settings: &Settings) -> Result<(), String> {
    let path = paths::shell_settings_file(mode);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// 当前 Shell 模式快捷别名。生产代码里反复写 `paths::ShellMode::current()`
/// 既冗长又容易漂移到不相关的语境——把"settings 与 Shell 模式绑定"这层
/// 语义集中到 settings 模块里。
pub fn current_mode() -> ShellMode {
    paths::ShellMode::current()
}

fn load_checked_inner(path: &Path, _mode: ShellMode) -> (Settings, Option<String>) {
    // 故意复用 [`load_checked`] 的语义，但 `path` 不再依赖 data_dir：
    // 我们直接传任意 `&Path` 给读 + 解析 + 备份逻辑。新旧布局共用一套
    // 损坏处理，避免行为分叉。
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Settings::default(), None);
        }
        Err(error) => {
            return (
                Settings::default(),
                Some(format!(
                    "设置文件无法读取（{error}），已改用默认设置（端口回到默认端口）。文件：{}",
                    path.display()
                )),
            );
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) => (settings, None),
        Err(error) => {
            let backup = path.with_extension("json.corrupt");
            let already_saved = fs::read(&backup)
                .map(|previous| previous == text.as_bytes())
                .unwrap_or(false);
            let backed_up = already_saved || fs::copy(path, &backup).is_ok();
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
    fn corrupt_file_is_not_backed_up_again_on_every_poll() {
        // `kernel::status()` 每 2.5s 轮询一次、每次都调 `load_checked`：无条件
        // `fs::copy` 会一直重写同一个备份。把备份设成只读来观测"到底有没有再写"——
        // 内容与磁盘上那份损坏文件一致时必须跳过复制（复制到只读目标会失败，
        // 于是诊断会从"已备份"变成"未能备份"）。这里不还原权限：删除只读文件
        // 取决于父目录是否可写，清理不受影响；而 `set_readonly(false)` 在 Unix 上
        // 会把文件变成人人可写，clippy 会直接拦。
        let dir = temp_dir("corrupt-once");
        let path = settings_file(&dir);
        let corrupt: &[u8] = b"{ \"port\": 3095, ";
        std::fs::write(&path, corrupt).unwrap();

        let (_, first) = load_checked(&dir);
        let first = first.expect("首次损坏必须产生诊断");
        assert!(first.contains("已备份"), "首次应真的写出备份：{first}");

        let backup = dir.join("settings.json.corrupt");
        let mut readonly = std::fs::metadata(&backup).unwrap().permissions();
        readonly.set_readonly(true);
        std::fs::set_permissions(&backup, readonly).unwrap();

        // 同一份损坏内容再读两次：都不该尝试写备份。
        for round in 1..=2 {
            let (_, warning) = load_checked(&dir);
            let warning = warning.expect("损坏依旧要产生诊断");
            assert!(
                warning.contains("已备份"),
                "第 {round} 次轮询不该重写备份（说明又执行了一次 fs::copy）：{warning}"
            );
        }
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            corrupt,
            "跳过的只是重复写入，备份内容必须原样保留"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_backup_follows_changed_content() {
        // 跳过重复写入不能变成"永远不再更新"：用户又改出一份新的损坏内容时，
        // 备份必须跟着变，否则照备份里的旧内容改不回去。
        let dir = temp_dir("corrupt-changed");
        let path = settings_file(&dir);
        std::fs::write(&path, b"{ \"port\": 3095, ").unwrap();
        let (_, warning) = load_checked(&dir);
        assert!(warning.expect("首次损坏要产生诊断").contains("已备份"));

        let changed: &[u8] = b"{ \"port\": 3096, ";
        std::fs::write(&path, changed).unwrap();
        let (_, warning) = load_checked(&dir);
        assert!(warning.expect("仍要产生诊断").contains("已备份"));
        assert_eq!(
            std::fs::read(dir.join("settings.json.corrupt")).unwrap(),
            changed,
            "损坏内容变化后备份必须跟着更新"
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

#[cfg(test)]
mod shell_aware_tests {
    use super::*;
    use crate::paths::ShellMode;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-settings-shell-{}-{}-{}",
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

    /// release / dev 的设置文件必须互不串写：同一目录树下两个 mode 的文件
    /// 同时存在时不会相互覆盖。
    ///
    /// P4 起改用共享 RAII `crate::tests::scoped_xlink_home`，与
    /// `plugins::TestHome` 走同一把进程级互斥锁——之前本模块自带的
    /// `ENV_LOCK` 跟 plugin 那边不互通，并行跑会同时改 DSH_XLINK_HOME
    /// 把对方覆盖。
    #[test]
    fn release_and_dev_settings_do_not_cross() {
        let home = temp_dir("shell-isolated");
        let _guard = crate::tests::scoped_xlink_home(&home);

        let release = Settings {
            port: 3190,
            ..Settings::default()
        };
        let dev = Settings {
            port: 3191,
            ..Settings::default()
        };

        save_for_shell(ShellMode::Release, &release).expect("save release");
        save_for_shell(ShellMode::Dev, &dev).expect("save dev");

        let loaded_release = load_for_shell(ShellMode::Release);
        let loaded_dev = load_for_shell(ShellMode::Dev);
        assert_eq!(loaded_release.port, 3190, "release 应保留自己的端口");
        assert_eq!(loaded_dev.port, 3191, "dev 应保留自己的端口");

        // 文件落点必须分别落在 shell/release 与 shell/dev。
        let release_path = home.join("shell").join("release").join("settings.json");
        let dev_path = home.join("shell").join("dev").join("settings.json");
        assert!(
            release_path.exists(),
            "release 设置文件应存在：{}",
            release_path.display()
        );
        assert!(
            dev_path.exists(),
            "dev 设置文件应存在：{}",
            dev_path.display()
        );

        std::fs::remove_dir_all(&home).ok();
    }

    /// 首次启动的 release 与 dev 都应该是「缺文件 = 默认 + 无警告」。
    #[test]
    fn shell_settings_missing_is_not_a_warning() {
        let home = temp_dir("shell-missing");
        let _guard = crate::tests::scoped_xlink_home(&home);

        let (release, warn_release) = load_checked_for_shell(ShellMode::Release);
        let (dev, warn_dev) = load_checked_for_shell(ShellMode::Dev);
        assert_eq!(release.port, DEFAULT_PORT);
        assert_eq!(dev.port, DEFAULT_PORT);
        assert!(warn_release.is_none(), "release 缺文件不该报警");
        assert!(warn_dev.is_none(), "dev 缺文件不该报警");

        std::fs::remove_dir_all(&home).ok();
    }

    /// 损坏备份策略与旧版一致：内容相同不重写，损坏再次发生仍备份。
    #[test]
    fn shell_settings_corrupt_backup_follows_content() {
        let home = temp_dir("shell-corrupt");
        let _guard = crate::tests::scoped_xlink_home(&home);

        let path = crate::paths::shell_settings_file(ShellMode::Release);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ \"port\": 3192, ").unwrap();

        let (_, first) = load_checked_for_shell(ShellMode::Release);
        assert!(first.expect("首次损坏必须报警").contains("已备份"));

        let backup = path.with_extension("json.corrupt");
        let mut readonly = std::fs::metadata(&backup).unwrap().permissions();
        readonly.set_readonly(true);
        std::fs::set_permissions(&backup, readonly).unwrap();

        let (_, second) = load_checked_for_shell(ShellMode::Release);
        assert!(second.expect("损坏依旧要报警").contains("已备份"));

        std::fs::remove_dir_all(&home).ok();
    }
}
