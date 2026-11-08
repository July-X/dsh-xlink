//! MiniMax WebUI 的任务完成通知订阅。
//!
//! # 为什么要单独一条订阅
//!
//! DSH 内核在 /api/remote.mux 上推 Cordis 事件（见 super::task），MiniMax 的
//! WebUI 是另一套进程与协议：本机回环上一个 WebSocket 服务，客户端发
//! protocolVersion = 1 的 request 帧，服务端用 event 帧回推应用事件。桌面壳作为
//! 第二个客户端订阅 watchEvents，判据只有一条：session.finish（某轮对话正常
//! 结束）→ 生成一条 MiniMax 来源的完成记录。
//!
//! # 生命周期
//!
//! 随 MiniMax 工作台启停：start_mcode 成功后 start()，stop_mcode 与退出时 stop()。
//! 工作台崩了/端口换了，订阅线程自己按退避重连；工作台不在时只轮询状态、不连端口。
//!
//! 这是上游私有协议，壳只支持当前交付的 WebUI 0.1.7（安装与启动本来就只认这一版）。
//! 它变了不能升级通知：连接或解析失败只写壳日志、把「MiniMax 事件流未连接」报给
//! 面板，DSH 通知照常，不影响工作台本身。
use crate::kernel::mcode_service;
use crate::notify::task::{self, CompletedTask, TaskKernel};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tungstenite::client::IntoClientRequest;
use tungstenite::Message;

/// 订阅请求的固定标识：同一条连接上唯一即可。
const REQUEST_ID: &str = "dsh-xlink-notify-events";
/// 上游协议版本；与 WebUI 0.1.7 的 WEBUI_PROTOCOL_VERSION 对齐。
const PROTOCOL_VERSION: u64 = 1;
/// 订阅操作名（watchEvents 是全量应用事件流，客户端侧栏也订阅它）。
const OPERATION: &str = "watchEvents";
/// 读超时：让线程周期性检查停止信号。
const READ_TICK: Duration = Duration::from_millis(400);
/// 工作台没起来时的探测间隔。
const IDLE_TICK: Duration = Duration::from_secs(1);
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(20);
/// 已通知的 turnId 保留条数：同一轮重放不重复弹通知。
const NOTIFIED_LIMIT: usize = 64;
/// 失败留痕的节流：坏协议不该刷屏「查看日志」。
const ERROR_NOTE_INTERVAL: Duration = Duration::from_secs(60);

/// 连接内的事件状态。重连即重置（上游不重放历史事件）。
#[derive(Default)]
struct Events {
    /// 会话 id → 标题（session.title_updated / session.created）。
    titles: HashMap<String, String>,
    /// 会话 id → 本轮开始时刻（毫秒）。
    running: HashMap<String, u64>,
    /// 已经通知过的轮次（turn id），新的在后。
    notified: VecDeque<String>,
    /// 子代理会话：它们的完成不该打扰用户（与 DSH 侧同一语义）。
    subagents: std::collections::HashSet<String>,
}

/// 一次 session.finish 拆出来的完成信息（纯数据，便于单测）。
#[derive(Debug, PartialEq, Eq)]
struct Completion {
    session_id: String,
    title: String,
    duration_ms: u64,
}

fn state() -> &'static Mutex<Events> {
    static STATE: OnceLock<Mutex<Events>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(Events::default()))
}

/// 这一帧的会话是不是子代理：带父会话 id 的都不打扰用户。
fn has_parent(payload: &Value) -> bool {
    payload
        .get("parentSessionId")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.trim().is_empty())
}

fn brief(id: &str) -> String {
    let head: String = id.chars().take(8).collect();
    if id.chars().count() > 8 {
        format!("{head}…")
    } else {
        head
    }
}

