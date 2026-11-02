//! 桌面外壳的 npm registry 配置与取源 URL 策略。
//!
//! 外壳从 npm 生态拉取的所有内容——内核安装、插件安装、profile 接入、
//! 内核发布列表、插件元数据、tarball 下载——都绑定到同一份候选链。
//!
//! ## 为什么官方优先、镜像只做兜底
//!
//! 默认源是官方 registry：版本列表与安装必须看到同一份真相，而
//! registry.npmmirror.com 对同一批包的同步是逐包到达的（2026-10-28 实测：
//! @deepseek-ai/dsh@0.2.2-alpha.2 与部分子包已同步，而 dsh-fs / dsh-commands
//! / dsh-session-telemetry-otel / dsh-web-deepseek-api 仍停在旧版本线）。
//! 以镜像为默认时，版本列表能列出内核版本、安装却在 pnpm 解析阶段失败，
//! 用户看到的错和实际发生的错对不上。
//!
//! 镜像只在传输层不可达（连接失败 / DNS / 超时）时兜底；HTTP 错误、包不存在、
//! 缺具体版本都不换源——那些情况下另一个源不会有不同答案，换源只会把
//! 「上游发布事故」伪装成「镜像延迟」，把用户指向重试网络这条死路。
//!
//! ## 显式覆盖
//!
//! 需要强制使用某个 registry 的部署（内网镜像、离线包）用 DSH_NPM_REGISTRY
//! 覆盖：此时候选链只有它一个，既不以官方为默认、也不再兜底——用户既然明确
//! 指定了源，壳不替他再挑一个。显式覆盖只接受 HTTPS：http / 带 userinfo /
//! 带 query 或 fragment 的 base 会在网络请求前直接报错，不会静默回落官方源。
//!
//! ## 下载域与重定向（2026-11-01 OPT-02）
//!
//! packument 里的 dist.tarball 与每一次重定向目标都走同一套校验：HTTPS、
//! 不含 userinfo、跳数有上限；tarball 的主机必须是服务该 packument 的 registry
//! 主机，或由 DSH_NPM_TARBALL_HOSTS（逗号分隔）显式声明的私服 / CDN 主机。
//! 这样官方源、镜像、自建私服都能工作，又不会因为任意重定向被带到别的域。
//!
//! 重定向**不交给 HTTP 客户端自动跟随**（2026-11-02）：自动跟随要等响应回来才
//! 知道去过哪，那时被禁止的目的地已经收到请求了。`releases` 手动逐跳跟随，
//! 每一跳发请求之前调用本模块的校验，因此被拒绝的目标收到的请求数为 0。
//!
//! 调用方按 format!("{base}{path}") 拼装 URL；每条候选末尾的斜杠是必需的。

use url::Url;

/// 官方 npm registry。版本列表与安装的默认来源。
pub const OFFICIAL_NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// 国内镜像。只在官方源传输层不可达时兜底，不作为默认源。
pub const MIRROR_NPM_REGISTRY: &str = "https://registry.npmmirror.com/";

/// 默认的首选源。不要把它改回镜像：镜像同步滞后时版本列表与安装会看到
/// 两份不同的版本线（见模块文档）。
pub const DEFAULT_NPM_REGISTRY: &str = OFFICIAL_NPM_REGISTRY;

/// 启动时读取的环境变量，用于覆盖 registry 基础 URL。
pub const NPM_REGISTRY_ENV: &str = "DSH_NPM_REGISTRY";

/// 自定义 tarball 下载域（逗号分隔）。官方源、镜像与服务 packument 的 registry
/// 主机自动允许；私服把 tarball 放在别的 CDN / 域名时用它显式声明。
pub const NPM_TARBALL_HOSTS_ENV: &str = "DSH_NPM_TARBALL_HOSTS";

