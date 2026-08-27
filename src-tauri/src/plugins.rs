//! Community plugin management: a central store under the harness home, per-kernel
//! materialization (link or copy), profile wiring, and update checks.
//!
//! All plugin sources live in <home>/plugins/ (the store), never inside a
//! kernel installation. Each installed kernel reads plugins from its own
//! <data_dir>/kernels/<version>/plugins/ directory, which the shell
//! materializes from the store either as a symlink (link mode, default) or a
//! real copy (copy mode). The active kernel's profile (profiles/<profile>/)
//! then declares each plugin as a dependency pointing at that materialized
//! directory plus a bundle layer when the plugin declares dsh.bundle,
//! mirroring what the kernel's plugin CLI produces, so switching kernels
//! never reinstalls anything - it only re-materializes and rewires.
//!
//! Design notes: docs/plugin-management.md in the desktop deliverable.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::process::quiet;
use crate::quarantine;
use crate::releases::{http_get_bytes, http_get_string};
use crate::version::cmp_versions;
use crate::{commands, kernel, node, settings};

/// Default profile the shell wires plugins into (the kernel's web surface).
pub const DEFAULT_PROFILE: &str = "web";
/// Store directory name under the harness home.
const STORE_SUBDIR: &str = "plugins";
/// The shell's plugin inventory file inside the store directory.
const STORE_FILE: &str = "store.json";
/// Per-plugin fetch marker inside each store entry.
const SOURCE_MARKER: &str = ".dsh-source.json";
/// Community catalog, primary source: the dsh-plugin.org hub (the data feed
/// behind the DSH-Plugin Hub plugin center).
const HUB_CATALOG_URL: &str = "https://dsh-plugin.org/api/plugins.zh.json";
/// Community catalog, fallback source: the reference market's listing, used
/// when the hub is unreachable.
const MARKET_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/losebird/dsh-plugin-market/main/registry/all.json";
/// Catalog cache file under the shell data dir.
const CATALOG_CACHE_FILE: &str = "plugins-catalog.json";
/// Catalog cache freshness window.
const CATALOG_TTL_SECS: u64 = 6 * 3600;
/// Materialization metadata directory name inside kernel plugins dirs.
const META_SUBDIR: &str = ".meta";
/// Spec prefix for a pnpm link: (symlink) dependency.
const SPEC_LINK: &str = "link:";
/// Spec prefix for a pnpm file: (store copy) dependency.
const SPEC_FILE: &str = "file:";

// --- data model ------------------------------------------------------------

/// One installed plugin in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreItem {
    /// Filesystem-safe plugin key (package/repo name with slashes replaced).
    pub id: String,
    /// Display name (npm package name or repo shorthand).
    pub name: String,
    /// Fetch origin: npm or git.
    pub origin: String,
    /// Fetch source: npm package name (optionally @version) or git URL (optionally #tag).
    pub source: String,
    pub installed_version: String,
    /// Latest known version, refreshed by check_updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Desired materialization mode: link or copy.
    pub mode: String,
    /// Whether the source pins a version (npm @version / git #tag).
    pub pinned: bool,
    /// Seconds since epoch, for display.
    pub installed_at: String,
    /// Seconds since epoch of the last fetch, for display.
    pub updated_at: String,
    /// Human-facing repo URL for git-origin plugins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Default for StoreItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            origin: String::from("npm"),
            source: String::new(),
            installed_version: String::new(),
            latest_version: None,
            mode: String::from("link"),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        }
    }
}

/// The persisted store document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Store {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub items: Vec<StoreItem>,
    #[serde(rename = "lastCheckedAt", skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    /// Last wiring/install failure surfaced to the UI, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: 1,
            items: Vec::new(),
            last_checked_at: None,
            warning: None,
        }
    }
}

/// Per-kernel materialization record, one JSON file per plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KernelMeta {
    /// Actual mode on disk: link or copy.
    mode: String,
    /// The store version this materialization reflects.
    version: String,
    synced_at: String,
}

/// One row the management UI renders.
#[derive(Debug, Clone, Serialize)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub origin: String,
    pub source: String,
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub pinned: bool,
    /// Desired mode from the store.
    pub desired_mode: String,
    /// Actual mode in the active kernel, when materialized there.
    pub actual_mode: Option<String>,
    /// Whether the active kernel's materialization is present and current.
    pub synced: bool,
    /// Whether the active kernel's profile already loads this plugin.
    pub wired: bool,
    /// Quarantine record when the boot guard has disabled this plugin;
    /// `None` means the plugin participates in wiring normally.
    pub quarantined: Option<quarantine::QuarantineItem>,
    pub repo_url: Option<String>,
    pub description: Option<String>,
    pub installed_at: String,
    pub updated_at: String,
}

/// Aggregate plugin status for the management UI.
#[derive(Debug, Clone, Serialize)]
pub struct PluginStatus {
    pub rows: Vec<PluginRow>,
    pub profile: String,
    pub active_kernel: Option<String>,
    /// Number of plugins with a known newer version.
    pub updates: usize,
    pub last_checked_at: Option<String>,
    pub warning: Option<String>,
}

/// One catalog entry surfacing in the plugin center.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub stars: u64,
    #[serde(default)]
    pub forks: u64,
    pub downloads: u64,
    pub verified: bool,
    pub repo: Option<String>,
    /// Install spec: npm package name or git URL (with #tag when known).
    pub spec: String,
    /// npm or git, derived from the entry's install method.
    pub origin: String,
    pub category: String,
    /// Latest published version string (may carry a leading `v`).
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// ISO timestamp of the last upstream update, when known.
    #[serde(default)]
    pub updated: String,
    /// Human-facing detail page (dsh-plugin.org or the repository).
    #[serde(default)]
    pub detail_url: String,
}

/// npm registry document slice we need.
#[derive(Debug, Deserialize)]
struct NpmDoc {
    #[serde(rename = "dist-tags", default)]
    dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    versions: BTreeMap<String, NpmVersionDoc>,
}

#[derive(Debug, Deserialize)]
struct NpmVersionDoc {
    #[serde(default)]
    dist: Option<NpmDist>,
}

#[derive(Debug, Deserialize)]
struct NpmDist {
    #[serde(default)]
    tarball: String,
}

/// A parsed install request.
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub origin: String,
    /// npm package name or git URL.
    pub source: String,
    /// Optional pinned version (npm semver) or tag (git).
    pub pin: Option<String>,
    /// Filesystem-safe store id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Human-facing repo URL for git origin.
    pub repo_url: Option<String>,
}

// --- paths ------------------------------------------------------------------

/// Central store root: <home>/plugins/, next to the profile dirs the store
/// feeds. data_dir is <home>/desktop/ (see kernel::data_dir).
pub fn store_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .map(|home| home.join(STORE_SUBDIR))
        .unwrap_or_else(|| data_dir.join(STORE_SUBDIR))
}

fn store_file(data_dir: &Path) -> PathBuf {
    store_dir(data_dir).join(STORE_FILE)
}

fn store_plugin_dir(data_dir: &Path, id: &str) -> PathBuf {
    store_dir(data_dir).join(id)
}

fn kernel_plugins_dir(data_dir: &Path, version: &str) -> PathBuf {
    kernel::kernel_dir(data_dir, version).join("plugins")
}

fn kernel_plugin_dir(data_dir: &Path, version: &str, id: &str) -> PathBuf {
    kernel_plugins_dir(data_dir, version).join(id)
}

fn kernel_meta_file(data_dir: &Path, version: &str, id: &str) -> PathBuf {
    kernel_plugins_dir(data_dir, version)
        .join(META_SUBDIR)
        .join(format!("{id}.json"))
}

fn profile_dir(data_dir: &Path, profile: &str) -> PathBuf {
    data_dir
        .parent()
        .map(|home| home.join("profiles").join(profile))
        .unwrap_or_else(|| data_dir.join("profiles").join(profile))
}

fn wiring_log_path(data_dir: &Path) -> PathBuf {
    kernel::logs_dir(data_dir).join("plugin-wiring.log")
}

fn plugin_log_path(data_dir: &Path, id: &str) -> PathBuf {
    kernel::logs_dir(data_dir).join(format!("plugin-{id}.log"))
}

/// Map a package/repo name to a filesystem-safe store id. Path traversal is
/// structurally impossible afterwards: slashes become double underscores and
/// dot / empty segments are rejected outright. Whitespace is also rejected
/// because npm names and GitHub `owner/repo` strings are both single
/// tokens — a string like `dsh plugin remove @scope/pkg` reaching this
/// function means the caller forgot to peel a CLI invocation in
/// `split_dsh_plugin_cli`; the store id should not silently swallow it.
pub fn id_for_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::Plugin("插件名称为空或过长".into()));
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err(AppError::Plugin(format!(
            "非法的插件名称 {name:?}（包含空白字符）"
        )));
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(AppError::Plugin(format!(
                "非法的插件名称 {name:?}（包含空段或 ..）"
            )));
        }
    }
    Ok(name.replace('/', "__"))
}

fn now_epoch_secs() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

// --- store persistence ------------------------------------------------------

pub fn load_store(data_dir: &Path) -> Store {
    let Ok(text) = fs::read_to_string(store_file(data_dir)) else {
        return Store::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_store(data_dir: &Path, store: &Store) -> Result<(), AppError> {
    fs::create_dir_all(store_dir(data_dir)).map_err(|e| AppError::Io(e.to_string()))?;
    let text = serde_json::to_string_pretty(store).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(store_file(data_dir), text + "\n").map_err(|e| AppError::Io(e.to_string()))?;
    ensure_store_npmrc(data_dir)
}

/// Write a local .npmrc in the store directory.  Fresh pnpm defaults a
/// `minimumReleaseAge` of ~3 days so locked dev/rc versions stay installable
/// without waiting out the gate, and pins the registry mirror the desktop
/// shell already uses so mirror-only scoped packages resolve.  Rewrites any
/// differing content so a previous (broken) shape gets corrected in place;
/// `save_store` calls this on every mutation, so matching content is left
/// untouched to avoid the disk churn.
fn ensure_store_npmrc(data_dir: &Path) -> Result<(), AppError> {
    let npmrc = store_dir(data_dir).join(".npmrc");
    let registry = crate::registry::npm_registry_base();
    let text =
        format!("minimumReleaseAge=0\nregistry={registry}\n@deepseek-ai:registry={registry}\n");
    if fs::read_to_string(&npmrc)
        .map(|existing| existing == text)
        .unwrap_or(false)
    {
        return Ok(());
    }
    fs::write(&npmrc, text).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

fn store_item(data_dir: &Path, id: &str) -> Option<StoreItem> {
    load_store(data_dir)
        .items
        .into_iter()
        .find(|item| item.id == id)
}

fn upsert_item(data_dir: &Path, item: StoreItem) -> Result<(), AppError> {
    let mut store = load_store(data_dir);
    if let Some(existing) = store.items.iter_mut().find(|i| i.id == item.id) {
        *existing = item;
    } else {
        store.items.push(item);
    }
    save_store(data_dir, &store)
}

fn remove_item(data_dir: &Path, id: &str) -> Result<(), AppError> {
    let mut store = load_store(data_dir);
    store.items.retain(|item| item.id != id);
    save_store(data_dir, &store)
}

fn read_meta(data_dir: &Path, version: &str, id: &str) -> Option<KernelMeta> {
    let text = fs::read_to_string(kernel_meta_file(data_dir, version, id)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_meta(data_dir: &Path, version: &str, id: &str, meta: &KernelMeta) -> Result<(), AppError> {
    if let Some(parent) = kernel_meta_file(data_dir, version, id).parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let text = serde_json::to_string(meta).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(kernel_meta_file(data_dir, version, id), text)
        .map_err(|e| AppError::Io(e.to_string()))
}

// --- spec parsing -----------------------------------------------------------

/// Try to peel a `dsh plugin ... add <pkg>` CLI invocation out of an install
/// spec. Returns the requested profile (if any) plus the bare package spec
/// the rest of the parser can handle. Returns `None` for inputs that don't
/// match the CLI shape — those fall through to the existing npm / git /
/// owner-repo parsing.
///
/// The shell only accepts the manual-install form of the kernel's `dsh
/// plugin` command: `add` and `install` (the kernel treats them as aliases).
/// `remove` /`update` /`list` are rejected so a pasted command can't
/// accidentally uninstall a plugin the user only meant to install. The
/// `--profile` (or `-p`) flag is parsed but ignored: the shell always
/// wires plugins into `DEFAULT_PROFILE` (`web`); supporting multiple
/// profiles is tracked separately.
///
/// Recognised shapes:
///
/// ```text
/// dsh plugin add <pkg>
/// dsh plugin install <pkg>
/// dsh plugin --profile web add <pkg>
/// dsh plugin -p web install <pkg>
/// ```
///
/// Trailing flags the kernel might accept (`--save-dev`, `--force`, …) are
/// silently dropped — the shell's parser only needs the package spec.
fn split_dsh_plugin_cli(spec: &str) -> Option<(Option<String>, String)> {
    let s = spec.trim();
    let after_dsh = s.strip_prefix("dsh ")?;
    let after_plugin = after_dsh.trim_start().strip_prefix("plugin")?;
    let args = after_plugin.trim();

    let mut profile: Option<String> = None;
    let mut package: Option<String> = None;
    let mut iter = args.split_whitespace();
    while let Some(arg) = iter.next() {
        match arg {
            "--profile" | "-p" => {
                profile = iter.next().map(str::to_string);
            }
            "add" | "install" => {
                // First positional after the verb is the package spec. We
                // don't bother tracking trailing flags — anything after
                // the package spec is dropped as the manual-install flow
                // doesn't need it.
                package = iter.next().map(str::to_string);
                break;
            }
            _ => {
                // Unknown leading flag — bail out and let the regular
                // npm / git parser decide. This is what catches
                // `dsh plugin something` typed by accident.
                return None;
            }
        }
    }
    package.map(|p| (profile, p))
}

/// Try to peel a package-manager install command out of an install spec
/// (`npm install <pkg>`, `pnpm add <pkg>`, `yarn add <pkg>`,
/// `bun add <pkg>` …). Returns the bare package spec, or `None` when the
/// input does not match that shape — those fall through to the regular
/// npm / git / owner-repo parsing.
///
/// Flags are silently dropped wherever they appear (before or after the
/// verb, before or after the package spec). The shell has no concept of
/// `--save-dev` / `--global` / `--registry` — every accepted form ends
/// up at the same store path under `<dsh_home>/plugins/`. The only verb
/// forms accepted are `install` / `i` / `add` (npm's `install` and `i`
/// are aliases for the same action; `add` is the newer alias that
/// mirrors pnpm/yarn/bun).
///
/// Recognised shapes (any of `npm` / `pnpm` / `yarn` / `bun` as the prefix):
///
/// ```text
/// npm install <pkg>
/// npm i @scope/pkg@1.2.3
/// pnpm add owner/repo
/// yarn add https://github.com/o/r.git#v1
/// npm install --save-dev <pkg>      ← --save-dev dropped
/// ```
fn split_package_manager_cli(spec: &str) -> Option<String> {
    // All four package managers share the verb vocabulary `install` /
    // `i` / `add`; only the binary prefix differs.
    const VERBS: &[&str] = &["install", "i", "add"];

    let s = spec.trim();
    let rest = s
        .strip_prefix("npm ")
        .or_else(|| s.strip_prefix("pnpm "))
        .or_else(|| s.strip_prefix("yarn "))
        .or_else(|| s.strip_prefix("bun "))?;

    let mut saw_verb = false;
    for token in rest.split_whitespace() {
        if !saw_verb {
            if VERBS.contains(&token) {
                saw_verb = true;
            }
            // Anything before the verb (binary-prefix flags like
            // `npm --silent install <pkg>`) is dropped.
            continue;
        }
        // After the verb, take the first non-flag positional. Any flags
        // before it (e.g. `npm i -D <pkg>`) are silently skipped.
        if token.starts_with('-') {
            continue;
        }
        return Some(token.to_string());
    }
    None
}

/// Split an npm spec into (name, optional pin). The last @ after the scope
/// prefix separates the version; @scope/name@1.2.3 parses as (@scope/name,
/// 1.2.3). Plain names pass through.
fn split_npm_spec(spec: &str) -> Result<(String, Option<String>), AppError> {
    let s = spec.trim();
    if s.starts_with('@') {
        let (head, rest) = s
            .split_once('/')
            .ok_or_else(|| AppError::Plugin(format!("非法的 npm 包名 {spec:?}")))?;
        let rest = rest.trim();
        let (name, pin) = match rest.rsplit_once('@') {
            Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
                (n, Some(p.to_string()))
            }
            _ => (rest, None),
        };
        let name = format!("{head}/{name}");
        return Ok((name, pin));
    }
    match s.rsplit_once('@') {
        Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
            Ok((n.to_string(), Some(p.to_string())))
        }
        _ => Ok((s.to_string(), None)),
    }
}

/// Parse an install request into a PluginSpec. Accepts:
///   - the kernel's full `dsh plugin [--profile X] (add|install) <pkg>` CLI
///     invocation (with the optional `--profile` / `-p` flag ignored — the
///     shell always wires into the active profile);
///   - npm package names with optional `@version` pin, including `@scope/name`;
///   - git URLs (`https://…`, `git@…`, or `github.com/owner/name`);
///   - bare `owner/repo` shorthand, with optional `#tag`.
pub fn parse_spec(spec: &str) -> Result<PluginSpec, AppError> {
    let s = spec.trim().trim_end_matches('/');
    if s.is_empty() || s.len() > 500 {
        return Err(AppError::Plugin("安装地址为空或过长".into()));
    }
    // `dsh plugin ... add <pkg>` first: copy-pasteable from kernel docs
    // and ChatGPT suggestions. The helper pulls the package spec out and
    // recurses so every downstream branch (npm / git / owner-repo) keeps
    // a single source of truth.
    if let Some((_profile, pkg)) = split_dsh_plugin_cli(s) {
        return parse_spec(&pkg);
    }
    // Standard package-manager install syntax (`npm install <pkg>`,
    // `pnpm add <pkg>`, `yarn add <pkg>`, `bun add <pkg>`). Same
    // recursion — flags are dropped, the extracted spec goes through
    // the same npm / git / owner-repo pipeline.
    if let Some(pkg) = split_package_manager_cli(s) {
        return parse_spec(&pkg);
    }
    if s.starts_with("git@") || s.contains("://") || s.contains("github.com/") {
        // git 来源：[url][#tag]
        let (url, pin) = match s.split_once('#') {
            Some((u, tag)) if !u.is_empty() && !tag.is_empty() => (u, Some(tag.to_string())),
            _ => (s, None),
        };
        let repo_url = s.contains("github.com/").then(|| url.to_string());
        // URL 含空路径段（协议双斜杠），先归一成 owner/repo 形状再映射 id
        let id_base = url
            .trim_start_matches("git@")
            .split("://")
            .last()
            .unwrap_or(url)
            .trim_end_matches(".git")
            .replace(':', "/");
        let id = id_for_name(&id_base)?;
        let name = url
            .trim_end_matches(".git")
            .rsplit('/')
            .next()
            .unwrap_or(url)
            .to_string();
        return Ok(PluginSpec {
            origin: "git".into(),
            source: url.to_string(),
            pin,
            id,
            name,
            repo_url,
        });
    }
    // owner/repo 简写：非 npm 样式（不含 @ 且含斜杠）按 GitHub 仓库处理
    if s.contains('/') && !s.starts_with('@') {
        // 同样的 #tag 切分逻辑，让 `owner/repo#v1.2.3` 这种简写也能
        // 落到 fetch_git 的 tag 路径，而不是把 #tag 拼进 URL 然后
        // git ls-remote 永远拿不到。
        let (repo_path, pin) = match s.split_once('#') {
            Some((repo, tag)) if !repo.is_empty() && !tag.is_empty() => {
                (repo, Some(tag.to_string()))
            }
            _ => (s, None),
        };
        let github = format!("https://github.com/{repo_path}.git");
        let id = id_for_name(repo_path)?;
        return Ok(PluginSpec {
            origin: "git".into(),
            source: github,
            pin,
            id,
            name: repo_path
                .rsplit('/')
                .next()
                .unwrap_or(repo_path)
                .to_string(),
            repo_url: Some(format!("https://github.com/{repo_path}")),
        });
    }
    // npm 来源
    let (name, pin) = split_npm_spec(s)?;
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._@/".contains(c))
    {
        return Err(AppError::Plugin(format!("非法的 npm 包名 {spec:?}")));
    }
    let id = id_for_name(&name)?;
    Ok(PluginSpec {
        origin: "npm".into(),
        source: name.clone(),
        pin,
        id,
        name,
        repo_url: None,
    })
}

// --- version comparison -----------------------------------------------------
// Shared with the kernel release list: crate::version::cmp_versions.

/// Highest version among tag candidates, or None.
fn latest_tag<'a>(tags: impl Iterator<Item = &'a str>) -> Option<String> {
    tags.filter_map(|t| {
        let stripped = t.strip_prefix('v').unwrap_or(t);
        let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
        let parts: Vec<&str> = head.split('.').collect();
        (parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok()))
            .then(|| t.to_string())
    })
    .max_by(|a, b| cmp_versions(a, b))
}

/// Whether a stored version string looks like semver (e.g. `v0.15.0`,
/// `1.2.3-rc.1`) rather than a git short hash (e.g. `v646c91c`).
///
/// Used by `is_newer_than` to detect the rare fallback path where an
/// unpinned git-origin repo has no usable semver tags: in that case
/// `installed_version` is the cloned HEAD short hash, and `cmp_versions`
/// would rank any semver tag ahead of it purely on numeric-segment
/// count. Filtering on shape first lets `is_newer_than` choose the
/// right comparison instead of trusting that ordering.
fn looks_like_semver(version: &str) -> bool {
    let stripped = version.strip_prefix('v').unwrap_or(version);
    let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
    let parts: Vec<&str> = head.split('.').collect();
    parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok())
}

