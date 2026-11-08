//! MiniMax 工作台的按壳配置与受控子进程。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::kernel::kernel_adapter::{
    mcode_node_supported, McodeAdapter, McodeLaunchConfig, McodeProcess,
};
use crate::kernel::lifecycle;
use crate::shell::{instance, paths, process, settings};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    instance_id: String,
    configured_package_path: Option<PathBuf>,
    configured_node_path: Option<PathBuf>,
    /// 配置的 Node 版本：保存时校验过并存下来，状态轮询直接读它，不必再派生
    /// `node --version`。老配置没有这一项，按缺省处理（读取时兜底探测一次）。
    #[serde(default)]
    configured_node_version: Option<String>,
    port: u16,
    miniapp_port: u16,
    /// 共享技能库是否接入 MiniMax。老配置没有这一项，按接入处理（保持原行为）。
    #[serde(default = "link_skills_default")]
    link_skills: bool,
}

fn link_skills_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct McodeStatus {
    pub installed: bool,
    pub running: bool,
    pub stop_required: bool,
    pub window_open: bool,
    pub instance_id: Option<String>,
    pub configured_package_path: Option<PathBuf>,
    pub configured_node_path: Option<PathBuf>,
    pub package_path: Option<PathBuf>,
    pub node_path: Option<PathBuf>,
    pub package_version: Option<String>,
    #[serde(rename = "nodeVersion")]
    pub node_version: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub port: Option<u16>,
    pub miniapp_port: Option<u16>,
    pub error: Option<String>,
    /// 旧运行记录里的 PID（无法证明它的后代已退出）。
    pub stale_pid: Option<u32>,
    /// 绑定实例、端口和两份记录的确认快照。
    pub recovery_snapshot: Option<String>,
    pub skills_state: Option<String>,
    pub skills_error: Option<String>,
    /// 共享技能库是否接入 MiniMax（配置值；下次启动生效）。
    pub skills_linked: Option<bool>,
    pub memory_state: Option<String>,
    pub memory_error: Option<String>,
    pub ponytail_enabled: Option<bool>,
    pub ponytail_error: Option<String>,
    pub memory_profile: String,
}

struct Running {
    process: McodeProcess,
    config: Config,
    ready: bool,
    stopped: bool,
    exited: bool,
    skills_state: String,
    skills_error: Option<String>,
    memory_state: String,
    memory_error: Option<String>,
    memory_bridge: Option<crate::plugins::builtin::mcode_memory::McodeMemoryBridge>,
    skill_token: Option<crate::skills::mcode::StartToken>,
}

fn owns_ports(active: &Running) -> bool {
    match (
        lifecycle::port_listen_pid(active.config.port),
        lifecycle::port_listen_pid(active.config.miniapp_port),
    ) {
        (Some(first), Some(second)) if first == second => active.process.owns_pid(first),
        _ => false,
    }
}

fn running() -> &'static Mutex<Option<Running>> {
    static RUNNING: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(None))
}

static START_GENERATION: AtomicU64 = AtomicU64::new(0);

// The running lock serializes the final generation check and publication of a
// child with stop(), without holding it during dependency checks or readiness.
fn with_current_start<T>(
    generation: u64,
    f: impl FnOnce(&mut Option<Running>) -> T,
) -> Result<T, String> {
    let mut active = crate::lock(running());
    if START_GENERATION.load(Ordering::SeqCst) != generation {
        return Err("MiniMax 启动已取消".into());
    }
    Ok(f(&mut active))
}

fn start_lock() -> &'static Mutex<()> {
    static START: OnceLock<Mutex<()>> = OnceLock::new();
    START.get_or_init(|| Mutex::new(()))
}

fn finish_skill_reservation(
    config: &Config,
    token: &crate::skills::mcode::StartToken,
    evidence: crate::skills::mcode::StopEvidence,
) -> Result<(), String> {
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    let lease = crate::skills::mcode::acquire_lease(&home).map_err(|error| error.to_string())?;
    crate::skills::mcode::finish_start(&lease, token, evidence).map_err(|error| error.to_string())
}

fn stop_memory_bridge(active: &mut Running) -> Result<(), String> {
    if let Some(bridge) = active.memory_bridge.as_mut() {
        bridge
            .stop()
            .map_err(|error| format!("无法确认 MiniMax 记忆桥已停止：{error}"))?;
    }
    drop(active.memory_bridge.take());
    Ok(())
}

fn stop_owned_process(active: &mut Running, acknowledge_forced: bool) -> Result<(), String> {
    match active.process.stop(acknowledge_forced) {
        Ok(()) => {
            active.stopped = true;
            stop_memory_bridge(active)
        }
        Err(error) => {
            if active.process.owned_tree_empty().unwrap_or(false) {
                if let Err(bridge_error) = stop_memory_bridge(active) {
                    return Err(format!("{error}；{bridge_error}"));
                }
            }
            Err(error)
        }
    }
}

fn config_path() -> PathBuf {
    paths::shell_dir(settings::current_mode()).join("mcode.json")
}

#[derive(Deserialize, Serialize)]
struct PonytailPreference {
    version: u32,
    enabled: bool,
}

fn ponytail_path(config: &Config) -> PathBuf {
    paths::instance_dir(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
        .join("runtime/xlink-ponytail.json")
}

fn ponytail_owner_present(home: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(home.join(".xlink-ponytail-owner.json")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("无法检查 Ponytail 归属：{error}")),
    }
}

fn ponytail_preference(config: &Config) -> Result<Option<bool>, String> {
    match process::read_state_file(&ponytail_path(config)) {
        process::StateRead::Missing => Ok(None),
        process::StateRead::Loaded(value) => {
            let value: PonytailPreference = value;
            if value.version != 1 {
                return Err("MiniMax Ponytail 设置版本不受支持".into());
            }
            Ok(Some(value.enabled))
        }
        process::StateRead::Corrupt { reason } => {
            Err(format!("MiniMax Ponytail 设置损坏：{reason}"))
        }
    }
}

fn save_ponytail_preference(config: &Config, enabled: bool) -> Result<(), String> {
    let path = ponytail_path(config);
    std::fs::create_dir_all(path.parent().ok_or("MiniMax 插件设置目录无效")?)
        .map_err(|error| error.to_string())?;
    let value = serde_json::to_vec(&PonytailPreference {
        version: 1,
        enabled,
    })
    .map_err(|error| error.to_string())?;
    crate::shell::atomic_write::atomic_write_private(&path, &value)
        .map_err(|error| format!("无法保存 MiniMax Ponytail 设置：{error}"))
}

fn load_config() -> Result<Option<Config>, String> {
    match process::read_state_file(&config_path()) {
        process::StateRead::Loaded(config) => {
            let config: Config = config;
            paths::validate_id_component(&config.instance_id).map_err(str::to_owned)?;
            if config.port == 0 || config.miniapp_port == 0 || config.port == config.miniapp_port {
                return Err("MiniMax 设置中的端口无效".into());
            }
            Ok(Some(config))
        }
        process::StateRead::Missing => Ok(None),
        process::StateRead::Corrupt { reason } => Err(format!(
            "MiniMax 设置损坏：{reason}。请查看日志并恢复设置文件"
        )),
    }
}

/// Read the current shell's configured and registered MiniMax identity without
/// creating an instance or inspecting the running process.
pub(crate) fn configured_instance_id() -> Result<Option<String>, String> {
    let Some(config) = load_config()? else {
        return Ok(None);
    };
    let _guard = crate::lock(instance::lifecycle_mutex());
    let registry = instance::load_registry().map_err(|error| error.to_string())?;
    let record = registry
        .get(&config.instance_id)
        .ok_or("MiniMax 实例未登记，请检查工作台设置")?;
    let disk = instance::load_record_from_disk(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
        .ok_or("MiniMax 实例记录缺失或损坏，请检查工作台设置")?;
    if record.id != config.instance_id
        || record.kernel_family != instance::KERNEL_FAMILY_MCODE
        || record.port != config.port
        || disk.id != config.instance_id
        || disk.kernel_family != instance::KERNEL_FAMILY_MCODE
        || disk.port != config.port
    {
        return Err("MiniMax 实例记录与设置不一致，请检查工作台设置".into());
    }
    Ok(Some(config.instance_id))
}

fn save_config(config: &Config) -> Result<(), String> {
    let path = config_path();
    std::fs::create_dir_all(path.parent().ok_or("MiniMax 设置目录无效")?)
        .map_err(|error| format!("创建 MiniMax 设置目录失败：{error}"))?;
    let value = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    process::atomic_write(&path, format!("{value}\n").as_bytes())
        .map_err(|error| format!("保存 MiniMax 设置失败：{error}"))
}

/// 端口是否空着。判据与全仓一致：`lifecycle::port_open`（有没有人在监听）。
///
/// 与 DSH 内核自己的启动判据同一条（`lifecycle.rs` 的启动路径也问 `port_open`）：
/// 两套判据分家时，同一个端口在两个内核族里会分别算出「空」与「不空」。
///
/// 不用 bind 判：bind 问的是「我能不能绑上去」，而且会在轮询路径上短暂占住
/// 端口——那本身就是副作用。
fn port_available(port: u16) -> bool {
    port != 0 && !crate::kernel::lifecycle::port_open(port)
}

/// 用户可见的错误里不混 Rust 的 Debug 形态（`Err("...")`）。
fn describe_outcome(result: &Result<(), String>) -> String {
    match result {
        Ok(()) => "已完成".to_string(),
        Err(reason) => reason.clone(),
    }
}

fn allocate_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("分配本机端口失败：{error}"))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| error.to_string())
}

fn new_config() -> Result<Config, String> {
    let port = allocate_port()?;
    let mut miniapp_port = allocate_port()?;
    while miniapp_port == port {
        miniapp_port = allocate_port()?;
    }
    Ok(Config {
        instance_id: format!(
            "mcode-{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(rand::random::<u128>().to_be_bytes())
        ),
        configured_package_path: None,
        configured_node_path: None,
        configured_node_version: None,
        port,
        miniapp_port,
        link_skills: true,
    })
}

