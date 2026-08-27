//! Fetching the official kernel release list from the
//! `@deepseek-ai/dsh` npm package.
//!
//! The primary source is the public npm registry, which is the canonical
//! destination the user navigates to when checking for updates
//! (`https://www.npmjs.com/package/@deepseek-ai/dsh`). Its JSON document
//! carries every published version plus `dist-tags` (latest, next, beta,
//! …), so the update menu has accurate prerelease flags and timestamps.
//!
//! GitHub is kept as a fallback: when the registry is unreachable, the
//! shell falls back to the GitHub REST API and then to its public Atom
//! feed (which is not rate-limited and exposes the same tags). The
//! fallback warning flows back to the UI so users can tell which source
//! they are looking at.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::version::cmp_versions;

/// Tag prefix on the official releases (e.g. `dsh-v0.1.1-rc.2`).
pub const TAG_PREFIX: &str = "dsh-v";

const USER_AGENT: &str = concat!("dsh-desktop/", env!("CARGO_PKG_VERSION"));

/// GET `url` and return the body as text. Every shell fetch goes out with
/// the desktop User-Agent; `accept` carries an extra Accept header for the
/// one endpoint that expects it (GitHub's REST API).
pub(crate) fn http_get_string(url: &str, accept: Option<&str>) -> Result<String, String> {
    let request = ureq::get(url).header("User-Agent", USER_AGENT);
    let request = match accept {
        Some(value) => request.header("Accept", value),
        None => request,
    };
    let mut response = request.call().map_err(|e: ureq::Error| e.to_string())?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e: ureq::Error| e.to_string())
}

/// GET `url` and return the body as bytes (npm tarball downloads).
pub(crate) fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e: ureq::Error| e.to_string())?;
    response
        .body_mut()
        .read_to_vec()
        .map_err(|e: ureq::Error| e.to_string())
}

/// npm registry endpoint for the `@deepseek-ai/dsh` package. The npm
/// registry returns a full JSON document with all published versions,
/// `dist-tags`, scripts, and metadata; it is what `npm view` reads.
/// Resolved through `registry::npm_registry_base()` so the shell's mirror
/// choice applies to the release list the update menu consumes.
fn npm_registry_url() -> String {
    format!("{}@deepseek-ai/dsh", crate::registry::npm_registry_base())
}
/// Human-facing web URL used in the update menu's "open release" link.
pub const NPM_PACKAGE_URL: &str = "https://www.npmjs.com/package/@deepseek-ai/dsh";

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/deepseek-ai/deepseek-harness/releases?per_page=30";
const GITHUB_ATOM_URL: &str = "https://github.com/deepseek-ai/deepseek-harness/releases.atom";

/// One official kernel release, as the update menu shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseInfo {
    /// Full tag, e.g. `dsh-v0.1.1-rc.2`.
    pub tag: String,
    /// Version extracted from the tag, e.g. `0.1.1-rc.2`.
    pub version: String,
    pub prerelease: bool,
    pub name: String,
    pub published_at: Option<String>,
    pub html_url: String,
}

/// Strip the `dsh-v` prefix; `None` for unrelated tags.
pub fn version_from_tag(tag: &str) -> Option<String> {
    let rest = tag.strip_prefix(TAG_PREFIX)?;
    (!rest.is_empty()).then(|| rest.to_string())
}

fn release_from_tag(tag: String, prerelease: bool) -> Option<ReleaseInfo> {
    let version = version_from_tag(&tag)?;
    Some(ReleaseInfo {
        tag: tag.clone(),
        version,
        prerelease,
        // `name` and `html_url` both consume `tag`; clone once for the other.
        name: tag.clone(),
        published_at: None,
        // The human-facing release URL now points at the npm package page;
        // GitHub and npm publish under the same tag, so users still see
        // their expected version there.
        html_url: NPM_PACKAGE_URL.to_string(),
    })
}

