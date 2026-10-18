//! 2api.store（TokenShop）中转的**剩余额度**：查询并展示用户自定义 provider
//! 那一把 API Key 的 `/v1/usage`。
//!
//! 端点 `GET <origin>/v1/usage`（Bearer = 该自定义 provider 的 `apiKeyEnv` 引用
//! 出来的 Key，HTTP 那一处由 [`subscription::http_get_json`] 统一处理）。
//! **端点不是常量**：2api.store 在内核模型设置里是**用户自定义 provider**，
//! 模型 Base URL 由用户填（通常形如 `https://2api.store/v1`），用量接口跟着它
//! 走——所以这里比其余六家多一步「从 [`crate::usage::credentials::ProviderBinding`]
//! 的 `baseURL` 推导端点」。凭据同样走内核凭据链：那一行自己写的 `apiKeyEnv`
//! （本机实测是 `Z_2API_API_KEY`），壳不收集、不保存任何 Key。
//!
//! ## 响应（2api.store 官方文档 + 2026-10-18 真机）
//!
//! 钱包余额模式（`mode: "unrestricted"`，2026-10-18 真机返回的字段形状）：
//!
//! ```json
//! {"balance":48.62526778,"mode":"unrestricted","planName":"钱包余额",
//!  "remaining":48.62526778,"unit":"USD","isValid":true,
//!  "usage":{"today":{"cost":15.7605616},"total":{"cost":0.048}}}
//! ```
//!
//! 额度模式（`mode: "quota_limited"`）额外带 `quota`（limit / used / remaining）
//! 与 `rate_limits[]`（5h / 1d / 7d 窗口）；`remaining` / `quota.remaining` 是
//! 当前可用额度。两种模式都按用户给出的 cc-switch 自定义脚本语义取值：
//!
//! - 剩余额度：`remaining` → `quota.remaining` → `balance`（数字或数字字符串）；
//! - 单位：`unit` → `quota.unit` → `"USD"`；
//! - 凭据是否可用：`is_active` → `isValid` → 缺省 `true`。
//!
//! 口径**刻意与「余额」系列一致**（`ProviderData::Balance`，单行金额）：本功能
//! 回答「这个账号还剩多少」，面板按余额类渲染。`rate_limits[]` 的周期窗口本
//! 版本**不透出**（真机这把 Key 是钱包模式，没有窗口可验；而 cc-switch 的自定义
//! 提取器同样只取一个 `remaining`——没有真实样张的窗口语义不在这里猜）。接口
//! 还返回 `usage` / `daily_usage` / `model_stats`（今日与累计消费），同样不展示：
//! 「过去花了多少」是本地用量窗口的事，本卡片只回答「还剩多少」。
//!
//! ## 失败口径
//!
//! - HTTP 401/403：确定性失败，文案指向工作台模型设置里这一行的 Key
//!   （[`subscription::credential_rejected_message`] 的 2api 分支）。
//! - 端点推导不出来（没填 baseURL / 非 HTTPS / 路径形状不认识）：确定性失败，
//!   文案告诉用户 Base URL 该写成什么样，**不发请求**（绝不拿 Key 打别的地址）。
//! - 响应没有 JSON / 没有可用的剩余额度字段：确定性失败 + 截断摘要落 Shell 日志。
//!   旧数据保留（keep-last-good），不画成「0 余额」。

use super::subscription::{self, CacheBalance, FetchOutcome, ProviderData, PROVIDER_API2STORE};

/// 接口没给 `unit` 时的缺省单位：2api.store 的金额口径是 USD。
const DEFAULT_UNIT: &str = "USD";

/// 一次查询要展示的三个读数。
#[derive(Debug)]
pub(crate) struct UsageSnapshot {
    /// 剩余额度（已格式化成展示用字符串，不是原始 JSON 数字）。
    pub remaining: String,
    /// 单位（如 `USD`）；接口没给时是 [`DEFAULT_UNIT`]。
    pub unit: String,
    /// 凭据是否可用（`is_active` / `isValid`，缺省视为可用）。
    pub valid: bool,
}