/// 按优先级排列的候选 registry。
///
/// 默认（未设置 DSH_NPM_REGISTRY）是 [官方, 镜像]：调用方遇到传输层
/// 失败才可尝试下一个；显式覆盖时只有一个元素，调用方不得再兜底。
///
/// 每次调用都重新读环境变量：GUI 进程内 fork-spawn 的频率很低，实时读取
/// 让测试与部署覆盖都能立刻生效。配置不合法时返回可操作的中文原因，
/// 调用方不得静默改用官方源。
pub fn npm_registry_candidates() -> Result<Vec<String>, String> {
    candidates(std::env::var(NPM_REGISTRY_ENV).ok().as_deref())
}

/// 当前首选的 registry 基础 URL（候选链第一项）。
///
/// 需要「多个源依次尝试」的调用方用 [npm_registry_candidates]；单 URL 的
/// 调用点（写 .npmrc、拼一个确定的请求地址）用这个。
pub fn npm_registry_base() -> Result<String, String> {
    npm_registry_candidates().map(|bases| {
        bases
            .into_iter()
            .next()
            .unwrap_or_else(|| DEFAULT_NPM_REGISTRY.to_string())
    })
}

/// 抽取出来的纯解析函数，以便测试在不动进程环境的情况下驱动它
/// （进程环境操作会与并发测试产生竞态）。
fn candidates(override_value: Option<&str>) -> Result<Vec<String>, String> {
    match override_value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => Ok(vec![normalize_registry(raw)?]),
        None => Ok(vec![
            normalize_registry(DEFAULT_NPM_REGISTRY)?,
            normalize_registry(MIRROR_NPM_REGISTRY)?,
        ]),
    }
}

/// 校验并规范化一个 registry base：只接受 HTTPS、无 userinfo、无 query /
/// fragment，路径统一补末尾斜杠（调用方按 {base}{path} 拼接）。
fn normalize_registry(raw: &str) -> Result<String, String> {
    let parsed = validate_https_url(raw, "DSH_NPM_REGISTRY")?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(
            "DSH_NPM_REGISTRY 不能带 query 或 fragment：registry 基础地址只应包含 https://主机/路径"
                .to_string(),
        );
    }
    let mut base = parsed;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    // Url 已做 scheme / host 小写归一化；to_string() 自带末尾斜杠时保持稳定。
    Ok(base.to_string())
}

/// 校验一个取源 URL：必须是 HTTPS、host 合法、不得带 userinfo。
///
/// what 用于报错文案（例如 DSH_NPM_REGISTRY / 下载地址），错误里不回显
/// 原始 URL——它可能带 token。
pub fn validate_https_url(raw: &str, what: &str) -> Result<Url, String> {
    let parsed = Url::parse(raw.trim())
        .map_err(|e| format!("{what} 不是合法的 URL（{e}）；需要 https://主机[/路径]"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "{what} 只接受 https，当前是 {}；请改成 https://… 后重试",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!(
            "{what} 不允许在 URL 里携带用户名或密码；请改用不内嵌凭据的 https 地址"
        ));
    }
    if parsed.host_str().is_none() {
        return Err(format!("{what} 缺少主机名"));
    }
    Ok(parsed)
}

/// 校验一跳重定向 / 下载 URL 链：逐条要求 HTTPS、无 userinfo；给了
/// allowed_hosts 时还要求主机在允许列表里（大小写不敏感）。
///
/// 纯函数。releases 的手动逐跳跟随在**每一跳发请求之前**调用它，因此被
/// 拒绝的目的地不会收到任何请求。
pub fn validate_redirect_chain(
    urls: &[String],
    allowed_hosts: Option<&[String]>,
) -> Result<(), String> {
    for raw in urls {
        let parsed = Url::parse(raw)
            .map_err(|e| format!("重定向目标不是合法 URL（{e}）；已在下载前中止"))?;
        if parsed.scheme() != "https" {
            return Err(format!(
                "重定向目标必须保持 https（收到 {}）；已在下载前中止",
                parsed.scheme()
            ));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("重定向目标不允许携带用户名或密码；已在下载前中止".to_string());
        }
        if let Some(hosts) = allowed_hosts {
            if parsed.host_str().is_none() {
                return Err("重定向目标缺少主机名；已在下载前中止".to_string());
            }
            host_allowed(&parsed, hosts)?;
        }
    }
    Ok(())
}

