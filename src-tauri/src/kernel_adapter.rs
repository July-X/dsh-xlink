//! 内核适配器接口与首份 DSH 实现（开发计划 §P3）。
//!
//! "适配器"是把 dsh-xlink 的通用实例生命周期映射到具体内核族（`dsh`、
//! 未来的 `mcode` 等）的模块。`kernel.rs` 只依赖适配器接口；DSH 专有的
//! 路径、profile 接线、DSH_HOME 注入与原生模块验证集中到 [`DshAdapter`]。
//!
//! ## 设计动机
//!
//! 改造前 `kernel.rs` 里有大量形如 `kernels/<version>/node_modules/...`、
//! `<data_dir>/profiles/<name>/`、`DSH_HOME` 隐式依赖 `$HOME` 的代码片段；
//! 这些路径与 `release/dev` 强耦合，且在多内核场景下会相互覆盖。适配器把
//! 这些知识集中到一处，未来 mcode / 自定义内核只需新增一个适配器实现，
//! 通用实例管理（注册表、端口、锁、生命周期）不变。
//!
//! ## 当前阶段（P3）的取舍
//!
//! - 安装目录 `kernels/<family>/versions/<version>/` **尚未迁移**：现存
//!   用户的内核二进制仍在 legacy `<dsh_home>/desktop[-dev]/kernels/<version>/`。
//!   P3 仅在适配器里提供 `resolve_install_dir` 函数，按"新位置优先、
//!   legacy 路径兜底"的双查找方式定位内核入口。完全迁移是 P3 第二步。
//! - `DSH_HOME`、`profile`、`workspace` 三个环境变量已经在 P3 切到实例
//!   目录：每个实例的内核进程拿到的是 `kernels/<family>/instances/<id>/home/`，
//!   互不串 session / storage / credentials。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};

use crate::instance::{InstanceRecord, KERNEL_FAMILY_DSH, KERNEL_FAMILY_MCODE};
use crate::paths;

/// PATH 类环境变量在不同平台的分隔符。Windows 上是 `;`，其他平台是 `:`。
#[cfg(windows)]
const ENV_PATH_SEP: &str = ";";
#[cfg(not(windows))]
const ENV_PATH_SEP: &str = ":";

/// 适配器声明的具体能力位。
///
/// 不是所有内核族都支持同一组操作；命令层与 UI 据此决定按钮可见性。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdapterCapability {
    /// 能从 npm registry / GitHub Releases 安装新版本。
    InstallVersion,
    /// 启动时会创建 profile `package.json` 并把插件接到 profile dep。
    /// 不支持的内核只把中央库当只读源。
    ProfileWiring,
    /// 接受 `DSH_HOME` / `customSkillDirs` 等 DSH 专有约定。
    DshHomeEnv,
    /// 启动后支持热重载插件（DSH 不支持，mcode 也许支持）。
    HotReload,
}

impl AdapterCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            AdapterCapability::InstallVersion => "install-version",
            AdapterCapability::ProfileWiring => "profile-wiring",
            AdapterCapability::DshHomeEnv => "dsh-home-env",
            AdapterCapability::HotReload => "hot-reload",
        }
    }
}

/// 适配器能力集合，UI / 命令层据此渲染提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterCapabilities {
    flags: u32,
}

impl AdapterCapabilities {
    pub const NONE: AdapterCapabilities = AdapterCapabilities { flags: 0 };

    pub fn from_iter<I: IntoIterator<Item = AdapterCapability>>(iter: I) -> Self {
        let mut flags = 0u32;
        for cap in iter {
            flags |= 1 << (cap as u32);
        }
        Self { flags }
    }

    pub fn contains(self, cap: AdapterCapability) -> bool {
        (self.flags & (1 << (cap as u32))) != 0
    }

    pub fn list(self) -> Vec<AdapterCapability> {
        [
            AdapterCapability::InstallVersion,
            AdapterCapability::ProfileWiring,
            AdapterCapability::DshHomeEnv,
            AdapterCapability::HotReload,
        ]
        .into_iter()
        .filter(|cap| self.contains(*cap))
        .collect()
    }
}

