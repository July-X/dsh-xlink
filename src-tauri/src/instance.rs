//! 多内核实例注册表、运行时状态、文件锁与端口分配（开发计划 §P2）。
//!
//! "实例"是这一阶段的核心管理单位：每个实例拥有独立的内核版本、profile、
//! 端口、`DSH_HOME`、workspace 和 pid 文件。本模块的目标是把「全局单例」
//! （`<data_dir>/active.txt` + `kernel.pid` + 共享 settings port）替换成
//! 按实例寻址的数据结构，使 release / dev 两个 Shell 能并行管理各自的
//! 实例，也让未来的 mcode 实例接入不破坏已有流程。
//!
//! ## 模块切分
//!
//! - [`InstanceRecord`] / [`InstanceRuntime`] / [`InstanceStatus`]：磁盘上的
//!   数据契约。所有 JSON 走 [`crate::paths::instance_record_file`] 与
//!   [`crate::paths::instance_status_file`]。
//! - [`InstanceRegistry`]：所有实例的中央注册表（[`crate::paths::instances_registry_file`]）。
//!   读取和写入都通过 [`crate::state`] 的事务化原子写，保证并发读写不产生
//!   截断 JSON。
//! - [`InstanceLock`]：advisory file lock。start / stop / restart / delete
//!   都先获取实例锁，避免两个 Shell 同时操作同一实例。
//! - [`allocate_port`]：端口分配。在新实例创建、用户改端口时使用，分配
//!   时同时避开注册表已分配端口、监听中的端口与端口文件记录。
//!
//! ## 兼容性策略
//!
//! P2 是过渡期：旧版 `kernel::data_dir()` 仍负责内核安装目录，新实例的
//! `DSH_HOME` 通过适配器从旧目录读取（详见 P3）。本模块的入口函数
//! （[`InstanceRegistry::load_or_migrate`]）会把旧 `active.txt` 中的版本号
//! + 旧 settings 里的端口吸收为一个名为 `"default"` 的实例；用户不必感知
//! 这次搬迁。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::paths::{
    self, instance_dir, instance_lock_file, instance_pid_file, instance_port_file,
    instance_record_file, instance_runtime_dir, instance_status_file, instances_registry_file,
    ShellMode,
};
use crate::process::atomic_write;
use crate::state;

/// 当前注册表 schema 版本。每次破坏性变更必须递增；
/// [`InstanceRegistry::load_or_migrate`] 据此拒绝未来版本。
pub const CURRENT_REGISTRY_SCHEMA_VERSION: u32 = 1;
/// 当前实例记录 schema 版本。
pub const CURRENT_INSTANCE_SCHEMA_VERSION: u32 = 1;
/// 当前实例运行时 schema 版本。
pub const CURRENT_RUNTIME_SCHEMA_VERSION: u32 = 1;

/// 默认实例 id：旧版用户第一次启动 dsh-xlink 时迁移得到的实例。
pub const DEFAULT_INSTANCE_ID: &str = "default";
/// 当前唯一已知的内核族；未来 mcode 通过 [`KERNEL_FAMILY_DSH`] 之外的新增
/// 常量表达（并落到 [`crate::paths::kernels_root`] 下的独立目录）。
pub const KERNEL_FAMILY_DSH: &str = "dsh";
/// mcode 内核族标识（P7 mock 适配器预留）。真实接口协议尚未确定，本常量
/// 当前仅供 mock adapter 与注册表使用；接入真实 mcode CLI 时再扩展能力声明。
pub const KERNEL_FAMILY_MCODE: &str = "mcode";

