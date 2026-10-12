//! opencode Go 套餐容量：滚动 5 小时 / 周 / 月三个窗口的剩余百分比与重置时刻。
//!
//! 端点 `GET https://opencode.ai/zen/go/v1/usage`（Bearer = 这个 workspace 的
//! Key）。密钥走本仓既有的内核凭据链——模型设置里 opencode 那两行 route
//! （`opencode-go` / Zen 的 `opencode`）各自写的 `apiKeyEnv`，默认派生名是
//! `OPENCODE_GO_API_KEY` / `OPENCODE_API_KEY`，见 [`crate::usage::credentials`]。
//! **不读 opencode 自己的 `~/.local/share/opencode/auth.json`**：那是另一个应用
//! 的凭据库，读它等于给本功能开一条用户没同意过的凭据来源（与 Codex 额度那条
//! 「显式许可」的纪律同源，见 `subscription-usage-design.md` 的凭据设计一节）。
//!
//! 内核里 opencode 有**两行 route**：`opencode-go`（Go 套餐）与 Zen 的
//! `opencode`。**两行都能查这个端点**（2026-10-13 实测：只配 Zen 行、用
//! `OPENCODE_API_KEY` 打 `/zen/go/v1/usage` 得到 200 与三窗口——端点认的是
//! workspace 的那把 Key，与 route 叫什么无关），所以 `classify_route` 两行都认，
//! 凭据取那一行自己写的 `apiKeyEnv`。**但别改成「route 名含 opencode」**：
//! `opencode-ox` / `opencode-zen` 这类自建 route 会跟着被认成同一家，等于替用户
//! 猜账号。
//!
//! 没配 Go 订阅的 workspace 打这个端点会拿到什么状态码**未经核实**（手上没有那种
//! 账号）；已知的三条失败路径（401/403 凭据被拒、结构不认识、网络）都不补数据，
//! 面板给的是错误态与「是否隐藏该项」的确认，绝不会画成「额度充足」。
//!
//! ## 为什么独立成模块
//!
//! `subscription.rs` 是反棘轮文件（预算 896 行、只许下调），这个 provider 的
//! 实现（端点 + 三窗口解析 + 样张单测）塞进去会当场顶破预算；而它与 MiniMax /
//! 智谱共用的只有「缓存绑定 / 指纹 / keep-last-good」那一层，那一层本来就住在
//! `subscription.rs` 里，这里只贡献「查一次 + 解析成 tier」。
//!
//! ## 响应（2026-10-12 真机，原样）
//!
//! ```json
//! {"usage":{"rolling":{"status":"ok","percent":1,"resetsAt":"2026-10-12T18:03:17.000Z"},
//!           "weekly": {"status":"ok","percent":0,"resetsAt":"2026-10-19T00:00:00.000Z"},
//!           "monthly":{"status":"ok","percent":0,"resetsAt":"2026-11-12T12:34:27.000Z"}}}
//! ```
//!
//! 三处口径：
//!
//! - `percent` 是**已用**百分比（与智谱 / OpenAI 同向，与 MiniMax 的剩余口径
//!   相反），统一换算成剩余后进 [`CacheTier`]。
//! - `resetsAt` 是 **ISO 8601 字符串**而不是 epoch 毫秒，走
//!   [`subscription::parse_epoch_ms`] 的字符串分支。
//! - 窗口**按字段名认**：三个窗口没有 `limit_window_seconds` 那样的自描述字段，
//!   字段名（rolling / weekly / monthly）本身就是语义。
//!
//! `status` 本版本不透出：真机只见 `"ok"`（社区实现记过 `"rate-limited"`），而
//! 窗口的核心读数已经是「已用百分比 + 重置时刻」——额度用尽时 percent 到 100
//! （剩余 0 → 红条），再加一个语义未经核实的徽章只会让人猜它是干什么的。字段
//! 留在这里，等第二处证据再决定。

use super::subscription::{self, CacheTier, FetchOutcome, ProviderData, PROVIDER_OPENCODE};

