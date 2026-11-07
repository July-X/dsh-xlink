//! MiniMax 工作台的按壳配置与受控子进程。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

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
    port: u16,
    miniapp_port: u16,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct McodeCapabilities {
    pub skills: bool,
    pub memory: bool,
    pub ponytail: bool,
    pub official_chat: bool,
    pub openai_oauth: bool,
    pub dsh_plugins: bool,
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
    pub skills_state: Option<String>,
    pub skills_error: Option<String>,
    pub memory_state: Option<String>,
    pub memory_error: Option<String>,
    pub ponytail_enabled: Option<bool>,
    pub ponytail_error: Option<String>,
    pub memory_profile: String,
    pub startup_blocked_reason: Option<String>,
    pub capabilities: McodeCapabilities,
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

fn port_available(port: u16) -> bool {
    port != 0 && TcpListener::bind(("127.0.0.1", port)).is_ok()
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
        port,
        miniapp_port,
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
    record.kernel_version = Some("0.1.7".into());
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
    if !path.exists() {
        return Ok(None);
    }
    let record = instance::read_pid(instance::KERNEL_FAMILY_MCODE, &config.instance_id)
        .ok_or_else(|| "MiniMax 进程记录损坏，无法确认旧进程状态，请查看日志".to_string())?;
    // The shell no longer holds the Job/Child here. Even a definitely absent
    // launcher PID cannot prove that all descendants have exited.
    Ok(Some(record.pid))
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

fn resolve_package(config: Option<&Config>, node: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = config.and_then(|value| value.configured_package_path.clone()) {
        return Some(path);
    }
    let mut candidates = Vec::new();
    candidates.push(
        paths::family_runtime_dir(instance::KERNEL_FAMILY_MCODE, settings::current_mode())
            .join("kernels/0.1.7/node_modules/@minimax-webui/minimax-code"),
    );
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        candidates
            .push(PathBuf::from(appdata).join("npm/node_modules/@minimax-webui/minimax-code"));
    }
    if let Some(parent) = node.and_then(Path::parent) {
        candidates.push(parent.join("node_modules/@minimax-webui/minimax-code"));
        if let Some(prefix) = parent.parent() {
            candidates.push(prefix.join("lib/node_modules/@minimax-webui/minimax-code"));
        }
    }
    candidates
        .into_iter()
        .find(|path| McodeAdapter::validate_package(path).is_ok())
}

/// Quit confirmation must not wait for Node or package detection. Unknown or
/// in-flight state is treated as requiring a stop.
pub(crate) fn stop_required_for_exit() -> bool {
    let Ok(_start) = start_lock().try_lock() else {
        return true;
    };
    let Ok(active) = running().try_lock() else {
        return true;
    };
    if active.is_some() {
        return true;
    }
    drop(active);
    let config = match load_config() {
        Ok(Some(config)) => config,
        Ok(None) => return false,
        Err(_) => return true,
    };
    let path = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
    !matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

pub fn status() -> Result<McodeStatus, String> {
    let config = load_config()?;
    let node_path = resolve_node(config.as_ref());
    let package_path = resolve_package(config.as_ref(), node_path.as_deref());
    let node_version = node_path
        .as_deref()
        .and_then(crate::node::detect::version_of);
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
    let error = if active.is_none() {
        config
            .as_ref()
            .map(stale_pid)
            .transpose()?
            .flatten()
            .map(|pid| format!("无法确认旧 MiniMax 进程 {pid} 是否已退出，请先手动检查"))
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
        configured_package_path: config
            .as_ref()
            .and_then(|value| value.configured_package_path.clone()),
        configured_node_path: config
            .as_ref()
            .and_then(|value| value.configured_node_path.clone()),
        package_version: package_path
            .as_deref()
            .filter(|path| McodeAdapter::validate_package(path).is_ok())
            .map(|_| "0.1.7".into()),
        package_path,
        node_path,
        node_version,
        data_dir: state_config.map(|value| {
            paths::instance_dsh_home(instance::KERNEL_FAMILY_MCODE, &value.instance_id)
        }),
        port: state_config.map(|value| value.port),
        miniapp_port: state_config.map(|value| value.miniapp_port),
        error,
        skills_state: active.as_ref().map(|value| value.skills_state.clone()),
        skills_error: active.as_ref().and_then(|value| value.skills_error.clone()),
        memory_state: active.as_ref().map(|value| value.memory_state.clone()),
        memory_error: active.as_ref().and_then(|value| value.memory_error.clone()),
        ponytail_enabled,
        ponytail_error,
        memory_profile: settings::load_for_shell(settings::current_mode()).profile,
        startup_blocked_reason: None,
        capabilities: McodeCapabilities::default(),
    })
}

pub fn save_installation(
    package_path: Option<String>,
    node_path: Option<String>,
) -> Result<(), String> {
    let _start = crate::lock(start_lock());
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
        if !path.is_absolute()
            || !crate::node::detect::version_of(path)
                .as_deref()
                .is_some_and(mcode_node_supported)
        {
            return Err("所选 Node.js 不可用或版本不受支持".into());
        }
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
pub fn install(progress: &mut dyn FnMut(&str)) -> Result<(), String> {
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
    let node = resolve_node(Some(&config)).ok_or("没有可用的 Node.js，请选择安装位置")?;
    let version = crate::node::detect::version_of(&node).ok_or("无法运行所选 Node.js")?;
    if !mcode_node_supported(&version) {
        return Err(format!(
            "Node.js {version} 不支持 MiniMax 0.1.7，请选择兼容版本"
        ));
    }
    let info = crate::node::detect::NodeInfo {
        path: node.to_string_lossy().into_owned(),
        version: Some(version),
        ok: true,
        reason: String::new(),
    };
    let _gate = crate::shell::shutdown::spawn_gate();
    if crate::shell::shutdown::is_shutting_down() {
        return Err("壳正在退出，已取消 MiniMax 安装".into());
    }
    let data_dir = lifecycle::data_dir(instance::KERNEL_FAMILY_DSH);
    let (_, pnpm) = crate::commands::promise_pnpm(&data_dir, &info, &mut *progress)?;
    let package = crate::kernel::mcode_install::install(&node, &pnpm, progress)?;
    config.configured_package_path = Some(package);
    ensure_registered(&config, fresh)?;
    save_config(&config)
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
    if !port_available(config.port) || !port_available(config.miniapp_port) {
        return Err(format!(
            "MiniMax 端口 {} 或 {} 已被占用，请释放端口后重试",
            config.port, config.miniapp_port
        ));
    }
    let node = resolve_node(Some(&config)).ok_or("没有可用的 Node.js，请选择安装位置")?;
    let package = resolve_package(Some(&config), Some(&node))
        .ok_or("未检测到 MiniMax WebUI 0.1.7，请安装或选择安装位置")?;
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
            let error = crate::skills::mcode::reconcile_locked(&lease)
                .err()
                .map(|error| error.to_string());
            if let Some(error) = &error {
                let message = format!("MiniMax 共享技能同步失败：{error}；工作台继续启动");
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
                    if !doc.meta.use_approved {
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
            let token = crate::skills::mcode::arm_start(&lease, generation)
                .map_err(|error| error.to_string())?;
            let state = if error.is_some() {
                "failed"
            } else {
                "pending_verification"
            };
            (
                Some(token),
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
                return Err(format!("{error}；停止结果：{cleanup:?}"));
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
        let owned = crate::lock(running())
            .as_ref()
            .is_some_and(|active| active.process.id() == pid && owns_ports(active));
        if owned && page_ready(config.port) {
            let mut guard = crate::lock(running());
            let active = guard.as_mut().ok_or("MiniMax 启动已取消")?;
            if active.process.id() != pid {
                return Err("MiniMax 启动代次已改变".into());
            }
            active.ready = true;
            return Ok(config.port);
        }
        if Instant::now() >= deadline {
            let cleanup = stop_inner(false);
            return Err(format!(
                "等待 MiniMax 工作台就绪超时，停止结果：{cleanup:?}。请检查端口与依赖"
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
            return Err(format!("{error}；接线回退结果：{cleanup:?}"));
        }
        if let Err(error) = save_ponytail_preference(&config, true) {
            let cleanup = if previous == Some(true) {
                Ok(())
            } else {
                crate::plugins::builtin::mcode_ponytail::disable(&lease)
            };
            return Err(format!("{error}；撤回插件接线结果：{cleanup:?}"));
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
        if let Some(pid) = stale_pid(&config)? {
            return Err(format!("旧 MiniMax 进程 {pid} 状态无法确认，请先检查"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!stop_required_for_exit());

        let start = crate::lock(start_lock());
        assert!(
            stop_required_for_exit(),
            "a start in progress must be treated as active"
        );
        drop(start);
        let active = crate::lock(running());
        assert!(
            stop_required_for_exit(),
            "a busy runtime must be treated as active"
        );
        drop(active);

        let config = new_config().unwrap();
        save_config(&config).unwrap();
        assert!(!stop_required_for_exit());
        let pid = paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &config.instance_id);
        std::fs::create_dir_all(pid.parent().unwrap()).unwrap();
        std::fs::write(&pid, b"unrecognized record").unwrap();
        assert!(
            stop_required_for_exit(),
            "even a corrupt PID record must not be ignored"
        );
        std::fs::remove_file(pid).unwrap();
        std::fs::write(config_path(), b"invalid").unwrap();
        assert!(
            stop_required_for_exit(),
            "corrupt configuration must not be ignored"
        );
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
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

        assert!(!stop_required_for_exit());
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
}
