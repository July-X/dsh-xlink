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
            Failure::Status(status, body) => format!(
                "服务端返回 {status}{}",
                if body.is_empty() {
                    String::new()
                } else {
                    format!("：{body}")
                }
            ),
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

/// 沿路由表跑一次请求。状态码错误直接返回（不换路）。
fn send<F>(url: &str, send_once: F) -> Result<String, Failure>
where
    F: Fn(&ureq::Agent, &str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
{
    let mut tried: Vec<String> = Vec::new();
    let mut last: Option<Failure> = None;
    for route in net_proxy::routes() {
        let agent = match agent_for(&route) {
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
                    return Ok(text);
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
    send(url, |agent, url| agent.get(url).call())
}

/// POST 一份 JSON 文本（公开客户端：PKCE，无 basic 凭据）。`&str` 直接
/// 作为 body 发送（AsSendBody 原生支持），不经 `send_json` 二次编码。
pub(crate) fn post_json(url: &str, body: &str) -> Result<String, Failure> {
    send(url, |agent, url| {
        agent
            .post(url)
            .header("content-type", "application/json")
            .send(body)
    })
}

/// 表单 POST（token 端点：`application/x-www-form-urlencoded`）。
pub(crate) fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<String, Failure> {
    send(url, |agent, url| agent.post(url).send_form(pairs.to_vec()))
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
