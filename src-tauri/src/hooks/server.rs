//! Hook 接收器：回环 TcpListener、固定工作线程、鉴权、限流与 v1 路由。
//!
//! P1：纯文本/参数入箱；P2：附件上传（PNG/JPEG/WebP 静态图 + 文本日志）、配额、
//! 原子归档与提交前核验。传输层复用 `openai::http` 的读请求原语，这里只做路由
//! 与业务编排；数据库访问全部经 [`crate::hooks::store`]，文件经
//! [`crate::hooks::blob`]。
//!
//! 停止语义：置取消旗标 → 非阻塞 accept 退出 → shutdown 跟踪中的连接 → join
//! 线程。退出时不留下仍在监听的线程或半截请求。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine as _;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hooks::network::LanPolicy;
use crate::hooks::protocol::{
    self, AttachmentCreated, AttachmentView, CreateEventRequest, RequestError,
};
use crate::hooks::store::{self, AttachmentRow, EndpointAuth, StoreError};
use crate::hooks::{blob, media};
use crate::openai::http;

const DRAIN_DEADLINE: Duration = Duration::from_millis(250);
const DRAIN_MAX_BYTES: usize = 256 * 1024;

const RATE_WINDOW: Duration = Duration::from_secs(60);
const UPLOAD_DEADLINE: Duration = Duration::from_secs(300);

/// 可调参数。测试用较小值覆盖限流/请求体上限，生产用默认值。
#[derive(Debug, Clone)]
pub(crate) struct Limits {
    pub max_body_bytes: usize,
    pub rate_per_minute: u32,
    pub uploads_per_endpoint: u32,
    pub workers: usize,
    pub queue: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body_bytes: 256 * 1024,
            rate_per_minute: 60,
            uploads_per_endpoint: 2,
            workers: 4,
            queue: 16,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
        }
    }
}

struct Job {
    stream: TcpStream,
    _registration: StreamRegistration,
}

// 注册随任务生命周期释放，覆盖拒绝入队、取消、正常结束及异常展开。
struct StreamRegistration {
    id: u64,
    streams: Arc<Mutex<HashMap<u64, TcpStream>>>,
}

