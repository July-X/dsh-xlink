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

fn serve(stream: &mut TcpStream, token: &str, models: &ModelsSource) {
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
    let body = match request.path.as_str() {
        "/v1/handshake" => {
            format!("{{\"protocol\":{PROTOCOL_VERSION},\"service\":\"xlink-openai-oauth\"}}")
        }
        "/v1/models" => match models() {
            Ok(payload) => payload,
            Err((status, reason)) => {
                let _ = crate::openai::http::write_response(
                    stream,
                    status,
                    "application/json",
                    &format!("{{\"error\":\"{reason}\"}}"),
                );
                return;
            }
        },
        _ => {
            let _ = crate::openai::http::write_response(
                stream,
                404,
                "application/json",
                "{\"error\":\"not-found\"}",
            );
            return;
        }
    };
    let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
}

/// `/v1/models` 的数据源：返回 Host 契约载荷，或（HTTP 状态码, 原因）。
/// 生产实现连真实目录（`catalog::serve_payload`）；测试注入桩。
pub(crate) type ModelsSource = Arc<dyn Fn() -> Result<String, (u16, String)> + Send + Sync>;

/// 起一个新桥接（绑定 `127.0.0.1:0`）。失败信息带监听地址语义，可直接给日志。
fn spawn_bridge(key: &str, models: ModelsSource) -> Result<Arc<Bridge>, String> {
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
                serve(&mut stream, &thread_token, &thread_models);
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
    if let Some(old) = registry.insert(key.clone(), spawn_bridge(&key, production_models_source())?)
    {
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
        let bridge = spawn_bridge(
            &format!("test-{tag}"),
            Arc::new(|| Ok("{\"revision\":\"stub-0\",\"models\":[]}".to_string())),
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
}