/// One entry from the npm `versions` object — only the fields we actually
/// need to render the update menu are deserialized.
#[derive(Debug, Deserialize)]
struct NpmVersion {
    /// `time[version]` from npm is read separately; we fall back to this
    /// when present (some mirrors only expose `time` inside the version).
    #[serde(default)]
    #[serde(rename = "date")]
    date: Option<String>,
    #[serde(default)]
    deprecated: Option<String>,
}

/// Top-level shape of the npm registry response for a single package.
#[derive(Debug, Deserialize)]
struct NpmPackageDoc {
    /// The full set of published versions: `version -> metadata`.
    #[serde(default)]
    versions: BTreeMap<String, NpmVersion>,
    /// Per-version publish timestamps (plus `created`/`modified` entries,
    /// which are not versions). This is where npm actually carries the
    /// publish date; `NpmVersion::date` is only a mirror fallback.
    #[serde(default)]
    time: BTreeMap<String, String>,
    /// `dist-tags`: `latest`, `next`, `beta`, etc. Versions tagged here are
    /// the only ones we treat as prereleases; everything else is stable.
    #[serde(rename = "dist-tags", default)]
    dist_tags: BTreeMap<String, String>,
}

impl NpmPackageDoc {
    /// Build a version → prerelease map from `dist-tags`. We treat every
    /// dist-tag other than `latest` as a prerelease channel; npm itself
    /// only ever has one `latest`, but maintainers sometimes publish
    /// `next`, `beta`, `rc`, etc.
    fn prerelease_versions(&self) -> std::collections::HashSet<String> {
        self.dist_tags
            .iter()
            .filter(|(tag, _)| tag.as_str() != "latest")
            .map(|(_, v)| v.clone())
            .collect()
    }
}

/// Fetch the kernel version list from the npm registry.
fn fetch_npm() -> Result<Vec<ReleaseInfo>, String> {
    let url = npm_registry_url();
    let body = http_get_string(&url, None)?;
    let pkg: NpmPackageDoc =
        serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;

    let prereleases = pkg.prerelease_versions();
    let times = pkg.time;
    let mut out: Vec<ReleaseInfo> = pkg
        .versions
        .into_iter()
        .filter(|(version, meta)| {
            // npm sometimes ships placeholder or yanked entries; skip them
            // so the update menu never offers an unusable version.
            !version.is_empty()
                && meta.deprecated.is_none()
                && !version.contains(' ')
                && !version.contains('/')
        })
        .map(|(version, meta)| {
            let tag = format!("{TAG_PREFIX}{version}");
            let prerelease = prereleases.contains(&version) || version.contains('-');
            ReleaseInfo {
                tag: tag.clone(),
                version: version.clone(),
                prerelease,
                name: tag,
                published_at: times.get(&version).cloned().or(meta.date),
                html_url: format!("{NPM_PACKAGE_URL}/v/{version}"),
            }
        })
        .collect();
    out.sort_by(|a, b| cmp_versions(&a.version, &b.version).reverse());
    if out.is_empty() {
        return Err("npm registry 未返回任何 @deepseek-ai/dsh 版本".into());
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// Pull the `dsh-v*` release tags from the GitHub API (fallback).
fn fetch_api() -> Result<Vec<ReleaseInfo>, String> {
    let body = http_get_string(GITHUB_API_URL, Some("application/vnd.github+json"))?;
    let releases: Vec<GhRelease> =
        serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;
    let mut out: Vec<ReleaseInfo> = releases
        .into_iter()
        .filter_map(|r| {
            let mut info = release_from_tag(r.tag_name, r.prerelease)?;
            info.published_at = r.published_at;
            if let Some(url) = r.html_url {
                info.html_url = url;
            }
            Some(info)
        })
        .collect();
    out.sort_by(|a, b| cmp_versions(&a.version, &b.version).reverse());
    Ok(out)
}

/// Parse `dsh-v*` entry titles out of the releases Atom feed.
///
/// The feed looks like
/// `<entry><title>dsh-v0.1.1-rc.2</title>...</entry>`, but the feed can
/// embed HTML entities (`&lt;`, `&gt;`, `&amp;`) inside titles, and the body
/// has a leading `<title>`/`<updated>` header pair that also needs to be
/// matched. We slice on byte indices, so every offset into `rest` must be
/// recomputed against the original buffer — using the same offset for the
/// pre- and post-`</title>` slices (as a previous version did) drifts once a
/// title contains a multi-byte UTF-8 character and can split a code point.
fn parse_atom(xml: &str) -> Vec<ReleaseInfo> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel_start) = xml[cursor..].find("<title>") {
        let title_start = cursor + rel_start;
        let after_open = title_start + "<title>".len();
        let rel_end = xml[after_open..].find("</title>");
        let title_end = match rel_end {
            Some(r) => after_open + r,
            None => break,
        };
        let title = xml[after_open..title_end].trim();
        if let Some(info) = release_from_tag(title.to_string(), false) {
            out.push(info);
        }
        cursor = title_end + "</title>".len();
    }
    out.sort_by(|a, b| cmp_versions(&a.version, &b.version).reverse());
    out
}