/// 从模型设置里那个自定义 provider 的 `baseURL` 推导 `/v1/usage` 端点。
///
/// 接受这几种用户写法（2api.store 文档里 Base URL 的口径与 OpenAI 兼容
/// 客户端常见的填法）：
///
/// - `https://2api.store/v1`（内核模型设置的常见形态）→ `…/v1/usage`；
/// - `https://2api.store`（只填 origin）→ `…/v1/usage`；
/// - `https://2api.store/v1/usage`（已经填了完整用量地址）→ 原样使用。
///
/// 国内优化线路只是换域名、路径不变，因此按 baseURL 推导同样适用。以 `/v1`
/// 结尾时允许前面有反向代理前缀（如 `https://host/proxy/v1` →
/// `…/proxy/v1/usage`，因为 2api 的端点路径就是拼在 Base URL 之后）；`/v1`
/// 之后还有段的形状（如 `https://2api.store/v1/openai`）不猜，返回可操作错误。
pub(crate) fn usage_endpoint(base_url: Option<&str>) -> Result<String, String> {
    let Some(raw) = base_url.map(str::trim).filter(|url| !url.is_empty()) else {
        return Err(
            "没有找到这个自定义 provider 的 baseURL（工作台模型设置里该 provider 的 \
             Base URL 为空？），无法推导 2api 的用量端点 /v1/usage。"
                .to_string(),
        );
    };
    let base = raw.trim_end_matches('/');
    let lower = base.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err(format!(
            "自定义 provider 的 Base URL 必须是 HTTPS 地址（当前为「{raw}」），\
             无法查询 2api 用量。请到工作台的模型设置改成 https://2api.store/v1 这种形式。"
        ));
    }
    if lower.ends_with("/v1/usage") {
        return Ok(base.to_string());
    }
    if lower.ends_with("/v1") {
        return Ok(format!("{base}/usage"));
    }
    // 只填了 origin（`https://` 之后没有路径段）时按文档补全 `/v1/usage`。
    let after_scheme = &lower["https://".len()..];
    if !after_scheme.contains('/') {
        return Ok(format!("{base}/v1/usage"));
    }
    Err(format!(
        "无法从这个自定义 provider 的 baseURL（「{base}」）推导 2api 的用量端点。\
         模型 Base URL 应写成 https://2api.store/v1 这种形式（用量接口是 \
         <origin>/v1/usage）；请到工作台的模型设置检查这一行的 Base URL。"
    ))
}

/// 解析 `/v1/usage` 响应（纯函数，样张进单测）。
pub(crate) fn parse_usage(body: &str) -> Result<UsageSnapshot, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        format!(
            "接口返回了无法解析的数据（非 JSON）。{}",
            subscription::UNRECOGNIZED_STRUCTURE
        )
    })?;
    // 提取器语义是 JavaScript 的 `??`：字段存在但值是 null / 非数字时**继续
    // 往后找**，不是就地失败（额度模式可能把用不上的字段显式给成 null）。
    let remaining = [
        value.get("remaining"),
        value.get("quota").and_then(|quota| quota.get("remaining")),
        value.get("balance"),
    ]
    .iter()
    .flatten()
    .find_map(|candidate| amount_string(candidate))
    .ok_or_else(|| {
        "2api 用量接口的响应里找不到剩余额度字段（remaining / quota.remaining / \
         balance）。接口可能已改版，详情见「查看日志」。"
            .to_string()
    })?;
    let unit = [
        value.get("unit"),
        value.get("quota").and_then(|quota| quota.get("unit")),
    ]
    .iter()
    .flatten()
    .filter_map(|candidate| candidate.as_str())
    .map(str::trim)
    .find(|unit| !unit.is_empty())
    .unwrap_or(DEFAULT_UNIT)
    .to_string();
    // 逐字段防御式：只有明确的布尔值才算数，缺字段 / 类型不对按「可用」处理
    // ——宁可多展示一次，也不要把一把好 Key 渲染成停用。
    let valid = value
        .get("is_active")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| value.get("isValid").and_then(serde_json::Value::as_bool))
        .unwrap_or(true);
    Ok(UsageSnapshot {
        remaining,
        unit,
        valid,
    })
}

/// JSON 里可能是数字，也可能是数字字符串（中转站实现不统一）：两种都接受。
fn amount_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(number) => number.as_f64().map(format_amount),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok().map(format_amount),
        _ => None,
    }
}

