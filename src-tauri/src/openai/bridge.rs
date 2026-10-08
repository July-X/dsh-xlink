//! 本地桥接服务：面向本次 dsh 子进程的受认证 HTTP 端点（开发计划 §5）。
//!
//! 形状与 `plugins/openai-oauth/host/bridge.js` 的客户端契约一一对应：
//! `GET /v1/handshake`（协议核对）、`GET /v1/models`（目录）。鉴权是
//! `Authorization: Bearer <token>`，令牌按「(族, 实例)」一代一换——同一
//! 实例再次启动会**先停旧监听再起新代**，旧令牌随之作废（设计 §9「工作台
//! 停止后撤销本次桥接令牌」在重启语义下的实现；无重启的停机期监听仍在，
//! 但它只绑回环、除桩目录外不暴露任何数据）。
//!
//! 实现刻意用 `std::net` + `httparse` 手写：端点三个、客户端只有内核
//! Host 插件一个，引一整套 async 服务框架不成比例；`httparse` 已在依赖
//! 树里（tauri 传递依赖），`rand` 同理——本模块没有引入任何新下载。
//!
//! **这不是针对恶意宿主插件的沙箱**（开发计划 §5 明示：dsh 插件拥有宿主
//! 进程权限）。令牌防的是**其它本地进程**顺手读目录；跨源浏览器请求由
//! 端口随机性 + 令牌共同挡住，P2 后续补 Host/Origin 显式校验。

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rand::Rng;

use crate::plugins::builtin;

/// 桥接协议版本（与 host/constants.js 的 BRIDGE_PROTOCOL_VERSION 对应）。
const PROTOCOL_VERSION: u32 = 1;

/// 一个运行中的桥接代次。
struct Bridge {
    addr: std::net::SocketAddr,
    token: String,
    shutdown: Arc<AtomicBool>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // 唤醒阻塞在 accept 上的线程：连一次自己。失败只能忽略——listener
        // 即将被回收，线程退出最多晚到进程退出。
        let _ = std::net::TcpStream::connect(self.addr);
    }
}

fn bridges() -> &'static Mutex<HashMap<String, Arc<Bridge>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Bridge>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn serve(stream: &mut TcpStream, token: &str, models: &ModelsSource, inference: &InferenceSource) {
    let Ok(request) = crate::openai::http::read_request(stream) else {
        return;
    };
    let authorized = request
        .headers
        .get("authorization")
        .is_some_and(|value| value == &format!("Bearer {token}"));
    if !authorized {
        let _ = crate::openai::http::write_response(
            stream,
            401,
            "application/json",
            "{\"error\":\"unauthorized\"}",
        );
        return;
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/handshake") => {
            let _ = crate::openai::http::write_response(
                stream,
                200,
                "application/json",
                &format!("{{\"protocol\":{PROTOCOL_VERSION},\"service\":\"xlink-openai-oauth\"}}"),
            );
        }
        ("GET", "/v1/models") => match models() {
            Ok(payload) => {
                let _ =
                    crate::openai::http::write_response(stream, 200, "application/json", &payload);
            }
            Err((status, reason)) => {
                let _ = crate::openai::http::write_response(
                    stream,
                    status,
                    "application/json",
                    &format!("{{\"error\":\"{reason}\"}}"),
                );
            }
        },
        ("POST", "/v1/responses") => {
            // 信封在请求体里；read_request 只消费到头结束，体经
            // `body_prefix` + 续读取得（http.rs 的坑修在结构里）。
            let length: usize = request
                .headers
                .get("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut envelope_bytes = request.body_prefix.clone();
            while envelope_bytes.len() < length {
                use std::io::Read;
                let mut chunk = [0u8; 2048];
                let Ok(n) = stream.read(&mut chunk) else {
                    break;
                };
                if n == 0 {
                    break;
                }
                envelope_bytes.extend_from_slice(&chunk[..n]);
            }
            envelope_bytes.truncate(length);
            let envelope: serde_json::Value = match serde_json::from_slice(&envelope_bytes) {
                Ok(value) => value,
                Err(error) => {
                    let _ = crate::openai::http::write_response(
                        stream,
                        400,
                        "application/json",
                        &format!("{{\"error\":\"请求信封不是有效 JSON：{error}\"}}"),
                    );
                    return;
                }
            };
            match inference(&envelope) {
                Err((status, reason)) => {
                    let _ = crate::openai::http::write_response(
                        stream,
                        status,
                        "application/json",
                        &format!("{{\"error\":\"{reason}\"}}"),
                    );
                }
                Ok(reader) => pump_response_stream(stream, reader),
            }
        }
        _ => {
            let _ = crate::openai::http::write_response(
                stream,
                404,
                "application/json",
                "{\"error\":\"not-found\"}",
            );
        }
    }
}

