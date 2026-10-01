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
//!   数据契约。所有 JSON 走 [`crate::shell::paths::instance_record_file`] 与
//!   [`crate::shell::paths::instance_status_file`]。
//! - [`InstanceRegistry`]：所有实例的中央注册表（[`crate::shell::paths::instances_registry_file`]）。
//!   读取和写入都通过 [`crate::shell::state`] 的事务化原子写，保证并发读写不产生
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
//!   这次搬迁。

use crate::shell;
use crate::shell::paths::instance_dir;
use crate::shell::paths::instance_lock_file;
use crate::shell::paths::instance_pid_file;
use crate::shell::paths::instance_port_file;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::shell::paths::{
    instance_record_file, instance_runtime_dir, instance_status_file, instances_registry_file,
    ShellMode,
};
use crate::shell::process::atomic_write;
/// 当前注册表 schema 版本。每次破坏性变更必须递增；
/// [`InstanceRegistry::load_or_migrate`] 据此拒绝未来版本。
pub const CURRENT_REGISTRY_SCHEMA_VERSION: u32 = 1;
/// 当前实例记录 schema 版本。
pub const CURRENT_INSTANCE_SCHEMA_VERSION: u32 = 1;
/// 当前实例运行时 schema 版本。
pub const CURRENT_RUNTIME_SCHEMA_VERSION: u32 = 1;

/// release 壳的默认实例 id：旧版用户第一次启动 dsh-xlink 时迁移得到的实例。
pub const DEFAULT_INSTANCE_ID: &str = "default";
/// dev 壳的默认实例 id。**与 release 分家**，见 [`default_instance_id`]。
pub const DEV_DEFAULT_INSTANCE_ID: &str = "default-dev";
/// 当前唯一已知的内核族；未来 mcode 通过 [`KERNEL_FAMILY_DSH`] 之外的新增
/// 常量表达（并落到 [`crate::shell::paths::kernels_root`] 下的独立目录）。
pub const KERNEL_FAMILY_DSH: &str = "dsh";
/// mcode 内核族标识（P7 mock 适配器预留）。真实接口协议尚未确定，本常量
/// 当前仅供 mock adapter 与注册表使用；接入真实 mcode CLI 时再扩展能力声明。
pub const KERNEL_FAMILY_MCODE: &str = "mcode";

/// **当前壳**的默认实例 id：release 用 [`DEFAULT_INSTANCE_ID`]，dev 用
/// [`DEV_DEFAULT_INSTANCE_ID`]。
///
/// 两个壳必须分家。壳自己的数据目录早就分家了（`desktop/` 与 `desktop-dev/`，
/// 见 [`crate::shell::paths::shell_dir`]），但**实例没有**——两边共用同一个默认实例，
/// 于是共用同一棵 DSH home：`profiles/web/`（profile 接线）、`extensions/plugins/`
/// （插件物化）、会话、凭据。实测后果：dev 装完新内核重跑一次接线，就把
/// release 正在跑的内核的 profile 换掉了；dev 更新中央库里的插件源码，release
/// 的内核通过 link 立刻改用新代码——工作台当场抛出
/// `scope '…' rendered without an installed adapter`，页面白屏。
///
/// 端口随之分开（实例记录里的 `port` 来自各自的 settings：3090 / 3091），
/// 两套环境可以同时跑。
pub fn default_instance_id() -> &'static str {
    default_instance_id_for(crate::shell::settings::current_mode())
}

/// 模式 → 默认实例 id 的映射。抽出来只为能在一个构建里同时测到两种模式
/// （`current_mode()` 由 `debug_assertions` 决定，测试进程恒为 dev）。
pub fn default_instance_id_for(mode: crate::shell::paths::ShellMode) -> &'static str {
    match mode {
        crate::shell::paths::ShellMode::Dev => DEV_DEFAULT_INSTANCE_ID,
        crate::shell::paths::ShellMode::Release => DEFAULT_INSTANCE_ID,
    }
}

/// 当前壳的默认实例元组（family + id）。所有生产 caller 都应走这里，
/// 不要 hard-code `DEFAULT_INSTANCE_ID`——那正是两套环境互相踩的入口。
///
/// 返回 `(&'static str, &'static str)` 而非结构体：caller 现有的
/// `kernel_log_spec(family, id)` / `current_kernel_log_path(data_dir,
/// family, id)` 等签名是 `(family, id)` 形式，元组更对称
/// 调用；引入结构体会让 caller 解构 + 重建，徒增行数。
pub fn resolve_default() -> (&'static str, &'static str) {
    (KERNEL_FAMILY_DSH, default_instance_id())
}

/// 历史数据（`~/.dsh` 里的会话/凭据/profile）永远只搬进 **release** 实例。
///
/// 不能跟着 [`default_instance_id`] 走：谁先跑谁搬走的话，dev 壳会把 release
/// 用户的历史数据搬进 `default-dev`，release 侧打开工作台就是空的。
pub fn legacy_migration_target() -> (&'static str, &'static str) {
    (KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID)
}

/// 本壳当前服务的实例 id：用户在顶部页签切换过的优先，否则按壳分家的默认值。
///
/// 刻意**不读**注册表的 `default_instance_id` 字段——那是一份共享状态，
/// 由 [`crate::shell::instance::default_family`] 用来解析 `data_dir` 的族；两个壳
/// 都能改它，就会出现「dev 壳点一下页签，release 壳下次启动就把 data_dir
/// 指到 dev 的实例」。壳自己的选择存在壳自己的 settings
/// （`shell/<mode>/settings.json`）里，见 [`set_current_instance_id`]。
///
/// 选择还必须是本壳注册表里**真实存在**的实例：注册表分家
/// （[`crate::shell::registry_split`]）之前两边共用一份文件，release 壳可以（且
/// 真的可能）把壳内选择切到 `default-dev` 并存进自己的 settings；分家收敛后
/// 那个 id 会从本壳注册表里让位消失，而 settings 里的值没人清。不校验的话，
/// 「找回历史会话」会把会话回收到**另一个壳的实例**里，列表的「默认」高亮
/// 也会一个都对不上。注册表读不出来（损坏等瞬态）时**保留**选择：读失败
/// 不等于实例不存在，静默丢掉用户的显式选择更糟。
pub fn current_instance_id() -> String {
    let chosen = crate::shell::settings::load_for_shell(crate::shell::settings::current_mode())
        .current_instance_id;
    let Some(id) = chosen else {
        return default_instance_id().to_string();
    };
    if id.trim().is_empty() {
        return default_instance_id().to_string();
    }
    match load_registry() {
        Ok(registry) if registry.get(&id).is_some() => id,
        // 选择已从本壳注册表里消失（对方默认实例让位 / 实例被删）：回退到
        // 按壳分家的默认值，别让壳停在一个不属于它的实例上。
        Ok(_) => default_instance_id().to_string(),
        // 注册表暂时读不出来：无从证明选择已失效，按用户的选择走。
        Err(_) => id,
    }
}

/// 记住本壳当前服务的实例。**只写壳自己的 settings**，绝不碰注册表里那份
/// 共享的 `default_instance_id`。
pub fn set_current_instance_id(id: &str) -> Result<(), String> {
    let mode = crate::shell::settings::current_mode();
    let mut settings = crate::shell::settings::load_for_shell(mode);
    settings.current_instance_id = Some(id.to_string());
    crate::shell::settings::save_for_shell(mode, &settings)
}

