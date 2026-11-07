//! MiniMax 只读记忆桥：不把内置插件管理桥的完整权限交给 hook。

#[cfg(windows)]
#[path = "mcode_private.rs"]
mod mcode_private;

#[path = "mcode_memory_install.rs"]
mod mcode_memory_install;
use mcode_memory_install::install_plugin;

use std::fs;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use rand::Rng;

const CREDENTIAL_FILE: &str = "xlink-memory-bridge.json";
// Serialize this process's credential replacement and generation-aware cleanup.
static CREDENTIAL_LOCK: Mutex<()> = Mutex::new(());

const PLUGIN_FILES: &[&str] = &[
    ".minimax-plugin/plugin.json",
    "hooks/hooks.json",
    "hooks/context.mjs",
    "icon.png",
    "package.json",
    "host/bridge.js",
    "host/policy.js",
    "host/mask.js",
    "host/extract.js",
];

/// Caller holds the offline lease through plugin preparation and child reservation.
pub(crate) fn prepare(
    source: &Path,
    lease: &crate::skills::mcode::InstanceLease,
    enabled: bool,
) -> Result<Option<McodeMemoryBridge>, String> {
    let home = crate::skills::mcode::offline_home(lease).map_err(|error| error.to_string())?;
    if !enabled {
        return Ok(None);
    }
    let instance_id = home
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or("MiniMax 实例标识无效")?;
    crate::shell::paths::validate_id_component(instance_id).map_err(str::to_owned)?;
    install_plugin(source, instance_id)?;
    McodeMemoryBridge::start(instance_id, true)
}

pub(crate) struct McodeMemoryBridge {
    token: String,
    credential: PathBuf,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    cleanup: Option<JoinHandle<Result<(), String>>>,
}

impl McodeMemoryBridge {
    /// The caller must pass the persisted opt-in for this instance and retain the returned
    /// generation for exactly the lifetime of the MiniMax service.
    fn start(instance_id: &str, enabled: bool) -> Result<Option<Self>, String> {
        if !enabled {
            return Ok(None);
        }
        Self::start_private(instance_id).map(Some)
    }

    /// Disable further requests immediately; keep the worker handle if it cannot be joined yet.
    pub(crate) fn stop(&mut self) -> Result<(), String> {
        self.stop_with_timeout(Duration::from_secs(2))
    }

    fn stop_with_timeout(&mut self, timeout: Duration) -> Result<(), String> {
        self.shutdown.store(true, Ordering::SeqCst);
        if self.cleanup.is_none() {
            let path = self.credential.clone();
            let token = self.token.clone();
            self.cleanup = Some(
                thread::Builder::new()
                    .name("mcode-memory-cleanup".into())
                    .spawn(move || remove_credential(&path, &token))
                    .map_err(|error| format!("无法启动记忆桥凭据清理：{error}"))?,
            );
        }
        let deadline = std::time::Instant::now() + timeout;
        while self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
            || self
                .cleanup
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
        {
            if std::time::Instant::now() >= deadline {
                return Err("MiniMax 记忆桥尚未退出，已停止接受请求；请稍后重试停止".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                return Err("MiniMax 记忆桥线程异常退出，请检查日志后重试".into());
            }
        }
        match self.cleanup.take() {
            Some(cleanup) => cleanup
                .join()
                .map_err(|_| "MiniMax 记忆桥凭据清理线程异常，请重试".to_string())?,
            None => Ok(()),
        }
    }

    fn start_private(instance_id: &str) -> Result<Self, String> {
        crate::shell::paths::validate_id_component(instance_id).map_err(str::to_string)?;
        let home = crate::shell::paths::instance_dsh_home(
            crate::shell::instance::KERNEL_FAMILY_MCODE,
            instance_id,
        );
        let credential = credential_path(&home)?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("无法启动 MiniMax 记忆桥：{e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|value| format!("{value:02x}")).collect();
        let body = serde_json::to_vec(&serde_json::json!({
            "url": format!("http://{address}"),
            "token": token,
        }))
        .map_err(|e| e.to_string())?;
        {
            let _guard = crate::lock(&CREDENTIAL_LOCK);
            #[cfg(windows)]
            mcode_private::write_private(&credential, &body)
                .map_err(|e| format!("无法安全保存 MiniMax 记忆桥凭据：{e}"))?;
            #[cfg(not(windows))]
            write_private_exclusive(&credential, &body)
                .map_err(|e| format!("无法保存 MiniMax 记忆桥凭据：{e}"))?;
        }
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_token = token.clone();
        let namespace = crate::memory::store::Namespace::for_bridge(
            crate::shell::instance::KERNEL_FAMILY_MCODE,
            instance_id,
        );
        let thread = match thread::Builder::new()
            .name("mcode-memory-bridge".into())
            .spawn(move || {
                while !thread_shutdown.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            serve(&mut stream, &thread_token, &namespace, &thread_shutdown)
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(25))
                        }
                        Err(_) => break,
                    }
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = remove_credential(&credential, &token);
                return Err(format!("无法启动 MiniMax 记忆桥线程：{error}"));
            }
        };
        Ok(Self {
            token,
            credential,
            shutdown,
            thread: Some(thread),
            cleanup: None,
        })
    }
}

