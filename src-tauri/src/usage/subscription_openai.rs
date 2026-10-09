//! OpenAI（ChatGPT 套餐）用量：SIWC OAuth 凭据 → 第一方额度接口 → 5h / 7d 层。
//!
//! 与 MiniMax / 智谱那一档 provider 的差别有三点，都落在本文件里：
//!
//! - **凭据不是内核模型凭据**，而是壳自己那份 Sign-in-with-ChatGPT OAuth
//!   （`crate::openai` 的加密 vault）。因此 `configured` 的判据是
//!   「已登录 **且** 拿得到账号 id」，不是 env / `.credentials.yaml`。
//!   外壳仍然不收集、不存储任何新凭据——用的是用户自己已经完成的那次授权。
//! - **必须带账号上下文**：端点要求 `ChatGPT-Account-Id` 请求头，值来自
//!   登录时从 ID token 取下的 `…/auth.chatgpt_account_id` 声明
//!   （`openai::auth::CHATGPT_ACCOUNT_ID_CLAIM`）。它**不在 access token
//!   上**——访问令牌受众是 `api.openai.com/v1`，不含账号上下文。
//! - **必须走代理路由**：这是本仓第一个境外 provider。国内网络上
//!   `chatgpt.com` 的直连解析可能不可用（2026-10-09 实测本机即如此，
//!   DNS 解析被污染），而 shell 是 GUI 程序、继承不到命令行里为 shell 设的
//!   `HTTPS_PROXY`，因此复用 `pkg::net_proxy::routes()`（src-tauri/AGENTS.md
//!   「Rust 侧出网必须走 `net_proxy::routes()`」）。
//!
//! ## 窗口分类为什么不能按槽位
//!
//! 接口把两个窗口放在 `primary_window` / `secondary_window`，但**槽位不
//! 恒定**：真实账户上观察到「7d 窗口跑在 primary 槽、secondary 为
//! `null`」的返回（2026-10-09 实测）。因此这里只认每个窗口自带的
//! `limit_window_seconds`（`18000` = 5h、`604800` = 7d），识别不出的窗口
//! 直接不呈现——**不按槽位猜、也不当成 100%**，前端因此会显示「暂无数据」。

use std::sync::OnceLock;
use std::time::Duration;

use super::credentials;
use super::subscription::{self, CacheTier};

/// 第一方额度接口。端点与响应形状来自 ChatGPT 第一方 Web 客户端，
/// 未经公开文档承诺，故按「逐字段防御式解析」对待（与另两个 provider 同）。
const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

/// 5 小时窗口的 `limit_window_seconds`。
const WINDOW_5H_SECS: u64 = 18000;
/// 7 天窗口的 `limit_window_seconds`。
const WINDOW_7D_SECS: u64 = 604_800;

const USER_AGENT: &str = concat!("dsh-xlink/", env!("CARGO_PKG_VERSION"));

/// 单次查询超时，与另几个 provider 同档。
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// 响应体上限（正常响应几 KB）。
const MAX_HTTP_BODY_BYTES: u64 = 1024 * 1024;

/// 查询失败的三种分类，与 `subscription::FetchOutcome` 一一对应。
pub(crate) enum FetchProblem {
    /// 瞬时失败（网络 / 超时 / 读体中断）：缓存不写不删，keep-last-good。
    Transient(String),
    /// 凭据被拒（HTTP 401/403）：确定性失败，标记凭据失效。
    Rejected(String),
    /// 结构不认识：调用方把响应摘要写日志，便于定位改版。携带响应体是
    /// 为了让那条日志真的能定位改版（空摘要等于没记）。
    Unrecognized(String),
}

/// 本 provider 要用的凭据。**只在内存里活到请求结束**，不进缓存 / 日志 /
/// UI（与另几个 provider 同一纪律）。
#[derive(Clone)]
pub(crate) struct OpenAiUsage {
    pub(crate) access_token: String,
    pub(crate) account_id: String,
}