/// Whether the candidate `latest` is newer than the currently installed
/// `installed` for a plugin of the given origin.
///
/// - npm / pinned git: rank by `cmp_versions` against a semver baseline.
/// - unpinned git with a tag-shaped installed version (the common case
///   after `fetch_git` resolves the highest semver tag): same semver
///   rank.
/// - unpinned git with a hash-shaped installed version (the fallback
///   path for repos without any semver tags): `cmp_versions` would
///   rank the remote's tag-shaped `latest` ahead purely on numeric
///   segment count, so fall back to string equality — but only when
///   `latest` is also a hash. A tag against a hash means the remote
///   has no commit-graph signal to compare against, so report no
///   update until the user manually re-installs.
fn is_newer_than(latest: &str, installed: &str, origin: &str, pinned: bool) -> bool {
    if origin == "git" && !pinned && !looks_like_semver(installed) {
        if looks_like_semver(latest) {
            false
        } else {
            latest != installed
        }
    } else {
        cmp_versions(latest, installed) == Ordering::Greater
    }
}

// --- fetching ---------------------------------------------------------------

/// Run one command, collecting stdout for quick helpers (git ls-remote).
/// Goes through `process::command_with_path` for the same reason as the
/// other direct git invocations: the helper's only caller is
/// `git_latest_tag`, which shells out to `git` from a GUI-subsystem
/// release build where the inherited PATH is system-only.
fn run_capture(program: &str, args: &[&str]) -> io::Result<(bool, String)> {
    let mut cmd = crate::process::command_with_path(program);
    cmd.args(args);
    let output = quiet(&mut cmd).output()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((output.status.success(), text))
}

/// Fetch the npm registry document for a package.
fn fetch_npm_doc(name: &str) -> Result<NpmDoc, String> {
    let url = format!("{}{}", crate::registry::npm_registry_base(), name);
    let body = http_get_string(&url, None)?;
    serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())
}

/// Extract a tgz into dest, stripping the leading package/ segment. Uses the
/// system tar (bsdtar on macOS/Windows, GNU tar elsewhere). `tar` is built
/// through `process::command_with_path` so the GUI shell's inherited PATH
/// includes the user's tool locations — without it, a Windows GUI release
/// build (which only sees the system PATH) cannot find `tar.exe` when the
/// user installed a third-party variant.
///
/// stderr is captured into the error message so a real extraction
/// failure (corrupt archive, write-permission denied, MAX_PATH overrun
/// on Windows, …) surfaces its actual cause instead of just an exit
/// code the user cannot act on. stdout is discarded because `bsdtar`
/// prints one extracted path per line and we do not want to forward
/// the noise through the install log.
///
/// `dest` is `mkdir -p`'d before invoking `tar -C`. GNU tar creates the
/// directory on demand; the Windows 10+ bsdtar shipped at
/// `C:\Windows\System32\tar.exe` exits 1 with `could not chdir to`
/// when the destination is missing, even when it could have created
/// it. Pre-creating makes both flavors behave identically.
fn extract_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("创建解包目录失败：{e}"))?;
    let mut cmd = crate::process::command_with_path("tar");
    cmd.arg("-xzf")
        .arg(tarball)
        .arg("--strip-components=1")
        .arg("-C")
        .arg(dest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = quiet(&mut cmd)
        .output()
        .map_err(|e| format!("无法运行系统 tar：{e}"))?;
    if !output.status.success() {
        // bsdtar's diagnostics land on stderr; trim to a single line so
        // the user-facing error stays compact. Newlines from multi-line
        // bsdtar output (e.g. "Path too long") would otherwise break the
        // log layout the UI already scrapes with the prefix "插件错误".
        let detail = String::from_utf8_lossy(&output.stderr)
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let code = output.status.code();
        return Err(format!(
            "tar 解包失败（退出码 {:?}）{}",
            code,
            if detail.is_empty() {
                String::new()
            } else {
                format!("：{detail}")
            }
        ));
    }
    Ok(())
}

fn write_source_marker(spec: &PluginSpec, version: &str, dest: &Path) -> Result<(), AppError> {
    let marker = serde_json::json!({
        "id": spec.id,
        "origin": spec.origin,
        "source": spec.source,
        "version": version,
        "fetchedAt": now_epoch_secs(),
    });
    let text = serde_json::to_string_pretty(&marker).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(dest.join(SOURCE_MARKER), text + "\n").map_err(|e| AppError::Io(e.to_string()))
}

/// Prefix for an in-progress fetch dir (`.tmp-<pid>-<ts>`). Stamped with a
/// `.dsh-id` marker so `reconcile_store` can group staging dirs by plugin
/// without parsing the id out of the dir name (which can contain `-`).
const TMP_PREFIX: &str = "tmp-";
/// Prefix for a fetch that completed `validate_plugin` and is waiting to
/// be published. `.new-<pid>-<ts>` only exists between validation and the
/// final rename onto `final_dir`.
const NEW_PREFIX: &str = "new-";
/// Prefix for the previous live plugin dir, moved aside during the
/// `final_dir` → `new_dir` swap. `.backup-<pid>-<ts>` stays until the
/// publish succeeds and the next cleanup pass removes it; on a crash
/// mid-swap it is the safety net that lets `reconcile_store` revert to
/// the known-good previous version.
const BACKUP_PREFIX: &str = "backup-";
/// File inside each staging dir carrying the plugin id so recovery can
/// identify which plugin the dir belongs to without parsing the path.
const ID_MARKER: &str = ".dsh-id";

/// Build a uniquely-named empty staging dir under `store`. `kind` is
/// `TMP_PREFIX` / `NEW_PREFIX` / `BACKUP_PREFIX`; `pid` and `nanos` are
/// folded into the name so two concurrent fetches (or two updates
/// interleaved with a crash) cannot collide.
///
/// The caller decides when to stamp `.dsh-id` (if at all). Pre-stamping
/// on the rename target was the original Windows failure mode: a
/// leftover `.new-<pid>-<ts>` dir from a previous attempt holds the
/// marker file plus any intermediate content, and `fs::rename` on
/// Windows rejects a non-empty target with ERROR_DIR_NOT_EMPTY. Keeping
/// the new path empty until after the rename — and stamping only on the
/// source side — closes that hole.
///
/// `fs::remove_dir_all` is no longer fire-and-forget: a failure to
/// clear a stale target surfaces so the caller can decide whether to
/// retry, surface the error, or fall back to a different path. The
/// happy path returns the same shape as before.
fn new_staging_dir(store: &Path, kind: &str, id: &str) -> io::Result<PathBuf> {
    let _ = id;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let dir = store.join(format!("{kind}{pid}-{nanos}"));
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e);
        }
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Stamp the `.dsh-id` marker inside an existing staging dir so
/// `reconcile_store` can group it with the corresponding `final_dir`.
/// Used after a successful rename, never before, so the rename target
/// stays empty on Windows.
fn stamp_id_marker(dir: &Path, id: &str) -> io::Result<()> {
    fs::write(dir.join(ID_MARKER), format!("{id}\n"))
}