/// 第一方容量端点。真机核实过非 200 的两种成因：无密钥 / 错密钥是 401
/// `{"error":{"type":"AuthError"}}`（确定性失败，文案指向模型设置里的 Key）；
/// 这个域名在 Cloudflare 后面，**按出口签名（UA）拦人**会把 403 与「凭据被拒」
/// 混在同一个状态码里（实测 Python-urllib 的 UA 被拦，curl / node 的 UA 都过），
/// 而共享的 [`subscription::http_get_json`] 统一带 `dsh-xlink/<版本>` 的 UA，
/// 因此不另造一套请求——UA 变了要么在这里说明，要么先真机验一遍。
const USAGE_ENDPOINT: &str = "https://opencode.ai/zen/go/v1/usage";

/// 三窗口的字段名与展示短名，数组顺序即展示顺序（滚动 5 小时 → 周 → 月）。
///
/// 短名直接给显示形态：前端 `tierRow` 只重映射 `5h` 与 `weekly`→`7d`，其余
/// 原样透出（概览卡的名称列只有 22px）。周窗口的重置间隔是 7 天、月窗口是
/// 一个月，因此写 `7d` / `1mo` 而不是 `weekly` / `monthly`。
const WINDOWS: [(&str, &str); 3] = [("rolling", "5h"), ("weekly", "7d"), ("monthly", "1mo")];

/// 查一次容量，返回 provider 数据。凭据只在 HTTPS 请求头这一处出现。
pub(crate) fn fetch(key: &str) -> FetchOutcome {
    let body = match subscription::http_get_json(PROVIDER_OPENCODE, USAGE_ENDPOINT, key) {
        Ok(body) => body,
        Err(outcome) => return outcome,
    };
    match parse_windows(&body) {
        Ok(tiers) => FetchOutcome::Success(ProviderData::Plan { tiers }),
        // 结构不认识是确定性失败（不是凭据问题、也不是网络问题）：旧数据保留，
        // 截断摘要落 Shell 日志供反馈。HTTP 401/403 已在 http_get_json 里分走。
        Err(message) => {
            subscription::log_unrecognized_structure(PROVIDER_OPENCODE, &body);
            FetchOutcome::Deterministic(message, false)
        }
    }
}