/// 解析当前活跃账号的凭据，供 `subscription.rs` 判「是否已配置」。
///
/// `None` = 尚未登录 / 拿不到账号 id，调用方据此 `configured = false` 让
/// 前端隐藏分区，而不是拿空值去打接口。
pub(crate) fn resolved_credential() -> Option<credentials::ResolvedCredential> {
    let usage = active_credentials().ok().flatten()?;
    Some(credentials::ResolvedCredential {
        reference: "openai-oauth".to_string(),
        // 指纹绑定到访问令牌：换账号即换令牌，缓存随之作废——与其它
        // provider 同一套绑定语义，不必为账号 id 另设一套。
        value: Some(usage.access_token),
        source: credentials::CredentialSource::OpenAiVault,
    })
}

/// 取当前活跃账号的凭据。**vault 文件不存在就直接返回**：没有它就没有账号
/// 条目可读，也就没有理由去碰系统钥匙串——单测在临时 home 下跑到这里时，
/// 钥匙串那半条路是绝不能碰的（`vault.rs` 的测试纪律）。
///
/// 缺 `chatgpt_account_id`（登录早于落库该字段的版本）同样按未配置处理：
/// 分区隐藏，重新登录一次即可补上，不拿空账号 id 去打接口。
pub(crate) fn active_credentials() -> Result<Option<OpenAiUsage>, String> {
    let paths = crate::openai::flow::shell_flow_paths();
    if !paths.vault_file.is_file() {
        return Ok(None);
    }
    let transport = crate::openai::flow::ProductionTransport { open_browser: None };
    let mode = crate::shell::settings::current_mode().as_str().to_string();
    let Some(tokens) = crate::openai::refresh::ensure_fresh_access(&paths, &transport, &mode)
        .map_err(|error| error.message())?
    else {
        return Ok(None);
    };
    match tokens.chatgpt_account_id {
        Some(account_id) if !account_id.trim().is_empty() => Ok(Some(OpenAiUsage {
            access_token: tokens.access_token,
            account_id,
        })),
        _ => Ok(None),
    }
}

/// 查一次额度窗口，返回 provider 数据。凭据只在 HTTPS 请求头这一处出现。
pub(crate) fn fetch_active(now_ms: u64) -> subscription::FetchOutcome {
    use subscription::{FetchOutcome, ProviderData};
    let Some(usage) = active_credentials().ok().flatten() else {
        return FetchOutcome::Deterministic(
            "未在工作台登录 OpenAI 账户，或该登录缺少账号信息。请重新登录后刷新".into(),
            false,
        );
    };
    match fetch_tiers(&usage, now_ms) {
        Ok(tiers) => FetchOutcome::Success(ProviderData::Plan { tiers }),
        Err(FetchProblem::Transient(message)) => FetchOutcome::Transient(message),
        Err(FetchProblem::Rejected(message)) => FetchOutcome::Deterministic(message, true),
        Err(FetchProblem::Unrecognized(body)) => {
            subscription::log_unrecognized_structure(subscription::PROVIDER_OPENAI, &body);
            FetchOutcome::Deterministic(subscription::UNRECOGNIZED_STRUCTURE.to_string(), false)
        }
    }
}

/// 查询额度窗口。
pub(crate) fn fetch_tiers(
    credential: &OpenAiUsage,
    now_ms: u64,
) -> Result<Vec<CacheTier>, FetchProblem> {
    // 先代理、失败再直连（net_proxy 恒以直连收尾）。**只有传输层失败才换
    // 路由**：401 与「结构不认识」换一条路只会同样地失败；若让它们继续往下
    // 走，分类会随路由顺序漂移——凭据失效被后续路由的传输错误盖住，报成
    // 「网络不可达」，这一轮也不会置 expired。
    let mut last_transport_error: Option<FetchProblem> = None;
    for route in crate::pkg::net_proxy::routes() {
        match call_route(&route, credential) {
            Ok(text) => {
                return match parse_tiers(&text, now_ms) {
                    Ok(tiers) if !tiers.is_empty() => Ok(tiers),
                    _ => Err(FetchProblem::Unrecognized(text)),
                }
            }
            Err(problem @ FetchProblem::Transient(_)) => last_transport_error = Some(problem),
            Err(problem) => return Err(problem),
        }
    }
    // 所有路由都传输失败：报最后一条，keep-last-good。
    Err(last_transport_error.unwrap_or(FetchProblem::Transient(
        "查询失败（没有可用的出网路径）。已保留上次结果，可点击刷新重试".into(),
    )))
}

