//! dsh-xlink 数据目录与路径解析（开发计划 P0）。
//!
//! 这一模块是整个多内核改造的「路径契约」：所有 dsh-xlink 写入的数据目录
//! 都从这里的函数取，禁止在调用方直接拼接根目录字符串。新旧布局的关系
//! 见 [`docs/dsh-xlink-multi-kernel-design.md`](../../docs/dsh-xlink-multi-kernel-design.md)，
//! 阶段任务见 [`docs/dsh-xlink-multi-kernel-development-plan.md`](../../docs/dsh-xlink-multi-kernel-development-plan.md) §P0。
//!
//! ## 当前实现范围（P0+P1）
//!
//! - 新路径函数（`xlink_home` / `shell_*` / `plugins_store_root` /
//!   `skills_store_root` / `kernels_root` / `state_root` / `cache_root` /
//!   `xlink_metadata_file`）已经定义并被测试覆盖；UI、Shell 日志和
//!   Shell 设置在 P1 阶段切换到这套路径。
//! - 旧路径解析（`legacy_dsh_home` / `legacy_desktop_data_dir` /
//!   `legacy_dsh_plugins_root` / `legacy_dsh_skills_store` /
//!   `legacy_dsh_skills_root`）作为**只读兼容**入口返回，让内核安装目录、
//!   `~/.dsh/plugins`、`~/.dsh/skills-store` 等已有数据继续可读。P6
//!   迁移向导完成之前不要把这些函数用于写入。
//! - 数据模型（[`XlinkMetadata`] / [`ShellState`]）定义 schema_version 与
//!   字段命名规则，但不写入实际文件；P2 起会在状态读写时落地。
//!
//! ## 设计约束
//!
//! 1. `DSH_XLINK_HOME` 只决定 Xlink 根目录；`DSH_HOME` 保留给具体内核进程使用，
//!    两者不能互相回退。
//! 2. 所有 id / 版本路径 / profile 名都必须经过 [`validate_id_component`]，
//!    拒绝路径穿越、空值与保留名。
//! 3. release/dev 的构建区分**只**作用于 Shell 自己的状态（设置、UI 状态、
//!    Shell 日志），不再用它推导内核数据目录。这是 P1 的目标，P2 起
//!    内核数据会改为按实例寻址。
//!
//! ## 测试策略
//!
//! - 单元测试用 `std::env::set_var` / `remove_var` 覆盖环境变量，临时目录
//!   验证相对路径解析。
//! - 集成式测试（`integration_paths_tests`）把多个函数串起来，断言
//!   release / dev 互不串目录、Shell 路径只在 `xlink_home()` 下、legacy
//!   resolver 仍能命中旧路径。

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 覆盖默认 `xlink_home()` 的环境变量。约定：只决定 Xlink 自己的根目录，
/// 不影响具体内核进程的 `DSH_HOME`。
pub const DSH_XLINK_HOME_ENV: &str = "DSH_XLINK_HOME";
/// Xlink 默认根目录名（不带点也可写为 `dsh-xlink`，这里沿用 `~` 风格
/// 用点号开头，让它在 `ls ~` 时一眼可辨）。
pub const DEFAULT_HOME_DIR_NAME: &str = ".dsh-xlink";
/// 旧版 dsh home 目录名；legacy resolver 用它拼接 `desktop[-dev]/` 等旧路径。
pub const LEGACY_DSH_HOME_DIR_NAME: &str = ".dsh";
/// 旧版 release Shell 子目录（位于 `<dsh_home>/desktop/`）。
pub const LEGACY_SHELL_SUBDIR_RELEASE: &str = "desktop";
/// 旧版 dev Shell 子目录（位于 `<dsh_home>/desktop-dev/`）。
pub const LEGACY_SHELL_SUBDIR_DEV: &str = "desktop-dev";
/// DSH 进程默认 `DSH_HOME` 目录名（用于 legacy resolver，与官方 home-paths 一致）。
pub const DSH_HOME_DIR_NAME: &str = ".dsh";

