//! 云端套餐 / Token Plan 用量：查询并展示 MiniMax Token Plan 的双窗口额度
//! 进度与 DeepSeek 按量余额（设计见 `docs/subscription-usage-design.md`）。
//!
//! 与 [`crate::usage`]（本地 token 统计）的关系：usage.rs 回答「过去消耗了
//! 多少」，本模块回答「云端账户还剩多少、何时重置」。两层数据互相独立。
//!
//! ## 凭据（复用当前内核模型凭据）
//!
//! dsh-xlink 不收集、不存储凭据：查询用当前 DSH 实例 / profile 已配置的模型
//! 凭据，由 [`crate::credentials`] 只读解析（profile `apiKeyEnv` → 环境变量 →
//! `.credentials.yaml` → `.env`，与内核同一优先级）。原始凭据只存在于凭据
//! 读取与 HTTPS 请求头两处；不进 UI、缓存、日志、事件或 toast。
//!
//! ## 关键约定
//!
//! - **keep-last-good**：瞬时失败（网络不可达 / 超时 / 读体中断）时缓存
//!   不写、不删，错误单独透出——绝不把失败渲染成「0 余额 / 0%」误导用户。
//!   确定性失败（凭据失效 / 业务错误码 / 结构不认识）保留旧数据、只更新
//!   `credential_status` 与 `error`。
//! - **`credential_status == expired` 的条目不参与自动刷新**（避免拿失效
//!   凭据反复打接口），仅用户 force（点刷新 / 测试连接）时重试。
//! - **缓存绑定实例 / profile / 凭据指纹**：缓存文档按实例隔离；条目记录
//!   credential reference 与凭据值的不可逆指纹。换凭据 / 切实例 / 换
//!   profile 后旧条目一律作废，绝不把上一个账号的数据当当前账号展示。
//! - **每个 provider 自带状态**：多 provider 查询允许部分成功，单个网络
//!   失败不吞掉其它 provider 的新数据。
//! - 解析层逐字段防御式：字段缺失或类型不合就跳过该字段，不做整体失败
//!   （两家端点都是第一方但未文档化 / 轻文档化接口，字段可能随时漂移）。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::credentials::{self, ResolvedCredential};
use crate::error::AppError;
use crate::instance;
use crate::paths;
use crate::state::{self, StateCtx};

/// 缓存文档的 schema 版本。v2 起绑定实例 / profile / 凭据指纹。
pub const CACHE_SCHEMA: u32 = 2;

/// 缓存 TTL：5 分钟内重复拉取直接用缓存；force 越过。
const CACHE_TTL_MS: u64 = 5 * 60_000;

/// 单次云端查询的超时。比 [`crate::releases`] 的 30 秒短：这里是用户盯着
/// 等结果的交互路径。
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// 查询响应体上限。余额 / 额度接口的正常响应只有几 KB，超限按错误处理，
/// 不给异常响应吃内存的机会。
const MAX_HTTP_BODY_BYTES: u64 = 1024 * 1024;

/// 错误写日志时的响应摘要上限：够定位「接口改成了什么」，又不至于把日志撑爆。
const LOG_RESPONSE_SNIPPET_BYTES: usize = 200;

pub const PROVIDER_MINIMAX_CN: &str = "minimax_cn";
pub const PROVIDER_MINIMAX_EN: &str = "minimax_en";
pub const PROVIDER_DEEPSEEK: &str = "deepseek";
pub const PROVIDER_ZAI_CODING_CN: &str = "zai_coding_cn";

/// 视图里的固定输出顺序：概览卡与独立窗口都按它排列。
const PROVIDER_ORDER: [&str; 4] = [
    PROVIDER_MINIMAX_CN,
    PROVIDER_MINIMAX_EN,
    PROVIDER_DEEPSEEK,
    PROVIDER_ZAI_CODING_CN,
];

/// 缓存条目的凭据状态：凭据可用（含「还没验证过」）。
pub const CREDENTIAL_VALID: &str = "valid";
/// 缓存条目的凭据状态：凭据已失效（仅 HTTP 401/403 会置位）。
pub const CREDENTIAL_EXPIRED: &str = "expired";

const MINIMAX_REMAINS_PATH: &str = "/v1/api/openplatform/coding_plan/remains";
/// MiniMax CN 入口。注意：额度接口只在 minimaxi.com 域名有出处（cc-switch
/// 同源消费）；platform.minimax.cn 站发的凭据能否查询此入口需真机验证
/// （设计稿步骤 0），不通则改此常量并回写文档。
const MINIMAX_CN_ENDPOINT: &str = "https://api.minimaxi.com";
const MINIMAX_EN_ENDPOINT: &str = "https://api.minimax.io";
/// DeepSeek 官方余额接口（轻文档化，字段以官方样张为准）。
const DEEPSEEK_BALANCE_ENDPOINT: &str = "https://api.deepseek.com/user/balance";
/// 智谱（bigmodel.cn）编程套餐额度接口：未文档化的 Web 端点，鉴权用 raw
/// API key（不带 Bearer），且必须携带组织 / 项目上下文头与 `type` 查询参数
/// （个人套餐 = 1，团队套餐 = 2），否则分别报「用户不存在 coding plan」或
/// 返回空 data。org / project / type 经凭据解析链配置（见 `ZhipuContext`）。
const ZHIPU_QUOTA_ENDPOINT: &str = "https://bigmodel.cn/api/monitor/usage/quota/limit";
/// `type` 查询参数缺省值：个人套餐。
const ZHIPU_PLAN_TYPE_DEFAULT: &str = "1";

const USER_AGENT: &str = concat!("dsh-xlink/", env!("CARGO_PKG_VERSION"));

fn provider_label(id: &str) -> &'static str {
    match id {
        PROVIDER_MINIMAX_CN => "MiniMax（国内站）",
        PROVIDER_MINIMAX_EN => "MiniMax（国际站）",
        PROVIDER_DEEPSEEK => "DeepSeek",
        PROVIDER_ZAI_CODING_CN => "智谱 GLM",
        _ => "未知供应商",
    }
}

/// 缓存条目 / 视图的 `kind`：MiniMax 是套餐窗口进度，DeepSeek 是货币余额。
fn provider_kind(id: &str) -> &'static str {
    if id == PROVIDER_DEEPSEEK {
        "balance"
    } else {
        "plan"
    }
}

// --- 脱敏与指纹 ---------------------------------------------------------------

/// 凭据脱敏：保留前 4 后 4 字符，中间 `****`；短于 12 字符全打码。
/// 只用于**日志里的诊断提示**（帮用户分辨是哪把凭据失败），UI 不展示。
pub fn redact_key(key: &str) -> String {
    let key = key.trim();
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 12 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}****{tail}")
}

/// 缓存条目绑定凭据的指纹（SHA-256 前 16 个 hex 字符）：只用于判断「这条
/// 缓存属不属于当前凭据」，不可逆、不含原文。
fn credential_fingerprint(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.trim().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

// --- 查询上下文（当前实例 / profile） -----------------------------------------

/// 一次查询绑定的当前实例 / profile。凭据与缓存都跟着它走。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstanceScope {
    pub family: String,
    pub id: String,
    pub profile: String,
}

impl InstanceScope {
    fn resolve() -> Self {
        let (family, id) = instance::resolve_default();
        Self {
            family: family.to_string(),
            id: id.to_string(),
            profile: credentials::default_profile(),
        }
    }

    fn dsh_home(&self) -> PathBuf {
        paths::instance_dsh_home(&self.family, &self.id)
    }

    fn cache_path(&self) -> PathBuf {
        paths::instance_subscription_cache_file(&self.family, &self.id)
    }
}

// --- 缓存文档 -----------------------------------------------------------------

/// 一个额度层（5h / 周）：`remaining_percent` 是**剩余**百分比。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct CacheTier {
    pub name: String,
    pub remaining_percent: f64,
    #[serde(default)]
    pub resets_at_ms: Option<u64>,
}

/// 一个币种的余额行。金额是数字字符串，透传展示、不转浮点参与计算。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct CacheBalance {
    pub currency: String,
    pub total: String,
    #[serde(default)]
    pub granted: Option<String>,
    #[serde(default)]
    pub topped_up: Option<String>,
}

/// 单个 provider 的缓存条目。`tiers` / `balances` 存的是**上一次成功**的
/// 数据；确定性失败时旧数据不动，只更新 `credential_status` 与 `error`。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct ProviderCacheEntry {
    pub kind: String,
    /// 解析该凭据用的 credential reference 名（引用名不是秘密）。
    #[serde(default)]
    pub credential_ref: String,
    /// 凭据值指纹（SHA-256 前 16 hex）；与当前凭据不符的条目按不存在处理。
    #[serde(default)]
    pub credential_fingerprint: String,
    #[serde(default)]
    pub credential_status: String,
    /// 最近一次**成功**查询的时间（毫秒）；0 表示从未成功过。
    #[serde(default)]
    pub queried_at_ms: u64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub is_available: Option<bool>,
    #[serde(default)]
    pub tiers: Vec<CacheTier>,
    #[serde(default)]
    pub balances: Vec<CacheBalance>,
}

