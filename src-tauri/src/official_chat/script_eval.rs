//! 官方对话回程：把页面里「跑完之后」的结果拿回宿主。
//!
//! # 三条硬事实（都是实测/读源码得来的，不是推断）
//!
//! 1. **Tauri 2.12 的 `WebviewWindow::eval_with_callback` 在 Windows 上拿不到值。**
//!    wry 0.57 的 `evaluate_script_with_callback` 走的是
//!    `ICoreWebView2::ExecuteScript`（`wry-0.57.0/src/webview2/mod.rs:1386-1404`），
//!    它的完成回调**只带 HRESULT**。要拿到值，Windows 上必须自己接
//!    `ICoreWebView2_21::ExecuteScriptWithResult`（在 webview2-com 0.39.1 里
//!    这个方法挂在 `ICoreWebView2_21` 上，不是常被引用的 `ICoreWebView2_6`）。
//! 2. **macOS 上 wry 走 `WKWebView.evaluateJavaScript`**
//!    （`wry-0.57.0/src/wkwebview/mod.rs:723-780`），回调里给的是结果的
//!    **JSON 序列化**（`NSJSONSerialization`，`FragmentsAllowed`）。
//! 3. **两端的原生求值都「不等 Promise」。** 拿到的是 Promise 对象本身的
//!    JSON 序列化（`{}`），不是它 resolve 出来的值。要拿到「完成后的值」，
//!    只能让**脚本自己 await**，把结果落到一个宿主能再读到的地方，宿主
//!    再轮询取值。
//! 4. **Windows 的 `TryGetResultAsString` 只在结果是字符串时才给值。**
//!    `ICoreWebView2ScriptResult::TryGetResultAsString` 的契约是：结果不是
//!    字符串时返回**空串**、`hasValue` 为 false——数字、布尔、对象一律读不
//!    回来（本机实测：`1 + 1`、`true`、对象都拿到空串，字符串才拿得到）。
//!    所以页面侧必须**自己 `JSON.stringify`**，把结果变成字符串再交给宿主。
//!    本模块的契约因此是：页面脚本返回**值的 JSON 文本**，平台层只负责把
//!    那段文本原样取回来，解析放在共享层。
//!
//! # 纪律
//!
//! 官网页面**故意**删除 `window.__TAURI__` / `__TAURI_INTERNALS__` 等全局量
//! （`harness/chat-fingerprint.js`），本模块**不恢复**那些桥：它只短暂放一个
//! **不可枚举、随机名**的槽位，取到值立刻删除。即便如此，这仍是一处页面内
//! 痕迹——正式接站点前需要协调者裁定：接受这种短命痕迹，还是改走本地中继
//! （R2：页面把结果 POST 到宿主回环端口，页面内不留任何东西；代价是引入
//! 一个监听端口，且要看站点的 `connect-src` 是否放行 `127.0.0.1`）。
//!
//! # 不阻塞
//!
//! 页面回调发生在事件循环线程上。本模块**不持锁等待页面回调**，也**不在回调
//! 里做阻塞工作**：原生调用只负责「发起」，结果通过通道交给等待方。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

/// 回程的默认超时。站点接口正常在秒级返回，这个上限只是兜底。
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// 单次读回的超时。`with_webview` 是**投递到事件循环**的消息：webview 已经
/// 被销毁时那条消息不会被处理，回调永远不会来。没有这一层，等待方会一直
/// 挂到总超时才返回——总超时是「本次请求的上限」，单次超时才是「这一轮
/// 读回是否还有希望」的判断。
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// 轮询间隔。每轮是一次跨线程的原生求值往返，间隔取「够快」与「不打满
/// 事件循环」之间的折中。
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(40);

/// 页面侧脚本跑起来之前的宽限期。`eval` 是投递到事件循环的异步调用，脚本
/// 未必已经执行；这段时间读不到槽位**不能**判成「页面没了」。
pub(crate) const STARTUP_GRACE: Duration = Duration::from_millis(1500);

/// 页面没有值时返回的哨兵。
///
/// Windows 的 `TryGetResultAsString` 只在结果是字符串时才给值，所以页面侧
/// 必须自己把值 `JSON.stringify` 成文本；`undefined` 没有 JSON 文本可写，
/// 用这个哨兵顶上。哨兵带引号（`json_string`），与「值恰好是这个字符串」
/// 的情况区分得开（后者会被 JSON 编码成 `"\"__DSH_UNDEFINED__\""`）。
const UNDEFINED_SENTINEL: &str = "__DSH_UNDEFINED__";

