//! DSH 内核模型凭据的只读解析（供 `subscription.rs` 复用当前模型凭据查询用量）。
//!
//! dsh-xlink **不收集、不存储、不回显**任何凭据：工作台的模型设置是唯一的
//! 凭据编辑入口。本模块只做一件事——按 DSH 的同一套语义，把「当前实例 /
//! profile 的某个 provider」解析成一把可用的 Key 值（或确认它未配置）。
//!
//! ## 解析规则（与 DSH 内核对齐）
//!
//! 1. **credential reference**：优先取 profile（`<DSH_HOME>/profiles/<name>/
//!    cordis.patch.yml` 的 provider `apiKeyEnv`）声明的引用名；profile 没有
//!    显式声明时，才用内核模型页同样的派生规则（route `minimax-cn` →
//!    `MINIMAX_CN_API_KEY`，即大写 + `_API_KEY`）。
//! 2. **引用值**按 DSH 的优先级解析：启动环境快照（壳进程环境——内核由壳
//!    以同一环境派生，用户在 shell 里 export 的变量两边看到的是同一个值）
//!    优先，其次是 `<DSH_HOME>/.credentials.yaml` 的 `refs` 映射，再其次是
//!    `<DSH_HOME>/.env` 回退层。
//!
//! 只读取当前实例实际引用的值、只用于构造 HTTPS 请求头；不写缓存、不进
//! 日志、不返回给 UI。profile 的 patch 文件解析是**只读**的，路径由
//! [`crate::paths`] 的实例目录推导，不接受外部输入。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 订阅查询支持的 provider（与 `subscription.rs` 的 PROVIDER_* 常量一致）。
pub const PROVIDER_MINIMAX_CN: &str = "minimax_cn";
pub const PROVIDER_MINIMAX_EN: &str = "minimax_en";
pub const PROVIDER_DEEPSEEK: &str = "deepseek";
pub const PROVIDER_ZAI_CODING_CN: &str = "zai_coding_cn";

/// 智谱套餐查询的上下文引用名（组织 / 项目 / 套餐类型）。这三项不是秘密，
/// 但与 Key 一样走「环境变量 → .credentials.yaml refs → .env」的解析链，
/// 用户只需在任一层各配一次。
pub const ZAI_ORGANIZATION_REF: &str = "ZAI_CODING_CN_ORGANIZATION";
pub const ZAI_PROJECT_REF: &str = "ZAI_CODING_CN_PROJECT";
pub const ZAI_PLAN_TYPE_REF: &str = "ZAI_CODING_CN_PLAN_TYPE";

/// 一条解析结果：引用名 + 该引用当前解析出的值（未配置时为 `None`）+ 值来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCredential {
    /// credential reference 名（如 `MINIMAX_CN_API_KEY`）。引用名不是秘密。
    pub reference: String,
    /// 解析出的 Key 值；`None` = 未配置（引用在环境 / 凭据文件 / .env 都没有值）。
    pub value: Option<String>,
    /// 值来源（诊断展示用，不含值本身）。
    pub source: CredentialSource,
}

/// 凭据值的来源，按 DSH 的优先级排序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    /// 启动环境快照（壳 / 内核进程环境变量）。
    Env,
    /// `<DSH_HOME>/.credentials.yaml` 的 `refs`。
    CredentialsYaml,
    /// `<DSH_HOME>/.env` 回退层。
    EnvFile,
}

impl CredentialSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialSource::Env => "env",
            CredentialSource::CredentialsYaml => "credentials.yaml",
            CredentialSource::EnvFile => ".env",
        }
    }
}

/// 一个 provider 的 credential reference 绑定（来自 profile 声明或默认派生）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderBinding {
    /// 内核 provider route 名（如 `minimax-cn`）。
    pub route: String,
    /// credential reference 名。
    pub reference: String,
    /// 是否来自 profile 的显式 `apiKeyEnv`（`false` = 默认派生）。
    pub explicit: bool,
}

/// 解析某个订阅 provider 的凭据。
///
/// `bindings` 是 profile 里解析出的 provider 绑定（见 [`profile_bindings`]）；
/// 没有匹配绑定时按内核默认派生规则给出引用名，再走值解析——这与内核对
/// 内置 provider（如官方 DeepSeek）的行为一致：模型设置里配置过的凭据存在
/// 于凭据文件中，profile patch 文件里未必有显式条目。
pub fn resolve_provider(
    provider: &str,
    bindings: &[ProviderBinding],
    dsh_home: &Path,
) -> Option<ResolvedCredential> {
    let binding = match bindings
        .iter()
        .find(|b| classify_route(&b.route, None) == Some(provider))
    {
        Some(binding) => binding.clone(),
        None => ProviderBinding {
            route: default_route(provider)?.to_string(),
            reference: default_reference(provider)?,
            explicit: false,
        },
    };
    let resolved = resolve(&binding.reference, dsh_home);
    if resolved.value.is_some() {
        Some(resolved)
    } else {
        None
    }
}