/// 缓存文档归属的实例 / profile：切换实例或 profile 后旧文档整体作废。
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct CacheInstanceBinding {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub profile: String,
}

/// 缓存文档（`<实例目录>/subscription-cache.json`）。
#[derive(Serialize, Deserialize, Default)]
pub struct SubscriptionCacheDoc {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub instance: Option<CacheInstanceBinding>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderCacheEntry>,
}

fn cache_ctx() -> StateCtx {
    StateCtx {
        corrupt: |reason| {
            format!(
                "套餐用量缓存损坏（{reason}）。已停止读写以免覆盖；\
                     如确认放弃缓存数据，可删除该文件后重试。"
            )
        },
        kind: AppError::Subscription,
    }
}

// --- 视图 ---------------------------------------------------------------------

/// 一个额度层的前端视图。
#[derive(Serialize, Debug)]
pub struct TierView {
    pub name: String,
    pub remaining_percent: f64,
    pub resets_at_ms: Option<u64>,
}

/// 一个币种余额的前端视图。
#[derive(Serialize, Debug)]
pub struct BalanceView {
    pub currency: String,
    pub total: String,
    pub granted: Option<String>,
    pub topped_up: Option<String>,
}

/// 单个 provider 的展示状态。`fetch_error` 是**本次调用**的瞬时失败（缓存
/// 未动，前端 keep-last-good + 横幅）；`error` 是缓存里的确定性失败。
#[derive(Serialize, Debug)]
pub struct ProviderView {
    pub id: String,
    pub label: &'static str,
    /// 未配置的 provider 为 `None`；已配置则为 "plan" / "balance"。
    pub kind: Option<&'static str>,
    pub configured: bool,
    /// 解析用的 credential reference 名；未配置或还没有条目时为 `None`。
    pub credential_ref: Option<String>,
    pub credential_status: Option<String>,
    pub queried_at_ms: Option<u64>,
    pub error: Option<String>,
    pub fetch_error: Option<String>,
    pub is_available: Option<bool>,
    pub tiers: Vec<TierView>,
    pub balances: Vec<BalanceView>,
}

/// 视图携带的当前实例 / profile：独立窗口与主面板都展示它，避免用户误解
/// 余额所属账号。
#[derive(Serialize, Debug)]
pub struct InstanceView {
    pub family: String,
    pub id: String,
    pub profile: String,
}

/// `get_subscription_usage` 的返回：全部（或指定）provider 的独立结果。
#[derive(Serialize, Debug)]
pub struct SubscriptionView {
    pub instance: InstanceView,
    pub providers: Vec<ProviderView>,
}

/// 同一时刻只允许一次云端查询（同 provider 在途请求去重的最简形态：整个
/// 查询段串行）。锁只在 `spawn_blocking` worker 内获取，与 `SCAN_LOCK` 同约定。
static FETCH_LOCK: Mutex<()> = Mutex::new(());

/// 查询全部（`provider = None`）或指定 provider 的用量视图。
///
/// 每个配置了凭据的 provider 按缓存 TTL 决定是否真的发请求；返回值恒为
/// `Ok`（除非参数非法或缓存文件损坏需要用户介入），失败都记录在对应
/// provider 的 `fetch_error` / `error` 上。
pub fn subscription_view(provider: Option<&str>, force: bool) -> Result<SubscriptionView, String> {
    if let Some(id) = provider {
        if !PROVIDER_ORDER.contains(&id) {
            return Err(format!(
                "未知的用量供应商「{id}」。支持的取值：minimax_cn / minimax_en / deepseek / zai_coding_cn"
            ));
        }
    }
    let _guard = FETCH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scope = InstanceScope::resolve();
    let dsh_home = scope.dsh_home();
    let bindings = credentials::profile_bindings(&dsh_home.join("profiles").join(&scope.profile));
    let path = scope.cache_path();
    let mut doc: SubscriptionCacheDoc =
        state::load_checked(&path, cache_ctx()).map_err(|e| e.to_string())?;
    doc.schema = CACHE_SCHEMA;
    // 实例 / profile 变了：旧文档整体作废（里面是另一个账号的数据）。
    let binding = CacheInstanceBinding {
        family: scope.family.clone(),
        id: scope.id.clone(),
        profile: scope.profile.clone(),
    };
    let mut doc_dirty = false;
    if doc.instance.as_ref() != Some(&binding) && !doc.providers.is_empty() {
        doc.providers.clear();
        doc_dirty = true;
    }
    doc.instance = Some(binding);
    let now_ms = crate::process::epoch_millis();
    let ids: Vec<&str> = match provider {
        Some(id) => vec![id],
        None => PROVIDER_ORDER.to_vec(),
    };
    let mut providers = Vec::with_capacity(ids.len());
    for id in ids {
        let credential = credentials::resolve_provider(id, &bindings, &dsh_home);
        let configured = credential
            .as_ref()
            .map(|c| c.value.as_deref().is_some())
            .unwrap_or(false);
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            id,
            credential.clone(),
            doc.providers.get(id).cloned(),
            force,
            now_ms,
            // 智谱查询除 Key 外还要组织 / 项目上下文（同样从凭据链解析），
            // 因此把 dsh_home 传进 fetch；其它 provider 不用它。
            |id, key| fetch_provider(id, key, &dsh_home),
        );
        // 本次调用真实发生的失败才记日志：TTL 命中时的陈旧错误不重复落盘。
        let this_call_failure = fetch_error.clone().or_else(|| {
            if dirty {
                entry.as_ref().and_then(|e| e.error.clone())
            } else {
                None
            }
        });
        if dirty {
            match entry {
                Some(entry) => {
                    doc.providers.insert(id.to_string(), entry);
                }
                None => {
                    doc.providers.remove(id);
                }
            }
        }
        providers.push(build_provider_view(
            id,
            configured,
            doc.providers.get(id),
            fetch_error,
        ));
        doc_dirty |= dirty;
        // 失败落现有 Shell 日志（设计稿「错误文案口径」）：provider + 错误文案 +
        // 脱敏凭据提示（引用名 + 前 4 后 4），绝不包含凭据原文。
        if let Some(message) = this_call_failure {
            let credential_hint = credential
                .as_ref()
                .map(|c| {
                    let redacted = c.value.as_deref().map(redact_key).unwrap_or_default();
                    format!(" credential_ref={} credential={redacted}", c.reference)
                })
                .unwrap_or_default();
            log_subscription_line(format!(
                "subscription 查询 provider={id} 失败：{message}{credential_hint}"
            ));
        }
    }
    if doc_dirty {
        state::save(&path, &doc, cache_ctx()).map_err(|e| e.to_string())?;
    }
    Ok(SubscriptionView {
        instance: InstanceView {
            family: scope.family,
            id: scope.id,
            profile: scope.profile,
        },
        providers,
    })
}

/// 单个 provider 的缓存刷新决策（可注入 fetch 以便离线测试）。
///
/// 返回 `(新条目, 本次瞬时错误, 缓存是否需要写盘)`。`新条目 = None` 表示该
/// provider 不应再留有缓存条目（未配置凭据，或换凭据后旧条目作废且尚未
/// 查到新数据）。
fn refresh_provider_entry(
    id: &str,
    credential: Option<ResolvedCredential>,
    cached: Option<ProviderCacheEntry>,
    force: bool,
    now_ms: u64,
    fetch: impl FnOnce(&str, &str) -> FetchOutcome,
) -> (Option<ProviderCacheEntry>, Option<String>, bool) {
    let Some(credential) = credential else {
        return (None, None, false);
    };
    let Some(key) = credential.value.as_deref().map(str::to_string) else {
        // 凭据未配置（引用解析不出值）：不产生 / 不保留缓存条目。
        let dirty = cached.is_some();
        return (None, None, dirty);
    };
    let mut dirty = false;
    let cached = match cached {
        Some(entry) if entry_belongs_to_credential(&entry, &key) => Some(entry),
        // 换凭据：旧账号缓存一律按不存在处理，随后立即用新凭据查询。
        Some(_) => {
            dirty = true;
            None
        }
        None => None,
    };
    let mut fetch_error: Option<String> = None;
    // expired 条目不参与自动刷新：拿失效凭据反复打接口只会徒增失败；
    // 用户 force（点刷新 / 测试连接）永远重试。
    let needs_fetch = match &cached {
        None => true,
        Some(entry) => {
            force
                || entry.credential_status != CREDENTIAL_EXPIRED
                    && now_ms.saturating_sub(entry.queried_at_ms) >= CACHE_TTL_MS
        }
    };
    let entry = if needs_fetch {
        match fetch(id, &key) {
            FetchOutcome::Success(data) => {
                let mut next = entry_from_data(id, data);
                next.credential_ref = credential.reference.clone();
                next.credential_fingerprint = credential_fingerprint(&key);
                next.credential_status = CREDENTIAL_VALID.to_string();
                next.queried_at_ms = now_ms;
                next.error = None;
                dirty = true;
                Some(next)
            }
            FetchOutcome::Deterministic(message, expired) => {
                // 确定性失败：旧 tiers/balances 原样保留，只更新状态与错误。
                let mut next = cached.unwrap_or_default();
                next.credential_ref = credential.reference.clone();
                next.credential_fingerprint = credential_fingerprint(&key);
                next.credential_status = if expired {
                    CREDENTIAL_EXPIRED.to_string()
                } else {
                    CREDENTIAL_VALID.to_string()
                };
                next.error = Some(message);
                dirty = true;
                Some(next)
            }
            FetchOutcome::Transient(message) => {
                // 瞬时失败：缓存不写、不删，keep-last-good。
                fetch_error = Some(message);
                cached
            }
        }
    } else {
        cached
    };
    (entry, fetch_error, dirty)
}