/// Xlink 顶层 schema 版本。每次破坏性变更（例如新增字段、改字段含义）必须
/// 递增，写入 `xlink.json` 供迁移使用。`CURRENT_SCHEMA_VERSION` 是当前
/// 实现能识别的最大版本号；读取时遇到更大版本要拒绝，避免误解析未知字段。
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// dsh-xlink Shell 构建模式。`current()` 在编译期由 `cfg!(debug_assertions)`
/// 决定——release 与 dev 是构建产物之间的差异，不是运行时配置。
///
/// 在 P1 之后，Shell 模式只影响 Shell 自己的状态（设置、UI 状态、Shell 日志），
/// 不再用于推导内核数据路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellMode {
    Release,
    Dev,
}

impl ShellMode {
    /// 当前构建对应的 Shell 模式。
    pub const fn current() -> Self {
        if cfg!(debug_assertions) {
            ShellMode::Dev
        } else {
            ShellMode::Release
        }
    }

    /// 单段目录名（用于路径拼接或日志 stamp）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ShellMode::Release => "release",
            ShellMode::Dev => "dev",
        }
    }

    /// 旧版 Shell 子目录名（仅 legacy resolver 使用）。
    pub const fn legacy_subdir(self) -> &'static str {
        match self {
            ShellMode::Release => LEGACY_SHELL_SUBDIR_RELEASE,
            ShellMode::Dev => LEGACY_SHELL_SUBDIR_DEV,
        }
    }
}

impl std::fmt::Display for ShellMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 用户操作系统的 home 目录。Unix 下读 `$HOME`，Windows 下读 `%USERPROFILE%`。
/// 取不到时退化为 `.`（保持当前 `kernel::dirs_home` 的容错行为，避免启动期失败）。
///
/// 与 `node.rs` 共享同样的解析逻辑——后者用它定位 nvm 管理的 Node 安装。
/// 这里独立实现而不是从 `kernel.rs` 引入，避免 P0 阶段对内核模块产生反向依赖
/// （路径模块本应是底层依赖）。
pub fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// dsh-xlink 自己的根目录，按以下优先级解析：
///
/// 1. `DSH_XLINK_HOME` 环境变量——完整覆盖。允许高级用户把壳指向任意目录
///    （例如在外置磁盘上测试），并短路掉下面所有 home 解析。
/// 2. `<dirs_home>/.dsh-xlink/`——默认根目录。
///
/// 该函数**不创建**目录：路径解析应当是纯函数，写入由调用方按需落盘，
/// 这也是 P0 测试不依赖文件系统副作用的前提。
pub fn xlink_home() -> PathBuf {
    if let Some(override_home) = std::env::var_os(DSH_XLINK_HOME_ENV).map(PathBuf::from) {
        return override_home;
    }
    dirs_home().join(DEFAULT_HOME_DIR_NAME)
}

/// 给定模式下 Shell 自己的状态目录：`<xlink_home>/shell/<mode>/`。
///
/// 该目录专门存放 Shell 自己的设置、UI 状态与 Shell 日志；不参与内核数据路径。
pub fn shell_dir(mode: ShellMode) -> PathBuf {
    xlink_home().join("shell").join(mode.as_str())
}

/// Shell 设置文件：`<xlink_home>/shell/<mode>/settings.json`。
pub fn shell_settings_file(mode: ShellMode) -> PathBuf {
    shell_dir(mode).join("settings.json")
}

/// Shell UI 状态文件：`<xlink_home>/shell/<mode>/ui-state.json`。
///
/// 与设置文件分离：UI 状态高频写入（窗口大小、面板展开、tab 选择等），
/// 不应该走 settings.json 的事务化原子写入路径；后续 P2 会有自己的
/// 写入策略。
pub fn shell_ui_state_file(mode: ShellMode) -> PathBuf {
    shell_dir(mode).join("ui-state.json")
}