/// 页面表达式抛错时返回的哨兵前缀，后面跟异常信息。
const ERROR_SENTINEL: &str = "__DSH_ERROR__:";

/// 回程失败的原因分类。每一类都要能直接说给人听。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RoundTripError {
    /// 超时：页面里的 Promise 没有在期限内 settle。
    Timeout { waited_ms: u64, timeout_ms: u64 },
    /// 页面里的 Promise 明确 reject 了。
    Rejected { message: String },
    /// 页面在途中没了（导航 / 重载 / webview 被关）。
    Gone { detail: String },
    /// 回程通道本身失败（脚本没注进去、原生调用报错、结果不是 JSON）。
    Transport { detail: String },
}

impl std::fmt::Display for RoundTripError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoundTripError::Timeout {
                waited_ms,
                timeout_ms,
            } => write!(
                f,
                "页内请求在 {timeout_ms}ms 内没有结束（已等 {waited_ms}ms）：页面里的 Promise 一直 pending。可能是站点接口挂住、请求被风控拦下，或表达式本身不会 settle。"
            ),
            RoundTripError::Rejected { message } => {
                write!(f, "页内请求被拒绝：{message}")
            }
            RoundTripError::Gone { detail } => {
                write!(f, "页面在请求途中不在了：{detail}。请确认页签是否被切换、页面是否跳转，或窗口是否已关闭，然后重试。")
            }
            RoundTripError::Transport { detail } => {
                write!(f, "回程通道失败：{detail}")
            }
        }
    }
}

impl std::error::Error for RoundTripError {}

/// 一次往返的结果。`value` 是**完成后**的值，不是 Promise 对象。
#[derive(Debug, Clone)]
pub(crate) struct RoundTrip {
    /// 页面里那个表达式 settle 之后的值。
    pub value: Value,
    /// 从注入到拿到结果的总耗时。
    pub elapsed_ms: u64,
    /// 读回轮数。用于区分「很快就拿到」与「等了很久才拿到」。
    pub polls: u32,
}

/// 在页面里执行 `expression`（可以是返回 Promise 的表达式），等它 settle，
/// 把**完成后的值**取回宿主。
///
/// `expression` 是壳自己拼的代码（站点适配器提供），不是用户输入；它会被
/// 原样嵌进注入脚本。
pub(crate) async fn round_trip(
    webview: &tauri::WebviewWindow,
    expression: &str,
    timeout: Duration,
) -> Result<RoundTrip, RoundTripError> {
    round_trip_with(webview, expression, timeout, ReadPolicy::FailFast).await
}

/// **读回策略**（`round_trip` 与「容错那一档」**共用同一个实现**）。
///
/// ⚠️ **为什么要策略化而不是各写一份**（PM 2026-10-23 11:3x 拍板）：两份槽位
/// 语义**会漂移**（一份清槽位、一份不清，或清法不同）⇒ ⇒ 「残留判断」在两处
/// 口径不一致 ⇒ ⇒ 后面判「残留归零」时读数不可比 ⇒ ⇒ **一份实现、两种策略**
/// （漂移在**结构上**就不可能）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadPolicy {
    /// **首读失败即整轮失败**（[`round_trip`] 的**现有语义**，一个字不许变）。
    FailFast,
    /// **单次读回失败不放弃**（继续轮询到总预算；耗尽时返回**最后一次**错误）。
    Tolerate,
}

