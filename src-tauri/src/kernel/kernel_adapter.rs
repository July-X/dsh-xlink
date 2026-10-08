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

use crate::kernel::profile_manifest;
use crate::shell::instance::{InstanceRecord, KERNEL_FAMILY_DSH, KERNEL_FAMILY_MCODE};
use crate::shell::paths;

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
    vec![Box::new(DshAdapter), Box::new(McodeAdapter::new())]
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
    /// 是 `node` 可执行文件路径；`logs_dir` 是内核 stdout/stderr 的落盘
    /// 目录（`<data_dir>/logs`，日志面板只扫这里）。
    ///
    /// 适配器必须设置 `DSH_HOME` 等环境变量，并把进程的 `cwd` 设到
    /// 实例 workspace。
    fn start(
        &self,
        record: &InstanceRecord,
        install_root: &Path,
        node: &Path,
        logs_dir: &Path,
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
        if paths::validate_id_component(version).is_err() {
            return None;
        }
        let new_path = paths::kernel_version_dir(KERNEL_FAMILY_DSH, version);
        if new_path.join(Self::KERNEL_BIN_REL).is_file() {
            return Some(new_path);
        }
        // 兜底：legacy `<dsh_home>/desktop[-dev]/kernels/<version>/`。
        // 实际位置由调用方通过 [`crate::kernel::lifecycle::data_dir`] 取到，再
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
        // 壳管理的技能活动视图（设计稿 §9.1）——`<xlink_home>/skills/active/`。
        // 2026-09-30 实测：已装内核 0.2.0-rc.2 **不读** `DSH_CUSTOM_SKILL_DIRS`
        // （全树 3481 个 js/d.ts 无一处 `process.env.DSH_CUSTOM_SKILL_DIRS`），
        // 它只从 `cordis.patch.yml` 的插件配置读 `customSkillDirs`——所以真正
        // 接通内核的是 [`ensure_skill_wiring`] 写的那一行 loader 配置。
        // 这条 env 仍然留着：内核一旦补上回退（对齐 `agentsHome` 的既有写法），
        // 不必改壳就能生效，而在那之前它无害。
        vec![paths::skills_active_root()]
    }

    fn prepare_instance(&self, record: &InstanceRecord) -> Result<(), AdapterError> {
        paths::validate_id_component(&record.id).map_err(|e| AdapterError::Io(e.to_string()))?;
        paths::validate_id_component(&record.kernel_family)
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        paths::validate_id_component(&record.profile)
            .map_err(|_e| AdapterError::InvalidProfile(record.profile.clone()))?;
        crate::shell::instance::ensure_instance_dirs(record)
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        // profile 目录的初值由 `profile_manifest::seed` 一家写：清单的**形状**
        // 是内核的启动契约（缺 `dsh.profile.bundles` 时内核解析出零个插件就正常
        // 退出，见该模块文档的实测矩阵）。这里此前落的是一个不带 bundle 的 stub，
        // 于是每一次新建实例——包括每一次预检沙盒——都起不来内核。
        let home = Self::dsh_home_for(record);
        profile_manifest::seed(
            &home.join("profiles").join(&record.profile),
            &record.profile,
        )
        .map_err(|e| AdapterError::Io(format!("无法准备实例 profile 目录：{e}")))?;
        // dsh-app-boot 要求 `cordis.patch.yml`（可缺席）是**顶层 YAML 数组**
        // （loader patch 条目列表）。P3 起的占位模板曾写成 `schema_version: 1`
        // 映射——内核从不读默认 `~/.dsh` 下不存在的同名文件所以多年无恙，而
        // e0cb7e6 把启动切到实例 DSH_HOME 后内核首次真正读到它，启动即
        // `fatal uncaught exception`（exit status 1），且安全模式不触及该文件、
        // 重试永远复现。这里除首次写入正确模板外，还把历史构建写出的坏模板
        // **字节级匹配**后原位改写；用户或内核写入的任何其他内容不动。
        const PATCH_YML_BROKEN: &str = "# dsh-xlink managed cordis patch\nschema_version: 1\n";
        // 早期构建写出的**合法**占位模板，现在已是历史形状。它同样是「没有
        // 接线」的状态，且顶层的 `[]` 与后面追加的 patch 条目不能共存于同一个
        // YAML 文档（空流式序列已经结束了一个文档），因此一并按字节匹配后重写。
        const PATCH_YML_LEGACY: &str = "# dsh-xlink managed cordis patch\n[]\n";
        const PATCH_YML_HEADER: &str = "# dsh-xlink managed cordis patch\n";
        let patch_yml = home.join("cordis.patch.yml");
        let existing = match std::fs::read_to_string(&patch_yml) {
            Ok(text) if text == PATCH_YML_BROKEN || text == PATCH_YML_LEGACY => {
                PATCH_YML_HEADER.to_string()
            }
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                PATCH_YML_HEADER.to_string()
            }
            Err(error) => return Err(AdapterError::Io(error.to_string())),
        };
        // 技能接线写不进去**不能让工作台起不来**（少一批技能 ≠ 内核不可用），
        // 但它是「用户看得见后果」的动作，必须留下可查的痕迹。
        match crate::kernel::skill_wiring::ensure_skill_wiring(
            &existing,
            &paths::skills_active_root(),
        ) {
            Ok(next) if next != existing => {
                if let Err(error) = crate::shell::process::atomic_write(&patch_yml, next.as_bytes())
                {
                    let line = format!(
                        "技能接线写入失败（{}）：{error}；工作台看不到壳管理的技能",
                        patch_yml.display()
                    );
                    crate::shell::shell_events::record("skill-wiring", &line);
                    eprintln!("{line}");
                }
            }
            Ok(_) => {}
            Err(why) => {
                let line = format!("跳过技能接线（{why}）：{}", patch_yml.display());
                crate::shell::shell_events::record("skill-wiring", &line);
                eprintln!("{line}");
            }
        }
        Ok(())
    }

    fn start(
        &self,
        record: &InstanceRecord,
        install_root: &Path,
        node: &Path,
        logs_dir: &Path,
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
        let mut cmd = crate::shell::process::command_with_path_dirs(node, &[node_dir]);
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
        // ——以 PATH 风格冒号分隔。**当前内核不消费它**（见 `custom_skill_dirs`
        // 的注释），接通靠的是 `prepare_instance` 写进 `cordis.patch.yml` 的
        // 接线行；这条 env 是给「内核补上回退」那天留的，空列表时**不**写，
        // 避免把空字符串污染到 DSH 端。
        let skill_dirs = self.custom_skill_dirs();
        if !skill_dirs.is_empty() {
            let joined = skill_dirs
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(ENV_PATH_SEP);
            cmd.env("DSH_CUSTOM_SKILL_DIRS", joined);
        }
        // 内嵌 openai-oauth 插件的本地桥接（P2）：接线行在实例 patch 里时
        // 注入地址与令牌；桥接起不来不阻断内核启动（见 bridge::launch_env）。
        for (key, value) in crate::openai::bridge::launch_env(
            &dsh_home,
            &record.profile,
            &record.kernel_family,
            &record.id,
        ) {
            cmd.env(key, value);
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

        let mut child = crate::shell::process::quiet(&mut cmd)
            .spawn()
            .map_err(|e| AdapterError::Io(format!("无法启动内核：{e}")))?;
        crate::shell::process::adopt_kernel_process(&child);
        if let Err(error) = crate::shell::process::attach_log_drainers(
            &mut child,
            logs_dir,
            &crate::kernel::lifecycle::kernel_log_spec(&record.kernel_family, &record.id),
        ) {
            crate::shell::process::terminate_process_tree(&mut child);
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
        _logs_dir: &Path,
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
    use crate::shell::instance::InstanceRecord;
    use crate::shell::paths;
    use crate::tests::scoped_xlink_home;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        let home = temp_dir("prepare");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let mut record = sample_record();
        record.kernel_version = Some("0.1.5-rc.1".into());
        // 先准备目录再 prepare。
        crate::shell::instance::ensure_instance_dirs(&record).expect("ensure dirs");
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
        // dsh-app-boot 要求顶层数组：壳管理的这份文件必须是一条 patch 列表，
        // 且列表里必须有技能接线行（否则工作台看不到壳管理的技能）。
        let patch_text = std::fs::read_to_string(&patch_yml).unwrap();
        assert!(
            patch_text.contains(crate::kernel::skill_wiring::SKILL_WIRING_ROW_ID),
            "cordis.patch.yml 缺技能接线行，实际：{patch_text:?}"
        );
        assert!(
            patch_text.contains(&crate::kernel::skill_wiring::skill_wiring_path_token(
                &paths::skills_active_root()
            )),
            "接线行必须指向共享活动视图，实际：{patch_text:?}"
        );
        assert!(
            !patch_text.lines().any(|line| line.trim() == "[]"),
            "顶层的 [] 会结束整个 YAML 文档，不能与后面的 patch 条目共存：{patch_text:?}"
        );
        // package.json 的形状是**内核的启动契约**，不是壳的记账簿。
        // 2026-10-06 前这里断言的是 `schema_version` 与 `kernel_family` 两个
        // 装饰字段——它们只存在于旧 stub 里，没有任何代码读过，本机真正跑得
        // 起来的实例也从来没有它们，而那份 stub 恰恰是内核起不来的原因。
        // 改钉真正决定成败的那两把钥匙。
        let text = std::fs::read_to_string(&profile_pkg).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            !profile_manifest::needs_repair(&manifest),
            "prepare_instance 落下的清单必须带 dependencies 与 dsh.profile.bundles，实际：{text}"
        );
        assert!(
            manifest["dsh"]["profile"]["bundles"]
                .as_array()
                .is_some_and(|b| b.iter().any(|x| x == "@deepseek-ai/dsh-web-app")),
            "web profile 必须带 web-app 层，实际：{text}"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 历史构建写出的坏模板（`schema_version: 1` 映射）必须被原位修复；
    /// 早期的合法占位模板（顶层 `[]`）也必须换成带接线行的列表。
    /// 用户改过的内容不得被删改，只允许在末尾追加壳自己的行。
    #[test]
    fn prepare_instance_repairs_broken_patch_template() {
        let home = temp_dir("patch-repair");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let record = sample_record();
        crate::shell::instance::ensure_instance_dirs(&record).expect("ensure dirs");
        let patch_yml = DshAdapter::dsh_home_for(&record).join("cordis.patch.yml");
        let active = paths::skills_active_root();
        std::fs::create_dir_all(patch_yml.parent().unwrap()).unwrap();

        // 坏模板：字节级匹配历史构建的输出 → 修复。
        std::fs::write(
            &patch_yml,
            "# dsh-xlink managed cordis patch\nschema_version: 1\n",
        )
        .unwrap();
        adapter.prepare_instance(&record).expect("prepare");
        let repaired = std::fs::read_to_string(&patch_yml).unwrap();
        assert!(
            repaired.contains(crate::kernel::skill_wiring::SKILL_WIRING_ROW_ID)
                && repaired.contains(&crate::kernel::skill_wiring::skill_wiring_path_token(
                    &active
                )),
            "坏模板必须被改写成带技能接线的列表，实际：{repaired:?}"
        );

        // 早期的合法占位模板：顶层 `[]` 同样要换成带接线行的列表。
        std::fs::write(&patch_yml, "# dsh-xlink managed cordis patch\n[]\n").unwrap();
        adapter.prepare_instance(&record).expect("prepare");
        let upgraded = std::fs::read_to_string(&patch_yml).unwrap();
        assert!(
            upgraded.contains(crate::kernel::skill_wiring::SKILL_WIRING_ROW_ID)
                && !upgraded.lines().any(|line| line.trim() == "[]"),
            "旧占位模板必须被升级成带接线的列表，实际：{upgraded:?}"
        );

        // 用户自己的 patch 条目：不得被覆盖，只在其后追加壳的行。
        std::fs::write(&patch_yml, "- my: patch\n").unwrap();
        adapter.prepare_instance(&record).expect("prepare");
        let merged = std::fs::read_to_string(&patch_yml).unwrap();
        assert!(
            merged.starts_with("- my: patch\n"),
            "用户自己的 patch 内容不得被覆盖，实际：{merged:?}"
        );
        assert!(
            merged.contains(crate::kernel::skill_wiring::SKILL_WIRING_ROW_ID),
            "壳的接线行必须追加在用户内容之后，实际：{merged:?}"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 接线行必须幂等：反复 prepare 不该让文件无限增长。
    #[test]
    fn prepare_instance_writes_skill_wiring_once() {
        let home = temp_dir("patch-idempotent");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let record = sample_record();
        crate::shell::instance::ensure_instance_dirs(&record).expect("ensure dirs");
        adapter.prepare_instance(&record).expect("prepare");
        let patch_yml = DshAdapter::dsh_home_for(&record).join("cordis.patch.yml");
        let first = std::fs::read_to_string(&patch_yml).unwrap();
        adapter.prepare_instance(&record).expect("prepare");
        assert_eq!(
            first,
            std::fs::read_to_string(&patch_yml).unwrap(),
            "重复 prepare 不得改动已经就位的接线文件"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 2026-10-06 回归：`prepare_instance` 此前落的是一个**不带
    /// `dsh.profile.bundles`** 的 stub。内核读到零个插件就正常退出（exit 0）
    /// 且一行日志不打，于是每一个新建实例——包括每一次安装预检的沙盒——都
    /// 起不来内核，报告里只剩「沙盒内核在就绪前退出（exit status: 0）」。
    ///
    /// 钉在**适配器的产出**这一侧而不是只测清单模块：那次 bug 的本质就是
    /// 两边各写各的形状，只测 `profile_manifest` 自己的测试全绿也照样漏。
    #[test]
    fn prepare_instance_leaves_a_profile_the_kernel_can_boot() {
        let home = temp_dir("prepare-bootable");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let record = sample_record();
        adapter.prepare_instance(&record).expect("prepare");

        let manifest = DshAdapter::dsh_home_for(&record)
            .join("profiles")
            .join(&record.profile)
            .join("package.json");
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
        assert!(
            !profile_manifest::needs_repair(&root),
            "prepare_instance 产出的清单必须带 dependencies 与 dsh.profile.bundles，实际：{root}"
        );
        let bundles = root["dsh"]["profile"]["bundles"].as_array().unwrap();
        assert!(
            bundles.iter().any(|b| b == "@deepseek-ai/dsh-web-app"),
            "web profile 必须带 web-app 层，否则内核启动后工作台是空的：{bundles:?}"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 存量实例已经被那个 stub 污染过了（`ensure_profile` 见到 `package.json`
    /// 存在就早退，永远补不上）。所以 `prepare_instance` 不能只在文件缺席时
    /// 落初值，还必须把写坏的那份修回来——修的时候不许动别的字段。
    #[test]
    fn prepare_instance_repairs_a_stub_left_by_an_older_build() {
        let home = temp_dir("prepare-repairs-stub");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let record = sample_record();
        crate::shell::instance::ensure_instance_dirs(&record).expect("ensure dirs");
        let profile = DshAdapter::dsh_home_for(&record)
            .join("profiles")
            .join(&record.profile);
        std::fs::create_dir_all(&profile).unwrap();
        // 逐字节复刻旧构建写出的 stub——修不认得就等于没修。
        std::fs::write(
            profile.join("package.json"),
            "{\n  \"name\": \"dsh-xlink-instance-default\",\n  \"private\": true,\n  \"version\": \"0.0.0\",\n  \"schema_version\": 1,\n  \"kernel_family\": \"dsh\"\n}\n",
        )
        .unwrap();

        adapter.prepare_instance(&record).expect("prepare");
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(profile.join("package.json")).unwrap())
                .unwrap();
        assert!(!profile_manifest::needs_repair(&root), "实际：{root}");
        assert_eq!(
            root["kernel_family"], "dsh",
            "修清单不是重写清单：别的字段必须原样保留"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// `resolve_install_root` 在 legacy 路径命中时返回 legacy 目录。
    #[test]
    fn resolve_install_root_falls_back_to_legacy() {
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
        let home = temp_dir("start-missing");
        let _xlink = scoped_xlink_home(&home);
        let adapter = DshAdapter;
        let mut record = sample_record();
        record.kernel_version = Some("0.1.5-rc.1".into());
        let empty = temp_dir("start-empty");
        let err = adapter
            .start(&record, &empty, Path::new("/nonexistent/node"), &empty)
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
                _logs_dir: &Path,
            ) -> Result<std::process::Child, AdapterError> {
                Err(AdapterError::Io("unused".into()))
            }
        }
        assert!(MinimalAdapter.custom_skill_dirs().is_empty());
    }

    #[test]
    fn dsh_adapter_custom_skill_dirs_returns_skills_active_root() {
        // DshAdapter 必须报告 Xlink 的共享技能活动视图——v1 全局共享。
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
        let home = temp_dir("mcode-start");
        let _xlink = scoped_xlink_home(&home);
        let mut record = sample_record();
        record.kernel_family = "mcode".into();
        record.kernel_version = Some("0.1.0".into());
        let empty = temp_dir("mcode-start-empty");
        let err = McodeAdapter
            .start(&record, &empty, Path::new("/nonexistent/node"), &empty)
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