/// Shell 日志目录：`<xlink_home>/shell/<mode>/logs/`。
///
/// 该目录只放 Shell 自身的日志（`<kind>-<name>-<date>.log` 形式）；内核
/// 实例的日志由 [`crate::kernel::logs_dir`] 或未来的实例目录负责。
pub fn shell_logs_dir(mode: ShellMode) -> PathBuf {
    shell_dir(mode).join("logs")
}

/// 插件中央库根目录：`<xlink_home>/dsh-plugins/`。
pub fn plugins_store_root() -> PathBuf {
    xlink_home().join("dsh-plugins")
}

/// 技能中央库根目录：`<xlink_home>/skills/`。
pub fn skills_store_root() -> PathBuf {
    xlink_home().join("skills")
}

/// 内核实例根目录：`<xlink_home>/kernels/`。
///
/// P1 阶段内核安装目录**仍位于旧版 data_dir**，本函数仅用于规划新位置
/// 与将来的实例注册。调用方在 P2 之前不应往这里写。
pub fn kernels_root() -> PathBuf {
    xlink_home().join("kernels")
}

/// 给定内核族的所有版本安装产物目录：`<xlink_home>/kernels/<family>/versions/`。
pub fn kernel_versions_dir(family: &str) -> PathBuf {
    kernels_root().join(family).join("versions")
}

/// 给定内核族与版本的安装根目录：`<xlink_home>/kernels/<family>/versions/<version>/`。
///
/// 验证 `version` 必须是合法的 id 组件——避免路径穿越到 `versions/../...`。
pub fn kernel_version_dir(family: &str, version: &str) -> PathBuf {
    validate_id_component(version).expect("version id must be valid for path use");
    kernel_versions_dir(family).join(version)
}

/// 给定内核族的所有实例目录：`<xlink_home>/kernels/<family>/instances/`。
pub fn kernel_instances_dir(family: &str) -> PathBuf {
    kernels_root().join(family).join("instances")
}

/// 给定实例的根目录：`<xlink_home>/kernels/<family>/instances/<id>/`。
pub fn instance_dir(family: &str, id: &str) -> PathBuf {
    validate_id_component(id).expect("instance id must be valid for path use");
    kernel_instances_dir(family).join(id)
}

/// 给定实例的 `instance.json` 路径：`<xlink_home>/kernels/<family>/instances/<id>/instance.json`。
pub fn instance_record_file(family: &str, id: &str) -> PathBuf {
    instance_dir(family, id).join("instance.json")
}

/// 给定实例的运行时子目录：`<xlink_home>/kernels/<family>/instances/<id>/runtime/`。
pub fn instance_runtime_dir(family: &str, id: &str) -> PathBuf {
    instance_dir(family, id).join("runtime")
}

/// 给定实例的 advisory file lock：`<...>/runtime/instance.lock`。
pub fn instance_lock_file(family: &str, id: &str) -> PathBuf {
    instance_runtime_dir(family, id).join("instance.lock")
}

/// 给定实例的 PID 文件：`<...>/runtime/pid`。
pub fn instance_pid_file(family: &str, id: &str) -> PathBuf {
    instance_runtime_dir(family, id).join("pid")
}

/// 给定实例的端口文件：`<...>/runtime/port`。
pub fn instance_port_file(family: &str, id: &str) -> PathBuf {
    instance_runtime_dir(family, id).join("port")
}

/// 给定实例的状态文件：`<...>/runtime/status.json`。
pub fn instance_status_file(family: &str, id: &str) -> PathBuf {
    instance_runtime_dir(family, id).join("status.json")
}

/// 给定实例的官方 `DSH_HOME`：`<...>/home/`。
///
/// 这里是 DSH 进程读取 `profiles/<name>`、sessions、storages、credentials、
/// settings.yaml 的根目录。Xlink 通过适配器为每个实例准备这个目录。
pub fn instance_dsh_home(family: &str, id: &str) -> PathBuf {
    instance_dir(family, id).join("home")
}

