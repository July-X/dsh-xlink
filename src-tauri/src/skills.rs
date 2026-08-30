//! 社区技能管理：在 dsh home 下维护中央库，将单个技能落地到 kernel 的用户级
//! skill 根目录，并提供更新检查。
//!
//! 技能是纯指令数据（一个 `SKILL.md` 目录包或一个带 YAML frontmatter 的扁平
//! Markdown 文件），不是代码：无需构建，也不涉及 profile 配置。
//! kernel 的 `dsh-skill-filesystem` provider 直接扫描 `<DSH_HOME>/skills`
//!（user-dsh 根目录）并用 chokidar 监听，外壳链接到该根目录的技能会被每一个
//! 已安装的 kernel 版本实时发现——无需按 kernel 版本逐一落地，也无需重启。
//! 安装单位是包（npm tarball、git 仓库、本地文件夹）；落地与启用/停用的
//! 粒度是单个技能。
//!
//! 设计说明：桌面交付物的 docs/skill-management.md。

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::process::{atomic_write, run_capture};
use crate::releases::{http_get_file, http_get_npm_latest, http_get_string};
use crate::version::cmp_versions;

/// dsh home 下的中央库目录名。位于 kernel 读取的 `<home>/skills/` 根目录旁，
/// 但归外壳所有：被停用的技能与来源标记绝不能出现在 kernel 的发现范围内。
const STORE_SUBDIR: &str = "skills-store";
/// 外壳的清单文件，位于中央库目录内。
const STORE_FILE: &str = "store.json";
/// 每个中央库条目内的单包获取标记。
const SOURCE_MARKER: &str = ".dsh-source.json";
/// 技能目录包被识别的最大目录深度（相对包根目录，0 层子目录即深度 0）。
/// 覆盖根目录的目录包以及常见的 monorepo 布局（`skills/<name>/SKILL.md`），
/// 而不必遍历整棵树。
const SCAN_MAX_DEPTH: usize = 3;
/// 用户可写的 spec 前缀，用于强制按本地文件夹解析。
const LOCAL_PREFIX: &str = "local:";

// --- 数据模型 ------------------------------------------------------------

/// 在已安装的包中发现的一个技能。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    /// frontmatter 中的名字，kebab-case；同时也是活动根中的条目名。
    pub name: String,
    pub description: String,
    /// 相对包根的路径（使用 `/` 分隔），指向目录包目录或扁平 `.md` 文件。
    /// 对 UI 不透明；相对包目录解析。
    pub path: String,
    /// 技能此刻是否已链接到活动根。
    pub enabled: bool,
}

/// 中央库中的一个已安装技能包。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStoreItem {
    /// 文件系统安全的包标识（包/仓库/文件夹名）。
    pub id: String,
    /// 显示名（npm 包名、仓库简写或文件夹名）。
    pub name: String,
    /// 获取来源：npm、git 或 local。
    pub origin: String,
    /// npm spec / git URL / 本地文件夹的绝对路径。
    pub source: String,
    pub installed_version: String,
    /// 已知的最新版本，由 check_updates 刷新；local 来源永不设置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// 期望的落地模式：link 或 copy。
    pub mode: String,
    /// 实际在磁盘上使用的模式（可能因回退而不同）。
    pub actual_mode: String,
    /// 来源是否锁定版本（npm @version / git #tag）；本地文件夹始终为 true。
    pub pinned: bool,
    /// 自 epoch 起的秒数，用于展示。
    pub installed_at: String,
    /// 最近一次获取时自 epoch 起的秒数，用于展示。
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 在安装/更新时此包内发现的技能。
    pub skills: Vec<SkillEntry>,
}

/// 持久化的中央库文档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStore {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub items: Vec<SkillStoreItem>,
    #[serde(rename = "lastCheckedAt", skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    /// 最近一次 reconcile/materialize 失败，向 UI 暴露的信息（如有）。
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

/// 在一个包行内渲染的一条技能记录。
#[derive(Debug, Clone, Serialize)]
pub struct SkillEntryView {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// 期望的条目是否实际存在于活动根中。
    pub present: bool,
}

/// 管理 UI 渲染的一行（对应一个已安装的包）。
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