/// 缓存条目是否属于这把凭据（指纹匹配）。
fn entry_belongs_to_credential(entry: &ProviderCacheEntry, key: &str) -> bool {
    entry.credential_fingerprint == credential_fingerprint(key)
}

fn build_provider_view(
    id: &str,
    configured: bool,
    entry: Option<&ProviderCacheEntry>,
    fetch_error: Option<String>,
) -> ProviderView {
    let empty = ProviderCacheEntry::default();
    let entry = entry.unwrap_or(&empty);
    ProviderView {
        id: id.to_string(),
        label: provider_label(id),
        kind: if configured {
            Some(provider_kind(id))
        } else {
            None
        },
        configured,
        credential_ref: if configured && !entry.credential_ref.is_empty() {
            Some(entry.credential_ref.clone())
        } else {
            None
        },
        credential_status: if configured {
            Some(entry.credential_status.clone())
        } else {
            None
        },
        queried_at_ms: if configured && entry.queried_at_ms > 0 {
            Some(entry.queried_at_ms)
        } else {
            None
        },
        error: entry.error.clone(),
        fetch_error,
        is_available: entry.is_available,
        tiers: entry
            .tiers
            .iter()
            .map(|tier| TierView {
                name: tier.name.clone(),
                remaining_percent: tier.remaining_percent,
                resets_at_ms: tier.resets_at_ms,
            })
            .collect(),
        balances: entry
            .balances
            .iter()
            .map(|balance| BalanceView {
                currency: balance.currency.clone(),
                total: balance.total.clone(),
                granted: balance.granted.clone(),
                topped_up: balance.topped_up.clone(),
            })
            .collect(),
    }
}

// --- 查询 ---------------------------------------------------------------------

enum FetchOutcome {
    /// 查询成功：写入 / 覆盖缓存。
    Success(ProviderData),
    /// 确定性失败（凭据失效 / 业务错误码 / 结构不认识）：保留旧数据，记录
    /// 错误。第二个字段 = 凭据是否已失效（expired 条目跳过自动刷新）。
    Deterministic(String, bool),
    /// 瞬时失败（网络不可达 / 超时 / 读体中断）：缓存不写、不删。
    Transient(String),
}

enum ProviderData {
    Plan {
        tiers: Vec<CacheTier>,
    },
    Balance {
        is_available: bool,
        balances: Vec<CacheBalance>,
    },
}

fn entry_from_data(id: &str, data: ProviderData) -> ProviderCacheEntry {
    let mut entry = ProviderCacheEntry {
        kind: provider_kind(id).to_string(),
        ..ProviderCacheEntry::default()
    };
    match data {
        ProviderData::Plan { tiers } => entry.tiers = tiers,
        ProviderData::Balance {
            is_available,
            balances,
        } => {
            entry.is_available = Some(is_available);
            entry.balances = balances;
        }
    }
    entry
}

fn fetch_provider(id: &str, key: &str, dsh_home: &std::path::Path) -> FetchOutcome {
    match id {
        PROVIDER_DEEPSEEK => fetch_deepseek(id, key),
        PROVIDER_ZAI_CODING_CN => fetch_zhipu(id, key, dsh_home),
        _ => fetch_minimax(id, key),
    }
}

fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .build()
            .new_agent()
    })
}

/// GET 一个 Bearer 认证的 JSON 接口，返回响应文本。
///
/// 这是原始凭据被允许出现的两处之一（HTTPS 请求头）；任何错误信息、日志、
/// 缓存都不得携带它。非 2xx 由 ureq 的默认 `http_status_as_error` 转成
/// [`ureq::Error::StatusCode`]，在此按状态码分类。
/// HTTP 401/403 的凭据失效文案：按 provider 定制。MiniMax 的「订阅 Key /
/// 按量 Key」辨析只对 MiniMax 有意义，不塞给 DeepSeek 用户（真机截图反馈
/// 过这处文案错位）。
fn credential_rejected_message(provider: &str, status: u16) -> String {
    if provider == PROVIDER_DEEPSEEK {
        format!(
            "DeepSeek 凭据无效或无权限（HTTP {status}）。请到工作台的模型设置更新 DeepSeek 的 API Key"
        )
    } else if provider == PROVIDER_ZAI_CODING_CN {
        format!(
            "智谱凭据无效或无权限（HTTP {status}）。请到工作台的模型设置更新 zai-coding-cn 的 API Key"
        )
    } else {
        let label = if provider == PROVIDER_MINIMAX_EN {
            "MiniMax（国际站）"
        } else {
            "MiniMax（国内站）"
        };
        format!(
            "{label}凭据无效或无权限（HTTP {status}）。请到工作台的模型设置更新对应 provider 的              API Key（注意：查询套餐需用 Token Plan 页的订阅 Key，不是接口密钥页的按量 Key）"
        )
    }
}

fn http_get_json(provider: &str, url: &str, key: &str) -> Result<String, FetchOutcome> {
    http_get_json_ext(provider, url, key, true, &[])
}

/// [`http_get_json`] 的扩展形态：智谱的额度接口用 raw API key（不带
/// `Bearer`）且要求组织 / 项目上下文头，其余 provider 走默认 Bearer 形态。
fn http_get_json_ext(
    provider: &str,
    url: &str,
    key: &str,
    bearer: bool,
    extra_headers: &[(&str, &str)],
) -> Result<String, FetchOutcome> {
    // Authorization 头是凭据的合法去处（另一处是凭据文件本身的只读解析）。
    let auth_value = if bearer {
        format!("Bearer {key}")
    } else {
        key.to_string()
    };
    let mut request = http_agent()
        .get(url)
        .header("Authorization", &auth_value)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    for &(name, value) in extra_headers {
        request = request.header(name, value);
    }
    match request.call() {
        Ok(mut response) => response
            .body_mut()
            .with_config()
            .limit(MAX_HTTP_BODY_BYTES)
            .read_to_string()
            .map_err(|e| {
                FetchOutcome::Transient(format!(
                    "读取响应失败（{e}）。已保留上次结果，可点击刷新重试"
                ))
            }),
        Err(ureq::Error::StatusCode(status)) => {
            if status == 401 || status == 403 {
                Err(FetchOutcome::Deterministic(
                    credential_rejected_message(provider, status),
                    true,
                ))
            } else {
                Err(FetchOutcome::Transient(format!(
                    "服务返回 HTTP {status}。已保留上次结果，稍后可重试"
                )))
            }
        }
        Err(e) => Err(FetchOutcome::Transient(format!(
            "查询失败（网络不可达或超时：{e}）。已保留上次结果，可点击刷新重试"
        ))),
    }
}

/// 把一行查询事件写进现有 Shell 日志（`<shell logs>/<kind>-subscription-<日期>.log`）。
///
/// 设计要求：查询事件与错误可落日志供「查看日志」反馈，但**绝不写入原始
/// 凭据或凭据文件内容**——调用方只允许传 provider、结果分类、HTTP 状态、
/// 脱敏提示与截断后的响应摘要。写日志失败静默（诊断不能掩盖查询结果）。
fn log_subscription_line(line: String) {
    let spec = crate::process::LogSpec::new(crate::process::build_log_kind(), "subscription");
    let mut log = match crate::process::RotatingLog::new(&crate::process::shell_logs_dir(), spec) {
        Ok(log) => log,
        Err(_) => return,
    };
    let _ = log.write_line(&line);
}

/// 结构不认识的响应：把截断摘要写进日志（≤200 字符，去换行）供定位改版。
fn log_unrecognized_structure(provider: &str, body: &str) {
    let snippet: String = body
        .chars()
        .take(LOG_RESPONSE_SNIPPET_BYTES)
        .collect::<String>()
        .replace(['\n', '\r'], " ");
    log_subscription_line(format!(
        "subscription 查询 provider={provider} outcome=unrecognized-structure 响应摘要={snippet}"
    ));
}

fn fetch_minimax(provider: &str, key: &str) -> FetchOutcome {
    let is_cn = provider == PROVIDER_MINIMAX_CN;
    let base = if is_cn {
        MINIMAX_CN_ENDPOINT
    } else {
        MINIMAX_EN_ENDPOINT
    };
    let body = match http_get_json(provider, &format!("{base}{MINIMAX_REMAINS_PATH}"), key) {
        Ok(body) => body,
        Err(outcome) => return outcome,
    };
    match parse_minimax_tiers(&body) {
        Ok(tiers) => FetchOutcome::Success(ProviderData::Plan { tiers }),
        // 业务错误码与结构不认识都是确定性失败；只有 HTTP 401/403 判凭据失效
        // （status_msg 语义不可靠，宁可多试一次也不要把好凭据错标成 expired）。
        // 结构不认识时把截断摘要写进日志供反馈（不进错误横幅，避免刷屏）。
        Err(message) => {
            log_unrecognized_structure(provider, &body);
            FetchOutcome::Deterministic(message, false)
        }
    }
}