fn ensure_registered(config: &Config, fresh: bool) -> Result<(), String> {
    paths::validate_id_component(&config.instance_id).map_err(str::to_owned)?;
    let _guard = crate::lock(instance::lifecycle_mutex());
    let mut registry = instance::load_registry().map_err(|error| error.to_string())?;
    if let Some(existing) = registry.get(&config.instance_id) {
        if existing.kernel_family != instance::KERNEL_FAMILY_MCODE || existing.port != config.port {
            return Err("MiniMax 实例记录与设置不一致，请查看日志后恢复设置".into());
        }
        return Ok(());
    }
    let mut record = instance::InstanceRecord::new(
        &config.instance_id,
        instance::KERNEL_FAMILY_MCODE,
        config.port,
        process::epoch_millis(),
    );
    record.kernel_version = resolve_package(Some(config))
        .as_deref()
        .and_then(crate::kernel::mcode_install::package_version);
    if fresh {
        crate::skills::mcode::enroll_fresh(&config.instance_id)
            .map_err(|error| error.to_string())?;
    }
    instance::ensure_instance_dirs(&record).map_err(|error| error.to_string())?;
    instance::save_record_to_disk(&record)?;
    registry.add(record).map_err(str::to_owned)?;
    instance::save_registry(&registry).map_err(|error| error.to_string())
}

fn stale_pid(config: &Config) -> Result<Option<u32>, String> {
    let path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    if read_recovery_file(&path)?.is_none() {
        return Ok(None);
    }
    let record = instance::read_pid(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
        .ok_or_else(|| {
            format!(
                "MiniMax 进程记录损坏，已保留：{}。请检查记录并查看日志",
                path.display()
            )
        })?;
    // The shell no longer holds the Job/Child here. Even a definitely absent
    // launcher PID cannot prove that all descendants have exited.
    Ok(Some(record.pid))
}

fn read_recovery_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "无法检查 MiniMax 运行记录 {}：{error}",
            path.display()
        )),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            std::fs::read(path)
                .map(Some)
                .map_err(|error| format!("无法读取 MiniMax 运行记录 {}：{error}", path.display()))
        }
        Ok(_) => Err(format!(
            "MiniMax 运行记录不是普通文件，请检查：{}",
            path.display()
        )),
    }
}

fn recovery_snapshot(config: &Config) -> Result<Option<String>, String> {
    let pid = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    let marker = crate::skills::mcode::running_path(&home);
    let pid = read_recovery_file(&pid)?;
    let marker = read_recovery_file(&marker)?;
    if pid.is_none() && marker.is_none() {
        return Ok(None);
    }
    let value = serde_json::to_vec(&(
        &config.instance_id,
        config.port,
        config.miniapp_port,
        pid,
        marker,
    ))
    .map_err(|error| error.to_string())?;
    Ok(Some(crate::shell::digest::hex(Sha256::digest(value))))
}

fn offline_mutation_lease(
    config: &Config,
) -> Result<Option<crate::skills::mcode::InstanceLease>, String> {
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    if !crate::skills::mcode::enrolled(&home) {
        return Ok(None);
    }
    let lease = crate::skills::mcode::acquire_lease(&home).map_err(|error| error.to_string())?;
    crate::skills::mcode::offline_home(&lease).map_err(|error| {
        format!("MiniMax 留有运行占位或旧记录，请先重启电脑并在概览页确认清理：{error}")
    })?;
    Ok(Some(lease))
}

/// 用户确认已重启电脑后，只移除确认时看到的旧记录和运行占位。
pub fn discard_run_record(
    instance_id: &str,
    expected_snapshot: &str,
) -> Result<Option<u32>, String> {
    let _start = start_lock()
        .try_lock()
        .map_err(|_| "MiniMax 正在启动或修改配置，请稍后重试".to_string())?;
    let _install = crate::kernel::mcode_install::install_lock()?;
    let active = crate::lock(running());
    if active.is_some() {
        return Err("MiniMax 工作台正在运行，请先停止".into());
    }
    let config = load_config()?.ok_or("MiniMax 实例不存在，请刷新后重试")?;
    if config.instance_id != instance_id {
        return Err("MiniMax 实例已改变，请刷新后重新确认".into());
    }
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, instance_id);
    let lease = if crate::skills::mcode::enrolled(&home) {
        Some(crate::skills::mcode::acquire_lease(&home).map_err(|error| error.to_string())?)
    } else {
        None
    };
    if recovery_snapshot(&config)?.as_deref() != Some(expected_snapshot) {
        return Err("MiniMax 运行记录已改变，请刷新后重新确认".into());
    }
    if !port_available(config.port) || !port_available(config.miniapp_port) {
        return Err(format!(
            "MiniMax 端口 {} 或 {} 仍被占用，请核查归属后再重试",
            config.port, config.miniapp_port
        ));
    }
    let path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, instance_id);
    let pid = if let Some(bytes) = read_recovery_file(&path)? {
        let record = instance::read_pid(instance::KERNEL_FAMILY_MCODE, instance_id)
            .ok_or_else(|| format!("MiniMax 进程记录损坏，已保留：{}", path.display()))?;
        let known_format = std::str::from_utf8(&bytes)
            .ok()
            .is_some_and(|text| text.split_whitespace().count() == 3);
        if !known_format
            || record.port != Some(config.port)
            || record.shell != Some(settings::current_mode().as_str())
        {
            return Err(format!(
                "MiniMax 进程记录归属无法确认，已保留：{}",
                path.display()
            ));
        }
        if record.pid == 0 || !lifecycle::process_is_definitely_gone(record.pid) {
            return Err(format!(
                "旧 MiniMax 进程 {} 仍在或无法确认，请先检查",
                record.pid
            ));
        }
        Some(record.pid)
    } else {
        None
    };
    // shortcut: 无法验证用户是否真的重启，支持跨平台启动标识时升级为自动核验。
    if let Some(lease) = &lease {
        crate::skills::mcode::discard_stale_running(lease).map_err(|error| error.to_string())?;
    }
    if pid.is_some() {
        std::fs::remove_file(&path)
            .map_err(|error| format!("清理 MiniMax 运行记录失败 {}：{error}", path.display()))?;
    }
    crate::shell::shell_events::record(
        "mcode-lifecycle",
        "用户确认重启后清理 MiniMax 旧运行记录与占位",
    );
    Ok(pid)
}

/// 状态轮询用的 Node 解析：**不派生 `node --version`**。
///
/// 概览页每 5 秒轮询一次状态，而 `node::detect::resolve` 与 `version_of` 每次
/// 都要派生进程（全仓同一结论，见 `commands::cached_node` 的 P2-27）。这里缓存
/// 自动探测的结果，键与那条缓存一致：壳设置里的 node 路径变了才重新解析。
///
/// 不用 `commands::cached_node`：它按**壳当前实例的族**解析 data dir，而这条
/// 链路一直按 DSH 族的 data dir 找托管运行时——换过去会在「壳停在 MiniMax
/// 页签启动」时换成用户环境里的 node（可能根本不存在）。
fn resolve_node_cached(config: Option<&Config>) -> (Option<PathBuf>, Option<String>) {
    if let Some(path) = config.and_then(|value| value.configured_node_path.clone()) {
        // 配置里存了版本就直接读，不派生进程。
        if let Some(version) = config.and_then(|value| value.configured_node_version.clone()) {
            return (Some(path), Some(version));
        }
        // 老配置只有路径、没有版本（configured_node_version 是后加的字段）：探测
        // 一次并在进程内记住。否则概览页每 5 秒一次的状态轮询都会派生一次
        // node --version，每分钟十几次。
        let key = format!("configured:{}", path.display());
        return cached_node(key, || {
            let version = crate::node::detect::version_of(&path);
            (Some(path.clone()), version)
        });
    }
    let settings = settings::load_for_shell(settings::current_mode());
    let key = format!(
        "settings:{}",
        settings.node_path.clone().unwrap_or_default()
    );
    let data_dir = lifecycle::data_dir(instance::KERNEL_FAMILY_DSH);
    cached_node(key, || {
        let info = crate::node::detect::resolve(&settings, &data_dir);
        (info.ok.then(|| PathBuf::from(info.path)), info.version)
    })
}

/// 进程内记住一次解析结果：键不变就不重算。
///
/// 探测可能派生 node 进程（上限 30 秒），所以**不持锁探测**——持锁派生会让另一个
/// 轮询线程一直等在这把锁上。
fn cached_node(
    key: String,
    resolve: impl FnOnce() -> (Option<PathBuf>, Option<String>),
) -> (Option<PathBuf>, Option<String>) {
    {
        let cache = crate::lock(node_cache());
        if let Some(value) = cache.get(&key) {
            return value.clone();
        }
    }
    let value = resolve();
    crate::lock(node_cache()).insert(key, value.clone());
    value
}

/// 解析出来的 Node：键 = 来源 + 路径，值 =（可执行路径, 版本）。
///
/// 按键分开存：自动探测与用户配置的路径各占一格，互不顶掉——顶掉的话，另一种
/// 来源的解析结果就得重算一次，等于把省下的进程又派生回来。
type NodeCache = Mutex<HashMap<String, (Option<PathBuf>, Option<String>)>>;

fn node_cache() -> &'static NodeCache {
    static CACHE: OnceLock<NodeCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve_node(config: Option<&Config>) -> Option<PathBuf> {
    if let Some(path) = config.and_then(|value| value.configured_node_path.clone()) {
        return Some(path);
    }
    let settings = settings::load_for_shell(settings::current_mode());
    let data_dir = lifecycle::data_dir(instance::KERNEL_FAMILY_DSH);
    let info = crate::node::detect::resolve(&settings, &data_dir);
    info.ok.then(|| PathBuf::from(info.path))
}

/// 只认壳自己装的那份：全局 npm / Node 前缀下的包不受壳管理，
/// 拿它当"已安装"会让概览和「内核版本」页报出互相矛盾的答案（2026-11-08 用户要求）。
fn resolve_package(config: Option<&Config>) -> Option<PathBuf> {
    if let Some(path) = config.and_then(|value| value.configured_package_path.clone()) {
        return Some(path);
    }
    // 没显式选过就取已装版本里最新的那一份（列表按版本降序）。不能写死 0.1.7：
    // 用户现在可以从官方版本列表装别的版本，写死会让那份安装「看不见」。
    crate::kernel::mcode_install::installed_versions()
        .ok()?
        .into_iter()
        .map(|item| item.package_path)
        .next()
}