/// 管理 UI 的技能聚合状态。
#[derive(Debug, Clone, Serialize)]
pub struct SkillStatus {
    pub rows: Vec<SkillRow>,
    /// kernel 读取的用户技能根的展示路径。
    pub skills_root: String,
    /// 已知存在更新版本的包数量。
    pub updates: usize,
    pub last_checked_at: Option<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SkillSpec {
    pub origin: String,
    /// npm 包名、git URL 或本地文件夹的绝对路径。
    pub source: String,
    /// 可选的版本锁定（npm semver）或 git tag。
    pub pin: Option<String>,
    /// 文件系统安全的中央库 id。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// git 来源下用户可见的仓库 URL。
    pub repo_url: Option<String>,
}

/// 解析出的单个包更新检查结果。
#[derive(Debug, Clone, Serialize)]
pub struct SkillUpdateInfo {
    pub id: String,
    pub latest: Option<String>,
    pub error: Option<String>,
}

// --- 路径 ------------------------------------------------------------------

/// 用户的操作系统主目录（Unix 上的 `$HOME`，Windows 上的 `%USERPROFILE%`），
/// 与 kernel.rs 自身的回退链保持一致。
fn os_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 将开头的 `~` 组件展开为操作系统主目录。`DSH_HOME="~/…"` 仍受支持，
/// 与 kernel 的 `resolveDshHome` 中波浪号展开行为一致。
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

/// 被启动的 kernel 自行解析出的 dsh home（`DSH_HOME` 环境变量或 `~/.dsh`）。
/// 外壳必须写入 kernel 实际读取的那个目录，因此这里镜像 kernel 的解析顺序，
/// 而不从外壳的数据目录派生（`DSH_DESKTOP_DATA_DIR` 可能将其重定向到别处）。
fn resolve_home() -> PathBuf {
    expand_tilde(
        &std::env::var_os("DSH_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| os_home().join(".dsh")),
    )
}

/// 中央库根目录：`<home>/skills-store/`。
pub fn store_dir(home: &Path) -> PathBuf {
    home.join(STORE_SUBDIR)
}

fn store_file(home: &Path) -> PathBuf {
    store_dir(home).join(STORE_FILE)
}

fn store_pkg_dir(home: &Path, id: &str) -> PathBuf {
    store_dir(home).join(id)
}

/// kernel 读取的用户技能根（`<home>/skills/`，user-dsh rank 400）：
/// 所有已安装 kernel 共享的单一落地目标。
pub fn skills_root(home: &Path) -> PathBuf {
    home.join("skills")
}

/// 单个技能的活动根条目：目录包以其 frontmatter 名建立链接；
/// 扁平文件则变为 `<name>.md`，使条目读起来就像技能本身。
fn skill_target_path(home: &Path, entry: &SkillEntry) -> PathBuf {
    if entry.path.ends_with(".md") {
        skills_root(home).join(format!("{}.md", entry.name))
    } else {
        skills_root(home).join(&entry.name)
    }
}

/// 将包/仓库/文件夹名映射为文件系统安全的中央库 id。规则与插件中央库一致：
/// 斜杠变为双下划线；空段或 `.`/`..` 段直接拒绝。
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

// --- 中央库持久化 -----------------------------------------------------------

fn store_mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 锁定技能商店的清单变更。网络更新检查只在最终提交阶段持有这把锁，
/// 不让慢速来源请求阻塞用户的技能安装或启停操作。
pub fn lock_store() -> std::sync::MutexGuard<'static, ()> {
    crate::lock(store_mutation_lock())
}

pub fn load_store(home: &Path) -> SkillStore {
    let Ok(text) = fs::read_to_string(store_file(home)) else {
        return SkillStore::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_store_unlocked(home: &Path, store: &SkillStore) -> Result<(), AppError> {
    fs::create_dir_all(store_dir(home)).map_err(|e| AppError::Io(e.to_string()))?;
    let text = serde_json::to_string_pretty(store).map_err(|e| AppError::Io(e.to_string()))?;
    atomic_write(&store_file(home), format!("{text}\n").as_bytes())
        .map_err(|e| AppError::Io(e.to_string()))
}

fn store_item(home: &Path, id: &str) -> Option<SkillStoreItem> {
    load_store(home)
        .items
        .into_iter()
        .find(|item| item.id == id)
}

fn upsert_item_unlocked(home: &Path, item: SkillStoreItem) -> Result<(), AppError> {
    let mut store = load_store(home);
    if let Some(existing) = store.items.iter_mut().find(|i| i.id == item.id) {
        *existing = item;
    } else {
        store.items.push(item);
    }
    save_store_unlocked(home, &store)
}

fn remove_item_unlocked(home: &Path, id: &str) -> Result<(), AppError> {
    let mut store = load_store(home);
    store.items.retain(|item| item.id != id);
    save_store_unlocked(home, &store)
}

// --- spec 解析 ------------------------------------------------------------

/// 将 npm spec 拆分为 (name, 可选 pin)。规则与插件中央库一致：
/// scope 前缀之后的最后一个 @ 用作版本分隔符。
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

/// 判断输入是否为本地文件夹：显式前缀、波浪号形式、绝对 POSIX 路径、
/// `.` 相对路径或 Windows 盘符路径。`\\?\` verbatim 前缀同样被识别：
/// `parse_local_spec` 会把规范化路径作为包来源存储，而 Windows 上的
/// `canonicalize` 返回 verbatim 路径——少了这条分支，本地包的「更新」重新
/// 解析会落到 npm 名称校验逻辑并失败。
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

/// 将安装请求解析为 SkillSpec。接受 npm 包名（可带 @version）、git URL
/// （https、git@ 或 owner/repo 简写，可选 #tag）以及本地文件夹路径
/// （`local:` 前缀、`~/…`、绝对路径或 Windows 盘符路径）。
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

// --- frontmatter -----------------------------------------------------------

/// 判断 `name` 是否匹配 kernel 的 kebab-case 技能命名规则
/// （`^[a-z0-9]+(?:-[a-z0-9]+)*$`）。
fn is_kebab_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

/// 从技能文件开头的 YAML frontmatter 中抽取 `(name, description)`。
/// 这里是有意只解析顶层子集——安装一个外壳自身无法校验的技能会让用户在
/// 不知情的情况下看到一个不可见的技能，因此无法解析的 frontmatter
/// 会直接拒绝该候选，而不是信任它。
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
        // 只接受顶层键：跳过嵌套映射、列表、块字符串。
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

// --- 包扫描 ---------------------------------------------------------------

/// 在包内发现的一个已校验技能。
#[derive(Debug, Clone)]
struct ScannedSkill {
    name: String,
    description: String,
    /// 相对包根的路径，使用 `/` 分隔，指向目录包目录（包含 SKILL.md）
    /// 或扁平 `.md` 文件。
    rel: String,
}

/// 扫描已获取的包以发现技能。包含 SKILL.md 的目录在 [`SCAN_MAX_DEPTH`]
/// 层数内被视为目录包候选；根目录下的扁平 `*.md` 文件也算候选。
/// kernel 会忽略的候选（缺少/无效 frontmatter、非 kebab 命名）通过
/// `warn` 暴露并跳过；扫描若没有产生任何可用技能，则视为错误。
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
    // 同一个包内 frontmatter 名字重复，会在落地时产生歧义
    // （活动根中每个名字只能有一个条目），因此直接报错。
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
        // 完全跳过符号链接：技能目录包应当是真实落盘的目录树，外壳随后落地
        // 时的 link/copy 目标就是条目本身。包内的符号链接（例如 blader/humanizer
        // v2.11.1 为 Claude Desktop 的 ZIP 上传增加的
        // `skills/humanizer/SKILL.md` → `../../SKILL.md`）会重复计入同一
        // 技能。跟随符号链接还可能越过扫描深度限制，落到包根之外。
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
            // 感知符号链接：包含符号链接 SKILL.md 的目录不算目录包，
            // 尽管 `is_file()` 会跟随该链接。
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
                // 目录包独占其子树，不再下钻。
                continue;
            }
            if depth + 1 < SCAN_MAX_DEPTH {
                walk_package(root, &path, depth + 1, found, warn)?;
            }
        } else if depth == 0 && file_name.to_ascii_lowercase().ends_with(".md") {
            // 扁平候选。根目录下的 README.md 等文件没有技能 frontmatter，
            // kernel 同样会忽略——此处静默跳过。
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

// --- 获取 -----------------------------------------------------------------

/// 候选 tag 中形如 semver 的最高版本，若无则返回 None。
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

/// 判断已存储的版本字符串看上去像 semver 而不是短哈希；之所以先按形状
/// 拆分比较，详见 plugins.rs 的 `looks_like_semver`。
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

/// 获取与更新检查所需的 npm registry 文档片段。
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

/// 将 npm tgz 解包到 `dest`，并去掉其开头的 `package/` 段。
/// 共享的 Rust 解包器会拒绝路径穿越、链接和特殊文件，并限制条目数量与
/// 声明的展开后内容，然后再发布。
fn extract_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    crate::archive::extract_gzip_tarball(tarball, dest)
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
    atomic_write(&dest.join(SOURCE_MARKER), format!("{text}\n").as_bytes())
        .map_err(|e| AppError::Io(e.to_string()))
}

/// 递归地将 source 复制到 target，若已存在则替换。
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

/// 暂存获取目录的前缀，镜像插件中央库的防崩溃交换术语：
/// `.tmp-*` 表示获取中，`.new-*` 表示已校验，`.backup-*` 表示交换期间
/// 先前活跃的目录树。前导的点让暂存目录不出现在任何扫描器与文件列表中。
const TMP_PREFIX: &str = ".tmp-";
const NEW_PREFIX: &str = ".new-";
const BACKUP_PREFIX: &str = ".backup-";
/// 在暂存目录中标记所属包 id 的标记文件。
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
    atomic_write(&dir.join(ID_MARKER), format!("{id}\n").as_bytes())
}

