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

/// 按路由建一个 ureq agent。
///
/// `timeout` 交给调用方给（OAuth 全流程 30s，额度查询 15s），其余纪律对所有
/// 调用点一致，**不接受各自复制一份**——这两条曾经就各抄了一份：
/// ① `http_status_as_error(false)`：非 2xx 必须读响应体才能分类
///    （ureq 默认转成 `Error::StatusCode` 且不带 body）；
/// ② `Route::Direct` 显式 `.proxy(None)`：默认配置会捡环境变量里的代理，
///    不关掉就等于「直连」其实又走了 `HTTPS_PROXY`（模块头纪律 ②）。
///
/// `pub(crate)` 是因为额度查询（`usage::subscription_openai`）打的是同一批
/// 网关、同一个 ureq 客户端形状，它那份只多一层按路由键的缓存。
pub(crate) fn agent_for(route: &Route, timeout: Duration) -> Result<ureq::Agent, String> {
    let builder = ureq::Agent::config_builder()
        // 状态码错误由 send() 手动分类（ureq 的 StatusCode 错误不带响应体，
        // 而 OAuth 的 invalid_grant 分类必须看 body——实测踩过）。
        .http_status_as_error(false)
        .timeout_global(Some(timeout));
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
///
/// `pub(crate)` 是因为额度查询（`usage::subscription_openai`）打的是同一个
/// `chatgpt.com`、经同一批网关，此前它缺这一分支：403 被一律报成「凭据被拒
/// 请刷新登录」，而真正的原因是**出口被拦**——用户按提示去刷新一个完全
/// 无辜的登录态。两边判错就会给出两套互相矛盾的诊断。
pub(crate) fn looks_like_gateway_challenge(status: u16, body: &str) -> bool {
    if status != 403 {
        return false;
    }
    // BOM（U+FEFF）在 Unicode 里不是 White_Space，`trim_start` **不会**剥它，
    // 而带 BOM 的响应体并不罕见。漏掉它不会造成误报，只会让一次挑战页被
    // 归成「凭据被拒」——正是这条分支要避免的那件事。
    body.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
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
        let agent = match agent_for(route, GLOBAL_TIMEOUT) {
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
///
/// `session_id` 非空时带 `session-id` 头：它与请求体里的 `prompt_cache_key`
/// 是**同一个会话身份的两个投递位置**——原生 Codex 两处都发
/// （`codex-rs/core/src/client.rs` 的 `prompt_cache_key` 与
/// `responses_session_id`，根代理下两者相等；头的构造在
/// `codex-rs/codex-api/src/requests/headers.rs`，`api.openai.com` 与
/// `chatgpt.com/backend-api/codex` 两条基址都发），而它自己的注释写着
/// 「ChatGPT derives cache affinity from the Responses session-id header」。
/// **别只发一半**：只发请求体那一份，等于把亲和性交给上游自己合成随机键。
pub(crate) fn post_stream(
    url: &str,
    bearer: &str,
    body: &str,
    session_id: Option<&str>,
) -> Result<Box<dyn std::io::BufRead + Send>, Failure> {
    post_stream_over(&net_proxy::routes(), url, bearer, body, session_id)
}

fn post_stream_over(
    routes: &[net_proxy::Route],
    url: &str,
    bearer: &str,
    body: &str,
    session_id: Option<&str>,
) -> Result<Box<dyn std::io::BufRead + Send>, Failure> {
    let routes = routes_for_url(routes, url);
    let mut tried: Vec<String> = Vec::new();
    let mut last: Option<Failure> = None;
    for route in &routes {
        let agent = match agent_for(route, GLOBAL_TIMEOUT) {
            Ok(agent) => agent,
            Err(error) => return Err(Failure::Transport(error)),
        };
        let mut builder = agent
            .post(url)
            .header("authorization", &format!("Bearer {bearer}"))
            .header("content-type", "application/json");
        // 缓存亲和性靠这个头（见 post_stream 的注释）。没有会话身份时**不发**，
        // 发一个空头比不发更糟——空值不是「没有身份」，是一段空字符串。
        if let Some(id) = session_id.filter(|id| !id.is_empty()) {
            builder = builder.header("session-id", id);
        }
        let request = builder.send(body);
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
#[allow(dead_code, reason = "PKCE 无 basic 凭据的 POST")]
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

    /// 本机回环的极简服务：回固定响应，让请求走真实的 ureq/网络栈。
    /// `head` 是状态行与响应头（不含 Content-Length / Connection）。
    ///
    /// **必须先把整份请求读掉再回响应**（`stream.read` 那行）：不等客户端的
    /// 请求字节离开接收缓冲区就 close，macOS 发的是 RST 而不是 FIN，客户端
    /// 能否读到响应纯看时序——于是同一条断言时而绿时而红，红时拿到的是
    /// Transport 而不是被测的 Status。2026-10-09 发 v0.4.4 时踩到：
    /// `transport_vs_status_classification` 在 CI 上绿、在发布流水线上红，
    /// 报 `Transport("io: Invalid argument (os error 22)")`，而同一批 820
    /// 个用例全绿。写这个服务时漏掉读请求，红的表象与真正原因隔着两层。
    ///
    /// 2026-10-13 又补一刀：**「读一次」不等于「读完整份请求」**。POST 的头与
    /// 体可能落在两次 write 里，服务端读完头（第一次 read 就返回）就回响应并
    /// close，接收缓冲区里还剩请求体，Windows 回 RST，客户端拿到的是
    /// `Transport("io: 你的主机中的软件中止了一个已建立的连接。 (os error 10053)")`
    /// ——红的正是 `get_and_form_roundtrip_over_real_stack` 里 post_form 那一行，
    /// 而它与被测的协议语义毫无关系。本机实测整机有负载时约 3/8 次（我的新
    /// 用例全跳过也照红，是既有 flake）。现在按 Content-Length 收到体满才回响应。
    fn serve_once(head: &'static str, body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok();
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request_complete(&request) {
                    match stream.read(&mut buf) {
                        // 对端关了 / 超时：手里有什么算什么，别把测试吊死。
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let response = format!(
                    "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        port
    }

    /// 请求是否已完整到达：头结束（CRLFCRLF）+ `Content-Length` 指明的体收满。
    /// 没有 Content-Length（GET）时只看头是否结束。
    fn request_complete(request: &[u8]) -> bool {
        let text = String::from_utf8_lossy(request);
        let Some(end) = text.find("\r\n\r\n") else {
            return false;
        };
        let length = text[..end]
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        text.len() - (end + 4) >= length
    }

    #[test]
    fn get_and_form_roundtrip_over_real_stack() {
        let port = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json",
            r#"{"issuer":"https://i"}"#,
        );
        let got = get_json(&format!("http://127.0.0.1:{port}/.well-known/x")).unwrap();
        assert!(got.contains("https://i"));
        let port = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json",
            r#"{"token_type":"Bearer"}"#,
        );
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
        // 404：状态错误，message 不说「网络不可达」。服务走 serve_once——本地
        // 内联一个「写完就关」的版本会让这条断言间歇性拿到 Transport（见该
        // helper 的注释），于是判据测的是时序而不是被测的状态分类。
        let port = serve_once("HTTP/1.1 404 Not Found", "");
        match get_json(&format!("http://127.0.0.1:{port}/x")) {
            Err(failure @ Failure::Status(404, _)) => {
                assert!(!failure.message(&[]).contains("网络不可达"));
            }
            other => panic!("{other:?}"),
        }
    }
}
