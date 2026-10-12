//! OpenAI 额度解析。SIWC 模型推理授权不等于 ChatGPT / Codex 额度接口授权。
//! 2026-10-10 同账号对照：SIWC 令牌即使带正确账号头也返回 401
//! no_matching_rule，Codex 令牌返回 200；不能用重新登录来解决权限不兼容。
//!
//! 与其它 provider 不同，模型登录状态与额度查询来源需要分别处理：
//!
//! - 默认只读本壳 OAuth vault 的登录状态。用户明确许可后，改用
//!   `subscription_codex` 的本机只读来源；账号不匹配或读取失败也保留分区，
//!   并作废旧来源数字、显示原因。模型登录不因此刷新或修改。
//! - **账号上下文不是充分条件**：端点要求 `ChatGPT-Account-Id` 请求头，值来自
//!   登录时从 ID token 取下的 `…/auth.chatgpt_account_id` 声明
//!   （`openai::auth::CHATGPT_AUTH_CLAIM`，摊平与嵌套两种形状都认）。部分
//!   access token 也会带同名声明，但 SIWC access token 的认证元数据可能是
//!   opaque，不能依赖它补出账号上下文。SIWC 推理令牌在发请求前明确拒绝，
//!   不把不兼容错误标成模型登录过期；Codex 来源的账号头由它自己的令牌提供。
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
use super::subscription::{
    self, CacheTier, InstanceScope, ProviderView, SubscriptionCacheDoc, PROVIDER_OPENAI,
};

/// 第一方额度接口。端点与响应形状来自 ChatGPT 第一方 Web 客户端，
/// 未经公开文档承诺，故按「逐字段防御式解析」对待（与另两个 provider 同）。
const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

/// 先说根因（2026-10-10 用户两轮反馈后定稿）：额度查询认的是 **Codex 的认证
/// 凭据**，且这是官方 API 的限制——OAuth 登录（模型调用）拿不到这种凭据，
/// 本机 Codex 没有登录就用不了。不要让用户先去折腾登录 / 账号 ID。
const UNSUPPORTED_SIWC: &str = "查询 GPT 套餐余额需要 Codex 的认证凭据，这是官方 API 的限制：OpenAI OAuth 登录只用于模型调用，其令牌不能查询额度——本机 Codex 没有登录就用不了。可开启「使用本机 Codex 登录查询额度」，或在 ChatGPT 设置 → 用量查看官方数据。";

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
///
/// `Debug`：断言要能打印出实际落在哪个变体上，否则「分类错了」只能看到一句
/// 「assertion failed」，而这正是本类型最需要讲清楚的事（凭据问题 vs 出口问题
/// 会引向完全相反的用户动作）。
#[derive(Debug)]
pub(crate) enum FetchProblem {
    /// 瞬时失败（网络 / 超时 / 读体中断）：缓存不写不删，keep-last-good。
    Transient(String),
    /// 凭据被拒（HTTP 401/403）：确定性失败，标记凭据失效。
    Rejected(String),
    /// 结构不认识。此响应含账号身份信息，不保留或记录原始响应。
    Unrecognized,
    /// 登录了但拿不到 ChatGPT 账号 id：额度端点要它当 `ChatGPT-Account-Id`
    /// 头，缺了就没有可发的请求。这**不是**凭据失效，也不是网络问题。
    NoAccountContext,
    /// 推理令牌不具备此内部额度接口的权限，不代表模型登录失效。
    UnsupportedCredential,
    /// 403 + HTML 挑战页 = 网关按**出口**拦人，请求本身没错（实测：直连 403、
    /// 代理 200，2026-10-09）。与凭据被拒不共处理：换一条路就能过，所以这一类
    /// 参与换路重试，全部路由试完仍被拦才上报（见 `call_route`）。
    GatewayBlocked(String),
}

/// 本 provider 要用的凭据。**只在内存里活到请求结束**，不进缓存 / 日志 /
/// UI（与另几个 provider 同一纪律）。
#[derive(Clone)]
pub(crate) struct OpenAiUsage {
    pub(crate) access_token: String,
    /// 缺失时（`None`）不是「未登录」，而是「登录了但服务端没给账号信息」——
    /// 两种情况要分开说，否则用户只会看到一句没头没尾的失败。
    pub(crate) account_id: Option<String>,
}