fn fetch_deepseek(provider: &str, key: &str) -> FetchOutcome {
    let body = match http_get_json(provider, DEEPSEEK_BALANCE_ENDPOINT, key) {
        Ok(body) => body,
        Err(outcome) => return outcome,
    };
    match parse_deepseek_balances(&body) {
        Ok((is_available, balances)) => FetchOutcome::Success(ProviderData::Balance {
            is_available,
            balances,
        }),
        Err(message) => {
            log_unrecognized_structure(PROVIDER_DEEPSEEK, &body);
            FetchOutcome::Deterministic(message, false)
        }
    }
}

// --- 智谱（bigmodel.cn）编程套餐 ----------------------------------------------

/// 智谱额度查询的上下文。当前版本**仅支持个人套餐**（type=1，缺省）：
/// 个人查询只需 Key，不需要组织 / 项目头——那两样是团队套餐的要件，
/// 未配置就不随请求发送。`ZAI_CODING_CN_PLAN_TYPE` 若显式配成 2（团队）
/// 会得到「仅支持个人套餐」的确定性提示，而不是一屏空数据让用户猜原因。
/// 与 Key 走同一条凭据解析链配置（环境变量 → `.credentials.yaml` refs →
/// `.env`）。
#[derive(Debug)]
struct ZhipuContext {
    organization: Option<String>,
    project: Option<String>,
    plan_type: String,
}

impl ZhipuContext {
    /// 从凭据解析链解析上下文；组织 / 项目缺失不是错误。
    fn resolve(dsh_home: &std::path::Path) -> Result<Self, String> {
        let organization = credentials::resolve(credentials::ZAI_ORGANIZATION_REF, dsh_home)
            .value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let project = credentials::resolve(credentials::ZAI_PROJECT_REF, dsh_home)
            .value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let configured_type = credentials::resolve(credentials::ZAI_PLAN_TYPE_REF, dsh_home)
            .value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if configured_type.as_deref() == Some("2") {
            return Err("当前版本仅支持智谱个人套餐（type=1），团队套餐暂不支持；\
                 如需团队套餐支持请到项目仓库反馈"
                .to_string());
        }
        Ok(Self {
            organization,
            project,
            plan_type: configured_type.unwrap_or_else(|| ZHIPU_PLAN_TYPE_DEFAULT.to_string()),
        })
    }
}

fn fetch_zhipu(provider: &str, key: &str, dsh_home: &std::path::Path) -> FetchOutcome {
    let context = match ZhipuContext::resolve(dsh_home) {
        Ok(context) => context,
        // 套餐类型不受支持是确定性失败：Key 本身有效，不标 expired（照常
        // TTL 重试，版本支持团队后无需手动刷新也能恢复）。
        Err(message) => return FetchOutcome::Deterministic(message, false),
    };
    let url = format!("{}?type={}", ZHIPU_QUOTA_ENDPOINT, context.plan_type);
    // 组织 / 项目头是团队套餐要件；当前版本仅支持个人套餐，未配置就不发送。
    let mut extra_headers: Vec<(&str, &str)> = Vec::new();
    if let (Some(org), Some(project)) = (&context.organization, &context.project) {
        extra_headers.push(("bigmodel-organization", org.as_str()));
        extra_headers.push(("bigmodel-project", project.as_str()));
    }
    let body = match http_get_json_ext(provider, &url, key, false, &extra_headers) {
        Ok(body) => body,
        Err(outcome) => return outcome,
    };
    match parse_zhipu_tiers(&body) {
        Ok(tiers) => FetchOutcome::Success(ProviderData::Plan { tiers }),
        Err(message) => {
            log_unrecognized_structure(provider, &body);
            FetchOutcome::Deterministic(message, false)
        }
    }
}

/// 解析智谱 `/api/monitor/usage/quota/limit` 响应为 tier 列表。
///
/// 只取 `data.limits[]` 中 `type == "CREDIT_LIMIT"` 的条目：`unit == 3` 是
/// 5 小时滚动窗口、`unit == 6` 是周窗口；`percentage` 是**已用**百分比
/// （0-100，与 MiniMax 的剩余口径相反），`nextResetTime` 为 epoch 毫秒。
/// `data` 缺失 / 非对象：先看顶层 `msg` / `message` 业务错误文案，否则按
/// 结构不认识处理；`limits` 缺失或空数组返回空列表（UI 显示「暂无额度数据」）。
fn parse_zhipu_tiers(body: &str) -> Result<Vec<CacheTier>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| format!("接口返回了无法解析的数据（非 JSON）。{UNRECOGNIZED_STRUCTURE}"))?;
    let data = match value.get("data") {
        Some(serde_json::Value::Object(map)) => map,
        Some(serde_json::Value::Null) | None => {
            // 缺 type 参数等业务拒绝：顶层带可读文案时优先透出。
            let message = ["msg", "message", "errorMessage"]
                .iter()
                .find_map(|field| opt_string(value.get(field)))
                .unwrap_or_else(|| UNRECOGNIZED_STRUCTURE.to_string());
            return Err(message);
        }
        Some(_) => return Err(UNRECOGNIZED_STRUCTURE.to_string()),
    };
    let limits = match data.get("limits") {
        None => return Ok(Vec::new()),
        Some(serde_json::Value::Array(items)) => items,
        Some(_) => return Err(UNRECOGNIZED_STRUCTURE.to_string()),
    };
    let mut tiers = Vec::new();
    for limit in limits {
        if limit.get("type").and_then(|v| v.as_str()) != Some("CREDIT_LIMIT") {
            continue;
        }
        // 已用百分比 → 剩余百分比（与 MiniMax / 前端进度条口径统一）。
        let Some(used) = limit.get("percentage").and_then(|v| v.as_f64()) else {
            continue;
        };
        let (name, sort_key) = match limit.get("unit").and_then(|v| v.as_i64()) {
            Some(3) => ("5h", 0),
            Some(6) => ("weekly", 1),
            _ => continue,
        };
        tiers.push((
            sort_key,
            CacheTier {
                name: name.to_string(),
                remaining_percent: clamp_percent(100.0 - used),
                resets_at_ms: limit.get("nextResetTime").and_then(parse_epoch_ms),
            },
        ));
    }
    tiers.sort_by_key(|(sort_key, _)| *sort_key);
    Ok(tiers.into_iter().map(|(_, tier)| tier).collect())
}

// --- 解析（纯函数，单测覆盖） --------------------------------------------------

/// 结构不认识的统一文案：用户能做的动作是反馈，不是重试。
const UNRECOGNIZED_STRUCTURE: &str = "接口返回了无法识别的数据结构，可能已改版。请到项目仓库反馈";

/// 解析 MiniMax `/coding_plan/remains` 响应为 tier 列表。
///
/// 只取 `model_remains[]` 中 `model_name == "general"` 的条目（编程套餐）；
/// 周层仅当 `current_weekly_status == 1` 时激活（`status == 3` 表示无周限额，
/// 不产出周条目，避免渲染一条永远满格的假数据）；空数组 / 字段缺失返回空
/// 列表（UI 显示「未查询到套餐额度」），结构类型漂移才报错。
fn parse_minimax_tiers(body: &str) -> Result<Vec<CacheTier>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| format!("接口返回了无法解析的数据（非 JSON）。{UNRECOGNIZED_STRUCTURE}"))?;
    if let Some(base) = value.get("base_resp") {
        let status_code = base
            .get("status_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if status_code != 0 {
            let message = base
                .get("status_msg")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("未知错误");
            return Err(format!("MiniMax 返回业务错误（{status_code}）：{message}"));
        }
    }
    let remains = match value.get("model_remains") {
        None => return Ok(Vec::new()),
        Some(serde_json::Value::Array(items)) => items,
        Some(_) => return Err(UNRECOGNIZED_STRUCTURE.to_string()),
    };
    let item = match remains
        .iter()
        .find(|item| item.get("model_name").and_then(|v| v.as_str()) == Some("general"))
    {
        Some(item) => item,
        None => return Ok(Vec::new()),
    };
    let mut tiers = Vec::new();
    if let Some(percent) = item
        .get("current_interval_remaining_percent")
        .and_then(|v| v.as_f64())
    {
        tiers.push(CacheTier {
            name: "5h".to_string(),
            remaining_percent: clamp_percent(percent),
            resets_at_ms: item.get("end_time").and_then(parse_epoch_ms),
        });
    }
    if item.get("current_weekly_status").and_then(|v| v.as_i64()) == Some(1) {
        if let Some(percent) = item
            .get("current_weekly_remaining_percent")
            .and_then(|v| v.as_f64())
        {
            tiers.push(CacheTier {
                name: "weekly".to_string(),
                remaining_percent: clamp_percent(percent),
                resets_at_ms: item.get("weekly_end_time").and_then(parse_epoch_ms),
            });
        }
    }
    Ok(tiers)
}