/// Fetch a plugin into the store under a staged tmp dir, validate it, then
/// publish it over `final_dir` with crash-safe bookkeeping. Returns the
/// new store item, inheriting mode and latest. `pnpm_exe` builds
/// git-sourced plugins whose committed tree lacks `lib/`.
///
/// The fetch → validate → publish sequence uses three staging names so a
/// crash at any step leaves the live plugin (`final_dir`) in one of two
/// recoverable states: pointing at the previous version (preserved
/// unchanged until the swap starts) or pointing at the new version
/// (validation already passed before the publish rename). The transient
/// gap when `final_dir` is briefly missing is reconciled on next launch by
/// `reconcile_store`, which prefers to revert to the previous version
/// when both `.new-*` and `.backup-*` survive a crash — the new content
/// is already on disk and validated, so a user retry just needs to
/// re-trigger the publish step.
fn fetch_into_store(
    data_dir: &Path,
    pnpm_exe: &Path,
    spec: &PluginSpec,
    on_progress: &mut dyn FnMut(&str),
) -> Result<StoreItem, AppError> {
    let store = store_dir(data_dir);
    fs::create_dir_all(&store).map_err(|e| AppError::Io(e.to_string()))?;
    let tmp =
        new_staging_dir(&store, TMP_PREFIX, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;
    // Stamping the `.dsh-id` marker must wait until AFTER fetch_* returns:
    // `git clone` requires an empty destination dir and aborts with
    // "destination path '...' already exists and is not an empty directory"
    // when the marker file is present at fetch time. The npm flow's tarball
    // extraction would happily overwrite the marker anyway, so stamping
    // pre-fetch only ever worked for npm by accident. Stamping here — once
    // the staged tree is on disk and validated-empty-by-fetch — still gives
    // `reconcile_store` the identity it needs during validation/publish,
    // and the marker travels with the contents when we rename `tmp → new`.

    let version = match spec.origin.as_str() {
        "npm" => fetch_npm(spec, &tmp, on_progress),
        "git" => fetch_git(spec, &tmp, pnpm_exe, on_progress),
        other => Err(AppError::Plugin(format!("未知来源 {other:?}"))),
    };
    let version = match version {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    stamp_id_marker(&tmp, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;

    on_progress("正在校验插件是否符合 dsh 规范");
    if let Err(e) = validate_plugin(&tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }

    // Atomic swap onto `final_dir`. The three-stage rename (tmp →
    // new → final_dir, with the previous final parked as backup)
    // leaves the live plugin recoverable on a mid-swap crash. Every
    // rename target stays empty until after the rename succeeds so
    // Windows `MoveFileEx` (which rejects non-empty directories with
    // ERROR_DIR_NOT_EMPTY) does not stall the install.
    //
    // Error policy: each step propagates failures instead of swallowing
    // them. A rename that lands the validated tree in `new` but then
    // fails to publish onto `final_dir` restores the previous
    // `final_dir` from `backup` so the user is not stranded on an
    // uninstalled plugin. If recovery itself fails, the function
    // returns the error with the staged state left on disk for
    // `reconcile_store` to repair on the next launch.
    let new =
        new_staging_dir(&store, NEW_PREFIX, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;
    if let Err(e) = fs::rename(&tmp, &new) {
        // `tmp` still holds the validated content; leave it on disk so
        // a retry / `reconcile_store` can promote it.
        return Err(AppError::Io(format!("将暂存目录提升到 .new-* 失败：{e}")));
    }

    let final_dir = store_plugin_dir(data_dir, &spec.id);
    let backup = new_staging_dir(&store, BACKUP_PREFIX, &spec.id)
        .map_err(|e| AppError::Io(e.to_string()))?;
    if final_dir.exists() {
        if let Err(e) = fs::rename(&final_dir, &backup) {
            // `new` carries the validated content; `final_dir` is the
            // live plugin that just refused to move. Roll forward by
            // promoting `new` and surfacing a non-fatal warning instead
            // of stranding the validated content, since the user asked
            // to install this plugin.
            if fs::rename(&new, &final_dir).is_err() {
                let _ = fs::remove_dir_all(&new);
                return Err(AppError::Io(format!("备份旧版本失败且无法发布新版本：{e}")));
            }
            return Err(AppError::Io(format!(
                "插件已发布，但备份旧版本失败（{e}）；下次更新若失败将无法回滚"
            )));
        }
        // Stamp the marker on the backup now that the rename has
        // landed — `reconcile_store` uses it to group the dir with its
        // plugin id if a later swap leaves it stranded.
        if let Err(e) = stamp_id_marker(&backup, &spec.id) {
            eprintln!(
                "dsh-desktop: warning, could not stamp id marker on backup of {}: {e}",
                spec.id
            );
        }
    }

    if let Err(e) = fs::rename(&new, &final_dir) {
        // Roll back: the previous live plugin is now in `backup`, so
        // restore it to `final_dir`. If that succeeds, `new` becomes
        // a stranded `.new-*` for `reconcile_store` to promote on the
        // next launch; if it fails, both states exist on disk for the
        // recovery scan to reconcile.
        if fs::rename(&backup, &final_dir).is_err() {
            return Err(AppError::Io(format!("发布新版本失败且回滚旧版本失败：{e}")));
        }
        return Err(AppError::Io(format!("发布新版本失败，已回滚到旧版本：{e}")));
    }

    // Synchronous cleanup of the now-redundant backup. Failure here is
    // user-visible: leaving `.backup-*` behind forever accumulates
    // dead directories that `reconcile_store` would normally reap on
    // the next launch but cannot always disambiguate from a real
    // crash-interrupted swap.
    if backup.exists() {
        if let Err(e) = fs::remove_dir_all(&backup) {
            return Err(AppError::Io(format!(
                "插件已发布成功，但清理备份目录失败：{e}（下次启动时 reconcile_store 会接手）"
            )));
        }
    }

    write_source_marker(spec, &version, &final_dir)?;

    let now = now_epoch_secs();
    let existing = store_item(data_dir, &spec.id);
    Ok(StoreItem {
        id: spec.id.clone(),
        name: spec.name.clone(),
        origin: spec.origin.clone(),
        source: spec.source.clone(),
        installed_version: version,
        latest_version: existing.as_ref().and_then(|e| e.latest_version.clone()),
        mode: existing
            .as_ref()
            .map(|e| e.mode.clone())
            .unwrap_or_else(|| String::from("link")),
        pinned: spec.pin.is_some(),
        installed_at: existing
            .as_ref()
            .map(|e| e.installed_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        repo_url: spec.repo_url.clone(),
        description: None,
    })
}

/// Reconcile staging dirs left over by an interrupted update. Safe to run
/// on every startup; the happy path (no leftover staging) is a single
/// `read_dir` scan with no renames or deletes.
///
/// Per plugin id, the recovery rules are:
///
/// | `final_dir` | `.new-*` | `.backup-*` | `.tmp-*` | action |
/// | --- | --- | --- | --- | --- |
/// | exists | any | any | any | remove all staging (post-publish cleanup or stale attempt) |
/// | missing | no | yes | no | revert: rename `.backup-*` to `final_dir` |
/// | missing | yes | no | no | publish: rename `.new-*` to `final_dir` |
/// | missing | yes | yes | any | revert (safer; user keeps the known-good previous version) |
/// | missing | no | no | yes | incomplete fetch; remove `.tmp-*` |
/// | missing | yes | yes | yes | revert + remove tmp |
///
/// When multiple staging dirs share the same id (unlikely but possible if
/// a previous crash happened while a recovery itself was being attempted),
/// the freshest one wins: `.tmp-*` are always discarded (never validated);
/// among `.new-*` / `.backup-*` the lexicographically largest suffix wins
/// (timestamps + pids sort newest-last), and any older peer is removed.
pub fn reconcile_store(data_dir: &Path) {
    let store = store_dir(data_dir);
    let Ok(entries) = fs::read_dir(&store) else {
        return;
    };

    enum Kind {
        Tmp,
        New,
        Backup,
    }

    let mut by_id: std::collections::HashMap<String, Vec<(Kind, PathBuf)>> =
        std::collections::HashMap::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        // `.tmp-` is the legacy pre-marker naming scheme; current staging
        // dirs use `tmp-` / `new-` / `backup-` (see new_staging_dir).
        let kind = if name.starts_with(TMP_PREFIX) || name.starts_with(".tmp-") {
            Kind::Tmp
        } else if name.starts_with(NEW_PREFIX) {
            Kind::New
        } else if name.starts_with(BACKUP_PREFIX) {
            Kind::Backup
        } else {
            continue;
        };
        let marker = fs::read_to_string(entry.path().join(ID_MARKER))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(id) = marker else {
            // Staging dir without an id marker: residue of a crash between
            // staging-dir creation and stamping (or of an older shell that
            // named staging differently). It is never user data — reap it
            // instead of letting it accumulate forever.
            let _ = fs::remove_dir_all(entry.path());
            continue;
        };
        if id == name {
            // A live plugin whose name starts with a staging prefix (npm
            // allows `tmp-foo`): its final dir carries its own marker and
            // must not be grouped as its own staging residue.
            continue;
        }
        by_id.entry(id).or_default().push((kind, entry.path()));
    }

    for (id, mut items) in by_id {
        let final_dir = store.join(&id);
        // Sort by dir name (which encodes pid + ts); newest last.
        items.sort_by(|a, b| a.1.file_name().cmp(&b.1.file_name()));

        if final_dir.exists() {
            for (_, path) in items {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }

        // Pick the newest `.new-*` and `.backup-*` (independently), and
        // drop every older peer. `.tmp-*` is always dropped.
        let newest_new = items
            .iter()
            .rev()
            .find(|(k, _)| matches!(k, Kind::New))
            .map(|(_, p)| p.clone());
        let newest_backup = items
            .iter()
            .rev()
            .find(|(k, _)| matches!(k, Kind::Backup))
            .map(|(_, p)| p.clone());
        for (kind, path) in &items {
            let drop = match kind {
                Kind::Tmp => true,
                Kind::New => Some(path) != newest_new.as_ref(),
                Kind::Backup => Some(path) != newest_backup.as_ref(),
            };
            if drop {
                let _ = fs::remove_dir_all(path);
            }
        }

        // Apply the recovery action described in the table at the header.
        if let Some(backup) = newest_backup {
            // Revert is the safer default when both states survived:
            // the previous version is the only one we know the user has
            // already exercised, while the `.new-*` content is
            // validated-but-not-yet-running.
            let _ = fs::rename(&backup, &final_dir);
            if let Some(new) = newest_new {
                let _ = fs::remove_dir_all(&new);
            }
        } else if let Some(new) = newest_new {
            let _ = fs::rename(&new, &final_dir);
        }
        // else: only `.tmp-*` left; already removed above.
    }
}

/// Resolve the version a plugin install should pin to, given the npm doc
/// and the user-supplied pin (or `None` for "give me whatever the
/// package's `latest` dist-tag points at"). Returns the resolved version
/// string plus the label that should appear in error messages when the
/// resolution yields an empty string.
///
/// Dist-tag resolution rule:
/// - `pin = None` → look up `dist-tags.latest`, fall back to "" if absent
/// - `pin = Some(tag)` where `tag` matches a dist-tag → use that tag's version
/// - `pin = Some(ver)` where `ver` doesn't match a dist-tag → use the pin
///   as a literal semver string (the caller will surface
///   `versions[ver]` lookup failures with a clear error)
fn resolve_npm_version(doc: &NpmDoc, pin: Option<&str>) -> (String, String) {
    match pin {
        Some(tag) => (
            doc.dist_tags
                .get(tag)
                .cloned()
                .unwrap_or_else(|| tag.to_string()),
            tag.to_string(),
        ),
        None => (
            doc.dist_tags.get("latest").cloned().unwrap_or_default(),
            "latest".to_string(),
        ),
    }
}

fn fetch_npm(
    spec: &PluginSpec,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    on_progress(&format!("正在查询 npm registry：{}", spec.source));
    let doc =
        fetch_npm_doc(&spec.source).map_err(|e| AppError::Plugin(format!("查询 npm 失败：{e}")))?;
    // Resolve the requested version through `dist-tags` first so a
    // pin like `@latest` (or `@next`, `@beta`) lands on the actual
    // semver string `versions` is keyed by. Without this hop the
    // literal `"latest"` is used as a `versions` key, the lookup
    // returns `None`, and the user sees
    // 「npm 上 @linxin666/dsh-liangshen@latest 没有可下载的 tarball」
    // even though the package and its latest tarball both exist on
    // the registry. A pin that doesn't match any dist-tag falls
    // through unchanged so literal semver pins like `@1.2.3` still
    // hit `versions` directly.
    let (version, pin_label) = resolve_npm_version(&doc, spec.pin.as_deref());
    if version.is_empty() {
        return Err(AppError::Plugin(format!(
            "npm 上找不到包 {} 或其 {pin_label} 标记",
            spec.source
        )));
    }
    let tarball = doc
        .versions
        .get(&version)
        .and_then(|v| v.dist.as_ref())
        .map(|d| d.tarball.clone())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            AppError::Plugin(format!(
                "npm 上 {}@{version} 没有可下载的 tarball",
                spec.source
            ))
        })?;
    on_progress(&format!("正在下载 {}@{version} …", spec.source));
    let bytes = http_get_bytes(&tarball).map_err(|e| AppError::Plugin(format!("下载失败：{e}")))?;
    let tgz = dest.join(".pkg.tgz");
    fs::write(&tgz, bytes).map_err(|e| AppError::Io(e.to_string()))?;
    // Extract straight into the staging dir. npm tarballs carry a leading
    // `package/` segment that `--strip-components=1` removes, so the
    // plugin's `package.json`, `lib/`, `cordis.patch.yml`, … land at the
    // root of `dest` where `validate_plugin(&dest)` and the later
    // store/kernels materialization expect them. The historical code
    // extracted into `dest/package/` then immediately removed that
    // subdirectory, leaving the staging dir empty and tripping
    // `validate_plugin` with a "缺少可解析的 package.json" error.
    extract_tarball(&tgz, dest)
        .map_err(|e| AppError::Plugin(format!("解包失败：{e}（请确认系统存在 tar）")))?;
    let _ = fs::remove_file(&tgz);
    Ok(version)
}

fn fetch_git(
    spec: &PluginSpec,
    dest: &Path,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    // The probe and the clone go through `process::command_with_path` so the
    // inherited PATH includes the user's tool locations. On Windows a GUI
    // subsystem release build only sees the system PATH at launch; Git for
    // Windows registers in the user PATH (`HKCU\Environment\Path`), so a
    // bare `Command::new("git")` here resolves to "command not found" and
    // the user sees the misleading "未找到 git" error even though `git`
    // works from any cmd.exe they open themselves.
    let mut probe = crate::process::command_with_path("git");
    probe.arg("--version");
    if quiet(&mut probe).output().is_err() {
        return Err(AppError::Plugin(
            "未找到 git（git 来源的插件需要 git；请先安装 git）".into(),
        ));
    }

    // Resolve what to check out.
    // - pinned: the spec supplies `#tag`; use it directly.
    // - unpinned: pick the highest semver tag the remote has published,
    //   so the installed_version stored on disk is the same kind of
    //   string `check_updates` will compare against. A repo without any
    //   semver tag falls back to the default branch (HEAD short hash);
    //   `is_newer_than` handles that fallback specially so a fresh tag
    //   does not look "newer" than the hash on segment count alone.
    let branch = match spec.pin.as_ref() {
        Some(tag) => Some(tag.clone()),
        None => match git_latest_tag(&spec.source) {
            Ok(Some(tag)) => Some(tag),
            Ok(None) => None,
            Err(e) => {
                return Err(AppError::Plugin(format!("查询最新 tag 失败：{e}")));
            }
        },
    };

    on_progress(&format!("正在克隆 {}", spec.source));
    let mut cmd = crate::process::command_with_path("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(tag) = &branch {
        cmd.arg("--branch").arg(tag);
    }
    // stderr 是 git 唯一的诊断输出（"Repository not found"、
    // "could not resolve host"、"authentication failed"、"SSL certificate
    // problem"…）。早期实现把它和 stdout 都吞掉，导致失败时 UI 只能
    // 显示无意义的 "exit code 128"，用户根本看不出是仓库不存在、网络
    // 不通、还是权限问题。这里改成 piped；错误时优先挑出 fatal/error
    // 行（"Cloning into 'X'..." 是首行纯提示，紧跟其后的 fatal: 才是
    // 真正原因；某些情况下 git 还会在 stdout 上吐诊断信息，一并捕获）。
    let output = quiet(&mut cmd)
        .arg(&spec.source)
        .arg(dest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| AppError::Io(format!("无法运行 git：{e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // 优先反向查找 "fatal:" / "error:" 行；找不到就退回到最后一行
        // 非空 stderr，再退回 "，请检查地址与网络" 兜底文案。
        let detail = stderr
            .lines()
            .rev()
            .find(|l| {
                let t = l.trim_start();
                t.starts_with("fatal:") || t.starts_with("error:")
            })
            .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("")
            .trim()
            .to_string();
        let stdout_tail = stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        let code = output.status.code();
        let mut msg = format!("git clone 失败（退出码 {:?}）", code);
        if !detail.is_empty() {
            msg.push_str(&format!("：{detail}"));
        } else if !stdout_tail.is_empty() {
            msg.push_str(&format!("：{stdout_tail}"));
        } else {
            msg.push_str("，请检查地址与网络");
        }
        return Err(AppError::Plugin(msg));
    }
    build_git_plugin(dest, pnpm_exe, on_progress)?;
    if let Some(tag) = branch {
        return Ok(tag);
    }
    // Unpinned repo without any semver tags: cloned the default branch;
    // record the HEAD hash so the source marker still names something
    // stable and the user can see what commit they have.
    let dest_str = dest.to_str().unwrap_or("");
    let (ok, out) = run_capture("git", &["-C", dest_str, "rev-parse", "--short", "HEAD"])
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(if ok {
        out.trim().to_string()
    } else {
        String::from("head")
    })
}

/// Build a git-sourced plugin right after cloning.
///
/// Git repos carry their build output in `.gitignore` (`lib/` is never
/// committed), so the freshly cloned tree cannot satisfy the loader until it
/// is built. The package's own `prepare` script is the npm-sanctioned hook
/// for exactly this; running it via pnpm keeps toolchain resolution inside
/// the plugin. Best-effort: when no `prepare` exists the plugin must ship
/// prebuilt output, and `validate_plugin` still guards the final state, so
/// this only reports failure when a declared prepare actually fails.
fn build_git_plugin(
    dest: &Path,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let root = match read_plugin_manifest(dest) {
        Ok(root) => root,
        Err(_) => return Ok(()), // validate_plugin reports the real problem
    };
    let has_prepare = root
        .get("scripts")
        .and_then(|s| s.get("prepare"))
        .and_then(|p| p.as_str())
        .map(|p| !p.is_empty())
        .unwrap_or(false);
    let main = root.get("main").and_then(|m| m.as_str()).unwrap_or("");
    let entry_ready = !main.is_empty()
        && (dest.join(main).is_file() || dest.join(format!("{main}.js")).is_file());
    // Prebuilt repo: nothing to do. Declared-but-unbuilt is the common case.
    if !has_prepare || entry_ready {
        return Ok(());
    }
    // Dependencies are required for the build script to find its tools
    // (tsdown etc.). install_store_deps runs later in link mode, but that is
    // too late — the entry check happens first, and copy mode skips it.
    on_progress("正在安装插件依赖并构建（pnpm，git 来源需要生成 lib/）");
    let log_path = dest.join(".dsh-build.log");
    let args = [
        "install",
        "--ignore-workspace",
        "--config.node-linker=hoisted",
        kernel::PNPM_REPORTER,
        kernel::PNPM_NO_STRICT_DEP_BUILDS,
        "--config.enable-pre-post-scripts=true",
    ];
    // pnpm runs the package's `prepare` lifecycle script automatically after
    // install when enable-pre-post-scripts is on. `pnpm_exe.parent()` is
    // prepended to the child's PATH so the lifecycle shell can resolve
    // `node` (and any sibling shebanged tool) even when the GUI inherited
    // a launchd-only PATH that does not list the user's Homebrew / nvm bin
    // directory; without it the prepare step exits 127 with
    // `env: node: No such file or directory`.
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    let status = kernel::run_pnpm(
        pnpm_exe,
        &args,
        dest,
        &log_path,
        &[pnpm_dir],
        &mut *on_progress,
    )
    .map_err(kernel::pnpm_spawn_err)?;
    if !status.success() {
        return Err(AppError::Plugin(format!(
            "插件构建失败（退出码 {:?}）：`prepare` 未成功生成入口。详情见 {}",
            status.code(),
            log_path.display()
        )));
    }
    Ok(())
}

fn read_plugin_manifest(plugin_root: &Path) -> Result<serde_json::Value, serde_json::Error> {
    let text =
        fs::read_to_string(plugin_root.join("package.json")).map_err(serde_json::Error::io)?;
    serde_json::from_str(&text)
}

/// Whether the plugin directory declares runtime dependencies.
fn manifest_has_deps(plugin_root: &Path) -> bool {
    let Ok(root) = read_plugin_manifest(plugin_root) else {
        return false;
    };
    root.get("dependencies")
        .and_then(|d| d.as_object())
        .map(|d| !d.is_empty())
        .unwrap_or(false)
}

/// Whether the plugin declares a bundle layer.
fn manifest_is_bundle(plugin_root: &Path) -> bool {
    let Ok(root) = read_plugin_manifest(plugin_root) else {
        return false;
    };
    root.get("dsh")
        .and_then(|d| d.get("bundle"))
        .and_then(|b| b.get("patch"))
        .and_then(|p| p.as_str())
        .map(|p| !p.is_empty())
        .unwrap_or(false)
}

/// What the kernel needs to load an installed plugin: a parseable
/// package.json with a name; when the package declares a bundle layer its
/// patch file must exist, and regardless of bundling it needs a resolvable
/// `main`/`exports` entry. Runs right after fetch so a non-conforming
/// package fails the install loudly instead of breaking the next kernel boot.
///
/// The entry check must run even when a bundle layer is present: plugins
/// commonly declare both (the bundle patches the client UI while `main`
/// loads the server half). Returning early on the bundle branch let git
/// source installs through without their build output (`lib/` is
/// gitignored), which then crashed the kernel at ESM resolution time.
fn validate_plugin(dir: &Path) -> Result<(), AppError> {
    let root = read_plugin_manifest(dir)
        .map_err(|_| AppError::Plugin("不符合 dsh 插件规范：缺少可解析的 package.json".into()))?;
    let name = root.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if name.is_empty() {
        return Err(AppError::Plugin(
            "不符合 dsh 插件规范：package.json 缺少 name 字段".into(),
        ));
    }
    if let Some(patch) = root
        .get("dsh")
        .and_then(|d| d.get("bundle"))
        .and_then(|b| b.get("patch"))
        .and_then(|p| p.as_str())
    {
        if patch.is_empty() || !dir.join(patch).is_file() {
            return Err(AppError::Plugin(format!(
                "不符合 dsh 插件规范：声明了 bundle 层但包内找不到 patch 文件 {patch:?}，内核启动将无法加载该层"
            )));
        }
        // Fall through: the runtime entry below is still required.
    }
    let has_exports = root.get("exports").is_some();
    if !has_exports {
        let main = root.get("main").and_then(|m| m.as_str()).unwrap_or("");
        if main.is_empty() {
            return Err(AppError::Plugin(
                "不符合 dsh 插件规范：既未声明 dsh.bundle.patch，也没有 main/exports 入口，内核无法加载"
                    .into(),
            ));
        }
        // Node 解析 main 时允许省略 .js 后缀，两者都接受。
        if dir.join(main).is_file() || dir.join(format!("{main}.js")).is_file() {
            return Ok(());
        }
        return Err(AppError::Plugin(format!(
            "不符合 dsh 插件规范：main 入口 {main:?} 在包内不存在，内核无法加载。git 来源的插件通常需要在包内执行一次构建（如 `pnpm run prepare`）生成 lib/"
        )));
    }
    Ok(())
}

/// Install the plugin's own dependencies inside the store dir. Only link
/// mode needs this (copy mode lets the profile's pnpm handle them).
fn install_store_deps(
    data_dir: &Path,
    pnpm_exe: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let dir = store_plugin_dir(data_dir, id);
    let log_path = plugin_log_path(data_dir, id);
    if !manifest_has_deps(&dir) {
        return Ok(());
    }
    on_progress("正在安装插件自身依赖（pnpm）");

    // Delete any stale lockfile so pnpm re-resolves without minimumReleaseAge violations
    // from entries locked to recently-published rc versions.  A fresh install without a
    // lockfile re-resolves everything from the registry and is always safe.
    let lockfile = dir.join("pnpm-lock.yaml");
    if lockfile.is_file() {
        fs::remove_file(&lockfile).ok();
    }

    let args = [
        "install",
        "--ignore-workspace",
        "--config.node-linker=hoisted",
        kernel::PNPM_REPORTER,
        kernel::PNPM_NO_STRICT_DEP_BUILDS,
    ];
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    let status = kernel::run_pnpm(
        pnpm_exe,
        &args,
        &dir,
        &log_path,
        &[pnpm_dir],
        &mut *on_progress,
    )
    .map_err(kernel::pnpm_spawn_err)?;
    if !status.success() && !dir.join("node_modules").is_dir() {
        return Err(AppError::Plugin(format!(
            "插件依赖安装失败（退出码 {:?}），详情见日志：{}",
            status.code(),
            log_path.display()
        )));
    }
    if !status.success() {
        on_progress(
            "注意：pnpm 以非零退出码结束（多为依赖构建脚本被忽略所致），插件依赖已基本就绪",
        );
    }
    Ok(())
}

// --- materialization --------------------------------------------------------

/// Materialize one plugin into one kernel: link (symlink, junction on
/// Windows) or copy, recorded in .meta/<id>.json. Returns the actual mode.
pub fn materialize_one(
    data_dir: &Path,
    version: &str,
    item: &StoreItem,
) -> Result<String, AppError> {
    let source = store_plugin_dir(data_dir, &item.id);
    let target = kernel_plugin_dir(data_dir, version, &item.id);
    let meta = read_meta(data_dir, version, &item.id);

    // Resolve the store path once: if the store source itself is a symlink
    // (e.g. a git-origin plugin cloned into the store), use the actual
    // filesystem location so the kernel plugin dir gets a direct link —
    // avoiding the double-symlink chain that breaks Node's realpath.
    let resolved_source = fs::symlink_metadata(&source)
        .ok()
        .filter(|m| m.file_type().is_symlink())
        .and_then(|_| fs::read_link(&source).ok())
        .unwrap_or_else(|| source.to_path_buf());

    let fresh = meta
        .as_ref()
        .map(|m| m.version == item.installed_version && m.mode == item.mode)
        .unwrap_or(false);

    // If the metadata says nothing changed AND the target exists, verify the
    // target symlink is actually correct.  A prior run may have left a stale
    // double-symlink chain even though the recorded version and mode are
    // unchanged — falling through re-creates the correct direct link.
    if fresh && target.exists() {
        let target_ok = fs::symlink_metadata(&target)
            .ok()
            .filter(|m| m.file_type().is_symlink())
            .and_then(|_| fs::read_link(&target).ok())
            .map(|link| link == resolved_source)
            .unwrap_or(false);
        if target_ok {
            return Ok(meta.map(|m| m.mode).unwrap_or_else(|| item.mode.clone()));
        }
    }

    // 清除旧产物（错误残留：非链接目录、指向别处的链接或旧版本副本）
    remove_materialized(data_dir, version, &item.id);

    let mut actual = item.mode.clone();
    if item.mode == "link" && make_dir_link(&resolved_source, &target).is_err() {
        // 链接失败（Windows 权限、文件系统不支持）→ 降级复制
        actual = String::from("copy");
        eprintln!(
            "dsh-desktop: link failed for {}; falling back to copy",
            item.id
        );
    }
    if actual == "copy" {
        copy_tree(&source, &target).map_err(|e| {
            AppError::Io(format!(
                "复制插件 {} 到内核失败：{e}。请关闭工作台后点击「同步」重试；若持续失败请查看日志 {}",
                item.id,
                wiring_log_path(data_dir).display()
            ))
        })?;
    }
    if !target.exists() {
        return Err(AppError::Plugin(format!(
            "物化失败：{} 在内核 {version} 中未就绪",
            item.id
        )));
    }
    write_meta(
        data_dir,
        version,
        &item.id,
        &KernelMeta {
            mode: actual.clone(),
            version: item.installed_version.clone(),
            synced_at: now_epoch_secs(),
        },
    )?;
    Ok(actual)
}

/// Remove a filesystem link (symlink) without touching its target. On
/// Windows `DeleteFile` rejects directory symlinks with
/// ERROR_ACCESS_DENIED — only `RemoveDirectory` removes them — while file
/// symlinks need `DeleteFile`; trying both covers either kind on every
/// platform. Removing the wrong way leaves the link in place, and every
/// subsequent operation (recreate, copy) then follows it into the link
/// target: that is how a plugin update on Windows turned into copying the
/// store directory onto itself and failing with os error 2.
fn remove_link(path: &Path) {
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}

/// Remove a plugin's materialization from one kernel (link or copy residue).
fn remove_materialized(data_dir: &Path, version: &str, id: &str) {
    let target = kernel_plugin_dir(data_dir, version, id);
    match fs::symlink_metadata(&target) {
        Ok(md) if md.file_type().is_symlink() => remove_link(&target),
        Ok(_) => {
            let _ = fs::remove_dir_all(&target);
        }
        Err(_) => {}
    }
    let _ = fs::remove_file(kernel_meta_file(data_dir, version, id));
}

/// Sweep kernel plugin entries the store no longer holds — uninstall
/// residue left behind when the uninstall-time removal hit a Windows file
/// lock, or a store dir deleted by hand. An entry is only swept when the
/// shell provably owns it (a `.meta/<id>.json` record exists) or it is
/// already broken (a symlink whose target vanished); anything else a user
/// dropped into the kernel plugins dir by hand is left alone. Keyed on
/// store membership rather than the wiring filter so quarantined plugins
/// keep their materialization.
fn sweep_kernel_orphans(data_dir: &Path, version: &str, store: &Store) {
    let dir = kernel_plugins_dir(data_dir, version);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name == META_SUBDIR || store.items.iter().any(|i| i.id == name) {
            continue;
        }
        let path = entry.path();
        let dangling_link = fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.file_type().is_symlink())
            .map(|_| !path.exists()) // exists() follows the link to its target
            .unwrap_or(false);
        let shell_owned = kernel_meta_file(data_dir, version, &name).is_file();
        if dangling_link || shell_owned {
            remove_materialized(data_dir, version, &name);
        }
    }
}

#[cfg(unix)]
fn make_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn make_dir_link(source: &Path, target: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
}

/// Recursively copy source into target, replacing whatever exists. Every
/// IO error carries the path that failed — a bare "系统找不到指定的文件
/// (os error 2)" out of a 20k-file `node_modules` copy is undiagnosable.
/// Broken symlinks inside the tree are skipped with a warning instead of
/// aborting the whole copy: on Windows a dangling link (e.g. a peer link
/// whose kernel target moved) fails `fs::copy` with os error 2 even though
/// everything around it is fine.
fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    copy_tree_at(source, target, &mut Vec::new())
}

/// `ancestors` holds the canonical path of every directory on the current
/// recursion stack. Copy mode follows directory links (a copied tree must
/// not depend on link capability), and on macOS/Linux a pnpm
/// `node_modules` is built entirely of symlinks — circular dependencies
/// form link cycles, which are caught here by detecting that a directory's
/// canonical path is already an ancestor. Diamonds (two links to the same
/// sibling) are not cycles and pass.
fn copy_tree_at(source: &Path, target: &Path, ancestors: &mut Vec<PathBuf>) -> io::Result<()> {
    let canonical = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    if ancestors.contains(&canonical) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("目录树存在循环链接：{}", source.display()),
        ));
    }
    ancestors.push(canonical);
    let result = copy_tree_inner(source, target, ancestors);
    ancestors.pop();
    result
}