/// 内核族。字符串形式用于 `kernels/<family>/` 下的目录归属。
pub type KernelFamily = str;

/// 适配器错误族。每个变体附带可操作的修复建议——调用方负责把
/// `AppError` 翻译成 UI 文案。
#[derive(Debug, Clone)]
pub enum AdapterError {
    /// 实例记录指向的版本未在 versions 目录里，也不在 legacy data_dir。
    VersionNotInstalled { family: String, version: String },
    /// profile 名字非法（空、含路径分隔符、Windows 保留名）。
    InvalidProfile(String),
    /// 启动前的健康检查发现端口已被无关进程占用。
    PortBusy { port: u16, owner_pid: Option<u32> },
    /// 缺少原生模块（`Cannot find module *.node`），写入失败前的可操作提示。
    NativeModuleMissing { package: String, relative: String },
    /// 通用 I/O 错误。
    Io(String),
    /// pnpm 启动失败（带 stderr 摘要）。
    Pnpm(String),
    /// 适配器声明「不支持」，把控制权交回调用方走默认行为。
    Unsupported(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::VersionNotInstalled { family, version } => {
                write!(f, "内核族 {family} 的版本 {version} 未安装或安装不完整")
            }
            AdapterError::InvalidProfile(name) => {
                write!(f, "profile 名非法：{name:?}")
            }
            AdapterError::PortBusy { port, owner_pid } => match owner_pid {
                Some(pid) => write!(f, "端口 {port} 已被无关进程占用（pid {pid}）"),
                None => write!(f, "端口 {port} 已被无关进程占用"),
            },
            AdapterError::NativeModuleMissing { package, relative } => write!(
                f,
                "内核依赖的原生模块未构建完成：{package}（缺 {relative}）"
            ),
            AdapterError::Io(msg) => write!(f, "I/O 错误：{msg}"),
            AdapterError::Pnpm(msg) => write!(f, "pnpm 启动失败：{msg}"),
            AdapterError::Unsupported(msg) => write!(f, "适配器不支持该操作：{msg}"),
        }
    }
}

/// 内核族 → 适配器的中央注册表。当前只有 `dsh`，未来 `mcode` 在此处追加。
///
/// 选用 `Vec<Box<dyn KernelAdapter>>` 而非 `match` 是为了让后续
/// `mcode` 适配器能作为独立 crate 引入；运行时多态省一层硬编码分支。
/// （`HashSet` 要求 trait 实现 `Hash + Eq`，与 `dyn` trait 对象不兼容，
/// 所以这里用 `Vec` + 线性扫描；适配器数量天然很少，开销可忽略。）
pub fn adapters() -> Vec<Box<dyn KernelAdapter>> {
    vec![
        Box::new(DshAdapter::default()),
        Box::new(McodeAdapter::new()),
    ]
}

/// 按 `family` 字符串查询适配器。找不到时返回 `None`，调用方应使用
/// `Unsupported` 错误把 UI 引导到"内核族不支持"分支。
pub fn lookup(family: &str) -> Option<Box<dyn KernelAdapter>> {
    adapters().into_iter().find(|a| a.family() == family)
}

/// 通用内核适配器接口。
///
/// `Send + Sync` 是为了能让适配器存进全局注册表 / Tauri state。
pub trait KernelAdapter: Send + Sync {
    /// 内核族标识：对应 `kernels/<family>/` 下的目录归属。
    fn family(&self) -> &'static KernelFamily;

    /// 声明能力位集合。
    fn capabilities(&self) -> AdapterCapabilities;

    /// 描述适配器的人类可读名字（UI 上展示）。
    fn display_name(&self) -> &'static str;

    /// 定位版本安装根目录：`kernels/<family>/versions/<version>/`。
    ///
    /// P3 阶段新增的位置会优先；legacy `<dsh_home>/desktop[-dev]/kernels/<version>/`
    /// 作为兜底存在，以便现存用户平滑过渡。返回 `None` 表示新旧位置都
    /// 找不到，调用方应报 `VersionNotInstalled`。
    fn resolve_install_dir(&self, version: &str) -> Option<PathBuf>;