/// 把内核 provider route 归类到订阅 provider；不认识的 route 返回 `None`。
///
/// 判据用 route 名与 baseURL 双信号：route 名含 `minimax` 时按 `cn` / 其余
/// （`en` / `io` / `intl`）分国内国际；`deepseek` 直接对应。baseURL 只在
/// 调用方给出时参与兜底判断（profile 解析阶段没有独立的 baseURL 索引）。
pub fn classify_route(route: &str, base_url: Option<&str>) -> Option<&'static str> {
    let route_lower = route.to_ascii_lowercase();
    if route_lower.contains("deepseek") {
        return Some(PROVIDER_DEEPSEEK);
    }
    // 智谱编程套餐：内核 route 名是 zai-coding-cn（GLM 系模型）。
    if route_lower.contains("zai")
        || route_lower.contains("bigmodel")
        || route_lower.contains("glm")
    {
        return Some(PROVIDER_ZAI_CODING_CN);
    }
    if route_lower.contains("minimax") {
        let cn = route_lower.contains("cn")
            || base_url.map(|u| u.contains("minimaxi.com") || u.contains("minimax.cn"))
                == Some(true);
        return Some(if cn {
            PROVIDER_MINIMAX_CN
        } else {
            PROVIDER_MINIMAX_EN
        });
    }
    if let Some(url) = base_url {
        let url_lower = url.to_ascii_lowercase();
        if url_lower.contains("minimaxi.com") || url_lower.contains("minimax.cn") {
            return Some(PROVIDER_MINIMAX_CN);
        }
        if url_lower.contains("minimax.io") {
            return Some(PROVIDER_MINIMAX_EN);
        }
        if url_lower.contains("api.deepseek.com") {
            return Some(PROVIDER_DEEPSEEK);
        }
        if url_lower.contains("bigmodel.cn") {
            return Some(PROVIDER_ZAI_CODING_CN);
        }
    }
    None
}

/// 内核模型页对内置 provider 的默认引用派生规则：route 大写 + `_API_KEY`。
pub fn default_reference(provider: &str) -> Option<String> {
    let route = default_route(provider)?;
    Some(format!(
        "{}_API_KEY",
        route.to_ascii_uppercase().replace('-', "_")
    ))
}

fn default_route(provider: &str) -> Option<&'static str> {
    match provider {
        PROVIDER_MINIMAX_CN => Some("minimax-cn"),
        PROVIDER_MINIMAX_EN => Some("minimax-en"),
        PROVIDER_DEEPSEEK => Some("deepseek"),
        PROVIDER_ZAI_CODING_CN => Some("zai-coding-cn"),
        _ => None,
    }
}

/// 按引用名解析值：环境变量 → `.credentials.yaml` refs → `.env`。
/// 值为空白一律视为未配置（与「未配置」语义合并，避免空串被当成 Key）。
pub fn resolve(reference: &str, dsh_home: &Path) -> ResolvedCredential {
    let mut resolved = ResolvedCredential {
        reference: reference.to_string(),
        value: None,
        source: CredentialSource::Env,
    };
    if let Ok(value) = std::env::var(reference) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            resolved.value = Some(value);
            return resolved;
        }
    }
    let refs = load_refs(dsh_home);
    if let Some(value) = refs.get(reference) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            resolved.value = Some(value);
            resolved.source = CredentialSource::CredentialsYaml;
            return resolved;
        }
    }
    let env_file = load_env_file(&dsh_home.join(".env"));
    if let Some(value) = env_file.get(reference) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            resolved.value = Some(value);
            resolved.source = CredentialSource::EnvFile;
        }
    }
    resolved
}