/// 金额的展示格式：`>= 1` 保留两位（货币习惯）；`< 1` 保留至多 4 / 6 位并裁掉
/// 末尾的零（余额只剩几美分时不把 0.0042 画成 0.00）。接口给的是美元浮点数，
/// 这里只做展示取整，不参与任何计算。
fn format_amount(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1.0 {
        return format!("{value:.2}");
    }
    if magnitude == 0.0 {
        return "0.00".to_string();
    }
    let text = if magnitude >= 0.01 {
        format!("{value:.4}")
    } else {
        format!("{value:.6}")
    };
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "0" || trimmed == "-0" {
        // 极小但非零（|value| < 1e-6）时 6 位小数会裁成 0：再放宽一档，
        // 只有真的裁没了才显示 0.00（现实余额不会到这个量级，纯防御）。
        let fine = format!("{value:.12}");
        let fine = fine.trim_end_matches('0').trim_end_matches('.');
        if fine.is_empty() || fine == "0" || fine == "-0" {
            "0.00".to_string()
        } else {
            fine.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

/// 查一次剩余额度，返回 provider 数据。凭据只在 HTTPS 请求头这一处出现。
pub(crate) fn fetch(key: &str, base_url: Option<&str>) -> FetchOutcome {
    let endpoint = match usage_endpoint(base_url) {
        Ok(endpoint) => endpoint,
        // 端点推导不出来是**确定性失败且不发请求**：先把 Key 留着，等用户
        // 把 Base URL 改对；缓存里的旧数据按 keep-last-good 保留。
        Err(message) => return FetchOutcome::Deterministic(message, false),
    };
    let body = match subscription::http_get_json(PROVIDER_API2STORE, &endpoint, key) {
        Ok(body) => body,
        Err(outcome) => return outcome,
    };
    match parse_usage(&body) {
        Ok(usage) => FetchOutcome::Success(ProviderData::Balance {
            is_available: usage.valid,
            balances: vec![CacheBalance {
                currency: usage.unit,
                total: usage.remaining,
                granted: None,
                topped_up: None,
            }],
        }),
        Err(message) => {
            // 结构不认识：截断摘要落 Shell 日志供反馈，不进错误横幅。
            subscription::log_unrecognized_structure(PROVIDER_API2STORE, &body);
            FetchOutcome::Deterministic(message, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钱包余额模式的样例（字段形状取自 2api.store 官方文档 + 2026-10-18 真机
    /// 响应；金额与消费数字是文档示例 / 虚构值，不包含账号真实数据）。
    const WALLET_SAMPLE: &str = r#"{
        "balance": 149.74,
        "mode": "unrestricted",
        "planName": "钱包余额",
        "remaining": 149.74,
        "unit": "USD",
        "isValid": true,
        "usage": {
            "today": { "requests": 0, "total_tokens": 0, "cost": 0 },
            "total": { "requests": 2, "total_tokens": 19114, "cost": 0.048 }
        },
        "daily_usage": [
            { "date": "2026-08-28", "requests": 2, "total_tokens": 19114, "cost": 0.048 }
        ]
    }"#;

    /// 额度模式的样例（`quota` 与 `rate_limits` 并存；窗口本版本不透出）。
    const QUOTA_SAMPLE: &str = r#"{
        "mode": "quota_limited",
        "isValid": true,
        "status": "active",
        "remaining": 13.5,
        "unit": "USD",
        "quota": { "limit": 20, "used": 6.5, "remaining": 13.5 },
        "rate_limits": [
            { "window": "5h", "limit": 20, "used": 6.5, "remaining": 13.5, "reset_at": "2028-06-01T00:00:00Z" }
        ]
    }"#;

    #[test]
    fn wallet_sample_parses_remaining_and_unit() {
        let usage = parse_usage(WALLET_SAMPLE).expect("钱包模式必须解析成功");
        assert_eq!(usage.remaining, "149.74");
        assert_eq!(usage.unit, "USD");
        assert!(usage.valid);
    }

    #[test]
    fn quota_mode_uses_top_level_remaining() {
        let usage = parse_usage(QUOTA_SAMPLE).expect("额度模式必须解析成功");
        assert_eq!(usage.remaining, "13.50");
        assert_eq!(usage.unit, "USD");
        // rate_limits 里的窗口刻意不进余额行：这里只回答「还剩多少」。
    }

    #[test]
    fn falls_back_to_quota_remaining_and_quota_unit() {
        let usage = parse_usage(r#"{"quota":{"remaining":3.25,"unit":"CNY"}}"#)
            .expect("没有顶层 remaining 时取 quota.remaining");
        assert_eq!(usage.remaining, "3.25");
        assert_eq!(usage.unit, "CNY");
        assert!(usage.valid, "缺 is_active / isValid 时按可用处理");
    }

    #[test]
    fn null_fields_fall_through_like_the_script_extractor() {
        // 用户提供的提取器是 `remaining ?? quota.remaining ?? balance`：字段存在但
        // 值为 null 时继续往后找。额度模式可能把用不上的字段显式给成 null。
        let usage =
            parse_usage(r#"{"remaining":null,"unit":null,"quota":{"remaining":7.5,"unit":"USD"}}"#)
                .expect("remaining=null 时继续取 quota.remaining");
        assert_eq!(usage.remaining, "7.50");
        assert_eq!(usage.unit, "USD");
    }

    #[test]
    fn falls_back_to_balance_and_defaults_unit() {
        let usage = parse_usage(r#"{"balance":"12.5","isValid":false}"#)
            .expect("只有 balance 的接口也要能展示");
        assert_eq!(usage.remaining, "12.50");
        assert_eq!(usage.unit, DEFAULT_UNIT, "接口没给单位时按 USD");
        assert!(!usage.valid, "isValid=false 是密钥停用，不是解析失败");
    }

    #[test]
    fn is_active_takes_precedence_over_is_valid() {
        let usage = parse_usage(r#"{"remaining":1,"is_active":false,"isValid":true}"#).unwrap();
        assert!(!usage.valid, "is_active 在提取器里优先于 isValid");
    }

    #[test]
    fn amount_formatting_keeps_small_nonzero_balances_visible() {
        assert_eq!(format_amount(48.62526778), "48.63");
        assert_eq!(format_amount(149.74), "149.74");
        assert_eq!(format_amount(12.0), "12.00");
        assert_eq!(format_amount(0.5), "0.5");
        assert_eq!(format_amount(0.0042), "0.0042");
        assert_eq!(
            format_amount(0.0000001),
            "0.0000001",
            "非零的极小余额不画成 0.00"
        );
        assert_eq!(format_amount(-3.2), "-3.20");
        assert_eq!(format_amount(0.0), "0.00");
        assert_eq!(format_amount(-0.0), "0.00");
    }

    #[test]
    fn missing_remaining_is_a_deterministic_error_not_a_zero_balance() {
        let message = parse_usage(r#"{"mode":"quota_limited","unit":"USD"}"#)
            .expect_err("没有剩余额度字段必须报错，不能画成 0");
        assert!(message.contains("剩余额度"), "{message}");
        // 非 JSON / 顶层不是对象同样是确定性失败。
        assert!(parse_usage("<html>网关错误</html>")
            .expect_err("非 JSON 必须报错")
            .contains("无法解析"));
        assert!(parse_usage("[]")
            .expect_err("数组不是对象")
            .contains("剩余额度"));
    }

    #[test]
    fn usage_endpoint_follows_the_configured_base_url() {
        assert_eq!(
            usage_endpoint(Some("https://2api.store/v1")).as_deref(),
            Ok("https://2api.store/v1/usage")
        );
        assert_eq!(
            usage_endpoint(Some("https://2api.store/v1/")).as_deref(),
            Ok("https://2api.store/v1/usage")
        );
        assert_eq!(
            usage_endpoint(Some("https://2api.store")).as_deref(),
            Ok("https://2api.store/v1/usage")
        );
        assert_eq!(
            usage_endpoint(Some("https://2api.store/v1/usage")).as_deref(),
            Ok("https://2api.store/v1/usage"),
            "用户已经填了完整用量地址时不能重复拼接"
        );
        // 国内优化线路只换域名、路径不变。
        assert_eq!(
            usage_endpoint(Some("https://cn.example.com/v1")).as_deref(),
            Ok("https://cn.example.com/v1/usage")
        );
        // 反向代理前缀：以 `/v1` 结尾即可，前缀原样保留（端点拼在 Base URL 之后）。
        assert_eq!(
            usage_endpoint(Some("https://host.example/proxy/v1")).as_deref(),
            Ok("https://host.example/proxy/v1/usage")
        );
    }

    #[test]
    fn usage_endpoint_refuses_shapes_it_cannot_derive() {
        assert!(usage_endpoint(None)
            .expect_err("没有 baseURL")
            .contains("baseURL"));
        assert!(usage_endpoint(Some("http://2api.store/v1"))
            .expect_err("http 不是合法端点")
            .contains("HTTPS"));
        assert!(usage_endpoint(Some("https://2api.store/v1/openai"))
            .expect_err("子路径形状不猜")
            .contains("Base URL"));
        // 端点推导失败是确定性失败，且**不发请求**。
        match fetch("test-key", None) {
            FetchOutcome::Deterministic(message, expired) => {
                assert!(message.contains("baseURL"), "{message}");
                assert!(!expired, "端点配置问题不是凭据失效");
            }
            _ => panic!("端点推导失败不能走网络"),
        }
    }
}