    /// 验证 `version` 字符串是否符合规范。DSH 默认是 semver，可由适配器
    /// 自定义更严格的版本形态。
    fn validate_version(&self, version: &str) -> Result<(), String>;

    /// 准备实例目录：按内核族惯例创建 `home / profile / sessions / ...` 子目录，
    /// 并把 profile 模板与 `cordis.patch.yml`、`pnpm-workspace.yaml` 落盘。
    fn prepare_instance(&self, record: &InstanceRecord) -> Result<(), AdapterError>;

    /// 自定义技能目录列表。默认空——内核族不主动接入额外技能源。
    ///
    /// 返回的路径会被 `start` 注入到内核进程（具体 env 名 / 注入格式
    /// 由各 adapter 在 `start` 内部约定；DSH 端的接入方式需要 DSH
    /// 自身支持）。即使内核端**暂未消费**该注入，把路径放在这里也是
    /// 接口完整化的一部分——DSH 端升级后不需要再改 Xlink 代码。
    fn custom_skill_dirs(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// 启动实例。`install_root` 是 `resolve_install_dir` 的结果；`node`
    /// 是 `node` 可执行文件路径。
    ///
    /// 适配器必须设置 `DSH_HOME` 等环境变量，并把进程的 `cwd` 设到
    /// 实例 workspace。
    fn start(
        &self,
        record: &InstanceRecord,
        install_root: &Path,
        node: &Path,
    ) -> Result<std::process::Child, AdapterError>;
}

// ─── DSH 适配器 ──────────────────────────────────────────────────────────

/// DeepSeek Harness 的具体实现。
///
/// P3 阶段负责：
/// - `DSH_HOME` = `kernels/dsh/instances/<id>/home/`
/// - profile = `<DSH_HOME>/profiles/<name>/`
/// - workspace = `kernels/dsh/instances/<id>/workspace/`
/// - 内核入口 = `kernels/dsh/versions/<version>/node_modules/@deepseek-ai/dsh/lib/bin.js`
///   （legacy：`<data_dir>/kernels/<version>/node_modules/@deepseek-ai/dsh/lib/bin.js`）
#[derive(Debug, Default)]
pub struct DshAdapter;

impl DshAdapter {
    pub const KERNEL_BIN_REL: &'static str = "node_modules/@deepseek-ai/dsh/lib/bin.js";

    /// 给定实例返回 DSH 进程要使用的 `DSH_HOME`。
    pub fn dsh_home_for(record: &InstanceRecord) -> PathBuf {
        paths::instance_dsh_home(record.kernel_family.as_str(), record.id.as_str())
    }
}

impl KernelAdapter for DshAdapter {
    fn family(&self) -> &'static KernelFamily {
        KERNEL_FAMILY_DSH
    }