/// **回程的共同实现**（两种策略走**同一段**槽位逻辑）。
///
/// ⚠️ **注入只发生一次**（两种策略都一样）—— 重新注入 ⇒ ⇒ 页内会跑**两个实例**
/// ⇒ ⇒ 两个实例各装一层 ⇒ ⇒ **还原链就错了**（后一个还原成前一个的包装 ⇒ ⇒
/// 残留不归零）。
async fn round_trip_with(
    webview: &tauri::WebviewWindow,
    expression: &str,
    timeout: Duration,
    policy: ReadPolicy,
) -> Result<RoundTrip, RoundTripError> {
    let slot = new_slot_name();
    let script = build_slot_script(&slot, expression);
    let started = Instant::now();

    webview
        .eval(&script)
        .map_err(|e| RoundTripError::Transport {
            detail: format!("注入脚本失败：{e}"),
        })?;

    let mut polls: u32 = 0;
    // 「见过 pending」是区分「脚本还没跑」与「页面被换掉」的唯一依据：
    // 脚本一旦跑起来就会先把槽位置成 pending。
    let mut seen_running = false;
    // ⚠️ **最后一次**读回错误（容错那一档在总预算耗尽时返回它，**不是第一次**）。
    let mut last_error: Option<RoundTripError> = None;

    loop {
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            clear_slot(webview, &slot);
            return Err(match policy {
                // ⚠️ **首读失败即整轮失败**（既有语义）：报**这次**超时。
                ReadPolicy::FailFast => RoundTripError::Timeout {
                    waited_ms: elapsed.as_millis() as u64,
                    timeout_ms: timeout.as_millis() as u64,
                },
                // ⚠️ **容错那一档**：报**最后一次**读回错误（一次都没读成功过 ⇒
                // 报超时）。
                ReadPolicy::Tolerate => last_error.unwrap_or(RoundTripError::Timeout {
                    waited_ms: elapsed.as_millis() as u64,
                    timeout_ms: timeout.as_millis() as u64,
                }),
            });
        }

        let remaining = timeout.saturating_sub(elapsed);
        match read_json(
            webview,
            &slot_expression(&slot),
            remaining.min(READ_TIMEOUT),
        )
        .await
        {
            Ok(value) => {
                // ⚠️ **一次成功的读回** ⇒ ⇒ 之前记下的错误**不代表当下** ⇒ ⇒ 清掉
                //（口径：总预算耗尽时，**只有最后一次读回是失败**才返回那个错误；
                // 最后一次读回是**成功**（哪怕只是 `pending`）⇒ ⇒ 返回 `Timeout`
                // —— 页面在跑、只是没结束，这时给 Timeout 比给一个更早的错误准）。
                last_error = None;
                match value {
                    Value::Object(map) => {
                        polls += 1;
                        match map.get("state").and_then(Value::as_str) {
                            Some("pending") => seen_running = true,
                            Some("done") => {
                                let value = map.get("value").cloned().unwrap_or(Value::Null);
                                clear_slot(webview, &slot);
                                return Ok(RoundTrip {
                                    value,
                                    elapsed_ms: started.elapsed().as_millis() as u64,
                                    polls,
                                });
                            }
                            Some("rejected") => {
                                let message = map
                                    .get("error")
                                    .and_then(Value::as_str)
                                    .unwrap_or("页面脚本拒绝（未给出原因）")
                                    .to_string();
                                clear_slot(webview, &slot);
                                return Err(RoundTripError::Rejected { message });
                            }
                            _ => {
                                // 形状不认识：继续等，但总超时兜底。
                            }
                        }
                    }
                    Value::Null => {
                        polls += 1;
                        if seen_running {
                            clear_slot(webview, &slot);
                            return Err(RoundTripError::Gone {
                                detail: "槽位在请求途中消失，页面文档已被替换（导航或重载）"
                                    .to_string(),
                            });
                        }
                        if started.elapsed() >= STARTUP_GRACE {
                            clear_slot(webview, &slot);
                            return Err(RoundTripError::Gone {
                                detail: "注入脚本始终没有执行（页面可能已导航，或文档已被替换）"
                                    .to_string(),
                            });
                        }
                    }
                    _ => {
                        polls += 1;
                    }
                }
            }
            Err(e) => match policy {
                // ⚠️ **既有语义**：单次读回失败 ⇒ **立刻整轮失败**并清槽位。
                ReadPolicy::FailFast => {
                    clear_slot(webview, &slot);
                    return Err(e);
                }
                // ⚠️ **容错那一档**：单次读回失败**不放弃** ⇒ ⇒ 记下来，**继续
                // 轮询**到总预算（页面正忙时那一次读回会超时）。
                ReadPolicy::Tolerate => {
                    last_error = Some(e);
                }
            },
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
// ==== macOS 专属：读回容错的那一档（形态 C「页内旁听」用）====================
//
// ⚠️ **为什么在 macOS 段里**（PM 2026-10-23 11:3x 拍板）：`script_eval.rs` 的
// **公共段保持单主人（形态 B）**；这一档**只有 macOS 的形态 C 会调** ⇒ ⇒ 放
// 公共段 ⇒ Windows 上没人调用 ⇒ ⇒ 放 macOS 段 ⇒ **Windows 根本不编译它**
// ⇒ ⇒ 零影响（Windows 那一面也不会有「没人调的死代码」）。
//
// ⚠️ **槽位机制仍只有一份**：[`round_trip_with`] 是**公共段**里的**同一个**
// 实现（两种策略走**同一段**逻辑）⇒ ⇒ 「两份槽位语义会漂移」在**结构上**就
// 不可能。

/// **注入只发生一次**、**读回容错**的那一次往返（形态 C「页内旁听」用）。
///
/// ## 与 [`round_trip`] 的差别（**`round_trip` 现有语义一个字不许改**）
///
/// | | [`round_trip`] | 本函数 |
/// | --- | --- | --- |
/// | 注入次数 | 1 | **1**（同） |
/// | 单次读回失败（超时／通道关闭） | **立刻整轮失败**并清槽位 | **记下来，继续轮询**到总预算 |
/// | 总预算耗尽时返回 | 首个错误 | **最后一次**读回错误（不是第一次） |
///
/// ## 为什么形态 C 需要它（**实测依据**）
///
/// 形态 C 的脚本**必须尽早注入**（站点那层 fetch 一出现就得装钩 ⇒ 错过就没有
/// 第二次），而「尽早」＝ **站点 bundle 正在解析**⇒ ⇒ 那段时间**页面主线程被
/// 占住**⇒ ⇒ 宿主那一次读回可能 **2 s 内没有回应**（`READ_TIMEOUT`）。
///
/// ⚠️ **上游实测**：形态 C 探针连跑 13 轮，有 **2 轮**（r7／r8）就是
/// 「求值在 2000ms 内没有回应」⇒ ⇒ 那两轮**整轮失败**（`round_trip` 首读失败
/// 即放弃）⇒ ⇒ 形态 C 的生产链**不能**用那个语义。
///
/// ## 为什么**注入必须只发生一次**
///
/// 重新注入 ⇒ ⇒ **页内会跑两个实例** ⇒ ⇒ 两个实例各装一层 ⇒ ⇒ **还原链就错了**
/// （后一个还原成前一个的包装 ⇒ ⇒ 残留不归零）⇒ ⇒ **注入只发生一次**。
///
/// ## 槽位纪律（与 [`round_trip`] 同形）
///
/// 槽位名随机（`__dsh_rt_<hex>`）、**不可枚举**、**取回即删** ⇒ ⇒ 那是**宿主
/// 那条求值通道**建的（协议 §9 **G4-29**），**不是**页内脚本自己的动作 ⇒ ⇒
/// 形态 C 的「不新增 `window` 自有属性」约束仍然成立（约束的对象是**形态自己
/// 的页内动作**）。
///
/// ## 失败时给什么
///
/// · **槽位说「rejected」** ⇒ `RoundTripError::Rejected`（带页面给的原因）；
/// · **槽位说「done」** ⇒ `Ok(RoundTrip)`（值就是它）；
/// · **总预算耗尽** ⇒ **最后一次**读回错误（一次都没读成功过 ⇒ `Timeout`）；
/// · **槽位在请求途中消失**（文档被换掉）⇒ `Gone`（与 [`round_trip`] 同口径）。
#[cfg(target_os = "macos")]
pub(crate) async fn round_trip_tolerant(
    webview: &tauri::WebviewWindow,
    expression: &str,
    timeout: Duration,
) -> Result<RoundTrip, RoundTripError> {
    round_trip_with(webview, expression, timeout, ReadPolicy::Tolerate).await
}


/// 读回页面里某个表达式的值（JSON）。
///
/// `expression` 必须是一个**同步**表达式：原生求值不等 Promise。
///
/// 平台层取回的是**值的 JSON 文本**（页面侧已 `JSON.stringify`），解析在
/// 这里统一做。
pub(crate) async fn read_json(
    webview: &tauri::WebviewWindow,
    expression: &str,
    read_timeout: Duration,
) -> Result<Value, RoundTripError> {
    let text = read_json_impl(webview, expression, read_timeout).await?;
    parse_script_json(&text)
}

// --- 平台实现 ---------------------------------------------------------------

/// Windows：`ICoreWebView2_21::ExecuteScriptWithResult`。
///
/// 走 `with_webview` 拿到平台 webview（`ICoreWebView2Controller`），再自己
/// 发起求值并挂完成回调。**不持锁、不阻塞事件循环**：原生调用只负责发起，
/// 结果通过通道交给等待方。
///
/// `ExecuteScriptWithResult` 在 webview2-com 0.39.1 里挂在
/// **`ICoreWebView2_21`** 上（`webview2-com-sys-0.39.1/src/bindings.rs:41798`
/// 起的 `impl ICoreWebView2_21`，方法在 41799 行）——不是常被引用的
/// `ICoreWebView2_6`。它要求本机 WebView2 运行时足够新；拿不到时下面会
/// 明确报出来，而不是静默返回空。
///
/// 返回值是**页面结果的 JSON 文本**：求值的脚本已经被 `wrap_expression`
/// 包成「总是返回字符串」，所以这里拿到的一定是文本，不会是空串。
#[cfg(windows)]
async fn read_json_impl(
    webview: &tauri::WebviewWindow,
    expression: &str,
    read_timeout: Duration,
) -> Result<String, RoundTripError> {
    use webview2_com::ExecuteScriptWithResultCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_21;
    use windows_core::{Interface, HSTRING};

    let (tx, mut rx) = tauri::async_runtime::channel(1);
    // 完成回调会拿走 `tx`，所以「发起失败」那条路径要留一份副本。
    let tx_fallback = tx.clone();
    let expression = wrap_expression(expression);

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
        let core21 = match core.cast::<ICoreWebView2_21>() {
            Ok(core) => core,
            Err(e) => {
                let _ = tx.try_send(Err(format!(
                    "本机 WebView2 运行时太旧，拿不到 ICoreWebView2_21（无法取回求值结果）：{e}"
                )));
                return;
            }
        };
        let handler =
            ExecuteScriptWithResultCompletedHandler::create(Box::new(move |hr, result| {
                // `completed_callback` 宏把 HRESULT 转成了 `Result<()>`，所以这里
                // 拿到的是「成功 / 失败」，不是裸 HRESULT。
                let payload: Result<String, String> = if let Err(e) = hr {
                    Err(e.to_string())
                } else {
                    match result {
                        Some(result) => script_result_to_string(&result),
                        None => Ok(String::new()),
                    }
                };
                let _ = tx.try_send(payload);
                Ok(())
            }));
        // SAFETY: `ExecuteScriptWithResult` 只把脚本交给页面求值；求值结果由
        // 完成回调异步送达，不阻塞调用方。
        if let Err(e) =
            unsafe { core21.ExecuteScriptWithResult(&HSTRING::from(expression), &handler) }
        {
            let _ = tx_fallback.try_send(Err(e.to_string()));
        }
    });

    if let Err(e) = dispatched {
        return Err(RoundTripError::Gone {
            detail: format!("webview 已不可用：{e}"),
        });
    }

    match tokio::time::timeout(read_timeout, rx.recv()).await {
        Ok(Some(Ok(text))) => Ok(text),
        Ok(Some(Err(detail))) => Err(RoundTripError::Transport { detail }),
        Ok(None) => Err(RoundTripError::Gone {
            detail: "结果通道已关闭（webview 被销毁）".to_string(),
        }),
        Err(_) => Err(RoundTripError::Gone {
            detail: format!(
                "求值在 {}ms 内没有回应（webview 可能已被销毁，事件循环没有处理这条消息）",
                read_timeout.as_millis()
            ),
        }),
    }
}

