//! npm registry configuration for the desktop shell.
//!
//! Every byte the shell pulls from the npm ecosystem — kernel installs,
//! plugin installs, profile wiring, kernel release listings, plugin
//! metadata, tarball downloads — is pinned to one base URL. The default
//! points at the npmmirror mirror so installs work on a Chinese-network
//! machine without touching the user's global npm config. Deployments
//! that need the upstream registry or a different mirror override the
//! default through the `DSH_NPM_REGISTRY` environment variable without
//! rebuilding.
//!
//! Callers compose the URL as `format!("{base}{pkg}")`; the trailing
//! slash on the default is required so unscoped packages resolve
//! correctly.

/// Default npm registry base. The trailing slash is load-bearing for URL
/// composition at call sites; `resolve` re-asserts it on every read.
pub const DEFAULT_NPM_REGISTRY: &str = "https://registry.npmmirror.com/";

/// Environment variable consulted at startup to override the registry
/// base. Empty / whitespace-only values fall back to the default.
pub const NPM_REGISTRY_ENV: &str = "DSH_NPM_REGISTRY";

/// Effective npm registry base URL. Reads `DSH_NPM_REGISTRY` from the
/// process environment on every call; the shell is a GUI app and does not
/// fork-spawn frequently enough for the read to matter, and live reload
/// (e.g. test fixtures) is worth more than a cached value.
///
/// Returns a `String` (rather than `&'static str`) because the override
/// is process-local state, not a constant.
pub fn npm_registry_base() -> String {
    resolve(std::env::var(NPM_REGISTRY_ENV).ok().as_deref())
}

/// Pure resolver split out so tests can drive it without mutating the
/// process environment (which would race parallel tests).
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