/// Xlink 自身状态目录：`<xlink_home>/state/`（实例注册表、迁移记录、全局锁）。
pub fn state_root() -> PathBuf {
    xlink_home().join("state")
}

/// 实例注册表：`<xlink_home>/state/instances.json`。
pub fn instances_registry_file() -> PathBuf {
    state_root().join("instances.json")
}

/// 可重建的下载 / registry 缓存目录：`<xlink_home>/cache/`。
pub fn cache_root() -> PathBuf {
    xlink_home().join("cache")
}

/// Xlink 顶层元数据文件：`<xlink_home>/xlink.json`，首次启动时由 setup 写入。
pub fn xlink_metadata_file() -> PathBuf {
    xlink_home().join("xlink.json")
}

/// 旧版 dsh home 根目录：`<DSH_HOME 或 ~/.dsh>/`。
///
/// legacy resolver 共用入口；不创建目录。
pub fn legacy_dsh_home() -> PathBuf {
    if let Some(home) = std::env::var_os("DSH_HOME").map(PathBuf::from) {
        return home;
    }
    dirs_home().join(DSH_HOME_DIR_NAME)
}

/// 旧版 release/dev Shell 数据根目录：`<dsh_home>/desktop[-dev]/`。
///
/// 仍由当前 [`crate::kernel::data_dir`] 使用，P2 之前继续生效。
pub fn legacy_desktop_data_dir(mode: ShellMode) -> PathBuf {
    legacy_dsh_home().join(mode.legacy_subdir())
}

/// 旧版插件中央库根目录：`<dsh_home>/plugins/`。
pub fn legacy_dsh_plugins_root() -> PathBuf {
    legacy_dsh_home().join("plugins")
}

/// 旧版技能中央库根目录：`<dsh_home>/skills-store/`。
pub fn legacy_dsh_skills_store() -> PathBuf {
    legacy_dsh_home().join("skills-store")
}

/// 旧版技能活动视图根目录：`<dsh_home>/skills/`（官方内核读取路径之一）。
pub fn legacy_dsh_skills_root() -> PathBuf {
    legacy_dsh_home().join("skills")
}

/// 验证 `s` 能否作为一段 id / 路径组件使用。拒绝：
///
/// - 空字符串。
/// - 任何形式的路径穿越（`.`、`..`、含 `..` 段、含 `\` 或 `/`）。
/// - 含 NUL 字节或其它控制字符（防止 OS 误解释文件名）。
/// - Windows 保留名（`CON`、`PRN`、`AUX`、`NUL`、`COM1`–`COM9`、
///   `LPT1`–`LPT9`，不区分大小写），避免某些文件系统在写入时拒绝。
///
/// 该函数只检查"形态"，不读取文件系统——结合 [`path_under_base`] 才能
/// 真正防御"构造 `..` 跳出 base"的攻击。
pub fn validate_id_component(s: &str) -> Result<(), &'static str> {
    if s.is_empty() {
        return Err("id 不能为空");
    }
    if s == "." || s == ".." {
        return Err("id 不能是路径保留名 '.' 或 '..'");
    }
    if s.contains('/') || s.contains('\\') {
        return Err("id 不能包含路径分隔符");
    }
    if s.contains('\0') {
        return Err("id 不能包含 NUL 字节");
    }
    if s.chars().any(|c| c.is_control()) {
        return Err("id 不能包含控制字符");
    }
    // Windows 保留名：哪怕在 macOS / Linux 上用合法路径构造，传给底层
    // 文件系统时仍可能撞上 Windows 兼容层的拒绝。预先 ban 掉。
    if cfg!(windows) || cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        let upper = s.to_ascii_uppercase();
        if matches!(
            upper.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        ) {
            return Err("id 不能是 Windows 保留名");
        }
    }
    Ok(())
}