/// 解析 DeepSeek `/user/balance` 响应为 `(is_available, 余额行列表)`。
/// `balance_infos[]` 逐条透传；金额必须是字符串（数字字符串透传展示，
/// 类型漂移的条目整条跳过而不是整体失败）。
fn parse_deepseek_balances(body: &str) -> Result<(bool, Vec<CacheBalance>), String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| format!("接口返回了无法解析的数据（非 JSON）。{UNRECOGNIZED_STRUCTURE}"))?;
    let is_available = value
        .get("is_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let balances = match value.get("balance_infos") {
        None => Vec::new(),
        Some(serde_json::Value::Array(items)) => items.iter().filter_map(parse_balance).collect(),
        Some(_) => return Err(UNRECOGNIZED_STRUCTURE.to_string()),
    };
    Ok((is_available, balances))
}

fn parse_balance(item: &serde_json::Value) -> Option<CacheBalance> {
    let currency = item.get("currency").and_then(|v| v.as_str())?.trim();
    if currency.is_empty() {
        return None;
    }
    let total = item.get("total_balance").and_then(|v| v.as_str())?.trim();
    if total.is_empty() {
        return None;
    }
    Some(CacheBalance {
        currency: currency.to_string(),
        total: total.to_string(),
        granted: opt_string(item.get("granted_balance")),
        topped_up: opt_string(item.get("topped_up_balance")),
    })
}

fn opt_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn clamp_percent(percent: f64) -> f64 {
    if percent.is_nan() {
        0.0
    } else {
        percent.clamp(0.0, 100.0)
    }
}

/// 时间戳容错解析：数字（毫秒为主，秒级自动放大）或 ISO 8601 字符串。
fn parse_epoch_ms(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().map(|f| f.max(0.0) as u64))
            .map(normalize_epoch_ms),
        serde_json::Value::String(text) => {
            let text = text.trim();
            if let Ok(ms) = text.parse::<u64>() {
                return Some(normalize_epoch_ms(ms));
            }
            time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
                .ok()
                .map(|dt| dt.unix_timestamp().max(0) as u64 * 1000)
        }
        _ => None,
    }
}

/// 秒级时间戳（< 1e11）按毫秒口径放大；更大的值原样返回。
fn normalize_epoch_ms(ms: u64) -> u64 {
    if ms < 100_000_000_000 {
        ms.saturating_mul(1000)
    } else {
        ms
    }
}

// --- Tauri 命令 ---------------------------------------------------------------

/// `get_subscription_usage`：面板 / 独立窗口读取云端套餐用量。
///
/// 重活在 `spawn_blocking` 里做：网络查询最长 15 秒，绝不能占 Tauri 主线程。
#[tauri::command]
pub async fn get_subscription_usage(
    provider: Option<String>,
    force: Option<bool>,
) -> Result<SubscriptionView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        subscription_view(provider.as_deref(), force.unwrap_or(false))
    })
    .await
    .map_err(|join| {
        format!(
            "后台任务异常结束（{join}）。请重试；若持续出现，请在终端用 `npm run dev` 启动以便看到完整输出"
        )
    })?
}

