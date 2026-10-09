//! OAuth 出网传输（设计 §9）：按 `net_proxy::routes()` 顺序试路。
//!
//! 三条纪律与 updater 完全一致（src-tauri/AGENTS.md「进程、出网与落盘」）：
//! ① 先系统代理、传输失败再直连（末位恒为直连）；② 直连显式
//! `.proxy(None)`——`Config::default()` 会自动捡环境变量代理，不显式关掉
//! 的话「回退直连」仍被 `HTTPS_PROXY` 拉回代理，同一条路试两遍；③ **只有
//! 传输层失败才换路**：HTTP 状态码、响应解析失败换一条路只会同样地失败
//! （`Failure::Status` 不换路，如实报给上层）。
//!
//! 客户端用 `ureq`（同步、rustls）：登录全流程一共几次 JSON 调用，跑在
//! 命令层的 blocking 线程上正合适；它已在依赖树里（零新下载），也免去
//! 为四次请求引入整套 async 客户端的版本耦合（reqwest 0.13 在离线索引下
//! 解析不出配套的 h2，实测放弃）。客户端逐路构造，不共享——代理路径不同，
//! 连接池混用没有意义。

use std::time::Duration;

use crate::pkg::net_proxy::{self, Route};

const GLOBAL_TIMEOUT: Duration = Duration::from_secs(30);

/// 传输结果的两类失败：`Transport`（可换路重试）与 `Status`（不可换路）。
#[derive(Debug)]
pub(crate) enum Failure {
    Transport(String),
    Status(u16, String),
}

impl Failure {
    /// 面向用户的一句话（含已试过的路）。
    pub(crate) fn message(&self, tried: &[String]) -> String {
        match self {
            Failure::Transport(detail) => format!(
                "网络不可达（{detail}）。已试：{}；请确认系统代理正在运行或网络可用后重试",
                tried.join(" → ")
            ),
            Failure::Status(status, body) => {
                // 网关拦截页（403 + HTML，Cloudflare「Just a moment…」一类）
                // 不再整页拍给用户——它描述的是**出口被拦**，不是账号或请求
                // 问题，给可执行的指引（2026-10-09 用户反馈）。
                if looks_like_gateway_challenge(*status, body) {
                    format!(
                        "服务端返回 {status}：该网络出口被网关拦截（HTML 挑战页，不是账号或请求问题）。已试：{}；请确认系统代理正在运行后重试",
                        tried.join(" → ")
                    )
                } else {
                    format!(
                        "服务端返回 {status}{}",
                        if body.is_empty() {
                            String::new()
                        } else {
                            format!("：{body}")
                        }
                    )
                }
            }
        }
    }
}

fn agent_for(route: &Route) -> Result<ureq::Agent, String> {
    let builder = ureq::Agent::config_builder()
        // 状态码错误由 send() 手动分类（ureq 的 StatusCode 错误不带响应体，
        // 而 OAuth 的 invalid_grant 分类必须看 body——实测踩过）。
        .http_status_as_error(false)
        .timeout_global(Some(GLOBAL_TIMEOUT));
    let config = match route {
        Route::Direct => builder.proxy(None),
        Route::Proxy { url, .. } => {
            let proxy = ureq::Proxy::new(url.as_str())
                .map_err(|error| format!("代理地址不合法（{url}）：{error}"))?;
            builder.proxy(Some(proxy))
        }
    };
    Ok(config.build().new_agent())
}

fn map_ureq_error(error: ureq::Error) -> Failure {
    // 4xx/5xx 不会再走这里（http_status_as_error=false），保守映射为传输错。
    Failure::Transport(format!("{error}"))
}

/// 回环目标不走代理路由：代理不会替你访问本机（还常常显式拒绝），而
/// 测试与本地诊断大量依赖回环。返回值：非回环 → 原路由表；回环 → 只剩
/// 直连。
fn routes_for_url(routes: &[net_proxy::Route], url: &str) -> Vec<net_proxy::Route> {
    let loopback = url.contains("://127.0.0.1:") || url.contains("://localhost:");
    if loopback {
        vec![net_proxy::Route::Direct]
    } else {
        routes.to_vec()
    }
}

/// 响应体像不像网关拦截页（Cloudflare「Just a moment…」一类挑战页）：
/// OpenAI 的 API 错误一律是 JSON，HTML 只会来自网络出口的网关。
fn looks_like_gateway_challenge(status: u16, body: &str) -> bool {
    status == 403
        && body
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("<!doctype html")
}

/// 沿给定路由表跑一次请求。2xx 返回响应体；状态错误原则上直接返回
/// （不换路）——**唯一例外**：403 + HTML 挑战页 = 网关按**出口**拦人，
/// 请求本身没错，换一条路就能过（实测：直连 403、代理 200，2026-10-09），
/// 这类失败换路继续，全部路由试完仍被拦才如实上报。
fn send_over<F>(routes: &[net_proxy::Route], url: &str, send_once: F) -> Result<String, Failure>
where
    F: Fn(&ureq::Agent, &str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
{
    let routes = routes_for_url(routes, url);
    let mut tried: Vec<String> = Vec::new();
    let mut last: Option<Failure> = None;
    for route in &routes {
        let agent = match agent_for(route) {
            Ok(agent) => agent,
            Err(error) => return Err(Failure::Transport(error)),
        };
        match send_once(&agent, url) {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let text = response
                    .body_mut()
                    .read_to_string()
                    .map_err(|error| Failure::Transport(format!("读取响应体失败：{error}")))?;
                if (200..300).contains(&status) {
                    if let net_proxy::Route::Proxy { url: proxy, .. } = route {
                        net_proxy::remember_proxy(proxy);
                    }
                    return Ok(text);
                }
                if looks_like_gateway_challenge(status, &text) {
                    tried.push(route.describe());
                    last = Some(Failure::Status(status, text.chars().take(120).collect()));
                    continue;
                }
                return Err(Failure::Status(status, text.chars().take(300).collect()));
            }
            Err(error) => match map_ureq_error(error) {
                failure @ Failure::Status(..) => return Err(failure),
                failure => {
                    tried.push(route.describe());
                    last = Some(failure);
                }
            },
        }
    }
    Err(last.unwrap_or_else(|| Failure::Transport("没有可用的网络路由".into())))
}