/// 单次成功的「获取并发布」周期的结果。
struct FetchedPackage {
    /// 该包发布后的中央库目录。
    dir: PathBuf,
    /// 记录在来源标记中的版本字符串（npm semver、git tag 或 HEAD 哈希，
    /// 文件夹导入则为 `local`）。
    version: String,
    /// 在包内发现并通过校验的技能。
    skills: Vec<ScannedSkill>,
}

/// 将一个包获取到中央库下某个 `.tmp-*` 暂存目录中，扫描其中的技能，
/// 再用插件中央库确立的三阶段原子改名覆盖到正式目录。任何一步崩溃，
/// 都可通过 [`reconcile`] 恢复已存在的包。
fn fetch_into_store(
    home: &Path,
    spec: &SkillSpec,
    on_progress: &mut dyn FnMut(&str),
) -> Result<FetchedPackage, AppError> {
    let store = store_dir(home);
    fs::create_dir_all(&store).map_err(|e| AppError::Io(e.to_string()))?;
    let tmp = new_staging_dir(&store, TMP_PREFIX).map_err(|e| AppError::Io(e.to_string()))?;
    // `.dsh-id` 标记必须在 fetch_* 调用返回之后再写入：`git clone` 要求目标
    // 目录为空，若获取时该标记已存在，就会报 "destination path '...' already
    // exists and is not an empty directory" 失败；`npm` 的 tarball 解包与
    // `local` 复制即便标记存在也会直接覆盖，所以预先写标记过去只是 npm
    // 场景下的巧合。我们把 `tmp → new` 改名时标记会跟着内容一起过去，
    // 因此先 fetch 再写标记，让改名把它顺带带到新的暂存路径。

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
        // tmp 中仍保留已扫描的内容，留给 reconcile 兜底。
        return Err(AppError::Io(format!("将暂存目录提升到 .new-* 失败：{e}")));
    }

    let final_dir = store_pkg_dir(home, &spec.id);
    let backup = new_staging_dir(&store, BACKUP_PREFIX).map_err(|e| AppError::Io(e.to_string()))?;
    if final_dir.exists() {
        if let Err(e) = fs::rename(&final_dir, &backup) {
            // 前向回滚：直接将已校验的目录推到正式位置，而不是让它孤立。
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
                "dsh-xlink: warning, could not stamp id marker on backup of {}: {e}",
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
    let tgz = dest.join(".pkg.tgz");
    http_get_file(&tarball, &tgz).map_err(|e| AppError::Skill(format!("下载失败：{e}")))?;
    // 通过共享的 Rust 归档处理器解包；它会校验 npm 的 `package/` 根目录，
    // 并将清单发布到 `dest` 供后续扫描。
    extract_tarball(&tgz, dest)
        .map_err(|e| AppError::Skill(format!("解包失败：{e}（请确认下载内容完整后重试）")))?;
    let _ = fs::remove_file(&tgz);
    Ok(version)
}

fn fetch_git(
    spec: &SkillSpec,
    dest: &Path,
    on_progress: &mut dyn FnMut(&str),
) -> Result<String, AppError> {
    if !matches!(run_capture("git", &["--version"]), Ok((true, _))) {
        return Err(AppError::Skill(
            "未找到 git（git 来源的技能包需要 git；请先安装 git）".into(),
        ));
    }
    // 锁定版本的 spec 直接使用其 tag；未锁定的仓库安装最高的 semver tag，
    // 以便 `installed_version` 可被 check_updates 正确比较。
    // 没有任何 semver tag 的仓库回退到默认分支（HEAD 哈希）。
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
    cmd.arg(&spec.source).arg(dest);
    let (success, _stdout, stderr) = crate::process::run_command_capture(cmd, "git clone")
        .map_err(|e| AppError::Io(format!("无法运行 git：{e}")))?;
    if !success {
        let detail = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim();
        return Err(AppError::Skill(if detail.is_empty() {
            "git clone 失败，请检查地址与网络".to_string()
        } else {
            format!("git clone 失败：{detail}")
        }));
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
    // 复制进来的 .git 目录在这里毫无用处，还会撑大中央库。
    let _ = fs::remove_dir_all(dest.join(".git"));
    Ok(String::from("local"))
}

// --- 落地 -----------------------------------------------------------------

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

/// 解析中央库一侧的链接来源：把符号链接的中央库条目展开到真实位置，
/// 让活动根的链接保持直接（避免出现双重符号链接链）。
fn resolved_source(path: &Path) -> PathBuf {
    fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.file_type().is_symlink())
        .and_then(|_| fs::read_link(path).ok())
        .unwrap_or_else(|| path.to_path_buf())
}

/// 删除一个文件系统链接而不触碰其目标。在 Windows 上 `DeleteFile` 会拒绝
/// 目录符号链接（ERROR_ACCESS_DENIED）——只有 `RemoveDirectory` 才能删除
/// 它们——而文件符号链接需要 `DeleteFile`；两者都试一遍，就能在任意平台上
/// 覆盖两种链接类型。
fn remove_link(path: &Path) {
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}

/// 删除某个活动根条目，无论是链接、目录还是文件。
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

/// 将单个技能以链接（或复制）形式落到活动根中，名称使用 frontmatter 名。
///
/// 若已有条目正好指向同一来源，则短路返回（重复运行幂等）。否则，
/// 若目标位置已被占用，未传 `replace_owned` 时报错；拥有该名字清单
/// 权限的调用方（更新刷新、reconcile 修复）会传入该参数，以替换先前
/// 版本留下的陈旧内容。返回 link→copy 回退后实际使用的模式。
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
            "dsh-xlink: link failed for skill {}; falling back to copy",
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

/// 移除某个技能的活动根条目，但仅当它仍是中央库拥有的链接时执行；
/// 被替换过或用户重新创建的条目保持不动。
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

// --- 编排 -----------------------------------------------------------------

/// 安装一个技能包：获取到中央库，扫描并校验其技能，发布，然后将所有
/// 技能落地到活动根。kernel 的 watcher 会实时发现新条目——无需重启。
pub fn install(
    spec_str: &str,
    mode: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<SkillStoreItem, AppError> {
    let _store_guard = lock_store();
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
    upsert_item_unlocked(home, item.clone())?;
    Ok(item)
}

/// 更新一个包：重新拉取同一来源，重新扫描技能，协调活动根——新增的技能
/// 以启用状态链接进去，上游移除的技能解除链接，保留的技能在布局变动或
/// 包以 copy 模式运行时刷新。通过进度回调报告差异。
pub fn update(id: &str, on_progress: &mut dyn FnMut(&str)) -> Result<SkillStoreItem, AppError> {
    let _store_guard = lock_store();
    update_into(&resolve_home(), id, on_progress)
}

fn update_into(
    home: &Path,
    id: &str,
    on_progress: &mut dyn FnMut(&str),
) -> Result<SkillStoreItem, AppError> {
    let previous =
        store_item(home, id).ok_or_else(|| AppError::Skill("技能包不在中央库中".into()))?;
    // 本地包按设计不参与版本检查，但手动「更新」是编辑源文件夹后的
    // 重新同步途径。
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
            // 保留的技能在更新后保持之前的启用状态；全新添加的技能默认启用。
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
            // 保持缺席；同时清理上一版布局留下的链接。
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
    upsert_item_unlocked(home, updated.clone())?;
    Ok(updated)
}

/// 在所有位置卸载一个包：解除其技能的链接，删除中央库目录树，
/// 删除清单行。
pub fn uninstall(id: &str, on_progress: &mut dyn FnMut(&str)) -> Result<(), AppError> {
    let _store_guard = lock_store();
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
    remove_item_unlocked(home, id)?;
    on_progress(&format!(
        "已卸载 {}（{} 个技能已从工作台摘除）",
        item.name,
        item.skills.len()
    ));
    Ok(())
}

/// 启用或停用一个包中的某个技能：在活动根中建立或移除其条目。
/// kernel watcher 会实时应用此变更。
pub fn set_enabled(
    id: &str,
    skill_name: &str,
    enabled: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<(), AppError> {
    let _store_guard = lock_store();
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
        // 在链接之前刷新 path/description 的缓存副本。
        ensure_entry(home, &pkg_dir, &item.mode, entry, false)?;
    } else {
        unmaterialize_entry(home, &pkg_dir, entry);
    }
    entry.enabled = enabled;
    upsert_item_unlocked(home, item)?;
    on_progress(&format!(
        "技能 {skill_name} 已{}（对运行中的工作台即时生效）",
        if enabled { "启用" } else { "停用" }
    ));
    Ok(())
}

/// 组装 UI 状态快照（不发起网络请求）。
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
        // 当陈旧的 "latest" 已不再比用户实际安装的版本更新时隐藏它，
        // 与插件行的徽标行为保持一致。
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

// --- 更新检查 --------------------------------------------------------------

/// 对每个非 local 的中央库条目与对应来源的最新版本做比对，
/// 并将结果持久化，供 UI 徽标展示。
pub fn check_updates() -> Result<Vec<SkillUpdateInfo>, AppError> {
    check_updates_for_home(&resolve_home())
}

fn check_updates_for_home(home: &Path) -> Result<Vec<SkillUpdateInfo>, AppError> {
    // 网络请求不能持有商店锁，否则一次远端超时会阻塞安装、卸载和启停。
    // 提交时重新读取当前清单，并按 installed_version 做乐观冲突校验，
    // 避免旧的检查结果覆盖刚完成的更新。
    let snapshot = {
        let _store_guard = lock_store();
        load_store(home)
            .items
            .into_iter()
            .filter(|item| item.origin != "local")
            .collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    let mut probes = Vec::with_capacity(snapshot.len());
    for item in snapshot {
        let (latest, error) = match item.origin.as_str() {
            "npm" => match http_get_npm_latest(&item.source) {
                Ok(latest) => (latest, None),
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
        probes.push((
            item.id.clone(),
            item.installed_version.clone(),
            newer.clone(),
        ));
        out.push(SkillUpdateInfo {
            id: item.id.clone(),
            latest: newer,
            error,
        });
    }

    let _store_guard = lock_store();
    let mut store = load_store(home);
    for (id, installed_version, latest) in probes {
        if let Some(current) = store.items.iter_mut().find(|item| item.id == id) {
            if current.installed_version == installed_version {
                current.latest_version = latest;
            }
        }
    }
    store.last_checked_at = Some(now_epoch_secs());
    save_store_unlocked(home, &store)?;
    Ok(out)
}

// --- 修复 -----------------------------------------------------------------

/// 启动期修复流程，可无条件运行：
///
/// 1. 按插件中央库的规则恢复暂存目录交换（final 存在则丢弃暂存；
///    否则回退到 backup、提升 new、丢弃 tmp）。
/// 2. 确保活动根存在；为启用技能重新落地缺失/损坏的条目；
///    清理被停用技能遗留的条目。
/// 3. 清理活动根中指向中央库、但当前清单行已不存在的符号链接
///    （手动删除中央库后留下的孤儿）。中央库不拥有的条目——普通文件、
///    目录、指向别处的链接——属于用户内容，永远不会被触碰。
///
/// 失败时写入 `store.warning` 供 UI 展示，而非阻塞启动。
pub fn reconcile() {
    let _store_guard = lock_store();
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
                // 停用的技能必须保持缺席；旧布局遗留的条目会被清掉。
                // 外来内容保持不动。
                unmaterialize_entry(home, &pkg_dir, entry);
            }
        }
    }

    if let Ok(entries) = fs::read_dir(&root) {
        // 两侧都用 canonicalize 后再比较：中央库可能位于符号链接路径段
        // 之下（macOS /var → /private/var），而 read_link 返回的是
        // 创建链接时所用的形态。
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
        let _ = save_store_unlocked(home, &next);
    }
}

/// 为中断的获取或更新所留下的暂存目录做崩溃恢复。分组依据是写入的
/// `.dsh-id` 标记；在相互竞争的暂存目录中，字典序最大的获胜，其余
/// 旧目录会被丢弃。当 `.backup-*` 和 `.new-*` 都幸存时，恢复优先
/// 回退到 `.backup-*`（已知的上一次可用目录树）。
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_HOME_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// 每个测试一个唯一、一次性的假 home，drop 时清理。
    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            let nano = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = TEST_HOME_COUNTER.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "dsh-skills-test-{}-{nano}-{seq}",
                std::process::id()
            ));
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

        // 带 frontmatter 的根目录扁平文件会被计入；两者都保留并按顺序排列。
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
        // kernel 同样会忽略的无效候选。
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

        // 状态反映磁盘上真实的存在性。
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
        // 预先占用活动根，放置外部内容。
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
        // `_item` 仅被下方 unix 专有的孤儿链接代码块读取。
        let _item =
            install_into(&home.root(), &src.to_string_lossy(), "link", &mut |_| {}).unwrap();

        // 模拟外部破坏：链接消失了。
        let target = skills_root(&home.root()).join("fix-me");
        remove_target(&target);
        assert!(!target.exists());
        reconcile_home(&home.root());
        assert!(target.exists());

        // 指向中央库、但中央库清单中已不存在的孤儿链接会被清理；
        // 用户文件保持不动。
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

        // 在上游变动之前停用一个保留技能。
        set_enabled_into(&home.root(), &item.id, "keep-skill", false, &mut |_| {}).unwrap();

        // 上游变更：移动 `move`、删除 `drop`、保留 `keep`、新增 `added`。
        fs::remove_dir_all(src.join("move")).unwrap();
        write_bundle(&src, "nested/move", "move-skill", "m");
        fs::remove_dir_all(src.join("drop")).unwrap();
        write_bundle(&src, "added", "added-skill", "a");

        let updated = update_into(&home.root(), &item.id, &mut |_| {}).unwrap();
        let names: Vec<&str> = updated.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["added-skill", "keep-skill", "move-skill"]);
        assert!(!skills_root(&home.root()).join("drop-skill").exists());
        // 停用状态在更新后保留；对应条目仍缺席。
        let kept = updated
            .skills
            .iter()
            .find(|s| s.name == "keep-skill")
            .unwrap();
        assert!(!kept.enabled);
        assert!(!skills_root(&home.root()).join("keep-skill").exists());
        // 被移动的技能在同一个公共名上重新建立链接。
        assert!(skills_root(&home.root()).join("move-skill").exists());
        // 新技能以启用状态加入。
        assert!(skills_root(&home.root()).join("added-skill").exists());
    }

    #[cfg(unix)]
    #[test]
    fn scan_skips_symlinked_skill_md_to_avoid_double_counting_redirects() {
        // 复现 blader/humanizer v2.11.1 的打包怪癖：根目录下有一份真实
        // SKILL.md，`skills/<name>/` 下还放了一份符号链接副本。
        // 若扫描不感知符号链接，重定向会导致同一技能被重复计入，
        // 名字重复检查会让安装失败。
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
