//! 内置插件的**回环桥接**：让跑在内核里的插件能把「立刻拉上游」这个动作
//! 交给桌面壳执行。
//!
//! ## 为什么需要它
//!
//! `/ponytail-update` 是一条**技能**，由模型在工作台里调用；而同步这件事
//! 必须在壳这一侧做（出网纪律、代理路由、SRI 校验、解包防穿越都只有壳这一侧
//! 有，见 [`crate::plugins::builtin::upstream`]）。插件在内核进程里、壳在另一个
//! 进程里，于是需要一条两者之间的通道。
//!
//! ## 为什么是「插件来请求壳」，而不是反过来
//!
//! 反过来（壳主动推给内核）需要长连接或轮询；正过来只要一个一次性请求。
//! 形状与 [`crate::openai::bridge`] 相同：`Authorization: Bearer <令牌>`，
//! 令牌按「(族, 实例)」一代一换，只绑回环。
//!
//! **这不是针对恶意插件的沙盒**（dsh 插件本来就拥有宿主进程权限，与 openai
//! 桥接同一条前提）。令牌防的是**其它本地进程**顺手发请求；随机端口 + 令牌
//! 一起挡住跨源浏览器请求。
//!
//! 只有一个端点：`POST /v1/builtin/sync`。加端点时想清楚「谁会调它」——
//! 这个桥的调用方是模型能触达的代码，能力面越小越好。

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rand::Rng;

/// 一个运行中的桥接代次。
struct Bridge {
    addr: std::net::SocketAddr,
    token: String,
    shutdown: Arc<AtomicBool>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // 唤醒阻塞在 accept 上的线程：连一次自己。
        let _ = TcpStream::connect(self.addr);
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

/// 令牌的环境变量名（与地址的一起交给内核进程）。
pub(crate) const TOKEN_ENV: &str = "DSH_XLINK_BUILTIN_BRIDGE_TOKEN";
/// 地址的环境变量名。
pub(crate) const URL_ENV: &str = "DSH_XLINK_BUILTIN_BRIDGE_URL";

/// 起一代桥接并返回 `Some((地址, 令牌))`；没有需要桥接的内置插件时返回 `None`。
///
/// 调用点只有启动内核那一处，且**起不来不阻断启动**：插件的「检查更新」按钮
/// （面板里那条命令）不依赖它，少一个能力不该让工作台起不来。
pub(crate) fn launch(dsh_home: &Path, scope_key: &str) -> Option<(String, String)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let addr = listener.local_addr().ok()?;
    let token = new_token();
    let bridge = Arc::new(Bridge {
        addr,
        token: token.clone(),
        shutdown: Arc::new(AtomicBool::new(false)),
    });
    bridges()
        .lock()
        .ok()?
        .insert(scope_key.to_string(), bridge.clone());
    let dsh_home = dsh_home.to_path_buf();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if bridge.shutdown.load(Ordering::SeqCst) {
                break;
            }
            let Ok(mut stream) = stream else { continue };
            let token = bridge.token.clone();
            let dsh_home = dsh_home.clone();
            // 每个连接一个线程：端点只有一个且是一次性请求，串行化反而会让
            // 一个慢请求堵住后面的（模型那边是并发调用的）。
            std::thread::spawn(move || serve(&mut stream, &token, &dsh_home));
        }
    });
    Some((format!("http://{addr}"), token))
}

fn serve(stream: &mut TcpStream, token: &str, dsh_home: &Path) {
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
            r#"{"error":"unauthorized"}"#,
        );
        return;
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/ping") => {
            let _ = crate::openai::http::write_response(
                stream,
                200,
                "application/json",
                r#"{"ok":true}"#,
            );
        }
        (method, path) if method == "GET" && path.starts_with("/v1/builtin/sync") => {
            // **GET + 查询串，不读请求体**：`openai::http::read_request` 只保证读到
            // 请求头，body 可能在同一次 read 里到达也可能没有（TCP 无消息边界）。
            // 拿 body_prefix 当请求体在「客户端分两次发」的写法下会读到空串，
            // 而症状是「插件名缺失」——查不出原因的那种。
            let plugin = query_value(path, "plugin").unwrap_or_default();
            if plugin.is_empty() {
                let _ = crate::openai::http::write_response(
                    stream,
                    400,
                    "application/json",
                    r#"{"error":"missing plugin"}"#,
                );
                return;
            }
            let Some(spec) = super::builtins::find(&plugin) else {
                let _ = crate::openai::http::write_response(
                    stream,
                    404,
                    "application/json",
                    r#"{"error":"not a builtin plugin"}"#,
                );
                return;
            };
            if spec.upstream.is_none() {
                let _ = crate::openai::http::write_response(
                    stream,
                    409,
                    "application/json",
                    r#"{"error":"plugin has no upstream"}"#,
                );
                return;
            }
            // 同步是慢的（下载 + 解包），而模型那边的工具调用有超时。这条端点
            // **同步等**，让调用方拿到真实结果而不是一个「已受理」——技能要把
            // 结果汇报给用户，拿到「已受理」就只能空口说「正在拉」。
            let state = super::upstream::sync(dsh_home, spec, true);
            let body = serde_json::json!({
                "plugin": spec.id,
                "displayName": spec.display_name,
                "version": state.version,
                "error": state.error,
                "checkedAt": state.checked_at,
            })
            .to_string();
            let _ = crate::openai::http::write_response(stream, 200, "application/json", &body);
        }
        _ => {
            let _ = crate::openai::http::write_response(
                stream,
                404,
                "application/json",
                r#"{"error":"not found"}"#,
            );
        }
    }
}

/// 从查询串里取一个值。读不出来返回 `None` 而不是猜一个默认：猜中的话
/// **替另一个插件拉了更新**，而用户毫无察觉。
fn query_value(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some((name, value)) = pair.split_once('=') {
            if name == key {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_value_reads_the_plugin_name() {
        assert_eq!(
            query_value("/v1/builtin/sync?plugin=ponytail", "plugin").as_deref(),
            Some("ponytail")
        );
        assert_eq!(
            query_value("/v1/builtin/sync?a=1&plugin=ponytail&b=2", "plugin").as_deref(),
            Some("ponytail")
        );
    }

    /// 读不出来必须回 `None` 而不是猜一个：技能会把 `None` 报成「插件名缺失」，
    /// 而猜一个默认插件的后果是**替另一个插件拉了更新**。
    #[test]
    fn query_value_gives_up_instead_of_guessing() {
        assert_eq!(query_value("/v1/builtin/sync", "plugin"), None);
        assert_eq!(
            query_value("/v1/builtin/sync?other=ponytail", "plugin"),
            None
        );
        assert_eq!(
            query_value("/v1/builtin/sync?plugin=", "plugin").as_deref(),
            Some("")
        );
    }
}