/// 解析当前活跃账号的凭据，供 `subscription.rs` 判「是否已配置」。
///
/// `None` = 尚未登录；已登录但拿不到账号 id 时仍返回凭据，让前端保留分区，
/// 查询侧再给出明确原因，而不是拿空值去打接口。
pub(crate) fn resolved_credential(
    usage: &Result<Option<OpenAiUsage>, String>,
    codex_enabled: bool,
) -> Option<credentials::ResolvedCredential> {
    // **登录了就该看得见这一栏**，哪怕暂时查不到数字。此前把「读不出账号 id」
    // 也归成「未配置」，结果是分区凭空消失、连一句原因都没有——用户看到的是
    // 「这功能没有」，而不是「这一栏在、但服务端没给账号信息」。这里只要
    // vault 里有活跃账号就算已配置，拿不到账号 id 的后果由查询侧如实报错。
    let reference = if codex_enabled {
        "codex-local"
    } else {
        "openai-oauth"
    };
    let source = if codex_enabled {
        credentials::CredentialSource::CodexLocal
    } else {
        credentials::CredentialSource::OpenAiVault
    };
    match usage {
        Ok(Some(usage)) => Some(credentials::ResolvedCredential {
            reference: reference.into(),
            value: Some(format!(
                "{reference}:{}:{}",
                usage.account_id.as_deref().unwrap_or(""),
                usage.access_token
            )),
            source,
            // 缓存绑**账号身份**而不是这枚 access_token：Codex 会按自己的节奏
            // 轮换令牌（它的 auth.json 里就带着 refresh_token），绑整枚令牌等于
            // 每换一次就作废缓存并越过 TTL 立刻重打接口——那次请求若恰好失败，
            // 用户看到的就是额度栏自己清空一次再跳回来。换账号仍然作废（账号
            // id 变了），切来源同样作废（`reference` 在前缀里）。
            cache_binding: Some(format!(
                "{reference}:{}",
                usage.account_id.as_deref().unwrap_or("")
            )),
        }),
        Ok(None) => None,
        // 错误也有独立绑定，不能把上一个已匹配账号的额度保留给不匹配账号。
        // 同样只绑来源，不绑错误文案：文案里的路径与句读不该让缓存说作废就作废。
        Err(message) => Some(credentials::ResolvedCredential {
            reference: reference.into(),
            value: Some(format!("{reference}:unavailable:{message}")),
            source,
            cache_binding: Some(format!("{reference}:unavailable")),
        }),
    }
}

pub(crate) fn resolve_usage(consent: &Result<bool, String>) -> Result<Option<OpenAiUsage>, String> {
    let codex_enabled = *consent.as_ref().map_err(Clone::clone)?;
    if codex_enabled {
        super::subscription_codex::active_usage()
    } else {
        active_credentials()
    }
}

/// 「套餐用量」OpenAI 分区的可见性门禁与离线处置（`subscription_view` 每个
/// provider 各调一次；非 OpenAI、或门禁放行时返回 `None` 走常规查询链）。
///
/// 判据（2026-10-10 用户拍板）：**内嵌 openai-oauth 插件已启用**。插件停用
/// 时，壳里的 OpenAI 登录没有消费方（它只服务于内嵌插件），整块额度入口
/// ——分区与「本机 Codex 登录查询额度」开关（前端按同一判据收起）——都不
/// 出现，哪怕本机 Codex 许可是开着的：这条查询注定失败（SIWC 令牌过不了
/// 额度接口），分区留着只会挂一块「查询异常」。按未配置呈现，缓存里的旧
/// 条目（含错误文案）一并丢弃，错误横幅不再提及。Codex 许可只在插件启用
/// 后决定查询来源（[`resolve_usage`]），不参与本门禁。
pub(crate) fn gated_view(
    id: &str,
    scope: &InstanceScope,
    doc: &mut SubscriptionCacheDoc,
) -> Option<(bool, ProviderView)> {
    if id != PROVIDER_OPENAI {
        return None;
    }
    let dsh_home = crate::shell::paths::instance_dsh_home(&scope.family, &scope.id);
    // 门禁认的是 **openai-oauth 那一个**内置插件的启用意图：按 id 从登记表
    // 取 spec，不 new 一个残缺的（spec 里还有物化文件集与接线行 id，凭 id
    // 造一个出来会让门禁与实际接线脱钩）。
    let spec =
        crate::plugins::builtin::builtins::find(crate::plugins::builtin::builtins::OPENAI_OAUTH)
            .expect("openai-oauth 必须在内置插件登记表里");
    if crate::plugins::builtin::requested_enabled(spec, &dsh_home, &scope.profile) {
        return None;
    }
    // 旧缓存条目一并移除：分区藏了，错误横幅不能再报它缓存里的失败。
    let removed = doc.providers.remove(PROVIDER_OPENAI).is_some();
    let view = subscription::build_provider_view(id, false, None, None);
    Some((removed, view))
}

