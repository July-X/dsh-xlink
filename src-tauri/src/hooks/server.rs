//! Hook 接收器：回环 TcpListener、固定工作线程、鉴权、限流与三条 API 路由。
//!
//! P1 只绑回环、只收纯文本/参数。传输层复用 `openai::http` 的读请求原语，
//! 这里只做路由与业务编排；数据库访问全部经 [`crate::hooks::store`]。
//!
//! 停止语义：置取消旗标 → 自连一次唤醒 accept → 关闭跟踪中的连接 → join 线程。
//! 与内置桥接同形（`plugins/builtin/bridge.rs` 的 `Drop`），保证退出时不留下
//! 仍在监听的线程或半截请求。

use std::collections::HashMap;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;

use crate::hooks::protocol::{self, CreateEventRequest, RequestError};
use crate::hooks::store::{self, EndpointAuth, StoreError};
use crate::openai::http;

const RATE_WINDOW: Duration = Duration::from_secs(60);

/// 可调参数。测试用较小值覆盖限流/请求体上限，生产用默认值。
#[derive(Debug, Clone)]
pub(crate) struct Limits {
    pub max_body_bytes: usize,
    pub rate_per_minute: u32,
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
            workers: 4,
            queue: 16,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
        }
    }
}

struct Job {
    id: u64,
    stream: TcpStream,
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
}

pub(crate) struct ServerHandle {
    pub(crate) addr: SocketAddr,
    cancel: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    workers: Vec<JoinHandle<()>>,
    streams: Arc<Mutex<HashMap<u64, TcpStream>>>,
}

