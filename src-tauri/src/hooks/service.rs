//! Hook 服务的壳级单例：配置应用、启动/停止、状态快照与后台维护。
//!
//! 服务只在应用进程内跑（明确绑定用户确认的可信网卡并保留本机回环），因此不需要跨进程租约；全局运行时
//! 由模块级 `OnceLock<Mutex<_>>` 持有，命令与 setup/退出路径共用同一份状态。
//! 所有启停动作都在锁内完成：启动要绑定端口、打开/迁移数据库，必须串行。
//!
//! 后台维护线程与监听解耦：即使接收器停用，已有收件箱仍要按容量清理并核对
//! 文件是否缺失。首次检查也在后台执行，不阻塞窗口启动；退出时协作取消。

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::hooks::config::{self, HooksConfig, CONFIG_SCHEMA_VERSION};
use crate::hooks::network::{self, LanInterface, LanPolicy};
use crate::hooks::server::{self, Limits, ServerHandle};
use crate::hooks::{blob, store};
use crate::shell::paths;
use crate::shell::process::StateRead;
use crate::shell::{settings, shell_events};

/// 维护周期：清理/恢复是低频动作，10 分钟足够。
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(600);
/// 临时文件与孤儿文件的宽限期：避免删掉正在进行中的上传。
const MAINTENANCE_GRACE_MS: i64 = 24 * 60 * 60 * 1000;

struct Maintenance {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

struct Runtime {
    handle: Option<ServerHandle>,
    maintenance: Option<Maintenance>,
    active_confirmation: Option<(LanPolicy, String)>,
    last_error: Option<String>,
    /// 最近一次成功绑定的内网地址（停止后保留，供面板继续展示）。
    lan_ip: Option<String>,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Mutex::new(Runtime {
            handle: None,
            maintenance: None,
            active_confirmation: None,
            last_error: None,
            lan_ip: None,
        })
    })
}

/// 管理面板与 P3 页面使用的状态快照。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct HooksStatus {
    pub config: HooksConfig,
    /// 运行中 = 此刻确实在监听；`config.enabled` 是用户的期望状态，两者分开，
    /// 端口被占用时不会显示成「已运行」。
    pub running: bool,
    pub bind_addr: Option<String>,
    pub last_error: Option<String>,
    pub endpoint_count: u64,
    pub event_count: u64,
    /// 附件占用（声明大小；含未完成上传的预留）。
    pub storage_bytes: u64,
    /// 全局存储预算；面板据此显示「已用 / 上限」，不在前端写死。
    pub storage_budget_bytes: u64,
    /// 当前成功绑定的内网地址；停止时为空，不展示可复制的过期地址。
    pub lan_ip: Option<String>,
    pub interfaces: Vec<LanInterface>,
}

pub(crate) fn db_path() -> PathBuf {
    paths::hooks_db_file(settings::current_mode())
}

fn bind_addrs_for(ip: Ipv4Addr, port: u16) -> Vec<SocketAddr> {
    #[cfg(test)]
    if ip.is_loopback() {
        return vec![SocketAddr::from((ip, port))];
    }

    vec![
        SocketAddr::from((ip, port)),
        SocketAddr::from(([127, 0, 0, 1], port)),
    ]
}

/// 读配置；损坏或更高版本时返回默认配置与错误原因，调用方不得覆盖原文件。
fn load_config() -> (HooksConfig, Option<String>) {
    match config::load() {
        StateRead::Loaded(config) if config.schema_version > CONFIG_SCHEMA_VERSION => (
            HooksConfig::default(),
            Some(format!(
                "Hook 配置版本 {} 高于本版本支持的 {CONFIG_SCHEMA_VERSION}；请升级应用",
                config.schema_version
            )),
        ),
        StateRead::Loaded(mut config) => {
            config.schema_version = CONFIG_SCHEMA_VERSION;
            (config, None)
        }
        StateRead::Missing => (HooksConfig::default(), None),
        StateRead::Corrupt { reason } => (
            HooksConfig::default(),
            Some(format!(
                "Hook 配置无法读取（{reason}）。请修复或删除 {} 后重启应用",
                config::config_path(settings::current_mode()).display()
            )),
        ),
    }
}