/// 在独立可缩放窗口中打开套餐用量（与 [`crate::usage::open_usage_window`]
/// 同一条路：webview 在新线程上构建，Windows 主线程同步建 webview 会死锁；
/// 已有窗口先销毁再重建——窗口只读，重建即顺手拿到一次 force 刷新）。
#[tauri::command]
pub async fn open_subscription_window(app: tauri::AppHandle) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("dsh-open-subscription-viewer".into())
        .spawn(move || {
            let label = crate::window::SUBSCRIPTION_VIEWER_LABEL;
            if let Some(existing) = handle.get_webview_window(label) {
                let _ = existing.destroy();
            }
            let backdrop = crate::commands::chrome_backdrop(&handle);
            let dock = handle.get_webview_window("main").and_then(|main| {
                crate::window::dock_position_logical(&main, crate::window::USAGE_VIEWER_SIZE)
            });
            let mut builder = tauri::WebviewWindowBuilder::new(
                &handle,
                label,
                tauri::WebviewUrl::App("index.html?subscription=1".into()),
            )
            .title("套餐用量")
            .inner_size(
                crate::window::USAGE_VIEWER_SIZE.width,
                crate::window::USAGE_VIEWER_SIZE.height,
            )
            .min_inner_size(720.0, 520.0)
            .resizable(true)
            .background_color(backdrop);
            if let Some((x, y)) = dock {
                builder = builder.position(x, y);
            }
            let result = builder
                .build()
                .map(|_| ())
                .map_err(|e| format!("打开套餐用量窗口失败：{e}。请重试"));
            let _ = tx.send(result);
        })
        .map_err(|e| format!("无法启动套餐用量窗口线程：{e}"))?;

    tauri::async_runtime::spawn_blocking(move || {
        match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(result) => result,
            Err(_) => Err("打开套餐用量窗口超时（20 秒）。请重试".into()),
        }
    })
    .await
    .map_err(|join| format!("后台任务异常结束（{join}）。请重试"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- redact_key -----------------------------------------------------------

    #[test]
    fn redact_key_keeps_head_and_tail_only() {
        assert_eq!(redact_key("abcdefghijklmn"), "abcd****klmn");
        assert_eq!(redact_key("  eyJhbGciOiJIUzI1NiJ9  "), "eyJh****NiJ9");
    }

    #[test]
    fn redact_key_masks_short_keys_entirely() {
        assert_eq!(redact_key("short"), "****");
        assert_eq!(
            redact_key("exactly-12!"),
            "****",
            "12 字符按「短于 12」的边界外，全打码"
        );
        assert_eq!(redact_key("0123456789ab"), "0123****89ab");
        assert_eq!(redact_key(""), "****");
    }

    // --- 指纹 -----------------------------------------------------------------

    #[test]
    fn credential_fingerprint_is_stable_and_discriminating() {
        let first = credential_fingerprint("sk-abc-123");
        assert_eq!(
            first,
            credential_fingerprint("  sk-abc-123  "),
            "trim 后指纹一致"
        );
        assert_ne!(
            first,
            credential_fingerprint("sk-abc-124"),
            "不同凭据指纹必须不同"
        );
        assert!(!first.contains("sk-abc"), "指纹不得包含凭据原文片段");
        assert_eq!(first.len(), 16, "只取 SHA-256 前 16 个 hex 字符");
    }

    #[test]
    fn cache_entry_binds_to_its_credential() {
        let entry = ProviderCacheEntry {
            credential_fingerprint: credential_fingerprint("key-A"),
            ..ProviderCacheEntry::default()
        };
        assert!(entry_belongs_to_credential(&entry, "key-A"));
        assert!(!entry_belongs_to_credential(&entry, "key-B"));
    }

    // --- MiniMax 解析 ----------------------------------------------------------

    /// 设计文档里的响应样张：双窗口都激活。
    const MINIMAX_SAMPLE: &str = r#"{
        "model_remains": [
            { "model_name": "video", "current_interval_remaining_percent": 10.0 },
            {
                "model_name": "general",
                "current_interval_remaining_percent": 73.2,
                "current_weekly_status": 1,
                "current_weekly_remaining_percent": 41.5,
                "end_time": 1761308400000,
                "weekly_end_time": 1761900000000
            }
        ],
        "base_resp": { "status_code": 0, "status_msg": "success" }
    }"#;

    #[test]
    fn minimax_sample_parses_both_windows_and_skips_other_models() {
        let tiers = parse_minimax_tiers(MINIMAX_SAMPLE).expect("样张应解析成功");
        assert_eq!(tiers.len(), 2, "video 条目必须跳过");
        assert_eq!(tiers[0].name, "5h");
        assert_eq!(tiers[0].remaining_percent, 73.2);
        assert_eq!(tiers[0].resets_at_ms, Some(1_761_308_400_000));
        assert_eq!(tiers[1].name, "weekly");
        assert_eq!(tiers[1].remaining_percent, 41.5);
        assert_eq!(tiers[1].resets_at_ms, Some(1_761_900_000_000));
    }

    #[test]
    fn minimax_weekly_status_3_means_no_weekly_tier() {
        let body = r#"{
            "model_remains": [
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 88,
                    "current_weekly_status": 3,
                    "current_weekly_remaining_percent": 100
                }
            ],
            "base_resp": { "status_code": 0 }
        }"#;
        let tiers = parse_minimax_tiers(body).expect("应解析成功");
        assert_eq!(
            tiers.len(),
            1,
            "status == 3 的周层不得产出（恒满格的假数据）"
        );
        assert_eq!(tiers[0].name, "5h");
    }

    #[test]
    fn minimax_empty_or_missing_remains_yields_empty_tiers_not_error() {
        assert!(
            parse_minimax_tiers(r#"{"model_remains": [], "base_resp": {"status_code": 0}}"#)
                .expect("空数组不报错")
                .is_empty()
        );
        assert!(parse_minimax_tiers(r#"{"base_resp": {"status_code": 0}}"#)
            .expect("字段缺失不报错")
            .is_empty());
        // general 条目不在时不报错。
        let body =
            r#"{"model_remains": [{"model_name": "video"}], "base_resp": {"status_code": 0}}"#;
        assert!(parse_minimax_tiers(body)
            .expect("无 general 不报错")
            .is_empty());
    }

    #[test]
    fn minimax_business_error_is_deterministic_failure() {
        let body = r#"{"base_resp": {"status_code": 1004, "status_msg": "invalid api key"}}"#;
        let err = parse_minimax_tiers(body).expect_err("业务错误必须报错");
        assert!(
            err.contains("1004") && err.contains("invalid api key"),
            "文案要带状态与原因：{err}"
        );
    }

    #[test]
    fn minimax_type_drift_is_reported_instead_of_panicking() {
        let body = r#"{"model_remains": {"general": 1}, "base_resp": {"status_code": 0}}"#;
        let err = parse_minimax_tiers(body).expect_err("类型漂移必须报错");
        assert!(err.contains("无法识别"), "{err}");
    }

    #[test]
    fn minimax_field_drift_skips_fields_but_keeps_tier() {
        // percent 变成了字符串、时间戳缺失：该字段跳过，5h 条目仍在。
        let body = r#"{
            "model_remains": [
                { "model_name": "general", "current_interval_remaining_percent": "73.2" }
            ],
            "base_resp": {"status_code": 0}
        }"#;
        let tiers = parse_minimax_tiers(body).expect("防御式解析不应整体失败");
        assert!(tiers.is_empty(), "percent 类型不合时没有可展示的 tier");
    }

    #[test]
    fn minimax_accepts_iso_string_end_time() {
        let body = r#"{
            "model_remains": [
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 50,
                    "end_time": "2025-10-24T15:00:00Z"
                }
            ],
            "base_resp": {"status_code": 0}
        }"#;
        let tiers = parse_minimax_tiers(body).expect("应解析成功");
        assert_eq!(tiers[0].resets_at_ms, Some(1_761_318_000_000));
    }

    // --- DeepSeek 解析 ---------------------------------------------------------

    /// 官方文档样张（多币种）。
    const DEEPSEEK_SAMPLE: &str = r#"{
        "is_available": true,
        "balance_infos": [
            { "currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00" },
            { "currency": "USD", "total_balance": "5.21", "granted_balance": "0.00", "topped_up_balance": "5.21" }
        ]
    }"#;

    #[test]
    fn deepseek_sample_keeps_all_currencies_and_strings() {
        let (is_available, balances) =
            parse_deepseek_balances(DEEPSEEK_SAMPLE).expect("样张应解析成功");
        assert!(is_available);
        assert_eq!(balances.len(), 2, "多币种逐条保留");
        assert_eq!(balances[0].currency, "CNY");
        assert_eq!(balances[0].total, "110.00", "金额是字符串，透传");
        assert_eq!(balances[0].granted.as_deref(), Some("10.00"));
        assert_eq!(balances[0].topped_up.as_deref(), Some("100.00"));
        assert_eq!(balances[1].currency, "USD");
        assert_eq!(
            balances[1].granted.as_deref(),
            Some("0.00"),
            "0.00 也透传，由 UI 决定展示"
        );
    }

    #[test]
    fn deepseek_unavailable_flag_is_preserved() {
        let body = r#"{"is_available": false, "balance_infos": [{"currency": "CNY", "total_balance": "0.50"}]}"#;
        let (is_available, balances) = parse_deepseek_balances(body).expect("应解析成功");
        assert!(!is_available, "余额不足标记必须透传");
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].granted, None, "缺字段跳过而不是报错");
    }

    #[test]
    fn deepseek_type_drift_skips_entry_and_reports_structure_change() {
        // total_balance 变成数字：整条跳过（防御式），但其余条目保留。
        let body = r#"{"is_available": true, "balance_infos": [
            {"currency": "CNY", "total_balance": 110},
            {"currency": "USD", "total_balance": "5.21"}
        ]}"#;
        let (_, balances) = parse_deepseek_balances(body).expect("单条漂移不整体失败");
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].currency, "USD");

        let broken = r#"{"is_available": true, "balance_infos": {"currency": "CNY"}}"#;
        let err = parse_deepseek_balances(broken).expect_err("类型漂移必须报错");
        assert!(err.contains("无法识别"), "{err}");
    }

    // --- 智谱解析 ---------------------------------------------------------------

    /// 社区实现（pi-glm-quota）实测的响应形态：limits 里混有非 CREDIT_LIMIT
    /// 条目与未知 unit，均须跳过；percentage 是已用口径。
    const ZHIPU_SAMPLE: &str = r#"{
        "data": {
            "limits": [
                { "type": "OTHER_LIMIT", "unit": 3, "percentage": 10 },
                { "type": "CREDIT_LIMIT", "unit": 6, "percentage": 48.0, "nextResetTime": 1761900000000 },
                { "type": "CREDIT_LIMIT", "unit": 3, "percentage": 12.0, "nextResetTime": 1761308400000 },
                { "type": "CREDIT_LIMIT", "unit": 9, "percentage": 30 }
            ]
        }
    }"#;

    #[test]
    fn zhipu_sample_converts_used_percent_and_orders_windows() {
        let tiers = parse_zhipu_tiers(ZHIPU_SAMPLE).expect("样张应解析成功");
        assert_eq!(tiers.len(), 2, "非 CREDIT_LIMIT 与未知 unit 条目必须跳过");
        assert_eq!(tiers[0].name, "5h", "5 小时窗口排在周窗口前");
        assert_eq!(
            tiers[0].remaining_percent, 88.0,
            "percentage 是已用口径，取 100 - 已用"
        );
        assert_eq!(tiers[0].resets_at_ms, Some(1_761_308_400_000));
        assert_eq!(tiers[1].name, "weekly");
        assert_eq!(tiers[1].remaining_percent, 52.0);
    }

    #[test]
    fn zhipu_empty_or_missing_limits_yields_empty_tiers_not_error() {
        assert!(parse_zhipu_tiers(r#"{"data": {}}"#)
            .expect("空 data 不报错")
            .is_empty());
        assert!(parse_zhipu_tiers(r#"{"data": {"limits": []}}"#)
            .expect("空数组不报错")
            .is_empty());
    }

    #[test]
    fn zhipu_business_message_is_surfaced_deterministically() {
        // 缺 type 参数时的「用户不存在 coding plan」类业务拒绝：文案透出。
        let err = parse_zhipu_tiers(r#"{"msg": "当前用户不存在coding plan"}"#)
            .expect_err("业务错误必须报错");
        assert!(err.contains("不存在"), "{err}");
        // data 类型漂移：按结构不认识处理。
        let err = parse_zhipu_tiers(r#"{"data": []}"#).expect_err("类型漂移必须报错");
        assert!(err.contains("无法识别"), "{err}");
    }

    #[test]
    fn zhipu_personal_plan_needs_no_org_project_context() {
        // 当前版本仅支持个人套餐：org / project 未配置不是错误，type 缺省 1。
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-zhipu-ctx-{}-{}",
            std::process::id(),
            crate::process::epoch_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let context = ZhipuContext::resolve(&dir).expect("个人套餐无需组织 / 项目上下文");
        assert_eq!(context.organization, None);
        assert_eq!(context.project, None);
        assert_eq!(context.plan_type, "1", "个人套餐是缺省 type");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zhipu_team_plan_type_is_rejected_with_actionable_copy() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-zhipu-team-{}-{}",
            std::process::id(),
            crate::process::epoch_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.yaml"),
            "version: 1\nrefs:\n  ZAI_CODING_CN_PLAN_TYPE: 2\n",
        )
        .unwrap();
        let err = ZhipuContext::resolve(&dir).expect_err("团队套餐必须得到明确文案");
        assert!(err.contains("个人套餐"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zhipu_context_reads_refs_and_defaults_plan_type() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-zhipu-ctx-ok-{}-{}",
            std::process::id(),
            crate::process::epoch_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".credentials.yaml"),
            "version: 1\nrefs:\n  ZAI_CODING_CN_ORGANIZATION: org-test\n  ZAI_CODING_CN_PROJECT: proj-test\n",
        )
        .unwrap();
        let context = ZhipuContext::resolve(&dir).expect("refs 齐全应解析成功");
        assert_eq!(context.organization.as_deref(), Some("org-test"));
        assert_eq!(context.project.as_deref(), Some("proj-test"));
        assert_eq!(context.plan_type, "1", "个人套餐是缺省 type");
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- 时间戳容错 -------------------------------------------------------------

    #[test]
    fn epoch_ms_accepts_seconds_numbers_and_numeric_strings() {
        assert_eq!(
            parse_epoch_ms(&serde_json::json!(1761308400000u64)),
            Some(1_761_308_400_000)
        );
        assert_eq!(
            parse_epoch_ms(&serde_json::json!(1761308400u64)),
            Some(1_761_308_400_000),
            "秒级自动放大"
        );
        assert_eq!(
            parse_epoch_ms(&serde_json::json!("1761308400000")),
            Some(1_761_308_400_000)
        );
        assert_eq!(parse_epoch_ms(&serde_json::json!(null)), None);
        assert_eq!(parse_epoch_ms(&serde_json::json!("not-a-time")), None);
    }
}

/// `refresh_provider_entry` 的离线决策测试：fetch 闭包注入假结果，覆盖
/// 「成功写入 / 确定性失败保数据 / 瞬时失败不动缓存 / 换凭据作废」四条
/// 主路径——这些决定了用户看到的数字是否可信。
#[cfg(test)]
mod refresh_tests {
    use super::*;
    use crate::credentials::CredentialSource;

    const NOW_MS: u64 = 1_761_308_400_000;

    fn resolved_credential(value: Option<&str>) -> ResolvedCredential {
        ResolvedCredential {
            reference: "MINIMAX_CN_API_KEY".to_string(),
            value: value.map(str::to_string),
            source: CredentialSource::CredentialsYaml,
        }
    }

    fn cached_entry(fingerprint: &str, queried_at_ms: u64) -> ProviderCacheEntry {
        ProviderCacheEntry {
            kind: "balance".to_string(),
            credential_status: CREDENTIAL_VALID.to_string(),
            queried_at_ms,
            balances: vec![CacheBalance {
                currency: "CNY".to_string(),
                total: "110.00".to_string(),
                granted: Some("10.00".to_string()),
                topped_up: Some("100.00".to_string()),
            }],
            credential_ref: "MINIMAX_CN_API_KEY".to_string(),
            credential_fingerprint: fingerprint.to_string(),
            ..ProviderCacheEntry::default()
        }
    }

    /// 永不触发的 fetch 闭包：决策路径碰它即测试失败。
    fn no_fetch() -> impl Fn(&str, &str) -> FetchOutcome {
        |_, _| panic!("测试不应触发真实查询")
    }

    #[test]
    fn success_overwrites_cache_and_clears_error() {
        let mut cached = cached_entry(&credential_fingerprint("key"), NOW_MS - CACHE_TTL_MS);
        cached.error = Some("旧错误".to_string());
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            PROVIDER_DEEPSEEK,
            Some(resolved_credential(Some("key"))),
            Some(cached),
            false,
            NOW_MS,
            |_, _| {
                FetchOutcome::Success(ProviderData::Balance {
                    is_available: true,
                    balances: vec![CacheBalance {
                        currency: "USD".to_string(),
                        total: "5.21".to_string(),
                        granted: None,
                        topped_up: Some("5.21".to_string()),
                    }],
                })
            },
        );
        assert!(dirty);
        assert!(fetch_error.is_none());
        let entry = entry.expect("成功后必须有缓存条目");
        assert_eq!(entry.queried_at_ms, NOW_MS);
        assert_eq!(entry.credential_status, CREDENTIAL_VALID);
        assert_eq!(entry.error, None, "成功必须清掉旧错误");
        assert_eq!(entry.balances[0].currency, "USD");
        assert_eq!(entry.credential_fingerprint, credential_fingerprint("key"));
    }

    #[test]
    fn deterministic_failure_keeps_last_good_data_and_sets_error() {
        let cached = cached_entry(&credential_fingerprint("key"), NOW_MS - CACHE_TTL_MS);
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            PROVIDER_DEEPSEEK,
            Some(resolved_credential(Some("key"))),
            Some(cached),
            false,
            NOW_MS,
            |_, _| {
                FetchOutcome::Deterministic(
                    "当前内核模型凭据无效或无权限（HTTP 401）".to_string(),
                    true,
                )
            },
        );
        assert!(dirty);
        assert!(fetch_error.is_none(), "确定性失败不是瞬时错误");
        let entry = entry.expect("确定性失败仍要保留缓存条目");
        assert_eq!(entry.credential_status, CREDENTIAL_EXPIRED);
        assert_eq!(
            entry.balances[0].total, "110.00",
            "旧数据必须保留（keep-last-good）"
        );
        assert!(entry.error.unwrap().contains("401"));
    }

    #[test]
    fn transient_failure_leaves_cache_untouched() {
        let cached = cached_entry(&credential_fingerprint("key"), NOW_MS - CACHE_TTL_MS);
        let original = cached.clone();
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            PROVIDER_DEEPSEEK,
            Some(resolved_credential(Some("key"))),
            Some(cached),
            false,
            NOW_MS,
            |_, _| FetchOutcome::Transient("查询失败（网络不可达或超时）".to_string()),
        );
        assert!(!dirty, "瞬时失败不得写盘");
        assert_eq!(fetch_error.as_deref(), Some("查询失败（网络不可达或超时）"));
        let entry = entry.expect("缓存条目原样保留");
        assert_eq!(entry, original, "条目内容一个字节都不能动");
    }

    #[test]
    fn changed_credential_drops_old_entry_then_queries_with_new_key() {
        let old = cached_entry(&credential_fingerprint("key-old"), NOW_MS);
        let mut queried_with = None;
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            PROVIDER_DEEPSEEK,
            Some(resolved_credential(Some("key-new"))),
            Some(old),
            false,
            NOW_MS,
            |id, key| {
                queried_with = Some((id.to_string(), key.to_string()));
                FetchOutcome::Success(ProviderData::Balance {
                    is_available: true,
                    balances: vec![CacheBalance {
                        currency: "CNY".to_string(),
                        total: "1.00".to_string(),
                        granted: None,
                        topped_up: None,
                    }],
                })
            },
        );
        assert!(dirty);
        assert_eq!(
            queried_with,
            Some((PROVIDER_DEEPSEEK.to_string(), "key-new".to_string())),
            "换凭据后必须用新值查询"
        );
        assert!(fetch_error.is_none());
        let entry = entry.expect("新凭据查询成功应有新条目");
        assert_eq!(entry.balances[0].total, "1.00", "展示的必须是新账号的数据");
        assert_eq!(
            entry.credential_fingerprint,
            credential_fingerprint("key-new")
        );
    }

    #[test]
    fn clearing_credential_drops_entry_without_fetch() {
        let old = cached_entry(&credential_fingerprint("key"), NOW_MS);
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            PROVIDER_DEEPSEEK,
            Some(resolved_credential(None)),
            Some(old),
            false,
            NOW_MS,
            no_fetch(),
        );
        assert!(dirty, "凭据未配置必须把旧条目从缓存里清掉");
        assert!(entry.is_none());
        assert!(fetch_error.is_none());
    }

    #[test]
    fn unconfigured_provider_without_cache_is_a_no_op() {
        let (entry, fetch_error, dirty) =
            refresh_provider_entry(PROVIDER_MINIMAX_CN, None, None, false, NOW_MS, no_fetch());
        assert!(!dirty);
        assert!(entry.is_none() && fetch_error.is_none());
    }

    #[test]
    fn fresh_valid_entry_within_ttl_skips_fetch() {
        let cached = cached_entry(&credential_fingerprint("key"), NOW_MS - 1000);
        let (entry, fetch_error, dirty) = refresh_provider_entry(
            PROVIDER_DEEPSEEK,
            Some(resolved_credential(Some("key"))),
            Some(cached.clone()),
            false,
            NOW_MS,
            no_fetch(),
        );
        assert!(!dirty);
        assert!(fetch_error.is_none());
        assert_eq!(entry.as_ref(), Some(&cached), "TTL 内原样返回缓存");
    }

    #[test]
    fn force_overrides_ttl_and_expired_skip() {
        // force 会重试 expired 条目（点刷新永远重试）。
        let mut cached = cached_entry(&credential_fingerprint("key"), NOW_MS - 1000);
        cached.credential_status = CREDENTIAL_EXPIRED.to_string();
        let (entry, _, dirty) = refresh_provider_entry(
            PROVIDER_MINIMAX_CN,
            Some(resolved_credential(Some("key"))),
            Some(cached),
            true,
            NOW_MS,
            |_, _| {
                FetchOutcome::Success(ProviderData::Plan {
                    tiers: vec![CacheTier {
                        name: "5h".to_string(),
                        remaining_percent: 73.2,
                        resets_at_ms: Some(NOW_MS + 3_600_000),
                    }],
                })
            },
        );
        assert!(dirty);
        let entry = entry.expect("force 成功后应有新条目");
        assert_eq!(entry.kind, "plan");
        assert_eq!(entry.tiers[0].name, "5h");
    }
}