impl Drop for StreamRegistration {
    fn drop(&mut self) {
        self.streams
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

struct RateWindow {
    started: Instant,
    count: u32,
}

struct ServerState {
    db_path: PathBuf,
    limits: Limits,
    listen_port: u16,
    rate: Mutex<HashMap<String, RateWindow>>,
    uploads: Mutex<HashMap<String, u32>>,
}

pub(crate) struct ServerHandle {
    /// 对外展示的主地址（第一个监听地址）。
    pub(crate) addr: SocketAddr,
    cancel: Arc<AtomicBool>,
    accepts: Vec<JoinHandle<()>>,
    workers: Vec<JoinHandle<()>>,
    streams: Arc<Mutex<HashMap<u64, TcpStream>>>,
}

impl ServerHandle {
    pub(crate) fn running(&self) -> bool {
        !self.cancel.load(Ordering::SeqCst)
    }

    /// 停止接收并回收线程。连接会被 shutdown，堵在读写上的工作线程立刻返回。
    pub(crate) fn stop(mut self) {
        self.stop_inner();
    }

    /// 仅所有线程成功退出才可记录干净停止；panic 不能清除网络重确认标记。
    pub(crate) fn stop_cleanly(mut self) -> bool {
        self.stop_inner()
    }

    fn stop_inner(&mut self) -> bool {
        let mut clean = true;
        if !self.accepts.is_empty() {
            self.cancel.store(true, Ordering::SeqCst);
            // accept 线程是非阻塞轮询，置旗标后最多等一个轮询周期，不需要自连唤醒。
            for accept in self.accepts.drain(..) {
                clean &= accept.join().is_ok();
            }
        }
        if let Ok(streams) = self.streams.lock() {
            for stream in streams.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        for worker in self.workers.drain(..) {
            clean &= worker.join().is_ok();
        }
        clean
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

#[cfg(test)]
pub(crate) fn start(
    addr: SocketAddr,
    db_path: PathBuf,
    limits: Limits,
) -> Result<ServerHandle, String> {
    start_on(vec![addr], db_path, limits)
}

/// 来源策略判断单独成函数，便于单测拒绝路径。
fn source_allowed(policy: Option<&LanPolicy>, peer: IpAddr) -> bool {
    policy.is_none_or(|policy| policy.allows(peer))
}

/// 在多个地址上监听同一端口（内网网卡 + 本机回环）。任一地址绑定失败都整体失败，
/// 不留下只监听一半的服务。
#[cfg(test)]
pub(crate) fn start_on(
    addrs: Vec<SocketAddr>,
    db_path: PathBuf,
    limits: Limits,
) -> Result<ServerHandle, String> {
    start_with_policy(addrs, db_path, limits, None)
}

pub(crate) fn start_with_policy(
    addrs: Vec<SocketAddr>,
    db_path: PathBuf,
    limits: Limits,
    policy: Option<LanPolicy>,
) -> Result<ServerHandle, String> {
    if addrs.is_empty() {
        return Err("Hook 服务没有可监听的地址".into());
    }
    let mut listeners = Vec::with_capacity(addrs.len());
    let mut bound = Vec::with_capacity(addrs.len());
    for addr in &addrs {
        let listener = TcpListener::bind(addr).map_err(|error| {
            format!(
                "Hook 服务无法监听 {addr}（端口可能被占用或没有权限）：{error}。\
                 请换一个端口后重试；服务保持停止状态"
            )
        })?;
        let local = listener
            .local_addr()
            .map_err(|error| format!("无法读取 Hook 监听地址 {addr}：{error}"))?;
        // 非阻塞 + 短轮询：接口掉线时停止服务不能卡在 join accept 上。
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Hook 服务无法为非阻塞监听 {addr} 设置模式：{error}"))?;
        bound.push(local);
        listeners.push(listener);
    }
    let primary = bound[0];
    let limits = Limits {
        workers: limits.workers.clamp(1, 16),
        queue: limits.queue.max(1),
        uploads_per_endpoint: limits.uploads_per_endpoint.max(1),
        ..limits
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let streams = Arc::new(Mutex::new(HashMap::<u64, TcpStream>::new()));
    let (tx, rx) = mpsc::sync_channel::<Job>(limits.queue);
    let rx = Arc::new(Mutex::new(rx));
    let listen_port = primary.port();
    let state = Arc::new(ServerState {
        db_path,
        limits: limits.clone(),
        listen_port,
        rate: Mutex::new(HashMap::new()),
        uploads: Mutex::new(HashMap::new()),
    });

    let mut workers = Vec::with_capacity(limits.workers);
    for index in 0..limits.workers {
        let worker_rx = Arc::clone(&rx);
        let worker_state = Arc::clone(&state);
        let worker_cancel = Arc::clone(&cancel);
        let worker = thread::Builder::new()
            .name(format!("dsh-hooks-worker-{index}"))
            .spawn(move || loop {
                if worker_cancel.load(Ordering::SeqCst) {
                    break;
                }
                let job = {
                    let guard = worker_rx
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.recv()
                };
                let Ok(job) = job else { break };
                if worker_cancel.load(Ordering::SeqCst) {
                    let _ = job.stream.shutdown(Shutdown::Both);
                    break;
                }
                handle_connection(job.stream, &worker_state);
            });
        match worker {
            Ok(worker) => workers.push(worker),
            Err(error) => {
                cancel.store(true, Ordering::SeqCst);
                drop(tx);
                for worker in workers.drain(..) {
                    let _ = worker.join();
                }
                return Err(format!("无法启动 Hook 工作线程：{error}"));
            }
        }
    }

    let write_timeout = limits.write_timeout;
    let next_id = Arc::new(AtomicU64::new(1));
    let mut accepts = Vec::with_capacity(listeners.len());
    for listener in listeners {
        let accept_cancel = Arc::clone(&cancel);
        let accept_streams = Arc::clone(&streams);
        let accept_next_id = Arc::clone(&next_id);
        let accept_tx = tx.clone();
        let accept_policy = policy.clone();
        let accept = thread::Builder::new()
            .name("dsh-hooks-accept".into())
            .spawn(move || {
                let mut last_network_check = Instant::now() - Duration::from_secs(2);
                loop {
                    if accept_cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    if last_network_check.elapsed() >= Duration::from_secs(1) {
                        last_network_check = Instant::now();
                        if accept_policy
                            .as_ref()
                            .is_some_and(|policy| !policy.available())
                        {
                            accept_cancel.store(true, Ordering::SeqCst);
                            for stream in accept_streams
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .values()
                            {
                                let _ = stream.shutdown(Shutdown::Both);
                            }
                            crate::shell::shell_events::record(
                                "hooks",
                                "可信局域网地址或掩码已变化，接收已暂停；请重新选择网卡并启用",
                            );
                            break;
                        }
                    }
                    let stream = match listener.accept() {
                        Ok((mut stream, peer)) => {
                            if !source_allowed(accept_policy.as_ref(), peer.ip()) {
                                // 给发送端一个可解释的 403，而不是静默重置；响应体不含任何凭据。
                                let _ = stream.set_nonblocking(false);
                                // 拒绝发生在 accept 线程，不能由慢客户端阻塞监听或网络撤销。
                                let _ = stream.set_write_timeout(Some(DRAIN_DEADLINE));
                                let body = serde_json::json!({
                                    "error": {
                                        "code": "forbidden",
                                        "message": "来源不在允许的可信局域网网段",
                                        "request_id": request_id(),
                                    }
                                })
                                .to_string();
                                let _ = http::write_response(
                                    &mut stream,
                                    403,
                                    "application/json",
                                    &body,
                                );
                                // 请求尚未读取；有界排空避免未读字节触发 RST 丢掉 403。
                                drain_incoming(&mut stream);
                                let _ = stream.shutdown(Shutdown::Both);
                                continue;
                            }
                            stream
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                    };
                    // 监听 socket 是非阻塞的；accept 出来的连接必须显式改回阻塞，
                    // 否则 worker 读请求头会立刻 WouldBlock，把正常请求当成坏请求。
                    let _ = stream.set_nonblocking(false);
                    let id = accept_next_id.fetch_add(1, Ordering::SeqCst);
                    let Ok(clone) = stream.try_clone() else {
                        continue;
                    };
                    accept_streams
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(id, clone);
                    let registration = StreamRegistration {
                        id,
                        streams: Arc::clone(&accept_streams),
                    };
                    match accept_tx.try_send(Job {
                        stream,
                        _registration: registration,
                    }) {
                        Ok(()) => {}
                        Err(mpsc::TrySendError::Full(mut job)) => {
                            let _ = job.stream.set_write_timeout(Some(write_timeout));
                            let request_id = request_id();
                            let body = serde_json::json!({
                                "error": {
                                    "code": "service_unavailable",
                                    "message": "接收队列已满，请稍后重试",
                                    "request_id": request_id,
                                }
                            })
                            .to_string();
                            let _ = http::write_response(
                                &mut job.stream,
                                503,
                                "application/json",
                                &body,
                            );
                            // 拒绝时同样可能尚有未读请求，沿用有界排空以保证 503 可见。
                            drain_incoming(&mut job.stream);
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
            });
        match accept {
            Ok(handle) => accepts.push(handle),
            Err(error) => {
                cancel.store(true, Ordering::SeqCst);
                // accept 线程非阻塞轮询，置旗标后会自行退出；回收已启动的线程。
                drop(tx);
                for handle in accepts.drain(..) {
                    let _ = handle.join();
                }
                for worker in workers.drain(..) {
                    let _ = worker.join();
                }
                return Err(format!("无法启动 Hook 接收线程：{error}"));
            }
        }
    }
    // 所有 accept 线程各持一份 tx clone；主线程这份必须丢掉，否则 worker 永不退出。
    drop(tx);

    Ok(ServerHandle {
        addr: primary,
        cancel,
        accepts,
        workers,
        streams,
    })
}

fn request_id() -> String {
    format!("req_{:016x}", rand::random::<u64>())
}

fn handle_connection(mut stream: TcpStream, state: &ServerState) {
    let _ = stream.set_read_timeout(Some(state.limits.read_timeout));
    let _ = stream.set_write_timeout(Some(state.limits.write_timeout));
    let request_id = request_id();
    let mut connection = match store::open(&state.db_path) {
        Ok(connection) => connection,
        Err(error) => {
            // 详情只进壳日志：局域网发送方不应拿到数据库路径或 SQL 错误原文。
            crate::shell::shell_events::record("hooks", &format!("收件箱不可用：{error}"));
            let failure = Failure::new(
                503,
                "service_unavailable",
                "收件箱暂不可用，请稍后重试；详情见桌面端日志",
            );
            write_failure(&mut stream, &failure, &request_id);
            // 数据库打不开时还没读请求体；直接 close 会让未读字节触发 RST，
            // macOS 客户端可能收不到 503。先排空再关闭。
            drain_incoming(&mut stream);
            return;
        }
    };
    match dispatch(&mut stream, state, &mut connection) {
        Ok(response) => write_response(&mut stream, response),
        Err(failure) => {
            if failure.status != 401 && failure.status != 404 {
                store::record_error(
                    &connection,
                    &request_id,
                    failure.endpoint_id.as_deref(),
                    failure.event_id.as_deref(),
                    failure.code,
                );
            }
            write_failure(&mut stream, &failure, &request_id);
        }
    }
}

fn drain_incoming(stream: &mut TcpStream) {
    // 先发送 FIN 让客户端及时读完 503；排空仅用于避免未读数据导致 RST，不能
    // 被持续发送的小包无限续期，也不能无上限消费攻击者的数据。
    let _ = stream.shutdown(Shutdown::Write);
    let deadline = Instant::now() + DRAIN_DEADLINE;
    let mut remaining = DRAIN_MAX_BYTES;
    let mut buffer = [0u8; 4096];
    while remaining > 0 {
        let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if stream
            .set_read_timeout(Some(wait.min(Duration::from_millis(100))))
            .is_err()
        {
            break;
        }
        let size = buffer.len().min(remaining);
        match stream.read(&mut buffer[..size]) {
            Ok(0) | Err(_) => break,
            Ok(count) => remaining -= count,
        }
    }
}

struct Response {
    status: u16,
    body: String,
    headers: Vec<(&'static str, String)>,
}

impl Response {
    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        Self {
            status,
            body: serde_json::to_string(value).unwrap_or_else(|_| "{}".into()),
            headers: Vec::new(),
        }
    }

    fn with_header(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }
}

struct Failure {
    status: u16,
    code: &'static str,
    message: String,
    endpoint_id: Option<String>,
    event_id: Option<String>,
    retry_after: Option<u64>,
}

impl Failure {
    fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            endpoint_id: None,
            event_id: None,
            retry_after: None,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", message)
    }

    fn unauthorized() -> Self {
        Self::new(401, "unauthorized", "令牌无效或已失效")
    }

    fn forbidden() -> Self {
        Self::new(403, "forbidden", "接入点已暂停，请先在管理面板恢复")
    }

    fn not_found() -> Self {
        Self::new(404, "not_found", "对象不存在")
    }

    fn too_large(message: impl Into<String>) -> Self {
        Self::new(413, "too_large", message)
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self::new(415, "unsupported_media_type", message)
    }

    fn rate_limited(retry_after: u64) -> Self {
        let mut failure = Self::new(429, "rate_limited", "请求过于频繁，请稍后重试");
        failure.retry_after = Some(retry_after);
        failure
    }

    fn with_endpoint(mut self, endpoint_id: &str) -> Self {
        self.endpoint_id = Some(endpoint_id.to_string());
        self
    }

    fn with_event(mut self, event_id: &str) -> Self {
        self.event_id = Some(event_id.to_string());
        self
    }
}

enum Route {
    Create {
        endpoint_id: String,
    },
    /// Jenkins Notification 兼容入口：独立 Basic 凭据 + Finalized 元数据映射。
    Notification {
        endpoint_id: String,
    },
    Commit {
        endpoint_id: String,
        event_id: String,
    },
    Receipt {
        endpoint_id: String,
        event_id: String,
    },
    Upload {
        endpoint_id: String,
        event_id: String,
        attachment_id: String,
    },
}

impl Route {
    fn endpoint_id(&self) -> &str {
        match self {
            Route::Create { endpoint_id }
            | Route::Notification { endpoint_id }
            | Route::Commit { endpoint_id, .. }
            | Route::Receipt { endpoint_id, .. }
            | Route::Upload { endpoint_id, .. } => endpoint_id,
        }
    }
}

fn parse_route(method: &str, path: &str) -> Option<Route> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.len() < 5
        || segments[0] != "hooks"
        || segments[1] != "v1"
        || segments[2] != "endpoints"
    {
        return None;
    }
    let endpoint_id = segments[3].to_string();
    if endpoint_id.is_empty() {
        return None;
    }
    if segments[4] == "jenkins-notification" {
        return (method == "POST" && segments.len() == 5)
            .then_some(Route::Notification { endpoint_id });
    }
    if segments[4] != "events" {
        return None;
    }
    match (method, segments.len()) {
        ("POST", 5) => Some(Route::Create { endpoint_id }),
        ("POST", 7) if segments[6] == "commit" => Some(Route::Commit {
            endpoint_id,
            event_id: segments[5].to_string(),
        }),
        ("GET", 7) if segments[6] == "receipt" => Some(Route::Receipt {
            endpoint_id,
            event_id: segments[5].to_string(),
        }),
        ("PUT", 8) if segments[6] == "attachments" => Some(Route::Upload {
            endpoint_id,
            event_id: segments[5].to_string(),
            attachment_id: segments[7].to_string(),
        }),
        _ => None,
    }
}

fn dispatch(
    stream: &mut TcpStream,
    state: &ServerState,
    connection: &mut Connection,
) -> Result<Response, Failure> {
    let head = http::read_request(stream)
        .map_err(|error| Failure::invalid(format!("请求头无法解析：{error}")))?;
    validate_host(&head, state)?;
    let route = parse_route(&head.method, &head.path).ok_or_else(Failure::not_found)?;

    let endpoint = match &route {
        Route::Notification { .. } => notification_endpoint(connection, &head)?,
        _ => {
            let token = bearer_token(&head).ok_or_else(Failure::unauthorized)?;
            store::auth_by_token_hash(connection, &store::hash_token(token))
                .map_err(|error| store_failure(error, None, None, "event_conflict"))?
                .ok_or_else(Failure::unauthorized)?
        }
    };
    if endpoint.deleted {
        return Err(Failure::unauthorized());
    }
    if !endpoint.enabled {
        return Err(Failure::forbidden().with_endpoint(&endpoint.id));
    }
    if endpoint.id != route.endpoint_id() {
        return Err(Failure::not_found());
    }

    match route {
        Route::Create { .. } => create_event(stream, state, connection, &endpoint, &head),
        Route::Notification { .. } => {
            jenkins_notification(stream, state, connection, &endpoint, &head)
        }
        Route::Commit { event_id, .. } => commit_event(connection, &endpoint, &event_id, &head),
        Route::Receipt { event_id, .. } => receipt(connection, &endpoint, &event_id),
        Route::Upload {
            event_id,
            attachment_id,
            ..
        } => upload_attachment(
            stream,
            state,
            connection,
            &endpoint,
            &event_id,
            &attachment_id,
            &head,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NotificationPayload {
    // 旧版示例曾多包一层 job，保留兼容；官方插件直接序列化 JobState。
    Wrapped { job: NotificationJob },
    Direct(NotificationJob),
}

impl NotificationPayload {
    fn into_job(self) -> NotificationJob {
        match self {
            Self::Wrapped { job } | Self::Direct(job) => job,
        }
    }
}

#[derive(Debug, Deserialize)]
struct NotificationJob {
    name: Option<String>,
    display_name: Option<String>,
    url: Option<String>,
    build: Option<NotificationBuild>,
}

#[derive(Debug, Deserialize)]
struct NotificationBuild {
    number: Option<i64>,
    queue_id: Option<i64>,
    timestamp: Option<i64>,
    duration: Option<i64>,
    phase: Option<String>,
    status: Option<String>,
    url: Option<String>,
    full_url: Option<String>,
    display_name: Option<String>,
    parameters: Option<serde_json::Value>,
    notes: Option<String>,
    scm: Option<serde_json::Value>,
    test_summary: Option<serde_json::Value>,
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// Jenkins Notification（notification-plugin 1.17）路由：独立 Basic 凭据，
/// 只收 FINALIZED 元数据，不下载日志或抓取负载里的 URL。
fn jenkins_notification(
    stream: &mut TcpStream,
    state: &ServerState,
    connection: &mut Connection,
    endpoint: &EndpointAuth,
    head: &http::RequestHead,
) -> Result<Response, Failure> {
    let with_endpoint = |failure: Failure| failure.with_endpoint(&endpoint.id);
    if let Some(retry_after) = rate_limited(state, &endpoint.id) {
        return Err(Failure::rate_limited(retry_after).with_endpoint(&endpoint.id));
    }
    if !endpoint
        .allowed_kinds
        .iter()
        .any(|kind| kind == "parameters")
    {
        return Err(with_endpoint(Failure::unsupported(
            "该接入点未允许结构化参数，通知元数据无法入箱",
        )));
    }
    let body = read_json_body(stream, head, state.limits.max_body_bytes).map_err(with_endpoint)?;
    let payload: NotificationPayload = serde_json::from_slice(&body)
        .map_err(|error| with_endpoint(Failure::invalid(format!("通知 JSON 无法解析：{error}"))))?;
    let job = payload.into_job();
    let job_name = trimmed(job.name.as_deref());
    let job_display = trimmed(job.display_name.as_deref()).or(job_name);
    let job_url = trimmed(job.url.as_deref());
    let display = job_display.unwrap_or("Jenkins job");
    if job_name.is_none() && job_url.is_none() {
        return Err(with_endpoint(Failure::invalid(
            "通知缺少 name 与 url，无法确定来源身份",
        )));
    }
    let build = job
        .build
        .ok_or_else(|| with_endpoint(Failure::invalid("通知缺少 build 对象")))?;
    let number = build
        .number
        .filter(|number| *number >= 0)
        .ok_or_else(|| with_endpoint(Failure::invalid("通知缺少合法的 build.number")))?;
    let phase = trimmed(build.phase.as_deref())
        .ok_or_else(|| with_endpoint(Failure::invalid("通知缺少 build.phase")))?
        .to_ascii_uppercase();
    if phase != "FINALIZED" {
        return Err(with_endpoint(Failure::new(
            400,
            "unsupported_phase",
            format!("只接收 FINALIZED 阶段的通知（收到 {phase}）；请在 Jenkins 插件里把事件改为 FINALIZED"),
        )));
    }
    let status = trimmed(build.status.as_deref())
        .ok_or_else(|| with_endpoint(Failure::invalid("通知缺少 build.status")))?
        .to_ascii_uppercase();
    // job.url 含文件夹路径（job/folder/job/name/），短名不足以区分同名 Job；
    // 缺失时保留事件但明确标记身份未解析，不用短名猜完整路径。
    let identity = job_url.unwrap_or(display);
    let identity_resolved = job_url.is_some();
    let identity_hash = crate::shell::digest::hex(
        &Sha256::digest(format!("v1|{identity}|{number}|{phase}|{identity_resolved}").as_bytes())[..],
    );
    let external_id = format!("jenkins:{identity_hash}");
    let fingerprint = crate::shell::digest::hex(
        &Sha256::digest(format!("v1|{identity}|{number}|{phase}|{status}").as_bytes())[..],
    );
    let now = crate::shell::process::epoch_millis() as i64;
    let occurred_at = build
        .timestamp
        .filter(|timestamp| *timestamp > 0 && *timestamp <= now + 86_400_000)
        .unwrap_or(now);

    let mut parameters = serde_json::Map::new();
    parameters.insert("source".into(), serde_json::json!("jenkins-notification"));
    parameters.insert("phase".into(), serde_json::json!(phase));
    parameters.insert("reported_result".into(), serde_json::json!(status));
    parameters.insert("build_number".into(), serde_json::json!(number));
    parameters.insert(
        "identity_resolved".into(),
        serde_json::json!(identity_resolved),
    );
    parameters.insert(
        "job_name".into(),
        serde_json::json!(job_name.unwrap_or(display)),
    );
    if let Some(value) = job_display {
        parameters.insert("job_display_name".into(), serde_json::json!(value));
    }
    if let Some(value) = job_url {
        parameters.insert("job_url".into(), serde_json::json!(value));
    }
    for (key, value) in [
        ("build_display_name", build.display_name),
        ("build_url", build.url),
        ("full_url", build.full_url),
    ] {
        if let Some(value) = trimmed(value.as_deref()) {
            parameters.insert(key.into(), serde_json::json!(value));
        }
    }
    if let Some(value) = build.queue_id {
        parameters.insert("queue_id".into(), serde_json::json!(value));
    }
    if let Some(value) = build.duration {
        parameters.insert("duration_ms".into(), serde_json::json!(value));
    }
    if let Some(value) = build.timestamp {
        parameters.insert("timestamp".into(), serde_json::json!(value));
    }
    if let Some(value) = build.parameters {
        parameters.insert("build_parameters".into(), value);
    }
    if let Some(value) = build.scm {
        parameters.insert("scm".into(), value);
    }
    if let Some(value) = build.test_summary {
        parameters.insert("test_summary".into(), value);
    }
    if let Some(value) = trimmed(build.notes.as_deref()) {
        parameters.insert(
            "notes".into(),
            serde_json::json!(value.chars().take(2000).collect::<String>()),
        );
    }
    let parameters = serde_json::Value::Object(parameters);
    protocol::validate_parameters(&parameters)
        .map_err(|error| protocol_failure(error, &endpoint.id, None))?;

    let title: String = format!("{display} #{number} {status}")
        .chars()
        .take(120)
        .collect();
    let new_event = store::NewEvent {
        external_id: &external_id,
        fingerprint: &fingerprint,
        event_type: "jenkins.notification",
        occurred_at,
        title: &title,
        text: "",
        parameters: &parameters,
        sender_policy: None,
        commit: true,
        attachments: &[],
    };
    ensure_event_disk_room(
        connection,
        &endpoint.id,
        new_event.external_id,
        body.len() as u64,
    )?;
    let (row, created, evicted) =
        store::create_event_evicting(connection, &endpoint.id, &new_event, now)
            .map_err(|error| store_failure(error, Some(&endpoint.id), None, "event_conflict"))?;
    remove_evicted_attachments(evicted);
    let response = protocol::EventStatusResponse {
        event_id: row.id.clone(),
        state: row.state,
        attachments: Vec::new(),
        expires_at: row.expires_at,
        receipt_url: protocol::receipt_url(&endpoint.id, &row.id),
    };
    Ok(Response::json(if created { 201 } else { 200 }, &response))
}

// 已存在的幂等回执不新增正文，不因磁盘水位不足而拒绝重试。
fn ensure_event_disk_room(
    connection: &Connection,
    endpoint_id: &str,
    external_id: &str,
    required: u64,
) -> Result<(), Failure> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE endpoint_id = ?1 AND external_id = ?2)",
            rusqlite::params![endpoint_id, external_id],
            |row| row.get(0),
        )
        .map_err(|error| {
            store_failure(
                StoreError::from(error),
                Some(endpoint_id),
                None,
                "event_conflict",
            )
        })?;
    if !exists {
        blob::disk_has_room(required.saturating_add(4096)).map_err(|message| {
            Failure::new(503, "service_unavailable", message).with_endpoint(endpoint_id)
        })?;
    }
    Ok(())
}

fn create_event(
    stream: &mut TcpStream,
    state: &ServerState,
    connection: &mut Connection,
    endpoint: &EndpointAuth,
    head: &http::RequestHead,
) -> Result<Response, Failure> {
    if let Some(retry_after) = rate_limited(state, &endpoint.id) {
        return Err(Failure::rate_limited(retry_after).with_endpoint(&endpoint.id));
    }
    let body = read_json_body(stream, head, state.limits.max_body_bytes)
        .map_err(|failure| failure.with_endpoint(&endpoint.id))?;
    let request: CreateEventRequest = serde_json::from_slice(&body).map_err(|error| {
        Failure::invalid(format!("JSON 请求体无法解析：{error}")).with_endpoint(&endpoint.id)
    })?;
    let event = protocol::normalize(request)
        .map_err(|error| protocol_failure(error, &endpoint.id, None))?;
    if !event.text.is_empty() && !endpoint.allowed_kinds.iter().any(|kind| kind == "text") {
        return Err(Failure::unsupported("该接入点未允许文本内容").with_endpoint(&endpoint.id));
    }
    let has_parameters = event
        .parameters
        .as_object()
        .is_some_and(|map| !map.is_empty());
    if has_parameters
        && !endpoint
            .allowed_kinds
            .iter()
            .any(|kind| kind == "parameters")
    {
        return Err(Failure::unsupported("该接入点未允许结构化参数").with_endpoint(&endpoint.id));
    }

    let new_attachments: Vec<store::NewAttachment<'_>> = event
        .attachments
        .iter()
        .map(|attachment| store::NewAttachment {
            client_id: &attachment.client_id,
            kind: &attachment.kind,
            filename: &attachment.filename,
            media_type: &attachment.media_type,
            encoding: attachment.encoding.as_deref(),
            size_bytes: attachment.size_bytes as i64,
        })
        .collect();
    let now = crate::shell::process::epoch_millis() as i64;
    let new_event = store::NewEvent {
        external_id: &event.external_id,
        fingerprint: &event.fingerprint,
        event_type: &event.event_type,
        occurred_at: event.occurred_at_ms,
        title: &event.title,
        text: &event.text,
        parameters: &event.parameters,
        sender_policy: event.sender_policy.as_deref(),
        commit: event.commit,
        attachments: &new_attachments,
    };
    ensure_event_disk_room(
        connection,
        &endpoint.id,
        new_event.external_id,
        body.len() as u64,
    )?;
    let (row, created, evicted) =
        store::create_event_evicting(connection, &endpoint.id, &new_event, now)
            .map_err(|error| store_failure(error, Some(&endpoint.id), None, "event_conflict"))?;
    remove_evicted_attachments(evicted);
    let attachments = created_attachments(connection, &row.id, &endpoint.id)?;
    let response = protocol::EventStatusResponse {
        event_id: row.id.clone(),
        state: row.state,
        attachments,
        expires_at: row.expires_at,
        receipt_url: protocol::receipt_url(&endpoint.id, &row.id),
    };
    Ok(Response::json(if created { 201 } else { 200 }, &response))
}

fn commit_event(
    connection: &mut Connection,
    endpoint: &EndpointAuth,
    event_id: &str,
    head: &http::RequestHead,
) -> Result<Response, Failure> {
    if head
        .headers
        .get("content-length")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .is_some_and(|length| length > 0)
    {
        return Err(Failure::invalid("commit 不接受请求体").with_endpoint(&endpoint.id));
    }
    let now = crate::shell::process::epoch_millis() as i64;
    let rows = store::event_attachments(connection, event_id).map_err(|error| {
        store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
    })?;
    let missing = rows
        .iter()
        .filter(|attachment| attachment.state != store::ATTACHMENT_STORED)
        .count();
    if missing > 0 {
        return Err(Failure::new(
            409,
            "attachments_incomplete",
            format!("还有 {missing} 个附件未完成上传"),
        )
        .with_endpoint(&endpoint.id)
        .with_event(event_id));
    }
    for attachment in &rows {
        let Some(rel) = attachment.storage_rel.as_deref() else {
            store::mark_event_unavailable(connection, event_id).map_err(|error| {
                store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
            })?;
            return Err(Failure::new(
                409,
                "state_conflict",
                "附件文件缺失，事件已标记不可用，请重新发送",
            )
            .with_endpoint(&endpoint.id)
            .with_event(event_id));
        };
        if !blob::exists_with_size(rel, attachment.size_bytes) {
            store::mark_event_unavailable(connection, event_id).map_err(|error| {
                store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
            })?;
            return Err(Failure::new(
                409,
                "state_conflict",
                "附件文件缺失，事件已标记不可用，请重新发送",
            )
            .with_endpoint(&endpoint.id)
            .with_event(event_id));
        }
    }
    let row = store::commit_event(connection, &endpoint.id, event_id, now).map_err(|error| {
        store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
    })?;
    let response = protocol::EventStatusResponse {
        event_id: row.id.clone(),
        state: row.state,
        attachments: attachment_created_from_rows(&rows),
        expires_at: row.expires_at,
        receipt_url: protocol::receipt_url(&endpoint.id, &row.id),
    };
    Ok(Response::json(200, &response))
}

fn receipt(
    connection: &mut Connection,
    endpoint: &EndpointAuth,
    event_id: &str,
) -> Result<Response, Failure> {
    let now = crate::shell::process::epoch_millis() as i64;
    let row = store::receipt_event(connection, &endpoint.id, event_id, now).map_err(|error| {
        store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
    })?;
    let attachments = store::event_attachments(connection, &row.id).map_err(|error| {
        store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
    })?;
    let response = protocol::ReceiptResponse {
        event_id: row.id,
        state: row.state,
        attachments: attachment_views(&attachments),
        occurred_at: row.occurred_at,
        received_at: row.received_at,
        expires_at: row.expires_at,
    };
    Ok(Response::json(200, &response))
}

fn upload_attachment(
    stream: &mut TcpStream,
    state: &ServerState,
    connection: &mut Connection,
    endpoint: &EndpointAuth,
    event_id: &str,
    attachment_id: &str,
    head: &http::RequestHead,
) -> Result<Response, Failure> {
    let now = crate::shell::process::epoch_millis() as i64;
    let event = store::receipt_event(connection, &endpoint.id, event_id, now).map_err(|error| {
        store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
    })?;
    if event.state != protocol::STATE_RECEIVING {
        return Err(
            Failure::new(409, "state_conflict", "事件已不是接收中状态，不能上传附件")
                .with_endpoint(&endpoint.id)
                .with_event(event_id),
        );
    }
    let attachment = store::attachment(connection, event_id, attachment_id)
        .map_err(|error| {
            store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
        })?
        .ok_or_else(Failure::not_found)?;
    let declared = upload_declared_length(head, &attachment)?;

    // 已 stored：同一附件重传只允许字节一致（返回原结果），不同内容必须冲突。
    if attachment.state == store::ATTACHMENT_STORED {
        let (size, sha256) = hash_body(stream, head, declared)?;
        if size == attachment.size_bytes as u64
            && attachment.sha256.as_deref() == Some(sha256.as_str())
        {
            return Ok(Response::json(
                200,
                &protocol::UploadResponse {
                    attachment_id: attachment.id,
                    state: attachment.state,
                    size_bytes: size,
                    sha256,
                },
            ));
        }
        return Err(Failure::new(
            409,
            "attachment_mismatch",
            "附件已存储，重传内容必须与原文件一致",
        )
        .with_endpoint(&endpoint.id)
        .with_event(event_id));
    }

    let _slot = UploadSlot::acquire(state, &endpoint.id)?;
    blob::disk_has_room(attachment.size_bytes.max(0) as u64).map_err(|message| {
        Failure::new(503, "service_unavailable", message)
            .with_endpoint(&endpoint.id)
            .with_event(event_id)
    })?;
    let upload_owner = format!("{:032x}", rand::random::<u128>());
    store::begin_upload(connection, event_id, attachment_id, now, &upload_owner).map_err(
        |error| match error {
            StoreError::Conflict => Failure::new(
                409,
                "attachment_busy",
                "该附件正在上传，请稍后查询回执或重试",
            )
            .with_endpoint(&endpoint.id)
            .with_event(event_id),
            other => store_failure(other, Some(&endpoint.id), Some(event_id), "state_conflict"),
        },
    )?;

    let (mut file, temp_path) = blob::create_temp().map_err(|message| {
        let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
        Failure::new(503, "service_unavailable", message)
            .with_endpoint(&endpoint.id)
            .with_event(event_id)
    })?;
    let hasher = match stream_body_to_temp(stream, head, declared, &mut file) {
        Ok(hasher) => hasher,
        Err(failure) => {
            let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
            drop(file);
            remove_temp(&temp_path);
            return Err(failure.with_endpoint(&endpoint.id).with_event(event_id));
        }
    };
    let sha256 = crate::shell::digest::hex(&hasher.finalize()[..]);
    if let Some(client_digest) = head.headers.get("x-content-sha256") {
        let client_digest = client_digest.trim().to_ascii_lowercase();
        if client_digest != sha256 {
            let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
            drop(file);
            remove_temp(&temp_path);
            return Err(Failure::invalid("客户端摘要与上传内容不一致")
                .with_endpoint(&endpoint.id)
                .with_event(event_id));
        }
    }
    let size_bytes = declared as u64;
    if size_bytes != attachment.size_bytes.max(0) as u64 {
        let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
        drop(file);
        remove_temp(&temp_path);
        return Err(Failure::invalid("上传字节数与声明大小不一致")
            .with_endpoint(&endpoint.id)
            .with_event(event_id));
    }
    if let Err(reject) = validate_attachment_content(&temp_path, &attachment) {
        let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
        drop(file);
        remove_temp(&temp_path);
        return Err(
            Failure::new(reject.status, media_code(reject.status), reject.message)
                .with_endpoint(&endpoint.id)
                .with_event(event_id),
        );
    }
    if let Err(error) = file.sync_all() {
        let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
        drop(file);
        remove_temp(&temp_path);
        return Err(Failure::new(
            503,
            "service_unavailable",
            format!("无法持久化附件：{error}"),
        )
        .with_endpoint(&endpoint.id)
        .with_event(event_id));
    }
    drop(file);
    let rel = match blob::place(&temp_path, event_id, attachment_id) {
        Ok(rel) => rel,
        Err(message) => {
            let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
            remove_temp(&temp_path);
            return Err(Failure::new(503, "service_unavailable", message)
                .with_endpoint(&endpoint.id)
                .with_event(event_id));
        }
    };
    let row = match store::finish_upload(
        connection,
        event_id,
        attachment_id,
        &sha256,
        size_bytes as i64,
        &rel,
        now,
        &upload_owner,
    ) {
        Ok(row) => row,
        Err(error) => {
            let _ = blob::remove_rel(&rel);
            let _ = store::abort_upload(connection, event_id, attachment_id, now, &upload_owner);
            return Err(store_failure(
                error,
                Some(&endpoint.id),
                Some(event_id),
                "state_conflict",
            ));
        }
    };
    Ok(Response::json(
        200,
        &protocol::UploadResponse {
            attachment_id: row.id,
            state: row.state,
            size_bytes,
            sha256,
        },
    ))
}

fn upload_declared_length(
    head: &http::RequestHead,
    attachment: &AttachmentRow,
) -> Result<usize, Failure> {
    if let Some(encoding) = head.headers.get("content-encoding") {
        if !encoding.eq_ignore_ascii_case("identity") {
            return Err(Failure::unsupported("不支持压缩的附件请求体"));
        }
    }
    if head.headers.contains_key("transfer-encoding") {
        return Err(Failure::invalid("不支持分块传输，请使用 Content-Length"));
    }
    let content_type = head
        .headers
        .get("content-type")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let base = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if base != attachment.media_type {
        return Err(Failure::unsupported(format!(
            "Content-Type 必须为 {}",
            attachment.media_type
        )));
    }
    let declared = head
        .headers
        .get("content-length")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .ok_or_else(|| Failure::invalid("附件上传必须带 Content-Length"))?;
    let expected = attachment.size_bytes.max(0) as usize;
    if declared > expected {
        return Err(Failure::too_large(format!(
            "上传大小超过声明的 {} 字节",
            expected
        )));
    }
    Ok(declared)
}

fn validate_attachment_content(
    path: &Path,
    attachment: &AttachmentRow,
) -> Result<(), media::MediaReject> {
    match attachment.kind.as_str() {
        protocol::KIND_IMAGE => {
            let bytes = std::fs::read(path).map_err(|error| media::MediaReject {
                status: 503,
                message: format!("无法读取临时附件：{error}"),
            })?;
            media::validate_image(&bytes, &attachment.media_type, protocol::MAX_IMAGE_PIXELS)?;
            Ok(())
        }
        protocol::KIND_LOG => {
            media::validate_text_file(path, attachment.encoding.as_deref().unwrap_or("utf-8"))
                .map_err(|message| media::MediaReject {
                    status: 400,
                    message,
                })
        }
        _ => Err(media::MediaReject {
            status: 415,
            message: "不支持的附件类型".into(),
        }),
    }
}

fn media_code(status: u16) -> &'static str {
    match status {
        413 => "too_large",
        415 => "unsupported_media_type",
        503 => "service_unavailable",
        _ => "invalid_request",
    }
}

fn read_body_hashed(
    stream: &mut TcpStream,
    head: &http::RequestHead,
    declared: usize,
    mut write: impl FnMut(&[u8]) -> Result<(), Failure>,
) -> Result<Sha256, Failure> {
    let mut hasher = Sha256::new();
    let mut remaining = declared;
    let prefix = &head.body_prefix;
    let take = prefix.len().min(remaining);
    if take > 0 {
        write(&prefix[..take])?;
        hasher.update(&prefix[..take]);
        remaining -= take;
    }
    let deadline = Instant::now() + UPLOAD_DEADLINE;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        if Instant::now() > deadline {
            return Err(Failure::invalid("附件上传超时"));
        }
        let want = buffer.len().min(remaining);
        let read = stream
            .read(&mut buffer[..want])
            .map_err(|error| Failure::invalid(format!("读取附件失败：{error}")))?;
        if read == 0 {
            return Err(Failure::invalid("附件请求体不完整"));
        }
        write(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        remaining -= read;
    }
    Ok(hasher)
}

fn stream_body_to_temp(
    stream: &mut TcpStream,
    head: &http::RequestHead,
    declared: usize,
    file: &mut File,
) -> Result<Sha256, Failure> {
    read_body_hashed(stream, head, declared, |chunk| {
        file.write_all(chunk).map_err(|error| {
            Failure::new(503, "service_unavailable", format!("无法写入附件：{error}"))
        })
    })
}

fn hash_body(
    stream: &mut TcpStream,
    head: &http::RequestHead,
    declared: usize,
) -> Result<(u64, String), Failure> {
    let hasher = read_body_hashed(stream, head, declared, |_| Ok(()))?;
    Ok((
        declared as u64,
        crate::shell::digest::hex(&hasher.finalize()[..]),
    ))
}

fn remove_temp(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn created_attachments(
    connection: &Connection,
    event_id: &str,
    endpoint_id: &str,
) -> Result<Vec<AttachmentCreated>, Failure> {
    let rows = store::event_attachments(connection, event_id).map_err(|error| {
        store_failure(error, Some(endpoint_id), Some(event_id), "event_conflict")
    })?;
    Ok(attachment_created_from_rows(&rows))
}

fn attachment_created_from_rows(rows: &[AttachmentRow]) -> Vec<AttachmentCreated> {
    rows.iter()
        .map(|attachment| AttachmentCreated {
            client_id: attachment.client_id.clone(),
            attachment_id: attachment.id.clone(),
        })
        .collect()
}

fn attachment_views(rows: &[AttachmentRow]) -> Vec<AttachmentView> {
    rows.iter()
        .map(|attachment| AttachmentView {
            attachment_id: attachment.id.clone(),
            client_id: attachment.client_id.clone(),
            kind: attachment.kind.clone(),
            filename: attachment.filename.clone(),
            state: attachment.state.clone(),
            size_bytes: attachment.size_bytes.max(0) as u64,
            sha256: attachment.sha256.clone(),
        })
        .collect()
}

struct UploadSlot<'a> {
    state: &'a ServerState,
    endpoint_id: String,
}

impl<'a> UploadSlot<'a> {
    fn acquire(state: &'a ServerState, endpoint_id: &str) -> Result<Self, Failure> {
        let mut uploads = state
            .uploads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = uploads.entry(endpoint_id.to_string()).or_insert(0);
        if *count >= state.limits.uploads_per_endpoint {
            return Err(Failure::rate_limited(1).with_endpoint(endpoint_id));
        }
        *count += 1;
        Ok(Self {
            state,
            endpoint_id: endpoint_id.to_string(),
        })
    }
}

impl Drop for UploadSlot<'_> {
    fn drop(&mut self) {
        let mut uploads = self
            .state
            .uploads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = uploads.get_mut(&self.endpoint_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                uploads.remove(&self.endpoint_id);
            }
        }
    }
}

fn read_json_body(
    stream: &mut TcpStream,
    head: &http::RequestHead,
    max_bytes: usize,
) -> Result<Vec<u8>, Failure> {
    if let Some(encoding) = head.headers.get("content-encoding") {
        if !encoding.eq_ignore_ascii_case("identity") {
            return Err(Failure::unsupported("不支持压缩请求体"));
        }
    }
    if head.headers.contains_key("transfer-encoding") {
        return Err(Failure::invalid("不支持分块传输，请使用 Content-Length"));
    }
    let content_type = head
        .headers
        .get("content-type")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !content_type.starts_with("application/json") {
        return Err(Failure::unsupported("Content-Type 必须为 application/json"));
    }
    if let Some(declared) = head
        .headers
        .get("content-length")
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        if declared > max_bytes {
            return Err(Failure::too_large(format!(
                "请求体超过上限 {max_bytes} 字节"
            )));
        }
    }
    http::read_body(stream, head, max_bytes)
        .map_err(|error| Failure::invalid(format!("无法读取请求体：{error}")))
}

/// 从 Authorization: Basic base64(user:password) 取密码；用户名只为兼容
/// Jenkins 插件的 URL userinfo 写法，不参与身份判断。
fn basic_password(head: &http::RequestHead) -> Option<String> {
    let value = head.headers.get("authorization")?.trim();
    let encoded = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (_, password) = text.split_once(':')?;
    (!password.is_empty()).then(|| password.to_string())
}

fn notification_endpoint(
    connection: &Connection,
    head: &http::RequestHead,
) -> Result<EndpointAuth, Failure> {
    let password = basic_password(head).ok_or_else(Failure::unauthorized)?;
    store::auth_by_notification_token_hash(connection, &store::hash_token(&password))
        .map_err(|error| store_failure(error, None, None, "event_conflict"))?
        .ok_or_else(Failure::unauthorized)
}

fn bearer_token(head: &http::RequestHead) -> Option<&str> {
    let value = head.headers.get("authorization")?.trim();
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// 只接受 localhost 与 IP 字面量（含本机内网 IP），域名仍拒绝：监听所有网卡后
/// 保留 DNS rebinding 防护，同时允许内网按 IP 直接投递。
fn validate_host(head: &http::RequestHead, state: &ServerState) -> Result<(), Failure> {
    let host = head
        .headers
        .get("host")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let allowed = host == format!("localhost:{}", state.listen_port)
        || host.rsplit_once(':').is_some_and(|(name, port)| {
            port.parse::<u16>() == Ok(state.listen_port) && name.parse::<std::net::IpAddr>().is_ok()
        });
    if allowed {
        Ok(())
    } else {
        Err(Failure::invalid(
            "Host 头必须是本机 IP 或 localhost（反向代理需把 Host 改写为 IP:端口）",
        ))
    }
}

fn rate_limited(state: &ServerState, endpoint_id: &str) -> Option<u64> {
    let now = Instant::now();
    let mut map = state
        .rate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let window = map.entry(endpoint_id.to_string()).or_insert(RateWindow {
        started: now,
        count: 0,
    });
    if now.duration_since(window.started) >= RATE_WINDOW {
        window.started = now;
        window.count = 0;
    }
    if window.count >= state.limits.rate_per_minute {
        let elapsed = now.duration_since(window.started).as_secs();
        return Some(RATE_WINDOW.as_secs().saturating_sub(elapsed).max(1));
    }
    window.count += 1;
    None
}

fn protocol_failure(error: RequestError, endpoint_id: &str, event_id: Option<&str>) -> Failure {
    let mut failure =
        Failure::new(error.status, error.code, error.message).with_endpoint(endpoint_id);
    if let Some(event_id) = event_id {
        failure = failure.with_event(event_id);
    }
    failure
}

/// 容量清理在事务里只清数据库；文件在提交后删。失败时文件成孤儿，由维护线程回收。
fn remove_evicted_attachments(targets: Vec<store::CleanupTarget>) {
    if targets.is_empty() {
        return;
    }
    let files: usize = targets.iter().map(|target| target.rels.len()).sum();
    crate::shell::shell_events::record(
        "hooks",
        &format!(
            "容量清理：{} 个历史事件、{files} 个附件已删除",
            targets.len()
        ),
    );
    for target in targets {
        for rel in target.rels {
            if let Err(error) = blob::remove_rel(&rel) {
                crate::shell::shell_events::record(
                    "hooks",
                    &format!(
                        "容量清理删除附件失败（event={}，{rel}）：{error}",
                        target.event_id
                    ),
                );
            }
        }
    }
}

fn store_failure(
    error: StoreError,
    endpoint_id: Option<&str>,
    event_id: Option<&str>,
    conflict_code: &'static str,
) -> Failure {
    let mut failure = match error {
        StoreError::NotFound => Failure::not_found(),
        StoreError::Conflict => Failure::new(
            409,
            conflict_code,
            "同一 external_id 已存在且内容不同，或当前状态不允许该操作",
        ),
        StoreError::QuotaExceeded(limit) => Failure::too_large(format!(
            "Hook 存储预算已满（上限 {limit} 字节），且没有可清理的历史事件；请等接收完成或手动删除"
        )),
        StoreError::Unavailable(reason) => {
            crate::shell::shell_events::record("hooks", &format!("收件箱不可用：{reason}"));
            Failure::new(
                503,
                "service_unavailable",
                "收件箱暂不可用，请稍后重试；详情见桌面端日志",
            )
        }
    };
    if let Some(endpoint_id) = endpoint_id {
        failure = failure.with_endpoint(endpoint_id);
    }
    if let Some(event_id) = event_id {
        failure = failure.with_event(event_id);
    }
    failure
}

fn write_response(stream: &mut TcpStream, response: Response) {
    let headers: Vec<(&str, &str)> = response
        .headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let _ = http::write_response_with_headers(
        stream,
        response.status,
        "application/json",
        &response.body,
        &headers,
    );
}

fn write_failure(stream: &mut TcpStream, failure: &Failure, request_id: &str) {
    let body = protocol::ErrorResponse {
        error: protocol::ErrorDetail {
            code: failure.code.to_string(),
            message: failure.message.clone(),
            request_id: request_id.to_string(),
        },
    };
    let mut response = Response::json(failure.status, &body);
    if let Some(retry_after) = failure.retry_after {
        response = response.with_header("Retry-After", retry_after.to_string());
    }
    write_response(stream, response);
}
#[cfg(test)]
mod tests {
    #[test]
    fn source_outside_allowed_subnets_is_rejected() {
        let interface = crate::hooks::network::LanInterface {
            name: "test-lan".into(),
            address: "10.1.2.3".into(),
            netmask: "255.255.255.0".into(),
            cidr: "10.1.2.0/24".into(),
        };
        let config = crate::hooks::config::HooksConfig {
            interface_name: interface.name.clone(),
            bind_ip: interface.address.clone(),
            netmask: interface.netmask.clone(),
            ..Default::default()
        };
        let policy =
            crate::hooks::network::LanPolicy::resolve(&config, &[interface]).expect("策略");
        assert!(source_allowed(Some(&policy), "10.1.2.20".parse().unwrap()));
        assert!(source_allowed(Some(&policy), "127.0.0.1".parse().unwrap()));
        assert!(!source_allowed(
            Some(&policy),
            "192.168.1.20".parse().unwrap()
        ));
        assert!(source_allowed(None, "8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn missing_confirmed_network_stops_without_ui_status_polling() {
        let interface = crate::hooks::network::LanInterface {
            name: "nonexistent-test-network".into(),
            address: "10.254.254.254".into(),
            netmask: "255.255.255.0".into(),
            cidr: "10.254.254.0/24".into(),
        };
        let config = crate::hooks::config::HooksConfig {
            interface_name: interface.name.clone(),
            bind_ip: interface.address.clone(),
            netmask: interface.netmask.clone(),
            ..Default::default()
        };
        let policy = crate::hooks::network::LanPolicy::resolve(&config, &[interface]).unwrap();
        let home =
            std::env::temp_dir().join(format!("hook-network-loss-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&home).unwrap();
        let _guard = crate::tests::scoped_xlink_home(&home);
        let handle = super::start_with_policy(
            vec!["127.0.0.1:0".parse().unwrap()],
            home.join("db"),
            super::Limits::default(),
            Some(policy),
        )
        .unwrap();
        let start = std::time::Instant::now();
        while handle.running() && start.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !handle.running(),
            "accept loop must independently revoke a disappeared network"
        );
        handle.stop();
        std::fs::remove_dir_all(home).unwrap();
    }

    use super::*;
    use crate::tests::{scoped_xlink_home, EnvGuard};
    use std::io::{Read, Write};
    use std::path::PathBuf;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dsh-xlink-hooks-server-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&dir).expect("创建临时目录");
            Self(dir)
        }

        fn db(&self) -> PathBuf {
            self.0.join("inbox.db")
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn setup_with_limits(
        label: &str,
        limits: Limits,
    ) -> (TempHome, EnvGuard, ServerHandle, String, String) {
        let home = TempHome::new(label);
        let guard = scoped_xlink_home(&home.0);
        let connection = store::open(&home.db()).expect("建库");
        let (endpoint, token) = store::create_endpoint(
            &connection,
            "测试接入点",
            &["text".to_string(), "parameters".to_string()],
        )
        .expect("建接入点");
        drop(connection);
        let handle = start("127.0.0.1:0".parse().expect("回环地址"), home.db(), limits)
            .expect("启动测试服务");
        (home, guard, handle, endpoint.id, token)
    }

    #[test]
    fn unclean_thread_exit_cannot_be_reported_as_clean_stop() {
        for panic_in_accept in [false, true] {
            let failed = thread::spawn(|| panic!("injected worker failure"));
            let mut handle = ServerHandle {
                addr: "127.0.0.1:0".parse().unwrap(),
                cancel: Arc::new(AtomicBool::new(false)),
                accepts: Vec::new(),
                workers: Vec::new(),
                streams: Arc::new(Mutex::new(HashMap::new())),
            };
            if panic_in_accept {
                handle.accepts.push(failed);
            } else {
                handle.workers.push(failed);
            }
            assert!(!handle.stop_cleanly());
        }
        let (home, _guard, handle, _, _) = setup_with_limits("clean-stop", Limits::default());
        assert!(handle.stop_cleanly());
        assert!(home.db().exists());
    }

    fn confirmed_test_policy() -> (LanPolicy, crate::hooks::network::InterfaceGuard) {
        let interface = crate::hooks::network::LanInterface {
            name: "test-confirmed-lan".into(),
            address: "10.253.252.1".into(),
            netmask: "255.255.255.0".into(),
            cidr: "10.253.252.0/24".into(),
        };
        let config = crate::hooks::config::HooksConfig {
            interface_name: interface.name.clone(),
            bind_ip: interface.address.clone(),
            netmask: interface.netmask.clone(),
            ..Default::default()
        };
        let policy = LanPolicy::resolve(&config, std::slice::from_ref(&interface)).unwrap();
        (
            policy,
            crate::hooks::network::test_interfaces(vec![interface]),
        )
    }

    #[test]
    fn forbidden_source_gets_real_http_403_before_dispatch() {
        let home = TempHome::new("source-http-403");
        let _home_guard = scoped_xlink_home(&home.0);
        let (policy, _interfaces) = confirmed_test_policy();
        // IPv6 loopback is outside the IPv4-only policy. The actual TCP peer is checked;
        // only interface discovery is injected, not accept/dispatch/HTTP serialization.
        let handle = start_with_policy(
            vec!["[::1]:0".parse().unwrap()],
            home.db(),
            Limits::default(),
            Some(policy),
        )
        .unwrap();
        let (status, _, body) = request(
            &handle,
            "POST",
            "/hooks/v1/endpoints/not-created/events",
            &[("Authorization", "Bearer must-not-be-reflected")],
            Some(b"{\"text\":\"unread request bytes\"}"),
        );
        assert_eq!(status, 403, "denied TCP peer must receive HTTP 403: {body}");
        assert_eq!(body_json(&body)["error"]["code"], "forbidden");
        assert!(!body.contains("must-not-be-reflected"));
        assert!(
            !home.db().exists(),
            "source denial must precede database dispatch"
        );
        handle.stop();
    }

    #[test]
    fn slow_forbidden_client_does_not_block_stop_or_lose_403() {
        let home = TempHome::new("source-slow-403");
        let _home_guard = scoped_xlink_home(&home.0);
        let (policy, _interfaces) = confirmed_test_policy();
        let handle = start_with_policy(
            vec!["[::1]:0".parse().unwrap()],
            home.db(),
            Limits::default(),
            Some(policy),
        )
        .unwrap();
        let mut client = TcpStream::connect(handle.addr).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        client
            .write_all(
                b"POST /hooks/v1/endpoints/x/events HTTP/1.1\r\nContent-Length: 1000000\r\n\r\na",
            )
            .unwrap();
        let mut writer = client.try_clone().unwrap();
        let sending = Arc::new(AtomicBool::new(true));
        let keep_sending = Arc::clone(&sending);
        let producer = thread::spawn(move || {
            writer
                .set_write_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            while keep_sending.load(Ordering::SeqCst) {
                if writer.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        let mut response = String::new();
        let _ = client.read_to_string(&mut response);
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        let began = Instant::now();
        handle.stop();
        sending.store(false, Ordering::SeqCst);
        producer.join().unwrap();
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "slow denial delayed stop"
        );
    }

    #[test]
    fn network_loss_closes_inflight_slow_request_without_ui_polling() {
        let home = TempHome::new("network-loss-slow-request");
        let _home_guard = scoped_xlink_home(&home.0);
        let (policy, _interfaces) = confirmed_test_policy();
        let handle = start_with_policy(
            vec!["127.0.0.1:0".parse().unwrap()],
            home.db(),
            Limits {
                read_timeout: Duration::from_secs(30),
                ..Limits::default()
            },
            Some(policy),
        )
        .unwrap();
        let addr = handle.addr;
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        // Incomplete headers keep the actual production worker blocked in request parsing.
        client
            .write_all(b"POST /hooks/v1/endpoints/x/events HTTP/1.1\r\nHost: ")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while handle.streams.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !handle.streams.lock().unwrap().is_empty(),
            "request must enter tracked production path"
        );
        let _lost = crate::hooks::network::test_interfaces(Vec::new());
        let began = Instant::now();
        let mut byte = [0u8; 1];
        match client.read(&mut byte) {
            Ok(0) => {}
            Err(error) => assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ),
                "expected connection closure, not timeout: {error}"
            ),
            other => panic!("unexpected slow-client result: {other:?}"),
        }
        assert!(
            !handle.running(),
            "network monitor must cancel without any UI call"
        );
        handle.stop();
        assert!(
            began.elapsed() < Duration::from_secs(3),
            "network loss waited for 30s header timeout"
        );
        assert!(TcpStream::connect(addr).is_err(), "listener must be closed");
    }

    fn host_state(port: u16) -> ServerState {
        ServerState {
            db_path: PathBuf::new(),
            limits: Limits::default(),
            listen_port: port,
            rate: Mutex::new(HashMap::new()),
            uploads: Mutex::new(HashMap::new()),
        }
    }

    fn head_with_host(host: &str) -> http::RequestHead {
        http::RequestHead {
            method: "POST".into(),
            path: "/hooks/v1/endpoints/x/events".into(),
            headers: [("host".to_string(), host.to_string())]
                .into_iter()
                .collect(),
            body_prefix: Vec::new(),
        }
    }

    #[test]
    fn host_header_accepts_ip_literals_but_still_rejects_domains() {
        let state = host_state(17890);
        for host in [
            "127.0.0.1:17890",
            "localhost:17890",
            "192.168.1.20:17890",
            "10.0.0.5:17890",
        ] {
            assert!(
                validate_host(&head_with_host(host), &state).is_ok(),
                "{host}"
            );
        }
        for host in ["evil.example:17890", "192.168.1.20:17891", "192.168.1.20"] {
            assert!(
                validate_host(&head_with_host(host), &state).is_err(),
                "{host}"
            );
        }
    }

    fn setup(label: &str) -> (TempHome, EnvGuard, ServerHandle, String, String) {
        setup_with_limits(
            label,
            Limits {
                rate_per_minute: 600,
                ..Limits::default()
            },
        )
    }

    fn text_body(external_id: &str, text: &str, commit: bool) -> String {
        serde_json::json!({
            "schema_version": "1.0",
            "external_id": external_id,
            "event_type": "build.materials",
            "occurred_at": "2026-11-15T02:00:00Z",
            "title": "测试事件",
            "text": text,
            "parameters": {"build": 128},
            "commit": commit,
        })
        .to_string()
    }

    fn mixed_body(external_id: &str, image_size: usize, log_size: usize) -> String {
        serde_json::json!({
            "schema_version": "1.0",
            "external_id": external_id,
            "event_type": "build.materials",
            "occurred_at": "2026-11-15T02:00:00Z",
            "title": "混合素材",
            "text": "构建失败",
            "parameters": {"build": 128},
            "attachments": [
                {
                    "client_id": "console",
                    "kind": "log",
                    "filename": "console.log",
                    "media_type": "text/plain",
                    "size_bytes": log_size,
                    "encoding": "utf-8"
                },
                {
                    "client_id": "screen",
                    "kind": "image",
                    "filename": "failure.png",
                    "media_type": "image/png",
                    "size_bytes": image_size
                }
            ],
            "commit": false,
        })
        .to_string()
    }

    /// 发一个原始 HTTP 请求，返回 (status, headers, body)。Host 默认按实际端口填。
    fn request(
        handle: &ServerHandle,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> (u16, String, String) {
        request_to(handle.addr, method, path, headers, body)
    }

    fn request_to(
        addr: SocketAddr,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> (u16, String, String) {
        let mut stream = TcpStream::connect(addr).expect("连接测试服务");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("读超时");
        let mut head = format!("{method} {path} HTTP/1.1\r\n");
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("host"))
        {
            head.push_str(&format!("Host: 127.0.0.1:{}\r\n", addr.port()));
        }
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if let Some(body) = body {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("Connection: close\r\n\r\n");
        stream.write_all(head.as_bytes()).expect("写请求头");
        if let Some(body) = body {
            stream.write_all(body).expect("写请求体");
        }
        let _ = stream.shutdown(Shutdown::Write);
        // 读超时是 per-read 的：响应可能分多段到达，不能因为一次 WouldBlock
        // 就把已读到的头当成完整响应。
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut raw = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => raw.extend_from_slice(&chunk[..read]),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    if Instant::now() >= deadline {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let raw = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw.as_str(), ""));
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        (status, head.to_string(), body.to_string())
    }

    fn create_headers(token: &str) -> Vec<(&'static str, String)> {
        vec![
            ("Authorization", format!("Bearer {token}")),
            ("Content-Type", "application/json".to_string()),
        ]
    }

    fn as_header_pairs<'a>(headers: &'a [(&'static str, String)]) -> Vec<(&'a str, &'a str)> {
        headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect()
    }

    fn create(
        handle: &ServerHandle,
        endpoint: &str,
        token: &str,
        body: &str,
    ) -> (u16, String, String) {
        let headers = create_headers(token);
        request(
            handle,
            "POST",
            &format!("/hooks/v1/endpoints/{endpoint}/events"),
            &as_header_pairs(&headers),
            Some(body.as_bytes()),
        )
    }

    fn receipt(
        handle: &ServerHandle,
        endpoint: &str,
        token: &str,
        event_id: &str,
    ) -> (u16, String, String) {
        let auth = format!("Bearer {token}");
        request(
            handle,
            "GET",
            &format!("/hooks/v1/endpoints/{endpoint}/events/{event_id}/receipt"),
            &[("Authorization", auth.as_str())],
            None,
        )
    }

    fn upload(
        handle: &ServerHandle,
        endpoint: &str,
        token: &str,
        event_id: &str,
        attachment_id: &str,
        body: &[u8],
        content_type: &str,
    ) -> (u16, String, String) {
        let auth = format!("Bearer {token}");
        request(
            handle,
            "PUT",
            &format!(
                "/hooks/v1/endpoints/{endpoint}/events/{event_id}/attachments/{attachment_id}"
            ),
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", content_type),
            ],
            Some(body),
        )
    }

    fn body_json(body: &str) -> serde_json::Value {
        serde_json::from_str(body).expect("响应是 JSON")
    }

    fn image_only_body(external_id: &str, image_size: usize) -> String {
        serde_json::json!({
            "schema_version": "1.0",
            "external_id": external_id,
            "event_type": "build.materials",
            "occurred_at": "2026-11-15T02:00:00Z",
            "title": "只有图片",
            "attachments": [{
                "client_id": "screen",
                "kind": "image",
                "filename": "failure.png",
                "media_type": "image/png",
                "size_bytes": image_size
            }],
            "commit": false,
        })
        .to_string()
    }

    fn attachment_id(body: &serde_json::Value, client_id: &str) -> String {
        body["attachments"]
            .as_array()
            .expect("attachments")
            .iter()
            .find(|item| item["client_id"] == client_id)
            .and_then(|item| item["attachment_id"].as_str())
            .expect("attachment id")
            .to_string()
    }

    fn png_bytes(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc = flate2::Crc::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(&crc.sum().to_be_bytes());
            out
        }
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut raw = Vec::new();
        for _ in 0..height {
            raw.extend_from_slice(&[0u8]);
            for _ in 0..width {
                raw.extend_from_slice(&color);
            }
        }
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).expect("压缩 PNG 行");
        let idat = encoder.finish().expect("完成 PNG 压缩");
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &idat));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    #[test]
    fn text_event_round_trip_is_idempotent_and_conflicts() {
        let (home, _guard, handle, endpoint, token) = setup("roundtrip");
        let payload = text_body("build:Project_Plague:128", "构建完成", true);
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201, "首次创建：{}", body);
        let event_id = body_json(&body)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();
        assert_eq!(body_json(&body)["state"], "ready");

        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        assert_eq!(body_json(&body)["state"], "ready");

        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 200, "相同内容重试返回原事件：{}", body);
        assert_eq!(body_json(&body)["event_id"], event_id);

        let changed = text_body("build:Project_Plague:128", "构建完成（内容已变）", true);
        let (status, _, body) = create(&handle, &endpoint, &token, &changed);
        assert_eq!(status, 409, "同 ID 不同内容必须冲突：{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "event_conflict");

        let connection = store::open(&home.db()).expect("读库");
        assert_eq!(store::counts(&connection).expect("计数").1, 1);
    }

    #[test]
    fn attachments_upload_commit_and_receipt() {
        let (home, _guard, handle, endpoint, token) = setup("attachments");
        let png = png_bytes(2, 2, [255, 0, 0, 255]);
        let log = b"line one\nline two\n";
        let payload = mixed_body("build:mixed:1", png.len(), log.len());
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201, "创建：{}", body);
        let event_id = body_json(&body)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();
        let image_id = attachment_id(&body_json(&body), "screen");
        let log_id = attachment_id(&body_json(&body), "console");

        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        assert_eq!(body_json(&body)["state"], "receiving");
        assert_eq!(body_json(&body)["attachments"][0]["state"], "pending");

        let auth = format!("Bearer {token}");
        let commit_path = format!("/hooks/v1/endpoints/{endpoint}/events/{event_id}/commit");
        let (status, _, body) = request(
            &handle,
            "POST",
            &commit_path,
            &[("Authorization", auth.as_str())],
            None,
        );
        assert_eq!(status, 409, "附件未上传不能提交：{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "attachments_incomplete");

        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &png,
            "image/png",
        );
        assert_eq!(status, 200, "图片上传：{}", body);
        let sha = body_json(&body)["sha256"]
            .as_str()
            .expect("sha")
            .to_string();
        assert_eq!(sha.len(), 64);
        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &png,
            "image/png",
        );
        assert_eq!(status, 200, "相同字节重传应幂等：{}", body);
        assert_eq!(body_json(&body)["sha256"], sha);

        let mut different = png.clone();
        let last = different.len() - 1;
        different[last] ^= 0xFF;
        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &different,
            "image/png",
        );
        assert_eq!(status, 409, "已存储附件不得替换：{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "attachment_mismatch");

        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &log_id,
            log,
            "text/plain",
        );
        assert_eq!(status, 200, "日志上传：{}", body);

        let (status, _, body) = request(
            &handle,
            "POST",
            &commit_path,
            &[("Authorization", auth.as_str())],
            None,
        );
        assert_eq!(status, 200, "提交：{}", body);
        assert_eq!(body_json(&body)["state"], "ready");

        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &png,
            "image/png",
        );
        assert_eq!(status, 409, "ready 后不得上传：{}", body);

        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        let json = body_json(&body);
        assert_eq!(json["state"], "ready");
        assert_eq!(json["attachments"].as_array().expect("附件").len(), 2);
        assert!(json["attachments"]
            .as_array()
            .expect("附件")
            .iter()
            .all(|item| item["state"] == "stored"));

        // 数据库里两张附件都是 stored。
        let connection = store::open(&home.db()).expect("读库");
        let rows = store::event_attachments(&connection, &event_id).expect("附件");
        assert!(rows.iter().all(|row| row.state == store::ATTACHMENT_STORED));
        assert!(rows.iter().all(|row| row.storage_rel.is_some()));
    }