/// 把 `ExecuteScriptWithResult` 的结果对象读成字符串。
///
/// WebView2 给的是结果的 **JSON 序列化**：对象是 JSON 文本，`undefined` 是
/// `undefined`，`null` 是 `null`。
#[cfg(windows)]
fn script_result_to_string(
    result: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2ExecuteScriptResult,
) -> Result<String, String> {
    use windows_core::{BOOL, PWSTR};

    let mut text = PWSTR::null();
    let mut has_value = BOOL::default();
    // SAFETY: `TryGetResultAsString` 只写出 PWSTR / BOOL 两个出参；PWSTR 由
    // `take_pwstr` 释放（CoTaskMem），不复制到别处。
    unsafe { result.TryGetResultAsString(&mut text, &mut has_value) }.map_err(|e| e.to_string())?;
    if !has_value.as_bool() {
        return Ok(String::new());
    }
    Ok(webview2_com::take_pwstr(text))
}

/// macOS：`WKWebView.evaluateJavaScript`（经 Tauri 的 `eval_with_callback`）。
///
/// wry 在 macOS 上把结果用 `NSJSONSerialization` 序列化成 JSON 文本交给回调
/// （`wry-0.57.0/src/wkwebview/mod.rs:723-780`）。**它同样不等 Promise**——
/// 要等 Promise 只能靠 `round_trip` 那套「脚本自己 await + 宿主轮询」。
///
/// ## ✅ 本机已验证（2026-10-22 22:0x，macOS 14/15，wry 0.57.0）
///
/// 这一段曾写着「本机未验证（没有 macOS 机器）」——**那句话现在作废**，
/// 实测读数（探针 `official_chat_macos_route_b_probe.rs`）：
///
/// · `read_json` 读回 `String(1+1)` ⇒ `"2"` ✔
/// · `round_trip` 读回 `Promise.resolve('a-ok')` ⇒ `"a-ok"`（1 轮轮询）✔
/// · 隐藏窗口（`visible(false)`）**照常**求值 ✔（生产取数窗就是隐藏的）
/// · `NSJSONSerialization` 给的是**结果的 JSON 序列化** ⇒ 我们的页面脚本
///   返回字符串时，这里要**解开一层** JSON 字符串包装才拿得到里面那段文本 ✔
///   （`unwrap_json_string_layer`）
///
/// ## ⚠️ 首次导航 commit 之前，求值会被**静默排队**（实测）
///
/// wry 的 macOS `eval` 在 `pending_scripts` **还是 `Some`** 时**只把脚本排队**
/// （`wry-0.57.0/src/wkwebview/mod.rs:723-725`），而排队分支里**连 callback
/// 一起丢掉** ⇒ ⇒ 宿主这边回程通道的 sender 没了 ⇒ `rx.recv()` 立刻返回
/// `None` ⇒ 报「**结果通道已关闭**（webview 被销毁）」——**这条报错会骗人**：
/// webview 好好的，是**脚本被排队了**。
///
/// `pending_scripts` **只在首次导航 commit 时置空**
/// （`wry-0.57.0/src/wkwebview/navigation.rs:28-35`）⇒ ⇒ **建窗之后、首次
/// 导航落地之前**的每一次求值都会踩这个坑。实测：建窗后第 1–4 次求值全部
/// 「通道已关闭」，第 5 次才成功（≈1.1s 后首次导航 commit）。
///
/// ⇒ ⇒ **调用方必须先有一次求值真的跑通**，再发正式请求（`round_trip` 自己
/// 的注入也依赖这一点）。`session_structure::capture_via_page` 的「等就绪」
/// 那一格正是这个作用。
#[cfg(target_os = "macos")]
async fn read_json_impl(
    webview: &tauri::WebviewWindow,
    expression: &str,
    read_timeout: Duration,
) -> Result<String, RoundTripError> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let expression = wrap_expression(expression);

    webview
        .eval_with_callback(expression, move |raw| {
            let _ = tx.try_send(Ok(raw));
        })
        .map_err(|e| RoundTripError::Transport {
            detail: format!("页内求值失败：{e}"),
        })?;

    match tokio::time::timeout(read_timeout, rx.recv()).await {
        Ok(Some(Ok(text))) => Ok(unwrap_json_string_layer(&text)),
        Ok(Some(Err(detail))) => Err(RoundTripError::Transport { detail }),
        Ok(None) => Err(RoundTripError::Gone {
            detail: "结果通道已关闭（webview 被销毁）".to_string(),
        }),
        Err(_) => Err(RoundTripError::Gone {
            detail: format!(
                "求值在 {}ms 内没有回应（webview 可能已被销毁，事件循环没有处理这条消息）",
                read_timeout.as_millis()
            ),
        }),
    }
}

