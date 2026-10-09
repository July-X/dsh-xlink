//! OAuth 回调监听（设计 §8「本地回调严格绑定 127.0.0.1、路径和本次端口」）。
//!
//! 一次性监听：`spawn` 绑定 `127.0.0.1:0` 随机端口并返回
//! `redirect_uri`（拼进授权 URL），浏览器带 code 重定向回来后完成一次
//! 交换即停机。`state` 不匹配按可疑请求处理：仍回一页说明但流程终止
//! （不留在端口上等第二次——等下去的收益抵不过暴露面）。

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::openai::auth::{parse_callback, Callback};
use crate::openai::http;

#[derive(Debug)]
pub(crate) enum Outcome {
    Code {
        code: String,
        /// 首次注册时授权服务器在回调里签发的正式 client_id。
        client_id: Option<String>,
    },
    Error(String),
    StateMismatch,
}

pub(crate) struct CallbackListener {
    port: u16,
    shutdown: Arc<AtomicBool>,
    result: mpsc::Receiver<Outcome>,
}

impl Drop for CallbackListener {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

const CALLBACK_PATH: &str = "/callback";

impl CallbackListener {
    pub(crate) fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// 等回调；超时报错（授权页没人动是常态，文案说清下一步）。
    pub(crate) fn wait(&self, timeout: Duration) -> Result<Outcome, String> {
        self.result.recv_timeout(timeout).map_err(|_| {
            format!(
                "等待浏览器回调超时（{} 秒）：请重试登录；若浏览器没有打开，检查默认浏览器设置",
                timeout.as_secs()
            )
        })
    }
}

pub(crate) fn spawn(state: &str) -> Result<CallbackListener, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("回调监听绑定 127.0.0.1 失败：{error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("取回调监听端口失败：{error}"))?
        .port();
    let shutdown = Arc::new(AtomicBool::new(false));
    let (sender, result) = mpsc::channel();
    let thread_flag = Arc::clone(&shutdown);
    let thread_state = state.to_string();
    std::thread::Builder::new()
        .name("oop-callback".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if thread_flag.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let Ok(request) = http::read_request(&mut stream) else { continue };
                let Some(query) = request.path.strip_prefix(&format!("{CALLBACK_PATH}?")) else {
                    let _ = http::write_response(
                        &mut stream,
                        404,
                        "text/plain; charset=utf-8",
                        "not found",
                    );
                    continue;
                };
                let outcome = match parse_callback(query, &thread_state) {
                    Callback::Code { code, client_id } => Outcome::Code { code, client_id },
                    Callback::Error { error } => Outcome::Error(error),
                    Callback::StateMismatch => Outcome::StateMismatch,
                };
                let body = match &outcome {
                    Outcome::Code { .. } => "<!doctype html><meta charset=\"utf-8\"><body style=\"font-family:system-ui;background:#f7f8fa;color:#1f2328;display:grid;place-items:center;height:100vh;margin:0\"><p>登录完成，可以关闭此页返回 dsh-xlink。</p>".to_string(),
                    Outcome::Error(_) => "<!doctype html><meta charset=\"utf-8\"><body style=\"font-family:system-ui;background:#f7f8fa;color:#1f2328;display:grid;place-items:center;height:100vh;margin:0\"><p>授权未完成（被拒绝或出错），请回到 dsh-xlink 重试。</p>".to_string(),
                    Outcome::StateMismatch => "<!doctype html><meta charset=\"utf-8\"><body style=\"font-family:system-ui;background:#f7f8fa;color:#1f2328;display:grid;place-items:center;height:100vh;margin:0\"><p>回调校验失败（state 不匹配），流程已终止；请回到 dsh-xlink 重新登录。</p>".to_string(),
                };
                let _ = http::write_response(&mut stream, 200, "text/html; charset=utf-8", &body);
                let _ = sender.send(outcome);
                break;
            }
        })
        .map_err(|error| format!("启动回调监听线程失败：{error}"))?;
    Ok(CallbackListener {
        port,
        shutdown,
        result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, path: &str) -> (u16, String) {
        use std::io::{Read, Write};
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        let status: u16 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, buf)
    }

    #[test]
    fn happy_path_returns_code_and_closes() {
        let listener = spawn("st1").unwrap();
        let (status, response) = get(listener.port, "/callback?code=abc&state=st1");
        assert_eq!(status, 200);
        assert!(response.contains("登录完成"));
        match listener.wait(Duration::from_secs(5)).unwrap() {
            Outcome::Code { code, .. } => assert_eq!(code, "abc"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn state_mismatch_stops_the_flow() {
        let listener = spawn("st2").unwrap();
        let (status, response) = get(listener.port, "/callback?code=abc&state=evil");
        assert_eq!(status, 200);
        assert!(response.contains("state 不匹配"));
        assert!(matches!(
            listener.wait(Duration::from_secs(5)).unwrap(),
            Outcome::StateMismatch
        ));
    }

    #[test]
    fn provider_error_is_reported() {
        let listener = spawn("st3").unwrap();
        get(listener.port, "/callback?error=access_denied&state=st3");
        match listener.wait(Duration::from_secs(5)).unwrap() {
            Outcome::Error(error) => assert_eq!(error, "access_denied"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn other_paths_get_404_without_consuming_the_listener() {
        let listener = spawn("st4").unwrap();
        assert_eq!(get(listener.port, "/").0, 404);
        // 404 不消费监听：正确的回调仍能完成。
        get(listener.port, "/callback?code=xyz&state=st4");
        match listener.wait(Duration::from_secs(5)).unwrap() {
            Outcome::Code { code, .. } => assert_eq!(code, "xyz"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn timeout_when_no_browser_redirect_arrives() {
        let listener = spawn("st5");
        let error = listener
            .unwrap()
            .wait(Duration::from_millis(200))
            .unwrap_err();
        assert!(error.contains("超时"), "{error}");
    }
}