    fn capabilities(&self) -> AdapterCapabilities {
        // P3：DSH 适配器支持安装版本、profile 接线与 DSH_HOME env；
        // 不支持热重载（DSH 设计上要求重启实例）。
        AdapterCapabilities::from_iter([
            AdapterCapability::InstallVersion,
            AdapterCapability::ProfileWiring,
            AdapterCapability::DshHomeEnv,
        ])
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn resolve_install_dir(&self, version: &str) -> Option<PathBuf> {
        if let Err(_) = paths::validate_id_component(version) {
            return None;
        }
        let new_path = paths::kernel_version_dir(KERNEL_FAMILY_DSH, version);
        if new_path.join(Self::KERNEL_BIN_REL).is_file() {
            return Some(new_path);
        }
        // 兜底：legacy `<dsh_home>/desktop[-dev]/kernels/<version>/`。
        // 实际位置由调用方通过 [`crate::kernel::data_dir`] 取到，再
        // 与 version 拼接。这里先打桩：让 caller 自己 fallback。
        None
    }

    fn validate_version(&self, version: &str) -> Result<(), String> {
        // DSH 用 semver（含可选 `-rc.N` 后缀）。允许空字符串与额外
        // 标识符——官方版本号会随时间演进，过于严格的形态校验会在
        // 升级到新命名规则时拒绝合法版本。
        if version.is_empty() {
            return Err("版本号不能为空".into());
        }
        Ok(())
    }

    fn custom_skill_dirs(&self) -> Vec<PathBuf> {
        // P5 起 Xlink 维护一份「共享技能活动视图」（设计稿 §9.1）——
        // `<xlink_home>/skills/active/`。DSH 端目前尚未官方支持通过 env
        // 注入额外 skill 目录，但保留这条接口让 DSH 升级时不需要再改
        // Xlink 侧；`start` 会主动把它写进 `DSH_CUSTOM_SKILL_DIRS` env
        // 供未来版本的 DSH 消费。
        vec![paths::skills_active_root()]
    }

    fn prepare_instance(&self, record: &InstanceRecord) -> Result<(), AdapterError> {
        paths::validate_id_component(&record.id).map_err(|e| AdapterError::Io(e.to_string()))?;
        paths::validate_id_component(&record.kernel_family)
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        paths::validate_id_component(&record.profile)
            .map_err(|e| AdapterError::InvalidProfile(record.profile.clone()))?;
        crate::instance::ensure_instance_dirs(record)
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        // DSH 实例的 profile / pnpm-workspace / cordis.patch 模板。
        // 这里只落空文件占位；P4 插件接线时再写入 bundle 列表。
        let home = Self::dsh_home_for(record);
        let profile = home.join("profiles").join(&record.profile);
        // ensure_instance_dirs 只准备 `home/profiles/` 父目录；profile
        // 子目录由 DshAdapter 显式创建。
        std::fs::create_dir_all(&profile).map_err(|e| AdapterError::Io(e.to_string()))?;
        let package_json = profile.join("package.json");
        if !package_json.exists() {
            let stub = serde_json::json!({
                "name": format!("dsh-xlink-instance-{}", record.id),
                "private": true,
                "version": "0.0.0",
                "schema_version": 1,
                "kernel_family": record.kernel_family,
            });
            std::fs::write(
                &package_json,
                format!("{}\n", serde_json::to_string_pretty(&stub).unwrap()),
            )
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        }
        let workspace_yml = profile.join("pnpm-workspace.yaml");
        if !workspace_yml.exists() {
            std::fs::write(&workspace_yml, "packages:\n  - .\n")
                .map_err(|e| AdapterError::Io(e.to_string()))?;
        }
        let patch_yml = home.join("cordis.patch.yml");
        if !patch_yml.exists() {
            std::fs::write(
                &patch_yml,
                "# dsh-xlink managed cordis patch\nschema_version: 1\n",
            )
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn start(
        &self,
        record: &InstanceRecord,
        install_root: &Path,
        node: &Path,
    ) -> Result<std::process::Child, AdapterError> {
        let bin = install_root.join(Self::KERNEL_BIN_REL);
        if !bin.is_file() {
            return Err(AdapterError::VersionNotInstalled {
                family: record.kernel_family.clone(),
                version: record.kernel_version.clone().unwrap_or_default(),
            });
        }
        let dsh_home = Self::dsh_home_for(record);
        if !dsh_home.join("profiles").join(&record.profile).is_dir() {
            return Err(AdapterError::InvalidProfile(format!(
                "实例 {} 的 profile {} 目录尚未准备好",
                record.id, record.profile
            )));
        }
        let node_dir = node.parent().unwrap_or_else(|| Path::new("."));
        let mut cmd = crate::process::command_with_path_dirs(node, &[node_dir]);
        let port_arg = record.port.to_string();
        // `current_dir` 设到实例 workspace —— DSH 启动后写入的 cwd 派
        // 生路径会从这里展开。`record.workspace` 默认指向
        // `kernels/<family>/instances/<id>/workspace`，允许用户改成
        // 外部目录。
        let workspace = PathBuf::from(&record.workspace);
        std::fs::create_dir_all(&workspace).map_err(|e| AdapterError::Io(e.to_string()))?;
        cmd.arg(&bin)
            .arg("web")
            .arg("--no-open")
            .arg("--port")
            .arg(&port_arg)
            .current_dir(&workspace)
            .env("DSH_HOME", &dsh_home)
            // DSH 也接受 `DSH_PROFILE`，但官方默认 profile 仍走
            // `$DSH_HOME/profiles/<name>/`，此处显式声明以防命令覆盖。
            .env("DSH_PROFILE", &record.profile);
        // P5：把 [`custom_skill_dirs`] 通过 `DSH_CUSTOM_SKILL_DIRS` 注入
        // ——以 PATH 风格冒号分隔。DSH 端目前尚未官方支持 env 注入技能
        // 目录，这条 env 是**接口预留**；DSH 端升级后（自定义 skill dir
        // 协议稳定）不需要再改 Xlink 这边。空列表时**不**写 env，避免
        // 把空字符串污染到 DSH 端。
        let skill_dirs = self.custom_skill_dirs();
        if !skill_dirs.is_empty() {
            let joined = skill_dirs
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(ENV_PATH_SEP);
            cmd.env("DSH_CUSTOM_SKILL_DIRS", joined);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 让 kill -pid 能回收整个进程组。
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let mut child = crate::process::quiet(&mut cmd)
            .spawn()
            .map_err(|e| AdapterError::Io(format!("无法启动内核：{e}")))?;
        crate::process::adopt_kernel_process(&child);
        if let Err(error) = crate::process::attach_log_drainers(
            &mut child,
            &paths::shell_logs_dir(crate::paths::ShellMode::current()),
            &crate::kernel::kernel_log_spec(&record.kernel_family, &record.id),
        ) {
            crate::process::terminate_process_tree(&mut child);
            return Err(AdapterError::Io(format!("无法接管内核日志：{error}")));
        }
        Ok(child)
    }
}

// ─── mcode 适配器（mock / dry-run）────────────────────────────────────────

/// mcode 内核族的**mock**实现——按开发计划 §P7「先实现 mock 或 dry-run
/// adapter，是否接入真实 mcode CLI 由独立需求决定」。
///
/// 当前**不接入**真实 mcode CLI：所有物化 / 启动操作都返回
/// [`AdapterError::VersionNotInstalled`]（mock 不真支持）。能力位全部不声明，
/// 命令层据此不渲染「安装 / 同步 / 接线」等按钮——只是让 Xlink 的通用
/// 实例管理代码（注册表 / 端口分配 / 锁 / 日志归属）能在两种内核并存时
/// 仍然走同一条路径，证明 `KernelAdapter` trait 真的能容纳第二种内核。
///
/// 真实接入时只需替换 [`McodeAdapter::start`] / [`McodeAdapter::prepare_instance`]
/// 等方法实现，**不**需要改 [`adapters`] 注册表与 [`KernelAdapter`] trait。
pub struct McodeAdapter;

impl McodeAdapter {
    pub const fn new() -> Self {
        McodeAdapter
    }
}

impl Default for McodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelAdapter for McodeAdapter {
    fn family(&self) -> &'static KernelFamily {
        // `KernelFamily = str`（P2 引入的类型别名）—— `&'static str`
        // 直接满足 trait 方法签名，无需转换。
        KERNEL_FAMILY_MCODE
    }

    fn capabilities(&self) -> AdapterCapabilities {
        // mock 不声明任何能力——所有物化 / 接线 / 安装按钮都不渲染。
        AdapterCapabilities::NONE
    }

    fn display_name(&self) -> &'static str {
        "Mcode (mock)"
    }

    fn resolve_install_dir(&self, _version: &str) -> Option<PathBuf> {
        // mock 不解析安装目录——调用方拿到 `None` 会走 legacy / 兜底，
        // 真实 mcode 接入时这里返回 `Some(path)` 即可。
        None
    }

    fn validate_version(&self, _version: &str) -> Result<(), String> {
        Ok(())
    }

    fn prepare_instance(&self, record: &InstanceRecord) -> Result<(), AdapterError> {
        // mock 不真准备实例目录——返回 VersionNotInstalled 让调用方
        // 弹「尚未支持」提示。真实 mcode 接入时在这里创建 mcode 专有
        // 目录布局即可。
        Err(AdapterError::VersionNotInstalled {
            family: record.kernel_family.clone(),
            version: record.kernel_version.clone().unwrap_or_default(),
        })
    }

    fn start(
        &self,
        record: &InstanceRecord,
        _install_root: &Path,
        _node: &Path,
    ) -> Result<std::process::Child, AdapterError> {
        // mock 不启动进程。真实 mcode 接入时在这里 spawn `mcode web` 即可。
        Err(AdapterError::VersionNotInstalled {
            family: record.kernel_family.clone(),
            version: record.kernel_version.clone().unwrap_or_default(),
        })
    }
}

// ─── 兼容期工具函数 ──────────────────────────────────────────────────────

/// 给定实例 + 适配器，按"新位置优先 / legacy 兜底"解析内核入口。
///
/// 返回 `Ok(install_root)` 表示找到了 `node_modules/@deepseek-ai/dsh/lib/bin.js`；
/// 返回 `Err(AdapterError::VersionNotInstalled)` 表示两处都没有。
pub fn resolve_install_root(
    record: &InstanceRecord,
    legacy_install_root: &Path,
) -> Result<PathBuf, AdapterError> {
    if let Some(adapter) = lookup(record.kernel_family.as_str()) {
        if let Some(version) = record.kernel_version.as_deref() {
            if let Some(new_path) = adapter.resolve_install_dir(version) {
                return Ok(new_path);
            }
        }
        let _ = adapter; // 不使用，避免未使用告警
    }
    // Legacy 兜底：调用方传入的 `legacy_install_root` 仍可能是
    // `<dsh_home>/desktop[-dev]/`（dev/release 共享）。
    let bin = legacy_install_root
        .join("kernels")
        .join(record.kernel_version.as_deref().unwrap_or(""));
    if bin.join(DshAdapter::KERNEL_BIN_REL).is_file() {
        return Ok(bin);
    }
    Err(AdapterError::VersionNotInstalled {
        family: record.kernel_family.clone(),
        version: record.kernel_version.clone().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::InstanceRecord;
    use crate::paths;
    use crate::tests::scoped_xlink_home;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-adapter-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_record() -> InstanceRecord {
        InstanceRecord::new("default", KERNEL_FAMILY_DSH, 3090, 1700000000000)
    }

    /// AdapterCapabilities 必须正确编码与解码。
    #[test]
    fn adapter_capabilities_round_trip() {
        let caps = AdapterCapabilities::from_iter([
            AdapterCapability::InstallVersion,
            AdapterCapability::ProfileWiring,
        ]);
        assert!(caps.contains(AdapterCapability::InstallVersion));
        assert!(caps.contains(AdapterCapability::ProfileWiring));
        assert!(!caps.contains(AdapterCapability::HotReload));
        assert_eq!(
            caps.list(),
            vec![
                AdapterCapability::InstallVersion,
                AdapterCapability::ProfileWiring,
            ]
        );
    }

    /// DshAdapter 声明能力位正确（Install + Wiring + DshHomeEnv，但无 HotReload）。
    #[test]
    fn dsh_adapter_declares_expected_capabilities() {
        let adapter = DshAdapter;
        let caps = adapter.capabilities();
        assert!(caps.contains(AdapterCapability::InstallVersion));
        assert!(caps.contains(AdapterCapability::ProfileWiring));
        assert!(caps.contains(AdapterCapability::DshHomeEnv));
        assert!(!caps.contains(AdapterCapability::HotReload));
    }

    /// lookup("dsh") 必须返回 DshAdapter；非空 family 都能找到（当前
    /// 只有 dsh）。
    #[test]
    fn lookup_finds_dsh() {
        let adapter = lookup(KERNEL_FAMILY_DSH).expect("dsh adapter");
        assert_eq!(adapter.family(), KERNEL_FAMILY_DSH);
        // 未知 family 返回 None。
        assert!(lookup("does-not-exist").is_none());
    }

    /// validate_version 必须拒绝空串，接受合法版本号。
    #[test]
    fn validate_version_rejects_empty() {
        let adapter = DshAdapter;
        assert!(adapter.validate_version("").is_err());
        assert!(adapter.validate_version("0.1.5-rc.1").is_ok());
        assert!(adapter.validate_version("v1.2.3").is_ok());
    }

    /// `resolve_install_dir` 在版本非法或目录不存在时返回 None。
    #[test]
    fn resolve_install_dir_returns_none_when_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("resolve-none");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        // 新位置 `kernels/dsh/versions/0.1.5/...` 不存在；返回 None。
        assert!(adapter.resolve_install_dir("0.1.5").is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    /// `prepare_instance` 必须创建 profile 模板与 cordis.patch.yml 占位。
    #[test]
    fn prepare_instance_creates_dsh_home_layout() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("prepare");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let mut record = sample_record();
        record.kernel_version = Some("0.1.5-rc.1".into());
        // 先准备目录再 prepare。
        crate::instance::ensure_instance_dirs(&record).expect("ensure dirs");
        adapter.prepare_instance(&record).expect("prepare");

        let dsh_home = DshAdapter::dsh_home_for(&record);
        let profile_pkg = dsh_home
            .join("profiles")
            .join(&record.profile)
            .join("package.json");
        let workspace_yml = dsh_home
            .join("profiles")
            .join(&record.profile)
            .join("pnpm-workspace.yaml");
        let patch_yml = dsh_home.join("cordis.patch.yml");
        assert!(
            profile_pkg.is_file(),
            "缺 profile package.json：{}",
            profile_pkg.display()
        );
        assert!(
            workspace_yml.is_file(),
            "缺 pnpm-workspace.yaml：{}",
            workspace_yml.display()
        );
        assert!(
            patch_yml.is_file(),
            "缺 cordis.patch.yml：{}",
            patch_yml.display()
        );
        // package.json 内容含 schema_version 与 kernel_family。
        let text = std::fs::read_to_string(&profile_pkg).unwrap();
        assert!(text.contains("kernel_family"));
        assert!(text.contains("schema_version"));
        std::fs::remove_dir_all(&home).ok();
    }

    /// `resolve_install_root` 在 legacy 路径命中时返回 legacy 目录。
    #[test]
    fn resolve_install_root_falls_back_to_legacy() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("legacy");
        let _xlink = scoped_xlink_home(&home);
        let mut record = sample_record();
        record.kernel_version = Some("0.1.5-rc.1".into());

        // legacy_root/kernels/0.1.5-rc.1/node_modules/@deepseek-ai/dsh/lib/bin.js
        let legacy_root = temp_dir("legacy-root");
        let bin = legacy_root
            .join("kernels")
            .join("0.1.5-rc.1")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, b"// stub").unwrap();

        let resolved =
            resolve_install_root(&record, &legacy_root).expect("legacy fallback should hit");
        assert_eq!(resolved, legacy_root.join("kernels").join("0.1.5-rc.1"));
        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&legacy_root).ok();
    }

    /// `resolve_install_root` 在两处都没有时返回 VersionNotInstalled。
    #[test]
    fn resolve_install_root_errors_when_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("missing");
        let _xlink = scoped_xlink_home(&home);
        let mut record = sample_record();
        record.kernel_version = Some("0.1.5-rc.1".into());
        let empty = temp_dir("empty-legacy");
        let err = resolve_install_root(&record, &empty).expect_err("should miss");
        match err {
            AdapterError::VersionNotInstalled { family, version } => {
                assert_eq!(family, KERNEL_FAMILY_DSH);
                assert_eq!(version, "0.1.5-rc.1");
            }
            other => panic!("unexpected: {other:?}"),
        }
        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    /// start 必须先校验 bin 与 profile 目录，缺失时报相应错误而不是默默 spawn。
    #[test]
    fn start_rejects_missing_binary() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("start-missing");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let mut record = sample_record();
        record.kernel_version = Some("0.1.5-rc.1".into());
        let empty = temp_dir("start-empty");
        let err = adapter
            .start(&record, &empty, Path::new("/nonexistent/node"))
            .expect_err("应拒绝");
        match err {
            AdapterError::VersionNotInstalled { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn custom_skill_dirs_default_returns_empty_for_minimal_adapter() {
        // 最小适配器不主动注入技能目录；DshAdapter 覆盖该方法返回非空。
        struct MinimalAdapter;
        impl KernelAdapter for MinimalAdapter {
            fn family(&self) -> &'static KernelFamily {
                KERNEL_FAMILY_DSH
            }
            fn capabilities(&self) -> AdapterCapabilities {
                AdapterCapabilities::NONE
            }
            fn display_name(&self) -> &'static str {
                "minimal"
            }
            fn resolve_install_dir(&self, _version: &str) -> Option<PathBuf> {
                None
            }
            fn validate_version(&self, _version: &str) -> Result<(), String> {
                Ok(())
            }
            fn prepare_instance(&self, _record: &InstanceRecord) -> Result<(), AdapterError> {
                Ok(())
            }
            fn start(
                &self,
                _record: &InstanceRecord,
                _install_root: &Path,
                _node: &Path,
            ) -> Result<std::process::Child, AdapterError> {
                Err(AdapterError::Io("unused".into()))
            }
        }
        assert!(MinimalAdapter.custom_skill_dirs().is_empty());
    }

    #[test]
    fn dsh_adapter_custom_skill_dirs_returns_skills_active_root() {
        // DshAdapter 必须报告 Xlink 的共享技能活动视图——v1 全局共享。
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("dsh-skill-dirs");
        let _xlink = scoped_xlink_home(&home);
        let dirs = DshAdapter.custom_skill_dirs();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], paths::skills_active_root());
        std::fs::remove_dir_all(&home).ok();
    }

    // --- P7：mcode mock 适配器测试 ---------------------------------------

    #[test]
    fn adapters_returns_both_dsh_and_mcode() {
        // 注册表必须同时返回 dsh 与 mcode 适配器——证明通用实例模型能
        // 容纳第二种内核（开发计划 §P7 测试要求）。按 family 排序便于
        // 测试断言稳定。
        let mut adapters = adapters();
        adapters.sort_by(|a, b| a.family().cmp(b.family()));
        let families: Vec<_> = adapters.iter().map(|a| a.family()).collect();
        assert_eq!(families, vec!["dsh", "mcode"]);
    }

    #[test]
    fn lookup_finds_mcode() {
        let adapter = lookup("mcode").expect("mcode adapter must be registered");
        assert_eq!(adapter.family(), "mcode");
        assert_eq!(adapter.display_name(), "Mcode (mock)");
    }

    #[test]
    fn mcode_adapter_declares_no_capabilities() {
        // mock 不支持任何能力——命令层据此不渲染「安装 / 同步 / 接线」按钮。
        let caps = McodeAdapter.capabilities();
        assert!(caps.list().is_empty(), "mock 不应声明任何能力位");
    }

    #[test]
    fn mcode_adapter_resolve_install_dir_returns_none() {
        // mock 不解析真实安装目录——调用方拿到 None 会走 legacy / 兜底。
        assert!(McodeAdapter.resolve_install_dir("0.1.0").is_none());
    }

    #[test]
    fn mcode_adapter_prepare_instance_rejects_as_unsupported() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("mcode-prepare");
        let _xlink = scoped_xlink_home(&home);
        let mut record = sample_record();
        record.kernel_family = "mcode".into();
        record.kernel_version = Some("0.1.0".into());
        let err = McodeAdapter
            .prepare_instance(&record)
            .expect_err("mock 不应真准备");
        match err {
            AdapterError::VersionNotInstalled { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn mcode_adapter_start_rejects_as_unsupported() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = temp_dir("mcode-start");
        let _xlink = scoped_xlink_home(&home);
        let mut record = sample_record();
        record.kernel_family = "mcode".into();
        record.kernel_version = Some("0.1.0".into());
        let empty = temp_dir("mcode-start-empty");
        let err = McodeAdapter
            .start(&record, &empty, Path::new("/nonexistent/node"))
            .expect_err("mock 不应真启动");
        match err {
            AdapterError::VersionNotInstalled { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn mcode_adapter_custom_skill_dirs_returns_empty() {
        // mock 不主动接入技能目录。
        assert!(McodeAdapter.custom_skill_dirs().is_empty());
    }

    #[test]
    fn dsh_and_mcode_have_distinct_family_strings() {
        // 防止未来有人把两个 family 写成同名（同名字符串会让 lookup
        // 走到错误的 adapter 上）。
        assert_ne!(McodeAdapter.family(), DshAdapter.family());
    }
}