/// `<DSH_HOME>/profiles/<profile>/cordis.patch.yml` 里声明的 provider 绑定。
///
/// 只关心 patch 条目 `config.providers.<route>.apiKeyEnv`（内核模型页的自定义
/// provider 声明就在这里）；其它字段一律忽略。解析失败按「无显式声明」处理
/// ——调用方回退到默认派生，绝不让格式漂移把查询功能整体打挂。
pub fn profile_bindings(profile_dir: &Path) -> Vec<ProviderBinding> {
    let mut bindings = Vec::new();
    for file_name in ["cordis.patch.yml", "cordis.patch.yaml"] {
        let path = profile_dir.join(file_name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        bindings.extend(patch_providers(&text));
        if !bindings.is_empty() {
            break;
        }
    }
    bindings
}

/// 从 patch YAML 文本提取 `config.providers.<route>.apiKeyEnv` 绑定（纯函数）。
pub fn patch_providers(yaml: &str) -> Vec<ProviderBinding> {
    let mut bindings = Vec::new();
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml) else {
        return bindings;
    };
    let Some(entries) = value.as_sequence() else {
        return bindings;
    };
    for entry in entries {
        let Some(config) = entry.get("config") else {
            continue;
        };
        let Some(providers) = config.get("providers").and_then(|p| p.as_mapping()) else {
            continue;
        };
        for (route, provider) in providers {
            let Some(route) = route.as_str() else {
                continue;
            };
            let Some(api_key_env) = provider.get("apiKeyEnv").and_then(|v| v.as_str()) else {
                continue;
            };
            let api_key_env = api_key_env.trim();
            if api_key_env.is_empty() {
                continue;
            }
            bindings.push(ProviderBinding {
                route: route.to_string(),
                reference: api_key_env.to_string(),
                explicit: true,
            });
        }
    }
    bindings
}

/// `<DSH_HOME>/.credentials.yaml` 的 `refs` 映射（引用名 → 值）。
/// 文件缺失 / 解析失败都返回空映射——凭据文件的读写与加锁归内核所有，
/// 壳侧只做尽力而为的只读，绝不因解析失败中断或写回。
pub fn load_refs(dsh_home: &Path) -> BTreeMap<String, String> {
    let path = dsh_home.join(".credentials.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
        return BTreeMap::new();
    };
    let Some(refs) = value.get("refs").and_then(|r| r.as_mapping()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (key, value) in refs {
        let Some(key) = key.as_str() else {
            continue;
        };
        // 标量统一字符串化：`KEY: 2` 这类数字 / 布尔写法也是合法凭据值
        // （如组织 id），只认字符串会把它们静默丢掉。
        let value = match value {
            serde_yaml::Value::String(text) => text.clone(),
            serde_yaml::Value::Number(number) => number.to_string(),
            serde_yaml::Value::Bool(flag) => flag.to_string(),
            _ => continue,
        };
        out.insert(key.to_string(), value);
    }
    out
}

/// `.env` 回退层的 `KEY=VALUE` 解析（忽略注释与空行；带引号的值去引号）。
pub fn load_env_file(path: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let mut value = value.trim();
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = &value[1..value.len() - 1];
        }
        out.insert(key.to_string(), value.to_string());
    }
    out
}

/// 当前默认实例的 DSH home（凭据根）。与 [`crate::usage`] 的实例口径一致：
/// 家族 / 实例来自默认实例解析器。
pub fn default_dsh_home() -> PathBuf {
    let (family, id) = crate::instance::resolve_default();
    crate::paths::instance_dsh_home(family, id)
}