/// 把上游 SSE 逐事件转成 NDJSON 下发，末尾补一枚**桥接终止包络**：
/// `{type:"bridge.terminal", status, replay?}`——成功终止 / 失败终止 /
/// 未完成（EOF 而无终止事件，设计 §7：这算失败，不算成功）。
fn pump_response_stream(stream: &mut TcpStream, reader: Box<dyn std::io::BufRead + Send>) {
    use std::io::Write;
    if crate::openai::http::write_stream_start(stream, "application/x-ndjson").is_err() {
        return;
    }
    let mut lines = crate::openai::inference::UpstreamLines { reader };
    let mut terminal = crate::openai::inference::TerminalEnvelope::incomplete();
    loop {
        match lines.next_event() {
            Ok(Some(event)) => {
                let line = event.data.to_string();
                let _ = stream.write_all(line.as_bytes());
                let _ = stream.write_all(b"\n");
                let _ = stream.flush();
                if let Some(classified) = crate::openai::inference::classify_terminal(&event.data) {
                    terminal = crate::openai::inference::TerminalEnvelope::from(classified);
                    break;
                }
            }
            Ok(None) => break, // EOF：terminal 保持 incomplete
            Err(error) => {
                terminal = crate::openai::inference::TerminalEnvelope::failed(error);
                break;
            }
        }
    }
    let _ = stream.write_all(terminal.to_line().as_bytes());
    let _ = stream.write_all(b"\n");
    let _ = stream.flush();
}

/// `/v1/models` 的数据源：返回 Host 契约载荷，或（HTTP 状态码, 原因）。
/// 生产实现连真实目录（`catalog::serve_payload`）；测试注入桩。
pub(crate) type ModelsSource = Arc<dyn Fn() -> Result<String, (u16, String)> + Send + Sync>;

/// `/v1/responses` 的推理源：收 Host 的请求信封，产出**已通过校验**的
/// 上流行读取器（生产实现做刷新令牌 → 目录校验 → 白名单 → 上游调用）。
pub(crate) type InferenceSource = Arc<
    dyn Fn(&serde_json::Value) -> Result<Box<dyn std::io::BufRead + Send>, (u16, String)>
        + Send
        + Sync,
>;

/// 起一个新桥接（绑定 `127.0.0.1:0`）。失败信息带监听地址语义，可直接给日志。
fn spawn_bridge(
    key: &str,
    models: ModelsSource,
    inference: InferenceSource,
) -> Result<Arc<Bridge>, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("桥接服务监听 127.0.0.1 失败：{error}"))?;
    let addr = listener
        .local_addr()
        .map_err(|error| format!("取桥接监听地址失败：{error}"))?;
    let token = new_token();
    let shutdown = Arc::new(AtomicBool::new(false));
    let thread_flag = Arc::clone(&shutdown);
    let thread_token = token.clone();
    let thread_models = Arc::clone(&models);
    let thread_inference = Arc::clone(&inference);
    std::thread::Builder::new()
        .name(format!("oop-bridge-{key}"))
        .spawn(move || {
            for stream in listener.incoming() {
                if thread_flag.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .ok();
                serve(
                    &mut stream,
                    &thread_token,
                    &thread_models,
                    &thread_inference,
                );
            }
        })
        .map_err(|error| format!("启动桥接线程失败：{error}"))?;
    Ok(Arc::new(Bridge {
        addr,
        token,
        shutdown,
    }))
}

/// 确保该实例有一个**当代**桥接：已有即复用；没有（或刚被停）就起新代。
fn ensure_bridge(family: &str, instance_id: &str) -> Result<(String, String), String> {
    let key = format!("{family}/{instance_id}");
    let mut registry = bridges().lock().unwrap_or_else(|p| p.into_inner());
    // Drop 旧的（触发停机唤醒）再插入新的：同一实例重启内核 = 旧令牌作废。
    if let Some(old) = registry.insert(
        key.clone(),
        spawn_bridge(
            &key,
            production_models_source(),
            production_inference_source(),
        )?,
    ) {
        drop(old);
    }
    let current = registry
        .get(&key)
        .ok_or_else(|| "桥接注册表刚被写入却读不到（不应发生）".to_string())?;
    Ok((format!("http://{}/", current.addr), current.token.clone()))
}

/// 生产目录源：壳内路径 + 生产传输；错误分类给 Host（401=重登 / 503=不可用）。
fn production_models_source() -> ModelsSource {
    Arc::new(|| {
        let paths = crate::openai::flow::shell_flow_paths();
        let transport = crate::openai::flow::ProductionTransport { open_browser: None };
        let mode = crate::shell::settings::current_mode().as_str().to_string();
        crate::openai::catalog::serve_payload(&paths, &transport, &mode)
    })
}