impl ServerHandle {
    /// 停止接收并回收线程。连接会被 shutdown，堵在读写上的工作线程立刻返回。
    pub(crate) fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        if let Some(accept) = self.accept.take() {
            self.cancel.store(true, Ordering::SeqCst);
            // 唤醒阻塞在 accept 上的线程；连接本身马上被丢弃。
            let _ = TcpStream::connect(self.addr);
            let _ = accept.join();
        }
        if let Ok(streams) = self.streams.lock() {
            for stream in streams.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

pub(crate) fn start(
    addr: SocketAddr,
    db_path: PathBuf,
    limits: Limits,
) -> Result<ServerHandle, String> {
    let listener = TcpListener::bind(addr).map_err(|error| {
        format!(
            "Hook 服务无法监听 {addr}（端口可能被占用或没有权限）：{error}。\
             请换一个端口后重试；服务保持停止状态"
        )
    })?;
    let bound = listener
        .local_addr()
        .map_err(|error| format!("无法读取 Hook 监听地址：{error}"))?;
    let limits = Limits {
        workers: limits.workers.clamp(1, 16),
        queue: limits.queue.max(1),
        ..limits
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let streams = Arc::new(Mutex::new(HashMap::new()));
    let (tx, rx) = mpsc::sync_channel::<Job>(limits.queue);
    let rx = Arc::new(Mutex::new(rx));
    let listen_port = bound.port();
    let state = Arc::new(ServerState {
        db_path,
        limits: limits.clone(),
        listen_port,
        rate: Mutex::new(HashMap::new()),
    });

    let mut workers = Vec::with_capacity(limits.workers);
    for index in 0..limits.workers {
        let worker_rx = Arc::clone(&rx);
        let worker_state = Arc::clone(&state);
        let worker_cancel = Arc::clone(&cancel);
        let worker_streams = Arc::clone(&streams);
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
                if let Ok(mut map) = worker_streams.lock() {
                    map.remove(&job.id);
                }
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

    let accept_cancel = Arc::clone(&cancel);
    let accept_streams = Arc::clone(&streams);
    let next_id = Arc::new(AtomicU64::new(1));
    let accept = thread::Builder::new()
        .name("dsh-hooks-accept".into())
        .spawn(move || {
            for incoming in listener.incoming() {
                if accept_cancel.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = incoming else { continue };
                let id = next_id.fetch_add(1, Ordering::SeqCst);
                if let Ok(clone) = stream.try_clone() {
                    if let Ok(mut map) = accept_streams.lock() {
                        map.insert(id, clone);
                    }
                }
                match tx.try_send(Job { id, stream }) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(mut job)) => {
                        let request_id = request_id();
                        let body = serde_json::json!({
                            "error": {
                                "code": "service_unavailable",
                                "message": "接收队列已满，请稍后重试",
                                "request_id": request_id,
                            }
                        })
                        .to_string();
                        let _ =
                            http::write_response(&mut job.stream, 503, "application/json", &body);
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
        });
    let accept = match accept {
        Ok(handle) => handle,
        Err(error) => {
            // spawn 失败时闭包（连同 tx）已经被丢弃，rx 会断开，工作线程自行退出。
            cancel.store(true, Ordering::SeqCst);
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
            return Err(format!("无法启动 Hook 接收线程：{error}"));
        }
    };

    Ok(ServerHandle {
        addr: bound,
        cancel,
        accept: Some(accept),
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
    Commit {
        endpoint_id: String,
        event_id: String,
    },
    Receipt {
        endpoint_id: String,
        event_id: String,
    },
}

impl Route {
    fn endpoint_id(&self) -> &str {
        match self {
            Route::Create { endpoint_id }
            | Route::Commit { endpoint_id, .. }
            | Route::Receipt { endpoint_id, .. } => endpoint_id,
        }
    }
}

fn parse_route(method: &str, path: &str) -> Option<Route> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.len() < 5
        || segments[0] != "hooks"
        || segments[1] != "v1"
        || segments[2] != "endpoints"
        || segments[4] != "events"
    {
        return None;
    }
    let endpoint_id = segments[3].to_string();
    if endpoint_id.is_empty() {
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

    let token = bearer_token(&head).ok_or_else(Failure::unauthorized)?;
    let endpoint = store::auth_by_token_hash(connection, &store::hash_token(token))
        .map_err(|error| store_failure(error, None, None, "event_conflict"))?
        .ok_or_else(Failure::unauthorized)?;
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
        Route::Commit { event_id, .. } => commit_event(connection, &endpoint, &event_id, &head),
        Route::Receipt { event_id, .. } => receipt(connection, &endpoint, &event_id),
    }
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
    };
    let (row, created) = store::create_event(connection, &endpoint.id, &new_event, now)
        .map_err(|error| store_failure(error, Some(&endpoint.id), None, "event_conflict"))?;
    let response = protocol::EventStatusResponse {
        event_id: row.id.clone(),
        state: row.state,
        attachments: Vec::new(),
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
    let row = store::commit_event(connection, &endpoint.id, event_id, now).map_err(|error| {
        store_failure(error, Some(&endpoint.id), Some(event_id), "state_conflict")
    })?;
    let response = protocol::EventStatusResponse {
        event_id: row.id.clone(),
        state: row.state,
        attachments: Vec::new(),
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
    let response = protocol::ReceiptResponse {
        event_id: row.id,
        state: row.state,
        attachments: Vec::new(),
        occurred_at: row.occurred_at,
        received_at: row.received_at,
        expires_at: row.expires_at,
    };
    Ok(Response::json(200, &response))
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

fn validate_host(head: &http::RequestHead, state: &ServerState) -> Result<(), Failure> {
    let host = head
        .headers
        .get("host")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let ipv4 = format!("127.0.0.1:{}", state.listen_port);
    let name = format!("localhost:{}", state.listen_port);
    if host == ipv4 || host == name {
        Ok(())
    } else {
        Err(Failure::invalid("Host 头与监听地址不匹配"))
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
    use super::*;
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

    fn setup_with_limits(label: &str, limits: Limits) -> (TempHome, ServerHandle, String, String) {
        let home = TempHome::new(label);
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
        (home, handle, endpoint.id, token)
    }

    fn setup(label: &str) -> (TempHome, ServerHandle, String, String) {
        setup_with_limits(
            label,
            Limits {
                rate_per_minute: 600,
                ..Limits::default()
            },
        )
    }

    fn event_body(external_id: &str, text: &str, commit: bool) -> String {
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

    /// 发一个原始 HTTP 请求，返回 (status, headers, body)。Host 默认按实际端口填。
    fn request(
        handle: &ServerHandle,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> (u16, String, String) {
        request_to(handle.addr, method, path, headers, body)
    }

    fn request_to(
        addr: SocketAddr,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
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
            stream.write_all(body.as_bytes()).expect("写请求体");
        }
        let _ = stream.shutdown(Shutdown::Write);
        // 读超时是 per-read 的：响应可能分多段到达，不能因为一次 WouldBlock
        // 就把已读到的头当成完整响应（曾有并行跑测试时 body 为空的一次偶发）。
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
            Some(body),
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

    fn body_json(body: &str) -> serde_json::Value {
        serde_json::from_str(body).expect("响应是 JSON")
    }

    #[test]
    fn text_event_round_trip_is_idempotent_and_conflicts() {
        let (home, handle, endpoint, token) = setup("roundtrip");
        let payload = event_body("build:Project_Plague:128", "构建完成", true);
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

        let changed = event_body("build:Project_Plague:128", "构建完成（内容已变）", true);
        let (status, _, body) = create(&handle, &endpoint, &token, &changed);
        assert_eq!(status, 409, "同 ID 不同内容必须冲突：{}", body);
        assert_eq!(body_json(&body)["error"]["code"], "event_conflict");
        assert!(body_json(&body)["error"]["request_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("req_")));

        // 数据库里只有一条事件。
        let connection = store::open(&home.db()).expect("读库");
        assert_eq!(store::counts(&connection).expect("计数").1, 1);
    }

    #[test]
    fn auth_and_endpoint_scoping_are_enforced() {
        let (home, handle, endpoint, token) = setup("auth");
        let payload = event_body("build:auth", "内容", true);

        let (status, _, _) = request(
            &handle,
            "POST",
            &format!("/hooks/v1/endpoints/{endpoint}/events"),
            &[("Content-Type", "application/json")],
            Some(&payload),
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
        let (status, _, _) = create(&handle, &other.id, &token, &payload);
        assert_eq!(status, 404, "本接入点令牌不能写别的接入点");

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
    }

    #[test]
    fn commit_flow_moves_receiving_to_ready() {
        let (_home, handle, endpoint, token) = setup("commit");
        let payload = event_body("build:draft", "草稿", false);
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
            Some("{}"),
        );
        assert_eq!(status, 400, "commit 不接受请求体：{}", body);
    }

    #[test]
    fn rejects_malformed_and_oversized_requests() {
        let (_home, handle, endpoint, token) = setup_with_limits(
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
            Some("{}"),
        );
        assert_eq!(status, 400, "Host 不匹配必须拒绝");

        let (status, _, body) = request(
            &handle,
            "POST",
            &path,
            &[("Authorization", auth.as_str())],
            Some(&event_body("build:no-ct", "内容", true)),
        );
        assert_eq!(status, 415, "{}", body);

        let mut compressed = create_headers(&token);
        compressed.push(("Content-Encoding", "gzip".to_string()));
        let (status, _, _) = request(
            &handle,
            "POST",
            &path,
            &as_header_pairs(&compressed),
            Some(""),
        );
        assert_eq!(status, 415);

        let mut chunked = create_headers(&token);
        chunked.push(("Transfer-Encoding", "chunked".to_string()));
        let (status, _, _) = request(&handle, "POST", &path, &as_header_pairs(&chunked), None);
        assert_eq!(status, 400);

        let (status, _, _) = create(&handle, &endpoint, &token, "{not-json");
        assert_eq!(status, 400);

        let mut unknown =
            serde_json::from_str::<serde_json::Value>(&event_body("build:unknown", "内容", true))
                .expect("样本");
        unknown["paramters"] = serde_json::json!({});
        let (status, _, _) = create(&handle, &endpoint, &token, &unknown.to_string());
        assert_eq!(status, 400);

        let mut attachments =
            serde_json::from_str::<serde_json::Value>(&event_body("build:att", "内容", true))
                .expect("样本");
        attachments["attachments"] = serde_json::json!([{"client_id": "log"}]);
        let (status, _, _) = create(&handle, &endpoint, &token, &attachments.to_string());
        assert_eq!(status, 415);

        let oversized = event_body("build:big", &"x".repeat(2000), true);
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
    }

    #[test]
    fn rate_limit_returns_retry_after() {
        let (_home, handle, endpoint, token) = setup_with_limits(
            "rate",
            Limits {
                rate_per_minute: 2,
                ..Limits::default()
            },
        );
        for index in 0..2 {
            let payload = event_body(&format!("build:rate:{index}"), "内容", true);
            let (status, _, _) = create(&handle, &endpoint, &token, &payload);
            assert_eq!(status, 201);
        }
        let payload = event_body("build:rate:3", "内容", true);
        let (status, headers, body) = create(&handle, &endpoint, &token, &payload);
        assert_eq!(status, 429, "{}", body);
        let headers_lower = headers.to_ascii_lowercase();
        assert!(headers_lower.contains("retry-after:"), "{}", headers);
        assert_eq!(body_json(&body)["error"]["code"], "rate_limited");
    }

    #[test]
    fn stops_accepting_and_restart_keeps_events() {
        let (home, handle, endpoint, token) = setup("restart");
        let payload = event_body("build:persist", "重启后还在", true);
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
        let (home, handle, endpoint, token) = setup("concurrent");
        let addr = handle.addr;
        let payload = event_body("build:concurrent", "并发内容", true);
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
                        Some(&payload),
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
        let (home, handle, endpoint_a, token_a) = setup("isolation");
        let connection = store::open(&home.db()).expect("建库");
        let (endpoint_b, token_b) =
            store::create_endpoint(&connection, "另一个接入点", &["text".to_string()])
                .expect("建接入点");
        drop(connection);

        let payload = event_body("build:iso", "A 的事件", true);
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
    }
}