/// 壳当前服务的内核族。setup 期用它把 data_dir 解析到
/// `<xlink_home>/<family>/desktop[-dev]/`，让 mcode 等新内核族将来接入时
/// 天然拿到自己的族目录。
///
/// **解析顺序刻意只走壳自己的状态**，一级都不读共享指针：
/// 1. 本壳当前服务的实例（`settings.current_instance_id`——两个壳各一份）；
/// 2. 本壳按编译模式分家的默认实例（`default_instance_id`：release `default`、
///    dev `default-dev`）；
/// 3. `KERNEL_FAMILY_DSH`。
///
/// 曾经的第一级是注册表里那个**共享**的 `default_instance_id`。它是一份被两个壳
/// 读写的可变字段，而 `data_dir` 是内核安装树（`active.txt` + `kernels/<version>/`
/// 几百 MB）所在的位置——让「本壳的数据目录」取决于「另一个壳上次写了什么」，
/// 是一次典型的跨壳竞态：谁先写谁赢，而写错的一方**不自愈**（认领条件是
/// 「无人认领」，指针被占后永远轮不到它纠正）。UI 上还完全看不见它：列表里的
/// 「默认」高亮读的是 per-shell 的 `current_instance_id`，与本字段无关。
///
/// 现在 `default_instance_id` 只剩**兼容用途**（旧版本构建会读它，见
/// [`ensure_default_registered`] 的修复逻辑），不再是任何决策的输入。要新增
/// 族解析分支时走上面的三级，别把这个字段接回去——`check:invariants` 第 12 项
/// 会拦住。
pub fn default_family() -> String {
    if let Ok(registry) = load_registry() {
        for candidate in [current_instance_id(), default_instance_id().to_string()] {
            if let Some(record) = registry.instances.iter().find(|item| item.id == candidate) {
                return record.kernel_family.clone();
            }
        }
    }
    KERNEL_FAMILY_DSH.to_string()
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
    /// 内核族：`"dsh"` 或未来的 `"mcode"`。决定 [`crate::shell::paths::kernels_root`]
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
    crate::plugins::center::DEFAULT_PROFILE.to_string()
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
/// 注册表级别的写入通过 [`crate::shell::state`] 的原子写入完成；实例锁防止两个
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
        shell::paths::validate_id_component(id)
    }
}

/// 读取注册表：文件不存在返回空注册表（首次启动），损坏返回错误并由调用方
/// 决定是否备份原文件后回退到空。
pub fn load_registry() -> Result<InstanceRegistry, RegistryError> {
    load_registry_for(crate::shell::settings::current_mode())
}