/// 取当前活跃账号的凭据。**vault 文件不存在就直接返回**：没有它就没有账号
/// 条目可读，也就没有理由去碰系统钥匙串——单测在临时 home 下跑到这里时，
/// 钥匙串那半条路是绝不能碰的（`vault.rs` 的测试纪律）。
///
/// 缺 `chatgpt_account_id`（登录早于落库该字段的版本，或授权服务没有把账号
/// 上下文放进令牌）仍保留为已登录状态；查询侧会给出明确原因，不拿空账号
/// id 去打接口。SIWC 令牌即使有账号 id 也不能用于此内部额度接口。
fn active_credentials() -> Result<Option<OpenAiUsage>, String> {
    let Some(tokens) = active_account()? else {
        return Ok(None);
    };
    Ok(Some(OpenAiUsage {
        access_token: tokens.access_token,
        account_id: tokens.chatgpt_account_id,
    }))
}

/// 用量查询只读现有库与密钥；不触发模型令牌刷新，也不创建缺失密钥。
///
/// 「不存在就返回、连钥匙串都不碰」「put 拒绝创建」这两条判据不在这里复述——
/// 它们住在 [`crate::openai::flow::read_active_entry`]，与账号状态面板共用同一份。
pub(crate) fn active_account() -> Result<Option<crate::openai::vault::AccountTokens>, String> {
    use crate::openai::vault;
    let paths = crate::openai::flow::shell_flow_paths();
    let mode = crate::shell::settings::current_mode().as_str().to_string();
    Ok(
        crate::openai::flow::read_active_entry(&paths, &mode, vault::keyring_get)?.map(
            |mut tokens| {
                tokens.chatgpt_account_id = tokens
                    .chatgpt_account_id
                    .filter(|id| !id.trim().is_empty())
                    .or_else(|| account_id_from_id_token(&tokens.id_token));
                tokens
            },
        ),
    )
}

/// 从本壳登录时验签留存的 ID token 回填旧 vault 的账号信息，供跨来源一致性
/// 校验；回填不改变 SIWC 的权限，也不赋予它访问内部额度接口的能力。
///
/// 这里**不再验签名**：那份 ID token 正是登录时做过完整验证（签名 / iss /
/// aud / exp / nonce）之后才存进来的，且整个 vault 是加密的；读出来的又只是
/// 「发给哪个账号」这一个查询上下文，不是身份判定。判据与 Codex 侧共用
/// [`crate::openai::jwk::unverified_claims`]。
fn account_id_from_id_token(id_token: &str) -> Option<String> {
    let claims = crate::openai::jwk::unverified_claims(id_token)?;
    crate::openai::auth::chatgpt_account_id(&claims)
}

/// 仅分类已读取的令牌，不作身份认证；opaque 认证元数据不解密、不猜测。
fn is_siwc_access_token(token: &str) -> bool {
    let Some(claims) = crate::openai::jwk::unverified_claims(token) else {
        return false;
    };
    claims
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|scope| {
            scope
                .split_whitespace()
                .any(|s| s == "chatgpt.tokens.use.direct")
        })
}

