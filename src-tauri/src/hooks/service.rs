//! Hook 服务的壳级单例：配置应用、启动/停止、状态快照与后台维护。
//!
//! 服务只在应用进程内跑（P1 起是回环监听），因此不需要跨进程租约；全局运行时
//! 由模块级 `OnceLock<Mutex<_>>` 持有，命令与 setup/退出路径共用同一份状态。
//! 所有启停动作都在锁内完成：启动要绑定端口、打开/迁移数据库，必须串行。
//!
//! 后台维护线程与监听解耦：即使服务停用，收件箱仍要按保留期清理、启动后仍要
//! 核对文件是否缺失。线程只在应用启动时拉起，退出时停止。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::hooks::config::{self, HooksConfig, CONFIG_SCHEMA_VERSION};
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
    last_error: Option<String>,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Mutex::new(Runtime {
            handle: None,
            maintenance: None,
            last_error: None,
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
}

pub(crate) fn db_path() -> PathBuf {
    paths::hooks_db_file(settings::current_mode())
}

fn listen_addr(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from(([127, 0, 0, 1], port))
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
        StateRead::Loaded(config) => (config, None),
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
        running: runtime.handle.is_some(),
        bind_addr: runtime
            .handle
            .as_ref()
            .map(|handle| handle.addr.to_string()),
        last_error,
        endpoint_count,
        event_count,
        storage_bytes,
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
    // 先准备目录与数据库：打不开时不要占住端口，错误也能说清是数据问题还是端口问题。
    blob::ensure_dirs()?;
    let connection = store::open(&db_path()).map_err(|error| {
        format!(
            "Hook 收件箱数据库无法打开：{error}。请在「查看日志」确认 {} 是否可访问",
            db_path().display()
        )
    })?;
    drop(connection);
    maintenance_once();
    let handle = server::start(listen_addr(config.port), db_path(), Limits::default())?;
    let addr = handle.addr;
    runtime.handle = Some(handle);
    runtime.last_error = None;
    let line = format!("Hook 接收器已启动：http://{addr}（仅本机）");
    shell_events::record("hooks", &line);
    Ok(())
}

fn stop_locked(runtime: &mut Runtime) {
    if let Some(handle) = runtime.handle.take() {
        handle.stop();
        shell_events::record("hooks", "Hook 接收器已停止");
    }
}

fn stop_maintenance(runtime: &mut Runtime) {
    if let Some(maintenance) = runtime.maintenance.take() {
        maintenance.stop.store(true, Ordering::SeqCst);
        let _ = maintenance.handle.join();
    }
}

/// 应用新配置：保存 → 停旧 → 按需启动。端口失败时配置仍记 enabled=true
/// （下次启动重试），状态里带错误，不显示为运行中。
pub(crate) fn apply_config(config: HooksConfig) -> Result<HooksStatus, String> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(format!(
            "Hook 配置版本必须为 {CONFIG_SCHEMA_VERSION}，拒绝写入 {}",
            config.schema_version
        ));
    }
    if config.port == 0 {
        return Err("Hook 端口不能为 0；请填写 1–65535 之间的端口".into());
    }
    config::check_writable()?;
    config::save(&config)?;

    let mut runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let unchanged = runtime.handle.is_some() == config.enabled
        && runtime
            .handle
            .as_ref()
            .is_some_and(|handle| handle.addr.port() == config.port);
    if unchanged {
        runtime.last_error = None;
        return Ok(build_status(&runtime));
    }
    stop_locked(&mut runtime);
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
    let (config, config_error) = load_config();
    let mut runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_maintenance_locked(&mut runtime);
    maintenance_once();
    if let Some(error) = config_error {
        runtime.last_error = Some(error.clone());
        shell_events::record("hooks", &format!("Hook 配置不可用：{error}"));
        return;
    }
    if !config.enabled || runtime.handle.is_some() {
        return;
    }
    if let Err(error) = start_locked(&mut runtime, &config) {
        runtime.last_error = Some(error.clone());
        shell_events::record("hooks", &format!("Hook 接收器启动失败：{error}"));
    }
}

/// 应用退出时停止接收与维护线程。幂等；即使服务没起来也只是空操作。
pub(crate) fn stop() {
    let mut runtime = runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    stop_locked(&mut runtime);
    stop_maintenance(&mut runtime);
    runtime.last_error = None;
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
            // 首次维护由 auto_start / start_locked 在监听前同步完成；线程只负责
            // 周期任务，避免启动时两路同时扫库。
            loop {
                let mut waited = 0u64;
                while waited < MAINTENANCE_INTERVAL.as_secs() {
                    if thread_stop.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(Duration::from_secs(1));
                    waited += 1;
                }
                if thread_stop.load(Ordering::SeqCst) {
                    return;
                }
                maintenance_once();
            }
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

/// 一次维护：核对已存储文件、清理过期内容、回收临时与孤儿文件。
///
/// 幂等；失败只记录，不阻断调用方。DB 不存在时什么也不做。
fn maintenance_once() {
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

    // 2) 过期/保留期清理：先清数据库内容，再删文件；删文件失败时文件成为孤儿，
    //    下一轮按宽限期回收，不会再把内容当可处理事件提供。
    match store::cleanup_content(&mut connection, now) {
        Ok(targets) => {
            for target in targets {
                for rel in target.rels {
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

    // 3) 临时半截文件与没有数据库记录的孤儿文件。
    let removed_temp = blob::cleanup_temp(now - MAINTENANCE_GRACE_MS);
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
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// 全局运行时是进程级单例：服务用例必须串行，且结束前 stop。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

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

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("临时端口");
        listener.local_addr().expect("地址").port()
    }

    fn connected(port: u16) -> bool {
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    }

    #[test]
    fn apply_config_starts_and_stops_the_listener() {
        let _serial = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_home("apply");
        let _guard = scoped_xlink_home(&home);
        stop();
        let port = free_port();
        let status = apply_config(HooksConfig {
            enabled: true,
            port,
            ..HooksConfig::default()
        })
        .expect("启用");
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
    fn auto_start_follows_saved_config() {
        let _serial = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_home("autostart");
        let _guard = scoped_xlink_home(&home);
        stop();
        let port = free_port();
        config::save(&HooksConfig {
            enabled: true,
            port,
            ..HooksConfig::default()
        })
        .expect("保存启用配置");
        auto_start();
        assert!(status().running, "auto_start 应按配置启动");
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
        let _serial = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_home("conflict");
        let _guard = scoped_xlink_home(&home);
        stop();
        let occupied = TcpListener::bind("127.0.0.1:0").expect("占住端口");
        let port = occupied.local_addr().expect("地址").port();
        let status = apply_config(HooksConfig {
            enabled: true,
            port,
            ..HooksConfig::default()
        })
        .expect("配置可保存");
        assert!(!status.running, "端口被占用时不得报告运行中");
        let error = status.last_error.clone().unwrap_or_default();
        assert!(error.contains("无法监听"), "错误应说明监听失败：{error}");
        assert!(status.config.enabled, "期望状态保留，下次启动重试");

        drop(occupied);
        let status = apply_config(HooksConfig {
            enabled: true,
            port,
            ..HooksConfig::default()
        })
        .expect("重试");
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
        let _serial = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    fn maintenance_marks_missing_attachment_files_unavailable() {
        let _serial = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        store::begin_upload(&connection, &row.id, &attachment.id, now + 10).expect("租约");
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
