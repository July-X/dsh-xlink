//! Windows 上的**零痕迹**页内求值：DevTools 协议的 `Runtime.evaluate`。
//!
//! # 为什么不是槽位
//!
//! `script_eval.rs` 那套「页内放一个不可枚举、随机名的槽位」的方案**已被裁定
//! 不得直接带进真实站点**：`Object.getOwnPropertyNames(window)` 能列出不可
//! 枚举的属性，随机名只降低可发现性、不消除可发现性，与
//! `harness/chat-fingerprint.js`「页面探针必须看到普通浏览器里什么都没有」
//! 这条纪律冲突。
//!
//! DevTools 协议求值在页面上下文执行，但**不创建任何全局量**——本机实测
//! （2026-10-20，探针 `--mode cdp`）：求值前后
//! `Object.getOwnPropertyNames(window).length` 均为 1239，`__dsh*` 前缀的
//! 全局量为 0。
//!
//! # 接口形态（与「`ICoreWebView2DevToolsProtocolController`」的说法有出入）
//!
//! WebView2 **没有** `ICoreWebView2DevToolsProtocolController` 这个接口。
//! `CallDevToolsProtocolMethod` 是 **`ICoreWebView2` 自己的方法**
//! （`webview2-com-sys 0.39.1` `src/bindings.rs:1455` 与 `:2089`），
//! 完成回调是 `ICoreWebView2CallDevToolsProtocolMethodCompletedHandler`
//! （`:4861`，`Invoke(HRESULT, PCWSTR)`）。
//!
//! # 未开 DevTools 窗口能不能调用
//!
//! **能**——本机实测：整个进程从头到尾没有打开过 DevTools 窗口
//! （`devtools_window_opened: false`），`Runtime.evaluate` 照常返回。
//!
//! # awaitPromise 是关键
//!
//! 带 `awaitPromise: true` 时直接拿到 **Promise 结算后的值**
//! （实测 `{"role":"assistant","value":42}`）；不带时拿到的是 **Promise 对象**
//! 的序列化（实测 `{}`）——与 `script_eval.rs` 那条「原生求值不等 Promise」
//! 的硬事实一致。

use std::time::Duration;

use serde_json::Value;

use super::script_eval::RoundTripError;

/// 用 DevTools 协议求值，返回**页面结果**（已解包 CDP 的外层信封）。
///
/// 返回的是 CDP 响应体：`{"result": {"type": ..., "value": ...}}`，
/// 异常在**与 `result` 同级**的 `exceptionDetails` 里。
#[cfg(windows)]
pub(crate) async fn cdp_eval(
    webview: &tauri::WebviewWindow,
    expression: &str,
    timeout: Duration,
) -> Result<Value, RoundTripError> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::HSTRING;

    let params = serde_json::json!({
        "expression": expression,
        "awaitPromise": true,
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
        return Err(RoundTripError::Gone {
            detail: format!("webview 已不可用：{e}"),
        });
    }

    match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(Ok(text))) => serde_json::from_str(&text).map_err(|e| RoundTripError::Transport {
            detail: format!(
                "CDP 返回的不是合法 JSON：{e}（前 200 字：{}）",
                text.chars().take(200).collect::<String>()
            ),
        }),
        Ok(Some(Err(detail))) => Err(RoundTripError::Transport { detail }),
        Ok(None) => Err(RoundTripError::Gone {
            detail: "结果通道已关闭（webview 被销毁）".to_string(),
        }),
        Err(_) => Err(RoundTripError::Gone {
            detail: format!(
                "CDP 求值在 {}ms 内没有回应（webview 可能已被销毁，事件循环没有处理这条消息）",
                timeout.as_millis()
            ),
        }),
    }
}

/// macOS：**暂无可用通路**。
///
/// `WKWebView.callAsyncJavaScript` 才带 `awaitPromise` 语义
/// （`evaluateJavaScript` 不等 Promise），但 wry 0.57 **没有暴露**它——只有
/// `evaluateJavaScript_completionHandler`（`wry-0.57.0/src/wkwebview/mod.rs:759`
/// 与 `:772`），且 raw `WKWebView` 字段是 `pub(crate)`
/// （`wry-0.57.0/src/wkwebview/mod.rs:135`），外部拿不到。
///
/// 因此 macOS 侧当前**只能做同步读取**；异步读取要么等上游 wry 暴露
/// `callAsyncJavaScript`，要么改走本地回环中继。这里明确报错，不静默降级。
#[cfg(not(windows))]
pub(crate) async fn cdp_eval(
    _webview: &tauri::WebviewWindow,
    _expression: &str,
    _timeout: Duration,
) -> Result<Value, RoundTripError> {
    Err(RoundTripError::Transport {
        detail: "macOS 尚无「等 Promise」的页内求值通路（wry 0.57 未暴露 callAsyncJavaScript）；请改用本地回环中继或等待上游支持".to_string(),
    })
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