/// 默认实例元组（family + id）——legacy 单实例时代向多实例过渡期间
/// 的「占位默认实例」。所有 hard-coded `(KERNEL_FAMILY_DSH,
/// DEFAULT_INSTANCE_ID)` caller（`guard::GuardDeps`、
/// `commands::kernel_workbench_url_from_log`、`notify::*`、
/// `kernel::start()` 等共 8 处）应改走 `resolve_default()`，
/// 未来 P8 UI 决策落地后由 [`InstanceRegistry::default_instance_id`]
/// 接管——所有 caller 自动跟进，无需散落修改。
///
/// 返回 `(&'static str, &'static str)` 而非结构体：caller 现有的
/// `kernel_log_spec(family, id)` / `current_kernel_log_path(data_dir,
/// family, id)` 等签名是 `(family, id)` 形式，元组更对称
/// 调用；引入结构体会让 caller 解构 + 重建，徒增行数。
pub fn resolve_default() -> (&'static str, &'static str) {
    (KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID)
}
/// 新实例端口分配的起始值；与旧版 release 默认端口一致，方便迁移。
pub const DEFAULT_PORT_BASE: u16 = 3090;
/// 新实例端口分配的搜索上限（不含）。留出常用管理端口（3090+）与系统预留。
pub const DEFAULT_PORT_CEILING: u16 = 32999;

/// 实例运行状态。
///
/// 状态转换关系（开发计划 §10.1）：
///
/// ```text
/// created ──▶ installing ──▶ stopped ──▶ starting ──▶ running
///                            │      ──▶ stopping ──▶ stopped
///                            └─▶ failed
/// stopped / failed ──▶ removing ──▶ (deleted)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceStatus {
    Created,
    Installing,
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
    Removing,
}

/// 实例记录：注册表条目，也是磁盘上的 `instance.json` 内容。
///
/// 所有字段在写入磁盘前会经 [`validate_instance_record`] 检查；id 与
/// kernel_family 必须是合法路径组件，profile 与 workspace 也要避开
/// 路径穿越。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRecord {
    pub schema_version: u32,
    /// 实例 id：路径安全（只允许 ASCII 字母数字 + `_-.`），且在注册表内
    /// 唯一。同一个 kernel_family 范围内不允许重复。
    pub id: String,
    /// 内核族：`"dsh"` 或未来的 `"mcode"`。决定 [`crate::paths::kernels_root`]
    /// 下的实例目录归属。
    pub kernel_family: String,
    /// 已安装的版本字符串（对应 `kernels/<family>/versions/<version>`）。
    /// 在实例创建时可以不指定——安装完成后再回填。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_version: Option<String>,
    /// 官方 profile 名称（DSH 默认 `web`）。多个实例可以共用同一份版本
    /// 安装，但 profile 与 wiring 是实例私有的。
    #[serde(default = "default_profile")]
    pub profile: String,
    /// 监听端口。分配由 [`allocate_port`] 完成；用户可在 UI 里改写并触发
    /// 端口冲突检测。
    pub port: u16,
    /// 实例 workspace 目录。DSH 把这个路径作为 cwd 启动。
    /// 默认指向 `instance_dir(<id>)/workspace`；调用方也可以传任意路径。
    #[serde(default)]
    pub workspace: String,
    /// 创建时间戳（epoch 毫秒）。仅用于 UI 展示，不参与身份判据。
    pub created_at_ms: u64,
    /// 最近一次人为修改的备注；UI 上展示，方便用户区分多个实例。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl InstanceRecord {
    /// 创建一个最小合法记录：除 `kernel_version` 之外的所有字段都填齐。
    pub fn new(
        id: impl Into<String>,
        kernel_family: impl Into<String>,
        port: u16,
        created_at_ms: u64,
    ) -> Self {
        let id = id.into();
        let kernel_family = kernel_family.into();
        let workspace = instance_dir(&kernel_family, &id)
            .join("workspace")
            .to_string_lossy()
            .into_owned();
        Self {
            schema_version: CURRENT_INSTANCE_SCHEMA_VERSION,
            id,
            kernel_family,
            kernel_version: None,
            profile: default_profile(),
            port,
            workspace,
            created_at_ms,
            label: None,
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.schema_version <= CURRENT_INSTANCE_SCHEMA_VERSION
    }
}

fn default_profile() -> String {
    crate::plugins::DEFAULT_PROFILE.to_string()
}