/// 其余平台：回程尚未实现，明确报错而不是静默返回空。
#[cfg(not(any(windows, target_os = "macos")))]
async fn read_json_impl(
    _webview: &tauri::WebviewWindow,
    _expression: &str,
    _read_timeout: Duration,
) -> Result<String, RoundTripError> {
    Err(RoundTripError::Transport {
        detail: "该平台尚未实现页内求值回程".to_string(),
    })
}

// --- 页面侧脚本 -------------------------------------------------------------

/// 槽位名。随机 + 进程内自增，避免与页面自己的变量撞名，也避免被猜到。
fn new_slot_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("__dsh_rt_{:x}{:x}", nanos, n)
}

/// 读回槽位用的表达式。
fn slot_expression(slot: &str) -> String {
    format!("window[{}]", json_string(slot))
}

/// 注入脚本：把 `expression` 的**完成后的值**放进槽位。
///
/// 关键点：`Promise.resolve().then(...)` 会同时兜住「表达式本身抛错」与
/// 「表达式返回 Promise」两种情形——前者变成 rejected，后者被 adopt。
fn build_slot_script(slot: &str, expression: &str) -> String {
    format!(
        r#"(function () {{
  var SLOT = {slot};
  function put(state) {{
    try {{
      Object.defineProperty(window, SLOT, {{
        value: state, writable: true, configurable: true, enumerable: false
      }});
    }} catch (e) {{
      try {{ window[SLOT] = state; }} catch (e2) {{ /* 页面不允许写就算了 */ }}
    }}
  }}
  put({{ state: "pending", startedAt: Date.now() }});
  try {{
    Promise.resolve().then(function () {{ return ({expression}); }}).then(
      function (value) {{
        put({{ state: "done", value: (value === undefined ? null : value), finishedAt: Date.now() }});
      }},
      function (error) {{
        var message = "";
        try {{ message = (error && error.message) ? String(error.message) : String(error); }}
        catch (e) {{ message = "无法读取拒绝原因"; }}
        put({{ state: "rejected", error: message, finishedAt: Date.now() }});
      }}
    );
  }} catch (e) {{
    put({{ state: "rejected", error: "表达式抛错: " + String(e), finishedAt: Date.now() }});
  }}
  return true;
}})()"#,
        slot = json_string(slot),
        expression = expression,
    )
}