fn build_status(runtime: &Runtime) -> HooksStatus {
    let (config, config_error) = load_config();
    let mut last_error = runtime.last_error.clone().or(config_error);
    let interfaces = match network::interfaces() {
        Ok(items) => items,
        Err(error) => {
            last_error.get_or_insert(error);
            Vec::new()
        }
    };
    let running = runtime.handle.as_ref().is_some_and(ServerHandle::running);
    if runtime.handle.is_some() && !running {
        last_error = Some("可信局域网已变化，接收已暂停；请重新选择网卡并启用".into());
    }
    let (mut endpoint_count, mut event_count, mut storage_bytes) = (0, 0, 0);
    match store::open_if_exists(&db_path()) {
        Ok(Some(connection)) => {
            match store::counts(&connection) {
                Ok((endpoints, events)) => {
                    endpoint_count = endpoints;
                    event_count = events;
                }
                Err(error) => {
                    last_error.get_or_insert_with(|| format!("Hook 收件箱无法读取：{error}"));
                }
            }
            match store::used_bytes(&connection) {
                Ok(bytes) => storage_bytes = bytes,
                Err(error) => {
                    last_error.get_or_insert_with(|| format!("Hook 存储占用无法读取：{error}"));
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            last_error.get_or_insert_with(|| format!("Hook 收件箱无法打开：{error}"));
        }
    }
    HooksStatus {
        config,
        running,
        bind_addr: runtime
            .handle
            .as_ref()
            .filter(|handle| handle.running())
            .map(|handle| handle.addr.to_string()),
        last_error,
        endpoint_count,
        event_count,
        storage_bytes,
        storage_budget_bytes: store::STORAGE_BUDGET_BYTES,
        lan_ip: running.then(|| runtime.lan_ip.clone()).flatten(),
        interfaces,
    }
}

/// 当前状态。读取配置与数据库计数，都是小文件操作，调用方在阻塞线程上跑。
pub(crate) fn status() -> HooksStatus {
    let runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    build_status(&runtime)
}

fn start_locked(runtime: &mut Runtime, config: &HooksConfig) -> Result<(), String> {
    // 先撤销跨重启自动恢复许可，再核验网络。启动时发现变更也必须重新确认。
    config::write_confirmation(&config.confirmation_id, false)?;
    let policy = LanPolicy::resolve(config, &network::interfaces()?)?;
    let ip: Ipv4Addr = policy
        .interface
        .address
        .parse()
        .map_err(|_| "网卡地址无效")?;
    // 先准备目录与数据库：打不开时不要占住端口，错误也能说清是数据问题还是端口问题。
    blob::ensure_dirs()?;
    let connection = store::open(&db_path()).map_err(|error| {
        format!(
            "Hook 收件箱数据库无法打开：{error}。请在「查看日志」确认 {} 是否可访问",
            db_path().display()
        )
    })?;
    drop(connection);
    ensure_maintenance_locked(runtime);
    runtime.lan_ip = None;
    let handle = server::start_with_policy(
        bind_addrs_for(ip, config.port),
        db_path(),
        Limits::default(),
        Some(policy.clone()),
    )?;
    let addr = handle.addr;
    runtime.lan_ip = (!addr.ip().is_unspecified()).then(|| addr.ip().to_string());
    runtime.handle = Some(handle);
    runtime.active_confirmation = Some((policy, config.confirmation_id.clone()));
    runtime.last_error = None;
    let line = format!("Hook 接收器已启动：{addr}（所选可信局域网与本机回环）");
    shell_events::record("hooks", &line);
    Ok(())
}

fn stop_locked(runtime: &mut Runtime, allow_clean: bool) {
    if let Some(handle) = runtime.handle.take() {
        // 网络检测线程不写配置。先回收旧代全部线程，再处理旧代 marker，不能污染新代。
        let joined = handle.stop_cleanly();
        if let Some((policy, id)) = runtime.active_confirmation.take() {
            if allow_clean && joined && !policy.revoked() && policy.available() {
                if let Err(error) = config::write_confirmation(&id, true) {
                    runtime.last_error = Some(error.clone());
                    shell_events::record("hooks", &error);
                }
            }
        }
        runtime.lan_ip = None;
        shell_events::record("hooks", "Hook 接收器已停止");
    }
}

fn stop_maintenance(runtime: &mut Runtime) {
    if let Some(maintenance) = runtime.maintenance.take() {
        maintenance.stop.store(true, Ordering::SeqCst);
        maintenance.handle.thread().unpark();
        let _ = maintenance.handle.join();
    }
}

/// 显式配置：停旧并回收 → 保存新确认代次 → 写 armed → 核验网络并启动。
/// 启动失败保留 enabled 意图但不伪报运行；armed/缺失标记要求重新确认。
/// 若 armed 写入失败且旧 clean 标记保留，不能保证跨重启继续阻断，错误明确披露。
pub(crate) fn apply_config(mut config: HooksConfig) -> Result<HooksStatus, String> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "Hook 配置版本必须为 {CONFIG_SCHEMA_VERSION}，拒绝写入 {}",
            config.schema_version
        ));
    }
    if config.port == 0 {
        return Err("Hook 端口不能为 0；请填写 1–65535 之间的端口".into());
    }
    if config.allowed_subnets.len() > 32 {
        return Err("额外可信网段最多 32 项".into());
    }
    for subnet in &config.allowed_subnets {
        network::Subnet::parse(subnet)?;
    }
    let mut runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    config::check_writable()?;
    // 显式保存 enabled=true 才是重新确认；停用保存保留已确认代次，避免清掉旧代次。
    if config.enabled {
        config.confirmation_id = format!("confirm_{:032x}", rand::random::<u128>());
    } else {
        config.confirmation_id = load_config().0.confirmation_id;
    }
    // 先落盘再停旧监听：写入失败时旧服务继续工作，不把可用接收器停成无服务状态。
    config::save(&config)?;
    stop_locked(&mut runtime, false);
    if config.enabled {
        if let Err(error) = start_locked(&mut runtime, &config) {
            runtime.last_error = Some(error.clone());
            shell_events::record("hooks", &format!("Hook 接收器启动失败：{error}"));
            return Ok(build_status(&runtime));
        }
    }
    Ok(build_status(&runtime))
}

/// 应用启动时按配置恢复监听，并启动后台维护。失败只记录，不阻断应用启动。
pub(crate) fn auto_start() {
    let mut runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (config, config_error) = load_config();
    // 维护线程与监听解耦：配置损坏或网卡暂时不可用时也要继续清理/核对收件箱。
    ensure_maintenance_locked(&mut runtime);
    if let Some(error) = config_error {
        runtime.last_error = Some(error.clone());
        shell_events::record("hooks", &format!("Hook 配置不可用：{error}"));
        return;
    }
    if !config.enabled || runtime.handle.is_some() {
        ensure_maintenance_locked(&mut runtime);
        return;
    }
    if !config::auto_start_confirmed(&config) {
        runtime.last_error = Some(
            "局域网需要重新确认（网络变化、异常退出或确认状态不可用）；请选择可信网卡并启用".into(),
        );
        return;
    }
    if let Err(error) = start_locked(&mut runtime, &config) {
        runtime.last_error = Some(error.clone());
        shell_events::record("hooks", &format!("Hook 接收器启动失败：{error}"));
    }
}