/// 生产推理源：刷新令牌 → 目录校验（revision/白名单/强度）→ 上游流。
/// 错误分类给 Host：401 需重登；409 目录过期（刷新后重试）；400 请求被
/// 拒（无法执行的配置 / 非法强度）；503 目录或网络不可用。
fn production_inference_source() -> InferenceSource {
    Arc::new(|envelope: &serde_json::Value| {
        let paths = crate::openai::flow::shell_flow_paths();
        let transport = crate::openai::flow::ProductionTransport { open_browser: None };
        let mode = crate::shell::settings::current_mode().as_str().to_string();
        let tokens = crate::openai::refresh::ensure_fresh_access(&paths, &transport, &mode)
            .map_err(|error| (401u16, error.message()))?
            .ok_or((401u16, "尚未登录；请先在工作台登录".to_string()))?;
        let catalog = crate::openai::catalog::load_catalog(&paths, &transport, &mode)
            .map_err(|error| (503u16, error.message()))?;
        let validated =
            crate::openai::inference::validate_request(envelope, &catalog).map_err(|error| {
                match error {
                    crate::openai::inference::RequestError::StaleRevision { .. } => {
                        (409u16, error.message())
                    }
                    _ => (400u16, error.message()),
                }
            })?;
        let url = format!(
            "{}{}",
            paths.issuer_base,
            crate::openai::inference::RESPONSES_PATH
        );
        crate::openai::inference::open_upstream(&url, &tokens.access_token, &validated.payload)
            .map(|lines| lines.reader)
            .map_err(|failure| (502u16, failure.message(&[])))
    })
}

