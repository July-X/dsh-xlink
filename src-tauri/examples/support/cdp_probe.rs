//! CDP 求值可行性实验（**只给探针用，不进生产实现**）。
//!
//! 目的：核实「用 DevTools 协议求值替代页面槽位」这条路是否成立。
//!
//! # 要核实的三件事
//!
//! 1. `ICoreWebView2::CallDevToolsProtocolMethod` 在**未打开 DevTools 窗口**时
//!    能否调用（生产环境不会开 DevTools 窗口）。
//! 2. `Runtime.evaluate` 带 `awaitPromise: true` 时，能否直接拿到 **Promise
//!    结算后的值**（而不是 Promise 对象的序列化）。
//! 3. 求值过程是否在**页面 JS 空间留下任何全局量**（槽位方案的核心缺陷）。
//!
//! # 形态（与裁定一致）
//!
//! - 方法名：`Runtime.evaluate`
//! - 参数：`{"expression": ..., "awaitPromise": true, "returnByValue": true}`
//! - 返回：`{"result": {"result": {"type": ..., "value": ...}}, "exceptionDetails": ...}`
//!
//! 注意接口形态与裁定里的措辞有一处出入：WebView2 **没有**
//! `ICoreWebView2DevToolsProtocolController` 这个东西——`CallDevToolsProtocolMethod`
//! 是 **`ICoreWebView2` 自己的方法**（`webview2-com-sys 0.39.1`
//! `src/bindings.rs:1455` 与 `:2089`），完成回调是
//! `ICoreWebView2CallDevToolsProtocolMethodCompletedHandler`
//! （`:4861`，`Invoke(HRESULT, PCWSTR)`）。

use std::time::Duration;

use serde_json::Value;

/// 用 DevTools 协议求值，返回**页面结果**（已解包 CDP 的外层信封）。
///
/// 返回值里同时带回原始信封，便于诊断 `exceptionDetails`。
#[cfg(windows)]
pub(crate) async fn cdp_eval(
    webview: &tauri::WebviewWindow,
    expression: &str,
    await_promise: bool,
    timeout: Duration,
) -> Result<Value, String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::HSTRING;

    let params = serde_json::json!({
        "expression": expression,
        "awaitPromise": await_promise,
        "returnByValue": true,
        "userGesture": true,
    })
    .to_string();

    let (tx, mut rx) = tauri::async_runtime::channel(1);
    // 完成回调会拿走 `tx`，「发起失败」那条路径要留一份副本。
    let tx_fallback = tx.clone();
    let method = HSTRING::from("Runtime.evaluate");
    let params_hstring = HSTRING::from(params);

    let dispatched = webview.with_webview(move |platform| {
        let controller = platform.controller();
        // SAFETY: `CoreWebView2` 只是取回控制器持有的核心对象引用，不转移
        // 所有权；返回值由 COM 引用计数管理。
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(e) => {
                let _ = tx.try_send(Err(format!("取不到 WebView2 核心：{e}")));
                return;
            }
        };
        let handler =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, result| {
                // 宏已经把 HRESULT 转成 `Result<()>`，结果文本也已经转成 `String`。
                let payload: Result<String, String> = match hr {
                    Ok(()) => Ok(result),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.try_send(payload);
                Ok(())
            }));
        // SAFETY: 只把方法名与参数交给 WebView2；结果异步送达，不阻塞调用方。
        if let Err(e) =
            unsafe { core.CallDevToolsProtocolMethod(&method, &params_hstring, &handler) }
        {
            let _ = tx_fallback.try_send(Err(e.to_string()));
        }
    });

    if let Err(e) = dispatched {
        return Err(format!("webview 已不可用：{e}"));
    }

    match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(Ok(text))) => Ok(serde_json::from_str(&text).map_err(|e| {
            format!(
                "CDP 返回的不是合法 JSON：{e}（前 200 字：{}）",
                text.chars().take(200).collect::<String>()
            )
        })?),
        Ok(Some(Err(detail))) => Err(detail),
        Ok(None) => Err("结果通道已关闭（webview 被销毁）".to_string()),
        Err(_) => Err(format!(
            "CDP 求值在 {}ms 内没有回应（webview 可能已被销毁）",
            timeout.as_millis()
        )),
    }
}

/// 非 Windows 平台：实验不成立，明确报错而不是假装成功。
#[cfg(not(windows))]
pub(crate) async fn cdp_eval(
    _webview: &tauri::WebviewWindow,
    _expression: &str,
    _await_promise: bool,
    _timeout: Duration,
) -> Result<Value, String> {
    Err("该平台尚未做 CDP 求值实验".to_string())
}

/// 从 CDP 响应里取出页面结果，并保留异常信息。
///
/// CDP `Runtime.evaluate` 的响应体形如：
/// `{"result": {"type": "object", "value": {...}}, "exceptionDetails": {...}}`
/// —— `exceptionDetails` 与 `result` **同级**，不在 `result` 里面。
pub(crate) fn unwrap_cdp_result(envelope: &Value) -> (Option<Value>, Option<String>) {
    let value = envelope.get("result").and_then(|r| r.get("value")).cloned();
    let exception = envelope.get("exceptionDetails").map(|d| {
        let text = d
            .get("exception")
            .and_then(|e| e.get("description"))
            .or_else(|| d.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("（CDP 未给出异常描述）");
        text.chars().take(200).collect::<String>()
    });
    (value, exception)
}