#[cfg(not(windows))]
fn write_private_exclusive(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("credential has no parent"))?;
    let temp = parent.join(format!(".xlink-memory-{:032x}.tmp", rand::random::<u128>()));
    crate::shell::atomic_write::atomic_write_private(&temp, body)?;
    let result = fs::hard_link(&temp, path);
    let cleanup = fs::remove_file(&temp);
    result?;
    if let Err(error) = cleanup {
        if fs::read(path).is_ok_and(|bytes| bytes == body) {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(())
}

impl Drop for McodeMemoryBridge {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // A worker stuck in file I/O must never hold application shutdown indefinitely.
        drop(self.thread.take());
        if self.cleanup.is_some() {
            drop(self.cleanup.take());
            return;
        }
        let path = self.credential.clone();
        let token = self.token.clone();
        let _ = thread::Builder::new()
            .name("mcode-memory-cleanup".into())
            .spawn(move || {
                let _ = remove_credential(&path, &token);
            });
    }
}

fn credential_path(home: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(home).map_err(|e| format!("无法准备 MiniMax 数据目录：{e}"))?;
    let mut dir = home.to_path_buf();
    for name in ["v2", "plugin-data", "hooks", "xlink-memory"] {
        dir.push(name);
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无法准备 MiniMax 记忆桥目录：{error}")),
        }
        let metadata = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("MiniMax 记忆桥目录不是普通目录，请检查实例数据目录".into());
        }
    }
    Ok(dir.join(CREDENTIAL_FILE))
}

fn remove_credential(path: &Path, token: &str) -> Result<(), String> {
    let _guard = crate::lock(&CREDENTIAL_LOCK);
    let body = match fs::read(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "无法检查 MiniMax 记忆桥凭据：{error}；请检查 {}",
                path.display()
            ))
        }
    };
    let belongs = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|body| {
            body.get("token")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .is_some_and(|value| value == token);
    if belongs {
        fs::remove_file(path).map_err(|error| {
            format!(
                "无法清理 MiniMax 记忆桥凭据：{error}；请检查 {}",
                path.display()
            )
        })?;
    } else {
        return Err(format!(
            "MiniMax 记忆桥凭据已改变，保留原文件：{}",
            path.display()
        ));
    }
    Ok(())
}