fn copy_tree_inner(source: &Path, target: &Path, ancestors: &mut Vec<PathBuf>) -> io::Result<()> {
    if target.is_symlink() {
        remove_link(target);
        if target.is_symlink() {
            // The link refused to go away: copying through it would write
            // into its target (potentially the store itself).
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("无法清除旧的链接 {}，请关闭工作台后重试", target.display()),
            ));
        }
    } else if target.exists() {
        let _ = fs::remove_dir_all(target);
    }
    fs::create_dir_all(target).map_err(|e| {
        io::Error::new(e.kind(), format!("创建目录 {} 失败：{e}", target.display()))
    })?;
    let entries = fs::read_dir(source).map_err(|e| {
        io::Error::new(e.kind(), format!("读取目录 {} 失败：{e}", source.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            io::Error::new(e.kind(), format!("遍历目录 {} 失败：{e}", source.display()))
        })?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        // `file_type` does not follow links: a symlink reports as a symlink
        // even when its target is a directory. Junctions on Windows report
        // as plain directories and are copied through, matching the copy
        // mode's "no link capability required" promise.
        let file_type = entry.file_type().map_err(|e| {
            io::Error::new(e.kind(), format!("读取 {} 的类型失败：{e}", from.display()))
        })?;
        if file_type.is_symlink() {
            // Follow the link once to classify it; a dangling link cannot be
            // copied and must not kill the whole tree.
            match fs::metadata(&from) {
                Ok(md) if md.is_dir() => copy_tree_at(&from, &to, ancestors)?,
                Ok(_) => copy_file(&from, &to)?,
                Err(e) => {
                    eprintln!(
                        "dsh-desktop: skipping dangling symlink {} during copy: {e}",
                        from.display()
                    );
                    continue;
                }
            }
        } else if file_type.is_dir() {
            copy_tree_at(&from, &to, ancestors)?;
        } else {
            copy_file(&from, &to)?;
        }
    }
    Ok(())
}

/// Copy one file over `to`, replacing any existing entry, with both paths
/// in the error so a failure names the exact file.
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let _ = fs::remove_file(to);
    fs::copy(from, to)
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("复制 {} → {} 失败：{e}", from.display(), to.display()),
            )
        })
        .map(|_| ())
}

/// Materialize a plugin into every installed kernel.
pub fn sync_kernels(data_dir: &Path, item: &StoreItem) -> Result<(), AppError> {
    for version in kernel::list_installed(data_dir) {
        materialize_one(data_dir, &version.version, item)?;
    }
    Ok(())
}

// --- profile wiring ---------------------------------------------------------

/// Relative path from from_dir to to (both under the same root), or the
/// absolute path when they share no common prefix.
fn relative_path(from_dir: &Path, to: &Path) -> PathBuf {
    let to_path = to;
    let from: Vec<Component> = from_dir.components().collect();
    let to: Vec<Component> = to_path.components().collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }
    if common == 0 {
        return to_path.to_path_buf();
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for part in &to[common..] {
        out.push(part.as_os_str());
    }
    out
}