/// 主机是否在允许列表内（大小写不敏感），不在时给出可操作的配置提示。
pub fn host_allowed(parsed: &Url, allowed_hosts: &[String]) -> Result<(), String> {
    let host = parsed
        .host_str()
        .ok_or_else(|| "取源地址缺少主机名；已在下载前中止".to_string())?
        .to_ascii_lowercase();
    if allowed_hosts.iter().any(|allowed| allowed == &host) {
        Ok(())
    } else {
        Err(format!(
            "下载域名 {host} 不在允许列表内；如果是自建 registry，请在 DSH_NPM_TARBALL_HOSTS 里显式声明"
        ))
    }
}

/// tarball 允许的下载主机：服务 packument 的 registry 主机 + DSH_NPM_TARBALL_HOSTS。
///
/// 读环境变量的入口；纯函数版本是 [tarball_hosts]，供单测驱动。
pub fn allowed_tarball_hosts(registry_base: &str) -> Result<Vec<String>, String> {
    tarball_hosts(
        registry_base,
        std::env::var(NPM_TARBALL_HOSTS_ENV).ok().as_deref(),
    )
}

fn tarball_hosts(registry_base: &str, extra: Option<&str>) -> Result<Vec<String>, String> {
    let base = validate_https_url(registry_base, "registry base")?;
    let host = base
        .host_str()
        .ok_or_else(|| "registry base 缺少主机名".to_string())?
        .to_ascii_lowercase();
    let mut hosts = vec![host];
    for raw in extra.unwrap_or("").split(',') {
        let candidate = raw.trim();
        if candidate.is_empty() {
            continue;
        }
        if candidate.contains(['/', '@', '?', '#'])
            || candidate.contains(':')
            || candidate.chars().any(char::is_whitespace)
        {
            return Err(format!(
                "DSH_NPM_TARBALL_HOSTS 只接受主机名（不含协议、端口、路径）：{candidate:?}"
            ));
        }
        let host = Url::parse(&format!("https://{candidate}/"))
            .map_err(|_| format!("DSH_NPM_TARBALL_HOSTS 里有不合法的主机名：{candidate:?}"))?
            .host_str()
            .ok_or_else(|| format!("DSH_NPM_TARBALL_HOSTS 里有不合法的主机名：{candidate:?}"))?
            .to_ascii_lowercase();
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_candidates_are_official_then_mirror() {
        assert_eq!(
            candidates(None).expect("默认候选必须合法"),
            vec![
                OFFICIAL_NPM_REGISTRY.to_string(),
                MIRROR_NPM_REGISTRY.to_string()
            ],
            "默认必须官方优先、镜像只做第二条兜底；顺序反过来会重现「列表能列出、安装装不上」"
        );
        assert_eq!(
            candidates(None).expect("默认候选必须合法").remove(0),
            OFFICIAL_NPM_REGISTRY,
            "首选源必须是官方 registry"
        );
    }

    #[test]
    fn explicit_override_has_no_fallback() {
        assert_eq!(
            candidates(Some("https://r.example.com")).expect("https 私服"),
            vec!["https://r.example.com/".to_string()],
            "显式覆盖是用户指定，不得再自动换源"
        );
        assert_eq!(
            candidates(Some("https://registry.npmmirror.com/")).expect("镜像"),
            vec![MIRROR_NPM_REGISTRY.to_string()],
            "把镜像显式写成默认也只走镜像：是否兜底由候选链长度表达"
        );
    }

    #[test]
    fn empty_or_whitespace_falls_back_to_default() {
        assert_eq!(
            candidates(Some("")).expect("空覆盖"),
            candidates(None).expect("默认")
        );
        assert_eq!(
            candidates(Some("   ")).expect("空白覆盖"),
            candidates(None).expect("默认")
        );
    }

    #[test]
    fn override_is_trimmed_and_trailing_slash_is_enforced() {
        assert_eq!(
            candidates(Some("  https://r.example.com  ")).expect("带空白"),
            vec!["https://r.example.com/".to_string()]
        );
        assert!(candidates(Some("https://r.example.com/npm"))
            .expect("带路径")
            .iter()
            .all(|base| base.ends_with('/')));
        assert!(candidates(None)
            .expect("默认")
            .iter()
            .all(|base| base.ends_with('/')));
    }

    #[test]
    fn rejects_plaintext_and_credentialed_registry() {
        for raw in [
            "http://r.example.com",
            "ftp://r.example.com",
            "https://user:token@r.example.com",
            "https://r.example.com/?token=1",
            "https://r.example.com/#frag",
            "not a url",
            "https://",
        ] {
            assert!(candidates(Some(raw)).is_err(), "{raw} 必须被拒绝");
        }
        assert!(candidates(Some("http://r.example.com"))
            .expect_err("http 必须拒绝")
            .contains("只接受 https"));
    }

    #[test]
    fn normalizes_host_case_and_port() {
        assert_eq!(
            candidates(Some("https://R.Example.COM:8443/")).expect("大小写与端口"),
            vec!["https://r.example.com:8443/".to_string()]
        );
    }

    #[test]
    fn redirect_chain_requires_https_and_allowed_hosts() {
        let https = vec!["https://registry.npmjs.org/a".to_string()];
        assert!(validate_redirect_chain(&https, None).is_ok());
        assert!(
            validate_redirect_chain(&["http://registry.npmjs.org/a".to_string()], None).is_err()
        );
        assert!(validate_redirect_chain(
            &["https://user:token@registry.npmjs.org/a".to_string()],
            None
        )
        .is_err());

        let hosts = vec![
            "registry.npmjs.org".to_string(),
            "cdn.example.com".to_string(),
        ];
        assert!(validate_redirect_chain(
            &["https://cdn.example.com/a.tgz".to_string()],
            Some(&hosts)
        )
        .is_ok());
        assert!(validate_redirect_chain(
            &["https://evil.example.com/a.tgz".to_string()],
            Some(&hosts)
        )
        .is_err());
        // 链里任何一跳越界都整体失败，而不是只看最后一跳。
        assert!(validate_redirect_chain(
            &[
                "https://registry.npmjs.org/a".to_string(),
                "https://evil.example.com/a".to_string(),
                "https://registry.npmjs.org/a".to_string(),
            ],
            Some(&hosts)
        )
        .is_err());
    }

    #[test]
    fn tarball_hosts_include_registry_and_explicit_extra_hosts() {
        let hosts = tarball_hosts(
            "https://registry.npmjs.org/",
            Some("cdn.example.com, files.example.com"),
        )
        .expect("hosts");
        assert_eq!(
            hosts,
            vec![
                "registry.npmjs.org".to_string(),
                "cdn.example.com".to_string(),
                "files.example.com".to_string()
            ]
        );
        assert!(tarball_hosts("http://registry.example.com/", None).is_err());
        assert!(tarball_hosts("https://registry.example.com/", Some("http://cdn")).is_err());
        assert!(
            tarball_hosts("https://registry.example.com/", Some("cdn.example.com:443")).is_err()
        );
    }

    #[test]
    fn validate_https_url_rejects_userinfo_and_plaintext() {
        assert!(validate_https_url("https://r.example.com/a", "下载地址").is_ok());
        assert!(validate_https_url("http://r.example.com/a", "下载地址").is_err());
        assert!(validate_https_url("https://u:p@r.example.com/a", "下载地址").is_err());
    }
}