/// 查一次额度窗口，返回 provider 数据。凭据只在 HTTPS 请求头这一处出现。
pub(crate) fn fetch_resolved(
    usage: &Result<Option<OpenAiUsage>, String>,
    now_ms: u64,
) -> subscription::FetchOutcome {
    use subscription::{FetchOutcome, ProviderData};
    let usage = match usage {
        Ok(Some(usage)) => usage,
        Ok(None) => return FetchOutcome::Deterministic("未在本壳登录 OpenAI 账户".into(), false),
        Err(message) => return FetchOutcome::Deterministic(message.clone(), false),
    };
    match fetch_tiers(usage, now_ms) {
        Ok(tiers) => FetchOutcome::Success(ProviderData::Plan { tiers }),
        Err(FetchProblem::Transient(message)) => FetchOutcome::Transient(message),
        Err(FetchProblem::Rejected(message)) => FetchOutcome::Deterministic(message, true),
        Err(FetchProblem::UnsupportedCredential) => FetchOutcome::Deterministic(UNSUPPORTED_SIWC.into(), false),
        Err(FetchProblem::NoAccountContext) => FetchOutcome::Deterministic(
            "当前登录没有额度接口要求的账号上下文，不能据此查询额度。请在 ChatGPT 设置 → 用量查看官方数据。".into(),
            false,
        ),
        Err(FetchProblem::Unrecognized) => {
            subscription::log_subscription_line("subscription Codex 额度响应缺少可识别的窗口结构；未记录原始响应以保护账号信息".into());
            FetchOutcome::Deterministic(subscription::UNRECOGNIZED_STRUCTURE.to_string(), false)
        }
        // `fetch_tiers` 内部已把挑战页换路重试并归并成 Transient，走不到这里；
        // 保留显式分支是为了让新增变体必须表态，而不是被 `_` 悄悄吞掉。
        Err(FetchProblem::GatewayBlocked(who)) => FetchOutcome::Transient(format!(
            "该网络出口被网关拦截（{who} 返回 HTML 挑战页，不是账号或请求问题）"
        )),
    }
}

/// 非 2xx 响应 → 失败分类（纯函数，可测）。
///
/// 抽出来不只是为了测：`403` 有两种含义完全不同的成因——凭据被拒（重试无用）
/// 与出口被网关拦（换条路就能过），而它们**只能靠响应体区分**。混在一起时用户
/// 会被引去刷新一个完全无辜的登录态，而真正该做的（启用代理）没人告诉他。
fn classify_failure(status: u16, body: &str, route_label: &str) -> FetchProblem {
    if crate::openai::transport::looks_like_gateway_challenge(status, body) {
        return FetchProblem::GatewayBlocked(route_label.to_string());
    }
    match status {
        401 | 403 => FetchProblem::Rejected(format!(
            "Codex 额度查询凭据被拒绝（HTTP {status}）。请先在 Codex 中刷新登录后再查询；这不代表 DSH 模型登录失效"
        )),
        _ => FetchProblem::Transient(format!(
            "OpenAI 用量接口返回 HTTP {status}。已保留上次结果，稍后可重试"
        )),
    }
}

/// 查询额度窗口。
fn fetch_tiers(credential: &OpenAiUsage, now_ms: u64) -> Result<Vec<CacheTier>, FetchProblem> {
    if is_siwc_access_token(&credential.access_token) {
        return Err(FetchProblem::UnsupportedCredential);
    }
    let Some(account_id) = credential.account_id.as_deref() else {
        return Err(FetchProblem::NoAccountContext);
    };
    let access_token = credential.access_token.as_str();
    // 先代理、失败再直连（net_proxy 恒以直连收尾）。**只有传输层失败与「出口被
    // 网关拦」才换路由**：401 与「结构不认识」换一条路只会同样地失败；若让它们
    // 继续往下走，分类会随路由顺序漂移——凭据失效被后续路由的传输错误盖住，
    // 报成「网络不可达」，这一轮也不会置 expired。
    let mut last_transport_error: Option<FetchProblem> = None;
    let mut blocked_by: Vec<String> = Vec::new();
    for route in crate::pkg::net_proxy::routes() {
        match call_route(&route, access_token, account_id) {
            Ok(text) => {
                return match parse_tiers(&text, now_ms) {
                    Ok(tiers) if !tiers.is_empty() => Ok(tiers),
                    _ => Err(FetchProblem::Unrecognized),
                }
            }
            Err(problem @ FetchProblem::Transient(_)) => last_transport_error = Some(problem),
            // 网关按出口拦人：换一条路就能过（实测直连 403、代理 200），所以
            // 继续试下一条路由，而不是当场放弃。
            Err(FetchProblem::GatewayBlocked(who)) => blocked_by.push(who),
            Err(problem) => return Err(problem),
        }
    }
    // 全部路由都被网关按出口拦：如实说这件事，别报成「网络不可达」——后者
    // 会把用户引去检查代理设置，而真正要做的正是启用它。
    if !blocked_by.is_empty() {
        return Err(FetchProblem::Transient(format!(
            "该网络出口被网关拦截（{} 返回 HTML 挑战页，不是账号或请求问题）。请确认系统代理正在运行后重试",
            blocked_by.join(" → ")
        )));
    }
    // 所有路由都传输失败：报最后一条，keep-last-good。
    Err(last_transport_error.unwrap_or(FetchProblem::Transient(
        "查询失败（没有可用的出网路径）。已保留上次结果，可点击刷新重试".into(),
    )))
}