/// 把一条应用事件并入状态机；只有 session.finish 会产出完成信息。
///
/// 不碰 AppHandle、不弹通知、不动未读——那些是调用方的决定，见
/// task::record_completion。
fn apply_event(
    state: &mut Events,
    event: &str,
    payload: &Value,
    now_ms: u64,
) -> Option<Completion> {
    let session_id = payload
        .get("sessionId")
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if session_id.is_empty() {
        return None;
    }
    match event {
        "session.start" => {
            state.running.entry(session_id).or_insert(now_ms);
            None
        }
        "session.title_updated" | "session.created" => {
            if let Some(title) = payload
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
            {
                state.titles.insert(session_id.clone(), title.to_string());
            }
            if has_parent(payload) {
                state.subagents.insert(session_id);
            }
            None
        }
        "session.finish" => {
            if has_parent(payload) || state.subagents.contains(&session_id) {
                state.running.remove(&session_id);
                return None;
            }
            // turnId 由上游在每轮开始时生成；没有它时退回「会话 + 完成时刻」当键。
            let turn_id = payload
                .get("turnId")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{session_id}#{now_ms}"));
            if state.notified.iter().any(|seen| seen == &turn_id) {
                return None;
            }
            state.notified.push_back(turn_id);
            while state.notified.len() > NOTIFIED_LIMIT {
                state.notified.pop_front();
            }
            let started = state.running.remove(&session_id);
            let title = state
                .titles
                .get(&session_id)
                .cloned()
                .unwrap_or_else(|| format!("MiniMax 会话 {}", brief(&session_id)));
            Some(Completion {
                session_id,
                title,
                duration_ms: started
                    .map(|started| now_ms.saturating_sub(started))
                    .unwrap_or(0),
            })
        }
        // 中止 / 出错不是「任务完成」：清掉运行标记，但不打扰用户。
        "session.abort" | "session.error" => {
            state.running.remove(&session_id);
            None
        }
        "session.deleted" => {
            state.running.remove(&session_id);
            state.titles.remove(&session_id);
            None
        }
        _ => None,
    }
}

/// 订阅请求帧。抽出来是为了让单测直接比对形状（协议字段名写错时连接只会
/// 收到 error 帧，症状是「一条通知都没有」且不报错）。
fn watch_request() -> Value {
    serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        "kind": "request",
        "requestId": REQUEST_ID,
        "operation": OPERATION,
        "body": {},
    })
}

struct Watcher {
    stop: Arc<AtomicBool>,
    done: mpsc::Receiver<()>,
}

fn watcher() -> &'static Mutex<Option<Watcher>> {
    static WATCHER: OnceLock<Mutex<Option<Watcher>>> = OnceLock::new();
    WATCHER.get_or_init(|| Mutex::new(None))
}

/// 启动 MiniMax 事件订阅线程。重复调用幂等（已有线程在跑就直接返回）。
pub fn start(app: &AppHandle) {
    let mut guard = crate::lock(watcher());
    if guard.is_some() {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, done) = mpsc::channel();
    let thread_stop = Arc::clone(&stop);
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("dsh-mcode-notify-watch".into())
        .spawn(move || {
            watch_loop(&handle, &thread_stop);
            let _ = tx.send(());
        });
    match spawned {
        Ok(_) => *guard = Some(Watcher { stop, done }),
        Err(error) => {
            crate::shell::shell_events::record(
                "notification-mcode",
                &format!("无法启动 MiniMax 任务通知订阅线程：{error}"),
            );
        }
    }
}

/// 停止订阅线程并等它退出（最多 3 秒）。MiniMax 停止、壳退出时调用。
pub fn stop() {
    let watcher = crate::lock(watcher()).take();
    let Some(watcher) = watcher else {
        return;
    };
    watcher.stop.store(true, Ordering::Relaxed);
    let _ = watcher.done.recv_timeout(Duration::from_secs(3));
    *crate::lock(state()) = Events::default();
}

fn sleep_tick(stop: &AtomicBool, duration: Duration) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn watch_loop(app: &AppHandle, stop: &AtomicBool) {
    let mut backoff = RECONNECT_MIN;
    let mut last_note: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let status = match mcode_service::status() {
            Ok(status) => status,
            Err(_) => {
                task::set_mcode_watch_state(app, false, None);
                sleep_tick(stop, IDLE_TICK);
                continue;
            }
        };
        let Some(port) = status.port.filter(|_| status.running) else {
            task::set_mcode_watch_state(app, false, None);
            sleep_tick(stop, IDLE_TICK);
            continue;
        };
        match subscribe_once(app, port, stop) {
            Ok(()) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                backoff = RECONNECT_MIN;
            }
            Err(error) => {
                task::set_mcode_watch_state(app, false, Some(error.clone()));
                let due = last_note.is_none_or(|at| at.elapsed() >= ERROR_NOTE_INTERVAL);
                if due {
                    last_note = Some(Instant::now());
                    crate::shell::shell_events::record("notification-mcode", &error);
                }
                sleep_tick(stop, backoff);
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }
    }
    task::set_mcode_watch_state(app, false, None);
}

