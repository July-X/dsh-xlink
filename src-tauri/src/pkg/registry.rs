//! 桌面外壳的 npm registry 配置。
//!
//! 外壳从 npm 生态拉取的所有内容——内核安装、插件安装、profile 接入、
//! 内核发布列表、插件元数据、tarball 下载——都绑定到同一个基础 URL。
//! 默认指向 npmmirror 镜像，使得国内网络环境下的安装无需触及用户的全局
//! npm 配置。需要使用上游 registry 或其他镜像的部署，可以通过
//! `DSH_NPM_REGISTRY` 环境变量覆盖默认值，无需重新构建。
//!
//! 调用方按 `format!("{base}{pkg}")` 拼装 URL；默认值末尾的斜杠是
//! 必需的，以保证无命名空间包能正确解析。

/// 默认的 npm registry 基础 URL。末尾的斜杠对调用方拼装 URL 至关重要；
/// `resolve` 在每次读取时都会重新确认它的存在。
pub const DEFAULT_NPM_REGISTRY: &str = "https://registry.npmmirror.com/";

/// 启动时读取的环境变量，用于覆盖 registry 基础 URL。
/// 为空或仅含空白字符时回退到默认值。
pub const NPM_REGISTRY_ENV: &str = "DSH_NPM_REGISTRY";

/// 实际生效的 npm registry 基础 URL。每次调用都会从进程环境读取
/// `DSH_NPM_REGISTRY`；外壳是 GUI 应用，fork-spawn 的频率并不高，
/// 这次读取开销可以忽略，并且实时重新加载（例如测试 fixture）的
/// 价值要高于缓存值。
///
/// 返回 `String`（而不是 `&'static str`），因为覆盖值是进程级本地状态，
/// 而不是常量。
pub fn npm_registry_base() -> String {
    resolve(std::env::var(NPM_REGISTRY_ENV).ok().as_deref())
}

/// 抽取出来的纯解析函数，以便测试在不动进程环境的情况下驱动它
/// （进程环境操作会与并发测试产生竞态）。
fn resolve(override_value: Option<&str>) -> String {
    let raw = override_value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_NPM_REGISTRY);
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
    fn default_when_unset() {
        assert_eq!(resolve(None), DEFAULT_NPM_REGISTRY);
    }

    #[test]
    fn empty_or_whitespace_falls_back_to_default() {
        assert_eq!(resolve(Some("")), DEFAULT_NPM_REGISTRY);
        assert_eq!(resolve(Some("   ")), DEFAULT_NPM_REGISTRY);
    }

    #[test]
    fn override_wins() {
        assert_eq!(
            resolve(Some("https://r.example.com")),
            "https://r.example.com/"
        );
        assert_eq!(
            resolve(Some("https://r.example.com/")),
            "https://r.example.com/"
        );
    }

    #[test]
    fn override_is_trimmed() {
        assert_eq!(
            resolve(Some("  https://r.example.com  ")),
            "https://r.example.com/"
        );
    }

    #[test]
    fn trailing_slash_is_enforced() {
        assert!(resolve(Some("https://r.example.com")).ends_with('/'));
        assert!(resolve(Some("https://r.example.com/")).ends_with('/'));
        assert!(resolve(None).ends_with('/'));
    }
}