/// Pull the `dsh-v*` release tags from the Atom feed (fallback).
fn fetch_atom() -> Result<Vec<ReleaseInfo>, String> {
    let body = http_get_string(GITHUB_ATOM_URL, None)?;
    let out = parse_atom(&body);
    if out.is_empty() {
        return Err("Atom feed 未解析到 dsh-v* 标签".into());
    }
    Ok(out)
}

/// Result of listing releases: the data plus any fallback warning.
#[derive(Serialize)]
pub struct ReleaseList {
    pub releases: Vec<ReleaseInfo>,
    pub warning: Option<String>,
}

/// List official kernel releases, newest first.
///
/// Source order:
/// 1. npm registry (`https://www.npmjs.com/package/@deepseek-ai/dsh`) —
///    canonical destination users see when checking for updates.
/// 2. GitHub REST API — used only when the registry is unreachable.
/// 3. GitHub Atom feed — last resort; not rate-limited but lacks
///    `prerelease` flags.
///
/// A fallback sets `warning` so the UI can tell the user which source
/// they are looking at, and that npm-side prerelease markers may be
/// missing.
pub fn list_releases() -> Result<ReleaseList, AppError> {
    match fetch_npm() {
        Ok(out) if !out.is_empty() => Ok(ReleaseList { releases: out, warning: None }),
        Ok(_) => Err(AppError::GitHub("npm registry 未返回任何 @deepseek-ai/dsh 版本".into())),
        Err(npm_err) => match fetch_api() {
            Ok(out) if !out.is_empty() => Ok(ReleaseList {
                releases: out,
                warning: Some(format!(
                    "npm registry 不可用（{npm_err}），已回退到 GitHub Releases API"
                )),
            }),
            Ok(_) => {
                let api_err = "GitHub Releases API 返回空列表".to_string();
                match fetch_atom() {
                    Ok(out) => Ok(ReleaseList {
                        releases: out,
                        warning: Some(format!(
                            "npm registry 与 GitHub API 均不可用（npm：{npm_err}；api：{api_err}），已回退到 GitHub Atom feed（prerelease 标记可能不完整）"
                        )),
                    }),
                    Err(atom_err) => Err(AppError::GitHub(format!(
                        "全部源不可用 — npm：{npm_err}；GitHub API：{api_err}；GitHub Atom：{atom_err}"
                    ))),
                }
            }
            Err(api_err) => match fetch_atom() {
                Ok(out) => Ok(ReleaseList {
                    releases: out,
                    warning: Some(format!(
                        "npm registry 与 GitHub API 均不可用（npm：{npm_err}；api：{api_err}），已回退到 GitHub Atom feed（prerelease 标记可能不完整）"
                    )),
                }),
                Err(atom_err) => Err(AppError::GitHub(format!(
                    "全部源不可用 — npm：{npm_err}；GitHub API：{api_err}；GitHub Atom：{atom_err}"
                ))),
            },
        },
    }
}