    #[test]
    fn upload_rejects_bad_payloads_and_headers() {
        let (_home, _guard, handle, endpoint, token) = setup("upload-reject");
        let png = png_bytes(1, 1, [0, 0, 255, 255]);
        let log = b"hello\n";
        let payload = mixed_body("build:reject:1", png.len(), log.len());
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201);
        let image_id = attachment_id(&body_json(&body), "screen");
        let log_id = attachment_id(&body_json(&body), "console");
        let event_id = body_json(&body)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();

        let garbage = vec![0u8; png.len()];
        let (status, _, _) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &garbage,
            "image/png",
        );
        assert_eq!(status, 400, "伪造图片必须拒绝");

        let invalid_utf8 = vec![0xFFu8; log.len()];
        let (status, _, _) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &log_id,
            &invalid_utf8,
            "text/plain",
        );
        assert_eq!(status, 400, "声明 utf-8 的非法字节必须拒绝");

        let (status, _, _) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &log_id,
            log,
            "application/octet-stream",
        );
        assert_eq!(status, 415);

        let auth = format!("Bearer {token}");
        let (status, _, _) = request(
            &handle,
            "PUT",
            &format!("/hooks/v1/endpoints/{endpoint}/events/{event_id}/attachments/{log_id}"),
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "text/plain"),
                ("Content-Encoding", "gzip"),
            ],
            Some(log),
        );
        assert_eq!(status, 415);

        let (status, _, _) = request(
            &handle,
            "PUT",
            &format!("/hooks/v1/endpoints/{endpoint}/events/{event_id}/attachments/{log_id}"),
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "text/plain"),
            ],
            Some(&vec![b'x'; log.len() + 1]),
        );
        assert_eq!(status, 413, "超过声明大小的上传必须拒绝");

        let (status, _, _) = upload(
            &handle,
            &endpoint,
            "wrong-token",
            &event_id,
            &log_id,
            log,
            "text/plain",
        );
        assert_eq!(status, 401);

        let (status, _, _) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            "att_missing",
            log,
            "text/plain",
        );
        assert_eq!(status, 404);

        // 失败的上传不改变事件状态：仍是 receiving，附件仍是 pending 可重试。
        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        assert_eq!(body_json(&body)["state"], "receiving");
        assert!(body_json(&body)["attachments"]
            .as_array()
            .expect("附件")
            .iter()
            .all(|item| item["state"] == "pending"));
    }

    #[test]
    fn database_lock_returns_503_and_does_not_create_an_event() {
        let (home, _guard, handle, endpoint, token) = setup("db-lock");
        let mut locker = store::open(&home.db()).expect("锁连接");
        let tx = locker
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .expect("立即事务");
        let payload = text_body("build:locked", "锁住时不能落库", true);
        let started = std::time::Instant::now();
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 503, "{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "service_unavailable");
        assert!(
            started.elapsed() >= std::time::Duration::from_secs(1),
            "必须实际等锁，不能假装成功"
        );
        drop(tx);
        drop(locker);
        let connection = store::open(&home.db()).expect("读库");
        assert_eq!(store::counts(&connection).expect("计数").1, 0);
        let audited: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM request_audit WHERE code = 'service_unavailable'",
                [],
                |row| row.get(0),
            )
            .expect("审计");
        assert_eq!(
            audited, 0,
            "数据库锁住时审计写不进去，这是已知边界，靠壳日志兜底"
        );
        let logs: String = std::fs::read_dir(crate::shell::process::shell_logs_dir())
            .expect("日志目录")
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
            .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
            .collect();
        assert!(
            logs.contains("收件箱不可用"),
            "数据库锁必须留在壳日志里：{logs}"
        );
    }

    #[test]
    fn disk_full_upload_returns_503_and_is_audited() {
        let (home, _guard, handle, endpoint, token) = setup("disk-full");
        let png = png_bytes(1, 1, [0, 0, 255, 255]);
        let payload = image_only_body("build:disk-full", png.len());
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201);
        let json = body_json(&body);
        let event_id = json["event_id"].as_str().expect("event_id").to_string();
        let image_id = attachment_id(&json, "screen");

        let root = blob::root();
        blob::set_free_space_override(root.clone(), 0);
        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &png,
            "image/png",
        );
        blob::clear_free_space_override(&root);
        assert_eq!(status, 503, "{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "service_unavailable");

        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        assert_eq!(body_json(&body)["attachments"][0]["state"], "pending");
        let connection = store::open(&home.db()).expect("读库");
        let audited: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM request_audit WHERE code = 'service_unavailable'",
                [],
                |row| row.get(0),
            )
            .expect("审计");
        assert_eq!(audited, 1, "磁盘满拒绝必须留下请求审计");
    }

    #[test]
    fn interrupted_upload_keeps_attachment_pending_and_leaves_no_temp_file() {
        let (home, _guard, handle, endpoint, token) = setup("upload-interrupt");
        let png = png_bytes(1, 1, [0, 0, 255, 255]);
        let payload = image_only_body("build:interrupt", png.len());
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201);
        let json = body_json(&body);
        let event_id = json["event_id"].as_str().expect("event_id").to_string();
        let image_id = attachment_id(&json, "screen");

        let mut stream = TcpStream::connect(handle.addr).expect("连接");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("读超时");
        let head = format!(
            "PUT /hooks/v1/endpoints/{endpoint}/events/{event_id}/attachments/{image_id} HTTP/1.1\r\n             Host: 127.0.0.1:{}\r\n             Authorization: Bearer {token}\r\n             Content-Type: image/png\r\n             Content-Length: {}\r\n             Connection: close\r\n\r\n",
            handle.addr.port(),
            png.len()
        );
        stream.write_all(head.as_bytes()).expect("写头");
        stream
            .write_all(&png[..png.len() / 2])
            .expect("只写半个附件");
        stream.shutdown(Shutdown::Write).expect("提前关写端");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        assert!(
            response.starts_with("HTTP/1.1 400"),
            "半截上传必须明确拒绝：{response}"
        );

        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        let json = body_json(&body);
        assert_eq!(json["state"], "receiving");
        assert_eq!(json["attachments"][0]["state"], "pending");
        let temps = std::fs::read_dir(blob::temp_dir())
            .map(|entries| entries.flatten().count())
            .unwrap_or(0);
        assert_eq!(temps, 0, "中断上传不能留下临时文件");
        let connection = store::open(&home.db()).expect("读库");
        let audited: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM request_audit WHERE code = 'invalid_request'",
                [],
                |row| row.get(0),
            )
            .expect("审计");
        assert_eq!(audited, 1, "半截上传必须留下请求审计");
    }

    #[test]
    fn missing_attachment_file_marks_event_unavailable_on_commit() {
        let (home, _guard, handle, endpoint, token) = setup("missing-file");
        let png = png_bytes(1, 1, [0, 0, 255, 255]);
        let payload = image_only_body("build:missing-file:1", png.len());
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201);
        let event_id = body_json(&body)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();
        let image_id = attachment_id(&body_json(&body), "screen");
        let (status, _, body) = upload(
            &handle,
            &endpoint,
            &token,
            &event_id,
            &image_id,
            &png,
            "image/png",
        );
        assert_eq!(status, 200, "{}", body);
        let connection = store::open(&home.db()).expect("读库");
        let attachment = store::attachment(&connection, &event_id, &image_id)
            .unwrap()
            .unwrap();
        let rel = blob::path_for_rel(attachment.storage_rel.as_deref().unwrap()).expect("合法路径");
        std::fs::remove_file(&rel).expect("删除模拟缺失");
        let auth = format!("Bearer {token}");
        let (status, _, body) = request(
            &handle,
            "POST",
            &format!("/hooks/v1/endpoints/{endpoint}/events/{event_id}/commit"),
            &[("Authorization", auth.as_str())],
            None,
        );
        assert_eq!(status, 409, "{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "state_conflict");
        let (status, _, body) = receipt(&handle, &endpoint, &token, &event_id);
        assert_eq!(status, 200);
        assert_eq!(body_json(&body)["state"], "unavailable");
    }

    #[test]
    fn auth_and_endpoint_scoping_are_enforced() {
        let (home, _guard, handle, endpoint, token) = setup("auth");
        let payload = text_body("build:auth", "内容", true);

        let (status, _, _) = request(
            &handle,
            "POST",
            &format!("/hooks/v1/endpoints/{endpoint}/events"),
            &[("Content-Type", "application/json")],
            Some(payload.as_bytes()),
        );
        assert_eq!(status, 401, "缺少令牌必须拒绝");

        let (status, _, _) = create(&handle, &endpoint, "wrong-token", &payload);
        assert_eq!(status, 401, "错误令牌必须拒绝");

        let connection = store::open(&home.db()).expect("读库");
        let (other, other_token) =
            store::create_endpoint(&connection, "另一个接入点", &["text".to_string()])
                .expect("建接入点");
        drop(connection);
        let (status, _, _) = create(&handle, &endpoint, &other_token, &payload);
        assert_eq!(status, 404, "别的接入点令牌不能写本接入点");

        let connection = store::open(&home.db()).expect("读库");
        store::set_endpoint_enabled(&connection, &endpoint, false).expect("暂停");
        drop(connection);
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 403, "暂停后必须拒绝：{}", body);

        let connection = store::open(&home.db()).expect("读库");
        store::delete_endpoint(&connection, &endpoint).expect("删除");
        drop(connection);
        let (status, _, _) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 401, "删除后凭据立即失效");

        let _ = other;
    }

    #[test]
    fn commit_flow_moves_receiving_to_ready() {
        let (_home, _guard, handle, endpoint, token) = setup("commit");
        let payload = text_body("build:draft", "草稿", false);
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201);
        let json = body_json(&body);
        assert_eq!(json["state"], "receiving");
        let event_id = json["event_id"].as_str().expect("event_id").to_string();
        assert!(json["expires_at"].as_i64().is_some());

        let auth = format!("Bearer {token}");
        let commit_path = format!("/hooks/v1/endpoints/{endpoint}/events/{event_id}/commit");
        let (status, _, body) = request(
            &handle,
            "POST",
            &commit_path,
            &[("Authorization", auth.as_str())],
            None,
        );
        assert_eq!(status, 200, "{}", body);
        assert_eq!(body_json(&body)["state"], "ready");

        let (status, _, body) = request(
            &handle,
            "POST",
            &commit_path,
            &[("Authorization", auth.as_str())],
            None,
        );
        assert_eq!(status, 200, "重复提交应幂等：{}", body);

        let (status, _, body) = request(
            &handle,
            "POST",
            &commit_path,
            &[("Authorization", auth.as_str())],
            Some(b"{}"),
        );
        assert_eq!(status, 400, "commit 不接受请求体：{}", body);
    }

    #[test]
    fn rejects_malformed_and_oversized_requests() {
        let (_home, _guard, handle, endpoint, token) = setup_with_limits(
            "reject",
            Limits {
                rate_per_minute: 600,
                max_body_bytes: 1024,
                ..Limits::default()
            },
        );
        let path = format!("/hooks/v1/endpoints/{endpoint}/events");

        let auth = format!("Bearer {token}");
        let (status, _, _) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "application/json"),
                ("Host", "example.com"),
            ],
            Some(b"{}"),
        );
        assert_eq!(status, 400, "Host 不匹配必须拒绝");

        let (status, _, body) = request(
            &handle,
            "POST",
            &path,
            &[("Authorization", auth.as_str())],
            Some(text_body("build:no-ct", "内容", true).as_bytes()),
        );
        assert_eq!(status, 415, "{}", body);

        let mut compressed = create_headers(&token);
        compressed.push(("Content-Encoding", "gzip".to_string()));
        let (status, _, _) = request(
            &handle,
            "POST",
            &path,
            &as_header_pairs(&compressed),
            Some(b""),
        );
        assert_eq!(status, 415);

        let mut chunked = create_headers(&token);
        chunked.push(("Transfer-Encoding", "chunked".to_string()));
        let (status, _, _) = request(&handle, "POST", &path, &as_header_pairs(&chunked), None);
        assert_eq!(status, 400);

        let (status, _, _) = create(&handle, &endpoint, &token, "{not-json");
        assert_eq!(status, 400);

        let mut unknown =
            serde_json::from_str::<serde_json::Value>(&text_body("build:unknown", "内容", true))
                .expect("样本");
        unknown["paramters"] = serde_json::json!({});
        let (status, _, _) = create(&handle, &endpoint, &token, &unknown.to_string());
        assert_eq!(status, 400);

        let oversized = text_body("build:big", &"x".repeat(2000), true);
        let (status, _, _) = create(&handle, &endpoint, &token, &oversized);
        assert_eq!(status, 413);

        let (status, _, _) = request(
            &handle,
            "GET",
            &format!("/hooks/v1/endpoints/{endpoint}/events/evt_missing/receipt"),
            &[("Authorization", auth.as_str())],
            None,
        );
        assert_eq!(status, 404);

        // X-Forwarded-* 不能替代 Host 校验；正确 Host 下应走到 404 而不是 400。
        let (status, _, _) = request(
            &handle,
            "GET",
            &format!("/hooks/v1/endpoints/{endpoint}/events/evt_missing/receipt"),
            &[
                ("Authorization", auth.as_str()),
                ("X-Forwarded-Host", "evil.example.com"),
            ],
            None,
        );
        assert_eq!(status, 404, "伪造代理来源头不能影响 Host 校验");
    }

    #[test]
    fn rate_limit_returns_retry_after() {
        let (_home, _guard, handle, endpoint, token) = setup_with_limits(
            "rate",
            Limits {
                rate_per_minute: 2,
                ..Limits::default()
            },
        );
        for index in 0..2 {
            let payload = text_body(&format!("build:rate:{index}"), "内容", true);
            let (status, _, _) = create(&handle, &endpoint, &token, &payload);
            assert_eq!(status, 201);
        }
        let payload = text_body("build:rate:3", "内容", true);
        let (status, headers, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 429, "{}", body);
        let headers_lower = headers.to_ascii_lowercase();
        assert!(headers_lower.contains("retry-after:"), "{}", headers);
        assert_eq!(body_json(&body)["error"]["code"], "rate_limited");
    }

    #[test]
    fn stops_accepting_and_restart_keeps_events() {
        let (home, _guard, handle, endpoint, token) = setup("restart");
        let payload = text_body("build:persist", "重启后还在", true);
        let (status, _, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 201);
        let event_id = body_json(&body)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();
        let addr = handle.addr;
        handle.stop();
        assert!(
            TcpStream::connect(addr).is_err(),
            "停止后端口必须不再接受连接"
        );

        let connection = store::open(&home.db()).expect("重开数据库");
        assert_eq!(store::counts(&connection).expect("计数").1, 1);
        drop(connection);

        let restarted = start(
            "127.0.0.1:0".parse().expect("回环"),
            home.db(),
            Limits::default(),
        )
        .expect("重启测试服务");
        let (status, _, body) = receipt(&restarted, &endpoint, &token, &event_id);
        assert_eq!(status, 200, "重启后回执仍可读：{}", body);
        assert_eq!(body_json(&body)["state"], "ready");
        restarted.stop();
    }

    #[test]
    fn concurrent_same_external_id_creates_one_event() {
        let (home, _guard, handle, endpoint, token) = setup("concurrent");
        let addr = handle.addr;
        let payload = text_body("build:concurrent", "并发内容", true);
        let mut statuses = Vec::new();
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let payload = payload.clone();
                let endpoint = endpoint.clone();
                let token = token.clone();
                handles.push(scope.spawn(move || {
                    let headers = create_headers(&token);
                    request_to(
                        addr,
                        "POST",
                        &format!("/hooks/v1/endpoints/{endpoint}/events"),
                        &as_header_pairs(&headers),
                        Some(payload.as_bytes()),
                    )
                }));
            }
            for handle in handles {
                statuses.push(handle.join().expect("并发请求").0);
            }
        });
        statuses.sort();
        assert_eq!(statuses, vec![200, 201], "同一 ID 只能成功创建一个");
        let connection = store::open(&home.db()).expect("读库");
        assert_eq!(store::counts(&connection).expect("计数").1, 1);
        drop(handle);
    }

    #[test]
    fn cross_endpoint_token_cannot_touch_another_endpoint() {
        let (home, _guard, handle, endpoint_a, token_a) = setup("isolation");
        let connection = store::open(&home.db()).expect("建库");
        let (endpoint_b, token_b) =
            store::create_endpoint(&connection, "另一个接入点", &["text".to_string()])
                .expect("建接入点");
        drop(connection);

        let payload = text_body("build:iso", "A 的事件", true);
        let (status, _, body) = create(&handle, &endpoint_a, &token_a, &payload);
        assert_eq!(status, 201);
        let event_id = body_json(&body)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();

        let (status, _, _) = create(&handle, &endpoint_a, &token_b, &payload);
        assert_eq!(status, 404, "B 的令牌不能写 A 的接入点");
        let (status, _, _) = receipt(&handle, &endpoint_b.id, &token_a, &event_id);
        assert_eq!(status, 404, "A 的令牌不能读 B 的命名空间");
        let (status, _, _) = receipt(&handle, &endpoint_a, &token_b, &event_id);
        assert_eq!(status, 404);
    }

    #[test]
    fn higher_database_version_fails_closed() {
        let home = TempHome::new("db-version");
        let guard = scoped_xlink_home(&home.0);
        let connection = store::open(&home.db()).expect("建库");
        connection
            .pragma_update(None, "user_version", 99)
            .expect("模拟高版本");
        drop(connection);
        let handle = start(
            "127.0.0.1:0".parse().expect("回环"),
            home.db(),
            Limits::default(),
        )
        .expect("监听仍可绑定");
        let (status, _, body) = request(&handle, "GET", "/hooks/v1/endpoints/none", &[], None);
        assert_eq!(status, 503, "{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "service_unavailable");
        handle.stop();
        drop(guard);
    }

    #[test]
    fn low_disk_rejects_new_text_but_preserves_idempotent_receipts() {
        let (_home, _guard, handle, endpoint, token) = setup("text-disk");
        let payload = text_body("existing", "正文", true);
        assert_eq!(create(&handle, &endpoint, &token, &payload).0, 201);
        let root = blob::root();
        blob::set_free_space_override(root.clone(), 0);
        let retry = create(&handle, &endpoint, &token, &payload);
        let fresh = create(&handle, &endpoint, &token, &text_body("new", "正文", true));
        blob::clear_free_space_override(&root);
        assert_eq!(retry.0, 200);
        assert_eq!(fresh.0, 503);
    }

    #[test]
    fn drain_has_a_total_deadline_for_slow_senders() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let sender = thread::spawn(move || {
            for _ in 0..100 {
                if client.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        drain_incoming(&mut stream);
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(stream);
        sender.join().unwrap();
    }

    #[test]
    fn drain_stops_at_byte_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let sender = thread::spawn(move || {
            client.write_all(&vec![b'x'; DRAIN_MAX_BYTES + 1]).unwrap();
            client.shutdown(Shutdown::Write).unwrap();
        });
        drain_incoming(&mut stream);
        let mut last = [0; 1];
        assert_eq!(stream.read(&mut last).unwrap(), 1, "超出预算的字节不再排空");
        sender.join().unwrap();
    }

    #[test]
    fn saturated_queue_releases_rejected_stream_registrations() {
        let (_home, _guard, handle, _, _) = setup_with_limits(
            "queue-full",
            Limits {
                workers: 1,
                queue: 1,
                read_timeout: Duration::from_secs(10),
                ..Limits::default()
            },
        );
        // 两条不发送请求头的连接占住 worker 和队列，后续全部被拒绝。
        let first = TcpStream::connect(handle.addr).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let second = loop {
            assert!(Instant::now() < deadline, "等待 worker 取走首个任务");
            let candidate = TcpStream::connect(handle.addr).unwrap();
            candidate
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let mut byte = [0; 1];
            match candidate.peek(&mut byte) {
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if handle.streams.lock().unwrap().len() == 2 {
                        break candidate;
                    }
                }
                Ok(_) => {} // worker 尚未取走首个任务，第二条被拒后重试。
                Err(error) => panic!("排队探测失败：{error}"),
            }
            drop(candidate);
            thread::sleep(Duration::from_millis(5));
        };
        for _ in 0..20 {
            let mut client = TcpStream::connect(handle.addr).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            client
                .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ntest")
                .unwrap();
            client.shutdown(Shutdown::Write).unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 503"));
            assert!(response.contains("service_unavailable"));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while handle.streams.lock().unwrap().len() > 2 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(handle.streams.lock().unwrap().len(), 2);
        drop((first, second));
        let streams = Arc::clone(&handle.streams);
        handle.stop();
        assert!(streams.lock().unwrap().is_empty());
    }

    #[test]
    fn notification_payload_accepts_official_root_and_legacy_wrapper() {
        let root = notification_body(Some("job/build-app/"), 128, "FINALIZED", "SUCCESS");
        for value in [root.clone(), format!(r#"{{"job":{root}}}"#)] {
            let payload: NotificationPayload = serde_json::from_str(&value).unwrap();
            let job = payload.into_job();
            assert_eq!(job.name.as_deref(), Some("build-app"));
            assert_eq!(job.build.unwrap().number, Some(128));
        }
    }

    fn notification_basic(password: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("jenkins:{password}"))
        )
    }

    fn notification_path(endpoint_id: &str) -> String {
        format!("/hooks/v1/endpoints/{endpoint_id}/jenkins-notification")
    }

    fn notification_body(job_url: Option<&str>, number: i64, phase: &str, status: &str) -> String {
        let mut job = serde_json::json!({
            "name": "build-app",
            "display_name": "Build App",
            "build": {
                "number": number,
                "phase": phase,
                "status": status,
                "timestamp": 1_763_172_000_000i64,
                "duration": 42_000,
                "url": "job/build-app/128/",
                "full_url": "https://jenkins.example.com/job/build-app/128/",
                "display_name": "#128",
                "parameters": {"BRANCH": "main"},
                "notes": "由 Jenkins 通知插件报告",
                "scm": {"branch": "main", "commit": "abc123"},
                "test_summary": {"total": 12, "failed": 1, "passed": 11}
            }
        });
        if let Some(url) = job_url {
            job["url"] = serde_json::json!(url);
        }
        job.to_string()
    }

    #[test]
    fn jenkins_notification_is_idempotent_and_keeps_reported_result() {
        let (home, _guard, handle, endpoint_id, _bearer) = setup("jenkins");
        let connection = store::open(&home.db()).expect("建库");
        let (_, password) =
            store::rotate_notification_token(&connection, &endpoint_id).expect("通知凭据");
        drop(connection);
        let path = notification_path(&endpoint_id);
        let auth = notification_basic(&password);
        let phase = "FINALIZED";
        let body = notification_body(Some("job/folder/job/build-app/"), 128, phase, "SUCCESS");

        let (status, _, first) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(status, 201, "{first}");
        let first: serde_json::Value = body_json(&first);
        let event_id = first["event_id"].as_str().expect("event_id").to_string();
        assert_eq!(first["state"], "ready");

        let (status, _, again) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(status, 200, "同一 FINALIZED 通知重试返回原事件：{again}");
        assert_eq!(body_json(&again)["event_id"], event_id.as_str());

        // 文件夹下同名 Job 靠 job.url 区分，不能共用同一条幂等键。
        let sibling = notification_body(
            Some("job/another-folder/job/build-app/"),
            128,
            phase,
            "SUCCESS",
        );
        let (status, _, sibling) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(sibling.as_bytes()),
        );
        assert_eq!(status, 201, "同名不同文件夹必须是另一条事件：{sibling}");
        assert_ne!(body_json(&sibling)["event_id"], event_id.as_str());

        // 同一构建的 reported_result 变化不能静默覆盖，交 409 让发送方判断。
        let changed = notification_body(Some("job/folder/job/build-app/"), 128, phase, "FAILURE");
        let (status, _, _) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(changed.as_bytes()),
        );
        assert_eq!(status, 409);

        let connection = store::open(&home.db()).expect("建库");
        let row = store::event_detail(&connection, &event_id)
            .expect("查询")
            .expect("事件存在");
        assert_eq!(row.event_type, "jenkins.notification");
        assert!(row.title.contains("#128 SUCCESS"), "{}", row.title);
        assert_eq!(row.parameters["source"], "jenkins-notification");
        assert_eq!(row.parameters["reported_result"], "SUCCESS");
        assert_eq!(row.parameters["identity_resolved"], true);
        assert_eq!(row.parameters["job_url"], "job/folder/job/build-app/");
        assert!(
            row.parameters.get("verified_result").is_none(),
            "reported_result 不能被写成已核验结果"
        );
        assert!(row.parameters.get("notes").is_some());
        assert!(row.parameters.get("build_parameters").is_some());
    }

    #[test]
    fn jenkins_notification_requires_its_own_basic_credential_and_finalized_phase() {
        let (home, _guard, handle, endpoint_id, bearer) = setup("jenkins-auth");
        let connection = store::open(&home.db()).expect("建库");
        let (_, password) =
            store::rotate_notification_token(&connection, &endpoint_id).expect("通知凭据");
        let (other, _) = store::create_endpoint(&connection, "另一个", &["parameters".to_string()])
            .expect("另一个接入点");
        drop(connection);
        let path = notification_path(&endpoint_id);
        let auth = notification_basic(&password);
        let body = notification_body(Some("job/build-app/"), 128, "FINALIZED", "SUCCESS");

        // Bearer 令牌不能进通知路由，通知密码也不能当 Bearer 用。
        let (status, _, _) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &format!("Bearer {bearer}")),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(status, 401);
        let (status, _, _) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &notification_basic("wrong")),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(status, 401);

        // 别的接入点的通知密码不能写这里（仍按 404 处理，不泄露存在性）。
        let (status, _, _) = request(
            &handle,
            "POST",
            &notification_path(&other.id),
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(status, 404);

        // 只接 FINALIZED：STARTED/COMPLETED 明确拒绝，而不是当作另一条事件。
        let started = notification_body(Some("job/build-app/"), 128, "STARTED", "SUCCESS");
        let (status, _, rejected) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(started.as_bytes()),
        );
        assert_eq!(status, 400, "{rejected}");
        assert_eq!(body_json(&rejected)["error"]["code"], "unsupported_phase");

        // 没有 job.url 时保留事件但明确标记身份未解析，不猜文件夹路径。
        let unresolved = notification_body(None, 129, "FINALIZED", "SUCCESS");
        let (status, _, created) = request(
            &handle,
            "POST",
            &path,
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
            ],
            Some(unresolved.as_bytes()),
        );
        assert_eq!(status, 201, "{created}");
        let event_id = body_json(&created)["event_id"]
            .as_str()
            .expect("event_id")
            .to_string();
        let connection = store::open(&home.db()).expect("建库");
        let row = store::event_detail(&connection, &event_id)
            .expect("查询")
            .expect("事件存在");
        assert_eq!(row.parameters["identity_resolved"], false);
        assert_eq!(row.parameters["job_name"], "build-app");
    }
}