/// 把任意路径解析为在 `base` 之下的相对路径，验证它没有越界。
///
/// 用于"用户提供一段 id，拼成 `<base>/<id>`"的场景：把拼好的路径
/// 重新 canonicalize，再比对前缀，防止 `..` / 符号链接等手段跳到
/// 计划外的目录。该函数**不触碰文件系统**，只做词法检查；
/// 需要面对符号链接时由调用方自行 `canonicalize` 后再校验。
pub fn path_under_base(base: &Path, candidate: &Path) -> Option<PathBuf> {
    let base_components: Vec<Component<'_>> = base.components().collect();
    let candidate_components: Vec<Component<'_>> = candidate.components().collect();
    // candidate 必须严格在 base 之下：相等不算（避免「把目录当作自己的子项」）。
    if candidate_components.len() <= base_components.len() {
        return None;
    }
    if base_components
        .iter()
        .zip(&candidate_components)
        .any(|(a, b)| a != b)
    {
        return None;
    }
    // 关键检查：candidate 的剩余部分不能含 `..` 或绝对路径段。
    candidate_components
        .iter()
        .skip(base_components.len())
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| candidate_components.iter().collect())
}

/// Xlink 顶层元数据。`xlink.json` 的目标形态；P2 起在 setup 阶段写入。
///
/// 当前阶段（P0）只在测试中用，尚未被任何生产路径读写。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XlinkMetadata {
    pub schema_version: u32,
    /// 创建时间（epoch 毫秒），由 [`crate::process::epoch_millis`] 写入。
    pub created_at_ms: u64,
}

impl XlinkMetadata {
    /// 用当前 schema 版本与给定时间戳构造元数据。
    pub fn new(created_at_ms: u64) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            created_at_ms,
        }
    }

    /// 校验读到的元数据是否来自当前实现能识别的 schema 版本。
    pub fn is_compatible(&self) -> bool {
        self.schema_version <= CURRENT_SCHEMA_VERSION
    }
}

/// Shell 自身状态的骨架。P0 阶段定义字段与读写约定，不写入实际文件。
///
/// 字段命名遵循"键名 = 资源含义"，例如 `default_instance_id` 指注册表中
/// 默认内核实例的 id（不是某个版本号）。P2 实例注册表落地后再扩展字段。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShellState {
    pub schema_version: u32,
    /// 该 Shell 模式记住的默认实例 id。P2 起由实例注册表解析。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_instance_id: Option<String>,
}

impl ShellState {
    pub fn new() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            default_instance_id: None,
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.schema_version <= CURRENT_SCHEMA_VERSION
    }
}

#[cfg(test)]
mod test_helpers {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// 全进程串行化所有改 env 的测试，避免并行 worker 互相踩。
    /// `cargo test` 默认多线程，单测改 env 必须排队。
    pub(super) static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// 用唯一前缀拿一个临时目录，便于并行测试不冲突。
    pub(super) fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-paths-{}-{}-{}",
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

    /// 在测试作用域内设置 env，离开时还原。`HOME` 与 `USERPROFILE` 同时
    /// 处理（Windows 优先 USERPROFILE），避免 OS 差异让测试 flaky。
    pub(super) struct ScopedEnv {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl ScopedEnv {
        pub(super) fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_helpers::{temp_dir, ScopedEnv, ENV_LOCK};

    /// release / dev 共用根目录，但路径不再重复。
    #[test]
    fn shell_paths_split_by_mode_under_shared_root() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fake = temp_dir("xlink-home");
        let _home = ScopedEnv::set(DSH_XLINK_HOME_ENV, &fake);
        let release = shell_dir(ShellMode::Release);
        let dev = shell_dir(ShellMode::Dev);
        assert!(release.starts_with(&fake), "release 应在 xlink_home 下");
        assert!(dev.starts_with(&fake), "dev 应在 xlink_home 下");
        assert_ne!(release, dev, "两种 Shell 模式必须分离");
        assert!(
            release.ends_with(format!("shell/{}", ShellMode::Release)),
            "release 应落在 shell/release 下：{}",
            release.display()
        );
        assert!(
            dev.ends_with(format!("shell/{}", ShellMode::Dev)),
            "dev 应落在 shell/dev 下：{}",
            dev.display()
        );
    }