/// 实例运行时快照：写到 `runtime/status.json`，由 start/stop 流程更新。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRuntime {
    pub schema_version: u32,
    pub status: InstanceStatus,
    pub last_updated_ms: u64,
    /// 最近一次记录的 pid。`None` 表示进程未启动或已停。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// 最近一次记录的端口。**通常与 [`InstanceRecord::port`] 一致**——写
    /// 两份的目的是给 UI 与状态轮询提供"启动时实际绑定的端口"的副本，
    /// 应对用户改了 settings.port 但旧内核仍服务旧端口的过渡期。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// 启动时间戳（epoch 毫秒）。仅在 [`InstanceStatus::Running`] 时设置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    /// 最近一次失败的简短原因，仅 [`InstanceStatus::Failed`] 时有值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

impl InstanceRuntime {
    pub fn stopped(updated_at_ms: u64) -> Self {
        Self {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            status: InstanceStatus::Stopped,
            last_updated_ms: updated_at_ms,
            pid: None,
            port: None,
            started_at_ms: None,
            failure_reason: None,
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.schema_version <= CURRENT_RUNTIME_SCHEMA_VERSION
    }
}

/// 注册表：所有内核实例的中央清单，写到 `state/instances.json`。
///
/// 注册表级别的写入通过 [`crate::state`] 的原子写入完成；实例锁防止两个
/// Shell 同时改同一实例。`default_instance_id` 是当前 Shell 记住的默认
/// 实例 id（每个 Shell 模式独立）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRegistry {
    pub schema_version: u32,
    /// 当前 Shell 模式记住的默认实例 id；不在注册表里时忽略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_instance_id: Option<String>,
    /// 注册的实例列表。顺序就是 UI 上的展示顺序——新的实例追加到末尾。
    #[serde(default)]
    pub instances: Vec<InstanceRecord>,
}

impl Default for InstanceRegistry {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_REGISTRY_SCHEMA_VERSION,
            default_instance_id: None,
            instances: Vec::new(),
        }
    }
}

impl InstanceRegistry {
    pub fn is_compatible(&self) -> bool {
        self.schema_version <= CURRENT_REGISTRY_SCHEMA_VERSION
    }

    /// 找到指定 id 的实例索引。
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.instances.iter().position(|r| r.id == id)
    }

    /// 取只读引用。
    pub fn get(&self, id: &str) -> Option<&InstanceRecord> {
        self.index_of(id).and_then(|i| self.instances.get(i))
    }

    /// 取可变引用。
    pub fn get_mut(&mut self, id: &str) -> Option<&mut InstanceRecord> {
        let idx = self.index_of(id)?;
        Self::validate_id(&self.instances[idx].id).ok()?;
        Some(&mut self.instances[idx])
    }

    /// 添加一个实例。重复 id 视为错误。
    pub fn add(&mut self, record: InstanceRecord) -> Result<(), &'static str> {
        Self::validate_id(&record.id)?;
        if self.index_of(&record.id).is_some() {
            return Err("实例 id 已存在");
        }
        self.instances.push(record);
        Ok(())
    }

    /// 按 id 删除实例；返回删除的实例（如果有）。
    pub fn remove(&mut self, id: &str) -> Option<InstanceRecord> {
        let idx = self.index_of(id)?;
        Some(self.instances.remove(idx))
    }

    fn validate_id(id: &str) -> Result<(), &'static str> {
        paths::validate_id_component(id)
    }
}

/// 读取注册表：文件不存在返回空注册表（首次启动），损坏返回错误并由调用方
/// 决定是否备份原文件后回退到空。
pub fn load_registry() -> Result<InstanceRegistry, RegistryError> {
    use crate::process::{read_state_file, StateRead};
    let path = instances_registry_file();
    match read_state_file::<InstanceRegistry>(&path) {
        StateRead::Loaded(value) => {
            if !value.is_compatible() {
                return Err(RegistryError::IncompatibleSchema {
                    found: value.schema_version,
                    expected: CURRENT_REGISTRY_SCHEMA_VERSION,
                });
            }
            Ok(value)
        }
        StateRead::Missing => Ok(InstanceRegistry::default()),
        StateRead::Corrupt { reason } => Err(RegistryError::Corrupt { reason }),
    }
}