/// 走一条路由发一次请求。
fn call_route(
    route: &crate::pkg::net_proxy::Route,
    credential: &OpenAiUsage,
) -> Result<String, FetchProblem> {
    let Some(agent) = agent_for(route) else {
        return Err(FetchProblem::Transient(format!(
            "代理地址无法解析（{}）。已跳过该路由，可点击刷新重试",
            route.describe()
        )));
    };
    // Authorization / ChatGPT-Account-Id 是原始凭据的合法去处（另一处是
    // vault 的只读解密）。任何错误信息、日志、缓存都不得携带它们。
    let request = agent
        .get(USAGE_ENDPOINT)
        .header(
            "Authorization",
            &format!("Bearer {}", credential.access_token),
        )
        .header("ChatGPT-Account-Id", &credential.account_id)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    match request.call() {
        Ok(mut response) => response
            .body_mut()
            .with_config()
            .limit(MAX_HTTP_BODY_BYTES)
            .read_to_string()
            .map_err(|error| {
                FetchProblem::Transient(format!(
                    "读取响应失败（{error}）。已保留上次结果，可点击刷新重试"
                ))
            }),
        Err(ureq::Error::StatusCode(status)) if status == 401 || status == 403 => {
            Err(FetchProblem::Rejected(format!(
                "OpenAI 登录已失效（HTTP {status}）。请在工作台重新登录 OpenAI 账户"
            )))
        }
        Err(ureq::Error::StatusCode(status)) => Err(FetchProblem::Transient(format!(
            "OpenAI 用量接口返回 HTTP {status}。已保留上次结果，稍后可重试"
        ))),
        Err(error) => Err(FetchProblem::Transient(format!(
            "查询失败（网络不可达或超时：{error}）。已保留上次结果，可点击刷新重试"
        ))),
    }
}

/// 按路由建 agent 并缓存。`Direct` 显式 `.proxy(None)`：ureq 的默认配置会
/// 读环境变量里的代理，不显式关掉就等于「直连」其实又走了 `HTTPS_PROXY`
/// （与 `pkg::updater` 同一条纪律）。
fn agent_for(route: &crate::pkg::net_proxy::Route) -> Option<ureq::Agent> {
    static AGENTS: OnceLock<std::sync::Mutex<Vec<(String, ureq::Agent)>>> = OnceLock::new();
    let builder = || ureq::Agent::config_builder().timeout_global(Some(HTTP_TIMEOUT));
    let cache = AGENTS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    // Direct 也进这张表：它与任何代理地址都不同键，而 `proxy(None)` 的结果
    // 同样只值得建一次。返回**拥有值**——借出 Vec 元素的引用会随别的线程
    // push 触发重分配而悬空。
    let key = route.describe();
    if let Some(index) = cache.iter().position(|(seen, _)| *seen == key) {
        return cache.get(index).map(|(_, agent)| agent.clone());
    }
    let agent = match route {
        crate::pkg::net_proxy::Route::Direct => builder().proxy(None).build().new_agent(),
        crate::pkg::net_proxy::Route::Proxy { url, .. } => {
            let proxy = ureq::Proxy::new(url.as_str()).ok()?;
            builder().proxy(Some(proxy)).build().new_agent()
        }
    };
    cache.push((key, agent.clone()));
    Some(agent)
}