/// 关闭接收是独立动作，不校验或保存尚未提交的网络草稿。
/// 即使配置不可写，也先停止网络服务；armed marker 保证重启不越过再次确认。
pub(crate) fn disable() -> Result<HooksStatus, String> {
    let mut runtime = runtime().lock().unwrap_or_else(|p| p.into_inner());
    stop_locked(&mut runtime, false);
    let (mut config, error) = load_config();
    config.enabled = false;
    let result = match error {
        Some(error) => Err(error),
        None => config::check_writable().and_then(|()| config::save(&config)),
    };
    if let Err(error) = result {
        let error = format!("接收器已停止，但停用设置未能保存：{error}");
        shell_events::record("hooks", &error);
        runtime.last_error = Some(error.clone());
        return Err(error);
    }
    runtime.last_error = None;
    Ok(build_status(&runtime))
}

/// 应用退出时停止接收与维护线程。幂等；即使服务没起来也只是空操作。
pub(crate) fn stop() {
    let mut runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    runtime.last_error = None;
    stop_locked(&mut runtime, true);
    stop_maintenance(&mut runtime);
}

fn ensure_maintenance_locked(runtime: &mut Runtime) {
    if runtime.maintenance.is_some() {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let spawned = thread::Builder::new()
        .name("dsh-hooks-maintenance".into())
        .spawn(move || {
            maintenance_loop(&thread_stop, MAINTENANCE_INTERVAL, || {
                maintenance_until_stopped(&thread_stop);
            });
        });
    match spawned {
        Ok(handle) => {
            runtime.maintenance = Some(Maintenance { stop, handle });
        }
        Err(error) => {
            runtime.last_error = Some(format!("Hook 后台维护线程未启动：{error}"));
            shell_events::record("hooks", &format!("Hook 后台维护线程未启动：{error}"));
        }
    }
}

/// 单一后台入口执行首次及周期维护；unpark 让退出不必等待整个维护周期。
fn maintenance_loop(stop: &AtomicBool, interval: Duration, mut run: impl FnMut()) {
    while !stop.load(Ordering::SeqCst) {
        run();
        let deadline = std::time::Instant::now() + interval;
        while !stop.load(Ordering::SeqCst) {
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                break;
            };
            // park 允许伪唤醒，始终重查截止时间，避免额外扫描。
            thread::park_timeout(remaining);
        }
    }
}

/// 一次维护：核对已存储文件、清理过期内容、回收临时与孤儿文件。
///
/// 幂等；失败只记录，不阻断调用方。DB 不存在时什么也不做。
#[cfg(test)]
fn maintenance_once() {
    maintenance_until_stopped(&AtomicBool::new(false));
}