/// GET 一份 JSON 文本。
pub(crate) fn get_json(url: &str) -> Result<String, Failure> {
    send_over(&net_proxy::routes(), url, |agent, url| {
        agent.get(url).call()
    })
}

/// 带 Bearer 鉴权的 GET（模型目录等账号端点）；路由纪律与 get_json 一致。
pub(crate) fn get_json_with_auth(url: &str, bearer: &str) -> Result<String, Failure> {
    send_over(&net_proxy::routes(), url, |agent, url| {
        agent
            .get(url)
            .header("authorization", &format!("Bearer {bearer}"))
            .call()
    })
}

/// 带鉴权的流式 POST（推理）：返回**未消费的响应体行读取器**——调用方
/// 逐行读到终止事件；网关拦截页（403 + HTML）按路由依赖失败换路重试，
/// 其余状态错误不换路。
pub(crate) fn post_stream(
    url: &str,
    bearer: &str,
    body: &str,
) -> Result<Box<dyn std::io::BufRead + Send>, Failure> {
    post_stream_over(&net_proxy::routes(), url, bearer, body)
}

fn post_stream_over(
    routes: &[net_proxy::Route],
    url: &str,
    bearer: &str,
    body: &str,
) -> Result<Box<dyn std::io::BufRead + Send>, Failure> {
    let routes = routes_for_url(routes, url);
    let mut tried: Vec<String> = Vec::new();
    let mut last: Option<Failure> = None;
    for route in &routes {
        let agent = match agent_for(route) {
            Ok(agent) => agent,
            Err(error) => return Err(Failure::Transport(error)),
        };
        let request = agent
            .post(url)
            .header("authorization", &format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .send(body);
        match request {
            Ok(response) => {
                let status = response.status().as_u16();
                if (200..300).contains(&status) {
                    if let net_proxy::Route::Proxy { url: proxy, .. } = route {
                        net_proxy::remember_proxy(proxy);
                    }
                    let (_, body) = response.into_parts();
                    return Ok(Box::new(std::io::BufReader::new(body.into_reader())));
                }
                // 状态错误要读出 body 再分类（不带 body 的分类是瞎猜）。
                let mut response = response;
                let text = response.body_mut().read_to_string().unwrap_or_default();
                if looks_like_gateway_challenge(status, &text) {
                    tried.push(route.describe());
                    last = Some(Failure::Status(status, text.chars().take(120).collect()));
                    continue;
                }
                return Err(Failure::Status(status, text.chars().take(300).collect()));
            }
            Err(error) => match map_ureq_error(error) {
                failure @ Failure::Status(..) => return Err(failure),
                failure => {
                    tried.push(route.describe());
                    last = Some(failure);
                }
            },
        }
    }
    Err(last.unwrap_or_else(|| Failure::Transport("没有可用的网络路由".into())))
}

/// POST 一份 JSON 文本（公开客户端：PKCE，无 basic 凭据）。`&str` 直接
/// 作为 body 发送（AsSendBody 原生支持），不经 `send_json` 二次编码。
pub(crate) fn post_json(url: &str, body: &str) -> Result<String, Failure> {
    send_over(&net_proxy::routes(), url, |agent, url| {
        agent
            .post(url)
            .header("content-type", "application/json")
            .send(body)
    })
}

/// 表单 POST（token 端点：`application/x-www-form-urlencoded`）。
pub(crate) fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<String, Failure> {
    send_over(&net_proxy::routes(), url, |agent, url| {
        agent.post(url).send_form(pairs.to_vec())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 本机回环的极简服务：回固定体，让请求走真实的 ureq/网络栈。
    fn serve_once(body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        port
    }

    #[test]
    fn get_and_form_roundtrip_over_real_stack() {
        let port = serve_once(r#"{"issuer":"https://i"}"#);
        let got = get_json(&format!("http://127.0.0.1:{port}/.well-known/x")).unwrap();
        assert!(got.contains("https://i"));
        let port = serve_once(r#"{"token_type":"Bearer"}"#);
        let posted = post_form(
            &format!("http://127.0.0.1:{port}/token"),
            &[("grant_type", "authorization_code")],
        )
        .unwrap();
        assert!(posted.contains("Bearer"));
    }

    #[test]
    fn transport_vs_status_classification() {
        // 端口立刻关闭：传输层错误（可换路的类别）。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(matches!(
            get_json(&format!("http://127.0.0.1:{port}/x")),
            Err(Failure::Transport(_))
        ));
        // 404：状态错误，message 不说「网络不可达」。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        match get_json(&format!("http://127.0.0.1:{port}/x")) {
            Err(failure @ Failure::Status(404, _)) => {
                assert!(!failure.message(&[]).contains("网络不可达"));
            }
            other => panic!("{other:?}"),
        }
    }
}