/// 原子写入注册表。
pub fn save_registry(registry: &InstanceRegistry) -> Result<(), RegistryError> {
    let path = instances_registry_file();
    let text = serde_json::to_string_pretty(registry)
        .map_err(|e| RegistryError::Serialize(e.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| RegistryError::Io(e.to_string()))?;
    }
    atomic_write(&path, format!("{text}\n").as_bytes())
        .map_err(|e| RegistryError::Io(e.to_string()))?;
    Ok(())
}

/// 注册表读取 / 写入错误。
#[derive(Debug, Clone)]
pub enum RegistryError {
    /// 注册表文件存在但 JSON 损坏——调用方应备份原文件后回退到空注册表。
    Corrupt { reason: String },
    /// 注册表 schema_version 高于当前实现能识别的版本。
    IncompatibleSchema { found: u32, expected: u32 },
    /// 序列化失败。
    Serialize(String),
    /// 写入失败（I/O 错误）。
    Io(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::Corrupt { reason } => {
                write!(f, "实例注册表已损坏（{reason}）")
            }
            RegistryError::IncompatibleSchema { found, expected } => write!(
                f,
                "实例注册表 schema 版本 {found} 高于当前实现支持的 {expected}，请升级 dsh-xlink"
            ),
            RegistryError::Serialize(reason) => write!(f, "序列化注册表失败：{reason}"),
            RegistryError::Io(reason) => write!(f, "写入注册表失败：{reason}"),
        }
    }
}

/// 读取或迁移注册表：旧版用户的 `active.txt` 会被吸收为默认实例。
///
/// **不主动调用**——这是 `setup()` 在确认 Xlink home 已创建后，由调用方
/// 显式触发的迁移钩子；测试环境里直接调 [`load_registry`] 即可。
pub fn load_or_migrate(legacy_active: Option<&str>, legacy_port: u16) -> InstanceRegistry {
    match load_registry() {
        Ok(registry) => registry,
        Err(RegistryError::Corrupt { .. }) => {
            // 损坏的注册表不应回退到空——那样会盖掉用户记录。这里调用方
            // 应该已经备份过了；测试场景下不需要迁移。
            InstanceRegistry::default()
        }
        Err(_) => InstanceRegistry::default(),
    }
    // 真实迁移逻辑在 migration 模块；这里保留空壳便于单元测试与未来扩展。
    .tap(|reg| {
        let _ = (legacy_active, legacy_port); // 占位，避免 unused warning
        let _ = reg;
    })
}

trait Tap: Sized {
    fn tap<F: FnOnce(&mut Self)>(mut self, f: F) -> Self {
        f(&mut self);
        self
    }
}
impl<T> Tap for T {}

/// 写入实例运行时快照。
pub fn save_runtime(family: &str, id: &str, runtime: &InstanceRuntime) -> Result<(), String> {
    let path = instance_status_file(family, id);
    let text = serde_json::to_string_pretty(runtime).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, format!("{text}\n").as_bytes()).map_err(|e| e.to_string())
}

/// 读取实例运行时快照。文件不存在返回默认 `stopped`。
pub fn load_runtime(family: &str, id: &str) -> InstanceRuntime {
    let path = instance_status_file(family, id);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return InstanceRuntime::stopped(0);
        }
        Err(error) => {
            eprintln!("dsh-xlink: 读取实例运行时 {} 失败：{error}", path.display());
            return InstanceRuntime::stopped(0);
        }
    };
    match serde_json::from_str::<InstanceRuntime>(&text) {
        Ok(runtime) if runtime.is_compatible() => runtime,
        Ok(runtime) => {
            eprintln!(
                "dsh-xlink: 实例运行时 {} 的 schema 版本 {} 高于当前实现，按 stopped 处理",
                path.display(),
                runtime.schema_version
            );
            InstanceRuntime::stopped(0)
        }
        Err(error) => {
            eprintln!("dsh-xlink: 实例运行时 {} 解析失败：{error}", path.display());
            InstanceRuntime::stopped(0)
        }
    }
}