/// 取走槽位，不留页面痕迹。
fn clear_slot(webview: &tauri::WebviewWindow, slot: &str) {
    let script = format!(
        "try {{ delete window[{}]; }} catch (e) {{ try {{ window[{}] = undefined; }} catch (e2) {{}} }}",
        json_string(slot),
        json_string(slot)
    );
    let _ = webview.eval(script);
}

/// 把一段文本编码成 JSON 字符串字面量（含引号）。
fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// 把求值表达式包成「**总是返回字符串**」的脚本。
///
/// 为什么必须这么包：Windows 的 `TryGetResultAsString` 只在结果是字符串时
/// 才给值，数字 / 布尔 / 对象一律读回来是空串（本机实测）。所以页面侧要
/// 自己把值 `JSON.stringify` 成文本。
///
/// 顺带处理两个边界：
/// - `undefined` 没有 JSON 文本可写 → 返回 [`UNDEFINED_SENTINEL`]。
/// - 表达式抛错 / 结果是循环引用（`JSON.stringify` 也会抛）→ 返回
///   [`ERROR_SENTINEL`] 加原因，宿主据此报「页面表达式抛错」而不是
///   「读回失败」。
fn wrap_expression(expression: &str) -> String {
    format!(
        r#"(function () {{
  try {{
    var v = ({expression});
    if (typeof v === "undefined") return {undefined};
    var t = JSON.stringify(v);
    if (typeof t === "undefined") return {undefined};
    return t;
  }} catch (e) {{
    var m = "";
    try {{ m = (e && e.message) ? String(e.message) : String(e); }}
    catch (e2) {{ m = "无法读取异常信息"; }}
    return {error} + m;
  }}
}})()"#,
        expression = expression,
        undefined = json_string(UNDEFINED_SENTINEL),
        error = json_string(ERROR_SENTINEL),
    )
}