/// 一次完整订阅：连 WS → 发 watchEvents → 读 ack 与事件，直到断开。
fn subscribe_once(app: &AppHandle, port: u16, stop: &AtomicBool) -> Result<(), String> {
    let url = format!("ws://127.0.0.1:{port}/");
    let request = url
        .into_client_request()
        .map_err(|error| format!("无法构造 MiniMax 事件流请求：{error}"))?;
    let (mut socket, _response) = tungstenite::connect(request).map_err(|error| {
        format!(
            "无法连上 MiniMax 事件流（{error}）。MiniMax 任务完成通知暂时不可用；重开 MiniMax 工作台后会自动重连"
        )
    })?;
    if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_ref() {
        let _ = tcp.set_read_timeout(Some(READ_TICK));
    }
    socket
        .send(Message::Text(watch_request().to_string().into()))
        .map_err(|error| format!("无法在 MiniMax 事件流上打开 watchEvents 订阅：{error}"))?;

    let mut acknowledged = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return Ok(());
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                handle_frame(app, text.as_str(), &mut acknowledged)?;
                let _ = socket.flush();
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(error) => {
                return Err(format!(
                    "MiniMax 事件流已断开（{error}）。MiniMax 任务完成通知会暂停，壳会自动重连"
                ));
            }
        }
    }
}