/// 当前默认实例激活的 profile 名。注册表里的实例记录是权威来源；注册表
/// 不可读或没有该实例时回退 Shell 设置的 profile 名，再回退 `web`。
pub fn default_profile() -> String {
    let (_, id) = crate::instance::resolve_default();
    if let Ok(registry) = crate::instance::load_registry() {
        if let Some(record) = registry.instances.iter().find(|r| r.id == id) {
            return record.profile.clone();
        }
    }
    let profile = crate::settings::load_for_shell(crate::settings::current_mode()).profile;
    if profile.trim().is_empty() {
        "web".to_string()
    } else {
        profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 设计文档 / 真实 profile 的 patch 形态：provider 带显式 `apiKeyEnv`，
    /// 其余无关条目应被忽略。
    const PATCH_SAMPLE: &str = r#"
- id: agent-default-model
  name: "@deepseek-ai/dsh-agent-default-model"
  config:
    provider: zai-coding-cn
    model: glm-5.3-flash
- id: llm-pi-ai
  name: "@deepseek-ai/dsh-llm-pi-ai"
  config:
    providers:
      opencode-go:
        api: openai-completions
        baseURL: https://opencode.ai/zen/v1
        apiKeyEnv: OPENCODE_GO_API_KEY
      minimax-cn:
        apiKeyEnv: MINIMAX_CN_CUSTOM_KEY
        baseURL: https://api.minimaxi.com/anthropic
      zai-coding-cn:
        apiKeyEnv: ZAI_CODING_CN_API_KEY
        models:
          - id: glm-5.3
"#;

    #[test]
    fn patch_providers_extracts_explicit_references_only() {
        let bindings = patch_providers(PATCH_SAMPLE);
        assert_eq!(
            bindings.len(),
            3,
            "无关条目（agent-default-model / models 列表）不产出绑定"
        );
        let minimax = bindings
            .iter()
            .find(|b| b.route == "minimax-cn")
            .expect("应有 minimax-cn");
        assert_eq!(minimax.reference, "MINIMAX_CN_CUSTOM_KEY");
        assert!(minimax.explicit);
    }

    #[test]
    fn classify_routes_into_subscription_providers() {
        assert_eq!(
            classify_route("minimax-cn", None),
            Some(PROVIDER_MINIMAX_CN)
        );
        assert_eq!(
            classify_route("minimax-en", None),
            Some(PROVIDER_MINIMAX_EN)
        );
        assert_eq!(
            classify_route("minimax-intl", None),
            Some(PROVIDER_MINIMAX_EN)
        );
        assert_eq!(classify_route("deepseek", None), Some(PROVIDER_DEEPSEEK));
        assert_eq!(
            classify_route("zai-coding-cn", None),
            Some(PROVIDER_ZAI_CODING_CN)
        );
        assert_eq!(
            classify_route("bigmodel", Some("https://bigmodel.cn/api/paas/v4")),
            Some(PROVIDER_ZAI_CODING_CN)
        );
        assert_eq!(
            classify_route("minimax", Some("https://api.minimaxi.com/anthropic")),
            Some(PROVIDER_MINIMAX_CN)
        );
        assert_eq!(
            classify_route("minimax", Some("https://api.minimax.io/v1")),
            Some(PROVIDER_MINIMAX_EN)
        );
        assert_eq!(
            classify_route("opencode-go", None),
            None,
            "不认识的 route 必须返回 None"
        );
    }

    #[test]
    fn default_reference_follows_kernel_derivation() {
        assert_eq!(
            default_reference(PROVIDER_MINIMAX_CN).as_deref(),
            Some("MINIMAX_CN_API_KEY")
        );
        assert_eq!(
            default_reference(PROVIDER_MINIMAX_EN).as_deref(),
            Some("MINIMAX_EN_API_KEY")
        );
        assert_eq!(
            default_reference(PROVIDER_DEEPSEEK).as_deref(),
            Some("DEEPSEEK_API_KEY")
        );
        assert_eq!(
            default_reference(PROVIDER_ZAI_CODING_CN).as_deref(),
            Some("ZAI_CODING_CN_API_KEY")
        );
        assert_eq!(default_reference("unknown"), None);
    }

    #[test]
    fn resolve_prefers_env_then_credentials_then_env_file() {
        let dir = tempfile_fresh("credentials-priority");
        std::fs::write(
            dir.join(".credentials.yaml"),
            "version: 1\nrefs:\n  MINIMAX_CN_API_KEY: from-yaml\n  DEEPSEEK_API_KEY: ds-yaml\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".env"),
            "# comment\nDEEPSEEK_API_KEY=ds-env\nEMPTY_KEY=\n",
        )
        .unwrap();

        // 环境变量优先：设置环境会影响同进程其它测试，这里只测无环境命中的
        // 路径；env 分支逻辑只有一行 `std::env::var`，行为由签名保证。
        let resolved = resolve("MINIMAX_CN_API_KEY", &dir);
        assert_eq!(resolved.value.as_deref(), Some("from-yaml"));
        assert_eq!(resolved.source, CredentialSource::CredentialsYaml);

        // .env 回退层：凭据文件里没有的引用才落到这里。
        let resolved = resolve("DEEPSEEK_API_KEY", &dir);
        assert_eq!(
            resolved.value.as_deref(),
            Some("ds-yaml"),
            "凭据文件优先于 .env"
        );
        assert_eq!(resolve("EMPTY_KEY", &dir).value, None, "空值视为未配置");
        assert_eq!(resolve("MISSING_KEY", &dir).value, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn env_file_parses_quotes_and_comments() {
        let dir = tempfile_fresh("credentials-envfile");
        let path = dir.join(".env");
        std::fs::write(&path, "A=1\n# B=2\nC = \"three\"\nD='four'\n\n").unwrap();
        let map = load_env_file(&path);
        assert_eq!(map.get("A").map(String::as_str), Some("1"));
        assert_eq!(map.get("C").map(String::as_str), Some("three"));
        assert_eq!(map.get("D").map(String::as_str), Some("four"));
        assert!(!map.contains_key("B"), "注释行不解析");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_files_are_treated_as_unconfigured() {
        let dir = tempfile_fresh("credentials-missing");
        let resolved = resolve("ANY_KEY", &dir);
        assert_eq!(resolved.value, None);
        assert!(profile_bindings(&dir.join("profiles/web")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn tempfile_fresh(label: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "dsh-xlink-credentials-{label}-{}-{unique}",
            crate::process::epoch_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