/// 三窗口 → tier 列表（纯函数，真机样张做单测）。
///
/// `usage` 缺失或不是对象 = 结构不认识（端点改版：落日志 + 确定性提示）；单个
/// 窗口字段缺失 / 类型不合只跳过该窗口（逐字段防御式，与另几家同）。三个窗口
/// 都读不出来时返回空列表 → 分区显示「暂无额度数据」，**绝不补一行 100%**：
/// 把「没测到」画成「额度充足」是本功能最不能犯的错。
fn parse_windows(body: &str) -> Result<Vec<CacheTier>, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        format!(
            "接口返回了无法解析的数据（非 JSON）。{}",
            subscription::UNRECOGNIZED_STRUCTURE
        )
    })?;
    let Some(usage) = value.get("usage").and_then(serde_json::Value::as_object) else {
        return Err(subscription::UNRECOGNIZED_STRUCTURE.to_string());
    };
    let mut tiers = Vec::new();
    for (field, name) in WINDOWS {
        let Some(window) = usage.get(field) else {
            continue;
        };
        let Some(used) = window.get("percent").and_then(serde_json::Value::as_f64) else {
            continue;
        };
        tiers.push(CacheTier {
            name: name.to_string(),
            // 接口给的是**已用**百分比，缓存字段统一存剩余。
            remaining_percent: subscription::clamp_percent(100.0 - used),
            resets_at_ms: window
                .get("resetsAt")
                .and_then(subscription::parse_epoch_ms),
            unlimited: false,
        });
    }
    Ok(tiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-12 真机返回原样：不含身份信息（只有百分比与重置时刻），密钥
    /// 不进样张。
    const REAL_SAMPLE: &str = r#"{"usage":{"rolling":{"status":"ok","percent":1,"resetsAt":"2026-10-12T18:03:17.000Z"},"weekly":{"status":"ok","percent":0,"resetsAt":"2026-10-19T00:00:00.000Z"},"monthly":{"status":"ok","percent":0,"resetsAt":"2026-11-12T12:34:27.000Z"}}}"#;

    fn parse(body: &str) -> Result<Vec<CacheTier>, String> {
        parse_windows(body)
    }

    #[test]
    fn real_response_shape_is_understood_and_percent_is_used() {
        let tiers = parse(REAL_SAMPLE).expect("真机形状可识别");
        let names: Vec<&str> = tiers.iter().map(|tier| tier.name.as_str()).collect();
        assert_eq!(names, ["5h", "7d", "1mo"], "顺序固定：滚动 → 周 → 月");
        // percent 是**已用**：真机 rolling 是 1，剩余就是 99；另两个是 0 → 100。
        assert_eq!(tiers[0].remaining_percent, 99.0);
        assert_eq!(tiers[1].remaining_percent, 100.0);
        // ISO 8601 字符串要解析成 epoch 毫秒，而不是被当成「没有重置时间」。
        assert_eq!(tiers[0].resets_at_ms, Some(1_791_828_197_000));
        assert_eq!(tiers[2].resets_at_ms, Some(1_794_486_867_000));
    }

    #[test]
    fn missing_window_drops_only_that_window() {
        let tiers =
            parse(r#"{"usage":{"rolling":{"percent":50,"resetsAt":"2026-10-12T18:03:17.000Z"}}}"#)
                .expect("结构本身可识别");
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "5h");
        assert_eq!(tiers[0].remaining_percent, 50.0);
    }

    #[test]
    fn non_numeric_fields_are_skipped_not_fatal() {
        let tiers = parse(
            r#"{"usage":{"rolling":{"percent":"1"},"weekly":{"resetsAt":"2026-10-19T00:00:00.000Z"},
                 "monthly":{"percent":0.5}}}"#,
        )
        .expect("结构本身可识别");
        // rolling 的 percent 是字符串、weekly 没有 percent：两行都跳过；
        // 只有 monthly 的数值可读。
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "1mo");
        assert_eq!(tiers[0].remaining_percent, 99.5);
    }

    #[test]
    fn unreadable_reset_time_leaves_the_countdown_empty() {
        let tiers = parse(
            r#"{"usage":{"rolling":{"percent":10},"weekly":{"percent":10,"resetsAt":"不是时间"},
                 "monthly":{"percent":10,"resetsAt":1794486867000}}}"#,
        )
        .expect("结构本身可识别");
        assert_eq!(tiers[0].resets_at_ms, None, "没有重置时间就不画倒计时");
        assert_eq!(tiers[1].resets_at_ms, None, "读不出来的时间同样留空，不猜");
        assert_eq!(
            tiers[2].resets_at_ms,
            Some(1_794_486_867_000),
            "数字形态按既有的秒 / 毫秒容错解析"
        );
    }

    #[test]
    fn out_of_range_percent_is_clamped() {
        let tiers = parse(r#"{"usage":{"rolling":{"percent":140},"weekly":{"percent":-20}}}"#)
            .expect("结构本身可识别");
        assert_eq!(tiers[0].remaining_percent, 0.0, "已用 140% → 剩余 0");
        assert_eq!(tiers[1].remaining_percent, 100.0, "已用 -20% → 剩余 100");
    }

    #[test]
    fn unrecognized_shapes_are_reported_not_guessed() {
        assert!(parse(r#"{"nope":1}"#).is_err(), "缺 usage 必须报结构不认识");
        assert!(parse(r#"{"usage":[]}"#).is_err(), "usage 不是对象同样报错");
        assert!(parse("not json").is_err());
        // usage 在、三个窗口一个都读不出来：空列表（UI 显示「暂无额度数据」），
        // 不是错误、更不是三行 100%。
        assert!(parse(r#"{"usage":{}}"#).expect("空 usage").is_empty());
        assert!(
            parse(r#"{"usage":{"rolling":{"status":"rate-limited"}}}"#)
                .expect("只有 status")
                .is_empty(),
            "status 不是读数：没有 percent 就不画这一行"
        );
    }
}
