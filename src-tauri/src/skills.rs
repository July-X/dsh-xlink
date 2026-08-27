//! Community skill management: a central store under the dsh home,
//! per-skill materialization into the kernel's user-level skill root, and
//! update checks.
//!
//! Skills are instruction data (a `SKILL.md` bundle or a flat Markdown file
//! with YAML frontmatter), not code: nothing to build, no profile wiring.
//! The kernel's `dsh-skill-filesystem` provider scans `<DSH_HOME>/skills`
//! (the user-dsh root) directly and watches it with chokidar, so a skill the
//! shell links into that root is discovered live by every installed kernel
//! version — no per-kernel materialization, no restart. Install unit is a
//! package (npm tarball, git repo, local folder); materialization and
//! enable/disable granularity is a single skill.
//!
//! Design notes: docs/skill-management.md in the desktop deliverable.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::process::quiet;
use crate::releases::http_get_string;
use crate::version::cmp_versions;

/// Central store directory name under the dsh home. Sits next to the
/// kernel-read `<home>/skills/` root but is shell-owned: disabled skills and
/// provenance markers must never be visible to kernel discovery.
const STORE_SUBDIR: &str = "skills-store";
/// The shell's inventory file inside the store directory.
const STORE_FILE: &str = "store.json";
/// Per-package fetch marker inside each store entry.
const SOURCE_MARKER: &str = ".dsh-source.json";
/// Maximum directory depth (relative to the package root, 0-based children =
/// depth 0) at which skill bundles are detected. Covers root-level bundles
/// plus common monorepo layouts (`skills/<name>/SKILL.md`) without walking
/// the whole tree.
const SCAN_MAX_DEPTH: usize = 3;
/// Spec prefix users can write to force local-folder parsing.
const LOCAL_PREFIX: &str = "local:";

// --- data model ------------------------------------------------------------

/// One skill discovered inside an installed package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    /// Frontmatter name, kebab-case; also the active-root entry name.
    pub name: String,
    pub description: String,
    /// Package-relative path to the bundle directory (`/` separators) or the
    /// flat `.md` file. Opaque to the UI; resolved against the package dir.
    pub path: String,
    /// Whether the skill is linked into the active root right now.
    pub enabled: bool,
}

/// One installed skill package in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStoreItem {
    /// Filesystem-safe package key (package/repo/folder name).
    pub id: String,
    /// Display name (npm package name, repo shorthand, or folder name).
    pub name: String,
    /// Fetch origin: npm, git, or local.
    pub origin: String,
    /// npm spec / git URL / absolute local folder path.
    pub source: String,
    pub installed_version: String,
    /// Latest known version, refreshed by check_updates; never set for local.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Desired materialization mode: link or copy.
    pub mode: String,
    /// Mode actually used on disk after fallback.
    pub actual_mode: String,
    /// Whether the source pins a version (npm @version / git #tag); always
    /// true for local folders.
    pub pinned: bool,
    /// Seconds since epoch, for display.
    pub installed_at: String,
    /// Seconds since epoch of the last fetch, for display.
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Skills discovered in this package at install/update time.
    pub skills: Vec<SkillEntry>,
}

/// The persisted store document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStore {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub items: Vec<SkillStoreItem>,
    #[serde(rename = "lastCheckedAt", skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    /// Last reconcile/materialize failure surfaced to the UI, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl Default for SkillStore {
    fn default() -> Self {
        Self {
            schema_version: 1,
            items: Vec::new(),
            last_checked_at: None,
            warning: None,
        }
    }
}

/// One skill line rendered inside a package row.
#[derive(Debug, Clone, Serialize)]
pub struct SkillEntryView {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Whether the expected entry actually exists in the active root.
    pub present: bool,
}

/// One row the management UI renders (one installed package).
#[derive(Debug, Clone, Serialize)]
pub struct SkillRow {
    pub id: String,
    pub name: String,
    pub origin: String,
    pub source: String,
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub pinned: bool,
    pub desired_mode: String,
    pub actual_mode: String,
    pub skills: Vec<SkillEntryView>,
    pub repo_url: Option<String>,
    pub description: Option<String>,
    pub installed_at: String,
    pub updated_at: String,
}