/// 解析额度窗口 → tier 列表。逐字段防御式：字段缺失或类型不合就跳过该字段，
/// 不做整体失败（端点是第一方但未文档化，字段可能随时漂移）。
///
/// 返回空列表表示「结构不认识」，由调用方落日志。
pub(crate) fn parse_tiers(body: &str, now_ms: u64) -> Result<Vec<CacheTier>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("响应不是有效 JSON：{error}"))?;
    let Some(rate_limit) = value.get("rate_limit") else {
        return Err("响应缺 rate_limit".into());
    };
    let mut tiers = Vec::new();
    for field in ["primary_window", "secondary_window"] {
        let Some(window) = rate_limit.get(field) else {
            continue;
        };
        // 认窗口靠它自带的时长，不靠它落在哪个槽位（见模块头）。
        let Some(seconds) = number_field(window, "limit_window_seconds") else {
            continue;
        };
        let name = match seconds.round() as u64 {
            WINDOW_5H_SECS => "5h",
            WINDOW_7D_SECS => "7d",
            // 认不出的窗口（如某天新增的时长）不呈现，更不按槽位硬套成 5h/7d。
            _ => continue,
        };
        let Some(used) = number_field(window, "used_percent") else {
            continue;
        };
        tiers.push(CacheTier {
            name: name.to_string(),
            // 接口给的是**已用**百分比，缓存字段统一存剩余。
            remaining_percent: (100.0 - used).clamp(0.0, 100.0),
            resets_at_ms: reset_at_ms(window, now_ms),
            unlimited: false,
        });
    }
    // 展示顺序固定为 5h 在前、7d 在后，**与它们落在哪个槽位无关**：
    // 槽位会变（周窗口跑到 primary 是真机见过的），顺序跟着变会让同一份
    // 数据在两次刷新之间上下跳行。
    tiers.sort_by_key(|tier| if tier.name == "5h" { 0u8 } else { 1u8 });
    Ok(tiers)
}

/// 窗口的重置时刻（epoch 毫秒）：优先绝对时刻 `reset_at`，其次相对秒数
/// `reset_after_seconds`。两个都缺就留空（前端不画倒计时，而不是画 0）。
fn reset_at_ms(window: &serde_json::Value, now_ms: u64) -> Option<u64> {
    if let Some(seconds) = number_field(window, "reset_at") {
        if seconds.is_finite() && seconds > 0.0 {
            return Some((seconds * 1000.0).round() as u64);
        }
    }
    let seconds = number_field(window, "reset_after_seconds")?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(now_ms + (seconds * 1000.0).round() as u64)
}

