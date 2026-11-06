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
    pub capabilities: McodeCapabilities,
}

struct Running {
    process: McodeProcess,
    config: Config,
    ready: bool,
    stopped: bool,
    exited: bool,
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

fn config_path() -> PathBuf {
    paths::shell_dir(settings::current_mode()).join("mcode.json")
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

fn ensure_registered(config: &Config) -> Result<(), String> {
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
        capabilities: McodeCapabilities::default(),
    })
}

pub fn save_installation(
    package_path: Option<String>,
    node_path: Option<String>,
) -> Result<(), String> {
    let _start = crate::lock(start_lock());
    if crate::lock(running()).is_some() {
        return Err("请先停止 MiniMax 工作台".into());
    }
    let mut config = match load_config()? {
        Some(config) => config,
        None => new_config()?,
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
    save_config(&config)?;
    ensure_registered(&config)
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

pub fn start(progress: &mut dyn FnMut(&str)) -> Result<u16, String> {
    let generation = START_GENERATION.load(Ordering::SeqCst);
    let _start = crate::lock(start_lock());
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
    let config = match load_config()? {
        Some(config) => config,
        None => {
            let config = new_config()?;
            save_config(&config)?;
            config
        }
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
    ensure_registered(&config)?;
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
    let pid = {
        let _gate = crate::shell::shutdown::spawn_gate();
        if crate::shell::shutdown::is_shutting_down() {
            return Err("壳正在退出，已取消 MiniMax 启动".into());
        }
        let result = with_current_start(generation, |active| {
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
        })?;
        match result {
            Ok(pid) => pid,
            Err((error, true)) => {
                let cleanup = stop_inner(false);
                return Err(format!("{error}；停止结果：{cleanup:?}"));
            }
            Err((error, false)) => return Err(error),
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
                    #[cfg(windows)]
                    {
                        active.process.stop(false)?;
                        active.stopped = true;
                    }
                    if port_available(config.port)
                        && port_available(config.miniapp_port)
                        && (cfg!(windows) && active.stopped
                            || lifecycle::process_is_definitely_gone(pid))
                    {
                        active.stopped = true;
                        let path = paths::instance_pid_file(
                            instance::KERNEL_FAMILY_MCODE,
                            &config.instance_id,
                        );
                        if path.exists() {
                            std::fs::remove_file(path)
                                .map_err(|error| format!("清理 MiniMax 进程记录失败：{error}"))?;
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
            if lifecycle::process_is_definitely_gone(pid)
                && port_available(active.config.port)
                && port_available(active.config.miniapp_port)
            {
                active.stopped = true;
            } else {
                return Err(
                    "MiniMax 已提前退出，但无法确认相关进程是否已停止，请先手动检查".into(),
                );
            }
        }
        if !active.stopped {
            active.process.stop(acknowledge_forced)?;
            active.stopped = true;
        }
        if !port_available(active.config.port) || !port_available(active.config.miniapp_port) {
            return Err("MiniMax 进程已停止，但工作台或应用端口仍被占用；保留进程记录，请检查残留进程后重试".into());
        }
        let path =
            paths::instance_pid_file(instance::KERNEL_FAMILY_MCODE, &active.config.instance_id);
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|error| format!("清理 MiniMax 进程记录失败：{error}"))?;
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
        ensure_registered(&config).unwrap();
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
}