/// 走一条路由发一次请求。
fn call_route(
    route: &crate::pkg::net_proxy::Route,
    access_token: &str,
    account_id: &str,
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
        .header("Authorization", &format!("Bearer {access_token}"))
        .header("ChatGPT-Account-Id", account_id)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    match request.call() {
        // agent 带 `http_status_as_error(false)`（见 `agent_for`）：非 2xx 走
        // 这里而不是 Err，因为**要靠响应体分辨「凭据被拒」与「出口被网关拦」**。
        // ureq 默认把状态码转成 Error 且不带 body，那正是这条信息消失的原因。
        Ok(mut response) => {
            let status = response.status().as_u16();
            // 读体失败**必须**原样分类，不能 `unwrap_or_default()` 吞掉：
            // 吞掉之后拿到空串，会被 `parse_tiers` 判成「结构不认识」——把一个
            // 传输层故障报成上游改版，用户会朝完全错误的方向排查。
            let text = match response
                .body_mut()
                .with_config()
                .limit(MAX_HTTP_BODY_BYTES)
                .read_to_string()
            {
                Ok(text) => text,
                Err(error) => {
                    return Err(FetchProblem::Transient(format!(
                        "读取响应失败（{error}）。已保留上次结果，可点击刷新重试"
                    )))
                }
            };
            if (200..300).contains(&status) {
                return Ok(text);
            }
            Err(classify_failure(status, &text, &route.describe()))
        }
        Err(error) => Err(FetchProblem::Transient(format!(
            "查询失败（网络不可达或超时：{error}）。已保留上次结果，可点击刷新重试"
        ))),
    }
}

/// 按路由取 agent 并缓存。构建本身走 [`crate::openai::transport::agent_for`]
/// （`Direct` 显式 `.proxy(None)`、`http_status_as_error(false)` 这两条纪律只在
/// 那里存在一份），这里只多一层按路由键的缓存——额度查询每次刷新都建 agent
/// 没有意义，而代理地址在一次进程生命周期里通常也不变。
fn agent_for(route: &crate::pkg::net_proxy::Route) -> Option<ureq::Agent> {
    static AGENTS: OnceLock<std::sync::Mutex<Vec<(String, ureq::Agent)>>> = OnceLock::new();
    let cache = AGENTS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    // Direct 也进这张表：它与任何代理地址都不同键，而 `proxy(None)` 的结果
    // 同样只值得建一次。返回**拥有值**——借出 Vec 元素的引用会随别的线程
    // push 触发重分配而悬空。
    let key = route.describe();
    if let Some(index) = cache.iter().position(|(seen, _)| *seen == key) {
        return cache.get(index).map(|(_, agent)| agent.clone());
    }
    // 代理地址不合法在这里是「这一路走不了」而不是致命错：调用方会把这一路
    // 当传输失败处理，继续试下一条（`call_route` 的既有语义）。
    let agent = crate::openai::transport::agent_for(route, HTTP_TIMEOUT).ok()?;
    cache.push((key, agent.clone()));
    Some(agent)
}