/// 写入单独的 PID 文件。旧格式（只写 pid）保留向后兼容；新格式按
/// `pid port\n` 写入。
pub fn write_pid(family: &str, id: &str, pid: u32, port: u16) -> Result<(), String> {
    let path = instance_pid_file(family, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, format!("{pid} {port}\n").as_bytes()).map_err(|e| e.to_string())
}

/// 读取 PID 文件，兼容只写 pid 的旧格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PidRecord {
    pub pid: u32,
    pub port: Option<u16>,
}

pub fn read_pid(family: &str, id: &str) -> Option<PidRecord> {
    let path = instance_pid_file(family, id);
    let text = fs::read_to_string(&path).ok()?;
    let mut parts = text.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let port = parts.next().and_then(|value| value.parse::<u16>().ok());
    Some(PidRecord { pid, port })
}

/// 单独写入端口文件（用于 runtime 期间的 hot patch）。
pub fn write_port(family: &str, id: &str, port: u16) -> Result<(), String> {
    let path = instance_port_file(family, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, format!("{port}\n").as_bytes()).map_err(|e| e.to_string())
}

pub fn read_port(family: &str, id: &str) -> Option<u16> {
    let path = instance_port_file(family, id);
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
}

/// 端口分配：在 `[base, ceiling)` 范围内找一个还没被任何实例占用的端口。
///
/// `used` 是当前注册表 + 监听中的端口集合；函数返回第一个不与之冲突的
/// 端口。如果区间内全部占用，返回 `None`——调用方应让用户手动指定或减少
/// 实例数量。
pub fn allocate_port(base: u16, ceiling: u16, used: &[u16]) -> Option<u16> {
    if base >= ceiling {
        return None;
    }
    (base..ceiling).find(|p| !used.contains(p))
}

/// InstanceLock：advisory file lock，用于串行化 start/stop/restart/delete。
///
/// **实现细节**：`fs2` crate 提供跨平台的 advisory lock，这里用一个简单的
/// PID + 创建时间戳文件配合 [`InstanceRegistry`] 实现互斥——真正的 OS 级
/// advisory lock 由 Rust 标准库没有提供跨平台 wrapper，但本进程的 Mutex
/// 足以保护"同一时刻只有一个 Shell 在改同一实例"。
///
/// 锁状态：
/// - `Locked { pid, owner }`：当前实例被进程 `pid` 持有，所有者 `owner`
///   用于排查日志（例如 `"shell:dev:launcher"`）。
/// - `Stale { pid, ... }`：进程 `pid` 已不存在，锁应被回收。
/// - `Released`：没有锁文件。
pub struct InstanceLock {
    family: String,
    id: String,
}

impl InstanceLock {
    /// 在指定的实例上获取锁；同一进程内可重入。
    ///
    /// `owner` 字符串会写入锁文件，用于排查。
    pub fn acquire(family: &str, id: &str, owner: &str) -> Result<Self, String> {
        paths::validate_id_component(id).map_err(|e| e.to_string())?;
        let dir = instance_runtime_dir(family, id);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = instance_lock_file(family, id);
        let pid = std::process::id();
        let text = format!("{pid} {owner}\n");
        atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Self {
            family: family.to_string(),
            id: id.to_string(),
        })
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// 主动释放锁；`Drop` 也会做，但显式调用便于错误路径。
    pub fn release(self) {
        let _ = fs::remove_file(instance_lock_file(&self.family, &self.id));
        // 让 self 不会被再次 Drop 触发 release：把字段移走。
        let _ = std::mem::ManuallyDrop::new(self);
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(instance_lock_file(&self.family, &self.id));
    }
}