fn maintenance_until_stopped(stop: &AtomicBool) {
    if stop.load(Ordering::SeqCst) {
        return;
    }
    let path = db_path();
    if !path.is_file() {
        return;
    }
    let mut connection = match store::open(&path) {
        Ok(connection) => connection,
        Err(error) => {
            shell_events::record("hooks", &format!("Hook 维护无法打开数据库：{error}"));
            return;
        }
    };
    let now = crate::shell::process::epoch_millis() as i64;
    let mut problems = 0usize;

    // 1) 启动恢复：数据库说已存储，但文件丢了或大小不符 → 事件不可处理。
    match store::stored_attachments(&connection) {
        Ok(rows) => {
            for (event_id, attachment_id, rel, size) in rows {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                if blob::exists_with_size(&rel, size) {
                    continue;
                }
                if store::mark_event_unavailable(&connection, &event_id).unwrap_or(false) {
                    problems += 1;
                    shell_events::record(
                        "hooks",
                        &format!(
                            "附件文件缺失：event={event_id} attachment={attachment_id} rel={rel}；事件已标记不可用"
                        ),
                    );
                }
            }
        }
        Err(error) => {
            shell_events::record("hooks", &format!("Hook 附件恢复检查失败：{error}"));
        }
    }

    if stop.load(Ordering::SeqCst) {
        return;
    }
    // 2) 接收超时/容量清理：先清数据库内容，再删文件；删文件失败时文件成为孤儿，
    //    下一轮按宽限期回收，不会再把内容当可处理事件提供。
    match store::cleanup_content(&mut connection, now) {
        Ok(targets) => {
            if !targets.is_empty() {
                let files: usize = targets.iter().map(|target| target.rels.len()).sum();
                shell_events::record(
                    "hooks",
                    &format!(
                        "维护清理：{} 个事件、{files} 个附件；含接收超时与容量清理",
                        targets.len()
                    ),
                );
            }
            for target in targets {
                for rel in target.rels {
                    // 数据库已清除的内容可留作孤儿，下次维护继续回收。
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Err(error) = blob::remove_rel(&rel) {
                        shell_events::record(
                            "hooks",
                            &format!("清理附件文件失败（event={}）：{error}", target.event_id),
                        );
                    }
                }
            }
        }
        Err(error) => {
            shell_events::record("hooks", &format!("Hook 过期清理失败：{error}"));
        }
    }

    if stop.load(Ordering::SeqCst) {
        return;
    }
    // 3) 临时半截文件与没有数据库记录的孤儿文件。
    let removed_temp = blob::cleanup_temp(now - MAINTENANCE_GRACE_MS);
    if stop.load(Ordering::SeqCst) {
        return;
    }
    let removed_orphans = match store::known_storage_rels(&connection) {
        Ok(known) => blob::remove_orphans(&known, now - MAINTENANCE_GRACE_MS),
        Err(_) => 0,
    };
    if removed_temp > 0 || removed_orphans > 0 {
        shell_events::record(
            "hooks",
            &format!("Hook 维护清理：临时文件 {removed_temp} 个，孤儿文件 {removed_orphans} 个"),
        );
    }
    if problems > 0 {
        shell_events::record("hooks", &format!("Hook 恢复发现 {problems} 个事件附件缺失"));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::blob;
    use crate::hooks::store::{self, NewAttachment, NewEvent};
    use crate::tests::scoped_xlink_home;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    // 全局运行时是进程级单例：服务用例必须串行，且结束前 stop。
    fn temp_home(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-hooks-service-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("创建临时 home");
        dir
    }

    fn copy_dir(source: &std::path::Path, target: &std::path::Path) {
        std::fs::create_dir_all(target).expect("建目标目录");
        for entry in std::fs::read_dir(source).expect("读源目录").flatten() {
            let path = entry.path();
            let destination = target.join(entry.file_name());
            if path.is_dir() {
                copy_dir(&path, &destination);
            } else {
                std::fs::copy(&path, &destination).expect("复制文件");
            }
        }
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind("0.0.0.0:0").expect("临时端口");
        listener.local_addr().expect("地址").port()
    }

    fn connected(port: u16) -> bool {
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    }

    #[test]
    fn explicit_binding_keeps_loopback_without_wildcard() {
        assert_eq!(
            bind_addrs_for("10.20.1.2".parse().unwrap(), 17890),
            vec![
                "10.20.1.2:17890".parse().unwrap(),
                "127.0.0.1:17890".parse().unwrap()
            ]
        );
    }

    fn selected_config(port: u16) -> (HooksConfig, network::InterfaceGuard) {
        // Only discovery is injected. Real service startup, source policy, HTTP and stop run on loopback.
        let interface = LanInterface {
            name: "test-loopback".into(),
            address: "127.0.0.1".into(),
            netmask: "255.0.0.0".into(),
            cidr: "10.0.0.0/8".into(),
        };
        let guard = network::test_interfaces(vec![interface.clone()]);
        (
            HooksConfig {
                enabled: true,
                port,
                interface_name: interface.name,
                bind_ip: interface.address,
                netmask: interface.netmask,
                ..HooksConfig::default()
            },
            guard,
        )
    }

    #[test]
    fn legacy_enabled_config_requires_network_confirmation() {
        let _serial = network::test_lock();
        let home = temp_home("legacy-network");
        let _guard = scoped_xlink_home(&home);
        stop();
        let _network = network::test_interfaces(Vec::new());
        let dir = config::ensure_dir().unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"schema_version":1,"enabled":true,"port":17890}"#,
        )
        .unwrap();
        auto_start();
        let state = status();
        assert!(!state.running);
        assert!(
            state.last_error.as_ref().unwrap().contains("重新确认"),
            "{:?}",
            state.last_error
        );
        assert_eq!(state.config.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(
            runtime()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .maintenance
                .is_some(),
            "配置不可用时后台维护仍应运行"
        );
        stop();
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn maintenance_runs_immediately_then_waits_and_stops_without_another_pass() {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (sent, received) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            maintenance_loop(&worker_stop, Duration::from_secs(600), || {
                sent.send(()).expect("报告维护执行");
            });
        });
        received
            .recv_timeout(Duration::from_secs(2))
            .expect("首次维护立即在后台执行");
        worker.thread().unpark();
        assert!(
            received.recv_timeout(Duration::from_millis(50)).is_err(),
            "提前唤醒不得重复扫描"
        );
        stop.store(true, Ordering::SeqCst);
        worker.thread().unpark();
        worker.join().expect("维护退出");
        assert!(received.try_recv().is_err(), "停止后不得继续维护");
    }

    #[test]
    fn cancelled_maintenance_does_not_create_a_database() {
        let home = temp_home("cancel-maintenance");
        let _guard = scoped_xlink_home(&home);
        maintenance_until_stopped(&AtomicBool::new(true));
        assert!(!db_path().exists());
        std::fs::remove_dir_all(&home).ok();
    }

    fn wait_until_paused() {
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        while runtime()
            .lock()
            .unwrap()
            .handle
            .as_ref()
            .is_some_and(ServerHandle::running)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "network watchdog did not pause"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn network_pause_survives_stop_and_restart_until_explicit_confirmation() {
        let _serial = network::test_lock();
        let home = temp_home("persistent-pause");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, network) = selected_config(free_port());
        let interfaces = network::interfaces().unwrap();
        let first = apply_config(selected.clone()).unwrap();
        assert!(first.running);
        assert!(
            !config::auto_start_confirmed(&first.config),
            "running marker is armed"
        );
        network.replace(Vec::new());
        wait_until_paused();
        network.replace(interfaces);
        stop();
        auto_start();
        let paused = status();
        assert!(!paused.running);
        assert!(
            paused
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("重新确认")),
            "blocked auto-start must explain re-confirmation: {:?}",
            paused.last_error
        );
        assert!(
            paused.config.enabled,
            "keep user intent, not silently disable"
        );
        let restarted = apply_config(selected).unwrap();
        assert!(restarted.running);
        assert_ne!(
            first.config.confirmation_id,
            restarted.config.confirmation_id
        );
        stop();
        assert!(config::auto_start_confirmed(&status().config));
        auto_start();
        assert!(status().running, "clean exit may auto restore");
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn changed_network_at_auto_start_revokes_future_automatic_restart() {
        let _serial = network::test_lock();
        let home = temp_home("startup-network-change");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, network) = selected_config(free_port());
        let interfaces = network::interfaces().unwrap();
        assert!(apply_config(selected).unwrap().running);
        stop();
        assert!(config::auto_start_confirmed(&status().config));
        network.replace(Vec::new());
        auto_start();
        assert!(!status().running);
        network.replace(interfaces);
        auto_start();
        assert!(!status().running);
        assert!(
            status()
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("重新确认")),
            "blocked auto-start must explain re-confirmation: {:?}",
            status().last_error
        );
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn armed_marker_or_missing_marker_never_autostarts() {
        let _serial = network::test_lock();
        let home = temp_home("crash-marker");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, _network) = selected_config(free_port());
        let started = apply_config(selected).unwrap();
        assert!(started.running);
        // Simulate abrupt exit cleanup without writing clean (the real crash leaves armed on disk).
        {
            let mut runtime = runtime().lock().unwrap();
            stop_locked(&mut runtime, false);
        }
        auto_start();
        assert!(!status().running);
        std::fs::remove_file(config::confirmation_path()).unwrap();
        auto_start();
        assert!(!status().running, "missing marker must fail closed");
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failed_arm_can_leave_old_clean_marker_and_reports_persistence_limit() {
        let _serial = network::test_lock();
        let home = temp_home("arm-preserved-clean");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, _network) = selected_config(free_port());
        assert!(apply_config(selected).unwrap().running);
        stop();
        assert!(config::auto_start_confirmed(&status().config));
        let old_marker = std::fs::read(config::confirmation_path()).unwrap();
        config::fail_next_confirmation_write();
        auto_start();
        let failed = status();
        assert!(!failed.running);
        assert!(failed.last_error.unwrap().contains("不能保证跨重启"));
        assert_eq!(
            old_marker,
            std::fs::read(config::confirmation_path()).unwrap()
        );
        assert!(
            config::auto_start_confirmed(&failed.config),
            "a failed write cannot revoke persisted clean state"
        );
        auto_start();
        assert!(
            status().running,
            "after storage recovers the retained clean marker may auto-start"
        );
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn confirmation_write_failure_prevents_bind_and_clean_failure_prevents_restart() {
        let _serial = network::test_lock();
        let home = temp_home("confirmation-io");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, _network) = selected_config(free_port());
        config::ensure_dir().unwrap();
        let marker = config::confirmation_path();
        std::fs::create_dir(&marker).unwrap();
        let failed = apply_config(selected.clone()).unwrap();
        assert!(!failed.running);
        assert!(!connected(selected.port));
        assert!(failed
            .last_error
            .unwrap()
            .contains("无法保存 Hook 网络确认状态"));
        std::fs::remove_dir(&marker).unwrap();
        assert!(apply_config(selected).unwrap().running);
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        stop();
        assert!(status()
            .last_error
            .unwrap()
            .contains("无法保存 Hook 网络确认状态"));
        auto_start();
        assert!(!status().running);
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn disable_preserves_saved_network_and_does_not_clear_revocation() {
        let _serial = network::test_lock();
        let home = temp_home("disable-revoked");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, network) = selected_config(free_port());
        let before = apply_config(selected).unwrap();
        network.replace(Vec::new());
        wait_until_paused();
        let disabled = disable().expect("保存停用设置");
        assert!(!disabled.running);
        assert!(!disabled.config.enabled);
        assert!(
            disabled.last_error.is_none(),
            "deliberate stop must not produce a confirmation warning: {:?}",
            disabled.last_error
        );
        assert_eq!(before.config.bind_ip, disabled.config.bind_ip);
        assert_eq!(
            before.config.confirmation_id,
            disabled.config.confirmation_id
        );
        assert!(!config::auto_start_confirmed(&disabled.config));
        let confirmation_id = disabled.config.confirmation_id.clone();
        let saved = apply_config(HooksConfig {
            enabled: false,
            ..disabled.config.clone()
        })
        .expect("停用状态保存设置");
        assert_eq!(
            saved.config.confirmation_id, confirmation_id,
            "停用状态保存设置不得清空已保存的确认代次"
        );
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn failed_config_save_keeps_old_listener_running() {
        let _serial = network::test_lock();
        let home = temp_home("failed-config-save");
        let _home = scoped_xlink_home(&home);
        stop();
        let port = free_port();
        let (selected, _network) = selected_config(port);
        assert!(apply_config(selected.clone()).expect("启用").running);
        config::fail_next_save();
        let mut replacement = selected.clone();
        replacement.port = free_port();
        let error = apply_config(replacement).expect_err("保存失败应报错");
        assert!(error.contains("注入"), "{error}");
        let state = status();
        assert!(state.running, "旧监听应保持运行：{:?}", state.last_error);
        assert_eq!(state.config.port, port, "旧配置不得被半截替换");
        stop();
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn failed_disable_save_reports_failure_but_stops_the_listener() {
        let _serial = network::test_lock();
        let home = temp_home("failed-disable-save");
        let _home = scoped_xlink_home(&home);
        stop();
        let (selected, _network) = selected_config(free_port());
        assert!(apply_config(selected).unwrap().running);

        config::fail_next_save();
        let error = disable().expect_err("停用设置保存失败必须报告给调用方");
        let state = status();
        assert!(!state.running);
        assert!(state.config.enabled, "磁盘仍是上一次成功保存的设置");
        assert_eq!(state.last_error.as_deref(), Some(error.as_str()));
        assert!(!config::auto_start_confirmed(&state.config));
        let disabled = disable().expect("修复写入后可重试停用");
        assert!(!disabled.config.enabled);
        assert!(disabled.last_error.is_none());
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn concurrent_saves_keep_disk_and_listener_consistent() {
        let _serial = network::test_lock();
        let home = temp_home("concurrent-config");
        let _guard = scoped_xlink_home(&home);
        stop();
        let (first, _network) = selected_config(free_port());
        let mut second = first.clone();
        second.port = free_port();
        let a = std::thread::spawn(move || apply_config(first).unwrap());
        let b = std::thread::spawn(move || apply_config(second).unwrap());
        a.join().unwrap();
        b.join().unwrap();
        let state = status();
        assert!(state.running, "{:?}", state.last_error);
        let addr: SocketAddr = state.bind_addr.unwrap().parse().unwrap();
        assert_eq!(addr.port(), state.config.port);
        stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn apply_config_starts_and_stops_the_listener() {
        let _serial = network::test_lock();
        let home = temp_home("apply");
        let _guard = scoped_xlink_home(&home);
        stop();
        let port = free_port();
        let (selected, _network_guard) = selected_config(port);
        let status = apply_config(selected).expect("启用");
        assert!(status.running, "启用后应处于监听状态");
        assert!(status.last_error.is_none());
        assert_eq!(status.config.port, port);
        assert!(connected(port), "端口应可连接");

        let status = apply_config(HooksConfig {
            enabled: false,
            port,
            ..HooksConfig::default()
        })
        .expect("停用");
        assert!(!status.running);
        assert!(!connected(port), "停用后端口必须不再接受连接");
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    // 测试用假网卡是回环；真实私网绑定与来源范围由 e2e 和 LanPolicy 用例覆盖。
    fn loopback_bind_serves_http_for_local_sender() {
        let _serial = network::test_lock();
        let home = temp_home("lan-http");
        let _guard = scoped_xlink_home(&home);
        stop();
        let port = free_port();
        let (selected, _network_guard) = selected_config(port);
        let status = apply_config(selected).expect("启用");
        let bind = status.bind_addr.clone().expect("绑定地址");
        let mut stream = std::net::TcpStream::connect(&bind).expect("连接内网绑定地址");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("读超时");
        stream
            .write_all(
                format!("GET / HTTP/1.1\r\nHost: {bind}\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .expect("发送请求");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("读取响应");
        assert!(
            response.starts_with("HTTP/1.1 404"),
            "内网地址应能完成请求：{response}"
        );
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn auto_start_follows_saved_config() {
        let _serial = network::test_lock();
        let home = temp_home("autostart");
        let _guard = scoped_xlink_home(&home);
        stop();
        let port = free_port();
        let (selected, _network_guard) = selected_config(port);
        apply_config(selected).expect("明确确认启用");
        stop();
        auto_start();
        assert!(status().running, "auto_start 应按配置启动");
        let maintenance_id = runtime()
            .lock()
            .expect("运行时")
            .maintenance
            .as_ref()
            .expect("维护线程")
            .handle
            .thread()
            .id();
        auto_start();
        assert_eq!(
            runtime()
                .lock()
                .expect("运行时")
                .maintenance
                .as_ref()
                .expect("维护线程")
                .handle
                .thread()
                .id(),
            maintenance_id,
            "重复启动只保留一个后台维护线程"
        );
        stop();
        assert!(!status().running);

        config::save(&HooksConfig {
            enabled: false,
            port,
            ..HooksConfig::default()
        })
        .expect("保存停用配置");
        auto_start();
        assert!(!status().running);
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn port_conflict_is_reported_without_faking_running() {
        let _serial = network::test_lock();
        let home = temp_home("conflict");
        let _guard = scoped_xlink_home(&home);
        stop();
        // 占住接收器将要绑定的第一个地址（内网网卡或通配地址），不能只占回环：
        // Windows 允许具体地址与通配地址同端口共存。
        let port = free_port();
        let (selected, _network_guard) = selected_config(port);
        let occupied_addr = bind_addrs_for(selected.bind_ip.parse().unwrap(), port)
            .into_iter()
            .next()
            .expect("监听地址");
        let occupied = TcpListener::bind(occupied_addr).expect("占住端口");
        let (selected, _network_guard) = selected_config(port);
        let status = apply_config(selected).expect("配置可保存");
        assert!(!status.running, "端口被占用时不得报告运行中");
        let error = status.last_error.clone().unwrap_or_default();
        assert!(error.contains("无法监听"), "错误应说明监听失败：{error}");
        assert!(status.config.enabled, "期望状态保留，下次启动重试");

        drop(occupied);
        let (selected, _network_guard) = selected_config(port);
        let status = apply_config(selected).expect("重试");
        assert!(
            status.running,
            "端口释放后应能启动：{:?}",
            status.last_error
        );
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn corrupt_config_is_reported_and_never_overwritten() {
        let _serial = network::test_lock();
        let home = temp_home("corrupt");
        let _guard = scoped_xlink_home(&home);
        stop();
        let dir = paths::hooks_dir(settings::current_mode());
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("config.json");
        std::fs::write(&path, b"{broken").expect("写坏配置");
        let state = status();
        assert!(!state.running);
        assert!(state
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("Hook 配置")));
        auto_start();
        assert!(!status().running, "损坏配置不得启动服务");
        let error = apply_config(HooksConfig::default()).expect_err("损坏配置不得覆盖");
        assert!(error.contains("Hook 配置"), "{error}");
        assert_eq!(std::fs::read(&path).expect("原文件"), b"{broken");
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn crash_after_archive_is_reclaimed_as_orphan_on_maintenance() {
        let _serial = network::test_lock();
        let home = temp_home("crash-orphan");
        let _guard = scoped_xlink_home(&home);
        stop();
        blob::ensure_dirs().expect("建 hook 目录");
        let mut connection = store::open(&db_path()).expect("建库");
        let (endpoint, _) = store::create_endpoint(
            &connection,
            "崩溃测试",
            &["text".to_string(), "parameters".to_string()],
        )
        .expect("建接入点");
        let attachments = [NewAttachment {
            client_id: "image",
            kind: "image",
            filename: "a.png",
            media_type: "image/png",
            encoding: None,
            size_bytes: 3,
        }];
        let parameters = serde_json::json!({});
        let event = NewEvent {
            external_id: "build:crash-orphan",
            fingerprint: "fingerprint-crash-orphan",
            event_type: "build.materials",
            occurred_at: 1_763_172_000_000,
            title: "崩溃恢复",
            text: "内容",
            parameters: &parameters,
            sender_policy: None,
            commit: false,
            attachments: &attachments,
        };
        let now = crate::shell::process::epoch_millis() as i64;
        let (row, _) =
            store::create_event(&mut connection, &endpoint.id, &event, now).expect("创建事件");
        let attachment = store::event_attachments(&connection, &row.id)
            .expect("附件")
            .remove(0);
        store::begin_upload(
            &connection,
            &row.id,
            &attachment.id,
            now + 10,
            "test-attempt",
        )
        .expect("租约");
        let path = blob::path_for_rel(&format!("{}/{}", row.id, attachment.id)).expect("路径");
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建事件目录");
        std::fs::write(&path, b"abc").expect("模拟已归档但未写库");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("打开")
            .set_modified(old)
            .expect("改 mtime");

        maintenance_once();

        assert!(!path.exists(), "没有数据库记录的归档文件必须被回收");
        let kept = store::event_detail(&connection, &row.id)
            .expect("查询")
            .expect("事件仍在");
        assert_eq!(kept.state, "receiving");
        drop(connection);
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn cleanup_failure_is_logged_and_still_clears_database_content() {
        let _serial = network::test_lock();
        let home = temp_home("cleanup-failure");
        let _guard = scoped_xlink_home(&home);
        stop();
        blob::ensure_dirs().expect("建 hook 目录");
        let mut connection = store::open(&db_path()).expect("建库");
        let (endpoint, _) = store::create_endpoint(
            &connection,
            "清理失败测试",
            &["text".to_string(), "parameters".to_string()],
        )
        .expect("建接入点");
        let attachments = [NewAttachment {
            client_id: "console",
            kind: "log",
            filename: "console.log",
            media_type: "text/plain",
            encoding: Some("utf-8"),
            size_bytes: 3,
        }];
        let parameters = serde_json::json!({});
        let event = NewEvent {
            external_id: "build:cleanup-failure",
            fingerprint: "fingerprint-cleanup-failure",
            event_type: "build.materials",
            occurred_at: 1_763_172_000_000,
            title: "清理失败",
            text: "内容",
            parameters: &parameters,
            sender_policy: None,
            commit: false,
            attachments: &attachments,
        };
        let now = crate::shell::process::epoch_millis() as i64;
        let (row, _) =
            store::create_event(&mut connection, &endpoint.id, &event, now).expect("创建事件");
        let attachment = store::event_attachments(&connection, &row.id)
            .expect("附件")
            .remove(0);
        store::begin_upload(
            &connection,
            &row.id,
            &attachment.id,
            now + 10,
            "test-attempt",
        )
        .expect("租约");
        let rel = format!("{}/{}", row.id, attachment.id);
        let path = blob::path_for_rel(&rel).expect("路径");
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建目录");
        std::fs::write(&path, b"abc").expect("写文件");
        store::finish_upload(
            &connection,
            &row.id,
            &attachment.id,
            "sha",
            3,
            &rel,
            now + 20,
            "test-attempt",
        )
        .expect("完成上传");
        store::commit_event(&mut connection, &endpoint.id, &row.id, now + 30).expect("提交");
        connection
            .execute(
                "UPDATE events SET state = 'expired' WHERE id = ?1",
                [&row.id],
            )
            .expect("模拟过期");
        std::fs::remove_file(&path).expect("删文件");
        std::fs::create_dir_all(&path).expect("用目录占住文件位置");

        maintenance_once();

        let detail = store::event_detail(&connection, &row.id)
            .expect("查询")
            .expect("墓碑仍在");
        assert_eq!(detail.state, "expired");
        assert_eq!(detail.attachment_count, 0, "数据库内容仍要清掉");
        let logs: String = std::fs::read_dir(crate::shell::process::shell_logs_dir())
            .expect("日志目录")
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
            .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
            .collect();
        assert!(
            logs.contains("清理附件文件失败"),
            "文件删不掉必须留下可追踪日志：{logs}"
        );

        // 模拟重启后故障消失：目录换回普通文件，下一次维护按孤儿回收。
        std::fs::remove_dir_all(&path).expect("移除占位目录");
        std::fs::write(&path, b"late").expect("写回文件");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("打开")
            .set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 60 * 60),
            )
            .expect("改 mtime");
        drop(connection);
        maintenance_once();
        assert!(!path.exists(), "重启后的维护应回收上次没删掉的孤儿文件");

        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn disk_guard_refuses_when_free_space_is_below_requirement() {
        let _serial = network::test_lock();
        let home = temp_home("disk-guard");
        let _guard = scoped_xlink_home(&home);
        stop();
        blob::ensure_dirs().expect("建 hook 目录");
        let error = blob::disk_has_room(u64::MAX).expect_err("必然拒绝");
        assert!(error.contains("磁盘剩余空间不足"), "{error}");
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn backup_and_restore_drill_keeps_event_and_attachment() {
        let _serial = network::test_lock();
        let home = temp_home("backup");
        let _guard = scoped_xlink_home(&home);
        stop();
        blob::ensure_dirs().expect("建 hook 目录");
        let mut connection = store::open(&db_path()).expect("建库");
        let (endpoint, _) = store::create_endpoint(
            &connection,
            "备份测试",
            &["text".to_string(), "parameters".to_string()],
        )
        .expect("建接入点");
        let attachments = [NewAttachment {
            client_id: "console",
            kind: "log",
            filename: "console.log",
            media_type: "text/plain",
            encoding: Some("utf-8"),
            size_bytes: 3,
        }];
        let parameters = serde_json::json!({"build": 1});
        let event = NewEvent {
            external_id: "build:backup",
            fingerprint: "fingerprint-backup",
            event_type: "build.materials",
            occurred_at: 1_763_172_000_000,
            title: "备份恢复",
            text: "内容",
            parameters: &parameters,
            sender_policy: None,
            commit: false,
            attachments: &attachments,
        };
        let now = crate::shell::process::epoch_millis() as i64;
        let (row, _) =
            store::create_event(&mut connection, &endpoint.id, &event, now).expect("创建事件");
        let attachment = store::event_attachments(&connection, &row.id)
            .expect("附件")
            .remove(0);
        store::begin_upload(
            &connection,
            &row.id,
            &attachment.id,
            now + 10,
            "test-attempt",
        )
        .expect("租约");
        let rel = format!("{}/{}", row.id, attachment.id);
        let path = blob::path_for_rel(&rel).expect("路径");
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建目录");
        std::fs::write(&path, b"abc").expect("写附件");
        store::finish_upload(
            &connection,
            &row.id,
            &attachment.id,
            "sha",
            3,
            &rel,
            now + 20,
            "test-attempt",
        )
        .expect("完成上传");
        store::commit_event(&mut connection, &endpoint.id, &row.id, now + 30).expect("提交");
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .expect("合并 WAL");
        drop(connection);

        let hooks_dir = db_path().parent().expect("hooks 目录").to_path_buf();
        let backup = home.join("backup-hooks");
        copy_dir(&hooks_dir, &backup);
        std::fs::remove_dir_all(&hooks_dir).expect("模拟丢失");
        copy_dir(&backup, &hooks_dir);

        let connection = store::open(&db_path()).expect("恢复后打开");
        let restored = store::event_detail(&connection, &row.id)
            .expect("查询")
            .expect("事件还在");
        assert_eq!(restored.title, "备份恢复");
        assert_eq!(restored.body_text, "内容");
        let restored_attachment = store::event_attachments(&connection, &row.id)
            .expect("附件")
            .remove(0);
        assert_eq!(restored_attachment.state, store::ATTACHMENT_STORED);
        assert!(blob::exists_with_size(&rel, 3), "附件文件也要恢复");
        drop(connection);
        stop();
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn maintenance_marks_missing_attachment_files_unavailable() {
        let _serial = network::test_lock();
        let home = temp_home("maintenance");
        let _guard = scoped_xlink_home(&home);
        stop();
        blob::ensure_dirs().expect("建 hook 目录");
        let mut connection = store::open(&db_path()).expect("建库");
        let (endpoint, _) = store::create_endpoint(
            &connection,
            "维护测试",
            &["text".to_string(), "parameters".to_string()],
        )
        .expect("建接入点");
        let parameters = serde_json::json!({"build": 1});
        let attachments = [NewAttachment {
            client_id: "image",
            kind: "image",
            filename: "a.png",
            media_type: "image/png",
            encoding: None,
            size_bytes: 3,
        }];
        let event = NewEvent {
            external_id: "build:maintenance",
            fingerprint: "fingerprint-maintenance",
            event_type: "build.materials",
            occurred_at: 1_763_172_000_000,
            title: "维护",
            text: "内容",
            parameters: &parameters,
            sender_policy: None,
            commit: false,
            attachments: &attachments,
        };
        let now = crate::shell::process::epoch_millis() as i64;
        let (row, _) =
            store::create_event(&mut connection, &endpoint.id, &event, now).expect("创建事件");
        let attachment = store::event_attachments(&connection, &row.id)
            .expect("附件")
            .remove(0);
        store::begin_upload(
            &connection,
            &row.id,
            &attachment.id,
            now + 10,
            "test-attempt",
        )
        .expect("租约");
        let event_dir = blob::root().join(&row.id);
        std::fs::create_dir_all(&event_dir).expect("事件目录");
        let file_path = event_dir.join(&attachment.id);
        std::fs::write(&file_path, b"abc").expect("写附件");
        let rel = format!("{}/{}", row.id, attachment.id);
        store::finish_upload(
            &connection,
            &row.id,
            &attachment.id,
            "sha",
            3,
            &rel,
            now + 20,
            "test-attempt",
        )
        .expect("完成上传");
        store::commit_event(&mut connection, &endpoint.id, &row.id, now + 30).expect("提交");
        drop(connection);
        std::fs::remove_file(&file_path).expect("模拟文件丢失");

        maintenance_once();

        let connection = store::open(&db_path()).expect("重开");
        let event = store::event_detail(&connection, &row.id)
            .expect("查询")
            .expect("事件仍在");
        assert_eq!(event.state, "unavailable");
        stop();
        std::fs::remove_dir_all(&home).ok();
    }
}