/// 取一个有限数值字段。类型不合 / 缺失 / NaN 一律当作「没有」。
fn number_field(value: &serde_json::Value, field: &str) -> Option<f64> {
    value
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .filter(|number| number.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> Result<Vec<CacheTier>, String> {
        parse_tiers(body, 1_000_000)
    }

    #[test]
    fn reads_both_windows_and_converts_to_remaining() {
        let tiers = parse(
            r#"{"rate_limit":{
                 "primary_window":{"used_percent":25,"limit_window_seconds":18000,"reset_at":1791560754},
                 "secondary_window":{"used_percent":60,"limit_window_seconds":604800,"reset_at":1792000000}}}"#,
        )
        .expect("结构可识别");
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, "5h");
        assert_eq!(tiers[0].remaining_percent, 75.0);
        assert_eq!(tiers[0].resets_at_ms, Some(1_791_560_754_000));
        assert_eq!(tiers[1].name, "7d");
        assert_eq!(tiers[1].remaining_percent, 40.0);
        assert_eq!(tiers[1].resets_at_ms, Some(1_792_000_000_000));
    }

    /// 真机返回过的形状：7 天窗口跑在 primary 槽、secondary 为 null。
    /// 按槽位认会把这一行标成 5h 并丢掉 7d —— 这是本模块存在的理由。
    #[test]
    fn classifies_by_duration_not_by_slot() {
        let tiers = parse(
            r#"{"rate_limit":{
                 "primary_window":{"used_percent":27,"limit_window_seconds":604800,"reset_at":1792147135},
                 "secondary_window":null}}"#,
        )
        .expect("结构可识别");
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "7d");
        assert_eq!(tiers[0].remaining_percent, 73.0);
    }

    #[test]
    fn slots_may_be_swapped_and_still_classify() {
        let tiers = parse(
            r#"{"rate_limit":{
                 "primary_window":{"used_percent":60,"limit_window_seconds":604800},
                 "secondary_window":{"used_percent":25,"limit_window_seconds":18000}}}"#,
        )
        .expect("结构可识别");
        assert_eq!(tiers.len(), 2);
        // 槽位反了，展示顺序仍应是 5h 在前。
        assert_eq!(tiers[0].name, "5h");
        assert_eq!(tiers[1].name, "7d");
    }

    /// 认不出的窗口不呈现，更不能按槽位硬套。
    #[test]
    fn unknown_duration_is_not_guessed() {
        let tiers = parse(
            r#"{"rate_limit":{"primary_window":{"used_percent":5,"limit_window_seconds":999}}}"#,
        )
        .expect("结构可识别");
        assert!(tiers.is_empty(), "认不出的窗口不得被当成 5h/7d");
    }

    #[test]
    fn missing_used_percent_drops_only_that_window() {
        let tiers = parse(
            r#"{"rate_limit":{
                 "primary_window":{"limit_window_seconds":18000},
                 "secondary_window":{"used_percent":10,"limit_window_seconds":604800}}}"#,
        )
        .expect("结构可识别");
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "7d");
    }

    #[test]
    fn clamps_out_of_range_percentages() {
        let tiers = parse(
            r#"{"rate_limit":{"primary_window":{"used_percent":140,"limit_window_seconds":18000}}}"#,
        )
        .expect("结构可识别");
        assert_eq!(tiers[0].remaining_percent, 0.0);
    }

    #[test]
    fn relative_reset_is_measured_from_now() {
        let tiers = parse_tiers(
            r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":18000,"reset_after_seconds":3600}}}"#,
            1_000_000,
        )
        .expect("结构可识别");
        assert_eq!(tiers[0].resets_at_ms, Some(1_000_000 + 3_600_000));
    }

    #[test]
    fn absent_reset_time_stays_absent() {
        let tiers = parse(
            r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":18000}}}"#,
        )
        .expect("结构可识别");
        assert_eq!(tiers[0].resets_at_ms, None, "没有重置时间就不画倒计时");
    }

    #[test]
    fn non_numeric_fields_are_skipped_not_fatal() {
        let tiers = parse(
            r#"{"rate_limit":{"primary_window":{"used_percent":"25","limit_window_seconds":18000},
                              "secondary_window":{"used_percent":60,"limit_window_seconds":"604800"}}}"#,
        )
        .expect("结构本身可识别");
        assert!(tiers.is_empty());
    }

    #[test]
    fn unrecognized_shapes_are_reported() {
        assert!(parse(r#"{"nope":1}"#).is_err());
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"rate_limit":{}}"#).expect("空窗口").is_empty());
    }

    #[test]
    fn real_response_shape_is_understood() {
        // 2026-10-09 真机返回的精简版（含 additional_rate_limits 与其余
        // 无关字段）：主窗口两个都要，附加桶不参与本 provider 展示。
        let tiers = parse(
            r#"{"user_id":"u","account_id":"a","email":"e","plan_type":"plus",
                 "rate_limit":{"allowed":false,"limit_reached":true,
                   "primary_window":{"used_percent":100,"limit_window_seconds":18000,
                     "reset_after_seconds":4264,"reset_at":1791560754},
                   "secondary_window":{"used_percent":82,"limit_window_seconds":604800,
                     "reset_after_seconds":396145,"reset_at":1791952635}},
                 "additional_rate_limits":[{"limit_name":"gpt-reserve","metered_feature":"base_model_inference",
                   "rate_limit":{"allowed":true,"limit_reached":false,
                     "primary_window":{"used_percent":27,"limit_window_seconds":604800,"reset_at":1792147135},
                     "secondary_window":null}}],
                 "credits":{"has_credits":false,"unlimited":false,"balance":"0"},
                 "spend_control":{"reached":false}}"#,
        )
        .expect("真机形状可识别");
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, "5h");
        assert_eq!(tiers[0].remaining_percent, 0.0);
        assert_eq!(tiers[1].name, "7d");
        assert_eq!(tiers[1].remaining_percent, 18.0);
    }
}