/// 读取**指定壳模式**的注册表。分文件后只有两处需要指定模式：setup 里的拆分
/// 步骤与回收路径（要读另一个壳的那份）。其余一律走 [`load_registry`]。
pub fn load_registry_for(
    mode: crate::shell::paths::ShellMode,
) -> Result<InstanceRegistry, RegistryError> {
    use crate::shell::process::{read_state_file, StateRead};
    let path = crate::shell::paths::instances_registry_file_for(mode);
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

/// 原子写入注册表到**指定路径**。分文件之后只有 [`crate::shell::registry_split`] 的
/// 认领步骤需要指定路径；其余一律走 [`save_registry`]，不要自己拼路径。
pub fn save_registry_to(registry: &InstanceRegistry, path: &Path) -> Result<(), RegistryError> {
    let text = serde_json::to_string_pretty(registry)
        .map_err(|e| RegistryError::Serialize(e.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| RegistryError::Io(e.to_string()))?;
    }
    atomic_write(path, format!("{text}\n").as_bytes())
        .map_err(|e| RegistryError::Io(e.to_string()))?;
    Ok(())
}

/// 原子写入**本壳**的注册表文件（`state/instances.json` 或
/// `state/instances-dev.json`，按壳模式分家，见
/// [`crate::shell::paths::instances_registry_file`]）。
pub fn save_registry(registry: &InstanceRegistry) -> Result<(), RegistryError> {
    save_registry_to(registry, &instances_registry_file())
}

/// 确保默认实例已注册（给 `lib.rs setup()` 直接调的**唯一**入口）。
///
/// 旧版壳首次启动时没走过实例系统，`<xlink_home>/state/instances.json`
/// 不存在。不主动跑这一步，顶部 dropdown 会一直显示「加载中」、
/// PluginsPanel「所有实例」tab 会显示「实例注册表加载失败」。
///
/// 抽成同步函数是为了避开 setup 闭包内的 `spawn_blocking`——setup 是
/// `FnOnce` 同步闭包，必须在主线程里串行做完才能继续启动。
///
/// 曾经还有一个功能完全相同的 Tauri 命令供前端调用，它把 id 写死成
/// `DEFAULT_INSTANCE_ID`；dev 壳一旦走到它，就会往共享注册表里塞一条名为
/// `default`、端口取 dev 的 3091、`kernel_version` 为空的幽灵记录。命令连同
/// 它的权限条目已删除，这里是仅剩的入口。
pub fn ensure_default_registered(data_dir: &Path) -> Result<(), String> {
    let _guard = crate::lock(lifecycle_mutex());
    let mut registry = load_registry().map_err(|e| format!("读取注册表失败：{e}"))?;
    let id = default_instance_id();
    let mut dirty = false;
    if registry.get(id).is_none() {
        let now_ms = crate::shell::process::epoch_millis();
        let settings =
            crate::shell::settings::load_for_shell(crate::shell::settings::current_mode());
        let active = crate::kernel::lifecycle::read_active(data_dir);
        let mut record = InstanceRecord::new(id, KERNEL_FAMILY_DSH, settings.port, now_ms);
        record.kernel_version = active;
        record.label = Some(if id == DEFAULT_INSTANCE_ID {
            "默认实例（迁移自旧版）".to_string()
        } else {
            "默认实例（dev 壳）".to_string()
        });
        ensure_instance_dirs(&record).map_err(|e| format!("准备实例目录失败：{e}"))?;
        save_record_to_disk(&record).map_err(|e| format!("写入实例记录失败：{e}"))?;
        registry
            .add(record)
            .map_err(|e| format!("注册表拒绝该 id：{e}"))?;
        dirty = true;
    }
    if claim_default_pointer(&mut registry) {
        dirty = true;
    }
    if !dirty {
        return Ok(());
    }
    save_registry(&registry).map_err(|e| format!("写入注册表失败：{e}"))
}

/// 实例被删除时维护本壳注册表里的 `default_instance_id`：被删的正是它指向的
/// 那个，就**置空**而不是改指某个兄弟实例。置空是修复（`check:invariants`
/// 第 11 项明确放行），下次启动由 [`claim_default_pointer`] 按本壳默认值重建。
///
/// 抽出来是为了让「谁在维护这个字段」只有 `instance.rs` 一处——
/// `check:invariants` 第 12 项禁止 `instance.rs` 之外**读**它。
pub fn forget_default_pointer(registry: &mut InstanceRegistry, removed_id: &str) {
    if registry.default_instance_id.as_deref() == Some(removed_id) {
        registry.default_instance_id = None;
    }
}

/// 认领**本壳**的默认实例指针，返回是否发生了写入。
///
/// 字段名沿用历史：它曾经是两个壳共享的一份（`state/instances.json`），2026-09-29
/// 起注册表按壳模式分文件（`instances.json` / `instances-dev.json`），它也随之变成
/// **每壳各有一份**。于是原��那套「只有 release 能写」的防御性限制没有存在理由了：
/// 谁写都只写自己那份文件，dev 壳的认领动不了 release 的任何状态。
///
/// 仍然要「指错了就改回来」：分文件之前被写坏的 `default-dev` 会随共享文件一起
/// 被 release 认领进来（拷的是整份），而它与本壳的语义相反。认领按本壳的默认值
/// 覆盖一次即可，顺带把这类历史垃圾清掉。
fn claim_default_pointer(registry: &mut InstanceRegistry) -> bool {
    claim_default_pointer_for(registry, default_instance_id())
}

/// [`claim_default_pointer`] 的可测形态：`shell_id` 由调用方给出。测试进程恒为
/// dev 壳，release 分支在这里拿不到——不抽出来就只能上 release 验这条修复。
fn claim_default_pointer_for(registry: &mut InstanceRegistry, shell_id: &str) -> bool {
    if registry.default_instance_id.as_deref() == Some(shell_id) {
        return false;
    }
    let previous = registry.default_instance_id.clone();
    registry.default_instance_id = Some(shell_id.to_string());
    eprintln!(
        "dsh-xlink: 本壳注册表的 default_instance_id 原为 {previous:?}，已认领为 {shell_id}\
         （该字段按壳模式分文件，dev 壳写不到 release 那份）"
    );
    true
}

// --- 内核 home 的一次性搬迁（~/.dsh → 实例 DSH_HOME）------------------------

/// 内核拥有的 home 子目录：存在即并入实例 home。目标是**合并**而不是整目录
/// rename——实例 home 的骨架（profiles/sessions/…）与外壳接线产物（profile
/// 里的 package.json / node_modules）早已就位，不能被整目录覆盖。
const LEGACY_DSH_HOME_DIRS: &[&str] = &[
    "sessions",
    "storages",
    "attachments",
    "logs",
    "cache",
    "profiles",
    "llm-deepseek",
    "synapse",
];
/// 内核拥有的 home 散文件：会话凭据、内核设置、遥测身份、任务板状态。
const LEGACY_DSH_HOME_FILES: &[&str] = &[
    ".credentials.yaml",
    "settings.yaml",
    "settings.yaml.imported",
    ".anonymous-user-id",
    "cordis.patch.yml",
    "dsh-taskboard.json",
    "dsh-taskboard-templates.json",
];
const DSH_HOME_MIGRATION_SCHEMA_VERSION: u64 = 2;

/// 把官方内核的默认 home（`~/.dsh`）里的用户数据一次性搬进实例 `DSH_HOME`。
///
/// 多内核改造前，内核始终以默认 home 启动（旧启动路径从不注入 `DSH_HOME`），
/// 会话、凭据、profile 都积累在 `~/.dsh`；改造后内核经 `DSH_HOME` 指向实例
/// 目录，不搬迁等于让用户面对一个空工作台。规则：
///
/// - `<home>/.dsh-home-migrated` 保存迁移版本；旧版本标记会触发一次补迁；
/// - 目录项**递归并入**：目标已有的条目以目标为准（多半是外壳接线刚重新
///   物化的产物），两侧都是目录则继续下探，缺失的条目整体移入；`node_modules`
///   不动（接线的 pnpm 产物，按 package.json 重建）。已移动的条目下次启动
///   自动跳过，因此中断后重跑是安全的；
/// - `settings.yaml.imported` 只在活动 `settings.yaml` 缺失时复制为活动设置，
///   原归档保留；
/// - 移动优先同卷 `rename`（原子），失败（跨卷）回退复制后删除；
/// - `~/.dsh` 里**外壳拥有**的旧数据（`desktop/`、`plugins/`、`skills*`）
///   不在清单内，绝不触碰——它们归历史数据迁移面板管。
///
/// `legacy_home` 由调用方传入（生产为 `$HOME/.dsh`，测试传临时目录）：函数
/// 本身可测，也不会在 `cargo test` 里扫到开发机的真实 home。
pub fn migrate_legacy_dsh_home_if_needed(
    family: &str,
    id: &str,
    legacy_home: &Path,
) -> Result<(), String> {
    if family != KERNEL_FAMILY_DSH {
        return Ok(()); // `~/.dsh` 只是 dsh 族的默认 home，其他族没有历史包袱
    }
    let target = shell::paths::instance_dsh_home(family, id);
    let marker = target.join(".dsh-home-migrated");
    let migration_version = fs::read(&marker)
        .ok()
        .and_then(|contents| serde_json::from_slice::<serde_json::Value>(&contents).ok())
        .and_then(|value| {
            value
                .get("schema_version")
                .and_then(serde_json::Value::as_u64)
        })
        .unwrap_or_default();
    if migration_version >= DSH_HOME_MIGRATION_SCHEMA_VERSION {
        return Ok(());
    }
    fs::create_dir_all(&target).map_err(|e| format!("无法创建实例内核目录 {target:?}：{e}"))?;
    if legacy_home.is_dir() {
        for name in LEGACY_DSH_HOME_DIRS {
            merge_move_dir(&legacy_home.join(name), &target.join(name))?;
        }
        for name in LEGACY_DSH_HOME_FILES {
            let src = legacy_home.join(name);
            let dst = target.join(name);
            if src.is_file() && !dst.exists() {
                move_item(&src, &dst)?;
            }
        }
    }
    restore_imported_settings_if_missing(&target)?;
    let marker_content = serde_json::json!({
        "schema_version": DSH_HOME_MIGRATION_SCHEMA_VERSION,
    });
    atomic_write(&marker, format!("{marker_content}\n").as_bytes())
        .map_err(|e| format!("无法写入搬迁标记 {marker:?}：{e}"))?;
    Ok(())
}

/// `settings.yaml.imported` 是内核的导入归档，不会被 settings-file 当作活动配置读取。
/// 已有活动文件始终优先；归档只在活动文件缺失时复制恢复，原件保留供回滚。
fn restore_imported_settings_if_missing(home: &Path) -> Result<(), String> {
    let active = home.join("settings.yaml");
    if active.exists() {
        return Ok(());
    }
    let imported = home.join("settings.yaml.imported");
    if !imported.is_file() {
        return Ok(());
    }
    fs::copy(&imported, &active)
        .map(|_| ())
        .map_err(|error| format!("无法从设置归档恢复内核设置 {imported:?} → {active:?}：{error}"))
}

/// 把 `src` 目录**递归并入** `dst`：`dst` 缺失的条目整体移入，已有的条目
/// 若两侧都是目录则继续下探、否则以目标为准留在源处。实例 home 的骨架
/// （`profiles/web`、`sessions`、`attachments/v1`…）早已就位，不做递归的
/// 话整个子树会被一层「已存在」挡住。`node_modules` 明确不下探也不移动：
/// 那是外壳接线的 pnpm 产物，由 `ensure_wiring` 按 package.json 重建，
/// 遗留的同名树整体留在源目录不动。
fn merge_move_dir(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(src).map_err(|e| format!("无法读取目录 {src:?}：{e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("无法读取目录 {src:?}：{e}"))?;
        if entry.file_name() == "node_modules" {
            continue;
        }
        let child_src = entry.path();
        let child_dst = dst.join(entry.file_name());
        if child_dst.exists() {
            if child_src.is_dir() && child_dst.is_dir() {
                merge_move_dir(&child_src, &child_dst)?;
            }
            continue;
        }
        move_item(&child_src, &child_dst)?;
    }
    // 内容并入后源目录只剩空壳时清掉它，让 `~/.dsh` 里不残留空骨架。
    // 只删空目录：非空说明有条目被目标保留，那是有意留给用户的。
    if fs::read_dir(src)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false)
    {
        let _ = fs::remove_dir(src);
    }
    Ok(())
}

/// 单个条目的移动：优先同卷 `rename`（原子、零拷贝），失败（通常为跨卷）
/// 回退到复制后删除源。
///
/// 移动前先把目标父目录建出来：`rename` 与 `fs::copy` 都不会创建中间目录，
/// 而实例 home 的骨架不保证含有清单子目录——遗留 home 里的散文件
/// （`storages/*.json`、`llm-deepseek/files-v3.json`）落到一个目标侧还不
/// 存在的目录时，rename 与 copy 会先后以 ENOENT 失败，整个搬迁就此中断且
/// 成功标记不落盘（0.2.2 实测）。
fn move_item(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建目录 {parent:?}：{e}"))?;
    }
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        copy_dir_recursive(src, dst)?;
        fs::remove_dir_all(src).map_err(|e| format!("无法删除源目录 {src:?}：{e}"))?;
    } else {
        fs::copy(src, dst).map_err(|e| format!("无法复制 {src:?} → {dst:?}：{e}"))?;
        fs::remove_file(src).map_err(|e| format!("无法删除源文件 {src:?}：{e}"))?;
    }
    Ok(())
}

/// 递归复制目录（std 没有 `copy_dir_all`；只覆盖本搬迁的跨卷回退场景）。
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("无法创建目录 {dst:?}：{e}"))?;
    let entries = fs::read_dir(src).map_err(|e| format!("无法读取目录 {src:?}：{e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("无法读取目录 {src:?}：{e}"))?;
        let child_dst = dst.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|e| format!("无法读取 {src:?}：{e}"))?
            .is_dir()
        {
            copy_dir_recursive(&entry.path(), &child_dst)?;
        } else {
            fs::copy(entry.path(), &child_dst)
                .map_err(|e| format!("无法复制 {:?} → {child_dst:?}：{e}", entry.path()))?;
        }
    }
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

/// 写入单独的 PID 文件。格式依次是 `pid`、`port`、`shell`——`shell` 记的是
/// 启动这个内核的**壳**（`release` / `dev`），用来在两个壳指向同一实例时认出
/// 「这个实例归另一个壳管」。旧格式（只写 pid）保留向后兼容。
pub fn write_pid(family: &str, id: &str, pid: u32, port: u16) -> Result<(), String> {
    let path = instance_pid_file(family, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let shell = crate::shell::settings::current_mode().as_str();
    atomic_write(&path, format!("{pid} {port} {shell}\n").as_bytes()).map_err(|e| e.to_string())
}

/// 读取 PID 文件，兼容只写 pid 的旧格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PidRecord {
    pub pid: u32,
    pub port: Option<u16>,
    /// 启动这个内核的壳；旧文件没有这一段时为 `None`（= 认不出来）。
    pub shell: Option<&'static str>,
}