/// 缓存文档读写与实例绑定的离线集成测试：不触网（凭据未配置或缓存新鲜）。
#[cfg(test)]
mod cache_tests {
    use super::*;

    fn temp_home(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-subscription-{}-{}-{}",
            label,
            std::process::id(),
            crate::process::epoch_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 未配置任何凭据：不发请求，且历史遗留条目必须被清掉（「未配置」与
    /// 「失效」是两个互斥状态，凭据没了不能还留着上一个账号的数据）。
    #[test]
    fn unconfigured_provider_drops_stale_entries_without_network() {
        let home = temp_home("unconfigured");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let scope = InstanceScope::resolve();
        std::fs::create_dir_all(scope.dsh_home()).unwrap();

        let mut doc = SubscriptionCacheDoc {
            schema: CACHE_SCHEMA,
            instance: None,
            providers: BTreeMap::new(),
        };
        doc.providers.insert(
            PROVIDER_DEEPSEEK.to_string(),
            ProviderCacheEntry {
                kind: "balance".to_string(),
                credential_fingerprint: credential_fingerprint("stale-key"),
                queried_at_ms: 1,
                ..ProviderCacheEntry::default()
            },
        );
        state::save(&scope.cache_path(), &doc, cache_ctx()).unwrap();

        let view = subscription_view(None, false).expect("未配置查询应当成功返回");
        assert_eq!(view.providers.len(), PROVIDER_ORDER.len());
        for provider in &view.providers {
            assert!(!provider.configured, "{} 不应处于已配置态", provider.id);
            assert_eq!(provider.kind, None);
            assert!(provider.error.is_none() && provider.fetch_error.is_none());
        }

        // 缓存文件里那条遗留数据必须已经消失。
        let doc: SubscriptionCacheDoc =
            state::load_checked(&scope.cache_path(), cache_ctx()).unwrap();
        assert!(!doc.providers.contains_key(PROVIDER_DEEPSEEK));
        std::fs::remove_dir_all(&home).ok();
    }

    /// 缓存新鲜且指纹匹配：TTL 内不发请求，直接返回缓存数据（这样单测可以
    /// 离线跑通「缓存命中」路径；真实网络路径由真机验证覆盖）。
    #[test]
    fn fresh_cache_is_served_without_refetch() {
        let home = temp_home("fresh-cache");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let scope = InstanceScope::resolve();
        // 让 DeepSeek 的默认引用能解析出值：写进实例的 .credentials.yaml。
        let dsh_home = scope.dsh_home();
        std::fs::create_dir_all(&dsh_home).unwrap();
        std::fs::write(
            dsh_home.join(".credentials.yaml"),
            "version: 1\nrefs:\n  DEEPSEEK_API_KEY: sk-test-fresh\n",
        )
        .unwrap();

        let mut doc = SubscriptionCacheDoc {
            schema: CACHE_SCHEMA,
            instance: Some(CacheInstanceBinding {
                family: scope.family.clone(),
                id: scope.id.clone(),
                profile: scope.profile.clone(),
            }),
            providers: BTreeMap::new(),
        };
        doc.providers.insert(
            PROVIDER_DEEPSEEK.to_string(),
            ProviderCacheEntry {
                kind: "balance".to_string(),
                credential_ref: "DEEPSEEK_API_KEY".to_string(),
                credential_status: CREDENTIAL_VALID.to_string(),
                queried_at_ms: crate::process::epoch_millis(),
                is_available: Some(true),
                balances: vec![CacheBalance {
                    currency: "CNY".to_string(),
                    total: "110.00".to_string(),
                    granted: Some("10.00".to_string()),
                    topped_up: Some("100.00".to_string()),
                }],
                credential_fingerprint: credential_fingerprint("sk-test-fresh"),
                ..ProviderCacheEntry::default()
            },
        );
        state::save(&scope.cache_path(), &doc, cache_ctx()).unwrap();

        let view = subscription_view(Some(PROVIDER_DEEPSEEK), false).expect("TTL 内应命中缓存");
        assert_eq!(view.providers.len(), 1);
        assert_eq!(
            view.instance.profile, scope.profile,
            "视图必须携带当前实例信息"
        );
        let provider = &view.providers[0];
        assert!(provider.configured);
        assert!(provider.fetch_error.is_none(), "TTL 内不得发起网络请求");
        assert_eq!(provider.balances.len(), 1);
        assert_eq!(provider.balances[0].total, "110.00");
        assert_eq!(provider.credential_ref.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(
            provider.credential_status.as_deref(),
            Some(CREDENTIAL_VALID)
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// expired 条目不参与自动刷新：TTL 过期 + credential_status expired 仍不
    /// 发请求（否则拿失效凭据反复打接口），旧数据 + 错误原样透出。
    #[test]
    fn expired_entries_skip_auto_refresh() {
        let home = temp_home("expired-skip");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let scope = InstanceScope::resolve();
        let dsh_home = scope.dsh_home();
        std::fs::create_dir_all(&dsh_home).unwrap();
        std::fs::write(
            dsh_home.join(".credentials.yaml"),
            "version: 1\nrefs:\n  DEEPSEEK_API_KEY: sk-test-expired\n",
        )
        .unwrap();

        let mut doc = SubscriptionCacheDoc {
            schema: CACHE_SCHEMA,
            instance: Some(CacheInstanceBinding {
                family: scope.family.clone(),
                id: scope.id.clone(),
                profile: scope.profile.clone(),
            }),
            providers: BTreeMap::new(),
        };
        doc.providers.insert(
            PROVIDER_DEEPSEEK.to_string(),
            ProviderCacheEntry {
                kind: "balance".to_string(),
                credential_ref: "DEEPSEEK_API_KEY".to_string(),
                credential_status: CREDENTIAL_EXPIRED.to_string(),
                queried_at_ms: crate::process::epoch_millis() - CACHE_TTL_MS - 1,
                error: Some("当前内核模型凭据无效或无权限（HTTP 401）".to_string()),
                balances: vec![CacheBalance {
                    currency: "CNY".to_string(),
                    total: "88.00".to_string(),
                    granted: None,
                    topped_up: Some("88.00".to_string()),
                }],
                credential_fingerprint: credential_fingerprint("sk-test-expired"),
                ..ProviderCacheEntry::default()
            },
        );
        state::save(&scope.cache_path(), &doc, cache_ctx()).unwrap();

        let view = subscription_view(Some(PROVIDER_DEEPSEEK), false).expect("expired 跳过刷新");
        let provider = &view.providers[0];
        assert!(provider.fetch_error.is_none(), "expired 条目不得自动重试");
        assert_eq!(
            provider.credential_status.as_deref(),
            Some(CREDENTIAL_EXPIRED)
        );
        assert_eq!(
            provider.balances[0].total, "88.00",
            "keep-last-good：旧数据保留"
        );
        assert!(provider.error.is_some(), "确定性错误必须随视图透出");
        std::fs::remove_dir_all(&home).ok();
    }

    /// 切换 profile（实例绑定不匹配）：旧文档整体作废——里面是另一个
    /// 账号 / profile 的数据，绝不能继续展示。凭据未配置（离线路径）：
    /// 条目清空且视图回到「未配置」。
    #[test]
    fn instance_binding_mismatch_invalidates_whole_doc() {
        let home = temp_home("binding-mismatch");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let scope = InstanceScope::resolve();
        std::fs::create_dir_all(scope.dsh_home()).unwrap();

        let mut doc = SubscriptionCacheDoc {
            schema: CACHE_SCHEMA,
            instance: Some(CacheInstanceBinding {
                family: scope.family.clone(),
                id: scope.id.clone(),
                profile: "other-profile".to_string(),
            }),
            providers: BTreeMap::new(),
        };
        doc.providers.insert(
            PROVIDER_DEEPSEEK.to_string(),
            ProviderCacheEntry {
                kind: "balance".to_string(),
                credential_status: CREDENTIAL_VALID.to_string(),
                queried_at_ms: crate::process::epoch_millis(),
                balances: vec![CacheBalance {
                    currency: "CNY".to_string(),
                    total: "999.00".to_string(),
                    granted: None,
                    topped_up: None,
                }],
                credential_fingerprint: credential_fingerprint("sk-old-profile"),
                ..ProviderCacheEntry::default()
            },
        );
        state::save(&scope.cache_path(), &doc, cache_ctx()).unwrap();

        // 绑定不匹配 → 文档整体作废；当前凭据未配置 → 不发请求、视图回到未配置。
        let view = subscription_view(Some(PROVIDER_DEEPSEEK), false).expect("绑定不匹配应离线处理");
        let provider = &view.providers[0];
        assert!(
            provider.balances.is_empty(),
            "绑定不匹配后旧 profile 的余额不得继续展示"
        );
        assert!(!provider.configured, "测试环境未配置凭据，视图必须如实回报");
        let doc: SubscriptionCacheDoc =
            state::load_checked(&scope.cache_path(), cache_ctx()).unwrap();
        assert!(!doc.providers.contains_key(PROVIDER_DEEPSEEK));
        assert_eq!(
            doc.instance.as_ref().map(|b| b.profile.as_str()),
            Some(scope.profile.as_str()),
            "绑定必须刷新为当前 profile"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 缓存文档损坏：命令报错而不是拿空文档覆盖（与 state.rs「不存在 ≠ 损坏」
    /// 同一哲学），文案要给出可操作的下一步。
    #[test]
    fn corrupt_cache_file_is_reported_not_clobbered() {
        let home = temp_home("corrupt-cache");
        let _guard = crate::tests::scoped_xlink_home(&home);
        let scope = InstanceScope::resolve();
        std::fs::create_dir_all(scope.dsh_home()).unwrap();

        let path = scope.cache_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ broken").unwrap();

        let err = subscription_view(None, false).expect_err("损坏必须报错");
        assert!(
            err.contains("套餐用量缓存损坏"),
            "文案要点名是缓存损坏：{err}"
        );
        assert!(err.contains("删除"), "要给出下一步：{err}");
        std::fs::remove_dir_all(&home).ok();
    }
}