/// 处理一条文本帧。返回错误表示这条连接不可再用（error 帧 / 协议不认识）。
fn handle_frame(app: &AppHandle, text: &str, acknowledged: &mut bool) -> Result<(), String> {
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return Ok(());
    };
    match frame.get("kind").and_then(Value::as_str) {
        Some("response") => {
            if !*acknowledged {
                *acknowledged = true;
                task::set_mcode_watch_state(app, true, None);
            }
            Ok(())
        }
        Some("error") => Err(format!(
            "MiniMax 事件流拒绝了订阅：{}。任务完成通知暂时不可用；若持续出现请把这条日志发给维护者（MiniMax WebUI 协议可能已变化）",
            frame
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
        )),
        Some("event") => {
            let Some(body) = frame.get("body") else {
                return Ok(());
            };
            let Some(event) = body.get("type").and_then(Value::as_str) else {
                return Ok(());
            };
            let payload = body.get("payload").cloned().unwrap_or(Value::Null);
            let now = crate::shell::process::epoch_millis();
            let completion = {
                let mut state = crate::lock(state());
                apply_event(&mut state, event, &payload, now)
            };
            if let Some(completion) = completion {
                task::record_completion(
                    app,
                    CompletedTask {
                        kernel: TaskKernel::Mcode,
                        session_id: completion.session_id,
                        title: completion.title,
                        cwd: String::new(),
                        finished_at_ms: now,
                        duration_ms: completion.duration_ms,
                        last_prompt: String::new(),
                        last_response: String::new(),
                    },
                );
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_request_matches_the_webui_envelope() {
        let request = watch_request();
        assert_eq!(request["protocolVersion"], 1);
        assert_eq!(request["kind"], "request");
        assert_eq!(request["requestId"], REQUEST_ID);
        assert_eq!(request["operation"], "watchEvents");
        assert_eq!(request["body"], serde_json::json!({}));
    }

    #[test]
    fn finish_after_start_reports_title_and_duration() {
        let mut state = Events::default();
        apply_event(
            &mut state,
            "session.start",
            &serde_json::json!({ "sessionId": "s-1", "turnId": "t-1" }),
            1_000,
        );
        apply_event(
            &mut state,
            "session.title_updated",
            &serde_json::json!({ "sessionId": "s-1", "title": "修登录" }),
            1_500,
        );
        let completion = apply_event(
            &mut state,
            "session.finish",
            &serde_json::json!({ "sessionId": "s-1", "turnId": "t-1", "status": "finished" }),
            61_000,
        )
        .expect("完成的轮次要产出通知");
        assert_eq!(completion.session_id, "s-1");
        assert_eq!(completion.title, "修登录");
        assert_eq!(completion.duration_ms, 60_000);
    }

    #[test]
    fn duplicate_finish_does_not_notify_twice() {
        let mut state = Events::default();
        let finish = serde_json::json!({ "sessionId": "s-1", "turnId": "t-1" });
        assert!(apply_event(&mut state, "session.finish", &finish, 2_000).is_some());
        assert!(
            apply_event(&mut state, "session.finish", &finish, 2_100).is_none(),
            "同一轮重放不能重复弹通知"
        );
    }

    #[test]
    fn subagent_sessions_do_not_notify() {
        let mut state = Events::default();
        apply_event(
            &mut state,
            "session.created",
            &serde_json::json!({ "sessionId": "child", "parentSessionId": "root" }),
            1_000,
        );
        assert!(
            apply_event(
                &mut state,
                "session.finish",
                &serde_json::json!({ "sessionId": "child", "turnId": "t-child" }),
                2_000,
            )
            .is_none(),
            "子代理会话跑完不该打扰用户"
        );
        // 父会话照样通知。
        assert!(apply_event(
            &mut state,
            "session.finish",
            &serde_json::json!({ "sessionId": "root", "turnId": "t-root" }),
            2_000,
        )
        .is_some());
    }

    #[test]
    fn finish_without_a_known_title_falls_back_to_the_session_id() {
        let mut state = Events::default();
        let completion = apply_event(
            &mut state,
            "session.finish",
            &serde_json::json!({ "sessionId": "abcdefghijkl", "turnId": "t-9" }),
            2_000,
        )
        .expect("没有标题也要能通知");
        assert_eq!(completion.title, "MiniMax 会话 abcdefgh…");
        assert_eq!(completion.duration_ms, 0, "没看到开始时刻就不编时长");
    }

    #[test]
    fn abort_and_error_are_not_task_completions() {
        let mut state = Events::default();
        assert!(apply_event(
            &mut state,
            "session.abort",
            &serde_json::json!({ "sessionId": "s" }),
            1,
        )
        .is_none());
        assert!(apply_event(
            &mut state,
            "session.error",
            &serde_json::json!({ "sessionId": "s" }),
            1,
        )
        .is_none());
        assert!(apply_event(&mut state, "session.finish", &serde_json::json!({}), 1).is_none());
    }

    /// 真机握手自检：在本机 MiniMax WebUI 上完成 admission + watchEvents ack。
    ///
    /// 壳的 Rust 客户端与浏览器发的头不一样（没有 Origin、不带 token），
    /// 「浏览器能连」不等于「壳能连」：这一条专门验那半截。用法：
    ///   MINIMAX_DATA_DIR=/tmp/mcode-live WEBUI_SERVER_PORT=23131 \
    ///   WEBUI_OPEN_BROWSER=0 node <包>/bin/mcode-webui.mjs &
    ///   cargo test --locked --lib mcode_watch::tests::live -- --ignored
    #[test]
    #[ignore = "需要本机 MiniMax WebUI 在 127.0.0.1:23131 运行"]
    fn live_mcode_stream_acknowledges_watch_events() {
        let url = "ws://127.0.0.1:23131/";
        let request = url.into_client_request().expect("能构造请求");
        let (mut socket, _response) =
            tungstenite::connect(request).expect("admission 应放行回环 dev 连接");
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = socket.get_ref() {
            let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
        }
        socket
            .send(Message::Text(watch_request().to_string().into()))
            .expect("能发出 watchEvents");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut acknowledged = false;
        while Instant::now() < deadline && !acknowledged {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let frame: Value = serde_json::from_str(text.as_str()).expect("帧是 JSON");
                    match frame.get("kind").and_then(Value::as_str) {
                        Some("response") => {
                            assert_eq!(
                                frame["body"]["stream"], true,
                                "watchEvents 的 ack 是 stream"
                            );
                            acknowledged = true;
                        }
                        Some("error") => panic!("订阅被拒：{frame}"),
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => panic!("读事件流失败：{error}"),
            }
        }
        assert!(acknowledged, "5 秒内没等到 watchEvents 的 ack");
    }
}