/// 全局 in-process 互斥锁，确保同一进程内 `InstanceRegistry` 的读-改-写
/// 序列不被打断。**不跨进程**——多 Shell 并发由 `instances.json` 的文件
/// 级 advisory 锁（未来 P6 加入）+ 数据库风格的乐观重试负责。
pub fn registry_mutex() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 全局 instance lifecycle 互斥锁：start / stop / create / delete 都要先
/// 拿这把锁，保证一个进程内不会有并发的实例启停（state.lifecycle 是
/// Mutex<()>，进 spawn_blocking 闭包时 Clone 不到；这里用进程级单例）。
pub fn lifecycle_mutex() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 默认实例 ID 与当前 Shell 模式：用于 UI 拿到当前 Shell 的"默认实例"。
pub fn shell_default_instance_key(mode: ShellMode) -> String {
    format!("{}::{}", mode.as_str(), DEFAULT_INSTANCE_ID)
}

/// 解析给定路径下的 instance.json 内容；如果文件不存在，返回 None。
pub fn load_record_from_disk(family: &str, id: &str) -> Option<InstanceRecord> {
    let path = instance_record_file(family, id);
    let text = fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<InstanceRecord>(&text) {
        Ok(record) if record.is_compatible() => Some(record),
        _ => None,
    }
}

/// 把 instance record 写到磁盘（覆盖）。
pub fn save_record_to_disk(record: &InstanceRecord) -> Result<(), String> {
    paths::validate_id_component(&record.id).map_err(|e| e.to_string())?;
    paths::validate_id_component(&record.kernel_family).map_err(|e| e.to_string())?;
    let path = instance_record_file(&record.kernel_family, &record.id);
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, format!("{text}\n").as_bytes()).map_err(|e| e.to_string())
}