/// Aggregate skill status for the management UI.
#[derive(Debug, Clone, Serialize)]
pub struct SkillStatus {
    pub rows: Vec<SkillRow>,
    /// Display path of the kernel-read user skill root.
    pub skills_root: String,
    /// Number of packages with a known newer version.
    pub updates: usize,
    pub last_checked_at: Option<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SkillSpec {
    pub origin: String,
    /// npm package name, git URL, or absolute local folder path.
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

/// A parsed update-check result for one package.
#[derive(Debug, Clone, Serialize)]
pub struct SkillUpdateInfo {
    pub id: String,
    pub latest: Option<String>,
    pub error: Option<String>,
}

// --- paths ------------------------------------------------------------------

/// The user's OS home directory (`$HOME` on Unix, `%USERPROFILE%` on
/// Windows), mirroring kernel.rs's own fallback chain.
fn os_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Expand a leading `~` component to the OS home. `DSH_HOME="~/…"` stays
/// supported, matching the kernel's tilde expansion in `resolveDshHome`.
fn expand_tilde(path: &Path) -> PathBuf {
    let Some(Component::Normal(first)) = path.components().next() else {
        return path.to_path_buf();
    };
    if first != "~" {
        return path.to_path_buf();
    }
    let rest = path.strip_prefix("~").unwrap_or(path);
    os_home().join(rest)
}

/// The dsh home the spawned kernel resolves on its own (`DSH_HOME` env or
/// `~/.dsh`). The shell writes exactly the directory the kernel reads, so
/// this mirrors the kernel's resolution order instead of deriving from the
/// shell data dir (which `DSH_DESKTOP_DATA_DIR` can relocate elsewhere).
fn resolve_home() -> PathBuf {
    expand_tilde(
        &std::env::var_os("DSH_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| os_home().join(".dsh")),
    )
}

/// Central store root: `<home>/skills-store/`.
pub fn store_dir(home: &Path) -> PathBuf {
    home.join(STORE_SUBDIR)
}

fn store_file(home: &Path) -> PathBuf {
    store_dir(home).join(STORE_FILE)
}

fn store_pkg_dir(home: &Path, id: &str) -> PathBuf {
    store_dir(home).join(id)
}

/// The kernel-read user skill root (`<home>/skills/`, user-dsh rank 400):
/// the single materialization target shared by every installed kernel.
pub fn skills_root(home: &Path) -> PathBuf {
    home.join("skills")
}

/// Active-root entry for one skill: bundle dirs link under their frontmatter
/// name; flat files become `<name>.md` so the entry reads like the skill.
fn skill_target_path(home: &Path, entry: &SkillEntry) -> PathBuf {
    if entry.path.ends_with(".md") {
        skills_root(home).join(format!("{}.md", entry.name))
    } else {
        skills_root(home).join(&entry.name)
    }
}

/// Map a package/repo/folder name to a filesystem-safe store id. Same rules
/// as the plugin store: slashes become double underscores, dot / empty
/// segments are rejected outright.
fn id_for_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::Skill("技能包名称为空或过长".into()));
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(AppError::Skill(format!(
                "非法的技能包名称 {name:?}（包含空段或 ..）"
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

pub fn load_store(home: &Path) -> SkillStore {
    let Ok(text) = fs::read_to_string(store_file(home)) else {
        return SkillStore::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_store(home: &Path, store: &SkillStore) -> Result<(), AppError> {
    fs::create_dir_all(store_dir(home)).map_err(|e| AppError::Io(e.to_string()))?;
    let text = serde_json::to_string_pretty(store).map_err(|e| AppError::Io(e.to_string()))?;
    fs::write(store_file(home), text + "\n").map_err(|e| AppError::Io(e.to_string()))
}

fn store_item(home: &Path, id: &str) -> Option<SkillStoreItem> {
    load_store(home)
        .items
        .into_iter()
        .find(|item| item.id == id)
}

fn upsert_item(home: &Path, item: SkillStoreItem) -> Result<(), AppError> {
    let mut store = load_store(home);
    if let Some(existing) = store.items.iter_mut().find(|i| i.id == item.id) {
        *existing = item;
    } else {
        store.items.push(item);
    }
    save_store(home, &store)
}

fn remove_item(home: &Path, id: &str) -> Result<(), AppError> {
    let mut store = load_store(home);
    store.items.retain(|item| item.id != id);
    save_store(home, &store)
}

// --- spec parsing -----------------------------------------------------------

/// Split an npm spec into (name, optional pin). Same rules as the plugin
/// store: the last @ after the scope prefix separates the version.
fn split_npm_spec(spec: &str) -> Result<(String, Option<String>), AppError> {
    let s = spec.trim();
    if s.starts_with('@') {
        let (head, rest) = s
            .split_once('/')
            .ok_or_else(|| AppError::Skill(format!("非法的 npm 包名 {spec:?}")))?;
        let rest = rest.trim();
        let (name, pin) = match rest.rsplit_once('@') {
            Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
                (n, Some(p.to_string()))
            }
            _ => (rest, None),
        };
        return Ok((format!("{head}/{name}"), pin));
    }
    match s.rsplit_once('@') {
        Some((n, p)) if !n.is_empty() && !p.is_empty() && !p.contains('/') => {
            Ok((n.to_string(), Some(p.to_string())))
        }
        _ => Ok((s.to_string(), None)),
    }
}

/// Whether the input names a local folder: explicit prefix, tilde form,
/// absolute POSIX path, `.`-relative path, or a Windows drive-letter path.
/// The `\\?\` verbatim prefix is recognized too: `parse_local_spec` stores
/// the canonicalized path as the package source, and `canonicalize` on
/// Windows returns verbatim paths — without this arm the 「更新」 re-parse
/// of a local package falls through to the npm-name validation and fails.
fn looks_like_local(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with(LOCAL_PREFIX)
        || s.starts_with("~/")
        || s.starts_with('/')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with(r"\\?\")
        || (s.len() >= 3
            && s.as_bytes()[1] == b':'
            && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/'))
}

/// Parse an install request into a SkillSpec. Accepts npm package names
/// (with optional @version), git URLs (https, git@, or owner/repo shorthand,
/// with optional #tag), and local folder paths (`local:` prefix, `~/…`,
/// absolute, or Windows drive paths).
pub fn parse_spec(raw: &str) -> Result<SkillSpec, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 500 {
        return Err(AppError::Skill("安装地址为空或过长".into()));
    }
    if looks_like_local(trimmed) {
        return parse_local_spec(trimmed);
    }
    let s = trimmed.trim_end_matches('/');
    if s.starts_with("git@") || s.contains("://") || s.contains("github.com/") {
        let (url, pin) = match s.split_once('#') {
            Some((u, tag)) if !u.is_empty() && !tag.is_empty() => (u, Some(tag.to_string())),
            _ => (s, None),
        };
        let repo_url = s.contains("github.com/").then(|| url.to_string());
        // URL 含协议双斜杠等空路径段，先归一成 owner/repo 形状再映射 id。
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
        return Ok(SkillSpec {
            origin: "git".into(),
            source: url.to_string(),
            pin,
            id,
            name,
            repo_url,
        });
    }
    // owner/repo 简写：非 npm 样式（不含 @ 且含斜杠）按 GitHub 仓库处理。
    if s.contains('/') && !s.starts_with('@') {
        let id = id_for_name(s)?;
        return Ok(SkillSpec {
            origin: "git".into(),
            source: format!("https://github.com/{s}.git"),
            pin: None,
            id,
            name: s.rsplit('/').next().unwrap_or(s).to_string(),
            repo_url: Some(format!("https://github.com/{s}")),
        });
    }
    let (name, pin) = split_npm_spec(s)?;
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._@/".contains(c))
    {
        return Err(AppError::Skill(format!("非法的 npm 包名 {raw:?}")));
    }
    let id = id_for_name(&name)?;
    Ok(SkillSpec {
        origin: "npm".into(),
        source: name.clone(),
        pin,
        id,
        name,
        repo_url: None,
    })
}

fn parse_local_spec(raw: &str) -> Result<SkillSpec, AppError> {
    let expanded = expand_tilde(Path::new(
        raw.strip_prefix(LOCAL_PREFIX)
            .unwrap_or(raw)
            .trim()
            .trim_matches('"'),
    ));
    let path = expanded.canonicalize().map_err(|e| {
        AppError::Skill(format!(
            "本地文件夹不存在或不可读：{}（{e}）",
            expanded.display()
        ))
    })?;
    if !path.is_dir() {
        return Err(AppError::Skill(format!(
            "本地来源必须是文件夹：{}",
            path.display()
        )));
    }
    let home = resolve_home();
    for guarded in [store_dir(&home), skills_root(&home)] {
        if path.starts_with(&guarded) {
            return Err(AppError::Skill(format!(
                "本地来源不能位于外壳管理的目录内：{}",
                guarded.display()
            )));
        }
    }
    let folder = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::Skill("无法从路径解析文件夹名".into()))?;
    Ok(SkillSpec {
        origin: "local".into(),
        source: path.to_string_lossy().into_owned(),
        pin: None,
        id: id_for_name(folder)?,
        name: folder.to_string(),
        repo_url: None,
    })
}

// --- frontmatter ------------------------------------------------------------

/// Whether `name` matches the kernel's kebab-case skill-name rule
/// (`^[a-z0-9]+(?:-[a-z0-9]+)*$`).
fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

/// Extract `(name, description)` from a skill file's leading YAML
/// frontmatter. A deliberate top-level subset: the kernel parses full YAML,
/// but installing something the shell cannot verify would put an invisible
/// skill in front of the user, so unparseable frontmatter rejects the
/// candidate instead of trusting it.
fn parse_skill_markdown(text: &str) -> Option<(String, String)> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for line in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == "---" || line == "..." {
            break;
        }
        // Top-level keys only: skip nested mappings, lists, block scalars.
        if line.starts_with(' ') || line.starts_with('\t') || line.starts_with('-') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = strip_quotes(value.trim());
        match key.trim() {
            "name" if name.is_none() => name = Some(value.to_string()),
            "description" if description.is_none() => description = Some(value.to_string()),
            _ => {}
        }
    }
    let name = name?;
    let description = description?;
    (!name.is_empty() && !description.is_empty()).then_some((name, description))
}

fn strip_quotes(value: &str) -> &str {
    for q in ['"', '\''] {
        if value.starts_with(q) && value.ends_with(q) && value.len() >= 2 {
            return &value[1..value.len() - 1];
        }
    }
    value
}

// --- package scanning -------------------------------------------------------

/// One validated skill found inside a package.
#[derive(Debug, Clone)]
struct ScannedSkill {
    name: String,
    description: String,
    /// Package-relative path with `/` separators, pointing at the bundle
    /// directory (containing SKILL.md) or the flat `.md` file.
    rel: String,
}

/// Scan a fetched package for skills. Directories containing SKILL.md are
/// bundle candidates down to [`SCAN_MAX_DEPTH`] levels; root-level flat
/// `*.md` files are candidates too. Candidates the kernel would ignore
/// (missing/invalid frontmatter, non-kebab names) surface through `warn`
/// and are skipped; a scan yielding zero usable skills is an error.
fn scan_package_skills(
    pkg: &Path,
    warn: &mut dyn FnMut(&str),
) -> Result<Vec<ScannedSkill>, AppError> {
    let mut found = Vec::new();
    walk_package(pkg, pkg, 0, &mut found, warn)
        .map_err(|e| AppError::Io(format!("扫描技能包失败：{e}")))?;
    if found.is_empty() {
        return Err(AppError::Skill(
            "包内没有发现可用技能：需要 <名称>/SKILL.md 目录（≤3 层深）或顶层带 \
             name/description frontmatter 的 <名称>.md 文件"
                .into(),
        ));
    }
    // Duplicate frontmatter names inside one package are ambiguous at
    // materialization time (one active-root entry per name) — reject loudly.
    let mut seen = std::collections::BTreeSet::new();
    for skill in &found {
        if !seen.insert(skill.name.clone()) {
            return Err(AppError::Skill(format!(
                "包内存在重名技能 {:?}（frontmatter name 冲突），请修正上游内容后重试",
                skill.name
            )));
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

fn walk_package(
    root: &Path,
    dir: &Path,
    depth: usize,
    found: &mut Vec<ScannedSkill>,
    warn: &mut dyn FnMut(&str),
) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') || file_name == "node_modules" {
            continue;
        }
        // Skip symlinks entirely: a skill bundle is a real on-disk tree, so
        // the link/copy target the shell later materializes is the entry itself.
        // A symlink inside the package (e.g. blader/humanizer's
        // `skills/humanizer/SKILL.md` → `../../SKILL.md`, added in v2.11.1
        // for Claude Desktop ZIP uploads) would otherwise double-count the
        // same skill. Following symlinks could also escape the scan-depth
        // limit and land outside the package root.
        if fs::symlink_metadata(&path)
            .map(|md| md.file_type().is_symlink())
            .unwrap_or(false)
        {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            // Symlink-aware: a directory containing a symlinked SKILL.md is
            // not a bundle even though `is_file()` would follow the link.
            let bundle = fs::symlink_metadata(&skill_md)
                .map(|md| md.is_file() && !md.file_type().is_symlink())
                .unwrap_or(false);
            if bundle {
                match fs::read_to_string(&skill_md) {
                    Ok(text) => match parse_skill_markdown(&text) {
                        Some((name, description)) if is_kebab_case(&name) => {
                            found.push(ScannedSkill {
                                name,
                                description,
                                rel,
                            });
                        }
                        Some((name, _)) => warn(&format!(
                            "跳过 {rel}：frontmatter name {name:?} 不符合 kebab-case 规范，内核也会忽略它"
                        )),
                        None => warn(&format!(
                            "跳过 {rel}：SKILL.md 缺少 name/description frontmatter，内核也会忽略它"
                        )),
                    },
                    Err(e) => warn(&format!("跳过 {rel}：SKILL.md 不可读（{e}）")),
                }
                // A bundle owns its subtree; do not descend further.
                continue;
            }
            if depth + 1 < SCAN_MAX_DEPTH {
                walk_package(root, &path, depth + 1, found, warn)?;
            }
        } else if depth == 0 && file_name.to_ascii_lowercase().ends_with(".md") {
            // Flat candidate. Root-level README.md etc. carry no skill
            // frontmatter and the kernel ignores them too — skip silently.
            if let Ok(text) = fs::read_to_string(&path) {
                if let Some((name, description)) = parse_skill_markdown(&text) {
                    if is_kebab_case(&name) {
                        found.push(ScannedSkill {
                            name,
                            description,
                            rel,
                        });
                    } else {
                        warn(&format!(
                            "跳过 {rel}：frontmatter name {name:?} 不符合 kebab-case 规范"
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

// --- fetching ---------------------------------------------------------------

/// Run one command, collecting stdout for quick helpers (git ls-remote).
/// Goes through `process::command_with_path` so the GUI shell's inherited
/// PATH includes the user's tool locations.
fn run_capture(program: &str, args: &[&str]) -> io::Result<(bool, String)> {
    let mut cmd = crate::process::command_with_path(program);
    cmd.args(args);
    let output = quiet(&mut cmd).output()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((output.status.success(), text))
}

/// Highest semver-shaped tag among candidates, or None.
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

/// Whether a stored version string looks like semver rather than a short
/// hash; see plugins.rs `looks_like_semver` for why the comparison splits
/// on shape first.
fn looks_like_semver(version: &str) -> bool {
    let stripped = version.strip_prefix('v').unwrap_or(version);
    let head = stripped.split_once('-').map(|(h, _)| h).unwrap_or(stripped);
    let parts: Vec<&str> = head.split('.').collect();
    parts.len() >= 2 && parts[..2].iter().all(|seg| seg.parse::<u64>().is_ok())
}

fn is_newer_than(latest: &str, installed: &str, origin: &str, pinned: bool) -> bool {
    if origin == "git" && !pinned && !looks_like_semver(installed) {
        if looks_like_semver(latest) {
            false
        } else {
            latest != installed
        }
    } else {
        cmp_versions(latest, installed) == std::cmp::Ordering::Greater
    }
}

/// npm registry document slice needed for fetch + update checks.
#[derive(Debug, Deserialize)]
struct NpmDoc {
    #[serde(rename = "dist-tags", default)]
    dist_tags: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    versions: std::collections::BTreeMap<String, NpmVersionDoc>,
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

fn fetch_npm_doc(name: &str) -> Result<NpmDoc, String> {
    let url = format!("{}{}", crate::registry::npm_registry_base(), name);
    let body = http_get_string(&url, None)?;
    serde_json::from_str(&body).map_err(|e: serde_json::Error| e.to_string())
}

/// Extract a tgz into dest, stripping the leading package/ segment. System
/// tar via `process::command_with_path`; the stderr excerpt surfaces real
/// causes (corrupt archive, permission denied, MAX_PATH overrun).
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
        let detail = String::from_utf8_lossy(&output.stderr)
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        return Err(format!(
            "tar 解包失败（退出码 {:?}）{}",
            output.status.code(),
            if detail.is_empty() {
                String::new()
            } else {
                format!("：{detail}")
            }
        ));
    }
    Ok(())
}

fn write_source_marker(spec: &SkillSpec, version: &str, dest: &Path) -> Result<(), AppError> {
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

/// Recursively copy source into target, replacing whatever exists.
fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    if target.is_symlink() {
        remove_link(target);
    } else if target.exists() {
        let _ = fs::remove_dir_all(target);
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            let _ = fs::remove_file(&to);
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Prefixes for staged fetch dirs, mirroring the plugin store's crash-safe
/// swap vocabulary (`.tmp-*` in flight, `.new-*` validated, `.backup-*` the
/// previous live tree during a swap). The leading dot keeps staging dirs out
/// of every scanner and file listing.
const TMP_PREFIX: &str = ".tmp-";
const NEW_PREFIX: &str = ".new-";
const BACKUP_PREFIX: &str = ".backup-";
/// Marker naming the owning package id inside a staging dir.
const ID_MARKER: &str = ".dsh-id";

fn new_staging_dir(store: &Path, kind: &str) -> io::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = store.join(format!("{kind}{}-{nanos}", std::process::id()));
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) if !dir.exists() => {}
        Err(e) => return Err(e),
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn stamp_id_marker(dir: &Path, id: &str) -> io::Result<()> {
    fs::write(dir.join(ID_MARKER), format!("{id}\n"))
}

/// Outcome of one successful fetch-and-publish cycle.
struct FetchedPackage {
    /// Published store dir for the package.
    dir: PathBuf,
    /// Version string recorded in the source marker (npm semver, git tag or
    /// HEAD hash, `local` for folder imports).
    version: String,
    /// Validated skills discovered inside the package.
    skills: Vec<ScannedSkill>,
}

/// Fetch one package into the store under a staged tmp dir, scan it for
/// skills, then publish over the final dir with the crash-safe three-stage
/// rename the plugin store established. A crash at any step leaves the live
/// package recoverable by [`reconcile`].
fn fetch_into_store(
    home: &Path,
    spec: &SkillSpec,
    on_progress: &mut dyn FnMut(&str),
) -> Result<FetchedPackage, AppError> {
    let store = store_dir(home);
    fs::create_dir_all(&store).map_err(|e| AppError::Io(e.to_string()))?;
    let tmp = new_staging_dir(&store, TMP_PREFIX).map_err(|e| AppError::Io(e.to_string()))?;
    // Stamping the `.dsh-id` marker must wait until AFTER the fetch_* call
    // returns: `git clone` requires an empty destination dir and aborts with
    // "destination path '...' already exists and is not an empty directory"
    // when the marker file is present at fetch time; `npm` tarball extraction
    // and `local` copy would happily overwrite the marker anyway, so
    // stamping pre-fetch only ever worked for npm by accident. The marker
    // travels with the contents when we rename `tmp → new`, so stamp after
    // fetch and let the rename propagate it into the new staging path.

    let version = match spec.origin.as_str() {
        "npm" => fetch_npm(spec, &tmp, on_progress),
        "git" => fetch_git(spec, &tmp, on_progress),
        "local" => fetch_local(spec, &tmp, on_progress),
        other => Err(AppError::Skill(format!("未知来源 {other:?}"))),
    };
    let version = match version {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    stamp_id_marker(&tmp, &spec.id).map_err(|e| AppError::Io(e.to_string()))?;

    on_progress("正在扫描包内技能并校验 frontmatter");
    let mut warnings: Vec<String> = Vec::new();
    let scanned = match scan_package_skills(&tmp, &mut |m| warnings.push(m.to_string())) {
        Ok(scanned) => scanned,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    for w in &warnings {
        on_progress(w);
    }

    let new = new_staging_dir(&store, NEW_PREFIX).map_err(|e| AppError::Io(e.to_string()))?;
    if let Err(e) = fs::rename(&tmp, &new) {
        // tmp still holds the scanned content; leave it for reconcile.
        return Err(AppError::Io(format!("将暂存目录提升到 .new-* 失败：{e}")));
    }

    let final_dir = store_pkg_dir(home, &spec.id);
    let backup = new_staging_dir(&store, BACKUP_PREFIX).map_err(|e| AppError::Io(e.to_string()))?;
    if final_dir.exists() {
        if let Err(e) = fs::rename(&final_dir, &backup) {
            // Roll forward: promote the validated tree rather than strand it.
            if fs::rename(&new, &final_dir).is_err() {
                let _ = fs::remove_dir_all(&new);
                return Err(AppError::Io(format!("备份旧版本失败且无法发布新版本：{e}")));
            }
            return Err(AppError::Io(format!(
                "技能包已发布，但备份旧版本失败（{e}）；下次更新若失败将无法回滚"
            )));
        }
        if let Err(e) = stamp_id_marker(&backup, &spec.id) {
            eprintln!(
                "dsh-desktop: warning, could not stamp id marker on backup of {}: {e}",
                spec.id
            );
        }
    }

    if let Err(e) = fs::rename(&new, &final_dir) {
        if fs::rename(&backup, &final_dir).is_err() {
            return Err(AppError::Io(format!("发布新版本失败且回滚旧版本失败：{e}")));
        }
        return Err(AppError::Io(format!("发布新版本失败，已回滚到旧版本：{e}")));
    }
    if backup.exists() {
        if let Err(e) = fs::remove_dir_all(&backup) {
            on_progress(&format!(
                "注意：清理备份目录失败（{e}），下次启动时 reconcile 会接手"
            ));
        }
    }

    write_source_marker(spec, &version, &final_dir)?;
    on_progress(&format!(
        "发现 {} 个技能：{}",
        scanned.len(),
        scanned
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join("、")
    ));
    Ok(FetchedPackage {
        dir: final_dir,
        version,
        skills: scanned,
    })
}

fn fetch_npm(
    spec: &SkillSpec,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    on_progress(&format!("正在查询 npm registry：{}", spec.source));
    let doc =
        fetch_npm_doc(&spec.source).map_err(|e| AppError::Skill(format!("查询 npm 失败：{e}")))?;
    let version = spec
        .pin
        .clone()
        .unwrap_or_else(|| doc.dist_tags.get("latest").cloned().unwrap_or_default());
    if version.is_empty() {
        return Err(AppError::Skill(format!(
            "npm 上找不到包 {} 或其 latest 标记",
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
            AppError::Skill(format!(
                "npm 上 {}@{version} 没有可下载的 tarball",
                spec.source
            ))
        })?;
    on_progress(&format!("正在下载 {}@{version} …", spec.source));
    let bytes = crate::releases::http_get_bytes(&tarball)
        .map_err(|e| AppError::Skill(format!("下载失败：{e}")))?;
    let tgz = dest.join(".pkg.tgz");
    fs::write(&tgz, bytes).map_err(|e| AppError::Io(e.to_string()))?;
    // npm tarballs carry a leading package/ segment that --strip-components=1
    // removes, landing the manifest at dest where the scanner expects it.
    extract_tarball(&tgz, dest)
        .map_err(|e| AppError::Skill(format!("解包失败：{e}（请确认系统存在 tar）")))?;
    let _ = fs::remove_file(&tgz);
    Ok(version)
}

fn fetch_git(
    spec: &SkillSpec,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    let mut probe = crate::process::command_with_path("git");
    probe.arg("--version");
    if quiet(&mut probe).output().is_err() {
        return Err(AppError::Skill(
            "未找到 git（git 来源的技能包需要 git；请先安装 git）".into(),
        ));
    }
    // Pinned specs use their tag directly; unpinned repos install the highest
    // semver tag so `installed_version` stays comparable by check_updates.
    // Repos without any semver tag fall back to the default branch (HEAD hash).
    let branch = match spec.pin.as_ref() {
        Some(tag) => Some(tag.clone()),
        None => match git_latest_tag(&spec.source) {
            Ok(Some(tag)) => Some(tag),
            Ok(None) => None,
            Err(e) => return Err(AppError::Skill(format!("查询最新 tag 失败：{e}"))),
        },
    };

    on_progress(&format!("正在克隆 {}", spec.source));
    let mut cmd = crate::process::command_with_path("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(tag) = &branch {
        cmd.arg("--branch").arg(tag);
    }
    let status = quiet(&mut cmd)
        .arg(&spec.source)
        .arg(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| AppError::Io(format!("无法运行 git：{e}")))?;
    if !status.success() {
        return Err(AppError::Skill(format!(
            "git clone 失败（退出码 {:?}），请检查地址与网络",
            status.code()
        )));
    }
    if let Some(tag) = branch {
        return Ok(tag);
    }
    let dest_str = dest.to_str().unwrap_or("");
    let (ok, out) = run_capture("git", &["-C", dest_str, "rev-parse", "--short", "HEAD"])
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(if ok {
        out.trim().to_string()
    } else {
        String::from("head")
    })
}

fn fetch_local(
    spec: &SkillSpec,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    on_progress(&format!("正在复制本地文件夹 {}", spec.source));
    let source = Path::new(&spec.source);
    if let Err(e) = copy_tree(source, dest) {
        let _ = fs::remove_dir_all(dest);
        return Err(AppError::Skill(format!("复制本地文件夹失败：{e}")));
    }
    // A copied-in .git tree serves nobody here and bloats the store.
    let _ = fs::remove_dir_all(dest.join(".git"));
    Ok(String::from("local"))
}

// --- materialization --------------------------------------------------------

#[cfg(unix)]
fn make_entry_link(source: &Path, target: &Path, _is_file: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn make_entry_link(source: &Path, target: &Path, is_file: bool) -> io::Result<()> {
    if is_file {
        std::os::windows::fs::symlink_file(source, target)
    } else {
        std::os::windows::fs::symlink_dir(source, target)
    }
}

/// Resolve one store-side link source: expand a symlinked store entry to its
/// real location so active-root links stay direct (no double-symlink chain).
fn resolved_source(path: &Path) -> PathBuf {
    fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.file_type().is_symlink())
        .and_then(|_| fs::read_link(path).ok())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Remove a filesystem link without touching its target. On Windows
/// `DeleteFile` rejects directory symlinks (ERROR_ACCESS_DENIED) — only
/// `RemoveDirectory` removes them — while file symlinks need `DeleteFile`;
/// trying both covers either kind on every platform.
fn remove_link(path: &Path) {
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}

/// Remove one active-root entry whatever it is (link, dir, or file).
fn remove_target(target: &Path) {
    match fs::symlink_metadata(target) {
        Ok(md) if md.file_type().is_symlink() => remove_link(target),
        Ok(md) if md.is_dir() => {
            let _ = fs::remove_dir_all(target);
        }
        Ok(_) => {
            let _ = fs::remove_file(target);
        }
        Err(_) => {}
    }
}

/// Link (or copy) one skill into the active root under its frontmatter name.
///
/// An entry already pointing at this exact source short-circuits (idempotent
/// re-runs). Otherwise an occupied entry errors out unless `replace_owned`,
/// which callers with inventory authority over the name (update refresh,
/// reconcile repair) set to replace stale content from a previous version.
/// Returns the actual mode used after any link→copy fallback.
fn ensure_entry(
    home: &Path,
    pkg_dir: &Path,
    mode: &str,
    entry: &SkillEntry,
    replace_owned: bool,
) -> Result<String, AppError> {
    let target = skill_target_path(home, entry);
    let source = resolved_source(&pkg_dir.join(&entry.path));
    if !source.exists() {
        return Err(AppError::Skill(format!(
            "技能 {} 的源路径在中央库中不存在：{}",
            entry.name,
            source.display()
        )));
    }
    if let Ok(md) = fs::symlink_metadata(&target) {
        let ours = md
            .file_type()
            .is_symlink()
            .then(|| fs::read_link(&target).ok())
            .flatten()
            .map(|link| link == source)
            .unwrap_or(false);
        if ours {
            return Ok(mode.to_string());
        }
        if !replace_owned {
            return Err(AppError::Skill(format!(
                "技能名冲突：活动根中已存在同名条目 {} 且不来自当前技能包；可能来自其他技能包或手动放置，请先处理该条目",
                target.display()
            )));
        }
        remove_target(&target);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let is_file = entry.path.ends_with(".md");
    let mut actual = mode.to_string();
    if mode == "link" && make_entry_link(&source, &target, is_file).is_err() {
        actual = String::from("copy");
        eprintln!(
            "dsh-desktop: link failed for skill {}; falling back to copy",
            entry.name
        );
    }
    if actual == "copy" {
        if is_file {
            fs::copy(&source, &target)
                .map_err(|e| AppError::Io(format!("复制技能文件失败：{e}")))?;
        } else {
            copy_tree(&source, &target)
                .map_err(|e| AppError::Io(format!("复制技能目录失败：{e}")))?;
        }
    }
    Ok(actual)
}

/// Remove one skill's active-root entry, but only when it still is the
/// store-owned link; a replaced or user-recreated entry is left alone.
fn unmaterialize_entry(home: &Path, pkg_dir: &Path, entry: &SkillEntry) {
    let target = skill_target_path(home, entry);
    let source = resolved_source(&pkg_dir.join(&entry.path));
    let owned = fs::symlink_metadata(&target)
        .ok()
        .filter(|m| m.file_type().is_symlink())
        .and_then(|_| fs::read_link(&target).ok())
        .map(|link| link == source)
        .unwrap_or(false);
    if owned {
        remove_target(&target);
    }
}

// --- orchestration ----------------------------------------------------------

/// Install a skill package: fetch into the store, scan and validate its
/// skills, publish, then materialize every skill into the active root.
/// The kernel's watcher picks the new entries up live — no restart.
pub fn install(
    spec_str: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<SkillStoreItem, AppError> {
    install_into(&resolve_home(), spec_str, mode, on_progress)
}

fn install_into(
    home: &Path,
    spec_str: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<SkillStoreItem, AppError> {
    let spec = parse_spec(spec_str)?;
    if store_item(home, &spec.id).is_some() {
        return Err(AppError::Skill(format!(
            "{} 已安装，请使用「更新」",
            spec.name
        )));
    }
    let desired_mode = if mode == "copy" { "copy" } else { "link" }.to_string();
    let now = now_epoch_secs();
    let mut item = SkillStoreItem {
        id: spec.id.clone(),
        name: spec.name.clone(),
        origin: spec.origin.clone(),
        source: spec.source.clone(),
        installed_version: String::new(),
        latest_version: None,
        mode: desired_mode.clone(),
        actual_mode: desired_mode.clone(),
        pinned: spec.pin.is_some() || spec.origin == "local",
        installed_at: now.clone(),
        updated_at: now,
        repo_url: spec.repo_url.clone(),
        description: None,
        skills: Vec::new(),
    };

    let fetched = fetch_into_store(home, &spec, on_progress)?;
    item.installed_version = fetched.version;
    for skill in &fetched.skills {
        let entry = SkillEntry {
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: skill.rel.clone(),
            enabled: true,
        };
        item.actual_mode = ensure_entry(home, &fetched.dir, &item.mode, &entry, false)?;
        item.skills.push(entry);
    }
    upsert_item(home, item.clone())?;
    Ok(item)
}

/// Update one package: re-fetch the same source, rescan skills, reconcile
/// the active root — added skills link in enabled, upstream removals unlink,
/// surviving skills refresh when their layout moved or the package runs in
/// copy mode. Reports the diff via progress.
pub fn update(id: &str, on_progress: &mut dyn FnMut(&str)) -> Result<SkillStoreItem, AppError> {
    update_into(&resolve_home(), id, on_progress)
}

fn update_into(
    home: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<SkillStoreItem, AppError> {
    let previous =
        store_item(home, id).ok_or_else(|| AppError::Skill("技能包不在中央库中".into()))?;
    // Local packages stay out of version checks by design, but a manual
    // 「更新」 is their re-sync path after editing the source folder.
    if previous.pinned && previous.origin != "local" {
        return Err(AppError::Skill(format!(
            "{} 已锁定版本 {}，如需升级请卸载后重新安装（不带版本号）",
            previous.name, previous.installed_version
        )));
    }
    let spec = parse_spec(&previous.source)?;
    on_progress(&format!("正在更新 {}", previous.name));
    let fetched = fetch_into_store(home, &spec, on_progress)?;

    let mut updated = previous.clone();
    updated.skills = fetched
        .skills
        .iter()
        .map(|s| SkillEntry {
            name: s.name.clone(),
            description: s.description.clone(),
            path: s.rel.clone(),
            // Surviving skills keep their previous enable state across the
            // update; brand-new skills start enabled.
            enabled: previous
                .skills
                .iter()
                .find(|e| e.name == s.name)
                .map(|e| e.enabled)
                .unwrap_or(true),
        })
        .collect();
    updated.updated_at = now_epoch_secs();

    for old in &previous.skills {
        if updated.skills.iter().any(|e| e.name == old.name) {
            continue;
        }
        unmaterialize_entry(home, &fetched.dir, old);
        on_progress(&format!("上游已移除技能 {}，已从工作台摘除", old.name));
    }
    for entry in &updated.skills {
        let old = previous.skills.iter().find(|e| e.name == entry.name);
        let moved = old.map(|o| o.path != entry.path).unwrap_or(false);
        if !entry.enabled {
            // Stay absent; also clear anything the previous layout linked.
            if let Some(o) = old {
                unmaterialize_entry(home, &fetched.dir, o);
            }
            continue;
        }
        let needs_refresh = old.is_none()
            || moved
            || updated.mode == "copy"
            || !skill_target_path(home, entry).exists();
        if needs_refresh {
            updated.actual_mode = ensure_entry(home, &fetched.dir, &updated.mode, entry, true)?;
        }
    }
    upsert_item(home, updated.clone())?;
    Ok(updated)
}

/// Uninstall one package everywhere: unlink its skills, delete the store
/// tree, drop the inventory row.
pub fn uninstall(id: &str, on_progress: &mut dyn FnMut(&str)) -> Result<(), AppError> {
    uninstall_into(&resolve_home(), id, on_progress)
}

fn uninstall_into(
    home: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let item = store_item(home, id).ok_or_else(|| AppError::Skill("技能包不在中央库中".into()))?;
    let pkg_dir = store_pkg_dir(home, id);
    for entry in &item.skills {
        unmaterialize_entry(home, &pkg_dir, entry);
    }
    let _ = fs::remove_dir_all(&pkg_dir);
    remove_item(home, id)?;
    on_progress(&format!(
        "已卸载 {}（{} 个技能已从工作台摘除）",
        item.name,
        item.skills.len()
    ));
    Ok(())
}

/// Enable or disable one skill of one package: build or remove its
/// active-root entry. The kernel watcher applies the change live.
pub fn set_enabled(
    id: &str,
    skill_name: &str,
    enabled: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    set_enabled_into(&resolve_home(), id, skill_name, enabled, on_progress)
}

fn set_enabled_into(
    home: &Path,
    id: &str,
    skill_name: &str,
    enabled: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let mut item =
        store_item(home, id).ok_or_else(|| AppError::Skill("技能包不在中央库中".into()))?;
    let pkg_dir = store_pkg_dir(home, id);
    let entry = item
        .skills
        .iter_mut()
        .find(|e| e.name == skill_name)
        .ok_or_else(|| AppError::Skill(format!("{id} 中不存在技能 {skill_name:?}")))?;
    if entry.enabled == enabled {
        return Ok(());
    }
    if enabled {
        // Freshen the cached copy of path/description before linking.
        ensure_entry(home, &pkg_dir, &item.mode, entry, false)?;
    } else {
        unmaterialize_entry(home, &pkg_dir, entry);
    }
    entry.enabled = enabled;
    upsert_item(home, item)?;
    on_progress(&format!(
        "技能 {skill_name} 已{}（对运行中的工作台即时生效）",
        if enabled { "启用" } else { "停用" }
    ));
    Ok(())
}

/// Compose the UI status snapshot (no network).
pub fn status() -> SkillStatus {
    status_for_home(&resolve_home())
}

fn status_for_home(home: &Path) -> SkillStatus {
    let store = load_store(home);
    let root = skills_root(home);
    let mut rows = Vec::new();
    let mut updates = 0;
    for item in &store.items {
        let skills = item
            .skills
            .iter()
            .map(|entry| SkillEntryView {
                name: entry.name.clone(),
                description: entry.description.clone(),
                enabled: entry.enabled,
                present: skill_target_path(home, entry).exists(),
            })
            .collect();
        if item
            .latest_version
            .as_deref()
            .map(|l| is_newer_than(l, &item.installed_version, &item.origin, item.pinned))
            .unwrap_or(false)
        {
            updates += 1;
        }
        // Hide a stale "latest" once it is no longer newer than what the
        // user actually has, mirroring the plugin rows' badge behavior.
        let row_latest = item
            .latest_version
            .as_deref()
            .filter(|l| is_newer_than(l, &item.installed_version, &item.origin, item.pinned))
            .map(str::to_string);
        rows.push(SkillRow {
            id: item.id.clone(),
            name: item.name.clone(),
            origin: item.origin.clone(),
            source: item.source.clone(),
            installed_version: item.installed_version.clone(),
            latest_version: row_latest,
            pinned: item.pinned,
            desired_mode: item.mode.clone(),
            actual_mode: item.actual_mode.clone(),
            skills,
            repo_url: item.repo_url.clone(),
            description: item.description.clone(),
            installed_at: item.installed_at.clone(),
            updated_at: item.updated_at.clone(),
        });
    }
    SkillStatus {
        rows,
        skills_root: root.display().to_string(),
        updates,
        last_checked_at: store.last_checked_at,
        warning: store.warning,
    }
}

// --- update checks ----------------------------------------------------------

/// Check every non-local store item against its origin's latest version and
/// persist the results for the UI badge.
pub fn check_updates() -> Result<Vec<SkillUpdateInfo>, AppError> {
    check_updates_for_home(&resolve_home())
}

fn check_updates_for_home(home: &Path) -> Result<Vec<SkillUpdateInfo>, AppError> {
    let mut store = load_store(home);
    let mut out = Vec::new();
    for item in &mut store.items {
        if item.origin == "local" {
            continue;
        }
        let (latest, error) = match item.origin.as_str() {
            "npm" => match fetch_npm_doc(&item.source) {
                Ok(doc) => (doc.dist_tags.get("latest").cloned(), None),
                Err(e) => (None, Some(e)),
            },
            "git" => match git_latest_tag(&item.source) {
                Ok(v) => (v, None),
                Err(e) => (None, Some(e)),
            },
            _ => (None, None),
        };
        let newer =
            latest.filter(|v| is_newer_than(v, &item.installed_version, &item.origin, item.pinned));
        item.latest_version = newer.clone();
        out.push(SkillUpdateInfo {
            id: item.id.clone(),
            latest: newer,
            error,
        });
    }
    store.last_checked_at = Some(now_epoch_secs());
    save_store(home, &store)?;
    Ok(out)
}

// --- reconcile --------------------------------------------------------------

/// Startup repair, safe to run unconditionally:
///
/// 1. Recover staging swaps with the plugin-store rules (final exists → drop
///    staging; else revert to backup, promote new, drop tmp).
/// 2. Ensure the active root exists; re-materialize missing/broken entries
///    for enabled skills; clear lingering entries for disabled skills.
/// 3. Sweep active-root symlinks that point into the store but match no
///    current inventory row (orphans of manual store deletion). Entries the
///    store does not own — plain files, dirs, links pointing elsewhere —
///    are user content and never touched.
///
/// Failures land in `store.warning` for the UI instead of blocking startup.
pub fn reconcile() {
    reconcile_home(&resolve_home());
}

fn reconcile_home(home: &Path) {
    recover_staging(home);

    let store = load_store(home);
    let root = skills_root(home);
    if fs::create_dir_all(&root).is_err() {
        return;
    }

    let mut warning = store.warning.clone();
    let mut owned_targets: std::collections::HashSet<PathBuf> = Default::default();
    for item in &store.items {
        let pkg_dir = store_pkg_dir(home, &item.id);
        for entry in &item.skills {
            let target = skill_target_path(home, entry);
            owned_targets.insert(target.clone());
            let healthy = fs::symlink_metadata(&target)
                .ok()
                .filter(|m| m.file_type().is_symlink())
                .and_then(|_| fs::read_link(&target).ok())
                .map(|link| link == resolved_source(&pkg_dir.join(&entry.path)))
                .unwrap_or(false);
            if entry.enabled {
                if !healthy && pkg_dir.exists() {
                    match ensure_entry(home, &pkg_dir, &item.mode, entry, true) {
                        Ok(_) => warning = None,
                        Err(e) => warning = Some(e.to_string()),
                    }
                }
            } else if !healthy {
                // Disabled skills must stay absent; a lingering entry from an
                // older layout goes away. Foreign content is left untouched.
                unmaterialize_entry(home, &pkg_dir, entry);
            }
        }
    }

    if let Ok(entries) = fs::read_dir(&root) {
        // Compare through canonicalize on both sides: the store may sit
        // under a symlinked path segment (macOS /var → /private/var), and
        // read_link returns whatever form the link was created with.
        let store_canon = store_dir(home)
            .canonicalize()
            .unwrap_or_else(|_| store_dir(home));
        for entry in entries.flatten() {
            let path = entry.path();
            if owned_targets.contains(&path) {
                continue;
            }
            let orphan = fs::symlink_metadata(&path)
                .ok()
                .filter(|m| m.file_type().is_symlink())
                .and_then(|_| fs::read_link(&path).ok())
                .map(|link| {
                    let target = if link.is_absolute() {
                        link
                    } else {
                        root.join(link)
                    };
                    target
                        .canonicalize()
                        .map(|c| c.starts_with(&store_canon))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if orphan {
                remove_link(&path);
            }
        }
    }

    if warning != store.warning {
        let mut next = store;
        next.warning = warning;
        let _ = save_store(home, &next);
    }
}

/// Crash-recovery for staging dirs left behind by an interrupted fetch or
/// update. Grouping uses the stamped `.dsh-id` marker; among competing
/// staging dirs the lexicographically newest wins and older peers are
/// dropped. Recovery prefers reverting to `.backup-*` (the known-good
/// previous tree) when both states survive.
fn recover_staging(home: &Path) {
    let store = store_dir(home);
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
        let kind = if name.starts_with(TMP_PREFIX) {
            Kind::Tmp
        } else if name.starts_with(NEW_PREFIX) {
            Kind::New
        } else if name.starts_with(BACKUP_PREFIX) {
            Kind::Backup
        } else {
            continue;
        };
        let Some(id) = fs::read_to_string(entry.path().join(ID_MARKER))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        by_id.entry(id).or_default().push((kind, entry.path()));
    }

    for (id, mut items) in by_id {
        let final_dir = store.join(&id);
        items.sort_by(|a, b| a.1.file_name().cmp(&b.1.file_name()));

        if final_dir.exists() {
            for (_, path) in items {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }

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

        if let Some(backup) = newest_backup {
            let _ = fs::rename(&backup, &final_dir);
            if let Some(new) = newest_new {
                let _ = fs::remove_dir_all(&new);
            }
        } else if let Some(new) = newest_new {
            let _ = fs::rename(&new, &final_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique throwaway fake-home per test, removed on drop.
    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            let nano = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let base =
                std::env::temp_dir().join(format!("dsh-skills-test-{}-{nano}", std::process::id()));
            fs::create_dir_all(&base).expect("create test home");
            TestHome(base)
        }

        fn root(&self) -> PathBuf {
            self.0.clone()
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_bundle(pkg: &Path, rel: &str, name: &str, description: &str) {
        let dir = pkg.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\nBody.\n"),
        )
        .unwrap();
    }

    #[test]
    fn kebab_case_accepts_only_kernel_valid_names() {
        assert!(is_kebab_case("docx"));
        assert!(is_kebab_case("pdf-a11y"));
        assert!(!is_kebab_case(""));
        assert!(!is_kebab_case("-lead"));
        assert!(!is_kebab_case("trail-"));
        assert!(!is_kebab_case("double--dash"));
        assert!(!is_kebab_case("Upper"));
        assert!(!is_kebab_case("with space"));
    }

    #[test]
    fn frontmatter_parses_name_and_description() {
        let text =
            "---\nname: my-skill\ndescription: \"Does things: well\"\nlicense: MIT\n---\nBody";
        assert_eq!(
            parse_skill_markdown(text),
            Some(("my-skill".into(), "Does things: well".into()))
        );
        assert_eq!(parse_skill_markdown("no frontmatter"), None);
        assert_eq!(parse_skill_markdown("---\nname: x\n---\n"), None);
    }

    #[test]
    fn spec_parses_npm_git_and_rejects_bad_local() {
        let npm = parse_spec("@scope/pkg@1.2.3").unwrap();
        assert_eq!(npm.origin, "npm");
        assert_eq!(npm.id, "@scope__pkg");
        assert_eq!(npm.pin.as_deref(), Some("1.2.3"));

        let git = parse_spec("owner/repo").unwrap();
        assert_eq!(git.origin, "git");
        assert_eq!(git.source, "https://github.com/owner/repo.git");

        assert!(parse_spec("/nonexistent/path/xyz").is_err());
    }

    #[test]
    fn scan_finds_bundles_and_flat_files_but_skips_readme() {
        let home = TestHome::new();
        let pkg = home.root().join("pkg");
        write_bundle(&pkg, "skills/deep", "deep-one", "nested bundle");
        fs::write(pkg.join("README.md"), "# readme without frontmatter\n").unwrap();

        let found = scan_package_skills(&pkg, &mut |_| {}).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "deep-one");
        assert_eq!(found[0].rel.replace('\\', "/"), "skills/deep");

        // A flat root file WITH frontmatter counts; both survive, sorted.
        fs::write(
            pkg.join("flat.md"),
            "---\nname: flat-one\ndescription: flat\n---\n",
        )
        .unwrap();
        let found = scan_package_skills(&pkg, &mut |_| {}).unwrap();
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["deep-one", "flat-one"]);
    }

    #[test]
    fn scan_warns_on_invalid_frontmatter_and_rejects_duplicates_and_empty() {
        let home = TestHome::new();
        let pkg = home.root().join("pkg");
        write_bundle(&pkg, "good", "good-one", "fine");
        // Invalid candidate the kernel would ignore too.
        fs::create_dir_all(pkg.join("bad")).unwrap();
        fs::write(pkg.join("bad/SKILL.md"), "---\ntitle: no name here\n---\n").unwrap();
        let mut warnings = Vec::new();
        let found = scan_package_skills(&pkg, &mut |m| warnings.push(m.to_string())).unwrap();
        assert_eq!(found.len(), 1);
        assert!(warnings.iter().any(|w| w.contains("跳过 bad")));

        let dup = home.root().join("dup");
        write_bundle(&dup, "a", "same-name", "dup");
        write_bundle(&dup, "b", "same-name", "dup");
        let err = scan_package_skills(&dup, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("重名"));

        let empty = home.root().join("empty-pkg");
        fs::create_dir_all(&empty).unwrap();
        fs::write(empty.join("README.md"), "# nothing\n").unwrap();
        let err = scan_package_skills(&empty, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("没有发现可用技能"));
    }

    #[test]
    fn install_materializes_enables_disables_and_uninstalls() {
        let home = TestHome::new();
        let src = home.root().join("my-source-pack");
        write_bundle(&src, "alpha", "alpha-skill", "first");
        write_bundle(&src, "beta", "beta-skill", "second");

        let spec_str = src.to_string_lossy().to_string();
        let item =
            install_into(&home.root(), &spec_str, "link", &mut |_| {}).expect("install succeeds");
        assert_eq!(item.skills.len(), 2);
        assert!(item.pinned);
        assert_eq!(item.installed_version, "local");

        let alpha = skills_root(&home.root()).join("alpha-skill");
        let beta = skills_root(&home.root()).join("beta-skill");
        assert!(alpha.is_dir());
        assert!(beta.is_dir());

        set_enabled_into(&home.root(), &item.id, "beta-skill", false, &mut |_| {}).unwrap();
        assert!(!beta.exists());
        assert!(alpha.exists());

        set_enabled_into(&home.root(), &item.id, "beta-skill", true, &mut |_| {}).unwrap();
        assert!(beta.exists());

        // Status reflects presence truth from disk.
        let view = status_for_home(&home.root());
        assert_eq!(view.rows.len(), 1);
        let entry = view.rows[0]
            .skills
            .iter()
            .find(|s| s.name == "beta-skill")
            .unwrap();
        assert!(entry.enabled && entry.present);

        uninstall_into(&home.root(), &item.id, &mut |_| {}).unwrap();
        assert!(!alpha.exists());
        assert!(!beta.exists());
        assert!(status_for_home(&home.root()).rows.is_empty());
    }

    #[test]
    fn install_rejects_conflicting_existing_entry() {
        let home = TestHome::new();
        let src = home.root().join("pack");
        write_bundle(&src, "one", "clash-skill", "x");
        // Pre-occupy the active root with foreign content.
        fs::create_dir_all(skills_root(&home.root()).join("clash-skill")).unwrap();
        let err =
            install_into(&home.root(), &src.to_string_lossy(), "link", &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("冲突"));
    }

    #[test]
    fn reconcile_repairs_missing_entries_and_sweeps_orphan_links() {
        let home = TestHome::new();
        let src = home.root().join("pack");
        write_bundle(&src, "one", "fix-me", "r");
        // `_item` is only read by the unix-only orphan-link block below.
        let _item =
            install_into(&home.root(), &src.to_string_lossy(), "link", &mut |_| {}).unwrap();

        // Simulate external breakage: the link disappeared.
        let target = skills_root(&home.root()).join("fix-me");
        remove_target(&target);
        assert!(!target.exists());
        reconcile_home(&home.root());
        assert!(target.exists());

        // An orphan link pointing into the store but absent from the store
        // inventory gets swept; user files stay untouched.
        #[cfg(unix)]
        {
            let ghost_src = store_pkg_dir(&home.root(), &_item.id).join("one");
            std::os::unix::fs::symlink(ghost_src, skills_root(&home.root()).join("ghost")).unwrap();
        }
        let user_file = skills_root(&home.root()).join("mine.md");
        fs::write(&user_file, "---\nname: mine\ndescription: keep\n---\n").unwrap();
        reconcile_home(&home.root());
        #[cfg(unix)]
        assert!(!skills_root(&home.root()).join("ghost").exists());
        assert!(user_file.exists());
        assert!(target.exists());
    }

    #[test]
    fn update_refreshes_layout_changes_while_keeping_disabled_state() {
        let home = TestHome::new();
        let src = home.root().join("pack");
        write_bundle(&src, "keep", "keep-skill", "k");
        write_bundle(&src, "drop", "drop-skill", "d");
        write_bundle(&src, "move", "move-skill", "m");
        let item = install_into(&home.root(), &src.to_string_lossy(), "link", &mut |_| {}).unwrap();

        // Disable one survivor before the upstream change.
        set_enabled_into(&home.root(), &item.id, "keep-skill", false, &mut |_| {}).unwrap();

        // Upstream: move `move`, drop `drop`, keep `keep`, add `added`.
        fs::remove_dir_all(src.join("move")).unwrap();
        write_bundle(&src, "nested/move", "move-skill", "m");
        fs::remove_dir_all(src.join("drop")).unwrap();
        write_bundle(&src, "added", "added-skill", "a");

        let updated = update_into(&home.root(), &item.id, &mut |_| {}).unwrap();
        let names: Vec<&str> = updated.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["added-skill", "keep-skill", "move-skill"]);
        assert!(!skills_root(&home.root()).join("drop-skill").exists());
        // Disabled state survives the update; the entry stays absent.
        let kept = updated
            .skills
            .iter()
            .find(|s| s.name == "keep-skill")
            .unwrap();
        assert!(!kept.enabled);
        assert!(!skills_root(&home.root()).join("keep-skill").exists());
        // Moved skill was relinked at the same public name.
        assert!(skills_root(&home.root()).join("move-skill").exists());
        // New skill arrived enabled.
        assert!(skills_root(&home.root()).join("added-skill").exists());
    }

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlinked_skill_md_to_avoid_double_counting_redirects() {
        // Reproduce the blader/humanizer v2.11.1 packaging quirk: a real
        // SKILL.md at the root plus a symlinked copy under `skills/<name>/`.
        // Without the symlink-aware scan, the redirect doubles the skill and
        // the duplicate-name check kills the install.
        let home = TestHome::new();
        let pkg = home.root().join("pkg");
        fs::create_dir_all(pkg.join("skills/humanizer")).unwrap();
        fs::write(
            pkg.join("SKILL.md"),
            "---\nname: humanizer\ndescription: |\n  Rewrite text that sounds AI.\n---\n",
        )
        .unwrap();
        std::os::unix::fs::symlink("../../SKILL.md", pkg.join("skills/humanizer/SKILL.md"))
            .unwrap();

        let found = scan_package_skills(&pkg, &mut |_| {}).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "humanizer");
        assert_eq!(found[0].rel, "SKILL.md");
    }
}