fn serve(
    stream: &mut TcpStream,
    token: &str,
    namespace: &crate::memory::store::Namespace,
    shutdown: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let Ok(request) = crate::openai::http::read_request(stream) else {
        return;
    };
    if shutdown.load(Ordering::SeqCst) {
        return;
    }
    if request.headers.contains_key("origin")
        || request
            .headers
            .get("authorization")
            .is_none_or(|value| value != &format!("Bearer {token}"))
    {
        let _ = crate::openai::http::write_response(
            stream,
            401,
            "application/json",
            r#"{"error":"unauthorized"}"#,
        );
        return;
    }
    if context_request_allowed(&request.method, &request.path) {
        let query = crate::plugins::builtin::bridge::query_decoded(&request.path, "q");
        let workspace = crate::plugins::builtin::bridge::query_decoded(&request.path, "workspace");
        let _grant_lock = match crate::memory::share::lock_grants() {
            Ok(lock) => lock,
            Err(_) => {
                let _ = crate::openai::http::write_response(
                    stream,
                    503,
                    "application/json",
                    r#"{"error":"authorization unavailable"}"#,
                );
                return;
            }
        };
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let current =
            crate::memory::store::Namespace::for_bridge(&namespace.family, &namespace.instance_id);
        if current.profile != namespace.profile {
            let _ = crate::openai::http::write_response(
                stream,
                200,
                "application/json",
                r#"{"enabled":false}"#,
            );
            return;
        }
        match crate::memory::share::retrieve_mcode(namespace, &query, &workspace) {
            Ok(context) => {
                let body = serde_json::to_string(&context)
                    .unwrap_or_else(|_| r#"{"enabled":false}"#.into());
                // 桥这一跳此前**零留痕**：注入成功与 hook 静默失败在壳里长得一模一样，
                // A07 没法判。只记条数与字节数——提问与记忆正文一个字都不落盘。
                crate::shell::shell_events::record(
                    "mcode-memory",
                    &format!(
                        "记忆桥已响应 MiniMax：{} 条记录、{} 字节，开关{}",
                        context.hits.len(),
                        context.total_bytes,
                        if context.enabled { "开" } else { "关" }
                    ),
                );
                if shutdown.load(Ordering::SeqCst) {
                    return;
                }
                let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
            }
            Err(error) => {
                crate::shell::shell_events::record(
                    "mcode-memory",
                    &format!("记忆桥检索失败：{error}"),
                );
                let body = serde_json::json!({ "error": error.to_string() }).to_string();
                let _ = crate::openai::http::write_response(stream, 500, "application/json", &body);
            }
        }
    } else {
        let _ = crate::openai::http::write_response(
            stream,
            404,
            "application/json",
            r#"{"error":"not found"}"#,
        );
    }
}

fn context_request_allowed(method: &str, path: &str) -> bool {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    if method != "GET" || route != "/v1/memory/context" {
        return false;
    }
    if query.is_empty() {
        return true;
    }
    let (mut saw_query, mut saw_workspace) = (false, false);
    for part in query.split('&') {
        let Some((key, _)) = part.split_once('=') else {
            return false;
        };
        let seen = match key {
            "q" => &mut saw_query,
            "workspace" => &mut saw_workspace,
            _ => return false,
        };
        if *seen {
            return false;
        }
        *seen = true;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn stop_times_out_without_losing_worker_and_can_be_retried() {
        let root =
            std::env::temp_dir().join(format!("mcode-memory-stop-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let credential = root.join("credential.json");
        fs::write(&credential, br#"{"token":"fixture-token"}"#).unwrap();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let worker = thread::spawn(move || {
            let _ = wait.recv();
        });
        let mut bridge = McodeMemoryBridge {
            token: "fixture-token".into(),
            credential: credential.clone(),
            shutdown: Arc::new(AtomicBool::new(false)),
            thread: Some(worker),
            cleanup: None,
        };
        assert!(bridge.stop_with_timeout(Duration::from_millis(20)).is_err());
        assert!(bridge.shutdown.load(Ordering::SeqCst));
        assert!(bridge.thread.is_some());
        release.send(()).unwrap();
        bridge.stop_with_timeout(Duration::from_secs(1)).unwrap();
        assert!(bridge.thread.is_none());
        assert!(!credential.exists());
        drop(bridge);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn plugin_install_preserves_an_existing_unrelated_directory() {
        use sha2::{Digest, Sha256};
        let root = std::env::temp_dir().join(format!(
            "mcode-memory-plugin-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let guard = crate::tests::scoped_xlink_home(&root);
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let mut files = Vec::new();
        for name in PLUGIN_FILES {
            let path = source.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let body = name.as_bytes();
            fs::write(path, body).unwrap();
            files.push(serde_json::json!({"path": name, "bytes": body.len(), "sha256": crate::shell::digest::hex(&Sha256::digest(body)[..])}));
        }
        fs::write(
            source.join(super::super::materialize::MANIFEST_FILE),
            serde_json::to_vec(&serde_json::json!({"files": files})).unwrap(),
        )
        .unwrap();
        let home = crate::shell::paths::instance_dsh_home("mcode", "fixture");
        fs::create_dir_all(&home).unwrap();
        install_plugin(&source, "fixture").unwrap();
        install_plugin(&source, "fixture").unwrap();
        let path = home.join("plugins/xlink-memory/icon.png");
        fs::write(&path, b"changed by user").unwrap();
        assert!(install_plugin(&source, "fixture").is_err());
        assert_eq!(fs::read(path).unwrap(), b"changed by user");
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn context_parameter_filter_accepts_only_one_of_each_hook_parameter() {
        for path in [
            "/v1/memory/context",
            "/v1/memory/context?q=%E4%B8%AD%E6%96%87&workspace=C%3A%5Cproject",
            "/v1/memory/context?workspace=project&q=question",
        ] {
            assert!(context_request_allowed("GET", path), "{path}");
        }
        for path in [
            "/v1/memory/context?pending=1",
            "/v1/memory/context?%70ending=1",
            "/v1/memory/context?q=x&pending=1",
            "/v1/memory/context?q=x&q=y",
            "/v1/memory/context?workspace=x&workspace=y",
            "/v1/memory/context?workspace_id=other",
            "/v1/memory/context?%71=x",
            "/v1/memory/context?q",
            "/v1/memory/context?q=x&",
            "/v1/memory/context/extra?q=x",
        ] {
            assert!(!context_request_allowed("GET", path), "{path}");
        }
        assert!(!context_request_allowed("POST", "/v1/memory/context?q=x"));
    }

    #[test]
    fn scoped_bridge_rejects_pending_unknown_and_duplicate_parameters() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-route-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let _home = crate::tests::scoped_xlink_home(&root);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let token = "test-token";
        let paths = [
            ("/v1/memory/context?q=test&workspace=project", 200),
            ("/v1/memory/context?pending=1", 404),
            ("/v1/memory/context?%70ending=1", 404),
            ("/v1/memory/context?q=test&q=other", 404),
            ("/v1/memory/context?workspace_id=other", 404),
        ];
        let worker = thread::spawn(move || {
            let shutdown = AtomicBool::new(false);
            for _ in 0..paths.len() {
                let (mut stream, _) = listener.accept().unwrap();
                let namespace = crate::memory::store::Namespace::for_bridge("mcode", "isolated");
                serve(&mut stream, token, &namespace, &shutdown);
            }
        });
        for (path, expected) in paths {
            assert!(
                request(&url, token, path, "").starts_with(&format!("HTTP/1.1 {expected}")),
                "{path} should return {expected}"
            );
        }
        worker.join().unwrap();
        // 桥这一跳必须留下可查的记录：否则「注入成功」与「hook 静默失败」长得一样。
        let log_path = crate::shell::process::LogSpec::new(
            crate::shell::process::build_log_kind(),
            "mcode-memory",
        )
        .path_for(
            &crate::shell::process::shell_logs_dir(),
            &crate::shell::process::current_date_string(),
        );
        assert!(
            log_path.exists(),
            "记忆桥请求必须落进壳日志：{}",
            log_path.display()
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_change_is_serialized_with_bridge_and_old_profile_is_disabled() {
        let root = std::env::temp_dir().join(format!(
            "mcode-profile-lock-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(&root).unwrap();
        let home = crate::tests::scoped_xlink_home(&root);
        let mode = crate::shell::settings::current_mode();
        let namespace = crate::memory::store::Namespace::for_bridge("mcode", "isolated");
        let held = crate::memory::share::lock_grants().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let new_profile = format!("{}-next", namespace.profile);
        let writer_profile = new_profile.clone();
        let writer = thread::spawn(move || {
            let mut settings = crate::shell::settings::load_for_shell(mode);
            settings.profile = writer_profile;
            started_tx.send(()).unwrap();
            done_tx
                .send(crate::shell::settings::save_for_shell(mode, &settings))
                .unwrap();
        });
        started_rx.recv().unwrap();
        assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(held);
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        writer.join().unwrap();
        assert_eq!(
            crate::memory::store::Namespace::for_bridge("mcode", "isolated").profile,
            new_profile
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            serve(
                &mut stream,
                "test-token",
                &namespace,
                &AtomicBool::new(false),
            );
        });
        let response = request(&url, "test-token", "/v1/memory/context", "");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains(r#"{"enabled":false}"#));
        worker.join().unwrap();
        drop(home);
        fs::remove_dir_all(root).unwrap();
    }

    fn request(url: &str, token: &str, path: &str, extra: &str) -> String {
        let address = url.strip_prefix("http://").unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\n{extra}Connection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[cfg(not(windows))]
    fn credential(bridge: &McodeMemoryBridge) -> serde_json::Value {
        serde_json::from_slice(&fs::read(&bridge.credential).unwrap()).unwrap()
    }

    #[cfg(not(windows))]
    #[test]
    fn instances_have_scoped_read_only_bridges_and_generation_safe_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-memory-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let _home = crate::tests::scoped_xlink_home(&root);
        assert!(McodeMemoryBridge::start("first", false).unwrap().is_none());
        assert!(!crate::shell::paths::instance_dsh_home("mcode", "first").exists());
        let mut old = McodeMemoryBridge::start("first", true).unwrap().unwrap();
        let mut other = McodeMemoryBridge::start("second", true).unwrap().unwrap();
        let old_cred = credential(&old);
        let other_cred = credential(&other);
        let old_url = old_cred["url"].as_str().unwrap();
        let old_token = old_cred["token"].as_str().unwrap();
        let other_url = other_cred["url"].as_str().unwrap();
        let other_token = other_cred["token"].as_str().unwrap();
        assert_ne!(old_token, other_token);
        assert!(request(other_url, old_token, "/v1/memory/context", "").starts_with("HTTP/1.1 401"));
        assert!(request(old_url, old_token, "/v1/builtin/sync?plugin=x", "")
            .starts_with("HTTP/1.1 404"));
        assert!(request(
            old_url,
            old_token,
            "/v1/memory/context",
            "Origin: https://example.com\r\n"
        )
        .starts_with("HTTP/1.1 401"));
        assert!(request(old_url, old_token, "/v1/memory/context?q=test", "")
            .starts_with("HTTP/1.1 200"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&old.credential).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(McodeMemoryBridge::start("first", true).is_err());
        let path = old.credential.clone();
        assert_eq!(credential(&old), old_cred);
        old.stop().unwrap();
        drop(old);
        assert!(!path.exists());
        assert!(other.credential.exists());
        other.stop().unwrap();
        drop(other);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn bridge_does_not_replace_an_existing_credential() {
        let root = std::env::temp_dir().join(format!(
            "xlink-mcode-memory-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let _home = crate::tests::scoped_xlink_home(&root);
        assert!(McodeMemoryBridge::start("first", false).unwrap().is_none());
        let home = crate::shell::paths::instance_dsh_home("mcode", "first");
        let credential = credential_path(&home).unwrap();
        fs::write(&credential, b"user data").unwrap();
        assert!(McodeMemoryBridge::start("first", true).is_err());
        assert_eq!(fs::read(&credential).unwrap(), b"user data");
        fs::remove_dir_all(&root).unwrap();
    }
}