pub fn read_pid(family: &str, id: &str) -> Option<PidRecord> {
    let path = instance_pid_file(family, id);
    let text = fs::read_to_string(&path).ok()?;
    let mut parts = text.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let port = parts.next().and_then(|value| value.parse::<u16>().ok());
    let shell = parts.next().and_then(known_shell_mode);
    Some(PidRecord { pid, port, shell })
}

fn known_shell_mode(raw: &str) -> Option<&'static str> {
    match raw {
        "release" => Some("release"),
        "dev" => Some("dev"),
        _ => None,
    }
}

/// 这个实例的内核是不是**另一个壳**启动的、且那个进程还活着。
///
/// 用于「改动实例前先问一句」：装/卸插件、切物化模式、切内核版本都会重写
/// 实例的 profile 接线与插件物化，落在另一个壳正跑着的内核上，工作台会当场
/// 崩掉（实测：dev 切内核 → release 工作台白屏）。pid 文件里记了启动方的
/// 壳模式，因此能直接认出主人，而不必靠端口猜。
///
/// 认不出来时（旧格式文件、pid 已退出）返回 `None`——**宁可放行也不误伤**：
/// 误报会挡住用户自己对自己实例的正常操作，而漏报只会在两壳真撞上时少一次提醒。
pub fn instance_owned_by_other_shell(family: &str, id: &str) -> Option<PidRecord> {
    let record = read_pid(family, id)?;
    let owner = record.shell?;
    if owner == crate::shell::settings::current_mode().as_str() {
        return None;
    }
    // 复用内核侧那套「这个 pid 现在还是一个 dsh web 内核吗」的校验（命令行
    // 身份 + 端口活体），而不是裸的进程存在性：pid 会被系统复用，认错了
    // 就把一个无关进程当成「另一个壳正在用」。
    crate::kernel::lifecycle::pid_is_kernel(record.pid, record.port).then_some(record)
}

/// 阻断类错误要说清「谁占着、怎么解」——用户照着做就能继续，而不是只知道自己
/// 被拒了。抽成纯函数是为了能直接测文案要素。
fn blocked_message(record: &PidRecord, action: &str) -> String {
    let port = record
        .port
        .map(|port| format!("、端口 {port}"))
        .unwrap_or_default();
    format!(
        "这个实例正被另一个 dsh-xlink（{} 壳，进程 {}{port}）使用，不能{action}——\
         它的内核还在跑，改接线会当场弄坏那边的工作台。\
         请先在那个壳里停止该实例的内核，或改用别的实例。",
        record.shell.unwrap_or("未知"),
        record.pid,
    )
}

/// 改动实例级状态前的互斥检查：实例正被另一个壳的内核占用时拒绝。
pub fn ensure_instance_mutable(family: &str, id: &str, action: &str) -> Result<(), String> {
    match instance_owned_by_other_shell(family, id) {
        Some(record) => Err(blocked_message(&record, action)),
        None => Ok(()),
    }
}

/// 这个实例的内核是否还在跑——**不管哪个壳拉起的**——并返回活体证据。
///
/// 供「运行期会写进实例 home」的合并类操作用（找回历史会话、恢复配置）：把
/// 状态缓存在内存里的是**目标实例自己的内核**，而用户自建实例在注册表分家后
/// 有意留在两份注册表里，另一个壳完全可能正跑着它。
///
/// 判据是实例 pid 文件 + [`crate::kernel::lifecycle::pid_is_kernel`] 的活体校验（pid 会被
/// 系统复用，裸的「进程存在」会误伤）。端口优先取 pid 文件里记的**启动时刻**
/// 端口——注册表端口可能在内核启动之后又被改过，拿新端口验旧进程会漏检；两处
/// 都给不出端口时放行，与 [`instance_owned_by_other_shell`] 同一取舍。
///
/// pid 文件缺失时退回端口监听者自己证明身份：内核启动那一步写 pid 文件是
/// `let _ =`（写失败不阻断启动，`kernel::start_instance`），只认 pid 文件会在
/// 「内核活着但没留下 pid 记录」时漏检，合并进去的清单会被它下一次落盘覆盖。
pub fn instance_kernel_running(family: &str, id: &str) -> Option<PidRecord> {
    let record_port = load_registry()
        .ok()
        .and_then(|registry| registry.get(id).map(|record| record.port))
        .or_else(|| load_record_from_disk(family, id).map(|record| record.port));
    let Some(record) = read_pid(family, id) else {
        let port = record_port?;
        let listener = crate::kernel::lifecycle::port_listen_pid(port)?;
        return crate::kernel::lifecycle::pid_is_kernel_identity(listener, Some(port)).then_some(
            PidRecord {
                pid: listener,
                port: Some(port),
                // 认不出主人（没有 pid 文件就没有壳段），文案据此指回概览页。
                shell: None,
            },
        );
    };
    let port = record.port.or(record_port)?;
    // 只查身份与命令行里的 `--port`，不反查端口活体：这条判据每 2.5 秒随状态
    // 轮询跑一次（跨壳那一份更是在 `workbench_running_in_other_shell` 里对着
    // 另一个壳的每个实例各跑一遍），而少一条判据只会让它更倾向于"还在跑"，
    // 守卫的误判方向因此始终是"多挡一次用户操作"。理由见
    // `crate::kernel::lifecycle::pid_is_kernel_identity`。
    crate::kernel::lifecycle::pid_is_kernel_identity(record.pid, Some(port)).then_some(PidRecord {
        port: Some(port),
        ..record
    })
}

/// 「实例的内核还在跑」阻断文案：谁在跑、为什么现在不能做、下一步去哪停。
///
/// 抽到共享层是因为 [`crate::migration::home_recovery`] 与 [`crate::diagnostics::restore`] 面对的是同一
/// 条纪律（内核把状态缓存在内存里，运行期写入会被它下一次落盘覆盖），两处各写
/// 一份文案必然漂移。`record` 取自 [`instance_kernel_running`]，端口一定是它
/// 实际校验过的那一个。
pub fn instance_kernel_running_message(record: &PidRecord, id: &str, action: &str) -> String {
    let port_hint = record
        .port
        .map(|port| format!("、端口 {port}"))
        .unwrap_or_default();
    let reason = "内核把状态缓存在内存里，运行期间的改动会被它下一次落盘整个覆盖";
    // `shell: None` = 旧格式 pid 文件或没有 pid 文件，认不出主人。这种情况按
    // 本壳处理：概览页的「关闭工作台」对用户永远是一个可尝试的下一步。
    match record
        .shell
        .filter(|owner| *owner != crate::shell::settings::current_mode().as_str())
    {
        Some(owner) => format!(
            "实例 {id} 正被另一个 dsh-xlink（{owner} 壳，进程 {}{port_hint}）使用，\
             无法{action}：{reason}。请先在那个壳里停止该实例，再回这里重试",
            record.pid,
        ),
        None => format!(
            "实例 {id} 的内核还在运行（进程 {}{port_hint}），无法{action}：{reason}。\
             请先在概览页点「关闭工作台」停止它，再重试",
            record.pid,
        ),
    }
}