/// 解析额度窗口 → tier 列表。逐字段防御式：字段缺失或类型不合就跳过该字段，
/// 不做整体失败（端点是第一方但未文档化，字段可能随时漂移）。
///
/// 返回空列表表示「结构不认识」，由调用方落日志。
fn parse_tiers(body: &str, now_ms: u64) -> Result<Vec<CacheTier>, String> {
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

    fn id_token_with(claim: &str, value: &str) -> String {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"{claim}":"{value}"}}"#).as_bytes());
        format!("h.{payload}.s")
    }

    #[test]
    fn rotating_the_token_keeps_the_cache_binding_but_changing_account_drops_it() {
        // Codex 会按自己的节奏轮换 access_token（auth.json 里带着 refresh_token
        // 就是为此）。把缓存绑在整枚令牌上，等于每换一次就作废缓存并越过 5 分钟
        // TTL 立刻重打接口；那次请求若恰好瞬时失败，用户看到的就是额度栏自己
        // 清空一次再跳回来，且没有任何提示。轮换的是令牌，不变的是账号。
        let at = |token: &str, account: &str| {
            Ok(Some(OpenAiUsage {
                access_token: token.into(),
                account_id: Some(account.into()),
            }))
        };
        let binding = |usage: &Result<Option<OpenAiUsage>, String>, codex: bool| {
            resolved_credential(usage, codex).unwrap().cache_binding
        };

        let first = binding(&at("token-1", "acct-a"), true);
        let rotated = binding(&at("token-2", "acct-a"), true);
        assert_eq!(
            first, rotated,
            "同一账号的令牌轮换不得作废缓存（否则 5 分钟 TTL 被架空）"
        );
        assert_ne!(
            first,
            binding(&at("token-1", "acct-b"), true),
            "换账号必须作废缓存"
        );
        assert_ne!(
            first,
            binding(&at("token-1", "acct-a"), false),
            "切来源（Codex → 本壳 OAuth）必须作废缓存"
        );
        // 真正发请求的凭据仍然带着令牌本身——绑定身份只用于缓存，不参与 HTTP。
        assert_ne!(
            resolved_credential(&at("token-1", "acct-a"), true)
                .unwrap()
                .value,
            resolved_credential(&at("token-2", "acct-a"), true)
                .unwrap()
                .value
        );
    }

    #[test]
    fn cache_binding_changes_with_source_workspace_and_resolution_failure() {
        let a = Ok(Some(OpenAiUsage {
            access_token: "same-token".into(),
            account_id: Some("a".into()),
        }));
        let b = Ok(Some(OpenAiUsage {
            access_token: "same-token".into(),
            account_id: Some("b".into()),
        }));
        let base = resolved_credential(&a, true).unwrap().value;
        assert_ne!(base, resolved_credential(&a, false).unwrap().value);
        assert_ne!(base, resolved_credential(&b, true).unwrap().value);
        let failed = Err("账号不匹配".to_string());
        assert_ne!(base, resolved_credential(&failed, true).unwrap().value);
        assert!(matches!(
            fetch_resolved(&failed, 0),
            subscription::FetchOutcome::Deterministic(_, false)
        ));
        assert!(resolved_credential(&Ok(None), false).is_none());
        let consent_error = Err("许可文件损坏".to_string());
        assert!(
            resolve_usage(&consent_error).is_err(),
            "许可未知时不读取任何凭据"
        );
        let resolved = resolved_credential(&a, true).unwrap();
        assert!(!resolved.log_hint().contains("same-token"));
        assert!(!resolved.log_hint().contains(":a:"));
    }

    #[test]
    fn siwc_cannot_query_usage_even_with_a_correct_account_context() {
        for account_id in [None, Some("account-context".to_string())] {
            let credential = OpenAiUsage {
                access_token: id_token_with("scope", crate::openai::auth::AUTH_SCOPE),
                account_id,
            };
            assert!(matches!(
                fetch_tiers(&credential, 0),
                Err(FetchProblem::UnsupportedCredential)
            ));
        }
        // 2026-10-10 用户两轮反馈定稿的判据：三句核心都在——要 Codex 认证
        // 凭据、官方 API 限制、本机 Codex 没登录就用不了；且不再出现「重新
        // 登录」这种把用户往登录上引的表述。
        assert!(UNSUPPORTED_SIWC.contains("Codex 的认证凭据"));
        assert!(UNSUPPORTED_SIWC.contains("官方 API"));
        assert!(UNSUPPORTED_SIWC.contains("本机 Codex 没有登录就用不了"));
        assert!(!UNSUPPORTED_SIWC.contains("重新登录"));
        assert!(!UNSUPPORTED_SIWC.contains("请重新登录"));
    }

    /// 403 有两种成因完全不同的含义，只能靠响应体分开：**凭据被拒**（重试无用）
    /// 与**出口被网关拦**（换条路就能过，实测直连 403、代理 200，2026-10-09）。
    /// 额度端点此前把两者一律报成「凭据被拒，请刷新登录」，把用户引去刷新一个
    /// 完全无辜的登录态。判据钉在这两种情形上——`call_route` 自身要发网络请求，
    /// 没法进单测，所以分类逻辑必须是这个纯函数。
    #[test]
    fn gateway_challenge_is_not_reported_as_a_rejected_credential() {
        let challenge = "<!DOCTYPE html><html><title>Just a moment...</title></html>";
        match classify_failure(403, challenge, "直连") {
            FetchProblem::GatewayBlocked(who) => assert_eq!(who, "直连"),
            other => panic!("403 + HTML 挑战页必须归类为出口被拦，实际是 {other:?}"),
        }
        // 大小写与前导空白都要认（真网关不一定规范输出）。
        assert!(matches!(
            classify_failure(403, "\n  <!doctype HTML><html></html>", "代理"),
            FetchProblem::GatewayBlocked(_)
        ));
        // 同为 403 但是 JSON → 那才是真的凭据被拒。
        match classify_failure(403, r#"{"detail":"invalid_token"}"#, "直连") {
            FetchProblem::Rejected(message) => {
                assert!(message.contains("凭据被拒绝"), "{message}");
                assert!(message.contains("不代表 DSH 模型登录失效"), "{message}");
            }
            other => panic!("403 + JSON 必须归类为凭据被拒，实际是 {other:?}"),
        }
        // 401 无论响应体是什么都是凭据被拒。
        assert!(matches!(
            classify_failure(401, "<!doctype html>", "直连"),
            FetchProblem::Rejected(_)
        ));
        // 其它状态码是普通瞬时失败。
        assert!(matches!(
            classify_failure(500, "", "直连"),
            FetchProblem::Transient(_)
        ));
        // 非挑战页的 403 文案不许出现「网关拦截」字样：那是给错方向的动作建议。
        let FetchProblem::Transient(message) = classify_failure(503, "", "直连") else {
            panic!("503 应为 Transient");
        };
        assert!(!message.contains("网关"), "{message}");
    }

    #[test]
    fn credential_classification_uses_exact_scope_not_opaque_metadata() {
        assert!(!is_siwc_access_token("invalid"));
        assert!(!is_siwc_access_token(&id_token_with(
            "scope",
            "openid email"
        )));
        assert!(!is_siwc_access_token(&id_token_with(
            "scope",
            "chatgpt.tokens.use.direct.extra"
        )));
        assert!(is_siwc_access_token(&id_token_with(
            "scope",
            "openid chatgpt.tokens.use.direct email"
        )));
    }

    #[test]
    fn account_id_is_recovered_from_the_stored_id_token() {
        // 摊平形状（Codex CLI 那份）：https://api.openai.com/auth.chatgpt_account_id
        let token = id_token_with(
            &format!(
                "{}.chatgpt_account_id",
                crate::openai::auth::CHATGPT_AUTH_CLAIM
            ),
            "7c26dd40-bf4b-42f7-8b81-cd546ca363e8",
        );
        assert_eq!(
            account_id_from_id_token(&token).as_deref(),
            Some("7c26dd40-bf4b-42f7-8b81-cd546ca363e8")
        );
    }

    #[test]
    fn a_token_without_the_claim_yields_nothing() {
        assert_eq!(account_id_from_id_token(&id_token_with("sub", "u-1")), None);
        // **嵌套形状**：整个命名空间是一个对象，账号 id 在对象内部。
        let nested = {
            use base64::Engine;
            let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acc-nested","chatgpt_plan_type":"plus"}}"#
                    .as_bytes(),
            );
            format!("h.{payload}.s")
        };
        assert_eq!(
            account_id_from_id_token(&nested).as_deref(),
            Some("acc-nested")
        );
        assert_eq!(account_id_from_id_token(""), None);
        assert_eq!(account_id_from_id_token("not-a-jwt"), None);
        // 三段齐全但载荷不是 JSON：读不出就当作没有，不报错。
        assert_eq!(account_id_from_id_token("a.!!!.c"), None);
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