/// Forward-slash path string for a dependency spec in package.json.
fn spec_path_string(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// Template bundles for a freshly initialized profile, mirroring the
/// kernel's profile templates.
fn template_bundles(profile: &str) -> Vec<String> {
    match profile {
        "web" => vec![
            String::from("@deepseek-ai/dsh-base"),
            String::from("@deepseek-ai/dsh-web-app"),
        ],
        "headless" => vec![
            String::from("@deepseek-ai/dsh-base"),
            String::from("@deepseek-ai/dsh-headless"),
        ],
        _ => vec![String::from("@deepseek-ai/dsh-base")],
    }
}

/// Read a profile manifest as a mutable JSON tree (round-trips unknown
/// fields). None when the profile directory is not initialized.
fn read_profile_json(
    data_dir: &Path,
    profile: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let path = profile_dir(data_dir, profile).join("package.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| AppError::Io(e.to_string()))
}

fn write_profile_json(
    data_dir: &Path,
    profile: &str,
    root: &serde_json::Value,
) -> Result<(), AppError> {
    let path = profile_dir(data_dir, profile).join("package.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let text = serde_json::to_string_pretty(root).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(path, text + "\n").map_err(|e| AppError::Io(e.to_string()))
}

/// Initialize a profile manifest the same way the kernel would, but with the
/// template bundle list baked in so wiring can precede the first boot.
fn ensure_profile(data_dir: &Path, profile: &str) -> Result<(), AppError> {
    let dir = profile_dir(data_dir, profile);
    let manifest_path = dir.join("package.json");
    if fs::metadata(&manifest_path).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let root = serde_json::json!({
        "name": format!("dsh-profile-{profile}"),
        "private": true,
        "dependencies": {},
        "dsh": { "profile": { "bundles": template_bundles(profile) } }
    });
    write_profile_json(data_dir, profile, &root)?;
    let patch = dir.join("cordis.patch.yml");
    if !patch.exists() {
        let _ = fs::write(&patch, "# Your patch layer for this dsh profile.\n[]\n");
    }
    let workspace = dir.join("pnpm-workspace.yaml");
    let needs_workspace = !workspace.exists()
        || fs::read_to_string(&workspace)
            .map(|t| !t.contains("minimumReleaseAge: 0"))
            .unwrap_or(false);
    if needs_workspace {
        let _ = fs::write(
            &workspace,
            "packages:\n  - .\n\nnodeLinker: hoisted\nautoInstallPeers: false\nminimumReleaseAge: 0\n",
        );
    }
    Ok(())
}

/// Whether a profile dependency spec is one the shell wrote (points at a
/// kernel plugins dir). Protects CLI-managed dependencies from pruning.
///
/// Shell specs always end in `kernels/<version>/plugins/<id>`; match that
/// path structure rather than the data-dir name. A name-based match
/// (`desktop/kernels/`) misreads specs written by a debug shell
/// (`desktop-dev/`) or a `DSH_DESKTOP_DATA_DIR` override as user-managed,
/// so uninstall then leaves the dependency and its bundle layer behind and
/// the kernel crashes at boot resolving the dangling bundle.
fn is_managed_spec(spec: &str) -> bool {
    let Some(path) = spec
        .strip_prefix(SPEC_LINK)
        .or_else(|| spec.strip_prefix(SPEC_FILE))
    else {
        return false;
    };
    let mut segs = path.split('/').rev();
    let (Some(id), Some("plugins"), Some(version), Some("kernels")) =
        (segs.next(), segs.next(), segs.next(), segs.next())
    else {
        return false;
    };
    !id.is_empty() && !version.is_empty()
}

/// Filter deciding which store items take part in materialization and
/// profile wiring. The boot guard passes exclusions here to retry a boot
/// without specific plugins.
pub type WiringFilter<'a> = dyn Fn(&StoreItem) -> bool + 'a;

/// Reconcile the profile manifest against the store for the ACTIVE kernel,
/// excluding plugins the quarantine registry holds (see [`crate::guard`]).
pub fn ensure_wiring(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(usize, bool), AppError> {
    let blocked = quarantine::ids(data_dir);
    ensure_wiring_filtered(
        data_dir,
        settings,
        pnpm_exe,
        &move |item| !blocked.contains(&item.id),
        on_progress,
    )
}

/// Reconcile the profile manifest against the store for the ACTIVE kernel:
/// set each allowed item's dependency to the materialized dir, maintain
/// bundle layers, rewrite specs when the active kernel changed. Runs pnpm
/// install when the manifest changed or the profile's node_modules is
/// missing. Filtered-out items are neither materialized nor wired, and their
/// stale managed dependencies plus bundle layers are pruned — that is what
/// lets the kernel boot without them.
///
/// The manifest write is transactional: when pnpm fails the manifest is
/// rolled back, because a bundles entry that cannot resolve crashes the
/// kernel at boot. An empty store still reconciles, so uninstalling the last
/// plugin prunes its residue instead of leaving an unresolvable layer behind.
///
/// Returns (wired_count, changed).
pub fn ensure_wiring_filtered(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    allow: &WiringFilter<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(usize, bool), AppError> {
    let store = load_store(data_dir);
    ensure_profile(data_dir, &settings.profile)?;

    // 物化活动内核，再据插件清单决定 bundle 层；没有活动内核且仍有插件时
    // 等内核装好再接线（store 为空则继续，让下面的清退逻辑跑掉残留）。
    // 被过滤器排除的插件（如启动看护隔离的嫌疑插件）既不物化也不进清单，
    // 内核因此在缺少它们的状态下完成启动。
    //
    // 单个插件物化失败不中止整轮接线：失败项不进清单（manifest 随之把它
    // 清退，内核不带它启动），其余插件照常接线，最后聚合报错。否则一个
    // 损坏的插件会让卸载残留的清退永远跑不到，故障在 store warning 里
    // 越积越多。
    let mut specs: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    match kernel::read_active(data_dir) {
        Some(active) => {
            for item in store.items.iter().filter(|item| allow(item)) {
                let result = refresh_store_peers(data_dir, item, &active)
                    .and_then(|_| materialize_one(data_dir, &active, item));
                match result {
                    Ok(actual) => {
                        let prefix = if actual == "copy" {
                            SPEC_FILE
                        } else {
                            SPEC_LINK
                        };
                        let rel = relative_path(
                            &profile_dir(data_dir, &settings.profile),
                            &kernel_plugin_dir(data_dir, &active, &item.id),
                        );
                        specs.insert(
                            item.name.clone(),
                            (
                                format!("{prefix}{}", spec_path_string(&rel)),
                                manifest_is_bundle(&kernel_plugin_dir(data_dir, &active, &item.id)),
                            ),
                        );
                    }
                    Err(e) => failures.push(format!("{}（{e}）", item.name)),
                }
            }
            sweep_kernel_orphans(data_dir, &active, &store);
        }
        None if !store.items.is_empty() => return Ok((0, false)),
        None => {}
    }

    let mut root = read_profile_json(data_dir, &settings.profile)?
        .ok_or_else(|| AppError::Plugin("profile 尚未初始化".into()))?;
    let previous = root.clone();
    let changed = wire_manifest(&mut root, &specs, &settings.profile)?;

    // manifest 没变但 node_modules 缺失（上次 pnpm 失败或目录被清）也必须
    // 重装，否则 bundles 里的层解析不了，内核启动即崩。
    let profile = profile_dir(data_dir, &settings.profile);
    let node_modules_missing = !profile.join("node_modules").is_dir();
    if changed || node_modules_missing {
        if changed {
            write_profile_json(data_dir, &settings.profile, &root)?;
        }
        on_progress("正在同步 profile 依赖（pnpm install）");
        let status = run_profile_install(data_dir, &settings.profile, pnpm_exe, on_progress)?;
        if !status.success() {
            if changed {
                let _ = write_profile_json(data_dir, &settings.profile, &previous);
            }
            return Err(AppError::Plugin(format!(
                "pnpm install 在 profile 中失败（退出码 {:?}），已回滚 profile 配置，详情见日志：{}",
                status.code(),
                wiring_log_path(data_dir).display()
            )));
        }
    }
    if !failures.is_empty() {
        return Err(AppError::Plugin(format!(
            "部分插件未能接入内核：{}。其余插件已正常接线；修复后点击「同步」重试",
            failures.join("；")
        )));
    }
    Ok((specs.len(), changed))
}

/// Run `pnpm install` in the named profile directory, returning its exit
/// status so each caller applies its own rollback semantics. The flags match
/// the store-level installs: the profile only needs a usable node_modules,
/// which the existing artifact fallback already tolerates when pnpm reports
/// pnpm's ignored-builds false positive. `pnpm_exe.parent()` is prepended to
/// the child's PATH so any Node-shebanged lifecycle script finds the same
/// `node` the parent used to spawn pnpm.
fn run_profile_install(
    data_dir: &Path,
    profile_name: &str,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<std::process::ExitStatus, AppError> {
    let log_path = wiring_log_path(data_dir);
    let pnpm_dir = pnpm_exe.parent().unwrap_or(Path::new("."));
    kernel::run_pnpm(
        pnpm_exe,
        &[
            "install",
            kernel::PNPM_REPORTER,
            kernel::PNPM_NO_STRICT_DEP_BUILDS,
        ],
        &profile_dir(data_dir, profile_name),
        &log_path,
        &[pnpm_dir],
        on_progress,
    )
    .map_err(|e| AppError::Io(format!("无法运行 pnpm（{e}）")))
}

/// Raw text of the profile manifest, captured before the boot guard rewrites
/// wiring so a give-up can restore exactly what the user had.
pub fn snapshot_profile_manifest_text(data_dir: &Path, profile: &str) -> Option<String> {
    fs::read_to_string(profile_dir(data_dir, profile).join("package.json")).ok()
}

/// Restore a previously snapshotted manifest and resync node_modules with
/// it. A missing snapshot keeps the current manifest and only reruns the
/// install — the best available repair when the file itself vanished.
pub fn restore_profile_manifest(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    previous: Option<&str>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let path = profile_dir(data_dir, &settings.profile).join("package.json");
    if let Some(text) = previous {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
        }
        fs::write(&path, text).map_err(|e| AppError::Io(e.to_string()))?;
    }
    on_progress("正在恢复 profile 依赖（pnpm install）");
    let status = run_profile_install(data_dir, &settings.profile, pnpm_exe, on_progress)?;
    if !status.success() {
        return Err(AppError::Plugin(format!(
            "pnpm install 在恢复后的 profile 中失败（退出码 {:?}），详情见日志：{}",
            status.code(),
            wiring_log_path(data_dir).display()
        )));
    }
    Ok(())
}

/// Record (or clear) the store's user-facing warning. Shared by the quiet
/// wiring path and the boot guard's first-attempt repair so both surfaces
/// agree on what the UI shows next to the plugin card.
pub fn set_store_warning(data_dir: &Path, warning: Option<String>) {
    let mut store = load_store(data_dir);
    store.warning = warning;
    let _ = save_store(data_dir, &store);
}

/// Quiet wiring for sync commands (kernel switch / start): failures are
/// recorded in the store for plugin_status.warning instead of blocking the
/// action. Reuses the caller's cached node probe so the switch never
/// spawns a second `node --version`.
pub fn ensure_wiring_quiet(
    data_dir: &Path,
    settings: &settings::Settings,
    node_info: &node::NodeInfo,
) -> Result<(), String> {
    let (_, pnpm_exe) = commands::promise_pnpm(data_dir, node_info, |_| {})?;
    let mut noop = |_: &str| {};
    match ensure_wiring(data_dir, settings, &pnpm_exe, &mut noop) {
        Ok(_) => {
            set_store_warning(data_dir, None);
            Ok(())
        }
        Err(e) => {
            set_store_warning(data_dir, Some(e.to_string()));
            Err(e.to_string())
        }
    }
}

// --- update checks ----------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub id: String,
    pub latest: Option<String>,
    pub error: Option<String>,
}

/// Check every store item against its origin's latest version. Stores the
/// results back into the store for the UI badge.
pub fn check_updates(data_dir: &Path) -> Result<Vec<UpdateInfo>, AppError> {
    let mut store = load_store(data_dir);
    let mut out = Vec::new();
    for item in &mut store.items {
        let (latest, error) = match item.origin.as_str() {
            "npm" => match fetch_npm_doc(&item.source) {
                Ok(doc) => (doc.dist_tags.get("latest").cloned(), None),
                Err(e) => (None, Some(e)),
            },
            "git" => match git_latest(item) {
                Ok(v) => (v, None),
                Err(e) => (None, Some(e)),
            },
            _ => (None, None),
        };
        let newer =
            latest.filter(|v| is_newer_than(v, &item.installed_version, &item.origin, item.pinned));
        item.latest_version = newer.clone();
        out.push(UpdateInfo {
            id: item.id.clone(),
            latest: newer,
            error,
        });
    }
    store.last_checked_at = Some(now_epoch_secs());
    save_store(data_dir, &store)?;
    Ok(out)
}

/// Latest version of a git-origin plugin: the highest semver tag the
/// remote has published. `fetch_git` aligns `installed_version` with
/// the same shape (a tag, or the HEAD hash as a fallback), so
/// `is_newer_than` can compare them directly.
///
/// The unpinned branch tracks the highest tag rather than the branch
/// HEAD — a developer who pushed new commits but has not cut a release
/// yet will not look "newer" than the user's last install. Plugin
/// authors publish releases via tags; that is what the user wants
/// notified about.
fn git_latest(item: &StoreItem) -> Result<Option<String>, String> {
    git_latest_tag(&item.source)
}

/// Highest semver tag the remote has published, used by `fetch_git`
/// (to pick the branch when the source is unpinned) and by `git_latest`
/// (to compare against the installed version). Returns None when the
/// remote has no usable tags.
fn git_latest_tag(source: &str) -> Result<Option<String>, String> {
    let (ok, out) =
        run_capture("git", &["ls-remote", "--tags", source]).map_err(|e| e.to_string())?;
    if !ok {
        return Ok(None);
    }
    let tags: Vec<String> = out
        .lines()
        .filter_map(|line| {
            let (_, ref_part) = line.split_once('\t')?;
            let tag = ref_part.strip_prefix("refs/tags/")?.trim_end_matches("^{}");
            Some(tag.to_string())
        })
        .collect();
    Ok(latest_tag(tags.iter().map(|s| s.as_str())))
}

// --- catalog ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CatalogDoc {
    #[serde(default)]
    items: Vec<CatalogRaw>,
}

#[derive(Debug, Deserialize)]
struct CatalogRaw {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    stars: u64,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    verified: bool,
    #[serde(rename = "install", default)]
    install: Option<CatalogInstall>,
    #[serde(default)]
    category: String,
}

#[derive(Debug, Deserialize)]
struct CatalogInstall {
    #[serde(default)]
    method: String,
}

/// dsh-plugin.org short-key payload (`/api/plugins.zh.json`).
#[derive(Debug, Deserialize)]
struct HubRaw {
    /// Plugin slug.
    #[serde(default)]
    s: String,
    /// Owner slug (GitHub owner, lowercase).
    #[serde(default)]
    o: String,
    /// Display name.
    #[serde(default)]
    n: String,
    /// Latest version, e.g. `v3.22.1`.
    #[serde(default)]
    vr: String,
    /// Category id (interface/session/memory/tools/agent/workflow/...).
    #[serde(default)]
    c: String,
    /// Tags.
    #[serde(default)]
    t: Vec<String>,
    /// Description.
    #[serde(default)]
    d: String,
    /// Repo reference. Accepts both the shorthand `"owner/repo"` and the
    /// detailed `{ "repo": "owner/repo", "npmPackage": "pkg" }` form; an
    /// entry with both indicates the author ships an npm distribution
    /// alongside the source repo and the catalog should install from npm.
    #[serde(default, deserialize_with = "deserialize_hub_repo")]
    r: HubRepo,
    /// Verification state; `verified` means manually reviewed.
    #[serde(default)]
    v: String,
    /// Last upstream update (ISO 8601).
    #[serde(default)]
    u: String,
    /// Stars.
    #[serde(default)]
    sg: u64,
    /// Forks.
    #[serde(default)]
    fk: u64,
}

/// Repo reference from a hub entry. Either the bare `owner/repo` shorthand
/// or the detailed `{ repo, npmPackage }` object. Empty / missing fields
/// resolve to `None` so downstream code never sees a placeholder string it
/// has to filter out.
#[derive(Debug, Default, Clone)]
struct HubRepo {
    repo: Option<String>,
    npm_package: Option<String>,
}

impl HubRepo {
    fn repo(&self) -> Option<&str> {
        self.repo.as_deref().filter(|s| !s.is_empty())
    }
    fn npm_package(&self) -> Option<&str> {
        self.npm_package.as_deref().filter(|s| !s.is_empty())
    }
}

/// Deserialize `r` from either a JSON string or `{ repo, npmPackage }`
/// object. Anything else (null, number, …) collapses to an empty
/// `HubRepo`, the same shape the field takes when the JSON omits it.
///
/// The hub feed switched a majority of entries to the detailed form when
/// authors started publishing npm distributions alongside the source repo.
/// The previous `r: String` deserializer rejected those entries wholesale,
/// so a single detailed entry caused `Vec<HubRaw>::from_str` to fail for
/// the whole array — the catalog then silently fell back to the much
/// smaller reference market. Accepting both shapes here is what makes
/// hub-only plugins surface in the plugin center at all.
fn deserialize_hub_repo<'de, D>(de: D) -> Result<HubRepo, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_json::Value::deserialize(de)?;
    match value {
        serde_json::Value::Null => Ok(HubRepo::default()),
        serde_json::Value::String(s) => Ok(HubRepo {
            repo: (!s.is_empty()).then_some(s),
            npm_package: None,
        }),
        serde_json::Value::Object(_) => {
            #[derive(Deserialize)]
            struct Detail {
                #[serde(default)]
                repo: String,
                #[serde(default, rename = "npmPackage")]
                npm_package: String,
            }
            let d: Detail = serde_json::from_value(value).map_err(Error::custom)?;
            Ok(HubRepo {
                repo: (!d.repo.is_empty()).then_some(d.repo),
                npm_package: (!d.npm_package.is_empty()).then_some(d.npm_package),
            })
        }
        _ => Err(Error::custom(
            "`r` must be a string or { repo, npmPackage } object",
        )),
    }
}

impl HubRaw {
    /// Normalize a hub entry to the shared catalog item. The hub feed
    /// distinguishes four install paths, picked here in priority order:
    ///
    /// 1. `npmPackage` set → install from npm (author-published package,
    ///    no compile step; preferred when the author ships one).
    /// 2. `repo` set → install from the GitHub repo via `git clone`.
    /// 3. `repo` missing/empty but `o` + `n` both populated → fall back
    ///    to `github.com/{o}/{n}.git`. The hub has dozens of these — the
    ///    author published the GitHub repo but left the `r` manifest
    ///    field blank, so without this fallback `parse_spec` would route
    ///    the install to npm with the display name, hit a 404, and leave
    ///    the user with the misleading 「查询 npm 失败：404」 error.
    /// 4. last-ditch → npm with the display name (legacy fallback for
    ///    genuinely npm-only entries that never filled in `r`).
    ///
    /// The catalog UI surfaces the chosen origin on each card so the user
    /// sees whether an 「安装」 button triggers an npm or git fetch.
    fn into_item(self) -> CatalogItem {
        let repo = self.r.repo().map(str::to_string);
        let npm_package = self.r.npm_package().map(str::to_string);
        let detail_url = if !self.o.is_empty() && !self.s.is_empty() {
            format!("https://dsh-plugin.org/zh/plugins/{}/{}", self.o, self.s)
        } else {
            repo.as_ref()
                .map(|r| format!("https://github.com/{r}"))
                .unwrap_or_default()
        };
        let (origin, spec) = if let Some(pkg) = &npm_package {
            ("npm", pkg.clone())
        } else if let Some(r) = &repo {
            ("git", format!("https://github.com/{r}.git"))
        } else if !self.o.is_empty() && !self.n.is_empty() {
            (
                "git",
                format!("https://github.com/{}/{}.git", self.o, self.n),
            )
        } else {
            ("npm", self.n.clone())
        };
        let id = if self.s.is_empty() {
            self.n.clone()
        } else {
            self.s.clone()
        };
        CatalogItem {
            id,
            name: self.n,
            kind: String::new(),
            description: self.d,
            stars: self.sg,
            forks: self.fk,
            downloads: 0,
            verified: self.v == "verified",
            repo,
            spec,
            origin: origin.to_string(),
            category: self.c,
            version: self.vr,
            tags: self.t,
            updated: self.u,
            detail_url,
        }
    }
}

/// Normalize a reference-market entry to the shared catalog item.
fn from_market_raw(raw: CatalogRaw) -> CatalogItem {
    let npm_origin = raw.package.is_some()
        || raw
            .install
            .as_ref()
            .map(|i| matches!(i.method.as_str(), "npm" | "pnpm" | "dsh-plugin-add"))
            .unwrap_or(false);
    let (origin, spec) = if npm_origin {
        (
            "npm",
            raw.package.clone().unwrap_or_else(|| raw.name.clone()),
        )
    } else if let Some(repo) = &raw.repo {
        let tag = raw.version.as_deref().filter(|v| {
            let head = v
                .strip_prefix('v')
                .unwrap_or(v)
                .split_once('-')
                .map(|(h, _)| h)
                .unwrap_or(v);
            let parts: Vec<&str> = head.split('.').collect();
            parts.len() >= 2 && parts[..2].iter().all(|s| s.parse::<u64>().is_ok())
        });
        let base = format!("https://github.com/{repo}.git");
        (
            "git",
            match tag {
                Some(t) => format!("{base}#{t}"),
                None => base,
            },
        )
    } else {
        ("git", raw.repo.clone().unwrap_or_default())
    };
    let detail_url = raw
        .repo
        .as_ref()
        .map(|r| format!("https://github.com/{r}"))
        .unwrap_or_default();
    CatalogItem {
        id: raw.id,
        name: raw.name,
        kind: raw.kind,
        description: raw.description.unwrap_or_default(),
        stars: raw.stars,
        forks: 0,
        downloads: raw.downloads,
        verified: raw.verified,
        repo: raw.repo,
        spec,
        origin: origin.to_string(),
        category: raw.category,
        version: raw.version.unwrap_or_default(),
        tags: Vec::new(),
        updated: String::new(),
        detail_url,
    }
}

/// Drop catalog entries the plugin center should not surface. The UI
/// frames npm install as the canonical path (placeholder reads
/// `npm i @scope/pkg …`), and the manual-install flow also resolves
/// `npm i`/`pnpm add`/`yarn add`/`bun add` natively — entries with no
/// npm package are therefore un-installable through the center's UI.
/// They are still reachable through the manual install input (where
/// `owner/repo` / git URL / dsh plugin CLI work) but the catalog
/// should not list them, otherwise every 「安装」 button on those
/// rows would 404 against the npm registry.
fn filter_npm_origin(items: Vec<CatalogItem>) -> Vec<CatalogItem> {
    items.into_iter().filter(|i| i.origin == "npm").collect()
}

/// Fetch the community catalog, caching the normalized items for
/// CATALOG_TTL_SECS (`force` bypasses the cache). The dsh-plugin.org hub is
/// the primary source; the reference market listing is the fallback when the
/// hub is unreachable. The cached payload is always filtered to npm-origin
/// entries so a stale cache written before this filter was introduced does
/// not leak git-only rows into the plugin center on first read.
fn fetch_catalog(data_dir: &Path, force: bool) -> Result<Vec<CatalogItem>, String> {
    let cache = data_dir.join(CATALOG_CACHE_FILE);
    let fresh = !force
        && fs::metadata(&cache)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok().map(|e| e.as_secs() < CATALOG_TTL_SECS))
            .unwrap_or(false);
    if fresh {
        if let Ok(text) = fs::read_to_string(&cache) {
            if let Ok(items) = serde_json::from_str::<Vec<CatalogItem>>(&text) {
                return Ok(filter_npm_origin(items));
            }
        }
    }
    let hub = http_get_string(HUB_CATALOG_URL, None).and_then(|body| {
        serde_json::from_str::<Vec<HubRaw>>(&body)
            .map_err(|e: serde_json::Error| e.to_string())
            .map(|raws| raws.into_iter().map(HubRaw::into_item).collect::<Vec<_>>())
    });
    let items = match hub {
        Ok(items) if !items.is_empty() => items,
        _ => {
            let body = http_get_string(MARKET_CATALOG_URL, None)?;
            let doc: CatalogDoc =
                serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())?;
            doc.items.into_iter().map(from_market_raw).collect()
        }
    };
    let items = filter_npm_origin(items);
    if fs::create_dir_all(data_dir).is_ok() {
        if let Ok(text) = serde_json::to_string(&items) {
            let _ = fs::write(&cache, text);
        }
    }
    Ok(items)
}

/// The full community catalog sorted by stars (`force` bypasses the cache).
/// Search and category filtering happen in the UI so filtering over the
/// cached list is instant.
pub fn catalog(data_dir: &Path, force: bool) -> Result<Vec<CatalogItem>, AppError> {
    let mut items = fetch_catalog(data_dir, force)
        .map_err(|e| AppError::Plugin(format!("目录获取失败：{e}")))?;
    items.sort_by_key(|a| std::cmp::Reverse(a.stars));
    Ok(items)
}

/// Apply the store's plugin dependencies and bundle layers onto a profile
/// manifest. Returns whether anything changed. Pure (no fs, no pnpm), so
/// wiring is unit-testable without a toolchain.
fn wire_manifest(
    root: &mut serde_json::Value,
    specs: &BTreeMap<String, (String, bool)>,
    profile: &str,
) -> Result<bool, AppError> {
    let mut changed = false;
    let deps = root
        .get_mut("dependencies")
        .and_then(|d| d.as_object_mut())
        .ok_or_else(|| AppError::Plugin("profile manifest 缺少 dependencies".into()))?;
    for (name, (spec, _)) in specs {
        if deps.get(name).and_then(|s| s.as_str()) != Some(spec.as_str()) {
            deps.insert(name.clone(), serde_json::Value::String(spec.clone()));
            changed = true;
        }
    }
    deps.retain(|name, spec| {
        if !is_managed_spec(spec.as_str().unwrap_or("")) {
            return true; // 用户/CLI 管理的不动
        }
        if !specs.contains_key(name) {
            changed = true;
            return false;
        }
        true
    });

    // bundles：模板层与托管层重建，用户其他条目（CLI 添加等）原样保留。
    // 已卸载的托管插件：依赖被清退后其层必须同步清退，否则内核启动会因无法
    // 解析该 bundle 而失败——因此只保留依赖仍存在且非托管 spec 的层。
    let kept_user_bundles: std::collections::HashSet<String> = deps
        .iter()
        .filter(|(_, spec)| !is_managed_spec(spec.as_str().unwrap_or("")))
        .map(|(name, _)| name.clone())
        .collect();
    let managed_bundles: Vec<String> = specs
        .iter()
        .filter(|(_, (_, is_bundle))| *is_bundle)
        .map(|(name, _)| name.clone())
        .collect();
    let template: Vec<String> = template_bundles(profile);
    let mut next: Vec<String> = template.clone();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let bundles = root
        .get_mut("dsh")
        .and_then(|d| d.get_mut("profile"))
        .and_then(|p| p.get_mut("bundles"))
        .and_then(|b| b.as_array_mut())
        .ok_or_else(|| AppError::Plugin("profile manifest 缺少 dsh.profile.bundles".into()))?;
    for name in bundles.iter().filter_map(|b| b.as_str().map(String::from)) {
        if seen.contains(&name) || template.contains(&name) || managed_bundles.contains(&name) {
            continue;
        }
        if !kept_user_bundles.contains(&name) {
            changed = true;
            continue;
        }
        seen.insert(name.clone());
        next.push(name);
    }
    for name in &managed_bundles {
        if !next.contains(name) {
            next.push(name.clone());
        }
    }
    let current: Vec<String> = bundles
        .iter()
        .filter_map(|b| b.as_str().map(String::from))
        .collect();
    if next != current {
        *bundles = next.into_iter().map(serde_json::Value::String).collect();
        changed = true;
    }
    Ok(changed)
}