/// **另一个壳**（不是本壳）还有哪个工作台在跑。装 / 删内核前把后果说清楚。
///
/// **它现在只提示、不阻断**（[`crate::kernel::lifecycle::warn_other_shell_workbench`] 是唯一
/// 生产调用方，另一个是 `status` 让 UI 在点之前显示）。曾经它是硬拦，理由写的是
/// 「装包是两万个文件的事件风暴，内核的文件监视器会把它当成模块图变更」——**那个
/// 机制查不实**，实测见 [`other_shell_workbench_notice`] 的文档注释。
///
/// **为什么还要问**：两棵安装树物理不相交、插件中央库也按壳分家了，但**pnpm 的
/// 内容寻址 store 是共享的**——两个壳的树与 store 里内容相同的文件是同一个
/// inode。2026-09-30 定案（当天五次装 / 删全部命中）：对面的树在被链接 / 取消
/// 链接时，正在服务的那边会在 4~6 秒内被内核 `dsh-client-hmr` 的 stat 轮询
/// 误判「bundle 重建」打死活页面（机制全文见
/// [`other_shell_workbench_notice`] 的文档注释）。装新版本已用 copy 隔离 inode，
/// 删旧版硬链接树仍可能惊动对面一次（自愈兜底）。
///
/// 候选 = 另一个壳注册表里的全部实例 **加上**它的默认实例。默认实例必须无条件
/// 补上：另一个壳可能从没在这台机器上启动过（注册表文件还不存在），而那台机器
/// 上最可能正在跑的恰恰是它的默认实例。判据复用 [`instance_kernel_running`]
/// （pid 文件 + 端口活体验证），读不出就当没有——**提示**误报的代价是用户以为对面
/// 在跑（多一句警告），**阻断**误报的代价是把用户挡在自己机器上（双壳并行直接
/// 没了），这也是它只提示的一个理由。
pub fn workbench_running_in_other_shell() -> Option<(ShellMode, String, PidRecord)> {
    let mine = crate::shell::settings::current_mode();
    for mode in [ShellMode::Release, ShellMode::Dev] {
        if mode == mine {
            continue;
        }
        for (family, id) in other_shell_candidates(mode) {
            if let Some(record) = instance_kernel_running(&family, &id) {
                return Some((mode, id, record));
            }
        }
    }
    None
}

/// 另一个壳可能正在服务的实例候选（判据见 [`workbench_running_in_other_shell`]）。
fn other_shell_candidates(mode: ShellMode) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Ok(registry) = load_registry_for(mode) {
        for record in &registry.instances {
            let pair = (record.kernel_family.clone(), record.id.clone());
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    let fallback = (
        KERNEL_FAMILY_DSH.to_string(),
        default_instance_id_for(mode).to_string(),
    );
    if !out.contains(&fallback) {
        out.push(fallback);
    }
    out
}

/// 另一个壳的工作台在跑时装 / 删内核**要告诉用户什么**——**不再阻断**。
///
/// 2026-09-30 之前这里是硬拦；当天机制定案（pnpm store 的 inode 共享 + NTFS
/// ChangeTime + 内核 `client-hmr` 的 stat 轮询，全文见模块文档与 AGENTS.md）后，
/// 安装改 `package-import-method=copy` 根治了**装**的路径。这条文案因此只在
/// **真有残余风险**时出现（调用侧已按 [`crate::kernel::install_isolation`] 的采样门控）：
/// 本壳还挂着共享 inode 的旧树、且对面正在服务的那棵树也是共享的。两侧任一
/// 独立，任何装 / 删都物理碰不到对方——那时安静就是正确表达。
pub fn other_shell_workbench_notice(mode: ShellMode, id: &str, record: &PidRecord) -> String {
    let port_hint = record
        .port
        .map(|port| format!("、端口 {port}"))
        .unwrap_or_default();
    format!(
        "另一个 dsh-xlink（{mode} 壳）的工作台正在运行（实例 {id}，进程 {}{port_hint}）。\
         本壳还装有旧版方式安装的内核（文件仍与其他目录共享存储，版本页里带\
         「共享存储」标记的就是）：删除或重装那种版本会短暂惊动对面的页面\
         （它会等风停后自己恢复）。把带标记的版本卸载后重装一次即彻底隔离，\
         这条提示随之消失；期间对面没恢复的话，在那个壳的概览页点\
         「刷新工作台」即可。",
        record.pid,
    )
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
        shell::paths::validate_id_component(id).map_err(|e| e.to_string())?;
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
    shell::paths::validate_id_component(&record.id).map_err(|e| e.to_string())?;
    shell::paths::validate_id_component(&record.kernel_family).map_err(|e| e.to_string())?;
    let path = instance_record_file(&record.kernel_family, &record.id);
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&path, format!("{text}\n").as_bytes()).map_err(|e| e.to_string())
}