/// 内核启动时注入的桥接环境变量（开发计划 §5：只经子进程环境传入）。
///
/// 实例 patch 里没有本插件接线行时返回空 vec——**未启用就连环境都不给**，
/// Host 插件按「桥接未随本次启动提供」处理（bridgeProvided=false）。
pub(crate) fn launch_env(
    dsh_home: &Path,
    profile: &str,
    family: &str,
    instance_id: &str,
) -> Vec<(String, String)> {
    let patch = dsh_home
        .join("profiles")
        .join(profile)
        .join("cordis.patch.yml");
    let wired = std::fs::read_to_string(&patch)
        .map(|text| text.contains(builtin::wiring::ROW_ID))
        .unwrap_or(false);
    if !wired {
        return Vec::new();
    }
    match ensure_bridge(family, instance_id) {
        Ok((url, token)) => vec![
            (builtin::BRIDGE_URL_ENV.to_string(), url),
            (builtin::BRIDGE_TOKEN_ENV.to_string(), token),
        ],
        Err(error) => {
            // 桥接起不来**不阻断内核启动**（工作台还有本地模型可用），
            // 但要留痕：shell_events 按日轮转落进「查看日志」面板。
            let line = format!("OpenAI 桥接启动失败（不阻断工作台）：{error}");
            crate::shell::shell_events::record("openai-bridge", &line);
            eprintln!("{line}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// 推理源桩：返回一个预置 SSE 流的读取器（Cursor 即可，走真实泵送）。
    fn stub_inference(sse_body: &'static str) -> InferenceSource {
        Arc::new(move |_envelope| {
            Ok(Box::new(std::io::Cursor::new(sse_body.as_bytes().to_vec()))
                as Box<dyn std::io::BufRead + Send>)
        })
    }

    /// 带体的 POST（推理信封）：返回（状态, 响应体全文）。
    fn post(addr: &std::net::SocketAddr, path: &str, token: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(
                format!(
                    "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        let status: u16 = text.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = text
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string();
        (status, body)
    }

    fn get(addr: &std::net::SocketAddr, path: &str, token: Option<&str>) -> (u16, String) {
        let mut stream = TcpStream::connect(addr).unwrap();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n"
        )
        .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        let status: u16 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = buf.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
        (status, body)
    }

    fn test_bridge(tag: &str) -> (std::net::SocketAddr, String, Arc<Bridge>) {
        test_bridge_with(tag, stub_inference(""))
    }

    fn test_bridge_with(
        tag: &str,
        inference: InferenceSource,
    ) -> (std::net::SocketAddr, String, Arc<Bridge>) {
        let bridge = spawn_bridge(
            &format!("test-{tag}"),
            Arc::new(|| Ok("{\"revision\":\"stub-0\",\"models\":[]}".to_string())),
            inference,
        )
        .unwrap();
        let addr = bridge.addr;
        let token = bridge.token.clone();
        // 返回句柄让调用方保活：Bridge 的 Drop 即停机（注册表换代语义），
        // 测试期间必须持住，否则连过去就是 ConnectionRefused。
        (addr, token, bridge)
    }

    #[test]
    fn handshake_models_and_auth() {
        let (addr, token, _keep) = test_bridge("auth");
        let (status, body) = get(&addr, "/v1/handshake", Some(&token));
        assert_eq!(status, 200);
        assert!(body.contains("\"protocol\":1"));
        let (status, body) = get(&addr, "/v1/models", Some(&token));
        assert_eq!(status, 200);
        assert!(body.contains("\"revision\":\"stub-0\""));
        // 错令牌 / 无令牌 → 401；未知路径 → 404。
        assert_eq!(get(&addr, "/v1/handshake", Some("wrong")).0, 401);
        assert_eq!(get(&addr, "/v1/handshake", None).0, 401);
        assert_eq!(get(&addr, "/v1/nope", Some(&token)).0, 404);
    }

    #[test]
    fn launch_env_follows_wiring_row() {
        let home = std::env::temp_dir().join(format!("oop-bridge-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let profile_dir = home.join("profiles/web");
        std::fs::create_dir_all(&profile_dir).unwrap();
        // 未接线 → 空 env。
        assert!(launch_env(&home, "web", "dsh", "x1").is_empty());
        // 接线行在 → 两对 env，URL 可达且令牌有效。
        std::fs::write(
            profile_dir.join("cordis.patch.yml"),
            "- insert:\n    - id: xlink-openai-oauth\n      name: '../../x'\n",
        )
        .unwrap();
        let env = launch_env(&home, "web", "dsh", "x2");
        assert_eq!(env.len(), 2);
        let url = env
            .iter()
            .find(|(k, _)| k.ends_with("URL"))
            .unwrap()
            .1
            .clone();
        let token = env
            .iter()
            .find(|(k, _)| k.ends_with("TOKEN"))
            .unwrap()
            .1
            .clone();
        let addr: std::net::SocketAddr = url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse()
            .unwrap();
        assert_eq!(get(&addr, "/v1/handshake", Some(&token)).0, 200);
        // 同一实例再次启动 → 新代令牌，旧代作废。
        let env2 = launch_env(&home, "web", "dsh", "x2");
        let token2 = env2
            .iter()
            .find(|(k, _)| k.ends_with("TOKEN"))
            .unwrap()
            .1
            .clone();
        assert_ne!(token, token2);
        let url2 = env2
            .iter()
            .find(|(k, _)| k.ends_with("URL"))
            .unwrap()
            .1
            .clone();
        let addr2: std::net::SocketAddr = url2
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse()
            .unwrap();
        assert_eq!(get(&addr2, "/v1/handshake", Some(&token2)).0, 200);
        let _ = std::fs::remove_dir_all(&home);
    }
    /// 推理流：上游 SSE 逐事件转 NDJSON，成功终止补 bridge.terminal
    /// （completed + 回放材料），下游拿到的就是**有界事件**而不是整段缓存。
    #[test]
    fn responses_stream_pumps_events_and_terminal() {
        let sse: &'static str = "event: a\ndata: {\"type\":\"response.output_text.delta\"}\n\nevent: b\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"output\":[]}}\n";
        let (addr, token, _keep) = test_bridge_with("infer-ok", stub_inference(sse));
        let envelope = r#"{"model":"gpt-x","catalogRevision":"r","payload":{}}"#;
        let (status, body) = post(&addr, "/v1/responses", &token, envelope);
        assert_eq!(status, 200);
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 3, "{body}");
        assert!(lines[0].contains("output_text.delta"));
        let terminal: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(terminal["type"], "bridge.terminal");
        assert_eq!(terminal["status"], "completed");
        assert!(terminal["replay"]["response"]["id"] == "r1");
    }

    /// 上游 EOF 而无终止事件 → bridge.terminal 标记 incomplete（设计 §7：
    /// 这算失败，不算成功——已有文本不丢，但成功不会被 EOF 伪造）。
    #[test]
    fn responses_stream_without_terminal_marks_incomplete() {
        let sse: &'static str = "data: {\"type\":\"response.output_text.delta\"}\n";
        let (addr, token, _keep) = test_bridge_with("infer-eof", stub_inference(sse));
        let envelope = r#"{"model":"gpt-x","catalogRevision":"r","payload":{}}"#;
        let (status, body) = post(&addr, "/v1/responses", &token, envelope);
        assert_eq!(status, 200);
        let terminal: serde_json::Value =
            serde_json::from_str(body.lines().last().unwrap()).unwrap();
        assert_eq!(terminal["status"], "incomplete");
    }
}