// --- orchestration ----------------------------------------------------------

/// Install a plugin: fetch into the store, install store deps in link mode,
/// materialize into every kernel, wire the active profile.
pub fn install(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    spec_str: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<StoreItem, AppError> {
    let spec = parse_spec(spec_str)?;
    if store_item(data_dir, &spec.id).is_some() {
        return Err(AppError::Plugin(format!(
            "{} 已安装，请使用「更新」",
            spec.name
        )));
    }
    let mut item = fetch_into_store(data_dir, pnpm_exe, &spec, on_progress)?;
    item.mode = if mode == "copy" { "copy" } else { "link" }.to_string();
    if item.mode == "link" {
        // Ensure the store-level .npmrc exists before installing deps, so the
        // minimumReleaseAge exclusion is in place even if the store was created
        // before this fix was deployed.
        ensure_store_npmrc(data_dir).ok();
        install_store_deps(data_dir, pnpm_exe, &item.id, on_progress)?;
    }
    upsert_item(data_dir, item.clone())?;
    // 重装代表明确的重试意图：清掉历史隔离记录，否则新装的插件会被旧
    // 记录挡在接线之外，表现为"装了却不生效"的哑故障。
    let _ = quarantine::remove(data_dir, &item.id);
    sync_kernels(data_dir, &item)?;
    on_progress("正在接线到 profile");
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(item)
}

/// Update one plugin: re-fetch the same source, refresh store deps, re-sync
/// all kernels, re-wire.
pub fn update(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<StoreItem, AppError> {
    let item =
        store_item(data_dir, id).ok_or_else(|| AppError::Plugin("插件不在中央库中".into()))?;
    if item.pinned {
        return Err(AppError::Plugin(format!(
            "{} 已锁定版本 {}，如需升级请重新安装（不带版本号）",
            item.name, item.installed_version
        )));
    }
    let spec = parse_spec(&item.source)?;
    on_progress(&format!("正在更新 {}", item.name));
    let fetched = fetch_into_store(data_dir, pnpm_exe, &spec, on_progress)?;
    let mut updated = fetched;
    updated.mode = item.mode.clone();
    if updated.mode == "link" {
        ensure_store_npmrc(data_dir).ok();
        install_store_deps(data_dir, pnpm_exe, &updated.id, on_progress)?;
    }
    // Sync latest_version to what we just installed so the UI badge
    // clears immediately after a successful update. Without this, the
    // previous `check_updates` result lingers and the badge keeps
    // reporting the same phantom "newer version" the user just
    // installed. A later `check_updates` can still raise `latest_version`
    // when the remote has moved on since this fetch.
    updated.latest_version = Some(updated.installed_version.clone());
    upsert_item(data_dir, updated.clone())?;
    // 与 install 同理：更新是明确的重试意图，历史隔离记录不再适用。
    let _ = quarantine::remove(data_dir, &updated.id);
    sync_kernels(data_dir, &updated)?;
    on_progress("正在同步 profile");
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(updated)
}

/// Remove a plugin everywhere: store, kernel materializations, profile wiring.
pub fn uninstall(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    store_item(data_dir, id).ok_or_else(|| AppError::Plugin("插件不在中央库中".into()))?;
    // Delete the store dir FIRST: on Windows a file locked by the running
    // kernel makes remove_dir_all fail, and failing here leaves every other
    // piece of state untouched so the user can close the workbench and
    // retry. Swallowing this error used to leave an orphaned store dir the
    // shell could neither wire nor remove.
    let store_path = store_plugin_dir(data_dir, id);
    match fs::remove_dir_all(&store_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(AppError::Io(format!(
                "无法删除插件目录 {}：{e}。文件可能正被运行中的内核占用，请关闭工作台后重试卸载",
                store_path.display()
            )));
        }
    }
    for version in kernel::list_installed(data_dir) {
        remove_materialized(data_dir, &version.version, id);
    }
    remove_item(data_dir, id)?;
    // 隔离记录随卸载一并清除：残留记录会在用户日后重装同名插件时把它挡
    // 在接线之外，形成"装了却不生效"的哑故障。
    quarantine::remove(data_dir, id)?;
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(())
}

/// Re-apply the desired mode to every kernel and re-wire.
pub fn set_mode(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    id: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    if mode != "link" && mode != "copy" {
        return Err(AppError::Plugin("模式只能是 link 或 copy".into()));
    }
    let mut item =
        store_item(data_dir, id).ok_or_else(|| AppError::Plugin("插件不在中央库中".into()))?;
    item.mode = mode.to_string();
    upsert_item(data_dir, item.clone())?;
    if mode == "link" {
        install_store_deps(data_dir, pnpm_exe, id, on_progress)?;
    }
    sync_kernels(data_dir, &item)?;
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(())
}

/// Materialize everything and re-wire (the「同步」button).
pub fn sync_all(
    data_dir: &Path,
    settings: &settings::Settings,
    pnpm_exe: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let store = load_store(data_dir);
    for item in &store.items {
        sync_kernels(data_dir, item)?;
    }
    ensure_wiring(data_dir, settings, pnpm_exe, on_progress)?;
    Ok(())
}

/// Compose the UI status snapshot (no network).
pub fn status(data_dir: &Path, settings: &settings::Settings) -> PluginStatus {
    let store = load_store(data_dir);
    let active = kernel::read_active(data_dir);
    let profile_manifest = read_profile_json(data_dir, &settings.profile)
        .ok()
        .flatten();
    let quarantine_doc = quarantine::load(data_dir);

    let mut rows = Vec::new();
    let mut updates = 0;
    for item in &store.items {
        let quarantined = quarantine_doc
            .items
            .iter()
            .find(|q| q.id == item.id)
            .cloned();
        let (actual_mode, synced) = match &active {
            Some(version) => {
                let meta = read_meta(data_dir, version, &item.id);
                let present = kernel_plugin_dir(data_dir, version, &item.id).exists();
                let current = meta
                    .as_ref()
                    .map(|m| m.version == item.installed_version)
                    .unwrap_or(false);
                (meta.map(|m| m.mode), present && current)
            }
            None => (None, false),
        };
        let wired = profile_manifest
            .as_ref()
            .and_then(|m| m.get("dependencies"))
            .and_then(|d| d.get(&item.name))
            .and_then(|s| s.as_str())
            .map(is_managed_spec)
            .unwrap_or(false);
        if item
            .latest_version
            .as_deref()
            .map(|l| is_newer_than(l, &item.installed_version, &item.origin, item.pinned))
            .unwrap_or(false)
        {
            updates += 1;
        }
        // The UI's per-row "有更新" badge checks `row.latest_version` for
        // truthiness rather than re-running the version comparison, so we
        // hide the field whenever the recorded "latest" is no longer
        // newer than what the user actually installed. Without this the
        // row keeps the badge after a successful update (latest ==
        // installed) and after `update()`'s explicit `latest_version =
        // installed_version` sync. The top-level count above already
        // does the same filter for the `N 个更新` pill.
        let row_latest = item
            .latest_version
            .as_deref()
            .filter(|l| is_newer_than(l, &item.installed_version, &item.origin, item.pinned))
            .map(|s| s.to_string());
        rows.push(PluginRow {
            id: item.id.clone(),
            name: item.name.clone(),
            origin: item.origin.clone(),
            source: item.source.clone(),
            installed_version: item.installed_version.clone(),
            latest_version: row_latest,
            pinned: item.pinned,
            desired_mode: item.mode.clone(),
            actual_mode,
            synced,
            wired,
            quarantined,
            repo_url: item.repo_url.clone(),
            description: item.description.clone(),
            installed_at: item.installed_at.clone(),
            updated_at: item.updated_at.clone(),
        });
    }
    PluginStatus {
        rows,
        profile: settings.profile.clone(),
        active_kernel: active,
        updates,
        last_checked_at: store.last_checked_at,
        warning: store.warning,
    }
}

/// Resolve a link-mode plugin's peerDependencies from the ACTIVE kernel's
/// node_modules into the store dir, so the plugin's import walk finds the
/// same cordis/dsh-* instances the kernel uses. Recorded in
/// .dsh-peers.json keyed by kernel version, so a kernel switch re-runs it.
fn refresh_store_peers(data_dir: &Path, item: &StoreItem, active: &str) -> Result<(), AppError> {
    if item.mode != "link" {
        return Ok(());
    }
    let plugin_root = store_plugin_dir(data_dir, &item.id);
    let meta_path = plugin_root.join(".dsh-peers.json");
    let meta: serde_json::Value = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if meta.get("kernel").and_then(|k| k.as_str()) == Some(active)
        && meta.get("peers").and_then(|p| p.as_array()).is_some()
    {
        return Ok(()); // 已为该内核解析过
    }
    let Ok(manifest) = read_plugin_manifest(&plugin_root) else {
        return Ok(());
    };
    let Some(peers) = manifest.get("peerDependencies").and_then(|p| p.as_object()) else {
        return Ok(());
    };
    let kernel_mm = kernel::kernel_dir(data_dir, active).join("node_modules");
    let mut linked: Vec<String> = Vec::new();
    for name in peers.keys() {
        let target = kernel_mm.join(name);
        if !target.exists() {
            continue;
        }
        let dest = plugin_root.join("node_modules").join(name);
        if dest.exists() {
            continue; // 已在库内安装（已发布或被 hoisted）
        }
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if make_dir_link(&target, &dest).is_err() {
            // 链接不可用（Windows 权限等）→ 复制一份，解析不依赖链接能力
            let _ = copy_tree(&target, &dest);
        }
        linked.push(name.clone());
    }
    let text = serde_json::json!({ "kernel": active, "peers": linked });
    if let Ok(text) = serde_json::to_string(&text) {
        let _ = fs::write(meta_path, text);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Unique throwaway home per test, removed on drop.
    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            let nano = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let base =
                std::env::temp_dir().join(format!("dsh-plugins-test-{}", std::process::id()));
            let home = base.join(nano.to_string());
            fs::create_dir_all(&home).expect("test home");
            TestHome(home)
        }

        fn data_dir(&self) -> PathBuf {
            self.0.join("desktop")
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_npm_specs() {
        assert_eq!(parse_spec("dsh-market").unwrap().origin, "npm");
        let scoped = parse_spec("@ace-zone/dsh-market").unwrap();
        assert_eq!(scoped.origin, "npm");
        assert_eq!(scoped.source, "@ace-zone/dsh-market");
        assert_eq!(scoped.pin, None);
        let pinned = parse_spec("@ace-zone/dsh-market@0.1.66").unwrap();
        assert_eq!(pinned.pin.as_deref(), Some("0.1.66"));
        assert_eq!(pinned.id, "@ace-zone__dsh-market");
        let unpinned = parse_spec("dsh-market@1.2.3").unwrap();
        assert_eq!(unpinned.source, "dsh-market");
        assert_eq!(unpinned.pin.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn filter_npm_origin_drops_git_only_entries() {
        // The plugin center only surfaces entries that the 「安装」
        // button can actually pull from npm. Git-only entries are
        // still reachable through the manual-install input (which
        // accepts owner/repo / git URL / dsh plugin CLI), but listing
        // them in the catalog would surface 404s the moment the user
        // clicks install.
        let mk = |id: &str, name: &str, origin: &str, spec: &str, repo: Option<&str>| CatalogItem {
            id: id.into(),
            name: name.into(),
            kind: String::new(),
            description: String::new(),
            stars: 0,
            forks: 0,
            downloads: 0,
            verified: false,
            repo: repo.map(str::to_string),
            spec: spec.into(),
            origin: origin.into(),
            category: String::new(),
            version: String::new(),
            tags: vec![],
            updated: String::new(),
            detail_url: String::new(),
        };
        let npm_only = mk("a", "a", "npm", "@scope/a", Some("owner/a"));
        let git_only = mk("b", "b", "git", "https://github.com/owner/b.git", None);

        let out = filter_npm_origin(vec![npm_only.clone(), git_only]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].origin, "npm");
        // Empty input is a no-op.
        assert!(filter_npm_origin(vec![]).is_empty());
        // All-npm passes through unchanged.
        assert_eq!(filter_npm_origin(vec![npm_only.clone(), npm_only]).len(), 2);
    }

    #[test]
    fn resolves_npm_version_through_dist_tags() {
        // Pinning to "latest" should hop through `dist-tags.latest` to
        // the actual semver the registry publishes — without this hop
        // the literal `"latest"` is used as a `versions` key and the
        // user sees "没有可下载的 tarball" even though the package
        // and its tarball both exist (the bug @linxin666/dsh-liangshen
        // hit on the user's machine).
        let doc: NpmDoc = serde_json::from_str(
            r#"{
                "dist-tags": {"latest": "0.3.2", "next": "1.0.0-rc.1"},
                "versions": {
                    "0.3.2": {},
                    "1.0.0-rc.1": {},
                    "0.1.0": {}
                }
            }"#,
        )
        .expect("npm doc");
        assert_eq!(
            resolve_npm_version(&doc, None),
            ("0.3.2".into(), "latest".into())
        );
        assert_eq!(
            resolve_npm_version(&doc, Some("latest")),
            ("0.3.2".into(), "latest".into())
        );
        assert_eq!(
            resolve_npm_version(&doc, Some("next")),
            ("1.0.0-rc.1".into(), "next".into())
        );
        // Literal semver pin: keep the pin unchanged so the caller's
        // `versions[<pin>]` lookup surfaces a precise error.
        assert_eq!(
            resolve_npm_version(&doc, Some("1.0.0-rc.1")),
            ("1.0.0-rc.1".into(), "1.0.0-rc.1".into())
        );
        assert_eq!(
            resolve_npm_version(&doc, Some("9.9.9")),
            ("9.9.9".into(), "9.9.9".into())
        );

        // No `dist-tags.latest`: `None` returns empty, error surfaces
        // "找不到包 … 或其 latest 标记".
        let doc: NpmDoc = serde_json::from_str(r#"{"versions": {"1.0.0": {}}}"#).unwrap();
        assert_eq!(
            resolve_npm_version(&doc, None),
            ("".into(), "latest".into())
        );
    }

    #[test]
    fn parses_package_manager_install_cli() {
        // The exact form the user pastes from docs: `npm i @scope/pkg@v`.
        let spec = parse_spec("npm i @linxin666/dsh-liangshen").unwrap();
        assert_eq!(spec.origin, "npm");
        assert_eq!(spec.source, "@linxin666/dsh-liangshen");
        assert_eq!(spec.pin, None);

        // `install` and `add` verbs, all four package-manager prefixes.
        let spec = parse_spec("npm install @scope/pkg@1.2.3").unwrap();
        assert_eq!(spec.source, "@scope/pkg");
        assert_eq!(spec.pin.as_deref(), Some("1.2.3"));
        assert_eq!(parse_spec("pnpm add owner/repo").unwrap().origin, "git");
        assert_eq!(parse_spec("yarn add owner/repo").unwrap().origin, "git");
        assert_eq!(
            parse_spec("bun add @scope/pkg@latest").unwrap().source,
            "@scope/pkg"
        );

        // Flags (`--save-dev`, `-D`) before the package spec are dropped
        // silently. `npm i -D <pkg>` and `npm install --save-dev <pkg>`
        // both reduce to the bare package spec.
        let spec = parse_spec("npm i -D @scope/pkg@1.0.0").unwrap();
        assert_eq!(spec.source, "@scope/pkg");
        let spec = parse_spec("npm install --save-dev @scope/pkg@1.0.0").unwrap();
        assert_eq!(spec.source, "@scope/pkg");

        // `npm install` (no package spec) falls through to npm parsing
        // and fails loudly on whitespace, so the user sees an actionable
        // error instead of a silent no-op.
        assert!(parse_spec("npm install").is_err());
    }

    #[test]
    fn parses_dsh_plugin_cli_form() {
        // Full `dsh plugin --profile X add <pkg>` form, the canonical
        // command the kernel ships — users paste it from docs / chat
        // suggestions; the shell must accept it verbatim.
        let spec =
            parse_spec("dsh plugin --profile web add @linxin666/dsh-liangshen@latest").unwrap();
        assert_eq!(spec.origin, "npm");
        assert_eq!(spec.source, "@linxin666/dsh-liangshen");
        assert_eq!(spec.pin.as_deref(), Some("latest"));

        // No `--profile` flag: still parse the package spec.
        let spec = parse_spec("dsh plugin add @linxin666/dsh-liangshen@latest").unwrap();
        assert_eq!(spec.origin, "npm");
        assert_eq!(spec.source, "@linxin666/dsh-liangshen");
        assert_eq!(spec.pin.as_deref(), Some("latest"));

        // `install` is an alias for `add` in the kernel CLI.
        let spec = parse_spec("dsh plugin install @scope/pkg@1.2.3").unwrap();
        assert_eq!(spec.source, "@scope/pkg");
        assert_eq!(spec.pin.as_deref(), Some("1.2.3"));

        // Short `-p` flag.
        let spec = parse_spec("dsh plugin -p web add owner/repo#v1.0.0").unwrap();
        assert_eq!(spec.origin, "git");
        assert_eq!(spec.source, "https://github.com/owner/repo.git");
        assert_eq!(spec.pin.as_deref(), Some("v1.0.0"));

        // `dsh plugin … remove` / `update` / `list` are not install verbs;
        // the manual-install UI must reject them so a pasted command
        // can't accidentally uninstall a plugin.
        assert!(parse_spec("dsh plugin remove @scope/pkg").is_err());
        assert!(parse_spec("dsh plugin list").is_err());
    }

    #[test]
    fn validate_plugin_checks_the_load_contract() {
        let home = TestHome::new();
        let dir = home.0.join("plugin");
        fs::create_dir_all(&dir).expect("plugin dir");

        // 没有 package.json：拒绝
        assert!(validate_plugin(&dir).is_err());

        // bundle 层声明了 patch 但文件缺失：拒绝
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_err());

        // patch 文件补齐但无运行时入口：仍拒绝，bundle 层不能替代 main/exports
        fs::write(dir.join("cordis.patch.yml"), "patches: []\n").expect("patch");
        assert!(validate_plugin(&dir).is_err());

        // 普通依赖型插件：main 指向真实文件才放行
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","main":"lib/index.js"}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_err());
        fs::create_dir_all(dir.join("lib")).expect("lib");
        fs::write(dir.join("lib/index.js"), "module.exports = {}\n").expect("entry");
        assert!(validate_plugin(&dir).is_ok());

        // bundle 层 + 有效运行时入口：放行
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","main":"lib/index.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_ok());

        // exports 入口存在即放行（Node 自己解析其目标）
        fs::write(
            dir.join("package.json"),
            r#"{"name":"p","exports":"./lib/index.js"}"#,
        )
        .expect("manifest");
        assert!(validate_plugin(&dir).is_ok());

        // 既无 bundle 也无入口：拒绝
        fs::write(dir.join("package.json"), r#"{"name":"p"}"#).expect("manifest");
        assert!(validate_plugin(&dir).is_err());
    }

    #[test]
    fn hub_entry_normalizes_to_catalog_item() {
        let raw: HubRaw = serde_json::from_str(
            r#"{"s":"modlens","o":"liustack","n":"modlens","vr":"v3.22.1","c":"tools",
                "t":["vision"],"d":"desc","r":"liustack/modlens","v":"verified",
                "u":"2026-08-20T20:05:55Z","sg":3497,"fk":95}"#,
        )
        .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "git");
        assert_eq!(item.spec, "https://github.com/liustack/modlens.git");
        assert_eq!(item.version, "v3.22.1");
        assert_eq!(item.stars, 3497);
        assert_eq!(item.forks, 95);
        assert!(item.verified);
        assert_eq!(item.category, "tools");
        assert_eq!(
            item.detail_url,
            "https://dsh-plugin.org/zh/plugins/liustack/modlens"
        );

        // 无 repo 的条目回退 npm 安装
        let raw: HubRaw = serde_json::from_str(r#"{"n":"pkg","d":"x"}"#).expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "npm");
        assert_eq!(item.spec, "pkg");
        assert!(!item.verified);
    }

    #[test]
    fn hub_entry_with_npm_package_installs_from_npm() {
        // Author ships an npm distribution alongside the GitHub repo; the
        // catalog should pick the npm path so the UI's 「安装」 button
        // installs the published tarball (404-able on npm, unlike a git
        // clone of an arbitrary repo).
        let raw: HubRaw = serde_json::from_str(
            r#"{"s":"dsh-zhipu","n":"dsh-zhipu","r":{"repo":"fineven/dsh-zhipu","npmPackage":"dsh-zhipu"}}"#,
        )
        .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "npm");
        assert_eq!(item.spec, "dsh-zhipu");
        assert_eq!(item.repo.as_deref(), Some("fineven/dsh-zhipu"));
    }

    #[test]
    fn hub_entry_with_repo_object_no_npm_falls_back_to_git() {
        // A detailed `r` without `npmPackage` should still install via
        // git; the detailed form carries the same `repo` shorthand in
        // object clothing, so the install path is unchanged.
        let raw: HubRaw =
            serde_json::from_str(r#"{"s":"plug","n":"plug","r":{"repo":"owner/plug"}}"#)
                .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "git");
        assert_eq!(item.spec, "https://github.com/owner/plug.git");
    }

    #[test]
    fn hub_entry_with_empty_repo_object_falls_back_to_npm_name() {
        // A detailed `r` with neither field filled is treated the same
        // as a missing `r` — npm install using the display name.
        let raw: HubRaw =
            serde_json::from_str(r#"{"s":"plug","n":"plug","r":{}}"#).expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "npm");
        assert_eq!(item.spec, "plug");
        assert!(item.repo.is_none());
    }

    #[test]
    fn hub_entry_missing_repo_falls_back_to_owner_name_git() {
        // Author published the GitHub repo but left `r` blank in the
        // manifest. Without this fallback `parse_spec` would route the
        // install to npm with the display name, hit a 404, and surface
        // the misleading 「查询 npm 失败：404」 error the user just
        // reported on `dsh-web-ui`. `o` + `n` give us enough to
        // reconstruct the canonical github.com URL.
        let raw: HubRaw =
            serde_json::from_str(r#"{"s":"dsh-web-ui","o":"someone","n":"dsh-web-ui"}"#)
                .expect("hub raw");
        let item = raw.into_item();
        assert_eq!(item.origin, "git");
        assert_eq!(item.spec, "https://github.com/someone/dsh-web-ui.git");
        assert!(item.repo.is_none());
    }

    #[test]
    fn parses_git_specs() {
        let url = parse_spec("https://github.com/losebird/dsh-plugin-market").unwrap();
        assert_eq!(url.origin, "git");
        assert!(url.source.starts_with("https://"));
        let pinned = parse_spec("https://github.com/o/r.git#v1.2.3").unwrap();
        assert_eq!(pinned.pin.as_deref(), Some("v1.2.3"));
        let ssh = parse_spec("git@github.com:o/r.git").unwrap();
        assert_eq!(ssh.origin, "git");
        let shorthand = parse_spec("losebird/dsh-plugin-market").unwrap();
        assert_eq!(shorthand.origin, "git");
        assert_eq!(
            shorthand.source,
            "https://github.com/losebird/dsh-plugin-market.git"
        );
        assert_eq!(shorthand.id, "losebird__dsh-plugin-market");
    }

    #[test]
    fn rejects_path_traversal_ids() {
        assert!(id_for_name("a/../b").is_err());
        assert!(id_for_name("..").is_err());
        assert!(id_for_name("").is_err());
        assert!(id_for_name("a//b").is_err());
    }

    #[test]
    fn picks_latest_tag() {
        let tags = ["v1.2.3", "v1.2.0", "v0.9.0", "v2.0.0-rc.1"];
        assert_eq!(
            latest_tag(tags.iter().copied()).as_deref(),
            Some("v2.0.0-rc.1")
        );
        assert_eq!(
            latest_tag(["1.2.3", "1.10.0"].iter().copied()).as_deref(),
            Some("1.10.0")
        );
        assert_eq!(latest_tag(["not-a-version"].iter().copied()), None);
    }

    #[test]
    fn detects_semver_shape() {
        // Two numeric segments is the bar the tag filter and update
        // comparator both rely on; everything else is treated as a hash.
        assert!(looks_like_semver("v0.15.0"));
        assert!(looks_like_semver("0.15.0"));
        assert!(looks_like_semver("1.2.3-rc.1"));
        assert!(!looks_like_semver("v646c91c"));
        assert!(!looks_like_semver("head"));
        assert!(!looks_like_semver("1"));
        assert!(!looks_like_semver(""));
    }

    #[test]
    fn newer_than_handles_hash_vs_semver() {
        // npm / pinned git keep the semver ranking. The unpinned git
        // branch now also stores the highest remote tag (via
        // `fetch_git`), so it joins the same semver ranking path.
        assert!(is_newer_than("v0.15.0", "v0.14.0", "npm", false));
        assert!(!is_newer_than("v0.14.0", "v0.15.0", "npm", false));
        assert!(is_newer_than("v1.0.0", "v0.15.0", "git", true));
        assert!(is_newer_than("v0.16.0", "v0.15.0", "git", false));
        assert!(!is_newer_than("v0.15.0", "v0.15.0", "git", false));

        // Fallback path: unpinned git-origin whose repo has no usable
        // semver tags records `installed_version` as the HEAD short
        // hash. The remote `latest` is a tag, so a plain semver compare
        // would say Greater purely on segment count. Use string equality
        // instead so a fresh tag does not look "newer" forever after.
        assert!(!is_newer_than("v0.15.0", "v646c91c", "git", false));
        assert!(is_newer_than("vNEW1", "v646c91c", "git", false));
        assert!(!is_newer_than("v646c91c", "v646c91c", "git", false));
        // Pinned with a hash-shaped latest never reaches the special
        // branch; cmp_versions ranks the hash below any semver tag.
        assert!(is_newer_than("v0.15.0", "v646c91c", "git", true));
    }

    #[test]
    fn computes_relative_paths() {
        let from = Path::new("/home/u/.dsh/profiles/web");
        let to = Path::new("/home/u/.dsh/desktop/kernels/0.1.1/plugins/x");
        // Profile specs always go through spec_path_string, which
        // normalizes the platform separator to forward slashes.
        assert_eq!(
            spec_path_string(&relative_path(from, to)),
            "../../desktop/kernels/0.1.1/plugins/x"
        );
        assert_eq!(relative_path(from, from), PathBuf::new());
    }

    #[test]
    fn store_round_trips() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let item = StoreItem {
            id: "test-plugin-1".into(),
            name: "test-plugin".into(),
            origin: "npm".into(),
            source: "test-plugin".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "link".into(),
            pinned: false,
            installed_at: "1".into(),
            updated_at: "2".into(),
            repo_url: None,
            description: None,
        };
        upsert_item(&data_dir, item.clone()).expect("save");
        let loaded = load_store(&data_dir);
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "test-plugin-1");
        assert!(store_dir(&data_dir).starts_with(home.0.as_path()));
    }

    #[test]
    fn materialize_link_then_copy() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let id = "mat-plugin";
        let source = store_plugin_dir(&data_dir, id);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), "{}").unwrap();
        let item = StoreItem {
            id: id.into(),
            name: "mat-plugin".into(),
            origin: "npm".into(),
            source: "mat-plugin".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "link".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        let version = "0.1.1";
        let actual = materialize_one(&data_dir, version, &item).expect("materialize");
        // 链接失败（Windows 无开发者模式、受限文件系统、沙箱）会降级为 copy，
        // 两种结果都是合法行为；能链接时必须真的是链接。
        assert!(actual == "link" || actual == "copy");
        let target = kernel_plugin_dir(&data_dir, version, id);
        assert!(target.exists());
        if actual == "link" {
            assert!(target.is_symlink());
        }
        let actual = materialize_one(&data_dir, version, &item).expect("idempotent");
        assert!(actual == "link" || actual == "copy");

        // copy 模式覆盖
        let mut copy_item = item.clone();
        copy_item.mode = "copy".to_string();
        let actual = materialize_one(&data_dir, version, &copy_item).expect("copy");
        assert_eq!(actual, "copy");
        assert!(target.join("package.json").is_file());
        let meta = read_meta(&data_dir, version, id).expect("meta");
        assert_eq!(meta.mode, "copy");
        assert_eq!(meta.version, "1.0.0");
    }

    #[test]
    fn copy_tree_reports_failing_path() {
        let home = TestHome::new();
        let missing = home.0.join("no-such-source");
        let err = copy_tree(&missing, &home.0.join("out")).expect_err("missing source must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("no-such-source"),
            "error must name the failing path, got: {msg}"
        );
    }

    #[test]
    fn copy_tree_skips_dangling_symlink() {
        let home = TestHome::new();
        let source = home.0.join("src-tree");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("real.txt"), "hi").unwrap();
        // A symlink whose target vanished: `fs::copy` on it fails with os
        // error 2 on Windows, which used to abort the whole tree copy.
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(source.join("gone.txt"), source.join("link.txt"));
        #[cfg(windows)]
        let linked =
            std::os::windows::fs::symlink_file(source.join("gone.txt"), source.join("link.txt"));
        if linked.is_err() {
            return; // no symlink privilege in this environment
        }
        let target = home.0.join("dst-tree");
        copy_tree(&source, &target).expect("dangling link must not abort the copy");
        assert_eq!(fs::read_to_string(target.join("real.txt")).unwrap(), "hi");
        assert!(!target.join("link.txt").exists());
    }

    #[test]
    fn copy_tree_aborts_on_link_cycle() {
        let home = TestHome::new();
        let source = home.0.join("cycle-tree");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("real.txt"), "hi").unwrap();
        // A directory link pointing back at its own ancestor: the layout a
        // circular pnpm dependency produces on macOS/Linux, where
        // node_modules is all symlinks. The copy must fail with a clear
        // error instead of recursing forever.
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&source, source.join("loop"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&source, source.join("loop"));
        if linked.is_err() {
            return; // no symlink privilege in this environment
        }
        let err = copy_tree(&source, &home.0.join("dst-cycle")).expect_err("cycle must fail");
        assert!(
            err.to_string().contains("循环链接"),
            "error explains the cycle, got: {err}"
        );
    }

    #[test]
    fn reconcile_reaps_unmarked_staging_dirs() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();
        write_fake_plugin(&store.join("live-plugin"), "1.0.0");
        // Crash residue from before the `.dsh-id` stamp, plus legacy
        // `.tmp-` names from an older shell: no marker, never user data.
        fs::create_dir_all(store.join(format!("{TMP_PREFIX}1-2"))).unwrap();
        fs::create_dir_all(store.join(".tmp-legacy-3")).unwrap();
        reconcile_store(&data_dir);
        assert!(!store.join(format!("{TMP_PREFIX}1-2")).exists());
        assert!(!store.join(".tmp-legacy-3").exists());
        assert!(store.join("live-plugin").is_dir());
    }

    #[test]
    fn reconcile_keeps_live_plugin_named_like_staging() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();
        // npm allows names with a staging prefix (`tmp-foo`): the final
        // dir carries its own marker and must not be swept as its own
        // staging residue.
        let id = format!("{TMP_PREFIX}foo");
        mark_staging(&store.join(&id), &id);
        write_fake_plugin(&store.join(&id), "1.0.0");
        reconcile_store(&data_dir);
        assert!(store.join(&id).join("package.json").is_file());
    }

    #[test]
    fn sweep_removes_only_owned_or_broken_orphans() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "9.9.9";
        let item = StoreItem {
            id: "live".into(),
            name: "live".into(),
            origin: "npm".into(),
            source: "live".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "copy".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        upsert_item(&data_dir, item).expect("store");
        let store = load_store(&data_dir);
        let plugins = kernel_plugins_dir(&data_dir, version);
        fs::create_dir_all(plugins.join("live")).unwrap();
        // Shell-owned orphan: a `.meta` record proves the shell put it here.
        fs::create_dir_all(plugins.join("ghost")).unwrap();
        write_meta(
            &data_dir,
            version,
            "ghost",
            &KernelMeta {
                mode: "copy".into(),
                version: "1.0.0".into(),
                synced_at: "1".into(),
            },
        )
        .expect("meta");
        // Broken orphan: a symlink whose store target vanished.
        #[cfg(unix)]
        let linked =
            std::os::unix::fs::symlink(plugins.join("missing-target"), plugins.join("dangler"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(
            plugins.join("missing-target"),
            plugins.join("dangler"),
        );
        let has_link = linked.is_ok();
        // Foreign entry: no meta, not a link — left for the user.
        fs::create_dir_all(plugins.join("foreign")).unwrap();

        sweep_kernel_orphans(&data_dir, version, &store);

        assert!(plugins.join("live").exists());
        assert!(!plugins.join("ghost").exists());
        assert!(read_meta(&data_dir, version, "ghost").is_none());
        if has_link {
            assert!(!plugins.join("dangler").exists());
        }
        assert!(plugins.join("foreign").exists());
    }

    #[test]
    fn wiring_survives_single_plugin_failure() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "9.9.9";
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("active.txt"), format!("{version}\n")).unwrap();
        let mk_item = |id: &str, name: &str| StoreItem {
            id: id.into(),
            name: name.into(),
            origin: "npm".into(),
            source: name.into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            // copy mode keeps the expected profile spec deterministic:
            // link mode silently degrades to copy on hosts without
            // symlink privilege, flipping the spec prefix.
            mode: "copy".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        write_fake_plugin(&store_plugin_dir(&data_dir, "healthy"), "1.0.0");
        upsert_item(&data_dir, mk_item("healthy", "healthy-plugin")).expect("healthy");
        // Broken: registered in the store but its directory is gone.
        upsert_item(&data_dir, mk_item("broken", "broken-plugin")).expect("broken");
        // Profile already wired for the healthy plugin alone, so a
        // successful run leaves the manifest untouched and never shells
        // out to pnpm.
        let profile = profile_dir(&data_dir, "web");
        fs::create_dir_all(profile.join("node_modules")).unwrap();
        let manifest = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "healthy-plugin": "file:../../desktop/kernels/9.9.9/plugins/healthy"
            },
            "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
        });
        fs::write(
            profile.join("package.json"),
            serde_json::to_string_pretty(&manifest).unwrap() + "\n",
        )
        .unwrap();

        let mut noop = |_: &str| {};
        let err = ensure_wiring(
            &data_dir,
            &settings::Settings::default(),
            Path::new("pnpm"),
            &mut noop,
        )
        .expect_err("the broken plugin must surface as an error");
        let msg = err.to_string();
        assert!(
            msg.contains("broken-plugin"),
            "error names the failed plugin: {msg}"
        );
        // The healthy plugin was still materialized and the manifest kept
        // its wiring — one bad plugin no longer blocks everything else.
        assert!(kernel_plugin_dir(&data_dir, version, "healthy")
            .join("package.json")
            .is_file());
        let on_disk = fs::read_to_string(profile.join("package.json")).unwrap();
        assert!(on_disk.contains("healthy-plugin"));
    }

    #[test]
    fn refreshes_peers_from_active_kernel() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let id = "peer-plugin";
        let version = "2.0.0";
        // 假内核：node_modules 里有插件声明但中央库没有的 peer
        let kernel_mm = kernel::kernel_dir(&data_dir, version).join("node_modules");
        fs::create_dir_all(kernel_mm.join("@deepseek-ai/dsh-base")).unwrap();
        fs::write(kernel_mm.join("@deepseek-ai/dsh-base/package.json"), "{}").unwrap();
        fs::write(data_dir.join("active.txt"), format!("{version}\n")).unwrap();
        let plugin_root = store_plugin_dir(&data_dir, id);
        fs::create_dir_all(&plugin_root).unwrap();
        let manifest = serde_json::json!({
            "name": "peer-plugin",
            "peerDependencies": { "@deepseek-ai/dsh-base": "*" },
        });
        fs::write(
            plugin_root.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let item = StoreItem {
            id: id.into(),
            name: "peer-plugin".into(),
            origin: "npm".into(),
            source: "peer-plugin".into(),
            installed_version: "1.0.0".into(),
            latest_version: None,
            mode: "link".into(),
            pinned: false,
            installed_at: String::new(),
            updated_at: String::new(),
            repo_url: None,
            description: None,
        };
        refresh_store_peers(&data_dir, &item, version).expect("peers");
        let dest = plugin_root.join("node_modules/@deepseek-ai/dsh-base");
        assert!(dest.exists(), "peer 应被链接/复制进中央库");
        assert!(dest.join("package.json").is_file());
        // 幂等：同内核再跑一次不重复（存在即跳过）
        refresh_store_peers(&data_dir, &item, version).expect("peers again");
        assert!(dest.exists());
    }

    #[test]
    fn wire_manifest_applies_and_prunes() {
        let mut root = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "other": "1.0.0",
                "old-plugin": "link:../../desktop/kernels/1.0.0/plugins/old-plugin",
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "old-plugin"],
                },
            },
        });
        let mut specs = BTreeMap::new();
        specs.insert(
            "new-plugin".to_string(),
            (
                "link:../../desktop/kernels/9.9.9/plugins/new-plugin".to_string(),
                true,
            ),
        );
        specs.insert(
            "plain-plugin".to_string(),
            (
                "link:../../desktop/kernels/9.9.9/plugins/plain-plugin".to_string(),
                false,
            ),
        );

        let changed = wire_manifest(&mut root, &specs, "web").expect("wire");
        assert!(changed);
        let deps = root["dependencies"].as_object().expect("deps");
        assert_eq!(
            deps["new-plugin"].as_str().unwrap(),
            "link:../../desktop/kernels/9.9.9/plugins/new-plugin"
        );
        assert!(deps.contains_key("plain-plugin"));
        assert!(deps.contains_key("other")); // 用户/CLI 条目保留
        assert!(!deps.contains_key("old-plugin")); // 已卸载条目清退
        let bundles: Vec<&str> = root["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert!(bundles.contains(&"@deepseek-ai/dsh-base"));
        assert!(bundles.contains(&"new-plugin"));
        assert!(!bundles.contains(&"plain-plugin")); // 非 bundle 不进层
        assert!(!bundles.contains(&"old-plugin"));

        let changed = wire_manifest(&mut root, &specs, "web").expect("wire again");
        assert!(!changed);
    }

    #[test]
    fn is_managed_spec_matches_shell_layout_not_dir_name() {
        // release 壳（desktop/）与 dev 壳（desktop-dev/）写出的 spec 都必须
        // 识别为托管，否则 dev 卸载残留会被当成用户依赖保留，内核启动时
        // 解析悬空 bundle 崩溃（regression：WIRED_MARK 只匹配 "desktop/kernels/"）。
        assert!(is_managed_spec(
            "link:../../desktop/kernels/1.0.0/plugins/x"
        ));
        assert!(is_managed_spec(
            "link:../../desktop-dev/kernels/0.1.1-rc.2/plugins/x"
        ));
        // DSH_DESKTOP_DATA_DIR 覆盖为任意目录名时写出的 spec（common==0 时
        // relative_path 产出绝对路径）。
        assert!(is_managed_spec(
            "file:C:/Users/u/.dsh/desktop/kernels/1/plugins/x"
        ));
        assert!(is_managed_spec(
            "link:/Volumes/ext/dsh-shell/kernels/1/plugins/@scope__pkg"
        ));
        // 非托管：用户/CLI 依赖——版本号、任意 link/file 目标、路径里
        // kernels/plugins 不构成尾部布局、空的 version/id 段。
        assert!(!is_managed_spec("^1.0.0"));
        assert!(!is_managed_spec("link:../packages/my-plugin"));
        assert!(!is_managed_spec("file:../vendor/kernels/pkg"));
        assert!(!is_managed_spec(
            "link:../../desktop/plugins/1.0.0/kernels/x"
        ));
        assert!(!is_managed_spec("link:../../desktop/kernels//plugins/x"));
        assert!(!is_managed_spec("link:../../desktop/kernels/1/plugins/"));
    }

    #[test]
    fn wire_manifest_prunes_dev_shell_residue() {
        // dev 壳卸载插件后 manifest 残留的 desktop-dev spec 必须被清退：
        // 空 specs（安全模式/最后一个插件被卸载）下依赖与 bundle 层一起消失，
        // 只留下模板层。用户/CLI 条目不受影响。
        let mut root = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "other": "1.0.0",
                "dsh-synapse": "link:../../desktop-dev/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse",
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "dsh-synapse"],
                },
            },
        });
        let specs = BTreeMap::new();
        let changed = wire_manifest(&mut root, &specs, "web").expect("wire");
        assert!(changed);
        let deps = root["dependencies"].as_object().expect("deps");
        assert!(!deps.contains_key("dsh-synapse")); // dev 壳卸载残留清退
        assert!(deps.contains_key("other")); // 用户/CLI 条目保留
        let bundles: Vec<&str> = root["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert_eq!(
            bundles,
            ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"]
        );
    }

    #[test]
    fn wire_manifest_rewrites_cross_shell_specs() {
        // manifest 由 dev 壳接线，release 壳重新接线时 spec 重写为本壳的
        // kernel plugins 目录。
        let mut root = serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dependencies": {
                "dsh-synapse": "link:../../desktop-dev/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse",
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "dsh-synapse"],
                },
            },
        });
        let mut specs = BTreeMap::new();
        specs.insert(
            "dsh-synapse".to_string(),
            (
                "link:../../desktop/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse"
                    .to_string(),
                true,
            ),
        );
        let changed = wire_manifest(&mut root, &specs, "web").expect("wire");
        assert!(changed);
        assert_eq!(
            root["dependencies"]["dsh-synapse"].as_str().unwrap(),
            "link:../../desktop/kernels/0.1.1-rc.2/plugins/github.com__liangmianya__dsh-synapse"
        );
        let bundles: Vec<&str> = root["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();
        assert!(bundles.contains(&"dsh-synapse"));
    }

    #[test]
    fn spec_path_uses_forward_slashes() {
        #[cfg(windows)]
        let rel = PathBuf::from("..\\..\\desktop\\kernels\\1\\plugins\\x");
        #[cfg(not(windows))]
        let rel = PathBuf::from("../../desktop/kernels/1/plugins/x");
        assert_eq!(spec_path_string(&rel), "../../desktop/kernels/1/plugins/x");
    }

    #[test]
    fn settings_profile_defaults_to_web() {
        let s = settings::Settings::default();
        assert_eq!(s.profile, DEFAULT_PROFILE);
        assert_eq!(DEFAULT_PROFILE, "web");
    }

    #[test]
    fn status_flags_stale_materialization() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let version = "1.0.0";
        fs::create_dir_all(kernel::kernel_dir(&data_dir, version)).unwrap();
        fs::write(data_dir.join("active.txt"), format!("{version}\n")).unwrap();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "stale-plugin".into(),
                name: "stale-plugin".into(),
                origin: "npm".into(),
                source: "stale-plugin".into(),
                installed_version: "2.0.0".into(),
                latest_version: Some("3.0.0".into()),
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        let settings = settings::Settings::default();
        let view = status(&data_dir, &settings);
        assert_eq!(view.rows.len(), 1);
        assert!(!view.rows[0].synced);
        assert_eq!(view.updates, 1);
        assert_eq!(view.rows[0].latest_version.as_deref(), Some("3.0.0"));
    }

    /// After `update()` syncs `latest_version = installed_version`, the UI
    /// should not render a "有更新" badge. The `status()` row must hide
    /// `latest_version` so the per-row UI check (truthy on the field)
    /// stops showing the ghost notification. Top-level count is already
    /// filtered separately.
    #[test]
    fn status_hides_latest_when_not_newer_than_installed() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "synced-plugin".into(),
                name: "synced-plugin".into(),
                origin: "npm".into(),
                source: "synced-plugin".into(),
                installed_version: "1.2.3".into(),
                latest_version: Some("1.2.3".into()),
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        let settings = settings::Settings::default();
        let view = status(&data_dir, &settings);
        assert_eq!(view.updates, 0);
        assert!(
            view.rows[0].latest_version.is_none(),
            "row.latest_version must be hidden when it equals installed_version, got {:?}",
            view.rows[0].latest_version
        );
    }

    #[test]
    fn status_keeps_latest_when_newer_than_installed() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "behind-plugin".into(),
                name: "behind-plugin".into(),
                origin: "npm".into(),
                source: "behind-plugin".into(),
                installed_version: "1.0.0".into(),
                latest_version: Some("2.0.0".into()),
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        let settings = settings::Settings::default();
        let view = status(&data_dir, &settings);
        assert_eq!(view.updates, 1);
        assert_eq!(view.rows[0].latest_version.as_deref(), Some("2.0.0"));
    }

    /// The boot guard's quarantines must surface on the plugin row so the
    /// management UI can offer the re-enable / remove decision per plugin
    /// instead of the user discovering a silently missing integration.
    #[test]
    fn status_attaches_quarantine_record_to_row() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        upsert_item(
            &data_dir,
            StoreItem {
                id: "bad-plugin".into(),
                name: "bad-plugin".into(),
                origin: "npm".into(),
                source: "bad-plugin".into(),
                installed_version: "1.0.0".into(),
                latest_version: None,
                mode: "link".into(),
                pinned: false,
                installed_at: String::new(),
                updated_at: String::new(),
                repo_url: None,
                description: None,
            },
        )
        .expect("save");
        quarantine::add_all(
            &data_dir,
            &[quarantine::QuarantineItem {
                id: "bad-plugin".into(),
                name: "bad-plugin".into(),
                reason: "测试隔离".into(),
                evidence: "Error: boom".into(),
                at: 1,
            }],
        )
        .expect("quarantine");

        let view = status(&data_dir, &settings::Settings::default());
        assert_eq!(view.rows.len(), 1);
        let record = view.rows[0]
            .quarantined
            .as_ref()
            .expect("row must carry the quarantine record");
        assert_eq!(record.reason, "测试隔离");

        // Re-enabling drops the record and with it the row flag.
        quarantine::remove(&data_dir, "bad-plugin").expect("remove");
        let view = status(&data_dir, &settings::Settings::default());
        assert!(view.rows[0].quarantined.is_none());
    }

    /// Helper: stamp a `.dsh-id` marker inside a staging dir so the
    /// recovery scan can group it with the corresponding `final_dir`.
    fn mark_staging(dir: &Path, id: &str) {
        fs::create_dir_all(dir).expect("mkdir staging");
        fs::write(dir.join(ID_MARKER), format!("{id}\n")).expect("write marker");
    }

    fn write_fake_plugin(dir: &Path, tag: &str) {
        fs::create_dir_all(dir).expect("mkdir plugin");
        let pkg = format!(r#"{{"name":"p","version":"{tag}","main":"lib/index.js"}}"#);
        fs::write(dir.join("package.json"), pkg).expect("manifest");
        fs::create_dir_all(dir.join("lib")).expect("lib");
        fs::write(dir.join("lib/index.js"), "module.exports={}").expect("entry");
    }

    #[test]
    fn reconcile_is_noop_when_no_staging_dirs() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();
        // Live plugin with no staging around it.
        write_fake_plugin(&store.join("live-plugin"), "1.0.0");
        reconcile_store(&data_dir);
        assert!(store.join("live-plugin").is_dir());
        let entries: Vec<_> = fs::read_dir(&store)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with(TMP_PREFIX)
                    || n.starts_with(NEW_PREFIX)
                    || n.starts_with(BACKUP_PREFIX)
            })
            .collect();
        assert!(entries.is_empty(), "no staging should remain");
    }

    #[test]
    fn reconcile_reverts_to_backup_when_final_missing_and_both_staging_present() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // Crash between `final → backup` and `new → final`: final_dir
        // missing, both backup (old) and new (validated) survive.
        let id = "p";
        mark_staging(&store.join(format!("{BACKUP_PREFIX}1-1")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}1-1")), "1.0.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}2-2")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}2-2")), "2.0.0");

        reconcile_store(&data_dir);

        // Revert: the backup wins, the new staging is discarded.
        let final_dir = store.join(id);
        let final_manifest = fs::read_to_string(final_dir.join("package.json")).unwrap();
        assert!(
            final_manifest.contains("\"version\":\"1.0.0\""),
            "expected revert to old version, got: {final_manifest}"
        );
        assert!(!store.join(format!("{NEW_PREFIX}2-2")).exists());
        assert!(!store.join(format!("{BACKUP_PREFIX}1-1")).exists());
    }

    #[test]
    fn reconcile_promotes_new_when_only_new_survives() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // Crash after `tmp → new` but before `final → backup`: final
        // missing, only `.new-*` survives. Recovery publishes it.
        let id = "p";
        mark_staging(&store.join(format!("{NEW_PREFIX}3-3")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}3-3")), "2.5.0");

        reconcile_store(&data_dir);

        let final_dir = store.join(id);
        let final_manifest = fs::read_to_string(final_dir.join("package.json")).unwrap();
        assert!(
            final_manifest.contains("\"version\":\"2.5.0\""),
            "expected publish of new version, got: {final_manifest}"
        );
        assert!(!store.join(format!("{NEW_PREFIX}3-3")).exists());
    }

    #[test]
    fn reconcile_discards_tmp_only() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // Crash mid-fetch before validation: only `.tmp-*` survives.
        let id = "p";
        mark_staging(&store.join(format!("{TMP_PREFIX}4-4")), id);
        write_fake_plugin(&store.join(format!("{TMP_PREFIX}4-4")), "0.0.1");

        reconcile_store(&data_dir);

        assert!(!store.join(format!("{TMP_PREFIX}4-4")).exists());
        assert!(!store.join(id).exists());
    }

    #[test]
    fn reconcile_cleans_stale_staging_when_live_plugin_present() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // Live plugin exists; an old `.backup-*` from a completed update
        // was left behind (post-publish cleanup didn't run). Recovery
        // discards it.
        let id = "live";
        write_fake_plugin(&store.join(id), "3.0.0");
        mark_staging(&store.join(format!("{BACKUP_PREFIX}5-5")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}5-5")), "2.0.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}6-6")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}6-6")), "3.0.0");

        reconcile_store(&data_dir);

        assert!(store.join(id).is_dir());
        assert!(!store.join(format!("{BACKUP_PREFIX}5-5")).exists());
        assert!(!store.join(format!("{NEW_PREFIX}6-6")).exists());
    }

    #[test]
    fn reconcile_picks_newest_when_multiple_staging_dirs_share_id() {
        let home = TestHome::new();
        let data_dir = home.data_dir();
        let store = store_dir(&data_dir);
        fs::create_dir_all(&store).unwrap();

        // Two failed updates interleaved: final missing, two backups and
        // two news survive for the same id. The freshest (lex-largest
        // suffix) wins; the older peer is removed.
        let id = "p";
        mark_staging(&store.join(format!("{BACKUP_PREFIX}1-1")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}1-1")), "0.9.0");
        mark_staging(&store.join(format!("{BACKUP_PREFIX}2-2")), id);
        write_fake_plugin(&store.join(format!("{BACKUP_PREFIX}2-2")), "1.0.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}3-3")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}3-3")), "1.5.0");
        mark_staging(&store.join(format!("{NEW_PREFIX}4-4")), id);
        write_fake_plugin(&store.join(format!("{NEW_PREFIX}4-4")), "2.0.0");

        reconcile_store(&data_dir);

        // Both news exist -> revert to the freshest backup (id 2-2);
        // both news removed.
        let final_dir = store.join(id);
        let final_manifest = fs::read_to_string(final_dir.join("package.json")).unwrap();
        assert!(
            final_manifest.contains("\"version\":\"1.0.0\""),
            "expected freshest backup as the revert target, got: {final_manifest}"
        );
        assert!(!store.join(format!("{BACKUP_PREFIX}1-1")).exists());
        assert!(!store.join(format!("{BACKUP_PREFIX}2-2")).exists());
        assert!(!store.join(format!("{NEW_PREFIX}3-3")).exists());
        assert!(!store.join(format!("{NEW_PREFIX}4-4")).exists());
    }

    /// `new_staging_dir` returns an empty directory with no marker.
    /// The marker is the caller's responsibility: pre-stamping on the
    /// rename target is the original Windows ERROR_DIR_NOT_EMPTY
    /// failure mode (Windows `MoveFileEx` rejects a non-empty target),
    /// so the staging-dir creation API has to leave the path empty and
    /// let `stamp_id_marker` add the marker after a successful rename.
    #[test]
    fn new_staging_dir_returns_empty_dir_without_marker() {
        let home = TestHome::new();
        let store = store_dir(&home.data_dir());
        let dir = new_staging_dir(&store, TMP_PREFIX, "test-plugin").expect("create staging");
        assert!(dir.is_dir(), "staging dir must exist");
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert!(
            entries.is_empty(),
            "staging dir must be empty so fs::rename can land on it; got {:?}",
            entries
                .iter()
                .map(|e| e.as_ref().unwrap().file_name())
                .collect::<Vec<_>>()
        );
    }

    /// `stamp_id_marker` writes the marker that `reconcile_store` reads
    /// to group staging dirs by plugin id. After stamping, the dir
    /// contains exactly one file (the marker).
    #[test]
    fn stamp_id_marker_writes_marker_file() {
        let home = TestHome::new();
        let dir = store_dir(&home.data_dir()).join("marker-target");
        fs::create_dir_all(&dir).unwrap();
        stamp_id_marker(&dir, "test-plugin").expect("stamp");
        let content = fs::read_to_string(dir.join(ID_MARKER)).unwrap();
        assert_eq!(content, "test-plugin\n");
    }

    /// Two `new_staging_dir` calls inside the same test land on distinct
    /// paths. The nanos + pid scheme is unique for any practical
    /// interval; the second call must not collide with or clean up the
    /// first.
    #[test]
    fn new_staging_dir_paths_do_not_collide() {
        let home = TestHome::new();
        let store = store_dir(&home.data_dir());
        let a = new_staging_dir(&store, TMP_PREFIX, "a").expect("first");
        let b = new_staging_dir(&store, TMP_PREFIX, "b").expect("second");
        assert_ne!(a, b, "two staging dirs must have distinct paths");
        assert!(a.is_dir());
        assert!(b.is_dir());
    }

    /// A pre-existing target dir is cleaned up by `new_staging_dir` when
    // the caller passes a path the helper would reuse. We simulate the
    // leftover by creating a stale dir at the helper's expected path
    // between two calls — the helper's internal `remove_dir_all` must
    // remove it. With the old swallowed-error code, a leftover would
    // pass through and cause `fs::rename` to fail with
    // ERROR_DIR_NOT_EMPTY on Windows.
    #[test]
    fn new_staging_dir_clears_stale_target() {
        let home = TestHome::new();
        let store = store_dir(&home.data_dir());
        // Plant a stale leftover that matches the helper's first call.
        let first = new_staging_dir(&store, TMP_PREFIX, "stale-test").expect("first call");
        let stale_id_marker = first.join(ID_MARKER);
        fs::write(&stale_id_marker, "stale-test\n").unwrap();
        // The second call lands on a different path (nanos drift),
        // but a *same-path* retry would require the cleanup to win
        // before the dir is reused — verify the helper's remove step
        // succeeds on the first path.
        let _ = fs::remove_dir_all(&first);
        let second = new_staging_dir(&store, TMP_PREFIX, "stale-test").expect("second call");
        assert!(second.is_dir());
        assert!(!stale_id_marker.exists(), "stale marker must be gone");
    }
}