/// 退出时 MiniMax 的真实状态。
///
/// 当前壳受管的进程与无法确认的旧记录分开：前者需要停止，后者保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McodeExitState {
    /// 没有在跑，也没有旧记录。
    Idle,
    /// 有进程在跑（或正在启动）。
    Running,
    /// 旧记录或占位仍在，无法确认进程是否退出。
    StaleRecord,
}

/// 退出探针：MiniMax 现在到底有没有在跑。
///
/// 不做 Node / 包探测——退出确认不能等这些慢操作；未知或在途的状态按
/// 「要停」处理。
pub(crate) fn exit_state() -> McodeExitState {
    let Ok(_start) = start_lock().try_lock() else {
        return McodeExitState::Running;
    };
    let Ok(active) = running().try_lock() else {
        return McodeExitState::Running;
    };
    if active.is_some() {
        return McodeExitState::Running;
    }
    drop(active);
    let config = match load_config() {
        Ok(Some(config)) => config,
        Ok(None) => return McodeExitState::Idle,
        Err(_) => return McodeExitState::Running,
    };
    match recovery_snapshot(&config) {
        Ok(None) => McodeExitState::Idle,
        _ => McodeExitState::StaleRecord,
    }
}

pub fn status() -> Result<McodeStatus, String> {
    let config = load_config()?;
    let (node_path, node_version) = resolve_node_cached(config.as_ref());
    let package_path = resolve_package(config.as_ref());
    let installed = package_path
        .as_deref()
        .is_some_and(|path| McodeAdapter::validate_package(path).is_ok())
        && node_version.as_deref().is_some_and(mcode_node_supported);
    let active = crate::lock(running());
    let active_config = active
        .as_ref()
        .filter(|entry| entry.ready && !entry.stopped && owns_ports(entry))
        .map(|entry| &entry.config);
    let state_config = active_config.or(config.as_ref());
    let (ponytail_enabled, ponytail_error) =
        match config.as_ref().map(ponytail_preference).transpose() {
            Ok(value) => (value.flatten(), None),
            Err(error) => (None, Some(error)),
        };
    let (stale, stale_error) = if active.is_none() {
        match config.as_ref().map(stale_pid).transpose() {
            Ok(value) => (value.flatten(), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        (None, None)
    };
    let recovery = if active.is_none() {
        config
            .as_ref()
            .map(recovery_snapshot)
            .transpose()?
            .flatten()
    } else {
        None
    };
    let error = if let Some(error) = stale_error {
        Some(error)
    } else if let Some(pid) = stale {
        Some(format!(
            "无法确认旧 MiniMax 进程 {pid} 是否已退出；请先重启电脑，再在概览页确认清理"
        ))
    } else if recovery.is_some() {
        Some("MiniMax 留有运行占位；请先重启电脑，再在概览页确认清理".into())
    } else if active_config.is_none() && active.as_ref().is_some_and(|entry| entry.ready) {
        Some("MiniMax 服务已停止或端口归属无法确认，请停止后重试".into())
    } else {
        None
    };
    Ok(McodeStatus {
        installed,
        running: active_config.is_some(),
        stop_required: active.is_some() || error.is_some(),
        window_open: false,
        instance_id: state_config.map(|value| value.instance_id.clone()),
        // 输出前统一成「已装版本列表里同一个形态」：安装返回的路径带着正斜杠
        // （`node_modules/@minimax-webui/minimax-code`），列表里的路径是
        // canonicalize 后的反斜杠；不归一化时前端按字符串比较「当前使用」会失败。
        configured_package_path: config.as_ref().and_then(|value| {
            value.configured_package_path.as_deref().map(|path| {
                std::path::Path::new(path).canonicalize().map_or_else(
                    |_| PathBuf::from(path),
                    |resolved| crate::shell::paths::plain_path(&resolved),
                )
            })
        }),
        configured_node_path: config
            .as_ref()
            .and_then(|value| value.configured_node_path.clone()),
        package_version: package_path
            .as_deref()
            .filter(|path| McodeAdapter::validate_package(path).is_ok())
            .and_then(crate::kernel::mcode_install::package_version),
        package_path,
        node_path,
        node_version,
        data_dir: state_config.map(|value| {
            paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &value.instance_id)
        }),
        port: state_config.map(|value| value.port),
        miniapp_port: state_config.map(|value| value.miniapp_port),
        error,
        stale_pid: stale,
        recovery_snapshot: recovery,
        skills_state: active.as_ref().map(|value| value.skills_state.clone()),
        skills_error: active.as_ref().and_then(|value| value.skills_error.clone()),
        skills_linked: config.as_ref().map(|value| value.link_skills),
        memory_state: active.as_ref().map(|value| value.memory_state.clone()),
        memory_error: active.as_ref().and_then(|value| value.memory_error.clone()),
        ponytail_enabled,
        ponytail_error,
        memory_profile: settings::load_for_shell(settings::current_mode()).profile,
    })
}

pub fn save_installation(
    package_path: Option<String>,
    node_path: Option<String>,
) -> Result<(), String> {
    // 不用阻塞锁：启动期间最长持有 start_lock 两分钟，阻塞等它等于让这次调用
    // 静默挂两分钟（面板的「保存」按钮在启动期间仍可点）。
    let _start = match start_lock().try_lock() {
        Ok(guard) => guard,
        Err(_) => return Err("MiniMax 工作台正在启动，请等启动结束后再保存".into()),
    };
    let _install = crate::kernel::mcode_install::install_lock()?;
    if crate::lock(running()).is_some() {
        return Err("请先停止 MiniMax 工作台".into());
    }
    let (mut config, fresh) = match load_config()? {
        Some(config) => (config, false),
        None => (new_config()?, true),
    };
    if let Some(pid) = stale_pid(&config)? {
        return Err(format!("旧 MiniMax 进程 {pid} 状态无法确认，请先检查"));
    }
    let _lease = offline_mutation_lease(&config)?;
    let package = package_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from);
    let node = node_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from);
    if let Some(path) = package.as_deref() {
        McodeAdapter::validate_package(path).map_err(|error| error.to_string())?;
    }
    if let Some(path) = node.as_deref() {
        if !path.is_absolute() {
            return Err("所选 Node.js 不可用或版本不受支持".into());
        }
        // 版本在这里校验并存进配置：状态轮询直接读它，不必每次派生 node 进程。
        let version = crate::node::detect::version_of(path)
            .filter(|value| mcode_node_supported(value))
            .ok_or("所选 Node.js 不可用或版本不受支持")?;
        config.configured_node_version = Some(version);
    } else {
        config.configured_node_version = None;
    }
    config.configured_package_path = package;
    config.configured_node_path = node;
    ensure_registered(&config, fresh)?;
    save_config(&config)
}

/// MiniMax 的端口只在没有任何进程可能还占着它时才能改：启动 / 停止过程中
/// 端口归属还没定下来，改设置会让两边记录分叉。
pub fn save_ports(port: u16, miniapp_port: u16) -> Result<(), String> {
    if !(1024..=65535).contains(&port) || !(1024..=65535).contains(&miniapp_port) {
        return Err("MiniMax 端口需为 1024–65535 的整数，请修改后重试".into());
    }
    if port == miniapp_port {
        return Err("工作台端口与 MiniMax App 端口不能相同，请改其中一个后重试".into());
    }
    let _start = match start_lock().try_lock() {
        Ok(guard) => guard,
        Err(_) => return Err("MiniMax 工作台正在启动，请等启动结束后再修改端口".into()),
    };
    match running().try_lock() {
        Ok(active) => {
            if active.is_some() {
                return Err("请先停止 MiniMax 工作台，再修改端口".into());
            }
        }
        Err(_) => return Err("MiniMax 工作台正在启动或停止，请完成后重试".into()),
    }
    let mut config = load_config()?.ok_or("请先安装 MiniMax 工作台，再修改端口")?;
    if let Some(pid) = stale_pid(&config)? {
        return Err(format!(
            "旧 MiniMax 进程 {pid} 状态无法确认，请先停止并检查后再修改端口"
        ));
    }
    let _lease = offline_mutation_lease(&config)?;
    if !port_available(port) {
        return Err(format!(
            "MiniMax 工作台端口 {port} 已被占用，请换个端口或释放后重试"
        ));
    }
    if !port_available(miniapp_port) {
        return Err(format!(
            "MiniMax App 端口 {miniapp_port} 已被占用，请换个端口或释放后重试"
        ));
    }
    config.port = port;
    config.miniapp_port = miniapp_port;
    sync_registered_port(&config)?;
    save_config(&config)
}

/// 共享技能库接入开关。只改配置：下次启动按新值同步或断开技能视图，
/// 正在运行的工作台不受影响，因此运行中也可以改。
pub fn set_skills_linked(enabled: bool) -> Result<(), String> {
    let _start = match start_lock().try_lock() {
        Ok(guard) => guard,
        Err(_) => return Err("MiniMax 工作台正在启动，请等启动结束后再修改技能接入".into()),
    };
    let mut config = load_config()?.ok_or("请先安装 MiniMax 工作台，再修改技能接入")?;
    config.link_skills = enabled;
    save_config(&config)
}

/// 改端口时实例记录必须一起改：两处端口不一致会让 configured_instance_id
/// 之后的每条 MiniMax 命令都报「实例记录与设置不一致」。
fn sync_registered_port(config: &Config) -> Result<(), String> {
    let _guard = crate::lock(instance::lifecycle_mutex());
    let mut registry = instance::load_registry().map_err(|error| error.to_string())?;
    let record = {
        let record = registry
            .get_mut(&config.instance_id)
            .ok_or("MiniMax 实例未登记，请先安装或启动一次工作台")?;
        if record.kernel_family != instance::KERNEL_FAMILY_MCODE {
            return Err("MiniMax 实例记录与设置不一致，请查看日志后恢复设置".into());
        }
        record.port = config.port;
        record.clone()
    };
    instance::save_record_to_disk(&record)?;
    instance::save_registry(&registry).map_err(|error| error.to_string())
}

