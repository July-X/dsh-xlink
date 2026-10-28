//! 桌面外壳的 npm registry 配置。
//!
//! 外壳从 npm 生态拉取的所有内容——内核安装、插件安装、profile 接入、
//! 内核发布列表、插件元数据、tarball 下载——都绑定到同一份**候选链**。
//!
//! ## 为什么官方优先、镜像只做兜底
//!
//! 默认源是**官方 registry**：版本列表与安装必须看到同一份真相，而
//! `registry.npmmirror.com` 对同一批包的同步是**逐包**到达的（2026-10-28 实测：
//! `@deepseek-ai/dsh@0.2.2-alpha.2` 与部分子包已同步，而 `dsh-fs` / `dsh-commands`
//! / `dsh-session-telemetry-otel` / `dsh-web-deepseek-api` 仍停在旧版本线）。
//! 以镜像为默认时，版本列表能列出内核版本、安装却在 pnpm 解析阶段失败，
//! 用户看到的错和实际发生的错对不上。
//!
//! 镜像只在**传输层不可达**（连接失败 / DNS / 超时）时兜底；HTTP 错误、包不存在、
//! 缺具体版本都**不换源**——那些情况下另一个源不会有不同答案，换源只会把
//! 「上游发布事故」伪装成「镜像延迟」，把用户指向重试网络这条死路。
//!
//! ## 显式覆盖
//!
//! 需要强制使用某个 registry 的部署（内网镜像、离线包）用 `DSH_NPM_REGISTRY`
//! 覆盖：此时候选链只有它一个，既不以官方为默认、也不再兜底——用户既然明确
//! 指定了源，壳不替他再挑一个。
//!
//! 调用方按 `format!("{base}{path}")` 拼装 URL；每条候选末尾的斜杠是必需的。

/// 官方 npm registry。版本列表与安装的默认来源。
pub const OFFICIAL_NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// 国内镜像。只在官方源传输层不可达时兜底，不作为默认源。
pub const MIRROR_NPM_REGISTRY: &str = "https://registry.npmmirror.com/";

/// 默认的首选源。**不要**把它改回镜像：镜像同步滞后时版本列表与安装会看到
/// 两份不同的版本线（见模块文档）。
pub const DEFAULT_NPM_REGISTRY: &str = OFFICIAL_NPM_REGISTRY;

/// 启动时读取的环境变量，用于覆盖 registry 基础 URL。
pub const NPM_REGISTRY_ENV: &str = "DSH_NPM_REGISTRY";

/// 按优先级排列的候选 registry。
///
/// 默认（未设置 `DSH_NPM_REGISTRY`）是 `[官方, 镜像]`：调用方遇到**传输层**
/// 失败才可尝试下一个；显式覆盖时只有一个元素，调用方不得再兜底。
///
/// 每次调用都重新读环境变量：GUI 进程内 fork-spawn 的频率很低，实时读取
/// 让测试与部署覆盖都能立刻生效。
pub fn npm_registry_candidates() -> Vec<String> {
    candidates(std::env::var(NPM_REGISTRY_ENV).ok().as_deref())
}

/// 当前首选的 registry 基础 URL（候选链第一项）。
///
/// 需要「多个源依次尝试」的调用方用 [`npm_registry_candidates`]；单 URL 的
/// 调用点（写 `.npmrc`、拼一个确定的请求地址）用这个。
pub fn npm_registry_base() -> String {
    // `candidates` 恒返回非空：默认两条，覆盖路径一条。
    npm_registry_candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| DEFAULT_NPM_REGISTRY.to_string())
}

/// 抽取出来的纯解析函数，以便测试在不动进程环境的情况下驱动它
/// （进程环境操作会与并发测试产生竞态）。
fn candidates(override_value: Option<&str>) -> Vec<String> {
    match override_value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => vec![normalize(raw)],
        None => vec![
            normalize(DEFAULT_NPM_REGISTRY),
            normalize(MIRROR_NPM_REGISTRY),
        ],
    }
}

/// 补上末尾斜杠：调用方按 `{base}{path}` 直接拼接。
fn normalize(raw: &str) -> String {
    if raw.ends_with('/') {
        raw.to_string()
    } else {
        format!("{raw}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_candidates_are_official_then_mirror() {
        assert_eq!(
            candidates(None),
            vec![
                OFFICIAL_NPM_REGISTRY.to_string(),
                MIRROR_NPM_REGISTRY.to_string()
            ],
            "默认必须官方优先、镜像只做第二条兜底；顺序反过来会重现「列表能列出、安装装不上」"
        );
        assert_eq!(
            npm_registry_base_candidate(),
            OFFICIAL_NPM_REGISTRY,
            "首选源必须是官方 registry"
        );
    }

    #[test]
    fn explicit_override_has_no_fallback() {
        assert_eq!(
            candidates(Some("https://r.example.com")),
            vec!["https://r.example.com/".to_string()],
            "显式覆盖是用户指定，不得再自动换源"
        );
        assert_eq!(
            candidates(Some("https://registry.npmmirror.com/")),
            vec![MIRROR_NPM_REGISTRY.to_string()],
            "把镜像显式写成默认也只走镜像：是否兜底由候选链长度表达"
        );
    }

    #[test]
    fn empty_or_whitespace_falls_back_to_default() {
        assert_eq!(candidates(Some("")), candidates(None));
        assert_eq!(candidates(Some("   ")), candidates(None));
    }

    #[test]
    fn override_is_trimmed_and_trailing_slash_is_enforced() {
        assert_eq!(
            candidates(Some("  https://r.example.com  ")),
            vec!["https://r.example.com/".to_string()]
        );
        assert!(candidates(Some("https://r.example.com"))
            .iter()
            .all(|base| base.ends_with('/')));
        assert!(candidates(None).iter().all(|base| base.ends_with('/')));
    }

    /// 与 `npm_registry_base()` 同义的纯函数版本：后者会读进程环境，测试里
    /// 不碰环境变量。
    fn npm_registry_base_candidate() -> String {
        candidates(None).remove(0)
    }
}