/// 准备实例目录：创建 home、workspace、runtime 子目录。已有的不报错。
pub fn ensure_instance_dirs(record: &InstanceRecord) -> Result<(), String> {
    shell::paths::validate_id_component(&record.id).map_err(|e| e.to_string())?;
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
    shell::paths::validate_id_component(&record.id).map_err(|e| e.to_string())?;
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

    /// pid / port / 所属壳写入与读取必须保持一致，包括只有 pid 的旧格式。
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
        assert_eq!(
            pid.shell,
            Some(crate::shell::settings::current_mode().as_str())
        );
        // 旧格式（只写 pid）仍要读得出来，只是认不出主人。
        std::fs::write(instance_pid_file(KERNEL_FAMILY_DSH, "default"), "777\n").expect("写旧格式");
        let old = read_pid(KERNEL_FAMILY_DSH, "default").expect("读旧格式");
        assert_eq!((old.pid, old.port, old.shell), (777, None, None));
        std::fs::remove_dir_all(&home).ok();
    }

    /// 跨壳守卫：pid 文件记着**另一个壳**且那个 pid 还活着时，实例不可改动；
    /// 认不出主人（旧格式）、主人就是自己、进程已退出这三种情况一律放行——
    /// 误报会挡住用户对自己实例的正常操作。
    #[test]
    fn instance_is_mutable_unless_another_shell_owns_it() {
        let home = temp_dir("guard");
        let _xlink = scoped_xlink_home(&home);
        let record = sample_record("default", 3090, KERNEL_FAMILY_DSH);
        ensure_instance_dirs(&record).expect("ensure dirs");
        let pid_path = instance_pid_file(KERNEL_FAMILY_DSH, "default");
        let mine = crate::shell::settings::current_mode().as_str();
        let other = if mine == "dev" { "release" } else { "dev" };

        // 认不出主人（旧格式）→ 放行。
        std::fs::write(&pid_path, "4242 3090\n").expect("写旧格式");
        assert!(ensure_instance_mutable(KERNEL_FAMILY_DSH, "default", "测试").is_ok());

        // 主人是自己 → 放行（自己启的内核当然能改）。
        std::fs::write(&pid_path, format!("4242 3090 {mine}\n")).expect("写自己");
        assert!(ensure_instance_mutable(KERNEL_FAMILY_DSH, "default", "测试").is_ok());

        // 主人是另一个壳，但那个 pid 已经不是 dsh 内核（测试里是本进程，
        // 命令行对不上）→ 放行。这是 `pid_is_kernel` 的身份校验在兜底，
        // 免得把一个无关进程当成"另一个壳正在用"。
        std::fs::write(&pid_path, format!("{} 3090 {other}\n", std::process::id()))
            .expect("写别的壳");
        assert!(ensure_instance_mutable(KERNEL_FAMILY_DSH, "default", "测试").is_ok());

        std::fs::remove_dir_all(&home).ok();
    }

    /// 跨壳提示的文案**不得**变回阻断措辞，而且必须带恢复出路。
    ///
    /// 钉的是「提示不拦」这条策略在**用户看得见的那句话**里的形态。文案是最容易
    /// 漂回去的地方：代码改对了、把 `warn` 又接成一次拒绝之前，往往先有人把
    /// 「不能装 / 请先去关掉」写回文案——那时读代码的人会觉得拦得对。
    #[test]
    fn other_shell_notice_warns_without_blocking_and_says_how_to_recover() {
        let record = PidRecord {
            pid: 37652,
            port: Some(3090),
            shell: Some("release"),
        };
        let notice = other_shell_workbench_notice(ShellMode::Release, "default", &record);

        // 要素齐全：谁占着、哪个壳、实例、进程、端口，以及**指路的标记**
        // （「共享存储」是版本页行上真实存在的标记，文案必须指向它，
        // 用户才知道重装哪一个）。
        for needle in [
            "另一个 dsh-xlink",
            "release 壳",
            "实例 default",
            "进程 37652",
            "端口 3090",
            "共享存储",
            "卸载后重装",
        ] {
            assert!(notice.contains(needle), "提示缺少要素：{needle}｜{notice}");
        }
        // 出路必须写出来——否则用户看到「可能惊动」却不知道怎么办。
        assert!(
            notice.contains("刷新工作台"),
            "提示没告诉用户怎么恢复：{notice}"
        );

        // 阻断措辞一律不许回来。「不能」与「请先…再重试」是硬拦那版的原话。
        for banned in ["不能", "请先在那个壳", "再回来重试", "无法"] {
            assert!(
                !notice.contains(banned),
                "跨壳提示里出现了阻断措辞「{banned}」——它现在是提示不是拦截：{notice}"
            );
        }
    }

    /// 阻断类错误必须说清"谁占着、怎么解"——用户照着做就能继续，
    /// 而不是只知道自己被拒了。钉住的是真实的文案构造函数。
    #[test]
    fn blocked_message_names_the_owner_and_the_way_out() {
        let record = PidRecord {
            pid: 42,
            port: Some(3090),
            shell: Some("release"),
        };
        let message = blocked_message(&record, "切换内核版本");
        for needle in [
            "另一个 dsh-xlink",
            "release 壳",
            "进程 42",
            "端口 3090",
            "切换内核版本",
            "停止该实例的内核",
        ] {
            assert!(message.contains(needle), "消息缺少要素：{needle}");
        }
        // 旧格式文件认不出主人时也要能说清，而不是留下空洞的「未知」占位。
        let unknown = blocked_message(
            &PidRecord {
                pid: 7,
                port: None,
                shell: None,
            },
            "装插件",
        );
        assert!(unknown.contains("进程 7") && unknown.contains("装插件"));
    }

    /// 实例级活体判据：pid 文件不在时改问端口，而不是直接放行。内核启动那
    /// 一步写 pid 是 `let _ =`，写失败照样把内核拉起来了——只认 pid 文件会在
    /// 「内核活着但没留下 pid 记录」时漏检，合并进去的清单会被它下一次落盘
    /// 整个覆盖。正向识别交给 `pid_is_kernel`（它自己已有三层校验的测试），
    /// 这里钉的是**判据的边界**：宁可放行也不误伤。
    #[test]
    fn instance_kernel_running_asks_the_port_when_there_is_no_pid_file() {
        let home = temp_dir("kernel-running");
        let _xlink = scoped_xlink_home(&home);
        // ① 记录与 pid 都没有：无从查证，放行。
        assert!(instance_kernel_running(KERNEL_FAMILY_DSH, "default").is_none());

        // ② 端口被一个**非内核**进程占着：不得当成"这个实例的内核在跑"。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let record = sample_record("default", port, KERNEL_FAMILY_DSH);
        save_record_to_disk(&record).expect("写实例记录");
        assert!(
            instance_kernel_running(KERNEL_FAMILY_DSH, "default").is_none(),
            "端口上的监听者不是 dsh 内核时必须放行，误报会挡住用户的正常操作"
        );

        // ③ pid 文件在、但进程早退了：活体校验挡住，放行。
        write_pid(KERNEL_FAMILY_DSH, "default", std::process::id(), port).expect("写 pid");
        assert!(
            instance_kernel_running(KERNEL_FAMILY_DSH, "default").is_none(),
            "命令行身份对不上时不得认领（pid 会被系统复用）"
        );
        drop(listener);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 阻断文案要按「谁在跑」分流：对方壳拉起的指明去那个壳停；本壳或认不出
    /// 壳的指回概览页。pid + 端口是两个壳并列运行时唯一能对上号的线索。
    #[test]
    fn kernel_running_message_points_at_the_right_shell() {
        let mine = PidRecord {
            pid: 1,
            port: Some(3091),
            shell: Some(crate::shell::settings::current_mode().as_str()),
        };
        let own = instance_kernel_running_message(&mine, "default-dev", "找回历史会话");
        assert!(own.contains("关闭工作台"), "{own}");
        assert!(own.contains("进程 1") && own.contains("端口 3091"), "{own}");
        assert!(!own.contains("另一个"), "{own}");

        // 旧格式 pid 文件（没有壳段）认不出主人，同样指回概览页。
        let unknown = instance_kernel_running_message(
            &PidRecord {
                pid: 1,
                port: None,
                shell: None,
            },
            "default-dev",
            "找回历史会话",
        );
        assert!(unknown.contains("关闭工作台"), "{unknown}");
        assert!(!unknown.contains("另一个"), "{unknown}");

        // 另一个壳拉起的：报出壳、进程、端口，并指明去那个壳里停。
        let other = PidRecord {
            pid: 4321,
            port: Some(3090),
            shell: Some("__other__"),
        };
        let message = instance_kernel_running_message(&other, "default", "恢复配置");
        for needle in [
            "另一个 dsh-xlink",
            "__other__ 壳",
            "4321",
            "3090",
            "那个壳",
            "恢复配置",
        ] {
            assert!(
                message.contains(needle),
                "消息缺少要素：{needle}｜{message}"
            );
        }
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

    /// `resolve_default()` 返回当前 Shell 记住的默认实例元组
    /// (family, id)。锁住 production caller（guard / commands / notify）
    /// 引用的语义：将来 P8 UI 决策把 `InstanceRegistry::default_instance_id`
    /// 接进来时，caller 一处不动即可跟进。
    #[test]
    fn resolve_default_returns_dsh_default_pair() {
        let (family, instance_id) = resolve_default();
        assert_eq!(family, KERNEL_FAMILY_DSH);
        assert_eq!(instance_id, default_instance_id());
        // 元组对齐 caller 现有签名：`kernel_log_spec(family, id)` 、
        // `current_kernel_log_path(data_dir, family, id)` —— 解构即可，
        // 无需结构体中间层。
        let (f, i) = resolve_default();
        assert_eq!((f, i), (KERNEL_FAMILY_DSH, default_instance_id()));
    }

    /// 两个壳必须各有各的默认实例：共用一个实例时，dev 改一次插件接线 /
    /// 换一次内核版本，release 正在跑的工作台就会崩（实测：dev 切内核 →
    /// release 工作台白屏）。`current_mode()` 由 `debug_assertions` 决定，
    /// 测试进程恒为 dev，所以映射抽成 `default_instance_id_for` 单独测。
    #[test]
    fn default_instance_is_scoped_per_shell() {
        use crate::shell::paths::ShellMode;
        assert_eq!(
            default_instance_id_for(ShellMode::Release),
            DEFAULT_INSTANCE_ID
        );
        assert_eq!(
            default_instance_id_for(ShellMode::Dev),
            DEV_DEFAULT_INSTANCE_ID
        );
        // 两种 id 都要能当路径段用（否则落盘时才炸）。
        for id in [DEFAULT_INSTANCE_ID, DEV_DEFAULT_INSTANCE_ID] {
            crate::shell::paths::validate_id_component(id).expect("实例 id 必须合法");
        }
    }

    /// 历史数据只搬进 release 实例：dev 壳先跑也不能把 `~/.dsh` 搬进自己的
    /// 实例，否则 release 用户打开工作台看到的是空的。
    #[test]
    fn legacy_migration_always_targets_the_release_instance() {
        assert_eq!(
            legacy_migration_target(),
            (KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID)
        );
    }

    /// dev 壳首次启动要建**自己**的实例，并写**自己的**注册表文件
    ///（`state/instances-dev.json`）。release 那份一个字节都不许动。
    ///
    /// 旧版这里断言的是「dev 不得改写共享的 `default_instance_id`」——那是
    /// 两个壳共用一个文件时的约束。注册表按壳模式分文件（`registry_split`）
    /// 之后约束从「不许写」变成「写不到」：dev 写的是另一个文件。这条测试现在
    /// 钉的是更强的事实——**release 那份文件的内容逐字节不变**。
    #[test]
    fn dev_shell_writes_only_its_own_registry_file() {
        let home = temp_dir("ensure-default");
        let _xlink = scoped_xlink_home(&home);
        let data_dir = home.join("shell-data");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        std::fs::write(data_dir.join("active.txt"), "0.1.7-rc.2\n").expect("active");

        // 先替 release 在**它自己那份**文件里建好实例并认领指针。
        let mut registry = load_or_migrate(None, 3090);
        let mut release = sample_record(DEFAULT_INSTANCE_ID, 3090, KERNEL_FAMILY_DSH);
        release.kernel_version = Some("0.1.7-rc.2".to_string());
        registry.add(release).expect("add release");
        registry.default_instance_id = Some(DEFAULT_INSTANCE_ID.to_string());
        let release_file = crate::shell::paths::instances_registry_file_for(ShellMode::Release);
        crate::shell::instance::save_registry_to(&registry, &release_file).expect("seed release");

        ensure_default_registered(&data_dir).expect("ensure current default");

        let after = load_registry().expect("load registry");
        assert!(
            after.get(default_instance_id()).is_some(),
            "壳必须建出当前模式自己的实例（当前模式 = {}）",
            default_instance_id()
        );
        assert_eq!(
            after.default_instance_id.as_deref(),
            Some(DEV_DEFAULT_INSTANCE_ID),
            "本壳文件里的指针指向本壳自己的默认实例"
        );
        let release_registry = crate::shell::instance::load_registry_for(ShellMode::Release)
            .expect("load release file");
        assert!(
            release_registry.get(DEV_DEFAULT_INSTANCE_ID).is_none(),
            "dev 壳不得把自己的实例写进 release 的注册表文件"
        );
        assert_eq!(
            release_registry.default_instance_id.as_deref(),
            Some(DEFAULT_INSTANCE_ID),
            "release 的指针不受 dev 壳影响"
        );
        // 幂等：再跑一次不报错也不重复建。
        ensure_default_registered(&data_dir).expect("第二次仍然幂等");
        assert_eq!(
            load_registry().expect("load").instances.len(),
            after.instances.len()
        );
        std::fs::remove_dir_all(&home).ok();
    }

    // --- 壳内「当前实例」的选择：只落在壳自己的 settings 里 ------------------

    /// 用户切实例**不得**改写注册表里那份共享的 `default_instance_id`。
    ///
    /// 这条曾经真的坏过：`commands::set_default_instance` 直接
    /// `registry.default_instance_id = Some(id)`，dev 壳在顶部点一下页签就把
    /// 共享指针永久改成 `default-dev`。它不可自愈——`ensure_default_registered`
    /// 的认领条件是「无人认领」，指针一旦被改就再也轮不到它纠正，只能手改
    /// `<xlink_home>/state/instances.json`。而这个字段是
    /// [`default_family`] 解析 `data_dir` 的输入，等于让一个壳改掉另一个壳的
    /// 数据目录。
    #[test]
    fn selecting_an_instance_never_touches_the_shared_registry_default() {
        let home = temp_dir("current-instance");
        let _xlink = scoped_xlink_home(&home);
        // release 已认领共享指针；同时预置两个实例，供「切换」挑一个非默认的。
        let mut registry = load_or_migrate(None, 3090);
        registry
            .add(sample_record(DEFAULT_INSTANCE_ID, 3090, KERNEL_FAMILY_DSH))
            .expect("add default");
        registry
            .add(sample_record("work", 3100, KERNEL_FAMILY_DSH))
            .expect("add work");
        registry.default_instance_id = Some(DEFAULT_INSTANCE_ID.to_string());
        save_registry(&registry).expect("seed registry");

        // 未选过时回退到按壳分家的默认值（测试进程恒为 dev 壳）。
        assert_eq!(current_instance_id(), DEV_DEFAULT_INSTANCE_ID);

        set_current_instance_id("work").expect("select work");
        assert_eq!(current_instance_id(), "work", "壳内选择必须被记住");

        let after = load_registry().expect("load registry");
        assert_eq!(
            after.default_instance_id.as_deref(),
            Some(DEFAULT_INSTANCE_ID),
            "壳内选择绝不写共享指针：它只属于壳自己的 settings.json"
        );
        assert_eq!(after.instances.len(), 2, "也不该顺手增删实例条目");

        // 清掉选择后回到按壳分家的默认值，而不是停在被删掉的实例上。
        let mode = crate::shell::settings::current_mode();
        let mut settings = crate::shell::settings::load_for_shell(mode);
        settings.current_instance_id = None;
        crate::shell::settings::save_for_shell(mode, &settings).expect("clear selection");
        assert_eq!(current_instance_id(), DEV_DEFAULT_INSTANCE_ID);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 空串与纯空白的选择按「没选过」处理：它们会一路传到
    /// `default_family()` 之后把 `data_dir` 解析到一个不存在的实例上。
    #[test]
    fn a_blank_selection_falls_back_to_the_shell_default() {
        let home = temp_dir("blank-selection");
        let _xlink = scoped_xlink_home(&home);
        set_current_instance_id("   ").expect("write blank");
        assert_eq!(
            current_instance_id(),
            DEV_DEFAULT_INSTANCE_ID,
            "空白选择必须回退，不能被当成一个真实实例 id"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 壳内选择指向一个**已不在本壳注册表里**的实例时必须回退：那个 id 多半
    /// 是分家让位掉的对方默认实例（registry_split 之前两边共用一份文件，
    /// release 壳确实可能选中过 `default-dev` 并存进自己的 settings），照走会把
    /// 「找回历史会话」回收到另一个壳的实例里。注册表读不出来时则保留选择——
    /// 读失败不等于实例不存在，静默丢掉用户的显式选择更糟。
    #[test]
    fn a_selection_missing_from_the_registry_falls_back_to_the_shell_default() {
        let home = temp_dir("stale-selection");
        let _xlink = scoped_xlink_home(&home);
        // 种一份只含本壳默认实例的注册表；选择指向 release 的默认实例。
        let mut registry = InstanceRegistry::default();
        registry
            .add(sample_record(
                DEV_DEFAULT_INSTANCE_ID,
                3091,
                KERNEL_FAMILY_DSH,
            ))
            .expect("add dev default");
        save_registry(&registry).expect("seed registry");

        set_current_instance_id(DEFAULT_INSTANCE_ID).expect("select release default");
        assert_eq!(
            current_instance_id(),
            DEV_DEFAULT_INSTANCE_ID,
            "选择已从本壳注册表消失，必须回退本壳默认，而不是照 settings 里的陈旧值走"
        );

        // 选择重新出现在注册表里时照常生效。
        let mut registry = load_registry().expect("load");
        registry
            .add(sample_record("work", 3100, KERNEL_FAMILY_DSH))
            .expect("add work");
        save_registry(&registry).expect("save");
        set_current_instance_id("work").expect("select work");
        assert_eq!(current_instance_id(), "work");

        // 注册表损坏（读不出来）时保留选择。
        std::fs::write(
            crate::shell::paths::instances_registry_file(),
            "{ not valid json",
        )
        .expect("corrupt registry");
        assert_eq!(
            current_instance_id(),
            "work",
            "注册表读不出来时按用户的选择走，静默丢掉显式选择更糟"
        );

        std::fs::remove_dir_all(&home).ok();
    }

    // --- 族解析不得读共享指针 ------------------------------------------------

    /// [`default_family`] 解析的是**本壳**的数据目录，因此它的输入只能是本壳
    /// 自己的状态。共享指针 `default_instance_id` 被写成什么、乃至指向一个
    /// 根本不存在的实例，都不许影响结果——那正是 2026-09-28 那次跨壳干扰的形状
    /// （`data_dir` 装的是内核安装树，解析错了等于「内核不见了」且无任何报错）。
    #[test]
    fn default_family_ignores_the_shared_registry_pointer() {
        let home = temp_dir("family-ignores-pointer");
        let _xlink = scoped_xlink_home(&home);
        // 指针指向 dev 的实例；本壳（测试进程恒为 dev）服务的是自己的实例。
        let mut registry = load_or_migrate(None, 3091);
        registry
            .add(sample_record(
                DEFAULT_INSTANCE_ID,
                3090,
                KERNEL_FAMILY_MCODE,
            ))
            .expect("add release");
        registry
            .add(sample_record(
                DEV_DEFAULT_INSTANCE_ID,
                3091,
                KERNEL_FAMILY_DSH,
            ))
            .expect("add dev");
        registry.default_instance_id = Some(DEV_DEFAULT_INSTANCE_ID.to_string());
        save_registry(&registry).expect("seed");

        assert_eq!(
            default_family(),
            KERNEL_FAMILY_DSH,
            "本壳的族必须由本壳的实例决定，而不是共享指针指向谁"
        );

        // 指针指向一个不存在的实例时同样不影响——旧版本留下的垃圾值不该有威力。
        let mut registry = load_registry().expect("load");
        registry.default_instance_id = Some("gone-after-delete".to_string());
        save_registry(&registry).expect("seed bogus");
        assert_eq!(default_family(), KERNEL_FAMILY_DSH);

        std::fs::remove_dir_all(&home).ok();
    }

    /// 每个壳在自己的注册表文件里认领**自己**的默认实例：分文件之前 dev 壳被
    /// 禁止写这个字段（那时它写的是别人的文件），分文件之后谁写都只影响自己，
    /// 那条限制随之撤销。顺带把历史写坏的值（对方的指针留在本壳这份文件里）
    /// 改回本壳的默认值。
    #[test]
    fn each_shell_claims_its_own_default_in_its_own_file() {
        let home = temp_dir("pointer-claim");
        let _xlink = scoped_xlink_home(&home);
        let mut registry = load_or_migrate(None, 3090);
        // 历史垃圾：dev 那份文件里留着 release 写下的指针。
        registry.default_instance_id = Some(DEFAULT_INSTANCE_ID.to_string());

        // dev 壳：认领自己的，写的是 `instances-dev.json`。
        assert!(
            claim_default_pointer_for(&mut registry, DEV_DEFAULT_INSTANCE_ID),
            "认领 dev 自己的默认实例（把对方的指针顶掉）"
        );
        assert_eq!(
            registry.default_instance_id.as_deref(),
            Some(DEV_DEFAULT_INSTANCE_ID)
        );
        // release 壳：把指错的值改回自己的。
        assert!(
            claim_default_pointer_for(&mut registry, DEFAULT_INSTANCE_ID),
            "release 必须把指错的指针改回来"
        );
        assert_eq!(
            registry.default_instance_id.as_deref(),
            Some(DEFAULT_INSTANCE_ID)
        );
        assert!(
            !claim_default_pointer_for(&mut registry, DEFAULT_INSTANCE_ID),
            "已经正确时不该反复写盘"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    // --- 内核 home 搬迁 -----------------------------------------------------

    /// 遗留 `~/.dsh` 里的内核数据必须整体进入实例 home，且搬迁幂等：
    /// 第二次调用是零操作，不再移动任何东西。
    #[test]
    fn legacy_home_migration_moves_user_data_once() {
        let home = temp_dir("home-migrate");
        let _xlink = scoped_xlink_home(&home);
        let legacy = home.join("legacy-dsh");
        fs::create_dir_all(legacy.join("profiles/web")).unwrap();
        fs::create_dir_all(legacy.join("sessions/2026-09")).unwrap();
        fs::write(legacy.join("profiles/web/cordis.yml"), "[]").unwrap();
        fs::write(legacy.join("sessions/2026-09/a.jsonl"), "{}").unwrap();
        fs::write(legacy.join(".credentials.yaml"), "token: x").unwrap();

        migrate_legacy_dsh_home_if_needed(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID, &legacy)
            .expect("first migration");

        let target = shell::paths::instance_dsh_home(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID);
        assert_eq!(
            fs::read_to_string(target.join("profiles/web/cordis.yml")).unwrap(),
            "[]"
        );
        assert_eq!(
            fs::read_to_string(target.join("sessions/2026-09/a.jsonl")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(target.join(".credentials.yaml")).unwrap(),
            "token: x"
        );
        assert!(
            !legacy.join("sessions").exists(),
            "整体移动后源目录不再保留"
        );
        assert!(!legacy.join(".credentials.yaml").exists());
        assert!(
            target.join(".dsh-home-migrated").exists(),
            "成功标记必须落盘"
        );

        // 幂等：标记存在后，即使遗留目录再出现新内容也不再搬。
        fs::write(legacy.join("stray.txt"), "later").unwrap();
        migrate_legacy_dsh_home_if_needed(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID, &legacy)
            .expect("second migration");
        assert!(!target.join("stray.txt").exists());
        std::fs::remove_dir_all(&home).ok();
    }

    /// 实例 home 已有条目（外壳接线刚物化的骨架/产物）时逐层并入：
    /// 目标已有的一律保留，缺失的移入。中断续跑即依赖这条规则。
    #[test]
    fn legacy_home_migration_merges_without_overwriting_targets() {
        let home = temp_dir("home-merge");
        let _xlink = scoped_xlink_home(&home);
        let legacy = home.join("legacy-dsh");
        fs::create_dir_all(legacy.join("profiles/web")).unwrap();
        fs::create_dir_all(legacy.join("sessions")).unwrap();
        fs::write(
            legacy.join("profiles/web/package.json"),
            "{\"legacy\":true}",
        )
        .unwrap();
        fs::write(legacy.join("profiles/web/cordis.yml"), "[]").unwrap();
        fs::write(legacy.join("sessions/old.jsonl"), "legacy").unwrap();

        // 目标先行存在：外壳接线写过的 package.json 与会话。
        let record = sample_record(DEFAULT_INSTANCE_ID, 3090, KERNEL_FAMILY_DSH);
        ensure_instance_dirs(&record).unwrap();
        let target = shell::paths::instance_dsh_home(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID);
        let wired_profile = target.join("profiles/web");
        fs::create_dir_all(&wired_profile).unwrap();
        fs::write(wired_profile.join("package.json"), "{\"wired\":true}").unwrap();
        fs::create_dir_all(target.join("sessions")).unwrap();
        fs::write(target.join("sessions/old.jsonl"), "wired").unwrap();

        migrate_legacy_dsh_home_if_needed(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID, &legacy)
            .expect("migration");

        // 目标已有的：原样保留。
        assert_eq!(
            fs::read_to_string(wired_profile.join("package.json")).unwrap(),
            "{\"wired\":true}",
            "接线产物不得被遗留目录覆盖"
        );
        assert_eq!(
            fs::read_to_string(target.join("sessions/old.jsonl")).unwrap(),
            "wired",
            "目标已有的会话不得被同名遗留会话覆盖"
        );
        // 目标缺失的：移入。
        assert_eq!(
            fs::read_to_string(wired_profile.join("cordis.yml")).unwrap(),
            "[]"
        );
        // 源里被跳过的条目原样保留（不删用户数据）。
        assert_eq!(
            fs::read_to_string(legacy.join("profiles/web/package.json")).unwrap(),
            "{\"legacy\":true}"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 非 dsh 族没有 `~/.dsh` 历史包袱：调用必须是零操作（不建目录、不写标记）。
    #[test]
    fn migration_is_noop_for_other_families() {
        let home = temp_dir("home-mcode");
        let _xlink = scoped_xlink_home(&home);
        let legacy = home.join("legacy-dsh");
        fs::create_dir_all(&legacy).unwrap();
        migrate_legacy_dsh_home_if_needed(KERNEL_FAMILY_MCODE, "default", &legacy)
            .expect("noop for mcode");
        assert!(
            !shell::paths::instance_dsh_home(KERNEL_FAMILY_MCODE, "default").exists(),
            "mcode 实例不得被建出 dsh 族的搬迁产物"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 散文件落入目标侧还不存在的子目录必须成功：`rename` / `fs::copy` 都
    /// 不会创建中间目录，`move_item` 必须先把父目录建出来，否则整个搬迁
    /// 以 ENOENT 中断、成功标记不落盘（0.2.2 实测卡死在
    /// `storages/*.json` / `llm-deepseek/files-v3.json`）。
    #[test]
    fn legacy_home_migration_creates_missing_target_parents() {
        let home = temp_dir("home-nested-file");
        let _xlink = scoped_xlink_home(&home);
        let legacy = home.join("legacy-dsh");
        fs::create_dir_all(legacy.join("storages")).unwrap();
        fs::create_dir_all(legacy.join("llm-deepseek")).unwrap();
        fs::write(legacy.join("storages/workspace.json"), "{}").unwrap();
        fs::write(legacy.join("llm-deepseek/files-v3.json"), "[]").unwrap();

        migrate_legacy_dsh_home_if_needed(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID, &legacy)
            .expect("migration 必须在全新 home 上一次走完");

        let target = shell::paths::instance_dsh_home(KERNEL_FAMILY_DSH, DEFAULT_INSTANCE_ID);
        assert_eq!(
            fs::read_to_string(target.join("storages/workspace.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(target.join("llm-deepseek/files-v3.json")).unwrap(),
            "[]"
        );
        assert!(target.join(".dsh-home-migrated").exists(), "标记必须落盘");
        assert!(!legacy.join("llm-deepseek").exists(), "搬空的源目录应清理");
        std::fs::remove_dir_all(&home).ok();
    }
}