/// Install into this shell's own MiniMax tree, preserving any external package.
pub fn install(version: String, progress: &mut dyn FnMut(&str)) -> Result<(), String> {
    let _start = crate::lock(start_lock());
    let _install = crate::kernel::mcode_install::install_lock()?;
    if crate::lock(running()).is_some() {
        return Err("请先停止 MiniMax 工作台再安装".into());
    }
    let (mut config, fresh) = match load_config()? {
        Some(config) => (config, false),
        None => (new_config()?, true),
    };
    if let Some(pid) = stale_pid(&config)? {
        return Err(format!("旧 MiniMax 进程 {pid} 状态无法确认，请先检查"));
    }
    let _lease = offline_mutation_lease(&config)?;
    let node = resolve_node(Some(&config)).ok_or("没有可用的 Node.js，请选择安装位置")?;
    let node_version = crate::node::detect::version_of(&node).ok_or("无法运行所选 Node.js")?;
    if !mcode_node_supported(&node_version) {
        return Err(format!(
            "Node.js {node_version} 不支持 MiniMax 内核，请选择 22.19+ 的 22.x 或 24.2 至 26.x"
        ));
    }
    let info = crate::node::detect::NodeInfo {
        path: node.to_string_lossy().into_owned(),
        version: Some(node_version),
        ok: true,
        reason: String::new(),
    };
    let _gate = crate::shell::shutdown::spawn_gate();
    if crate::shell::shutdown::is_shutting_down() {
        return Err("壳正在退出，已取消 MiniMax 安装".into());
    }
    let data_dir = lifecycle::data_dir(instance::KERNEL_FAMILY_DSH);
    let (_, pnpm) = crate::commands::promise_pnpm(&data_dir, &info, &mut *progress)?;
    let package = crate::kernel::mcode_install::install(&node, &pnpm, &version, progress)?;
    config.configured_package_path = Some(package);
    ensure_registered(&config, fresh)?;
    save_config(&config)
}

/// 删除一个未在使用的已装版本。与 DSH 内核的「删除」同一条纪律：工作台运行
/// 期间不动内核树；当前选用的版本不许删（否则下次启动就没有包可用了）。
pub fn remove_version(version: &str) -> Result<(), String> {
    let _start = crate::lock(start_lock());
    let _install = crate::kernel::mcode_install::install_lock()?;
    if crate::lock(running()).is_some() {
        return Err("请先停止 MiniMax 工作台再删除版本".into());
    }
    let config = load_config()?;
    let kernels =
        paths::family_runtime_dir(instance::KERNEL_FAMILY_MCODE, settings::current_mode())
            .join("kernels");
    if let Some(configured) = config
        .as_ref()
        .and_then(|value| value.configured_package_path.as_deref())
    {
        if std::path::Path::new(configured).starts_with(kernels.join(version)) {
            return Err(format!(
                "版本 {version} 是当前使用的版本，请先选用其它版本再删除"
            ));
        }
    }
    crate::kernel::mcode_install::remove_version(version)
}