/// 解开一层 JSON 字符串包装（macOS 路径用）。
///
/// `NSJSONSerialization` 把「结果是字符串」这件事也 JSON 编码了一遍，所以
/// 拿到的文本是 `"\"{...}\""` 这种双层形态；解开后才是页面那段的 JSON 文本。
/// 不是 JSON 字符串就原样返回。
fn unwrap_json_string_layer(raw: &str) -> String {
    match serde_json::from_str::<Value>(raw.trim()) {
        Ok(Value::String(inner)) => inner,
        _ => raw.to_string(),
    }
}

/// 解析页面回传的 JSON 文本。
///
/// 页面侧（见 [`wrap_expression`]）已经把值 `JSON.stringify` 过，所以这里
/// 拿到的就是值的 JSON 文本。两个哨兵单独处理：`undefined` 归一成
/// `Value::Null`（「没有值」），表达式抛错归一成传输错误。
fn parse_script_json(raw: &str) -> Result<Value, RoundTripError> {
    let trimmed = raw.trim();
    if trimmed == UNDEFINED_SENTINEL {
        return Ok(Value::Null);
    }
    if let Some(message) = trimmed.strip_prefix(ERROR_SENTINEL) {
        return Err(RoundTripError::Transport {
            detail: format!("页面表达式抛错：{message}"),
        });
    }
    if trimmed.is_empty() || trimmed == "undefined" || trimmed == "null" {
        return Ok(Value::Null);
    }
    serde_json::from_str(trimmed).map_err(|e| RoundTripError::Transport {
        detail: format!(
            "页面返回值不是合法 JSON：{e}（前 120 字：{}）",
            trimmed.chars().take(120).collect::<String>()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_names_are_unique_and_unguessable_shape() {
        let a = new_slot_name();
        let b = new_slot_name();
        assert_ne!(a, b);
        assert!(a.starts_with("__dsh_rt_"));
        // 随机部分（时间戳 + 计数器）必须全是十六进制位——槽位名要能被
        // 当成普通标识符安全地嵌进脚本，也不好被猜到。
        let suffix = a.strip_prefix("__dsh_rt_").expect("槽位名缺少前缀");
        assert!(!suffix.is_empty());
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn injected_script_awaits_the_expression_and_reports_state() {
        let script = build_slot_script("__dsh_rt_1", "window.probeDelayed(10, 42)");
        assert!(script.contains("__dsh_rt_1"));
        assert!(script.contains("window.probeDelayed(10, 42)"));
        // 必须先落 pending，再落 done / rejected
        assert!(script.contains("\"pending\""));
        assert!(script.contains("\"done\""));
        assert!(script.contains("\"rejected\""));
        // 槽位不可枚举
        assert!(script.contains("enumerable: false"));
    }

    #[test]
    fn slot_name_is_json_escaped_into_the_script() {
        let script = build_slot_script("a\"b", "1");
        assert!(script.contains(r#""a\"b""#));
    }

    #[test]
    fn missing_values_normalize_to_null() {
        assert_eq!(parse_script_json("").unwrap(), Value::Null);
        assert_eq!(parse_script_json("  ").unwrap(), Value::Null);
        assert_eq!(parse_script_json("undefined").unwrap(), Value::Null);
        assert_eq!(parse_script_json("null").unwrap(), Value::Null);
    }

    #[test]
    fn structured_values_round_trip_through_json_text() {
        let value = parse_script_json(r#"{"state":"done","value":{"n":42}}"#).unwrap();
        assert_eq!(value["state"], "done");
        assert_eq!(value["value"]["n"], 42);
    }

    #[test]
    fn non_json_output_is_an_error_not_an_empty_value() {
        let err = parse_script_json("not json at all").unwrap_err();
        assert!(matches!(err, RoundTripError::Transport { .. }));
    }

    #[test]
    fn wrapped_expression_always_returns_a_string() {
        // Windows 的 TryGetResultAsString 只给字符串，所以包装里必须有
        // JSON.stringify，并且不能直接把表达式暴露成非字符串结果。
        let script = wrap_expression("1 + 1");
        assert!(script.contains("JSON.stringify(v)"));
        assert!(script.contains("typeof v === \"undefined\""));
        assert!(script.contains("(1 + 1)"));
        assert!(script.contains(UNDEFINED_SENTINEL));
        assert!(script.contains(ERROR_SENTINEL));
    }

    #[test]
    fn undefined_sentinel_normalizes_to_null() {
        assert_eq!(parse_script_json(UNDEFINED_SENTINEL).unwrap(), Value::Null);
    }

    #[test]
    fn error_sentinel_becomes_a_transport_error() {
        let err = parse_script_json(&format!("{ERROR_SENTINEL}boom")).unwrap_err();
        match err {
            RoundTripError::Transport { detail } => assert!(detail.contains("boom")),
            other => panic!("期望 Transport，实际：{other:?}"),
        }
    }

    #[test]
    fn json_string_layer_is_unwrapped_for_macos_path() {
        // NSJSONSerialization 会把「结果是字符串」再 JSON 编码一层。
        assert_eq!(unwrap_json_string_layer(r#""{\"a\":1}""#), r#"{"a":1}"#);
        // 不是 JSON 字符串就原样返回。
        assert_eq!(unwrap_json_string_layer(r#"{"a":1}"#), r#"{"a":1}"#);
    }
}