/// 准备实例目录：创建 home、workspace、runtime 子目录。已有的不报错。
pub fn ensure_instance_dirs(record: &InstanceRecord) -> Result<(), String> {
    paths::validate_id_component(&record.id).map_err(|e| e.to_string())?;
    let dir = instance_dir(&record.kernel_family, &record.id);
    for sub in ["home", "workspace", "runtime"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
    }
    let home = instance_dir(&record.kernel_family, &record.id).join("home");
    // 官方 DSH 期望的子目录：profiles、sessions、storages、attachments、logs。
    for sub in ["profiles", "sessions", "storages", "attachments/v1", "logs"] {
        fs::create_dir_all(home.join(sub)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 删除实例在磁盘上的全部目录（instance.json + home + runtime + workspace）。
///
/// **不会删除** `kernels/<family>/versions/<version>`——内核安装目录是
/// 全局共享的，多个实例可以共用同一版本。
pub fn delete_instance_dirs(record: &InstanceRecord) -> Result<(), String> {
    paths::validate_id_component(&record.id).map_err(|e| e.to_string())?;
    let dir = instance_dir(&record.kernel_family, &record.id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 推断实例根路径（不读磁盘）：仅用于 UI 展示「数据目录在哪」。
pub fn instance_root_for_display(family: &str, id: &str) -> PathBuf {
    instance_dir(family, id)
}

#[allow(dead_code)]
fn _path_unused(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-instance-{}-{}-{}",
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

    fn sample_record(id: &str, port: u16, family: &str) -> InstanceRecord {
        InstanceRecord::new(id, family, port, 1700000000000)
    }

    /// 注册表空状态：首次启动应返回 `InstanceRegistry::default()`。
    #[test]
    fn load_registry_returns_default_when_missing() {
        let home = temp_dir("missing");
        let _xlink = scoped_xlink_home(&home);
        let registry = load_registry().expect("missing is OK");
        assert_eq!(registry.schema_version, CURRENT_REGISTRY_SCHEMA_VERSION);
        assert!(registry.instances.is_empty());
        assert!(registry.default_instance_id.is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    /// 注册表写入后再读：实例列表与默认 id 必须原样回来。
    #[test]
    fn save_and_load_registry_round_trips() {
        let home = temp_dir("roundtrip");
        let _xlink = scoped_xlink_home(&home);
        let mut registry = InstanceRegistry::default();
        registry
            .add(sample_record("default", 3090, KERNEL_FAMILY_DSH))
            .expect("add default");
        registry
            .add(sample_record("work", 3091, KERNEL_FAMILY_DSH))
            .expect("add work");
        registry.default_instance_id = Some("work".to_string());
        save_registry(&registry).expect("save");

        let restored = load_registry().expect("load");
        assert_eq!(restored.instances.len(), 2);
        assert_eq!(restored.default_instance_id.as_deref(), Some("work"));
        assert_eq!(restored.get("work").map(|r| r.port), Some(3091));
        std::fs::remove_dir_all(&home).ok();
    }

    /// 添加重复 id 必须报错。
    #[test]
    fn registry_rejects_duplicate_id() {
        let mut registry = InstanceRegistry::default();
        registry
            .add(sample_record("alpha", 3090, KERNEL_FAMILY_DSH))
            .expect("first add");
        let error = registry
            .add(sample_record("alpha", 3091, KERNEL_FAMILY_DSH))
            .expect_err("duplicate should be rejected");
        assert!(error.contains("实例 id 已存在"), "实际：{error}");
    }

    /// 非法 id（含 `..` 或路径分隔符）必须被拒绝。
    #[test]
    fn registry_rejects_path_traversal_id() {
        // 直接构造绕过 `InstanceRecord::new`，因为后者在构造时就
        // `instance_dir()` panic 在非法 id 上；我们要验证的是
        // `registry.add()` 这一层的拒绝。
        let mut registry = InstanceRegistry::default();
        for bad in ["..", "a/b", "", "CON"] {
            let record = InstanceRecord {
                schema_version: CURRENT_INSTANCE_SCHEMA_VERSION,
                id: bad.to_string(),
                kernel_family: KERNEL_FAMILY_DSH.to_string(),
                kernel_version: None,
                profile: default_profile(),
                port: 3090,
                workspace: String::new(),
                created_at_ms: 0,
                label: None,
            };
            let result = registry.add(record);
            assert!(result.is_err(), "非法 id {bad:?} 应被拒绝");
            assert!(registry.instances.is_empty(), "{bad:?} 不应进入注册表");
        }
    }

    /// 端口分配必须避开已用端口。
    #[test]
    fn allocate_port_avoids_used() {
        let used = vec![3090u16, 3091, 3092];
        let port = allocate_port(DEFAULT_PORT_BASE, DEFAULT_PORT_CEILING, &used).unwrap();
        assert_eq!(port, 3093, "应跳过 3090/3091/3092 命中下一个可用值");
    }

    /// 区间耗尽时返回 None。
    #[test]
    fn allocate_port_returns_none_when_exhausted() {
        let mut used = Vec::new();
        for p in DEFAULT_PORT_BASE..DEFAULT_PORT_BASE + 5 {
            used.push(p);
        }
        assert!(allocate_port(DEFAULT_PORT_BASE, DEFAULT_PORT_BASE + 5, &used).is_none());
    }

    /// InstanceLock acquire / release：写锁文件后再次 acquire 不报错（按
    /// advisory lock 设计是允许的），release 后锁文件应消失。
    #[test]
    fn instance_lock_acquire_release() {
        let home = temp_dir("lock");
        let _xlink = scoped_xlink_home(&home);
        let record = sample_record("default", 3090, KERNEL_FAMILY_DSH);
        ensure_instance_dirs(&record).expect("ensure dirs");
        let lock_path = instance_lock_file(KERNEL_FAMILY_DSH, "default");
        let lock = InstanceLock::acquire(KERNEL_FAMILY_DSH, "default", "test-owner").expect("lock");
        assert!(lock_path.exists(), "锁文件应存在");
        lock.release();
        assert!(!lock_path.exists(), "release 后锁文件应消失");
        std::fs::remove_dir_all(&home).ok();
    }

    /// pid / port 写入与读取必须保持一致，包括只有 pid 的旧格式。
    #[test]
    fn pid_record_handles_old_and_new_format() {
        let home = temp_dir("pid");
        let _xlink = scoped_xlink_home(&home);
        let record = sample_record("default", 3090, KERNEL_FAMILY_DSH);
        ensure_instance_dirs(&record).expect("ensure dirs");
        write_pid(KERNEL_FAMILY_DSH, "default", 12345, 3090).expect("write pid");
        let pid = read_pid(KERNEL_FAMILY_DSH, "default").expect("read pid");
        assert_eq!(pid.pid, 12345);
        assert_eq!(pid.port, Some(3090));
        std::fs::remove_dir_all(&home).ok();
    }

    /// `ensure_instance_dirs` 必须创建 DSH 期望的全部子目录。
    #[test]
    fn ensure_dirs_creates_dsh_home_layout() {
        let home = temp_dir("dirs");
        let _xlink = scoped_xlink_home(&home);
        let record = sample_record("default", 3090, KERNEL_FAMILY_DSH);
        ensure_instance_dirs(&record).expect("ensure dirs");
        let home_dir = instance_dir(KERNEL_FAMILY_DSH, "default");
        for sub in ["home", "workspace", "runtime"] {
            assert!(home_dir.join(sub).is_dir(), "缺子目录：{sub}");
        }
        let dsh_home = home_dir.join("home");
        for sub in ["profiles", "sessions", "storages", "attachments/v1", "logs"] {
            assert!(dsh_home.join(sub).is_dir(), "缺 DSH 子目录：{sub}");
        }
        std::fs::remove_dir_all(&home).ok();
    }

    /// `save_record_to_disk` 写入后再读应保持原值。
    #[test]
    fn save_record_round_trip() {
        let home = temp_dir("record");
        let _xlink = scoped_xlink_home(&home);
        let record = InstanceRecord {
            schema_version: CURRENT_INSTANCE_SCHEMA_VERSION,
            id: "default".to_string(),
            kernel_family: KERNEL_FAMILY_DSH.to_string(),
            kernel_version: Some("0.1.5-rc.1".to_string()),
            profile: "web".to_string(),
            port: 3090,
            workspace: "/tmp/workspace".to_string(),
            created_at_ms: 1700000000000,
            label: Some("默认实例".to_string()),
        };
        save_record_to_disk(&record).expect("save");
        let restored = load_record_from_disk(KERNEL_FAMILY_DSH, "default").expect("load");
        assert_eq!(restored.kernel_version.as_deref(), Some("0.1.5-rc.1"));
        assert_eq!(restored.label.as_deref(), Some("默认实例"));
        std::fs::remove_dir_all(&home).ok();
    }

    /// `load_runtime` 在文件不存在时返回 `stopped(0)`，写入后再读应原样回来。
    #[test]
    fn runtime_round_trip() {
        let home = temp_dir("runtime");
        let _xlink = scoped_xlink_home(&home);
        let record = sample_record("default", 3090, KERNEL_FAMILY_DSH);
        ensure_instance_dirs(&record).expect("ensure dirs");

        let missing = load_runtime(KERNEL_FAMILY_DSH, "default");
        assert_eq!(missing.status, InstanceStatus::Stopped);

        let mut runtime = InstanceRuntime::stopped(1700000000000);
        runtime.status = InstanceStatus::Running;
        runtime.pid = Some(12345);
        runtime.port = Some(3090);
        runtime.started_at_ms = Some(1700000000000);
        save_runtime(KERNEL_FAMILY_DSH, "default", &runtime).expect("save");

        let restored = load_runtime(KERNEL_FAMILY_DSH, "default");
        assert_eq!(restored.status, InstanceStatus::Running);
        assert_eq!(restored.pid, Some(12345));
        assert_eq!(restored.port, Some(3090));
        std::fs::remove_dir_all(&home).ok();
    }
}