fn page_ready(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(1)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if write!(
        stream,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .is_err()
    {
        return false;
    }
    let mut bytes = Vec::new();
    if stream.take(65536).read_to_end(&mut bytes).is_err() {
        return false;
    }
    bytes.starts_with(b"HTTP/1.1 200 ")
        && bytes
            .windows(b"__WEBUI_CONFIG__".len())
            .any(|chunk| chunk == b"__WEBUI_CONFIG__")
}

pub fn start(
    memory_source: Option<&Path>,
    ponytail_source: Option<&Path>,
    progress: &mut dyn FnMut(&str),
) -> Result<u16, String> {
    let generation = START_GENERATION.load(Ordering::SeqCst);
    let _start = crate::lock(start_lock());
    let _install = crate::kernel::mcode_install::install_lock()?;
    if START_GENERATION.load(Ordering::SeqCst) != generation {
        return Err("MiniMax 启动已取消".into());
    }
    if crate::shell::shutdown::is_shutting_down() {
        return Err("壳正在退出，已取消 MiniMax 启动".into());
    }
    if let Some(active) = crate::lock(running()).as_ref() {
        return if active.ready && !active.stopped && owns_ports(active) {
            Ok(active.config.port)
        } else {
            Err("MiniMax 工作台正在启动或服务状态无法确认，请先停止".into())
        };
    }
    progress("正在检测 MiniMax 安装和 Node.js…");
    let (config, fresh) = match load_config()? {
        Some(config) => (config, false),
        None => (new_config()?, true),
    };
    if let Some(pid) = stale_pid(&config)? {
        return Err(format!("旧 MiniMax 进程 {pid} 状态无法确认，请先检查"));
    }
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    if read_recovery_file(&crate::skills::mcode::running_path(&home))?.is_some() {
        return Err("MiniMax 留有运行占位；请先重启电脑，再在概览页确认清理".into());
    }
    if !port_available(config.port) || !port_available(config.miniapp_port) {
        return Err(format!(
            "MiniMax 端口 {} 或 {} 已被占用，请释放端口后重试",
            config.port, config.miniapp_port
        ));
    }
    let node = resolve_node(Some(&config)).ok_or("没有可用的 Node.js，请选择安装位置")?;
    let package = resolve_package(Some(&config))
        .ok_or("壳内尚未安装 MiniMax 内核，请先在「内核版本」页安装")?;
    ensure_registered(&config, fresh)?;
    if fresh {
        save_config(&config)?;
    }
    let record =
        instance::load_record_from_disk(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
            .ok_or("MiniMax 实例记录不存在，请查看日志")?;
    progress("正在准备 MiniMax 实例并启动服务…");
    let logs_dir = paths::shell_logs_dir(settings::current_mode());
    let adapter = McodeAdapter::new();
    let prepared = adapter
        .prepare_launch(
            &record,
            &McodeLaunchConfig {
                package_dir: package,
                miniapp_port: config.miniapp_port,
            },
            &node,
            &logs_dir,
        )
        .map_err(|error| error.to_string())?;
    let (skill_token, skills_state, skills_error, memory_bridge, memory_state, memory_error) = {
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        if crate::skills::mcode::enrolled(&home) {
            let lease =
                crate::skills::mcode::acquire_lease(&home).map_err(|error| error.to_string())?;
            crate::skills::mcode::offline_home(&lease).map_err(|error| error.to_string())?;
            match ponytail_preference(&config)? {
                Some(true) => {
                    if !crate::plugins::builtin::mcode_ponytail::reuse(&lease, true)? {
                        let source =
                            ponytail_source.ok_or("Ponytail 内置资源不可用，请重新安装应用")?;
                        crate::plugins::builtin::mcode_ponytail_source::with_source(
                            &lease,
                            progress,
                            |upstream| {
                                crate::plugins::builtin::mcode_ponytail::install(
                                    &lease, true, source, upstream,
                                )
                            },
                        )?;
                    }
                }
                Some(false) => crate::plugins::builtin::mcode_ponytail::disable(&lease)?,
                None if ponytail_owner_present(&home)? => {
                    return Err(
                        "MiniMax Ponytail 授权记录缺失，已保留插件；请先检查并恢复授权状态".into(),
                    )
                }
                None => {}
            }
            let linked = config.link_skills;
            let error = if linked {
                crate::skills::mcode::reconcile_locked(&lease)
            } else {
                crate::skills::mcode::detach_locked(&lease)
            }
            .err()
            .map(|error| error.to_string());
            if let Some(error) = &error {
                let message = if linked {
                    format!("MiniMax 共享技能同步失败：{error}；工作台继续启动")
                } else {
                    format!("MiniMax 共享技能断开失败：{error}；工作台继续启动")
                };
                crate::shell::shell_events::record("skill-wiring", &message);
                progress(&message);
            }
            let namespace = crate::memory::store::Namespace::for_bridge(
                instance::KERNEL_FAMILY_MCODE,
                &config.instance_id,
            );
            let memory = crate::memory::store::load(&namespace)
                .map_err(|error| error.to_string())
                .and_then(|doc| {
                    if !doc.meta.use_approved
                        && !crate::memory::module::enabled_for(&namespace)
                            .map_err(|error| error.to_string())?
                    {
                        return Ok(None);
                    }
                    let source = memory_source.ok_or("记忆插件资源不可用，请重新构建或安装")?;
                    crate::plugins::builtin::mcode_memory::prepare(source, &lease, true)
                });
            let (bridge, memory_state, memory_error) = match memory {
                Ok(Some(bridge)) => (Some(bridge), "pending_verification".to_string(), None),
                Ok(None) => (None, "disabled".to_string(), None),
                Err(error) => {
                    let message = format!("MiniMax 记忆接线失败：{error}；工作台继续启动");
                    crate::shell::shell_events::record("memory-bridge", &message);
                    progress(&message);
                    (None, "failed".to_string(), Some(error))
                }
            };
            let token = Some(
                crate::skills::mcode::arm_start(&lease, generation)
                    .map_err(|error| error.to_string())?,
            );
            let state = if error.is_some() {
                "failed"
            } else if linked {
                "pending_verification"
            } else {
                "detached"
            };
            (
                token,
                state.to_string(),
                error,
                bridge,
                memory_state,
                memory_error,
            )
        } else {
            if ponytail_preference(&config)?.is_some() || ponytail_owner_present(&home)? {
                return Err(
                    "MiniMax 生命周期记录缺失，无法确认 Ponytail 停用状态；请检查实例后重试".into(),
                );
            }
            (
                None,
                "legacy_unavailable".to_string(),
                None,
                None,
                "legacy_unavailable".to_string(),
                None,
            )
        }
    };
    let pid = {
        let _gate = crate::shell::shutdown::spawn_gate();
        if crate::shell::shutdown::is_shutting_down() {
            if let Some(token) = &skill_token {
                finish_skill_reservation(
                    &config,
                    token,
                    crate::skills::mcode::StopEvidence::NeverSpawned,
                )
                .map_err(|error| {
                    format!("壳正在退出，MiniMax 启动已取消；技能运行占位清理失败：{error}")
                })?;
            }
            return Err("壳正在退出，已取消 MiniMax 启动".into());
        }
        let result = with_current_start(generation, |active| {
            let _skill_lease = if let Some(token) = &skill_token {
                let home =
                    paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
                let lease = crate::skills::mcode::acquire_lease(&home)
                    .map_err(|error| (error.to_string(), false))?;
                crate::skills::mcode::verify_start(&lease, token, generation)
                    .map_err(|error| (error.to_string(), false))?;
                Some(lease)
            } else {
                None
            };
            let process = adapter
                .spawn_prepared(&prepared, &logs_dir)
                .map_err(|error| (error.to_string(), false))?;
            let pid = process.id();
            *active = Some(Running {
                process,
                config: config.clone(),
                ready: false,
                stopped: false,
                exited: false,
                skills_state: skills_state.clone(),
                skills_error: skills_error.clone(),
                memory_state: memory_state.clone(),
                memory_error: memory_error.clone(),
                memory_bridge,
                skill_token: skill_token.clone(),
            });
            instance::write_pid(
                instance::KERNEL_FAMILY_MCODE,
                &config.instance_id,
                pid,
                config.port,
            )
            .map_err(|error| (format!("保存 MiniMax 进程记录失败：{error}"), true))?;
            active
                .as_mut()
                .expect("published process")
                .process
                .release()
                .map_err(|error| (format!("启动 MiniMax 服务失败：{error}"), true))?;
            Ok(pid)
        });
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if let Some(token) = &skill_token {
                    finish_skill_reservation(
                        &config,
                        token,
                        crate::skills::mcode::StopEvidence::NeverSpawned,
                    )
                    .map_err(|cleanup| format!("{error}；技能运行占位清理失败：{cleanup}"))?;
                }
                return Err(error);
            }
        };
        match result {
            Ok(pid) => pid,
            Err((error, true)) => {
                let cleanup = stop_inner(false);
                return Err(format!("{error}；停止结果：{}", describe_outcome(&cleanup)));
            }
            Err((error, false)) => {
                if let Some(token) = &skill_token {
                    finish_skill_reservation(
                        &config,
                        token,
                        crate::skills::mcode::StopEvidence::NeverSpawned,
                    )
                    .map_err(|cleanup| format!("{error}；技能运行占位清理失败：{cleanup}"))?;
                }
                return Err(error);
            }
        }
    };
    progress("正在等待 MiniMax 页面就绪…");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        {
            let mut guard = crate::lock(running());
            let active = guard.as_mut().ok_or("MiniMax 启动已取消")?;
            match active.process.child.try_wait() {
                Ok(Some(status)) => {
                    active.exited = true;
                    // 根进程退出不等于整棵树退出（Unix 的 hook / 终端会脱离进程组）。
                    // 收不到「树已空」的确认就如实报错，不假装已停止。
                    stop_owned_process(active, false)?;
                    if port_available(config.port) && port_available(config.miniapp_port) {
                        active.stopped = true;
                        let path = paths::instance_pid_file(
                            instance::KERNEL_FAMILY_MCODE,
                            &config.instance_id,
                        );
                        if path.exists() {
                            std::fs::remove_file(path)
                                .map_err(|error| format!("清理 MiniMax 进程记录失败：{error}"))?;
                        }
                        if let Some(token) = &active.skill_token {
                            finish_skill_reservation(
                                &active.config,
                                token,
                                crate::skills::mcode::StopEvidence::TreeStopped,
                            )?;
                        }
                        *guard = None;
                    }
                    return Err(format!(
                        "MiniMax 服务提前退出（{status}），请检查安装和依赖"
                    ));
                }
                Err(error) => return Err(format!("无法检查 MiniMax 进程：{error}")),
                Ok(None) => {}
            }
        }
        // 便宜的判据先跑：owns_ports 要派生 lsof（Unix 上一次一个进程），
        // 每 300ms 核验一次太贵——只在页面已经就绪时才核验端口归属。
        if page_ready(config.port) {
            let owned = crate::lock(running())
                .as_ref()
                .is_some_and(|active| active.process.id() == pid && owns_ports(active));
            if owned {
                let mut guard = crate::lock(running());
                let active = guard.as_mut().ok_or("MiniMax 启动已取消")?;
                if active.process.id() != pid {
                    return Err("MiniMax 启动代次已改变".into());
                }
                active.ready = true;
                return Ok(config.port);
            }
        }
        if Instant::now() >= deadline {
            let cleanup = stop_inner(false);
            return Err(format!(
                "等待 MiniMax 工作台就绪超时，停止结果：{}。请检查端口与依赖",
                describe_outcome(&cleanup)
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

pub fn set_ponytail(
    instance_id: &str,
    enabled: bool,
    source: Option<&Path>,
    progress: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let _start = crate::lock(start_lock());
    let _install = crate::kernel::mcode_install::install_lock()?;
    if crate::lock(running()).is_some() {
        return Err("请先停止 MiniMax 工作台，再修改 Ponytail 授权".into());
    }
    let config = load_config()?.ok_or("请先配置 MiniMax 实例")?;
    if config.instance_id != instance_id
        || configured_instance_id()?.as_deref() != Some(instance_id)
    {
        return Err("MiniMax 实例已改变，请刷新页面后重试".into());
    }
    if stale_pid(&config)?.is_some() {
        return Err("无法确认 MiniMax 旧进程已退出，请先检查".into());
    }
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, instance_id);
    let lease = crate::skills::mcode::acquire_lease(&home).map_err(|error| error.to_string())?;
    // 先验证离线状态，再修改授权；残留占位不应触发授权写入或回退。
    crate::skills::mcode::offline_home(&lease).map_err(|error| {
        format!("MiniMax 留有运行占位，请先重启电脑并在概览页确认清理：{error}")
    })?;
    let previous = ponytail_preference(&config)?;
    if enabled {
        // Persist the disabled intent before publication: a crash must not leave an
        // unrecorded plugin that MiniMax will enable by default on the next start.
        if previous != Some(true) {
            save_ponytail_preference(&config, false)?;
        }
        let install =
            crate::plugins::builtin::mcode_ponytail::reuse(&lease, true).and_then(|reused| {
                if reused {
                    return Ok(());
                }
                let source = source.ok_or("Ponytail 内置资源不可用，请重新安装应用")?;
                crate::plugins::builtin::mcode_ponytail_source::with_source(
                    &lease,
                    progress,
                    |upstream| {
                        crate::plugins::builtin::mcode_ponytail::install(
                            &lease, true, source, upstream,
                        )
                    },
                )
            });
        if let Err(error) = install {
            if previous == Some(true) {
                return Err(error);
            }
            let cleanup = crate::plugins::builtin::mcode_ponytail::disable(&lease);
            if previous.is_none() && cleanup.is_ok() && !ponytail_owner_present(&home)? {
                std::fs::remove_file(ponytail_path(&config)).map_err(|cleanup_error| {
                    format!("{error}；无法恢复此前的授权状态：{cleanup_error}")
                })?;
            }
            return Err(format!(
                "{error}；接线回退结果：{}",
                describe_outcome(&cleanup)
            ));
        }
        if let Err(error) = save_ponytail_preference(&config, true) {
            let cleanup = if previous == Some(true) {
                Ok(())
            } else {
                crate::plugins::builtin::mcode_ponytail::disable(&lease)
            };
            return Err(format!(
                "{error}；撤回插件接线结果：{}",
                describe_outcome(&cleanup)
            ));
        }
    } else if previous.is_some() || ponytail_owner_present(&home)? {
        crate::plugins::builtin::mcode_ponytail::disable(&lease)?;
        save_ponytail_preference(&config, false)?;
    } else {
        return Err("此实例尚未由 dsh-xlink 管理 Ponytail，未修改现有插件".into());
    }
    Ok(())
}

pub fn upgrade_ponytail(
    instance_id: &str,
    source: Option<&Path>,
    progress: &mut dyn FnMut(&str),
) -> Result<bool, String> {
    let _start = crate::lock(start_lock());
    let _install = crate::kernel::mcode_install::install_lock()?;
    if crate::lock(running()).is_some() {
        return Err("请先停止 MiniMax 工作台，再更新 Ponytail".into());
    }
    let config = load_config()?.ok_or("请先配置 MiniMax 实例")?;
    if config.instance_id != instance_id
        || configured_instance_id()?.as_deref() != Some(instance_id)
    {
        return Err("MiniMax 实例已改变，请刷新页面后重试".into());
    }
    if stale_pid(&config)?.is_some() {
        return Err("无法确认 MiniMax 旧进程已退出，请先检查".into());
    }
    if ponytail_preference(&config)? != Some(true) {
        return Err("请先授权 MiniMax 使用 Ponytail".into());
    }
    let source = source.ok_or("Ponytail 内置资源不可用，请重新安装应用")?;
    let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, instance_id);
    let lease = crate::skills::mcode::acquire_lease(&home).map_err(|error| error.to_string())?;
    crate::plugins::builtin::mcode_ponytail_source::with_latest(&lease, progress, |upstream| {
        crate::plugins::builtin::mcode_ponytail::upgrade(&lease, true, source, upstream)
    })
}

pub fn stop() -> Result<(), String> {
    stop_inner(true)
}

fn stop_inner(acknowledge_forced: bool) -> Result<(), String> {
    START_GENERATION.fetch_add(1, Ordering::SeqCst);
    let mut guard = crate::lock(running());
    if let Some(active) = guard.as_mut() {
        #[cfg(not(windows))]
        if active.exited && !active.stopped {
            let pid = active.process.id();
            // 根进程没了不代表整棵树没了：脱离进程组的 hook / 终端还在时不能算已停止。
            // 收不到确认就走下面的统一收尾（stop_owned_process），由它如实上报。
            if active.process.owned_tree_empty().unwrap_or(false)
                && lifecycle::process_is_definitely_gone(pid)
                && port_available(active.config.port)
                && port_available(active.config.miniapp_port)
            {
                active.stopped = true;
            }
        }
        if !active.stopped {
            stop_owned_process(active, acknowledge_forced)?;
        }
        stop_memory_bridge(active)?;
        if !port_available(active.config.port) || !port_available(active.config.miniapp_port) {
            return Err("MiniMax 进程已停止，但工作台或应用端口仍被占用；保留进程记录，请检查残留进程后重试".into());
        }
        let path =
            paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &active.config.instance_id);
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|error| format!("清理 MiniMax 进程记录失败：{error}"))?;
        }
        if let Some(token) = &active.skill_token {
            finish_skill_reservation(
                &active.config,
                token,
                crate::skills::mcode::StopEvidence::TreeStopped,
            )?;
        }
        *guard = None;
    } else if let Some(config) = load_config()? {
        let path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        if path.exists() && (!port_available(config.port) || !port_available(config.miniapp_port)) {
            return Err(format!(
                "旧 MiniMax 进程可能仍占用端口 {} 或 {}；请检查端口归属，确认停止后重试",
                config.port, config.miniapp_port
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exited_test_pid() -> u32 {
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", "exit", "0"])
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("true").spawn().unwrap()
        };
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn recovery_rejects_changed_or_uncertain_records_and_busy_ports() {
        let root = std::env::temp_dir().join(format!(
            "xlink-recovery-scope-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let config = new_config().unwrap();
        ensure_registered(&config, true).unwrap();
        save_config(&config).unwrap();
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let marker = crate::skills::mcode::running_path(&home);
        let pid_path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let old_pid = exited_test_pid();
        let marker_data = |nonce: &str, owner| {
            format!(r#"{{"nonce":"{nonce}","generation":1,"owner_pid":{owner}}}"#)
        };
        std::fs::write(&marker, marker_data(&"a".repeat(32), old_pid)).unwrap();
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            old_pid,
            config.port,
        )
        .unwrap();
        std::fs::write(home.join("keep"), b"user data").unwrap();
        let snapshot = recovery_snapshot(&config).unwrap().unwrap();

        assert!(discard_run_record("other-instance", &snapshot).is_err());
        let lock = crate::lock(start_lock());
        assert!(discard_run_record(&config.instance_id, &snapshot).is_err());
        drop(lock);
        std::fs::write(&marker, marker_data(&"b".repeat(32), old_pid)).unwrap();
        assert!(discard_run_record(&config.instance_id, &snapshot).is_err());
        assert!(pid_path.exists() && marker.exists());
        std::fs::write(&marker, marker_data(&"a".repeat(32), old_pid)).unwrap();

        let listener = TcpListener::bind(("127.0.0.1", config.miniapp_port)).unwrap();
        assert!(discard_run_record(&config.instance_id, &snapshot).is_err());
        assert!(pid_path.exists() && marker.exists());
        drop(listener);

        std::fs::write(&marker, marker_data(&"a".repeat(32), std::process::id())).unwrap();
        let live_snapshot = recovery_snapshot(&config).unwrap().unwrap();
        assert!(discard_run_record(&config.instance_id, &live_snapshot).is_err());
        assert!(pid_path.exists() && marker.exists());
        std::fs::write(&marker, b"broken").unwrap();
        let corrupt_snapshot = recovery_snapshot(&config).unwrap().unwrap();
        assert!(discard_run_record(&config.instance_id, &corrupt_snapshot).is_err());
        assert_eq!(std::fs::read(&marker).unwrap(), b"broken");
        assert!(pid_path.exists());
        std::fs::write(&marker, marker_data(&"a".repeat(32), old_pid)).unwrap();
        std::fs::write(&pid_path, b"broken pid").unwrap();
        let view = status().unwrap();
        assert!(view.stop_required && view.recovery_snapshot.is_some());
        assert!(view.error.unwrap().contains("损坏"));
        assert!(discard_run_record(&config.instance_id, &view.recovery_snapshot.unwrap()).is_err());
        assert_eq!(std::fs::read(&pid_path).unwrap(), b"broken pid");
        assert!(marker.exists());

        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            std::process::id(),
            config.port,
        )
        .unwrap();
        let live_snapshot = recovery_snapshot(&config).unwrap().unwrap();
        assert!(discard_run_record(&config.instance_id, &live_snapshot).is_err());
        assert!(pid_path.exists() && marker.exists());
        std::fs::write(
            &pid_path,
            format!("{old_pid} {} unknown-shell\n", config.port),
        )
        .unwrap();
        let unknown_snapshot = recovery_snapshot(&config).unwrap().unwrap();
        assert!(discard_run_record(&config.instance_id, &unknown_snapshot).is_err());
        assert!(pid_path.exists() && marker.exists());
        let extended = format!(
            "{old_pid} {} {} extra\n",
            config.port,
            settings::current_mode().as_str()
        );
        std::fs::write(&pid_path, &extended).unwrap();
        let unknown_snapshot = recovery_snapshot(&config).unwrap().unwrap();
        assert!(discard_run_record(&config.instance_id, &unknown_snapshot).is_err());
        assert_eq!(std::fs::read_to_string(&pid_path).unwrap(), extended);
        assert!(marker.exists());
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            old_pid,
            config.port,
        )
        .unwrap();

        assert_eq!(
            discard_run_record(&config.instance_id, &snapshot).unwrap(),
            Some(old_pid)
        );
        assert!(!pid_path.exists() && !marker.exists());
        assert_eq!(std::fs::read(home.join("keep")).unwrap(), b"user data");
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn start_rejects_stale_reservation_before_node_and_package_preparation() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-start-reservation-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let mut config = new_config().unwrap();
        ensure_registered(&config, true).unwrap();
        config.configured_node_path = Some(root.join("missing-node"));
        config.configured_package_path = Some(root.join("missing-package"));
        save_config(&config).unwrap();
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let lease = crate::skills::mcode::acquire_lease(&home).unwrap();
        crate::skills::mcode::arm_start(&lease, 1).unwrap();
        drop(lease);
        let marker = home.join(".xlink-mcode-lifecycle/running");
        let saved = std::fs::read(&marker).unwrap();
        let mut messages = Vec::new();
        let error = start(None, None, &mut |message| {
            messages.push(message.to_string())
        })
        .unwrap_err();
        assert!(
            error.contains("概览页") && error.contains("重启电脑"),
            "{error}"
        );
        assert!(!messages.iter().any(|message| message.contains("正在准备")));
        assert_eq!(std::fs::read(&marker).unwrap(), saved);
        assert!(
            !paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id).exists()
        );
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_reservation_keeps_ponytail_authorization_unchanged() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-ponytail-reservation-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let config = new_config().unwrap();
        ensure_registered(&config, true).unwrap();
        save_config(&config).unwrap();
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let lease = crate::skills::mcode::acquire_lease(&home).unwrap();
        crate::skills::mcode::arm_start(&lease, 1).unwrap();
        drop(lease);
        let marker = crate::skills::mcode::running_path(&home);
        let saved = std::fs::read(&marker).unwrap();
        let view = status().unwrap();
        assert!(view.recovery_snapshot.is_some() && view.stop_required);
        assert_eq!(view.stale_pid, None);
        for previous in [None, Some(false), Some(true)] {
            if let Some(enabled) = previous {
                save_ponytail_preference(&config, enabled).unwrap();
            }
            for enabled in [true, false] {
                let error =
                    set_ponytail(&config.instance_id, enabled, None, &mut |_| {}).unwrap_err();
                assert!(
                    error.contains("概览页") && error.contains("重启电脑"),
                    "{error}"
                );
                assert!(!error.contains("回退"));
                assert_eq!(ponytail_preference(&config).unwrap(), previous);
                assert_eq!(std::fs::read(&marker).unwrap(), saved);
            }
        }
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_ponytail_enable_keeps_disabled_intent_when_owned_or_rollback_fails() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-ponytail-intent-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let config = new_config().unwrap();
        ensure_registered(&config, true).unwrap();
        save_config(&config).unwrap();
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let owner = home.join(".xlink-ponytail-owner.json");
        let owner_bytes = format!(
            "{{\"nonce\":\"{}\",\"digest\":\"{}\"}}",
            "a".repeat(32),
            "b".repeat(64)
        );
        std::fs::write(&owner, owner_bytes.as_bytes()).unwrap();
        assert!(set_ponytail(&config.instance_id, true, None, &mut |_| {}).is_err());
        assert_eq!(ponytail_preference(&config).unwrap(), Some(false));
        assert_eq!(std::fs::read(&owner).unwrap(), owner_bytes.as_bytes());

        std::fs::remove_file(&owner).unwrap();
        std::fs::remove_file(ponytail_path(&config)).unwrap();
        let existing = home.join("plugins/xlink-ponytail");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("keep"), b"user content").unwrap();
        assert!(set_ponytail(&config.instance_id, true, None, &mut |_| {}).is_err());
        assert_eq!(ponytail_preference(&config).unwrap(), Some(false));
        assert_eq!(
            std::fs::read(existing.join("keep")).unwrap(),
            b"user content"
        );
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configured_instance_is_read_only_and_requires_consistent_registry_and_record() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-identity-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        assert_eq!(configured_instance_id().unwrap(), None);
        let config = new_config().unwrap();
        save_config(&config).unwrap();
        assert!(
            configured_instance_id().is_err(),
            "unregistered config must not be adopted"
        );
        ensure_registered(&config, false).unwrap();
        assert_eq!(
            configured_instance_id().unwrap(),
            Some(config.instance_id.clone())
        );
        let path = paths::instance_record_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let mut record =
            instance::load_record_from_disk(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
                .unwrap();
        record.port = if config.port == u16::MAX {
            config.port - 1
        } else {
            config.port + 1
        };
        std::fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(
            configured_instance_id().is_err(),
            "mismatched record must not be accepted"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn shorter_ids_preserve_old_configured_instance_ids() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-id-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).expect("temporary home");
        let guard = crate::tests::scoped_xlink_home(&home);
        let mut config = new_config().expect("new configuration");
        assert_eq!(config.instance_id.len(), 28);
        assert!(config.instance_id.starts_with("mcode-"));
        assert!(config.instance_id[6..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
        config.instance_id = "mcode-0123456789abcdef0123456789abcdef".into();
        save_config(&config).expect("save legacy config");
        assert_eq!(
            load_config()
                .expect("read config")
                .expect("present")
                .instance_id,
            config.instance_id
        );
        drop(guard);
        std::fs::remove_dir_all(home).expect("remove temporary home");
    }

    #[test]
    fn stop_cancels_preparation_and_serializes_with_spawn() {
        use std::sync::mpsc;

        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-cancel-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).expect("temporary home");
        let guard = crate::tests::scoped_xlink_home(&home);
        let generation = START_GENERATION.load(Ordering::SeqCst);
        let (prepared_tx, prepared_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let starter = std::thread::spawn(move || {
            prepared_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
            with_current_start(generation, |_| ()).is_err()
        });
        prepared_rx.recv().unwrap();
        stop().expect("stop while preparing");
        continue_tx.send(()).unwrap();
        assert!(
            starter.join().unwrap(),
            "cancelled preparation must not spawn"
        );

        let generation = START_GENERATION.load(Ordering::SeqCst);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let starter = std::thread::spawn(move || {
            with_current_start(generation, |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        });
        entered_rx.recv().unwrap();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let stopper = std::thread::spawn(move || stopped_tx.send(stop()).unwrap());
        assert!(
            stopped_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "stop must wait for in-flight spawn publication"
        );
        release_tx.send(()).unwrap();
        starter.join().unwrap();
        stopped_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        stopper.join().unwrap();
        drop(guard);
        std::fs::remove_dir_all(home).expect("remove temporary home");
    }

    #[test]
    fn exit_probe_treats_busy_corrupt_and_recorded_state_conservatively() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-exit-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        assert_eq!(exit_state(), McodeExitState::Idle);

        let start = crate::lock(start_lock());
        assert_eq!(
            exit_state(),
            McodeExitState::Running,
            "a start in progress must be treated as active"
        );
        drop(start);
        let active = crate::lock(running());
        assert_eq!(
            exit_state(),
            McodeExitState::Running,
            "a busy runtime must be treated as active"
        );
        drop(active);

        let config = new_config().unwrap();
        save_config(&config).unwrap();
        assert_eq!(exit_state(), McodeExitState::Idle);
        let pid = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        std::fs::create_dir_all(pid.parent().unwrap()).unwrap();
        std::fs::write(&pid, b"unrecognized record").unwrap();
        assert_eq!(
            exit_state(),
            McodeExitState::StaleRecord,
            "even a corrupt PID record must be reported as a stale record"
        );
        std::fs::remove_file(pid).unwrap();
        std::fs::write(config_path(), b"invalid").unwrap();
        assert_eq!(
            exit_state(),
            McodeExitState::Running,
            "corrupt configuration must not be ignored"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn stop_preserves_a_stale_record_even_when_the_ports_are_free() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-stale-exit-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let config = test_config();
        let pid_path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            u32::MAX,
            config.port,
        )
        .expect("stale pid record");

        assert_eq!(exit_state(), McodeExitState::StaleRecord);
        stop().expect("无受管进程时允许退出");
        assert!(pid_path.exists(), "空端口不能证明进程树已退出");
        assert_eq!(exit_state(), McodeExitState::StaleRecord);

        // 端口仍被占：如实报错，记录保留（不能替用户认领端口）。
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            u32::MAX,
            config.port,
        )
        .expect("stale pid record");
        let listener = TcpListener::bind(("127.0.0.1", config.port)).expect("hold port");
        let err = stop().expect_err("端口被占时必须如实报错");
        assert!(err.contains("仍占用端口"), "{err}");
        assert!(pid_path.exists(), "报错时记录必须保留");
        drop(listener);
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn port_available_treats_a_listening_port_as_occupied() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(
            !port_available(port),
            "被监听的端口不能算空闲——否则停止后会把残留进程当成还在跑"
        );
        drop(listener);
        // 释放后端口算空闲（Windows 的 TCP 表可能滞后一拍，给它一点时间）。
        let deadline = Instant::now() + Duration::from_secs(2);
        while !port_available(port) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(port_available(port), "释放后的端口应算空闲");
    }

    #[cfg(windows)]
    #[test]
    fn exit_probe_does_not_invoke_slow_node_detection() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-exit-node-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let node = home.join("slow-node.cmd");
        let marker = home.join("node-invoked");
        std::fs::write(
            &node,
            b"@echo off\r\n>\"%~dp0node-invoked\" echo invoked\r\nping -n 3 127.0.0.1 >nul\r\necho v26.12.0\r\n",
        )
        .unwrap();
        let mut config = new_config().unwrap();
        config.configured_node_path = Some(node.clone());
        config.configured_package_path = Some(home.join("no-package"));
        save_config(&config).unwrap();

        assert_eq!(exit_state(), McodeExitState::Idle);
        assert!(!marker.exists(), "exit confirmation must not run Node");
        assert_eq!(
            crate::node::detect::version_of(&node).as_deref(),
            Some("v26.12.0")
        );
        assert!(
            marker.exists(),
            "the slow Node fixture must actually be executable"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn unknown_live_pid_is_kept_and_never_adopted() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-pid-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).expect("temporary home");
        let guard = crate::tests::scoped_xlink_home(&home);
        let mut config = new_config().expect("configuration");
        config.configured_package_path = Some(home.join("no-package"));
        config.configured_node_path = Some(home.join("no-node"));
        save_config(&config).unwrap();
        assert!(!status().unwrap().stop_required);
        instance::ensure_instance_dirs(&instance::InstanceRecord::new(
            &config.instance_id,
            instance::KERNEL_FAMILY_MCODE,
            config.port,
            process::epoch_millis(),
        ))
        .expect("instance dirs");
        let pid = std::process::id();
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            pid,
            config.port,
        )
        .expect("pid record");
        let view = status().unwrap();
        assert!(!view.running);
        assert!(
            view.stop_required,
            "unknown ownership must keep the stop action available"
        );
        assert_eq!(stale_pid(&config).expect("pid state"), Some(pid));
        assert!(
            paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id).exists()
        );
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            u32::MAX,
            config.port,
        )
        .expect("stale pid record");
        assert_eq!(stale_pid(&config).expect("stale pid state"), Some(u32::MAX));
        assert!(
            paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id).exists()
        );
        drop(guard);
        std::fs::remove_dir_all(home).expect("remove temporary home");
    }

    #[cfg(unix)]
    #[test]
    fn broken_pid_symlink_cannot_be_treated_as_absent() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-pid-link-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let mut config = test_config();
        config.configured_package_path = Some(root.join("no-package"));
        config.configured_node_path = Some(root.join("no-node"));
        save_config(&config).unwrap();
        let pid = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        symlink(root.join("missing-target"), &pid).unwrap();

        assert!(start(None, None, &mut |_| {})
            .unwrap_err()
            .contains("不是普通文件"));
        let (port, miniapp_port) = free_ports();
        assert!(save_ports(port, miniapp_port)
            .unwrap_err()
            .contains("不是普通文件"));
        assert!(std::fs::symlink_metadata(&pid)
            .unwrap()
            .file_type()
            .is_symlink());
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn disabled_skills_still_reserve_instance_in_real_launcher() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "xlink-real-launcher-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let home_guard = crate::tests::scoped_xlink_home(&root);
        let (found, node) = process::run_capture("which", &["node"]).unwrap();
        assert!(found, "测试需要 Node.js");
        let node = node.trim().replace(char::from(39), "'\\''");
        let wrapper = root.join("node-wrapper");
        std::fs::write(&wrapper, format!("#!/bin/sh\nunset NODE_OPTIONS DSH_XLINK_MCODE_NODE_OPTIONS\nexec '{node}' \"$@\"\n")).unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();

        let package = root.join("fake-mcode");
        for dir in [
            "bin",
            "node_modules/better-sqlite3",
            "node_modules/node-pty",
        ] {
            std::fs::create_dir_all(package.join(dir)).unwrap();
        }
        std::fs::write(package.join("package.json"), r#"{"name":"@minimax-webui/minimax-code","version":"0.1.7","bin":{"mcode-webui":"bin/mcode-webui.mjs"}}"#).unwrap();
        std::fs::write(
            package.join("node_modules/better-sqlite3/index.js"),
            "module.exports = class { close() {} };\n",
        )
        .unwrap();
        std::fs::write(
            package.join("node_modules/node-pty/index.js"),
            "module.exports = {};\n",
        )
        .unwrap();
        std::fs::write(
            package.join("bin/mcode-webui.mjs"),
            r#"
import http from 'node:http';
const serve = () => http.createServer((_req, res) => res.end('__WEBUI_CONFIG__'));
serve().listen(Number(process.env.WEBUI_SERVER_PORT), '127.0.0.1');
serve().listen(Number(process.env.WEBUI_MINIAPP_PORT), '127.0.0.1');
"#,
        )
        .unwrap();
        let mut config = new_config().unwrap();
        config.link_skills = false;
        config.configured_package_path = Some(package);
        config.configured_node_path = Some(wrapper);
        ensure_registered(&config, true).unwrap();
        save_config(&config).unwrap();
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let marker = crate::skills::mcode::running_path(&home);
        let pid = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);

        struct StopOnDrop(PathBuf);
        impl Drop for StopOnDrop {
            fn drop(&mut self) {
                if stop().is_ok() {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
        }
        let cleanup = StopOnDrop(root.clone());
        assert_eq!(start(None, None, &mut |_| {}).unwrap(), config.port);
        assert!(marker.exists(), "未接入共享技能也必须保留运行占位");
        assert!(pid.exists());
        let lease = crate::skills::mcode::acquire_lease(&home).unwrap();
        assert!(crate::skills::mcode::offline_home(&lease).is_err());
        drop(lease);
        stop().unwrap();
        assert!(!marker.exists() && !pid.exists());
        drop(cleanup);
        assert!(!root.exists());
        drop(home_guard);
    }

    #[test]
    fn orphaned_reservation_is_visible_and_blocks_start_without_skills() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-orphan-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let mut config = new_config().unwrap();
        config.link_skills = false;
        ensure_registered(&config, true).unwrap();
        save_config(&config).unwrap();
        let home = paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        let marker = home.join(".xlink-mcode-lifecycle/running");
        let old_owner = exited_test_pid();
        std::fs::write(
            &marker,
            format!(
                r#"{{"nonce":"{}","generation":1,"owner_pid":{old_owner}}}"#,
                "a".repeat(32)
            ),
        )
        .unwrap();

        assert!(
            status().unwrap().stop_required,
            "没有 PID 也必须显示运行占位"
        );
        assert_eq!(exit_state(), McodeExitState::StaleRecord);
        assert!(start(None, None, &mut |_| {})
            .unwrap_err()
            .contains("运行占位"));
        assert!(marker.exists());
        let (new_port, new_miniapp_port) = free_ports();
        assert!(
            save_ports(new_port, new_miniapp_port).is_err(),
            "旧占位存在时不能更改端口"
        );
        assert!(
            save_installation(None, None).is_err(),
            "旧占位存在时不能更改运行环境"
        );
        assert!(install(
            crate::kernel::mcode_install::default_version().to_string(),
            &mut |_| {}
        )
        .unwrap_err()
        .contains("运行占位"));
        let saved = load_config().unwrap().unwrap();
        assert_eq!(
            (saved.port, saved.miniapp_port),
            (config.port, config.miniapp_port)
        );
        let snapshot = status().unwrap().recovery_snapshot.unwrap();
        assert_eq!(
            discard_run_record(&config.instance_id, &snapshot).unwrap(),
            None
        );
        assert!(!marker.exists());
        assert!(!status().unwrap().stop_required);
        drop(guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discard_run_record_needs_owner_action_and_released_ports() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-discard-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).expect("temporary home");
        let guard = crate::tests::scoped_xlink_home(&home);
        let config = test_config();
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            u32::MAX,
            config.port,
        )
        .expect("stale pid record");
        let pid_path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        assert_eq!(status().unwrap().stale_pid, Some(u32::MAX));

        // 端口还被占着时拒绝：占用方可能正是旧记录写下的那棵树。
        let listener = TcpListener::bind(("127.0.0.1", config.port)).expect("hold port");
        let snapshot = status().unwrap().recovery_snapshot.unwrap();
        assert!(discard_run_record(&config.instance_id, &snapshot).is_err());
        assert!(pid_path.exists(), "refused cleanup must keep the record");
        drop(listener);

        assert_eq!(
            discard_run_record(&config.instance_id, &snapshot).unwrap(),
            Some(u32::MAX)
        );
        assert!(!pid_path.exists());
        let view = status().unwrap();
        assert_eq!(view.stale_pid, None);
        assert!(!view.stop_required);
        assert!(
            discard_run_record(&config.instance_id, &snapshot).is_err(),
            "a second cleanup has nothing left to remove"
        );
        drop(guard);
        std::fs::remove_dir_all(home).expect("remove temporary home");
    }

    fn free_ports() -> (u16, u16) {
        let port = allocate_port().expect("free port");
        let mut miniapp_port = allocate_port().expect("free app port");
        while miniapp_port == port {
            miniapp_port = allocate_port().expect("free app port");
        }
        (port, miniapp_port)
    }

    /// 每个用例都在自己的临时 home 里先落一份「已登记 + 已保存」的配置，
    /// 拒绝类用例才能断言配置文件一个字节都没动。
    fn test_config() -> Config {
        let config = new_config().expect("configuration");
        ensure_registered(&config, false).expect("registry");
        save_config(&config).expect("saved config");
        config
    }

    /// 状态轮询不能派生 node 进程：配置的 Node 版本在保存时就校验并存进配置，
    /// 状态直接读它。这里配一个**不存在的**路径——还能报出版本就说明没去探测它。
    #[test]
    fn status_reports_the_configured_node_version_without_probing() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-node-version-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let node = home.join("no-such-node");
        let mut config = test_config();
        config.configured_node_path = Some(node.clone());
        config.configured_node_version = Some("24.2.0".into());
        save_config(&config).unwrap();
        let view = status().unwrap();
        assert_eq!(view.node_version.as_deref(), Some("24.2.0"));
        assert_eq!(view.node_path.as_deref(), Some(node.as_path()));
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 共享技能库默认接入；开关落盘；老配置没有这一项时按接入处理。
    #[test]
    fn skills_link_switch_is_persisted_and_old_config_defaults_to_linked() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-skills-link-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        test_config();
        assert_eq!(status().unwrap().skills_linked, Some(true));
        set_skills_linked(false).unwrap();
        assert_eq!(status().unwrap().skills_linked, Some(false));
        let path = config_path();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("link_skills");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(status().unwrap().skills_linked, Some(true));
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 假 node：每被叫一次就往标记文件追加一行，并报一个版本号。
    #[cfg(windows)]
    fn fake_node(home: &Path, marker: &Path) -> PathBuf {
        let path = home.join("fake-node.cmd");
        std::fs::write(
            &path,
            format!(
                "@echo off\r\n>>\"{}\" echo invoked\r\necho v26.12.0\r\n",
                marker.display()
            ),
        )
        .unwrap();
        path
    }

    #[cfg(not(windows))]
    fn fake_node(home: &Path, marker: &Path) -> PathBuf {
        let path = home.join("fake-node.sh");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho invoked >> \"{}\"\necho v26.12.0\n",
                marker.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// 假 node 被叫过几次。
    fn invocations(marker: &Path) -> usize {
        std::fs::read_to_string(marker)
            .map(|body| body.lines().filter(|line| !line.trim().is_empty()).count())
            .unwrap_or(0)
    }

    /// 老配置只有 Node 路径、没有版本时，状态轮询**只能探测一次**。
    ///
    /// 概览页每 5 秒轮询一次状态；每次都派生 `node --version` 的话，一分钟就是
    /// 十几次进程派生（node 探测有 30 秒超时上限，机器上 node 半卡时整片轮询跟着
    /// 卡住）。这里用一个会被记数的假 node：两次状态查询之后，它只该被叫过一次。
    #[test]
    fn an_old_config_probes_the_node_version_only_once() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-node-probe-once-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let marker = home.join("node-invoked");
        let node = fake_node(&home, &marker);
        let mut config = test_config();
        config.configured_node_path = Some(node);
        // 老配置：没有 configured_node_version 这一项（它是后加的字段）。
        config.configured_node_version = None;
        save_config(&config).unwrap();

        let first = status().unwrap();
        assert_eq!(first.node_version.as_deref(), Some("v26.12.0"));
        assert_eq!(invocations(&marker), 1, "第一次状态查询应当探测一次");
        let second = status().unwrap();
        assert_eq!(second.node_version.as_deref(), Some("v26.12.0"));
        assert_eq!(
            invocations(&marker),
            1,
            "第二次状态查询必须走缓存，不得再派生 node"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn save_ports_rejects_while_running_or_starting_and_keeps_config() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-ports-guard-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let config = test_config();
        let (port, miniapp_port) = free_ports();

        // 启动期间端口归属还没定下来：start 锁被占时直接拒绝。
        let start = crate::lock(start_lock());
        assert!(save_ports(port, miniapp_port).is_err());
        drop(start);

        // 运行期由 running 锁保护；「锁被占即视为运行中」与退出探测用例同一判法。
        let active = crate::lock(running());
        assert!(save_ports(port, miniapp_port).is_err());
        drop(active);

        // 无法确认归属的旧进程记录同样拒绝。
        instance::write_pid(
            instance::KERNEL_FAMILY_MCODE,
            &config.instance_id,
            u32::MAX,
            config.port,
        )
        .expect("stale pid record");
        assert!(save_ports(port, miniapp_port).is_err());

        let saved = load_config().expect("config").expect("present");
        assert_eq!(saved.port, config.port);
        assert_eq!(saved.miniapp_port, config.miniapp_port);
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn save_ports_rejects_invalid_or_duplicate_ports() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-ports-invalid-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let config = test_config();
        let (port, miniapp_port) = free_ports();

        assert!(save_ports(0, miniapp_port).is_err(), "0 is not usable");
        assert!(save_ports(1023, miniapp_port).is_err(), "privileged port");
        assert!(save_ports(port, port).is_err(), "the two ports collide");

        let saved = load_config().expect("config").expect("present");
        assert_eq!(saved.port, config.port);
        assert_eq!(saved.miniapp_port, config.miniapp_port);
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn save_ports_persists_config_and_instance_record_together() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-ports-save-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let config = test_config();
        let (port, miniapp_port) = free_ports();

        save_ports(port, miniapp_port).expect("save ports");

        let saved = load_config().expect("config").expect("present");
        assert_eq!((saved.port, saved.miniapp_port), (port, miniapp_port));
        let record =
            instance::load_record_from_disk(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
                .expect("instance record");
        assert_eq!(record.port, port);
        let registry = instance::load_registry().expect("registry");
        assert_eq!(
            registry
                .get(&config.instance_id)
                .expect("registered instance")
                .port,
            port
        );
        assert_eq!(
            configured_instance_id().expect("consistent identity"),
            Some(config.instance_id)
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 删除版本与 DSH 内核同一套纪律：当前选用的版本拒绝，其它已装版本删得掉，
    /// 而且只删自己那一层。
    #[test]
    fn remove_version_refuses_the_configured_package_and_deletes_the_rest() {
        let home = std::env::temp_dir().join(format!(
            "xlink-mcode-remove-version-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = crate::tests::scoped_xlink_home(&home);
        let mut config = test_config();
        let kernels =
            paths::family_runtime_dir(instance::KERNEL_FAMILY_MCODE, settings::current_mode())
                .join("kernels");
        for version in ["0.1.7", "0.1.6"] {
            let package = kernels
                .join(version)
                .join("node_modules/@minimax-webui/minimax-code");
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(
                package.join("package.json"),
                format!(r#"{{"name":"@minimax-webui/minimax-code","version":"{version}"}}"#),
            )
            .unwrap();
        }
        config.configured_package_path =
            Some(kernels.join("0.1.7/node_modules/@minimax-webui/minimax-code"));
        save_config(&config).unwrap();

        assert!(
            remove_version("0.1.7").unwrap_err().contains("当前使用"),
            "当前选用的版本不许删"
        );
        assert!(remove_version("9.9.9").unwrap_err().contains("未安装"));
        remove_version("0.1.6").expect("未使用的版本应能删除");
        assert!(!kernels.join("0.1.6").exists());
        assert!(kernels.join("0.1.7").exists(), "只许删自己那一层");
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }
}