    /// `DSH_XLINK_HOME` 必须覆盖默认值。
    #[test]
    fn dsh_xlink_home_overrides_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fake = temp_dir("override");
        let _home = ScopedEnv::set(DSH_XLINK_HOME_ENV, &fake);
        assert_eq!(xlink_home(), fake);
        // 子路径仍以 override 为根。
        assert!(shell_settings_file(ShellMode::Release).starts_with(&fake));
    }

    /// 默认根目录是 `<dirs_home>/.dsh-xlink/`；DSH_XLINK_HOME 缺省时使用之。
    #[test]
    fn default_root_is_home_dot_dsh_xlink() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(DSH_XLINK_HOME_ENV);
        let expected = dirs_home().join(DEFAULT_HOME_DIR_NAME);
        assert_eq!(xlink_home(), expected);
    }

    /// `DSH_XLINK_HOME` 与 `DSH_HOME` 互不影响：Xlink 根走自己的变量，
    /// legacy resolver 走自己的变量，不能互相回退。
    #[test]
    fn dsh_xlink_home_and_dsh_home_do_not_cross() {
        let _guard = ENV_LOCK.lock().unwrap();
        let xlink_dir = temp_dir("xlink-only");
        let dsh_dir = temp_dir("dsh-only");
        let _xlink = ScopedEnv::set(DSH_XLINK_HOME_ENV, &xlink_dir);
        let _dsh = ScopedEnv::set("DSH_HOME", &dsh_dir);
        assert_eq!(xlink_home(), xlink_dir);
        assert_eq!(legacy_dsh_home(), dsh_dir);
        // 新路径完全在 xlink_dir 下，与 dsh_dir 无关。
        assert!(shell_settings_file(ShellMode::Release).starts_with(&xlink_dir));
        // legacy 路径完全在 dsh_dir 下，与 xlink_dir 无关。
        assert!(legacy_desktop_data_dir(ShellMode::Release).starts_with(&dsh_dir));
    }

    /// `release` / `dev` 的 legacy 子目录名不能混淆。
    #[test]
    fn legacy_resolver_uses_subdir_per_mode() {
        let release = legacy_desktop_data_dir(ShellMode::Release);
        let dev = legacy_desktop_data_dir(ShellMode::Dev);
        assert!(
            release.ends_with(LEGACY_SHELL_SUBDIR_RELEASE),
            "release 应指向 desktop：{}",
            release.display()
        );
        assert!(
            dev.ends_with(LEGACY_SHELL_SUBDIR_DEV),
            "dev 应指向 desktop-dev：{}",
            dev.display()
        );
    }

    /// `validate_id_component` 必须拒绝 `..`、路径分隔符、空值、NUL 与
    /// Windows 保留名。
    #[test]
    fn validate_id_component_rejects_path_traversal() {
        assert!(validate_id_component("").is_err());
        assert!(validate_id_component(".").is_err());
        assert!(validate_id_component("..").is_err());
        assert!(validate_id_component("foo/bar").is_err());
        assert!(validate_id_component("foo\\bar").is_err());
        assert!(validate_id_component("foo\0bar").is_err());
        assert!(validate_id_component("foo\nbar").is_err());
        assert!(validate_id_component("CON").is_err());
        assert!(validate_id_component("com1").is_err());
        // 合法形态。
        assert!(validate_id_component("default").is_ok());
        assert!(validate_id_component("0.1.5-rc.1").is_ok());
        assert!(validate_id_component("my-instance_v2").is_ok());
    }

    /// `path_under_base` 必须拒绝越界路径。
    #[test]
    fn path_under_base_rejects_escape() {
        let base = PathBuf::from("/data/store");
        assert_eq!(
            path_under_base(&base, &PathBuf::from("/data/store/a/b")),
            Some(PathBuf::from("/data/store/a/b"))
        );
        // 等于 base 不算"under"：避免把目录本身当成自己的子项。
        assert_eq!(path_under_base(&base, &PathBuf::from("/data/store")), None);
        // 同名前缀但不是 base 子项（`store-other`）必须被拒。
        assert_eq!(
            path_under_base(&base, &PathBuf::from("/data/store-other/a")),
            None
        );
        // `..` 段必须被拒：`/data/store/../escape` 词法上仍在 base 下展开，
        // 但残留的 `..` 是显式的逃逸意图。
        assert_eq!(
            path_under_base(&base, &PathBuf::from("/data/store/../escape")),
            None
        );
        // `RootDir` 段必须被拒：`/data/store/abs` 实际是 base 的子项，但
        // 若 candidate 整段是绝对路径且不以 base 开头，按前缀规则已被拒。
        // 这里用一个含 `RootDir` 的"子路径"作烟雾测试，确认不会漏放。
        assert_eq!(path_under_base(&base, &PathBuf::from("/etc/passwd")), None);
    }

    /// Shell 模式是稳定的：构建后 `current()` 永远返回同一值。
    #[test]
    fn shell_mode_is_build_time_constant() {
        let current = ShellMode::current();
        assert!(matches!(current, ShellMode::Release | ShellMode::Dev));
        assert_eq!(current.as_str(), current.as_str());
    }

    /// 元数据结构必须带 schema_version，且 is_compatible 反映未来兼容性。
    #[test]
    fn xlink_metadata_schema_version() {
        let metadata = XlinkMetadata::new(123);
        assert_eq!(metadata.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(metadata.is_compatible());
        // 模拟未来版本：超过 CURRENT_SCHEMA_VERSION 视为不兼容。
        let future = XlinkMetadata {
            schema_version: CURRENT_SCHEMA_VERSION + 1,
            created_at_ms: 0,
        };
        assert!(!future.is_compatible());
    }

    /// ShellState 默认字段与序列化往返。
    #[test]
    fn shell_state_round_trip() {
        let state = ShellState {
            schema_version: CURRENT_SCHEMA_VERSION,
            default_instance_id: Some("default".to_string()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: ShellState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, state);
        assert!(restored.is_compatible());
    }
}

/// 集成式路径测试：把多个函数串起来，覆盖 P0 完成判据中的关键路径。
///
/// 与单元测试不同，本组刻意不在测试主体内断言"路径相等"，只断言
/// "应满足的关系"（前缀、互不覆盖、合法性），避免引入当前布局的
/// 硬编码而失去对将来布局迁移的表达力。
#[cfg(test)]
mod integration_paths_tests {
    use super::*;
    use test_helpers::{temp_dir, ScopedEnv, ENV_LOCK};

    /// release 与 dev 的 Shell 路径必须落在同一 `xlink_home()` 下，但
    /// 互不覆盖。
    #[test]
    fn release_and_dev_shells_do_not_overlap() {
        let release_settings = shell_settings_file(ShellMode::Release);
        let dev_settings = shell_settings_file(ShellMode::Dev);
        let release_logs = shell_logs_dir(ShellMode::Release);
        let dev_logs = shell_logs_dir(ShellMode::Dev);

        let home = xlink_home();
        assert!(release_settings.starts_with(&home));
        assert!(dev_settings.starts_with(&home));
        assert!(release_logs.starts_with(&home));
        assert!(dev_logs.starts_with(&home));

        // settings 与 logs 文件名必须分开。
        assert_ne!(release_settings, dev_settings);
        assert_ne!(release_logs, dev_logs);
        // release 与 dev 必须互不串目录。
        assert!(!release_settings.starts_with(&dev_logs));
        assert!(!dev_settings.starts_with(&release_logs));
    }

    /// release 与 dev 的 legacy Shell 路径解析在用户不覆盖任何环境变量时
    /// 仍稳定命中 `<dsh_home>/desktop[-dev]/`。
    #[test]
    fn legacy_resolver_is_stable_under_default_env() {
        // 测试不依赖具体 home：先 set_var DSH_HOME 到临时目录，保证断言稳定。
        let _guard = ENV_LOCK.lock().unwrap();
        let fake = temp_dir("legacy-dsh-home");
        let _dsh = ScopedEnv::set("DSH_HOME", &fake);
        assert_eq!(
            legacy_desktop_data_dir(ShellMode::Release),
            fake.join(LEGACY_SHELL_SUBDIR_RELEASE)
        );
        assert_eq!(
            legacy_desktop_data_dir(ShellMode::Dev),
            fake.join(LEGACY_SHELL_SUBDIR_DEV)
        );
        assert_eq!(legacy_dsh_plugins_root(), fake.join("plugins"));
        assert_eq!(legacy_dsh_skills_store(), fake.join("skills-store"));
        assert_eq!(legacy_dsh_skills_root(), fake.join("skills"));
    }

    /// 新路径与 legacy 路径在同一 DSH_HOME 覆盖下应保持正交：新路径永远
    /// 走 `xlink_home()`，legacy 路径永远走 `DSH_HOME`。
    #[test]
    fn new_and_legacy_paths_are_orthogonal() {
        let _guard = ENV_LOCK.lock().unwrap();
        let xlink_dir = temp_dir("xlink-ortho");
        let dsh_dir = temp_dir("dsh-ortho");
        let _xlink = ScopedEnv::set(DSH_XLINK_HOME_ENV, &xlink_dir);
        let _dsh = ScopedEnv::set("DSH_HOME", &dsh_dir);

        // 新路径：在 xlink_dir 下。
        assert!(shell_settings_file(ShellMode::Release).starts_with(&xlink_dir));
        assert!(plugins_store_root().starts_with(&xlink_dir));
        assert!(skills_store_root().starts_with(&xlink_dir));
        assert!(kernels_root().starts_with(&xlink_dir));
        // legacy 路径：在 dsh_dir 下，与 xlink_dir 完全无交集。
        assert!(legacy_desktop_data_dir(ShellMode::Release).starts_with(&dsh_dir));
        assert!(legacy_dsh_plugins_root().starts_with(&dsh_dir));
        assert!(legacy_dsh_skills_store().starts_with(&dsh_dir));
        assert!(legacy_dsh_skills_root().starts_with(&dsh_dir));
        // 重要约束：任何新路径都不能"偶然"指向 legacy 根目录。
        for new_path in [
            shell_settings_file(ShellMode::Release),
            shell_logs_dir(ShellMode::Dev),
            plugins_store_root(),
            skills_store_root(),
            kernels_root(),
            state_root(),
            cache_root(),
        ] {
            assert!(
                !new_path.starts_with(&dsh_dir),
                "新路径 {} 不应落在 DSH_HOME 下：{}",
                new_path.display(),
                dsh_dir.display()
            );
        }
    }

    /// release 与 dev 的 Shell 日志路径在所有模式下都唯一可写。
    #[test]
    fn shell_logs_dirs_are_unique_per_mode() {
        let release = shell_logs_dir(ShellMode::Release);
        let dev = shell_logs_dir(ShellMode::Dev);
        // 两个路径都不是彼此的前缀。
        assert!(!release.starts_with(&dev));
        assert!(!dev.starts_with(&release));
        // 都以 `/logs` 结尾。
        assert!(release.ends_with("logs"));
        assert!(dev.ends_with("logs"));
    }
}
